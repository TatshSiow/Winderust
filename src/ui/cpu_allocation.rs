use super::design;
use super::widgets::{action_button, button, checkbox, icon_button, text_input};
use crate::application::SettingsEditor;
use crate::config::{CpuAllocationPreset, CpuAllocationRule, CpuAllocationSettings, Settings};
use crate::cpu_allocation::{self, LogicalProcessorInfo, LogicalProcessorKind};
use crate::cpu_topology::{
    CacheKind, CpuSelection, CpuTopology, ProcessorId, SavedSelectionError, SelectionError,
};
use crate::foreground::process_list::normalize_path_slashes;
use crate::ui::process_rules::{
    can_add_exact_process_candidate, exact_path_input, picked_path_input,
};
use crate::ui::scrolling::scrollable;
use iced::widget::{column, row, text};
use iced::{Element, Fill};
use rust_i18n::t;
use std::{rc::Rc, sync::Arc};

#[derive(Debug, Clone, Copy)]
pub(super) enum Kind {
    Soft,
    Hard,
}
impl Kind {
    fn supports(self, id: ProcessorId) -> bool {
        matches!(self, Self::Soft) || id.group == 0
    }

    fn verify(
        self,
        selection: &CpuSelection,
        topology: &CpuTopology,
    ) -> Result<(), SavedSelectionError> {
        match self {
            Self::Soft => selection.resolve_processors(topology).map(|_| ()),
            Self::Hard => selection.resolve_group_zero(topology).map(|_| ()),
        }
    }
}
pub(super) fn settings(s: &Settings, k: Kind) -> &CpuAllocationSettings {
    match k {
        Kind::Soft => &s.cpu_sets_soft,
        Kind::Hard => &s.processor_affinity_hard,
    }
}
pub(super) fn settings_mut(s: &mut Settings, k: Kind) -> &mut CpuAllocationSettings {
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
    pub(super) fn selection(self, r: &CpuAllocationRule) -> &CpuSelection {
        match self {
            Self::Focus => &r.focus_selection,
            Self::Visible => &r.visible_window_selection,
            Self::Background => &r.background_selection,
        }
    }
    pub(super) fn set(self, r: &mut CpuAllocationRule, selection: CpuSelection) {
        match self {
            Self::Focus => r.focus_selection = selection,
            Self::Visible => r.visible_window_selection = selection,
            Self::Background => r.background_selection = selection,
        }
    }
}
#[derive(Default)]
pub(super) struct Editor {
    path: String,
    selected_presets:
        std::collections::BTreeMap<(String, usize), super::widgets::Choice<CpuSelection>>,
    preset_name: String,
    preset_selection: CpuSelection,
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
    Mask(usize, Tier, super::widgets::Choice<CpuSelection>),
    Remove(usize),

    NewPreset,
    ViewPreset(String, CpuSelection),
    EditPreset(usize),
    PresetName(String),
    PresetSelection(CpuSelection),
    TogglePresetProcessor(ProcessorId, Arc<CpuTopology>),
    SavePreset,
    DeletePreset(usize),
    ClosePreset,
}
impl Editor {
    pub(super) fn update(&mut self, s: &mut SettingsEditor, k: Kind, m: Message) {
        match m {
            Message::Enabled(v) => settings_mut(s, k).enabled = v,
            Message::Path(v) => self.path = v,
            Message::Browse => {}
            Message::Add => {
                let path = exact_path_input(&self.path);
                if can_add(s, path) {
                    let selection = match SelectionCatalog::capture(k).all_selection() {
                        Ok(selection) => selection,
                        Err(error) => {
                            self.error = error;
                            return;
                        }
                    };
                    settings_mut(s, k).rules.push(CpuAllocationRule {
                        enabled: true,
                        executable_path: normalize_path_slashes(path).into_owned(),
                        focus_selection: selection.clone(),
                        visible_window_selection: selection.clone(),
                        background_selection: selection,
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
                self.set_rule_selection(
                    s,
                    k,
                    i,
                    tier,
                    choice,
                    &cpu_allocation::processor_topology(),
                );
            }
            Message::Remove(i) => {
                let rules = &mut settings_mut(s, k).rules;
                if i < rules.len() {
                    let removed = rules.remove(i);
                    self.selected_presets
                        .retain(|(path, _), _| *path != removed.executable_path);
                }
            }

            Message::ViewPreset(name, selection) => {
                self.preset_open = true;
                self.preset_read_only = true;
                self.preset_name = name;
                self.preset_selection = selection;
                self.error.clear();
            }
            Message::NewPreset => {
                self.preset_read_only = false;
                self.preset_open = true;
                self.editing_preset = None;
                self.preset_name.clear();
                let catalog = SelectionCatalog::capture(k);
                self.preset_selection = catalog.all_selection().unwrap_or_default();
                self.preset_topology = Some(catalog.topology);
                self.error.clear();
            }
            Message::EditPreset(i) => {
                self.preset_read_only = false;
                if let Some(p) = s.global().cpu_allocation_presets.get(i) {
                    self.preset_open = true;
                    self.editing_preset = Some(i);
                    self.preset_name = p.name.clone();
                    self.preset_selection = p.selection.clone();
                    self.preset_topology = Some(cpu_allocation::processor_topology());
                    self.error.clear();
                }
            }
            Message::PresetName(v) if !self.preset_read_only => self.preset_name = v,
            Message::PresetSelection(selection) if !self.preset_read_only => {
                let topology = cpu_allocation::processor_topology();
                let selection =
                    verify_selection_against(&selection, &topology, k).map(|()| selection);
                self.stage_preset_selection(selection, topology);
            }
            Message::TogglePresetProcessor(id, rendered) if self.has_pending_editor() => {
                self.toggle_preset_processor(
                    id,
                    k,
                    &rendered,
                    cpu_allocation::processor_topology(),
                );
            }
            Message::PresetName(_)
            | Message::PresetSelection(_)
            | Message::TogglePresetProcessor(..) => {}
            Message::SavePreset => {
                if self.preset_read_only {
                    return;
                }
                if let Err(error) =
                    self.validate_preset_selection(&cpu_allocation::processor_topology(), k)
                {
                    self.error = error;
                    return;
                }
                let name = self.preset_name.trim();
                if !name.is_empty()
                    && !s
                        .global()
                        .cpu_allocation_presets
                        .iter()
                        .enumerate()
                        .any(|(i, p)| {
                            Some(i) != self.editing_preset
                                && p.name.trim().eq_ignore_ascii_case(name)
                        })
                {
                    let p = CpuAllocationPreset {
                        name: name.into(),
                        selection: self.preset_selection.clone(),
                    };
                    s.edit_global(|settings| {
                        if let Some(i) = self.editing_preset {
                            if let Some(old) = settings.cpu_allocation_presets.get_mut(i) {
                                *old = p;
                            }
                        } else {
                            settings.cpu_allocation_presets.push(p);
                        }
                    });
                    self.preset_open = false;
                    self.error.clear();
                }
            }
            Message::DeletePreset(i) => {
                if i < s.global().cpu_allocation_presets.len() {
                    s.edit_global(|settings| {
                        settings.cpu_allocation_presets.remove(i);
                    });
                    self.preset_open = false;
                }
            }
            Message::ClosePreset => {
                self.preset_open = false;
                self.error.clear();
            }
        }
    }

    fn set_rule_selection(
        &mut self,
        s: &mut Settings,
        kind: Kind,
        index: usize,
        tier: Tier,
        choice: super::widgets::Choice<CpuSelection>,
        topology: &Result<CpuTopology, String>,
    ) {
        if let Err(error) = verify_selection_against(&choice.0, topology, kind) {
            self.error = error;
            return;
        }
        if let Some(rule) = settings_mut(s, kind).rules.get_mut(index) {
            tier.set(rule, choice.0.clone());
            self.selected_presets
                .insert((rule.executable_path.clone(), tier as usize), choice);
            self.error.clear();
        }
    }

    fn stage_preset_selection(
        &mut self,
        selection: Result<CpuSelection, String>,
        topology: Result<CpuTopology, String>,
    ) {
        match selection {
            Ok(selection) => {
                self.preset_selection = selection;
                self.preset_topology = Some(topology);
                self.error.clear();
            }
            Err(error) => self.error = error,
        }
    }

    fn toggle_preset_processor(
        &mut self,
        id: ProcessorId,
        kind: Kind,
        rendered: &CpuTopology,
        current: Result<CpuTopology, String>,
    ) {
        let selection = current
            .as_ref()
            .map_err(|error| {
                t!("cpu_allocation.preset_topology_unavailable", error = error).to_string()
            })
            .and_then(|topology| {
                if topology != rendered {
                    return Err(t!("cpu_allocation.topology_changed").to_string());
                }
                toggle_processor(&self.preset_selection, id, kind, topology)
                    .map_err(saved_selection_error)
            });
        self.stage_preset_selection(selection, current);
    }

    fn validate_preset_selection(
        &self,
        current: &Result<crate::cpu_topology::CpuTopology, String>,
        kind: Kind,
    ) -> Result<(), String> {
        let unavailable = |error: &String| {
            t!("cpu_allocation.preset_topology_unavailable", error = error).to_string()
        };
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
        kind.verify(&self.preset_selection, current)
            .map_err(saved_selection_error)?;
        if self.preset_selection.is_empty() {
            return Err(t!("cpu_allocation.no_logical_cpus").to_string());
        }
        Ok(())
    }
    fn selected_preset(
        &self,
        path: &str,
        tier: Tier,
        selection: &CpuSelection,
        choices: &[super::widgets::Choice<CpuSelection>],
    ) -> Option<super::widgets::Choice<CpuSelection>> {
        self.selected_presets
            .get(&(path.to_owned(), tier as usize))
            .filter(|choice| &choice.0 == selection && choices.contains(choice))
            .or_else(|| choices.iter().find(|choice| &choice.0 == selection))
            .cloned()
    }

    pub(super) fn reset_drafts(&mut self) {
        *self = Self {
            selected_presets: std::mem::take(&mut self.selected_presets),
            ..Self::default()
        };
    }

    pub(super) fn preset_open(&self) -> bool {
        self.preset_open
    }

    pub(super) fn has_pending_editor(&self) -> bool {
        self.preset_open() && !self.preset_read_only
    }
    pub(super) fn view<'a>(
        &'a self,
        s: &'a Settings,
        k: Kind,
        candidates: &'a [super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        let feature = settings(s, k);
        let picker = super::app_picker::view_with_path_selection(
            &self.path,
            candidates,
            true,
            (Message::Path, |path| Message::Path(picked_path_input(path))),
            Message::Browse,
            can_add(s, exact_path_input(&self.path)).then_some(Message::Add),
            |path| can_add(s, path).then_some(true),
        );
        let mut rules = Vec::new();
        let mut rule_options: Option<(SelectionCatalog, Rc<[_]>)> = None;
        for (i, r) in feature.rules.iter().enumerate() {
            let (catalog, presets) = rule_options.get_or_insert_with(|| {
                let catalog = SelectionCatalog::capture(k);
                let mut choices = catalog.presets.clone();
                choices.extend(
                    s.cpu_allocation_presets
                        .iter()
                        .map(|p| super::widgets::Choice(p.selection.clone(), p.name.clone())),
                );
                choices.retain(|p| !p.0.is_empty() && catalog.error(&p.0).is_none());
                (catalog, choices.into())
            });
            let controls = [Tier::Focus, Tier::Visible, Tier::Background]
                .into_iter()
                .map(|tier| {
                    let selection = tier.selection(r);
                    let error = catalog.error(selection);
                    let (choices, selected) = if let Some(selected) =
                        self.selected_preset(&r.executable_path, tier, selection, presets)
                    {
                        (Rc::clone(presets), selected)
                    } else {
                        let selected = super::widgets::Choice(
                            selection.clone(),
                            t!(if error.is_some() {
                                "cpu_allocation.unavailable_selection"
                            } else {
                                "common.custom"
                            })
                            .to_string(),
                        );
                        let mut choices = presets.to_vec();
                        choices.push(selected.clone());
                        (choices.into(), selected)
                    };
                    let mut control =
                        column![
                            super::widgets::pick_list(choices, Some(selected), move |v| {
                                Message::Mask(i, tier, v)
                            })
                            .width(Fill)
                        ]
                        .spacing(design::space::TIGHT);
                    if let Some(error) = error {
                        control = control.push(text(error).style(text::danger));
                        control = control.push(text(selection_text(selection)));
                        if let Some(reviewed) = catalog.reviewed(selection) {
                            control = control.push(
                                button(text(t!("cpu_allocation.use_this_selection").to_string()))
                                    .on_press(Message::Mask(
                                        i,
                                        tier,
                                        super::widgets::Choice(
                                            reviewed,
                                            t!("common.custom").to_string(),
                                        ),
                                    )),
                            );
                        }
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
        let table = super::widgets::process_rules_table(
            [Tier::Focus, Tier::Visible, Tier::Background].map(|tier| tier.label()),
            rules,
            false,
        );
        let mut body = column![
            super::widgets::toggle_card(
                match k {
                    Kind::Soft => "cpu_sets_soft.enable",
                    Kind::Hard => "processor_affinity_hard.enable",
                },
                feature.enabled,
                Some(Message::Enabled),
            ),
            super::widgets::settings_section(
                "common.rules",
                column![
                    super::widgets::settings_card(
                        column![
                            super::widgets::help_text("cpu_allocation.rules_help"),
                            picker
                        ]
                        .spacing(design::space::SMALL),
                    ),
                    table,
                ]
                .spacing(super::widgets::CARD_GAP),
            ),
        ]
        .spacing(super::widgets::CARD_GAP);
        if !self.error.is_empty() {
            body = body.push(text(self.error.clone()).style(text::danger));
        }

        scrollable(body).width(Fill).height(Fill).into()
    }

    pub(super) fn modal<'a>(&'a self, s: &Settings, kind: Kind) -> Option<Element<'a, Message>> {
        if !self.preset_open {
            return None;
        }
        let title = if self.preset_read_only {
            self.preset_name.clone()
        } else {
            t!(if self.editing_preset.is_some() {
                "common.edit_preset"
            } else {
                "common.add_preset"
            })
            .to_string()
        };
        let header = row![
            text(title).width(Fill),
            icon_button("icons/x.svg").on_press(Message::ClosePreset)
        ]
        .align_y(iced::Center);
        let catalog = SelectionCatalog::capture(kind);
        let processors = &catalog.processors;
        let validation_error = if self.preset_read_only {
            catalog.error(&self.preset_selection)
        } else {
            self.validate_preset_selection(&catalog.topology, kind)
                .err()
        };
        let toggle_topology = catalog
            .topology
            .as_ref()
            .ok()
            .filter(|topology| {
                !self.preset_read_only && kind.verify(&self.preset_selection, topology).is_ok()
            })
            .cloned()
            .map(Arc::new);
        let mut cpus = row![].spacing(design::space::CONTROL);
        for (id, cpu) in processors {
            let selected = self.preset_selection.processors().binary_search(id).is_ok();
            cpus = cpus.push(processor_button(
                *id,
                processor_kind_label(cpu.kind),
                selected,
                toggle_topology
                    .as_ref()
                    .map(|topology| Message::TogglePresetProcessor(*id, Arc::clone(topology))),
            ));
        }
        let valid_name = !self.preset_name.trim().is_empty()
            && !s.cpu_allocation_presets.iter().enumerate().any(|(i, p)| {
                Some(i) != self.editing_preset
                    && p.name.trim().eq_ignore_ascii_case(self.preset_name.trim())
            });
        let name = (!self.preset_read_only).then(|| {
            let mut field = column![
                text(t!("cpu_allocation.preset_name").to_string()),
                text_input(
                    &t!("cpu_allocation.preset_name_placeholder"),
                    &self.preset_name
                )
                .on_input(Message::PresetName),
                text(t!("cpu_allocation.preset_name_help").to_string()).style(text::secondary)
            ]
            .spacing(design::space::SMALL);
            if !self.preset_name.trim().is_empty() && !valid_name {
                field = field.push(
                    text(t!("cpu_allocation.duplicate_preset_name").to_string())
                        .style(text::danger),
                );
            }
            field.into()
        });

        let mut selection = column![text(t!("cpu_allocation.selected_logical_cpus").to_string())]
            .spacing(design::space::SMALL);
        if let Ok(topology) = &catalog.topology {
            let cores = topology
                .processors()
                .iter()
                .map(|cpu| cpu.core)
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            selection = selection.push(
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
        if let Some(error) = &validation_error {
            selection = selection.push(text(error.clone()).style(text::danger));
        }
        if !self.error.is_empty() && validation_error.as_deref() != Some(self.error.as_str()) {
            selection = selection.push(text(self.error.clone()).style(text::danger));
        }
        if !self.preset_read_only {
            if catalog.error(&self.preset_selection).is_some() {
                if let Some(reviewed) = catalog.reviewed(&self.preset_selection) {
                    selection = selection.push(
                        button(text(t!("cpu_allocation.use_this_selection").to_string()))
                            .on_press(Message::PresetSelection(reviewed)),
                    );
                }
            }

            let mut choices = row![].spacing(design::space::TIGHT);
            for preset in &catalog.presets {
                choices = choices.push(
                    button(text(preset.1.clone()))
                        .on_press(Message::PresetSelection(preset.0.clone())),
                );
            }
            selection = selection.push(choices.wrap());
        }
        let selection = selection
            .push(cpus.wrap())
            .push(text(selection_text(&self.preset_selection)));
        let body = column![
            super::widgets::heading(
                t!("common.preset_details").to_string(),
                design::typography::BODY
            ),
            super::widgets::settings_card(super::widgets::settings_rows(
                name.into_iter().chain([selection.into()]),
            ))
        ]
        .spacing(design::space::MEDIUM);
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
        s: &'a SettingsEditor,
        kind: Kind,
    ) -> Element<'a, Message> {
        use super::widgets;
        let catalog = SelectionCatalog::capture(kind);
        let mut rail = widgets::preset_list()
            .push(widgets::heading(
                t!("cpu_allocation.core_presets").to_string(),
                design::typography::SECONDARY,
            ))
            .push(text(t!("cpu_allocation.cache_domains_help").to_string()).style(text::secondary));
        for widgets::Choice(selection, label) in catalog.presets {
            rail = rail.push(widgets::preset_row(
                label.clone(),
                Message::ViewPreset(label, selection),
                None,
            ));
        }
        rail = rail.push(widgets::heading(
            t!("common.custom_presets").to_string(),
            design::typography::SECONDARY,
        ));
        if s.cpu_allocation_presets.is_empty() {
            rail =
                rail.push(text(t!("common.no_custom_presets").to_string()).style(text::secondary));
        }
        for (i, p) in s.cpu_allocation_presets.iter().enumerate() {
            rail = rail.push(widgets::preset_row(
                p.name.as_str(),
                Message::ViewPreset(p.name.clone(), p.selection.clone()),
                Some((Message::EditPreset(i), Message::DeletePreset(i))),
            ));
        }
        widgets::preset_panel(
            rail,
            t!("common.add_preset").to_string(),
            Message::NewPreset,
        )
    }
}
fn can_add(s: &Settings, path: &str) -> bool {
    can_add_exact_process_candidate(
        path,
        |p| s.cpu_sets_soft.contains_rule_for(p) || s.processor_affinity_hard.contains_rule_for(p),
        cpu_allocation::is_builtin_excluded,
    )
}
pub(super) struct SelectionCatalog {
    topology: Result<CpuTopology, String>,
    kind: Kind,
    processors: Vec<(ProcessorId, LogicalProcessorInfo)>,
    presets: Vec<super::widgets::Choice<CpuSelection>>,
}
impl SelectionCatalog {
    pub(super) fn capture(kind: Kind) -> Self {
        Self::from_topology(cpu_allocation::processor_topology(), kind)
    }

    fn from_topology(topology: Result<CpuTopology, String>, kind: Kind) -> Self {
        let processors = topology
            .as_ref()
            .map(|topology| {
                cpu_allocation::logical_processors_for_topology(topology)
                    .into_iter()
                    .filter(|(id, _)| kind.supports(*id))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let presets = match topology
            .as_ref()
            .map(|topology| core_presets(topology, &processors))
        {
            Ok(Ok(presets)) => presets,
            Ok(Err(error)) => {
                return Self {
                    topology: Err(error.to_string()),
                    kind,
                    processors: Vec::new(),
                    presets: Vec::new(),
                }
            }
            Err(_) => Vec::new(),
        };
        Self {
            topology,
            kind,
            processors,
            presets,
        }
    }

    pub(super) fn all_selection(&self) -> Result<CpuSelection, String> {
        let topology = self.topology.as_ref().map_err(|error| {
            t!("cpu_allocation.topology_unavailable", error = error).to_string()
        })?;
        CpuSelection::bind(
            topology,
            self.processors.iter().map(|(id, _)| *id).collect(),
        )
        .map_err(|error| error.to_string())
    }

    pub(super) fn processor_count(&self) -> usize {
        self.processors.len()
    }

    pub(super) fn error(&self, selection: &CpuSelection) -> Option<String> {
        match &self.topology {
            Ok(_) => verify_selection_against(selection, &self.topology, self.kind).err(),
            Err(error) => {
                Some(t!("cpu_allocation.topology_unavailable", error = error).to_string())
            }
        }
    }

    fn reviewed(&self, selection: &CpuSelection) -> Option<CpuSelection> {
        let topology = self.topology.as_ref().ok()?;
        let rebound = selection.rebind(topology).ok()?;
        self.kind.verify(&rebound, topology).ok()?;
        Some(rebound)
    }

    fn toggled(&self, selection: &CpuSelection, id: ProcessorId) -> Option<CpuSelection> {
        toggle_processor(selection, id, self.kind, self.topology.as_ref().ok()?).ok()
    }
}

fn toggle_processor(
    selection: &CpuSelection,
    id: ProcessorId,
    kind: Kind,
    topology: &CpuTopology,
) -> Result<CpuSelection, SavedSelectionError> {
    kind.verify(selection, topology)?;
    if !kind.supports(id) {
        return Err(SelectionError::UnsupportedGroup.into());
    }
    let mut processors = selection.processors().to_vec();
    match processors.binary_search(&id) {
        Ok(index) => {
            processors.remove(index);
        }
        Err(index) => processors.insert(index, id),
    }
    CpuSelection::bind(topology, processors).map_err(Into::into)
}

pub(super) fn verify_selection(selection: &CpuSelection, kind: Kind) -> Result<(), String> {
    verify_selection_against(selection, &cpu_allocation::processor_topology(), kind)
}

pub(super) fn verify_selection_against(
    selection: &CpuSelection,
    topology: &Result<CpuTopology, String>,
    kind: Kind,
) -> Result<(), String> {
    let topology = topology
        .as_ref()
        .map_err(|error| t!("cpu_allocation.topology_unavailable", error = error).to_string())?;
    kind.verify(selection, topology)
        .map_err(saved_selection_error)
}

fn saved_selection_error(error: crate::cpu_topology::SavedSelectionError) -> String {
    let key = match error {
        SavedSelectionError::MissingTopology => "cpu_allocation.selection_missing_topology",
        SavedSelectionError::TopologyChanged => "cpu_allocation.selection_topology_changed",
        SavedSelectionError::Selection(SelectionError::Empty) => "cpu_allocation.no_logical_cpus",
        SavedSelectionError::Selection(SelectionError::Unknown) => {
            "cpu_allocation.selection_unavailable"
        }
        SavedSelectionError::Selection(SelectionError::UnsupportedGroup) => {
            "cpu_allocation.selection_group_unsupported"
        }
        SavedSelectionError::Selection(
            SelectionError::Duplicate | SelectionError::InvalidNumber,
        ) => "cpu_allocation.selection_invalid",
    };
    t!("cpu_allocation.selection_needs_review", reason = t!(key)).to_string()
}

pub(super) fn selection_text(selection: &CpuSelection) -> String {
    if selection.is_empty() {
        return t!("cpu_allocation.no_logical_cpus").to_string();
    }
    selection
        .processors()
        .iter()
        .map(|id| format!("G{}:CPU {}", id.group, id.number))
        .collect::<Vec<_>>()
        .join(", ")
}

fn core_presets(
    topology: &CpuTopology,
    processors: &[(ProcessorId, LogicalProcessorInfo)],
) -> Result<Vec<super::widgets::Choice<CpuSelection>>, crate::cpu_topology::SelectionError> {
    let all = processors.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    let of_kind = |kind| {
        processors
            .iter()
            .filter(|(_, cpu)| cpu.kind == kind)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>()
    };
    let p = of_kind(LogicalProcessorKind::Performance);
    let e = of_kind(LogicalProcessorKind::Efficiency);
    let mut cores = std::collections::BTreeSet::new();
    let smt = processors
        .iter()
        .filter(|(_, cpu)| cores.insert(cpu.core_index))
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    let without_smt = |members: &[ProcessorId]| {
        members
            .iter()
            .filter(|id| smt.contains(id))
            .copied()
            .collect::<Vec<_>>()
    };
    let p_smt = without_smt(&p);
    let e_smt = without_smt(&e);
    let mut presets = [
        ("cpu_allocation.all", all.clone()),
        ("cpu_allocation.p_cores", p),
        ("cpu_allocation.e_cores", e),
        ("cpu_allocation.all_cores_no_smt", smt),
        ("cpu_allocation.p_cores_no_smt", p_smt),
        ("cpu_allocation.e_cores_no_smt", e_smt),
    ]
    .into_iter()
    .filter(|(_, members)| !members.is_empty())
    .map(|(key, members)| {
        CpuSelection::bind(topology, members)
            .map(|selection| super::widgets::Choice(selection, t!(key).to_string()))
    })
    .collect::<Result<Vec<_>, _>>()?;
    let groups = all
        .iter()
        .map(|id| id.group)
        .collect::<std::collections::BTreeSet<_>>();
    if groups.len() > 1 {
        for group in groups {
            let members = all.iter().copied().filter(|id| id.group == group).collect();
            presets.push(super::widgets::Choice(
                CpuSelection::bind(topology, members)?,
                t!("cpu_allocation.processor_group_preset", group = group).into_owned(),
            ));
        }
    }
    presets.extend(cache_presets(topology, &all)?);
    Ok(presets)
}

fn cache_presets(
    topology: &CpuTopology,
    available: &[ProcessorId],
) -> Result<Vec<super::widgets::Choice<CpuSelection>>, crate::cpu_topology::SelectionError> {
    topology
        .caches()
        .iter()
        .filter(|domain| {
            domain.level == 3 && matches!(domain.kind, CacheKind::Unified | CacheKind::Data)
        })
        .enumerate()
        .filter(|(_, domain)| domain.processors.iter().all(|id| available.contains(id)))
        .map(|(index, domain)| {
            let size = if domain.size_bytes >= 1_000_000 {
                format!("{:.1} MB", f64::from(domain.size_bytes) / 1_000_000.0)
            } else {
                format!("{:.1} KB", f64::from(domain.size_bytes) / 1_000.0)
            };
            let groups = domain
                .processors
                .iter()
                .map(|id| id.group)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(|group| format!("G{group}"))
                .collect::<Vec<_>>()
                .join(" / ");
            Ok(super::widgets::Choice(
                CpuSelection::bind(topology, domain.processors.clone())?,
                t!(
                    "cpu_allocation.cache_domain",
                    index = index + 1,
                    groups = groups,
                    size = size
                )
                .to_string(),
            ))
        })
        .collect()
}

fn processor_kind_label(kind: LogicalProcessorKind) -> String {
    t!(match kind {
        LogicalProcessorKind::Performance => "cpu_allocation.p_core",
        LogicalProcessorKind::Efficiency => "cpu_allocation.e_core",
        LogicalProcessorKind::Standard => "cpu_allocation.core",
    })
    .into_owned()
}

fn processor_button<'a, M: Clone + 'a>(
    id: ProcessorId,
    caption: String,
    selected: bool,
    on_press: Option<M>,
) -> Element<'a, M> {
    button(
        iced::widget::container(
            column![
                text(caption).size(design::typography::CAPTION),
                text(format!("G{}:CPU {}", id.group, id.number))
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
    .on_press_maybe(on_press)
    .into()
}

#[derive(Clone, Copy)]
pub(super) enum ProcessorPickerLayout {
    Compact,
    Buttons,
}

pub(super) fn mask_selector<'a, M: Clone + 'a>(
    selection: &CpuSelection,
    catalog: &SelectionCatalog,
    custom: &[CpuAllocationPreset],
    layout: ProcessorPickerLayout,
    message: impl Fn(CpuSelection) -> M,
) -> Element<'a, M> {
    let mut presets = row![].spacing(design::space::TIGHT);
    for preset in &catalog.presets {
        presets = presets.push(button(text(preset.1.clone())).on_press(message(preset.0.clone())));
    }
    let mut unavailable = std::collections::BTreeMap::<_, Vec<&str>>::new();
    for preset in custom {
        let reason = catalog.error(&preset.selection).or_else(|| {
            preset
                .selection
                .is_empty()
                .then(|| t!("cpu_allocation.no_logical_cpus").into_owned())
        });
        if let Some(reason) = reason {
            unavailable.entry(reason).or_default().push(&preset.name);
        } else {
            presets = presets.push(
                button(text(preset.name.clone())).on_press(message(preset.selection.clone())),
            );
        }
    }
    let mut cpus = row![].spacing(design::space::CONTROL);
    for (id, cpu) in &catalog.processors {
        let selected = selection.processors().binary_search(id).is_ok();
        let toggle = catalog.toggled(selection, *id).map(&message);
        let core = format!("{} {}", processor_kind_label(cpu.kind), cpu.core_index);
        let control = match layout {
            ProcessorPickerLayout::Compact => checkbox(selected)
                .label(format!("G{}:CPU {} ({core})", id.group, id.number))
                .on_toggle_maybe(toggle.map(|message| move |_| message.clone()))
                .into(),
            ProcessorPickerLayout::Buttons => processor_button(*id, core, selected, toggle),
        };
        cpus = cpus.push(control);
    }
    let unavailable = unavailable.into_iter().map(|(reason, names)| {
        column![
            text(names.join(" \u{00b7} "))
                .size(design::typography::SECONDARY)
                .wrapping(text::Wrapping::WordOrGlyph),
            text(reason)
                .size(design::typography::SECONDARY)
                .style(text::secondary)
                .wrapping(text::Wrapping::WordOrGlyph),
        ]
        .spacing(design::space::TIGHT)
        .width(Fill)
        .into()
    });
    let mut selector = column![presets.wrap()]
        .extend(unavailable)
        .push(cpus.wrap())
        .push(text(selection_text(selection)))
        .spacing(design::space::SMALL);
    if let Some(error) = catalog.error(selection) {
        selector = selector.push(text(error).style(text::danger));
        if let Some(reviewed) = catalog.reviewed(selection) {
            selector = selector.push(
                button(text(t!("cpu_allocation.use_this_selection").to_string()))
                    .on_press(message(reviewed)),
            );
        }
    }
    selector.into()
}
#[cfg(test)]
mod tests {
    use super::*;

    fn multi_group_topology() -> CpuTopology {
        use crate::cpu_topology::{CacheDomain, LogicalProcessor, ProcessorGroup};
        let id = |group, number| ProcessorId { group, number };
        let zero = vec![id(0, 0), id(0, 1)];
        let one = vec![id(1, 0), id(1, 63)];
        let all = [zero.clone(), one.clone()].concat();
        let cache = |size_bytes, processors| CacheDomain {
            level: 3,
            kind: CacheKind::Unified,
            size_bytes,
            processors,
        };
        let mut instructions = cache(1_000_000, zero.clone());
        instructions.kind = CacheKind::Instruction;
        CpuTopology::new(
            all.iter()
                .map(|id| LogicalProcessor {
                    id: *id,
                    core: ProcessorId {
                        group: id.group,
                        number: 0,
                    },
                    efficiency_class: u8::from(id.group == 0),
                })
                .collect(),
            vec![
                ProcessorGroup {
                    number: 0,
                    active_mask: 3,
                },
                ProcessorGroup {
                    number: 1,
                    active_mask: 1 | (1_u64 << 63),
                },
            ],
            vec![cache(32_000_000, zero), cache(512_000, one), instructions],
            Vec::new(),
        )
        .unwrap()
    }

    #[test]
    fn cache_choices_keep_complete_group_members_and_decimal_sizes() {
        let topology = multi_group_topology();
        let all = topology
            .processors()
            .iter()
            .map(|cpu| cpu.id)
            .collect::<Vec<_>>();
        let choices = cache_presets(&topology, &all).unwrap();
        assert_eq!(choices.len(), 2);
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.0.processors())
                .collect::<Vec<_>>(),
            vec![&all[2..], &all[..2]]
        );
        assert!(choices[0].1.contains("512.0 KB"));
        assert!(choices[1].1.contains("32.0 MB"));
        let hard = cache_presets(&topology, &all[..2]).unwrap();
        assert_eq!(hard.len(), 1);
        assert_eq!(hard[0].0.processors(), &all[..2]);
        assert!(cache_presets(&topology, &all[..1]).unwrap().is_empty());
        let spanning = CpuTopology::new(
            topology.processors().to_vec(),
            topology.groups().to_vec(),
            vec![crate::cpu_topology::CacheDomain {
                level: 3,
                kind: CacheKind::Unified,
                size_bytes: 64_000_000,
                processors: all.clone(),
            }],
            Vec::new(),
        )
        .unwrap();
        let spanning_choices = cache_presets(&spanning, &all).unwrap();
        assert_eq!(spanning_choices.len(), 1);
        assert_eq!(spanning_choices[0].0.processors(), all);
        assert!(spanning_choices[0].1.contains("G0 / G1"));
        assert!(cache_presets(&spanning, &all[..2]).unwrap().is_empty());
    }

    #[test]
    fn soft_catalog_edits_all_groups_while_hard_catalog_preserves_group_zero_limits() {
        let topology = multi_group_topology();
        let soft = SelectionCatalog::from_topology(Ok(topology.clone()), Kind::Soft);
        let hard = SelectionCatalog::from_topology(Ok(topology), Kind::Hard);
        let all = soft.all_selection().unwrap();
        let zero = hard.all_selection().unwrap();
        assert_eq!(all.processors().len(), 4);
        assert_eq!(zero.processors().len(), 2);
        assert!(soft.error(&all).is_none());
        assert!(hard.error(&all).is_some());
        for group in [0, 1] {
            let label = t!("cpu_allocation.processor_group_preset", group = group);
            let preset = soft
                .presets
                .iter()
                .find(|preset| preset.1 == label)
                .unwrap();
            assert_eq!(
                preset.0.processors(),
                all.processors()
                    .iter()
                    .copied()
                    .filter(|id| id.group == group)
                    .collect::<Vec<_>>()
            );
            assert!(!hard.presets.iter().any(|preset| preset.1 == label));
        }
        let group_one = ProcessorId {
            group: 1,
            number: 0,
        };
        let toggled = soft.toggled(&zero, group_one).unwrap();
        assert_eq!(toggled.processors().len(), 3);
        assert!(toggled.processors().contains(&ProcessorId {
            group: 0,
            number: 0
        }));
        assert!(toggled.processors().contains(&group_one));
        assert!(hard.toggled(&zero, group_one).is_none());
        assert!(hard.reviewed(&all).is_none());
        assert_eq!(soft.reviewed(&all), Some(all));
        assert_eq!(
            soft.presets[3].0.processors(),
            &[
                ProcessorId {
                    group: 0,
                    number: 0
                },
                ProcessorId {
                    group: 1,
                    number: 0
                },
            ]
        );
        assert!(hard.presets.iter().all(|preset| preset
            .0
            .processors()
            .iter()
            .all(|id| id.group == 0)));
    }

    fn rule(selection: CpuSelection) -> CpuAllocationRule {
        CpuAllocationRule {
            enabled: true,
            executable_path: "app.exe".into(),
            focus_selection: selection.clone(),
            visible_window_selection: selection.clone(),
            background_selection: selection,
        }
    }

    fn changed_topology() -> CpuTopology {
        let original = crate::cpu_topology::test_topology();
        CpuTopology::new(
            original.processors().to_vec(),
            original.groups().to_vec(),
            vec![crate::cpu_topology::CacheDomain {
                level: 3,
                kind: crate::cpu_topology::CacheKind::Unified,
                size_bytes: 32_000_000,
                processors: original.processors().iter().map(|cpu| cpu.id).collect(),
            }],
            original.numa_nodes().to_vec(),
        )
        .unwrap()
    }

    #[test]
    fn preset_save_requires_captured_topology_and_every_saved_member() {
        use crate::cpu_topology::{LogicalProcessor, ProcessorGroup, ProcessorId};
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
            preset_selection: CpuSelection::from_group_zero_mask(&topology, 1).unwrap(),
            preset_topology: Some(Ok(topology.clone())),
            ..Editor::default()
        };
        assert!(editor
            .validate_preset_selection(&Ok(topology.clone()), Kind::Hard)
            .is_ok());
        editor.preset_selection = crate::cpu_topology::test_selection(3);
        let saved = editor.preset_selection.clone();
        assert!(editor
            .validate_preset_selection(&Ok(topology.clone()), Kind::Hard)
            .is_err());
        assert_eq!(editor.preset_selection, saved);
        assert!(editor
            .validate_preset_selection(&Err("unavailable".into()), Kind::Hard)
            .is_err());
        editor.preset_topology = Some(Err("unavailable at open".into()));
        assert!(editor
            .validate_preset_selection(&Ok(topology), Kind::Hard)
            .is_err());
    }

    #[test]
    fn same_members_on_changed_topology_require_explicit_review() {
        let saved = crate::cpu_topology::test_selection(3);
        let first = saved.processors()[0];
        for kind in [Kind::Soft, Kind::Hard] {
            let catalog = SelectionCatalog::from_topology(Ok(changed_topology()), kind);
            assert!(catalog.error(&saved).is_some());
            assert!(catalog.toggled(&saved, first).is_none());
            let reviewed = catalog.reviewed(&saved).unwrap();
            assert_eq!(saved.processors(), reviewed.processors());
            assert_ne!(saved.topology(), reviewed.topology());
            assert!(catalog.error(&reviewed).is_none());
            let toggled = catalog.toggled(&reviewed, first).unwrap();
            assert_eq!(toggled.processors(), &saved.processors()[1..]);
            assert_eq!(toggled.topology(), reviewed.topology());
        }
        assert_eq!(saved, crate::cpu_topology::test_selection(3));
        assert!(
            SelectionCatalog::from_topology(Err("unavailable".into()), Kind::Hard)
                .reviewed(&saved)
                .is_none()
        );
    }

    #[test]
    fn preset_processor_click_rejects_stale_layout_and_can_recover_an_empty_selection() {
        let original = crate::cpu_topology::test_topology();
        let current = changed_topology();
        let id = original.processors()[0].id;
        let mut editor = Editor {
            preset_open: true,
            preset_topology: Some(Ok(original.clone())),
            ..Editor::default()
        };

        editor.toggle_preset_processor(id, Kind::Soft, &original, Ok(current.clone()));
        assert_eq!(editor.preset_selection, CpuSelection::default());
        assert_eq!(editor.preset_topology, Some(Ok(original)));
        assert_eq!(editor.error, t!("cpu_allocation.topology_changed"));

        editor.toggle_preset_processor(id, Kind::Soft, &current, Ok(current.clone()));
        assert_eq!(editor.preset_selection.processors(), &[id]);
        assert!(editor
            .validate_preset_selection(&Ok(current.clone()), Kind::Soft)
            .is_ok());

        editor.toggle_preset_processor(id, Kind::Soft, &current, Ok(current.clone()));
        assert!(editor.preset_selection.is_empty());
        assert!(editor
            .validate_preset_selection(&Ok(current.clone()), Kind::Soft)
            .is_err());

        let missing = ProcessorId {
            group: 1,
            number: 0,
        };
        editor.toggle_preset_processor(missing, Kind::Soft, &current, Ok(current.clone()));
        assert_eq!(
            editor.error,
            saved_selection_error(SelectionError::Unknown.into())
        );
        assert!(editor.preset_selection.is_empty());

        editor.toggle_preset_processor(id, Kind::Soft, &current, Ok(current.clone()));
        assert_eq!(editor.preset_selection.processors(), &[id]);
        assert!(editor.error.is_empty());
    }

    #[test]
    fn review_never_drops_missing_or_nonzero_group_members() {
        use crate::cpu_topology::{LogicalProcessor, ProcessorGroup, ProcessorId};
        let zero = ProcessorId {
            group: 0,
            number: 0,
        };
        let one = ProcessorId {
            group: 1,
            number: 0,
        };
        let topology = CpuTopology::new(
            vec![
                LogicalProcessor {
                    id: zero,
                    core: zero,
                    efficiency_class: 0,
                },
                LogicalProcessor {
                    id: one,
                    core: one,
                    efficiency_class: 0,
                },
            ],
            vec![
                ProcessorGroup {
                    number: 0,
                    active_mask: 1,
                },
                ProcessorGroup {
                    number: 1,
                    active_mask: 1,
                },
            ],
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        let unsupported = CpuSelection::bind(&topology, vec![zero, one]).unwrap();
        let catalog = SelectionCatalog::from_topology(Ok(topology), Kind::Hard);
        assert!(catalog
            .reviewed(&crate::cpu_topology::test_selection(3))
            .is_none());
        assert!(catalog.reviewed(&unsupported).is_none());
        assert!(catalog.toggled(&unsupported, zero).is_none());
        assert_eq!(unsupported.processors(), &[zero, one]);
    }

    #[test]
    fn opening_a_saved_preset_preserves_its_binding_and_members() {
        let mut settings = Settings::default();
        let selection = crate::cpu_topology::test_selection(u64::MAX);
        settings.cpu_allocation_presets.push(CpuAllocationPreset {
            name: "saved".into(),
            selection: selection.clone(),
        });
        let mut settings = SettingsEditor::with_settings(settings);
        let mut editor = Editor::default();
        editor.update(
            &mut settings,
            Kind::Hard,
            Message::ViewPreset("saved".into(), selection.clone()),
        );
        assert_eq!(editor.preset_selection, selection);
        assert!(!editor.has_pending_editor());
        editor.update(&mut settings, Kind::Hard, Message::ClosePreset);
        editor.update(&mut settings, Kind::Soft, Message::EditPreset(0));
        assert_eq!(editor.preset_selection, selection);
        assert!(editor.has_pending_editor());
        assert_eq!(settings.cpu_allocation_presets[0].selection, selection);
        editor.update(&mut settings, Kind::Soft, Message::ClosePreset);
        assert_eq!(settings.cpu_allocation_presets[0].selection, selection);
    }

    #[test]
    fn rule_copies_preserve_bindings_and_reject_a_stale_catalog() {
        let topology = crate::cpu_topology::test_topology();
        let selection = crate::cpu_topology::test_selection(3);
        for kind in [Kind::Soft, Kind::Hard] {
            let mut editor = Editor::default();
            let mut settings = Settings::default();
            settings_mut(&mut settings, kind)
                .rules
                .push(rule(selection.clone()));
            let options = vec![
                super::super::widgets::Choice(selection.clone(), "All".into()),
                super::super::widgets::Choice(selection.clone(), "Mine".into()),
            ];
            editor.set_rule_selection(
                &mut settings,
                kind,
                0,
                Tier::Focus,
                options[1].clone(),
                &Ok(topology.clone()),
            );
            assert_eq!(
                editor.selected_preset("app.exe", Tier::Focus, &selection, &options),
                Some(options[1].clone())
            );
            assert_eq!(
                editor.selected_preset("app.exe", Tier::Background, &selection, &options),
                Some(options[0].clone())
            );
            editor.reset_drafts();
            assert_eq!(
                editor.selected_preset("app.exe", Tier::Focus, &selection, &options),
                Some(options[1].clone())
            );
            let before = settings.clone();
            editor.set_rule_selection(
                &mut settings,
                kind,
                0,
                Tier::Focus,
                options[0].clone(),
                &Ok(changed_topology()),
            );
            assert_eq!(settings, before);
            assert!(!editor.error.is_empty());
            assert_eq!(
                super::settings(&before, kind).rules[0].focus_selection,
                selection
            );
            let mut settings = SettingsEditor::with_settings(settings);
            editor.update(&mut settings, kind, Message::Remove(0));
            assert!(editor.selected_presets.is_empty());
        }
    }

    #[test]
    fn builtin_preview_is_read_only_and_cancel_discards_new_preset() {
        let mut editor = Editor::default();
        let before = Settings::default();
        let mut settings = SettingsEditor::with_settings(before.clone());
        settings.select_power_source(crate::config::PowerSourceProfile::OnBattery);
        let selection = crate::cpu_topology::test_selection(u64::MAX);
        editor.update(
            &mut settings,
            Kind::Soft,
            Message::ViewPreset("All".into(), selection.clone()),
        );
        editor.update(
            &mut settings,
            Kind::Soft,
            Message::PresetSelection(CpuSelection::default()),
        );
        editor.update(
            &mut settings,
            Kind::Soft,
            Message::TogglePresetProcessor(
                selection.processors()[0],
                Arc::new(crate::cpu_topology::test_topology()),
            ),
        );
        editor.update(
            &mut settings,
            Kind::Soft,
            Message::PresetName("Changed".into()),
        );
        editor.update(&mut settings, Kind::Soft, Message::SavePreset);
        assert_eq!(editor.preset_selection, selection);
        assert_eq!(editor.preset_name, "All");
        assert_eq!(settings.global(), &before);
        editor.update(&mut settings, Kind::Hard, Message::NewPreset);
        assert!(editor.has_pending_editor());
        editor.update(&mut settings, Kind::Hard, Message::ClosePreset);
        assert!(!editor.has_pending_editor());
        assert_eq!(settings.global(), &before);
    }

    #[test]
    fn preset_edits_do_not_rewrite_copied_rule_selections() {
        let mut settings = Settings::default();
        let selection = crate::cpu_topology::test_selection(1);
        settings.cpu_sets_soft.rules.push(rule(selection.clone()));
        settings.cpu_allocation_presets.push(CpuAllocationPreset {
            name: "Shared".into(),
            selection: selection.clone(),
        });
        let mut settings = SettingsEditor::with_settings(settings);
        settings.select_power_source(crate::config::PowerSourceProfile::OnBattery);
        Editor::default().update(&mut settings, Kind::Soft, Message::DeletePreset(0));
        assert_eq!(settings.cpu_sets_soft.rules[0].focus_selection, selection);
        assert!(settings.cpu_allocation_presets.is_empty());
        assert!(settings.global().on_battery.is_none());
    }
}
