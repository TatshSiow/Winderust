use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) enum PowerPlanKind {
    Idle,
    Active,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum IoPriorityDefaultTarget {
    Background,
    Foreground,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ProcessPriorityDefaultTarget {
    Background,
    Foreground,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ThreadPriorityDefaultTarget {
    Background,
    Foreground,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum DynamicPriorityBoostDefaultTarget {
    Background,
    Foreground,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum GpuPriorityDefaultTarget {
    Background,
    Foreground,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum MemoryPriorityDefaultTarget {
    Background,
    Foreground,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum PowerPlanField {
    ActivityKind(PowerPlanKind),
    ByForegroundRule(usize),
    ByRunningAppRule(usize),
    ByTimeRule(usize),
    CpuRule(usize),
    CpuRuleElse(usize),
    ProcessorPowerTarget,
}

pub(super) fn power_plan_option_row(
    id: String,
    label: String,
    selected: bool,
    guid: Option<String>,
    field: PowerPlanField,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    dropdown_option_row(SharedString::from(id), label, selected, cx)
        .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
            app.set_power_plan_field(field, guid.clone());
            app.active_power_plan_picker = None;
            cx.notify();
        }))
        .into_any_element()
}

pub(super) fn dropdown_list_height(row_count: usize) -> Pixels {
    let row_count = row_count.max(1);
    px(DROPDOWN_SURFACE_VERTICAL_PADDING
        + (row_count as f32 * DROPDOWN_OPTION_ROW_HEIGHT)
        + (row_count.saturating_sub(1) as f32 * DROPDOWN_OPTION_GAP))
}

pub(super) fn dropdown_popup_phase(
    id: &str,
    is_open: bool,
    cx: &mut Context<WinderustApp>,
) -> DropdownPopupPhase {
    let mut schedule_cleanup = false;
    let phase = {
        let Ok(mut state) = DROPDOWN_MOTION_STATE.lock() else {
            return if is_open {
                DropdownPopupPhase::Open(0)
            } else {
                DropdownPopupPhase::Hidden
            };
        };

        if is_open {
            let generation = match state.open.get(id).copied() {
                Some(generation) => generation,
                None => {
                    state.generation = state.generation.wrapping_add(1);
                    let generation = state.generation;
                    state.open.insert(id.to_owned(), generation);
                    generation
                }
            };
            state.closing.remove(id);
            DropdownPopupPhase::Open(generation)
        } else if let Some(_generation) = state.open.remove(id) {
            if ui_animations_enabled() {
                state.generation = state.generation.wrapping_add(1);
                let generation = state.generation;
                state.closing.insert(
                    id.to_owned(),
                    DropdownCloseTransition {
                        started: Instant::now(),
                        generation,
                    },
                );
                schedule_cleanup = true;
                DropdownPopupPhase::Closing(generation)
            } else {
                state.closing.remove(id);
                DropdownPopupPhase::Hidden
            }
        } else if let Some(transition) = state.closing.get(id).copied() {
            if transition.started.elapsed() < Duration::from_secs_f64(MOTION_FAST_SECONDS) {
                DropdownPopupPhase::Closing(transition.generation)
            } else {
                state.closing.remove(id);
                DropdownPopupPhase::Hidden
            }
        } else {
            DropdownPopupPhase::Hidden
        }
    };

    if schedule_cleanup {
        let id = id.to_owned();
        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_secs_f64(MOTION_FAST_SECONDS)).await;
            if let Ok(mut state) = DROPDOWN_MOTION_STATE.lock() {
                let expired = state.closing.get(&id).is_some_and(|transition| {
                    transition.started.elapsed() >= Duration::from_secs_f64(MOTION_FAST_SECONDS)
                });
                if expired {
                    state.closing.remove(&id);
                }
            }
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    phase
}

pub(super) fn dropdown_anchor_sensor(
    id: impl Into<String>,
    anchor_bounds: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
) -> AnyElement {
    let id = id.into();
    canvas(
        move |bounds, _, _| {
            anchor_bounds.borrow_mut().insert(id, bounds);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
    .into_any_element()
}

#[derive(Clone, Copy)]
pub(super) enum DropdownSelectWidth {
    Compact,
    Table,
    Standard,
    Wide,
}

pub(super) fn dropdown_select_container(width: DropdownSelectWidth) -> gpui::Div {
    let width = match width {
        DropdownSelectWidth::Compact => DROPDOWN_SELECT_COMPACT_WIDTH,
        DropdownSelectWidth::Table => DROPDOWN_SELECT_TABLE_WIDTH,
        DropdownSelectWidth::Standard => DROPDOWN_SELECT_STANDARD_WIDTH,
        DropdownSelectWidth::Wide => DROPDOWN_SELECT_WIDE_WIDTH,
    };

    v_flex()
        .w(px(width))
        .min_w(px(width))
        .max_w(px(width))
        .flex_shrink_0()
        .relative()
        .min_h(px(DROPDOWN_CONTROL_HEIGHT))
}

pub(super) fn dropdown_popup_layer(placement: DropdownPlacement, interactive: bool) -> gpui::Div {
    let layer = div()
        .absolute()
        .left(px(0.0))
        .right(px(0.0))
        .when(interactive, |layer| layer.occlude());

    if placement.open_up {
        layer.bottom(px(DROPDOWN_MENU_OFFSET))
    } else {
        layer.top(px(DROPDOWN_MENU_OFFSET))
    }
}

pub(super) fn dropdown_popup_or_empty(
    id: SharedString,
    phase: DropdownPopupPhase,
    placement: DropdownPlacement,
    options: Scrollable<gpui::Div>,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    dropdown_popup_for_phase(phase, id, placement, options, cx)
}

pub(super) fn dropdown_popup_or_empty_lazy(
    is_open: bool,
    id: SharedString,
    placement: impl FnOnce() -> DropdownPlacement,
    options: impl FnOnce(Pixels, &mut Context<WinderustApp>) -> Scrollable<gpui::Div>,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let phase = dropdown_popup_phase(id.as_ref(), is_open, cx);
    if matches!(phase, DropdownPopupPhase::Hidden) {
        return Empty.into_any_element();
    }

    let placement = placement();
    let options = options(placement.max_height, cx);
    dropdown_popup_for_phase(phase, id, placement, options, cx)
}

pub(super) fn dropdown_popup_for_phase(
    phase: DropdownPopupPhase,
    id: SharedString,
    placement: DropdownPlacement,
    options: Scrollable<gpui::Div>,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    match phase {
        DropdownPopupPhase::Hidden => Empty.into_any_element(),
        DropdownPopupPhase::Open(generation) => {
            let popup = dropdown_popup_layer(placement, true)
                .on_mouse_down_out(cx.listener(|app, event: &gpui::MouseDownEvent, _, cx| {
                    let click_is_on_trigger = app
                        .active_power_plan_picker
                        .as_deref()
                        .and_then(|id| app.dropdown_anchor_bounds.borrow().get(id).copied())
                        .is_some_and(|bounds| bounds.contains(&event.position));

                    if click_is_on_trigger {
                        return;
                    }

                    app.active_power_plan_picker = None;
                    cx.notify();
                }))
                .child(options);

            let popup = with_optional_motion(
                popup,
                SharedString::from(format!("dropdown-popup-open-{id}-{generation}")),
                MotionSpeed::Fast,
                |popup| popup,
                move |popup, delta| {
                    let offset = (1.0 - delta) * 6.0;
                    let popup = popup.opacity(0.18 + 0.82 * delta);
                    if placement.open_up {
                        popup.bottom(px(DROPDOWN_MENU_OFFSET + offset))
                    } else {
                        popup.top(px(DROPDOWN_MENU_OFFSET + offset))
                    }
                },
            );

            deferred(popup)
                .with_priority(PROCESS_PICKER_LAYER_PRIORITY)
                .into_any_element()
        }
        DropdownPopupPhase::Closing(generation) => {
            let popup = dropdown_popup_layer(placement, false).child(options);
            let popup = with_optional_motion(
                popup,
                SharedString::from(format!("dropdown-popup-close-{id}-{generation}")),
                MotionSpeed::Fast,
                |popup| popup.opacity(0.0),
                move |popup, delta| {
                    let offset = delta * 6.0;
                    let popup = popup.opacity(1.0 - delta);
                    if placement.open_up {
                        popup.bottom(px(DROPDOWN_MENU_OFFSET + offset))
                    } else {
                        popup.top(px(DROPDOWN_MENU_OFFSET + offset))
                    }
                },
            );

            deferred(popup)
                .with_priority(PROCESS_PICKER_LAYER_PRIORITY)
                .into_any_element()
        }
    }
}

pub(super) fn dropdown_select_control(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    enabled: bool,
    open: bool,
    phase: DropdownPopupPhase,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    let id: SharedString = id.into();
    let label: SharedString = label.into();
    let border_color: Hsla = if enabled && open {
        cx.theme().accent
    } else {
        rgb(dropdown_control_border_color()).into()
    };
    let hover_border_color: Hsla = if enabled && open {
        cx.theme().accent
    } else {
        rgb(dropdown_control_hover_border_color()).into()
    };

    h_flex()
        .id(id.clone())
        .h(px(DROPDOWN_CONTROL_HEIGHT))
        .w_full()
        .min_w(px(0.0))
        .items_center()
        .justify_between()
        .gap_2()
        .px_3()
        .rounded(px(BRAND_RADIUS_CONTROL))
        .border_1()
        .border_color(border_color)
        .bg(rgb(dropdown_control_color()))
        .text_size(px(TEXT_CONTROL_SIZE))
        .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
        .text_color(cx.theme().foreground)
        .hover(move |style| {
            if enabled {
                style
                    .bg(rgb(dropdown_control_hover_color()))
                    .border_color(hover_border_color)
            } else {
                style
            }
        })
        .when(enabled, |style| style.cursor_pointer())
        .when(!enabled, |style| style.cursor_default().opacity(0.48))
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .child(div().flex_1().min_w(px(0.0)).truncate().child(label))
        .child(dropdown_chevron(id, open, phase, cx))
}

pub(super) fn dropdown_surface(
    cx: &mut Context<WinderustApp>,
    max_height: Pixels,
) -> Scrollable<gpui::Div> {
    v_flex()
        .w_full()
        .max_h(max_height)
        .overflow_y_scrollbar()
        .gap_1()
        .p_2()
        .rounded(px(BRAND_RADIUS_CONTROL))
        .border_1()
        .border_color(cx.theme().border)
        .bg(rgb(dropdown_surface_color()))
}

pub(super) fn dropdown_option_row(
    id: SharedString,
    label: String,
    selected: bool,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    h_flex()
        .id(id)
        .relative()
        .min_h(px(40.0))
        .items_center()
        .pl_3()
        .pr_3()
        .rounded(px(BRAND_RADIUS_CONTROL))
        .text_size(px(TEXT_CONTROL_SIZE))
        .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
        .text_color(cx.theme().popover_foreground)
        .when(selected, |row| {
            row.bg(rgb(dropdown_selected_color())).child(
                div()
                    .absolute()
                    .left(px(0.0))
                    .top(px(11.0))
                    .bottom(px(11.0))
                    .w(px(3.0))
                    .rounded(px(BRAND_RADIUS_CONTROL))
                    .bg(cx.theme().accent),
            )
        })
        .hover(|style| style.bg(rgb(dropdown_option_hover_color())))
        .cursor_pointer()
        .child(label)
}

pub(super) fn dropdown_process_option_row(
    id: SharedString,
    process: &ProcessCandidate,
    selected: bool,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    h_flex()
        .id(id)
        .relative()
        .min_h(px(40.0))
        .items_center()
        .gap_2()
        .pl_3()
        .pr_3()
        .rounded(px(BRAND_RADIUS_CONTROL))
        .text_size(px(TEXT_CONTROL_SIZE))
        .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
        .text_color(cx.theme().popover_foreground)
        .when(selected, |row| {
            row.bg(rgb(dropdown_selected_color())).child(
                div()
                    .absolute()
                    .left(px(0.0))
                    .top(px(11.0))
                    .bottom(px(11.0))
                    .w(px(3.0))
                    .rounded(px(BRAND_RADIUS_CONTROL))
                    .bg(cx.theme().accent),
            )
        })
        .hover(|style| style.bg(rgb(dropdown_option_hover_color())))
        .cursor_pointer()
        .child(process_icon_cell(process.icon.as_ref(), cx))
        .child(div().min_w(px(0.0)).truncate().child(process.name.clone()))
}

pub(super) fn process_icon_cell(
    icon: Option<&Arc<Image>>,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    div()
        .size(px(20.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .child(match icon {
            Some(icon) => img(Arc::clone(icon)).size(px(18.0)).into_any_element(),
            None => div()
                .size(px(18.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(BRAND_RADIUS_CONTROL))
                .border_1()
                .border_color(cx.theme().border)
                .child(Icon::new(NavIcon::AppWindow).with_size(px(13.0)))
                .into_any_element(),
        })
        .into_any_element()
}

pub(super) fn dropdown_empty_row(message: String, cx: &mut Context<WinderustApp>) -> gpui::Div {
    div()
        .min_h(px(40.0))
        .px_3()
        .flex()
        .relative()
        .items_center()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .text_color(cx.theme().muted_foreground)
        .child(message)
}

pub(super) fn dropdown_chevron(
    id: SharedString,
    open: bool,
    phase: DropdownPopupPhase,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let start_turns = if open { 0.0 } else { 180.0 / 360.0 };
    let end_turns = if open { 180.0 / 360.0 } else { 0.0 };
    let icon = Icon::new(NavIcon::ChevronDown)
        .with_size(px(16.0))
        .text_color(cx.theme().muted_foreground);
    let icon = match phase {
        DropdownPopupPhase::Open(generation) => with_optional_motion(
            icon,
            SharedString::from(format!("dropdown-chevron-open-{id}-{generation}")),
            MotionSpeed::Fast,
            move |icon| icon.rotate(percentage(180.0 / 360.0)),
            move |icon, delta| icon.rotate(percentage(delta * 180.0 / 360.0)),
        ),
        DropdownPopupPhase::Closing(generation) => with_optional_motion(
            icon,
            SharedString::from(format!("dropdown-chevron-close-{id}-{generation}")),
            MotionSpeed::Fast,
            move |icon| icon.rotate(percentage(0.0)),
            move |icon, delta| icon.rotate(percentage((1.0 - delta) * 180.0 / 360.0)),
        ),
        DropdownPopupPhase::Hidden => with_state_change_motion(
            icon,
            SharedString::from(format!("dropdown-chevron-{id}")),
            SharedString::from(open.to_string()),
            MotionSpeed::Fast,
            move |icon| icon.rotate(percentage(end_turns)),
            move |icon, delta| {
                let turns = start_turns + (end_turns - start_turns) * delta;
                icon.rotate(percentage(turns))
            },
        ),
    };

    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(px(18.0))
        .child(icon)
        .into_any_element()
}

pub(super) fn dropdown_control_color() -> u32 {
    settings_card_color()
}

pub(super) fn dropdown_control_border_color() -> u32 {
    if ui_is_dark() {
        COLOR_BORDER
    } else {
        0xdedede
    }
}

pub(super) fn dropdown_control_hover_color() -> u32 {
    settings_card_hover_color()
}

pub(super) fn dropdown_control_hover_border_color() -> u32 {
    if ui_is_dark() {
        0x6a6a6a
    } else {
        0x9a9a9a
    }
}

pub(super) fn dropdown_surface_color() -> u32 {
    settings_card_color()
}

pub(super) fn dropdown_selected_color() -> u32 {
    panel_active_color()
}

pub(super) fn dropdown_option_hover_color() -> u32 {
    settings_card_hover_color()
}
