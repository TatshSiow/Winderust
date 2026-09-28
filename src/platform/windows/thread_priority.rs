use std::{collections::BTreeMap, mem::size_of};

use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, HANDLE, INVALID_HANDLE_VALUE,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        },
        Threading::{
            GetProcessIdOfThread, GetThreadPriority, GetThreadTimes, OpenThread, SetThreadPriority,
            WaitForSingleObject, THREAD_PRIORITY_ABOVE_NORMAL, THREAD_PRIORITY_BELOW_NORMAL,
            THREAD_PRIORITY_HIGHEST, THREAD_PRIORITY_IDLE, THREAD_PRIORITY_LOWEST,
            THREAD_PRIORITY_NORMAL, THREAD_PRIORITY_TIME_CRITICAL, THREAD_QUERY_INFORMATION,
            THREAD_SET_INFORMATION, THREAD_SYNCHRONIZE,
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

pub(crate) fn thread_inventory() -> Result<BTreeMap<u32, Vec<u32>>, ThreadPriorityError> {
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
    let mut inventory = BTreeMap::<u32, Vec<u32>>::new();
    // SAFETY: snapshot is live and entry declares its size and remains writable.
    let mut present = unsafe { Thread32First(snapshot.raw(), &mut entry) };
    while present != 0 {
        inventory
            .entry(entry.th32OwnerProcessID)
            .or_default()
            .push(entry.th32ThreadID);
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
    Ok(inventory)
}

pub(crate) fn open_thread(thread_id: u32) -> Result<ThreadHandle, ThreadPriorityError> {
    // SAFETY: thread_id came from a current Toolhelp snapshot and the handle is not inheritable.
    let handle = unsafe {
        OpenThread(
            THREAD_QUERY_INFORMATION | THREAD_SET_INFORMATION | THREAD_SYNCHRONIZE,
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

pub(crate) fn ensure_active(thread: &ThreadHandle) -> Result<(), ThreadPriorityError> {
    // SAFETY: thread owns a handle with SYNCHRONIZE access; zero timeout never blocks.
    match unsafe { WaitForSingleObject(thread.raw(), 0) } {
        WAIT_TIMEOUT => Ok(()),
        WAIT_OBJECT_0 => Err(ThreadPriorityError::ThreadExited),
        _ => Err(capture_thread_error("WaitForSingleObject", thread.id)),
    }
}

pub(crate) fn owner_process_id(thread: &ThreadHandle) -> Result<u32, ThreadPriorityError> {
    // SAFETY: thread owns a live handle opened with query access.
    let process_id = unsafe { GetProcessIdOfThread(thread.handle.raw()) };
    if process_id == 0 {
        Err(capture_retained_thread_error(
            "GetProcessIdOfThread",
            thread,
        ))
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
        Err(capture_retained_thread_error("GetThreadTimes", thread))
    } else {
        Ok(filetime_to_u64(creation))
    }
}

pub(crate) fn query_priority(thread: &ThreadHandle) -> Result<i32, ThreadPriorityError> {
    // SAFETY: thread owns a live handle opened with query access.
    let priority = unsafe { GetThreadPriority(thread.handle.raw()) };
    if priority == PRIORITY_ERROR_RETURN {
        Err(capture_retained_thread_error("GetThreadPriority", thread))
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
        Err(capture_retained_thread_error("SetThreadPriority", thread))
    } else {
        Ok(())
    }
}

fn capture_thread_error(operation: &'static str, thread_id: u32) -> ThreadPriorityError {
    classify_thread_error(operation, thread_id, last_error(), false)
}

fn capture_retained_thread_error(
    operation: &'static str,
    thread: &ThreadHandle,
) -> ThreadPriorityError {
    let code = last_error();
    // SAFETY: this retained handle has SYNCHRONIZE access; the zero-timeout check cannot block.
    let terminated = unsafe { WaitForSingleObject(thread.raw(), 0) } == WAIT_OBJECT_0;
    classify_thread_error(operation, thread.id, code, terminated)
}

fn classify_thread_error(
    operation: &'static str,
    thread_id: u32,
    code: u32,
    terminated: bool,
) -> ThreadPriorityError {
    // OpenThread uses a fixed valid access mask: invalid parameter identifies a vanished TID.
    // Errors from operations on retained handles require an explicitly signaled object.
    if terminated || (operation == "OpenThread" && code == ERROR_INVALID_PARAMETER) {
        return ThreadPriorityError::ThreadExited;
    }
    match code {
        ERROR_ACCESS_DENIED => ThreadPriorityError::AccessDenied,
        code => ThreadPriorityError::Failed {
            operation,
            thread_id: Some(thread_id),
            code,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_handle_errors_require_positive_exit_evidence() {
        // SAFETY: reads the calling thread ID only.
        let id = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
        let handle = open_thread(id).unwrap();
        for operation in [
            "GetThreadTimes",
            "SetThreadPriority",
            "GetThreadPriority",
            "GetProcessIdOfThread",
        ] {
            // SAFETY: modifies only the test thread's last-error slot to inject an API failure.
            unsafe { windows_sys::Win32::Foundation::SetLastError(ERROR_INVALID_PARAMETER) };
            assert_eq!(
                capture_retained_thread_error(operation, &handle),
                ThreadPriorityError::Failed {
                    operation,
                    thread_id: Some(id),
                    code: ERROR_INVALID_PARAMETER,
                }
            );
        }
        assert!(matches!(
            classify_thread_error("WaitForSingleObject", id, ERROR_INVALID_PARAMETER, false),
            ThreadPriorityError::Failed { .. }
        ));
        assert_eq!(
            classify_thread_error("GetThreadTimes", id, ERROR_ACCESS_DENIED, true),
            ThreadPriorityError::ThreadExited
        );
    }

    #[test]
    fn inventory_contains_the_calling_thread_under_its_process() {
        // SAFETY: These calls only read the current process and thread identifiers.
        let (process_id, thread_id) = unsafe {
            (
                windows_sys::Win32::System::Threading::GetCurrentProcessId(),
                windows_sys::Win32::System::Threading::GetCurrentThreadId(),
            )
        };
        let inventory = thread_inventory().unwrap();
        assert!(inventory[&process_id].contains(&thread_id));
    }

    #[test]
    fn retained_thread_handle_detects_exit_without_changing_priority() {
        let (id_tx, id_rx) = std::sync::mpsc::channel();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            // SAFETY: GetCurrentThreadId takes no arguments and only reads the caller's ID.
            let id = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
            id_tx.send(id).unwrap();
            let _ = stop_rx.recv();
        });
        let handle = open_thread(id_rx.recv().unwrap()).unwrap();
        assert_eq!(ensure_active(&handle), Ok(()));
        drop(stop_tx);
        worker.join().unwrap();
        assert_eq!(
            ensure_active(&handle),
            Err(ThreadPriorityError::ThreadExited)
        );
        // SAFETY: inject a failed query after the retained object has become signaled.
        unsafe { windows_sys::Win32::Foundation::SetLastError(ERROR_ACCESS_DENIED) };
        assert_eq!(
            capture_retained_thread_error("GetThreadTimes", &handle),
            ThreadPriorityError::ThreadExited
        );
    }
}
