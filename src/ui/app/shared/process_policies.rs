use crate::config::ProcessRuleMode;
use crate::ui::app::*;

pub(in crate::ui::app) fn process_target_can_accept(
    target: SuggestionTarget,
    settings: &Settings,
    process: &str,
    has_suspendable_instance: bool,
) -> bool {
    match target {
        SuggestionTarget::Foreground => {
            can_add_foreground_process(&settings.by_foreground, process)
        }
        SuggestionTarget::BackgroundEfficiency => {
            can_add_background_efficiency_process(&settings.background_efficiency, process)
        }
        SuggestionTarget::CpuSetsSoft | SuggestionTarget::ProcessorAffinityHard => {
            can_add_cpu_allocation_process(settings, process)
        }
        SuggestionTarget::MemoryTrim => {
            can_add_memory_trim_exclusion(&settings.memory_trim, process)
        }
        SuggestionTarget::AppSuspension => {
            has_suspendable_instance
                && can_add_app_suspension_process(&settings.app_suspension, process)
        }
        SuggestionTarget::CpuLimiter => can_add_cpu_limiter_process(&settings.cpu_limiter, process),
        SuggestionTarget::ByRunningApp => {
            can_add_by_running_app_process(&settings.by_running_app, process)
        }
        SuggestionTarget::CpuScheduler => {
            can_add_cpu_scheduler_custom_rule(&settings.cpu_scheduler, process)
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
    }
}

pub(in crate::ui::app) fn can_add_process_candidate(
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

pub(in crate::ui::app) fn can_add_foreground_process(
    settings: &ByForegroundSettings,
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
        |_| false,
    )
}

pub(in crate::ui::app) fn foreground_power_plan_override_guid(
    settings: &ByForegroundSettings,
    process_name: &str,
) -> Option<String> {
    settings
        .rules
        .iter()
        .find(|rule| rule.enabled && process_setting_matches(&rule.executable_path, process_name))
        .and_then(|rule| rule.power_plan_guid.clone())
}

pub(in crate::ui::app) fn set_foreground_power_plan_override(
    settings: &mut ByForegroundSettings,
    process_name: &str,
    power_plan_guid: Option<String>,
) {
    if let Some(power_plan_guid) = power_plan_guid {
        if let Some(rule) = settings
            .rules
            .iter_mut()
            .find(|rule| process_setting_matches(&rule.executable_path, process_name))
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
            .retain(|rule| !process_setting_matches(&rule.executable_path, process_name));
    }
}

pub(in crate::ui::app) fn new_foreground_rule(
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

pub(in crate::ui::app) fn can_add_background_efficiency_process(
    settings: &BackgroundEfficiencySettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_custom_rule(process),
        background_efficiency::is_builtin_excluded,
    )
}

pub(in crate::ui::app) fn can_add_memory_trim_exclusion(
    settings: &MemoryTrimSettings,
    process: &str,
) -> bool {
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

pub(in crate::ui::app) fn new_process_exclusion_rule(process: &str) -> ProcessExclusionRule {
    ProcessExclusionRule {
        executable_path: executable_path_key(Path::new(process)),
        ..Default::default()
    }
}

pub(in crate::ui::app) fn new_background_efficiency_rule(
    process: &str,
) -> BackgroundEfficiencyRule {
    BackgroundEfficiencyRule {
        enabled: true,
        executable_path: executable_path_key(Path::new(process)),
        focus_efficiency_mode: ProcessRuleMode::Default,
        visible_window_efficiency_mode: ProcessRuleMode::Default,
        background_efficiency_mode: ProcessRuleMode::Default,
    }
}

pub(in crate::ui::app) fn set_background_efficiency_custom_rule(
    settings: &mut BackgroundEfficiencySettings,
    process_name: &str,
    excluded: bool,
) {
    if excluded {
        if let Some(rule) = settings
            .custom_rules
            .iter_mut()
            .find(|rule| process_setting_matches(&rule.executable_path, process_name))
        {
            rule.enabled = true;
            rule.focus_efficiency_mode = ProcessRuleMode::Disabled;
            rule.visible_window_efficiency_mode = ProcessRuleMode::Disabled;
            rule.background_efficiency_mode = ProcessRuleMode::Disabled;
        } else {
            let mut rule = new_background_efficiency_rule(process_name);
            rule.focus_efficiency_mode = ProcessRuleMode::Disabled;
            rule.visible_window_efficiency_mode = ProcessRuleMode::Disabled;
            rule.background_efficiency_mode = ProcessRuleMode::Disabled;
            settings.custom_rules.push(rule);
        }
    } else {
        settings
            .custom_rules
            .retain(|rule| !process_setting_matches(&rule.executable_path, process_name));
    }
}

pub(in crate::ui::app) fn set_process_exclusion(
    rules: &mut Vec<ProcessExclusionRule>,
    process_name: &str,
    excluded: bool,
) {
    if excluded {
        if let Some(rule) = rules
            .iter_mut()
            .find(|rule| process_setting_matches(&rule.executable_path, process_name))
        {
            rule.enabled = true;
        } else {
            rules.push(new_process_exclusion_rule(process_name));
        }
    } else {
        rules.retain(|rule| !process_setting_matches(&rule.executable_path, process_name));
    }
}

pub(in crate::ui::app) fn set_process_priority_rule(
    settings: &mut ProcessPrioritySettings,
    process_name: &str,
    tier: Option<ProcessRuleTier>,
    priority: ProcessPrioritySetting,
) {
    set_priority_rule(&mut settings.exclusions, process_name, |rule| {
        set_priority_rule_tiers(tier, |foreground, visible_window| {
            rule.set_process_priority_override(foreground, visible_window, priority);
        });
    });
}

pub(in crate::ui::app) fn set_thread_priority_rule(
    settings: &mut ThreadPrioritySettings,
    process_name: &str,
    tier: Option<ProcessRuleTier>,
    priority: ProcessThreadPrioritySetting,
) {
    set_priority_rule(&mut settings.exclusions, process_name, |rule| {
        set_priority_rule_tiers(tier, |foreground, visible_window| {
            rule.set_thread_priority_override(foreground, visible_window, priority);
        });
    });
}

pub(in crate::ui::app) fn set_dynamic_priority_boost_rule(
    settings: &mut DynamicPriorityBoostSettings,
    process_name: &str,
    tier: Option<ProcessRuleTier>,
    boost: ProcessDynamicPriorityBoostSetting,
) {
    set_priority_rule(&mut settings.exclusions, process_name, |rule| {
        set_priority_rule_tiers(tier, |foreground, visible_window| {
            rule.set_dynamic_priority_boost_override(foreground, visible_window, boost);
        });
    });
}

pub(in crate::ui::app) fn set_io_priority_rule(
    settings: &mut IoPrioritySettings,
    process_name: &str,
    tier: Option<ProcessRuleTier>,
    priority: ProcessIoPrioritySetting,
) {
    set_priority_rule(&mut settings.exclusions, process_name, |rule| {
        set_priority_rule_tiers(tier, |foreground, visible_window| {
            rule.set_io_priority_override(foreground, visible_window, priority);
        });
    });
}

pub(in crate::ui::app) fn set_gpu_priority_rule(
    settings: &mut GpuPrioritySettings,
    process_name: &str,
    tier: Option<ProcessRuleTier>,
    priority: ProcessGpuPrioritySetting,
) {
    set_priority_rule(&mut settings.exclusions, process_name, |rule| {
        set_priority_rule_tiers(tier, |foreground, visible_window| {
            rule.set_gpu_priority_override(foreground, visible_window, priority);
        });
    });
}

pub(in crate::ui::app) fn set_memory_priority_rule(
    settings: &mut MemoryPrioritySettings,
    process_name: &str,
    tier: Option<ProcessRuleTier>,
    priority: ProcessMemoryPrioritySetting,
) {
    set_priority_rule(&mut settings.exclusions, process_name, |rule| {
        set_priority_rule_tiers(tier, |foreground, visible_window| {
            rule.set_memory_priority_override(foreground, visible_window, priority);
        });
    });
}

fn set_priority_rule_tiers(tier: Option<ProcessRuleTier>, mut update: impl FnMut(bool, bool)) {
    if let Some(tier) = tier {
        let (foreground, visible_window) = tier.flags();
        update(foreground, visible_window);
    } else {
        for tier in ProcessRuleTier::ALL {
            let (foreground, visible_window) = tier.flags();
            update(foreground, visible_window);
        }
    }
}

fn set_priority_rule(
    rules: &mut Vec<ProcessExclusionRule>,
    process_name: &str,
    update: impl FnOnce(&mut ProcessExclusionRule),
) {
    let index = rules
        .iter()
        .position(|rule| process_setting_matches(&rule.executable_path, process_name));
    let index = index.unwrap_or_else(|| {
        rules.push(new_process_exclusion_rule(process_name));
        rules.len() - 1
    });
    let rule = &mut rules[index];
    rule.enabled = true;
    update(rule);
}

pub(in crate::ui::app) fn can_add_app_suspension_process(
    settings: &AppSuspensionSettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_suspendable_app(process),
        app_suspension::is_builtin_excluded,
    )
}

pub(in crate::ui::app) fn can_add_cpu_allocation_process(
    settings: &Settings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| {
            settings.cpu_sets_soft.contains_rule_for(process)
                || settings.processor_affinity_hard.contains_rule_for(process)
        },
        cpu_allocation::is_builtin_excluded,
    )
}

pub(in crate::ui::app) fn can_add_io_priority_exclusion(
    settings: &IoPrioritySettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_exclusion(process),
        io_priority::is_builtin_excluded,
    )
}

pub(in crate::ui::app) fn can_add_process_priority_exclusion(
    settings: &ProcessPrioritySettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_exclusion(process),
        process_priority::is_builtin_excluded,
    )
}

pub(in crate::ui::app) fn can_add_thread_priority_exclusion(
    settings: &ThreadPrioritySettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_exclusion(process),
        thread_priority::is_builtin_excluded,
    )
}

pub(in crate::ui::app) fn can_add_dynamic_priority_boost_exclusion(
    settings: &DynamicPriorityBoostSettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_exclusion(process),
        dynamic_priority_boost::is_builtin_excluded,
    )
}

pub(in crate::ui::app) fn can_add_gpu_priority_exclusion(
    settings: &GpuPrioritySettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_exclusion(process),
        gpu_priority::is_builtin_excluded,
    )
}

pub(in crate::ui::app) fn can_add_memory_priority_exclusion(
    settings: &MemoryPrioritySettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_exclusion(process),
        memory_priority::is_builtin_excluded,
    )
}

pub(in crate::ui::app) fn can_add_timer_resolution_process(
    settings: &TimerResolutionSettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_rule_for(process),
        |_| false,
    )
}

pub(in crate::ui::app) fn can_add_cpu_scheduler_custom_rule(
    settings: &CpuSchedulerSettings,
    process: &str,
) -> bool {
    can_add_process_candidate(
        process,
        |process| settings.contains_custom_rule(process),
        cpu_scheduler::is_builtin_excluded,
    )
}

pub(in crate::ui::app) fn can_add_cpu_limiter_process(
    settings: &CpuLimiterSettings,
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
        cpu_limiter::is_builtin_excluded,
    )
}

pub(in crate::ui::app) fn can_add_by_running_app_process(
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

pub(in crate::ui::app) fn new_app_suspension_rule(process: &str) -> AppSuspensionRule {
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

pub(in crate::ui::app) fn new_cpu_allocation_rule(process: &str) -> CpuAllocationRule {
    let core_mask = default_affinity_mask();
    CpuAllocationRule {
        enabled: true,
        executable_path: executable_path_key(Path::new(process)),
        focus_core_mask: core_mask,
        visible_window_core_mask: core_mask,
        background_core_mask: core_mask,
    }
}

pub(in crate::ui::app) fn new_timer_resolution_rule(
    process: &str,
    desired_100ns: u32,
) -> TimerResolutionRule {
    TimerResolutionRule {
        enabled: true,
        executable_path: executable_path_key(Path::new(process)),
        desired_100ns,
    }
}
pub(in crate::ui::app) fn new_cpu_limiter_rule(process: &str) -> CpuLimiterRule {
    CpuLimiterRule {
        enabled: true,
        executable_path: executable_path_key(Path::new(process)),
        focus_mode: ProcessRuleMode::Disabled,
        visible_window_mode: ProcessRuleMode::Disabled,
        background_mode: ProcessRuleMode::Enabled,
        focus_allowed_cpu_time_percent: 50,
        visible_window_allowed_cpu_time_percent: 50,
        background_allowed_cpu_time_percent: 50,
    }
}
pub(in crate::ui::app) fn by_running_app_power_plan_override_guid(
    settings: &ByRunningAppSettings,
    process_name: &str,
) -> Option<String> {
    settings
        .rules
        .iter()
        .find(|rule| rule.enabled && process_setting_matches(&rule.executable_path, process_name))
        .and_then(|rule| rule.power_plan_guid.clone())
}

pub(in crate::ui::app) fn set_by_running_app_power_plan_override(
    settings: &mut ByRunningAppSettings,
    process_name: &str,
    power_plan_guid: Option<String>,
) {
    if let Some(power_plan_guid) = power_plan_guid {
        if let Some(rule) = settings
            .rules
            .iter_mut()
            .find(|rule| process_setting_matches(&rule.executable_path, process_name))
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
            .retain(|rule| !process_setting_matches(&rule.executable_path, process_name));
    }
}
pub(in crate::ui::app) fn cpu_limiter_indicator(
    status: &CpuLimiterSnapshot,
    process: &str,
) -> (String, u32, u32) {
    if cpu_limiter::is_builtin_excluded(process) {
        (
            t!("cpu_allocation.indicator.protected").to_string(),
            settings_card_hover_color(),
            accent_color(),
        )
    } else if cpu_limiter_app_contains(&status.limited_apps, process) {
        (
            t!("cpu_limiter.indicator_limited").to_string(),
            success_bg_color(),
            success_text_color(),
        )
    } else if status.enabled {
        (
            t!("cpu_allocation.indicator.ready").to_string(),
            panel_active_color(),
            muted_text_color(),
        )
    } else {
        (
            t!("cpu_allocation.indicator.off").to_string(),
            panel_active_color(),
            dim_text_color(),
        )
    }
}

pub(in crate::ui::app) fn cpu_limiter_app_contains(apps: &[String], process: &str) -> bool {
    apps.iter()
        .any(|app| same_executable_path(Path::new(app), Path::new(process)))
}

pub(in crate::ui::app) fn new_by_running_app_rule(
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

pub(in crate::ui::app) fn process_policy_summary(
    settings: &Settings,
    plans: &[PowerPlan],
    process_name: &str,
) -> ProcessPolicySummary {
    let mut summary = default_process_policy_summary();

    summary.power_plan_foreground =
        foreground_power_plan_policy_label(settings, plans, process_name);
    let foreground_power_plan_custom =
        foreground_power_plan_policy_is_custom(settings, process_name);
    if foreground_power_plan_custom {
        summary.mark_custom(ProcessListColumn::PowerPlanForeground);
    }
    summary.set_active(
        ProcessListColumn::PowerPlanForeground,
        foreground_power_plan_override_guid(&settings.by_foreground, process_name).is_some(),
    );
    summary.power_plan_running =
        running_app_power_plan_policy_label(&settings.by_running_app, plans, process_name);
    let running_power_plan_custom =
        running_app_power_plan_policy_is_custom(&settings.by_running_app, process_name);
    if running_power_plan_custom {
        summary.mark_custom(ProcessListColumn::PowerPlanRunning);
    }
    summary.set_active(
        ProcessListColumn::PowerPlanRunning,
        by_running_app_power_plan_override_guid(&settings.by_running_app, process_name).is_some(),
    );
    let adaptive_engine_excluded = settings.cpu_scheduler.custom_rule_enabled_for(process_name);
    summary.adaptive_engine = process_list_include_exclude_label(!adaptive_engine_excluded);
    summary.set_active(ProcessListColumn::AdaptiveEngine, !adaptive_engine_excluded);
    if adaptive_engine_excluded {
        summary.mark_custom(ProcessListColumn::AdaptiveEngine);
    }
    let background_efficiency_rule = settings.background_efficiency.custom_rule_for(process_name);
    let background_efficiency_active = background_efficiency_rule.is_none_or(|rule| {
        settings
            .background_efficiency
            .custom_rule_applies_efficiency_mode(rule)
    });
    summary.background_efficiency =
        process_list_include_exclude_label(background_efficiency_active);
    summary.set_active(
        ProcessListColumn::BackgroundEfficiency,
        background_efficiency_active,
    );
    if background_efficiency_rule.is_some() {
        summary.mark_custom(ProcessListColumn::BackgroundEfficiency);
    }

    let process_rule = process_rule_state(
        settings
            .process_priority
            .override_for(process_name, true, false),
        settings
            .process_priority
            .override_for(process_name, false, true),
        settings
            .process_priority
            .override_for(process_name, false, false),
    );
    summary.process_priority = priority_tier_label(
        process_rule.foreground,
        process_rule.visible_window,
        process_rule.background,
        process_priority_setting_label,
    );
    if process_rule.configured {
        summary.mark_custom(ProcessListColumn::ProcessPriority);
    }
    summary.set_active(ProcessListColumn::ProcessPriority, process_rule.active);

    let thread_rule = process_rule_state(
        settings
            .thread_priority
            .override_for(process_name, true, false),
        settings
            .thread_priority
            .override_for(process_name, false, true),
        settings
            .thread_priority
            .override_for(process_name, false, false),
    );
    summary.thread_priority = priority_tier_label(
        thread_rule.foreground,
        thread_rule.visible_window,
        thread_rule.background,
        process_thread_priority_setting_label,
    );
    if thread_rule.configured {
        summary.mark_custom(ProcessListColumn::ThreadPriority);
    }
    summary.set_active(ProcessListColumn::ThreadPriority, thread_rule.active);

    let boost_rule = process_rule_state(
        settings
            .dynamic_priority_boost
            .override_for(process_name, true, false),
        settings
            .dynamic_priority_boost
            .override_for(process_name, false, true),
        settings
            .dynamic_priority_boost
            .override_for(process_name, false, false),
    );
    summary.dynamic_priority_boost = priority_tier_label(
        boost_rule.foreground,
        boost_rule.visible_window,
        boost_rule.background,
        process_dynamic_priority_boost_setting_label,
    );
    if boost_rule.configured {
        summary.mark_custom(ProcessListColumn::DynamicPriorityBoost);
    }
    summary.set_active(ProcessListColumn::DynamicPriorityBoost, boost_rule.active);

    let io_rule = process_rule_state(
        settings.io_priority.override_for(process_name, true, false),
        settings.io_priority.override_for(process_name, false, true),
        settings
            .io_priority
            .override_for(process_name, false, false),
    );
    if io_rule.configured {
        summary.io_priority = if settings.io_priority.exclusion_enabled_for(process_name) {
            process_list_exclude_label()
        } else {
            priority_tier_label(
                io_rule.foreground,
                io_rule.visible_window,
                io_rule.background,
                process_io_priority_setting_label,
            )
        };
        summary.mark_custom(ProcessListColumn::IoPriority);
    } else {
        summary.io_priority = io_priority_policy_label(&settings.io_priority);
    }
    summary.set_active(ProcessListColumn::IoPriority, io_rule.active);

    let gpu_rule = process_rule_state(
        settings
            .gpu_priority
            .override_for(process_name, true, false),
        settings
            .gpu_priority
            .override_for(process_name, false, true),
        settings
            .gpu_priority
            .override_for(process_name, false, false),
    );
    if gpu_rule.configured {
        summary.gpu_priority = if settings.gpu_priority.exclusion_enabled_for(process_name) {
            process_list_exclude_label()
        } else {
            priority_tier_label(
                gpu_rule.foreground,
                gpu_rule.visible_window,
                gpu_rule.background,
                process_gpu_priority_setting_label,
            )
        };
        summary.mark_custom(ProcessListColumn::GpuPriority);
    } else {
        summary.gpu_priority = gpu_priority_policy_label(&settings.gpu_priority);
    }
    summary.set_active(ProcessListColumn::GpuPriority, gpu_rule.active);

    let memory_rule = process_rule_state(
        settings
            .memory_priority
            .override_for(process_name, true, false),
        settings
            .memory_priority
            .override_for(process_name, false, true),
        settings
            .memory_priority
            .override_for(process_name, false, false),
    );
    if memory_rule.configured {
        summary.memory_priority = if settings.memory_priority.exclusion_enabled_for(process_name) {
            process_list_exclude_label()
        } else {
            priority_tier_label(
                memory_rule.foreground,
                memory_rule.visible_window,
                memory_rule.background,
                process_memory_priority_setting_label,
            )
        };
        summary.mark_custom(ProcessListColumn::MemoryPriority);
    } else {
        summary.memory_priority = memory_priority_policy_label(&settings.memory_priority);
    }
    summary.set_active(ProcessListColumn::MemoryPriority, memory_rule.active);

    summary
}

struct ProcessRuleState<T> {
    foreground: T,
    visible_window: T,
    background: T,
    configured: bool,
    active: bool,
}

fn process_rule_state<T: Copy + Default>(
    foreground: Option<Option<T>>,
    visible_window: Option<Option<T>>,
    background: Option<Option<T>>,
) -> ProcessRuleState<T> {
    ProcessRuleState {
        foreground: foreground.flatten().unwrap_or_default(),
        visible_window: visible_window.flatten().unwrap_or_default(),
        background: background.flatten().unwrap_or_default(),
        configured: foreground.is_some() || visible_window.is_some() || background.is_some(),
        active: foreground.flatten().is_some()
            || visible_window.flatten().is_some()
            || background.flatten().is_some(),
    }
}

pub(in crate::ui::app) fn default_process_policy_summary() -> ProcessPolicySummary {
    ProcessPolicySummary {
        status: t!("process_list.status_active").to_string(),
        cpu_percent: None,
        memory_bytes: None,
        power_plan_foreground: process_list_default_label(),
        power_plan_running: process_list_default_label(),
        adaptive_engine: process_list_include_label(),
        background_efficiency: process_list_include_label(),
        process_priority: process_priority_setting_label(ProcessPrioritySetting::Default),
        thread_priority: process_thread_priority_setting_label(
            ProcessThreadPrioritySetting::Default,
        ),
        dynamic_priority_boost: process_dynamic_priority_boost_setting_label(
            ProcessDynamicPriorityBoostSetting::Default,
        ),
        io_priority: process_io_priority_setting_label(ProcessIoPrioritySetting::Default),
        gpu_priority: process_gpu_priority_setting_label(ProcessGpuPrioritySetting::Default),
        memory_priority: process_memory_priority_setting_label(
            ProcessMemoryPrioritySetting::Default,
        ),
        custom_columns: HashSet::new(),
        active_columns: HashSet::from([
            ProcessListColumn::AdaptiveEngine,
            ProcessListColumn::BackgroundEfficiency,
        ]),
    }
}

fn priority_tier_label<T: Copy + PartialEq>(
    foreground: T,
    visible_window: T,
    background: T,
    label: impl Fn(T) -> String,
) -> String {
    if foreground == visible_window && visible_window == background {
        label(foreground)
    } else {
        format!(
            "{} / {} / {}",
            label(foreground),
            label(visible_window),
            label(background)
        )
    }
}

pub(in crate::ui::app) fn process_setting_matches(
    configured_process: &str,
    process_name: &str,
) -> bool {
    let configured_process = configured_process.trim();
    !configured_process.is_empty()
        && same_executable_path(
            Path::new(configured_process),
            Path::new(process_name.trim()),
        )
}

pub(in crate::ui::app) fn process_path_matches_display_name(
    executable_path: &str,
    display_name: &str,
) -> bool {
    Path::new(executable_path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(display_name.trim()))
}

pub(in crate::ui::app) fn foreground_power_plan_policy_label(
    settings: &Settings,
    plans: &[PowerPlan],
    process_name: &str,
) -> String {
    if let Some(rule) =
        settings.by_foreground.rules.iter().find(|rule| {
            rule.enabled && process_setting_matches(&rule.executable_path, process_name)
        })
    {
        return power_plan_policy_value_label(plans, rule.power_plan_guid.as_deref());
    }

    process_list_default_label()
}

pub(in crate::ui::app) fn foreground_power_plan_policy_is_custom(
    settings: &Settings,
    process_name: &str,
) -> bool {
    settings
        .by_foreground
        .rules
        .iter()
        .any(|rule| rule.enabled && process_setting_matches(&rule.executable_path, process_name))
}

pub(in crate::ui::app) fn running_app_power_plan_policy_label(
    settings: &ByRunningAppSettings,
    plans: &[PowerPlan],
    process_name: &str,
) -> String {
    if let Some(rule) = settings
        .rules
        .iter()
        .find(|rule| rule.enabled && process_setting_matches(&rule.executable_path, process_name))
    {
        return power_plan_policy_value_label(plans, rule.power_plan_guid.as_deref());
    }

    process_list_default_label()
}

pub(in crate::ui::app) fn running_app_power_plan_policy_is_custom(
    settings: &ByRunningAppSettings,
    process_name: &str,
) -> bool {
    settings
        .rules
        .iter()
        .any(|rule| rule.enabled && process_setting_matches(&rule.executable_path, process_name))
}

pub(in crate::ui::app) fn power_plan_policy_value_label(
    plans: &[PowerPlan],
    guid: Option<&str>,
) -> String {
    let Some(guid) = guid.map(str::trim).filter(|guid| !guid.is_empty()) else {
        return process_list_default_label();
    };

    plans
        .iter()
        .find(|plan| plan.guid.eq_ignore_ascii_case(guid))
        .map(|plan| plan.name.clone())
        .unwrap_or_else(|| guid.to_owned())
}
pub(in crate::ui::app) fn process_list_include_exclude_label(included: bool) -> String {
    if included {
        process_list_include_label()
    } else {
        process_list_exclude_label()
    }
}
pub(in crate::ui::app) fn process_list_include_label() -> String {
    t!("process_list.include").to_string()
}

pub(in crate::ui::app) fn process_list_exclude_label() -> String {
    t!("process_list.exclude").to_string()
}

pub(in crate::ui::app) fn process_list_default_label() -> String {
    t!("process_list.default").to_string()
}
pub(in crate::ui::app) fn io_priority_has_tier_split(settings: &IoPrioritySettings) -> bool {
    priority_policy_has_tier_split(
        settings.enabled,
        settings.foreground_detection_enabled,
        settings.visible_window_detection_enabled,
        settings.foreground_priority,
        settings.visible_window_priority,
        settings.background_priority,
    )
}

pub(in crate::ui::app) fn io_priority_policy_label(settings: &IoPrioritySettings) -> String {
    priority_policy_label(
        settings.foreground_detection_enabled,
        settings.visible_window_detection_enabled,
        settings.foreground_priority,
        settings.visible_window_priority,
        settings.background_priority,
        process_io_priority_setting_label,
    )
}

pub(in crate::ui::app) fn gpu_priority_has_tier_split(settings: &GpuPrioritySettings) -> bool {
    priority_policy_has_tier_split(
        settings.enabled,
        settings.foreground_detection_enabled,
        settings.visible_window_detection_enabled,
        settings.foreground_priority,
        settings.visible_window_priority,
        settings.background_priority,
    )
}

pub(in crate::ui::app) fn gpu_priority_policy_label(settings: &GpuPrioritySettings) -> String {
    priority_policy_label(
        settings.foreground_detection_enabled,
        settings.visible_window_detection_enabled,
        settings.foreground_priority,
        settings.visible_window_priority,
        settings.background_priority,
        process_gpu_priority_setting_label,
    )
}

pub(in crate::ui::app) fn memory_priority_has_tier_split(
    settings: &MemoryPrioritySettings,
) -> bool {
    priority_policy_has_tier_split(
        settings.enabled,
        settings.foreground_detection_enabled,
        settings.visible_window_detection_enabled,
        settings.foreground_priority,
        settings.visible_window_priority,
        settings.background_priority,
    )
}

pub(in crate::ui::app) fn memory_priority_policy_label(
    settings: &MemoryPrioritySettings,
) -> String {
    priority_policy_label(
        settings.foreground_detection_enabled,
        settings.visible_window_detection_enabled,
        settings.foreground_priority,
        settings.visible_window_priority,
        settings.background_priority,
        process_memory_priority_setting_label,
    )
}

fn priority_policy_has_tier_split<T: PartialEq>(
    enabled: bool,
    foreground_detection_enabled: bool,
    visible_window_detection_enabled: bool,
    foreground: T,
    visible_window: T,
    background: T,
) -> bool {
    enabled
        && (foreground_detection_enabled && foreground != background
            || visible_window_detection_enabled && visible_window != background)
}

fn priority_policy_label<T: Copy + PartialEq>(
    foreground_detection_enabled: bool,
    visible_window_detection_enabled: bool,
    foreground: T,
    visible_window: T,
    background: T,
    label: impl Fn(T) -> String,
) -> String {
    priority_tier_label(
        if foreground_detection_enabled {
            foreground
        } else {
            background
        },
        if visible_window_detection_enabled {
            visible_window
        } else {
            background
        },
        background,
        label,
    )
}
pub(in crate::ui::app) fn process_priority_setting_label(
    priority: ProcessPrioritySetting,
) -> String {
    match priority {
        ProcessPrioritySetting::Default => t!("process_priority.priority_default").to_string(),
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

pub(in crate::ui::app) fn process_thread_priority_setting_label(
    priority: ProcessThreadPrioritySetting,
) -> String {
    match priority {
        ProcessThreadPrioritySetting::Default => t!("thread_priority.priority_default").to_string(),
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

pub(in crate::ui::app) fn process_dynamic_priority_boost_setting_label(
    boost: ProcessDynamicPriorityBoostSetting,
) -> String {
    match boost {
        ProcessDynamicPriorityBoostSetting::Default => {
            t!("dynamic_priority_boost.boost_default").to_string()
        }
        ProcessDynamicPriorityBoostSetting::Enabled => {
            t!("dynamic_priority_boost.boost_enabled").to_string()
        }
        ProcessDynamicPriorityBoostSetting::Disabled => {
            t!("dynamic_priority_boost.boost_disabled").to_string()
        }
    }
}

pub(in crate::ui::app) fn process_io_priority_setting_label(
    priority: ProcessIoPrioritySetting,
) -> String {
    match priority {
        ProcessIoPrioritySetting::Default => t!("io_priority.priority_default").to_string(),
        ProcessIoPrioritySetting::Critical => {
            process_io_priority_label(ProcessIoPriority::Critical)
        }
        ProcessIoPrioritySetting::High => process_io_priority_label(ProcessIoPriority::High),
        ProcessIoPrioritySetting::Normal => process_io_priority_label(ProcessIoPriority::Normal),
        ProcessIoPrioritySetting::Low => process_io_priority_label(ProcessIoPriority::Low),
        ProcessIoPrioritySetting::VeryLow => process_io_priority_label(ProcessIoPriority::VeryLow),
    }
}

pub(in crate::ui::app) fn process_io_priority_label(priority: ProcessIoPriority) -> String {
    match priority {
        ProcessIoPriority::Critical => t!("io_priority.priority_critical"),
        ProcessIoPriority::High => t!("io_priority.priority_high"),
        ProcessIoPriority::Normal => t!("io_priority.priority_normal"),
        ProcessIoPriority::Low => t!("io_priority.priority_low"),
        ProcessIoPriority::VeryLow => t!("io_priority.priority_very_low"),
    }
    .to_string()
}

pub(in crate::ui::app) fn process_gpu_priority_setting_label(
    priority: ProcessGpuPrioritySetting,
) -> String {
    match priority {
        ProcessGpuPrioritySetting::Default => t!("gpu_priority.priority_default").to_string(),
        ProcessGpuPrioritySetting::Realtime => {
            process_gpu_priority_label(ProcessGpuPriority::Realtime)
        }
        ProcessGpuPrioritySetting::High => process_gpu_priority_label(ProcessGpuPriority::High),
        ProcessGpuPrioritySetting::AboveNormal => {
            process_gpu_priority_label(ProcessGpuPriority::AboveNormal)
        }
        ProcessGpuPrioritySetting::Normal => process_gpu_priority_label(ProcessGpuPriority::Normal),
        ProcessGpuPrioritySetting::BelowNormal => {
            process_gpu_priority_label(ProcessGpuPriority::BelowNormal)
        }
        ProcessGpuPrioritySetting::Idle => process_gpu_priority_label(ProcessGpuPriority::Idle),
    }
}

pub(in crate::ui::app) fn process_gpu_priority_label(priority: ProcessGpuPriority) -> String {
    match priority {
        ProcessGpuPriority::Realtime => t!("gpu_priority.priority_realtime"),
        ProcessGpuPriority::High => t!("gpu_priority.priority_high"),
        ProcessGpuPriority::AboveNormal => t!("gpu_priority.priority_above_normal"),
        ProcessGpuPriority::Normal => t!("gpu_priority.priority_normal"),
        ProcessGpuPriority::BelowNormal => t!("gpu_priority.priority_below_normal"),
        ProcessGpuPriority::Idle => t!("gpu_priority.priority_idle"),
    }
    .to_string()
}

pub(in crate::ui::app) fn timer_resolution_edit_value(value_100ns: u32) -> String {
    let milliseconds = value_100ns as f64 / 10_000.0;
    let value = format!("{milliseconds:.4}");
    value.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_process_path_requires_its_display_name() {
        assert!(process_path_matches_display_name(
            r"C:\Apps\Example.exe",
            "example.EXE"
        ));
        assert!(!process_path_matches_display_name(
            r"C:\Apps\Example.exe",
            "Other.exe"
        ));
    }

    #[test]
    fn app_suspension_requires_a_suspendable_process_instance() {
        let settings = Settings::default();
        let process = "C:/Apps/Example.exe";

        assert!(process_target_can_accept(
            SuggestionTarget::AppSuspension,
            &settings,
            process,
            true,
        ));
        assert!(!process_target_can_accept(
            SuggestionTarget::AppSuspension,
            &settings,
            process,
            false,
        ));
        assert!(process_target_can_accept(
            SuggestionTarget::Foreground,
            &settings,
            process,
            false,
        ));
    }

    #[test]
    fn new_cpu_limiter_rules_limit_background_apps_to_half_time() {
        let rule = new_cpu_limiter_rule(r"C:\Apps\encoder.exe");

        assert_eq!(rule.focus_allowed_cpu_time_percent, 50);
        assert_eq!(rule.visible_window_allowed_cpu_time_percent, 50);
        assert_eq!(rule.background_allowed_cpu_time_percent, 50);
        assert_eq!(rule.focus_mode, ProcessRuleMode::Disabled);
        assert_eq!(rule.visible_window_mode, ProcessRuleMode::Disabled);
        assert_eq!(rule.background_mode, ProcessRuleMode::Enabled);
    }
}
