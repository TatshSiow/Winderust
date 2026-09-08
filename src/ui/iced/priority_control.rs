use crate::config::*;
use crate::ui::process_rules::can_add_process_candidate;
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input,
};
use iced::{Element, Fill};
use rust_i18n::t;
use std::path::Path;

#[derive(Default)]
pub(super) struct Editor {
    path: String,
    removing: Option<(Kind, String)>,
    deleting: Option<(Kind, usize, ProcessExclusionRule)>,
    collapsed: [[bool; 3]; 6],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Process,
    Thread,
    Io,
    Gpu,
    Memory,
    DynamicBoost,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tier {
    Focus,
    VisibleWindow,
    Background,
}
impl Tier {
    pub(super) const ALL: [Self; 3] = [Self::Focus, Self::VisibleWindow, Self::Background];
    pub(super) fn label(self) -> String {
        match self {
            Self::Focus => t!("cpu_allocation.focus"),
            Self::VisibleWindow => t!("common.visible_window"),
            Self::Background => t!("process_list.background"),
        }
        .to_string()
    }
    pub(super) fn flags(self) -> (bool, bool) {
        (self == Self::Focus, self == Self::VisibleWindow)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Value {
    Process(ProcessPrioritySetting),
    Thread(ProcessThreadPrioritySetting),
    Io(ProcessIoPrioritySetting),
    Gpu(ProcessGpuPrioritySetting),
    Memory(ProcessMemoryPrioritySetting),
    DynamicBoost(ProcessDynamicPriorityBoostSetting),
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Detection(Tier, bool),
    Preserve(Tier, bool),
    Default(Tier, Value),
    Path(String),
    Browse,
    Add,
    RuleEnabled(usize, bool),
    RuleValue(usize, Tier, Value),
    Remove(String),
    ConfirmRemove,
    Removed(String),
    Collapse(Tier),
    CancelRemove,
}
impl Kind {
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Process => "process_priority",
            Self::Thread => "thread_priority",
            Self::Io => "io_priority",
            Self::Gpu => "gpu_priority",
            Self::Memory => "memory_priority",
            Self::DynamicBoost => "dynamic_priority_boost",
        }
    }
    pub(super) fn enabled(self, settings: &Settings) -> bool {
        match self {
            Self::Process => settings.process_priority.enabled,
            Self::Thread => settings.thread_priority.enabled,
            Self::Io => settings.io_priority.enabled,
            Self::Gpu => settings.gpu_priority.enabled,
            Self::Memory => settings.memory_priority.enabled,
            Self::DynamicBoost => settings.dynamic_priority_boost.enabled,
        }
    }
    pub(super) fn rules(self, settings: &Settings) -> &[ProcessExclusionRule] {
        match self {
            Self::Process => &settings.process_priority.exclusions,
            Self::Thread => &settings.thread_priority.exclusions,
            Self::Io => &settings.io_priority.exclusions,
            Self::Gpu => &settings.gpu_priority.exclusions,
            Self::Memory => &settings.memory_priority.exclusions,
            Self::DynamicBoost => &settings.dynamic_priority_boost.exclusions,
        }
    }
    pub(super) fn rules_mut(self, settings: &mut Settings) -> &mut Vec<ProcessExclusionRule> {
        match self {
            Self::Process => &mut settings.process_priority.exclusions,
            Self::Thread => &mut settings.thread_priority.exclusions,
            Self::Io => &mut settings.io_priority.exclusions,
            Self::Gpu => &mut settings.gpu_priority.exclusions,
            Self::Memory => &mut settings.memory_priority.exclusions,
            Self::DynamicBoost => &mut settings.dynamic_priority_boost.exclusions,
        }
    }
    pub(super) fn detection(self, settings: &Settings, tier: Tier) -> bool {
        match self {
            Self::Process => match tier {
                Tier::Focus => settings.process_priority.foreground_detection_enabled,
                Tier::VisibleWindow => settings.process_priority.visible_window_detection_enabled,
                Tier::Background => true,
            },
            Self::Thread => match tier {
                Tier::Focus => settings.thread_priority.foreground_detection_enabled,
                Tier::VisibleWindow => settings.thread_priority.visible_window_detection_enabled,
                Tier::Background => true,
            },
            Self::Io => match tier {
                Tier::Focus => settings.io_priority.foreground_detection_enabled,
                Tier::VisibleWindow => settings.io_priority.visible_window_detection_enabled,
                Tier::Background => true,
            },
            Self::Gpu => match tier {
                Tier::Focus => settings.gpu_priority.foreground_detection_enabled,
                Tier::VisibleWindow => settings.gpu_priority.visible_window_detection_enabled,
                Tier::Background => true,
            },
            Self::Memory => match tier {
                Tier::Focus => settings.memory_priority.foreground_detection_enabled,
                Tier::VisibleWindow => settings.memory_priority.visible_window_detection_enabled,
                Tier::Background => true,
            },
            Self::DynamicBoost => match tier {
                Tier::Focus => settings.dynamic_priority_boost.foreground_detection_enabled,
                Tier::VisibleWindow => {
                    settings
                        .dynamic_priority_boost
                        .visible_window_detection_enabled
                }
                Tier::Background => true,
            },
        }
    }
    pub(super) fn preserve(self, settings: &Settings, tier: Tier) -> Option<bool> {
        match self {
            Self::Process => Some(match tier {
                Tier::Focus => settings.process_priority.preserve_foreground_priority,
                Tier::VisibleWindow => settings.process_priority.preserve_visible_window_priority,
                Tier::Background => settings.process_priority.preserve_background_priority,
            }),
            Self::Thread => Some(match tier {
                Tier::Focus => settings.thread_priority.preserve_foreground_priority,
                Tier::VisibleWindow => settings.thread_priority.preserve_visible_window_priority,
                Tier::Background => settings.thread_priority.preserve_background_priority,
            }),
            Self::Io => Some(match tier {
                Tier::Focus => settings.io_priority.preserve_foreground_priority,
                Tier::VisibleWindow => settings.io_priority.preserve_visible_window_priority,
                Tier::Background => settings.io_priority.preserve_background_priority,
            }),
            Self::Gpu => Some(match tier {
                Tier::Focus => settings.gpu_priority.preserve_foreground_priority,
                Tier::VisibleWindow => settings.gpu_priority.preserve_visible_window_priority,
                Tier::Background => settings.gpu_priority.preserve_background_priority,
            }),
            Self::Memory => Some(match tier {
                Tier::Focus => settings.memory_priority.preserve_foreground_priority,
                Tier::VisibleWindow => settings.memory_priority.preserve_visible_window_priority,
                Tier::Background => settings.memory_priority.preserve_background_priority,
            }),
            Self::DynamicBoost => None,
        }
    }
    pub(super) fn value(self, settings: &Settings, tier: Tier) -> Value {
        match self {
            Self::Process => Value::Process(match tier {
                Tier::Focus => settings.process_priority.foreground_priority,
                Tier::VisibleWindow => settings.process_priority.visible_window_priority,
                Tier::Background => settings.process_priority.background_priority,
            }),
            Self::Thread => Value::Thread(match tier {
                Tier::Focus => settings.thread_priority.foreground_priority,
                Tier::VisibleWindow => settings.thread_priority.visible_window_priority,
                Tier::Background => settings.thread_priority.background_priority,
            }),
            Self::Io => Value::Io(match tier {
                Tier::Focus => settings.io_priority.foreground_priority,
                Tier::VisibleWindow => settings.io_priority.visible_window_priority,
                Tier::Background => settings.io_priority.background_priority,
            }),
            Self::Gpu => Value::Gpu(match tier {
                Tier::Focus => settings.gpu_priority.foreground_priority,
                Tier::VisibleWindow => settings.gpu_priority.visible_window_priority,
                Tier::Background => settings.gpu_priority.background_priority,
            }),
            Self::Memory => Value::Memory(match tier {
                Tier::Focus => settings.memory_priority.foreground_priority,
                Tier::VisibleWindow => settings.memory_priority.visible_window_priority,
                Tier::Background => settings.memory_priority.background_priority,
            }),
            Self::DynamicBoost => Value::DynamicBoost(match tier {
                Tier::Focus => settings.dynamic_priority_boost.foreground_boost,
                Tier::VisibleWindow => settings.dynamic_priority_boost.visible_window_boost,
                Tier::Background => settings.dynamic_priority_boost.background_boost,
            }),
        }
    }
    pub(super) fn rule_value(self, rule: &ProcessExclusionRule, tier: Tier) -> Value {
        let (focus, visible) = tier.flags();
        match self {
            Self::Process => Value::Process(rule.process_priority_override(focus, visible)),
            Self::Thread => Value::Thread(rule.thread_priority_override(focus, visible)),
            Self::Io => Value::Io(rule.io_priority_override(focus, visible)),
            Self::Gpu => Value::Gpu(rule.gpu_priority_override(focus, visible)),
            Self::Memory => Value::Memory(rule.memory_priority_override(focus, visible)),
            Self::DynamicBoost => {
                Value::DynamicBoost(rule.dynamic_priority_boost_override(focus, visible))
            }
        }
    }
    pub(super) fn choices(self, advanced: bool) -> Vec<Value> {
        match self {
            Self::Process => (if advanced {
                &ProcessPrioritySetting::ADVANCED_ALL[..]
            } else {
                &ProcessPrioritySetting::ALL[..]
            })
            .iter()
            .copied()
            .map(Value::Process)
            .collect(),
            Self::Thread => (if advanced {
                &ProcessThreadPrioritySetting::ADVANCED_ALL[..]
            } else {
                &ProcessThreadPrioritySetting::ALL[..]
            })
            .iter()
            .copied()
            .map(Value::Thread)
            .collect(),
            Self::Io => (if advanced {
                &ProcessIoPrioritySetting::ADVANCED_ALL[..]
            } else {
                &ProcessIoPrioritySetting::ALL[..]
            })
            .iter()
            .copied()
            .map(Value::Io)
            .collect(),
            Self::Gpu => (if advanced {
                &ProcessGpuPrioritySetting::ADVANCED_ALL[..]
            } else {
                &ProcessGpuPrioritySetting::ALL[..]
            })
            .iter()
            .copied()
            .map(Value::Gpu)
            .collect(),
            Self::Memory => ProcessMemoryPrioritySetting::ALL[..]
                .iter()
                .copied()
                .map(Value::Memory)
                .collect(),
            Self::DynamicBoost => ProcessDynamicPriorityBoostSetting::ALL[..]
                .iter()
                .copied()
                .map(Value::DynamicBoost)
                .collect(),
        }
    }
    pub(super) fn can_add(self, settings: &Settings, path: &str) -> bool {
        can_add_process_candidate(
            path,
            |path| match self {
                Self::Process => settings.process_priority.contains_exclusion(path),
                Self::Thread => settings.thread_priority.contains_exclusion(path),
                Self::Io => settings.io_priority.contains_exclusion(path),
                Self::Gpu => settings.gpu_priority.contains_exclusion(path),
                Self::Memory => settings.memory_priority.contains_exclusion(path),
                Self::DynamicBoost => settings.dynamic_priority_boost.contains_exclusion(path),
            },
            |name| match self {
                Self::Process => crate::process_priority::is_builtin_excluded(name),
                Self::Thread => crate::thread_priority::is_builtin_excluded(name),
                Self::Io => crate::io_priority::is_builtin_excluded(name),
                Self::Gpu => crate::gpu_priority::is_builtin_excluded(name),
                Self::Memory => crate::memory_priority::is_builtin_excluded(name),
                Self::DynamicBoost => crate::dynamic_priority_boost::is_builtin_excluded(name),
            },
        )
    }
}
impl Editor {
    pub(super) fn update(&mut self, settings: &mut Settings, kind: Kind, message: Message) {
        match message {
            Message::Path(path) => self.path = path,
            Message::Browse => {} // The application opens the native executable picker.
            Message::Add => {
                if kind.enabled(settings) && kind.can_add(settings, &self.path) {
                    kind.rules_mut(settings).push(ProcessExclusionRule {
                        executable_path: crate::foreground::executable_path_key(Path::new(
                            self.path.trim(),
                        )),
                        ..Default::default()
                    });
                    self.path.clear();
                }
            }
            Message::Remove(path) => self.removing = Some((kind, path)),
            Message::Collapse(tier) => {
                let collapsed = &mut self.collapsed[kind as usize][tier as usize];
                *collapsed = !*collapsed;
            }
            Message::CancelRemove => self.removing = None,
            Message::ConfirmRemove => {
                if let Some((target, path)) = self.removing.take() {
                    let rules = target.rules_mut(settings);
                    if let Some(index) = rules.iter().position(|rule| rule.executable_path == path)
                    {
                        self.deleting = Some((target, index, rules.remove(index)));
                    }
                }
            }
            Message::Removed(path) => {
                if self.deleting.as_ref().is_some_and(|(target, _, rule)| {
                    *target == kind && rule.executable_path == path
                }) {
                    self.deleting = None;
                }
            }
            Message::RuleEnabled(index, value) => {
                if let Some(rule) = kind.rules_mut(settings).get_mut(index) {
                    rule.enabled = value;
                }
            }
            Message::RuleValue(index, tier, value) => {
                if !kind
                    .choices(settings.advanced.expose_all_priority_values)
                    .contains(&value)
                {
                    return;
                }
                if let Some(rule) = kind.rules_mut(settings).get_mut(index) {
                    let (focus, visible) = tier.flags();
                    match value {
                        Value::Process(value) => {
                            rule.set_process_priority_override(focus, visible, value)
                        }
                        Value::Thread(value) => {
                            rule.set_thread_priority_override(focus, visible, value)
                        }
                        Value::Io(value) => rule.set_io_priority_override(focus, visible, value),
                        Value::Gpu(value) => rule.set_gpu_priority_override(focus, visible, value),
                        Value::Memory(value) => {
                            rule.set_memory_priority_override(focus, visible, value)
                        }
                        Value::DynamicBoost(value) => {
                            rule.set_dynamic_priority_boost_override(focus, visible, value)
                        }
                    }
                }
            }
            Message::Default(tier, value) => {
                if !kind
                    .choices(settings.advanced.expose_all_priority_values)
                    .contains(&value)
                {
                    return;
                }
                match value {
                    Value::Process(value) => {
                        *match tier {
                            Tier::Focus => &mut settings.process_priority.foreground_priority,
                            Tier::VisibleWindow => {
                                &mut settings.process_priority.visible_window_priority
                            }
                            Tier::Background => &mut settings.process_priority.background_priority,
                        } = value
                    }
                    Value::Thread(value) => {
                        *match tier {
                            Tier::Focus => &mut settings.thread_priority.foreground_priority,
                            Tier::VisibleWindow => {
                                &mut settings.thread_priority.visible_window_priority
                            }
                            Tier::Background => &mut settings.thread_priority.background_priority,
                        } = value
                    }
                    Value::Io(value) => {
                        *match tier {
                            Tier::Focus => &mut settings.io_priority.foreground_priority,
                            Tier::VisibleWindow => {
                                &mut settings.io_priority.visible_window_priority
                            }
                            Tier::Background => &mut settings.io_priority.background_priority,
                        } = value
                    }
                    Value::Gpu(value) => {
                        *match tier {
                            Tier::Focus => &mut settings.gpu_priority.foreground_priority,
                            Tier::VisibleWindow => {
                                &mut settings.gpu_priority.visible_window_priority
                            }
                            Tier::Background => &mut settings.gpu_priority.background_priority,
                        } = value
                    }
                    Value::Memory(value) => {
                        *match tier {
                            Tier::Focus => &mut settings.memory_priority.foreground_priority,
                            Tier::VisibleWindow => {
                                &mut settings.memory_priority.visible_window_priority
                            }
                            Tier::Background => &mut settings.memory_priority.background_priority,
                        } = value
                    }
                    Value::DynamicBoost(value) => {
                        *match tier {
                            Tier::Focus => &mut settings.dynamic_priority_boost.foreground_boost,
                            Tier::VisibleWindow => {
                                &mut settings.dynamic_priority_boost.visible_window_boost
                            }
                            Tier::Background => {
                                &mut settings.dynamic_priority_boost.background_boost
                            }
                        } = value
                    }
                }
            }
            Message::Enabled(value) => match kind {
                Kind::Process => settings.process_priority.enabled = value,
                Kind::Thread => settings.thread_priority.enabled = value,
                Kind::Io => settings.io_priority.enabled = value,
                Kind::Gpu => settings.gpu_priority.enabled = value,
                Kind::Memory => settings.memory_priority.enabled = value,
                Kind::DynamicBoost => settings.dynamic_priority_boost.enabled = value,
            },
            Message::Detection(tier, value) => match kind {
                Kind::Process => match tier {
                    Tier::Focus => settings.process_priority.foreground_detection_enabled = value,
                    Tier::VisibleWindow => {
                        settings.process_priority.visible_window_detection_enabled = value
                    }
                    Tier::Background => {}
                },
                Kind::Thread => match tier {
                    Tier::Focus => settings.thread_priority.foreground_detection_enabled = value,
                    Tier::VisibleWindow => {
                        settings.thread_priority.visible_window_detection_enabled = value
                    }
                    Tier::Background => {}
                },
                Kind::Io => match tier {
                    Tier::Focus => settings.io_priority.foreground_detection_enabled = value,
                    Tier::VisibleWindow => {
                        settings.io_priority.visible_window_detection_enabled = value
                    }
                    Tier::Background => {}
                },
                Kind::Gpu => match tier {
                    Tier::Focus => settings.gpu_priority.foreground_detection_enabled = value,
                    Tier::VisibleWindow => {
                        settings.gpu_priority.visible_window_detection_enabled = value
                    }
                    Tier::Background => {}
                },
                Kind::Memory => match tier {
                    Tier::Focus => settings.memory_priority.foreground_detection_enabled = value,
                    Tier::VisibleWindow => {
                        settings.memory_priority.visible_window_detection_enabled = value
                    }
                    Tier::Background => {}
                },
                Kind::DynamicBoost => match tier {
                    Tier::Focus => {
                        settings.dynamic_priority_boost.foreground_detection_enabled = value
                    }
                    Tier::VisibleWindow => {
                        settings
                            .dynamic_priority_boost
                            .visible_window_detection_enabled = value
                    }
                    Tier::Background => {}
                },
            },
            Message::Preserve(tier, value) => match kind {
                Kind::Process => {
                    *match tier {
                        Tier::Focus => &mut settings.process_priority.preserve_foreground_priority,
                        Tier::VisibleWindow => {
                            &mut settings.process_priority.preserve_visible_window_priority
                        }
                        Tier::Background => {
                            &mut settings.process_priority.preserve_background_priority
                        }
                    } = value
                }
                Kind::Thread => {
                    *match tier {
                        Tier::Focus => &mut settings.thread_priority.preserve_foreground_priority,
                        Tier::VisibleWindow => {
                            &mut settings.thread_priority.preserve_visible_window_priority
                        }
                        Tier::Background => {
                            &mut settings.thread_priority.preserve_background_priority
                        }
                    } = value
                }
                Kind::Io => {
                    *match tier {
                        Tier::Focus => &mut settings.io_priority.preserve_foreground_priority,
                        Tier::VisibleWindow => {
                            &mut settings.io_priority.preserve_visible_window_priority
                        }
                        Tier::Background => &mut settings.io_priority.preserve_background_priority,
                    } = value
                }
                Kind::Gpu => {
                    *match tier {
                        Tier::Focus => &mut settings.gpu_priority.preserve_foreground_priority,
                        Tier::VisibleWindow => {
                            &mut settings.gpu_priority.preserve_visible_window_priority
                        }
                        Tier::Background => &mut settings.gpu_priority.preserve_background_priority,
                    } = value
                }
                Kind::Memory => {
                    *match tier {
                        Tier::Focus => &mut settings.memory_priority.preserve_foreground_priority,
                        Tier::VisibleWindow => {
                            &mut settings.memory_priority.preserve_visible_window_priority
                        }
                        Tier::Background => {
                            &mut settings.memory_priority.preserve_background_priority
                        }
                    } = value
                }
                Kind::DynamicBoost => {}
            },
        }
    }
    pub(super) fn view<'a>(
        &'a self,
        settings: &'a Settings,
        kind: Kind,
        candidates: &[String],
        motion_enabled: bool,
    ) -> Element<'a, Message> {
        let key = kind.key();
        let enabled = kind.enabled(settings);
        let choices = kind.choices(settings.advanced.expose_all_priority_values);
        let mut body = column![
            checkbox(enabled)
                .label(localized(key, "enable"))
                .on_toggle(Message::Enabled),
            text(localized(key, "intro_1"))
                .width(Fill)
                .style(text::secondary),
            text(localized(key, "intro_2"))
                .width(Fill)
                .style(text::secondary)
        ]
        .spacing(12);
        if kind == Kind::Gpu {
            body = body.push(
                text(localized(key, "intro_3"))
                    .width(Fill)
                    .style(text::secondary),
            );
        }
        for tier in Tier::ALL {
            let detection = kind.detection(settings, tier);
            let mut group = column![].spacing(8);
            if tier != Tier::Background {
                let label = if tier == Tier::Focus {
                    localized(key, "foreground_detection")
                } else {
                    t!("common.visible_window_detection").to_string()
                };
                group = group.push(checkbox(detection).label(label).on_toggle_maybe(
                    enabled.then_some(move |value| Message::Detection(tier, value)),
                ));
            }
            let control: Element<'_, Message> = if enabled && detection {
                pick_list(
                    choices.clone(),
                    Some(kind.value(settings, tier)),
                    move |value| Message::Default(tier, value),
                )
                .into()
            } else {
                text(kind.value(settings, tier).to_string()).into()
            };
            group = group.push(control);
            if let Some(preserve) = kind.preserve(settings, tier) {
                let label = match tier {
                    Tier::Focus => t!("common.preserve_foreground_priority"),
                    Tier::VisibleWindow => t!("common.preserve_visible_window_priority"),
                    Tier::Background => t!("common.preserve_background_priority"),
                };
                group = group.push(checkbox(preserve).label(label.to_string()).on_toggle_maybe(
                    (enabled && detection).then_some(move |value| Message::Preserve(tier, value)),
                ));
            }
            body = body.push(
                container(
                    column![
                        button(text(format!(
                            "{} {}",
                            if self.collapsed[kind as usize][tier as usize] {
                                ">"
                            } else {
                                "v"
                            },
                            tier.label()
                        )))
                        .on_press(Message::Collapse(tier))
                        .style(button::text),
                        super::motion::reveal(
                            group,
                            !self.collapsed[kind as usize][tier as usize],
                            motion_enabled
                        )
                    ]
                    .spacing(8),
                )
                .padding(12)
                .width(Fill)
                .style(iced::widget::container::bordered_box),
            );
        }
        body = body
            .push(text(localized(key, "exclusions")).size(16))
            .push(text(localized(key, "exclusions_help")));
        body = body.push(
            row![
                text_input(&t!("process_list.executable_path"), &self.path).on_input(Message::Path),
                button(text(t!("common.browse_executable").to_string()))
                    .on_press_maybe(enabled.then_some(Message::Browse)),
                button(text(t!("common.add").to_string())).on_press_maybe(
                    (enabled && kind.can_add(settings, &self.path)).then_some(Message::Add)
                )
            ]
            .spacing(8),
        );
        let filter = self.path.to_lowercase();
        let candidates: Vec<_> = candidates
            .iter()
            .filter(|path| path.to_lowercase().contains(&filter) && kind.can_add(settings, path))
            .cloned()
            .collect();
        if enabled && !candidates.is_empty() {
            body = body.push(
                pick_list(candidates, None::<String>, Message::Path)
                    .placeholder(t!("process_list.app_name").to_string()),
            );
        }
        let mut rule_cards = Vec::new();
        let mut visible_rules: Vec<_> = kind.rules(settings).iter().enumerate().collect();
        if let Some((target, index, rule)) = &self.deleting {
            if *target == kind {
                visible_rules.insert((*index).min(visible_rules.len()), (usize::MAX, rule));
            }
        }
        for (index, rule) in visible_rules {
            let mut card = column![row![
                checkbox(rule.enabled)
                    .label(rule.executable_path.clone())
                    .on_toggle_maybe(
                        enabled.then_some(move |value| Message::RuleEnabled(index, value))
                    ),
                button(text(t!("common.remove").to_string())).on_press_maybe(
                    enabled.then_some(Message::Remove(rule.executable_path.clone()))
                )
            ]
            .spacing(8)]
            .spacing(8);
            for tier in Tier::ALL {
                let control: Element<'_, Message> = if enabled {
                    pick_list(
                        choices.clone(),
                        Some(kind.rule_value(rule, tier)),
                        move |value| Message::RuleValue(index, tier, value),
                    )
                    .into()
                } else {
                    text(kind.rule_value(rule, tier).to_string()).into()
                };
                card = card.push(row![text(tier.label()).width(180), control].spacing(8));
            }
            if self
                .removing
                .as_ref()
                .is_some_and(|(target, path)| *target == kind && path == &rule.executable_path)
            {
                card = card.push(
                    row![
                        button(text(t!("common.remove").to_string()))
                            .on_press(Message::ConfirmRemove),
                        button(text(t!("common.cancel").to_string()))
                            .on_press(Message::CancelRemove)
                    ]
                    .spacing(8),
                );
            }
            rule_cards.push((
                super::motion::key(&rule.executable_path),
                super::motion::removal(
                    container(card)
                        .padding(12)
                        .width(Fill)
                        .style(iced::widget::container::bordered_box),
                    index == usize::MAX,
                    motion_enabled,
                    Message::Removed(rule.executable_path.clone()),
                ),
            ));
        }
        body = body.push(iced::widget::keyed_column(rule_cards).spacing(8));
        if kind.rules(settings).is_empty() {
            body = body.push(text(localized(key, "no_exclusions")));
        }
        scrollable(body).spacing(10).height(Fill).into()
    }
}
pub(super) fn process_priority_setting_label(priority: ProcessPrioritySetting) -> String {
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

pub(super) fn process_thread_priority_setting_label(
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

pub(super) fn process_dynamic_priority_boost_setting_label(
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

pub(super) fn process_io_priority_setting_label(priority: ProcessIoPrioritySetting) -> String {
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

pub(super) fn process_io_priority_label(priority: ProcessIoPriority) -> String {
    match priority {
        ProcessIoPriority::Critical => t!("io_priority.priority_critical"),
        ProcessIoPriority::High => t!("io_priority.priority_high"),
        ProcessIoPriority::Normal => t!("io_priority.priority_normal"),
        ProcessIoPriority::Low => t!("io_priority.priority_low"),
        ProcessIoPriority::VeryLow => t!("io_priority.priority_very_low"),
    }
    .to_string()
}

pub(super) fn process_gpu_priority_setting_label(priority: ProcessGpuPrioritySetting) -> String {
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

pub(super) fn process_gpu_priority_label(priority: ProcessGpuPriority) -> String {
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

pub(super) fn process_memory_priority_setting_label(
    priority: ProcessMemoryPrioritySetting,
) -> String {
    match priority {
        ProcessMemoryPrioritySetting::Default => t!("memory_priority.priority_default").to_string(),
        ProcessMemoryPrioritySetting::VeryLow => {
            t!("memory_priority.priority_very_low").to_string()
        }
        ProcessMemoryPrioritySetting::Low => t!("memory_priority.priority_low").to_string(),
        ProcessMemoryPrioritySetting::Medium => t!("memory_priority.priority_medium").to_string(),
        ProcessMemoryPrioritySetting::BelowNormal => {
            t!("memory_priority.priority_below_normal").to_string()
        }
        ProcessMemoryPrioritySetting::Normal => t!("memory_priority.priority_normal").to_string(),
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match *self {
            Self::Process(value) => process_priority_setting_label(value),
            Self::Thread(value) => process_thread_priority_setting_label(value),
            Self::Io(value) => process_io_priority_setting_label(value),
            Self::Gpu(value) => process_gpu_priority_setting_label(value),
            Self::Memory(value) => process_memory_priority_setting_label(value),
            Self::DynamicBoost(value) => process_dynamic_priority_boost_setting_label(value),
        })
    }
}
fn localized(section: &str, field: &str) -> String {
    let key = format!("{section}.{field}");
    t!(&key).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn priority_edits_preserve_tiers_and_enforce_advanced_values() {
        let mut settings = Settings::default();
        let mut editor = Editor::default();
        editor.update(&mut settings, Kind::Process, Message::Enabled(true));
        editor.update(
            &mut settings,
            Kind::Process,
            Message::Path(r"C:\Apps\editor.exe".into()),
        );
        editor.update(&mut settings, Kind::Process, Message::Add);
        assert_eq!(settings.process_priority.exclusions.len(), 1);
        editor.update(
            &mut settings,
            Kind::Process,
            Message::RuleValue(
                0,
                Tier::VisibleWindow,
                Value::Process(ProcessPrioritySetting::BelowNormal),
            ),
        );
        assert_eq!(
            settings.process_priority.exclusions[0].process_visible_window_priority,
            Some(ProcessPrioritySetting::BelowNormal)
        );
        assert_eq!(
            settings.process_priority.exclusions[0].process_foreground_priority,
            None
        );
        let previous = settings.process_priority.foreground_priority;
        editor.update(
            &mut settings,
            Kind::Process,
            Message::Default(
                Tier::Focus,
                Value::Process(ProcessPrioritySetting::Realtime),
            ),
        );
        assert_eq!(settings.process_priority.foreground_priority, previous);
        settings.advanced.expose_all_priority_values = true;
        editor.update(
            &mut settings,
            Kind::Process,
            Message::Default(
                Tier::Focus,
                Value::Process(ProcessPrioritySetting::Realtime),
            ),
        );
        assert_eq!(
            settings.process_priority.foreground_priority,
            ProcessPrioritySetting::Realtime
        );
        editor.update(
            &mut settings,
            Kind::Process,
            Message::Remove(settings_path()),
        );
        editor.update(&mut settings, Kind::Process, Message::CancelRemove);
        editor.update(&mut settings, Kind::Process, Message::ConfirmRemove);
        assert_eq!(settings.process_priority.exclusions.len(), 1);
    }
    #[test]
    fn every_priority_page_updates_only_its_own_defaults_and_rule_tier() {
        for kind in [
            Kind::Process,
            Kind::Thread,
            Kind::Io,
            Kind::Gpu,
            Kind::Memory,
            Kind::DynamicBoost,
        ] {
            let mut settings = Settings::default();
            let mut editor = Editor::default();
            editor.update(&mut settings, kind, Message::Enabled(true));
            editor.update(&mut settings, kind, Message::Path(settings_path()));
            editor.update(&mut settings, kind, Message::Add);
            let selected = kind.choices(false)[1];
            let background = kind.value(&settings, Tier::Background);
            editor.update(
                &mut settings,
                kind,
                Message::Default(Tier::VisibleWindow, selected),
            );
            assert_eq!(kind.value(&settings, Tier::VisibleWindow), selected);
            assert_eq!(kind.value(&settings, Tier::Background), background);
            editor.update(
                &mut settings,
                kind,
                Message::RuleValue(0, Tier::Focus, selected),
            );
            assert_eq!(
                kind.rule_value(&kind.rules(&settings)[0], Tier::Focus),
                selected
            );
            assert_eq!(
                kind.rule_value(&kind.rules(&settings)[0], Tier::VisibleWindow),
                kind.choices(false)[0]
            );
            editor.update(
                &mut settings,
                kind,
                Message::Detection(Tier::VisibleWindow, true),
            );
            assert!(kind.detection(&settings, Tier::VisibleWindow));
            if kind != Kind::DynamicBoost {
                editor.update(
                    &mut settings,
                    kind,
                    Message::Preserve(Tier::VisibleWindow, false),
                );
                assert_eq!(kind.preserve(&settings, Tier::VisibleWindow), Some(false));
                assert_eq!(kind.preserve(&settings, Tier::Background), Some(true));
            }
        }
    }
    #[test]
    fn confirmed_removal_changes_settings_without_waiting_for_a_visible_widget() {
        let mut settings = Settings::default();
        let mut editor = Editor::default();
        settings
            .process_priority
            .exclusions
            .push(ProcessExclusionRule {
                executable_path: settings_path(),
                ..Default::default()
            });
        editor.update(
            &mut settings,
            Kind::Process,
            Message::Remove(settings_path()),
        );
        editor.update(&mut settings, Kind::Process, Message::ConfirmRemove);
        assert!(settings.process_priority.exclusions.is_empty());
        assert!(editor.deleting.is_some());
        editor.update(
            &mut settings,
            Kind::Process,
            Message::Removed(settings_path()),
        );
        assert!(editor.deleting.is_none());
    }
    fn settings_path() -> String {
        crate::foreground::executable_path_key(Path::new(r"C:\Apps\editor.exe"))
    }
}
