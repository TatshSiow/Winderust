use std::{
    mem::size_of,
    panic::{catch_unwind, AssertUnwindSafe},
    ptr::null,
    sync::{
        atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

use rust_i18n::t;
use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Shell::{
            Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_ERROR, NIIF_WARNING,
            NIM_ADD, NIM_DELETE, NIM_MODIFY, NIN_BALLOONUSERCLICK, NOTIFYICONDATAW,
        },
        WindowsAndMessaging::{
            AppendMenuW, ChangeWindowMessageFilterEx, CreatePopupMenu, CreateWindowExW,
            DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, EndMenu, GetCursorPos,
            GetForegroundWindow, GetMessageW, IsIconic, LoadImageW, PostMessageW, PostQuitMessage,
            RegisterClassW, RegisterWindowMessageW, SendMessageW, SetForegroundWindow, ShowWindow,
            TrackPopupMenu, TranslateMessage, HICON, ICON_BIG, ICON_SMALL, IMAGE_ICON,
            LR_DEFAULTSIZE, LR_SHARED, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING,
            MSG, MSGFLT_ALLOW, PBT_APMPOWERSTATUSCHANGE, SW_RESTORE, SW_SHOW, TPM_RETURNCMD,
            TPM_RIGHTBUTTON, WM_APP, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_POWERBROADCAST,
            WM_RBUTTONUP, WM_SETICON, WNDCLASSW,
        },
    },
};

use crate::win_util::wide_null;

const TRAY_UID: u32 = 1;
const WM_TRAYICON: u32 = WM_APP + 1;
const WM_STOP_TRAY: u32 = WM_APP + 2;
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

fn wake_ui() {
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

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static NOTIFICATION_WINDOW: AtomicIsize = AtomicIsize::new(0);

pub(crate) fn show_notification(title: &str, body: &str, critical: bool) -> Result<(), String> {
    let hwnd = NOTIFICATION_WINDOW.load(Ordering::Acquire);
    if hwnd == 0 {
        return Err("Windows notification tray icon is unavailable.".into());
    }
    let mut data = notify_data(hwnd as HWND);
    data.uFlags = NIF_INFO;
    data.dwInfoFlags = if critical { NIIF_ERROR } else { NIIF_WARNING };
    write_wide_fixed(&mut data.szInfoTitle, title);
    write_wide_fixed(&mut data.szInfo, body);
    // SAFETY: data is initialized and identifies the registered tray icon. Shell validates
    // the window if teardown raced with this call; no borrowed pointers escape.
    if unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) } == 0 {
        Err("Windows rejected the Watchdog notification.".into())
    } else {
        Ok(())
    }
}
static TASKBAR_RECREATED: AtomicBool = AtomicBool::new(false);

pub fn take_taskbar_created() -> bool {
    take_requested(&TASKBAR_RECREATED)
}

static HIDDEN_TO_TRAY: AtomicBool = AtomicBool::new(false);
static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
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
    thread: Option<JoinHandle<()>>,
    registered: bool,
}

impl TrayIcon {
    pub fn install() -> Result<Self, String> {
        let name = wide_null("TaskbarCreated");
        // SAFETY: name is null-terminated and valid throughout registration.
        let message = unsafe { RegisterWindowMessageW(name.as_ptr()) };
        if message == 0 {
            return Err("Failed to register the taskbar recreation message.".into());
        }
        TASKBAR_CREATED.store(message, Ordering::Relaxed);
        let (hwnd, thread) = start_tray_window()?;
        let mut icon = Self {
            hwnd,
            thread: Some(thread),
            registered: false,
        };
        icon.register()?;
        Ok(icon)
    }

    pub fn is_registered(&self) -> bool {
        self.registered
            && self
                .thread
                .as_ref()
                .is_some_and(|thread| !thread.is_finished())
    }

    pub fn invalidate(&mut self) {
        NOTIFICATION_WINDOW.store(0, Ordering::Release);
        self.registered = false;
    }

    pub fn register(&mut self) -> Result<(), String> {
        NOTIFICATION_WINDOW.store(0, Ordering::Release);
        self.registered = false;
        if self.thread.as_ref().is_none_or(JoinHandle::is_finished) {
            if let Some(thread) = self.thread.take() {
                if thread.join().is_err() {
                    eprintln!("Tray window thread panicked before registration retry.");
                }
            }
            let (hwnd, thread) = start_tray_window()?;
            self.hwnd = hwnd;
            self.thread = Some(thread);
        }
        let message = TASKBAR_CREATED.load(Ordering::Relaxed);
        // SAFETY: hwnd is the live tray host; only the payload-free shell notification is allowed.
        if unsafe {
            ChangeWindowMessageFilterEx(self.hwnd, message, MSGFLT_ALLOW, std::ptr::null_mut())
        } == 0
        {
            return Err("Failed to allow taskbar recreation notifications.".into());
        }
        let mut data = notify_data(self.hwnd);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_TRAYICON;
        data.hIcon = load_app_icon();
        write_wide_fixed(&mut data.szTip, "Winderust");
        // SAFETY: data identifies the live window and uses a shared icon handle.
        self.registered = unsafe { Shell_NotifyIconW(NIM_ADD, &data) } != 0;
        if self.registered {
            NOTIFICATION_WINDOW.store(self.hwnd as isize, Ordering::Release);
            Ok(())
        } else {
            Err("Failed to add Winderust to the system tray.".into())
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        NOTIFICATION_WINDOW.store(0, Ordering::Release);
        let data = notify_data(self.hwnd);
        // SAFETY: data identifies the tray icon installed for this live window and deletion does
        // not transfer ownership.
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &data);
        }
        // SAFETY: hwnd belongs to our live tray thread, and the stop message carries no pointers.
        let stopped = unsafe { PostMessageW(self.hwnd, WM_STOP_TRAY, 0, 0) } != 0;
        if let Some(thread) = self.thread.take() {
            if stopped || thread.is_finished() {
                if thread.join().is_err() {
                    eprintln!("Tray window thread panicked during shutdown.");
                }
            } else {
                eprintln!("Failed to stop the tray window thread.");
            }
        }
    }
}

pub fn take_exit_requested() -> bool {
    take_requested(&EXIT_REQUESTED)
}

pub fn take_restore_requested() -> bool {
    take_requested(&RESTORE_REQUESTED)
}

fn take_requested(requested: &AtomicBool) -> bool {
    requested.swap(false, Ordering::Relaxed)
}

pub fn is_hidden_to_tray() -> bool {
    HIDDEN_TO_TRAY.load(Ordering::Relaxed)
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
    target.fill(0);
    let mut used = 0;
    for character in value.chars() {
        let mut units = [0; 2];
        let encoded = character.encode_utf16(&mut units);
        if used + encoded.len() >= N {
            break;
        }
        target[used..used + encoded.len()].copy_from_slice(encoded);
        used += encoded.len();
    }
}

pub(crate) fn set_window_icon(hwnd: HWND) -> Result<(), String> {
    if hwnd.is_null() {
        return Err("Cannot set Winderust icon without a window handle.".into());
    }
    let icon = load_app_icon();
    if icon.is_null() {
        return Err(format!(
            "Failed to load Winderust window icon with error code {}.",
            // SAFETY: GetLastError has no preconditions and follows the failed icon load.
            unsafe { GetLastError() }
        ));
    }
    // SAFETY: hwnd is the live application window; icon is a shared embedded resource that
    // remains valid for the process lifetime and must not be destroyed by the caller.
    unsafe {
        SendMessageW(hwnd, WM_SETICON, ICON_SMALL as WPARAM, icon as LPARAM);
        SendMessageW(hwnd, WM_SETICON, ICON_BIG as WPARAM, icon as LPARAM);
    }
    Ok(())
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

fn start_tray_window() -> Result<(HWND, JoinHandle<()>), String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let thread = thread::Builder::new()
        .name("tray-window".into())
        .spawn(move || tray_window_thread(sender))
        .map_err(|error| format!("Failed to start the tray window thread: {error}"))?;
    match receiver
        .recv()
        .map_err(|error| format!("Tray window did not start: {error}"))
        .and_then(std::convert::identity)
    {
        Ok(hwnd) => Ok((hwnd as HWND, thread)),
        Err(error) => {
            if thread.join().is_err() {
                eprintln!("Tray window thread panicked during startup.");
            }
            Err(error)
        }
    }
}

fn tray_window_thread(sender: mpsc::SyncSender<Result<usize, String>>) {
    let hwnd = match create_tray_window() {
        Ok(hwnd) => hwnd,
        Err(error) => {
            if sender.send(Err(error)).is_err() {
                eprintln!("Tray window startup receiver disconnected.");
            }
            return;
        }
    };
    if sender.send(Ok(hwnd as usize)).is_ok() {
        let mut message = MSG::default();
        loop {
            // SAFETY: message is writable; this thread owns the tray host and message queue.
            let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
            if result <= 0 {
                if result < 0 {
                    eprintln!("Tray window message loop failed.");
                    request_restore();
                }
                break;
            }
            // SAFETY: message was initialized by GetMessageW and is dispatched on its owner thread.
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    // SAFETY: this thread owns the hidden tray window and will not use it after destruction.
    unsafe { DestroyWindow(hwnd) };
}

fn create_tray_window() -> Result<HWND, String> {
    let class_name = wide_null("WinderustTray");
    // SAFETY: a null name requests the current process module without transferring ownership.
    let module = unsafe { GetModuleHandleW(null()) };
    let class = WNDCLASSW {
        lpfnWndProc: Some(tray_wnd_proc),
        hInstance: module,
        lpszClassName: class_name.as_ptr(),
        ..Default::default()
    };
    // SAFETY: class owns valid pointers for registration and the callback has a static lifetime.
    if unsafe { RegisterClassW(&class) } == 0 {
        // SAFETY: GetLastError immediately follows failed class registration.
        let error = unsafe { GetLastError() };
        if error != ERROR_CLASS_ALREADY_EXISTS {
            return Err(format!(
                "Failed to register the tray window class: {error}."
            ));
        }
    }
    // A hidden top-level window receives TaskbarCreated broadcasts; message-only windows do not.
    // SAFETY: names are terminated, the registered callback is static, and optional inputs are null.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            module,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        // SAFETY: GetLastError immediately follows failed window creation.
        let error = unsafe { GetLastError() };
        Err(format!("Failed to create the tray window: {error}."))
    } else {
        Ok(hwnd)
    }
}

unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_STOP_TRAY {
        // SAFETY: this callback runs on the tray owner thread; both calls retain no pointers.
        unsafe {
            EndMenu();
            PostQuitMessage(0);
        }
        return 0;
    }
    if message != 0 && message == TASKBAR_CREATED.load(Ordering::Relaxed) {
        TASKBAR_RECREATED.store(true, Ordering::Relaxed);
        wake_ui();
    }
    if message == WM_POWERBROADCAST && wparam == PBT_APMPOWERSTATUSCHANGE as usize {
        wake_ui();
    }
    if message == WM_TRAYICON && wparam as u32 == TRAY_UID {
        match lparam as u32 {
            WM_LBUTTONUP | WM_LBUTTONDBLCLK | NIN_BALLOONUSERCLICK => {
                request_restore();
                return 0;
            }
            WM_RBUTTONUP => {
                show_tray_menu(hwnd);
                return 0;
            }
            _ => {}
        }
    }

    // SAFETY: unhandled messages are forwarded with the unchanged window callback arguments.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

pub(crate) fn request_restore() {
    set_hidden_to_tray(false);
    RESTORE_REQUESTED.store(true, Ordering::Relaxed);
    wake_ui();
}

pub(crate) fn show_window(hwnd: HWND) {
    set_hidden_to_tray(false);
    // SAFETY: hwnd is the current live application window supplied by the UI.
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

pub(crate) fn close_needs_prompt(hwnd: HWND) -> bool {
    // SAFETY: hwnd is the current live GUI window; these calls only query its state.
    let (minimized, foreground) = unsafe { (IsIconic(hwnd) != 0, GetForegroundWindow() == hwnd) };
    close_needs_prompt_state(minimized, foreground)
}

fn close_needs_prompt_state(minimized: bool, foreground: bool) -> bool {
    minimized || !foreground
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
        MENU_SHOW => request_restore(),
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

pub(crate) fn set_hidden_to_tray(hidden: bool) {
    if HIDDEN_TO_TRAY.swap(hidden, Ordering::Relaxed) == hidden {
        return;
    }
    wake_ui();
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
    fn window_caption_and_switcher_use_the_embedded_app_icon() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WM_GETICON, WS_OVERLAPPEDWINDOW,
        };
        let class = wide_null("STATIC");
        // SAFETY: STATIC is a built-in window class; this test creates a hidden window on
        // its own thread, with a valid terminated class name and no retained input pointers.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                null(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                100,
                100,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                GetModuleHandleW(null()),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null());
        let result = set_window_icon(hwnd);
        // SAFETY: hwnd is the live hidden test window; these messages return its assigned
        // shared icon handles without transferring ownership.
        let (small, big) = unsafe {
            (
                SendMessageW(hwnd, WM_GETICON, ICON_SMALL as WPARAM, 0),
                SendMessageW(hwnd, WM_GETICON, ICON_BIG as WPARAM, 0),
            )
        };
        // SAFETY: this thread owns hwnd and no further operation will use the window.
        unsafe {
            DestroyWindow(hwnd);
        }
        result.unwrap();
        assert_ne!(small, 0);
        assert_eq!(small, load_app_icon() as LPARAM);
        assert_eq!(big, small);
    }

    #[test]
    fn tray_host_lives_independently_of_the_gui_window() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetParent, IsWindow, IsWindowVisible};

        let (host, thread) = start_tray_window().expect("start independent tray host");
        let class = wide_null("STATIC");
        // SAFETY: STATIC is a built-in class; this test owns the hidden GUI stand-in window.
        let gui = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                null(),
                0,
                0,
                0,
                100,
                100,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                GetModuleHandleW(null()),
                std::ptr::null(),
            )
        };
        // SAFETY: the GUI is owned by this thread; the tray host remains owned by its own thread.
        let (gui_destroyed, host_exists, host_hidden, host_top_level) = unsafe {
            (
                DestroyWindow(gui) != 0,
                IsWindow(host) != 0,
                IsWindowVisible(host) == 0,
                GetParent(host).is_null(),
            )
        };
        // SAFETY: the host is live; this pointer-free message asks its owner thread to stop.
        let posted = unsafe { PostMessageW(host, WM_STOP_TRAY, 0, 0) } != 0;
        let joined = thread.join();
        // SAFETY: IsWindow accepts an old handle and only queries whether it remains valid.
        let host_destroyed = unsafe { IsWindow(host) == 0 };
        assert!(!gui.is_null());
        assert!(gui_destroyed && host_exists && host_hidden && host_top_level);
        assert!(posted && joined.is_ok() && host_destroyed);
    }

    #[test]
    fn background_and_minimized_closes_prompt_instead_of_hiding() {
        assert!(close_needs_prompt_state(false, false));
        assert!(close_needs_prompt_state(true, false));
        assert!(close_needs_prompt_state(true, true));
        assert!(!close_needs_prompt_state(false, true));
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

    #[test]
    fn notification_text_is_terminated_without_splitting_surrogates() {
        let mut text = [1; 5];
        write_wide_fixed(&mut text, "ab😀c");
        assert_eq!(String::from_utf16(&text[..4]).unwrap(), "ab😀");
        assert_eq!(text[4], 0);
        write_wide_fixed(&mut text, "a");
        assert_eq!(text, [97, 0, 0, 0, 0]);
        let mut tiny = [1; 2];
        write_wide_fixed(&mut tiny, "😀");
        assert_eq!(tiny, [0, 0]);
    }
}
