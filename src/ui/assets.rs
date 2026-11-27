use std::{cell::RefCell, collections::HashMap, sync::LazyLock};

use iced::advanced::{graphics::geometry::Renderer as _, Renderer as _};
use iced::widget::canvas;
use iced::{mouse, Color, Element, Point, Rectangle, Renderer, Size, Theme, Vector};
use icondata_core::IconData;

pub(super) fn logo() -> iced::widget::image::Handle {
    static HANDLE: LazyLock<iced::widget::image::Handle> = LazyLock::new(|| {
        iced::widget::image::Handle::from_bytes(
            include_bytes!("../../image/icon-design.png").as_slice(),
        )
    });
    HANDLE.clone()
}

const ICON_ASSETS: &[(&str, &IconData)] = &[
    ("icons/app-window.svg", icondata_lu::LuAppWindow),
    ("icons/book-open.svg", icondata_lu::LuBookOpen),
    ("icons/brain-circuit.svg", icondata_lu::LuBrainCircuit),
    ("icons/bring-to-front.svg", icondata_lu::LuBringToFront),
    ("icons/calendar-days.svg", icondata_lu::LuCalendarDays),
    ("icons/chart-column.svg", icondata_lu::LuChartColumn),
    (
        "icons/check.svg",
        &IconData {
            stroke_width: Some("4"),
            view_box: Some("0 -0.5 24 24"),
            ..*icondata_lu::LuCheck
        },
    ),
    ("icons/chevron-down.svg", icondata_lu::LuChevronDown),
    ("icons/chevron-up.svg", icondata_lu::LuChevronUp),
    ("icons/chevron-right.svg", icondata_lu::LuChevronRight),
    (
        "icons/circle-fading-arrow-up.svg",
        icondata_lu::LuCircleFadingArrowUp,
    ),
    ("icons/cog.svg", icondata_lu::LuCog),
    ("icons/coffee.svg", icondata_lu::LuCoffee),
    ("icons/clock.svg", icondata_lu::LuClock),
    ("icons/cpu.svg", icondata_lu::LuCpu),
    ("icons/drill.svg", icondata_lu::LuDrill),
    ("icons/flask-conical.svg", icondata_lu::LuFlaskConical),
    ("icons/footprints.svg", icondata_lu::LuFootprints),
    ("icons/github.svg", icondata_lu::LuGithub),
    ("icons/gpu.svg", icondata_lu::LuGpu),
    ("icons/hourglass.svg", icondata_lu::LuHourglass),
    ("icons/house.svg", icondata_lu::LuHouse),
    ("icons/square-menu.svg", icondata_lu::LuSquareMenu),
    ("icons/info.svg", icondata_lu::LuInfo),
    ("icons/leaf.svg", icondata_lu::LuLeaf),
    ("icons/minus.svg", icondata_lu::LuMinus),
    ("icons/square.svg", icondata_lu::LuSquare),
    ("icons/copy.svg", icondata_lu::LuCopy),
    ("icons/life-buoy.svg", icondata_lu::LuLifeBuoy),
    ("icons/list.svg", icondata_lu::LuList),
    ("icons/memory-stick.svg", icondata_lu::LuMemoryStick),
    ("icons/monitor-pause.svg", icondata_lu::LuMonitorPause),
    ("icons/monitor-x.svg", icondata_lu::LuMonitorX),
    ("icons/octagon-minus.svg", icondata_lu::LuOctagonMinus),
    ("icons/palette.svg", icondata_lu::LuPalette),
    ("icons/panel-right-open.svg", icondata_lu::LuPanelRightOpen),
    ("icons/panels-top-left.svg", icondata_lu::LuPanelsTopLeft),
    ("icons/play.svg", icondata_lu::LuPlay),
    ("icons/pause.svg", icondata_lu::LuPause),
    ("icons/ban.svg", icondata_lu::LuBan),
    ("icons/shield.svg", icondata_lu::LuShield),
    ("icons/circle-help.svg", icondata_lu::LuCircleHelp),
    ("icons/pencil.svg", icondata_lu::LuPencil),
    (
        "icons/plus.svg",
        &IconData {
            // Keep both strokes in one fill so dimmed icons have uniform opacity.
            data: r#"<path d="M5 12h14 M12 5v14" />"#,
            ..*icondata_lu::LuPlus
        },
    ),
    ("icons/rotate-ccw.svg", icondata_lu::LuRotateCcw),
    ("icons/rotate-3d.svg", icondata_lu::LuRotate3d),
    ("icons/scale.svg", icondata_lu::LuScale),
    ("icons/scissors.svg", icondata_lu::LuScissors),
    ("icons/search.svg", icondata_lu::LuSearch),
    ("icons/settings.svg", icondata_lu::LuSettings),
    ("icons/spline.svg", icondata_lu::LuSpline),
    ("icons/square-activity.svg", icondata_lu::LuSquareActivity),
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

const DISCORD: &str = "icons/discord.svg";
static ICONS: LazyLock<HashMap<&'static str, VectorIcon>> = LazyLock::new(|| {
    ICON_ASSETS
        .iter()
        .map(|(path, icon)| (*path, VectorIcon::parse(&lucide_svg(icon))))
        .chain([(
            DISCORD,
            VectorIcon::parse(include_str!("../../image/discord.svg")),
        )])
        .collect()
});

struct VectorIcon {
    viewport: Size,
    shapes: Vec<Shape>,
}

struct Shape {
    path: canvas::Path,
    even_odd: bool,
    opacity: f32,
}

impl VectorIcon {
    fn parse(svg: &str) -> Self {
        let tree = usvg::Tree::from_str(svg, &usvg::Options::default())
            .expect("Bundled icons must be valid SVG");
        let mut shapes = Vec::new();
        collect_shapes(tree.root(), &mut shapes);
        Self {
            viewport: Size::new(tree.size().width(), tree.size().height()),
            shapes,
        }
    }
}

fn collect_shapes(group: &usvg::Group, shapes: &mut Vec<Shape>) {
    assert!(group.clip_path().is_none() && group.mask().is_none() && group.filters().is_empty());
    assert_eq!(
        group.opacity().get(),
        1.0,
        "Bundled icon groups must be opaque"
    );
    for child in group.children() {
        match child {
            usvg::Node::Group(group) => collect_shapes(group, shapes),
            usvg::Node::Path(path) if path.is_visible() => {
                let transform = path.abs_transform();
                if let Some(fill) = path.fill() {
                    shapes.push(shape(
                        path.data().clone(),
                        transform,
                        fill.rule() == usvg::FillRule::EvenOdd,
                        fill.opacity().get(),
                    ));
                }
                if let Some(stroke) = path.stroke() {
                    if let Some(outline) = path.data().stroke(&stroke.to_tiny_skia(), 1.0) {
                        shapes.push(shape(outline, transform, false, stroke.opacity().get()));
                    }
                }
            }
            usvg::Node::Path(_) => {}
            _ => panic!("Bundled icons must contain vector paths only"),
        }
    }
}

fn shape(
    path: usvg::tiny_skia_path::Path,
    transform: usvg::Transform,
    even_odd: bool,
    opacity: f32,
) -> Shape {
    use usvg::tiny_skia_path::PathSegment;
    let path = path
        .transform(transform)
        .expect("Bundled icon transform is valid");
    let point = |p: usvg::tiny_skia_path::Point| Point::new(p.x, p.y);
    let path = canvas::Path::new(|builder| {
        for segment in path.segments() {
            match segment {
                PathSegment::MoveTo(p) => builder.move_to(point(p)),
                PathSegment::LineTo(p) => builder.line_to(point(p)),
                PathSegment::QuadTo(a, b) => builder.quadratic_curve_to(point(a), point(b)),
                PathSegment::CubicTo(a, b, c) => {
                    builder.bezier_curve_to(point(a), point(b), point(c))
                }
                PathSegment::Close => builder.close(),
            }
        }
    });
    Shape {
        path,
        even_odd,
        opacity,
    }
}

struct IconProgram<F> {
    path: &'static str,
    color: F,
}

impl<M, F: Fn(&Theme) -> Color> canvas::Program<M> for IconProgram<F> {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        vec![icon_geometry(
            renderer,
            self.path,
            bounds.size(),
            (self.color)(theme),
            0.0,
        )]
    }
}

pub(super) fn icon<'a, M: 'a>(
    path: &'static str,
    size: u32,
    color: impl Fn(&Theme) -> Color + 'a,
) -> Element<'a, M> {
    canvas::Canvas::new(IconProgram { path, color })
        .width(size)
        .height(size)
        .into()
}

#[cfg(test)]
pub(super) fn has_icon(path: &str) -> bool {
    ICONS.contains_key(path)
}

pub(super) fn draw_icon(
    renderer: &mut Renderer,
    path: &'static str,
    bounds: Rectangle,
    color: Color,
    quarter_turns: f32,
) {
    let geometry = icon_geometry(renderer, path, bounds.size(), color, quarter_turns);
    renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
        renderer.draw_geometry(geometry)
    });
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct IconFrame {
    path: &'static str,
    size: [u32; 2],
    color: [u32; 4],
    rotation: u8,
}

thread_local! {
    static FRAMES: RefCell<HashMap<IconFrame, canvas::Cache>> = RefCell::default();
}

fn icon_geometry(
    renderer: &Renderer,
    path: &'static str,
    size: Size,
    color: Color,
    quarter_turns: f32,
) -> canvas::Geometry {
    use iced::advanced::graphics::geometry::fill::Rule;
    let rotation = (quarter_turns.clamp(0.0, 3.0) * 18.0).round() as u8;
    let key = IconFrame {
        path,
        size: [size.width.to_bits(), size.height.to_bits()],
        color: [color.r, color.g, color.b, color.a].map(f32::to_bits),
        rotation,
    };
    FRAMES.with_borrow_mut(|frames| {
        // Theme color previews can produce arbitrarily many tints.
        if frames.len() >= 512 && !frames.contains_key(&key) {
            frames.clear();
        }
        frames
            .entry(key)
            .or_default()
            .draw(renderer, size, |frame| {
                let icon = ICONS.get(path).expect("Every UI icon is bundled");
                let scale =
                    (size.width / icon.viewport.width).min(size.height / icon.viewport.height);
                // Draw paths at the actual layout origin. SVG pixmap positions truncate fractional pixels.
                frame.translate(Vector::new(size.width / 2.0, size.height / 2.0));
                frame.rotate(iced::Radians((f32::from(rotation) * 5.0).to_radians()));
                frame.scale(scale);
                frame.translate(Vector::new(
                    -icon.viewport.width / 2.0,
                    -icon.viewport.height / 2.0,
                ));
                for shape in &icon.shapes {
                    frame.fill(
                        &shape.path,
                        canvas::Fill {
                            style: color.scale_alpha(shape.opacity).into(),
                            rule: if shape.even_odd {
                                Rule::EvenOdd
                            } else {
                                Rule::NonZero
                            },
                        },
                    );
                }
            })
    })
}

#[cfg(test)]
mod centering_tests;

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn icon_asset_paths_are_unique() {
        let mut paths = HashSet::new();
        for path in ICON_ASSETS.iter().map(|(path, _)| *path).chain([DISCORD]) {
            assert!(paths.insert(path), "duplicate icon asset path: {path}");
            assert!(!ICONS[path].shapes.is_empty(), "empty icon: {path}");
        }
    }
}
