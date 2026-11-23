//! Keep Running's retained desktop and provisional-launch boundary.
//!
//! The caller owns intent ordering, fresh absence checks and worker capacity. A successful job
//! disarm is irreversible acceptance; publication of that result is a separate operation.
use std::{
    cell::OnceCell,
    ffi::{c_void, OsString},
    mem::{offset_of, size_of, size_of_val},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{
        DuplicateHandle, HANDLE, HWND, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    Security::{
        GetKernelObjectSecurity, GetSecurityDescriptorDacl, GetSecurityDescriptorSacl,
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, IsWellKnownSid,
        SetKernelObjectSecurity, TokenElevation, TokenElevationType, TokenGroups,
        TokenIntegrityLevel, TokenIsAppContainer, TokenLinkedToken, TokenPrivileges,
        TokenRestrictedSids, TokenSessionId, TokenStatistics, TokenUIAccess, TokenUser,
        WinLocalServiceSid, WinLocalSystemSid, WinNetworkServiceSid, DACL_SECURITY_INFORMATION,
        LABEL_SECURITY_INFORMATION, LUID_AND_ATTRIBUTES, SID_AND_ATTRIBUTES, TOKEN_DUPLICATE,
        TOKEN_GROUPS, TOKEN_INFORMATION_CLASS, TOKEN_LINKED_TOKEN, TOKEN_MANDATORY_LABEL,
        TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_STATISTICS, TOKEN_USER,
    },
    Storage::FileSystem::{
        CreateFileW, FileIdInfo, GetFileInformationByHandleEx, FILE_ATTRIBUTE_NORMAL, FILE_ID_INFO,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        READ_CONTROL, WRITE_DAC, WRITE_OWNER,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock},
        JobObjects::{
            CreateJobObjectW, IsProcessInJob, JobObjectExtendedLimitInformation,
            QueryInformationJobObject, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
        StationsAndDesktops::{
            GetProcessWindowStation, GetThreadDesktop, GetUserObjectInformationW, UOI_NAME,
        },
        Threading::{
            CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess, GetCurrentThreadId,
            GetPriorityClass, InitializeProcThreadAttributeList, OpenProcess, OpenProcessToken,
            QueryFullProcessImageNameW, ResumeThread, TerminateProcess, UpdateProcThreadAttribute,
            WaitForSingleObject, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
            EXTENDED_STARTUPINFO_PRESENT, NORMAL_PRIORITY_CLASS, PROCESS_CREATE_PROCESS,
            PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_PARENT_PROCESS, STARTUPINFOEXW,
        },
    },
    UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId},
};

use crate::win_util::{last_error, WinHandle};

use super::sid_bytes;

type Result<T> = std::result::Result<T, KeepRunningError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorKind {
    Retryable,
    Unsupported,
    CleanupUnknown,
}

#[derive(Debug, Clone, thiserror::Error)]
pub(crate) enum KeepRunningError {
    #[error("{0}")]
    Unsupported(&'static str),
    #[error("{operation} failed (Windows error {code}).")]
    Failed { operation: &'static str, code: u32 },
    #[error("The prepared process's exit could not be confirmed (Windows error {0}).")]
    CleanupUnknown(u32),
}

impl KeepRunningError {
    pub(crate) fn kind(&self) -> ErrorKind {
        match self {
            Self::Unsupported(_) => ErrorKind::Unsupported,
            Self::Failed { code, .. }
                if matches!(
                    *code,
                    windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED
                        | windows_sys::Win32::Foundation::ERROR_PRIVILEGE_NOT_HELD
                        | windows_sys::Win32::Foundation::ERROR_ELEVATION_REQUIRED
                ) =>
            {
                ErrorKind::Unsupported
            }
            Self::Failed { .. } => ErrorKind::Retryable,
            Self::CleanupUnknown(_) => ErrorKind::CleanupUnknown,
        }
    }
}

fn failed(operation: &'static str) -> KeepRunningError {
    KeepRunningError::Failed {
        operation,
        code: last_error(),
    }
}

fn check(ok: i32, operation: &'static str) -> Result<()> {
    if ok == 0 {
        Err(failed(operation))
    } else {
        Ok(())
    }
}

fn wide(value: &Path) -> Vec<u16> {
    value.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn wait(process: &WinHandle, milliseconds: u32) -> Result<bool> {
    // SAFETY: this retained process handle has SYNCHRONIZE access.
    match unsafe { WaitForSingleObject(process.raw(), milliseconds) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(failed("WaitForSingleObject")),
    }
}

fn image(process: &WinHandle) -> Result<PathBuf> {
    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    check(
        // SAFETY: process is queryable and buffer holds length writable UTF-16 elements.
        unsafe { QueryFullProcessImageNameW(process.raw(), 0, buffer.as_mut_ptr(), &mut length) },
        "QueryFullProcessImageNameW",
    )?;
    Ok(PathBuf::from(OsString::from_wide(
        &buffer[..length as usize],
    )))
}

/// Resolved file identity. This identifies the file currently at the queried image path; it does
/// not certify an unchanged loaded image section after an on-disk replacement.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ExecutableIdentity {
    volume: u64,
    file: [u8; 16],
}

fn file_identity(path: &Path) -> Result<ExecutableIdentity> {
    let path = wide(path);
    // SAFETY: path is NUL-terminated; the returned file handle is noninheritable and read-only.
    let raw = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(failed("CreateFileW(executable identity)"));
    }
    let file = WinHandle::new(raw);
    let mut info = FILE_ID_INFO::default();
    check(
        // SAFETY: info is an aligned writable FILE_ID_INFO and file is a retained file handle.
        unsafe {
            GetFileInformationByHandleEx(
                file.raw(),
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
        },
        "GetFileInformationByHandleEx(FileIdInfo)",
    )?;
    Ok(ExecutableIdentity {
        volume: info.VolumeSerialNumber,
        file: info.FileId.Identifier,
    })
}

#[derive(Clone)]
pub(crate) struct LaunchSpec {
    executable: PathBuf,
    arguments: String,
    directory: PathBuf,
    executable_identity: ExecutableIdentity,
}

impl LaunchSpec {
    pub(crate) fn new(executable: &Path, arguments: &str, directory: &Path) -> Result<Self> {
        if !executable.is_absolute() || !directory.is_absolute() || arguments.contains('\0') {
            return Err(KeepRunningError::Unsupported(
                "Keep Running requires absolute executable and working-directory paths without NUL characters.",
            ));
        }
        let executable = executable.canonicalize().map_err(|_| {
            KeepRunningError::Unsupported("The executable path could not be resolved.")
        })?;
        let directory = directory.canonicalize().map_err(|_| {
            KeepRunningError::Unsupported("The working directory could not be resolved.")
        })?;
        if !executable.is_file()
            || !directory.is_dir()
            || executable
                .extension()
                .and_then(|ext| ext.to_str())
                .is_none_or(|ext| !ext.eq_ignore_ascii_case("exe"))
        {
            return Err(KeepRunningError::Unsupported(
                "Keep Running requires an executable file and an existing working directory.",
            ));
        }
        let executable_identity = file_identity(&executable)?;
        let spec = Self {
            executable,
            arguments: arguments.to_owned(),
            directory,
            executable_identity,
        };
        spec.command_line()?;
        Ok(spec)
    }

    pub(crate) fn identity(&self) -> Result<ExecutableIdentity> {
        let current = file_identity(&self.executable)?;
        if current != self.executable_identity {
            return Err(KeepRunningError::Unsupported(
                "The configured executable file has changed; enable Keep Running again after reviewing it.",
            ));
        }
        Ok(current)
    }

    fn command_line(&self) -> Result<Vec<u16>> {
        let mut command = vec![b'"' as u16];
        command.extend(self.executable.as_os_str().encode_wide());
        command.push(b'"' as u16);
        if !self.arguments.is_empty() {
            command.push(b' ' as u16);
            command.extend(self.arguments.encode_utf16());
        }
        command.push(0);
        if command.len() > 32767 || command[..command.len() - 1].contains(&0) {
            return Err(KeepRunningError::Unsupported(
                "The Keep Running command line is invalid or exceeds Windows' length limit.",
            ));
        }
        Ok(command)
    }
}

fn token(process: HANDLE, access: u32) -> Result<WinHandle> {
    let mut result = ptr::null_mut();
    check(
        // SAFETY: process is a retained process or current-process pseudo-handle; result is writable.
        unsafe { OpenProcessToken(process, access, &mut result) },
        "OpenProcessToken",
    )?;
    Ok(WinHandle::new(result))
}

fn token_fixed<T: Default>(token: &WinHandle, class: TOKEN_INFORMATION_CLASS) -> Result<T> {
    let mut value = T::default();
    let mut bytes = 0;
    check(
        // SAFETY: internal call sites pair each class with its documented fixed-size native type.
        unsafe {
            GetTokenInformation(
                token.raw(),
                class,
                (&mut value as *mut T).cast(),
                size_of::<T>() as u32,
                &mut bytes,
            )
        },
        "GetTokenInformation(fixed)",
    )?;
    if bytes as usize != size_of::<T>() {
        return Err(KeepRunningError::Unsupported(
            "Windows returned an unexpected token-information layout.",
        ));
    }
    Ok(value)
}

fn token_buffer(token: &WinHandle, class: TOKEN_INFORMATION_CLASS) -> Result<Vec<usize>> {
    let mut bytes = 0;
    // SAFETY: a null zero-size buffer requests the required allocation length.
    unsafe { GetTokenInformation(token.raw(), class, ptr::null_mut(), 0, &mut bytes) };
    if bytes == 0 {
        return Err(failed("GetTokenInformation(size)"));
    }
    let mut storage = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
    check(
        // SAFETY: storage is naturally aligned with at least bytes writable bytes.
        unsafe {
            GetTokenInformation(
                token.raw(),
                class,
                storage.as_mut_ptr().cast(),
                bytes,
                &mut bytes,
            )
        },
        "GetTokenInformation(buffer)",
    )?;
    Ok(storage)
}

#[derive(PartialEq, Eq)]
struct TokenFacts {
    sid: Vec<u8>,
    service_account: bool,
    authentication: (u32, i32),
    session: u32,
    elevated: u32,
    elevation_type: u32,
    integrity: u32,
    restricted: u32,
    app_container: u32,
    ui_access: u32,
    groups: Vec<(Vec<u8>, u32)>,
    privileges: Vec<(u32, i32, u32)>,
}

impl TokenFacts {
    fn ordinary(&self) -> bool {
        self.elevated == 0
            && matches!(self.elevation_type, 1 | 3)
            && (0x2000..0x3000).contains(&self.integrity)
            && self.session != 0
            && !self.service_account
            && self.restricted == 0
            && self.app_container == 0
            && self.ui_access == 0
    }

    fn same_binding(&self, other: &Self) -> bool {
        self.sid == other.sid
            && self.authentication == other.authentication
            && self.session == other.session
    }
}

fn facts(token: &WinHandle) -> Result<TokenFacts> {
    let statistics: TOKEN_STATISTICS = token_fixed(token, TokenStatistics)?;
    let user = token_buffer(token, TokenUser)?;
    if size_of_val(user.as_slice()) < size_of::<TOKEN_USER>() {
        return Err(KeepRunningError::Unsupported(
            "Short native TokenUser response.",
        ));
    }
    // SAFETY: user is aligned and contains the complete native TOKEN_USER header.
    let user_sid = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let sid = sid_bytes(user_sid, &user)
        .map_err(KeepRunningError::Unsupported)?
        .to_vec();
    // SAFETY: sid_bytes validated this SID in retained native user-token storage.
    let service_account = unsafe {
        IsWellKnownSid(user_sid, WinLocalSystemSid) != 0
            || IsWellKnownSid(user_sid, WinLocalServiceSid) != 0
            || IsWellKnownSid(user_sid, WinNetworkServiceSid) != 0
    };
    let label = token_buffer(token, TokenIntegrityLevel)?;
    if size_of_val(label.as_slice()) < size_of::<TOKEN_MANDATORY_LABEL>() {
        return Err(KeepRunningError::Unsupported(
            "Short native integrity response.",
        ));
    }
    // SAFETY: label is aligned and contains the complete TOKEN_MANDATORY_LABEL header.
    let label_sid = unsafe { (*label.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid };
    sid_bytes(label_sid, &label).map_err(KeepRunningError::Unsupported)?;
    // SAFETY: sid_bytes validated the complete SID inside the retained label allocation.
    let count = unsafe { *GetSidSubAuthorityCount(label_sid) };
    if count == 0 {
        return Err(KeepRunningError::Unsupported(
            "The token has no integrity RID.",
        ));
    }
    // SAFETY: count is nonzero and the complete SID was validated above.
    let integrity = unsafe { *GetSidSubAuthority(label_sid, u32::from(count - 1)) };
    let restricted = token_buffer(token, TokenRestrictedSids)?;
    let group_storage = token_buffer(token, TokenGroups)?;
    let group_count = group_storage[0] as u32 as usize;
    let group_offset = offset_of!(TOKEN_GROUPS, Groups);
    if group_offset + group_count.saturating_mul(size_of::<SID_AND_ATTRIBUTES>())
        > size_of_val(group_storage.as_slice())
    {
        return Err(KeepRunningError::Unsupported(
            "Short native token-groups response.",
        ));
    }
    let mut groups = Vec::with_capacity(group_count);
    for index in 0..group_count {
        // SAFETY: the counted SID_AND_ATTRIBUTES array was checked against its retained allocation.
        let entry = unsafe {
            &*group_storage
                .as_ptr()
                .cast::<u8>()
                .add(group_offset)
                .cast::<SID_AND_ATTRIBUTES>()
                .add(index)
        };
        let sid = sid_bytes(entry.Sid, &group_storage).map_err(KeepRunningError::Unsupported)?;
        groups.push((sid.to_vec(), entry.Attributes));
    }
    groups.sort_unstable();
    let privilege_storage = token_buffer(token, TokenPrivileges)?;
    let privilege_count = privilege_storage[0] as u32 as usize;
    let privilege_offset = offset_of!(TOKEN_PRIVILEGES, Privileges);
    if privilege_offset + privilege_count.saturating_mul(size_of::<LUID_AND_ATTRIBUTES>())
        > size_of_val(privilege_storage.as_slice())
    {
        return Err(KeepRunningError::Unsupported(
            "Short native token-privileges response.",
        ));
    }
    let mut privileges = Vec::with_capacity(privilege_count);
    for index in 0..privilege_count {
        // SAFETY: the counted LUID_AND_ATTRIBUTES array was checked against its retained allocation.
        let entry = unsafe {
            &*privilege_storage
                .as_ptr()
                .cast::<u8>()
                .add(privilege_offset)
                .cast::<LUID_AND_ATTRIBUTES>()
                .add(index)
        };
        privileges.push((entry.Luid.LowPart, entry.Luid.HighPart, entry.Attributes));
    }
    privileges.sort_unstable();
    Ok(TokenFacts {
        sid,
        service_account,
        authentication: (
            statistics.AuthenticationId.LowPart,
            statistics.AuthenticationId.HighPart,
        ),
        session: token_fixed(token, TokenSessionId)?,
        elevated: token_fixed(token, TokenElevation)?,
        elevation_type: token_fixed(token, TokenElevationType)?,
        integrity,
        restricted: restricted[0] as u32,
        app_container: token_fixed(token, TokenIsAppContainer)?,
        ui_access: token_fixed(token, TokenUIAccess)?,
        groups,
        privileges,
    })
}

fn user_object_name(object: HANDLE) -> Result<String> {
    if object.is_null() {
        return Err(failed("GetThreadDesktop/GetProcessWindowStation"));
    }
    let mut bytes = 0;
    // SAFETY: the borrowed desktop/window-station handle is valid; zero length queries size.
    unsafe { GetUserObjectInformationW(object, UOI_NAME, ptr::null_mut(), 0, &mut bytes) };
    if bytes == 0 || bytes % 2 != 0 {
        return Err(failed("GetUserObjectInformationW(size)"));
    }
    let mut name = vec![0u16; bytes as usize / 2];
    check(
        // SAFETY: name has bytes writable bytes and object is a borrowed user-object handle.
        unsafe {
            GetUserObjectInformationW(
                object,
                UOI_NAME,
                name.as_mut_ptr().cast(),
                bytes,
                &mut bytes,
            )
        },
        "GetUserObjectInformationW",
    )?;
    let end = name
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(name.len());
    String::from_utf16(&name[..end])
        .map_err(|_| KeepRunningError::Unsupported("The desktop name is not valid UTF-16."))
}

pub(crate) struct DesktopContext {
    shell: WinHandle,
    shell_token: WinHandle,
    window: HWND,
    shell_id: u32,
    shell_creation: u64,
    shell_token_id: (u32, i32),
    facts: TokenFacts,
    desktop: String,
    binding: String,
}

impl DesktopContext {
    pub(crate) fn capture() -> Result<Self> {
        // SAFETY: these queries have no caller-side memory requirements and do not mutate desktop state.
        let window = unsafe { GetShellWindow() };
        if window.is_null() {
            return Err(KeepRunningError::Unsupported(
                "No desktop shell is available in this session.",
            ));
        }
        let mut shell_id = 0;
        // SAFETY: window is freshly selected and shell_id is writable.
        let shell_thread = unsafe { GetWindowThreadProcessId(window, &mut shell_id) };
        if shell_thread == 0 || shell_id == 0 {
            return Err(failed("GetWindowThreadProcessId(shell)"));
        }
        // SAFETY: the selected PID is from GetShellWindow; no handle inheritance or privilege adjustment.
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_CREATE_PROCESS,
                0,
                shell_id,
            )
        };
        if raw.is_null() {
            return Err(failed("OpenProcess(shell)"));
        }
        let shell = WinHandle::new(raw);
        let shell_creation = shell
            .process_times()
            .map_err(|code| KeepRunningError::Failed {
                operation: "GetProcessTimes(shell)",
                code,
            })?
            .0;
        let shell_token = token(shell.raw(), TOKEN_QUERY | TOKEN_DUPLICATE | READ_CONTROL)?;
        let statistics: TOKEN_STATISTICS = token_fixed(&shell_token, TokenStatistics)?;
        let shell_facts = facts(&shell_token)?;
        if !shell_facts.ordinary() {
            return Err(KeepRunningError::Unsupported(
                "Keep Running requires an ordinary interactive desktop shell.",
            ));
        }
        // SAFETY: current-process pseudo-handle requires no close and is valid for token query.
        let caller_token = token(unsafe { GetCurrentProcess() }, TOKEN_QUERY)?;
        let caller = facts(&caller_token)?;
        if caller.sid != shell_facts.sid || caller.session != shell_facts.session {
            return Err(KeepRunningError::Unsupported(
                "The caller and desktop shell do not belong to the same user and session.",
            ));
        }
        if !caller.same_binding(&shell_facts) {
            if caller.elevated != 1 || caller.elevation_type != 2 {
                return Err(KeepRunningError::Unsupported(
                    "The caller's logon does not match the desktop shell.",
                ));
            }
            let linked: TOKEN_LINKED_TOKEN = token_fixed(&caller_token, TokenLinkedToken)?;
            if linked.LinkedToken.is_null() {
                return Err(KeepRunningError::Unsupported(
                    "The elevated caller has no linked desktop token.",
                ));
            }
            let linked = WinHandle::new(linked.LinkedToken);
            let linked = facts(&linked)?;
            if !linked.ordinary()
                || linked.elevation_type != 3
                || !linked.same_binding(&shell_facts)
            {
                return Err(KeepRunningError::Unsupported(
                    "The elevated caller's linked token does not match this desktop shell.",
                ));
            }
        }
        // SAFETY: queried desktops/station are borrowed handles; this code never closes them.
        let (station, current_desktop, shell_desktop) = unsafe {
            (
                GetProcessWindowStation(),
                GetThreadDesktop(GetCurrentThreadId()),
                GetThreadDesktop(shell_thread),
            )
        };
        let station = user_object_name(station)?;
        let current_desktop = user_object_name(current_desktop)?;
        let shell_desktop = user_object_name(shell_desktop)?;
        if !current_desktop.eq_ignore_ascii_case(&shell_desktop) {
            return Err(KeepRunningError::Unsupported(
                "The shell is not on the caller's current desktop.",
            ));
        }
        let desktop = format!("{station}\\{shell_desktop}");
        let binding = format!(
            "{:02x?}|{}:{}|{}|{}",
            shell_facts.sid,
            shell_facts.authentication.0,
            shell_facts.authentication.1,
            shell_facts.session,
            desktop.to_lowercase()
        );
        let context = Self {
            shell,
            shell_token,
            window,
            shell_id,
            shell_creation,
            shell_token_id: (statistics.TokenId.LowPart, statistics.TokenId.HighPart),
            facts: shell_facts,
            desktop,
            binding,
        };
        context.verify_current()?;
        Ok(context)
    }

    /// Opaque security context: never include this value in logs or UI.
    pub(crate) fn binding_key(&self) -> &str {
        &self.binding
    }

    fn verify_current(&self) -> Result<()> {
        // SAFETY: GetShellWindow is a read-only desktop query.
        if unsafe { GetShellWindow() } != self.window || wait(&self.shell, 0)? {
            return Err(KeepRunningError::Unsupported(
                "The desktop shell changed during preparation.",
            ));
        }
        let mut id = 0;
        // SAFETY: self.window is the currently selected shell window and id is writable.
        unsafe { GetWindowThreadProcessId(self.window, &mut id) };
        let current_token = token(self.shell.raw(), TOKEN_QUERY)?;
        let current_statistics: TOKEN_STATISTICS = token_fixed(&current_token, TokenStatistics)?;
        if id != self.shell_id
            || self.shell.process_creation_time() != Some(self.shell_creation)
            || (
                current_statistics.TokenId.LowPart,
                current_statistics.TokenId.HighPart,
            ) != self.shell_token_id
            || facts(&current_token)? != self.facts
        {
            return Err(KeepRunningError::Unsupported(
                "The retained desktop shell identity changed.",
            ));
        }
        Ok(())
    }
}

pub(crate) struct Member {
    process: WinHandle,
    executable_identity: ExecutableIdentity,
}

impl Member {
    /// The control layer must first acquire this handle through its shared safety boundary.
    pub(crate) fn retain(process: WinHandle, context: &DesktopContext) -> Result<Self> {
        context.verify_current()?;
        let member_token = token(process.raw(), TOKEN_QUERY)?;
        let member_facts = facts(&member_token)?;
        // Equal canonical groups/privileges are deliberately conservative. This is not full token
        // equivalence (claims, policy, token-object security and loaded-image provenance differ).
        if !member_facts.ordinary() || member_facts != context.facts {
            return Err(KeepRunningError::Unsupported(
                "The app is not an eligible ordinary app in the retained desktop context.",
            ));
        }
        let executable_identity = file_identity(&image(&process)?)?;
        Ok(Self {
            process,
            executable_identity,
        })
    }

    pub(crate) fn executable_identity(&self) -> &ExecutableIdentity {
        &self.executable_identity
    }
    pub(crate) fn exit_confirmed(&self) -> Result<bool> {
        wait(&self.process, 0)
    }
}

pub(crate) struct ObservedProcess {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) creation_time: u64,
}

type CreationTimeResult = std::result::Result<u64, u32>;

enum ProcessProbe<Creation> {
    Exited,
    Unreadable(&'static str),
    Failed(KeepRunningError),
    Image {
        path: PathBuf,
        identity: ExecutableIdentity,
        creation: Creation,
    },
}

fn probe_process<Creation>(
    id: u32,
    creation_source: impl FnOnce(WinHandle) -> Creation,
) -> ProcessProbe<Creation> {
    // SAFETY: snapshot PID is used only for a noninheritable query/synchronize handle.
    let raw = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            id,
        )
    };
    if raw.is_null() {
        return ProcessProbe::Unreadable(
            "A possible matching app could not be inspected; absence is unknown.",
        );
    }
    let process = WinHandle::new(raw);
    match wait(&process, 0) {
        Ok(true) => return ProcessProbe::Exited,
        Err(error) => return ProcessProbe::Failed(error),
        Ok(false) => {}
    }
    match image(&process).and_then(|path| file_identity(&path).map(|identity| (path, identity))) {
        Ok((path, identity)) => ProcessProbe::Image {
            path,
            identity,
            creation: creation_source(process),
        },
        Err(_) => ProcessProbe::Unreadable(
            "A possible matching executable could not be verified; absence is unknown.",
        ),
    }
}

struct ExecutableMatch {
    identity: ExecutableIdentity,
    basename: Vec<u16>,
}

impl ExecutableMatch {
    fn candidate<Creation>(
        &self,
        id: u32,
        name: &[u16],
        probe: &ProcessProbe<Creation>,
        creation_time: impl FnOnce(&Creation) -> CreationTimeResult,
    ) -> Result<Option<ObservedProcess>> {
        match probe {
            ProcessProbe::Failed(error) => Err(error.clone()),
            ProcessProbe::Unreadable(reason)
                if super::ordinal_equal(name, &self.basename).unwrap_or(true) =>
            {
                Err(KeepRunningError::Unsupported(reason))
            }
            ProcessProbe::Image {
                path,
                identity,
                creation,
            } if *identity == self.identity => {
                let creation_time =
                    creation_time(creation).map_err(|code| KeepRunningError::Failed {
                        operation: "GetProcessTimes(observation)",
                        code,
                    })?;
                Ok(Some(ObservedProcess {
                    id,
                    name: String::from_utf16_lossy(name),
                    path: path.clone(),
                    creation_time,
                }))
            }
            _ => Ok(None),
        }
    }
}

#[derive(Default)]
pub(crate) struct ProcessCatalog(OnceCell<CatalogRows>);

struct CatalogRow {
    id: u32,
    name: Box<[u16]>,
    probe: OnceCell<ProcessProbe<CreationTimeResult>>,
}

struct CatalogRows {
    entries: Vec<CatalogRow>,
    completion: Result<()>,
}

impl CatalogRows {
    fn capture() -> Self {
        let mut entries = Vec::new();
        let completion = for_each_process(|id, name| {
            entries.push(CatalogRow {
                id,
                name: name.into(),
                probe: OnceCell::new(),
            });
            Ok(())
        });
        Self {
            entries,
            completion,
        }
    }
}

impl ProcessCatalog {
    fn matching(
        &self,
        expected: &ExecutableMatch,
        capture: impl FnOnce() -> CatalogRows,
        probe: impl Fn(u32) -> ProcessProbe<CreationTimeResult>,
    ) -> Result<Vec<ObservedProcess>> {
        let rows = self.0.get_or_init(capture);
        let mut matches = Vec::new();
        for row in &rows.entries {
            matches.extend(expected.candidate(
                row.id,
                &row.name,
                row.probe.get_or_init(|| probe(row.id)),
                |creation| *creation,
            )?);
        }
        rows.completion.clone()?;
        Ok(matches)
    }
}

pub(crate) fn matching_processes(
    spec: &LaunchSpec,
    catalog: Option<&ProcessCatalog>,
) -> Result<(ExecutableIdentity, Vec<ObservedProcess>)> {
    let expected = ExecutableMatch {
        identity: spec.identity()?,
        basename: spec
            .executable
            .file_name()
            .ok_or(KeepRunningError::Unsupported(
                "Missing executable filename.",
            ))?
            .encode_wide()
            .collect(),
    };
    let matches = if let Some(catalog) = catalog {
        catalog.matching(&expected, CatalogRows::capture, |id| {
            probe_process(id, |process| {
                process.process_times().map(|(creation, _)| creation)
            })
        })?
    } else {
        let mut matches = Vec::new();
        for_each_process(|id, name| {
            matches.extend(expected.candidate(
                id,
                name,
                &probe_process(id, |process| process),
                |process| process.process_times().map(|(creation, _)| creation),
            )?);
            Ok(())
        })?;
        matches
    };
    Ok((expected.identity, matches))
}

fn for_each_process(mut visit: impl FnMut(u32, &[u16]) -> Result<()>) -> Result<()> {
    // SAFETY: this read-only snapshot owns only a process-list handle.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(failed("CreateToolhelp32Snapshot"));
    }
    let snapshot = WinHandle::new(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: entry is the correctly sized writable process-snapshot record.
    let mut present = unsafe { Process32FirstW(snapshot.raw(), &mut entry) };
    while present != 0 {
        let length = entry
            .szExeFile
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(entry.szExeFile.len());
        if entry.th32ProcessID != 0 {
            visit(entry.th32ProcessID, &entry.szExeFile[..length])?;
        }
        // SAFETY: entry remains correctly sized writable snapshot storage.
        present = unsafe { Process32NextW(snapshot.raw(), &mut entry) };
    }
    // ERROR_NO_MORE_FILES is the only successful end-of-enumeration result.
    if last_error() != windows_sys::Win32::Foundation::ERROR_NO_MORE_FILES {
        return Err(failed("Process32NextW"));
    }
    Ok(())
}

struct AttributeList(Vec<usize>);

impl AttributeList {
    fn new() -> Result<Self> {
        let mut bytes = 0;
        // SAFETY: a null zero-size list requests the documented allocation length.
        unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 2, 0, &mut bytes) };
        if bytes == 0 {
            return Err(failed("InitializeProcThreadAttributeList(size)"));
        }
        let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        check(
            // SAFETY: storage is aligned with at least bytes writable bytes.
            unsafe {
                InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 2, 0, &mut bytes)
            },
            "InitializeProcThreadAttributeList",
        )?;
        Ok(Self(storage))
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: the successfully initialized list remains allocated until this deletion.
        unsafe { DeleteProcThreadAttributeList(self.0.as_mut_ptr().cast()) };
    }
}

struct Environment(*mut c_void);

impl Drop for Environment {
    fn drop(&mut self) {
        // SAFETY: this block is owned from a successful CreateEnvironmentBlock call.
        unsafe { DestroyEnvironmentBlock(self.0) };
    }
}

fn descriptor(token: &WinHandle) -> Result<Vec<usize>> {
    let information = DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION;
    let mut bytes = 0;
    // SAFETY: a null zero-size output requests the security descriptor's required allocation.
    unsafe { GetKernelObjectSecurity(token.raw(), information, ptr::null_mut(), 0, &mut bytes) };
    if bytes == 0 {
        return Err(failed("GetKernelObjectSecurity(size)"));
    }
    let mut storage = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
    check(
        // SAFETY: storage is aligned and sized; token has READ_CONTROL access.
        unsafe {
            GetKernelObjectSecurity(
                token.raw(),
                information,
                storage.as_mut_ptr().cast(),
                bytes,
                &mut bytes,
            )
        },
        "GetKernelObjectSecurity",
    )?;
    Ok(storage)
}

fn acl_bytes(descriptor: &[usize], sacl: bool) -> Result<(bool, Option<Vec<u8>>)> {
    let (mut present, mut defaulted) = (0, 0);
    let mut acl = ptr::null_mut();
    // SAFETY: descriptor is a retained aligned successful native security-descriptor response.
    let ok = unsafe {
        if sacl {
            GetSecurityDescriptorSacl(
                descriptor.as_ptr().cast_mut().cast(),
                &mut present,
                &mut acl,
                &mut defaulted,
            )
        } else {
            GetSecurityDescriptorDacl(
                descriptor.as_ptr().cast_mut().cast(),
                &mut present,
                &mut acl,
                &mut defaulted,
            )
        }
    };
    check(ok, "GetSecurityDescriptorAcl")?;
    if acl.is_null() {
        return Ok((present != 0, None));
    }
    let start = descriptor.as_ptr() as usize;
    let end = start + size_of_val(descriptor);
    let address = acl as usize;
    if address < start
        || address
            .checked_add(size_of::<windows_sys::Win32::Security::ACL>())
            .is_none_or(|value| value > end)
    {
        return Err(KeepRunningError::Unsupported(
            "Windows returned an invalid token ACL.",
        ));
    }
    // SAFETY: the ACL header lies within the retained native descriptor.
    let bytes = unsafe { (*acl).AclSize } as usize;
    if bytes < size_of::<windows_sys::Win32::Security::ACL>()
        || address.checked_add(bytes).is_none_or(|value| value > end)
    {
        return Err(KeepRunningError::Unsupported(
            "Windows returned a truncated token ACL.",
        ));
    }
    // SAFETY: the complete counted ACL lies within descriptor's retained storage.
    Ok((
        present != 0,
        Some(unsafe { std::slice::from_raw_parts(acl.cast::<u8>(), bytes) }.to_vec()),
    ))
}

fn job_flags(job: &WinHandle) -> Result<u32> {
    let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    check(
        // SAFETY: information is correctly sized writable storage and job is our retained private job.
        unsafe {
            QueryInformationJobObject(
                job.raw(),
                JobObjectExtendedLimitInformation,
                (&mut information as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ptr::null_mut(),
            )
        },
        "QueryInformationJobObject",
    )?;
    Ok(information.BasicLimitInformation.LimitFlags)
}

fn set_job_flags(job: &WinHandle, flags: u32) -> Result<()> {
    let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    information.BasicLimitInformation.LimitFlags = flags;
    check(
        // SAFETY: information has the documented layout; this job is unnamed and exclusively owned.
        unsafe {
            SetInformationJobObject(
                job.raw(),
                JobObjectExtendedLimitInformation,
                (&information as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        },
        "SetInformationJobObject",
    )
}

fn in_job(process: &WinHandle, job: &WinHandle) -> Result<bool> {
    let mut member = 0;
    check(
        // SAFETY: retained process and private job are queryable; member is writable.
        unsafe { IsProcessInJob(process.raw(), job.raw(), &mut member) },
        "IsProcessInJob",
    )?;
    Ok(member != 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisposeOutcome {
    Disposed,
    AlreadyAccepted,
}

pub(crate) struct PreparedLaunch {
    process: WinHandle,
    thread: WinHandle,
    job: WinHandle,
    context: DesktopContext,
    identity: Option<ObservedProcess>,
    executable_identity: ExecutableIdentity,
    resumed: bool,
    verified: bool,
    accepted: bool,
    disposed: bool,
    member_taken: bool,
}

impl PreparedLaunch {
    pub(crate) fn identity(&self) -> Option<&ObservedProcess> {
        self.identity.as_ref()
    }
    pub(crate) fn is_accepted(&self) -> bool {
        self.accepted
    }

    pub(crate) fn resume_accept(&mut self) -> Result<()> {
        if !self.verified || self.resumed || self.accepted || self.disposed {
            return Err(KeepRunningError::Unsupported(
                "The prepared launch is no longer available for a first resume.",
            ));
        }
        self.verify_current()?;
        // Mark before calling: even an unexpected suspend count must never cause a second resume.
        self.resumed = true;
        // SAFETY: thread is the exclusively owned primary thread created suspended by this object.
        if unsafe { ResumeThread(self.thread.raw()) } != 1 {
            return Err(failed("ResumeThread(prepared launch)"));
        }
        let flags = job_flags(&self.job)?;
        if flags != JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK {
            return Err(KeepRunningError::Unsupported(
                "The prepared launch job's safety flags changed.",
            ));
        }
        set_job_flags(&self.job, flags & !JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE)?;
        // Irreversible native acceptance is the successful disarm above, before readback or result
        // publication. An error after this assignment must never dispose/terminate the child.
        self.accepted = true;
        if job_flags(&self.job)? != JOB_OBJECT_LIMIT_BREAKAWAY_OK
            || !in_job(&self.process, &self.job)?
        {
            return Err(KeepRunningError::Unsupported(
                "The accepted launch's retained job membership could not be verified.",
            ));
        }
        Ok(())
    }

    fn verify_current(&self) -> Result<()> {
        self.context.verify_current()?;
        let identity = self.identity.as_ref().ok_or(KeepRunningError::Unsupported(
            "The prepared process identity was not verified.",
        ))?;
        if self.process.process_creation_time() != Some(identity.creation_time)
            || file_identity(&image(&self.process)?)? != self.executable_identity
            || wait(&self.process, 0)?
        {
            return Err(KeepRunningError::Unsupported(
                "The prepared process identity changed or exited.",
            ));
        }
        let child_token = token(self.process.raw(), TOKEN_QUERY | READ_CONTROL)?;
        if facts(&child_token)? != self.context.facts {
            return Err(KeepRunningError::Unsupported(
                "The prepared token no longer matches its desktop context.",
            ));
        }
        let child_security = descriptor(&child_token)?;
        let source_security = descriptor(&self.context.shell_token)?;
        if acl_bytes(&child_security, false)? != acl_bytes(&source_security, false)?
            || acl_bytes(&child_security, true)? != acl_bytes(&source_security, true)?
        {
            return Err(KeepRunningError::Unsupported(
                "The prepared token's security no longer matches the desktop token.",
            ));
        }
        // SAFETY: this retained child process has query rights from CreateProcessW.
        if unsafe { GetPriorityClass(self.process.raw()) } != NORMAL_PRIORITY_CLASS
            || !in_job(&self.process, &self.job)?
            || job_flags(&self.job)?
                != JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK
        {
            return Err(KeepRunningError::Unsupported(
                "The prepared process's priority or job safety changed.",
            ));
        }
        Ok(())
    }

    /// A positive exact-handle wait is the only cleanup acknowledgement. On uncertainty this
    /// object still retains every handle and the caller must quarantine it and retry disposal.
    pub(crate) fn dispose(&mut self) -> Result<DisposeOutcome> {
        if self.accepted {
            return Ok(DisposeOutcome::AlreadyAccepted);
        }
        if self.disposed {
            return Ok(DisposeOutcome::Disposed);
        }
        if wait(&self.process, 0).map_err(|_| KeepRunningError::CleanupUnknown(last_error()))? {
            self.disposed = true;
            return Ok(DisposeOutcome::Disposed);
        }
        // SAFETY: this is exclusively our own created, unaccepted process; original apps are never
        // opened with termination access. Descendants may already have escaped by explicit breakaway.
        let terminated = unsafe { TerminateProcess(self.process.raw(), 0x5eed) };
        let termination_error = if terminated == 0 {
            last_error()
        } else {
            WAIT_TIMEOUT
        };
        match wait(&self.process, 2000) {
            Ok(true) => {
                self.disposed = true;
                Ok(DisposeOutcome::Disposed)
            }
            Ok(false) => Err(KeepRunningError::CleanupUnknown(termination_error)),
            Err(_) => Err(KeepRunningError::CleanupUnknown(last_error())),
        }
    }

    /// Transfer observation of the same kernel process object, including an immediate exit after
    /// acceptance. No PID reopen and no missing-scan inference is involved.
    pub(crate) fn take_member(&mut self) -> Result<Option<Member>> {
        if !self.accepted || self.member_taken {
            return Ok(None);
        }
        let mut duplicate = ptr::null_mut();
        check(
            // SAFETY: both current-process pseudo-handles are valid; source is our retained child and
            // duplicate is writable. Only query/synchronize access, without inheritance, is requested.
            unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    self.process.raw(),
                    GetCurrentProcess(),
                    &mut duplicate,
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    0,
                )
            },
            "DuplicateHandle(accepted observation)",
        )?;
        self.member_taken = true;
        Ok(Some(Member {
            process: WinHandle::new(duplicate),
            executable_identity: self.executable_identity.clone(),
        }))
    }
}

// Dropping this object never waits. Closing its exclusive armed job is last-resort crash/shutdown
// containment, not a confirmed cleanup ACK. Accepted jobs are disarmed, so their roots survive.

pub(crate) struct PrepareFailure {
    pub(crate) error: KeepRunningError,
    pub(crate) cleanup: Option<Box<PreparedLaunch>>,
}

impl From<KeepRunningError> for PrepareFailure {
    fn from(error: KeepRunningError) -> Self {
        Self {
            error,
            cleanup: None,
        }
    }
}

pub(crate) fn prepare(
    spec: &LaunchSpec,
    expected_binding: &str,
) -> std::result::Result<Box<PreparedLaunch>, PrepareFailure> {
    let executable_identity = spec.identity()?;
    let context = DesktopContext::capture()?;
    if context.binding_key() != expected_binding {
        return Err(KeepRunningError::Unsupported(
            "The desktop context changed before preparation.",
        )
        .into());
    }
    let source_security = descriptor(&context.shell_token)?;
    let source_statistics: TOKEN_STATISTICS = token_fixed(&context.shell_token, TokenStatistics)?;
    // SAFETY: no security attributes/name means an unnamed, noninheritable private job.
    let raw_job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if raw_job.is_null() {
        return Err(failed("CreateJobObjectW").into());
    }
    let job = WinHandle::new(raw_job);
    let flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
    set_job_flags(&job, flags)?;
    if job_flags(&job)? != flags {
        return Err(KeepRunningError::Unsupported(
            "Private job safety flags could not be verified.",
        )
        .into());
    }
    let mut attributes = AttributeList::new()?;
    let mut parent = context.shell.raw();
    let mut private_job = job.raw();
    check(
        // SAFETY: attributes is initialized; parent points to a retained handle and remains alive until
        // CreateProcessW returns. No inherited child handle table entries are requested.
        unsafe {
            UpdateProcThreadAttribute(
                attributes.0.as_mut_ptr().cast(),
                0,
                PROC_THREAD_ATTRIBUTE_PARENT_PROCESS as usize,
                (&mut parent as *mut HANDLE).cast(),
                size_of::<HANDLE>(),
                ptr::null_mut(),
                ptr::null(),
            )
        },
        "UpdateProcThreadAttribute(parent)",
    )?;
    check(
        // SAFETY: the one-element job list references our retained job until synchronous creation ends;
        // assigning atomically avoids a suspended process existing outside the private cleanup job.
        unsafe {
            UpdateProcThreadAttribute(
                attributes.0.as_mut_ptr().cast(),
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                (&mut private_job as *mut HANDLE).cast(),
                size_of::<HANDLE>(),
                ptr::null_mut(),
                ptr::null(),
            )
        },
        "UpdateProcThreadAttribute(job list)",
    )?;
    let mut environment = ptr::null_mut();
    check(
        // SAFETY: shell_token is queryable/duplicable; FALSE builds a fresh shell-user environment and
        // does not copy the creator's environment. The returned block is exclusively owned.
        unsafe { CreateEnvironmentBlock(&mut environment, context.shell_token.raw(), 0) },
        "CreateEnvironmentBlock",
    )?;
    let environment = Environment(environment);
    let executable = wide(&spec.executable);
    let directory = wide(&spec.directory);
    let mut desktop: Vec<u16> = context.desktop.encode_utf16().chain(Some(0)).collect();
    let mut command = spec.command_line()?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.lpDesktop = desktop.as_mut_ptr();
    startup.lpAttributeList = attributes.0.as_mut_ptr().cast();
    let mut output = PROCESS_INFORMATION::default();
    context.verify_current()?;
    spec.identity()?;
    check(
        // SAFETY: all NUL-terminated strings, writable command line, environment and attribute backing
        // handles remain alive through this synchronous call. The executable is explicit, handles are
        // not inherited, and the private job is applied atomically with the verified shell parent.
        unsafe {
            CreateProcessW(
                executable.as_ptr(),
                command.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                CREATE_SUSPENDED
                    | CREATE_UNICODE_ENVIRONMENT
                    | EXTENDED_STARTUPINFO_PRESENT
                    | NORMAL_PRIORITY_CLASS,
                environment.0,
                directory.as_ptr(),
                &startup.StartupInfo,
                &mut output,
            )
        },
        "CreateProcessW(Keep Running)",
    )?;
    let mut prepared = Box::new(PreparedLaunch {
        process: WinHandle::new(output.hProcess),
        thread: WinHandle::new(output.hThread),
        job,
        context,
        identity: None,
        executable_identity,
        resumed: false,
        verified: false,
        accepted: false,
        disposed: false,
        member_taken: false,
    });
    let verified = (|| -> Result<()> {
        let path = image(&prepared.process)?;
        let creation_time = prepared
            .process
            .process_times()
            .map_err(|code| KeepRunningError::Failed {
                operation: "GetProcessTimes(prepared)",
                code,
            })?
            .0;
        if file_identity(&path)? != prepared.executable_identity {
            return Err(KeepRunningError::Unsupported(
                "The prepared image differs from the configured executable.",
            ));
        }
        let child_token = token(
            prepared.process.raw(),
            TOKEN_QUERY | READ_CONTROL | WRITE_DAC | WRITE_OWNER,
        )?;
        let child_statistics: TOKEN_STATISTICS = token_fixed(&child_token, TokenStatistics)?;
        if child_statistics.TokenId.LowPart == source_statistics.TokenId.LowPart
            && child_statistics.TokenId.HighPart == source_statistics.TokenId.HighPart
        {
            return Err(KeepRunningError::Unsupported(
                "The prepared launch unexpectedly shares its parent's token object.",
            ));
        }
        if facts(&child_token)? != prepared.context.facts {
            return Err(KeepRunningError::Unsupported(
                "The prepared launch did not inherit the ordinary desktop token.",
            ));
        }
        check(
            // SAFETY: child_token is a distinct token belonging only to our suspended child and has the
            // required WRITE_DAC/WRITE_OWNER rights. Source is a retained native descriptor, not settings.
            unsafe {
                SetKernelObjectSecurity(
                    child_token.raw(),
                    DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
                    source_security.as_ptr().cast_mut().cast(),
                )
            },
            "SetKernelObjectSecurity(prepared token)",
        )?;
        let actual_security = descriptor(&child_token)?;
        if acl_bytes(&actual_security, false)? != acl_bytes(&source_security, false)?
            || acl_bytes(&actual_security, true)? != acl_bytes(&source_security, true)?
        {
            return Err(KeepRunningError::Unsupported(
                "The prepared token's DACL or mandatory label differs from the desktop token.",
            ));
        }
        // SAFETY: prepared.process is retained with query rights from CreateProcessW.
        if unsafe { GetPriorityClass(prepared.process.raw()) } != NORMAL_PRIORITY_CLASS
            || !in_job(&prepared.process, &prepared.job)?
        {
            return Err(KeepRunningError::Unsupported(
                "The prepared launch's priority or private job membership is invalid.",
            ));
        }
        let name = path
            .file_name()
            .ok_or(KeepRunningError::Unsupported(
                "The prepared launch has no executable filename.",
            ))?
            .to_string_lossy()
            .into_owned();
        prepared.identity = Some(ObservedProcess {
            id: output.dwProcessId,
            name,
            path,
            creation_time,
        });
        prepared.verify_current()?;
        prepared.verified = true;
        Ok(())
    })();
    if let Err(error) = verified {
        return Err(match prepared.dispose() {
            Ok(_) => PrepareFailure {
                error,
                cleanup: None,
            },
            Err(cleanup_error) => PrepareFailure {
                error: cleanup_error,
                cleanup: Some(prepared),
            },
        });
    }
    Ok(prepared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs, thread,
        time::{Duration, Instant},
    };

    fn marker(id: u32) -> PathBuf {
        std::env::current_exe()
            .unwrap()
            .with_extension(format!("{id}.keep-running-child"))
    }

    struct Marker(PathBuf);
    impl Drop for Marker {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn shared_catalog_reuses_probes_and_keeps_uncertainty_rule_specific() {
        use std::cell::{Cell, RefCell};

        let selector = |file, basename: &str| ExecutableMatch {
            identity: ExecutableIdentity {
                volume: 1,
                file: [file; 16],
            },
            basename: basename.encode_utf16().collect(),
        };
        let first = selector(1, "selected.exe");
        let second = selector(2, "UNREADABLE.EXE");
        let third = selector(3, "changed.exe");
        let captures = Cell::new(0);
        let capture = || {
            captures.set(captures.get() + 1);
            CatalogRows {
                entries: [
                    "unrelated.exe",
                    "alias.exe",
                    "unreadable.exe",
                    "other-alias.exe",
                ]
                .into_iter()
                .enumerate()
                .map(|(index, name)| CatalogRow {
                    id: index as u32 + 1,
                    name: name.encode_utf16().collect(),
                    probe: OnceCell::new(),
                })
                .collect(),
                completion: Ok(()),
            }
        };
        let queried = RefCell::new(Vec::new());
        let probe = |id| {
            queried.borrow_mut().push(id);
            match id {
                2 => ProcessProbe::Image {
                    path: r"C:\Apps\alias.exe".into(),
                    identity: first.identity.clone(),
                    creation: Ok(10),
                },
                4 => ProcessProbe::Image {
                    path: r"C:\Apps\other-alias.exe".into(),
                    identity: third.identity.clone(),
                    creation: Err(5),
                },
                _ => ProcessProbe::Unreadable("read denied"),
            }
        };

        let catalog = ProcessCatalog::default();
        for _ in 0..2 {
            let matches = catalog.matching(&first, capture, probe).unwrap();
            assert_eq!(matches.len(), 1);
            assert_eq!((matches[0].id, matches[0].creation_time), (2, 10));
            assert_eq!(matches[0].path, Path::new(r"C:\Apps\alias.exe"));
        }
        assert!(matches!(
            catalog.matching(&second, capture, probe),
            Err(KeepRunningError::Unsupported("read denied"))
        ));
        assert!(matches!(
            catalog.matching(&third, capture, probe),
            Err(KeepRunningError::Failed {
                operation: "GetProcessTimes(observation)",
                code: 5
            })
        ));
        assert_eq!(captures.get(), 1);
        assert_eq!(*queried.borrow(), [1, 2, 3, 4]);
    }

    #[test]
    #[ignore = "Reads the native process catalog and executable file identities."]
    fn native_catalog_verifies_the_configured_identity() {
        use crate::platform::windows::process::{open, ProcessAccess};

        let executable = std::env::current_exe().expect("test executable path");
        let directory = executable.parent().expect("test executable directory");
        let mut spec = LaunchSpec::new(&executable, "", directory).expect("test launch spec");
        let pid = std::process::id();
        let process = open(pid, ProcessAccess::Observation).expect("open the test process");
        let creation_time = process
            .process_creation_time()
            .expect("test process creation time");

        let catalog = ProcessCatalog::default();
        for catalog in [None, Some(&catalog), Some(&catalog)] {
            let (identity, matches) =
                matching_processes(&spec, catalog).expect("scan the native catalog");
            assert!(identity == spec.executable_identity);
            let own = matches
                .iter()
                .find(|candidate| candidate.id == pid)
                .expect("find the test process");
            assert_eq!(own.creation_time, creation_time);
            assert!(file_identity(&own.path).expect("observed image file identity") == identity);
        }

        spec.executable_identity.file[0] ^= 1;
        let error = matching_processes(&spec, Some(&catalog))
            .err()
            .expect("stale file identity must fail the scan");
        assert_eq!(error.kind(), ErrorKind::Unsupported);
    }

    #[test]
    #[ignore = "Disposable subprocess fixture, invoked only by the native lifecycle test."]
    fn disposable_child() {
        fs::write(marker(std::process::id()), b"started").unwrap();
        thread::sleep(Duration::from_millis(1500));
    }

    #[test]
    #[ignore = "Requires the current user's interactive desktop; launches only this disposable test executable."]
    fn native_suspend_cancel_and_accept_exact_child() {
        let started = Instant::now();
        let context =
            DesktopContext::capture().unwrap_or_else(|error| panic!("desktop capture: {error}"));
        // SAFETY: the current-process pseudo-handle is valid for this read-only token query.
        let caller = facts(&token(unsafe { GetCurrentProcess() }, TOKEN_QUERY).unwrap()).unwrap();
        println!(
            "caller_elevated={} ordinary_source={}",
            caller.elevated != 0,
            context.facts.ordinary()
        );
        let executable = std::env::current_exe().unwrap();
        let fixture = format!(
            "--exact {}::disposable_child --ignored --nocapture",
            module_path!().split_once("::").unwrap().1
        );
        let spec = LaunchSpec::new(&executable, &fixture, executable.parent().unwrap()).unwrap();
        let mut canceled = prepare(&spec, context.binding_key())
            .unwrap_or_else(|failure| panic!("prepare cancellation: {}", failure.error));
        let canceled_marker = Marker(marker(canceled.identity().unwrap().id));
        let _ = fs::remove_file(&canceled_marker.0);
        assert!(!canceled_marker.0.exists());
        assert_eq!(canceled.dispose().unwrap(), DisposeOutcome::Disposed);
        assert!(
            !canceled_marker.0.exists(),
            "a canceled suspended child executed"
        );
        assert!(canceled.resume_accept().is_err());

        let mut accepted = prepare(&spec, context.binding_key())
            .unwrap_or_else(|failure| panic!("prepare acceptance: {}", failure.error));
        let accepted_marker = Marker(marker(accepted.identity().unwrap().id));
        let _ = fs::remove_file(&accepted_marker.0);
        accepted.resume_accept().unwrap();
        assert!(accepted.is_accepted());
        assert_eq!(
            job_flags(&accepted.job).unwrap(),
            JOB_OBJECT_LIMIT_BREAKAWAY_OK
        );
        assert!(in_job(&accepted.process, &accepted.job).unwrap());
        assert!(
            accepted.resume_accept().is_err(),
            "resume must remain one-shot"
        );
        assert_eq!(accepted.dispose().unwrap(), DisposeOutcome::AlreadyAccepted);
        let member = accepted.take_member().unwrap().unwrap();
        assert!(accepted.take_member().unwrap().is_none());
        assert!(!member.exit_confirmed().unwrap());
        drop(accepted);
        assert!(
            !member.exit_confirmed().unwrap(),
            "an accepted child died on owner drop"
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        while !member.exit_confirmed().unwrap() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            accepted_marker.0.exists(),
            "the accepted child did not execute"
        );
        assert!(
            member.exit_confirmed().unwrap(),
            "the finite disposable child did not exit"
        );
        println!("canceled_never_started=true accepted_survived_owner_drop=true exact_exit_confirmed=true elapsed_ms={}", started.elapsed().as_millis());
    }
}
