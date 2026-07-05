use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    mem, ptr,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER,
        ERROR_NOT_SUPPORTED, FILETIME, HANDLE, NO_ERROR, WAIT_TIMEOUT,
    },
    NetworkManagement::IpHelper::{
        GetExtendedTcpTable, GetExtendedUdpTable, GetPerTcp6ConnectionEStats,
        GetPerTcpConnectionEStats, SetPerTcp6ConnectionEStats, SetPerTcpConnectionEStats,
        TCP_ESTATS_DATA_ROD_v0, TCP_ESTATS_DATA_RW_v0, TcpConnectionEstatsData, MIB_TCP6ROW,
        MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_LH, MIB_TCPROW_LH_0, MIB_TCPROW_OWNER_PID,
        MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_CONNECTIONS,
        UDP_TABLE_OWNER_PID,
    },
    Networking::WinSock::{AF_INET, AF_INET6, IN6_ADDR, IN6_ADDR_0},
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, SetInformationJobObject,
        },
        Threading::{
            GetCurrentProcessId, GetProcessTimes, OpenProcess, WaitForSingleObject,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_SYNCHRONIZE,
            PROCESS_TERMINATE,
        },
    },
};

use crate::{
    audio_activity::active_audio_process_ids,
    win_util::{filetime_to_u64, last_error, WinHandle},
};

use crate::config::AppSuspensionSettings;
use crate::foreground::{
    contains_process_name, is_process_exited_message, list_processes, process_name_key,
    process_session_id, unique_app_names, EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
};
use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
};

const BUILT_IN_EXCLUSIONS: &[&str] = EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS;
const NETWORK_DETECTION_FAILURE_KEY: &str = "network-detection";
const AUDIO_DETECTION_FAILURE_KEY: &str = "audio-detection";
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSuspensionSnapshot {
    pub enabled: bool,
    pub unsupported: bool,
    pub grace_apps: usize,
    pub suspended_processes: usize,
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
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct AppSuspensionManager {
    tracked: BTreeMap<String, TrackedApp>,
    suspended: BTreeMap<u32, SuspendedProcess>,
    freezers: BTreeMap<u32, ProcessFreezer>,
    temporary_thawed: BTreeMap<u32, TemporaryThaw>,
    failure_suppression: ExecutionFailureTracker,
    action_failure_suppression: ExecutionFailureTracker,
    network_snapshot: NetworkConnectionSnapshot,
    network_wake_windows: BTreeMap<String, NetworkWakeWindow>,
    audio_wake_windows: BTreeMap<String, AudioWakeWindow>,
    running_apps: BTreeSet<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NetworkActivityCounters {
    bytes_in: u64,
    bytes_out: u64,
}

impl NetworkActivityCounters {
    fn exceeds_threshold_since(
        self,
        previous: Self,
        thresholds: NetworkActivityThresholds,
    ) -> bool {
        let bytes_in = self.bytes_in.saturating_sub(previous.bytes_in);
        let bytes_out = self.bytes_out.saturating_sub(previous.bytes_out);
        if thresholds.bytes_in == 0 && thresholds.bytes_out == 0 {
            return bytes_in > 0 || bytes_out > 0;
        }
        (thresholds.bytes_in > 0 && bytes_in >= thresholds.bytes_in)
            || (thresholds.bytes_out > 0 && bytes_out >= thresholds.bytes_out)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NetworkActivityThresholds {
    bytes_in: u64,
    bytes_out: u64,
}

struct TrackedApp {
    process_name: String,
    background_since: Instant,
}

struct SuspendedProcess {
    process_name: String,
    suspended_since: Instant,
}

struct TemporaryThaw {
    process_name: String,
    thaw_until: Instant,
    reason: TemporaryThawReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TemporaryThawReason {
    Fallback,
    NetworkWake,
    AudioWake,
    UserIntent,
}

const USER_INTENT_THAW_SECONDS: u64 = 10;

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
    pub fn has_suspended_processes(&self) -> bool {
        !self.suspended.is_empty()
    }

    pub fn release_interactive_process(
        &mut self,
        process_id: u32,
        process_name: Option<&str>,
        action_log: &mut ActionLog,
    ) -> Option<AppSuspensionSnapshot> {
        let process_ids = self.interactive_process_ids(process_id, process_name);
        if process_ids.is_empty() {
            return None;
        }

        let process_ids = process_ids.into_iter().collect::<Vec<_>>();
        let failed_actions = self.release_foreground_processes(
            &process_ids,
            action_log,
            "released because the app became interactive",
        );
        Some(self.snapshot(
            true,
            self.job_freeze_unsupported,
            0,
            failed_actions,
            "App Suspension active.".to_owned(),
            None,
        ))
    }

    pub fn update(
        &mut self,
        settings: &AppSuspensionSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        manual_freeze_processes: &[String],
        action_log: &mut ActionLog,
    ) -> AppSuspensionSnapshot {
        let now = Instant::now();

        if !automation_enabled {
            let failed = self.clear_all(action_log, "automation disabled");
            self.failure_suppression.clear();
            self.action_failure_suppression.clear();
            return AppSuspensionSnapshot {
                enabled: false,
                failed_actions: failed,
                message: "Automation disabled.".to_owned(),
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failed = self.clear_all(action_log, "App Suspension disabled");
            self.failure_suppression.clear();
            self.action_failure_suppression.clear();
            return AppSuspensionSnapshot {
                enabled: false,
                failed_actions: failed,
                message: "App Suspension disabled.".to_owned(),
                ..Default::default()
            };
        }

        let mut failed_actions = 0;
        if self.job_freeze_unsupported {
            action_log.record(
                ActionLogFeature::AppSuspension,
                None,
                "",
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                "Skipped because Windows Job Object freeze is unsupported.",
            );
            failed_actions += self.clear_all(action_log, "Job Object freeze unsupported");
            return AppSuspensionSnapshot {
                enabled: true,
                unsupported: true,
                failed_actions,
                message: "App Suspension unavailable: Windows Job Object freeze is not supported on this system."
                    .to_owned(),
                ..Default::default()
            };
        }

        let Some(foreground_process_id) = foreground_process_id else {
            return self.pause_without_clearing(
                "Paused: foreground app is unknown.".to_owned(),
                failed_actions,
                None,
            );
        };

        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            return self.pause_without_clearing(
                "Paused: current Windows session is unknown.".to_owned(),
                failed_actions,
                None,
            );
        };

        let processes = match list_processes() {
            Ok(processes) => processes,
            Err(err) => {
                failed_actions += 1;
                return self.pause_without_clearing(err.clone(), failed_actions, Some(err));
            }
        };

        let foreground_process_name = processes
            .iter()
            .find(|process| process.id == foreground_process_id)
            .map(|process| process.name.clone());
        let delay = Duration::from_secs(settings.background_delay_seconds);
        let mut target_processes = BTreeMap::new();
        let mut running_apps = BTreeSet::new();

        for process in processes {
            if process.id == 0
                || process.id == current_process_id
                || is_builtin_excluded(&process.name)
                || !settings.suspendable_app_enabled_for(&process.name)
            {
                continue;
            }

            if process_session_id(process.id) != Some(current_session_id) {
                continue;
            }

            running_apps.insert(process_name_key(&process.name));
            if should_skip_foreground_process(
                process.id,
                &process.name,
                foreground_process_id,
                foreground_process_name.as_deref(),
            ) {
                continue;
            }

            target_processes.insert(process.id, process.name);
        }
        self.running_apps = running_apps;

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let active_target_names = target_processes
            .values()
            .map(|name| process_name_key(name))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);
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
            &target_ids,
            action_log,
            "process no longer matches an App Suspension rule",
        );
        self.tracked
            .retain(|process_name, _process| active_target_names.contains(process_name));
        self.temporary_thawed
            .retain(|process_id, _process| target_ids.contains(process_id));
        let network_target_processes = target_processes
            .iter()
            .filter(|(_process_id, process_name)| settings.network_wake_enabled_for(process_name))
            .map(|(process_id, process_name)| (*process_id, process_name.clone()))
            .collect::<BTreeMap<_, _>>();
        let network_thresholds = network_activity_thresholds(settings, &network_target_processes);
        let network_target_process_names = network_target_processes
            .values()
            .map(|process_name| process_name_key(process_name))
            .collect::<BTreeSet<_>>();
        let audio_target_processes = target_processes
            .iter()
            .filter(|(_process_id, process_name)| settings.audio_wake_enabled_for(process_name))
            .map(|(process_id, process_name)| (*process_id, process_name.clone()))
            .collect::<BTreeMap<_, _>>();
        let audio_target_process_names = audio_target_processes
            .values()
            .map(|process_name| process_name_key(process_name))
            .collect::<BTreeSet<_>>();
        let manual_freeze_apps = manual_freeze_app_names(manual_freeze_processes);
        for process_name in &manual_freeze_apps {
            self.network_wake_windows.remove(process_name);
            self.audio_wake_windows.remove(process_name);
        }
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
            && network_wake_scan_needed(&network_target_process_names)
            && !self.is_action_suppressed(
                NETWORK_DETECTION_FAILURE_KEY,
                "network activity detection",
                action_log,
            ) {
            match network_connection_snapshot(&network_target_processes) {
                Ok(snapshot) => {
                    self.clear_action_failure(NETWORK_DETECTION_FAILURE_KEY);
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
                    self.record_action_failure(NETWORK_DETECTION_FAILURE_KEY);
                    action_log.record(
                        ActionLogFeature::AppSuspension,
                        None,
                        "",
                        ActionLogAction::Fail,
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
                    self.clear_action_failure(AUDIO_DETECTION_FAILURE_KEY);
                    self.extend_audio_wake_windows(settings, &audio_event_names, now);
                }
                Err(err) => {
                    failed_actions += 1;
                    self.record_action_failure(AUDIO_DETECTION_FAILURE_KEY);
                    action_log.record(
                        ActionLogFeature::AppSuspension,
                        None,
                        "",
                        ActionLogAction::Fail,
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
        failed_actions +=
            self.apply_network_wake(&target_processes, &network_wake_names, now, action_log);
        let audio_wake_names = self.active_audio_wake_names(now);
        failed_actions +=
            self.apply_audio_wake(&target_processes, &audio_wake_names, now, action_log);
        self.network_snapshot = network_snapshot;
        failed_actions += self.release_for_temporary_thaw(settings, &target_ids, now, action_log);

        let mut auto_excluded_processes = BTreeSet::new();
        let mut suspended_app_names = BTreeSet::new();
        for (process_id, process_name) in target_processes {
            if self.suspended.contains_key(&process_id) {
                continue;
            }

            if self.is_process_suppressed(
                process_id,
                &process_name,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }

            let manual_freeze = manual_freeze_apps.contains(&process_name_key(&process_name));
            let lifecycle = self.suspension_lifecycle_state(
                process_id,
                &process_name,
                now,
                delay,
                manual_freeze,
            );
            if lifecycle.is_manual_freeze() {
                action_log.record(
                    ActionLogFeature::AppSuspension,
                    Some(process_id),
                    process_name.clone(),
                    ActionLogAction::Apply,
                    ActionLogResult::Applied,
                    "Manual freeze requested.",
                );
            }

            if !lifecycle.should_suspend() {
                continue;
            }

            match self.apply_suspend_action(process_id, process_name.clone(), now) {
                Ok(()) => {
                    self.clear_process_failure(&process_name);
                    action_log.record(
                        ActionLogFeature::AppSuspension,
                        Some(process_id),
                        process_name.clone(),
                        ActionLogAction::Apply,
                        ActionLogResult::Applied,
                        if lifecycle.is_manual_freeze() {
                            "Manually froze background process."
                        } else {
                            "Froze background process after delay."
                        },
                    );
                    suspended_app_names.insert(process_name_key(&process_name));
                }
                Err(SuspensionError::ProcessExited) => {
                    skipped_processes += 1;
                }
                Err(SuspensionError::AccessDenied | SuspensionError::NotSupported) => {
                    skipped_processes += 1;
                    action_log.record(
                        ActionLogFeature::AppSuspension,
                        Some(process_id),
                        process_name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because the process cannot be frozen.",
                    );
                }
                Err(SuspensionError::Unsupported) => {
                    skipped_processes += 1;
                    unsupported = true;
                    self.job_freeze_unsupported = true;
                    action_log.record(
                        ActionLogFeature::AppSuspension,
                        Some(process_id),
                        process_name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because Windows Job Object freeze is unsupported.",
                    );
                    failed_actions += self.clear_all(action_log, "Job Object freeze unsupported");
                    break;
                }
                Err(SuspensionError::Failed(err)) => {
                    if is_process_exited_message(&err) {
                        skipped_processes += 1;
                        continue;
                    }
                    failed_actions += 1;
                    self.record_process_failure(&process_name);
                    action_log.record(
                        ActionLogFeature::AppSuspension,
                        Some(process_id),
                        process_name,
                        ActionLogAction::Fail,
                        ActionLogResult::Failed,
                        err.clone(),
                    );
                    if last_error.is_none() {
                        last_error = Some(err);
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
                "App Suspension unavailable: Windows Job Object freeze is not supported on this system."
                    .to_owned()
            } else {
                "App Suspension active.".to_owned()
            },
            last_error,
        );
        snapshot.auto_excluded_processes = auto_excluded_processes.into_iter().collect();
        snapshot
    }

    fn release_non_targets(
        &mut self,
        target_ids: &BTreeSet<u32>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> usize {
        let process_ids = self
            .managed_process_ids()
            .into_iter()
            .filter(|process_id| !target_ids.contains(process_id))
            .collect::<Vec<_>>();

        self.release_processes(&process_ids, action_log, reason)
    }

    fn clear_all(&mut self, action_log: &mut ActionLog, reason: &str) -> usize {
        self.tracked.clear();
        self.network_snapshot.clear();
        self.network_wake_windows.clear();
        self.audio_wake_windows.clear();
        self.running_apps.clear();
        let process_ids = self.managed_process_ids().into_iter().collect::<Vec<_>>();
        let failed = self.release_processes(&process_ids, action_log, reason);
        self.temporary_thawed.clear();
        failed
    }

    fn pause_without_clearing(
        &mut self,
        message: String,
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
            message,
            last_error,
        );
        snapshot.status_unknown = true;
        snapshot
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        action_log: &mut ActionLog,
        reason: &str,
    ) -> usize {
        let mut failed = 0;
        for process_id in process_ids {
            let suspended_name = self
                .suspended
                .get(process_id)
                .map(|process| process.process_name.clone());
            if let Some(process_name) = suspended_name {
                match self.thaw_process(*process_id) {
                    Ok(()) => {
                        self.suspended.remove(process_id);
                        action_log.record(
                            ActionLogFeature::AppSuspension,
                            Some(*process_id),
                            process_name,
                            ActionLogAction::Restore,
                            ActionLogResult::Restored,
                            reason.to_owned(),
                        );
                    }
                    Err(SuspensionError::ProcessExited) => {
                        self.suspended.remove(process_id);
                    }
                    Err(err) => {
                        failed += 1;
                        action_log.record(
                            ActionLogFeature::AppSuspension,
                            Some(*process_id),
                            process_name,
                            ActionLogAction::Fail,
                            ActionLogResult::Failed,
                            suspension_error_message(err),
                        );
                    }
                }
            }
            self.temporary_thawed.remove(process_id);
            self.freezers.remove(process_id);
        }
        failed
    }

    fn forget_process_state(&mut self, process_id: u32) {
        let process_name = self.controlled_process_name(process_id).map(str::to_owned);
        if let Some(process_name) = process_name {
            self.tracked.remove(&process_name_key(&process_name));
        }
        self.suspended.remove(&process_id);
        self.temporary_thawed.remove(&process_id);
        self.freezers.remove(&process_id);
    }

    fn release_foreground_processes(
        &mut self,
        process_ids: &[u32],
        action_log: &mut ActionLog,
        reason: &str,
    ) -> usize {
        let mut failed = 0;
        for process_id in process_ids {
            let process_name = self.controlled_process_name(*process_id).map(str::to_owned);
            if let Some(process_name) = process_name.clone() {
                if self.suspended.contains_key(process_id) {
                    match self.thaw_process(*process_id) {
                        Ok(()) => {
                            action_log.record(
                                ActionLogFeature::AppSuspension,
                                Some(*process_id),
                                process_name,
                                ActionLogAction::Restore,
                                ActionLogResult::Restored,
                                reason.to_owned(),
                            );
                        }
                        Err(SuspensionError::ProcessExited) => {
                            self.forget_process_state(*process_id);
                            continue;
                        }
                        Err(err) => {
                            failed += 1;
                            action_log.record(
                                ActionLogFeature::AppSuspension,
                                Some(*process_id),
                                process_name,
                                ActionLogAction::Fail,
                                ActionLogResult::Failed,
                                suspension_error_message(err),
                            );
                            continue;
                        }
                    }
                }
            }

            if let Some(process_name) = &process_name {
                self.tracked.remove(&process_name_key(process_name));
            }
            self.suspended.remove(process_id);
            self.temporary_thawed.remove(process_id);
            self.freezers.remove(process_id);
        }

        failed
    }

    pub fn release_window_owner_processes_for_user_intent(
        &mut self,
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

        let failed_actions =
            self.thaw_processes_for_user_intent(&process_ids, Instant::now(), action_log);
        Some(self.snapshot(
            true,
            self.job_freeze_unsupported,
            0,
            failed_actions,
            "App Suspension active.".to_owned(),
            None,
        ))
    }

    fn thaw_processes_for_user_intent(
        &mut self,
        process_ids: &[u32],
        now: Instant,
        action_log: &mut ActionLog,
    ) -> usize {
        let mut failed = 0;
        for process_id in process_ids {
            let process_name = self.controlled_process_name(*process_id).map(str::to_owned);
            if let Some(process_name) = process_name.clone() {
                if self.suspended.contains_key(process_id) {
                    match self.thaw_process(*process_id) {
                        Ok(()) => {
                            action_log.record(
                                ActionLogFeature::AppSuspension,
                                Some(*process_id),
                                process_name.clone(),
                                ActionLogAction::Restore,
                                ActionLogResult::Restored,
                                "Thawed because the user interacted with the window.",
                            );
                        }
                        Err(SuspensionError::ProcessExited) => {
                            self.forget_process_state(*process_id);
                            continue;
                        }
                        Err(err) => {
                            failed += 1;
                            action_log.record(
                                ActionLogFeature::AppSuspension,
                                Some(*process_id),
                                process_name,
                                ActionLogAction::Fail,
                                ActionLogResult::Failed,
                                suspension_error_message(err),
                            );
                            continue;
                        }
                    }
                }
            }

            if let Some(process_name) = &process_name {
                self.tracked.remove(&process_name_key(process_name));
            }
            self.suspended.remove(process_id);
            if let Some(process_name) = process_name {
                self.set_temporary_thaw(
                    *process_id,
                    process_name,
                    now + Duration::from_secs(USER_INTENT_THAW_SECONDS),
                    TemporaryThawReason::UserIntent,
                );
            } else {
                self.temporary_thawed.remove(process_id);
                self.freezers.remove(process_id);
            }
        }

        failed
    }

    fn managed_process_ids(&self) -> BTreeSet<u32> {
        self.suspended
            .keys()
            .chain(self.freezers.keys())
            .copied()
            .collect()
    }

    fn interactive_process_ids(
        &self,
        process_id: u32,
        process_name: Option<&str>,
    ) -> BTreeSet<u32> {
        let mut process_ids = BTreeSet::new();
        if self.is_controlled_process_id(process_id) {
            process_ids.insert(process_id);
        }

        let process_name = process_name.map(process_name_key).or_else(|| {
            self.controlled_process_name(process_id)
                .map(process_name_key)
        });
        let Some(process_name) = process_name else {
            return process_ids;
        };

        process_ids.extend(self.controlled_process_ids_by_name(&process_name));
        process_ids
    }

    fn is_controlled_process_id(&self, process_id: u32) -> bool {
        self.suspended.contains_key(&process_id)
            || self.temporary_thawed.contains_key(&process_id)
            || self.freezers.contains_key(&process_id)
    }

    fn controlled_process_name(&self, process_id: u32) -> Option<&str> {
        self.suspended
            .get(&process_id)
            .map(|process| process.process_name.as_str())
            .or_else(|| {
                self.temporary_thawed
                    .get(&process_id)
                    .map(|process| process.process_name.as_str())
            })
    }

    fn controlled_process_ids_by_name(&self, process_name: &str) -> BTreeSet<u32> {
        self.suspended
            .iter()
            .filter(|(_process_id, process)| {
                process_name_key(&process.process_name) == process_name
            })
            .map(|(process_id, _process)| *process_id)
            .chain(
                self.temporary_thawed
                    .iter()
                    .filter(|(_process_id, process)| {
                        process_name_key(&process.process_name) == process_name
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
        let duration = Duration::from_secs(settings.temporary_thaw_duration_seconds);
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
                match self.thaw_process(process_id) {
                    Ok(()) => {
                        self.suspended.remove(&process_id);
                        action_log.record(
                            ActionLogFeature::AppSuspension,
                            Some(process_id),
                            process_name.clone(),
                            ActionLogAction::Restore,
                            ActionLogResult::Restored,
                            "Temporary thaw interval elapsed.",
                        );
                        self.temporary_thawed.insert(
                            process_id,
                            TemporaryThaw {
                                process_name,
                                thaw_until: now + duration,
                                reason: TemporaryThawReason::Fallback,
                            },
                        );
                    }
                    Err(SuspensionError::ProcessExited) => {
                        self.forget_process_state(process_id);
                    }
                    Err(_) => {
                        failed += 1;
                    }
                }
            }
        }

        failed
    }

    fn apply_network_wake(
        &mut self,
        target_processes: &BTreeMap<u32, String>,
        network_process_names: &BTreeSet<String>,
        now: Instant,
        action_log: &mut ActionLog,
    ) -> usize {
        let process_ids = target_processes
            .iter()
            .filter(|(_process_id, process_name)| {
                network_process_names.contains(&process_name_key(process_name))
            })
            .map(|(process_id, process_name)| (*process_id, process_name.clone()))
            .collect::<Vec<_>>();

        let mut failed = 0;
        for (process_id, process_name) in process_ids {
            let Some(thaw_until) = self.active_network_wake_until(&process_name, now) else {
                continue;
            };

            let was_suspended = self.suspended.contains_key(&process_id);
            if was_suspended {
                match self.thaw_process(process_id) {
                    Ok(()) => {}
                    Err(SuspensionError::ProcessExited) => {
                        self.forget_process_state(process_id);
                        continue;
                    }
                    Err(err) => {
                        failed += 1;
                        action_log.record(
                            ActionLogFeature::AppSuspension,
                            Some(process_id),
                            process_name,
                            ActionLogAction::Fail,
                            ActionLogResult::Failed,
                            suspension_error_message(err),
                        );
                        continue;
                    }
                }
            }
            self.suspended.remove(&process_id);

            self.tracked.remove(&process_name_key(&process_name));
            if was_suspended {
                action_log.record(
                    ActionLogFeature::AppSuspension,
                    Some(process_id),
                    process_name.clone(),
                    ActionLogAction::Restore,
                    ActionLogResult::Restored,
                    "Network activity woke the suspended process.",
                );
            }
            self.set_temporary_thaw(
                process_id,
                process_name,
                thaw_until,
                TemporaryThawReason::NetworkWake,
            );
        }

        failed
    }

    fn apply_audio_wake(
        &mut self,
        target_processes: &BTreeMap<u32, String>,
        audio_process_names: &BTreeSet<String>,
        now: Instant,
        action_log: &mut ActionLog,
    ) -> usize {
        let process_ids = target_processes
            .iter()
            .filter(|(_process_id, process_name)| {
                audio_process_names.contains(&process_name_key(process_name))
            })
            .map(|(process_id, process_name)| (*process_id, process_name.clone()))
            .collect::<Vec<_>>();

        let mut failed = 0;
        for (process_id, process_name) in process_ids {
            let Some(thaw_until) = self.active_audio_wake_until(&process_name, now) else {
                continue;
            };

            let was_suspended = self.suspended.contains_key(&process_id);
            if was_suspended {
                match self.thaw_process(process_id) {
                    Ok(()) => {}
                    Err(SuspensionError::ProcessExited) => {
                        self.forget_process_state(process_id);
                        continue;
                    }
                    Err(err) => {
                        failed += 1;
                        action_log.record(
                            ActionLogFeature::AppSuspension,
                            Some(process_id),
                            process_name,
                            ActionLogAction::Fail,
                            ActionLogResult::Failed,
                            suspension_error_message(err),
                        );
                        continue;
                    }
                }
            }
            self.suspended.remove(&process_id);

            self.tracked.remove(&process_name_key(&process_name));
            if was_suspended {
                action_log.record(
                    ActionLogFeature::AppSuspension,
                    Some(process_id),
                    process_name.clone(),
                    ActionLogAction::Restore,
                    ActionLogResult::Restored,
                    "Audio activity woke the suspended process.",
                );
            }
            self.set_temporary_thaw(
                process_id,
                process_name,
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
        thaw_until: Instant,
        reason: TemporaryThawReason,
    ) {
        match self.temporary_thawed.get_mut(&process_id) {
            Some(existing) if existing.thaw_until >= thaw_until => {
                existing.process_name = process_name;
            }
            Some(existing) => {
                existing.process_name = process_name;
                existing.thaw_until = thaw_until;
                existing.reason = reason;
            }
            None => {
                self.temporary_thawed.insert(
                    process_id,
                    TemporaryThaw {
                        process_name,
                        thaw_until,
                        reason,
                    },
                );
            }
        }
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

    fn active_network_wake_until(&self, process_name: &str, now: Instant) -> Option<Instant> {
        let window = self
            .network_wake_windows
            .get(&process_name_key(process_name))?;
        (now < window.wake_until).then_some(window.wake_until)
    }

    fn active_audio_wake_until(&self, process_name: &str, now: Instant) -> Option<Instant> {
        let window = self
            .audio_wake_windows
            .get(&process_name_key(process_name))?;
        (now < window.wake_until).then_some(window.wake_until)
    }

    fn network_wake_process_count(&self) -> usize {
        self.temporary_thawed
            .values()
            .filter(|process| process.reason == TemporaryThawReason::NetworkWake)
            .count()
    }

    fn audio_wake_process_count(&self) -> usize {
        self.temporary_thawed
            .values()
            .filter(|process| process.reason == TemporaryThawReason::AudioWake)
            .count()
    }

    fn temporary_thaw_state(
        &mut self,
        process_id: u32,
        process_name: &str,
        now: Instant,
    ) -> TemporaryThawState {
        let Some(thaw) = self.temporary_thawed.get_mut(&process_id) else {
            return TemporaryThawState::None;
        };

        if now < thaw.thaw_until {
            thaw.process_name = process_name.to_owned();
            TemporaryThawState::Active
        } else {
            self.temporary_thawed.remove(&process_id);
            TemporaryThawState::Expired
        }
    }

    fn suspension_lifecycle_state(
        &mut self,
        process_id: u32,
        process_name: &str,
        now: Instant,
        delay: Duration,
        manual_freeze: bool,
    ) -> SuspensionLifecycleState {
        let app_key = process_name_key(process_name);
        if manual_freeze {
            self.temporary_thawed.remove(&process_id);
            self.tracked.remove(&app_key);
            return SuspensionLifecycleState::ManualFreeze;
        }

        match self.temporary_thaw_state(process_id, process_name, now) {
            TemporaryThawState::Active => SuspensionLifecycleState::IntentActive,
            TemporaryThawState::Expired => {
                self.tracked.insert(
                    app_key,
                    TrackedApp {
                        process_name: process_name.to_owned(),
                        background_since: now.checked_sub(delay).unwrap_or(now),
                    },
                );
                SuspensionLifecycleState::ReadyToSuspend
            }
            TemporaryThawState::None => {
                let tracked = self.tracked.entry(app_key).or_insert_with(|| TrackedApp {
                    process_name: process_name.to_owned(),
                    background_since: now,
                });
                tracked.process_name = process_name.to_owned();
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
        process_id: u32,
        process_name: String,
        suspended_since: Instant,
    ) -> Result<(), SuspensionError> {
        if self
            .freezers
            .get(&process_id)
            .is_some_and(|freezer| !freezer.matches_process_id(process_id))
        {
            self.freezers.remove(&process_id);
        }

        match self.freezers.entry(process_id) {
            std::collections::btree_map::Entry::Occupied(entry) => {
                entry.get().set_frozen(true)?;
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                let freezer = ProcessFreezer::assign(process_id)?;
                if let Err(err) = freezer.set_frozen(true) {
                    drop(freezer);
                    return Err(err);
                }
                entry.insert(freezer);
            }
        }

        self.suspended.insert(
            process_id,
            SuspendedProcess {
                process_name,
                suspended_since,
            },
        );
        Ok(())
    }

    fn thaw_process(&self, process_id: u32) -> Result<(), SuspensionError> {
        match self.freezers.get(&process_id) {
            Some(freezer) if freezer.matches_process_id(process_id) => freezer.set_frozen(false),
            Some(_) => Err(SuspensionError::ProcessExited),
            None => Ok(()),
        }
    }

    fn snapshot(
        &self,
        enabled: bool,
        unsupported: bool,
        skipped_processes: usize,
        failed_actions: usize,
        message: String,
        last_error: Option<String>,
    ) -> AppSuspensionSnapshot {
        AppSuspensionSnapshot {
            enabled,
            unsupported,
            grace_apps: self.tracked.len(),
            suspended_processes: self.suspended.len(),
            temporary_thawed_processes: self.temporary_thawed.len(),
            network_wake_processes: self.network_wake_process_count(),
            audio_wake_processes: self.audio_wake_process_count(),
            background_grace_apps: unique_app_names(
                self.tracked
                    .values()
                    .map(|process| process.process_name.as_str()),
            ),
            suspended_apps: unique_app_names(
                self.suspended
                    .values()
                    .map(|process| process.process_name.as_str()),
            ),
            temporary_thawed_apps: unique_app_names(
                self.temporary_thawed
                    .values()
                    .map(|process| process.process_name.as_str()),
            ),
            network_wake_apps: unique_app_names(
                self.temporary_thawed
                    .values()
                    .filter(|process| process.reason == TemporaryThawReason::NetworkWake)
                    .map(|process| process.process_name.as_str()),
            ),
            audio_wake_apps: unique_app_names(
                self.temporary_thawed
                    .values()
                    .filter(|process| process.reason == TemporaryThawReason::AudioWake)
                    .map(|process| process.process_name.as_str()),
            ),
            running_apps: self.running_apps.iter().cloned().collect(),
            status_unknown: false,
            skipped_processes,
            failed_actions,
            auto_excluded_processes: Vec::new(),
            message,
            last_error,
        }
    }

    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log: &mut ActionLog,
        auto_excluded_processes: &mut BTreeSet<String>,
    ) -> bool {
        let suppression = self.failure_suppression.process_suppression(process_name);
        if !suppression.suppressed {
            return false;
        }

        if suppression.newly_suppressed {
            auto_excluded_processes.insert(process_name_key(process_name));
            action_log.record(
                ActionLogFeature::AppSuspension,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying App Suspension after {} failed attempts.",
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
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying App Suspension {action_label} after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }

        true
    }

    fn record_action_failure(&mut self, key: &str) {
        self.action_failure_suppression.record_key_failure(key);
    }

    fn clear_action_failure(&mut self, key: &str) {
        self.action_failure_suppression.clear_key_failure(key);
    }
}

impl Drop for AppSuspensionManager {
    fn drop(&mut self) {
        let mut action_log = ActionLog::new(1);
        self.clear_all(&mut action_log, "App Suspension manager dropped");
    }
}

impl AppSuspensionManager {
    fn apply_suspend_action(
        &mut self,
        process_id: u32,
        process_name: String,
        now: Instant,
    ) -> Result<(), SuspensionError> {
        self.suspend_process(process_id, process_name, now)
    }
}

impl Default for AppSuspensionSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            unsupported: false,
            grace_apps: 0,
            suspended_processes: 0,
            temporary_thawed_processes: 0,
            network_wake_processes: 0,
            audio_wake_processes: 0,
            background_grace_apps: Vec::new(),
            suspended_apps: Vec::new(),
            temporary_thawed_apps: Vec::new(),
            network_wake_apps: Vec::new(),
            audio_wake_apps: Vec::new(),
            running_apps: Vec::new(),
            status_unknown: false,
            skipped_processes: 0,
            failed_actions: 0,
            auto_excluded_processes: Vec::new(),
            message: "App Suspension disabled.".to_owned(),
            last_error: None,
        }
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    contains_process_name(BUILT_IN_EXCLUSIONS, process_name)
}

pub fn contains_process(list: &[String], process_name: &str) -> bool {
    contains_process_name(list, process_name)
}

fn network_wake_duration(settings: &AppSuspensionSettings) -> Option<Duration> {
    (settings.network_wake_enabled && settings.network_wake_duration_seconds > 0)
        .then(|| Duration::from_secs(settings.network_wake_duration_seconds))
}

fn audio_wake_duration(settings: &AppSuspensionSettings) -> Option<Duration> {
    (settings.audio_wake_enabled && settings.audio_wake_duration_seconds > 0)
        .then(|| Duration::from_secs(settings.audio_wake_duration_seconds))
}

fn audio_process_names_with_activity(
    target_processes: &BTreeMap<u32, String>,
) -> Result<BTreeSet<String>, String> {
    if target_processes.is_empty() {
        return Ok(BTreeSet::new());
    }

    let active_process_ids = active_audio_process_ids()?;
    Ok(target_processes
        .iter()
        .filter(|(process_id, _process_name)| active_process_ids.contains(process_id))
        .map(|(_process_id, process_name)| process_name_key(process_name))
        .collect())
}

fn network_connection_snapshot(
    target_processes: &BTreeMap<u32, String>,
) -> Result<NetworkConnectionSnapshot, String> {
    let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
    let mut connections_by_pid: NetworkConnectionsByProcess = BTreeMap::new();

    add_tcp_connections(&mut connections_by_pid, &target_ids, AF_INET as u32)?;
    add_tcp_connections(&mut connections_by_pid, &target_ids, AF_INET6 as u32)?;
    add_udp_connections(&mut connections_by_pid, &target_ids, AF_INET as u32)?;
    add_udp_connections(&mut connections_by_pid, &target_ids, AF_INET6 as u32)?;

    let mut snapshot = target_processes
        .values()
        .map(|process_name| (process_name_key(process_name), BTreeMap::new()))
        .collect::<NetworkConnectionSnapshot>();
    for (process_id, connections) in connections_by_pid {
        let Some(process_name) = target_processes.get(&process_id) else {
            continue;
        };

        snapshot
            .entry(process_name_key(process_name))
            .or_insert_with(BTreeMap::new)
            .extend(connections);
    }

    Ok(snapshot)
}

fn network_process_names_with_activity(
    previous: &NetworkConnectionSnapshot,
    current: &NetworkConnectionSnapshot,
    thresholds_by_process: &NetworkActivityThresholdsByProcess,
) -> BTreeSet<String> {
    current
        .iter()
        .filter(|(process_name, connections)| {
            let Some(thresholds) = thresholds_by_process.get(*process_name) else {
                return false;
            };
            previous
                .get(*process_name)
                .is_some_and(|previous_connections| {
                    connections.iter().any(|(connection, activity)| {
                        match previous_connections.get(connection) {
                            Some(Some(previous_activity)) => activity.is_some_and(|activity| {
                                activity.exceeds_threshold_since(*previous_activity, *thresholds)
                            }),
                            Some(None) => false,
                            None => false,
                        }
                    })
                })
        })
        .map(|(process_name, _connections)| process_name.clone())
        .collect()
}

fn network_activity_thresholds(
    settings: &AppSuspensionSettings,
    target_processes: &BTreeMap<u32, String>,
) -> NetworkActivityThresholdsByProcess {
    target_processes
        .values()
        .filter_map(|process_name| {
            let (bytes_in, bytes_out) = settings.network_wake_thresholds_for(process_name)?;
            Some((
                process_name_key(process_name),
                NetworkActivityThresholds {
                    bytes_in,
                    bytes_out,
                },
            ))
        })
        .collect()
}

fn eligible_network_wake_names(
    network_process_names: &BTreeSet<String>,
    network_target_process_names: &BTreeSet<String>,
) -> BTreeSet<String> {
    network_process_names
        .iter()
        .filter(|process_name| network_target_process_names.contains(*process_name))
        .cloned()
        .collect()
}

fn network_wake_scan_needed(network_target_process_names: &BTreeSet<String>) -> bool {
    !network_target_process_names.is_empty()
}

fn manual_freeze_app_names(process_names: &[String]) -> BTreeSet<String> {
    process_names
        .iter()
        .map(|process_name| process_name_key(process_name))
        .filter(|process_name| !process_name.is_empty())
        .collect()
}

fn add_tcp_connections(
    connections_by_pid: &mut NetworkConnectionsByProcess,
    target_ids: &BTreeSet<u32>,
    address_family: u32,
) -> Result<(), String> {
    let buffer = query_ip_helper_table(|table, size| unsafe {
        GetExtendedTcpTable(
            table,
            size,
            0,
            address_family,
            TCP_TABLE_OWNER_PID_CONNECTIONS,
            0,
        )
    })?;

    if address_family == AF_INET as u32 {
        for row in table_rows::<MIB_TCPROW_OWNER_PID>(&buffer) {
            if target_ids.contains(&row.dwOwningPid) {
                let Some(connection) = tcp4_connection_key(&row) else {
                    continue;
                };
                let activity = tcp4_connection_activity(&row);

                connections_by_pid
                    .entry(row.dwOwningPid)
                    .or_default()
                    .insert(connection, activity);
            }
        }
    } else {
        for row in table_rows::<MIB_TCP6ROW_OWNER_PID>(&buffer) {
            if target_ids.contains(&row.dwOwningPid) {
                let Some(connection) = tcp6_connection_key(&row) else {
                    continue;
                };
                let activity = tcp6_connection_activity(&row);

                connections_by_pid
                    .entry(row.dwOwningPid)
                    .or_default()
                    .insert(connection, activity);
            }
        }
    }

    Ok(())
}

fn tcp4_connection_key(row: &MIB_TCPROW_OWNER_PID) -> Option<String> {
    is_network_intent_tcp_state(row.dwState).then(|| {
        format!(
            "tcp4:{}:{}:{}:{}",
            row.dwLocalAddr, row.dwLocalPort, row.dwRemoteAddr, row.dwRemotePort
        )
    })
}

fn tcp6_connection_key(row: &MIB_TCP6ROW_OWNER_PID) -> Option<String> {
    is_network_intent_tcp_state(row.dwState).then(|| {
        format!(
            "tcp6:{:?}:{}:{:?}:{}:{}:{}",
            row.ucLocalAddr,
            row.dwLocalScopeId,
            row.ucRemoteAddr,
            row.dwRemoteScopeId,
            row.dwLocalPort,
            row.dwRemotePort
        )
    })
}

fn tcp4_connection_activity(row: &MIB_TCPROW_OWNER_PID) -> Option<NetworkActivityCounters> {
    let tcp_row = MIB_TCPROW_LH {
        Anonymous: MIB_TCPROW_LH_0 {
            dwState: row.dwState,
        },
        dwLocalAddr: row.dwLocalAddr,
        dwLocalPort: row.dwLocalPort,
        dwRemoteAddr: row.dwRemoteAddr,
        dwRemotePort: row.dwRemotePort,
    };
    enable_tcp4_data_stats(&tcp_row);
    tcp4_data_stats(&tcp_row)
}

fn tcp6_connection_activity(row: &MIB_TCP6ROW_OWNER_PID) -> Option<NetworkActivityCounters> {
    let tcp_row = MIB_TCP6ROW {
        State: row.dwState as i32,
        LocalAddr: IN6_ADDR {
            u: IN6_ADDR_0 {
                Byte: row.ucLocalAddr,
            },
        },
        dwLocalScopeId: row.dwLocalScopeId,
        dwLocalPort: row.dwLocalPort,
        RemoteAddr: IN6_ADDR {
            u: IN6_ADDR_0 {
                Byte: row.ucRemoteAddr,
            },
        },
        dwRemoteScopeId: row.dwRemoteScopeId,
        dwRemotePort: row.dwRemotePort,
    };
    enable_tcp6_data_stats(&tcp_row);
    tcp6_data_stats(&tcp_row)
}

fn enable_tcp4_data_stats(row: &MIB_TCPROW_LH) {
    let rw = TCP_ESTATS_DATA_RW_v0 {
        EnableCollection: true,
    };
    unsafe {
        SetPerTcpConnectionEStats(
            row,
            TcpConnectionEstatsData,
            &rw as *const _ as *const u8,
            0,
            mem::size_of::<TCP_ESTATS_DATA_RW_v0>() as u32,
            0,
        );
    }
}

fn enable_tcp6_data_stats(row: &MIB_TCP6ROW) {
    let rw = TCP_ESTATS_DATA_RW_v0 {
        EnableCollection: true,
    };
    unsafe {
        SetPerTcp6ConnectionEStats(
            row,
            TcpConnectionEstatsData,
            &rw as *const _ as *const u8,
            0,
            mem::size_of::<TCP_ESTATS_DATA_RW_v0>() as u32,
            0,
        );
    }
}

fn tcp4_data_stats(row: &MIB_TCPROW_LH) -> Option<NetworkActivityCounters> {
    let mut rod = TCP_ESTATS_DATA_ROD_v0::default();
    let status = unsafe {
        GetPerTcpConnectionEStats(
            row,
            TcpConnectionEstatsData,
            null_mut(),
            0,
            0,
            null_mut(),
            0,
            0,
            &mut rod as *mut _ as *mut u8,
            0,
            mem::size_of::<TCP_ESTATS_DATA_ROD_v0>() as u32,
        )
    };

    (status == NO_ERROR).then_some(NetworkActivityCounters {
        bytes_in: rod.DataBytesIn,
        bytes_out: rod.DataBytesOut,
    })
}

fn tcp6_data_stats(row: &MIB_TCP6ROW) -> Option<NetworkActivityCounters> {
    let mut rod = TCP_ESTATS_DATA_ROD_v0::default();
    let status = unsafe {
        GetPerTcp6ConnectionEStats(
            row,
            TcpConnectionEstatsData,
            null_mut(),
            0,
            0,
            null_mut(),
            0,
            0,
            &mut rod as *mut _ as *mut u8,
            0,
            mem::size_of::<TCP_ESTATS_DATA_ROD_v0>() as u32,
        )
    };

    (status == NO_ERROR).then_some(NetworkActivityCounters {
        bytes_in: rod.DataBytesIn,
        bytes_out: rod.DataBytesOut,
    })
}

fn is_network_intent_tcp_state(state: u32) -> bool {
    matches!(
        state,
        TCP_STATE_SYN_SENT | TCP_STATE_SYN_RECEIVED | TCP_STATE_ESTABLISHED
    )
}

fn add_udp_connections(
    connections_by_pid: &mut NetworkConnectionsByProcess,
    target_ids: &BTreeSet<u32>,
    address_family: u32,
) -> Result<(), String> {
    let buffer = query_ip_helper_table(|table, size| unsafe {
        GetExtendedUdpTable(table, size, 0, address_family, UDP_TABLE_OWNER_PID, 0)
    })?;

    if address_family == AF_INET as u32 {
        for row in table_rows::<MIB_UDPROW_OWNER_PID>(&buffer) {
            if target_ids.contains(&row.dwOwningPid) {
                connections_by_pid
                    .entry(row.dwOwningPid)
                    .or_default()
                    .insert(
                        format!("udp4:{}:{}", row.dwLocalAddr, row.dwLocalPort),
                        None,
                    );
            }
        }
    } else {
        for row in table_rows::<MIB_UDP6ROW_OWNER_PID>(&buffer) {
            if target_ids.contains(&row.dwOwningPid) {
                connections_by_pid
                    .entry(row.dwOwningPid)
                    .or_default()
                    .insert(
                        format!(
                            "udp6:{:?}:{}:{}",
                            row.ucLocalAddr, row.dwLocalScopeId, row.dwLocalPort
                        ),
                        None,
                    );
            }
        }
    }

    Ok(())
}

fn query_ip_helper_table(
    mut query: impl FnMut(*mut c_void, *mut u32) -> u32,
) -> Result<Vec<u8>, String> {
    let mut size = 0;
    let first_status = query(null_mut(), &mut size);
    if first_status != ERROR_INSUFFICIENT_BUFFER && first_status != NO_ERROR {
        return Err(format!(
            "Network intent detection failed to size IP Helper table with error {first_status}."
        ));
    }

    if size == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u8; size as usize];
    let status = query(buffer.as_mut_ptr() as *mut c_void, &mut size);
    if status != NO_ERROR {
        return Err(format!(
            "Network intent detection failed to read IP Helper table with error {status}."
        ));
    }

    Ok(buffer)
}

fn table_rows<T: Copy>(buffer: &[u8]) -> Vec<T> {
    if buffer.len() < mem::size_of::<u32>() {
        return Vec::new();
    }

    let count = unsafe { ptr::read_unaligned(buffer.as_ptr() as *const u32) as usize };
    if count == 0 {
        return Vec::new();
    }

    let rows_offset = mem::size_of::<u32>();
    let row_size = mem::size_of::<T>();
    let Some(rows_len) = count.checked_mul(row_size) else {
        return Vec::new();
    };
    let Some(required_len) = rows_offset.checked_add(rows_len) else {
        return Vec::new();
    };
    if row_size == 0 || buffer.len() < required_len {
        return Vec::new();
    }

    let rows_ptr = unsafe { buffer.as_ptr().add(rows_offset) as *const T };
    (0..count)
        .map(|index| unsafe { ptr::read_unaligned(rows_ptr.add(index)) })
        .collect()
}

fn should_skip_foreground_process(
    process_id: u32,
    process_name: &str,
    foreground_process_id: u32,
    foreground_process_name: Option<&str>,
) -> bool {
    process_id == foreground_process_id
        || foreground_process_name
            .is_some_and(|name| process_name_key(name) == process_name_key(process_name))
}

#[derive(Debug, PartialEq, Eq)]
enum SuspensionError {
    AccessDenied,
    ProcessExited,
    NotSupported,
    Unsupported,
    Failed(String),
}

fn suspension_error_message(error: SuspensionError) -> String {
    match error {
        SuspensionError::AccessDenied => "Access denied.".to_owned(),
        SuspensionError::ProcessExited => "Process exited.".to_owned(),
        SuspensionError::NotSupported => "Operation not supported for this process.".to_owned(),
        SuspensionError::Unsupported => "Windows Job Object freeze is unsupported.".to_owned(),
        SuspensionError::Failed(message) => message,
    }
}

const JOB_OBJECT_FREEZE_INFORMATION_CLASS: i32 = 18;
const JOB_OBJECT_FREEZE_OPERATION: u32 = 1;

#[repr(C)]
struct JobObjectFreezeInformation {
    flags: u32,
    freeze: u8,
    swap: u8,
    spare: u16,
    wake_filter_high: u32,
    wake_filter_low: u32,
}

impl JobObjectFreezeInformation {
    fn new(frozen: bool) -> Self {
        Self {
            flags: JOB_OBJECT_FREEZE_OPERATION,
            freeze: u8::from(frozen),
            swap: 0,
            spare: 0,
            wake_filter_high: 0,
            wake_filter_low: 0,
        }
    }
}

struct ProcessFreezer {
    job_handle: Option<WinHandle>,
    process_handle: Option<WinHandle>,
    process_creation_time: Option<u64>,
    can_wait_for_process: bool,
}

impl ProcessFreezer {
    fn assign(process_id: u32) -> Result<Self, SuspensionError> {
        let (process_handle, can_wait_for_process) = open_process_for_job_assignment(process_id)?;

        let job_handle = unsafe { CreateJobObjectW(null(), null()) };
        if job_handle.is_null() {
            let error = last_error();
            return Err(SuspensionError::Failed(format!(
                "CreateJobObjectW failed with error {error}."
            )));
        }
        let job_handle = WinHandle::new(job_handle);

        let assigned =
            unsafe { AssignProcessToJobObject(job_handle.raw(), process_handle.raw()) != 0 };
        if !assigned {
            let error = last_error();
            let assignment_error =
                assign_process_to_job_error_with_context(process_id, process_handle.raw(), error);
            return Err(assignment_error);
        }

        Ok(Self {
            job_handle: Some(job_handle),
            process_creation_time: process_creation_time(process_handle.raw()),
            process_handle: Some(process_handle),
            can_wait_for_process,
        })
    }

    fn set_frozen(&self, frozen: bool) -> Result<(), SuspensionError> {
        let mut info = JobObjectFreezeInformation::new(frozen);
        let Some(job_handle) = &self.job_handle else {
            return Ok(());
        };

        let ok = unsafe {
            SetInformationJobObject(
                job_handle.raw(),
                JOB_OBJECT_FREEZE_INFORMATION_CLASS,
                &mut info as *mut _ as *mut c_void,
                std::mem::size_of::<JobObjectFreezeInformation>() as u32,
            )
        };

        if ok == 0 {
            Err(job_freeze_error(frozen, last_error()))
        } else {
            Ok(())
        }
    }

    fn is_process_alive(&self) -> bool {
        !self.can_wait_for_process
            || self
                .process_handle
                .as_ref()
                .is_some_and(|process_handle| unsafe {
                    WaitForSingleObject(process_handle.raw(), 0) == WAIT_TIMEOUT
                })
    }

    fn matches_process_id(&self, process_id: u32) -> bool {
        self.is_process_alive()
            && (self.can_wait_for_process
                || process_creation_time_matches(
                    self.process_creation_time,
                    current_process_creation_time(process_id),
                ))
    }

    fn close(&mut self) {
        self.job_handle = None;
        self.process_handle = None;
    }
}

impl Drop for ProcessFreezer {
    fn drop(&mut self) {
        if self.job_handle.is_some() {
            let _ = self.set_frozen(false);
        }
        self.close();
    }
}

fn null_mut_handle() -> HANDLE {
    std::ptr::null_mut()
}

fn open_process_for_job_assignment(process_id: u32) -> Result<(WinHandle, bool), SuspensionError> {
    let access_masks = [
        PROCESS_SET_QUOTA
            | PROCESS_TERMINATE
            | PROCESS_SYNCHRONIZE
            | PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SET_QUOTA | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SET_QUOTA | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
        PROCESS_SET_QUOTA | PROCESS_TERMINATE,
    ];

    let mut last_open_error = 0;
    for access in access_masks {
        let handle = unsafe { OpenProcess(access, 0, process_id) };
        if !handle.is_null() {
            return Ok((WinHandle::new(handle), access & PROCESS_SYNCHRONIZE != 0));
        }
        last_open_error = last_error();
    }

    Err(open_process_error(process_id, last_open_error))
}

fn current_process_creation_time(process_id: u32) -> Option<u64> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return None;
    }
    let handle = WinHandle::new(handle);
    process_creation_time(handle.raw())
}

fn process_creation_time(process_handle: HANDLE) -> Option<u64> {
    let mut creation_time = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut exit_time = creation_time;
    let mut kernel_time = creation_time;
    let mut user_time = creation_time;

    let ok = unsafe {
        GetProcessTimes(
            process_handle,
            &mut creation_time,
            &mut exit_time,
            &mut kernel_time,
            &mut user_time,
        )
    };
    (ok != 0).then(|| filetime_to_u64(creation_time))
}

fn process_creation_time_matches(recorded: Option<u64>, current: Option<u64>) -> bool {
    match recorded {
        Some(recorded) => current == Some(recorded),
        None => true,
    }
}

fn open_process_error(process_id: u32, error: u32) -> SuspensionError {
    match error {
        ERROR_ACCESS_DENIED => SuspensionError::AccessDenied,
        ERROR_INVALID_PARAMETER => SuspensionError::ProcessExited,
        ERROR_NOT_SUPPORTED => SuspensionError::NotSupported,
        _ => SuspensionError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

fn assign_process_to_job_error(process_id: u32, error: u32) -> SuspensionError {
    match error {
        ERROR_ACCESS_DENIED => SuspensionError::AccessDenied,
        ERROR_NOT_SUPPORTED => SuspensionError::NotSupported,
        _ => SuspensionError::Failed(format!(
            "AssignProcessToJobObject({process_id}) failed with error {error}."
        )),
    }
}

fn assign_process_to_job_error_with_context(
    process_id: u32,
    process_handle: HANDLE,
    error: u32,
) -> SuspensionError {
    if process_is_in_job(process_handle) == Some(true) {
        return SuspensionError::Failed(format!(
            "AssignProcessToJobObject({process_id}) failed with error {error}; process is already in a job object."
        ));
    }

    assign_process_to_job_error(process_id, error)
}

fn process_is_in_job(process_handle: HANDLE) -> Option<bool> {
    let mut in_job = 0;
    let ok = unsafe { IsProcessInJob(process_handle, null_mut_handle(), &mut in_job) };
    (ok != 0).then_some(in_job != 0)
}

fn job_freeze_error(frozen: bool, error: u32) -> SuspensionError {
    match error {
        ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED => SuspensionError::Unsupported,
        _ => SuspensionError::Failed(format!(
            "SetInformationJobObject freeze={frozen} failed with error {error}."
        )),
    }
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

    fn inert_freezer() -> ProcessFreezer {
        ProcessFreezer {
            job_handle: None,
            process_handle: None,
            process_creation_time: None,
            can_wait_for_process: false,
        }
    }

    #[test]
    fn process_creation_time_must_match_when_recorded() {
        assert!(process_creation_time_matches(None, None));
        assert!(process_creation_time_matches(Some(10), Some(10)));
        assert!(!process_creation_time_matches(Some(10), Some(11)));
        assert!(!process_creation_time_matches(Some(10), None));
    }

    #[test]
    fn suspendable_app_match_is_case_insensitive() {
        let suspendable_apps = vec!["chat.exe".to_owned()];

        assert!(contains_process(&suspendable_apps, "CHAT.EXE"));
        assert!(!contains_process(&suspendable_apps, "browser.exe"));
    }

    #[test]
    fn builtin_exclusions_cover_sensitive_windows_shell_processes() {
        for process_name in [
            "explorer.exe",
            "SearchApp.exe",
            "SearchHost.exe",
            "SystemSettings.exe",
            "TextInputHost.exe",
        ] {
            assert!(is_builtin_excluded(process_name), "{process_name}");
        }

        assert!(!is_builtin_excluded("chat.exe"));
    }

    #[test]
    fn foreground_skip_matches_pid_or_process_name() {
        assert!(should_skip_foreground_process(
            42,
            "helper.exe",
            42,
            Some("app.exe"),
        ));
        assert!(should_skip_foreground_process(
            99,
            "APP.EXE",
            42,
            Some("app.exe"),
        ));
        assert!(!should_skip_foreground_process(
            99,
            "other.exe",
            42,
            Some("app.exe"),
        ));
    }

    #[test]
    fn repeated_failures_suppress_future_suspension_attempts_once() {
        let mut manager = AppSuspensionManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure("APP.exe");
        manager.record_process_failure("app.exe");
        assert!(!manager.is_process_suppressed(42, "app.exe", &mut log, &mut BTreeSet::new()));
        assert!(log.entries().is_empty());

        manager.record_process_failure("app.exe");
        assert!(manager.is_process_suppressed(42, "app.exe", &mut log, &mut BTreeSet::new()));
        assert!(manager.is_process_suppressed(43, "APP.exe", &mut log, &mut BTreeSet::new()));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].process_name, "app.exe");
        assert_eq!(entries[0].action, ActionLogAction::Skip);
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
    }

    #[test]
    fn repeated_action_failures_suppress_future_suspension_detection_once() {
        let mut manager = AppSuspensionManager::default();
        let mut log = ActionLog::new(8);

        manager.record_action_failure(NETWORK_DETECTION_FAILURE_KEY);
        manager.record_action_failure(NETWORK_DETECTION_FAILURE_KEY);
        assert!(!manager.is_action_suppressed(
            NETWORK_DETECTION_FAILURE_KEY,
            "network activity detection",
            &mut log,
        ));
        assert!(log.entries().is_empty());

        manager.record_action_failure(NETWORK_DETECTION_FAILURE_KEY);
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
        assert_eq!(entries[0].action, ActionLogAction::Skip);
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
    }

    #[test]
    fn user_intent_release_thaws_window_owner_processes_only() {
        let mut manager = AppSuspensionManager::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        manager.suspended.insert(
            7,
            SuspendedProcess {
                process_name: "chat.exe".to_owned(),
                suspended_since: now,
            },
        );
        manager.suspended.insert(
            8,
            SuspendedProcess {
                process_name: "mail.exe".to_owned(),
                suspended_since: now,
            },
        );

        let status = manager
            .release_window_owner_processes_for_user_intent(&BTreeSet::from([7]), &mut log)
            .unwrap();

        assert_eq!(status.suspended_processes, 1);
        assert_eq!(status.temporary_thawed_processes, 1);
        assert!(!manager.suspended.contains_key(&7));
        assert!(manager.suspended.contains_key(&8));
        assert!(manager.temporary_thawed.contains_key(&7));
    }

    #[test]
    fn user_intent_release_does_not_extend_existing_temporary_thaw() {
        let mut manager = AppSuspensionManager::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        manager.temporary_thawed.insert(
            7,
            TemporaryThaw {
                process_name: "chat.exe".to_owned(),
                thaw_until: now + Duration::from_secs(5),
                reason: TemporaryThawReason::UserIntent,
            },
        );

        assert!(manager
            .release_window_owner_processes_for_user_intent(&BTreeSet::from([7]), &mut log)
            .is_none());
        assert_eq!(
            manager.temporary_thawed.get(&7).unwrap().thaw_until,
            now + Duration::from_secs(5)
        );
    }

    #[test]
    fn user_intent_release_returns_none_without_matching_window_owner() {
        let mut manager = AppSuspensionManager::default();
        let mut log = ActionLog::new(8);

        assert!(manager
            .release_window_owner_processes_for_user_intent(&BTreeSet::from([42]), &mut log)
            .is_none());
    }

    #[test]
    fn temporary_thaw_state_blocks_until_window_expires() {
        let mut manager = AppSuspensionManager::default();
        let now = Instant::now();
        manager.temporary_thawed.insert(
            7,
            TemporaryThaw {
                process_name: "chat.exe".to_owned(),
                thaw_until: now + Duration::from_secs(5),
                reason: TemporaryThawReason::Fallback,
            },
        );

        assert_eq!(
            manager.temporary_thaw_state(7, "CHAT.EXE", now),
            TemporaryThawState::Active
        );
        assert_eq!(
            manager.temporary_thawed.get(&7).unwrap().process_name,
            "CHAT.EXE"
        );
        assert_eq!(
            manager.temporary_thaw_state(7, "chat.exe", now + Duration::from_secs(6)),
            TemporaryThawState::Expired
        );
        assert!(!manager.temporary_thawed.contains_key(&7));
    }

    #[test]
    fn temporary_thaw_state_reports_none_without_entry() {
        let mut manager = AppSuspensionManager::default();

        assert_eq!(
            manager.temporary_thaw_state(99, "chat.exe", Instant::now()),
            TemporaryThawState::None
        );
    }

    #[test]
    fn suspension_lifecycle_keeps_intent_above_delay_unless_manual_freeze() {
        let mut manager = AppSuspensionManager::default();
        let now = Instant::now();
        manager.temporary_thawed.insert(
            7,
            TemporaryThaw {
                process_name: "chat.exe".to_owned(),
                thaw_until: now + Duration::from_secs(5),
                reason: TemporaryThawReason::NetworkWake,
            },
        );

        assert_eq!(
            manager.suspension_lifecycle_state(7, "chat.exe", now, Duration::ZERO, false),
            SuspensionLifecycleState::IntentActive
        );
        assert_eq!(
            manager.suspension_lifecycle_state(7, "chat.exe", now, Duration::ZERO, true),
            SuspensionLifecycleState::ManualFreeze
        );
        assert!(!manager.temporary_thawed.contains_key(&7));
    }

    #[test]
    fn suspension_lifecycle_uses_background_grace_before_ready() {
        let mut manager = AppSuspensionManager::default();
        let now = Instant::now();

        assert_eq!(
            manager.suspension_lifecycle_state(7, "chat.exe", now, Duration::from_secs(10), false),
            SuspensionLifecycleState::BackgroundGrace
        );
        manager
            .tracked
            .get_mut("chat.exe")
            .unwrap()
            .background_since = now.checked_sub(Duration::from_secs(11)).unwrap();

        assert_eq!(
            manager.suspension_lifecycle_state(7, "chat.exe", now, Duration::from_secs(10), false),
            SuspensionLifecycleState::ReadyToSuspend
        );
    }

    #[test]
    fn suspension_lifecycle_shares_background_grace_by_app_name() {
        let mut manager = AppSuspensionManager::default();
        let now = Instant::now();

        assert_eq!(
            manager.suspension_lifecycle_state(7, "chat.exe", now, Duration::from_secs(10), false),
            SuspensionLifecycleState::BackgroundGrace
        );
        manager
            .tracked
            .get_mut("chat.exe")
            .unwrap()
            .background_since = now.checked_sub(Duration::from_secs(11)).unwrap();

        assert_eq!(
            manager.suspension_lifecycle_state(8, "CHAT.EXE", now, Duration::from_secs(10), false),
            SuspensionLifecycleState::ReadyToSuspend
        );
    }

    #[test]
    fn snapshot_reports_running_apps() {
        let mut manager = AppSuspensionManager::default();
        manager.running_apps.insert("chat.exe".to_owned());

        let status = manager.snapshot(true, false, 0, 0, "App Suspension active.".to_owned(), None);

        assert_eq!(status.running_apps, vec!["chat.exe".to_owned()]);
    }

    #[test]
    fn assign_process_error_50_is_skipped_not_failed() {
        assert_eq!(
            assign_process_to_job_error(3252, ERROR_NOT_SUPPORTED),
            SuspensionError::NotSupported
        );
    }

    #[test]
    fn open_process_invalid_parameter_means_process_exited() {
        assert_eq!(
            open_process_error(42, ERROR_INVALID_PARAMETER),
            SuspensionError::ProcessExited
        );
    }

    #[test]
    fn job_freeze_unsupported_codes_mark_feature_unsupported() {
        assert_eq!(
            job_freeze_error(true, ERROR_NOT_SUPPORTED),
            SuspensionError::Unsupported
        );
        assert_eq!(
            job_freeze_error(true, ERROR_INVALID_PARAMETER),
            SuspensionError::Unsupported
        );
    }

    #[test]
    fn job_freeze_information_uses_expected_layout() {
        let frozen = JobObjectFreezeInformation::new(true);
        let thawed = JobObjectFreezeInformation::new(false);

        assert_eq!(mem::size_of::<JobObjectFreezeInformation>(), 16);
        assert_eq!(frozen.flags, JOB_OBJECT_FREEZE_OPERATION);
        assert_eq!(frozen.freeze, 1);
        assert_eq!(thawed.freeze, 0);
        assert_eq!(frozen.swap, 0);
        assert_eq!(frozen.spare, 0);
        assert_eq!(frozen.wake_filter_high, 0);
        assert_eq!(frozen.wake_filter_low, 0);
    }

    #[test]
    fn release_non_targets_closes_thawed_freezers() {
        let mut manager = AppSuspensionManager::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        manager.freezers.insert(7, inert_freezer());
        manager.temporary_thawed.insert(
            7,
            TemporaryThaw {
                process_name: "chat.exe".to_owned(),
                thaw_until: now + Duration::from_secs(5),
                reason: TemporaryThawReason::Fallback,
            },
        );

        assert_eq!(
            manager.release_non_targets(&BTreeSet::new(), &mut log, "test"),
            0
        );
        assert!(manager.freezers.is_empty());
        assert!(manager.temporary_thawed.is_empty());
    }

    #[test]
    fn release_non_targets_keeps_target_thawed_freezers() {
        let mut manager = AppSuspensionManager::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        manager.freezers.insert(7, inert_freezer());
        manager.temporary_thawed.insert(
            7,
            TemporaryThaw {
                process_name: "chat.exe".to_owned(),
                thaw_until: now + Duration::from_secs(5),
                reason: TemporaryThawReason::Fallback,
            },
        );

        assert_eq!(
            manager.release_non_targets(&BTreeSet::from([7]), &mut log, "test"),
            0
        );
        assert!(manager.freezers.contains_key(&7));
        assert!(manager.temporary_thawed.contains_key(&7));
    }

    #[test]
    fn foreground_unknown_pauses_without_releasing_suspended_processes() {
        let mut manager = AppSuspensionManager::default();
        let mut log = ActionLog::new(8);
        let settings = AppSuspensionSettings {
            enabled: true,
            ..Default::default()
        };
        let now = Instant::now();
        manager.tracked.insert(
            "chat.exe".to_owned(),
            TrackedApp {
                process_name: "chat.exe".to_owned(),
                background_since: now,
            },
        );
        manager.freezers.insert(7, inert_freezer());
        manager.suspended.insert(
            7,
            SuspendedProcess {
                process_name: "chat.exe".to_owned(),
                suspended_since: now,
            },
        );

        let status = manager.update(&settings, true, None, &[], &mut log);

        assert_eq!(status.message, "Paused: foreground app is unknown.");
        assert!(status.status_unknown);
        assert_eq!(status.grace_apps, 0);
        assert_eq!(status.suspended_processes, 1);
        assert!(manager.tracked.is_empty());
        assert!(manager.suspended.contains_key(&7));
        assert!(manager.freezers.contains_key(&7));
    }

    #[test]
    fn interactive_release_matches_process_app_group() {
        let mut manager = AppSuspensionManager::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        manager.tracked.insert(
            "chat.exe".to_owned(),
            TrackedApp {
                process_name: "chat.exe".to_owned(),
                background_since: now,
            },
        );
        manager.suspended.insert(
            7,
            SuspendedProcess {
                process_name: "chat.exe".to_owned(),
                suspended_since: now,
            },
        );
        manager.suspended.insert(
            8,
            SuspendedProcess {
                process_name: "CHAT.EXE".to_owned(),
                suspended_since: now,
            },
        );
        manager.suspended.insert(
            9,
            SuspendedProcess {
                process_name: "mail.exe".to_owned(),
                suspended_since: now,
            },
        );

        let status = manager
            .release_interactive_process(7, Some("chat.exe"), &mut log)
            .unwrap();

        assert_eq!(status.grace_apps, 0);
        assert_eq!(status.suspended_processes, 1);
        assert!(!manager.tracked.contains_key("chat.exe"));
        assert!(!manager.suspended.contains_key(&7));
        assert!(!manager.suspended.contains_key(&8));
        assert!(manager.suspended.contains_key(&9));
    }

    #[test]
    fn interactive_release_uses_managed_process_name_when_lookup_is_unavailable() {
        let mut manager = AppSuspensionManager::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        manager.suspended.insert(
            7,
            SuspendedProcess {
                process_name: "browser.exe".to_owned(),
                suspended_since: now,
            },
        );
        manager.suspended.insert(
            8,
            SuspendedProcess {
                process_name: "BROWSER.EXE".to_owned(),
                suspended_since: now,
            },
        );

        let status = manager
            .release_interactive_process(7, None, &mut log)
            .unwrap();

        assert_eq!(status.suspended_processes, 0);
        assert!(manager.suspended.is_empty());
    }

    #[test]
    fn interactive_release_clears_matching_thawed_freezers() {
        let mut manager = AppSuspensionManager::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();
        manager.freezers.insert(7, inert_freezer());
        manager.freezers.insert(8, inert_freezer());
        manager.temporary_thawed.insert(
            7,
            TemporaryThaw {
                process_name: "chat.exe".to_owned(),
                thaw_until: now + Duration::from_secs(5),
                reason: TemporaryThawReason::Fallback,
            },
        );
        manager.temporary_thawed.insert(
            8,
            TemporaryThaw {
                process_name: "CHAT.EXE".to_owned(),
                thaw_until: now + Duration::from_secs(5),
                reason: TemporaryThawReason::Fallback,
            },
        );

        let status = manager
            .release_interactive_process(7, Some("chat.exe"), &mut log)
            .unwrap();

        assert_eq!(status.temporary_thawed_processes, 0);
        assert!(!manager.freezers.contains_key(&7));
        assert!(!manager.temporary_thawed.contains_key(&7));
        assert!(!manager.freezers.contains_key(&8));
        assert!(!manager.temporary_thawed.contains_key(&8));
    }

    #[test]
    fn interactive_release_returns_none_without_matching_controlled_process() {
        let mut manager = AppSuspensionManager::default();
        let mut log = ActionLog::new(8);

        assert!(manager
            .release_interactive_process(42, Some("chat.exe"), &mut log)
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
    fn network_wake_scan_is_needed_only_for_network_wake_targets() {
        let empty = BTreeSet::new();
        let target = BTreeSet::from(["chat.exe".to_owned()]);

        assert!(!network_wake_scan_needed(&empty));
        assert!(network_wake_scan_needed(&target));
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
    fn process_name_key_trims_and_lowercases() {
        assert_eq!(process_name_key(" Chrome.EXE "), "chrome.exe");
    }

    #[test]
    fn manual_freeze_app_names_match_entire_app_group() {
        let requests = manual_freeze_app_names(&[" Chat.EXE ".to_owned(), "chat.exe".to_owned()]);

        assert!(requests.contains("chat.exe"));
        assert_eq!(requests.len(), 1);
        assert!(!requests.contains("mail.exe"));
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
