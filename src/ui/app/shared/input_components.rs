use crate::ui::app::*;

pub(in crate::ui::app) fn app_input(
    input: &Entity<InputState>,
    focused: bool,
    cx: &mut Context<WinderustApp>,
) -> gpui::Div {
    div()
        .w_full()
        .h(px(32.0))
        .flex()
        .flex_col()
        .relative()
        .overflow_hidden()
        .rounded(px(BRAND_RADIUS_CONTROL))
        .border_1()
        .border_color(rgb(app_input_border_color(focused)))
        .bg(rgb(app_input_color(focused)))
        .hover(|style| style.border_color(rgb(app_input_hover_border_color())))
        .child(
            Input::new(input)
                .appearance(false)
                .bordered(false)
                .focus_bordered(false)
                .w_full()
                .h_full()
                .text_color(cx.theme().foreground)
                .into_any_element(),
        )
}

pub(in crate::ui::app) fn app_input_color(focused: bool) -> u32 {
    if focused {
        settings_card_hover_color()
    } else {
        settings_card_color()
    }
}

pub(in crate::ui::app) fn app_input_border_color(focused: bool) -> u32 {
    if ui_is_dark() {
        if focused {
            0x5c5c5c
        } else {
            COLOR_BORDER
        }
    } else if focused {
        0x757575
    } else {
        0xdedede
    }
}

pub(in crate::ui::app) fn app_input_hover_border_color() -> u32 {
    if ui_is_dark() {
        0x6a6a6a
    } else {
        0x9a9a9a
    }
}

pub(in crate::ui::app) fn syncing_rule_card(index: usize) -> AnyElement {
    section_card(&t!("common.rule", number = index + 1))
        .child(syncing_input_message())
        .into_any_element()
}

pub(in crate::ui::app) fn rule_card_title(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        t!("common.unnamed_rule").to_string()
    } else {
        name.to_owned()
    }
}

pub(in crate::ui::app) fn status_pill(
    label: impl Into<SharedString>,
    bg: u32,
    fg: u32,
) -> AnyElement {
    status_pill_div(label, bg, fg).into_any_element()
}

pub(in crate::ui::app) fn status_pill_with_tooltip(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    bg: u32,
    fg: u32,
    tooltip: impl Into<SharedString>,
) -> AnyElement {
    let tooltip = tooltip.into();
    status_pill_div(label, bg, fg)
        .id(id.into())
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .into_any_element()
}

pub(in crate::ui::app) fn status_pill_div(
    label: impl Into<SharedString>,
    bg: u32,
    fg: u32,
) -> gpui::Div {
    let label: SharedString = label.into();

    div()
        .flex_shrink_0()
        .px_2()
        .py(px(2.0))
        .rounded(px(BRAND_RADIUS_CONTROL))
        .bg(rgb(bg))
        .text_color(rgb(fg))
        .text_size(px(TEXT_LABEL_SIZE))
        .line_height(px(TEXT_LABEL_LINE_HEIGHT))
        .child(label)
}

pub(in crate::ui::app) fn animated_checkmark(
    id: impl Into<SharedString>,
    check_color: u32,
    progress: f32,
) -> AnyElement {
    let id = id.into();
    let progress = progress.clamp(0.0, 1.0);
    div()
        .id(id)
        .text_size(px(TEXT_LABEL_SIZE))
        .line_height(px(TEXT_LABEL_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(check_color))
        .opacity(progress)
        .mt(px(-3.0 * (1.0 - progress)))
        .child("\u{2713}")
        .into_any_element()
}

pub(in crate::ui::app) fn checkbox_box(
    id: impl Into<SharedString>,
    size: f32,
    mark_id: SharedString,
    check_color: u32,
    progress: f32,
) -> AnyElement {
    let id = id.into();
    let progress = progress.clamp(0.0, 1.0);
    let accent = accent_color();
    let unchecked_border = border_color();
    let unchecked_bg = settings_card_color();
    let checked_bg = accent;
    let border = lerp_rgb(unchecked_border, accent, progress);
    let bg = lerp_rgb(unchecked_bg, checked_bg, progress);
    let mut box_el = div()
        .id(id)
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .rounded(px(BRAND_RADIUS_CONTROL))
        .border_1()
        .border_color(rgb(border))
        .bg(rgb(bg));

    if progress > 0.001 {
        box_el = box_el.child(animated_checkmark(mark_id, check_color, progress));
    }

    box_el.into_any_element()
}

pub(in crate::ui::app) fn rule_enable_checkbox(
    id: impl Into<SharedString>,
    checked: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let accent = accent_color();
    let check_color = accent_glyph_color(accent);
    let mark_id = SharedString::from(format!("{id}-mark"));
    let motion_id = format!("checkbox-{id}");
    let progress = control_motion_progress(&motion_id, checked);

    div()
        .id(id.clone())
        .size(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .rounded(px(BRAND_RADIUS_CONTROL))
        .hover(|style| style.opacity(0.86))
        .cursor_pointer()
        .child(checkbox_box(
            SharedString::from(format!("{id}-box")),
            16.0,
            mark_id,
            check_color,
            progress,
        ))
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            let next = !checked;
            begin_control_motion(motion_id.clone(), next, cx);
            handler(&next, window, cx);
        })
        .into_any_element()
}

pub(in crate::ui::app) fn syncing_input_message() -> gpui::Div {
    text_muted(t!("common.syncing_rule_editor").to_string())
}

pub(in crate::ui::app) fn checkbox(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    checked: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let label = label.into();
    let accent = accent_color();
    let text_color = if checked {
        primary_text_color()
    } else {
        muted_text_color()
    };
    let check_color = accent_glyph_color(accent);
    let handler = Rc::new(handler);
    let box_handler = handler.clone();
    let label_handler = handler;
    let mark_id = SharedString::from(format!("{id}-mark"));
    let motion_id = format!("checkbox-{id}");
    let progress = control_motion_progress(&motion_id, checked);
    let box_motion_id = motion_id.clone();
    let label_motion_id = motion_id;

    h_flex()
        .w_full()
        .min_w(px(0.0))
        .child(
            h_flex()
                .id(id.clone())
                .flex_none()
                .items_center()
                .gap_2()
                .py_1()
                .px_1()
                .rounded(px(BRAND_RADIUS_CONTROL))
                .text_color(rgb(text_color))
                .text_size(px(TEXT_BODY_SIZE))
                .line_height(px(TEXT_BODY_LINE_HEIGHT))
                .child(
                    div()
                        .id(SharedString::from(format!("{id}-box-hitbox")))
                        .hover(|style| style.opacity(0.86))
                        .cursor_pointer()
                        .child(checkbox_box(
                            SharedString::from(format!("{id}-box")),
                            16.0,
                            mark_id,
                            check_color,
                            progress,
                        ))
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            let next = !checked;
                            begin_control_motion(box_motion_id.clone(), next, cx);
                            box_handler(&next, window, cx);
                        }),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("{id}-label")))
                        .hover(|style| style.opacity(0.86))
                        .cursor_pointer()
                        .child(label)
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            let next = !checked;
                            begin_control_motion(label_motion_id.clone(), next, cx);
                            label_handler(&next, window, cx);
                        }),
                ),
        )
        .into_any_element()
}
