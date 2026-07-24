use std::{
    cell::RefCell,
    panic::{catch_unwind, AssertUnwindSafe},
    ptr::null,
    sync::{mpsc, Arc},
    thread::{self, JoinHandle},
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::{
        LibraryLoader::GetModuleHandleW,
        Power::{
            RegisterPowerSettingNotification, RegisterSuspendResumeNotification,
            UnregisterPowerSettingNotification, UnregisterSuspendResumeNotification, HPOWERNOTIFY,
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
            DEVICE_NOTIFY_WINDOW_HANDLE, EVENT_OBJECT_CREATE, EVENT_SYSTEM_FOREGROUND, MSG,
            OBJID_WINDOW, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMECRITICAL, PBT_APMRESUMESTANDBY,
            PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, PBT_POWERSETTINGCHANGE, PM_NOREMOVE,
            WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_POWERBROADCAST, WM_QUIT,
            WM_SETTINGCHANGE, WM_WTSSESSION_CHANGE, WNDCLASSW,
        },
    },
};

use crate::win_util::wide_null;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsAutomationEvent {
    ForegroundChanged,
    WindowCreated,
    PowerChanged,
    SessionChanged,
    AppearanceChanged,
}

type EventCallback = Arc<dyn Fn(WindowsAutomationEvent) + Send + Sync>;

thread_local! {
    static EVENT_CALLBACK: RefCell<Option<EventCallback>> = RefCell::new(None);
}

pub struct WindowsEventWatcher {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

struct PowerNotifications {
    settings: Vec<HPOWERNOTIFY>,
    suspend_resume: HPOWERNOTIFY,
    settings_complete: bool,
}

impl WindowsEventWatcher {
    pub fn start(callback: EventCallback) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        let thread = thread::spawn(move || event_thread(sender, callback));

        match receiver.recv() {
            Ok(Ok(thread_id)) => Ok(Self {
                thread_id,
                thread: Some(thread),
            }),
            Ok(Err(err)) => {
                let _ = thread.join();
                Err(err)
            }
            Err(err) => {
                let _ = thread.join();
                Err(format!("Windows event watcher did not start: {err}"))
            }
        }
    }
}

impl Drop for WindowsEventWatcher {
    fn drop(&mut self) {
        // SAFETY: thread_id identifies the live watcher thread and WM_QUIT carries no pointers.
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn event_thread(sender: mpsc::Sender<Result<u32, String>>, callback: EventCallback) {
    // SAFETY: GetCurrentThreadId takes no arguments and has no caller requirements.
    let thread_id = unsafe { GetCurrentThreadId() };
    let mut msg = MSG::default();

    EVENT_CALLBACK.with(|slot| {
        *slot.borrow_mut() = Some(callback);
    });

    // SAFETY: msg is writable; PM_NOREMOVE creates this thread's message queue without consuming
    // a message.
    unsafe {
        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
    }

    let window = create_event_window();
    // SAFETY: win_event_proc has the required static ABI and the out-of-context hook retains no
    // borrowed Rust data.
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
    // SAFETY: win_event_proc has the required static ABI and the out-of-context hook retains no
    // borrowed Rust data.
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

    let mut power_notifications = PowerNotifications {
        settings: Vec::new(),
        suspend_resume: 0,
        settings_complete: false,
    };
    let mut session_notifications_registered = false;
    if !window.is_null() {
        power_notifications = register_power_notifications(window);
        // SAFETY: window is the live hidden event window owned by this thread.
        session_notifications_registered =
            unsafe { WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) } != 0;
    }

    if !event_sources_ready(
        window,
        foreground_hook,
        window_hook,
        &power_notifications,
        session_notifications_registered,
    ) {
        cleanup_event_sources(
            window,
            foreground_hook,
            window_hook,
            power_notifications,
            session_notifications_registered,
        );
        let _ = sender.send(Err(
            "Failed to register every required Windows automation event source.".to_owned(),
        ));
        EVENT_CALLBACK.with(|slot| {
            *slot.borrow_mut() = None;
        });
        return;
    }

    let _ = sender.send(Ok(thread_id));

    // SAFETY: msg is writable and the hidden window remains live while messages are dispatched.
    unsafe {
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    cleanup_event_sources(
        window,
        foreground_hook,
        window_hook,
        power_notifications,
        session_notifications_registered,
    );

    EVENT_CALLBACK.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

fn event_sources_ready(
    window: HWND,
    foreground_hook: HWINEVENTHOOK,
    window_hook: HWINEVENTHOOK,
    power_notifications: &PowerNotifications,
    session_notifications_registered: bool,
) -> bool {
    !window.is_null()
        && !foreground_hook.is_null()
        && !window_hook.is_null()
        && power_notifications.settings_complete
        && power_notifications.suspend_resume != 0
        && session_notifications_registered
}

fn cleanup_event_sources(
    window: HWND,
    foreground_hook: HWINEVENTHOOK,
    window_hook: HWINEVENTHOOK,
    power_notifications: PowerNotifications,
    session_notifications_registered: bool,
) {
    // SAFETY: every non-null handle was returned by its matching registration call on this
    // owning thread and is released at most once here.
    unsafe {
        if session_notifications_registered {
            WTSUnRegisterSessionNotification(window);
        }
        for notification in power_notifications.settings {
            UnregisterPowerSettingNotification(notification);
        }
        if power_notifications.suspend_resume != 0 {
            UnregisterSuspendResumeNotification(power_notifications.suspend_resume);
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
}

fn create_event_window() -> HWND {
    let class_name = wide_null("WinderustAutomationEvents");
    // SAFETY: A null module name asks Windows for the current process module.
    let module = unsafe { GetModuleHandleW(null()) };
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(event_window_proc),
        hInstance: module as _,
        lpszClassName: class_name.as_ptr(),
        // SAFETY: WNDCLASSW is a plain Win32 data structure for which zero is a valid default.
        ..unsafe { std::mem::zeroed() }
    };

    // SAFETY: class_name remains alive for registration and creation, event_window_proc has the
    // required static ABI, and all optional handles and creation data are null.
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
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            module as _,
            std::ptr::null(),
        )
    }
}

fn register_power_notifications(window: HWND) -> PowerNotifications {
    let power_settings = [
        GUID_ACDC_POWER_SOURCE,
        GUID_BATTERY_PERCENTAGE_REMAINING,
        GUID_POWERSCHEME_PERSONALITY,
    ];
    let mut settings = Vec::new();

    for setting in &power_settings {
        // SAFETY: window is live and setting points to a static GUID for the duration of the call.
        let notification = unsafe {
            RegisterPowerSettingNotification(window as _, setting, DEVICE_NOTIFY_WINDOW_HANDLE)
        };
        if notification != 0 {
            settings.push(notification);
        }
    }

    // SAFETY: window is live and the registration stores only the HWND value.
    let suspend_resume =
        unsafe { RegisterSuspendResumeNotification(window as _, DEVICE_NOTIFY_WINDOW_HANDLE) };
    let settings_complete = settings.len() == power_settings.len();
    PowerNotifications {
        settings,
        suspend_resume,
        settings_complete,
    }
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
        WM_SETTINGCHANGE => {
            notify_event(WindowsAutomationEvent::AppearanceChanged);
            0
        }
        // SAFETY: Forwarding unhandled messages with their unchanged callback arguments is the
        // required window-procedure contract.
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
    let callback = EVENT_CALLBACK.with(|slot| slot.borrow().clone());
    if let Some(callback) = callback {
        if catch_unwind(AssertUnwindSafe(|| callback(event))).is_err() {
            eprintln!("Windows event callback panicked.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watcher_requires_every_event_source() {
        let handle = std::ptr::dangling_mut();
        let complete_power = PowerNotifications {
            settings: Vec::new(),
            suspend_resume: 1,
            settings_complete: true,
        };

        assert!(event_sources_ready(
            handle,
            handle,
            handle,
            &complete_power,
            true,
        ));
        assert!(!event_sources_ready(
            std::ptr::null_mut(),
            handle,
            handle,
            &complete_power,
            true,
        ));

        let incomplete_power = PowerNotifications {
            settings_complete: false,
            ..complete_power
        };
        assert!(!event_sources_ready(
            handle,
            handle,
            handle,
            &incomplete_power,
            true,
        ));
    }

    #[test]
    fn callback_panic_does_not_escape_windows_dispatch() {
        EVENT_CALLBACK.with(|slot| {
            *slot.borrow_mut() = Some(Arc::new(|_| panic!("callback failed")));
        });

        notify_event(WindowsAutomationEvent::ForegroundChanged);

        EVENT_CALLBACK.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }
}
