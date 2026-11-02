use super::*;
use std::{
    fs,
    os::windows::process::CommandExt,
    process::{Child, Command, Stdio},
    thread,
};

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "compiles and restarts only its owned disposable fixture; run in Windows integration QA"]
fn live_restart_preserves_context_and_rejects_stale_identity() -> Result<(), String> {
    let directory = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join("target")
        .join(format!("watchdog-restart-{}", std::process::id()));
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let source = directory.join("fixture.rs");
    let executable = directory.join("watchdog-restart-fixture.exe");
    let log = executable.with_extension("pids");
    fs::write(&source, r##"
#![windows_subsystem = "windows"]
use std::{fs::OpenOptions, io::Write, time::Duration};
fn main() {
    let executable = std::env::current_exe().unwrap();
    let mut file = OpenOptions::new().create(true).append(true).open(executable.with_extension("pids")).unwrap();
    writeln!(file, "{}|{}|{}|{}", std::process::id(), std::env::var("USERNAME").unwrap(), std::env::current_dir().unwrap().display(), std::env::args().count()).unwrap();
    std::thread::sleep(Duration::from_secs(15));
}
"##).map_err(|e| e.to_string())?;
    let compiled = Command::new("rustc")
        .args(["--crate-name", "watchdog_restart_fixture", "--edition=2021"])
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|e| e.to_string())?;
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    // A previous QA run cannot supply identities for this run.
    if log.exists() {
        fs::remove_file(&log).map_err(|e| e.to_string())?;
    }
    let mut child = ChildGuard(
        Command::new(&executable)
            .current_dir(&directory)
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    let wait_for_lines = |count: usize| -> Result<Vec<String>, String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(text) = fs::read_to_string(&log) {
                let lines: Vec<_> = text.lines().map(str::to_owned).collect();
                if lines.len() >= count && text.ends_with('\n') {
                    return Ok(lines);
                }
            }
            if Instant::now() >= deadline {
                return Err("Fixture did not report its launch.".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    };
    let before = wait_for_lines(1)?;
    let target = crate::foreground::capture_process_action_target(child.0.id(), &executable, false)
        .map_err(|e| e.to_string())?;
    let mut stale = target.clone();
    stale.creation_time = stale.creation_time.wrapping_add(1);
    let rejected = restart_app(vec![target.clone(), stale], false);
    assert!(rejected.result.is_err());
    assert!(rejected.stopped.is_empty());
    assert!(child.0.try_wait().map_err(|e| e.to_string())?.is_none());

    let (_, process) =
        open_process_for_restart(&ProcessControlTarget::from_action_target(&target), false)
            .map_err(|error| error.to_string())?;
    let prepared = process_restart::prepare(&[process], &executable)?;
    thread::sleep(Duration::from_millis(50));
    assert_eq!(
        fs::read_to_string(&log)
            .map_err(|e| e.to_string())?
            .lines()
            .count(),
        1
    );
    drop(prepared);
    assert!(child.0.try_wait().map_err(|e| e.to_string())?.is_none());

    let mut other = ChildGuard(
        Command::new(&executable)
            .current_dir(&directory)
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    wait_for_lines(2)?;
    let other_target =
        crate::foreground::capture_process_action_target(other.0.id(), &executable, false)
            .map_err(|e| e.to_string())?;
    let restarted = restart_app(vec![other_target, target], false);
    restarted.result?;
    assert_eq!(restarted.stopped.len(), 2);
    other.0.wait().map_err(|e| e.to_string())?;
    child.0.wait().map_err(|e| e.to_string())?;
    let after = wait_for_lines(3)?;
    assert_eq!(after.len(), 3);
    let first: Vec<_> = before[0].split('|').collect();
    let second: Vec<_> = after[2].split('|').collect();
    let replacement_id = second[0].parse::<u32>().map_err(|e| e.to_string())?;
    let replacement =
        crate::foreground::capture_process_action_target(replacement_id, &executable, false)
            .map_err(|e| e.to_string())?;
    // Clean up only the verified replacement from this fixture before asserting its launch context.
    super::super::process_termination::ProcessTerminationController::default()
        .terminate_batch(vec![replacement], false)
        .into_process_list_result()?;
    assert_ne!(first[0], second[0]);
    assert_eq!(first[1..], second[1..]);
    assert_eq!(second[3], "1");
    Ok(())
}
