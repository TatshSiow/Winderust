use super::*;

pub(super) fn process_target_can_accept(
    target: SuggestionTarget,
    settings: &Settings,
    process: &str,
) -> bool {
    match target {
        SuggestionTarget::Foreground => {
            can_add_foreground_process(&settings.by_foreground, process)
        }
        SuggestionTarget::BackgroundEfficiency => {
            can_add_background_efficiency_process(&settings.background_efficiency, process)
        }
        SuggestionTarget::BackgroundCpu => {
            can_add_background_cpu_exclusion(&settings.background_cpu_restriction, process)
        }
        SuggestionTarget::MemoryTrim => {
            can_add_memory_trim_exclusion(&settings.memory_trim, process)
        }
        SuggestionTarget::AppSuspension => {
            can_add_app_suspension_process(&settings.app_suspension, process)
        }
        SuggestionTarget::CoreLimiter => {
            can_add_core_limiter_process(&settings.core_limiter, process)
        }
        SuggestionTarget::ByRunningApp => {
            can_add_by_running_app_process(&settings.by_running_app, process)
        }
        SuggestionTarget::WorkloadEngine => {
            can_add_workload_engine_process(&settings.workload_engine, process)
        }
        SuggestionTarget::ProcessPriority => {
            can_add_process_priority_exclusion(&settings.process_priority, process)
        }
        SuggestionTarget::ThreadPriority => {
            can_add_thread_priority_exclusion(&settings.thread_priority, process)
        }
        SuggestionTarget::DynamicPriorityBoost => {
            can_add_dynamic_priority_boost_exclusion(&settings.dynamic_priority_boost, process)
        }
        SuggestionTarget::IoPriority => {
            can_add_io_priority_exclusion(&settings.io_priority, process)
        }
        SuggestionTarget::GpuPriority => {
            can_add_gpu_priority_exclusion(&settings.gpu_priority, process)
        }
        SuggestionTarget::MemoryPriority => {
            can_add_memory_priority_exclusion(&settings.memory_priority, process)
        }
        SuggestionTarget::TimerResolution => {
            can_add_timer_resolution_process(&settings.timer_resolution, process)
        }
        SuggestionTarget::CoreSteering => {
            can_add_core_steering_process(&settings.core_steering, process)
        }
    }
}

pub(super) fn can_add_process_candidate(
    process: &str,
    contains_process: impl FnOnce(&str) -> bool,
    is_builtin_excluded: impl FnOnce(&str) -> bool,
) -> bool {
    let process = process.trim();
    !process.is_empty() && !contains_process(process) && !is_builtin_excluded(process)
}

pub(super) fn can_add_foreground_process(settings: &ByForegroundSettings, process: &str) -> bool {
    can_add_process_candidate(
        process,
        |process| {
            settings
                .rules
                .iter()
                .any(|rule| same_process_name(&rule.process_name, process))
        },
        |_| false,
    )
}

pub(super) fn foreground_power_plan_override_guid(
    settings: &ByForegroundSettings,
    process_name: &str,
) -> Option<String> {
    settings
        .rules
        .iter()
        .find(|rule| rule.enabled && process_setting_matches(&rule.process_name, process_name))
        .and_then(|rule| rule.power_plan_guid.clone())
}

pub(super) fn set_foreground_power_plan_override(
    settings: &mut ByForegroundSettings,
    process_name: &str,
    power_plan_guid: Option<String>,
) {
    if let Some(power_plan_guid) = power_plan_guid {
        if let Some(rule) = settings
            .rules
            .iter_mut()
            .find(|rule| process_setting_matches(&rule.process_name, process_name))
        {
            rule.enabled = true;
            rule.power_plan_guid = Some(power_plan_guid);
        } else {
            settings
                .rules
                .push(new_foreground_rule(process_name, Some(power_plan_guid)));
        }
    } else {
        settings
            .rules
            .retain(|rule| !process_setting_matches(&rule.process_name, process_name));
    }
}

pub(super) fn new_foreground_rule(
    process: &str,
    power_plan_guid: Option<String>,
) -> ByForegroundRule {
    let process_name = process.trim().to_ascii_lowercase();
    ByForegroundRule {
        enabled: true,
        name: process_name.clone(),
        process_name,
        power_plan_guid,
    }
}

pub(super) fn can_add_background_efficiency_process(
    settings: &BackgroundEfficiencySettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_custom_rule(process),
        background_efficiency::is_builtin_excluded,
    )
}

pub(super) fn can_add_background_cpu_exclusion(
    settings: &BackgroundCpuRestrictionSettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_exclusion(process),
        core_steering::is_builtin_excluded,
    )
}

pub(super) fn can_add_memory_trim_exclusion(settings: &MemoryTrimSettings, process: &str) -> bool {
    can_add_process_candidate(
        process,
        |process| {
            settings.exclusion_enabled_for(process)
                || settings
                    .exclusions
                    .iter()
                    .any(|rule| same_process_name(&rule.process_name, process))
        },
        memory_trim::is_builtin_excluded,
    )
}

pub(super) fn new_process_exclusion_rule(process: &str) -> ProcessExclusionRule {
    ProcessExclusionRule {
        process_name: process.trim().to_ascii_lowercase(),
        ..Default::default()
    }
}

pub(super) fn new_background_efficiency_rule(process: &str) -> BackgroundEfficiencyRule {
    BackgroundEfficiencyRule {
        enabled: true,
        process_name: process.trim().to_ascii_lowercase(),
    }
}

pub(super) fn set_background_efficiency_custom_rule(
    settings: &mut BackgroundEfficiencySettings,
    process_name: &str,
    excluded: bool,
) {
    if excluded {
        if let Some(rule) = settings
            .custom_rules
            .iter_mut()
            .find(|rule| process_setting_matches(&rule.process_name, process_name))
        {
            rule.enabled = true;
        } else {
            settings
                .custom_rules
                .push(new_background_efficiency_rule(process_name));
        }
    } else {
        settings
            .custom_rules
            .retain(|rule| !process_setting_matches(&rule.process_name, process_name));
    }
}

pub(super) fn set_process_exclusion(
    rules: &mut Vec<ProcessExclusionRule>,
    process_name: &str,
    excluded: bool,
) {
    if excluded {
        if let Some(rule) = rules
            .iter_mut()
            .find(|rule| process_setting_matches(&rule.process_name, process_name))
        {
            rule.enabled = true;
        } else {
            rules.push(new_process_exclusion_rule(process_name));
        }
    } else {
        rules.retain(|rule| !process_setting_matches(&rule.process_name, process_name));
    }
}

pub(super) fn set_process_priority_rule(
    settings: &mut ProcessPrioritySettings,
    process_name: &str,
    priority: ProcessPrioritySetting,
) {
    let rule = settings
        .exclusions
        .iter_mut()
        .find(|rule| process_setting_matches(&rule.process_name, process_name));
    let rule = if let Some(rule) = rule {
        rule
    } else {
        settings
            .exclusions
            .push(new_process_exclusion_rule(process_name));
        settings.exclusions.last_mut().expect("rule was just added")
    };
    rule.enabled = true;
    rule.set_process_priority_override(true, priority);
    rule.set_process_priority_override(false, priority);
}

pub(super) fn set_memory_priority_rule(
    settings: &mut MemoryPrioritySettings,
    process_name: &str,
    priority: ProcessMemoryPrioritySetting,
) {
    let rule = settings
        .exclusions
        .iter_mut()
        .find(|rule| process_setting_matches(&rule.process_name, process_name));
    let rule = if let Some(rule) = rule {
        rule
    } else {
        settings
            .exclusions
            .push(new_process_exclusion_rule(process_name));
        settings.exclusions.last_mut().expect("rule was just added")
    };
    rule.enabled = true;
    rule.set_memory_priority_override(true, priority);
    rule.set_memory_priority_override(false, priority);
}

pub(super) fn can_add_app_suspension_process(
    settings: &AppSuspensionSettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_suspendable_app(process),
        app_suspension::is_builtin_excluded,
    )
}

pub(super) fn can_add_core_steering_process(
    settings: &CoreSteeringSettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_rule_for(process),
        core_steering::is_builtin_excluded,
    )
}

pub(super) fn can_add_workload_engine_process(
    settings: &WorkloadEngineSettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_rule_for(process),
        workload_engine::is_builtin_excluded,
    )
}

pub(super) fn can_add_io_priority_exclusion(settings: &IoPrioritySettings, process: &str) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_exclusion(process),
        io_priority::is_builtin_excluded,
    )
}

pub(super) fn can_add_process_priority_exclusion(
    settings: &ProcessPrioritySettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_exclusion(process),
        process_priority::is_builtin_excluded,
    )
}

pub(super) fn can_add_thread_priority_exclusion(
    settings: &ThreadPrioritySettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_exclusion(process),
        thread_priority::is_builtin_excluded,
    )
}

pub(super) fn can_add_dynamic_priority_boost_exclusion(
    settings: &DynamicPriorityBoostSettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_exclusion(process),
        dynamic_priority_boost::is_builtin_excluded,
    )
}

pub(super) fn can_add_gpu_priority_exclusion(
    settings: &GpuPrioritySettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_exclusion(process),
        gpu_priority::is_builtin_excluded,
    )
}

pub(super) fn can_add_memory_priority_exclusion(
    settings: &MemoryPrioritySettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_exclusion(process),
        memory_priority::is_builtin_excluded,
    )
}

pub(super) fn can_add_timer_resolution_process(
    settings: &TimerResolutionSettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_rule_for(process),
        |_| false,
    )
}

pub(super) fn can_add_workload_engine_exclusion(
    settings: &WorkloadEngineSettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_workload_engine_exclusion(process),
        |_| false,
    )
}

pub(super) fn can_add_core_limiter_process(settings: &CoreLimiterSettings, process: &str) -> bool {
    can_add_process_candidate(
        process,
        |process| {
            settings
                .rules
                .iter()
                .any(|rule| same_process_name(&rule.process_name, process))
        },
        core_limiter::is_builtin_excluded,
    )
}

pub(super) fn can_add_by_running_app_process(
    settings: &ByRunningAppSettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| {
            settings
                .rules
                .iter()
                .any(|rule| same_process_name(&rule.process_name, process))
        },
        by_running_app::is_builtin_excluded,
    )
}

pub(super) fn new_app_suspension_rule(process: &str) -> AppSuspensionRule {
    AppSuspensionRule {
        enabled: true,
        process_name: process.trim().to_ascii_lowercase(),
        network_wake_enabled: true,
        audio_wake_enabled: true,
        network_download_threshold_bytes: 1,
        network_download_threshold_unit: NetworkThresholdUnit::Bytes,
        network_upload_threshold_bytes: 0,
        network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
    }
}

pub(super) fn new_core_steering_rule(process: &str) -> CoreSteeringRule {
    CoreSteeringRule {
        enabled: true,
        mode: CoreSteeringMode::Soft,
        process_name: process.trim().to_ascii_lowercase(),
        core_mask: default_affinity_mask(),
    }
}

pub(super) fn new_timer_resolution_rule(process: &str, desired_100ns: u32) -> TimerResolutionRule {
    TimerResolutionRule {
        enabled: true,
        process_name: process.trim().to_ascii_lowercase(),
        desired_100ns,
    }
}

pub(super) fn set_timer_resolution_override(
    settings: &mut TimerResolutionSettings,
    process_name: &str,
    desired_100ns: Option<u32>,
) {
    if let Some(desired_100ns) = desired_100ns {
        if let Some(rule) = settings
            .rules
            .iter_mut()
            .find(|rule| process_setting_matches(&rule.process_name, process_name))
        {
            rule.enabled = true;
            rule.desired_100ns = desired_100ns;
        } else {
            settings
                .rules
                .push(new_timer_resolution_rule(process_name, desired_100ns));
        }
    } else {
        settings
            .rules
            .retain(|rule| !process_setting_matches(&rule.process_name, process_name));
    }
}

pub(super) fn new_core_limiter_rule(process: &str) -> CoreLimiterRule {
    CoreLimiterRule {
        enabled: true,
        process_name: process.trim().to_ascii_lowercase(),
        threshold_percent: 75,
        sustain_seconds: 5,
        cooldown_seconds: 10,
        max_logical_processors: 1,
    }
}

pub(super) fn core_limiter_override_percent(
    settings: &CoreLimiterSettings,
    process_name: &str,
) -> Option<u8> {
    settings
        .rules
        .iter()
        .find(|rule| rule.enabled && process_setting_matches(&rule.process_name, process_name))
        .map(|rule| rule.max_logical_processors.min(100))
}

pub(super) fn set_core_limiter_override(
    settings: &mut CoreLimiterSettings,
    process_name: &str,
    max_logical_processors: Option<u8>,
) {
    if let Some(max_logical_processors) = max_logical_processors {
        if let Some(rule) = settings
            .rules
            .iter_mut()
            .find(|rule| process_setting_matches(&rule.process_name, process_name))
        {
            rule.enabled = true;
            rule.max_logical_processors = max_logical_processors.min(100);
        } else {
            let mut rule = new_core_limiter_rule(process_name);
            rule.max_logical_processors = max_logical_processors.min(100);
            settings.rules.push(rule);
        }
    } else {
        settings
            .rules
            .retain(|rule| !process_setting_matches(&rule.process_name, process_name));
    }
}

pub(super) fn set_app_suspension_override(
    settings: &mut AppSuspensionSettings,
    process_name: &str,
    included: bool,
) {
    if included {
        if let Some(rule) = settings
            .suspendable_apps
            .iter_mut()
            .find(|rule| process_setting_matches(&rule.process_name, process_name))
        {
            rule.enabled = true;
        } else {
            settings
                .suspendable_apps
                .push(new_app_suspension_rule(process_name));
        }
    } else {
        settings
            .suspendable_apps
            .retain(|rule| !process_setting_matches(&rule.process_name, process_name));
    }
}

pub(super) fn by_running_app_power_plan_override_guid(
    settings: &ByRunningAppSettings,
    process_name: &str,
) -> Option<String> {
    settings
        .rules
        .iter()
        .find(|rule| rule.enabled && process_setting_matches(&rule.process_name, process_name))
        .and_then(|rule| rule.power_plan_guid.clone())
}

pub(super) fn set_by_running_app_power_plan_override(
    settings: &mut ByRunningAppSettings,
    process_name: &str,
    power_plan_guid: Option<String>,
) {
    if let Some(power_plan_guid) = power_plan_guid {
        if let Some(rule) = settings
            .rules
            .iter_mut()
            .find(|rule| process_setting_matches(&rule.process_name, process_name))
        {
            rule.enabled = true;
            rule.power_plan_guid = Some(power_plan_guid);
        } else {
            settings
                .rules
                .push(new_by_running_app_rule(process_name, Some(power_plan_guid)));
        }
    } else {
        settings
            .rules
            .retain(|rule| !process_setting_matches(&rule.process_name, process_name));
    }
}

pub(super) fn process_list_timer_resolution_options(app: &WinderustApp) -> Vec<Option<u32>> {
    let mut options = Vec::new();
    for option in [
        None,
        Some(app.settings.timer_resolution.desired_100ns),
        Some(10_000),
        Some(20_000),
        Some(160_000),
    ] {
        if !options.contains(&option) {
            options.push(option);
        }
    }
    options
}

pub(super) fn core_limiter_indicator(
    status: &CoreLimiterSnapshot,
    process: &str,
) -> (String, u32, u32) {
    if core_limiter::is_builtin_excluded(process) {
        (
            t!("core_steering.indicator.protected").to_string(),
            settings_card_hover_color(),
            accent_color(),
        )
    } else if core_limiter_app_contains(&status.limited_apps, process) {
        (
            t!("core_limiter.indicator_limited").to_string(),
            success_bg_color(),
            success_text_color(),
        )
    } else if status.enabled {
        (
            t!("core_steering.indicator.ready").to_string(),
            panel_active_color(),
            muted_text_color(),
        )
    } else {
        (
            t!("core_steering.indicator.off").to_string(),
            panel_active_color(),
            dim_text_color(),
        )
    }
}

pub(super) fn core_limiter_app_contains(apps: &[String], process: &str) -> bool {
    apps.iter()
        .any(|app| app.trim().eq_ignore_ascii_case(process.trim()))
}

pub(super) fn new_by_running_app_rule(
    process: &str,
    power_plan_guid: Option<String>,
) -> ByRunningAppRule {
    let process_name = process.trim().to_ascii_lowercase();
    ByRunningAppRule {
        enabled: true,
        name: process_name.clone(),
        process_name,
        power_plan_guid,
    }
}

pub(super) fn foreground_lookup_required(settings: &Settings) -> bool {
    settings.by_foreground.enabled && !settings.by_foreground.rules.is_empty()
}

pub(super) fn by_running_app_decision(
    status: &ByRunningAppSnapshot,
) -> Option<ByRunningAppDecision> {
    Some(ByRunningAppDecision {
        rule_name: status.active_rule.clone()?,
        process_name: status.active_process.clone()?,
        power_plan_guid: status.target_guid.clone()?,
    })
}

pub(super) fn process_policy_summary(
    settings: &Settings,
    plans: &[PowerPlan],
    process_name: &str,
) -> ProcessPolicySummary {
    let mut summary = default_process_policy_summary();

    summary.power_plan_foreground =
        foreground_power_plan_policy_label(settings, plans, process_name);
    if foreground_power_plan_policy_is_custom(settings, process_name) {
        summary.mark_custom(ProcessListColumn::PowerPlanForeground);
    }
    summary.power_plan_running =
        running_app_power_plan_policy_label(&settings.by_running_app, plans, process_name);
    if running_app_power_plan_policy_is_custom(&settings.by_running_app, process_name) {
        summary.mark_custom(ProcessListColumn::PowerPlanRunning);
    }
    summary.background_efficiency = process_list_include_exclude_label(
        !settings
            .background_efficiency
            .custom_rule_enabled_for(process_name),
    );
    if settings
        .background_efficiency
        .custom_rule_enabled_for(process_name)
    {
        summary.mark_custom(ProcessListColumn::BackgroundEfficiency);
    }

    for rule in
        settings.workload_engine.rules.iter().filter(|rule| {
            rule.enabled && process_setting_matches(&rule.process_name, process_name)
        })
    {
        summary.process_priority = process_priority_label(rule.priority);
        summary.mark_custom(ProcessListColumn::ProcessPriority);
    }

    for rule in
        settings.core_limiter.rules.iter().filter(|rule| {
            rule.enabled && process_setting_matches(&rule.process_name, process_name)
        })
    {
        summary.core_limiter =
            process_list_include_value_label(format!("{}%", rule.max_logical_processors.min(100)));
        summary.mark_custom(ProcessListColumn::CoreLimiter);
    }

    if settings
        .background_cpu_restriction
        .exclusion_enabled_for(process_name)
    {
        summary.mark_custom(ProcessListColumn::BackgroundCpuRestriction);
    } else {
        summary.background_cpu_restriction =
            background_cpu_restriction_policy_label(&settings.background_cpu_restriction);
    }

    for rule in
        settings.core_steering.rules.iter().filter(|rule| {
            rule.enabled && process_setting_matches(&rule.process_name, process_name)
        })
    {
        summary.core_steering = core_steering_rule_label(rule);
        summary.mark_custom(ProcessListColumn::CoreSteering);
    }

    if settings.io_priority.exclusion_enabled_for(process_name) {
        summary.io_priority = process_list_exclude_label();
        summary.mark_custom(ProcessListColumn::IoPriority);
    } else {
        summary.io_priority = io_priority_policy_label(&settings.io_priority);
    }

    if settings.gpu_priority.exclusion_enabled_for(process_name) {
        summary.gpu_priority = process_list_exclude_label();
        summary.mark_custom(ProcessListColumn::GpuPriority);
    } else {
        summary.gpu_priority = gpu_priority_policy_label(&settings.gpu_priority);
    }

    if settings.memory_priority.exclusion_enabled_for(process_name) {
        summary.memory_priority = process_list_exclude_label();
        summary.mark_custom(ProcessListColumn::MemoryPriority);
    } else {
        summary.memory_priority = memory_priority_policy_label(&settings.memory_priority);
    }

    summary.memory_trim = process_list_include_exclude_label(
        !settings.memory_trim.exclusion_enabled_for(process_name),
    );
    if settings.memory_trim.exclusion_enabled_for(process_name) {
        summary.mark_custom(ProcessListColumn::MemoryTrim);
    }
    summary.app_suspension = process_list_include_exclude_label(
        settings
            .app_suspension
            .suspendable_app_enabled_for(process_name),
    );
    if settings
        .app_suspension
        .suspendable_app_enabled_for(process_name)
    {
        summary.mark_custom(ProcessListColumn::AppSuspension);
    }
    let timer_resolution_rule = settings
        .timer_resolution
        .desired_resolution_for_foreground(process_name);
    if let Some((_, desired_100ns)) = timer_resolution_rule {
        summary.timer_resolution = timer_resolution::format_resolution_ms(desired_100ns);
        summary.mark_custom(ProcessListColumn::TimerResolution);
    } else {
        summary.timer_resolution = process_list_default_label();
    }

    summary
}

pub(super) fn default_process_policy_summary() -> ProcessPolicySummary {
    ProcessPolicySummary {
        power_plan_foreground: process_list_default_label(),
        power_plan_running: process_list_default_label(),
        background_efficiency: process_list_include_label(),
        core_limiter: process_list_exclude_label(),
        background_cpu_restriction: process_list_off_label(),
        core_steering: default_core_steering_label(),
        process_priority: process_priority_setting_label(ProcessPrioritySetting::Default),
        io_priority: process_io_priority_setting_label(ProcessIoPrioritySetting::Default),
        gpu_priority: process_gpu_priority_setting_label(ProcessGpuPrioritySetting::Default),
        memory_priority: process_memory_priority_setting_label(
            ProcessMemoryPrioritySetting::Default,
        ),
        memory_trim: process_list_include_label(),
        app_suspension: process_list_exclude_label(),
        timer_resolution: process_list_default_label(),
        custom_columns: HashSet::new(),
    }
}

pub(super) fn process_setting_matches(configured_process: &str, process_name: &str) -> bool {
    let configured_process = configured_process.trim();
    !configured_process.is_empty() && same_process_name(configured_process, process_name)
}

pub(super) fn foreground_power_plan_policy_label(
    settings: &Settings,
    plans: &[PowerPlan],
    process_name: &str,
) -> String {
    if let Some(rule) = settings
        .by_foreground
        .rules
        .iter()
        .find(|rule| rule.enabled && process_setting_matches(&rule.process_name, process_name))
    {
        return power_plan_policy_value_label(plans, rule.power_plan_guid.as_deref());
    }

    process_list_default_label()
}

pub(super) fn foreground_power_plan_policy_is_custom(
    settings: &Settings,
    process_name: &str,
) -> bool {
    settings
        .by_foreground
        .rules
        .iter()
        .any(|rule| rule.enabled && process_setting_matches(&rule.process_name, process_name))
}

pub(super) fn running_app_power_plan_policy_label(
    settings: &ByRunningAppSettings,
    plans: &[PowerPlan],
    process_name: &str,
) -> String {
    if let Some(rule) = settings
        .rules
        .iter()
        .find(|rule| rule.enabled && process_setting_matches(&rule.process_name, process_name))
    {
        return power_plan_policy_value_label(plans, rule.power_plan_guid.as_deref());
    }

    process_list_default_label()
}

pub(super) fn running_app_power_plan_policy_is_custom(
    settings: &ByRunningAppSettings,
    process_name: &str,
) -> bool {
    settings
        .rules
        .iter()
        .any(|rule| rule.enabled && process_setting_matches(&rule.process_name, process_name))
}

pub(super) fn power_plan_policy_value_label(plans: &[PowerPlan], guid: Option<&str>) -> String {
    let Some(guid) = guid.map(str::trim).filter(|guid| !guid.is_empty()) else {
        return process_list_default_label();
    };

    plans
        .iter()
        .find(|plan| plan.guid.eq_ignore_ascii_case(guid))
        .map(|plan| plan.name.clone())
        .unwrap_or_else(|| guid.to_owned())
}

pub(super) fn process_list_off_label() -> String {
    "Off".to_owned()
}

pub(super) fn process_list_include_exclude_label(included: bool) -> String {
    if included {
        process_list_include_label()
    } else {
        process_list_exclude_label()
    }
}

pub(super) fn process_list_include_value_label(value: impl std::fmt::Display) -> String {
    format!("{} ({value})", process_list_include_label())
}

pub(super) fn process_list_include_label() -> String {
    "Include".to_owned()
}

pub(super) fn process_list_exclude_label() -> String {
    "Exclude".to_owned()
}

pub(super) fn process_list_default_label() -> String {
    t!("process_list.default").to_string()
}

pub(super) fn background_cpu_restriction_policy_label(
    settings: &BackgroundCpuRestrictionSettings,
) -> String {
    if settings.control_style == CpuRestrictionControlStyle::CoreToggle && settings.core_mask != 0 {
        return format_cpu_mask(settings.core_mask);
    }

    format!("{}%", settings.percent.min(100))
}

pub(super) fn core_steering_rule_label(rule: &CoreSteeringRule) -> String {
    match rule.mode {
        CoreSteeringMode::EfficiencyOff => {
            core_steering_mode_label(CoreSteeringMode::EfficiencyOff)
        }
        CoreSteeringMode::Hard | CoreSteeringMode::Soft => format_cpu_mask(rule.core_mask),
    }
}

pub(super) fn default_core_steering_label() -> String {
    format_cpu_mask(default_affinity_mask())
}

pub(super) fn io_priority_has_foreground_background_split(settings: &IoPrioritySettings) -> bool {
    settings.enabled
        && settings.foreground_detection_enabled
        && settings.foreground_priority != settings.background_priority
}

pub(super) fn io_priority_policy_label(settings: &IoPrioritySettings) -> String {
    if io_priority_has_foreground_background_split(settings) {
        format!(
            "{} / {}",
            process_io_priority_setting_label(settings.foreground_priority),
            process_io_priority_setting_label(settings.background_priority)
        )
    } else {
        process_io_priority_setting_label(settings.background_priority)
    }
}

pub(super) fn gpu_priority_has_foreground_background_split(settings: &GpuPrioritySettings) -> bool {
    settings.enabled
        && settings.foreground_detection_enabled
        && settings.foreground_priority != settings.background_priority
}

pub(super) fn gpu_priority_policy_label(settings: &GpuPrioritySettings) -> String {
    if gpu_priority_has_foreground_background_split(settings) {
        format!(
            "{} / {}",
            process_gpu_priority_setting_label(settings.foreground_priority),
            process_gpu_priority_setting_label(settings.background_priority)
        )
    } else {
        process_gpu_priority_setting_label(settings.background_priority)
    }
}

pub(super) fn memory_priority_has_foreground_background_split(
    settings: &MemoryPrioritySettings,
) -> bool {
    settings.enabled
        && settings.foreground_detection_enabled
        && settings.foreground_priority != settings.background_priority
}

pub(super) fn memory_priority_policy_label(settings: &MemoryPrioritySettings) -> String {
    if memory_priority_has_foreground_background_split(settings) {
        format!(
            "{} / {}",
            process_memory_priority_setting_label(settings.foreground_priority),
            process_memory_priority_setting_label(settings.background_priority)
        )
    } else {
        process_memory_priority_setting_label(settings.background_priority)
    }
}

pub(super) fn format_cpu_mask(mask: u64) -> String {
    if mask == 0 {
        return t!("common.none").to_string();
    }

    let mut ranges = Vec::new();
    let mut index = 0;
    while index < u64::BITS {
        if mask & (1_u64 << index) == 0 {
            index += 1;
            continue;
        }

        let start = index;
        while index + 1 < u64::BITS && mask & (1_u64 << (index + 1)) != 0 {
            index += 1;
        }
        let end = index;
        if start == end {
            ranges.push(start.to_string());
        } else {
            ranges.push(format!("{start}-{end}"));
        }
        index += 1;
    }

    ranges.join(", ")
}

pub(super) fn process_priority_label(priority: ProcessPriority) -> String {
    match priority {
        ProcessPriority::Normal => format!("8 ({})", t!("workload_engine.priority_normal")),
        ProcessPriority::BelowNormal => {
            format!("6 ({})", t!("workload_engine.priority_below_normal"))
        }
        ProcessPriority::Idle => format!("4 ({})", t!("workload_engine.priority_idle")),
    }
}

pub(super) fn process_priority_setting_label(priority: ProcessPrioritySetting) -> String {
    match priority {
        ProcessPrioritySetting::Default => t!("process_priority.priority_default").to_string(),
        ProcessPrioritySetting::Auto => t!("workload_engine.priority_auto").to_string(),
        ProcessPrioritySetting::Realtime => {
            format!("24 ({})", t!("process_priority.priority_realtime"))
        }
        ProcessPrioritySetting::High => format!("13 ({})", t!("process_priority.priority_high")),
        ProcessPrioritySetting::AboveNormal => {
            format!("10 ({})", t!("process_priority.priority_above_normal"))
        }
        ProcessPrioritySetting::Normal => {
            format!("8 ({})", t!("process_priority.priority_normal"))
        }
        ProcessPrioritySetting::BelowNormal => {
            format!("6 ({})", t!("process_priority.priority_below_normal"))
        }
        ProcessPrioritySetting::Idle => format!("4 ({})", t!("process_priority.priority_idle")),
    }
}

pub(super) fn process_thread_priority_setting_label(
    priority: ProcessThreadPrioritySetting,
) -> String {
    match priority {
        ProcessThreadPrioritySetting::Default => t!("thread_priority.priority_default").to_string(),
        ProcessThreadPrioritySetting::Auto => t!("workload_engine.priority_auto").to_string(),
        ProcessThreadPrioritySetting::TimeCritical => {
            format!("15 ({})", t!("thread_priority.priority_time_critical"))
        }
        ProcessThreadPrioritySetting::Highest => {
            format!("2 ({})", t!("thread_priority.priority_highest"))
        }
        ProcessThreadPrioritySetting::AboveNormal => {
            format!("1 ({})", t!("thread_priority.priority_above_normal"))
        }
        ProcessThreadPrioritySetting::Normal => {
            format!("0 ({})", t!("thread_priority.priority_normal"))
        }
        ProcessThreadPrioritySetting::BelowNormal => {
            format!("-1 ({})", t!("thread_priority.priority_below_normal"))
        }
        ProcessThreadPrioritySetting::Lowest => {
            format!("-2 ({})", t!("thread_priority.priority_lowest"))
        }
        ProcessThreadPrioritySetting::Idle => {
            format!("-15 ({})", t!("thread_priority.priority_idle"))
        }
    }
}

pub(super) fn process_dynamic_priority_boost_setting_label(
    boost: ProcessDynamicPriorityBoostSetting,
) -> String {
    match boost {
        ProcessDynamicPriorityBoostSetting::Default => {
            t!("dynamic_priority_boost.boost_default").to_string()
        }
        ProcessDynamicPriorityBoostSetting::Auto => t!("workload_engine.priority_auto").to_string(),
        ProcessDynamicPriorityBoostSetting::Enabled => {
            t!("dynamic_priority_boost.boost_enabled").to_string()
        }
        ProcessDynamicPriorityBoostSetting::Disabled => {
            t!("dynamic_priority_boost.boost_disabled").to_string()
        }
    }
}

pub(super) fn process_io_priority_setting_label(priority: ProcessIoPrioritySetting) -> String {
    match priority {
        ProcessIoPrioritySetting::Default => t!("io_priority.priority_default").to_string(),
        ProcessIoPrioritySetting::Auto => t!("workload_engine.priority_auto").to_string(),
        ProcessIoPrioritySetting::Critical => t!("io_priority.priority_critical").to_string(),
        ProcessIoPrioritySetting::High => t!("io_priority.priority_high").to_string(),
        ProcessIoPrioritySetting::Normal => t!("io_priority.priority_normal").to_string(),
        ProcessIoPrioritySetting::Low => t!("io_priority.priority_low").to_string(),
        ProcessIoPrioritySetting::VeryLow => t!("io_priority.priority_very_low").to_string(),
    }
}

pub(super) fn process_gpu_priority_setting_label(priority: ProcessGpuPrioritySetting) -> String {
    match priority {
        ProcessGpuPrioritySetting::Default => t!("gpu_priority.priority_default").to_string(),
        ProcessGpuPrioritySetting::Auto => t!("workload_engine.priority_auto").to_string(),
        ProcessGpuPrioritySetting::Realtime => t!("gpu_priority.priority_realtime").to_string(),
        ProcessGpuPrioritySetting::High => t!("gpu_priority.priority_high").to_string(),
        ProcessGpuPrioritySetting::AboveNormal => {
            t!("gpu_priority.priority_above_normal").to_string()
        }
        ProcessGpuPrioritySetting::Normal => t!("gpu_priority.priority_normal").to_string(),
        ProcessGpuPrioritySetting::BelowNormal => {
            t!("gpu_priority.priority_below_normal").to_string()
        }
        ProcessGpuPrioritySetting::Idle => t!("gpu_priority.priority_idle").to_string(),
    }
}

pub(super) fn timer_resolution_edit_value(value_100ns: u32) -> String {
    let milliseconds = value_100ns as f64 / 10_000.0;
    let value = format!("{milliseconds:.4}");
    value.trim_end_matches('0').trim_end_matches('.').to_owned()
}

pub(super) fn format_optional_timer_resolution(value_100ns: Option<u32>) -> String {
    value_100ns
        .map(timer_resolution::format_resolution_ms)
        .unwrap_or_else(|| t!("common.unknown").to_string())
}

pub(super) fn process_memory_priority_label(priority: ProcessMemoryPriority) -> String {
    match priority {
        ProcessMemoryPriority::VeryLow => {
            t!("workload_engine.memory_priority_very_low").to_string()
        }
        ProcessMemoryPriority::Low => t!("workload_engine.memory_priority_low").to_string(),
        ProcessMemoryPriority::Medium => t!("workload_engine.memory_priority_medium").to_string(),
        ProcessMemoryPriority::BelowNormal => {
            t!("workload_engine.memory_priority_below_normal").to_string()
        }
        ProcessMemoryPriority::Normal => t!("workload_engine.memory_priority_normal").to_string(),
    }
}
