use std::{borrow::Cow, collections::HashMap, sync::LazyLock};

use gpui::{AssetSource, Result, SharedString};
use icondata_core::IconData;

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICON_ASSET_BYTES
            .get(path)
            .map(|asset| Cow::Borrowed(asset.as_slice())))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        if path == "icons" {
            Ok(ICON_ASSETS
                .iter()
                .map(|(path, _)| SharedString::from(*path))
                .collect())
        } else {
            Ok(Vec::new())
        }
    }
}

const ICON_ASSETS: &[(&str, &IconData)] = &[
    ("icons/app-window.svg", icondata_lu::LuAppWindow),
    ("icons/brain-cog.svg", icondata_lu::LuBrainCog),
    ("icons/bring-to-front.svg", icondata_lu::LuBringToFront),
    ("icons/calendar-days.svg", icondata_lu::LuCalendarDays),
    ("icons/chart-column.svg", icondata_lu::LuChartColumn),
    ("icons/chevron-down.svg", icondata_lu::LuChevronDown),
    ("icons/chevron-right.svg", icondata_lu::LuChevronRight),
    (
        "icons/circle-fading-arrow-up.svg",
        icondata_lu::LuCircleFadingArrowUp,
    ),
    ("icons/cog.svg", icondata_lu::LuCog),
    ("icons/cpu.svg", icondata_lu::LuCpu),
    ("icons/drill.svg", icondata_lu::LuDrill),
    ("icons/feather.svg", icondata_lu::LuFeather),
    ("icons/footprints.svg", icondata_lu::LuFootprints),
    ("icons/gpu.svg", icondata_lu::LuGpu),
    ("icons/hourglass.svg", icondata_lu::LuHourglass),
    ("icons/house.svg", icondata_lu::LuHouse),
    ("icons/info.svg", icondata_lu::LuInfo),
    ("icons/leaf.svg", icondata_lu::LuLeaf),
    ("icons/life-buoy.svg", icondata_lu::LuLifeBuoy),
    ("icons/list.svg", icondata_lu::LuList),
    ("icons/memory-stick.svg", icondata_lu::LuMemoryStick),
    ("icons/monitor-pause.svg", icondata_lu::LuMonitorPause),
    ("icons/monitor-x.svg", icondata_lu::LuMonitorX),
    ("icons/octagon-minus.svg", icondata_lu::LuOctagonMinus),
    ("icons/palette.svg", icondata_lu::LuPalette),
    ("icons/panels-top-left.svg", icondata_lu::LuPanelsTopLeft),
    ("icons/rocket.svg", icondata_lu::LuRocket),
    ("icons/rotate-3d.svg", icondata_lu::LuRotate3d),
    ("icons/scan-eye.svg", icondata_lu::LuScanEye),
    ("icons/scissors.svg", icondata_lu::LuScissors),
    ("icons/settings.svg", icondata_lu::LuSettings),
    ("icons/spline.svg", icondata_lu::LuSpline),
    ("icons/square-activity.svg", icondata_lu::LuSquareActivity),
    ("icons/square-pen.svg", icondata_lu::LuSquarePen),
    ("icons/trash-2.svg", icondata_lu::LuTrash2),
    ("icons/trending-up-down.svg", icondata_lu::LuTrendingUpDown),
    ("icons/wrench.svg", icondata_lu::LuWrench),
    ("icons/zap.svg", icondata_lu::LuZap),
];

static ICON_ASSET_BYTES: LazyLock<HashMap<&'static str, Vec<u8>>> = LazyLock::new(|| {
    ICON_ASSETS
        .iter()
        .map(|(path, icon)| (*path, lucide_svg(icon).into_bytes()))
        .collect()
});

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
