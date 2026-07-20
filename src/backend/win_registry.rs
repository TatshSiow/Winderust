use std::{mem::size_of, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW, HKEY, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, REG_DWORD,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    },
};

use crate::win_util::wide_null;

pub(crate) fn read_registry_dword_root(root: HKEY, sub_key: &str, value_name: &str) -> Option<u32> {
    let key = open_registry_key(root, sub_key, KEY_QUERY_VALUE)
        .ok()
        .flatten()?;
    read_registry_dword(&key, value_name)
}

pub(crate) fn read_registry_dword(key: &RegistryKey, value_name: &str) -> Option<u32> {
    let value_name = wide_null(value_name);
    let mut value_type = 0;
    let mut value = 0_u32;
    let mut value_size = size_of::<u32>() as u32;
    let status = unsafe {
        RegQueryValueExW(
            key.raw(),
            value_name.as_ptr(),
            null_mut(),
            &mut value_type,
            &mut value as *mut u32 as *mut u8,
            &mut value_size,
        )
    };

    if status == ERROR_SUCCESS && value_type == REG_DWORD && value_size == size_of::<u32>() as u32 {
        Some(value)
    } else {
        None
    }
}

pub(crate) fn read_registry_binary_root(
    root: HKEY,
    sub_key: &str,
    value_name: &str,
) -> Option<Vec<u8>> {
    let key = open_registry_key(root, sub_key, KEY_QUERY_VALUE)
        .ok()
        .flatten()?;
    let value_name = wide_null(value_name);
    let mut value_type = 0;
    let mut value_size = 0_u32;
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            value_name.as_ptr(),
            null_mut(),
            &mut value_type,
            null_mut(),
            &mut value_size,
        )
    };
    if status != ERROR_SUCCESS || value_type != REG_BINARY || value_size == 0 {
        return None;
    }

    let mut value = vec![0; value_size as usize];
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            value_name.as_ptr(),
            null_mut(),
            &mut value_type,
            value.as_mut_ptr(),
            &mut value_size,
        )
    };
    if status == ERROR_SUCCESS && value_type == REG_BINARY {
        value.truncate(value_size as usize);
        Some(value)
    } else {
        None
    }
}

pub(crate) fn write_registry_dword_root(
    root: HKEY,
    sub_key: &str,
    value_name: &str,
    value: u32,
) -> Result<(), String> {
    let key = open_registry_key(root, sub_key, KEY_SET_VALUE)?.ok_or_else(|| {
        registry_error_message("open registry key for write", ERROR_FILE_NOT_FOUND)
    })?;
    write_registry_dword(&key, value_name, value)
}

pub(crate) fn write_registry_dword_create_root(
    root: HKEY,
    sub_key: &str,
    value_name: &str,
    value: u32,
) -> Result<(), String> {
    let key = create_registry_key(root, sub_key, KEY_SET_VALUE)?;
    write_registry_dword(&key, value_name, value)
}

pub(crate) fn create_registry_key(
    root: HKEY,
    sub_key: &str,
    access: u32,
) -> Result<RegistryKey, String> {
    let sub_key = wide_null(sub_key);
    let mut key: HKEY = null_mut();
    let mut disposition = 0_u32;
    let status = unsafe {
        RegCreateKeyExW(
            root,
            sub_key.as_ptr(),
            0,
            null_mut(),
            REG_OPTION_NON_VOLATILE,
            access,
            null_mut(),
            &mut key,
            &mut disposition,
        )
    };

    if status == ERROR_SUCCESS {
        Ok(RegistryKey(key))
    } else {
        Err(registry_error_message("create registry key", status))
    }
}

pub(crate) fn open_registry_key(
    root: HKEY,
    sub_key: &str,
    access: u32,
) -> Result<Option<RegistryKey>, String> {
    let sub_key = wide_null(sub_key);
    let mut key: HKEY = null_mut();
    let status = unsafe { RegOpenKeyExW(root, sub_key.as_ptr(), 0, access, &mut key) };

    match status {
        ERROR_SUCCESS => Ok(Some(RegistryKey(key))),
        ERROR_FILE_NOT_FOUND => Ok(None),
        status => Err(registry_error_message("open registry key", status)),
    }
}

pub(crate) fn write_registry_dword(
    key: &RegistryKey,
    value_name: &str,
    value: u32,
) -> Result<(), String> {
    let value_name = wide_null(value_name);
    let status = unsafe {
        RegSetValueExW(
            key.raw(),
            value_name.as_ptr(),
            0,
            REG_DWORD,
            &value as *const u32 as *const u8,
            size_of::<u32>() as u32,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(registry_error_message("write registry value", status))
    }
}

pub(crate) fn write_registry_string(
    key: &RegistryKey,
    value_name: &str,
    value: &str,
) -> Result<(), String> {
    let value_name = wide_null(value_name);
    let value = wide_null(value);
    let data = unsafe {
        std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * size_of::<u16>())
    };
    let status = unsafe {
        RegSetValueExW(
            key.raw(),
            value_name.as_ptr(),
            0,
            REG_SZ,
            data.as_ptr(),
            data.len() as u32,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(registry_error_message("write registry value", status))
    }
}

pub(crate) fn delete_registry_value(key: &RegistryKey, value_name: &str) -> Result<(), String> {
    let value_name = wide_null(value_name);
    let status = unsafe { RegDeleteValueW(key.raw(), value_name.as_ptr()) };
    match status {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        status => Err(registry_error_message("delete registry value", status)),
    }
}

pub(crate) fn registry_error_message(action: &str, status: u32) -> String {
    format!("Failed to {action}: Windows error {status}.")
}

pub(crate) struct RegistryKey(HKEY);

impl RegistryKey {
    pub(crate) const fn raw(&self) -> HKEY {
        self.0
    }
}

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
