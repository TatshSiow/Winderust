//! Event-driven tray verification with deadlines and retries only while unavailable.
use super::{native_error, timer_milliseconds, ResidentShellError};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    rc::Rc,
    sync::{mpsc, Arc, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{
        Shell::GetWindowSubclass,
        WindowsAndMessaging::{GetWindowThreadProcessId, KillTimer, PostMessageW, SetTimer},
    },
};

const RETRY_INTERVAL: Duration = Duration::from_secs(5);
const QUERY_TIMEOUT: Duration = Duration::from_secs(2);
type QueryResult = Result<bool, ResidentShellError>;
type Completion = Arc<Mutex<Option<QueryResult>>>;

pub(super) struct Notification {
    notify: Rc<dyn Fn(QueryResult)>,
    result: QueryResult,
}

impl Notification {
    pub(super) fn deliver(self) {
        if catch_unwind(AssertUnwindSafe(|| (self.notify)(self.result))).is_err() {
            crate::backend::diagnostics::error("The tray-health callback panicked.");
        }
    }
}

pub(super) struct TrayMonitor {
    host: HWND,
    timer: usize,
    requests: Option<mpsc::Sender<()>>,
    worker: Option<JoinHandle<()>>,
    completion: Completion,
    notify: Rc<dyn Fn(QueryResult)>,
    deadline: Option<Instant>,
    timer_armed: bool,
    in_flight: bool,
    refresh_pending: bool,
    reported: Option<bool>,
}

impl TrayMonitor {
    #[cfg(test)]
    pub(super) fn in_flight(&self) -> bool {
        self.in_flight
    }

    #[cfg(test)]
    pub(super) fn timer_armed(&self) -> bool {
        self.timer_armed
    }

    pub(super) fn start(
        host: HWND,
        timer: usize,
        message: u32,
        process_id: u32,
        thread_id: u32,
        query: impl Fn() -> QueryResult + Send + 'static,
        notify: impl Fn(QueryResult) + 'static,
    ) -> Result<Self, ResidentShellError> {
        let completion: Completion = Arc::new(Mutex::new(None));
        let (requests, incoming) = mpsc::channel();
        let completed = Arc::clone(&completion);
        let host_raw = host as usize;
        let worker = std::thread::Builder::new()
            .name("winderust-tray-health".into())
            .spawn(move || {
                while incoming.recv().is_ok() {
                    let result = catch_unwind(AssertUnwindSafe(&query))
                        .unwrap_or(Err(ResidentShellError::TrayWorkerUnavailable));
                    // The potentially blocking Explorer query is finished before this lock.
                    // Holding it across PostMessage ensures a failed post is visible before
                    // the UI can consume the result through its timeout fallback.
                    let mut slot = completed
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    *slot = Some(result);
                    let mut current_process = 0;
                    // SAFETY: the numeric HWND is borrowed; this query only writes the PID.
                    let current_thread =
                        unsafe { GetWindowThreadProcessId(host_raw as HWND, &mut current_process) };
                    if current_process != process_id || current_thread != thread_id {
                        *slot = Some(Err(ResidentShellError::HostUnavailable));
                        continue;
                    }
                    // SAFETY: no pointers cross threads. The receiver checks the current
                    // subclass token and consumes only its own private completion slot, so
                    // a late message cannot affect a replacement host/monitor.
                    if unsafe { PostMessageW(host_raw as HWND, message, timer, 0) } == 0 {
                        *slot = Some(Err(native_error("Posting the tray-health result")));
                    }
                }
            })
            .map_err(|error| ResidentShellError::TrayListenerStart {
                code: error.raw_os_error(),
            })?;
        let mut monitor = Self {
            host,
            timer,
            requests: Some(requests),
            worker: Some(worker),
            completion,
            notify: Rc::new(notify),
            deadline: None,
            timer_armed: false,
            in_flight: false,
            refresh_pending: false,
            reported: None,
        };
        monitor.start_query()?;
        Ok(monitor)
    }

    fn arm(&mut self, delay: Duration) -> Result<(), ResidentShellError> {
        self.arm_until(Instant::now() + delay)
    }

    fn arm_until(&mut self, deadline: Instant) -> Result<(), ResidentShellError> {
        // SAFETY: the UI-thread owner retains this exact host and unique timer ID. No
        // TIMERPROC is installed; ResidentShell consumes the resulting WM_TIMER itself.
        if unsafe { SetTimer(self.host, self.timer, timer_milliseconds(deadline), None) } == 0 {
            return Err(native_error("Scheduling tray-health maintenance"));
        }
        self.deadline = Some(deadline);
        self.timer_armed = true;
        Ok(())
    }

    fn disarm(&mut self) -> Result<(), ResidentShellError> {
        self.deadline = None;
        if !self.timer_armed {
            return Ok(());
        }
        let mut data = 0;
        // SAFETY: this read-only UI-thread lookup verifies our exact subclass token;
        // an invalid/reused HWND must never cancel another window's native timer.
        if unsafe { GetWindowSubclass(self.host, Some(super::host_proc), self.timer, &mut data) }
            == 0
            || data != self.timer
        {
            self.timer_armed = false;
            return Err(ResidentShellError::HostUnavailable);
        }
        // SAFETY: this timer belongs to this monitor's UI-thread host/ID pair.
        if unsafe { KillTimer(self.host, self.timer) } == 0 {
            return Err(native_error("Canceling tray-health maintenance"));
        }
        self.timer_armed = false;
        Ok(())
    }

    fn start_query(&mut self) -> Result<(), ResidentShellError> {
        if self.in_flight {
            return Ok(());
        }
        self.arm(QUERY_TIMEOUT)?;
        self.in_flight = true;
        if self
            .requests
            .as_ref()
            .is_none_or(|requests| requests.send(()).is_err())
        {
            self.in_flight = false;
            self.disarm()?;
            return Err(ResidentShellError::TrayWorkerUnavailable);
        }
        Ok(())
    }

    fn notification(&mut self, result: QueryResult) -> Option<Notification> {
        if let Ok(available) = result {
            if self.reported == Some(available) {
                return None;
            }
            self.reported = Some(available);
        } else {
            self.reported = None;
        }
        Some(Notification {
            notify: Rc::clone(&self.notify),
            result,
        })
    }

    pub(super) fn completed(&mut self) -> Option<Notification> {
        let result = self
            .completion
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()?;
        self.in_flight = false;
        if self.refresh_pending {
            self.refresh_pending = false;
            // An Explorer/window event can invalidate a query that was already in
            // progress. Coalesce those events into one fresh verification.
            return self
                .disarm()
                .and_then(|()| self.start_query())
                .err()
                .and_then(|error| self.notification(Err(error)));
        }
        let result = match self.disarm().and_then(|()| {
            if result == Ok(true) {
                Ok(())
            } else {
                self.arm(RETRY_INTERVAL)
            }
        }) {
            Ok(()) => result,
            Err(error) => Err(error),
        };
        self.notification(result)
    }

    pub(super) fn timer(&mut self) -> Option<Notification> {
        // KillTimer does not remove already queued WM_TIMER messages. Checking our
        // current deadline prevents an old message from expiring a newly armed query.
        let deadline = self.deadline?;
        if Instant::now() < deadline {
            // Native timers can tick before the higher-resolution Instant deadline.
            // Do not wait another complete periodic interval, or extend the deadline
            // when an old queued WM_TIMER arrives after a phase change.
            return self
                .arm_until(deadline)
                .err()
                .and_then(|error| self.notification(Err(error)));
        }
        if self.in_flight {
            let notification = self.completed();
            // An unchanged healthy completion may have no notification or timer.
            // A pending refresh may instead have started a new bounded query.
            if !self.in_flight || self.deadline != Some(deadline) {
                return notification;
            }
        }
        if let Err(error) = self.disarm() {
            return self.notification(Err(error));
        }
        if self.in_flight {
            // The single query may still be blocked in Explorer. Keep it in flight and
            // wait for its eventual completion; do not spawn or queue another query.
            self.notification(Err(ResidentShellError::TrayProbeTimedOut))
        } else if let Err(error) = self.start_query() {
            let result = self.arm(RETRY_INTERVAL).err().unwrap_or(error);
            self.notification(Err(result))
        } else {
            None
        }
    }

    pub(super) fn refresh(&mut self) -> Option<Notification> {
        if self.in_flight {
            self.refresh_pending = true;
            return self.completed();
        }
        if let Err(error) = self.start_query() {
            self.notification(Err(error))
        } else {
            None
        }
    }
}

impl Drop for TrayMonitor {
    fn drop(&mut self) {
        if let Err(error) = self.disarm() {
            crate::backend::diagnostics::error(&error.to_string());
        }
        self.requests.take();
        if let Some(worker) = self.worker.take() {
            // Explorer may be hung inside its read-only query. Dropping the JoinHandle
            // detaches that one worker; it owns no UI state or native resources to restore.
            if worker.is_finished() && worker.join().is_err() {
                crate::backend::diagnostics::error("The tray-health worker panicked.");
            }
        }
    }
}
