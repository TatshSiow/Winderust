use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    fmt, mem,
    path::Path,
    ptr,
    ptr::null_mut,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR},
    NetworkManagement::IpHelper::{
        GetExtendedTcpTable, GetExtendedUdpTable, GetPerTcp6ConnectionEStats,
        GetPerTcpConnectionEStats, SetPerTcp6ConnectionEStats, SetPerTcpConnectionEStats,
        TCP_ESTATS_DATA_ROD_v0, TCP_ESTATS_DATA_RW_v0, TcpConnectionEstatsData, MIB_TCP6ROW,
        MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_LH, MIB_TCPROW_LH_0, MIB_TCPROW_OWNER_PID,
        MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_CONNECTIONS,
        UDP_TABLE_OWNER_PID,
    },
    Networking::WinSock::{AF_INET, AF_INET6, IN6_ADDR, IN6_ADDR_0},
    System::Threading::GetCurrentProcessId,
};

use crate::audio_activity::active_audio_process_ids;

use crate::config::AppSuspensionSettings;
use crate::foreground::{
    contains_process_name, executable_path_key, process_executable_path, process_session_id,
    same_executable_path, ProcessActionTarget,
};
use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    control::suspension::{
        self as suspension_control, suspension_error_message, SuspensionController,
        SuspensionError, SuspensionFreezeOutcome, SuspensionTarget,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
    runtime::observations::CycleObservations,
};

const NETWORK_DETECTION_FAILURE_KEY: &str = "network-detection";
const AUDIO_DETECTION_FAILURE_KEY: &str = "audio-detection";
mod wake_activity;

use wake_activity::*;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppSuspensionSnapshot {
    pub enabled: bool,
    pub unsupported: bool,
    pub grace_apps: usize,
    pub suspended_processes: usize,
    pub suspended_process_ids: Vec<u32>,
    pub temporary_thawed_processes: usize,
    pub network_wake_processes: usize,
    pub audio_wake_processes: usize,
    pub background_grace_apps: Vec<String>,
    pub suspended_apps: Vec<String>,
    pub temporary_thawed_apps: Vec<String>,
    pub network_wake_apps: Vec<String>,
    pub audio_wake_apps: Vec<String>,
    pub running_apps: Vec<String>,
    pub status_unknown: bool,
    pub skipped_processes: usize,
    pub failed_actions: usize,
    pub auto_excluded_processes: Vec<String>,
    pub status: AppSuspensionStatus,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AppSuspensionStatus {
    AutomationDisabled,
    #[default]
    Disabled,
    NoRulesConfigured,
    Unsupported,
    ForegroundUnknown,
    SessionUnknown,
    Active,
    Error(String),
}

impl fmt::Display for AppSuspensionStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::AutomationDisabled => "Automation disabled.",
            Self::Disabled => "App Suspension disabled.",
            Self::NoRulesConfigured => "No App Suspension rules configured.",
            Self::Unsupported => "App Suspension unavailable: Windows Job Object freeze is not supported on this system.",
            Self::ForegroundUnknown => "Paused: foreground app is unknown.",
            Self::SessionUnknown => "Paused: current Windows session is unknown.",
            Self::Active => "App Suspension active.",
            Self::Error(error) => error,
        })
    }
}

#[derive(Default)]
pub struct AppSuspensionManager {
    tracked: BTreeMap<String, TrackedApp>,
    suspended: BTreeMap<u32, SuspendedProcess>,
    temporary_thawed: BTreeMap<u32, TemporaryThaw>,
    failure_suppression: ExecutionFailureTracker,
    action_failure_suppression: ExecutionFailureTracker,
    network_snapshot: NetworkConnectionSnapshot,
    network_wake_windows: BTreeMap<String, NetworkWakeWindow>,
    audio_wake_windows: BTreeMap<String, AudioWakeWindow>,
    running_apps: BTreeSet<String>,
    manual_freeze_outcomes: BTreeMap<String, ManualFreezeOutcome>,
    job_freeze_unsupported: bool,
}

type NetworkConnectionSnapshot = BTreeMap<String, NetworkConnections>;
type NetworkConnections = BTreeMap<String, Option<NetworkActivityCounters>>;
type NetworkConnectionsByProcess = BTreeMap<u32, NetworkConnections>;
type NetworkActivityThresholdsByProcess = BTreeMap<String, NetworkActivityThresholds>;

const TCP_STATE_SYN_SENT: u32 = 3;
const TCP_STATE_SYN_RECEIVED: u32 = 4;
const TCP_STATE_ESTABLISHED: u32 = 5;

#[derive(Debug, Clone, Copy)]
struct NetworkWakeWindow {
    wake_until: Instant,
    max_until: Instant,
    suppress_until: Instant,
}

#[derive(Debug, Clone, Copy)]
struct AudioWakeWindow {
    wake_until: Instant,
}

struct TrackedApp {
    background_since: Instant,
}

struct SuspendedProcess {
    process_name: String,
    executable_path: String,
    creation_time: u64,
    suspended_since: Instant,
    manual: bool,
}

impl SuspendedProcess {
    fn suspension_target(&self, process_id: u32) -> SuspensionTarget {
        SuspensionTarget::automatic(
            process_id,
            self.process_name.clone(),
            Path::new(&self.executable_path).to_path_buf(),
            self.creation_time,
            None,
        )
    }
}

struct TemporaryThaw {
    process_name: String,
    executable_path: String,
    thaw_until: Instant,
    reason: TemporaryThawReason,
}

#[derive(Default)]
struct ManualFreezeOutcome {
    attempts: usize,
    failures: usize,
    first_error: Option<String>,
}

#[derive(Clone)]
pub(super) struct TargetProcess {
    process_name: String,
    executable_path: String,
    creation_time: u64,
    is_service_account: Option<bool>,
}

impl TargetProcess {
    fn key(&self) -> String {
        executable_path_key(Path::new(&self.executable_path))
    }

    fn suspension_target(&self, process_id: u32) -> SuspensionTarget {
        SuspensionTarget::automatic(
            process_id,
            self.process_name.clone(),
            Path::new(&self.executable_path).to_path_buf(),
            self.creation_time,
            self.is_service_account,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TemporaryThawReason {
    Fallback,
    NetworkWake,
    AudioWake,
    UserIntent,
}

const USER_INTENT_THAW_SECONDS: u64 = 10;
const MAX_SUSPENSION_DURATION_SECONDS: u64 = 3_600;

fn bounded_suspension_duration(seconds: u64) -> Duration {
    Duration::from_secs(seconds.min(MAX_SUSPENSION_DURATION_SECONDS))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TemporaryThawState {
    None,
    Active,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SuspensionLifecycleState {
    IntentActive,
    BackgroundGrace,
    ReadyToSuspend,
    ManualFreeze,
}

impl SuspensionLifecycleState {
    fn should_suspend(self) -> bool {
        matches!(self, Self::ReadyToSuspend | Self::ManualFreeze)
    }

    fn is_manual_freeze(self) -> bool {
        matches!(self, Self::ManualFreeze)
    }
}

impl AppSuspensionManager {
    pub fn apply_manual_process_action(
        &mut self,
        controller: &mut SuspensionController,
        target: &ProcessActionTarget,
        suspend: bool,
        allow_cross_session: bool,
        action_log: &mut ActionLog,
    ) -> Result<(), String> {
        if suspend {
            let suspension_target = SuspensionTarget::from_action_target(target);
            let result = if self.job_freeze_unsupported {
                Err(SuspensionError::Unsupported)
            } else {
                self.suspend_process(
                    controller,
                    &suspension_target,
                    SuspendedProcess {
                        process_name: target.name.clone(),
                        executable_path: target.executable_path.to_string_lossy().into_owned(),
                        creation_time: target.creation_time,
                        suspended_since: Instant::now(),
                        manual: true,
                    },
                    allow_cross_session,
                )
            };
            if matches!(&result, Err(SuspensionError::Unsupported)) {
                self.job_freeze_unsupported = true;
            }
            action_log.record(
                ActionLogFeature::AppSuspension,
                Some(target.id),
                target.name.clone(),
                if result.is_ok() {
                    ActionLogResult::Applied
                } else {
                    ActionLogResult::Failed
                },
                result.as_ref().map_or_else(
                    |error| error.to_string(),
                    |_| "Manually suspended process.".to_owned(),
                ),
            );
            result.map(|_| ()).map_err(|error| error.to_string())
        } else {
            let suspension_target = SuspensionTarget::from_action_target(target);
            let result = controller
                .thaw_keep_target(&suspension_target, true)
                .map(|_| ());
            if result.is_ok() || !controller.is_frozen_process(target.id) {
                self.suspended.remove(&target.id);
                let now = Instant::now();
                self.set_temporary_thaw(
                    target.id,
                    target.name.clone(),
                    target.executable_path.to_string_lossy().into_owned(),
                    now + Duration::from_secs(USER_INTENT_THAW_SECONDS),
                    TemporaryThawReason::UserIntent,
                );
            }
            action_log.record(
                ActionLogFeature::AppSuspension,
                Some(target.id),
                target.name.clone(),
                if result.is_ok() {
                    ActionLogResult::Restored
                } else {
                    ActionLogResult::Failed
                },
                result.as_ref().map_or_else(
                    |error| error.to_string(),
                    |_| "Manually resumed process.".to_owned(),
                ),
            );
            result.map_err(|error| error.to_string())
        }
    }

    pub fn has_suspended_processes(&self, controller: &SuspensionController) -> bool {
        !self.suspended.is_empty() || controller.managed_process_ids().next().is_some()
    }

    pub fn manual_freeze_result(&self, executable_path: &str) -> Result<(), String> {
        let key = executable_path_key(Path::new(executable_path));
        let Some(outcome) = self.manual_freeze_outcomes.get(&key) else {
            return Err(format!(
                "No App Suspension result was produced for {executable_path}."
            ));
        };
        if outcome.attempts == 0 {
            return Err(format!(
                "No eligible running process matched {executable_path}."
            ));
        }
        if outcome.failures == 0 {
            Ok(())
        } else {
            Err(format!(
                "{} of {} App Suspension actions failed: {}",
                outcome.failures,
                outcome.attempts,
                outcome
                    .first_error
                    .as_deref()
                    .unwrap_or("Unknown App Suspension failure.")
            ))
        }
    }

    pub fn shutdown(
        &mut self,
        controller: &mut SuspensionController,
        action_log: &mut ActionLog,
    ) -> Result<(), String> {
        let failed = self.clear_all(controller, action_log, "Winderust is shutting down");
        if failed == 0 {
            self.suspended.clear();
            self.temporary_thawed.clear();
            Ok(())
        } else {
            Err(format!(
                "Failed to restore {failed} App Suspension process(es)."
            ))
        }
    }

    pub fn release_interactive_process(
        &mut self,
        controller: &mut SuspensionController,
        process_id: u32,
        executable_path: Option<&Path>,
        action_log: &mut ActionLog,
    ) -> Option<AppSuspensionSnapshot> {
        let process_ids = self.interactive_process_ids(controller, process_id, executable_path);
        if process_ids.is_empty() {
            return None;
        }

        let process_ids = process_ids.into_iter().collect::<Vec<_>>();
        let failed_actions = self.release_foreground_processes(
            controller,
            &process_ids,
            action_log,
            "released because the app became interactive",
        );
        Some(self.snapshot(
            true,
            self.job_freeze_unsupported,
            0,
            failed_actions,
            AppSuspensionStatus::Active,
            None,
        ))
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the pass-local observation dependency is clearer here than an unrelated argument bundle"
    )]
    pub fn update(
        &mut self,
        controller: &mut SuspensionController,
        settings: &AppSuspensionSettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        manual_freeze_processes: &[String],
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> AppSuspensionSnapshot {
        self.manual_freeze_outcomes.clear();
        for executable_path in manual_freeze_processes {
            self.manual_freeze_outcomes
                .entry(executable_path_key(Path::new(executable_path)))
                .or_default();
        }
        let now = Instant::now();
        controller.reconcile_pending(false);
        self.suspended.retain(|process_id, process| {
            controller.is_frozen_target(&process.suspension_target(*process_id))
        });

        if !automation_enabled {
            let failed = self.clear_automatic(controller, action_log, "automation disabled");
            self.failure_suppression.clear();
            self.action_failure_suppression.clear();
            return self.snapshot(
                false,
                self.job_freeze_unsupported,
                0,
                failed,
                AppSuspensionStatus::AutomationDisabled,
                None,
            );
        }

        if !settings.enabled {
            let failed = self.clear_automatic(controller, action_log, "App Suspension disabled");
            self.failure_suppression.clear();
            self.action_failure_suppression.clear();
            return self.snapshot(
                false,
                self.job_freeze_unsupported,
                0,
                failed,
                AppSuspensionStatus::Disabled,
                None,
            );
        }

        let enabled_process_names = settings
            .suspendable_apps
            .iter()
            .filter(|rule| rule.enabled && Path::new(rule.executable_path.trim()).is_absolute())
            .filter_map(|rule| Path::new(&rule.executable_path).file_name())
            .filter_map(|name| name.to_str())
            .collect::<Vec<_>>();
        if enabled_process_names.is_empty() {
            let failed = self.clear_automatic(
                controller,
                action_log,
                "no App Suspension rules are enabled",
            );
            self.failure_suppression.clear();
            self.action_failure_suppression.clear();
            return self.snapshot(
                true,
                self.job_freeze_unsupported,
                0,
                failed,
                AppSuspensionStatus::NoRulesConfigured,
                None,
            );
        }

        let mut failed_actions = 0;
        if self.job_freeze_unsupported {
            failed_actions +=
                self.clear_automatic(controller, action_log, "Job Object freeze unsupported");
            return self.snapshot(
                true,
                true,
                0,
                failed_actions,
                AppSuspensionStatus::Unsupported,
                None,
            );
        }

        let Some(foreground_process_id) = foreground_process_id else {
            return self.pause_without_clearing(
                AppSuspensionStatus::ForegroundUnknown,
                failed_actions,
                None,
            );
        };

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            return self.pause_without_clearing(
                AppSuspensionStatus::SessionUnknown,
                failed_actions,
                None,
            );
        };

        let processes = match observations.processes_with_paths() {
            Ok(processes) => processes,
            Err(err) => {
                failed_actions += 1;
                return self.pause_without_clearing(
                    AppSuspensionStatus::Error(err.clone()),
                    failed_actions,
                    Some(err),
                );
            }
        };

        let foreground_executable_path = processes
            .iter()
            .find(|process| process.id == foreground_process_id)
            .and_then(process_executable_path);
        let delay = Duration::from_secs(settings.background_delay_seconds);
        let mut target_processes = BTreeMap::new();
        let mut running_apps = BTreeSet::new();

        for process in processes.iter() {
            if process.id == 0
                || process.is_critical != Some(false)
                || process.id == current_process_id
                || is_builtin_excluded(&process.name)
                || !contains_process_name(&enabled_process_names, &process.name)
                || process.creation_time.is_none()
                || process.session_id.is_none_or(|session_id| session_id == 0)
                || process.is_service_account != Some(false)
            {
                continue;
            }

            if !allow_cross_session_process_control
                && process_session_id(process.id) != Some(current_session_id)
            {
                continue;
            }
            let Some(executable_path) = process_executable_path(process) else {
                continue;
            };
            let executable_path = executable_path.to_string_lossy().into_owned();
            if !settings.suspendable_app_enabled_for(&executable_path) {
                continue;
            }

            running_apps.insert(executable_path_key(Path::new(&executable_path)));
            if should_skip_foreground_process(
                process.id,
                Path::new(&executable_path),
                foreground_process_id,
                foreground_executable_path.as_deref(),
            ) {
                continue;
            }

            target_processes.insert(
                process.id,
                TargetProcess {
                    process_name: process.name.clone(),
                    executable_path,
                    creation_time: process.creation_time.unwrap_or_default(),
                    is_service_account: process.is_service_account,
                },
            );
        }
        self.running_apps = running_apps;

        let stale_process_ids = target_processes
            .iter()
            .filter_map(|(process_id, process)| {
                let managed = self.suspended.contains_key(process_id)
                    || self.temporary_thawed.contains_key(process_id)
                    || controller.contains_process(*process_id);
                (managed && !controller.matches_target(&process.suspension_target(*process_id)))
                    .then_some(*process_id)
            })
            .collect::<Vec<_>>();
        for process_id in stale_process_ids {
            let _ = self.release_process_state(
                controller,
                process_id,
                action_log,
                "process instance changed",
            );
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let active_target_paths = target_processes
            .values()
            .map(TargetProcess::key)
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_paths);
        let mut active_action_failure_keys = BTreeSet::new();
        if settings.network_wake_enabled {
            active_action_failure_keys.insert(NETWORK_DETECTION_FAILURE_KEY.to_owned());
        }
        if settings.audio_wake_enabled {
            active_action_failure_keys.insert(AUDIO_DETECTION_FAILURE_KEY.to_owned());
        }
        self.action_failure_suppression
            .retain_keys(&active_action_failure_keys);
        failed_actions += self.release_non_targets(
            controller,
            &target_ids,
            action_log,
            "process no longer matches an App Suspension rule",
        );
        self.tracked
            .retain(|path, _process| active_target_paths.contains(path));
        self.temporary_thawed
            .retain(|process_id, _process| target_ids.contains(process_id));
        let network_target_processes = target_processes
            .iter()
            .filter(|(_process_id, process)| {
                settings.network_wake_enabled_for(&process.executable_path)
            })
            .map(|(process_id, process)| (*process_id, process.clone()))
            .collect::<BTreeMap<_, _>>();
        let network_thresholds = network_activity_thresholds(settings, &network_target_processes);
        let network_target_process_names = network_target_processes
            .values()
            .map(TargetProcess::key)
            .collect::<BTreeSet<_>>();
        let audio_target_processes = target_processes
            .iter()
            .filter(|(_process_id, process)| {
                settings.audio_wake_enabled_for(&process.executable_path)
            })
            .map(|(process_id, process)| (*process_id, process.clone()))
            .collect::<BTreeMap<_, _>>();
        let audio_target_process_names = audio_target_processes
            .values()
            .map(TargetProcess::key)
            .collect::<BTreeSet<_>>();
        self.network_wake_windows
            .retain(|path, _window| !contains_process(manual_freeze_processes, path));
        self.audio_wake_windows
            .retain(|path, _window| !contains_process(manual_freeze_processes, path));
        if settings.network_wake_enabled {
            self.prune_network_wake_windows(&network_target_process_names, now);
        } else {
            self.network_wake_windows.clear();
        }
        if settings.audio_wake_enabled {
            self.prune_audio_wake_windows(&audio_target_process_names, now);
        } else {
            self.audio_wake_windows.clear();
        }

        let mut skipped_processes = 0;
        let mut last_error = None;
        let mut unsupported = false;
        let (network_snapshot, network_event_names) = if settings.network_wake_enabled
            && !network_target_process_names.is_empty()
            && !self.is_action_suppressed(
                NETWORK_DETECTION_FAILURE_KEY,
                "network activity detection",
                action_log,
            ) {
            match network_connection_snapshot(&network_target_processes) {
                Ok(snapshot) => {
                    self.action_failure_suppression
                        .clear_key_failure(NETWORK_DETECTION_FAILURE_KEY);
                    let wake_names = network_process_names_with_activity(
                        &self.network_snapshot,
                        &snapshot,
                        &network_thresholds,
                    );
                    (
                        snapshot,
                        eligible_network_wake_names(&wake_names, &network_target_process_names),
                    )
                }
                Err(err) => {
                    failed_actions += 1;
                    self.action_failure_suppression
                        .record_key_failure(NETWORK_DETECTION_FAILURE_KEY);
                    action_log.record(
                        ActionLogFeature::AppSuspension,
                        None,
                        "",
                        ActionLogResult::Failed,
                        err.clone(),
                    );
                    last_error = Some(err);
                    (self.network_snapshot.clone(), BTreeSet::new())
                }
            }
        } else {
            (BTreeMap::new(), BTreeSet::new())
        };
        if settings.network_wake_enabled {
            self.extend_network_wake_windows(settings, &network_event_names, now);
        }
        if settings.audio_wake_enabled
            && !self.is_action_suppressed(
                AUDIO_DETECTION_FAILURE_KEY,
                "audio activity detection",
                action_log,
            )
        {
            match audio_process_names_with_activity(&audio_target_processes) {
                Ok(audio_event_names) => {
                    self.action_failure_suppression
                        .clear_key_failure(AUDIO_DETECTION_FAILURE_KEY);
                    self.extend_audio_wake_windows(settings, &audio_event_names, now);
                }
                Err(err) => {
                    failed_actions += 1;
                    self.action_failure_suppression
                        .record_key_failure(AUDIO_DETECTION_FAILURE_KEY);
                    action_log.record(
                        ActionLogFeature::AppSuspension,
                        None,
                        "",
                        ActionLogResult::Failed,
                        err.clone(),
                    );
                    if last_error.is_none() {
                        last_error = Some(err);
                    }
                }
            }
        }
        let network_wake_names = self.active_network_wake_names(now);
        failed_actions += self.apply_network_wake(
            controller,
            &target_processes,
            &network_wake_names,
            now,
            action_log,
        );
        let audio_wake_names = self.active_audio_wake_names(now);
        failed_actions += self.apply_audio_wake(
            controller,
            &target_processes,
            &audio_wake_names,
            now,
            action_log,
        );
        self.network_snapshot = network_snapshot;
        failed_actions +=
            self.release_for_temporary_thaw(controller, settings, &target_ids, now, action_log);

        let mut auto_excluded_processes = BTreeSet::new();
        let mut suspended_app_names = BTreeSet::new();
        for (process_id, process) in target_processes {
            let process_name = process.process_name.clone();
            let manual_freeze = contains_process(manual_freeze_processes, &process.executable_path);
            if self.suspended.contains_key(&process_id) {
                if controller.matches_target(&process.suspension_target(process_id)) {
                    if manual_freeze {
                        self.record_manual_freeze_result(&process.executable_path, Ok(()));
                    }
                    continue;
                }
                let _ = self.release_process_state(
                    controller,
                    process_id,
                    action_log,
                    "process instance changed",
                );
            }

            if self.is_process_suppressed(
                process_id,
                &process_name,
                &process.executable_path,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                if manual_freeze {
                    self.record_manual_freeze_result(
                        &process.executable_path,
                        Err(
                            "The process is excluded after repeated suspension failures."
                                .to_owned(),
                        ),
                    );
                }
                continue;
            }

            let lifecycle = self.suspension_lifecycle_state(
                controller,
                process_id,
                &process,
                now,
                delay,
                manual_freeze,
            );
            if !lifecycle.should_suspend() {
                continue;
            }

            match self.suspend_process(
                controller,
                &process.suspension_target(process_id),
                SuspendedProcess {
                    process_name: process_name.clone(),
                    executable_path: process.executable_path.clone(),
                    creation_time: process.creation_time,
                    suspended_since: now,
                    manual: false,
                },
                allow_cross_session_process_control,
            ) {
                Ok(()) => {
                    if manual_freeze {
                        self.record_manual_freeze_result(&process.executable_path, Ok(()));
                    }
                    self.failure_suppression
                        .clear_process_failure(&process.key());
                    action_log.record(
                        ActionLogFeature::AppSuspension,
                        Some(process_id),
                        process_name.clone(),
                        ActionLogResult::Applied,
                        if lifecycle.is_manual_freeze() {
                            "Manually froze background process."
                        } else {
                            "Froze background process after delay."
                        },
                    );
                    suspended_app_names.insert(process.key());
                }
                Err(error @ SuspensionError::ProcessExited) => {
                    if manual_freeze {
                        self.record_manual_freeze_result(
                            &process.executable_path,
                            Err(error.to_string()),
                        );
                    }
                    skipped_processes += 1;
                    let _ = self.release_process_state(
                        controller,
                        process_id,
                        action_log,
                        "process exited",
                    );
                }
                Err(error @ (SuspensionError::AccessDenied | SuspensionError::NotSupported)) => {
                    if manual_freeze {
                        self.record_manual_freeze_result(
                            &process.executable_path,
                            Err(error.to_string()),
                        );
                    }
                    self.failure_suppression
                        .suppress_process_failure(&process.key());
                    skipped_processes += 1;
                    action_log.record(
                        ActionLogFeature::AppSuspension,
                        Some(process_id),
                        process_name,
                        ActionLogResult::Skipped,
                        "Skipped because the process cannot be frozen.",
                    );
                }
                Err(error @ SuspensionError::Unsupported) => {
                    if manual_freeze {
                        self.record_manual_freeze_result(
                            &process.executable_path,
                            Err(error.to_string()),
                        );
                    }
                    skipped_processes += 1;
                    unsupported = true;
                    self.job_freeze_unsupported = true;
                    action_log.record(
                        ActionLogFeature::AppSuspension,
                        Some(process_id),
                        process_name,
                        ActionLogResult::Skipped,
                        "Skipped because Windows Job Object freeze is unsupported.",
                    );
                    failed_actions += self.clear_automatic(
                        controller,
                        action_log,
                        "Job Object freeze unsupported",
                    );
                    break;
                }
                Err(error @ (SuspensionError::Failed(_) | SuspensionError::RetryPending(_))) => {
                    if manual_freeze {
                        self.record_manual_freeze_result(
                            &process.executable_path,
                            Err(error.to_string()),
                        );
                    }
                    if error.should_report() {
                        let error = error.to_string();
                        failed_actions += 1;
                        self.failure_suppression
                            .record_process_failure(&process.key());
                        action_log.record(
                            ActionLogFeature::AppSuspension,
                            Some(process_id),
                            process_name,
                            ActionLogResult::Failed,
                            error.clone(),
                        );
                        if last_error.is_none() {
                            last_error = Some(error);
                        }
                    }
                }
            }
        }
        for process_name in suspended_app_names {
            self.tracked.remove(&process_name);
        }

        let mut snapshot = self.snapshot(
            true,
            unsupported,
            skipped_processes,
            failed_actions,
            if unsupported {
                AppSuspensionStatus::Unsupported
            } else {
                AppSuspensionStatus::Active
            },
            last_error,
        );
        snapshot.auto_excluded_processes = auto_excluded_processes.into_iter().collect();
        snapshot
    }

    fn release_non_targets(
        &mut self,
        controller: &mut SuspensionController,
        target_ids: &BTreeSet<u32>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> usize {
        let process_ids = self
            .managed_process_ids(controller)
            .into_iter()
            .filter(|process_id| {
                !target_ids.contains(process_id)
                    && !self
                        .suspended
                        .get(process_id)
                        .is_some_and(|process| process.manual)
            })
            .collect::<Vec<_>>();

        self.release_processes(controller, &process_ids, action_log, reason)
    }

    fn clear_all(
        &mut self,
        controller: &mut SuspensionController,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> usize {
        self.tracked.clear();
        self.network_snapshot.clear();
        self.network_wake_windows.clear();
        self.audio_wake_windows.clear();
        self.running_apps.clear();
        let process_ids = self
            .managed_process_ids(controller)
            .into_iter()
            .collect::<Vec<_>>();
        let failed = self.release_processes(controller, &process_ids, action_log, reason);
        self.temporary_thawed.clear();
        failed
    }

    fn clear_automatic(
        &mut self,
        controller: &mut SuspensionController,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> usize {
        self.tracked.clear();
        self.network_snapshot.clear();
        self.network_wake_windows.clear();
        self.audio_wake_windows.clear();
        self.running_apps.clear();
        let manual_process_ids = self
            .suspended
            .iter()
            .filter_map(|(process_id, process)| process.manual.then_some(*process_id))
            .collect::<BTreeSet<_>>();
        let process_ids = self
            .managed_process_ids(controller)
            .into_iter()
            .filter(|process_id| !manual_process_ids.contains(process_id))
            .collect::<Vec<_>>();
        let failed = self.release_processes(controller, &process_ids, action_log, reason);
        self.temporary_thawed.clear();
        failed
    }

    fn pause_without_clearing(
        &mut self,
        status: AppSuspensionStatus,
        failed_actions: usize,
        last_error: Option<String>,
    ) -> AppSuspensionSnapshot {
        self.tracked.clear();
        self.network_snapshot.clear();
        let mut snapshot = self.snapshot(
            true,
            self.job_freeze_unsupported,
            0,
            failed_actions,
            status,
            last_error,
        );
        snapshot.status_unknown = true;
        snapshot
    }

    fn release_processes(
        &mut self,
        controller: &mut SuspensionController,
        process_ids: &[u32],
        action_log: &mut ActionLog,
        reason: &str,
    ) -> usize {
        let mut failed = 0;
        for process_id in process_ids {
            let process_name = self
                .controlled_process_name(*process_id)
                .unwrap_or("")
                .to_owned();
            let was_suspended = self.suspended.contains_key(process_id);
            match controller.release_process(*process_id, false) {
                Ok(_) => {
                    self.suspended.remove(process_id);
                    if was_suspended {
                        action_log.record(
                            ActionLogFeature::AppSuspension,
                            Some(*process_id),
                            process_name,
                            ActionLogResult::Restored,
                            reason.to_owned(),
                        );
                    }
                }
                Err(err) => {
                    if !controller.is_frozen_process(*process_id) {
                        self.suspended.remove(process_id);
                    }
                    if err.should_report() {
                        failed += 1;
                        action_log.record(
                            ActionLogFeature::AppSuspension,
                            Some(*process_id),
                            process_name,
                            ActionLogResult::Failed,
                            suspension_error_message(&err),
                        );
                    }
                }
            }
            self.temporary_thawed.remove(process_id);
        }
        failed
    }

    fn release_process_state(
        &mut self,
        controller: &mut SuspensionController,
        process_id: u32,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> Result<(), SuspensionError> {
        if let Some(process_key) = self.controlled_process_key(process_id) {
            self.tracked.remove(&process_key);
        }
        let process_name = self
            .controlled_process_name(process_id)
            .unwrap_or("")
            .to_owned();
        let result = controller.release_process(process_id, false).map(|_| ());
        if result.is_ok() || !controller.is_frozen_process(process_id) {
            self.suspended.remove(&process_id);
        }
        self.temporary_thawed.remove(&process_id);
        if let Err(error) = &result {
            if error.should_report() {
                action_log.record(
                    ActionLogFeature::AppSuspension,
                    Some(process_id),
                    process_name,
                    ActionLogResult::Failed,
                    format!("{reason}: {error}"),
                );
            }
        }
        result
    }

    fn release_foreground_processes(
        &mut self,
        controller: &mut SuspensionController,
        process_ids: &[u32],
        action_log: &mut ActionLog,
        reason: &str,
    ) -> usize {
        let mut failed = 0;
        for process_id in process_ids {
            let process_name = self.controlled_process_name(*process_id).map(str::to_owned);
            let was_suspended = self.suspended.contains_key(process_id);
            match controller.release_process(*process_id, true) {
                Ok(_) => {
                    if was_suspended {
                        if let Some(process_name) = process_name.clone() {
                            action_log.record(
                                ActionLogFeature::AppSuspension,
                                Some(*process_id),
                                process_name,
                                ActionLogResult::Restored,
                                reason.to_owned(),
                            );
                        }
                    }
                }
                Err(err) => {
                    if !controller.is_frozen_process(*process_id) {
                        self.suspended.remove(process_id);
                    }
                    if err.should_report() {
                        failed += 1;
                        action_log.record(
                            ActionLogFeature::AppSuspension,
                            Some(*process_id),
                            process_name.unwrap_or_default(),
                            ActionLogResult::Failed,
                            suspension_error_message(&err),
                        );
                    }
                    continue;
                }
            }

            if let Some(process_key) = self.controlled_process_key(*process_id) {
                self.tracked.remove(&process_key);
            }
            self.suspended.remove(process_id);
            self.temporary_thawed.remove(process_id);
        }

        failed
    }

    pub fn release_window_owner_processes_for_user_intent(
        &mut self,
        controller: &mut SuspensionController,
        window_owner_process_ids: &BTreeSet<u32>,
        action_log: &mut ActionLog,
    ) -> Option<AppSuspensionSnapshot> {
        let process_ids = self
            .window_owner_suspended_process_ids(window_owner_process_ids)
            .into_iter()
            .collect::<Vec<_>>();
        if process_ids.is_empty() {
            return None;
        }
        Some(self.release_process_ids_for_user_intent(controller, &process_ids, action_log))
    }

    pub fn release_all_suspended_processes_for_user_intent(
        &mut self,
        controller: &mut SuspensionController,
        action_log: &mut ActionLog,
    ) -> Option<AppSuspensionSnapshot> {
        let process_ids = self.suspended.keys().copied().collect::<Vec<_>>();
        if process_ids.is_empty() {
            return None;
        }
        Some(self.release_process_ids_for_user_intent(controller, &process_ids, action_log))
    }

    pub fn release_suspended_path_for_user_intent(
        &mut self,
        controller: &mut SuspensionController,
        executable_path: &Path,
        action_log: &mut ActionLog,
    ) -> AppSuspensionSnapshot {
        let process_ids = self
            .suspended
            .iter()
            .filter(|(_process_id, process)| {
                same_executable_path(Path::new(&process.executable_path), executable_path)
            })
            .map(|(process_id, _process)| *process_id)
            .collect::<Vec<_>>();
        self.release_process_ids_for_user_intent(controller, &process_ids, action_log)
    }

    fn release_process_ids_for_user_intent(
        &mut self,
        controller: &mut SuspensionController,
        process_ids: &[u32],
        action_log: &mut ActionLog,
    ) -> AppSuspensionSnapshot {
        let failed_actions = self.thaw_processes_for_user_intent(
            controller,
            process_ids,
            Instant::now(),
            action_log,
        );
        self.snapshot(
            true,
            self.job_freeze_unsupported,
            0,
            failed_actions,
            AppSuspensionStatus::Active,
            None,
        )
    }

    fn thaw_processes_for_user_intent(
        &mut self,
        controller: &mut SuspensionController,
        process_ids: &[u32],
        now: Instant,
        action_log: &mut ActionLog,
    ) -> usize {
        let mut failed = 0;
        for process_id in process_ids {
            let process = self
                .controlled_process(*process_id)
                .map(|(name, path)| (name.to_owned(), path.to_owned()));
            if let Some((process_name, executable_path)) = process.clone() {
                if !self.managed_process_matches_target(controller, *process_id, &executable_path) {
                    let _ = self.release_process_state(
                        controller,
                        *process_id,
                        action_log,
                        "process instance changed",
                    );
                    continue;
                }
                if self.suspended.contains_key(process_id) {
                    match controller.thaw_keep_process(*process_id, true) {
                        Ok(_) => {
                            action_log.record(
                                ActionLogFeature::AppSuspension,
                                Some(*process_id),
                                process_name.clone(),
                                ActionLogResult::Restored,
                                "Thawed because the user interacted with the window.",
                            );
                        }
                        Err(err) => {
                            let thawed = !controller.is_frozen_process(*process_id);
                            if thawed {
                                if let Some(process_key) = self.controlled_process_key(*process_id)
                                {
                                    self.tracked.remove(&process_key);
                                }
                                self.suspended.remove(process_id);
                                self.set_temporary_thaw(
                                    *process_id,
                                    process_name.clone(),
                                    executable_path.clone(),
                                    now + Duration::from_secs(USER_INTENT_THAW_SECONDS),
                                    TemporaryThawReason::UserIntent,
                                );
                            }
                            if err.should_report() {
                                failed += 1;
                                action_log.record(
                                    ActionLogFeature::AppSuspension,
                                    Some(*process_id),
                                    process_name,
                                    ActionLogResult::Failed,
                                    suspension_error_message(&err),
                                );
                            }
                            continue;
                        }
                    }
                }
            }

            if let Some(process_key) = self.controlled_process_key(*process_id) {
                self.tracked.remove(&process_key);
            }
            self.suspended.remove(process_id);
            if let Some((process_name, executable_path)) = process {
                self.set_temporary_thaw(
                    *process_id,
                    process_name,
                    executable_path,
                    now + Duration::from_secs(USER_INTENT_THAW_SECONDS),
                    TemporaryThawReason::UserIntent,
                );
            } else {
                self.temporary_thawed.remove(process_id);
            }
        }

        failed
    }

    fn managed_process_ids(&self, controller: &SuspensionController) -> BTreeSet<u32> {
        self.suspended
            .keys()
            .copied()
            .chain(controller.managed_process_ids())
            .chain(self.temporary_thawed.keys().copied())
            .collect()
    }

    fn interactive_process_ids(
        &self,
        controller: &SuspensionController,
        process_id: u32,
        executable_path: Option<&Path>,
    ) -> BTreeSet<u32> {
        let mut process_ids = BTreeSet::new();
        if self.suspended.contains_key(&process_id)
            || self.temporary_thawed.contains_key(&process_id)
            || controller.contains_process(process_id)
        {
            process_ids.insert(process_id);
        }

        let executable_path = executable_path
            .map(|path| path.to_string_lossy().into_owned())
            .or_else(|| {
                self.controlled_process(process_id)
                    .map(|(_name, path)| path.to_owned())
            });
        let Some(executable_path) = executable_path else {
            return process_ids;
        };

        process_ids.extend(self.controlled_process_ids_by_path(Path::new(&executable_path)));
        process_ids
    }

    fn controlled_process(&self, process_id: u32) -> Option<(&str, &str)> {
        self.suspended
            .get(&process_id)
            .map(|process| {
                (
                    process.process_name.as_str(),
                    process.executable_path.as_str(),
                )
            })
            .or_else(|| {
                self.temporary_thawed.get(&process_id).map(|process| {
                    (
                        process.process_name.as_str(),
                        process.executable_path.as_str(),
                    )
                })
            })
    }

    fn managed_process_matches_target(
        &self,
        controller: &SuspensionController,
        process_id: u32,
        executable_path: &str,
    ) -> bool {
        self.controlled_process(process_id).is_some_and(
            |(_process_name, managed_executable_path)| {
                same_executable_path(
                    Path::new(managed_executable_path),
                    Path::new(executable_path),
                )
            },
        ) && controller.matches_process_path(process_id, Path::new(executable_path))
    }

    fn controlled_process_name(&self, process_id: u32) -> Option<&str> {
        self.controlled_process(process_id)
            .map(|(process_name, _executable_path)| process_name)
    }

    fn controlled_process_key(&self, process_id: u32) -> Option<String> {
        self.controlled_process(process_id)
            .map(|(_process_name, executable_path)| executable_path_key(Path::new(executable_path)))
    }

    fn controlled_process_ids_by_path(&self, executable_path: &Path) -> BTreeSet<u32> {
        self.suspended
            .iter()
            .filter(|(_process_id, process)| {
                same_executable_path(Path::new(&process.executable_path), executable_path)
            })
            .map(|(process_id, _process)| *process_id)
            .chain(
                self.temporary_thawed
                    .iter()
                    .filter(|(_process_id, process)| {
                        same_executable_path(Path::new(&process.executable_path), executable_path)
                    })
                    .map(|(process_id, _process)| *process_id),
            )
            .collect()
    }

    fn window_owner_suspended_process_ids(
        &self,
        window_owner_process_ids: &BTreeSet<u32>,
    ) -> BTreeSet<u32> {
        window_owner_process_ids
            .iter()
            .copied()
            .filter(|process_id| self.suspended.contains_key(process_id))
            .collect()
    }

    fn release_for_temporary_thaw(
        &mut self,
        controller: &mut SuspensionController,
        settings: &AppSuspensionSettings,
        target_ids: &BTreeSet<u32>,
        now: Instant,
        action_log: &mut ActionLog,
    ) -> usize {
        if !settings.temporary_thaw_enabled
            || settings.temporary_thaw_interval_seconds == 0
            || settings.temporary_thaw_duration_seconds == 0
        {
            return 0;
        }

        let interval = Duration::from_secs(settings.temporary_thaw_interval_seconds);
        let duration = bounded_suspension_duration(settings.temporary_thaw_duration_seconds);
        let process_ids = self
            .suspended
            .iter()
            .filter(|(process_id, process)| {
                target_ids.contains(process_id)
                    && now.duration_since(process.suspended_since) >= interval
            })
            .map(|(process_id, _process)| *process_id)
            .collect::<Vec<_>>();

        let mut failed = 0;
        for process_id in process_ids {
            if let Some(process) = self.suspended.get(&process_id) {
                let process_name = process.process_name.clone();
                let executable_path = process.executable_path.clone();
                match controller.thaw_keep_process(process_id, false) {
                    Ok(_) => {
                        self.suspended.remove(&process_id);
                        action_log.record(
                            ActionLogFeature::AppSuspension,
                            Some(process_id),
                            process_name.clone(),
                            ActionLogResult::Restored,
                            "Temporary thaw interval elapsed.",
                        );
                        self.set_temporary_thaw(
                            process_id,
                            process_name,
                            executable_path,
                            now + duration,
                            TemporaryThawReason::Fallback,
                        );
                    }
                    Err(error) => {
                        let thawed = !controller.is_frozen_process(process_id);
                        if thawed {
                            self.suspended.remove(&process_id);
                        }
                        if error.should_report() {
                            failed += 1;
                            action_log.record(
                                ActionLogFeature::AppSuspension,
                                Some(process_id),
                                process_name.clone(),
                                ActionLogResult::Failed,
                                suspension_error_message(&error),
                            );
                        }
                        if thawed {
                            self.set_temporary_thaw(
                                process_id,
                                process_name,
                                executable_path,
                                now + duration,
                                TemporaryThawReason::Fallback,
                            );
                        }
                    }
                }
            }
        }

        failed
    }

    fn apply_network_wake(
        &mut self,
        controller: &mut SuspensionController,
        target_processes: &BTreeMap<u32, TargetProcess>,
        network_process_names: &BTreeSet<String>,
        now: Instant,
        action_log: &mut ActionLog,
    ) -> usize {
        let process_ids = target_processes
            .iter()
            .filter(|(_process_id, process)| network_process_names.contains(&process.key()))
            .map(|(process_id, process)| (*process_id, process.clone()))
            .collect::<Vec<_>>();

        let mut failed = 0;
        for (process_id, process) in process_ids {
            let process_name = process.process_name.clone();
            let Some(thaw_until) = self.active_network_wake_until(&process.executable_path, now)
            else {
                continue;
            };

            let was_suspended = self.suspended.contains_key(&process_id);
            let mut cleanup_failed = false;
            if was_suspended {
                match controller.thaw_keep_process(process_id, false) {
                    Ok(_) => {}
                    Err(err) => {
                        if controller.is_frozen_process(process_id) {
                            if err.should_report() {
                                failed += 1;
                                action_log.record(
                                    ActionLogFeature::AppSuspension,
                                    Some(process_id),
                                    process_name.clone(),
                                    ActionLogResult::Failed,
                                    suspension_error_message(&err),
                                );
                            }
                            continue;
                        } else {
                            self.suspended.remove(&process_id);
                            cleanup_failed = true;
                        }
                        if err.should_report() {
                            failed += 1;
                            action_log.record(
                                ActionLogFeature::AppSuspension,
                                Some(process_id),
                                process_name.clone(),
                                ActionLogResult::Failed,
                                suspension_error_message(&err),
                            );
                        }
                    }
                }
            }
            self.suspended.remove(&process_id);

            self.tracked.remove(&process.key());
            if was_suspended && !cleanup_failed {
                action_log.record(
                    ActionLogFeature::AppSuspension,
                    Some(process_id),
                    process_name.clone(),
                    ActionLogResult::Restored,
                    "Network activity woke the suspended process.",
                );
            }
            self.set_temporary_thaw(
                process_id,
                process_name,
                process.executable_path,
                thaw_until,
                TemporaryThawReason::NetworkWake,
            );
        }

        failed
    }

    fn apply_audio_wake(
        &mut self,
        controller: &mut SuspensionController,
        target_processes: &BTreeMap<u32, TargetProcess>,
        audio_process_names: &BTreeSet<String>,
        now: Instant,
        action_log: &mut ActionLog,
    ) -> usize {
        let process_ids = target_processes
            .iter()
            .filter(|(_process_id, process)| audio_process_names.contains(&process.key()))
            .map(|(process_id, process)| (*process_id, process.clone()))
            .collect::<Vec<_>>();

        let mut failed = 0;
        for (process_id, process) in process_ids {
            let process_name = process.process_name.clone();
            let Some(thaw_until) = self.active_audio_wake_until(&process.executable_path, now)
            else {
                continue;
            };

            let was_suspended = self.suspended.contains_key(&process_id);
            let mut cleanup_failed = false;
            if was_suspended {
                match controller.thaw_keep_process(process_id, false) {
                    Ok(_) => {}
                    Err(err) => {
                        if controller.is_frozen_process(process_id) {
                            if err.should_report() {
                                failed += 1;
                                action_log.record(
                                    ActionLogFeature::AppSuspension,
                                    Some(process_id),
                                    process_name.clone(),
                                    ActionLogResult::Failed,
                                    suspension_error_message(&err),
                                );
                            }
                            continue;
                        } else {
                            self.suspended.remove(&process_id);
                            cleanup_failed = true;
                        }
                        if err.should_report() {
                            failed += 1;
                            action_log.record(
                                ActionLogFeature::AppSuspension,
                                Some(process_id),
                                process_name.clone(),
                                ActionLogResult::Failed,
                                suspension_error_message(&err),
                            );
                        }
                    }
                }
            }
            self.suspended.remove(&process_id);

            self.tracked.remove(&process.key());
            if was_suspended && !cleanup_failed {
                action_log.record(
                    ActionLogFeature::AppSuspension,
                    Some(process_id),
                    process_name.clone(),
                    ActionLogResult::Restored,
                    "Audio activity woke the suspended process.",
                );
            }
            self.set_temporary_thaw(
                process_id,
                process_name,
                process.executable_path,
                thaw_until,
                TemporaryThawReason::AudioWake,
            );
        }

        failed
    }

    fn set_temporary_thaw(
        &mut self,
        process_id: u32,
        process_name: String,
        executable_path: String,
        thaw_until: Instant,
        reason: TemporaryThawReason,
    ) {
        if let Some(existing) = self.temporary_thawed.get_mut(&process_id) {
            if same_executable_path(
                Path::new(&existing.executable_path),
                Path::new(&executable_path),
            ) {
                existing.process_name = process_name;
                existing.executable_path = executable_path;
                if existing.thaw_until < thaw_until {
                    existing.thaw_until = thaw_until;
                    existing.reason = reason;
                }
                return;
            }
        }
        self.temporary_thawed.insert(
            process_id,
            TemporaryThaw {
                process_name,
                executable_path,
                thaw_until,
                reason,
            },
        );
    }

    fn extend_network_wake_windows(
        &mut self,
        settings: &AppSuspensionSettings,
        network_process_names: &BTreeSet<String>,
        now: Instant,
    ) {
        let Some(duration) = network_wake_duration(settings) else {
            return;
        };

        for process_name in network_process_names {
            let wake_until = now + duration;
            let max_until = now + duration.saturating_mul(2);
            let suppress_until = now + duration.saturating_mul(3);
            self.network_wake_windows
                .entry(process_name.clone())
                .and_modify(|window| {
                    if now < window.max_until {
                        window.wake_until = window.wake_until.max(wake_until.min(window.max_until));
                    }
                })
                .or_insert(NetworkWakeWindow {
                    wake_until,
                    max_until,
                    suppress_until,
                });
        }
    }

    fn prune_network_wake_windows(
        &mut self,
        target_process_names: &BTreeSet<String>,
        now: Instant,
    ) {
        self.network_wake_windows.retain(|process_name, window| {
            target_process_names.contains(process_name) && now < window.suppress_until
        });
    }

    fn extend_audio_wake_windows(
        &mut self,
        settings: &AppSuspensionSettings,
        audio_process_names: &BTreeSet<String>,
        now: Instant,
    ) {
        let Some(duration) = audio_wake_duration(settings) else {
            return;
        };

        for process_name in audio_process_names {
            self.audio_wake_windows.insert(
                process_name.clone(),
                AudioWakeWindow {
                    wake_until: now + duration,
                },
            );
        }
    }

    fn prune_audio_wake_windows(&mut self, target_process_names: &BTreeSet<String>, now: Instant) {
        self.audio_wake_windows.retain(|process_name, window| {
            target_process_names.contains(process_name) && now < window.wake_until
        });
    }

    fn active_network_wake_names(&self, now: Instant) -> BTreeSet<String> {
        self.network_wake_windows
            .iter()
            .filter(|(_process_name, window)| now < window.wake_until)
            .map(|(process_name, _window)| process_name.clone())
            .collect()
    }

    fn active_audio_wake_names(&self, now: Instant) -> BTreeSet<String> {
        self.audio_wake_windows
            .iter()
            .filter(|(_process_name, window)| now < window.wake_until)
            .map(|(process_name, _window)| process_name.clone())
            .collect()
    }

    fn active_network_wake_until(&self, executable_path: &str, now: Instant) -> Option<Instant> {
        let window = self
            .network_wake_windows
            .get(&executable_path_key(Path::new(executable_path)))?;
        (now < window.wake_until).then_some(window.wake_until)
    }

    fn active_audio_wake_until(&self, executable_path: &str, now: Instant) -> Option<Instant> {
        let window = self
            .audio_wake_windows
            .get(&executable_path_key(Path::new(executable_path)))?;
        (now < window.wake_until).then_some(window.wake_until)
    }

    fn temporary_thaw_state(
        &mut self,
        controller: &mut SuspensionController,
        process_id: u32,
        process_name: &str,
        executable_path: &str,
        now: Instant,
    ) -> TemporaryThawState {
        if self.temporary_thawed.contains_key(&process_id)
            && !self.managed_process_matches_target(controller, process_id, executable_path)
        {
            self.temporary_thawed.remove(&process_id);
            return TemporaryThawState::None;
        }
        let Some(thaw) = self.temporary_thawed.get_mut(&process_id) else {
            return TemporaryThawState::None;
        };

        thaw.process_name = process_name.to_owned();
        thaw.executable_path = executable_path.to_owned();
        if now < thaw.thaw_until {
            TemporaryThawState::Active
        } else {
            TemporaryThawState::Expired
        }
    }

    fn record_manual_freeze_result(&mut self, executable_path: &str, result: Result<(), String>) {
        let outcome = self
            .manual_freeze_outcomes
            .entry(executable_path_key(Path::new(executable_path)))
            .or_default();
        outcome.attempts += 1;
        if let Err(error) = result {
            outcome.failures += 1;
            outcome.first_error.get_or_insert(error);
        }
    }

    fn suspension_lifecycle_state(
        &mut self,
        controller: &mut SuspensionController,
        process_id: u32,
        process: &TargetProcess,
        now: Instant,
        delay: Duration,
        manual_freeze: bool,
    ) -> SuspensionLifecycleState {
        let process_name = process.process_name.as_str();
        let executable_path = process.executable_path.as_str();
        let app_key = executable_path_key(Path::new(executable_path));
        if manual_freeze {
            self.temporary_thawed.remove(&process_id);
            self.tracked.remove(&app_key);
            return SuspensionLifecycleState::ManualFreeze;
        }

        match self.temporary_thaw_state(controller, process_id, process_name, executable_path, now)
        {
            TemporaryThawState::Active => SuspensionLifecycleState::IntentActive,
            TemporaryThawState::Expired => {
                self.tracked.insert(
                    app_key,
                    TrackedApp {
                        background_since: now.checked_sub(delay).unwrap_or(now),
                    },
                );
                SuspensionLifecycleState::ReadyToSuspend
            }
            TemporaryThawState::None => {
                let tracked = self.tracked.entry(app_key).or_insert_with(|| TrackedApp {
                    background_since: now,
                });
                if now.duration_since(tracked.background_since) < delay {
                    SuspensionLifecycleState::BackgroundGrace
                } else {
                    SuspensionLifecycleState::ReadyToSuspend
                }
            }
        }
    }

    fn suspend_process(
        &mut self,
        controller: &mut SuspensionController,
        target: &SuspensionTarget,
        process: SuspendedProcess,
        allow_cross_session_process_control: bool,
    ) -> Result<(), SuspensionError> {
        let process_id = target.process.id;
        let result = controller.freeze(target, allow_cross_session_process_control);
        if result.is_ok() || controller.is_frozen_process(process_id) {
            self.suspended.insert(process_id, process);
            self.temporary_thawed.remove(&process_id);
        }
        result.map(|_outcome: SuspensionFreezeOutcome| ())
    }

    fn snapshot(
        &self,
        enabled: bool,
        unsupported: bool,
        skipped_processes: usize,
        failed_actions: usize,
        status: AppSuspensionStatus,
        last_error: Option<String>,
    ) -> AppSuspensionSnapshot {
        AppSuspensionSnapshot {
            enabled,
            unsupported,
            grace_apps: self.tracked.len(),
            suspended_processes: self.suspended.len(),
            suspended_process_ids: self.suspended.keys().copied().collect(),
            temporary_thawed_processes: self.temporary_thawed.len(),
            network_wake_processes: self
                .temporary_thawed
                .values()
                .filter(|process| process.reason == TemporaryThawReason::NetworkWake)
                .count(),
            audio_wake_processes: self
                .temporary_thawed
                .values()
                .filter(|process| process.reason == TemporaryThawReason::AudioWake)
                .count(),
            background_grace_apps: self.tracked.keys().cloned().collect(),
            suspended_apps: self
                .suspended
                .values()
                .map(|process| process.executable_path.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            temporary_thawed_apps: self
                .temporary_thawed
                .values()
                .map(|process| process.executable_path.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            network_wake_apps: self
                .temporary_thawed
                .values()
                .filter(|process| process.reason == TemporaryThawReason::NetworkWake)
                .map(|process| process.executable_path.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            audio_wake_apps: self
                .temporary_thawed
                .values()
                .filter(|process| process.reason == TemporaryThawReason::AudioWake)
                .map(|process| process.executable_path.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            running_apps: self.running_apps.iter().cloned().collect(),
            status_unknown: false,
            skipped_processes,
            failed_actions,
            auto_excluded_processes: Vec::new(),
            status,
            last_error,
        }
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
                ActionLogFeature::AppSuspension,
                Some(process_id),
                process_name.to_owned(),
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying App Suspension after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }

        true
    }

    fn is_action_suppressed(
        &mut self,
        key: &str,
        action_label: &str,
        action_log: &mut ActionLog,
    ) -> bool {
        let suppression = self.action_failure_suppression.key_suppression(key);
        if !suppression.suppressed {
            return false;
        }

        if suppression.newly_suppressed {
            action_log.record(
                ActionLogFeature::AppSuspension,
                None,
                "",
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying App Suspension {action_label} after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }

        true
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    suspension_control::is_builtin_excluded(process_name)
}

pub fn process_is_suspendable(target: &ProcessActionTarget) -> bool {
    suspension_control::process_is_suspendable(target)
}

pub fn contains_process(list: &[String], executable_path: &str) -> bool {
    list.iter()
        .any(|path| same_executable_path(Path::new(path), Path::new(executable_path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network_snapshot(
        process_name: &str,
        connections: &[(&str, Option<NetworkActivityCounters>)],
    ) -> NetworkConnectionSnapshot {
        BTreeMap::from([(
            process_name.to_owned(),
            connections
                .iter()
                .map(|(connection, activity)| ((*connection).to_owned(), *activity))
                .collect(),
        )])
    }

    fn network_thresholds(
        process_name: &str,
        bytes_in: u64,
        bytes_out: u64,
    ) -> NetworkActivityThresholdsByProcess {
        BTreeMap::from([(
            process_name.to_owned(),
            NetworkActivityThresholds {
                bytes_in,
                bytes_out,
            },
        )])
    }

    fn inert_target(process_id: u32, name: &str, executable_path: &str) -> SuspensionTarget {
        SuspensionTarget::automatic(
            process_id,
            name.to_owned(),
            Path::new(executable_path).to_path_buf(),
            u64::from(process_id) + 1,
            Some(false),
        )
    }

    fn target_process(name: &str, executable_path: &str) -> TargetProcess {
        TargetProcess {
            process_name: name.to_owned(),
            executable_path: executable_path.to_owned(),
            creation_time: 1,
            is_service_account: Some(false),
        }
    }

    fn suspension_rule(executable_path: &str) -> crate::config::AppSuspensionRule {
        crate::config::AppSuspensionRule {
            enabled: true,
            executable_path: executable_path.to_owned(),
            network_wake_enabled: false,
            audio_wake_enabled: false,
            network_download_threshold_bytes: 0,
            network_download_threshold_unit: Default::default(),
            network_upload_threshold_bytes: 0,
            network_upload_threshold_unit: Default::default(),
        }
    }

    fn insert_inert(
        controller: &mut SuspensionController,
        process_id: u32,
        name: &str,
        executable_path: &str,
        frozen: bool,
    ) {
        controller.insert_inert(inert_target(process_id, name, executable_path), frozen);
    }

    #[test]
    fn manual_freeze_result_reports_no_target_and_partial_failure() {
        let mut manager = AppSuspensionManager::default();
        let path = r"C:\Apps\chat.exe";
        manager.manual_freeze_outcomes.insert(
            executable_path_key(Path::new(path)),
            ManualFreezeOutcome::default(),
        );

        assert!(manager
            .manual_freeze_result(path)
            .unwrap_err()
            .starts_with("No eligible running process matched"));

        manager.record_manual_freeze_result(path, Ok(()));
        assert!(manager.manual_freeze_result(path).is_ok());
        manager.record_manual_freeze_result(path, Err("commit failed".to_owned()));
        assert_eq!(
            manager.manual_freeze_result(path).unwrap_err(),
            "1 of 2 App Suspension actions failed: commit failed"
        );
    }

    #[test]
    fn disabled_update_prunes_a_reconciled_compensation_record() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        let path = r"C:\Apps\chat.exe";
        let target = inert_target(7, "chat.exe", path);
        controller.insert_inert_compensation_pending(target);
        manager.suspended.insert(
            7,
            SuspendedProcess {
                process_name: "chat.exe".to_owned(),
                executable_path: path.to_owned(),
                creation_time: 8,
                suspended_since: now,
                manual: true,
            },
        );
        let mut observations = CycleObservations::default();

        let status = manager.update(
            &mut controller,
            &AppSuspensionSettings::default(),
            true,
            false,
            None,
            &[],
            &mut observations,
            &mut log,
        );

        assert_eq!(status.suspended_processes, 0);
        assert!(manager.suspended.is_empty());
        assert!(!controller.contains_process(7));
        assert!(!manager.has_suspended_processes(&controller));
    }

    #[test]
    fn sticky_unsupported_state_does_not_repeat_action_log_entries() {
        let mut manager = AppSuspensionManager {
            job_freeze_unsupported: true,
            ..Default::default()
        };
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);
        let path = r"C:\Apps\chat.exe";
        let settings = AppSuspensionSettings {
            enabled: true,
            suspendable_apps: vec![suspension_rule(path)],
            ..Default::default()
        };

        for _ in 0..2 {
            manager.update(
                &mut controller,
                &settings,
                true,
                false,
                None,
                &[],
                &mut CycleObservations::default(),
                &mut log,
            );
        }

        assert!(log.entries().is_empty());
    }

    #[test]
    fn disabling_automation_preserves_only_manual_suspensions() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        for (process_id, manual) in [(7, true), (8, false)] {
            let name = format!("{process_id}.exe");
            let path = format!("C:/Apps/{process_id}.exe");
            insert_inert(&mut controller, process_id, &name, &path, true);
            manager.suspended.insert(
                process_id,
                SuspendedProcess {
                    process_name: format!("{process_id}.exe"),
                    executable_path: format!("C:/Apps/{process_id}.exe"),
                    creation_time: u64::from(process_id) + 1,
                    suspended_since: now,
                    manual,
                },
            );
        }
        let mut observations = CycleObservations::default();

        let status = manager.update(
            &mut controller,
            &AppSuspensionSettings::default(),
            false,
            false,
            None,
            &[],
            &mut observations,
            &mut log,
        );

        assert_eq!(status.suspended_process_ids, vec![7]);
        assert!(manager.suspended.contains_key(&7));
        assert!(!manager.suspended.contains_key(&8));
        assert!(controller.contains_process(7));
        assert!(!controller.contains_process(8));
    }

    #[test]
    fn enabled_without_rules_preserves_only_manual_suspensions() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        for (process_id, manual) in [(7, true), (8, false)] {
            let name = format!("{process_id}.exe");
            let path = format!("C:/Apps/{process_id}.exe");
            insert_inert(&mut controller, process_id, &name, &path, true);
            manager.suspended.insert(
                process_id,
                SuspendedProcess {
                    process_name: format!("{process_id}.exe"),
                    executable_path: format!("C:/Apps/{process_id}.exe"),
                    creation_time: u64::from(process_id) + 1,
                    suspended_since: now,
                    manual,
                },
            );
        }
        let settings = AppSuspensionSettings {
            enabled: true,
            ..Default::default()
        };
        let mut observations = CycleObservations::default();

        let status = manager.update(
            &mut controller,
            &settings,
            true,
            false,
            None,
            &[],
            &mut observations,
            &mut log,
        );

        assert_eq!(status.suspended_process_ids, vec![7]);
        assert!(manager.suspended.contains_key(&7));
        assert!(!manager.suspended.contains_key(&8));
    }

    #[test]
    fn target_churn_does_not_release_manual_suspension() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        for (process_id, manual) in [(7, true), (8, false)] {
            let name = format!("{process_id}.exe");
            let path = format!("C:/Apps/{process_id}.exe");
            insert_inert(&mut controller, process_id, &name, &path, true);
            manager.suspended.insert(
                process_id,
                SuspendedProcess {
                    process_name: format!("{process_id}.exe"),
                    executable_path: format!("C:/Apps/{process_id}.exe"),
                    creation_time: u64::from(process_id) + 1,
                    suspended_since: now,
                    manual,
                },
            );
        }

        manager.release_non_targets(
            &mut controller,
            &BTreeSet::new(),
            &mut log,
            "test target churn",
        );

        assert!(manager.suspended.contains_key(&7));
        assert!(!manager.suspended.contains_key(&8));
        assert!(controller.contains_process(7));
        assert!(!controller.contains_process(8));
    }

    #[test]
    fn manual_freeze_matching_handles_path_case_and_slashes() {
        let suspendable_apps = vec![r"C:\Apps\chat.exe".to_owned()];

        assert!(contains_process(&suspendable_apps, r"c:/apps/CHAT.exe"));
        assert!(!contains_process(&suspendable_apps, r"C:\Other\chat.exe"));
    }

    #[test]
    fn builtin_exclusions_cover_sensitive_windows_shell_processes() {
        for process_name in [
            "AppActions.exe",
            "ApplicationFrameHost.exe",
            "backgroundTaskHost.exe",
            "CrossDeviceResume.exe",
            "dllhost.exe",
            "explorer.exe",
            "LockApp.exe",
            "RuntimeBroker.exe",
            "SearchApp.exe",
            "SearchHost.exe",
            "ShellHost.exe",
            "svchost.exe",
            "SystemSettings.exe",
            "taskhostw.exe",
            "TextInputHost.exe",
            "unsecapp.exe",
            "WmiPrvSE.exe",
        ] {
            assert!(is_builtin_excluded(process_name), "{process_name}");
        }

        assert!(!is_builtin_excluded("chat.exe"));

        let mut target = ProcessActionTarget {
            id: 42,
            name: "chat.exe".to_owned(),
            executable_path: std::path::PathBuf::from("chat.exe"),
            creation_time: 1,
            session_id: Some(1),
            is_service_account: Some(false),
        };
        assert!(process_is_suspendable(&target));
        target.is_service_account = Some(true);
        assert!(!process_is_suspendable(&target));
        target.is_service_account = None;
        assert!(!process_is_suspendable(&target));
        target.is_service_account = Some(false);
        target.session_id = Some(0);
        assert!(!process_is_suspendable(&target));
    }

    #[test]
    fn foreground_skip_matches_pid_or_exact_executable_path() {
        assert!(should_skip_foreground_process(
            42,
            Path::new(r"C:\Other\helper.exe"),
            42,
            Some(Path::new(r"C:\Apps\app.exe")),
        ));
        assert!(should_skip_foreground_process(
            99,
            Path::new(r"c:/apps/APP.exe"),
            42,
            Some(Path::new(r"C:\Apps\app.exe")),
        ));
        assert!(!should_skip_foreground_process(
            99,
            Path::new(r"C:\Other\app.exe"),
            42,
            Some(Path::new(r"C:\Apps\app.exe")),
        ));
    }

    #[test]
    fn repeated_failures_suppress_future_suspension_attempts_once() {
        let mut manager = AppSuspensionManager::default();
        let mut log = ActionLog::new(8);

        manager
            .failure_suppression
            .record_process_failure(r"C:\Apps\app.exe");
        manager
            .failure_suppression
            .record_process_failure(r"C:\Apps\app.exe");
        assert!(!manager.is_process_suppressed(
            42,
            "app.exe",
            r"C:\Apps\app.exe",
            &mut log,
            &mut BTreeSet::new(),
        ));
        assert!(log.entries().is_empty());

        manager
            .failure_suppression
            .record_process_failure(r"C:\Apps\app.exe");
        let mut auto_excluded_processes = BTreeSet::new();
        assert!(manager.is_process_suppressed(
            42,
            "app.exe",
            r"C:\Apps\app.exe",
            &mut log,
            &mut auto_excluded_processes,
        ));
        assert!(!manager.is_process_suppressed(
            43,
            "app.exe",
            r"C:\Other\app.exe",
            &mut log,
            &mut auto_excluded_processes,
        ));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].process_name, "app.exe");
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
        assert_eq!(
            auto_excluded_processes,
            BTreeSet::from([r"C:\Apps\app.exe".to_owned()])
        );
    }

    #[test]
    fn repeated_action_failures_suppress_future_suspension_detection_once() {
        let mut manager = AppSuspensionManager::default();
        let mut log = ActionLog::new(8);

        manager
            .action_failure_suppression
            .record_key_failure(NETWORK_DETECTION_FAILURE_KEY);
        manager
            .action_failure_suppression
            .record_key_failure(NETWORK_DETECTION_FAILURE_KEY);
        assert!(!manager.is_action_suppressed(
            NETWORK_DETECTION_FAILURE_KEY,
            "network activity detection",
            &mut log,
        ));
        assert!(log.entries().is_empty());

        manager
            .action_failure_suppression
            .record_key_failure(NETWORK_DETECTION_FAILURE_KEY);
        assert!(manager.is_action_suppressed(
            NETWORK_DETECTION_FAILURE_KEY,
            "network activity detection",
            &mut log,
        ));
        assert!(manager.is_action_suppressed(
            NETWORK_DETECTION_FAILURE_KEY,
            "network activity detection",
            &mut log,
        ));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
    }

    #[test]
    fn user_intent_release_supports_targeted_and_shell_fallback() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        insert_inert(&mut controller, 7, "chat.exe", r"C:\Apps\chat.exe", true);
        insert_inert(&mut controller, 8, "mail.exe", r"C:\Mail\mail.exe", true);
        manager.suspended.insert(
            7,
            SuspendedProcess {
                process_name: "chat.exe".to_owned(),
                executable_path: r"C:\Apps\chat.exe".to_owned(),
                creation_time: 8,
                suspended_since: now,
                manual: false,
            },
        );
        manager.suspended.insert(
            8,
            SuspendedProcess {
                process_name: "mail.exe".to_owned(),
                executable_path: r"C:\Mail\mail.exe".to_owned(),
                creation_time: 9,
                suspended_since: now,
                manual: false,
            },
        );

        let status = manager
            .release_window_owner_processes_for_user_intent(
                &mut controller,
                &BTreeSet::from([7]),
                &mut log,
            )
            .unwrap();

        assert_eq!(status.suspended_processes, 1);
        assert_eq!(status.temporary_thawed_processes, 1);
        assert!(!manager.suspended.contains_key(&7));
        assert!(manager.suspended.contains_key(&8));
        assert!(manager.temporary_thawed.contains_key(&7));

        let status = manager
            .release_all_suspended_processes_for_user_intent(&mut controller, &mut log)
            .unwrap();
        assert_eq!(status.suspended_processes, 0);
        assert!(manager.suspended.is_empty());
        assert!(manager.temporary_thawed.contains_key(&8));
    }

    #[test]
    fn user_intent_release_by_path_thaws_only_matching_instances() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        for (process_id, name, path) in [
            (7, "chat.exe", r"C:\Apps\chat.exe"),
            (8, "chat.exe", r"C:\Apps\chat.exe"),
            (9, "mail.exe", r"C:\Mail\mail.exe"),
        ] {
            insert_inert(&mut controller, process_id, name, path, true);
            manager.suspended.insert(
                process_id,
                SuspendedProcess {
                    process_name: name.to_owned(),
                    executable_path: path.to_owned(),
                    creation_time: u64::from(process_id) + 1,
                    suspended_since: now,
                    manual: true,
                },
            );
        }

        let status = manager.release_suspended_path_for_user_intent(
            &mut controller,
            Path::new(r"c:/apps/CHAT.exe"),
            &mut log,
        );

        assert_eq!(status.suspended_process_ids, vec![9]);
        assert_eq!(status.temporary_thawed_processes, 2);
    }

    #[test]
    fn user_intent_release_does_not_extend_existing_temporary_thaw() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        manager.temporary_thawed.insert(
            7,
            TemporaryThaw {
                process_name: "chat.exe".to_owned(),
                executable_path: r"C:\Apps\chat.exe".to_owned(),
                thaw_until: now + Duration::from_secs(5),
                reason: TemporaryThawReason::UserIntent,
            },
        );

        assert!(manager
            .release_window_owner_processes_for_user_intent(
                &mut controller,
                &BTreeSet::from([7]),
                &mut log,
            )
            .is_none());
        assert_eq!(
            manager.temporary_thawed.get(&7).unwrap().thaw_until,
            now + Duration::from_secs(5)
        );
    }

    #[test]
    fn user_intent_release_returns_none_without_matching_window_owner() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);

        assert!(manager
            .release_window_owner_processes_for_user_intent(
                &mut controller,
                &BTreeSet::from([42]),
                &mut log,
            )
            .is_none());
    }

    #[test]
    fn temporary_thaw_state_preserves_path_after_expiration() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let now = Instant::now();
        insert_inert(&mut controller, 7, "chat.exe", r"C:\Apps\chat.exe", false);
        manager.temporary_thawed.insert(
            7,
            TemporaryThaw {
                process_name: "chat.exe".to_owned(),
                executable_path: r"C:\Apps\chat.exe".to_owned(),
                thaw_until: now + Duration::from_secs(5),
                reason: TemporaryThawReason::Fallback,
            },
        );

        assert_eq!(
            manager.temporary_thaw_state(&mut controller, 7, "CHAT.EXE", r"c:/apps/CHAT.exe", now,),
            TemporaryThawState::Active
        );
        assert_eq!(
            manager.temporary_thawed.get(&7).unwrap().process_name,
            "CHAT.EXE"
        );
        assert_eq!(
            manager.temporary_thaw_state(
                &mut controller,
                7,
                "chat.exe",
                r"C:\Apps\chat.exe",
                now + Duration::from_secs(6),
            ),
            TemporaryThawState::Expired
        );
        assert_eq!(
            manager.temporary_thawed.get(&7).unwrap().executable_path,
            r"C:\Apps\chat.exe"
        );
    }

    #[test]
    fn temporary_thaw_state_reports_none_without_entry() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();

        assert_eq!(
            manager.temporary_thaw_state(
                &mut controller,
                99,
                "chat.exe",
                r"C:\Apps\chat.exe",
                Instant::now(),
            ),
            TemporaryThawState::None
        );
    }

    #[test]
    fn suspension_lifecycle_keeps_intent_above_delay_unless_manual_freeze() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let now = Instant::now();
        insert_inert(&mut controller, 7, "chat.exe", r"C:\Apps\chat.exe", false);
        manager.temporary_thawed.insert(
            7,
            TemporaryThaw {
                process_name: "chat.exe".to_owned(),
                executable_path: r"C:\Apps\chat.exe".to_owned(),
                thaw_until: now + Duration::from_secs(5),
                reason: TemporaryThawReason::NetworkWake,
            },
        );

        assert_eq!(
            manager.suspension_lifecycle_state(
                &mut controller,
                7,
                &target_process("chat.exe", r"C:\Apps\chat.exe"),
                now,
                Duration::ZERO,
                false,
            ),
            SuspensionLifecycleState::IntentActive
        );
        assert_eq!(
            manager.suspension_lifecycle_state(
                &mut controller,
                7,
                &target_process("chat.exe", r"C:\Apps\chat.exe"),
                now,
                Duration::ZERO,
                true,
            ),
            SuspensionLifecycleState::ManualFreeze
        );
        assert!(!manager.temporary_thawed.contains_key(&7));
    }

    #[test]
    fn suspension_lifecycle_uses_background_grace_before_ready() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let now = Instant::now();

        assert_eq!(
            manager.suspension_lifecycle_state(
                &mut controller,
                7,
                &target_process("chat.exe", r"C:\Apps\chat.exe"),
                now,
                Duration::from_secs(10),
                false,
            ),
            SuspensionLifecycleState::BackgroundGrace
        );
        manager
            .tracked
            .get_mut(r"C:\Apps\chat.exe")
            .unwrap()
            .background_since = now.checked_sub(Duration::from_secs(11)).unwrap();

        assert_eq!(
            manager.suspension_lifecycle_state(
                &mut controller,
                7,
                &target_process("chat.exe", r"C:\Apps\chat.exe"),
                now,
                Duration::from_secs(10),
                false,
            ),
            SuspensionLifecycleState::ReadyToSuspend
        );
    }

    #[test]
    fn suspension_lifecycle_shares_background_grace_by_executable_path() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let now = Instant::now();

        assert_eq!(
            manager.suspension_lifecycle_state(
                &mut controller,
                7,
                &target_process("chat.exe", r"C:\Apps\chat.exe"),
                now,
                Duration::from_secs(10),
                false,
            ),
            SuspensionLifecycleState::BackgroundGrace
        );
        manager
            .tracked
            .get_mut(r"C:\Apps\chat.exe")
            .unwrap()
            .background_since = now.checked_sub(Duration::from_secs(11)).unwrap();

        assert_eq!(
            manager.suspension_lifecycle_state(
                &mut controller,
                8,
                &target_process("CHAT.EXE", r"C:\Apps\chat.exe"),
                now,
                Duration::from_secs(10),
                false,
            ),
            SuspensionLifecycleState::ReadyToSuspend
        );
    }

    #[test]
    fn suspension_lifecycle_separates_same_named_executables() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let now = Instant::now();

        assert_eq!(
            manager.suspension_lifecycle_state(
                &mut controller,
                7,
                &target_process("chat.exe", r"C:\Apps\chat.exe"),
                now,
                Duration::from_secs(10),
                false,
            ),
            SuspensionLifecycleState::BackgroundGrace
        );
        manager
            .tracked
            .get_mut(r"C:\Apps\chat.exe")
            .unwrap()
            .background_since = now.checked_sub(Duration::from_secs(11)).unwrap();

        assert_eq!(
            manager.suspension_lifecycle_state(
                &mut controller,
                8,
                &target_process("chat.exe", r"C:\Other\chat.exe"),
                now,
                Duration::from_secs(10),
                false,
            ),
            SuspensionLifecycleState::BackgroundGrace
        );
    }

    #[test]
    fn snapshot_reports_paths_owned_by_lifecycle_records() {
        let mut manager = AppSuspensionManager::default();
        let now = Instant::now();
        manager.running_apps.insert(r"C:\Apps\chat.exe".to_owned());
        manager.suspended.insert(
            7,
            SuspendedProcess {
                process_name: "chat.exe".to_owned(),
                executable_path: r"C:\Apps\chat.exe".to_owned(),
                creation_time: 8,
                suspended_since: now,
                manual: false,
            },
        );
        manager.temporary_thawed.insert(
            8,
            TemporaryThaw {
                process_name: "chat.exe".to_owned(),
                executable_path: r"C:\Other\chat.exe".to_owned(),
                thaw_until: now + Duration::from_secs(5),
                reason: TemporaryThawReason::NetworkWake,
            },
        );

        let status = manager.snapshot(true, false, 0, 0, AppSuspensionStatus::Active, None);

        assert_eq!(status.running_apps, vec![r"C:\Apps\chat.exe".to_owned()]);
        assert_eq!(status.suspended_apps, vec![r"C:\Apps\chat.exe".to_owned()]);
        assert_eq!(
            status.temporary_thawed_apps,
            vec![r"C:\Other\chat.exe".to_owned()]
        );
        assert_eq!(
            status.network_wake_apps,
            vec![r"C:\Other\chat.exe".to_owned()]
        );
    }

    #[test]
    fn release_non_targets_closes_thawed_jobs() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        insert_inert(&mut controller, 7, "chat.exe", r"C:\Apps\chat.exe", false);
        manager.temporary_thawed.insert(
            7,
            TemporaryThaw {
                process_name: "chat.exe".to_owned(),
                executable_path: r"C:\Apps\chat.exe".to_owned(),
                thaw_until: now + Duration::from_secs(5),
                reason: TemporaryThawReason::Fallback,
            },
        );

        assert_eq!(
            manager.release_non_targets(&mut controller, &BTreeSet::new(), &mut log, "test"),
            0
        );
        assert!(!controller.contains_process(7));
        assert!(manager.temporary_thawed.is_empty());
    }

    #[test]
    fn release_non_targets_keeps_target_thawed_jobs() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        insert_inert(&mut controller, 7, "chat.exe", r"C:\Apps\chat.exe", false);
        manager.temporary_thawed.insert(
            7,
            TemporaryThaw {
                process_name: "chat.exe".to_owned(),
                executable_path: r"C:\Apps\chat.exe".to_owned(),
                thaw_until: now + Duration::from_secs(5),
                reason: TemporaryThawReason::Fallback,
            },
        );

        assert_eq!(
            manager.release_non_targets(&mut controller, &BTreeSet::from([7]), &mut log, "test",),
            0
        );
        assert!(controller.contains_process(7));
        assert!(manager.temporary_thawed.contains_key(&7));
    }

    #[test]
    fn foreground_unknown_pauses_without_releasing_suspended_processes() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);
        let settings = AppSuspensionSettings {
            enabled: true,
            suspendable_apps: vec![crate::config::AppSuspensionRule {
                enabled: true,
                executable_path: r"C:\Apps\chat.exe".to_owned(),
                network_wake_enabled: false,
                audio_wake_enabled: false,
                network_download_threshold_bytes: 0,
                network_download_threshold_unit: Default::default(),
                network_upload_threshold_bytes: 0,
                network_upload_threshold_unit: Default::default(),
            }],
            ..Default::default()
        };
        let now = Instant::now();
        manager.tracked.insert(
            r"C:\Apps\chat.exe".to_owned(),
            TrackedApp {
                background_since: now,
            },
        );
        insert_inert(&mut controller, 7, "chat.exe", r"C:\Apps\chat.exe", true);
        manager.suspended.insert(
            7,
            SuspendedProcess {
                process_name: "chat.exe".to_owned(),
                executable_path: r"C:\Apps\chat.exe".to_owned(),
                creation_time: 8,
                suspended_since: now,
                manual: false,
            },
        );

        let mut observations = CycleObservations::default();
        let status = manager.update(
            &mut controller,
            &settings,
            true,
            false,
            None,
            &[],
            &mut observations,
            &mut log,
        );

        assert_eq!(status.status, AppSuspensionStatus::ForegroundUnknown);
        assert_eq!(
            status.status.to_string(),
            "Paused: foreground app is unknown."
        );
        assert!(status.status_unknown);
        assert_eq!(status.grace_apps, 0);
        assert_eq!(status.suspended_processes, 1);
        assert_eq!(status.suspended_apps, vec![r"C:\Apps\chat.exe".to_owned()]);
        assert!(manager.tracked.is_empty());
        assert!(manager.suspended.contains_key(&7));
        assert!(controller.contains_process(7));
    }

    #[test]
    fn interactive_release_matches_executable_path_group() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        manager.tracked.insert(
            r"C:\Apps\chat.exe".to_owned(),
            TrackedApp {
                background_since: now,
            },
        );
        manager.suspended.insert(
            7,
            SuspendedProcess {
                process_name: "chat.exe".to_owned(),
                executable_path: r"C:\Apps\chat.exe".to_owned(),
                creation_time: 8,
                suspended_since: now,
                manual: false,
            },
        );
        manager.suspended.insert(
            8,
            SuspendedProcess {
                process_name: "CHAT.EXE".to_owned(),
                executable_path: r"C:\Apps\chat.exe".to_owned(),
                creation_time: 9,
                suspended_since: now,
                manual: false,
            },
        );
        manager.suspended.insert(
            9,
            SuspendedProcess {
                process_name: "mail.exe".to_owned(),
                executable_path: r"C:\Mail\mail.exe".to_owned(),
                creation_time: 10,
                suspended_since: now,
                manual: false,
            },
        );

        let status = manager
            .release_interactive_process(
                &mut controller,
                7,
                Some(Path::new(r"c:/apps/CHAT.exe")),
                &mut log,
            )
            .unwrap();

        assert_eq!(status.grace_apps, 0);
        assert_eq!(status.suspended_processes, 1);
        assert!(!manager.tracked.contains_key(r"C:\Apps\chat.exe"));
        assert!(!manager.suspended.contains_key(&7));
        assert!(!manager.suspended.contains_key(&8));
        assert!(manager.suspended.contains_key(&9));
    }

    #[test]
    fn interactive_release_uses_managed_executable_path_when_lookup_is_unavailable() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        manager.suspended.insert(
            7,
            SuspendedProcess {
                process_name: "browser.exe".to_owned(),
                executable_path: r"C:\Apps\browser.exe".to_owned(),
                creation_time: 8,
                suspended_since: now,
                manual: false,
            },
        );
        manager.suspended.insert(
            8,
            SuspendedProcess {
                process_name: "BROWSER.EXE".to_owned(),
                executable_path: r"C:\Apps\browser.exe".to_owned(),
                creation_time: 9,
                suspended_since: now,
                manual: false,
            },
        );

        let status = manager
            .release_interactive_process(&mut controller, 7, None, &mut log)
            .unwrap();

        assert_eq!(status.suspended_processes, 0);
        assert!(manager.suspended.is_empty());
    }

    #[test]
    fn interactive_release_clears_matching_thawed_jobs() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        insert_inert(&mut controller, 7, "chat.exe", r"C:\Apps\chat.exe", false);
        insert_inert(&mut controller, 8, "CHAT.EXE", r"C:\Apps\chat.exe", false);
        manager.temporary_thawed.insert(
            7,
            TemporaryThaw {
                process_name: "chat.exe".to_owned(),
                executable_path: r"C:\Apps\chat.exe".to_owned(),
                thaw_until: now + Duration::from_secs(5),
                reason: TemporaryThawReason::Fallback,
            },
        );
        manager.temporary_thawed.insert(
            8,
            TemporaryThaw {
                process_name: "CHAT.EXE".to_owned(),
                executable_path: r"C:\Apps\chat.exe".to_owned(),
                thaw_until: now + Duration::from_secs(5),
                reason: TemporaryThawReason::Fallback,
            },
        );
        let status = manager
            .release_interactive_process(
                &mut controller,
                7,
                Some(Path::new(r"C:\Apps\chat.exe")),
                &mut log,
            )
            .unwrap();

        assert_eq!(status.temporary_thawed_processes, 0);
        assert!(!controller.contains_process(7));
        assert!(!manager.temporary_thawed.contains_key(&7));
        assert!(!controller.contains_process(8));
        assert!(!manager.temporary_thawed.contains_key(&8));
    }

    #[test]
    fn interactive_release_returns_none_without_matching_controlled_process() {
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut log = ActionLog::new(8);

        assert!(manager
            .release_interactive_process(
                &mut controller,
                42,
                Some(Path::new(r"C:\Apps\chat.exe")),
                &mut log,
            )
            .is_none());
    }

    #[test]
    fn network_wake_duration_requires_toggle_and_positive_duration() {
        let mut settings = AppSuspensionSettings::default();

        assert_eq!(network_wake_duration(&settings), None);

        settings.network_wake_enabled = true;
        settings.network_wake_duration_seconds = 30;
        assert_eq!(
            network_wake_duration(&settings),
            Some(Duration::from_secs(30))
        );

        settings.network_wake_duration_seconds = u64::MAX;
        assert_eq!(
            network_wake_duration(&settings),
            Some(Duration::from_secs(MAX_SUSPENSION_DURATION_SECONDS))
        );

        settings.network_wake_duration_seconds = 0;
        assert_eq!(network_wake_duration(&settings), None);
    }

    #[test]
    fn audio_wake_duration_requires_toggle_and_positive_duration() {
        let mut settings = AppSuspensionSettings::default();

        assert_eq!(audio_wake_duration(&settings), None);

        settings.audio_wake_enabled = true;
        settings.audio_wake_duration_seconds = 10;
        assert_eq!(
            audio_wake_duration(&settings),
            Some(Duration::from_secs(10))
        );

        settings.audio_wake_duration_seconds = 0;
        assert_eq!(audio_wake_duration(&settings), None);
    }

    #[test]
    fn network_process_names_with_activity_ignores_steady_sockets() {
        let previous = network_snapshot("chrome.exe", &[("tcp4:1:2:3:4", None)]);
        let current = previous.clone();
        let thresholds = network_thresholds("chrome.exe", 1, 0);

        let names = network_process_names_with_activity(&previous, &current, &thresholds);

        assert!(names.is_empty());
    }

    #[test]
    fn network_process_names_with_activity_ignores_socket_presence_without_payload() {
        let previous = network_snapshot("chrome.exe", &[("tcp4:1:2:3:4", None)]);
        let current = network_snapshot(
            "chrome.exe",
            &[("tcp4:1:2:3:4", None), ("tcp4:1:6:7:8", None)],
        );
        let thresholds = network_thresholds("chrome.exe", 1, 0);

        let names = network_process_names_with_activity(&previous, &current, &thresholds);

        assert!(names.is_empty());
    }

    #[test]
    fn network_process_names_with_activity_uses_first_seen_process_as_baseline() {
        let previous = BTreeMap::new();
        let current = network_snapshot("chrome.exe", &[("tcp4:1:2:3:4", None)]);
        let thresholds = network_thresholds("chrome.exe", 1, 0);

        let names = network_process_names_with_activity(&previous, &current, &thresholds);

        assert!(names.is_empty());
    }

    #[test]
    fn network_process_names_with_activity_ignores_first_socket_after_baseline() {
        let previous = network_snapshot("chrome.exe", &[]);
        let current = network_snapshot("chrome.exe", &[("tcp4:1:2:3:4", None)]);
        let thresholds = network_thresholds("chrome.exe", 1, 0);

        let names = network_process_names_with_activity(&previous, &current, &thresholds);

        assert!(names.is_empty());
    }

    #[test]
    fn network_process_names_with_activity_detects_tcp_byte_counter_increase() {
        let previous = network_snapshot(
            "chrome.exe",
            &[(
                "tcp4:1:2:3:4",
                Some(NetworkActivityCounters {
                    bytes_in: 10,
                    bytes_out: 5,
                }),
            )],
        );
        let current = network_snapshot(
            "chrome.exe",
            &[(
                "tcp4:1:2:3:4",
                Some(NetworkActivityCounters {
                    bytes_in: 11,
                    bytes_out: 5,
                }),
            )],
        );
        let thresholds = network_thresholds("chrome.exe", 1, 0);

        let names = network_process_names_with_activity(&previous, &current, &thresholds);

        assert_eq!(names, BTreeSet::from(["chrome.exe".to_owned()]));
    }

    #[test]
    fn network_process_names_with_activity_respects_download_threshold() {
        let previous = network_snapshot(
            "chrome.exe",
            &[(
                "tcp4:1:2:3:4",
                Some(NetworkActivityCounters {
                    bytes_in: 10,
                    bytes_out: 5,
                }),
            )],
        );
        let current = network_snapshot(
            "chrome.exe",
            &[(
                "tcp4:1:2:3:4",
                Some(NetworkActivityCounters {
                    bytes_in: 14,
                    bytes_out: 5,
                }),
            )],
        );
        let thresholds = network_thresholds("chrome.exe", 5, 0);

        let names = network_process_names_with_activity(&previous, &current, &thresholds);

        assert!(names.is_empty());
    }

    #[test]
    fn network_process_names_with_activity_ignores_outbound_only_counter_increase() {
        let previous = network_snapshot(
            "chrome.exe",
            &[(
                "tcp4:1:2:3:4",
                Some(NetworkActivityCounters {
                    bytes_in: 10,
                    bytes_out: 5,
                }),
            )],
        );
        let current = network_snapshot(
            "chrome.exe",
            &[(
                "tcp4:1:2:3:4",
                Some(NetworkActivityCounters {
                    bytes_in: 10,
                    bytes_out: 6,
                }),
            )],
        );
        let thresholds = network_thresholds("chrome.exe", 1, 0);

        let names = network_process_names_with_activity(&previous, &current, &thresholds);

        assert!(names.is_empty());
    }

    #[test]
    fn network_process_names_with_activity_detects_upload_when_threshold_enabled() {
        let previous = network_snapshot(
            "chrome.exe",
            &[(
                "tcp4:1:2:3:4",
                Some(NetworkActivityCounters {
                    bytes_in: 10,
                    bytes_out: 5,
                }),
            )],
        );
        let current = network_snapshot(
            "chrome.exe",
            &[(
                "tcp4:1:2:3:4",
                Some(NetworkActivityCounters {
                    bytes_in: 10,
                    bytes_out: 9,
                }),
            )],
        );
        let thresholds = network_thresholds("chrome.exe", 0, 4);

        let names = network_process_names_with_activity(&previous, &current, &thresholds);

        assert_eq!(names, BTreeSet::from(["chrome.exe".to_owned()]));
    }

    #[test]
    fn network_process_names_with_activity_treats_zero_thresholds_as_any_activity() {
        let previous = network_snapshot(
            "chrome.exe",
            &[(
                "tcp4:1:2:3:4",
                Some(NetworkActivityCounters {
                    bytes_in: 10,
                    bytes_out: 5,
                }),
            )],
        );
        let current = network_snapshot(
            "chrome.exe",
            &[(
                "tcp4:1:2:3:4",
                Some(NetworkActivityCounters {
                    bytes_in: 10,
                    bytes_out: 6,
                }),
            )],
        );
        let thresholds = network_thresholds("chrome.exe", 0, 0);

        let names = network_process_names_with_activity(&previous, &current, &thresholds);

        assert_eq!(names, BTreeSet::from(["chrome.exe".to_owned()]));
    }

    #[test]
    fn eligible_network_wake_names_require_network_wake_target() {
        let network_names = BTreeSet::from([
            "chat.exe".to_owned(),
            "mail.exe".to_owned(),
            "browser.exe".to_owned(),
        ]);
        let target_names = BTreeSet::from(["chat.exe".to_owned(), "mail.exe".to_owned()]);

        let names = eligible_network_wake_names(&network_names, &target_names);

        assert_eq!(
            names,
            BTreeSet::from(["chat.exe".to_owned(), "mail.exe".to_owned()])
        );
    }

    #[test]
    fn tcp_connection_key_ignores_state_transitions_and_listeners() {
        let established = MIB_TCPROW_OWNER_PID {
            dwState: TCP_STATE_ESTABLISHED,
            dwLocalAddr: 1,
            dwLocalPort: 2,
            dwRemoteAddr: 3,
            dwRemotePort: 4,
            dwOwningPid: 42,
        };
        let syn_sent = MIB_TCPROW_OWNER_PID {
            dwState: TCP_STATE_SYN_SENT,
            ..established
        };
        let listener = MIB_TCPROW_OWNER_PID {
            dwState: 2,
            ..established
        };

        assert_eq!(
            tcp4_connection_key(&established),
            Some("tcp4:1:2:3:4".to_owned())
        );
        assert_eq!(
            tcp4_connection_key(&syn_sent),
            tcp4_connection_key(&established)
        );
        assert_eq!(tcp4_connection_key(&listener), None);
    }

    #[test]
    fn network_wake_window_extends_until_quiet_or_cycle_cap() {
        let mut manager = AppSuspensionManager::default();
        let settings = AppSuspensionSettings {
            network_wake_enabled: true,
            network_wake_duration_seconds: 10,
            ..Default::default()
        };
        let now = Instant::now();
        let names = BTreeSet::from(["chrome.exe".to_owned()]);

        manager.extend_network_wake_windows(&settings, &names, now);
        let first_window = manager.network_wake_windows["chrome.exe"];
        manager.extend_network_wake_windows(&settings, &names, now + Duration::from_secs(5));
        let second_window = manager.network_wake_windows["chrome.exe"];

        assert_eq!(first_window.wake_until, now + Duration::from_secs(10));
        assert_eq!(second_window.wake_until, now + Duration::from_secs(15));
        assert_eq!(
            manager.active_network_wake_names(now + Duration::from_secs(14)),
            names
        );

        manager.extend_network_wake_windows(&settings, &names, now + Duration::from_secs(18));
        let capped_window = manager.network_wake_windows["chrome.exe"];
        assert_eq!(capped_window.wake_until, now + Duration::from_secs(20));

        manager.extend_network_wake_windows(&settings, &names, now + Duration::from_secs(21));
        let suppressed_window = manager.network_wake_windows["chrome.exe"];
        assert_eq!(suppressed_window.wake_until, now + Duration::from_secs(20));
        assert!(manager
            .active_network_wake_names(now + Duration::from_secs(21))
            .is_empty());

        manager.prune_network_wake_windows(&names, now + Duration::from_secs(29));
        assert!(manager.network_wake_windows.contains_key("chrome.exe"));

        manager.prune_network_wake_windows(&names, now + Duration::from_secs(30));
        assert!(manager.network_wake_windows.is_empty());
    }

    #[test]
    fn audio_wake_window_extends_until_quiet() {
        let mut manager = AppSuspensionManager::default();
        let settings = AppSuspensionSettings {
            audio_wake_enabled: true,
            audio_wake_duration_seconds: 10,
            ..Default::default()
        };
        let now = Instant::now();
        let names = BTreeSet::from(["music.exe".to_owned()]);

        manager.extend_audio_wake_windows(&settings, &names, now);
        assert_eq!(
            manager.active_audio_wake_names(now + Duration::from_secs(9)),
            names
        );

        manager.extend_audio_wake_windows(&settings, &names, now + Duration::from_secs(8));
        assert_eq!(
            manager.active_audio_wake_names(now + Duration::from_secs(17)),
            names
        );

        manager.prune_audio_wake_windows(&names, now + Duration::from_secs(18));
        assert!(manager.audio_wake_windows.is_empty());
    }

    #[test]
    fn table_rows_reads_owner_pid_rows() {
        let rows = [
            MIB_UDPROW_OWNER_PID {
                dwLocalAddr: 1,
                dwLocalPort: 2,
                dwOwningPid: 42,
            },
            MIB_UDPROW_OWNER_PID {
                dwLocalAddr: 3,
                dwLocalPort: 4,
                dwOwningPid: 99,
            },
        ];
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&(rows.len() as u32).to_ne_bytes());
        for row in rows {
            // SAFETY: row is a fully initialized plain Win32 record and the slice is limited to
            // its exact in-memory size for immediate copying.
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    &row as *const MIB_UDPROW_OWNER_PID as *const u8,
                    mem::size_of::<MIB_UDPROW_OWNER_PID>(),
                )
            };
            buffer.extend_from_slice(bytes);
        }

        let parsed = table_rows::<MIB_UDPROW_OWNER_PID>(&buffer);

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].dwOwningPid, 42);
        assert_eq!(parsed[1].dwOwningPid, 99);
    }

    #[test]
    fn table_rows_rejects_overflowing_row_count() {
        let buffer = usize::MAX.to_ne_bytes();

        assert!(table_rows::<MIB_UDPROW_OWNER_PID>(&buffer).is_empty());
    }

    #[test]
    fn built_in_exclusions_include_system_processes() {
        assert!(is_builtin_excluded("csrss.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("browser.exe"));
        assert!(!is_builtin_excluded("ms-teams.exe"));
    }
}
