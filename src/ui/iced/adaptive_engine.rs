use super::priority_control::{
    process_dynamic_priority_boost_setting_label, process_gpu_priority_setting_label,
    process_io_priority_setting_label, process_memory_priority_setting_label,
    process_priority_setting_label, process_thread_priority_setting_label,
};
use crate::automation::RuntimeStatusSnapshot;
use crate::config::*;
use crate::power::ProcessorBoostMode;
use iced::widget::{button, checkbox, column, pick_list, row, scrollable, text, text_input};
use iced::{Element, Fill};
use rust_i18n::t;
#[path = "adaptive_presets.rs"]
mod presets;
use presets::*;
#[derive(Default)]
pub(super) struct Editor {
    name: String,
    draft: Option<Settings>,
    editing: Option<usize>,
    read_only: bool,
    presets_tab: bool,
    path: String,
    error: String,
    removing: Option<usize>,
    deleting: Option<ProcessExclusionRule>,
    numbers: std::collections::HashMap<&'static str, String>,
    invalid_numbers: std::collections::HashMap<&'static str, (u64, u64, &'static str)>,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Toggle(fn(&mut Settings, bool), bool),
    RailTab(bool),
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
    ConfirmRemoveExclusion,
    CancelRemoveExclusion,
    RemovedExclusion(String),
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
            Message::RemoveExclusion(i) => self.removing = Some(i),
            Message::ConfirmRemoveExclusion => {
                if let Some(i) = self.removing.take() {
                    if i < s.cpu_scheduler.custom_rules.len() {
                        self.deleting = Some(s.cpu_scheduler.custom_rules.remove(i));
                    }
                }
            }
            Message::RemovedExclusion(path) => {
                if self
                    .deleting
                    .as_ref()
                    .is_some_and(|r| r.executable_path == path)
                {
                    self.deleting = None;
                }
            }
            Message::CancelRemoveExclusion => self.removing = None,
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
        status: &'a RuntimeStatusSnapshot,
        candidates: &'a [String],
        motion: bool,
    ) -> Element<'a, Message> {
        let s = self.draft.as_ref().unwrap_or(live);
        let editable = self.draft.is_none() || !self.read_only;
        let mut body = column![text(t!("adaptive_engine.intro_1").to_string())
            .width(Fill)
            .style(text::secondary)]
        .spacing(12);
        macro_rules! toggle {($key:expr,$($field:ident).+) => {body=body.push(checkbox(s.$($field).+).label(t!($key).to_string()).on_toggle_maybe(editable.then_some(|v|Message::Toggle(|s,v|s.$($field).+ = v,v))));};}
        macro_rules! number {($key:expr,$min:expr,$max:expr,$($field:ident).+) => {{let key=stringify!($($field).+);let value=self.numbers.get(key).cloned().unwrap_or_else(||s.$($field).+.to_string());body=body.push(row![text(t!($key).to_string()).width(Fill),text_input("",&value).on_input_maybe(editable.then_some(move|v|Message::Number(|s,n|s.$($field).+ = n as _,v,$min,$max,key,$key))).width(100)].spacing(8));}};}

        macro_rules! choice {($s:ident,$key:expr,$ty:ty,$options:expr,$label:expr,$($field:ident).+) => {{let values:&[$ty]=$options;let selected=s.$($field).+;let control:Element<'_,Message>=if editable{pick_list(values.iter().copied().map(|v|Choice(v,$label(v))).collect::<Vec<_>>(),Some(Choice(selected,$label(selected))),move |v|Message::Choice(|$s,i|{let options:&[$ty]=$options;if let Some(v)=options.get(i){$s.$($field).+ = *v;}},values.iter().position(|x|*x==v.0).unwrap_or(0))).into()}else{text($label(selected)).into()};body=body.push(row![text(t!($key).to_string()).width(Fill),control].spacing(8));}};}

        if self.draft.is_none() {
            toggle!("adaptive_engine.enable", adaptive_engine.enabled);
        }
        toggle!(
            "adaptive_engine.processor_power_policy",
            adaptive_engine.processor_power_policy_enabled
        );
        number!(
            "processor_power.core_parking_min",
            0,
            100,
            adaptive_engine.base_processor_policy.core_parking_min
        );
        number!(
            "processor_power.processor_min",
            0,
            100,
            adaptive_engine.base_processor_policy.performance_min
        );
        number!(
            "processor_power.processor_max",
            0,
            100,
            adaptive_engine.base_processor_policy.performance_max
        );
        number!(
            "processor_power.boost_policy",
            0,
            100,
            adaptive_engine.base_processor_policy.boost_policy
        );
        choice!(
            s,
            "processor_power.boost_mode",
            ProcessorBoostMode,
            &ProcessorBoostMode::ALL,
            boost_label,
            adaptive_engine.base_processor_policy.boost_mode
        );
        body = body.push(text(
            t!("adaptive_engine.background_pressure_profile").to_string(),
        ));
        number!(
            "adaptive_engine.ac_boost_policy",
            0,
            100,
            adaptive_engine.background_pressure_profile.ac_policy
        );
        choice!(
            s,
            "adaptive_engine.ac_boost_mode",
            ProcessorBoostMode,
            &ProcessorBoostMode::ALL,
            boost_label,
            adaptive_engine.background_pressure_profile.ac_mode
        );
        number!(
            "adaptive_engine.battery_boost_policy",
            0,
            100,
            adaptive_engine.background_pressure_profile.battery_policy
        );
        choice!(
            s,
            "adaptive_engine.battery_boost_mode",
            ProcessorBoostMode,
            &ProcessorBoostMode::ALL,
            boost_label,
            adaptive_engine.background_pressure_profile.battery_mode
        );
        body = body.push(text(
            t!("adaptive_engine.focus_and_launch_profile").to_string(),
        ));
        number!(
            "adaptive_engine.ac_boost_policy",
            0,
            100,
            adaptive_engine.focus_and_launch_profile.ac_policy
        );
        choice!(
            s,
            "adaptive_engine.ac_boost_mode",
            ProcessorBoostMode,
            &ProcessorBoostMode::ALL,
            boost_label,
            adaptive_engine.focus_and_launch_profile.ac_mode
        );
        number!(
            "adaptive_engine.battery_boost_policy",
            0,
            100,
            adaptive_engine.focus_and_launch_profile.battery_policy
        );
        choice!(
            s,
            "adaptive_engine.battery_boost_mode",
            ProcessorBoostMode,
            &ProcessorBoostMode::ALL,
            boost_label,
            adaptive_engine.focus_and_launch_profile.battery_mode
        );
        toggle!(
            "adaptive_engine.cpu_pressure",
            cpu_scheduler.cpu_pressure_restraint_enabled
        );
        toggle!(
            "cpu_scheduler.limit_background_processors",
            cpu_scheduler.limit_background_processors_enabled
        );
        toggle!(
            "cpu_scheduler.dynamic_resource_zones",
            cpu_scheduler.dynamic_resource_zones_enabled
        );
        toggle!(
            "nav.process_priority",
            cpu_scheduler.process_priority_enabled
        );
        toggle!(
            "nav.background_efficiency",
            cpu_scheduler.background_efficiency_enabled
        );
        toggle!(
            "background_efficiency.foreground_detection",
            cpu_scheduler.focus_process_background_efficiency_override_enabled
        );
        toggle!(
            "common.visible_window_detection",
            cpu_scheduler.visible_window_background_efficiency_override_enabled
        );
        toggle!(
            "cpu_allocation.focus",
            cpu_scheduler.focus_process_background_efficiency_mode
        );
        toggle!(
            "common.visible_window",
            cpu_scheduler.visible_window_background_efficiency_mode
        );
        toggle!(
            "common.background_process",
            cpu_scheduler.background_efficiency_mode
        );
        toggle!("nav.memory_priority", cpu_scheduler.memory_priority_enabled);
        choice!(
            s,
            "cpu_scheduler.cpu_allocation_method",
            CpuAllocationMethod,
            &CpuAllocationMethod::ALL,
            allocation_label,
            cpu_scheduler.cpu_allocation_method
        );
        choice!(
            s,
            "cpu_scheduler.processor_selection",
            BackgroundProcessorSelection,
            &BackgroundProcessorSelection::ALL,
            background_processor_selection_label,
            cpu_scheduler.background_processor_selection
        );
        number!(
            "cpu_scheduler.processor_limit",
            1,
            100,
            cpu_scheduler.processor_limit_percent
        );
        number!(
            "cpu_scheduler.foreground_or_system_cpu_threshold",
            1,
            100,
            cpu_scheduler.foreground_or_system_cpu_threshold_percent
        );
        number!(
            "cpu_scheduler.background_app_cpu_threshold",
            1,
            100,
            cpu_scheduler.background_app_cpu_threshold_percent
        );
        number!(
            "cpu_scheduler.cpu_recovery_threshold",
            1,
            100,
            cpu_scheduler.cpu_recovery_threshold_percent
        );
        number!(
            "cpu_scheduler.reaction_time",
            250,
            5000,
            cpu_scheduler.reaction_time_ms
        );
        number!(
            "cpu_scheduler.cpu_restraint_time",
            1,
            3600,
            cpu_scheduler.cpu_restraint_time_seconds
        );
        number!(
            "cpu_scheduler.cpu_recovery_time",
            1,
            3600,
            cpu_scheduler.cpu_recovery_time_seconds
        );
        number!(
            "cpu_scheduler.maximum_restrained_apps",
            1,
            64,
            cpu_scheduler.maximum_restrained_apps
        );
        if s.cpu_scheduler.background_processor_selection == BackgroundProcessorSelection::Custom {
            let mask = s
                .cpu_scheduler
                .specific_processors
                .iter()
                .filter(|i| **i < 64)
                .fold(0u64, |m, i| m | (1u64 << i));
            body = body.push(super::cpu_allocation::mask_selector(
                mask,
                &crate::cpu_allocation::logical_processors(),
                &s.cpu_allocation_presets,
                Message::Mask,
            ));
        }
        choice!(
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
        );
        choice!(
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
        );
        choice!(
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
        );
        choice!(
            s,
            "cpu_allocation.focus",
            ProcessMemoryPrioritySetting,
            &ProcessMemoryPrioritySetting::ALL,
            process_memory_priority_setting_label,
            cpu_scheduler.focus_process_memory_priority
        );
        choice!(
            s,
            "common.visible_window",
            ProcessMemoryPrioritySetting,
            &ProcessMemoryPrioritySetting::ALL,
            process_memory_priority_setting_label,
            cpu_scheduler.visible_window_memory_priority
        );
        choice!(
            s,
            "common.background_process",
            ProcessMemoryPrioritySetting,
            &ProcessMemoryPrioritySetting::ALL,
            process_memory_priority_setting_label,
            cpu_scheduler.background_memory_priority
        );
        body = body.push(text(t!("nav.io_priority").to_string()));
        toggle!("io_priority.enable", cpu_scheduler.io_priority.enabled);
        toggle!(
            "io_priority.foreground_detection",
            cpu_scheduler.io_priority.foreground_detection_enabled
        );
        toggle!(
            "common.visible_window_detection",
            cpu_scheduler.io_priority.visible_window_detection_enabled
        );
        choice!(
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
        );
        toggle!(
            "common.preserve_foreground_priority",
            cpu_scheduler.io_priority.preserve_foreground_priority
        );
        choice!(
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
        );
        toggle!(
            "common.preserve_visible_window_priority",
            cpu_scheduler.io_priority.preserve_visible_window_priority
        );
        choice!(
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
        );
        toggle!(
            "common.preserve_background_priority",
            cpu_scheduler.io_priority.preserve_background_priority
        );
        body = body.push(text(t!("nav.thread_priority").to_string()));
        toggle!(
            "thread_priority.enable",
            cpu_scheduler.thread_priority.enabled
        );
        toggle!(
            "thread_priority.foreground_detection",
            cpu_scheduler.thread_priority.foreground_detection_enabled
        );
        toggle!(
            "common.visible_window_detection",
            cpu_scheduler
                .thread_priority
                .visible_window_detection_enabled
        );
        choice!(
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
        );
        toggle!(
            "common.preserve_foreground_priority",
            cpu_scheduler.thread_priority.preserve_foreground_priority
        );
        choice!(
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
        );
        toggle!(
            "common.preserve_visible_window_priority",
            cpu_scheduler
                .thread_priority
                .preserve_visible_window_priority
        );
        choice!(
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
        );
        toggle!(
            "common.preserve_background_priority",
            cpu_scheduler.thread_priority.preserve_background_priority
        );
        body = body.push(text(t!("nav.dynamic_priority_boost").to_string()));
        toggle!(
            "dynamic_priority_boost.enable",
            cpu_scheduler.dynamic_priority_boost.enabled
        );
        toggle!(
            "dynamic_priority_boost.foreground_detection",
            cpu_scheduler
                .dynamic_priority_boost
                .foreground_detection_enabled
        );
        toggle!(
            "common.visible_window_detection",
            cpu_scheduler
                .dynamic_priority_boost
                .visible_window_detection_enabled
        );
        choice!(
            s,
            "cpu_allocation.focus",
            ProcessDynamicPriorityBoostSetting,
            &ProcessDynamicPriorityBoostSetting::ALL,
            process_dynamic_priority_boost_setting_label,
            cpu_scheduler.dynamic_priority_boost.foreground_boost
        );
        choice!(
            s,
            "common.visible_window",
            ProcessDynamicPriorityBoostSetting,
            &ProcessDynamicPriorityBoostSetting::ALL,
            process_dynamic_priority_boost_setting_label,
            cpu_scheduler.dynamic_priority_boost.visible_window_boost
        );
        choice!(
            s,
            "common.background_process",
            ProcessDynamicPriorityBoostSetting,
            &ProcessDynamicPriorityBoostSetting::ALL,
            process_dynamic_priority_boost_setting_label,
            cpu_scheduler.dynamic_priority_boost.background_boost
        );
        body = body.push(text(t!("nav.gpu_priority").to_string()));
        toggle!("gpu_priority.enable", cpu_scheduler.gpu_priority.enabled);
        toggle!(
            "gpu_priority.foreground_detection",
            cpu_scheduler.gpu_priority.foreground_detection_enabled
        );
        toggle!(
            "common.visible_window_detection",
            cpu_scheduler.gpu_priority.visible_window_detection_enabled
        );
        choice!(
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
        );
        toggle!(
            "common.preserve_foreground_priority",
            cpu_scheduler.gpu_priority.preserve_foreground_priority
        );
        choice!(
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
        );
        toggle!(
            "common.preserve_visible_window_priority",
            cpu_scheduler.gpu_priority.preserve_visible_window_priority
        );
        choice!(
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
        );
        toggle!(
            "common.preserve_background_priority",
            cpu_scheduler.gpu_priority.preserve_background_priority
        );

        if self.draft.is_none() {
            body = body
                .push(text(t!("cpu_scheduler.custom_rules").to_string()))
                .push(
                    row![
                        text_input("C:\\App\\app.exe", &self.path).on_input(Message::Path),
                        button(text(t!("common.browse_executable").to_string()))
                            .on_press(Message::Browse),
                        button(text(t!("common.add").to_string())).on_press(Message::AddExclusion)
                    ]
                    .spacing(8),
                );
            for candidate in candidates
                .iter()
                .filter(|p| p.to_lowercase().contains(&self.path.to_lowercase()))
                .take(8)
            {
                body =
                    body.push(button(text(candidate)).on_press(Message::Path(candidate.clone())));
            }
            let mut rules = Vec::new();
            for (i, r) in s
                .cpu_scheduler
                .custom_rules
                .iter()
                .chain(self.deleting.iter())
                .enumerate()
            {
                rules.push((
                    super::motion::key(&r.executable_path),
                    super::motion::removal(
                        row![
                            checkbox(r.enabled)
                                .label(r.executable_path.clone())
                                .on_toggle(move |v| Message::ExclusionEnabled(i, v)),
                            button(text(t!("common.remove").to_string()))
                                .on_press(Message::RemoveExclusion(i))
                        ]
                        .spacing(8),
                        self.deleting
                            .as_ref()
                            .is_some_and(|old| old.executable_path == r.executable_path),
                        motion,
                        Message::RemovedExclusion(r.executable_path.clone()),
                    ),
                ));
            }
            body = body.push(iced::widget::keyed_column(rules).spacing(8));
        }
        if self.removing.is_some() {
            body = body.push(
                row![
                    text(t!("common.remove").to_string()),
                    button(text(t!("common.remove").to_string()))
                        .on_press(Message::ConfirmRemoveExclusion),
                    button(text(t!("common.cancel").to_string()))
                        .on_press(Message::CancelRemoveExclusion)
                ]
                .spacing(8),
            );
        }
        if let Some(error) = self.validation_error() {
            body = body.push(text(error));
        }
        let mut rail = column![text(t!("adaptive_engine.presets").to_string())].spacing(8);
        for p in BuiltInAdaptiveEnginePreset::ALL {
            rail = rail.push(
                row![
                    button(text(built_in_adaptive_engine_preset_label(p)))
                        .on_press(Message::BuiltIn(p)),
                    button(text(t!("adaptive_engine.view_preset").to_string()))
                        .on_press(Message::ViewBuiltIn(p))
                ]
                .spacing(4),
            );
        }
        for (i, p) in live.adaptive_engine_presets.iter().enumerate() {
            rail = rail.push(
                row![
                    button(text(p.name.clone())).on_press(Message::Apply(i)),
                    button(text(t!("adaptive_engine.edit_preset").to_string()))
                        .on_press(Message::Edit(i))
                ]
                .spacing(4),
            );
        }
        rail = rail.push(
            button(text(t!("adaptive_engine.add_preset").to_string())).on_press(Message::New),
        );
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
        let snapshot = &status.feature_status.cpu_scheduler;
        rail = rail.push(text(snapshot.message.clone())).push(text(format!(
            "{}: {}",
            t!("common.adjusted_processes"),
            snapshot.adjusted_processes
        )));
        if let Some(e) = &snapshot.last_error {
            rail = rail.push(text(e.clone()));
        }
        for app in &snapshot.adjusted_apps {
            rail = rail.push(text(app.clone()));
        }
        rail = rail.push(text(self.error.clone()));
        if !self.presets_tab {
            rail = column![];
            if let Some(status) =
                super::status_rail::view(crate::ui::Page::AdaptiveEngine, live, status, &[])
            {
                rail = rail.push(status.map(Message::Status));
            }
        }
        let rail = column![
            row![
                button(text(t!("common.status").to_string()))
                    .on_press(Message::RailTab(false))
                    .style(if self.presets_tab {
                        iced::widget::button::secondary
                    } else {
                        iced::widget::button::primary
                    }),
                button(text(t!("adaptive_engine.presets").to_string()))
                    .on_press(Message::RailTab(true))
                    .style(if self.presets_tab {
                        iced::widget::button::primary
                    } else {
                        iced::widget::button::secondary
                    })
            ]
            .spacing(8),
            rail
        ]
        .spacing(12);
        row![
            scrollable(body).spacing(10).width(Fill),
            scrollable(rail).spacing(10).width(216)
        ]
        .spacing(16)
        .into()
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
    use super::*;
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
        editor.update(&mut settings, Message::ConfirmRemoveExclusion);
        assert!(settings.cpu_scheduler.custom_rules.is_empty());
        assert!(editor.deleting.is_some());
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
