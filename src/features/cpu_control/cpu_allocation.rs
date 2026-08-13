use std::{
    collections::{BTreeMap, BTreeSet},
    mem::size_of,
    path::{Path, PathBuf},
    ptr::{null_mut, read_unaligned},
    slice,
};

use windows_sys::Win32::System::{
    SystemInformation::{
        GetLogicalProcessorInformationEx, RelationProcessorCore, GROUP_AFFINITY,
        LOGICAL_PROCESSOR_RELATIONSHIP, PROCESSOR_RELATIONSHIP,
        SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    },
    Threading::{GetActiveProcessorGroupCount, GetCurrentProcessId},
};

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{CpuAllocationRule, CpuAllocationSettings},
    control::{
        cpu_allocation::{
            CpuAllocationApplyOutcome, CpuAllocationClaim, CpuAllocationCoordinator,
            CpuAllocationReconciliationSummary, CpuAllocationReleaseSummary, CpuAllocationRequest,
        },
        process::{ControlOwner, ProcessControlError, ProcessControlTarget},
    },
    features::priority_control::PriorityProcessTier,
    foreground::{
        contains_process_name, is_foreground_process, process_executable_path, process_failure_key,
        process_session_id, same_executable_path, ProtectedProcesses,
        EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::{
        execution_failure_suppression_threshold, ExecutionFailureTracker, ExecutionSuppression,
    },
    runtime::observations::CycleObservations,
};

const BUILT_IN_EXCLUSIONS: &[&str] = EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS;
const EXTRA_BUILT_IN_EXCLUSIONS: &[&str] = &["rtkauduservice64.exe"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuAllocationSnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub adjusted_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub auto_excluded_processes: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

pub(crate) struct CpuAllocationTarget {
    pub(crate) process_id: u32,
    pub(crate) process_name: String,
    pub(crate) executable_path: String,
    pub(crate) mode: CpuAllocationMode,
    pub(crate) core_mask: u64,
    pub(crate) creation_time: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalProcessorKind {
    Performance,
    Efficiency,
    Standard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalProcessorInfo {
    pub index: usize,
    pub core_index: usize,
    pub kind: LogicalProcessorKind,
    pub efficiency_class: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LogicalProcessorInformationHeader {
    relationship: LOGICAL_PROCESSOR_RELATIONSHIP,
    size: u32,
}

pub struct CpuAllocationManager {
    failure_suppression: ExecutionFailureTracker,
    action_log_feature: ActionLogFeature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CpuAllocationMode {
    SoftCpuSets,
    HardAffinity,
}

impl CpuAllocationManager {
    pub fn with_action_log_feature(action_log_feature: ActionLogFeature) -> Self {
        Self {
            failure_suppression: ExecutionFailureTracker::default(),
            action_log_feature,
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the policy boundary receives the shared runtime observations and coordinator"
    )]
    pub fn update(
        &mut self,
        coordinator: &mut CpuAllocationCoordinator,
        owner: ControlOwner,
        settings: &CpuAllocationSettings,
        allocation: (CpuAllocationMode, ActionLogFeature),
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> CpuAllocationSnapshot {
        let (mode, action_log_feature) = allocation;
        self.action_log_feature = action_log_feature;
        if !automation_enabled {
            return self.disabled_snapshot(
                coordinator,
                owner,
                false,
                "Automation disabled.",
                "automation disabled",
                action_log,
            );
        }

        if !settings.enabled {
            return self.disabled_snapshot(
                coordinator,
                owner,
                false,
                &format!("{} disabled.", self.feature_label()),
                &format!("{} disabled", self.feature_label()),
                action_log,
            );
        }

        let enabled_process_names = settings
            .rules
            .iter()
            .filter(|rule| cpu_allocation_rule_is_active(rule))
            .filter_map(|rule| Path::new(&rule.executable_path).file_name())
            .filter_map(|name| name.to_str())
            .map(str::to_ascii_lowercase)
            .collect::<BTreeSet<_>>();
        if enabled_process_names.is_empty() {
            let reason = format!("no {} rules configured", self.feature_label());
            return self.disabled_snapshot(
                coordinator,
                owner,
                true,
                &format!("No {} rules configured.", self.feature_label()),
                &reason,
                action_log,
            );
        }

        let (needs_foreground, needs_visible_windows) =
            cpu_allocation_observation_requirements(&settings.rules);
        if needs_foreground && foreground_process_id.is_none() {
            return self.paused_snapshot(
                coordinator,
                owner,
                "Paused: foreground app is unknown.".to_owned(),
                "foreground app is unknown",
                action_log,
            );
        }

        let visible_window_process_ids = if needs_visible_windows {
            let Ok(process_ids) = observations.visible_window_process_ids() else {
                return self.paused_snapshot(
                    coordinator,
                    owner,
                    "Paused: visible windows are unavailable.".to_owned(),
                    "visible windows are unavailable",
                    action_log,
                );
            };
            process_ids
        } else {
            Default::default()
        };

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            return self.paused_snapshot(
                coordinator,
                owner,
                "Paused: current Windows session is unknown.".to_owned(),
                "current Windows session is unknown",
                action_log,
            );
        };

        let processes = match observations.processes() {
            Ok(processes) => processes,
            Err(err) => {
                let mut snapshot = self.paused_snapshot(
                    coordinator,
                    owner,
                    err.clone(),
                    "process list unavailable",
                    action_log,
                );
                snapshot.last_error.get_or_insert(err);
                return snapshot;
            }
        };

        let scanned_processes = processes.len();
        let foreground_executable_path = foreground_process_id.and_then(|id| {
            processes
                .iter()
                .find(|process| process.id == id)
                .and_then(process_executable_path)
        });
        let visible_processes = ProtectedProcesses::capture(
            processes.as_ref(),
            false,
            None,
            visible_window_process_ids,
        );
        let mut targets = Vec::new();
        for process in processes.iter() {
            if process.id == 0
                || process.is_critical != Some(false)
                || !process.can_set_information
                || process.id == current_process_id
                || is_builtin_excluded(&process.name)
                || !enabled_process_names.contains(&process.name.to_ascii_lowercase())
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
            let Some(creation_time) = process.creation_time else {
                continue;
            };

            if let Some(rule) = matching_rule(&settings.rules, &executable_path) {
                let foreground = is_foreground_process(
                    process.id,
                    &executable_path,
                    foreground_process_id,
                    foreground_executable_path.as_deref(),
                );
                let visible_window =
                    !foreground && visible_processes.contains(process.id, &executable_path);
                let core_mask = cpu_allocation_rule_core_mask(rule, foreground, visible_window);
                if core_mask == 0 {
                    continue;
                }
                targets.push(CpuAllocationTarget {
                    process_id: process.id,
                    process_name: process.name.clone(),
                    executable_path: executable_path.to_string_lossy().into_owned(),
                    mode,
                    core_mask,
                    creation_time,
                });
            }
        }

        self.apply_targets(
            coordinator,
            owner,
            targets,
            scanned_processes,
            cpu_allocation_message(mode),
            allow_cross_session_process_control,
            action_log,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Workload Engine supplies already-discovered exact process targets"
    )]
    pub(crate) fn update_discovered_targets(
        &mut self,
        coordinator: &mut CpuAllocationCoordinator,
        owner: ControlOwner,
        targets: Vec<CpuAllocationTarget>,
        scanned_processes: usize,
        message: &str,
        allow_cross_session_process_control: bool,
        action_log: &mut ActionLog,
    ) -> CpuAllocationSnapshot {
        self.apply_targets(
            coordinator,
            owner,
            targets,
            scanned_processes,
            message.to_owned(),
            allow_cross_session_process_control,
            action_log,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the controller call keeps policy owner, access, and reporting context explicit"
    )]
    fn apply_targets(
        &mut self,
        coordinator: &mut CpuAllocationCoordinator,
        owner: ControlOwner,
        targets: Vec<CpuAllocationTarget>,
        scanned_processes: usize,
        message: String,
        allow_cross_session_process_control: bool,
        action_log: &mut ActionLog,
    ) -> CpuAllocationSnapshot {
        let active_targets = targets
            .iter()
            .map(cpu_allocation_target_key)
            .collect::<BTreeSet<_>>();
        let active_target_names = targets
            .iter()
            .map(|target| process_failure_key(&target.executable_path))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);

        let mut failures = CpuAllocationFailures::default();
        self.merge_release_summary(
            coordinator.release_policy_except(owner, &active_targets),
            owner,
            action_log,
            &format!("process no longer matches a {} rule", self.feature_label()),
            &mut failures,
        );
        let mut skipped_processes = 0;
        let mut auto_excluded_processes = BTreeSet::new();

        for target in targets {
            let failure_process_name = target.process_name.clone();
            let failure_executable_path = target.executable_path.clone();
            let suppression = self.check_process_suppression(
                target.process_id,
                &failure_process_name,
                &failure_executable_path,
                action_log,
            );
            if suppression.suppressed {
                skipped_processes += 1;
                if suppression.newly_suppressed {
                    auto_excluded_processes.insert(failure_executable_path.clone());
                }
                continue;
            }

            let request = match target.mode {
                CpuAllocationMode::SoftCpuSets => CpuAllocationRequest::SoftCpuSets {
                    logical_processor_mask: target.core_mask,
                },
                CpuAllocationMode::HardAffinity => CpuAllocationRequest::HardAffinity {
                    logical_processor_mask: target.core_mask,
                },
            };
            let claim = CpuAllocationClaim {
                target: ProcessControlTarget::automatic(
                    target.process_id,
                    target.process_name.clone(),
                    PathBuf::from(&target.executable_path),
                    target.creation_time,
                ),
                owner,
                request,
            };
            match coordinator.apply_policy_claim(claim, allow_cross_session_process_control) {
                Ok(CpuAllocationApplyOutcome::Applied) => {
                    self.clear_process_failure(&failure_executable_path);
                    action_log.record(
                        self.action_log_feature,
                        Some(target.process_id),
                        target.process_name,
                        ActionLogResult::Applied,
                        format!("Applied {}.", cpu_allocation_mode_label(target.mode)),
                    );
                }
                Ok(CpuAllocationApplyOutcome::Unchanged) => {
                    self.clear_process_failure(&failure_executable_path);
                }
                Ok(
                    CpuAllocationApplyOutcome::Shadowed | CpuAllocationApplyOutcome::NoUsableTarget,
                ) => {
                    skipped_processes += 1;
                    self.clear_process_failure(&failure_executable_path);
                }
                Err(ProcessControlError::ProcessExited) => {
                    skipped_processes += 1;
                }
                Err(ProcessControlError::AccessDenied(message)) => {
                    skipped_processes += 1;
                    self.failure_suppression
                        .suppress_process_failure(&failure_executable_path);
                    action_log.record(
                        self.action_log_feature,
                        Some(target.process_id),
                        failure_process_name,
                        ActionLogResult::Skipped,
                        message,
                    );
                }
                Err(error) => {
                    self.record_process_failure(&failure_executable_path);
                    failures.record(
                        "Apply",
                        target.process_id,
                        &failure_process_name,
                        error,
                        self.action_log_feature,
                        action_log,
                    );
                }
            }
        }

        CpuAllocationSnapshot {
            enabled: true,
            scanned_processes,
            adjusted_processes: coordinator.policy_managed_process_count(owner),
            skipped_processes,
            failed_processes: failures.count,
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            message,
            last_error: failures.last_error,
        }
    }

    fn disabled_snapshot(
        &mut self,
        coordinator: &mut CpuAllocationCoordinator,
        owner: ControlOwner,
        enabled: bool,
        message: &str,
        reason: &str,
        action_log: &mut ActionLog,
    ) -> CpuAllocationSnapshot {
        let mut failures = CpuAllocationFailures::default();
        self.merge_release_summary(
            coordinator.release_all_policy(owner),
            owner,
            action_log,
            reason,
            &mut failures,
        );
        self.failure_suppression.clear();
        CpuAllocationSnapshot {
            enabled,
            failed_processes: failures.count,
            message: message.to_owned(),
            last_error: failures.last_error,
            ..Default::default()
        }
    }

    fn paused_snapshot(
        &mut self,
        coordinator: &mut CpuAllocationCoordinator,
        owner: ControlOwner,
        message: String,
        reason: &str,
        action_log: &mut ActionLog,
    ) -> CpuAllocationSnapshot {
        let mut snapshot =
            self.disabled_snapshot(coordinator, owner, true, &message, reason, action_log);
        snapshot.enabled = true;
        snapshot
    }

    fn merge_release_summary(
        &mut self,
        summary: CpuAllocationReleaseSummary,
        owner: ControlOwner,
        action_log: &mut ActionLog,
        reason: &str,
        failures: &mut CpuAllocationFailures,
    ) {
        record_cpu_allocation_restorations(summary.restored_owners, reason, action_log);
        for failure in summary.failures {
            let (action_log_feature, _) = cpu_allocation_action_log_context(failure.owner);
            let message = failure.error.to_string();
            if failure.owner == owner {
                failures.note(
                    "Restore",
                    failure.process_id,
                    &failure.process_name,
                    &message,
                );
            }
            action_log.record(
                action_log_feature,
                Some(failure.process_id),
                failure.process_name,
                ActionLogResult::Failed,
                message,
            );
        }
    }

    fn check_process_suppression(
        &mut self,
        process_id: u32,
        process_name: &str,
        executable_path: &str,
        action_log: &mut ActionLog,
    ) -> ExecutionSuppression {
        let suppression = self
            .failure_suppression
            .process_suppression(executable_path);
        if suppression.newly_suppressed {
            action_log.record(
                self.action_log_feature,
                Some(process_id),
                process_name.to_owned(),
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying {} after {} failed attempts.",
                    self.feature_label(),
                    execution_failure_suppression_threshold(),
                ),
            );
        }

        suppression
    }

    #[cfg(test)]
    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log: &mut ActionLog,
    ) -> bool {
        self.check_process_suppression(process_id, process_name, process_name, action_log)
            .suppressed
    }

    fn record_process_failure(&mut self, process_name: &str) {
        self.failure_suppression
            .record_process_failure(process_name);
    }

    fn clear_process_failure(&mut self, process_name: &str) {
        self.failure_suppression.clear_process_failure(process_name);
    }

    fn feature_label(&self) -> &'static str {
        match self.action_log_feature {
            ActionLogFeature::CpuSetsSoft => "CPU Sets (Soft)",
            ActionLogFeature::ProcessorAffinityHard => "Processor Affinity (Hard)",
            _ => "CPU allocation",
        }
    }
}

pub(crate) fn record_cpu_allocation_restorations(
    restored_owners: Vec<ControlOwner>,
    reason: &str,
    action_log: &mut ActionLog,
) {
    let mut counts = BTreeMap::new();
    for owner in restored_owners {
        *counts.entry(owner).or_insert(0_usize) += 1;
    }
    for (owner, count) in counts {
        let (feature, label) = cpu_allocation_action_log_context(owner);
        action_log.record(
            feature,
            None,
            label,
            ActionLogResult::Restored,
            format!(
                "Restored {count} CPU allocation {}: {reason}.",
                if count == 1 { "property" } else { "properties" }
            ),
        );
    }
}

pub(crate) fn record_cpu_allocation_reconciliation(
    summary: CpuAllocationReconciliationSummary,
    action_log: &mut ActionLog,
) {
    for application in summary.applications {
        let (feature, _) = cpu_allocation_action_log_context(application.owner);
        action_log.record(
            feature,
            Some(application.process_id),
            application.process_name,
            ActionLogResult::Applied,
            format!(
                "Applied {} after CPU allocation precedence changed.",
                cpu_allocation_request_label(application.request)
            ),
        );
    }
    for failure in summary.failures {
        let (feature, _) = cpu_allocation_action_log_context(failure.owner);
        action_log.record(
            feature,
            Some(failure.process_id),
            failure.process_name,
            ActionLogResult::Failed,
            failure.error.to_string(),
        );
    }
    record_cpu_allocation_restorations(
        summary.releases.restored_owners,
        "CPU allocation precedence changed",
        action_log,
    );
    for failure in summary.releases.failures {
        let (feature, _) = cpu_allocation_action_log_context(failure.owner);
        action_log.record(
            feature,
            Some(failure.process_id),
            failure.process_name,
            ActionLogResult::Failed,
            failure.error.to_string(),
        );
    }
}

pub(crate) fn cpu_allocation_action_log_context(
    owner: ControlOwner,
) -> (ActionLogFeature, &'static str) {
    match owner {
        ControlOwner::CpuSetsSoft => (ActionLogFeature::CpuSetsSoft, "CPU Sets (Soft)"),
        ControlOwner::ProcessorAffinityHard => (
            ActionLogFeature::ProcessorAffinityHard,
            "Processor Affinity (Hard)",
        ),
        ControlOwner::CoreLimiter => (ActionLogFeature::CoreLimiter, "Core Limiter"),
        ControlOwner::AdaptiveEngine => (ActionLogFeature::WorkloadEngine, "Workload Engine"),
        unsupported => {
            unreachable!("unsupported CPU allocation Action Log owner: {unsupported:?}")
        }
    }
}

impl Default for CpuAllocationManager {
    fn default() -> Self {
        Self {
            failure_suppression: ExecutionFailureTracker::default(),
            action_log_feature: ActionLogFeature::CpuSetsSoft,
        }
    }
}

#[derive(Default)]
struct CpuAllocationFailures {
    count: usize,
    last_error: Option<String>,
}

impl CpuAllocationFailures {
    fn record(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: ProcessControlError,
        action_log_feature: ActionLogFeature,
        action_log: &mut ActionLog,
    ) {
        let message = error.to_string();
        self.note(action, process_id, process_name, &message);
        action_log.record(
            action_log_feature,
            Some(process_id),
            process_name.to_owned(),
            ActionLogResult::Failed,
            message,
        );
    }

    fn note(&mut self, action: &str, process_id: u32, process_name: &str, message: &str) {
        self.last_error
            .get_or_insert_with(|| format!("{action} {process_name} ({process_id}): {message}"));
        self.count += 1;
    }
}

fn cpu_allocation_target_key(
    target: &CpuAllocationTarget,
) -> crate::control::process::ProcessTargetKey {
    ProcessControlTarget::automatic(
        target.process_id,
        target.process_name.clone(),
        PathBuf::from(&target.executable_path),
        target.creation_time,
    )
    .key()
}

fn cpu_allocation_mode_label(mode: CpuAllocationMode) -> &'static str {
    match mode {
        CpuAllocationMode::SoftCpuSets => "CPU Sets (Soft)",
        CpuAllocationMode::HardAffinity => "Processor Affinity (Hard)",
    }
}

fn cpu_allocation_request_label(request: CpuAllocationRequest) -> &'static str {
    match request {
        CpuAllocationRequest::SoftCpuSets { .. } => "CPU Sets (Soft)",
        CpuAllocationRequest::HardAffinity { .. } => "Processor Affinity (Hard)",
        CpuAllocationRequest::LimitLogicalProcessors { .. } => "Core Limiter",
    }
}

impl Default for CpuAllocationSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            scanned_processes: 0,
            adjusted_processes: 0,
            skipped_processes: 0,
            failed_processes: 0,
            auto_excluded_processes: Vec::new(),
            message: "CPU allocation disabled.".to_owned(),
            last_error: None,
        }
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    let process_name = std::path::Path::new(process_name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(process_name);
    contains_process_name(BUILT_IN_EXCLUSIONS, process_name)
        || contains_process_name(EXTRA_BUILT_IN_EXCLUSIONS, process_name)
}

pub fn contains_process(list: &[String], executable_path: &str) -> bool {
    list.iter().any(|path| {
        crate::foreground::same_executable_path(
            std::path::Path::new(path),
            std::path::Path::new(executable_path),
        )
    })
}

pub fn logical_processors() -> Vec<LogicalProcessorInfo> {
    logical_processors_from_topology().unwrap_or_else(fallback_logical_processors)
}

pub fn default_cpu_mask() -> u64 {
    let processors = logical_processors();
    let mask = processors
        .iter()
        .filter_map(|processor| (processor.index < 64).then_some(1_u64 << processor.index))
        .fold(0, |mask, bit| mask | bit);
    if mask != 0 {
        return mask;
    }

    let processor_count = processors.len().clamp(1, 64);
    if processor_count == 64 {
        u64::MAX
    } else {
        (1_u64 << processor_count) - 1
    }
}

pub fn has_multiple_processor_groups() -> bool {
    active_processor_group_count() > 1
}

fn cpu_allocation_message(mode: CpuAllocationMode) -> String {
    match mode {
        CpuAllocationMode::SoftCpuSets => "CPU Sets (Soft) active.".to_owned(),
        CpuAllocationMode::HardAffinity => {
            processor_affinity_hard_message_for_group_count(active_processor_group_count())
        }
    }
}

fn processor_affinity_hard_message_for_group_count(group_count: u16) -> String {
    if group_count > 1 {
        "Processor Affinity (Hard) active. Multi-group CPU detected: affinity can only control CPUs in the process primary processor group. Apps that are not processor-group-aware may not use the full CPU."
            .to_owned()
    } else {
        "Processor Affinity (Hard) active.".to_owned()
    }
}

fn active_processor_group_count() -> u16 {
    // SAFETY: GetActiveProcessorGroupCount takes no arguments and has no caller requirements.
    unsafe { GetActiveProcessorGroupCount() }
}

fn logical_processors_from_topology() -> Option<Vec<LogicalProcessorInfo>> {
    let mut returned_length = 0;
    // SAFETY: A null buffer with zero length requests the required byte count in returned_length.
    unsafe {
        GetLogicalProcessorInformationEx(RelationProcessorCore, null_mut(), &mut returned_length);
    }

    if returned_length == 0 {
        return None;
    }

    let word_count = (returned_length as usize).div_ceil(size_of::<usize>());
    let mut buffer = vec![0_usize; word_count];
    // SAFETY: buffer provides at least returned_length writable bytes and returned_length remains
    // writable for the actual result size.
    let ok = unsafe {
        GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            buffer.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
            &mut returned_length,
        )
    };
    if ok == 0 || returned_length == 0 {
        return None;
    }

    // SAFETY: The successful topology query initialized returned_length bytes within buffer.
    let bytes =
        unsafe { slice::from_raw_parts(buffer.as_ptr() as *const u8, returned_length as usize) };

    logical_processors_from_topology_bytes(bytes)
}

fn logical_processors_from_topology_bytes(buffer: &[u8]) -> Option<Vec<LogicalProcessorInfo>> {
    let mut processors = Vec::new();
    let mut core_index = 0;
    let mut offset = 0;
    let header_size = size_of::<LogicalProcessorInformationHeader>();
    let processor_size = size_of::<PROCESSOR_RELATIONSHIP>();
    let group_mask_offset = header_size + std::mem::offset_of!(PROCESSOR_RELATIONSHIP, GroupMask);
    let group_mask_size = size_of::<GROUP_AFFINITY>();

    while offset + header_size <= buffer.len() {
        // SAFETY: The header bounds check above guarantees enough bytes; unaligned reads are used
        // because the Win32 records are byte-packed.
        let header = unsafe {
            read_unaligned(buffer.as_ptr().add(offset) as *const LogicalProcessorInformationHeader)
        };
        let record_size = header.size as usize;
        if record_size < header_size || offset + record_size > buffer.len() {
            break;
        }

        if header.relationship == RelationProcessorCore
            && record_size >= header_size + processor_size
        {
            // SAFETY: record_size was validated against the buffer and includes a complete
            // PROCESSOR_RELATIONSHIP.
            let processor = unsafe {
                read_unaligned(
                    buffer.as_ptr().add(offset + header_size) as *const PROCESSOR_RELATIONSHIP
                )
            };
            let available_group_count =
                record_size.saturating_sub(group_mask_offset) / group_mask_size;
            let group_count = usize::from(processor.GroupCount).min(available_group_count);
            for group_index in 0..group_count {
                // SAFETY: group_count is capped by the number of complete GROUP_AFFINITY records
                // inside this validated record.
                let group_affinity = unsafe {
                    read_unaligned(
                        buffer
                            .as_ptr()
                            .add(offset + group_mask_offset + group_index * group_mask_size)
                            as *const GROUP_AFFINITY,
                    )
                };
                if group_affinity.Group != 0 {
                    continue;
                }

                for bit in 0..usize::BITS as usize {
                    if (group_affinity.Mask & (1_usize << bit)) != 0 && bit < 64 {
                        processors.push(LogicalProcessorInfo {
                            index: bit,
                            core_index,
                            kind: LogicalProcessorKind::Standard,
                            efficiency_class: processor.EfficiencyClass,
                        });
                    }
                }
            }
            core_index += 1;
        }

        offset += record_size;
    }

    if processors.is_empty() {
        None
    } else {
        classify_logical_processors(&mut processors);
        processors.sort_by_key(|processor| processor.index);
        processors.dedup_by_key(|processor| processor.index);
        Some(processors)
    }
}

fn fallback_logical_processors() -> Vec<LogicalProcessorInfo> {
    let count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .clamp(1, 64);

    (0..count)
        .map(|index| LogicalProcessorInfo {
            index,
            core_index: index,
            kind: LogicalProcessorKind::Standard,
            efficiency_class: 0,
        })
        .collect()
}

fn classify_logical_processors(processors: &mut [LogicalProcessorInfo]) {
    let Some(min_efficiency_class) = processors
        .iter()
        .map(|processor| processor.efficiency_class)
        .min()
    else {
        return;
    };
    let max_efficiency_class = processors
        .iter()
        .map(|processor| processor.efficiency_class)
        .max()
        .unwrap_or(min_efficiency_class);

    for processor in processors {
        processor.kind = processor_kind(
            processor.efficiency_class,
            min_efficiency_class,
            max_efficiency_class,
        );
    }
}

fn processor_kind(
    efficiency_class: u8,
    min_efficiency_class: u8,
    max_efficiency_class: u8,
) -> LogicalProcessorKind {
    if min_efficiency_class == max_efficiency_class {
        LogicalProcessorKind::Standard
    } else if efficiency_class == max_efficiency_class {
        LogicalProcessorKind::Performance
    } else if efficiency_class == min_efficiency_class {
        LogicalProcessorKind::Efficiency
    } else {
        LogicalProcessorKind::Standard
    }
}

fn matching_rule<'a>(
    rules: &'a [CpuAllocationRule],
    executable_path: &Path,
) -> Option<&'a CpuAllocationRule> {
    rules.iter().find(|rule| {
        cpu_allocation_rule_is_active(rule)
            && same_executable_path(Path::new(&rule.executable_path), executable_path)
    })
}

fn cpu_allocation_rule_is_active(rule: &CpuAllocationRule) -> bool {
    rule.enabled && rule.has_cpu_selection() && Path::new(rule.executable_path.trim()).is_absolute()
}

fn cpu_allocation_observation_requirements(rules: &[CpuAllocationRule]) -> (bool, bool) {
    let mut needs_foreground = false;
    let mut needs_visible_windows = false;
    for rule in rules
        .iter()
        .filter(|rule| cpu_allocation_rule_is_active(rule))
    {
        needs_foreground |= rule.focus_core_mask != rule.visible_window_core_mask;
        needs_visible_windows |= rule.visible_window_core_mask != rule.background_core_mask;
    }
    (needs_foreground, needs_visible_windows)
}

fn cpu_allocation_rule_core_mask(
    rule: &CpuAllocationRule,
    foreground: bool,
    visible_window: bool,
) -> u64 {
    PriorityProcessTier::from_flags(foreground, visible_window).select(
        rule.focus_core_mask,
        rule.visible_window_core_mask,
        rule.background_core_mask,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_match_is_case_insensitive_and_ignores_disabled_or_empty_masks() {
        let rules = vec![
            CpuAllocationRule {
                enabled: false,
                executable_path: r"C:\Apps\Browser\browser.exe".to_owned(),
                focus_core_mask: 1,
                visible_window_core_mask: 1,
                background_core_mask: 1,
            },
            CpuAllocationRule {
                enabled: true,
                executable_path: r"C:\Apps\Backup\backup.exe".to_owned(),
                focus_core_mask: 0,
                visible_window_core_mask: 0,
                background_core_mask: 0,
            },
            CpuAllocationRule {
                enabled: true,
                executable_path: r" C:\Apps\Worker\Worker.EXE ".to_owned(),
                focus_core_mask: 0b001,
                visible_window_core_mask: 0b010,
                background_core_mask: 0b100,
            },
        ];

        assert!(matching_rule(&rules, Path::new(r"c:\apps\worker\worker.exe")).is_some());
        assert!(matching_rule(&rules, Path::new(r"C:\Apps\Browser\browser.exe")).is_none());
        assert!(matching_rule(&rules, Path::new(r"C:\Apps\Backup\backup.exe")).is_none());
        assert!(matching_rule(&rules, Path::new(r"D:\Other\worker.exe")).is_none());
    }

    #[test]
    fn cpu_allocation_rule_prefers_focus_then_visible_window_then_background() {
        let rule = CpuAllocationRule {
            enabled: true,
            executable_path: r"C:\Apps\Worker\worker.exe".to_owned(),
            focus_core_mask: 0b001,
            visible_window_core_mask: 0b010,
            background_core_mask: 0b100,
        };

        assert_eq!(cpu_allocation_rule_core_mask(&rule, true, true), 0b001);
        assert_eq!(cpu_allocation_rule_core_mask(&rule, false, true), 0b010);
        assert_eq!(cpu_allocation_rule_core_mask(&rule, false, false), 0b100);
    }

    #[test]
    fn cpu_allocation_observations_are_required_only_when_tiers_differ() {
        for (focus, visible, background, expected) in [
            (1, 1, 1, (false, false)),
            (2, 1, 1, (true, false)),
            (1, 1, 2, (false, true)),
            (2, 1, 2, (true, true)),
        ] {
            let rule = CpuAllocationRule {
                enabled: true,
                executable_path: r"C:\Apps\Worker\worker.exe".to_owned(),
                focus_core_mask: focus,
                visible_window_core_mask: visible,
                background_core_mask: background,
            };
            assert_eq!(cpu_allocation_observation_requirements(&[rule]), expected);
        }
    }

    #[test]
    fn builtin_exclusions_cover_sensitive_windows_shell_processes() {
        for process_name in [
            "explorer.exe",
            "RtkAudUService64.exe",
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
    fn repeated_failures_suppress_future_cpu_allocation_attempts_once() {
        let mut manager = CpuAllocationManager::default();
        let mut log = ActionLog::new(8);
        let executable_path = r"C:\Apps\app.exe";

        manager.record_process_failure(executable_path);
        manager.record_process_failure(executable_path);
        assert!(
            !manager
                .check_process_suppression(42, "app.exe", executable_path, &mut log)
                .suppressed
        );
        assert!(log.entries().is_empty());

        manager.record_process_failure(executable_path);
        assert!(
            manager
                .check_process_suppression(42, "app.exe", executable_path, &mut log)
                .suppressed
        );
        assert!(
            manager
                .check_process_suppression(43, "app.exe", r"C:/Apps/app.exe", &mut log)
                .suppressed
        );

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].process_name, "app.exe");
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
    }

    #[test]
    fn configured_action_log_feature_is_used_for_suppression_entries() {
        let mut manager =
            CpuAllocationManager::with_action_log_feature(ActionLogFeature::ProcessorAffinityHard);
        let mut log = ActionLog::new(8);

        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");
        assert!(manager.is_process_suppressed(42, "app.exe", &mut log));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].feature, ActionLogFeature::ProcessorAffinityHard);
        assert!(entries[0].reason.contains("Processor Affinity (Hard)"));
    }

    #[test]
    fn restorations_are_logged_against_the_actual_managed_owner() {
        let mut log = ActionLog::new(8);

        record_cpu_allocation_restorations(
            vec![
                ControlOwner::CpuSetsSoft,
                ControlOwner::AdaptiveEngine,
                ControlOwner::CpuSetsSoft,
            ],
            "settings changed",
            &mut log,
        );

        let entries = log.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].feature, ActionLogFeature::CpuSetsSoft);
        assert!(entries[0].reason.contains("2 CPU allocation properties"));
        assert_eq!(entries[1].feature, ActionLogFeature::WorkloadEngine);
        assert!(entries[1].reason.contains("1 CPU allocation property"));
    }

    #[test]
    fn cross_owner_release_failure_is_not_charged_to_the_releasing_status() {
        let mut manager =
            CpuAllocationManager::with_action_log_feature(ActionLogFeature::WorkloadEngine);
        let mut failures = CpuAllocationFailures::default();
        let mut log = ActionLog::new(8);

        manager.merge_release_summary(
            CpuAllocationReleaseSummary {
                restored_owners: Vec::new(),
                failures: vec![
                    crate::control::cpu_allocation::CpuAllocationReleaseFailure {
                        owner: ControlOwner::CpuSetsSoft,
                        process_id: 42,
                        process_name: "worker.exe".to_owned(),
                        property: "CPU Sets (Soft)",
                        error: ProcessControlError::Failed(
                            "injected restoration failure".to_owned(),
                        ),
                    },
                ],
            },
            ControlOwner::AdaptiveEngine,
            &mut log,
            "settings changed",
            &mut failures,
        );

        assert_eq!(failures.count, 0);
        assert!(failures.last_error.is_none());
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].feature, ActionLogFeature::CpuSetsSoft);
        assert_eq!(entries[0].result, ActionLogResult::Failed);
    }

    #[test]
    fn pending_handoff_application_uses_the_effective_owner_log() {
        let mut log = ActionLog::new(8);

        record_cpu_allocation_reconciliation(
            CpuAllocationReconciliationSummary {
                applications: vec![
                    crate::control::cpu_allocation::CpuAllocationReconciledApplication {
                        owner: ControlOwner::CoreLimiter,
                        process_id: 42,
                        process_name: "worker.exe".to_owned(),
                        request: CpuAllocationRequest::LimitLogicalProcessors { maximum: 2 },
                    },
                ],
                ..Default::default()
            },
            &mut log,
        );

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].feature, ActionLogFeature::CoreLimiter);
        assert_eq!(entries[0].result, ActionLogResult::Applied);
        assert!(entries[0].reason.contains("Core Limiter"));
    }

    #[test]
    fn first_cpu_allocation_suppression_reports_auto_exclusion_once() {
        let mut manager =
            CpuAllocationManager::with_action_log_feature(ActionLogFeature::CpuSetsSoft);
        let mut log = ActionLog::new(8);

        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");

        let first = manager.check_process_suppression(42, "app.exe", "app.exe", &mut log);
        let second = manager.check_process_suppression(42, "app.exe", "app.exe", &mut log);

        assert!(first.suppressed);
        assert!(first.newly_suppressed);
        assert!(second.suppressed);
        assert!(!second.newly_suppressed);
    }

    #[test]
    fn processor_affinity_hard_message_warns_on_multiple_processor_groups() {
        assert_eq!(
            processor_affinity_hard_message_for_group_count(1),
            "Processor Affinity (Hard) active."
        );
        let message = processor_affinity_hard_message_for_group_count(2);
        assert!(message.contains("Multi-group CPU detected"));
        assert!(message.contains("processor-group-aware"));
    }

    #[test]
    fn built_in_exclusions_include_system_processes() {
        assert!(is_builtin_excluded("csrss.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("browser.exe"));
    }

    #[test]
    fn homogeneous_topology_is_standard() {
        let mut processors = vec![
            LogicalProcessorInfo {
                index: 0,
                core_index: 0,
                kind: LogicalProcessorKind::Performance,
                efficiency_class: 0,
            },
            LogicalProcessorInfo {
                index: 1,
                core_index: 1,
                kind: LogicalProcessorKind::Efficiency,
                efficiency_class: 0,
            },
        ];

        classify_logical_processors(&mut processors);

        assert!(processors
            .iter()
            .all(|processor| processor.kind == LogicalProcessorKind::Standard));
    }

    #[test]
    fn hybrid_topology_classifies_min_and_max_efficiency_classes() {
        assert_eq!(processor_kind(0, 0, 1), LogicalProcessorKind::Efficiency);
        assert_eq!(processor_kind(1, 0, 1), LogicalProcessorKind::Performance);
        assert_eq!(processor_kind(2, 0, 3), LogicalProcessorKind::Standard);
    }

    #[test]
    fn topology_parser_reads_final_minimal_processor_record() {
        let mut buffer = Vec::new();
        append_processor_record(&mut buffer, 1, 1);
        append_processor_record(&mut buffer, 1_usize << 11, 0);

        let processors = logical_processors_from_topology_bytes(&buffer).unwrap();

        assert_eq!(
            processors
                .iter()
                .map(|processor| processor.index)
                .collect::<Vec<_>>(),
            vec![0, 11]
        );
        assert_eq!(processors[0].kind, LogicalProcessorKind::Performance);
        assert_eq!(processors[1].kind, LogicalProcessorKind::Efficiency);
    }

    fn append_processor_record(buffer: &mut Vec<u8>, mask: usize, efficiency_class: u8) {
        let header_size = size_of::<LogicalProcessorInformationHeader>();
        let processor_size = size_of::<PROCESSOR_RELATIONSHIP>();
        let record_size = header_size + processor_size;
        let start = buffer.len();
        buffer.resize(start + record_size, 0);

        let header = LogicalProcessorInformationHeader {
            relationship: RelationProcessorCore,
            size: record_size as u32,
        };
        let processor = PROCESSOR_RELATIONSHIP {
            EfficiencyClass: efficiency_class,
            GroupCount: 1,
            GroupMask: [GROUP_AFFINITY {
                Mask: mask,
                Group: 0,
                Reserved: [0; 3],
            }],
            ..Default::default()
        };

        // SAFETY: buffer was allocated for the complete header and processor records, and
        // write_unaligned matches their byte-packed representation.
        unsafe {
            std::ptr::write_unaligned(
                buffer.as_mut_ptr().add(start) as *mut LogicalProcessorInformationHeader,
                header,
            );
            std::ptr::write_unaligned(
                buffer.as_mut_ptr().add(start + header_size) as *mut PROCESSOR_RELATIONSHIP,
                processor,
            );
        }
    }
}
