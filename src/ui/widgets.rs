use super::design;
use crate::power::PowerPlan;
use iced::widget::{row, text};
use iced::{Element, Fill};

pub(super) const CARD_GAP: u32 = design::space::MEDIUM;
pub(super) const CARD_PADDING: u32 = design::space::LARGE;
pub(super) const SETTING_ROW_HEIGHT: u32 = 34;
pub(super) const CARD_HEIGHT: u32 = SETTING_ROW_HEIGHT + 2 * CARD_PADDING;

pub(super) fn rules_table<'a, M: 'a>(
    header: iced::widget::Row<'a, M>,
    rows: Element<'a, M>,
) -> Element<'a, M> {
    (iced::widget::container(iced::widget::column![
        iced::widget::container(header)
            .padding(CARD_PADDING as u16)
            .style(|theme: &iced::Theme| iced::widget::container::Style {
                text_color: Some(muted_color(theme)),
                ..Default::default()
            }),
        settings_divider(),
        rows,
    ])
    .width(Fill)
    .clip(true)
    .style(surface))
    .into()
}

pub(super) fn table_frame<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    super::scrolling::table_frame(content)
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
            None,
            controls,
            iced::widget::container(rule_delete_button(remove))
                .center_x(80)
                .into()
        ),
        settings_divider()
    ]
    .into()
}

pub(super) fn process_rule_header<'a, M: 'a>(
    path: &str,
    candidates: &[super::app_picker::Candidate],
    active: Element<'a, M>,
    status: Option<Element<'a, M>>,
    controls: Vec<Element<'a, M>>,
    action: Element<'a, M>,
) -> Element<'a, M> {
    use iced::widget::container;
    let mut cells = row![container(active).width(48)]
        .spacing(design::space::MEDIUM)
        .align_y(iced::Center);
    if let Some(status) = status {
        cells = cells.push(container(status).width(140));
    }
    let mut cells = cells
        .push(
            container(super::app_picker::app_name(path, candidates))
                .width(iced::Length::FillPortion(2))
                .clip(true),
        )
        .push(
            container(
                text(path.to_owned())
                    .style(text::secondary)
                    .wrapping(text::Wrapping::None),
            )
            .width(iced::Length::FillPortion(3))
            .clip(true),
        );
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
    process_rules_table_with_actions(tiers, rows, empty, 80, false)
}

pub(super) fn process_rules_table_with_actions<'a, M: 'a>(
    tiers: impl IntoIterator<Item = String>,
    rows: Vec<(u64, Element<'a, M>)>,
    empty: String,
    actions_width: u32,
    show_status: bool,
) -> Element<'a, M> {
    use iced::widget::container;
    let tiers: Vec<_> = tiers.into_iter().collect();
    let min_width = 720.0 + 100.0 * tiers.len() as f32 + if show_status { 140.0 } else { 0.0 };
    let mut header = row![text(rust_i18n::t!("common.enable").to_string()).width(48)]
        .spacing(design::space::MEDIUM);
    if show_status {
        header = header.push(text(rust_i18n::t!("common.status").to_string()).width(140));
    }
    let mut header = header
        .push(
            text(rust_i18n::t!("process_list.app_name").to_string())
                .width(iced::Length::FillPortion(2)),
        )
        .push(
            text(rust_i18n::t!("process_list.executable_path").to_string())
                .width(iced::Length::FillPortion(3)),
        );
    for tier in tiers {
        header = header.push(text(tier).width(iced::Length::FillPortion(2)));
    }
    header = header.push(
        text(rust_i18n::t!("common.actions").to_string())
            .width(actions_width)
            .align_x(iced::Center),
    );
    let rows = if rows.is_empty() {
        container(text(empty).style(text::secondary))
            .padding(design::space::LARGE as u16)
            .into()
    } else {
        iced::widget::keyed_column(rows).into()
    };
    super::scrolling::horizontal_table(rules_table(header, rows), min_width)
}

pub(super) fn active_indicator<'a, M: 'a>(active: bool) -> Element<'a, M> {
    let blend = std::rc::Rc::new(std::cell::Cell::new(f32::from(active)));
    let progress = blend.clone();
    super::motion::wrap(
        iced::widget::container(iced::widget::Space::new())
            .width(3)
            .height(18)
            .style(move |theme: &iced::Theme| iced::widget::container::Style {
                background: Some(theme.palette().primary.scale_alpha(progress.get()).into()),
                border: iced::border::rounded(2),
                ..Default::default()
            }),
        active,
        super::motion::Effect::Control(blend),
    )
}

pub(super) fn panel_heading<'a, M: 'a>(label: String) -> Element<'a, M> {
    iced::widget::container(heading(label, design::typography::SUBTITLE))
        .width(Fill)
        .center_y(design::NAVIGATION_ROW_HEIGHT)
        .padding([0, design::space::MEDIUM as u16])
        .into()
}

pub(super) fn panel_tab<'a, M: Clone + 'a>(
    label: String,
    selected: bool,
    message: M,
) -> super::animated_controls::Button<'a, M> {
    button(
        iced::widget::container(heading(label, design::typography::BODY))
            .center_x(Fill)
            .center_y(Fill),
    )
    .width(iced::Shrink)
    .height(32)
    .padding([0, design::space::MEDIUM as u16])
    .style(move |theme, status| tab_style(theme, status, selected))
    .on_press(message)
}

pub(super) fn tab_bar(theme: &iced::Theme) -> iced::widget::container::Style {
    let mut style = surface(theme);
    style.background = None;
    style.border.radius = design::CONTROL_RADIUS.into();
    style
}

pub(super) fn tab_style(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
    active: bool,
) -> iced::widget::button::Style {
    let mut style = if active {
        selected(theme, status)
    } else {
        quiet(theme, status)
    };
    style.border.width = 1.0;
    style.border.color = if active {
        theme.palette().primary.scale_alpha(0.6)
    } else {
        iced::Color::TRANSPARENT
    };
    style
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
            .style(primary_button)
            .on_press(message)
        )
        .padding([design::space::MEDIUM as u16, 0])
    ]
    .into()
}

pub(super) fn rule_status_chip(theme: &iced::Theme, key: &str) -> iced::widget::container::Style {
    let color = match key {
        "common.applied" => theme.palette().success,
        "common.waiting" => theme.palette().warning,
        "common.error" => theme.palette().danger,
        "common.unknown" => muted_color(theme),
        _ => return indicator_chip(theme, false),
    };
    iced::widget::container::Style {
        text_color: Some(color),
        background: Some(color.scale_alpha(0.15).into()),
        ..indicator_chip(theme, false)
    }
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

pub(super) fn button<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
) -> super::animated_controls::Button<'a, M> {
    super::animated_controls::Button::new(content)
        .padding(design::CONTROL_PADDING)
        .style(secondary_button)
}

pub(super) fn action_button<'a, M: 'a>(
    label: impl Into<String>,
) -> super::animated_controls::Button<'a, M> {
    button(
        iced::widget::container(text(label.into()).size(design::typography::BODY)).center_y(Fill),
    )
    .height(design::BUTTON_HEIGHT)
    .padding([0, design::space::MEDIUM as u16])
}

pub(super) fn icon_button<'a, M: 'a>(
    icon: &'static str,
) -> super::animated_controls::Button<'a, M> {
    button(super::navigation::glyph(icon))
        .padding(7)
        .width(design::BUTTON_HEIGHT)
        .height(design::BUTTON_HEIGHT)
        .style(quiet)
}

pub(super) fn primary_button(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let mut style = iced::widget::button::primary(theme, status);
    style.border.radius = design::CONTROL_RADIUS.into();
    style.border.width = 1.0;
    style
}

pub(super) fn secondary_button(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    use iced::widget::button::Status;
    let mut style = quiet(theme, status);
    let disabled = status == Status::Disabled;
    style.border.width = 1.0;
    style.border.color = control_border(theme).scale_alpha(if disabled { 0.35 } else { 0.75 });
    style.text_color = theme
        .palette()
        .primary
        .scale_alpha(if disabled { 0.35 } else { 1.0 });
    style.background = match status {
        Status::Hovered => Some(theme.palette().primary.scale_alpha(0.08).into()),
        Status::Pressed => Some(theme.palette().primary.scale_alpha(0.14).into()),
        _ => None,
    };
    style
}

pub(super) fn tertiary_button(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    use iced::widget::button::Status;
    let mut style = secondary_button(theme, status);
    style.text_color = if status == Status::Disabled {
        muted_color(theme).scale_alpha(0.5)
    } else {
        theme.palette().text
    };
    style.background = match status {
        Status::Hovered => Some(theme.extended_palette().background.neutral.color.into()),
        Status::Pressed => Some(
            theme
                .extended_palette()
                .background
                .strong
                .color
                .scale_alpha(0.6)
                .into(),
        ),
        _ => None,
    };
    style
}

pub(super) fn danger_button(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let mut style = iced::widget::button::danger(theme, status);
    style.border.radius = design::CONTROL_RADIUS.into();
    style.border.width = 1.0;
    style
}

pub(super) const SEARCH_INPUT_PADDING: iced::Padding = iced::Padding {
    right: 32.0,
    ..iced::Padding::new(design::INPUT_PADDING as f32)
};

pub(super) fn search_field<'a, M: Clone + 'a>(
    input: impl Into<Element<'a, M>>,
    clear: Option<M>,
) -> Element<'a, M> {
    let mut field = iced::widget::stack![input.into()];
    if let Some(clear) = clear {
        field = field.push(
            iced::widget::container(iced::widget::tooltip(
                button(super::navigation::glyph("icons/x.svg"))
                    .padding(3)
                    .style(quiet)
                    .on_press(clear),
                text(rust_i18n::t!("common.clear").to_string()),
                iced::widget::tooltip::Position::Left,
            ))
            .padding([0, design::INPUT_PADDING])
            .align_right(Fill)
            .center_y(Fill),
        );
    }
    field.into()
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
            style.background = theme.extended_palette().background.neutral.color.into();
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
    let background = theme.extended_palette().background.neutral.color;
    style.background = background.into();
    style.text_color = theme.palette().text;
    style.handle_color = muted_color(theme);
    style.border.radius = design::CONTROL_RADIUS.into();
    style.border.color = match status {
        Status::Active => control_border(theme),
        Status::Hovered => theme.palette().primary.scale_alpha(0.55),
        Status::Opened { .. } => theme.palette().primary,
    };
    if matches!(status, Status::Hovered) {
        style.background =
            iced::theme::palette::mix(background, theme.palette().primary, 0.08).into();
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

pub(super) fn checkbox<'a, M: 'a>(value: bool) -> super::animated_controls::Checkbox<'a, M> {
    super::animated_controls::Checkbox::new(value)
}

pub(super) fn checkbox_style(
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
) -> super::animated_controls::Button<'a, M> {
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
    super::motion::wrap(content, visible, super::motion::Effect::Visible)
}

pub(super) fn settings_card<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
) -> iced::widget::Container<'a, M> {
    iced::widget::container(super::scrolling::repaint_group(
        iced::widget::container(content)
            .padding(CARD_PADDING as u16)
            .width(Fill)
            .style(surface),
    ))
    .width(Fill)
}

/// Keep native controls above the card's full-surface action.
pub(super) fn action_card<'a, M: Clone + 'a>(
    content: impl Into<Element<'a, M>>,
    message: Option<M>,
) -> Element<'a, M> {
    super::scrolling::repaint_group(
        iced::widget::Stack::new()
            .push(
                iced::widget::container(content)
                    .padding(CARD_PADDING as u16)
                    .width(Fill),
            )
            .push_under(
                button(iced::widget::Space::new())
                    .padding(0)
                    .width(Fill)
                    .height(Fill)
                    .style(card)
                    .on_press_maybe(message),
            ),
    )
}

pub(super) fn toggle_card<'a, M: Clone + 'a>(
    key: &str,
    value: bool,
    action: Option<impl Fn(bool) -> M + 'a>,
) -> Element<'a, M> {
    let message = action.as_ref().map(|action| action(!value));
    action_card(setting_row(key, switch(value, action)), message)
}

/// Separate complete settings, keeping each label, description, and control together.
pub(super) fn settings_rows<'a, M: 'a>(
    rows: impl IntoIterator<Item = Element<'a, M>>,
) -> iced::widget::Column<'a, M> {
    let mut content = iced::widget::Column::new().spacing(design::space::MEDIUM);
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            content = content.push(settings_divider());
        }
        content = content.push(row);
    }
    content
}

pub(super) fn settings_divider<'a>() -> iced::widget::Rule<'a> {
    iced::widget::rule::horizontal(1).style(|theme: &iced::Theme| iced::widget::rule::Style {
        color: control_border(theme).scale_alpha(0.3),
        ..iced::widget::rule::default(theme)
    })
}

pub(super) fn setting_group<'a, M: Clone + 'a>(
    label: String,
    expanded: bool,
    message: M,
    action: impl Into<Element<'a, M>>,
    content: iced::widget::Column<'a, M>,
) -> Element<'a, M> {
    super::scrolling::repaint_group(
        iced::widget::container(
            iced::widget::column![
                iced::widget::Stack::new()
                    .push(
                        iced::widget::container(
                            row![
                                iced::widget::container(setting_title(&label)).width(Fill),
                                action.into(),
                                super::motion::wrap(
                                    super::navigation::glyph("icons/chevron-right.svg"),
                                    expanded,
                                    super::motion::Effect::Chevron
                                )
                            ]
                            .spacing(design::space::SMALL)
                            .align_y(iced::Center)
                        )
                        .padding(CARD_PADDING as u16)
                        .width(Fill)
                    )
                    .push_under(
                        button(iced::widget::Space::new())
                            .padding(0)
                            .width(Fill)
                            .height(Fill)
                            .style(quiet)
                            .on_press(message)
                    ),
                super::motion::wrap(
                    iced::widget::column![
                        iced::widget::container(settings_divider())
                            .padding([0, CARD_PADDING as u16]),
                        iced::widget::container(content).padding(CARD_PADDING as u16)
                    ],
                    expanded,
                    super::motion::Effect::Visible
                )
            ]
            .spacing(0),
        )
        .width(Fill)
        .style(surface),
    )
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
    .align_y(iced::Center)
}

pub(super) fn setting_title<'a, M: 'a>(key: &str) -> Element<'a, M> {
    setting_title_with_unit(key, "").into()
}

pub(super) fn setting_title_with_unit<'a, M: 'a>(
    key: &str,
    unit: &str,
) -> iced::widget::Column<'a, M> {
    setting_title_with_help(key, unit, &format!("{key}_help"))
}

pub(super) fn setting_title_with_help<'a, M: 'a>(
    key: &str,
    unit: &str,
    help_key: &str,
) -> iced::widget::Column<'a, M> {
    let help = rust_i18n::t!(help_key).to_string();
    let mut label = iced::widget::column![heading(
        label_with_unit(&rust_i18n::t!(key), unit),
        design::typography::BODY
    )
    .width(Fill)]
    .spacing(design::space::TIGHT)
    .width(Fill);
    if help != help_key {
        label = label.push(
            text(help)
                .size(design::typography::SECONDARY)
                .style(text::secondary)
                .width(Fill),
        );
    }
    label
}

pub(super) fn switch<'a, M: Clone + 'a>(
    value: bool,
    action: Option<impl Fn(bool) -> M + 'a>,
) -> Element<'a, M> {
    let interactive = action.is_some();
    row![
        text(rust_i18n::t!(if value { "common.on" } else { "common.off" }).to_string()),
        super::motion::wrap(
            iced::widget::toggler(value)
                .size(design::SWITCH_SIZE)
                .style(toggle_style)
                .on_toggle_maybe(action),
            value,
            super::motion::Effect::Switch { interactive }
        )
    ]
    .spacing(design::space::SMALL)
    .align_y(iced::Center)
    .into()
}

pub(super) fn toggle_style(
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
    tertiary_button(theme, status)
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
mod card_hitbox_tests {
    use super::*;
    use iced::advanced::{layout, widget::Tree, Layout, Shell};
    use iced::{keyboard, mouse, touch, Event, Point, Rectangle, Size};

    #[derive(Debug, Clone, PartialEq)]
    enum Message {
        Card,
        Toggle(bool),
        Header,
        Body,
        Input(String),
    }

    fn interact(
        mut view: Element<'_, Message>,
        position: impl FnOnce(&layout::Node) -> Point,
        touch: bool,
    ) -> (Vec<Message>, mouse::Interaction) {
        let renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let mut tree = Tree::new(&view);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(800.0, 600.0)),
        );
        let point = position(&node);
        let cursor = mouse::Cursor::Available(point);
        let interaction = view.as_widget().mouse_interaction(
            &tree,
            Layout::new(&node),
            cursor,
            &node.bounds(),
            &renderer,
        );
        let events = if touch {
            vec![
                Event::Touch(touch::Event::FingerPressed {
                    id: touch::Finger(0),
                    position: point,
                }),
                Event::Touch(touch::Event::FingerLifted {
                    id: touch::Finger(0),
                    position: point,
                }),
            ]
        } else {
            vec![
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ]
        };
        let mut messages = Vec::new();
        for event in events
            .into_iter()
            .chain([Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Character("x".into()),
                modified_key: keyboard::Key::Character("x".into()),
                physical_key: keyboard::key::Physical::Unidentified(
                    keyboard::key::NativeCode::Unidentified,
                ),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::empty(),
                text: Some("x".into()),
                repeat: false,
            })])
        {
            view.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                cursor,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &node.bounds(),
            );
        }
        (messages, interaction)
    }

    fn control_bounds(layout: Layout<'_>, width: f32) -> Option<Rectangle> {
        layout
            .children()
            .find_map(|child| control_bounds(child, width))
            .or_else(|| (layout.bounds().width == width).then_some(layout.bounds()))
    }

    fn group(expanded: bool) -> Element<'static, Message> {
        setting_group(
            "watchdog.enable".into(),
            expanded,
            Message::Card,
            action_button("Header").width(120).on_press(Message::Header),
            iced::widget::column![
                action_button("Body").width(140).on_press(Message::Body),
                text_input("", "").width(160).on_input(Message::Input),
            ]
            .spacing(12),
        )
    }

    fn header_layout(node: &layout::Node) -> Layout<'_> {
        Layout::new(node)
            .children()
            .next()
            .unwrap()
            .children()
            .next()
            .unwrap()
    }

    #[test]
    fn expandable_card_labels_and_body_share_the_standard_inset() {
        fn first_leaf(layout: Layout<'_>) -> Rectangle {
            layout
                .children()
                .next()
                .map(first_leaf)
                .unwrap_or_else(|| layout.bounds())
        }
        for expanded in [false, true] {
            interact(
                group(expanded),
                |node| {
                    let layout = Layout::new(node);
                    let foreground = header_layout(node).children().last().unwrap();
                    let label = first_leaf(foreground);
                    assert_eq!(label.x, CARD_PADDING as f32);
                    assert_eq!(label.y, CARD_PADDING as f32);
                    if expanded {
                        let body = control_bounds(layout, 140.0).unwrap();
                        assert_eq!(body.x, label.x);
                    }
                    Point::new(1.0, 1.0)
                },
                false,
            );
        }
    }

    #[test]
    fn full_header_padding_and_blank_space_accept_mouse_and_touch() {
        for expanded in [false, true] {
            for touch in [false, true] {
                for (x, y) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.5)] {
                    let (messages, interaction) = interact(
                        group(expanded),
                        |node| {
                            let bounds = header_layout(node).bounds();
                            Point::new(
                                1.0 + (bounds.width - 2.0) * x,
                                1.0 + (bounds.height - 2.0) * y,
                            )
                        },
                        touch,
                    );
                    assert_eq!(
                        messages,
                        [Message::Card],
                        "expanded={expanded}, touch={touch}, point=({x},{y})"
                    );
                    assert_eq!(interaction, mouse::Interaction::Pointer);
                }
            }
        }
        assert!(interact(group(true), |_| Point::new(-1.0, -1.0), false)
            .0
            .is_empty());
    }

    #[test]
    fn expanded_body_padding_and_blank_space_do_not_collapse_the_card() {
        for touch in [false, true] {
            for (x, y) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.5)] {
                let (messages, interaction) = interact(
                    group(true),
                    |node| {
                        let header = header_layout(node).bounds();
                        let top = header.y + header.height;
                        let bounds = node.bounds();
                        Point::new(
                            1.0 + (bounds.width - 2.0) * x,
                            top + 1.0 + (bounds.height - top - 2.0) * y,
                        )
                    },
                    touch,
                );
                assert!(messages.is_empty(), "touch={touch}, point=({x},{y})");
                assert_eq!(interaction, mouse::Interaction::None);
            }
        }
    }

    #[test]
    fn nested_controls_keep_their_actions_cursor_and_keyboard_input() {
        for touch in [false, true] {
            for (width, expected) in [
                (120.0, Message::Header),
                (140.0, Message::Body),
                (160.0, Message::Input("x".into())),
            ] {
                let (messages, interaction) = interact(
                    group(true),
                    |node| control_bounds(Layout::new(node), width).unwrap().center(),
                    touch,
                );
                assert_eq!(messages, [expected], "touch={touch}, width={width}");
                if width == 160.0 {
                    assert_eq!(interaction, mouse::Interaction::Text);
                }
            }
        }
        for enabled in [false, true] {
            for point in [
                Point::new(1.0, 1.0),
                Point::new(799.0, 1.0),
                Point::new(760.0, 30.0),
            ] {
                let card =
                    toggle_card("watchdog.enable", false, enabled.then_some(Message::Toggle));
                let (messages, _) = interact(card, |_| point, false);
                assert_eq!(
                    messages,
                    if enabled {
                        vec![Message::Toggle(true)]
                    } else {
                        Vec::new()
                    }
                );
            }
        }
    }
}

#[cfg(test)]
mod step_tests {
    #[test]
    fn narrow_tables_scroll_without_compressing_their_columns() {
        use iced::advanced::{layout, widget::Tree};
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        for width in [430.0, 1200.0] {
            let mut view: iced::Element<'_, ()> = crate::ui::scrolling::horizontal_table(
                iced::widget::Space::new().width(iced::Fill).height(60),
                900.0,
            );
            let mut tree = Tree::new(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, 1000.0)),
            );
            assert_eq!(node.bounds().width, width);
            assert_eq!(node.children()[0].bounds().width, width.max(900.0));
            assert!(node.bounds().height < 100.0);
        }
    }

    #[test]
    fn setting_descriptions_wrap_without_clipping_rows_or_controls() {
        use iced::advanced::{layout, widget::Tree};
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut heights = Vec::new();
        for width in [760.0, 360.0] {
            let mut view: iced::Element<'_, ()> = super::setting_row(
                "settings.allow_cross_session_process_control",
                super::action_button("Control").width(120).on_press(()),
            )
            .into();
            let mut tree = Tree::new(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, 1000.0)),
            );
            let label = &node.children()[0].children()[0];
            assert_eq!(
                label.children().len(),
                2,
                "description must be visible without hover"
            );
            assert!(label.bounds().height > super::SETTING_ROW_HEIGHT as f32);
            let control = node.children()[1].bounds();
            assert_eq!(control.width, 120.0);
            assert_eq!(control.height, super::design::BUTTON_HEIGHT as f32);
            assert!(control.y + control.height <= node.bounds().height);
            heights.push(node.bounds().height);
        }
        assert!(
            heights[1] > heights[0],
            "narrow rows must grow with their descriptions"
        );
    }

    #[test]
    fn search_clear_button_preserves_size_and_emits_empty_query() {
        use iced::advanced::{layout, widget::Tree, Layout, Shell};
        use iced::{mouse, Event, Point, Size};
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut expected_size = None;
        for value in ["", "process"] {
            let mut field = super::search_field(
                super::text_input("Search", value)
                    .padding(super::SEARCH_INPUT_PADDING)
                    .width(280)
                    .on_input(std::convert::identity),
                (!value.is_empty()).then(String::new),
            );
            let mut tree = Tree::new(&field);
            let node = field.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(500.0, 100.0)),
            );
            let bounds = node.bounds();
            assert_eq!(*expected_size.get_or_insert(bounds.size()), bounds.size());
            assert_eq!(bounds.width, 280.0);
            let mut messages = Vec::new();
            for event in [
                mouse::Event::ButtonPressed(mouse::Button::Left),
                mouse::Event::ButtonReleased(mouse::Button::Left),
            ] {
                field.as_widget_mut().update(
                    &mut tree,
                    &Event::Mouse(event),
                    Layout::new(&node),
                    mouse::Cursor::Available(Point::new(bounds.width - 16.0, bounds.height / 2.0)),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut Shell::new(&mut messages),
                    &bounds,
                );
            }
            if value.is_empty() {
                assert!(messages.is_empty());
            } else {
                assert_eq!(messages, vec![String::new()]);
            }
        }
    }

    #[test]
    fn tab_text_bounds_stay_stable_when_selected() {
        use crate::ui::{design, widgets::panel_tab};
        use iced::advanced::{layout, widget::Tree, Layout};
        use iced::Element;
        fn text_bounds(mut view: Element<'_, ()>) -> iced::Rectangle {
            let renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
            let mut tree = Tree::new(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, iced::Size::new(150.0, 40.0)),
            );
            let mut layout = Layout::new(&node);
            while let Some(child) = layout.children().next() {
                layout = child;
            }
            layout.bounds()
        }
        let inactive = text_bounds(panel_tab("Status".into(), false, ()).into());
        let active = text_bounds(panel_tab("Status".into(), true, ()).into());
        assert_eq!(inactive, active);
    }

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
        border: iced::Border {
            color: control_border(theme).scale_alpha(0.75),
            width: 1.0,
            radius: design::CARD_RADIUS.into(),
        },
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
        style.background = Some(
            theme
                .palette()
                .primary
                .scale_alpha(match status {
                    iced::widget::button::Status::Hovered => 0.16,
                    iced::widget::button::Status::Pressed => 0.22,
                    _ => 0.10,
                })
                .into(),
        );
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
    fn button_hierarchy_tracks_accent_and_keeps_tertiary_subdued() {
        for light in [false, true] {
            let theme = |accent| {
                let palette = design::palette(light, Some(accent));
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
                    secondary_button(&red, status).text_color,
                    secondary_button(&blue, status).text_color
                );
                if matches!(status, Status::Hovered | Status::Pressed) {
                    assert_ne!(
                        tertiary_button(&red, status).background,
                        tertiary_button(&blue, status).background
                    );
                } else {
                    assert_eq!(
                        tertiary_button(&red, status).background,
                        tertiary_button(&blue, status).background
                    );
                }
            }
            assert_eq!(
                primary_button(&red, Status::Active).text_color,
                red.extended_palette().primary.base.text
            );
        }
    }

    #[test]
    fn surface_tints_and_control_marks_follow_accent_in_all_themes() {
        use crate::config::AppThemeMode;
        for mode in [
            AppThemeMode::Light,
            AppThemeMode::Dark,
            AppThemeMode::Amoled,
        ] {
            let themes = [
                iced::Color::from_rgb8(240, 80, 100),
                iced::Color::from_rgb8(50, 160, 240),
                iced::Color::BLACK,
                iced::Color::WHITE,
                iced::Color::from_rgb8(255, 235, 20),
            ]
            .map(|accent| {
                if mode == AppThemeMode::Amoled {
                    design::amoled_theme(Some(accent))
                } else {
                    let palette = design::palette(mode == AppThemeMode::Light, Some(accent));
                    Theme::custom_with_fn("test", palette, design::extended_palette)
                }
            });
            if mode != AppThemeMode::Amoled {
                assert_ne!(
                    themes[0].palette().background,
                    themes[1].palette().background
                );
                assert_ne!(
                    navigation_surface(&themes[0]).background,
                    navigation_surface(&themes[1]).background
                );
            }
            assert_ne!(
                super::surface(&themes[0]).background,
                super::surface(&themes[1]).background
            );
            assert_ne!(
                control_button(&themes[0], Status::Hovered).background,
                control_button(&themes[1], Status::Hovered).background
            );
            assert_ne!(
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
                let field = select_field(theme, iced::widget::pick_list::Status::Active);
                let hover = select_field(theme, iced::widget::pick_list::Status::Hovered);
                assert_eq!(
                    field.background,
                    iced::Background::Color(theme.extended_palette().background.neutral.color)
                );
                assert_ne!(Some(field.background), super::surface(theme).background);
                assert_ne!(field.background, hover.background);
                let iced::Background::Color(hover_color) = hover.background else {
                    panic!("field hover must use an opaque surface");
                };
                assert_eq!(hover_color.a, 1.0);
                let expected = theme.extended_palette().primary.base.text;
                assert_eq!(checkbox(theme).icon_color, expected);
                assert_eq!(toggle(theme).foreground, iced::Background::Color(expected));
            }
        }
    }

    #[test]
    fn interactive_surfaces_distinguish_hover_press_selection_and_disabled() {
        for theme in [Theme::CatppuccinLatte, Theme::CatppuccinMocha] {
            assert_ne!(selected(&theme, Status::Active).background, None);
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
                assert_eq!(active.border.width, hovered.border.width);
                assert_eq!(active.border.width, pressed.border.width);
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
                control_button(&theme, Status::Active).border,
                quiet(&theme, Status::Active).border
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
    use iced::widget::{column, container};
    container(column![
        container(header).padding(16),
        settings_divider(),
        container(body).padding(16).height(Fill),
        settings_divider(),
        container(footer)
            .padding(16)
            .width(Fill)
            .align_x(iced::Right)
    ])
    .width(Fill)
    .max_width(size.0)
    .height(Fill)
    .max_height(size.1)
    .style(surface)
    .into()
}

pub(super) fn label_with_unit(label: &str, unit: &str) -> String {
    if unit.is_empty() {
        label.to_owned()
    } else {
        format!("{label} ({unit})")
    }
}

// Fit list cells before drawing: overflowing text requires a software clip mask per cell.
pub(super) fn fitted_text(content: &str, width: f32, size: u32) -> String {
    if text_width(content, size) <= width {
        return content.to_owned();
    }
    if text_width("\u{2026}", size) > width {
        return String::new();
    }
    let boundaries: Vec<_> = content.char_indices().map(|(index, _)| index).collect();
    let mut low = 0;
    let mut high = boundaries.len();
    while low < high {
        let middle = (low + high).div_ceil(2);
        let candidate = format!("{}\u{2026}", &content[..boundaries[middle - 1]]);
        if text_width(&candidate, size) <= width {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    if low == 0 {
        "\u{2026}".to_owned()
    } else {
        format!("{}\u{2026}", &content[..boundaries[low - 1]])
    }
}

pub(super) fn text_width(content: &str, size: u32) -> f32 {
    use iced::advanced::text::{Paragraph, Renderer, Text};
    <iced::Renderer as Renderer>::Paragraph::with_text(Text {
        content,
        bounds: iced::Size::INFINITE,
        size: (size as f32).into(),
        line_height: Default::default(),
        font: design::typography::FONT,
        align_x: Default::default(),
        align_y: iced::alignment::Vertical::Center,
        shaping: iced::advanced::text::Shaping::Advanced,
        wrapping: iced::advanced::text::Wrapping::None,
    })
    .min_width()
}

#[cfg(test)]
mod fitted_text_tests {
    use super::*;
    #[test]
    fn cells_fit_narrow_columns_without_losing_full_width_text() {
        for original in [
            "long process name.exe",
            "\u{7a0b}\u{5e8f}\u{540d}\u{7a31}.exe",
            "\u{1f680} application",
            "",
        ] {
            for width in [0.0, 5.0, 24.0, 80.0, 400.0] {
                let fitted = fitted_text(original, width, design::typography::BODY);
                assert!(text_width(&fitted, design::typography::BODY) <= width);
                if text_width(original, design::typography::BODY) <= width {
                    assert_eq!(fitted, original);
                } else if text_width("\u{2026}", design::typography::BODY) <= width {
                    let prefix = fitted.strip_suffix('\u{2026}').expect("ellipsis suffix");
                    assert!(original.starts_with(prefix));
                } else {
                    assert!(fitted.is_empty());
                }
            }
        }
    }
}
