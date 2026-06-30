use std::{
    cell::RefCell,
    ptr::null,
    sync::{
        atomic::{AtomicU8, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_0, VK_9, VK_CONTROL, VK_ESCAPE, VK_LEFT, VK_LWIN, VK_MENU,
            VK_NUMPAD0, VK_NUMPAD9, VK_RIGHT, VK_RWIN, VK_T, VK_TAB,
        },
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW,
            SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, HHOOK,
            KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, WH_KEYBOARD_LL, WH_MOUSE_LL,
            WM_KEYDOWN, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_QUIT, WM_RBUTTONDOWN, WM_SYSKEYDOWN,
        },
    },
};

const KEYBOARD_EVENT: u8 = 0b01;
const MOUSE_EVENT: u8 = 0b10;
const APP_SWITCH_EVENT: u8 = 0b100;
const MOUSE_CLICK_EVENT: u8 = 0b1000;
const LLKHF_INJECTED: u32 = 0x0000_0010;
const LLKHF_INJECTED_LOWER_IL: u32 = 0x0000_0002;
const LLMHF_INJECTED: u32 = 0x0000_0001;
const LLMHF_INJECTED_LOWER_IL: u32 = 0x0000_0002;

static INPUT_EVENTS: AtomicU8 = AtomicU8::new(0);

type EventCallback = Arc<dyn Fn(InputHookEvents) + Send + Sync>;

thread_local! {
    static INPUT_EVENT_CALLBACK: RefCell<Option<EventCallback>> = RefCell::new(None);
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputHookEvents {
    pub keyboard: bool,
    pub mouse: bool,
    pub app_switch: bool,
    pub mouse_click: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputHookConfig {
    pub keyboard: bool,
    pub mouse: bool,
}

pub struct InputHook {
    config: InputHookConfig,
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl InputHook {
    pub fn install(config: InputHookConfig, callback: EventCallback) -> Result<Self, String> {
        if !config.keyboard && !config.mouse {
            return Err("No input hooks are enabled.".to_owned());
        }

        let (sender, receiver) = mpsc::channel();
        let thread = thread::spawn(move || hook_thread(config, callback, sender));

        match receiver.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(thread_id)) => Ok(Self {
                config,
                thread_id,
                thread: Some(thread),
            }),
            Ok(Err(err)) => {
                let _ = thread.join();
                Err(err)
            }
            Err(err) => Err(format!("Input hooks did not start: {err}")),
        }
    }

    pub fn config(&self) -> InputHookConfig {
        self.config
    }
}

impl Drop for InputHook {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn hook_thread(
    config: InputHookConfig,
    callback: EventCallback,
    sender: mpsc::Sender<Result<u32, String>>,
) {
    let thread_id = unsafe { GetCurrentThreadId() };
    let mut msg = MSG::default();

    INPUT_EVENT_CALLBACK.with(|slot| {
        *slot.borrow_mut() = Some(callback);
    });

    unsafe {
        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
    }

    let module = unsafe { GetModuleHandleW(null()) };
    let mut keyboard_hook = std::ptr::null_mut();
    if config.keyboard {
        keyboard_hook =
            unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) };
        if keyboard_hook.is_null() {
            INPUT_EVENT_CALLBACK.with(|slot| {
                *slot.borrow_mut() = None;
            });
            let _ = sender.send(Err("Failed to install keyboard input hook.".to_owned()));
            return;
        }
    }

    let mut mouse_hook = std::ptr::null_mut();
    if config.mouse {
        mouse_hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0) };
        if mouse_hook.is_null() {
            unsafe {
                if !keyboard_hook.is_null() {
                    UnhookWindowsHookEx(keyboard_hook);
                }
            }
            INPUT_EVENT_CALLBACK.with(|slot| {
                *slot.borrow_mut() = None;
            });
            let _ = sender.send(Err("Failed to install mouse input hook.".to_owned()));
            return;
        }
    }

    let _ = sender.send(Ok(thread_id));

    unsafe {
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if !keyboard_hook.is_null() {
            UnhookWindowsHookEx(keyboard_hook);
        }
        if !mouse_hook.is_null() {
            UnhookWindowsHookEx(mouse_hook);
        }
    }

    INPUT_EVENT_CALLBACK.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb_event = &*(lparam as *const KBDLLHOOKSTRUCT);
        if kb_event.flags & (LLKHF_INJECTED | LLKHF_INJECTED_LOWER_IL) != 0 {
            return unsafe {
                CallNextHookEx(
                    std::ptr::null_mut::<std::ffi::c_void>() as HHOOK,
                    code,
                    wparam,
                    lparam,
                )
            };
        }

        let mut event = KEYBOARD_EVENT;
        if is_app_switch_key_event(wparam, kb_event) {
            event |= APP_SWITCH_EVENT;
        }
        record_input_event(event);
    }
    unsafe {
        CallNextHookEx(
            std::ptr::null_mut::<std::ffi::c_void>() as HHOOK,
            code,
            wparam,
            lparam,
        )
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let mouse_event = &*(lparam as *const MSLLHOOKSTRUCT);
        if mouse_event.flags & (LLMHF_INJECTED | LLMHF_INJECTED_LOWER_IL) != 0 {
            return unsafe {
                CallNextHookEx(
                    std::ptr::null_mut::<std::ffi::c_void>() as HHOOK,
                    code,
                    wparam,
                    lparam,
                )
            };
        }

        let mut event = MOUSE_EVENT;
        if is_mouse_button_down(wparam) {
            event |= MOUSE_CLICK_EVENT;
        }
        record_input_event(event);
    }
    unsafe {
        CallNextHookEx(
            std::ptr::null_mut::<std::ffi::c_void>() as HHOOK,
            code,
            wparam,
            lparam,
        )
    }
}

fn record_input_event(event: u8) {
    INPUT_EVENTS.fetch_or(event, Ordering::Relaxed);
    let events = input_events_from_bits(event);
    INPUT_EVENT_CALLBACK.with(|slot| {
        if let Some(callback) = slot.borrow().as_ref() {
            callback(events);
        }
    });
}

pub fn take_pending_events() -> InputHookEvents {
    let events = if INPUT_EVENTS.load(Ordering::Relaxed) == 0 {
        0
    } else {
        INPUT_EVENTS.swap(0, Ordering::Relaxed)
    };
    input_events_from_bits(events)
}

fn input_events_from_bits(events: u8) -> InputHookEvents {
    InputHookEvents {
        keyboard: events & KEYBOARD_EVENT != 0,
        mouse: events & MOUSE_EVENT != 0,
        app_switch: events & APP_SWITCH_EVENT != 0,
        mouse_click: events & MOUSE_CLICK_EVENT != 0,
    }
}

unsafe fn is_app_switch_key_event(wparam: WPARAM, event: &KBDLLHOOKSTRUCT) -> bool {
    if wparam != WM_KEYDOWN as WPARAM && wparam != WM_SYSKEYDOWN as WPARAM {
        return false;
    }

    let alt_down = virtual_key_pressed(VK_MENU);
    let win_down = virtual_key_pressed(VK_LWIN) || virtual_key_pressed(VK_RWIN);
    let ctrl_down = virtual_key_pressed(VK_CONTROL);
    is_app_switch_virtual_key(event.vkCode, alt_down, win_down, ctrl_down)
}

fn is_mouse_button_down(wparam: WPARAM) -> bool {
    matches!(
        wparam,
        value if value == WM_LBUTTONDOWN as WPARAM
            || value == WM_RBUTTONDOWN as WPARAM
            || value == WM_MBUTTONDOWN as WPARAM
    )
}

fn is_app_switch_virtual_key(
    vk_code: u32,
    alt_down: bool,
    win_down: bool,
    ctrl_down: bool,
) -> bool {
    (vk_code == u32::from(VK_TAB) && (alt_down || win_down))
        || (vk_code == u32::from(VK_ESCAPE) && alt_down)
        || (vk_code == u32::from(VK_T) && win_down)
        || (win_down && is_taskbar_number_key(vk_code))
        || (win_down
            && ctrl_down
            && (vk_code == u32::from(VK_LEFT) || vk_code == u32::from(VK_RIGHT)))
}

fn is_taskbar_number_key(vk_code: u32) -> bool {
    (u32::from(VK_0)..=u32::from(VK_9)).contains(&vk_code)
        || (u32::from(VK_NUMPAD0)..=u32::from(VK_NUMPAD9)).contains(&vk_code)
}

unsafe fn virtual_key_pressed(key: u16) -> bool {
    GetAsyncKeyState(i32::from(key)) < 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_1, VK_NUMPAD1};

    #[test]
    fn app_switch_virtual_key_matches_alt_or_win_tab() {
        assert!(is_app_switch_virtual_key(
            u32::from(VK_TAB),
            true,
            false,
            false
        ));
        assert!(is_app_switch_virtual_key(
            u32::from(VK_TAB),
            false,
            true,
            false
        ));
        assert!(!is_app_switch_virtual_key(
            u32::from(VK_TAB),
            false,
            false,
            false
        ));
    }

    #[test]
    fn app_switch_virtual_key_matches_alt_escape() {
        assert!(is_app_switch_virtual_key(
            u32::from(VK_ESCAPE),
            true,
            false,
            false
        ));
        assert!(!is_app_switch_virtual_key(
            u32::from(VK_ESCAPE),
            false,
            true,
            false
        ));
    }

    #[test]
    fn app_switch_virtual_key_matches_win_taskbar_navigation() {
        assert!(is_app_switch_virtual_key(
            u32::from(VK_T),
            false,
            true,
            false
        ));
        assert!(!is_app_switch_virtual_key(
            u32::from(VK_T),
            false,
            false,
            false
        ));
    }

    #[test]
    fn app_switch_virtual_key_matches_win_ctrl_virtual_desktop_switching() {
        assert!(is_app_switch_virtual_key(
            u32::from(VK_LEFT),
            false,
            true,
            true
        ));
        assert!(is_app_switch_virtual_key(
            u32::from(VK_RIGHT),
            false,
            true,
            true
        ));
        assert!(!is_app_switch_virtual_key(
            u32::from(VK_LEFT),
            false,
            true,
            false
        ));
    }

    #[test]
    fn app_switch_virtual_key_matches_win_number_shortcuts() {
        assert!(is_app_switch_virtual_key(
            u32::from(VK_1),
            false,
            true,
            false
        ));
        assert!(is_app_switch_virtual_key(
            u32::from(VK_0),
            false,
            true,
            false
        ));
        assert!(is_app_switch_virtual_key(
            u32::from(VK_NUMPAD1),
            false,
            true,
            false
        ));
        assert!(is_app_switch_virtual_key(
            u32::from(VK_NUMPAD0),
            false,
            true,
            false
        ));
    }

    #[test]
    fn app_switch_virtual_key_ignores_numbers_without_windows_key() {
        assert!(!is_app_switch_virtual_key(
            u32::from(VK_1),
            true,
            false,
            false
        ));
        assert!(!is_app_switch_virtual_key(
            u32::from(VK_NUMPAD1),
            false,
            false,
            false
        ));
    }
}
