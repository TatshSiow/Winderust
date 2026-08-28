use std::{
    collections::{BTreeMap, BTreeSet},
    mem::size_of,
    ptr::null_mut,
};

use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, ERROR_NO_MORE_ITEMS,
        FILETIME, HANDLE, INVALID_HANDLE_VALUE, STILL_ACTIVE,
    },
    System::{
        Diagnostics::{
            ProcessSnapshotting::{
                PssCaptureSnapshot, PssFreeSnapshot, PssWalkMarkerCreate, PssWalkMarkerFree,
                PssWalkSnapshot, HPSS, HPSSWALK, PSS_CAPTURE_THREADS, PSS_THREAD_ENTRY,
                PSS_WALK_THREADS,
            },
            ToolHelp::{
                CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
                THREADENTRY32,
            },
        },
        Threading::{
            GetCurrentProcess, GetExitCodeThread, GetProcessIdOfThread, GetThreadTimes, OpenThread,
            ResumeThread, SuspendThread, THREAD_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
        },
    },
};

use crate::win_util::{filetime_to_u64, last_error, WinHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CapturedThread {
    pub(crate) id: u32,
    pub(crate) creation_time: u64,
    pub(crate) suspend_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ThreadSuspensionError {
    AccessDenied {
        thread_id: Option<u32>,
    },
    ProcessExited,
    ThreadExited {
        thread_id: u32,
    },
    IdentityChanged {
        thread_id: u32,
    },
    SuspendCountConflict {
        thread_id: u32,
        expected: u32,
        actual: u32,
    },
    Failed {
        operation: &'static str,
        thread_id: Option<u32>,
        code: u32,
    },
}

pub(crate) struct ThreadHandle {
    id: u32,
    handle: WinHandle,
}

impl ThreadHandle {
    pub(crate) fn id(&self) -> u32 {
        self.id
    }

    pub(crate) fn raw(&self) -> HANDLE {
        self.handle.raw()
    }
}

pub(crate) fn capture_threads(
    process: HANDLE,
) -> Result<Vec<CapturedThread>, ThreadSuspensionError> {
    let mut snapshot = null_mut();
    // SAFETY: process is a caller-owned process handle and snapshot is writable for the call.
    let code = unsafe { PssCaptureSnapshot(process, PSS_CAPTURE_THREADS, 0, &mut snapshot) };
    if code != 0 {
        return Err(error_from_code("PssCaptureSnapshot", None, code));
    }
    let snapshot = Snapshot { snapshot };

    let mut marker = null_mut();
    // SAFETY: null requests the default allocator and marker is writable for the returned handle.
    let code = unsafe { PssWalkMarkerCreate(std::ptr::null(), &mut marker) };
    if code != 0 {
        return Err(error_from_code("PssWalkMarkerCreate", None, code));
    }
    let marker = WalkMarker { marker };
    let mut threads = Vec::new();

    loop {
        let mut entry = PSS_THREAD_ENTRY::default();
        // SAFETY: snapshot and marker remain live; entry is writable for exactly its declared size.
        let code = unsafe {
            PssWalkSnapshot(
                snapshot.snapshot,
                PSS_WALK_THREADS,
                marker.marker,
                (&mut entry as *mut PSS_THREAD_ENTRY).cast(),
                size_of::<PSS_THREAD_ENTRY>() as u32,
            )
        };
        if code == 0 {
            threads.push(captured_thread_from_entry(&entry));
        } else if code == ERROR_NO_MORE_ITEMS {
            return Ok(threads);
        } else {
            return Err(error_from_code("PssWalkSnapshot", None, code));
        }
    }
}

pub(crate) fn exact_suspend_count(
    process: HANDLE,
    thread_id: u32,
    thread_creation_time: u64,
) -> Result<u16, ThreadSuspensionError> {
    let Some(thread) = capture_threads(process)?
        .into_iter()
        .find(|thread| thread.id == thread_id)
    else {
        return Err(ThreadSuspensionError::ThreadExited { thread_id });
    };
    if thread.creation_time != thread_creation_time {
        return Err(ThreadSuspensionError::IdentityChanged { thread_id });
    }
    Ok(thread.suspend_count)
}

pub(crate) fn thread_ids_by_process(
    process_ids: &BTreeSet<u32>,
) -> Result<BTreeMap<u32, BTreeSet<u32>>, ThreadSuspensionError> {
    // SAFETY: TH32CS_SNAPTHREAD ignores the process id argument and returns an owned handle.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(capture_error("CreateToolhelp32Snapshot", None));
    }
    let snapshot = WinHandle::new(snapshot);
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    let mut result = BTreeMap::<u32, BTreeSet<u32>>::new();

    // SAFETY: snapshot is live and entry declares its size and remains writable.
    let mut present = unsafe { Thread32First(snapshot.raw(), &mut entry) };
    while present != 0 {
        if process_ids.contains(&entry.th32OwnerProcessID) {
            result
                .entry(entry.th32OwnerProcessID)
                .or_default()
                .insert(entry.th32ThreadID);
        }
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        // SAFETY: snapshot remains live and entry remains writable for the next record.
        present = unsafe { Thread32Next(snapshot.raw(), &mut entry) };
    }
    let code = last_error();
    if code == ERROR_NO_MORE_FILES {
        Ok(result)
    } else {
        Err(error_from_code("Thread enumeration", None, code))
    }
}

pub(crate) fn open_exact_thread(
    process_id: u32,
    thread: CapturedThread,
) -> Result<ThreadHandle, ThreadSuspensionError> {
    // SAFETY: thread.id came from a captured process snapshot and the handle is not inheritable.
    let raw = unsafe {
        OpenThread(
            THREAD_QUERY_LIMITED_INFORMATION | THREAD_SUSPEND_RESUME,
            0,
            thread.id,
        )
    };
    if raw.is_null() {
        return Err(capture_error("OpenThread", Some(thread.id)));
    }
    let handle = ThreadHandle {
        id: thread.id,
        handle: WinHandle::new(raw),
    };

    // SAFETY: handle owns a live thread handle opened with query access.
    let actual_process_id = unsafe { GetProcessIdOfThread(handle.raw()) };
    if actual_process_id == 0 {
        return Err(capture_error("GetProcessIdOfThread", Some(thread.id)));
    }
    if actual_process_id != process_id {
        return Err(ThreadSuspensionError::IdentityChanged {
            thread_id: thread.id,
        });
    }

    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: handle owns a live thread handle and every FILETIME output is writable.
    let ok = unsafe {
        GetThreadTimes(
            handle.raw(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    };
    if ok == 0 {
        return Err(capture_error("GetThreadTimes", Some(thread.id)));
    }
    if filetime_to_u64(creation) != thread.creation_time {
        return Err(ThreadSuspensionError::IdentityChanged {
            thread_id: thread.id,
        });
    }

    Ok(handle)
}

pub(crate) fn suspend_once(thread: &ThreadHandle) -> Result<u32, ThreadSuspensionError> {
    // SAFETY: thread owns a live handle opened with THREAD_SUSPEND_RESUME access.
    let count = unsafe { SuspendThread(thread.raw()) };
    if count == u32::MAX {
        Err(capture_error("SuspendThread", Some(thread.id())))
    } else {
        Ok(count)
    }
}

pub(crate) fn resume_once(thread: &ThreadHandle) -> Result<u32, ThreadSuspensionError> {
    // SAFETY: thread owns a live handle opened with THREAD_SUSPEND_RESUME access.
    let count = unsafe { ResumeThread(thread.raw()) };
    if count == u32::MAX {
        Err(capture_error("ResumeThread", Some(thread.id())))
    } else {
        Ok(count)
    }
}

pub(crate) fn is_active(thread: &ThreadHandle) -> Result<bool, ThreadSuspensionError> {
    let mut exit_code = 0;
    // SAFETY: thread owns a live query-limited handle and exit_code is writable for the call.
    let succeeded = unsafe { GetExitCodeThread(thread.raw(), &mut exit_code) } != 0;
    let error_code = (!succeeded).then(last_error).unwrap_or_default();
    thread_active_from_result(succeeded, exit_code, error_code, thread.id())
}

fn thread_active_from_result(
    succeeded: bool,
    exit_code: u32,
    error_code: u32,
    thread_id: u32,
) -> Result<bool, ThreadSuspensionError> {
    if succeeded {
        Ok(exit_code == STILL_ACTIVE as u32)
    } else {
        Err(error_from_code(
            "GetExitCodeThread",
            Some(thread_id),
            error_code,
        ))
    }
}

fn captured_thread_from_entry(entry: &PSS_THREAD_ENTRY) -> CapturedThread {
    CapturedThread {
        id: entry.ThreadId,
        creation_time: filetime_to_u64(entry.CreateTime),
        suspend_count: entry.SuspendCount,
    }
}

fn capture_error(operation: &'static str, thread_id: Option<u32>) -> ThreadSuspensionError {
    error_from_code(operation, thread_id, last_error())
}

fn error_from_code(
    operation: &'static str,
    thread_id: Option<u32>,
    code: u32,
) -> ThreadSuspensionError {
    match (code, thread_id) {
        (ERROR_ACCESS_DENIED, thread_id) => ThreadSuspensionError::AccessDenied { thread_id },
        (ERROR_INVALID_PARAMETER, Some(thread_id)) => {
            ThreadSuspensionError::ThreadExited { thread_id }
        }
        (ERROR_INVALID_PARAMETER, None) if operation == "PssCaptureSnapshot" => {
            ThreadSuspensionError::ProcessExited
        }
        (code, thread_id) => ThreadSuspensionError::Failed {
            operation,
            thread_id,
            code,
        },
    }
}

struct Snapshot {
    snapshot: HPSS,
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        // SAFETY: this locally captured snapshot must be freed by the current process; the guard
        // owns it and invokes PssFreeSnapshot exactly once.
        unsafe {
            PssFreeSnapshot(GetCurrentProcess(), self.snapshot);
        }
    }
}

struct WalkMarker {
    marker: HPSSWALK,
}

impl Drop for WalkMarker {
    fn drop(&mut self) {
        // SAFETY: PssWalkMarkerCreate returned this marker; this guard frees it once.
        unsafe {
            PssWalkMarkerFree(self.marker);
        }
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::{
        Foundation::FILETIME, System::Diagnostics::ProcessSnapshotting::PSS_THREAD_ENTRY,
    };

    use super::*;

    #[test]
    fn captured_thread_preserves_snapshot_identity_and_suspend_count() {
        let entry = PSS_THREAD_ENTRY {
            ThreadId: 41,
            CreateTime: FILETIME {
                dwLowDateTime: 0x89ab_cdef,
                dwHighDateTime: 0x0123_4567,
            },
            SuspendCount: 7,
            ..PSS_THREAD_ENTRY::default()
        };

        assert_eq!(
            captured_thread_from_entry(&entry),
            CapturedThread {
                id: 41,
                creation_time: 0x0123_4567_89ab_cdef,
                suspend_count: 7,
            }
        );
    }

    #[test]
    fn pss_walk_invalid_parameter_is_not_a_process_exit() {
        assert_eq!(
            error_from_code("PssWalkSnapshot", None, ERROR_INVALID_PARAMETER),
            ThreadSuspensionError::Failed {
                operation: "PssWalkSnapshot",
                thread_id: None,
                code: ERROR_INVALID_PARAMETER,
            }
        );
    }

    #[test]
    fn thread_liveness_distinguishes_active_exit_and_query_failure() {
        assert_eq!(
            thread_active_from_result(true, STILL_ACTIVE as u32, 0, 41),
            Ok(true)
        );
        assert_eq!(thread_active_from_result(true, 0, 0, 41), Ok(false));
        assert_eq!(
            thread_active_from_result(false, 0, ERROR_ACCESS_DENIED, 41),
            Err(ThreadSuspensionError::AccessDenied {
                thread_id: Some(41)
            })
        );
    }
}
