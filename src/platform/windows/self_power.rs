use std::{ffi::c_void, mem::size_of};

use windows_sys::Win32::{
    Foundation::HANDLE,
    System::Threading::{
        GetCurrentProcess, GetPriorityClass, GetProcessInformation, ProcessPowerThrottling,
        SetPriorityClass, SetProcessInformation, PROCESS_POWER_THROTTLING_STATE,
    },
};

use crate::win_util::last_error;

pub(crate) use super::priority_efficiency::{
    PowerThrottlingState, POWER_CURRENT_VERSION, POWER_EXECUTION_SPEED,
    POWER_IGNORE_TIMER_RESOLUTION, PRIORITY_IDLE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SelfPowerState {
    pub(crate) power_throttling: PowerThrottlingState,
    pub(crate) priority_class: u32,
}

pub(crate) fn query() -> Result<SelfPowerState, String> {
    let process = current_process();
    Ok(SelfPowerState {
        power_throttling: query_power_throttling(process)?,
        priority_class: query_priority_class(process)?,
    })
}

pub(crate) fn set_power_throttling(state: PowerThrottlingState) -> Result<(), String> {
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: state.version,
        ControlMask: state.control_mask,
        StateMask: state.state_mask,
    };
    // SAFETY: GetCurrentProcess returns a valid pseudo-handle and state is initialized for exactly
    // the supplied structure size.
    let ok = unsafe {
        SetProcessInformation(
            current_process(),
            ProcessPowerThrottling,
            (&state as *const PROCESS_POWER_THROTTLING_STATE).cast::<c_void>(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
    if ok == 0 {
        Err(format!(
            "SetProcessInformation ProcessPowerThrottling failed with error {}.",
            last_error()
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn set_priority_class(priority_class: u32) -> Result<(), String> {
    // SAFETY: GetCurrentProcess returns a valid pseudo-handle and priority_class is a documented
    // class or an exact raw value previously returned by Windows.
    let ok = unsafe { SetPriorityClass(current_process(), priority_class) };
    if ok == 0 {
        Err(format!(
            "SetPriorityClass failed with error {}.",
            last_error()
        ))
    } else {
        Ok(())
    }
}

fn current_process() -> HANDLE {
    // SAFETY: GetCurrentProcess takes no arguments and always returns the current-process
    // pseudo-handle.
    unsafe { GetCurrentProcess() }
}

fn query_power_throttling(process: HANDLE) -> Result<PowerThrottlingState, String> {
    let mut state = PROCESS_POWER_THROTTLING_STATE {
        Version: POWER_CURRENT_VERSION,
        ..Default::default()
    };
    // SAFETY: process is the current-process pseudo-handle and state is writable for exactly the
    // structure size passed to Windows.
    let ok = unsafe {
        GetProcessInformation(
            process,
            ProcessPowerThrottling,
            (&mut state as *mut PROCESS_POWER_THROTTLING_STATE).cast::<c_void>(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
    if ok == 0 {
        Err(format!(
            "GetProcessInformation ProcessPowerThrottling failed with error {}.",
            last_error()
        ))
    } else {
        Ok(PowerThrottlingState {
            version: state.Version,
            control_mask: state.ControlMask,
            state_mask: state.StateMask,
        })
    }
}

fn query_priority_class(process: HANDLE) -> Result<u32, String> {
    // SAFETY: process is the valid current-process pseudo-handle.
    let priority = unsafe { GetPriorityClass(process) };
    if priority == 0 {
        Err(format!(
            "GetPriorityClass failed with error {}.",
            last_error()
        ))
    } else {
        Ok(priority)
    }
}
