use std::{
    fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{self, Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use rust_i18n::t;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use super::{runtime_settings, RuntimeHandle};
use crate::{
    action_log::{ActionLogFeature, ActionLogResult},
    config::{InstanceLimitsRule, InstanceLimitsSettings, Settings},
};

struct FixtureImage(PathBuf);

impl Drop for FixtureImage {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        if let Some(directory) = self.0.parent() {
            let _ = fs::remove_dir(directory);
        }
    }
}

struct Worker(Child);

impl Worker {
    fn start(image: &Path) -> Self {
        let child = Command::new(image)
            .args([
                "--ignored",
                "--exact",
                "control::instance_limits::tests::native_worker",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        Self(child)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        drop(self.0.stdin.take());
        let timeout = Duration::from_secs(2);
        if wait_for(timeout, || self.0.try_wait().ok().flatten()).is_none() {
            let _ = self.0.kill();
            let _ = wait_for(timeout, || self.0.try_wait().ok().flatten());
        }
    }
}

fn wait_for<T>(timeout: Duration, mut observe: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(evidence) = observe() {
            return Some(evidence);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "Uses the Windows event watcher and two private bounded process fixtures"]
fn native_instance_limits_reports_excess_and_recovery_without_stopping_workers() {
    let target = std::env::current_dir().unwrap().join("target");
    fs::create_dir_all(&target).unwrap();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("instance-limits-{}-{nonce}", process::id());
    let directory = target.join(&name);
    fs::create_dir(&directory).unwrap();
    let image = FixtureImage(directory.join(format!("{name}.exe")));
    fs::hard_link(std::env::current_exe().unwrap(), &image.0).unwrap();
    let path = image.0.to_str().unwrap();
    let mut rule = InstanceLimitsRule::new(path.into());
    rule.notify = false;
    let settings = Settings {
        instance_limits: InstanceLimitsSettings {
            enabled: true,
            notify: false,
            rules: vec![rule],
        },
        ..Settings::default()
    };

    let mut first = Worker::start(&image.0);
    let mut survivor = Worker::start(&image.0);
    let runtime = RuntimeHandle::start(&runtime_settings(settings));
    let entries = || {
        let snapshot = runtime.status_snapshot_since(0).unwrap();
        assert_eq!(snapshot.worker_error, None);
        snapshot
            .action_log_entries
            .iter()
            .filter(|entry| entry.feature == ActionLogFeature::InstanceLimits)
            .cloned()
            .collect::<Vec<_>>()
    };
    let timeout = Duration::from_secs(8);
    let excess = t!("instance_limits.excess_log", count = 2, maximum = 1);
    let recovered = t!("instance_limits.recovered_log", count = 1, maximum = 1);
    let await_entry = |reason: &str| {
        wait_for(timeout, || {
            entries().into_iter().find(|entry| entry.reason == reason)
        })
        .unwrap_or_else(|| panic!("Expected {reason}, observed {:?}", entries()))
    };

    let excess = await_entry(&excess);
    assert_eq!(first.0.try_wait().unwrap(), None);
    assert_eq!(survivor.0.try_wait().unwrap(), None);
    drop(first.0.stdin.take());
    let exit = wait_for(timeout, || first.0.try_wait().unwrap()).expect("first worker exit");
    assert!(exit.success());

    let recovered = await_entry(&recovered);
    assert!(excess.sequence < recovered.sequence);
    assert_eq!(survivor.0.try_wait().unwrap(), None);
    for entry in [&excess, &recovered] {
        assert_eq!(entry.process_name, path);
        assert_eq!(entry.process_id, None);
        assert_eq!(entry.result, ActionLogResult::Skipped);
    }
    assert_eq!(entries(), [excess, recovered]);
    runtime.shutdown().unwrap();
    assert_eq!(survivor.0.try_wait().unwrap(), None);
    drop(survivor.0.stdin.take());
    let exit = wait_for(timeout, || survivor.0.try_wait().unwrap()).expect("surviving worker exit");
    assert!(exit.success());
    println!("excess_count=2 recovery_count=1 monitor_stopped_workers=0 notifications=0");
}
