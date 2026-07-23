use crate::ui::app::*;

pub(in crate::ui::app) fn control_button(button: Button) -> Button {
    button
        .small()
        .h(px(32.0))
        .text_size(px(TEXT_CONTROL_SIZE))
        .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
}

pub(in crate::ui::app) fn primary_control_button(
    button: Button,
    cx: &mut Context<WinderustApp>,
) -> Button {
    control_button(button.primary()).text_color(cx.theme().primary_foreground)
}

pub(in crate::ui::app) fn danger_control_button(button: Button) -> Button {
    control_button(button.danger()).text_color(rgb(0xffffff))
}

pub(in crate::ui::app) fn remove_control_button(button: Button) -> Button {
    danger_control_button(button)
        .with_size(px(32.0))
        .icon(Icon::new(NavIcon::Trash2).with_size(px(14.0)))
        .tooltip(t!("common.remove").to_string())
}
