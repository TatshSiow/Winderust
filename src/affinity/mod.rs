use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    mem::size_of,
    ptr::{null_mut, read_unaligned},
    slice,
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED, HANDLE},
    System::{
        RemoteDesktop::ProcessIdToSessionId,
        SystemInformation::{
            GetLogicalProcessorInformationEx, GetSystemCpuSetInformation, RelationProcessorCore,
            GROUP_AFFINITY, LOGICAL_PROCESSOR_RELATIONSHIP, PROCESSOR_RELATIONSHIP,
            SYSTEM_CPU_SET_INFORMATION, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
        },
        Threading::{
            GetActiveProcessorGroupCount, GetCurrentProcessId, GetProcessAffinityMask,
            GetProcessDefaultCpuSets, GetProcessInformation, OpenProcess, ProcessPowerThrottling,
            SetProcessAffinityMask, SetProcessDefaultCpuSets, SetProcessInformation,
            PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_INFORMATION,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
        },
    },
};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    config::{CpuAffinityMode, CpuAffinityRule, CpuAffinitySettings},
    foreground::list_processes,
};

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
    "rtkauduservice64.exe",
    "searchapp.exe",
    "searchhost.exe",
    "securityhealthservice.exe",
    "securityhealthsystray.exe",
    "services.exe",
    "shellexperiencehost.exe",
    "sihost.exe",
    "smss.exe",
    "startmenuexperiencehost.exe",
    "systemsettings.exe",
    "system",
    "taskmgr.exe",
    "textinputhost.exe",
    "wininit.exe",
    "winlogon.exe",
    "wudfhost.exe",
];
const FAILURE_SUPPRESSION_THRESHOLD: u8 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuAffinitySnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub adjusted_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub auto_excluded_processes: Vec<String>,
    pub adjusted_apps: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
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

#[repr(C)]
#[derive(Clone, Copy)]
struct CpuSetInformationHeader {
    size: u32,
    cpu_set_type: u32,
}

pub struct CpuAffinityManager {
    adjusted: BTreeMap<u32, AdjustedProcess>,
    failure_suppression: BTreeMap<String, CpuAffinityFailureSuppression>,
    action_log_feature: ActionLogFeature,
}

#[derive(Clone)]
struct AdjustedProcess {
    process_name: String,
    adjustment: AffinityAdjustment,
}

#[derive(Default)]
struct CpuAffinityFailureSuppression {
    attempts: u8,
    suppression_logged: bool,
}

#[derive(Clone)]
enum AffinityAdjustment {
    Hard {
        previous_affinity: usize,
        applied_affinity: usize,
    },
    Soft {
        previous_cpu_set_ids: Vec<u32>,
        applied_cpu_set_ids: Vec<u32>,
    },
    EfficiencyOff {
        previous_state: Option<PROCESS_POWER_THROTTLING_STATE>,
    },
}

impl CpuAffinityManager {
    pub fn with_action_log_feature(action_log_feature: ActionLogFeature) -> Self {
        Self {
            adjusted: BTreeMap::new(),
            failure_suppression: BTreeMap::new(),
            action_log_feature,
        }
    }

    pub fn adjusted_process_ids(&self) -> BTreeSet<u32> {
        self.adjusted.keys().copied().collect()
    }

    pub fn update(
        &mut self,
        settings: &CpuAffinitySettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        action_log: &mut ActionLog,
    ) -> CpuAffinitySnapshot {
        if !automation_enabled {
            let failed = self.clear_all(action_log, "automation disabled");
            self.failure_suppression.clear();
            return CpuAffinitySnapshot {
                enabled: false,
                failed_processes: failed,
                message: "Automation disabled.".to_owned(),
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failed = self.clear_all(action_log, &format!("{} disabled", self.feature_label()));
            self.failure_suppression.clear();
            return CpuAffinitySnapshot {
                enabled: false,
                failed_processes: failed,
                message: format!("{} disabled.", self.feature_label()),
                ..Default::default()
            };
        }

        if settings.exclude_foreground_app && foreground_process_id.is_none() {
            let failed = self.clear_all(action_log, "foreground app is unknown");
            return CpuAffinitySnapshot {
                enabled: true,
                failed_processes: failed,
                message: "Paused: foreground app is unknown.".to_owned(),
                ..Default::default()
            };
        }

        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            let failed = self.clear_all(action_log, "current Windows session is unknown");
            return CpuAffinitySnapshot {
                enabled: true,
                failed_processes: failed,
                message: "Paused: current Windows session is unknown.".to_owned(),
                ..Default::default()
            };
        };

        let processes = match list_processes() {
            Ok(processes) => processes,
            Err(err) => {
                let failed = self.clear_all(action_log, "process list unavailable");
                return CpuAffinitySnapshot {
                    enabled: true,
                    failed_processes: failed,
                    message: err,
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let current_process_names = processes
            .iter()
            .map(|process| (process.id, process.name.clone()))
            .collect::<BTreeMap<_, _>>();
        let foreground_process_name = if settings.exclude_foreground_app {
            foreground_process_id.and_then(|id| {
                processes
                    .iter()
                    .find(|process| process.id == id)
                    .map(|process| process.name.clone())
            })
        } else {
            None
        };
        let mut target_processes = BTreeMap::new();
        for process in processes {
            if process.id == 0
                || process.id == current_process_id
                || should_ignore_foreground_process(
                    settings,
                    process.id,
                    &process.name,
                    foreground_process_id,
                    foreground_process_name.as_deref(),
                )
                || is_builtin_excluded(&process.name)
            {
                continue;
            }

            if process_session_id(process.id) != Some(current_session_id) {
                continue;
            }

            if let Some(rule) = matching_rule(settings, &process.name) {
                target_processes.insert(process.id, (process.name, rule.mode, rule.core_mask));
            }
        }

        let active_target_names = target_processes
            .values()
            .map(|(name, _mode, _rule_mask)| process_failure_key(name))
            .collect::<BTreeSet<_>>();
        self.failure_suppression
            .retain(|name, _| active_target_names.contains(name));

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let mut failed_processes = self.release_non_targets(
            &target_ids,
            &current_process_names,
            action_log,
            &format!("process no longer matches a {} rule", self.feature_label()),
        );
        let mut skipped_processes = 0;
        let mut last_error = None;
        let mut auto_excluded_processes = BTreeSet::new();

        for (process_id, (process_name, mode, rule_mask)) in target_processes {
            let failure_process_name = process_name.clone();
            let suppression =
                self.check_process_suppression(process_id, &failure_process_name, action_log);
            if suppression.suppressed {
                skipped_processes += 1;
                if suppression.newly_suppressed
                    && self.action_log_feature == ActionLogFeature::BackgroundCpuRestriction
                {
                    auto_excluded_processes.insert(process_failure_key(&failure_process_name));
                }
                continue;
            }

            match apply_affinity(
                process_id,
                process_name,
                mode,
                rule_mask,
                self.adjusted.get(&process_id),
                self.action_log_feature,
                action_log,
            ) {
                Ok(Some(adjusted)) => {
                    self.clear_process_failure(&failure_process_name);
                    self.adjusted.insert(process_id, adjusted);
                }
                Ok(None) => {
                    skipped_processes += 1;
                }
                Err(AffinityError::AccessDenied) => {
                    skipped_processes += 1;
                    self.record_process_failure(&failure_process_name);
                    action_log.record(
                        self.action_log_feature,
                        Some(process_id),
                        failure_process_name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                Err(AffinityError::Failed(err)) => {
                    failed_processes += 1;
                    self.record_process_failure(&failure_process_name);
                    if last_error.is_none() {
                        last_error = Some(err.clone());
                    }
                    action_log.record(
                        self.action_log_feature,
                        Some(process_id),
                        failure_process_name,
                        ActionLogAction::Fail,
                        ActionLogResult::Failed,
                        err,
                    );
                }
            }
        }

        CpuAffinitySnapshot {
            enabled: true,
            scanned_processes,
            adjusted_processes: self.adjusted.len(),
            skipped_processes,
            failed_processes,
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            adjusted_apps: unique_app_names(
                self.adjusted
                    .values()
                    .map(|process| process.process_name.as_str()),
            ),
            message: cpu_affinity_message(settings),
            last_error,
        }
    }

    fn release_non_targets(
        &mut self,
        target_ids: &BTreeSet<u32>,
        current_process_names: &BTreeMap<u32, String>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> usize {
        let process_ids = self
            .adjusted
            .keys()
            .copied()
            .filter(|process_id| !target_ids.contains(process_id))
            .collect::<Vec<_>>();

        self.release_processes(
            &process_ids,
            Some(current_process_names),
            action_log,
            reason,
        )
    }

    fn clear_all(&mut self, action_log: &mut ActionLog, reason: &str) -> usize {
        let process_ids = self.adjusted.keys().copied().collect::<Vec<_>>();
        self.release_processes(&process_ids, None, action_log, reason)
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        current_process_names: Option<&BTreeMap<u32, String>>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> usize {
        let mut failed = 0;
        for process_id in process_ids {
            if let Some(process) = self.adjusted.remove(process_id) {
                let process_name = process.process_name.clone();
                let adjustment = process.adjustment.clone();
                let still_same_process = current_process_names.map_or(true, |names| {
                    names
                        .get(process_id)
                        .is_some_and(|name| name.eq_ignore_ascii_case(&process.process_name))
                });
                if still_same_process {
                    if let Err(err) = restore_affinity(*process_id, process) {
                        failed += 1;
                        action_log.record(
                            self.action_log_feature,
                            Some(*process_id),
                            process_name,
                            ActionLogAction::Fail,
                            ActionLogResult::Failed,
                            affinity_error_message(err),
                        );
                    } else {
                        action_log.record(
                            self.action_log_feature,
                            Some(*process_id),
                            process_name,
                            ActionLogAction::Restore,
                            ActionLogResult::Restored,
                            format!("{reason}: restored {}.", adjustment_label(&adjustment)),
                        );
                    }
                }
            }
        }
        failed
    }

    fn check_process_suppression(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log: &mut ActionLog,
    ) -> ProcessSuppression {
        let Some(suppression) = self
            .failure_suppression
            .get_mut(&process_failure_key(process_name))
        else {
            return ProcessSuppression::default();
        };
        if suppression.attempts < FAILURE_SUPPRESSION_THRESHOLD {
            return ProcessSuppression::default();
        }

        let mut newly_suppressed = false;
        if !suppression.suppression_logged {
            suppression.suppression_logged = true;
            newly_suppressed = true;
            action_log.record(
                self.action_log_feature,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying {} after {FAILURE_SUPPRESSION_THRESHOLD} failed attempts.",
                    self.feature_label(),
                ),
            );
        }

        ProcessSuppression {
            suppressed: true,
            newly_suppressed,
        }
    }

    fn record_process_failure(&mut self, process_name: &str) {
        let suppression = self
            .failure_suppression
            .entry(process_failure_key(process_name))
            .or_default();
        suppression.attempts = suppression.attempts.saturating_add(1);
    }

    fn clear_process_failure(&mut self, process_name: &str) {
        self.failure_suppression
            .remove(&process_failure_key(process_name));
    }

    fn feature_label(&self) -> &'static str {
        match self.action_log_feature {
            ActionLogFeature::BackgroundCpuRestriction => "Background CPU Restriction",
            _ => "Core Steering",
        }
    }
}

#[derive(Default)]
struct ProcessSuppression {
    suppressed: bool,
    newly_suppressed: bool,
}

impl Drop for CpuAffinityManager {
    fn drop(&mut self) {
        let mut action_log = ActionLog::new(1);
        self.clear_all(
            &mut action_log,
            &format!("{} manager dropped", self.feature_label()),
        );
    }
}

impl Default for CpuAffinityManager {
    fn default() -> Self {
        Self {
            adjusted: BTreeMap::new(),
            failure_suppression: BTreeMap::new(),
            action_log_feature: ActionLogFeature::CoreSteering,
        }
    }
}

impl Default for CpuAffinitySnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            scanned_processes: 0,
            adjusted_processes: 0,
            skipped_processes: 0,
            failed_processes: 0,
            auto_excluded_processes: Vec::new(),
            adjusted_apps: Vec::new(),
            message: "Core Steering disabled.".to_owned(),
            last_error: None,
        }
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    let process_name = process_name.trim();
    BUILT_IN_EXCLUSIONS
        .iter()
        .any(|excluded| excluded.eq_ignore_ascii_case(process_name))
}

pub fn contains_process(list: &[String], process_name: &str) -> bool {
    list.iter()
        .any(|name| name.trim().eq_ignore_ascii_case(process_name.trim()))
}

pub fn logical_processors() -> Vec<LogicalProcessorInfo> {
    logical_processors_from_topology().unwrap_or_else(fallback_logical_processors)
}

fn cpu_affinity_message(settings: &CpuAffinitySettings) -> String {
    cpu_affinity_message_for_group_count(
        active_processor_group_count(),
        settings
            .rules
            .iter()
            .any(|rule| rule.enabled && rule.mode == CpuAffinityMode::Hard),
    )
}

fn cpu_affinity_message_for_group_count(group_count: u16, has_hard_rules: bool) -> String {
    if group_count > 1 && has_hard_rules {
        "Core Steering active. Multi-group CPU detected: hard steering can only control CPUs in the process primary processor group. Apps that are not processor-group-aware may not use the full CPU."
            .to_owned()
    } else {
        "Core Steering active.".to_owned()
    }
}

fn active_processor_group_count() -> u16 {
    unsafe { GetActiveProcessorGroupCount() }
}

fn logical_processors_from_topology() -> Option<Vec<LogicalProcessorInfo>> {
    let mut returned_length = 0;
    unsafe {
        GetLogicalProcessorInformationEx(RelationProcessorCore, null_mut(), &mut returned_length);
    }

    if returned_length == 0 {
        return None;
    }

    let word_count = (returned_length as usize).div_ceil(size_of::<usize>());
    let mut buffer = vec![0_usize; word_count];
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
            let processor = unsafe {
                read_unaligned(
                    buffer.as_ptr().add(offset + header_size) as *const PROCESSOR_RELATIONSHIP
                )
            };
            let available_group_count =
                record_size.saturating_sub(group_mask_offset) / group_mask_size;
            let group_count = usize::from(processor.GroupCount).min(available_group_count);
            for group_index in 0..group_count {
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

fn should_ignore_foreground_process(
    settings: &CpuAffinitySettings,
    process_id: u32,
    process_name: &str,
    foreground_process_id: Option<u32>,
    foreground_process_name: Option<&str>,
) -> bool {
    settings.exclude_foreground_app
        && (foreground_process_id.is_some_and(|id| id == process_id)
            || foreground_process_name
                .is_some_and(|name| name.eq_ignore_ascii_case(process_name.trim())))
}

fn process_failure_key(process_name: &str) -> String {
    process_name.trim().to_ascii_lowercase()
}

fn matching_rule<'a>(
    settings: &'a CpuAffinitySettings,
    process_name: &str,
) -> Option<&'a CpuAffinityRule> {
    settings.rules.iter().find(|rule| {
        rule.enabled
            && rule_has_target(rule)
            && rule
                .process_name
                .trim()
                .eq_ignore_ascii_case(process_name.trim())
    })
}

fn rule_has_target(rule: &CpuAffinityRule) -> bool {
    rule.mode == CpuAffinityMode::EfficiencyOff || rule.core_mask != 0
}

fn process_session_id(process_id: u32) -> Option<u32> {
    let mut session_id = 0;
    let ok = unsafe { ProcessIdToSessionId(process_id, &mut session_id) };
    (ok != 0).then_some(session_id)
}

enum AffinityError {
    AccessDenied,
    Failed(String),
}

fn apply_affinity(
    process_id: u32,
    process_name: String,
    mode: CpuAffinityMode,
    rule_mask: u64,
    existing: Option<&AdjustedProcess>,
    action_log_feature: ActionLogFeature,
    action_log: &mut ActionLog,
) -> Result<Option<AdjustedProcess>, AffinityError> {
    let process = ProcessHandle::open(process_id)?;
    let reusable_existing = existing
        .filter(|adjusted| adjusted.process_name.eq_ignore_ascii_case(&process_name))
        .filter(|adjusted| adjusted.adjustment.mode() == mode);

    if let Some(adjusted) = existing {
        if !adjusted.process_name.eq_ignore_ascii_case(&process_name)
            || adjusted.adjustment.mode() != mode
        {
            restore_adjustment(&process, &adjusted.adjustment)?;
            action_log.record(
                action_log_feature,
                Some(process_id),
                process_name.clone(),
                ActionLogAction::Restore,
                ActionLogResult::Restored,
                format!(
                    "Rule changed: restored previous {}.",
                    adjustment_label(&adjusted.adjustment)
                ),
            );
        }
    }

    match mode {
        CpuAffinityMode::Hard => apply_hard_affinity(
            process_id,
            &process,
            process_name,
            rule_mask,
            reusable_existing,
            action_log_feature,
            action_log,
        ),
        CpuAffinityMode::Soft => apply_soft_affinity(
            process_id,
            &process,
            process_name,
            rule_mask,
            reusable_existing,
            action_log_feature,
            action_log,
        ),
        CpuAffinityMode::EfficiencyOff => apply_efficiency_mode_off(
            process_id,
            &process,
            process_name,
            reusable_existing,
            action_log_feature,
            action_log,
        ),
    }
}

fn restore_affinity(process_id: u32, process_state: AdjustedProcess) -> Result<(), AffinityError> {
    let process = ProcessHandle::open(process_id)?;
    restore_adjustment(&process, &process_state.adjustment)
}

fn apply_hard_affinity(
    process_id: u32,
    process: &ProcessHandle,
    process_name: String,
    rule_mask: u64,
    existing: Option<&AdjustedProcess>,
    action_log_feature: ActionLogFeature,
    action_log: &mut ActionLog,
) -> Result<Option<AdjustedProcess>, AffinityError> {
    let (current_affinity, system_affinity) = process.affinity_mask()?;
    let Some(target_affinity) = target_affinity_mask(rule_mask, system_affinity) else {
        return Ok(None);
    };

    if existing.is_some_and(|adjusted| {
        matches!(
            adjusted.adjustment,
            AffinityAdjustment::Hard {
                applied_affinity,
                ..
            } if applied_affinity == target_affinity
        ) && current_affinity == target_affinity
    }) {
        return Ok(existing.cloned());
    }

    process.set_affinity_mask(target_affinity)?;

    let previous_affinity = existing
        .and_then(|adjusted| match adjusted.adjustment {
            AffinityAdjustment::Hard {
                previous_affinity, ..
            } => Some(previous_affinity),
            AffinityAdjustment::Soft { .. } | AffinityAdjustment::EfficiencyOff { .. } => None,
        })
        .unwrap_or(current_affinity);
    action_log.record(
        action_log_feature,
        Some(process_id),
        process_name.clone(),
        ActionLogAction::Apply,
        ActionLogResult::Applied,
        format!("Applied hard affinity mask {target_affinity:#x}."),
    );

    Ok(Some(AdjustedProcess {
        process_name,
        adjustment: AffinityAdjustment::Hard {
            previous_affinity,
            applied_affinity: target_affinity,
        },
    }))
}

fn apply_soft_affinity(
    process_id: u32,
    process: &ProcessHandle,
    process_name: String,
    rule_mask: u64,
    existing: Option<&AdjustedProcess>,
    action_log_feature: ActionLogFeature,
    action_log: &mut ActionLog,
) -> Result<Option<AdjustedProcess>, AffinityError> {
    let Some(target_cpu_set_ids) = target_cpu_set_ids(rule_mask)? else {
        return Ok(None);
    };
    let current_cpu_set_ids = process.default_cpu_set_ids()?;

    if existing.is_some_and(|adjusted| {
        matches!(
            &adjusted.adjustment,
            AffinityAdjustment::Soft {
                applied_cpu_set_ids,
                ..
            } if *applied_cpu_set_ids == target_cpu_set_ids
        ) && current_cpu_set_ids == target_cpu_set_ids
    }) {
        return Ok(existing.cloned());
    }

    process.set_default_cpu_set_ids(&target_cpu_set_ids)?;

    let previous_cpu_set_ids = existing
        .and_then(|adjusted| match &adjusted.adjustment {
            AffinityAdjustment::Soft {
                previous_cpu_set_ids,
                ..
            } => Some(previous_cpu_set_ids.clone()),
            AffinityAdjustment::Hard { .. } | AffinityAdjustment::EfficiencyOff { .. } => None,
        })
        .unwrap_or(current_cpu_set_ids);
    action_log.record(
        action_log_feature,
        Some(process_id),
        process_name.clone(),
        ActionLogAction::Apply,
        ActionLogResult::Applied,
        format!("Applied CPU Sets: {}.", target_cpu_set_ids.len()),
    );

    Ok(Some(AdjustedProcess {
        process_name,
        adjustment: AffinityAdjustment::Soft {
            previous_cpu_set_ids,
            applied_cpu_set_ids: target_cpu_set_ids,
        },
    }))
}

fn apply_efficiency_mode_off(
    process_id: u32,
    process: &ProcessHandle,
    process_name: String,
    existing: Option<&AdjustedProcess>,
    action_log_feature: ActionLogFeature,
    action_log: &mut ActionLog,
) -> Result<Option<AdjustedProcess>, AffinityError> {
    let current_state = process.power_throttling_state().ok();

    if existing.is_some_and(|adjusted| {
        matches!(
            adjusted.adjustment,
            AffinityAdjustment::EfficiencyOff { .. }
        ) && current_state.is_some_and(|state| !power_throttling_execution_enabled(state))
    }) {
        return Ok(existing.cloned());
    }

    let mut next_state = current_state.unwrap_or_else(power_throttling_disabled_state);
    next_state.Version = PROCESS_POWER_THROTTLING_CURRENT_VERSION;
    next_state.ControlMask |= PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    next_state.StateMask &= !PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    process.set_power_throttling_state(next_state)?;
    action_log.record(
        action_log_feature,
        Some(process_id),
        process_name.clone(),
        ActionLogAction::Apply,
        ActionLogResult::Applied,
        "Disabled Efficiency Mode execution-speed throttling.",
    );

    let previous_state = existing
        .and_then(|adjusted| match adjusted.adjustment {
            AffinityAdjustment::EfficiencyOff { previous_state } => previous_state,
            AffinityAdjustment::Hard { .. } | AffinityAdjustment::Soft { .. } => None,
        })
        .or(current_state);

    Ok(Some(AdjustedProcess {
        process_name,
        adjustment: AffinityAdjustment::EfficiencyOff { previous_state },
    }))
}

fn restore_adjustment(
    process: &ProcessHandle,
    adjustment: &AffinityAdjustment,
) -> Result<(), AffinityError> {
    match adjustment {
        AffinityAdjustment::Hard {
            previous_affinity, ..
        } => process.set_affinity_mask(*previous_affinity),
        AffinityAdjustment::Soft {
            previous_cpu_set_ids,
            ..
        } => process.set_default_cpu_set_ids(previous_cpu_set_ids),
        AffinityAdjustment::EfficiencyOff { previous_state } => process.set_power_throttling_state(
            previous_state.unwrap_or_else(power_throttling_disabled_state),
        ),
    }
}

impl AffinityAdjustment {
    fn mode(&self) -> CpuAffinityMode {
        match self {
            Self::Hard { .. } => CpuAffinityMode::Hard,
            Self::Soft { .. } => CpuAffinityMode::Soft,
            Self::EfficiencyOff { .. } => CpuAffinityMode::EfficiencyOff,
        }
    }
}

fn adjustment_label(adjustment: &AffinityAdjustment) -> &'static str {
    match adjustment {
        AffinityAdjustment::Hard { .. } => "hard affinity",
        AffinityAdjustment::Soft { .. } => "CPU Sets",
        AffinityAdjustment::EfficiencyOff { .. } => "Efficiency Mode Off",
    }
}

fn affinity_error_message(error: AffinityError) -> String {
    match error {
        AffinityError::AccessDenied => "Access denied.".to_owned(),
        AffinityError::Failed(message) => message,
    }
}

fn power_throttling_disabled_state() -> PROCESS_POWER_THROTTLING_STATE {
    PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0,
    }
}

fn power_throttling_execution_enabled(state: PROCESS_POWER_THROTTLING_STATE) -> bool {
    (state.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED) != 0
}

fn target_affinity_mask(rule_mask: u64, system_affinity: usize) -> Option<usize> {
    let mut mask = (rule_mask & usize::MAX as u64) as usize;
    if system_affinity != 0 {
        mask &= system_affinity;
    }
    (mask != 0).then_some(mask)
}

fn target_cpu_set_ids(rule_mask: u64) -> Result<Option<Vec<u32>>, AffinityError> {
    let mut ids = system_cpu_set_ids_for_mask(rule_mask)?;
    ids.sort_unstable();
    ids.dedup();
    Ok((!ids.is_empty()).then_some(ids))
}

fn system_cpu_set_ids_for_mask(rule_mask: u64) -> Result<Vec<u32>, AffinityError> {
    let mut returned_length = 0;
    unsafe {
        GetSystemCpuSetInformation(null_mut(), 0, &mut returned_length, null_mut(), 0);
    }

    if returned_length == 0 {
        return Ok(Vec::new());
    }

    let word_count = (returned_length as usize).div_ceil(size_of::<usize>());
    let mut buffer = vec![0_usize; word_count];
    let ok = unsafe {
        GetSystemCpuSetInformation(
            buffer.as_mut_ptr() as *mut SYSTEM_CPU_SET_INFORMATION,
            returned_length,
            &mut returned_length,
            null_mut(),
            0,
        )
    };
    if ok == 0 {
        return Err(AffinityError::Failed(format!(
            "GetSystemCpuSetInformation failed with error {}.",
            last_error()
        )));
    }

    Ok(cpu_set_ids_for_mask_from_bytes(
        unsafe { slice::from_raw_parts(buffer.as_ptr() as *const u8, returned_length as usize) },
        rule_mask,
    ))
}

fn cpu_set_ids_for_mask_from_bytes(buffer: &[u8], rule_mask: u64) -> Vec<u32> {
    let mut ids = Vec::new();
    let mut offset = 0;
    let header_size = size_of::<CpuSetInformationHeader>();

    while offset + header_size <= buffer.len() {
        let header = unsafe {
            read_unaligned(buffer.as_ptr().add(offset) as *const CpuSetInformationHeader)
        };
        let record_size = header.size as usize;
        if record_size < header_size || offset + record_size > buffer.len() {
            break;
        }

        if header.cpu_set_type == 0 && record_size >= size_of::<SYSTEM_CPU_SET_INFORMATION>() {
            let info = unsafe {
                read_unaligned(buffer.as_ptr().add(offset) as *const SYSTEM_CPU_SET_INFORMATION)
            };
            let cpu_set = unsafe { info.Anonymous.CpuSet };
            if cpu_set.Group == 0 && cpu_set.LogicalProcessorIndex < 64 {
                let bit = 1_u64 << cpu_set.LogicalProcessorIndex;
                if (rule_mask & bit) != 0 {
                    ids.push(cpu_set.Id);
                }
            }
        }

        offset += record_size;
    }

    ids
}

fn unique_app_names<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    names
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

struct ProcessHandle(HANDLE);

impl ProcessHandle {
    fn open(process_id: u32) -> Result<Self, AffinityError> {
        let access_masks = [
            PROCESS_QUERY_INFORMATION | PROCESS_SET_INFORMATION,
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
        ];

        let mut last_open_error = 0;
        for access in access_masks {
            let handle = unsafe { OpenProcess(access, 0, process_id) };
            if !handle.is_null() {
                return Ok(Self(handle));
            }
            last_open_error = last_error();
        }

        if last_open_error == ERROR_ACCESS_DENIED {
            Err(AffinityError::AccessDenied)
        } else {
            Err(AffinityError::Failed(format!(
                "OpenProcess({process_id}) failed with error {last_open_error}."
            )))
        }
    }

    fn affinity_mask(&self) -> Result<(usize, usize), AffinityError> {
        let mut process_affinity = 0;
        let mut system_affinity = 0;
        let ok =
            unsafe { GetProcessAffinityMask(self.0, &mut process_affinity, &mut system_affinity) };
        if ok == 0 {
            Err(AffinityError::Failed(format!(
                "GetProcessAffinityMask failed with error {}.",
                last_error()
            )))
        } else {
            Ok((process_affinity, system_affinity))
        }
    }

    fn set_affinity_mask(&self, affinity_mask: usize) -> Result<(), AffinityError> {
        let ok = unsafe { SetProcessAffinityMask(self.0, affinity_mask) };
        if ok == 0 {
            Err(AffinityError::Failed(format!(
                "SetProcessAffinityMask failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    fn default_cpu_set_ids(&self) -> Result<Vec<u32>, AffinityError> {
        let mut required_id_count = 0;
        unsafe {
            GetProcessDefaultCpuSets(self.0, null_mut(), 0, &mut required_id_count);
        }
        if required_id_count == 0 {
            return Ok(Vec::new());
        }

        let mut ids = vec![0_u32; required_id_count as usize];
        let ok = unsafe {
            GetProcessDefaultCpuSets(
                self.0,
                ids.as_mut_ptr(),
                ids.len() as u32,
                &mut required_id_count,
            )
        };
        if ok == 0 {
            Err(AffinityError::Failed(format!(
                "GetProcessDefaultCpuSets failed with error {}.",
                last_error()
            )))
        } else {
            ids.truncate(required_id_count as usize);
            Ok(ids)
        }
    }

    fn set_default_cpu_set_ids(&self, ids: &[u32]) -> Result<(), AffinityError> {
        let (ptr, count) = if ids.is_empty() {
            (null_mut(), 0)
        } else {
            (ids.as_ptr() as *mut u32, ids.len() as u32)
        };
        let ok = unsafe { SetProcessDefaultCpuSets(self.0, ptr, count) };
        if ok == 0 {
            Err(AffinityError::Failed(format!(
                "SetProcessDefaultCpuSets failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    fn power_throttling_state(&self) -> Result<PROCESS_POWER_THROTTLING_STATE, AffinityError> {
        let mut state = PROCESS_POWER_THROTTLING_STATE::default();
        let ok = unsafe {
            GetProcessInformation(
                self.0,
                ProcessPowerThrottling,
                &mut state as *mut _ as *mut c_void,
                size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        };
        if ok == 0 {
            Err(AffinityError::Failed(format!(
                "GetProcessInformation ProcessPowerThrottling failed with error {}.",
                last_error()
            )))
        } else {
            Ok(state)
        }
    }

    fn set_power_throttling_state(
        &self,
        state: PROCESS_POWER_THROTTLING_STATE,
    ) -> Result<(), AffinityError> {
        let ok = unsafe {
            SetProcessInformation(
                self.0,
                ProcessPowerThrottling,
                &state as *const _ as *const c_void,
                size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        };
        if ok == 0 {
            Err(AffinityError::Failed(format!(
                "SetProcessInformation ProcessPowerThrottling failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn last_error() -> u32 {
    unsafe { GetLastError() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_match_is_case_insensitive_and_ignores_disabled_or_empty_masks() {
        let settings = CpuAffinitySettings {
            enabled: true,
            exclude_foreground_app: true,
            rules: vec![
                CpuAffinityRule {
                    enabled: false,
                    mode: CpuAffinityMode::Hard,
                    process_name: "browser.exe".to_owned(),
                    core_mask: 1,
                },
                CpuAffinityRule {
                    enabled: true,
                    mode: CpuAffinityMode::Hard,
                    process_name: "backup.exe".to_owned(),
                    core_mask: 0,
                },
                CpuAffinityRule {
                    enabled: true,
                    mode: CpuAffinityMode::Soft,
                    process_name: " Worker.EXE ".to_owned(),
                    core_mask: 0b11,
                },
                CpuAffinityRule {
                    enabled: true,
                    mode: CpuAffinityMode::EfficiencyOff,
                    process_name: "Game.EXE".to_owned(),
                    core_mask: 0,
                },
            ],
        };

        assert!(matching_rule(&settings, "worker.exe").is_some());
        assert!(matching_rule(&settings, "game.exe").is_some());
        assert!(matching_rule(&settings, "browser.exe").is_none());
        assert!(matching_rule(&settings, "backup.exe").is_none());
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
    fn repeated_failures_suppress_future_core_steering_attempts_once() {
        let mut manager = CpuAffinityManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure("APP.exe");
        manager.record_process_failure("app.exe");
        assert!(!manager.is_process_suppressed(42, "app.exe", &mut log));
        assert!(log.entries().is_empty());

        manager.record_process_failure("app.exe");
        assert!(manager.is_process_suppressed(42, "app.exe", &mut log));
        assert!(manager.is_process_suppressed(43, "APP.exe", &mut log));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].process_name, "app.exe");
        assert_eq!(entries[0].action, ActionLogAction::Skip);
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
    }

    #[test]
    fn configured_action_log_feature_is_used_for_suppression_entries() {
        let mut manager =
            CpuAffinityManager::with_action_log_feature(ActionLogFeature::BackgroundCpuRestriction);
        let mut log = ActionLog::new(8);

        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");
        assert!(manager.is_process_suppressed(42, "app.exe", &mut log));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].feature,
            ActionLogFeature::BackgroundCpuRestriction
        );
        assert!(entries[0].reason.contains("Background CPU Restriction"));
    }

    #[test]
    fn first_background_cpu_suppression_reports_auto_exclusion_once() {
        let mut manager =
            CpuAffinityManager::with_action_log_feature(ActionLogFeature::BackgroundCpuRestriction);
        let mut log = ActionLog::new(8);

        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");

        let first = manager.check_process_suppression(42, "app.exe", &mut log);
        let second = manager.check_process_suppression(42, "app.exe", &mut log);

        assert!(first.suppressed);
        assert!(first.newly_suppressed);
        assert!(second.suppressed);
        assert!(!second.newly_suppressed);
    }

    #[test]
    fn cpu_affinity_message_warns_on_multiple_processor_groups() {
        assert_eq!(
            cpu_affinity_message_for_group_count(1, true),
            "Core Steering active."
        );
        assert_eq!(
            cpu_affinity_message_for_group_count(2, false),
            "Core Steering active."
        );
        let message = cpu_affinity_message_for_group_count(2, true);
        assert!(message.contains("Multi-group CPU detected"));
        assert!(message.contains("processor-group-aware"));
    }

    #[test]
    fn target_mask_intersects_system_affinity() {
        assert_eq!(target_affinity_mask(0b1110, 0b0110), Some(0b0110));
        assert_eq!(target_affinity_mask(0b1000, 0b0111), None);
        assert_eq!(target_affinity_mask(0, 0b0111), None);
    }

    #[test]
    fn target_cpu_set_ids_empty_when_mask_selects_no_known_cpus() {
        assert!(cpu_set_ids_for_mask_from_bytes(&[], 0b11).is_empty());
    }

    #[test]
    fn power_throttling_execution_flag_detection() {
        let mut state = power_throttling_disabled_state();
        assert!(!power_throttling_execution_enabled(state));

        state.StateMask |= PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
        assert!(power_throttling_execution_enabled(state));
    }

    #[test]
    fn foreground_skip_matches_pid_or_name() {
        let mut settings = CpuAffinitySettings::default();
        settings.exclude_foreground_app = true;

        assert!(should_ignore_foreground_process(
            &settings,
            42,
            "helper.exe",
            Some(42),
            Some("app.exe"),
        ));
        assert!(should_ignore_foreground_process(
            &settings,
            99,
            "APP.EXE",
            Some(42),
            Some("app.exe"),
        ));
        assert!(!should_ignore_foreground_process(
            &settings,
            99,
            "other.exe",
            Some(42),
            Some("app.exe"),
        ));

        settings.exclude_foreground_app = false;
        assert!(!should_ignore_foreground_process(
            &settings,
            42,
            "app.exe",
            Some(42),
            Some("app.exe"),
        ));
    }

    #[test]
    fn built_in_exclusions_include_system_processes() {
        assert!(is_builtin_excluded("csrss.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("browser.exe"));
    }

    #[test]
    fn release_processes_skips_restore_when_process_identity_is_unknown() {
        let mut manager = CpuAffinityManager::default();
        manager.adjusted.insert(
            0,
            AdjustedProcess {
                process_name: "exited.exe".to_owned(),
                adjustment: AffinityAdjustment::Hard {
                    previous_affinity: 0b1111,
                    applied_affinity: 0b0001,
                },
            },
        );
        let mut log = ActionLog::new(8);

        let failed = manager.release_processes(&[0], Some(&BTreeMap::new()), &mut log, "test");

        assert_eq!(failed, 0);
        assert!(log.entries().is_empty());
        assert!(manager.adjusted.is_empty());
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
