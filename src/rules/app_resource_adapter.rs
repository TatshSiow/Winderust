#![allow(dead_code)]

use crate::{
    config::{
        AppSuspensionSettings, BackgroundCpuRestrictionSettings, CpuAffinityMode,
        CpuAffinitySettings, CpuLimiterSettings, EcoQosCpuRestrictionMode, EcoQosSettings,
        ForegroundBoostPriority, ForegroundResponsivenessSettings,
        ProcessPriority as ConfigPriority, Settings, WatchdogAction, WatchdogSettings,
    },
    foreground::process_name_key,
    rules::{
        Action, AffinityPolicy, AppMatcher, Rule, RuleId, RuleProcessPriority, Trigger,
        PRIORITY_FOREGROUND_RESPONSIVENESS, PRIORITY_WATCHDOG,
    },
};

const AUTO_BALANCE_FOREGROUND_SATURATION_PERCENT: f32 = 85.0;

pub fn active_app_resource_rules_for_settings(
    settings: &Settings,
    foreground_app: Option<&str>,
    total_cpu_usage_percent: Option<f32>,
) -> Vec<Rule> {
    let mut rules = Vec::new();
    rules.extend(foreground_responsiveness_rules(
        &settings.foreground_responsiveness,
        foreground_app,
        total_cpu_usage_percent,
        settings.eco_qos.enabled,
    ));
    rules.extend(watchdog_rules(&settings.watchdog));
    rules.extend(cpu_affinity_rules(&settings.cpu_affinity));
    rules.extend(background_cpu_restriction_rules(
        &settings.background_cpu_restriction,
    ));
    rules.extend(app_suspension_rules(&settings.app_suspension));
    rules.extend(eco_qos_rules(&settings.eco_qos));
    rules.extend(cpu_limiter_rules(&settings.cpu_limiter));
    rules
}

pub fn cpu_limiter_rules(settings: &CpuLimiterSettings) -> Vec<Rule> {
    if !settings.enabled {
        return Vec::new();
    }

    settings
        .rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter_map(|rule| {
            let process_name = rule.process_name.trim();
            if process_name.is_empty() {
                return None;
            }

            let process_key = process_name_key(process_name);
            Some(Rule {
                id: RuleId(format!("cpu-limiter.{process_key}")),
                name: format!("CPU limiter: {process_name}"),
                enabled: true,
                priority: crate::rules::PRIORITY_BACKGROUND_APP,
                trigger: Trigger::CpuLoadAbove {
                    percent: rule.threshold_percent.min(100),
                    duration_secs: rule.sustain_seconds,
                },
                actions: vec![Action::SetAppCpuLimit {
                    app: AppMatcher::ProcessName(process_key),
                    logical_processor_percent: rule.max_logical_processors.min(100),
                }],
                restore_actions: Vec::new(),
                cooldown_secs: rule.cooldown_seconds,
            })
        })
        .collect()
}

pub fn eco_qos_rules(settings: &EcoQosSettings) -> Vec<Rule> {
    if !settings.enabled {
        return Vec::new();
    }

    vec![Rule {
        id: RuleId("ecoqos.background-efficiency-policy".to_owned()),
        name: "EcoQoS background efficiency policy".to_owned(),
        enabled: true,
        priority: crate::rules::PRIORITY_BACKGROUND_APP,
        trigger: Trigger::AppBackground {
            app: AppMatcher::Pattern("*".to_owned()),
            duration_secs: 0,
        },
        actions: vec![Action::ConfigureBackgroundEfficiencyPolicy {
            exclusions: settings
                .efficiency_whitelist
                .iter()
                .filter(|rule| rule.enabled)
                .map(|rule| AppMatcher::ProcessName(process_name_key(&rule.process_name)))
                .collect(),
            prefer_efficiency_cores: settings.prefer_efficiency_cores,
            logical_processor_percent: (settings.cpu_restriction_percent < 100)
                .then_some(settings.cpu_restriction_percent),
        }],
        restore_actions: Vec::new(),
        cooldown_secs: 0,
    }]
}

pub fn cpu_affinity_rules(settings: &CpuAffinitySettings) -> Vec<Rule> {
    if !settings.enabled {
        return Vec::new();
    }

    settings
        .rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter_map(|rule| {
            let process_name = rule.process_name.trim();
            if process_name.is_empty()
                || (rule.mode != CpuAffinityMode::EfficiencyOff && rule.core_mask == 0)
            {
                return None;
            }

            let process_key = process_name_key(process_name);
            let app = AppMatcher::ProcessName(process_key.clone());
            let affinity = match rule.mode {
                CpuAffinityMode::Hard => AffinityPolicy::CustomMask(rule.core_mask),
                CpuAffinityMode::Soft => AffinityPolicy::CpuSetMask(rule.core_mask),
                CpuAffinityMode::EfficiencyOff => AffinityPolicy::DisableEfficiencyMode,
            };
            Some(Rule {
                id: RuleId(format!("cpu-affinity.{process_key}")),
                name: format!("CPU affinity: {process_name}"),
                enabled: true,
                priority: crate::rules::PRIORITY_BACKGROUND_APP,
                trigger: Trigger::AppRunning { app: app.clone() },
                actions: vec![Action::SetAppAffinity { app, affinity }],
                restore_actions: Vec::new(),
                cooldown_secs: 0,
            })
        })
        .collect()
}

pub fn background_cpu_restriction_rules(settings: &BackgroundCpuRestrictionSettings) -> Vec<Rule> {
    if !settings.enabled {
        return Vec::new();
    }

    vec![Rule {
        id: RuleId("background-cpu-restriction.policy".to_owned()),
        name: "Background CPU restriction policy".to_owned(),
        enabled: true,
        priority: crate::rules::PRIORITY_BACKGROUND_APP,
        trigger: Trigger::AppBackground {
            app: AppMatcher::Pattern("*".to_owned()),
            duration_secs: 0,
        },
        actions: vec![Action::SetAppCpuLimit {
            app: AppMatcher::Pattern("*".to_owned()),
            logical_processor_percent: settings.percent.min(100),
        }],
        restore_actions: Vec::new(),
        cooldown_secs: 0,
    }]
}

pub fn app_suspension_rules(settings: &AppSuspensionSettings) -> Vec<Rule> {
    if !settings.enabled {
        return Vec::new();
    }

    settings
        .suspendable_apps
        .iter()
        .filter(|rule| rule.enabled)
        .filter_map(|rule| {
            let process_name = rule.process_name.trim();
            if process_name.is_empty() {
                return None;
            }

            let process_key = process_name_key(process_name);
            let app = AppMatcher::ProcessName(process_key.clone());
            Some(Rule {
                id: RuleId(format!("app-suspension.{process_key}")),
                name: format!("App suspension: {process_name}"),
                enabled: true,
                priority: crate::rules::PRIORITY_BACKGROUND_APP,
                trigger: Trigger::AppBackgroundIdle {
                    app: app.clone(),
                    duration_secs: settings.background_delay_seconds,
                },
                actions: vec![Action::SuspendApp { app }],
                restore_actions: Vec::new(),
                cooldown_secs: settings.background_delay_seconds,
            })
        })
        .collect()
}

pub fn watchdog_rules(settings: &WatchdogSettings) -> Vec<Rule> {
    if !settings.enabled {
        return Vec::new();
    }

    settings
        .rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter_map(|rule| {
            let process_name = rule.process_name.trim();
            if process_name.is_empty() {
                return None;
            }

            let process_key = process_name_key(process_name);
            let app = AppMatcher::ProcessName(process_key.clone());
            let (trigger, actions) = match rule.action {
                WatchdogAction::TerminateOnLaunch => (
                    Trigger::ProcessStarted { app: app.clone() },
                    vec![Action::TerminateApp { app }],
                ),
                WatchdogAction::RestartIfExited => (
                    Trigger::ProcessMissing {
                        app: app.clone(),
                        duration_secs: rule.restart_delay_seconds,
                    },
                    vec![Action::RestartApp {
                        app,
                        launch_path: rule.launch_path.clone(),
                        args: rule.launch_args.clone(),
                    }],
                ),
            };

            Some(Rule {
                id: RuleId(format!("watchdog.{process_key}")),
                name: if rule.name.trim().is_empty() {
                    format!("Watchdog: {process_name}")
                } else {
                    rule.name.clone()
                },
                enabled: true,
                priority: PRIORITY_WATCHDOG,
                trigger,
                actions,
                restore_actions: Vec::new(),
                cooldown_secs: rule.restart_delay_seconds,
            })
        })
        .collect()
}

pub fn foreground_responsiveness_rules(
    settings: &ForegroundResponsivenessSettings,
    foreground_app: Option<&str>,
    total_cpu_usage_percent: Option<f32>,
    background_efficiency_managed: bool,
) -> Vec<Rule> {
    if !settings.enabled {
        return Vec::new();
    }

    let mut rules = Vec::new();

    if settings.lower_background_apps && !background_efficiency_managed {
        let mut actions = Vec::new();
        if smart_efficiency_should_run(settings, total_cpu_usage_percent) {
            actions.push(Action::ConfigureBackgroundEfficiencyPolicy {
                exclusions: foreground_app
                    .map(|app| AppMatcher::ProcessName(process_name_key(app)))
                    .into_iter()
                    .collect(),
                prefer_efficiency_cores: true,
                logical_processor_percent: None,
            });
        }
        actions.extend(
            settings
                .rules
                .iter()
                .filter(|rule| rule.enabled)
                .filter_map(|rule| {
                    let process_name = rule.process_name.trim();
                    (!process_name.is_empty()).then(|| Action::SetAppPriority {
                        app: AppMatcher::ProcessName(process_name_key(process_name)),
                        priority: map_process_priority(rule.priority),
                    })
                }),
        );
        if !actions.is_empty() {
            rules.push(Rule {
                id: RuleId("foreground-responsiveness.background-efficiency".to_owned()),
                name: "Foreground Responsiveness background efficiency".to_owned(),
                enabled: true,
                priority: PRIORITY_FOREGROUND_RESPONSIVENESS,
                trigger: Trigger::AppBackground {
                    app: AppMatcher::Pattern("*".to_owned()),
                    duration_secs: 0,
                },
                actions,
                restore_actions: Vec::new(),
                cooldown_secs: 0,
            });
        }
    }

    if settings.boost_foreground_app {
        if let Some(app) = foreground_app.map(str::trim).filter(|app| !app.is_empty()) {
            let app_key = process_name_key(app);
            rules.push(Rule {
                id: RuleId(format!("foreground-responsiveness.boost.{app_key}")),
                name: format!("Foreground Responsiveness boost: {app}"),
                enabled: true,
                priority: PRIORITY_FOREGROUND_RESPONSIVENESS,
                trigger: Trigger::AppFocused {
                    app: AppMatcher::ProcessName(app_key.clone()),
                },
                actions: vec![Action::BoostForegroundPriority {
                    app: AppMatcher::ProcessName(app_key),
                    priority: map_foreground_boost_priority(
                        settings.foreground_boost,
                        total_cpu_usage_percent,
                    ),
                }],
                restore_actions: Vec::new(),
                cooldown_secs: 0,
            });
        }
    }

    if settings.auto_balance_enabled
        && total_cpu_usage_percent.is_some_and(|usage| {
            usage >= f32::from(settings.auto_balance_total_threshold_percent.min(100))
                && !foreground_cpu_saturates_workload(usage)
        })
    {
        if let Some(action) = auto_balance_affinity_action(settings, total_cpu_usage_percent) {
            rules.push(Rule {
                id: RuleId("foreground-responsiveness.auto-balance".to_owned()),
                name: "Foreground Responsiveness auto-balance".to_owned(),
                enabled: true,
                priority: PRIORITY_FOREGROUND_RESPONSIVENESS,
                trigger: Trigger::ForegroundCpuPressure {
                    foreground: foreground_app
                        .map(|app| AppMatcher::ProcessName(process_name_key(app)))
                        .unwrap_or_else(|| AppMatcher::Pattern("*".to_owned())),
                    total_cpu_above_percent: settings.auto_balance_total_threshold_percent.min(100),
                    background_process_above_percent: settings
                        .auto_balance_threshold_percent
                        .min(100),
                    duration_secs: settings.auto_balance_sustain_seconds,
                },
                actions: vec![action],
                restore_actions: Vec::new(),
                cooldown_secs: settings.auto_balance_cooldown_seconds,
            });
        }
    }

    rules
}

fn map_foreground_boost_priority(
    priority: ForegroundBoostPriority,
    foreground_cpu_usage_percent: Option<f32>,
) -> RuleProcessPriority {
    match priority {
        ForegroundBoostPriority::Auto => {
            if foreground_cpu_usage_percent.is_some_and(foreground_cpu_saturates_workload) {
                RuleProcessPriority::Normal
            } else {
                RuleProcessPriority::AboveNormal
            }
        }
        ForegroundBoostPriority::Normal => RuleProcessPriority::Normal,
        ForegroundBoostPriority::AboveNormal => RuleProcessPriority::AboveNormal,
    }
}

fn auto_balance_affinity_action(
    settings: &ForegroundResponsivenessSettings,
    foreground_cpu_usage_percent: Option<f32>,
) -> Option<Action> {
    if settings.lower_background_auto_cpu_percent {
        return None;
    }

    let percent = auto_balance_effective_cpu_percent(settings, foreground_cpu_usage_percent);
    if percent >= 100 {
        return None;
    }

    percent_affinity_action(auto_balance_effective_restriction_mode(settings), percent)
}

fn foreground_cpu_saturates_workload(usage: f32) -> bool {
    usage >= AUTO_BALANCE_FOREGROUND_SATURATION_PERCENT
}

fn smart_efficiency_should_run(
    settings: &ForegroundResponsivenessSettings,
    foreground_cpu_usage_percent: Option<f32>,
) -> bool {
    if !settings.lower_background_auto_cpu_percent {
        return true;
    }

    foreground_cpu_usage_percent.is_some_and(|usage| {
        usage >= f32::from(settings.auto_balance_total_threshold_percent.min(100))
            && !foreground_cpu_saturates_workload(usage)
    })
}

fn auto_balance_effective_restriction_mode(
    settings: &ForegroundResponsivenessSettings,
) -> EcoQosCpuRestrictionMode {
    if settings.lower_background_auto_cpu_percent {
        EcoQosCpuRestrictionMode::SoftCpuSets
    } else {
        settings.auto_balance_affinity_mode
    }
}

fn auto_balance_effective_cpu_percent(
    settings: &ForegroundResponsivenessSettings,
    foreground_cpu_usage_percent: Option<f32>,
) -> u8 {
    let configured = auto_balance_minimum_cpu_percent(settings);
    let Some(usage) = foreground_cpu_usage_percent else {
        return configured;
    };
    let threshold = f32::from(settings.auto_balance_total_threshold_percent.min(100));
    let saturation = AUTO_BALANCE_FOREGROUND_SATURATION_PERCENT;
    if usage >= saturation || threshold >= saturation {
        return if settings.lower_background_auto_cpu_percent {
            100
        } else {
            configured
        };
    }

    let relaxed = if settings.lower_background_auto_cpu_percent {
        100.0
    } else {
        ((u16::from(configured) + 100) / 2) as f32
    };
    let pressure = ((usage - threshold) / (saturation - threshold)).clamp(0.0, 1.0);
    (relaxed - ((relaxed - f32::from(configured)) * pressure))
        .round()
        .clamp(f32::from(configured), 100.0) as u8
}

fn auto_balance_minimum_cpu_percent(settings: &ForegroundResponsivenessSettings) -> u8 {
    if !settings.lower_background_auto_cpu_percent {
        return settings.auto_balance_cpu_percent.clamp(1, 100);
    }

    let trigger = settings.auto_balance_total_threshold_percent.min(100);
    if trigger >= 80 {
        85
    } else if trigger >= 70 {
        75
    } else {
        65
    }
}

fn percent_affinity_action(mode: EcoQosCpuRestrictionMode, percent: u8) -> Option<Action> {
    let percent = percent.clamp(1, 100);
    let affinity = match mode {
        EcoQosCpuRestrictionMode::SoftCpuSets => AffinityPolicy::LogicalProcessorPercent(percent),
        EcoQosCpuRestrictionMode::HardAffinity => AffinityPolicy::LogicalProcessorPercent(percent),
    };
    Some(Action::SetAppAffinity {
        app: AppMatcher::Pattern("*".to_owned()),
        affinity,
    })
}

fn map_process_priority(priority: ConfigPriority) -> RuleProcessPriority {
    match priority {
        ConfigPriority::Normal => RuleProcessPriority::Normal,
        ConfigPriority::BelowNormal => RuleProcessPriority::BelowNormal,
        ConfigPriority::Idle => RuleProcessPriority::Idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{
            AppSuspensionRule, CpuAffinityMode, CpuAffinityRule, CpuLimiterRule,
            EcoQosExclusionRule, EcoQosSettings, ForegroundResponsivenessSettings,
            NetworkThresholdUnit, PriorityRule, WatchdogRule,
        },
        rules::{ConflictGroup, PriorityResolver},
    };

    #[test]
    fn disabled_foreground_responsiveness_emits_no_rules() {
        let settings = ForegroundResponsivenessSettings::default();

        assert!(
            foreground_responsiveness_rules(&settings, Some("app.exe"), Some(90.0), false)
                .is_empty()
        );
    }

    #[test]
    fn cpu_affinity_rules_emit_custom_mask_actions() {
        let settings = CpuAffinitySettings {
            enabled: true,
            exclude_foreground_app: true,
            rules: vec![CpuAffinityRule {
                enabled: true,
                mode: CpuAffinityMode::Hard,
                process_name: "Worker.EXE".to_owned(),
                core_mask: 0b1010,
            }],
        };

        let rules = cpu_affinity_rules(&settings);

        assert_eq!(rules.len(), 1);
        assert!(matches!(
            &rules[0].actions[0],
            Action::SetAppAffinity {
                app: AppMatcher::ProcessName(name),
                affinity: AffinityPolicy::CustomMask(0b1010),
            } if name == "worker.exe"
        ));
    }

    #[test]
    fn cpu_affinity_rules_preserve_soft_cpu_set_mode() {
        let settings = CpuAffinitySettings {
            enabled: true,
            exclude_foreground_app: true,
            rules: vec![CpuAffinityRule {
                enabled: true,
                mode: CpuAffinityMode::Soft,
                process_name: "Worker.EXE".to_owned(),
                core_mask: 0b1010,
            }],
        };

        let rules = cpu_affinity_rules(&settings);

        assert_eq!(rules.len(), 1);
        assert!(matches!(
            &rules[0].actions[0],
            Action::SetAppAffinity {
                app: AppMatcher::ProcessName(name),
                affinity: AffinityPolicy::CpuSetMask(0b1010),
            } if name == "worker.exe"
        ));
    }

    #[test]
    fn cpu_affinity_rules_preserve_efficiency_mode_off_without_mask() {
        let settings = CpuAffinitySettings {
            enabled: true,
            exclude_foreground_app: true,
            rules: vec![CpuAffinityRule {
                enabled: true,
                mode: CpuAffinityMode::EfficiencyOff,
                process_name: "Game.EXE".to_owned(),
                core_mask: 0,
            }],
        };

        let rules = cpu_affinity_rules(&settings);

        assert_eq!(rules.len(), 1);
        assert!(matches!(
            &rules[0].actions[0],
            Action::SetAppAffinity {
                app: AppMatcher::ProcessName(name),
                affinity: AffinityPolicy::DisableEfficiencyMode,
            } if name == "game.exe"
        ));
    }

    #[test]
    fn cpu_limiter_rules_emit_per_app_cpu_limit_actions() {
        let settings = CpuLimiterSettings {
            enabled: true,
            exclude_foreground_app: true,
            rules: vec![CpuLimiterRule {
                enabled: true,
                process_name: "Worker.EXE".to_owned(),
                threshold_percent: 80,
                sustain_seconds: 10,
                cooldown_seconds: 30,
                max_logical_processors: 25,
            }],
        };

        let rules = cpu_limiter_rules(&settings);

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].cooldown_secs, 30);
        assert!(matches!(
            &rules[0].trigger,
            Trigger::CpuLoadAbove {
                percent: 80,
                duration_secs: 10,
            }
        ));
        assert!(matches!(
            &rules[0].actions[0],
            Action::SetAppCpuLimit {
                app: AppMatcher::ProcessName(name),
                logical_processor_percent: 25,
            } if name == "worker.exe"
        ));
    }

    #[test]
    fn eco_qos_rules_emit_background_efficiency_policy() {
        let settings = EcoQosSettings {
            enabled: true,
            prefer_efficiency_cores: true,
            cpu_restriction_percent: 50,
            efficiency_whitelist: vec![EcoQosExclusionRule {
                enabled: true,
                process_name: "Game.EXE".to_owned(),
            }],
            ..Default::default()
        };

        let rules = eco_qos_rules(&settings);

        assert_eq!(rules.len(), 1);
        assert!(matches!(
            &rules[0].actions[0],
            Action::ConfigureBackgroundEfficiencyPolicy {
                exclusions,
                prefer_efficiency_cores: true,
                logical_processor_percent: Some(50),
            } if matches!(
                exclusions.first(),
                Some(AppMatcher::ProcessName(name)) if name == "game.exe"
            )
        ));
        assert_eq!(
            rules[0].actions[0].conflict_group(),
            ConflictGroup::BackgroundEfficiencyPolicy
        );
    }

    #[test]
    fn background_cpu_restriction_emits_wildcard_cpu_limit() {
        let settings = BackgroundCpuRestrictionSettings {
            enabled: true,
            percent: 45,
            ..Default::default()
        };

        let rules = background_cpu_restriction_rules(&settings);

        assert_eq!(rules.len(), 1);
        assert!(matches!(
            &rules[0].actions[0],
            Action::SetAppCpuLimit {
                app: AppMatcher::Pattern(pattern),
                logical_processor_percent: 45,
            } if pattern == "*"
        ));
    }

    #[test]
    fn app_suspension_rules_emit_suspend_actions() {
        let settings = AppSuspensionSettings {
            enabled: true,
            background_delay_seconds: 600,
            temporary_thaw_enabled: false,
            temporary_thaw_interval_seconds: 1800,
            temporary_thaw_duration_seconds: 30,
            network_wake_enabled: false,
            network_wake_duration_seconds: 30,
            audio_wake_enabled: false,
            audio_wake_duration_seconds: 30,
            suspendable_apps: vec![AppSuspensionRule {
                enabled: true,
                process_name: "Chat.EXE".to_owned(),
                network_wake_enabled: false,
                audio_wake_enabled: false,
                network_download_threshold_bytes: 0,
                network_download_threshold_unit: NetworkThresholdUnit::Bytes,
                network_upload_threshold_bytes: 0,
                network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
            }],
        };

        let rules = app_suspension_rules(&settings);

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].cooldown_secs, 600);
        assert!(matches!(
            &rules[0].trigger,
            Trigger::AppBackgroundIdle {
                app: AppMatcher::ProcessName(name),
                duration_secs: 600,
            } if name == "chat.exe"
        ));
        assert!(matches!(
            &rules[0].actions[0],
            Action::SuspendApp { app: AppMatcher::ProcessName(name) } if name == "chat.exe"
        ));
    }

    #[test]
    fn combined_app_resource_rules_include_migrated_feature_actions() {
        let settings = Settings {
            foreground_responsiveness: ForegroundResponsivenessSettings {
                enabled: true,
                boost_foreground_app: true,
                foreground_boost: ForegroundBoostPriority::AboveNormal,
                ..Default::default()
            },
            watchdog: WatchdogSettings {
                enabled: true,
                rules: vec![WatchdogRule {
                    enabled: true,
                    name: "Block".to_owned(),
                    process_name: "tool.exe".to_owned(),
                    action: WatchdogAction::TerminateOnLaunch,
                    launch_path: String::new(),
                    launch_args: Vec::new(),
                    restart_delay_seconds: 5,
                }],
            },
            cpu_affinity: CpuAffinitySettings {
                enabled: true,
                exclude_foreground_app: true,
                rules: vec![CpuAffinityRule {
                    enabled: true,
                    mode: CpuAffinityMode::Hard,
                    process_name: "worker.exe".to_owned(),
                    core_mask: 0b11,
                }],
            },
            ..Default::default()
        };

        let rules = active_app_resource_rules_for_settings(&settings, Some("game.exe"), Some(90.0));

        assert!(rules.iter().any(|rule| matches!(
            rule.actions.first(),
            Some(Action::BoostForegroundPriority { .. })
        )));
        assert!(rules
            .iter()
            .any(|rule| matches!(rule.actions.first(), Some(Action::TerminateApp { .. }))));
        assert!(rules
            .iter()
            .any(|rule| matches!(rule.actions.first(), Some(Action::SetAppAffinity { .. }))));
    }

    #[test]
    fn combined_app_resource_rules_resolve_per_app_priority_conflicts() {
        let settings = Settings {
            foreground_responsiveness: ForegroundResponsivenessSettings {
                enabled: true,
                lower_background_apps: true,
                boost_foreground_app: true,
                foreground_boost: ForegroundBoostPriority::AboveNormal,
                rules: vec![PriorityRule {
                    enabled: true,
                    process_name: "game.exe".to_owned(),
                    priority: ConfigPriority::BelowNormal,
                }],
                ..Default::default()
            },
            ..Default::default()
        };

        let rules = active_app_resource_rules_for_settings(&settings, Some("game.exe"), Some(90.0));
        let resolved = PriorityResolver.resolve(&rules);
        let app_priority_actions = resolved
            .iter()
            .filter(|action| matches!(action.conflict_group, ConflictGroup::AppPriority(_)))
            .count();

        assert_eq!(app_priority_actions, 1);
    }

    #[test]
    fn priority_rules_emit_per_app_priority_actions() {
        let mut settings = ForegroundResponsivenessSettings {
            enabled: true,
            lower_background_apps: true,
            boost_foreground_app: false,
            ..Default::default()
        };
        settings.rules.push(PriorityRule {
            enabled: true,
            process_name: "Worker.EXE".to_owned(),
            priority: ConfigPriority::BelowNormal,
        });

        let rules = foreground_responsiveness_rules(&settings, Some("app.exe"), None, false);

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].actions.len(), 1);
        assert!(matches!(
            &rules[0].actions[0],
            Action::SetAppPriority { app: AppMatcher::ProcessName(name), priority }
                if name == "worker.exe" && *priority == RuleProcessPriority::BelowNormal
        ));
    }

    #[test]
    fn global_efficiency_suppresses_auto_balance_efficiency_policy() {
        let settings = ForegroundResponsivenessSettings {
            enabled: true,
            lower_background_apps: true,
            boost_foreground_app: true,
            foreground_boost: ForegroundBoostPriority::AboveNormal,
            auto_balance_enabled: true,
            auto_balance_total_threshold_percent: 70,
            ..Default::default()
        };

        let rules = foreground_responsiveness_rules(&settings, Some("game.exe"), Some(80.0), true);

        assert_eq!(rules.len(), 1);
        assert!(!rules.iter().any(|rule| {
            rule.actions
                .iter()
                .any(|action| matches!(action, Action::ConfigureBackgroundEfficiencyPolicy { .. }))
        }));
        assert!(rules.iter().any(|rule| {
            rule.actions
                .iter()
                .any(|action| matches!(action, Action::BoostForegroundPriority { .. }))
        }));
        assert!(!rules.iter().any(|rule| {
            rule.actions
                .iter()
                .any(|action| matches!(action, Action::SetAppAffinity { .. }))
        }));
    }

    #[test]
    fn foreground_boost_emits_boost_action() {
        let settings = ForegroundResponsivenessSettings {
            enabled: true,
            lower_background_apps: false,
            boost_foreground_app: true,
            foreground_boost: ForegroundBoostPriority::AboveNormal,
            ..Default::default()
        };

        let rules = foreground_responsiveness_rules(&settings, Some("Game.EXE"), None, false);

        assert_eq!(rules.len(), 1);
        assert!(matches!(
            &rules[0].actions[0],
            Action::BoostForegroundPriority { app: AppMatcher::ProcessName(name), priority }
                if name == "game.exe" && *priority == RuleProcessPriority::AboveNormal
        ));
    }

    #[test]
    fn foreground_boost_emits_normal_priority_action() {
        let settings = ForegroundResponsivenessSettings {
            enabled: true,
            lower_background_apps: false,
            boost_foreground_app: true,
            foreground_boost: ForegroundBoostPriority::Normal,
            ..Default::default()
        };

        let rules = foreground_responsiveness_rules(&settings, Some("Game.EXE"), None, false);

        assert_eq!(rules.len(), 1);
        assert!(matches!(
            &rules[0].actions[0],
            Action::BoostForegroundPriority { app: AppMatcher::ProcessName(name), priority }
                if name == "game.exe" && *priority == RuleProcessPriority::Normal
        ));
    }

    #[test]
    fn foreground_boost_auto_adapts_to_foreground_cpu_pressure() {
        let settings = ForegroundResponsivenessSettings {
            enabled: true,
            lower_background_apps: false,
            boost_foreground_app: true,
            foreground_boost: ForegroundBoostPriority::Auto,
            ..Default::default()
        };

        let rules = foreground_responsiveness_rules(&settings, Some("Game.EXE"), Some(50.0), false);
        assert!(matches!(
            &rules[0].actions[0],
            Action::BoostForegroundPriority { priority, .. }
                if *priority == RuleProcessPriority::AboveNormal
        ));

        let rules = foreground_responsiveness_rules(&settings, Some("Game.EXE"), Some(85.0), false);
        assert!(matches!(
            &rules[0].actions[0],
            Action::BoostForegroundPriority { priority, .. }
                if *priority == RuleProcessPriority::Normal
        ));
    }

    #[test]
    fn auto_balance_emits_affinity_action_only_when_total_cpu_is_high() {
        let settings = ForegroundResponsivenessSettings {
            enabled: true,
            lower_background_apps: false,
            lower_background_auto_cpu_percent: false,
            boost_foreground_app: false,
            auto_balance_enabled: true,
            auto_balance_cpu_percent: 50,
            auto_balance_total_threshold_percent: 70,
            auto_balance_threshold_percent: 25,
            auto_balance_restore_threshold_percent: 5,
            ..Default::default()
        };

        assert!(
            foreground_responsiveness_rules(&settings, Some("app.exe"), Some(69.0), false)
                .is_empty()
        );
        let rules = foreground_responsiveness_rules(&settings, Some("app.exe"), Some(80.0), false);

        assert_eq!(rules.len(), 1);
        assert!(matches!(
            rules[0].actions.first(),
            Some(Action::SetAppAffinity {
                app: AppMatcher::Pattern(pattern),
                affinity: AffinityPolicy::LogicalProcessorPercent(58),
            }) if pattern == "*"
        ));
        assert!(matches!(
            rules[0].trigger,
            Trigger::ForegroundCpuPressure { .. }
        ));
        assert!(
            foreground_responsiveness_rules(&settings, Some("app.exe"), Some(85.0), false)
                .is_empty()
        );
    }

    #[test]
    fn auto_balance_affinity_share_tightens_with_foreground_pressure() {
        let settings = ForegroundResponsivenessSettings {
            lower_background_auto_cpu_percent: false,
            auto_balance_cpu_percent: 50,
            auto_balance_total_threshold_percent: 70,
            ..Default::default()
        };

        assert_eq!(auto_balance_effective_cpu_percent(&settings, None), 50);
        assert_eq!(
            auto_balance_effective_cpu_percent(&settings, Some(70.0)),
            75
        );
        assert_eq!(
            auto_balance_effective_cpu_percent(&settings, Some(77.5)),
            63
        );
        assert_eq!(
            auto_balance_effective_cpu_percent(&settings, Some(85.0)),
            50
        );
    }

    #[test]
    fn auto_balance_auto_affinity_share_uses_behavior_floor() {
        let settings = ForegroundResponsivenessSettings {
            lower_background_auto_cpu_percent: true,
            auto_balance_cpu_percent: 25,
            auto_balance_total_threshold_percent: 75,
            ..Default::default()
        };

        assert_eq!(auto_balance_minimum_cpu_percent(&settings), 75);
        assert_eq!(
            auto_balance_effective_cpu_percent(&settings, Some(75.0)),
            100
        );
        assert_eq!(
            auto_balance_effective_cpu_percent(&settings, Some(80.0)),
            88
        );
    }

    #[test]
    fn auto_balance_auto_mode_uses_soft_affinity_and_skips_noop_actions() {
        let settings = ForegroundResponsivenessSettings {
            enabled: true,
            lower_background_apps: false,
            lower_background_auto_cpu_percent: true,
            boost_foreground_app: false,
            auto_balance_enabled: true,
            auto_balance_affinity_mode: EcoQosCpuRestrictionMode::HardAffinity,
            auto_balance_total_threshold_percent: 75,
            ..Default::default()
        };

        assert_eq!(
            auto_balance_effective_restriction_mode(&settings),
            EcoQosCpuRestrictionMode::SoftCpuSets
        );
        assert!(
            foreground_responsiveness_rules(&settings, Some("app.exe"), Some(75.0), false)
                .is_empty()
        );

        assert!(
            foreground_responsiveness_rules(&settings, Some("app.exe"), Some(80.0), false)
                .is_empty()
        );
    }

    #[test]
    fn smart_efficiency_auto_mode_waits_for_foreground_pressure() {
        let settings = ForegroundResponsivenessSettings {
            enabled: true,
            lower_background_apps: true,
            lower_background_auto_cpu_percent: true,
            boost_foreground_app: false,
            auto_balance_total_threshold_percent: 75,
            ..Default::default()
        };

        assert!(
            foreground_responsiveness_rules(&settings, Some("app.exe"), None, false).is_empty()
        );
        assert!(
            foreground_responsiveness_rules(&settings, Some("app.exe"), Some(85.0), false)
                .is_empty()
        );
        assert_eq!(
            foreground_responsiveness_rules(&settings, Some("app.exe"), Some(75.0), false).len(),
            1
        );
    }

    #[test]
    fn foreground_boost_conflicts_with_direct_priority_for_same_process() {
        let settings = ForegroundResponsivenessSettings {
            enabled: true,
            lower_background_apps: true,
            boost_foreground_app: true,
            foreground_boost: ForegroundBoostPriority::AboveNormal,
            rules: vec![PriorityRule {
                enabled: true,
                process_name: "game.exe".to_owned(),
                priority: ConfigPriority::BelowNormal,
            }],
            ..Default::default()
        };

        let rules = foreground_responsiveness_rules(&settings, Some("game.exe"), Some(90.0), false);
        let resolved = PriorityResolver.resolve(&rules);

        assert!(resolved
            .iter()
            .any(|action| { matches!(action.conflict_group, ConflictGroup::AppPriority(_)) }));
    }

    #[test]
    fn disabled_watchdog_emits_no_rules() {
        let settings = WatchdogSettings::default();

        assert!(watchdog_rules(&settings).is_empty());
    }

    #[test]
    fn watchdog_terminate_rule_emits_lifecycle_action() {
        let settings = WatchdogSettings {
            enabled: true,
            rules: vec![WatchdogRule {
                enabled: true,
                name: "Block tool".to_owned(),
                process_name: "Tool.EXE".to_owned(),
                action: WatchdogAction::TerminateOnLaunch,
                launch_path: String::new(),
                launch_args: Vec::new(),
                restart_delay_seconds: 5,
            }],
        };

        let rules = watchdog_rules(&settings);

        assert_eq!(rules.len(), 1);
        assert!(matches!(
            &rules[0].trigger,
            Trigger::ProcessStarted { app: AppMatcher::ProcessName(name) } if name == "tool.exe"
        ));
        assert!(matches!(
            &rules[0].actions[0],
            Action::TerminateApp { app: AppMatcher::ProcessName(name) } if name == "tool.exe"
        ));
    }

    #[test]
    fn watchdog_restart_rule_emits_restart_action() {
        let settings = WatchdogSettings {
            enabled: true,
            rules: vec![WatchdogRule {
                enabled: true,
                name: "Keep tool running".to_owned(),
                process_name: "tool.exe".to_owned(),
                action: WatchdogAction::RestartIfExited,
                launch_path: "C:\\Tools\\tool.exe".to_owned(),
                launch_args: vec!["--minimized".to_owned()],
                restart_delay_seconds: 30,
            }],
        };

        let rules = watchdog_rules(&settings);

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].cooldown_secs, 30);
        assert!(matches!(
            &rules[0].trigger,
            Trigger::ProcessMissing {
                duration_secs: 30,
                ..
            }
        ));
        assert!(matches!(
            &rules[0].actions[0],
            Action::RestartApp { launch_path, args, .. }
                if launch_path == "C:\\Tools\\tool.exe" && args == &vec!["--minimized".to_owned()]
        ));
    }

    #[test]
    fn watchdog_lifecycle_actions_conflict_per_process() {
        let settings = WatchdogSettings {
            enabled: true,
            rules: vec![
                WatchdogRule {
                    enabled: true,
                    name: "Block".to_owned(),
                    process_name: "tool.exe".to_owned(),
                    action: WatchdogAction::TerminateOnLaunch,
                    launch_path: String::new(),
                    launch_args: Vec::new(),
                    restart_delay_seconds: 5,
                },
                WatchdogRule {
                    enabled: true,
                    name: "Restart".to_owned(),
                    process_name: "TOOL.EXE".to_owned(),
                    action: WatchdogAction::RestartIfExited,
                    launch_path: "C:\\Tools\\tool.exe".to_owned(),
                    launch_args: Vec::new(),
                    restart_delay_seconds: 5,
                },
            ],
        };

        let rules = watchdog_rules(&settings);
        let resolved = PriorityResolver.resolve(&rules);

        assert_eq!(resolved.len(), 1);
        assert!(matches!(
            resolved[0].conflict_group,
            ConflictGroup::AppLifecycle(_)
        ));
    }
}
