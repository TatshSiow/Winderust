use super::design;
use super::widgets::{
    action_button, checkbox, icon_button, pick_list, preset_list, preset_panel, preset_row,
    preset_select, text_input,
};
use crate::application::SettingsEditor;
use crate::config::*;
use crate::foreground::process_list::{normalize_path_slashes, same_executable_name};
use crate::ui::process_rules::{
    can_add_exact_process_candidate, can_add_executable_name, exact_path_input, picked_path_input,
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

const FOREGROUND_THRESHOLD_FIELD: &str =
    stringify!(adaptive_engine_process.foreground_cpu_threshold_percent);
const FOREGROUND_THRESHOLD_LABEL: &str = "adaptive_engine_process.foreground_cpu_threshold";
const SHARED_THRESHOLD_FIELD: &str =
    stringify!(adaptive_engine_process.foreground_or_system_cpu_threshold_percent);
const SHARED_THRESHOLD_LABEL: &str = "adaptive_engine_process.pressure_threshold";
const SYSTEM_THRESHOLD_LABEL: &str = "adaptive_engine_process.system_cpu_threshold";
const MINIMUM_RESTRAINT_TIME_FIELD: &str =
    stringify!(adaptive_engine_process.minimum_restraint_time_ms);
const MAXIMUM_RESTRAINT_TIME_FIELD: &str =
    stringify!(adaptive_engine_process.maximum_restraint_time_ms);

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
            Self::CustomRules => "common.rules",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum TuningGroup {
    SharedDetection,
    BackgroundProcessors,
    ResourceZones,
    CpuPressureRestraint,
}

impl TuningGroup {
    const COUNT: usize = Self::CpuPressureRestraint as usize + 1;
}

fn can_add_path_exclusion(settings: &AdaptiveEngineProcessSettings, path: &str) -> bool {
    can_add_exact_process_candidate(
        path,
        |path| {
            settings.custom_rules.iter().any(|rule| {
                rule.selector
                    .exact_path()
                    .is_some_and(|target| same_rule_executable_path(target, path))
            })
        },
        crate::adaptive_engine_process::is_builtin_excluded,
    )
}

fn can_add_name_exclusion(settings: &AdaptiveEngineProcessSettings, name: &str) -> bool {
    !crate::adaptive_engine_process::is_builtin_excluded(name)
        && can_add_executable_name(
            settings
                .custom_rules
                .iter()
                .filter_map(|rule| rule.selector.exact()),
            name,
        )
}

fn has_pattern_exclusion(settings: &AdaptiveEngineProcessSettings, pattern: &str) -> bool {
    settings.custom_rules.iter().any(|rule| {
        matches!(&rule.selector, AdaptiveEngineExclusionSelector::NamePattern { executable_name_pattern }
            if same_executable_name(executable_name_pattern.as_str(), pattern))
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ExclusionMatchBy {
    #[default]
    Path,
    Name,
    NamePattern,
}

impl ExclusionMatchBy {
    const ALL: [Self; 3] = [Self::Path, Self::Name, Self::NamePattern];

    fn key(self) -> &'static str {
        match self {
            Self::Path => "common.executable_path",
            Self::Name => "common.executable_name",
            Self::NamePattern => "adaptive_engine_process.executable_name_pattern",
        }
    }
}

impl std::fmt::Display for ExclusionMatchBy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(self.key()))
    }
}

#[derive(Default)]
pub(super) struct Editor {
    name: String,
    selected_preset: Option<Choice<usize>>,
    draft: Option<Settings>,
    editing: Option<usize>,
    preview: Option<(PowerSourceProfile, usize)>,
    tuning_tabs: [TuningTab; 2],
    expanded: [[bool; TuningGroup::COUNT]; 2],
    match_by: ExclusionMatchBy,
    path: String,
    executable_name: String,
    executable_name_pattern: String,
    error: String,
    live_numbers: NumberInputs,
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

    fn update_shared_threshold_label(&mut self, separate: bool) {
        if let Some((_, _, label)) = self
            .drafts
            .get_mut(SHARED_THRESHOLD_FIELD)
            .and_then(|draft| draft.invalid.as_mut())
        {
            *label = if separate {
                SYSTEM_THRESHOLD_LABEL
            } else {
                SHARED_THRESHOLD_LABEL
            };
        }
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
    SeparateForegroundThreshold(bool),
    InterruptTiming(super::interrupt_timing::Message),
    TuningTab(TuningTab),
    Collapse(TuningGroup),
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
    ViewCustom(usize),
    Apply(usize),
    RestoreDefaults,
    UsePreset,
    Edit(usize),
    New,
    Name(String),
    Save,
    Delete(usize),
    Cancel,
    UseCurrent,
    MatchBy(ExclusionMatchBy),
    Path(String),
    ExecutableName(String),
    ExecutableNamePattern(String),
    Browse,
    AddExclusion,
    RemoveExclusion(usize),

    ExclusionEnabled(usize, bool),
}
impl Editor {
    pub(super) fn update(&mut self, s: &mut SettingsEditor, m: Message) {
        if matches!(
            m,
            Message::ViewBuiltIn(_)
                | Message::ViewCustom(_)
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
            Message::Collapse(group) => {
                let expanded =
                    &mut self.expanded[usize::from(self.draft.is_some())][group as usize];
                *expanded = !*expanded;
            }
            message @ (Message::ViewBuiltIn(p) | Message::BuiltIn(p)) => {
                let Some(index) = BuiltInAdaptiveEnginePreset::ALL
                    .iter()
                    .position(|v| *v == p)
                else {
                    return;
                };
                let name = built_in_adaptive_engine_preset_label(p);
                let mut draft = (**s).clone();
                apply_built_in_adaptive_engine_preset(&mut draft, p);
                if matches!(message, Message::ViewBuiltIn(_)) {
                    self.draft = Some(draft);
                    self.name = name;
                    self.editing = None;
                    self.preview = Some((s.selected_power_source(), index));
                } else {
                    self.stage_preset(s, index, &capture_adaptive_engine_preset(&draft, name));
                }
            }
            Message::Cancel => {
                self.draft = None;
                self.preview = None;
            }
            Message::Apply(i) => {
                if let Some(p) = s.global().adaptive_engine_presets.get(i).cloned() {
                    self.stage_preset(s, i + BuiltInAdaptiveEnginePreset::ALL.len(), &p);
                }
            }
            Message::RestoreDefaults if self.draft.is_none() => {
                let mut defaults = s.adaptive_engine_process.clone();
                defaults.restore_cpu_behaviour_defaults();
                if defaults != s.adaptive_engine_process {
                    if s.selected_power_source() == PowerSourceProfile::PluggedIn {
                        s.battery_profile_mut();
                    }
                    s.adaptive_engine_process = defaults;
                }
                self.selected_preset = None;
                self.live_numbers.drafts.clear();
                self.error.clear();
            }
            Message::UsePreset => {
                let Some((_, index)) = self
                    .preview
                    .filter(|(source, _)| *source == s.selected_power_source())
                else {
                    return;
                };
                let Some(draft) = self.draft.take() else {
                    return;
                };
                self.preview = None;
                let preset = capture_adaptive_engine_preset(&draft, self.name.clone());
                self.stage_preset(s, index, &preset);
            }
            message @ (Message::ViewCustom(i) | Message::Edit(i)) => {
                if let Some(p) = s.global().adaptive_engine_presets.get(i) {
                    let mut draft = (**s).clone();
                    apply_adaptive_engine_preset(&mut draft, p);
                    self.name = p.name.clone();
                    let preview = matches!(message, Message::ViewCustom(_));
                    self.editing = (!preview).then_some(i);
                    self.preview = preview.then_some((
                        s.selected_power_source(),
                        i + BuiltInAdaptiveEnginePreset::ALL.len(),
                    ));
                    self.draft = Some(draft);
                }
            }
            Message::New => {
                self.draft = Some((**s).clone());
                self.name.clear();
                self.editing = None;
                self.preview = None;
            }
            Message::Name(v) if self.preview.is_none() => self.name = v,
            Message::Save => {
                if let Some(draft) = &self.draft {
                    if self.preview.is_some() {
                        return;
                    }
                    let cpu = &draft.adaptive_engine_process;
                    if let Err(error) = cpu.validate_cpu_behaviour().and_then(|()| {
                        cpu.dynamic_resource_zone_settings
                            .validate(cpu.dynamic_resource_zones_enabled)
                    }) {
                        self.error = error;
                        return;
                    }
                    let name = self.name.trim();
                    if self.preset_numbers.invalid().is_none()
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
                        self.error.clear();
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
                self.preview = None;
                self.preset_numbers.drafts.clear();
            }
            Message::UseCurrent => {
                if self.preview.is_none() {
                    self.draft = Some((**s).clone())
                }
            }
            Message::MatchBy(match_by) => self.match_by = match_by,
            Message::Path(v) => self.path = v,
            Message::ExecutableName(v) => self.executable_name = v,
            Message::ExecutableNamePattern(v) => self.executable_name_pattern = v,
            Message::Browse => {}
            Message::AddExclusion => {
                let rule = match self.match_by {
                    ExclusionMatchBy::Path => {
                        let path = exact_path_input(&self.path);
                        if !can_add_path_exclusion(&s.adaptive_engine_process, path) {
                            return;
                        }
                        let rule =
                            AdaptiveEngineExclusionRule::exact_path(normalize_path_slashes(path));
                        self.path.clear();
                        rule
                    }
                    ExclusionMatchBy::Name => {
                        if !can_add_name_exclusion(
                            &s.adaptive_engine_process,
                            &self.executable_name,
                        ) {
                            return;
                        }
                        AdaptiveEngineExclusionRule::exact_name(std::mem::take(
                            &mut self.executable_name,
                        ))
                    }
                    ExclusionMatchBy::NamePattern => {
                        let Ok(executable_name_pattern) =
                            ExecutableNamePattern::try_from(self.executable_name_pattern.clone())
                        else {
                            return;
                        };
                        if has_pattern_exclusion(
                            &s.adaptive_engine_process,
                            executable_name_pattern.as_str(),
                        ) {
                            return;
                        }
                        self.executable_name_pattern.clear();
                        AdaptiveEngineExclusionRule {
                            enabled: true,
                            selector: AdaptiveEngineExclusionSelector::NamePattern {
                                executable_name_pattern,
                            },
                        }
                    }
                };
                s.adaptive_engine_process.custom_rules.push(rule);
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
                if self.preview.is_some() {
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
                    Message::SeparateForegroundThreshold(enabled) => {
                        let (target, numbers) = match &mut self.draft {
                            Some(draft) => (draft, &mut self.preset_numbers),
                            None => (&mut **s, &mut self.live_numbers),
                        };
                        let cpu = &mut target.adaptive_engine_process;
                        if cpu.pressure_source.supports_separate_foreground_threshold() {
                            cpu.foreground_cpu_threshold_percent = enabled.then(|| {
                                cpu.foreground_cpu_threshold_percent.unwrap_or_else(|| {
                                    cpu.foreground_or_system_cpu_threshold_percent.clamp(1, 100)
                                })
                            });
                            numbers.drafts.remove(FOREGROUND_THRESHOLD_FIELD);
                            numbers.update_shared_threshold_label(enabled);
                        }
                    }
                    Message::Number(f, v, min, max, key, label) => {
                        let numbers = if self.draft.is_some() {
                            if matches!(
                                key,
                                MINIMUM_RESTRAINT_TIME_FIELD | MAXIMUM_RESTRAINT_TIME_FIELD
                            ) {
                                self.error.clear();
                            }
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
                    Message::Choice(f, v) => {
                        let (target, numbers) = match &mut self.draft {
                            Some(draft) => (draft, &mut self.preset_numbers),
                            None => (&mut **s, &mut self.live_numbers),
                        };
                        f(target, v);
                        let inactive_limit = if target
                            .adaptive_engine_process
                            .cpu_allocation_method
                            .uses_processor_selection()
                        {
                            "adaptive_engine_process.processors_to_remove"
                        } else {
                            "adaptive_engine_process.processor_limit_percent"
                        };
                        numbers.drafts.remove(inactive_limit);
                        let separate = target
                            .adaptive_engine_process
                            .active_foreground_cpu_threshold()
                            .is_some();
                        if !separate {
                            numbers.drafts.remove(FOREGROUND_THRESHOLD_FIELD);
                        }
                        numbers.update_shared_threshold_label(separate);
                    }
                    _ => {}
                }
            }
        }
    }
    fn stage_preset(
        &mut self,
        settings: &mut SettingsEditor,
        index: usize,
        preset: &AdaptiveEnginePreset,
    ) {
        self.error.clear();
        self.live_numbers.drafts.clear();
        self.selected_preset = Some(Choice(index, preset.name.clone()));
        apply_adaptive_engine_preset(settings, preset);
    }

    fn set_cpu_selection(
        &mut self,
        s: &mut SettingsEditor,
        zone: bool,
        selection: crate::cpu_topology::CpuSelection,
        topology: &Result<crate::cpu_topology::CpuTopology, String>,
    ) {
        if self.preview.is_some() {
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
        let current = comparable_adaptive_engine_process_tuning(&live.adaptive_engine_process);
        let preferred = self
            .selected_preset
            .as_ref()
            .filter(|choice| options.contains(choice));
        preferred
            .into_iter()
            .chain(options.iter())
            .find(|choice| {
                if let Some(preset) = BuiltInAdaptiveEnginePreset::ALL.get(choice.0) {
                    let mut candidate = current.clone();
                    apply_adaptive_engine_process_preset(&mut candidate, *preset);
                    candidate == current
                } else {
                    comparable_adaptive_engine_process_tuning(
                        &live.adaptive_engine_presets
                            [choice.0 - BuiltInAdaptiveEnginePreset::ALL.len()]
                        .adaptive_engine_process,
                    ) == current
                }
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
        self.live_numbers.validation_error()
    }

    pub(super) fn preset_open(&self) -> bool {
        self.draft.is_some()
    }
    pub(super) fn has_pending_editor(&self) -> bool {
        self.preset_open() && self.preview.is_none()
    }
    pub(super) fn view<'a>(
        &'a self,
        live: &'a Settings,
        candidates: &'a [super::app_picker::Candidate],
        interrupt_timing: &'a super::interrupt_timing::State,
        shell_height: f32,
    ) -> Element<'a, Message> {
        const PINNED_HEADER_MIN_HEIGHT: f32 = 750.0;
        self.tuning_view(
            live,
            candidates,
            Some(interrupt_timing),
            shell_height < PINNED_HEADER_MIN_HEIGHT,
        )
    }

    pub(super) fn preset_modal<'a>(
        &'a self,
        live: &'a Settings,
        candidates: &'a [super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        use iced::widget::Space;
        let read_only = self.preview.is_some();
        let title = if read_only {
            self.name.clone()
        } else {
            t!(if self.editing.is_some() {
                "common.edit_preset"
            } else {
                "common.add_preset"
            })
            .to_string()
        };
        let header = row![
            text(title).width(Fill),
            icon_button("icons/x.svg").on_press(Message::Cancel)
        ]
        .align_y(iced::Center);
        let mut footer = row![Space::new().width(Fill)]
            .spacing(design::space::SMALL)
            .align_y(iced::Center);
        if let Some(index) = self.editing {
            footer = footer.push(
                action_button(t!("common.remove").to_string())
                    .style(super::widgets::danger_button)
                    .on_press(Message::Delete(index)),
            );
        }
        if !read_only {
            footer = footer.push(
                action_button(t!("adaptive_engine.use_current_settings").to_string())
                    .on_press(Message::UseCurrent),
            );
        }
        footer = footer.push(
            action_button(t!("common.cancel").to_string())
                .style(super::widgets::tertiary_button)
                .on_press(Message::Cancel),
        );
        if read_only {
            footer = footer.push(
                action_button(t!("common.use_preset").to_string())
                    .style(super::widgets::primary_button)
                    .on_press(Message::UsePreset),
            );
        } else {
            footer = footer.push(
                action_button(t!("common.save").to_string())
                    .style(super::widgets::primary_button)
                    .on_press_maybe(
                        (!self.name.trim().is_empty() && self.preset_numbers.invalid().is_none())
                            .then_some(Message::Save),
                    ),
            );
        }
        let mut body = column![super::widgets::heading(
            t!("common.preset_details").to_string(),
            design::typography::BODY
        )]
        .spacing(design::space::MEDIUM)
        .height(Fill);
        if !read_only {
            body = body.push(super::widgets::settings_card(
                column![
                    text(t!("adaptive_engine.preset_name").to_string()),
                    text_input(&t!("adaptive_engine.preset_name_placeholder"), &self.name)
                        .on_input(Message::Name),
                    text(t!("adaptive_engine.preset_name_help").to_string()).style(text::secondary)
                ]
                .spacing(design::space::SMALL),
            ));
        }
        body = body.push(self.tuning_view(live, candidates, None, false));
        let error = self
            .preset_numbers
            .validation_error()
            .unwrap_or_else(|| self.error.clone());
        if !error.is_empty() {
            body = body.push(text(error).style(text::danger));
        }
        let frame = super::widgets::modal_frame(header, body, footer, (1200, 850));
        if read_only {
            frame
        } else {
            super::modal::dismiss_on_escape(frame, Message::Cancel)
        }
    }

    fn tuning_view<'a>(
        &'a self,
        live: &'a Settings,
        candidates: &'a [super::app_picker::Candidate],
        interrupt_timing: Option<&'a super::interrupt_timing::State>,
        scroll_live_header: bool,
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
        let editable = !preset || self.preview.is_none();
        let mut body = column![].spacing(super::widgets::CARD_GAP);
        macro_rules! number {
            ($key:expr, $min:expr, $max:expr, $($field:ident).+) => {{
                let key = stringify!($($field).+);
                let draft = numbers.drafts.get(key);
                let current = s.$($field).+;
                let unit = if key.ends_with("_ms") {
                    "ms"
                } else if key.ends_with("_seconds") {
                    "s"
                } else if key.ends_with("_percent") || $max == 100 {
                    "%"
                } else {
                    ""
                };
                super::widgets::numeric_setting($key, unit, move || {
                    let formatted;
                    let value = if let Some(draft) = draft {
                        &draft.text
                    } else {
                        formatted = current.to_string();
                        &formatted
                    };
                    super::widgets::stepper(
                        value,
                        $min..=$max,
                        1,
                        editable.then_some(move |value| {
                            Message::Number(|s, n| s.$($field).+ = n as _, value, $min, $max, key, $key)
                        }),
                    )
                })
            }};
        }
        macro_rules! selector {($s:ident,$key:expr,$ty:ty,$options:expr,$label:expr,$($field:ident).+) => {{let values:&[$ty]=$options;let selected=s.$($field).+;let control:Element<'_,Message>=if editable{pick_list(values.iter().copied().map(|v|Choice(v,$label(v))).collect::<Vec<_>>(),Some(Choice(selected,$label(selected))),move |v|Message::Choice(|$s,i|{let options:&[$ty]=$options;if let Some(v)=options.get(i){$s.$($field).+ = *v;}},values.iter().position(|x|*x==v.0).unwrap_or(0))).width(Fill).into()}else{text($label(selected)).into()};control}};}
        macro_rules! choice {($s:ident,$key:expr,$ty:ty,$options:expr,$label:expr,$($field:ident).+) => {row![setting_label($key).width(Fill),iced::widget::container(selector!($s,$key,$ty,$options,$label,$($field).+)).width(280)].spacing(design::space::SMALL).padding([design::space::CONTROL as u16, 0]).align_y(iced::Center)};}
        let tab = self.tuning_tabs[usize::from(preset)];
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
            let built_ins = options.len();
            options.extend(
                live.adaptive_engine_presets
                    .iter()
                    .enumerate()
                    .map(|(i, p)| Choice(i + built_ins, p.name.clone())),
            );
            let selected = self.selected_preset(live, &options);
            let picker = super::widgets::setting_row(
                "adaptive_engine.preset",
                preset_select(options, selected, move |choice| {
                    if let Some(&preset) = BuiltInAdaptiveEnginePreset::ALL.get(choice.0) {
                        Message::BuiltIn(preset)
                    } else {
                        Message::Apply(choice.0 - built_ins)
                    }
                }),
            );
            let restore = (tab == TuningTab::CpuBehaviour).then(|| {
                super::widgets::setting_row(
                    "adaptive_engine.cpu_behaviour_defaults",
                    action_button(t!("adaptive_engine.restore_defaults").to_string())
                        .on_press(Message::RestoreDefaults),
                )
                .into()
            });
            body = body.push(super::widgets::settings_card(
                super::widgets::settings_rows(std::iter::once(picker.into()).chain(restore)),
            ));
        }
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
                use TuningGroup::{
                    BackgroundProcessors, CpuPressureRestraint, ResourceZones, SharedDetection,
                };

                let cpu = &s.adaptive_engine_process;
                let placement_only = cpu.restraint_priority.is_placement_only();
                let method = cpu.cpu_allocation_method;
                let selection = cpu.background_processor_selection;
                let custom_selection = editable
                    && ((method.uses_processor_selection()
                        && selection == BackgroundProcessorSelection::Custom)
                        || cpu
                            .dynamic_resource_zone_settings
                            .background_processor_selection
                            == BackgroundProcessorSelection::Custom);
                let cpu_catalog = custom_selection.then(|| {
                    super::cpu_allocation::SelectionCatalog::capture(
                        super::cpu_allocation::Kind::Hard,
                    )
                });
                let supports_split = cpu.pressure_source.supports_separate_foreground_threshold();
                let foreground_threshold = cpu.active_foreground_cpu_threshold();
                let pressure_source = choice!(
                    s,
                    "adaptive_engine_process.pressure_source",
                    AdaptivePressureSource,
                    &[
                        AdaptivePressureSource::Either,
                        AdaptivePressureSource::System,
                        AdaptivePressureSource::Foreground,
                        AdaptivePressureSource::Both,
                    ],
                    pressure_source_label,
                    adaptive_engine_process.pressure_source
                )
                .into();
                let separate_threshold = supports_split.then(|| {
                    super::widgets::setting_row(
                        "adaptive_engine_process.separate_foreground_cpu_threshold",
                        super::widgets::switch(
                            foreground_threshold.is_some(),
                            editable.then_some(Message::SeparateForegroundThreshold),
                        ),
                    )
                    .into()
                });
                let threshold_slider =
                    |value: u8,
                     key: &'static str,
                     label: &'static str,
                     apply: fn(&mut Settings, u64)| {
                        if editable {
                            super::widgets::slider_setting(label, "%", move || {
                                super::widgets::slider_stepper(
                                    u64::from(value),
                                    numbers.drafts.get(key).map(|draft| draft.text.as_str()),
                                    1..=100,
                                    1,
                                    move |value| Message::Number(apply, value, 1, 100, key, label),
                                )
                            })
                        } else {
                            super::widgets::numeric_setting(label, "%", move || {
                                super::widgets::stepper(
                                    &value.to_string(),
                                    1..=100,
                                    1,
                                    None::<fn(String) -> Message>,
                                )
                            })
                        }
                    };
                let shared_threshold = if foreground_threshold.is_some() {
                    threshold_slider(
                        cpu.foreground_or_system_cpu_threshold_percent,
                        SHARED_THRESHOLD_FIELD,
                        SYSTEM_THRESHOLD_LABEL,
                        |s, n| {
                            s.adaptive_engine_process
                                .foreground_or_system_cpu_threshold_percent = n as u8
                        },
                    )
                } else {
                    number!(
                        SHARED_THRESHOLD_LABEL,
                        1,
                        100,
                        adaptive_engine_process.foreground_or_system_cpu_threshold_percent
                    )
                };
                let foreground_threshold = foreground_threshold.map(|value| {
                    threshold_slider(
                        value,
                        FOREGROUND_THRESHOLD_FIELD,
                        FOREGROUND_THRESHOLD_LABEL,
                        |s, n| {
                            s.adaptive_engine_process.foreground_cpu_threshold_percent =
                                Some(n as u8)
                        },
                    )
                });
                let pressure = super::widgets::settings_rows(
                    [
                        Some(pressure_source),
                        separate_threshold,
                        Some(shared_threshold),
                        foreground_threshold,
                    ]
                    .into_iter()
                    .flatten()
                    .chain([
                        number!(
                            "adaptive_engine_process.background_app_cpu_threshold",
                            1,
                            100,
                            adaptive_engine_process.background_app_cpu_threshold_percent
                        ),
                        number!(
                            "adaptive_engine_process.restraint_candidate_limit",
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
                            "adaptive_engine_process.minimum_restraint_time",
                            0,
                            ADAPTIVE_RESTRAINT_MAX_TIME_MS,
                            adaptive_engine_process.minimum_restraint_time_ms
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
                        ),
                        super::widgets::setting_row(
                            "adaptive_engine_process.exclude_service_hosts",
                            super::widgets::switch(
                                cpu.exclude_service_hosts,
                                editable.then_some(|v| {
                                    Message::Toggle(
                                        |s, v| s.adaptive_engine_process.exclude_service_hosts = v,
                                        v,
                                    )
                                }),
                            ),
                        )
                        .into(),
                    ]),
                );
                body = body.push(super::widgets::setting_group(
                    "adaptive_engine_process.shared_detection".to_string(),
                    self.expanded[usize::from(preset)][SharedDetection as usize],
                    Message::Collapse(SharedDetection),
                    iced::widget::Space::new(),
                    pressure,
                ));
                let restraint_action = choice!(
                    s,
                    "adaptive_engine_process.restraint_action",
                    AdaptiveRestraintPriority,
                    &[
                        AdaptiveRestraintPriority::BelowNormal,
                        AdaptiveRestraintPriority::Idle,
                        AdaptiveRestraintPriority::ProcessorPlacementOnly,
                    ],
                    restraint_action_label,
                    adaptive_engine_process.restraint_priority
                );
                let priority_effects = (!placement_only).then(|| {
                    [
                        choice!(
                            s,
                            "adaptive_engine_process.restraint_io_priority",
                            AdaptiveRestraintIoPriority,
                            &[
                                AdaptiveRestraintIoPriority::Unchanged,
                                AdaptiveRestraintIoPriority::Low,
                                AdaptiveRestraintIoPriority::VeryLow,
                            ],
                            restraint_io_priority_label,
                            adaptive_engine_process.restraint_io_priority
                        )
                        .into(),
                        super::widgets::setting_row(
                            "adaptive_engine_process.restraint_energy_efficient_scheduling",
                            super::widgets::switch(
                                cpu.restraint_energy_efficient_scheduling,
                                editable.then_some(|value| {
                                    Message::Toggle(
                                        |s, enabled| {
                                            s.adaptive_engine_process
                                                .restraint_energy_efficient_scheduling = enabled
                                        },
                                        value,
                                    )
                                }),
                            ),
                        )
                        .into(),
                    ]
                });
                let protection = |key, is_on, set: fn(&mut Settings, bool)| {
                    super::widgets::setting_row(
                        key,
                        super::widgets::switch(
                            is_on,
                            editable.then_some(move |value| Message::Toggle(set, value)),
                        ),
                    )
                    .into()
                };
                let restraint = super::widgets::settings_rows(
                    std::iter::once(restraint_action.into())
                        .chain(priority_effects.into_iter().flatten())
                        .chain([
                            protection(
                                "adaptive_engine_process.protect_focused_app",
                                cpu.protect_focused_app,
                                |s, v| s.adaptive_engine_process.protect_focused_app = v,
                            ),
                            protection(
                                "adaptive_engine_process.protect_focused_children",
                                cpu.protect_focused_children,
                                |s, v| s.adaptive_engine_process.protect_focused_children = v,
                            ),
                            protection(
                                "adaptive_engine_process.protect_non_normal_priority",
                                cpu.protect_non_normal_priority,
                                |s, v| s.adaptive_engine_process.protect_non_normal_priority = v,
                            ),
                            number!(
                                "adaptive_engine_process.restraint_trigger_time",
                                ADAPTIVE_RESTRAINT_TRIGGER_MIN_MS,
                                ADAPTIVE_RESTRAINT_TRIGGER_MAX_MS,
                                adaptive_engine_process.restraint_trigger_time_ms
                            ),
                            number!(
                                "adaptive_engine_process.maximum_restraint_time",
                                0,
                                ADAPTIVE_RESTRAINT_MAX_TIME_MS,
                                adaptive_engine_process.maximum_restraint_time_ms
                            ),
                            super::widgets::setting_row(
                                "adaptive_engine_process.pause_restraint_when_idle",
                                super::widgets::switch(
                                    s.adaptive_engine_process.pause_restraint_when_idle,
                                    editable.then_some(|v| {
                                        Message::Toggle(
                                            |s, v| {
                                                s.adaptive_engine_process
                                                    .pause_restraint_when_idle = v
                                            },
                                            v,
                                        )
                                    }),
                                ),
                            )
                            .into(),
                            number!(
                                "adaptive_engine_process.restraint_idle_timeout",
                                ADAPTIVE_RESTRAINT_IDLE_MIN_SECONDS,
                                ADAPTIVE_RESTRAINT_IDLE_MAX_SECONDS,
                                adaptive_engine_process.restraint_idle_timeout_seconds
                            ),
                        ]),
                );
                body = body.push(super::widgets::setting_group(
                    "adaptive_engine.cpu_pressure".to_string(),
                    self.expanded[usize::from(preset)][CpuPressureRestraint as usize],
                    Message::Collapse(CpuPressureRestraint),
                    super::widgets::switch(
                        s.adaptive_engine_process.cpu_pressure_restraint_enabled,
                        editable.then_some(|v| {
                            Message::Toggle(
                                |s, v| s.adaptive_engine_process.cpu_pressure_restraint_enabled = v,
                                v,
                            )
                        }),
                    ),
                    restraint,
                ));
                let mut allocation = super::widgets::settings_rows([choice!(
                    s,
                    "adaptive_engine_process.cpu_allocation_method",
                    CpuAllocationMethod,
                    &CpuAllocationMethod::ALL,
                    allocation_label,
                    adaptive_engine_process.cpu_allocation_method
                )
                .into()]);
                if method.uses_processor_selection() {
                    allocation = allocation
                        .push(super::widgets::settings_divider())
                        .push(choice!(
                            s,
                            "adaptive_engine_process.processor_selection",
                            BackgroundProcessorSelection,
                            &BackgroundProcessorSelection::ALL,
                            background_processor_selection_label,
                            adaptive_engine_process.background_processor_selection
                        ));
                    if selection.uses_percentage() {
                        allocation =
                            allocation
                                .push(super::widgets::settings_divider())
                                .push(number!(
                                    "adaptive_engine_process.processor_limit",
                                    1,
                                    100,
                                    adaptive_engine_process.processor_limit_percent
                                ));
                    }
                    if selection == BackgroundProcessorSelection::Custom {
                        if let Some(catalog) = &cpu_catalog {
                            allocation = allocation.push(super::widgets::settings_divider()).push(
                                super::cpu_allocation::mask_selector(
                                    &cpu.specific_processors,
                                    catalog,
                                    &s.cpu_allocation_presets,
                                    super::cpu_allocation::ProcessorPickerLayout::Compact,
                                    Message::Mask,
                                ),
                            );
                        } else if !editable {
                            allocation =
                                allocation
                                    .push(super::widgets::settings_divider())
                                    .push(text(format!(
                                        "{}: {}",
                                        t!("adaptive_engine_process.specific_processors"),
                                        super::cpu_allocation::selection_text(
                                            &cpu.specific_processors
                                        )
                                    )));
                        }
                    }
                } else {
                    allocation = allocation
                        .push(super::widgets::settings_divider())
                        .push(number!(
                            "adaptive_engine_process.processors_to_remove",
                            1,
                            u64::from(ADAPTIVE_AFFINITY_REMOVAL_MAX),
                            adaptive_engine_process.processors_to_remove
                        ));
                }
                if method.is_hard_affinity() {
                    allocation = allocation.push(super::widgets::settings_divider()).push(
                        super::widgets::setting_row(
                            "adaptive_engine_process.preserve_custom_affinity",
                            super::widgets::switch(
                                cpu.preserve_custom_affinity,
                                editable.then_some(|v| {
                                    Message::Toggle(
                                        |s, v| {
                                            s.adaptive_engine_process.preserve_custom_affinity = v
                                        },
                                        v,
                                    )
                                }),
                            ),
                        ),
                    );
                    allocation = allocation.push(super::widgets::settings_divider()).push(
                        super::widgets::setting_row(
                            "adaptive_engine_process.restore_child_affinity",
                            super::widgets::switch(
                                cpu.restore_child_affinity,
                                editable.then_some(|value| {
                                    Message::Toggle(
                                        |settings, enabled| {
                                            settings
                                                .adaptive_engine_process
                                                .restore_child_affinity = enabled
                                        },
                                        value,
                                    )
                                }),
                            ),
                        ),
                    );
                    allocation = allocation.push(super::widgets::settings_divider()).push(
                        super::widgets::setting_row(
                            "adaptive_engine_process.restore_affinity_before_managed_restart",
                            super::widgets::switch(
                                cpu.restore_affinity_before_managed_restart,
                                editable.then_some(|value| {
                                    Message::Toggle(
                                        |settings, enabled| {
                                            settings
                                                .adaptive_engine_process
                                                .restore_affinity_before_managed_restart = enabled
                                        },
                                        value,
                                    )
                                }),
                            ),
                        ),
                    );
                }
                let allocation_action: Element<'_, Message> = if placement_only {
                    let scope = if cpu.placement_only_restraint_enabled() {
                        "adaptive_engine_process.placement_only_scope"
                    } else {
                        "adaptive_engine_process.placement_only_scope_disabled"
                    };
                    text(t!(scope).to_string())
                        .size(design::typography::SECONDARY)
                        .style(text::secondary)
                        .into()
                } else {
                    allocation =
                        allocation
                            .push(super::widgets::settings_divider())
                            .push(super::widgets::setting_row(
                            "adaptive_engine_process.limit_background_processors_during_restraint",
                            super::widgets::switch(
                                cpu.limit_background_processors_during_restraint,
                                editable.then_some(|enabled| {
                                    Message::Toggle(
                                        |settings, enabled| {
                                            settings
                                                .adaptive_engine_process
                                                .limit_background_processors_during_restraint =
                                                enabled
                                        },
                                        enabled,
                                    )
                                }),
                            ),
                        ));
                    super::widgets::switch(
                        cpu.limit_background_processors_enabled,
                        editable.then_some(|v| {
                            Message::Toggle(
                                |s, v| {
                                    s.adaptive_engine_process
                                        .limit_background_processors_enabled = v
                                },
                                v,
                            )
                        }),
                    )
                };
                body = body.push(super::widgets::setting_group(
                    "adaptive_engine_process.limit_background_processors".to_string(),
                    self.expanded[usize::from(preset)][BackgroundProcessors as usize],
                    Message::Collapse(BackgroundProcessors),
                    allocation_action,
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
                                    super::cpu_allocation::ProcessorPickerLayout::Compact,
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
                    self.expanded[usize::from(preset)][ResourceZones as usize],
                    Message::Collapse(ResourceZones),
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
                if let Some(interrupt_timing) = interrupt_timing {
                    body = body.push(interrupt_timing.view().map(Message::InterruptTiming));
                }
            }
            TuningTab::CustomRules => {}
        }
        if !preset && tab == TuningTab::CustomRules {
            let cpu = &s.adaptive_engine_process;
            let target_input = match self.match_by {
                ExclusionMatchBy::Path => super::app_picker::view_with_path_selection(
                    &self.path,
                    candidates,
                    true,
                    (Message::Path, |path| Message::Path(picked_path_input(path))),
                    Message::Browse,
                    can_add_path_exclusion(cpu, exact_path_input(&self.path))
                        .then_some(Message::AddExclusion),
                    |path| can_add_path_exclusion(cpu, path).then_some(true),
                ),
                ExclusionMatchBy::Name => {
                    let add = can_add_name_exclusion(cpu, &self.executable_name)
                        .then_some(Message::AddExclusion);
                    column![
                        super::widgets::setting_title("common.executable_name"),
                        row![
                            text_input("app.exe", &self.executable_name)
                                .on_input(Message::ExecutableName)
                                .on_submit_maybe(add.clone())
                                .width(Fill),
                            super::widgets::rule_add_button(add),
                        ]
                        .spacing(design::space::SMALL)
                        .align_y(iced::Center),
                    ]
                    .spacing(design::space::SMALL)
                    .into()
                }
                ExclusionMatchBy::NamePattern => {
                    let valid =
                        ExecutableNamePattern::try_from(self.executable_name_pattern.clone());
                    let add = (valid.is_ok()
                        && !has_pattern_exclusion(cpu, &self.executable_name_pattern))
                    .then_some(Message::AddExclusion);
                    let help = match &valid {
                        Err(error) if !self.executable_name_pattern.is_empty() => error.clone(),
                        _ => t!("adaptive_engine_process.custom_rule_pattern_help").into_owned(),
                    };
                    column![
                        super::widgets::setting_title(self.match_by.key()),
                        row![
                            text_input("worker-*.exe", &self.executable_name_pattern)
                                .on_input(Message::ExecutableNamePattern)
                                .on_submit_maybe(add.clone())
                                .width(Fill),
                            super::widgets::rule_add_button(add),
                        ]
                        .spacing(design::space::SMALL)
                        .align_y(iced::Center),
                        text(help)
                            .size(design::typography::SECONDARY)
                            .style(text::secondary),
                    ]
                    .spacing(design::space::SMALL)
                    .into()
                }
            };
            body = body.push(super::widgets::settings_card(
                super::widgets::settings_rows([
                    text(t!("adaptive_engine_process.custom_rules_help").to_string())
                        .size(design::typography::SECONDARY)
                        .style(text::secondary)
                        .into(),
                    super::widgets::setting_row(
                        "common.match_by",
                        pick_list(ExclusionMatchBy::ALL, Some(self.match_by), Message::MatchBy)
                            .width(design::SELECT_WIDTH),
                    )
                    .into(),
                    target_input,
                ]),
            ));
            let rules = s
                .adaptive_engine_process
                .custom_rules
                .iter()
                .enumerate()
                .map(|(i, rule)| {
                    let (app, target, match_by, value) = match &rule.selector {
                        AdaptiveEngineExclusionSelector::Exact(selector) => {
                            let (app, target) =
                                super::widgets::process_rule_target(selector, candidates);
                            if selector.exact_path().is_some() {
                                let target = column![
                                    target,
                                    text(t!("common.executable_path").to_string())
                                        .size(design::typography::CAPTION)
                                        .style(text::secondary),
                                ]
                                .spacing(design::space::TINY)
                                .into();
                                (app, target, ExclusionMatchBy::Path, selector.value())
                            } else {
                                (app, target, ExclusionMatchBy::Name, selector.value())
                            }
                        }
                        AdaptiveEngineExclusionSelector::NamePattern {
                            executable_name_pattern,
                        } => {
                            let value = executable_name_pattern.as_str();
                            let match_by = ExclusionMatchBy::NamePattern;
                            let app = text(value.to_owned())
                                .wrapping(text::Wrapping::WordOrGlyph)
                                .into();
                            let target = text(t!(match_by.key()).into_owned())
                                .size(design::typography::CAPTION)
                                .style(text::secondary)
                                .into();
                            (app, target, match_by, value)
                        }
                    };
                    (
                        super::widgets::stable_key(&(match_by.key(), value)),
                        column![
                            super::widgets::process_rule_header_with_target(
                                app,
                                target,
                                checkbox(rule.enabled)
                                    .on_toggle(move |value| Message::ExclusionEnabled(i, value))
                                    .into(),
                                None,
                                vec![],
                                super::widgets::rule_actions(
                                    None,
                                    Some(Message::RemoveExclusion(i))
                                ),
                            ),
                            super::widgets::settings_divider(),
                        ]
                        .into(),
                    )
                })
                .collect();
            body = body.push(super::widgets::process_rules_table_with_target(
                rules,
                "adaptive_engine_process.rule_match_details",
            ));
        }

        if !preset {
            if let Some(error) = numbers.validation_error() {
                body = body.push(text(error));
            }
            if !self.error.is_empty() {
                body = body.push(text(self.error.clone()).style(text::danger));
            }
        }
        let body = super::motion::wrap(body, true, super::motion::Effect::Content(tab as u64));
        if scroll_live_header {
            scrollable(header.push(body))
                .id("adaptive-engine-content")
                .width(Fill)
                .height(Fill)
                .into()
        } else {
            header
                .push(
                    scrollable(body)
                        .id(if preset {
                            "adaptive-engine-preset-content"
                        } else {
                            "adaptive-engine-content"
                        })
                        .width(Fill)
                        .height(Fill),
                )
                .into()
        }
    }
    pub(super) fn presets_panel<'a>(&'a self, live: &'a SettingsEditor) -> Element<'a, Message> {
        let mut rail = preset_list().push(super::widgets::heading(
            t!("common.built_in_presets").to_string(),
            design::typography::SECONDARY,
        ));
        for p in BuiltInAdaptiveEnginePreset::ALL {
            rail = rail.push(preset_row(
                built_in_adaptive_engine_preset_label(p),
                Message::ViewBuiltIn(p),
                None,
            ));
        }
        rail = rail.push(super::widgets::heading(
            t!("common.custom_presets").to_string(),
            design::typography::SECONDARY,
        ));
        if live.adaptive_engine_presets.is_empty() {
            rail =
                rail.push(text(t!("common.no_custom_presets").to_string()).style(text::secondary));
        }
        for (i, p) in live.adaptive_engine_presets.iter().enumerate() {
            rail = rail.push(preset_row(
                p.name.as_str(),
                Message::ViewCustom(i),
                Some((Message::Edit(i), Message::Delete(i))),
            ));
        }
        preset_panel(rail, t!("common.add_preset").to_string(), Message::New)
    }
}
fn setting_label(key: &str) -> iced::widget::Column<'static, Message> {
    super::widgets::setting_title_with_unit(key, "")
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
        CpuAllocationMethod::ReduceAffinity => "adaptive_engine_process.reduce_affinity",
    })
    .to_string()
}
fn pressure_source_label(source: AdaptivePressureSource) -> String {
    t!(match source {
        AdaptivePressureSource::Either => "adaptive_engine_process.pressure_source_either",
        AdaptivePressureSource::System => "adaptive_engine_process.pressure_source_system",
        AdaptivePressureSource::Foreground => "adaptive_engine_process.pressure_source_foreground",
        AdaptivePressureSource::Both => "adaptive_engine_process.pressure_source_both",
    })
    .to_string()
}
fn restraint_action_label(priority: AdaptiveRestraintPriority) -> String {
    t!(match priority {
        AdaptiveRestraintPriority::BelowNormal =>
            "adaptive_engine_process.restraint_action_below_normal",
        AdaptiveRestraintPriority::Idle => "adaptive_engine_process.restraint_action_idle",
        AdaptiveRestraintPriority::ProcessorPlacementOnly =>
            "adaptive_engine_process.restraint_action_placement_only",
    })
    .to_string()
}
fn restraint_io_priority_label(priority: AdaptiveRestraintIoPriority) -> String {
    t!(match priority {
        AdaptiveRestraintIoPriority::Unchanged => {
            "adaptive_engine_process.restraint_io_priority_unchanged"
        }
        AdaptiveRestraintIoPriority::Low => "io_priority.priority_low",
        AdaptiveRestraintIoPriority::VeryLow => "io_priority.priority_very_low",
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
        editor.update(&mut settings, Message::Collapse(TuningGroup::ResourceZones));
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
        assert_eq!(editor.expanded[0], [false; TuningGroup::COUNT]);
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
        settings.edit_global(|settings| {
            settings.adaptive_engine_presets[0]
                .adaptive_engine_process
                .custom_rules
                .push(AdaptiveEngineExclusionRule::exact_name("excluded.exe"));
        });
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
    fn sidebar_preview_stages_shown_tuning_only_on_its_source() {
        let mut live = Settings::default();
        let mut custom = capture_adaptive_engine_preset(&live, "Custom preview".into());
        custom.adaptive_engine_process.reaction_time_ms = 1900;
        live.adaptive_engine_presets.push(custom);

        for preview in [
            Message::ViewBuiltIn(BuiltInAdaptiveEnginePreset::Speed),
            Message::ViewCustom(0),
        ] {
            let mut settings = SettingsEditor::with_settings(live.clone());
            settings.select_power_source(PowerSourceProfile::OnBattery);
            let mut editor = Editor::default();
            editor.update(&mut settings, preview.clone());
            assert!(editor.preset_open());
            assert!(!editor.has_pending_editor());
            editor.update(&mut settings, Message::Cancel);
            assert_eq!(settings.global(), &live);

            editor.update(&mut settings, preview);
            let shown = editor
                .draft
                .as_ref()
                .unwrap()
                .adaptive_engine_process
                .clone();
            settings.select_power_source(PowerSourceProfile::PluggedIn);
            editor.update(&mut settings, Message::UsePreset);
            assert_eq!(settings.global(), &live);
            assert!(editor.preset_open());

            settings.select_power_source(PowerSourceProfile::OnBattery);
            editor.update(&mut settings, Message::UsePreset);
            assert_eq!(settings.adaptive_engine_process, shown);
            assert_eq!(
                settings.global().adaptive_engine_process,
                live.adaptive_engine_process
            );
            assert_eq!(settings.persisted(), &live);
            assert!(!editor.preset_open());
        }
    }

    #[test]
    fn remove_updates_only_the_draft_and_discard_restores_the_rule() {
        let mut original = Settings::default();
        original
            .adaptive_engine_process
            .custom_rules
            .push(AdaptiveEngineExclusionRule::exact_path(r"C:\Apps\test.exe"));
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
    fn custom_rules_accept_match_types_and_reject_duplicates() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        let mut editor = Editor::default();
        for message in [
            Message::Path("C:/Apps/worker.exe".into()),
            Message::AddExclusion,
            Message::Path(r"c:\apps\worker.exe".into()),
            Message::AddExclusion,
            Message::MatchBy(ExclusionMatchBy::Name),
            Message::ExecutableName(r"C:\Apps\worker.exe".into()),
            Message::AddExclusion,
            Message::ExecutableName("*.exe".into()),
            Message::AddExclusion,
            Message::ExecutableName("lsass.exe".into()),
            Message::AddExclusion,
        ] {
            editor.update(&mut settings, message);
        }
        assert_eq!(settings.adaptive_engine_process.custom_rules.len(), 1);

        for message in [
            Message::ExecutableName("worker.exe".into()),
            Message::AddExclusion,
            Message::ExecutableName("WORKER.EXE".into()),
            Message::AddExclusion,
            Message::MatchBy(ExclusionMatchBy::NamePattern),
            Message::ExecutableNamePattern("C:/Apps/*.exe".into()),
            Message::AddExclusion,
            Message::ExecutableNamePattern("worker.exe".into()),
            Message::AddExclusion,
            Message::ExecutableNamePattern("worker-?.exe".into()),
            Message::AddExclusion,
            Message::ExecutableNamePattern("WORKER-?.EXE".into()),
            Message::AddExclusion,
        ] {
            editor.update(&mut settings, message);
        }
        let rules = &settings.adaptive_engine_process.custom_rules;
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].selector.exact_path(), Some(r"C:\Apps\worker.exe"));
        assert_eq!(
            rules[1],
            AdaptiveEngineExclusionRule::exact_name("worker.exe")
        );
        assert!(rules[2].selector.matches_name("WORKER-A.EXE"));
        assert!(!rules[2].selector.matches_name("worker-ab.exe"));
        assert!(settings
            .persisted()
            .adaptive_engine_process
            .custom_rules
            .is_empty());
    }

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
                let mut view = editor.view(&settings, &[], &interrupt_timing, 860.0);
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
                    editor.view(&settings, &[], &interrupt_timing, 860.0),
                    iced::Rectangle::with_size(iced::Size::new(width, 700.0)),
                );
            }
        }
    }

    #[test]
    fn live_tabs_have_room_to_scroll_at_minimum_height() {
        use iced::advanced::{
            layout,
            widget::{
                operation::{Focusable, Scrollable, TextInput},
                Id, Operation, Tree,
            },
            Layout,
        };
        use iced::{Rectangle, Size, Vector};

        #[derive(Default)]
        struct EditorScroll {
            bounds: Option<(Rectangle, Rectangle)>,
            focus_first_field: bool,
            next_is_first_field: bool,
            first_field_focused: Option<bool>,
        }
        impl Operation for EditorScroll {
            fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
                visit(self);
            }

            fn scrollable(
                &mut self,
                id: Option<&Id>,
                bounds: Rectangle,
                content_bounds: Rectangle,
                _: Vector,
                _: &mut dyn Scrollable,
            ) {
                if id == Some(&Id::from("adaptive-engine-content")) {
                    self.bounds = Some((bounds, content_bounds));
                }
            }

            fn text_input(&mut self, _: Option<&Id>, _: Rectangle, _: &mut dyn TextInput) {
                self.next_is_first_field = self.first_field_focused.is_none();
            }

            fn focusable(&mut self, _: Option<&Id>, _: Rectangle, state: &mut dyn Focusable) {
                if std::mem::take(&mut self.next_is_first_field) {
                    if self.focus_first_field {
                        state.focus();
                    }
                    self.first_field_focused = Some(state.is_focused());
                }
            }
        }

        let mut settings = Settings::default();
        settings.adaptive_engine_process.custom_rules = (0..8)
            .map(|index| {
                AdaptiveEngineExclusionRule::exact_path(format!(r"C:\Apps\app-{index}.exe"))
            })
            .collect();
        let settings = SettingsEditor::with_settings(settings);
        let interrupt_timing = super::super::interrupt_timing::State::default();
        let renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        for tab in TuningTab::ALL {
            let mut editor = Editor {
                tuning_tabs: [tab, TuningTab::default()],
                ..Editor::default()
            };
            let mut tree = Tree::empty();
            for (shell_height, editor_height) in [
                (583.0, 320.0), // Minimum window with Info and Save visible.
                (683.0, 527.0), // Before an edit.
                (683.0, 421.0), // The same window after Save appears.
                (749.0, 593.0), // Just below the layout breakpoint.
            ] {
                let checking_focus = matches!(editor_height, 527.0 | 421.0);
                editor.expanded[0][TuningGroup::SharedDetection as usize] = checking_focus;
                let workspace = Rectangle::with_size(Size::new(466.0, editor_height));
                let mut view = editor.view(&settings, &[], &interrupt_timing, shell_height);
                tree.diff(view.as_widget());
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, workspace.size()),
                );
                let mut scroll = EditorScroll {
                    focus_first_field: editor_height == 527.0,
                    ..EditorScroll::default()
                };
                view.as_widget_mut()
                    .operate(&mut tree, Layout::new(&node), &renderer, &mut scroll);
                let (viewport, content) = scroll.bounds.expect("live editor scroll area");
                assert_eq!(viewport, workspace, "{tab:?}");
                if editor_height == 320.0 {
                    assert!(
                        content.height > viewport.height,
                        "{tab:?} fits without scrolling"
                    );
                }
                if tab == TuningTab::CpuBehaviour && checking_focus {
                    assert_eq!(scroll.first_field_focused, Some(true));
                }
                super::super::scrolling::check_scroll_damage(view, workspace);
            }
        }
    }

    #[test]
    fn tuning_navigation_keeps_live_and_preset_state_separate() {
        let mut editor = Editor::default();
        let mut settings = SettingsEditor::with_settings(Settings::default());
        let before = settings.clone();
        editor.update(&mut settings, Message::TuningTab(TuningTab::CustomRules));
        editor.update(
            &mut settings,
            Message::Collapse(TuningGroup::CpuPressureRestraint),
        );
        assert_eq!(settings.global(), &before);
        editor.update(
            &mut settings,
            Message::ViewBuiltIn(BuiltInAdaptiveEnginePreset::Balanced),
        );
        assert_eq!(editor.tuning_tabs[1], TuningTab::CpuBehaviour);
        assert_eq!(editor.expanded[1], [false; TuningGroup::COUNT]);
        editor.update(&mut settings, Message::TuningTab(TuningTab::CustomRules));
        assert_eq!(editor.tuning_tabs[1], TuningTab::CpuBehaviour);
        editor.update(
            &mut settings,
            Message::Collapse(TuningGroup::CpuPressureRestraint),
        );
        editor.update(&mut settings, Message::Cancel);
        assert_eq!(editor.tuning_tabs[0], TuningTab::CustomRules);
        assert_eq!(editor.expanded[0], [false, false, false, true]);
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
        let exclusion = AdaptiveEngineExclusionRule::exact_name("app.exe");
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
    fn preset_restraint_options_reject_a_maximum_below_the_minimum() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        let mut editor = Editor::default();
        let maximum = |value: &str| {
            Message::Number(
                |s, n| s.adaptive_engine_process.maximum_restraint_time_ms = n,
                value.into(),
                0,
                ADAPTIVE_RESTRAINT_MAX_TIME_MS,
                MAXIMUM_RESTRAINT_TIME_FIELD,
                "adaptive_engine_process.maximum_restraint_time",
            )
        };
        for message in [
            Message::New,
            Message::Name("Restraint".into()),
            Message::Choice(
                |s, _| s.adaptive_engine_process.pressure_source = AdaptivePressureSource::Both,
                0,
            ),
            Message::Choice(
                |s, _| {
                    s.adaptive_engine_process.restraint_priority = AdaptiveRestraintPriority::Idle
                },
                0,
            ),
            Message::Number(
                |s, n| s.adaptive_engine_process.minimum_restraint_time_ms = n,
                "2100".into(),
                0,
                ADAPTIVE_RESTRAINT_MAX_TIME_MS,
                MINIMUM_RESTRAINT_TIME_FIELD,
                "adaptive_engine_process.minimum_restraint_time",
            ),
            maximum("2099"),
            Message::Save,
        ] {
            editor.update(&mut settings, message);
        }
        assert!(settings.global().adaptive_engine_presets.is_empty());
        assert_eq!(
            editor.error,
            t!("adaptive_engine_process.maximum_restraint_time_invalid").to_string()
        );

        editor.update(&mut settings, maximum("2100"));
        assert!(editor.error.is_empty());
        editor.update(&mut settings, Message::Save);
        assert!(editor.draft.is_none());
        assert!(editor.error.is_empty());
        assert_eq!(
            settings.adaptive_engine_process.maximum_restraint_time_ms,
            0
        );

        editor.update(&mut settings, Message::Apply(0));
        let cpu = &settings.adaptive_engine_process;
        assert_eq!(cpu.pressure_source, AdaptivePressureSource::Both);
        assert_eq!(cpu.restraint_priority, AdaptiveRestraintPriority::Idle);
        assert_eq!(cpu.minimum_restraint_time_ms, 2100);
        assert_eq!(cpu.maximum_restraint_time_ms, 2100);
    }

    #[test]
    fn switching_placement_methods_discards_invalid_hidden_inputs() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        let mut editor = Editor::default();
        let method = |settings: &mut Settings, index| {
            settings.adaptive_engine_process.cpu_allocation_method =
                CpuAllocationMethod::ALL[index];
        };
        for (current, next, field, max) in [
            (
                CpuAllocationMethod::CpuSetsSoft,
                CpuAllocationMethod::ReduceAffinity,
                "adaptive_engine_process.processor_limit_percent",
                100,
            ),
            (
                CpuAllocationMethod::ReduceAffinity,
                CpuAllocationMethod::ProcessorAffinityHard,
                "adaptive_engine_process.processors_to_remove",
                u64::from(ADAPTIVE_AFFINITY_REMOVAL_MAX),
            ),
        ] {
            settings.adaptive_engine_process.cpu_allocation_method = current;
            editor.update(
                &mut settings,
                Message::Number(|_, _| {}, String::new(), 1, max, field, field),
            );
            assert!(editor.validation_error().is_some());
            let index = CpuAllocationMethod::ALL
                .iter()
                .position(|option| *option == next)
                .unwrap();
            editor.update(&mut settings, Message::Choice(method, index));
            assert!(editor.validation_error().is_none());
        }
    }

    #[test]
    fn restoring_defaults_stages_only_the_selected_profile_and_can_be_discarded() {
        let mut original = Settings::default();
        original.adaptive_engine_process.restraint_trigger_time_ms = 1_750;
        let battery = &mut original.battery_profile_mut().adaptive_engine_process;
        battery.minimum_restraint_time_ms = 2_100;
        battery.maximum_restraint_time_ms = 6_000;
        battery
            .custom_rules
            .push(AdaptiveEngineExclusionRule::exact_path(
                r"C:\Apps\Example.exe",
            ));

        let mut settings = SettingsEditor::with_settings(original.clone());
        settings.select_power_source(PowerSourceProfile::OnBattery);
        let mut editor = Editor::default();
        editor.update(&mut settings, Message::RestoreDefaults);

        assert!(settings.has_unsaved_changes());
        assert_eq!(settings.persisted(), &original);
        assert_eq!(
            settings.global().adaptive_engine_process,
            original.adaptive_engine_process
        );
        assert_eq!(
            settings.adaptive_engine_process.minimum_restraint_time_ms,
            3_000
        );
        assert_eq!(
            settings.adaptive_engine_process.maximum_restraint_time_ms,
            0
        );
        assert_eq!(
            settings.adaptive_engine_process.custom_rules,
            original
                .battery_profile()
                .adaptive_engine_process
                .custom_rules,
        );

        settings.cancel();
        assert_eq!(settings.global(), &original);
        assert!(!settings.has_unsaved_changes());
    }

    #[test]
    fn restoring_already_default_battery_settings_clears_drafts_without_creating_a_profile() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        settings.select_power_source(PowerSourceProfile::OnBattery);
        let mut editor = Editor {
            error: "stale error".into(),
            selected_preset: Some(Choice(0, "Old selection".into())),
            ..Editor::default()
        };
        editor.update(
            &mut settings,
            Message::Number(
                |_, _| {},
                String::new(),
                0,
                ADAPTIVE_RESTRAINT_MAX_TIME_MS,
                MINIMUM_RESTRAINT_TIME_FIELD,
                "adaptive_engine_process.minimum_restraint_time",
            ),
        );
        assert!(editor.validation_error().is_some());

        editor.update(&mut settings, Message::RestoreDefaults);
        assert!(editor.validation_error().is_none());
        assert!(editor.error.is_empty());
        assert!(editor.selected_preset.is_none());
        assert!(settings.global().on_battery.is_none());
        assert!(!settings.has_unsaved_changes());
    }

    #[test]
    fn restoring_plugged_in_defaults_preserves_inherited_battery_settings() {
        let mut original = Settings::default();
        original.adaptive_engine_process.minimum_restraint_time_ms = 2_100;
        let mut settings = SettingsEditor::with_settings(original.clone());
        let mut editor = Editor::default();
        editor.update(&mut settings, Message::RestoreDefaults);

        assert!(settings.has_unsaved_changes());
        assert_eq!(
            settings.adaptive_engine_process.minimum_restraint_time_ms,
            3_000
        );
        assert_eq!(
            settings.global().battery_profile().adaptive_engine_process,
            original.adaptive_engine_process
        );
        assert_eq!(settings.persisted(), &original);

        settings.cancel();
        assert_eq!(settings.global(), &original);
        assert!(settings.global().on_battery.is_none());
    }

    #[test]
    fn switching_pressure_source_preserves_thresholds_and_discards_hidden_invalid_input() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        let mut editor = Editor::default();
        settings.adaptive_engine_process.pressure_source = AdaptivePressureSource::Either;
        settings
            .adaptive_engine_process
            .foreground_or_system_cpu_threshold_percent = 74;
        let foreground = |text: &str| {
            Message::Number(
                |s, value| {
                    s.adaptive_engine_process.foreground_cpu_threshold_percent = Some(value as u8)
                },
                text.into(),
                1,
                100,
                FOREGROUND_THRESHOLD_FIELD,
                FOREGROUND_THRESHOLD_LABEL,
            )
        };
        editor.update(&mut settings, Message::SeparateForegroundThreshold(true));
        assert_eq!(
            settings
                .adaptive_engine_process
                .active_foreground_cpu_threshold(),
            Some(74)
        );
        editor.update(&mut settings, foreground("42"));
        editor.update(&mut settings, foreground("0"));
        assert!(editor.validation_error().is_some());

        editor.update(
            &mut settings,
            Message::Choice(
                |s, _| {
                    s.adaptive_engine_process.pressure_source = AdaptivePressureSource::Foreground
                },
                0,
            ),
        );
        assert!(editor.validation_error().is_none());
        assert_eq!(
            settings
                .adaptive_engine_process
                .active_foreground_cpu_threshold(),
            None
        );
        assert_eq!(
            settings
                .adaptive_engine_process
                .foreground_cpu_threshold_percent,
            Some(42)
        );

        editor.update(
            &mut settings,
            Message::Choice(
                |s, _| s.adaptive_engine_process.pressure_source = AdaptivePressureSource::Both,
                0,
            ),
        );
        assert_eq!(
            settings
                .adaptive_engine_process
                .active_foreground_cpu_threshold(),
            Some(42)
        );
        editor.update(&mut settings, foreground("101"));
        assert!(editor.validation_error().is_some());
        editor.update(&mut settings, Message::SeparateForegroundThreshold(false));
        assert!(editor.validation_error().is_none());
        assert_eq!(
            settings
                .adaptive_engine_process
                .foreground_cpu_threshold_percent,
            None
        );
        assert_eq!(
            settings
                .adaptive_engine_process
                .foreground_or_system_cpu_threshold_percent,
            74
        );
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
    fn process_preset_keeps_unfinished_processor_power_edit() {
        use super::super::adaptive_processor_power::{self, PROCESSOR_MAX_FIELD};

        let mut settings = SettingsEditor::with_settings(Settings::default());
        let mut editor = Editor::default();
        let mut processor_power = adaptive_processor_power::Editor::default();
        processor_power.update(
            &mut settings,
            adaptive_processor_power::Message::Number(
                |settings, value| settings.base_processor_policy.performance_max = value as u32,
                String::new(),
                0,
                100,
                PROCESSOR_MAX_FIELD,
                "processor_power.processor_max",
            ),
        );
        let power_before = settings.adaptive_engine.clone();
        let validation_error = processor_power.validation_error();
        assert!(validation_error.is_some());

        editor.update(
            &mut settings,
            Message::BuiltIn(BuiltInAdaptiveEnginePreset::Speed),
        );

        assert_eq!(settings.adaptive_engine, power_before);
        assert_eq!(processor_power.validation_error(), validation_error);
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
