use std::mem::size_of;

use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, HANDLE, INVALID_HANDLE_VALUE,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        },
        Threading::{
            GetProcessIdOfThread, GetThreadPriority, GetThreadTimes, OpenThread, SetThreadPriority,
            THREAD_PRIORITY_ABOVE_NORMAL, THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_HIGHEST,
            THREAD_PRIORITY_IDLE, THREAD_PRIORITY_LOWEST, THREAD_PRIORITY_NORMAL,
            THREAD_PRIORITY_TIME_CRITICAL, THREAD_QUERY_INFORMATION, THREAD_SET_INFORMATION,
        },
    },
};

use crate::win_util::{filetime_to_u64, last_error, WinHandle};

pub(crate) const PRIORITY_TIME_CRITICAL: i32 = THREAD_PRIORITY_TIME_CRITICAL;
pub(crate) const PRIORITY_HIGHEST: i32 = THREAD_PRIORITY_HIGHEST;
pub(crate) const PRIORITY_ABOVE_NORMAL: i32 = THREAD_PRIORITY_ABOVE_NORMAL;
pub(crate) const PRIORITY_NORMAL: i32 = THREAD_PRIORITY_NORMAL;
pub(crate) const PRIORITY_BELOW_NORMAL: i32 = THREAD_PRIORITY_BELOW_NORMAL;
pub(crate) const PRIORITY_LOWEST: i32 = THREAD_PRIORITY_LOWEST;
pub(crate) const PRIORITY_IDLE: i32 = THREAD_PRIORITY_IDLE;

const PRIORITY_ERROR_RETURN: i32 = i32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadPriorityError {
    AccessDenied,
    ThreadExited,
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
    pub(crate) fn raw(&self) -> HANDLE {
        self.handle.raw()
    }
}

pub(crate) fn thread_ids(process_id: u32) -> Result<Vec<u32>, ThreadPriorityError> {
    // SAFETY: TH32CS_SNAPTHREAD ignores the process id argument and returns an owned handle.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(ThreadPriorityError::Failed {
            operation: "CreateToolhelp32Snapshot",
            thread_id: None,
            code: last_error(),
        });
    }
    let snapshot = WinHandle::new(snapshot);
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    let mut ids = Vec::new();
    // SAFETY: snapshot is live and entry declares its size and remains writable.
    let mut present = unsafe { Thread32First(snapshot.raw(), &mut entry) };
    while present != 0 {
        if entry.th32OwnerProcessID == process_id {
            ids.push(entry.th32ThreadID);
        }
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        // SAFETY: snapshot remains live and entry remains writable for the next record.
        present = unsafe { Thread32Next(snapshot.raw(), &mut entry) };
    }
    let error = last_error();
    if error != windows_sys::Win32::Foundation::ERROR_NO_MORE_FILES {
        return Err(ThreadPriorityError::Failed {
            operation: "Thread enumeration",
            thread_id: None,
            code: error,
        });
    }
    Ok(ids)
}

pub(crate) fn open_thread(thread_id: u32) -> Result<ThreadHandle, ThreadPriorityError> {
    // SAFETY: thread_id came from a current Toolhelp snapshot and the handle is not inheritable.
    let handle = unsafe {
        OpenThread(
            THREAD_QUERY_INFORMATION | THREAD_SET_INFORMATION,
            0,
            thread_id,
        )
    };
    if handle.is_null() {
        return Err(capture_thread_error("OpenThread", thread_id));
    }
    Ok(ThreadHandle {
        id: thread_id,
        handle: WinHandle::new(handle),
    })
}

pub(crate) fn owner_process_id(thread: &ThreadHandle) -> Result<u32, ThreadPriorityError> {
    // SAFETY: thread owns a live handle opened with query access.
    let process_id = unsafe { GetProcessIdOfThread(thread.handle.raw()) };
    if process_id == 0 {
        Err(capture_thread_error("GetProcessIdOfThread", thread.id))
    } else {
        Ok(process_id)
    }
}

pub(crate) fn creation_time(thread: &ThreadHandle) -> Result<u64, ThreadPriorityError> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: thread owns a live handle and every FILETIME output is writable for the call.
    let ok = unsafe {
        GetThreadTimes(
            thread.handle.raw(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    };
    if ok == 0 {
        Err(capture_thread_error("GetThreadTimes", thread.id))
    } else {
        Ok(filetime_to_u64(creation))
    }
}

pub(crate) fn query_priority(thread: &ThreadHandle) -> Result<i32, ThreadPriorityError> {
    // SAFETY: thread owns a live handle opened with query access.
    let priority = unsafe { GetThreadPriority(thread.handle.raw()) };
    if priority == PRIORITY_ERROR_RETURN {
        Err(capture_thread_error("GetThreadPriority", thread.id))
    } else {
        Ok(priority)
    }
}

pub(crate) fn set_priority(
    thread: &ThreadHandle,
    priority: i32,
) -> Result<(), ThreadPriorityError> {
    // SAFETY: thread owns a live handle opened with set access and priority is a documented class
    // or the raw value previously returned by Windows for this exact thread.
    let ok = unsafe { SetThreadPriority(thread.handle.raw(), priority) };
    if ok == 0 {
        Err(capture_thread_error("SetThreadPriority", thread.id))
    } else {
        Ok(())
    }
}

fn capture_thread_error(operation: &'static str, thread_id: u32) -> ThreadPriorityError {
    match last_error() {
        ERROR_ACCESS_DENIED => ThreadPriorityError::AccessDenied,
        ERROR_INVALID_PARAMETER => ThreadPriorityError::ThreadExited,
        code => ThreadPriorityError::Failed {
            operation,
            thread_id: Some(thread_id),
            code,
        },
    }
}
