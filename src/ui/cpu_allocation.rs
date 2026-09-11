use super::design;
use super::widgets::{button, checkbox, text_input};
use crate::config::{CpuAllocationPreset, CpuAllocationRule, CpuAllocationSettings, Settings};
use crate::cpu_allocation::{self, LogicalProcessorInfo, LogicalProcessorKind};
use crate::foreground::executable_path_key;
use crate::ui::process_rules::can_add_process_candidate;
use iced::widget::{column, row, scrollable, text};
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
    Mask(usize, Tier, u64),
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
            Message::Remove(i) => {
                let rules = &mut settings_mut(s, k).rules;
                if i < rules.len() {
                    rules.remove(i);
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
    pub(super) fn has_pending_editor(&self) -> bool {
        self.preset_open && !self.preset_read_only
    }
    pub(super) fn view<'a>(
        &'a self,
        s: &'a Settings,
        k: Kind,
        candidates: &'a [super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        let processors = cpu_allocation::logical_processors();
        let feature = settings(s, k);
        let mut body = column![
            super::widgets::settings_card(super::widgets::setting_row(
                match k {
                    Kind::Soft => "cpu_sets_soft.enable",
                    Kind::Hard => "processor_affinity_hard.enable",
                },
                super::widgets::switch(feature.enabled, Some(Message::Enabled)),
            )),
            super::widgets::setting_title("cpu_allocation.custom_rules"),
            text(t!("cpu_allocation.rules_help").to_string())
                .width(Fill)
                .style(text::secondary),
            super::app_picker::view(
                &self.path,
                candidates,
                feature.enabled,
                Message::Path,
                Message::Browse,
                (feature.enabled && can_add(s, &self.path)).then_some(Message::Add),
                |path| can_add(s, path).then_some(true)
            )
        ]
        .spacing(super::widgets::CARD_GAP);
        if matches!(k, Kind::Hard) {
            body = body.push(text(t!("processor_affinity_hard.warning").to_string()));
        } else if cpu_allocation::has_multiple_processor_groups() {
            body = body.push(text(t!("cpu_sets_soft.warning").to_string()));
        }
        let mut rules = Vec::new();
        for (i, r) in feature.rules.iter().enumerate() {
            let controls = [Tier::Focus, Tier::Visible, Tier::Background]
                .into_iter()
                .map(|tier| {
                    let mask = tier.mask(r);
                    let mut choices = core_presets(&processors);
                    choices.extend(
                        s.cpu_allocation_presets
                            .iter()
                            .map(|p| super::widgets::Choice(p.core_mask, p.name.clone())),
                    );
                    choices.retain(|p| p.0 != 0);
                    let selected = choices
                        .iter()
                        .find(|p| p.0 == mask)
                        .cloned()
                        .unwrap_or_else(|| {
                            super::widgets::Choice(mask, t!("common.custom").to_string())
                        });
                    if !choices.contains(&selected) {
                        choices.push(selected.clone());
                    }
                    super::widgets::pick_list(choices, Some(selected), move |v| {
                        Message::Mask(i, tier, v.0)
                    })
                    .width(Fill)
                    .into()
                })
                .collect();
            rules.push((
                super::widgets::stable_key(&r.executable_path),
                super::widgets::process_rule_row(
                    &r.executable_path,
                    candidates,
                    checkbox(r.enabled)
                        .on_toggle(move |v| Message::RuleEnabled(i, v))
                        .into(),
                    controls,
                    Some(Message::Remove(i)),
                ),
            ));
        }
        body = body.push(super::widgets::process_rules_table(
            [Tier::Focus, Tier::Visible, Tier::Background].map(|tier| tier.label()),
            rules,
            t!("common.no_custom_rules").to_string(),
        ));

        scrollable(body).width(Fill).height(Fill).into()
    }

    pub(super) fn modal<'a>(&'a self, s: &Settings) -> Option<Element<'a, Message>> {
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
        let header = row![
            text(title).width(Fill),
            button(super::navigation::glyph("icons/x.svg")).on_press(Message::ClosePreset)
        ]
        .align_y(iced::Center);
        let mut cpus = row![].spacing(design::space::CONTROL);
        for cpu in cpu_allocation::logical_processors()
            .iter()
            .filter(|cpu| cpu.index < 64)
        {
            let bit = 1u64 << cpu.index;
            let selected = self.preset_mask & bit != 0;
            let kind = t!(match cpu.kind {
                LogicalProcessorKind::Performance => "cpu_allocation.p_core",
                LogicalProcessorKind::Efficiency => "cpu_allocation.e_core",
                LogicalProcessorKind::Standard => "cpu_allocation.core",
            })
            .to_string();
            cpus = cpus.push(
                button(
                    iced::widget::container(
                        column![
                            text(kind).size(design::typography::CAPTION),
                            text(format!("CPU {}", cpu.index))
                        ]
                        .align_x(iced::Center),
                    )
                    .center_x(Fill)
                    .center_y(Fill),
                )
                .width(100)
                .height(64)
                .style(if selected {
                    super::widgets::primary_button
                } else {
                    super::widgets::tertiary_button
                })
                .on_press_maybe(
                    (!self.preset_read_only).then_some(Message::PresetMask(self.preset_mask ^ bit)),
                ),
            );
        }
        let valid_name = !self.preset_name.trim().is_empty()
            && !s.cpu_allocation_presets.iter().enumerate().any(|(i, p)| {
                Some(i) != self.editing_preset
                    && p.name.trim().eq_ignore_ascii_case(self.preset_name.trim())
            });
        let mut body = column![
            super::widgets::settings_card(
                column![
                    text(t!("cpu_allocation.preset_name").to_string()),
                    text_input(
                        &t!("cpu_allocation.preset_name_placeholder"),
                        &self.preset_name
                    )
                    .on_input_maybe((!self.preset_read_only).then_some(Message::PresetName)),
                    text(t!("cpu_allocation.preset_name_help").to_string()).style(text::secondary)
                ]
                .spacing(design::space::SMALL)
            ),
            super::widgets::settings_card(
                column![
                    text(t!("cpu_allocation.selected_logical_cpus").to_string()),
                    cpus.wrap()
                ]
                .spacing(design::space::SMALL)
            )
        ]
        .spacing(design::space::MEDIUM);
        if self.preset_mask == 0 {
            body = body
                .push(text(t!("cpu_allocation.no_logical_cpus").to_string()).style(text::danger));
        }
        if !self.preset_read_only && !self.preset_name.trim().is_empty() && !valid_name {
            body = body.push(
                text(t!("cpu_allocation.duplicate_preset_name").to_string()).style(text::danger),
            );
        }
        let mut footer = row![
            iced::widget::Space::new().width(Fill),
            button(text(t!("common.cancel").to_string()))
                .style(super::widgets::tertiary_button)
                .on_press(Message::ClosePreset)
        ]
        .spacing(design::space::SMALL);
        if !self.preset_read_only {
            footer = footer.push(
                button(text(t!("common.save").to_string()))
                    .style(super::widgets::primary_button)
                    .on_press_maybe(
                        (valid_name && self.preset_mask != 0).then_some(Message::SavePreset),
                    ),
            );
        }
        Some(super::widgets::modal_frame(
            header,
            scrollable(body).height(Fill),
            footer,
            (950, 760),
        ))
    }
    pub(super) fn side_panel<'a>(
        &'a self,
        s: &'a Settings,
        k: Kind,
        status: &'a crate::automation::RuntimeStatusSnapshot,
    ) -> Element<'a, Message> {
        let processors = cpu_allocation::logical_processors();
        let mut rail = column![
            super::widgets::heading(
                t!("cpu_allocation.presets").to_string(),
                design::typography::SUBTITLE
            ),
            super::widgets::heading(
                t!("cpu_allocation.core_presets").to_string(),
                design::typography::SECONDARY
            ),
        ]
        .spacing(design::space::MEDIUM);
        for preset in core_presets(&processors).into_iter().filter(|p| p.0 != 0) {
            let message = Message::ViewPreset(preset.1.clone(), preset.0);
            rail = rail.push(
                row![
                    button(
                        iced::widget::container(text(preset.1))
                            .center_y(design::NAVIGATION_ROW_HEIGHT - 10)
                    )
                    .width(Fill)
                    .style(super::widgets::quiet)
                    .on_press(message.clone()),
                    button(super::navigation::glyph("icons/info.svg"))
                        .style(super::widgets::quiet)
                        .on_press(message)
                ]
                .align_y(iced::Center)
                .spacing(design::space::TIGHT),
            );
        }
        rail = rail.push(super::widgets::heading(
            t!("adaptive_engine.custom_presets").to_string(),
            design::typography::SECONDARY,
        ));
        if s.cpu_allocation_presets.is_empty() {
            rail = rail.push(
                text(t!("cpu_allocation.no_custom_presets").to_string()).style(text::secondary),
            );
        }
        for (i, p) in s.cpu_allocation_presets.iter().enumerate() {
            rail = rail.push(
                row![
                    button(text(p.name.clone()))
                        .width(Fill)
                        .style(super::widgets::quiet)
                        .on_press(Message::EditPreset(i)),
                    iced::widget::tooltip(
                        button(super::navigation::glyph("icons/pencil.svg"))
                            .padding(7)
                            .width(32)
                            .height(32)
                            .style(super::widgets::quiet)
                            .on_press(Message::EditPreset(i)),
                        text(t!("common.edit").to_string()),
                        iced::widget::tooltip::Position::Top
                    ),
                    super::widgets::rule_delete_button(Some(Message::DeletePreset(i)))
                ]
                .align_y(iced::Center)
                .spacing(design::space::TIGHT),
            );
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
        let tabs = row![
            super::widgets::panel_tab(
                t!("common.status").to_string(),
                !self.presets_tab,
                Message::RailTab(false)
            ),
            super::widgets::panel_tab(
                t!("cpu_allocation.presets").to_string(),
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
                t!("cpu_allocation.add_preset").to_string(),
                Message::NewPreset,
            ));
        }
        panel.into()
    }
}
fn can_add(s: &Settings, path: &str) -> bool {
    can_add_process_candidate(
        path,
        |p| s.cpu_sets_soft.contains_rule_for(p) || s.processor_affinity_hard.contains_rule_for(p),
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
pub(super) fn mask_selector<'a, M: Clone + 'a>(
    mask: u64,
    processors: &[LogicalProcessorInfo],
    custom: &[CpuAllocationPreset],
    message: impl Fn(u64) -> M + Clone + 'a,
) -> Element<'a, M> {
    let all = cpu_allocation::logical_processor_mask(processors);
    let mut presets = row![].spacing(design::space::TIGHT);
    for preset in core_presets(processors) {
        presets = presets.push(
            button(text(preset.1)).on_press_maybe((preset.0 != 0).then(|| message(preset.0))),
        );
    }
    for preset in custom {
        let m = preset.core_mask & all;
        presets = presets
            .push(button(text(preset.name.clone())).on_press_maybe((m != 0).then(|| message(m))));
    }
    let mut cpus = row![].spacing(design::space::CONTROL);
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
        .spacing(design::space::SMALL)
        .into()
}
#[cfg(test)]
mod tests {
    use super::*;
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
