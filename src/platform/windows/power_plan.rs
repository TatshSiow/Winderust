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

const EFFECTIVE_POWER_MODE_UNKNOWN: i32 = -1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PowerSetting {
    Personality,
    CoreParkingMinimum,
    PerformanceMinimum,
    PerformanceMaximum,
    BoostPolicy,
    BoostMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PowerSchemeRecord {
    pub(crate) guid: String,
    pub(crate) name: String,
}

#[derive(Debug)]
pub(crate) struct EffectivePowerModeRegistration {
    mode: Arc<AtomicI32>,
    registration: *mut c_void,
}

pub(crate) fn list_schemes() -> Result<Vec<PowerSchemeRecord>, String> {
    let mut schemes = Vec::new();
    let mut index = 0;
    while let Some(guid) = enumerate_scheme_guid(index)? {
        let guid_text = format_guid(&guid);
        let name = read_scheme_name_raw(&guid).unwrap_or_else(|_| guid_text.clone());
        schemes.push(PowerSchemeRecord {
            guid: guid_text,
            name,
        });
        index += 1;
    }
    Ok(schemes)
}

pub(crate) fn active_scheme_guid() -> Result<String, String> {
    let mut guid_ptr: *mut GUID = null_mut();
    // SAFETY: guid_ptr is a writable out-pointer whose successful allocation is released with
    // LocalFree below.
    let result = unsafe { PowerGetActiveScheme(null_mut(), &mut guid_ptr) };
    if result != ERROR_SUCCESS {
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

pub(crate) fn set_active(guid: &str) -> Result<(), String> {
    let guid = parse_guid(guid).ok_or_else(|| "Invalid power plan GUID.".to_owned())?;
    // SAFETY: guid is a fully parsed GUID and a null root key selects the current user.
    let result = unsafe { PowerSetActiveScheme(null_mut(), &guid) };
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "PowerSetActiveScheme failed with error code {result}."
        ))
    }
}

pub(crate) fn duplicate_scheme(source_guid: &str) -> Result<String, String> {
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
    Ok(format_guid(&duplicate))
}

pub(crate) fn delete_scheme(guid: &str) -> Result<(), String> {
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

pub(crate) fn read_scheme_description(guid: &str) -> Result<String, String> {
    let guid = parse_guid(guid).ok_or_else(|| "Invalid power plan GUID.".to_owned())?;
    let mut buffer_size = 0;
    // SAFETY: guid is valid and buffer_size is writable; a null buffer requests the required size.
    let size_result = unsafe {
        PowerReadDescription(
            null_mut(),
            &guid,
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
            &guid,
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

pub(crate) fn write_scheme_name(guid: &str, name: &str) -> Result<(), String> {
    let guid = parse_guid(guid).ok_or_else(|| "Invalid power plan GUID.".to_owned())?;
    let buffer = encode_power_string(name);
    // SAFETY: buffer is a terminated UTF-16 byte sequence and all GUID references remain live.
    let result = unsafe {
        PowerWriteFriendlyName(
            null_mut(),
            &guid,
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

pub(crate) fn write_scheme_description(guid: &str, description: &str) -> Result<(), String> {
    let guid = parse_guid(guid).ok_or_else(|| "Invalid power plan GUID.".to_owned())?;
    let buffer = encode_power_string(description);
    // SAFETY: buffer is a terminated UTF-16 byte sequence and all GUID references remain live.
    let result = unsafe {
        PowerWriteDescription(
            null_mut(),
            &guid,
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

pub(crate) fn read_ac_value(guid: &str, setting: PowerSetting) -> Result<u32, String> {
    let scheme = parse_guid(guid).ok_or_else(|| "Invalid power plan GUID.".to_owned())?;
    let (subgroup, setting) = setting_guids(setting);
    let mut value = 0;
    // SAFETY: GUID references remain live and value is writable for the synchronous call.
    let result =
        unsafe { PowerReadACValueIndex(null_mut(), &scheme, &subgroup, &setting, &mut value) };
    if result == ERROR_SUCCESS {
        Ok(value)
    } else {
        Err(format!(
            "PowerReadACValueIndex({}) failed with error code {result}.",
            format_guid(&setting)
        ))
    }
}

pub(crate) fn read_dc_value(guid: &str, setting: PowerSetting) -> Result<u32, String> {
    let scheme = parse_guid(guid).ok_or_else(|| "Invalid power plan GUID.".to_owned())?;
    let (subgroup, setting) = setting_guids(setting);
    let mut value = 0;
    // SAFETY: GUID references remain live and value is writable for the synchronous call.
    let result =
        unsafe { PowerReadDCValueIndex(null_mut(), &scheme, &subgroup, &setting, &mut value) };
    if result == ERROR_SUCCESS {
        Ok(value)
    } else {
        Err(format!(
            "PowerReadDCValueIndex({}) failed with error code {result}.",
            format_guid(&setting)
        ))
    }
}

pub(crate) fn write_ac_value(guid: &str, setting: PowerSetting, value: u32) -> Result<(), String> {
    let scheme = parse_guid(guid).ok_or_else(|| "Invalid power plan GUID.".to_owned())?;
    let (subgroup, setting) = setting_guids(setting);
    // SAFETY: scheme, subgroup, and setting are valid GUID references for the synchronous call.
    let result = unsafe { PowerWriteACValueIndex(null_mut(), &scheme, &subgroup, &setting, value) };
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "PowerWriteACValueIndex({}) failed with error code {result}.",
            format_guid(&setting)
        ))
    }
}

pub(crate) fn write_dc_value(guid: &str, setting: PowerSetting, value: u32) -> Result<(), String> {
    let scheme = parse_guid(guid).ok_or_else(|| "Invalid power plan GUID.".to_owned())?;
    let (subgroup, setting) = setting_guids(setting);
    // SAFETY: scheme, subgroup, and setting are valid GUID references for the synchronous call.
    let result = unsafe { PowerWriteDCValueIndex(null_mut(), &scheme, &subgroup, &setting, value) };
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "PowerWriteDCValueIndex({}) failed with error code {result}.",
            format_guid(&setting)
        ))
    }
}

pub(crate) fn is_valid_guid(value: &str) -> bool {
    parse_guid(value).is_some()
}

impl EffectivePowerModeRegistration {
    pub(crate) fn new() -> Result<Self, String> {
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

        if result == 0 {
            Ok(Self { mode, registration })
        } else {
            Err(format!(
                "PowerRegisterForEffectivePowerModeNotifications failed with HRESULT {result:#x}."
            ))
        }
    }

    pub(crate) fn snapshot_raw(&self) -> i32 {
        self.mode.load(Ordering::Relaxed)
    }
}

impl Drop for EffectivePowerModeRegistration {
    fn drop(&mut self) {
        if self.registration.is_null() {
            return;
        }

        // SAFETY: registration was returned by the successful registration call and is
        // unregistered exactly once; successful cleanup waits for callbacks to finish.
        let result = unsafe {
            PowerUnregisterFromEffectivePowerModeNotifications(self.registration.cast_const())
        };
        if result != 0 {
            eprintln!(
                "PowerUnregisterFromEffectivePowerModeNotifications failed with HRESULT {result:#x}."
            );
            // ponytail: Retain the callback context after failed unregister; process exit is the
            // only safe cleanup while Windows may still invoke the callback.
            std::mem::forget(Arc::clone(&self.mode));
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

fn read_scheme_name_raw(guid: &GUID) -> Result<String, String> {
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

fn setting_guids(setting: PowerSetting) -> (GUID, GUID) {
    match setting {
        PowerSetting::Personality => (GUID_NO_SUBGROUP, GUID_POWERSCHEME_PERSONALITY),
        PowerSetting::CoreParkingMinimum => (
            GUID_PROCESSOR_SETTINGS_SUBGROUP,
            GUID_CORE_PARKING_MIN_CORES,
        ),
        PowerSetting::PerformanceMinimum => (
            GUID_PROCESSOR_SETTINGS_SUBGROUP,
            GUID_PROCESSOR_PERFORMANCE_MIN,
        ),
        PowerSetting::PerformanceMaximum => (
            GUID_PROCESSOR_SETTINGS_SUBGROUP,
            GUID_PROCESSOR_PERFORMANCE_MAX,
        ),
        PowerSetting::BoostPolicy => (
            GUID_PROCESSOR_SETTINGS_SUBGROUP,
            GUID_PROCESSOR_PERFORMANCE_BOOST_POLICY,
        ),
        PowerSetting::BoostMode => (
            GUID_PROCESSOR_SETTINGS_SUBGROUP,
            GUID_PROCESSOR_PERFORMANCE_BOOST_MODE,
        ),
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
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| u16::from_le_bytes(*chunk))
        .take_while(|code| *code != 0)
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&utf16)
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

    let prefix = parse_hex_bytes::<2>(parts[3])?;
    let suffix = parse_hex_bytes::<6>(parts[4])?;
    let mut data4 = [0_u8; 8];
    data4[..2].copy_from_slice(&prefix);
    data4[2..].copy_from_slice(&suffix);

    Some(GUID {
        data1: u32::from_str_radix(parts[0], 16).ok()?,
        data2: u16::from_str_radix(parts[1], 16).ok()?,
        data3: u16::from_str_radix(parts[2], 16).ok()?,
        data4,
    })
}

fn parse_hex_bytes<const N: usize>(value: &str) -> Option<[u8; N]> {
    let bytes = value.as_bytes();
    if bytes.len() != N * 2 {
        return None;
    }

    let mut parsed = [0_u8; N];
    for (index, output) in parsed.iter_mut().enumerate() {
        let high = hex_nibble(bytes[index * 2])?;
        let low = hex_nibble(bytes[index * 2 + 1])?;
        *output = (high << 4) | low;
    }
    Some(parsed)
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_guid() {
        let raw = "381b4222-f694-41f0-9685-ff5bb260df2e";
        let guid = parse_guid(raw).unwrap();

        assert_eq!(format_guid(&guid), raw);
    }

    #[test]
    fn rejects_non_ascii_guid_without_panicking() {
        assert!(parse_guid("381b4222-f694-41f0-測a-ff5bb260df2e").is_none());
    }

    #[test]
    fn power_strings_round_trip() {
        let encoded = encode_power_string("Winderust Adaptive 計畫");

        assert_eq!(decode_power_string(&encoded), "Winderust Adaptive 計畫");
    }
}
