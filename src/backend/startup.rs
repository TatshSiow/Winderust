use std::{ffi::OsString, path::PathBuf};

use winreg::{enums::HKEY_CURRENT_USER, RegKey};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Winderust";
const STARTUP_ALLOWED_EXTENSIONS: &[&str] = &["exe", "com"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartupRegistrationError {
    ReadExecutablePath(String),
    ResolveExecutablePath(String),
    ExecutableIsNotFile,
    ExecutableHasNoExtension,
    UnexpectedExecutableExtension(String),
    CreateRegistryKey(String),
    WriteRegistryValue(String),
    OpenRegistryKey(String),
    DeleteRegistryValue(String),
}

impl std::fmt::Display for StartupRegistrationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadExecutablePath(error) => {
                write!(
                    formatter,
                    "failed to read Winderust executable path: {error}"
                )
            }
            Self::ResolveExecutablePath(error) => {
                write!(
                    formatter,
                    "failed to resolve Winderust executable path: {error}"
                )
            }
            Self::ExecutableIsNotFile => {
                formatter.write_str("Winderust executable path is not a file.")
            }
            Self::ExecutableHasNoExtension => {
                formatter.write_str("Winderust executable path has no extension.")
            }
            Self::UnexpectedExecutableExtension(extension) => write!(
                formatter,
                "Winderust executable path has unexpected extension ({extension:?})."
            ),
            Self::CreateRegistryKey(error) => {
                write!(formatter, "Failed to create startup registry key: {error}")
            }
            Self::WriteRegistryValue(error) => {
                write!(formatter, "Failed to write startup registry value: {error}")
            }
            Self::OpenRegistryKey(error) => {
                write!(formatter, "Failed to open startup registry key: {error}")
            }
            Self::DeleteRegistryValue(error) => {
                write!(
                    formatter,
                    "Failed to delete startup registry value: {error}"
                )
            }
        }
    }
}

impl std::error::Error for StartupRegistrationError {}

pub(crate) fn set_startup_with_windows(enabled: bool) -> Result<(), StartupRegistrationError> {
    if enabled {
        enable_startup()
    } else {
        disable_startup()
    }
}

fn enable_startup() -> Result<(), StartupRegistrationError> {
    let command = startup_command()?;
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(RUN_KEY)
        .map_err(|error| StartupRegistrationError::CreateRegistryKey(error.to_string()))?;
    key.set_value(VALUE_NAME, &command)
        .map_err(|error| StartupRegistrationError::WriteRegistryValue(error.to_string()))
}

fn disable_startup() -> Result<(), StartupRegistrationError> {
    let key = RegKey::predef(HKEY_CURRENT_USER);
    let key = match key.open_subkey_with_flags(RUN_KEY, winreg::enums::KEY_SET_VALUE) {
        Ok(key) => key,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(StartupRegistrationError::OpenRegistryKey(error.to_string()));
        }
    };
    match key.delete_value(VALUE_NAME) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StartupRegistrationError::DeleteRegistryValue(
            error.to_string(),
        )),
    }
}

fn startup_command() -> Result<OsString, StartupRegistrationError> {
    let exe = std::env::current_exe()
        .map_err(|error| StartupRegistrationError::ReadExecutablePath(error.to_string()))?;
    let exe = sanitize_startup_executable(exe)?;
    let mut command = OsString::from("\"");
    command.push(exe);
    command.push("\"");
    Ok(command)
}

fn sanitize_startup_executable(exe: PathBuf) -> Result<PathBuf, StartupRegistrationError> {
    let exe = exe
        .canonicalize()
        .map_err(|error| StartupRegistrationError::ResolveExecutablePath(error.to_string()))?;

    if !exe.is_file() {
        return Err(StartupRegistrationError::ExecutableIsNotFile);
    }
    if let Some(extension) = exe.extension().and_then(|extension| extension.to_str()) {
        if !STARTUP_ALLOWED_EXTENSIONS
            .iter()
            .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        {
            return Err(StartupRegistrationError::UnexpectedExecutableExtension(
                extension.to_owned(),
            ));
        }
    } else {
        return Err(StartupRegistrationError::ExecutableHasNoExtension);
    }

    Ok(exe)
}
