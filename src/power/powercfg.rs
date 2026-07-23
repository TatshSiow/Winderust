use std::{
    ffi::c_void,
    ptr::null_mut,
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc,
    },
};

use windows_sys::{
    core::GUID,
    Win32::{
        Foundation::{LocalFree, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
        System::Power::{
            PowerDeleteScheme, PowerDuplicateScheme, PowerEnumerate, PowerGetActiveScheme,
            PowerReadACValueIndex, PowerReadDCValueIndex, PowerReadDescription,
            PowerReadFriendlyName, PowerRegisterForEffectivePowerModeNotifications,
            PowerSetActiveScheme, PowerUnregisterFromEffectivePowerModeNotifications,
            PowerWriteACValueIndex, PowerWriteDCValueIndex, PowerWriteDescription,
            PowerWriteFriendlyName, ACCESS_SCHEME, EFFECTIVE_POWER_MODE, EFFECTIVE_POWER_MODE_V2,
        },
    },
};

use super::{
    EffectivePowerMode, PowerPlan, PowerPlanPersonality, ProcessorBoostMode,
    ProcessorPowerAcDcValues, ProcessorPowerValues,
};

const ADAPTIVE_PLAN_NAME: &str = "Winderust Adaptive";
const ADAPTIVE_PLAN_DESCRIPTION_PREFIX: &str = "Winderust managed adaptive plan; restore=";

#[derive(Debug)]
pub struct EffectivePowerModeMonitor {
    mode: Arc<AtomicI32>,
    registration: *mut c_void,
}

pub fn list_plans() -> Result<Vec<PowerPlan>, String> {
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

pub fn active_plan() -> Result<PowerPlan, String> {
    let active_guid = active_scheme_guid()?;
    Ok(PowerPlan {
        guid: active_guid,
        name: "Active power plan".to_owned(),
        active: true,
    })
}

pub fn set_active(guid: &str) -> Result<(), String> {
    let guid = parse_guid(guid).ok_or_else(|| "Invalid power plan GUID.".to_owned())?;
    // SAFETY: guid is a fully parsed GUID and a null root key selects the current user.
    let result = unsafe { PowerSetActiveScheme(null_mut(), &guid) };
    if result == 0 {
        Ok(())
    } else {
        Err(format!(
            "PowerSetActiveScheme failed with error code {result}."
        ))
    }
}

pub fn create_adaptive_plan(source_guid: &str) -> Result<String, String> {
    let source = parse_guid(source_guid).ok_or_else(|| "Invalid power plan GUID.".to_owned())?;
    let mut duplicate_ptr: *mut GUID = null_mut();
    // SAFETY: source is a valid GUID and duplicate_ptr is a writable out-pointer owned by
    // LocalFree on success.
    let result = unsafe { PowerDuplicateScheme(null_mut(), &source, &mut duplicate_ptr) };
    if result != ERROR_SUCCESS {
        return Err(format!(
            "PowerDuplicateScheme failed with error code {result}."
        ));
    }
    if duplicate_ptr.is_null() {
        return Err("PowerDuplicateScheme returned no power plan.".to_owned());
    }

    // SAFETY: PowerDuplicateScheme succeeded and returned a non-null GUID allocation.
    let duplicate = unsafe { *duplicate_ptr };
    // SAFETY: duplicate_ptr was allocated by PowerDuplicateScheme and is freed exactly once.
    unsafe {
        LocalFree(duplicate_ptr.cast());
    }
    let duplicate_guid = format_guid(&duplicate);
    let description = format!("{ADAPTIVE_PLAN_DESCRIPTION_PREFIX}{source_guid}");

    if let Err(error) = write_scheme_name(&duplicate, ADAPTIVE_PLAN_NAME)
        .and_then(|()| write_scheme_description(&duplicate, &description))
    {
        // SAFETY: duplicate is the valid scheme created above; deletion is best-effort cleanup
        // after initialization failed.
        unsafe {
            PowerDeleteScheme(null_mut(), &duplicate);
        }
        return Err(error);
    }

    Ok(duplicate_guid)
}

pub fn delete_plan(guid: &str) -> Result<(), String> {
    let guid = parse_guid(guid).ok_or_else(|| "Invalid power plan GUID.".to_owned())?;
    // SAFETY: guid is a fully parsed scheme GUID and a null root key selects the current user.
    let result = unsafe { PowerDeleteScheme(null_mut(), &guid) };
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "PowerDeleteScheme failed with error code {result}."
        ))
    }
}

pub fn restore_stale_adaptive_plans() -> Result<(), String> {
    let plans = list_plans()?;
    for plan in plans.iter().filter(|plan| plan.name == ADAPTIVE_PLAN_NAME) {
        let guid =
            parse_guid(&plan.guid).ok_or_else(|| "Invalid managed power plan GUID.".to_owned())?;
        let description = read_scheme_description(&guid)?;
        let Some(restore_guid) = managed_adaptive_restore_guid(&plan.name, &description) else {
            continue;
        };
        let restore_exists = plans
            .iter()
            .any(|candidate| candidate.guid.eq_ignore_ascii_case(restore_guid));

        if plan.active {
            if !restore_exists {
                continue;
            }
            set_active(restore_guid)?;
        }
        delete_plan(&plan.guid)?;
    }

    Ok(())
}

pub fn read_plan_personality(guid: &str) -> Result<PowerPlanPersonality, String> {
    let scheme = parse_guid(guid).ok_or_else(|| "Invalid power plan GUID.".to_owned())?;
    Ok(PowerPlanPersonality::from_power_value(read_ac_value(
        &scheme,
        &GUID_NO_SUBGROUP,
        &GUID_POWERSCHEME_PERSONALITY,
    )?))
}

pub fn apply_processor_power_values(
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
        &GUID_PROCESSOR_PERFORMANCE_BOOST_POLICY,
        values.ac.boost_policy,
    )?;
    write_dc_value(
        &scheme,
        &GUID_PROCESSOR_SETTINGS_SUBGROUP,
        &GUID_PROCESSOR_PERFORMANCE_BOOST_POLICY,
        values.dc.boost_policy,
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
        set_active(guid)?;
    }

    Ok(())
}

pub fn read_processor_power_values(guid: &str) -> Result<ProcessorPowerAcDcValues, String> {
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
            read_ac_value(
                &scheme,
                &GUID_PROCESSOR_SETTINGS_SUBGROUP,
                &GUID_PROCESSOR_PERFORMANCE_BOOST_POLICY,
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
            read_dc_value(
                &scheme,
                &GUID_PROCESSOR_SETTINGS_SUBGROUP,
                &GUID_PROCESSOR_PERFORMANCE_BOOST_POLICY,
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

impl EffectivePowerModeMonitor {
    pub fn new() -> Result<Self, String> {
        let mode = Arc::new(AtomicI32::new(EFFECTIVE_POWER_MODE_UNKNOWN));
        let mut registration = null_mut();
        // SAFETY: mode remains alive in self for the full registration lifetime, registration is
        // writable, and the callback has the required static ABI.
        let result = unsafe {
            PowerRegisterForEffectivePowerModeNotifications(
                EFFECTIVE_POWER_MODE_V2,
                Some(effective_power_mode_callback),
                Arc::as_ptr(&mode).cast(),
                &mut registration,
            )
        };

        if result >= 0 {
            Ok(Self { mode, registration })
        } else {
            Err(format!(
                "PowerRegisterForEffectivePowerModeNotifications failed with HRESULT {result:#x}."
            ))
        }
    }

    pub fn snapshot(&self) -> EffectivePowerMode {
        EffectivePowerMode::from_raw(self.mode.load(Ordering::Relaxed))
    }
}

impl Drop for EffectivePowerModeMonitor {
    fn drop(&mut self) {
        if !self.registration.is_null() {
            // SAFETY: registration was returned by the successful registration call and is
            // unregistered exactly once before the context Arc is dropped.
            unsafe {
                PowerUnregisterFromEffectivePowerModeNotifications(self.registration.cast_const());
            }
        }
    }
}

unsafe extern "system" fn effective_power_mode_callback(
    mode: EFFECTIVE_POWER_MODE,
    context: *const c_void,
) {
    if !context.is_null() {
        // SAFETY: context is the Arc-owned AtomicI32 pointer registered by new and remains alive
        // until after the callback is unregistered.
        unsafe {
            (*(context as *const AtomicI32)).store(mode, Ordering::Relaxed);
        }
    }
}

const EFFECTIVE_POWER_MODE_UNKNOWN: i32 = -1;

const GUID_NO_SUBGROUP: GUID = GUID {
    data1: 0xfea3413e,
    data2: 0x7e05,
    data3: 0x4911,
    data4: [0x9a, 0x71, 0x70, 0x03, 0x31, 0xf1, 0xc2, 0x94],
};

const GUID_POWERSCHEME_PERSONALITY: GUID = GUID {
    data1: 0x245d8541,
    data2: 0x3943,
    data3: 0x4422,
    data4: [0xb0, 0x25, 0x13, 0xa7, 0x84, 0xf6, 0x79, 0xb7],
};

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

const GUID_PROCESSOR_PERFORMANCE_BOOST_POLICY: GUID = GUID {
    data1: 0x45bcc044,
    data2: 0xd885,
    data3: 0x43e2,
    data4: [0x86, 0x05, 0xee, 0x0e, 0xc6, 0xe9, 0x6b, 0x59],
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
    // SAFETY: scheme, subgroup, and setting are valid GUID references for the duration of the
    // synchronous call.
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
    // SAFETY: scheme, subgroup, and setting are valid GUID references for the duration of the
    // synchronous call.
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
    // SAFETY: GUID references remain live and value is writable for the synchronous call.
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
    // SAFETY: GUID references remain live and value is writable for the synchronous call.
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
    // SAFETY: guid and buffer_size are writable and the buffer is exactly one GUID in size.
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
    // SAFETY: guid is valid and buffer_size is writable; a null buffer requests the required size.
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
    // SAFETY: buffer has the requested writable size and guid remains live for the call.
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

    let name = decode_power_string(&buffer);
    if name.is_empty() {
        Err("PowerReadFriendlyName returned an empty name.".to_owned())
    } else {
        Ok(name)
    }
}

fn read_scheme_description(guid: &GUID) -> Result<String, String> {
    let mut buffer_size = 0;
    // SAFETY: guid is valid and buffer_size is writable; a null buffer requests the required size.
    let size_result = unsafe {
        PowerReadDescription(
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
            "PowerReadDescription failed to read buffer size with error code {size_result}."
        ));
    }
    if buffer_size == 0 {
        return Ok(String::new());
    }

    let mut buffer = vec![0_u8; buffer_size as usize];
    // SAFETY: buffer has the requested writable size and guid remains live for the call.
    let result = unsafe {
        PowerReadDescription(
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
            "PowerReadDescription failed with error code {result}."
        ));
    }

    Ok(decode_power_string(&buffer))
}

fn write_scheme_name(guid: &GUID, name: &str) -> Result<(), String> {
    let buffer = encode_power_string(name);
    // SAFETY: buffer is a terminated UTF-16 byte sequence and all GUID references remain live.
    let result = unsafe {
        PowerWriteFriendlyName(
            null_mut(),
            guid,
            null_mut(),
            null_mut(),
            buffer.as_ptr(),
            buffer.len() as u32,
        )
    };
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "PowerWriteFriendlyName failed with error code {result}."
        ))
    }
}

fn write_scheme_description(guid: &GUID, description: &str) -> Result<(), String> {
    let buffer = encode_power_string(description);
    // SAFETY: buffer is a terminated UTF-16 byte sequence and all GUID references remain live.
    let result = unsafe {
        PowerWriteDescription(
            null_mut(),
            guid,
            null_mut(),
            null_mut(),
            buffer.as_ptr(),
            buffer.len() as u32,
        )
    };
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "PowerWriteDescription failed with error code {result}."
        ))
    }
}

fn encode_power_string(value: &str) -> Vec<u8> {
    value
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn decode_power_string(buffer: &[u8]) -> String {
    let utf16 = buffer
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .take_while(|code| *code != 0)
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&utf16)
}

fn active_scheme_guid() -> Result<String, String> {
    let mut guid_ptr: *mut GUID = null_mut();
    // SAFETY: guid_ptr is a writable out-pointer whose successful allocation is released with
    // LocalFree below.
    let result = unsafe { PowerGetActiveScheme(null_mut(), &mut guid_ptr) };
    if result != 0 {
        return Err(format!(
            "PowerGetActiveScheme failed with error code {result}."
        ));
    }
    if guid_ptr.is_null() {
        return Err("PowerGetActiveScheme returned no active plan.".to_owned());
    }

    // SAFETY: PowerGetActiveScheme succeeded and returned a non-null GUID allocation.
    let guid = unsafe { *guid_ptr };
    // SAFETY: guid_ptr was allocated by PowerGetActiveScheme and is freed exactly once.
    unsafe {
        LocalFree(guid_ptr.cast());
    }
    Ok(format_guid(&guid))
}

fn managed_adaptive_restore_guid<'a>(plan_name: &str, description: &'a str) -> Option<&'a str> {
    if plan_name != ADAPTIVE_PLAN_NAME {
        return None;
    }
    let restore_guid = description.strip_prefix(ADAPTIVE_PLAN_DESCRIPTION_PREFIX)?;
    parse_guid(restore_guid).map(|_| restore_guid)
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
    use crate::power::{EffectivePowerMode, PowerPlanPersonality, ProcessorPowerPreset};

    use super::*;

    #[test]
    fn parses_and_formats_guid() {
        let raw = "381b4222-f694-41f0-9685-ff5bb260df2e";
        let guid = parse_guid(raw).unwrap();

        assert_eq!(format_guid(&guid), raw);
    }

    #[test]
    fn power_strings_round_trip() {
        let encoded = encode_power_string("Winderust 電源");

        assert_eq!(decode_power_string(&encoded), "Winderust 電源");
    }

    #[test]
    fn recognizes_only_winderust_managed_adaptive_plan() {
        let restore_guid = "381b4222-f694-41f0-9685-ff5bb260df2e";

        assert_eq!(
            managed_adaptive_restore_guid(
                ADAPTIVE_PLAN_NAME,
                &format!("{ADAPTIVE_PLAN_DESCRIPTION_PREFIX}{restore_guid}"),
            ),
            Some(restore_guid)
        );
        assert_eq!(
            managed_adaptive_restore_guid(
                "Another App Adaptive",
                &format!("Another App managed adaptive plan; restore={restore_guid}"),
            ),
            None
        );
    }

    #[test]
    fn maps_windows_power_modes() {
        assert_eq!(
            PowerPlanPersonality::from_power_value(2),
            PowerPlanPersonality::Balanced
        );
        assert_eq!(
            EffectivePowerMode::from_raw(4),
            EffectivePowerMode::MaxPerformance
        );
        assert_eq!(
            EffectivePowerMode::from_raw(-1),
            EffectivePowerMode::Unknown
        );
    }

    #[test]
    fn processor_power_presets_use_explicit_percentages() {
        let performance = ProcessorPowerValues::for_preset(ProcessorPowerPreset::Performance);
        assert_eq!(performance.core_parking_min, 100);
        assert_eq!(performance.performance_min, 100);
        assert_eq!(performance.performance_max, 100);
        assert_eq!(performance.boost_policy, 100);
        assert_eq!(performance.boost_mode, ProcessorBoostMode::Aggressive);

        let saver = ProcessorPowerValues::for_preset(ProcessorPowerPreset::Saver);
        assert_eq!(saver.core_parking_min, 0);
        assert_eq!(saver.performance_min, 5);
        assert_eq!(saver.performance_max, 60);
        assert_eq!(saver.boost_policy, 0);
        assert_eq!(saver.boost_mode, ProcessorBoostMode::Disabled);
    }

    #[test]
    fn processor_power_values_normalize_to_valid_percentages() {
        let values = ProcessorPowerValues::new_with_boost_mode(
            140,
            75,
            20,
            150,
            ProcessorBoostMode::Enabled,
        )
        .normalized();

        assert_eq!(values.core_parking_min, 100);
        assert_eq!(values.performance_min, 75);
        assert_eq!(values.performance_max, 75);
        assert_eq!(values.boost_policy, 100);
        assert_eq!(values.boost_mode, ProcessorBoostMode::Enabled);
    }

    #[test]
    fn processor_power_ac_dc_values_normalize_each_power_source() {
        let values = ProcessorPowerAcDcValues::new(
            ProcessorPowerValues::new_with_boost_mode(
                120,
                90,
                80,
                120,
                ProcessorBoostMode::Enabled,
            ),
            ProcessorPowerValues::new_with_boost_mode(10, 20, 15, 30, ProcessorBoostMode::Enabled),
        )
        .normalized();

        assert_eq!(values.ac.core_parking_min, 100);
        assert_eq!(values.ac.performance_min, 90);
        assert_eq!(values.ac.performance_max, 90);
        assert_eq!(values.ac.boost_policy, 100);
        assert_eq!(values.dc.core_parking_min, 10);
        assert_eq!(values.dc.performance_min, 20);
        assert_eq!(values.dc.performance_max, 20);
        assert_eq!(values.dc.boost_policy, 30);
    }
}
