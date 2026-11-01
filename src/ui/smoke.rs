//! Opt-in native render check; compiled only with the render-smoke feature.
use super::*;
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
    if extra == Some(51) {
        return Some(app.shutdown());
    }
    if run.ticks == 0 {
        let chinese = extra.map_or(run.index >= run.pages.len(), |index| {
            matches!(index, 1 | 5 | 7 | 9 | 11 | 36 | 40 | 42 | 45)
        });
        let page = match extra {
            Some(0) => Page::BackgroundEfficiency,
            Some(1) => Page::CpuLimiter,
            Some(2) => Page::SettingsHome,
            Some(3) => Page::Home,
            Some(4..=10) => Page::AdaptiveEngine,
            Some(11) => Page::MemoryTrim,
            Some(12) => Page::ByForeground,
            Some(13) => Page::Home,
            Some(14) => Page::ProcessList,
            Some(15) => Page::PowerPlanControl,
            Some(16) => Page::ActionLog,
            Some(17 | 24) => Page::AdaptiveEngine,
            Some(18) => Page::BackgroundEfficiency,
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
        if extra.is_some_and(|i| i >= 38) {
            let _ = app.update(Message::Preferences(settings_pages::Message::Theme(
                crate::config::AppThemeMode::Amoled,
            )));
            assert_eq!(app.appearance.palette().background, iced::Color::BLACK);
        }
        // Exercise disclosure messages and long/compact navigation without enabling automation.
        let was_open = app.side_panel_open;
        let navigate = app.update(Message::Page(page));
        assert_eq!(app.side_panel_tab, SidePanelTab::for_page(page)[0]);
        assert_eq!(app.side_panel_open, was_open);
        let _ = app.update(Message::CloseSidePanel);
        if !navigation::page_feature_info(page).is_empty() {
            assert!(app.side_panel(page).is_some());
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
            let _ = app.update(Message::Efficiency(
                background_efficiency::Message::Collapse(priority_control::Tier::Focus),
            ));
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
                } else if index == 5 || index == 8 {
                    if index == 8 {
                        let _ = app.update(Message::Adaptive(AdaptiveMessage::New));
                    }
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(
                        TuningTab::ProcessorPower,
                    )));
                } else if index == 6 || index == 9 {
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(
                        TuningTab::PriorityControl,
                    )));
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
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::ViewBuiltIn(
                        adaptive_engine::BuiltInAdaptiveEnginePreset::Balanced,
                    )));
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
                app.settings.by_foreground.rules.push(
                    crate::ui::process_rules::new_foreground_rule(
                        r"C:\Apps\Example\example.exe",
                        None,
                    ),
                );
            }
        }
        if extra == Some(9) {
            let before = app.side_panel_open;
            let _ = app.update(Message::ToggleSidePanel);
            assert_eq!(app.side_panel_open, !before);
        }
        if extra == Some(18) {
            app.settings.background_efficiency.enabled = true;
        }
        if extra == Some(19) {
            app.settings.by_activity.enabled = true;
        }
        if extra == Some(21) {
            app.settings.app_suspension.enabled = true;
        }
        if extra == Some(24) {
            let _ = app.update(Message::Adaptive(adaptive_engine::Message::TuningTab(
                adaptive_engine::TuningTab::PriorityControl,
            )));
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
                assert!(app.side_panel(page).is_some());
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
            assert!(app.side_panel(page).is_some());
            let _ = app.update(Message::ToggleSidePanel);
            assert!(app.side_panel_open);
            app.settings = SettingsEditor::with_settings((*app.settings).clone());
        }
        if matches!(extra, Some(47..=50)) {
            let _ = app.update(Message::ToggleSidePanel);
            assert!(app.side_panel_open);
            app.settings = SettingsEditor::with_settings((*app.settings).clone());
        }
        let size = match extra {
            Some(35..=37 | 40 | 42 | 44..=50) => iced::Size::new(900.0, 620.0),
            Some(30) => iced::Size::new(1280.0, 720.0),
            Some(31) => iced::Size::new(1440.0, 900.0),
            Some(32) => iced::Size::new(1920.0, 720.0),
            Some(33) => iced::Size::new(900.0, 1000.0),
            Some(34) => iced::Size::new(1536.0, 960.0),
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
    if run.ticks
        == if extra.is_some_and(|i| i >= 13) {
            12
        } else {
            4
        }
    {
        Some(iced::window::screenshot(window).map(Message::SmokeScreenshot))
    } else {
        Some(Task::none())
    }
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
    REQUESTED.store(true, Ordering::Relaxed);
    let result = run(editor, None, runtime, None, recovery);
    REQUESTED.store(false, Ordering::Relaxed);
    result.expect("all page windows rendered");
    assert_eq!(
        std::fs::read_dir("target/iced-smoke")
            .expect("read smoke output")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
            .count(),
        119
    );
}
