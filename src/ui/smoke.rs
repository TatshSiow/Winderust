//! Opt-in native render check; compiled only with the render-smoke feature.
use super::*;
use crate::backend::power_source::is_plugged_in;
use std::sync::atomic::{AtomicBool, Ordering};

static REQUESTED: AtomicBool = AtomicBool::new(false);
pub(super) struct Run {
    pages: Vec<Page>,
    index: usize,
    ticks: u8,
}
impl Run {
    pub(super) fn requested() -> Option<Self> {
        REQUESTED.load(Ordering::Relaxed).then(|| {
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
            Self {
                pages,
                index: 0,
                ticks: 0,
            }
        })
    }
}
pub(super) fn advance(app: &mut WinderustApp) -> Option<Task<Message>> {
    let window = app.window?;
    let run = app.smoke.as_mut()?;
    let extra = run.index.checked_sub(run.pages.len() * 2);
    if extra == Some(72) {
        return Some(app.shutdown());
    }
    if run.ticks == 0 {
        let chinese = extra.map_or(run.index >= run.pages.len(), |index| {
            matches!(
                index,
                1 | 5
                    | 7
                    | 9
                    | 11
                    | 36
                    | 40
                    | 42
                    | 45
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
            )
        });
        let page = match extra {
            Some(0) => Page::EfficiencyMode,
            Some(1) => Page::CpuLimiter,
            Some(2) => Page::SettingsHome,
            Some(3) => Page::Home,
            Some(4 | 6..=10) => Page::AdaptiveEngine,
            Some(5 | 70 | 71) => Page::AdaptiveProcessorPower,
            Some(11) => Page::MemoryTrim,
            Some(12) => Page::ByFocus,
            Some(13) => Page::Home,
            Some(14) => Page::ProcessList,
            Some(15) => Page::PowerPlanControl,
            Some(16) => Page::ActionLog,
            Some(17 | 24) => Page::AdaptiveEngine,
            Some(18) => Page::EfficiencyMode,
            Some(19) => Page::ByActivity,
            Some(20) => Page::AdvancedPowerPlanTuning,
            Some(21) => Page::AppSuspension,
            Some(22) => Page::LanguageAndAppearance,
            Some(23) => Page::About,
            Some(25) => Page::ProcessPriority,
            Some(26..=37) => Page::Home,
            Some(38 | 42 | 43) => Page::Home,
            Some(39) => Page::ProcessList,
            Some(40) => Page::LanguageAndAppearance,
            Some(41) => Page::AdaptiveEngine,
            Some(44) => Page::CpuSetsSoft,
            Some(45) => Page::ProcessorAffinityHard,
            Some(46) => Page::AdvancedPowerPlanTuning,
            Some(47) => Page::AdaptiveEngine,
            Some(48) => Page::ProcessList,
            Some(49) => Page::ActionLog,
            Some(50) => Page::WinderustBehaviour,
            Some(51..=53) => Page::Watchdog,
            Some(54..=57) => Page::ByCpuLoad,
            Some(58..=59) => Page::SleepPrevention,
            Some(60..=61) => Page::KeepRunning,
            Some(62..=65) => Page::InstanceLimits,
            Some(66..=69) => Page::InstanceBalancing,
            _ => run.pages[run.index % run.pages.len()],
        };
        run.ticks = 1;
        app.settings.general.language = if chinese {
            crate::config::AppLanguage::ZhTw
        } else {
            crate::config::AppLanguage::English
        };
        app.settings.general.theme_mode =
            if chinese || extra.is_some_and(|i| (14..=34).contains(&i)) {
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
        if extra.is_some_and(|i| (38..=53).contains(&i)) {
            let _ = app.update(Message::Preferences(settings_pages::Message::Theme(
                crate::config::AppThemeMode::Amoled,
            )));
            assert_eq!(app.appearance.palette().background, iced::Color::BLACK);
        }
        // Exercise disclosure messages and long/compact navigation without enabling automation.
        if matches!(extra, Some(62..=65)) {
            use crate::{
                control::instance_limits::CountObservation,
                features::advanced_controls::instance_limits::{
                    InstanceLimitsRuleSnapshot, NotificationStatus,
                },
            };
            let paths = [
                r"C:\Apps\Long Application Name\renderer.exe",
                r"C:\Apps\editor.exe",
                r"C:\Apps\worker.exe",
            ];
            app.settings.edit_global(|settings| {
                settings.instance_limits.enabled = true;
                settings.instance_limits.rules = paths
                    .iter()
                    .map(|path| crate::config::InstanceLimitsRule::new((*path).into()))
                    .collect();
            });
            let status = &mut Arc::make_mut(&mut app.status.feature_status).instance_limits;
            status.enabled = true;
            status.rules = paths
                .iter()
                .enumerate()
                .map(|(index, path)| InstanceLimitsRuleSnapshot {
                    executable_path: (*path).into(),
                    max_instances: 1,
                    enabled: true,
                    count: match index {
                        0 => CountObservation::Complete(3),
                        1 => CountObservation::Incomplete {
                            known_lower_bound: 2,
                            reason: "A matching process could not be queried.".into(),
                        },
                        _ => CountObservation::Incomplete {
                            known_lower_bound: 0,
                            reason: "Process scope could not be verified.".into(),
                        },
                    },
                    sampled_at: Some(std::time::Instant::now()),
                    incident: index == 0,
                    notification: if index == 0 {
                        NotificationStatus::Failed(
                            "Windows rejected the notification request.".into(),
                        )
                    } else {
                        NotificationStatus::NotAttempted
                    },
                })
                .collect();
            if matches!(extra, Some(64..=65)) {
                let _ = app.update(Message::InstanceLimits(instance_limits::Message::Edit(0)));
            }
        }
        if matches!(extra, Some(60..=61)) {
            use crate::features::advanced_controls::keep_running::{
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
            let mut rule =
                crate::config::WatchdogRule::new(r"C:\Apps\Resource Monitor\sample.exe".into());
            rule.memory.enabled = true;
            rule.memory_unit = crate::config::WatchdogMemoryUnit::Megabytes;
            rule.memory.warning.threshold = 1000;
            rule.memory.critical.threshold = 2000;
            rule.memory.critical.action = crate::config::WatchdogAction::Terminate;
            app.settings
                .edit_global(|settings| settings.watchdog.rules = vec![rule]);
            app.watchdog = watchdog::Editor::default();
            let _ = app.update(Message::Watchdog(watchdog::Message::Edit(0)));
        }
        if matches!(extra, Some(54..=57)) {
            use crate::config::{
                ByCpuLoadRule, ByCpuLoadStability, CpuLoadPercent, CpuUsageComparison,
            };
            let _ = app.update(Message::Watchdog(watchdog::Message::CancelRule));
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
        let was_open = app.side_panel_open;
        let navigate = app.update(Message::Page(page));
        if matches!(extra, Some(66..=69)) {
            use crate::automation::InstanceBalancingDetail;

            app.reset_editors();
            app.settings.edit_global(|settings| {
                settings.general.enabled = true;
                settings.instance_balancing.enabled = true;
                settings.instance_balancing.rules = [
                    r"C:\Apps\Long Application Name\renderer.exe",
                    r"C:\Apps\editor.exe",
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
            status.enabled = true;
            status.scanned_processes = 4;
            status.adjusted_processes = if extra == Some(66) { 4 } else { 0 };
            status.detail = Some(if extra == Some(66) {
                InstanceBalancingDetail::Assigned(4)
            } else {
                InstanceBalancingDetail::WaitingForStableCohort
            });
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
            let before = app.settings.global().watchdog.clone();
            assert!(app.pending_editor());
            let _ = app.update(Message::Watchdog(watchdog::Message::Threshold(
                crate::config::WatchdogResource::Cpu,
                false,
                String::new(),
            )));
            let _ = app.search_keyboard(
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                iced::keyboard::Modifiers::empty(),
                false,
                iced::event::Status::Ignored,
            );
            assert!(!app.pending_editor());
            assert_eq!(app.settings.global().watchdog, before);
            let _ = app.update(Message::Watchdog(watchdog::Message::Edit(0)));
            if extra == Some(51) {
                let _ = app.update(Message::Watchdog(watchdog::Message::CancelRule));
            } else if extra == Some(52) {
                let _ = app.update(Message::Watchdog(watchdog::Message::MonitorEnabled(
                    crate::config::WatchdogResource::Cpu,
                    false,
                )));
            } else if extra == Some(53) {
                let _ = app.update(Message::Watchdog(watchdog::Message::Tab(
                    crate::config::WatchdogResource::Memory,
                )));
            }
        }
        assert_eq!(app.side_panel_tab, SidePanelTab::for_page(page)[0]);
        assert_eq!(app.side_panel_open, was_open);
        let _ = app.update(Message::CloseSidePanel);
        if !navigation::page_feature_info(page).is_empty() {
            assert!(app.side_panel(is_plugged_in).is_some());
        }
        if matches!(extra, Some(5 | 70 | 71)) {
            assert!(SidePanelTab::for_page(page).contains(&SidePanelTab::Presets));
            let _ = app.update(Message::ToggleSidePanel);
            let _ = app.update(Message::SidePanelTab(SidePanelTab::Presets));
            assert!(app.side_panel_open);
            assert_eq!(app.side_panel_tab, SidePanelTab::Presets);
            assert!(app.side_panel(is_plugged_in).is_some());
        }
        if extra == Some(0) {
            let section = Page::SettingsHome;
            let _ = app.update(Message::Navigation(navigation::Event::Select(section)));
            assert_eq!(app.page, section);
            assert_eq!(app.navigation.section, None);
            let _ = app.update(Message::Page(page));
            let _ = app.update(Message::Navigation(navigation::Event::OpenKeyboard(
                section,
            )));
            assert_eq!(app.page, page);
            assert_eq!(app.navigation.section, Some(section));
            let _ = app.update(Message::Navigation(navigation::Event::Dismiss));
            assert_eq!(app.navigation.section, None);
            let _ = app.update(Message::Navigation(navigation::Event::OpenKeyboard(
                Page::PowerPlanControl,
            )));
            assert_eq!(app.navigation.section, Some(Page::PowerPlanControl));
            let _ = app.update(Message::Page(page));
            assert_eq!(app.navigation.section, None);
            let _ = app.update(Message::OpenNavigationSearch);
            assert!(app.navigation_search_open);
            let _ = app.update(Message::NavigationSearch("cpu".into()));
            let _ = app.update(Message::CloseNavigationSearch);
            assert!(!app.navigation_search_open);
            assert!(app.navigation_search.is_empty());
            let _ = app.update(Message::OpenNavigationSearch);
            let _ = app.update(Message::Page(page));
            assert!(!app.navigation_search_open);
            assert!(app.navigation_search.is_empty());
        }
        if extra == Some(0) {
            let _ = app.update(Message::EfficiencyMode(efficiency_mode::Message::Collapse(
                priority_control::Tier::Focus,
            )));
        } else if extra == Some(1) {
            let _ = app.update(Message::CpuLimiter(cpu_limiter::Message::Collapse));
        }
        if let Some(index @ 4..=12) = extra {
            if index <= 10 {
                use adaptive_engine::{Message as AdaptiveMessage, TuningTab};
                if index == 4 {
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(
                        TuningTab::CpuBehaviour,
                    )));
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::Collapse(0)));
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
                    assert!(app.pending_changes());
                    let _ = app.update(Message::AdaptiveProcessorPower(PowerMessage::Tab(
                        Tab::DemandProfiles,
                    )));
                } else if index == 8 {
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::New));
                } else if index == 6 || index == 9 {
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(
                        TuningTab::CpuBehaviour,
                    )));
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::Collapse(1)));
                } else if index == 7 {
                    app.settings.adaptive_engine_process.custom_rules.push(
                        crate::config::ProcessExclusionRule {
                            executable_path: r"C:\Apps\Example\example.exe".into(),
                            enabled: false,
                            ..Default::default()
                        },
                    );
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(
                        TuningTab::CustomRules,
                    )));
                } else {
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
            } else if index == 11 {
                app.settings
                    .memory_trim
                    .exclusions
                    .push(crate::config::ProcessExclusionRule {
                        executable_path: r"C:\Apps\Example\example.exe".into(),
                        enabled: false,
                        ..Default::default()
                    });
                let _ = app.update(Message::Trim(memory_trim::Message::Collapse(0)));
            } else {
                app.settings
                    .by_focus
                    .rules
                    .push(crate::ui::process_rules::new_by_focus_rule(
                        r"C:\Apps\Example\example.exe",
                        None,
                    ));
            }
        }
        if extra == Some(9) {
            let before = app.side_panel_open;
            let _ = app.update(Message::ToggleSidePanel);
            assert_eq!(app.side_panel_open, !before);
        }
        if extra == Some(20) {
            for source in [
                advanced_power_plan_tuning::Source::Ac,
                advanced_power_plan_tuning::Source::Battery,
            ] {
                let _ = app.update(Message::PowerTuning(
                    advanced_power_plan_tuning::Message::Group(source),
                ));
            }
        }
        if extra == Some(18) {
            app.settings.efficiency_mode.enabled = true;
        }
        if extra == Some(19) {
            app.settings.by_activity.enabled = true;
        }
        if extra == Some(21) {
            app.settings.app_suspension.enabled = true;
        }
        if extra == Some(24) {
            let _ = app.update(Message::Adaptive(adaptive_engine::Message::TuningTab(
                adaptive_engine::TuningTab::CpuBehaviour,
            )));
            let _ = app.update(Message::Adaptive(adaptive_engine::Message::Collapse(2)));
        }
        if extra == Some(17) {
            app.adaptive = adaptive_engine::Editor::default();
        }
        if matches!(extra, Some(13 | 29..=34 | 38 | 42 | 43)) {
            app.home.populate_smoke_history();
            app.settings.general.enabled = true;
            app.settings.cpu_limiter.enabled = true;
            app.settings.process_priority.enabled = true;
            app.settings.memory_trim.enabled = true;
            app.settings.io_priority.enabled = true;
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
            app.settings = SettingsEditor::with_settings((*app.settings).clone());
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
                let _ = app.update(Message::SearchKeyboard {
                    key: iced::keyboard::Key::Character("k".into()),
                    modifiers: iced::keyboard::Modifiers::CTRL,
                    repeat: false,
                    status: iced::event::Status::Ignored,
                });
                assert!(app.navigation_search_open);
            }
            let open = app.update(Message::OpenNavigationSearch);
            if extra != Some(26) {
                let _ = app.update(Message::NavigationSearch(
                    if matches!(extra, Some(27 | 43)) {
                        "cpu"
                    } else {
                        "zzzz-no-match"
                    }
                    .into(),
                ));
            }
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
                let _ = app.update(Message::Page(Page::Home));
                let _ = app.update(Message::OpenNavigationSearch);
                let _ = app.update(Message::NavigationSearch("cpu".into()));
                let _ = app.update(down());
            }
            open
        } else {
            Task::none()
        };
        let navigation = if matches!(extra, Some(35..=37 | 42)) {
            let section = if extra == Some(36) {
                Page::AdvancedControls
            } else {
                Page::SettingsHome
            };
            let _ = app.update(Message::Navigation(navigation::Event::OpenKeyboard(
                section,
            )));
            assert_eq!(app.page, page);
            assert_eq!(app.navigation.selected, Some(0));
            let _ = app.update(Message::Navigation(navigation::Event::Key(
                iced::keyboard::Key::Named(iced::keyboard::key::Named::End),
                false,
            )));
            assert_eq!(
                app.navigation.selected,
                Some(navigation::children(section).len() - 1)
            );
            let _ = app.update(Message::Navigation(navigation::Event::Key(
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
                false,
            )));
            assert_eq!(app.page, *navigation::children(section).last().unwrap());
            assert_eq!(app.navigation.section, None);
            let _ = app.update(Message::Page(page));
            let _ = app.update(Message::Navigation(navigation::Event::OpenKeyboard(
                section,
            )));
            let _ = app.update(Message::Navigation(navigation::Event::Key(
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                false,
            )));
            assert_eq!(app.page, page);
            assert_eq!(app.navigation.section, None);
            app.update(Message::Navigation(navigation::Event::OpenKeyboard(
                section,
            )))
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
            assert_eq!(app.side_panel_tab, SidePanelTab::for_page(page)[0]);
            let _ = app.update(Message::SidePanelTab(SidePanelTab::Info));
            assert_eq!(app.side_panel_tab, SidePanelTab::Info);
            let _ = app.update(Message::ToggleSidePanel);
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
            let _ = app.update(Message::Navigation(navigation::Event::Dismiss));
            assert!(app.side_panel_open);
            let _ = app.update(Message::CloseSidePanel);
            assert!(!app.side_panel_open);
            let _ = app.update(Message::ToggleSidePanel);
            app.settings = SettingsEditor::with_settings((*app.settings).clone());
        }
        if matches!(extra, Some(44..=46)) {
            let tab = if extra == Some(46) {
                SidePanelTab::Info
            } else {
                SidePanelTab::Presets
            };
            let _ = app.update(Message::SidePanelTab(tab));
            assert_eq!(app.side_panel_tab, tab);
            assert!(app.side_panel(is_plugged_in).is_some());
            let _ = app.update(Message::ToggleSidePanel);
            assert!(app.side_panel_open);
            app.settings = SettingsEditor::with_settings((*app.settings).clone());
        }
        if matches!(extra, Some(47..=50)) {
            let _ = app.update(Message::ToggleSidePanel);
            assert!(app.side_panel_open);
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
        let size = match extra {
            Some(35..=37 | 40 | 42 | 44..=50 | 52..=69 | 71) => iced::Size::new(900.0, 620.0),
            Some(30) => iced::Size::new(1280.0, 720.0),
            Some(31) => iced::Size::new(1440.0, 900.0),
            Some(32) => iced::Size::new(1920.0, 720.0),
            Some(33) => iced::Size::new(900.0, 1000.0),
            Some(34) => iced::Size::new(1536.0, 960.0),
            Some(70) => iced::Size::new(1120.0, 920.0),
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
            search,
            navigation,
        ]));
    }
    run.ticks += 1;
    if extra == Some(53) && run.ticks == 3 {
        return Some(iced::widget::operation::snap_to(
            "watchdog-rule-details",
            iced::widget::scrollable::RelativeOffset { x: 0.0, y: 1.0 },
        ));
    }
    if matches!(extra, Some(56 | 57)) && run.ticks == 3 {
        return Some(iced::widget::operation::snap_to(
            "by-cpu-load-rule-details",
            iced::widget::scrollable::RelativeOffset { x: 0.0, y: 1.0 },
        ));
    }
    if run.ticks
        == if extra.is_some_and(|i| i >= 13) {
            12
        } else {
            4
        }
    {
        if matches!(extra, Some(62..=65)) {
            for rule in &mut Arc::make_mut(&mut app.status.feature_status)
                .instance_limits
                .rules
            {
                rule.sampled_at = Some(std::time::Instant::now());
            }
        }
        Some(iced::window::screenshot(window).map(Message::SmokeScreenshot))
    } else {
        Some(Task::none())
    }
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
    assert!(app.pending_changes());
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
    run.index += 1;
    run.ticks = 0;
    Task::none()
}

pub(crate) fn render_all_pages() {
    std::fs::create_dir_all("target/iced-smoke").expect("create smoke output");
    let mut settings = crate::config::Settings::default();
    settings.general.enabled = false;
    settings.general.check_for_updates = false;
    settings.general.hide_to_tray = false;
    settings.general.start_minimized = false;
    settings.general.startup_with_windows = false;
    settings.advanced.show_advanced_controls = true;
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
        152
    );
}
