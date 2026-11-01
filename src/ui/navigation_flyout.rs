use super::{navigation, Page};
use iced::advanced::{
    layout, renderer,
    widget::{operation::Focusable, tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};
use iced::{
    keyboard, mouse, overlay, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
};
use navigation::Event as Message;

pub(super) fn view<'a>(
    page: Page,
    anchor: Element<'a, Message>,
    panel: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    Element::new(Anchor {
        page: Some(page),
        anchor,
        panel,
    })
}

pub(super) fn search(anchor: Element<'_, Message>) -> Element<'_, Message> {
    Element::new(Anchor {
        page: None,
        anchor,
        panel: None,
    })
}

struct Anchor<'a> {
    page: Option<Page>,
    anchor: Element<'a, Message>,
    panel: Option<Element<'a, Message>>,
}

#[derive(Default)]
struct State {
    focused: bool,
    hovered: bool,
    panel_hovered: bool,
}

impl Focusable for State {
    fn is_focused(&self) -> bool {
        self.focused
    }
    fn focus(&mut self) {
        self.focused = true;
    }
    fn unfocus(&mut self) {
        self.focused = false;
    }
}

impl Widget<Message, Theme, Renderer> for Anchor<'_> {
    fn size(&self) -> Size<Length> {
        self.anchor.as_widget().size()
    }
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn children(&self) -> Vec<Tree> {
        let mut children = vec![Tree::new(&self.anchor)];
        if let Some(panel) = &self.panel {
            children.push(Tree::new(panel));
        }
        children
    }
    fn diff(&self, tree: &mut Tree) {
        tree.children[0].diff(&self.anchor);
        if let Some(panel) = &self.panel {
            if tree.children.len() == 1 {
                tree.children.push(Tree::new(panel));
            } else {
                tree.children[1].diff(panel);
            }
        } else {
            tree.children.truncate(1);
            tree.state.downcast_mut::<State>().panel_hovered = false;
        }
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
        if tree.state.downcast_ref::<State>().focused {
            use iced::advanced::Renderer as _;
            renderer.fill_quad(
                renderer::Quad {
                    bounds: layout.bounds(),
                    border: iced::Border {
                        color: theme.palette().primary,
                        width: 2.0,
                        radius: super::design::CONTROL_RADIUS.into(),
                    },
                    ..Default::default()
                },
                iced::Color::TRANSPARENT,
            );
        }
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.focusable(
            Some(&navigation::rail_id(self.page)),
            layout.bounds(),
            tree.state.downcast_mut::<State>(),
        );
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
        let state = tree.state.downcast_mut::<State>();
        let hovered = cursor.is_over(layout.bounds()) && cursor.is_over(*viewport);
        if state.focused
            && !hovered
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                    | Event::Touch(iced::touch::Event::FingerPressed { .. })
            )
        {
            state.focused = false;
            shell.publish(Message::Blur);
            shell.request_redraw();
        }
        if hovered != state.hovered {
            state.hovered = hovered;
            if let Some(page) = self.page.filter(|page| navigation::has_children(*page)) {
                shell.publish(if hovered {
                    Message::Hover(page)
                } else {
                    Message::Leave(page)
                });
            }
        }
        if let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
            if state.focused
                && (modifiers.is_empty()
                    || (*key == keyboard::Key::Named(keyboard::key::Named::Tab)
                        && *modifiers == keyboard::Modifiers::SHIFT))
            {
                use keyboard::{key::Named, Key};
                let message = match key {
                    Key::Named(Named::ArrowDown) => Some(Message::FocusMove(self.page, true)),
                    Key::Named(Named::ArrowUp) => Some(Message::FocusMove(self.page, false)),
                    Key::Named(Named::Tab) => Some(Message::Tab(self.page, !modifiers.shift())),
                    Key::Named(Named::ArrowRight) => self
                        .page
                        .filter(|page| navigation::has_children(*page))
                        .map(Message::OpenKeyboard),
                    Key::Named(Named::Enter | Named::Space) => Some(match self.page {
                        Some(page) => Message::Select(page),
                        None => Message::Search,
                    }),
                    _ => None,
                };
                if let Some(message) = message {
                    shell.publish(message);
                    shell.capture_event();
                    return;
                }
            }
        }
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
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.anchor.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let panel = self.panel.as_mut()?;
        let anchor = layout.bounds() + translation;
        // A scrolled-out anchor must not leave a detached menu over the page.
        if !viewport.intersects(&anchor) {
            return None;
        }
        Some(overlay::Element::new(Box::new(Panel {
            content: panel,
            tree: &mut tree.children[1],
            anchor,
            hovered: &mut tree.state.downcast_mut::<State>().panel_hovered,
        })))
    }
}

struct Panel<'a, 'b> {
    content: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    anchor: Rectangle,
    hovered: &'b mut bool,
}

impl iced::advanced::Overlay<Message, Theme, Renderer> for Panel<'_, '_> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let margin = super::design::space::CONTROL as f32;
        let max = Size::new(
            (bounds.width - 2.0 * margin).clamp(0.0, super::design::NAVIGATION_FLYOUT_WIDTH),
            (bounds.height - 2.0 * margin).max(0.0),
        );
        let node = self.content.as_widget_mut().layout(
            self.tree,
            renderer,
            &layout::Limits::new(Size::ZERO, max),
        );
        let size = node.size();
        node.move_to(iced::Point::new(
            (super::design::NAVIGATION_RAIL_WIDTH + 1.0)
                .min((bounds.width - size.width - margin).max(0.0)),
            self.anchor
                .y
                .max(margin)
                .min((bounds.height - size.height - margin).max(0.0)),
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
        use keyboard::{key::Named, Key};
        if let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
            if matches!(
                key,
                Key::Named(
                    Named::Escape
                        | Named::ArrowLeft
                        | Named::ArrowUp
                        | Named::ArrowDown
                        | Named::Enter
                        | Named::Space
                        | Named::Tab
                        | Named::Home
                        | Named::End
                )
            ) {
                shell.publish(Message::Key(key.clone(), modifiers.shift()));
                shell.capture_event();
                return;
            }
        }
        if matches!(event, Event::Window(iced::window::Event::Unfocused)) {
            shell.publish(Message::Dismiss);
            shell.capture_event();
            return;
        }
        let over = cursor.is_over(layout.bounds());
        if over != *self.hovered {
            *self.hovered = over;
            shell.publish(if over { Message::Enter } else { Message::Exit });
        }
        let outside_press = match event {
            Event::Mouse(mouse::Event::ButtonPressed(_)) => !over,
            Event::Touch(iced::touch::Event::FingerPressed { position, .. }) => {
                !layout.bounds().contains(*position)
            }
            _ => false,
        };
        if outside_press {
            // Let rail buttons open another section in this same click.
            shell.publish(Message::Dismiss);
            let position = match event {
                Event::Touch(iced::touch::Event::FingerPressed { position, .. }) => Some(*position),
                _ => cursor.position(),
            };
            if position.is_none_or(|position| position.x > super::design::NAVIGATION_RAIL_WIDTH) {
                shell.capture_event();
            }
            return;
        }
        self.content.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &layout.bounds(),
        );
        if over && matches!(event, Event::Mouse(_) | Event::Touch(_)) {
            shell.capture_event();
        }
    }
    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content
            .as_widget_mut()
            .operate(self.tree, layout, renderer, operation);
    }
    fn overlay<'c>(
        &'c mut self,
        layout: Layout<'c>,
        renderer: &Renderer,
    ) -> Option<overlay::Element<'c, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            self.tree,
            layout,
            renderer,
            &layout.bounds(),
            Vector::ZERO,
        )
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::Overlay;

    fn key(key: keyboard::key::Named) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key),
            modified_key: keyboard::Key::Named(key),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        })
    }

    #[test]
    fn rail_hover_requests_children_but_mouse_and_touch_select_the_parent_page() {
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let page = Page::SettingsHome;
        let mut element = view(
            page,
            navigation::rail_item(page, false, Message::Select(page)),
            None,
        );
        let mut tree = Tree::new(&element);
        let bounds = Rectangle::with_size(Size::new(76.0, 400.0));
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, bounds.size()),
        );
        let position = node.bounds().center();
        let cursor = mouse::Cursor::Available(position);
        let mut messages = Vec::new();
        for event in [
            Event::Mouse(mouse::Event::CursorMoved { position }),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            Event::Touch(iced::touch::Event::FingerPressed {
                id: iced::touch::Finger(1),
                position,
            }),
            Event::Touch(iced::touch::Event::FingerLifted {
                id: iced::touch::Finger(1),
                position,
            }),
        ] {
            element.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                cursor,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &bounds,
            );
        }
        assert!(matches!(
            messages.as_slice(),
            [
                Message::Hover(Page::SettingsHome),
                Message::Select(Page::SettingsHome),
                Message::Select(Page::SettingsHome)
            ]
        ));
    }

    #[test]
    fn rail_focus_supports_arrow_keys_and_opens_without_hover() {
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut element = view(
            Page::SettingsHome,
            iced::widget::button("Settings")
                .width(76)
                .height(64)
                .on_press(Message::Select(Page::SettingsHome))
                .into(),
            None,
        );
        let mut tree = Tree::new(&element);
        let bounds = Rectangle::with_size(Size::new(500.0, 400.0));
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, bounds.size()),
        );
        let mut focus = iced::advanced::widget::operation::focusable::focus::<()>(
            navigation::rail_id(Some(Page::SettingsHome)),
        );
        element
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut focus);
        assert!(tree.state.downcast_ref::<State>().focused);
        let mut messages = Vec::new();
        for event in [
            key(keyboard::key::Named::ArrowDown),
            key(keyboard::key::Named::Tab),
            key(keyboard::key::Named::ArrowRight),
            key(keyboard::key::Named::Enter),
        ] {
            let mut shell = Shell::new(&mut messages);
            element.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &bounds,
            );
            assert!(shell.is_event_captured());
        }
        assert!(matches!(
            messages.as_slice(),
            [
                Message::FocusMove(Some(Page::SettingsHome), true),
                Message::Tab(Some(Page::SettingsHome), true),
                Message::OpenKeyboard(Page::SettingsHome),
                Message::Select(Page::SettingsHome)
            ]
        ));
    }

    #[test]
    fn flyout_partial_repaints_match_full_frames_when_opening_moving_and_closing() {
        use iced::advanced::renderer::Renderer as _;
        let theme = Theme::custom_with_fn(
            "test",
            super::super::design::palette(true, None),
            super::super::design::extended_palette,
        );
        let size = Size::new(600.0, 400.0);
        let bounds = Rectangle::with_size(size);
        let viewport =
            iced::advanced::graphics::Viewport::with_physical_size(Size::new(600, 400), 1.0);
        let mut renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut pixels = tiny_skia::Pixmap::new(600, 400).unwrap();
        let mut mask = tiny_skia::Mask::new(600, 400).unwrap();
        let mut previous = Vec::new();
        let mut tree = Tree::empty();
        for (frame, position) in [None, Some(12.0), Some(12.0), Some(240.0), None]
            .into_iter()
            .enumerate()
        {
            let mut element = view(
                Page::SettingsHome,
                iced::widget::button("Settings")
                    .width(76)
                    .height(64)
                    .on_press(Message::Select(Page::SettingsHome))
                    .into(),
                position.map(|_| {
                    iced::widget::container(
                        iced::widget::column![
                            iced::widget::text("Settings"),
                            iced::widget::button("Winderust Behaviour")
                                .on_press(Message::Select(Page::WinderustBehaviour))
                        ]
                        .spacing(12),
                    )
                    .padding(12)
                    .width(Length::Fill)
                    .style(super::super::widgets::surface)
                    .into()
                }),
            );
            tree.diff(&element);
            let node = element
                .as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(Size::ZERO, size))
                .move_to(iced::Point::new(6.0, position.unwrap_or(12.0)));
            renderer.reset(bounds);
            let style = renderer::Style {
                text_color: theme.palette().text,
            };
            element.as_widget().draw(
                &tree,
                &mut renderer,
                &theme,
                &style,
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &bounds,
            );
            if let Some(mut overlay) = element.as_widget_mut().overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &bounds,
                Vector::ZERO,
            ) {
                let layout = overlay.as_overlay_mut().layout(&renderer, size);
                overlay.as_overlay().draw(
                    &mut renderer,
                    &theme,
                    &style,
                    Layout::new(&layout),
                    mouse::Cursor::Unavailable,
                );
            }
            let damage = if previous.is_empty() {
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
            if frame == 2 {
                assert!(damage.is_empty(), "Stable flyout repainted: {damage:?}");
            }
            renderer.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &viewport,
                &damage,
                theme.palette().background,
            );
            let mut full = tiny_skia::Pixmap::new(600, 400).unwrap();
            renderer.draw(
                &mut full.as_mut(),
                &mut mask,
                &viewport,
                &[bounds],
                theme.palette().background,
            );
            assert!(
                pixels.data() == full.data(),
                "Flyout left stale pixels in frame {frame}"
            );
            previous = renderer.layers().to_vec();
        }
    }

    #[test]
    fn floating_panel_keeps_anchor_size_and_uses_the_scrolled_position() {
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let make = |open: bool| {
            view(
                Page::SettingsHome,
                iced::widget::button("Settings")
                    .width(76)
                    .height(64)
                    .on_press(Message::Select(Page::SettingsHome))
                    .into(),
                open.then(|| {
                    iced::widget::container("Settings pages")
                        .width(Length::Fill)
                        .height(800)
                        .into()
                }),
            )
        };
        let mut closed = make(false);
        let mut open = make(true);
        let mut closed_tree = Tree::new(&closed);
        let mut tree = Tree::new(&open);
        let size = Size::new(1000.0, 700.0);
        let limits = layout::Limits::new(Size::ZERO, size);
        let closed_node = closed
            .as_widget_mut()
            .layout(&mut closed_tree, &renderer, &limits);
        let node = open
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits)
            .move_to(iced::Point::new(6.0, 800.0));
        assert_eq!(node.size(), closed_node.size());
        let viewport = Rectangle::with_size(size);
        let translation = Vector::new(0.0, -600.0);
        let mut overlay = open
            .as_widget_mut()
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &viewport,
                translation,
            )
            .unwrap();
        let panel = overlay.as_overlay_mut().layout(&renderer, size);
        assert_eq!(
            panel.bounds().x,
            super::super::design::NAVIGATION_RAIL_WIDTH + 1.0
        );
        assert!(viewport.contains(panel.bounds().position()));
        assert!(panel.bounds().x + panel.size().width <= size.width);
        assert!(panel.bounds().y + panel.size().height <= size.height);
        assert_eq!(panel.size().width, 252.0);
        drop(overlay);
        assert!(open
            .as_widget_mut()
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &viewport,
                Vector::ZERO
            )
            .is_none());
    }

    #[test]
    fn overlay_supports_pointer_crossing_touch_outside_click_and_escape() {
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut content: Element<'_, Message> = iced::widget::button("By Activity")
            .width(Length::Fill)
            .height(45)
            .on_press(Message::Select(Page::ByActivity))
            .into();
        let mut tree = Tree::new(&content);
        let mut hovered = false;
        let mut panel = Panel {
            content: &mut content,
            tree: &mut tree,
            hovered: &mut hovered,
            anchor: Rectangle::new(iced::Point::new(6.0, 680.0), Size::new(76.0, 64.0)),
        };
        let size = Size::new(400.0, 300.0);
        let node = panel.layout(&renderer, size);
        assert!(node.bounds().y + node.size().height <= size.height);
        let inside = node.bounds().center();
        let outside = iced::Point::new(390.0, 20.0);
        let mut clipboard = iced::advanced::clipboard::Null;
        let mut messages = Vec::new();
        for (point, expected) in [(inside, true), (outside, false), (inside, true)] {
            let mut shell = Shell::new(&mut messages);
            panel.update(
                &Event::Mouse(mouse::Event::CursorMoved { position: point }),
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut clipboard,
                &mut shell,
            );
            assert_eq!(shell.is_event_captured(), expected);
        }
        assert!(matches!(
            messages.as_slice(),
            [Message::Enter, Message::Exit, Message::Enter]
        ));
        messages.clear();
        for event in [
            iced::touch::Event::FingerPressed {
                id: iced::touch::Finger(1),
                position: inside,
            },
            iced::touch::Event::FingerLifted {
                id: iced::touch::Finger(1),
                position: inside,
            },
        ] {
            panel.update(
                &Event::Touch(event),
                Layout::new(&node),
                mouse::Cursor::Available(inside),
                &renderer,
                &mut clipboard,
                &mut Shell::new(&mut messages),
            );
        }
        assert!(messages
            .iter()
            .any(|message| matches!(message, Message::Select(Page::ByActivity))));
        messages.clear();
        for event in [
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Touch(iced::touch::Event::FingerPressed {
                id: iced::touch::Finger(1),
                position: outside,
            }),
        ] {
            let mut shell = Shell::new(&mut messages);
            panel.update(
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(outside),
                &renderer,
                &mut clipboard,
                &mut shell,
            );
            assert!(shell.is_event_captured());
        }
        assert_eq!(
            messages
                .iter()
                .filter(|message| matches!(message, Message::Dismiss))
                .count(),
            2
        );
        let mut shell = Shell::new(&mut messages);
        panel.update(
            &key(keyboard::key::Named::Escape),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut clipboard,
            &mut shell,
        );
        assert!(shell.is_event_captured());
        assert!(matches!(
            messages.last(),
            Some(Message::Key(
                keyboard::Key::Named(keyboard::key::Named::Escape),
                false
            ))
        ));
    }
}
