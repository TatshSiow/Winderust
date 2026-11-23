use super::*;

pub(super) fn automation_refresh_interval(
    hidden_to_tray: bool,
    adaptive_engine_enabled: bool,
    hidden_interval: Duration,
) -> Duration {
    // ponytail: one global saver cadence; add per-feature intervals only if a real workflow needs it.
    if adaptive_engine_enabled {
        hidden_interval.max(ADAPTIVE_ENGINE_AUTOMATION_REFRESH_INTERVAL)
    } else if hidden_to_tray {
        hidden_interval.max(HIDDEN_AUTOMATION_REFRESH_INTERVAL)
    } else {
        VISIBLE_AUTOMATION_REFRESH_INTERVAL
    }
}

pub(super) fn process_appearance_refresh_interval(
    hidden_to_tray: bool,
    adaptive_engine_enabled: bool,
    cpu_limiter_active: bool,
) -> Duration {
    if cpu_limiter_active {
        PROCESS_APPEARANCE_SCAN_INTERVAL
    } else {
        automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            PROCESS_APPEARANCE_SCAN_INTERVAL,
        )
    }
}

pub(super) fn input_hook_config(settings: &Settings) -> InputHookConfig {
    let mut config = InputHookConfig::default();
    for profile in std::iter::once(settings)
        .chain(settings.on_battery.as_deref())
        .filter(|profile| profile.general.enabled)
    {
        if profile.app_suspension.enabled {
            return InputHookConfig {
                keyboard: true,
                mouse: true,
            };
        }
        if activity_power_plan_required(profile) {
            config.keyboard |= profile.by_activity.input_detection.keyboard;
            config.mouse |= profile.by_activity.input_detection.mouse;
        }
    }
    config
}

pub(super) fn adaptive_engine_process_refresh_interval(settings: &Settings) -> Duration {
    Duration::from_millis(settings.adaptive_engine_process.reaction_time_ms.clamp(
        crate::config::ADAPTIVE_ENGINE_PROCESS_REACTION_INTERVAL_MIN_MS,
        crate::config::ADAPTIVE_ENGINE_PROCESS_REACTION_INTERVAL_MAX_MS,
    ))
}

pub(super) fn feature_refresh_required(settings: &Settings, feature_enabled: bool) -> bool {
    settings.general.enabled && feature_enabled
}

fn enabled_executable_path_rule(enabled: bool, executable_path: &str) -> bool {
    enabled && std::path::Path::new(executable_path.trim()).is_absolute()
}

pub(super) fn adaptive_engine_process_required(settings: &Settings) -> bool {
    settings.adaptive_engine.enabled && settings.adaptive_engine_process.has_enabled_policies()
}

pub(super) fn app_suspension_required(settings: &Settings) -> bool {
    settings.app_suspension.enabled && app_suspension::has_enabled_rules(&settings.app_suspension)
}

pub(super) fn app_suspension_policy_varies_by_source(settings: &Settings) -> bool {
    settings.general.enabled
        && settings.on_battery.as_deref().is_some_and(|battery| {
            !app_suspension::same_app_suspension_policy(
                &settings.app_suspension,
                &battery.app_suspension,
            )
        })
}

pub(super) fn cpu_sets_soft_required(settings: &Settings) -> bool {
    settings.cpu_sets_soft.enabled
        && settings.cpu_sets_soft.rules.iter().any(|rule| {
            enabled_executable_path_rule(rule.enabled, &rule.executable_path)
                && rule.has_cpu_selection()
        })
}

pub(super) fn processor_affinity_hard_required(settings: &Settings) -> bool {
    settings.processor_affinity_hard.enabled
        && settings.processor_affinity_hard.rules.iter().any(|rule| {
            enabled_executable_path_rule(rule.enabled, &rule.executable_path)
                && rule.has_cpu_selection()
                && !settings
                    .cpu_sets_soft
                    .contains_rule_for(rule.executable_path.trim_start())
        })
}

pub(super) fn cpu_limiter_required(settings: &Settings) -> bool {
    settings.cpu_limiter.enabled
        && crate::features::cpu_control::cpu_limiter::has_finite_limits(&settings.cpu_limiter)
}

pub(super) fn cpu_limiter_policy_varies_by_source(settings: &Settings) -> bool {
    if !settings.general.enabled {
        return false;
    }
    let Some(battery) = settings.on_battery.as_deref() else {
        return false;
    };
    if settings.cpu_limiter == battery.cpu_limiter {
        return false;
    }
    let ac_active = cpu_limiter_required(settings);
    let battery_active = cpu_limiter_required(battery);
    ac_active != battery_active
        || (ac_active
            && !crate::features::cpu_control::cpu_limiter::same_limit_policy(
                &settings.cpu_limiter,
                &battery.cpu_limiter,
            ))
}

pub(super) fn timer_resolution_required(settings: &Settings) -> bool {
    settings.timer_resolution.enabled
        && settings
            .timer_resolution
            .rules
            .iter()
            .any(|rule| enabled_executable_path_rule(rule.enabled, &rule.executable_path))
}

pub(super) fn sleep_prevention_required(settings: &Settings) -> bool {
    settings.sleep_prevention.enabled
        && settings
            .sleep_prevention
            .rules
            .iter()
            .any(|rule| enabled_executable_path_rule(rule.enabled, &rule.executable_path))
}

pub(super) fn process_appearance_scan_required(settings: &Settings) -> bool {
    process_appearance_scan_required_for_runtime(
        settings,
        cpu_limiter_required(settings),
        settings.memory_trim.enabled,
    )
}

pub(super) fn process_appearance_scan_required_for_runtime(
    settings: &Settings,
    cpu_limiter_active: bool,
    memory_trim_active: bool,
) -> bool {
    settings.general.enabled
        && (settings.background_efficiency.enabled
            || cpu_sets_soft_required(settings)
            || processor_affinity_hard_required(settings)
            || cpu_limiter_active
            || by_running_app_required(settings)
            || adaptive_engine_process_required(settings)
            || settings.process_priority.enabled
            || settings.thread_priority.enabled
            || settings.dynamic_priority_boost.enabled
            || settings.io_priority.enabled
            || settings.gpu_priority.enabled
            || settings.memory_priority.enabled
            || memory_trim_active)
}

pub(super) fn power_plan_checks_required(settings: &Settings) -> bool {
    settings.general.enabled
        && (activity_power_plan_required(settings)
            || by_focus_required(settings)
            || by_time_rules_required(settings)
            || by_cpu_load_rules_required(settings)
            || by_running_app_required(settings))
}

pub(super) fn ordinary_power_plan_waits_for_source(
    settings: &Settings,
    plugged_in: Option<bool>,
) -> bool {
    if plugged_in.is_some() || !settings.general.enabled {
        return false;
    }

    let battery = settings.battery_profile();
    (settings.general.pause_power_plan_switching_while_plugged_in
        || settings.on_battery.as_deref().is_some_and(|battery| {
            settings.by_activity != battery.by_activity
                || settings.by_focus != battery.by_focus
                || settings.by_running_app != battery.by_running_app
                || settings.by_cpu_load != battery.by_cpu_load
                || settings.by_time != battery.by_time
        }))
        && (power_plan_checks_required(settings) || power_plan_checks_required(battery))
}

pub(super) fn adaptive_power_plan_waits_for_source(
    settings: &Settings,
    plugged_in: Option<bool>,
) -> bool {
    if plugged_in.is_some() || !settings.general.enabled {
        return false;
    }
    let Some(battery) = settings.on_battery.as_deref() else {
        return false;
    };
    let ac = &settings.adaptive_engine;
    let battery = &battery.adaptive_engine;
    ac.adaptive_processor_power_enabled != battery.adaptive_processor_power_enabled
        || (ac.adaptive_processor_power_enabled
            && (ac.base_processor_policy.normalized()
                != battery.base_processor_policy.normalized()
                || ac.background_pressure_profile.normalized()
                    != battery.background_pressure_profile.normalized()
                || ac.focus_and_launch_profile.normalized()
                    != battery.focus_and_launch_profile.normalized()))
}

pub(super) fn automation_worker_required(settings: &Settings) -> bool {
    keep_running_required(settings)
        || instance_limits_required(settings)
        || instance_balancing_required(settings)
        || automation_worker_required_for_profile(settings)
        || settings
            .on_battery
            .as_deref()
            .is_some_and(automation_worker_required_for_profile)
}

fn automation_worker_required_for_profile(settings: &Settings) -> bool {
    settings.general.enabled
        && (watchdog_required(settings)
            || power_plan_checks_required(settings)
            || adaptive_power_plan_required(settings)
            || bottleneck_classifier_required(settings)
            || app_suspension_required(settings)
            || process_appearance_scan_required(settings)
            || timer_resolution_required(settings)
            || sleep_prevention_required(settings))
}

pub(super) fn bottleneck_classifier_required(settings: &Settings) -> bool {
    settings.general.enabled && settings.adaptive_engine.enabled
}

pub(super) fn watchdog_required(settings: &Settings) -> bool {
    settings.watchdog.enabled && settings.watchdog.rules.iter().any(|rule| rule.enabled)
}

pub(super) fn keep_running_required(settings: &Settings) -> bool {
    settings.general.enabled
        && settings.keep_running.enabled
        && settings
            .keep_running
            .rules
            .iter()
            .any(|rule| enabled_executable_path_rule(rule.enabled, &rule.executable_path))
}

pub(super) fn instance_limits_required(settings: &Settings) -> bool {
    settings.general.enabled
        && settings.instance_limits.enabled
        && settings
            .instance_limits
            .rules
            .iter()
            .any(|rule| rule.enabled)
}

pub(super) fn instance_balancing_required(settings: &Settings) -> bool {
    settings.general.enabled
        && settings.instance_balancing.enabled
        && settings
            .instance_balancing
            .rules
            .iter()
            .any(|rule| rule.enabled)
}

pub(super) fn windows_event_watcher_mode(settings: &Settings) -> Option<WindowsEventWatcherMode> {
    if windows_event_watcher_required(settings)
        || adaptive_enablement_varies_by_source(settings)
        || cpu_limiter_policy_varies_by_source(settings)
        || memory_trim_policy_difference(settings) != MemoryTrimPolicyDifference::Common
    {
        Some(WindowsEventWatcherMode::Full)
    } else if std::iter::once(settings)
        .chain(settings.on_battery.as_deref())
        .any(|profile| {
            feature_refresh_required(
                profile,
                adaptive_engine_process_required(profile) || adaptive_power_plan_required(profile),
            )
        })
    {
        Some(WindowsEventWatcherMode::SuspendResumeOnly)
    } else {
        None
    }
}

pub(super) fn adaptive_enablement_varies_by_source(settings: &Settings) -> bool {
    let enabled = |profile: &Settings| {
        (
            bottleneck_classifier_required(profile),
            feature_refresh_required(profile, adaptive_engine_process_required(profile)),
            feature_refresh_required(profile, adaptive_power_plan_required(profile)),
        )
    };
    settings
        .on_battery
        .as_deref()
        .is_some_and(|battery| enabled(settings) != enabled(battery))
}

// Full Windows event coverage is required by the existing event-driven features.
pub(super) fn windows_event_watcher_required(settings: &Settings) -> bool {
    keep_running_required(settings)
        || instance_limits_required(settings)
        || instance_balancing_required(settings)
        || windows_event_watcher_required_for_profile(settings)
        || settings
            .on_battery
            .as_deref()
            .is_some_and(windows_event_watcher_required_for_profile)
}

fn windows_event_watcher_required_for_profile(settings: &Settings) -> bool {
    automation_windows_event_watcher_required(settings)
        || (!settings.adaptive_engine.enabled && appearance_events_required(settings))
}

pub(super) fn automation_windows_event_watcher_required(settings: &Settings) -> bool {
    settings.general.enabled
        && (power_plan_checks_required(settings)
            || event_driven_process_work_required(settings)
            || sleep_prevention_required(settings))
}

pub(super) fn event_driven_process_work_required(settings: &Settings) -> bool {
    app_suspension_required(settings)
        || (!settings.adaptive_engine.enabled && process_appearance_scan_required(settings))
}

pub(super) fn windows_event_wake_required(
    settings: &Settings,
    event: WindowsAutomationEvent,
) -> bool {
    if (keep_running_required(settings)
        || instance_limits_required(settings)
        || instance_balancing_required(settings))
        && matches!(
            event,
            WindowsAutomationEvent::SuspendResume | WindowsAutomationEvent::SessionChanged
        )
    {
        return true;
    }
    std::iter::once(settings)
        .chain(settings.on_battery.as_deref())
        .any(|profile| windows_event_wake_required_for_profile(profile, event))
}

fn windows_event_wake_required_for_profile(
    settings: &Settings,
    event: WindowsAutomationEvent,
) -> bool {
    if event == WindowsAutomationEvent::AppearanceChanged {
        return !settings.adaptive_engine.enabled && appearance_events_required(settings);
    }

    if settings.general.enabled {
        match event {
            WindowsAutomationEvent::ForegroundChanged => {
                by_focus_required(settings)
                    || event_driven_process_work_required(settings)
                    // Preserve foreground work already scheduled when another plan rule keeps the watcher available.
                    || (power_plan_checks_required(settings)
                        && (process_appearance_scan_required(settings)
                            || timer_resolution_required(settings)
                            || adaptive_power_plan_required(settings)))
            }
            WindowsAutomationEvent::WindowCreated => {
                event_driven_process_work_required(settings) || sleep_prevention_required(settings)
            }
            WindowsAutomationEvent::PowerChanged | WindowsAutomationEvent::SuspendResume => {
                automation_worker_required_for_profile(settings)
            }
            WindowsAutomationEvent::SessionChanged => windows_event_watcher_required(settings),
            WindowsAutomationEvent::ClockChanged => by_time_rules_required(settings),
            WindowsAutomationEvent::AppearanceChanged => false,
        }
    } else {
        false
    }
}

pub(super) fn appearance_events_required(settings: &Settings) -> bool {
    settings.general.theme_mode == AppThemeMode::System
        || settings.general.accent.source == AccentColorSource::Windows
}

pub(super) fn activity_power_plan_required(settings: &Settings) -> bool {
    settings.by_activity.enabled
        && (has_idle_plan(&settings.by_activity.power_plans)
            || (settings.by_activity.switch_to_performance_on_resume
                && settings.by_activity.input_detection.any_enabled()
                && has_active_plan(&settings.by_activity.power_plans)))
}

pub(super) fn controller_activity_poll_required(settings: &Settings) -> bool {
    settings.general.enabled
        && settings.by_activity.enabled
        && settings.by_activity.input_detection.controller
        && (has_idle_plan(&settings.by_activity.power_plans)
            || (settings.by_activity.switch_to_performance_on_resume
                && has_active_plan(&settings.by_activity.power_plans)))
}

pub(super) fn by_focus_required(settings: &Settings) -> bool {
    settings.by_focus.enabled
        && settings
            .by_focus
            .rules
            .iter()
            .any(|rule| rule.configured_plan().is_some())
}

pub(super) fn foreground_lookup_required(settings: &Settings) -> bool {
    by_focus_required(settings)
}

pub(super) fn by_time_rules_required(settings: &Settings) -> bool {
    settings.by_time.enabled
        && settings
            .by_time
            .rules
            .iter()
            .any(|rule| rule.configured_plan().is_some())
}

pub(super) fn cpu_usage_required(settings: &Settings) -> bool {
    bottleneck_classifier_required(settings)
        || feature_refresh_required(settings, by_cpu_load_rules_required(settings))
}

pub(super) fn by_cpu_load_rules_required(settings: &Settings) -> bool {
    settings.by_cpu_load.enabled
        && settings.by_cpu_load.rules.iter().any(|rule| {
            rule.enabled
                && (rule.power_plan_guid.is_some()
                    || (rule.else_enabled && rule.else_power_plan_guid.is_some()))
        })
}

pub(super) fn by_running_app_required(settings: &Settings) -> bool {
    settings.by_running_app.enabled
        && settings.by_running_app.rules.iter().any(|rule| {
            rule.configured_plan().is_some()
                && rule
                    .selector
                    .exact_path()
                    .is_none_or(|path| std::path::Path::new(path.trim()).is_absolute())
        })
}

pub(super) fn has_idle_plan(power_plans: &PowerPlanSettings) -> bool {
    power_plans.power_save_guid.is_some()
}

pub(super) fn has_active_plan(power_plans: &PowerPlanSettings) -> bool {
    power_plans.performance_guid.is_some()
}

pub(super) fn configured_check_interval(settings: &Settings) -> Duration {
    Duration::from_millis(
        settings
            .general
            .check_interval_ms
            .clamp(CHECK_INTERVAL_MIN_MS, CHECK_INTERVAL_MAX_MS),
    )
}

pub(super) fn power_plan_check_delay(
    settings: &Settings,
    windows_event_watcher_active: bool,
    activity_idle_for: Option<Duration>,
    wall_time: DateTime<Local>,
) -> Option<Duration> {
    if !windows_event_watcher_active {
        return Some(configured_check_interval(settings));
    }

    let mut delay = None;
    if by_cpu_load_rules_required(settings) {
        delay = Some(min_worker_wait(delay, CPU_USAGE_REFRESH_INTERVAL));
    }
    if by_time_rules_required(settings) {
        let schedule_delay = bounded_by_time_check_delay(
            next_by_time_change_delay(&settings.by_time, wall_time),
            configured_check_interval(settings),
        );
        delay = Some(min_worker_wait(delay, schedule_delay));
    }
    if by_running_app_required(settings) {
        delay = Some(min_worker_wait(delay, PERFORMANCE_MODE_REFRESH_INTERVAL));
    }
    if let Some(activity_delay) = activity_idle_check_delay(settings, activity_idle_for) {
        delay = Some(min_worker_wait(delay, activity_delay));
    }
    delay
}

pub(super) fn bounded_by_time_check_delay(
    next_change_delay: Option<Duration>,
    fallback: Duration,
) -> Duration {
    next_change_delay
        .unwrap_or(fallback)
        .min(SCHEDULE_RULE_MAX_SLEEP)
}

pub(super) fn activity_idle_check_delay(
    settings: &Settings,
    idle_for: Option<Duration>,
) -> Option<Duration> {
    if !settings.general.enabled
        || !settings.by_activity.enabled
        || !has_idle_plan(&settings.by_activity.power_plans)
    {
        return None;
    }

    let timeout = Duration::from_secs(settings.by_activity.idle_timeout_seconds);
    match idle_for {
        Some(idle_for) if idle_for < timeout => Some(timeout - idle_for),
        Some(_) => None,
        None => Some(configured_check_interval(settings)),
    }
}
