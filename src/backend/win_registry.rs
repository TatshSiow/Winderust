use std::io;

use windows_sys::Win32::System::Registry::{HKEY, KEY_READ, KEY_WRITE};
use winreg::{enums::RegType, RegKey};

pub(crate) fn read_registry_dword_root(root: HKEY, sub_key: &str, value_name: &str) -> Option<u32> {
    RegKey::predef(root)
        .open_subkey_with_flags(sub_key, KEY_READ)
        .ok()?
        .get_value(value_name)
        .ok()
}

pub(crate) fn read_registry_binary_root(
    root: HKEY,
    sub_key: &str,
    value_name: &str,
) -> Option<Vec<u8>> {
    let value = RegKey::predef(root)
        .open_subkey_with_flags(sub_key, KEY_READ)
        .ok()?
        .get_raw_value(value_name)
        .ok()?;
    (value.vtype == RegType::REG_BINARY).then(|| value.bytes.into_owned())
}

pub(crate) fn write_registry_dword_root(
    root: HKEY,
    sub_key: &str,
    value_name: &str,
    value: u32,
) -> Result<(), String> {
    let key = RegKey::predef(root)
        .open_subkey_with_flags(sub_key, KEY_WRITE)
        .map_err(registry_error("open registry key for write"))?;
    key.set_value(value_name, &value)
        .map_err(registry_error("write registry value"))
}

pub(crate) fn write_registry_dword_create_root(
    root: HKEY,
    sub_key: &str,
    value_name: &str,
    value: u32,
) -> Result<(), String> {
    let (key, _) = RegKey::predef(root)
        .create_subkey_with_flags(sub_key, KEY_WRITE)
        .map_err(registry_error("create registry key"))?;
    key.set_value(value_name, &value)
        .map_err(registry_error("write registry value"))
}

fn registry_error(action: &'static str) -> impl FnOnce(io::Error) -> String {
    move |error| format!("Failed to {action}: {error}")
}
