use std::{ffi::c_void, mem::size_of, sync::Mutex};

use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetPriorityClass, GetProcessInformation, ProcessPowerThrottling,
    SetPriorityClass, SetProcessInformation, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION, PROCESS_POWER_THROTTLING_STATE,
};

use crate::win_util::last_error;

static SELF_POWER_STATE: Mutex<Option<SelfPowerState>> = Mutex::new(None);

#[derive(Clone, Copy)]
struct SelfPowerState {
    previous_power_throttling: Option<PROCESS_POWER_THROTTLING_STATE>,
    previous_priority: Option<u32>,
    hidden_mode: bool,
    smart_saver_mode: bool,
}

pub fn enable_hidden_mode() -> Result<(), String> {
    set_hidden_mode(true)
}

pub fn disable_hidden_mode() -> Result<(), String> {
    set_hidden_mode(false)
}

pub fn enable_smart_saver_mode() -> Result<(), String> {
    set_smart_saver_mode(true)
}

pub fn disable_smart_saver_mode() -> Result<(), String> {
    set_smart_saver_mode(false)
}

fn set_hidden_mode(enabled: bool) -> Result<(), String> {
    let mut state = SELF_POWER_STATE
        .lock()
        .map_err(|_| "Winderust self power state lock is poisoned.".to_owned())?;
    ensure_state(&mut state)?.hidden_mode = enabled;
    apply_self_power_state(&mut state)
}

fn set_smart_saver_mode(enabled: bool) -> Result<(), String> {
    let mut state = SELF_POWER_STATE
        .lock()
        .map_err(|_| "Winderust self power state lock is poisoned.".to_owned())?;
    ensure_state(&mut state)?.smart_saver_mode = enabled;
    apply_self_power_state(&mut state)
}

fn ensure_state(state: &mut Option<SelfPowerState>) -> Result<&mut SelfPowerState, String> {
    if state.is_none() {
        let process = unsafe { GetCurrentProcess() };
        *state = Some(SelfPowerState {
            previous_power_throttling: power_throttling_state(process).ok(),
            previous_priority: priority_class(process).ok(),
            hidden_mode: false,
            smart_saver_mode: false,
        });
    }

    state
        .as_mut()
        .ok_or_else(|| "Winderust self power state is unavailable.".to_owned())
}

fn apply_self_power_state(state: &mut Option<SelfPowerState>) -> Result<(), String> {
    let Some(current) = *state else {
        return Ok(());
    };

    let process = unsafe { GetCurrentProcess() };
    let enabled = current.hidden_mode || current.smart_saver_mode;
    let next_power_state = if enabled {
        power_throttling_enabled_state(current.previous_power_throttling, current.smart_saver_mode)
    } else {
        current
            .previous_power_throttling
            .unwrap_or_else(power_throttling_disabled_state)
    };
    let next_priority = if current.hidden_mode {
        IDLE_PRIORITY_CLASS
    } else {
        current.previous_priority.unwrap_or(NORMAL_PRIORITY_CLASS)
    };

    set_power_throttling_state(process, next_power_state)?;
    set_priority_class(process, next_priority)?;

    if !enabled {
        *state = None;
    }

    Ok(())
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
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        StateMask: 0,
    }
}

fn power_throttling_enabled_state(
    previous: Option<PROCESS_POWER_THROTTLING_STATE>,
    ignore_timer_resolution: bool,
) -> PROCESS_POWER_THROTTLING_STATE {
    let previous_ignored_timer = previous.is_some_and(|state| {
        state.StateMask & PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION != 0
    });
    let mut state = previous.unwrap_or_else(power_throttling_disabled_state);
    state.Version = PROCESS_POWER_THROTTLING_CURRENT_VERSION;
    state.ControlMask |=
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;
    state.StateMask |= PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    if ignore_timer_resolution || previous_ignored_timer {
        state.StateMask |= PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;
    } else {
        state.StateMask &= !PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_state_clears_execution_speed_control() {
        let state = power_throttling_disabled_state();

        assert_eq!(state.Version, PROCESS_POWER_THROTTLING_CURRENT_VERSION);
        assert_eq!(
            state.ControlMask,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION
        );
        assert_eq!(state.StateMask, 0);
    }

    #[test]
    fn smart_saver_state_ignores_self_timer_resolution() {
        let state = power_throttling_enabled_state(None, true);

        assert_ne!(
            state.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            0
        );
        assert_ne!(
            state.StateMask & PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            0
        );
    }
}
