use std::{
    cell::RefCell,
    panic::{catch_unwind, AssertUnwindSafe},
    ptr::null,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
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
        SystemServices::{GUID_ACDC_POWER_SOURCE, GUID_POWERSCHEME_PERSONALITY},
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
            WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_DWMCOLORIZATIONCOLORCHANGED,
            WM_POWERBROADCAST, WM_QUIT, WM_SETTINGCHANGE, WM_SYSCOLORCHANGE, WM_THEMECHANGED,
            WM_TIMECHANGE, WM_WTSSESSION_CHANGE, WNDCLASSW,
        },
    },
};

use crate::win_util::wide_null;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsAutomationEvent {
    ForegroundChanged,
    WindowCreated,
    PowerChanged,
    SuspendResume,
    SessionChanged,
    ClockChanged,
    AppearanceChanged,
}

type EventCallback = Arc<dyn Fn(WindowsAutomationEvent) + Send + Sync>;

thread_local! {
    static EVENT_CALLBACK: RefCell<Option<EventCallback>> = RefCell::new(None);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsEventWatcherMode {
    Full,
    SuspendResumeAndSession,
    SuspendResumeOnly,
}

pub struct WindowsEventWatcher {
    mode: WindowsEventWatcherMode,
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
    stopped: Arc<AtomicBool>,
}

struct WatcherStopped {
    stopped: Arc<AtomicBool>,
    callback: Arc<dyn Fn() + Send + Sync>,
}

impl Drop for WatcherStopped {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        (self.callback)();
    }
}

struct PowerNotifications {
    settings: Vec<HPOWERNOTIFY>,
    suspend_resume: HPOWERNOTIFY,
    settings_complete: bool,
}

impl WindowsEventWatcher {
    pub fn start(
        mode: WindowsEventWatcherMode,
        callback: EventCallback,
        on_stopped: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let notification = WatcherStopped {
            stopped: Arc::clone(&stopped),
            callback: on_stopped,
        };
        let thread = thread::spawn(move || {
            let _notification = notification;
            event_thread(sender, callback, mode);
        });

        match receiver.recv() {
            Ok(Ok(thread_id)) => Ok(Self {
                mode,
                thread_id,
                thread: Some(thread),
                stopped,
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

    pub fn mode(&self) -> WindowsEventWatcherMode {
        self.mode
    }

    pub fn is_running(&self) -> bool {
        !self.stopped.load(Ordering::Acquire)
            && self
                .thread
                .as_ref()
                .is_some_and(|thread| !thread.is_finished())
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

fn event_thread(
    sender: mpsc::Sender<Result<u32, String>>,
    callback: EventCallback,
    mode: WindowsEventWatcherMode,
) {
    let full = mode == WindowsEventWatcherMode::Full;
    let session = mode != WindowsEventWatcherMode::SuspendResumeOnly;
    // SAFETY: GetCurrentThreadId takes no arguments and has no caller requirements.
    let thread_id = unsafe { GetCurrentThreadId() };
    let mut msg = MSG::default();

    let callback = if full {
        callback
    } else {
        Arc::new(move |event| {
            if event == WindowsAutomationEvent::SuspendResume
                || (session && event == WindowsAutomationEvent::SessionChanged)
            {
                callback(event);
            }
        })
    };
    EVENT_CALLBACK.with(|slot| {
        *slot.borrow_mut() = Some(callback);
    });

    // SAFETY: msg is writable; PM_NOREMOVE creates this thread's message queue without consuming
    // a message.
    unsafe {
        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
    }

    let window = create_event_window();
    let (foreground_hook, window_hook) = if full {
        (
            register_window_hook(EVENT_SYSTEM_FOREGROUND),
            register_window_hook(EVENT_OBJECT_CREATE),
        )
    } else {
        (std::ptr::null_mut(), std::ptr::null_mut())
    };

    let mut power_notifications = PowerNotifications {
        settings: Vec::new(),
        suspend_resume: 0,
        settings_complete: false,
    };
    let mut session_notifications_registered = false;
    if !window.is_null() {
        power_notifications = register_power_notifications(window, full);
        if session {
            // SAFETY: window is the live hidden event window owned by this thread.
            session_notifications_registered =
                unsafe { WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) } != 0;
        }
    }

    if !event_sources_ready(
        mode,
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
    mode: WindowsEventWatcherMode,
    window: HWND,
    foreground_hook: HWINEVENTHOOK,
    window_hook: HWINEVENTHOOK,
    power_notifications: &PowerNotifications,
    session_notifications_registered: bool,
) -> bool {
    !window.is_null()
        && power_notifications.suspend_resume != 0
        && (mode == WindowsEventWatcherMode::SuspendResumeOnly || session_notifications_registered)
        && (mode != WindowsEventWatcherMode::Full
            || (!foreground_hook.is_null()
                && !window_hook.is_null()
                && power_notifications.settings_complete))
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

fn register_window_hook(event: u32) -> HWINEVENTHOOK {
    // Focusing Winderust can end another app's By Focus match.
    let flags = if event == EVENT_SYSTEM_FOREGROUND {
        WINEVENT_OUTOFCONTEXT
    } else {
        WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS
    };
    // SAFETY: win_event_proc has the required static ABI and the out-of-context hook
    // retains no borrowed Rust data.
    unsafe {
        SetWinEventHook(
            event,
            event,
            std::ptr::null_mut(),
            Some(win_event_proc),
            0,
            0,
            flags,
        )
    }
}

fn register_power_notifications(window: HWND, include_power_settings: bool) -> PowerNotifications {
    let power_settings: &[_] = if include_power_settings {
        &[GUID_ACDC_POWER_SOURCE, GUID_POWERSCHEME_PERSONALITY]
    } else {
        &[]
    };
    let mut settings = Vec::new();

    for setting in power_settings {
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
                PBT_POWERSETTINGCHANGE => notify_event(WindowsAutomationEvent::PowerChanged),
                PBT_APMRESUMEAUTOMATIC
                | PBT_APMRESUMECRITICAL
                | PBT_APMRESUMESTANDBY
                | PBT_APMRESUMESUSPEND
                | PBT_APMSUSPEND => notify_event(WindowsAutomationEvent::SuspendResume),
                _ => {}
            }
            1
        }
        WM_WTSSESSION_CHANGE => {
            notify_event(WindowsAutomationEvent::SessionChanged);
            0
        }
        WM_TIMECHANGE => {
            notify_event(WindowsAutomationEvent::ClockChanged);
            0
        }
        WM_SETTINGCHANGE | WM_SYSCOLORCHANGE | WM_THEMECHANGED | WM_DWMCOLORIZATIONCOLORCHANGED => {
            // System settings include time-zone changes; recheck without interpreting lparam.
            if message == WM_SETTINGCHANGE {
                notify_event(WindowsAutomationEvent::ClockChanged);
            }
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
    fn watcher_exit_marks_stopped_before_notifying_once_on_return_or_unwind() {
        use std::sync::atomic::AtomicUsize;

        for unwind in [false, true] {
            let stopped = Arc::new(AtomicBool::new(false));
            let observed_stopped = Arc::new(AtomicBool::new(false));
            let notifications = Arc::new(AtomicUsize::new(0));
            let callback_stopped = Arc::clone(&stopped);
            let callback_observed = Arc::clone(&observed_stopped);
            let callback_notifications = Arc::clone(&notifications);
            let notification = WatcherStopped {
                stopped: Arc::clone(&stopped),
                callback: Arc::new(move || {
                    callback_observed
                        .store(callback_stopped.load(Ordering::Acquire), Ordering::Release);
                    callback_notifications.fetch_add(1, Ordering::Relaxed);
                }),
            };
            let worker = thread::spawn(move || {
                let _notification = notification;
                if unwind {
                    panic!("simulated watcher failure");
                }
            });
            assert_eq!(worker.join().is_err(), unwind);
            assert!(stopped.load(Ordering::Acquire));
            assert!(observed_stopped.load(Ordering::Acquire));
            assert_eq!(notifications.load(Ordering::Relaxed), 1);
        }
    }

    #[test]
    fn power_setting_and_suspend_resume_messages_remain_distinct() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let received = Arc::clone(&events);
        EVENT_CALLBACK.with(|slot| {
            *slot.borrow_mut() = Some(Arc::new(move |event| {
                received.lock().unwrap().push(event);
            }));
        });
        for broadcast in [
            PBT_POWERSETTINGCHANGE,
            PBT_APMSUSPEND,
            PBT_APMRESUMEAUTOMATIC,
            PBT_APMRESUMECRITICAL,
            PBT_APMRESUMESTANDBY,
            PBT_APMRESUMESUSPEND,
        ] {
            assert_eq!(
                // SAFETY: These broadcasts are classified by wparam without reading lparam.
                unsafe {
                    event_window_proc(
                        std::ptr::null_mut(),
                        WM_POWERBROADCAST,
                        broadcast as WPARAM,
                        0,
                    )
                },
                1
            );
        }
        EVENT_CALLBACK.with(|slot| *slot.borrow_mut() = None);
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                WindowsAutomationEvent::PowerChanged,
                WindowsAutomationEvent::SuspendResume,
                WindowsAutomationEvent::SuspendResume,
                WindowsAutomationEvent::SuspendResume,
                WindowsAutomationEvent::SuspendResume,
                WindowsAutomationEvent::SuspendResume,
            ]
        );
    }

    #[test]
    fn appearance_messages_refresh_the_shared_theme() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let received = Arc::clone(&events);
        EVENT_CALLBACK.with(|slot| {
            *slot.borrow_mut() = Some(Arc::new(move |event| {
                received.lock().unwrap().push(event);
            }));
        });
        for message in [
            WM_SETTINGCHANGE,
            WM_SYSCOLORCHANGE,
            WM_THEMECHANGED,
            WM_DWMCOLORIZATIONCOLORCHANGED,
            WM_TIMECHANGE,
        ] {
            assert_eq!(
                // SAFETY: These appearance messages use no window handle or pointer arguments.
                unsafe { event_window_proc(std::ptr::null_mut(), message, 0, 0) },
                0
            );
        }
        EVENT_CALLBACK.with(|slot| *slot.borrow_mut() = None);
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                WindowsAutomationEvent::ClockChanged,
                WindowsAutomationEvent::AppearanceChanged,
                WindowsAutomationEvent::AppearanceChanged,
                WindowsAutomationEvent::AppearanceChanged,
                WindowsAutomationEvent::AppearanceChanged,
                WindowsAutomationEvent::ClockChanged,
            ]
        );
    }

    #[test]
    fn watcher_requires_every_event_source() {
        let handle = std::ptr::dangling_mut();
        let complete_power = PowerNotifications {
            settings: Vec::new(),
            suspend_resume: 1,
            settings_complete: true,
        };

        assert!(event_sources_ready(
            WindowsEventWatcherMode::Full,
            handle,
            handle,
            handle,
            &complete_power,
            true,
        ));
        assert!(!event_sources_ready(
            WindowsEventWatcherMode::Full,
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
            WindowsEventWatcherMode::Full,
            handle,
            handle,
            handle,
            &incomplete_power,
            true,
        ));

        let no_hook = std::ptr::null_mut();
        assert!(event_sources_ready(
            WindowsEventWatcherMode::SuspendResumeAndSession,
            handle,
            no_hook,
            no_hook,
            &incomplete_power,
            true,
        ));
        assert!(!event_sources_ready(
            WindowsEventWatcherMode::SuspendResumeAndSession,
            handle,
            no_hook,
            no_hook,
            &incomplete_power,
            false,
        ));
        assert!(event_sources_ready(
            WindowsEventWatcherMode::SuspendResumeOnly,
            handle,
            no_hook,
            no_hook,
            &incomplete_power,
            false,
        ));
        let no_suspend_resume = PowerNotifications {
            suspend_resume: 0,
            ..incomplete_power
        };
        assert!(!event_sources_ready(
            WindowsEventWatcherMode::SuspendResumeOnly,
            handle,
            no_hook,
            no_hook,
            &no_suspend_resume,
            false,
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
