use std::{ffi::OsStr, mem::size_of, os::windows::ffi::OsStrExt};

use windows_sys::Win32::{
    Foundation::WAIT_TIMEOUT,
    System::Threading::WaitForSingleObject,
    UI::{
        Shell::{
            IsUserAnAdmin, ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
            SHELLEXECUTEINFOW,
        },
        WindowsAndMessaging::SW_SHOWNORMAL,
    },
};

use crate::win_util::{wide_null, WinHandle};

const ELEVATED_RELAUNCH_ARGUMENT: &str = "--winderust-elevated-relaunch";

pub fn is_running_as_admin() -> bool {
    // SAFETY: IsUserAnAdmin takes no arguments and has no caller requirements.
    unsafe { IsUserAnAdmin() != 0 }
}

pub fn relaunch_as_admin() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };

    let operation = wide_null("runas");
    let file = wide_os_null(exe.as_os_str());
    let parameters = wide_null(ELEVATED_RELAUNCH_ARGUMENT);
    let mut execute_info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_NOCLOSEPROCESS,
        lpVerb: operation.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: parameters.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };

    // SAFETY: execute_info contains its documented size and live terminated UTF-16 strings for
    // the duration of the synchronous call. A returned process handle is closed below.
    if unsafe { ShellExecuteExW(&mut execute_info) } == 0 || execute_info.hProcess.is_null() {
        return false;
    }
    let launched_process = WinHandle::new(execute_info.hProcess);
    // SAFETY: launched_process owns a live process handle and a zero timeout only samples its
    // state. The elevated replacement waits on the single-instance mutex until this process exits.
    unsafe { WaitForSingleObject(launched_process.raw(), 0) == WAIT_TIMEOUT }
}

pub fn elevated_relaunch_requested() -> bool {
    is_elevated_relaunch_argument(std::env::args_os().nth(1).as_deref())
}

fn is_elevated_relaunch_argument(argument: Option<&OsStr>) -> bool {
    argument == Some(OsStr::new(ELEVATED_RELAUNCH_ARGUMENT))
}

fn wide_os_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevated_relaunch_argument_matching_is_exact() {
        assert!(is_elevated_relaunch_argument(Some(OsStr::new(
            ELEVATED_RELAUNCH_ARGUMENT
        ))));
        assert!(!is_elevated_relaunch_argument(Some(OsStr::new(
            "--crash-recovery-watchdog"
        ))));
        assert!(!is_elevated_relaunch_argument(None));
    }
}
