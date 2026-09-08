use crate::power::PowerPlan;
use iced::widget::{pick_list, row, slider, text, text_input};
use iced::{Element, Fill};

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
    row![
        text(label).width(iced::Length::FillPortion(2)),
        slider(range, value, move |value| action(value.to_string()))
            .width(iced::Length::FillPortion(3)),
        text_input("", &value.to_string())
            .on_input(input_action)
            .width(80)
    ]
    .spacing(12)
    .align_y(iced::Center)
    .into()
}
