use super::widgets::{self, Ui};
use crate::config::{CpuAllocationPreset, CpuAllocationRule, CpuAllocationSettings, Settings};
use crate::cpu_allocation::{self, LogicalProcessorInfo, LogicalProcessorKind};
use crate::foreground::executable_path_key;
use crate::ui::process_rules::can_add_process_candidate;
use rust_i18n::t;
use std::path::Path;
use windui::prelude::{signal, Element};

#[derive(Debug, Clone, Copy)]
pub(super) enum Kind {
    Soft,
    Hard,
}
fn settings(s: &Settings, k: Kind) -> &CpuAllocationSettings {
    match k {
        Kind::Soft => &s.cpu_sets_soft,
        Kind::Hard => &s.processor_affinity_hard,
    }
}
fn settings_mut(s: &mut Settings, k: Kind) -> &mut CpuAllocationSettings {
    match k {
        Kind::Soft => &mut s.cpu_sets_soft,
        Kind::Hard => &mut s.processor_affinity_hard,
    }
}
#[derive(Debug, Clone, Copy)]
pub(super) enum Tier {
    Focus,
    Visible,
    Background,
}
impl Tier {
    fn label(self) -> String {
        t!(match self {
            Self::Focus => "cpu_allocation.focus",
            Self::Visible => "common.visible_window",
            Self::Background => "common.background_process",
        })
        .to_string()
    }
    fn mask(self, r: &CpuAllocationRule) -> u64 {
        match self {
            Self::Focus => r.focus_core_mask,
            Self::Visible => r.visible_window_core_mask,
            Self::Background => r.background_core_mask,
        }
    }
    fn set(self, r: &mut CpuAllocationRule, m: u64) {
        match self {
            Self::Focus => r.focus_core_mask = m,
            Self::Visible => r.visible_window_core_mask = m,
            Self::Background => r.background_core_mask = m,
        }
    }
}
#[derive(Default)]
pub(super) struct Editor {
    path: String,
    selected_presets: std::collections::BTreeMap<(String, usize), super::widgets::Choice<u64>>,
    preset_name: String,
    preset_mask: u64,
    editing_preset: Option<usize>,
    preset_open: bool,
    preset_read_only: bool,
    presets_tab: bool,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    RailTab(bool),
    Status(super::status_rail::Message),
    Path(String),
    Browse,
    Add,
    RuleEnabled(usize, bool),
    Mask(usize, Tier, super::widgets::Choice<u64>),
    Remove(usize),

    NewPreset,
    ViewPreset(String, u64),
    EditPreset(usize),
    PresetName(String),
    PresetMask(u64),
    SavePreset,
    DeletePreset(usize),
    ClosePreset,
}
impl Editor {
    pub(super) fn shows_live_status(&self) -> bool {
        !self.presets_tab
    }

    pub(super) fn update(&mut self, s: &mut Settings, k: Kind, m: Message) {
        let available =
            cpu_allocation::logical_processor_mask(&cpu_allocation::logical_processors());
        match m {
            Message::RailTab(value) => self.presets_tab = value,
            Message::Status(_) => {}
            Message::Enabled(v) => settings_mut(s, k).enabled = v,
            Message::Path(v) => self.path = v,
            Message::Browse => {}
            Message::Add => {
                if can_add(s, &self.path) {
                    settings_mut(s, k).rules.push(CpuAllocationRule {
                        enabled: true,
                        executable_path: executable_path_key(Path::new(&self.path)),
                        focus_core_mask: available,
                        visible_window_core_mask: available,
                        background_core_mask: available,
                    });
                    self.path.clear();
                }
            }
            Message::RuleEnabled(i, v) => {
                if let Some(r) = settings_mut(s, k).rules.get_mut(i) {
                    r.enabled = v
                }
            }
            Message::Mask(i, tier, choice) => {
                if let Some(r) = settings_mut(s, k).rules.get_mut(i) {
                    tier.set(r, choice.0 & available);
                    self.selected_presets
                        .insert((r.executable_path.clone(), tier as usize), choice);
                }
            }
            Message::Remove(i) => {
                let rules = &mut settings_mut(s, k).rules;
                if i < rules.len() {
                    let removed = rules.remove(i);
                    self.selected_presets
                        .retain(|(path, _), _| *path != removed.executable_path);
                }
            }

            Message::ViewPreset(name, mask) => {
                self.preset_open = true;
                self.preset_read_only = true;
                self.preset_name = name;
                self.preset_mask = mask & available;
            }
            Message::NewPreset => {
                self.preset_read_only = false;
                self.preset_open = true;
                self.presets_tab = true;
                self.editing_preset = None;
                self.preset_name.clear();
                self.preset_mask = available;
            }
            Message::EditPreset(i) => {
                self.preset_read_only = false;
                if let Some(p) = s.cpu_allocation_presets.get(i) {
                    self.preset_open = true;
                    self.presets_tab = true;
                    self.editing_preset = Some(i);
                    self.preset_name = p.name.clone();
                    self.preset_mask = p.core_mask & available;
                }
            }
            Message::PresetName(v) if !self.preset_read_only => self.preset_name = v,
            Message::PresetMask(v) if !self.preset_read_only => self.preset_mask = v & available,
            Message::PresetName(_) | Message::PresetMask(_) => {}
            Message::SavePreset => {
                let name = self.preset_name.trim();
                if !self.preset_read_only
                    && !name.is_empty()
                    && self.preset_mask != 0
                    && !s.cpu_allocation_presets.iter().enumerate().any(|(i, p)| {
                        Some(i) != self.editing_preset && p.name.trim().eq_ignore_ascii_case(name)
                    })
                {
                    let p = CpuAllocationPreset {
                        name: name.into(),
                        core_mask: self.preset_mask,
                    };
                    if let Some(i) = self.editing_preset {
                        if let Some(old) = s.cpu_allocation_presets.get_mut(i) {
                            *old = p;
                        }
                    } else {
                        s.cpu_allocation_presets.push(p);
                    }
                    self.preset_open = false;
                }
            }
            Message::DeletePreset(i) => {
                if i < s.cpu_allocation_presets.len() {
                    s.cpu_allocation_presets.remove(i);
                    self.preset_open = false;
                }
            }
            Message::ClosePreset => self.preset_open = false,
        }
    }
    fn selected_preset(
        &self,
        path: &str,
        tier: Tier,
        mask: u64,
        choices: &[super::widgets::Choice<u64>],
    ) -> Option<super::widgets::Choice<u64>> {
        self.selected_presets
            .get(&(path.to_owned(), tier as usize))
            .filter(|choice| choice.0 == mask && choices.contains(choice))
            .or_else(|| choices.iter().find(|choice| choice.0 == mask))
            .cloned()
    }

    pub(super) fn reset_drafts(&mut self) {
        *self = Self {
            selected_presets: std::mem::take(&mut self.selected_presets),
            ..Self::default()
        };
    }

    pub(super) fn has_pending_editor(&self) -> bool {
        self.preset_open && !self.preset_read_only
    }
    pub(super) fn view(
        &self,
        settings: &Settings,
        kind: Kind,
        candidates: &[super::app_picker::Candidate],
        ui: Ui<Message>,
    ) -> Element {
        let processors = cpu_allocation::logical_processors();
        let feature = self::settings(settings, kind);
        let candidate_soft = settings.cpu_sets_soft.clone();
        let candidate_hard = settings.processor_affinity_hard.clone();
        let key = match kind {
            Kind::Soft => "cpu_sets_soft.enable",
            Kind::Hard => "processor_affinity_hard.enable",
        };
        let mut body = vec![
            widgets::card(
                "",
                [widgets::setting_row(
                    t!(key).to_string(),
                    ui.toggle(feature.enabled, Message::Enabled),
                )],
            ),
            widgets::heading(format!("{} ⓘ", t!("cpu_allocation.custom_rules")))
                .tooltip(t!("cpu_allocation.rules_help").to_string()),
            super::app_picker::view(
                &self.path,
                candidates,
                true,
                Message::Path,
                Message::Browse,
                Some(Message::Add),
                move |candidate| {
                    can_add_with_settings(
                        &candidate_soft,
                        &candidate_hard,
                        &candidate.info.image_path.to_string_lossy(),
                    )
                    .then_some(true)
                },
                ui.clone(),
            ),
        ];
        let mut rows = Vec::new();
        for (index, rule) in feature.rules.iter().enumerate() {
            let mut controls = Vec::new();
            for tier in [Tier::Focus, Tier::Visible, Tier::Background] {
                let mask = tier.mask(rule);
                let mut options = core_presets(&processors);
                options.extend(
                    settings
                        .cpu_allocation_presets
                        .iter()
                        .map(|preset| widgets::Choice(preset.core_mask, preset.name.clone())),
                );
                options.retain(|preset| preset.0 != 0);
                let selected = self
                    .selected_preset(&rule.executable_path, tier, mask, &options)
                    .unwrap_or_else(|| widgets::Choice(mask, t!("common.custom").to_string()));
                controls.push(ui.select(options, selected, move |choice| {
                    Message::Mask(index, tier, choice)
                }));
            }
            rows.push(widgets::process_rule_row(
                &rule.executable_path,
                candidates,
                ui.checkbox("", rule.enabled, move |value| {
                    Message::RuleEnabled(index, value)
                }),
                controls,
                widgets::rule_delete_button(&ui, Some(Message::Remove(index))),
            ));
        }
        body.push(widgets::process_rules_table(
            [Tier::Focus, Tier::Visible, Tier::Background].map(|tier| tier.label()),
            rows,
            t!("common.no_custom_rules").to_string(),
        ));
        widgets::page(body)
    }

    pub(super) fn modal(&self, settings: &Settings, ui: Ui<Message>) -> Option<Element> {
        if !self.preset_open {
            return None;
        }
        let title = if self.preset_read_only {
            self.preset_name.clone()
        } else {
            t!(if self.editing_preset.is_some() {
                "common.edit"
            } else {
                "cpu_allocation.add_preset"
            })
            .to_string()
        };
        let processors = cpu_allocation::logical_processors();
        let name = signal(self.preset_name.clone());
        let other_names = settings
            .cpu_allocation_presets
            .iter()
            .enumerate()
            .filter(|(index, _)| Some(*index) != self.editing_preset)
            .map(|(_, preset)| preset.name.trim().to_owned())
            .collect::<Vec<_>>();
        let mut body = vec![
            widgets::setting_row(
                t!("cpu_allocation.preset_name").to_string(),
                ui.input_signal(
                    name,
                    t!("cpu_allocation.preset_name_placeholder").to_string(),
                    Message::PresetName,
                )
                .enabled(!self.preset_read_only),
            ),
            widgets::help("cpu_allocation.preset_name_help"),
            widgets::heading(t!("cpu_allocation.selected_logical_cpus").to_string()),
            mask_selector(
                self.preset_mask,
                &processors,
                &settings.cpu_allocation_presets,
                Message::PresetMask,
                ui.clone(),
            )
            .enabled(!self.preset_read_only),
        ];
        if self.preset_mask == 0 {
            body.push(widgets::error(
                t!("cpu_allocation.no_logical_cpus").to_string(),
            ));
        }
        let mut actions = Vec::new();
        if !self.preset_read_only {
            let names = other_names.clone();
            body.push(Element::label(name.map(move |raw| {
                if !raw.trim().is_empty()
                    && names
                        .iter()
                        .any(|other| other.eq_ignore_ascii_case(raw.trim()))
                {
                    t!("cpu_allocation.duplicate_preset_name").to_string()
                } else {
                    String::new()
                }
            })));
            let mask = self.preset_mask;
            actions.push(
                ui.primary_button(t!("common.save").to_string(), Message::SavePreset)
                    .enabled_when(move || {
                        let name = name.get();
                        mask != 0
                            && !name.trim().is_empty()
                            && !other_names
                                .iter()
                                .any(|other| other.eq_ignore_ascii_case(name.trim()))
                    }),
            );
        }
        actions.push(ui.button(t!("common.cancel").to_string(), Message::ClosePreset));
        Some(widgets::editor_panel(
            title,
            widgets::col().children(body),
            widgets::row().children(actions),
        ))
    }

    pub(super) fn side_panel(
        &self,
        settings: &crate::application::SettingsEditor,
        kind: Kind,
        status: &crate::automation::RuntimeStatusSnapshot,
        ui: Ui<Message>,
    ) -> Element {
        let mut body = vec![ui.segmented(
            [
                widgets::Choice(false, t!("common.status").to_string()),
                widgets::Choice(true, t!("cpu_allocation.presets").to_string()),
            ],
            widgets::Choice(
                self.presets_tab,
                t!(if self.presets_tab {
                    "cpu_allocation.presets"
                } else {
                    "common.status"
                })
                .to_string(),
            ),
            |value| Message::RailTab(value.0),
        )];
        if !self.presets_tab {
            let page = match kind {
                Kind::Soft => crate::ui::Page::CpuSetsSoft,
                Kind::Hard => crate::ui::Page::ProcessorAffinityHard,
            };
            if let Some(status) =
                super::status_rail::view(page, settings, status, &[], ui.map(Message::Status))
            {
                body.push(status);
            }
        } else {
            body.push(widgets::heading(
                t!("cpu_allocation.core_presets").to_string(),
            ));
            for preset in core_presets(&cpu_allocation::logical_processors())
                .into_iter()
                .filter(|preset| preset.0 != 0)
            {
                body.push(ui.button(preset.1.clone(), Message::ViewPreset(preset.1, preset.0)));
            }
            body.push(widgets::heading(
                t!("adaptive_engine.custom_presets").to_string(),
            ));
            if settings.cpu_allocation_presets.is_empty() {
                body.push(widgets::label(
                    t!("cpu_allocation.no_custom_presets").to_string(),
                ));
            }
            for (index, preset) in settings.cpu_allocation_presets.iter().enumerate() {
                body.push(widgets::row().children([
                    ui.button(preset.name.clone(), Message::EditPreset(index)),
                    ui.button(
                        t!("common.remove").to_string(),
                        Message::DeletePreset(index),
                    ),
                ]));
            }
            body.push(ui.button(
                t!("cpu_allocation.add_preset").to_string(),
                Message::NewPreset,
            ));
        }
        widgets::page(body)
    }
}
fn can_add(s: &Settings, path: &str) -> bool {
    can_add_with_settings(&s.cpu_sets_soft, &s.processor_affinity_hard, path)
}
fn can_add_with_settings(
    soft: &CpuAllocationSettings,
    hard: &CpuAllocationSettings,
    path: &str,
) -> bool {
    can_add_process_candidate(
        path,
        |p| soft.contains_rule_for(p) || hard.contains_rule_for(p),
        cpu_allocation::is_builtin_excluded,
    )
}
fn core_presets(processors: &[LogicalProcessorInfo]) -> Vec<super::widgets::Choice<u64>> {
    let all = cpu_allocation::logical_processor_mask(processors);
    let p =
        cpu_allocation::logical_processor_kind_mask(processors, LogicalProcessorKind::Performance);
    let e =
        cpu_allocation::logical_processor_kind_mask(processors, LogicalProcessorKind::Efficiency);
    let smt = cpu_allocation::logical_processor_no_smt_mask(processors);
    [
        ("cpu_allocation.all", all),
        ("cpu_allocation.p_cores", p),
        ("cpu_allocation.e_cores", e),
        ("cpu_allocation.all_cores_no_smt", smt),
        ("cpu_allocation.p_cores_no_smt", p & smt),
        ("cpu_allocation.e_cores_no_smt", e & smt),
    ]
    .into_iter()
    .map(|(key, mask)| super::widgets::Choice(mask, t!(key).to_string()))
    .collect()
}
pub(super) fn mask_selector<M: Clone + 'static>(
    mask: u64,
    processors: &[LogicalProcessorInfo],
    custom: &[CpuAllocationPreset],
    message: impl Fn(u64) -> M + Clone + 'static,
    ui: Ui<M>,
) -> Element {
    let all = cpu_allocation::logical_processor_mask(processors);
    let mut options = core_presets(processors);
    options.extend(
        custom
            .iter()
            .map(|preset| widgets::Choice(preset.core_mask & all, preset.name.clone())),
    );
    options.retain(|preset| preset.0 != 0);
    let selected = options
        .iter()
        .find(|preset| preset.0 == mask)
        .cloned()
        .unwrap_or_else(|| widgets::Choice(mask, t!("common.custom").to_string()));
    let select = message.clone();
    let mut body = vec![ui.select(options, selected, move |choice| select(choice.0))];
    for cpu in processors.iter().filter(|cpu| cpu.index < 64) {
        let bit = 1u64 << cpu.index;
        let message = message.clone();
        let kind = t!(match cpu.kind {
            LogicalProcessorKind::Performance => "cpu_allocation.p_core",
            LogicalProcessorKind::Efficiency => "cpu_allocation.e_core",
            LogicalProcessorKind::Standard => "cpu_allocation.core",
        });
        body.push(ui.checkbox(
            format!("CPU {} ({kind} {})", cpu.index, cpu.core_index),
            mask & bit != 0,
            move |value| message(if value { mask | bit } else { mask & !bit }),
        ));
    }
    body.push(widgets::label(format!("0x{mask:016X}")));
    widgets::col().children(body)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_candidate_policy_rejects_duplicates_across_allocation_families() {
        let rule = CpuAllocationRule {
            enabled: false,
            executable_path: r"C:\Apps\example.exe".into(),
            focus_core_mask: 0,
            visible_window_core_mask: 0,
            background_core_mask: 0,
        };
        let empty = CpuAllocationSettings::default();
        let configured = CpuAllocationSettings {
            rules: vec![rule],
            ..Default::default()
        };
        assert!(!can_add_with_settings(
            &configured,
            &empty,
            "c:/apps/EXAMPLE.exe"
        ));
        assert!(!can_add_with_settings(
            &empty,
            &configured,
            "c:/apps/EXAMPLE.exe"
        ));
        assert!(can_add_with_settings(
            &empty,
            &empty,
            r"C:\Apps\example.exe"
        ));
    }

    #[test]
    fn identical_masks_keep_explicit_selection_per_rule_and_tier() {
        for kind in [Kind::Soft, Kind::Hard] {
            let mut editor = Editor::default();
            let mut s = Settings::default();
            let mask =
                cpu_allocation::logical_processor_mask(&cpu_allocation::logical_processors());
            let options = vec![
                super::super::widgets::Choice(mask, "All".into()),
                super::super::widgets::Choice(mask, "Mine".into()),
            ];
            settings_mut(&mut s, kind).rules.push(CpuAllocationRule {
                enabled: true,
                executable_path: "app.exe".into(),
                focus_core_mask: mask,
                visible_window_core_mask: mask,
                background_core_mask: mask,
            });
            editor.update(
                &mut s,
                kind,
                Message::Mask(0, Tier::Focus, options[1].clone()),
            );
            assert_eq!(
                editor.selected_preset("app.exe", Tier::Focus, mask, &options),
                Some(options[1].clone())
            );
            assert_eq!(
                editor.selected_preset("app.exe", Tier::Background, mask, &options),
                Some(options[0].clone())
            );
            editor.reset_drafts();
            assert_eq!(
                editor.selected_preset("app.exe", Tier::Focus, mask, &options),
                Some(options[1].clone())
            );
            editor.update(
                &mut s,
                kind,
                Message::Mask(0, Tier::Focus, options[0].clone()),
            );
            assert_eq!(
                editor.selected_preset("app.exe", Tier::Focus, mask, &options),
                Some(options[0].clone())
            );
            editor.update(&mut s, kind, Message::Remove(0));
            assert!(editor.selected_presets.is_empty());
        }
    }

    #[test]
    fn builtin_preview_is_read_only_and_cancel_discards_new_preset() {
        let mut editor = Editor::default();
        let mut s = Settings::default();
        let before = s.clone();
        editor.update(
            &mut s,
            Kind::Soft,
            Message::ViewPreset("All".into(), u64::MAX),
        );
        let mask = editor.preset_mask;
        editor.update(&mut s, Kind::Soft, Message::PresetMask(0));
        editor.update(&mut s, Kind::Soft, Message::PresetName("Changed".into()));
        editor.update(&mut s, Kind::Soft, Message::SavePreset);
        assert_eq!(editor.preset_mask, mask);
        assert_eq!(editor.preset_name, "All");
        assert_eq!(s, before);
        editor.update(&mut s, Kind::Hard, Message::NewPreset);
        editor.update(&mut s, Kind::Hard, Message::PresetName("Draft".into()));
        editor.update(&mut s, Kind::Hard, Message::ClosePreset);
        assert_eq!(s, before);
        assert!(!editor.preset_open);
    }

    #[test]
    fn preset_edits_do_not_rewrite_rules() {
        let mut s = Settings::default();
        s.cpu_sets_soft.rules.push(CpuAllocationRule {
            enabled: true,
            executable_path: r"C:\App\app.exe".into(),
            focus_core_mask: 1,
            visible_window_core_mask: 2,
            background_core_mask: 3,
        });
        s.cpu_allocation_presets.push(CpuAllocationPreset {
            name: "one".into(),
            core_mask: 1,
        });
        let mut e = Editor::default();
        e.update(&mut s, Kind::Soft, Message::DeletePreset(0));
        assert_eq!(s.cpu_sets_soft.rules[0].focus_core_mask, 1);
        assert!(s.cpu_allocation_presets.is_empty());
        e.update(&mut s, Kind::Soft, Message::Remove(0));
        assert!(s.cpu_sets_soft.rules.is_empty());
    }
}
