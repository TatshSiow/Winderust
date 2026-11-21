use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use atomic_write_file::AtomicWriteFile;
use chrono::Local;

use super::Settings;

const CONFIG_FILE: &str = "settings.toml";

pub fn config_path() -> PathBuf {
    config_dir().join(CONFIG_FILE)
}

pub fn default_export_toml_path() -> PathBuf {
    config_dir().join(default_export_toml_filename())
}

fn default_export_toml_filename() -> String {
    format!(
        "winderust_{}_{}.toml",
        env!("CARGO_PKG_VERSION"),
        Local::now().format("%Y-%m-%d")
    )
}

fn config_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn load() -> Result<Settings, String> {
    load_from_path(&config_path())
}

pub(crate) fn load_from_path(path: &Path) -> Result<Settings, String> {
    match fs::read_to_string(path) {
        Ok(raw) => parse_toml_settings(path, &raw),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(format!("Failed to read {}: {error}", path.display())),
    }
}

pub fn save(settings: &Settings) -> Result<(), String> {
    let path = config_path();
    write_toml_settings(&path, settings)
        .map_err(|err| format!("Failed to save {}: {err}", path.display()))
}

pub fn export_toml_to(path: &Path, settings: &Settings) -> Result<(), String> {
    export_destination(path, &config_path())
        .and_then(|destination| write_toml_settings(&destination, settings))
        .map_err(|error| {
            rust_i18n::t!(
                "settings.export_failed",
                path = path.display(),
                error = error
            )
            .to_string()
        })
}

pub fn export_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_bytes_atomically(&export_destination(path, &config_path())?, bytes)
}

fn export_destination(path: &Path, active_settings: &Path) -> io::Result<PathBuf> {
    let path = std::path::absolute(path)?;
    let active_settings = std::path::absolute(active_settings)?;
    // The directory entry is reserved even before settings.toml exists.
    let same_entry = path.as_os_str().eq_ignore_ascii_case(&active_settings)
        || path
            .file_name()
            .zip(active_settings.file_name())
            .is_some_and(|(name, active_name)| name.eq_ignore_ascii_case(active_name))
            && path
                .parent()
                .zip(active_settings.parent())
                .is_some_and(|(parent, active_parent)| same_canonical_path(parent, active_parent));
    if same_entry || same_canonical_path(&path, &active_settings) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            rust_i18n::t!("common.export_active_settings").to_string(),
        ));
    }
    Ok(path)
}

fn same_canonical_path(left: &Path, right: &Path) -> bool {
    left.canonicalize()
        .is_ok_and(|left| right.canonicalize().ok().as_ref() == Some(&left))
}

pub fn import_toml_from(path: &Path) -> Result<Settings, String> {
    read_toml_settings(path)
}

fn read_toml_settings(path: &Path) -> Result<Settings, String> {
    let raw = fs::read_to_string(path)
        .map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
    parse_toml_settings(path, &raw)
}

fn parse_toml_settings(path: &Path, raw: &str) -> Result<Settings, String> {
    let settings: Settings =
        toml::from_str(raw).map_err(|err| format!("Failed to parse {}: {err}", path.display()))?;
    settings.validate()?;
    Ok(settings)
}

fn write_toml_settings(path: &Path, settings: &Settings) -> io::Result<()> {
    settings
        .validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let raw = toml::to_string_pretty(settings)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    write_bytes_atomically(path, raw.as_bytes())
}

fn write_bytes_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = AtomicWriteFile::open(path)?;
    file.write_all(bytes)?;
    file.commit()
}

#[cfg(test)]
mod tests;
