use std::io;

use windows_sys::Win32::System::Registry::{HKEY, KEY_READ, KEY_WRITE};
use winreg::RegKey;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RegistryError {
    #[error("Failed to open registry key for read: {0}")]
    OpenForRead(String),
    #[error("Failed to read registry value: {0}")]
    ReadValue(String),
    #[error("Failed to open registry key for write: {0}")]
    OpenForWrite(String),
    #[error("Failed to create registry key: {0}")]
    CreateKey(String),
    #[error("Failed to write registry value: {0}")]
    WriteValue(String),
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
