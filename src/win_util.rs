use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, FILETIME, HANDLE};

pub(crate) struct WinHandle(HANDLE);

impl WinHandle {
    pub(crate) fn new(handle: HANDLE) -> Self {
        debug_assert!(!handle.is_null());
        Self(handle)
    }

    pub(crate) fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for WinHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub(crate) fn last_error() -> u32 {
    unsafe { GetLastError() }
}

pub(crate) fn filetime_to_u64(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

pub(crate) fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
