use std::{mem::size_of, os::windows::ffi::OsStrExt, path::Path, ptr};

use windows_sys::Win32::{
    Foundation::{ERROR_INSUFFICIENT_BUFFER, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Security::{
        GetKernelObjectSecurity, GetTokenInformation, SetKernelObjectSecurity, TokenStatistics,
        DACL_SECURITY_INFORMATION, LABEL_SECURITY_INFORMATION, TOKEN_DUPLICATE, TOKEN_QUERY,
        TOKEN_STATISTICS,
    },
    Storage::FileSystem::{READ_CONTROL, WRITE_DAC, WRITE_OWNER},
    System::{
        Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock},
        Threading::{
            CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList,
            OpenProcessToken, ResumeThread, UpdateProcThreadAttribute, WaitForSingleObject,
            CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT,
            PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_PARENT_PROCESS, STARTUPINFOEXW,
        },
    },
};

use super::process_termination;
use crate::win_util::{last_error, WinHandle};

pub(crate) struct RestartLaunch {
    process: WinHandle,
    thread: WinHandle,
    resumed: bool,
}

impl Drop for RestartLaunch {
    fn drop(&mut self) {
        if !self.resumed {
            // A prepared replacement must never remain suspended after a failed restart.
            let _ = process_termination::terminate(&self.process);
        }
    }
}

struct AttributeList(Vec<usize>);
impl AttributeList {
    fn new() -> Result<Self, String> {
        let mut size = 0;
        // SAFETY: null storage with zero size requests the documented allocation length.
        unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size) };
        if size == 0 || last_error() != ERROR_INSUFFICIENT_BUFFER {
            return Err(error("InitializeProcThreadAttributeList(size)"));
        }
        let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
        // SAFETY: storage is sufficiently sized and naturally aligned, and size is writable.
        if unsafe {
            InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 1, 0, &mut size)
        } == 0
        {
            return Err(error("InitializeProcThreadAttributeList"));
        }
        Ok(Self(storage))
    }
}
impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: this list was successfully initialized and remains allocated until after deletion.
        unsafe { DeleteProcThreadAttributeList(self.0.as_mut_ptr().cast()) };
    }
}

fn error(operation: &str) -> String {
    format!("{operation} failed (Windows error {}).", last_error())
}

fn token(process: &WinHandle, access: u32) -> Result<WinHandle, String> {
    let mut handle = ptr::null_mut();
    // SAFETY: process is retained and handle is writable; returned token ownership is transferred.
    if unsafe { OpenProcessToken(process.raw(), access, &mut handle) } == 0 {
        return Err(error("OpenProcessToken"));
    }
    Ok(WinHandle::new(handle))
}

fn token_statistics(token: &WinHandle) -> Result<TOKEN_STATISTICS, String> {
    let mut statistics = TOKEN_STATISTICS::default();
    let mut size = 0;
    // SAFETY: token is queryable and statistics is aligned writable storage of the declared size.
    if unsafe {
        GetTokenInformation(
            token.raw(),
            TokenStatistics,
            (&mut statistics as *mut TOKEN_STATISTICS).cast(),
            size_of::<TOKEN_STATISTICS>() as u32,
            &mut size,
        )
    } == 0
    {
        return Err(error("GetTokenInformation(TokenStatistics)"));
    }
    Ok(statistics)
}

fn token_security_descriptor(token: &WinHandle) -> Result<Vec<usize>, String> {
    let information = DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION;
    let mut bytes = 0;
    // SAFETY: token has READ_CONTROL; null storage with zero length queries the required size.
    unsafe { GetKernelObjectSecurity(token.raw(), information, ptr::null_mut(), 0, &mut bytes) };
    if bytes == 0 || last_error() != ERROR_INSUFFICIENT_BUFFER {
        return Err(error("GetKernelObjectSecurity(size)"));
    }
    let mut descriptor = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
    // SAFETY: descriptor is aligned, sufficiently sized writable storage; token is retained.
    if unsafe {
        GetKernelObjectSecurity(
            token.raw(),
            information,
            descriptor.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        )
    } == 0
    {
        return Err(error("GetKernelObjectSecurity(token)"));
    }
    Ok(descriptor)
}

pub(crate) fn prepare(processes: &[WinHandle], path: &Path) -> Result<RestartLaunch, String> {
    let first = processes
        .first()
        .ok_or("No process targets were available.")?;
    let launch_token = token(first, TOKEN_QUERY | TOKEN_DUPLICATE | READ_CONTROL)?;
    let original_statistics = token_statistics(&launch_token)?;
    for process in processes.iter().skip(1) {
        let statistics = token_statistics(&token(process, TOKEN_QUERY)?)?;
        if (
            statistics.AuthenticationId.LowPart,
            statistics.AuthenticationId.HighPart,
        ) != (
            original_statistics.AuthenticationId.LowPart,
            original_statistics.AuthenticationId.HighPart,
        ) {
            return Err(
                "Restart cannot combine processes from different Windows logons.".to_owned(),
            );
        }
    }
    let mut security = token_security_descriptor(&launch_token)?;
    let executable: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if executable[..executable.len() - 1]
        .iter()
        .any(|c| *c == 0 || *c == u16::from(b'"'))
        || !path.is_file()
    {
        return Err("The restart executable is unavailable or its path is invalid.".to_owned());
    }
    let directory: Vec<_> = path
        .parent()
        .ok_or("The restart executable has no parent directory.")?
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut command_line: Vec<_> = std::iter::once(u16::from(b'"'))
        .chain(executable[..executable.len() - 1].iter().copied())
        .chain([u16::from(b'"'), 0])
        .collect();
    let mut parent = first.raw();
    let mut attributes = AttributeList::new()?;
    // SAFETY: parent is a retained exact handle with PROCESS_CREATE_PROCESS and remains alive until
    // the attribute list is destroyed. Windows inherits the parent's token instead of Winderust's.
    if unsafe {
        UpdateProcThreadAttribute(
            attributes.0.as_mut_ptr().cast(),
            0,
            PROC_THREAD_ATTRIBUTE_PARENT_PROCESS as usize,
            (&mut parent as *mut windows_sys::Win32::Foundation::HANDLE).cast(),
            size_of::<windows_sys::Win32::Foundation::HANDLE>(),
            ptr::null_mut(),
            ptr::null(),
        )
    } == 0
    {
        return Err(error("UpdateProcThreadAttribute(parent)"));
    }
    let startup = STARTUPINFOEXW {
        StartupInfo: windows_sys::Win32::System::Threading::STARTUPINFOW {
            cb: size_of::<STARTUPINFOEXW>() as u32,
            ..Default::default()
        },
        lpAttributeList: attributes.0.as_mut_ptr().cast(),
    };
    let mut environment = ptr::null_mut();
    // SAFETY: launch_token is a retained primary token and environment is writable.
    if unsafe { CreateEnvironmentBlock(&mut environment, launch_token.raw(), 0) } == 0 {
        return Err(error("CreateEnvironmentBlock"));
    }
    let mut process = PROCESS_INFORMATION::default();
    // SAFETY: strings, writable command line, Unicode environment, parent handle, and attribute list
    // remain live through the call. The child starts suspended and no handles are inherited.
    let created = unsafe {
        CreateProcessW(
            executable.as_ptr(),
            command_line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
            environment,
            directory.as_ptr(),
            &startup.StartupInfo,
            &mut process,
        )
    };
    let creation_error = (created == 0).then(|| error("CreateProcessW"));
    // SAFETY: environment was returned by CreateEnvironmentBlock and CreateProcessW has copied it.
    unsafe { DestroyEnvironmentBlock(environment) };
    if let Some(error) = creation_error {
        return Err(error);
    }
    let launch = RestartLaunch {
        process: WinHandle::new(process.hProcess),
        thread: WinHandle::new(process.hThread),
        resumed: false,
    };
    let replacement_token = token(&launch.process, TOKEN_QUERY | WRITE_DAC | WRITE_OWNER)?;
    let replacement_statistics = token_statistics(&replacement_token)?;
    if (
        replacement_statistics.TokenId.LowPart,
        replacement_statistics.TokenId.HighPart,
    ) == (
        original_statistics.TokenId.LowPart,
        original_statistics.TokenId.HighPart,
    ) {
        return Err("The replacement must own a separate primary token.".to_owned());
    }
    // CreateProcess inherits the parent's token contents, but the creator sets the new token
    // object's security. Preserve the app's permissions/label before any browser code runs.
    // SAFETY: this changes only the distinct token of our owned suspended replacement; security
    // contains the original app token's native DACL and mandatory label, retained through the call.
    if unsafe {
        SetKernelObjectSecurity(
            replacement_token.raw(),
            DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
            security.as_mut_ptr().cast(),
        )
    } == 0
    {
        return Err(error("SetKernelObjectSecurity(replacement token)"));
    }
    Ok(launch)
}

pub(crate) fn wait(process: &WinHandle, milliseconds: u32) -> Result<(), String> {
    // SAFETY: process is an exact retained handle opened with SYNCHRONIZE; the wait is bounded.
    match unsafe { WaitForSingleObject(process.raw(), milliseconds) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => {
            Err("Timed out waiting for the app to stop; it was not relaunched.".to_owned())
        }
        _ => Err(error("WaitForSingleObject")),
    }
}

pub(crate) fn launch(mut launch: RestartLaunch) -> Result<(), String> {
    // SAFETY: launch owns the suspended replacement process and a zero timeout only samples exit.
    match unsafe { WaitForSingleObject(launch.process.raw(), 0) } {
        WAIT_TIMEOUT => {}
        WAIT_OBJECT_0 => return Err("The prepared replacement exited before restart.".to_owned()),
        _ => return Err(error("WaitForSingleObject(replacement)")),
    }
    // SAFETY: thread is the owned primary thread created with CREATE_SUSPENDED exactly once.
    match unsafe { ResumeThread(launch.thread.raw()) } {
        1 => {}
        u32::MAX => return Err(error("ResumeThread")),
        count => {
            return Err(format!(
                "Unexpected replacement thread suspension count: {count}."
            ))
        }
    }
    launch.resumed = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetSecurityDescriptorDacl, GetSecurityDescriptorSacl, SetKernelObjectSecurity,
        },
        Storage::FileSystem::WRITE_OWNER,
    };

    fn acl_bytes(descriptor: &mut [usize], sacl: bool) -> Vec<u8> {
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = ptr::null_mut();
        // SAFETY: descriptor contains a successful native security query and outputs are writable.
        let ok = unsafe {
            if sacl {
                GetSecurityDescriptorSacl(
                    descriptor.as_mut_ptr().cast(),
                    &mut present,
                    &mut acl,
                    &mut defaulted,
                )
            } else {
                GetSecurityDescriptorDacl(
                    descriptor.as_mut_ptr().cast(),
                    &mut present,
                    &mut acl,
                    &mut defaulted,
                )
            }
        };
        assert_ne!(ok, 0);
        assert_ne!(present, 0);
        assert!(!acl.is_null());
        // SAFETY: the successful descriptor query returns an ACL contained in the live buffer.
        unsafe { std::slice::from_raw_parts(acl.cast::<u8>(), (*acl).AclSize as usize).to_vec() }
    }

    #[test]
    fn suspended_replacement_preserves_token_dacl_and_mandatory_label() -> Result<(), String> {
        use super::super::process::{self, ProcessAccess};
        // Neither owned copy of the test executable is ever resumed.
        let parent = process::open(std::process::id(), ProcessAccess::Restart)
            .map_err(|e| format!("{e:?}"))?;
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let original_launch = prepare(&[parent], &executable)?;
        let original = token(
            &original_launch.process,
            TOKEN_QUERY | READ_CONTROL | WRITE_OWNER,
        )?;
        // Reproduce a creator above the target token object's label without elevating the test.
        let label = crate::win_util::wide_null("S:(ML;;NWNRNX;;;LW)");
        let mut descriptor = ptr::null_mut();
        // SAFETY: label is terminated SDDL and descriptor is a writable LocalAlloc output.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                label.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(error(
                "ConvertStringSecurityDescriptorToSecurityDescriptorW",
            ));
        }
        // SAFETY: original is only our suspended child's token with WRITE_OWNER, not the test token.
        let changed = unsafe {
            SetKernelObjectSecurity(original.raw(), LABEL_SECURITY_INFORMATION, descriptor)
        };
        let change_error = (changed == 0).then(|| error("SetKernelObjectSecurity(test label)"));
        // SAFETY: descriptor was allocated by the successful SDDL conversion and is no longer used.
        unsafe { LocalFree(descriptor) };
        if let Some(error) = change_error {
            return Err(error);
        }
        let launch = prepare(std::slice::from_ref(&original_launch.process), &executable)?;
        let replacement = token(&launch.process, TOKEN_QUERY | READ_CONTROL)?;
        let mut source = token_security_descriptor(&original)?;
        let mut destination = token_security_descriptor(&replacement)?;
        assert_eq!(
            acl_bytes(&mut source, false),
            acl_bytes(&mut destination, false),
            "replacement token permissions changed"
        );
        assert_eq!(
            acl_bytes(&mut source, true),
            acl_bytes(&mut destination, true),
            "replacement token integrity label changed"
        );
        Ok(())
    }
}
