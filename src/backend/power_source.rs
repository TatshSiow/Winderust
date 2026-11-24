use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

const BATTERY_CHARGING: u8 = 8;
const NO_SYSTEM_BATTERY: u8 = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatteryStatus {
    Absent,
    Unavailable,
    Level(u8),
    Charging(Option<u8>),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PowerStatus {
    pub(crate) plugged_in: Option<bool>,
    pub(crate) battery: BatteryStatus,
    pub(crate) energy_saver_active: bool,
}

pub fn is_plugged_in() -> Option<bool> {
    read().and_then(|status| status.plugged_in)
}

pub(crate) fn read() -> Option<PowerStatus> {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: status is valid writable storage for the duration of the call.
    let ok = unsafe { GetSystemPowerStatus(&mut status) };
    (ok != 0).then(|| PowerStatus::from(status))
}

impl From<SYSTEM_POWER_STATUS> for PowerStatus {
    fn from(status: SYSTEM_POWER_STATUS) -> Self {
        let plugged_in = match status.ACLineStatus {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        };
        let percent = (status.BatteryLifePercent <= 100).then_some(status.BatteryLifePercent);
        // Windows reports 255 when the flags are unknown. None of its bits are evidence.
        let flags = match status.BatteryFlag {
            u8::MAX => 0,
            flags => flags,
        };
        let battery = match flags {
            flags if flags & NO_SYSTEM_BATTERY != 0 => BatteryStatus::Absent,
            flags if flags & BATTERY_CHARGING != 0 => BatteryStatus::Charging(percent),
            _ => percent.map_or(BatteryStatus::Unavailable, BatteryStatus::Level),
        };
        Self {
            plugged_in,
            battery,
            energy_saver_active: status.SystemStatusFlag == 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_status_decodes_independent_windows_readings() {
        let mut status = SYSTEM_POWER_STATUS {
            ACLineStatus: 1,
            BatteryFlag: BATTERY_CHARGING | 1,
            BatteryLifePercent: 100,
            SystemStatusFlag: 1,
            ..Default::default()
        };
        let charging = PowerStatus::from(status);
        assert_eq!(charging.plugged_in, Some(true));
        assert_eq!(charging.battery, BatteryStatus::Charging(Some(100)));
        assert!(charging.energy_saver_active);

        status.ACLineStatus = 0;
        status.BatteryFlag = u8::MAX;
        status.BatteryLifePercent = 0;
        let measured = PowerStatus::from(status);
        assert_eq!(measured.plugged_in, Some(false));
        assert_eq!(measured.battery, BatteryStatus::Level(0));

        status.ACLineStatus = u8::MAX;
        status.BatteryLifePercent = u8::MAX;
        status.SystemStatusFlag = u8::MAX;
        let unknown = PowerStatus::from(status);
        assert_eq!(unknown.plugged_in, None);
        assert_eq!(unknown.battery, BatteryStatus::Unavailable);
        assert!(!unknown.energy_saver_active);

        status.BatteryFlag = BATTERY_CHARGING;
        assert_eq!(
            PowerStatus::from(status).battery,
            BatteryStatus::Charging(None)
        );
        status.BatteryFlag = NO_SYSTEM_BATTERY;
        assert_eq!(PowerStatus::from(status).battery, BatteryStatus::Absent);
    }
}
