use std::{ffi::c_void, mem::size_of, sync::Mutex};

use windows_sys::Win32::{
    Foundation::GetLastError,
    System::Threading::{
        GetCurrentProcess, GetPriorityClass, GetProcessInformation, ProcessPowerThrottling,
        SetPriorityClass, SetProcessInformation, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
        PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        PROCESS_POWER_THROTTLING_STATE,
    },
};

static SELF_POWER_STATE: Mutex<Option<SelfPowerState>> = Mutex::new(None);

#[derive(Clone, Copy)]
struct SelfPowerState {
    previous_power_throttling: Option<PROCESS_POWER_THROTTLING_STATE>,
    previous_priority: Option<u32>,
}

pub fn enable_hidden_mode() -> Result<(), String> {
    let mut state = SELF_POWER_STATE
        .lock()
        .map_err(|_| "PowerLeaf self power state lock is poisoned.".to_owned())?;
    if state.is_some() {
        return Ok(());
    }

    let process = unsafe { GetCurrentProcess() };
    let previous_power_throttling = power_throttling_state(process).ok();
    let previous_priority = priority_class(process).ok();

    let mut next_state = previous_power_throttling.unwrap_or_default();
    next_state.Version = PROCESS_POWER_THROTTLING_CURRENT_VERSION;
    next_state.ControlMask |= PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    next_state.StateMask |= PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    set_power_throttling_state(process, next_state)?;

    if let Err(err) = set_priority_class(process, IDLE_PRIORITY_CLASS) {
        let _ = set_power_throttling_state(
            process,
            previous_power_throttling.unwrap_or_else(power_throttling_disabled_state),
        );
        return Err(err);
    }

    *state = Some(SelfPowerState {
        previous_power_throttling,
        previous_priority,
    });
    Ok(())
}

pub fn disable_hidden_mode() -> Result<(), String> {
    let Some(state) = SELF_POWER_STATE
        .lock()
        .map_err(|_| "PowerLeaf self power state lock is poisoned.".to_owned())?
        .take()
    else {
        return Ok(());
    };

    let process = unsafe { GetCurrentProcess() };
    let mut last_error = None;

    if let Err(err) = set_power_throttling_state(
        process,
        state
            .previous_power_throttling
            .unwrap_or_else(power_throttling_disabled_state),
    ) {
        last_error = Some(err);
    }

    if let Err(err) = set_priority_class(
        process,
        state.previous_priority.unwrap_or(NORMAL_PRIORITY_CLASS),
    ) {
        last_error = Some(err);
    }

    match last_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn power_throttling_state(
    process: windows_sys::Win32::Foundation::HANDLE,
) -> Result<PROCESS_POWER_THROTTLING_STATE, String> {
    let mut state = PROCESS_POWER_THROTTLING_STATE::default();
    let ok = unsafe {
        GetProcessInformation(
            process,
            ProcessPowerThrottling,
            &mut state as *mut _ as *mut c_void,
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
    if ok == 0 {
        Err(format!(
            "GetProcessInformation ProcessPowerThrottling failed with error {}.",
            last_error()
        ))
    } else {
        Ok(state)
    }
}

fn set_power_throttling_state(
    process: windows_sys::Win32::Foundation::HANDLE,
    state: PROCESS_POWER_THROTTLING_STATE,
) -> Result<(), String> {
    let ok = unsafe {
        SetProcessInformation(
            process,
            ProcessPowerThrottling,
            &state as *const _ as *const c_void,
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

fn priority_class(process: windows_sys::Win32::Foundation::HANDLE) -> Result<u32, String> {
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

fn set_priority_class(
    process: windows_sys::Win32::Foundation::HANDLE,
    priority_class: u32,
) -> Result<(), String> {
    let ok = unsafe { SetPriorityClass(process, priority_class) };
    if ok == 0 {
        Err(format!(
            "SetPriorityClass failed with error {}.",
            last_error()
        ))
    } else {
        Ok(())
    }
}

fn power_throttling_disabled_state() -> PROCESS_POWER_THROTTLING_STATE {
    PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0,
    }
}

fn last_error() -> u32 {
    unsafe { GetLastError() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_state_clears_execution_speed_control() {
        let state = power_throttling_disabled_state();

        assert_eq!(state.Version, PROCESS_POWER_THROTTLING_CURRENT_VERSION);
        assert_eq!(state.ControlMask, PROCESS_POWER_THROTTLING_EXECUTION_SPEED);
        assert_eq!(state.StateMask, 0);
    }
}
