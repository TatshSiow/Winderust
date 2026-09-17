use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, FILETIME, HANDLE};
use windows_sys::Win32::System::Threading::GetProcessTimes;

pub(crate) struct WinHandle(HANDLE);

impl WinHandle {
    pub(crate) fn new(handle: HANDLE) -> Self {
        debug_assert!(!handle.is_null());
        Self(handle)
    }

    pub(crate) fn raw(&self) -> HANDLE {
        self.0
    }

    pub(crate) fn process_creation_time(&self) -> Option<u64> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: self owns a live process handle and all FILETIME outputs are writable for the
        // duration of the call.
        let ok =
            unsafe { GetProcessTimes(self.0, &mut creation, &mut exit, &mut kernel, &mut user) };
        (ok != 0).then(|| filetime_to_u64(creation))
    }
}

impl Drop for WinHandle {
    fn drop(&mut self) {
        // SAFETY: self.0 is an owned non-null Win32 handle and is closed exactly once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub(crate) fn last_error() -> u32 {
    // SAFETY: GetLastError takes no arguments and reads thread-local state.
    unsafe { GetLastError() }
}

pub(crate) fn filetime_to_u64(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

pub(crate) fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Open an HTTPS link in the user's registered browser.
pub(crate) fn open_url(url: &str) -> Result<(), String> {
    if !valid_web_url(url) {
        return Err("Only valid HTTPS links can be opened.".to_owned());
    }
    let operation = wide_null("open");
    let url = wide_null(url);
    // SAFETY: Both strings are NUL-terminated and remain alive for the synchronous
    // shell call. No owner, parameters, or working-directory pointers are supplied.
    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    } as isize;
    if result > 32 {
        Ok(())
    } else {
        Err(format!(
            "Windows could not open the link (ShellExecute error {result})."
        ))
    }
}
fn valid_web_url(url: &str) -> bool {
    url.strip_prefix("https://").is_some_and(|rest| {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        !authority.is_empty()
            && !authority.contains('@')
            && !url.chars().any(|c| c.is_control() || c.is_whitespace())
    })
}
#[cfg(test)]
mod web_url_tests {
    use super::*;
    #[test]
    fn rejects_shell_protocols_and_embedded_string_terminators() {
        assert!(valid_web_url(
            "https://github.com/TatshSiow/Winderust#readme"
        ));
        for invalid in [
            "file:///C:/Windows/notepad.exe",
            "cmd:/test",
            "https://",
            "https://example.com\0bad",
            "https://example.com /bad",
            "https://name@example.com",
        ] {
            assert!(!valid_web_url(invalid));
        }
    }
}
