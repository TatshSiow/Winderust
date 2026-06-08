use std::path::PathBuf;
use std::{mem::size_of, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        RegCloseKey, RegCreateKeyW, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
    },
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "PowerLeaf";
const STARTUP_ALLOWED_EXTENSIONS: &[&str] = &["exe", "com"];

pub fn set_startup_with_windows(enabled: bool) -> Result<(), String> {
    if enabled {
        enable_startup()
    } else {
        disable_startup()
    }
}

fn enable_startup() -> Result<(), String> {
    let key = create_run_key()?;
    let value_name = wide_null(VALUE_NAME);
    let command = startup_command()?;
    let command = wide_null(&command);
    let data = unsafe {
        std::slice::from_raw_parts(
            command.as_ptr() as *const u8,
            command.len() * size_of::<u16>(),
        )
    };

    let status = unsafe {
        RegSetValueExW(
            key.0,
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
        Err(format!(
            "failed to enable Windows startup entry: error {status}"
        ))
    }
}

fn disable_startup() -> Result<(), String> {
    let Some(key) = open_run_key_for_write()? else {
        return Ok(());
    };

    let value_name = wide_null(VALUE_NAME);
    let status = unsafe { RegDeleteValueW(key.0, value_name.as_ptr()) };
    if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(format!(
            "failed to disable Windows startup entry: error {status}"
        ))
    }
}

fn create_run_key() -> Result<RegKey, String> {
    let sub_key = wide_null(RUN_KEY);
    let mut key = null_mut();
    let status = unsafe { RegCreateKeyW(HKEY_CURRENT_USER, sub_key.as_ptr(), &mut key) };

    if status == ERROR_SUCCESS {
        Ok(RegKey(key))
    } else {
        Err(format!(
            "failed to open Windows startup registry key: error {status}"
        ))
    }
}

fn open_run_key_for_write() -> Result<Option<RegKey>, String> {
    let sub_key = wide_null(RUN_KEY);
    let mut key = null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            sub_key.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut key,
        )
    };

    if status == ERROR_SUCCESS {
        Ok(Some(RegKey(key)))
    } else if status == ERROR_FILE_NOT_FOUND {
        Ok(None)
    } else {
        Err(format!(
            "failed to open Windows startup registry key: error {status}"
        ))
    }
}

fn startup_command() -> Result<String, String> {
    let exe = std::env::current_exe()
        .map_err(|err| format!("failed to read PowerLeaf executable path: {err}"))?;
    let exe = sanitize_startup_executable(exe)?;
    Ok(format!("\"{}\"", exe.display()))
}

fn sanitize_startup_executable(exe: PathBuf) -> Result<PathBuf, String> {
    let exe = exe
        .canonicalize()
        .map_err(|err| format!("failed to resolve PowerLeaf executable path: {err}"))?;

    if !exe.is_file() {
        return Err("PowerLeaf executable path is not a file.".to_owned());
    }
    if let Some(extension) = exe.extension().and_then(|extension| extension.to_str()) {
        if !STARTUP_ALLOWED_EXTENSIONS
            .iter()
            .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        {
            return Err(format!(
                "PowerLeaf executable path has unexpected extension ({extension:?})."
            ));
        }
    } else {
        return Err("PowerLeaf executable path has no extension.".to_owned());
    }

    Ok(exe)
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

struct RegKey(HKEY);

impl Drop for RegKey {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }
}
