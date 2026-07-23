use std::{ffi::OsStr, os::windows::ffi::OsStrExt};

use windows_sys::Win32::UI::{
    Shell::{IsUserAnAdmin, ShellExecuteW},
    WindowsAndMessaging::SW_SHOWNORMAL,
};
pub fn is_running_as_admin() -> bool {
    // SAFETY: IsUserAnAdmin takes no arguments and has no caller requirements.
    unsafe { IsUserAnAdmin() != 0 }
}

pub fn relaunch_as_admin() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };

    let operation = wide("runas");
    let file = wide_os(exe.as_os_str());
    // SAFETY: operation and file are terminated UTF-16 strings, optional parameters are null,
    // and no returned handle is transferred to the caller.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    result as isize > 32
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_os(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}
