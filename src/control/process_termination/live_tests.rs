use super::*;
use std::path::PathBuf;

#[test]
#[ignore = "trims and terminates its own disposable Windows process; run in integration QA"]
fn live_trim_and_termination_validate_identity() -> Result<(), String> {
    use crate::control::memory_trim::MemoryTrimController;
    use std::{
        io::BufRead,
        os::windows::process::CommandExt,
        process::{Child, Command, Stdio},
    };

    struct DisposableProcess(Child);
    impl Drop for DisposableProcess {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let executable = PathBuf::from(std::env::var_os("SystemRoot").ok_or("SystemRoot missing")?)
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let mut child = DisposableProcess(Command::new(&executable)
        .args(["-NoProfile", "-Command", "$a = New-Object byte[] 67108864; for ($i=0; $i -lt $a.Length; $i+=4096) { $a[$i]=1 }; [Console]::WriteLine('ready'); Start-Sleep -Seconds 30"])
        .creation_flags(0x0800_0000)
        .stdout(Stdio::piped()).spawn().map_err(|e| e.to_string())?);
    let mut ready = String::new();
    std::io::BufReader::new(child.0.stdout.take().ok_or("No stdout")?)
        .read_line(&mut ready)
        .map_err(|e| e.to_string())?;
    assert_eq!(ready.trim(), "ready");
    let target = crate::foreground::capture_process_action_target(child.0.id(), &executable, false)
        .map_err(|e| e.to_string())?;
    let control_target = ProcessControlTarget::from_action_target(&target);
    let mut trim = MemoryTrimController::default();
    let before = trim
        .sample(&control_target, false, true)
        .map_err(|e| e.to_string())?;
    assert!(before.working_set_bytes >= 64 * 1024 * 1024);
    let outcome = trim
        .trim(&control_target, false)
        .map_err(|e| e.to_string())?;
    assert!(outcome.freed_bytes.is_some_and(|bytes| bytes > 0));
    assert!(child.0.try_wait().map_err(|e| e.to_string())?.is_none());

    let mut termination = ProcessTerminationController::default();
    let mut stale = target.clone();
    stale.creation_time = stale.creation_time.wrapping_add(1);
    let rejected = termination.terminate_batch(vec![target.clone(), stale], false);
    assert!(rejected.preflight_failed);
    assert!(child.0.try_wait().map_err(|e| e.to_string())?.is_none());
    termination
        .terminate_batch(vec![target], false)
        .into_process_list_result()?;
    child.0.wait().map_err(|e| e.to_string())?;
    Ok(())
}
