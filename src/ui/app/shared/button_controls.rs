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

pub(in crate::ui::app) fn animated_button_hover(button: Button, id: &'static str) -> AnyElement {
    let (hovered, animation_generation) = card_hover_snapshot(id);
    let button = button.on_hover(move |hovered, _, cx| {
        set_card_hovered(id.to_owned(), *hovered, cx);
    });
    if !ui_animations_enabled() {
        return button.into_any_element();
    }

    let target_opacity = if hovered { 1.0 } else { 0.96 };
    let Some(generation) = animation_generation else {
        return button.opacity(target_opacity).into_any_element();
    };
    let start_opacity = if hovered { 0.96 } else { 1.0 };
    with_optional_motion(
        button,
        SharedString::from(format!("button-hover-{id}-{generation}")),
        MotionSpeed::Fast,
        move |button| button.opacity(target_opacity),
        move |button, delta| {
            button.opacity(start_opacity + (target_opacity - start_opacity) * delta)
        },
    )
}

pub(in crate::ui::app) fn remove_control_button(button: Button) -> Button {
    danger_control_button(button)
        .with_size(px(32.0))
        .icon(Icon::new(NavIcon::Trash2).with_size(px(14.0)))
        .tooltip(t!("common.remove").to_string())
}
