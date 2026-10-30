//! Native grouped menu adapter for the Wind UI-owned tray icon.
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    ptr::null,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use rust_i18n::t;
use windows_sys::Win32::{
    Foundation::{HWND, POINT},
    UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, SetForegroundWindow,
        TrackPopupMenu, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, TPM_RETURNCMD,
        TPM_RIGHTBUTTON,
    },
};

use crate::win_util::wide_null;

const MENU_SHOW: usize = 1001;
const MENU_EXIT: usize = 1002;
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

static UI_WAKE: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);

pub(crate) fn set_ui_wake(wake: Option<Arc<dyn Fn() + Send + Sync>>) {
    *UI_WAKE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = wake;
}

pub(crate) fn wake_ui() {
    let wake = UI_WAKE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(wake) = wake {
        if catch_unwind(AssertUnwindSafe(|| wake())).is_err() {
            eprintln!("Tray UI wake callback panicked.");
        }
    }
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

static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static RESTORE_REQUESTED: AtomicBool = AtomicBool::new(false);

pub fn take_exit_requested() -> bool {
    take_requested(&EXIT_REQUESTED)
}

pub fn take_restore_requested() -> bool {
    take_requested(&RESTORE_REQUESTED)
}

fn take_requested(requested: &AtomicBool) -> bool {
    requested.swap(false, Ordering::Relaxed)
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

/// Wind UI's tray menu has no submenus or dynamic visibility. Keep the existing native
/// menu for those capabilities, while its resident host retains icon ownership.
pub(crate) fn show_resident_menu(hwnd: HWND) {
    crate::platform::windows::menu_theme::apply(hwnd);
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
        && append_menu(menu, MENU_EXIT, &t!("tray.exit"), MF_STRING);
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
        // The resident host is an invisible message window. The application
        // recreates or restores its UI after this native callback returns.
        MENU_SHOW => RESTORE_REQUESTED.store(true, Ordering::Relaxed),
        // The app restores its window and owns confirmation and shutdown.
        MENU_EXIT => EXIT_REQUESTED.store(true, Ordering::Relaxed),
        command => {
            if let Some(action) = menu_action(command, &state) {
                MENU_ACTIONS
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(action);
            }
        }
    }
    if command != 0 {
        wake_ui();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
