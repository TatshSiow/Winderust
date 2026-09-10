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
use iced::widget::{column, row, scrollable, text};
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
            Self::CustomRules => "cpu_scheduler.custom_rules",
        }
    }
}
#[derive(Default)]
pub(super) struct Editor {
    name: String,
    draft: Option<Settings>,
    editing: Option<usize>,
    read_only: bool,
    presets_tab: bool,
    tuning_tabs: [TuningTab; 2],
    collapsed: [[bool; 2]; 2],
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
    BuiltIn(BuiltInAdaptiveEnginePreset),
    ViewBuiltIn(BuiltInAdaptiveEnginePreset),
    Apply(usize),
    Edit(usize),
    New,
    Name(String),
    Save,
    Delete,
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
                if let Some(value) =
                    self.collapsed[usize::from(self.draft.is_some())].get_mut(index)
                {
                    *value = !*value;
                }
            }
            Message::RailTab(value) => self.presets_tab = value,
            Message::Status(_) => {}
            Message::BuiltIn(p) => {
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
            Message::Delete => {
                if !self.read_only {
                    if let Some(i) = self.editing {
                        if i < s.adaptive_engine_presets.len() {
                            s.adaptive_engine_presets.remove(i);
                        }
                    }
                    self.draft = None;
                }
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
                        s.cpu_scheduler.custom_rules.iter().any(|r| {
                            crate::ui::process_rules::process_setting_matches(&r.executable_path, p)
                        })
                    },
                    crate::cpu_scheduler::is_builtin_excluded,
                ) {
                    s.cpu_scheduler.custom_rules.push(ProcessExclusionRule {
                        executable_path: crate::foreground::executable_path_key(
                            std::path::Path::new(&self.path),
                        ),
                        ..Default::default()
                    });
                    self.path.clear();
                }
            }
            Message::RemoveExclusion(i) => {
                if i < s.cpu_scheduler.custom_rules.len() {
                    s.cpu_scheduler.custom_rules.remove(i);
                }
            }

            Message::ExclusionEnabled(i, v) => {
                if let Some(r) = s.cpu_scheduler.custom_rules.get_mut(i) {
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
                    Message::Mask(mask) => {
                        target.cpu_scheduler.specific_processors =
                            (0..64).filter(|i| mask & (1u64 << i) != 0).collect()
                    }
                    _ => {}
                }
            }
        }
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
        _status: &'a RuntimeStatusSnapshot,
        candidates: &'a [super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        let s = self.draft.as_ref().unwrap_or(live);
        let editable = self.draft.is_none() || !self.read_only;
        let mut body = column![].spacing(super::widgets::CARD_GAP);
        macro_rules! toggle {($key:expr,$($field:ident).+) => {row![
            setting_label($key).width(Fill),
            super::widgets::switch(s.$($field).+, editable.then_some(|v|Message::Toggle(|s,v|s.$($field).+ = v,v)))
        ].spacing(design::space::SMALL).height(super::widgets::SETTING_ROW_HEIGHT).align_y(iced::Center)};}
        macro_rules! number {($key:expr,$min:expr,$max:expr,$($field:ident).+) => {{
            let key=stringify!($($field).+);
            let value=self.numbers.get(key).cloned().unwrap_or_else(||s.$($field).+.to_string());
            let unit=if key.ends_with("_ms") {"ms"} else if key.ends_with("_seconds") {"s"} else if $max==100 {"%"} else {""};
            row![setting_label($key).width(Fill),
                super::widgets::stepper(&value, $min..=$max, 1, unit,
                    editable.then_some(move|v|Message::Number(|s,n|s.$($field).+ = n as _,v,$min,$max,key,$key)))
            ].spacing(design::space::SMALL).height(46).align_y(iced::Center)
        }};}
        macro_rules! selector {($s:ident,$key:expr,$ty:ty,$options:expr,$label:expr,$($field:ident).+) => {{let values:&[$ty]=$options;let selected=s.$($field).+;let control:Element<'_,Message>=if editable{pick_list(values.iter().copied().map(|v|Choice(v,$label(v))).collect::<Vec<_>>(),Some(Choice(selected,$label(selected))),move |v|Message::Choice(|$s,i|{let options:&[$ty]=$options;if let Some(v)=options.get(i){$s.$($field).+ = *v;}},values.iter().position(|x|*x==v.0).unwrap_or(0))).width(Fill).into()}else{text($label(selected)).into()};control}};}
        macro_rules! choice {($s:ident,$key:expr,$ty:ty,$options:expr,$label:expr,$($field:ident).+) => {row![setting_label($key).width(Fill),iced::widget::container(selector!($s,$key,$ty,$options,$label,$($field).+)).width(280)].spacing(design::space::SMALL).height(46).align_y(iced::Center)};}
        if self.draft.is_none() {
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
            let current = capture_adaptive_engine_preset(live, String::new());
            let selected = options
                .iter()
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
                .cloned();
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
        let tab = self.tuning_tabs[usize::from(self.draft.is_some())];
        let mut tabs = row![].spacing(design::space::TIGHT);
        for next in TuningTab::ALL
            .into_iter()
            .filter(|tab| self.draft.is_none() || *tab != TuningTab::CustomRules)
        {
            tabs = tabs.push(
                button(
                    iced::widget::container(text(t!(next.key()).to_string()))
                        .center_x(Fill)
                        .center_y(Fill),
                )
                .style(if next == tab {
                    super::widgets::selected_control
                } else {
                    super::widgets::quiet
                })
                .width(Fill)
                .height(36)
                .on_press(Message::TuningTab(next)),
            );
        }
        body = body.push(
            iced::widget::container(tabs)
                .padding(design::space::TIGHT as u16)
                .width(Fill)
                .style(super::widgets::surface),
        );
        match tab {
            TuningTab::CpuBehaviour => {
                let pressure = column![
                    number!(
                        "cpu_scheduler.maximum_restrained_apps",
                        1,
                        64,
                        cpu_scheduler.maximum_restrained_apps
                    ),
                    number!(
                        "cpu_scheduler.reaction_time",
                        250,
                        5000,
                        cpu_scheduler.reaction_time_ms
                    ),
                    number!(
                        "cpu_scheduler.foreground_or_system_cpu_threshold",
                        1,
                        100,
                        cpu_scheduler.foreground_or_system_cpu_threshold_percent
                    ),
                    number!(
                        "cpu_scheduler.cpu_restraint_time",
                        1,
                        3600,
                        cpu_scheduler.cpu_restraint_time_seconds
                    ),
                    number!(
                        "cpu_scheduler.cpu_recovery_threshold",
                        1,
                        100,
                        cpu_scheduler.cpu_recovery_threshold_percent
                    ),
                    number!(
                        "cpu_scheduler.cpu_recovery_time",
                        1,
                        3600,
                        cpu_scheduler.cpu_recovery_time_seconds
                    )
                ]
                .spacing(design::space::MEDIUM);
                let action: Element<'_, Message> = if self.draft.is_none() {
                    super::widgets::switch(
                        s.cpu_scheduler.cpu_pressure_restraint_enabled,
                        Some(|v| {
                            Message::Toggle(
                                |s, v| s.cpu_scheduler.cpu_pressure_restraint_enabled = v,
                                v,
                            )
                        }),
                    )
                } else {
                    iced::widget::Space::new().into()
                };
                body = body.push(super::widgets::setting_group(
                    "adaptive_engine.cpu_pressure".to_string(),
                    !self.collapsed[usize::from(self.draft.is_some())][0],
                    Message::Collapse(0),
                    action,
                    pressure,
                ));
                let mut allocation = column![
                    number!(
                        "cpu_scheduler.background_app_cpu_threshold",
                        1,
                        100,
                        cpu_scheduler.background_app_cpu_threshold_percent
                    ),
                    choice!(
                        s,
                        "cpu_scheduler.processor_selection",
                        BackgroundProcessorSelection,
                        &BackgroundProcessorSelection::ALL,
                        background_processor_selection_label,
                        cpu_scheduler.background_processor_selection
                    ),
                    toggle!(
                        "cpu_scheduler.dynamic_resource_zones",
                        cpu_scheduler.dynamic_resource_zones_enabled
                    )
                ]
                .spacing(design::space::MEDIUM);
                if !s.cpu_scheduler.dynamic_resource_zones_enabled {
                    allocation = allocation.push(choice!(
                        s,
                        "cpu_scheduler.cpu_allocation_method",
                        CpuAllocationMethod,
                        &CpuAllocationMethod::ALL,
                        allocation_label,
                        cpu_scheduler.cpu_allocation_method
                    ));
                }
                if matches!(
                    s.cpu_scheduler.background_processor_selection,
                    BackgroundProcessorSelection::LeastUsed
                        | BackgroundProcessorSelection::LeastUsedPerformanceCores
                        | BackgroundProcessorSelection::LeastUsedEfficiencyCores
                ) {
                    allocation = allocation.push(number!(
                        if s.cpu_scheduler.dynamic_resource_zones_enabled {
                            "cpu_scheduler.foreground_zone_share"
                        } else {
                            "cpu_scheduler.processor_limit"
                        },
                        1,
                        100,
                        cpu_scheduler.processor_limit_percent
                    ));
                }
                if s.cpu_scheduler.background_processor_selection
                    == BackgroundProcessorSelection::Custom
                    && editable
                {
                    let mask = s
                        .cpu_scheduler
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
                if s.cpu_scheduler.background_processor_selection
                    == BackgroundProcessorSelection::Custom
                    && !editable
                {
                    allocation = allocation.push(text(format!(
                        "{}: {:?}",
                        t!("cpu_scheduler.specific_processors"),
                        s.cpu_scheduler.specific_processors
                    )));
                }
                body = body.push(super::widgets::setting_group(
                    "cpu_scheduler.limit_background_processors".to_string(),
                    !self.collapsed[usize::from(self.draft.is_some())][1],
                    Message::Collapse(1),
                    super::widgets::switch(
                        s.cpu_scheduler.limit_background_processors_enabled,
                        editable.then_some(|v| {
                            Message::Toggle(
                                |s, v| s.cpu_scheduler.limit_background_processors_enabled = v,
                                v,
                            )
                        }),
                    ),
                    allocation,
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
                        text(t!("common.enabled").to_string()).width(64),
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
                    self.priority_expanded[usize::from(self.draft.is_some())][0],
                    Message::TogglePriority(0),
                    row![
                        iced::widget::container(super::widgets::switch(
                            s.cpu_scheduler.process_priority_enabled,
                            editable.then_some(|v| {
                                Message::Toggle(
                                    |s, v| s.cpu_scheduler.process_priority_enabled = v,
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
                            cpu_scheduler.focus_process_priority
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
                            cpu_scheduler.visible_window_priority
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
                            cpu_scheduler.background_priority
                        ))
                        .width(Fill)
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                    .width(iced::Length::FillPortion(3)),
                    column![
                        iced::widget::rule::horizontal(1),
                        priority_option_row(
                            "adaptive_engine.detection",
                            [
                                checkbox(
                                    s.cpu_scheduler
                                        .process_priority_foreground_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
                                            .process_priority_foreground_detection_enabled = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.cpu_scheduler
                                        .process_priority_visible_window_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
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
                                checkbox(s.cpu_scheduler.process_priority_preserve_foreground)
                                    .label(t!("adaptive_engine.same_or_higher").to_string())
                                    .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                        |s, v| {
                                            s.cpu_scheduler.process_priority_preserve_foreground = v
                                        },
                                        v
                                    )))
                                    .into(),
                                checkbox(s.cpu_scheduler.process_priority_preserve_visible_window)
                                    .label(t!("adaptive_engine.same_or_higher").to_string())
                                    .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                        |s, v| {
                                            s.cpu_scheduler
                                                .process_priority_preserve_visible_window = v
                                        },
                                        v
                                    )))
                                    .into(),
                                checkbox(s.cpu_scheduler.process_priority_preserve_background)
                                    .label(t!("adaptive_engine.same_or_lower").to_string())
                                    .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                        |s, v| {
                                            s.cpu_scheduler.process_priority_preserve_background = v
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
                    self.priority_expanded[usize::from(self.draft.is_some())][1],
                    Message::TogglePriority(1),
                    row![
                        iced::widget::container(super::widgets::switch(
                            s.cpu_scheduler.background_efficiency_enabled,
                            editable.then_some(|v| {
                                Message::Toggle(
                                    |s, v| s.cpu_scheduler.background_efficiency_enabled = v,
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
                                    s.cpu_scheduler.focus_process_background_efficiency_mode,
                                    t!(
                                        if s.cpu_scheduler.focus_process_background_efficiency_mode
                                        {
                                            "common.enabled"
                                        } else {
                                            "common.disabled"
                                        }
                                    )
                                    .to_string()
                                )),
                                |v| Message::Toggle(
                                    |s, v| s
                                        .cpu_scheduler
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
                                    s.cpu_scheduler.visible_window_background_efficiency_mode,
                                    t!(
                                        if s.cpu_scheduler.visible_window_background_efficiency_mode
                                        {
                                            "common.enabled"
                                        } else {
                                            "common.disabled"
                                        }
                                    )
                                    .to_string()
                                )),
                                |v| Message::Toggle(
                                    |s, v| s
                                        .cpu_scheduler
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
                                    s.cpu_scheduler.background_efficiency_mode,
                                    t!(if s.cpu_scheduler.background_efficiency_mode {
                                        "common.enabled"
                                    } else {
                                        "common.disabled"
                                    })
                                    .to_string()
                                )),
                                |v| Message::Toggle(
                                    |s, v| s.cpu_scheduler.background_efficiency_mode = v,
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
                    column![
                        iced::widget::rule::horizontal(1),
                        priority_option_row(
                            "adaptive_engine.detection",
                            [
                                checkbox(
                                    s.cpu_scheduler
                                        .focus_process_background_efficiency_override_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
                                            .focus_process_background_efficiency_override_enabled =
                                            v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.cpu_scheduler
                                        .visible_window_background_efficiency_override_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
                                            .visible_window_background_efficiency_override_enabled =
                                            v
                                    },
                                    v
                                )))
                                .into(),
                                text("\u{2014}").style(iced::widget::text::secondary).into()
                            ]
                        )
                    ]
                    .spacing(design::space::SMALL),
                ));
                table = table.push(super::widgets::setting_group(
                    "nav.thread_priority".to_string(),
                    self.priority_expanded[usize::from(self.draft.is_some())][2],
                    Message::TogglePriority(2),
                    row![
                        iced::widget::container(super::widgets::switch(
                            s.cpu_scheduler.thread_priority.enabled,
                            editable.then_some(|v| {
                                Message::Toggle(
                                    |s, v| s.cpu_scheduler.thread_priority.enabled = v,
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
                            cpu_scheduler.thread_priority.foreground_priority
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
                            cpu_scheduler.thread_priority.visible_window_priority
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
                            cpu_scheduler.thread_priority.background_priority
                        ))
                        .width(Fill)
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                    .width(iced::Length::FillPortion(3)),
                    column![
                        iced::widget::rule::horizontal(1),
                        priority_option_row(
                            "adaptive_engine.detection",
                            [
                                checkbox(
                                    s.cpu_scheduler.thread_priority.foreground_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
                                            .thread_priority
                                            .foreground_detection_enabled = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.cpu_scheduler
                                        .thread_priority
                                        .visible_window_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
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
                                    s.cpu_scheduler.thread_priority.preserve_foreground_priority
                                )
                                .label(t!("adaptive_engine.same_or_higher").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
                                            .thread_priority
                                            .preserve_foreground_priority = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.cpu_scheduler
                                        .thread_priority
                                        .preserve_visible_window_priority
                                )
                                .label(t!("adaptive_engine.same_or_higher").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
                                            .thread_priority
                                            .preserve_visible_window_priority = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.cpu_scheduler.thread_priority.preserve_background_priority
                                )
                                .label(t!("adaptive_engine.same_or_lower").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
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
                    self.priority_expanded[usize::from(self.draft.is_some())][3],
                    Message::TogglePriority(3),
                    row![
                        iced::widget::container(super::widgets::switch(
                            s.cpu_scheduler.dynamic_priority_boost.enabled,
                            editable.then_some(|v| {
                                Message::Toggle(
                                    |s, v| s.cpu_scheduler.dynamic_priority_boost.enabled = v,
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
                            cpu_scheduler.dynamic_priority_boost.foreground_boost
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.visible_window",
                            ProcessDynamicPriorityBoostSetting,
                            &ProcessDynamicPriorityBoostSetting::ALL,
                            process_dynamic_priority_boost_setting_label,
                            cpu_scheduler.dynamic_priority_boost.visible_window_boost
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.background_process",
                            ProcessDynamicPriorityBoostSetting,
                            &ProcessDynamicPriorityBoostSetting::ALL,
                            process_dynamic_priority_boost_setting_label,
                            cpu_scheduler.dynamic_priority_boost.background_boost
                        ))
                        .width(Fill)
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                    .width(iced::Length::FillPortion(3)),
                    column![
                        iced::widget::rule::horizontal(1),
                        priority_option_row(
                            "adaptive_engine.detection",
                            [
                                checkbox(
                                    s.cpu_scheduler
                                        .dynamic_priority_boost
                                        .foreground_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
                                            .dynamic_priority_boost
                                            .foreground_detection_enabled = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.cpu_scheduler
                                        .dynamic_priority_boost
                                        .visible_window_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
                                            .dynamic_priority_boost
                                            .visible_window_detection_enabled = v
                                    },
                                    v
                                )))
                                .into(),
                                text("\u{2014}").style(iced::widget::text::secondary).into()
                            ]
                        )
                    ]
                    .spacing(design::space::SMALL),
                ));
                table = table.push(super::widgets::setting_group(
                    "nav.io_priority".to_string(),
                    self.priority_expanded[usize::from(self.draft.is_some())][4],
                    Message::TogglePriority(4),
                    row![
                        iced::widget::container(super::widgets::switch(
                            s.cpu_scheduler.io_priority.enabled,
                            editable.then_some(|v| {
                                Message::Toggle(|s, v| s.cpu_scheduler.io_priority.enabled = v, v)
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
                            cpu_scheduler.io_priority.foreground_priority
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
                            cpu_scheduler.io_priority.visible_window_priority
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
                            cpu_scheduler.io_priority.background_priority
                        ))
                        .width(Fill)
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                    .width(iced::Length::FillPortion(3)),
                    column![
                        iced::widget::rule::horizontal(1),
                        priority_option_row(
                            "adaptive_engine.detection",
                            [
                                checkbox(s.cpu_scheduler.io_priority.foreground_detection_enabled)
                                    .label(t!("common.enabled").to_string())
                                    .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                        |s, v| {
                                            s.cpu_scheduler
                                                .io_priority
                                                .foreground_detection_enabled = v
                                        },
                                        v
                                    )))
                                    .into(),
                                checkbox(
                                    s.cpu_scheduler.io_priority.visible_window_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
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
                                checkbox(s.cpu_scheduler.io_priority.preserve_foreground_priority)
                                    .label(t!("adaptive_engine.same_or_higher").to_string())
                                    .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                        |s, v| {
                                            s.cpu_scheduler
                                                .io_priority
                                                .preserve_foreground_priority = v
                                        },
                                        v
                                    )))
                                    .into(),
                                checkbox(
                                    s.cpu_scheduler.io_priority.preserve_visible_window_priority
                                )
                                .label(t!("adaptive_engine.same_or_higher").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
                                            .io_priority
                                            .preserve_visible_window_priority = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(s.cpu_scheduler.io_priority.preserve_background_priority)
                                    .label(t!("adaptive_engine.same_or_lower").to_string())
                                    .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                        |s, v| {
                                            s.cpu_scheduler
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
                    self.priority_expanded[usize::from(self.draft.is_some())][5],
                    Message::TogglePriority(5),
                    row![
                        iced::widget::container(super::widgets::switch(
                            s.cpu_scheduler.gpu_priority.enabled,
                            editable.then_some(|v| {
                                Message::Toggle(|s, v| s.cpu_scheduler.gpu_priority.enabled = v, v)
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
                            cpu_scheduler.gpu_priority.foreground_priority
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
                            cpu_scheduler.gpu_priority.visible_window_priority
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
                            cpu_scheduler.gpu_priority.background_priority
                        ))
                        .width(Fill)
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                    .width(iced::Length::FillPortion(3)),
                    column![
                        iced::widget::rule::horizontal(1),
                        priority_option_row(
                            "adaptive_engine.detection",
                            [
                                checkbox(s.cpu_scheduler.gpu_priority.foreground_detection_enabled)
                                    .label(t!("common.enabled").to_string())
                                    .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                        |s, v| {
                                            s.cpu_scheduler
                                                .gpu_priority
                                                .foreground_detection_enabled = v
                                        },
                                        v
                                    )))
                                    .into(),
                                checkbox(
                                    s.cpu_scheduler
                                        .gpu_priority
                                        .visible_window_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
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
                                checkbox(s.cpu_scheduler.gpu_priority.preserve_foreground_priority)
                                    .label(t!("adaptive_engine.same_or_higher").to_string())
                                    .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                        |s, v| {
                                            s.cpu_scheduler
                                                .gpu_priority
                                                .preserve_foreground_priority = v
                                        },
                                        v
                                    )))
                                    .into(),
                                checkbox(
                                    s.cpu_scheduler
                                        .gpu_priority
                                        .preserve_visible_window_priority
                                )
                                .label(t!("adaptive_engine.same_or_higher").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
                                            .gpu_priority
                                            .preserve_visible_window_priority = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(s.cpu_scheduler.gpu_priority.preserve_background_priority)
                                    .label(t!("adaptive_engine.same_or_lower").to_string())
                                    .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                        |s, v| {
                                            s.cpu_scheduler
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
                    self.priority_expanded[usize::from(self.draft.is_some())][6],
                    Message::TogglePriority(6),
                    row![
                        iced::widget::container(super::widgets::switch(
                            s.cpu_scheduler.memory_priority_enabled,
                            editable.then_some(|v| {
                                Message::Toggle(
                                    |s, v| s.cpu_scheduler.memory_priority_enabled = v,
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
                            cpu_scheduler.focus_process_memory_priority
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.visible_window",
                            ProcessMemoryPrioritySetting,
                            &ProcessMemoryPrioritySetting::ALL,
                            process_memory_priority_setting_label,
                            cpu_scheduler.visible_window_memory_priority
                        ))
                        .width(Fill),
                        iced::widget::container(selector!(
                            s,
                            "common.background_process",
                            ProcessMemoryPrioritySetting,
                            &ProcessMemoryPrioritySetting::ALL,
                            process_memory_priority_setting_label,
                            cpu_scheduler.background_memory_priority
                        ))
                        .width(Fill)
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                    .width(iced::Length::FillPortion(3)),
                    column![
                        iced::widget::rule::horizontal(1),
                        priority_option_row(
                            "adaptive_engine.detection",
                            [
                                checkbox(
                                    s.cpu_scheduler.memory_priority_foreground_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
                                            .memory_priority_foreground_detection_enabled = v
                                    },
                                    v
                                )))
                                .into(),
                                checkbox(
                                    s.cpu_scheduler
                                        .memory_priority_visible_window_detection_enabled
                                )
                                .label(t!("common.enabled").to_string())
                                .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                    |s, v| {
                                        s.cpu_scheduler
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
                                checkbox(s.cpu_scheduler.memory_priority_preserve_foreground)
                                    .label(t!("adaptive_engine.same_or_higher").to_string())
                                    .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                        |s, v| {
                                            s.cpu_scheduler.memory_priority_preserve_foreground = v
                                        },
                                        v
                                    )))
                                    .into(),
                                checkbox(s.cpu_scheduler.memory_priority_preserve_visible_window)
                                    .label(t!("adaptive_engine.same_or_higher").to_string())
                                    .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                        |s, v| {
                                            s.cpu_scheduler
                                                .memory_priority_preserve_visible_window = v
                                        },
                                        v
                                    )))
                                    .into(),
                                checkbox(s.cpu_scheduler.memory_priority_preserve_background)
                                    .label(t!("adaptive_engine.same_or_lower").to_string())
                                    .on_toggle_maybe(editable.then_some(|v| Message::Toggle(
                                        |s, v| {
                                            s.cpu_scheduler.memory_priority_preserve_background = v
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
        if self.draft.is_none() && tab == TuningTab::CustomRules {
            body = body
                .push(text(t!("cpu_scheduler.custom_rules").to_string()))
                .push(super::app_picker::view(
                    &self.path,
                    candidates,
                    true,
                    Message::Path,
                    Message::Browse,
                    super::process_rules::can_add_process_candidate(
                        &self.path,
                        |path| {
                            s.cpu_scheduler.custom_rules.iter().any(|rule| {
                                super::process_rules::process_setting_matches(
                                    &rule.executable_path,
                                    path,
                                )
                            })
                        },
                        crate::cpu_scheduler::is_builtin_excluded,
                    )
                    .then_some(Message::AddExclusion),
                    |path| {
                        super::process_rules::can_add_process_candidate(
                            path,
                            |path| {
                                s.cpu_scheduler.custom_rules.iter().any(|rule| {
                                    super::process_rules::process_setting_matches(
                                        &rule.executable_path,
                                        path,
                                    )
                                })
                            },
                            crate::cpu_scheduler::is_builtin_excluded,
                        )
                        .then_some(true)
                    },
                ));
            let mut rules = column![
                row![
                    text(t!("common.active").to_string()).width(48),
                    text(t!("process_list.app_name").to_string()).width(Fill),
                    text(t!("process_list.executable_path").to_string())
                        .width(iced::Length::FillPortion(2)),
                    text(t!("common.actions").to_string()).width(64),
                ]
                .spacing(design::space::SMALL)
                .padding(super::widgets::CARD_PADDING as u16),
                iced::widget::rule::horizontal(1)
            ];
            for (i, rule) in s.cpu_scheduler.custom_rules.iter().enumerate() {
                rules = rules.push(
                    row![
                        checkbox(rule.enabled)
                            .on_toggle(move |value| Message::ExclusionEnabled(i, value))
                            .width(48),
                        iced::widget::container(super::app_picker::app_name(
                            &rule.executable_path,
                            candidates
                        ))
                        .width(Fill)
                        .clip(true),
                        text(rule.executable_path.clone())
                            .style(text::secondary)
                            .wrapping(iced::widget::text::Wrapping::None)
                            .width(iced::Length::FillPortion(2)),
                        iced::widget::container(iced::widget::tooltip(
                            button(super::navigation::glyph("icons/trash-2.svg"))
                                .style(iced::widget::button::danger)
                                .on_press(Message::RemoveExclusion(i)),
                            text(t!("common.remove").to_string()),
                            iced::widget::tooltip::Position::Top
                        ))
                        .width(64)
                        .center_x(64),
                    ]
                    .height(super::widgets::CARD_HEIGHT)
                    .padding(super::widgets::CARD_PADDING as u16)
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center),
                );
            }
            if s.cpu_scheduler.custom_rules.is_empty() {
                rules = rules.push(
                    iced::widget::container(
                        text(t!("common.no_custom_rules").to_string()).style(text::secondary),
                    )
                    .height(super::widgets::CARD_HEIGHT)
                    .center_y(super::widgets::CARD_HEIGHT)
                    .padding(super::widgets::CARD_PADDING as u16),
                );
            }
            body = body.push(
                iced::widget::container(rules)
                    .width(Fill)
                    .style(super::widgets::surface)
                    .clip(true),
            );
        }

        if let Some(error) = self.validation_error() {
            body = body.push(text(error));
        }
        scrollable(body).width(Fill).height(Fill).into()
    }
    pub(super) fn side_panel<'a>(
        &'a self,
        live: &'a Settings,
        status: &'a RuntimeStatusSnapshot,
    ) -> Element<'a, Message> {
        let mut rail = column![
            text(t!("adaptive_engine.built_in_presets").to_string()).style(text::secondary)
        ]
        .spacing(design::space::SMALL);
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
        rail = rail
            .push(text(t!("adaptive_engine.custom_presets").to_string()).style(text::secondary));
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
                    button(text(t!("adaptive_engine.edit_preset").to_string()))
                        .on_press(Message::Edit(i))
                ]
                .spacing(design::space::TIGHT)
                .align_y(iced::Center),
            );
        }
        if self.draft.is_some() {
            rail = rail
                .push(
                    text_input(&t!("adaptive_engine.preset_name"), &self.name)
                        .on_input(Message::Name),
                )
                .push(button(text(t!("common.cancel").to_string())).on_press(Message::Cancel));
            if !self.read_only {
                rail = rail
                    .push(button(text(t!("common.save").to_string())).on_press(Message::Save))
                    .push(
                        button(text(t!("adaptive_engine.use_current_settings").to_string()))
                            .on_press(Message::UseCurrent),
                    );
                if self.editing.is_some() {
                    rail = rail.push(
                        button(text(t!("common.remove").to_string())).on_press(Message::Delete),
                    );
                }
            }
        }
        if !self.error.is_empty() {
            rail = rail.push(text(self.error.clone()).style(text::danger));
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
                Message::RailTab(false)
            ),
            super::widgets::panel_tab(
                t!("adaptive_engine.presets").to_string(),
                self.presets_tab,
                Message::RailTab(true)
            )
        ]
        .spacing(design::space::SMALL);
        let mut panel = column![tabs, scrollable(rail).width(Fill).height(Fill)]
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
    let help_key = match key {
        "adaptive_engine.enable" => "adaptive_engine.intro_1".to_string(),
        "processor_power.core_parking_min" => "adaptive_engine.core_parking_min_help".to_string(),
        "processor_power.processor_min" => "adaptive_engine.processor_min_help".to_string(),
        "processor_power.processor_max" => "adaptive_engine.processor_max_help".to_string(),
        "processor_power.boost_policy" => "adaptive_engine.base_boost_policy_help".to_string(),
        _ => format!("{key}_help"),
    };
    let mut label = row![super::widgets::heading(
        t!(key).to_string(),
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
    fn remove_updates_only_the_draft_and_discard_restores_the_rule() {
        let mut original = Settings::default();
        original
            .cpu_scheduler
            .custom_rules
            .push(ProcessExclusionRule {
                executable_path: r"C:\Apps\test.exe".into(),
                ..Default::default()
            });
        let mut settings = crate::application::SettingsEditor::with_settings(original.clone());
        let mut editor = Editor::default();
        editor.update(&mut settings, Message::RemoveExclusion(0));
        assert!(settings.cpu_scheduler.custom_rules.is_empty());
        assert_eq!(
            settings.persisted().cpu_scheduler.custom_rules,
            original.cpu_scheduler.custom_rules
        );
        settings.cancel();
        assert_eq!(
            settings.cpu_scheduler.custom_rules,
            original.cpu_scheduler.custom_rules
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
        assert_eq!(editor.collapsed[1], [false; 2]);
        editor.update(&mut settings, Message::TuningTab(TuningTab::CustomRules));
        assert_eq!(editor.tuning_tabs[1], TuningTab::CpuBehaviour);
        editor.update(
            &mut settings,
            Message::TuningTab(TuningTab::PriorityControl),
        );
        editor.update(&mut settings, Message::Collapse(1));
        editor.update(&mut settings, Message::Cancel);
        assert_eq!(editor.tuning_tabs[0], TuningTab::CustomRules);
        assert_eq!(editor.collapsed[0], [true, false]);
        assert_eq!(settings, before);
    }

    #[test]
    fn independent_cpu_gates_and_preset_isolation() {
        let mut settings = Settings::default();
        let mut editor = Editor::default();
        settings.cpu_scheduler.limit_background_processors_enabled = true;
        editor.update(
            &mut settings,
            Message::Toggle(
                |s, v| s.cpu_scheduler.cpu_pressure_restraint_enabled = v,
                false,
            ),
        );
        assert!(settings.cpu_scheduler.limit_background_processors_enabled);
        let exclusion = ProcessExclusionRule {
            executable_path: r"C:\App\app.exe".into(),
            ..Default::default()
        };
        settings.cpu_scheduler.custom_rules.push(exclusion.clone());
        editor.update(&mut settings, Message::New);
        editor.update(
            &mut settings,
            Message::Toggle(|s, v| s.cpu_scheduler.process_priority_enabled = v, false),
        );
        assert!(settings.cpu_scheduler.process_priority_enabled);
        editor.update(&mut settings, Message::Name("Saved".into()));
        editor.update(&mut settings, Message::Save);
        editor.update(&mut settings, Message::Apply(0));
        assert!(!settings.adaptive_engine.enabled);
        assert!(!settings.cpu_scheduler.cpu_pressure_restraint_enabled);
        assert_eq!(settings.cpu_scheduler.custom_rules, vec![exclusion]);
        assert!(!settings.cpu_scheduler.process_priority_enabled);
        editor.update(&mut settings, Message::RemoveExclusion(0));
        assert!(settings.cpu_scheduler.custom_rules.is_empty());
    }
    #[test]
    fn read_only_builtin_and_invalid_numbers_do_not_mutate() {
        let mut settings = Settings::default();
        let old = settings.clone();
        let mut editor = Editor::default();
        editor.update(
            &mut settings,
            Message::Number(
                |s, n| s.cpu_scheduler.reaction_time_ms = n,
                "1".into(),
                250,
                5000,
                "reaction",
                "cpu_scheduler.reaction_time",
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
                |s, v| s.cpu_scheduler.dynamic_resource_zones_enabled = v,
                true,
            ),
        );
        assert_eq!(editor.draft, draft);
        assert_eq!(settings, old);
    }
}
