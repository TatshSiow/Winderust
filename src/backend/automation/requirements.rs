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

pub(super) fn workload_refresh_interval(
    settings: &Settings,
    hidden_to_tray: bool,
    adaptive_engine_enabled: bool,
) -> Duration {
    if adaptive_power_plan_required(settings) {
        WORKLOAD_ENGINE_FAST_REFRESH_INTERVAL
    } else {
        automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            WORKLOAD_ENGINE_REFRESH_INTERVAL,
        )
    }
}

pub(super) fn workload_engine_fast_refresh_deadline(
    settings: &Settings,
    now: Instant,
) -> Option<Instant> {
    feature_refresh_required(settings, workload_engine_required(settings))
        .then_some(now + WORKLOAD_ENGINE_FAST_REFRESH_WINDOW)
}

pub(super) fn workload_engine_fast_refresh_active(
    settings: &Settings,
    fast_until: Option<Instant>,
    now: Instant,
) -> bool {
    feature_refresh_required(settings, workload_engine_required(settings))
        && fast_until.is_some_and(|until| now < until)
}

pub(super) fn feature_refresh_required(settings: &Settings, feature_enabled: bool) -> bool {
    settings.general.enabled && feature_enabled
}

pub(super) fn workload_engine_required(settings: &Settings) -> bool {
    let workload = &settings.workload_engine;
    workload.enabled
        && (workload.lower_background_apps
            || workload.workload_engine_background_efficiency_enabled
            || workload.workload_engine_enabled
            || workload.boost_foreground_app)
}

pub(super) fn io_priority_required(settings: &Settings) -> bool {
    settings.io_priority.enabled
        || (settings.workload_engine.enabled
            && (settings
                .workload_engine
                .lower_background_io_priority_enabled
                || settings.workload_engine.workload_engine_io_priority.enabled))
}

pub(super) fn workload_engine_priority_assist_required(settings: &Settings) -> bool {
    settings.workload_engine.enabled && settings.workload_engine.workload_engine_enabled
}

pub(super) fn thread_priority_required(settings: &Settings) -> bool {
    settings.thread_priority.enabled
        || (workload_engine_priority_assist_required(settings)
            && settings
                .workload_engine
                .workload_engine_thread_priority
                .enabled)
}

pub(super) fn dynamic_priority_boost_required(settings: &Settings) -> bool {
    settings.dynamic_priority_boost.enabled
        || (workload_engine_priority_assist_required(settings)
            && settings
                .workload_engine
                .workload_engine_dynamic_priority_boost
                .enabled)
}

pub(super) fn gpu_priority_required(settings: &Settings) -> bool {
    settings.gpu_priority.enabled
        || (workload_engine_priority_assist_required(settings)
            && settings
                .workload_engine
                .workload_engine_gpu_priority
                .enabled)
}

pub(super) fn effective_io_priority_settings(
    settings: &Settings,
    workload_engine_active: bool,
) -> crate::config::IoPrioritySettings {
    let mut io_priority = settings.io_priority.clone();
    if workload_engine_active {
        let auto_io_priority = workload_engine_io_priority_settings(settings);
        if auto_io_priority.enabled {
            io_priority = auto_io_priority;
            io_priority
                .exclusions
                .extend(settings.workload_engine.workload_engine_exclusions.clone());
        }
    }
    io_priority
}

pub(super) fn workload_engine_io_priority_settings(
    settings: &Settings,
) -> crate::config::IoPrioritySettings {
    let mut io_priority = settings.workload_engine.workload_engine_io_priority.clone();
    if !io_priority.enabled
        && settings
            .workload_engine
            .lower_background_io_priority_enabled
    {
        io_priority.enabled = true;
        io_priority.foreground_priority = ProcessIoPriority::Normal.into();
        io_priority.background_priority =
            settings.workload_engine.lower_background_io_priority.into();
    }
    io_priority.foreground_detection_enabled = true;
    io_priority.preserve_foreground_priority = true;
    io_priority.preserve_background_priority = true;
    io_priority
}

pub(super) fn effective_thread_priority_settings(
    settings: &Settings,
    workload_engine_active: bool,
) -> crate::config::ThreadPrioritySettings {
    let mut thread_priority = settings.thread_priority.clone();
    if workload_engine_active
        && workload_engine_priority_assist_required(settings)
        && settings
            .workload_engine
            .workload_engine_thread_priority
            .enabled
    {
        thread_priority = settings
            .workload_engine
            .workload_engine_thread_priority
            .clone();
        thread_priority.foreground_detection_enabled = true;
        thread_priority.preserve_foreground_priority = true;
        thread_priority.preserve_background_priority = true;
        thread_priority
            .exclusions
            .extend(settings.workload_engine.workload_engine_exclusions.clone());
    }
    thread_priority
}

pub(super) fn effective_dynamic_priority_boost_settings(
    settings: &Settings,
    workload_engine_active: bool,
) -> crate::config::DynamicPriorityBoostSettings {
    let mut dynamic_priority_boost = settings.dynamic_priority_boost.clone();
    if workload_engine_active
        && workload_engine_priority_assist_required(settings)
        && settings
            .workload_engine
            .workload_engine_dynamic_priority_boost
            .enabled
    {
        dynamic_priority_boost = settings
            .workload_engine
            .workload_engine_dynamic_priority_boost
            .clone();
        dynamic_priority_boost.foreground_detection_enabled = true;
        dynamic_priority_boost
            .exclusions
            .extend(settings.workload_engine.workload_engine_exclusions.clone());
    }
    dynamic_priority_boost
}

pub(super) fn effective_gpu_priority_settings(
    settings: &Settings,
    workload_engine_active: bool,
) -> crate::config::GpuPrioritySettings {
    let mut gpu_priority = settings.gpu_priority.clone();
    if workload_engine_active
        && workload_engine_priority_assist_required(settings)
        && settings
            .workload_engine
            .workload_engine_gpu_priority
            .enabled
    {
        gpu_priority = settings
            .workload_engine
            .workload_engine_gpu_priority
            .clone();
        gpu_priority.foreground_detection_enabled = true;
        gpu_priority.preserve_foreground_priority = true;
        gpu_priority.preserve_background_priority = true;
        gpu_priority
            .exclusions
            .extend(settings.workload_engine.workload_engine_exclusions.clone());
    }
    gpu_priority
}

pub(super) fn process_appearance_scan_required(settings: &Settings) -> bool {
    settings.general.enabled
        && (settings.background_efficiency.enabled
            || settings.core_steering.enabled
            || settings.background_cpu_restriction.enabled
            || settings.core_limiter.enabled
            || settings.by_running_app.enabled
            || settings.workload_engine.enabled
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
    settings.general.enabled
        && (power_plan_checks_required(settings)
            || adaptive_power_plan_required(settings)
            || settings.background_efficiency.enabled
            || settings.app_suspension.enabled
            || settings.core_steering.enabled
            || settings.background_cpu_restriction.enabled
            || settings.core_limiter.enabled
            || settings.by_running_app.enabled
            || settings.workload_engine.enabled
            || settings.process_priority.enabled
            || thread_priority_required(settings)
            || dynamic_priority_boost_required(settings)
            || io_priority_required(settings)
            || gpu_priority_required(settings)
            || settings.memory_priority.enabled
            || settings.memory_trim.enabled
            || settings.timer_resolution.enabled)
}

pub(super) fn windows_event_watcher_required(settings: &Settings) -> bool {
    automation_windows_event_watcher_required(settings)
        || (!settings.adaptive_engine.enabled && appearance_events_required(settings))
}

pub(super) fn automation_windows_event_watcher_required(settings: &Settings) -> bool {
    settings.general.enabled
        && (power_plan_checks_required(settings) || event_driven_process_work_required(settings))
}

pub(super) fn event_driven_process_work_required(settings: &Settings) -> bool {
    !settings.adaptive_engine.enabled
        && (settings.app_suspension.enabled || process_appearance_scan_required(settings))
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
        && (settings
            .by_foreground
            .rules
            .iter()
            .any(|rule| rule.enabled && rule.power_plan_guid.is_some()))
}

pub(crate) fn foreground_lookup_required(settings: &Settings) -> bool {
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
        && settings
            .by_running_app
            .rules
            .iter()
            .any(|rule| rule.enabled && rule.power_plan_guid.is_some())
}

pub(super) fn has_idle_plan(power_plans: &PowerPlanSettings) -> bool {
    power_plans.power_save_guid.is_some()
}

pub(super) fn has_active_plan(power_plans: &PowerPlanSettings) -> bool {
    power_plans.performance_guid.is_some()
}

pub(super) fn configured_check_interval(settings: &Settings) -> Duration {
    Duration::from_millis(settings.general.check_interval_ms.max(250))
}

pub(super) fn hidden_power_plan_check_delay(
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
