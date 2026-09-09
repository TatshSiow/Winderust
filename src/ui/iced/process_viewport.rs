//! Keep native scrolling local while the viewport is inside the rendered buffer.
use super::Message;
use iced::advanced::{
    layout, renderer,
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};
use iced::{mouse, overlay, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};
use std::cell::Cell;

pub(super) fn buffered<'a>(
    content: Element<'a, Message>,
    offset: &'a Cell<f32>,
    start: f32,
    end: f32,
    first: bool,
    last: bool,
) -> Element<'a, Message> {
    Element::new(Buffered {
        content,
        offset,
        start,
        end,
        first,
        last,
    })
}
struct Buffered<'a> {
    content: Element<'a, Message>,
    offset: &'a Cell<f32>,
    start: f32,
    end: f32,
    first: bool,
    last: bool,
}
fn needs_rows(offset: f32, height: f32, start: f32, end: f32, first: bool, last: bool) -> bool {
    (!first && offset < start + super::ROW_HEIGHT)
        || (!last && offset + height > end - super::ROW_HEIGHT)
}
impl Widget<Message, Theme, Renderer> for Buffered<'_> {
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
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
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
            if let Message::Scrolled(offset) = message {
                self.offset.set(offset);
                if !needs_rows(
                    offset,
                    layout.bounds().height,
                    self.start,
                    self.end,
                    self.first,
                    self.last,
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
mod tests {
    use super::*;
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
