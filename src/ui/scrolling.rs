//! Shared software-rendered scrolling, surfaces, and virtual-list buffering.

use iced::advanced::{
    layout, renderer,
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};
use iced::{mouse, overlay, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};
use std::cell::Cell;

pub(super) fn buffered<'a, Message: 'a>(
    content: Element<'a, Message>,
    offset: &'a Cell<f32>,
    start: f32,
    end: f32,
    first: bool,
    last: bool,
    scroll_offset: fn(&Message) -> Option<f32>,
) -> Element<'a, Message> {
    Element::new(ScrollContent {
        content,
        behavior: Behavior::Buffer(Buffer {
            offset,
            start,
            end,
            first,
            last,
            scroll_offset,
        }),
    })
}
struct Buffer<'a, Message> {
    offset: &'a Cell<f32>,
    start: f32,
    end: f32,
    first: bool,
    last: bool,
    scroll_offset: fn(&Message) -> Option<f32>,
}
fn needs_rows(offset: f32, height: f32, start: f32, end: f32, first: bool, last: bool) -> bool {
    (!first && offset < start + 36.0) || (!last && offset + height > end - 36.0)
}
pub(super) fn table_surface<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    iced::widget::Stack::new()
        .push(content)
        .push_under(
            iced::widget::Canvas::new(TableSurface::Background)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .into()
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum TableSurface {
    Background,
    Outline,
    Backdrop,
}
#[derive(Default)]
struct SurfaceState {
    cache: iced::widget::canvas::Cache,
    style: Cell<Option<(TableSurface, iced::Color)>>,
}
impl<Message> iced::widget::canvas::Program<Message> for TableSurface {
    type State = SurfaceState;

    fn draw(
        &self,
        state: &SurfaceState,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Path, Stroke};
        let color = match self {
            Self::Background => theme.extended_palette().background.weak.color,
            Self::Outline => super::widgets::surface(theme).border.color,
            Self::Backdrop => iced::Color::from_rgba(0.0, 0.0, 0.0, 0.45),
        };
        let style = (*self, color);
        if state.style.replace(Some(style)) != Some(style) {
            state.cache.clear();
        }
        // Retain geometry across redraws, including antialias coverage at fractional bounds.
        vec![state.cache.draw_with_bounds(
            renderer,
            Rectangle::with_size(bounds.size()).expand(1.0),
            |frame| {
                if matches!(self, Self::Outline) {
                    if bounds.width > 1.0 && bounds.height > 1.0 {
                        // Stroke only the outline; a transparent quad still paints every pixel.
                        let path = Path::rounded_rectangle(
                            iced::Point::new(0.5, 0.5),
                            Size::new(bounds.width - 1.0, bounds.height - 1.0),
                            (super::design::CARD_RADIUS - 0.5).into(),
                        );
                        frame.stroke(&path, Stroke::default().with_width(1.0).with_color(color));
                    }
                    return;
                }
                let radius = super::design::CARD_RADIUS;
                // Small paths let the software renderer skip masking the untouched
                // portion of a large table background during partial repaints.
                // Overlap internal edges to avoid antialiasing seams at fractional DPI.
                let columns = ((bounds.width / 64.0) as usize).max(1);
                let rows = ((bounds.height / 64.0) as usize).max(1);
                for y in 0..rows {
                    for x in 0..columns {
                        let tile = Rectangle {
                            x: x as f32 * 64.0,
                            y: y as f32 * 64.0,
                            width: if x + 1 == columns {
                                bounds.width - x as f32 * 64.0
                            } else {
                                65.0
                            },
                            height: if y + 1 == rows {
                                bounds.height - y as f32 * 64.0
                            } else {
                                65.0
                            },
                        };
                        if matches!(self, Self::Backdrop) {
                            // Rectangle fills avoid antialias seams and double alpha at tile edges.
                            frame.fill_rectangle(
                                tile.position(),
                                Size::new(
                                    if x + 1 == columns { tile.width } else { 64.0 },
                                    if y + 1 == rows { tile.height } else { 64.0 },
                                ),
                                color,
                            );
                            continue;
                        }
                        let corners = iced::border::Radius {
                            top_left: if x == 0 && y == 0 { radius } else { 0.0 },
                            top_right: if x + 1 == columns && y == 0 {
                                radius
                            } else {
                                0.0
                            },
                            bottom_left: if x == 0 && y + 1 == rows { radius } else { 0.0 },
                            bottom_right: if x + 1 == columns && y + 1 == rows {
                                radius
                            } else {
                                0.0
                            },
                        };
                        frame.fill(
                            &Path::rounded_rectangle(tile.position(), tile.size(), corners),
                            color,
                        );
                    }
                }
            },
        )]
    }
}
pub(super) fn table_frame<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    iced::widget::Stack::new()
        .push(
            iced::widget::container(content)
                .padding(1)
                .width(Length::Fill)
                .height(Length::Fill)
                .clip(true),
        )
        .push_under(
            iced::widget::Canvas::new(TableSurface::Outline)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .into()
}

pub(super) fn modal_backdrop<'a, Message: 'a>() -> Element<'a, Message> {
    iced::widget::Canvas::new(TableSurface::Backdrop)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
pub(super) fn repaint_group<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    Element::new(ScrollContent {
        content: content.into(),
        behavior: Behavior::Group,
    })
}

pub(super) fn repaint_popup<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    Element::new(ScrollContent {
        content: content.into(),
        behavior: Behavior::Popup,
    })
}

enum Behavior<'a, Message> {
    Scroll,
    Table(f32),
    FillWidth,
    Group,
    Popup,
    Buffer(Buffer<'a, Message>),
}

pub(super) const SCROLLBAR_GUTTER: f32 =
    (super::design::space::SMALL + super::design::space::CONTROL) as f32;

/// Keep content geometry stable even while the scrollbar is hidden.
pub(super) fn vertical_gutter<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> iced::widget::Container<'a, Message> {
    iced::widget::container(content).padding(iced::Padding {
        right: SCROLLBAR_GUTTER,
        ..Default::default()
    })
}

/// The reserved content padding owns the gutter, independently of overflow.
pub(super) fn scrollbar() -> iced::widget::scrollable::Scrollbar {
    iced::widget::scrollable::Scrollbar::new()
        .width(super::design::space::SMALL as f32)
        .scroller_width(super::design::space::TIGHT as f32)
}

fn scroll_style(
    theme: &Theme,
    status: iced::widget::scrollable::Status,
) -> iced::widget::scrollable::Style {
    use iced::widget::scrollable::{Rail, Scroller, Status};
    let (horizontal, vertical) = match status {
        Status::Active {
            is_horizontal_scrollbar_disabled,
            is_vertical_scrollbar_disabled,
        } => (
            (is_horizontal_scrollbar_disabled, false, false),
            (is_vertical_scrollbar_disabled, false, false),
        ),
        Status::Hovered {
            is_horizontal_scrollbar_disabled,
            is_vertical_scrollbar_disabled,
            is_horizontal_scrollbar_hovered,
            is_vertical_scrollbar_hovered,
        } => (
            (
                is_horizontal_scrollbar_disabled,
                is_horizontal_scrollbar_hovered,
                false,
            ),
            (
                is_vertical_scrollbar_disabled,
                is_vertical_scrollbar_hovered,
                false,
            ),
        ),
        Status::Dragged {
            is_horizontal_scrollbar_disabled,
            is_vertical_scrollbar_disabled,
            is_horizontal_scrollbar_dragged,
            is_vertical_scrollbar_dragged,
        } => (
            (
                is_horizontal_scrollbar_disabled,
                false,
                is_horizontal_scrollbar_dragged,
            ),
            (
                is_vertical_scrollbar_disabled,
                false,
                is_vertical_scrollbar_dragged,
            ),
        ),
    };
    let rail = |(disabled, hovered, dragged)| Rail {
        background: None,
        border: iced::Border::default(),
        scroller: Scroller {
            background: theme
                .palette()
                .text
                .scale_alpha(if disabled {
                    0.0
                } else if dragged {
                    0.8
                } else if hovered {
                    0.6
                } else {
                    0.4
                })
                .into(),
            border: iced::border::rounded(super::design::space::TINY),
        },
    };
    iced::widget::scrollable::Style {
        horizontal_rail: rail(horizontal),
        vertical_rail: rail(vertical),
        gap: None,
        ..iced::widget::scrollable::default(theme, status)
    }
}

/// Retain native input, scrollbars, IDs and overlays; share the repaint policy.
pub(super) fn scrollable<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> iced::widget::Scrollable<'a, Message> {
    iced::widget::scrollable(Element::new(ScrollContent {
        content: vertical_gutter(content).into(),
        behavior: Behavior::Scroll,
    }))
    .direction(iced::widget::scrollable::Direction::Vertical(scrollbar()))
    .style(scroll_style)
}

/// Popups reserve scrollbar space only when their own content overflows.
pub(super) fn popup_scrollable<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> iced::widget::Scrollable<'a, Message> {
    iced::widget::scrollable(content)
        .direction(iced::widget::scrollable::Direction::Vertical(
            scrollbar().spacing(super::design::space::CONTROL as f32),
        ))
        .style(scroll_style)
}

/// The compact navigation rail keeps its scrollbar inside the outer inset.
pub(super) fn navigation_scrollable<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> iced::widget::Scrollable<'a, Message> {
    iced::widget::scrollable(Element::new(ScrollContent {
        content: iced::widget::container(content)
            .padding([0, super::design::space::CONTROL as u16])
            .into(),
        behavior: Behavior::Scroll,
    }))
    .direction(iced::widget::scrollable::Direction::Vertical(
        scrollbar().width(3).scroller_width(3).margin(1),
    ))
    .style(scroll_style)
}

/// Keep short tab panes in one clipping layer as their content changes height.
pub(super) fn scrollable_pane<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> iced::widget::Scrollable<'a, Message> {
    iced::widget::scrollable(vertical_gutter(repaint_group(content)))
        .direction(iced::widget::scrollable::Direction::Vertical(scrollbar()))
        .style(scroll_style)
}

pub(super) fn horizontal_scrollable<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> iced::widget::Scrollable<'a, Message> {
    iced::widget::scrollable(Element::new(ScrollContent {
        content: iced::widget::container(content)
            .padding(iced::Padding {
                bottom: SCROLLBAR_GUTTER,
                ..Default::default()
            })
            .into(),
        behavior: Behavior::Scroll,
    }))
    .direction(iced::widget::scrollable::Direction::Horizontal(scrollbar()))
    .style(scroll_style)
}

/// Keep dense columns readable when the dock reduces the editing area.
pub(super) fn horizontal_table<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    min_width: f32,
) -> Element<'a, Message> {
    let table = iced::widget::scrollable(Element::new(ScrollContent {
        content: iced::widget::container(content)
            .padding(iced::Padding {
                bottom: SCROLLBAR_GUTTER,
                ..Default::default()
            })
            .into(),
        behavior: Behavior::Table(min_width),
    }))
    .direction(iced::widget::scrollable::Direction::Horizontal(scrollbar()))
    .style(scroll_style)
    .width(Length::Fill);
    Element::new(ScrollContent {
        content: table.into(),
        behavior: Behavior::FillWidth,
    })
}

struct ScrollContent<'a, Message> {
    content: Element<'a, Message>,
    behavior: Behavior<'a, Message>,
}
impl<Message> Widget<Message, Theme, Renderer> for ScrollContent<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = match self.behavior {
            Behavior::Table(min_width) => limits.width(min_width.max(limits.min().width)),
            // Carry the viewport width through the scrollable's unbounded horizontal limits.
            Behavior::FillWidth => limits.min_width(limits.max().width),
            _ => *limits,
        };
        self.content.as_widget_mut().layout(tree, renderer, &limits)
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if matches!(
            self.behavior,
            Behavior::Group | Behavior::Popup | Behavior::FillWidth
        ) {
            use iced::advanced::Renderer as _;
            // Overlay hosts often pass the exact surface bounds, excluding edge coverage.
            let viewport = if matches!(self.behavior, Behavior::Popup) {
                viewport.expand(1.0)
            } else {
                *viewport
            };
            // Leave room for antialias coverage along card borders.
            if let Some(clip) = layout.bounds().expand(1.0).intersection(&viewport) {
                renderer.with_layer(clip, |renderer| {
                    self.content
                        .as_widget()
                        .draw(tree, renderer, theme, style, layout, cursor, &clip);
                });
            }
            return;
        }
        if matches!(self.behavior, Behavior::Scroll) {
            use iced::advanced::Renderer as _;
            let content_bounds = layout.bounds();
            if let Some(visible) = content_bounds.intersection(viewport) {
                if content_bounds.width <= visible.width && content_bounds.height <= visible.height
                {
                    self.content
                        .as_widget()
                        .draw(tree, renderer, theme, style, layout, cursor, &visible);
                    return;
                }

                // Content-anchored sections move with scrolling, so damage is
                // tracked per section rather than independently for every glyph.
                let width = if content_bounds.width > visible.width {
                    (visible.width / 2.0).clamp(1.0, 512.0)
                } else {
                    content_bounds.width.max(1.0)
                };
                let height = if content_bounds.height > visible.height {
                    (visible.height / 2.0).clamp(1.0, 512.0)
                } else {
                    content_bounds.height.max(1.0)
                };
                let left = ((visible.x - content_bounds.x) / width).floor() as usize;
                let right =
                    ((visible.x + visible.width - content_bounds.x) / width).ceil() as usize;
                let top = ((visible.y - content_bounds.y) / height).floor() as usize;
                let bottom =
                    ((visible.y + visible.height - content_bounds.y) / height).ceil() as usize;
                for y in top..bottom {
                    for x in left..right {
                        let section = Rectangle {
                            x: content_bounds.x + x as f32 * width,
                            y: content_bounds.y + y as f32 * height,
                            width,
                            height,
                        };
                        if let Some(clip) = section.intersection(&visible) {
                            renderer.with_layer(clip, |renderer| {
                                self.content
                                    .as_widget()
                                    .draw(tree, renderer, theme, style, layout, cursor, &clip);
                            });
                        }
                    }
                }
            }
        } else {
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let Behavior::Buffer(buffer) = &self.behavior else {
            self.content.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
            return;
        };
        let mut messages = Vec::new();
        {
            let mut local = Shell::new(&mut messages);
            self.content.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, &mut local, viewport,
            );
            // Preserve every native widget effect; only buffered scroll messages stay local.
            if local.is_event_captured() {
                shell.capture_event();
            }
            shell.request_redraw_at(local.redraw_request());
            shell.request_input_method(local.input_method());
            if local.is_layout_invalid() {
                shell.invalidate_layout();
            }
            if local.are_widgets_invalid() {
                shell.invalidate_widgets();
            }
        }
        for message in messages {
            if let Some(offset) = (buffer.scroll_offset)(&message) {
                buffer.offset.set(offset);
                if !needs_rows(
                    offset,
                    layout.bounds().height,
                    buffer.start,
                    buffer.end,
                    buffer.first,
                    buffer.last,
                ) {
                    continue;
                }
            }
            shell.publish(message);
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

#[cfg(test)]
pub(super) fn check_scroll_damage<M>(view: Element<'_, M>, bounds: Rectangle) {
    check_scroll_damage_with_delta(
        view,
        bounds,
        iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -12.0 },
    );
}

#[cfg(test)]
fn check_scroll_damage_with_delta<M>(
    mut view: Element<'_, M>,
    bounds: Rectangle,
    delta: iced::mouse::ScrollDelta,
) {
    use iced::advanced::{layout, renderer::Renderer as _, widget::Tree};
    let mut renderer = iced::Renderer::new(super::design::typography::FONT, iced::Pixels(14.0));
    let mut tree = Tree::new(&view);
    let node = view.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(iced::Size::ZERO, bounds.size()),
    );
    let mut previous = Vec::new();
    let physical_size = iced::Size::new(bounds.width as u32, bounds.height as u32);
    let mut pixels = tiny_skia::Pixmap::new(physical_size.width, physical_size.height).unwrap();
    let mut mask = tiny_skia::Mask::new(physical_size.width, physical_size.height).unwrap();
    let viewport = iced::advanced::graphics::Viewport::with_physical_size(physical_size, 1.0);
    let mut samples = Vec::new();
    for frame in 0..7 {
        let mut messages = Vec::new();
        view.as_widget_mut().update(
            &mut tree,
            &iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }),
            iced::advanced::Layout::new(&node),
            iced::mouse::Cursor::Available(bounds.center()),
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut iced::advanced::Shell::new(&mut messages),
            &bounds,
        );
        renderer.reset(bounds);
        view.as_widget().draw(
            &tree,
            &mut renderer,
            &iced::Theme::Dark,
            &iced::advanced::renderer::Style::default(),
            iced::advanced::Layout::new(&node),
            iced::mouse::Cursor::Unavailable,
            &bounds,
        );
        let damage = if frame == 0 {
            vec![bounds]
        } else {
            iced::advanced::graphics::damage::group(
                iced::advanced::graphics::damage::diff(
                    &previous,
                    renderer.layers(),
                    |layer| vec![layer.bounds],
                    iced_tiny_skia::Layer::damage,
                ),
                bounds,
            )
        };
        assert!(
            damage.len() <= 2,
            "scrolling fragmented into {} repaint regions",
            damage.len()
        );
        previous = renderer.layers().to_vec();
        let start = std::time::Instant::now();
        renderer.draw(
            &mut pixels.as_mut(),
            &mut mask,
            &viewport,
            &damage,
            iced::Color::BLACK,
        );
        samples.push(start.elapsed());
        println!(
            "scroll frame {frame}: {} regions, {:?}, {} messages",
            damage.len(),
            start.elapsed(),
            messages.len()
        );
    }
    samples.sort();
    println!("median scroll raster: {:?}", samples[3]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_scrollbars_reserve_space_only_for_overflow_changes() {
        use iced::advanced::{layout, widget::Tree};
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = Size::new(240.0, 120.0);
        let mut tree = Tree::empty();
        for overflowing in [false, true, false, true] {
            let mut view: Element<'_, ()> = popup_scrollable(
                iced::widget::Space::new()
                    .width(Length::Fill)
                    .height(if overflowing { 480.0 } else { 80.0 }),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
            tree.diff(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            assert_eq!(node.size(), size);
            assert_eq!(
                size.width - node.children()[0].size().width,
                if overflowing { SCROLLBAR_GUTTER } else { 0.0 },
            );
        }
    }

    #[test]
    fn scrollbar_visibility_does_not_change_content_geometry() {
        use iced::advanced::{layout, widget::Tree};
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = Size::new(240.0, 120.0);
        for horizontal in [false, true] {
            let mut tree = Tree::empty();
            for overflowing in [false, true, false, true] {
                let length = if overflowing { 480.0 } else { 80.0 };
                let (content, direction) = if horizontal {
                    (
                        iced::widget::Space::new()
                            .width(length)
                            .height(Length::Fill),
                        iced::widget::scrollable::Direction::Horizontal(scrollbar()),
                    )
                } else {
                    (
                        iced::widget::Space::new()
                            .width(Length::Fill)
                            .height(length),
                        iced::widget::scrollable::Direction::Vertical(scrollbar()),
                    )
                };
                let mut view: Element<'_, ()> = if horizontal {
                    horizontal_scrollable(content)
                } else {
                    scrollable(content)
                }
                .direction(direction)
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
                tree.diff(&view);
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                let content = node.children()[0].children()[0].bounds();
                let gutter = SCROLLBAR_GUTTER;
                assert_eq!(node.size(), size);
                if horizontal {
                    assert_eq!(content.height, size.height - gutter);
                } else {
                    assert_eq!(content.width, size.width - gutter);
                }
            }
        }
    }
    #[test]
    fn table_surfaces_cache_geometry_and_track_theme_and_size() {
        use iced::advanced::renderer::{Headless as _, Renderer as _};
        let accent_theme = |accent| {
            Theme::custom_with_fn(
                "accent",
                super::super::design::palette(false, Some(accent)),
                super::super::design::extended_palette,
            )
        };
        let blue = accent_theme(iced::Color::from_rgb8(40, 130, 225));
        let red = accent_theme(iced::Color::from_rgb8(215, 60, 80));
        let amoled = super::super::design::amoled_theme(Some(iced::Color::from_rgb8(215, 60, 80)));
        for outline in [false, true] {
            let mut renderer =
                Renderer::new(super::super::design::typography::FONT, iced::Pixels(14.0));
            let content = iced::widget::Space::new()
                .width(Length::Fill)
                .height(Length::Fill);
            let mut view: Element<'_, ()> = if outline {
                table_frame(content)
            } else {
                table_surface(content)
            };
            let mut tree = Tree::new(&view);
            let mut previous = Vec::new();
            let mut screenshots = Vec::new();
            for (frame, (theme, size)) in [
                (Theme::Light, Size::new(220.0, 120.0)),
                (Theme::Light, Size::new(220.0, 120.0)),
                (Theme::Dark, Size::new(220.0, 120.0)),
                (Theme::Dark, Size::new(220.0, 120.0)),
                (Theme::Dark, Size::new(260.0, 140.0)),
                (Theme::Dark, Size::new(260.0, 140.0)),
                (blue.clone(), Size::new(260.0, 140.0)),
                (blue.clone(), Size::new(260.0, 140.0)),
                (red.clone(), Size::new(260.0, 140.0)),
                (red.clone(), Size::new(260.0, 140.0)),
                (amoled.clone(), Size::new(260.0, 140.0)),
                (amoled.clone(), Size::new(260.0, 140.0)),
            ]
            .into_iter()
            .enumerate()
            {
                let bounds = Rectangle::with_size(size);
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                renderer.reset(bounds);
                view.as_widget().draw(
                    &tree,
                    &mut renderer,
                    &theme,
                    &renderer::Style::default(),
                    Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &bounds,
                );
                let damage = iced::advanced::graphics::damage::diff(
                    &previous,
                    renderer.layers(),
                    |layer| vec![layer.bounds],
                    iced_tiny_skia::Layer::damage,
                );
                // AMOLED reuses the dark outline; only its surface fill changes.
                assert_eq!(
                    damage.is_empty(),
                    frame % 2 == 1 || (outline && frame == 10)
                );
                previous = renderer.layers().to_vec();
                screenshots.push(renderer.screenshot(
                    Size::new(size.width as u32, size.height as u32),
                    1.0,
                    theme.palette().background,
                ));
            }
            assert_eq!(screenshots[0], screenshots[1]);
            assert_ne!(screenshots[1], screenshots[2]);
            assert_eq!(screenshots[2], screenshots[3]);
            assert_eq!(screenshots[4], screenshots[5]);
            assert_ne!(screenshots[5], screenshots[6]);
            assert_eq!(screenshots[6], screenshots[7]);
            assert_ne!(screenshots[7], screenshots[8]);
            assert_eq!(screenshots[8], screenshots[9]);
            assert_ne!(screenshots[9], screenshots[10]);
            assert_eq!(screenshots[10], screenshots[11]);
        }
    }

    #[test]
    fn table_outline_matches_native_border_without_a_full_surface_quad() {
        use iced::advanced::renderer::{Headless as _, Renderer as _};
        for theme in [Theme::Light, Theme::Dark] {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let render = |outline| {
                    let mut renderer =
                        Renderer::new(super::super::design::typography::FONT, iced::Pixels(14.0));
                    let content = iced::widget::Space::new().width(220).height(120);
                    let mut view: Element<'_, ()> = if outline {
                        table_frame(content)
                    } else {
                        iced::widget::container(content)
                            .padding(1)
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .clip(true)
                            .style(|theme| iced::widget::container::Style {
                                background: None,
                                ..super::super::widgets::surface(theme)
                            })
                            .into()
                    };
                    let bounds = Rectangle::with_size(Size::new(222.0, 122.0));
                    let mut tree = Tree::new(&view);
                    let node = view.as_widget_mut().layout(
                        &mut tree,
                        &renderer,
                        &layout::Limits::new(Size::ZERO, bounds.size()),
                    );
                    renderer.reset(bounds);
                    view.as_widget().draw(
                        &tree,
                        &mut renderer,
                        &theme,
                        &renderer::Style::default(),
                        Layout::new(&node),
                        mouse::Cursor::Unavailable,
                        &bounds,
                    );
                    if outline {
                        assert!(renderer.layers().iter().all(|layer| layer.quads.is_empty()));
                    }
                    renderer.screenshot(
                        Size::new((222.0 * scale) as u32, (122.0 * scale) as u32),
                        scale,
                        theme.palette().background,
                    )
                };
                let expected = render(false);
                let actual = render(true);
                let max = actual
                    .iter()
                    .zip(&expected)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                // The two path builders differ slightly in rounded-corner antialiasing.
                assert!(max <= 12, "outline changed at scale {scale}: delta {max}");
            }
        }
    }

    #[test]
    fn tiled_modal_backdrop_preserves_native_pixels() {
        use iced::advanced::renderer::{Headless as _, Renderer as _};
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let render = |tiled| {
                let mut renderer =
                    Renderer::new(super::super::design::typography::FONT, iced::Pixels(14.0));
                let mut view: Element<'_, ()> = if tiled {
                    modal_backdrop()
                } else {
                    iced::widget::container(iced::widget::Space::new())
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .style(|_| iced::widget::container::Style {
                            background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()),
                            ..Default::default()
                        })
                        .into()
                };
                let bounds = Rectangle::with_size(Size::new(222.0, 170.0));
                let mut tree = Tree::new(&view);
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, bounds.size()),
                );
                renderer.reset(bounds);
                view.as_widget().draw(
                    &tree,
                    &mut renderer,
                    &Theme::Light,
                    &renderer::Style::default(),
                    Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &bounds,
                );
                renderer.screenshot(
                    Size::new((222.0 * scale) as u32, (170.0 * scale) as u32),
                    scale,
                    iced::Color::WHITE,
                )
            };
            assert_eq!(
                render(true),
                render(false),
                "backdrop seams at scale {scale}"
            );
        }
    }

    #[test]
    fn shared_scroll_preserves_native_pixels_after_scrolling() {
        use iced::advanced::renderer::{Headless as _, Renderer as _};
        let render = |shared, scale, theme: &Theme| {
            let mut renderer =
                Renderer::new(super::super::design::typography::FONT, iced::Pixels(14.0));
            let items = iced::widget::column((0..30).map(|index| {
                super::super::widgets::settings_card(
                    iced::widget::row![
                        super::super::navigation::glyph("icons/info.svg"),
                        iced::widget::text(format!("Setting {index}: configured value")),
                        iced::widget::Space::new().width(Length::Fill),
                        super::super::widgets::action_button("Edit").on_press(())
                    ]
                    .spacing(12),
                )
                .into()
            }))
            .spacing(8);
            let mut view: Element<'_, ()> = if shared {
                scrollable(items)
            } else {
                iced::widget::scrollable(vertical_gutter(items))
                    .direction(iced::widget::scrollable::Direction::Vertical(scrollbar()))
                    .style(scroll_style)
            }
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
            let bounds = Rectangle::with_size(Size::new(640.0, 360.0));
            let mut tree = Tree::new(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, bounds.size()),
            );
            for _ in 0..3 {
                let mut messages = Vec::new();
                view.as_widget_mut().update(
                    &mut tree,
                    &Event::Mouse(mouse::Event::WheelScrolled {
                        delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -37.0 },
                    }),
                    Layout::new(&node),
                    mouse::Cursor::Available(bounds.center()),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut Shell::new(&mut messages),
                    &bounds,
                );
            }
            renderer.reset(bounds);
            view.as_widget().draw(
                &tree,
                &mut renderer,
                theme,
                &renderer::Style {
                    text_color: theme.palette().text,
                },
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &bounds,
            );
            renderer.screenshot(
                Size::new((640.0 * scale) as u32, (360.0 * scale) as u32),
                scale,
                theme.palette().background,
            )
        };
        for (name, theme) in [
            (
                "light",
                Theme::custom_with_fn(
                    "preview",
                    super::super::design::palette(true, None),
                    super::super::design::extended_palette,
                ),
            ),
            (
                "dark",
                Theme::custom_with_fn(
                    "preview",
                    super::super::design::palette(false, None),
                    super::super::design::extended_palette,
                ),
            ),
            ("amoled", super::super::design::amoled_theme(None)),
        ] {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let expected = render(false, scale, &theme);
                let actual = render(true, scale, &theme);
                // Masked and unmasked raster paths can round coverage by one byte.
                assert!(
                    actual
                        .iter()
                        .zip(&expected)
                        .all(|(a, b)| a.abs_diff(*b) <= 1),
                    "scroll clipping changed pixels for {name} at scale {scale}"
                );
                if scale == 1.0 {
                    std::fs::create_dir_all("target/ui-preview").unwrap();
                    image::save_buffer(
                        format!("target/ui-preview/scrollbar-{name}.png"),
                        &actual,
                        640,
                        360,
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                }
            }
        }
    }

    #[test]
    fn shared_page_panel_and_horizontal_scroll_damage_stays_bounded() {
        for size in [Size::new(1200.0, 900.0), Size::new(320.0, 240.0)] {
            let items = iced::widget::column((0..200).map(|index| {
                super::super::widgets::settings_card(iced::widget::row![
                    iced::widget::text(format!("Setting {index}")).width(Length::Fill),
                    iced::widget::text("Configured value")
                ])
                .into()
            }))
            .spacing(8);
            let view: Element<'_, ()> = scrollable(items)
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
            check_scroll_damage(view, Rectangle::with_size(size));
        }
        let items = iced::widget::row((0..100).map(|index| {
            iced::widget::container(iced::widget::text(format!("Page {index}")))
                .width(180)
                .height(50)
                .into()
        }))
        .spacing(8);
        let view: Element<'_, ()> = horizontal_scrollable(items)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        check_scroll_damage_with_delta(
            view,
            Rectangle::with_size(Size::new(900.0, 100.0)),
            iced::mouse::ScrollDelta::Pixels { x: -12.0, y: 0.0 },
        );
    }

    #[test]
    fn table_background_matches_native_panel_fill_at_fractional_dpi() {
        use iced::advanced::renderer::{Headless as _, Renderer as _};
        for theme in [Theme::Light, Theme::Dark] {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let render = |tiled| {
                    let mut renderer =
                        Renderer::new(super::super::design::typography::FONT, iced::Pixels(14.0));
                    let content = iced::widget::Space::new().width(129).height(97);
                    let mut view: Element<'_, ()> = if tiled {
                        table_surface(content)
                    } else {
                        iced::widget::container(content)
                            .style(|theme| {
                                let mut style = super::super::widgets::surface(theme);
                                // Dense virtual tables retain their borderless tiled background.
                                style.border.width = 0.0;
                                style
                            })
                            .into()
                    };
                    let bounds = Rectangle::with_size(Size::new(132.0, 100.0));
                    let mut tree = Tree::new(&view);
                    let node = view.as_widget_mut().layout(
                        &mut tree,
                        &renderer,
                        &layout::Limits::new(Size::ZERO, bounds.size()),
                    );
                    renderer.reset(bounds);
                    renderer.with_translation(Vector::new(0.5, 0.5), |renderer| {
                        view.as_widget().draw(
                            &tree,
                            renderer,
                            &theme,
                            &renderer::Style::default(),
                            Layout::new(&node),
                            mouse::Cursor::Unavailable,
                            &bounds,
                        );
                    });
                    renderer.screenshot(
                        Size::new((132.0 * scale) as u32, (100.0 * scale) as u32),
                        scale,
                        theme.palette().background,
                    )
                };
                let expected = render(false);
                let actual = render(true);
                // Interior pixels remain exact; the canvas and native corner
                // paths can differ slightly in antialias coverage.
                let mismatch = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
                let max_difference = actual
                    .iter()
                    .zip(&expected)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap_or_default();
                assert!(
                    mismatch <= 240 && max_difference <= 8,
                    "surface changed at scale {scale}: {mismatch} channels, delta {max_difference}"
                );
                let width = (132.0 * scale) as usize;
                for y in (8.0 * scale) as usize..(89.0 * scale) as usize {
                    for x in (8.0 * scale) as usize..(121.0 * scale) as usize {
                        let pixel = (y * width + x) * 4;
                        assert_eq!(&actual[pixel..pixel + 4], &expected[pixel..pixel + 4]);
                    }
                }
            }
        }
    }

    #[test]
    fn scrolling_only_rebuilds_when_leaving_the_buffer() {
        for offset in 500..900 {
            assert!(!needs_rows(
                offset as f32,
                600.0,
                400.0,
                1600.0,
                false,
                false
            ));
        }
        assert!(needs_rows(420.0, 600.0, 400.0, 1600.0, false, false));
        assert!(needs_rows(1000.0, 600.0, 400.0, 1600.0, false, false));
        assert!(needs_rows(5000.0, 600.0, 400.0, 1600.0, false, false));
        assert!(!needs_rows(0.0, 600.0, 32.0, 200.0, true, true));
    }
}
