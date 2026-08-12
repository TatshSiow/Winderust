use std::ffi::c_void;

use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED},
    System::Threading::{
        GetPriorityClass, GetProcessInformation, ProcessPowerThrottling, SetPriorityClass,
        SetProcessInformation, ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS,
        HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
        PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION, PROCESS_POWER_THROTTLING_STATE,
        REALTIME_PRIORITY_CLASS,
    },
};

use crate::win_util::{last_error, WinHandle};

use super::ProcessOperationError;

pub(crate) const PRIORITY_IDLE: u32 = IDLE_PRIORITY_CLASS;
pub(crate) const PRIORITY_BELOW_NORMAL: u32 = BELOW_NORMAL_PRIORITY_CLASS;
pub(crate) const PRIORITY_NORMAL: u32 = NORMAL_PRIORITY_CLASS;
pub(crate) const PRIORITY_ABOVE_NORMAL: u32 = ABOVE_NORMAL_PRIORITY_CLASS;
pub(crate) const PRIORITY_HIGH: u32 = HIGH_PRIORITY_CLASS;
pub(crate) const PRIORITY_REALTIME: u32 = REALTIME_PRIORITY_CLASS;

pub(crate) const POWER_CURRENT_VERSION: u32 = PROCESS_POWER_THROTTLING_CURRENT_VERSION;
pub(crate) const POWER_EXECUTION_SPEED: u32 = PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
pub(crate) const POWER_IGNORE_TIMER_RESOLUTION: u32 =
    PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PowerThrottlingState {
    pub(crate) version: u32,
    pub(crate) control_mask: u32,
    pub(crate) state_mask: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PowerThrottlingError {
    AccessDenied,
    Unavailable,
    Failed { operation: &'static str, code: u32 },
}

pub(crate) fn query_priority(process: &WinHandle) -> Result<u32, ProcessOperationError> {
    // SAFETY: process is a verified live handle and GetPriorityClass only reads its state.
    let priority = unsafe { GetPriorityClass(process.raw()) };
    if priority == 0 {
        Err(ProcessOperationError::capture("GetPriorityClass"))
    } else {
        Ok(priority)
    }
}

pub(crate) fn set_priority(
    process: &WinHandle,
    priority: u32,
) -> Result<(), ProcessOperationError> {
    // SAFETY: process is a verified live handle and priority is a documented class or a raw value
    // previously returned by Windows.
    let ok = unsafe { SetPriorityClass(process.raw(), priority) };
    if ok == 0 {
        Err(ProcessOperationError::capture("SetPriorityClass"))
    } else {
        Ok(())
    }
}

pub(crate) fn query_power(
    process: &WinHandle,
) -> Result<PowerThrottlingState, PowerThrottlingError> {
    let mut state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ..Default::default()
    };
    // SAFETY: process is a verified live handle and state is writable for exactly the supplied
    // structure size.
    let ok = unsafe {
        GetProcessInformation(
            process.raw(),
            ProcessPowerThrottling,
            (&mut state as *mut PROCESS_POWER_THROTTLING_STATE).cast::<c_void>(),
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
    if ok == 0 {
        Err(capture_power_error("GetProcessInformation"))
    } else {
        Ok(PowerThrottlingState {
            version: state.Version,
            control_mask: state.ControlMask,
            state_mask: state.StateMask,
        })
    }
}

pub(crate) fn set_power(
    process: &WinHandle,
    state: PowerThrottlingState,
) -> Result<(), PowerThrottlingError> {
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: state.version,
        ControlMask: state.control_mask,
        StateMask: state.state_mask,
    };
    // SAFETY: process is a verified live handle and state is initialized for exactly the supplied
    // structure size.
    let ok = unsafe {
        SetProcessInformation(
            process.raw(),
            ProcessPowerThrottling,
            (&state as *const PROCESS_POWER_THROTTLING_STATE).cast::<c_void>(),
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
    if ok == 0 {
        Err(capture_power_error("SetProcessInformation"))
    } else {
        Ok(())
    }
}

fn capture_power_error(operation: &'static str) -> PowerThrottlingError {
    match last_error() {
        ERROR_ACCESS_DENIED => PowerThrottlingError::AccessDenied,
        ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED => PowerThrottlingError::Unavailable,
        code => PowerThrottlingError::Failed { operation, code },
    }
}
