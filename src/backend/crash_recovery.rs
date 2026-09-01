use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
    io::{BufRead, BufReader, Write},
    os::windows::ffi::OsStrExt,
    os::windows::process::CommandExt,
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    ptr::null_mut,
    sync::{Mutex, MutexGuard, OnceLock},
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

#[cfg(test)]
use windows_sys::Win32::System::Threading::PROCESS_POWER_THROTTLING_EXECUTION_SPEED;

const WATCHDOG_ARGUMENT: &str = "--winderust-recovery-watchdog";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const PROCESS_IO_PRIORITY: u32 = 33;
const THREAD_PRIORITY_ERROR_RETURN: i32 = i32::MAX;

static RUNTIME: Mutex<Option<RecoveryRuntime>> = Mutex::new(None);
static STARTUP_ERROR: OnceLock<String> = OnceLock::new();

#[derive(Debug)]
struct RecoveryRuntime {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    entries: Vec<RecoveryEntry>,
    next_intent_id: u64,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
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
        compact_or_push(&mut runtime.entries, entry);
        send_command(
            &mut runtime.stdin,
            &mut runtime.stdout,
            &RecoveryCommand::Commit { id: self.id },
        )
    }
}

impl Drop for RecoveryIntent {
    fn drop(&mut self) {
        if self.entry.is_some() {
            if let Some(runtime) = self.runtime.as_mut().and_then(|runtime| runtime.as_mut()) {
                let _ = send_command(
                    &mut runtime.stdin,
                    &mut runtime.stdout,
                    &RecoveryCommand::Cancel { id: self.id },
                );
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
    let mut entries = Vec::new();
    let mut pending = Vec::new();
    let mut jobs = HashMap::new();
    let mut output = std::io::stdout();
    for line in BufReader::new(std::io::stdin()).lines() {
        let result = line
            .map_err(|error| format!("Failed to read recovery command: {error}"))
            .and_then(|line| {
                serde_json::from_str::<RecoveryCommand>(&line)
                    .map_err(|error| format!("Invalid recovery command: {error}"))
            })
            .and_then(|command| {
                apply_watchdog_command(command, &mut entries, &mut pending, &mut jobs)
            });
        let response = match result {
            Ok(()) => "ok".to_owned(),
            Err(error) => format!("error:{error}"),
        };
        if writeln!(output, "{response}")
            .and_then(|()| output.flush())
            .is_err()
        {
            break;
        }
    }
    for (_, entry) in pending {
        compact_or_push(&mut entries, entry);
    }
    if !entries.is_empty() {
        if let Err(error) = recover_with_retry(&entries) {
            eprintln!("Winderust crash recovery failed: {error}");
            std::process::exit(2);
        }
    }
    true
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
        RecoveryCommand::ForgetJob { name } => {
            let key = format!("job:{name}");
            entries.retain(|entry| entry.key() != key);
            pending.retain(|(_, entry)| entry.key() != key);
            jobs.remove(&name);
        }
    }
    Ok(())
}

fn recover_with_retry(entries: &[RecoveryEntry]) -> Result<(), String> {
    let mut last_error = None;
    for attempt in 0..3 {
        let recovery = recover_journal(entries);
        let plan_cleanup = restore_stale_adaptive_plans();
        match (recovery, plan_cleanup) {
            (Ok(()), Ok(())) => return Ok(()),
            (recovery, cleanup) => {
                last_error = Some(
                    [recovery.err(), cleanup.err()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
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
    drop(runtime.stdin);
    let status = runtime
        .child
        .wait()
        .map_err(|error| format!("Failed to wait for crash recovery helper: {error}"))?;
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
    let runtime = RecoveryRuntime {
        child,
        stdin,
        stdout,
        entries: Vec::new(),
        next_intent_id: 1,
    };
    RUNTIME
        .lock()
        .map_err(|_| "Crash recovery state is poisoned.".to_owned())?
        .replace(runtime);
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
    send_command(
        &mut runtime.stdin,
        &mut runtime.stdout,
        &RecoveryCommand::ForgetJob {
            name: name.to_owned(),
        },
    )?;
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
    send_command(
        &mut runtime.stdin,
        &mut runtime.stdout,
        &RecoveryCommand::ForgetThreadPriority {
            process_id,
            process_creation_time,
            thread_id,
            thread_creation_time,
        },
    )?;
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
    send_command(
        &mut runtime.stdin,
        &mut runtime.stdout,
        &RecoveryCommand::ForgetThreadSuspension {
            process_id,
            process_creation_time,
            thread_id,
            thread_creation_time,
        },
    )?;
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
    send_command(
        &mut runtime.stdin,
        &mut runtime.stdout,
        &RecoveryCommand::ForgetProcess {
            process_id,
            creation_time,
            value: value.clone(),
        },
    )?;
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
    send_command(
        &mut state.stdin,
        &mut state.stdout,
        &RecoveryCommand::Begin {
            id,
            entry: entry.clone(),
        },
    )?;
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

fn recover_journal(entries: &[RecoveryEntry]) -> Result<(), String> {
    let mut recovered = HashSet::new();
    let mut failures = Vec::new();
    for entry in entries.iter().rev() {
        let key = entry.key();
        if recovered.insert(key.clone()) {
            if let Err(error) = recover_entry(entry, &key, entries) {
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
) -> Result<(), String> {
    match entry {
        RecoveryEntry::Process {
            identity, expected, ..
        } => recover_process_key(key, identity, expected, entries),
        RecoveryEntry::ThreadPriority {
            process,
            thread_id,
            thread_creation_time,
            ..
        } => recover_thread_key(key, process, *thread_id, *thread_creation_time, entries),
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
        RecoveryEntry::PowerPlan { .. } => recover_power_plan_key(key, entries),
        RecoveryEntry::SuspendedJob { name, .. } => thaw_job(name),
    }
}

fn recover_process_key(
    key: &str,
    identity: &ProcessIdentity,
    value_kind: &ProcessValue,
    entries: &[RecoveryEntry],
) -> Result<(), String> {
    let Some(process) = open_matching_process(identity)? else {
        return Ok(());
    };
    let current = query_process_value(process.raw(), value_kind)?;
    let desired = unwind_process_value(key, &current, entries);
    if desired != current {
        apply_process_value(process.raw(), &desired)?;
    }
    Ok(())
}

fn unwind_process_value(
    key: &str,
    current: &ProcessValue,
    entries: &[RecoveryEntry],
) -> ProcessValue {
    let mut desired = current.clone();
    for entry in entries.iter().rev().filter(|entry| entry.key() == key) {
        if let RecoveryEntry::Process {
            original, expected, ..
        } = entry
        {
            if *expected == desired {
                desired = original.clone();
            } else {
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
) -> Result<(), String> {
    let Some(process_handle) = open_matching_process(process)? else {
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
    let mut desired = current;
    for entry in entries.iter().rev().filter(|entry| entry.key() == key) {
        if let RecoveryEntry::ThreadPriority {
            original, expected, ..
        } = entry
        {
            if *expected == desired {
                desired = *original;
            } else {
                break;
            }
        }
    }
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

fn recover_power_plan_key(key: &str, entries: &[RecoveryEntry]) -> Result<(), String> {
    let current = active_plan()?.guid;
    let mut desired = current.clone();
    for entry in entries.iter().rev().filter(|entry| entry.key() == key) {
        if let RecoveryEntry::PowerPlan {
            original_guid,
            expected_guid,
        } = entry
        {
            if expected_guid.eq_ignore_ascii_case(&desired) {
                desired = original_guid.clone();
            } else {
                break;
            }
        }
    }
    if !desired.eq_ignore_ascii_case(&current) {
        set_active(&desired)?;
    }
    Ok(())
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
            // SAFETY: raw points to exactly the supplied u32 size.
            let status = unsafe {
                NtSetInformationProcess(
                    handle,
                    PROCESS_IO_PRIORITY,
                    (&mut raw as *mut u32).cast(),
                    std::mem::size_of::<u32>() as u32,
                )
            };
            nt_success(status, "NtSetInformationProcess")?;
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

fn send_command(
    stdin: &mut impl Write,
    stdout: &mut impl BufRead,
    command: &RecoveryCommand,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(command)
        .map_err(|error| format!("Failed to serialize crash recovery state: {error}"))?;
    stdin
        .write_all(&bytes)
        .and_then(|()| stdin.write_all(b"\n"))
        .and_then(|()| stdin.flush())
        .map_err(|error| format!("Failed to update the recovery watchdog: {error}"))?;
    let mut response = String::new();
    stdout
        .read_line(&mut response)
        .map_err(|error| format!("Failed to read the recovery watchdog response: {error}"))?;
    match response.trim_end() {
        "ok" => Ok(()),
        response if response.starts_with("error:") => Err(response[6..].to_owned()),
        response => Err(format!("Invalid recovery watchdog response: {response}")),
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
mod tests {
    use super::*;
    use crate::platform::windows::thread_suspension;
    use std::{
        io::Cursor,
        process::{Child, Command, Stdio},
    };

    use windows_sys::Win32::{
        Foundation::{INVALID_HANDLE_VALUE, WAIT_OBJECT_0},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
                THREADENTRY32,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
                QueryInformationJobObject, TerminateJobObject,
                JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            },
            Threading::{
                CreateEventW, CreateProcessW, ResumeThread, WaitForSingleObject,
                BELOW_NORMAL_PRIORITY_CLASS, CREATE_SUSPENDED, IDLE_PRIORITY_CLASS,
                PROCESS_INFORMATION, PROCESS_QUERY_INFORMATION, STARTUPINFOW,
                THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_LOWEST,
            },
        },
    };

    struct FrozenJobCleanup {
        job: WinHandle,
    }

    impl Drop for FrozenJobCleanup {
        fn drop(&mut self) {
            let mut thaw = JobObjectFreezeInformation::new(false);
            // SAFETY: the test owns this live Job Object handle and thaw has the exact buffer
            // layout required by the same contract exercised by the test.
            let _ = unsafe {
                SetInformationJobObject(
                    self.job.raw(),
                    JOB_OBJECT_FREEZE_INFORMATION_CLASS,
                    (&mut thaw as *mut JobObjectFreezeInformation).cast(),
                    std::mem::size_of::<JobObjectFreezeInformation>() as u32,
                )
            };
            // SAFETY: the test owns this disposable Job Object and terminates only its test tree.
            let _ = unsafe { TerminateJobObject(self.job.raw(), 1) };
        }
    }

    fn resume_disposable_root(process: &WinHandle, thread: &WinHandle) {
        // SAFETY: both handles belong to the suspended disposable process created by the test.
        let _ = unsafe { ResumeThread(thread.raw()) };
        // SAFETY: process is a live disposable handle; this bounded wait retains no pointer.
        let _ = unsafe { WaitForSingleObject(process.raw(), 5_000) };
    }

    struct DisposableChild {
        child: Child,
    }

    impl DisposableChild {
        fn spawn() -> Result<Self, String> {
            let system_root = std::env::var_os("SystemRoot")
                .ok_or_else(|| "SystemRoot is unavailable.".to_owned())?;
            let executable = Path::new(&system_root).join("System32").join("ping.exe");
            let child = Command::new(executable)
                .args(["127.0.0.1", "-n", "120", "-w", "1000"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| format!("Failed to start disposable test process: {error}"))?;
            Ok(Self { child })
        }

        fn process_handle(&self) -> Result<WinHandle, String> {
            // SAFETY: child.id identifies the disposable process created by this test.
            let handle = unsafe {
                OpenProcess(
                    PROCESS_QUERY_INFORMATION
                        | PROCESS_QUERY_LIMITED_INFORMATION
                        | PROCESS_SET_INFORMATION,
                    0,
                    self.child.id(),
                )
            };
            (!handle.is_null())
                .then(|| WinHandle::new(handle))
                .ok_or_else(|| {
                    format!(
                        "OpenProcess(test child) failed with error {}.",
                        last_error()
                    )
                })
        }
    }

    impl Drop for DisposableChild {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn test_runtime(mut child: Child) -> Result<RecoveryRuntime, String> {
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "The recovery helper stdin pipe is unavailable.".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "The recovery helper stdout pipe is unavailable.".to_owned())?;
        Ok(RecoveryRuntime {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            entries: Vec::new(),
            next_intent_id: 1,
        })
    }

    fn spawn_test_helper(command: &str, args: &[&str]) -> Result<Child, String> {
        Command::new("cmd")
            .arg("/C")
            .arg(format!("{command} {}", args.join(" ")))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Failed to spawn test recovery helper: {error}"))
    }

    fn test_thread_handle(process_id: u32) -> Result<WinHandle, String> {
        // SAFETY: a thread snapshot takes no borrowed pointers and returns an owned handle.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(format!(
                "CreateToolhelp32Snapshot failed with error {}.",
                last_error()
            ));
        }
        let snapshot = WinHandle::new(snapshot);
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        // SAFETY: snapshot is live and entry has its required initialized size.
        let mut present = unsafe { Thread32First(snapshot.raw(), &mut entry) } != 0;
        while present {
            if entry.th32OwnerProcessID == process_id {
                // SAFETY: the thread ID came from the current system snapshot.
                let thread = unsafe {
                    OpenThread(
                        THREAD_QUERY_INFORMATION | THREAD_SET_INFORMATION,
                        0,
                        entry.th32ThreadID,
                    )
                };
                if !thread.is_null() {
                    return Ok(WinHandle::new(thread));
                }
            }
            // SAFETY: snapshot remains live and entry remains writable for the next record.
            present = unsafe { Thread32Next(snapshot.raw(), &mut entry) } != 0;
        }
        Err("No queryable thread was found for the disposable test process.".to_owned())
    }

    fn identity() -> ProcessIdentity {
        ProcessIdentity {
            id: 7,
            creation_time: 11,
            executable_path: "C:\\app.exe".to_owned(),
        }
    }

    fn captured_test_thread(
        process: &WinHandle,
    ) -> Result<thread_suspension::CapturedThread, String> {
        thread_suspension::capture_threads(process.raw())
            .map_err(|error| format!("{error:?}"))?
            .into_iter()
            .next()
            .ok_or_else(|| "No thread was captured for the disposable test process.".to_owned())
    }

    #[test]
    fn recovery_client_finish_is_idempotent_without_an_active_runtime() {
        let mut client = RecoveryClient { active: false };
        assert!(client.finish().is_ok());
        assert!(client.finish().is_ok());
    }

    #[test]
    fn recovery_runtime_finish_accepts_successful_helper_exit() -> Result<(), String> {
        let runtime = test_runtime(spawn_test_helper("exit", &["0"])?)?;
        finish_recovery_runtime(runtime)
    }

    #[test]
    fn recovery_runtime_finish_reports_helper_exit_code() -> Result<(), String> {
        let helper = spawn_test_helper("exit", &["11"])?;
        let result = finish_recovery_runtime(test_runtime(helper)?);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Crash recovery helper exited unexpectedly with status code 11."));
        Ok(())
    }

    #[test]
    fn consecutive_process_changes_compact_to_the_original_baseline() {
        let mut entries = Vec::new();
        compact_or_push(
            &mut entries,
            RecoveryEntry::Process {
                identity: identity(),
                original: ProcessValue::PriorityClass(1),
                expected: ProcessValue::PriorityClass(2),
            },
        );
        compact_or_push(
            &mut entries,
            RecoveryEntry::Process {
                identity: identity(),
                original: ProcessValue::PriorityClass(2),
                expected: ProcessValue::PriorityClass(3),
            },
        );
        assert_eq!(entries.len(), 1);
        assert!(matches!(
            &entries[0],
            RecoveryEntry::Process {
                original: ProcessValue::PriorityClass(1),
                expected: ProcessValue::PriorityClass(3),
                ..
            }
        ));
    }

    #[test]
    fn external_state_break_starts_a_new_recovery_segment() {
        let mut entries = Vec::new();
        compact_or_push(
            &mut entries,
            RecoveryEntry::Process {
                identity: identity(),
                original: ProcessValue::PriorityClass(1),
                expected: ProcessValue::PriorityClass(2),
            },
        );
        compact_or_push(
            &mut entries,
            RecoveryEntry::Process {
                identity: identity(),
                original: ProcessValue::PriorityClass(4),
                expected: ProcessValue::PriorityClass(3),
            },
        );
        assert_eq!(entries.len(), 2);
        let key = entries[0].key();
        assert_eq!(
            unwind_process_value(&key, &ProcessValue::PriorityClass(3), &entries),
            ProcessValue::PriorityClass(4)
        );
        assert_eq!(
            unwind_process_value(&key, &ProcessValue::PriorityClass(2), &entries),
            ProcessValue::PriorityClass(2)
        );
    }

    #[test]
    fn returning_to_the_baseline_removes_the_recovery_entry() {
        let mut entries = Vec::new();
        compact_or_push(
            &mut entries,
            RecoveryEntry::Process {
                identity: identity(),
                original: ProcessValue::PriorityClass(1),
                expected: ProcessValue::PriorityClass(2),
            },
        );
        compact_or_push(
            &mut entries,
            RecoveryEntry::Process {
                identity: identity(),
                original: ProcessValue::PriorityClass(2),
                expected: ProcessValue::PriorityClass(1),
            },
        );
        assert!(entries.is_empty());
    }

    #[test]
    fn every_process_value_kind_compacts_and_returns_to_its_baseline() {
        let cases = [
            (
                ProcessValue::PriorityClass(1),
                ProcessValue::PriorityClass(2),
                ProcessValue::PriorityClass(3),
            ),
            (
                ProcessValue::PowerThrottling {
                    version: 1,
                    control_mask: 1,
                    state_mask: 0,
                },
                ProcessValue::PowerThrottling {
                    version: 1,
                    control_mask: 1,
                    state_mask: 1,
                },
                ProcessValue::PowerThrottling {
                    version: 1,
                    control_mask: 3,
                    state_mask: 1,
                },
            ),
            (
                ProcessValue::Affinity(1),
                ProcessValue::Affinity(3),
                ProcessValue::Affinity(7),
            ),
            (
                ProcessValue::CpuSets(vec![]),
                ProcessValue::CpuSets(vec![1]),
                ProcessValue::CpuSets(vec![1, 2]),
            ),
            (
                ProcessValue::DynamicPriorityBoostDisabled(false),
                ProcessValue::DynamicPriorityBoostDisabled(true),
                ProcessValue::DynamicPriorityBoostDisabled(true),
            ),
            (
                ProcessValue::IoPriority(0),
                ProcessValue::IoPriority(1),
                ProcessValue::IoPriority(2),
            ),
            (
                ProcessValue::GpuPriority(0),
                ProcessValue::GpuPriority(1),
                ProcessValue::GpuPriority(2),
            ),
            (
                ProcessValue::MemoryPriority(1),
                ProcessValue::MemoryPriority(2),
                ProcessValue::MemoryPriority(3),
            ),
        ];

        for (baseline, middle, final_value) in cases {
            let mut entries = Vec::new();
            compact_or_push(
                &mut entries,
                RecoveryEntry::Process {
                    identity: identity(),
                    original: baseline.clone(),
                    expected: middle.clone(),
                },
            );
            compact_or_push(
                &mut entries,
                RecoveryEntry::Process {
                    identity: identity(),
                    original: middle,
                    expected: final_value.clone(),
                },
            );
            assert_eq!(entries.len(), usize::from(baseline != final_value));
            compact_or_push(
                &mut entries,
                RecoveryEntry::Process {
                    identity: identity(),
                    original: final_value,
                    expected: baseline,
                },
            );
            assert!(entries.is_empty());
        }
    }

    #[test]
    fn thread_and_power_plan_changes_compact_to_the_original_baseline() {
        let mut thread_entries = Vec::new();
        compact_or_push(
            &mut thread_entries,
            RecoveryEntry::ThreadPriority {
                process: identity(),
                thread_id: 13,
                thread_creation_time: 17,
                original: 0,
                expected: 1,
            },
        );
        compact_or_push(
            &mut thread_entries,
            RecoveryEntry::ThreadPriority {
                process: identity(),
                thread_id: 13,
                thread_creation_time: 17,
                original: 1,
                expected: 2,
            },
        );
        assert!(matches!(
            thread_entries.as_slice(),
            [RecoveryEntry::ThreadPriority {
                original: 0,
                expected: 2,
                ..
            }]
        ));
        compact_or_push(
            &mut thread_entries,
            RecoveryEntry::ThreadPriority {
                process: identity(),
                thread_id: 13,
                thread_creation_time: 17,
                original: 2,
                expected: 0,
            },
        );
        assert!(thread_entries.is_empty());

        let mut plan_entries = Vec::new();
        compact_or_push(
            &mut plan_entries,
            RecoveryEntry::PowerPlan {
                original_guid: "plan-a".to_owned(),
                expected_guid: "plan-b".to_owned(),
            },
        );
        compact_or_push(
            &mut plan_entries,
            RecoveryEntry::PowerPlan {
                original_guid: "PLAN-B".to_owned(),
                expected_guid: "plan-c".to_owned(),
            },
        );
        assert!(matches!(
            plan_entries.as_slice(),
            [RecoveryEntry::PowerPlan {
                original_guid,
                expected_guid,
            }] if original_guid == "plan-a" && expected_guid == "plan-c"
        ));
        compact_or_push(
            &mut plan_entries,
            RecoveryEntry::PowerPlan {
                original_guid: "PLAN-C".to_owned(),
                expected_guid: "PLAN-A".to_owned(),
            },
        );
        assert!(plan_entries.is_empty());
    }

    #[test]
    fn thread_suspension_resume_gate_requires_exactly_one_owned_count() {
        assert!(should_resume_thread_suspension(4, 5, 5));
        assert!(!should_resume_thread_suspension(4, 5, 4));
        assert!(!should_resume_thread_suspension(4, 5, 6));
        assert!(!should_resume_thread_suspension(4, 6, 6));
        assert!(!should_resume_thread_suspension(u16::MAX, 0, 0));
    }

    #[test]
    fn thread_suspension_begin_is_pending_before_mutation_commit() {
        let entry = RecoveryEntry::ThreadSuspension {
            process: identity(),
            thread_id: 13,
            thread_creation_time: 17,
            original_suspend_count: 0,
            expected_suspend_count: 1,
        };
        let mut entries = Vec::new();
        let mut pending = Vec::new();
        let mut jobs = HashMap::new();

        apply_watchdog_command_with_open_job(
            RecoveryCommand::Begin {
                id: 1,
                entry: entry.clone(),
            },
            &mut entries,
            &mut pending,
            &mut jobs,
            |_, _| Err("thread suspension must not open jobs".to_owned()),
        )
        .unwrap();
        assert!(entries.is_empty());
        assert_eq!(pending, vec![(1, entry.clone())]);

        apply_watchdog_command_with_open_job(
            RecoveryCommand::Commit { id: 1 },
            &mut entries,
            &mut pending,
            &mut jobs,
            |_, _| Err("commit must not open jobs".to_owned()),
        )
        .unwrap();
        assert_eq!(entries, vec![entry]);
        assert!(pending.is_empty());
    }

    #[test]
    fn record_thread_suspension_rejects_suspend_count_overflow_before_handle_queries() {
        match record_thread_suspension(std::ptr::null_mut(), std::ptr::null_mut(), u16::MAX) {
            Ok(_) => panic!("overflow must be rejected before either handle is queried"),
            Err(error) => assert_eq!(error, "Thread suspend count cannot exceed 65535."),
        }
    }

    #[test]
    fn watchdog_commands_cover_begin_cancel_commit_replacement_and_clean_release() {
        let entry = RecoveryEntry::Process {
            identity: identity(),
            original: ProcessValue::PriorityClass(1),
            expected: ProcessValue::PriorityClass(2),
        };
        let mut entries = Vec::new();
        let mut pending = Vec::new();
        let mut jobs = HashMap::new();

        apply_watchdog_command_with_open_job(
            RecoveryCommand::Begin {
                id: 1,
                entry: entry.clone(),
            },
            &mut entries,
            &mut pending,
            &mut jobs,
            |_, _| Err("process entries must not open jobs".to_owned()),
        )
        .unwrap();
        assert_eq!(pending, vec![(1, entry.clone())]);
        apply_watchdog_command_with_open_job(
            RecoveryCommand::Cancel { id: 1 },
            &mut entries,
            &mut pending,
            &mut jobs,
            |_, _| Err("cancel must not open jobs".to_owned()),
        )
        .unwrap();
        assert!(pending.is_empty());
        assert!(entries.is_empty());

        apply_watchdog_command_with_open_job(
            RecoveryCommand::Begin {
                id: 2,
                entry: entry.clone(),
            },
            &mut entries,
            &mut pending,
            &mut jobs,
            |_, _| Err("process entries must not open jobs".to_owned()),
        )
        .unwrap();
        apply_watchdog_command_with_open_job(
            RecoveryCommand::Commit { id: 2 },
            &mut entries,
            &mut pending,
            &mut jobs,
            |_, _| Err("commit must not open jobs".to_owned()),
        )
        .unwrap();
        assert_eq!(entries.len(), 1);

        let release = RecoveryEntry::Process {
            identity: identity(),
            original: ProcessValue::PriorityClass(2),
            expected: ProcessValue::PriorityClass(1),
        };
        apply_watchdog_command_with_open_job(
            RecoveryCommand::Begin {
                id: 3,
                entry: release,
            },
            &mut entries,
            &mut pending,
            &mut jobs,
            |_, _| Err("process entries must not open jobs".to_owned()),
        )
        .unwrap();
        apply_watchdog_command_with_open_job(
            RecoveryCommand::Commit { id: 3 },
            &mut entries,
            &mut pending,
            &mut jobs,
            |_, _| Err("commit must not open jobs".to_owned()),
        )
        .unwrap();
        assert!(entries.is_empty());
        assert!(pending.is_empty());
    }

    #[test]
    fn suspended_job_begin_requires_a_retained_handle_before_acknowledgement() {
        let entry = RecoveryEntry::SuspendedJob {
            name: "Local\\Winderust.Suspend.test".to_owned(),
            process: identity(),
        };
        let mut entries = Vec::new();
        let mut pending = Vec::new();
        let mut jobs = HashMap::new();

        let error = apply_watchdog_command_with_open_job(
            RecoveryCommand::Begin {
                id: 1,
                entry: entry.clone(),
            },
            &mut entries,
            &mut pending,
            &mut jobs,
            |_, _| Err("job open failed".to_owned()),
        )
        .unwrap_err();
        assert_eq!(error, "job open failed");
        assert!(pending.is_empty());
        assert!(jobs.is_empty());

        apply_watchdog_command_with_open_job(
            RecoveryCommand::Begin {
                id: 2,
                entry: entry.clone(),
            },
            &mut entries,
            &mut pending,
            &mut jobs,
            |_, _| {
                // SAFETY: null security attributes/name are allowed; the returned owned event
                // handle is used only as a deterministic stand-in for the retained Job handle.
                let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
                (!handle.is_null())
                    .then(|| WinHandle::new(handle))
                    .ok_or_else(|| "CreateEventW failed".to_owned())
            },
        )
        .unwrap();
        assert_eq!(pending, vec![(2, entry)]);
        assert!(jobs.contains_key("Local\\Winderust.Suspend.test"));
        apply_watchdog_command_with_open_job(
            RecoveryCommand::Commit { id: 2 },
            &mut entries,
            &mut pending,
            &mut jobs,
            |_, _| Err("commit must not open jobs".to_owned()),
        )
        .unwrap();
        assert_eq!(entries.len(), 1);
        apply_watchdog_command_with_open_job(
            RecoveryCommand::ForgetJob {
                name: "Local\\Winderust.Suspend.test".to_owned(),
            },
            &mut entries,
            &mut pending,
            &mut jobs,
            |_, _| Err("forget must not open jobs".to_owned()),
        )
        .unwrap();
        assert!(entries.is_empty());
        assert!(jobs.is_empty());
    }

    #[test]
    fn forget_process_removes_only_the_exact_instance_property() {
        let process = identity();
        let boost = RecoveryEntry::Process {
            identity: process.clone(),
            original: ProcessValue::DynamicPriorityBoostDisabled(false),
            expected: ProcessValue::DynamicPriorityBoostDisabled(true),
        };
        let priority = RecoveryEntry::Process {
            identity: process.clone(),
            original: ProcessValue::PriorityClass(1),
            expected: ProcessValue::PriorityClass(2),
        };
        let mut replacement = process.clone();
        replacement.creation_time += 1;
        let replacement_boost = RecoveryEntry::Process {
            identity: replacement,
            original: ProcessValue::DynamicPriorityBoostDisabled(false),
            expected: ProcessValue::DynamicPriorityBoostDisabled(true),
        };
        let mut entries = vec![boost.clone(), priority.clone()];
        let mut pending = vec![(7, boost), (8, replacement_boost.clone())];
        let mut jobs = HashMap::new();

        apply_watchdog_command_with_open_job(
            RecoveryCommand::ForgetProcess {
                process_id: process.id,
                creation_time: process.creation_time,
                value: ProcessValue::DynamicPriorityBoostDisabled(false),
            },
            &mut entries,
            &mut pending,
            &mut jobs,
            |_, _| Err("forget process must not open jobs".to_owned()),
        )
        .unwrap();

        assert_eq!(entries, vec![priority]);
        assert_eq!(pending, vec![(8, replacement_boost)]);
    }

    #[test]
    fn forget_thread_priority_removes_only_the_exact_thread_instance() {
        let process = identity();
        let target = RecoveryEntry::ThreadPriority {
            process: process.clone(),
            thread_id: 13,
            thread_creation_time: 17,
            original: 0,
            expected: 1,
        };
        let replacement_thread = RecoveryEntry::ThreadPriority {
            process: process.clone(),
            thread_id: 13,
            thread_creation_time: 18,
            original: 0,
            expected: 1,
        };
        let other_thread = RecoveryEntry::ThreadPriority {
            process: process.clone(),
            thread_id: 14,
            thread_creation_time: 17,
            original: 0,
            expected: 1,
        };
        let mut entries = vec![target.clone(), replacement_thread.clone()];
        let mut pending = vec![(7, target), (8, other_thread.clone())];
        let mut jobs = HashMap::new();

        apply_watchdog_command_with_open_job(
            RecoveryCommand::ForgetThreadPriority {
                process_id: process.id,
                process_creation_time: process.creation_time,
                thread_id: 13,
                thread_creation_time: 17,
            },
            &mut entries,
            &mut pending,
            &mut jobs,
            |_, _| Err("forget thread priority must not open jobs".to_owned()),
        )
        .unwrap();

        assert_eq!(entries, vec![replacement_thread]);
        assert_eq!(pending, vec![(8, other_thread)]);
    }

    #[test]
    fn forget_thread_suspension_preserves_reused_thread_and_thread_priority() {
        let process = identity();
        let target = RecoveryEntry::ThreadSuspension {
            process: process.clone(),
            thread_id: 13,
            thread_creation_time: 17,
            original_suspend_count: 0,
            expected_suspend_count: 1,
        };
        let reused = RecoveryEntry::ThreadSuspension {
            process: process.clone(),
            thread_id: 13,
            thread_creation_time: 18,
            original_suspend_count: 0,
            expected_suspend_count: 1,
        };
        let priority = RecoveryEntry::ThreadPriority {
            process: process.clone(),
            thread_id: 13,
            thread_creation_time: 17,
            original: 0,
            expected: 1,
        };
        let mut entries = vec![target.clone(), reused.clone(), priority.clone()];
        let mut pending = vec![(7, target), (8, reused.clone()), (9, priority.clone())];
        let mut jobs = HashMap::new();

        apply_watchdog_command_with_open_job(
            RecoveryCommand::ForgetThreadSuspension {
                process_id: process.id,
                process_creation_time: process.creation_time,
                thread_id: 13,
                thread_creation_time: 17,
            },
            &mut entries,
            &mut pending,
            &mut jobs,
            |_, _| Err("forget thread suspension must not open jobs".to_owned()),
        )
        .unwrap();

        assert_eq!(entries, vec![reused.clone(), priority.clone()]);
        assert_eq!(pending, vec![(8, reused), (9, priority)]);
    }

    #[test]
    fn recovery_transport_requires_an_acknowledgement_before_returning_success() {
        let command = RecoveryCommand::Begin {
            id: 9,
            entry: RecoveryEntry::Process {
                identity: identity(),
                original: ProcessValue::PriorityClass(1),
                expected: ProcessValue::PriorityClass(2),
            },
        };
        let mut written = Vec::new();
        let mut ok = Cursor::new(b"ok\n");
        send_command(&mut written, &mut ok, &command).unwrap();
        let encoded = String::from_utf8(written).unwrap();
        assert!(encoded.ends_with('\n'));
        assert_eq!(
            serde_json::from_str::<RecoveryCommand>(encoded.trim()).unwrap(),
            command
        );

        let mut written = Vec::new();
        let mut rejected = Cursor::new(b"error:watchdog rejected Begin\n");
        assert_eq!(
            send_command(&mut written, &mut rejected, &command).unwrap_err(),
            "watchdog rejected Begin"
        );

        let mut written = Vec::new();
        let mut invalid = Cursor::new(b"maybe\n");
        assert_eq!(
            send_command(&mut written, &mut invalid, &command).unwrap_err(),
            "Invalid recovery watchdog response: maybe"
        );
    }

    #[test]
    fn failed_clean_process_release_keeps_journal_for_helper_recovery() -> Result<(), String> {
        let child = DisposableChild::spawn()?;
        let process = child.process_handle()?;
        let original = query_process_value(process.raw(), &ProcessValue::PriorityClass(0))?;
        let original_priority = match original {
            ProcessValue::PriorityClass(priority) => priority,
            _ => unreachable!("priority query must produce a priority value"),
        };
        let expected_priority = if original_priority != BELOW_NORMAL_PRIORITY_CLASS {
            BELOW_NORMAL_PRIORITY_CLASS
        } else {
            IDLE_PRIORITY_CLASS
        };
        let expected = ProcessValue::PriorityClass(expected_priority);
        let original = ProcessValue::PriorityClass(original_priority);
        let entry = RecoveryEntry::Process {
            identity: process_identity(process.raw())?,
            original: original.clone(),
            expected: expected.clone(),
        };

        apply_process_value(process.raw(), &expected)?;
        assert_eq!(
            query_process_value(process.raw(), &expected)?,
            expected,
            "the disposable process must reach the journal's expected state"
        );

        let mut entries = Vec::new();
        let mut pending = Vec::new();
        let mut jobs = HashMap::new();
        apply_watchdog_command(
            RecoveryCommand::Begin {
                id: 1,
                entry: entry.clone(),
            },
            &mut entries,
            &mut pending,
            &mut jobs,
        )?;
        apply_watchdog_command(
            RecoveryCommand::Commit { id: 1 },
            &mut entries,
            &mut pending,
            &mut jobs,
        )?;

        let failed_release = RecoveryEntry::Process {
            identity: process_identity(process.raw())?,
            original: expected.clone(),
            expected: original.clone(),
        };
        apply_watchdog_command(
            RecoveryCommand::Begin {
                id: 2,
                entry: failed_release,
            },
            &mut entries,
            &mut pending,
            &mut jobs,
        )?;
        // A clean restore that fails drops its uncommitted intent, which sends Cancel. The
        // earlier committed mutation must remain available to the helper when its pipe closes.
        apply_watchdog_command(
            RecoveryCommand::Cancel { id: 2 },
            &mut entries,
            &mut pending,
            &mut jobs,
        )?;
        assert_eq!(entries, vec![entry]);
        assert!(pending.is_empty());

        recover_journal(&entries)?;
        assert_eq!(
            query_process_value(process.raw(), &expected)?,
            original,
            "recovery must restore the captured process baseline"
        );
        Ok(())
    }

    #[test]
    fn power_throttling_recovery_restores_a_disposable_process() -> Result<(), String> {
        let child = DisposableChild::spawn()?;
        let process = child.process_handle()?;
        let probe = ProcessValue::PowerThrottling {
            version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            control_mask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            state_mask: 0,
        };
        let original = query_process_value(process.raw(), &probe)?;
        let ProcessValue::PowerThrottling {
            version,
            control_mask,
            state_mask,
        } = original
        else {
            unreachable!("power-throttling query must produce a power value")
        };
        let expected = ProcessValue::PowerThrottling {
            version,
            control_mask: control_mask | PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            state_mask: state_mask ^ PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        };
        let original = ProcessValue::PowerThrottling {
            version,
            control_mask,
            state_mask,
        };
        let entry = RecoveryEntry::Process {
            identity: process_identity(process.raw())?,
            original: original.clone(),
            expected: expected.clone(),
        };

        apply_process_value(process.raw(), &expected)?;
        assert_eq!(query_process_value(process.raw(), &expected)?, expected);
        recover_journal(&[entry])?;
        assert_eq!(query_process_value(process.raw(), &original)?, original);
        Ok(())
    }

    #[test]
    fn dynamic_priority_boost_recovery_restores_a_disposable_process() -> Result<(), String> {
        let child = DisposableChild::spawn()?;
        let process = child.process_handle()?;
        let probe = ProcessValue::DynamicPriorityBoostDisabled(false);
        let original = query_process_value(process.raw(), &probe)?;
        let ProcessValue::DynamicPriorityBoostDisabled(original_disabled) = original else {
            unreachable!("dynamic priority boost query must produce a boost value");
        };
        let original = ProcessValue::DynamicPriorityBoostDisabled(original_disabled);
        let expected = ProcessValue::DynamicPriorityBoostDisabled(!original_disabled);
        let entry = RecoveryEntry::Process {
            identity: process_identity(process.raw())?,
            original: original.clone(),
            expected: expected.clone(),
        };

        apply_process_value(process.raw(), &expected)?;
        assert_eq!(query_process_value(process.raw(), &expected)?, expected);

        recover_journal(&[entry])?;

        assert_eq!(query_process_value(process.raw(), &original)?, original);
        Ok(())
    }

    #[test]
    #[allow(unknown_lints, clippy::manual_isolate_lowest_one)]
    fn affinity_recovery_restores_a_disposable_process() -> Result<(), String> {
        let child = DisposableChild::spawn()?;
        let process = child.process_handle()?;
        let probe = ProcessValue::Affinity(0);
        let original = query_process_value(process.raw(), &probe)?;
        let ProcessValue::Affinity(original_mask) = original else {
            unreachable!("affinity query must produce an affinity value");
        };
        if original_mask.count_ones() <= 1 {
            return Ok(());
        }
        let expected = ProcessValue::Affinity(original_mask & original_mask.wrapping_neg());
        let original = ProcessValue::Affinity(original_mask);
        let entry = RecoveryEntry::Process {
            identity: process_identity(process.raw())?,
            original: original.clone(),
            expected: expected.clone(),
        };

        apply_process_value(process.raw(), &expected)?;
        let recovery_result = recover_journal(&[entry]);
        let observed = query_process_value(process.raw(), &original);
        let cleanup_result = match &observed {
            Ok(value) if value == &original => Ok(()),
            _ => apply_process_value(process.raw(), &original),
        };
        recovery_result?;
        let observed = observed?;
        cleanup_result?;
        if observed != original {
            return Err(format!(
                "Processor Affinity recovery returned {observed:?}, expected {original:?}."
            ));
        }
        Ok(())
    }

    #[test]
    fn cpu_sets_recovery_restores_a_disposable_process() -> Result<(), String> {
        let child = DisposableChild::spawn()?;
        let process = child.process_handle()?;
        let probe = ProcessValue::CpuSets(Vec::new());
        let original = query_process_value(process.raw(), &probe)?;
        let ProcessValue::CpuSets(mut original_ids) = original else {
            unreachable!("CPU Sets query must produce CPU Set IDs");
        };
        original_ids.sort_unstable();
        original_ids.dedup();
        let expected_ids = (0..64).find_map(|bit| {
            let mut ids =
                crate::platform::windows::cpu_allocation::cpu_set_ids_for_mask(1_u64 << bit)
                    .ok()?;
            ids.sort_unstable();
            ids.dedup();
            (!ids.is_empty() && ids != original_ids).then_some(ids)
        });
        let Some(expected_ids) = expected_ids else {
            return Ok(());
        };
        let original = ProcessValue::CpuSets(original_ids);
        let expected = ProcessValue::CpuSets(expected_ids);
        let entry = RecoveryEntry::Process {
            identity: process_identity(process.raw())?,
            original: original.clone(),
            expected: expected.clone(),
        };

        apply_process_value(process.raw(), &expected)?;
        let recovery_result = recover_journal(&[entry]);
        let observed = query_process_value(process.raw(), &original);
        let cleanup_result = match &observed {
            Ok(value) if value == &original => Ok(()),
            _ => apply_process_value(process.raw(), &original),
        };
        recovery_result?;
        let observed = observed?;
        cleanup_result?;
        if observed != original {
            return Err(format!(
                "CPU Sets recovery returned {observed:?}, expected {original:?}."
            ));
        }
        Ok(())
    }

    #[test]
    fn io_priority_recovery_restores_a_disposable_process() -> Result<(), String> {
        let child = DisposableChild::spawn()?;
        let process = child.process_handle()?;
        let probe = ProcessValue::IoPriority(0);
        let original = query_process_value(process.raw(), &probe)?;
        let ProcessValue::IoPriority(original_priority) = original else {
            unreachable!("I/O priority query must produce an I/O priority value");
        };
        let original = ProcessValue::IoPriority(original_priority);
        let expected = ProcessValue::IoPriority(if original_priority == 0 { 1 } else { 0 });
        let entry = RecoveryEntry::Process {
            identity: process_identity(process.raw())?,
            original: original.clone(),
            expected: expected.clone(),
        };

        apply_process_value(process.raw(), &expected)?;
        assert_eq!(query_process_value(process.raw(), &expected)?, expected);

        recover_journal(&[entry])?;

        assert_eq!(query_process_value(process.raw(), &original)?, original);
        Ok(())
    }

    #[test]
    #[ignore = "modifies and restores the explicit WINDERUST_GPU_TEST_PID target; run in integration QA"]
    fn gpu_priority_recovery_restores_an_explicit_gpu_process() -> Result<(), String> {
        let process_id = std::env::var("WINDERUST_GPU_TEST_PID")
            .map_err(|_| {
                "Set WINDERUST_GPU_TEST_PID to a disposable GPU-using process.".to_owned()
            })?
            .parse::<u32>()
            .map_err(|error| format!("WINDERUST_GPU_TEST_PID is invalid: {error}"))?;
        let executable_path = std::env::var_os("WINDERUST_GPU_TEST_PATH").ok_or_else(|| {
            "Set WINDERUST_GPU_TEST_PATH to that process's absolute executable path.".to_owned()
        })?;
        let target = crate::foreground::capture_process_action_target(
            process_id,
            Path::new(&executable_path),
            true,
        )
        .map_err(|error| error.to_string())?;
        // SAFETY: target is an exact, validated live process instance captured for this test.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
                0,
                target.id,
            )
        };
        if handle.is_null() {
            return Err(format!(
                "OpenProcess(GPU test target) failed with error {}.",
                last_error()
            ));
        }
        let process = WinHandle::new(handle);
        let probe = ProcessValue::GpuPriority(0);
        let original = query_process_value(process.raw(), &probe)?;
        let ProcessValue::GpuPriority(original_priority) = original else {
            unreachable!("GPU priority query must produce a GPU priority value");
        };
        let original = ProcessValue::GpuPriority(original_priority);
        let expected = ProcessValue::GpuPriority(if original_priority == 0 { 1 } else { 0 });
        let entry = RecoveryEntry::Process {
            identity: process_identity(process.raw())?,
            original: original.clone(),
            expected: expected.clone(),
        };

        apply_process_value(process.raw(), &expected)?;
        let recovery_result = recover_journal(&[entry]);
        let observed = query_process_value(process.raw(), &original);
        let cleanup_result = match &observed {
            Ok(value) if value == &original => Ok(()),
            _ => apply_process_value(process.raw(), &original),
        };
        recovery_result?;
        let observed = observed?;
        cleanup_result?;
        if observed != original {
            return Err(format!(
                "GPU priority recovery returned {observed:?}, expected {original:?}."
            ));
        }
        Ok(())
    }

    #[test]
    #[ignore = "modifies and restores a disposable Windows process; run in explicit integration QA"]
    fn memory_priority_recovery_restores_an_explicit_process() -> Result<(), String> {
        let child = DisposableChild::spawn()?;
        let process = child.process_handle()?;
        let probe = ProcessValue::MemoryPriority(0);
        let original = query_process_value(process.raw(), &probe)?;
        let ProcessValue::MemoryPriority(original_priority) = original else {
            unreachable!("Memory Priority query must produce a Memory Priority value");
        };
        let original = ProcessValue::MemoryPriority(original_priority);
        let expected = ProcessValue::MemoryPriority(if original_priority == 1 { 2 } else { 1 });
        let entry = RecoveryEntry::Process {
            identity: process_identity(process.raw())?,
            original: original.clone(),
            expected: expected.clone(),
        };

        apply_process_value(process.raw(), &expected)?;
        let recovery_result = recover_journal(&[entry]);
        let observed = query_process_value(process.raw(), &original);
        let cleanup_result = match &observed {
            Ok(value) if value == &original => Ok(()),
            _ => apply_process_value(process.raw(), &original),
        };
        recovery_result?;
        let observed = observed?;
        cleanup_result?;
        if observed != original {
            return Err(format!(
                "Memory Priority recovery returned {observed:?}, expected {original:?}."
            ));
        }
        Ok(())
    }

    #[test]
    fn thread_priority_recovery_restores_a_disposable_process_thread_after_the_expected_mutation(
    ) -> Result<(), String> {
        let child = DisposableChild::spawn()?;
        let process = child.process_handle()?;
        let thread = test_thread_handle(child.child.id())?;
        // SAFETY: thread is a live, queryable thread owned by the disposable test process.
        let original = unsafe { GetThreadPriority(thread.raw()) };
        if original == THREAD_PRIORITY_ERROR_RETURN {
            return Err(format!(
                "GetThreadPriority failed with error {}.",
                last_error()
            ));
        }
        let expected = if original != THREAD_PRIORITY_BELOW_NORMAL {
            THREAD_PRIORITY_BELOW_NORMAL
        } else {
            THREAD_PRIORITY_LOWEST
        };
        let entry = RecoveryEntry::ThreadPriority {
            process: process_identity(process.raw())?,
            thread_id: thread_id(thread.raw())?,
            thread_creation_time: thread_creation_time(thread.raw())?,
            original,
            expected,
        };

        // SAFETY: thread is a live, queryable thread owned by the disposable test process.
        if unsafe { SetThreadPriority(thread.raw(), expected) } == 0 {
            return Err(format!(
                "SetThreadPriority failed with error {}.",
                last_error()
            ));
        }
        // SAFETY: thread remains live and is still owned by the disposable process.
        assert_eq!(unsafe { GetThreadPriority(thread.raw()) }, expected);
        recover_journal(&[entry])?;
        // SAFETY: thread remains live and is still owned by the disposable process.
        assert_eq!(unsafe { GetThreadPriority(thread.raw()) }, original);
        Ok(())
    }

    #[test]
    fn thread_suspension_recovery_resumes_exact_owned_count_once() -> Result<(), String> {
        let child = DisposableChild::spawn()?;
        let process = child.process_handle()?;
        let captured = captured_test_thread(&process)?;
        let thread = thread_suspension::open_exact_thread(child.child.id(), captured)
            .map_err(|error| format!("{error:?}"))?;
        let original = captured.suspend_count;
        let expected = original
            .checked_add(1)
            .ok_or_else(|| "Disposable thread suspend count overflowed.".to_owned())?;
        assert_eq!(
            thread_suspension::suspend_once(&thread).map_err(|error| format!("{error:?}"))?,
            u32::from(original)
        );
        let entry = RecoveryEntry::ThreadSuspension {
            process: process_identity(process.raw())?,
            thread_id: captured.id,
            thread_creation_time: captured.creation_time,
            original_suspend_count: original,
            expected_suspend_count: expected,
        };

        recover_journal(&[entry])?;

        assert_eq!(
            thread_suspension::exact_suspend_count(
                process.raw(),
                captured.id,
                captured.creation_time,
            )
            .map_err(|error| format!("{error:?}"))?,
            original
        );
        Ok(())
    }

    #[test]
    fn thread_suspension_recovery_leaves_identity_mismatch_unchanged() -> Result<(), String> {
        let child = DisposableChild::spawn()?;
        let process = child.process_handle()?;
        let captured = captured_test_thread(&process)?;
        let thread = thread_suspension::open_exact_thread(child.child.id(), captured)
            .map_err(|error| format!("{error:?}"))?;
        let original = captured.suspend_count;
        let expected = original
            .checked_add(1)
            .ok_or_else(|| "Disposable thread suspend count overflowed.".to_owned())?;
        thread_suspension::suspend_once(&thread).map_err(|error| format!("{error:?}"))?;

        recover_journal(&[RecoveryEntry::ThreadSuspension {
            process: process_identity(process.raw())?,
            thread_id: captured.id,
            thread_creation_time: captured.creation_time.wrapping_add(1),
            original_suspend_count: original,
            expected_suspend_count: expected,
        }])?;
        assert_eq!(
            thread_suspension::exact_suspend_count(
                process.raw(),
                captured.id,
                captured.creation_time,
            )
            .map_err(|error| format!("{error:?}"))?,
            expected
        );
        Ok(())
    }

    #[test]
    fn thread_suspension_recovery_leaves_conflicting_count_unchanged() -> Result<(), String> {
        let child = DisposableChild::spawn()?;
        let process = child.process_handle()?;
        let captured = captured_test_thread(&process)?;
        let thread = thread_suspension::open_exact_thread(child.child.id(), captured)
            .map_err(|error| format!("{error:?}"))?;
        let original = captured.suspend_count;
        let expected = original
            .checked_add(1)
            .ok_or_else(|| "Disposable thread suspend count overflowed.".to_owned())?;
        let conflicting = original
            .checked_add(2)
            .ok_or_else(|| "Disposable thread suspend count cannot be raised twice.".to_owned())?;
        thread_suspension::suspend_once(&thread).map_err(|error| format!("{error:?}"))?;
        thread_suspension::suspend_once(&thread).map_err(|error| format!("{error:?}"))?;
        recover_journal(&[RecoveryEntry::ThreadSuspension {
            process: process_identity(process.raw())?,
            thread_id: captured.id,
            thread_creation_time: captured.creation_time,
            original_suspend_count: original,
            expected_suspend_count: expected,
        }])?;
        assert_eq!(
            thread_suspension::exact_suspend_count(
                process.raw(),
                captured.id,
                captured.creation_time,
            )
            .map_err(|error| format!("{error:?}"))?,
            conflicting
        );
        Ok(())
    }

    #[test]
    fn thread_suspension_recovery_ignores_an_exited_process() -> Result<(), String> {
        let entry = {
            let child = DisposableChild::spawn()?;
            let process = child.process_handle()?;
            let captured = captured_test_thread(&process)?;
            let original = captured.suspend_count;
            RecoveryEntry::ThreadSuspension {
                process: process_identity(process.raw())?,
                thread_id: captured.id,
                thread_creation_time: captured.creation_time,
                original_suspend_count: original,
                expected_suspend_count: original
                    .checked_add(1)
                    .ok_or_else(|| "Disposable thread suspend count overflowed.".to_owned())?,
            }
        };

        recover_journal(&[entry])
    }

    #[test]
    #[ignore = "uses the undocumented JobObjectFreezeInformation contract; run in explicit Windows integration QA"]
    fn suspended_job_recovery_thaws_descendants_after_the_recorded_root_exits() -> Result<(), String>
    {
        let system_root = std::env::var_os("SystemRoot")
            .ok_or_else(|| "SystemRoot is unavailable.".to_owned())?;
        let command = Path::new(&system_root).join("System32").join("cmd.exe");
        let wide_command = command
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut command_line = "cmd.exe /D /C start \"\" /B ping.exe 127.0.0.1 -n 15 -w 1000"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let startup = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut process_information = PROCESS_INFORMATION::default();
        // SAFETY: all pointers reference live, correctly sized buffers; the mutable command line
        // is terminated UTF-16 and the returned handles are owned by this test.
        let created = unsafe {
            CreateProcessW(
                wide_command.as_ptr(),
                command_line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_SUSPENDED | CREATE_NO_WINDOW,
                std::ptr::null(),
                std::ptr::null(),
                &startup,
                &mut process_information,
            )
        };
        if created == 0 {
            return Err(format!(
                "CreateProcessW(test root) failed with error {}.",
                last_error()
            ));
        }
        let process = WinHandle::new(process_information.hProcess);
        let thread = WinHandle::new(process_information.hThread);
        let identity = match process_identity(process.raw()) {
            Ok(identity) => identity,
            Err(error) => {
                resume_disposable_root(&process, &thread);
                return Err(error);
            }
        };
        let name = suspension_job_name(identity.id, identity.creation_time);
        let wide_name = name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // SAFETY: wide_name is terminated UTF-16 and the returned handle is owned by this test.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), wide_name.as_ptr()) };
        if job.is_null() {
            let error = last_error();
            resume_disposable_root(&process, &thread);
            return Err(format!("CreateJobObjectW failed with error {error}."));
        }
        let job = WinHandle::new(job);
        // SAFETY: job and process are live handles owned by this test.
        if unsafe { AssignProcessToJobObject(job.raw(), process.raw()) } == 0 {
            let error = last_error();
            resume_disposable_root(&process, &thread);
            return Err(format!(
                "AssignProcessToJobObject failed with error {error}."
            ));
        }
        let cleanup = FrozenJobCleanup { job };
        // SAFETY: thread is the suspended primary thread created above and has not been resumed.
        if unsafe { ResumeThread(thread.raw()) } == u32::MAX {
            return Err(format!(
                "ResumeThread(test root) failed with error {}.",
                last_error()
            ));
        }
        // SAFETY: process is a live disposable handle; this bounded wait retains no pointer.
        if unsafe { WaitForSingleObject(process.raw(), 5_000) } != WAIT_OBJECT_0 {
            return Err("The disposable Job Object root did not exit in time.".to_owned());
        }
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: cleanup owns the live Job Object handle and accounting is writable for the
        // exact information-class buffer size.
        if unsafe {
            QueryInformationJobObject(
                cleanup.job.raw(),
                JobObjectBasicAccountingInformation,
                (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(format!(
                "QueryInformationJobObject failed with error {}.",
                last_error()
            ));
        }
        if accounting.ActiveProcesses == 0 {
            return Err(
                "The disposable root did not leave an inherited child in the job.".to_owned(),
            );
        }
        let mut freeze = JobObjectFreezeInformation::new(true);
        // SAFETY: job is live and freeze is writable for exactly the supplied structure size.
        if unsafe {
            SetInformationJobObject(
                cleanup.job.raw(),
                JOB_OBJECT_FREEZE_INFORMATION_CLASS,
                (&mut freeze as *mut JobObjectFreezeInformation).cast(),
                std::mem::size_of::<JobObjectFreezeInformation>() as u32,
            )
        } == 0
        {
            return Err(format!(
                "SetInformationJobObject(freeze) failed with error {}.",
                last_error()
            ));
        }

        recover_journal(&[RecoveryEntry::SuspendedJob {
            name,
            process: identity,
        }])?;
        Ok(())
    }

    struct PowerPlanCleanup {
        original_guid: String,
        disposable_guid: Option<String>,
    }

    impl PowerPlanCleanup {
        fn restore_and_delete(&mut self) -> Result<(), String> {
            set_active(&self.original_guid)?;
            if let Some(guid) = self.disposable_guid.as_deref() {
                crate::power::powercfg::delete_plan(guid)?;
                self.disposable_guid = None;
            }
            Ok(())
        }
    }

    impl Drop for PowerPlanCleanup {
        fn drop(&mut self) {
            let _ = set_active(&self.original_guid);
            if let Some(guid) = self.disposable_guid.take() {
                let _ = crate::power::powercfg::delete_plan(&guid);
            }
        }
    }

    #[test]
    #[ignore = "changes the machine-wide active Windows power plan; run manually on a disposable Windows session"]
    fn power_plan_recovery_restores_the_original_plan_and_deletes_the_disposable_plan(
    ) -> Result<(), String> {
        let original_guid = active_plan()?.guid;
        let disposable_guid = crate::power::powercfg::create_adaptive_plan(&original_guid)?;
        let mut cleanup = PowerPlanCleanup {
            original_guid: original_guid.clone(),
            disposable_guid: Some(disposable_guid.clone()),
        };

        set_active(&disposable_guid)?;
        recover_journal(&[RecoveryEntry::PowerPlan {
            original_guid: original_guid.clone(),
            expected_guid: disposable_guid,
        }])?;
        assert!(active_plan()?.guid.eq_ignore_ascii_case(&original_guid));
        cleanup.restore_and_delete()?;
        Ok(())
    }
}
