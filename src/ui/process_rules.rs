use crate::config::{
    is_valid_executable_name, ByFocusRule, ByFocusSettings, ByRunningAppRule, ByRunningAppSettings,
    ExecutableSelector,
};
use crate::config::{
    same_rule_executable_path, CpuLimiterRule, CpuLimiterSettings, ProcessRuleMode,
};
use crate::config::{
    AppSuspensionRule, AppSuspensionSettings, MemoryTrimSettings, NetworkThresholdUnit,
    ProcessExclusionRule, TimerResolutionRule, TimerResolutionSettings,
};
use crate::control::instance_limits::validate_path;
use crate::cpu_limiter;
use crate::foreground::{
    process_list::{normalize_path_slashes, same_executable_name},
    same_executable_path,
};
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

pub(super) fn can_add_exact_process_candidate(
    process: &str,
    contains_process: impl FnOnce(&str) -> bool,
    is_builtin_excluded: impl FnOnce(&str) -> bool,
) -> bool {
    let process_path = Path::new(process);
    let process_name = process_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(process);
    process_path.is_absolute() && !contains_process(process) && !is_builtin_excluded(process_name)
}

pub(crate) fn can_add_cpu_limiter_process(settings: &CpuLimiterSettings, process: &str) -> bool {
    can_add_exact_process_candidate(
        process,
        |process| {
            settings
                .rules
                .iter()
                .any(|rule| same_rule_executable_path(&rule.executable_path, process))
        },
        cpu_limiter::is_builtin_excluded,
    )
}

pub(crate) fn new_cpu_limiter_rule(process: &str) -> CpuLimiterRule {
    CpuLimiterRule {
        enabled: true,
        executable_path: normalize_path_slashes(process).into_owned(),
        focus_mode: ProcessRuleMode::Default,
        visible_window_mode: ProcessRuleMode::Default,
        background_mode: ProcessRuleMode::Default,
        focus_allowed_cpu_time_percent: 50,
        visible_window_allowed_cpu_time_percent: 50,
        background_allowed_cpu_time_percent: 50,
        affinity_limit: None,
    }
}

pub(super) fn exact_path_input(input: &str) -> &str {
    let input = input.trim_start();
    if let Some(quoted) = input
        .trim_end()
        .strip_prefix('"')
        .and_then(|path| path.strip_suffix('"'))
    {
        return quoted;
    }
    if input.starts_with(r"\\?\") {
        input
    } else {
        input.trim_ascii_end()
    }
}

pub(super) fn picked_path_input(path: String) -> String {
    if exact_path_input(&path) == path {
        path
    } else {
        format!("\"{path}\"")
    }
}

pub(super) fn power_rule_path(input: &str) -> Option<&str> {
    let path = exact_path_input(input);
    validate_path(path).is_ok().then_some(path)
}

pub(super) fn addable_by_focus_path<'a>(
    settings: &ByFocusSettings,
    path: &'a str,
) -> Option<&'a str> {
    validate_path(path).ok()?;
    (!settings
        .rules
        .iter()
        .any(|rule| selector_path_matches(&rule.selector, path)))
    .then_some(path)
}

pub(super) fn can_add_by_focus_name(settings: &ByFocusSettings, name: &str) -> bool {
    can_add_executable_name(settings.rules.iter().map(|rule| &rule.selector), name)
}

pub(crate) fn new_by_focus_rule(process: &str, power_plan_guid: Option<String>) -> ByFocusRule {
    let executable_path = normalize_path_slashes(process).into_owned();
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

pub(super) fn addable_by_running_app_path<'a>(
    settings: &ByRunningAppSettings,
    path: &'a str,
) -> Option<&'a str> {
    validate_path(path).ok()?;
    can_add_exact_process_candidate(
        path,
        |path| {
            settings
                .rules
                .iter()
                .any(|rule| selector_path_matches(&rule.selector, path))
        },
        crate::features::power_plan_control::by_running_app::is_builtin_excluded,
    )
    .then_some(path)
}

pub(super) fn selector_path_matches(selector: &ExecutableSelector, path: &str) -> bool {
    selector
        .exact_path()
        .is_some_and(|target| same_executable_path(Path::new(target.trim_start()), Path::new(path)))
}

pub(super) fn can_add_by_running_app_name(settings: &ByRunningAppSettings, name: &str) -> bool {
    !crate::features::power_plan_control::by_running_app::is_builtin_excluded(name)
        && can_add_executable_name(settings.rules.iter().map(|rule| &rule.selector), name)
}

pub(super) fn can_add_executable_name<'a>(
    mut selectors: impl Iterator<Item = &'a ExecutableSelector>,
    name: &str,
) -> bool {
    is_valid_executable_name(name)
        && !selectors.any(|selector| {
            matches!(selector, ExecutableSelector::ExactName { executable_name }
                if same_executable_name(executable_name, name))
        })
}

pub(crate) fn new_by_running_app_rule(
    process: &str,
    power_plan_guid: Option<String>,
) -> ByRunningAppRule {
    let executable_path = normalize_path_slashes(process).into_owned();
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
    can_add_exact_process_candidate(
        process,
        |process| {
            settings
                .exclusions
                .iter()
                .any(|rule| same_rule_executable_path(&rule.executable_path, process))
        },
        memory_trim::is_builtin_excluded,
    )
}

pub(crate) fn new_process_exclusion_rule(process: &str) -> ProcessExclusionRule {
    ProcessExclusionRule {
        executable_path: normalize_path_slashes(process).into_owned(),
        ..Default::default()
    }
}

pub(crate) fn can_add_app_suspension_process(
    settings: &AppSuspensionSettings,
    process: &str,
) -> bool {
    can_add_exact_process_candidate(
        process,
        |process| settings.contains_suspendable_app(process),
        app_suspension::is_builtin_excluded,
    )
}

pub(crate) fn new_app_suspension_rule(process: &str) -> AppSuspensionRule {
    AppSuspensionRule {
        enabled: true,
        executable_path: normalize_path_slashes(process).into_owned(),
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
    can_add_exact_process_candidate(
        process,
        |process| settings.contains_rule_for(process),
        |_| false,
    )
}

pub(crate) fn new_timer_resolution_rule(process: &str, desired_100ns: u32) -> TimerResolutionRule {
    TimerResolutionRule {
        enabled: true,
        executable_path: normalize_path_slashes(process).into_owned(),
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
