use super::design;
use super::priority_control::{
    process_dynamic_priority_boost_setting_label, process_gpu_priority_setting_label,
    process_io_priority_setting_label, process_memory_priority_setting_label,
    process_priority_setting_label, process_thread_priority_setting_label,
};
use super::widgets::{button, checkbox, pick_list, text_input};
use crate::automation::RuntimeStatusSnapshot;
use crate::config::*;
use crate::power::ProcessorBoostMode;
use crate::ui::scrolling::scrollable;
use iced::widget::{column, row, text};
use iced::{Element, Fill};
use rust_i18n::t;
#[path = "adaptive_presets.rs"]
mod presets;
#[cfg(feature = "render-smoke")]
pub(super) use presets::BuiltInAdaptiveEnginePreset;
use presets::*;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum TuningTab {
    #[default]
    CpuBehaviour,
    ProcessorPower,
    PriorityControl,
    CustomRules,
}
impl TuningTab {
    const ALL: [Self; 4] = [
        Self::CpuBehaviour,
        Self::ProcessorPower,
        Self::PriorityControl,
        Self::CustomRules,
    ];
    fn key(self) -> &'static str {
        match self {
            Self::CpuBehaviour => "adaptive_engine.cpu_behaviour",
            Self::ProcessorPower => "adaptive_engine.processor_power",
            Self::PriorityControl => "adaptive_engine.priority_control",
            Self::CustomRules => "adaptive_engine_process.custom_rules",
        }
    }
}
#[derive(Default)]
pub(super) struct Editor {
    name: String,
    selected_preset: Option<Choice<usize>>,
    draft: Option<Settings>,
    editing: Option<usize>,
    read_only: bool,
    presets_tab: bool,
    tuning_tabs: [TuningTab; 2],
    expanded: [[bool; 3]; 2],
    priority_expanded: [[bool; 7]; 2],
    path: String,
    error: String,
    numbers: std::collections::HashMap<&'static str, String>,
    invalid_numbers: std::collections::HashMap<&'static str, (u64, u64, &'static str)>,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Toggle(fn(&mut Settings, bool), bool),
    RailTab(bool),
    TuningTab(TuningTab),
    Collapse(usize),
    TogglePriority(usize),
    Status(super::status_rail::Message),
    Number(
        fn(&mut Settings, u64),
        String,
        u64,
        u64,
        &'static str,
        &'static str,
    ),
    Choice(fn(&mut Settings, usize), usize),
    Mask(u64),
    ZoneMask(u64),
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
    pub(super) fn update(&mut self, s: &mut Settings, m: Message) {
        if matches!(
            m,
            Message::BuiltIn(_)
                | Message::ViewBuiltIn(_)
                | Message::Apply(_)
                | Message::Edit(_)
                | Message::New
                | Message::Cancel
                | Message::UseCurrent
        ) {
            self.error.clear();
            self.numbers.clear();
            self.invalid_numbers.clear();
        }
        match m {
            Message::TuningTab(tab) => {
                if self.draft.is_none() || tab != TuningTab::CustomRules {
                    self.tuning_tabs[usize::from(self.draft.is_some())] = tab;
                }
            }
            Message::TogglePriority(index) => {
                if let Some(expanded) =
                    self.priority_expanded[usize::from(self.draft.is_some())].get_mut(index)
                {
                    *expanded = !*expanded;
                }
            }
            Message::Collapse(index) => {
                if let Some(value) = self.expanded[usize::from(self.draft.is_some())].get_mut(index)
                {
                    *value = !*value;
                }
            }
            Message::RailTab(value) => self.presets_tab = value,
            Message::Status(_) => {}
            Message::BuiltIn(p) => {
                self.selected_preset = BuiltInAdaptiveEnginePreset::ALL
                    .iter()
                    .position(|v| *v == p)
                    .map(|i| Choice(i, built_in_adaptive_engine_preset_label(p)));
                let mut draft = s.clone();
                apply_built_in_adaptive_engine_preset(&mut draft, p);
                let preset = capture_adaptive_engine_preset(&draft, String::new());
                apply_adaptive_engine_preset(s, &preset);
            }
            Message::ViewBuiltIn(p) => {
                let mut draft = s.clone();
                apply_built_in_adaptive_engine_preset(&mut draft, p);
                self.draft = Some(draft);
                self.presets_tab = true;
                self.read_only = true;
                self.name = built_in_adaptive_engine_preset_label(p);
            }
            Message::Apply(i) => {
                if let Some(p) = s.adaptive_engine_presets.get(i).cloned() {
                    self.selected_preset = Some(Choice(i + 4, p.name.clone()));
                    apply_adaptive_engine_preset(s, &p)
                }
            }
            Message::Edit(i) => {
                if let Some(p) = s.adaptive_engine_presets.get(i) {
                    let mut draft = s.clone();
                    apply_adaptive_engine_preset(&mut draft, p);
                    self.name = p.name.clone();
                    self.editing = Some(i);
                    self.read_only = false;
                    self.draft = Some(draft);
                    self.presets_tab = true;
                }
            }
            Message::New => {
                self.draft = Some(s.clone());
                self.presets_tab = true;
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
                        && self.invalid_numbers.is_empty()
                        && !name.is_empty()
                        && !s.adaptive_engine_presets.iter().enumerate().any(|(i, p)| {
                            Some(i) != self.editing && p.name.trim().eq_ignore_ascii_case(name)
                        })
                    {
                        let p = capture_adaptive_engine_preset(draft, name.into());
                        if let Some(i) = self.editing {
                            if let Some(old) = s.adaptive_engine_presets.get_mut(i) {
                                *old = p;
                            }
                        } else {
                            s.adaptive_engine_presets.push(p);
                        }
                        self.draft = None;
                    } else {
                        self.error = t!("adaptive_engine.duplicate_preset_name").to_string();
                    }
                }
            }
            Message::Delete(index) => {
                if index < s.adaptive_engine_presets.len() {
                    s.adaptive_engine_presets.remove(index);
                }
                self.draft = None;
            }
            Message::Cancel => {
                self.draft = None;
                self.error.clear();
            }
            Message::UseCurrent => {
                if !self.read_only {
                    self.draft = Some(s.clone())
                }
            }
            Message::Path(v) => self.path = v,
            Message::Browse => {}
            Message::AddExclusion => {
                if crate::ui::process_rules::can_add_process_candidate(
                    &self.path,
                    |p| {
                        s.adaptive_engine_process.custom_rules.iter().any(|r| {
                            crate::ui::process_rules::process_setting_matches(&r.executable_path, p)
                        })
                    },
                    crate::adaptive_engine_process::is_builtin_excluded,
                ) {
                    s.adaptive_engine_process
                        .custom_rules
                        .push(ProcessExclusionRule {
                            executable_path: crate::foreground::executable_path_key(
                                std::path::Path::new(&self.path),
                            ),
                            ..Default::default()
                        });
                    self.path.clear();
                }
            }
            Message::RemoveExclusion(i) => {
                if i < s.adaptive_engine_process.custom_rules.len() {
                    s.adaptive_engine_process.custom_rules.remove(i);
                }
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
                let target = self.draft.as_mut().unwrap_or(s);
                match edit {
                    Message::Toggle(f, v) => f(target, v),
                    Message::Number(f, v, min, max, key, label) => {
                        self.numbers.insert(key, v.clone());
                        if v.parse::<u64>().is_ok_and(|n| (min..=max).contains(&n)) {
                            self.invalid_numbers.remove(key);
                        } else {
                            self.invalid_numbers.insert(key, (min, max, label));
                        }
                        if let Ok(n) = v.parse::<u64>() {
                            if (min..=max).contains(&n) {
                                let before = target.adaptive_engine.base_processor_policy;
                                f(target, n);
                                target.adaptive_engine.base_processor_policy =
                                    target.adaptive_engine.base_processor_policy.normalized();
                                let after = target.adaptive_engine.base_processor_policy;
                                for (field, changed) in [
                                    (
                                        stringify!(
                                            adaptive_engine.base_processor_policy.performance_min
                                        ),
                                        before.performance_min != after.performance_min,
                                    ),
                                    (
                                        stringify!(
                                            adaptive_engine.base_processor_policy.performance_max
                                        ),
                                        before.performance_max != after.performance_max,
                                    ),
                                ] {
                                    if field != key && changed {
                                        self.numbers.remove(field);
                                        self.invalid_numbers.remove(field);
                                    }
                                }
                            }
                        }
                    }
                    Message::Choice(f, v) => f(target, v),
                    Message::ZoneMask(mask) => {
                        target
                            .adaptive_engine_process
                            .dynamic_resource_zone_settings
                            .specific_processors =
                            (0..64).filter(|i| mask & (1u64 << i) != 0).collect()
                    }
                    Message::Mask(mask) => {
                        target.adaptive_engine_process.specific_processors =
                            (0..64).filter(|i| mask & (1u64 << i) != 0).collect()
                    }
                    _ => {}
                }
            }
        }
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
        self.invalid_numbers
            .values()
            .next()
            .map(|(min, max, label)| format!("{}: {min}..{max}", t!(*label)))
    }
    pub(super) fn has_pending_editor(&self) -> bool {
        self.draft.is_some()
    }
    pub(super) fn view<'a>(
        &'a self,
        live: &'a Settings,
        candidates: &'a [super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        self.tuning_view(live, false, candidates)
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
            button(super::navigation::glyph("icons/x.svg")).on_press(Message::Cancel)
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
                button(text(t!("common.remove").to_string()))
                    .style(super::widgets::danger_button)
                    .on_press(Message::Delete(index)),
            );
        }
        if !self.read_only {
            footer = footer.push(
                button(text(t!("adaptive_engine.use_current_settings").to_string()))
                    .on_press(Message::UseCurrent),
            );
        }
        footer = footer.push(
            button(text(t!("common.cancel").to_string()))
                .style(super::widgets::tertiary_button)
                .on_press(Message::Cancel),
        );
        if !self.read_only {
            footer = footer.push(
                button(text(t!("common.save").to_string()))
                    .style(super::widgets::primary_button)
                    .on_press_maybe(
                        (!self.name.trim().is_empty() && self.invalid_numbers.is_empty())
                            .then_some(Message::Save),
                    ),
            );
        }
        let mut body = column![name, self.tuning_view(live, true, candidates)]
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
        preset: bool,
        candidates: &'a [super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        let s = if preset {
            self.draft.as_ref().unwrap_or(live)
        } else {
            live
        };
        let editable = !preset || !self.read_only;
        let mut body = column![].spacing(super::widgets::CARD_GAP);
        macro_rules! toggle {($key:expr,$($field:ident).+) => {row![
            setting_label($key).width(Fill),
            super::widgets::switch(s.$($field).+, editable.then_some(|v|Message::Toggle(|s,v|s.$($field).+ = v,v)))
        ].spacing(design::space::SMALL).height(super::widgets::SETTING_ROW_HEIGHT).align_y(iced::Center)};}
        macro_rules! number {($key:expr,$min:expr,$max:expr,$($field:ident).+) => {{
            let key=stringify!($($field).+);
            let value=self.numbers.get(key).cloned().unwrap_or_else(||s.$($field).+.to_string());
            let unit=if key.ends_with("_ms") {"ms"} else if key.ends_with("_seconds") {"s"} else if key.ends_with("_percent") || $max==100 {"%"} else {""};
            row![setting_label_with_unit($key, unit).width(Fill),
                super::widgets::stepper(&value, $min..=$max, 1,
                    editable.then_some(move|v|Message::Number(|s,n|s.$($field).+ = n as _,v,$min,$max,key,$key)))
            ].spacing(design::space::SMALL).height(46).align_y(iced::Center)
        }};}
        macro_rules! selector {($s:ident,$key:expr,$ty:ty,$options:expr,$label:expr,$($field:ident).+ $(, $color:expr)?) => {{let values:&[$ty]=$options;let selected=s.$($field).+;let control:Element<'_,Message>=if editable{pick_list(values.iter().copied().map(|v|Choice(v,$label(v))).collect::<Vec<_>>(),Some(Choice(selected,$label(selected))),move |v|Message::Choice(|$s,i|{let options:&[$ty]=$options;if let Some(v)=options.get(i){$s.$($field).+ = *v;}},values.iter().position(|x|*x==v.0).unwrap_or(0)))$(.option_color($color))?.width(Fill).into()}else{text($label(selected)).into()};control}};}
        macro_rules! choice {($s:ident,$key:expr,$ty:ty,$options:expr,$label:expr,$($field:ident).+) => {row![setting_label($key).width(Fill),iced::widget::container(selector!($s,$key,$ty,$options,$label,$($field).+)).width(280)].spacing(design::space::SMALL).height(46).align_y(iced::Center)};}
        if !preset {
            body = body.push(super::widgets::settings_card(toggle!(
                "adaptive_engine.enable",
                adaptive_engine.enabled
            )));
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
        let mut tabs = row![].spacing(design::space::TIGHT);
        for next in TuningTab::ALL
            .into_iter()
            .filter(|tab| !preset || *tab != TuningTab::CustomRules)
        {
            tabs = tabs.push(
                super::widgets::panel_tab(
                    t!(next.key()).to_string(),
                    next == tab,
                    Message::TuningTab(next),
                    false,
                )
                .height(36),
            );
        }
        body = body.push(
            iced::widget::container(tabs)
                .padding(design::space::TIGHT as u16)
                .width(Fill)
                .style(super::widgets::surface),
        );
        let header = body;
        let mut body = column![].spacing(super::widgets::CARD_GAP);
        match tab {
            TuningTab::CpuBehaviour => {
                let pressure = column![
                    number!(
                        "adaptive_engine_process.background_app_cpu_threshold",
                        1,
                        100,
                        adaptive_engine_process.background_app_cpu_threshold_percent
                    ),
                    number!(
                        "adaptive_engine_process.maximum_restrained_apps",
                        1,
                        64,
                        adaptive_engine_process.maximum_restrained_apps
                    ),
                    number!(
                        "adaptive_engine_process.reaction_time",
                        250,
                        5000,
                        adaptive_engine_process.reaction_time_ms
                    ),
                    number!(
                        "adaptive_engine_process.foreground_or_system_cpu_threshold",
                        1,
                        100,
                        adaptive_engine_process.foreground_or_system_cpu_threshold_percent
                    ),
                    number!(
                        "adaptive_engine_process.cpu_restraint_time",
                        1,
                        3600,
                        adaptive_engine_process.cpu_restraint_time_seconds
                    ),
                    number!(
                        "adaptive_engine_process.cpu_recovery_threshold",
                        1,
                        100,
                        adaptive_engine_process.cpu_recovery_threshold_percent
                    ),
                    number!(
                        "adaptive_engine_process.cpu_recovery_time",
                        1,
                        3600,
                        adaptive_engine_process.cpu_recovery_time_seconds
                    )
                ]
                .spacing(design::space::MEDIUM);
                body = body
                    .push(text(
                        t!("adaptive_engine_process.shared_detection").to_string(),
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
                let mut allocation = column![
                    choice!(
                        s,
                        "adaptive_engine_process.processor_selection",
                        BackgroundProcessorSelection,
                        &BackgroundProcessorSelection::ALL,
                        background_processor_selection_label,
                        adaptive_engine_process.background_processor_selection
                    ),
                    choice!(
                        s,
                        "adaptive_engine_process.cpu_allocation_method",
                        CpuAllocationMethod,
                        &CpuAllocationMethod::ALL,
                        allocation_label,
                        adaptive_engine_process.cpu_allocation_method
                    )
                ]
                .spacing(design::space::MEDIUM);
                if s.adaptive_engine_process
                    .background_processor_selection
                    .uses_percentage()
                {
                    allocation = allocation.push(number!(
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
                    let mask = s
                        .adaptive_engine_process
                        .specific_processors
                        .iter()
                        .filter(|i| **i < 64)
                        .fold(0u64, |m, i| m | (1u64 << i));
                    allocation = allocation.push(super::cpu_allocation::mask_selector(
                        mask,
                        &crate::cpu_allocation::logical_processors(),
                        &s.cpu_allocation_presets,
                        Message::Mask,
                    ));
                }
                if s.adaptive_engine_process.background_processor_selection
                    == BackgroundProcessorSelection::Custom
                    && !editable
                {
                    allocation = allocation.push(text(format!(
                        "{}: {:?}",
                        t!("adaptive_engine_process.specific_processors"),
                        s.adaptive_engine_process.specific_processors
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
                let mut zone_content = column![
                    text(t!("adaptive_engine_process.zone_warning").to_string())
                        .style(text::secondary),
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
                ]
                .spacing(design::space::MEDIUM);
                if zones.background_processor_selection.uses_percentage() {
                    zone_content = zone_content.push(number!(
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
                        zone_content = zone_content.push(super::cpu_allocation::mask_selector(
                            crate::cpu_allocation::logical_processor_indices_mask(
                                &zones.specific_processors,
                            ),
                            &crate::cpu_allocation::logical_processors(),
                            &s.cpu_allocation_presets,
                            Message::ZoneMask,
                        ));
                    } else {
                        zone_content = zone_content.push(text(format!(
                            "{}: {:?}",
                            t!("adaptive_engine_process.specific_processors"),
                            zones.specific_processors
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
            TuningTab::ProcessorPower => {
                body = body.push(super::widgets::settings_card(toggle!(
                    "adaptive_engine.processor_power_policy",
                    adaptive_engine.processor_power_policy_enabled
                )));
                body = body.push(
                    text(t!("adaptive_engine.base_processor_policy").to_string())
                        .size(design::typography::SECTION),
                );
                body = body.push(super::widgets::settings_card(number!(
                    "processor_power.core_parking_min",
                    0,
                    100,
                    adaptive_engine.base_processor_policy.core_parking_min
                )));
                body = body.push(super::widgets::settings_card(number!(
                    "processor_power.processor_min",
                    0,
                    100,
                    adaptive_engine.base_processor_policy.performance_min
                )));
                body = body.push(super::widgets::settings_card(number!(
                    "processor_power.processor_max",
                    0,
                    100,
                    adaptive_engine.base_processor_policy.performance_max
                )));
                body = body.push(super::widgets::settings_card(number!(
                    "processor_power.boost_policy",
                    0,
                    100,
                    adaptive_engine.base_processor_policy.boost_policy
                )));
                body = body.push(super::widgets::settings_card(choice!(
                    s,
                    "processor_power.boost_mode",
                    ProcessorBoostMode,
                    &ProcessorBoostMode::ALL,
                    boost_label,
                    adaptive_engine.base_processor_policy.boost_mode
                )));

                body = body.push(
                    text(t!("adaptive_engine.background_pressure_profile").to_string())
                        .size(design::typography::SECTION),
                );
                body = body.push(super::widgets::settings_card(number!(
                    "adaptive_engine.ac_boost_policy",
                    0,
                    100,
                    adaptive_engine.background_pressure_profile.ac_policy
                )));
                body = body.push(super::widgets::settings_card(choice!(
                    s,
                    "adaptive_engine.ac_boost_mode",
                    ProcessorBoostMode,
                    &ProcessorBoostMode::ALL,
                    boost_label,
                    adaptive_engine.background_pressure_profile.ac_mode
                )));
                body = body.push(super::widgets::settings_card(number!(
                    "adaptive_engine.battery_boost_policy",
                    0,
                    100,
                    adaptive_engine.background_pressure_profile.battery_policy
                )));
                body = body.push(super::widgets::settings_card(choice!(
                    s,
                    "adaptive_engine.battery_boost_mode",
                    ProcessorBoostMode,
                    &ProcessorBoostMode::ALL,
                    boost_label,
                    adaptive_engine.background_pressure_profile.battery_mode
                )));

                body = body.push(
                    text(t!("adaptive_engine.focus_and_launch_profile").to_string())
                        .size(design::typography::SECTION),
                );
                body = body.push(super::widgets::settings_card(number!(
                    "adaptive_engine.ac_boost_policy",
                    0,
                    100,
                    adaptive_engine.focus_and_launch_profile.ac_policy
                )));
                body = body.push(super::widgets::settings_card(choice!(
                    s,
                    "adaptive_engine.ac_boost_mode",
                    ProcessorBoostMode,
                    &ProcessorBoostMode::ALL,
                    boost_label,
                    adaptive_engine.focus_and_launch_profile.ac_mode
                )));
                body = body.push(super::widgets::settings_card(number!(
                    "adaptive_engine.battery_boost_policy",
                    0,
                    100,
                    adaptive_engine.focus_and_launch_profile.battery_policy
                )));
                body = body.push(super::widgets::settings_card(choice!(
                    s,
                    "adaptive_engine.battery_boost_mode",
                    ProcessorBoostMode,
                    &ProcessorBoostMode::ALL,
                    boost_label,
                    adaptive_engine.focus_and_launch_profile.battery_mode
                )));
            }
            TuningTab::PriorityControl => {
                let mut table = column![row![
                    text(t!("common.control").to_string()).width(Fill),
                    row![
                        text(t!("common.enable").to_string()).width(64),
                        text(t!("common.focus_process").to_string()).width(Fill),
                        text(t!("common.visible_window").to_string()).width(Fill),
                        text(t!("common.background_process").to_string()).width(Fill),
                    ]
                    .spacing(design::space::SMALL)
                    .width(iced::Length::FillPortion(3)),
                    iced::widget::Space::new().width(design::ICON_SIZE),
                ]
                .spacing(design::space::SMALL)
                .padding(super::widgets::CARD_PADDING as u16)]
                .spacing(super::widgets::CARD_GAP);
                table = table.push(super::widgets::setting_group(
                    "nav.process_priority".to_string(),
                    self.priority_expanded[usize::from(preset)][0],
                    Message::TogglePriority(0),
                    row![
                        iced::widget::container(super::widgets::switch(
                            s.adaptive_engine_process.process_priority_enabled,
                            editable.then_some(|v| {
                                Message::Toggle(
                                    |s, v| s.adaptive_engine_process.process_priority_enabled = v,
                                    v,
                                )
                            }),
                        ))
                        .width(64),
                        iced::widget::container(selector!(
                            s,
                            "cpu_allocation.focus",
                            ProcessPrioritySetting,
                            if s.advanced.expose_all_priority_values {
                                &ProcessPrioritySetting::ADVANCED_ALL
                            } else {
                                &ProcessPrioritySetting::ALL
                            },
                            process_priority_setting_label,
                            adaptive_engine_process.focus_process_priority,
                            |value, theme| super::priority_control::Value::Process(value.0)
                                .color(theme)
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.visible_window",
                            ProcessPrioritySetting,
                            if s.advanced.expose_all_priority_values {
                                &ProcessPrioritySetting::ADVANCED_ALL
                            } else {
                                &ProcessPrioritySetting::ALL
                            },
                            process_priority_setting_label,
                            adaptive_engine_process.visible_window_priority,
                            |value, theme| super::priority_control::Value::Process(value.0)
                                .color(theme)
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.background_process",
                            ProcessPrioritySetting,
                            if s.advanced.expose_all_priority_values {
                                &ProcessPrioritySetting::ADVANCED_ALL
                            } else {
                                &ProcessPrioritySetting::ALL
                            },
                            process_priority_setting_label,
                            adaptive_engine_process.background_priority,
                            |value, theme| super::priority_control::Value::Process(value.0)
                                .color(theme)
                        ))
                        .width(Fill)
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                    .width(iced::Length::FillPortion(3)),
                    column![
                        priority_option_row(
                            "adaptive_engine.detection",
                            [
                                checkbox(
                                    s.adaptive_engine_process
                                        .process_priority_foreground_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .process_priority_foreground_detection_enabled = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.adaptive_engine_process
                                        .process_priority_visible_window_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .process_priority_visible_window_detection_enabled = v
                                    },
                                    v
                                )))
                                .into(),
                                text("\u{2014}").style(iced::widget::text::secondary).into()
                            ]
                        ),
                        priority_option_row(
                            "adaptive_engine.keep_existing_priority",
                            [
                                checkbox(
                                    s.adaptive_engine_process
                                        .process_priority_preserve_foreground
                                )
                                .label(t!("adaptive_engine.same_or_higher").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .process_priority_preserve_foreground = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.adaptive_engine_process
                                        .process_priority_preserve_visible_window
                                )
                                .label(t!("adaptive_engine.same_or_higher").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .process_priority_preserve_visible_window = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.adaptive_engine_process
                                        .process_priority_preserve_background
                                )
                                .label(t!("adaptive_engine.same_or_lower").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .process_priority_preserve_background = v
                                    },
                                    v
                                )))
                                .into()
                            ]
                        )
                    ]
                    .spacing(design::space::SMALL),
                ));
                table = table.push(super::widgets::setting_group(
                    "nav.background_efficiency".to_string(),
                    self.priority_expanded[usize::from(preset)][1],
                    Message::TogglePriority(1),
                    row![
                        iced::widget::container(super::widgets::switch(
                            s.adaptive_engine_process.background_efficiency_enabled,
                            editable.then_some(|v| {
                                Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process.background_efficiency_enabled = v
                                    },
                                    v,
                                )
                            }),
                        ))
                        .width(64),
                        iced::widget::container(
                            pick_list(
                                [
                                    Choice(false, t!("common.disabled").to_string()),
                                    Choice(true, t!("common.enabled").to_string())
                                ],
                                Some(Choice(
                                    s.adaptive_engine_process
                                        .focus_process_background_efficiency_mode,
                                    t!(if s
                                        .adaptive_engine_process
                                        .focus_process_background_efficiency_mode
                                    {
                                        "common.enabled"
                                    } else {
                                        "common.disabled"
                                    })
                                    .to_string()
                                )),
                                |v| Message::Toggle(
                                    |s, v| s
                                        .adaptive_engine_process
                                        .focus_process_background_efficiency_mode = v,
                                    v.0
                                )
                            )
                            .width(Fill)
                        )
                        .width(Fill),
                        iced::widget::container(
                            pick_list(
                                [
                                    Choice(false, t!("common.disabled").to_string()),
                                    Choice(true, t!("common.enabled").to_string())
                                ],
                                Some(Choice(
                                    s.adaptive_engine_process
                                        .visible_window_background_efficiency_mode,
                                    t!(if s
                                        .adaptive_engine_process
                                        .visible_window_background_efficiency_mode
                                    {
                                        "common.enabled"
                                    } else {
                                        "common.disabled"
                                    })
                                    .to_string()
                                )),
                                |v| Message::Toggle(
                                    |s, v| s
                                        .adaptive_engine_process
                                        .visible_window_background_efficiency_mode = v,
                                    v.0
                                )
                            )
                            .width(Fill)
                        )
                        .width(Fill),
                        iced::widget::container(
                            pick_list(
                                [
                                    Choice(false, t!("common.disabled").to_string()),
                                    Choice(true, t!("common.enabled").to_string())
                                ],
                                Some(Choice(
                                    s.adaptive_engine_process.background_efficiency_mode,
                                    t!(if s.adaptive_engine_process.background_efficiency_mode {
                                        "common.enabled"
                                    } else {
                                        "common.disabled"
                                    })
                                    .to_string()
                                )),
                                |v| Message::Toggle(
                                    |s, v| s.adaptive_engine_process.background_efficiency_mode = v,
                                    v.0
                                )
                            )
                            .width(Fill)
                        )
                        .width(Fill)
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                    .width(iced::Length::FillPortion(3)),
                    column![priority_option_row(
                        "adaptive_engine.detection",
                        [
                            checkbox(
                                s.adaptive_engine_process
                                    .focus_process_background_efficiency_override_enabled
                            )
                            .label(t!("common.enabled").to_string())
                            .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                |s, v| {
                                    s.adaptive_engine_process
                                        .focus_process_background_efficiency_override_enabled = v
                                },
                                v
                            )))
                            .into(),
                            checkbox(
                                s.adaptive_engine_process
                                    .visible_window_background_efficiency_override_enabled
                            )
                            .label(t!("common.enabled").to_string())
                            .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                |s, v| {
                                    s.adaptive_engine_process
                                        .visible_window_background_efficiency_override_enabled = v
                                },
                                v
                            )))
                            .into(),
                            text("\u{2014}").style(iced::widget::text::secondary).into()
                        ]
                    )]
                    .spacing(design::space::SMALL),
                ));
                table = table.push(super::widgets::setting_group(
                    "nav.thread_priority".to_string(),
                    self.priority_expanded[usize::from(preset)][2],
                    Message::TogglePriority(2),
                    row![
                        iced::widget::container(super::widgets::switch(
                            s.adaptive_engine_process.thread_priority.enabled,
                            editable.then_some(|v| {
                                Message::Toggle(
                                    |s, v| s.adaptive_engine_process.thread_priority.enabled = v,
                                    v,
                                )
                            }),
                        ))
                        .width(64),
                        iced::widget::container(selector!(
                            s,
                            "cpu_allocation.focus",
                            ProcessThreadPrioritySetting,
                            if s.advanced.expose_all_priority_values {
                                &ProcessThreadPrioritySetting::ADVANCED_ALL
                            } else {
                                &ProcessThreadPrioritySetting::ALL
                            },
                            process_thread_priority_setting_label,
                            adaptive_engine_process.thread_priority.foreground_priority,
                            |value, theme| super::priority_control::Value::Thread(value.0)
                                .color(theme)
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.visible_window",
                            ProcessThreadPrioritySetting,
                            if s.advanced.expose_all_priority_values {
                                &ProcessThreadPrioritySetting::ADVANCED_ALL
                            } else {
                                &ProcessThreadPrioritySetting::ALL
                            },
                            process_thread_priority_setting_label,
                            adaptive_engine_process
                                .thread_priority
                                .visible_window_priority,
                            |value, theme| super::priority_control::Value::Thread(value.0)
                                .color(theme)
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.background_process",
                            ProcessThreadPrioritySetting,
                            if s.advanced.expose_all_priority_values {
                                &ProcessThreadPrioritySetting::ADVANCED_ALL
                            } else {
                                &ProcessThreadPrioritySetting::ALL
                            },
                            process_thread_priority_setting_label,
                            adaptive_engine_process.thread_priority.background_priority,
                            |value, theme| super::priority_control::Value::Thread(value.0)
                                .color(theme)
                        ))
                        .width(Fill)
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                    .width(iced::Length::FillPortion(3)),
                    column![
                        priority_option_row(
                            "adaptive_engine.detection",
                            [
                                checkbox(
                                    s.adaptive_engine_process
                                        .thread_priority
                                        .foreground_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .thread_priority
                                            .foreground_detection_enabled = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.adaptive_engine_process
                                        .thread_priority
                                        .visible_window_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .thread_priority
                                            .visible_window_detection_enabled = v
                                    },
                                    v
                                )))
                                .into(),
                                text("\u{2014}").style(iced::widget::text::secondary).into()
                            ]
                        ),
                        priority_option_row(
                            "adaptive_engine.keep_existing_priority",
                            [
                                checkbox(
                                    s.adaptive_engine_process
                                        .thread_priority
                                        .preserve_foreground_priority
                                )
                                .label(t!("adaptive_engine.same_or_higher").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .thread_priority
                                            .preserve_foreground_priority = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.adaptive_engine_process
                                        .thread_priority
                                        .preserve_visible_window_priority
                                )
                                .label(t!("adaptive_engine.same_or_higher").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .thread_priority
                                            .preserve_visible_window_priority = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.adaptive_engine_process
                                        .thread_priority
                                        .preserve_background_priority
                                )
                                .label(t!("adaptive_engine.same_or_lower").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .thread_priority
                                            .preserve_background_priority = v
                                    },
                                    v
                                )))
                                .into()
                            ]
                        )
                    ]
                    .spacing(design::space::SMALL),
                ));
                table = table.push(super::widgets::setting_group(
                    "nav.dynamic_priority_boost".to_string(),
                    self.priority_expanded[usize::from(preset)][3],
                    Message::TogglePriority(3),
                    row![
                        iced::widget::container(super::widgets::switch(
                            s.adaptive_engine_process.dynamic_priority_boost.enabled,
                            editable.then_some(|v| {
                                Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process.dynamic_priority_boost.enabled = v
                                    },
                                    v,
                                )
                            }),
                        ))
                        .width(64),
                        iced::widget::container(selector!(
                            s,
                            "cpu_allocation.focus",
                            ProcessDynamicPriorityBoostSetting,
                            &ProcessDynamicPriorityBoostSetting::ALL,
                            process_dynamic_priority_boost_setting_label,
                            adaptive_engine_process
                                .dynamic_priority_boost
                                .foreground_boost
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.visible_window",
                            ProcessDynamicPriorityBoostSetting,
                            &ProcessDynamicPriorityBoostSetting::ALL,
                            process_dynamic_priority_boost_setting_label,
                            adaptive_engine_process
                                .dynamic_priority_boost
                                .visible_window_boost
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.background_process",
                            ProcessDynamicPriorityBoostSetting,
                            &ProcessDynamicPriorityBoostSetting::ALL,
                            process_dynamic_priority_boost_setting_label,
                            adaptive_engine_process
                                .dynamic_priority_boost
                                .background_boost
                        ))
                        .width(Fill)
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                    .width(iced::Length::FillPortion(3)),
                    column![priority_option_row(
                        "adaptive_engine.detection",
                        [
                            checkbox(
                                s.adaptive_engine_process
                                    .dynamic_priority_boost
                                    .foreground_detection_enabled
                            )
                            .label(t!("common.enabled").to_string())
                            .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                |s, v| {
                                    s.adaptive_engine_process
                                        .dynamic_priority_boost
                                        .foreground_detection_enabled = v
                                },
                                v
                            )))
                            .into(),
                            checkbox(
                                s.adaptive_engine_process
                                    .dynamic_priority_boost
                                    .visible_window_detection_enabled
                            )
                            .label(t!("common.enabled").to_string())
                            .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                |s, v| {
                                    s.adaptive_engine_process
                                        .dynamic_priority_boost
                                        .visible_window_detection_enabled = v
                                },
                                v
                            )))
                            .into(),
                            text("\u{2014}").style(iced::widget::text::secondary).into()
                        ]
                    )]
                    .spacing(design::space::SMALL),
                ));
                table = table.push(super::widgets::setting_group(
                    "nav.io_priority".to_string(),
                    self.priority_expanded[usize::from(preset)][4],
                    Message::TogglePriority(4),
                    row![
                        iced::widget::container(super::widgets::switch(
                            s.adaptive_engine_process.io_priority.enabled,
                            editable.then_some(|v| {
                                Message::Toggle(
                                    |s, v| s.adaptive_engine_process.io_priority.enabled = v,
                                    v,
                                )
                            }),
                        ))
                        .width(64),
                        iced::widget::container(selector!(
                            s,
                            "cpu_allocation.focus",
                            ProcessIoPrioritySetting,
                            if s.advanced.expose_all_priority_values {
                                &ProcessIoPrioritySetting::ADVANCED_ALL
                            } else {
                                &ProcessIoPrioritySetting::ALL
                            },
                            process_io_priority_setting_label,
                            adaptive_engine_process.io_priority.foreground_priority,
                            |value, theme| super::priority_control::Value::Io(value.0).color(theme)
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.visible_window",
                            ProcessIoPrioritySetting,
                            if s.advanced.expose_all_priority_values {
                                &ProcessIoPrioritySetting::ADVANCED_ALL
                            } else {
                                &ProcessIoPrioritySetting::ALL
                            },
                            process_io_priority_setting_label,
                            adaptive_engine_process.io_priority.visible_window_priority,
                            |value, theme| super::priority_control::Value::Io(value.0).color(theme)
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.background_process",
                            ProcessIoPrioritySetting,
                            if s.advanced.expose_all_priority_values {
                                &ProcessIoPrioritySetting::ADVANCED_ALL
                            } else {
                                &ProcessIoPrioritySetting::ALL
                            },
                            process_io_priority_setting_label,
                            adaptive_engine_process.io_priority.background_priority,
                            |value, theme| super::priority_control::Value::Io(value.0).color(theme)
                        ))
                        .width(Fill)
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                    .width(iced::Length::FillPortion(3)),
                    column![
                        priority_option_row(
                            "adaptive_engine.detection",
                            [
                                checkbox(
                                    s.adaptive_engine_process
                                        .io_priority
                                        .foreground_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .io_priority
                                            .foreground_detection_enabled = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.adaptive_engine_process
                                        .io_priority
                                        .visible_window_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .io_priority
                                            .visible_window_detection_enabled = v
                                    },
                                    v
                                )))
                                .into(),
                                text("\u{2014}").style(iced::widget::text::secondary).into()
                            ]
                        ),
                        priority_option_row(
                            "adaptive_engine.keep_existing_priority",
                            [
                                checkbox(
                                    s.adaptive_engine_process
                                        .io_priority
                                        .preserve_foreground_priority
                                )
                                .label(t!("adaptive_engine.same_or_higher").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .io_priority
                                            .preserve_foreground_priority = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.adaptive_engine_process
                                        .io_priority
                                        .preserve_visible_window_priority
                                )
                                .label(t!("adaptive_engine.same_or_higher").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .io_priority
                                            .preserve_visible_window_priority = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.adaptive_engine_process
                                        .io_priority
                                        .preserve_background_priority
                                )
                                .label(t!("adaptive_engine.same_or_lower").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .io_priority
                                            .preserve_background_priority = v
                                    },
                                    v
                                )))
                                .into()
                            ]
                        )
                    ]
                    .spacing(design::space::SMALL),
                ));
                table = table.push(super::widgets::setting_group(
                    "nav.gpu_priority".to_string(),
                    self.priority_expanded[usize::from(preset)][5],
                    Message::TogglePriority(5),
                    row![
                        iced::widget::container(super::widgets::switch(
                            s.adaptive_engine_process.gpu_priority.enabled,
                            editable.then_some(|v| {
                                Message::Toggle(
                                    |s, v| s.adaptive_engine_process.gpu_priority.enabled = v,
                                    v,
                                )
                            }),
                        ))
                        .width(64),
                        iced::widget::container(selector!(
                            s,
                            "cpu_allocation.focus",
                            ProcessGpuPrioritySetting,
                            if s.advanced.expose_all_priority_values {
                                &ProcessGpuPrioritySetting::ADVANCED_ALL
                            } else {
                                &ProcessGpuPrioritySetting::ALL
                            },
                            process_gpu_priority_setting_label,
                            adaptive_engine_process.gpu_priority.foreground_priority,
                            |value, theme| super::priority_control::Value::Gpu(value.0)
                                .color(theme)
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.visible_window",
                            ProcessGpuPrioritySetting,
                            if s.advanced.expose_all_priority_values {
                                &ProcessGpuPrioritySetting::ADVANCED_ALL
                            } else {
                                &ProcessGpuPrioritySetting::ALL
                            },
                            process_gpu_priority_setting_label,
                            adaptive_engine_process.gpu_priority.visible_window_priority,
                            |value, theme| super::priority_control::Value::Gpu(value.0)
                                .color(theme)
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.background_process",
                            ProcessGpuPrioritySetting,
                            if s.advanced.expose_all_priority_values {
                                &ProcessGpuPrioritySetting::ADVANCED_ALL
                            } else {
                                &ProcessGpuPrioritySetting::ALL
                            },
                            process_gpu_priority_setting_label,
                            adaptive_engine_process.gpu_priority.background_priority,
                            |value, theme| super::priority_control::Value::Gpu(value.0)
                                .color(theme)
                        ))
                        .width(Fill)
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                    .width(iced::Length::FillPortion(3)),
                    column![
                        priority_option_row(
                            "adaptive_engine.detection",
                            [
                                checkbox(
                                    s.adaptive_engine_process
                                        .gpu_priority
                                        .foreground_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .gpu_priority
                                            .foreground_detection_enabled = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.adaptive_engine_process
                                        .gpu_priority
                                        .visible_window_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .gpu_priority
                                            .visible_window_detection_enabled = v
                                    },
                                    v
                                )))
                                .into(),
                                text("\u{2014}").style(iced::widget::text::secondary).into()
                            ]
                        ),
                        priority_option_row(
                            "adaptive_engine.keep_existing_priority",
                            [
                                checkbox(
                                    s.adaptive_engine_process
                                        .gpu_priority
                                        .preserve_foreground_priority
                                )
                                .label(t!("adaptive_engine.same_or_higher").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .gpu_priority
                                            .preserve_foreground_priority = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.adaptive_engine_process
                                        .gpu_priority
                                        .preserve_visible_window_priority
                                )
                                .label(t!("adaptive_engine.same_or_higher").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .gpu_priority
                                            .preserve_visible_window_priority = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.adaptive_engine_process
                                        .gpu_priority
                                        .preserve_background_priority
                                )
                                .label(t!("adaptive_engine.same_or_lower").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .gpu_priority
                                            .preserve_background_priority = v
                                    },
                                    v
                                )))
                                .into()
                            ]
                        )
                    ]
                    .spacing(design::space::SMALL),
                ));
                table = table.push(super::widgets::setting_group(
                    "nav.memory_priority".to_string(),
                    self.priority_expanded[usize::from(preset)][6],
                    Message::TogglePriority(6),
                    row![
                        iced::widget::container(super::widgets::switch(
                            s.adaptive_engine_process.memory_priority_enabled,
                            editable.then_some(|v| {
                                Message::Toggle(
                                    |s, v| s.adaptive_engine_process.memory_priority_enabled = v,
                                    v,
                                )
                            }),
                        ))
                        .width(64),
                        iced::widget::container(selector!(
                            s,
                            "cpu_allocation.focus",
                            ProcessMemoryPrioritySetting,
                            &ProcessMemoryPrioritySetting::ALL,
                            process_memory_priority_setting_label,
                            adaptive_engine_process.focus_process_memory_priority
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.visible_window",
                            ProcessMemoryPrioritySetting,
                            &ProcessMemoryPrioritySetting::ALL,
                            process_memory_priority_setting_label,
                            adaptive_engine_process.visible_window_memory_priority
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.background_process",
                            ProcessMemoryPrioritySetting,
                            &ProcessMemoryPrioritySetting::ALL,
                            process_memory_priority_setting_label,
                            adaptive_engine_process.background_memory_priority
                        ))
                        .width(Fill)
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                    .width(iced::Length::FillPortion(3)),
                    column![
                        priority_option_row(
                            "adaptive_engine.detection",
                            [
                                checkbox(
                                    s.adaptive_engine_process
                                        .memory_priority_foreground_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .memory_priority_foreground_detection_enabled = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.adaptive_engine_process
                                        .memory_priority_visible_window_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .memory_priority_visible_window_detection_enabled = v
                                    },
                                    v
                                )))
                                .into(),
                                text("\u{2014}").style(iced::widget::text::secondary).into()
                            ]
                        ),
                        priority_option_row(
                            "adaptive_engine.keep_existing_priority",
                            [
                                checkbox(
                                    s.adaptive_engine_process
                                        .memory_priority_preserve_foreground
                                )
                                .label(t!("adaptive_engine.same_or_higher").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .memory_priority_preserve_foreground = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.adaptive_engine_process
                                        .memory_priority_preserve_visible_window
                                )
                                .label(t!("adaptive_engine.same_or_higher").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .memory_priority_preserve_visible_window = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.adaptive_engine_process
                                        .memory_priority_preserve_background
                                )
                                .label(t!("adaptive_engine.same_or_lower").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.adaptive_engine_process
                                            .memory_priority_preserve_background = v
                                    },
                                    v
                                )))
                                .into()
                            ]
                        )
                    ]
                    .spacing(design::space::SMALL),
                ));
                body = body.push(table.width(Fill));
            }
            TuningTab::CustomRules => {}
        }
        if !preset && tab == TuningTab::CustomRules {
            body = body
                .push(text(t!("adaptive_engine_process.custom_rules").to_string()))
                .push(super::app_picker::view(
                    &self.path,
                    candidates,
                    true,
                    Message::Path,
                    Message::Browse,
                    super::process_rules::can_add_process_candidate(
                        &self.path,
                        |path| {
                            s.adaptive_engine_process.custom_rules.iter().any(|rule| {
                                super::process_rules::process_setting_matches(
                                    &rule.executable_path,
                                    path,
                                )
                            })
                        },
                        crate::adaptive_engine_process::is_builtin_excluded,
                    )
                    .then_some(Message::AddExclusion),
                    |path| {
                        super::process_rules::can_add_process_candidate(
                            path,
                            |path| {
                                s.adaptive_engine_process.custom_rules.iter().any(|rule| {
                                    super::process_rules::process_setting_matches(
                                        &rule.executable_path,
                                        path,
                                    )
                                })
                            },
                            crate::adaptive_engine_process::is_builtin_excluded,
                        )
                        .then_some(true)
                    },
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

        if let Some(error) = self.validation_error() {
            body = body.push(text(error));
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
    pub(super) fn side_panel<'a>(
        &'a self,
        live: &'a crate::application::SettingsEditor,
        status: &'a RuntimeStatusSnapshot,
    ) -> Element<'a, Message> {
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
                    button(super::navigation::glyph("icons/info.svg"))
                        .style(super::widgets::quiet)
                        .on_press(Message::ViewBuiltIn(p))
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
                    iced::widget::tooltip(
                        button(super::navigation::glyph("icons/pencil.svg"))
                            .padding(7)
                            .width(32)
                            .height(32)
                            .style(super::widgets::quiet)
                            .on_press(Message::Edit(i)),
                        text(t!("adaptive_engine.edit_preset").to_string()),
                        iced::widget::tooltip::Position::Top,
                    ),
                    super::widgets::rule_delete_button(Some(Message::Delete(i)))
                ]
                .spacing(design::space::TIGHT)
                .align_y(iced::Center),
            );
        }
        if !self.presets_tab {
            rail = column![];
            if let Some(status) =
                super::status_rail::view(crate::ui::Page::AdaptiveEngine, live, status, &[])
            {
                rail = rail.push(status.map(Message::Status));
            }
        }
        let tabs = row![
            super::widgets::panel_tab(
                t!("common.status").to_string(),
                !self.presets_tab,
                Message::RailTab(false),
                true,
            ),
            super::widgets::panel_tab(
                t!("adaptive_engine.presets").to_string(),
                self.presets_tab,
                Message::RailTab(true),
                true,
            )
        ]
        .spacing(design::space::SMALL);
        let mut panel = column![
            tabs,
            super::motion::wrap(
                scrollable(rail).width(Fill).height(Fill),
                true,
                super::motion::Effect::Content(u64::from(self.presets_tab))
            )
        ]
        .spacing(design::space::MEDIUM)
        .height(Fill);
        if self.presets_tab {
            panel = panel.push(super::widgets::preset_footer(
                t!("adaptive_engine.add_preset").to_string(),
                Message::New,
            ));
        }
        panel.into()
    }
}
fn priority_option_row<'a>(key: &str, cells: [Element<'a, Message>; 3]) -> Element<'a, Message> {
    let [focus, visible, background] = cells;
    row![
        text(t!(key).to_string())
            .size(design::typography::SECONDARY)
            .style(iced::widget::text::secondary)
            .width(Fill),
        row![
            iced::widget::Space::new().width(64),
            iced::widget::container(focus).width(Fill),
            iced::widget::container(visible).width(Fill),
            iced::widget::container(background).width(Fill),
        ]
        .spacing(design::space::SMALL)
        .align_y(iced::Center)
        .width(iced::Length::FillPortion(3)),
        iced::widget::Space::new().width(design::ICON_SIZE),
    ]
    .spacing(design::space::SMALL)
    .height(super::widgets::SETTING_ROW_HEIGHT)
    .align_y(iced::Center)
    .into()
}

fn setting_label(key: &str) -> iced::widget::Row<'static, Message> {
    setting_label_with_unit(key, "")
}

fn setting_label_with_unit(key: &str, unit: &str) -> iced::widget::Row<'static, Message> {
    let help_key = match key {
        "processor_power.core_parking_min" => "adaptive_engine.core_parking_min_help".to_string(),
        "processor_power.processor_min" => "adaptive_engine.processor_min_help".to_string(),
        "processor_power.processor_max" => "adaptive_engine.processor_max_help".to_string(),
        "processor_power.boost_policy" => "adaptive_engine.base_boost_policy_help".to_string(),
        _ => format!("{key}_help"),
    };
    let mut label = row![super::widgets::heading(
        super::widgets::label_with_unit(&t!(key), unit),
        design::typography::BODY
    )]
    .spacing(design::space::SMALL)
    .align_y(iced::Center);
    let help = t!(&help_key).to_string();
    if help != help_key {
        label = label.push(iced::widget::tooltip(
            super::navigation::glyph("icons/info.svg"),
            iced::widget::container(text(help).width(320))
                .padding(design::space::COMPACT as u16)
                .style(super::widgets::surface),
            iced::widget::tooltip::Position::Top,
        ));
    }
    label
}
#[derive(Debug, Clone, PartialEq)]
struct Choice<T>(T, String);
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
fn boost_label(boost_mode: ProcessorBoostMode) -> String {
    match boost_mode {
        ProcessorBoostMode::Disabled => t!("processor_power.boost_disabled").to_string(),
        ProcessorBoostMode::Enabled => t!("processor_power.boost_enabled").to_string(),
        ProcessorBoostMode::Aggressive => t!("processor_power.boost_aggressive").to_string(),
        ProcessorBoostMode::EfficientEnabled => {
            t!("processor_power.boost_efficient_enabled").to_string()
        }
        ProcessorBoostMode::EfficientAggressive => {
            t!("processor_power.boost_efficient_aggressive").to_string()
        }
        ProcessorBoostMode::AggressiveAtGuaranteed => {
            t!("processor_power.boost_aggressive_at_guaranteed").to_string()
        }
        ProcessorBoostMode::EfficientAggressiveAtGuaranteed => {
            t!("processor_power.boost_efficient_aggressive_at_guaranteed").to_string()
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn zone_preset_editing_is_independent_and_speed_keeps_background_limiting() {
        let mut editor = Editor::default();
        let mut settings = Settings::default();
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
        editor.update(&mut settings, Message::ZoneMask(10));
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
            vec![1, 3]
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
        assert_eq!(settings, before);
        editor.update(
            &mut settings,
            Message::ViewBuiltIn(BuiltInAdaptiveEnginePreset::Performance),
        );
        let preview = editor.draft.clone();
        editor.update(&mut settings, Message::ZoneMask(1));
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
        let mut settings = Settings::default();
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
        let mut settings = Settings::default();
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

    #[test]
    fn priority_cards_expand_independently_for_live_and_preset_views() {
        let mut editor = Editor::default();
        let mut settings = Settings::default();
        editor.update(&mut settings, Message::TogglePriority(2));
        assert_eq!(
            editor.priority_expanded[0],
            [false, false, true, false, false, false, false]
        );
        editor.update(
            &mut settings,
            Message::ViewBuiltIn(BuiltInAdaptiveEnginePreset::Balanced),
        );
        editor.update(&mut settings, Message::TogglePriority(4));
        assert!(editor.priority_expanded[1][4]);
        assert!(!editor.priority_expanded[1][2]);
        editor.update(&mut settings, Message::Cancel);
        assert!(editor.priority_expanded[0][2]);
        assert!(!editor.priority_expanded[0][4]);
    }

    use super::*;
    #[test]
    fn priority_card_changes_keep_repaint_regions_bounded() {
        use iced::advanced::Renderer as _;
        use iced::advanced::{layout, widget::Tree};
        let mut renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let mut editor = Editor::default();
        let mut settings = Settings::default();
        let bounds = iced::Rectangle::with_size(iced::Size::new(1040.0, 700.0));
        let mut pixels = tiny_skia::Pixmap::new(1040, 700).unwrap();
        let mut mask = tiny_skia::Mask::new(1040, 700).unwrap();
        let viewport =
            iced::advanced::graphics::Viewport::with_physical_size(iced::Size::new(1040, 700), 1.0);
        for tab in TuningTab::ALL {
            editor.update(&mut settings, Message::TuningTab(tab));
            let mut tree = Tree::empty();
            let mut previous = Vec::new();
            for frame in 0..6 {
                editor.priority_expanded[0][0] = (frame / 2) % 2 == 0;
                settings.adaptive_engine_process.process_priority_enabled = frame % 2 == 0;
                let mut view = editor.view(&settings, &[]);
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
                let start = std::time::Instant::now();
                renderer.draw(
                    &mut pixels.as_mut(),
                    &mut mask,
                    &viewport,
                    &damage,
                    iced::Color::BLACK,
                );
                println!("raster {:?}, regions {}", start.elapsed(), damage.len());
            }
        }
    }

    #[test]
    fn priority_tab_scroll_damage_stays_bounded() {
        let mut editor = Editor::default();
        let mut settings = Settings::default();
        editor.update(
            &mut settings,
            Message::TuningTab(TuningTab::PriorityControl),
        );
        for expanded in [false, true] {
            editor.priority_expanded[0].fill(expanded);
            super::super::scrolling::check_scroll_damage(
                editor.view(&settings, &[]),
                iced::Rectangle::with_size(iced::Size::new(1040.0, 700.0)),
            );
        }
    }

    #[test]
    fn tuning_navigation_keeps_live_and_preset_state_separate() {
        let mut editor = Editor::default();
        let mut settings = Settings::default();
        let before = settings.clone();
        editor.update(&mut settings, Message::TuningTab(TuningTab::CustomRules));
        editor.update(&mut settings, Message::Collapse(0));
        assert_eq!(settings, before);
        editor.update(
            &mut settings,
            Message::ViewBuiltIn(BuiltInAdaptiveEnginePreset::Balanced),
        );
        assert_eq!(editor.tuning_tabs[1], TuningTab::CpuBehaviour);
        assert_eq!(editor.expanded[1], [false; 3]);
        editor.update(&mut settings, Message::TuningTab(TuningTab::CustomRules));
        assert_eq!(editor.tuning_tabs[1], TuningTab::CpuBehaviour);
        editor.update(
            &mut settings,
            Message::TuningTab(TuningTab::PriorityControl),
        );
        editor.update(&mut settings, Message::Collapse(1));
        editor.update(&mut settings, Message::Cancel);
        assert_eq!(editor.tuning_tabs[0], TuningTab::CustomRules);
        assert_eq!(editor.expanded[0], [true, false, false]);
        assert_eq!(settings, before);
    }

    #[test]
    fn independent_cpu_gates_and_preset_isolation() {
        let mut settings = Settings::default();
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
                |s, v| s.adaptive_engine_process.process_priority_enabled = v,
                false,
            ),
        );
        assert!(settings.adaptive_engine_process.process_priority_enabled);
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
        assert!(!settings.adaptive_engine_process.process_priority_enabled);
        editor.update(&mut settings, Message::RemoveExclusion(0));
        assert!(settings.adaptive_engine_process.custom_rules.is_empty());
    }
    #[test]
    fn read_only_builtin_and_invalid_numbers_do_not_mutate() {
        let mut settings = Settings::default();
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
        let draft = editor.draft.clone();
        editor.update(
            &mut settings,
            Message::Toggle(
                |s, v| s.adaptive_engine_process.dynamic_resource_zones_enabled = v,
                true,
            ),
        );
        assert_eq!(editor.draft, draft);
        assert_eq!(settings, old);
    }
}
