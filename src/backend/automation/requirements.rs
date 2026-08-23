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

pub(super) fn input_hook_required(settings: &Settings) -> bool {
    input_hook_required_for_profile(settings)
        || settings
            .on_battery
            .as_deref()
            .is_some_and(input_hook_required_for_profile)
}

fn input_hook_required_for_profile(settings: &Settings) -> bool {
    settings.general.enabled
        && (activity_input_hook_required(settings) || app_suspension_input_hook_required(settings))
}

pub(super) fn input_hook_config(settings: &Settings) -> InputHookConfig {
    let plugged_in_app_suspension = app_suspension_input_hook_required(settings);
    let battery = settings.on_battery.as_deref();
    let battery_app_suspension = battery.is_some_and(app_suspension_input_hook_required);
    InputHookConfig {
        keyboard: settings.by_activity.input_detection.keyboard
            || plugged_in_app_suspension
            || battery.is_some_and(|settings| settings.by_activity.input_detection.keyboard)
            || battery_app_suspension,
        mouse: settings.by_activity.input_detection.mouse
            || plugged_in_app_suspension
            || battery.is_some_and(|settings| settings.by_activity.input_detection.mouse)
            || battery_app_suspension,
    }
}

fn app_suspension_input_hook_required(settings: &Settings) -> bool {
    settings.app_suspension.enabled
}

fn activity_input_hook_required(settings: &Settings) -> bool {
    settings.by_activity.enabled
        && settings.by_activity.switch_to_performance_on_resume
        && settings
            .by_activity
            .input_detection
            .keyboard_or_mouse_enabled()
        && settings.by_activity.power_plans.performance_guid.is_some()
}

pub(super) fn cpu_scheduler_refresh_interval(settings: &Settings) -> Duration {
    Duration::from_millis(settings.cpu_scheduler.reaction_time_ms.clamp(
        crate::config::CPU_SCHEDULER_REACTION_INTERVAL_MIN_MS,
        crate::config::CPU_SCHEDULER_REACTION_INTERVAL_MAX_MS,
    ))
}

pub(super) fn feature_refresh_required(settings: &Settings, feature_enabled: bool) -> bool {
    settings.general.enabled && feature_enabled
}

fn enabled_executable_path_rule(enabled: bool, executable_path: &str) -> bool {
    enabled && std::path::Path::new(executable_path.trim()).is_absolute()
}

pub(super) fn cpu_scheduler_required(settings: &Settings) -> bool {
    settings.adaptive_engine.enabled
        && (settings.cpu_scheduler.cpu_pressure_restraint_enabled
            || settings.cpu_scheduler.limit_background_processors_enabled)
}

pub(super) fn app_suspension_required(settings: &Settings) -> bool {
    settings.app_suspension.enabled
        && settings
            .app_suspension
            .suspendable_apps
            .iter()
            .any(|rule| enabled_executable_path_rule(rule.enabled, &rule.executable_path))
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
                    .contains_rule_for(&rule.executable_path)
        })
}

pub(super) fn core_limiter_required(settings: &Settings) -> bool {
    settings.core_limiter.enabled
        && settings
            .core_limiter
            .rules
            .iter()
            .any(|rule| enabled_executable_path_rule(rule.enabled, &rule.executable_path))
}

pub(super) fn timer_resolution_required(settings: &Settings) -> bool {
    settings.timer_resolution.enabled
        && settings
            .timer_resolution
            .rules
            .iter()
            .any(|rule| enabled_executable_path_rule(rule.enabled, &rule.executable_path))
}

pub(super) fn io_priority_required(settings: &Settings) -> bool {
    settings.io_priority.enabled
        || (settings.adaptive_engine.enabled
            && settings.cpu_scheduler.cpu_pressure_restraint_enabled
            && settings.cpu_scheduler.io_priority.enabled)
}

pub(super) fn cpu_scheduler_priority_assist_required(settings: &Settings) -> bool {
    settings.adaptive_engine.enabled && settings.cpu_scheduler.cpu_pressure_restraint_enabled
}

pub(super) fn thread_priority_required(settings: &Settings) -> bool {
    settings.thread_priority.enabled
        || (cpu_scheduler_priority_assist_required(settings)
            && settings.cpu_scheduler.thread_priority.enabled)
}

pub(super) fn dynamic_priority_boost_required(settings: &Settings) -> bool {
    settings.dynamic_priority_boost.enabled
        || (cpu_scheduler_priority_assist_required(settings)
            && settings.cpu_scheduler.dynamic_priority_boost.enabled)
}

pub(super) fn gpu_priority_required(settings: &Settings) -> bool {
    settings.gpu_priority.enabled
        || (cpu_scheduler_priority_assist_required(settings)
            && settings.cpu_scheduler.gpu_priority.enabled)
}

pub(super) fn effective_io_priority_settings(
    settings: &Settings,
    cpu_pressure_restraint_active: bool,
) -> crate::config::IoPrioritySettings {
    let mut io_priority = settings.io_priority.clone();
    if adaptive_io_priority_active(settings, cpu_pressure_restraint_active) {
        let auto_io_priority = io_priority_settings(settings);
        io_priority = auto_io_priority;
        io_priority
            .exclusions
            .extend(settings.cpu_scheduler.custom_rules.clone());
    }
    io_priority
}

pub(super) fn io_priority_control_owner(
    settings: &Settings,
    cpu_pressure_restraint_active: bool,
) -> ControlOwner {
    if adaptive_io_priority_active(settings, cpu_pressure_restraint_active) {
        ControlOwner::AdaptiveEngine
    } else {
        ControlOwner::IoPriority
    }
}

fn adaptive_io_priority_active(settings: &Settings, cpu_pressure_restraint_active: bool) -> bool {
    cpu_pressure_restraint_active && settings.cpu_scheduler.io_priority.enabled
}

pub(super) fn io_priority_settings(settings: &Settings) -> crate::config::IoPrioritySettings {
    let mut io_priority = settings.cpu_scheduler.io_priority.clone();
    io_priority.foreground_detection_enabled = true;
    io_priority.visible_window_detection_enabled = true;
    io_priority.preserve_foreground_priority = true;
    io_priority.preserve_visible_window_priority = true;
    io_priority.preserve_background_priority = true;
    io_priority
}

pub(super) fn effective_thread_priority_settings(
    settings: &Settings,
    cpu_pressure_restraint_active: bool,
) -> crate::config::ThreadPrioritySettings {
    let mut thread_priority = settings.thread_priority.clone();
    if adaptive_thread_priority_active(settings, cpu_pressure_restraint_active) {
        thread_priority = settings.cpu_scheduler.thread_priority.clone();
        thread_priority.foreground_detection_enabled = true;
        thread_priority.visible_window_detection_enabled = true;
        thread_priority.preserve_foreground_priority = true;
        thread_priority.preserve_visible_window_priority = true;
        thread_priority.preserve_background_priority = true;
        thread_priority
            .exclusions
            .extend(settings.cpu_scheduler.custom_rules.clone());
    }
    thread_priority
}

pub(super) fn thread_priority_control_owner(
    settings: &Settings,
    cpu_pressure_restraint_active: bool,
) -> ControlOwner {
    if adaptive_thread_priority_active(settings, cpu_pressure_restraint_active) {
        ControlOwner::AdaptiveEngine
    } else {
        ControlOwner::ThreadPriority
    }
}

fn adaptive_thread_priority_active(
    settings: &Settings,
    cpu_pressure_restraint_active: bool,
) -> bool {
    cpu_pressure_restraint_active
        && cpu_scheduler_priority_assist_required(settings)
        && settings.cpu_scheduler.thread_priority.enabled
}

pub(super) fn effective_dynamic_priority_boost_settings(
    settings: &Settings,
    cpu_pressure_restraint_active: bool,
) -> crate::config::DynamicPriorityBoostSettings {
    let mut dynamic_priority_boost = settings.dynamic_priority_boost.clone();
    if adaptive_dynamic_priority_boost_active(settings, cpu_pressure_restraint_active) {
        dynamic_priority_boost = settings.cpu_scheduler.dynamic_priority_boost.clone();
        dynamic_priority_boost.foreground_detection_enabled = true;
        dynamic_priority_boost.visible_window_detection_enabled = true;
        dynamic_priority_boost
            .exclusions
            .extend(settings.cpu_scheduler.custom_rules.clone());
    }
    dynamic_priority_boost
}

pub(super) fn dynamic_priority_boost_control_owner(
    settings: &Settings,
    cpu_pressure_restraint_active: bool,
) -> ControlOwner {
    if adaptive_dynamic_priority_boost_active(settings, cpu_pressure_restraint_active) {
        ControlOwner::AdaptiveEngine
    } else {
        ControlOwner::DynamicPriorityBoost
    }
}

fn adaptive_dynamic_priority_boost_active(
    settings: &Settings,
    cpu_pressure_restraint_active: bool,
) -> bool {
    cpu_pressure_restraint_active
        && cpu_scheduler_priority_assist_required(settings)
        && settings.cpu_scheduler.dynamic_priority_boost.enabled
}

pub(super) fn effective_gpu_priority_settings(
    settings: &Settings,
    cpu_pressure_restraint_active: bool,
) -> crate::config::GpuPrioritySettings {
    let mut gpu_priority = settings.gpu_priority.clone();
    if adaptive_gpu_priority_active(settings, cpu_pressure_restraint_active) {
        gpu_priority = settings.cpu_scheduler.gpu_priority.clone();
        gpu_priority.foreground_detection_enabled = true;
        gpu_priority.visible_window_detection_enabled = true;
        gpu_priority.preserve_foreground_priority = true;
        gpu_priority.preserve_visible_window_priority = true;
        gpu_priority.preserve_background_priority = true;
        gpu_priority
            .exclusions
            .extend(settings.cpu_scheduler.custom_rules.clone());
    }
    gpu_priority
}

pub(super) fn gpu_priority_control_owner(
    settings: &Settings,
    cpu_pressure_restraint_active: bool,
) -> ControlOwner {
    if adaptive_gpu_priority_active(settings, cpu_pressure_restraint_active) {
        ControlOwner::AdaptiveEngine
    } else {
        ControlOwner::GpuPriority
    }
}

fn adaptive_gpu_priority_active(settings: &Settings, cpu_pressure_restraint_active: bool) -> bool {
    cpu_pressure_restraint_active
        && cpu_scheduler_priority_assist_required(settings)
        && settings.cpu_scheduler.gpu_priority.enabled
}

pub(super) fn process_appearance_scan_required(settings: &Settings) -> bool {
    settings.general.enabled
        && (settings.background_efficiency.enabled
            || cpu_sets_soft_required(settings)
            || processor_affinity_hard_required(settings)
            || core_limiter_required(settings)
            || by_running_app_required(settings)
            || cpu_scheduler_required(settings)
            || settings.process_priority.enabled
            || thread_priority_required(settings)
            || dynamic_priority_boost_required(settings)
            || io_priority_required(settings)
            || gpu_priority_required(settings)
            || settings.memory_priority.enabled
            || settings.memory_trim.enabled)
}

pub(super) fn power_plan_checks_required(settings: &Settings) -> bool {
    settings.general.enabled
        && (activity_power_plan_required(settings)
            || by_foreground_required(settings)
            || by_time_rules_required(settings)
            || by_cpu_load_rules_required(settings)
            || by_running_app_required(settings))
}

pub(super) fn automation_worker_required(settings: &Settings) -> bool {
    automation_worker_required_for_profile(settings)
        || settings
            .on_battery
            .as_deref()
            .is_some_and(automation_worker_required_for_profile)
}

fn automation_worker_required_for_profile(settings: &Settings) -> bool {
    settings.general.enabled
        && (power_plan_checks_required(settings)
            || adaptive_power_plan_required(settings)
            || bottleneck_classifier_required(settings)
            || app_suspension_required(settings)
            || process_appearance_scan_required(settings)
            || timer_resolution_required(settings))
}

pub(super) fn bottleneck_classifier_required(settings: &Settings) -> bool {
    settings.general.enabled && settings.adaptive_engine.enabled
}

pub(super) fn windows_event_watcher_required(settings: &Settings) -> bool {
    windows_event_watcher_required_for_profile(settings)
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
        && (power_plan_checks_required(settings) || event_driven_process_work_required(settings))
}

pub(super) fn event_driven_process_work_required(settings: &Settings) -> bool {
    app_suspension_required(settings)
        || (!settings.adaptive_engine.enabled && process_appearance_scan_required(settings))
}

pub(super) fn windows_event_wake_required(
    settings: &Settings,
    event: WindowsAutomationEvent,
) -> bool {
    if event == WindowsAutomationEvent::AppearanceChanged {
        return !settings.adaptive_engine.enabled && appearance_events_required(settings);
    }

    if settings.general.enabled {
        match event {
            WindowsAutomationEvent::ForegroundChanged => {
                power_plan_checks_required(settings) || event_driven_process_work_required(settings)
            }
            WindowsAutomationEvent::WindowCreated => event_driven_process_work_required(settings),
            WindowsAutomationEvent::PowerChanged => power_plan_checks_required(settings),
            WindowsAutomationEvent::SessionChanged => windows_event_watcher_required(settings),
            WindowsAutomationEvent::AppearanceChanged => false,
        }
    } else {
        false
    }
}

pub(super) fn appearance_events_required(settings: &Settings) -> bool {
    settings.general.theme_mode == AppThemeMode::System
        || settings.general.accent.source == AccentColorSource::Windows
        || settings.general.animation_mode == AnimationMode::System
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

pub(super) fn by_foreground_required(settings: &Settings) -> bool {
    settings.by_foreground.enabled
        && (settings.by_foreground.rules.iter().any(|rule| {
            enabled_executable_path_rule(rule.enabled, &rule.executable_path)
                && rule.power_plan_guid.is_some()
        }))
}

pub(super) fn foreground_lookup_required(settings: &Settings) -> bool {
    by_foreground_required(settings)
}

pub(super) fn by_time_rules_required(settings: &Settings) -> bool {
    settings.by_time.enabled
        && settings
            .by_time
            .rules
            .iter()
            .any(|rule| rule.enabled && rule.power_plan_guid.is_some())
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
            enabled_executable_path_rule(rule.enabled, &rule.executable_path)
                && rule.power_plan_guid.is_some()
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
) -> Option<Duration> {
    if !windows_event_watcher_active {
        return Some(configured_check_interval(settings));
    }

    let mut delay = None;
    if by_cpu_load_rules_required(settings) {
        delay = Some(min_worker_wait(delay, CPU_USAGE_REFRESH_INTERVAL));
    }
    if by_time_rules_required(settings) {
        let schedule_delay = next_by_time_change_delay(&settings.by_time)
            .map(|delay| delay.min(SCHEDULE_RULE_MAX_SLEEP))
            .unwrap_or_else(|| configured_check_interval(settings));
        delay = Some(min_worker_wait(delay, schedule_delay));
    }
    if by_running_app_required(settings) {
        delay = Some(min_worker_wait(delay, PERFORMANCE_MODE_REFRESH_INTERVAL));
    }
    if let Some(activity_delay) = activity_idle_check_delay(settings) {
        delay = Some(min_worker_wait(delay, activity_delay));
    }
    delay
}

pub(super) fn activity_idle_check_delay(settings: &Settings) -> Option<Duration> {
    if !settings.general.enabled
        || !settings.by_activity.enabled
        || !has_idle_plan(&settings.by_activity.power_plans)
    {
        return None;
    }

    let timeout = Duration::from_secs(settings.by_activity.idle_timeout_seconds);
    match input_tracker::last_input_elapsed() {
        Some(idle_for) if idle_for < timeout => Some(timeout - idle_for),
        Some(_) => None,
        None => Some(configured_check_interval(settings)),
    }
}
