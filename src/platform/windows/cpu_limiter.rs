use std::{ptr::null, sync::Arc, time::Duration};

use windows_sys::Win32::{
    Foundation::{WAIT_FAILED, WAIT_OBJECT_0},
    System::Threading::{
        CancelWaitableTimer, CreateEventW, CreateWaitableTimerExW, ResetEvent, SetEvent,
        SetWaitableTimer, WaitForMultipleObjects, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, INFINITE,
        TIMER_ALL_ACCESS,
    },
};

use crate::win_util::{last_error, WinHandle};

#[derive(Clone)]
pub(crate) struct CommandEvent(Arc<CommandEventInner>);

struct CommandEventInner(WinHandle);

// SAFETY: a Win32 event handle supports concurrent SetEvent, ResetEvent, wait, and close calls;
// Arc owns the handle until both the controller and worker release it.
unsafe impl Send for CommandEventInner {}
// SAFETY: the event operations used through this shared handle are thread-safe Win32 operations.
unsafe impl Sync for CommandEventInner {}

impl CommandEvent {
    pub(crate) fn new() -> Result<Self, String> {
        // SAFETY: null security attributes and name request a process-local manual-reset event.
        let handle = unsafe { CreateEventW(null(), 1, 0, null()) };
        if handle.is_null() {
            Err(format!(
                "CreateEventW for CPU Limiter failed with error {}.",
                last_error()
            ))
        } else {
            Ok(Self(Arc::new(CommandEventInner(WinHandle::new(handle)))))
        }
    }

    pub(crate) fn signal(&self) -> Result<(), String> {
        // SAFETY: self owns a live event handle.
        let ok = unsafe { SetEvent(self.0 .0.raw()) };
        if ok == 0 {
            Err(format!(
                "SetEvent for CPU Limiter failed with error {}.",
                last_error()
            ))
        } else {
            Ok(())
        }
    }

    fn reset(&self) -> Result<(), String> {
        // SAFETY: self owns a live event handle.
        let ok = unsafe { ResetEvent(self.0 .0.raw()) };
        if ok == 0 {
            Err(format!(
                "ResetEvent for CPU Limiter failed with error {}.",
                last_error()
            ))
        } else {
            Ok(())
        }
    }

    fn raw(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.0 .0.raw()
    }
}

pub(crate) struct HighResolutionTimer(WinHandle);

impl HighResolutionTimer {
    pub(crate) fn new() -> Result<Self, String> {
        // SAFETY: null attributes/name request an unnamed timer; the returned handle is owned.
        let handle = unsafe {
            CreateWaitableTimerExW(
                null(),
                null(),
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS,
            )
        };
        if handle.is_null() {
            Err(format!(
                "CreateWaitableTimerExW for CPU Limiter failed with error {}.",
                last_error()
            ))
        } else {
            Ok(Self(WinHandle::new(handle)))
        }
    }

    pub(crate) fn arm(&self, delay: Duration) -> Result<(), String> {
        let due_time = relative_due_time(delay);
        // SAFETY: self owns a live timer; due_time is readable for the call; no APC callback or
        // borrowed context is supplied.
        let ok = unsafe { SetWaitableTimer(self.0.raw(), &due_time, 0, None, null(), 0) };
        if ok == 0 {
            Err(format!(
                "SetWaitableTimer for CPU Limiter failed with error {}.",
                last_error()
            ))
        } else {
            Ok(())
        }
    }

    pub(crate) fn cancel(&self) -> Result<(), String> {
        // SAFETY: self owns a live timer handle.
        let ok = unsafe { CancelWaitableTimer(self.0.raw()) };
        if ok == 0 {
            Err(format!(
                "CancelWaitableTimer for CPU Limiter failed with error {}.",
                last_error()
            ))
        } else {
            Ok(())
        }
    }

    pub(crate) fn wait(&self, command: &CommandEvent) -> Result<(), String> {
        let handles = [command.raw(), self.0.raw()];
        // SAFETY: both handles remain live for the wait and handles points to two readable entries.
        let result = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
        match result {
            WAIT_OBJECT_0 => {
                command.reset()?;
                Ok(())
            }
            result if result == WAIT_OBJECT_0 + 1 => Ok(()),
            WAIT_FAILED => Err(format!(
                "WaitForMultipleObjects for CPU Limiter failed with error {}.",
                last_error()
            )),
            result => Err(format!(
                "WaitForMultipleObjects for CPU Limiter returned {result}."
            )),
        }
    }
}

fn relative_due_time(delay: Duration) -> i64 {
    let ticks = (delay.as_nanos() / 100).max(1).min(i64::MAX as u128) as i64;
    -ticks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waitable_timer_due_times_are_relative_hundred_nanosecond_ticks() {
        assert_eq!(relative_due_time(Duration::from_millis(1)), -10_000);
        assert_eq!(relative_due_time(Duration::from_millis(99)), -990_000);
    }
}
