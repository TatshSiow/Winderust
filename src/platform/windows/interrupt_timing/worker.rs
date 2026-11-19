//! A five-second, real-time ETW session owned exclusively by the diagnostic helper.

use std::{
    mem::{offset_of, size_of},
    sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use windows_sys::{
    core::GUID,
    Win32::{
        Foundation::{
            ERROR_ACCESS_DENIED, ERROR_ACTIVE_CONNECTIONS, ERROR_ALREADY_EXISTS, ERROR_MORE_DATA,
            ERROR_PRIVILEGE_NOT_HELD, ERROR_SUCCESS, ERROR_WMI_INSTANCE_NOT_FOUND,
        },
        System::{
            Diagnostics::Etw::{
                ControlTraceW, StartTraceW, CONTROLTRACE_HANDLE, EVENT_TRACE_CONTROL,
                EVENT_TRACE_CONTROL_QUERY, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_FLAG_DPC,
                EVENT_TRACE_FLAG_IMAGE_LOAD, EVENT_TRACE_FLAG_INTERRUPT,
                EVENT_TRACE_FLAG_NO_SYSCONFIG, EVENT_TRACE_INDEPENDENT_SESSION_MODE,
                EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE, EVENT_TRACE_SYSTEM_LOGGER_MODE,
                WNODE_FLAG_TRACED_GUID, WNODE_HEADER,
            },
            Performance::QueryPerformanceCounter,
        },
    },
};

use super::{decode::decode_live, InterruptTimingError, Outcome};

const SESSION_GUID: GUID = GUID::from_u128(0xacd9dac4_48da_4f73_a66d_87e51252a93c);
const WINDOW: Duration = Duration::from_secs(5);
const STARTUP_LIMIT: Duration = Duration::from_secs(2);
const DRAIN_LIMIT: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(10);

pub(super) fn capture(name: &str, cancelled: &Receiver<()>) -> Outcome {
    if stopping(cancelled) {
        return Err(InterruptTimingError::Cancelled);
    }
    let (mut session, start_tick) = Session::start(name)?;
    let started = Instant::now();
    let (ready_sender, ready) = mpsc::channel();
    let (done_sender, done) = mpsc::channel();
    let consumer_name = name.to_owned();
    let consumer = thread::Builder::new()
        .name("interrupt-timing-consumer".into())
        .spawn(move || {
            let _ = done_sender.send(decode_live(&consumer_name, ready_sender, start_tick));
        });
    if let Err(error) = consumer {
        session.stop()?;
        return Err(capture_error(error));
    }

    let mut attached = false;
    let was_cancelled = loop {
        if stopping(cancelled) {
            break Ok(true);
        }
        let elapsed = started.elapsed();
        let Some(remaining) = WINDOW.checked_sub(elapsed) else {
            break Ok(false);
        };
        attached |= ready.try_recv().is_ok();
        if !attached && elapsed >= STARTUP_LIMIT {
            break Err(capture_error("ETW consumer did not attach."));
        }
        match done.recv_timeout(remaining.min(POLL)) {
            Ok(Err(error)) => break Err(capture_error(error)),
            Ok(Ok(_)) => {
                break Err(capture_error(
                    "ETW consumer ended before the capture stopped.",
                ))
            }
            Err(RecvTimeoutError::Disconnected) => {
                break Err(capture_error("ETW consumer exited unexpectedly."));
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    };

    let statistics = session.stop()?;
    let was_cancelled = was_cancelled?;
    let decoded = done.recv_timeout(DRAIN_LIMIT).map_err(capture_error)?;
    if was_cancelled || stopping(cancelled) {
        return Err(InterruptTimingError::Cancelled);
    }
    let mut report = decoded.map_err(capture_error)?;
    report.events_lost = report.events_lost.max(statistics.EventsLost);
    report.buffers_lost = report.buffers_lost.max(
        statistics
            .LogBuffersLost
            .saturating_add(statistics.RealTimeBuffersLost),
    );
    Ok(report)
}

fn stopping(cancelled: &Receiver<()>) -> bool {
    !matches!(cancelled.try_recv(), Err(TryRecvError::Empty))
}

#[repr(C)]
struct Properties {
    trace: EVENT_TRACE_PROPERTIES,
    logger_name: [u16; 1025],
    log_file_name: [u16; 1025],
}

impl Properties {
    fn new() -> Self {
        Self {
            trace: EVENT_TRACE_PROPERTIES {
                Wnode: WNODE_HEADER {
                    BufferSize: size_of::<Self>() as u32,
                    Flags: WNODE_FLAG_TRACED_GUID,
                    ..WNODE_HEADER::default()
                },
                LoggerNameOffset: offset_of!(Self, logger_name) as u32,
                LogFileNameOffset: offset_of!(Self, log_file_name) as u32,
                ..EVENT_TRACE_PROPERTIES::default()
            },
            logger_name: [0; 1025],
            log_file_name: [0; 1025],
        }
    }

    fn control(&mut self, name: &[u16], operation: EVENT_TRACE_CONTROL) -> u32 {
        // SAFETY: name is UTF-16 with a trailing NUL and this buffer has space for both names.
        unsafe {
            ControlTraceW(
                CONTROLTRACE_HANDLE::default(),
                name.as_ptr(),
                (self as *mut Self).cast::<EVENT_TRACE_PROPERTIES>(),
                operation,
            )
        }
    }

    fn matches_identity(&self, name: &[u16]) -> bool {
        let guid = self.trace.Wnode.Guid;
        self.logger_name.starts_with(name)
            && guid.data1 == SESSION_GUID.data1
            && guid.data2 == SESSION_GUID.data2
            && guid.data3 == SESSION_GUID.data3
            && guid.data4 == SESSION_GUID.data4
    }
}

pub(super) fn stop_owned(
    name: &str,
) -> Result<Option<EVENT_TRACE_PROPERTIES>, InterruptTimingError> {
    let name = name.encode_utf16().chain([0]).collect::<Vec<_>>();
    let mut current = Properties::new();
    match current.control(&name, EVENT_TRACE_CONTROL_QUERY) {
        ERROR_WMI_INSTANCE_NOT_FOUND => return Ok(None),
        ERROR_SUCCESS if current.matches_identity(&name) => {}
        ERROR_SUCCESS => {
            return Err(capture_error(
                "The interrupt timing ETW session identity changed.",
            ))
        }
        code => {
            return Err(capture_error(format!(
                "ControlTraceW query failed with Windows error {code}."
            )))
        }
    }

    let mut statistics = Properties::new();
    // The exact capture name is not reused, even if Windows recycles a logger handle.
    match statistics.control(&name, EVENT_TRACE_CONTROL_STOP) {
        ERROR_SUCCESS => Ok(Some(statistics.trace)),
        ERROR_MORE_DATA | ERROR_WMI_INSTANCE_NOT_FOUND | ERROR_ACTIVE_CONNECTIONS => Ok(None),
        code => Err(capture_error(format!(
            "ControlTraceW stop failed with Windows error {code}."
        ))),
    }
}

struct Session<'a> {
    name: &'a str,
    active: bool,
}

impl<'a> Session<'a> {
    fn start(name: &'a str) -> Result<(Self, i64), InterruptTimingError> {
        let mut properties = Properties::new();
        properties.trace.Wnode.Guid = SESSION_GUID;
        properties.trace.Wnode.ClientContext = 1; // QueryPerformanceCounter timestamps.
        properties.trace.LogFileNameOffset = 0;
        properties.trace.LogFileMode = EVENT_TRACE_SYSTEM_LOGGER_MODE
            | EVENT_TRACE_REAL_TIME_MODE
            | EVENT_TRACE_INDEPENDENT_SESSION_MODE;
        properties.trace.BufferSize = 64;
        properties.trace.MinimumBuffers = 64;
        properties.trace.MaximumBuffers = 256;
        properties.trace.FlushTimer = 1;
        properties.trace.EnableFlags = EVENT_TRACE_FLAG_DPC
            | EVENT_TRACE_FLAG_INTERRUPT
            | EVENT_TRACE_FLAG_IMAGE_LOAD
            | EVENT_TRACE_FLAG_NO_SYSCONFIG;
        let wide = name.encode_utf16().chain([0]).collect::<Vec<_>>();
        let mut start_tick = 0;
        // SAFETY: start_tick is a valid writable i64 for the synchronous call.
        if unsafe { QueryPerformanceCounter(&raw mut start_tick) } == 0 {
            return Err(capture_error(std::io::Error::last_os_error()));
        }
        let mut handle = CONTROLTRACE_HANDLE::default();
        // SAFETY: start takes a writable handle, a NUL-terminated UTF-16 name,
        // and a properties buffer with trailing string space at its advertised offsets.
        let code = unsafe {
            StartTraceW(
                &raw mut handle,
                wide.as_ptr(),
                (&raw mut properties).cast::<EVENT_TRACE_PROPERTIES>(),
            )
        };
        match code {
            ERROR_SUCCESS => Ok((Self { name, active: true }, start_tick)),
            ERROR_ALREADY_EXISTS => Err(InterruptTimingError::InUse),
            ERROR_ACCESS_DENIED | ERROR_PRIVILEGE_NOT_HELD => {
                Err(InterruptTimingError::AccessDenied)
            }
            code => Err(capture_error(format!(
                "StartTraceW failed with Windows error {code}."
            ))),
        }
    }

    fn stop(&mut self) -> Result<EVENT_TRACE_PROPERTIES, InterruptTimingError> {
        let statistics = stop_owned(self.name)?;
        self.active = false;
        statistics.ok_or_else(|| {
            capture_error("The ETW session stopped before statistics were available.")
        })
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        if self.active {
            let _ = self.stop();
        }
    }
}

fn capture_error(error: impl std::fmt::Display) -> InterruptTimingError {
    InterruptTimingError::Capture(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_parent_signal_requests_a_stop() {
        let (sender, receiver) = mpsc::channel();
        assert!(!stopping(&receiver));
        drop(sender);
        assert!(stopping(&receiver));
    }

    #[test]
    fn cleanup_requires_the_exact_returned_name_and_logger_guid() {
        let exact = "Winderust Interrupt Timing 00000000000000000000000000000001"
            .encode_utf16()
            .chain([0])
            .collect::<Vec<_>>();
        let other = "Winderust Interrupt Timing 00000000000000000000000000000002"
            .encode_utf16()
            .chain([0])
            .collect::<Vec<_>>();
        let mut properties = Properties::new();
        properties.logger_name[..exact.len()].copy_from_slice(&exact);
        assert!(!properties.matches_identity(&exact));
        properties.trace.Wnode.Guid = SESSION_GUID;
        assert!(properties.matches_identity(&exact));
        assert!(!properties.matches_identity(&other));
    }
}
