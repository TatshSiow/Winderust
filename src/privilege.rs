use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_NOT_ALL_ASSIGNED, LUID},
    Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_DEBUG_NAME,
        SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub fn enable_debug_privilege() -> bool {
    let mut token = std::ptr::null_mut();
    let opened = unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
    };
    if opened == 0 {
        return false;
    }

    let enabled = enable_debug_privilege_for_token(token);
    unsafe {
        CloseHandle(token);
    }
    enabled
}

fn enable_debug_privilege_for_token(token: windows_sys::Win32::Foundation::HANDLE) -> bool {
    let mut luid = LUID::default();
    let found = unsafe { LookupPrivilegeValueW(std::ptr::null(), SE_DEBUG_NAME, &mut luid) };
    if found == 0 {
        return false;
    }

    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };

    let adjusted = unsafe {
        AdjustTokenPrivileges(
            token,
            0,
            &privileges,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    adjusted != 0 && unsafe { GetLastError() } != ERROR_NOT_ALL_ASSIGNED
}
