use crate::ui::app::*;

pub(in crate::ui::app) fn title_bar_controls(
    window: &Window,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
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

pub(in crate::ui::app) fn title_bar_control_button(
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

pub(in crate::ui::app) fn section_landing_card(
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

pub(in crate::ui::app) fn nav_row(
    page: Page,
    selected: bool,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    let row_id = SharedString::from(format!("nav-row-{page:?}"));
    let hover_id = row_id.to_string();
    let (hovered, _) = card_hover_snapshot(&hover_id);
    let bg_layer = animated_nav_row_bg(&hover_id, selected);
    let content_opacity = if selected || hovered { 1.0 } else { 0.86 };
    let hover_row_id = (!selected).then_some(hover_id);

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

pub(in crate::ui::app) fn animated_nav_row_bg(id: &str, selected: bool) -> AnyElement {
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

pub(in crate::ui::app) fn nav_selection_indicator(page: Page, selected: bool) -> AnyElement {
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

pub(in crate::ui::app) fn nav_icon(
    page: Page,
    selected: bool,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
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

pub(in crate::ui::app) fn nav_icon_name(page: Page) -> NavIcon {
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
pub(in crate::ui::app) enum NavIcon {
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
