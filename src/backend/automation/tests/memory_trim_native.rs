use std::{
    fs,
    hint::black_box,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{self, Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use super::{runtime_settings, ActionLogFeature, ActionLogResult, RuntimeHandle, Settings};
use crate::{
    control::{memory_trim::MemoryTrimController, process::ProcessControlTarget},
    foreground::capture_process_action_target,
};

const PAYLOAD_BYTES: usize = 32 * 1024 * 1024;
const PAGE_STRIDE: usize = 4096;
const POLL: Duration = Duration::from_millis(25);

struct Fixture {
    directory: PathBuf,
    workers: Vec<Child>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for child in &mut self.workers {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Ok(entries) = fs::read_dir(&self.directory) {
            for entry in entries.flatten() {
                let _ = fs::remove_file(entry.path());
            }
        }
        let _ = fs::remove_dir(&self.directory);
    }
}

fn marker(directory: &Path, name: &str, id: u32) -> PathBuf {
    directory.join(format!("{name}-{id}"))
}

fn await_evidence(name: &str, mut observed: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !observed() {
        assert!(Instant::now() < deadline, "Timed out waiting for {name}");
        thread::sleep(POLL);
    }
}

#[test]
#[ignore = "Private child entry point for the native Process List Memory Trim check"]
fn worker() {
    let executable = std::env::current_exe().unwrap();
    let directory = std::env::current_dir().unwrap();
    assert_eq!(executable.parent(), Some(directory.as_path()));
    let mut memory = vec![0; PAYLOAD_BYTES];
    for (index, byte) in memory.iter_mut().step_by(PAGE_STRIDE).enumerate() {
        *byte = (index % 251) as u8 + 1;
    }
    let id = process::id();
    black_box(&mut memory);
    fs::write(marker(&directory, "ready", id), []).unwrap();
    await_evidence("refill request", || {
        marker(&directory, "refill", id).is_file()
    });
    for (index, &byte) in black_box(&memory).iter().step_by(PAGE_STRIDE).enumerate() {
        assert_eq!(byte, (index % 251) as u8 + 1);
    }
    fs::write(marker(&directory, "verified", id), []).unwrap();
    thread::sleep(Duration::from_secs(10));
    black_box(memory);
}

#[test]
#[ignore = "Spawns and trims only private workers; requires ordinary Windows process access"]
fn native_process_list_trim_releases_resident_pages_and_refills() {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let name = format!("memory-trim-{}-{}", process::id(), unique.as_nanos());
    let target = std::env::current_dir().unwrap().join("target");
    fs::create_dir_all(&target).unwrap();
    let directory = target.join(&name);
    fs::create_dir(&directory).unwrap();
    let mut fixture = Fixture {
        directory,
        workers: Vec::new(),
    };
    let executable = fixture.directory.join(format!("{name}.exe"));
    fs::hard_link(std::env::current_exe().unwrap(), &executable).unwrap();
    let module = module_path!().split_once("::").unwrap().1;
    let entry = format!("{module}::worker");
    for _ in 0..2 {
        let child = Command::new(&executable)
            .args(["--ignored", "--exact", &entry])
            .current_dir(&fixture.directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        fixture.workers.push(child);
    }
    let selected = fixture.workers[0].id();
    let unselected = fixture.workers[1].id();
    await_evidence("both worker allocations", || {
        [selected, unselected]
            .into_iter()
            .all(|id| marker(&fixture.directory, "ready", id).is_file())
    });

    let selected_target = capture_process_action_target(selected, &executable, false).unwrap();
    let unselected_target = capture_process_action_target(unselected, &executable, false).unwrap();
    let mut controller = MemoryTrimController::default();
    let mut working_set = |target| {
        controller
            .sample(
                &ProcessControlTarget::from_action_target(target),
                false,
                false,
            )
            .unwrap()
            .working_set_bytes
    };
    let before = working_set(&selected_target);
    let unselected_before = working_set(&unselected_target);
    assert!(before >= PAYLOAD_BYTES as u64 && unselected_before >= PAYLOAD_BYTES as u64);

    let mut settings = Settings::default();
    settings.general.enabled = false;
    settings.general.allow_cross_session_process_control = false;
    let runtime = RuntimeHandle::start(&runtime_settings(settings));
    runtime
        .request_process_memory_trim(vec![Ok(selected_target.clone())])
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap()
        .into_process_list_result()
        .unwrap();
    let after = working_set(&selected_target);
    let unselected_after = working_set(&unselected_target);
    let measurable_drop = PAYLOAD_BYTES as u64 / 2;
    assert!(before.saturating_sub(after) >= measurable_drop);
    assert!(unselected_before.saturating_sub(unselected_after) < measurable_drop);

    await_evidence("Memory Trim Action Log entry", || {
        runtime.status_snapshot_since(0).is_some_and(|status| {
            let entries = status.action_log_entries.iter().filter(|entry| entry.feature == ActionLogFeature::MemoryTrim).collect::<Vec<_>>();
            matches!(entries.as_slice(), [entry] if entry.process_id == Some(selected) && entry.result == ActionLogResult::Applied)
        })
    });
    fs::write(marker(&fixture.directory, "refill", selected), []).unwrap();
    await_evidence("verified worker contents after trim", || {
        marker(&fixture.directory, "verified", selected).is_file()
    });
    let refilled = working_set(&selected_target);
    assert!(refilled.saturating_sub(after) >= measurable_drop);
    println!("Selected PID {selected} working-set bytes {before} -> {after} -> {refilled}; unselected PID {unselected} {unselected_before} -> {unselected_after}");
    runtime.shutdown().unwrap();
}
