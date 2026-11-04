use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
    io::{BufRead, BufReader, Write},
    os::windows::ffi::OsStrExt,
    os::windows::process::CommandExt,
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    ptr::null_mut,
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc, Mutex, MutexGuard, OnceLock,
    },
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use windows_sys::{
    Wdk::Graphics::Direct3D::{
        D3DKMTGetProcessSchedulingPriorityClass, D3DKMTSetProcessSchedulingPriorityClass,
        D3DKMT_SCHEDULINGPRIORITYCLASS,
    },
    Win32::{
        Foundation::{
            ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, FILETIME, HANDLE, STILL_ACTIVE,
        },
        System::{
            JobObjects::{OpenJobObjectW, SetInformationJobObject},
            SystemServices::JOB_OBJECT_SET_ATTRIBUTES,
            Threading::{
                GetExitCodeProcess, GetPriorityClass, GetProcessAffinityMask,
                GetProcessDefaultCpuSets, GetProcessId, GetProcessInformation,
                GetProcessPriorityBoost, GetProcessTimes, GetThreadId, GetThreadPriority,
                GetThreadTimes, OpenProcess, OpenThread, ProcessMemoryPriority,
                ProcessPowerThrottling, QueryFullProcessImageNameW, SetPriorityClass,
                SetProcessAffinityMask, SetProcessDefaultCpuSets, SetProcessInformation,
                SetProcessPriorityBoost, SetThreadPriority, MEMORY_PRIORITY_INFORMATION,
                PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_STATE,
                PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SET_INFORMATION, THREAD_QUERY_INFORMATION, THREAD_SET_INFORMATION,
            },
        },
    },
};

use crate::{
    foreground::same_executable_path,
    platform::windows::{
        suspension::{JobObjectFreezeInformation, JOB_OBJECT_FREEZE_INFORMATION_CLASS},
        thread_suspension::{self, CapturedThread, ThreadSuspensionError},
    },
    power::powercfg::{active_plan, restore_stale_adaptive_plans, set_active},
    win_util::{last_error, WinHandle},
};

const WATCHDOG_ARGUMENT: &str = "--winderust-recovery-watchdog";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const PROCESS_IO_PRIORITY: u32 = 33;
const THREAD_PRIORITY_ERROR_RETURN: i32 = i32::MAX;

static RUNTIME: Mutex<Option<RecoveryRuntime>> = Mutex::new(None);
// Read without RUNTIME: controllers may already hold a recovery intent's mutex.
static WATCHDOG_PROCESS_ID: AtomicU32 = AtomicU32::new(0);

pub(crate) fn is_watchdog_process(process_id: u32) -> bool {
    process_id != 0 && process_id == WATCHDOG_PROCESS_ID.load(Ordering::Relaxed)
}

static STARTUP_ERROR: OnceLock<String> = OnceLock::new();

#[cfg(feature = "recovery-smoke")]
pub(crate) mod smoke;

#[derive(Debug)]
struct RecoveryRuntime {
    child: Child,
    transport: RecoveryTransport,
    entries: Vec<RecoveryEntry>,
    next_intent_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum RecoveryCommand {
    Begin {
        id: u64,
        entry: RecoveryEntry,
    },
    Commit {
        id: u64,
    },
    Cancel {
        id: u64,
    },
    ForgetProcess {
        process_id: u32,
        creation_time: u64,
        value: ProcessValue,
    },
    ForgetThreadPriority {
        process_id: u32,
        process_creation_time: u64,
        thread_id: u32,
        thread_creation_time: u64,
    },
    ForgetThreadSuspension {
        process_id: u32,
        process_creation_time: u64,
        thread_id: u32,
        thread_creation_time: u64,
    },
    ForgetPowerPlan,
    ForgetJob {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ProcessIdentity {
    id: u32,
    creation_time: u64,
    executable_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "property", content = "value", rename_all = "snake_case")]
pub(crate) enum ProcessValue {
    PriorityClass(u32),
    PowerThrottling {
        version: u32,
        control_mask: u32,
        state_mask: u32,
    },
    Affinity(u64),
    CpuSets(Vec<u32>),
    DynamicPriorityBoostDisabled(bool),
    IoPriority(u32),
    GpuPriority(u32),
    MemoryPriority(u32),
}

impl ProcessValue {
    pub(crate) fn power_throttling(state: PROCESS_POWER_THROTTLING_STATE) -> Self {
        Self::PowerThrottling {
            version: state.Version,
            control_mask: state.ControlMask,
            state_mask: state.StateMask,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::PriorityClass(_) => "priority_class",
            Self::PowerThrottling { .. } => "power_throttling",
            Self::Affinity(_) => "affinity",
            Self::CpuSets(_) => "cpu_sets",
            Self::DynamicPriorityBoostDisabled(_) => "dynamic_priority_boost",
            Self::IoPriority(_) => "io_priority",
            Self::GpuPriority(_) => "gpu_priority",
            Self::MemoryPriority(_) => "memory_priority",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
enum RecoveryEntry {
    Process {
        identity: ProcessIdentity,
        original: ProcessValue,
        expected: ProcessValue,
    },
    ThreadPriority {
        process: ProcessIdentity,
        thread_id: u32,
        thread_creation_time: u64,
        original: i32,
        expected: i32,
    },
    ThreadSuspension {
        process: ProcessIdentity,
        thread_id: u32,
        thread_creation_time: u64,
        original_suspend_count: u16,
        expected_suspend_count: u16,
    },
    PowerPlan {
        original_guid: String,
        expected_guid: String,
    },
    SuspendedJob {
        name: String,
        process: ProcessIdentity,
    },
}

#[must_use = "dropping a recovery intent cancels it; commit it after the mutation succeeds"]
pub(crate) struct RecoveryIntent {
    runtime: Option<MutexGuard<'static, Option<RecoveryRuntime>>>,
    id: u64,
    entry: Option<RecoveryEntry>,
}

pub(crate) struct DeferredRecoveryIntent {
    id: u64,
    entry: Option<RecoveryEntry>,
}

impl DeferredRecoveryIntent {
    pub(crate) fn resolve(&mut self, applied: bool) -> Result<(), String> {
        if self.entry.is_none() {
            return Ok(());
        }
        let mut runtime = RUNTIME
            .lock()
            .map_err(|_| "Crash recovery state is poisoned.".to_owned())?;
        let runtime = runtime.as_mut().ok_or_else(|| {
            "Crash recovery stopped before deferred mutation resolution.".to_owned()
        })?;
        self.resolve_with_runtime(runtime, applied)
    }

    fn resolve_with_runtime(
        &mut self,
        runtime: &mut RecoveryRuntime,
        applied: bool,
    ) -> Result<(), String> {
        let Some(entry) = self.entry.as_ref() else {
            return Ok(());
        };
        let command = if applied {
            RecoveryCommand::Commit { id: self.id }
        } else {
            RecoveryCommand::Cancel { id: self.id }
        };
        runtime.transport.send(&command)?;
        if applied {
            compact_or_push(&mut runtime.entries, entry.clone());
        }
        self.entry = None;
        Ok(())
    }
}

impl RecoveryIntent {
    fn noop() -> Self {
        Self {
            runtime: None,
            id: 0,
            entry: None,
        }
    }

    pub(crate) fn commit(mut self) -> Result<(), String> {
        let Some(runtime) = self.runtime.as_mut() else {
            return Ok(());
        };
        let entry = self
            .entry
            .take()
            .ok_or_else(|| "Recovery intent was already completed.".to_owned())?;
        let runtime = runtime
            .as_mut()
            .ok_or_else(|| "Crash recovery stopped before mutation commit.".to_owned())?;
        commit_recovery_entry(runtime, self.id, entry)
    }

    pub(crate) fn defer(mut self) -> DeferredRecoveryIntent {
        let pending = DeferredRecoveryIntent {
            id: self.id,
            entry: self.entry.take(),
        };
        // Keep the acknowledged Begin pending in the helper without holding the
        // global mutex while the controller waits for a conclusive observation.
        drop(self.runtime.take());
        pending
    }

    pub(crate) fn retain_pending(mut self) -> Result<(), String> {
        // Disable Drop's Cancel even when internal state is unexpectedly missing.
        let entry = self.entry.take();
        let Some(runtime) = self.runtime.as_mut() else {
            return if entry.is_none() {
                Ok(())
            } else {
                Err("Recovery intent lost its runtime before pending retention.".to_owned())
            };
        };
        let runtime = runtime
            .as_mut()
            .ok_or_else(|| "Crash recovery stopped before pending retention.".to_owned())?;
        retain_pending_recovery(runtime);
        if entry.is_none() {
            return Err(
                "Recovery intent was already completed before pending retention.".to_owned(),
            );
        }
        Ok(())
    }

    pub(crate) fn commit_or_defer(mut self) -> Result<(), (String, DeferredRecoveryIntent)> {
        let result = self.commit_acknowledged();
        result.map_err(|error| (error, self.defer()))
    }

    fn commit_acknowledged(&mut self) -> Result<(), String> {
        let Some(runtime) = self.runtime.as_mut() else {
            return Ok(());
        };
        let entry = self
            .entry
            .as_ref()
            .ok_or_else(|| "Recovery intent was already completed.".to_owned())?;
        let runtime = runtime
            .as_mut()
            .ok_or_else(|| "Crash recovery stopped before mutation commit.".to_owned())?;
        runtime
            .transport
            .send(&RecoveryCommand::Commit { id: self.id })?;
        compact_or_push(&mut runtime.entries, entry.clone());
        self.entry = None;
        Ok(())
    }
}

fn commit_recovery_entry(
    runtime: &mut RecoveryRuntime,
    id: u64,
    entry: RecoveryEntry,
) -> Result<(), String> {
    compact_or_push(&mut runtime.entries, entry);
    if let Err(error) = runtime.transport.send(&RecoveryCommand::Commit { id }) {
        // The applied transition must stay recoverable whether Commit reached the
        // helper or left its exact Begin pending. Generic callers do not retain
        // that ID, so even an explicit rejection must prevent later mutations.
        let error = runtime.transport.failed.get_or_insert_with(|| {
            format!("{error} Recovery commit is unconfirmed; further recovery commands are blocked and outstanding recovery ownership is retained.")
        });
        return Err(error.clone());
    }
    Ok(())
}

fn retain_pending_recovery(runtime: &mut RecoveryRuntime) {
    runtime.transport.failed.get_or_insert_with(|| {
        "Mutation recovery is unresolved; further recovery commands are blocked and the acknowledged intent remains pending for helper recovery.".to_owned()
    });
}

impl Drop for RecoveryIntent {
    fn drop(&mut self) {
        if self.entry.is_some() {
            if let Some(runtime) = self.runtime.as_mut().and_then(|runtime| runtime.as_mut()) {
                let _ = runtime
                    .transport
                    .send(&RecoveryCommand::Cancel { id: self.id });
            }
        }
    }
}

impl RecoveryEntry {
    fn key(&self) -> String {
        match self {
            Self::Process {
                identity, expected, ..
            } => process_recovery_key(identity.id, identity.creation_time, expected),
            Self::ThreadPriority {
                process,
                thread_id,
                thread_creation_time,
                ..
            } => thread_recovery_key(
                process.id,
                process.creation_time,
                *thread_id,
                *thread_creation_time,
            ),
            Self::ThreadSuspension {
                process,
                thread_id,
                thread_creation_time,
                ..
            } => thread_suspension_recovery_key(
                process.id,
                process.creation_time,
                *thread_id,
                *thread_creation_time,
            ),
            Self::PowerPlan { .. } => "power_plan".to_owned(),
            Self::SuspendedJob { name, .. } => format!("job:{name}"),
        }
    }

    fn job_name_and_access(&self) -> Option<(&str, u32)> {
        match self {
            Self::SuspendedJob { name, .. } => Some((name, JOB_OBJECT_SET_ATTRIBUTES)),
            _ => None,
        }
    }
}

fn process_recovery_key(process_id: u32, creation_time: u64, value: &ProcessValue) -> String {
    format!("process:{process_id}:{creation_time}:{}", value.kind())
}

fn thread_recovery_key(
    process_id: u32,
    process_creation_time: u64,
    thread_id: u32,
    thread_creation_time: u64,
) -> String {
    format!("thread:{process_id}:{process_creation_time}:{thread_id}:{thread_creation_time}")
}

fn thread_suspension_recovery_key(
    process_id: u32,
    process_creation_time: u64,
    thread_id: u32,
    thread_creation_time: u64,
) -> String {
    format!(
        "thread_suspension:{process_id}:{process_creation_time}:{thread_id}:{thread_creation_time}"
    )
}

pub(crate) fn run_watchdog_if_requested() -> bool {
    if std::env::args().nth(1).as_deref() != Some(WATCHDOG_ARGUMENT) {
        return false;
    }
    super::diagnostics::initialize("winderust-recovery");
    if let Err(error) = run_watchdog_loop(
        BufReader::new(std::io::stdin()),
        std::io::stdout(),
        |_| Ok(()),
        restore_stale_adaptive_plans,
    ) {
        super::diagnostics::error(&format!("Crash recovery failed: {error}"));
        eprintln!("Winderust crash recovery failed: {error}");
        super::diagnostics::finish();
        std::process::exit(2);
    }
    super::diagnostics::event("Recovery helper finished restoration.");
    super::diagnostics::finish();
    true
}

fn run_watchdog_loop(
    input: impl BufRead,
    mut output: impl Write,
    mut authorize: impl FnMut(&RecoveryCommand) -> Result<(), String>,
    cleanup: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let mut entries = Vec::new();
    let mut pending = Vec::new();
    let mut jobs = HashMap::new();
    for line in input.lines() {
        let result = line
            .map_err(|error| format!("Failed to read recovery command: {error}"))
            .and_then(|line| {
                serde_json::from_str::<RecoveryCommand>(&line)
                    .map_err(|error| format!("Invalid recovery command: {error}"))
            })
            .and_then(|command| {
                authorize(&command)?;
                apply_watchdog_command(command, &mut entries, &mut pending, &mut jobs)
            });
        let response = match result {
            Ok(()) => "ok".to_owned(),
            Err(error) => {
                super::diagnostics::error(&error);
                format!("error:{error}")
            }
        };
        if writeln!(output, "{response}")
            .and_then(|()| output.flush())
            .is_err()
        {
            break;
        }
    }
    let committed_len = append_pending_intents(&mut entries, pending);
    if !entries.is_empty() {
        recover_with_retry_using(
            || recover_journal_with_pending(&entries, committed_len),
            cleanup,
        )?;
    }
    Ok(())
}

fn apply_watchdog_command(
    command: RecoveryCommand,
    entries: &mut Vec<RecoveryEntry>,
    pending: &mut Vec<(u64, RecoveryEntry)>,
    jobs: &mut HashMap<String, WinHandle>,
) -> Result<(), String> {
    apply_watchdog_command_with_open_job(command, entries, pending, jobs, open_job)
}

fn apply_watchdog_command_with_open_job(
    command: RecoveryCommand,
    entries: &mut Vec<RecoveryEntry>,
    pending: &mut Vec<(u64, RecoveryEntry)>,
    jobs: &mut HashMap<String, WinHandle>,
    open_retained_job: impl FnOnce(&str, u32) -> Result<WinHandle, String>,
) -> Result<(), String> {
    match command {
        RecoveryCommand::Begin { id, entry } => {
            if let Some((name, desired_access)) = entry.job_name_and_access() {
                if !jobs.contains_key(name) {
                    jobs.insert(name.to_owned(), open_retained_job(name, desired_access)?);
                }
            }
            pending.push((id, entry));
        }
        RecoveryCommand::Commit { id } => {
            if let Some(index) = pending.iter().position(|(candidate, _)| *candidate == id) {
                let (_, entry) = pending.remove(index);
                compact_or_push(entries, entry);
            }
        }
        RecoveryCommand::Cancel { id } => {
            if let Some(index) = pending.iter().position(|(candidate, _)| *candidate == id) {
                let (_, entry) = pending.remove(index);
                if let Some((name, _)) = entry.job_name_and_access() {
                    let key = format!("job:{name}");
                    if !entries.iter().any(|entry| entry.key() == key)
                        && !pending.iter().any(|(_, entry)| entry.key() == key)
                    {
                        jobs.remove(name);
                    }
                }
            }
        }
        RecoveryCommand::ForgetProcess {
            process_id,
            creation_time,
            value,
        } => {
            let key = process_recovery_key(process_id, creation_time, &value);
            entries.retain(|entry| entry.key() != key);
            pending.retain(|(_, entry)| entry.key() != key);
        }
        RecoveryCommand::ForgetThreadPriority {
            process_id,
            process_creation_time,
            thread_id,
            thread_creation_time,
        } => {
            let key = thread_recovery_key(
                process_id,
                process_creation_time,
                thread_id,
                thread_creation_time,
            );
            entries.retain(|entry| entry.key() != key);
            pending.retain(|(_, entry)| entry.key() != key);
        }
        RecoveryCommand::ForgetThreadSuspension {
            process_id,
            process_creation_time,
            thread_id,
            thread_creation_time,
        } => {
            let key = thread_suspension_recovery_key(
                process_id,
                process_creation_time,
                thread_id,
                thread_creation_time,
            );
            entries.retain(|entry| entry.key() != key);
            pending.retain(|(_, entry)| entry.key() != key);
        }
        RecoveryCommand::ForgetPowerPlan => {
            entries.retain(|entry| !matches!(entry, RecoveryEntry::PowerPlan { .. }));
            pending.retain(|(_, entry)| !matches!(entry, RecoveryEntry::PowerPlan { .. }));
        }
        RecoveryCommand::ForgetJob { name } => {
            let key = format!("job:{name}");
            entries.retain(|entry| entry.key() != key);
            pending.retain(|(_, entry)| entry.key() != key);
            jobs.remove(&name);
        }
    }
    Ok(())
}

// Pending intents may or may not have executed. Keep their edges and the committed
// boundary intact so recovery can recognize either outcome without losing a baseline.
fn append_pending_intents(
    entries: &mut Vec<RecoveryEntry>,
    pending: Vec<(u64, RecoveryEntry)>,
) -> usize {
    let committed_len = entries.len();
    entries.extend(pending.into_iter().map(|(_, entry)| entry));
    committed_len
}

fn recover_with_retry_using(
    mut recover: impl FnMut() -> Result<(), String>,
    mut cleanup: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let mut last_error = None;
    for attempt in 0..3 {
        // Metadata restores only the immediate source; the journal may own an
        // older baseline. Cleanup must not hide that chain after failed replay.
        match recover().and_then(|()| cleanup()) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        if attempt < 2 {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    Err(last_error.unwrap_or_else(|| "Unknown recovery failure.".to_owned()))
}

pub(crate) struct RecoveryClient {
    active: bool,
}

impl RecoveryClient {
    pub(crate) fn start() -> Self {
        match initialize_inner() {
            Ok(()) => Self { active: true },
            Err(error) => {
                set_startup_error(format!("Crash recovery protection is unavailable: {error}"));
                Self { active: false }
            }
        }
    }

    pub(crate) fn finish(&mut self) -> Result<(), String> {
        if !self.active {
            return Ok(());
        }

        let runtime = RUNTIME
            .lock()
            .map_err(|_| "Crash recovery state is poisoned.".to_owned())?
            .take();
        self.active = false;
        runtime.map_or(Ok(()), finish_recovery_runtime)
    }
}

fn finish_recovery_runtime(mut runtime: RecoveryRuntime) -> Result<(), String> {
    drop(runtime.transport);
    wait_for_watchdog(&mut runtime.child, Duration::from_secs(5))
}

fn wait_for_watchdog(child: &mut Child, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("Failed to wait for crash recovery helper: {error}"))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            return Err("Crash recovery helper shutdown timed out; recovery is unconfirmed. The helper has been left running with its recovery journal.".to_owned());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        return Err(format!(
            "Crash recovery helper exited unexpectedly with status code {}.",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

impl Drop for RecoveryClient {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn initialize_inner() -> Result<(), String> {
    let (child, stdin, stdout) = spawn_watchdog()?;
    let watchdog_id = child.id();
    let runtime = RecoveryRuntime {
        child,
        transport: RecoveryTransport::start(stdin, stdout, Duration::from_secs(2))?,
        entries: Vec::new(),
        next_intent_id: 1,
    };
    RUNTIME
        .lock()
        .map_err(|_| "Crash recovery state is poisoned.".to_owned())?
        .replace(runtime);
    WATCHDOG_PROCESS_ID.store(watchdog_id, Ordering::Relaxed);
    Ok(())
}

pub(crate) fn startup_error() -> Option<String> {
    STARTUP_ERROR.get().cloned()
}

pub(crate) fn record_process_change(
    handle: HANDLE,
    original: ProcessValue,
    expected: ProcessValue,
) -> Result<RecoveryIntent, String> {
    if original.kind() != expected.kind() {
        return Err("Recovery values describe different process properties.".to_owned());
    }
    if original == expected {
        return Ok(RecoveryIntent::noop());
    }
    record_entry(RecoveryEntry::Process {
        identity: process_identity(handle)?,
        original,
        expected,
    })
}

pub(crate) fn record_thread_priority_change(
    process_handle: HANDLE,
    thread_handle: HANDLE,
    original: i32,
    expected: i32,
) -> Result<RecoveryIntent, String> {
    if original == expected {
        return Ok(RecoveryIntent::noop());
    }
    record_entry(RecoveryEntry::ThreadPriority {
        process: process_identity(process_handle)?,
        thread_id: thread_id(thread_handle)?,
        thread_creation_time: thread_creation_time(thread_handle)?,
        original,
        expected,
    })
}

pub(crate) fn record_thread_suspension(
    process_handle: HANDLE,
    thread_handle: HANDLE,
    original_suspend_count: u16,
) -> Result<RecoveryIntent, String> {
    let expected_suspend_count = original_suspend_count
        .checked_add(1)
        .ok_or_else(|| "Thread suspend count cannot exceed 65535.".to_owned())?;
    record_entry(RecoveryEntry::ThreadSuspension {
        process: process_identity(process_handle)?,
        thread_id: thread_id(thread_handle)?,
        thread_creation_time: thread_creation_time(thread_handle)?,
        original_suspend_count,
        expected_suspend_count,
    })
}

pub(crate) fn record_power_plan_change(
    original: &str,
    expected: &str,
) -> Result<RecoveryIntent, String> {
    if original.eq_ignore_ascii_case(expected) {
        return Ok(RecoveryIntent::noop());
    }
    record_entry(RecoveryEntry::PowerPlan {
        original_guid: original.to_owned(),
        expected_guid: expected.to_owned(),
    })
}

pub(crate) fn forget_power_plan_changes() -> Result<(), String> {
    let mut runtime = RUNTIME
        .lock()
        .map_err(|_| "Crash recovery state is poisoned.".to_owned())?;
    let Some(runtime) = runtime.as_mut() else {
        #[cfg(test)]
        return Ok(());
        #[cfg(not(test))]
        return Err("The external recovery watchdog is unavailable.".to_owned());
    };
    runtime.transport.send(&RecoveryCommand::ForgetPowerPlan)?;
    runtime
        .entries
        .retain(|entry| !matches!(entry, RecoveryEntry::PowerPlan { .. }));
    Ok(())
}

pub(crate) fn suspension_job_name(process_id: u32, creation_time: u64) -> String {
    job_name("Suspend", process_id, creation_time)
}

fn job_name(mechanism: &str, process_id: u32, creation_time: u64) -> String {
    let executable_hash = std::env::current_exe()
        .ok()
        .map(|path| fnv1a64(path.as_os_str().encode_wide()))
        .unwrap_or(0x5f3f_2a4e_13a5_59f0);
    format!("Local\\Winderust.{mechanism}.{executable_hash:016x}.{process_id}.{creation_time}")
}

pub(crate) fn record_suspended_job(
    name: &str,
    process_handle: HANDLE,
) -> Result<RecoveryIntent, String> {
    record_entry(RecoveryEntry::SuspendedJob {
        name: name.to_owned(),
        process: process_identity(process_handle)?,
    })
}

pub(crate) fn forget_suspended_job(name: &str) -> Result<(), String> {
    forget_job(name)
}

fn forget_job(name: &str) -> Result<(), String> {
    let mut runtime = RUNTIME
        .lock()
        .map_err(|_| "Crash recovery state is poisoned.".to_owned())?;
    let Some(runtime) = runtime.as_mut() else {
        #[cfg(test)]
        return Ok(());
        #[cfg(not(test))]
        return Err("The external recovery watchdog is unavailable.".to_owned());
    };
    runtime.transport.send(&RecoveryCommand::ForgetJob {
        name: name.to_owned(),
    })?;
    let key = format!("job:{name}");
    runtime.entries.retain(|entry| entry.key() != key);
    Ok(())
}

pub(crate) fn forget_dynamic_priority_boost_change(
    process_id: u32,
    creation_time: u64,
) -> Result<(), String> {
    forget_process_change(
        process_id,
        creation_time,
        ProcessValue::DynamicPriorityBoostDisabled(false),
    )
}

pub(crate) fn forget_priority_class_change(
    process_id: u32,
    creation_time: u64,
) -> Result<(), String> {
    forget_process_change(process_id, creation_time, ProcessValue::PriorityClass(0))
}

pub(crate) fn forget_power_throttling_change(
    process_id: u32,
    creation_time: u64,
) -> Result<(), String> {
    forget_process_change(
        process_id,
        creation_time,
        ProcessValue::PowerThrottling {
            version: 0,
            control_mask: 0,
            state_mask: 0,
        },
    )
}

pub(crate) fn forget_affinity_change(process_id: u32, creation_time: u64) -> Result<(), String> {
    forget_process_change(process_id, creation_time, ProcessValue::Affinity(0))
}

pub(crate) fn forget_cpu_sets_change(process_id: u32, creation_time: u64) -> Result<(), String> {
    forget_process_change(process_id, creation_time, ProcessValue::CpuSets(Vec::new()))
}

pub(crate) fn forget_io_priority_change(process_id: u32, creation_time: u64) -> Result<(), String> {
    forget_process_change(process_id, creation_time, ProcessValue::IoPriority(0))
}

pub(crate) fn forget_gpu_priority_change(
    process_id: u32,
    creation_time: u64,
) -> Result<(), String> {
    forget_process_change(process_id, creation_time, ProcessValue::GpuPriority(0))
}

pub(crate) fn forget_memory_priority_change(
    process_id: u32,
    creation_time: u64,
) -> Result<(), String> {
    forget_process_change(process_id, creation_time, ProcessValue::MemoryPriority(0))
}

pub(crate) fn forget_thread_priority_change(
    process_id: u32,
    process_creation_time: u64,
    thread_id: u32,
    thread_creation_time: u64,
) -> Result<(), String> {
    let mut runtime = RUNTIME
        .lock()
        .map_err(|_| "Crash recovery state is poisoned.".to_owned())?;
    let Some(runtime) = runtime.as_mut() else {
        #[cfg(test)]
        return Ok(());
        #[cfg(not(test))]
        return Err("The external recovery watchdog is unavailable.".to_owned());
    };
    runtime
        .transport
        .send(&RecoveryCommand::ForgetThreadPriority {
            process_id,
            process_creation_time,
            thread_id,
            thread_creation_time,
        })?;
    let key = thread_recovery_key(
        process_id,
        process_creation_time,
        thread_id,
        thread_creation_time,
    );
    runtime.entries.retain(|entry| entry.key() != key);
    Ok(())
}

pub(crate) fn forget_thread_suspension(
    process_id: u32,
    process_creation_time: u64,
    thread_id: u32,
    thread_creation_time: u64,
) -> Result<(), String> {
    let mut runtime = RUNTIME
        .lock()
        .map_err(|_| "Crash recovery state is poisoned.".to_owned())?;
    let Some(runtime) = runtime.as_mut() else {
        #[cfg(test)]
        return Ok(());
        #[cfg(not(test))]
        return Err("The external recovery watchdog is unavailable.".to_owned());
    };
    runtime
        .transport
        .send(&RecoveryCommand::ForgetThreadSuspension {
            process_id,
            process_creation_time,
            thread_id,
            thread_creation_time,
        })?;
    let key = thread_suspension_recovery_key(
        process_id,
        process_creation_time,
        thread_id,
        thread_creation_time,
    );
    runtime.entries.retain(|entry| entry.key() != key);
    Ok(())
}

fn forget_process_change(
    process_id: u32,
    creation_time: u64,
    value: ProcessValue,
) -> Result<(), String> {
    let mut runtime = RUNTIME
        .lock()
        .map_err(|_| "Crash recovery state is poisoned.".to_owned())?;
    let Some(runtime) = runtime.as_mut() else {
        #[cfg(test)]
        return Ok(());
        #[cfg(not(test))]
        return Err("The external recovery watchdog is unavailable.".to_owned());
    };
    runtime.transport.send(&RecoveryCommand::ForgetProcess {
        process_id,
        creation_time,
        value: value.clone(),
    })?;
    let key = process_recovery_key(process_id, creation_time, &value);
    runtime.entries.retain(|entry| entry.key() != key);
    Ok(())
}

fn record_entry(entry: RecoveryEntry) -> Result<RecoveryIntent, String> {
    let mut runtime = RUNTIME
        .lock()
        .map_err(|_| "Crash recovery state is poisoned.".to_owned())?;
    if runtime.is_none() {
        #[cfg(test)]
        return Ok(RecoveryIntent::noop());
        #[cfg(not(test))]
        return Err(
            "The external recovery watchdog was not initialized; the change was blocked."
                .to_owned(),
        );
    }
    let state = runtime
        .as_mut()
        .ok_or_else(|| "Crash recovery state disappeared.".to_owned())?;
    let id = state.next_intent_id;
    state.next_intent_id = state.next_intent_id.wrapping_add(1).max(1);
    state.transport.send(&RecoveryCommand::Begin {
        id,
        entry: entry.clone(),
    })?;
    Ok(RecoveryIntent {
        runtime: Some(runtime),
        id,
        entry: Some(entry),
    })
}

fn compact_or_push(entries: &mut Vec<RecoveryEntry>, entry: RecoveryEntry) {
    let key = entry.key();
    if let Some(index) = entries.iter().rposition(|previous| previous.key() == key) {
        let mut compacted = true;
        let remove = match (&mut entries[index], &entry) {
            (
                RecoveryEntry::Process {
                    original: baseline,
                    expected: prior,
                    ..
                },
                RecoveryEntry::Process {
                    original, expected, ..
                },
            ) if prior == original => {
                *prior = expected.clone();
                baseline == prior
            }
            (
                RecoveryEntry::ThreadPriority {
                    original: baseline,
                    expected: prior,
                    ..
                },
                RecoveryEntry::ThreadPriority {
                    original, expected, ..
                },
            ) if prior == original => {
                *prior = *expected;
                baseline == prior
            }
            (RecoveryEntry::ThreadSuspension { .. }, RecoveryEntry::ThreadSuspension { .. }) => {
                return
            }
            (
                RecoveryEntry::PowerPlan {
                    original_guid: baseline,
                    expected_guid: prior,
                },
                RecoveryEntry::PowerPlan {
                    original_guid,
                    expected_guid,
                },
            ) if prior.eq_ignore_ascii_case(original_guid) => {
                *prior = expected_guid.clone();
                baseline.eq_ignore_ascii_case(prior)
            }
            (RecoveryEntry::SuspendedJob { .. }, RecoveryEntry::SuspendedJob { .. }) => return,
            _ => {
                compacted = false;
                false
            }
        };
        if compacted {
            if remove {
                entries.remove(index);
            }
            return;
        }
    }
    entries.push(entry);
}

#[cfg(test)]
fn recover_journal(entries: &[RecoveryEntry]) -> Result<(), String> {
    recover_journal_with_pending(entries, entries.len())
}

fn recover_journal_with_pending(
    entries: &[RecoveryEntry],
    committed_len: usize,
) -> Result<(), String> {
    let mut recovered = HashSet::new();
    let mut failures = Vec::new();
    for entry in entries.iter().rev() {
        let key = entry.key();
        if recovered.insert(key.clone()) {
            if let Err(error) = recover_entry(entry, &key, entries, committed_len) {
                failures.push(error);
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join(" "))
    }
}

fn recover_entry(
    entry: &RecoveryEntry,
    key: &str,
    entries: &[RecoveryEntry],
    committed_len: usize,
) -> Result<(), String> {
    match entry {
        RecoveryEntry::Process {
            identity, expected, ..
        } => recover_process_key(key, identity, expected, entries, committed_len),
        RecoveryEntry::ThreadPriority {
            process,
            thread_id,
            thread_creation_time,
            ..
        } => recover_thread_key(
            key,
            process,
            *thread_id,
            *thread_creation_time,
            entries,
            committed_len,
        ),
        RecoveryEntry::ThreadSuspension {
            process,
            thread_id,
            thread_creation_time,
            original_suspend_count,
            expected_suspend_count,
        } => recover_thread_suspension(
            process,
            *thread_id,
            *thread_creation_time,
            *original_suspend_count,
            *expected_suspend_count,
        ),
        RecoveryEntry::PowerPlan { .. } => recover_power_plan_key(key, entries, committed_len),
        RecoveryEntry::SuspendedJob { name, .. } => thaw_job(name),
    }
}

fn recover_process_key(
    key: &str,
    identity: &ProcessIdentity,
    value_kind: &ProcessValue,
    entries: &[RecoveryEntry],
    committed_len: usize,
) -> Result<(), String> {
    let Some(process) = open_matching_process(identity)? else {
        return Ok(());
    };
    let current = query_process_value(process.raw(), value_kind)?;
    let desired = unwind_process_value(key, &current, entries, committed_len);
    if desired != current {
        apply_process_value(process.raw(), &desired)?;
    }
    Ok(())
}

fn unwind_process_value(
    key: &str,
    current: &ProcessValue,
    entries: &[RecoveryEntry],
    committed_len: usize,
) -> ProcessValue {
    let mut desired = current.clone();
    for (index, entry) in entries
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, entry)| entry.key() == key)
    {
        if let RecoveryEntry::Process {
            original, expected, ..
        } = entry
        {
            if *expected == desired {
                desired = original.clone();
            } else if index < committed_len || *original != desired {
                break;
            }
        }
    }
    desired
}

fn recover_thread_key(
    key: &str,
    process: &ProcessIdentity,
    thread_id: u32,
    expected_creation_time: u64,
    entries: &[RecoveryEntry],
    committed_len: usize,
) -> Result<(), String> {
    let Some(process_handle) =
        open_matching_process_with_access(process, PROCESS_QUERY_LIMITED_INFORMATION)?
    else {
        return Ok(());
    };
    // SAFETY: thread_id came from the recovery journal and no inherited handle is requested.
    let handle = unsafe {
        OpenThread(
            THREAD_QUERY_INFORMATION | THREAD_SET_INFORMATION,
            0,
            thread_id,
        )
    };
    if handle.is_null() {
        return match last_error() {
            ERROR_INVALID_PARAMETER => Ok(()),
            error => Err(format!(
                "OpenThread({thread_id}) failed with error {error}."
            )),
        };
    }
    let thread = WinHandle::new(handle);
    if thread_creation_time(thread.raw())? != expected_creation_time {
        return Ok(());
    }
    // SAFETY: thread is live and opened with query access.
    let owner =
        unsafe { windows_sys::Win32::System::Threading::GetProcessIdOfThread(thread.raw()) };
    if owner == 0 {
        return Err(format!(
            "GetProcessIdOfThread({thread_id}) failed with error {}.",
            last_error()
        ));
    }
    if owner != process.id || process_creation_time(process_handle.raw())? != process.creation_time
    {
        return Ok(());
    }
    // SAFETY: thread is live and was revalidated against the recorded process instance.
    let current = unsafe { GetThreadPriority(thread.raw()) };
    if current == THREAD_PRIORITY_ERROR_RETURN {
        return Err(format!(
            "GetThreadPriority({thread_id}) failed with error {}.",
            last_error()
        ));
    }
    let desired = unwind_thread_priority(key, current, entries, committed_len);
    if desired != current {
        // SAFETY: desired was previously read from this validated thread instance.
        if unsafe { SetThreadPriority(thread.raw(), desired) } == 0 {
            return Err(format!(
                "SetThreadPriority({thread_id}) failed with error {}.",
                last_error()
            ));
        }
    }
    Ok(())
}

fn unwind_thread_priority(
    key: &str,
    current: i32,
    entries: &[RecoveryEntry],
    committed_len: usize,
) -> i32 {
    let mut desired = current;
    for (index, entry) in entries
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, entry)| entry.key() == key)
    {
        if let RecoveryEntry::ThreadPriority {
            original, expected, ..
        } = entry
        {
            if *expected == desired {
                desired = *original;
            } else if index < committed_len || *original != desired {
                break;
            }
        }
    }
    desired
}

fn should_resume_thread_suspension(original: u16, expected: u16, current: u16) -> bool {
    original.checked_add(1) == Some(expected) && current == expected
}

fn recover_thread_suspension(
    process: &ProcessIdentity,
    thread_id: u32,
    thread_creation_time: u64,
    original_suspend_count: u16,
    expected_suspend_count: u16,
) -> Result<(), String> {
    let Some(process_handle) =
        open_matching_process_with_access(process, PROCESS_QUERY_INFORMATION)?
    else {
        return Ok(());
    };
    let Some(thread) = recoverable_thread_suspension_result(
        thread_suspension::open_exact_thread(
            process.id,
            CapturedThread {
                id: thread_id,
                creation_time: thread_creation_time,
                suspend_count: expected_suspend_count,
            },
        ),
        thread_id,
    )?
    else {
        return Ok(());
    };
    let Some(current_suspend_count) = recoverable_thread_suspension_result(
        thread_suspension::exact_suspend_count(
            process_handle.raw(),
            thread_id,
            thread_creation_time,
        ),
        thread_id,
    )?
    else {
        return Ok(());
    };
    if should_resume_thread_suspension(
        original_suspend_count,
        expected_suspend_count,
        current_suspend_count,
    ) {
        recoverable_thread_suspension_result(thread_suspension::resume_once(&thread), thread_id)?;
    }
    Ok(())
}

fn recoverable_thread_suspension_result<T>(
    result: Result<T, ThreadSuspensionError>,
    thread_id: u32,
) -> Result<Option<T>, String> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(
            ThreadSuspensionError::ProcessExited
            | ThreadSuspensionError::ThreadExited { .. }
            | ThreadSuspensionError::IdentityChanged { .. }
            | ThreadSuspensionError::SuspendCountConflict { .. },
        ) => Ok(None),
        Err(error) => Err(format!(
            "Thread suspension recovery for thread {thread_id} failed: {error:?}."
        )),
    }
}

fn recover_power_plan_key(
    key: &str,
    entries: &[RecoveryEntry],
    committed_len: usize,
) -> Result<(), String> {
    recover_power_plan_key_with(
        key,
        entries,
        committed_len,
        || active_plan().map(|plan| plan.guid),
        set_active,
    )
}

fn recover_power_plan_key_with(
    key: &str,
    entries: &[RecoveryEntry],
    committed_len: usize,
    mut active_guid: impl FnMut() -> Result<String, String>,
    mut activate: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    let current = active_guid()?;
    let desired = unwind_power_plan_guid(key, &current, entries, committed_len);
    if !desired.eq_ignore_ascii_case(&current) {
        if !active_guid()?.eq_ignore_ascii_case(&current) {
            return Ok(());
        }
        activate(&desired)?;
        let actual = active_guid()?;
        if !actual.eq_ignore_ascii_case(&desired) {
            return Err(format!(
                "Power-plan recovery expected {desired}, but Windows reported {actual}."
            ));
        }
    }
    Ok(())
}

fn unwind_power_plan_guid(
    key: &str,
    current: &str,
    entries: &[RecoveryEntry],
    committed_len: usize,
) -> String {
    let mut desired = current.to_owned();
    for (index, entry) in entries
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, entry)| entry.key() == key)
    {
        if let RecoveryEntry::PowerPlan {
            original_guid,
            expected_guid,
        } = entry
        {
            if expected_guid.eq_ignore_ascii_case(&desired) {
                desired = original_guid.clone();
            } else if index < committed_len || !original_guid.eq_ignore_ascii_case(&desired) {
                break;
            }
        }
    }
    desired
}

fn query_process_value(handle: HANDLE, kind: &ProcessValue) -> Result<ProcessValue, String> {
    match kind {
        ProcessValue::PriorityClass(_) => {
            // SAFETY: handle is live and opened with query access.
            let value = unsafe { GetPriorityClass(handle) };
            (value != 0)
                .then_some(ProcessValue::PriorityClass(value))
                .ok_or_else(|| format!("GetPriorityClass failed with error {}.", last_error()))
        }
        ProcessValue::PowerThrottling { .. } => {
            let mut state = PROCESS_POWER_THROTTLING_STATE {
                Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                ..Default::default()
            };
            // SAFETY: state is writable for exactly the supplied structure size.
            let ok = unsafe {
                GetProcessInformation(
                    handle,
                    ProcessPowerThrottling,
                    (&mut state as *mut PROCESS_POWER_THROTTLING_STATE).cast(),
                    std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
                )
            };
            (ok != 0)
                .then_some(ProcessValue::power_throttling(state))
                .ok_or_else(|| format!("GetProcessInformation failed with error {}.", last_error()))
        }
        ProcessValue::Affinity(_) => {
            let mut process_mask = 0;
            let mut system_mask = 0;
            // SAFETY: both outputs are writable for this live process handle.
            let ok = unsafe { GetProcessAffinityMask(handle, &mut process_mask, &mut system_mask) };
            (ok != 0)
                .then_some(ProcessValue::Affinity(process_mask as u64))
                .ok_or_else(|| {
                    format!("GetProcessAffinityMask failed with error {}.", last_error())
                })
        }
        ProcessValue::CpuSets(_) => query_cpu_sets(handle).map(ProcessValue::CpuSets),
        ProcessValue::DynamicPriorityBoostDisabled(_) => {
            let mut disabled = 0;
            // SAFETY: disabled is writable for this live process handle.
            let ok = unsafe { GetProcessPriorityBoost(handle, &mut disabled) };
            (ok != 0)
                .then_some(ProcessValue::DynamicPriorityBoostDisabled(disabled != 0))
                .ok_or_else(|| {
                    format!(
                        "GetProcessPriorityBoost failed with error {}.",
                        last_error()
                    )
                })
        }
        ProcessValue::IoPriority(_) => {
            let mut raw = 0_u32;
            // SAFETY: raw is writable for exactly the supplied size.
            let status = unsafe {
                NtQueryInformationProcess(
                    handle,
                    PROCESS_IO_PRIORITY,
                    (&mut raw as *mut u32).cast(),
                    std::mem::size_of::<u32>() as u32,
                    null_mut(),
                )
            };
            nt_success(status, "NtQueryInformationProcess")?;
            Ok(ProcessValue::IoPriority(raw))
        }
        ProcessValue::GpuPriority(_) => {
            let mut raw = 0;
            // SAFETY: raw is writable for this live process handle.
            let status = unsafe { D3DKMTGetProcessSchedulingPriorityClass(handle, &mut raw) };
            nt_success(status, "D3DKMTGetProcessSchedulingPriorityClass")?;
            Ok(ProcessValue::GpuPriority(
                u32::try_from(raw).map_err(|_| format!("Invalid GPU priority {raw}."))?,
            ))
        }
        ProcessValue::MemoryPriority(_) => {
            let mut info = MEMORY_PRIORITY_INFORMATION::default();
            // SAFETY: info is writable for exactly the supplied structure size.
            let ok = unsafe {
                GetProcessInformation(
                    handle,
                    ProcessMemoryPriority,
                    (&mut info as *mut MEMORY_PRIORITY_INFORMATION).cast(),
                    std::mem::size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
                )
            };
            (ok != 0)
                .then_some(ProcessValue::MemoryPriority(info.MemoryPriority))
                .ok_or_else(|| format!("GetProcessInformation failed with error {}.", last_error()))
        }
    }
}

fn apply_process_value(handle: HANDLE, value: &ProcessValue) -> Result<(), String> {
    let ok = match value {
        ProcessValue::PriorityClass(value) => {
            // SAFETY: value was previously read from this validated process instance.
            unsafe { SetPriorityClass(handle, *value) }
        }
        ProcessValue::PowerThrottling {
            version,
            control_mask,
            state_mask,
        } => {
            let state = PROCESS_POWER_THROTTLING_STATE {
                Version: *version,
                ControlMask: *control_mask,
                StateMask: *state_mask,
            };
            // SAFETY: state is initialized for exactly the supplied structure size.
            unsafe {
                SetProcessInformation(
                    handle,
                    ProcessPowerThrottling,
                    (&state as *const PROCESS_POWER_THROTTLING_STATE).cast(),
                    std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
                )
            }
        }
        ProcessValue::Affinity(value) => {
            let value = usize::try_from(*value)
                .map_err(|_| format!("Affinity mask {value:#x} does not fit this platform."))?;
            // SAFETY: value was previously read from this validated process instance.
            unsafe { SetProcessAffinityMask(handle, value) }
        }
        ProcessValue::CpuSets(ids) => {
            let (pointer, count) = if ids.is_empty() {
                (null_mut(), 0)
            } else {
                (ids.as_ptr() as *mut u32, ids.len() as u32)
            };
            // SAFETY: pointer covers count IDs for this synchronous call.
            unsafe { SetProcessDefaultCpuSets(handle, pointer, count) }
        }
        ProcessValue::DynamicPriorityBoostDisabled(disabled) => {
            // SAFETY: disabled is converted to the documented BOOL representation.
            unsafe { SetProcessPriorityBoost(handle, i32::from(*disabled)) }
        }
        ProcessValue::IoPriority(raw) => {
            let mut raw = *raw;
            crate::platform::windows::io_priority::with_privilege_retry(|| {
                // SAFETY: raw points to exactly the supplied u32 size.
                unsafe {
                    NtSetInformationProcess(
                        handle,
                        PROCESS_IO_PRIORITY,
                        (&mut raw as *mut u32).cast(),
                        std::mem::size_of::<u32>() as u32,
                    )
                }
            })
            .map_err(|error| error.to_string())?;
            return Ok(());
        }
        ProcessValue::GpuPriority(raw) => {
            let priority = D3DKMT_SCHEDULINGPRIORITYCLASS::try_from(*raw)
                .map_err(|_| format!("Invalid GPU priority {raw}."))?;
            // SAFETY: priority was validated by the SDK enum conversion.
            let status = unsafe { D3DKMTSetProcessSchedulingPriorityClass(handle, priority) };
            nt_success(status, "D3DKMTSetProcessSchedulingPriorityClass")?;
            return Ok(());
        }
        ProcessValue::MemoryPriority(raw) => {
            let info = MEMORY_PRIORITY_INFORMATION {
                MemoryPriority: *raw,
            };
            // SAFETY: info is initialized for exactly the supplied structure size.
            unsafe {
                SetProcessInformation(
                    handle,
                    ProcessMemoryPriority,
                    (&info as *const MEMORY_PRIORITY_INFORMATION).cast(),
                    std::mem::size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
                )
            }
        }
    };
    (ok != 0)
        .then_some(())
        .ok_or_else(|| format!("Recovery mutation failed with error {}.", last_error()))
}

fn open_matching_process(identity: &ProcessIdentity) -> Result<Option<WinHandle>, String> {
    open_matching_process_with_access(
        identity,
        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
    )
}

fn open_matching_process_with_access(
    identity: &ProcessIdentity,
    access: u32,
) -> Result<Option<WinHandle>, String> {
    // SAFETY: identity.id came from a persisted validated process handle.
    let handle = unsafe { OpenProcess(access, 0, identity.id) };
    if handle.is_null() {
        return match last_error() {
            ERROR_INVALID_PARAMETER => Ok(None),
            error => Err(format!(
                "OpenProcess({}) failed with error {error}.",
                identity.id
            )),
        };
    }
    let handle = WinHandle::new(handle);
    let mut exit_code = 0;
    // SAFETY: handle is live and opened with process query access; exit_code is writable.
    if unsafe { GetExitCodeProcess(handle.raw(), &mut exit_code) } == 0 {
        return Err(format!(
            "GetExitCodeProcess({}) failed with error {}.",
            identity.id,
            last_error()
        ));
    }
    if exit_code != STILL_ACTIVE as u32 {
        return Ok(None);
    }
    if process_creation_time(handle.raw())? != identity.creation_time
        || !same_executable_path(
            Path::new(&process_executable_path(handle.raw())?),
            Path::new(&identity.executable_path),
        )
    {
        Ok(None)
    } else {
        Ok(Some(handle))
    }
}

fn process_identity(handle: HANDLE) -> Result<ProcessIdentity, String> {
    // SAFETY: handle is live and opened with query access.
    let id = unsafe { GetProcessId(handle) };
    if id == 0 {
        return Err(format!("GetProcessId failed with error {}.", last_error()));
    }
    Ok(ProcessIdentity {
        id,
        creation_time: process_creation_time(handle)?,
        executable_path: process_executable_path(handle)?,
    })
}

fn process_creation_time(handle: HANDLE) -> Result<u64, String> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: every FILETIME output is writable for this live process handle.
    let ok = unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
    (ok != 0)
        .then_some(filetime_to_u64(creation))
        .ok_or_else(|| format!("GetProcessTimes failed with error {}.", last_error()))
}

fn process_executable_path(handle: HANDLE) -> Result<String, String> {
    let mut buffer = vec![0_u16; 32_768];
    let mut length = buffer.len() as u32;
    // SAFETY: buffer provides length writable UTF-16 units for this live process handle.
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) };
    if ok == 0 {
        return Err(format!(
            "QueryFullProcessImageNameW failed with error {}.",
            last_error()
        ));
    }
    buffer.truncate(length as usize);
    Ok(String::from_utf16_lossy(&buffer))
}

fn thread_id(handle: HANDLE) -> Result<u32, String> {
    // SAFETY: handle is live and opened with query access.
    let id = unsafe { GetThreadId(handle) };
    (id != 0)
        .then_some(id)
        .ok_or_else(|| format!("GetThreadId failed with error {}.", last_error()))
}

fn thread_creation_time(handle: HANDLE) -> Result<u64, String> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: every FILETIME output is writable for this live thread handle.
    let ok = unsafe { GetThreadTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
    (ok != 0)
        .then_some(filetime_to_u64(creation))
        .ok_or_else(|| format!("GetThreadTimes failed with error {}.", last_error()))
}

fn query_cpu_sets(handle: HANDLE) -> Result<Vec<u32>, String> {
    let mut required = 0;
    // SAFETY: a null buffer with zero capacity requests the required count.
    let probe_ok = unsafe { GetProcessDefaultCpuSets(handle, null_mut(), 0, &mut required) };
    if probe_ok == 0 {
        let error = last_error();
        if error != ERROR_INSUFFICIENT_BUFFER {
            return Err(format!(
                "GetProcessDefaultCpuSets failed with error {error}."
            ));
        }
    }
    if required == 0 {
        return Ok(Vec::new());
    }
    let mut ids = vec![0_u32; required as usize];
    // SAFETY: ids provides required writable entries.
    let ok = unsafe {
        GetProcessDefaultCpuSets(handle, ids.as_mut_ptr(), ids.len() as u32, &mut required)
    };
    if ok == 0 {
        return Err(format!(
            "GetProcessDefaultCpuSets failed with error {}.",
            last_error()
        ));
    }
    ids.truncate(required as usize);
    Ok(ids)
}

fn thaw_job(name: &str) -> Result<(), String> {
    let handle = open_job(name, JOB_OBJECT_SET_ATTRIBUTES)?;
    // The recovery helper opened and retained this exact named Job Object before acknowledging
    // Begin. Its root process may have exited while inherited children remain frozen, so root
    // identity is not a prerequisite for thawing the helper-owned job.
    let mut info = JobObjectFreezeInformation::new(false);
    // SAFETY: handle is live and info is writable for exactly the supplied structure size.
    let ok = unsafe {
        SetInformationJobObject(
            handle.raw(),
            JOB_OBJECT_FREEZE_INFORMATION_CLASS,
            (&mut info as *mut JobObjectFreezeInformation).cast(),
            std::mem::size_of::<JobObjectFreezeInformation>() as u32,
        )
    };
    (ok != 0)
        .then_some(())
        .ok_or_else(|| format!("Thawing suspended job failed with error {}.", last_error()))
}

fn open_job(name: &str, desired_access: u32) -> Result<WinHandle, String> {
    let wide = name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: wide is terminated UTF-16 and the returned handle is owned here.
    let handle = unsafe { OpenJobObjectW(desired_access, 0, wide.as_ptr()) };
    if handle.is_null() {
        return Err(format!(
            "OpenJobObjectW failed with error {}.",
            last_error()
        ));
    }
    Ok(WinHandle::new(handle))
}

fn spawn_watchdog() -> Result<(Child, ChildStdin, BufReader<ChildStdout>), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("Failed to resolve the Winderust executable: {error}"))?;
    let mut child = Command::new(executable)
        .arg(WATCHDOG_ARGUMENT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| format!("Failed to start the recovery watchdog: {error}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "The recovery watchdog stdin pipe is unavailable.".to_owned())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "The recovery watchdog stdout pipe is unavailable.".to_owned())?;
    Ok((child, stdin, BufReader::new(stdout)))
}

#[derive(Debug)]
enum RecoveryTransportError {
    Rejected(String),
    Uncertain(String),
}

type RecoveryRequest = (
    RecoveryCommand,
    mpsc::SyncSender<Result<(), RecoveryTransportError>>,
);

#[derive(Debug)]
struct RecoveryTransport {
    requests: mpsc::Sender<RecoveryRequest>,
    failed: Option<String>,
    timeout: Duration,
}

impl RecoveryTransport {
    fn start(
        mut stdin: impl Write + Send + 'static,
        mut stdout: impl BufRead + Send + 'static,
        timeout: Duration,
    ) -> Result<Self, String> {
        let (requests, incoming) = mpsc::channel::<RecoveryRequest>();
        std::thread::Builder::new()
            .name("recovery-transport".into())
            .spawn(move || {
                for (command, response) in incoming {
                    let result = send_command(&mut stdin, &mut stdout, &command);
                    let _ = response.send(result);
                }
                // Keep the pipes open after an uncertain reply until the runtime ends.
                // Closing stdin earlier could start recovery while controllers still own changes.
            })
            .map_err(|error| format!("Failed to start recovery transport: {error}"))?;
        Ok(Self {
            requests,
            failed: None,
            timeout,
        })
    }

    fn send(&mut self, command: &RecoveryCommand) -> Result<(), String> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        let (response, received) = mpsc::sync_channel(1);
        let result = if self.requests.send((command.clone(), response)).is_err() {
            Err(RecoveryTransportError::Uncertain(
                "Recovery transport stopped.".into(),
            ))
        } else {
            match received.recv_timeout(self.timeout) {
                Ok(result) => result,
                Err(error) => Err(RecoveryTransportError::Uncertain(format!(
                    "Recovery watchdog acknowledgement unavailable: {error}"
                ))),
            }
        };
        match result {
            Ok(()) => Ok(()),
            Err(RecoveryTransportError::Rejected(error)) => Err(error),
            Err(RecoveryTransportError::Uncertain(error)) => {
                let error = format!("{error} Command outcome is unknown; further recovery commands are blocked and outstanding recovery ownership is retained.");
                self.failed = Some(error.clone());
                super::diagnostics::error(&error);
                Err(error)
            }
        }
    }
}

fn send_command(
    stdin: &mut impl Write,
    stdout: &mut impl BufRead,
    command: &RecoveryCommand,
) -> Result<(), RecoveryTransportError> {
    let bytes = serde_json::to_vec(command).map_err(|error| {
        RecoveryTransportError::Rejected(format!(
            "Failed to serialize crash recovery state: {error}"
        ))
    })?;
    stdin
        .write_all(&bytes)
        .and_then(|()| stdin.write_all(b"\n"))
        .and_then(|()| stdin.flush())
        .map_err(|error| {
            RecoveryTransportError::Uncertain(format!(
                "Failed to update the recovery watchdog: {error}"
            ))
        })?;
    let mut response = String::new();
    stdout.read_line(&mut response).map_err(|error| {
        RecoveryTransportError::Uncertain(format!(
            "Failed to read the recovery watchdog response: {error}"
        ))
    })?;
    match response.trim_end() {
        "ok" => Ok(()),
        response if response.starts_with("error:") => {
            Err(RecoveryTransportError::Rejected(response[6..].to_owned()))
        }
        response => Err(RecoveryTransportError::Uncertain(format!(
            "Invalid recovery watchdog response: {response}"
        ))),
    }
}

fn nt_success(status: i32, operation: &str) -> Result<(), String> {
    (status >= 0)
        .then_some(())
        .ok_or_else(|| format!("{operation} failed with NTSTATUS 0x{:08X}.", status as u32))
}

fn filetime_to_u64(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

fn fnv1a64(input: impl IntoIterator<Item = u16>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for unit in input {
        for byte in unit.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

fn set_startup_error(error: String) {
    super::diagnostics::error(&error);
    let _ = STARTUP_ERROR.set(error);
}

unsafe extern "system" {
    fn NtQueryInformationProcess(
        process_handle: HANDLE,
        information_class: u32,
        information: *mut c_void,
        information_length: u32,
        return_length: *mut u32,
    ) -> i32;
    fn NtSetInformationProcess(
        process_handle: HANDLE,
        information_class: u32,
        information: *mut c_void,
        information_length: u32,
    ) -> i32;
}

#[cfg(test)]
mod tests;
