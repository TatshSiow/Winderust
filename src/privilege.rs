use windows_sys::Win32::{
    Foundation::{ERROR_NOT_ALL_ASSIGNED, LUID},
    Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_DEBUG_NAME,
        SE_INCREASE_QUOTA_NAME, SE_PRIVILEGE_ENABLED, SE_PROF_SINGLE_PROCESS_NAME,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

use crate::win_util::{last_error, WinHandle};

pub fn enable_debug_privilege() -> bool {
    enable_privilege(SE_DEBUG_NAME)
}

pub fn enable_increase_quota_privilege() -> bool {
    enable_privilege(SE_INCREASE_QUOTA_NAME)
}

pub fn enable_profile_single_process_privilege() -> bool {
    enable_privilege(SE_PROF_SINGLE_PROCESS_NAME)
}

fn enable_privilege(name: windows_sys::core::PCWSTR) -> bool {
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

    let token = WinHandle::new(token);
    enable_privilege_for_token(token.raw(), name)
}

fn enable_privilege_for_token(
    token: windows_sys::Win32::Foundation::HANDLE,
    name: windows_sys::core::PCWSTR,
) -> bool {
    let mut luid = LUID::default();
    let found = unsafe { LookupPrivilegeValueW(std::ptr::null(), name, &mut luid) };
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
    adjusted != 0 && last_error() != ERROR_NOT_ALL_ASSIGNED
}
