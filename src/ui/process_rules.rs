use crate::config::{
    AppSuspensionRule, AppSuspensionSettings, MemoryTrimSettings, NetworkThresholdUnit,
    ProcessExclusionRule, TimerResolutionRule, TimerResolutionSettings,
};
use crate::config::{
    ByForegroundRule, ByForegroundSettings, ByRunningAppRule, ByRunningAppSettings,
};
use crate::config::{CpuLimiterRule, CpuLimiterSettings, ProcessRuleMode};
use crate::cpu_limiter;
use crate::foreground::executable_path_key;
use crate::foreground::same_executable_path;
use crate::{app_suspension, memory_trim};
use std::path::Path;

pub(crate) fn can_add_process_candidate(
    process: &str,
    contains_process: impl FnOnce(&str) -> bool,
    is_builtin_excluded: impl FnOnce(&str) -> bool,
) -> bool {
    let process = process.trim();
    let process_path = Path::new(process);
    let process_name = process_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(process);
    process_path.is_absolute() && !contains_process(process) && !is_builtin_excluded(process_name)
}

pub(crate) fn can_add_cpu_limiter_process(settings: &CpuLimiterSettings, process: &str) -> bool {
    can_add_process_candidate(
        process,
        |process| {
            settings
                .rules
                .iter()
                .any(|rule| process_setting_matches(&rule.executable_path, process))
        },
        cpu_limiter::is_builtin_excluded,
    )
}

pub(crate) fn new_cpu_limiter_rule(process: &str) -> CpuLimiterRule {
    CpuLimiterRule {
        enabled: true,
        executable_path: executable_path_key(Path::new(process)),
        focus_mode: ProcessRuleMode::Default,
        visible_window_mode: ProcessRuleMode::Default,
        background_mode: ProcessRuleMode::Default,
        focus_allowed_cpu_time_percent: 50,
        visible_window_allowed_cpu_time_percent: 50,
        background_allowed_cpu_time_percent: 50,
    }
}

pub(crate) fn process_setting_matches(configured_process: &str, process_name: &str) -> bool {
    let configured_process = configured_process.trim();
    !configured_process.is_empty()
        && same_executable_path(
            Path::new(configured_process),
            Path::new(process_name.trim()),
        )
}

pub(crate) fn can_add_foreground_process(settings: &ByForegroundSettings, process: &str) -> bool {
    can_add_process_candidate(
        process,
        |process| {
            settings
                .rules
                .iter()
                .any(|rule| process_setting_matches(&rule.executable_path, process))
        },
        |_| false,
    )
}

pub(crate) fn new_foreground_rule(
    process: &str,
    power_plan_guid: Option<String>,
) -> ByForegroundRule {
    let executable_path = executable_path_key(Path::new(process));
    ByForegroundRule {
        enabled: true,
        name: Path::new(process)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(process)
            .to_owned(),
        executable_path,
        power_plan_guid,
    }
}

pub(crate) fn can_add_by_running_app_process(
    settings: &ByRunningAppSettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| {
            settings
                .rules
                .iter()
                .any(|rule| process_setting_matches(&rule.executable_path, process))
        },
        crate::features::power_plan_control::by_running_app::is_builtin_excluded,
    )
}

pub(crate) fn new_by_running_app_rule(
    process: &str,
    power_plan_guid: Option<String>,
) -> ByRunningAppRule {
    let executable_path = executable_path_key(Path::new(process));
    ByRunningAppRule {
        enabled: true,
        name: Path::new(process)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(process)
            .to_owned(),
        executable_path,
        power_plan_guid,
    }
}

pub(crate) fn can_add_memory_trim_exclusion(settings: &MemoryTrimSettings, process: &str) -> bool {
    can_add_process_candidate(
        process,
        |process| {
            settings.exclusion_enabled_for(process)
                || settings
                    .exclusions
                    .iter()
                    .any(|rule| process_setting_matches(&rule.executable_path, process))
        },
        memory_trim::is_builtin_excluded,
    )
}

pub(crate) fn new_process_exclusion_rule(process: &str) -> ProcessExclusionRule {
    ProcessExclusionRule {
        executable_path: executable_path_key(Path::new(process)),
        ..Default::default()
    }
}

pub(crate) fn can_add_app_suspension_process(
    settings: &AppSuspensionSettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_suspendable_app(process),
        app_suspension::is_builtin_excluded,
    )
}

pub(crate) fn new_app_suspension_rule(process: &str) -> AppSuspensionRule {
    AppSuspensionRule {
        enabled: true,
        executable_path: executable_path_key(Path::new(process)),
        network_wake_enabled: true,
        audio_wake_enabled: true,
        network_download_threshold_bytes: 1,
        network_download_threshold_unit: NetworkThresholdUnit::Bytes,
        network_upload_threshold_bytes: 0,
        network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
    }
}

pub(crate) fn can_add_timer_resolution_process(
    settings: &TimerResolutionSettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_rule_for(process),
        |_| false,
    )
}

pub(crate) fn new_timer_resolution_rule(process: &str, desired_100ns: u32) -> TimerResolutionRule {
    TimerResolutionRule {
        enabled: true,
        executable_path: executable_path_key(Path::new(process)),
        desired_100ns,
    }
}
