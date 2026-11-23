use super::design;
use super::widgets::{action_button, button, checkbox, icon_button, pick_list, text_input};
use crate::application::SettingsEditor;
use crate::config::*;
use crate::ui::process_rules::{
    can_add_exact_process_candidate, exact_path_input, new_process_exclusion_rule,
    picked_path_input,
};
use crate::ui::scrolling::scrollable;
use iced::widget::{column, row, text};
use iced::{Element, Fill};
use rust_i18n::t;
use std::collections::HashMap;
#[cfg(feature = "runtime-benchmark")]
#[path = "runtime_benchmark.rs"]
pub(crate) mod benchmark;
#[path = "adaptive_presets.rs"]
mod presets;
#[cfg(feature = "render-smoke")]
pub(super) use presets::BuiltInAdaptiveEnginePreset;
use presets::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum TuningTab {
    #[default]
    CpuBehaviour,
    CustomRules,
}
impl TuningTab {
    const ALL: [Self; 2] = [Self::CpuBehaviour, Self::CustomRules];
    fn key(self) -> &'static str {
        match self {
            Self::CpuBehaviour => "adaptive_engine.cpu_behaviour",
            Self::CustomRules => "adaptive_engine_process.custom_rules",
        }
    }
}
fn can_add_exclusion(settings: &AdaptiveEngineProcessSettings, path: &str) -> bool {
    can_add_exact_process_candidate(
        path,
        |path| {
            settings
                .custom_rules
                .iter()
                .any(|rule| same_rule_executable_path(&rule.executable_path, path))
        },
        crate::adaptive_engine_process::is_builtin_excluded,
    )
}

#[derive(Default)]
pub(super) struct Editor {
    name: String,
    selected_preset: Option<Choice<usize>>,
    draft: Option<Settings>,
    editing: Option<usize>,
    read_only: bool,
    tuning_tabs: [TuningTab; 2],
    expanded: [[bool; 3]; 2],
    path: String,
    error: String,
    live_numbers: NumberInputs,
    processor_power_numbers: NumberInputs,
    preset_numbers: NumberInputs,
}
#[derive(Default)]
pub(super) struct NumberInputs {
    pub(super) drafts: HashMap<&'static str, NumberDraft>,
}
pub(super) struct NumberDraft {
    pub(super) text: String,
    pub(super) invalid: Option<(u64, u64, &'static str)>,
}
impl NumberInputs {
    fn invalid(&self) -> Option<(u64, u64, &'static str)> {
        self.drafts.values().find_map(|draft| draft.invalid)
    }

    pub(super) fn validation_error(&self) -> Option<String> {
        self.invalid()
            .map(|(min, max, label)| format!("{}: {min}..{max}", t!(label)))
    }
}

pub(super) fn parse_number(text: &str, min: u64, max: u64) -> Option<u64> {
    text.parse()
        .ok()
        .filter(|value| (min..=max).contains(value))
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Toggle(fn(&mut Settings, bool), bool),
    ProcessorPower(super::adaptive_processor_power::Message),
    InterruptTiming(super::interrupt_timing::Message),
    TuningTab(TuningTab),
    Collapse(usize),
    Number(
        fn(&mut Settings, u64),
        String,
        u64,
        u64,
        &'static str,
        &'static str,
    ),
    Choice(fn(&mut Settings, usize), usize),
    Mask(crate::cpu_topology::CpuSelection),
    ZoneMask(crate::cpu_topology::CpuSelection),
    BuiltIn(BuiltInAdaptiveEnginePreset),
    ViewBuiltIn(BuiltInAdaptiveEnginePreset),
    Apply(usize),
    Edit(usize),
    New,
    Name(String),
    Save,
    Delete(usize),
    Cancel,
    UseCurrent,
    Path(String),
    Browse,
    AddExclusion,
    RemoveExclusion(usize),

    ExclusionEnabled(usize, bool),
}
impl Editor {
    pub(super) fn processor_power_view<'a>(
        &'a self,
        settings: &'a SettingsEditor,
    ) -> Element<'a, Message> {
        super::adaptive_processor_power::view(settings, &self.processor_power_numbers)
            .map(Message::ProcessorPower)
    }

    pub(super) fn update(&mut self, s: &mut SettingsEditor, m: Message) {
        if matches!(
            m,
            Message::ViewBuiltIn(_)
                | Message::Edit(_)
                | Message::New
                | Message::Cancel
                | Message::UseCurrent
        ) {
            self.error.clear();
            self.preset_numbers.drafts.clear();
        }
        match m {
            Message::TuningTab(tab) => {
                if self.draft.is_none() || tab != TuningTab::CustomRules {
                    self.tuning_tabs[usize::from(self.draft.is_some())] = tab;
                }
            }
            Message::Collapse(index) => {
                if let Some(value) = self.expanded[usize::from(self.draft.is_some())].get_mut(index)
                {
                    *value = !*value;
                }
            }
            Message::ViewBuiltIn(p) => {
                let mut draft = (**s).clone();
                apply_built_in_adaptive_engine_preset(&mut draft, p);
                self.draft = Some(draft);
                self.read_only = true;
                self.name = built_in_adaptive_engine_preset_label(p);
            }
            Message::Cancel => self.draft = None,
            Message::BuiltIn(p) => {
                self.error.clear();
                self.live_numbers.drafts.clear();
                self.selected_preset = BuiltInAdaptiveEnginePreset::ALL
                    .iter()
                    .position(|v| *v == p)
                    .map(|i| Choice(i, built_in_adaptive_engine_preset_label(p)));
                let mut draft = (**s).clone();
                apply_built_in_adaptive_engine_preset(&mut draft, p);
                let preset = capture_adaptive_engine_preset(&draft, String::new());
                apply_adaptive_engine_preset(s, &preset);
            }
            Message::Apply(i) => {
                if let Some(p) = s.global().adaptive_engine_presets.get(i).cloned() {
                    self.error.clear();
                    self.live_numbers.drafts.clear();
                    self.selected_preset = Some(Choice(i + 4, p.name.clone()));
                    apply_adaptive_engine_preset(s, &p)
                }
            }
            Message::Edit(i) => {
                if let Some(p) = s.global().adaptive_engine_presets.get(i) {
                    let mut draft = (**s).clone();
                    apply_adaptive_engine_preset(&mut draft, p);
                    self.name = p.name.clone();
                    self.editing = Some(i);
                    self.read_only = false;
                    self.draft = Some(draft);
                }
            }
            Message::New => {
                self.draft = Some((**s).clone());
                self.name.clear();
                self.editing = None;
                self.read_only = false;
            }
            Message::Name(v) => self.name = v,
            Message::Save => {
                if let Some(draft) = &self.draft {
                    if let Err(error) = draft
                        .adaptive_engine_process
                        .dynamic_resource_zone_settings
                        .validate(draft.adaptive_engine_process.dynamic_resource_zones_enabled)
                    {
                        self.error = error;
                        return;
                    }
                    let name = self.name.trim();
                    if !self.read_only
                        && self.preset_numbers.invalid().is_none()
                        && !name.is_empty()
                        && !s
                            .global()
                            .adaptive_engine_presets
                            .iter()
                            .enumerate()
                            .any(|(i, p)| {
                                Some(i) != self.editing && p.name.trim().eq_ignore_ascii_case(name)
                            })
                    {
                        let p = capture_adaptive_engine_preset(draft, name.into());
                        s.edit_global(|settings| {
                            if let Some(i) = self.editing {
                                if let Some(old) = settings.adaptive_engine_presets.get_mut(i) {
                                    old.name = p.name;
                                    old.adaptive_engine_process = p.adaptive_engine_process;
                                }
                            } else {
                                settings.adaptive_engine_presets.push(p);
                            }
                        });
                        self.draft = None;
                        self.preset_numbers.drafts.clear();
                    } else {
                        self.error = t!("adaptive_engine.duplicate_preset_name").to_string();
                    }
                }
            }
            Message::Delete(index) => {
                if index < s.global().adaptive_engine_presets.len() {
                    s.edit_global(|settings| {
                        settings.adaptive_engine_presets.remove(index);
                    });
                }
                self.draft = None;
                self.preset_numbers.drafts.clear();
            }
            Message::UseCurrent => {
                if !self.read_only {
                    self.draft = Some((**s).clone())
                }
            }
            Message::Path(v) => self.path = v,
            Message::Browse => {}
            Message::AddExclusion => {
                let path = exact_path_input(&self.path);
                if can_add_exclusion(&s.adaptive_engine_process, path) {
                    s.adaptive_engine_process
                        .custom_rules
                        .push(new_process_exclusion_rule(path));
                    self.path.clear();
                }
            }
            Message::RemoveExclusion(i) => {
                if i < s.adaptive_engine_process.custom_rules.len() {
                    s.adaptive_engine_process.custom_rules.remove(i);
                }
            }

            Message::ProcessorPower(message) => {
                super::adaptive_processor_power::update(
                    s,
                    &mut self.processor_power_numbers,
                    message,
                );
            }
            Message::ExclusionEnabled(i, v) => {
                if let Some(r) = s.adaptive_engine_process.custom_rules.get_mut(i) {
                    r.enabled = v
                }
            }
            edit => {
                if self.draft.is_some() && self.read_only {
                    return;
                }
                if let Message::Mask(selection) | Message::ZoneMask(selection) = &edit {
                    self.set_cpu_selection(
                        s,
                        matches!(edit, Message::ZoneMask(_)),
                        selection.clone(),
                        &crate::cpu_allocation::processor_topology(),
                    );
                    return;
                }
                match edit {
                    Message::Toggle(f, v) => f(self.draft.as_mut().unwrap_or_else(|| &mut **s), v),
                    Message::Number(f, v, min, max, key, label) => {
                        let numbers = if self.draft.is_some() {
                            &mut self.preset_numbers
                        } else {
                            &mut self.live_numbers
                        };
                        let value = parse_number(&v, min, max);
                        numbers.drafts.insert(
                            key,
                            NumberDraft {
                                text: v,
                                invalid: value.is_none().then_some((min, max, label)),
                            },
                        );
                        if let Some(n) = value {
                            let target = self.draft.as_mut().unwrap_or_else(|| &mut **s);
                            f(target, n);
                        }
                    }
                    Message::Choice(f, v) => f(self.draft.as_mut().unwrap_or_else(|| &mut **s), v),
                    _ => {}
                }
            }
        }
    }
    fn set_cpu_selection(
        &mut self,
        s: &mut SettingsEditor,
        zone: bool,
        selection: crate::cpu_topology::CpuSelection,
        topology: &Result<crate::cpu_topology::CpuTopology, String>,
    ) {
        if self.draft.is_some() && self.read_only {
            return;
        }
        if let Err(error) = super::cpu_allocation::verify_selection_against(
            &selection,
            topology,
            super::cpu_allocation::Kind::Hard,
        ) {
            self.error = error;
            return;
        }
        let target = self.draft.as_mut().unwrap_or_else(|| &mut **s);
        if zone {
            target
                .adaptive_engine_process
                .dynamic_resource_zone_settings
                .specific_processors = selection;
        } else {
            target.adaptive_engine_process.specific_processors = selection;
        }
        self.error.clear();
    }
    fn selected_preset(&self, live: &Settings, options: &[Choice<usize>]) -> Option<Choice<usize>> {
        let current = capture_adaptive_engine_preset(live, String::new());
        let preferred = self
            .selected_preset
            .as_ref()
            .filter(|choice| options.contains(choice));
        preferred
            .into_iter()
            .chain(options.iter())
            .find(|choice| {
                let mut candidate = live.clone();
                if choice.0 < 4 {
                    apply_built_in_adaptive_engine_preset(
                        &mut candidate,
                        BuiltInAdaptiveEnginePreset::ALL[choice.0],
                    );
                } else {
                    apply_adaptive_engine_preset(
                        &mut candidate,
                        &live.adaptive_engine_presets[choice.0 - 4],
                    );
                }
                capture_adaptive_engine_preset(&candidate, String::new()) == current
            })
            .cloned()
    }

    pub(super) fn reset_drafts(&mut self) {
        *self = Self {
            selected_preset: self.selected_preset.take(),
            ..Self::default()
        };
    }

    pub(super) fn validation_error(&self) -> Option<String> {
        self.live_numbers
            .validation_error()
            .or_else(|| self.processor_power_numbers.validation_error())
    }
    pub(super) fn preset_open(&self) -> bool {
        self.draft.is_some()
    }
    pub(super) fn has_pending_editor(&self) -> bool {
        self.preset_open() && !self.read_only
    }
    pub(super) fn view<'a>(
        &'a self,
        live: &'a Settings,
        candidates: &'a [super::app_picker::Candidate],
        interrupt_timing: &'a super::interrupt_timing::State,
    ) -> Element<'a, Message> {
        self.tuning_view(live, candidates, Some(interrupt_timing))
    }

    pub(super) fn preset_modal<'a>(
        &'a self,
        live: &'a Settings,
        candidates: &'a [super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        use iced::widget::Space;
        let title = if self.read_only {
            self.name.clone()
        } else {
            t!(if self.editing.is_some() {
                "adaptive_engine.edit_preset"
            } else {
                "adaptive_engine.add_preset"
            })
            .to_string()
        };
        let header = row![
            text(title).width(Fill),
            icon_button("icons/x.svg").on_press(Message::Cancel)
        ]
        .align_y(iced::Center);
        let name = super::widgets::settings_card(
            column![
                text(t!("adaptive_engine.preset_name").to_string()),
                text_input(&t!("adaptive_engine.preset_name_placeholder"), &self.name)
                    .on_input_maybe((!self.read_only).then_some(Message::Name)),
                text(t!("adaptive_engine.preset_name_help").to_string()).style(text::secondary)
            ]
            .spacing(design::space::SMALL),
        );
        let mut footer = row![Space::new().width(Fill)]
            .spacing(design::space::SMALL)
            .align_y(iced::Center);
        if let Some(index) = self.editing.filter(|_| !self.read_only) {
            footer = footer.push(
                action_button(t!("common.remove").to_string())
                    .style(super::widgets::danger_button)
                    .on_press(Message::Delete(index)),
            );
        }
        if !self.read_only {
            footer = footer.push(
                action_button(t!("adaptive_engine.use_current_settings").to_string())
                    .on_press(Message::UseCurrent),
            );
        }
        footer = footer.push(
            action_button(
                t!(if self.read_only {
                    "common.close"
                } else {
                    "common.cancel"
                })
                .to_string(),
            )
            .style(super::widgets::tertiary_button)
            .on_press(Message::Cancel),
        );
        if !self.read_only {
            footer = footer.push(
                action_button(t!("common.save").to_string())
                    .style(super::widgets::primary_button)
                    .on_press_maybe(
                        (!self.name.trim().is_empty() && self.preset_numbers.invalid().is_none())
                            .then_some(Message::Save),
                    ),
            );
        }
        let mut body = column![
            super::widgets::heading(
                t!("common.preset_details").to_string(),
                design::typography::BODY
            ),
            name,
            self.tuning_view(live, candidates, None)
        ]
        .spacing(design::space::MEDIUM)
        .height(Fill);
        if !self.error.is_empty() {
            body = body.push(text(self.error.clone()).style(text::danger));
        }
        super::widgets::modal_frame(header, body, footer, (1200, 850))
    }

    fn tuning_view<'a>(
        &'a self,
        live: &'a Settings,
        candidates: &'a [super::app_picker::Candidate],
        interrupt_timing: Option<&'a super::interrupt_timing::State>,
    ) -> Element<'a, Message> {
        let preset = interrupt_timing.is_none();
        let s = if preset {
            self.draft.as_ref().unwrap_or(live)
        } else {
            live
        };
        let numbers = if preset {
            &self.preset_numbers
        } else {
            &self.live_numbers
        };
        let editable = !preset || !self.read_only;
        let mut body = column![].spacing(super::widgets::CARD_GAP);
        macro_rules! number {($key:expr,$min:expr,$max:expr,$($field:ident).+) => {{
            let key=stringify!($($field).+);
            let min = $min;
            let formatted;
            let value = if let Some(draft) = numbers.drafts.get(key) {
                &draft.text
            } else {
                formatted = s.$($field).+.to_string();
                &formatted
            };
            let unit=if key.ends_with("_ms") {"ms"} else if key.ends_with("_seconds") {"s"} else if key.ends_with("_percent") || $max==100 {"%"} else {""};
            row![setting_label_with_unit($key, unit).width(Fill),
                super::widgets::stepper(value, min..=$max, 1,
                    editable.then_some(move|v|Message::Number(|s,n|s.$($field).+ = n as _,v,min,$max,key,$key)))
            ].spacing(design::space::SMALL).padding([design::space::CONTROL as u16, 0]).align_y(iced::Center)
        }};}
        macro_rules! selector {($s:ident,$key:expr,$ty:ty,$options:expr,$label:expr,$($field:ident).+) => {{let values:&[$ty]=$options;let selected=s.$($field).+;let control:Element<'_,Message>=if editable{pick_list(values.iter().copied().map(|v|Choice(v,$label(v))).collect::<Vec<_>>(),Some(Choice(selected,$label(selected))),move |v|Message::Choice(|$s,i|{let options:&[$ty]=$options;if let Some(v)=options.get(i){$s.$($field).+ = *v;}},values.iter().position(|x|*x==v.0).unwrap_or(0))).width(Fill).into()}else{text($label(selected)).into()};control}};}
        macro_rules! choice {($s:ident,$key:expr,$ty:ty,$options:expr,$label:expr,$($field:ident).+) => {row![setting_label($key).width(Fill),iced::widget::container(selector!($s,$key,$ty,$options,$label,$($field).+)).width(280)].spacing(design::space::SMALL).padding([design::space::CONTROL as u16, 0]).align_y(iced::Center)};}
        if !preset {
            body = body.push(super::widgets::toggle_card(
                "adaptive_engine.enable",
                s.adaptive_engine.enabled,
                Some(|value| Message::Toggle(|s, v| s.adaptive_engine.enabled = v, value)),
            ));
            let mut options: Vec<_> = BuiltInAdaptiveEnginePreset::ALL
                .into_iter()
                .enumerate()
                .map(|(i, p)| Choice(i, built_in_adaptive_engine_preset_label(p)))
                .collect();
            options.extend(
                live.adaptive_engine_presets
                    .iter()
                    .enumerate()
                    .map(|(i, p)| Choice(i + 4, p.name.clone())),
            );
            let selected = self.selected_preset(live, &options);
            body = body.push(super::widgets::settings_card(
                row![
                    setting_label("adaptive_engine.preset").width(Fill),
                    pick_list(options, selected, |choice| if choice.0 < 4 {
                        Message::BuiltIn(BuiltInAdaptiveEnginePreset::ALL[choice.0])
                    } else {
                        Message::Apply(choice.0 - 4)
                    })
                    .width(280)
                    .placeholder(t!("common.custom").to_string())
                ]
                .spacing(design::space::SMALL)
                .align_y(iced::Center),
            ));
        }
        let tab = self.tuning_tabs[usize::from(preset)];
        let mut tabs = row![];
        for next in TuningTab::ALL
            .into_iter()
            .filter(|tab| !preset || *tab != TuningTab::CustomRules)
        {
            tabs = tabs.push(super::widgets::panel_tab(
                t!(next.key()).to_string(),
                next == tab,
                Message::TuningTab(next),
            ));
        }
        body = body.push(iced::widget::container(tabs.wrap()).style(super::widgets::tab_bar));
        let header = body;
        let mut body = column![].spacing(super::widgets::CARD_GAP);
        match tab {
            TuningTab::CpuBehaviour => {
                if let Some(interrupt_timing) = interrupt_timing {
                    body = body.push(interrupt_timing.view().map(Message::InterruptTiming));
                }
                let custom_selection = editable
                    && (s.adaptive_engine_process.background_processor_selection
                        == BackgroundProcessorSelection::Custom
                        || s.adaptive_engine_process
                            .dynamic_resource_zone_settings
                            .background_processor_selection
                            == BackgroundProcessorSelection::Custom);
                let cpu_catalog = custom_selection.then(|| {
                    super::cpu_allocation::SelectionCatalog::capture(
                        super::cpu_allocation::Kind::Hard,
                    )
                });
                let pressure = super::widgets::settings_rows([
                    number!(
                        "adaptive_engine_process.background_app_cpu_threshold",
                        1,
                        100,
                        adaptive_engine_process.background_app_cpu_threshold_percent
                    )
                    .into(),
                    number!(
                        "adaptive_engine_process.restraint_candidate_limit",
                        1,
                        64,
                        adaptive_engine_process.maximum_restrained_apps
                    )
                    .into(),
                    number!(
                        "adaptive_engine_process.reaction_time",
                        250,
                        5000,
                        adaptive_engine_process.reaction_time_ms
                    )
                    .into(),
                    number!(
                        "adaptive_engine_process.foreground_or_system_cpu_threshold",
                        1,
                        100,
                        adaptive_engine_process.foreground_or_system_cpu_threshold_percent
                    )
                    .into(),
                    number!(
                        "adaptive_engine_process.cpu_restraint_time",
                        1,
                        3600,
                        adaptive_engine_process.cpu_restraint_time_seconds
                    )
                    .into(),
                    number!(
                        "adaptive_engine_process.cpu_recovery_threshold",
                        1,
                        100,
                        adaptive_engine_process.cpu_recovery_threshold_percent
                    )
                    .into(),
                    number!(
                        "adaptive_engine_process.cpu_recovery_time",
                        1,
                        3600,
                        adaptive_engine_process.cpu_recovery_time_seconds
                    )
                    .into(),
                ]);
                body = body
                    .push(super::widgets::heading(
                        t!("adaptive_engine_process.shared_detection").to_string(),
                        design::typography::BODY,
                    ))
                    .push(super::widgets::settings_card(pressure));
                let action = super::widgets::switch(
                    s.adaptive_engine_process.cpu_pressure_restraint_enabled,
                    editable.then_some(|v| {
                        Message::Toggle(
                            |s, v| s.adaptive_engine_process.cpu_pressure_restraint_enabled = v,
                            v,
                        )
                    }),
                );
                body = body.push(super::widgets::setting_group(
                    "adaptive_engine.cpu_pressure".to_string(),
                    self.expanded[usize::from(preset)][0],
                    Message::Collapse(0),
                    action,
                    column![text(
                        t!("adaptive_engine_process.cpu_pressure_restraint_help").to_string()
                    )
                    .style(text::secondary)],
                ));
                let mut allocation = super::widgets::settings_rows([
                    choice!(
                        s,
                        "adaptive_engine_process.processor_selection",
                        BackgroundProcessorSelection,
                        &BackgroundProcessorSelection::ALL,
                        background_processor_selection_label,
                        adaptive_engine_process.background_processor_selection
                    )
                    .into(),
                    choice!(
                        s,
                        "adaptive_engine_process.cpu_allocation_method",
                        CpuAllocationMethod,
                        &CpuAllocationMethod::ALL,
                        allocation_label,
                        adaptive_engine_process.cpu_allocation_method
                    )
                    .into(),
                ]);
                if s.adaptive_engine_process
                    .background_processor_selection
                    .uses_percentage()
                {
                    allocation = allocation
                        .push(super::widgets::settings_divider())
                        .push(number!(
                            "adaptive_engine_process.processor_limit",
                            1,
                            100,
                            adaptive_engine_process.processor_limit_percent
                        ));
                }
                if s.adaptive_engine_process.background_processor_selection
                    == BackgroundProcessorSelection::Custom
                    && editable
                {
                    if let Some(catalog) = &cpu_catalog {
                        allocation = allocation.push(super::widgets::settings_divider()).push(
                            super::cpu_allocation::mask_selector(
                                &s.adaptive_engine_process.specific_processors,
                                catalog,
                                &s.cpu_allocation_presets,
                                Message::Mask,
                            ),
                        );
                    }
                }
                if s.adaptive_engine_process.background_processor_selection
                    == BackgroundProcessorSelection::Custom
                    && !editable
                {
                    allocation = allocation
                        .push(super::widgets::settings_divider())
                        .push(text(format!(
                            "{}: {}",
                            t!("adaptive_engine_process.specific_processors"),
                            super::cpu_allocation::selection_text(
                                &s.adaptive_engine_process.specific_processors
                            )
                        )));
                }
                body = body.push(super::widgets::setting_group(
                    "adaptive_engine_process.limit_background_processors".to_string(),
                    self.expanded[usize::from(preset)][1],
                    Message::Collapse(1),
                    super::widgets::switch(
                        s.adaptive_engine_process
                            .limit_background_processors_enabled,
                        editable.then_some(|v| {
                            Message::Toggle(
                                |s, v| {
                                    s.adaptive_engine_process
                                        .limit_background_processors_enabled = v
                                },
                                v,
                            )
                        }),
                    ),
                    allocation,
                ));
                let zones = &s.adaptive_engine_process.dynamic_resource_zone_settings;
                let mut zone_content = super::widgets::settings_rows([
                    text(t!("adaptive_engine_process.zone_warning").to_string())
                        .style(text::secondary)
                        .into(),
                    choice!(
                        s,
                        "adaptive_engine_process.zone_selection",
                        BackgroundProcessorSelection,
                        &BackgroundProcessorSelection::ALL,
                        background_processor_selection_label,
                        adaptive_engine_process
                            .dynamic_resource_zone_settings
                            .background_processor_selection
                    )
                    .into(),
                ]);
                if zones.background_processor_selection.uses_percentage() {
                    zone_content =
                        zone_content
                            .push(super::widgets::settings_divider())
                            .push(number!(
                                "adaptive_engine_process.foreground_zone_share",
                                1,
                                99,
                                adaptive_engine_process
                                    .dynamic_resource_zone_settings
                                    .foreground_share_percent
                            ));
                }
                if zones.background_processor_selection == BackgroundProcessorSelection::Custom {
                    if editable {
                        if let Some(catalog) = &cpu_catalog {
                            zone_content = zone_content
                                .push(super::widgets::settings_divider())
                                .push(super::cpu_allocation::mask_selector(
                                    &zones.specific_processors,
                                    catalog,
                                    &s.cpu_allocation_presets,
                                    Message::ZoneMask,
                                ));
                        }
                    } else {
                        zone_content =
                            zone_content
                                .push(super::widgets::settings_divider())
                                .push(text(format!(
                                    "{}: {}",
                                    t!("adaptive_engine_process.specific_processors"),
                                    super::cpu_allocation::selection_text(
                                        &zones.specific_processors
                                    )
                                )));
                    }
                }
                body = body.push(super::widgets::setting_group(
                    "adaptive_engine_process.dynamic_resource_zones".to_string(),
                    self.expanded[usize::from(preset)][2],
                    Message::Collapse(2),
                    super::widgets::switch(
                        s.adaptive_engine_process.dynamic_resource_zones_enabled,
                        editable.then_some(|v| {
                            Message::Toggle(
                                |s, v| s.adaptive_engine_process.dynamic_resource_zones_enabled = v,
                                v,
                            )
                        }),
                    ),
                    zone_content,
                ));
            }
            TuningTab::CustomRules => {}
        }
        if !preset && tab == TuningTab::CustomRules {
            body = body
                .push(super::widgets::setting_title(
                    "adaptive_engine_process.custom_rules",
                ))
                .push(super::app_picker::view_with_path_selection(
                    &self.path,
                    candidates,
                    true,
                    (Message::Path, |path| Message::Path(picked_path_input(path))),
                    Message::Browse,
                    can_add_exclusion(&s.adaptive_engine_process, exact_path_input(&self.path))
                        .then_some(Message::AddExclusion),
                    |path| can_add_exclusion(&s.adaptive_engine_process, path).then_some(true),
                ));
            let rules = s
                .adaptive_engine_process
                .custom_rules
                .iter()
                .enumerate()
                .map(|(i, rule)| {
                    (
                        super::widgets::stable_key(&rule.executable_path),
                        super::widgets::process_rule_row(
                            &rule.executable_path,
                            candidates,
                            checkbox(rule.enabled)
                                .on_toggle(move |value| Message::ExclusionEnabled(i, value))
                                .into(),
                            vec![],
                            Some(Message::RemoveExclusion(i)),
                        ),
                    )
                })
                .collect();
            body = body.push(super::widgets::process_rules_table(
                [],
                rules,
                t!("common.no_custom_rules").to_string(),
            ));
        }

        if let Some(error) = numbers.validation_error() {
            body = body.push(text(error));
        }
        if !preset && !self.error.is_empty() {
            body = body.push(text(self.error.clone()).style(text::danger));
        }
        scrollable(header.push(super::motion::wrap(
            body,
            true,
            super::motion::Effect::Content(tab as u64),
        )))
        .width(Fill)
        .height(Fill)
        .into()
    }
    pub(super) fn presets_panel<'a>(&'a self, live: &'a SettingsEditor) -> Element<'a, Message> {
        let mut rail = column![super::widgets::heading(
            t!("adaptive_engine.built_in_presets").to_string(),
            design::typography::SECONDARY
        )]
        .spacing(design::space::MEDIUM)
        .padding([0, design::space::MEDIUM as u16]);
        for p in BuiltInAdaptiveEnginePreset::ALL {
            rail = rail.push(
                row![
                    button(
                        iced::widget::container(text(built_in_adaptive_engine_preset_label(p)))
                            .center_y(design::NAVIGATION_ROW_HEIGHT - 10)
                    )
                    .width(Fill)
                    .style(super::widgets::quiet)
                    .on_press(Message::BuiltIn(p)),
                    icon_button("icons/info.svg").on_press(Message::ViewBuiltIn(p))
                ]
                .spacing(design::space::TIGHT)
                .align_y(iced::Center),
            );
        }
        rail = rail.push(super::widgets::heading(
            t!("adaptive_engine.custom_presets").to_string(),
            design::typography::SECONDARY,
        ));
        if live.adaptive_engine_presets.is_empty() {
            rail = rail.push(
                text(t!("adaptive_engine.no_custom_presets").to_string()).style(text::secondary),
            );
        }
        for (i, p) in live.adaptive_engine_presets.iter().enumerate() {
            rail = rail.push(
                row![
                    button(text(p.name.clone()))
                        .width(Fill)
                        .style(super::widgets::quiet)
                        .on_press(Message::Apply(i)),
                    super::widgets::tooltip(
                        icon_button("icons/pencil.svg").on_press(Message::Edit(i)),
                        text(t!("adaptive_engine.edit_preset").to_string()),
                        iced::widget::tooltip::Position::Top,
                    ),
                    super::widgets::rule_delete_button(Some(Message::Delete(i)))
                ]
                .spacing(design::space::TIGHT)
                .align_y(iced::Center),
            );
        }
        column![
            scrollable(rail).width(Fill).height(Fill),
            super::widgets::preset_footer(
                t!("adaptive_engine.add_preset").to_string(),
                Message::New
            ),
        ]
        .spacing(design::space::MEDIUM)
        .height(Fill)
        .into()
    }
}
fn setting_label(key: &str) -> iced::widget::Column<'static, Message> {
    setting_label_with_unit(key, "")
}

fn setting_label_with_unit(key: &str, unit: &str) -> iced::widget::Column<'static, Message> {
    super::widgets::setting_title_with_help(key, unit, &format!("{key}_help"))
}
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Choice<T>(pub(super) T, pub(super) String);
impl<T> std::fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.1.fmt(f)
    }
}
fn allocation_label(v: CpuAllocationMethod) -> String {
    t!(match v {
        CpuAllocationMethod::CpuSetsSoft => "nav.cpu_sets_soft",
        CpuAllocationMethod::ProcessorAffinityHard => "nav.processor_affinity_hard",
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn preset_copies_keep_stale_bindings_and_raced_edits_leave_them_intact() {
        let selection = crate::cpu_topology::test_selection(3);
        let mut settings = Settings::default();
        settings.adaptive_engine_process.specific_processors = selection.clone();
        settings
            .adaptive_engine_process
            .dynamic_resource_zone_settings
            .specific_processors = selection.clone();
        let preset = capture_adaptive_engine_preset(&settings, "Saved".into());
        let mut applied = Settings::default();
        apply_adaptive_engine_preset(&mut applied, &preset);
        assert_eq!(
            applied.adaptive_engine_process.specific_processors,
            selection
        );
        assert_eq!(
            applied
                .adaptive_engine_process
                .dynamic_resource_zone_settings
                .specific_processors,
            selection
        );
        let original = crate::cpu_topology::test_topology();
        let mut processors = original.processors().to_vec();
        processors[0].efficiency_class = 1;
        let changed = crate::cpu_topology::CpuTopology::new(
            processors,
            original.groups().to_vec(),
            original.caches().to_vec(),
            original.numa_nodes().to_vec(),
        )
        .unwrap();
        let before = applied.clone();
        let mut applied = SettingsEditor::with_settings(applied);
        let mut editor = Editor::default();
        for zone in [false, true] {
            editor.set_cpu_selection(&mut applied, zone, selection.clone(), &Ok(changed.clone()));
            assert_eq!(applied.global(), &before);
            assert!(!editor.error.is_empty());
        }
    }

    #[test]
    fn zone_preset_editing_is_independent_and_speed_keeps_background_limiting() {
        let mut editor = Editor::default();
        let mut settings = SettingsEditor::with_settings(Settings::default());
        editor.update(
            &mut settings,
            Message::BuiltIn(BuiltInAdaptiveEnginePreset::Speed),
        );
        assert!(
            !settings
                .adaptive_engine_process
                .dynamic_resource_zones_enabled
        );
        assert!(
            settings
                .adaptive_engine_process
                .limit_background_processors_enabled
        );
        assert_eq!(settings.adaptive_engine_process.processor_limit_percent, 75);
        assert_eq!(
            settings
                .adaptive_engine_process
                .foreground_or_system_cpu_threshold_percent,
            35
        );
        let before = settings.clone();
        editor.update(&mut settings, Message::New);
        editor.update(&mut settings, Message::Collapse(2));
        editor.set_cpu_selection(
            &mut settings,
            true,
            crate::cpu_topology::test_selection(10),
            &Ok(crate::cpu_topology::test_topology()),
        );
        editor.update(
            &mut settings,
            Message::Number(
                |s, v| {
                    s.adaptive_engine_process
                        .dynamic_resource_zone_settings
                        .foreground_share_percent = v as u8
                },
                "65".into(),
                1,
                99,
                "adaptive_engine_process.dynamic_resource_zone_settings.foreground_share_percent",
                "adaptive_engine_process.foreground_zone_share",
            ),
        );
        let draft = editor.draft.as_ref().unwrap();
        assert_eq!(draft.adaptive_engine_process.processor_limit_percent, 75);
        assert_eq!(
            draft
                .adaptive_engine_process
                .dynamic_resource_zone_settings
                .foreground_share_percent,
            65
        );
        assert_eq!(
            draft
                .adaptive_engine_process
                .dynamic_resource_zone_settings
                .specific_processors,
            crate::cpu_topology::test_selection(10)
        );
        assert_eq!(editor.expanded[0], [false; 3]);
        let preset = capture_adaptive_engine_preset(draft, "Zones".into());
        let mut applied = settings.clone();
        apply_adaptive_engine_preset(&mut applied, &preset);
        assert_eq!(
            applied
                .adaptive_engine_process
                .dynamic_resource_zone_settings,
            draft.adaptive_engine_process.dynamic_resource_zone_settings
        );
        editor.update(&mut settings, Message::Cancel);
        assert_eq!(settings.global(), &before);
        editor.update(
            &mut settings,
            Message::ViewBuiltIn(BuiltInAdaptiveEnginePreset::Performance),
        );
        let preview = editor.draft.clone();
        editor.set_cpu_selection(
            &mut settings,
            true,
            crate::cpu_topology::test_selection(1),
            &Ok(crate::cpu_topology::test_topology()),
        );
        assert_eq!(editor.draft, preview);
        assert!(
            preview
                .unwrap()
                .adaptive_engine_process
                .dynamic_resource_zones_enabled
        );
    }

    #[test]
    fn explicit_custom_selection_wins_over_identical_builtin() {
        let mut editor = Editor::default();
        let mut settings = SettingsEditor::with_settings(Settings::default());
        let builtin = BuiltInAdaptiveEnginePreset::Balanced;
        editor.update(&mut settings, Message::BuiltIn(builtin));
        editor.update(&mut settings, Message::New);
        editor.update(&mut settings, Message::Name("My balanced".into()));
        editor.update(&mut settings, Message::Save);
        let options = vec![
            Choice(1, built_in_adaptive_engine_preset_label(builtin)),
            Choice(4, "My balanced".into()),
        ];
        editor.update(&mut settings, Message::Apply(0));
        assert_eq!(
            editor.selected_preset(&settings, &options),
            Some(options[1].clone())
        );
        editor.reset_drafts();
        assert_eq!(
            editor.selected_preset(&settings, &options),
            Some(options[1].clone())
        );
        editor.update(&mut settings, Message::BuiltIn(builtin));
        assert_eq!(
            editor.selected_preset(&settings, &options),
            Some(options[0].clone())
        );
        settings.adaptive_engine_process.reaction_time_ms += 1;
        assert_eq!(editor.selected_preset(&settings, &options), None);
    }

    #[test]
    fn sidebar_delete_removes_only_the_selected_preset() {
        let mut editor = Editor::default();
        let mut settings = SettingsEditor::with_settings(Settings::default());
        for name in ["First", "Second"] {
            editor.update(&mut settings, Message::New);
            editor.update(&mut settings, Message::Name(name.into()));
            editor.update(&mut settings, Message::Save);
        }
        editor.update(&mut settings, Message::Delete(0));
        assert_eq!(settings.adaptive_engine_presets.len(), 1);
        assert_eq!(settings.adaptive_engine_presets[0].name, "Second");
        editor.update(&mut settings, Message::Delete(10));
        assert_eq!(settings.adaptive_engine_presets.len(), 1);
    }

    #[test]
    fn remove_updates_only_the_draft_and_discard_restores_the_rule() {
        let mut original = Settings::default();
        original
            .adaptive_engine_process
            .custom_rules
            .push(ProcessExclusionRule {
                executable_path: r"C:\Apps\test.exe".into(),
                ..Default::default()
            });
        let mut settings = crate::application::SettingsEditor::with_settings(original.clone());
        let mut editor = Editor::default();
        editor.update(&mut settings, Message::RemoveExclusion(0));
        assert!(settings.adaptive_engine_process.custom_rules.is_empty());
        assert_eq!(
            settings.persisted().adaptive_engine_process.custom_rules,
            original.adaptive_engine_process.custom_rules
        );
        settings.cancel();
        assert_eq!(
            settings.adaptive_engine_process.custom_rules,
            original.adaptive_engine_process.custom_rules
        );
    }

    use super::super::adaptive_processor_power::{PROCESSOR_MAX_FIELD, PROCESSOR_MIN_FIELD};
    use super::*;

    #[test]
    fn cpu_behaviour_changes_keep_repaint_regions_bounded() {
        use iced::advanced::Renderer as _;
        use iced::advanced::{layout, widget::Tree};
        let mut renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let mut editor = Editor::default();
        let mut settings = SettingsEditor::with_settings(Settings::default());
        let interrupt_timing = super::super::interrupt_timing::State::default();
        let bounds = iced::Rectangle::with_size(iced::Size::new(1040.0, 700.0));
        for tab in TuningTab::ALL {
            editor.update(&mut settings, Message::TuningTab(tab));
            let mut tree = Tree::empty();
            let mut previous = Vec::new();
            for frame in 0..6 {
                editor.expanded[0][0] = (frame / 2) % 2 == 0;
                settings
                    .adaptive_engine_process
                    .cpu_pressure_restraint_enabled = frame % 2 == 0;
                let mut view = editor.view(&settings, &[], &interrupt_timing);
                tree.diff(view.as_widget());
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, iced::Size::new(1040.0, 700.0)),
                );
                renderer.reset(bounds);
                view.as_widget().draw(
                    &tree,
                    &mut renderer,
                    &iced::Theme::Dark,
                    &Default::default(),
                    iced::advanced::Layout::new(&node),
                    iced::mouse::Cursor::Unavailable,
                    &bounds,
                );
                let damage = if frame == 0 {
                    vec![bounds]
                } else {
                    iced::advanced::graphics::damage::group(
                        iced::advanced::graphics::damage::diff(
                            &previous,
                            renderer.layers(),
                            |l| vec![l.bounds],
                            iced_tiny_skia::Layer::damage,
                        ),
                        bounds,
                    )
                };
                assert!(
                    damage.len() <= 4,
                    "{tab:?}: {} repaint regions",
                    damage.len()
                );
                previous = renderer.layers().to_vec();
            }
        }
    }

    #[test]
    fn cpu_behaviour_scroll_damage_stays_bounded() {
        let mut editor = Editor::default();
        let settings = SettingsEditor::with_settings(Settings::default());
        let interrupt_timing = super::super::interrupt_timing::State::default();
        for expanded in [false, true] {
            editor.expanded[0].fill(expanded);
            for width in [430.0, 1040.0] {
                super::super::scrolling::check_scroll_damage(
                    editor.view(&settings, &[], &interrupt_timing),
                    iced::Rectangle::with_size(iced::Size::new(width, 700.0)),
                );
            }
        }
    }

    #[test]
    fn tuning_navigation_keeps_live_and_preset_state_separate() {
        let mut editor = Editor::default();
        let mut settings = SettingsEditor::with_settings(Settings::default());
        let before = settings.clone();
        editor.update(&mut settings, Message::TuningTab(TuningTab::CustomRules));
        editor.update(&mut settings, Message::Collapse(0));
        assert_eq!(settings.global(), &before);
        editor.update(
            &mut settings,
            Message::ViewBuiltIn(BuiltInAdaptiveEnginePreset::Balanced),
        );
        assert_eq!(editor.tuning_tabs[1], TuningTab::CpuBehaviour);
        assert_eq!(editor.expanded[1], [false; 3]);
        editor.update(&mut settings, Message::TuningTab(TuningTab::CustomRules));
        assert_eq!(editor.tuning_tabs[1], TuningTab::CpuBehaviour);
        editor.update(&mut settings, Message::Collapse(1));
        editor.update(&mut settings, Message::Cancel);
        assert_eq!(editor.tuning_tabs[0], TuningTab::CustomRules);
        assert_eq!(editor.expanded[0], [true, false, false]);
        assert_eq!(settings.global(), &before);
    }

    #[test]
    fn independent_cpu_gates_and_preset_isolation() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        let mut editor = Editor::default();
        settings
            .adaptive_engine_process
            .limit_background_processors_enabled = true;
        editor.update(
            &mut settings,
            Message::Toggle(
                |s, v| s.adaptive_engine_process.cpu_pressure_restraint_enabled = v,
                false,
            ),
        );
        assert!(
            settings
                .adaptive_engine_process
                .limit_background_processors_enabled
        );
        let exclusion = ProcessExclusionRule {
            executable_path: r"C:\App\app.exe".into(),
            ..Default::default()
        };
        settings
            .adaptive_engine_process
            .custom_rules
            .push(exclusion.clone());
        editor.update(&mut settings, Message::New);
        editor.update(
            &mut settings,
            Message::Toggle(
                |s, v| {
                    s.adaptive_engine_process
                        .limit_background_processors_enabled = v
                },
                false,
            ),
        );
        assert!(
            settings
                .adaptive_engine_process
                .limit_background_processors_enabled
        );
        editor.update(
            &mut settings,
            Message::Toggle(
                |s, v| s.adaptive_engine_process.cpu_pressure_restraint_enabled = v,
                true,
            ),
        );
        assert!(
            !settings
                .adaptive_engine_process
                .cpu_pressure_restraint_enabled
        );
        editor.update(&mut settings, Message::Name("Saved".into()));
        editor.update(&mut settings, Message::Save);
        editor.update(&mut settings, Message::Apply(0));
        assert!(!settings.adaptive_engine.enabled);
        assert!(
            settings
                .adaptive_engine_process
                .cpu_pressure_restraint_enabled
        );
        assert_eq!(
            settings.adaptive_engine_process.custom_rules,
            vec![exclusion]
        );
        assert!(
            !settings
                .adaptive_engine_process
                .limit_background_processors_enabled
        );
        editor.update(&mut settings, Message::RemoveExclusion(0));
        assert!(settings.adaptive_engine_process.custom_rules.is_empty());
    }
    #[test]
    fn editing_process_preset_preserves_its_stored_processor_power() {
        let mut saved = capture_adaptive_engine_preset(&Settings::default(), "Saved".into());
        saved.processor_power_policy_enabled = true;
        saved.base_processor_policy.performance_min = 79;
        saved.background_pressure_profile.ac_policy = 23;
        saved.focus_and_launch_profile.battery_policy = 61;
        let mut live = Settings::default();
        live.adaptive_engine.base_processor_policy.performance_min = 12;
        live.adaptive_engine_presets.push(saved.clone());
        let mut settings = SettingsEditor::with_settings(live);
        let mut editor = Editor::default();
        for message in [
            Message::Edit(0),
            Message::UseCurrent,
            Message::Name("Renamed".into()),
            Message::Number(
                |settings, value| settings.adaptive_engine_process.reaction_time_ms = value,
                "1900".into(),
                250,
                5000,
                "reaction",
                "adaptive_engine_process.reaction_time",
            ),
            Message::Save,
        ] {
            editor.update(&mut settings, message);
        }
        saved.name = "Renamed".into();
        saved.adaptive_engine_process =
            capture_adaptive_engine_preset(&settings, String::new()).adaptive_engine_process;
        saved.adaptive_engine_process.reaction_time_ms = 1900;
        assert_eq!(settings.global().adaptive_engine_presets, vec![saved]);
        assert_eq!(
            settings
                .adaptive_engine
                .base_processor_policy
                .performance_min,
            12
        );
    }

    #[test]
    fn processor_power_toggle_is_independent_and_profile_scoped() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        let mut editor = Editor::default();
        assert!(settings.global().on_battery.is_none());
        let enabled = |value| {
            Message::ProcessorPower(super::super::adaptive_processor_power::Message::Enabled(
                value,
            ))
        };
        editor.update(&mut settings, enabled(false));
        assert!(!settings.has_unsaved_changes());
        assert!(settings.global().on_battery.is_none());
        editor.update(&mut settings, enabled(true));
        assert!(!settings.adaptive_engine.enabled);
        assert!(settings.adaptive_engine.adaptive_processor_power_enabled);
        assert!(settings.global().on_battery.is_some());
        settings.select_power_source(PowerSourceProfile::OnBattery);
        assert!(!settings.adaptive_engine.adaptive_processor_power_enabled);
        editor.update(&mut settings, enabled(true));
        assert!(!settings.adaptive_engine.enabled);
        settings.select_power_source(PowerSourceProfile::PluggedIn);
        assert!(settings.adaptive_engine.adaptive_processor_power_enabled);
        editor.update(&mut settings, enabled(false));
        settings.select_power_source(PowerSourceProfile::OnBattery);
        assert!(settings.adaptive_engine.adaptive_processor_power_enabled);
    }

    #[test]
    fn processor_max_draft_is_rechecked_when_minimum_changes() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        settings
            .adaptive_engine
            .base_processor_policy
            .performance_min = 25;
        settings
            .adaptive_engine
            .base_processor_policy
            .performance_max = 100;
        let mut editor = Editor::default();
        let set_min = |text: &str| {
            Message::ProcessorPower(super::super::adaptive_processor_power::Message::Number(
                |settings, value| settings.base_processor_policy.performance_min = value as u32,
                text.into(),
                0,
                100,
                PROCESSOR_MIN_FIELD,
                "processor_power.processor_min",
            ))
        };
        let set_max = |text: &str| {
            Message::ProcessorPower(super::super::adaptive_processor_power::Message::Number(
                |settings, value| settings.base_processor_policy.performance_max = value as u32,
                text.into(),
                0,
                100,
                PROCESSOR_MAX_FIELD,
                "processor_power.processor_max",
            ))
        };
        let policy = |settings: &Settings| settings.adaptive_engine.base_processor_policy;

        editor.update(&mut settings, set_max("10"));
        assert_eq!(policy(&settings).performance_max, 100);
        assert_eq!(
            editor.processor_power_numbers.invalid(),
            Some((25, 100, "processor_power.processor_max"))
        );
        editor.update(&mut settings, set_min("5"));
        assert_eq!(policy(&settings).performance_max, 10);
        assert_eq!(
            editor.processor_power_numbers.drafts[PROCESSOR_MAX_FIELD].text,
            "10"
        );
        assert!(editor.processor_power_numbers.invalid().is_none());

        editor.update(&mut settings, set_min("15"));
        assert_eq!(policy(&settings).performance_max, 15);
        assert!(!editor
            .processor_power_numbers
            .drafts
            .contains_key(PROCESSOR_MAX_FIELD));
        editor.update(&mut settings, set_max(""));
        editor.update(&mut settings, set_min("20"));
        assert_eq!(policy(&settings).performance_max, 20);
        assert_eq!(
            editor.processor_power_numbers.drafts[PROCESSOR_MAX_FIELD].text,
            ""
        );
        assert_eq!(
            editor.processor_power_numbers.invalid(),
            Some((20, 100, "processor_power.processor_max"))
        );
    }

    #[test]
    fn process_preset_keeps_unfinished_processor_power_edit() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        let mut editor = Editor::default();
        editor.update(
            &mut settings,
            Message::ProcessorPower(super::super::adaptive_processor_power::Message::Number(
                |settings, value| settings.base_processor_policy.performance_max = value as u32,
                String::new(),
                0,
                100,
                PROCESSOR_MAX_FIELD,
                "processor_power.processor_max",
            )),
        );
        let power_before = settings.adaptive_engine.clone();
        editor.update(
            &mut settings,
            Message::BuiltIn(BuiltInAdaptiveEnginePreset::Speed),
        );
        assert_eq!(settings.adaptive_engine, power_before);
        assert_eq!(
            editor.processor_power_numbers.drafts[PROCESSOR_MAX_FIELD].text,
            ""
        );
        assert!(editor.validation_error().is_some());
        assert!(editor.live_numbers.drafts.is_empty());
    }

    #[test]
    fn preset_numbers_are_independent_of_unfinished_live_edits() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        let mut editor = Editor::default();
        let threshold = |text: &str| {
            Message::Number(
                |s, value| {
                    s.adaptive_engine_process
                        .background_app_cpu_threshold_percent = value as u8
                },
                text.into(),
                1,
                100,
                "threshold",
                "adaptive_engine_process.background_app_cpu_threshold",
            )
        };
        for message in [
            threshold(""),
            Message::New,
            Message::Name("Custom".into()),
            threshold("35"),
            Message::Save,
        ] {
            editor.update(&mut settings, message);
        }
        assert!(editor.draft.is_none());
        assert!(editor.preset_numbers.drafts.is_empty());
        assert!(editor.validation_error().is_some());
        assert_eq!(editor.live_numbers.drafts["threshold"].text, "");
        assert_eq!(
            settings
                .adaptive_engine_process
                .background_app_cpu_threshold_percent,
            30
        );
        assert_eq!(
            settings.adaptive_engine_presets[0]
                .adaptive_engine_process
                .background_app_cpu_threshold_percent,
            35
        );

        for message in [
            threshold("31"),
            Message::Edit(0),
            threshold(""),
            Message::Delete(0),
        ] {
            editor.update(&mut settings, message);
        }
        assert!(settings.adaptive_engine_presets.is_empty());
        assert!(editor.preset_numbers.drafts.is_empty());
        assert!(editor.validation_error().is_none());
        assert_eq!(editor.live_numbers.drafts["threshold"].text, "31");
    }

    #[test]
    fn read_only_builtin_and_invalid_numbers_do_not_mutate() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        settings.select_power_source(PowerSourceProfile::OnBattery);
        let old = settings.clone();
        let mut editor = Editor::default();
        editor.update(
            &mut settings,
            Message::Number(
                |s, n| s.adaptive_engine_process.reaction_time_ms = n,
                "1".into(),
                250,
                5000,
                "reaction",
                "adaptive_engine_process.reaction_time",
            ),
        );
        editor.update(
            &mut settings,
            Message::ViewBuiltIn(BuiltInAdaptiveEnginePreset::Speed),
        );
        assert!(editor.preset_open());
        assert!(!editor.has_pending_editor());
        let draft = editor.draft.clone();
        editor.update(
            &mut settings,
            Message::Toggle(
                |s, v| s.adaptive_engine_process.dynamic_resource_zones_enabled = v,
                true,
            ),
        );
        assert_eq!(editor.draft, draft);
        assert_eq!(settings.global(), &old);
    }
}
