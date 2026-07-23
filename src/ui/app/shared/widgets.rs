use crate::ui::app::*;

pub(in crate::ui::app) fn action_log_page_help() -> SharedString {
    tooltip_lines(vec![
        t!("action_log.intro_1").to_string(),
        t!("action_log.intro_2").to_string(),
    ])
}

pub(in crate::ui::app) fn page_header_with_help(
    page: Page,
    help: Option<SharedString>,
    transition: Option<&BreadcrumbTransition>,
    cx: &mut Context<WinderustApp>,
) -> gpui::Div {
    let mut header = h_flex()
        .w_full()
        .min_h(px(PAGE_HEADER_HEIGHT))
        .flex_shrink_0()
        .items_center()
        .gap_2()
        .overflow_hidden();
    let mut breadcrumb_row = h_flex()
        .w_full()
        .min_w(px(0.0))
        .items_center()
        .gap_2()
        .overflow_hidden();

    let current_trail = breadcrumb_trail(page);
    let transition = transition.filter(|transition| transition.current == current_trail);
    let entering_start = transition
        .map(|transition| common_breadcrumb_prefix_len(&transition.previous, &current_trail))
        .unwrap_or(current_trail.len());

    if let Some(first) = current_trail.first() {
        breadcrumb_row = breadcrumb_row.child(breadcrumb_segment_element(
            first,
            current_trail.len() == 1,
            true,
            cx,
        ));
    }

    for (index, segment) in current_trail.iter().enumerate().skip(1) {
        let current = index + 1 == current_trail.len();
        let group = breadcrumb_segment_group(segment, current, true, cx);

        if transition.is_some() && index >= entering_start {
            breadcrumb_row = breadcrumb_row.child(breadcrumb_transition_group(
                SharedString::from(format!("breadcrumb-{:?}-{index}", segment.page)),
                true,
                group,
            ));
        } else {
            breadcrumb_row = breadcrumb_row.child(group);
        }
    }

    let mut breadcrumbs = div()
        .flex_1()
        .min_w(px(0.0))
        .relative()
        .overflow_hidden()
        .child(breadcrumb_row);

    if let Some(transition) = transition {
        if breadcrumb_starts_with(&transition.previous, &current_trail)
            && transition.previous.len() > current_trail.len()
        {
            breadcrumbs =
                breadcrumbs.child(breadcrumb_exit_overlay(transition, current_trail.len(), cx));
        }
    }

    header = header.child(breadcrumbs);

    if let Some(help) = help {
        header = header.child(title_info_button(
            SharedString::from(format!("page-info-{page:?}")),
            help,
        ));
    }

    header
}

pub(in crate::ui::app) fn tooltip_lines(
    lines: impl IntoIterator<Item = impl Into<SharedString>>,
) -> SharedString {
    let mut tooltip = String::new();
    for line in lines {
        let line: SharedString = line.into();
        if !tooltip.is_empty() {
            tooltip.push('\n');
        }
        tooltip.push_str(line.as_ref());
    }
    tooltip.into()
}

pub(in crate::ui::app) fn branded_panel() -> gpui::Div {
    v_flex()
        .w_full()
        .min_w(px(0.0))
        .relative()
        .overflow_hidden()
        .rounded(px(BRAND_RADIUS_SURFACE))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
}

pub(in crate::ui::app) fn section_card(title: &str) -> gpui::Div {
    branded_panel()
        .gap_3()
        .p_4()
        .child(section_title_text(title.to_owned()))
}

pub(in crate::ui::app) fn section_header(title: &str, help: impl Into<SharedString>) -> gpui::Div {
    let help = help.into();

    v_flex().w_full().min_w(px(0.0)).child(
        h_flex()
            .w_full()
            .min_h(px(26.0))
            .min_w(px(0.0))
            .items_center()
            .gap_1()
            .child(section_title_text(title.to_owned()))
            .child(title_info_button(
                SharedString::from(format!("section-info-{title}")),
                help,
            )),
    )
}

pub(in crate::ui::app) fn section_title_label(title: impl Into<SharedString>) -> Label {
    Label::new(title)
        .w_full()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::BOLD)
}

pub(in crate::ui::app) fn section_title_text(title: impl Into<SharedString>) -> Label {
    Label::new(title)
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::BOLD)
}

pub(in crate::ui::app) fn title_info_button(
    id: impl Into<SharedString>,
    tooltip: impl Into<SharedString>,
) -> AnyElement {
    div()
        .size(px(26.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .child(
            Button::new(id.into())
                .ghost()
                .rounded(px(999.0))
                .with_size(px(26.0))
                .icon(
                    Icon::new(NavIcon::Info)
                        .with_size(px(14.0))
                        .text_color(rgb(dim_text_color())),
                )
                .tooltip(tooltip),
        )
        .into_any_element()
}

pub(in crate::ui::app) fn rule_card(
    title: AnyElement,
    leading: AnyElement,
    collapse_indicator: AnyElement,
    card_target: RuleCardTarget,
    collapsed: bool,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    rule_card_with_header_action(
        title,
        leading,
        None,
        collapse_indicator,
        card_target,
        collapsed,
        cx,
    )
}

pub(in crate::ui::app) fn rule_card_with_header_action(
    title: AnyElement,
    leading: AnyElement,
    header_action: Option<AnyElement>,
    collapse_indicator: AnyElement,
    card_target: RuleCardTarget,
    _collapsed: bool,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    let header_padding = if header_action.is_some() {
        px(134.0)
    } else {
        px(52.0)
    };
    let card_id = SharedString::from(format!("rule-card-{card_target:?}"));
    let header_id = SharedString::from(format!("rule-card-header-{card_target:?}"));
    let header_action_id = SharedString::from(format!("rule-card-header-action-{card_target:?}"));
    let hover_id = format!("rule-card-hover-{card_target:?}");
    let header_card_target = card_target.clone();
    let trailing_card_target = card_target.clone();
    let trailing_hover_id = hover_id.clone();
    let mut trailing = h_flex()
        .id(SharedString::from(format!(
            "rule-card-trailing-{card_target:?}"
        )))
        .absolute()
        .top(px(0.0))
        .right(px(0.0))
        .h(px(CARD_ROW_HEIGHT))
        .items_center()
        .gap_1()
        .px_2()
        .block_mouse_except_scroll()
        .cursor_pointer()
        .capture_any_mouse_down(cx.listener(|app, event: &gpui::MouseDownEvent, _, cx| {
            handle_navigation_mouse_button(app, event.button, cx);
        }))
        .on_hover(move |hovered, _, cx| {
            set_card_hovered(trailing_hover_id.clone(), *hovered, cx);
        })
        .on_click(cx.listener(move |app, _, _, cx| {
            app.toggle_rule_card(trailing_card_target.clone(), cx);
        }));
    if let Some(header_action) = header_action {
        trailing = trailing.child(header_action);
    }
    trailing = trailing.child(collapse_indicator);

    v_flex()
        .id(card_id)
        .w_full()
        .min_w(px(0.0))
        .relative()
        .overflow_hidden()
        .border_b_1()
        .border_color(rgb(border_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .child(
            div()
                .relative()
                .w_full()
                .min_w(px(0.0))
                .h(px(CARD_ROW_HEIGHT))
                .id(header_id)
                .overflow_hidden()
                .child(animated_card_hover_layer(&hover_id))
                .child(
                    h_flex()
                        .w_full()
                        .min_w(px(0.0))
                        .h(px(CARD_ROW_HEIGHT))
                        .items_center()
                        .gap_2()
                        .pl_4()
                        .pr(header_padding)
                        .id(header_action_id)
                        .block_mouse_except_scroll()
                        .cursor_pointer()
                        .capture_any_mouse_down(cx.listener(
                            |app, event: &gpui::MouseDownEvent, _, cx| {
                                handle_navigation_mouse_button(app, event.button, cx);
                            },
                        ))
                        .on_hover({
                            let hover_id = hover_id.clone();
                            move |hovered, _, cx| {
                                set_card_hovered(hover_id.clone(), *hovered, cx);
                            }
                        })
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.toggle_rule_card(header_card_target.clone(), cx);
                        }))
                        .child(leading)
                        .child(title),
                )
                .child(trailing),
        )
}

pub(in crate::ui::app) fn rule_card_collapse_indicator(
    card_target: RuleCardTarget,
    collapsed: bool,
) -> AnyElement {
    div()
        .w(px(28.0))
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(dim_text_color()))
        .opacity(0.72)
        .cursor_pointer()
        .child(collapsible_chevron_icon(
            rule_card_body_motion_id(&card_target),
            collapsed,
        ))
        .into_any_element()
}

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

pub(in crate::ui::app) fn setting_group(
    target: SettingGroupTarget,
    title: impl Into<SharedString>,
    action: AnyElement,
    collapsed: bool,
    rows: Vec<AnyElement>,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    let title: SharedString = title.into();
    setting_group_with_title_element(
        target,
        div()
            .flex_1()
            .min_w(px(0.0))
            .truncate()
            .child(title)
            .into_any_element(),
        action,
        collapsed,
        rows,
        window,
        cx,
    )
}

pub(in crate::ui::app) fn setting_group_with_help(
    target: SettingGroupTarget,
    title_help: (impl Into<SharedString>, impl Into<SharedString>),
    action: AnyElement,
    collapsed: bool,
    rows: Vec<AnyElement>,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    let (title, help) = title_help;
    let title: SharedString = title.into();
    setting_group_with_title_element(
        target,
        h_flex()
            .flex_1()
            .min_w(px(0.0))
            .gap_1()
            .items_center()
            .child(div().truncate().child(title))
            .child(title_info_button(
                SharedString::from(format!("setting-group-info-{target:?}")),
                help,
            ))
            .into_any_element(),
        action,
        collapsed,
        rows,
        window,
        cx,
    )
}

pub(in crate::ui::app) fn setting_group_with_title_element(
    target: SettingGroupTarget,
    title: AnyElement,
    action: AnyElement,
    collapsed: bool,
    rows: Vec<AnyElement>,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    setting_group_with_title_element_with_body_height(
        target,
        title,
        action,
        SettingGroupBody {
            collapsed,
            rows,
            animation_height: None,
        },
        window,
        cx,
    )
}

pub(in crate::ui::app) fn setting_group_with_title_element_with_body_height(
    target: SettingGroupTarget,
    title: AnyElement,
    action: AnyElement,
    body: SettingGroupBody,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    let SettingGroupBody {
        collapsed,
        rows,
        animation_height,
    } = body;
    let chevron_target = target;
    let hover_id = format!("setting-group-hover-{target:?}");
    let motion_id = format!("setting-group-{target:?}");
    let motion_progress = expandable_motion_progress(&motion_id);
    if motion_progress.is_some() {
        window.request_animation_frame();
    }
    let mut group = v_flex()
        .id(SharedString::from(format!("setting-group-{target:?}")))
        .w_full()
        .min_w(px(0.0))
        .relative()
        .overflow_hidden()
        .rounded(px(BRAND_RADIUS_SURFACE))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .child(
            h_flex()
                .id(SharedString::from(format!(
                    "setting-group-header-{target:?}"
                )))
                .w_full()
                .min_w(px(0.0))
                .h(px(CARD_ROW_HEIGHT))
                .items_center()
                .justify_between()
                .gap_2()
                .py_3()
                .pl_4()
                .pr_2()
                .relative()
                .overflow_hidden()
                .block_mouse_except_scroll()
                .cursor_pointer()
                .capture_any_mouse_down(cx.listener(|app, event: &gpui::MouseDownEvent, _, cx| {
                    handle_navigation_mouse_button(app, event.button, cx);
                }))
                .on_hover({
                    let hover_id = hover_id.clone();
                    move |hovered, _, cx| {
                        set_card_hovered(hover_id.clone(), *hovered, cx);
                    }
                })
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.toggle_setting_group(target, cx);
                }))
                .child(animated_card_hover_layer(&hover_id))
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "setting-group-title-{target:?}"
                        )))
                        .flex_1()
                        .min_w(px(0.0))
                        .child(title),
                )
                .child(
                    h_flex()
                        .items_center()
                        .justify_end()
                        .gap_1()
                        .min_w(px(0.0))
                        .flex_shrink_0()
                        .child(action)
                        .child(setting_group_collapse_button(chevron_target, collapsed, cx)),
                ),
        );
    if !collapsed || motion_progress.is_some() {
        let row_count = rows.len();
        let mut body = v_flex().w_full().min_w(px(0.0));
        for row in rows {
            body = body.child(row);
        }
        let body_animation_height =
            animation_height.or_else(|| setting_group_body_animation_height(target, row_count));
        group = group.child(if let Some(progress) = motion_progress {
            expanded_child_at_progress(body.into_any_element(), body_animation_height, progress)
        } else if let Some(height) = body_animation_height {
            animated_expanded_child_with_height(
                SharedString::from(format!("setting-group-{target:?}-body")),
                height,
                body,
            )
        } else {
            animated_expanded_child(
                SharedString::from(format!("setting-group-{target:?}-body")),
                body.into_any_element(),
            )
        });
    }
    group
}

pub(in crate::ui::app) fn expanded_child_at_progress(
    child: AnyElement,
    target_height: Option<f32>,
    progress: f32,
) -> AnyElement {
    let progress = progress.clamp(0.0, 1.0);
    let mut body = div()
        .w_full()
        .min_w(px(0.0))
        .child(child)
        .mt(px(-EXPANDED_CHILD_SLIDE_PX * (1.0 - progress)))
        .opacity(0.08 + 0.92 * progress);
    let container = div().w_full().min_w(px(0.0)).overflow_hidden();

    if let Some(target_height) = target_height {
        body = body.h(px(target_height.max(1.0)));
        container
            .h(px(target_height.max(1.0) * progress))
            .child(body)
    } else {
        container
            .max_h(px(EXPANDED_CHILD_MAX_ANIMATION_HEIGHT * progress))
            .child(body)
    }
    .into_any_element()
}

pub(in crate::ui::app) fn setting_group_body_animation_height(
    target: SettingGroupTarget,
    row_count: usize,
) -> Option<f32> {
    match target {
        SettingGroupTarget::AccentColor | SettingGroupTarget::BackgroundCpuRestriction => None,
        _ => Some(CARD_ROW_HEIGHT * row_count.max(1) as f32),
    }
}

pub(in crate::ui::app) fn setting_group_collapse_button(
    target: SettingGroupTarget,
    collapsed: bool,
    _cx: &mut Context<WinderustApp>,
) -> AnyElement {
    div()
        .id(SharedString::from(format!(
            "setting-group-chevron-{target:?}"
        )))
        .w(px(28.0))
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .rounded(px(BRAND_RADIUS_CONTROL))
        .text_color(rgb(dim_text_color()))
        .opacity(0.72)
        .hover(|style| style.opacity(1.0))
        .cursor_pointer()
        .child(collapsible_chevron_icon(
            SharedString::from(format!("setting-group-{target:?}")),
            collapsed,
        ))
        .into_any_element()
}

pub(in crate::ui::app) fn setting_group_switch_action(
    id: impl Into<SharedString>,
    enabled: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    switch_toggle_action(id, enabled, handler)
}

pub(in crate::ui::app) fn setting_group_action_row(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    action: AnyElement,
    divided: bool,
) -> gpui::Stateful<gpui::Div> {
    setting_group_action_row_element(
        id,
        div()
            .flex_1()
            .min_w(px(0.0))
            .truncate()
            .child(title.into())
            .into_any_element(),
        action,
        divided,
    )
}

pub(in crate::ui::app) fn setting_group_action_row_with_help(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    help: impl Into<SharedString>,
    action: AnyElement,
    divided: bool,
) -> gpui::Stateful<gpui::Div> {
    let id: SharedString = id.into();
    let label_id = id.clone();

    setting_group_action_row_element(
        id,
        h_flex()
            .flex_1()
            .min_w(px(0.0))
            .items_center()
            .gap_1()
            .child(div().min_w(px(0.0)).truncate().child(title.into()))
            .child(title_info_button(format!("{label_id}-info"), help))
            .into_any_element(),
        action,
        divided,
    )
}

pub(in crate::ui::app) fn setting_group_action_row_element(
    id: impl Into<SharedString>,
    title: AnyElement,
    action: AnyElement,
    _divided: bool,
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
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .child(title)
        .child(
            h_flex()
                .items_center()
                .justify_end()
                .gap_2()
                .min_w(px(0.0))
                .flex_shrink_0()
                .child(action),
        )
}

pub(in crate::ui::app) fn setting_group_stacked_action_row(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    action: AnyElement,
    _divided: bool,
) -> gpui::Stateful<gpui::Div> {
    v_flex()
        .id(id.into())
        .w_full()
        .min_w(px(0.0))
        .gap_2()
        .py_3()
        .px_4()
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .child(div().w_full().min_w(px(0.0)).child(title.into()))
        .child(
            div()
                .w_full()
                .min_w(px(0.0))
                .overflow_hidden()
                .child(action),
        )
}

pub(in crate::ui::app) fn setting_group_stepper_row_u64(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    value: u64,
    value_element: AnyElement,
    divided: bool,
    handler: impl Fn(&StepChange<u64>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    setting_group_stepper_row_u64_inner(id, title, None, value, value_element, divided, handler)
}

pub(in crate::ui::app) fn setting_group_stepper_row_u64_with_help(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    help: impl Into<SharedString>,
    value: u64,
    value_element: AnyElement,
    divided: bool,
    handler: impl Fn(&StepChange<u64>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    setting_group_stepper_row_u64_inner(
        id,
        title,
        Some(help.into()),
        value,
        value_element,
        divided,
        handler,
    )
}

pub(in crate::ui::app) fn setting_group_stepper_row_u64_inner(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    help: Option<SharedString>,
    value: u64,
    value_element: AnyElement,
    divided: bool,
    handler: impl Fn(&StepChange<u64>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let row_id = id.clone();
    let handler: StepChangeHandler<u64> = Rc::new(handler);
    let down = Rc::clone(&handler);
    let delta = u64_step(value);
    let action = h_flex()
        .items_center()
        .justify_end()
        .gap_2()
        .min_w(px(0.0))
        .flex_shrink_0()
        .child(
            control_button(Button::new((gpui::ElementId::from(id.clone()), "down")))
                .label("-")
                .on_click(move |_, window, cx| {
                    down(
                        &StepChange {
                            delta,
                            increase: false,
                        },
                        window,
                        cx,
                    )
                }),
        )
        .child(value_element)
        .child(
            control_button(Button::new((gpui::ElementId::from(id), "up")))
                .label("+")
                .on_click(move |_, window, cx| {
                    handler(
                        &StepChange {
                            delta,
                            increase: true,
                        },
                        window,
                        cx,
                    )
                }),
        )
        .into_any_element();

    match help {
        Some(help) => setting_group_action_row_with_help(row_id, title, help, action, divided),
        None => setting_group_action_row(row_id, title, action, divided),
    }
    .into_any_element()
}

pub(in crate::ui::app) fn rule_action_row(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    action: AnyElement,
) -> gpui::Stateful<gpui::Div> {
    rule_action_row_with_title_color(id, title, action, primary_text_color())
}

pub(in crate::ui::app) fn rule_action_row_with_title_color(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    action: AnyElement,
    title_color: u32,
) -> gpui::Stateful<gpui::Div> {
    setting_group_action_row_element(
        id,
        div()
            .flex_1()
            .min_w(px(0.0))
            .truncate()
            .text_color(rgb(title_color))
            .child(title.into())
            .into_any_element(),
        action,
        false,
    )
}

pub(in crate::ui::app) fn rule_stepper_row_u64(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    value: u64,
    value_element: AnyElement,
    handler: impl Fn(&StepChange<u64>, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let id: SharedString = id.into();
    let handler: StepChangeHandler<u64> = Rc::new(handler);
    let down = Rc::clone(&handler);
    let delta = u64_step(value);

    rule_action_row(
        id.clone(),
        title,
        h_flex()
            .items_center()
            .justify_end()
            .gap_2()
            .min_w(px(0.0))
            .flex_shrink_0()
            .child(
                control_button(Button::new((gpui::ElementId::from(id.clone()), "down")))
                    .label("-")
                    .on_click(move |_, window, cx| {
                        down(
                            &StepChange {
                                delta,
                                increase: false,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .child(value_element)
            .child(
                control_button(Button::new((gpui::ElementId::from(id), "up")))
                    .label("+")
                    .on_click(move |_, window, cx| {
                        handler(
                            &StepChange {
                                delta,
                                increase: true,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .into_any_element(),
    )
}

pub(in crate::ui::app) fn rule_checkbox_row(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    checked: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let title: SharedString = title.into();
    let handler: BoolChangeHandler = Rc::new(handler);
    let checkbox_handler = Rc::clone(&handler);
    let label_handler = Rc::clone(&handler);

    setting_group_action_row_element(
        id.clone(),
        div()
            .flex_1()
            .min_w(px(0.0))
            .child(
                div()
                    .id(SharedString::from(format!("{id}-label")))
                    .min_w(px(0.0))
                    .truncate()
                    .cursor_pointer()
                    .hover(|style| style.opacity(0.86))
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        let next = !checked;
                        label_handler(&next, window, cx);
                    })
                    .child(title),
            )
            .into_any_element(),
        rule_enable_checkbox(format!("{id}-check"), checked, move |next, window, cx| {
            checkbox_handler(next, window, cx);
        }),
        false,
    )
    .into_any_element()
}

pub(in crate::ui::app) fn setting_action_card(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    action: AnyElement,
) -> gpui::Stateful<gpui::Div> {
    setting_action_card_element(
        id,
        div()
            .flex_1()
            .min_w(px(0.0))
            .truncate()
            .child(title.into())
            .into_any_element(),
        action,
    )
}

pub(in crate::ui::app) fn setting_action_card_with_help(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    help: impl Into<SharedString>,
    action: AnyElement,
) -> gpui::Stateful<gpui::Div> {
    let id: SharedString = id.into();
    setting_action_card_element(
        id.clone(),
        h_flex()
            .flex_1()
            .min_w(px(0.0))
            .gap_1()
            .items_center()
            .child(div().truncate().child(title.into()))
            .child(title_info_button(format!("{id}-info"), help))
            .into_any_element(),
        action,
    )
}

pub(in crate::ui::app) fn setting_action_card_element(
    id: impl Into<SharedString>,
    title: AnyElement,
    action: AnyElement,
) -> gpui::Stateful<gpui::Div> {
    let id: SharedString = id.into();

    h_flex()
        .id(id)
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
        .rounded(px(BRAND_RADIUS_SURFACE))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .child(title)
        .child(
            h_flex()
                .items_center()
                .justify_end()
                .flex_shrink_0()
                .child(action),
        )
}

pub(in crate::ui::app) fn setting_stepper_card_u64(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    value: u64,
    value_element: AnyElement,
    handler: impl Fn(&StepChange<u64>, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let id: SharedString = id.into();
    let handler: StepChangeHandler<u64> = Rc::new(handler);
    let down = Rc::clone(&handler);
    let delta = u64_step(value);

    setting_action_card(
        id.clone(),
        title,
        h_flex()
            .items_center()
            .justify_end()
            .gap_2()
            .flex_shrink_0()
            .child(
                control_button(Button::new((gpui::ElementId::from(id.clone()), "down")))
                    .label("-")
                    .on_click(move |_, window, cx| {
                        down(
                            &StepChange {
                                delta,
                                increase: false,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .child(value_element)
            .child(
                control_button(Button::new((gpui::ElementId::from(id), "up")))
                    .label("+")
                    .on_click(move |_, window, cx| {
                        handler(
                            &StepChange {
                                delta,
                                increase: true,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .into_any_element(),
    )
}

pub(in crate::ui::app) fn stat_grid(rows: Vec<(String, String)>) -> gpui::Div {
    let mut list = v_flex().w_full().min_w(px(0.0));
    for (index, (label, value)) in rows.into_iter().enumerate() {
        list = list.child(
            h_flex()
                .w_full()
                .min_w(px(0.0))
                .min_h(px(36.0))
                .items_center()
                .gap_3()
                .px_4()
                .when(index > 0, |row| {
                    row.border_t_1().border_color(rgb(border_color()))
                })
                .child(
                    div()
                        .w(px(172.0))
                        .flex_shrink_0()
                        .truncate()
                        .text_color(rgb(dim_text_color()))
                        .text_size(px(TEXT_BODY_SIZE))
                        .line_height(px(TEXT_BODY_LINE_HEIGHT))
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_color(rgb(primary_text_color()))
                        .text_size(px(TEXT_BODY_SIZE))
                        .line_height(px(TEXT_BODY_LINE_HEIGHT))
                        .child(value),
                ),
        );
    }
    branded_panel().py_1().child(list)
}

pub(in crate::ui::app) fn dashboard_card_slot(card: AnyElement) -> gpui::Div {
    div()
        .w(relative(0.49))
        .min_w(px(320.0))
        .flex_1()
        .child(card)
}

pub(in crate::ui::app) fn dashboard_summary_card(
    title: impl Into<SharedString>,
    header_trailing: Option<AnyElement>,
    body: AnyElement,
) -> gpui::Div {
    let mut header = h_flex()
        .w_full()
        .min_w(px(0.0))
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .child(section_title_label(title)),
        );

    if let Some(header_trailing) = header_trailing {
        header = header.child(header_trailing);
    }

    v_flex()
        .w_full()
        .min_w(px(0.0))
        .h(px(DASHBOARD_SUMMARY_CARD_HEIGHT))
        .relative()
        .overflow_hidden()
        .p_3()
        .gap_2()
        .rounded(px(BRAND_RADIUS_SURFACE))
        .bg(rgb(settings_card_color()))
        .child(header)
        .child(
            div()
                .w_full()
                .min_w(px(0.0))
                .flex_1()
                .min_h(px(0.0))
                .child(body),
        )
}

pub(in crate::ui::app) fn dashboard_summary_header_value(
    value: impl Into<SharedString>,
) -> gpui::Div {
    div()
        .max_w(px(180.0))
        .flex_shrink_0()
        .truncate()
        .whitespace_nowrap()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(value.into())
}

pub(in crate::ui::app) fn dashboard_split_value_row(items: [gpui::Div; 2]) -> gpui::Div {
    items.into_iter().fold(
        h_flex()
            .w_full()
            .min_w(px(0.0))
            .items_center()
            .justify_center()
            .gap_2()
            .flex_wrap(),
        |row, item| row.child(item),
    )
}

pub(in crate::ui::app) fn dashboard_split_value(
    label: String,
    value: impl Into<SharedString>,
    color: Hsla,
) -> gpui::Div {
    h_flex()
        .w(px(DASHBOARD_SPLIT_ITEM_WIDTH))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .gap_1()
        .text_size(px(TEXT_CAPTION_SIZE))
        .line_height(px(TEXT_CAPTION_LINE_HEIGHT))
        .text_color(rgb(muted_text_color()))
        .child(div().size(px(7.0)).rounded_full().flex_shrink_0().bg(color))
        .child(div().flex_shrink_0().whitespace_nowrap().child(label))
        .child(
            div()
                .w(px(DASHBOARD_SPLIT_VALUE_WIDTH))
                .flex_shrink_0()
                .truncate()
                .whitespace_nowrap()
                .child(value.into()),
        )
}

pub(in crate::ui::app) fn io_usage_split_value(
    label: String,
    value: Option<f64>,
    color: Hsla,
) -> gpui::Div {
    dashboard_split_value(label, io_usage_label(value), color)
}

pub(in crate::ui::app) fn dashboard_graph_hover_overlay(
    graph_id: &'static str,
    tooltips: Vec<SharedString>,
) -> gpui::Div {
    let mut overlay = h_flex().absolute().inset_0().w_full().h_full();
    for (index, tooltip) in tooltips.into_iter().enumerate() {
        overlay = overlay.child(
            div()
                .id(SharedString::from(format!(
                    "{graph_id}-sample-hover-{index}"
                )))
                .h_full()
                .flex_1()
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx)),
        );
    }
    overlay
}

pub(in crate::ui::app) fn dashboard_graph_sample_tooltips(
    points: &[DashboardDualLinePoint],
    first_series_label: &str,
    second_series_label: &str,
) -> Vec<SharedString> {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            tooltip_lines([
                dashboard_sample_age_label(index),
                format!("{first_series_label}: {}", point.first_label),
                format!("{second_series_label}: {}", point.second_label),
            ])
        })
        .collect()
}

pub(in crate::ui::app) fn dashboard_sample_age_label(index: usize) -> String {
    let age = CPU_USAGE_HISTORY_LEN.saturating_sub(index + 1);
    if age == 0 {
        t!("common.latest_sample").to_string()
    } else {
        t!("common.seconds_ago", count = age).to_string()
    }
}

pub(in crate::ui::app) fn dashboard_cpu_dual_line_points(
    values: &VecDeque<CpuUsageHistorySample>,
    base_frequency_mhz: Option<u32>,
) -> Vec<DashboardDualLinePoint> {
    let sample_count = values.len().min(CPU_USAGE_HISTORY_LEN);
    let missing_samples = CPU_USAGE_HISTORY_LEN - sample_count;
    let start_index = values.len().saturating_sub(sample_count);
    let base_frequency_mhz = base_frequency_mhz
        .or_else(|| {
            values
                .iter()
                .skip(start_index)
                .filter_map(|sample| sample.frequency_mhz)
                .min()
        })
        .unwrap_or(0);
    let peak_frequency_mhz = values
        .iter()
        .skip(start_index)
        .filter_map(|sample| sample.frequency_mhz)
        .max()
        .filter(|peak| *peak > base_frequency_mhz);

    let mut points = Vec::with_capacity(CPU_USAGE_HISTORY_LEN);
    for index in 0..CPU_USAGE_HISTORY_LEN {
        let sample = if index < missing_samples {
            None
        } else {
            Some(values[start_index + index - missing_samples])
        };

        points.push(DashboardDualLinePoint {
            tick: dashboard_history_tick(index),
            first_value: f64::from(sample.map_or(0.0, |sample| sample.percent.max(0.0))),
            second_value: f64::from(normalize_cpu_frequency_percent(
                sample.and_then(|sample| sample.frequency_mhz),
                base_frequency_mhz,
                peak_frequency_mhz,
            )),
            first_label: cpu_usage_label(sample.map(|sample| sample.percent)),
            second_label: cpu_frequency_label(sample.and_then(|sample| sample.frequency_mhz)),
        });
    }
    points
}

pub(in crate::ui::app) fn normalize_cpu_frequency_percent(
    frequency_mhz: Option<u32>,
    base_frequency_mhz: u32,
    peak_frequency_mhz: Option<u32>,
) -> f32 {
    let Some(frequency_mhz) = frequency_mhz else {
        return 0.0;
    };
    let Some(peak_frequency_mhz) = peak_frequency_mhz else {
        return 0.0;
    };
    let range = peak_frequency_mhz.saturating_sub(base_frequency_mhz);
    if range == 0 || frequency_mhz <= base_frequency_mhz {
        return 0.0;
    }

    ((frequency_mhz.saturating_sub(base_frequency_mhz) as f32 / range as f32) * 100.0)
        .clamp(0.0, 100.0)
}

pub(in crate::ui::app) fn dashboard_dual_line_points(
    values: impl ExactSizeIterator<Item = (f32, f32)>,
    first_label: impl Fn(Option<f32>) -> String,
    second_label: impl Fn(Option<f32>) -> String,
) -> Vec<DashboardDualLinePoint> {
    let value_count = values.len();
    let sample_count = value_count.min(CPU_USAGE_HISTORY_LEN);
    let missing_samples = CPU_USAGE_HISTORY_LEN - sample_count;
    let mut values = values.skip(value_count.saturating_sub(sample_count));

    let mut points = Vec::with_capacity(CPU_USAGE_HISTORY_LEN);
    for index in 0..CPU_USAGE_HISTORY_LEN {
        let sample = if index < missing_samples {
            None
        } else {
            values.next()
        };
        let (first_value, second_value) = sample.unwrap_or((0.0, 0.0));

        points.push(DashboardDualLinePoint {
            tick: dashboard_history_tick(index),
            first_value: f64::from(first_value.max(0.0)),
            second_value: f64::from(second_value.max(0.0)),
            first_label: first_label(sample.map(|sample| sample.0)),
            second_label: second_label(sample.map(|sample| sample.1)),
        });
    }
    points
}

pub(in crate::ui::app) fn dashboard_history_tick(index: usize) -> String {
    format!("sample-{index:02}")
}

pub(in crate::ui::app) fn dashboard_primary_series_color() -> Hsla {
    Hsla::from(rgb(accent_color())).lighten(0.16)
}

pub(in crate::ui::app) fn dashboard_secondary_series_color() -> Hsla {
    Hsla::from(rgb(accent_color())).darken(0.18)
}

pub(in crate::ui::app) fn titled_status_list(
    title: &str,
    header_trailing: Option<AnyElement>,
    items: Vec<(String, String)>,
    empty_message: Option<String>,
) -> gpui::Div {
    let mut list = v_flex()
        .w_full()
        .min_w(px(0.0))
        .flex_1()
        .min_h(px(0.0))
        .gap_1()
        .overflow_y_scrollbar();

    if items.is_empty() {
        if let Some(message) = empty_message {
            list = list.child(text_muted(message).py_1());
        }
    }

    for (label, detail) in items {
        let mut content = h_flex()
            .flex_1()
            .min_w(px(0.0))
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(px(TEXT_BODY_SIZE))
                    .line_height(px(TEXT_BODY_LINE_HEIGHT))
                    .child(label),
            );

        if !detail.is_empty() {
            content = content.child(text_muted(detail).truncate().flex_shrink_0());
        }

        list = list.child(
            h_flex()
                .w_full()
                .min_w(px(0.0))
                .items_center()
                .gap_2()
                .py_1()
                .child(
                    div()
                        .size(px(6.0))
                        .rounded_full()
                        .flex_shrink_0()
                        .bg(rgb(accent_color())),
                )
                .child(content),
        );
    }

    dashboard_summary_card(title.to_owned(), header_trailing, list.into_any_element())
}

pub(in crate::ui::app) const PROCESS_LIST_NAME_MIN_WIDTH: f32 = 180.0;
pub(in crate::ui::app) const PROCESS_LIST_NAME_MAX_WIDTH: f32 = 340.0;
pub(in crate::ui::app) const PROCESS_LIST_PID_MIN_WIDTH: f32 = 56.0;
pub(in crate::ui::app) const PROCESS_LIST_PID_MAX_WIDTH: f32 = 90.0;
pub(in crate::ui::app) const PROCESS_LIST_COLUMN_MIN_WIDTH: f32 = 72.0;
pub(in crate::ui::app) const PROCESS_LIST_COLUMN_MAX_WIDTH: f32 = 250.0;
pub(in crate::ui::app) const PROCESS_LIST_TEXT_CELL_HORIZONTAL_PADDING: f32 = 24.0;
pub(in crate::ui::app) const PROCESS_LIST_NAME_CELL_NON_TEXT_WIDTH: f32 = 76.0;
pub(in crate::ui::app) const PROCESS_LIST_SORT_ICON_WIDTH: f32 = 18.0;
pub(in crate::ui::app) const PROCESS_LIST_SORT_HEADER_GAP: f32 = 4.0;
pub(in crate::ui::app) const PROCESS_LIST_SPLIT_LABEL_WIDTH: f32 = 18.0;
pub(in crate::ui::app) const PROCESS_LIST_SPLIT_LABEL_GAP: f32 = 4.0;
pub(in crate::ui::app) const PROCESS_LIST_CELL_EDITOR_WIDTH: f32 = 220.0;
pub(in crate::ui::app) const PROCESS_LIST_ROW_HORIZONTAL_PADDING: f32 = 32.0;
pub(in crate::ui::app) const PROCESS_LIST_COLUMN_GAP: f32 = 12.0;
pub(in crate::ui::app) const PROCESS_LIST_HEADER_HEIGHT: f32 = 32.0;
pub(in crate::ui::app) const PROCESS_LIST_ROW_HEIGHT: f32 = 52.0;
pub(in crate::ui::app) const PROCESS_LIST_TOOLBAR_HEIGHT: f32 = 40.0;
pub(in crate::ui::app) const PROCESS_LIST_VERTICAL_GAP_TOTAL: f32 = 8.0;
pub(in crate::ui::app) const PROCESS_LIST_SCROLLBAR_GUTTER: f32 = 16.0;
pub(in crate::ui::app) const PROCESS_LIST_TREE_TOGGLE_WIDTH: f32 = 16.0;
pub(in crate::ui::app) const PROCESS_LIST_COLUMN_VISIBILITY_DROPDOWN_ID: &str =
    "process-list-column-visibility";
pub(in crate::ui::app) const PROCESS_LIST_COLUMN_VISIBILITY_DROPDOWN_WIDTH: f32 = 360.0;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::ui::app) enum ProcessListColumn {
    Pid,
    PowerPlanForeground,
    PowerPlanRunning,
    BackgroundEfficiency,
    CoreLimiter,
    BackgroundCpuRestriction,
    CoreSteering,
    ProcessPriority,
    IoPriority,
    GpuPriority,
    MemoryPriority,
    MemoryTrim,
    AppSuspension,
    TimerResolution,
}

pub(in crate::ui::app) const PROCESS_LIST_OPTIONAL_COLUMNS: [ProcessListColumn; 14] = [
    ProcessListColumn::Pid,
    ProcessListColumn::PowerPlanForeground,
    ProcessListColumn::PowerPlanRunning,
    ProcessListColumn::BackgroundEfficiency,
    ProcessListColumn::CoreLimiter,
    ProcessListColumn::BackgroundCpuRestriction,
    ProcessListColumn::CoreSteering,
    ProcessListColumn::ProcessPriority,
    ProcessListColumn::IoPriority,
    ProcessListColumn::GpuPriority,
    ProcessListColumn::MemoryPriority,
    ProcessListColumn::MemoryTrim,
    ProcessListColumn::AppSuspension,
    ProcessListColumn::TimerResolution,
];
