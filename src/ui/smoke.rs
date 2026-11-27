//! Opt-in native render check; compiled only with the render-smoke feature.
use super::*;
use crate::backend::power_source::is_plugged_in;
use iced::advanced::widget::{operation, Id, Operation};
use iced::{Rectangle, Vector};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

static REQUESTED: AtomicBool = AtomicBool::new(false);
const NAVIGATION_CPU_COMPACT: usize = 35;
const NAVIGATION_CPU_COMPACT_ZH_TW: usize = 36;
const NAVIGATION_POWER_COMPACT: usize = 37;
const NAVIGATION_POWER_COMPACT_ZH_TW: usize = 42;
const DASHBOARD_PREVIEW: usize = 13;
const PROCESS_LIST_COMMIT_WIDE: usize = 94;
const PROCESS_LIST_COMMIT_COMPACT: usize = 95;
const DASHBOARD_ENABLED_FEATURES_COMPACT: usize = 96;
const DASHBOARD_POWER_SOURCE_UNAVAILABLE: usize = 97;
const CONTRAST_DARK_APPEARANCE: usize = 98;
const CONTRAST_LIGHT_PROCESS_LIST: usize = 99;
const DASHBOARD_HISTORY_WIDE: usize = 100;
const DASHBOARD_HISTORY_COMPACT: usize = 101;
const RESOURCE_PREVIEW_WIDE: usize = 102;
const RESOURCE_PREVIEW_COMPACT: usize = 103;
const SHOW_SHORTCUT_INVALID: usize = 104;
const SHOW_SHORTCUT_READY: usize = 105;
const DOCK_FOCUS_WIDE: usize = 106;
const DOCK_FOCUS_COMPACT: usize = 107;
const MEMORY_TRIM_PARTIAL: usize = 108;
const MEMORY_TRIM_UNKNOWN: usize = 109;
const NAVIGATION_RESIZE_TALL: usize = 110;
const NAVIGATION_RESIZE_COMPACT: usize = 111;
const KEEP_AWAKE_READY: usize = 112;
const KEEP_AWAKE_ACTIVE: usize = 113;
const GPU_MEMORY_WIDE: usize = 114;
const GPU_MEMORY_COMPACT: usize = 115;
const CPU_LIMITER_AFFINITY_WIDE: usize = 116;
const CPU_LIMITER_AFFINITY_COMPACT: usize = 117;
const CPU_LIMITER_PROCESSORS_COMPACT: usize = 118;
const ADAPTIVE_NAME_PATTERN: usize = 119;
const RESOURCE_NAME_INPUT: usize = 120;
const APP_SUSPENSION_FOCUS_OFF: usize = 121;
const ADAPTIVE_RESTRAINT_RECOVERY: usize = 122;
const ADAPTIVE_SETUP_ENABLED: usize = 123;
const KEEP_AWAKE_MIXED: usize = 124;
const APP_SUSPENSION_SETUP_COMPACT: usize = 125;
const FEATURE_TAB_STATES_START: usize = 126;
const FEATURE_TAB_STATES: &[(Page, FeatureTab)] = &[
    (Page::PowerPlanControl, FeatureTab::Automation),
    (Page::PowerPlanControl, FeatureTab::Advanced),
    (Page::MemoryTrim, FeatureTab::Exclusions),
    (Page::AppSuspension, FeatureTab::CustomRules),
    (Page::CpuLimiter, FeatureTab::CustomRules),
    (Page::DynamicPriorityBoost, FeatureTab::CustomRules),
    (Page::EfficiencyMode, FeatureTab::CustomRules),
    (Page::GpuPriority, FeatureTab::CustomRules),
    (Page::IoPriority, FeatureTab::CustomRules),
    (Page::MemoryPriority, FeatureTab::CustomRules),
    (Page::ProcessPriority, FeatureTab::CustomRules),
    (Page::ResourceThresholds, FeatureTab::CustomRules),
    (Page::SleepPrevention, FeatureTab::CustomRules),
    (Page::ThreadPriority, FeatureTab::CustomRules),
];
const EXTRA_STATES: usize = FEATURE_TAB_STATES_START + FEATURE_TAB_STATES.len();
pub(super) struct Run {
    pages: Vec<Page>,
    remaining: VecDeque<usize>,
    index: usize,
    ticks: u8,
    navigation_settings: Option<SettingsEditor>,
}
impl Run {
    fn pages() -> Vec<Page> {
        let mut pages = Vec::new();
        for section in Page::sections() {
            if !pages.contains(&section.landing_page) {
                pages.push(section.landing_page);
            }
            for page in section.pages {
                if !pages.contains(page) {
                    pages.push(*page);
                }
            }
        }
        pages
    }

    pub(super) fn requested() -> Option<Self> {
        REQUESTED.load(Ordering::Relaxed).then(|| {
            let pages = Self::pages();
            let mut remaining = Self::scenarios(&pages);
            let index = remaining.pop_front().expect("smoke run has scenarios");
            Self {
                pages,
                remaining,
                index,
                ticks: 0,
                navigation_settings: None,
            }
        })
    }

    fn scenarios(pages: &[Page]) -> VecDeque<usize> {
        let extra_start = pages.len() * 2;
        if std::env::args().any(|arg| arg == "--dashboard-preview-only") {
            [extra_start + DASHBOARD_PREVIEW].into()
        } else if std::env::args().any(|arg| arg == "--dashboard-only") {
            [0, pages.len()]
                .into_iter()
                .chain(
                    [
                        PROCESS_LIST_COMMIT_COMPACT,
                        DASHBOARD_ENABLED_FEATURES_COMPACT,
                        DASHBOARD_POWER_SOURCE_UNAVAILABLE,
                        DASHBOARD_HISTORY_WIDE,
                        DASHBOARD_HISTORY_COMPACT,
                        79,
                        80,
                    ]
                    .map(|extra| extra_start + extra),
                )
                .collect()
        } else if std::env::args().any(|arg| arg == "--search-only") {
            [0, pages.len()]
                .into_iter()
                .chain([26, 27, 28, 43].map(|extra| extra_start + extra))
                .collect()
        } else if std::env::args().any(|arg| arg == "--navigation-only") {
            [0, pages.len()]
                .into_iter()
                .chain(
                    [
                        NAVIGATION_CPU_COMPACT,
                        NAVIGATION_CPU_COMPACT_ZH_TW,
                        NAVIGATION_POWER_COMPACT,
                        NAVIGATION_POWER_COMPACT_ZH_TW,
                        NAVIGATION_RESIZE_TALL,
                        NAVIGATION_RESIZE_COMPACT,
                    ]
                    .map(|extra| extra_start + extra),
                )
                .collect()
        } else if std::env::args().any(|arg| arg == "--side-panel-only") {
            (0..extra_start)
                .filter(|index| {
                    matches!(
                        pages[index % pages.len()],
                        Page::Dashboard | Page::PowerPlanControl
                    )
                })
                .chain(
                    [0, 1, 16, DOCK_FOCUS_WIDE, DOCK_FOCUS_COMPACT]
                        .map(|extra| extra_start + extra),
                )
                .collect()
        } else if std::env::args().any(|arg| arg == "--feature-tabs-only") {
            std::iter::once(0)
                .chain((0..extra_start).filter(|index| {
                    let page = pages[index % pages.len()];
                    !FeatureTab::for_page(page).is_empty()
                        || matches!(
                            page,
                            Page::AdvancedPowerPlanTuning
                                | Page::AdaptiveProcessorPower
                                | Page::ByActivity
                                | Page::ByFocus
                                | Page::ByRunningApp
                                | Page::ByCpuLoad
                                | Page::ByTime
                                | Page::CpuSetsSoft
                                | Page::ProcessorAffinityHard
                                | Page::InstanceBalancing
                                | Page::TimerResolution
                        )
                }))
                .chain(
                    [
                        11,
                        19,
                        20,
                        21,
                        51,
                        58,
                        60,
                        61,
                        62,
                        63,
                        64,
                        65,
                        71,
                        72,
                        MEMORY_TRIM_PARTIAL,
                        MEMORY_TRIM_UNKNOWN,
                        KEEP_AWAKE_READY,
                        KEEP_AWAKE_ACTIVE,
                        KEEP_AWAKE_MIXED,
                        APP_SUSPENSION_FOCUS_OFF,
                        APP_SUSPENSION_SETUP_COMPACT,
                        CPU_LIMITER_AFFINITY_COMPACT,
                    ]
                    .into_iter()
                    .chain(FEATURE_TAB_STATES_START..EXTRA_STATES)
                    .map(|extra| extra_start + extra),
                )
                .collect()
        } else if std::env::args().any(|arg| arg == "--priority-control-only") {
            // Start on Home so the initial frame is captured before navigating.
            std::iter::once(0)
                .chain((0..extra_start).filter(|index| {
                    let page = pages[index % pages.len()];
                    page == Page::EfficiencyMode || priority_kind(page).is_some()
                }))
                .chain([extra_start, extra_start + 18])
                .collect()
        } else if std::env::args().any(|arg| arg == "--adaptive-restraint-only") {
            std::iter::once(0)
                .chain(
                    (0..extra_start)
                        .filter(|index| pages[index % pages.len()] == Page::AdaptiveEngine),
                )
                .chain(
                    [
                        4,
                        6,
                        7,
                        9,
                        10,
                        92,
                        93,
                        ADAPTIVE_NAME_PATTERN,
                        ADAPTIVE_RESTRAINT_RECOVERY,
                        ADAPTIVE_SETUP_ENABLED,
                    ]
                    .map(|extra| extra_start + extra),
                )
                .collect()
        } else if std::env::args().any(|arg| arg == "--resource-thresholds-only") {
            [
                51,
                52,
                53,
                RESOURCE_NAME_INPUT,
                RESOURCE_PREVIEW_WIDE,
                RESOURCE_PREVIEW_COMPACT,
            ]
            .into_iter()
            .map(|extra| extra_start + extra)
            .collect()
        } else if std::env::args().any(|arg| arg == "--memory-trim-only") {
            [11, MEMORY_TRIM_PARTIAL, MEMORY_TRIM_UNKNOWN]
                .into_iter()
                .map(|extra| extra_start + extra)
                .collect()
        } else if std::env::args().any(|arg| arg == "--watchdog-only") {
            [11, 21, 51, 52, 53]
                .into_iter()
                .chain(58..=65)
                .chain([
                    72,
                    RESOURCE_PREVIEW_WIDE,
                    RESOURCE_PREVIEW_COMPACT,
                    MEMORY_TRIM_PARTIAL,
                    MEMORY_TRIM_UNKNOWN,
                    KEEP_AWAKE_READY,
                    KEEP_AWAKE_ACTIVE,
                    KEEP_AWAKE_MIXED,
                    RESOURCE_NAME_INPUT,
                    APP_SUSPENSION_FOCUS_OFF,
                    APP_SUSPENSION_SETUP_COMPACT,
                ])
                .map(|extra| extra_start + extra)
                .collect()
        } else {
            (0..extra_start + EXTRA_STATES).collect()
        }
    }
}
pub(super) fn advance(app: &mut WinderustApp) -> Option<Task<Message>> {
    let window = app.window?;
    let run = app.smoke.as_mut()?;
    let extra = run.index.checked_sub(run.pages.len() * 2);
    if extra == Some(EXTRA_STATES) {
        return Some(app.shutdown());
    }
    if extra == Some(NAVIGATION_RESIZE_COMPACT) && run.ticks == 0 {
        // Exercise the native resize without navigating or changing keyboard focus.
        run.ticks = 1;
        return Some(iced::window::resize(window, iced::Size::new(900.0, 620.0)));
    }
    let compact_navigation = matches!(
        extra,
        Some(
            NAVIGATION_CPU_COMPACT
                | NAVIGATION_CPU_COMPACT_ZH_TW
                | NAVIGATION_POWER_COMPACT
                | NAVIGATION_POWER_COMPACT_ZH_TW
        )
    );
    if run.ticks == 0 {
        let chinese = extra.map_or(run.index >= run.pages.len(), |index| {
            index >= FEATURE_TAB_STATES_START && index % 2 == 1
                || matches!(
                    index,
                    1 | 5
                        | 7
                        | 9
                        | 11
                        | NAVIGATION_CPU_COMPACT_ZH_TW
                        | 40
                        | NAVIGATION_POWER_COMPACT_ZH_TW
                        | 43
                        | 45
                        | 46
                        | 49
                        | 52
                        | 55
                        | 56
                        | 59
                        | 61
                        | 63
                        | 65
                        | 67
                        | 69
                        | 71
                        | 72
                        | 74
                        | 76
                        | 78
                        | 80
                        | 82
                        | 83
                        | 85
                        | 86
                        | 88
                        | 90
                        | 91
                        | 93
                        | PROCESS_LIST_COMMIT_COMPACT
                        | DASHBOARD_ENABLED_FEATURES_COMPACT
                        | CONTRAST_LIGHT_PROCESS_LIST
                        | DASHBOARD_HISTORY_COMPACT
                        | RESOURCE_PREVIEW_COMPACT
                        | SHOW_SHORTCUT_READY
                        | DOCK_FOCUS_COMPACT
                        | MEMORY_TRIM_UNKNOWN
                        | KEEP_AWAKE_ACTIVE
                        | KEEP_AWAKE_MIXED
                        | APP_SUSPENSION_FOCUS_OFF
                        | APP_SUSPENSION_SETUP_COMPACT
                        | ADAPTIVE_RESTRAINT_RECOVERY
                        | ADAPTIVE_SETUP_ENABLED
                        | GPU_MEMORY_COMPACT
                        | CPU_LIMITER_AFFINITY_COMPACT
                        | CPU_LIMITER_PROCESSORS_COMPACT
                )
        });
        let page = match extra {
            Some(0) => Page::EfficiencyMode,
            Some(
                1
                | 89..=91
                | CPU_LIMITER_AFFINITY_WIDE
                | CPU_LIMITER_AFFINITY_COMPACT
                | CPU_LIMITER_PROCESSORS_COMPACT,
            ) => Page::CpuLimiter,
            Some(2) => Page::SettingsHome,
            Some(
                3
                | DASHBOARD_PREVIEW
                | 14
                | 26..=34
                | 38
                | 39
                | 43
                | 48
                | 79..=80
                | PROCESS_LIST_COMMIT_WIDE
                | PROCESS_LIST_COMMIT_COMPACT
                | DASHBOARD_ENABLED_FEATURES_COMPACT
                | DASHBOARD_POWER_SOURCE_UNAVAILABLE
                | CONTRAST_LIGHT_PROCESS_LIST
                | DASHBOARD_HISTORY_WIDE
                | DASHBOARD_HISTORY_COMPACT
                | GPU_MEMORY_WIDE
                | GPU_MEMORY_COMPACT,
            ) => Page::Dashboard,
            Some(
                4
                | 6..=10
                | ADAPTIVE_NAME_PATTERN
                | ADAPTIVE_RESTRAINT_RECOVERY
                | ADAPTIVE_SETUP_ENABLED,
            ) => Page::AdaptiveEngine,
            Some(5 | 70 | 71) => Page::AdaptiveProcessorPower,
            Some(11 | MEMORY_TRIM_PARTIAL | MEMORY_TRIM_UNKNOWN) => Page::MemoryTrim,
            Some(NAVIGATION_RESIZE_TALL) => Page::AdvancedControls,
            Some(KEEP_AWAKE_READY | KEEP_AWAKE_ACTIVE | KEEP_AWAKE_MIXED) => Page::SleepPrevention,
            Some(12 | 82) => Page::ByFocus,
            Some(15) => Page::PowerPlanControl,
            Some(16) => Page::ActionLog,
            Some(17 | 24 | 84..=85 | 92..=93) => Page::AdaptiveEngine,
            Some(18) => Page::EfficiencyMode,
            Some(19) => Page::ByActivity,
            Some(20) => Page::AdvancedPowerPlanTuning,
            Some(21 | 72 | APP_SUSPENSION_FOCUS_OFF | APP_SUSPENSION_SETUP_COMPACT) => {
                Page::AppSuspension
            }
            Some(22) => Page::LanguageAndAppearance,
            Some(23) => Page::About,
            Some(25 | 87..=88) => Page::ProcessPriority,
            Some(NAVIGATION_CPU_COMPACT | NAVIGATION_CPU_COMPACT_ZH_TW) => Page::AdaptiveEngine,
            Some(NAVIGATION_POWER_COMPACT | NAVIGATION_POWER_COMPACT_ZH_TW) => Page::ByFocus,
            Some(40) => Page::LanguageAndAppearance,
            Some(41) => Page::AdaptiveEngine,
            Some(44 | 73 | 76) => Page::CpuSetsSoft,
            Some(45 | 74 | 75) => Page::ProcessorAffinityHard,
            Some(46) => Page::AdvancedPowerPlanTuning,
            Some(47) => Page::AdaptiveEngine,
            Some(CONTRAST_DARK_APPEARANCE) => Page::LanguageAndAppearance,
            Some(49 | 86) => Page::ActionLog,
            Some(50 | 77..=78 | SHOW_SHORTCUT_INVALID | SHOW_SHORTCUT_READY) => {
                Page::WinderustBehaviour
            }
            Some(
                51..=53 | RESOURCE_NAME_INPUT | RESOURCE_PREVIEW_WIDE | RESOURCE_PREVIEW_COMPACT,
            ) => Page::ResourceThresholds,
            Some(54..=57 | 81 | DOCK_FOCUS_WIDE | DOCK_FOCUS_COMPACT) => Page::ByCpuLoad,
            Some(58..=59) => Page::SleepPrevention,
            Some(60..=61) => Page::KeepRunning,
            Some(62..=65) => Page::InstanceLimits,
            Some(66..=69) => Page::InstanceBalancing,
            Some(83) => Page::ByTime,
            Some(index @ FEATURE_TAB_STATES_START..) => {
                FEATURE_TAB_STATES[index - FEATURE_TAB_STATES_START].0
            }
            _ => run.pages[run.index % run.pages.len()],
        };
        run.ticks = 1;
        app.settings.general.language = if chinese {
            crate::config::AppLanguage::ZhTw
        } else {
            crate::config::AppLanguage::English
        };
        app.settings.general.theme_mode =
            if chinese || extra.is_some_and(|i| (DASHBOARD_PREVIEW..=34).contains(&i)) {
                crate::config::AppThemeMode::Dark
            } else {
                crate::config::AppThemeMode::Light
            };
        app.settings.general.accent.source = crate::config::AccentColorSource::Custom;
        app.settings.general.accent.custom_color = match extra {
            Some(0..=12) => 0x9b59d0,
            Some(13..=25) => 0x16895f,
            Some(_) => 0xd65a65,
            None if chinese => 0x9b59d0,
            None => 0x246bfe,
        };
        rust_i18n::set_locale(app.settings.general.language.locale());
        app.appearance = settings_pages::theme(&app.settings.general);
        if compact_navigation || extra == Some(NAVIGATION_RESIZE_TALL) {
            app.settings
                .edit_global(|settings| settings.advanced.show_advanced_controls = true);
        }
        if matches!(
            extra,
            Some(KEEP_AWAKE_READY | KEEP_AWAKE_ACTIVE | KEEP_AWAKE_MIXED)
        ) {
            use crate::sleep_prevention::TemporarySleepPrevention;

            let status = &mut Arc::make_mut(&mut app.status.feature_status).sleep_prevention;
            let now = std::time::Instant::now();
            let active = extra == Some(KEEP_AWAKE_ACTIVE);
            status.temporary.set(
                TemporarySleepPrevention::KeepAwake,
                (active || extra == Some(KEEP_AWAKE_MIXED))
                    .then(|| now + Duration::from_secs(15 * 60)),
            );
            status.temporary.set(
                TemporarySleepPrevention::KeepDisplayOn,
                active.then(|| now + Duration::from_secs(37 * 60)),
            );
        }
        if matches!(
            extra,
            Some(CONTRAST_DARK_APPEARANCE | CONTRAST_LIGHT_PROCESS_LIST)
        ) {
            use crate::platform::windows::appearance::ContrastColors;
            app.settings.general.theme_mode = crate::config::AppThemeMode::System;
            let colors = if extra == Some(CONTRAST_DARK_APPEARANCE) {
                ContrastColors {
                    window: 0x000000,
                    window_text: 0xffffff,
                    highlight: 0xffff00,
                    highlight_text: 0x000000,
                }
            } else {
                ContrastColors {
                    window: 0xffffff,
                    window_text: 0x000000,
                    highlight: 0x0055aa,
                    highlight_text: 0xffffff,
                }
            };
            app.appearance =
                settings_pages::theme_from_appearance(&app.settings.general, None, Some(colors));
        }
        if extra.is_some_and(|i| (38..=53).contains(&i)) {
            let _ = app.update(Message::Preferences(settings_pages::Message::Theme(
                crate::config::AppThemeMode::Amoled,
            )));
            assert_eq!(app.appearance.palette().background, iced::Color::BLACK);
        }
        if extra.is_none() && page == Page::ResourceThresholds {
            app.resource_thresholds = Default::default();
            let mut expected = app.settings.global().resource_thresholds.clone();
            expected.rules[1].notify = false;
            let _ = app.update(Message::ResourceThresholds(
                resource_thresholds::Message::RuleNotify(1, false),
            ));
            assert_eq!(app.settings.global().resource_thresholds, expected);
        }
        // Exercise the rule table and editor without enabling automation.
        if matches!(extra, Some(62..=65)) {
            let paths = [
                r"C:\Apps\Long Application Name\renderer.exe",
                r"C:\Apps\editor.exe",
                r"C:\Apps\worker.exe",
            ];
            app.settings.edit_global(|settings| {
                settings.instance_limits.enabled = true;
                settings.instance_limits.rules = paths
                    .into_iter()
                    .zip([2, 4, 8])
                    .map(|(path, max_instances)| crate::config::InstanceLimitsRule {
                        max_instances,
                        ..crate::config::InstanceLimitsRule::new(path.into())
                    })
                    .collect();
            });
            let mut expected = app.settings.global().instance_limits.clone();
            expected.rules[1].notify = false;
            let _ = app.update(Message::InstanceLimits(
                instance_limits::Message::RuleNotify(1, false),
            ));
            assert_eq!(app.settings.global().instance_limits, expected);
            if matches!(extra, Some(64..=65)) {
                let _ = app.update(Message::InstanceLimits(instance_limits::Message::Edit(0)));
            }
        }
        if matches!(extra, Some(60..=61)) {
            use crate::features::watchdog::keep_running::{
                KeepRunningRuleSnapshot, KeepRunningState, PauseReason,
            };
            let paths = [
                r"C:\Apps\Long Application Name\renderer.exe",
                r"C:\Apps\editor.exe",
            ];
            app.settings.edit_global(|settings| {
                settings.keep_running.enabled = true;
                settings.keep_running.rules = paths
                    .iter()
                    .map(|path| crate::config::KeepRunningRule::new((*path).into()))
                    .collect();
            });
            let status = &mut Arc::make_mut(&mut app.status.feature_status).keep_running;
            status.enabled = true;
            status.observation_known = true;
            status.rules = app
                .settings
                .global()
                .keep_running
                .rules
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, rule)| KeepRunningRuleSnapshot {
                    rule: Arc::new(rule),
                    state: if index == 0 {
                        KeepRunningState::Armed
                    } else {
                        KeepRunningState::Paused(PauseReason::RetryBudgetExhausted)
                    },
                    attempts: if index == 0 { 0 } else { 3 },
                    next_retry_seconds: None,
                    last_error: None,
                })
                .collect();
            let mut expected = app.settings.global().keep_running.clone();
            expected.rules[1].notify = true;
            let _ = app.update(Message::KeepRunning(keep_running::Message::RuleNotify(
                1, true,
            )));
            assert_eq!(app.settings.global().keep_running, expected);
            let status = &app.status.feature_status.keep_running;
            assert!(status.rule_status(&expected.rules[1]).is_some());
            if extra == Some(61) {
                let _ = app.update(Message::KeepRunning(keep_running::Message::Edit(0)));
            }
        }
        if matches!(extra, Some(58..=59)) {
            let _ = app.update(Message::PowerRules(
                power_rules::Kind::CpuLoad,
                power_rules::Message::Cancel,
            ));
            app.settings.sleep_prevention.rules = vec![
                crate::config::SleepPreventionRule {
                    executable_path: r"C:\Apps\Long Application Name\video-renderer.exe".into(),
                    enabled: true,
                    keep_display_on: false,
                },
                crate::config::SleepPreventionRule {
                    executable_path: r"C:\Apps\presentation.exe".into(),
                    enabled: true,
                    keep_display_on: true,
                },
            ];
        }
        if matches!(extra, Some(51..=53)) {
            app.settings.edit_global(|settings| {
                let rule = &mut settings.resource_thresholds.rules[0];
                rule.memory.enabled = true;
                rule.memory.warning.threshold = 50;
                rule.memory.critical.threshold = 75;
                if extra == Some(53) {
                    rule.memory_unit = crate::config::ResourceThresholdsMemoryUnit::Megabytes;
                    rule.memory.warning.threshold = 512;
                    rule.memory.critical.threshold = 1024;
                }
                rule.memory.critical.action = crate::config::ResourceThresholdsAction::TrimMemory;
            });
            app.resource_thresholds = resource_thresholds::Editor::default();
            let _ = app.update(Message::ResourceThresholds(
                resource_thresholds::Message::Edit(0),
            ));
        }
        if matches!(extra, Some(54..=57)) {
            use crate::config::{
                ByCpuLoadRule, ByCpuLoadStability, CpuLoadPercent, CpuUsageComparison,
            };
            let _ = app.update(Message::ResourceThresholds(
                resource_thresholds::Message::CancelRule,
            ));
            let _ = app.update(Message::PowerRules(
                power_rules::Kind::CpuLoad,
                power_rules::Message::Cancel,
            ));
            let (comparison, threshold, upper, release, release_upper) = match extra {
                Some(54) => (CpuUsageComparison::AtOrAbove, 75, None, 65, None),
                Some(57) => (CpuUsageComparison::AtOrBelow, 25, None, 35, None),
                _ => (CpuUsageComparison::Between, 40, Some(60), 30, Some(70)),
            };
            let percent = |value| CpuLoadPercent::new(value).expect("valid smoke CPU percentage");
            let power_plan_guid = app.power_plans.first().map(|plan| plan.guid.clone());
            app.settings.edit_global(|settings| {
                settings.general.enabled = false;
                settings.by_cpu_load.rules = vec![ByCpuLoadRule {
                    name: "CPU load stability".into(),
                    comparison,
                    threshold_percent: percent(threshold),
                    upper_threshold_percent: upper.map(percent),
                    duration_seconds: 5,
                    stability: Some(ByCpuLoadStability {
                        exit_threshold_percent: percent(release),
                        exit_upper_threshold_percent: release_upper.map(percent),
                        minimum_hold_seconds: 30,
                    }),
                    power_plan_guid,
                    ..ByCpuLoadRule::default()
                }];
            });
            if extra == Some(54) {
                let before = app.settings.by_time.clone();
                let _ = app.update(Message::Page(Page::ByTime));
                let _ = app.update(Message::PowerRules(
                    power_rules::Kind::Time,
                    power_rules::Message::Edit(0),
                ));
                let _ = app.update(Message::PowerRules(
                    power_rules::Kind::Time,
                    power_rules::Message::Start(0, String::new()),
                ));
                let _ = app.search_keyboard(
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    iced::keyboard::Modifiers::empty(),
                    false,
                    iced::event::Status::Ignored,
                );
                assert!(!app.pending_editor());
                assert_eq!(app.settings.by_time, before);
            }
        }
        if extra == Some(72) {
            let _ = app.update(Message::AdaptiveProcessorPower(
                adaptive_processor_power::Message::CancelPreset,
            ));
        }
        if extra == Some(77) {
            for kind in [cpu_allocation::Kind::Soft, cpu_allocation::Kind::Hard] {
                let _ = app.update(Message::Allocation(
                    kind,
                    cpu_allocation::Message::ClosePreset,
                ));
            }
        }
        if matches!(extra, Some(GPU_MEMORY_WIDE | GPU_MEMORY_COMPACT)) {
            use process_list::{Message as ProcessMessage, Sort};

            for column in [
                Sort::GpuUsage,
                Sort::DedicatedGpuMemory,
                Sort::SharedGpuMemory,
            ] {
                let _ = app.update(Message::Processes(ProcessMessage::Column(column, true)));
            }
            let _ = app.update(Message::Processes(ProcessMessage::Group(
                extra == Some(GPU_MEMORY_COMPACT),
            )));
            let _ = app.update(Message::Processes(ProcessMessage::Sort(Sort::GpuUsage)));
        }
        let was_open = app.side_panel_open;
        let toggle_side_panel = || Message::WindowAction(window_frame::Event::ToggleSidePanel);
        if page == Page::Dashboard {
            if app.processes.context_open() || app.processes.modal_open() {
                let _ = app.update(Message::Processes(process_list::Message::CloseSelection));
            }
            if page == app.page {
                let _ = app.update(Message::SidePanelTab(SidePanelTab::Settings));
            }
        }
        if extra == Some(DASHBOARD_PREVIEW) {
            app.processes.populate_smoke_rows();
        }
        let navigate = app.update(Message::Page(page));
        let feature_tab = match extra {
            Some(index @ FEATURE_TAB_STATES_START..) => {
                FEATURE_TAB_STATES[index - FEATURE_TAB_STATES_START].1
            }
            Some(
                51..=53
                | 58..=59
                | 62..=65
                | 72
                | 89..=91
                | RESOURCE_NAME_INPUT
                | RESOURCE_PREVIEW_WIDE
                | RESOURCE_PREVIEW_COMPACT
                | CPU_LIMITER_AFFINITY_WIDE
                | CPU_LIMITER_AFFINITY_COMPACT
                | CPU_LIMITER_PROCESSORS_COMPACT,
            ) => FeatureTab::CustomRules,
            _ => FeatureTab::Setup,
        };
        let before = app.settings.runtime_settings_snapshot().value;
        let switch_tab = app.update(Message::FeatureTab(feature_tab));
        assert_eq!(app.feature_tab(), feature_tab);
        assert_eq!(app.settings.runtime_settings_snapshot().value, before);
        assert_eq!(app.side_panel_open, was_open);
        if matches!(extra, Some(66..=69)) {
            use crate::action_log::ActionLogResult;
            use crate::automation::InstanceBalancingDetail;

            app.reset_editors();
            app.settings.edit_global(|settings| {
                settings.general.enabled = true;
                settings.instance_balancing.enabled = true;
                settings.instance_balancing.rules = [
                    r"C:\Apps\Long Application Name\renderer.exe",
                    r"C:\Other\renderer.exe",
                ]
                .into_iter()
                .map(|path| crate::config::InstanceBalancingRule {
                    enabled: true,
                    executable_path: path.into(),
                })
                .collect();
            });
            // Only the UI's saved-state projection changes; the smoke runtime stays disabled.
            app.settings = SettingsEditor::with_settings(app.settings.global().clone());
            app.status.generation = app.status.generation.max(1);
            let status = &mut Arc::make_mut(&mut app.status.feature_status).instance_balancing;
            *status = Default::default();
            status.enabled = true;
            status.scanned_processes = 4;
            let (result, detail) = if matches!(extra, Some(66..=67)) {
                status.adjusted_processes = 4;
                (
                    ActionLogResult::Applied,
                    InstanceBalancingDetail::Assigned(4),
                )
            } else {
                (
                    ActionLogResult::Skipped,
                    InstanceBalancingDetail::WaitingForStableCohort,
                )
            };
            status.detail = Some(detail.clone());
            let rules = &app.settings.global().instance_balancing.rules;
            status.record_rule(Arc::new(rules[0].clone()), result, detail);
            status.record_rule(
                Arc::new(rules[1].clone()),
                ActionLogResult::Skipped,
                InstanceBalancingDetail::WaitingForInstances,
            );
            if matches!(extra, Some(68..=69)) {
                let before = app.settings.global().instance_balancing.clone();
                let _ = app.update(Message::InstanceBalancing(
                    instance_balancing::Message::Edit(0),
                ));
                let _ = app.update(Message::InstanceBalancing(
                    instance_balancing::Message::Path("relative.exe".into()),
                ));
                let _ = app.search_keyboard(
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    iced::keyboard::Modifiers::empty(),
                    false,
                    iced::event::Status::Ignored,
                );
                assert!(!app.pending_editor());
                assert_eq!(app.settings.global().instance_balancing, before);
                let _ = app.update(Message::InstanceBalancing(
                    instance_balancing::Message::Edit(0),
                ));
                let _ = app.update(Message::InstanceBalancing(
                    instance_balancing::Message::Path(if extra == Some(68) {
                        before.rules[1].executable_path.clone()
                    } else {
                        r"C:\Apps\Long Application Name\renderer-updated.exe".into()
                    }),
                ));
                assert!(app.pending_editor());
            }
        }
        if matches!(extra, Some(54..=57)) {
            let edit =
                || Message::PowerRules(power_rules::Kind::CpuLoad, power_rules::Message::Edit(0));
            if extra == Some(54) {
                let before = app.settings.by_cpu_load.clone();
                let _ = app.update(edit());
                let _ = app.update(Message::PowerRules(
                    power_rules::Kind::CpuLoad,
                    power_rules::Message::ExitThreshold(0, String::new()),
                ));
                let _ = app.search_keyboard(
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    iced::keyboard::Modifiers::empty(),
                    false,
                    iced::event::Status::Ignored,
                );
                assert!(!app.pending_editor());
                assert_eq!(app.settings.by_cpu_load, before);
            }
            let _ = app.update(edit());
            assert!(app.cpu_rules.has_pending_editor());
            assert!(!app.settings.general.enabled);
        }
        if matches!(extra, Some(51..=53)) {
            let before = app.settings.global().resource_thresholds.clone();
            assert!(app.pending_editor());
            let _ = app.update(Message::ResourceThresholds(
                resource_thresholds::Message::Threshold(
                    crate::config::ResourceThresholdsResource::Cpu,
                    false,
                    String::new(),
                ),
            ));
            let _ = app.search_keyboard(
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                iced::keyboard::Modifiers::empty(),
                false,
                iced::event::Status::Ignored,
            );
            assert!(!app.pending_editor());
            assert_eq!(app.settings.global().resource_thresholds, before);
            let _ = app.update(Message::ResourceThresholds(
                resource_thresholds::Message::Edit(0),
            ));
            if extra == Some(51) {
                let _ = app.update(Message::ResourceThresholds(
                    resource_thresholds::Message::CancelRule,
                ));
                let _ = app.update(Message::ResourceThresholds(
                    resource_thresholds::Message::Path(r"C:\Apps\worker.exe".into()),
                ));
                assert_eq!(app.settings.global().resource_thresholds, before);
            } else if extra == Some(52) {
                let _ = app.update(Message::ResourceThresholds(
                    resource_thresholds::Message::MonitorEnabled(
                        crate::config::ResourceThresholdsResource::Cpu,
                        false,
                    ),
                ));
            } else if extra == Some(53) {
                let _ = app.update(Message::ResourceThresholds(
                    resource_thresholds::Message::Tab(
                        crate::config::ResourceThresholdsResource::Memory,
                    ),
                ));
            }
        }
        if extra == Some(RESOURCE_NAME_INPUT) {
            app.resource_thresholds = resource_thresholds::Editor::default();
            let _ = app.update(Message::ResourceThresholds(
                resource_thresholds::Message::MatchBy(
                    super::super::process_rules::MatchBy::ExecutableName,
                ),
            ));
            let _ = app.update(Message::ResourceThresholds(
                resource_thresholds::Message::Name("abc*def.exe".into()),
            ));
        }
        assert_eq!(app.side_panel_tab, SidePanelTab::initial_for_page(page));
        assert_eq!(app.side_panel_open, was_open);
        let _ = app.update(Message::CloseSidePanel);
        if !SidePanelTab::for_page(page).is_empty() {
            assert!(app.side_panel(is_plugged_in).is_some());
        } else if page == Page::Watchdog || navigation::children(Page::Watchdog).contains(&page) {
            assert!(app.side_panel(is_plugged_in).is_none());
        }
        if extra.is_none()
            && matches!(
                page,
                Page::IoPriority
                    | Page::ThreadPriority
                    | Page::GpuPriority
                    | Page::MemoryPriority
                    | Page::TimerResolution
            )
        {
            app.side_panel_open = true;
            assert_eq!(app.side_panel_tab, SidePanelTab::Status);
        }
        if matches!(extra, Some(5 | 70 | 71)) {
            assert!(SidePanelTab::for_page(page).contains(&SidePanelTab::Presets));
            app.side_panel_open = true;
            let _ = app.update(Message::SidePanelTab(SidePanelTab::Presets));
            assert!(app.side_panel_open);
            assert_eq!(app.side_panel_tab, SidePanelTab::Presets);
            assert!(app.side_panel(is_plugged_in).is_some());
        }
        if extra == Some(0) {
            let _ = app.update(Message::NavigationSearch("cpu".into()));
            assert!(app.navigation_search_open);
            let _ = app.update(Message::CloseNavigationSearch);
            assert!(!app.navigation_search_open);
            assert!(app.navigation_search.is_empty());
            let _ = app.update(Message::OpenNavigationSearch);
            let _ = app.update(Message::Page(page));
            assert!(!app.navigation_search_open);
            assert!(app.navigation_search.is_empty());
        }
        if page == Page::EfficiencyMode && (extra.is_none() || matches!(extra, Some(0 | 18))) {
            app.efficiency_mode = Default::default();
        }
        if let Some(index @ 4..=11) = extra {
            if index <= 10 {
                use adaptive_engine::{Message as AdaptiveMessage, TuningGroup, TuningTab};
                if index == 4 {
                    app.settings
                        .adaptive_engine_process
                        .background_app_cpu_threshold_percent = 8;
                    app.settings
                        .adaptive_engine_process
                        .cpu_recovery_threshold_percent = 10;
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(
                        TuningTab::Setup,
                    )));
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::Collapse(
                        TuningGroup::CpuTriggers,
                    )));
                } else if index == 5 {
                    use adaptive_processor_power::{BuiltInPreset, Message as PowerMessage, Tab};

                    let _ = app.update(Message::PowerSource(PowerSourceProfile::OnBattery));
                    assert_eq!(app.power_source, PowerSourceProfile::OnBattery);
                    let ac = app.settings.global().adaptive_engine.clone();
                    let before = app.settings.runtime_settings_snapshot().value;
                    let _ =
                        app.update(Message::AdaptiveProcessorPower(PowerMessage::Enabled(true)));
                    let _ = app.update(Message::AdaptiveProcessorPower(
                        PowerMessage::PreviewBuiltIn(BuiltInPreset::Balanced),
                    ));
                    let _ = app.update(Message::AdaptiveProcessorPower(PowerMessage::UsePreset));
                    assert_ne!(
                        app.settings.adaptive_engine.base_processor_policy,
                        before
                            .battery_profile()
                            .adaptive_engine
                            .base_processor_policy
                    );
                    assert!(
                        app.settings
                            .adaptive_engine
                            .adaptive_processor_power_enabled
                    );
                    assert_eq!(app.settings.global().adaptive_engine, ac);
                    assert!(!app.settings.general.enabled);
                    assert_eq!(&app.settings.runtime_settings_snapshot().value, &before);
                    assert!(app.pending_settings_changes());
                    let _ = app.update(Message::AdaptiveProcessorPower(PowerMessage::Tab(
                        Tab::DemandProfiles,
                    )));
                } else if index == 8 {
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::New));
                } else if index == 6 || index == 9 {
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::Cancel));
                    let source = if index == 6 {
                        PowerSourceProfile::PluggedIn
                    } else {
                        PowerSourceProfile::OnBattery
                    };
                    let _ = app.update(Message::PowerSource(source));
                    app.settings.adaptive_engine_process.cpu_allocation_method =
                        crate::config::CpuAllocationMethod::ReduceAffinity;
                    app.settings
                        .adaptive_engine_process
                        .limit_background_processors_enabled = true;
                    app.settings
                        .adaptive_engine_process
                        .limit_background_processors_during_restraint = index == 9;
                    app.settings
                        .adaptive_engine_process
                        .cpu_pressure_restraint_enabled = false;
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::Toggle(
                        |settings, enabled| {
                            settings.adaptive_engine_process.restore_child_affinity = enabled;
                        },
                        index == 9,
                    )));
                    assert_eq!(
                        app.settings.adaptive_engine_process.restore_child_affinity,
                        index == 9
                    );
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(
                        TuningTab::Setup,
                    )));
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::Collapse(
                        TuningGroup::BackgroundProcessors,
                    )));
                } else if index == 10 {
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::Cancel));
                    let open = || {
                        Message::Adaptive(AdaptiveMessage::ViewBuiltIn(
                            adaptive_engine::BuiltInAdaptiveEnginePreset::Balanced,
                        ))
                    };
                    let _ = app.update(open());
                    let _ = app.search_keyboard(
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                        iced::keyboard::Modifiers::empty(),
                        false,
                        iced::event::Status::Ignored,
                    );
                    assert!(!app.adaptive.preset_open());
                    let _ = app.update(open());
                }
            } else {
                app.settings.edit_global(|settings| {
                    settings
                        .memory_trim
                        .exclusions
                        .push(crate::config::ProcessExclusionRule {
                            executable_path: r"C:\Apps\Example\example.exe".into(),
                            enabled: false,
                            ..Default::default()
                        });
                });
            }
        }
        if matches!(extra, Some(7 | ADAPTIVE_NAME_PATTERN)) {
            use adaptive_engine::{ExclusionMatchBy, Message as AdaptiveMessage, TuningTab};

            let prefix = if extra == Some(7) { "worker" } else { "helper" };
            let initial = app.settings.adaptive_engine_process.custom_rules.len();
            for message in [
                AdaptiveMessage::Cancel,
                AdaptiveMessage::TuningTab(TuningTab::Exclusions),
                AdaptiveMessage::MatchBy(ExclusionMatchBy::NamePattern),
                AdaptiveMessage::ExecutableNamePattern(format!("{prefix}-*.exe")),
                AdaptiveMessage::AddExclusion,
                AdaptiveMessage::ExecutableNamePattern(format!("{prefix}-?.exe")),
            ] {
                let _ = app.update(Message::Adaptive(message));
            }
            let rules = &app.settings.adaptive_engine_process.custom_rules;
            assert_eq!(rules.len(), initial + 1);
            assert!(rules[initial]
                .selector
                .matches_name(&format!("{prefix}-SAMPLE.EXE")));
        }
        if extra == Some(9) {
            let before = app.side_panel_open;
            let _ = app.update(toggle_side_panel());
            assert_eq!(app.side_panel_open, !before);
        }
        if extra == Some(20) {
            let _ = app.update(Message::PowerTuning(
                advanced_power_plan_tuning::Message::SourceTab(
                    advanced_power_plan_tuning::Source::Battery,
                ),
            ));
        }
        if extra == Some(18) {
            check_child_protection_drafts(app);
            app.settings.efficiency_mode.enabled = true;
        }
        if extra == Some(19) {
            app.settings.by_activity.enabled = true;
        }
        if extra.is_none() && page == Page::ByActivity {
            app.settings.by_activity.input_detection = Default::default();
            for message in [
                by_activity::Message::Keyboard(false),
                by_activity::Message::Mouse(false),
                by_activity::Message::Controller(false),
            ] {
                let _ = app.update(Message::ByActivity(message));
            }
            let input = &app.settings.by_activity.input_detection;
            assert!(input.controller && !input.keyboard && !input.mouse);
            app.settings = SettingsEditor::with_settings((*app.settings).clone());
        }
        if matches!(extra, Some(21 | APP_SUSPENSION_FOCUS_OFF)) {
            app.suspension = Default::default();
            app.settings.app_suspension.enabled = true;
        }
        if matches!(extra, Some(21 | 72 | APP_SUSPENSION_FOCUS_OFF)) {
            assert_eq!(app.page, Page::AppSuspension);
            app.settings.app_suspension.suspendable_apps =
                vec![super::super::process_rules::new_app_suspension_rule(
                    r"C:\Apps\Example\example.exe",
                )];
            app.settings = SettingsEditor::with_settings((*app.settings).clone());
        }
        if matches!(extra, Some(21 | APP_SUSPENSION_SETUP_COMPACT)) {
            use app_suspension::{Delay, Message as SuspensionMessage};

            let saved = app.settings.persisted().app_suspension.clone();
            let duration = if saved.network_wake_duration_seconds == 45 {
                60
            } else {
                45
            };
            let _ = app.update(Message::Suspension(SuspensionMessage::Delay(
                Delay::Network,
                duration.to_string(),
            )));
            assert_eq!(
                app.settings.app_suspension.network_wake_duration_seconds,
                duration
            );
            assert!(!app.settings.app_suspension.network_wake_enabled);
            let draft = app.settings.app_suspension.clone();
            for tab in [FeatureTab::CustomRules, FeatureTab::Setup] {
                let _ = app.update(Message::FeatureTab(tab));
                assert_eq!(app.feature_tab(), tab);
                assert_eq!(app.settings.app_suspension, draft);
                assert_eq!(
                    app.settings
                        .runtime_settings_snapshot()
                        .value
                        .app_suspension,
                    saved
                );
            }
        }
        if extra == Some(APP_SUSPENSION_FOCUS_OFF) {
            let _ = app.update(Message::Suspension(
                app_suspension::Message::AutoResumeWhenFocused(false),
            ));
            let draft = &app.settings.global().app_suspension;
            let saved = &app.settings.persisted().app_suspension;
            assert!(!draft.auto_resume_when_focused);
            assert!(saved.auto_resume_when_focused);
        }
        if extra == Some(24) {
            let _ = app.update(Message::Adaptive(adaptive_engine::Message::TuningTab(
                adaptive_engine::TuningTab::Setup,
            )));
            let _ = app.update(Message::Adaptive(adaptive_engine::Message::Collapse(
                adaptive_engine::TuningGroup::ResourceZones,
            )));
        }
        if extra == Some(17) {
            app.adaptive = adaptive_engine::Editor::default();
        }
        if matches!(
            extra,
            Some(
                DASHBOARD_PREVIEW | 29
                    ..=34
                        | 38
                        | 42
                        | 43
                        | DASHBOARD_ENABLED_FEATURES_COMPACT
                        | DASHBOARD_POWER_SOURCE_UNAVAILABLE
                        | DASHBOARD_HISTORY_WIDE
                        | DASHBOARD_HISTORY_COMPACT
            )
        ) {
            app.dashboard.populate_smoke_history();
            app.settings.general.enabled = true;
            app.settings.cpu_limiter.enabled = true;
            app.settings.process_priority.enabled = true;
            app.settings.memory_trim.enabled = true;
            app.settings.io_priority.enabled = true;
            app.settings = SettingsEditor::with_settings((*app.settings).clone());
        }
        if matches!(
            extra,
            Some(
                PROCESS_LIST_COMMIT_WIDE
                    | PROCESS_LIST_COMMIT_COMPACT
                    | CONTRAST_LIGHT_PROCESS_LIST
                    | GPU_MEMORY_WIDE
                    | GPU_MEMORY_COMPACT
            )
        ) {
            let _ = app.update(Message::Processes(process_list::Message::CloseSelection));
            app.settings = SettingsEditor::with_settings(app.settings.global().clone());
            assert!(!app.processes.modal_open());
        }
        if matches!(extra, Some(GPU_MEMORY_WIDE | GPU_MEMORY_COMPACT)) {
            app.side_panel_open = extra == Some(GPU_MEMORY_COMPACT);
        }
        if extra == Some(DASHBOARD_POWER_SOURCE_UNAVAILABLE) {
            app.dashboard
                .set_power_source(dashboard::PowerSourceReading::Unavailable);
        }
        if matches!(
            extra,
            Some(
                DASHBOARD_PREVIEW | 16 | 29
                    ..=34 | 38 | 42 | 43 | 49 | 86 | DASHBOARD_ENABLED_FEATURES_COMPACT
            )
        ) {
            use crate::action_log::{ActionLogEntry, ActionLogFeature, ActionLogResult};
            app.status.action_log_entries = Arc::new(
                (0..10)
                    .map(|index| ActionLogEntry {
                        sequence: index + 1,
                        batch_id: index + 1,
                        timestamp_epoch_ms: 1_770_000_000_000 + index as u128 * 10_000,
                        feature: ActionLogFeature::MemoryTrim,
                        process_id: Some(1234),
                        process_name: "example.exe".into(),
                        result: if index % 3 == 0 {
                            ActionLogResult::Skipped
                        } else {
                            ActionLogResult::Applied
                        },
                        reason: "Render-smoke fixture: memory trim completed.".into(),
                    })
                    .collect(),
            );
        }
        let search = if matches!(extra, Some(26..=28 | 43)) {
            if extra == Some(26) {
                let _ = app.update(Message::SearchKeyboard {
                    key: iced::keyboard::Key::Character("/".into()),
                    modifiers: iced::keyboard::Modifiers::empty(),
                    repeat: false,
                    status: iced::event::Status::Captured,
                });
                assert!(!app.navigation_search_open);
                let focus = app.update(Message::SearchKeyboard {
                    key: iced::keyboard::Key::Character("k".into()),
                    modifiers: iced::keyboard::Modifiers::CTRL,
                    repeat: false,
                    status: iced::event::Status::Ignored,
                });
                assert!(app.navigation_search_open);
                focus
            } else {
                let _ = app.update(Message::NavigationSearch(
                    if matches!(extra, Some(27 | 43)) {
                        "cpu"
                    } else {
                        "zzzz-no-match"
                    }
                    .into(),
                ));
                assert!(app.navigation_search_open);
                if extra == Some(27) {
                    let down = || Message::SearchKeyboard {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown),
                        modifiers: iced::keyboard::Modifiers::empty(),
                        repeat: false,
                        status: iced::event::Status::Ignored,
                    };
                    let _ = app.update(down());
                    assert_eq!(app.navigation_search_selected, 1);
                    let expected = app.search_choices()[1];
                    let _ = app.update(Message::ConfirmSearch);
                    assert_eq!(app.page, expected);
                    assert!(!app.navigation_search_open);
                    assert!(app.navigation_search.is_empty());
                    let _ = app.update(Message::Page(Page::Dashboard));
                    let _ = app.update(Message::NavigationSearch("cpu".into()));
                    let _ = app.update(down());
                }
                iced::widget::operation::focus(SEARCH_INPUT_ID)
            }
        } else if matches!(extra, Some(16 | 49)) {
            let query = if extra == Some(16) {
                "1234"
            } else {
                "no-match.exe"
            };
            app.update(Message::ActionLog(action_log::Message::Search(
                query.into(),
            )))
        } else {
            Task::none()
        };
        let navigation = if compact_navigation {
            let section = page.section_landing_page();
            let select = |page| Message::Navigation(navigation::Event::Select(page));
            let _ = app.update(select(Page::Dashboard));
            assert_eq!(app.page, Page::Dashboard);
            let _ = app.update(select(section));
            assert_eq!(app.page, section);
            let _ = app.update(select(page));
            assert_eq!(app.page, page);
            let _ = app.update(Message::NavigateHistory(false));
            assert_eq!(app.page, section);
            let forward = app.update(Message::NavigateHistory(true));
            assert_eq!(app.page, page);
            assert!(!app.side_panel_open);
            let _ = app.update(toggle_side_panel());
            assert_eq!(app.side_panel_toggle_state(), Some(true));
            forward
        } else {
            Task::none()
        };
        if matches!(extra, Some(0 | 1 | 41)) {
            for &tab in SidePanelTab::for_page(page) {
                let _ = app.update(Message::SidePanelTab(tab));
                assert_eq!(app.side_panel_tab, tab);
                assert!(app.side_panel(is_plugged_in).is_some());
            }
            let _ = app.update(Message::Page(page));
            assert_eq!(app.side_panel_tab, SidePanelTab::Status);
            let _ = app.update(toggle_side_panel());
            assert!(app.side_panel_open);
            let _ = app.update(Message::SearchKeyboard {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                modifiers: iced::keyboard::Modifiers::empty(),
                repeat: false,
                status: iced::event::Status::Ignored,
            });
            assert!(app.side_panel_open);
            assert_eq!(app.page, page);
            let _ = app.update(Message::OpenNavigationSearch);
            assert!(app.side_panel_open);
            let _ = app.update(Message::CloseNavigationSearch);
            assert!(app.side_panel_open);
            let _ = app.update(Message::CloseSidePanel);
            assert!(!app.side_panel_open);
            let _ = app.update(toggle_side_panel());
            app.settings = SettingsEditor::with_settings((*app.settings).clone());
        }
        if matches!(extra, Some(44..=46)) {
            let tab = SidePanelTab::Presets;
            let _ = app.update(Message::SidePanelTab(tab));
            assert_eq!(app.side_panel_tab, tab);
            assert!(app.side_panel(is_plugged_in).is_some());
            app.side_panel_open = true;
            app.settings = SettingsEditor::with_settings((*app.settings).clone());
        }
        if matches!(extra, Some(16 | 47..=50)) {
            app.side_panel_open = extra != Some(50);
            app.settings = SettingsEditor::with_settings((*app.settings).clone());
        }
        if matches!(extra, Some(66..=69)) {
            app.side_panel_open = true;
            app.side_panel_tab = SidePanelTab::Status;
        }
        if extra == Some(70) {
            exercise_adaptive_processor_power_presets(app);
            let _ = app.update(Message::AdaptiveProcessorPower(
                adaptive_processor_power::Message::Tab(
                    adaptive_processor_power::Tab::DemandProfiles,
                ),
            ));
        } else if extra == Some(71) {
            use adaptive_processor_power::Message as PowerMessage;

            let _ = app.update(Message::AdaptiveProcessorPower(PowerMessage::NewPreset));
            assert!(app.adaptive_processor_power.preset_open());
        }
        if let Some(index @ 73..=76) = extra {
            exercise_cpu_preset_dialog(app, index);
        }
        if extra == Some(77) {
            exercise_settings_export(app);
        }
        if matches!(extra, Some(79..=80)) {
            app.settings = SettingsEditor::with_settings(app.settings.global().clone());
            app.settings.edit_global(|settings| {
                settings.general.show_feature_status_on_cards =
                    !settings.general.show_feature_status_on_cards;
            });
            assert!(app.settings.has_unsaved_changes());
            let context = extra == Some(79);
            app.processes.show_copy_path_fixture(context);
            assert_eq!(app.processes.context_open(), context);
        }
        if matches!(
            extra,
            Some(PROCESS_LIST_COMMIT_WIDE | PROCESS_LIST_COMMIT_COMPACT)
        ) {
            app.side_panel_open = extra == Some(PROCESS_LIST_COMMIT_COMPACT);
            let _ = app.update(Message::Processes(process_list::Message::Column(
                process_list::Sort::CpuTime,
                true,
            )));
            let _ = app.update(Message::Processes(process_list::Message::Column(
                process_list::Sort::PrivateCommit,
                true,
            )));
            let _ = app.update(Message::Processes(process_list::Message::Sort(
                process_list::Sort::PrivateCommit,
            )));
            let _ = app.update(Message::Processes(process_list::Message::MemoryPercentage(
                true,
            )));
        }
        let dashboard_status = if extra == Some(DASHBOARD_PREVIEW) {
            let _ = app.update(Message::SidePanelTab(SidePanelTab::Status));
            app.side_panel_open = true;
            Task::none()
        } else if matches!(
            extra,
            Some(DASHBOARD_ENABLED_FEATURES_COMPACT | DASHBOARD_POWER_SOURCE_UNAVAILABLE)
        ) {
            assert_eq!(app.side_panel_tab, SidePanelTab::Settings);
            assert!(!app.side_panel_open);
            let _ = app.update(toggle_side_panel());
            assert!(app.side_panel_open);
            let status = app.update(Message::SidePanelTab(SidePanelTab::Status));
            assert!(app.dashboard_status_open());
            assert!(app.side_panel(is_plugged_in).is_some());
            status
        } else {
            Task::none()
        };
        if matches!(extra, Some(81..=83)) {
            use crate::control::power_plan::{PowerPlanOwner, PowerPlanStatus};
            use crate::rules::DecisionState;

            let mut settings = app.settings.global().clone();
            settings.general.enabled = true;
            *navigation::feature_toggle(page).expect("power rule has a switch")(&mut settings) =
                true;
            app.settings = SettingsEditor::with_settings(settings);
            app.status.generation = app.status.generation.max(1);
            let guid = "381b4222-f694-41f0-9685-ff5bb260df2e";
            let owner = (extra == Some(83)).then_some(PowerPlanOwner::AdaptiveProcessorPower);
            app.status.power_plan_status = Arc::new(PowerPlanStatus {
                owner,
                current_guid: Some(guid.into()),
                target_guid: owner.map(|_| guid.into()),
                decision_state: Some(if extra == Some(82) {
                    DecisionState::WaitingForObservation
                } else {
                    DecisionState::PausedWhilePluggedIn
                }),
                ..Default::default()
            });
            app.side_panel_open = true;
            let _ = app.update(Message::SidePanelTab(SidePanelTab::Status));
            assert_eq!(app.side_panel_tab, SidePanelTab::Status);
        }
        if extra == Some(ADAPTIVE_RESTRAINT_RECOVERY) {
            use adaptive_engine::{Message as AdaptiveMessage, TuningGroup, TuningTab};

            app.adaptive.reset_drafts();
            let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(
                TuningTab::Setup,
            )));
            let _ = app.update(Message::Adaptive(AdaptiveMessage::Collapse(
                TuningGroup::CpuTriggers,
            )));
        }
        if extra == Some(ADAPTIVE_SETUP_ENABLED) {
            use adaptive_engine::{Message as AdaptiveMessage, TuningTab};

            let mut settings = crate::config::Settings::default();
            settings.general = app.settings.global().general.clone();
            settings.general.enabled = false;
            app.settings = SettingsEditor::with_settings(settings);
            app.power_source = PowerSourceProfile::PluggedIn;
            app.adaptive.reset_drafts();

            let before = app.settings.runtime_settings_snapshot().value;
            let _ = app.update(Message::PowerSource(PowerSourceProfile::OnBattery));
            let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(
                TuningTab::Setup,
            )));
            let _ = app.update(Message::Adaptive(AdaptiveMessage::SetEnabled(true)));
            assert_eq!(
                app.settings.selected_power_source(),
                PowerSourceProfile::OnBattery
            );
            assert!(app.settings.adaptive_engine.enabled);
            assert!(
                app.settings
                    .adaptive_engine_process
                    .cpu_pressure_restraint_enabled
            );
            assert_eq!(app.settings.adaptive_engine_process.reaction_time_ms, 1_500);
            assert_eq!(
                app.settings.global().adaptive_engine,
                before.adaptive_engine
            );
            assert_eq!(
                app.settings.global().adaptive_engine_process,
                before.adaptive_engine_process
            );
            assert_eq!(app.settings.runtime_settings_snapshot().value, before);
            assert!(app.pending_settings_changes());
        }
        if matches!(extra, Some(84..=85)) {
            use adaptive_engine::{Message as AdaptiveMessage, TuningGroup, TuningTab};

            let tab = if extra == Some(84) {
                app.adaptive.reset_drafts();
                app.settings.general.enabled = false;
                app.settings = SettingsEditor::with_settings(app.settings.global().clone());
                let enabled = !app
                    .settings
                    .adaptive_engine_process
                    .cpu_pressure_restraint_enabled;
                let _ = app.update(Message::Adaptive(AdaptiveMessage::Toggle(
                    |settings, value| {
                        settings
                            .adaptive_engine_process
                            .cpu_pressure_restraint_enabled = value;
                    },
                    enabled,
                )));
                let _ = app.update(Message::Adaptive(AdaptiveMessage::Collapse(
                    TuningGroup::CpuTriggers,
                )));
                app.side_panel_open = true;
                TuningTab::Setup
            } else {
                TuningTab::Exclusions
            };
            let pending = app.settings.adaptive_engine_process.clone();
            let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(tab)));
            assert_eq!(app.settings.adaptive_engine_process, pending);
            assert!(app.pending_settings_changes());
        }
        if extra == Some(86) {
            app.settings = SettingsEditor::with_settings(app.settings.global().clone());
            let mode =
                if app.settings.advanced.action_log_mode == crate::config::ActionLogMode::Full {
                    crate::config::ActionLogMode::Warning
                } else {
                    crate::config::ActionLogMode::Full
                };
            let _ = app.update(Message::ActionLog(action_log::Message::LogMode(mode)));
            app.side_panel_open = true;
            assert!(app.pending_settings_changes());
        }
        if matches!(extra, Some(87..=88)) {
            let mut settings = app.settings.global().clone();
            settings.general.enabled = true;
            settings.process_priority.enabled = true;
            app.settings = SettingsEditor::with_settings(settings);
            app.status.generation = app.status.generation.max(1);
            let status = &mut Arc::make_mut(&mut app.status.feature_status).process_priority;
            status.enabled = true;
            status.scanned_processes = 7;
            status.adjusted_processes = 3;
            status.efficiency_mode_shadowed_processes = 2;
            app.side_panel_open = true;
            let _ = app.update(Message::SidePanelTab(SidePanelTab::Status));
            assert_eq!(app.side_panel_tab, SidePanelTab::Status);
        }
        let limiter_affinity = matches!(
            extra,
            Some(
                CPU_LIMITER_AFFINITY_WIDE
                    | CPU_LIMITER_AFFINITY_COMPACT
                    | CPU_LIMITER_PROCESSORS_COMPACT
            )
        );
        if matches!(extra, Some(89..=91)) || limiter_affinity {
            use crate::config::{CpuLimiterAffinityLimit, Settings};
            use crate::control::cpu_limiter::CpuLimiterActivation;
            use crate::cpu_topology::CpuSelection;
            use crate::features::cpu_control::cpu_limiter::CpuLimiterAffinityStatus;

            let path = r"C:\Apps\example.exe";
            let mut settings = Settings {
                general: app.settings.global().general.clone(),
                ..Settings::default()
            };
            settings.general.enabled = true;
            settings.cpu_limiter.enabled = true;
            let mut rule = super::super::process_rules::new_cpu_limiter_rule(path);
            if limiter_affinity {
                let selection = crate::cpu_allocation::processor_topology()
                    .ok()
                    .and_then(|topology| {
                        let processor =
                            topology.processors().iter().find(|cpu| cpu.id.group == 0)?;
                        CpuSelection::bind(&topology, vec![processor.id]).ok()
                    })
                    .unwrap_or_default();
                rule.affinity_limit = Some(CpuLimiterAffinityLimit {
                    selection,
                    ..CpuLimiterAffinityLimit::default()
                });
            }
            let affinity_count = rule
                .affinity_limit
                .as_ref()
                .map(|limit| limit.selection.processors().len());
            settings.cpu_limiter.rules = vec![rule];
            let plugged_in = is_plugged_in().unwrap_or(true);
            let source = if plugged_in {
                PowerSourceProfile::PluggedIn
            } else {
                PowerSourceProfile::OnBattery
            };
            app.settings = SettingsEditor::with_settings(settings);
            app.settings.select_power_source(source);
            app.power_source = source;
            app.cpu_limiter = Default::default();
            let status = &mut Arc::make_mut(&mut app.status.feature_status).cpu_limiter;
            status.source = Some(source);
            status.enabled = true;
            status.limited_processes = 1;
            status.active_affinity.clear();
            status.active_limits.clear();
            if let Some(count) = affinity_count {
                let active = if count > 0 {
                    CpuLimiterAffinityStatus::Active(count)
                } else {
                    CpuLimiterAffinityStatus::Unavailable
                };
                status.active_affinity.push((path.into(), active));
            } else {
                status.active_limits.push((
                    path.into(),
                    match extra {
                        Some(89) => CpuLimiterActivation::Job(40),
                        Some(90) => CpuLimiterActivation::AncestorJob(60),
                        _ => CpuLimiterActivation::ThreadFallback(25),
                    },
                ));
            }
            assert!(app.cpu_limiter_rule_status(|| Some(plugged_in)).is_some());
            assert!(app.cpu_limiter_rule_status(|| Some(!plugged_in)).is_none());
            let _ = app.update(Message::CpuLimiter(cpu_limiter::Message::Edit(0)));
            assert!(app.cpu_limiter.has_pending_editor());
        }
        if matches!(extra, Some(92..=93)) {
            use crate::backend::interrupt_timing::InterruptTimingReport;
            use adaptive_engine::{Message as AdaptiveMessage, TuningTab};

            app.adaptive.reset_drafts();
            let before = app.settings.runtime_settings_snapshot().value;
            let draft = app.settings.adaptive_engine_process.clone();
            let source = app.settings.selected_power_source();
            let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(
                TuningTab::Setup,
            )));
            let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(
                TuningTab::InterruptTiming,
            )));
            assert_eq!(app.settings.adaptive_engine_process, draft);
            assert_eq!(app.settings.selected_power_source(), source);
            assert_eq!(app.settings.runtime_settings_snapshot().value, before);
            let modules = if extra == Some(92) {
                vec![serde_json::from_value(serde_json::json!({
                    "kind": "DPC",
                    "module": "example.sys",
                    "count": 2,
                    "total_inclusive_ms": 0.6,
                    "max_inclusive_ms": 0.4,
                }))
                .expect("render-smoke module fixture")]
            } else {
                Vec::new()
            };
            app.interrupt_timing
                .set_smoke_report(InterruptTimingReport {
                    capture_start: None,
                    events_lost: 1,
                    buffers_lost: 2,
                    invalid_intervals: 3,
                    incomplete_intervals: 4,
                    invalid_image_records: 0,
                    unresolved_attribution: 0,
                    modules,
                    per_cpu: None,
                    timeline: Default::default(),
                });
        }
        if matches!(
            extra,
            Some(RESOURCE_PREVIEW_WIDE | RESOURCE_PREVIEW_COMPACT)
        ) {
            app.settings.edit_global(|settings| {
                settings.resource_thresholds.rules =
                    vec![crate::config::ResourceThresholdsRule::with_selector(
                        crate::config::ResourceThresholdsSelector::NamePattern {
                            executable_name_pattern:
                                crate::config::ExecutableNamePattern::try_from(
                                    "work*.exe".to_owned(),
                                )
                                .expect("preview fixture has a valid executable name pattern"),
                        },
                    )];
            });
            let _ = app.update(Message::ResourceThresholds(
                resource_thresholds::Message::Edit(0),
            ));
            app.resource_thresholds.set_smoke_preview_matches();
        }
        if matches!(extra, Some(SHOW_SHORTCUT_INVALID | SHOW_SHORTCUT_READY)) {
            use iced::keyboard::{key::Named, Key, Modifiers};
            use settings_pages::{Message as Preference, ShortcutSlot};

            app.settings = SettingsEditor::with_settings(app.settings.global().clone());
            app.side_panel_open = extra == Some(SHOW_SHORTCUT_READY);
            let _ = app.update(Message::Preferences(Preference::RecordShortcut(
                ShortcutSlot::ShowWinderust,
            )));
            let modifiers = if extra == Some(SHOW_SHORTCUT_READY) {
                Modifiers::CTRL | Modifiers::ALT
            } else {
                Modifiers::CTRL
            };
            let _ = app.search_keyboard(
                Key::Named(Named::F24),
                modifiers,
                false,
                iced::event::Status::Ignored,
            );
            assert!(app
                .settings
                .persisted()
                .general
                .show_winderust_shortcut
                .is_none());
            assert_eq!(
                app.settings
                    .global()
                    .general
                    .show_winderust_shortcut
                    .is_some(),
                extra == Some(SHOW_SHORTCUT_READY),
            );
            assert_eq!(
                app.preferences.show_shortcut_capture_error.is_some(),
                extra == Some(SHOW_SHORTCUT_INVALID),
            );
        }
        if matches!(extra, Some(DOCK_FOCUS_WIDE | DOCK_FOCUS_COMPACT)) {
            app.settings.cancel();
            app.reset_editors();
            assert!(!app.side_panel_open);
            for open in [true, false] {
                let _ = app.update(toggle_side_panel());
                assert_eq!(app.side_panel_open, open);
                for destination in [Page::About, Page::Dashboard, Page::PowerPlanControl, page] {
                    let _ = app.update(Message::Navigation(navigation::Event::Select(destination)));
                    assert_eq!(app.page, destination);
                    assert_eq!(app.side_panel_open, open);
                    let supported = destination != Page::About;
                    assert_eq!(app.side_panel_toggle_state(), supported.then_some(open));
                    assert_eq!(app.side_panel(is_plugged_in).is_some(), supported);
                }
            }
            let _ = app.update(toggle_side_panel());
            assert!(app.side_panel_open);
        }
        if matches!(extra, Some(MEMORY_TRIM_PARTIAL | MEMORY_TRIM_UNKNOWN)) {
            use crate::memory_trim::{MemoryTrimReadback, MemoryTrimSnapshot, MemoryTrimStatus};

            app.trim = memory_trim::Editor::default();
            if extra == Some(MEMORY_TRIM_UNKNOWN) {
                let _ = app.update(Message::Trim(memory_trim::Message::ExcludeForeground(
                    false,
                )));
                let _ = app.update(Message::Trim(memory_trim::Message::ExcludeVisibleWindows(
                    true,
                )));
            }
            let _ = app.update(Message::TrimFinished(Ok(MemoryTrimSnapshot {
                status: MemoryTrimStatus::ManualCompleted,
                trimmed_processes: 3,
                readback: (extra == Some(MEMORY_TRIM_PARTIAL)).then_some(MemoryTrimReadback {
                    processes: 2,
                    reduction_bytes: 12_000_000,
                }),
                ..Default::default()
            })));
        }
        if compact_navigation {
            let mut fixture = SettingsEditor::with_settings(app.settings.global().clone());
            fixture.edit_global(|settings| {
                settings.general.show_enabled_feature_counts =
                    extra != Some(NAVIGATION_CPU_COMPACT_ZH_TW);
            });
            fixture.select_power_source(app.settings.selected_power_source());
            for section in [
                Page::Watchdog,
                Page::PowerPlanControl,
                Page::PriorityControl,
                Page::CpuControl,
            ] {
                let mut enabled = 0;
                for toggle in navigation::children(section)
                    .into_iter()
                    .filter_map(navigation::feature_toggle)
                {
                    *toggle(&mut fixture) = true;
                    enabled += 1;
                }
                assert!(enabled > 0);
                assert_eq!(
                    navigation::section_enabled_feature_count(&fixture, section),
                    Some(enabled)
                );
            }
            app.smoke
                .as_mut()
                .expect("navigation smoke run")
                .navigation_settings = Some(std::mem::replace(&mut app.settings, fixture));
        }
        let size = match extra {
            Some(
                PROCESS_LIST_COMMIT_COMPACT
                | DASHBOARD_ENABLED_FEATURES_COMPACT
                | DASHBOARD_POWER_SOURCE_UNAVAILABLE
                | CONTRAST_LIGHT_PROCESS_LIST
                | DASHBOARD_HISTORY_COMPACT
                | SHOW_SHORTCUT_READY
                | DOCK_FOCUS_COMPACT
                | MEMORY_TRIM_PARTIAL
                | MEMORY_TRIM_UNKNOWN
                | KEEP_AWAKE_ACTIVE
                | KEEP_AWAKE_MIXED
                | APP_SUSPENSION_FOCUS_OFF
                | APP_SUSPENSION_SETUP_COMPACT
                | GPU_MEMORY_COMPACT
                | CPU_LIMITER_AFFINITY_COMPACT
                | CPU_LIMITER_PROCESSORS_COMPACT
                | ADAPTIVE_SETUP_ENABLED
                | 7
                | 9
                | ADAPTIVE_NAME_PATTERN,
            ) => iced::Size::new(900.0, 620.0),
            Some(RESOURCE_PREVIEW_COMPACT) => iced::Size::new(900.0, 620.0),
            Some(
                RESOURCE_PREVIEW_WIDE | SHOW_SHORTCUT_INVALID | DOCK_FOCUS_WIDE | KEEP_AWAKE_READY,
            ) => iced::Size::new(1120.0, 760.0),
            Some(
                6
                | 11
                | CONTRAST_DARK_APPEARANCE
                | DASHBOARD_HISTORY_WIDE
                | CPU_LIMITER_AFFINITY_WIDE,
            ) => iced::Size::new(1120.0, 760.0),
            _ if compact_navigation => iced::Size::new(900.0, 620.0),
            Some(40 | 43..=50 | 52..=69 | 71..=93) => iced::Size::new(900.0, 620.0),
            Some(26..=28) => iced::Size::new(1120.0, 760.0),
            Some(30) => iced::Size::new(1280.0, 720.0),
            Some(DASHBOARD_PREVIEW | 31) => iced::Size::new(1440.0, 900.0),
            Some(32) => iced::Size::new(1920.0, 720.0),
            Some(33 | NAVIGATION_RESIZE_TALL) => iced::Size::new(900.0, 1000.0),
            Some(34) => iced::Size::new(1536.0, 960.0),
            Some(4 | 70 | ADAPTIVE_RESTRAINT_RECOVERY) => iced::Size::new(1120.0, 920.0),
            Some(FEATURE_TAB_STATES_START..) if chinese => iced::Size::new(900.0, 620.0),
            Some(FEATURE_TAB_STATES_START..) => iced::Size::new(1120.0, 760.0),
            _ => iced::Size::new(
                if extra.is_some_and(|i| i >= 13) {
                    1920.0
                } else if chinese || extra.is_some() {
                    900.0
                } else {
                    1120.0
                },
                if extra.is_some_and(|i| i >= 13) {
                    1080.0
                } else if chinese {
                    620.0
                } else {
                    760.0
                },
            ),
        };
        return Some(Task::batch([
            iced::window::resize(window, size),
            navigate,
            switch_tab,
            search,
            navigation,
            dashboard_status,
        ]));
    }
    run.ticks += 1;
    if matches!(extra, Some(GPU_MEMORY_WIDE | GPU_MEMORY_COMPACT)) && run.ticks == 7 {
        return Some(app.update(Message::Processes(process_list::Message::Refresh)));
    }
    if matches!(extra, Some(DOCK_FOCUS_WIDE | DOCK_FOCUS_COMPACT)) {
        match run.ticks {
            3 => return Some(iced::widget::operation::focus(SIDE_PANEL_CLOSE)),
            5 => return Some(app.update(Message::CloseSidePanel)),
            12 => assert!(!app.side_panel_open),
            _ => {}
        }
    }
    if compact_navigation {
        match run.ticks {
            3 => {
                return Some(iced::widget::operation::focus(navigation::page_id(
                    app.page.section_landing_page(),
                )));
            }
            5 => return Some(crate::ui::keyboard_focus::move_focus(true, false)),
            7 => {
                return Some(iced::advanced::widget::operate(NavigationLayout::new(
                    app.page,
                    app.window_settings.size.width,
                    &app.settings,
                )));
            }
            _ => {}
        }
    } else if extra.is_none() && app.page == Page::Dashboard && run.ticks == 3 {
        return Some(Task::batch([
            iced::advanced::widget::operate(NavigationLayout::new(
                app.page,
                app.window_settings.size.width,
                &app.settings,
            )),
            iced::advanced::widget::operate(DashboardLayout::new(false, false)),
        ]));
    }
    if run.ticks == 3 {
        match extra {
            Some(PROCESS_LIST_COMMIT_COMPACT) => {
                return Some(iced::advanced::widget::operate(DashboardLayout::new(
                    true, false,
                )));
            }
            Some(
                DASHBOARD_PREVIEW
                | DASHBOARD_ENABLED_FEATURES_COMPACT
                | DASHBOARD_POWER_SOURCE_UNAVAILABLE,
            ) => {
                return Some(iced::advanced::widget::operate(DashboardLayout::new(
                    true, true,
                )));
            }
            Some(26..=28 | 43) => {
                return Some(iced::advanced::widget::operate(NavigationLayout {
                    search_open: true,
                    ..NavigationLayout::new(app.page, app.window_settings.size.width, &app.settings)
                }));
            }
            Some(CPU_LIMITER_AFFINITY_COMPACT) => {
                return Some(iced::advanced::widget::operate(
                    ProcessorPickerVisibility::default(),
                ));
            }
            Some(CPU_LIMITER_PROCESSORS_COMPACT) => {
                return Some(
                    app.update(Message::CpuLimiter(cpu_limiter::Message::ChooseProcessors)),
                );
            }
            Some(CONTRAST_DARK_APPEARANCE) => {
                return Some(iced::widget::operation::focus(navigation::workspace_id(
                    Page::SettingsHome,
                )));
            }
            Some(CONTRAST_LIGHT_PROCESS_LIST) => {
                return Some(Task::batch([
                    iced::widget::operation::focus("process-list"),
                    app.update(Message::Processes(process_list::Message::MoveRowFocus(
                        true, 400.0,
                    ))),
                ]));
            }
            Some(DASHBOARD_HISTORY_WIDE | DASHBOARD_HISTORY_COMPACT) => {
                let kind = if extra == Some(DASHBOARD_HISTORY_WIDE) {
                    dashboard::ChartKind::Cpu
                } else {
                    dashboard::ChartKind::Network
                };
                for _ in 0..2 {
                    let _ = app.update(Message::Dashboard(dashboard::Message::InspectChart(
                        kind,
                        dashboard::HistoryAction::Step { newer: false },
                    )));
                }
                return Some(iced::widget::operation::focus(kind.focus_id()).chain(
                    crate::ui::keyboard_focus::reveal(Some(kind.focus_id().into())),
                ));
            }
            _ => {}
        }
    }
    if extra == Some(DASHBOARD_ENABLED_FEATURES_COMPACT) && run.ticks == 5 {
        return Some(iced::widget::operation::snap_to(
            "dashboard-status",
            iced::widget::scrollable::RelativeOffset::END,
        ));
    }
    if extra == Some(53) && run.ticks == 3 {
        return Some(iced::widget::operation::snap_to(
            "resource-thresholds-rule-details",
            iced::widget::scrollable::RelativeOffset { x: 0.0, y: 1.0 },
        ));
    }
    if matches!(extra, Some(56 | 57)) && run.ticks == 3 {
        return Some(iced::widget::operation::snap_to(
            "by-cpu-load-rule-details",
            iced::widget::scrollable::RelativeOffset { x: 0.0, y: 1.0 },
        ));
    }
    if matches!(
        extra,
        Some(62..=63 | 72 | MEMORY_TRIM_UNKNOWN | APP_SUSPENSION_SETUP_COMPACT)
    ) && run.ticks == 3
    {
        return Some(iced::widget::operation::snap_to(
            feature_tabs::CONTENT_ID,
            iced::widget::scrollable::RelativeOffset { x: 0.0, y: 1.0 },
        ));
    }
    if matches!(
        extra,
        Some(77..=78 | SHOW_SHORTCUT_INVALID | SHOW_SHORTCUT_READY)
    ) && run.ticks == 3
    {
        return Some(iced::widget::operation::snap_to(
            "settings-content",
            iced::widget::scrollable::RelativeOffset { x: 0.0, y: 1.0 },
        ));
    }
    if matches!(extra, Some(4 | 6 | 7 | 9 | 84 | ADAPTIVE_NAME_PATTERN)) && run.ticks == 3 {
        let y = match extra {
            Some(4) => 0.55,
            Some(9) => 0.92,
            _ => 1.0,
        };
        return Some(iced::widget::operation::snap_to(
            adaptive_engine::CONTENT_ID,
            iced::widget::scrollable::RelativeOffset { x: 0.0, y },
        ));
    }
    if extra == Some(DASHBOARD_PREVIEW) && run.ticks == 11 {
        app.processes.populate_smoke_rows();
        let guid = "381b4222-f694-41f0-9685-ff5bb260df2e".to_owned();
        app.status.power_plan_status = Arc::new(crate::control::power_plan::PowerPlanStatus {
            current_guid: Some(guid.clone()),
            ..Default::default()
        });
        app.power_plans = vec![crate::power::PowerPlan {
            guid,
            name: "Balanced".into(),
            active: true,
            managed_adaptive: false,
        }];
        return Some(Task::none());
    }
    if run.ticks
        == if extra.is_some_and(|i| i >= 13) {
            12
        } else {
            4
        }
    {
        if matches!(
            extra,
            Some(NAVIGATION_RESIZE_TALL | NAVIGATION_RESIZE_COMPACT)
        ) {
            assert_eq!(app.page, Page::AdvancedControls);
            assert!(app.settings.global().advanced.show_advanced_controls);
            let height = if extra == Some(NAVIGATION_RESIZE_TALL) {
                1000.0
            } else {
                620.0
            };
            assert!((app.window_settings.size.height - height).abs() <= 1.0);
        }
        Some(iced::window::screenshot(window).map(Message::SmokeScreenshot))
    } else {
        Some(Task::none())
    }
}

struct NavigationLayout {
    window_width: f32,
    workspaces: Vec<(Id, Option<Rectangle>, Option<usize>)>,
    count_labels: Vec<(Rectangle, usize)>,
    search: Option<Rectangle>,
    search_open: bool,
    selected: Option<(Id, Option<Rectangle>)>,
    left: Option<(Rectangle, Vector)>,
    right_close: Option<Rectangle>,
}

impl NavigationLayout {
    fn new(page: Page, window_width: f32, settings: &SettingsEditor) -> Self {
        let workspaces =
            navigation::workspace_pages(settings.global().advanced.show_advanced_controls)
                .map(|section| {
                    let count = navigation::section_enabled_feature_count(settings, section)
                        .filter(|&count| settings.general.show_enabled_feature_counts && count > 0);
                    (navigation::workspace_id(section), None, count)
                })
                .collect();
        let selected = (!navigation::children(page.section_landing_page()).is_empty())
            .then(|| (navigation::page_id(page), None));
        Self {
            window_width,
            workspaces,
            count_labels: Vec::new(),
            search: None,
            search_open: false,
            selected,
            left: None,
            right_close: None,
        }
    }
}

impl Operation<Message> for NavigationLayout {
    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation<Message>)) {
        visit(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        _: Rectangle,
        translation: Vector,
        _: &mut dyn operation::Scrollable,
    ) {
        if id == Some(&Id::from(navigation::PAGE_LIST_ID)) {
            self.left = Some((bounds, translation));
        }
    }

    fn text_input(&mut self, id: Option<&Id>, bounds: Rectangle, _: &mut dyn operation::TextInput) {
        if id == Some(&Id::from(SEARCH_INPUT_ID)) {
            assert!(self.search.replace(bounds).is_none(), "Search is one input");
        }
    }

    fn text(&mut self, _: Option<&Id>, bounds: Rectangle, label: &str) {
        if let Ok(count) = label.parse() {
            self.count_labels.push((bounds, count));
        }
    }

    fn focusable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        state: &mut dyn operation::Focusable,
    ) {
        if let Some((_, workspace, _)) = self
            .workspaces
            .iter_mut()
            .find(|(key, _, _)| id == Some(key))
        {
            *workspace = Some(bounds);
        }
        if let Some((key, selected)) = &mut self.selected {
            if id == Some(key) {
                assert!(state.is_focused(), "Tab focuses the next page");
                *selected = Some(bounds);
            }
        }
        if self.search_open && id == Some(&Id::from(SEARCH_INPUT_ID)) {
            assert!(
                state.is_focused(),
                "Search results keep title bar input focused"
            );
        }
        if id == Some(&Id::from(SIDE_PANEL_CLOSE)) {
            self.right_close = Some(bounds);
        }
    }

    fn finish(&self) -> operation::Outcome<Message> {
        let search = self.search.expect("Search is a title bar text input");
        assert!(search.width > 0.0 && search.x >= 0.0);
        assert!((search.center_x() - self.window_width / 2.0).abs() <= 1.0);
        if self.search_open {
            return operation::Outcome::None;
        }
        for (id, bounds, count) in &self.workspaces {
            let bounds = bounds.unwrap_or_else(|| panic!("workspace is focusable: {id:?}"));
            assert!(bounds.width > 0.0 && bounds.x >= 0.0);
            assert!(bounds.x + bounds.width <= self.window_width + 1.0);
            assert!(search.y + search.height <= bounds.y);
            let mut counts = self
                .count_labels
                .iter()
                .filter_map(|(label, count)| label.is_within(&bounds).then_some(*count));
            assert_eq!(counts.next(), *count, "workspace badge: {id:?}");
            assert_eq!(counts.next(), None, "one workspace badge: {id:?}");
        }
        if let Some((_, selected)) = self.selected {
            let (left, translation) = self.left.expect("selected workspace has a page list");
            let active = selected.expect("active page is focusable");
            assert!((active - translation).is_within(&left));
            assert!(left.y >= search.y + search.height);
            assert!(!self
                .count_labels
                .iter()
                .any(|(label, _)| label.is_within(&left)));
            let close = self.right_close.expect("right Side panel is open");
            assert!(left.x + left.width < close.x);
            assert!(close.x + close.width <= self.window_width + 1.0);
        } else {
            assert!(self.left.is_none());
            assert!(self.right_close.is_none());
        }
        operation::Outcome::None
    }
}

struct DashboardLayout {
    dock_open: bool,
    status_open: bool,
    charts: [(Id, Option<Rectangle>); 4],
    table: Option<Rectangle>,
    dock: Option<Rectangle>,
    status: Option<Rectangle>,
}

impl DashboardLayout {
    fn new(dock_open: bool, status_open: bool) -> Self {
        use dashboard::ChartKind::{Cpu, Io, Memory, Network};

        Self {
            dock_open,
            status_open,
            charts: [Cpu, Memory, Io, Network].map(|kind| (Id::from(kind.focus_id()), None)),
            table: None,
            dock: None,
            status: None,
        }
    }
}

impl Operation<Message> for DashboardLayout {
    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation<Message>)) {
        visit(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        _: Rectangle,
        _: Vector,
        _: &mut dyn operation::Scrollable,
    ) {
        if id == Some(&Id::from("process-list")) {
            self.table = Some(bounds);
        } else if id == Some(&Id::from("dashboard-status")) {
            self.status = Some(bounds);
        }
    }

    fn focusable(&mut self, id: Option<&Id>, bounds: Rectangle, _: &mut dyn operation::Focusable) {
        for (key, chart) in &mut self.charts {
            if id == Some(key) {
                *chart = Some(bounds);
            }
        }
        if id == Some(&Id::from(SIDE_PANEL_CLOSE)) {
            self.dock = Some(bounds);
        }
    }

    fn finish(&self) -> operation::Outcome<Message> {
        let table = self.table.expect("Dashboard contains the Process List");
        assert!(
            table.height >= 108.0,
            "Process List displays at least three rows"
        );
        let charts = self.charts.each_ref().map(|(id, bounds)| {
            bounds.unwrap_or_else(|| panic!("Dashboard chart visible: {id:?}"))
        });
        for chart in &charts {
            assert!(chart.width > 0.0 && chart.height > 0.0);
            assert!((chart.y - charts[0].y).abs() <= 1.0);
            assert!(chart.y + chart.height <= table.y);
        }
        for pair in charts.windows(2) {
            assert!(pair[0].x + pair[0].width <= pair[1].x);
        }
        assert_eq!(self.dock.is_some(), self.dock_open);
        assert_eq!(self.status.is_some(), self.status_open);
        if let Some(status) = self.status {
            assert!(charts[3].x + charts[3].width <= status.x);
        }
        operation::Outcome::None
    }
}

#[derive(Default)]
struct ProcessorPickerVisibility {
    viewport: Option<(Rectangle, Vector)>,
    button: Option<Rectangle>,
}

impl Operation<Message> for ProcessorPickerVisibility {
    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation<Message>)) {
        visit(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        _: Rectangle,
        translation: Vector,
        _: &mut dyn operation::Scrollable,
    ) {
        if id == Some(&Id::from("cpu-limiter-rule-details")) {
            self.viewport = Some((bounds, translation));
        }
    }

    fn focusable(&mut self, id: Option<&Id>, bounds: Rectangle, _: &mut dyn operation::Focusable) {
        if id == Some(&Id::from(cpu_limiter::CHOOSE_PROCESSORS_ID)) {
            self.button = Some(bounds);
        }
    }

    fn finish(&self) -> operation::Outcome<Message> {
        let (viewport, translation) = self.viewport.expect("CPU Limiter rule scroll area");
        let button = self.button.expect("Choose processors is focusable");
        assert!((button - translation).is_within(&viewport));
        operation::Outcome::None
    }
}

fn check_child_protection_drafts(app: &mut WinderustApp) {
    use priority_control::ChildProtection;

    let previous = std::mem::replace(
        &mut app.settings,
        SettingsEditor::with_settings(crate::config::Settings::default()),
    );
    let saved = app.settings.persisted().clone();
    let mut expected = saved.clone();
    let battery = expected.battery_profile_mut();
    battery
        .process_priority
        .child_protection
        .focused_app_children = false;
    battery
        .efficiency_mode
        .child_protection
        .visible_window_app_children = false;

    app.settings
        .select_power_source(PowerSourceProfile::OnBattery);
    let _ = app.update(Message::Priority(
        priority_control::Kind::Process,
        priority_control::Message::ProtectChildren(ChildProtection::Focus, false),
    ));
    let _ = app.update(Message::EfficiencyMode(
        efficiency_mode::Message::ProtectChildren(ChildProtection::VisibleWindow, false),
    ));
    assert_eq!(app.settings.global(), &expected);
    assert_eq!(app.settings.persisted(), &saved);
    assert!(app.settings.has_unsaved_changes());
    app.settings.cancel();
    assert_eq!(app.settings.global(), &saved);
    app.settings = previous;
}

fn exercise_cpu_preset_dialog(app: &mut WinderustApp, index: usize) {
    use crate::{config::CpuAllocationPreset, cpu_topology::CpuSelection};
    use cpu_allocation::{Kind, Message as PresetMessage, SelectionCatalog};

    let kind = if matches!(index, 73 | 76) {
        Kind::Soft
    } else {
        Kind::Hard
    };
    let selection = SelectionCatalog::capture(kind)
        .all_selection()
        .expect("render-smoke requires a readable CPU topology");
    if matches!(index, 73 | 74) {
        let id = selection.processors()[0];
        let topology = crate::cpu_allocation::processor_topology()
            .expect("render-smoke requires the live CPU topology");
        let added = app.settings.global().cpu_allocation_presets.len();
        let persisted = app.settings.persisted().cpu_allocation_presets.clone();
        for message in [
            PresetMessage::NewPreset,
            PresetMessage::PresetName("Processor toggle test".into()),
            PresetMessage::PresetSelection(CpuSelection::default()),
            PresetMessage::TogglePresetProcessor(id, Arc::new(topology)),
            PresetMessage::SavePreset,
        ] {
            let _ = app.update(Message::Allocation(kind, message));
        }
        let presets = &app.settings.global().cpu_allocation_presets;
        assert_eq!(presets.len(), added + 1);
        assert_eq!(presets[added].selection.processors(), &[id]);
        assert_eq!(app.settings.persisted().cpu_allocation_presets, persisted);
        let _ = app.update(Message::Allocation(
            kind,
            PresetMessage::DeletePreset(added),
        ));
    }
    let message = if matches!(index, 75 | 76) {
        let preset = CpuAllocationPreset {
            name: "Preset to review".into(),
            selection: CpuSelection::new(selection.processors().to_vec(), None)
                .expect("an unbound selection preserves the saved logical processors"),
        };
        let preset_index = app.settings.global().cpu_allocation_presets.len();
        app.settings.edit_global(|settings| {
            settings.cpu_allocation_presets.push(preset);
            if index == 76 {
                settings.cpu_allocation_presets.push(CpuAllocationPreset {
                    name: "Example preset".into(),
                    selection,
                });
            }
        });
        app.settings = SettingsEditor::with_settings((*app.settings).clone());
        PresetMessage::EditPreset(preset_index)
    } else {
        PresetMessage::ViewPreset("Example preset".into(), selection)
    };
    let before = app.settings.global().cpu_allocation_presets.clone();
    let _ = app.update(Message::Allocation(kind, message));
    if index == 76 {
        let _ = app.update(Message::Allocation(
            kind,
            PresetMessage::PresetName("Example preset".into()),
        ));
    }
    let editor = match kind {
        Kind::Soft => &app.soft_allocation,
        Kind::Hard => &app.hard_allocation,
    };
    assert!(editor.preset_open());
    assert_eq!(app.settings.global().cpu_allocation_presets, before);
}

fn exercise_settings_export(app: &mut WinderustApp) {
    let saved = app.settings.persisted().clone();
    let start_minimized = !saved.general.start_minimized;
    let _ = app.update(Message::Preferences(settings_pages::Message::Flag(
        settings_pages::Flag::Minimized,
        start_minimized,
    )));
    let draft = app.settings.global().clone();
    assert!(app.settings.has_unsaved_changes());
    let path = std::path::PathBuf::from("target/iced-smoke/settings-draft.toml");
    let _ = app.update(Message::SettingsFileChosen(
        FileDialogMode::Save,
        Some(path.clone()),
    ));
    let contents = std::fs::read_to_string(&path).expect("read the scratch export");
    let exported: crate::config::Settings = toml::from_str(&contents).expect("parse the export");
    assert_eq!(exported.general.start_minimized, start_minimized);
    let _ = app.update(Message::SettingsFileChosen(FileDialogMode::Open, None));
    assert_eq!(app.settings.global(), &draft);
    assert_eq!(app.settings.persisted(), &saved);
}

fn exercise_adaptive_processor_power_presets(app: &mut WinderustApp) {
    use adaptive_processor_power::{BuiltInPreset, Message as PowerMessage};

    let mut settings = crate::config::Settings::default();
    settings.general = app.settings.global().general.clone();
    settings.general.enabled = false;
    app.settings = SettingsEditor::with_settings(settings);
    app.power_source = PowerSourceProfile::PluggedIn;
    app.reset_editors();

    let runtime_before = app.settings.runtime_settings_snapshot().value;
    let _ = app.update(Message::PowerSource(PowerSourceProfile::OnBattery));
    assert_eq!(app.power_source, PowerSourceProfile::OnBattery);
    let _ = app.update(Message::AdaptiveProcessorPower(
        PowerMessage::PreviewBuiltIn(BuiltInPreset::Balanced),
    ));
    let _ = app.update(Message::AdaptiveProcessorPower(PowerMessage::UsePreset));
    let battery = app.settings.adaptive_engine.clone();

    let _ = app.update(Message::AdaptiveProcessorPower(PowerMessage::NewPreset));
    assert!(app.adaptive_processor_power.preset_open());
    let _ = app.update(Message::AdaptiveProcessorPower(PowerMessage::PresetName(
        "Canceled preset".into(),
    )));
    let _ = app.search_keyboard(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
        iced::keyboard::Modifiers::empty(),
        false,
        iced::event::Status::Ignored,
    );
    assert!(!app.adaptive_processor_power.preset_open());
    assert!(app
        .settings
        .global()
        .adaptive_processor_power_presets
        .is_empty());

    for message in [
        PowerMessage::NewPreset,
        PowerMessage::PresetName("Quiet battery".into()),
        PowerMessage::SavePreset,
    ] {
        let _ = app.update(Message::AdaptiveProcessorPower(message));
    }
    assert!(!app.adaptive_processor_power.preset_open());
    let presets = &app.settings.global().adaptive_processor_power_presets;
    assert_eq!(presets.len(), 1);
    let preset = presets[0].clone();
    assert_eq!(preset.name, "Quiet battery");
    assert!(preset.matches(&battery, PowerSourceProfile::OnBattery));

    let _ = app.update(Message::PowerSource(PowerSourceProfile::PluggedIn));
    assert_eq!(app.power_source, PowerSourceProfile::PluggedIn);
    let _ = app.update(Message::AdaptiveProcessorPower(
        PowerMessage::PreviewCustom(0),
    ));
    let _ = app.update(Message::AdaptiveProcessorPower(PowerMessage::UsePreset));
    assert!(preset.matches(&app.settings.adaptive_engine, PowerSourceProfile::PluggedIn));
    assert!(
        !app.settings
            .adaptive_engine
            .adaptive_processor_power_enabled
    );
    assert_eq!(
        app.settings.global().battery_profile().adaptive_engine,
        battery
    );
    assert!(app
        .settings
        .persisted()
        .adaptive_processor_power_presets
        .is_empty());
    assert_eq!(
        app.settings.runtime_settings_snapshot().value,
        runtime_before
    );
    assert!(app.pending_settings_changes());
}

pub(super) fn captured(app: &mut WinderustApp, shot: iced::window::Screenshot) -> Task<Message> {
    let run = app
        .smoke
        .as_mut()
        .expect("smoke screenshot belongs to a smoke run");
    assert!(shot.size.width >= 900 && shot.size.height >= 620);
    let path = format!("target/iced-smoke/{:02}-{:?}.png", run.index, app.page);
    image::save_buffer(
        path,
        &shot.rgba,
        shot.size.width,
        shot.size.height,
        image::ColorType::Rgba8,
    )
    .expect("save rendered smoke evidence");
    run.index = run
        .remaining
        .pop_front()
        .unwrap_or(run.pages.len() * 2 + EXTRA_STATES);
    run.ticks = 0;
    if let Some(settings) = run.navigation_settings.take() {
        app.settings = settings;
    }
    if app.navigation_search_open {
        return app.update(Message::CloseNavigationSearch);
    }
    if app.page == Page::CpuLimiter && app.cpu_limiter.has_pending_editor() {
        let cancel = app.update(Message::CpuLimiter(cpu_limiter::Message::CancelRule));
        assert!(!app.cpu_limiter.has_pending_editor());
        return cancel;
    }
    Task::none()
}

pub(crate) fn render_all_pages() {
    use crate::config::{
        ByFocusRule, ByRunningAppRule, ExecutableSelector, ResourceThresholdsRule,
        ResourceThresholdsSelector,
    };

    std::fs::create_dir_all("target/iced-smoke").expect("create smoke output");
    let mut settings = crate::config::Settings::default();
    settings.general.enabled = false;
    settings.general.check_for_updates = false;
    settings.general.hide_to_tray = false;
    settings.general.start_minimized = false;
    settings.general.startup_with_windows = false;
    settings.advanced.show_advanced_controls = true;
    let name = "sample.exe";
    let path = r"C:\Apps\Resource Monitor\sample.exe";
    let selectors = [
        ExecutableSelector::ExactPath {
            executable_path: path.into(),
        },
        ExecutableSelector::ExactName {
            executable_name: name.into(),
        },
    ];
    settings.by_focus.rules = selectors
        .iter()
        .cloned()
        .map(|selector| ByFocusRule {
            enabled: true,
            name: name.into(),
            selector,
            power_plan_guid: None,
        })
        .collect();
    settings.by_running_app.rules = selectors
        .iter()
        .cloned()
        .map(|selector| ByRunningAppRule {
            enabled: true,
            name: name.into(),
            selector,
            power_plan_guid: None,
        })
        .collect();
    settings.resource_thresholds.rules = vec![
        ResourceThresholdsRule::new(path.into()),
        ResourceThresholdsRule::with_selector(ResourceThresholdsSelector::ExactName {
            executable_name: name.into(),
        }),
    ];
    let mut editor = SettingsEditor::with_settings(settings);
    let recovery = Arc::new(Mutex::new(crate::crash_recovery::RecoveryClient::start()));
    let runtime = RuntimeHandle::start(&editor.runtime_settings_snapshot());
    let startup_warnings = crate::crash_recovery::startup_error().into_iter().collect();
    REQUESTED.store(true, Ordering::Relaxed);
    let result = run(editor, startup_warnings, runtime, None, recovery);
    REQUESTED.store(false, Ordering::Relaxed);
    result.expect("all page windows rendered");
    assert_eq!(
        std::fs::read_dir("target/iced-smoke")
            .expect("read smoke output")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
            .count(),
        Run::scenarios(&Run::pages()).len()
    );
}
