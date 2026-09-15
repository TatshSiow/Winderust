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
    Element::new(ScrollContent {
        content: content.into(),
        behavior: Behavior::Surface,
    })
}
pub(super) fn repaint_group<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    Element::new(ScrollContent {
        content: content.into(),
        behavior: Behavior::Group,
    })
}

enum Behavior<'a, Message> {
    Surface,
    Scroll,
    Group,
    Buffer(Buffer<'a, Message>),
}

/// Retain native input, scrollbars, IDs and overlays; share the repaint policy.
pub(super) fn scrollable<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> iced::widget::Scrollable<'a, Message> {
    iced::widget::scrollable(Element::new(ScrollContent {
        content: content.into(),
        behavior: Behavior::Scroll,
    }))
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
        self.content.as_widget_mut().layout(tree, renderer, limits)
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
        if matches!(self.behavior, Behavior::Group) {
            use iced::advanced::Renderer as _;
            if let Some(clip) = layout.bounds().intersection(viewport) {
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
            if matches!(self.behavior, Behavior::Surface) {
                use iced::advanced::Renderer as _;
                let bounds = layout.bounds();
                let radius = super::design::CARD_RADIUS;
                // Small quads let the software renderer skip masking the untouched
                // portion of a large table background during partial repaints.
                // Overlap internal edges to avoid antialiasing seams at fractional DPI.
                let columns = ((bounds.width / 64.0) as usize).max(1);
                let rows = ((bounds.height / 64.0) as usize).max(1);
                for y in 0..rows {
                    for x in 0..columns {
                        let tile = Rectangle {
                            x: bounds.x + x as f32 * 64.0,
                            y: bounds.y + y as f32 * 64.0,
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
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: tile,
                                border: iced::Border {
                                    radius: iced::border::Radius {
                                        top_left: if x == 0 && y == 0 { radius } else { 0.0 },
                                        top_right: if x + 1 == columns && y == 0 {
                                            radius
                                        } else {
                                            0.0
                                        },
                                        bottom_left: if x == 0 && y + 1 == rows {
                                            radius
                                        } else {
                                            0.0
                                        },
                                        bottom_right: if x + 1 == columns && y + 1 == rows {
                                            radius
                                        } else {
                                            0.0
                                        },
                                    },
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                            theme.extended_palette().background.weak.color,
                        );
                    }
                }
            }
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
    fn shared_scroll_preserves_native_pixels_after_scrolling() {
        use iced::advanced::renderer::{Headless as _, Renderer as _};
        let render = |shared, scale| {
            let mut renderer =
                Renderer::new(super::super::design::typography::FONT, iced::Pixels(14.0));
            let items = iced::widget::column((0..30).map(|index| {
                super::super::widgets::settings_card(
                    iced::widget::row![
                        super::super::navigation::glyph("icons/info.svg"),
                        iced::widget::text(format!("Setting {index}: configured value"))
                    ]
                    .spacing(12),
                )
                .into()
            }))
            .spacing(8);
            let mut view: Element<'_, ()> = if shared {
                scrollable(items)
            } else {
                iced::widget::scrollable(items)
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
                &Theme::Dark,
                &renderer::Style::default(),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &bounds,
            );
            renderer.screenshot(
                Size::new((640.0 * scale) as u32, (360.0 * scale) as u32),
                scale,
                Theme::Dark.palette().background,
            )
        };
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let expected = render(false, scale);
            let actual = render(true, scale);
            // Masked and unmasked raster paths can round coverage by one byte.
            assert!(
                actual
                    .iter()
                    .zip(&expected)
                    .all(|(a, b)| a.abs_diff(*b) <= 1),
                "scroll clipping changed pixels at scale {scale}"
            );
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
        let view: Element<'_, ()> = scrollable(items)
            .direction(iced::widget::scrollable::Direction::Horizontal(
                Default::default(),
            ))
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
    fn table_background_matches_native_surface_at_fractional_dpi() {
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
                            .style(super::super::widgets::surface)
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
                // Interior pixels must remain exact. At a handful of outer-edge
                // joins, antialias coverage can differ by at most 6/255.
                let mismatch = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
                let max_difference = actual
                    .iter()
                    .zip(&expected)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap_or_default();
                assert!(
                    mismatch <= 18 && max_difference <= 6,
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
