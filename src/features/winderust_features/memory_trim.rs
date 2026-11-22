use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    ops::Bound::{Excluded, Unbounded},
    path::PathBuf,
    time::{Duration, Instant},
};

use windows_sys::Win32::System::{
    SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
    Threading::GetCurrentProcessId,
};

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::MemoryTrimSettings,
    control::{
        memory_trim::{MemoryTrimController, MemoryTrimOutcome, MemoryTrimPlatform},
        process::{ProcessControlError, ProcessControlTarget},
    },
    cpu::{process_cpu_usage_percent, ProcessCpuSample},
    foreground::{
        contains_process_name, executable_grouping_key, process_executable_path,
        process_list::catalog_process_executable_path, same_executable_path,
        should_ignore_foreground_process, EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
    runtime::observations::CycleObservations,
    win_util::last_error,
};

const MB: u64 = 1_000_000;
const CPU_IDLE_THRESHOLD_PERCENT: f32 = 1.0;
const AUTOMATIC_TRIM_ATTEMPT_LIMIT: usize = 4;

const BUILT_IN_EXCLUSIONS: &[&str] = EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryTrimSnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub candidate_processes: usize,
    pub(crate) has_deferred_candidates: bool,
    pub trimmed_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub memory_load_percent: Option<u8>,
    pub auto_excluded_processes: Vec<String>,
    pub status: MemoryTrimStatus,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum MemoryTrimStatus {
    AutomationDisabled,
    #[default]
    Disabled,
    ForegroundUnknown,
    WaitingForMemoryLoad {
        threshold_percent: u8,
    },
    Active,
    ManualCompleted,
    Error(String),
}

#[derive(Default)]
pub struct MemoryTrimManager {
    tracked: BTreeMap<u32, TrackedProcess>,
    failure_suppression: ExecutionFailureTracker<OsString>,
    last_automatic_trim_pid: u32,
}

#[derive(Clone)]
struct TrackedProcess {
    executable_path: PathBuf,
    creation_time: u64,
    previous_cpu_time: Option<ProcessCpuSample>,
    idle_since: Option<Instant>,
    trimmed_while_idle: bool,
}

impl TrackedProcess {
    fn break_idle_interval(&mut self) {
        self.previous_cpu_time = None;
        self.idle_since = None;
    }

    fn observe_cpu(
        &mut self,
        current: ProcessCpuSample,
        idle_duration: Duration,
        now: Instant,
    ) -> bool {
        let Some(usage) = self.sample_cpu_usage(current) else {
            return false;
        };
        ready_to_trim(self, usage, idle_duration, now)
    }

    fn observe_below_working_set_threshold(&mut self, current: Option<ProcessCpuSample>) {
        self.idle_since = None;
        let Some(current) = current else {
            self.break_idle_interval();
            return;
        };
        if self
            .sample_cpu_usage(current)
            .is_some_and(|usage| usage > CPU_IDLE_THRESHOLD_PERCENT)
        {
            self.trimmed_while_idle = false;
        }
    }

    fn sample_cpu_usage(&mut self, current: ProcessCpuSample) -> Option<f32> {
        let previous = self.previous_cpu_time.replace(current);
        let Some(previous) = previous else {
            self.idle_since = None;
            return None;
        };
        let Some(usage) = process_cpu_usage_percent(previous, current) else {
            self.break_idle_interval();
            return None;
        };
        Some(usage)
    }
}

impl MemoryTrimManager {
    #[expect(
        clippy::too_many_arguments,
        reason = "the policy pass keeps its typed controller and pass-local observations explicit"
    )]
    pub fn update(
        &mut self,
        controller: &mut MemoryTrimController,
        settings: &MemoryTrimSettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> MemoryTrimSnapshot {
        self.update_with_mode(
            controller,
            settings,
            automation_enabled,
            allow_cross_session_process_control,
            foreground_process_id,
            MemoryTrimMode::Automatic,
            observations,
            action_log,
            system_memory_load_percent,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the policy pass keeps its typed controller and pass-local observations explicit"
    )]
    pub fn trim_now(
        &mut self,
        controller: &mut MemoryTrimController,
        settings: &MemoryTrimSettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> MemoryTrimSnapshot {
        self.update_with_mode(
            controller,
            settings,
            automation_enabled,
            allow_cross_session_process_control,
            foreground_process_id,
            MemoryTrimMode::Manual,
            observations,
            action_log,
            system_memory_load_percent,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the pass-local observation dependency is clearer here than an unrelated argument bundle"
    )]
    fn update_with_mode<P: MemoryTrimPlatform>(
        &mut self,
        controller: &mut MemoryTrimController<P>,
        settings: &MemoryTrimSettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        mode: MemoryTrimMode,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
        read_memory_load: impl FnOnce() -> Result<u8, String>,
    ) -> MemoryTrimSnapshot {
        if !automation_enabled {
            self.clear_tracking();
            self.clear_failure_suppression();
            return MemoryTrimSnapshot {
                enabled: false,
                status: MemoryTrimStatus::AutomationDisabled,
                ..Default::default()
            };
        }

        if !settings.enabled {
            self.clear_tracking();
            self.clear_failure_suppression();
            return MemoryTrimSnapshot {
                enabled: false,
                ..Default::default()
            };
        }

        let Some(foreground_process_id) = foreground_process_id else {
            self.break_idle_intervals();
            return MemoryTrimSnapshot {
                enabled: true,
                status: MemoryTrimStatus::ForegroundUnknown,
                ..Default::default()
            };
        };

        let memory_load_percent = match read_memory_load() {
            Ok(percent) => percent,
            Err(err) => {
                self.break_idle_intervals();
                return MemoryTrimSnapshot {
                    enabled: true,
                    status: MemoryTrimStatus::Error(err.clone()),
                    last_error: Some(err),
                    ..Default::default()
                };
            }
        };

        let threshold = settings.system_memory_load_threshold_percent.min(100);
        let below_threshold = memory_load_percent < threshold;
        if below_threshold {
            self.break_idle_intervals();
        }
        if mode == MemoryTrimMode::Automatic && below_threshold {
            return MemoryTrimSnapshot {
                enabled: true,
                memory_load_percent: Some(memory_load_percent),
                status: MemoryTrimStatus::WaitingForMemoryLoad {
                    threshold_percent: threshold,
                },
                ..Default::default()
            };
        }

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };

        let processes = match observations.processes() {
            Ok(processes) => processes,
            Err(err) => {
                self.break_idle_intervals();
                return MemoryTrimSnapshot {
                    enabled: true,
                    memory_load_percent: Some(memory_load_percent),
                    status: MemoryTrimStatus::Error(err.clone()),
                    last_error: Some(err),
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let Some(foreground_executable_path) =
            catalog_process_executable_path(&processes, foreground_process_id)
        else {
            self.break_idle_intervals();
            return MemoryTrimSnapshot {
                enabled: true,
                memory_load_percent: Some(memory_load_percent),
                status: MemoryTrimStatus::ForegroundUnknown,
                ..Default::default()
            };
        };

        let mut target_processes = BTreeMap::new();
        let mut observed_tracked_processes = BTreeSet::new();
        for process in processes.iter() {
            if self.tracked.get(&process.id).is_some_and(|tracked| {
                process
                    .creation_time
                    .is_none_or(|creation_time| creation_time == tracked.creation_time)
            }) {
                observed_tracked_processes.insert(process.id);
            }
            if process.id == 0
                || process.is_critical != Some(false)
                || process.id == current_process_id
                || is_builtin_excluded(&process.name)
            {
                continue;
            }

            let Some(creation_time) = process.creation_time else {
                continue;
            };
            let Some(executable_path) = process_executable_path(process) else {
                continue;
            };
            if should_ignore_foreground_process(
                true,
                process.id,
                &executable_path,
                Some(foreground_process_id),
                Some(&foreground_executable_path),
            ) || executable_path
                .to_str()
                .is_some_and(|path| settings.exclusion_enabled_for(path))
            {
                continue;
            }

            target_processes.insert(
                process.id,
                ProcessControlTarget::automatic(
                    process.id,
                    process.name.clone(),
                    executable_path,
                    creation_time,
                ),
            );
        }

        self.tracked.retain(|process_id, tracked| {
            if !observed_tracked_processes.contains(process_id) {
                return false;
            }
            if !target_processes.contains_key(process_id) {
                // A temporary loss of eligibility does not rearm a trimmed idle episode.
                tracked.break_idle_interval();
            }
            true
        });
        let active_target_paths = target_processes
            .values()
            .map(|target| executable_grouping_key(&target.executable_path))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_paths);

        let mut candidate_processes = 0;
        let mut has_deferred_candidates = false;
        let mut trimmed_processes = 0;
        let mut skipped_processes = 0;
        let mut failures = MemoryTrimFailures::default();
        let mut auto_excluded_processes = BTreeSet::new();
        let mut successful_trim_paths = BTreeSet::new();
        let mut unresolved_trim_paths = BTreeSet::new();
        let now = Instant::now();

        let mut remaining_attempts =
            (mode == MemoryTrimMode::Automatic).then_some(AUTOMATIC_TRIM_ATTEMPT_LIMIT);
        let cursor = match mode {
            MemoryTrimMode::Automatic => self.last_automatic_trim_pid,
            MemoryTrimMode::Manual => 0,
        };
        for (_, target) in target_processes
            .range((Excluded(cursor), Unbounded))
            .chain(target_processes.range(..=cursor))
        {
            let process_id = target.id;
            let process_name = target.name.as_str();
            let path_key = executable_grouping_key(&target.executable_path);
            if self.is_process_suppressed(
                target,
                &path_key,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }

            match self.update_process(
                controller,
                target,
                allow_cross_session_process_control,
                settings,
                remaining_attempts.as_mut(),
                now,
            ) {
                Ok(ProcessUpdate::Waiting) => {}
                Ok(ProcessUpdate::Candidate) => candidate_processes += 1,
                Ok(ProcessUpdate::Deferred) => {
                    candidate_processes += 1;
                    has_deferred_candidates = true;
                    unresolved_trim_paths.insert(path_key);
                }
                Ok(ProcessUpdate::Trimmed {
                    working_set_reduction_bytes,
                }) => {
                    candidate_processes += 1;
                    trimmed_processes += 1;
                    successful_trim_paths.insert(path_key);
                    action_log.record(
                        ActionLogFeature::MemoryTrim,
                        Some(process_id),
                        process_name,
                        ActionLogResult::Applied,
                        trim_reason(mode, working_set_reduction_bytes),
                    );
                }
                Err(ProcessControlError::ProcessExited) => {
                    skipped_processes += 1;
                    self.tracked.remove(&process_id);
                }
                Err(ProcessControlError::AccessDenied(_)) => {
                    skipped_processes += 1;
                    self.failure_suppression.suppress_key_failure(&path_key);
                    unresolved_trim_paths.insert(path_key);
                    action_log.record(
                        ActionLogFeature::MemoryTrim,
                        Some(process_id),
                        process_name,
                        ActionLogResult::Skipped,
                        "Access denied when opening process.",
                    );
                }
                Err(err) => {
                    self.failure_suppression.record_key_failure(&path_key);
                    unresolved_trim_paths.insert(path_key);
                    failures.record(process_id, process_name, err, action_log);
                }
            }
        }
        // A successful trim cannot clear an unresolved attempt from a same-path sibling.
        for key in successful_trim_paths.difference(&unresolved_trim_paths) {
            self.failure_suppression.clear_key_failure(key);
        }

        MemoryTrimSnapshot {
            enabled: true,
            scanned_processes,
            candidate_processes,
            has_deferred_candidates,
            trimmed_processes,
            skipped_processes,
            failed_processes: failures.count,
            memory_load_percent: Some(memory_load_percent),
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            status: match mode {
                MemoryTrimMode::Automatic => MemoryTrimStatus::Active,
                MemoryTrimMode::Manual => MemoryTrimStatus::ManualCompleted,
            },
            last_error: failures.last_error,
        }
    }

    fn update_process<P: MemoryTrimPlatform>(
        &mut self,
        controller: &mut MemoryTrimController<P>,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
        settings: &MemoryTrimSettings,
        remaining_attempts: Option<&mut usize>,
        now: Instant,
    ) -> Result<ProcessUpdate, ProcessControlError> {
        let process_id = target.id;
        if self.tracked.get(&process_id).is_some_and(|state| {
            state.creation_time != target.creation_time
                || !same_executable_path(&state.executable_path, &target.executable_path)
        }) {
            self.tracked.remove(&process_id);
        }
        let sample = match controller.sample(
            target,
            allow_cross_session_process_control,
            remaining_attempts.is_some(),
        ) {
            Ok(sample) => sample,
            Err(error) => {
                if let Some(state) = self.tracked.get_mut(&process_id) {
                    state.break_idle_interval();
                }
                return Err(error);
            }
        };
        let threshold_bytes = settings.process_working_set_threshold_mb.saturating_mul(MB);
        if sample.working_set_bytes < threshold_bytes {
            if let Some(state) = self.tracked.get_mut(&process_id) {
                state.observe_below_working_set_threshold(sample.cpu);
            }
            return Ok(ProcessUpdate::Waiting);
        }

        let Some(remaining_attempts) = remaining_attempts else {
            let MemoryTrimOutcome::Trimmed {
                working_set_reduction_bytes,
            } = controller.trim(target, allow_cross_session_process_control, threshold_bytes)?
            else {
                if let Some(state) = self.tracked.get_mut(&process_id) {
                    state.break_idle_interval();
                }
                return Ok(ProcessUpdate::Waiting);
            };
            return Ok(ProcessUpdate::Trimmed {
                working_set_reduction_bytes,
            });
        };

        let Some(cpu_sample) = sample.cpu else {
            if let Some(state) = self.tracked.get_mut(&process_id) {
                state.break_idle_interval();
            }
            return Err(ProcessControlError::Failed(
                "Memory Trim CPU sample is unavailable.".to_owned(),
            ));
        };
        let executable_path = &target.executable_path;
        let state = self
            .tracked
            .entry(process_id)
            .and_modify(|state| {
                if state.executable_path.as_os_str() != executable_path.as_os_str() {
                    state.executable_path.clone_from(executable_path);
                }
            })
            .or_insert_with(|| TrackedProcess {
                executable_path: executable_path.clone(),
                creation_time: target.creation_time,
                previous_cpu_time: None,
                idle_since: None,
                trimmed_while_idle: false,
            });

        if !state.observe_cpu(
            cpu_sample,
            Duration::from_secs(settings.process_idle_seconds),
            now,
        ) {
            return Ok(ProcessUpdate::Candidate);
        }

        if *remaining_attempts == 0 {
            return Ok(ProcessUpdate::Deferred);
        }
        *remaining_attempts -= 1;
        self.last_automatic_trim_pid = process_id;
        let MemoryTrimOutcome::Trimmed {
            working_set_reduction_bytes,
        } = controller.trim(target, allow_cross_session_process_control, threshold_bytes)?
        else {
            state.break_idle_interval();
            return Ok(ProcessUpdate::Waiting);
        };
        state.trimmed_while_idle = true;
        Ok(ProcessUpdate::Trimmed {
            working_set_reduction_bytes,
        })
    }

    fn clear_tracking(&mut self) {
        self.tracked.clear();
        self.last_automatic_trim_pid = 0;
    }

    fn break_idle_intervals(&mut self) {
        for state in self.tracked.values_mut() {
            state.break_idle_interval();
        }
    }

    fn clear_failure_suppression(&mut self) {
        self.failure_suppression.clear();
    }

    fn is_process_suppressed(
        &mut self,
        target: &ProcessControlTarget,
        path_key: &OsStr,
        action_log: &mut ActionLog,
        auto_excluded_processes: &mut BTreeSet<String>,
    ) -> bool {
        let suppression = self.failure_suppression.key_suppression(path_key);
        if !suppression.suppressed {
            return false;
        }

        if suppression.newly_suppressed {
            if let Some(path) = target.executable_path.to_str() {
                auto_excluded_processes.insert(path.to_owned());
            }
            action_log.record(
                ActionLogFeature::MemoryTrim,
                Some(target.id),
                &target.name,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Memory Trim after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }

        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemoryTrimMode {
    Automatic,
    Manual,
}

enum ProcessUpdate {
    Waiting,
    Candidate,
    Deferred,
    Trimmed {
        working_set_reduction_bytes: Option<u64>,
    },
}

fn ready_to_trim(
    state: &mut TrackedProcess,
    cpu_usage_percent: f32,
    idle_duration: Duration,
    now: Instant,
) -> bool {
    if cpu_usage_percent > CPU_IDLE_THRESHOLD_PERCENT {
        state.idle_since = None;
        state.trimmed_while_idle = false;
        return false;
    }
    if state.trimmed_while_idle {
        return false;
    }

    let idle_since = *state.idle_since.get_or_insert(now);
    now.duration_since(idle_since) >= idle_duration
}
fn trim_reason(mode: MemoryTrimMode, working_set_reduction_bytes: Option<u64>) -> String {
    let action = match mode {
        MemoryTrimMode::Automatic => "Trimmed working set",
        MemoryTrimMode::Manual => "Manually trimmed working set",
    };
    match working_set_reduction_bytes {
        Some(bytes) => format!("{action}; working-set reduction {}.", size_label(bytes)),
        None => format!("{action}; working-set reduction unavailable."),
    }
}

#[derive(Default)]
struct MemoryTrimFailures {
    count: usize,
    last_error: Option<String>,
}

impl MemoryTrimFailures {
    fn record(
        &mut self,
        process_id: u32,
        process_name: &str,
        error: ProcessControlError,
        action_log: &mut ActionLog,
    ) {
        if matches!(&error, ProcessControlError::ProcessExited) {
            return;
        }
        let message = error.to_string();
        self.count += 1;
        if self.last_error.is_none() {
            self.last_error = Some(format!("Trim {process_name} ({process_id}): {message}"));
        }
        action_log.record(
            ActionLogFeature::MemoryTrim,
            Some(process_id),
            process_name,
            ActionLogResult::Failed,
            message,
        );
    }
}

fn system_memory_load_percent() -> Result<u8, String> {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: status declares its initialized size and remains writable for the call.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        Err(format!(
            "GlobalMemoryStatusEx failed with error {}.",
            last_error()
        ))
    } else {
        Ok(status.dwMemoryLoad.min(100) as u8)
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    contains_process_name(BUILT_IN_EXCLUSIONS, process_name)
}

fn size_label(bytes: u64) -> String {
    if bytes >= MB {
        format!("{} MB", bytes / MB)
    } else if bytes >= 1_000 {
        format!("{} KB", bytes / 1_000)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests;
