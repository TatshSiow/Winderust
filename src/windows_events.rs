use std::{
    cell::RefCell,
    ptr::null,
    sync::{mpsc, Arc},
    thread::{self, JoinHandle},
    time::Duration,
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::{
        LibraryLoader::GetModuleHandleW,
        Power::{
            RegisterPowerSettingNotification, RegisterSuspendResumeNotification,
            UnregisterPowerSettingNotification, HPOWERNOTIFY,
        },
        RemoteDesktop::{
            WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
            NOTIFY_FOR_THIS_SESSION,
        },
        SystemServices::{
            GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING, GUID_POWERSCHEME_PERSONALITY,
        },
        Threading::GetCurrentThreadId,
    },
    UI::{
        Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
            PeekMessageW, PostThreadMessageW, RegisterClassW, TranslateMessage,
            DEVICE_NOTIFY_WINDOW_HANDLE, EVENT_OBJECT_CREATE, EVENT_SYSTEM_FOREGROUND,
            HWND_MESSAGE, MSG, OBJID_WINDOW, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMECRITICAL,
            PBT_APMRESUMESTANDBY, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, PBT_POWERSETTINGCHANGE,
            PM_NOREMOVE, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_POWERBROADCAST,
            WM_QUIT, WM_WTSSESSION_CHANGE, WNDCLASSW,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsAutomationEvent {
    ForegroundChanged,
    WindowCreated,
    PowerChanged,
    SessionChanged,
}

type EventCallback = Arc<dyn Fn(WindowsAutomationEvent) + Send + Sync>;

thread_local! {
    static EVENT_CALLBACK: RefCell<Option<EventCallback>> = RefCell::new(None);
}

pub struct WindowsEventWatcher {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl WindowsEventWatcher {
    pub fn start(callback: EventCallback) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        let thread = thread::spawn(move || event_thread(sender, callback));

        match receiver.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(thread_id)) => Ok(Self {
                thread_id,
                thread: Some(thread),
            }),
            Ok(Err(err)) => {
                let _ = thread.join();
                Err(err)
            }
            Err(err) => Err(format!("Windows event watcher did not start: {err}")),
        }
    }
}

impl Drop for WindowsEventWatcher {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn event_thread(sender: mpsc::Sender<Result<u32, String>>, callback: EventCallback) {
    let thread_id = unsafe { GetCurrentThreadId() };
    let mut msg = MSG::default();

    EVENT_CALLBACK.with(|slot| {
        *slot.borrow_mut() = Some(callback);
    });

    unsafe {
        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
    }

    let window = create_event_window();
    let foreground_hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            std::ptr::null_mut(),
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    let window_hook = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_CREATE,
            EVENT_OBJECT_CREATE,
            std::ptr::null_mut(),
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };

    if foreground_hook.is_null() && window_hook.is_null() && window.is_null() {
        let _ = sender.send(Err(
            "Failed to start Windows foreground/window event hooks.".to_owned(),
        ));
        EVENT_CALLBACK.with(|slot| {
            *slot.borrow_mut() = None;
        });
        return;
    }

    let mut power_notifications = Vec::new();
    let mut session_notifications_registered = false;
    if !window.is_null() {
        power_notifications = register_power_notifications(window);
        session_notifications_registered =
            unsafe { WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) } != 0;
    }

    let _ = sender.send(Ok(thread_id));

    unsafe {
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if session_notifications_registered {
            WTSUnRegisterSessionNotification(window);
        }
        for notification in power_notifications {
            UnregisterPowerSettingNotification(notification);
        }
        if !foreground_hook.is_null() {
            UnhookWinEvent(foreground_hook);
        }
        if !window_hook.is_null() {
            UnhookWinEvent(window_hook);
        }
        if !window.is_null() {
            DestroyWindow(window);
        }
    }

    EVENT_CALLBACK.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

fn create_event_window() -> HWND {
    let class_name = wide_null("PowerLeafAutomationEvents");
    let module = unsafe { GetModuleHandleW(null()) };
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(event_window_proc),
        hInstance: module as _,
        lpszClassName: class_name.as_ptr(),
        ..unsafe { std::mem::zeroed() }
    };

    unsafe {
        RegisterClassW(&window_class);
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            module as _,
            std::ptr::null(),
        )
    }
}

fn register_power_notifications(window: HWND) -> Vec<HPOWERNOTIFY> {
    let power_settings = [
        GUID_ACDC_POWER_SOURCE,
        GUID_BATTERY_PERCENTAGE_REMAINING,
        GUID_POWERSCHEME_PERSONALITY,
    ];
    let mut notifications = Vec::new();

    for setting in &power_settings {
        let notification = unsafe {
            RegisterPowerSettingNotification(window as _, setting, DEVICE_NOTIFY_WINDOW_HANDLE)
        };
        if notification != 0 {
            notifications.push(notification);
        }
    }

    let suspend_resume =
        unsafe { RegisterSuspendResumeNotification(window as _, DEVICE_NOTIFY_WINDOW_HANDLE) };
    if suspend_resume != 0 {
        notifications.push(suspend_resume);
    }

    notifications
}

unsafe extern "system" fn event_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_POWERBROADCAST => {
            match wparam as u32 {
                PBT_POWERSETTINGCHANGE
                | PBT_APMRESUMEAUTOMATIC
                | PBT_APMRESUMECRITICAL
                | PBT_APMRESUMESTANDBY
                | PBT_APMRESUMESUSPEND
                | PBT_APMSUSPEND => notify_event(WindowsAutomationEvent::PowerChanged),
                _ => {}
            }
            1
        }
        WM_WTSSESSION_CHANGE => {
            notify_event(WindowsAutomationEvent::SessionChanged);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object_id: i32,
    child_id: i32,
    _event_thread: u32,
    _event_time: u32,
) {
    match event {
        EVENT_SYSTEM_FOREGROUND => notify_event(WindowsAutomationEvent::ForegroundChanged),
        EVENT_OBJECT_CREATE if !hwnd.is_null() && object_id == OBJID_WINDOW && child_id == 0 => {
            notify_event(WindowsAutomationEvent::WindowCreated);
        }
        _ => {}
    }
}

fn notify_event(event: WindowsAutomationEvent) {
    EVENT_CALLBACK.with(|slot| {
        if let Some(callback) = slot.borrow().as_ref() {
            callback(event);
        }
    });
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
