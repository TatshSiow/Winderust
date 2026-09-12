use super::design;
use crate::power::PowerPlan;
use iced::widget::{row, text};
use iced::{Element, Fill};

pub(super) const CARD_GAP: u32 = design::space::SMALL;
pub(super) const CARD_PADDING: u32 = design::space::MEDIUM;
pub(super) const SETTING_ROW_HEIGHT: u32 = 34;
pub(super) const CARD_HEIGHT: u32 = SETTING_ROW_HEIGHT + 2 * CARD_PADDING;

pub(super) fn rules_table<'a, M: 'a>(
    header: iced::widget::Row<'a, M>,
    rows: Element<'a, M>,
) -> Element<'a, M> {
    iced::widget::container(iced::widget::column![
        iced::widget::container(header)
            .padding(CARD_PADDING as u16)
            .style(|theme: &iced::Theme| iced::widget::container::Style {
                text_color: Some(muted_color(theme)),
                ..Default::default()
            }),
        iced::widget::rule::horizontal(1),
        rows,
    ])
    .width(Fill)
    .clip(true)
    .style(|theme| {
        let mut style = surface(theme);
        style.border.width = 1.0;
        style.border.color = control_border(theme);
        style
    })
    .into()
}

pub(super) fn process_rule_row<'a, M: Clone + 'a>(
    path: &str,
    candidates: &[super::app_picker::Candidate],
    active: Element<'a, M>,
    controls: Vec<Element<'a, M>>,
    remove: Option<M>,
) -> Element<'a, M> {
    iced::widget::column![
        process_rule_header(
            path,
            candidates,
            active,
            controls,
            rule_delete_button(remove)
        ),
        iced::widget::rule::horizontal(1)
    ]
    .into()
}

pub(super) fn process_rule_header<'a, M: 'a>(
    path: &str,
    candidates: &[super::app_picker::Candidate],
    active: Element<'a, M>,
    controls: Vec<Element<'a, M>>,
    action: Element<'a, M>,
) -> Element<'a, M> {
    use iced::widget::container;
    let mut cells = row![
        container(active).width(48),
        container(super::app_picker::app_name(path, candidates))
            .width(iced::Length::FillPortion(2))
            .clip(true),
        container(
            text(path.to_owned())
                .style(text::secondary)
                .wrapping(text::Wrapping::None)
        )
        .width(iced::Length::FillPortion(3))
        .clip(true),
    ]
    .spacing(design::space::MEDIUM)
    .align_y(iced::Center);
    for control in controls {
        cells = cells.push(container(control).width(iced::Length::FillPortion(2)));
    }
    cells = cells.push(action);
    container(cells).padding(CARD_PADDING as u16).into()
}

pub(super) fn rule_delete_button<'a, M: Clone + 'a>(remove: Option<M>) -> Element<'a, M> {
    use iced::widget::container;
    let removable = remove.is_some();
    let delete = button(
        iced::widget::svg(
            super::assets::iced_icon("icons/trash-2.svg").expect("Trash icon is bundled"),
        )
        .width(design::ICON_SIZE)
        .height(design::ICON_SIZE)
        .style(move |theme: &iced::Theme, _| iced::widget::svg::Style {
            color: Some(if removable {
                theme.palette().danger
            } else {
                muted_color(theme)
            }),
        }),
    )
    .padding(7)
    .width(32)
    .height(32)
    .style(|theme, status| {
        let mut style = quiet(theme, status);
        style.background = match status {
            iced::widget::button::Status::Hovered => {
                Some(theme.palette().danger.scale_alpha(0.12).into())
            }
            iced::widget::button::Status::Pressed => {
                Some(theme.palette().danger.scale_alpha(0.20).into())
            }
            _ => None,
        };
        style
    })
    .on_press_maybe(remove);

    container(iced::widget::tooltip(
        delete,
        text(rust_i18n::t!("common.remove").to_string()),
        iced::widget::tooltip::Position::Top,
    ))
    .center_x(40)
    .into()
}

pub(super) fn process_rules_table<'a, M: 'a>(
    tiers: impl IntoIterator<Item = String>,
    rows: Vec<(u64, Element<'a, M>)>,
    empty: String,
) -> Element<'a, M> {
    process_rules_table_with_actions(tiers, rows, empty, 40)
}

pub(super) fn process_rules_table_with_actions<'a, M: 'a>(
    tiers: impl IntoIterator<Item = String>,
    rows: Vec<(u64, Element<'a, M>)>,
    empty: String,
    actions_width: u32,
) -> Element<'a, M> {
    use iced::widget::container;
    let mut header = row![
        text(rust_i18n::t!("common.active").to_string()).width(48),
        text(rust_i18n::t!("process_list.app_name").to_string())
            .width(iced::Length::FillPortion(2)),
        text(rust_i18n::t!("process_list.executable_path").to_string())
            .width(iced::Length::FillPortion(3)),
    ]
    .spacing(design::space::MEDIUM);
    for tier in tiers {
        header = header.push(text(tier).width(iced::Length::FillPortion(2)));
    }
    header = header.push(text(rust_i18n::t!("common.actions").to_string()).width(actions_width));
    let rows = if rows.is_empty() {
        container(text(empty).style(text::secondary))
            .padding(design::space::LARGE as u16)
            .into()
    } else {
        iced::widget::keyed_column(rows).into()
    };
    rules_table(header, rows)
}

pub(super) fn active_indicator<'a, M: 'a>(active: bool) -> Element<'a, M> {
    iced::widget::container(iced::widget::Space::new())
        .width(3)
        .height(18)
        .style(move |theme: &iced::Theme| iced::widget::container::Style {
            background: active.then(|| theme.palette().primary.into()),
            border: iced::border::rounded(2),
            ..Default::default()
        })
        .into()
}

pub(super) fn panel_tab<'a, M: Clone + 'a>(
    label: String,
    selected: bool,
    message: M,
) -> iced::widget::Button<'a, M> {
    button(
        iced::widget::column![
            iced::widget::container(heading(label, design::typography::BODY))
                .center_x(Fill)
                .center_y(Fill),
            iced::widget::container(iced::widget::Space::new())
                .width(Fill)
                .height(3)
                .style(move |theme: &iced::Theme| iced::widget::container::Style {
                    background: selected.then(|| theme.palette().primary.into()),
                    border: iced::border::rounded(2),
                    ..Default::default()
                }),
        ]
        .height(Fill),
    )
    .width(Fill)
    .height(design::NAVIGATION_ROW_HEIGHT)
    .padding([0, design::space::SMALL as u16])
    .style(if selected { secondary_button } else { quiet })
    .on_press(message)
}

pub(super) fn preset_footer<'a, M: Clone + 'a>(label: String, message: M) -> Element<'a, M> {
    iced::widget::column![
        iced::widget::rule::horizontal(1),
        iced::widget::container(
            button(
                iced::widget::container(
                    row![
                        iced::widget::svg(
                            super::assets::iced_icon("icons/plus.svg")
                                .expect("Every UI icon is bundled")
                        )
                        .width(design::ICON_SIZE)
                        .height(design::ICON_SIZE)
                        .style(|theme: &iced::Theme, _| {
                            iced::widget::svg::Style {
                                color: Some(theme.extended_palette().primary.base.text),
                            }
                        }),
                        text(label)
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                )
                .center_x(Fill)
                .center_y(Fill)
            )
            .width(Fill)
            .height(32)
            .style(|theme, status| {
                let mut style = iced::widget::button::primary(theme, status);
                style.border.radius = design::CONTROL_RADIUS.into();
                style
            })
            .on_press(message)
        )
        .padding([design::space::MEDIUM as u16, 0])
    ]
    .into()
}

pub(super) fn indicator_chip(theme: &iced::Theme, active: bool) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: Some(if active {
            theme.palette().primary
        } else {
            muted_color(theme)
        }),
        background: Some(if active {
            theme.palette().primary.scale_alpha(0.15).into()
        } else {
            theme.extended_palette().background.neutral.color.into()
        }),
        border: iced::border::rounded(design::CONTROL_RADIUS),
        ..Default::default()
    }
}

pub(super) fn muted_color(theme: &iced::Theme) -> iced::Color {
    theme.extended_palette().secondary.base.color
}

fn control_border(theme: &iced::Theme) -> iced::Color {
    theme.extended_palette().background.strong.color
}

pub(super) fn button<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> iced::widget::Button<'a, M> {
    iced::widget::button(content)
        .padding(design::CONTROL_PADDING)
        .style(secondary_button)
}

pub(super) fn primary_button(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let mut style = iced::widget::button::primary(theme, status);
    style.border.radius = design::CONTROL_RADIUS.into();
    style
}

pub(super) fn secondary_button(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    use iced::widget::button::Status;
    let mut style = quiet(theme, status);
    let alpha = match status {
        Status::Active => 0.15,
        Status::Hovered => 0.23,
        Status::Pressed => 0.30,
        Status::Disabled => 0.06,
    };
    style.background = Some(theme.palette().primary.scale_alpha(alpha).into());
    style
}

pub(super) fn tertiary_button(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let mut style = iced::widget::button::secondary(theme, status);
    style.border.radius = design::CONTROL_RADIUS.into();
    style
}

pub(super) fn danger_button(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let mut style = iced::widget::button::danger(theme, status);
    style.border.radius = design::CONTROL_RADIUS.into();
    style
}

pub(super) fn sidebar_toggle<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
) -> iced::widget::Button<'a, M> {
    button(content)
        .width(Fill)
        .height(design::NAVIGATION_ROW_HEIGHT)
        .padding([design::space::COMPACT as u16, 13])
        .style(quiet)
}

pub(super) fn text_input<'a, M: Clone + 'a>(
    placeholder: &str,
    value: &str,
) -> iced::widget::TextInput<'a, M> {
    iced::widget::text_input(placeholder, value)
        .size(design::typography::BODY)
        .padding(design::INPUT_PADDING)
        .style(|theme, status| {
            let mut style = iced::widget::text_input::default(theme, status);
            style.placeholder = muted_color(theme);
            style.background = theme.extended_palette().background.weak.color.into();
            style.border.radius = design::CONTROL_RADIUS.into();
            if matches!(status, iced::widget::text_input::Status::Active) {
                style.border.color = control_border(theme);
            }
            style
        })
}

pub(super) fn pick_list<'a, T, L, V, M>(
    options: L,
    selected: Option<V>,
    on_selected: impl Fn(T) -> M + 'a,
) -> super::select::Select<'a, T, M>
where
    T: ToString + PartialEq + Clone + 'a,
    L: std::borrow::Borrow<[T]> + 'a,
    V: std::borrow::Borrow<T> + 'a,
    M: Clone + 'a,
{
    super::select::Select::new(
        options.borrow().to_vec(),
        selected.map(|value| value.borrow().clone()),
        on_selected,
    )
}

pub(super) fn select_field(
    theme: &iced::Theme,
    status: iced::widget::pick_list::Status,
) -> iced::widget::pick_list::Style {
    use iced::widget::pick_list::{self, Status};
    let mut style = pick_list::default(theme, status);
    style.background = surface(theme)
        .background
        .unwrap_or_else(|| theme.palette().background.into());
    style.text_color = theme.palette().text;
    style.handle_color = muted_color(theme);
    style.border.radius = design::CONTROL_RADIUS.into();
    style.border.color = match status {
        Status::Active => control_border(theme),
        Status::Hovered => theme.palette().primary.scale_alpha(0.55),
        Status::Opened { .. } => theme.palette().primary,
    };
    if matches!(status, Status::Hovered) {
        style.background = quiet(theme, iced::widget::button::Status::Hovered)
            .background
            .unwrap_or(style.background);
    }
    style
}

pub(super) fn select_menu(theme: &iced::Theme) -> iced::widget::overlay::menu::Style {
    let mut style = iced::widget::overlay::menu::default(theme);
    style.background = select_field(theme, iced::widget::pick_list::Status::Active).background;
    style.border.radius = design::CONTROL_RADIUS.into();
    style.text_color = theme.palette().text;
    style.selected_text_color = theme.palette().text;
    style.selected_background = selected_control(theme, iced::widget::button::Status::Active)
        .background
        .unwrap_or(style.background);
    style
}

pub(super) fn slider<'a, T, M>(
    range: std::ops::RangeInclusive<T>,
    value: T,
    on_change: impl Fn(T) -> M + 'a,
) -> iced::widget::Slider<'a, T, M>
where
    T: Copy + From<u8> + PartialOrd,
    M: Clone + 'a,
{
    iced::widget::slider(range, value, on_change).height(design::SLIDER_HEIGHT)
}

pub(super) fn checkbox<'a, M: 'a>(value: bool) -> iced::widget::Checkbox<'a, M> {
    iced::widget::checkbox(value)
        .size(design::CHECKBOX_SIZE)
        .text_size(design::typography::BODY)
        .spacing(design::space::SMALL)
        .style(checkbox_style)
}

fn checkbox_style(
    theme: &iced::Theme,
    status: iced::widget::checkbox::Status,
) -> iced::widget::checkbox::Style {
    use iced::widget::checkbox::Status;
    let mut style = iced::widget::checkbox::primary(theme, status);
    style.border.radius = design::CONTROL_RADIUS.into();
    if matches!(
        status,
        Status::Active { is_checked: false } | Status::Hovered { is_checked: false }
    ) {
        style.border.color = control_border(theme);
        style.background = theme.extended_palette().background.weak.color.into();
        style.text_color = Some(muted_color(theme));
    }
    style
}

pub(super) fn card_button<'a, M: Clone + 'a>(
    content: iced::widget::Row<'a, M>,
) -> iced::widget::Button<'a, M> {
    button(
        content
            .height(Fill)
            .align_y(iced::Center)
            .spacing(design::space::LARGE),
    )
    .height(CARD_HEIGHT)
    .padding([CARD_PADDING as u16, design::space::WIDE as u16])
    .width(Fill)
    .style(card)
}

/// Stable identity for native keyed rows across insertions and deletions.
pub(super) fn stable_key(value: &impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hash);
    hash.finish()
}

pub(super) fn optional_content<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    visible: bool,
) -> Element<'a, M> {
    if visible {
        content.into()
    } else {
        iced::widget::Space::new().height(0).into()
    }
}

pub(super) fn settings_card<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
) -> iced::widget::Container<'a, M> {
    iced::widget::container(content)
        .padding(CARD_PADDING as u16)
        .width(Fill)
        .style(surface)
}

pub(super) fn setting_group<'a, M: Clone + 'a>(
    label: String,
    expanded: bool,
    message: M,
    action: impl Into<Element<'a, M>>,
    content: iced::widget::Column<'a, M>,
) -> Element<'a, M> {
    settings_card(
        iced::widget::column![
            button(
                row![
                    iced::widget::container(setting_title(&label)).width(Fill),
                    action.into(),
                    super::navigation::glyph(if expanded {
                        "icons/chevron-down.svg"
                    } else {
                        "icons/chevron-right.svg"
                    })
                ]
                .spacing(design::space::SMALL)
                .height(SETTING_ROW_HEIGHT)
                .align_y(iced::Center)
            )
            .width(Fill)
            .padding(CARD_PADDING as u16)
            .style(quiet)
            .on_press(message),
            optional_content(
                iced::widget::container(content.spacing(2 * CARD_PADDING)).padding(iced::Padding {
                    top: CARD_PADDING as f32,
                    right: CARD_PADDING as f32,
                    bottom: CARD_PADDING as f32,
                    left: CARD_PADDING as f32,
                }),
                expanded,
            )
        ]
        .spacing(0),
    )
    .padding(0)
    .into()
}

pub(super) fn setting_row<'a, M: 'a>(
    key: &str,
    action: impl Into<Element<'a, M>>,
) -> iced::widget::Row<'a, M> {
    setting_row_with_unit(key, "", action)
}

pub(super) fn setting_row_with_unit<'a, M: 'a>(
    key: &str,
    unit: &str,
    action: impl Into<Element<'a, M>>,
) -> iced::widget::Row<'a, M> {
    row![
        iced::widget::container(setting_title_with_unit(key, unit)).width(Fill),
        action.into()
    ]
    .spacing(design::space::MEDIUM)
    .height(SETTING_ROW_HEIGHT)
    .align_y(iced::Center)
}

pub(super) fn setting_title<'a, M: 'a>(key: &str) -> Element<'a, M> {
    setting_title_with_unit(key, "")
}

fn setting_title_with_unit<'a, M: 'a>(key: &str, unit: &str) -> Element<'a, M> {
    let help_key = format!("{key}_help");
    let help = rust_i18n::t!(&help_key).to_string();
    let mut label = row![heading(
        label_with_unit(&rust_i18n::t!(key), unit),
        design::typography::BODY
    )]
    .spacing(design::space::SMALL)
    .align_y(iced::Center);
    if help != help_key {
        label = label.push(iced::widget::tooltip(
            super::navigation::glyph("icons/info.svg"),
            iced::widget::container(text(help).width(320))
                .padding(design::space::MEDIUM as u16)
                .style(surface),
            iced::widget::tooltip::Position::Top,
        ));
    }
    label.into()
}

pub(super) fn switch<'a, M: Clone + 'a>(
    value: bool,
    action: Option<impl Fn(bool) -> M + 'a>,
) -> Element<'a, M> {
    row![
        text(rust_i18n::t!(if value { "common.on" } else { "common.off" }).to_string()),
        iced::widget::toggler(value)
            .size(design::SWITCH_SIZE)
            .style(toggle_style)
            .on_toggle_maybe(action)
    ]
    .spacing(design::space::SMALL)
    .align_y(iced::Center)
    .into()
}

fn toggle_style(
    theme: &iced::Theme,
    status: iced::widget::toggler::Status,
) -> iced::widget::toggler::Style {
    use iced::widget::toggler::Status;
    let mut style = iced::widget::toggler::default(theme, status);
    if matches!(status, Status::Hovered { is_toggled: true }) {
        style.foreground = theme.extended_palette().primary.base.text.into();
    } else if matches!(
        status,
        Status::Active { is_toggled: false } | Status::Hovered { is_toggled: false }
    ) {
        style.background = theme.extended_palette().background.weak.color.into();
        style.background_border_width = 1.0;
        style.background_border_color = control_border(theme);
        style.foreground = muted_color(theme).into();
    }
    style
}

pub(super) fn control_button(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let mut style = quiet(theme, status);
    if matches!(
        status,
        iced::widget::button::Status::Active | iced::widget::button::Status::Disabled
    ) {
        style.background = Some(theme.extended_palette().background.neutral.color.into());
    }
    style
}

pub(super) fn stepper<'a, M: Clone + 'a>(
    value: &str,
    range: std::ops::RangeInclusive<u64>,
    step: u64,
    action: Option<impl Fn(String) -> M + Clone + 'a>,
) -> Element<'a, M> {
    let (lower, upper) = step_values(value, range, step);
    row![
        button(super::navigation::glyph("icons/minus.svg"))
            .padding(7)
            .height(32)
            .style(control_button)
            .width(design::STEPPER_BUTTON_WIDTH)
            .on_press_maybe(lower.zip(action.as_ref()).map(|(v, f)| f(v.to_string()))),
        text_input("", value)
            .align_x(iced::alignment::Horizontal::Center)
            .on_input_maybe(action.clone())
            .width(design::NUMERIC_WIDTH),
        button(super::navigation::glyph("icons/plus.svg"))
            .padding(7)
            .height(32)
            .style(control_button)
            .width(design::STEPPER_BUTTON_WIDTH)
            .on_press_maybe(upper.zip(action.as_ref()).map(|(v, f)| f(v.to_string())))
    ]
    .spacing(design::space::CONTROL)
    .align_y(iced::Center)
    .into()
}

fn step_values(
    value: &str,
    range: std::ops::RangeInclusive<u64>,
    step: u64,
) -> (Option<u64>, Option<u64>) {
    let parsed = value.parse::<u64>().ok().filter(|v| range.contains(v));
    let lower = parsed
        .filter(|v| v > range.start())
        .map(|v| v.saturating_sub(step).max(*range.start()));
    let upper = parsed
        .filter(|v| v < range.end())
        .map(|v| v.saturating_add(step).min(*range.end()));
    (lower, upper)
}
#[cfg(test)]
mod step_tests {
    #[test]
    fn stepper_keeps_bounds_and_invalid_drafts() {
        use super::step_values;
        assert_eq!(step_values("1", 1..=10, 3), (None, Some(4)));
        assert_eq!(step_values("9", 1..=10, 3), (Some(6), Some(10)));
        assert_eq!(step_values("10", 1..=10, 3), (Some(7), None));
        assert_eq!(step_values("", 1..=10, 1), (None, None));
        assert_eq!(step_values("11", 1..=10, 1), (None, None));
        assert_eq!(
            step_values(&u64::MAX.to_string(), 0..=u64::MAX, 1),
            (Some(u64::MAX - 1), None)
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Choice<T>(pub T, pub String);

impl<T> std::fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

pub(super) fn plan<M: Clone + 'static>(
    guid: Option<String>,
    plans: &[PowerPlan],
    action: impl Fn(Option<String>) -> M + 'static,
) -> Element<'static, M> {
    let mut choices = vec![Choice(None, rust_i18n::t!("common.none").to_string())];
    choices.extend(
        plans
            .iter()
            .map(|plan| Choice(Some(plan.guid.clone()), plan.display_name())),
    );
    let selected = choices
        .iter()
        .find(|choice| choice.0 == guid)
        .cloned()
        .unwrap_or_else(|| Choice(guid.clone(), guid.unwrap_or_default()));
    pick_list(choices, Some(selected), move |choice| action(choice.0))
        .width(Fill)
        .into()
}

// Theme-derived surfaces and interaction states; controls remain standard Iced widgets.
pub(super) fn heading<'a>(label: String, size: u32) -> iced::widget::Text<'a> {
    text(label).size(size).font(design::typography::FONT)
}

pub(super) fn navigation_surface(theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(theme.extended_palette().background.weaker.color.into()),
        ..Default::default()
    }
}

pub(super) fn surface(theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(theme.extended_palette().background.weak.color.into()),
        border: iced::border::rounded(design::CARD_RADIUS),
        ..Default::default()
    }
}

pub(super) fn selected(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let mut style = quiet(theme, status);
    if status != iced::widget::button::Status::Disabled {
        style.text_color = theme.palette().primary;
    }
    style
}

pub(super) fn selected_control(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let mut style = control_button(theme, status);
    if status == iced::widget::button::Status::Active {
        style.background = Some(theme.palette().primary.scale_alpha(0.15).into());
    }
    style
}

pub(super) fn quiet(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    use iced::widget::button::{self, Status};
    let mut style = button::text(theme, status);
    style.border.radius = design::CONTROL_RADIUS.into();
    style.background = match status {
        Status::Hovered => Some(theme.palette().primary.scale_alpha(0.10).into()),
        Status::Pressed => Some(theme.palette().primary.scale_alpha(0.20).into()),
        _ => None,
    };
    style
}

pub(super) fn card(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    use iced::widget::button::Status;
    let mut style = quiet(theme, status);
    let surface = surface(theme);
    style.border = surface.border;
    if matches!(status, Status::Active | Status::Disabled) {
        style.background = surface.background;
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{widget::button::Status, Theme};

    #[test]
    fn button_hierarchy_tracks_accent_and_keeps_tertiary_neutral() {
        for light in [false, true] {
            let theme = |accent| {
                let mut palette = design::palette(light);
                palette.primary = accent;
                Theme::custom_with_fn("buttons", palette, design::extended_palette)
            };
            let red = theme(iced::Color::from_rgb8(220, 70, 70));
            let blue = theme(iced::Color::from_rgb8(40, 150, 220));
            for status in [
                Status::Active,
                Status::Hovered,
                Status::Pressed,
                Status::Disabled,
            ] {
                assert_ne!(
                    primary_button(&red, status).background,
                    primary_button(&blue, status).background
                );
                assert_ne!(
                    secondary_button(&red, status).background,
                    secondary_button(&blue, status).background
                );
                assert_eq!(
                    tertiary_button(&red, status).background,
                    tertiary_button(&blue, status).background
                );
            }
            assert_eq!(
                primary_button(&red, Status::Active).text_color,
                red.extended_palette().primary.base.text
            );
        }
    }

    #[test]
    fn neutral_surfaces_and_adaptive_marks_in_both_themes() {
        for light in [false, true] {
            let themes = [
                iced::Color::from_rgb8(240, 80, 100),
                iced::Color::from_rgb8(50, 160, 240),
                iced::Color::BLACK,
                iced::Color::WHITE,
                iced::Color::from_rgb8(255, 235, 20),
            ]
            .map(|accent| {
                let mut palette = design::palette(light);
                palette.primary = accent;
                Theme::custom_with_fn("test", palette, design::extended_palette)
            });
            assert_eq!(
                themes[0].palette().background,
                themes[1].palette().background
            );
            for surface in [navigation_surface, super::surface] {
                assert_eq!(
                    surface(&themes[0]).background,
                    surface(&themes[1]).background
                );
            }
            assert_eq!(
                control_button(&themes[0], Status::Active).background,
                control_button(&themes[1], Status::Active).background
            );
            assert_eq!(
                select_menu(&themes[0]).background,
                select_menu(&themes[1]).background
            );
            let checkbox = |theme: &Theme| {
                checkbox_style(
                    theme,
                    iced::widget::checkbox::Status::Active { is_checked: true },
                )
            };
            let toggle = |theme: &Theme| {
                toggle_style(
                    theme,
                    iced::widget::toggler::Status::Active { is_toggled: true },
                )
            };
            assert_ne!(
                checkbox(&themes[0]).background,
                checkbox(&themes[1]).background
            );
            assert_ne!(toggle(&themes[0]).background, toggle(&themes[1]).background);
            for theme in &themes {
                let expected = theme.extended_palette().primary.base.text;
                assert_eq!(checkbox(theme).icon_color, expected);
                assert_eq!(toggle(theme).foreground, iced::Background::Color(expected));
            }
        }
    }

    #[test]
    fn interactive_surfaces_distinguish_hover_press_selection_and_disabled() {
        for theme in [Theme::CatppuccinLatte, Theme::CatppuccinMocha] {
            assert_eq!(selected(&theme, Status::Active).background, None);
            assert_eq!(
                selected(&theme, Status::Active).text_color,
                theme.palette().primary
            );
            assert_eq!(
                selected(&theme, Status::Hovered).text_color,
                theme.palette().primary
            );
            for style in [quiet, selected, card, control_button, selected_control] {
                let active = style(&theme, Status::Active);
                let hovered = style(&theme, Status::Hovered);
                let pressed = style(&theme, Status::Pressed);
                assert_ne!(active.background, hovered.background);
                assert_ne!(hovered.background, pressed.background);
                assert_eq!(
                    hovered.background,
                    quiet(&theme, Status::Hovered).background
                );
                assert_eq!(active.border.width, 0.0);
                assert_eq!(hovered.border.width, 0.0);
                assert_eq!(pressed.border.width, 0.0);
                assert_eq!(active.border.radius, hovered.border.radius);
                assert_ne!(
                    style(&theme, Status::Disabled).text_color,
                    active.text_color
                );
            }
            assert_eq!(
                card(&theme, Status::Disabled).background,
                card(&theme, Status::Active).background
            );
            assert_eq!(
                control_button(&theme, Status::Disabled).background,
                control_button(&theme, Status::Active).background
            );
            assert_ne!(
                control_button(&theme, Status::Active).background,
                quiet(&theme, Status::Active).background
            );
        }
    }
}

pub(super) fn modal_frame<'a, M: 'a>(
    header: impl Into<Element<'a, M>>,
    body: impl Into<Element<'a, M>>,
    footer: impl Into<Element<'a, M>>,
    size: (u32, u32),
) -> Element<'a, M> {
    use iced::widget::{column, container, rule};
    container(column![
        container(header).padding(16),
        rule::horizontal(1),
        container(body).padding(16).height(Fill),
        rule::horizontal(1),
        container(footer).padding(16)
    ])
    .width(Fill)
    .max_width(size.0)
    .height(Fill)
    .max_height(size.1)
    .style(|theme: &iced::Theme| container::Style {
        background: Some(theme.palette().background.into()),
        border: iced::Border {
            color: theme.extended_palette().background.strong.color,
            width: 1.0,
            radius: design::CARD_RADIUS.into(),
        },
        ..Default::default()
    })
    .into()
}

pub(super) fn label_with_unit(label: &str, unit: &str) -> String {
    if unit.is_empty() {
        label.to_owned()
    } else {
        format!("{label} ({unit})")
    }
}
