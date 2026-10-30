//! Shared layout and typed bindings around Wind UI's built-in controls.

use std::{any::Any, cell::Cell, fmt::Display, rc::Rc};

use windui::{
    core::{EventCtx, Tree, Widget},
    prelude::{signal, Align, Element, Role, Signal},
};

use super::design;
use crate::power::PowerPlan;

/// Delivers presentation intents without coupling pages to the application owner.
/// The bool requests a structural refresh; text edits keep the current controls alive.
pub(super) struct Ui<M> {
    callback: Rc<dyn Fn(M, bool)>,
    plan_catalog: Option<Signal<Vec<PowerPlan>>>,
    candidate_catalog: Option<Signal<Vec<super::app_picker::Candidate>>>,
}

impl<M> Clone for Ui<M> {
    fn clone(&self) -> Self {
        Self {
            callback: Rc::clone(&self.callback),
            plan_catalog: self.plan_catalog,
            candidate_catalog: self.candidate_catalog,
        }
    }
}

impl<M: 'static> Ui<M> {
    pub(super) fn new(callback: impl Fn(M, bool) + 'static) -> Self {
        Self {
            callback: Rc::new(callback),
            plan_catalog: None,
            candidate_catalog: None,
        }
    }

    pub(super) fn map<N: 'static>(&self, map: impl Fn(N) -> M + 'static) -> Ui<N> {
        let parent = self.clone();
        Ui {
            callback: Rc::new(move |message, refresh| (parent.callback)(map(message), refresh)),
            plan_catalog: self.plan_catalog,
            candidate_catalog: self.candidate_catalog,
        }
    }

    pub(super) fn with_plan_catalog(mut self, catalog: Signal<Vec<PowerPlan>>) -> Self {
        self.plan_catalog = Some(catalog);
        self
    }

    pub(super) fn with_candidate_catalog(
        mut self,
        catalog: Signal<Vec<super::app_picker::Candidate>>,
    ) -> Self {
        self.candidate_catalog = Some(catalog);
        self
    }

    pub(super) fn candidates(
        &self,
        fallback: &[super::app_picker::Candidate],
    ) -> Vec<super::app_picker::Candidate> {
        self.candidate_catalog
            .filter(|catalog| catalog.is_alive())
            .map(|catalog| catalog.get())
            .unwrap_or_else(|| fallback.to_vec())
    }

    /// Observe typed intents while retaining the native presentation resources.
    pub(super) fn inspect(&self, inspect: impl Fn(&M, bool) + 'static) -> Self {
        let parent = self.clone();
        Self {
            callback: Rc::new(move |message, refresh| {
                inspect(&message, refresh);
                (parent.callback)(message, refresh);
            }),
            plan_catalog: self.plan_catalog,
            candidate_catalog: self.candidate_catalog,
        }
    }

    pub(super) fn emit(&self, message: M) {
        (self.callback)(message, true);
    }

    pub(super) fn edit(&self, message: M) {
        (self.callback)(message, false);
    }

    pub(super) fn button(&self, title: impl Into<String>, message: M) -> Element
    where
        M: Clone,
    {
        self.primary_button(title, message).neutral().outline_soft()
    }

    /// Reserve the native accent-filled variant for an explicit primary command.
    pub(super) fn primary_button(&self, title: impl Into<String>, message: M) -> Element
    where
        M: Clone,
    {
        let ui = self.clone();
        Element::button(title.into())
            .small()
            .font_weight(400)
            .on_click(move |ctx| {
                flush_bindings(ctx);
                ui.emit(message.clone());
            })
    }

    pub(super) fn input(
        &self,
        value: impl Into<String>,
        placeholder: impl Into<String>,
        make: impl Fn(String) -> M + 'static,
    ) -> Element {
        self.input_signal(signal(value.into()), placeholder, make)
    }

    /// Share a native text signal with validation labels and enabled_when guards.
    /// Derived signal maps must stay pure; the binding delivers application edits.
    pub(super) fn input_signal(
        &self,
        value: Signal<String>,
        placeholder: impl Into<String>,
        make: impl Fn(String) -> M + 'static,
    ) -> Element {
        let ui = self.clone();
        observe(
            Element::text_input(value, placeholder.into())
                .font_weight(400)
                .width_match(),
            value,
            move |value| ui.edit(make(value)),
        )
    }

    /// Native integer slider plus text input. Invalid raw text remains editable;
    /// only slider gestures snap to the configured step.
    pub(super) fn number(
        &self,
        raw: Signal<String>,
        min: u64,
        max: u64,
        step: u64,
        make: impl Fn(String) -> M + 'static,
    ) -> Element {
        debug_assert!(min <= max);
        let span = max.saturating_sub(min);
        let normalize = move |value: u64| {
            if span == 0 {
                0.0
            } else {
                (value.saturating_sub(min) as f64 / span as f64) as f32
            }
        };
        let initial = raw
            .get()
            .parse::<u64>()
            .ok()
            .filter(|value| (min..=max).contains(value))
            .unwrap_or(min);
        let position = signal(normalize(initial));
        let mirrored = Rc::new(Cell::new(position.get()));
        let slider_mirrored = Rc::clone(&mirrored);
        let slider = observe(
            Element::slider(position).width_match().enabled(span != 0),
            position,
            move |next| {
                if next != slider_mirrored.get() {
                    let steps = (f64::from(next.clamp(0.0, 1.0)) * span as f64
                        / step.max(1) as f64)
                        .round();
                    let value = min
                        .saturating_add((steps * step.max(1) as f64) as u64)
                        .min(max);
                    let snapped = normalize(value);
                    slider_mirrored.set(snapped);
                    position.set(snapped);
                    raw.set(value.to_string());
                }
            },
        )
        .weight(1.0);
        observe(
            row().children([
                slider,
                self.input_signal(raw, format!("{min}–{max}"), make)
                    .width(80),
            ]),
            raw,
            move |text| {
                if let Ok(value) = text.parse::<u64>() {
                    if (min..=max).contains(&value) {
                        let normalized = normalize(value);
                        mirrored.set(normalized);
                        if position.get() != normalized {
                            position.set(normalized);
                        }
                    }
                }
            },
        )
    }

    pub(super) fn toggle(&self, value: bool, make: impl Fn(bool) -> M + 'static) -> Element {
        let ui = self.clone();
        Element::switch(signal(value))
            .small()
            .on_switch_change(move |ctx, value| {
                flush_bindings(ctx);
                ui.emit(make(value));
            })
    }

    pub(super) fn checkbox(
        &self,
        title: impl Into<String>,
        value: bool,
        make: impl Fn(bool) -> M + 'static,
    ) -> Element {
        let value = signal(value);
        let ui = self.clone();
        Element::checkbox(title.into(), value)
            .font_weight(400)
            .on_toggle(move |ctx| {
                let next = !value.get();
                value.set(next);
                flush_bindings(ctx);
                ui.emit(make(next));
            })
    }

    pub(super) fn select<T>(
        &self,
        values: impl IntoIterator<Item = T>,
        selected: T,
        make: impl Fn(T) -> M + 'static,
    ) -> Element
    where
        T: Clone + PartialEq + Display + 'static,
    {
        let mut values: Vec<T> = values.into_iter().collect();
        let selected = match values.iter().position(|value| value == &selected) {
            Some(index) => index,
            None => {
                values.push(selected);
                values.len() - 1
            }
        };
        let labels: Vec<String> = values.iter().map(ToString::to_string).collect();
        let selected = signal(selected);
        let ui = self.clone();
        observe_action(
            Element::dropdown(labels, selected)
                .font_weight(400)
                .width_match(),
            selected,
            move |index| {
                if let Some(value) = values.get(index) {
                    ui.emit(make(value.clone()));
                } else {
                    eprintln!("Wind UI dropdown selected an invalid option index: {index}");
                }
            },
        )
    }

    pub(super) fn plan(
        &self,
        guid: Option<String>,
        plans: &[PowerPlan],
        make: impl Fn(Option<String>) -> M + 'static,
    ) -> Element {
        let ui = self.clone();
        let make = Rc::new(make);
        self.plan_catalog_host(plans, move |plans| {
            let make = Rc::clone(&make);
            ui.plan_snapshot(guid.clone(), &plans, true, move |guid| Some(make(guid)))
        })
    }

    /// A required target has no application-wide or None fallback.
    pub(super) fn required_plan(
        &self,
        guid: Option<String>,
        plans: &[PowerPlan],
        make: impl Fn(String) -> M + 'static,
    ) -> Element {
        let ui = self.clone();
        let make = Rc::new(make);
        self.plan_catalog_host(plans, move |plans| {
            let selected = guid.clone().or_else(|| {
                plans
                    .iter()
                    .find(|plan| plan.active)
                    .or_else(|| plans.first())
                    .map(|plan| plan.guid.clone())
            });
            let make = Rc::clone(&make);
            ui.plan_snapshot(selected, &plans, false, move |guid| {
                // The native placeholder is disabled when no target is available.
                guid.map(&*make)
            })
        })
    }

    /// Rebuild only catalog-dependent controls when asynchronous plans arrive.
    /// The surrounding editor and its native input/focus nodes stay alive.
    pub(super) fn plan_catalog_host(
        &self,
        fallback: &[PowerPlan],
        build: impl Fn(Vec<PowerPlan>) -> Element + 'static,
    ) -> Element {
        if let Some(catalog) = self.plan_catalog {
            Element::host_signal(catalog.map(|plans| vec![plans.clone()]), build).width_match()
        } else {
            build(fallback.to_vec())
        }
    }

    fn plan_snapshot(
        &self,
        guid: Option<String>,
        plans: &[PowerPlan],
        allow_none: bool,
        make: impl Fn(Option<String>) -> Option<M> + 'static,
    ) -> Element {
        let mut choices = Vec::new();
        if allow_none {
            choices.push(Choice(None, rust_i18n::t!("common.none").to_string()));
        }
        choices.extend(
            plans
                .iter()
                .map(|plan| Choice(Some(plan.guid.clone()), plan.display_name())),
        );
        let selected = choices
            .iter()
            .find(|choice| choice.0 == guid)
            .cloned()
            .unwrap_or_else(|| {
                Choice(
                    guid.clone(),
                    match guid {
                        Some(guid) => format!(
                            "{} ({guid})",
                            rust_i18n::t!("common.selected_plan_unavailable")
                        ),
                        None => rust_i18n::t!("common.no_power_plans_loaded").to_string(),
                    },
                )
            });
        let available = allow_none || !choices.is_empty();
        let ui = self.clone();
        let selected_index = choices
            .iter()
            .position(|choice| choice == &selected)
            .unwrap_or_else(|| {
                choices.push(selected);
                choices.len() - 1
            });
        let selected = signal(selected_index);
        observe_action(
            Element::dropdown(
                choices.iter().map(ToString::to_string).collect::<Vec<_>>(),
                selected,
            )
            .width_match(),
            selected,
            move |index| {
                if let Some(choice) = choices.get(index) {
                    if let Some(message) = make(choice.0.clone()) {
                        ui.emit(message);
                    }
                } else {
                    eprintln!(
                        "Wind UI power-plan dropdown selected an invalid option index: {index}"
                    );
                }
            },
        )
        .enabled(available)
    }

    pub(super) fn segmented<T>(
        &self,
        values: impl IntoIterator<Item = T>,
        selected: T,
        make: impl Fn(T) -> M + 'static,
    ) -> Element
    where
        T: Clone + PartialEq + Display + 'static,
    {
        let mut values: Vec<T> = values.into_iter().collect();
        let selected = match values.iter().position(|value| value == &selected) {
            Some(index) => index,
            None => {
                values.push(selected);
                values.len() - 1
            }
        };
        let labels: Vec<String> = values.iter().map(ToString::to_string).collect();
        let selected = signal(selected);
        let ui = self.clone();
        observe_phase(
            Element::segmented(labels, selected),
            selected,
            move |index| {
                if let Some(value) = values.get(index) {
                    ui.emit(make(value.clone()));
                } else {
                    eprintln!(
                        "Wind UI segmented control selected an invalid option index: {index}"
                    );
                }
            },
            BindingPhase::Action,
        )
    }

    pub(super) fn collapsible(
        &self,
        title: impl Into<String>,
        expanded: bool,
        body: Element,
        make: impl Fn(bool) -> M + 'static,
    ) -> Element {
        let expanded = signal(expanded);
        let ui = self.clone();
        observe(
            Element::collapsible(title.into(), expanded, body).width_match(),
            expanded,
            move |expanded| ui.edit(make(expanded)),
        )
    }
}

/// Nonvisual delivery only: native widgets continue to own input, focus and painting.
struct Binding<T> {
    value: Signal<T>,
    version: u64,
    previous: T,
    changed: Box<dyn Fn(T)>,
}

impl<T: Clone + PartialEq + 'static> Binding<T> {
    fn new(value: Signal<T>, changed: impl Fn(T) + 'static) -> Self {
        Self {
            version: value.version(),
            previous: value.get(),
            value,
            changed: Box::new(changed),
        }
    }

    fn poll(&mut self) {
        let version = self.value.version();
        if version == self.version {
            return;
        }
        self.version = version;
        let next = self.value.get();
        if next != self.previous {
            self.previous = next.clone();
            // Signal's read borrow has ended before application code runs.
            (self.changed)(next);
        }
    }
}

trait PendingBinding {
    fn pending(&self) -> bool;
    fn poll(&mut self);
}

impl<T: Clone + PartialEq + 'static> PendingBinding for Binding<T> {
    fn pending(&self) -> bool {
        self.value.version() != self.version
    }

    fn poll(&mut self) {
        Binding::poll(self);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BindingPhase {
    Edit,
    Action,
}

struct Observer {
    binding: Box<dyn PendingBinding>,
    phase: BindingPhase,
}

impl Widget for Observer {
    fn on_update(&mut self, ctx: &mut EventCtx) {
        if self.binding.pending() {
            if self.phase == BindingPhase::Action {
                flush_phase(ctx.tree_mut(), BindingPhase::Edit);
            }
            self.binding.poll();
        }
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn Any> {
        Some(self)
    }
}

/// Observe an edit without reconstructing its native input or dependent controls.
pub(super) fn observe<T: Clone + PartialEq + 'static>(
    control: Element,
    value: Signal<T>,
    changed: impl Fn(T) + 'static,
) -> Element {
    observe_phase(control, value, changed, BindingPhase::Edit).width_match()
}

/// Publish structural changes only after pending text/other edits in the same tree.
pub(super) fn observe_action<T: Clone + PartialEq + 'static>(
    control: Element,
    value: Signal<T>,
    changed: impl Fn(T) + 'static,
) -> Element {
    observe_phase(control, value, changed, BindingPhase::Action).width_match()
}

fn observe_phase<T: Clone + PartialEq + 'static>(
    control: Element,
    value: Signal<T>,
    changed: impl Fn(T) + 'static,
    phase: BindingPhase,
) -> Element {
    // A native control already owns its widget slot. Observe on a plain parent
    // instead of replacing that slot; enabling/disabling still inherits normally.
    Element::col()
        .widget(Observer {
            binding: Box::new(Binding::new(value, changed)),
            phase,
        })
        .reactive()
        .child(control)
}

/// Deliver current native values before Save, navigation or close disposes a tree.
/// Callbacks only enqueue typed application intents; no application state is borrowed.
pub(super) fn flush_bindings(ctx: &mut EventCtx) {
    let tree = ctx.tree_mut();
    flush_phase(tree, BindingPhase::Edit);
    flush_phase(tree, BindingPhase::Action);
}

fn flush_phase(tree: &mut Tree, phase: BindingPhase) {
    let Some(root) = tree.root else { return };
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        let Some(node) = tree.get_mut(id) else {
            continue;
        };
        pending.extend(node.children.iter().rev().copied());
        if let Some(observer) = node
            .widget
            .as_any_mut()
            .and_then(|value| value.downcast_mut::<Observer>())
        {
            if observer.phase == phase {
                observer.binding.poll();
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Choice<T>(pub T, pub String);

impl<T> Display for Choice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

pub(super) fn label(text: impl Into<String>) -> Element {
    Element::label(text.into())
        .font_size(design::typography::BODY as f32)
        .font_weight(400)
}

pub(super) fn heading(text: impl Into<String>) -> Element {
    label(text)
        .font_size(design::typography::SUBTITLE as f32)
        .font_weight(600)
        .width_match()
}

pub(super) fn col() -> Element {
    Element::col()
        .width_match()
        .spacing(design::space::MEDIUM as i32)
}

pub(super) fn row() -> Element {
    Element::row()
        .width_match()
        .cross(Align::Center)
        .spacing(design::space::SMALL as i32)
}

pub(super) fn card(
    title: impl Into<String>,
    children: impl IntoIterator<Item = Element>,
) -> Element {
    let title = title.into();
    let body = col().spacing(design::CARD_GAP as i32).children(children);
    let surface = if title.is_empty() {
        let theme = windui::theme::current();
        body.bg_role(Role::Surface)
            .corner(theme.card.corner(&theme.metrics))
            .padding(theme.card.pad())
    } else {
        Element::card(title, body)
    };
    surface.width_match().border_role(Role::Border, 1)
}

pub(super) fn setting_row(title: impl Into<String>, control: Element) -> Element {
    Element::setting_row(title.into(), control.max_width(240)).width_match()
}

pub(super) fn setting_row_with_help(
    title: impl Into<String>,
    help: impl Into<String>,
    control: Element,
) -> Element {
    let theme = windui::theme::current();
    let form = &theme.form;
    let mut title = Element::label(format!("{} ⓘ", title.into()))
        .font_size(form.label_size(&theme.metrics))
        .font_weight(form.label_weight())
        .fg_role(Role::Text)
        .width_match();
    if let Some(lines) = form.label_max_lines() {
        title = title.max_lines(lines).truncate(windui::ui::Truncate::End);
    }
    // Native setting_row hides its label slot; its full-title tooltip masks row help.
    Element::row()
        .width_match()
        .height(form.row_height())
        .cross(Align::Center)
        .spacing(form.gap())
        .child(
            Element::col()
                .weight(1.0)
                .spacing(2)
                .child(title.tooltip(help.into())),
        )
        .child(control)
}

pub(super) fn setting(key: &str, control: Element) -> Element {
    let title = rust_i18n::t!(key).to_string();
    let help_key = format!("{key}_help");
    let help = rust_i18n::t!(&help_key).to_string();
    if help == help_key {
        setting_row(title, control)
    } else {
        setting_row_with_help(title, help, control.max_width(240))
    }
}

pub(super) fn setting_title(key: &str) -> Element {
    let title = rust_i18n::t!(key).to_string();
    let help_key = format!("{key}_help");
    let help = rust_i18n::t!(&help_key).to_string();
    if help == help_key {
        label(title)
    } else {
        label(format!("{title} ⓘ")).tooltip(help)
    }
}

pub(super) fn icon_button<M: Clone + 'static>(
    ui: &Ui<M>,
    path: &'static str,
    tooltip: impl Into<String>,
    message: M,
) -> Element {
    let mut icon = windui::ui::ImageContent::from_svg_bytes(
        super::assets::svg(path).unwrap_or_else(|| unreachable!("UI action icons are embedded")),
        None,
    );
    icon.set_tint_role(Role::TextMuted);
    let ui = ui.clone();
    Element::icon_button_content(icon)
        .size(32, 32)
        .tooltip(tooltip.into())
        .on_click(move |ctx| {
            flush_bindings(ctx);
            ui.emit(message.clone());
        })
}

pub(super) fn rule_delete_button<M: Clone + 'static>(ui: &Ui<M>, remove: Option<M>) -> Element {
    let mut icon = windui::ui::ImageContent::from_svg_bytes(
        super::assets::svg("icons/trash-2.svg")
            .unwrap_or_else(|| unreachable!("UI action icons are embedded")),
        None,
    );
    icon.set_tint_role(if remove.is_some() {
        Role::Danger
    } else {
        Role::TextDisabled
    });
    let ui = ui.clone();
    let enabled = remove.is_some();
    Element::icon_button_content(icon)
        .size(32, 32)
        .enabled(enabled)
        .tooltip(rust_i18n::t!("common.remove").to_string())
        .on_click(move |ctx| {
            if let Some(message) = &remove {
                flush_bindings(ctx);
                ui.emit(message.clone());
            }
        })
}

pub(super) fn process_rule_row(
    path: &str,
    candidates: &[super::app_picker::Candidate],
    active: Element,
    controls: Vec<Element>,
    actions: Element,
) -> Vec<Element> {
    process_rule_header(path, candidates, active, None, controls, actions)
}

pub(super) fn process_rule_header(
    path: &str,
    candidates: &[super::app_picker::Candidate],
    active: Element,
    status: Option<Element>,
    controls: Vec<Element>,
    actions: Element,
) -> Vec<Element> {
    let mut cells = vec![rule_cell(active)];
    if let Some(status) = status {
        cells.push(rule_cell(status));
    }
    cells.push(rule_cell(
        super::app_picker::app_name(path, candidates).tooltip(path),
    ));
    cells.push(rule_cell(
        label(path)
            .fg_role(Role::TextMuted)
            .max_lines(1)
            .tooltip(path),
    ));
    cells.extend(controls.into_iter().map(rule_cell));
    cells.push(rule_cell(actions));
    cells
}

fn rule_cell(content: Element) -> Element {
    Element::row()
        .width_match()
        .height((design::SETTING_ROW_HEIGHT + 2 * design::CARD_PADDING) as i32)
        .cross(Align::Center)
        .padding(design::CARD_PADDING as i32)
        .child(content.width_match())
}

pub(super) fn process_rules_table(
    tiers: impl IntoIterator<Item = String>,
    rows: Vec<Vec<Element>>,
    empty: String,
) -> Element {
    process_rules_table_with_actions(tiers, rows, empty, 80, false)
}

pub(super) fn process_rules_table_with_actions(
    tiers: impl IntoIterator<Item = String>,
    rows: Vec<Vec<Element>>,
    empty: String,
    actions_width: u32,
    show_status: bool,
) -> Element {
    if rows.is_empty() {
        return label(empty)
            .fg_role(Role::TextMuted)
            .padding_xy(0, 8)
            .width_match();
    }
    let mut columns = vec![(rust_i18n::t!("common.enable").to_string(), 80.0)];
    if show_status {
        columns.push((rust_i18n::t!("common.status").to_string(), 140.0));
    }
    columns.extend([
        (rust_i18n::t!("process_list.app_name").to_string(), 160.0),
        (
            rust_i18n::t!("process_list.executable_path").to_string(),
            240.0,
        ),
    ]);
    columns.extend(tiers.into_iter().map(|title| (title, 160.0)));
    columns.push((
        rust_i18n::t!("common.actions").to_string(),
        actions_width as f32,
    ));
    assert!(
        rows.iter().all(|row| row.len() == columns.len()),
        "Process rule cells must match the native table columns"
    );
    let minimum_width = columns.iter().map(|(_, width)| *width as i32).sum();
    let row_height = design::SETTING_ROW_HEIGHT + 2 * design::CARD_PADDING + 1;
    let height = (39 + rows.len().min(6) as u32 * row_height) as i32;
    let table = Element::table_custom(columns, rows)
        // Horizontal Scroll measures unconstrained content; native weighted cells need a finite width.
        .width(minimum_width)
        .height(height);
    let body = Element::col().width_match().child(
        Element::scroll()
            .scroll_axis(windui::spec::ScrollAxis::Horizontal)
            .width_match()
            .height(height + 16)
            .child(table),
    );
    body.bg_role(Role::Surface)
        .corner(design::CARD_RADIUS)
        .border_role(Role::Border, 1)
}

pub(super) fn help(key: &str) -> Element {
    label(rust_i18n::t!(key).to_string())
        .font_size(design::typography::SECONDARY as f32)
        .fg_role(Role::TextMuted)
        .width_match()
}

pub(super) fn page(children: impl IntoIterator<Item = Element>) -> Element {
    super::scrolling::scrollable(
        Element::col()
            .width_match()
            .cross(Align::Center)
            .padding_xy(24, 16)
            .child(
                col()
                    .spacing(design::space::SECTION as i32)
                    .max_width(design::CONTENT_WIDTH as i32)
                    .children(children),
            ),
    )
}

pub(super) fn error(text: impl Into<String>) -> Element {
    label(text).fg_role(Role::Danger).width_match()
}

/// A native editor keeps its actions reachable while only the form body scrolls.
/// Wrap height lets short forms shrink; the cap leaves room for the title and
/// actions at the main window's minimum size.
pub(super) fn editor_panel(title: impl Into<String>, body: Element, footer: Element) -> Element {
    card(
        title,
        [
            Element::scroll()
                .width_match()
                .max_height(420)
                .child(body.width_match()),
            footer.width_match(),
        ],
    )
    .width_match()
}

/// Keep native Escape/outside dismissal in sync with application editor state.
pub(super) fn dialog<M: Clone + 'static>(
    show: Signal<bool>,
    content: Element,
    ui: Ui<M>,
    cancel: M,
) -> Element {
    let panel = content
        .max_width(860)
        .margin(20)
        .bg_role(Role::Surface)
        .corner(design::CARD_RADIUS);
    observe_action(
        Element::dialog(show, panel).weight(1.0),
        show,
        move |shown| {
            if !shown {
                ui.emit(cancel.clone());
            }
        },
    )
    .fill()
}

pub(super) fn label_with_unit(label: &str, unit: &str) -> String {
    if unit.is_empty() {
        label.to_owned()
    } else {
        format!("{label} ({unit})")
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use windui::{
        event::{Key, KeyEvent, MenuAction, MouseButton, PointerEvent, PointerKind},
        prelude::{Point, Size},
        signal::SignalScope,
        text::{LineAwareTextEngine, NullTextEngine},
    };

    use super::*;

    #[test]
    fn setting_help_survives_native_label_hit_testing_and_truncation_in_both_languages() {
        let original_theme = windui::theme::current();
        windui::theme::set_current(Rc::new(design::theme(false, None)));
        for (title, help) in [
            (
                "Foreground or system CPU threshold for monitoring pressure and protecting active applications",
                "Start pressure monitoring when either CPU measurement reaches this threshold.",
            ),
            (
                "前景應用程式或系統的處理器使用率門檻，用於監控負載並保護目前使用中的應用程式",
                "當前景應用程式或系統的處理器使用率達到此門檻時，開始監控負載。",
            ),
        ] {
            for (width, truncated) in [(1600, false), (240, true)] {
                let clicks = Rc::new(Cell::new(0));
                let received = Rc::clone(&clicks);
                let mut tree = Tree::new();
                let root = setting_row_with_help(
                    title,
                    help,
                    Element::button("Apply").width(80).on_click(move |_| {
                        received.set(received.get() + 1);
                    }),
                )
                .build(&mut tree);
                tree.root = Some(root);
                tree.layout_root(Size::new(width, 50), &mut LineAwareTextEngine);
                let left = tree.get(root).unwrap().children[0];
                let title = tree.get(left).unwrap().children[0];
                assert_eq!(tree.get(title).unwrap().widget.text_truncated(), Some(truncated));
                let bounds = tree.abs_bounds(title);
                let point = Point::new(bounds.x + bounds.w / 2, bounds.y + bounds.h / 2);
                let hit = tree.hit_test(point).unwrap();
                assert_eq!(hit, title);
                assert_eq!(tree.node_tooltip(hit).as_deref(), Some(help));

                let control = tree.get(root).unwrap().children[1];
                assert_eq!(tree.focusable_order(), vec![control]);
                tree.set_focused(Some(control), None);
                tree.dispatch_key(enter(), Some(control));
                assert_eq!(clicks.get(), 1);
                let bounds = tree.abs_bounds(control);
                let point = Point::new(bounds.x + bounds.w / 2, bounds.y + bounds.h / 2);
                assert_eq!(tree.hit_test(point), Some(control));
                let (mut hover, mut capture) = (None, None);
                for kind in [PointerKind::Down, PointerKind::Up] {
                    tree.dispatch_pointer(
                        PointerEvent::single(kind, point, MouseButton::Left),
                        &mut hover,
                        &mut capture,
                    );
                }
                assert_eq!(clicks.get(), 2);
                assert!(tree.get(control).unwrap().focused);
            }
        }
        windui::theme::set_current(original_theme);
    }

    #[test]
    fn setting_looks_up_help_and_keeps_missing_help_labels_and_control_width() {
        let original_theme = windui::theme::current();
        windui::theme::set_current(Rc::new(design::theme(false, None)));
        for (key, has_help) in [
            ("common.visible_window_detection", true),
            ("common.close", false),
        ] {
            let title = rust_i18n::t!(key).to_string();
            let help_key = format!("{key}_help");
            let help = rust_i18n::t!(&help_key).to_string();
            assert_eq!(help != help_key, has_help);
            let expected_title = if has_help {
                format!("{title} ⓘ")
            } else {
                title
            };
            let mut tree = Tree::new();
            let root = setting(key, Element::button("Apply").width(500)).build(&mut tree);
            tree.root = Some(root);
            tree.layout_root(Size::new(1000, 50), &mut LineAwareTextEngine);
            let left = tree.get(root).unwrap().children[0];
            let title = tree.get(left).unwrap().children[0];
            assert_eq!(
                tree.get(title).unwrap().widget.display_text().as_deref(),
                Some(expected_title.as_str())
            );
            let bounds = tree.abs_bounds(title);
            let point = Point::new(bounds.x + bounds.w / 2, bounds.y + bounds.h / 2);
            assert_eq!(tree.hit_test(point), Some(title));
            assert_eq!(tree.node_tooltip(title), has_help.then_some(help.clone()));
            let control = tree.get(root).unwrap().children[1];
            assert!(tree.abs_bounds(control).w <= 240);

            let mut tree = Tree::new();
            let root = setting_title(key).build(&mut tree);
            tree.root = Some(root);
            tree.layout_root(Size::new(1000, 50), &mut LineAwareTextEngine);
            assert_eq!(
                tree.get(root).unwrap().widget.display_text().as_deref(),
                Some(expected_title.as_str())
            );
            assert_eq!(tree.node_tooltip(root), has_help.then_some(help));
        }
        windui::theme::set_current(original_theme);
    }

    #[test]
    fn native_rule_table_keeps_a_single_row_fully_visible_without_vertical_scrolling() {
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let cells = process_rule_row(
                r"C:\Apps\sample.exe",
                &[],
                label(""),
                vec![label("Default"), label("Default"), label("Default")],
                label(""),
            );
            let mut tree = Tree::new();
            let root = process_rules_table(
                ["Focus".into(), "Visible window".into(), "Background".into()],
                vec![cells],
                "Empty".into(),
            )
            .build(&mut tree);
            tree.root = Some(root);
            tree.layout_root(Size::new(1200, 200), &mut NullTextEngine);
            let horizontal = tree.get(root).unwrap().children[0];
            let table = tree.get(horizontal).unwrap().children[0];
            let vertical = tree.get(table).unwrap().children[2];
            assert_eq!(
                tree.scroll_range(vertical, windui::spec::Axis::Vertical)
                    .unwrap()
                    .1,
                0,
            );
        });
    }

    #[test]
    fn native_rule_table_retains_controls_and_scrolls_both_axes_in_a_narrow_page() {
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let events = Rc::new(RefCell::new(Vec::new()));
            let received = Rc::clone(&events);
            let ui = Ui::new(move |message: bool, _| received.borrow_mut().push(message));
            let rows = (0..10)
                .map(|_| {
                    process_rule_row(
                        r"C:\Apps\sample.exe",
                        &[],
                        ui.checkbox("", false, std::convert::identity),
                        vec![
                            ui.select([false, true], false, std::convert::identity),
                            ui.select([false, true], false, std::convert::identity),
                            ui.select([false, true], false, std::convert::identity),
                        ],
                        rule_delete_button(&ui, Some(false)),
                    )
                })
                .collect();
            let mut tree = Tree::new();
            let root = process_rules_table(
                ["Focus".into(), "Visible window".into(), "Background".into()],
                rows,
                "Empty".into(),
            )
            .build(&mut tree);
            tree.root = Some(root);
            tree.layout_root(Size::new(520, 460), &mut NullTextEngine);
            let horizontal = tree.get(root).unwrap().children[0];
            assert!(
                tree.scroll_range(horizontal, windui::spec::Axis::Horizontal)
                    .unwrap()
                    .1
                    > 0
            );
            let table = tree.get(horizontal).unwrap().children[0];
            let vertical = tree.get(table).unwrap().children[2];
            assert!(
                tree.scroll_range(vertical, windui::spec::Axis::Vertical)
                    .unwrap()
                    .1
                    > 0
            );
            assert_eq!(tree.focusable_order().len(), 50);
            let checkbox = tree.focusable_order()[0];
            let bounds = tree.abs_bounds(checkbox);
            assert!(
                bounds.w > 0 && bounds.h > 0,
                "checkbox must have a usable hit target: {bounds:?}"
            );
            let point = Point::new(bounds.x + bounds.w / 2, bounds.y + bounds.h / 2);
            assert_eq!(tree.hit_test(point), Some(checkbox));
            let (mut hover, mut capture) = (None, None);
            for kind in [PointerKind::Down, PointerKind::Up] {
                tree.dispatch_pointer(
                    PointerEvent::single(kind, point, MouseButton::Left),
                    &mut hover,
                    &mut capture,
                );
            }
            assert_eq!(&*events.borrow(), &[true]);
        });
    }

    #[test]
    #[should_panic(expected = "Process rule cells must match the native table columns")]
    fn native_rule_table_rejects_misaligned_status_and_policy_columns() {
        let _ = process_rules_table(["Focus".into()], vec![vec![label("Incomplete")]], "".into());
    }

    #[test]
    fn native_segmented_header_actions_share_space_and_wrap_on_resize() {
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let ui = Ui::new(|_: (), _| {});
            let mut tree = Tree::new();
            let root = Element::flow()
                .width_match()
                .spacing(8)
                .run_spacing(8)
                .child(ui.segmented(["Plugged in", "On battery"], "Plugged in", |_| ()))
                .child(ui.button("Feature info", ()).small())
                .child(ui.button("Status", ()).small())
                .build(&mut tree);
            tree.root = Some(root);
            let children = tree.get(root).unwrap().children.clone();
            tree.layout_root(Size::new(900, 180), &mut NullTextEngine);
            let profile = tree.abs_bounds(children[0]);
            assert!(profile.w > 0 && profile.w < 900);
            for &action in &children[1..] {
                let bounds = tree.abs_bounds(action);
                assert!(
                    ((bounds.y + bounds.h / 2) - (profile.y + profile.h / 2)).abs() <= 1,
                    "wide header actions must share a row"
                );
                assert!(bounds.x >= profile.x + profile.w + 8);
            }

            // Resize the same native tree: profile state and binding identity stay intact.
            tree.layout_root(Size::new(profile.w, 180), &mut NullTextEngine);
            let narrow_profile = tree.abs_bounds(children[0]);
            let action = tree.abs_bounds(children[1]);
            assert_eq!(profile.w, narrow_profile.w);
            assert!(action.y >= narrow_profile.y + narrow_profile.h + 8);
            assert!(action.x + action.w <= profile.w);
        });
    }

    #[test]
    fn binding_delivers_edits_once_without_writing_back_or_initial_emission() {
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let draft = signal(String::from("10"));
            let events = Rc::new(RefCell::new(Vec::new()));
            let received = Rc::clone(&events);
            let mut binding = Binding::new(draft, move |value| received.borrow_mut().push(value));
            binding.poll();
            assert!(events.borrow().is_empty());
            draft.set(String::from("-"));
            binding.poll();
            binding.poll();
            assert_eq!(draft.get(), "-");
            assert_eq!(&*events.borrow(), &["-".to_owned()]);
            draft.set(String::from("-"));
            binding.poll();
            assert_eq!(events.borrow().len(), 1);
        });
    }

    #[test]
    fn mapped_ui_preserves_structural_refresh_and_edit_flags_without_clone_messages() {
        struct Message(u8);
        let events = Rc::new(RefCell::new(Vec::new()));
        let received = Rc::clone(&events);
        let ui = Ui::new(move |message: Message, refresh| {
            received.borrow_mut().push((message.0, refresh));
        });
        let mapped = ui.clone().map(Message);
        mapped.edit(1);
        mapped.emit(2);
        assert_eq!(&*events.borrow(), &[(1, false), (2, true)]);
    }

    #[test]
    fn native_save_delivers_the_last_edit_before_the_next_layout() {
        #[derive(Clone, Debug, PartialEq, Eq)]
        enum Message {
            Edit(String),
            Save,
        }
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let draft = signal(String::from("before"));
            let events = Rc::new(RefCell::new(Vec::new()));
            let received = Rc::clone(&events);
            let ui = Ui::new(move |message, _| received.borrow_mut().push(message));
            let mut tree = Tree::new();
            let root = col()
                .child(ui.input_signal(draft, "", Message::Edit))
                .child(ui.button("Save", Message::Save).height(40))
                .build(&mut tree);
            tree.root = Some(root);
            tree.layout_root(Size::new(320, 160), &mut NullTextEngine);
            let button = tree.get(root).unwrap().children[1];
            let bounds = tree.abs_bounds(button);
            let position = Point::new(bounds.x + bounds.w / 2, bounds.y + bounds.h / 2);
            // A native text signal can change after its update pass. Save must
            // publish this edit even if its click arrives before the next paint.
            draft.set(String::from("last keystroke"));
            let (mut hover, mut capture) = (None, None);
            for kind in [PointerKind::Down, PointerKind::Up] {
                tree.dispatch_pointer(
                    PointerEvent::single(kind, position, MouseButton::Left),
                    &mut hover,
                    &mut capture,
                );
            }
            assert_eq!(
                &*events.borrow(),
                &[Message::Edit(String::from("last keystroke")), Message::Save]
            );
            tree.layout_root(Size::new(320, 160), &mut NullTextEngine);
            assert_eq!(events.borrow().len(), 2, "flushed edits are not duplicated");
        });
    }

    #[test]
    fn earlier_structural_observer_flushes_later_text_edits_first() {
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let selected = signal(0_usize);
            let draft = signal(String::from("old profile"));
            let events = Rc::new(RefCell::new(Vec::new()));
            let received = Rc::clone(&events);
            let action = observe_action(
                Element::segmented(vec![String::from("A"), String::from("B")], selected),
                selected,
                move |index| received.borrow_mut().push(format!("select {index}")),
            );
            let received = Rc::clone(&events);
            let input = observe(Element::text_input(draft, ""), draft, move |text| {
                received.borrow_mut().push(text);
            });
            let mut tree = Tree::new();
            let root = col().children([action, input]).build(&mut tree);
            tree.root = Some(root);
            tree.layout_root(Size::new(320, 160), &mut NullTextEngine);
            draft.set(String::from("edit A"));
            selected.set(1);
            tree.layout_root(Size::new(320, 160), &mut NullTextEngine);
            assert_eq!(&*events.borrow(), &["edit A", "select 1"]);
        });
    }

    fn enter() -> KeyEvent {
        KeyEvent {
            key: Key::Enter,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        }
    }

    #[test]
    fn asynchronous_plan_choices_preserve_input_and_selected_guid_without_emitting() {
        #[derive(Clone, Debug, PartialEq, Eq)]
        enum Message {
            Edit(String),
            Plan(Option<String>),
        }
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let catalog = signal(Vec::new());
            let raw = signal(String::from("draft"));
            let events = Rc::new(RefCell::new(Vec::new()));
            let received = Rc::clone(&events);
            let ui = Ui::new(move |message, _| received.borrow_mut().push(message))
                .with_plan_catalog(catalog)
                .map(|message| message)
                .inspect(|_, _| {});
            let mut tree = Tree::new();
            let root = col()
                .children([
                    ui.input_signal(raw, "", Message::Edit),
                    ui.plan(Some(String::from("missing")), &[], Message::Plan),
                ])
                .build(&mut tree);
            tree.root = Some(root);
            tree.layout_root(Size::new(500, 160), &mut NullTextEngine);
            let input = tree.focusable_order()[0];
            let dropdown = tree.focusable_order()[1];
            let menu = tree.dispatch_key(enter(), Some(dropdown)).menu.unwrap();
            assert!(menu
                .items
                .iter()
                .any(|item| item.checked && item.label.contains("missing")));
            tree.set_focused(Some(input), None);
            raw.set(String::from("last keystroke"));
            catalog.set(vec![
                PowerPlan {
                    guid: String::from("b"),
                    name: String::from("Beta"),
                    active: true,
                },
                PowerPlan {
                    guid: String::from("missing"),
                    name: String::from("Recovered"),
                    active: false,
                },
            ]);
            tree.layout_root(Size::new(500, 160), &mut NullTextEngine);
            assert!(tree.get(input).unwrap().focused);
            assert!(tree.get(input).unwrap().widget.accepts_text());
            assert_eq!(raw.get(), "last keystroke");
            assert_eq!(
                &*events.borrow(),
                &[Message::Edit(String::from("last keystroke"))]
            );
            let dropdown = tree.focusable_order()[1];
            let menu = tree.dispatch_key(enter(), Some(dropdown)).menu.unwrap();
            assert!(menu
                .items
                .iter()
                .any(|item| item.checked && item.label.contains("Recovered")));
            let beta = menu
                .items
                .into_iter()
                .find(|item| item.label.contains("Beta"))
                .unwrap();
            let MenuAction::Run(select) = beta.action else {
                panic!("native dropdown item must be runnable")
            };
            windui::testing::run_with_ctx_in(&mut tree, dropdown, move |ctx| select(ctx));
            tree.layout_root(Size::new(500, 160), &mut NullTextEngine);
            assert_eq!(
                &*events.borrow(),
                &[
                    Message::Edit(String::from("last keystroke")),
                    Message::Plan(Some(String::from("b")))
                ]
            );
        });
    }

    #[test]
    fn native_number_preserves_invalid_and_unsnapped_text_but_snaps_slider_gestures() {
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let raw = signal(String::from("10"));
            let events = Rc::new(RefCell::new(Vec::new()));
            let received = Rc::clone(&events);
            let ui = Ui::new(move |value: String, _| received.borrow_mut().push(value));
            let mut tree = Tree::new();
            let root = ui.number(raw, 0, 60, 5, |value| value).build(&mut tree);
            tree.root = Some(root);
            tree.layout_root(Size::new(400, 50), &mut NullTextEngine);
            for text in ["-", "17"] {
                raw.set(String::from(text));
                tree.layout_root(Size::new(400, 50), &mut NullTextEngine);
                assert_eq!(raw.get(), text);
            }
            assert_eq!(&*events.borrow(), &[String::from("-"), String::from("17")]);
            let slider = tree
                .focusable_order()
                .into_iter()
                .find(|id| !tree.get(*id).unwrap().widget.accepts_text())
                .unwrap();
            let bounds = tree.abs_bounds(slider);
            let at = Point::new(bounds.x + bounds.w - 1, bounds.y + bounds.h / 2);
            let (mut hover, mut capture) = (None, None);
            for kind in [PointerKind::Down, PointerKind::Up] {
                tree.dispatch_pointer(
                    PointerEvent::single(kind, at, MouseButton::Left),
                    &mut hover,
                    &mut capture,
                );
            }
            tree.layout_root(Size::new(400, 50), &mut NullTextEngine);
            assert_eq!(raw.get(), "60");
            assert_eq!(
                &*events.borrow(),
                &[String::from("-"), String::from("17"), String::from("60")]
            );
        });
    }

    #[test]
    fn native_editor_dialog_caps_long_body_and_keeps_footer_in_minimum_viewport() {
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let mut panel_heights = Vec::new();
            for lines in [1, 80] {
                let ui = Ui::new(|(): (), _| {});
                let content = editor_panel(
                    "Editor",
                    col().children((0..lines).map(|_| label("Setting").height(32))),
                    ui.button("Save", ()),
                );
                let mut tree = Tree::new();
                let root = dialog(signal(true), content, ui, ()).build(&mut tree);
                tree.root = Some(root);
                let viewport = Size::new(900, 580);
                tree.layout_root(viewport, &mut NullTextEngine);
                let scrim = tree.get(root).unwrap().children[0];
                assert_eq!(tree.abs_bounds(scrim).h, viewport.h);
                let panel = tree.get(scrim).unwrap().children[0];
                let panel_bounds = tree.abs_bounds(panel);
                assert!(panel_bounds.w <= 860);
                assert!(panel_bounds.x >= 20);
                assert!(panel_bounds.y >= 20);
                assert!(panel_bounds.y + panel_bounds.h <= viewport.h - 20);
                panel_heights.push(panel_bounds.h);
                let footer = *tree.focusable_order().last().unwrap();
                let footer_bounds = tree.abs_bounds(footer);
                assert!(footer_bounds.y >= panel_bounds.y);
                assert!(footer_bounds.y + footer_bounds.h <= panel_bounds.y + panel_bounds.h);
                let mut nodes = vec![root];
                let mut scroll_ranges = Vec::new();
                while let Some(id) = nodes.pop() {
                    let node = tree.get(id).unwrap();
                    nodes.extend(node.children.iter().copied());
                    if matches!(node.layout, windui::core::Layout::Scroll) {
                        scroll_ranges.push(
                            tree.scroll_range(id, windui::spec::Axis::Vertical)
                                .unwrap()
                                .1,
                        );
                    }
                }
                assert_eq!(scroll_ranges.len(), 1);
                assert_eq!(scroll_ranges[0] > 0, lines > 1);
            }
            assert!(panel_heights[0] < panel_heights[1]);
        });
    }
}
