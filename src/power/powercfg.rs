use std::ptr::null_mut;

use windows_sys::{
    core::GUID,
    Win32::{
        Foundation::{LocalFree, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
        System::Power::{
            PowerEnumerate, PowerGetActiveScheme, PowerReadFriendlyName, PowerSetActiveScheme,
            ACCESS_SCHEME,
        },
    },
};

use super::PowerPlan;

#[derive(Debug, Default)]
pub struct PowerPlanManager;

impl PowerPlanManager {
    pub fn list_plans(&self) -> Result<Vec<PowerPlan>, String> {
        let active_guid = active_scheme_guid().ok();
        let mut plans = Vec::new();
        let mut index = 0;

        loop {
            let Some(guid) = enumerate_scheme_guid(index)? else {
                break;
            };
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
    use super::*;

    #[test]
    fn parses_and_formats_guid() {
        let raw = "381b4222-f694-41f0-9685-ff5bb260df2e";
        let guid = parse_guid(raw).unwrap();

        assert_eq!(format_guid(&guid), raw);
    }
}
