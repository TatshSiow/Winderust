use crate::ui::app::*;

pub(in crate::ui::app) fn rule_list(headers: Vec<AnyElement>) -> gpui::Div {
    let mut header = h_flex()
        .w_full()
        .min_w(px(0.0))
        .h(px(32.0))
        .items_center()
        .gap_2()
        .px_4()
        .border_b_1()
        .border_color(rgb(border_color()))
        .text_size(px(TEXT_LABEL_SIZE))
        .line_height(px(TEXT_LABEL_LINE_HEIGHT))
        .text_color(rgb(muted_text_color()));
    for cell in headers {
        header = header.child(cell);
    }

    v_flex()
        .w_full()
        .min_w(px(0.0))
        .overflow_hidden()
        .rounded(px(BRAND_RADIUS_SURFACE))
        .border_1()
        .border_color(rgb(border_color()))
        .bg(rgb(settings_card_color()))
        .child(header)
}

pub(in crate::ui::app) fn rule_table_active_header() -> AnyElement {
    rule_table_centered_header("Active".to_string(), SUSPENSION_ACTIVE_COLUMN_WIDTH)
}

pub(in crate::ui::app) fn rule_table_title_header(title: impl Into<SharedString>) -> AnyElement {
    div()
        .flex_1()
        .min_w(px(0.0))
        .truncate()
        .child(title.into())
        .into_any_element()
}

pub(in crate::ui::app) fn rule_table_title_input_header(
    title: impl Into<SharedString>,
) -> AnyElement {
    div()
        .flex_1()
        .min_w(px(160.0))
        .pl_3()
        .truncate()
        .child(title.into())
        .into_any_element()
}

pub(in crate::ui::app) fn rule_table_title_input_cell(input: gpui::Div) -> AnyElement {
    div()
        .flex_1()
        .min_w(px(160.0))
        .child(input)
        .into_any_element()
}

pub(in crate::ui::app) fn rule_table_centered_header(
    title: impl Into<SharedString>,
    width: f32,
) -> AnyElement {
    div()
        .w(px(width))
        .min_w(px(0.0))
        .flex_shrink_0()
        .text_align(gpui::TextAlign::Center)
        .truncate()
        .child(title.into())
        .into_any_element()
}

pub(in crate::ui::app) fn rule_table_left_header(
    title: impl Into<SharedString>,
    width: f32,
) -> AnyElement {
    div()
        .w(px(width))
        .min_w(px(0.0))
        .flex_shrink_0()
        .truncate()
        .child(title.into())
        .into_any_element()
}

pub(in crate::ui::app) fn rule_table_action_header() -> AnyElement {
    rule_table_centered_header("Actions".to_string(), SUSPENSION_ACTION_COLUMN_WIDTH)
}

pub(in crate::ui::app) fn rule_table_action_cell(action: AnyElement) -> AnyElement {
    h_flex()
        .w(px(SUSPENSION_ACTION_COLUMN_WIDTH))
        .min_w(px(0.0))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .child(action)
        .into_any_element()
}

pub(in crate::ui::app) fn process_rule_table_headers() -> Vec<AnyElement> {
    vec![
        rule_table_active_header(),
        rule_table_title_header(t!("process_list.process_name").to_string()),
        rule_table_action_header(),
    ]
}

pub(in crate::ui::app) fn feature_body(_enabled: bool) -> gpui::Div {
    v_flex().w_full().min_w(px(0.0)).gap_2().relative()
}

pub(in crate::ui::app) fn disabled_feature_body(
    id: impl Into<SharedString>,
    body: gpui::Div,
    enabled: bool,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    const DISABLED_FEATURE_DIM_OPACITY: f32 = 0.58;

    let id = id.into();
    let id_key = id.to_string();
    let target_opacity = if enabled {
        0.0
    } else {
        DISABLED_FEATURE_DIM_OPACITY
    };
    let previous_enabled = DISABLED_FEATURE_STATES
        .lock()
        .ok()
        .and_then(|mut states| states.insert(id_key, enabled));
    let dim_layer = if previous_enabled.is_some_and(|previous| previous != enabled) {
        let start_opacity = if enabled {
            DISABLED_FEATURE_DIM_OPACITY
        } else {
            0.0
        };
        with_optional_motion(
            div().absolute().inset_0().bg(cx.theme().background),
            SharedString::from(format!("feature-gray-layer-{id}-{enabled}")),
            MotionSpeed::Standard,
            move |layer| layer.opacity(target_opacity),
            move |layer, delta| {
                let opacity = start_opacity + (target_opacity - start_opacity) * delta;
                layer.opacity(opacity)
            },
        )
    } else {
        div()
            .absolute()
            .inset_0()
            .bg(cx.theme().background)
            .opacity(target_opacity)
            .into_any_element()
    };
    let body = body
        .child(dim_layer)
        .when(!enabled, |body| body.child(disabled_interaction_shield(cx)));

    body.into_any_element()
}

pub(in crate::ui::app) fn disabled_interaction_shield(
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .capture_any_mouse_down(cx.listener(|app, event: &gpui::MouseDownEvent, _, cx| {
            if !handle_navigation_mouse_button(app, event.button, cx) {
                cx.stop_propagation();
            }
        }))
        .capture_any_mouse_up(|event, _, cx| {
            if !matches!(event.button, MouseButton::Navigate(_)) {
                cx.stop_propagation();
            }
        })
        .into_any_element()
}

pub(in crate::ui::app) fn rule_card_body_row(children: Vec<AnyElement>) -> gpui::Div {
    let mut row = v_flex().w_full().min_w(px(0.0));
    for child in children {
        row = row.child(child);
    }
    row
}

pub(in crate::ui::app) fn rule_card_body_action(action: AnyElement) -> gpui::Div {
    rule_card_body_actions(vec![action])
}

pub(in crate::ui::app) fn rule_card_body_actions(actions: Vec<AnyElement>) -> gpui::Div {
    let mut row = h_flex().items_center().justify_end().gap_2();
    for action in actions {
        row = row.child(action);
    }

    h_flex()
        .w_full()
        .min_w(px(0.0))
        .h(px(CARD_ROW_HEIGHT))
        .items_center()
        .justify_end()
        .gap_2()
        .px_4()
        .py_3()
        .child(row)
}

pub(in crate::ui::app) fn rename_rule_button(
    target: RuleTitleTarget,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    control_button(Button::new(SharedString::from(format!(
        "rename-rule-{target:?}"
    ))))
    .icon(Icon::new(NavIcon::SquarePen).with_size(px(14.0)))
    .label(t!("common.rename").to_string())
    .tooltip(t!("common.rename_rule").to_string())
    .on_click(cx.listener(move |app, _, window, cx| {
        app.begin_rule_title_edit(target, window, cx);
    }))
    .into_any_element()
}

pub(in crate::ui::app) fn compact_rule_row(
    id: impl Into<SharedString>,
) -> gpui::Stateful<gpui::Div> {
    h_flex()
        .id(id.into())
        .w_full()
        .min_w(px(0.0))
        .h(px(CARD_ROW_HEIGHT))
        .items_center()
        .justify_between()
        .gap_2()
        .py_3()
        .px_4()
        .relative()
        .overflow_hidden()
        .border_b_1()
        .border_color(rgb(border_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
}

pub(in crate::ui::app) fn priority_exclusion_table_cell(
    value: impl Into<SharedString>,
) -> AnyElement {
    div()
        .w(px(DROPDOWN_SELECT_TABLE_WIDTH))
        .min_w(px(0.0))
        .flex_shrink_0()
        .text_align(gpui::TextAlign::Center)
        .truncate()
        .child(value.into())
        .into_any_element()
}

pub(in crate::ui::app) fn rule_active_cell(
    id: impl Into<SharedString>,
    checked: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    h_flex()
        .w(px(SUSPENSION_ACTIVE_COLUMN_WIDTH))
        .min_w(px(0.0))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .child(rule_enable_checkbox(id, checked, handler))
        .into_any_element()
}

pub(in crate::ui::app) fn rule_table_input_cell(
    input: Entity<InputState>,
    width: f32,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
) -> gpui::Div {
    let focused = input.read(cx).focus_handle(cx).is_focused(window);
    div()
        .w(px(width))
        .min_w(px(0.0))
        .flex_shrink_0()
        .child(app_input(&input, focused, cx))
}

pub(in crate::ui::app) fn rule_table_checkbox_cell(
    id_prefix: &'static str,
    index: usize,
    checked: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    h_flex()
        .w(px(SUSPENSION_DETECT_COLUMN_WIDTH))
        .min_w(px(0.0))
        .flex_shrink_0()
        .justify_center()
        .child(rule_enable_checkbox(
            format!("{id_prefix}-{index}-check"),
            checked,
            handler,
        ))
}

pub(in crate::ui::app) fn create_rule_card(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    action: AnyElement,
) -> gpui::Stateful<gpui::Div> {
    setting_action_card(id, title, action)
}
