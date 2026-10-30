//! Opt-in scheduler workloads, shared verbatim by the two framework fixtures.
//! Process mutations are restricted to the dedicated benchmark executable.
use crate::{
    backend::automation::RuntimeStatusSnapshot, config::*, cpu_allocation, power,
    rules::DecisionState,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Profile {
    Disabled,
    EnabledNoMatch,
    ProcessPriority,
    ThreadPriority,
    DynamicPriorityBoost,
    IoPriority,
    MemoryPriority,
    GpuPriority,
    BackgroundEfficiency,
    CpuSets,
    ProcessorAffinity,
    CpuLimiter,
    AppSuspension,
    ByActivity,
    ByForeground,
    ByRunningApp,
    ByCpuLoad,
    ByTime,
    TimerResolution,
    AdaptiveDetection,
    AdaptivePressureMonitor,
    Combined,
}

impl Profile {
    fn parse(value: &str) -> Result<Self, String> {
        Ok(match value {
            "disabled" => Self::Disabled,
            "enabled-no-match" => Self::EnabledNoMatch,
            "process-priority" => Self::ProcessPriority,
            "thread-priority" => Self::ThreadPriority,
            "dynamic-priority-boost" => Self::DynamicPriorityBoost,
            "io-priority" => Self::IoPriority,
            "memory-priority" => Self::MemoryPriority,
            "gpu-priority" => Self::GpuPriority,
            "background-efficiency" => Self::BackgroundEfficiency,
            "cpu-sets" => Self::CpuSets,
            "processor-affinity" => Self::ProcessorAffinity,
            "cpu-limiter" => Self::CpuLimiter,
            "app-suspension" => Self::AppSuspension,
            "by-activity" => Self::ByActivity,
            "by-foreground" => Self::ByForeground,
            "by-running-app" => Self::ByRunningApp,
            "by-cpu-load" => Self::ByCpuLoad,
            "by-time" => Self::ByTime,
            "timer-resolution" => Self::TimerResolution,
            "adaptive-detection" => Self::AdaptiveDetection,
            "adaptive-pressure-monitor" => Self::AdaptivePressureMonitor,
            "combined" => Self::Combined,
            _ => return Err(format!("Unsupported scheduler benchmark profile: {value}")),
        })
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::EnabledNoMatch => "enabled-no-match",
            Self::ProcessPriority => "process-priority",
            Self::ThreadPriority => "thread-priority",
            Self::DynamicPriorityBoost => "dynamic-priority-boost",
            Self::IoPriority => "io-priority",
            Self::MemoryPriority => "memory-priority",
            Self::GpuPriority => "gpu-priority",
            Self::BackgroundEfficiency => "background-efficiency",
            Self::CpuSets => "cpu-sets",
            Self::ProcessorAffinity => "processor-affinity",
            Self::CpuLimiter => "cpu-limiter",
            Self::AppSuspension => "app-suspension",
            Self::ByActivity => "by-activity",
            Self::ByForeground => "by-foreground",
            Self::ByRunningApp => "by-running-app",
            Self::ByCpuLoad => "by-cpu-load",
            Self::ByTime => "by-time",
            Self::TimerResolution => "timer-resolution",
            Self::AdaptiveDetection => "adaptive-detection",
            Self::AdaptivePressureMonitor => "adaptive-pressure-monitor",
            Self::Combined => "combined",
        }
    }

    fn needs_current_plan(self) -> bool {
        matches!(
            self,
            Self::ByActivity
                | Self::ByForeground
                | Self::ByRunningApp
                | Self::ByCpuLoad
                | Self::ByTime
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Options {
    pub(crate) profile: Profile,
    pub(crate) target: PathBuf,
    pub(crate) current_plan: Option<String>,
}

impl Options {
    pub(crate) fn parse() -> Result<Option<Self>, String> {
        Self::parse_arguments(&std::env::args().collect::<Vec<_>>())
    }

    fn parse_arguments(arguments: &[String]) -> Result<Option<Self>, String> {
        let argument = |name| -> Result<Option<String>, String> {
            let positions = arguments
                .iter()
                .enumerate()
                .filter(|(_, value)| value.as_str() == name)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            match positions.as_slice() {
                [] => Ok(None),
                [index] => arguments
                    .get(index + 1)
                    .filter(|value| !value.starts_with("--"))
                    .cloned()
                    .map(Some)
                    .ok_or_else(|| format!("Missing value for {name}")),
                _ => Err(format!("Duplicate benchmark argument: {name}")),
            }
        };
        let profile = argument("--framework-benchmark-scheduler")?;
        let target = argument("--framework-benchmark-target")?;
        let current_plan = argument("--framework-benchmark-current-plan")?;
        let Some(profile) = profile else {
            if target.is_some() || current_plan.is_some() {
                return Err("Scheduler target/plan requires a scheduler benchmark profile".into());
            }
            return Ok(None);
        };
        let profile = Profile::parse(&profile)?;
        let target = PathBuf::from(target.ok_or("Missing scheduler benchmark target")?);
        validate_target(&target)?;
        let options = Self {
            profile,
            target,
            current_plan,
        };
        options.validate_current_plan()?;
        Ok(Some(options))
    }

    fn validate_current_plan(&self) -> Result<(), String> {
        if !self.profile.needs_current_plan() {
            return Ok(());
        }
        let requested = self
            .current_plan
            .as_deref()
            .ok_or("Power-plan benchmark requires the original active plan GUID")?;
        if !power::active_plan()?.guid.eq_ignore_ascii_case(requested) {
            return Err("Power-plan benchmark must select the currently active plan".into());
        }
        Ok(())
    }

    pub(crate) fn enabled_features(&self) -> &'static [&'static str] {
        match self.profile {
            Profile::Disabled => &[],
            Profile::EnabledNoMatch | Profile::ProcessPriority => &["Process Priority"],
            Profile::ThreadPriority => &["Thread Priority"],
            Profile::DynamicPriorityBoost => &["Dynamic Priority Boost"],
            Profile::IoPriority => &["IO Priority"],
            Profile::MemoryPriority => &["Memory Priority"],
            Profile::GpuPriority => &["GPU Priority"],
            Profile::BackgroundEfficiency => &["Background Efficiency"],
            Profile::CpuSets => &["CPU Sets (Soft)"],
            Profile::ProcessorAffinity => &["Processor Affinity (Hard)"],
            Profile::CpuLimiter => &["CPU Limiter"],
            Profile::AppSuspension => &["App Suspension"],
            Profile::ByActivity => &["By Activity"],
            Profile::ByForeground => &["By Foreground"],
            Profile::ByRunningApp => &["By Running App"],
            Profile::ByCpuLoad => &["By CPU Load"],
            Profile::ByTime => &["By Time"],
            Profile::TimerResolution => &["Timer Resolution"],
            Profile::AdaptiveDetection | Profile::AdaptivePressureMonitor => &["Adaptive Engine"],
            Profile::Combined => &[
                "Background Efficiency",
                "IO Priority",
                "Memory Priority",
                "CPU Sets (Soft)",
            ],
        }
    }

    pub(crate) fn initial_pass_observed(&self, status: &RuntimeStatusSnapshot) -> bool {
        if status.worker_error.is_some() {
            return false;
        }
        let s = &status.feature_status;
        let successful = |enabled, adjusted, failed, error: Option<&str>| {
            enabled && adjusted > 0 && failed == 0 && error.is_none()
        };
        match self.profile {
            Profile::Disabled => true,
            Profile::EnabledNoMatch => s.process_priority.enabled,
            Profile::ProcessPriority => successful(
                s.process_priority.enabled,
                s.process_priority.adjusted_processes,
                s.process_priority.failed_processes,
                s.process_priority.last_error.as_deref(),
            ),
            Profile::ThreadPriority => {
                s.thread_priority.adjusted_threads > 0
                    && successful(
                        s.thread_priority.enabled,
                        s.thread_priority.adjusted_processes,
                        s.thread_priority.failed_processes,
                        s.thread_priority.last_error.as_deref(),
                    )
            }
            Profile::DynamicPriorityBoost => successful(
                s.dynamic_priority_boost.enabled,
                s.dynamic_priority_boost.adjusted_processes,
                s.dynamic_priority_boost.failed_processes,
                s.dynamic_priority_boost.last_error.as_deref(),
            ),
            Profile::IoPriority => successful(
                s.io_priority.enabled,
                s.io_priority.adjusted_processes,
                s.io_priority.failed_processes,
                s.io_priority.last_error.as_deref(),
            ),
            Profile::MemoryPriority => successful(
                s.memory_priority.enabled,
                s.memory_priority.adjusted_processes,
                s.memory_priority.failed_processes,
                s.memory_priority.last_error.as_deref(),
            ),
            Profile::GpuPriority => {
                s.gpu_priority.denied_processes == 0
                    && s.gpu_priority.suppressed_processes == 0
                    && successful(
                        s.gpu_priority.enabled,
                        s.gpu_priority
                            .adjusted_processes
                            .max(s.gpu_priority.pending_processes),
                        s.gpu_priority.failed_processes,
                        s.gpu_priority.last_error.as_deref(),
                    )
            }
            Profile::BackgroundEfficiency => {
                !s.background_efficiency.unsupported
                    && s.background_efficiency.access_denied_processes == 0
                    && successful(
                        s.background_efficiency.enabled,
                        s.background_efficiency.throttled_processes,
                        s.background_efficiency.failed_processes,
                        s.background_efficiency.last_error.as_deref(),
                    )
            }
            Profile::CpuSets => successful(
                s.cpu_sets_soft.enabled,
                s.cpu_sets_soft.adjusted_processes,
                s.cpu_sets_soft.failed_processes,
                s.cpu_sets_soft.last_error.as_deref(),
            ),
            Profile::ProcessorAffinity => successful(
                s.processor_affinity_hard.enabled,
                s.processor_affinity_hard.adjusted_processes,
                s.processor_affinity_hard.failed_processes,
                s.processor_affinity_hard.last_error.as_deref(),
            ),
            Profile::CpuLimiter => {
                s.cpu_limiter.tracked_processes > 0
                    && successful(
                        s.cpu_limiter.enabled,
                        s.cpu_limiter.limited_processes,
                        s.cpu_limiter.failed_processes,
                        s.cpu_limiter.last_error.as_deref(),
                    )
            }
            Profile::AppSuspension => {
                !s.app_suspension.status_unknown
                    && !s.app_suspension.unsupported
                    && successful(
                        s.app_suspension.enabled,
                        s.app_suspension.suspended_processes,
                        s.app_suspension.failed_actions,
                        s.app_suspension.last_error.as_deref(),
                    )
            }
            Profile::ByActivity
            | Profile::ByForeground
            | Profile::ByRunningApp
            | Profile::ByCpuLoad
            | Profile::ByTime => self.power_decision_observed(status),
            Profile::TimerResolution => s.timer_resolution.enabled,
            Profile::AdaptiveDetection => s.bottleneck_classifier.enabled,
            Profile::AdaptivePressureMonitor => {
                let monitor = &s.adaptive_engine_process;
                s.bottleneck_classifier.enabled
                    && monitor.enabled
                    && monitor.message == "Adaptive Engine active."
                    && monitor.scanned_processes > 0
                    && monitor.adjusted_processes == 0
                    && monitor.failed_processes == 0
                    && monitor.last_error.is_none()
            }
            Profile::Combined => [
                Profile::BackgroundEfficiency,
                Profile::IoPriority,
                Profile::MemoryPriority,
                Profile::CpuSets,
            ]
            .into_iter()
            .all(|profile| {
                Self {
                    profile,
                    ..self.clone()
                }
                .initial_pass_observed(status)
            }),
        }
    }

    fn power_decision_observed(&self, status: &RuntimeStatusSnapshot) -> bool {
        let Some(original) = self.current_plan.as_deref() else {
            return false;
        };
        let plan = &status.power_plan_status;
        let is_original =
            |guid: Option<&str>| guid.is_some_and(|guid| guid.eq_ignore_ascii_case(original));
        if plan.apply_failed || !is_original(plan.current_guid.as_deref()) {
            return false;
        }
        let reason = plan.reason.as_deref().unwrap_or_default();
        if self.profile == Profile::ByForeground {
            // The dedicated workload has no window, so only unmatched evaluation is covered.
            return plan.decision_state == Some(DecisionState::NoPowerPlanSelected)
                && plan.rule_index.is_none()
                && plan.target_guid.is_none()
                && reason == "Using the configured default Active plan. Select the required power plan first.";
        }
        if !is_original(plan.target_guid.as_deref()) {
            return false;
        }
        match self.profile {
            Profile::ByActivity => {
                plan.rule_index.is_none()
                    && match plan.decision_state {
                        Some(DecisionState::ByActivityIdle) => {
                            reason == "User input has been idle past the configured timeout."
                        }
                        Some(DecisionState::ByActivityActive) => {
                            reason == "Recent user input detected; using the Active plan."
                        }
                        _ => false,
                    }
            }
            Profile::ByRunningApp => {
                let running = &status.feature_status.by_running_app;
                let process_matches = self
                    .target
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        running
                            .active_process
                            .as_deref()
                            .is_some_and(|process| process.eq_ignore_ascii_case(name))
                    });
                plan.decision_state == Some(DecisionState::ByRunningApp)
                    && plan.rule_index == Some(0)
                    && running.active_rule.as_deref() == Some("Scheduler benchmark target")
                    && process_matches
                    && is_original(running.target_guid.as_deref())
                    && reason
                        .ends_with(" matched By Running App rule 'Scheduler benchmark target'.")
            }
            Profile::ByCpuLoad => {
                plan.decision_state == Some(DecisionState::ByCpuLoad)
                    && plan.rule_index == Some(0)
                    && reason.starts_with("CPU load is ")
                    && reason.ends_with(" matched rule 'Scheduler benchmark current plan'.")
            }
            Profile::ByTime => {
                plan.decision_state == Some(DecisionState::ByTime)
                    && plan.rule_index == Some(0)
                    && reason == "By Time rule 'Scheduler benchmark current plan' is active."
            }
            _ => false,
        }
    }

    pub(crate) fn apply(&self, settings: &mut Settings) -> Result<(), String> {
        validate_target(&self.target)?;
        self.validate_current_plan()?;
        // Never inherit a user rule or an enabled Battery profile into a fixture.
        let general = settings.general.clone();
        let advanced = settings.advanced.clone();
        *settings = Settings {
            general,
            advanced,
            ..Settings::default()
        };
        settings.general.enabled = self.profile != Profile::Disabled;
        settings.general.allow_cross_session_process_control = false;
        settings.general.check_for_updates = false;
        settings.general.startup_with_windows = false;
        settings.general.start_minimized = false;
        settings.general.hide_to_tray = true;
        settings.general.pause_power_plan_switching_while_plugged_in = false;
        settings.general.check_interval_ms = 1_000;
        settings.general.theme_mode = AppThemeMode::Dark;
        settings.general.language = AppLanguage::English;
        let path = self.target.to_string_lossy().into_owned();
        match self.profile {
            Profile::Disabled => {}
            Profile::EnabledNoMatch | Profile::ProcessPriority => {
                let path = if self.profile == Profile::EnabledNoMatch {
                    let stem = self
                        .target
                        .file_stem()
                        .ok_or("Benchmark target has no filename")?
                        .to_string_lossy();
                    let missing = self.target.with_file_name(format!("{stem}-no-match.exe"));
                    if missing.exists() {
                        return Err("No-match benchmark executable unexpectedly exists".into());
                    }
                    missing.to_string_lossy().into_owned()
                } else {
                    path
                };
                process_priority(settings, &path);
            }
            Profile::ThreadPriority => {
                let s = &mut settings.thread_priority;
                s.enabled = true;
                s.foreground_priority = ProcessThreadPrioritySetting::Default;
                s.visible_window_priority = ProcessThreadPrioritySetting::Default;
                s.background_priority = ProcessThreadPrioritySetting::Default;
                s.preserve_foreground_priority = false;
                s.preserve_visible_window_priority = false;
                s.preserve_background_priority = false;
                s.exclusions = vec![ProcessExclusionRule {
                    executable_path: path,
                    thread_foreground_priority: Some(ProcessThreadPrioritySetting::BelowNormal),
                    thread_visible_window_priority: Some(ProcessThreadPrioritySetting::BelowNormal),
                    thread_background_priority: Some(ProcessThreadPrioritySetting::BelowNormal),
                    ..Default::default()
                }];
            }
            Profile::DynamicPriorityBoost => {
                let s = &mut settings.dynamic_priority_boost;
                s.enabled = true;
                s.foreground_boost = ProcessDynamicPriorityBoostSetting::Default;
                s.visible_window_boost = ProcessDynamicPriorityBoostSetting::Default;
                s.background_boost = ProcessDynamicPriorityBoostSetting::Default;
                s.exclusions = vec![ProcessExclusionRule {
                    executable_path: path,
                    dynamic_priority_boost_foreground: Some(
                        ProcessDynamicPriorityBoostSetting::Disabled,
                    ),
                    dynamic_priority_boost_visible_window: Some(
                        ProcessDynamicPriorityBoostSetting::Disabled,
                    ),
                    dynamic_priority_boost_background: Some(
                        ProcessDynamicPriorityBoostSetting::Disabled,
                    ),
                    ..Default::default()
                }];
            }
            Profile::IoPriority => io_priority(settings, &path),
            Profile::MemoryPriority => memory_priority(settings, &path),
            Profile::GpuPriority => {
                let s = &mut settings.gpu_priority;
                s.enabled = true;
                s.foreground_priority = ProcessGpuPrioritySetting::Default;
                s.visible_window_priority = ProcessGpuPrioritySetting::Default;
                s.background_priority = ProcessGpuPrioritySetting::Default;
                s.preserve_foreground_priority = false;
                s.preserve_visible_window_priority = false;
                s.preserve_background_priority = false;
                s.exclusions = vec![ProcessExclusionRule {
                    executable_path: path,
                    gpu_foreground_priority: Some(ProcessGpuPrioritySetting::BelowNormal),
                    gpu_visible_window_priority: Some(ProcessGpuPrioritySetting::BelowNormal),
                    gpu_background_priority: Some(ProcessGpuPrioritySetting::BelowNormal),
                    ..Default::default()
                }];
            }
            Profile::BackgroundEfficiency => background_efficiency(settings, &path),
            Profile::CpuSets | Profile::ProcessorAffinity => {
                let rule = allocation_rule(&path)?;
                let s = if self.profile == Profile::CpuSets {
                    &mut settings.cpu_sets_soft
                } else {
                    &mut settings.processor_affinity_hard
                };
                s.enabled = true;
                s.rules = vec![rule];
            }
            Profile::CpuLimiter => {
                let s = &mut settings.cpu_limiter;
                s.enabled = true;
                s.rules = vec![CpuLimiterRule {
                    enabled: true,
                    executable_path: path,
                    focus_mode: ProcessRuleMode::Enabled,
                    visible_window_mode: ProcessRuleMode::Enabled,
                    background_mode: ProcessRuleMode::Enabled,
                    focus_allowed_cpu_time_percent: 50,
                    visible_window_allowed_cpu_time_percent: 50,
                    background_allowed_cpu_time_percent: 50,
                }];
            }
            Profile::AppSuspension => {
                let s = &mut settings.app_suspension;
                s.enabled = true;
                s.background_delay_seconds = 1;
                s.temporary_thaw_enabled = false;
                s.network_wake_enabled = false;
                s.audio_wake_enabled = false;
                s.suspendable_apps = vec![AppSuspensionRule {
                    enabled: true,
                    executable_path: path,
                    network_wake_enabled: false,
                    audio_wake_enabled: false,
                    network_download_threshold_bytes: 0,
                    network_download_threshold_unit: NetworkThresholdUnit::Bytes,
                    network_upload_threshold_bytes: 0,
                    network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
                }];
            }
            Profile::ByActivity => {
                let s = &mut settings.by_activity;
                s.enabled = true;
                s.idle_timeout_seconds = 5;
                s.input_detection.controller = false;
                s.power_plans.power_save_guid = self.current_plan.clone();
                s.power_plans.performance_guid = self.current_plan.clone();
            }
            Profile::ByForeground => {
                settings.by_foreground.enabled = true;
                settings.by_foreground.rules = vec![ByForegroundRule {
                    enabled: true,
                    name: "Scheduler benchmark target".into(),
                    executable_path: path,
                    power_plan_guid: self.current_plan.clone(),
                }];
            }
            Profile::ByRunningApp => {
                settings.by_running_app.enabled = true;
                settings.by_running_app.rules = vec![ByRunningAppRule {
                    enabled: true,
                    name: "Scheduler benchmark target".into(),
                    executable_path: path,
                    power_plan_guid: self.current_plan.clone(),
                }];
            }
            Profile::ByCpuLoad => {
                settings.by_cpu_load.enabled = true;
                settings.by_cpu_load.rules = vec![ByCpuLoadRule {
                    enabled: true,
                    name: "Scheduler benchmark current plan".into(),
                    comparison: CpuUsageComparison::AtOrAbove,
                    threshold_percent: 0,
                    upper_threshold_percent: None,
                    duration_seconds: 1,
                    power_plan_guid: self.current_plan.clone(),
                    else_enabled: false,
                    else_power_plan_guid: None,
                }];
            }
            Profile::ByTime => {
                settings.by_time.enabled = true;
                settings.by_time.rules = vec![ByTimeRule {
                    enabled: true,
                    name: "Scheduler benchmark current plan".into(),
                    days: WeekdaySetting::all().to_vec(),
                    start_time: "00:00".into(),
                    end_time: "23:59".into(),
                    power_plan_guid: self.current_plan.clone(),
                }];
            }
            Profile::TimerResolution => {
                settings.timer_resolution.enabled = true;
                settings.timer_resolution.rules = vec![TimerResolutionRule {
                    enabled: true,
                    executable_path: path,
                    desired_100ns: 10_000,
                }];
            }
            Profile::AdaptiveDetection | Profile::AdaptivePressureMonitor => {
                settings.adaptive_engine.enabled = true;
                settings.adaptive_engine.processor_power_policy_enabled = false;
                let s = &mut settings.adaptive_engine_process;
                s.process_priority_enabled = false;
                s.focus_process_priority = ProcessPrioritySetting::Default;
                s.visible_window_priority = ProcessPrioritySetting::Default;
                s.background_priority = ProcessPrioritySetting::Default;
                s.background_efficiency_enabled = false;
                s.focus_process_background_efficiency_mode = false;
                s.visible_window_background_efficiency_mode = false;
                s.background_efficiency_mode = false;
                s.io_priority.enabled = false;
                s.thread_priority.enabled = false;
                s.dynamic_priority_boost.enabled = false;
                s.gpu_priority.enabled = false;
                s.memory_priority_enabled = false;
                s.limit_background_processors_enabled = false;
                s.dynamic_resource_zones_enabled = false;
                s.cpu_pressure_restraint_enabled = self.profile == Profile::AdaptivePressureMonitor;
            }
            Profile::Combined => {
                background_efficiency(settings, &path);
                io_priority(settings, &path);
                memory_priority(settings, &path);
                settings.cpu_sets_soft.enabled = true;
                settings.cpu_sets_soft.rules = vec![allocation_rule(&path)?];
            }
        }
        Ok(())
    }

    pub(crate) fn evidence(
        &self,
        settings: &Settings,
        worker_running: bool,
        restoration_succeeded: Option<bool>,
    ) -> Result<Value, String> {
        Ok(json!({
            "schema_version": 1,
            "profile": self.profile.as_str(),
            "target_executable": self.target,
            "scope": "fixture_owned_processes",
            "safety_scope": "dedicated_exact_executable_path_only; adaptive_profiles_observe_without_process_actuators",
            "settings_io": "in_memory",
            "cross_session_control": settings.general.allow_cross_session_process_control,
            "enabled_features": self.enabled_features(),
            "expected_core_mask": settings.cpu_sets_soft.rules.first()
                .or_else(|| settings.processor_affinity_hard.rules.first())
                .map(|rule| rule.background_core_mask),
            "worker_running": worker_running,
            "restoration_succeeded": restoration_succeeded,
            "original_power_plan_guid": self.current_plan,
            "power_plan_scope": "evaluate_current_plan_only; no_alternate_plan_selected",
            "foreground_scope": (self.profile == Profile::ByForeground)
                .then_some("headless_unmatched_evaluation_only; no_foreground_match_claimed"),
            "gpu_scope": "pending_context_is_reported_separately_from_success",
            "settings_toml": toml::to_string(settings).map_err(|error| error.to_string())?,
        }))
    }
}

fn validate_target(path: &Path) -> Result<(), String> {
    let dedicated_name = |path: &Path| {
        path.file_name()
            .and_then(|value| value.to_str())
            .and_then(|name| name.strip_prefix("WinderustSchedulerBench-"))
            .and_then(|name| name.strip_suffix(".exe"))
            .is_some_and(|name| {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
    };
    if !path.is_absolute() || !path.is_file() || !dedicated_name(path) {
        return Err(
            "Scheduler target must be an existing absolute WinderustSchedulerBench-<id>.exe path"
                .into(),
        );
    }
    let resolved = path.canonicalize().map_err(|error| error.to_string())?;
    if !dedicated_name(&resolved) {
        return Err("Scheduler target resolves to a different executable name".into());
    }
    Ok(())
}

fn process_priority(settings: &mut Settings, path: &str) {
    let s = &mut settings.process_priority;
    s.enabled = true;
    s.foreground_priority = ProcessPrioritySetting::Default;
    s.visible_window_priority = ProcessPrioritySetting::Default;
    s.background_priority = ProcessPrioritySetting::Default;
    s.preserve_foreground_priority = false;
    s.preserve_visible_window_priority = false;
    s.preserve_background_priority = false;
    s.exclusions = vec![ProcessExclusionRule {
        executable_path: path.into(),
        process_foreground_priority: Some(ProcessPrioritySetting::BelowNormal),
        process_visible_window_priority: Some(ProcessPrioritySetting::BelowNormal),
        process_background_priority: Some(ProcessPrioritySetting::BelowNormal),
        ..Default::default()
    }];
}

fn memory_priority(settings: &mut Settings, path: &str) {
    let s = &mut settings.memory_priority;
    s.enabled = true;
    s.foreground_priority = ProcessMemoryPrioritySetting::Default;
    s.visible_window_priority = ProcessMemoryPrioritySetting::Default;
    s.background_priority = ProcessMemoryPrioritySetting::Default;
    s.preserve_foreground_priority = false;
    s.preserve_visible_window_priority = false;
    s.preserve_background_priority = false;
    s.exclusions = vec![ProcessExclusionRule {
        executable_path: path.into(),
        memory_foreground_priority: Some(ProcessMemoryPrioritySetting::Low),
        memory_visible_window_priority: Some(ProcessMemoryPrioritySetting::Low),
        memory_background_priority: Some(ProcessMemoryPrioritySetting::Low),
        ..Default::default()
    }];
}

fn io_priority(settings: &mut Settings, path: &str) {
    let s = &mut settings.io_priority;
    s.enabled = true;
    s.foreground_priority = ProcessIoPrioritySetting::Default;
    s.visible_window_priority = ProcessIoPrioritySetting::Default;
    s.background_priority = ProcessIoPrioritySetting::Default;
    s.preserve_foreground_priority = false;
    s.preserve_visible_window_priority = false;
    s.preserve_background_priority = false;
    s.exclusions = vec![ProcessExclusionRule {
        executable_path: path.into(),
        io_foreground_priority: Some(ProcessIoPrioritySetting::Low),
        io_visible_window_priority: Some(ProcessIoPrioritySetting::Low),
        io_background_priority: Some(ProcessIoPrioritySetting::Low),
        ..Default::default()
    }];
}

fn background_efficiency(settings: &mut Settings, path: &str) {
    let s = &mut settings.background_efficiency;
    s.enabled = true;
    s.foreground_efficiency_mode = false;
    s.visible_window_efficiency_mode = false;
    s.background_efficiency_mode = false;
    s.custom_rules = vec![BackgroundEfficiencyRule {
        enabled: true,
        executable_path: path.into(),
        focus_efficiency_mode: ProcessRuleMode::Enabled,
        visible_window_efficiency_mode: ProcessRuleMode::Enabled,
        background_efficiency_mode: ProcessRuleMode::Enabled,
    }];
}

fn allocation_rule(path: &str) -> Result<CpuAllocationRule, String> {
    if cpu_allocation::has_multiple_processor_groups() {
        return Err("Scheduler allocation benchmark requires one processor group".into());
    }
    let available = cpu_allocation::default_cpu_mask();
    if available == 0 {
        return Err("Scheduler allocation benchmark has no available processor".into());
    }
    let mask = 1u64 << available.trailing_zeros();
    Ok(CpuAllocationRule {
        enabled: true,
        executable_path: path.into(),
        focus_core_mask: mask,
        visible_window_core_mask: mask,
        background_core_mask: mask,
    })
}

pub(crate) fn status_evidence(status: &RuntimeStatusSnapshot) -> Value {
    let s = &status.feature_status;
    json!({
        "generation": status.generation,
        "worker_error": status.worker_error,
        "process_priority": {"adjusted": s.process_priority.adjusted_processes, "failed": s.process_priority.failed_processes, "error": s.process_priority.last_error},
        "thread_priority": {"adjusted": s.thread_priority.adjusted_processes, "adjusted_threads": s.thread_priority.adjusted_threads, "failed": s.thread_priority.failed_processes, "error": s.thread_priority.last_error},
        "dynamic_priority_boost": {"adjusted": s.dynamic_priority_boost.adjusted_processes, "failed": s.dynamic_priority_boost.failed_processes, "error": s.dynamic_priority_boost.last_error},
        "io_priority": {"adjusted": s.io_priority.adjusted_processes, "failed": s.io_priority.failed_processes, "error": s.io_priority.last_error},
        "memory_priority": {"adjusted": s.memory_priority.adjusted_processes, "failed": s.memory_priority.failed_processes, "error": s.memory_priority.last_error},
        "gpu_priority": {"adjusted": s.gpu_priority.adjusted_processes, "pending": s.gpu_priority.pending_processes, "denied": s.gpu_priority.denied_processes, "failed": s.gpu_priority.failed_processes, "error": s.gpu_priority.last_error},
        "background_efficiency": {"adjusted": s.background_efficiency.throttled_processes, "unsupported": s.background_efficiency.unsupported, "failed": s.background_efficiency.failed_processes, "error": s.background_efficiency.last_error},
        "cpu_sets_soft": {"adjusted": s.cpu_sets_soft.adjusted_processes, "failed": s.cpu_sets_soft.failed_processes, "error": s.cpu_sets_soft.last_error},
        "processor_affinity_hard": {"adjusted": s.processor_affinity_hard.adjusted_processes, "failed": s.processor_affinity_hard.failed_processes, "error": s.processor_affinity_hard.last_error},
        "cpu_limiter": {"adjusted": s.cpu_limiter.limited_processes, "tracked": s.cpu_limiter.tracked_processes, "failed": s.cpu_limiter.failed_processes, "error": s.cpu_limiter.last_error},
        "app_suspension": {"adjusted": s.app_suspension.suspended_processes, "suspended_process_ids": s.app_suspension.suspended_process_ids, "unsupported": s.app_suspension.unsupported, "status_unknown": s.app_suspension.status_unknown, "failed": s.app_suspension.failed_actions, "error": s.app_suspension.last_error},
        "adaptive_engine_process": {"adjusted": s.adaptive_engine_process.adjusted_processes, "scanned": s.adaptive_engine_process.scanned_processes, "failed": s.adaptive_engine_process.failed_processes, "error": s.adaptive_engine_process.last_error},
        "by_running_app": {"active_rule": s.by_running_app.active_rule, "active_process": s.by_running_app.active_process, "target_guid": s.by_running_app.target_guid},
        "power_plan": {"current_guid": status.power_plan_status.current_guid, "target_guid": status.power_plan_status.target_guid, "decision_state": status.power_plan_status.decision_state.map(|state| format!("{state:?}")), "rule_index": status.power_plan_status.rule_index, "reason": status.power_plan_status.reason, "apply_failed": status.power_plan_status.apply_failed},
        "features_debug": format!("{s:?}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn options(profile: Profile) -> Options {
        Options {
            profile,
            target: std::env::temp_dir().join(format!(
                "WinderustSchedulerBench-test-{}-{}.exe",
                std::process::id(),
                profile.as_str()
            )),
            current_plan: None,
        }
    }

    fn action_status(adjusted: usize, failed: usize, error: Option<&str>) -> RuntimeStatusSnapshot {
        let mut status = RuntimeStatusSnapshot::default();
        let features = Arc::make_mut(&mut status.feature_status);
        macro_rules! result {
            ($feature:ident, $count:ident, $failures:ident) => {{
                let result = &mut features.$feature;
                result.enabled = true;
                result.$count = adjusted;
                result.$failures = failed;
                result.last_error = error.map(String::from);
            }};
        }
        result!(process_priority, adjusted_processes, failed_processes);
        result!(thread_priority, adjusted_processes, failed_processes);
        result!(dynamic_priority_boost, adjusted_processes, failed_processes);
        result!(io_priority, adjusted_processes, failed_processes);
        result!(memory_priority, adjusted_processes, failed_processes);
        result!(gpu_priority, adjusted_processes, failed_processes);
        result!(background_efficiency, throttled_processes, failed_processes);
        result!(cpu_sets_soft, adjusted_processes, failed_processes);
        result!(
            processor_affinity_hard,
            adjusted_processes,
            failed_processes
        );
        result!(cpu_limiter, limited_processes, failed_processes);
        result!(app_suspension, suspended_processes, failed_actions);
        features.thread_priority.adjusted_threads = adjusted;
        features.cpu_limiter.tracked_processes = adjusted;
        status
    }

    #[test]
    fn action_readiness_requires_a_nonzero_successful_result() {
        let initial = action_status(0, 0, None);
        let complete = action_status(1, 0, None);
        let failed = action_status(1, 1, None);
        let error = action_status(1, 0, Some("apply failed"));
        for profile in [
            Profile::ProcessPriority,
            Profile::ThreadPriority,
            Profile::DynamicPriorityBoost,
            Profile::IoPriority,
            Profile::MemoryPriority,
            Profile::GpuPriority,
            Profile::BackgroundEfficiency,
            Profile::CpuSets,
            Profile::ProcessorAffinity,
            Profile::CpuLimiter,
            Profile::AppSuspension,
            Profile::Combined,
        ] {
            let o = options(profile);
            assert!(
                !o.initial_pass_observed(&initial),
                "{profile:?} started before acting"
            );
            assert!(
                o.initial_pass_observed(&complete),
                "{profile:?} never became ready"
            );
            assert!(
                !o.initial_pass_observed(&failed),
                "{profile:?} accepted a failed action"
            );
            assert!(
                !o.initial_pass_observed(&error),
                "{profile:?} accepted an action error"
            );
        }
        assert!(options(Profile::EnabledNoMatch).initial_pass_observed(&initial));
    }

    #[test]
    fn action_readiness_rejects_incomplete_or_unsupported_feature_results() {
        let complete = action_status(1, 0, None);
        for condition in [
            "threads",
            "limiter",
            "suspension_unknown",
            "suspension_unsupported",
            "efficiency_unsupported",
            "efficiency_denied",
            "gpu_denied",
            "gpu_suppressed",
        ] {
            let mut invalid = complete.clone();
            let features = Arc::make_mut(&mut invalid.feature_status);
            let profile = match condition {
                "threads" => {
                    features.thread_priority.adjusted_threads = 0;
                    Profile::ThreadPriority
                }
                "limiter" => {
                    features.cpu_limiter.tracked_processes = 0;
                    Profile::CpuLimiter
                }
                "suspension_unknown" => {
                    features.app_suspension.status_unknown = true;
                    Profile::AppSuspension
                }
                "suspension_unsupported" => {
                    features.app_suspension.unsupported = true;
                    Profile::AppSuspension
                }
                "efficiency_unsupported" => {
                    features.background_efficiency.unsupported = true;
                    Profile::BackgroundEfficiency
                }
                "efficiency_denied" => {
                    features.background_efficiency.access_denied_processes = 1;
                    Profile::BackgroundEfficiency
                }
                "gpu_denied" => {
                    features.gpu_priority.denied_processes = 1;
                    Profile::GpuPriority
                }
                "gpu_suppressed" => {
                    features.gpu_priority.suppressed_processes = 1;
                    Profile::GpuPriority
                }
                _ => unreachable!(),
            };
            assert!(
                !options(profile).initial_pass_observed(&invalid),
                "accepted {condition}"
            );
        }
        let mut pending = action_status(0, 0, None);
        Arc::make_mut(&mut pending.feature_status)
            .gpu_priority
            .pending_processes = 1;
        assert!(options(Profile::GpuPriority).initial_pass_observed(&pending));
        for missing in ["efficiency", "io", "memory", "cpu_sets"] {
            let mut incomplete = complete.clone();
            let features = Arc::make_mut(&mut incomplete.feature_status);
            match missing {
                "efficiency" => features.background_efficiency.throttled_processes = 0,
                "io" => features.io_priority.adjusted_processes = 0,
                "memory" => features.memory_priority.adjusted_processes = 0,
                "cpu_sets" => features.cpu_sets_soft.adjusted_processes = 0,
                _ => unreachable!(),
            }
            assert!(
                !options(Profile::Combined).initial_pass_observed(&incomplete),
                "Combined accepted missing {missing}"
            );
        }
    }

    #[test]
    fn pressure_monitor_readiness_requires_a_completed_scan_without_mutations() {
        let o = options(Profile::AdaptivePressureMonitor);
        let mut complete = RuntimeStatusSnapshot::default();
        let features = Arc::make_mut(&mut complete.feature_status);
        features.bottleneck_classifier.enabled = true;
        features.adaptive_engine_process.enabled = true;
        features.adaptive_engine_process.message = "Adaptive Engine active.".into();
        features.adaptive_engine_process.scanned_processes = 1;
        assert!(o.initial_pass_observed(&complete));
        for condition in [
            "paused",
            "unscanned",
            "mutated",
            "failed",
            "error",
            "classifier_disabled",
        ] {
            let mut invalid = complete.clone();
            let features = Arc::make_mut(&mut invalid.feature_status);
            let monitor = &mut features.adaptive_engine_process;
            match condition {
                "paused" => monitor.message = "Paused: visible windows are unavailable.".into(),
                "unscanned" => monitor.scanned_processes = 0,
                "mutated" => monitor.adjusted_processes = 1,
                "failed" => monitor.failed_processes = 1,
                "error" => monitor.last_error = Some("scan failed".into()),
                "classifier_disabled" => features.bottleneck_classifier.enabled = false,
                _ => unreachable!(),
            }
            assert!(!o.initial_pass_observed(&invalid), "accepted {condition}");
        }
    }

    fn completed_power_decision(profile: Profile) -> (Options, RuntimeStatusSnapshot) {
        let mut o = options(profile);
        o.current_plan = Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into());
        let mut status = RuntimeStatusSnapshot::default();
        let plan = Arc::make_mut(&mut status.power_plan_status);
        plan.current_guid = o.current_plan.as_ref().map(|guid| guid.to_uppercase());
        plan.target_guid = o.current_plan.clone();
        let (state, rule_index, reason) = match profile {
            Profile::ByActivity => (
                DecisionState::ByActivityIdle,
                None,
                "User input has been idle past the configured timeout.",
            ),
            Profile::ByRunningApp => (
                DecisionState::ByRunningApp,
                Some(0),
                "worker.exe is running and matched By Running App rule 'Scheduler benchmark target'.",
            ),
            Profile::ByCpuLoad => (
                DecisionState::ByCpuLoad,
                Some(0),
                "CPU load is 0.0% and matched rule 'Scheduler benchmark current plan'.",
            ),
            Profile::ByTime => (
                DecisionState::ByTime,
                Some(0),
                "By Time rule 'Scheduler benchmark current plan' is active.",
            ),
            Profile::ByForeground => {
                plan.target_guid = None;
                (
                    DecisionState::NoPowerPlanSelected,
                    None,
                    "Using the configured default Active plan. Select the required power plan first.",
                )
            }
            _ => panic!("Expected a power-plan benchmark profile"),
        };
        plan.decision_state = Some(state);
        plan.rule_index = rule_index;
        plan.reason = Some(reason.into());
        if profile == Profile::ByRunningApp {
            let running = &mut Arc::make_mut(&mut status.feature_status).by_running_app;
            running.active_rule = Some("Scheduler benchmark target".into());
            running.active_process = o
                .target
                .file_name()
                .map(|name| name.to_string_lossy().into());
            running.target_guid = o.current_plan.clone();
        }
        (o, status)
    }

    #[test]
    fn power_profile_readiness_waits_for_its_completed_decision() {
        for profile in [
            Profile::ByActivity,
            Profile::ByRunningApp,
            Profile::ByCpuLoad,
            Profile::ByTime,
        ] {
            let (o, complete) = completed_power_decision(profile);
            let mut starting = RuntimeStatusSnapshot::default();
            Arc::make_mut(&mut starting.power_plan_status).current_guid = o.current_plan.clone();
            assert!(
                !o.initial_pass_observed(&starting),
                "{profile:?} started too early"
            );
            assert!(
                o.initial_pass_observed(&complete),
                "{profile:?} never became ready"
            );

            for field in [
                "current", "target", "state", "rule", "reason", "apply", "worker",
            ] {
                let mut invalid = complete.clone();
                let plan = Arc::make_mut(&mut invalid.power_plan_status);
                match field {
                    "current" => plan.current_guid = Some("different-plan".into()),
                    "target" => plan.target_guid = None,
                    "state" => plan.decision_state = Some(DecisionState::NoPowerPlanSelected),
                    "rule" => plan.rule_index = Some(1),
                    "reason" => {
                        plan.reason = Some("Using the configured default Active plan.".into())
                    }
                    "apply" => plan.apply_failed = true,
                    "worker" => invalid.worker_error = Some("worker stopped".into()),
                    _ => unreachable!(),
                }
                assert!(
                    !o.initial_pass_observed(&invalid),
                    "{profile:?} accepted invalid {field}"
                );
            }
            let mut missing_original = o.clone();
            missing_original.current_plan = None;
            assert!(!missing_original.initial_pass_observed(&complete));
        }
    }

    #[test]
    fn activity_readiness_accepts_idle_or_active_with_the_matching_reason() {
        let (o, complete) = completed_power_decision(Profile::ByActivity);
        let idle_reason = "User input has been idle past the configured timeout.";
        let active_reason = "Recent user input detected; using the Active plan.";
        for (state, reason, expected) in [
            (DecisionState::ByActivityIdle, idle_reason, true),
            (DecisionState::ByActivityActive, active_reason, true),
            (DecisionState::ByActivityIdle, active_reason, false),
            (DecisionState::ByActivityActive, idle_reason, false),
            (DecisionState::NoPowerPlanSelected, active_reason, false),
            (
                DecisionState::ByActivityActive,
                "Using the configured default Active plan.",
                false,
            ),
        ] {
            let mut status = complete.clone();
            let plan = Arc::make_mut(&mut status.power_plan_status);
            plan.decision_state = Some(state);
            plan.reason = Some(reason.into());
            assert_eq!(
                o.initial_pass_observed(&status),
                expected,
                "{state:?}: {reason}"
            );
        }
    }

    #[test]
    fn running_app_readiness_rejects_a_different_process_or_rule() {
        let (o, complete) = completed_power_decision(Profile::ByRunningApp);
        for field in ["process", "rule", "target"] {
            let mut invalid = complete.clone();
            let running = &mut Arc::make_mut(&mut invalid.feature_status).by_running_app;
            match field {
                "process" => running.active_process = Some("unrelated.exe".into()),
                "rule" => running.active_rule = Some("Unrelated rule".into()),
                "target" => running.target_guid = Some("different-plan".into()),
                _ => unreachable!(),
            }
            assert!(!o.initial_pass_observed(&invalid));
        }
    }

    #[test]
    fn headless_foreground_readiness_requires_a_completed_unmatched_evaluation() {
        let (o, complete) = completed_power_decision(Profile::ByForeground);
        assert!(o.initial_pass_observed(&complete));
        let evidence = status_evidence(&complete);
        assert_eq!(
            evidence["power_plan"]["decision_state"],
            "NoPowerPlanSelected"
        );
        assert!(evidence["power_plan"]["rule_index"].is_null());
        for field in ["state", "target", "rule", "reason", "current", "apply"] {
            let mut invalid = complete.clone();
            let plan = Arc::make_mut(&mut invalid.power_plan_status);
            match field {
                "state" => plan.decision_state = None,
                "target" => plan.target_guid = o.current_plan.clone(),
                "rule" => plan.rule_index = Some(0),
                "reason" => plan.reason = None,
                "current" => plan.current_guid = None,
                "apply" => plan.apply_failed = true,
                _ => unreachable!(),
            }
            assert!(
                !o.initial_pass_observed(&invalid),
                "accepted invalid {field}"
            );
        }
    }

    #[test]
    fn scheduler_cli_is_opt_in_and_rejects_unsafe_targets() {
        assert!(Options::parse_arguments(&[]).unwrap().is_none());
        for args in [
            vec!["--framework-benchmark-target", "anything.exe"],
            vec!["--framework-benchmark-scheduler", "process-priority"],
            vec!["--framework-benchmark-scheduler", "memory-trim"],
            vec![
                "--framework-benchmark-scheduler",
                "disabled",
                "--framework-benchmark-target",
                "C:\\Windows\\explorer.exe",
            ],
            vec![
                "--framework-benchmark-scheduler",
                "disabled",
                "--framework-benchmark-scheduler",
                "disabled",
            ],
        ] {
            assert!(Options::parse_arguments(
                &args.into_iter().map(String::from).collect::<Vec<_>>()
            )
            .is_err());
        }
    }

    #[test]
    fn priority_profile_cannot_apply_defaults_to_unrelated_processes() {
        let o = options(Profile::ProcessPriority);
        std::fs::write(&o.target, []).unwrap();
        let mut settings = Settings::default();
        settings.background_efficiency.enabled = true;
        settings.on_battery = Some(Box::new(settings.clone()));
        o.apply(&mut settings).unwrap();
        std::fs::remove_file(&o.target).unwrap();
        assert!(settings.process_priority.enabled);
        assert_eq!(
            settings.process_priority.background_priority,
            ProcessPrioritySetting::Default
        );
        assert_eq!(
            settings.process_priority.foreground_priority,
            ProcessPrioritySetting::Default
        );
        assert_eq!(settings.process_priority.exclusions.len(), 1);
        assert!(settings
            .process_priority
            .override_for("C:\\unrelated.exe", false, false)
            .is_none());
        assert_eq!(
            settings
                .process_priority
                .override_for(&o.target.to_string_lossy(), false, false),
            Some(Some(ProcessPrioritySetting::BelowNormal))
        );
        assert!(!settings.background_efficiency.enabled);
        assert!(!settings.general.allow_cross_session_process_control);
        assert!(!settings.general.startup_with_windows);
        assert!(!settings.general.check_for_updates);
        assert!(settings.on_battery.is_none());
    }

    #[test]
    fn adaptive_monitor_has_no_process_or_power_plan_actuators() {
        let o = options(Profile::AdaptivePressureMonitor);
        std::fs::write(&o.target, []).unwrap();
        let mut settings = Settings::default();
        o.apply(&mut settings).unwrap();
        std::fs::remove_file(&o.target).unwrap();
        let p = &settings.adaptive_engine_process;
        assert!(settings.general.enabled && settings.adaptive_engine.enabled);
        assert!(p.cpu_pressure_restraint_enabled);
        assert!(!settings.adaptive_engine.processor_power_policy_enabled);
        assert!(
            !p.process_priority_enabled
                && !p.background_efficiency_enabled
                && !p.memory_priority_enabled
        );
        assert!(
            !p.io_priority.enabled
                && !p.thread_priority.enabled
                && !p.dynamic_priority_boost.enabled
                && !p.gpu_priority.enabled
        );
        assert!(!p.limit_background_processors_enabled && !p.dynamic_resource_zones_enabled);
        let evidence = o.evidence(&settings, true, None).unwrap();
        assert!(evidence["restoration_succeeded"].is_null());
        assert_eq!(
            toml::from_str::<Settings>(evidence["settings_toml"].as_str().unwrap()).unwrap(),
            settings
        );
    }

    #[test]
    fn static_profiles_only_claim_the_dedicated_executable() {
        for profile in [
            Profile::ThreadPriority,
            Profile::DynamicPriorityBoost,
            Profile::IoPriority,
            Profile::MemoryPriority,
            Profile::GpuPriority,
            Profile::BackgroundEfficiency,
        ] {
            let o = options(profile);
            std::fs::write(&o.target, []).unwrap();
            let mut settings = Settings::default();
            o.apply(&mut settings).unwrap();
            std::fs::remove_file(&o.target).unwrap();
            let unrelated = "C:\\unrelated.exe";
            assert!(settings
                .thread_priority
                .override_for(unrelated, false, false)
                .is_none());
            assert!(settings
                .dynamic_priority_boost
                .override_for(unrelated, false, false)
                .is_none());
            assert!(settings
                .io_priority
                .override_for(unrelated, false, false)
                .is_none());
            assert!(settings
                .memory_priority
                .override_for(unrelated, false, false)
                .is_none());
            assert!(settings
                .gpu_priority
                .override_for(unrelated, false, false)
                .is_none());
            let serialized = toml::Value::try_from(&settings).unwrap();
            for key in [
                "thread_priority",
                "io_priority",
                "memory_priority",
                "gpu_priority",
            ] {
                if serialized[key]["enabled"].as_bool() == Some(true) {
                    for tier in [
                        "foreground_priority",
                        "visible_window_priority",
                        "background_priority",
                    ] {
                        assert_eq!(serialized[key][tier].as_str(), Some("default"));
                    }
                    assert_eq!(serialized[key]["exclusions"].as_array().unwrap().len(), 1);
                }
            }
            if profile == Profile::BackgroundEfficiency {
                let s = settings.background_efficiency;
                assert!(
                    !s.foreground_efficiency_mode
                        && !s.visible_window_efficiency_mode
                        && !s.background_efficiency_mode
                );
                assert_eq!(s.custom_rules.len(), 1);
                assert_eq!(
                    s.custom_rules[0].executable_path,
                    o.target.to_string_lossy()
                );
            }
            if profile == Profile::DynamicPriorityBoost {
                let s = settings.dynamic_priority_boost;
                assert_eq!(
                    s.foreground_boost,
                    ProcessDynamicPriorityBoostSetting::Default
                );
                assert_eq!(
                    s.visible_window_boost,
                    ProcessDynamicPriorityBoostSetting::Default
                );
                assert_eq!(
                    s.background_boost,
                    ProcessDynamicPriorityBoostSetting::Default
                );
                assert_eq!(s.exclusions.len(), 1);
            }
        }
    }
}
