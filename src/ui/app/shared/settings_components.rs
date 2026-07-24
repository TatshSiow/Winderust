use crate::ui::app::*;

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
