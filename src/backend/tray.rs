use std::{
    mem::{size_of, transmute},
    panic::{catch_unwind, AssertUnwindSafe},
    ptr::null,
    sync::{
        atomic::{AtomicBool, AtomicIsize, Ordering},
        Arc, Mutex,
    },
};

use rust_i18n::t;
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Shell::{
            Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CallWindowProcW, CreatePopupMenu, DestroyMenu, GetCursorPos,
            GetForegroundWindow, IsIconic, LoadImageW, SetForegroundWindow, SetWindowLongPtrW,
            ShowWindow, TrackPopupMenu, GWLP_WNDPROC, HICON, IMAGE_ICON, LR_DEFAULTSIZE, LR_SHARED,
            MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, SW_HIDE, SW_RESTORE, SW_SHOW,
            TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_LBUTTONDBLCLK, WM_LBUTTONUP,
            WM_RBUTTONUP, WM_SHOWWINDOW, WNDPROC,
        },
    },
};

use crate::win_util::wide_null;

const TRAY_UID: u32 = 1;
const WM_TRAYICON: u32 = WM_APP + 1;
const MENU_SHOW: usize = 1001;
const MENU_QUIT: usize = 1002;
const MENU_MASTER: usize = 1003;
const MENU_FEATURE_BASE: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    MasterSwitch(bool),
    Feature {
        id: usize,
        profile: crate::config::PowerSourceProfile,
        enabled: bool,
    },
}

#[derive(Clone)]
pub struct FeatureToggle {
    pub id: usize,
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, Default)]
pub struct MenuState {
    pub enabled: bool,
    pub profile: crate::config::PowerSourceProfile,
    pub groups: Vec<(String, Vec<FeatureToggle>)>,
}

static MENU_STATE: Mutex<Option<MenuState>> = Mutex::new(None);
static MENU_ACTIONS: Mutex<Vec<MenuAction>> = Mutex::new(Vec::new());

pub fn set_menu_state(state: MenuState) {
    *MENU_STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(state);
}

pub fn take_menu_actions() -> Vec<MenuAction> {
    std::mem::take(
        &mut *MENU_ACTIONS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

fn menu_action(command: usize, state: &MenuState) -> Option<MenuAction> {
    if command == MENU_MASTER {
        return Some(MenuAction::MasterSwitch(!state.enabled));
    }
    let id = command.checked_sub(MENU_FEATURE_BASE)?;
    state
        .groups
        .iter()
        .flat_map(|(_, items)| items)
        .find(|item| item.id == id)
        .map(|item| MenuAction::Feature {
            id,
            profile: state.profile,
            enabled: !item.enabled,
        })
}

static ORIGINAL_WNDPROC: AtomicIsize = AtomicIsize::new(0);
static HIDE_ON_CLOSE: AtomicBool = AtomicBool::new(false);
static HIDDEN_TO_TRAY: AtomicBool = AtomicBool::new(false);
static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static RESTORE_REQUESTED: AtomicBool = AtomicBool::new(false);
static VISIBILITY_CALLBACK: Mutex<Option<VisibilityCallback>> = Mutex::new(None);

type VisibilityCallback = Arc<dyn Fn(bool) + Send + Sync>;

pub struct TrayVisibilityWatcher {
    callback: VisibilityCallback,
}

impl TrayVisibilityWatcher {
    pub fn start(callback: VisibilityCallback) -> Self {
        let mut slot = VISIBILITY_CALLBACK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = Some(Arc::clone(&callback));
        Self { callback }
    }
}

impl Drop for TrayVisibilityWatcher {
    fn drop(&mut self) {
        let mut slot = VISIBILITY_CALLBACK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot
            .as_ref()
            .is_some_and(|callback| Arc::ptr_eq(callback, &self.callback))
        {
            *slot = None;
        }
    }
}

pub struct TrayIcon {
    hwnd: HWND,
    original_wndproc: isize,
}

impl TrayIcon {
    pub fn install(hwnd: HWND) -> Result<Self, String> {
        if hwnd.is_null() {
            return Err("Cannot create tray icon without a window handle.".to_owned());
        }

        let original_wndproc = subclass_window(hwnd)?;

        let mut data = notify_data(hwnd);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_TRAYICON;
        data.hIcon = load_app_icon();
        write_wide_fixed(&mut data.szTip, "Winderust");

        // SAFETY: data has the required size, references the live app window, and contains a
        // shared or null icon handle valid for the call.
        let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
        if ok == 0 {
            restore_window_proc(hwnd, original_wndproc)?;
            return Err("Failed to add Winderust to the system tray.".to_owned());
        }

        Ok(Self {
            hwnd,
            original_wndproc,
        })
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        let data = notify_data(self.hwnd);
        // SAFETY: data identifies the tray icon installed for this live window and deletion does
        // not transfer ownership.
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &data);
        }
        if let Err(error) = restore_window_proc(self.hwnd, self.original_wndproc) {
            eprintln!("{error}");
        }
    }
}

pub fn take_quit_requested() -> bool {
    take_requested(&QUIT_REQUESTED)
}

pub fn take_restore_requested() -> bool {
    take_requested(&RESTORE_REQUESTED)
}

fn take_requested(requested: &AtomicBool) -> bool {
    requested.swap(false, Ordering::Relaxed)
}

pub fn set_hide_on_close(enabled: bool) {
    HIDE_ON_CLOSE.store(enabled, Ordering::Relaxed);
}

pub fn is_hidden_to_tray() -> bool {
    HIDDEN_TO_TRAY.load(Ordering::Relaxed)
}

pub fn hide_window(hwnd: HWND) {
    set_hidden_to_tray(true);
    // SAFETY: hwnd is obtained from the live Iced window; ShowWindow does not retain pointers.
    unsafe { ShowWindow(hwnd, SW_HIDE) };
}

fn notify_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_UID,
        ..Default::default()
    }
}

fn write_wide_fixed<const N: usize>(target: &mut [u16; N], value: &str) {
    for (slot, code) in target
        .iter_mut()
        .zip(value.encode_utf16().chain(std::iter::once(0)))
    {
        *slot = code;
    }
}

fn load_app_icon() -> HICON {
    // SAFETY: The current module handle and integer resource id identify the embedded icon;
    // LR_SHARED keeps ownership with Windows.
    unsafe {
        LoadImageW(
            GetModuleHandleW(std::ptr::null()),
            1usize as windows_sys::core::PCWSTR,
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE | LR_SHARED,
        ) as HICON
    }
}

fn subclass_window(hwnd: HWND) -> Result<isize, String> {
    if ORIGINAL_WNDPROC.load(Ordering::SeqCst) != 0 {
        return Err("The tray window is already subclassed.".to_owned());
    }

    // SAFETY: clearing the calling thread's last-error value lets the zero return from the
    // following SetWindowLongPtrW call be distinguished from failure.
    unsafe { SetLastError(0) };
    // SAFETY: hwnd is the live Iced window and tray_wnd_proc has the required static callback ABI.
    let previous =
        unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, tray_wnd_proc as *const () as isize) };
    if previous == 0 {
        // SAFETY: GetLastError is captured immediately after the failed SetWindowLongPtrW call.
        let error = unsafe { GetLastError() };
        return Err(format!(
            "Failed to subclass the tray window with error code {error}."
        ));
    }

    ORIGINAL_WNDPROC.store(previous, Ordering::SeqCst);
    Ok(previous)
}

fn restore_window_proc(hwnd: HWND, original_wndproc: isize) -> Result<(), String> {
    // SAFETY: hwnd is the same live window subclassed by TrayIcon, and original_wndproc is the
    // window procedure returned by that successful SetWindowLongPtrW call.
    unsafe { SetLastError(0) };
    // SAFETY: the original callback has the WNDPROC ABI and remains owned by the live window.
    let previous = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, original_wndproc) };
    if previous == 0 {
        // SAFETY: GetLastError is captured immediately after SetWindowLongPtrW returned zero.
        let error = unsafe { GetLastError() };
        if error != 0 {
            return Err(format!(
                "Failed to restore the tray window procedure with error code {error}."
            ));
        }
    }

    ORIGINAL_WNDPROC.store(0, Ordering::SeqCst);
    Ok(())
}

fn close_needs_prompt(minimized: bool, foreground: bool) -> bool {
    minimized || !foreground
}

unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_CLOSE {
        // SAFETY: hwnd belongs to this active window procedure callback.
        if unsafe { close_needs_prompt(IsIconic(hwnd) != 0, GetForegroundWindow() == hwnd) } {
            QUIT_REQUESTED.store(true, Ordering::Relaxed);
            return 0;
        }
    }
    if message == WM_CLOSE && HIDE_ON_CLOSE.load(Ordering::Relaxed) {
        set_hidden_to_tray(true);
        // SAFETY: hwnd is the window associated with this active window procedure callback.
        unsafe { ShowWindow(hwnd, SW_HIDE) };
        return 0;
    }

    if message == WM_SHOWWINDOW && wparam != 0 && HIDDEN_TO_TRAY.load(Ordering::Relaxed) {
        // SAFETY: hwnd is the window associated with this active window procedure callback.
        unsafe { ShowWindow(hwnd, SW_HIDE) };
        return 0;
    }

    if message == WM_TRAYICON && wparam as u32 == TRAY_UID {
        match lparam as u32 {
            WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                show_window(hwnd);
                return 0;
            }
            WM_RBUTTONUP => {
                show_tray_menu(hwnd);
                return 0;
            }
            _ => {}
        }
    }

    let previous = ORIGINAL_WNDPROC.load(Ordering::SeqCst);
    if previous != 0 {
        // SAFETY: previous was returned by SetWindowLongPtrW for GWLP_WNDPROC and therefore has
        // the WNDPROC ABI.
        unsafe {
            let proc: WNDPROC = transmute(previous);
            CallWindowProcW(proc, hwnd, message, wparam, lparam)
        }
    } else {
        0
    }
}

pub(crate) fn show_window(hwnd: HWND) {
    set_hidden_to_tray(false);
    RESTORE_REQUESTED.store(true, Ordering::Relaxed);
    // SAFETY: hwnd is the live application window supplied by its window procedure callback or
    // captured when the single-instance restore listener starts.
    unsafe {
        ShowWindow(
            hwnd,
            if IsIconic(hwnd) != 0 {
                SW_RESTORE
            } else {
                SW_SHOW
            },
        );
        SetForegroundWindow(hwnd);
    }
}

fn append_menu(
    menu: windows_sys::Win32::UI::WindowsAndMessaging::HMENU,
    id: usize,
    label: &str,
    flags: u32,
) -> bool {
    let label = wide_null(label);
    // SAFETY: callers supply a live menu; the null-terminated label is valid for the call.
    unsafe { AppendMenuW(menu, flags, id, label.as_ptr()) != 0 }
}

fn show_tray_menu(hwnd: HWND) {
    // SAFETY: CreatePopupMenu has no pointer inputs and returns either a menu handle or null.
    let menu = unsafe { CreatePopupMenu() };
    if menu.is_null() {
        return;
    }

    let state = MENU_STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .unwrap_or_default();
    let built = append_menu(menu, MENU_SHOW, &t!("tray.show_winderust"), MF_STRING)
        && append_menu(menu, 0, "", MF_SEPARATOR)
        && append_menu(
            menu,
            MENU_MASTER,
            &t!("settings.master_switch"),
            if state.enabled { MF_CHECKED } else { MF_STRING },
        )
        && append_menu(menu, 0, "", MF_SEPARATOR)
        && append_menu(
            menu,
            0,
            &match state.profile {
                crate::config::PowerSourceProfile::PluggedIn => t!("power_source.plugged_in"),
                crate::config::PowerSourceProfile::OnBattery => t!("power_source.on_battery"),
            },
            MF_GRAYED,
        )
        && state.groups.iter().all(|(label, items)| {
            // SAFETY: CreatePopupMenu takes no pointers and returns an owned handle or null.
            let submenu = unsafe { CreatePopupMenu() };
            if submenu.is_null() {
                return false;
            }
            let built = items.iter().all(|item| {
                append_menu(
                    submenu,
                    MENU_FEATURE_BASE + item.id,
                    &item.label,
                    if item.enabled { MF_CHECKED } else { MF_STRING },
                )
            }) && append_menu(menu, submenu as usize, label, MF_POPUP);
            if !built {
                // SAFETY: this submenu was not attached; ownership remains with this call.
                unsafe { DestroyMenu(submenu) };
            }
            built
        })
        && append_menu(menu, 0, "", MF_SEPARATOR)
        && append_menu(menu, MENU_QUIT, &t!("tray.quit"), MF_STRING);
    if !built {
        // SAFETY: this call owns the menu and any successfully attached submenus.
        unsafe { DestroyMenu(menu) };
        return;
    }

    let mut point = POINT { x: 0, y: 0 };
    // SAFETY: point is writable, and hwnd is the live application window from the callback.
    unsafe {
        GetCursorPos(&mut point);
        SetForegroundWindow(hwnd);
    }

    // SAFETY: menu and hwnd are live for the duration of the call; a null exclusion rectangle is
    // permitted. The owned menu is destroyed exactly once after TrackPopupMenu returns.
    let command = unsafe {
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            null(),
        );
        DestroyMenu(menu);
        command
    };

    match command as usize {
        MENU_SHOW => show_window(hwnd),
        // The app restores its window and owns confirmation and shutdown.
        MENU_QUIT => QUIT_REQUESTED.store(true, Ordering::Relaxed),
        command => {
            if let Some(action) = menu_action(command, &state) {
                MENU_ACTIONS
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(action);
            }
        }
    }
}

fn set_hidden_to_tray(hidden: bool) {
    if HIDDEN_TO_TRAY.swap(hidden, Ordering::Relaxed) == hidden {
        return;
    }
    let callback = VISIBILITY_CALLBACK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(callback) = callback {
        if catch_unwind(AssertUnwindSafe(|| callback(hidden))).is_err() {
            eprintln!("Tray visibility callback panicked.");
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_and_minimized_closes_prompt_instead_of_hiding() {
        assert!(close_needs_prompt(false, false));
        assert!(close_needs_prompt(true, false));
        assert!(close_needs_prompt(true, true));
        assert!(!close_needs_prompt(false, true));
    }

    #[test]
    fn menu_toggles_target_the_displayed_profile_and_state() {
        for enabled in [false, true] {
            for profile in [
                crate::config::PowerSourceProfile::PluggedIn,
                crate::config::PowerSourceProfile::OnBattery,
            ] {
                let state = MenuState {
                    enabled,
                    profile,
                    groups: vec![(
                        "CPU".into(),
                        vec![FeatureToggle {
                            id: 7,
                            label: "Limiter".into(),
                            enabled,
                        }],
                    )],
                };
                assert_eq!(
                    menu_action(MENU_MASTER, &state),
                    Some(MenuAction::MasterSwitch(!enabled))
                );
                assert_eq!(
                    menu_action(MENU_FEATURE_BASE + 7, &state),
                    Some(MenuAction::Feature {
                        id: 7,
                        profile,
                        enabled: !enabled
                    })
                );
                assert_eq!(menu_action(0, &state), None);
                assert_eq!(menu_action(MENU_FEATURE_BASE + 8, &state), None);
            }
        }
    }

    #[test]
    fn tray_requests_are_consumed_once() {
        let requested = AtomicBool::new(true);

        assert!(take_requested(&requested));
        assert!(!take_requested(&requested));
    }
}
