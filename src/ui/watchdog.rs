use super::{app_picker, design, widgets};
use crate::config::{
    WatchdogAction, WatchdogMemoryUnit, WatchdogMetric, WatchdogResource, WatchdogRule,
    WatchdogSettings,
};
use iced::{
    widget::{column, row, text},
    Element, Fill,
};
use rust_i18n::t;

impl std::fmt::Display for WatchdogMetric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(match self {
            Self::CpuPercent => "watchdog.cpu",
            Self::MemoryPercent => "watchdog.memory_percent",
            Self::MemoryMib => "watchdog.memory_mib",
        }))
    }
}
impl std::fmt::Display for WatchdogMemoryUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(match self {
            Self::Percent => "watchdog.memory_percent",
            Self::Mib => "watchdog.memory_mib",
        }))
    }
}

impl std::fmt::Display for WatchdogAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(match self {
            Self::None => "watchdog.no_action",
            Self::Terminate => "watchdog.terminate",
            Self::Restart => "watchdog.restart",
        }))
    }
}

#[derive(Default)]
pub(super) struct Editor {
    path: String,
    duration: Option<String>,
    draft: Option<Draft>,
}

struct Draft {
    index: Option<usize>,
    rule: WatchdogRule,
    tab: WatchdogResource,
    monitors: [ThresholdInputs; 2],
}

struct ThresholdInputs {
    warning: String,
    critical: String,
}

impl Draft {
    fn new(index: Option<usize>, rule: WatchdogRule) -> Self {
        Self {
            index,
            tab: if rule.cpu.enabled {
                WatchdogResource::Cpu
            } else {
                WatchdogResource::Memory
            },
            monitors: WatchdogResource::ALL.map(|resource| {
                let monitor = rule.monitor(resource);
                ThresholdInputs {
                    warning: monitor.warning.threshold.to_string(),
                    critical: monitor.critical.threshold.to_string(),
                }
            }),
            rule,
        }
    }

    fn validated_rule(&self) -> Option<WatchdogRule> {
        let mut rule = self.rule.clone();
        for (resource, raw) in WatchdogResource::ALL.into_iter().zip(&self.monitors) {
            let monitor = rule.monitor_mut(resource);
            if monitor.enabled {
                monitor.warning.threshold = raw.warning.parse().ok()?;
                monitor.critical.threshold = raw.critical.parse().ok()?;
            }
        }
        rule.validate().ok()?;
        Some(rule)
    }
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Path(String),
    Browse,
    Add,
    Remove(usize),
    Edit(usize),
    SaveRule,
    CancelRule,
    RuleEnabled(usize, bool),
    Tab(WatchdogResource),
    MonitorEnabled(WatchdogResource, bool),
    MemoryUnit(WatchdogMemoryUnit),
    Duration(String),
    Threshold(WatchdogResource, bool, String),
    Notify(bool),
    Action(WatchdogResource, bool, WatchdogAction),
}

impl Editor {
    fn can_add(&self, settings: &WatchdogSettings, path: &str) -> bool {
        super::process_rules::can_add_process_candidate(
            path,
            |path| {
                settings.rules.iter().any(|rule| {
                    crate::foreground::same_executable_path(
                        std::path::Path::new(&rule.executable_path),
                        std::path::Path::new(path),
                    )
                })
            },
            |name| name.eq_ignore_ascii_case("winderust.exe"),
        )
    }

    pub(super) fn has_pending_editor(&self) -> bool {
        self.draft.is_some()
    }

    pub(super) fn has_invalid_inputs(&self) -> bool {
        self.duration
            .as_deref()
            .is_some_and(|value| parse_duration(value).is_none())
    }

    pub(super) fn select_tab(&mut self, resource: WatchdogResource) {
        if let Some(draft) = &mut self.draft {
            draft.tab = resource;
        }
    }

    pub(super) fn update(&mut self, settings: &mut WatchdogSettings, message: Message) {
        match message {
            Message::Enabled(value) => settings.enabled = value,
            Message::Path(value) => self.path = value,
            Message::Add if self.can_add(settings, &self.path) => {
                self.draft = Some(Draft::new(
                    None,
                    WatchdogRule::new(crate::foreground::executable_path_key(
                        std::path::Path::new(self.path.trim()),
                    )),
                ));
            }
            Message::Edit(index) => {
                self.draft = settings
                    .rules
                    .get(index)
                    .cloned()
                    .map(|rule| Draft::new(Some(index), rule));
            }
            Message::SaveRule => {
                if let Some(draft) = &self.draft {
                    if let Some(rule) = draft.validated_rule() {
                        if let Some(index) = draft.index {
                            if let Some(target) = settings.rules.get_mut(index) {
                                *target = rule;
                            }
                        } else {
                            settings.rules.push(rule);
                            self.path.clear();
                        }
                        self.draft = None;
                    }
                }
            }
            Message::CancelRule => self.draft = None,
            Message::Tab(resource) => self.select_tab(resource),
            Message::Remove(index) if index < settings.rules.len() => {
                settings.rules.remove(index);
                self.draft = None;
            }
            Message::RuleEnabled(index, value) => {
                if let Some(rule) = settings.rules.get_mut(index) {
                    rule.enabled = value;
                }
            }
            Message::MonitorEnabled(resource, enabled) => {
                if let Some(draft) = &mut self.draft {
                    draft.rule.monitor_mut(resource).enabled = enabled;
                }
            }
            Message::MemoryUnit(unit) => {
                if let Some(draft) = &mut self.draft {
                    if draft.rule.memory_unit != unit {
                        draft.rule.memory_unit = unit;
                        let (warning, critical) = if unit == WatchdogMemoryUnit::Mib {
                            (1024, 2048)
                        } else {
                            (70, 90)
                        };
                        draft.monitors[WatchdogResource::Memory as usize] = ThresholdInputs {
                            warning: warning.to_string(),
                            critical: critical.to_string(),
                        };
                    }
                }
            }
            Message::Duration(value) => {
                if let Some(seconds) = parse_duration(&value) {
                    settings.sustained_seconds = seconds;
                }
                self.duration = Some(value);
            }
            Message::Threshold(resource, critical, value) => {
                if let Some(draft) = &mut self.draft {
                    if critical {
                        draft.monitors[resource as usize].critical = value;
                    } else {
                        draft.monitors[resource as usize].warning = value;
                    }
                }
            }
            Message::Notify(value) => settings.notify = value,
            Message::Action(resource, critical, value) => {
                if let Some(draft) = &mut self.draft {
                    response(draft.rule.monitor_mut(resource), critical).action = value;
                }
            }
            Message::Add | Message::Remove(_) | Message::Browse => {}
        }
    }

    pub(super) fn view<'a>(
        &'a self,
        settings: &'a WatchdogSettings,
        candidates: &[app_picker::Candidate],
    ) -> Element<'a, Message> {
        let rules = settings
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| {
                (
                    widgets::stable_key(&format!("{index}:{}", rule.executable_path)),
                    column![
                        widgets::process_rule_header(
                            &rule.executable_path,
                            candidates,
                            widgets::checkbox(rule.enabled)
                                .on_toggle(move |value| Message::RuleEnabled(index, value))
                                .into(),
                            None,
                            WatchdogResource::ALL
                                .into_iter()
                                .map(|resource| {
                                    let monitor = rule.monitor(resource);
                                    let unit = if rule.metric(resource) == WatchdogMetric::MemoryMib
                                    {
                                        " MiB"
                                    } else {
                                        "%"
                                    };
                                    text(if monitor.enabled {
                                        format!(
                                            "{}{unit} / {}{unit}",
                                            monitor.warning.threshold, monitor.critical.threshold
                                        )
                                    } else {
                                        t!("common.off").to_string()
                                    })
                                    .size(design::typography::SECONDARY)
                                    .into()
                                })
                                .collect(),
                            row![
                                iced::widget::tooltip(
                                    widgets::icon_button("icons/pencil.svg")
                                        .on_press(Message::Edit(index)),
                                    text(t!("common.edit").to_string()),
                                    iced::widget::tooltip::Position::Top,
                                ),
                                widgets::rule_delete_button(Some(Message::Remove(index)))
                            ]
                            .spacing(design::space::SMALL)
                            .into(),
                        ),
                        widgets::settings_divider(),
                    ]
                    .into(),
                )
            })
            .collect();
        super::scrolling::scrollable(
            column![
                widgets::settings_card(widgets::setting_row(
                    "watchdog.enable",
                    widgets::switch(settings.enabled, Some(Message::Enabled))
                )),
                widgets::heading(t!("watchdog.shared").to_string(), design::typography::BODY),
                widgets::settings_card(widgets::settings_rows([
                    widgets::setting_row_with_unit(
                        "watchdog.duration",
                        "s",
                        widgets::text_input(
                            "",
                            self.duration
                                .as_deref()
                                .unwrap_or(&settings.sustained_seconds.to_string()),
                        )
                        .on_input(Message::Duration)
                        .width(design::STANDALONE_NUMERIC_WIDTH),
                    )
                    .into(),
                    widgets::setting_row(
                        "watchdog.notify",
                        widgets::switch(settings.notify, Some(Message::Notify)),
                    )
                    .into(),
                ])),
                widgets::setting_title("watchdog.rules"),
                app_picker::view(
                    &self.path,
                    candidates,
                    true,
                    Message::Path,
                    Message::Browse,
                    self.can_add(settings, &self.path).then_some(Message::Add),
                    |path| self.can_add(settings, path).then_some(true)
                ),
                widgets::process_rules_table_with_actions(
                    ["watchdog.cpu_tab", "watchdog.memory_tab"].map(|key| t!(key).to_string()),
                    rules,
                    t!("watchdog.empty").to_string(),
                    80,
                    false,
                ),
            ]
            .spacing(widgets::CARD_GAP),
        )
        .height(Fill)
        .into()
    }

    pub(super) fn modal<'a>(
        &'a self,
        candidates: &[app_picker::Candidate],
    ) -> Option<Element<'a, Message>> {
        let draft = self.draft.as_ref()?;
        let rule = &draft.rule;
        let valid = draft.validated_rule().is_some();
        let resource = draft.tab;
        let monitor = rule.monitor(resource);
        let mut details = column![widgets::settings_card(widgets::setting_row(
            match resource {
                WatchdogResource::Cpu => "watchdog.monitor_cpu",
                WatchdogResource::Memory => "watchdog.monitor_memory",
            },
            widgets::switch(
                monitor.enabled,
                Some(move |enabled| Message::MonitorEnabled(resource, enabled))
            ),
        ))]
        .spacing(widgets::CARD_GAP);
        if resource == WatchdogResource::Memory && monitor.enabled {
            details = details
                .push(widgets::heading(
                    t!("watchdog.measurement").to_string(),
                    design::typography::BODY,
                ))
                .push(widgets::settings_card(widgets::setting_row(
                    "watchdog.memory_unit",
                    widgets::pick_list(
                        WatchdogMemoryUnit::ALL.to_vec(),
                        Some(rule.memory_unit),
                        Message::MemoryUnit,
                    )
                    .width(180),
                )));
        }
        if monitor.enabled {
            let raw = &draft.monitors[resource as usize];
            let metric = rule.metric(resource);
            for (critical, response, threshold) in [
                (false, &monitor.warning, &raw.warning),
                (true, &monitor.critical, &raw.critical),
            ] {
                details = details
                    .push(widgets::heading(
                        format!(
                            "{} · {}",
                            metric,
                            t!(if critical {
                                "watchdog.critical"
                            } else {
                                "watchdog.warning"
                            })
                        ),
                        design::typography::BODY,
                    ))
                    .push(widgets::settings_card(widgets::settings_rows([
                        widgets::setting_row_with_unit(
                            "watchdog.threshold",
                            if metric == WatchdogMetric::MemoryMib {
                                "MiB"
                            } else {
                                "%"
                            },
                            widgets::text_input("", threshold)
                                .on_input(move |value| {
                                    Message::Threshold(resource, critical, value)
                                })
                                .on_submit(Message::SaveRule)
                                .width(design::STANDALONE_NUMERIC_WIDTH),
                        )
                        .into(),
                        widgets::setting_row(
                            "watchdog.action",
                            widgets::pick_list(
                                vec![
                                    WatchdogAction::None,
                                    WatchdogAction::Terminate,
                                    WatchdogAction::Restart,
                                ],
                                Some(response.action),
                                move |value| Message::Action(resource, critical, value),
                            )
                            .width(180),
                        )
                        .into(),
                    ])));
            }
        }
        if !valid {
            details = details.push(text(t!("watchdog.invalid").to_string()).style(text::danger));
        }
        Some(widgets::modal_frame(
            row![
                widgets::heading(
                    t!(if draft.index.is_some() {
                        "watchdog.edit_rule"
                    } else {
                        "watchdog.add_rule"
                    })
                    .to_string(),
                    design::typography::SECTION,
                )
                .width(Fill),
                widgets::icon_button("icons/x.svg").on_press(Message::CancelRule),
            ]
            .align_y(iced::Center),
            column![
                column![
                    app_picker::app_name(&rule.executable_path, candidates),
                    text(rule.executable_path.clone()).style(text::secondary),
                ]
                .spacing(design::space::SMALL),
                iced::widget::container(row![
                    widgets::panel_tab(
                        t!("watchdog.cpu_tab").to_string(),
                        resource == WatchdogResource::Cpu,
                        Message::Tab(WatchdogResource::Cpu)
                    ),
                    widgets::panel_tab(
                        t!("watchdog.memory_tab").to_string(),
                        resource == WatchdogResource::Memory,
                        Message::Tab(WatchdogResource::Memory)
                    ),
                ])
                .style(widgets::tab_bar),
                super::motion::wrap(
                    super::scrolling::scrollable_pane(details)
                        .id("watchdog-rule-details")
                        .height(Fill),
                    true,
                    super::motion::Effect::Content(resource as u64),
                ),
            ]
            .spacing(widgets::CARD_GAP)
            .height(Fill),
            row![
                iced::widget::Space::new().width(Fill),
                widgets::action_button(t!("common.cancel").to_string())
                    .style(widgets::tertiary_button)
                    .on_press(Message::CancelRule),
                widgets::action_button(t!("common.ok").to_string())
                    .style(widgets::primary_button)
                    .on_press_maybe(valid.then_some(Message::SaveRule)),
            ]
            .spacing(design::space::SMALL),
            (760, 840),
        ))
    }
}

fn parse_duration(value: &str) -> Option<u64> {
    value
        .parse()
        .ok()
        .filter(|value| (1..=3600).contains(value))
}

fn response(
    rule: &mut crate::config::WatchdogMonitor,
    critical: bool,
) -> &mut crate::config::WatchdogResponse {
    if critical {
        &mut rule.critical
    } else {
        &mut rule.warning
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_keeps_temporary_text_and_only_updates_settings_when_valid() {
        let mut editor = Editor::default();
        let mut settings = WatchdogSettings::default();
        assert!(!editor.has_invalid_inputs());
        for value in ["", "0", "3601", "invalid", "18446744073709551616"] {
            editor.update(&mut settings, Message::Duration(value.into()));
            assert_eq!(editor.duration.as_deref(), Some(value));
            assert!(editor.has_invalid_inputs());
            assert_eq!(settings.sustained_seconds, 5);
        }
        for (value, expected) in [("1", 1), ("10", 10), ("3600", 3600)] {
            editor.update(&mut settings, Message::Duration(value.into()));
            assert_eq!(editor.duration.as_deref(), Some(value));
            assert!(!editor.has_invalid_inputs());
            assert_eq!(settings.sustained_seconds, expected);
        }
        editor.update(&mut settings, Message::Duration(String::new()));
        assert!(editor.has_invalid_inputs());
        assert_eq!(settings.sustained_seconds, 3600);
        editor = Editor::default();
        assert!(editor.duration.is_none());
        assert!(!editor.has_invalid_inputs());
    }

    #[test]
    fn cpu_memory_tab_transitions_keep_repaints_bounded() {
        use iced::advanced::{layout, renderer::Renderer as _, widget::Tree, Layout};
        use iced::{mouse, Rectangle, Size};
        use std::time::Instant;

        let mut rule = WatchdogRule::new(r"C:\Apps\test.exe".into());
        rule.memory.enabled = true;
        let mut settings = WatchdogSettings {
            rules: vec![rule],
            ..Default::default()
        };
        let mut editor = Editor::default();
        editor.update(&mut settings, Message::Edit(0));
        let mut renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let bounds = Rectangle::with_size(Size::new(1400.0, 900.0));
        let physical = Size::new(2100, 1350);
        let viewport = iced::advanced::graphics::Viewport::with_physical_size(physical, 1.5);
        let mut pixels = tiny_skia::Pixmap::new(physical.width, physical.height).unwrap();
        let mut mask = tiny_skia::Mask::new(physical.width, physical.height).unwrap();
        let mut tree = Tree::empty();
        let mut previous = Vec::new();
        for frame in 0..12 {
            let start = Instant::now();
            editor.update(
                &mut settings,
                Message::Tab(WatchdogResource::ALL[frame % 2]),
            );
            let mut view: Element<'_, Message> = iced::widget::stack![
                editor.view(&settings, &[]),
                super::super::scrolling::modal_backdrop(),
                iced::widget::container(editor.modal(&[]).unwrap())
                    .padding(16)
                    .center_x(Fill)
                    .center_y(Fill),
            ]
            .into();
            tree.diff(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, bounds.size()),
            );
            let laid_out = start.elapsed();
            renderer.reset(bounds);
            view.as_widget().draw(
                &tree,
                &mut renderer,
                &iced::Theme::Light,
                &iced::advanced::renderer::Style::default(),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &bounds,
            );
            let recorded = start.elapsed();
            let damage = if frame == 0 {
                vec![bounds]
            } else {
                iced::advanced::graphics::damage::group(
                    iced::advanced::graphics::damage::diff(
                        &previous,
                        renderer.layers(),
                        |layer| vec![layer.bounds],
                        iced_tiny_skia::Layer::damage,
                    ),
                    bounds,
                )
            };
            previous = renderer.layers().to_vec();
            assert!(
                damage.len() <= 4,
                "tab switch fragmented repaint: {damage:?}"
            );
            if frame > 0 {
                assert!(
                    damage
                        .iter()
                        .all(|region| region.x >= 320.0 && region.x + region.width <= 1080.0),
                    "tab switch repainted the page behind the dialog: {damage:?}"
                );
            }
            renderer.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &viewport,
                &damage,
                iced::Color::WHITE,
            );
            eprintln!("Watchdog tab {frame}: layout={laid_out:?}, draw={:?}, raster={:?}, total={:?}, regions={}",
                recorded-laid_out, start.elapsed()-recorded, start.elapsed(), damage.len());
            let mut expected = tiny_skia::Pixmap::new(physical.width, physical.height).unwrap();
            renderer.draw(
                &mut expected.as_mut(),
                &mut mask,
                &viewport,
                &[bounds],
                iced::Color::WHITE,
            );

            // Upstream damage-edge antialias coverage differs from a full redraw.
            // Compare the interiors to catch stale content without masking that known edge rounding.
            for (index, (actual, expected)) in pixels
                .data()
                .chunks_exact(4)
                .zip(expected.data().chunks_exact(4))
                .enumerate()
            {
                if actual
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| a.abs_diff(*b) <= 1)
                {
                    continue;
                }
                let x = (index % physical.width as usize) as f32 + 0.5;
                let y = (index / physical.width as usize) as f32 + 0.5;
                assert!(
                    damage.iter().any(|r| {
                        let r = *r * 1.5;
                        ((x - r.x).abs() <= 1.0 || (x - r.x - r.width).abs() <= 1.0)
                            && y >= r.y - 1.0
                            && y <= r.y + r.height + 1.0
                            || ((y - r.y).abs() <= 1.0 || (y - r.y - r.height).abs() <= 1.0)
                                && x >= r.x - 1.0
                                && x <= r.x + r.width + 1.0
                    }),
                    "partial repaint left stale interior pixels on tab frame {frame}: {x},{y}"
                );
            }
        }
    }

    #[test]
    fn both_resources_edit_independently_and_dialog_remains_transactional() {
        let mut editor = Editor::default();
        let mut settings = WatchdogSettings::default();
        editor.update(&mut settings, Message::Path(r"C:\Apps\test.exe".into()));
        editor.update(&mut settings, Message::Add);
        assert!(editor.has_pending_editor());
        assert!(settings.rules.is_empty());
        editor.update(&mut settings, Message::CancelRule);
        assert!(settings.rules.is_empty());
        editor.update(&mut settings, Message::Add);
        editor.update(
            &mut settings,
            Message::MonitorEnabled(WatchdogResource::Memory, true),
        );
        editor.update(&mut settings, Message::Tab(WatchdogResource::Memory));
        editor.update(&mut settings, Message::MemoryUnit(WatchdogMemoryUnit::Mib));
        editor.update(
            &mut settings,
            Message::Threshold(WatchdogResource::Cpu, false, "65".into()),
        );
        editor.update(
            &mut settings,
            Message::Action(WatchdogResource::Memory, true, WatchdogAction::Restart),
        );
        editor.update(&mut settings, Message::Tab(WatchdogResource::Cpu));
        editor.update(&mut settings, Message::SaveRule);
        assert!(!editor.has_pending_editor());
        assert!(settings.rules[0].cpu.enabled && settings.rules[0].memory.enabled);
        assert_eq!(settings.rules[0].cpu.warning.threshold, 65);
        assert_eq!(settings.rules[0].memory.critical.threshold, 2048);
        assert_eq!(settings.rules[0].cpu.critical.action, WatchdogAction::None);
        assert_eq!(
            settings.rules[0].memory.critical.action,
            WatchdogAction::Restart
        );
        let saved = settings.clone();
        editor.update(&mut settings, Message::Edit(0));
        editor.update(
            &mut settings,
            Message::Threshold(WatchdogResource::Memory, true, "".into()),
        );
        editor.update(&mut settings, Message::SaveRule);
        assert!(editor.has_pending_editor());
        assert_eq!(settings, saved);
        editor.update(&mut settings, Message::CancelRule);
        assert_eq!(settings, saved);
        editor.update(&mut settings, Message::Edit(0));
        editor.update(
            &mut settings,
            Message::MemoryUnit(WatchdogMemoryUnit::Percent),
        );
        editor.update(&mut settings, Message::SaveRule);
        assert_eq!(settings.rules[0].cpu.warning.threshold, 65);
        assert_eq!(settings.rules[0].memory.warning.threshold, 70);
        assert_eq!(settings.rules[0].memory.critical.threshold, 90);
        editor.update(&mut settings, Message::Edit(0));
        editor.update(
            &mut settings,
            Message::MonitorEnabled(WatchdogResource::Cpu, false),
        );
        editor.update(
            &mut settings,
            Message::MonitorEnabled(WatchdogResource::Memory, false),
        );
        editor.update(&mut settings, Message::SaveRule);
        assert!(editor.has_pending_editor());
        editor.update(&mut settings, Message::CancelRule);
        editor.update(&mut settings, Message::Path(r"C:\Apps\test.exe".into()));
        editor.update(&mut settings, Message::Add);
        assert!(!editor.has_pending_editor());
    }
}
