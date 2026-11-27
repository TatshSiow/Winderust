use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

pub fn is_plugged_in() -> Option<bool> {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: status is valid writable storage for the duration of the call.
    let ok = unsafe { GetSystemPowerStatus(&mut status) };
    if ok == 0 {
        return None;
    }
    plugged_in_from_ac_line_status(status.ACLineStatus)
}

fn plugged_in_from_ac_line_status(status: u8) -> Option<bool> {
    match status {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_ac_line_status_has_no_power_source() {
        assert_eq!(plugged_in_from_ac_line_status(0), Some(false));
        assert_eq!(plugged_in_from_ac_line_status(1), Some(true));
        assert_eq!(plugged_in_from_ac_line_status(255), None);
    }
}
