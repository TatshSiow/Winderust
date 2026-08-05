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
        Foundation::{ERROR_INVALID_PARAMETER, FILETIME, HANDLE},
        System::{
            JobObjects::{OpenJobObjectW, SetInformationJobObject},
            SystemServices::{JOB_OBJECT_QUERY, JOB_OBJECT_SET_ATTRIBUTES},
            Threading::{
                GetPriorityClass, GetProcessAffinityMask, GetProcessDefaultCpuSets, GetProcessId,
                GetProcessInformation, GetProcessPriorityBoost, GetProcessTimes, GetThreadId,
                GetThreadPriority, GetThreadTimes, OpenProcess, OpenThread, ProcessMemoryPriority,
                ProcessPowerThrottling, QueryFullProcessImageNameW, SetPriorityClass,
                SetProcessAffinityMask, SetProcessDefaultCpuSets, SetProcessInformation,
                SetProcessPriorityBoost, SetThreadPriority, MEMORY_PRIORITY_INFORMATION,
                PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SET_INFORMATION, THREAD_QUERY_INFORMATION, THREAD_SET_INFORMATION,
            },
        },
    },
};

use crate::{
    foreground::same_executable_path,
    power::powercfg::{active_plan, restore_stale_adaptive_plans, set_active},
    win_util::{last_error, WinHandle},
};

const WATCHDOG_ARGUMENT: &str = "--winderust-recovery-watchdog";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const PROCESS_IO_PRIORITY: u32 = 33;
const JOB_OBJECT_FREEZE_INFORMATION_CLASS: i32 = 18;
const JOB_OBJECT_FREEZE_OPERATION: u32 = 1;
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum RecoveryCommand {
    Begin { id: u64, entry: RecoveryEntry },
    Commit { id: u64 },
    Cancel { id: u64 },
    ForgetJob { name: String },
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
            } => format!(
                "process:{}:{}:{}",
                identity.id,
                identity.creation_time,
                expected.kind()
            ),
            Self::ThreadPriority {
                process,
                thread_id,
                thread_creation_time,
                ..
            } => format!(
                "thread:{}:{}:{thread_id}:{thread_creation_time}",
                process.id, process.creation_time
            ),
            Self::PowerPlan { .. } => "power_plan".to_owned(),
            Self::SuspendedJob { name, .. } => format!("job:{name}"),
        }
    }
}

#[repr(C)]
struct JobObjectFreezeInformation {
    flags: u32,
    freeze: u8,
    swap: u8,
    spare: u16,
    wake_filter_high: u32,
    wake_filter_low: u32,
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
    match command {
        RecoveryCommand::Begin { id, entry } => {
            if let RecoveryEntry::SuspendedJob { name, .. } = &entry {
                if !jobs.contains_key(name) {
                    jobs.insert(name.clone(), open_job(name)?);
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
                if let RecoveryEntry::SuspendedJob { name, .. } = entry {
                    let key = format!("job:{name}");
                    if !entries.iter().any(|entry| entry.key() == key)
                        && !pending.iter().any(|(_, entry)| entry.key() == key)
                    {
                        jobs.remove(&name);
                    }
                }
            }
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

pub(crate) fn initialize() {
    if let Err(error) = initialize_inner() {
        set_startup_error(format!("Crash recovery protection is unavailable: {error}"));
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

pub(crate) fn finish_clean_shutdown() {
    let runtime = RUNTIME.lock().ok().and_then(|mut runtime| runtime.take());
    if let Some(mut runtime) = runtime {
        drop(runtime.stdin);
        let _ = runtime.child.wait();
    }
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
    let executable_hash = std::env::current_exe()
        .ok()
        .map(|path| fnv1a64(path.as_os_str().encode_wide()))
        .unwrap_or(0x5f3f_2a4e_13a5_59f0);
    format!("Local\\Winderust.Suspend.{executable_hash:016x}.{process_id}.{creation_time}")
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
        RecoveryEntry::PowerPlan { .. } => recover_power_plan_key(key, entries),
        RecoveryEntry::SuspendedJob { name, process } => thaw_job(name, process),
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
            let mut state = PROCESS_POWER_THROTTLING_STATE::default();
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
    unsafe { GetProcessDefaultCpuSets(handle, null_mut(), 0, &mut required) };
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

fn thaw_job(name: &str, process: &ProcessIdentity) -> Result<(), String> {
    let handle = open_job(name)?;
    let Some(process_handle) =
        open_matching_process_with_access(process, PROCESS_QUERY_LIMITED_INFORMATION)?
    else {
        return Ok(());
    };
    let mut assigned = 0;
    // SAFETY: both handles are live and assigned is writable for this call.
    let checked = unsafe {
        windows_sys::Win32::System::JobObjects::IsProcessInJob(
            process_handle.raw(),
            handle.raw(),
            &mut assigned,
        )
    };
    if checked == 0 {
        return Err(format!(
            "IsProcessInJob failed with error {}.",
            last_error()
        ));
    }
    if assigned == 0 {
        return Ok(());
    }
    let mut info = JobObjectFreezeInformation {
        flags: JOB_OBJECT_FREEZE_OPERATION,
        freeze: 0,
        swap: 0,
        spare: 0,
        wake_filter_high: 0,
        wake_filter_low: 0,
    };
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

fn open_job(name: &str) -> Result<WinHandle, String> {
    let wide = name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: wide is terminated UTF-16 and the returned handle is owned here.
    let handle = unsafe {
        OpenJobObjectW(
            JOB_OBJECT_QUERY | JOB_OBJECT_SET_ATTRIBUTES,
            0,
            wide.as_ptr(),
        )
    };
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
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
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

    fn identity() -> ProcessIdentity {
        ProcessIdentity {
            id: 7,
            creation_time: 11,
            executable_path: "C:\\app.exe".to_owned(),
        }
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
}
