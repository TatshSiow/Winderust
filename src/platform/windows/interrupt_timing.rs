//! Owns the short-lived helper used by the on-demand interrupt diagnostic.

use std::{
    io::{self, Read, Write},
    num::NonZeroU128,
    os::windows::process::CommandExt,
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicU32, Ordering::Relaxed},
        mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use windows::Win32::System::Com::CoCreateGuid;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

mod decode;
mod lease;
mod worker;

pub(crate) use decode::{InterruptKind, InterruptTimingReport};
use lease::LeaseGuard;

const WORKER_ARGUMENT: &str = "--winderust-interrupt-timing-worker";
const SESSION_PREFIX: &str = "Winderust Interrupt Timing";
const START: u8 = 1;
const STARTUP_RECORD_LEN: usize = 17;
const STARTING: u32 = u32::MAX;
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;
const CAPTURE_LIMIT: Duration = Duration::from_secs(12);
const CANCEL_LIMIT: Duration = Duration::from_secs(4);
const CLEANUP_GRACE: Duration = Duration::from_secs(1);
const WORKER_POLL: Duration = Duration::from_millis(20);
const EXIT_CONFIRMATION_POLL: Duration = Duration::from_millis(50);
static WORKER_PROCESS_ID: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub(crate) enum InterruptTimingError {
    #[error("Windows denied access to the interrupt timing session.")]
    AccessDenied,
    #[error("An interrupt timing capture is already running.")]
    InUse,
    #[error("Interrupt timing capture was cancelled.")]
    Cancelled,
    #[error("Interrupt timing helper timed out: {0}")]
    TimedOut(String),
    #[error("{0}")]
    Cleanup(String),
    #[error("{0}")]
    Worker(String),
    #[error("{0}")]
    Capture(String),
}

type Outcome = Result<InterruptTimingReport, InterruptTimingError>;

pub(crate) enum CaptureUpdate {
    CleanupPending(InterruptTimingError),
    Finished(Outcome),
}

#[derive(Debug)]
pub(crate) struct InterruptTimingCapture {
    cancel_signal: Option<Sender<()>>,
    outcome: Option<Receiver<Outcome>>,
    deadline: Instant,
    cleanup_pending: bool,
}

impl InterruptTimingCapture {
    pub(crate) fn start() -> Result<Self, InterruptTimingError> {
        let reservation = WorkerReservation::acquire()?;
        let identity = CaptureIdentity::new()?;
        let deadline = Instant::now() + CAPTURE_LIMIT;
        let (cancel_signal, cancelled) = mpsc::channel();
        let (sender, outcome) = mpsc::channel();
        thread::Builder::new()
            .name("interrupt-timing-worker-reaper".into())
            .spawn(move || {
                let result = supervise_worker(identity, &cancelled, deadline);
                drop(reservation);
                if let Err(mpsc::SendError(Err(error))) = sender.send(result) {
                    crate::backend::diagnostics::error(&format!(
                        "Interrupt Timing failed with {error}"
                    ));
                }
            })
            .map_err(worker_error)?;
        Ok(Self {
            cancel_signal: Some(cancel_signal),
            outcome: Some(outcome),
            deadline: deadline + CLEANUP_GRACE,
            cleanup_pending: false,
        })
    }

    pub(crate) fn cancel(&mut self) {
        if self.cancel_signal.take().is_some() {
            self.deadline = self
                .deadline
                .min(Instant::now() + CANCEL_LIMIT + CLEANUP_GRACE);
        }
    }

    pub(crate) fn poll(&mut self) -> Option<CaptureUpdate> {
        let result = match self.outcome.as_ref()?.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => {
                if self.cleanup_pending || Instant::now() < self.deadline {
                    return None;
                }
                self.cleanup_pending = true;
                self.cancel();
                return Some(CaptureUpdate::CleanupPending(pending_timeout()));
            }
            Err(error) => Err(worker_error(error)),
        };
        self.outcome.take();
        self.cancel();
        Some(CaptureUpdate::Finished(result))
    }

    pub(crate) fn finish(self) -> Result<(), InterruptTimingError> {
        self.finish_with_grace(Duration::ZERO).map(|_| ())
    }

    #[cfg(feature = "runtime-benchmark")]
    pub(crate) fn finish_with_report(
        self,
    ) -> Result<Option<InterruptTimingReport>, InterruptTimingError> {
        // Give the reaper a bounded chance to confirm cleanup after a capture timeout.
        self.finish_with_grace(CLEANUP_GRACE)
    }

    fn finish_with_grace(
        mut self,
        grace: Duration,
    ) -> Result<Option<InterruptTimingReport>, InterruptTimingError> {
        self.cancel();
        let Some(outcome) = self.outcome.take() else {
            return Ok(None);
        };
        let deadline = self.deadline + grace;
        match outcome.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(report)) => Ok(Some(report)),
            Ok(Err(InterruptTimingError::Cancelled)) => Ok(None),
            Ok(Err(error)) => Err(error),
            Err(RecvTimeoutError::Timeout) => Err(pending_timeout()),
            Err(error) => Err(worker_error(error)),
        }
    }

    #[cfg(test)]
    pub(crate) fn expired_for_test() -> (Self, Sender<Outcome>) {
        let (sender, outcome) = mpsc::channel();
        (
            Self {
                cancel_signal: None,
                outcome: Some(outcome),
                deadline: Instant::now(),
                cleanup_pending: false,
            },
            sender,
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
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

pub(crate) fn recover_at_startup() -> Result<(), InterruptTimingError> {
    recover_if_idle(None)
}

fn recover_if_idle(expected: Option<CaptureIdentity>) -> Result<(), InterruptTimingError> {
    if let Some(lease) = LeaseGuard::try_acquire()? {
        recover(&lease, expected)?;
    }
    Ok(())
}

fn recover(
    lease: &LeaseGuard,
    expected: Option<CaptureIdentity>,
) -> Result<(), InterruptTimingError> {
    if let Some(identity) = lease.read()? {
        if expected.is_none_or(|expected| expected == identity) {
            worker::stop_owned(&identity.name())?;
            lease.clear()?;
        }
    }
    Ok(())
}

pub(crate) fn run_worker_if_requested() -> bool {
    if !std::env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == WORKER_ARGUMENT)
    {
        return false;
    }
    let outcome = run_worker();
    // The pipe can close if the parent exited. Unfinished cleanup stays recorded in the lease.
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
    let lease = LeaseGuard::try_acquire()?.ok_or(InterruptTimingError::InUse)?;
    recover(&lease, None).map_err(|error| match error {
        InterruptTimingError::AccessDenied => error,
        _ => InterruptTimingError::Cleanup(format!("Previous capture cleanup failed: {error}")),
    })?;
    lease.publish(identity)?;
    let outcome = worker::capture(&identity.name(), &cancelled);
    match &outcome {
        // These outcomes occur only before ETW starts or after the session stops.
        Ok(_)
        | Err(
            InterruptTimingError::AccessDenied
            | InterruptTimingError::InUse
            | InterruptTimingError::Cancelled,
        ) => lease.clear()?,
        Err(error) => recover(&lease, Some(identity))
            .map_err(|cleanup| worker_error(format!("{error} ETW cleanup failed: {cleanup}")))?,
    }
    outcome
}

fn supervise_worker(
    identity: CaptureIdentity,
    cancelled: &Receiver<()>,
    deadline: Instant,
) -> Outcome {
    if Instant::now() >= deadline {
        return Err(helper_timeout());
    }
    if cancel_requested(cancelled) {
        return Err(InterruptTimingError::Cancelled);
    }
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
    let completed = prepare_worker(&mut child).and_then(|(mut input, output)| {
        if Instant::now() >= deadline {
            return Err(helper_timeout());
        }
        if cancel_requested(cancelled) {
            return Err(InterruptTimingError::Cancelled);
        }
        input
            .write_all(&identity.startup_record())
            .map_err(worker_error)?;
        wait_for_worker(&mut child, input, cancelled, deadline).map(|status| (status, output))
    });
    if completed.is_err() {
        stop_worker(&mut child);
    }
    // Keep the reservation during cleanup without protecting the old PID.
    WORKER_PROCESS_ID.store(STARTING, Relaxed);
    let outcome = completed.and_then(|(status, output)| decode_worker_outcome(status, output));
    match outcome {
        Err(
            error @ (InterruptTimingError::TimedOut(_)
            | InterruptTimingError::Worker(_)
            | InterruptTimingError::Capture(_)),
        ) => match recover_if_idle(Some(identity)) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(InterruptTimingError::Cleanup(format!(
                "{error} ETW cleanup failed: {cleanup}"
            ))),
        },
        outcome => outcome,
    }
}

type WorkerOutput = Receiver<io::Result<Vec<u8>>>;

fn prepare_worker(child: &mut Child) -> Result<(ChildStdin, WorkerOutput), InterruptTimingError> {
    let input = child
        .stdin
        .take()
        .ok_or_else(|| worker_error("stdin unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| worker_error("stdout unavailable"))?;
    let (send, receive) = mpsc::channel();
    thread::Builder::new()
        .name("interrupt-timing-worker-stdout".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            let read = stdout.take(MAX_RESPONSE_BYTES + 1).read_to_end(&mut bytes);
            let _ = send.send(read.map(|_| bytes));
        })
        .map_err(worker_error)?;
    Ok((input, receive))
}

fn wait_for_worker(
    child: &mut Child,
    input: ChildStdin,
    cancelled: &Receiver<()>,
    mut deadline: Instant,
) -> Result<ExitStatus, InterruptTimingError> {
    let mut input = Some(input);
    loop {
        if let Some(status) = child.try_wait().map_err(worker_error)? {
            return Ok(status);
        }
        let now = Instant::now();
        if input.is_some() && cancel_requested(cancelled) {
            input.take();
            deadline = deadline.min(now + CANCEL_LIMIT);
        }
        if now >= deadline {
            return Err(helper_timeout());
        }
        thread::sleep(WORKER_POLL.min(deadline - now));
    }
}

fn cancel_requested(cancelled: &Receiver<()>) -> bool {
    !matches!(cancelled.try_recv(), Err(TryRecvError::Empty))
}

fn stop_worker(child: &mut Child) {
    let mut requested = false;
    while !matches!(child.try_wait(), Ok(Some(_))) {
        if !requested {
            requested = child.kill().is_ok();
        }
        thread::sleep(EXIT_CONFIRMATION_POLL);
    }
}

fn decode_worker_outcome(status: ExitStatus, output: WorkerOutput) -> Outcome {
    let bytes = output
        .recv_timeout(CLEANUP_GRACE)
        .map_err(worker_error)?
        .map_err(worker_error)?;
    if status.success() && bytes.len() as u64 <= MAX_RESPONSE_BYTES {
        return serde_json::from_slice::<Outcome>(&bytes).map_err(worker_error)?;
    }
    Err(worker_error(format!("invalid report ({status})")))
}

fn pending_timeout() -> InterruptTimingError {
    InterruptTimingError::TimedOut("Waiting for helper exit and capture cleanup.".into())
}

fn helper_timeout() -> InterruptTimingError {
    InterruptTimingError::TimedOut("The helper did not finish within its time limit.".into())
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
