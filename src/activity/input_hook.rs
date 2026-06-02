use std::{
    ptr::null,
    sync::{
        atomic::{AtomicU8, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW,
        SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, HHOOK, MSG,
        PM_NOREMOVE, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_QUIT,
    },
};

const KEYBOARD_EVENT: u8 = 0b01;
const MOUSE_EVENT: u8 = 0b10;

static INPUT_EVENTS: AtomicU8 = AtomicU8::new(0);

#[derive(Debug, Clone, Copy, Default)]
pub struct InputHookEvents {
    pub keyboard: bool,
    pub mouse: bool,
}

pub struct InputHook {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl InputHook {
    pub fn install() -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        let thread = thread::spawn(move || hook_thread(sender));

        match receiver.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(thread_id)) => Ok(Self {
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

    pub fn take_events(&self) -> InputHookEvents {
        take_pending_events()
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

fn hook_thread(sender: mpsc::Sender<Result<u32, String>>) {
    let thread_id = unsafe { GetCurrentThreadId() };
    let mut msg = MSG::default();

    unsafe {
        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
    }

    let module = unsafe { GetModuleHandleW(null()) };
    let keyboard_hook =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) };
    if keyboard_hook.is_null() {
        let _ = sender.send(Err("Failed to install keyboard input hook.".to_owned()));
        return;
    }

    let mouse_hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0) };
    if mouse_hook.is_null() {
        unsafe {
            UnhookWindowsHookEx(keyboard_hook);
        }
        let _ = sender.send(Err("Failed to install mouse input hook.".to_owned()));
        return;
    }

    let _ = sender.send(Ok(thread_id));

    unsafe {
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        UnhookWindowsHookEx(keyboard_hook);
        UnhookWindowsHookEx(mouse_hook);
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        record_input_event(KEYBOARD_EVENT);
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
        record_input_event(MOUSE_EVENT);
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
    INPUT_EVENTS.fetch_or(event, Ordering::AcqRel);
}

pub fn take_pending_events() -> InputHookEvents {
    let events = INPUT_EVENTS.swap(0, Ordering::AcqRel);
    InputHookEvents {
        keyboard: events & KEYBOARD_EVENT != 0,
        mouse: events & MOUSE_EVENT != 0,
    }
}
