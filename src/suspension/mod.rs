use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    mem, ptr,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER,
        ERROR_NOT_SUPPORTED, HANDLE, NO_ERROR,
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
        JobObjects::{AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject},
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{GetCurrentProcessId, OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
    },
};

use crate::{config::AppSuspensionSettings, foreground::list_processes};

const BUILT_IN_EXCLUSIONS: &[&str] = &[
    "audiodg.exe",
    "conhost.exe",
    "csrss.exe",
    "ctfmon.exe",
    "dwm.exe",
    "explorer.exe",
    "fontdrvhost.exe",
    "lsaiso.exe",
    "lsass.exe",
    "registry",
    "searchhost.exe",
    "securityhealthservice.exe",
    "securityhealthsystray.exe",
    "services.exe",
    "shellexperiencehost.exe",
    "sihost.exe",
    "smss.exe",
    "startmenuexperiencehost.exe",
    "system",
    "taskmgr.exe",
    "textinputhost.exe",
    "wininit.exe",
    "winlogon.exe",
    "wudfhost.exe",
];
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSuspensionSnapshot {
    pub enabled: bool,
    pub tracked_processes: usize,
    pub suspended_processes: usize,
    pub temporary_thawed_processes: usize,
    pub network_wake_processes: usize,
    pub tracked_apps: Vec<String>,
    pub suspended_apps: Vec<String>,
    pub temporary_thawed_apps: Vec<String>,
    pub network_wake_apps: Vec<String>,
    pub skipped_processes: usize,
    pub failed_actions: usize,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct AppSuspensionManager {
    tracked: BTreeMap<u32, TrackedProcess>,
    suspended: BTreeMap<u32, SuspendedProcess>,
    temporary_thawed: BTreeMap<u32, TemporaryThaw>,
    network_snapshot: NetworkConnectionSnapshot,
    network_wake_windows: BTreeMap<String, NetworkWakeWindow>,
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
        (thresholds.bytes_in > 0 && bytes_in >= thresholds.bytes_in)
            || (thresholds.bytes_out > 0 && bytes_out >= thresholds.bytes_out)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NetworkActivityThresholds {
    bytes_in: u64,
    bytes_out: u64,
}

struct TrackedProcess {
    process_name: String,
    background_since: Instant,
}

struct SuspendedProcess {
    process_name: String,
    freezer: ProcessFreezer,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TemporaryThawState {
    None,
    Active,
    Expired,
}

impl AppSuspensionManager {
    pub fn update(
        &mut self,
        settings: &AppSuspensionSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        manual_freeze_processes: &[String],
    ) -> AppSuspensionSnapshot {
        let now = Instant::now();

        if !automation_enabled {
            let failed = self.clear_all();
            return AppSuspensionSnapshot {
                enabled: false,
                failed_actions: failed,
                message: "Automation disabled.".to_owned(),
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failed = self.clear_all();
            return AppSuspensionSnapshot {
                enabled: false,
                failed_actions: failed,
                message: "App Suspension disabled.".to_owned(),
                ..Default::default()
            };
        }

        let mut failed_actions = 0;

        let Some(foreground_process_id) = foreground_process_id else {
            failed_actions += self.clear_all();
            return AppSuspensionSnapshot {
                enabled: true,
                failed_actions,
                message: "Paused: foreground app is unknown.".to_owned(),
                ..Default::default()
            };
        };

        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            failed_actions += self.clear_all();
            return AppSuspensionSnapshot {
                enabled: true,
                failed_actions,
                message: "Paused: current Windows session is unknown.".to_owned(),
                ..Default::default()
            };
        };

        let processes = match list_processes() {
            Ok(processes) => processes,
            Err(err) => {
                failed_actions += self.clear_all();
                return AppSuspensionSnapshot {
                    enabled: true,
                    failed_actions,
                    message: err,
                    ..Default::default()
                };
            }
        };

        let foreground_process_name = processes
            .iter()
            .find(|process| process.id == foreground_process_id)
            .map(|process| process.name.clone());
        let delay = Duration::from_secs(settings.background_delay_seconds);
        let mut target_processes = BTreeMap::new();

        for process in processes {
            if process.id == 0
                || process.id == current_process_id
                || is_builtin_excluded(&process.name)
                || !settings.contains_suspendable_app(&process.name)
                || should_skip_foreground_process(
                    process.id,
                    &process.name,
                    foreground_process_id,
                    foreground_process_name.as_deref(),
                )
            {
                continue;
            }

            if process_session_id(process.id) != Some(current_session_id) {
                continue;
            }

            target_processes.insert(process.id, process.name);
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        failed_actions += self.release_non_targets(&target_ids);
        self.tracked
            .retain(|process_id, _process| target_ids.contains(process_id));
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
        let manual_freeze_names = manual_freeze_processes
            .iter()
            .map(|process_name| process_name_key(process_name))
            .collect::<BTreeSet<_>>();
        for process_name in &manual_freeze_names {
            self.network_wake_windows.remove(process_name);
        }
        if settings.network_wake_enabled {
            self.prune_network_wake_windows(&network_target_process_names, now);
        } else {
            self.network_wake_windows.clear();
        }

        let mut skipped_processes = 0;
        let mut last_error = None;
        let suspended_process_names = self
            .suspended
            .values()
            .map(|process| process_name_key(&process.process_name))
            .collect::<BTreeSet<_>>();
        let active_network_wake_names = self.active_network_wake_names(now);
        let (network_snapshot, network_event_names) = if settings.network_wake_enabled {
            match network_connection_snapshot(&network_target_processes) {
                Ok(snapshot) => {
                    let wake_names = network_process_names_with_activity(
                        &self.network_snapshot,
                        &snapshot,
                        &network_thresholds,
                    );
                    (
                        snapshot,
                        eligible_network_wake_names(
                            &wake_names,
                            &suspended_process_names,
                            &active_network_wake_names,
                        ),
                    )
                }
                Err(err) => {
                    failed_actions += 1;
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
        let network_wake_names = self.active_network_wake_names(now);
        failed_actions += self.apply_network_wake(&target_processes, &network_wake_names, now);
        self.network_snapshot = network_snapshot;
        failed_actions += self.release_for_temporary_thaw(settings, &target_ids, now);

        for (process_id, process_name) in target_processes {
            if self.suspended.contains_key(&process_id) {
                continue;
            }

            let manual_freeze = manual_freeze_names.contains(&process_name_key(&process_name));
            if manual_freeze {
                self.temporary_thawed.remove(&process_id);
                self.tracked.remove(&process_id);
            }

            match self.temporary_thaw_state(process_id, &process_name, now) {
                TemporaryThawState::Active if !manual_freeze => continue,
                TemporaryThawState::Active => {}
                TemporaryThawState::Expired => {
                    self.tracked.remove(&process_id);
                }
                TemporaryThawState::None if !manual_freeze => {
                    let tracked =
                        self.tracked
                            .entry(process_id)
                            .or_insert_with(|| TrackedProcess {
                                process_name: process_name.clone(),
                                background_since: now,
                            });
                    tracked.process_name = process_name.clone();
                    if tracked.background_since.elapsed() < delay {
                        continue;
                    }
                }
                TemporaryThawState::None => {}
            }

            match suspend_process(process_id, process_name, now) {
                Ok(suspended_process) => {
                    self.tracked.remove(&process_id);
                    self.suspended.insert(process_id, suspended_process);
                }
                Err(SuspensionError::AccessDenied | SuspensionError::NotSupported) => {
                    skipped_processes += 1;
                }
                Err(SuspensionError::Failed(err)) => {
                    failed_actions += 1;
                    if last_error.is_none() {
                        last_error = Some(err);
                    }
                }
            }
        }

        AppSuspensionSnapshot {
            enabled: true,
            tracked_processes: self.tracked.len(),
            suspended_processes: self.suspended.len(),
            temporary_thawed_processes: self.temporary_thawed.len(),
            network_wake_processes: self.network_wake_process_count(),
            tracked_apps: unique_app_names(
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
            skipped_processes,
            failed_actions,
            message: "App Suspension active.".to_owned(),
            last_error,
        }
    }

    fn release_non_targets(&mut self, target_ids: &BTreeSet<u32>) -> usize {
        let process_ids = self
            .suspended
            .keys()
            .copied()
            .filter(|process_id| !target_ids.contains(process_id))
            .collect::<Vec<_>>();

        self.release_processes(&process_ids)
    }

    fn clear_all(&mut self) -> usize {
        self.tracked.clear();
        self.temporary_thawed.clear();
        self.network_snapshot.clear();
        self.network_wake_windows.clear();
        let process_ids = self.suspended.keys().copied().collect::<Vec<_>>();
        self.release_processes(&process_ids)
    }

    fn release_processes(&mut self, process_ids: &[u32]) -> usize {
        let mut failed = 0;
        for process_id in process_ids {
            if let Some(process) = self.suspended.remove(process_id) {
                if resume_process(process).is_err() {
                    failed += 1;
                }
            }
        }
        failed
    }

    fn release_for_temporary_thaw(
        &mut self,
        settings: &AppSuspensionSettings,
        target_ids: &BTreeSet<u32>,
        now: Instant,
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
            if let Some(process) = self.suspended.remove(&process_id) {
                let process_name = process.process_name.clone();
                if resume_process(process).is_err() {
                    failed += 1;
                } else {
                    self.temporary_thawed.insert(
                        process_id,
                        TemporaryThaw {
                            process_name,
                            thaw_until: now + duration,
                            reason: TemporaryThawReason::Fallback,
                        },
                    );
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

            if let Some(process) = self.suspended.remove(&process_id) {
                if resume_process(process).is_err() {
                    failed += 1;
                    continue;
                }
            }

            self.tracked.remove(&process_id);
            self.set_temporary_thaw(
                process_id,
                process_name,
                thaw_until,
                TemporaryThawReason::NetworkWake,
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

    fn active_network_wake_names(&self, now: Instant) -> BTreeSet<String> {
        self.network_wake_windows
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

    fn network_wake_process_count(&self) -> usize {
        self.temporary_thawed
            .values()
            .filter(|process| process.reason == TemporaryThawReason::NetworkWake)
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
}

impl Drop for AppSuspensionManager {
    fn drop(&mut self) {
        self.clear_all();
    }
}

impl Default for AppSuspensionSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            tracked_processes: 0,
            suspended_processes: 0,
            temporary_thawed_processes: 0,
            network_wake_processes: 0,
            tracked_apps: Vec::new(),
            suspended_apps: Vec::new(),
            temporary_thawed_apps: Vec::new(),
            network_wake_apps: Vec::new(),
            skipped_processes: 0,
            failed_actions: 0,
            message: "App Suspension disabled.".to_owned(),
            last_error: None,
        }
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    BUILT_IN_EXCLUSIONS
        .iter()
        .any(|excluded| excluded.eq_ignore_ascii_case(process_name.trim()))
}

pub fn contains_process(list: &[String], process_name: &str) -> bool {
    list.iter()
        .any(|process| process.trim().eq_ignore_ascii_case(process_name.trim()))
}

fn network_wake_duration(settings: &AppSuspensionSettings) -> Option<Duration> {
    (settings.network_wake_enabled && settings.network_wake_duration_seconds > 0)
        .then(|| Duration::from_secs(settings.network_wake_duration_seconds))
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
    suspended_process_names: &BTreeSet<String>,
    active_network_wake_names: &BTreeSet<String>,
) -> BTreeSet<String> {
    network_process_names
        .iter()
        .filter(|process_name| {
            suspended_process_names.contains(*process_name)
                || active_network_wake_names.contains(*process_name)
        })
        .cloned()
        .collect()
}

fn process_name_key(process_name: &str) -> String {
    process_name.trim().to_ascii_lowercase()
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

    (status == NO_ERROR).then(|| NetworkActivityCounters {
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

    (status == NO_ERROR).then(|| NetworkActivityCounters {
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
            "Network wake watcher failed to size IP Helper table with error {first_status}."
        ));
    }

    if size == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u8; size as usize];
    let status = query(buffer.as_mut_ptr() as *mut c_void, &mut size);
    if status != NO_ERROR {
        return Err(format!(
            "Network wake watcher failed to read IP Helper table with error {status}."
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
    if row_size == 0 || buffer.len() < rows_offset + (count * row_size) {
        return Vec::new();
    }

    let rows_ptr = unsafe { buffer.as_ptr().add(rows_offset) as *const T };
    (0..count)
        .map(|index| unsafe { ptr::read_unaligned(rows_ptr.add(index)) })
        .collect()
}

fn unique_app_names<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut unique = BTreeMap::new();
    for name in names {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            unique
                .entry(trimmed.to_ascii_lowercase())
                .or_insert_with(|| trimmed.to_owned());
        }
    }

    unique.into_values().collect()
}

fn should_skip_foreground_process(
    process_id: u32,
    process_name: &str,
    foreground_process_id: u32,
    foreground_process_name: Option<&str>,
) -> bool {
    process_id == foreground_process_id
        || foreground_process_name
            .is_some_and(|name| name.eq_ignore_ascii_case(process_name.trim()))
}

fn process_session_id(process_id: u32) -> Option<u32> {
    let mut session_id = 0;
    let ok = unsafe { ProcessIdToSessionId(process_id, &mut session_id) };
    (ok != 0).then_some(session_id)
}

#[derive(Debug, PartialEq, Eq)]
enum SuspensionError {
    AccessDenied,
    NotSupported,
    Failed(String),
}

const JOB_OBJECT_FREEZE_INFORMATION_CLASS: i32 = 18;

#[repr(C)]
struct JobObjectFreezeInformation {
    flags: u32,
    freeze: u8,
    swap: u8,
    spare: u16,
    wake_filter_high: u32,
    wake_filter_low: u32,
}

struct ProcessFreezer {
    job_handle: HANDLE,
}

impl ProcessFreezer {
    fn freeze(process_id: u32) -> Result<Self, SuspensionError> {
        let process_handle =
            unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, process_id) };
        if process_handle.is_null() {
            return Err(open_process_error(process_id));
        }

        let job_handle = unsafe { CreateJobObjectW(null(), null()) };
        if job_handle.is_null() {
            let error = last_error();
            unsafe {
                CloseHandle(process_handle);
            }
            return Err(SuspensionError::Failed(format!(
                "CreateJobObjectW failed with error {error}."
            )));
        }

        let assigned = unsafe { AssignProcessToJobObject(job_handle, process_handle) != 0 };
        unsafe {
            CloseHandle(process_handle);
        }
        if !assigned {
            let error = last_error();
            unsafe {
                CloseHandle(job_handle);
            }
            return Err(assign_process_to_job_error(process_id, error));
        }

        let freezer = Self { job_handle };
        if let Err(err) = freezer.set_frozen(true) {
            drop(freezer);
            return Err(err);
        }

        Ok(freezer)
    }

    fn thaw(mut self) -> Result<(), SuspensionError> {
        let result = self.set_frozen(false);
        self.close();
        result
    }

    fn set_frozen(&self, frozen: bool) -> Result<(), SuspensionError> {
        let mut info = JobObjectFreezeInformation {
            flags: 1,
            freeze: u8::from(frozen),
            swap: 0,
            spare: 0,
            wake_filter_high: 0,
            wake_filter_low: 0,
        };

        let ok = unsafe {
            SetInformationJobObject(
                self.job_handle,
                JOB_OBJECT_FREEZE_INFORMATION_CLASS,
                &mut info as *mut _ as *mut c_void,
                std::mem::size_of::<JobObjectFreezeInformation>() as u32,
            )
        };

        if ok == 0 {
            Err(SuspensionError::Failed(format!(
                "SetInformationJobObject freeze={} failed with error {}.",
                frozen,
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    fn close(&mut self) {
        if !self.job_handle.is_null() {
            unsafe {
                CloseHandle(self.job_handle);
            }
            self.job_handle = null_mut_handle();
        }
    }
}

impl Drop for ProcessFreezer {
    fn drop(&mut self) {
        if !self.job_handle.is_null() {
            let _ = self.set_frozen(false);
            self.close();
        }
    }
}

fn null_mut_handle() -> HANDLE {
    std::ptr::null_mut()
}

fn open_process_error(process_id: u32) -> SuspensionError {
    let error = last_error();
    match error {
        ERROR_ACCESS_DENIED => SuspensionError::AccessDenied,
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

fn suspend_process(
    process_id: u32,
    process_name: String,
    suspended_since: Instant,
) -> Result<SuspendedProcess, SuspensionError> {
    let freezer = ProcessFreezer::freeze(process_id)?;
    Ok(SuspendedProcess {
        process_name,
        freezer,
        suspended_since,
    })
}

fn resume_process(process: SuspendedProcess) -> Result<(), SuspensionError> {
    process.freezer.thaw()
}

fn last_error() -> u32 {
    unsafe { GetLastError() }
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

    #[test]
    fn suspendable_app_match_is_case_insensitive() {
        let suspendable_apps = vec!["chat.exe".to_owned()];

        assert!(contains_process(&suspendable_apps, "CHAT.EXE"));
        assert!(!contains_process(&suspendable_apps, "browser.exe"));
    }

    #[test]
    fn foreground_skip_matches_pid_or_name() {
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
    fn assign_process_error_50_is_skipped_not_failed() {
        assert_eq!(
            assign_process_to_job_error(3252, ERROR_NOT_SUPPORTED),
            SuspensionError::NotSupported
        );
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
    fn eligible_network_wake_names_require_suspended_or_active_wake_process() {
        let network_names = BTreeSet::from([
            "chat.exe".to_owned(),
            "mail.exe".to_owned(),
            "browser.exe".to_owned(),
        ]);
        let suspended_names = BTreeSet::from(["chat.exe".to_owned()]);
        let active_wake_names = BTreeSet::from(["mail.exe".to_owned()]);

        let names =
            eligible_network_wake_names(&network_names, &suspended_names, &active_wake_names);

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
        let mut settings = AppSuspensionSettings::default();
        settings.network_wake_enabled = true;
        settings.network_wake_duration_seconds = 10;
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
    fn process_name_key_trims_and_lowercases() {
        assert_eq!(process_name_key(" Chrome.EXE "), "chrome.exe");
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
    fn built_in_exclusions_include_system_processes() {
        assert!(is_builtin_excluded("csrss.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("browser.exe"));
        assert!(!is_builtin_excluded("ms-teams.exe"));
    }
}
