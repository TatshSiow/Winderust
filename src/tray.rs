use std::{
    mem::{size_of, transmute},
    ptr::null,
    sync::atomic::{AtomicBool, AtomicIsize, Ordering},
};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    UI::{
        Shell::{
            Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CallWindowProcW, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW,
            PostMessageW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow, TrackPopupMenu,
            GWLP_WNDPROC, IDI_APPLICATION, MF_STRING, SW_HIDE, SW_RESTORE, SW_SHOWNA,
            TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_LBUTTONDBLCLK, WM_LBUTTONUP,
            WM_RBUTTONUP, WM_SHOWWINDOW, WNDPROC,
        },
    },
};

const TRAY_UID: u32 = 1;
const WM_TRAYICON: u32 = WM_APP + 1;
const MENU_SHOW: usize = 1001;
const MENU_QUIT: usize = 1002;

static ORIGINAL_WNDPROC: AtomicIsize = AtomicIsize::new(0);
static HIDE_ON_CLOSE: AtomicBool = AtomicBool::new(false);
static HIDDEN_TO_TRAY: AtomicBool = AtomicBool::new(false);
static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

pub struct TrayIcon {
    hwnd: HWND,
}

impl TrayIcon {
    pub fn install(hwnd: HWND) -> Result<Self, String> {
        if hwnd.is_null() {
            return Err("Cannot create tray icon without a window handle.".to_owned());
        }

        subclass_window(hwnd);

        let mut data = notify_data(hwnd);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_TRAYICON;
        data.hIcon = unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) };
        write_wide_fixed(&mut data.szTip, "PowerLeaf");

        let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
        if ok == 0 {
            return Err("Failed to add PowerLeaf to the system tray.".to_owned());
        }

        Ok(Self { hwnd })
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        let data = notify_data(self.hwnd);
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &data);
        }
    }
}

pub fn hwnd_from_window(window: &gpui::Window) -> Option<HWND> {
    let handle = HasWindowHandle::window_handle(window).ok()?.as_raw();
    match handle {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as HWND),
        _ => None,
    }
}

pub fn take_quit_requested() -> bool {
    QUIT_REQUESTED.swap(false, Ordering::SeqCst)
}

pub fn set_hide_on_close(enabled: bool) {
    HIDE_ON_CLOSE.store(enabled, Ordering::SeqCst);
}

pub fn is_hidden_to_tray() -> bool {
    HIDDEN_TO_TRAY.load(Ordering::SeqCst)
}

pub fn hide_window(hwnd: HWND) {
    unsafe {
        HIDDEN_TO_TRAY.store(true, Ordering::SeqCst);
        ShowWindow(hwnd, SW_HIDE);
    }
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

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn subclass_window(hwnd: HWND) {
    if ORIGINAL_WNDPROC.load(Ordering::SeqCst) != 0 {
        return;
    }

    let previous =
        unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, tray_wnd_proc as *const () as isize) };
    ORIGINAL_WNDPROC.store(previous, Ordering::SeqCst);
}

unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_CLOSE && HIDE_ON_CLOSE.load(Ordering::SeqCst) {
        HIDDEN_TO_TRAY.store(true, Ordering::SeqCst);
        ShowWindow(hwnd, SW_HIDE);
        return 0;
    }

    if message == WM_SHOWWINDOW && wparam != 0 && HIDDEN_TO_TRAY.load(Ordering::SeqCst) {
        ShowWindow(hwnd, SW_HIDE);
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
        let proc: WNDPROC = transmute(previous);
        CallWindowProcW(proc, hwnd, message, wparam, lparam)
    } else {
        0
    }
}

unsafe fn show_window(hwnd: HWND) {
    HIDDEN_TO_TRAY.store(false, Ordering::SeqCst);
    ShowWindow(hwnd, SW_RESTORE);
    SetForegroundWindow(hwnd);
}

unsafe fn show_tray_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }

    let show = wide_null("Show PowerLeaf");
    let quit = wide_null("Quit");
    AppendMenuW(menu, MF_STRING, MENU_SHOW, show.as_ptr());
    AppendMenuW(menu, MF_STRING, MENU_QUIT, quit.as_ptr());

    let mut point = POINT { x: 0, y: 0 };
    GetCursorPos(&mut point);
    SetForegroundWindow(hwnd);

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

    match command as usize {
        MENU_SHOW => show_window(hwnd),
        MENU_QUIT => {
            quit_window(hwnd);
        }
        _ => {}
    }
}

unsafe fn quit_window(hwnd: HWND) {
    HIDE_ON_CLOSE.store(false, Ordering::SeqCst);
    HIDDEN_TO_TRAY.store(false, Ordering::SeqCst);
    QUIT_REQUESTED.store(true, Ordering::SeqCst);

    ShowWindow(hwnd, SW_SHOWNA);
    PostMessageW(hwnd, WM_CLOSE, 0, 0);
}
