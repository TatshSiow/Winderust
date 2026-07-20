use std::path::PathBuf;

use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, KEY_SET_VALUE};

use crate::win_registry::{
    create_registry_key, delete_registry_value, open_registry_key, write_registry_string,
};

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
    let key = create_registry_key(HKEY_CURRENT_USER, RUN_KEY, KEY_SET_VALUE)?;
    write_registry_string(&key, VALUE_NAME, &command)
}

fn disable_startup() -> Result<(), String> {
    let Some(key) = open_registry_key(HKEY_CURRENT_USER, RUN_KEY, KEY_SET_VALUE)? else {
        return Ok(());
    };

    delete_registry_value(&key, VALUE_NAME)
}

fn startup_command() -> Result<String, String> {
    let exe = std::env::current_exe()
        .map_err(|err| format!("failed to read Winderust executable path: {err}"))?;
    let exe = sanitize_startup_executable(exe)?;
    Ok(format!("\"{}\"", exe.display()))
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
