use std::{
    num::NonZeroIsize,
    path::{Path, PathBuf},
};

use chrono::Local;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};
use rfd::FileDialog;
use windows_sys::Win32::Foundation::HWND;

use crate::config;

#[derive(Debug, Clone, Copy)]
pub(crate) enum FileDialogMode {
    Open,
    Save,
}

pub(crate) fn choose_settings_file(hwnd: Option<HWND>, mode: FileDialogMode) -> Option<PathBuf> {
    let default_path = match mode {
        FileDialogMode::Open => config::storage::config_path(),
        FileDialogMode::Save => config::storage::default_export_toml_path(),
    };
    let dialog = dialog(hwnd)
        .add_filter("TOML settings", &["toml"])
        .set_directory(default_path.parent().unwrap_or_else(|| Path::new(".")))
        .set_file_name(
            default_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
        )
        .set_title(match mode {
            FileDialogMode::Open => "Import settings",
            FileDialogMode::Save => "Export settings",
        });
    match mode {
        FileDialogMode::Open => dialog.pick_file(),
        FileDialogMode::Save => dialog.save_file(),
    }
}

pub(crate) fn choose_action_log_export_file(hwnd: Option<HWND>) -> Option<PathBuf> {
    let filename = format!(
        "winderust_action_log_{}_{}.csv",
        env!("CARGO_PKG_VERSION"),
        Local::now().format("%Y-%m-%d")
    );
    dialog(hwnd)
        .add_filter("CSV files", &["csv"])
        .set_directory(
            config::storage::config_path()
                .parent()
                .unwrap_or_else(|| Path::new(".")),
        )
        .set_file_name(filename)
        .set_title("Export log")
        .save_file()
}

fn dialog(hwnd: Option<HWND>) -> FileDialog {
    hwnd.and_then(DialogParent::new)
        .map_or_else(FileDialog::new, |parent| {
            FileDialog::new().set_parent(&parent)
        })
}

struct DialogParent(NonZeroIsize);

impl DialogParent {
    fn new(hwnd: HWND) -> Option<Self> {
        NonZeroIsize::new(hwnd as isize).map(Self)
    }
}

impl HasWindowHandle for DialogParent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = RawWindowHandle::Win32(Win32WindowHandle::new(self.0));
        // SAFETY: self stores a non-null HWND borrowed from the live GPUI window for no longer
        // than this DialogParent value.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for DialogParent {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let raw = RawDisplayHandle::Windows(WindowsDisplayHandle::new());
        // SAFETY: Windows has a process-global display handle with no owned resource to release.
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}
