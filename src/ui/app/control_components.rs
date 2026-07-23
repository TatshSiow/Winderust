use super::*;

pub(super) fn app_input(
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

pub(super) fn app_input_color(focused: bool) -> u32 {
    if focused {
        settings_card_hover_color()
    } else {
        settings_card_color()
    }
}

pub(super) fn app_input_border_color(focused: bool) -> u32 {
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

pub(super) fn app_input_hover_border_color() -> u32 {
    if ui_is_dark() {
        0x6a6a6a
    } else {
        0x9a9a9a
    }
}

pub(super) fn syncing_rule_card(index: usize) -> AnyElement {
    section_card(&t!("common.rule", number = index + 1))
        .child(syncing_input_message())
        .into_any_element()
}

pub(super) fn rule_card_title(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        t!("common.unnamed_rule").to_string()
    } else {
        name.to_owned()
    }
}

pub(super) fn status_pill(label: impl Into<SharedString>, bg: u32, fg: u32) -> AnyElement {
    status_pill_div(label, bg, fg).into_any_element()
}

pub(super) fn status_pill_with_tooltip(
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

pub(super) fn status_pill_div(label: impl Into<SharedString>, bg: u32, fg: u32) -> gpui::Div {
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

pub(super) fn animated_checkmark(
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

pub(super) fn checkbox_box(
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
        .id(id.clone())
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

pub(super) fn rule_enable_checkbox(
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

pub(super) fn syncing_input_message() -> gpui::Div {
    text_muted(t!("common.syncing_rule_editor").to_string())
}

pub(super) fn checkbox(
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

pub(super) fn title_bar_controls(window: &Window, cx: &mut Context<WinderustApp>) -> AnyElement {
    let (maximize_id, maximize_icon) = if window.is_maximized() {
        ("titlebar-restore", "\u{e923}")
    } else {
        ("titlebar-maximize", "\u{e922}")
    };

    h_flex()
        .id("titlebar-controls")
        .h_full()
        .flex_none()
        .font_family(FONT_WINDOW_CONTROLS)
        .child(title_bar_control_button(
            "titlebar-minimize",
            "\u{e921}",
            WindowControlArea::Min,
            false,
            cx,
        ))
        .child(title_bar_control_button(
            maximize_id,
            maximize_icon,
            WindowControlArea::Max,
            false,
            cx,
        ))
        .child(title_bar_control_button(
            "titlebar-close",
            "\u{e8bb}",
            WindowControlArea::Close,
            true,
            cx,
        ))
        .into_any_element()
}

pub(super) fn title_bar_control_button(
    id: &'static str,
    icon: &'static str,
    control_area: WindowControlArea,
    is_close: bool,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let hover_bg = if is_close {
        cx.theme().danger_hover
    } else {
        cx.theme().secondary_hover
    };
    let active_bg = if is_close {
        cx.theme().danger_active
    } else {
        cx.theme().secondary_active
    };

    h_flex()
        .id(id)
        .window_control_area(control_area)
        .occlude()
        .flex_none()
        .w(px(TITLE_BAR_CONTROL_WIDTH))
        .h(px(TITLE_BAR_HEIGHT))
        .items_center()
        .justify_center()
        .text_size(px(TITLE_BAR_CONTROL_ICON_SIZE))
        .line_height(px(TITLE_BAR_CONTROL_ICON_LINE_HEIGHT))
        .text_color(cx.theme().muted_foreground)
        .hover(move |style| style.bg(hover_bg))
        .active(move |style| style.bg(active_bg))
        .child(icon)
        .into_any_element()
}

pub(super) fn section_landing_card(
    page: Page,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    let trailing = h_flex()
        .items_center()
        .justify_end()
        .gap_2()
        .flex_shrink_0()
        .child(
            Icon::new(NavIcon::ChevronRight)
                .with_size(px(16.0))
                .text_color(cx.theme().muted_foreground),
        );
    let id = SharedString::from(format!("section-card-{page:?}"));
    let hover_id = id.to_string();

    h_flex()
        .id(id)
        .w_full()
        .min_w(px(0.0))
        .h(px(CARD_ROW_HEIGHT))
        .items_center()
        .justify_between()
        .gap_3()
        .py_3()
        .px_4()
        .relative()
        .overflow_hidden()
        .rounded(px(BRAND_RADIUS_SURFACE))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
        .on_hover({
            let hover_id = hover_id.clone();
            move |hovered, _, cx| {
                set_card_hovered(hover_id.clone(), *hovered, cx);
            }
        })
        .cursor_pointer()
        .child(animated_card_hover_layer(&hover_id))
        .child(nav_icon(page, true, cx))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(TEXT_BODY_SIZE))
                .line_height(px(TEXT_BODY_LINE_HEIGHT))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(primary_text_color()))
                .child(page.label()),
        )
        .child(trailing)
}

pub(super) fn nav_row(
    page: Page,
    selected: bool,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    let row_id = SharedString::from(format!("nav-row-{page:?}"));
    let hover_id = row_id.to_string();
    let (hovered, _) = card_hover_snapshot(&hover_id);
    let bg_layer = animated_nav_row_bg(&hover_id, selected);
    let content_opacity = if selected || hovered { 1.0 } else { 0.86 };
    let hover_row_id = (!selected).then_some(hover_id.clone());

    h_flex()
        .id(row_id)
        .h(px(40.0))
        .w_full()
        .items_center()
        .gap_3()
        .pl(px(0.0))
        .pr(px(12.0))
        .relative()
        .overflow_hidden()
        .rounded(px(BRAND_RADIUS_CONTROL))
        .text_color(cx.theme().sidebar_foreground)
        .on_hover({
            let hover_row_id = hover_row_id.clone();
            move |hovered, _, cx| {
                if let Some(hover_id) = hover_row_id.as_ref() {
                    set_card_hovered(hover_id.clone(), *hovered, cx);
                }
            }
        })
        .cursor_pointer()
        .child(bg_layer)
        .child(nav_selection_indicator(page, selected))
        .child(nav_icon(page, selected, cx))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .opacity(content_opacity)
                .text_size(px(TEXT_CONTROL_SIZE))
                .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
                .truncate()
                .child(page.label()),
        )
}

pub(super) fn animated_nav_row_bg(id: &str, selected: bool) -> AnyElement {
    let (hovered, _) = card_hover_snapshot(id);
    let hover_active = !selected && hovered;
    let selected_layer = div()
        .absolute()
        .inset_0()
        .rounded(px(BRAND_RADIUS_CONTROL))
        .bg(rgb(sidebar_selected_color()))
        .opacity(if selected { 1.0 } else { 0.0 });
    let hover_layer = div()
        .absolute()
        .inset_0()
        .rounded(px(BRAND_RADIUS_CONTROL))
        .bg(rgb(sidebar_hover_color()))
        .opacity(if hover_active { 0.86 } else { 0.0 });

    div()
        .absolute()
        .inset_0()
        .child(with_optional_motion(
            selected_layer,
            SharedString::from(format!("nav-row-selected-{id}-{selected}")),
            MotionSpeed::Fast,
            |layer| layer,
            move |layer, delta| layer.opacity(if selected { delta } else { 1.0 - delta }),
        ))
        .child(with_optional_motion(
            hover_layer,
            SharedString::from(format!("nav-row-hover-{id}-{hover_active}")),
            MotionSpeed::Fast,
            |layer| layer,
            move |layer, delta| {
                let progress = if hover_active { delta } else { 1.0 - delta };
                layer.opacity(0.86 * progress)
            },
        ))
        .into_any_element()
}

pub(super) fn nav_selection_indicator(page: Page, selected: bool) -> AnyElement {
    let indicator = div()
        .w(px(3.0))
        .h(px(20.0))
        .rounded(px(BRAND_RADIUS_CONTROL))
        .bg(rgb(accent_color()))
        .opacity(if selected { 1.0 } else { 0.0 });

    with_optional_motion(
        indicator,
        SharedString::from(format!("nav-selection-indicator-{page:?}-{selected}")),
        MotionSpeed::Fast,
        |indicator| indicator,
        move |indicator, delta| {
            let progress = if selected { delta } else { 1.0 - delta };
            let height = 4.0 + 16.0 * progress;
            indicator.h(px(height)).opacity(progress)
        },
    )
}

pub(super) fn nav_icon(page: Page, selected: bool, cx: &mut Context<WinderustApp>) -> AnyElement {
    let color = if selected {
        rgb(accent_color()).into()
    } else {
        cx.theme().muted_foreground
    };

    let icon = div()
        .w(px(22.0))
        .h(px(22.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .child(
            Icon::new(nav_icon_name(page))
                .with_size(px(18.0))
                .text_color(color),
        );

    icon.into_any_element()
}

pub(super) fn nav_icon_name(page: Page) -> NavIcon {
    match page {
        Page::Home => NavIcon::House,
        Page::PowerPlanControl => NavIcon::Zap,
        Page::WinderustFeatures => NavIcon::Feather,
        Page::CpuControl => NavIcon::Cpu,
        Page::PriorityControl => NavIcon::CircleFadingArrowUp,
        Page::SettingsHome => NavIcon::Settings,
        Page::AdvancedControls => NavIcon::Cog,
        Page::ByActivity => NavIcon::SquareActivity,
        Page::ByCpuLoad => NavIcon::ChartColumn,
        Page::AdvancedPowerPlanTuning => NavIcon::Drill,
        Page::ProcessPriority => NavIcon::PanelsTopLeft,
        Page::ThreadPriority => NavIcon::Spline,
        Page::DynamicPriorityBoost => NavIcon::TrendingUpDown,
        Page::CoreLimiter => NavIcon::OctagonMinus,
        Page::BackgroundCpuRestriction => NavIcon::MonitorX,
        Page::ProcessList => NavIcon::List,
        Page::AdaptiveEngine => NavIcon::Leaf,
        Page::BackgroundEfficiency => NavIcon::Leaf,
        Page::AppSuspension => NavIcon::MonitorPause,
        Page::ByRunningApp => NavIcon::Footprints,
        Page::IoPriority => NavIcon::Rotate3d,
        Page::GpuPriority => NavIcon::Gpu,
        Page::MemoryPriority => NavIcon::MemoryStick,
        Page::MemoryTrim => NavIcon::Scissors,
        Page::CoreSteering => NavIcon::LifeBuoy,
        Page::ByForeground => NavIcon::BringToFront,
        Page::ByTime => NavIcon::CalendarDays,
        Page::ActionLog => NavIcon::Info,
        Page::WinderustBehaviour => NavIcon::Settings,
        Page::LanguageAndAppearance => NavIcon::Palette,
        Page::ExperimentalFeatures => NavIcon::FlaskConical,
        Page::TimerResolution => NavIcon::Hourglass,
        Page::Win32PrioritySeparation => NavIcon::Wrench,
        Page::About => NavIcon::Info,
    }
}

#[derive(Clone, Copy)]
pub(super) enum NavIcon {
    AppWindow,
    BringToFront,
    CalendarDays,
    ChartColumn,
    ChevronDown,
    ChevronRight,
    CircleFadingArrowUp,
    Cog,
    Cpu,
    Drill,
    Feather,
    FlaskConical,
    Footprints,
    Gpu,
    Hourglass,
    House,
    Info,
    Leaf,
    LifeBuoy,
    List,
    MemoryStick,
    MonitorPause,
    MonitorX,
    OctagonMinus,
    Palette,
    PanelsTopLeft,
    Rotate3d,
    Scissors,
    Settings,
    Snowflake,
    Spline,
    SquareActivity,
    SquarePen,
    Trash2,
    TrendingUpDown,
    Wrench,
    Zap,
}

impl IconNamed for NavIcon {
    fn path(self) -> SharedString {
        match self {
            Self::AppWindow => "icons/app-window.svg",
            Self::BringToFront => "icons/bring-to-front.svg",
            Self::CalendarDays => "icons/calendar-days.svg",
            Self::ChartColumn => "icons/chart-column.svg",
            Self::ChevronDown => "icons/chevron-down.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::CircleFadingArrowUp => "icons/circle-fading-arrow-up.svg",
            Self::Cog => "icons/cog.svg",
            Self::Cpu => "icons/cpu.svg",
            Self::Drill => "icons/drill.svg",
            Self::Feather => "icons/feather.svg",
            Self::FlaskConical => "icons/flask-conical.svg",
            Self::Footprints => "icons/footprints.svg",
            Self::Gpu => "icons/gpu.svg",
            Self::Hourglass => "icons/hourglass.svg",
            Self::House => "icons/house.svg",
            Self::Info => "icons/info.svg",
            Self::Leaf => "icons/leaf.svg",
            Self::LifeBuoy => "icons/life-buoy.svg",
            Self::List => "icons/list.svg",
            Self::MemoryStick => "icons/memory-stick.svg",
            Self::MonitorPause => "icons/monitor-pause.svg",
            Self::MonitorX => "icons/monitor-x.svg",
            Self::OctagonMinus => "icons/octagon-minus.svg",
            Self::Palette => "icons/palette.svg",
            Self::PanelsTopLeft => "icons/panels-top-left.svg",
            Self::Rotate3d => "icons/rotate-3d.svg",
            Self::Scissors => "icons/scissors.svg",
            Self::Settings => "icons/settings.svg",
            Self::Snowflake => "icons/snowflake.svg",
            Self::Spline => "icons/spline.svg",
            Self::SquareActivity => "icons/square-activity.svg",
            Self::SquarePen => "icons/square-pen.svg",
            Self::Trash2 => "icons/trash-2.svg",
            Self::TrendingUpDown => "icons/trending-up-down.svg",
            Self::Wrench => "icons/wrench.svg",
            Self::Zap => "icons/zap.svg",
        }
        .into()
    }
}

pub(super) fn control_button(button: Button) -> Button {
    button
        .small()
        .h(px(32.0))
        .text_size(px(TEXT_CONTROL_SIZE))
        .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
}

pub(super) fn primary_control_button(button: Button, cx: &mut Context<WinderustApp>) -> Button {
    control_button(button.primary()).text_color(cx.theme().primary_foreground)
}

pub(super) fn danger_control_button(button: Button) -> Button {
    control_button(button.danger()).text_color(rgb(0xffffff))
}

pub(super) fn remove_control_button(button: Button) -> Button {
    danger_control_button(button)
        .with_size(px(32.0))
        .icon(Icon::new(NavIcon::Trash2).with_size(px(14.0)))
        .tooltip(t!("common.remove").to_string())
}

pub(super) fn accent_swatch(
    id_prefix: &'static str,
    color: u32,
    selected: bool,
) -> gpui::Stateful<gpui::Div> {
    let border = if selected {
        primary_text_color()
    } else {
        color
    };

    div()
        .id(SharedString::from(format!("{id_prefix}-{color:06x}")))
        .size(px(ACCENT_SWATCH_SIZE))
        .flex_shrink_0()
        .rounded(px(BRAND_RADIUS_CONTROL))
        .border_1()
        .border_color(rgb(border))
        .bg(rgb(color))
        .hover(|style| style.border_color(rgb(primary_text_color())))
        .cursor_pointer()
        .when(selected, |style| style.border_2())
}

pub(super) fn accent_color_group(
    title: impl Into<SharedString>,
    swatches: AnyElement,
) -> gpui::Div {
    v_flex()
        .w_full()
        .min_w(px(0.0))
        .gap_2()
        .child(section_title_label(title))
        .child(swatches)
}

pub(super) fn accent_picker_featured_colors() -> Vec<Hsla> {
    ACCENT_PALETTE
        .iter()
        .copied()
        .map(|color| rgb(color).into())
        .collect()
}

pub(super) fn add_custom_accent_color(settings: &mut AccentSettings, color: u32) {
    settings
        .custom_colors
        .retain(|stored_color| *stored_color != color);
    settings.custom_colors.push(color);
}

pub(super) fn remove_custom_accent_color(settings: &mut AccentSettings, color: u32) {
    settings
        .custom_colors
        .retain(|stored_color| *stored_color != color);
}

pub(super) fn hsla_to_rgb_u32(color: Hsla) -> Option<u32> {
    let hex = color.to_hex();
    let hex = hex.trim_start_matches('#');
    if hex.len() < 6 {
        return None;
    }
    u32::from_str_radix(&hex[..6], 16).ok()
}

pub(super) fn accent_palette_animation_height(custom_color_count: usize) -> f32 {
    let palette_rows = accent_swatch_row_count(ACCENT_PALETTE.len());
    px_spacing(6)
        + accent_custom_color_group_height(custom_color_count)
        + px_spacing(4)
        + accent_color_group_height(palette_rows)
}

pub(super) fn accent_custom_color_group_height(custom_color_count: usize) -> f32 {
    let swatch_rows = accent_swatch_row_count(custom_color_count);
    let swatch_height = if swatch_rows == 0 {
        0.0
    } else {
        ACCENT_SWATCH_SIZE * swatch_rows as f32
            + px_spacing(2) * swatch_rows.saturating_sub(1) as f32
    };
    TEXT_BODY_LINE_HEIGHT + px_spacing(2) + swatch_height.max(ACCENT_COLOR_PICKER_WRAPPER_SIZE)
}

pub(super) fn accent_color_group_height(row_count: usize) -> f32 {
    let row_count = row_count.max(1);
    TEXT_BODY_LINE_HEIGHT
        + px_spacing(2)
        + ACCENT_SWATCH_SIZE * row_count as f32
        + px_spacing(2) * row_count.saturating_sub(1) as f32
}

pub(super) fn accent_swatch_row_count(swatch_count: usize) -> usize {
    swatch_count.div_ceil(ACCENT_SWATCHES_PER_ROW)
}

pub(super) fn feature_toggle_switch(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    enabled: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    feature_toggle_switch_inner(id, label, None, enabled, handler)
}

pub(super) fn feature_toggle_switch_with_help(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    help: impl Into<SharedString>,
    enabled: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    feature_toggle_switch_inner(id, label, Some(help.into()), enabled, handler)
}

pub(super) fn feature_toggle_switch_inner(
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

pub(super) fn switch_toggle_action(
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

pub(super) fn switch_indicator(id: SharedString, enabled: bool) -> gpui::Div {
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

pub(super) fn value_pill(value: impl Into<SharedString>) -> gpui::Div {
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

pub(super) fn numeric_value_width(field: NumericField) -> f32 {
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
        | NumericField::MemoryTrimCpuIdleThreshold
        | NumericField::CoreLimiterThreshold(_)
        | NumericField::CoreLimiterMaxProcessors(_) => 76.0,
        NumericField::MemoryTrimCheckIntervalMinutes
        | NumericField::MemoryTrimPurgeFreeRamThreshold
        | NumericField::TimerResolutionRule(_) => 104.0,
        NumericField::MemoryTrimWorkingSetThreshold
        | NumericField::MemoryTrimIdleSeconds
        | NumericField::MemoryTrimCooldownSeconds => 112.0,
        NumericField::NetworkThreshold(_) => 76.0,
        _ => 96.0,
    }
}

pub(super) fn max_logical_processor_count() -> u8 {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .clamp(1, u8::MAX as usize) as u8
}

pub(super) fn text_muted(value: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .opacity(0.72)
        .child(value.into())
}

pub(super) fn text_warning(value: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .text_color(rgb(warning_text_color()))
        .child(value.into())
}

pub(super) fn processor_power_column_header(value: impl Into<SharedString>) -> gpui::Div {
    div()
        .w_full()
        .min_w(px(0.0))
        .pb_1()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::BOLD)
        .child(value.into())
}

pub(super) fn processor_power_slider(
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

pub(super) fn processor_power_setting_row(
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

pub(super) fn win32_priority_registry_value_row(
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

pub(super) fn win32_priority_row(
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

pub(super) fn threshold_level_slider(
    spec: SliderRowSpec<'_, u8>,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
    handler: impl Fn(&StepChange<u8>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    rule_percent_slider_row(spec, window, cx, handler)
}

pub(super) fn stable_slider(
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

pub(super) fn stable_slider_percentage(value: f32, min: u64, max: u64) -> f32 {
    let min = min as f32;
    let max = max as f32;
    let range = max - min;
    if range <= 0.0 {
        0.0
    } else {
        ((value.clamp(min, max) - min) / range).clamp(0.0, 1.0)
    }
}

pub(super) fn update_stable_slider_from_position(
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

pub(super) fn activity_slider_card(
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

pub(super) fn rule_percent_slider_row<T>(
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

pub(super) fn percent_slider_row<T>(
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

pub(super) fn u64_step(value: u64) -> u64 {
    if value >= 1_000 {
        100
    } else if value >= 100 {
        10
    } else {
        1
    }
}

pub(super) fn apply_u64_step(current: u64, change: &StepChange<u64>, min: u64, max: u64) -> u64 {
    let next = if change.increase {
        current.saturating_add(change.delta)
    } else {
        current.saturating_sub(change.delta)
    };
    next.clamp(min, max)
}

pub(super) fn apply_u8_step(current: u8, change: &StepChange<u8>, min: u8, max: u8) -> u8 {
    let next = if change.increase {
        current.saturating_add(change.delta)
    } else {
        current.saturating_sub(change.delta)
    };
    next.clamp(min, max)
}

pub(super) fn activity_slider_normalized_value(slider: ActivitySlider, value: u64) -> u64 {
    match slider {
        ActivitySlider::IdleTimeout => value.clamp(
            ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS,
            ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS,
        ),
        ActivitySlider::CheckInterval => snap_to_step(value, ACTIVITY_CHECK_INTERVAL_STEP_MS)
            .clamp(
                ACTIVITY_CHECK_INTERVAL_MIN_MS,
                ACTIVITY_CHECK_INTERVAL_MAX_MS,
            ),
    }
}

pub(super) fn snap_to_step(value: u64, step: u64) -> u64 {
    if step == 0 {
        return value;
    }
    ((value + (step / 2)) / step) * step
}

pub(super) fn seconds_label(seconds: u64) -> String {
    duration_label_ms(
        seconds
            .clamp(
                ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS,
                ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS,
            )
            .saturating_mul(1_000),
    )
}

pub(super) fn milliseconds_label(milliseconds: u64) -> String {
    duration_label_ms(
        snap_to_step(milliseconds, ACTIVITY_CHECK_INTERVAL_STEP_MS).clamp(
            ACTIVITY_CHECK_INTERVAL_MIN_MS,
            ACTIVITY_CHECK_INTERVAL_MAX_MS,
        ),
    )
}

pub(super) fn duration_label_ms(milliseconds: u64) -> String {
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

pub(super) fn rounded_duration_value(value: f64, unit: &str) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if (rounded - rounded.round()).abs() < f64::EPSILON {
        format!("{} {unit}", rounded.round() as u64)
    } else {
        format!("{rounded:.1} {unit}")
    }
}

pub(super) fn parse_u64_input(value: &str, min: u64, max: u64) -> Option<u64> {
    value.parse::<u64>().ok().map(|value| value.clamp(min, max))
}

pub(super) fn parse_timer_resolution_input_100ns(
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

pub(super) fn cpu_usage_label(percent: Option<f32>) -> String {
    percent
        .map(|percent| format!("{percent:.1}%"))
        .unwrap_or_else(|| t!("home.collecting").to_string())
}

pub(super) fn cpu_frequency_label(frequency_mhz: Option<u32>) -> String {
    frequency_mhz
        .map(|frequency_mhz| {
            if frequency_mhz >= 1_000 {
                format!("{:.2} GHz", frequency_mhz as f64 / 1_000.0)
            } else {
                format!("{frequency_mhz} MHz")
            }
        })
        .unwrap_or_else(|| t!("home.collecting").to_string())
}

pub(super) fn memory_usage_label(percent: Option<f32>) -> String {
    percent
        .map(|percent| format!("{percent:.1}%"))
        .unwrap_or_else(|| t!("home.collecting").to_string())
}

pub(super) fn memory_usage_value_label(snapshot: MemoryUsageSnapshot) -> String {
    match (snapshot.used_physical_bytes, snapshot.total_physical_bytes) {
        (Some(used), Some(total)) => format_memory_used_total(used, total),
        _ => t!("home.collecting").to_string(),
    }
}

pub(super) fn memory_cache_value_label(snapshot: MemoryUsageSnapshot) -> String {
    snapshot
        .cached_physical_bytes
        .map(format_memory_capacity)
        .unwrap_or_else(|| t!("home.collecting").to_string())
}

pub(super) fn memory_cache_percent(snapshot: MemoryUsageSnapshot) -> Option<f32> {
    memory_bytes_percent(
        snapshot.cached_physical_bytes,
        snapshot.total_physical_bytes,
    )
}

pub(super) fn refresh_due(now: Instant, next_refresh: &mut Instant, interval: Duration) -> bool {
    if now < *next_refresh {
        return false;
    }

    *next_refresh = now + interval;
    true
}

pub(super) fn active_plan_guid(plans: &[PowerPlan]) -> Option<&str> {
    plans
        .iter()
        .find(|plan| plan.active)
        .map(|plan| plan.guid.as_str())
}

pub(super) fn memory_bytes_percent(bytes: Option<u64>, total_bytes: Option<u64>) -> Option<f32> {
    let bytes = bytes?;
    let total_bytes = total_bytes?;
    if total_bytes == 0 {
        return None;
    }

    Some(((bytes as f64 / total_bytes as f64) * 100.0).clamp(0.0, 100.0) as f32)
}

pub(super) fn io_usage_label(bytes_per_second: Option<f64>) -> String {
    bytes_per_second
        .map(format_bytes_per_second)
        .unwrap_or_else(|| t!("home.collecting").to_string())
}

pub(super) fn format_memory_used_total(used_bytes: u64, total_bytes: u64) -> String {
    let used = memory_capacity_parts(used_bytes);
    let total = memory_capacity_parts(total_bytes);

    if used.unit == total.unit && used.unit != "B" {
        format!(
            "{} / {} {}",
            format_capacity_number(used.value),
            format_capacity_number(total.value),
            used.unit
        )
    } else {
        format!(
            "{} / {}",
            format_memory_capacity(used_bytes),
            format_memory_capacity(total_bytes)
        )
    }
}

pub(super) fn format_memory_capacity(bytes: u64) -> String {
    let capacity = memory_capacity_parts(bytes);
    if capacity.unit == "B" {
        format!("{} B", bytes)
    } else {
        format!(
            "{} {}",
            format_capacity_number(capacity.value),
            capacity.unit
        )
    }
}

pub(super) fn format_capacity_number(value: f64) -> String {
    format!("{value:.1}")
}

pub(super) fn memory_capacity_parts(bytes: u64) -> MemoryCapacityParts {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    const TIB: f64 = GIB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= TIB {
        MemoryCapacityParts {
            value: bytes / TIB,
            unit: "TB",
        }
    } else if bytes >= GIB {
        MemoryCapacityParts {
            value: bytes / GIB,
            unit: "GB",
        }
    } else if bytes >= MIB {
        MemoryCapacityParts {
            value: bytes / MIB,
            unit: "MB",
        }
    } else if bytes >= KIB {
        MemoryCapacityParts {
            value: bytes / KIB,
            unit: "KB",
        }
    } else {
        MemoryCapacityParts {
            value: bytes,
            unit: "B",
        }
    }
}

pub(super) fn format_bytes_per_second(bytes_per_second: f64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    if bytes_per_second >= GIB {
        format!("{:.1} GB/s", bytes_per_second / GIB)
    } else if bytes_per_second >= MIB {
        format!("{:.1} MB/s", bytes_per_second / MIB)
    } else if bytes_per_second >= KIB {
        format!("{:.1} KB/s", bytes_per_second / KIB)
    } else {
        format!("{bytes_per_second:.0} B/s")
    }
}

pub(super) fn input_hook_required(settings: &Settings) -> bool {
    settings.general.enabled
        && (activity_input_hook_required(settings) || app_suspension_input_hook_required(settings))
}

pub(super) fn input_hook_config(settings: &Settings) -> InputHookConfig {
    let app_suspension = app_suspension_input_hook_required(settings);
    InputHookConfig {
        keyboard: settings.by_activity.input_detection.keyboard || app_suspension,
        mouse: settings.by_activity.input_detection.mouse || app_suspension,
    }
}

pub(super) fn app_suspension_input_hook_required(settings: &Settings) -> bool {
    settings.app_suspension.enabled && !settings.adaptive_engine.enabled
}

pub(super) fn activity_input_hook_required(settings: &Settings) -> bool {
    settings.by_activity.enabled
        && settings.by_activity.switch_to_performance_on_resume
        && settings
            .by_activity
            .input_detection
            .keyboard_or_mouse_enabled()
        && settings.by_activity.power_plans.performance_guid.is_some()
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
