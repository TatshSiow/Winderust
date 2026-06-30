use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

use chrono::Local;
use windows_sys::Win32::{
    Foundation::HWND,
    UI::Controls::Dialogs::{
        CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_FILEMUSTEXIST,
        OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    },
};

use crate::{config, win_util::wide_null};

#[derive(Debug, Clone, Copy)]
pub(crate) enum FileDialogMode {
    Open,
    Save,
}

struct FileDialogOptions {
    mode: FileDialogMode,
    default_path: PathBuf,
    filter: &'static str,
    default_extension: &'static str,
    title: &'static str,
}

pub(crate) fn choose_settings_file(
    hwnd: Option<HWND>,
    mode: FileDialogMode,
) -> Result<Option<PathBuf>, String> {
    choose_file(
        hwnd,
        FileDialogOptions {
            mode,
            default_path: match mode {
                FileDialogMode::Open => config::storage::config_path(),
                FileDialogMode::Save => config::storage::default_export_toml_path(),
            },
            filter: "TOML settings (*.toml)\0*.toml\0All files (*.*)\0*.*\0",
            default_extension: "toml",
            title: match mode {
                FileDialogMode::Open => "Import settings",
                FileDialogMode::Save => "Export settings",
            },
        },
    )
}

pub(crate) fn choose_action_log_export_file(hwnd: Option<HWND>) -> Result<Option<PathBuf>, String> {
    choose_file(
        hwnd,
        FileDialogOptions {
            mode: FileDialogMode::Save,
            default_path: default_action_log_export_csv_path(),
            filter: "CSV files (*.csv)\0*.csv\0All files (*.*)\0*.*\0",
            default_extension: "csv",
            title: "Export log",
        },
    )
}

fn choose_file(hwnd: Option<HWND>, options: FileDialogOptions) -> Result<Option<PathBuf>, String> {
    const FILE_BUFFER_LEN: usize = 4096;

    let mut file_buffer = path_to_wide_buffer(&options.default_path, FILE_BUFFER_LEN);
    let filter = wide_null(options.filter);
    let default_extension = wide_null(options.default_extension);
    let title = wide_null(options.title);
    let mut flags = OFN_HIDEREADONLY | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST;
    flags |= match options.mode {
        FileDialogMode::Open => OFN_FILEMUSTEXIST,
        FileDialogMode::Save => OFN_OVERWRITEPROMPT,
    };

    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd.unwrap_or_default(),
        lpstrFilter: filter.as_ptr(),
        lpstrFile: file_buffer.as_mut_ptr(),
        nMaxFile: file_buffer.len() as u32,
        lpstrTitle: title.as_ptr(),
        lpstrDefExt: default_extension.as_ptr(),
        Flags: flags,
        ..Default::default()
    };

    let selected = unsafe {
        match options.mode {
            FileDialogMode::Open => GetOpenFileNameW(&mut dialog),
            FileDialogMode::Save => GetSaveFileNameW(&mut dialog),
        }
    };
    if selected != 0 {
        return Ok(Some(path_from_wide_buffer(&file_buffer)));
    }

    let error = unsafe { CommDlgExtendedError() };
    if error == 0 {
        Ok(None)
    } else {
        Err(format!("File dialog failed with error code {error}"))
    }
}

fn default_action_log_export_csv_path() -> PathBuf {
    config::storage::config_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(format!(
            "powerleaf_action_log_{}_{}.csv",
            env!("CARGO_PKG_VERSION"),
            Local::now().format("%Y-%m-%d")
        ))
}

fn path_to_wide_buffer(path: &Path, len: usize) -> Vec<u16> {
    let mut buffer: Vec<u16> = path.as_os_str().encode_wide().take(len - 1).collect();
    buffer.resize(len, 0);
    buffer
}

fn path_from_wide_buffer(buffer: &[u16]) -> PathBuf {
    let len = buffer
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(buffer.len());
    PathBuf::from(OsString::from_wide(&buffer[..len]))
}
