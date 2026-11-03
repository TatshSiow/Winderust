//! Modal input boundaries without changing popup placement or background state.
use iced::advanced::{
    layout, renderer,
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};
use iced::{keyboard, mouse, overlay, Element, Event, Rectangle, Renderer, Size, Theme, Vector};

pub(super) fn background<'a, M: Clone + 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    Element::new(Guard {
        content: content.into(),
        blocked: true,
        dismiss: None,
    })
}

pub(super) fn picker<'a, M: Clone + 'a>(
    content: impl Into<Element<'a, M>>,
    open: bool,
    dismiss: M,
) -> Element<'a, M> {
    Element::new(Guard {
        content: content.into(),
        blocked: open,
        dismiss: Some(dismiss),
    })
}

struct Guard<'a, M> {
    content: Element<'a, M>,
    blocked: bool,
    dismiss: Option<M>,
}

fn input(event: &Event) -> bool {
    !matches!(event, Event::Window(_))
}

impl<M: Clone> Widget<M, Theme, Renderer> for Guard<'_, M> {
    // Remain transparent to the tree so opening a modal preserves scroll/focus state.
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
    fn size(&self) -> Size<iced::Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<iced::Length> {
        self.content.as_widget().size_hint()
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
        self.content.as_widget().draw(
            tree,
            renderer,
            theme,
            style,
            layout,
            if self.blocked {
                mouse::Cursor::Unavailable
            } else {
                cursor
            },
            viewport,
        );
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        if self.blocked && input(event) {
            return;
        }
        self.content.as_widget_mut().update(
            tree,
            event,
            layout,
            if self.blocked {
                mouse::Cursor::Unavailable
            } else {
                cursor
            },
            renderer,
            clipboard,
            shell,
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
        if !self.blocked {
            self.content
                .as_widget_mut()
                .operate(tree, layout, renderer, operation);
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.blocked {
            mouse::Interaction::None
        } else {
            self.content
                .as_widget()
                .mouse_interaction(tree, layout, cursor, viewport, renderer)
        }
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, M, Theme, Renderer>> {
        if self.blocked && self.dismiss.is_none() {
            return None;
        }
        let dismiss = self.dismiss.as_ref().filter(|_| self.blocked).cloned();
        let bounds = layout.bounds() + translation;
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
            .map(|content| match dismiss {
                Some(dismiss) => overlay::Element::new(Box::new(Picker {
                    content,
                    bounds,
                    dismiss,
                })),
                None => content,
            })
    }
}

struct Picker<'a, M> {
    content: overlay::Element<'a, M, Theme, Renderer>,
    bounds: Rectangle,
    dismiss: M,
}

impl<M: Clone> iced::advanced::Overlay<M, Theme, Renderer> for Picker<'_, M> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        layout::Node::with_children(
            bounds,
            vec![self.content.as_overlay_mut().layout(renderer, bounds)],
        )
    }
    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.content.as_overlay().draw(
            renderer,
            theme,
            style,
            layout.children().next().unwrap(),
            cursor,
        );
    }
    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
    ) {
        let popup = layout.children().next().unwrap();
        let point = match event {
            Event::Touch(iced::touch::Event::FingerPressed { position, .. }) => Some(*position),
            _ => cursor.position(),
        };
        // Window chrome and resize edges remain usable outside the workspace.
        if input(event)
            && !matches!(event, Event::Keyboard(_) | Event::InputMethod(_))
            && point.is_some_and(|point| !self.bounds.contains(point))
        {
            return;
        }
        let escape = matches!(
            event,
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            })
        );
        let outside_press = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(_))
                | Event::Touch(iced::touch::Event::FingerPressed { .. })
        ) && point.is_some_and(|point| {
            // Parent stacks expose full-window overlay groups, not just popup bounds.
            self.content.as_overlay().mouse_interaction(
                popup,
                mouse::Cursor::Available(point),
                renderer,
            ) == mouse::Interaction::None
        });
        if escape || outside_press {
            shell.publish(self.dismiss.clone());
            shell.request_redraw();
        } else {
            self.content
                .as_overlay_mut()
                .update(event, popup, cursor, renderer, clipboard, shell);
        }
        if input(event) {
            shell.capture_event();
        }
    }
    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if !cursor.is_over(self.bounds) {
            return mouse::Interaction::None;
        }
        self.content
            .as_overlay()
            .mouse_interaction(layout.children().next().unwrap(), cursor, renderer)
            .max(mouse::Interaction::Idle)
    }
    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content.as_overlay_mut().operate(
            layout.children().next().unwrap(),
            renderer,
            operation,
        );
    }
    fn overlay<'a>(
        &'a mut self,
        layout: Layout<'a>,
        renderer: &Renderer,
    ) -> Option<overlay::Element<'a, M, Theme, Renderer>> {
        self.content
            .as_overlay_mut()
            .overlay(layout.children().next().unwrap(), renderer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{
        widget::{button, mouse_area},
        Point,
    };

    #[derive(Debug, Clone, PartialEq)]
    enum Message {
        Background,
        Hover,
        Scroll,
        Accept,
        Dismiss,
        Input(String),
    }

    fn fixture(open: bool) -> Element<'static, Message> {
        picker(
            iced::widget::stack![iced_aw::DropDown::new(
                mouse_area(
                    button("Background")
                        .width(400)
                        .height(300)
                        .on_press(Message::Background),
                )
                .on_enter(Message::Hover)
                .on_scroll(|_| Message::Scroll),
                iced::widget::opaque(
                    iced::widget::container(
                        button("OK")
                            .width(iced::Fill)
                            .height(iced::Fill)
                            .on_press(Message::Accept),
                    )
                    .width(120)
                    .height(60)
                    .padding(8)
                ),
                open,
            )
            .width(120)
            .alignment(iced_aw::drop_down::Alignment::End)
            .offset([-260.0, 0.0])],
            open,
            Message::Dismiss,
        )
    }

    fn send(
        view: &mut Element<'_, Message>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &Renderer,
        event: Event,
        cursor: mouse::Cursor,
    ) -> (Vec<Message>, bool) {
        let viewport = Rectangle::with_size(Size::new(400.0, 336.0));
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        let mut clipboard = iced::advanced::clipboard::Null;
        let mut base_cursor = cursor;
        if let Some(mut overlay) =
            view.as_widget_mut()
                .overlay(tree, Layout::new(node), renderer, &viewport, Vector::ZERO)
        {
            let overlay = overlay.as_overlay_mut();
            let overlay_node = overlay.layout(renderer, viewport.size());
            if overlay.mouse_interaction(Layout::new(&overlay_node), cursor, renderer)
                != mouse::Interaction::None
            {
                base_cursor = mouse::Cursor::Unavailable;
            }
            overlay.update(
                &event,
                Layout::new(&overlay_node),
                cursor,
                renderer,
                &mut clipboard,
                &mut shell,
            );
        }
        if !shell.is_event_captured() {
            view.as_widget_mut().update(
                tree,
                &event,
                Layout::new(node),
                base_cursor,
                renderer,
                &mut clipboard,
                &mut shell,
                &viewport,
            );
        }
        let captured = shell.is_event_captured();
        (messages, captured)
    }

    #[test]
    fn picker_captures_background_input_dismisses_without_clickthrough_and_keeps_chrome_available()
    {
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let viewport = Rectangle::with_size(Size::new(400.0, 336.0));
        let mut view = fixture(true);
        let mut tree = Tree::new(&view);
        let node = view
            .as_widget_mut()
            .layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, viewport.size()),
            )
            .move_to(Point::new(0.0, 36.0));
        let outside = Point::new(20.0, 70.0);
        for event in [
            Event::Mouse(mouse::Event::CursorMoved { position: outside }),
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
            }),
            Event::Keyboard(keyboard::Event::ModifiersChanged(
                keyboard::Modifiers::SHIFT,
            )),
        ] {
            assert_eq!(
                send(
                    &mut view,
                    &mut tree,
                    &node,
                    &renderer,
                    event,
                    mouse::Cursor::Available(outside)
                ),
                (vec![], true)
            );
        }
        for event in [
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle)),
            Event::Touch(iced::touch::Event::FingerPressed {
                id: iced::touch::Finger(0),
                position: outside,
            }),
        ] {
            assert_eq!(
                send(
                    &mut view,
                    &mut tree,
                    &node,
                    &renderer,
                    event,
                    mouse::Cursor::Available(outside)
                ),
                (vec![Message::Dismiss], true)
            );
        }
        {
            let mut overlay = view
                .as_widget_mut()
                .overlay(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &viewport,
                    Vector::ZERO,
                )
                .unwrap();
            let overlay = overlay.as_overlay_mut();
            let layout = overlay.layout(&renderer, viewport.size());
            assert_eq!(
                layout.children()[0].children()[0].bounds().center(),
                Point::new(200.0, 186.0)
            );
            assert_eq!(
                overlay.mouse_interaction(
                    Layout::new(&layout),
                    mouse::Cursor::Available(outside),
                    &renderer
                ),
                mouse::Interaction::Idle
            );
            let chrome = mouse::Cursor::Available(Point::new(20.0, 10.0));
            assert_eq!(
                overlay.mouse_interaction(Layout::new(&layout), chrome, &renderer),
                mouse::Interaction::None
            );
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            overlay.update(
                &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Layout::new(&layout),
                chrome,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
            );
            assert!(!shell.is_event_captured());
            assert!(messages.is_empty());
        }
        // Rebuild after an outside press closes the picker. Its release must not click the parent.
        view = fixture(false);
        view.as_widget().diff(&mut tree);
        assert!(!send(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            mouse::Cursor::Available(outside)
        )
        .0
        .contains(&Message::Background));
        let _ = send(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            mouse::Cursor::Available(outside),
        );
        assert_eq!(
            send(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                mouse::Cursor::Available(outside)
            )
            .0,
            vec![Message::Background]
        );
    }

    #[test]
    fn picker_buttons_remain_interactive() {
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut view = fixture(true);
        let mut tree = Tree::new(&view);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(400.0, 300.0)),
        );
        let cursor = mouse::Cursor::Available(Point::new(200.0, 150.0));
        assert!(
            send(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                mouse::Cursor::Available(Point::new(141.0, 121.0))
            )
            .0
            .is_empty(),
            "blank popup padding must not dismiss the picker"
        );
        assert!(send(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            cursor
        )
        .0
        .is_empty());
        assert_eq!(
            send(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                cursor
            )
            .0,
            vec![Message::Accept]
        );
    }

    #[test]
    fn picker_text_input_and_escape_work_without_reaching_the_parent() {
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut view = picker(
            iced_aw::DropDown::new(
                button("Background")
                    .width(400)
                    .height(300)
                    .on_press(Message::Background),
                iced::widget::text_input("", "")
                    .width(120)
                    .on_input(Message::Input),
                true,
            )
            .width(120)
            .alignment(iced_aw::drop_down::Alignment::End)
            .offset([-260.0, 0.0]),
            true,
            Message::Dismiss,
        );
        let mut tree = Tree::new(&view);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(400.0, 300.0)),
        );
        let cursor = mouse::Cursor::Available(Point::new(200.0, 150.0));
        let _ = send(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            cursor,
        );
        let key = |key: keyboard::Key, text| {
            Event::Keyboard(keyboard::Event::KeyPressed {
                modified_key: key.clone(),
                key,
                physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyX),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::default(),
                text,
                repeat: false,
            })
        };
        assert_eq!(
            send(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                key(keyboard::Key::Character("x".into()), Some("x".into())),
                cursor
            ),
            (vec![Message::Input("x".into())], true)
        );
        assert_eq!(
            send(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                key(keyboard::Key::Named(keyboard::key::Named::Escape), None),
                cursor
            ),
            (vec![Message::Dismiss], true)
        );
    }

    #[test]
    fn modal_background_keeps_input_state_but_cannot_hover_edit_or_open_overlays() {
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let field = || {
            iced::widget::text_input("", "")
                .width(400)
                .on_input(Message::Input)
        };
        let mut view = picker(field(), false, Message::Dismiss);
        let mut tree = Tree::new(&view);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(400.0, 300.0)),
        );
        let cursor = mouse::Cursor::Available(node.bounds().center());
        let _ = send(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            cursor,
        );
        let typed = Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character("x".into()),
            modified_key: keyboard::Key::Character("x".into()),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyX),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
            text: Some("x".into()),
            repeat: false,
        });
        view = background(field());
        view.as_widget().diff(&mut tree);
        assert!(send(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            typed.clone(),
            cursor
        )
        .0
        .is_empty());
        view = picker(field(), false, Message::Dismiss);
        view.as_widget().diff(&mut tree);
        assert_eq!(
            send(&mut view, &mut tree, &node, &renderer, typed, cursor).0,
            vec![Message::Input("x".into())]
        );

        let mut popup = background(fixture(true));
        let mut tree = Tree::new(&popup);
        let node = popup.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(400.0, 300.0)),
        );
        assert!(popup
            .as_widget_mut()
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &Rectangle::with_size(Size::new(400.0, 300.0)),
                Vector::ZERO
            )
            .is_none());
        for event in [
            Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(20.0, 20.0),
            }),
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
            }),
        ] {
            assert!(send(&mut popup, &mut tree, &node, &renderer, event, cursor)
                .0
                .is_empty());
        }
    }
}
