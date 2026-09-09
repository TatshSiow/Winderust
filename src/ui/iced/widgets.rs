use crate::power::PowerPlan;
use iced::widget::{pick_list, row, slider, text, text_input};
use iced::{Element, Fill};

pub(super) fn settings_card<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
) -> iced::widget::Container<'a, M> {
    iced::widget::container(content)
        .padding(12)
        .width(Fill)
        .style(surface)
}

pub(super) fn setting_group<'a, M: Clone + 'a>(
    label: String,
    expanded: bool,
    message: M,
    action: impl Into<Element<'a, M>>,
    content: impl Into<Element<'a, M>>,
    motion_enabled: bool,
) -> Element<'a, M> {
    settings_card(
        iced::widget::column![
            iced::widget::button(
                row![
                    iced::widget::container(setting_title(&label)).width(Fill),
                    action.into(),
                    super::navigation::glyph(if expanded {
                        "icons/chevron-down.svg"
                    } else {
                        "icons/chevron-right.svg"
                    })
                ]
                .spacing(8)
                .height(34)
                .align_y(iced::Center)
            )
            .width(Fill)
            .padding(0)
            .style(quiet)
            .on_press(message),
            super::motion::reveal(content, expanded, motion_enabled)
        ]
        .spacing(8),
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
    .spacing(12)
    .height(34)
    .align_y(iced::Center)
}

pub(super) fn setting_title<'a, M: 'a>(key: &str) -> Element<'a, M> {
    let help_key = key.strip_suffix(".enable").map_or_else(
        || format!("{key}_help"),
        |prefix| format!("{prefix}.intro_1"),
    );
    let help = rust_i18n::t!(&help_key).to_string();
    let mut label = row![heading(rust_i18n::t!(key).to_string(), 14)]
        .spacing(8)
        .align_y(iced::Center);
    if help != help_key {
        label = label.push(iced::widget::tooltip(
            super::navigation::glyph("icons/info.svg"),
            iced::widget::container(text(help).width(320))
                .padding(12)
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
            .size(20)
            .on_toggle_maybe(action)
    ]
    .spacing(8)
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
        iced::widget::button(text("-"))
            .style(control_button)
            .width(32)
            .on_press_maybe(lower.zip(action.as_ref()).map(|(v, f)| f(v.to_string()))),
        text_input("", value)
            .align_x(iced::alignment::Horizontal::Center)
            .on_input_maybe(action.clone())
            .width(80),
        text(unit.to_owned()).width(24),
        iced::widget::button(text("+"))
            .style(control_button)
            .width(32)
            .on_press_maybe(upper.zip(action.as_ref()).map(|(v, f)| f(v.to_string())))
    ]
    .spacing(6)
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

pub(super) fn number<M: Clone + 'static>(
    label: String,
    value: u32,
    range: std::ops::RangeInclusive<u32>,
    action: impl Fn(String) -> M + Clone + 'static,
) -> Element<'static, M> {
    let input_action = action.clone();
    settings_card(
        row![
            text(label).width(iced::Length::FillPortion(2)),
            slider(range.clone(), value, move |value| action(value.to_string()))
                .width(iced::Length::FillPortion(3)),
            stepper(
                &value.to_string(),
                u64::from(*range.start())..=u64::from(*range.end()),
                1,
                "",
                Some(input_action)
            )
        ]
        .spacing(12)
        .align_y(iced::Center),
    )
    .into()
}

// Theme-derived surfaces and interaction states; controls remain standard Iced widgets.
pub(super) fn heading<'a>(label: String, size: u32) -> iced::widget::Text<'a> {
    text(label).size(size).font(iced::Font {
        weight: iced::font::Weight::Semibold,
        ..iced::Font::with_name("Segoe UI")
    })
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
        border: iced::border::rounded(6),
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
    style.border.radius = 4.into();
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
