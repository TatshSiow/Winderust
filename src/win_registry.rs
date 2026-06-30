use std::{mem::size_of, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::ERROR_SUCCESS,
    System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, REG_DWORD, REG_OPTION_NON_VOLATILE,
    },
};

use crate::win_util::wide_null;

pub(crate) fn read_registry_dword_root(root: HKEY, sub_key: &str, value_name: &str) -> Option<u32> {
    let sub_key = wide_null(sub_key);
    let value_name = wide_null(value_name);
    let mut key: HKEY = null_mut();
    let status = unsafe { RegOpenKeyExW(root, sub_key.as_ptr(), 0, KEY_QUERY_VALUE, &mut key) };
    if status != ERROR_SUCCESS {
        return None;
    }

    let key = RegistryKey(key);
    let mut value_type = 0;
    let mut value = 0_u32;
    let mut value_size = size_of::<u32>() as u32;
    let status = unsafe {
        RegQueryValueExW(
            key.0,
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
    let sub_key = wide_null(sub_key);
    let value_name = wide_null(value_name);
    let mut key: HKEY = null_mut();
    let status = unsafe { RegOpenKeyExW(root, sub_key.as_ptr(), 0, KEY_QUERY_VALUE, &mut key) };
    if status != ERROR_SUCCESS {
        return None;
    }

    let key = RegistryKey(key);
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
    let sub_key = wide_null(sub_key);
    let value_name = wide_null(value_name);
    let mut key: HKEY = null_mut();
    let status = unsafe { RegOpenKeyExW(root, sub_key.as_ptr(), 0, KEY_SET_VALUE, &mut key) };
    if status != ERROR_SUCCESS {
        return Err(registry_error_message(
            "open registry key for write",
            status,
        ));
    }

    let key = RegistryKey(key);
    write_registry_dword(&key, &value_name, value, "write registry value")
}

pub(crate) fn write_registry_dword_create_root(
    root: HKEY,
    sub_key: &str,
    value_name: &str,
    value: u32,
) -> Result<(), String> {
    let sub_key = wide_null(sub_key);
    let value_name = wide_null(value_name);
    let mut key: HKEY = null_mut();
    let mut disposition = 0_u32;
    let status = unsafe {
        RegCreateKeyExW(
            root,
            sub_key.as_ptr(),
            0,
            null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            null_mut(),
            &mut key,
            &mut disposition,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(registry_error_message(
            "create registry key for backup",
            status,
        ));
    }

    let key = RegistryKey(key);
    write_registry_dword(&key, &value_name, value, "write registry backup")
}

fn write_registry_dword(
    key: &RegistryKey,
    value_name: &[u16],
    value: u32,
    action: &str,
) -> Result<(), String> {
    let status = unsafe {
        RegSetValueExW(
            key.0,
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
        Err(registry_error_message(action, status))
    }
}

fn registry_error_message(action: &str, status: u32) -> String {
    format!("Failed to {action}: Windows error {status}.")
}

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
