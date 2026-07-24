use crate::ui::app::*;

pub(in crate::ui::app) fn feature_toggle_switch(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    enabled: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    feature_toggle_switch_inner(id, label, None, enabled, handler)
}

pub(in crate::ui::app) fn feature_toggle_switch_with_help(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    help: impl Into<SharedString>,
    enabled: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    feature_toggle_switch_inner(id, label, Some(help.into()), enabled, handler)
}

pub(in crate::ui::app) fn feature_toggle_switch_inner(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    help: Option<SharedString>,
    enabled: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let label = label.into();
    let label_id = id.clone();
    let mut label_row = h_flex()
        .flex_1()
        .min_w(px(0.0))
        .items_center()
        .gap_1()
        .child(div().min_w(px(0.0)).truncate().child(label));
    if let Some(help) = help {
        label_row = label_row.child(title_info_button(format!("{label_id}-info"), help));
    }

    h_flex()
        .id(id.clone())
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
        .child(label_row)
        .child(switch_toggle_action(
            format!("{id}-switch"),
            enabled,
            handler,
        ))
        .into_any_element()
}

pub(in crate::ui::app) fn switch_toggle_action(
    id: impl Into<SharedString>,
    enabled: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id = id.into();
    let motion_id = format!("switch-{id}");
    h_flex()
        .id(id.clone())
        .items_center()
        .child(switch_indicator(id, enabled))
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            let next = !enabled;
            begin_control_motion(motion_id.clone(), next, cx);
            handler(&next, window, cx);
        })
        .into_any_element()
}

pub(in crate::ui::app) fn switch_indicator(id: SharedString, enabled: bool) -> gpui::Div {
    let accent = switch_accent_color();
    let switch_on_bg = accent;
    let switch_off_bg = settings_card_color();
    let switch_on_border = accent;
    let switch_off_border = border_color();
    let knob_on_bg = accent_glyph_color(accent);
    let knob_off_bg = if ui_is_dark() { 0xd0d0d0 } else { 0x5f5f5f };
    let state_label = if enabled { "On" } else { "Off" };
    let progress = control_motion_progress(&format!("switch-{id}"), enabled);
    let switch_bg = lerp_rgb(switch_off_bg, switch_on_bg, progress);
    let switch_border = lerp_rgb(switch_off_border, switch_on_border, progress);
    let knob_bg = lerp_rgb(knob_off_bg, knob_on_bg, progress);
    let knob_left = 4.0 + (24.0 - 4.0) * progress;
    let knob = div()
        .absolute()
        .top(px(3.0))
        .left(px(knob_left))
        .size(px(12.0))
        .rounded_full()
        .bg(rgb(knob_bg))
        .into_any_element();
    let track = h_flex()
        .w(px(40.0))
        .h(px(20.0))
        .items_center()
        .flex_shrink_0()
        .rounded_full()
        .border_1()
        .border_color(rgb(switch_border))
        .bg(rgb(switch_bg))
        .relative()
        .child(knob);

    h_flex()
        .items_center()
        .justify_end()
        .gap_2()
        .flex_shrink_0()
        .child(
            div()
                .text_size(px(TEXT_BODY_SIZE))
                .line_height(px(TEXT_BODY_LINE_HEIGHT))
                .text_color(rgb(primary_text_color()))
                .child(state_label),
        )
        .child(track)
}

pub(in crate::ui::app) fn value_pill(value: impl Into<SharedString>) -> gpui::Div {
    div()
        .min_w(px(56.0))
        .h(px(32.0))
        .px_3()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(BRAND_RADIUS_CONTROL))
        .border_1()
        .border_color(rgb(app_input_border_color(false)))
        .bg(rgb(app_input_color(false)))
        .text_size(px(TEXT_CONTROL_SIZE))
        .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
        .text_color(rgb(primary_text_color()))
        .child(value.into())
}

pub(in crate::ui::app) fn numeric_value_width(field: NumericField) -> f32 {
    match field {
        NumericField::ProcessorAcCoreParkingMin
        | NumericField::ProcessorAcPerformanceMin
        | NumericField::ProcessorAcPerformanceMax
        | NumericField::ProcessorAcBoostPolicy
        | NumericField::ProcessorDcCoreParkingMin
        | NumericField::ProcessorDcPerformanceMin
        | NumericField::ProcessorDcPerformanceMax
        | NumericField::ProcessorDcBoostPolicy
        | NumericField::AdaptiveEngineProcessorPolicy(_)
        | NumericField::BackgroundCpuRestrictionPercent
        | NumericField::MemoryTrimMemoryLoadThreshold
        | NumericField::CoreLimiterThreshold(_)
        | NumericField::CoreLimiterMaxProcessors(_) => 76.0,
        NumericField::TimerResolutionRule(_) => 104.0,
        NumericField::MemoryTrimWorkingSetThreshold | NumericField::MemoryTrimIdleSeconds => 112.0,
        NumericField::NetworkThreshold(_) => 76.0,
        _ => 96.0,
    }
}

pub(in crate::ui::app) fn max_logical_processor_count() -> u8 {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .clamp(1, u8::MAX as usize) as u8
}

pub(in crate::ui::app) fn text_muted(value: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .opacity(0.72)
        .child(value.into())
}

pub(in crate::ui::app) fn text_warning(value: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .text_color(rgb(warning_text_color()))
        .child(value.into())
}

pub(in crate::ui::app) fn processor_power_column_header(
    value: impl Into<SharedString>,
) -> gpui::Div {
    div()
        .w_full()
        .min_w(px(0.0))
        .pb_1()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::BOLD)
        .child(value.into())
}

pub(in crate::ui::app) fn processor_power_slider(
    id: impl Into<SharedString>,
    label: &str,
    value_element: AnyElement,
    state: &Entity<SliderState>,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
    handler: impl Fn(&StepChange<u64>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    percent_slider_row(
        SliderRowSpec {
            id: id.into(),
            label: SharedString::from(label.to_owned()),
            value_element,
            state,
            enabled: true,
            delta: 1_u64,
        },
        window,
        cx,
        handler,
    )
}

pub(in crate::ui::app) fn processor_power_setting_row(
    id: &'static str,
    label: impl Into<SharedString>,
    value_element: AnyElement,
) -> AnyElement {
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
        .child(
            div()
                .w(px(180.0))
                .min_w(px(120.0))
                .flex_shrink_0()
                .truncate()
                .child(label.into()),
        )
        .child(
            h_flex()
                .flex_1()
                .min_w(px(0.0))
                .justify_end()
                .child(value_element),
        )
        .into_any_element()
}

pub(in crate::ui::app) fn win32_priority_registry_value_row(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    help: Option<String>,
    value: impl Into<SharedString>,
    divided: bool,
) -> AnyElement {
    let id: SharedString = id.into();
    let mut label_row = h_flex()
        .flex_1()
        .min_w(px(0.0))
        .items_center()
        .gap_1()
        .child(div().min_w(px(0.0)).truncate().child(label.into()));
    if let Some(help) = help {
        label_row = label_row.child(title_info_button(format!("{id}-info"), help));
    }

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
        .when(divided, |row| {
            row.border_t_1().border_color(rgb(border_color()))
        })
        .child(label_row)
        .child(value_pill(value))
        .into_any_element()
}

pub(in crate::ui::app) fn win32_priority_row(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    help: Option<String>,
    value_element: AnyElement,
) -> AnyElement {
    let id: SharedString = id.into();
    let mut label_row = h_flex()
        .flex_1()
        .min_w(px(0.0))
        .items_center()
        .gap_1()
        .child(div().min_w(px(0.0)).truncate().child(label.into()));
    if let Some(help) = help {
        label_row = label_row.child(title_info_button(format!("{id}-info"), help));
    }

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
        .child(label_row)
        .child(
            h_flex()
                .w(px(260.0))
                .max_w(px(260.0))
                .items_center()
                .justify_end()
                .flex_shrink_0()
                .child(value_element),
        )
        .into_any_element()
}

pub(in crate::ui::app) fn threshold_level_slider(
    spec: SliderRowSpec<'_, u8>,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
    handler: impl Fn(&StepChange<u8>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    rule_percent_slider_row(spec, window, cx, handler)
}

pub(in crate::ui::app) fn stable_slider(
    state: &Entity<SliderState>,
    spec: StableSliderSpec,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let StableSliderSpec {
        range,
        enabled,
        track_color,
        thumb_color,
    } = spec;
    let value = state.read(cx).value().end();
    let min = range.min.min(range.max);
    let max = range.max.max(min + u64::from(range.max == min));
    let step = range.step.max(1);
    let range = SliderRange { min, max, step };
    let percentage = stable_slider_percentage(value, min, max);
    let track = Hsla::from(rgb(track_color));
    let bounds = Rc::new(RefCell::new(Bounds::<Pixels>::default()));
    let click_bounds = Rc::clone(&bounds);
    let drag_bounds = Rc::clone(&bounds);
    let canvas_bounds = Rc::clone(&bounds);
    let click_state = state.clone();
    let entity_id = state.entity_id();

    div()
        .id(("stable-slider", entity_id))
        .relative()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .w_full()
        .h(px(24.0))
        .when(enabled, |slider| {
            slider
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    cx.stop_propagation();
                    let bounds = *click_bounds.borrow();
                    click_state.update(cx, |state, cx| {
                        update_stable_slider_from_position(
                            state,
                            bounds,
                            event.position,
                            range,
                            window,
                            cx,
                        );
                    });
                })
                .on_drag(DragStableSlider(entity_id), |drag, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| drag.clone())
                })
                .on_drag_move(window.listener_for(
                    state,
                    move |state, event: &DragMoveEvent<DragStableSlider>, window, cx| {
                        match event.drag(cx) {
                            DragStableSlider(id) if *id == entity_id => {
                                update_stable_slider_from_position(
                                    state,
                                    *drag_bounds.borrow(),
                                    event.event.position,
                                    range,
                                    window,
                                    cx,
                                );
                            }
                            _ => {}
                        }
                    },
                ))
        })
        .child(
            div()
                .relative()
                .w_full()
                .h_1p5()
                .bg(track.opacity(0.2))
                .rounded_full()
                .child(
                    div()
                        .absolute()
                        .left(px(0.0))
                        .top(px(0.0))
                        .bottom(px(0.0))
                        .w(relative(percentage))
                        .bg(rgb(track_color))
                        .rounded_full(),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(-5.0))
                        .left(relative(percentage))
                        .ml(-px(8.0))
                        .size_4()
                        .p(px(1.0))
                        .rounded_full()
                        .bg(track.opacity(0.5))
                        .child(div().size_full().rounded_full().bg(rgb(thumb_color))),
                )
                .child(
                    canvas(
                        move |bounds, _, _| {
                            *canvas_bounds.borrow_mut() = bounds;
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                ),
        )
        .into_any_element()
}

pub(in crate::ui::app) fn stable_slider_percentage(value: f32, min: u64, max: u64) -> f32 {
    let min = min as f32;
    let max = max as f32;
    let range = max - min;
    if range <= 0.0 {
        0.0
    } else {
        ((value.clamp(min, max) - min) / range).clamp(0.0, 1.0)
    }
}

pub(in crate::ui::app) fn update_stable_slider_from_position(
    state: &mut SliderState,
    bounds: Bounds<Pixels>,
    position: Point<Pixels>,
    range: SliderRange,
    window: &mut Window,
    cx: &mut Context<SliderState>,
) {
    let SliderRange { min, max, step } = range;
    let total_size = bounds.size.width;
    if total_size <= px(0.0) {
        return;
    }

    let percentage = (position.x - bounds.left()).clamp(px(0.0), total_size) / total_size;
    let min = min as f32;
    let max = max as f32;
    let step = step.max(1) as f32;
    let value = min + ((max - min) * percentage);
    let value = (((value - min) / step).round() * step + min).clamp(min, max);

    state.set_value(value, window, cx);
    cx.emit(SliderEvent::Change(SliderValue::Single(value)));
}

pub(in crate::ui::app) fn activity_slider_card(
    spec: ActivitySliderCardSpec<'_>,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
    handler: impl Fn(&StepChange<u64>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let ActivitySliderCardSpec {
        id,
        label,
        value_element,
        state,
        enabled,
        range,
    } = spec;
    let handler: StepChangeHandler<u64> = Rc::new(handler);
    let down = Rc::clone(&handler);
    let delta = range.step;
    let slider_track_color = if enabled {
        accent_color()
    } else {
        disabled_slider_track_color()
    };
    let slider_thumb_color = if enabled {
        windows_slider_thumb_color()
    } else {
        disabled_slider_thumb_color()
    };

    setting_action_card(
        id.clone(),
        label.to_owned(),
        h_flex()
            .items_center()
            .justify_end()
            .gap_2()
            .min_w(px(0.0))
            .flex_shrink_0()
            .child(
                control_button(Button::new((gpui::ElementId::from(id.clone()), "down")))
                    .label("-")
                    .disabled(!enabled)
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
            .child(
                div()
                    .w(px(260.0))
                    .px(px(8.0))
                    .flex_none()
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(stable_slider(
                        state,
                        StableSliderSpec {
                            range,
                            enabled,
                            track_color: slider_track_color,
                            thumb_color: slider_thumb_color,
                        },
                        window,
                        cx,
                    )),
            )
            .child(
                control_button(Button::new((gpui::ElementId::from(id), "up")))
                    .label("+")
                    .disabled(!enabled)
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
            .child(value_element)
            .into_any_element(),
    )
    .into_any_element()
}

pub(in crate::ui::app) fn rule_percent_slider_row<T>(
    spec: SliderRowSpec<'_, T>,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
    handler: impl Fn(&StepChange<T>, &mut Window, &mut App) + 'static,
) -> AnyElement
where
    T: Copy + 'static,
{
    let SliderRowSpec {
        id,
        label,
        value_element,
        state,
        enabled,
        delta,
    } = spec;
    let handler: StepChangeHandler<T> = Rc::new(handler);
    let down = Rc::clone(&handler);
    let down_delta = delta;
    let up_delta = delta;
    let label_color = if enabled {
        primary_text_color()
    } else {
        dim_text_color()
    };
    let slider_track_color = if enabled {
        accent_color()
    } else {
        disabled_slider_track_color()
    };
    let slider_thumb_color = if enabled {
        windows_slider_thumb_color()
    } else {
        disabled_slider_thumb_color()
    };

    rule_action_row_with_title_color(
        id.clone(),
        label,
        h_flex()
            .items_center()
            .justify_end()
            .gap_2()
            .min_w(px(0.0))
            .flex_shrink_0()
            .child(
                control_button(Button::new((gpui::ElementId::from(id.clone()), "down")))
                    .label("-")
                    .disabled(!enabled)
                    .on_click(move |_, window, cx| {
                        down(
                            &StepChange {
                                delta: down_delta,
                                increase: false,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .child(
                div()
                    .w(px(220.0))
                    .px(px(8.0))
                    .flex_none()
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(stable_slider(
                        state,
                        StableSliderSpec {
                            range: SliderRange {
                                min: 0,
                                max: 100,
                                step: 1,
                            },
                            enabled,
                            track_color: slider_track_color,
                            thumb_color: slider_thumb_color,
                        },
                        window,
                        cx,
                    )),
            )
            .child(
                control_button(Button::new((gpui::ElementId::from(id), "up")))
                    .label("+")
                    .disabled(!enabled)
                    .on_click(move |_, window, cx| {
                        handler(
                            &StepChange {
                                delta: up_delta,
                                increase: true,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .child(value_element)
            .into_any_element(),
        label_color,
    )
    .into_any_element()
}

pub(in crate::ui::app) fn percent_slider_row<T>(
    spec: SliderRowSpec<'_, T>,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
    handler: impl Fn(&StepChange<T>, &mut Window, &mut App) + 'static,
) -> AnyElement
where
    T: Copy + 'static,
{
    let SliderRowSpec {
        id,
        label,
        value_element,
        state,
        enabled,
        delta,
    } = spec;
    let handler: StepChangeHandler<T> = Rc::new(handler);
    let down = Rc::clone(&handler);
    let down_delta = delta;
    let up_delta = delta;
    let label_color = if enabled {
        primary_text_color()
    } else {
        dim_text_color()
    };
    let slider_track_color = if enabled {
        accent_color()
    } else {
        disabled_slider_track_color()
    };
    let slider_thumb_color = if enabled {
        windows_slider_thumb_color()
    } else {
        disabled_slider_thumb_color()
    };

    h_flex()
        .id(id.clone())
        .w_full()
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
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_color(rgb(label_color))
                .child(label),
        )
        .child(
            h_flex()
                .items_center()
                .justify_end()
                .gap_2()
                .min_w(px(0.0))
                .flex_shrink_0()
                .child(
                    control_button(Button::new((gpui::ElementId::from(id.clone()), "down")))
                        .label("-")
                        .disabled(!enabled)
                        .on_click(move |_, window, cx| {
                            down(
                                &StepChange {
                                    delta: down_delta,
                                    increase: false,
                                },
                                window,
                                cx,
                            )
                        }),
                )
                .child(
                    div()
                        .w(px(220.0))
                        .px(px(8.0))
                        .flex_none()
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .child(stable_slider(
                            state,
                            StableSliderSpec {
                                range: SliderRange {
                                    min: 0,
                                    max: 100,
                                    step: 1,
                                },
                                enabled,
                                track_color: slider_track_color,
                                thumb_color: slider_thumb_color,
                            },
                            window,
                            cx,
                        )),
                )
                .child(
                    control_button(Button::new((gpui::ElementId::from(id), "up")))
                        .label("+")
                        .disabled(!enabled)
                        .on_click(move |_, window, cx| {
                            handler(
                                &StepChange {
                                    delta: up_delta,
                                    increase: true,
                                },
                                window,
                                cx,
                            )
                        }),
                )
                .child(value_element),
        )
        .into_any_element()
}

pub(in crate::ui::app) fn u64_step(value: u64) -> u64 {
    if value >= 1_000 {
        100
    } else if value >= 100 {
        10
    } else {
        1
    }
}

pub(in crate::ui::app) fn apply_u64_step(
    current: u64,
    change: &StepChange<u64>,
    min: u64,
    max: u64,
) -> u64 {
    let next = if change.increase {
        current.saturating_add(change.delta)
    } else {
        current.saturating_sub(change.delta)
    };
    next.clamp(min, max)
}

pub(in crate::ui::app) fn apply_u8_step(
    current: u8,
    change: &StepChange<u8>,
    min: u8,
    max: u8,
) -> u8 {
    let next = if change.increase {
        current.saturating_add(change.delta)
    } else {
        current.saturating_sub(change.delta)
    };
    next.clamp(min, max)
}

pub(in crate::ui::app) fn activity_slider_normalized_value(
    slider: ActivitySlider,
    value: u64,
) -> u64 {
    match slider {
        ActivitySlider::IdleTimeout => value.clamp(
            ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS,
            ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS,
        ),
        ActivitySlider::CheckInterval => snap_to_step(value, ACTIVITY_CHECK_INTERVAL_STEP_MS)
            .clamp(CHECK_INTERVAL_MIN_MS, CHECK_INTERVAL_MAX_MS),
    }
}

pub(in crate::ui::app) fn snap_to_step(value: u64, step: u64) -> u64 {
    if step == 0 {
        return value;
    }
    ((value + (step / 2)) / step) * step
}

pub(in crate::ui::app) fn seconds_label(seconds: u64) -> String {
    duration_label_ms(
        seconds
            .clamp(
                ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS,
                ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS,
            )
            .saturating_mul(1_000),
    )
}

pub(in crate::ui::app) fn milliseconds_label(milliseconds: u64) -> String {
    duration_label_ms(
        snap_to_step(milliseconds, ACTIVITY_CHECK_INTERVAL_STEP_MS)
            .clamp(CHECK_INTERVAL_MIN_MS, CHECK_INTERVAL_MAX_MS),
    )
}

pub(in crate::ui::app) fn duration_label_ms(milliseconds: u64) -> String {
    if milliseconds < 1_000 {
        return format!("{milliseconds} ms");
    }

    let (value, unit) = if milliseconds < 60_000 {
        (milliseconds as f64 / 1_000.0, "sec")
    } else if milliseconds < 3_600_000 {
        (milliseconds as f64 / 60_000.0, "min")
    } else {
        (milliseconds as f64 / 3_600_000.0, "hr")
    };

    rounded_duration_value(value, unit)
}

pub(in crate::ui::app) fn rounded_duration_value(value: f64, unit: &str) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if (rounded - rounded.round()).abs() < f64::EPSILON {
        format!("{} {unit}", rounded.round() as u64)
    } else {
        format!("{rounded:.1} {unit}")
    }
}

pub(in crate::ui::app) fn parse_u64_input(value: &str, min: u64, max: u64) -> Option<u64> {
    value.parse::<u64>().ok().map(|value| value.clamp(min, max))
}

pub(in crate::ui::app) fn parse_timer_resolution_input_100ns(
    value: &str,
    minimum_100ns: u32,
    maximum_100ns: u32,
) -> Option<u32> {
    let value = value.trim();
    let value = value
        .strip_suffix("ms")
        .or_else(|| value.strip_suffix("MS"))
        .or_else(|| value.strip_suffix("Ms"))
        .or_else(|| value.strip_suffix("mS"))
        .unwrap_or(value)
        .trim();
    let milliseconds = value
        .parse::<f64>()
        .ok()?
        .clamp(TIMER_RESOLUTION_INPUT_MIN_MS, TIMER_RESOLUTION_INPUT_MAX_MS);
    let value_100ns = (milliseconds * 10_000.0)
        .round()
        .clamp(1.0, u32::MAX as f64) as u32;
    Some(timer_resolution::normalize_desired_resolution(
        value_100ns,
        minimum_100ns,
        maximum_100ns,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_resolution_input_accepts_decimal_milliseconds() {
        assert_eq!(
            parse_timer_resolution_input_100ns("1", 10_000, 160_000),
            Some(10_000)
        );
        assert_eq!(
            parse_timer_resolution_input_100ns("0.5 ms", 10_000, 160_000),
            Some(10_000)
        );
        assert_eq!(
            parse_timer_resolution_input_100ns("15.625 MS", 10_000, 160_000),
            Some(160_000)
        );
    }

    #[test]
    fn timer_resolution_input_clamps_to_supported_range() {
        assert_eq!(
            parse_timer_resolution_input_100ns("0.1", 10_000, 160_000),
            Some(10_000)
        );
        assert_eq!(
            parse_timer_resolution_input_100ns("1000", 10_000, 160_000),
            Some(160_000)
        );
    }
}
