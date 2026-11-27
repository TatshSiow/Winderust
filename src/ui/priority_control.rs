use super::design;
use super::feature_tabs::{self, FeatureTab};
use super::widgets::{self, checkbox, pick_list};
use crate::application::SettingsEditor;
use crate::config::*;
use crate::ui::process_rules::{
    can_add_exact_process_candidate, exact_path_input, new_process_exclusion_rule,
    picked_path_input,
};
use iced::widget::{column, Column};
use iced::{Element, Fill};
use rust_i18n::t;
use std::rc::Rc;

#[derive(Default)]
pub(super) struct Editor {
    path: String,
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
    pub(super) fn label_key(self) -> &'static str {
        match self {
            Self::Focus => "cpu_allocation.focus",
            Self::VisibleWindow => "common.visible_window",
            Self::Background => "common.background_process",
        }
    }

    pub(super) fn label(self) -> String {
        t!(self.label_key()).into_owned()
    }

    pub(super) fn detection_key(self) -> Option<&'static str> {
        match self {
            Self::Focus => Some("common.foreground_detection"),
            Self::VisibleWindow => Some("common.visible_window_detection"),
            Self::Background => None,
        }
    }

    pub(super) fn flags(self) -> (bool, bool) {
        (self == Self::Focus, self == Self::VisibleWindow)
    }

    pub(super) fn child_protection(self) -> Option<ChildProtection> {
        match self {
            Self::Focus => Some(ChildProtection::Focus),
            Self::VisibleWindow => Some(ChildProtection::VisibleWindow),
            Self::Background => None,
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub(super) enum ChildProtection {
    Focus,
    VisibleWindow,
}
impl ChildProtection {
    pub(super) fn enabled(self, settings: PriorityChildProtectionSettings) -> bool {
        match self {
            Self::Focus => settings.focused_app_children,
            Self::VisibleWindow => settings.visible_window_app_children,
        }
    }

    pub(super) fn set(self, settings: &mut PriorityChildProtectionSettings, enabled: bool) {
        match self {
            Self::Focus => settings.focused_app_children = enabled,
            Self::VisibleWindow => settings.visible_window_app_children = enabled,
        }
    }

    pub(super) fn row<'a, M: Clone + 'a>(
        self,
        settings: PriorityChildProtectionSettings,
        on_toggle: impl Fn(bool) -> M + 'a,
    ) -> iced::widget::Row<'a, M> {
        super::widgets::setting_row(
            "priority_control.protect_child_processes",
            super::widgets::switch(self.enabled(settings), Some(on_toggle)),
        )
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
impl Value {
    pub(super) fn color(&self, theme: &iced::Theme) -> Option<iced::Color> {
        match self {
            Self::Process(ProcessPrioritySetting::High)
            | Self::Io(ProcessIoPrioritySetting::High)
            | Self::Gpu(ProcessGpuPrioritySetting::High) => Some(theme.palette().warning),
            Self::Process(ProcessPrioritySetting::Realtime)
            | Self::Thread(ProcessThreadPrioritySetting::TimeCritical)
            | Self::Io(ProcessIoPrioritySetting::Critical)
            | Self::Gpu(ProcessGpuPrioritySetting::Realtime) => Some(theme.palette().danger),
            _ => None,
        }
    }

    pub(super) fn set_rule(self, rule: &mut ProcessExclusionRule, tier: Tier) {
        let (focus, visible) = tier.flags();
        match self {
            Self::Process(value) => rule.set_process_priority_override(focus, visible, value),
            Self::Thread(value) => rule.set_thread_priority_override(focus, visible, value),
            Self::Io(value) => rule.set_io_priority_override(focus, visible, value),
            Self::Gpu(value) => rule.set_gpu_priority_override(focus, visible, value),
            Self::Memory(value) => rule.set_memory_priority_override(focus, visible, value),
            Self::DynamicBoost(value) => {
                rule.set_dynamic_priority_boost_override(focus, visible, value)
            }
        }
    }
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Detection(Tier, bool),
    ProtectChildren(ChildProtection, bool),
    Preserve(Tier, bool),
    Default(Tier, Value),
    Path(String),
    Browse,
    Add,
    RuleEnabled(usize, bool),
    RuleValue(usize, Tier, Value),
    Remove(String),
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
        match tier {
            Tier::Focus => match self {
                Self::Process => settings.process_priority.foreground_detection_enabled,
                Self::Thread => settings.thread_priority.foreground_detection_enabled,
                Self::Io => settings.io_priority.foreground_detection_enabled,
                Self::Gpu => settings.gpu_priority.foreground_detection_enabled,
                Self::Memory => settings.memory_priority.foreground_detection_enabled,
                Self::DynamicBoost => settings.dynamic_priority_boost.foreground_detection_enabled,
            },
            Tier::VisibleWindow => match self {
                Self::Process => settings.process_priority.visible_window_detection_enabled,
                Self::Thread => settings.thread_priority.visible_window_detection_enabled,
                Self::Io => settings.io_priority.visible_window_detection_enabled,
                Self::Gpu => settings.gpu_priority.visible_window_detection_enabled,
                Self::Memory => settings.memory_priority.visible_window_detection_enabled,
                Self::DynamicBoost => {
                    settings
                        .dynamic_priority_boost
                        .visible_window_detection_enabled
                }
            },
            Tier::Background => true,
        }
    }
    fn child_protection(self, settings: &Settings) -> PriorityChildProtectionSettings {
        match self {
            Self::Process => settings.process_priority.child_protection,
            Self::Thread => settings.thread_priority.child_protection,
            Self::Io => settings.io_priority.child_protection,
            Self::Gpu => settings.gpu_priority.child_protection,
            Self::Memory => settings.memory_priority.child_protection,
            Self::DynamicBoost => settings.dynamic_priority_boost.child_protection,
        }
    }
    fn child_protection_mut(self, settings: &mut Settings) -> &mut PriorityChildProtectionSettings {
        match self {
            Self::Process => &mut settings.process_priority.child_protection,
            Self::Thread => &mut settings.thread_priority.child_protection,
            Self::Io => &mut settings.io_priority.child_protection,
            Self::Gpu => &mut settings.gpu_priority.child_protection,
            Self::Memory => &mut settings.memory_priority.child_protection,
            Self::DynamicBoost => &mut settings.dynamic_priority_boost.child_protection,
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
        can_add_exact_process_candidate(
            path,
            |path| {
                self.rules(settings)
                    .iter()
                    .any(|rule| same_rule_executable_path(&rule.executable_path, path))
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
    pub(super) fn update(&mut self, settings: &mut SettingsEditor, kind: Kind, message: Message) {
        match message {
            Message::Enabled(value) if kind.enabled(settings) == value => {}
            Message::Detection(tier, value) if kind.detection(settings, tier) == value => {}
            Message::Preserve(tier, value) if kind.preserve(settings, tier) == Some(value) => {}
            Message::ProtectChildren(protection, value) => {
                if protection.enabled(kind.child_protection(settings)) != value {
                    protection.set(kind.child_protection_mut(settings), value);
                }
            }
            Message::Path(path) => self.path = path,
            Message::Browse => {} // The application opens the native executable picker.
            Message::Add => {
                let path = exact_path_input(&self.path);
                if kind.can_add(settings, path) {
                    kind.rules_mut(settings)
                        .push(new_process_exclusion_rule(path));
                    self.path.clear();
                }
            }
            Message::Remove(path) => {
                if kind
                    .rules(settings)
                    .iter()
                    .any(|rule| rule.executable_path == path)
                {
                    kind.rules_mut(settings)
                        .retain(|rule| rule.executable_path != path);
                }
            }
            Message::RuleEnabled(index, value) => {
                if kind
                    .rules(settings)
                    .get(index)
                    .is_some_and(|rule| rule.enabled != value)
                {
                    kind.rules_mut(settings)[index].enabled = value;
                }
            }
            Message::RuleValue(index, tier, value) => {
                if !kind
                    .choices(settings.advanced.expose_all_priority_values)
                    .contains(&value)
                {
                    return;
                }
                if let Some(original) = kind.rules(settings).get(index) {
                    let mut rule = original.clone();
                    value.set_rule(&mut rule, tier);
                    if rule != *original {
                        kind.rules_mut(settings)[index] = rule;
                    }
                }
            }
            Message::Default(tier, value) => {
                if kind.value(settings, tier) == value
                    || !kind
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
            Message::Detection(tier, value) => match tier {
                Tier::Focus => match kind {
                    Kind::Process => settings.process_priority.foreground_detection_enabled = value,
                    Kind::Thread => settings.thread_priority.foreground_detection_enabled = value,
                    Kind::Io => settings.io_priority.foreground_detection_enabled = value,
                    Kind::Gpu => settings.gpu_priority.foreground_detection_enabled = value,
                    Kind::Memory => settings.memory_priority.foreground_detection_enabled = value,
                    Kind::DynamicBoost => {
                        settings.dynamic_priority_boost.foreground_detection_enabled = value
                    }
                },
                Tier::VisibleWindow => match kind {
                    Kind::Process => {
                        settings.process_priority.visible_window_detection_enabled = value
                    }
                    Kind::Thread => {
                        settings.thread_priority.visible_window_detection_enabled = value
                    }
                    Kind::Io => settings.io_priority.visible_window_detection_enabled = value,
                    Kind::Gpu => settings.gpu_priority.visible_window_detection_enabled = value,
                    Kind::Memory => {
                        settings.memory_priority.visible_window_detection_enabled = value
                    }
                    Kind::DynamicBoost => {
                        settings
                            .dynamic_priority_boost
                            .visible_window_detection_enabled = value
                    }
                },
                Tier::Background => {}
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
        candidates: &'a [super::app_picker::Candidate],
        tab: FeatureTab,
        scroll_clearance: f32,
    ) -> Element<'a, Message> {
        let choices: Rc<[_]> = kind
            .choices(settings.advanced.expose_all_priority_values)
            .into();
        let body = if tab == FeatureTab::CustomRules {
            self.rules_view(settings, kind, candidates, choices)
        } else {
            Self::setup(settings, kind, choices)
        };
        feature_tabs::scroll_content(body, scroll_clearance)
    }

    fn setup(settings: &Settings, kind: Kind, choices: Rc<[Value]>) -> Column<'_, Message> {
        let enable_key = format!("{}.enable", kind.key());
        let mut body = column![widgets::toggle_card(
            &enable_key,
            kind.enabled(settings),
            Some(Message::Enabled),
        )]
        .spacing(widgets::CARD_GAP);
        for tier in [Tier::Background, Tier::Focus, Tier::VisibleWindow] {
            let detection = tier.detection_key().map(|label| {
                widgets::setting_row(
                    label,
                    widgets::switch(
                        kind.detection(settings, tier),
                        Some(move |value| Message::Detection(tier, value)),
                    ),
                )
                .into()
            });
            let control: Element<'_, Message> = {
                pick_list(
                    Rc::clone(&choices),
                    Some(kind.value(settings, tier)),
                    move |value| Message::Default(tier, value),
                )
                .option_color(Value::color)
                .width(design::SELECT_WIDTH)
                .into()
            };
            let default = widgets::setting_row("common.default", control);
            let preserve = kind.preserve(settings, tier).map(|preserve| {
                let label = match tier {
                    Tier::Focus => "common.preserve_foreground_priority",
                    Tier::VisibleWindow => "common.preserve_visible_window_priority",
                    Tier::Background => "common.preserve_background_priority",
                };
                widgets::setting_row(
                    label,
                    widgets::switch(preserve, Some(move |value| Message::Preserve(tier, value))),
                )
                .into()
            });
            let children = tier.child_protection().map(|protection| {
                protection
                    .row(kind.child_protection(settings), move |value| {
                        Message::ProtectChildren(protection, value)
                    })
                    .into()
            });
            let rows = detection
                .into_iter()
                .chain([default.into()])
                .chain(preserve)
                .chain(children);
            body = body.push(widgets::settings_section(
                tier.label_key(),
                widgets::settings_card(widgets::settings_rows(rows)),
            ));
        }
        body
    }

    fn rules_view<'a>(
        &'a self,
        settings: &'a Settings,
        kind: Kind,
        candidates: &'a [super::app_picker::Candidate],
        choices: Rc<[Value]>,
    ) -> Column<'a, Message> {
        let picker = column![
            widgets::help_text("priority_control.custom_rules_help"),
            super::app_picker::view_with_path_selection(
                &self.path,
                candidates,
                true,
                (Message::Path, |path| Message::Path(picked_path_input(path))),
                Message::Browse,
                (kind.can_add(settings, exact_path_input(&self.path))).then_some(Message::Add),
                |path| kind.can_add(settings, path).then_some(true),
            ),
        ]
        .spacing(design::space::SMALL);
        let mut rule_cards = Vec::new();
        for (index, rule) in kind.rules(settings).iter().enumerate() {
            let mut controls = Vec::new();
            for tier in Tier::ALL {
                let control: Element<'_, Message> = {
                    pick_list(
                        Rc::clone(&choices),
                        Some(kind.rule_value(rule, tier)),
                        move |value| Message::RuleValue(index, tier, value),
                    )
                    .option_color(Value::color)
                    .width(Fill)
                    .into()
                };
                controls.push(control);
            }
            rule_cards.push((
                super::widgets::stable_key(&rule.executable_path),
                super::widgets::process_rule_row(
                    &rule.executable_path,
                    candidates,
                    checkbox(rule.enabled)
                        .on_toggle_maybe(Some(move |v| Message::RuleEnabled(index, v)))
                        .into(),
                    controls,
                    Some(Message::Remove(rule.executable_path.clone())),
                ),
            ));
        }
        column![
            super::widgets::settings_card(picker),
            super::widgets::process_rules_table(
                Tier::ALL.map(|tier| tier.label()),
                rule_cards,
                false,
            ),
        ]
        .spacing(super::widgets::CARD_GAP)
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn priority_warning_colors_match_the_original_risk_mapping() {
        for theme in [iced::Theme::Dark, iced::Theme::Light] {
            let caution = [
                Value::Process(ProcessPrioritySetting::High),
                Value::Io(ProcessIoPrioritySetting::High),
                Value::Gpu(ProcessGpuPrioritySetting::High),
            ];
            let danger = [
                Value::Process(ProcessPrioritySetting::Realtime),
                Value::Thread(ProcessThreadPrioritySetting::TimeCritical),
                Value::Io(ProcessIoPrioritySetting::Critical),
                Value::Gpu(ProcessGpuPrioritySetting::Realtime),
            ];
            for kind in [
                Kind::Process,
                Kind::Thread,
                Kind::Io,
                Kind::Gpu,
                Kind::Memory,
                Kind::DynamicBoost,
            ] {
                for value in kind.choices(true) {
                    let expected = if caution.contains(&value) {
                        Some(theme.palette().warning)
                    } else if danger.contains(&value) {
                        Some(theme.palette().danger)
                    } else {
                        None
                    };
                    assert_eq!(value.color(&theme), expected, "{value:?}");
                }
            }
        }
    }

    #[test]
    fn priority_edits_preserve_tiers_and_enforce_advanced_values() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
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
        assert!(settings.process_priority.exclusions.is_empty());
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
            let mut settings = SettingsEditor::with_settings(Settings::default());
            let mut editor = Editor::default();
            editor.update(&mut settings, kind, Message::Enabled(true));
            editor.update(&mut settings, kind, Message::Path(settings_path()));
            editor.update(&mut settings, kind, Message::Add);
            let literal = format!("{}\u{3000}", settings_path());
            editor.update(
                &mut settings,
                kind,
                Message::Path(picked_path_input(literal.clone())),
            );
            editor.update(&mut settings, kind, Message::Add);
            assert_eq!(kind.rules(&settings)[0].executable_path, settings_path());
            assert_eq!(kind.rules(&settings)[1].executable_path, literal);
            assert!(!kind.can_add(&settings, &literal));
            assert!(!kind.can_add(&settings, r"C:\Apps\csrss.exe"));
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
            for (tier, other_tier) in [
                (Tier::Focus, Tier::VisibleWindow),
                (Tier::VisibleWindow, Tier::Focus),
            ] {
                let other_detection = kind.detection(&settings, other_tier);
                for value in [false, true] {
                    editor.update(&mut settings, kind, Message::Detection(tier, value));
                    assert_eq!(kind.detection(&settings, tier), value);
                    assert_eq!(kind.detection(&settings, other_tier), other_detection);
                }
            }
            let before_background_detection = settings.global().clone();
            editor.update(
                &mut settings,
                kind,
                Message::Detection(Tier::Background, false),
            );
            assert!(kind.detection(&settings, Tier::Background));
            assert_eq!(settings.global(), &before_background_detection);
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
        let mut settings = SettingsEditor::with_settings(Settings::default());
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
        assert!(settings.process_priority.exclusions.is_empty());
    }

    #[test]
    fn editing_rule_path_preserves_battery_inheritance_until_add() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        settings.select_power_source(PowerSourceProfile::OnBattery);
        let mut editor = Editor::default();
        let kind = Kind::Process;
        editor.update(&mut settings, kind, Message::Path(settings_path()));
        assert!(settings.global().on_battery.is_none());

        editor.update(&mut settings, kind, Message::Add);
        assert_eq!(kind.rules(&settings).len(), 1);
        assert!(settings.global().process_priority.exclusions.is_empty());
    }

    fn settings_path() -> String {
        r"C:\Apps\editor.exe".into()
    }
}
