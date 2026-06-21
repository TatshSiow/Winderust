use std::ptr::null_mut;

use windows_sys::{
    core::GUID,
    Win32::{
        Foundation::{LocalFree, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
        System::Power::{
            PowerEnumerate, PowerGetActiveScheme, PowerReadACValueIndex, PowerReadDCValueIndex,
            PowerReadFriendlyName, PowerSetActiveScheme, PowerWriteACValueIndex,
            PowerWriteDCValueIndex, ACCESS_SCHEME,
        },
    },
};

use super::{PowerPlan, ProcessorBoostMode, ProcessorPowerAcDcValues, ProcessorPowerValues};

#[derive(Debug, Default)]
pub struct PowerPlanManager;

impl PowerPlanManager {
    pub fn list_plans(&self) -> Result<Vec<PowerPlan>, String> {
        let active_guid = active_scheme_guid().ok();
        let mut plans = Vec::new();
        let mut index = 0;

        while let Some(guid) = enumerate_scheme_guid(index)? {
            let guid_text = format_guid(&guid);
            let name = read_scheme_name(&guid).unwrap_or_else(|_| guid_text.clone());
            let active = active_guid
                .as_deref()
                .is_some_and(|active_guid| active_guid.eq_ignore_ascii_case(&guid_text));

            plans.push(PowerPlan {
                guid: guid_text,
                name,
                active,
            });
            index += 1;
        }

        if plans.is_empty() {
            Err("No Windows power plans were detected.".to_owned())
        } else {
            Ok(plans)
        }
    }

    pub fn active_plan(&self) -> Result<Option<PowerPlan>, String> {
        let active_guid = active_scheme_guid()?;
        Ok(Some(PowerPlan {
            guid: active_guid,
            name: "Active power plan".to_owned(),
            active: true,
        }))
    }

    pub fn set_active(&self, guid: &str) -> Result<(), String> {
        let guid = parse_guid(guid).ok_or_else(|| "Invalid power plan GUID.".to_owned())?;
        let result = unsafe { PowerSetActiveScheme(null_mut(), &guid) };
        if result == 0 {
            Ok(())
        } else {
            Err(format!(
                "PowerSetActiveScheme failed with error code {result}."
            ))
        }
    }

    pub fn apply_processor_power_values(
        &self,
        guid: &str,
        values: ProcessorPowerAcDcValues,
    ) -> Result<(), String> {
        let scheme = parse_guid(guid).ok_or_else(|| "Invalid power plan GUID.".to_owned())?;
        let values = values.normalized();

        write_ac_value(
            &scheme,
            &GUID_PROCESSOR_SETTINGS_SUBGROUP,
            &GUID_CORE_PARKING_MIN_CORES,
            values.ac.core_parking_min,
        )?;
        write_dc_value(
            &scheme,
            &GUID_PROCESSOR_SETTINGS_SUBGROUP,
            &GUID_CORE_PARKING_MIN_CORES,
            values.dc.core_parking_min,
        )?;
        write_ac_value(
            &scheme,
            &GUID_PROCESSOR_SETTINGS_SUBGROUP,
            &GUID_PROCESSOR_PERFORMANCE_MIN,
            values.ac.performance_min,
        )?;
        write_dc_value(
            &scheme,
            &GUID_PROCESSOR_SETTINGS_SUBGROUP,
            &GUID_PROCESSOR_PERFORMANCE_MIN,
            values.dc.performance_min,
        )?;
        write_ac_value(
            &scheme,
            &GUID_PROCESSOR_SETTINGS_SUBGROUP,
            &GUID_PROCESSOR_PERFORMANCE_MAX,
            values.ac.performance_max,
        )?;
        write_dc_value(
            &scheme,
            &GUID_PROCESSOR_SETTINGS_SUBGROUP,
            &GUID_PROCESSOR_PERFORMANCE_MAX,
            values.dc.performance_max,
        )?;
        write_ac_value(
            &scheme,
            &GUID_PROCESSOR_SETTINGS_SUBGROUP,
            &GUID_PROCESSOR_PERFORMANCE_BOOST_MODE,
            values.ac.boost_mode.power_value(),
        )?;
        write_dc_value(
            &scheme,
            &GUID_PROCESSOR_SETTINGS_SUBGROUP,
            &GUID_PROCESSOR_PERFORMANCE_BOOST_MODE,
            values.dc.boost_mode.power_value(),
        )?;

        if active_scheme_guid()
            .ok()
            .is_some_and(|active_guid| active_guid.eq_ignore_ascii_case(guid))
        {
            self.set_active(guid)?;
        }

        Ok(())
    }

    pub fn read_processor_power_values(
        &self,
        guid: &str,
    ) -> Result<ProcessorPowerAcDcValues, String> {
        let scheme = parse_guid(guid).ok_or_else(|| "Invalid power plan GUID.".to_owned())?;
        Ok(ProcessorPowerAcDcValues::new(
            ProcessorPowerValues::new_with_boost_mode(
                read_ac_value(
                    &scheme,
                    &GUID_PROCESSOR_SETTINGS_SUBGROUP,
                    &GUID_CORE_PARKING_MIN_CORES,
                )?,
                read_ac_value(
                    &scheme,
                    &GUID_PROCESSOR_SETTINGS_SUBGROUP,
                    &GUID_PROCESSOR_PERFORMANCE_MIN,
                )?,
                read_ac_value(
                    &scheme,
                    &GUID_PROCESSOR_SETTINGS_SUBGROUP,
                    &GUID_PROCESSOR_PERFORMANCE_MAX,
                )?,
                ProcessorBoostMode::from_power_value(read_ac_value(
                    &scheme,
                    &GUID_PROCESSOR_SETTINGS_SUBGROUP,
                    &GUID_PROCESSOR_PERFORMANCE_BOOST_MODE,
                )?),
            )
            .normalized(),
            ProcessorPowerValues::new_with_boost_mode(
                read_dc_value(
                    &scheme,
                    &GUID_PROCESSOR_SETTINGS_SUBGROUP,
                    &GUID_CORE_PARKING_MIN_CORES,
                )?,
                read_dc_value(
                    &scheme,
                    &GUID_PROCESSOR_SETTINGS_SUBGROUP,
                    &GUID_PROCESSOR_PERFORMANCE_MIN,
                )?,
                read_dc_value(
                    &scheme,
                    &GUID_PROCESSOR_SETTINGS_SUBGROUP,
                    &GUID_PROCESSOR_PERFORMANCE_MAX,
                )?,
                ProcessorBoostMode::from_power_value(read_dc_value(
                    &scheme,
                    &GUID_PROCESSOR_SETTINGS_SUBGROUP,
                    &GUID_PROCESSOR_PERFORMANCE_BOOST_MODE,
                )?),
            )
            .normalized(),
        ))
    }
}

const GUID_PROCESSOR_SETTINGS_SUBGROUP: GUID = GUID {
    data1: 0x54533251,
    data2: 0x82be,
    data3: 0x4824,
    data4: [0x96, 0xc1, 0x47, 0xb6, 0x0b, 0x74, 0x0d, 0x00],
};

const GUID_CORE_PARKING_MIN_CORES: GUID = GUID {
    data1: 0x0cc5b647,
    data2: 0xc1df,
    data3: 0x4637,
    data4: [0x89, 0x1a, 0xde, 0xc3, 0x5c, 0x31, 0x85, 0x83],
};

const GUID_PROCESSOR_PERFORMANCE_MIN: GUID = GUID {
    data1: 0x893dee8e,
    data2: 0x2bef,
    data3: 0x41e0,
    data4: [0x89, 0xc6, 0xb5, 0x5d, 0x09, 0x29, 0x96, 0x4c],
};

const GUID_PROCESSOR_PERFORMANCE_MAX: GUID = GUID {
    data1: 0xbc5038f7,
    data2: 0x23e0,
    data3: 0x4960,
    data4: [0x96, 0xda, 0x33, 0xab, 0xaf, 0x59, 0x35, 0xec],
};

const GUID_PROCESSOR_PERFORMANCE_BOOST_MODE: GUID = GUID {
    data1: 0xbe337238,
    data2: 0x0d82,
    data3: 0x4146,
    data4: [0xa9, 0x60, 0x4f, 0x37, 0x49, 0xd4, 0x70, 0xc7],
};

fn write_ac_value(
    scheme: &GUID,
    subgroup: &GUID,
    setting: &GUID,
    value: u32,
) -> Result<(), String> {
    let ac_result = unsafe { PowerWriteACValueIndex(null_mut(), scheme, subgroup, setting, value) };
    if ac_result != ERROR_SUCCESS {
        return Err(format!(
            "PowerWriteACValueIndex({}) failed with error code {ac_result}.",
            format_guid(setting)
        ));
    }

    Ok(())
}

fn write_dc_value(
    scheme: &GUID,
    subgroup: &GUID,
    setting: &GUID,
    value: u32,
) -> Result<(), String> {
    let dc_result = unsafe { PowerWriteDCValueIndex(null_mut(), scheme, subgroup, setting, value) };
    if dc_result != ERROR_SUCCESS {
        return Err(format!(
            "PowerWriteDCValueIndex({}) failed with error code {dc_result}.",
            format_guid(setting)
        ));
    }

    Ok(())
}

fn read_ac_value(scheme: &GUID, subgroup: &GUID, setting: &GUID) -> Result<u32, String> {
    let mut value = 0;
    let result =
        unsafe { PowerReadACValueIndex(null_mut(), scheme, subgroup, setting, &mut value) };
    if result == ERROR_SUCCESS {
        Ok(value)
    } else {
        Err(format!(
            "PowerReadACValueIndex({}) failed with error code {result}.",
            format_guid(setting)
        ))
    }
}

fn read_dc_value(scheme: &GUID, subgroup: &GUID, setting: &GUID) -> Result<u32, String> {
    let mut value = 0;
    let result =
        unsafe { PowerReadDCValueIndex(null_mut(), scheme, subgroup, setting, &mut value) };
    if result == ERROR_SUCCESS {
        Ok(value)
    } else {
        Err(format!(
            "PowerReadDCValueIndex({}) failed with error code {result}.",
            format_guid(setting)
        ))
    }
}

fn enumerate_scheme_guid(index: u32) -> Result<Option<GUID>, String> {
    let mut guid = GUID::default();
    let mut buffer_size = std::mem::size_of::<GUID>() as u32;
    let result = unsafe {
        PowerEnumerate(
            null_mut(),
            null_mut(),
            null_mut(),
            ACCESS_SCHEME,
            index,
            (&mut guid as *mut GUID).cast(),
            &mut buffer_size,
        )
    };

    match result {
        ERROR_SUCCESS => Ok(Some(guid)),
        ERROR_NO_MORE_ITEMS => Ok(None),
        _ => Err(format!(
            "PowerEnumerate failed at index {index} with error code {result}."
        )),
    }
}

fn read_scheme_name(guid: &GUID) -> Result<String, String> {
    let mut buffer_size = 0;
    let size_result = unsafe {
        PowerReadFriendlyName(
            null_mut(),
            guid,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut buffer_size,
        )
    };

    if size_result != ERROR_SUCCESS && size_result != ERROR_MORE_DATA {
        return Err(format!(
            "PowerReadFriendlyName failed to read buffer size with error code {size_result}."
        ));
    }
    if buffer_size == 0 {
        return Err("PowerReadFriendlyName returned an empty name.".to_owned());
    }

    let mut buffer = vec![0_u8; buffer_size as usize];
    let result = unsafe {
        PowerReadFriendlyName(
            null_mut(),
            guid,
            null_mut(),
            null_mut(),
            buffer.as_mut_ptr(),
            &mut buffer_size,
        )
    };

    if result != ERROR_SUCCESS {
        return Err(format!(
            "PowerReadFriendlyName failed with error code {result}."
        ));
    }

    let utf16: Vec<u16> = buffer
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .take_while(|code| *code != 0)
        .collect();
    if utf16.is_empty() {
        Err("PowerReadFriendlyName returned an empty name.".to_owned())
    } else {
        Ok(String::from_utf16_lossy(&utf16))
    }
}

fn active_scheme_guid() -> Result<String, String> {
    let mut guid_ptr: *mut GUID = null_mut();
    let result = unsafe { PowerGetActiveScheme(null_mut(), &mut guid_ptr) };
    if result != 0 {
        return Err(format!(
            "PowerGetActiveScheme failed with error code {result}."
        ));
    }
    if guid_ptr.is_null() {
        return Err("PowerGetActiveScheme returned no active plan.".to_owned());
    }

    let guid = unsafe { *guid_ptr };
    unsafe {
        LocalFree(guid_ptr.cast());
    }
    Ok(format_guid(&guid))
}

fn parse_guid(value: &str) -> Option<GUID> {
    let value = value.trim().trim_start_matches('{').trim_end_matches('}');
    let parts: Vec<_> = value.split('-').collect();
    if parts.len() != 5
        || parts[0].len() != 8
        || parts[1].len() != 4
        || parts[2].len() != 4
        || parts[3].len() != 4
        || parts[4].len() != 12
    {
        return None;
    }

    let mut data4 = [0_u8; 8];
    data4[0] = parse_hex_byte(&parts[3][0..2])?;
    data4[1] = parse_hex_byte(&parts[3][2..4])?;
    for index in 0..6 {
        let start = index * 2;
        data4[index + 2] = parse_hex_byte(&parts[4][start..start + 2])?;
    }

    Some(GUID {
        data1: u32::from_str_radix(parts[0], 16).ok()?,
        data2: u16::from_str_radix(parts[1], 16).ok()?,
        data3: u16::from_str_radix(parts[2], 16).ok()?,
        data4,
    })
}

fn parse_hex_byte(value: &str) -> Option<u8> {
    u8::from_str_radix(value, 16).ok()
}

fn format_guid(guid: &GUID) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4[0],
        guid.data4[1],
        guid.data4[2],
        guid.data4[3],
        guid.data4[4],
        guid.data4[5],
        guid.data4[6],
        guid.data4[7],
    )
}

#[cfg(test)]
mod tests {
    use crate::power::ProcessorPowerPreset;

    use super::*;

    #[test]
    fn parses_and_formats_guid() {
        let raw = "381b4222-f694-41f0-9685-ff5bb260df2e";
        let guid = parse_guid(raw).unwrap();

        assert_eq!(format_guid(&guid), raw);
    }

    #[test]
    fn processor_power_presets_use_explicit_percentages() {
        let performance = ProcessorPowerValues::for_preset(ProcessorPowerPreset::Performance);
        assert_eq!(performance.core_parking_min, 100);
        assert_eq!(performance.performance_min, 100);
        assert_eq!(performance.performance_max, 100);
        assert_eq!(performance.boost_mode, ProcessorBoostMode::Aggressive);

        let saver = ProcessorPowerValues::for_preset(ProcessorPowerPreset::Saver);
        assert_eq!(saver.core_parking_min, 0);
        assert_eq!(saver.performance_min, 5);
        assert_eq!(saver.performance_max, 80);
        assert_eq!(saver.boost_mode, ProcessorBoostMode::EfficientEnabled);
    }

    #[test]
    fn processor_power_values_normalize_to_valid_percentages() {
        let values =
            ProcessorPowerValues::new_with_boost_mode(140, 75, 20, ProcessorBoostMode::Enabled)
                .normalized();

        assert_eq!(values.core_parking_min, 100);
        assert_eq!(values.performance_min, 75);
        assert_eq!(values.performance_max, 75);
        assert_eq!(values.boost_mode, ProcessorBoostMode::Enabled);
    }

    #[test]
    fn processor_power_ac_dc_values_normalize_each_power_source() {
        let values = ProcessorPowerAcDcValues::new(
            ProcessorPowerValues::new_with_boost_mode(120, 90, 80, ProcessorBoostMode::Enabled),
            ProcessorPowerValues::new_with_boost_mode(10, 20, 15, ProcessorBoostMode::Enabled),
        )
        .normalized();

        assert_eq!(values.ac.core_parking_min, 100);
        assert_eq!(values.ac.performance_min, 90);
        assert_eq!(values.ac.performance_max, 90);
        assert_eq!(values.dc.core_parking_min, 10);
        assert_eq!(values.dc.performance_min, 20);
        assert_eq!(values.dc.performance_max, 20);
    }
}
