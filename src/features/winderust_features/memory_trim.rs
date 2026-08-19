use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
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
        memory_trim::MemoryTrimController,
        process::{ProcessControlError, ProcessControlTarget},
    },
    cpu::{process_cpu_usage_percent, ProcessCpuSample},
    foreground::{
        contains_process_name, process_executable_path, process_failure_key, same_executable_path,
        should_ignore_foreground_process, EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
    runtime::observations::CycleObservations,
    win_util::last_error,
};

const MB: u64 = 1024 * 1024;
const CPU_IDLE_THRESHOLD_PERCENT: f32 = 1.0;

const BUILT_IN_EXCLUSIONS: &[&str] = EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryTrimSnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub candidate_processes: usize,
    pub trimmed_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub memory_load_percent: Option<u8>,
    pub trimmed_apps: Vec<String>,
    pub auto_excluded_processes: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct MemoryTrimManager {
    tracked: BTreeMap<u32, TrackedProcess>,
    failure_suppression: ExecutionFailureTracker,
}

#[derive(Clone)]
struct TrackedProcess {
    executable_path: String,
    creation_time: u64,
    previous_cpu_time: Option<ProcessCpuSample>,
    idle_since: Option<Instant>,
    trimmed_while_idle: bool,
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
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the pass-local observation dependency is clearer here than an unrelated argument bundle"
    )]
    fn update_with_mode(
        &mut self,
        controller: &mut MemoryTrimController,
        settings: &MemoryTrimSettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        mode: MemoryTrimMode,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> MemoryTrimSnapshot {
        if !automation_enabled {
            self.clear_tracking();
            self.clear_failure_suppression();
            return MemoryTrimSnapshot {
                enabled: false,
                message: "Automation disabled.".to_owned(),
                ..Default::default()
            };
        }

        if !settings.enabled {
            self.clear_tracking();
            self.clear_failure_suppression();
            return MemoryTrimSnapshot {
                enabled: false,
                message: "Memory Trim disabled.".to_owned(),
                ..Default::default()
            };
        }

        if foreground_process_id.is_none() {
            self.clear_tracking();
            return MemoryTrimSnapshot {
                enabled: true,
                message: "Paused: foreground app is unknown.".to_owned(),
                ..Default::default()
            };
        }

        let memory_load_percent = match system_memory_load_percent() {
            Ok(percent) => percent,
            Err(err) => {
                self.clear_tracking();
                return MemoryTrimSnapshot {
                    enabled: true,
                    message: err.clone(),
                    last_error: Some(err),
                    ..Default::default()
                };
            }
        };

        let threshold = settings.system_memory_load_threshold_percent.min(100);
        if mode == MemoryTrimMode::Automatic && memory_load_percent < threshold {
            self.clear_tracking();
            return MemoryTrimSnapshot {
                enabled: true,
                memory_load_percent: Some(memory_load_percent),
                message: format!("Memory Trim waiting for system memory load >= {threshold}%."),
                ..Default::default()
            };
        }

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };

        let processes = match observations.processes() {
            Ok(processes) => processes,
            Err(err) => {
                self.clear_tracking();
                return MemoryTrimSnapshot {
                    enabled: true,
                    memory_load_percent: Some(memory_load_percent),
                    message: err,
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let foreground_executable_path = foreground_process_id.and_then(|id| {
            processes
                .iter()
                .find(|process| process.id == id)
                .and_then(process_executable_path)
        });

        let mut target_processes = BTreeMap::new();
        for process in processes.iter() {
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
                foreground_process_id,
                foreground_executable_path.as_deref(),
            ) || settings.exclusion_enabled_for(executable_path.to_string_lossy().as_ref())
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

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        self.tracked
            .retain(|process_id, _| target_ids.contains(process_id));
        let active_target_names = target_processes
            .values()
            .map(|target| process_failure_key(target.executable_path.to_string_lossy().as_ref()))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);

        let mut candidate_processes = 0;
        let mut trimmed_processes = 0;
        let mut skipped_processes = 0;
        let mut failures = MemoryTrimFailures::default();
        let mut trimmed_apps = BTreeSet::new();
        let mut auto_excluded_processes = BTreeSet::new();
        let now = Instant::now();

        for target in target_processes.into_values() {
            let process_id = target.id;
            let process_name = target.name.clone();
            let executable_path = target.executable_path.to_string_lossy().into_owned();
            if self.is_process_suppressed(
                process_id,
                &process_name,
                &executable_path,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }

            match self.update_process(
                controller,
                &target,
                allow_cross_session_process_control,
                settings,
                mode,
                now,
            ) {
                Ok(ProcessUpdate::Waiting) => {
                    self.clear_process_failure(&executable_path);
                }
                Ok(ProcessUpdate::Candidate) => {
                    candidate_processes += 1;
                    self.clear_process_failure(&executable_path);
                }
                Ok(ProcessUpdate::Trimmed { freed_bytes }) => {
                    candidate_processes += 1;
                    trimmed_processes += 1;
                    trimmed_apps.insert(process_name.clone());
                    self.clear_process_failure(&executable_path);
                    action_log.record(
                        ActionLogFeature::MemoryTrim,
                        Some(process_id),
                        process_name,
                        ActionLogResult::Applied,
                        trim_reason(mode, freed_bytes),
                    );
                }
                Err(ProcessControlError::ProcessExited) => {
                    skipped_processes += 1;
                    self.tracked.remove(&process_id);
                }
                Err(ProcessControlError::AccessDenied(_)) => {
                    skipped_processes += 1;
                    self.failure_suppression
                        .suppress_process_failure(&executable_path);
                    action_log.record(
                        ActionLogFeature::MemoryTrim,
                        Some(process_id),
                        process_name,
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                Err(err) => {
                    self.record_process_failure(&executable_path);
                    failures.record(process_id, &process_name, err, action_log);
                }
            }
        }

        MemoryTrimSnapshot {
            enabled: true,
            scanned_processes,
            candidate_processes,
            trimmed_processes,
            skipped_processes,
            failed_processes: failures.count,
            memory_load_percent: Some(memory_load_percent),
            trimmed_apps: trimmed_apps.into_iter().collect(),
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            message: match mode {
                MemoryTrimMode::Automatic => "Memory Trim active.".to_owned(),
                MemoryTrimMode::Manual => "Manual Memory Trim pass completed.".to_owned(),
            },
            last_error: failures.last_error,
        }
    }

    fn update_process(
        &mut self,
        controller: &mut MemoryTrimController,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
        settings: &MemoryTrimSettings,
        mode: MemoryTrimMode,
        now: Instant,
    ) -> Result<ProcessUpdate, ProcessControlError> {
        let process_id = target.id;
        let executable_path = target.executable_path.to_string_lossy().into_owned();
        let sample = controller.sample(
            target,
            allow_cross_session_process_control,
            mode == MemoryTrimMode::Automatic,
        )?;
        let threshold_bytes = settings.process_working_set_threshold_mb.saturating_mul(MB);
        if sample.working_set_bytes < threshold_bytes {
            self.tracked.remove(&process_id);
            return Ok(ProcessUpdate::Waiting);
        }

        if mode == MemoryTrimMode::Manual {
            let outcome = controller.trim(target, allow_cross_session_process_control)?;
            return Ok(ProcessUpdate::Trimmed {
                freed_bytes: outcome.freed_bytes,
            });
        }

        let cpu_sample = sample.cpu.ok_or_else(|| {
            ProcessControlError::Failed("Memory Trim CPU sample is unavailable.".to_owned())
        })?;
        if self.tracked.get(&process_id).is_some_and(|state| {
            state.creation_time != target.creation_time
                || !same_executable_path(Path::new(&state.executable_path), &target.executable_path)
        }) {
            self.tracked.remove(&process_id);
        }
        let state = self
            .tracked
            .entry(process_id)
            .or_insert_with(|| TrackedProcess {
                executable_path: executable_path.clone(),
                creation_time: target.creation_time,
                previous_cpu_time: None,
                idle_since: None,
                trimmed_while_idle: false,
            });
        state.executable_path = executable_path;

        let usage = state
            .previous_cpu_time
            .and_then(|previous| process_cpu_usage_percent(previous, cpu_sample));
        state.previous_cpu_time = Some(cpu_sample);
        let Some(usage) = usage else {
            return Ok(ProcessUpdate::Candidate);
        };

        if !ready_to_trim(
            state,
            usage,
            Duration::from_secs(settings.process_idle_seconds),
            now,
        ) {
            return Ok(ProcessUpdate::Candidate);
        }

        let outcome = controller.trim(target, allow_cross_session_process_control)?;
        state.trimmed_while_idle = true;
        Ok(ProcessUpdate::Trimmed {
            freed_bytes: outcome.freed_bytes,
        })
    }

    fn clear_tracking(&mut self) {
        self.tracked.clear();
    }

    fn clear_failure_suppression(&mut self) {
        self.failure_suppression.clear();
    }

    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        executable_path: &str,
        action_log: &mut ActionLog,
        auto_excluded_processes: &mut BTreeSet<String>,
    ) -> bool {
        let suppression = self
            .failure_suppression
            .process_suppression(executable_path);
        if !suppression.suppressed {
            return false;
        }

        if suppression.newly_suppressed {
            auto_excluded_processes.insert(executable_path.to_owned());
            action_log.record(
                ActionLogFeature::MemoryTrim,
                Some(process_id),
                process_name.to_owned(),
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Memory Trim after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }

        true
    }

    fn record_process_failure(&mut self, process_name: &str) {
        self.failure_suppression
            .record_process_failure(process_name);
    }

    fn clear_process_failure(&mut self, process_name: &str) {
        self.failure_suppression.clear_process_failure(process_name);
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
    Trimmed { freed_bytes: Option<u64> },
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
fn trim_reason(mode: MemoryTrimMode, freed_bytes: Option<u64>) -> String {
    let action = match mode {
        MemoryTrimMode::Automatic => "Trimmed working set",
        MemoryTrimMode::Manual => "Manually trimmed working set",
    };
    match freed_bytes {
        Some(freed_bytes) => format!("{action}; estimated freed {}.", size_label(freed_bytes)),
        None => format!("{action}; freed-memory estimate unavailable."),
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
            process_name.to_owned(),
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
        format!("{} MiB", bytes / MB)
    } else {
        format!("{} KiB", bytes / 1024)
    }
}

impl Default for MemoryTrimSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            scanned_processes: 0,
            candidate_processes: 0,
            trimmed_processes: 0,
            skipped_processes: 0,
            failed_processes: 0,
            memory_load_percent: None,
            trimmed_apps: Vec::new(),
            auto_excluded_processes: Vec::new(),
            message: "Memory Trim disabled.".to_owned(),
            last_error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_process_failures_suppress_memory_trim_retries() {
        let mut manager = MemoryTrimManager::default();
        let mut log = ActionLog::new(8);
        let executable_path = r"C:\Apps\app.exe";

        manager.record_process_failure(executable_path);
        manager.record_process_failure(r"C:/Apps/app.exe");
        assert!(!manager.is_process_suppressed(
            42,
            "app.exe",
            executable_path,
            &mut log,
            &mut BTreeSet::new()
        ));

        manager.record_process_failure(executable_path);
        assert!(manager.is_process_suppressed(
            42,
            "app.exe",
            executable_path,
            &mut log,
            &mut BTreeSet::new()
        ));
        assert!(manager.is_process_suppressed(
            43,
            "app.exe",
            r"C:/Apps/app.exe",
            &mut log,
            &mut BTreeSet::new()
        ));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].feature, ActionLogFeature::MemoryTrim);
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
        assert!(entries[0].reason.contains("Stopped retrying Memory Trim"));
    }

    #[test]
    fn successful_process_clears_memory_trim_failure_suppression() {
        let mut manager = MemoryTrimManager::default();
        let mut log = ActionLog::new(8);
        let executable_path = r"C:\Apps\app.exe";

        manager.record_process_failure(executable_path);
        manager.record_process_failure(executable_path);
        manager.record_process_failure(executable_path);
        assert!(manager.is_process_suppressed(
            42,
            "app.exe",
            executable_path,
            &mut log,
            &mut BTreeSet::new()
        ));

        manager.clear_process_failure(r"C:/Apps/app.exe");
        assert!(!manager.is_process_suppressed(
            42,
            "app.exe",
            executable_path,
            &mut log,
            &mut BTreeSet::new()
        ));
    }

    #[test]
    fn builtin_exclusions_cover_sensitive_windows_processes() {
        assert!(is_builtin_excluded("csrss.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("worker.exe"));
    }

    #[test]
    fn foreground_skip_matches_pid_or_exact_path() {
        let foreground = Path::new(r"C:\Apps\Foreground\app.exe");

        assert!(should_ignore_foreground_process(
            true,
            42,
            Path::new(r"C:\Apps\helper.exe"),
            Some(42),
            Some(foreground),
        ));
        assert!(should_ignore_foreground_process(
            true,
            99,
            Path::new(r"c:\apps\foreground\APP.EXE"),
            Some(42),
            Some(foreground),
        ));
        assert!(!should_ignore_foreground_process(
            true,
            99,
            Path::new(r"D:\Other\app.exe"),
            Some(42),
            Some(foreground),
        ));
    }

    #[test]
    fn trim_eligibility_rearms_only_after_process_activity() {
        let now = Instant::now();
        let mut process = TrackedProcess {
            executable_path: r"C:\Apps\app.exe".to_owned(),
            creation_time: 1,
            previous_cpu_time: None,
            idle_since: None,
            trimmed_while_idle: false,
        };
        let idle_duration = Duration::from_secs(300);

        assert!(!ready_to_trim(&mut process, 0.0, idle_duration, now));
        assert!(ready_to_trim(
            &mut process,
            0.0,
            idle_duration,
            now + idle_duration
        ));

        process.trimmed_while_idle = true;
        assert!(!ready_to_trim(
            &mut process,
            0.0,
            idle_duration,
            now + idle_duration
        ));
        assert!(!ready_to_trim(
            &mut process,
            2.0,
            idle_duration,
            now + idle_duration
        ));
        assert!(!process.trimmed_while_idle);
    }
    #[test]
    fn process_cpu_usage_percent_scales_by_processor_count() {
        let now = Instant::now();
        let previous = ProcessCpuSample {
            cpu_time_100ns: 0,
            sampled_at: now,
        };
        let current = ProcessCpuSample {
            cpu_time_100ns: 10_000_000,
            sampled_at: now + Duration::from_secs(1),
        };

        let usage = process_cpu_usage_percent(previous, current).unwrap();

        assert!(usage > 0.0);
        assert!(usage <= 100.0);
    }
}
