use std::{
    fs,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{self, Command},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use super::{automation_worker_required, runtime_settings, RuntimeHandle, RuntimeStatusSnapshot};
use crate::{
    action_log::{ActionLogFeature, ActionLogResult},
    config::{KeepRunningRule, Settings},
    control::{
        keep_running::DesktopContext,
        process::{open_process_for_observation, ProcessControlTarget},
    },
    features::watchdog::keep_running::{KeepRunningState, PauseReason},
    foreground::capture_process_action_target,
    platform::windows::process::poll_exit,
};

const WORKER_POLL: Duration = Duration::from_millis(50);
const KEEP_ALIVE: &str = "keep-alive";

struct FixtureDirectory(PathBuf);

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0.join(KEEP_ALIVE));
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(entries) = fs::read_dir(&self.0) {
                for entry in entries.flatten() {
                    let _ = fs::remove_file(entry.path());
                }
            }
            if fs::remove_dir(&self.0).is_ok() || Instant::now() >= deadline {
                break;
            }
            thread::sleep(WORKER_POLL);
        }
    }
}

fn wait_for<T>(name: &str, mut observe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(evidence) = observe() {
            return evidence;
        }
        assert!(Instant::now() < deadline, "Timed out waiting for {name}");
        thread::sleep(WORKER_POLL);
    }
}

fn assert_during(duration: Duration, mut check: impl FnMut()) {
    let deadline = Instant::now() + duration;
    loop {
        check();
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(WORKER_POLL);
    }
}

#[test]
#[ignore = "Private child entry point for the native Keep Running lifecycle test"]
fn worker() {
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let directory = std::env::current_dir().unwrap().canonicalize().unwrap();
    assert_eq!(executable.parent(), Some(directory.as_path()));
    let id = process::id();
    fs::write(directory.join(format!("started-{id}")), []).unwrap();
    let keep_alive = directory.join(KEEP_ALIVE);
    let exit = directory.join(format!("exit-{id}"));
    let deadline = Instant::now() + Duration::from_secs(30);
    while keep_alive.is_file() && !exit.exists() && Instant::now() < deadline {
        thread::sleep(WORKER_POLL);
    }
}

#[test]
#[ignore = "Requires an ordinary interactive Windows desktop; relaunches and stops only a private fixture"]
fn native_keep_running_relaunches_after_exit_and_respects_stop() {
    DesktopContext::capture().expect("Keep Running requires a compatible interactive desktop");
    let target = std::env::current_dir().unwrap().join("target");
    fs::create_dir_all(&target).unwrap();
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let name = format!("keep-running-{}-{}", process::id(), unique.as_nanos());
    let directory = target.join(&name);
    fs::create_dir(&directory).unwrap();
    let fixture = FixtureDirectory(directory);
    let executable = fixture.0.join(format!("{name}.exe"));
    // Keep Running matches file identities. A hard link would include the test runner.
    fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    fs::write(fixture.0.join(KEEP_ALIVE), []).unwrap();
    let module = module_path!().split_once("::").unwrap().1;
    let entry = format!("{module}::worker");
    let arguments = ["--exact", &entry, "--ignored"];
    let worker_starts = || {
        fs::read_dir(&fixture.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().starts_with("started-"))
            .count()
    };

    let mut rule = KeepRunningRule::new(executable.to_str().unwrap().into());
    rule.arguments = arguments.join(" ");
    let mut settings = Settings::default();
    assert!(!automation_worker_required(&settings));
    settings.general.allow_cross_session_process_control = false;
    settings.keep_running.enabled = true;
    settings.keep_running.rules.push(rule.clone());
    let runtime = RuntimeHandle::start(&runtime_settings(settings));
    let status = || {
        let snapshot = runtime.status_snapshot_since(0).unwrap();
        assert_eq!(snapshot.worker_error, None);
        snapshot
    };
    let applied_ids = |snapshot: &RuntimeStatusSnapshot| {
        snapshot
            .action_log_entries
            .iter()
            .filter(|entry| {
                entry.feature == ActionLogFeature::KeepRunning
                    && entry.result == ActionLogResult::Applied
            })
            .map(|entry| entry.process_id.expect("relaunch must include the new PID"))
            .collect::<Vec<_>>()
    };
    let waiting_for_app = |snapshot: &RuntimeStatusSnapshot| {
        let keep_running = &snapshot.feature_status.keep_running;
        assert_eq!(keep_running.last_error, None);
        keep_running.observation_known
            && keep_running.rule_status(&rule).is_some_and(|status| {
                status.state == KeepRunningState::WaitingForApp && status.attempts == 0
            })
    };
    wait_for("known absent Keep Running app", || {
        waiting_for_app(&status()).then_some(())
    });
    assert_during(Duration::from_millis(2200), || {
        let snapshot = status();
        assert!(waiting_for_app(&snapshot));
        assert!(!snapshot
            .action_log_entries
            .iter()
            .any(|entry| entry.feature == ActionLogFeature::KeepRunning));
        assert_eq!(worker_starts(), 0);
    });

    let mut original = Command::new(&executable)
        .args(arguments)
        .current_dir(&fixture.0)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .unwrap();
    let original_id = original.id();
    let started = |id| fixture.0.join(format!("started-{id}")).is_file();
    wait_for("original worker startup", || {
        started(original_id).then_some(())
    });
    wait_for("known armed Keep Running rule", || {
        let snapshot = status();
        let keep_running = &snapshot.feature_status.keep_running;
        assert_eq!(keep_running.last_error, None);
        let rule_status = keep_running.rule_status(&rule)?;
        (keep_running.observation_known && rule_status.state == KeepRunningState::Armed)
            .then_some(())
    });
    assert!(applied_ids(&status()).is_empty());
    fs::write(fixture.0.join(format!("exit-{original_id}")), []).unwrap();
    let original_exit = wait_for("original process exit", || original.try_wait().unwrap());
    assert!(original_exit.success());

    let replacement = wait_for("accepted replacement process and startup", || {
        let ids = applied_ids(&status());
        assert!(
            ids.len() <= 1,
            "Unexpected multiple Keep Running relaunches"
        );
        ids.first().copied().filter(|id| started(*id))
    });
    assert_ne!(replacement, original_id);
    let target = capture_process_action_target(replacement, &executable, false).unwrap();
    let identity = ProcessControlTarget::from_action_target(&target);
    let (_, handle) = open_process_for_observation(&identity, false).unwrap();
    assert!(!poll_exit(&handle).unwrap());
    let result = runtime.request_process_termination(vec![target]).unwrap();
    assert_eq!(
        result
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap()
            .into_termination_result(),
        Ok(1)
    );
    wait_for("replacement process exit", || {
        poll_exit(&handle).unwrap().then_some(())
    });

    let paused = |snapshot: &RuntimeStatusSnapshot| {
        let keep_running = &snapshot.feature_status.keep_running;
        keep_running.observation_known
            && keep_running.rule_status(&rule).is_some_and(|status| {
                status.state == KeepRunningState::Paused(PauseReason::IntentionalStop)
                    && status.attempts == 1
            })
    };
    wait_for("known rule pause after deliberate Stop", || {
        paused(&status()).then_some(())
    });
    // Leave time for the five-second retry delay and subsequent scheduler ticks.
    assert_during(Duration::from_secs(10), || {
        let snapshot = status();
        assert!(paused(&snapshot));
        assert_eq!(applied_ids(&snapshot), [replacement]);
    });
    runtime.shutdown().unwrap();
    assert_eq!(worker_starts(), 2);
    println!(
        "observed_exit=true replacement_started=true deliberate_stop=true unwanted_relaunches=0"
    );
    println!("initial_absence_verified=true");
}
