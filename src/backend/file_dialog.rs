use std::{
    num::NonZeroIsize,
    path::{Path, PathBuf},
};

use chrono::Local;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};
use rfd::AsyncFileDialog;
use rust_i18n::t;
use windows_sys::Win32::Foundation::HWND;

use crate::config;

#[derive(Debug, Clone, Copy)]
pub(crate) enum FileDialogMode {
    Open,
    Save,
}

pub(crate) fn choose_settings_file(
    hwnd: Option<HWND>,
    mode: FileDialogMode,
) -> impl std::future::Future<Output = Option<PathBuf>> + Send {
    let default_path = match mode {
        FileDialogMode::Open => config::storage::config_path(),
        FileDialogMode::Save => config::storage::default_export_toml_path(),
    };
    let dialog = async_dialog(hwnd)
        .add_filter(t!("settings.settings_files").to_string(), &["toml"])
        .set_directory(default_path.parent().unwrap_or_else(|| Path::new(".")))
        .set_file_name(
            default_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
        )
        .set_title(match mode {
            FileDialogMode::Open => t!("settings.import_settings").to_string(),
            FileDialogMode::Save => t!("settings.export_settings").to_string(),
        });
    async move {
        let file = match mode {
            FileDialogMode::Open => dialog.pick_file().await,
            FileDialogMode::Save => dialog.save_file().await,
        }?;
        Some(file.path().to_owned())
    }
}

pub(crate) fn choose_action_log_export_file(
    hwnd: Option<HWND>,
) -> impl std::future::Future<Output = Option<PathBuf>> + Send {
    let filename = format!(
        "winderust_action_log_{}_{}.csv",
        env!("CARGO_PKG_VERSION"),
        Local::now().format("%Y-%m-%d")
    );
    let dialog = async_dialog(hwnd)
        .add_filter(t!("action_log.csv_files").to_string(), &["csv"])
        .set_directory(
            config::storage::config_path()
                .parent()
                .unwrap_or_else(|| Path::new(".")),
        )
        .set_file_name(filename)
        .set_title(t!("action_log.export_csv").to_string());
    async move {
        let file = dialog.save_file().await?;
        Some(file.path().to_owned())
    }
}

pub(crate) fn choose_executable_file(
    hwnd: Option<HWND>,
) -> impl std::future::Future<Output = Option<PathBuf>> + Send {
    let dialog = async_dialog(hwnd)
        .add_filter(t!("common.executable_files").to_string(), &["exe"])
        .set_title(t!("common.select_executable").to_string());
    async move {
        let file = dialog.pick_file().await?;
        let path = file.path().to_owned();
        is_executable_file(&path).then_some(path)
    }
}

fn is_executable_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
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
        // SAFETY: self stores a non-null HWND borrowed from the live application window for no longer
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

fn async_dialog(hwnd: Option<HWND>) -> AsyncFileDialog {
    hwnd.and_then(DialogParent::new)
        .map_or_else(AsyncFileDialog::new, |parent| {
            AsyncFileDialog::new().set_parent(&parent)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_picker_accepts_only_exe_paths() {
        assert!(is_executable_file(Path::new(r"C:\Apps\Example.exe")));
        assert!(is_executable_file(Path::new(r"C:\Apps\Example.EXE")));
        assert!(!is_executable_file(Path::new(r"C:\Apps\Example.com")));
        assert!(!is_executable_file(Path::new(r"C:\Apps\Example")));
    }
}

/// Run on a worker thread: the modal loop must not block Iced's event loop.
pub(crate) fn choose_color(owner: isize, color: u32, saved: &[u32]) -> Result<Option<u32>, String> {
    use windows_sys::Win32::UI::Controls::Dialogs::{
        ChooseColorW, CommDlgExtendedError, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW,
    };
    let mut custom = [0x00ff_ffff; 16];
    for (slot, color) in custom.iter_mut().zip(saved) {
        *slot = swap_red_blue(*color);
    }
    let mut dialog = CHOOSECOLORW {
        lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: owner as HWND,
        rgbResult: swap_red_blue(color),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    // SAFETY: the owner is borrowed from the live application window;
    // dialog and the 16 writable custom colors remain valid throughout the modal call.
    if unsafe { ChooseColorW(&mut dialog) } != 0 {
        return Ok(Some(swap_red_blue(dialog.rgbResult)));
    }
    // SAFETY: queried immediately on the same thread after the common dialog returns.
    let error = unsafe { CommDlgExtendedError() };
    if error == 0 {
        Ok(None)
    } else {
        Err(format!("Color dialog failed (0x{error:08X})"))
    }
}

fn swap_red_blue(color: u32) -> u32 {
    ((color & 0xff) << 16) | (color & 0xff00) | ((color >> 16) & 0xff)
}

#[test]
fn native_color_channels_round_trip() {
    assert_eq!(swap_red_blue(0x123456), 0x563412);
    assert_eq!(swap_red_blue(swap_red_blue(0x123456)), 0x123456);
}
