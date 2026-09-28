use std::{collections::HashMap, sync::LazyLock};

use icondata_core::IconData;

const ICON_ASSETS: &[(&str, &IconData)] = &[
    ("icons/app-window.svg", icondata_lu::LuAppWindow),
    ("icons/brain-circuit.svg", icondata_lu::LuBrainCircuit),
    ("icons/bring-to-front.svg", icondata_lu::LuBringToFront),
    ("icons/calendar-days.svg", icondata_lu::LuCalendarDays),
    ("icons/chart-column.svg", icondata_lu::LuChartColumn),
    ("icons/circle-pause.svg", icondata_lu::LuCirclePause),
    ("icons/check.svg", icondata_lu::LuCheck),
    ("icons/chevron-down.svg", icondata_lu::LuChevronDown),
    ("icons/chevron-up.svg", icondata_lu::LuChevronUp),
    ("icons/chevron-right.svg", icondata_lu::LuChevronRight),
    (
        "icons/circle-fading-arrow-up.svg",
        icondata_lu::LuCircleFadingArrowUp,
    ),
    ("icons/cog.svg", icondata_lu::LuCog),
    ("icons/computer.svg", icondata_lu::LuComputer),
    ("icons/cpu.svg", icondata_lu::LuCpu),
    ("icons/drill.svg", icondata_lu::LuDrill),
    ("icons/flask-conical.svg", icondata_lu::LuFlaskConical),
    ("icons/footprints.svg", icondata_lu::LuFootprints),
    ("icons/gpu.svg", icondata_lu::LuGpu),
    ("icons/hourglass.svg", icondata_lu::LuHourglass),
    ("icons/house.svg", icondata_lu::LuHouse),
    ("icons/square-menu.svg", icondata_lu::LuSquareMenu),
    ("icons/info.svg", icondata_lu::LuInfo),
    ("icons/leaf.svg", icondata_lu::LuLeaf),
    ("icons/minus.svg", icondata_lu::LuMinus),
    ("icons/life-buoy.svg", icondata_lu::LuLifeBuoy),
    ("icons/list.svg", icondata_lu::LuList),
    ("icons/memory-stick.svg", icondata_lu::LuMemoryStick),
    ("icons/monitor-pause.svg", icondata_lu::LuMonitorPause),
    ("icons/monitor-x.svg", icondata_lu::LuMonitorX),
    ("icons/octagon-minus.svg", icondata_lu::LuOctagonMinus),
    ("icons/palette.svg", icondata_lu::LuPalette),
    ("icons/panel-left-close.svg", icondata_lu::LuPanelLeftClose),
    ("icons/panel-left-open.svg", icondata_lu::LuPanelLeftOpen),
    (
        "icons/panel-right-close.svg",
        icondata_lu::LuPanelRightClose,
    ),
    ("icons/panel-right-open.svg", icondata_lu::LuPanelRightOpen),
    ("icons/panels-top-left.svg", icondata_lu::LuPanelsTopLeft),
    ("icons/play.svg", icondata_lu::LuPlay),
    ("icons/pause.svg", icondata_lu::LuPause),
    ("icons/ban.svg", icondata_lu::LuBan),
    ("icons/shield.svg", icondata_lu::LuShield),
    ("icons/circle-help.svg", icondata_lu::LuCircleHelp),
    ("icons/pencil.svg", icondata_lu::LuPencil),
    ("icons/plus.svg", icondata_lu::LuPlus),
    ("icons/refresh-cw.svg", icondata_lu::LuRefreshCw),
    ("icons/rocket.svg", icondata_lu::LuRocket),
    ("icons/rotate-3d.svg", icondata_lu::LuRotate3d),
    ("icons/scissors.svg", icondata_lu::LuScissors),
    ("icons/search.svg", icondata_lu::LuSearch),
    ("icons/settings.svg", icondata_lu::LuSettings),
    ("icons/snowflake.svg", icondata_lu::LuSnowflake),
    ("icons/spline.svg", icondata_lu::LuSpline),
    ("icons/square-activity.svg", icondata_lu::LuSquareActivity),
    ("icons/square-pen.svg", icondata_lu::LuSquarePen),
    ("icons/trash-2.svg", icondata_lu::LuTrash2),
    ("icons/trending-up-down.svg", icondata_lu::LuTrendingUpDown),
    ("icons/wrench.svg", icondata_lu::LuWrench),
    ("icons/x.svg", icondata_lu::LuX),
    ("icons/zap.svg", icondata_lu::LuZap),
];

fn lucide_svg(icon: &IconData) -> String {
    let mut svg = String::from(r#"<svg xmlns="http://www.w3.org/2000/svg""#);
    push_attr(&mut svg, "style", icon.style);
    push_attr(&mut svg, "x", icon.x);
    push_attr(&mut svg, "y", icon.y);
    push_attr(&mut svg, "width", icon.width);
    push_attr(&mut svg, "height", icon.height);
    push_attr(&mut svg, "viewBox", icon.view_box);
    push_attr(&mut svg, "fill", icon.fill);
    push_attr(&mut svg, "stroke", icon.stroke);
    push_attr(&mut svg, "stroke-width", icon.stroke_width);
    push_attr(&mut svg, "stroke-linecap", icon.stroke_linecap);
    push_attr(&mut svg, "stroke-linejoin", icon.stroke_linejoin);
    svg.push('>');
    svg.push_str(icon.data);
    svg.push_str("</svg>");
    svg
}

fn push_attr(svg: &mut String, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        svg.push(' ');
        svg.push_str(name);
        svg.push_str(r#"=""#);
        svg.push_str(value);
        svg.push('"');
    }
}
pub(crate) fn iced_icon(path: &str) -> Option<iced::widget::svg::Handle> {
    static HANDLES: LazyLock<HashMap<&'static str, iced::widget::svg::Handle>> =
        LazyLock::new(|| {
            ICON_ASSETS
                .iter()
                .map(|(path, icon)| {
                    (
                        *path,
                        iced::widget::svg::Handle::from_memory(lucide_svg(icon).into_bytes()),
                    )
                })
                .collect()
        });
    HANDLES.get(path).cloned()
}

// SVG-local rotation works with the unmodified software renderer.
pub(super) fn chevron_frame(progress: f32) -> iced::widget::svg::Handle {
    static FRAMES: LazyLock<[iced::widget::svg::Handle; 19]> = LazyLock::new(|| {
        std::array::from_fn(|index| {
            let icon = icondata_lu::LuChevronRight;
            let svg = lucide_svg(icon).replace(
                icon.data,
                &format!(
                    "<g transform=\"rotate({} 12 12)\">{}</g>",
                    index * 5,
                    icon.data
                ),
            );
            iced::widget::svg::Handle::from_memory(svg.into_bytes())
        })
    });
    FRAMES[(progress.clamp(0.0, 1.0) * 18.0).round() as usize].clone()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn icon_asset_paths_are_unique() {
        let mut paths = HashSet::new();
        for (path, _) in ICON_ASSETS {
            assert!(paths.insert(*path), "duplicate icon asset path: {path}");
            let first = iced_icon(path).unwrap();
            assert_eq!(first.id(), iced_icon(path).unwrap().id());
        }
    }
}
