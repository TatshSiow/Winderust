//! Bridge the resident application's native close request to asynchronous restoration.
//!
//! `WindUiAppHostClass` and `WindUiWindowClass` are undocumented contracts verified against
//! windui revision d5c105067d7a31f0f14e388f1fe6394f1a043988 (0.28.0). Its app-host `WM_CLOSE` destroys
//! registered UI windows before ending the message loop; `EventCtx::force_close` in an app
//! channel does not quit. Its tray icon uses native ID 1 and WM_APP + 1 on that host.
//! Its tray menu cannot express submenus or dynamic visibility, so this adapter intercepts
//! right clicks for the existing Win32 popup. Revalidate these contracts on framework updates.

use std::{
    cell::{Cell, RefCell},
    error::Error,
    fmt,
    panic::{catch_unwind, AssertUnwindSafe},
    rc::Rc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

mod tray_monitor;

use windows_sys::Win32::{
    Foundation::{
        GetLastError, HANDLE, HWND, LPARAM, LRESULT, RECT, WAIT_FAILED, WAIT_OBJECT_0, WPARAM,
    },
    System::Threading::{
        CreateEventW, GetCurrentProcessId, GetCurrentThreadId, SetEvent, WaitForMultipleObjects,
        INFINITE,
    },
    UI::{
        Input::KeyboardAndMouse::GetCapture,
        Shell::{
            DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass,
            Shell_NotifyIconGetRect, NOTIFYICONIDENTIFIER,
        },
        WindowsAndMessaging::{
            CallNextHookEx, ChangeWindowMessageFilterEx, EnumThreadWindows, GetClassNameW,
            GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible, KillTimer,
            PostMessageW, RegisterWindowMessageW, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx,
            CWPSTRUCT, HC_ACTION, HHOOK, MSG, MSGFLT_ALLOW, PBT_APMPOWERSTATUSCHANGE, PM_REMOVE,
            WH_CALLWNDPROC, WH_GETMESSAGE, WM_APP, WM_CLOSE, WM_CREATE, WM_DESTROY, WM_NCDESTROY,
            WM_NULL, WM_POWERBROADCAST, WM_RBUTTONUP, WM_SHOWWINDOW, WM_SIZE, WM_TIMER,
            WM_WINDOWPOSCHANGED, WM_XBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP, XBUTTON1,
            XBUTTON2,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowVisibility {
    Absent,
    Hidden,
    Minimized,
    Visible,
}

impl WindowVisibility {
    /// A minimized window remains visible in Windows; only hidden/absent UI enables the
    /// application's existing background power policy.
    pub(crate) fn is_visible(self) -> bool {
        matches!(self, Self::Visible | Self::Minimized)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResidentShellError {
    HostNotFound,
    MultipleHosts,
    WrongThread,
    HostUnavailable,
    Native { operation: &'static str, code: u32 },
    ListenerStart { code: Option<i32> },
    NavigationAlreadyAttached,
    VisibilityAlreadyAttached,
    TrayAlreadyAttached,
    TrayListenerStart { code: Option<i32> },
    TrayWorkerUnavailable,
    TrayProbeTimedOut,
}

impl fmt::Display for ResidentShellError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HostNotFound => formatter.write_str("Wind UI resident host was not found."),
            Self::MultipleHosts => {
                formatter.write_str("Multiple Wind UI resident hosts were found.")
            }
            Self::WrongThread => formatter.write_str("The resident shell requires its UI thread."),
            Self::HostUnavailable => {
                formatter.write_str("The resident host is no longer attached.")
            }
            Self::Native { operation, code } => write!(formatter, "{operation} failed ({code})."),
            Self::ListenerStart { code } => match code {
                Some(code) => write!(formatter, "Starting the restore listener failed ({code})."),
                None => formatter.write_str("Starting the restore listener failed."),
            },
            Self::NavigationAlreadyAttached => {
                formatter.write_str("Window navigation is already attached.")
            }
            Self::VisibilityAlreadyAttached => {
                formatter.write_str("Window visibility is already attached.")
            }
            Self::TrayAlreadyAttached => formatter.write_str("Tray health is already attached."),
            Self::TrayListenerStart { code: Some(code) } => {
                write!(
                    formatter,
                    "Starting the tray-health worker failed ({code})."
                )
            }
            Self::TrayListenerStart { code: None } => {
                formatter.write_str("Starting the tray-health worker failed.")
            }
            Self::TrayWorkerUnavailable => {
                formatter.write_str("The tray-health worker is unavailable.")
            }
            Self::TrayProbeTimedOut => formatter.write_str("The tray-health query timed out."),
        }
    }
}

impl Error for ResidentShellError {}

struct CloseGuard {
    process_id: u32,
    thread_id: u32,
    attached: Cell<bool>,
    quit_authorized: Cell<bool>,
    request_exit: Box<dyn Fn()>,
    taskbar_created: u32,
    tray_completed: u32,
    tray: RefCell<Option<tray_monitor::TrayMonitor>>,
    retry_due: Cell<Option<Instant>>,
}

/// Blocks on native events without retaining a window handle or polling the UI thread.
/// The callback may only enqueue application work; it must not borrow UI state.
pub(crate) struct RestoreListener {
    cancel: Option<crate::win_util::WinHandle>,
    worker: Option<JoinHandle<()>>,
}

impl RestoreListener {
    pub(crate) fn start(
        event: crate::SingleInstanceRestoreEvent,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self, ResidentShellError> {
        // SAFETY: no name or security attributes are borrowed; this call creates an owned
        // manual-reset event whose sole purpose is cancellation of the native wait.
        let cancel = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if cancel.is_null() {
            return Err(native_error(
                "Creating the restore-listener cancellation event",
            ));
        }
        let cancel = crate::win_util::WinHandle::new(cancel);
        let cancel_raw = cancel.raw() as usize;
        let worker = std::thread::Builder::new()
            .name("winderust-instance-restore".into())
            .spawn(move || Self::wait(event, cancel_raw, notify))
            .map_err(|error| ResidentShellError::ListenerStart {
                code: error.raw_os_error(),
            })?;
        Ok(Self {
            cancel: Some(cancel),
            worker: Some(worker),
        })
    }

    fn wait(event: crate::SingleInstanceRestoreEvent, cancel_raw: usize, notify: impl Fn()) {
        // The worker owns event. RestoreListener retains cancel until this worker
        // has exited, so neither handle is closed during WaitForMultipleObjects.
        let handles = [cancel_raw as HANDLE, event.handle.raw()];
        loop {
            // SAFETY: both event handles remain live for this worker's entire wait;
            // the local two-element array is valid for this synchronous call.
            let result = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
            if result == WAIT_OBJECT_0 {
                break;
            }
            if result == WAIT_OBJECT_0 + 1 {
                if catch_unwind(AssertUnwindSafe(&notify)).is_err() {
                    crate::backend::diagnostics::error("The restore-listener callback panicked.");
                }
            } else {
                let error = if result == WAIT_FAILED {
                    native_error("Waiting for a second-instance restore request")
                } else {
                    ResidentShellError::Native {
                        operation: "Unexpected restore-listener wait result",
                        code: result,
                    }
                };
                crate::backend::diagnostics::error(&error.to_string());
                break;
            }
        }
    }
}

impl Drop for RestoreListener {
    fn drop(&mut self) {
        let Some(cancel) = self.cancel.take() else {
            return;
        };
        // SAFETY: this guard owns the live cancellation event and no other code closes it.
        if unsafe { SetEvent(cancel.raw()) } == 0 {
            crate::backend::diagnostics::error(
                &native_error("Canceling the restore listener").to_string(),
            );
            // A failed cancellation must not close a handle still being waited on. Keep
            // it alive until process teardown rather than cause an invalid native wait.
            std::mem::forget(cancel);
            return;
        }
        if self
            .worker
            .take()
            .is_some_and(|worker| worker.join().is_err())
        {
            crate::backend::diagnostics::error("The restore-listener worker panicked.");
        }
        // cancel is closed only after the worker's wait and callback have returned.
    }
}

/// UI-thread-only owner. The callback must enqueue an exit request, without borrowing UI state
/// or performing synchronous cleanup from the native window procedure.
pub(crate) struct ResidentShell {
    host: HWND,
    guard: Rc<CloseGuard>,
    navigation: Option<NavigationListener>,
    visibility_listener: Option<VisibilityListener>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavigationDirection {
    Back,
    Forward,
}

struct NavigationState {
    process_id: u32,
    thread_id: u32,
    window_class: &'static str,
    notify: Box<dyn Fn(NavigationDirection)>,
}

thread_local! {
    static NAVIGATION: RefCell<Option<Rc<NavigationState>>> = const { RefCell::new(None) };
    static VISIBILITY: RefCell<Option<Rc<VisibilityState>>> = const { RefCell::new(None) };
}

struct VisibilityState {
    process_id: u32,
    thread_id: u32,
    window_class: &'static str,
    notify: Box<dyn Fn()>,
}

/// Observes sent lifecycle messages before destruction, without retaining any UI HWND.
struct VisibilityListener {
    hook: HHOOK,
    state: Rc<VisibilityState>,
}

impl VisibilityListener {
    fn start(
        window_class: &'static str,
        notify: impl Fn() + 'static,
    ) -> Result<Self, ResidentShellError> {
        if VISIBILITY.with(|current| current.borrow().is_some()) {
            return Err(ResidentShellError::VisibilityAlreadyAttached);
        }
        let state = Rc::new(VisibilityState {
            // SAFETY: current identity queries have no preconditions.
            process_id: unsafe { GetCurrentProcessId() },
            // SAFETY: current identity queries have no preconditions.
            thread_id: unsafe { GetCurrentThreadId() },
            window_class,
            notify: Box::new(notify),
        });
        // SAFETY: this executable owns the static hook and the target is this UI thread;
        // no DLL injection or observation of other processes is requested.
        let hook = unsafe {
            SetWindowsHookExW(
                WH_CALLWNDPROC,
                Some(visibility_proc),
                std::ptr::null_mut(),
                state.thread_id,
            )
        };
        if hook.is_null() {
            return Err(native_error("Observing native window visibility"));
        }
        VISIBILITY.with(|current| *current.borrow_mut() = Some(state.clone()));
        Ok(Self { hook, state })
    }
}

impl Drop for VisibilityListener {
    fn drop(&mut self) {
        VISIBILITY.with(|current| {
            let mut current = current.borrow_mut();
            if current
                .as_ref()
                .is_some_and(|state| Rc::ptr_eq(state, &self.state))
            {
                *current = None;
            }
        });
        // SAFETY: this non-Send owner installed the hook on this same UI thread. Its
        // registration was cleared first, so a failed removal leaves only forwarding code.
        if unsafe { UnhookWindowsHookEx(self.hook) } == 0 {
            crate::backend::diagnostics::error(
                &native_error("Detaching native visibility observation").to_string(),
            );
        }
    }
}

unsafe extern "system" fn visibility_proc(code: i32, sent: WPARAM, data: LPARAM) -> LRESULT {
    if code >= HC_ACTION as i32 && data != 0 {
        // SAFETY: WH_CALLWNDPROC supplies a CWPSTRUCT valid until this callback returns.
        let message = unsafe { &*(data as *const CWPSTRUCT) };
        if matches!(
            message.message,
            WM_CREATE | WM_SHOWWINDOW | WM_SIZE | WM_WINDOWPOSCHANGED | WM_DESTROY | WM_NCDESTROY
        ) {
            let state = VISIBILITY.with(|current| current.borrow().clone());
            if let Some(state) = state {
                if owns_ui_window(
                    message.hwnd,
                    state.process_id,
                    state.thread_id,
                    state.window_class,
                ) && catch_unwind(AssertUnwindSafe(|| (state.notify)())).is_err()
                {
                    crate::backend::diagnostics::error("The native visibility callback panicked.");
                }
            }
        }
    }
    // SAFETY: this observation hook never changes the message or consumes its delivery.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, sent, data) }
}

/// Thread-scoped message observation follows windows created after a close/reopen without
/// retaining their HWNDs. No global input hook or background input sampling is installed.
struct NavigationListener {
    hook: HHOOK,
    state: Rc<NavigationState>,
}

impl NavigationListener {
    fn start(
        window_class: &'static str,
        notify: impl Fn(NavigationDirection) + 'static,
    ) -> Result<Self, ResidentShellError> {
        if NAVIGATION.with(|current| current.borrow().is_some()) {
            return Err(ResidentShellError::NavigationAlreadyAttached);
        }
        let state = Rc::new(NavigationState {
            // SAFETY: current process/thread identity queries have no preconditions.
            process_id: unsafe { GetCurrentProcessId() },
            // SAFETY: current thread identity queries have no preconditions.
            thread_id: unsafe { GetCurrentThreadId() },
            window_class,
            notify: Box::new(notify),
        });
        // SAFETY: this executable owns the static callback and the specified current thread;
        // no DLL injection is required. The hook never accesses framework-owned state.
        let hook = unsafe {
            SetWindowsHookExW(
                WH_GETMESSAGE,
                Some(navigation_proc),
                std::ptr::null_mut(),
                state.thread_id,
            )
        };
        if hook.is_null() {
            return Err(native_error("Attaching native window navigation"));
        }
        NAVIGATION.with(|current| *current.borrow_mut() = Some(state.clone()));
        Ok(Self { hook, state })
    }
}

impl Drop for NavigationListener {
    fn drop(&mut self) {
        NAVIGATION.with(|current| {
            let mut current = current.borrow_mut();
            if current
                .as_ref()
                .is_some_and(|state| Rc::ptr_eq(state, &self.state))
            {
                *current = None;
            }
        });
        // SAFETY: this non-Send guard owns the hook installed on this same UI thread.
        if unsafe { UnhookWindowsHookEx(self.hook) } == 0 {
            // The static callback remains safe even if removal fails: its thread-local
            // registration has been cleared and it only forwards remaining messages.
            crate::backend::diagnostics::error(
                &native_error("Detaching native window navigation").to_string(),
            );
        }
    }
}

unsafe extern "system" fn navigation_proc(code: i32, removed: WPARAM, data: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && removed == PM_REMOVE as usize && data != 0 {
        // SAFETY: WH_GETMESSAGE supplies a writable MSG for HC_ACTION. Only removed messages
        // are handled, so repeated PM_NOREMOVE peeks cannot repeat a navigation action.
        let message = unsafe { &mut *(data as *mut MSG) };
        let button = (message.wParam >> 16) as u16;
        if matches!(
            message.message,
            WM_XBUTTONDOWN | WM_XBUTTONDBLCLK | WM_XBUTTONUP
        ) && matches!(button, XBUTTON1 | XBUTTON2)
        {
            let state = NAVIGATION.with(|current| current.borrow().clone());
            if let Some(state) = state.filter(|state| state.owns_ui_window(message.hwnd)) {
                if message.message != WM_XBUTTONUP {
                    let direction = if button == XBUTTON1 {
                        NavigationDirection::Back
                    } else {
                        NavigationDirection::Forward
                    };
                    // Release the thread-local borrow before enqueuing. The callback must not
                    // borrow the application, and a panic must never unwind through user32.
                    if catch_unwind(AssertUnwindSafe(|| (state.notify)(direction))).is_err() {
                        crate::backend::diagnostics::error(
                            "The native navigation callback panicked.",
                        );
                    }
                }
                // Consume release too: DefWindowProc synthesizes WM_APPCOMMAND on XBUTTONUP.
                // No other mouse button, unrelated window, or native dialog is changed.
                message.message = WM_NULL;
                message.wParam = 0;
                message.lParam = 0;
            }
        }
    }
    // SAFETY: every message, including negative hook codes, continues down the hook chain;
    // the documented first parameter is ignored and no hook handle is borrowed.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, removed, data) }
}

impl NavigationState {
    fn owns_ui_window(&self, hwnd: HWND) -> bool {
        owns_ui_window(hwnd, self.process_id, self.thread_id, self.window_class)
    }
}

fn owns_ui_window(hwnd: HWND, process_id: u32, thread_id: u32, class: &str) -> bool {
    let mut owner_process = 0;
    // SAFETY: this is a read-only identity query; invalid HWNDs return zero.
    let owner_thread = unsafe { GetWindowThreadProcessId(hwnd, &mut owner_process) };
    owner_process == process_id
        && owner_thread == thread_id
        && window_class(hwnd).is_ok_and(|name| name == class)
}

/// Keep captured native controls alive until the UI thread finishes their pointer gesture.
pub(crate) fn ui_pointer_capture_active() -> bool {
    pointer_capture_for_class("WindUiWindowClass")
}

fn pointer_capture_for_class(class: &str) -> bool {
    // SAFETY: GetCapture only queries capture associated with the calling thread.
    let hwnd = unsafe { GetCapture() };
    if hwnd.is_null() {
        return false;
    }
    // SAFETY: GetCurrentProcessId has no preconditions.
    let process_id = unsafe { GetCurrentProcessId() };
    // SAFETY: GetCurrentThreadId has no preconditions.
    let thread_id = unsafe { GetCurrentThreadId() };
    owns_ui_window(hwnd, process_id, thread_id, class)
}

/// An owned, read-only identity for querying the shell away from UI/framework borrows.
/// A probe may travel to a worker; it never owns or destroys the native window.
#[derive(Clone)]
pub(crate) struct TrayProbe {
    host: usize,
    process_id: u32,
    thread_id: u32,
}

impl TrayProbe {
    /// Failure to resolve the icon includes missing/restarting Explorer and is treated as
    /// unavailable. Callers must keep an accessible UI until a query confirms the icon.
    pub(crate) fn is_registered(&self) -> Result<bool, ResidentShellError> {
        self.validate()?;
        let identifier = NOTIFYICONIDENTIFIER {
            cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: self.host as HWND,
            uID: 1,
            ..Default::default()
        };
        let mut rectangle = RECT::default();
        // SAFETY: identifier contains the validated app host/native icon ID, and rectangle
        // is writable. This query does not transfer ownership or mutate the native window.
        let status = unsafe { Shell_NotifyIconGetRect(&identifier, &mut rectangle) };
        // The host may have been destroyed while the worker queried Explorer.
        self.validate()?;
        Ok(status == 0)
    }

    fn validate(&self) -> Result<(), ResidentShellError> {
        let mut process_id = 0;
        // SAFETY: GetWindowThreadProcessId only queries the numeric handle and writes the
        // supplied process ID. It returns zero if the host no longer exists.
        let thread_id = unsafe { GetWindowThreadProcessId(self.host as HWND, &mut process_id) };
        if process_id != self.process_id || thread_id != self.thread_id {
            return Err(ResidentShellError::HostUnavailable);
        }
        if window_class(self.host as HWND)? != "WindUiAppHostClass" {
            return Err(ResidentShellError::HostUnavailable);
        }
        Ok(())
    }
}

impl ResidentShell {
    pub(crate) fn attach(request_exit: impl Fn() + 'static) -> Result<Self, ResidentShellError> {
        let windows = thread_windows()?;
        let host = match windows.hosts.as_slice() {
            [] => return Err(ResidentShellError::HostNotFound),
            [host] => *host,
            _ => return Err(ResidentShellError::MultipleHosts),
        };
        Self::attach_to(host, request_exit)
    }

    fn attach_to(
        host: HWND,
        request_exit: impl Fn() + 'static,
    ) -> Result<Self, ResidentShellError> {
        // SAFETY: GetCurrentThreadId has no preconditions.
        let thread_id = unsafe { GetCurrentThreadId() };
        let mut process_id = 0;
        // SAFETY: process_id is writable; GetWindowThreadProcessId only queries the handle.
        let owner_thread = unsafe { GetWindowThreadProcessId(host, &mut process_id) };
        // SAFETY: GetCurrentProcessId has no preconditions.
        if owner_thread != thread_id || process_id != unsafe { GetCurrentProcessId() } {
            return Err(ResidentShellError::WrongThread);
        }
        let taskbar_name = crate::win_util::wide_null("TaskbarCreated");
        // SAFETY: taskbar_name is valid terminated UTF-16 throughout registration.
        let taskbar_created = unsafe { RegisterWindowMessageW(taskbar_name.as_ptr()) };
        if taskbar_created == 0 {
            return Err(native_error(
                "Registering the Explorer-restart notification",
            ));
        }
        // SAFETY: host is this process's live UI-thread window; this payload-free shell
        // notification is the only newly permitted message from lower-integrity Explorer.
        if unsafe {
            ChangeWindowMessageFilterEx(host, taskbar_created, MSGFLT_ALLOW, std::ptr::null_mut())
        } == 0
        {
            return Err(native_error("Allowing the Explorer-restart notification"));
        }
        let completion_name =
            crate::win_util::wide_null(&format!("Winderust.ResidentTrayHealth.{process_id}"));
        // SAFETY: completion_name remains terminated UTF-16 throughout registration.
        let tray_completed = unsafe { RegisterWindowMessageW(completion_name.as_ptr()) };
        if tray_completed == 0 {
            return Err(native_error(
                "Registering the tray-health completion message",
            ));
        }
        let guard = Rc::new(CloseGuard {
            process_id,
            thread_id,
            attached: Cell::new(true),
            quit_authorized: Cell::new(false),
            request_exit: Box::new(request_exit),
            taskbar_created,
            tray_completed,
            tray: RefCell::new(None),
            retry_due: Cell::new(None),
        });
        // The subclass owns one strong reference until removal or WM_NCDESTROY. A callback
        // takes its own temporary reference so dropping the adapter during a callback is safe.
        let data = Rc::into_raw(guard.clone()) as usize;
        let id = data;
        // SAFETY: host belongs to this thread and data remains alive through the subclass's
        // owned Rc reference. The address also gives this installation a unique subclass ID.
        if unsafe { SetWindowSubclass(host, Some(host_proc), id, data) } == 0 {
            let error = native_error("Installing the resident close guard");
            // SAFETY: installation failed, so no callback owns the extra reference.
            unsafe { drop(Rc::from_raw(data as *const CloseGuard)) };
            return Err(error);
        }
        Ok(Self {
            host,
            guard,
            navigation: None,
            visibility_listener: None,
        })
    }

    /// Add the mouse Back/Forward events not exposed by this pinned Wind UI version.
    /// The callback may only enqueue a typed intent, without borrowing UI state.
    pub(crate) fn listen_navigation(
        &mut self,
        notify: impl Fn(NavigationDirection) + 'static,
    ) -> Result<(), ResidentShellError> {
        self.validate()?;
        if self.navigation.is_some() {
            return Err(ResidentShellError::NavigationAlreadyAttached);
        }
        self.navigation = Some(NavigationListener::start("WindUiWindowClass", notify)?);
        Ok(())
    }

    /// Observe creation/show/hide/minimize/destruction of this thread's own UI windows.
    /// The callback runs before native dispatch finishes and must only enqueue work;
    /// the eventual application intent inspects the settled native visibility.
    pub(crate) fn listen_visibility(
        &mut self,
        notify: impl Fn() + 'static,
    ) -> Result<(), ResidentShellError> {
        self.validate()?;
        if self.visibility_listener.is_some() {
            return Err(ResidentShellError::VisibilityAlreadyAttached);
        }
        self.visibility_listener = Some(VisibilityListener::start("WindUiWindowClass", notify)?);
        Ok(())
    }

    pub(crate) fn tray_probe(&self) -> TrayProbe {
        TrayProbe {
            host: self.host as usize,
            process_id: self.guard.process_id,
            thread_id: self.guard.thread_id,
        }
    }

    /// Verify startup and native recovery events with one reusable Explorer-query worker.
    /// Healthy state has no timer; unavailable state retries until recovery. The callback
    /// must only enqueue a typed intent and must not borrow UI state.
    pub(crate) fn listen_tray(
        &mut self,
        notify: impl Fn(Result<bool, ResidentShellError>) + 'static,
    ) -> Result<(), ResidentShellError> {
        let probe = self.tray_probe();
        self.listen_tray_query(move || probe.is_registered(), notify)
    }

    fn listen_tray_query(
        &mut self,
        query: impl Fn() -> Result<bool, ResidentShellError> + Send + 'static,
        notify: impl Fn(Result<bool, ResidentShellError>) + 'static,
    ) -> Result<(), ResidentShellError> {
        self.validate()?;
        if self.guard.tray.borrow().is_some() {
            return Err(ResidentShellError::TrayAlreadyAttached);
        }
        let monitor = tray_monitor::TrayMonitor::start(
            self.host,
            Rc::as_ptr(&self.guard) as usize,
            self.guard.tray_completed,
            self.guard.process_id,
            self.guard.thread_id,
            query,
            notify,
        )?;
        *self.guard.tray.borrow_mut() = Some(monitor);
        Ok(())
    }

    pub(crate) fn refresh_tray(&self) {
        notify_tray(&self.guard, tray_monitor::TrayMonitor::refresh);
    }

    /// Retry a pending application change every five seconds. No timer exists when false.
    /// This deadline remains independent of a tray-health query blocked in Explorer.
    pub(crate) fn set_retry_pending(&self, pending: bool) -> Result<(), ResidentShellError> {
        self.validate()?;
        let id = retry_timer_id(&self.guard);
        if pending && self.guard.retry_due.get().is_none() {
            let deadline = Instant::now() + Duration::from_secs(5);
            // SAFETY: this UI-thread owner retains the exact host and its unique retry ID.
            // The host subclass consumes WM_TIMER before the framework sees this timer.
            if unsafe { SetTimer(self.host, id, timer_milliseconds(deadline), None) } == 0 {
                return Err(native_error("Scheduling a pending settings retry"));
            }
            self.guard.retry_due.set(Some(deadline));
        } else if !pending && self.guard.retry_due.get().is_some() {
            // SAFETY: this ID belongs to our live UI-thread host and no TIMERPROC exists.
            if unsafe { KillTimer(self.host, id) } == 0 {
                return Err(native_error("Canceling the pending settings retry"));
            }
            self.guard.retry_due.set(None);
        }
        Ok(())
    }

    /// Inspect actual UI windows, rather than the framework's application-context snapshot.
    /// A restored visible window takes precedence over minimized and hidden windows.
    pub(crate) fn visibility(&self) -> Result<WindowVisibility, ResidentShellError> {
        self.validate()?;
        Ok(visibility_for_windows(&thread_windows()?.ui))
    }

    /// Find an actual UI window for a native dialog owner; never return the hidden app host.
    /// The numeric handle is a borrowed identity and does not transfer native ownership.
    pub(crate) fn ui_window(&self) -> Result<Option<usize>, ResidentShellError> {
        self.validate()?;
        let windows = thread_windows()?.ui;
        // SAFETY: GetForegroundWindow is a read-only query without preconditions.
        let foreground = unsafe { GetForegroundWindow() };
        let selected = windows
            .iter()
            .copied()
            .find(|hwnd| *hwnd == foreground)
            .or_else(|| {
                windows.iter().copied().find(|hwnd| {
                    // SAFETY: enumeration selected live windows owned by this UI thread;
                    // no native messages have been dispatched since enumeration.
                    unsafe { IsWindowVisible(*hwnd) != 0 }
                })
            })
            .or_else(|| windows.first().copied());
        Ok(selected.map(|hwnd| hwnd as usize))
    }

    /// Authorize the upstream teardown only after runtime restoration has completed.
    /// Posting releases all callback/UI borrows before upstream destroys its windows.
    pub(crate) fn request_quit(&self) -> Result<(), ResidentShellError> {
        self.validate()?;
        self.guard.quit_authorized.set(true);
        // SAFETY: validate confirmed this exact subclass remains attached to our UI-thread
        // host. WM_CLOSE carries no pointers and is processed after this callback returns.
        if unsafe { PostMessageW(self.host, WM_CLOSE, 0, 0) } == 0 {
            self.guard.quit_authorized.set(false);
            return Err(native_error("Posting the resident quit request"));
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), ResidentShellError> {
        // SAFETY: GetCurrentThreadId has no preconditions.
        if unsafe { GetCurrentThreadId() } != self.guard.thread_id {
            return Err(ResidentShellError::WrongThread);
        }
        let mut data = 0;
        let expected = Rc::as_ptr(&self.guard) as usize;
        if !self.guard.attached.get() {
            return Err(ResidentShellError::HostUnavailable);
        }
        // SAFETY: GetWindowSubclass is a read-only lookup; an invalid or reused HWND cannot
        // match our unique procedure/ID/reference-data combination.
        if unsafe { GetWindowSubclass(self.host, Some(host_proc), expected, &mut data) } == 0
            || data != expected
        {
            return Err(ResidentShellError::HostUnavailable);
        }
        Ok(())
    }
}

impl Drop for ResidentShell {
    fn drop(&mut self) {
        // The worker owns no UI state and detaches safely if Explorer is hung. Stop its
        // native deadlines even if subclass validation/removal subsequently fails.
        self.guard.tray.borrow_mut().take();
        if self.guard.retry_due.take().is_some() && self.validate().is_ok() {
            // SAFETY: this non-Send guard is dropped on its host's owning UI thread.
            if unsafe { KillTimer(self.host, retry_timer_id(&self.guard)) } == 0 {
                crate::backend::diagnostics::error(
                    &native_error("Canceling the pending settings retry").to_string(),
                );
            }
        }
        if !self.guard.attached.get() {
            return;
        }
        if let Err(error) = self.validate() {
            eprintln!("Could not detach the resident close guard: {error}");
            return;
        }
        let data = Rc::as_ptr(&self.guard) as usize;
        // SAFETY: validate confirmed this exact subclass belongs to our live UI-thread host.
        if unsafe { RemoveWindowSubclass(self.host, Some(host_proc), data) } != 0 {
            self.guard.attached.set(false);
            // SAFETY: successful removal releases the subclass's sole owned reference.
            unsafe { drop(Rc::from_raw(data as *const CloseGuard)) };
        } else {
            // Keep the registration's reference alive if removal fails; its callback must
            // never observe freed state. WM_NCDESTROY will release that reference instead.
            eprintln!(
                "Could not remove the resident close guard: {}",
                native_error("RemoveWindowSubclass")
            );
        }
    }
}

fn retry_timer_id(guard: &Rc<CloseGuard>) -> usize {
    // CloseGuard's Rc data is aligned for pointer/Instant fields, leaving its low bit
    // available. Health uses the aligned address; retry uses that address with bit 0 set.
    Rc::as_ptr(guard) as usize | 1
}

fn timer_milliseconds(deadline: Instant) -> u32 {
    // Round up to whole milliseconds and respect SetTimer's 10 ms minimum. An
    // early tick must rearm the remaining time without moving the logical deadline.
    deadline
        .saturating_duration_since(Instant::now())
        .as_nanos()
        .div_ceil(1_000_000)
        .clamp(10, u32::MAX as u128) as u32
}

fn notify_tray(
    guard: &CloseGuard,
    event: impl FnOnce(&mut tray_monitor::TrayMonitor) -> Option<tray_monitor::Notification>,
) {
    let notification = guard.tray.borrow_mut().as_mut().and_then(event);
    // Drop the RefCell borrow before a callback can enqueue work or drop the adapter.
    if let Some(notification) = notification {
        notification.deliver();
    }
}

unsafe extern "system" fn host_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    let pointer = data as *const CloseGuard;
    // SAFETY: successful installation retains a strong Rc reference until removal; this
    // temporary reference keeps the state alive even if the adapter is dropped in a callback.
    unsafe { Rc::increment_strong_count(pointer) };
    // SAFETY: the increment above transferred one new strong reference to this local owner.
    let guard = unsafe { Rc::from_raw(pointer) };
    if message == WM_TIMER && wparam == data {
        notify_tray(&guard, tray_monitor::TrayMonitor::timer);
        return 0;
    }
    if message == guard.tray_completed && wparam == data {
        notify_tray(&guard, tray_monitor::TrayMonitor::completed);
        return 0;
    }
    if message == WM_TIMER && wparam == retry_timer_id(&guard) {
        if let Some(deadline) = guard.retry_due.get() {
            let now = Instant::now();
            let due = now >= deadline;
            let next = if due {
                now + Duration::from_secs(5)
            } else {
                deadline
            };
            // SAFETY: this active timer belongs to the exact UI-thread host/subclass.
            // A due retry restores the full period after any short remaining-time rearm.
            if unsafe { SetTimer(hwnd, wparam, timer_milliseconds(next), None) } == 0 {
                let error = native_error("Rescheduling a pending settings retry");
                // SAFETY: a failed replacement may leave our existing native timer active.
                if unsafe { KillTimer(hwnd, wparam) } != 0 {
                    guard.retry_due.set(None);
                } else {
                    // Retain the ownership marker so normal cancellation/Drop can retry.
                    crate::backend::diagnostics::error(
                        &native_error("Canceling a failed settings retry").to_string(),
                    );
                }
                crate::backend::diagnostics::error(&error.to_string());
                crate::backend::tray::wake_ui();
            } else {
                guard.retry_due.set(Some(next));
                if due {
                    crate::backend::tray::wake_ui();
                }
            }
        }
        return 0;
    }
    if (message == WM_POWERBROADCAST && wparam == PBT_APMPOWERSTATUSCHANGE as usize)
        || message == guard.taskbar_created
    {
        if message == guard.taskbar_created {
            notify_tray(&guard, tray_monitor::TrayMonitor::refresh);
        }
        crate::backend::tray::wake_ui();
    }
    if message == WM_APP + 1 && wparam == 1 && lparam as u32 == WM_RBUTTONUP {
        // The upstream window procedure has not run and therefore holds no framework
        // state borrow. The popup snapshots menu state and only queues application intents.
        if catch_unwind(AssertUnwindSafe(|| {
            crate::backend::tray::show_resident_menu(hwnd)
        }))
        .is_err()
        {
            eprintln!("The resident tray menu callback panicked.");
        }
        return 0;
    }
    if message == WM_CLOSE && !guard.quit_authorized.get() {
        if catch_unwind(AssertUnwindSafe(|| (guard.request_exit)())).is_err() {
            eprintln!("The resident exit-request callback panicked.");
        }
        return 0;
    }
    if message == WM_NCDESTROY && guard.attached.replace(false) {
        guard.tray.borrow_mut().take();
        if guard.retry_due.take().is_some() {
            // SAFETY: this is the owning host's final destruction notification.
            unsafe { KillTimer(hwnd, retry_timer_id(&guard)) };
        }
        // SAFETY: this is our active subclass's destruction notification on its owning thread.
        unsafe { RemoveWindowSubclass(hwnd, Some(host_proc), id) };
        // SAFETY: destruction ends the subclass lifetime and releases its registered reference.
        unsafe { drop(Rc::from_raw(pointer)) };
    }
    // SAFETY: preserve the remaining subclass chain and upstream's application teardown.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[derive(Default)]
struct ThreadWindows {
    hosts: Vec<HWND>,
    ui: Vec<HWND>,
    error: Option<ResidentShellError>,
}

fn thread_windows() -> Result<ThreadWindows, ResidentShellError> {
    let mut windows = ThreadWindows::default();
    // SAFETY: the callback synchronously borrows windows until enumeration returns, and
    // GetCurrentThreadId has no preconditions. Only this thread's native windows are visited.
    let succeeded = unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(collect_window),
            &mut windows as *mut ThreadWindows as isize,
        )
    };
    if let Some(error) = windows.error {
        return Err(error);
    }
    if succeeded == 0 {
        return Err(native_error("Enumerating resident windows"));
    }
    Ok(windows)
}

unsafe extern "system" fn collect_window(hwnd: HWND, data: LPARAM) -> i32 {
    // SAFETY: EnumThreadWindows receives this live, uniquely borrowed stack context.
    let windows = unsafe { &mut *(data as *mut ThreadWindows) };
    let mut process_id = 0;
    // SAFETY: process_id is writable and hwnd is the window being enumerated.
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
    // SAFETY: GetCurrentProcessId has no preconditions.
    let current_process = unsafe { GetCurrentProcessId() };
    // SAFETY: GetCurrentThreadId has no preconditions.
    let current_thread = unsafe { GetCurrentThreadId() };
    if process_id != current_process || thread_id != current_thread {
        return 1;
    }
    let class = match window_class(hwnd) {
        Ok(class) => class,
        Err(error) => {
            windows.error = Some(error);
            return 0;
        }
    };
    match class.as_str() {
        "WindUiAppHostClass" => windows.hosts.push(hwnd),
        "WindUiWindowClass" => windows.ui.push(hwnd),
        _ => {}
    }
    1
}

fn window_class(hwnd: HWND) -> Result<String, ResidentShellError> {
    let mut name = [0u16; 256];
    // SAFETY: name is a writable UTF-16 buffer with the supplied element capacity;
    // GetClassNameW queries the numeric handle and fails if it is no longer valid.
    let length = unsafe { GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32) };
    if length == 0 {
        return Err(native_error("Reading a resident window class"));
    }
    Ok(String::from_utf16_lossy(&name[..length as usize]))
}

fn visibility_for_windows(windows: &[HWND]) -> WindowVisibility {
    let mut state = WindowVisibility::Absent;
    for &hwnd in windows {
        // SAFETY: handles were enumerated on this same UI thread without dispatching messages.
        let visible = unsafe { IsWindowVisible(hwnd) } != 0;
        // SAFETY: hwnd is another still-live window on this UI thread.
        let minimized = unsafe { IsIconic(hwnd) } != 0;
        if !visible {
            if state == WindowVisibility::Absent {
                state = WindowVisibility::Hidden;
            }
        } else if minimized {
            state = WindowVisibility::Minimized;
        } else {
            return WindowVisibility::Visible;
        }
    }
    state
}

/// Exercise the normal close decision for this thread's own Wind UI windows.
pub(crate) fn close_ui_windows() -> Result<usize, ResidentShellError> {
    let windows = thread_windows()?.ui;
    for &hwnd in &windows {
        // SAFETY: enumeration selected only Wind UI windows owned by this process/UI thread.
        // The pointer-free close message runs after the current framework dispatch returns.
        if unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) } == 0 {
            return Err(native_error("Posting a UI window close"));
        }
    }
    Ok(windows.len())
}

fn native_error(operation: &'static str) -> ResidentShellError {
    // SAFETY: GetLastError reads this thread's Win32 error code without preconditions.
    let code = unsafe { GetLastError() };
    ResidentShellError::Native { operation, code }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetCapture;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, IsWindow, PeekMessageW, SendMessageW,
        ShowWindow, PM_NOREMOVE, SW_HIDE, SW_MINIMIZE, SW_SHOW, SW_SHOWNOACTIVATE,
        WS_OVERLAPPEDWINDOW,
    };

    fn pump_until(mut ready: impl FnMut() -> bool, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while !ready() {
            let mut message = MSG::default();
            // SAFETY: this test owns its UI thread and dispatches only messages removed
            // from that thread's queue; all pointer-bearing message storage remains live.
            while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0
            {
                // SAFETY: the removed message belongs to this test's native UI thread.
                unsafe { DispatchMessageW(&message) };
            }
            if ready() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "native event did not arrive in time"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn tray_query_finished(shell: &ResidentShell) -> bool {
        shell
            .guard
            .tray
            .borrow()
            .as_ref()
            .is_some_and(|monitor| !monitor.in_flight())
    }

    fn tray_timer_armed(shell: &ResidentShell) -> bool {
        shell
            .guard
            .tray
            .borrow()
            .as_ref()
            .is_some_and(tray_monitor::TrayMonitor::timer_armed)
    }

    struct TestWindow(HWND);

    impl TestWindow {
        fn new() -> Self {
            Self::with_class("STATIC")
        }

        fn with_class(name: &str) -> Self {
            let class = crate::win_util::wide_null(name);
            // SAFETY: create an owned, initially hidden window on this test's thread.
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    std::ptr::null(),
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                    160,
                    120,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            };
            assert!(!hwnd.is_null());
            Self(hwnd)
        }

        fn post_mouse(&self, message: u32, button: u16) {
            // SAFETY: this test owns the live UI-thread window; mouse messages carry no pointers.
            let posted = unsafe { PostMessageW(self.0, message, (button as usize) << 16, 0) };
            assert_ne!(posted, 0);
        }

        fn mouse_message(&self, remove: bool) -> MSG {
            let mut message = MSG::default();
            let flags = if remove { PM_REMOVE } else { PM_NOREMOVE };
            // SAFETY: message is writable and this test owns the window on the calling thread.
            let retrieved = unsafe {
                PeekMessageW(
                    &mut message,
                    self.0,
                    WM_XBUTTONDOWN,
                    WM_XBUTTONDBLCLK,
                    flags,
                )
            };
            assert_ne!(retrieved, 0);
            message
        }
    }

    impl Drop for TestWindow {
        fn drop(&mut self) {
            // SAFETY: the window belongs to this thread; an already destroyed handle is ignored.
            unsafe { DestroyWindow(self.0) };
        }
    }

    #[test]
    fn pointer_capture_query_requires_the_owned_ui_class_and_clears_on_destruction() {
        let window = TestWindow::new();
        assert!(!pointer_capture_for_class("Static"));
        // SAFETY: this thread owns the hidden test window; dropping it releases its capture.
        unsafe { SetCapture(window.0) };
        assert!(pointer_capture_for_class("Static"));
        assert!(!pointer_capture_for_class("Button"));
        assert!(!ui_pointer_capture_active());
        drop(window);
        assert!(!pointer_capture_for_class("Static"));
    }

    #[test]
    fn navigation_uses_removed_owned_messages_and_follows_recreated_windows() {
        let initial = TestWindow::new();
        assert_eq!(window_class(initial.0).as_deref(), Ok("Static"));
        let directions = Rc::new(RefCell::new(Vec::new()));
        let received = directions.clone();
        // The system class Static substitutes only the framework class name; the production identity/thread
        // filtering and native message hook are otherwise exercised unchanged.
        let listener = NavigationListener::start("Static", move |direction| {
            received.borrow_mut().push(direction);
        })
        .expect("attach the thread's navigation listener");
        assert!(matches!(
            NavigationListener::start("Static", |_| {}),
            Err(ResidentShellError::NavigationAlreadyAttached)
        ));
        drop(initial);

        for button in [XBUTTON1, XBUTTON2] {
            let window = TestWindow::new();
            window.post_mouse(WM_XBUTTONDOWN, button);
            let count = directions.borrow().len();
            assert_eq!(window.mouse_message(false).message, WM_XBUTTONDOWN);
            assert_eq!(directions.borrow().len(), count);
            assert_eq!(window.mouse_message(true).message, WM_NULL);
            assert_eq!(directions.borrow().len(), count + 1);
            window.post_mouse(WM_XBUTTONUP, button);
            assert_eq!(window.mouse_message(true).message, WM_NULL);
            assert_eq!(directions.borrow().len(), count + 1);
            // Dropping and recreating the UI window never replaces the listener.
        }
        assert_eq!(
            *directions.borrow(),
            [NavigationDirection::Back, NavigationDirection::Forward]
        );

        let unrelated = TestWindow::with_class("BUTTON");
        unrelated.post_mouse(WM_XBUTTONDOWN, XBUTTON1);
        assert_eq!(unrelated.mouse_message(true).message, WM_XBUTTONDOWN);
        assert_eq!(directions.borrow().len(), 2);

        drop(listener);
        let window = TestWindow::new();
        window.post_mouse(WM_XBUTTONDOWN, XBUTTON1);
        assert_eq!(window.mouse_message(true).message, WM_XBUTTONDOWN);
        assert_eq!(directions.borrow().len(), 2);
    }

    #[test]
    fn restore_listener_delivers_repeated_requests_and_releases_its_event_on_drop() {
        let name = format!(
            "Local\\Winderust.Tests.RestoreListener.{}.{:?}",
            std::process::id(),
            std::thread::current().id(),
        );
        let event = crate::SingleInstanceRestoreEvent::create_named(&name)
            .expect("create an isolated restore event");
        let (sent, received) = std::sync::mpsc::channel();
        let listener = RestoreListener::start(event, move || {
            sent.send(()).expect("the test receiver remains alive");
        })
        .expect("start an isolated native event listener");
        for _ in 0..2 {
            assert!(crate::SingleInstanceRestoreEvent::signal_named(&name));
            received
                .recv_timeout(std::time::Duration::from_secs(3))
                .expect("the blocked native listener delivers the request");
        }
        drop(listener);
        assert!(!crate::SingleInstanceRestoreEvent::signal_named(&name));
    }

    #[test]
    fn close_waits_for_authorization_and_then_forwards_upstream() {
        let window = TestWindow::new();
        let exits = Rc::new(Cell::new(0));
        let requested = exits.clone();
        let shell = ResidentShell::attach_to(window.0, move || requested.set(requested.get() + 1))
            .expect("attach close guard to the test window");
        // SAFETY: synchronously ask our owned test window to close.
        unsafe { SendMessageW(window.0, WM_CLOSE, 0, 0) };
        assert_eq!(exits.get(), 1);
        // SAFETY: query our owned native window.
        assert_ne!(unsafe { IsWindow(window.0) }, 0);
        shell.request_quit().expect("post an authorized close");
        let mut message = MSG::default();
        // SAFETY: retrieve only the posted close to our window and dispatch it on its owner thread.
        let received =
            unsafe { PeekMessageW(&mut message, window.0, WM_CLOSE, WM_CLOSE, PM_REMOVE) };
        assert_ne!(received, 0);
        // SAFETY: this message was retrieved for our live test window on its own thread.
        unsafe { DispatchMessageW(&message) };
        // SAFETY: IsWindow accepts an expired handle and returns false after native teardown.
        assert_eq!(unsafe { IsWindow(window.0) }, 0);
        assert_eq!(
            shell.request_quit(),
            Err(ResidentShellError::HostUnavailable)
        );
        assert_eq!(exits.get(), 1);
    }

    #[test]
    fn dropping_the_adapter_removes_its_close_interceptor() {
        let window = TestWindow::new();
        let shell = ResidentShell::attach_to(window.0, || panic!("detached callback ran"))
            .expect("attach close guard to the test window");
        drop(shell);
        // SAFETY: the guard is detached, so the owned test window receives normal default close.
        unsafe { SendMessageW(window.0, WM_CLOSE, 0, 0) };
        // SAFETY: IsWindow accepts an expired handle and returns false after native teardown.
        assert_eq!(unsafe { IsWindow(window.0) }, 0);
    }

    #[test]
    fn tray_probe_is_send_and_rejects_an_unrelated_native_window() {
        fn assert_send<T: Send>() {}
        assert_send::<TrayProbe>();
        let window = TestWindow::new();
        let shell = ResidentShell::attach_to(window.0, || {})
            .expect("attach close guard to the test window");
        let probe = shell.tray_probe();
        let result = std::thread::spawn(move || probe.is_registered())
            .join()
            .expect("the native tray probe worker must finish");
        assert_eq!(result, Err(ResidentShellError::HostUnavailable));
    }

    #[test]
    fn tray_health_suppresses_unchanged_results_and_reports_loss_and_recovery() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::Arc;

        let window = TestWindow::new();
        let mut shell = ResidentShell::attach_to(window.0, || {}).expect("attach native owner");
        let available = Arc::new(AtomicBool::new(true));
        let queries = Arc::new(AtomicUsize::new(0));
        let queried = Arc::clone(&queries);
        let desired = Arc::clone(&available);
        let notifications = Rc::new(RefCell::new(Vec::new()));
        let received = Rc::clone(&notifications);
        shell
            .listen_tray_query(
                move || {
                    queried.fetch_add(1, Ordering::SeqCst);
                    Ok(desired.load(Ordering::SeqCst))
                },
                move |result| received.borrow_mut().push(result),
            )
            .expect("start one health worker");
        pump_until(|| tray_query_finished(&shell), Duration::from_secs(3));
        assert_eq!(*notifications.borrow(), [Ok(true)]);
        assert!(
            !tray_timer_armed(&shell),
            "healthy state must sleep without a timer"
        );
        assert!(matches!(
            shell.listen_tray(|_| {}),
            Err(ResidentShellError::TrayAlreadyAttached)
        ));

        // A queued timer from an older deadline must not start another query early.
        let token = Rc::as_ptr(&shell.guard) as usize;
        // SAFETY: this test owns the live subclass and sends a pointer-free timer message.
        unsafe { SendMessageW(window.0, WM_TIMER, token, 0) };
        assert_eq!(queries.load(Ordering::SeqCst), 1);

        notify_tray(&shell.guard, tray_monitor::TrayMonitor::refresh);
        pump_until(|| tray_query_finished(&shell), Duration::from_secs(3));
        assert_eq!(queries.load(Ordering::SeqCst), 2);
        assert_eq!(*notifications.borrow(), [Ok(true)]);

        available.store(false, Ordering::SeqCst);
        notify_tray(&shell.guard, tray_monitor::TrayMonitor::refresh);
        pump_until(|| tray_query_finished(&shell), Duration::from_secs(3));
        assert!(tray_timer_armed(&shell), "unavailable state must retry");
        available.store(true, Ordering::SeqCst);
        notify_tray(&shell.guard, tray_monitor::TrayMonitor::refresh);
        pump_until(|| tray_query_finished(&shell), Duration::from_secs(3));
        assert_eq!(*notifications.borrow(), [Ok(true), Ok(false), Ok(true)]);
        assert_eq!(queries.load(Ordering::SeqCst), 4);
        assert!(!tray_timer_armed(&shell));

        shell
            .set_retry_pending(true)
            .expect("schedule conditional retry");
        assert!(shell.guard.retry_due.get().is_some());
        shell
            .set_retry_pending(false)
            .expect("cancel conditional retry");
        // SAFETY: the canceled timer's stale queued message contains no pointers.
        unsafe { SendMessageW(window.0, WM_TIMER, retry_timer_id(&shell.guard), 0) };
        assert!(shell.guard.retry_due.get().is_none());
    }

    #[test]
    fn tray_restart_during_a_query_requires_one_fresh_verification() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::{mpsc, Arc};

        let window = TestWindow::new();
        let mut shell = ResidentShell::attach_to(window.0, || {}).expect("attach native owner");
        let available = Arc::new(AtomicBool::new(true));
        let queries = Arc::new(AtomicUsize::new(0));
        let queried = Arc::clone(&queries);
        let desired = Arc::clone(&available);
        let (entered, started) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let notifications = Rc::new(RefCell::new(Vec::new()));
        let received = Rc::clone(&notifications);
        shell
            .listen_tray_query(
                move || {
                    let result = desired.load(Ordering::SeqCst);
                    if queried.fetch_add(1, Ordering::SeqCst) == 0 {
                        entered
                            .send(())
                            .map_err(|_| ResidentShellError::TrayWorkerUnavailable)?;
                        wait.recv()
                            .map_err(|_| ResidentShellError::TrayWorkerUnavailable)?;
                    }
                    Ok(result)
                },
                move |result| received.borrow_mut().push(result),
            )
            .expect("start verification");
        started
            .recv_timeout(Duration::from_secs(3))
            .expect("first query entered");
        available.store(false, Ordering::SeqCst);
        for _ in 0..3 {
            // SAFETY: the owned host receives a registered pointer-free restart event.
            unsafe { SendMessageW(window.0, shell.guard.taskbar_created, 0, 0) };
        }
        assert_eq!(queries.load(Ordering::SeqCst), 1);
        release.send(()).expect("release pre-restart query");
        pump_until(|| tray_query_finished(&shell), Duration::from_secs(3));
        assert_eq!(
            *notifications.borrow(),
            [Ok(false)],
            "old success cannot authorize hiding"
        );
        assert_eq!(queries.load(Ordering::SeqCst), 2, "restart events coalesce");
        assert!(tray_timer_armed(&shell));

        available.store(true, Ordering::SeqCst);
        shell.refresh_tray();
        pump_until(|| tray_query_finished(&shell), Duration::from_secs(3));
        assert_eq!(*notifications.borrow(), [Ok(false), Ok(true)]);
        assert!(!tray_timer_armed(&shell));
        let token = Rc::as_ptr(&shell.guard) as usize;
        // SAFETY: simulate the canceled retry's queued pointer-free timer message.
        unsafe { SendMessageW(window.0, WM_TIMER, token, 0) };
        assert_eq!(queries.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn settings_retry_preserves_early_deadline_then_restores_full_period() {
        let window = TestWindow::new();
        let shell = ResidentShell::attach_to(window.0, || {}).expect("attach native owner");
        shell.set_retry_pending(true).expect("start settings retry");
        let timer = retry_timer_id(&shell.guard);
        // Keep the physical five-second timer but bring the logical deadline close,
        // reproducing an early delivery without making this test wait five seconds.
        let deadline = Instant::now() + Duration::from_millis(60);
        shell.guard.retry_due.set(Some(deadline));
        // SAFETY: the owned subclass consumes this payload-free native timer message.
        unsafe { SendMessageW(window.0, WM_TIMER, timer, 0) };
        assert_eq!(shell.guard.retry_due.get(), Some(deadline));
        pump_until(
            || shell.guard.retry_due.get() != Some(deadline),
            Duration::from_secs(1),
        );
        let next = shell.guard.retry_due.get().expect("retry remains active");
        assert!(next.saturating_duration_since(Instant::now()) > Duration::from_secs(4));

        // The short remainder must not become the repeating native interval.
        std::thread::sleep(Duration::from_millis(120));
        let mut message = MSG::default();
        assert_eq!(
            // SAFETY: this test only peeks for timer messages belonging to its own window.
            unsafe { PeekMessageW(&mut message, window.0, WM_TIMER, WM_TIMER, PM_NOREMOVE) },
            0
        );
        shell
            .set_retry_pending(false)
            .expect("cancel settings retry");
        // SAFETY: this simulates a timer message queued before cancellation.
        unsafe { SendMessageW(window.0, WM_TIMER, timer, 0) };
        assert!(shell.guard.retry_due.get().is_none());
    }

    #[test]
    fn tray_health_timeout_keeps_one_query_and_recovers_from_its_late_result() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{mpsc, Arc};

        let window = TestWindow::new();
        let mut shell = ResidentShell::attach_to(window.0, || {}).expect("attach native owner");
        let (release, blocked) = mpsc::channel();
        let queries = Arc::new(AtomicUsize::new(0));
        let queried = Arc::clone(&queries);
        let notifications = Rc::new(RefCell::new(Vec::new()));
        let received = Rc::clone(&notifications);
        shell
            .listen_tray_query(
                move || {
                    if queried.fetch_add(1, Ordering::SeqCst) == 0 {
                        blocked
                            .recv()
                            .map_err(|_| ResidentShellError::TrayWorkerUnavailable)?;
                    }
                    Ok(true)
                },
                move |result| received.borrow_mut().push(result),
            )
            .expect("start one blocked health query");
        // Simulate an early/stale native delivery whose next periodic tick would
        // otherwise be much later than the logical query deadline. The handler
        // must preserve that deadline and rearm only the remaining duration.
        let timer = Rc::as_ptr(&shell.guard) as usize;
        // SAFETY: this test owns the exact UI-thread HWND and its health timer ID.
        assert_ne!(unsafe { SetTimer(window.0, timer, 10_000, None) }, 0);
        // SAFETY: the owned subclass consumes this payload-free native timer message.
        unsafe { SendMessageW(window.0, WM_TIMER, timer, 0) };
        assert!(notifications.borrow().is_empty());
        pump_until(
            || !notifications.borrow().is_empty(),
            Duration::from_secs(4),
        );
        assert_eq!(
            *notifications.borrow(),
            [Err(ResidentShellError::TrayProbeTimedOut)]
        );
        notify_tray(&shell.guard, tray_monitor::TrayMonitor::refresh);
        assert_eq!(queries.load(Ordering::SeqCst), 1);
        assert!(!tray_query_finished(&shell));

        release.send(()).expect("release the existing query");
        pump_until(|| notifications.borrow().len() == 2, Duration::from_secs(3));
        assert_eq!(
            *notifications.borrow(),
            [Err(ResidentShellError::TrayProbeTimedOut), Ok(true)]
        );
        assert_eq!(queries.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn dropping_tray_health_does_not_wait_for_a_blocked_query_or_deliver_its_result() {
        use std::sync::mpsc;

        let window = TestWindow::new();
        let mut shell = ResidentShell::attach_to(window.0, || {}).expect("attach native owner");
        let (release, blocked) = mpsc::channel();
        let (started, began) = mpsc::channel();
        let notifications = Rc::new(Cell::new(0));
        let received = Rc::clone(&notifications);
        shell
            .listen_tray_query(
                move || {
                    started
                        .send(())
                        .map_err(|_| ResidentShellError::TrayWorkerUnavailable)?;
                    blocked
                        .recv()
                        .map_err(|_| ResidentShellError::TrayWorkerUnavailable)?;
                    Ok(true)
                },
                move |_| received.set(received.get() + 1),
            )
            .expect("start a blocked health query");
        began
            .recv_timeout(Duration::from_secs(3))
            .expect("query started");
        let completion = shell.guard.tray_completed;
        let dropped_at = Instant::now();
        drop(shell);
        assert!(dropped_at.elapsed() < Duration::from_secs(1));
        release
            .send(())
            .expect("release the detached read-only query");
        let mut message = MSG::default();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            // SAFETY: retrieve only this worker's pointer-free completion message.
            if unsafe { PeekMessageW(&mut message, window.0, completion, completion, PM_REMOVE) }
                != 0
            {
                break;
            }
            assert!(Instant::now() < deadline, "late completion did not arrive");
            std::thread::sleep(Duration::from_millis(1));
        }
        // SAFETY: the old subclass was removed; the live test window ignores this late message.
        unsafe { DispatchMessageW(&message) };
        assert_eq!(notifications.get(), 0);
    }

    #[test]
    fn visibility_observation_follows_owned_window_lifecycle_and_ignores_other_classes() {
        let events = Rc::new(Cell::new(0));
        let received = Rc::clone(&events);
        let listener =
            VisibilityListener::start("Static", move || received.set(received.get() + 1))
                .expect("observe only this thread's test UI class");
        let window = TestWindow::new();
        // STARTUPINFO can override the process's first ShowWindow command. Consume
        // that override, then establish the hidden state before testing transitions.
        // SAFETY: this test owns the live window on this UI thread.
        unsafe { ShowWindow(window.0, SW_HIDE) };
        // SAFETY: this second call applies SW_HIDE even when the first was overridden.
        unsafe { ShowWindow(window.0, SW_HIDE) };
        let created = events.get();
        assert!(created > 0);
        // SAFETY: this test owns the live window on this UI thread.
        unsafe { ShowWindow(window.0, SW_SHOW) };
        assert!(events.get() > created);
        let shown = events.get();
        // SAFETY: this test owns the live window on this UI thread.
        unsafe { ShowWindow(window.0, SW_MINIMIZE) };
        assert!(events.get() > shown);
        let minimized = events.get();
        // SAFETY: this test owns the live window on this UI thread.
        unsafe { ShowWindow(window.0, SW_HIDE) };
        assert!(events.get() > minimized);
        let hidden = events.get();
        drop(window);
        assert!(events.get() > hidden);

        let count = events.get();
        let unrelated = TestWindow::with_class("BUTTON");
        // SAFETY: this test owns the unrelated-class window too.
        unsafe { ShowWindow(unrelated.0, SW_SHOW) };
        drop(unrelated);
        assert_eq!(events.get(), count);
        drop(listener);
        let _detached = TestWindow::new();
        assert_eq!(events.get(), count);
    }

    #[test]
    fn native_dialog_owner_excludes_the_message_host_and_unrelated_windows() {
        let window = TestWindow::new();
        let shell = ResidentShell::attach_to(window.0, || {})
            .expect("attach close guard to the test window");
        assert_eq!(shell.ui_window(), Ok(None));
        drop(shell);
        // SAFETY: query the test's owned native window without taking ownership.
        assert_ne!(unsafe { IsWindow(window.0) }, 0);
    }

    #[test]
    fn visibility_uses_native_visibility_and_minimization() {
        let window = TestWindow::new();
        assert_eq!(visibility_for_windows(&[]), WindowVisibility::Absent);
        assert_eq!(
            visibility_for_windows(&[window.0]),
            WindowVisibility::Hidden
        );
        // SAFETY: only change this test's owned window, without activating it.
        unsafe { ShowWindow(window.0, SW_SHOWNOACTIVATE) };
        assert_eq!(
            visibility_for_windows(&[window.0]),
            WindowVisibility::Visible
        );
        // SAFETY: minimize only this test's owned window.
        unsafe { ShowWindow(window.0, SW_MINIMIZE) };
        assert_eq!(
            visibility_for_windows(&[window.0]),
            WindowVisibility::Minimized
        );
        // SAFETY: hide only this test's owned window.
        unsafe { ShowWindow(window.0, SW_HIDE) };
        assert_eq!(
            visibility_for_windows(&[window.0]),
            WindowVisibility::Hidden
        );
    }
}
