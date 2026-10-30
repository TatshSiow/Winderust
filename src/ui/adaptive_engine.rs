use super::priority_control::{
    process_dynamic_priority_boost_setting_label, process_gpu_priority_setting_label,
    process_io_priority_setting_label, process_memory_priority_setting_label,
    process_priority_setting_label, process_thread_priority_setting_label,
};
use super::widgets::{self, Ui};
use crate::automation::RuntimeStatusSnapshot;
use crate::config::*;
use crate::power::ProcessorBoostMode;
use rust_i18n::t;
use windui::prelude::{signal, Align, Element, Signal};
#[cfg(feature = "runtime-benchmark")]
#[path = "runtime_benchmark.rs"]
pub(crate) mod benchmark;
#[path = "adaptive_presets.rs"]
mod presets;
#[cfg(feature = "render-smoke")]
pub(super) use presets::BuiltInAdaptiveEnginePreset;
use presets::*;
type NumericInput = (&'static str, Signal<String>, u64, u64);
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
    pub(super) fn shows_live_status(&self) -> bool {
        !self.presets_tab
    }

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
    pub(super) fn view(
        &self,
        live: &Settings,
        candidates: &[super::app_picker::Candidate],
        ui: Ui<Message>,
    ) -> Element {
        self.tuning_view(live, false, candidates, ui, &mut Vec::new())
    }

    pub(super) fn preset_modal(
        &self,
        live: &Settings,
        candidates: &[super::app_picker::Candidate],
        ui: Ui<Message>,
    ) -> Element {
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
        let name = signal(self.name.clone());
        let invalid_numbers = signal(self.invalid_numbers.clone());
        let ui = ui.inspect(move |message: &Message, _refresh| {
            if let Message::Number(_, raw, min, max, key, title) = message {
                invalid_numbers.update(|invalid| {
                    if raw
                        .parse::<u64>()
                        .is_ok_and(|value| (*min..=*max).contains(&value))
                    {
                        invalid.remove(key);
                    } else {
                        invalid.insert(*key, (*min, *max, *title));
                    }
                });
            }
        });
        let other_names = live
            .adaptive_engine_presets
            .iter()
            .enumerate()
            .filter(|(index, _)| Some(*index) != self.editing)
            .map(|(_, preset)| preset.name.trim().to_owned())
            .collect::<Vec<_>>();
        let mut native_numbers = Vec::new();
        let mut body = vec![
            Element::setting_row_desc(
                t!("adaptive_engine.preset_name").to_string(),
                t!("adaptive_engine.preset_name_help").to_string(),
                ui.input_signal(
                    name,
                    t!("adaptive_engine.preset_name_placeholder").to_string(),
                    Message::Name,
                )
                .enabled(!self.read_only)
                .max_width(340),
            )
            .width_match(),
            self.tuning_view(live, true, candidates, ui.clone(), &mut native_numbers),
        ];
        if !self.error.is_empty() {
            body.push(widgets::error(self.error.clone()));
        }
        let mut actions = Vec::new();
        if !self.read_only {
            if let Some(index) = self.editing {
                actions.push(ui.button(t!("common.remove").to_string(), Message::Delete(index)));
            }
            actions.push(ui.button(
                t!("adaptive_engine.use_current_settings").to_string(),
                Message::UseCurrent,
            ));
            let names = other_names.clone();
            body.push(Element::label(name.map(move |raw| {
                if !raw.trim().is_empty()
                    && names
                        .iter()
                        .any(|other| other.eq_ignore_ascii_case(raw.trim()))
                {
                    t!("adaptive_engine.duplicate_preset_name").to_string()
                } else {
                    String::new()
                }
            })));
            body.push(Element::label(invalid_numbers.map(|invalid| {
                invalid
                    .values()
                    .next()
                    .map(|(min, max, title)| format!("{}: {min}..{max}", t!(*title)))
                    .unwrap_or_default()
            })));
            let other_numbers_valid = self.invalid_numbers.keys().all(|key| {
                native_numbers
                    .iter()
                    .any(|(visible, _, _, _)| visible == key)
            });
            actions.push(
                ui.primary_button(t!("common.save").to_string(), Message::Save)
                    .enabled_when(move || {
                        let name = name.get();
                        other_numbers_valid
                            && native_numbers.iter().all(|(_, raw, min, max)| {
                                raw.get()
                                    .parse::<u64>()
                                    .is_ok_and(|value| (*min..=*max).contains(&value))
                            })
                            && !name.trim().is_empty()
                            && !other_names
                                .iter()
                                .any(|other| other.eq_ignore_ascii_case(name.trim()))
                    }),
            );
        }
        actions.push(ui.button(t!("common.cancel").to_string(), Message::Cancel));
        widgets::editor_panel(
            title,
            widgets::col().children(body),
            widgets::row().children(actions),
        )
    }

    fn tuning_view(
        &self,
        live: &Settings,
        preset: bool,
        candidates: &[super::app_picker::Candidate],
        ui: Ui<Message>,
        native_numbers: &mut Vec<NumericInput>,
    ) -> Element {
        let s = if preset {
            self.draft.as_ref().unwrap_or(live)
        } else {
            live
        };
        let editable = !preset || !self.read_only;
        let mut body = Vec::new();
        macro_rules! toggle {
            ($key:expr, $($field:ident).+) => {
                setting($key, "", ui.toggle(s.$($field).+, |value| Message::Toggle(|s, value| s.$($field).+ = value, value)).enabled(editable))
            };
        }
        macro_rules! number {
            ($key:expr, $min:expr, $max:expr, $($field:ident).+) => {{
                let key = stringify!($($field).+);
                let value = self.numbers.get(key).cloned().unwrap_or_else(|| s.$($field).+.to_string());
                let raw = signal(value);
                native_numbers.push((key, raw, $min, $max));
                let unit = if key.ends_with("_ms") { "ms" } else if key.ends_with("_seconds") { "s" } else if key.ends_with("_percent") || $max == 100 { "%" } else { "" };
                setting($key, unit, trailing_control(ui.number(raw, $min, $max, 1, move |value| Message::Number(|s, number| s.$($field).+ = number as _, value, $min, $max, key, $key)).enabled(editable)))
            }};
        }
        macro_rules! choice {
            ($s:ident, $key:expr, $ty:ty, $options:expr, $label:expr, $($field:ident).+) => {{
                let values: &[$ty] = $options;
                let selected = s.$($field).+;
                setting($key, "", trailing_control(ui.select(values.iter().copied().map(|value| Choice(value, $label(value))), Choice(selected, $label(selected)), move |value| Message::Choice(|$s, index| {
                    let options: &[$ty] = $options;
                    if let Some(value) = options.get(index) { $s.$($field).+ = *value; }
                }, values.iter().position(|option| *option == value.0).unwrap_or(0))).enabled(editable)))
            }};
        }
        if !preset {
            let mut options = BuiltInAdaptiveEnginePreset::ALL
                .into_iter()
                .enumerate()
                .map(|(index, value)| Choice(index, built_in_adaptive_engine_preset_label(value)))
                .collect::<Vec<_>>();
            options.extend(
                live.adaptive_engine_presets
                    .iter()
                    .enumerate()
                    .map(|(index, value)| Choice(index + 4, value.name.clone())),
            );
            let selected = self
                .selected_preset(live, &options)
                .unwrap_or_else(|| Choice(usize::MAX, t!("common.custom").to_string()));
            body.push(widgets::card(
                "",
                [
                    toggle!("adaptive_engine.enable", adaptive_engine.enabled),
                    Element::divider(),
                    setting(
                        "adaptive_engine.preset",
                        "",
                        trailing_control(ui.select(options, selected, |choice| {
                            if choice.0 < 4 {
                                Message::BuiltIn(BuiltInAdaptiveEnginePreset::ALL[choice.0])
                            } else if choice.0 == usize::MAX {
                                Message::RailTab(true)
                            } else {
                                Message::Apply(choice.0 - 4)
                            }
                        })),
                    ),
                ],
            ));
        }
        let tab = self.tuning_tabs[usize::from(preset)];
        body.push(
            Element::scroll()
                .scroll_axis(windui::spec::ScrollAxis::Horizontal)
                .width_match()
                .height(44)
                .child(
                    ui.segmented(
                        TuningTab::ALL
                            .into_iter()
                            .filter(|tab| !preset || *tab != TuningTab::CustomRules),
                        tab,
                        Message::TuningTab,
                    )
                    .min_width(520),
                ),
        );
        match tab {
            TuningTab::CpuBehaviour => {
                body.push(widgets::card(
                    t!("adaptive_engine_process.shared_detection").to_string(),
                    [
                        number!(
                            "adaptive_engine_process.background_app_cpu_threshold",
                            1,
                            100,
                            adaptive_engine_process.background_app_cpu_threshold_percent
                        ),
                        Element::divider(),
                        number!(
                            "adaptive_engine_process.maximum_restrained_apps",
                            1,
                            64,
                            adaptive_engine_process.maximum_restrained_apps
                        ),
                        Element::divider(),
                        number!(
                            "adaptive_engine_process.reaction_time",
                            250,
                            5000,
                            adaptive_engine_process.reaction_time_ms
                        ),
                        Element::divider(),
                        number!(
                            "adaptive_engine_process.foreground_or_system_cpu_threshold",
                            1,
                            100,
                            adaptive_engine_process.foreground_or_system_cpu_threshold_percent
                        ),
                        Element::divider(),
                        number!(
                            "adaptive_engine_process.cpu_restraint_time",
                            1,
                            3600,
                            adaptive_engine_process.cpu_restraint_time_seconds
                        ),
                        Element::divider(),
                        number!(
                            "adaptive_engine_process.cpu_recovery_threshold",
                            1,
                            100,
                            adaptive_engine_process.cpu_recovery_threshold_percent
                        ),
                        Element::divider(),
                        number!(
                            "adaptive_engine_process.cpu_recovery_time",
                            1,
                            3600,
                            adaptive_engine_process.cpu_recovery_time_seconds
                        ),
                    ],
                ));
                body.push(widgets::card(
                    "",
                    [
                        toggle!(
                            "adaptive_engine.cpu_pressure",
                            adaptive_engine_process.cpu_pressure_restraint_enabled
                        ),
                        ui.collapsible(
                            t!("common.feature_info").to_string(),
                            self.expanded[usize::from(preset)][0],
                            widgets::help("adaptive_engine_process.cpu_pressure_restraint_help"),
                            |_| Message::Collapse(0),
                        ),
                    ],
                ));
                let mut allocation = vec![
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
                    ),
                ];
                if s.adaptive_engine_process
                    .background_processor_selection
                    .uses_percentage()
                {
                    allocation.push(number!(
                        "adaptive_engine_process.processor_limit",
                        1,
                        100,
                        adaptive_engine_process.processor_limit_percent
                    ));
                }
                if s.adaptive_engine_process.background_processor_selection
                    == BackgroundProcessorSelection::Custom
                {
                    let mask = crate::cpu_allocation::logical_processor_indices_mask(
                        &s.adaptive_engine_process.specific_processors,
                    );
                    allocation.push(
                        super::cpu_allocation::mask_selector(
                            mask,
                            &crate::cpu_allocation::logical_processors(),
                            &s.cpu_allocation_presets,
                            Message::Mask,
                            ui.clone(),
                        )
                        .enabled(editable),
                    );
                }
                body.push(widgets::card(
                    "",
                    [
                        toggle!(
                            "adaptive_engine_process.limit_background_processors",
                            adaptive_engine_process.limit_background_processors_enabled
                        ),
                        ui.collapsible(
                            t!("settings.advanced").to_string(),
                            self.expanded[usize::from(preset)][1],
                            widgets::col().children(allocation),
                            |_| Message::Collapse(1),
                        ),
                    ],
                ));
                let zones = &s.adaptive_engine_process.dynamic_resource_zone_settings;
                let mut zone_controls = vec![
                    widgets::help("adaptive_engine_process.zone_warning"),
                    choice!(
                        s,
                        "adaptive_engine_process.zone_selection",
                        BackgroundProcessorSelection,
                        &BackgroundProcessorSelection::ALL,
                        background_processor_selection_label,
                        adaptive_engine_process
                            .dynamic_resource_zone_settings
                            .background_processor_selection
                    ),
                ];
                if zones.background_processor_selection.uses_percentage() {
                    zone_controls.push(number!(
                        "adaptive_engine_process.foreground_zone_share",
                        1,
                        99,
                        adaptive_engine_process
                            .dynamic_resource_zone_settings
                            .foreground_share_percent
                    ));
                }
                if zones.background_processor_selection == BackgroundProcessorSelection::Custom {
                    zone_controls.push(
                        super::cpu_allocation::mask_selector(
                            crate::cpu_allocation::logical_processor_indices_mask(
                                &zones.specific_processors,
                            ),
                            &crate::cpu_allocation::logical_processors(),
                            &s.cpu_allocation_presets,
                            Message::ZoneMask,
                            ui.clone(),
                        )
                        .enabled(editable),
                    );
                }
                body.push(widgets::card(
                    "",
                    [
                        toggle!(
                            "adaptive_engine_process.dynamic_resource_zones",
                            adaptive_engine_process.dynamic_resource_zones_enabled
                        ),
                        ui.collapsible(
                            t!("settings.advanced").to_string(),
                            self.expanded[usize::from(preset)][2],
                            widgets::col().children(zone_controls),
                            |_| Message::Collapse(2),
                        ),
                    ],
                ));
            }
            TuningTab::ProcessorPower => {
                body.push(widgets::card(
                    "",
                    [toggle!(
                        "adaptive_engine.processor_power_policy",
                        adaptive_engine.processor_power_policy_enabled
                    )],
                ));
                body.push(widgets::card(
                    t!("adaptive_engine.base_processor_policy").to_string(),
                    [
                        number!(
                            "processor_power.core_parking_min",
                            0,
                            100,
                            adaptive_engine.base_processor_policy.core_parking_min
                        ),
                        Element::divider(),
                        number!(
                            "processor_power.processor_min",
                            0,
                            100,
                            adaptive_engine.base_processor_policy.performance_min
                        ),
                        Element::divider(),
                        number!(
                            "processor_power.processor_max",
                            0,
                            100,
                            adaptive_engine.base_processor_policy.performance_max
                        ),
                        Element::divider(),
                        number!(
                            "processor_power.boost_policy",
                            0,
                            100,
                            adaptive_engine.base_processor_policy.boost_policy
                        ),
                        Element::divider(),
                        choice!(
                            s,
                            "processor_power.boost_mode",
                            ProcessorBoostMode,
                            &ProcessorBoostMode::ALL,
                            boost_label,
                            adaptive_engine.base_processor_policy.boost_mode
                        ),
                    ],
                ));
                body.push(widgets::card(
                    t!("adaptive_engine.background_pressure_profile").to_string(),
                    [
                        number!(
                            "adaptive_engine.ac_boost_policy",
                            0,
                            100,
                            adaptive_engine.background_pressure_profile.ac_policy
                        ),
                        Element::divider(),
                        choice!(
                            s,
                            "adaptive_engine.ac_boost_mode",
                            ProcessorBoostMode,
                            &ProcessorBoostMode::ALL,
                            boost_label,
                            adaptive_engine.background_pressure_profile.ac_mode
                        ),
                        Element::divider(),
                        number!(
                            "adaptive_engine.battery_boost_policy",
                            0,
                            100,
                            adaptive_engine.background_pressure_profile.battery_policy
                        ),
                        Element::divider(),
                        choice!(
                            s,
                            "adaptive_engine.battery_boost_mode",
                            ProcessorBoostMode,
                            &ProcessorBoostMode::ALL,
                            boost_label,
                            adaptive_engine.background_pressure_profile.battery_mode
                        ),
                    ],
                ));
                body.push(widgets::card(
                    t!("adaptive_engine.focus_and_launch_profile").to_string(),
                    [
                        number!(
                            "adaptive_engine.ac_boost_policy",
                            0,
                            100,
                            adaptive_engine.focus_and_launch_profile.ac_policy
                        ),
                        Element::divider(),
                        choice!(
                            s,
                            "adaptive_engine.ac_boost_mode",
                            ProcessorBoostMode,
                            &ProcessorBoostMode::ALL,
                            boost_label,
                            adaptive_engine.focus_and_launch_profile.ac_mode
                        ),
                        Element::divider(),
                        number!(
                            "adaptive_engine.battery_boost_policy",
                            0,
                            100,
                            adaptive_engine.focus_and_launch_profile.battery_policy
                        ),
                        Element::divider(),
                        choice!(
                            s,
                            "adaptive_engine.battery_boost_mode",
                            ProcessorBoostMode,
                            &ProcessorBoostMode::ALL,
                            boost_label,
                            adaptive_engine.focus_and_launch_profile.battery_mode
                        ),
                    ],
                ));
            }
            TuningTab::PriorityControl => {
                let mut controls = vec![toggle!(
                    "common.enable",
                    adaptive_engine_process.process_priority_enabled
                )];
                controls.push(choice!(
                    s,
                    "common.focus_process",
                    ProcessPrioritySetting,
                    if s.advanced.expose_all_priority_values {
                        &ProcessPrioritySetting::ADVANCED_ALL
                    } else {
                        &ProcessPrioritySetting::ALL
                    },
                    process_priority_setting_label,
                    adaptive_engine_process.focus_process_priority
                ));
                controls.push(choice!(
                    s,
                    "common.visible_window",
                    ProcessPrioritySetting,
                    if s.advanced.expose_all_priority_values {
                        &ProcessPrioritySetting::ADVANCED_ALL
                    } else {
                        &ProcessPrioritySetting::ALL
                    },
                    process_priority_setting_label,
                    adaptive_engine_process.visible_window_priority
                ));
                controls.push(choice!(
                    s,
                    "common.background_process",
                    ProcessPrioritySetting,
                    if s.advanced.expose_all_priority_values {
                        &ProcessPrioritySetting::ADVANCED_ALL
                    } else {
                        &ProcessPrioritySetting::ALL
                    },
                    process_priority_setting_label,
                    adaptive_engine_process.background_priority
                ));
                let options = vec![
                    toggle!(
                        "adaptive_engine.detection",
                        adaptive_engine_process.process_priority_foreground_detection_enabled
                    ),
                    toggle!(
                        "common.visible_window_detection",
                        adaptive_engine_process.process_priority_visible_window_detection_enabled
                    ),
                    toggle!(
                        "common.preserve_foreground_priority",
                        adaptive_engine_process.process_priority_preserve_foreground
                    ),
                    toggle!(
                        "common.preserve_visible_window_priority",
                        adaptive_engine_process.process_priority_preserve_visible_window
                    ),
                    toggle!(
                        "common.preserve_background_priority",
                        adaptive_engine_process.process_priority_preserve_background
                    ),
                ];
                controls.push(ui.collapsible(
                    t!("adaptive_engine.detection").to_string(),
                    self.priority_expanded[usize::from(preset)][0],
                    widgets::col().children(options),
                    |_| Message::TogglePriority(0),
                ));
                body.push(widgets::card(
                    t!("nav.process_priority").to_string(),
                    controls,
                ));
                let mut controls = vec![
                    toggle!(
                        "common.enable",
                        adaptive_engine_process.background_efficiency_enabled
                    ),
                    toggle!(
                        "common.focus_process",
                        adaptive_engine_process.focus_process_background_efficiency_mode
                    ),
                    toggle!(
                        "common.visible_window",
                        adaptive_engine_process.visible_window_background_efficiency_mode
                    ),
                    toggle!(
                        "common.background_process",
                        adaptive_engine_process.background_efficiency_mode
                    ),
                ];
                controls.push(ui.collapsible(
                    t!("adaptive_engine.detection").to_string(),
                    self.priority_expanded[usize::from(preset)][1],
                    widgets::col().children([
                        toggle!(
                                "adaptive_engine.detection",
                                adaptive_engine_process
                                    .focus_process_background_efficiency_override_enabled
                            ),
                        toggle!(
                                "common.visible_window_detection",
                                adaptive_engine_process
                                    .visible_window_background_efficiency_override_enabled
                            ),
                    ]),
                    |_| Message::TogglePriority(1),
                ));
                body.push(widgets::card(
                    t!("nav.background_efficiency").to_string(),
                    controls,
                ));
                let mut controls = vec![toggle!(
                    "common.enable",
                    adaptive_engine_process.thread_priority.enabled
                )];
                controls.push(choice!(
                    s,
                    "common.focus_process",
                    ProcessThreadPrioritySetting,
                    if s.advanced.expose_all_priority_values {
                        &ProcessThreadPrioritySetting::ADVANCED_ALL
                    } else {
                        &ProcessThreadPrioritySetting::ALL
                    },
                    process_thread_priority_setting_label,
                    adaptive_engine_process.thread_priority.foreground_priority
                ));
                controls.push(choice!(
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
                        .visible_window_priority
                ));
                controls.push(choice!(
                    s,
                    "common.background_process",
                    ProcessThreadPrioritySetting,
                    if s.advanced.expose_all_priority_values {
                        &ProcessThreadPrioritySetting::ADVANCED_ALL
                    } else {
                        &ProcessThreadPrioritySetting::ALL
                    },
                    process_thread_priority_setting_label,
                    adaptive_engine_process.thread_priority.background_priority
                ));
                let options = vec![
                    toggle!(
                        "adaptive_engine.detection",
                        adaptive_engine_process
                            .thread_priority
                            .foreground_detection_enabled
                    ),
                    toggle!(
                        "common.visible_window_detection",
                        adaptive_engine_process
                            .thread_priority
                            .visible_window_detection_enabled
                    ),
                    toggle!(
                        "common.preserve_foreground_priority",
                        adaptive_engine_process
                            .thread_priority
                            .preserve_foreground_priority
                    ),
                    toggle!(
                        "common.preserve_visible_window_priority",
                        adaptive_engine_process
                            .thread_priority
                            .preserve_visible_window_priority
                    ),
                    toggle!(
                        "common.preserve_background_priority",
                        adaptive_engine_process
                            .thread_priority
                            .preserve_background_priority
                    ),
                ];
                controls.push(ui.collapsible(
                    t!("adaptive_engine.detection").to_string(),
                    self.priority_expanded[usize::from(preset)][2],
                    widgets::col().children(options),
                    |_| Message::TogglePriority(2),
                ));
                body.push(widgets::card(
                    t!("nav.thread_priority").to_string(),
                    controls,
                ));
                let mut controls = vec![toggle!(
                    "common.enable",
                    adaptive_engine_process.dynamic_priority_boost.enabled
                )];
                controls.push(choice!(
                    s,
                    "common.focus_process",
                    ProcessDynamicPriorityBoostSetting,
                    &ProcessDynamicPriorityBoostSetting::ALL,
                    process_dynamic_priority_boost_setting_label,
                    adaptive_engine_process
                        .dynamic_priority_boost
                        .foreground_boost
                ));
                controls.push(choice!(
                    s,
                    "common.visible_window",
                    ProcessDynamicPriorityBoostSetting,
                    &ProcessDynamicPriorityBoostSetting::ALL,
                    process_dynamic_priority_boost_setting_label,
                    adaptive_engine_process
                        .dynamic_priority_boost
                        .visible_window_boost
                ));
                controls.push(choice!(
                    s,
                    "common.background_process",
                    ProcessDynamicPriorityBoostSetting,
                    &ProcessDynamicPriorityBoostSetting::ALL,
                    process_dynamic_priority_boost_setting_label,
                    adaptive_engine_process
                        .dynamic_priority_boost
                        .background_boost
                ));
                let options = vec![
                    toggle!(
                        "adaptive_engine.detection",
                        adaptive_engine_process
                            .dynamic_priority_boost
                            .foreground_detection_enabled
                    ),
                    toggle!(
                        "common.visible_window_detection",
                        adaptive_engine_process
                            .dynamic_priority_boost
                            .visible_window_detection_enabled
                    ),
                ];
                controls.push(ui.collapsible(
                    t!("adaptive_engine.detection").to_string(),
                    self.priority_expanded[usize::from(preset)][3],
                    widgets::col().children(options),
                    |_| Message::TogglePriority(3),
                ));
                body.push(widgets::card(
                    t!("nav.dynamic_priority_boost").to_string(),
                    controls,
                ));
                let mut controls = vec![toggle!(
                    "common.enable",
                    adaptive_engine_process.io_priority.enabled
                )];
                controls.push(choice!(
                    s,
                    "common.focus_process",
                    ProcessIoPrioritySetting,
                    if s.advanced.expose_all_priority_values {
                        &ProcessIoPrioritySetting::ADVANCED_ALL
                    } else {
                        &ProcessIoPrioritySetting::ALL
                    },
                    process_io_priority_setting_label,
                    adaptive_engine_process.io_priority.foreground_priority
                ));
                controls.push(choice!(
                    s,
                    "common.visible_window",
                    ProcessIoPrioritySetting,
                    if s.advanced.expose_all_priority_values {
                        &ProcessIoPrioritySetting::ADVANCED_ALL
                    } else {
                        &ProcessIoPrioritySetting::ALL
                    },
                    process_io_priority_setting_label,
                    adaptive_engine_process.io_priority.visible_window_priority
                ));
                controls.push(choice!(
                    s,
                    "common.background_process",
                    ProcessIoPrioritySetting,
                    if s.advanced.expose_all_priority_values {
                        &ProcessIoPrioritySetting::ADVANCED_ALL
                    } else {
                        &ProcessIoPrioritySetting::ALL
                    },
                    process_io_priority_setting_label,
                    adaptive_engine_process.io_priority.background_priority
                ));
                let options = vec![
                    toggle!(
                        "adaptive_engine.detection",
                        adaptive_engine_process
                            .io_priority
                            .foreground_detection_enabled
                    ),
                    toggle!(
                        "common.visible_window_detection",
                        adaptive_engine_process
                            .io_priority
                            .visible_window_detection_enabled
                    ),
                    toggle!(
                        "common.preserve_foreground_priority",
                        adaptive_engine_process
                            .io_priority
                            .preserve_foreground_priority
                    ),
                    toggle!(
                        "common.preserve_visible_window_priority",
                        adaptive_engine_process
                            .io_priority
                            .preserve_visible_window_priority
                    ),
                    toggle!(
                        "common.preserve_background_priority",
                        adaptive_engine_process
                            .io_priority
                            .preserve_background_priority
                    ),
                ];
                controls.push(ui.collapsible(
                    t!("adaptive_engine.detection").to_string(),
                    self.priority_expanded[usize::from(preset)][4],
                    widgets::col().children(options),
                    |_| Message::TogglePriority(4),
                ));
                body.push(widgets::card(t!("nav.io_priority").to_string(), controls));
                let mut controls = vec![toggle!(
                    "common.enable",
                    adaptive_engine_process.gpu_priority.enabled
                )];
                controls.push(choice!(
                    s,
                    "common.focus_process",
                    ProcessGpuPrioritySetting,
                    if s.advanced.expose_all_priority_values {
                        &ProcessGpuPrioritySetting::ADVANCED_ALL
                    } else {
                        &ProcessGpuPrioritySetting::ALL
                    },
                    process_gpu_priority_setting_label,
                    adaptive_engine_process.gpu_priority.foreground_priority
                ));
                controls.push(choice!(
                    s,
                    "common.visible_window",
                    ProcessGpuPrioritySetting,
                    if s.advanced.expose_all_priority_values {
                        &ProcessGpuPrioritySetting::ADVANCED_ALL
                    } else {
                        &ProcessGpuPrioritySetting::ALL
                    },
                    process_gpu_priority_setting_label,
                    adaptive_engine_process.gpu_priority.visible_window_priority
                ));
                controls.push(choice!(
                    s,
                    "common.background_process",
                    ProcessGpuPrioritySetting,
                    if s.advanced.expose_all_priority_values {
                        &ProcessGpuPrioritySetting::ADVANCED_ALL
                    } else {
                        &ProcessGpuPrioritySetting::ALL
                    },
                    process_gpu_priority_setting_label,
                    adaptive_engine_process.gpu_priority.background_priority
                ));
                let options = vec![
                    toggle!(
                        "adaptive_engine.detection",
                        adaptive_engine_process
                            .gpu_priority
                            .foreground_detection_enabled
                    ),
                    toggle!(
                        "common.visible_window_detection",
                        adaptive_engine_process
                            .gpu_priority
                            .visible_window_detection_enabled
                    ),
                    toggle!(
                        "common.preserve_foreground_priority",
                        adaptive_engine_process
                            .gpu_priority
                            .preserve_foreground_priority
                    ),
                    toggle!(
                        "common.preserve_visible_window_priority",
                        adaptive_engine_process
                            .gpu_priority
                            .preserve_visible_window_priority
                    ),
                    toggle!(
                        "common.preserve_background_priority",
                        adaptive_engine_process
                            .gpu_priority
                            .preserve_background_priority
                    ),
                ];
                controls.push(ui.collapsible(
                    t!("adaptive_engine.detection").to_string(),
                    self.priority_expanded[usize::from(preset)][5],
                    widgets::col().children(options),
                    |_| Message::TogglePriority(5),
                ));
                body.push(widgets::card(t!("nav.gpu_priority").to_string(), controls));
                let mut controls = vec![toggle!(
                    "common.enable",
                    adaptive_engine_process.memory_priority_enabled
                )];
                controls.push(choice!(
                    s,
                    "common.focus_process",
                    ProcessMemoryPrioritySetting,
                    &ProcessMemoryPrioritySetting::ALL,
                    process_memory_priority_setting_label,
                    adaptive_engine_process.focus_process_memory_priority
                ));
                controls.push(choice!(
                    s,
                    "common.visible_window",
                    ProcessMemoryPrioritySetting,
                    &ProcessMemoryPrioritySetting::ALL,
                    process_memory_priority_setting_label,
                    adaptive_engine_process.visible_window_memory_priority
                ));
                controls.push(choice!(
                    s,
                    "common.background_process",
                    ProcessMemoryPrioritySetting,
                    &ProcessMemoryPrioritySetting::ALL,
                    process_memory_priority_setting_label,
                    adaptive_engine_process.background_memory_priority
                ));
                let options = vec![
                    toggle!(
                        "adaptive_engine.detection",
                        adaptive_engine_process.memory_priority_foreground_detection_enabled
                    ),
                    toggle!(
                        "common.visible_window_detection",
                        adaptive_engine_process.memory_priority_visible_window_detection_enabled
                    ),
                    toggle!(
                        "common.preserve_foreground_priority",
                        adaptive_engine_process.memory_priority_preserve_foreground
                    ),
                    toggle!(
                        "common.preserve_visible_window_priority",
                        adaptive_engine_process.memory_priority_preserve_visible_window
                    ),
                    toggle!(
                        "common.preserve_background_priority",
                        adaptive_engine_process.memory_priority_preserve_background
                    ),
                ];
                controls.push(ui.collapsible(
                    t!("adaptive_engine.detection").to_string(),
                    self.priority_expanded[usize::from(preset)][6],
                    widgets::col().children(options),
                    |_| Message::TogglePriority(6),
                ));
                body.push(widgets::card(
                    t!("nav.memory_priority").to_string(),
                    controls,
                ));
            }
            TuningTab::CustomRules => {
                if !preset {
                    let candidate_rules = s.adaptive_engine_process.custom_rules.clone();
                    body.push(super::app_picker::view(
                        &self.path,
                        candidates,
                        true,
                        Message::Path,
                        Message::Browse,
                        Some(Message::AddExclusion),
                        move |candidate| {
                            super::process_rules::can_add_process_candidate(
                                &candidate.info.image_path.to_string_lossy(),
                                |path| {
                                    candidate_rules.iter().any(|rule| {
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
                        ui.clone(),
                    ));
                    for (index, rule) in s.adaptive_engine_process.custom_rules.iter().enumerate() {
                        body.push(widgets::card(
                            rule.executable_path.clone(),
                            [
                                ui.checkbox(
                                    t!("common.enable").to_string(),
                                    rule.enabled,
                                    move |value| Message::ExclusionEnabled(index, value),
                                ),
                                ui.button(
                                    t!("common.remove").to_string(),
                                    Message::RemoveExclusion(index),
                                ),
                            ],
                        ));
                    }
                    if s.adaptive_engine_process.custom_rules.is_empty() {
                        body.push(widgets::label(t!("common.no_custom_rules").to_string()));
                    }
                }
            }
        }
        if let Some(error) = self.validation_error() {
            body.push(widgets::error(error));
        }
        if preset {
            widgets::col().children(body)
        } else {
            widgets::page(body)
        }
    }

    pub(super) fn side_panel(
        &self,
        live: &crate::application::SettingsEditor,
        status: &RuntimeStatusSnapshot,
        ui: Ui<Message>,
    ) -> Element {
        let tabs = ui.segmented(
            [
                Choice(false, t!("common.status").to_string()),
                Choice(true, t!("adaptive_engine.presets").to_string()),
            ],
            Choice(
                self.presets_tab,
                t!(if self.presets_tab {
                    "adaptive_engine.presets"
                } else {
                    "common.status"
                })
                .to_string(),
            ),
            |value| Message::RailTab(value.0),
        );
        let panel = widgets::col()
            .fill()
            .child(Element::row().width_match().padding_xy(16, 0).child(tabs));
        let mut body = Vec::new();
        if self.presets_tab {
            body.push(widgets::heading(
                t!("adaptive_engine.built_in_presets").to_string(),
            ));
            for preset in BuiltInAdaptiveEnginePreset::ALL {
                body.push(widgets::row().children([
                    ui.button(
                        built_in_adaptive_engine_preset_label(preset),
                        Message::BuiltIn(preset),
                    ),
                    ui.button(
                        t!("common.feature_info").to_string(),
                        Message::ViewBuiltIn(preset),
                    ),
                ]));
            }
            body.push(widgets::heading(
                t!("adaptive_engine.custom_presets").to_string(),
            ));
            if live.adaptive_engine_presets.is_empty() {
                body.push(widgets::label(
                    t!("adaptive_engine.no_custom_presets").to_string(),
                ));
            }
            for (index, preset) in live.adaptive_engine_presets.iter().enumerate() {
                body.push(widgets::row().children([
                    ui.button(preset.name.clone(), Message::Apply(index)),
                    ui.button(
                        t!("adaptive_engine.edit_preset").to_string(),
                        Message::Edit(index),
                    ),
                    ui.button(t!("common.remove").to_string(), Message::Delete(index)),
                ]));
            }
            body.push(ui.button(t!("adaptive_engine.add_preset").to_string(), Message::New));
        } else if let Some(status) = super::status_rail::view(
            crate::ui::Page::AdaptiveEngine,
            live,
            status,
            &[],
            ui.map(Message::Status),
        ) {
            return panel.child(status.weight(1.0));
        }
        panel.child(
            super::scrolling::scrollable(widgets::col().padding_xy(16, 0).children(body))
                .weight(1.0),
        )
    }
}

fn trailing_control(control: Element) -> Element {
    Element::col()
        .weight(1.0)
        .cross(Align::End)
        .child(control.max_width(240))
}

fn setting(key: &str, unit: &str, control: Element) -> Element {
    let help_key = match key {
        "processor_power.core_parking_min" => "adaptive_engine.core_parking_min_help".to_string(),
        "processor_power.processor_min" => "adaptive_engine.processor_min_help".to_string(),
        "processor_power.processor_max" => "adaptive_engine.processor_max_help".to_string(),
        "processor_power.boost_policy" => "adaptive_engine.base_boost_policy_help".to_string(),
        _ => format!("{key}_help"),
    };
    let title = widgets::label_with_unit(&t!(key), unit);
    let help = t!(&help_key).to_string();
    if help != help_key {
        widgets::setting_row_with_help(title, help, control)
    } else {
        Element::setting_row(title, control).width_match()
    }
}

impl std::fmt::Display for TuningTab {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&t!(self.key()))
    }
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
    fn tuning_label_hover_resolves_its_help_including_processor_power_overrides() {
        use windui::{core::Tree, geometry::Size, signal::SignalScope, text::NullTextEngine};

        let mut scope = SignalScope::new();
        scope.collect(|| {
            for (key, unit, help_key) in [
                (
                    "adaptive_engine_process.reaction_time",
                    "ms",
                    "adaptive_engine_process.reaction_time_help",
                ),
                (
                    "processor_power.core_parking_min",
                    "%",
                    "adaptive_engine.core_parking_min_help",
                ),
                (
                    "processor_power.processor_min",
                    "%",
                    "adaptive_engine.processor_min_help",
                ),
                (
                    "processor_power.processor_max",
                    "%",
                    "adaptive_engine.processor_max_help",
                ),
                (
                    "processor_power.boost_policy",
                    "",
                    "adaptive_engine.base_boost_policy_help",
                ),
            ] {
                let mut tree = Tree::new();
                let root =
                    setting(key, unit, Element::button("Control").width(160)).build(&mut tree);
                tree.root = Some(root);
                tree.layout_root(Size::new(640, 80), &mut NullTextEngine);
                let label = tree
                    .get(tree.get(root).unwrap().children[0])
                    .unwrap()
                    .children[0];
                let bounds = tree.abs_bounds(label);
                let hit = tree
                    .hit_test(windui::geometry::Point::new(
                        bounds.x + 2,
                        bounds.y + bounds.h / 2,
                    ))
                    .unwrap();
                assert_eq!(hit, label);
                assert_eq!(
                    tree.node_tooltip(hit),
                    Some(t!(help_key).to_string()),
                    "{key}"
                );
            }
        });
    }

    #[test]
    fn narrow_editor_keeps_preset_label_and_scrolls_tuning_tabs() {
        use windui::{
            core::Tree, geometry::Size, signal::SignalScope, spec::Axis, text::NullTextEngine,
        };

        let mut scope = SignalScope::new();
        scope.collect(|| {
            let mut tree = Tree::new();
            let root = Editor::default()
                .view(&Settings::default(), &[], Ui::new(|_, _| {}))
                .build(&mut tree);
            tree.root = Some(root);
            let page = tree.get(root).unwrap().children[0];
            let body = tree.get(page).unwrap().children[0];
            let children = tree.get(body).unwrap().children.clone();
            let master = children[0];
            let preset = tree.get(master).unwrap().children[2];
            let preset_children = tree.get(preset).unwrap().children.clone();
            let control = tree.get(preset_children[1]).unwrap().children[0];
            let tabs = children[1];

            for width in [316, 1280] {
                tree.layout_root(Size::new(width, 620), &mut NullTextEngine);
                let label = tree.abs_bounds(preset_children[0]);
                let control = tree.abs_bounds(control);
                assert!(
                    label.w >= 80,
                    "narrow controls must leave the label readable"
                );
                assert!(control.w >= 80 && control.w <= 240);
                assert!(label.x + label.w <= control.x);
                let row = tree.abs_bounds(preset);
                assert_eq!(control.x + control.w, row.x + row.w);
                let (_, range) = tree.scroll_range(tabs, Axis::Horizontal).unwrap();
                assert_eq!(range > 0, width == 316);
                if range > 0 {
                    assert!(tree.set_scroll_x(tabs, range));
                    tree.layout_root(Size::new(width, 620), &mut NullTextEngine);
                    assert_eq!(tree.scroll_range(tabs, Axis::Horizontal).unwrap().0, range);
                }
            }
        });
    }

    #[test]
    fn side_panel_has_one_scroller_and_keeps_tabs_fixed() {
        use windui::{
            core::{Layout, Tree},
            geometry::Size,
            signal::SignalScope,
            spec::Axis,
            text::NullTextEngine,
        };

        for presets_tab in [false, true] {
            let mut scope = SignalScope::new();
            scope.collect(|| {
                let editor = Editor {
                    presets_tab,
                    ..Default::default()
                };
                let settings =
                    crate::application::SettingsEditor::with_settings(Settings::default());
                let mut tree = Tree::new();
                let root = editor
                    .side_panel(
                        &settings,
                        &RuntimeStatusSnapshot::default(),
                        Ui::new(|_, _| {}),
                    )
                    .build(&mut tree);
                tree.root = Some(root);
                tree.layout_root(Size::new(272, 80), &mut NullTextEngine);
                let tabs = tree.get(root).unwrap().children[0];
                let tabs_bounds = tree.abs_bounds(tabs);
                let mut pending = vec![root];
                let mut scrolls = Vec::new();
                while let Some(id) = pending.pop() {
                    let node = tree.get(id).unwrap();
                    if matches!(node.layout, Layout::Scroll) {
                        scrolls.push(id);
                    }
                    pending.extend(node.children.iter().copied());
                }
                assert_eq!(
                    scrolls.len(),
                    1,
                    "status and presets each need one scroll owner"
                );
                let scroll = scrolls[0];
                assert!(tree.scroll_range(scroll, Axis::Vertical).unwrap().1 > 0);
                assert!(tree.set_scroll_y(scroll, 20));
                tree.layout_root(Size::new(272, 80), &mut NullTextEngine);
                assert_eq!(tree.abs_bounds(tabs), tabs_bounds);
                assert_eq!(tree.scroll_range(scroll, Axis::Vertical).unwrap().0, 20);
            });
        }
    }

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
