use iced::advanced::{
    layout, renderer,
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};
use iced::{
    keyboard, mouse, overlay, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
};

pub(super) fn view<'a, Message: 'a>(
    id: u64,
    anchor: Element<'a, Message>,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    Element::new(Popover {
        id,
        anchor,
        content,
        context: false,
    })
}
pub(super) fn context_menu<'a, Message: 'a>(
    id: u64,
    anchor: Element<'a, Message>,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    Element::new(Popover {
        id,
        anchor,
        content,
        context: true,
    })
}
struct Popover<'a, Message> {
    context: bool,
    id: u64,
    anchor: Element<'a, Message>,
    content: Element<'a, Message>,
}
#[derive(Default)]
struct State {
    id: u64,
    open: bool,
}
impl<Message> Widget<Message, Theme, Renderer> for Popover<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.anchor.as_widget().size()
    }
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State {
            id: self.id,
            open: false,
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.anchor), Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.id != self.id {
            *state = State {
                id: self.id,
                open: false,
            };
            tree.children[1] = Tree::new(&self.content);
        }
        tree.children[0].diff(&self.anchor);
        tree.children[1].diff(&self.content);
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.anchor
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        self.anchor.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.anchor
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        if self.context
            && !matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
            )
        {
            self.anchor.as_widget_mut().update(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
        let trigger = if self.context {
            matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
            )
        } else {
            matches!(
                event,
                Event::Mouse(
                    mouse::Event::CursorMoved { .. }
                        | mouse::Event::ButtonPressed(mouse::Button::Left)
                )
            )
        };
        if trigger && cursor.is_over(layout.bounds()) && cursor.is_over(*viewport) {
            let state = tree.state.downcast_mut::<State>();
            if !state.open {
                state.open = true;
                shell.invalidate_layout();
                shell.request_redraw();
            }
            if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_))) {
                shell.capture_event();
            }
        }
    }
    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        if !state.open {
            return None;
        }
        if !viewport.intersects(&layout.bounds()) {
            state.open = false;
            return None;
        }
        Some(overlay::Element::new(Box::new(Popup {
            content: &mut self.content,
            tree: &mut tree.children[1],
            open: &mut state.open,
            anchor: layout.bounds() + translation,
            context: self.context,
        })))
    }
}
struct Popup<'a, 'b, Message> {
    context: bool,
    content: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    open: &'b mut bool,
    anchor: Rectangle,
}
impl<Message> iced::advanced::Overlay<Message, Theme, Renderer> for Popup<'_, '_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let below = (bounds.height - self.anchor.y - self.anchor.height).max(0.0);
        let above = self.anchor.y.max(0.0);
        let node = self.content.as_widget_mut().layout(
            self.tree,
            renderer,
            &layout::Limits::new(
                Size::ZERO,
                Size::new(bounds.width.min(360.0), below.max(above).min(300.0)),
            ),
        );
        let size = node.size();
        node.move_to(iced::Point::new(
            self.anchor.x.min((bounds.width - size.width).max(0.0)),
            if below >= size.height {
                self.anchor.y + self.anchor.height
            } else {
                (self.anchor.y - size.height).max(0.0)
            },
        ))
    }
    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.content.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &layout.bounds(),
        );
    }
    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let outside =
            !cursor.is_over(layout.bounds()) && (self.context || !cursor.is_over(self.anchor));
        let escape = matches!(
            event,
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            })
        );
        if escape
            || matches!(event, Event::Window(iced::window::Event::Unfocused))
            || (outside && matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_))))
            || (!self.context
                && outside
                && matches!(event, Event::Mouse(mouse::Event::CursorMoved { .. })))
        {
            *self.open = false;
            shell.invalidate_layout();
            shell.request_redraw();
            if escape {
                shell.capture_event();
            }
            return;
        }
        let mut messages = Vec::new();
        let mut content_shell = Shell::new(&mut messages);
        self.content.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            &mut content_shell,
            &layout.bounds(),
        );
        let selected = !content_shell.is_empty();
        shell.merge(content_shell, |message| message);
        if self.context && selected {
            *self.open = false;
            shell.invalidate_layout();
            shell.request_redraw();
        }
        if cursor.is_over(layout.bounds()) && matches!(event, Event::Mouse(_)) {
            shell.capture_event();
        }
    }
    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let interaction = self.content.as_widget().mouse_interaction(
            self.tree,
            layout,
            cursor,
            &layout.bounds(),
            renderer,
        );
        if cursor.is_over(layout.bounds()) {
            interaction.max(mouse::Interaction::Idle)
        } else {
            interaction
        }
    }
    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content
            .as_widget_mut()
            .operate(self.tree, layout, renderer, operation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::action_log::Message;
    use iced::advanced::Overlay;
    #[test]
    fn context_menu_stays_open_until_dismissed_or_an_action_is_selected() {
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut content: Element<'_, ()> = iced::widget::button("Edit").on_press(()).into();
        let mut tree = Tree::new(&content);
        let mut open = true;
        let mut popup = Popup {
            content: &mut content,
            tree: &mut tree,
            open: &mut open,
            anchor: Rectangle::new(iced::Point::new(20., 20.), Size::new(42., 42.)),
            context: true,
        };
        let node = popup.layout(&renderer, Size::new(400., 300.));
        let mut messages = Vec::new();
        let mut clipboard = iced::advanced::clipboard::Null;
        popup.update(
            &Event::Mouse(mouse::Event::CursorMoved {
                position: iced::Point::ORIGIN,
            }),
            Layout::new(&node),
            mouse::Cursor::Available(iced::Point::ORIGIN),
            &renderer,
            &mut clipboard,
            &mut Shell::new(&mut messages),
        );
        assert!(*popup.open);
        let cursor = mouse::Cursor::Available(node.bounds().center());
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            popup.update(
                &Event::Mouse(event),
                Layout::new(&node),
                cursor,
                &renderer,
                &mut clipboard,
                &mut Shell::new(&mut messages),
            );
        }
        assert!(!*popup.open);
        assert_eq!(messages, vec![()]);
        *popup.open = true;
        popup.update(
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Layout::new(&node),
            mouse::Cursor::Available(iced::Point::ORIGIN),
            &renderer,
            &mut clipboard,
            &mut Shell::new(&mut messages),
        );
        assert!(!*popup.open);
    }

    #[test]
    fn process_popup_stays_in_bounds_and_closes_only_after_leaving() {
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut content: Element<'_, Message> =
            crate::ui::scrolling::scrollable(iced::widget::column(
                (0..40).map(|i| iced::widget::text(format!("app-{i}.exe ({i})")).into()),
            ))
            .height(Length::Fill)
            .width(Length::Shrink)
            .into();
        let mut tree = Tree::new(&content);
        let mut open = true;
        let anchor = Rectangle {
            x: 390.0,
            y: 270.0,
            width: 100.0,
            height: 20.0,
        };
        let mut popup = Popup {
            content: &mut content,
            tree: &mut tree,
            open: &mut open,
            anchor,
            context: false,
        };
        let node = popup.layout(&renderer, Size::new(500.0, 300.0));
        let bounds = node.bounds();
        assert!(bounds.x >= 0.0 && bounds.x + bounds.width <= 500.0);
        assert!(bounds.y >= 0.0 && bounds.y + bounds.height <= 300.0);
        assert!(bounds.width < 360.0);
        assert_ne!(
            popup.mouse_interaction(
                Layout::new(&node),
                mouse::Cursor::Available(bounds.center()),
                &renderer,
            ),
            mouse::Interaction::None,
        );
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        popup.update(
            &Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -3.0 },
            }),
            Layout::new(&node),
            mouse::Cursor::Available(bounds.center()),
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
        );
        assert!(shell.is_event_captured());
        assert!(*popup.open);
        for (position, expected) in [
            (anchor.center(), true),
            (bounds.center(), true),
            (iced::Point::ORIGIN, false),
        ] {
            let mut shell = Shell::new(&mut messages);
            popup.update(
                &Event::Mouse(mouse::Event::CursorMoved { position }),
                Layout::new(&node),
                mouse::Cursor::Available(position),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
            );
            assert_eq!(*popup.open, expected);
            if position == bounds.center() {
                assert!(shell.is_event_captured());
            }
        }
    }
}
