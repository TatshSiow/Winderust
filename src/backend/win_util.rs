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
