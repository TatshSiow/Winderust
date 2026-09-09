use crate::config::{CpuAllocationPreset, CpuAllocationRule, CpuAllocationSettings, Settings};
use crate::cpu_allocation::{self, LogicalProcessorInfo, LogicalProcessorKind};
use crate::foreground::executable_path_key;
use crate::ui::process_rules::can_add_process_candidate;
use iced::widget::{button, checkbox, column, row, scrollable, text, text_input};
use iced::{Element, Fill};
use rust_i18n::t;
use std::path::Path;

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
    removing: Option<usize>,
    deleting: Option<CpuAllocationRule>,
    preset_name: String,
    preset_mask: u64,
    editing_preset: Option<usize>,
    preset_open: bool,
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
    Mask(usize, Tier, u64),
    Remove(usize),
    ConfirmRemove,
    CancelRemove,
    Removed(String),
    NewPreset,
    EditPreset(usize),
    PresetName(String),
    PresetMask(u64),
    SavePreset,
    DeletePreset(usize),
    ClosePreset,
}
impl Editor {
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
                if settings(s, k).enabled && can_add(s, &self.path) {
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
            Message::Mask(i, tier, m) => {
                if let Some(r) = settings_mut(s, k).rules.get_mut(i) {
                    tier.set(r, m & available)
                }
            }
            Message::Remove(i) => self.removing = Some(i),
            Message::ConfirmRemove => {
                if let Some(i) = self.removing.take() {
                    let rules = &mut settings_mut(s, k).rules;
                    if i < rules.len() {
                        self.deleting = Some(rules.remove(i));
                    }
                }
            }
            Message::Removed(path) => {
                if self
                    .deleting
                    .as_ref()
                    .is_some_and(|r| r.executable_path == path)
                {
                    self.deleting = None;
                }
            }
            Message::CancelRemove => self.removing = None,
            Message::NewPreset => {
                self.preset_open = true;
                self.presets_tab = true;
                self.editing_preset = None;
                self.preset_name.clear();
                self.preset_mask = available;
            }
            Message::EditPreset(i) => {
                if let Some(p) = s.cpu_allocation_presets.get(i) {
                    self.preset_open = true;
                    self.presets_tab = true;
                    self.editing_preset = Some(i);
                    self.preset_name = p.name.clone();
                    self.preset_mask = p.core_mask & available;
                }
            }
            Message::PresetName(v) => self.preset_name = v,
            Message::PresetMask(v) => self.preset_mask = v & available,
            Message::SavePreset => {
                let name = self.preset_name.trim();
                if !name.is_empty()
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
    pub(super) fn has_pending_editor(&self) -> bool {
        self.preset_open
    }
    pub(super) fn view<'a>(
        &'a self,
        s: &'a Settings,
        k: Kind,
        candidates: &'a [String],
        motion: bool,
        status: &'a crate::automation::RuntimeStatusSnapshot,
    ) -> Element<'a, Message> {
        let processors = cpu_allocation::logical_processors();
        let feature = settings(s, k);
        let mut body = column![
            super::widgets::settings_card(
                checkbox(feature.enabled)
                    .label(
                        t!(match k {
                            Kind::Soft => "cpu_sets_soft.enable",
                            Kind::Hard => "processor_affinity_hard.enable",
                        })
                        .to_string()
                    )
                    .on_toggle(Message::Enabled)
            ),
            text(t!("cpu_allocation.rules_help").to_string())
                .width(Fill)
                .style(text::secondary),
            super::widgets::settings_card(
                row![
                    text_input("C:\\App\\app.exe", &self.path)
                        .on_input(Message::Path)
                        .width(Fill),
                    button(text(t!("common.browse_executable").to_string()))
                        .on_press(Message::Browse),
                    button(text(t!("common.add").to_string())).on_press_maybe(
                        (feature.enabled && can_add(s, &self.path)).then_some(Message::Add)
                    )
                ]
                .spacing(8)
                .align_y(iced::Center)
            )
        ]
        .spacing(12);
        for candidate in candidates
            .iter()
            .filter(|p| p.to_lowercase().contains(&self.path.to_lowercase()) && can_add(s, p))
            .take(8)
        {
            body = body.push(button(text(candidate)).on_press(Message::Path(candidate.clone())));
        }
        if matches!(k, Kind::Hard) {
            body = body.push(text(t!("processor_affinity_hard.warning").to_string()));
        } else if cpu_allocation::has_multiple_processor_groups() {
            body = body.push(text(t!("cpu_sets_soft.warning").to_string()));
        }
        let mut rules = Vec::new();
        for (i, r) in feature.rules.iter().chain(self.deleting.iter()).enumerate() {
            let mut rule = column![row![
                checkbox(r.enabled)
                    .label(r.executable_path.clone())
                    .on_toggle(move |v| Message::RuleEnabled(i, v)),
                button(text(t!("common.remove").to_string())).on_press(Message::Remove(i))
            ]
            .spacing(8)]
            .spacing(8);
            for tier in [Tier::Focus, Tier::Visible, Tier::Background] {
                let mask = tier.mask(r);
                rule = rule.push(text(tier.label())).push(mask_selector(
                    mask,
                    &processors,
                    &s.cpu_allocation_presets,
                    move |v| Message::Mask(i, tier, v),
                ));
            }
            rules.push((
                super::motion::key(&r.executable_path),
                super::motion::removal(
                    super::widgets::settings_card(rule),
                    self.deleting
                        .as_ref()
                        .is_some_and(|old| old.executable_path == r.executable_path),
                    motion,
                    Message::Removed(r.executable_path.clone()),
                ),
            ));
        }
        body = body.push(iced::widget::keyed_column(rules).spacing(12));
        if self.removing.is_some() {
            body = body.push(super::widgets::settings_card(
                row![
                    text(t!("common.remove").to_string()),
                    button(text(t!("common.remove").to_string())).on_press(Message::ConfirmRemove),
                    button(text(t!("common.cancel").to_string())).on_press(Message::CancelRemove)
                ]
                .spacing(8)
                .align_y(iced::Center),
            ));
        }
        let mut rail = column![
            text(t!("cpu_allocation.presets").to_string()),
            button(text(t!("cpu_allocation.add_preset").to_string())).on_press(Message::NewPreset)
        ]
        .spacing(8);
        for (i, p) in s.cpu_allocation_presets.iter().enumerate() {
            rail = rail.push(button(text(p.name.clone())).on_press(Message::EditPreset(i)));
        }
        if self.preset_open {
            if self.preset_mask == 0 {
                rail = rail.push(text(t!("cpu_allocation.no_logical_cpus").to_string()));
            }
            if s.cpu_allocation_presets.iter().enumerate().any(|(i, p)| {
                Some(i) != self.editing_preset
                    && p.name.trim().eq_ignore_ascii_case(self.preset_name.trim())
            }) {
                rail = rail.push(text(t!("cpu_allocation.duplicate_preset_name").to_string()));
            }
            rail = rail
                .push(
                    text_input(&t!("cpu_allocation.preset_name"), &self.preset_name)
                        .on_input(Message::PresetName),
                )
                .push(mask_selector(
                    self.preset_mask,
                    &processors,
                    &[],
                    Message::PresetMask,
                ))
                .push(
                    row![
                        button(text(t!("common.save").to_string())).on_press(Message::SavePreset),
                        button(text(t!("common.cancel").to_string()))
                            .on_press(Message::ClosePreset)
                    ]
                    .spacing(8),
                );
            if let Some(i) = self.editing_preset {
                rail = rail.push(
                    button(text(t!("common.remove").to_string()))
                        .on_press(Message::DeletePreset(i)),
                );
            }
        }
        if !self.presets_tab {
            let page = match k {
                Kind::Soft => crate::ui::Page::CpuSetsSoft,
                Kind::Hard => crate::ui::Page::ProcessorAffinityHard,
            };
            rail = column![];
            if let Some(status) = super::status_rail::view(page, s, status, &[]) {
                rail = rail.push(status.map(Message::Status));
            }
        }
        let rail = column![
            row![
                button(text(t!("common.status").to_string()))
                    .on_press(Message::RailTab(false))
                    .style(if self.presets_tab {
                        super::widgets::quiet
                    } else {
                        super::widgets::selected
                    }),
                button(text(t!("cpu_allocation.presets").to_string()))
                    .on_press(Message::RailTab(true))
                    .style(if self.presets_tab {
                        super::widgets::selected
                    } else {
                        super::widgets::quiet
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
fn can_add(s: &Settings, path: &str) -> bool {
    can_add_process_candidate(
        path,
        |p| s.cpu_sets_soft.contains_rule_for(p) || s.processor_affinity_hard.contains_rule_for(p),
        cpu_allocation::is_builtin_excluded,
    )
}
pub(super) fn mask_selector<'a, M: Clone + 'a>(
    mask: u64,
    processors: &[LogicalProcessorInfo],
    custom: &[CpuAllocationPreset],
    message: impl Fn(u64) -> M + Clone + 'a,
) -> Element<'a, M> {
    let all = cpu_allocation::logical_processor_mask(processors);
    let p =
        cpu_allocation::logical_processor_kind_mask(processors, LogicalProcessorKind::Performance);
    let e =
        cpu_allocation::logical_processor_kind_mask(processors, LogicalProcessorKind::Efficiency);
    let smt = cpu_allocation::logical_processor_no_smt_mask(processors);
    let mut presets = row![].spacing(4);
    for (key, m) in [
        ("cpu_allocation.all", all),
        ("cpu_allocation.p_cores", p),
        ("cpu_allocation.e_cores", e),
        ("cpu_allocation.all_cores_no_smt", smt),
        ("cpu_allocation.p_cores_no_smt", p & smt),
        ("cpu_allocation.e_cores_no_smt", e & smt),
    ] {
        presets = presets
            .push(button(text(t!(key).to_string())).on_press_maybe((m != 0).then(|| message(m))));
    }
    for preset in custom {
        let m = preset.core_mask & all;
        presets = presets
            .push(button(text(preset.name.clone())).on_press_maybe((m != 0).then(|| message(m))));
    }
    let mut cpus = row![].spacing(6);
    for cpu in processors.iter().filter(|p| p.index < 64) {
        let bit = 1u64 << cpu.index;
        let msg = message.clone();
        let kind = t!(match cpu.kind {
            LogicalProcessorKind::Performance => "cpu_allocation.p_core",
            LogicalProcessorKind::Efficiency => "cpu_allocation.e_core",
            LogicalProcessorKind::Standard => "cpu_allocation.core",
        });
        cpus = cpus.push(
            checkbox(mask & bit != 0)
                .label(format!("{} ({kind} {})", cpu.index, cpu.core_index))
                .on_toggle(move |v| msg(if v { mask | bit } else { mask & !bit })),
        );
    }
    column![presets.wrap(), cpus.wrap(), text(format!("0x{mask:016X}"))]
        .spacing(8)
        .into()
}
#[cfg(test)]
mod tests {
    use super::*;
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
        e.update(&mut s, Kind::Soft, Message::ConfirmRemove);
        assert!(s.cpu_sets_soft.rules.is_empty());
        assert!(e.deleting.is_some());
    }
}
