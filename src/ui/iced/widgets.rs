use super::design;
use crate::power::PowerPlan;
use iced::widget::{row, text};
use iced::{Element, Fill};

pub(super) const CARD_GAP: u32 = design::space::SMALL;
pub(super) const CARD_PADDING: u32 = design::space::MEDIUM;
pub(super) const SETTING_ROW_HEIGHT: u32 = 34;
pub(super) const CARD_HEIGHT: u32 = SETTING_ROW_HEIGHT + 2 * CARD_PADDING;

pub(super) fn button<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> iced::widget::Button<'a, M> {
    iced::widget::button(content).padding(design::CONTROL_PADDING)
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
}

pub(super) fn pick_list<'a, T, L, V, M>(
    options: L,
    selected: Option<V>,
    on_selected: impl Fn(T) -> M + 'a,
) -> iced::widget::PickList<'a, T, L, V, M>
where
    T: ToString + PartialEq + Clone + 'a,
    L: std::borrow::Borrow<[T]> + 'a,
    V: std::borrow::Borrow<T> + 'a,
    M: Clone + 'a,
{
    iced::widget::pick_list(options, selected, on_selected)
        .text_size(design::typography::BODY)
        .padding(design::CONTROL_PADDING)
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
    content: impl Into<Element<'a, M>>,
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
            .padding(0)
            .style(quiet)
            .on_press(message),
            optional_content(content, expanded)
        ]
        .spacing(design::space::SMALL),
    )
    .into()
}

pub(super) fn setting_row<'a, M: 'a>(
    key: &str,
    action: impl Into<Element<'a, M>>,
) -> iced::widget::Row<'a, M> {
    row![
        iced::widget::container(setting_title(key)).width(Fill),
        action.into()
    ]
    .spacing(design::space::MEDIUM)
    .height(SETTING_ROW_HEIGHT)
    .align_y(iced::Center)
}

pub(super) fn setting_title<'a, M: 'a>(key: &str) -> Element<'a, M> {
    let help_key = key.strip_suffix(".enable").map_or_else(
        || format!("{key}_help"),
        |prefix| format!("{prefix}.intro_1"),
    );
    let help = rust_i18n::t!(&help_key).to_string();
    let mut label = row![heading(
        rust_i18n::t!(key).to_string(),
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
            .on_toggle_maybe(action)
    ]
    .spacing(design::space::SMALL)
    .align_y(iced::Center)
    .into()
}

pub(super) fn control_button(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let mut style = quiet(theme, status);
    if matches!(status, iced::widget::button::Status::Active) {
        style.background = Some(
            if theme.extended_palette().is_dark {
                iced::Color::from_rgb8(44, 49, 55)
            } else {
                iced::Color::from_rgb8(226, 230, 234)
            }
            .into(),
        );
    }
    style
}

pub(super) fn stepper<'a, M: Clone + 'a>(
    value: &str,
    range: std::ops::RangeInclusive<u64>,
    step: u64,
    unit: &str,
    action: Option<impl Fn(String) -> M + Clone + 'a>,
) -> Element<'a, M> {
    let (lower, upper) = step_values(value, range, step);
    row![
        button(text("-"))
            .style(control_button)
            .width(design::STEPPER_BUTTON_WIDTH)
            .on_press_maybe(lower.zip(action.as_ref()).map(|(v, f)| f(v.to_string()))),
        text_input("", value)
            .align_x(iced::alignment::Horizontal::Center)
            .on_input_maybe(action.clone())
            .width(design::NUMERIC_WIDTH),
        text(unit.to_owned()).width(design::STEPPER_UNIT_WIDTH),
        button(text("+"))
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
        background: Some(
            if theme.extended_palette().is_dark {
                iced::Color::from_rgb8(11, 13, 15)
            } else {
                iced::Color::from_rgb8(234, 236, 239)
            }
            .into(),
        ),
        ..Default::default()
    }
}

pub(super) fn surface(theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(
            if theme.extended_palette().is_dark {
                iced::Color::from_rgb8(25, 27, 30)
            } else {
                iced::Color::WHITE
            }
            .into(),
        ),
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
        let alpha = match status {
            iced::widget::button::Status::Hovered => 0.22,
            iced::widget::button::Status::Pressed => 0.30,
            _ => 0.12,
        };
        style.background = Some(theme.palette().primary.scale_alpha(alpha).into());
        style.text_color = theme.extended_palette().primary.strong.color;
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
        Status::Hovered => Some(
            theme
                .extended_palette()
                .background
                .strong
                .color
                .scale_alpha(0.35)
                .into(),
        ),
        Status::Pressed => Some(theme.palette().primary.scale_alpha(0.18).into()),
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
    if status == Status::Active {
        style.background = surface.background;
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{widget::button::Status, Theme};

    #[test]
    fn interactive_surfaces_distinguish_hover_press_and_disabled_in_both_themes() {
        for theme in [Theme::CatppuccinLatte, Theme::CatppuccinMocha] {
            for style in [quiet, selected, card] {
                let active = style(&theme, Status::Active);
                let hovered = style(&theme, Status::Hovered);
                let pressed = style(&theme, Status::Pressed);
                assert_ne!(active.background, hovered.background);
                assert_ne!(hovered.background, pressed.background);
                assert_eq!(active.border.width, 0.0);
                assert_eq!(hovered.border.width, 0.0);
                assert_eq!(pressed.border.width, 0.0);
                assert_eq!(active.border.radius, hovered.border.radius);
                assert_eq!(style(&theme, Status::Disabled).background, None);
            }
        }
    }
}
