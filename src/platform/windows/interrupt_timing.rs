//! Owns the short-lived helper used by the on-demand interrupt diagnostic.

use std::{
    io::{self, Read, Write},
    num::NonZeroU128,
    os::windows::process::CommandExt,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU32, Ordering::Relaxed},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use windows::Win32::System::Com::CoCreateGuid;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

mod decode;
mod worker;

pub(crate) use decode::{InterruptKind, InterruptTimingReport};

const WORKER_ARGUMENT: &str = "--winderust-interrupt-timing-worker";
const SESSION_PREFIX: &str = "Winderust Interrupt Timing";
const START: u8 = 1;
const STARTUP_RECORD_LEN: usize = 17;
const STARTING: u32 = u32::MAX;
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;
static WORKER_PROCESS_ID: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub(crate) enum InterruptTimingError {
    #[error("Windows denied access to the interrupt timing session.")]
    AccessDenied,
    #[error("An interrupt timing capture is already running.")]
    InUse,
    #[error("Interrupt timing capture was cancelled.")]
    Cancelled,
    #[error("{0}")]
    Worker(String),
    #[error("{0}")]
    Capture(String),
}

type Outcome = Result<InterruptTimingReport, InterruptTimingError>;

#[derive(Debug)]
pub(crate) struct InterruptTimingCapture {
    cancel_pipe: Option<ChildStdin>,
    outcome: Option<Receiver<Outcome>>,
}

impl InterruptTimingCapture {
    pub(crate) fn start() -> Result<Self, InterruptTimingError> {
        let reservation = WorkerReservation::acquire()?;
        let identity = CaptureIdentity::new()?;
        let executable = std::env::current_exe().map_err(worker_error)?;
        let mut child = Command::new(executable)
            .arg(WORKER_ARGUMENT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(worker_error)?;
        WORKER_PROCESS_ID.store(child.id(), Relaxed);
        let mut cancel_pipe = child.stdin.take().ok_or_else(|| {
            InterruptTimingError::Worker("Interrupt timing worker stdin is unavailable.".into())
        })?;
        let (sender, outcome) = mpsc::channel();
        thread::Builder::new()
            .name("interrupt-timing-worker-reaper".into())
            .spawn(move || {
                let result = receive_worker_outcome(child, identity);
                drop(reservation);
                let _ = sender.send(result);
            })
            .map_err(worker_error)?;
        cancel_pipe
            .write_all(&identity.startup_record())
            .map_err(worker_error)?;
        Ok(Self {
            cancel_pipe: Some(cancel_pipe),
            outcome: Some(outcome),
        })
    }

    pub(crate) fn cancel(&mut self) {
        self.cancel_pipe.take();
    }

    pub(crate) fn poll(&mut self) -> Option<Outcome> {
        let result = match self.outcome.as_ref()?.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(InterruptTimingError::Worker(
                "Interrupt timing worker exited without a report.".into(),
            )),
        };
        self.outcome.take();
        self.cancel();
        Some(result)
    }

    pub(crate) fn finish(mut self) -> Result<(), InterruptTimingError> {
        self.cancel();
        let Some(outcome) = self.outcome.take() else {
            return Ok(());
        };
        match outcome.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(_) | Err(InterruptTimingError::Cancelled)) => Ok(()),
            Ok(Err(error)) => Err(error),
            Err(error) => Err(worker_error(error)),
        }
    }
}

#[derive(Clone, Copy)]
struct CaptureIdentity(NonZeroU128);

impl CaptureIdentity {
    fn new() -> Result<Self, InterruptTimingError> {
        // SAFETY: CoCreateGuid writes to storage owned by the Windows binding.
        let guid = unsafe { CoCreateGuid() }.map_err(worker_error)?;
        NonZeroU128::new(guid.to_u128())
            .map(Self)
            .ok_or_else(|| worker_error("Windows generated an empty capture identity."))
    }

    fn startup_record(self) -> [u8; STARTUP_RECORD_LEN] {
        let mut record = [START; STARTUP_RECORD_LEN];
        record[1..].copy_from_slice(&self.0.get().to_be_bytes());
        record
    }

    fn read(mut input: impl Read) -> Option<Self> {
        let mut record = [0; STARTUP_RECORD_LEN];
        input.read_exact(&mut record).ok()?;
        let [marker, id @ ..] = record;
        if marker != START {
            return None;
        }
        NonZeroU128::new(u128::from_be_bytes(id)).map(Self)
    }

    fn name(self) -> String {
        format!("{SESSION_PREFIX} {:032x}", self.0)
    }
}

struct WorkerReservation;

impl WorkerReservation {
    fn acquire() -> Result<Self, InterruptTimingError> {
        WORKER_PROCESS_ID
            .compare_exchange(0, STARTING, Relaxed, Relaxed)
            .map(|_| Self)
            .map_err(|_| InterruptTimingError::InUse)
    }
}

impl Drop for WorkerReservation {
    fn drop(&mut self) {
        WORKER_PROCESS_ID.store(0, Relaxed);
    }
}

pub(crate) fn is_worker_process(pid: u32) -> bool {
    pid != 0 && pid != STARTING && pid == WORKER_PROCESS_ID.load(Relaxed)
}

pub(crate) fn run_worker_if_requested() -> bool {
    if !std::env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == WORKER_ARGUMENT)
    {
        return false;
    }
    let outcome = run_worker();
    // The pipe can close if the parent exited. The worker has already stopped its session.
    let mut output = io::stdout().lock();
    if serde_json::to_writer(&mut output, &outcome).is_ok() {
        let _ = output.flush();
    }
    true
}

fn run_worker() -> Outcome {
    let identity = CaptureIdentity::read(io::stdin()).ok_or(InterruptTimingError::Cancelled)?;
    let (sender, cancelled) = mpsc::channel();
    thread::Builder::new()
        .name("interrupt-timing-parent-lifetime".into())
        .spawn(move || {
            // Both explicit cancellation and parent termination close the only write end.
            let _ = io::stdin().read(&mut [0]);
            let _ = sender.send(());
        })
        .map_err(worker_error)?;
    worker::capture(&identity.name(), &cancelled)
}

fn receive_worker_outcome(mut child: Child, identity: CaptureIdentity) -> Outcome {
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or_else(|| {
            InterruptTimingError::Worker("Interrupt timing worker stdout is unavailable.".into())
        })
        .and_then(|pipe| {
            pipe.take(MAX_RESPONSE_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(worker_error)
        });
    let status = child.wait().map_err(worker_error)?;
    let outcome = read.and_then(|_| {
        if status.success() && bytes.len() as u64 <= MAX_RESPONSE_BYTES {
            return serde_json::from_slice::<Outcome>(&bytes).map_err(worker_error)?;
        }
        Err(InterruptTimingError::Worker(format!(
            "Interrupt timing worker returned an invalid report ({status})."
        )))
    });
    match outcome {
        Err(error @ (InterruptTimingError::Worker(_) | InterruptTimingError::Capture(_))) => {
            worker::stop_owned(&identity.name()).map_err(|cleanup| {
                worker_error(format!("{error} ETW cleanup failed: {cleanup}"))
            })?;
            Err(error)
        }
        outcome => outcome,
    }
}

fn worker_error(error: impl std::fmt::Display) -> InterruptTimingError {
    InterruptTimingError::Worker(format!("Interrupt timing worker: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_record_selects_the_same_unique_name_in_both_processes() {
        let identity = CaptureIdentity(NonZeroU128::new(42).unwrap());
        let record = identity.startup_record();
        assert_eq!(
            CaptureIdentity::read(record.as_slice()).unwrap().name(),
            "Winderust Interrupt Timing 0000000000000000000000000000002a"
        );
        assert!(CaptureIdentity::read(&record[..1]).is_none());
        let mut empty_id = [0; STARTUP_RECORD_LEN];
        empty_id[0] = START;
        assert!(CaptureIdentity::read(empty_id.as_slice()).is_none());
    }

    #[test]
    fn capture_reservation_excludes_only_the_registered_process() {
        let reservation = WorkerReservation::acquire().unwrap();
        assert!(matches!(
            WorkerReservation::acquire(),
            Err(InterruptTimingError::InUse)
        ));
        assert!(!is_worker_process(STARTING));

        let worker = std::process::id();
        WORKER_PROCESS_ID.store(worker, Relaxed);
        assert!(is_worker_process(worker));
        assert!(!is_worker_process(0));
        drop(reservation);
        assert!(!is_worker_process(worker));
    }
}
