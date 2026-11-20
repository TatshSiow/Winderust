use crate::config::{
    is_valid_executable_name, ByFocusRule, ByFocusSettings, ByRunningAppRule, ByRunningAppSettings,
    ExecutableSelector,
};
use crate::config::{
    AppSuspensionRule, AppSuspensionSettings, MemoryTrimSettings, NetworkThresholdUnit,
    ProcessExclusionRule, TimerResolutionRule, TimerResolutionSettings,
};
use crate::config::{CpuLimiterRule, CpuLimiterSettings, ProcessRuleMode};
use crate::cpu_limiter;
use crate::foreground::executable_path_key;
use crate::foreground::{same_executable_path, same_process_name};
use crate::{app_suspension, memory_trim};
use rust_i18n::t;
use std::path::Path;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum MatchBy {
    #[default]
    ExecutablePath,
    ExecutableName,
}

impl std::fmt::Display for MatchBy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(match self {
            Self::ExecutablePath => "common.executable_path",
            Self::ExecutableName => "common.executable_name",
        }))
    }
}

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

pub(crate) fn can_add_by_focus_process(settings: &ByFocusSettings, process: &str) -> bool {
    can_add_process_candidate(
        process,
        |process| {
            settings
                .rules
                .iter()
                .any(|rule| selector_path_matches(&rule.selector, process))
        },
        |_| false,
    )
}

pub(super) fn can_add_by_focus_name(settings: &ByFocusSettings, name: &str) -> bool {
    can_add_executable_name(settings.rules.iter().map(|rule| &rule.selector), name)
}

pub(crate) fn new_by_focus_rule(process: &str, power_plan_guid: Option<String>) -> ByFocusRule {
    let executable_path = executable_path_key(Path::new(process));
    ByFocusRule {
        enabled: true,
        name: Path::new(process)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(process)
            .to_owned(),
        selector: ExecutableSelector::ExactPath { executable_path },
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
                .any(|rule| selector_path_matches(&rule.selector, process))
        },
        crate::features::power_plan_control::by_running_app::is_builtin_excluded,
    )
}

pub(super) fn selector_path_matches(selector: &ExecutableSelector, path: &str) -> bool {
    selector
        .exact_path()
        .is_some_and(|target| process_setting_matches(target, path))
}

pub(super) fn can_add_by_running_app_name(settings: &ByRunningAppSettings, name: &str) -> bool {
    !crate::features::power_plan_control::by_running_app::is_builtin_excluded(name)
        && can_add_executable_name(settings.rules.iter().map(|rule| &rule.selector), name)
}

fn can_add_executable_name<'a>(
    mut selectors: impl Iterator<Item = &'a ExecutableSelector>,
    name: &str,
) -> bool {
    is_valid_executable_name(name)
        && !selectors.any(|selector| {
            matches!(selector, ExecutableSelector::ExactName { executable_name }
                if same_process_name(executable_name, name))
        })
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
        selector: ExecutableSelector::ExactPath { executable_path },
        power_plan_guid,
    }
}

pub(crate) fn can_add_memory_trim_exclusion(settings: &MemoryTrimSettings, process: &str) -> bool {
    can_add_process_candidate(
        process,
        |process| {
            settings
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

#[test]
fn memory_trim_add_rejects_enabled_and_disabled_equivalent_exclusions() {
    let mut settings = MemoryTrimSettings::default();
    assert!(can_add_memory_trim_exclusion(
        &settings,
        "C:/Apps/example.exe"
    ));
    settings
        .exclusions
        .push(new_process_exclusion_rule("C:/Apps/example.exe"));
    for enabled in [true, false] {
        settings.exclusions[0].enabled = enabled;
        assert!(!can_add_memory_trim_exclusion(
            &settings,
            "c:/apps/EXAMPLE.exe"
        ));
    }
}
