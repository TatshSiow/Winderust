use super::design;
use super::widgets::{action_button, button, checkbox, icon_button, text_input};
use crate::config::{CpuAllocationPreset, CpuAllocationRule, CpuAllocationSettings, Settings};
use crate::cpu_allocation::{self, LogicalProcessorInfo, LogicalProcessorKind};
use crate::foreground::executable_path_key;
use crate::ui::process_rules::can_add_process_candidate;
use crate::ui::scrolling::scrollable;
use iced::widget::{column, row, text};
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
    selected_presets: std::collections::BTreeMap<(String, usize), super::widgets::Choice<u64>>,
    preset_name: String,
    preset_mask: u64,
    editing_preset: Option<usize>,
    preset_open: bool,
    preset_read_only: bool,
    preset_topology: Option<Result<crate::cpu_topology::CpuTopology, String>>,
    error: String,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
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
    pub(super) fn update(&mut self, s: &mut Settings, k: Kind, m: Message) {
        let available =
            || cpu_allocation::logical_processor_mask(&cpu_allocation::logical_processors());
        match m {
            Message::Enabled(v) => settings_mut(s, k).enabled = v,
            Message::Path(v) => self.path = v,
            Message::Browse => {}
            Message::Add => {
                if can_add(s, &self.path) {
                    let available = available();
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
                if !mask_is_available(choice.0, available()) {
                    self.error = t!("cpu_allocation.selection_unavailable").to_string();
                    return;
                }
                if let Some(r) = settings_mut(s, k).rules.get_mut(i) {
                    tier.set(r, choice.0);
                    self.selected_presets
                        .insert((r.executable_path.clone(), tier as usize), choice);
                    self.error.clear();
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
                self.preset_mask = mask;
                self.error.clear();
            }
            Message::NewPreset => {
                self.preset_read_only = false;
                self.preset_open = true;
                self.editing_preset = None;
                self.preset_name.clear();
                self.preset_mask = available();
                self.preset_topology = Some(cpu_allocation::processor_topology());
                self.error.clear();
            }
            Message::EditPreset(i) => {
                self.preset_read_only = false;
                if let Some(p) = s.cpu_allocation_presets.get(i) {
                    self.preset_open = true;
                    self.editing_preset = Some(i);
                    self.preset_name = p.name.clone();
                    self.preset_mask = p.core_mask;
                    self.preset_topology = Some(cpu_allocation::processor_topology());
                    self.error.clear();
                }
            }
            Message::PresetName(v) if !self.preset_read_only => self.preset_name = v,
            Message::PresetMask(v) if !self.preset_read_only => {
                if mask_is_available(v, available()) {
                    self.preset_mask = v;
                    self.error.clear();
                } else {
                    self.error = t!("cpu_allocation.selection_unavailable").to_string();
                }
            }
            Message::PresetName(_) | Message::PresetMask(_) => {}
            Message::SavePreset => {
                if self.preset_read_only {
                    return;
                }
                if let Err(error) =
                    self.validate_preset_selection(&cpu_allocation::processor_topology())
                {
                    self.error = error;
                    return;
                }
                let name = self.preset_name.trim();
                if !name.is_empty()
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
                    self.error.clear();
                }
            }
            Message::DeletePreset(i) => {
                if i < s.cpu_allocation_presets.len() {
                    s.cpu_allocation_presets.remove(i);
                    self.preset_open = false;
                }
            }
            Message::ClosePreset => {
                self.preset_open = false;
                self.error.clear();
            }
        }
    }

    fn validate_preset_selection(
        &self,
        current: &Result<crate::cpu_topology::CpuTopology, String>,
    ) -> Result<(), String> {
        let unavailable =
            |error: &String| t!("cpu_allocation.topology_unavailable", error = error).to_string();
        let current = current.as_ref().map_err(unavailable)?;
        let captured = self
            .preset_topology
            .as_ref()
            .ok_or_else(|| t!("cpu_allocation.topology_changed").to_string())?
            .as_ref()
            .map_err(unavailable)?;
        if current != captured {
            return Err(t!("cpu_allocation.topology_changed").to_string());
        }
        let available = current
            .groups()
            .iter()
            .find(|group| group.number == 0)
            .map_or(0, |group| group.active_mask);
        if !mask_is_available(self.preset_mask, available) {
            return Err(t!("cpu_allocation.selection_unavailable").to_string());
        }
        if self.preset_mask == 0 {
            return Err(t!("cpu_allocation.no_logical_cpus").to_string());
        }
        Ok(())
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
    pub(super) fn view<'a>(
        &'a self,
        s: &'a Settings,
        k: Kind,
        candidates: &'a [super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        let feature = settings(s, k);
        let processors = if feature.rules.is_empty() {
            Vec::new()
        } else {
            cpu_allocation::logical_processors()
        };
        let available = cpu_allocation::logical_processor_mask(&processors);
        let presets = if processors.is_empty() {
            Vec::new()
        } else {
            core_presets(&processors)
        };
        let mut body = column![
            super::widgets::toggle_card(
                match k {
                    Kind::Soft => "cpu_sets_soft.enable",
                    Kind::Hard => "processor_affinity_hard.enable",
                },
                feature.enabled,
                Some(Message::Enabled),
            ),
            super::widgets::setting_title_with_help(
                "cpu_allocation.custom_rules",
                "",
                "cpu_allocation.rules_help"
            ),
            super::app_picker::view(
                &self.path,
                candidates,
                true,
                Message::Path,
                Message::Browse,
                (can_add(s, &self.path)).then_some(Message::Add),
                |path| can_add(s, path).then_some(true)
            )
        ]
        .spacing(super::widgets::CARD_GAP);
        let mut rules = Vec::new();
        for (i, r) in feature.rules.iter().enumerate() {
            let controls = [Tier::Focus, Tier::Visible, Tier::Background]
                .into_iter()
                .map(|tier| {
                    let mask = tier.mask(r);
                    let mut choices = presets.clone();
                    choices.extend(
                        s.cpu_allocation_presets
                            .iter()
                            .map(|p| super::widgets::Choice(p.core_mask, p.name.clone())),
                    );
                    choices.retain(|p| p.0 != 0 && mask_is_available(p.0, available));
                    let selected = self
                        .selected_preset(&r.executable_path, tier, mask, &choices)
                        .unwrap_or_else(|| {
                            super::widgets::Choice(
                                mask,
                                t!(if mask_is_available(mask, available) {
                                    "common.custom"
                                } else {
                                    "cpu_allocation.unavailable_selection"
                                })
                                .to_string(),
                            )
                        });
                    if !choices.contains(&selected) {
                        choices.push(selected.clone());
                    }
                    let mut control =
                        column![
                            super::widgets::pick_list(choices, Some(selected), move |v| {
                                Message::Mask(i, tier, v)
                            })
                            .width(Fill)
                        ]
                        .spacing(design::space::TIGHT);
                    if !mask_is_available(mask, available) {
                        control = control.push(
                            text(t!("cpu_allocation.selection_unavailable").to_string())
                                .style(text::danger),
                        );
                    }
                    control.into()
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
        if !self.error.is_empty() {
            body = body.push(text(self.error.clone()).style(text::danger));
        }

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
            icon_button("icons/x.svg").on_press(Message::ClosePreset)
        ]
        .align_y(iced::Center);
        let processors = cpu_allocation::logical_processors();
        let available = cpu_allocation::logical_processor_mask(&processors);
        let topology = cpu_allocation::processor_topology();
        let validation_error = if self.preset_read_only {
            (!mask_is_available(self.preset_mask, available))
                .then(|| t!("cpu_allocation.selection_unavailable").to_string())
        } else {
            self.validate_preset_selection(&topology).err()
        };
        let mut cpus = row![].spacing(design::space::CONTROL);
        for cpu in processors.iter().filter(|cpu| cpu.index < 64) {
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
                .style(move |theme, status| {
                    if selected {
                        super::widgets::tab_style(theme, status, true)
                    } else {
                        super::widgets::tertiary_button(theme, status)
                    }
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
            super::widgets::heading(
                t!("common.preset_details").to_string(),
                design::typography::BODY
            ),
            super::widgets::settings_card(super::widgets::settings_rows([
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
                .into(),
                column![
                    text(t!("cpu_allocation.selected_logical_cpus").to_string()),
                    cpus.wrap(),
                    text(format!("0x{:016X}", self.preset_mask))
                ]
                .spacing(design::space::SMALL)
                .into(),
            ]))
        ]
        .spacing(design::space::MEDIUM);
        if let Ok(topology) = &topology {
            let cores = topology
                .processors()
                .iter()
                .map(|cpu| cpu.core)
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            body = body.push(
                text(
                    t!(
                        "cpu_allocation.topology_summary",
                        processors = topology.processors().len(),
                        cores = cores,
                        groups = topology.groups().len(),
                        numa_nodes = topology.numa_nodes().len()
                    )
                    .to_string(),
                )
                .style(text::secondary),
            );
        }
        if !self.preset_read_only {
            let mut choices = row![].spacing(design::space::TIGHT);
            for preset in core_presets(&processors) {
                choices = choices.push(
                    button(text(preset.1)).on_press_maybe(
                        (preset.0 != 0 && mask_is_available(preset.0, available))
                            .then_some(Message::PresetMask(preset.0)),
                    ),
                );
            }
            body = body.push(choices.wrap());
        }
        if let Some(error) = &validation_error {
            body = body.push(text(error.clone()).style(text::danger));
        }
        if !self.error.is_empty() && validation_error.as_deref() != Some(self.error.as_str()) {
            body = body.push(text(self.error.clone()).style(text::danger));
        }
        if !self.preset_read_only && !self.preset_name.trim().is_empty() && !valid_name {
            body = body.push(
                text(t!("cpu_allocation.duplicate_preset_name").to_string()).style(text::danger),
            );
        }
        let mut footer = row![
            iced::widget::Space::new().width(Fill),
            action_button(
                t!(if self.preset_read_only {
                    "common.close"
                } else {
                    "common.cancel"
                })
                .to_string(),
            )
            .style(super::widgets::tertiary_button)
            .on_press(Message::ClosePreset)
        ]
        .spacing(design::space::SMALL);
        if !self.preset_read_only {
            footer = footer.push(
                action_button(t!("common.save").to_string())
                    .style(super::widgets::primary_button)
                    .on_press_maybe(
                        (valid_name && validation_error.is_none()).then_some(Message::SavePreset),
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
    pub(super) fn presets_panel<'a>(
        &'a self,
        s: &'a crate::application::SettingsEditor,
    ) -> Element<'a, Message> {
        let processors = cpu_allocation::logical_processors();
        let mut rail = column![super::widgets::heading(
            t!("cpu_allocation.core_presets").to_string(),
            design::typography::SECONDARY
        ),]
        .spacing(design::space::MEDIUM)
        .padding([0, design::space::MEDIUM as u16]);
        rail = rail
            .push(text(t!("cpu_allocation.cache_domains_help").to_string()).style(text::secondary));
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
                    icon_button("icons/info.svg").on_press(message)
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
                    super::widgets::tooltip(
                        icon_button("icons/pencil.svg").on_press(Message::EditPreset(i)),
                        text(t!("common.edit").to_string()),
                        iced::widget::tooltip::Position::Top
                    ),
                    super::widgets::rule_delete_button(Some(Message::DeletePreset(i)))
                ]
                .align_y(iced::Center)
                .spacing(design::space::TIGHT),
            );
        }
        column![
            scrollable(rail).width(Fill).height(Fill),
            super::widgets::preset_footer(
                t!("cpu_allocation.add_preset").to_string(),
                Message::NewPreset
            ),
        ]
        .spacing(design::space::MEDIUM)
        .height(Fill)
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
pub(super) fn core_presets(
    processors: &[LogicalProcessorInfo],
) -> Vec<super::widgets::Choice<u64>> {
    let all = cpu_allocation::logical_processor_mask(processors);
    let p =
        cpu_allocation::logical_processor_kind_mask(processors, LogicalProcessorKind::Performance);
    let e =
        cpu_allocation::logical_processor_kind_mask(processors, LogicalProcessorKind::Efficiency);
    let smt = cpu_allocation::logical_processor_no_smt_mask(processors);
    let mut presets = [
        ("cpu_allocation.all", all),
        ("cpu_allocation.p_cores", p),
        ("cpu_allocation.e_cores", e),
        ("cpu_allocation.all_cores_no_smt", smt),
        ("cpu_allocation.p_cores_no_smt", p & smt),
        ("cpu_allocation.e_cores_no_smt", e & smt),
    ]
    .into_iter()
    .map(|(key, mask)| super::widgets::Choice(mask, t!(key).to_string()))
    .collect::<Vec<_>>();
    if let Ok(domains) = cpu_allocation::cache_domain_masks() {
        presets.extend(cache_presets(all, &domains));
    }
    presets
}

fn cache_presets(
    available: u64,
    domains: &[cpu_allocation::CacheDomainMask],
) -> Vec<super::widgets::Choice<u64>> {
    domains
        .iter()
        .filter(|domain| {
            domain.group == 0 && domain.mask != 0 && mask_is_available(domain.mask, available)
        })
        .map(|domain| {
            let size = if domain.size_bytes >= 1_000_000 {
                format!("{:.1} MB", f64::from(domain.size_bytes) / 1_000_000.0)
            } else {
                format!("{:.1} KB", f64::from(domain.size_bytes) / 1_000.0)
            };
            super::widgets::Choice(
                domain.mask,
                t!(
                    "cpu_allocation.cache_domain",
                    index = domain.index,
                    group = domain.group,
                    size = size
                )
                .to_string(),
            )
        })
        .collect()
}

pub(super) fn mask_is_available(mask: u64, available: u64) -> bool {
    mask & !available == 0
}

pub(super) fn mask_selector<'a, M: Clone + 'a>(
    mask: u64,
    processors: &[LogicalProcessorInfo],
    core_presets: &[super::widgets::Choice<u64>],
    custom: &[CpuAllocationPreset],
    message: impl Fn(u64) -> M + Clone + 'a,
) -> Element<'a, M> {
    let all = cpu_allocation::logical_processor_mask(processors);
    let mut presets = row![].spacing(design::space::TIGHT);
    for preset in core_presets {
        presets = presets.push(button(text(preset.1.clone())).on_press_maybe(
            (preset.0 != 0 && mask_is_available(preset.0, all)).then(|| message(preset.0)),
        ));
    }
    for preset in custom {
        let m = preset.core_mask;
        presets = presets.push(
            button(text(preset.name.clone()))
                .on_press_maybe((m != 0 && mask_is_available(m, all)).then(|| message(m))),
        );
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
    let mut selector = column![presets.wrap(), cpus.wrap(), text(format!("0x{mask:016X}"))]
        .spacing(design::space::SMALL);
    if !mask_is_available(mask, all) {
        selector = selector
            .push(text(t!("cpu_allocation.selection_unavailable").to_string()).style(text::danger));
    }
    selector.into()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_choices_keep_complete_masks_and_decimal_sizes() {
        let domains = [
            cpu_allocation::CacheDomainMask {
                index: 1,
                group: 0,
                size_bytes: 32_000_000,
                mask: 0b0011,
            },
            cpu_allocation::CacheDomainMask {
                index: 2,
                group: 0,
                size_bytes: 512_000,
                mask: 0b1100,
            },
            cpu_allocation::CacheDomainMask {
                index: 3,
                group: 1,
                size_bytes: 32_000_000,
                mask: 0b0011,
            },
            cpu_allocation::CacheDomainMask {
                index: 4,
                group: 0,
                size_bytes: 32_000_000,
                mask: 0b10001,
            },
        ];
        let choices = cache_presets(0b1111, &domains);
        assert_eq!(
            choices.iter().map(|choice| choice.0).collect::<Vec<_>>(),
            [3, 12]
        );
        assert!(choices[0].1.contains("32.0 MB"));
        assert!(choices[1].1.contains("512.0 KB"));
    }

    #[test]
    fn preset_save_requires_captured_topology_and_rejects_absent_cpus() {
        use crate::cpu_topology::{
            CacheDomain, CacheKind, CpuTopology, LogicalProcessor, ProcessorGroup, ProcessorId,
        };

        let id = ProcessorId {
            group: 0,
            number: 0,
        };
        let topology = CpuTopology::new(
            vec![LogicalProcessor {
                id,
                core: id,
                efficiency_class: 0,
            }],
            vec![ProcessorGroup {
                number: 0,
                active_mask: 1,
            }],
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        let mut editor = Editor {
            preset_mask: 1,
            preset_topology: Some(Ok(topology.clone())),
            ..Editor::default()
        };
        assert!(editor
            .validate_preset_selection(&Ok(topology.clone()))
            .is_ok());
        editor.preset_mask = 3;
        assert!(editor
            .validate_preset_selection(&Ok(topology.clone()))
            .is_err());
        assert_eq!(editor.preset_mask, 3);
        editor.preset_mask = 1;
        let changed = CpuTopology::new(
            topology.processors().to_vec(),
            topology.groups().to_vec(),
            vec![CacheDomain {
                level: 3,
                kind: CacheKind::Unified,
                size_bytes: 32_000_000,
                processors: vec![id],
            }],
            topology.numa_nodes().to_vec(),
        )
        .unwrap();
        assert!(editor.validate_preset_selection(&Ok(changed)).is_err());
        assert!(editor
            .validate_preset_selection(&Err("unavailable".into()))
            .is_err());
        editor.preset_topology = Some(Err("unavailable at open".into()));
        assert!(editor.validate_preset_selection(&Ok(topology)).is_err());
    }

    #[test]
    fn opening_a_saved_preset_preserves_unavailable_bits_for_review() {
        let mut settings = Settings::default();
        settings.cpu_allocation_presets.push(CpuAllocationPreset {
            name: "saved".into(),
            core_mask: u64::MAX,
        });
        let mut editor = Editor::default();
        editor.update(&mut settings, Kind::Soft, Message::EditPreset(0));
        assert_eq!(editor.preset_mask, u64::MAX);
        assert_eq!(settings.cpu_allocation_presets[0].core_mask, u64::MAX);
        editor.update(&mut settings, Kind::Soft, Message::ClosePreset);
        assert_eq!(settings.cpu_allocation_presets[0].core_mask, u64::MAX);
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
