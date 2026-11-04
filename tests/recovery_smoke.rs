#![cfg(all(windows, feature = "recovery-smoke"))]
#![warn(clippy::undocumented_unsafe_blocks)]
#![warn(unsafe_op_in_unsafe_fn)]

use std::{
    io::{BufRead, BufReader, Read},
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::{
    Foundation::{CloseHandle, FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        GetExitCodeProcess, GetPriorityClass, GetProcessTimes, OpenProcess,
        QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
        BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    },
};

const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Serialize, Deserialize)]
struct Identity {
    id: u32,
    creation_time: u64,
    executable_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum FixtureState {
    Spawned {
        helper: Identity,
    },
    Ready {
        target: u32,
        priority: u32,
        rejected: usize,
    },
}

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        match self.0.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(error) => eprintln!("Could not inspect owned fixture during cleanup: {error}"),
        }
        let kill_error = self.0.kill().err();
        if let Err(error) = wait_child(&mut self.0) {
            eprintln!("Owned fixture cleanup failed: {error}; termination error: {kill_error:?}");
        }
    }
}

struct NativeHandle(HANDLE);

impl Drop for NativeHandle {
    fn drop(&mut self) {
        // SAFETY: this guard owns the successfully opened handle and closes it once.
        unsafe { CloseHandle(self.0) };
    }
}

struct HelperGuard {
    handle: NativeHandle,
}

impl HelperGuard {
    fn open(identity: &Identity) -> Result<Self, String> {
        // SAFETY: the fixture reported a PID; the returned handle is verified before termination is allowed.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                0,
                identity.id,
            )
        };
        if handle.is_null() {
            return Err(format!(
                "Could not open smoke helper: {}",
                std::io::Error::last_os_error()
            ));
        }
        let handle = NativeHandle(handle);
        let path = process_path(handle.0)?;
        if creation_time(handle.0)? != identity.creation_time
            || !same_path(&path, &identity.executable_path)
            || !same_path(&path, env!("CARGO_BIN_EXE_winderust"))
        {
            return Err(
                "Reported helper identity did not match the exact fixture executable.".to_owned(),
            );
        }
        Ok(Self { handle })
    }

    fn wait_success(&self) -> Result<(), String> {
        // SAFETY: the guard owns a verified helper handle opened with SYNCHRONIZE.
        let status = unsafe { WaitForSingleObject(self.handle.0, TIMEOUT.as_millis() as u32) };
        if status != WAIT_OBJECT_0 {
            return Err(format!(
                "Smoke helper did not exit within the deadline: wait status {status}."
            ));
        }
        let mut code = 0;
        // SAFETY: the live query handle remains owned and code is writable.
        if unsafe { GetExitCodeProcess(self.handle.0, &mut code) } == 0 {
            return Err(format!(
                "Could not query helper exit code: {}",
                std::io::Error::last_os_error()
            ));
        }
        if code != 0 {
            return Err(format!("Smoke helper exited with code {code}."));
        }
        Ok(())
    }
}

impl Drop for HelperGuard {
    fn drop(&mut self) {
        // SAFETY: the verified fixture helper handle remains live and has SYNCHRONIZE access.
        if unsafe { WaitForSingleObject(self.handle.0, 0) } == WAIT_TIMEOUT {
            // SAFETY: this is the exact helper started by the owned fixture, never a PID reopened after reuse.
            let terminated = unsafe { TerminateProcess(self.handle.0, 2) };
            // SAFETY: the same verified handle stays live throughout this bounded wait.
            let status = unsafe { WaitForSingleObject(self.handle.0, TIMEOUT.as_millis() as u32) };
            if terminated == 0 || status != WAIT_OBJECT_0 {
                eprintln!("Helper cleanup failed: termination {terminated}, wait status {status}.");
            }
        }
    }
}

#[test]
fn committed_chain_restores_after_pipe_eof() -> Result<(), String> {
    exercise("committed", false)
}

#[test]
fn committed_chain_restores_after_parent_death() -> Result<(), String> {
    exercise("committed", true)
}

#[test]
fn applied_pending_intent_restores_after_pipe_eof() -> Result<(), String> {
    exercise("pending-applied", false)
}

#[test]
fn applied_pending_intent_restores_after_parent_death() -> Result<(), String> {
    exercise("pending-applied", true)
}

#[test]
fn unapplied_pending_intent_preserves_older_baseline_after_pipe_eof() -> Result<(), String> {
    exercise("pending-unapplied", false)
}

#[test]
fn unapplied_pending_intent_preserves_older_baseline_after_parent_death() -> Result<(), String> {
    exercise("pending-unapplied", true)
}

fn exercise(scenario: &str, kill_parent: bool) -> Result<(), String> {
    let system_root =
        std::env::var_os("SystemRoot").ok_or_else(|| "SystemRoot is unavailable.".to_owned())?;
    let path = Path::new(&system_root).join("System32").join("ping.exe");
    let mut target = OwnedChild(
        Command::new(&path)
            .args(["-t", "127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW | NORMAL_PRIORITY_CLASS)
            .spawn()
            .map_err(|error| format!("Could not start disposable target: {error}"))?,
    );
    let identity = Identity {
        id: target.0.id(),
        creation_time: creation_time(target.0.as_raw_handle().cast())?,
        executable_path: process_path(target.0.as_raw_handle().cast())?,
    };
    if priority(&target.0)? != NORMAL_PRIORITY_CLASS {
        return Err("Disposable target did not start at Normal priority.".to_owned());
    }
    let identity_json = serde_json::to_string(&identity).map_err(|error| error.to_string())?;
    let mut parent = OwnedChild(
        Command::new(env!("CARGO_BIN_EXE_winderust"))
            .args(["--recovery-smoke-parent", scenario, &identity_json])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| format!("Could not start fixture parent: {error}"))?,
    );
    let messages = read_fixture_messages(
        parent
            .0
            .stdout
            .take()
            .ok_or_else(|| "Fixture stdout is unavailable.".to_owned())?,
    );
    let FixtureState::Spawned { helper } = next_state(&messages)? else {
        return Err("Expected helper identity before readiness.".to_owned());
    };
    let helper = HelperGuard::open(&helper)?;
    let FixtureState::Ready {
        target: reported_target,
        priority: reported_priority,
        rejected,
    } = next_state(&messages)?
    else {
        return Err("Expected readiness after helper identity.".to_owned());
    };
    let expected = if scenario == "pending-unapplied" {
        BELOW_NORMAL_PRIORITY_CLASS
    } else {
        IDLE_PRIORITY_CLASS
    };
    if reported_target != identity.id
        || reported_priority != expected
        || rejected != 6
        || priority(&target.0)? != expected
    {
        return Err(
            "Fixture readiness did not establish acknowledged transitions and scope rejection."
                .to_owned(),
        );
    }
    if kill_parent {
        parent
            .0
            .kill()
            .map_err(|error| format!("Could not terminate the fixture parent: {error}"))?;
    } else {
        drop(parent.0.stdin.take());
    }
    let status = wait_child(&mut parent.0)?;
    if !kill_parent && !status.success() {
        return Err(format!("Fixture parent failed during clean EOF: {status}."));
    }
    helper.wait_success()?;
    if target
        .0
        .try_wait()
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("Disposable target exited before restoration could be established.".to_owned());
    }
    if priority(&target.0)? != NORMAL_PRIORITY_CLASS {
        return Err("Helper exited without restoring the original Normal priority.".to_owned());
    }
    Ok(())
}

fn read_fixture_messages(stdout: std::process::ChildStdout) -> Receiver<Result<String, String>> {
    let (sender, receiver) = mpsc::sync_channel(2);
    // This reader owns only stdout. Keep state waits bounded instead of joining a
    // stalled pipe; the owned-parent cleanup closes its writer on every exit path.
    std::thread::spawn(move || {
        let mut input = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            let result = match input.by_ref().take(16 * 1024).read_line(&mut line) {
                Ok(0) => return,
                Ok(_) if line.ends_with('\n') => Ok(line),
                Ok(_) => Err(
                    "Fixture output exceeded its bounded line length or ended mid-line.".to_owned(),
                ),
                Err(error) => Err(format!("Could not read fixture output: {error}")),
            };
            let failed = result.is_err();
            if sender.send(result).is_err() || failed {
                return;
            }
        }
    });
    receiver
}

fn next_state(messages: &Receiver<Result<String, String>>) -> Result<FixtureState, String> {
    let line = messages
        .recv_timeout(TIMEOUT)
        .map_err(|error| format!("Fixture state deadline failed: {error}"))??;
    serde_json::from_str(&line).map_err(|error| format!("Invalid fixture state: {error}: {line}"))
}

fn wait_child(child: &mut Child) -> Result<ExitStatus, String> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err("Owned fixture did not exit before the deadline.".to_owned());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn priority(child: &Child) -> Result<u32, String> {
    // SAFETY: Child owns this exact live process handle with query access.
    let value = unsafe { GetPriorityClass(child.as_raw_handle().cast()) };
    if value == 0 {
        Err(format!(
            "Could not query target priority: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(value)
    }
}

fn creation_time(handle: HANDLE) -> Result<u64, String> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the caller retains a live process query handle; every FILETIME output is writable.
    if unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        return Err(format!(
            "Could not query process creation time: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

fn process_path(handle: HANDLE) -> Result<String, String> {
    let mut buffer = vec![0_u16; 32_768];
    let mut length = buffer.len() as u32;
    // SAFETY: the caller retains a live query handle and buffer covers length writable UTF-16 units.
    if unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) } == 0 {
        return Err(format!(
            "Could not query process path: {}",
            std::io::Error::last_os_error()
        ));
    }
    buffer.truncate(length as usize);
    Ok(String::from_utf16_lossy(&buffer))
}

fn same_path(left: &str, right: &str) -> bool {
    left.replace('/', "\\")
        .trim_start_matches(r"\\?\")
        .eq_ignore_ascii_case(right.replace('/', "\\").trim_start_matches(r"\\?\"))
}

#[test]
fn fixture_paths_match_cargo_target_directory_separators() {
    let native = r"C:\work\Winderust\target\dead-code-audit\debug\winderust.exe";
    assert!(same_path(
        native,
        r"C:\work\Winderust\target/dead-code-audit\debug\winderust.exe"
    ));
    assert!(same_path(
        native,
        r"\\?\c:\work\Winderust\target/dead-code-audit\debug\winderust.exe"
    ));
    assert!(!same_path(
        native,
        r"C:\work\Winderust\target\debug\winderust.exe"
    ));
}
