use std::{ffi::OsString, path::PathBuf};

use winreg::{enums::HKEY_CURRENT_USER, RegKey};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Winderust";
const STARTUP_ALLOWED_EXTENSIONS: &[&str] = &["exe", "com"];

pub fn set_startup_with_windows(enabled: bool) -> Result<(), String> {
    if enabled {
        enable_startup()
    } else {
        disable_startup()
    }
}

fn enable_startup() -> Result<(), String> {
    let command = startup_command()?;
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(RUN_KEY)
        .map_err(|error| format!("Failed to create startup registry key: {error}"))?;
    key.set_value(VALUE_NAME, &command)
        .map_err(|error| format!("Failed to write startup registry value: {error}"))
}

fn disable_startup() -> Result<(), String> {
    let key = RegKey::predef(HKEY_CURRENT_USER);
    let key = match key.open_subkey_with_flags(RUN_KEY, winreg::enums::KEY_SET_VALUE) {
        Ok(key) => key,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!("Failed to open startup registry key: {error}"));
        }
    };
    match key.delete_value(VALUE_NAME) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Failed to delete startup registry value: {error}")),
    }
}

fn startup_command() -> Result<OsString, String> {
    let exe = std::env::current_exe()
        .map_err(|err| format!("failed to read Winderust executable path: {err}"))?;
    let exe = sanitize_startup_executable(exe)?;
    let mut command = OsString::from("\"");
    command.push(exe);
    command.push("\"");
    Ok(command)
}

fn sanitize_startup_executable(exe: PathBuf) -> Result<PathBuf, String> {
    let exe = exe
        .canonicalize()
        .map_err(|err| format!("failed to resolve Winderust executable path: {err}"))?;

    if !exe.is_file() {
        return Err("Winderust executable path is not a file.".to_owned());
    }
    if let Some(extension) = exe.extension().and_then(|extension| extension.to_str()) {
        if !STARTUP_ALLOWED_EXTENSIONS
            .iter()
            .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        {
            return Err(format!(
                "Winderust executable path has unexpected extension ({extension:?})."
            ));
        }
    } else {
        return Err("Winderust executable path has no extension.".to_owned());
    }

    Ok(exe)
}
