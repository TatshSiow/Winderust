//! One fresh, read-only catalog for all Instance Limits rules. Paths identify names, not files.
use std::{
    collections::{BTreeMap, BTreeSet},
    mem::size_of,
    ptr,
};

use windows_sys::Win32::{
    Foundation::{
        ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_FILES, FILETIME, HANDLE, INVALID_HANDLE_VALUE,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    Security::{
        GetLengthSid, GetTokenInformation, IsValidSid, TokenSessionId, TokenUser, TOKEN_QUERY,
        TOKEN_USER,
    },
    Storage::FileSystem::{
        CreateFileW, GetFinalPathNameByHandleW, FILE_ATTRIBUTE_NORMAL, FILE_NAME_NORMALIZED,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        VOLUME_NAME_DOS,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        SystemInformation::GetSystemTimeAsFileTime,
        Threading::{
            GetCurrentProcess, OpenProcessToken, QueryFullProcessImageNameW, WaitForSingleObject,
        },
    },
};

use crate::win_util::{filetime_to_u64, last_error, WinHandle};

use super::{
    ordinal_equal,
    process::{self, ProcessAccess},
};

type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CountObservation {
    Complete(u32),
    Incomplete {
        known_lower_bound: u32,
        reason: String,
    },
    Unavailable {
        reason: String,
    },
    Conflict,
}

/// Read-only membership evidence; the retained handle does not authorize mutation.
pub(crate) struct Member {
    pub(crate) id: u32,
    pub(crate) creation_time: u64,
    pub(crate) path: String,
    pub(crate) process: WinHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberState {
    Current,
    Exited,
    OutOfScope,
}

pub(crate) enum MembershipObservation {
    Complete(Vec<Member>),
    Incomplete {
        members: Vec<Member>,
        reason: String,
    },
    Unavailable {
        reason: String,
    },
    Conflict,
}

impl MembershipObservation {
    pub(crate) fn count(&self) -> CountObservation {
        match self {
            Self::Complete(members) => CountObservation::Complete(members.len() as u32),
            Self::Incomplete { members, reason } => CountObservation::Incomplete {
                known_lower_bound: members.len() as u32,
                reason: reason.clone(),
            },
            Self::Unavailable { reason } => CountObservation::Unavailable {
                reason: reason.clone(),
            },
            Self::Conflict => CountObservation::Conflict,
        }
    }
}

fn failure(operation: &str) -> String {
    let code = last_error();
    format!("{operation} failed (Windows error {code}).")
}

fn usable_basename(value: &[u16], terminated: bool) -> bool {
    terminated
        && !value.is_empty()
        && value.len() < 259
        && String::from_utf16(value).is_ok()
        && !value.iter().any(|value| {
            *value < 32
                || b"\\/:\"<>|?*"
                    .iter()
                    .any(|invalid| *value == u16::from(*invalid))
        })
        && !value
            .last()
            .is_some_and(|value| [u16::from(b'.'), u16::from(b' ')].contains(value))
}

fn normalized_path(path: &str) -> Result<Vec<u16>> {
    let path = path.replace('/', "\\");
    let value = path.encode_utf16().collect::<Vec<_>>();
    let drive = value.len() >= 3
        && value[0] <= 127
        && (value[0] as u8).is_ascii_alphabetic()
        && value[1..3] == [u16::from(b':'), u16::from(b'\\')];
    let unc = path.starts_with("\\\\");
    if value.len() > 32_767
        || (!drive && !unc)
        || path.starts_with("\\\\?\\")
        || path.starts_with("\\\\.\\")
    {
        return Err("A normal absolute drive or UNC executable path is required; device and extended paths are unsupported.".into());
    }
    let components = value[if drive { 3 } else { 2 }..]
        .split(|value| *value == u16::from(b'\\'))
        .collect::<Vec<_>>();
    if components.len() < if drive { 1 } else { 3 }
        || components.iter().any(|value| {
            !usable_basename(value, true)
                || value.len() > 255
                || String::from_utf16(value).is_ok_and(|name| reserved_component(&name))
        })
    {
        return Err("Executable paths require complete names without dot segments, control characters, trailing dots or spaces.".into());
    }
    Ok(value)
}

fn reserved_component(name: &str) -> bool {
    let name = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    matches!(
        name.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        name.strip_prefix(prefix).is_some_and(|number| {
            matches!(
                number,
                "1" | "2"
                    | "3"
                    | "4"
                    | "5"
                    | "6"
                    | "7"
                    | "8"
                    | "9"
                    | "\u{00b9}"
                    | "\u{00b2}"
                    | "\u{00b3}"
            )
        })
    })
}

pub(crate) fn validate_path(path: &str) -> Result<()> {
    normalized_path(path).map(|_| ())
}

pub(crate) fn paths_equal(left: &str, right: &str) -> Result<bool> {
    ordinal_equal(&normalized_path(left)?, &normalized_path(right)?)
}

fn normal_final_path(path: &str) -> Result<Vec<u16>> {
    let extended = path
        .strip_prefix("\\\\?\\")
        .ok_or("Windows returned an unsupported final path format.")?;
    let normal = if extended
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("UNC\\"))
    {
        format!("\\\\{}", &extended[4..])
    } else {
        extended.to_owned()
    };
    normalized_path(&normal)
}

fn verify_selected_path(path: &[u16]) -> Result<()> {
    let terminated = path.iter().copied().chain(Some(0)).collect::<Vec<_>>();
    // SAFETY: the validated, terminated path remains alive; this opens only existing read-only metadata.
    let raw = unsafe {
        CreateFileW(
            terminated.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(failure("CreateFileW(selected executable)"));
    }
    let file = WinHandle::new(raw);
    let mut buffer = vec![0u16; 32_768];
    // SAFETY: file is retained and buffer contains its declared writable UTF-16 capacity.
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.raw(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    } as usize;
    if length == 0 {
        return Err(failure("GetFinalPathNameByHandleW(selected executable)"));
    }
    if length >= buffer.len() {
        return Err("The selected executable's final path exceeds the supported length.".into());
    }
    let final_path = String::from_utf16(&buffer[..length])
        .map_err(|_| "The selected executable's final path is not valid Unicode.")?;
    if !ordinal_equal(path, &normal_final_path(&final_path)?)? {
        return Err("The selected path resolves to a different spelling. Short names, reparse aliases and remapped paths are unsupported; select the final executable path.".into());
    }
    Ok(())
}

struct Scope {
    sid: Vec<u8>,
    session: u32,
}

fn scope(process: HANDLE) -> Result<Scope> {
    let mut raw = ptr::null_mut();
    // SAFETY: process is a retained queryable process handle or the current-process pseudo-handle.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut raw) } == 0 {
        return Err(failure("OpenProcessToken"));
    }
    let token = WinHandle::new(raw);
    let mut bytes = 0;
    // SAFETY: a null zero-sized output requests only the required allocation length.
    let result =
        unsafe { GetTokenInformation(token.raw(), TokenUser, ptr::null_mut(), 0, &mut bytes) };
    let error = last_error();
    if result != 0 || error != ERROR_INSUFFICIENT_BUFFER || bytes < size_of::<TOKEN_USER>() as u32 {
        return Err(format!(
            "TokenUser size query failed (Windows error {error})."
        ));
    }
    let mut storage = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
    // SAFETY: storage is naturally aligned and has at least bytes writable bytes.
    if unsafe {
        GetTokenInformation(
            token.raw(),
            TokenUser,
            storage.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        )
    } == 0
    {
        return Err(failure("GetTokenInformation(TokenUser)"));
    }
    if bytes < size_of::<TOKEN_USER>() as u32 || bytes as usize > size_of_val(storage.as_slice()) {
        return Err("Windows returned an invalid TokenUser size.".into());
    }
    // SAFETY: the aligned response contains a complete TOKEN_USER and remains alive.
    let sid = unsafe { (*storage.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let start = storage.as_ptr() as usize;
    let end = start + bytes as usize;
    let address = sid as usize;
    if address < start || address.checked_add(8).is_none_or(|value| value > end) {
        return Err("Windows returned an invalid token SID.".into());
    }
    // SAFETY: the SID header lies within the initialized retained TokenUser response.
    let count = unsafe { *sid.cast::<u8>().add(1) } as usize;
    if address
        .checked_add(8 + count * 4)
        .is_none_or(|value| value > end)
    {
        return Err("Windows returned a truncated token SID.".into());
    }
    // SAFETY: the SID and all its declared subauthorities lie within the retained response.
    if unsafe { IsValidSid(sid) } == 0 {
        return Err("Windows returned an invalid token SID.".into());
    }
    // SAFETY: sid was validated and the complete response remains retained.
    let length = unsafe { GetLengthSid(sid) } as usize;
    // SAFETY: the validated SID's declared length is covered by the retained response.
    let sid = unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), length) }.to_vec();
    let mut session = 0u32;
    let mut returned = 0;
    // SAFETY: TokenSessionId returns one DWORD, and both output locations are writable.
    if unsafe {
        GetTokenInformation(
            token.raw(),
            TokenSessionId,
            (&mut session as *mut u32).cast(),
            size_of::<u32>() as u32,
            &mut returned,
        )
    } == 0
    {
        return Err(failure("GetTokenInformation(TokenSessionId)"));
    }
    if returned as usize != size_of::<u32>() {
        return Err("Windows returned an invalid TokenSessionId size.".into());
    }
    Ok(Scope { sid, session })
}

struct Row {
    id: u32,
    name: Option<Vec<u16>>,
}

fn catalog() -> Result<Vec<Row>> {
    // SAFETY: the process snapshot flag is read-only; its PID argument is ignored.
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(failure("CreateToolhelp32Snapshot"));
    }
    let snapshot = WinHandle::new(raw);
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: entry has the required size and the snapshot remains retained.
    let mut present = unsafe { Process32FirstW(snapshot.raw(), &mut entry) };
    let mut rows = Vec::new();
    while present != 0 {
        let end = entry.szExeFile.iter().position(|value| *value == 0);
        let name = end.and_then(|end| {
            let value = &entry.szExeFile[..end];
            usable_basename(value, true).then(|| value.to_vec())
        });
        rows.push(Row {
            id: entry.th32ProcessID,
            name,
        });
        // SAFETY: entry remains writable with its required size and snapshot remains retained.
        present = unsafe { Process32NextW(snapshot.raw(), &mut entry) };
    }
    let error = last_error();
    if error != ERROR_NO_MORE_FILES {
        return Err(format!(
            "Process enumeration failed (Windows error {error})."
        ));
    }
    Ok(rows)
}

fn process_path(process: &WinHandle) -> Result<String> {
    let mut buffer = vec![0u16; 32_768];
    let mut length = buffer.len() as u32;
    // SAFETY: process has query access and buffer contains length writable UTF-16 elements.
    if unsafe { QueryFullProcessImageNameW(process.raw(), 0, buffer.as_mut_ptr(), &mut length) }
        == 0
    {
        return Err(failure("QueryFullProcessImageNameW"));
    }
    let value = buffer
        .get(..length as usize)
        .ok_or("Windows returned an invalid image path length.")?;
    String::from_utf16(value).map_err(|_| "The process image path is not valid Unicode.".into())
}

fn running(process: &WinHandle) -> Result<bool> {
    // SAFETY: process has SYNCHRONIZE access; zero timeout only queries its signaled state.
    match unsafe { WaitForSingleObject(process.raw(), 0) } {
        WAIT_OBJECT_0 => Ok(false),
        WAIT_TIMEOUT => Ok(true),
        _ => Err(failure("WaitForSingleObject")),
    }
}

/// Refreshes read-only liveness and user/session scope, without granting mutation authority.
pub(crate) fn member_state(member: &Member) -> Result<MemberState> {
    if !running(&member.process)? {
        return Ok(MemberState::Exited);
    }
    let matching_scope = (|| -> Result<bool> {
        // SAFETY: GetCurrentProcess returns a queryable pseudo-handle with no caller requirements.
        let expected = scope(unsafe { GetCurrentProcess() })?;
        let actual = scope(member.process.raw())?;
        Ok(actual.sid == expected.sid && actual.session == expected.session)
    })();
    if !running(&member.process)? {
        return Ok(MemberState::Exited);
    }
    Ok(if matching_scope? {
        MemberState::Current
    } else {
        MemberState::OutOfScope
    })
}

struct Rule {
    path: Vec<u16>,
    basename: Vec<u16>,
    members: BTreeMap<(u32, u64), Member>,
    issue: Option<String>,
}

enum CandidateError {
    Uncertain(String),
    Contradiction(&'static str),
}

impl From<String> for CandidateError {
    fn from(reason: String) -> Self {
        Self::Uncertain(reason)
    }
}

impl From<&str> for CandidateError {
    fn from(reason: &str) -> Self {
        Self::Uncertain(reason.into())
    }
}

fn relevant_rules(row: &Row, rules: &[Option<Rule>]) -> Result<Vec<usize>> {
    rules
        .iter()
        .enumerate()
        .filter_map(|(index, rule)| {
            let rule = rule.as_ref()?;
            match &row.name {
                None => Some(Ok(index)),
                Some(name) => match ordinal_equal(name, &rule.basename) {
                    Ok(true) => Some(Ok(index)),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                },
            }
        })
        .collect()
}

/// The callback applies shared process safety checks using the same retained handle.
pub(crate) fn collect(
    paths: &[String],
    safety: impl Fn(u32, &str, u64, &WinHandle) -> Result<()>,
) -> Vec<MembershipObservation> {
    let mut results = Vec::with_capacity(paths.len());
    let mut rules = Vec::with_capacity(paths.len());
    for path in paths {
        match normalized_path(path) {
            Ok(path) => {
                let basename = path
                    .rsplit(|value| *value == u16::from(b'\\'))
                    .next()
                    .unwrap_or_default()
                    .to_vec();
                rules.push(Some(Rule {
                    path,
                    basename,
                    members: BTreeMap::new(),
                    issue: None,
                }));
                results.push(MembershipObservation::Complete(Vec::new()));
            }
            Err(reason) => {
                rules.push(None);
                results.push(MembershipObservation::Unavailable { reason });
            }
        }
    }
    let outcome = collect_pass(&mut rules, &mut results, safety);
    match outcome {
        Err(reason) => {
            for (index, rule) in rules.iter().enumerate() {
                if rule.is_some() {
                    results[index] = MembershipObservation::Unavailable {
                        reason: reason.clone(),
                    };
                }
            }
        }
        Ok(()) => {
            for (index, rule) in rules.into_iter().enumerate() {
                if let Some(rule) = rule {
                    let members = rule.members.into_values().collect();
                    results[index] = match rule.issue {
                        Some(reason) => MembershipObservation::Incomplete { members, reason },
                        None => MembershipObservation::Complete(members),
                    };
                }
            }
        }
    }
    results
}

fn collect_pass(
    rules: &mut [Option<Rule>],
    results: &mut [MembershipObservation],
    safety: impl Fn(u32, &str, u64, &WinHandle) -> Result<()>,
) -> Result<()> {
    let mut conflicts = BTreeSet::new();
    for (index, rule) in rules.iter().enumerate() {
        let Some(rule) = rule else { continue };
        for (other_index, other) in rules.iter().enumerate().skip(index + 1) {
            if let Some(other) = other {
                if ordinal_equal(&rule.path, &other.path)? {
                    conflicts.extend([index, other_index]);
                }
            }
        }
    }
    for index in conflicts {
        rules[index] = None;
        results[index] = MembershipObservation::Conflict;
    }
    for (index, rule) in rules.iter_mut().enumerate() {
        if let Some(active) = rule {
            if let Err(reason) = verify_selected_path(&active.path) {
                *rule = None;
                results[index] = MembershipObservation::Unavailable { reason };
            }
        }
    }
    if rules.iter().all(Option::is_none) {
        return Ok(());
    }
    // SAFETY: GetCurrentProcess returns a read-only pseudo-handle with no caller requirements.
    let expected = scope(unsafe { GetCurrentProcess() })?;
    let mut started = FILETIME::default();
    // SAFETY: started is writable FILETIME storage.
    unsafe { GetSystemTimeAsFileTime(&mut started) };
    let started = filetime_to_u64(started);
    let rows = catalog()?;
    for row in rows {
        let mut relevant = relevant_rules(&row, rules)?;
        if relevant.is_empty() {
            continue;
        }
        let observed = (|| -> std::result::Result<(), CandidateError> {
            let (process, synchronized) = match process::open(row.id, ProcessAccess::Observation) {
                Ok(process) => (process, true),
                Err(_) => (
                    process::open(row.id, ProcessAccess::SafetyOnly).map_err(|error| {
                        format!("A potentially matching process could not be queried: {error:?}.")
                    })?,
                    false,
                ),
            };
            if synchronized {
                if !running(&process)? {
                    return Ok(());
                }
            } else if process::exit_confirmed(&process).map_err(|error| format!("{error:?}"))? {
                return Ok(());
            }
            let created = process
                .process_times()
                .map_err(|code| format!("GetProcessTimes failed (Windows error {code})."))?
                .0;
            if created > started {
                return Err("A process generation changed during the catalog pass.".into());
            }
            let member = Member {
                id: row.id,
                creation_time: created,
                path: process_path(&process)?,
                process,
            };
            let basename = member
                .path
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or_default()
                .encode_utf16()
                .collect::<Vec<_>>();
            if let Some(name) = &row.name {
                if !ordinal_equal(name, &basename)? {
                    return Err(CandidateError::Contradiction(
                        "The process catalog and queried image names disagree; counts are unavailable for this pass.",
                    ));
                }
            }
            let image = normalized_path(&member.path)?;
            let mut matching = Vec::new();
            for index in &relevant {
                let rule = rules[*index]
                    .as_ref()
                    .ok_or("Instance Limits lost an active rule during collection.")?;
                if ordinal_equal(&image, &rule.path)? {
                    matching.push(*index)
                }
            }
            relevant = matching;
            let index =
                match relevant.as_slice() {
                    [] => return Ok(()),
                    [index] => *index,
                    _ => return Err(CandidateError::Contradiction(
                        "Equivalent executable paths survived Instance Limits conflict detection.",
                    )),
                };
            let actual = scope(member.process.raw())?;
            if actual.sid != expected.sid || actual.session != expected.session {
                return Ok(());
            }
            if !synchronized {
                return Err("A matching process lacks verified liveness; query-only access cannot prove a live count.".into());
            }
            safety(
                member.id,
                &member.path,
                member.creation_time,
                &member.process,
            )?;
            if !running(&member.process)? {
                return Ok(());
            }
            let rule = rules[index]
                .as_mut()
                .ok_or("Instance Limits lost an active rule during collection.")?;
            rule.members
                .insert((member.id, member.creation_time), member);
            Ok(())
        })();
        match observed {
            Err(CandidateError::Contradiction(reason)) => return Err(reason.into()),
            Err(CandidateError::Uncertain(reason)) => {
                for index in relevant {
                    let rule = rules[index]
                        .as_mut()
                        .ok_or("Instance Limits lost an active rule during collection.")?;
                    rule.issue.get_or_insert_with(|| reason.clone());
                }
            }
            Ok(()) => {}
        }
    }
    // A later candidate query may outlive an earlier member; refresh every retained positive.
    for rule in rules.iter_mut().flatten() {
        rule.members
            .retain(|_, member| match running(&member.process) {
                Ok(true) => true,
                Ok(false) => false,
                Err(reason) => {
                    rule.issue.get_or_insert(reason);
                    false
                }
            });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_projection_preserves_uncertain_and_conflicting_membership() {
        for (members, expected) in [
            (
                MembershipObservation::Complete(Vec::new()),
                CountObservation::Complete(0),
            ),
            (
                MembershipObservation::Incomplete {
                    members: Vec::new(),
                    reason: "A plausible member could not be queried.".into(),
                },
                CountObservation::Incomplete {
                    known_lower_bound: 0,
                    reason: "A plausible member could not be queried.".into(),
                },
            ),
            (
                MembershipObservation::Unavailable {
                    reason: "The process catalog is unavailable.".into(),
                },
                CountObservation::Unavailable {
                    reason: "The process catalog is unavailable.".into(),
                },
            ),
            (MembershipObservation::Conflict, CountObservation::Conflict),
        ] {
            assert_eq!(members.count(), expected);
        }
    }

    #[test]
    fn normal_paths_compare_by_ordinal_case_and_separators_without_alias_resolution() {
        assert!(paths_equal(
            "C:/Apps/\u{00e9}\u{6d4b}\u{8bd5}.exe",
            "c:\\apps\\\u{00c9}\u{6d4b}\u{8bd5}.EXE"
        )
        .unwrap());
        assert!(!paths_equal("C:\\Apps\\app.exe", "C:\\Other\\app.exe").unwrap());
        assert!(validate_path("\\\\server\\share\\app.exe").is_ok());
        for path in [
            "app.exe",
            "C:app.exe",
            "\\app.exe",
            "C:\\Apps\\..\\app.exe",
            "C:\\Apps\\.\\app.exe",
            "C:\\Apps\\app.exe ",
            "C:\\Apps\\app.exe.",
            "C:\\Apps\\app.exe\0",
            "\\\\?\\C:\\app.exe",
            "\\\\.\\C:\\app.exe",
            "\\\\server\\share",
            "C:\\Apps\\\\app.exe",
            "C:\\Apps\\NUL.exe",
            "C:\\COM\u{00b9}\\app.exe",
        ] {
            assert!(validate_path(path).is_err(), "{path:?}");
        }
    }

    #[test]
    fn missing_invalid_or_truncated_names_cannot_certify_absence() {
        for (name, terminated) in [
            (vec![], true),
            (vec![0xd800], true),
            (vec![b'x' as u16; 259], true),
            (vec![b'x' as u16], false),
            ("C:\\app.exe".encode_utf16().collect(), true),
        ] {
            assert!(!usable_basename(&name, terminated));
        }
        let rules = vec![Some(Rule {
            path: Vec::new(),
            basename: "app.exe".encode_utf16().collect(),
            members: BTreeMap::new(),
            issue: None,
        })];
        assert_eq!(
            relevant_rules(&Row { id: 1, name: None }, &rules).unwrap(),
            [0]
        );
        assert!(relevant_rules(
            &Row {
                id: 1,
                name: Some("other.exe".encode_utf16().collect())
            },
            &rules
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn equivalent_rules_are_conflicts_without_native_enumeration() {
        assert_eq!(
            collect(
                &["C:/Apps/app.exe".into(), "c:\\apps\\APP.exe".into()],
                |_, _, _, _| panic!("conflicts cannot acquire targets")
            )
            .iter()
            .map(MembershipObservation::count)
            .collect::<Vec<_>>(),
            vec![CountObservation::Conflict, CountObservation::Conflict]
        );
    }

    #[test]
    fn final_path_prefixes_only_convert_documented_drive_and_unc_forms() {
        assert_eq!(
            normal_final_path(r"\\?\C:\Apps\app.exe").unwrap(),
            normalized_path(r"C:\Apps\app.exe").unwrap()
        );
        assert_eq!(
            normal_final_path(r"\\?\UNC\server\share\app.exe").unwrap(),
            normalized_path(r"\\server\share\app.exe").unwrap()
        );
        for path in [
            r"C:\Apps\app.exe",
            r"\\?\Volume{1234}\app.exe",
            r"\Device\HarddiskVolume1\app.exe",
            r"\\?\C:\Apps\..\app.exe",
        ] {
            assert!(normal_final_path(path).is_err(), "{path}");
        }
    }
}
