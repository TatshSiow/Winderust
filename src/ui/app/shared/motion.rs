use crate::ui::app::*;

#[derive(Clone, Copy)]
pub(in crate::ui::app) enum PageTransitionMotion {
    EnterSub,
    ExitSub,
    SameLevel,
}

pub(in crate::ui::app) fn animated_page_content_frame(
    frame: gpui::Div,
    transition: Option<&BreadcrumbTransition>,
) -> AnyElement {
    let Some(transition) = transition else {
        return frame.into_any_element();
    };
    let Some(motion) = page_transition_motion(transition) else {
        return frame.into_any_element();
    };
    let (x, y) = match motion {
        PageTransitionMotion::EnterSub => (18.0, 0.0),
        PageTransitionMotion::ExitSub => (-18.0, 0.0),
        PageTransitionMotion::SameLevel => (0.0, 14.0),
    };

    with_optional_motion(
        frame,
        SharedString::from(format!("page-transition-{}", transition.generation)),
        MotionSpeed::Fast,
        |frame| frame,
        move |frame, delta| {
            frame
                .relative()
                .left(px(x * (1.0 - delta)))
                .top(px(y * (1.0 - delta)))
                .opacity(0.2 + 0.8 * delta)
        },
    )
}

pub(in crate::ui::app) fn page_transition_motion(
    transition: &BreadcrumbTransition,
) -> Option<PageTransitionMotion> {
    let previous = transition.previous.as_slice();
    let current = transition.current.as_slice();
    if previous == current {
        return None;
    }

    if current.len() > previous.len() {
        return Some(PageTransitionMotion::EnterSub);
    }
    if current.len() < previous.len() {
        return Some(PageTransitionMotion::ExitSub);
    }

    Some(PageTransitionMotion::SameLevel)
}

#[derive(Clone, Copy)]
pub(in crate::ui::app) enum MotionSpeed {
    Fast,
    Standard,
    Expand,
}

impl MotionSpeed {
    pub(in crate::ui::app) fn animation(self) -> Animation {
        match self {
            Self::Fast => Animation::new(Duration::from_secs_f64(MOTION_FAST_SECONDS))
                .with_easing(cubic_bezier(0.25, 1.0, 0.5, 1.0)),
            Self::Standard => Animation::new(Duration::from_secs_f64(MOTION_STANDARD_SECONDS))
                .with_easing(cubic_bezier(0.22, 1.0, 0.36, 1.0)),
            Self::Expand => Animation::new(Duration::from_secs_f64(MOTION_EXPAND_SECONDS))
                .with_easing(cubic_bezier(0.16, 1.0, 0.3, 1.0)),
        }
    }
}

pub(in crate::ui::app) fn with_optional_motion<E>(
    element: E,
    id: impl Into<SharedString>,
    speed: MotionSpeed,
    final_state: impl FnOnce(E) -> E,
    animator: impl Fn(E, f32) -> E + 'static,
) -> AnyElement
where
    E: IntoElement + 'static,
{
    if ui_animations_enabled() {
        element
            .with_animation(
                gpui::ElementId::from(id.into()),
                speed.animation(),
                animator,
            )
            .into_any_element()
    } else {
        final_state(element).into_any_element()
    }
}

pub(in crate::ui::app) fn begin_expandable_motion(id: impl Into<String>, expanded: bool) {
    if !ui_animations_enabled() {
        return;
    }

    let id = id.into();
    let started = Instant::now();
    let to_progress = if expanded { 1.0 } else { 0.0 };
    if let Ok(mut state) = EXPANDABLE_MOTION_STATE.lock() {
        let from_progress = state
            .transitions
            .get(&id)
            .map(|transition| expandable_transition_progress(*transition, started))
            .unwrap_or(if expanded { 0.0 } else { 1.0 });
        let distance = (to_progress - from_progress).abs();

        if distance <= 0.001 {
            state.transitions.remove(&id);
            return;
        }

        let duration = Duration::from_secs_f64(
            (MOTION_EXPAND_SECONDS * distance as f64)
                .clamp(MOTION_EXPAND_MIN_SECONDS, MOTION_EXPAND_SECONDS),
        );
        state.transitions.insert(
            id.clone(),
            ExpandableTransition {
                from_progress,
                to_progress,
                started,
                duration,
            },
        );
    }
}

pub(in crate::ui::app) fn expandable_motion_progress(id: &str) -> Option<f32> {
    let Ok(mut state) = EXPANDABLE_MOTION_STATE.lock() else {
        return None;
    };

    let now = Instant::now();
    let transition = state.transitions.get(id).copied()?;
    if now.saturating_duration_since(transition.started) >= transition.duration {
        state.transitions.remove(id);
        Some(transition.to_progress)
    } else {
        Some(expandable_transition_progress(transition, now))
    }
}

pub(in crate::ui::app) fn expandable_motion_progress_snapshot(id: &str) -> Option<f32> {
    let Ok(state) = EXPANDABLE_MOTION_STATE.lock() else {
        return None;
    };

    let now = Instant::now();
    let transition = state.transitions.get(id).copied()?;
    if now.saturating_duration_since(transition.started) >= transition.duration {
        Some(transition.to_progress)
    } else {
        Some(expandable_transition_progress(transition, now))
    }
}

pub(in crate::ui::app) fn expandable_motion_active(id: &str) -> bool {
    let Ok(mut state) = EXPANDABLE_MOTION_STATE.lock() else {
        return false;
    };

    let Some(transition) = state.transitions.get(id).copied() else {
        return false;
    };

    if Instant::now().saturating_duration_since(transition.started) >= transition.duration {
        state.transitions.remove(id);
        transition.to_progress > 0.001
    } else {
        true
    }
}

pub(in crate::ui::app) fn expandable_transition_progress(
    transition: ExpandableTransition,
    now: Instant,
) -> f32 {
    let duration = transition.duration.as_secs_f32().max(f32::EPSILON);
    let raw = now
        .saturating_duration_since(transition.started)
        .as_secs_f32()
        / duration;
    let expanding = transition.to_progress >= transition.from_progress;
    let eased = expandable_motion_ease(raw.clamp(0.0, 1.0), expanding);
    (transition.from_progress + (transition.to_progress - transition.from_progress) * eased)
        .clamp(0.0, 1.0)
}

pub(in crate::ui::app) fn expandable_motion_ease(delta: f32, expanding: bool) -> f32 {
    if expanding {
        1.0 - (1.0 - delta).powi(3)
    } else {
        delta * delta * (3.0 - 2.0 * delta)
    }
}

pub(in crate::ui::app) fn begin_control_motion(
    id: impl Into<String>,
    target_on: bool,
    cx: &mut App,
) {
    let id = id.into();
    let started = Instant::now();
    let target_state = if target_on { "true" } else { "false" };
    let to_progress = if target_on { 1.0 } else { 0.0 };
    let mut generation = None;

    if let Ok(mut state) = CONTROL_MOTION_STATE.lock() {
        let from_progress = state
            .transitions
            .get(&id)
            .map(|transition| control_transition_progress(*transition, started))
            .unwrap_or_else(|| {
                state
                    .values
                    .get(&id)
                    .map(|state| control_state_progress(state))
                    .unwrap_or(if target_on { 0.0 } else { 1.0 })
            });

        state.values.insert(id.clone(), target_state.to_owned());

        if !ui_animations_enabled() {
            state.transitions.remove(&id);
            cx.refresh_windows();
            return;
        }

        let distance = (to_progress - from_progress).abs();
        if distance <= 0.001 {
            state.transitions.remove(&id);
            cx.refresh_windows();
            return;
        }

        state.generation = state.generation.wrapping_add(1);
        let transition_generation = state.generation;
        let duration = Duration::from_secs_f64(
            (MOTION_CONTROL_SECONDS * distance as f64)
                .clamp(MOTION_CONTROL_MIN_SECONDS, MOTION_CONTROL_SECONDS),
        );

        state.transitions.insert(
            id.clone(),
            ControlTransition {
                from_progress,
                to_progress,
                started,
                duration,
                generation: transition_generation,
            },
        );
        generation = Some(transition_generation);
    }

    cx.refresh_windows();

    let Some(generation) = generation else {
        return;
    };

    cx.spawn(async move |cx| loop {
        Timer::after(MOTION_CONTROL_FRAME_INTERVAL).await;
        let active = control_motion_active(&id, generation);
        let _ = cx.update(|cx| cx.refresh_windows());

        if !active {
            break;
        }
    })
    .detach();
}

pub(in crate::ui::app) fn control_motion_progress(id: &str, target_on: bool) -> f32 {
    let target_progress = if target_on { 1.0 } else { 0.0 };
    let Ok(mut state) = CONTROL_MOTION_STATE.lock() else {
        return target_progress;
    };

    let now = Instant::now();
    let Some(transition) = state.transitions.get(id).copied() else {
        return target_progress;
    };

    if now.saturating_duration_since(transition.started) >= transition.duration {
        state.transitions.remove(id);
        transition.to_progress
    } else {
        control_transition_progress(transition, now)
    }
}

pub(in crate::ui::app) fn control_motion_active(id: &str, generation: u64) -> bool {
    let Ok(mut state) = CONTROL_MOTION_STATE.lock() else {
        return false;
    };

    let Some(transition) = state.transitions.get(id).copied() else {
        return false;
    };

    if transition.generation != generation {
        return false;
    }

    if Instant::now().saturating_duration_since(transition.started) >= transition.duration {
        state.transitions.remove(id);
        false
    } else {
        true
    }
}

pub(in crate::ui::app) fn control_transition_progress(
    transition: ControlTransition,
    now: Instant,
) -> f32 {
    let duration = transition.duration.as_secs_f32().max(f32::EPSILON);
    let raw = now
        .saturating_duration_since(transition.started)
        .as_secs_f32()
        / duration;
    let expanding = transition.to_progress >= transition.from_progress;
    let eased = expandable_motion_ease(raw.clamp(0.0, 1.0), expanding);
    (transition.from_progress + (transition.to_progress - transition.from_progress) * eased)
        .clamp(0.0, 1.0)
}

pub(in crate::ui::app) fn control_state_progress(state: &str) -> f32 {
    match state {
        "true" | "checked" | "enabled" | "visible" => 1.0,
        _ => 0.0,
    }
}

pub(in crate::ui::app) fn control_motion_generation(id: &str, state: &str) -> Option<u64> {
    let Ok(mut motion_state) = CONTROL_MOTION_STATE.lock() else {
        return None;
    };

    let previous = motion_state.values.insert(id.to_owned(), state.to_owned());
    if previous.is_some_and(|previous| previous != state) && ui_animations_enabled() {
        motion_state.generation = motion_state.generation.wrapping_add(1);
        Some(motion_state.generation)
    } else {
        None
    }
}

pub(in crate::ui::app) fn with_state_change_motion<E>(
    element: E,
    id: impl Into<SharedString>,
    state: impl Into<SharedString>,
    speed: MotionSpeed,
    final_state: impl FnOnce(E) -> E,
    animator: impl Fn(E, f32) -> E + 'static,
) -> AnyElement
where
    E: IntoElement + 'static,
{
    let id = id.into();
    let state = state.into();

    if let Some(generation) = control_motion_generation(id.as_ref(), state.as_ref()) {
        with_optional_motion(
            element,
            SharedString::from(format!("control-motion-{id}-{state}-{generation}")),
            speed,
            final_state,
            animator,
        )
    } else {
        final_state(element).into_any_element()
    }
}

pub(in crate::ui::app) fn card_hover_snapshot(id: &str) -> (bool, Option<u64>) {
    let Ok(state) = CARD_HOVER_STATE.lock() else {
        return (false, None);
    };
    let hovered = state.hovered.contains(id);
    let animation_generation = state.changes.get(id).and_then(|change| {
        let fresh =
            change.changed_at.elapsed() <= Duration::from_secs_f64(MOTION_FAST_SECONDS + 0.05);
        (change.hovered == hovered && fresh).then_some(change.generation)
    });

    (hovered, animation_generation)
}

pub(in crate::ui::app) fn set_card_hovered(id: String, hovered: bool, cx: &mut App) {
    let Ok(mut state) = CARD_HOVER_STATE.lock() else {
        return;
    };

    let changed = if hovered {
        state.hovered.insert(id.clone())
    } else {
        state.hovered.remove(&id)
    };

    if changed {
        state.generation = state.generation.wrapping_add(1);
        let generation = state.generation;
        state.changes.insert(
            id,
            CardHoverChange {
                hovered,
                generation,
                changed_at: Instant::now(),
            },
        );
        cx.refresh_windows();
    }
}

pub(in crate::ui::app) fn clear_page_hovered() {
    let Ok(mut state) = CARD_HOVER_STATE.lock() else {
        return;
    };

    state.hovered.clear();
    state.changes.clear();
}

pub(in crate::ui::app) fn animated_card_hover_layer(id: &str) -> AnyElement {
    let (hovered, animation_generation) = card_hover_snapshot(id);
    let target_opacity = if hovered { 1.0 } else { 0.0 };
    let layer = div()
        .absolute()
        .inset_0()
        .bg(rgb(settings_card_hover_color()))
        .opacity(target_opacity);

    if ui_animations_enabled() {
        if let Some(generation) = animation_generation {
            let start_opacity = if hovered { 0.0 } else { 1.0 };
            return with_optional_motion(
                layer,
                SharedString::from(format!("card-hover-{id}-{generation}")),
                MotionSpeed::Fast,
                move |layer| layer.opacity(target_opacity),
                move |layer, delta| {
                    let opacity = start_opacity + (target_opacity - start_opacity) * delta;
                    layer.opacity(opacity)
                },
            );
        }
    }

    layer.into_any_element()
}

pub(in crate::ui::app) fn collapsible_chevron_icon(
    id: impl Into<SharedString>,
    collapsed: bool,
) -> AnyElement {
    let id = id.into();
    if let Some(progress) = expandable_motion_progress_snapshot(id.as_ref()) {
        return chevron_right_at_progress(progress);
    }

    chevron_right_at_progress(if collapsed { 0.0 } else { 1.0 })
}

pub(in crate::ui::app) fn collapsible_chevron_icon_with_progress(
    id: impl Into<SharedString>,
    collapsed: bool,
    progress: Option<f32>,
) -> AnyElement {
    if let Some(progress) = progress {
        return chevron_right_at_progress(progress);
    }

    collapsible_chevron_icon(id, collapsed)
}

pub(in crate::ui::app) fn chevron_right_at_progress(progress: f32) -> AnyElement {
    Icon::new(NavIcon::ChevronRight)
        .with_size(px(16.0))
        .rotate(percentage(progress.clamp(0.0, 1.0) * 90.0 / 360.0))
        .into_any_element()
}

pub(in crate::ui::app) fn handle_navigation_mouse_button(
    app: &mut WinderustApp,
    button: MouseButton,
    cx: &mut Context<WinderustApp>,
) -> bool {
    match button {
        MouseButton::Navigate(NavigationDirection::Back) => {
            app.navigate_back(cx);
            cx.stop_propagation();
            true
        }
        MouseButton::Navigate(NavigationDirection::Forward) => {
            app.navigate_forward(cx);
            cx.stop_propagation();
            true
        }
        _ => false,
    }
}

pub(in crate::ui::app) fn animated_expanded_child(
    id: impl Into<SharedString>,
    child: AnyElement,
) -> AnyElement {
    let id = id.into();
    let motion_id = format!("expanded-child-{id}");
    let animation_generation = control_motion_generation(&motion_id, "visible");
    let container = div().w_full().min_w(px(0.0)).overflow_hidden().child(child);

    if let Some(generation) = animation_generation {
        with_optional_motion(
            container,
            SharedString::from(format!("{motion_id}-{generation}")),
            MotionSpeed::Expand,
            |container| container,
            |container, delta| {
                container
                    .max_h(px(EXPANDED_CHILD_MAX_ANIMATION_HEIGHT * delta))
                    .opacity(0.04 + 0.96 * delta)
            },
        )
    } else {
        container.into_any_element()
    }
}

pub(in crate::ui::app) fn expanded_child(child: AnyElement) -> AnyElement {
    div()
        .w_full()
        .min_w(px(0.0))
        .overflow_hidden()
        .child(child)
        .into_any_element()
}

pub(in crate::ui::app) fn animated_expanded_child_with_height(
    id: impl Into<SharedString>,
    target_height: f32,
    child: impl IntoElement + 'static,
) -> AnyElement {
    let id = id.into();
    let motion_id = format!("expanded-child-{id}");
    let animation_generation = control_motion_generation(&motion_id, "visible");
    let target_height = target_height.max(1.0);
    let container = div()
        .w_full()
        .min_w(px(0.0))
        .overflow_hidden()
        .child(child.into_any_element());

    if let Some(generation) = animation_generation {
        with_optional_motion(
            container,
            SharedString::from(format!("{motion_id}-{generation}")),
            MotionSpeed::Expand,
            |container| container,
            move |container, delta| {
                container
                    .h(px(target_height * delta))
                    .opacity(0.04 + 0.96 * delta)
            },
        )
    } else {
        container.into_any_element()
    }
}

pub(in crate::ui::app) fn remember_expanded_child_hidden(id: impl Into<SharedString>) {
    let id = id.into();
    let _ = control_motion_generation(&format!("expanded-child-{id}"), "hidden");
}

pub(in crate::ui::app) fn animated_rule_card_body_child(
    card_target: &RuleCardTarget,
    index: usize,
    row_count: usize,
    child: impl IntoElement + 'static,
) -> AnyElement {
    animated_rule_card_body_child_with_height(
        card_target,
        index,
        rule_card_body_height(row_count),
        child,
    )
}

pub(in crate::ui::app) fn animated_rule_card_body_child_with_height(
    card_target: &RuleCardTarget,
    index: usize,
    target_height: f32,
    child: impl IntoElement + 'static,
) -> AnyElement {
    let row_id = SharedString::from(format!("rule-card-{card_target:?}-body-{index}"));
    let child = div()
        .id(row_id)
        .w_full()
        .min_w(px(0.0))
        .child(child.into_any_element())
        .into_any_element();
    let target_height = target_height.max(1.0);
    if let Some(progress) =
        expandable_motion_progress_snapshot(&rule_card_body_motion_id(card_target))
    {
        expanded_child_at_progress(child, Some(target_height), progress)
    } else {
        div()
            .w_full()
            .min_w(px(0.0))
            .h(px(target_height))
            .overflow_hidden()
            .child(child)
            .into_any_element()
    }
}

pub(in crate::ui::app) fn rule_card_body_height(row_count: usize) -> f32 {
    CARD_ROW_HEIGHT * row_count.max(1) as f32
}

pub(in crate::ui::app) fn core_steering_selector_body_height() -> f32 {
    rule_card_body_height(1)
        + px_spacing(3) * 2.0
        + TEXT_BODY_LINE_HEIGHT
        + px_spacing(2)
        + core_tile_grid_height()
}

pub(in crate::ui::app) fn setting_group_core_grid_body_height(fixed_row_count: usize) -> f32 {
    CARD_ROW_HEIGHT * fixed_row_count.max(1) as f32
        + px_spacing(3) * 2.0
        + TEXT_BODY_LINE_HEIGHT
        + px_spacing(2)
        + core_tile_grid_height()
}

pub(in crate::ui::app) fn core_tile_grid_height() -> f32 {
    let processor_count = core_steering::logical_processors().len();
    let grid_rows =
        processor_count.saturating_add(CORE_TILE_GRID_COLUMNS - 1) / CORE_TILE_GRID_COLUMNS;
    if grid_rows == 0 {
        TEXT_BODY_LINE_HEIGHT
    } else {
        let row_gaps = grid_rows.saturating_sub(1) as f32 * CORE_TILE_GRID_GAP;
        grid_rows as f32 * CORE_TILE_HEIGHT + row_gaps
    }
}

pub(in crate::ui::app) fn px_spacing(slot: usize) -> f32 {
    slot as f32 * 4.0
}

pub(in crate::ui::app) fn rule_card_body_motion_id(card_target: &RuleCardTarget) -> String {
    format!("rule-card-{card_target:?}-body")
}

pub(in crate::ui::app) fn rule_card_body_visible(
    card_target: &RuleCardTarget,
    collapsed: bool,
    window: &mut Window,
) -> bool {
    let motion_active = expandable_motion_active(&rule_card_body_motion_id(card_target));
    if motion_active {
        window.request_animation_frame();
    }

    !collapsed || motion_active
}

pub(in crate::ui::app) fn animated_presence_child(
    id: impl Into<SharedString>,
    child: AnyElement,
) -> AnyElement {
    let id = id.into();
    let container = div().w_full().min_w(px(0.0)).overflow_hidden().child(child);
    with_optional_motion(
        container,
        SharedString::from(format!("presence-{id}")),
        MotionSpeed::Fast,
        |container| container,
        |container, delta| container.opacity(0.18 + 0.82 * delta),
    )
}

pub(in crate::ui::app) fn animated_list_item_child(
    id: impl Into<SharedString>,
    child: AnyElement,
    exiting: bool,
) -> AnyElement {
    let id = id.into();
    let container = div()
        .w_full()
        .min_w(px(0.0))
        .overflow_hidden()
        .when(exiting, |container| {
            container.block_mouse_except_scroll().cursor_default()
        })
        .child(child);

    if exiting {
        with_optional_motion(
            container,
            SharedString::from(format!("presence-exit-{id}")),
            MotionSpeed::Fast,
            |container| container.opacity(0.0),
            |container, delta| container.opacity(1.0 - delta),
        )
    } else {
        container.into_any_element()
    }
}
