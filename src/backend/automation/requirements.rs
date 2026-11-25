use super::*;
use crate::features::power_plan_control::by_running_app::same_exclusion_selectors;

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

pub(super) struct PrioritySourceSchedule {
    pub(super) active: bool,
    pub(super) refresh_required: bool,
    pub(super) waiting_for_source: bool,
    pub(super) source_recheck: bool,
}

pub(super) fn priority_source_schedule<T: Eq>(
    configured: &Settings,
    settings: &Settings,
    plugged_in: Option<bool>,
    windows_event_watcher_active: bool,
    settings_changed: bool,
    policy: impl Fn(&Settings) -> (&T, bool),
) -> PrioritySourceSchedule {
    let active_policy = |settings| {
        let (policy, enabled) = policy(settings);
        feature_refresh_required(settings, enabled).then_some(policy)
    };
    let current = active_policy(settings);
    let source_recheck = (plugged_in.is_none()
        || (!windows_event_watcher_active && current.is_none()))
        && configured.on_battery.as_deref().is_some_and(|battery| {
            let other = if plugged_in == Some(false) {
                configured
            } else {
                battery
            };
            current != active_policy(other)
        });
    let waiting_for_source = plugged_in.is_none() && source_recheck;
    let active = current.is_some() && !waiting_for_source;
    PrioritySourceSchedule {
        active,
        refresh_required: settings_changed || active,
        waiting_for_source,
        source_recheck,
    }
}

fn enabled_executable_path_rule(enabled: bool, executable_path: &str) -> bool {
    enabled && std::path::Path::new(executable_path.trim()).is_absolute()
}

pub(super) fn adaptive_engine_process_required(settings: &Settings) -> bool {
    settings.adaptive_engine.enabled && settings.adaptive_engine_process.has_enabled_policies()
}

pub(super) fn adaptive_engine_process_waits_for_source(
    settings: &Settings,
    plugged_in: Option<bool>,
) -> bool {
    if plugged_in.is_some() {
        return false;
    }
    let Some(battery) = settings.on_battery.as_deref() else {
        return false;
    };
    let active =
        |profile| feature_refresh_required(profile, adaptive_engine_process_required(profile));
    let (ac_active, battery_active) = (active(settings), active(battery));
    if !ac_active || !battery_active {
        return ac_active != battery_active;
    }

    let ac = &settings.adaptive_engine_process;
    let dc = &battery.adaptive_engine_process;
    let shared_policy = |policy: &crate::config::AdaptiveEngineProcessSettings| {
        (
            policy.cpu_pressure_restraint_enabled.then_some((
                policy.restraint_trigger_time_ms,
                policy.restraint_priority,
                policy.restraint_io_priority,
                policy.restraint_energy_efficient_scheduling,
                policy.maximum_restraint_time_seconds,
                policy
                    .pause_restraint_when_idle
                    .then_some(policy.restraint_idle_timeout_seconds),
            )),
            policy.limit_background_processors_enabled,
            policy.dynamic_resource_zones_enabled,
            policy.pressure_source,
            policy.exclude_service_hosts,
            policy.foreground_or_system_cpu_threshold_percent,
            policy.background_app_cpu_threshold_percent,
            policy.cpu_recovery_threshold_percent,
            policy.reaction_time_ms,
            policy.cpu_restraint_time_seconds,
            policy.cpu_recovery_time_seconds,
            policy.maximum_restrained_apps,
        )
    };
    !same_exclusion_selectors(&settings.by_running_app, &battery.by_running_app)
        || shared_policy(ac) != shared_policy(dc)
        || (ac.limit_background_processors_enabled
            && (ac.cpu_allocation_method != dc.cpu_allocation_method
                || (ac.cpu_allocation_method.is_hard_affinity()
                    && ac.preserve_custom_affinity != dc.preserve_custom_affinity)
                || if ac.cpu_allocation_method.uses_processor_selection() {
                    ac.background_processor_selection != dc.background_processor_selection
                        || (ac.background_processor_selection.uses_percentage()
                            && ac.processor_limit_percent != dc.processor_limit_percent)
                        || (ac.background_processor_selection
                            == crate::config::BackgroundProcessorSelection::Custom
                            && ac.specific_processors != dc.specific_processors)
                } else {
                    ac.processors_to_remove != dc.processors_to_remove
                }))
        || (ac.dynamic_resource_zones_enabled
            && ac.dynamic_resource_zone_settings != dc.dynamic_resource_zone_settings)
        || ac
            .custom_rules
            .iter()
            .filter(|rule| rule.enabled)
            .map(|rule| &rule.executable_path)
            .ne(dc
                .custom_rules
                .iter()
                .filter(|rule| rule.enabled)
                .map(|rule| &rule.executable_path))
}

pub(super) fn efficiency_mode_policy_varies_by_source(settings: &Settings) -> bool {
    let Some(battery) = settings.on_battery.as_deref() else {
        return false;
    };
    if settings.general.enabled == battery.general.enabled
        && settings.efficiency_mode == battery.efficiency_mode
    {
        return false;
    }
    let has_policy = |profile: &Settings| {
        let efficiency = &profile.efficiency_mode;
        feature_refresh_required(profile, efficiency.enabled)
            && (efficiency.background_efficiency_mode
                || (efficiency.foreground_detection_enabled
                    && efficiency.foreground_efficiency_mode)
                || (efficiency.visible_window_detection_enabled
                    && efficiency.visible_window_efficiency_mode)
                || efficiency.custom_rules.iter().any(|rule| {
                    rule.enabled && efficiency.custom_rule_applies_efficiency_mode(rule)
                }))
    };
    has_policy(settings) || has_policy(battery)
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

#[derive(Clone, Copy)]
pub(super) struct CpuAllocationSourceDifference {
    pub(super) soft: bool,
    pub(super) hard: bool,
}

pub(super) fn cpu_allocation_source_difference(
    settings: &Settings,
) -> Option<CpuAllocationSourceDifference> {
    let battery = settings.on_battery.as_deref()?;
    if !settings.general.enabled
        || (settings.cpu_sets_soft == battery.cpu_sets_soft
            && settings.processor_affinity_hard == battery.processor_affinity_hard)
    {
        return None;
    }
    // Hard yields to configured Soft paths even if those Soft rules are disabled.
    let has_rule = |feature: &CpuAllocationSettings| {
        feature.enabled
            && feature.rules.iter().any(|rule| {
                enabled_executable_path_rule(rule.enabled, &rule.executable_path)
                    && rule.has_cpu_selection()
            })
    };
    let soft = has_rule(&settings.cpu_sets_soft) || has_rule(&battery.cpu_sets_soft);
    let hard =
        has_rule(&settings.processor_affinity_hard) || has_rule(&battery.processor_affinity_hard);
    (soft || hard).then_some(CpuAllocationSourceDifference { soft, hard })
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

pub(super) fn timer_resolution_enablement_varies_by_source(settings: &Settings) -> bool {
    let enabled =
        |profile: &Settings| feature_refresh_required(profile, timer_resolution_required(profile));
    settings
        .on_battery
        .as_deref()
        .is_some_and(|battery| enabled(settings) != enabled(battery))
}

pub(super) fn timer_resolution_policy_varies_by_source(settings: &Settings) -> bool {
    fn intents(profile: &Settings) -> impl Iterator<Item = (&str, u32)> {
        let enabled = feature_refresh_required(profile, profile.timer_resolution.enabled);
        profile
            .timer_resolution
            .rules
            .iter()
            .filter(move |rule| {
                enabled && enabled_executable_path_rule(rule.enabled, &rule.executable_path)
            })
            .map(|rule| (rule.executable_path.trim_start(), rule.desired_100ns))
    }

    settings
        .on_battery
        .as_deref()
        .is_some_and(|battery| intents(settings).ne(intents(battery)))
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
    let priority_or_efficiency_configured = settings.efficiency_mode.enabled
        || settings.process_priority.enabled
        || settings.thread_priority.enabled
        || settings.dynamic_priority_boost.enabled
        || settings.io_priority.enabled
        || settings.gpu_priority.enabled
        || settings.memory_priority.enabled;
    process_appearance_scan_required_for_runtime(
        settings,
        cpu_limiter_required(settings),
        settings.memory_trim.enabled,
        settings.general.enabled
            && (cpu_sets_soft_required(settings) || processor_affinity_hard_required(settings)),
        priority_or_efficiency_configured,
        adaptive_engine_process_required(settings),
    )
}

pub(super) fn process_appearance_scan_required_for_runtime(
    settings: &Settings,
    cpu_limiter_active: bool,
    memory_trim_active: bool,
    explicit_cpu_allocation_active: bool,
    priority_or_efficiency_active: bool,
    adaptive_engine_process_active: bool,
) -> bool {
    settings.general.enabled
        && (priority_or_efficiency_active
            || explicit_cpu_allocation_active
            || cpu_limiter_active
            || by_running_app_required(settings)
            || adaptive_engine_process_active
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
    plugged_in.is_none() && ordinary_power_plan_policy_varies_by_source(settings)
}

pub(super) fn ordinary_power_plan_policy_varies_by_source(settings: &Settings) -> bool {
    if !settings.general.enabled {
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
        && (resource_thresholds_required(settings)
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

pub(super) fn resource_thresholds_required(settings: &Settings) -> bool {
    settings.resource_thresholds.enabled
        && settings
            .resource_thresholds
            .rules
            .iter()
            .any(|rule| rule.enabled)
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
        || [settings, settings.battery_profile()]
            .into_iter()
            .any(keep_awake_enabled)
        || adaptive_enablement_varies_by_source(settings)
        || cpu_limiter_policy_varies_by_source(settings)
        || timer_resolution_enablement_varies_by_source(settings)
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

pub(super) fn cpu_usage_required(configured: &Settings, plugged_in: Option<bool>) -> bool {
    let settings = active_power_source_settings(configured, plugged_in);
    bottleneck_classifier_required(settings)
        || (feature_refresh_required(settings, adaptive_power_plan_required(settings))
            && !adaptive_power_plan_waits_for_source(configured, plugged_in))
        || (by_cpu_load_sample_required(settings, plugged_in)
            && !ordinary_power_plan_waits_for_source(configured, plugged_in))
}

pub(super) fn by_cpu_load_sample_required(settings: &Settings, plugged_in: Option<bool>) -> bool {
    crate::rules::decision_engine::ordinary_power_plan_stop(settings, plugged_in).is_none()
        && by_cpu_load_rules_required(settings)
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
    paused_cpu_load_readback_delay: Option<Duration>,
) -> Option<Duration> {
    if !windows_event_watcher_active {
        return Some(configured_check_interval(settings));
    }

    let mut delay = None;
    if by_cpu_load_rules_required(settings) {
        let cpu_delay = paused_cpu_load_readback_delay.unwrap_or(CPU_USAGE_REFRESH_INTERVAL);
        delay = Some(min_worker_wait(delay, cpu_delay));
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
