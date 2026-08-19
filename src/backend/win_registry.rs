use std::io;

use windows_sys::Win32::System::Registry::{HKEY, KEY_READ, KEY_WRITE};
use winreg::{enums::RegType, RegKey};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RegistryError {
    OpenForRead(String),
    ReadValue(String),
    OpenForWrite(String),
    CreateKey(String),
    WriteValue(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenForRead(error) => {
                write!(formatter, "Failed to open registry key for read: {error}")
            }
            Self::ReadValue(error) => write!(formatter, "Failed to read registry value: {error}"),
            Self::OpenForWrite(error) => {
                write!(formatter, "Failed to open registry key for write: {error}")
            }
            Self::CreateKey(error) => write!(formatter, "Failed to create registry key: {error}"),
            Self::WriteValue(error) => {
                write!(formatter, "Failed to write registry value: {error}")
            }
        }
    }
}

impl std::error::Error for RegistryError {}

pub(crate) fn read_registry_dword_root(root: HKEY, sub_key: &str, value_name: &str) -> Option<u32> {
    try_read_registry_dword_root(root, sub_key, value_name)
        .ok()
        .flatten()
}

pub(crate) fn try_read_registry_dword_root(
    root: HKEY,
    sub_key: &str,
    value_name: &str,
) -> Result<Option<u32>, RegistryError> {
    let key = match RegKey::predef(root).open_subkey_with_flags(sub_key, KEY_READ) {
        Ok(key) => key,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(RegistryError::OpenForRead(error.to_string())),
    };
    match key.get_value(value_name) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(RegistryError::ReadValue(error.to_string())),
    }
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
) -> Result<(), RegistryError> {
    let key = RegKey::predef(root)
        .open_subkey_with_flags(sub_key, KEY_WRITE)
        .map_err(|error| RegistryError::OpenForWrite(error.to_string()))?;
    key.set_value(value_name, &value)
        .map_err(|error| RegistryError::WriteValue(error.to_string()))
}

pub(crate) fn write_registry_dword_create_root(
    root: HKEY,
    sub_key: &str,
    value_name: &str,
    value: u32,
) -> Result<(), RegistryError> {
    let (key, _) = RegKey::predef(root)
        .create_subkey_with_flags(sub_key, KEY_WRITE)
        .map_err(|error| RegistryError::CreateKey(error.to_string()))?;
    key.set_value(value_name, &value)
        .map_err(|error| RegistryError::WriteValue(error.to_string()))
}
