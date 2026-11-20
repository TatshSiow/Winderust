use iced::advanced::{renderer, widget::operation::Focusable, Renderer as _, Shell};
use iced::{keyboard, mouse, touch, Event, Rectangle, Renderer, Theme};

#[derive(Default)]
pub(super) struct Focus(bool);

impl Focusable for Focus {
    fn is_focused(&self) -> bool {
        self.0
    }

    fn focus(&mut self) {
        self.0 = true;
    }

    fn unfocus(&mut self) {
        self.0 = false;
    }
}

impl Focus {
    pub(super) fn activate<M>(&self, event: &Event, shell: &mut Shell<'_, M>) -> bool {
        use keyboard::{key::Named, Key};
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(Named::Enter | Named::Space),
            modifiers,
            repeat,
            ..
        }) = event
        {
            if self.0 && modifiers.is_empty() && !shell.is_event_captured() {
                shell.capture_event();
                return !repeat;
            }
        }
        false
    }

    pub(super) fn clear_on_pointer<M>(&mut self, event: &Event, shell: &mut Shell<'_, M>) {
        if self.0
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(_))
                    | Event::Touch(touch::Event::FingerPressed { .. })
            )
        {
            self.unfocus();
            shell.request_redraw();
        }
    }

    pub(super) fn draw(&self, renderer: &mut Renderer, theme: &Theme, bounds: Rectangle) {
        if self.0 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::Border {
                        color: theme.palette().text,
                        width: 2.0,
                        radius: super::design::CONTROL_RADIUS.into(),
                    },
                    ..Default::default()
                },
                iced::Color::TRANSPARENT,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{design, widgets};
    use iced::advanced::{layout, widget::Tree, Layout};
    use iced::{Element, Size};

    #[test]
    fn enabled_action_controls_take_one_tab_stop_and_activate_once() {
        #[derive(Default)]
        struct TabStops(usize);

        impl iced::advanced::widget::Operation for TabStops {
            fn traverse(
                &mut self,
                visit: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation),
            ) {
                visit(self);
            }

            fn focusable(
                &mut self,
                _: Option<&iced::advanced::widget::Id>,
                _: Rectangle,
                state: &mut dyn Focusable,
            ) {
                self.0 += 1;
                state.focus();
            }
        }

        let renderer = Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let size = Size::new(360.0, 70.0);
        let bounds = Rectangle::with_size(size);
        let controls: [(Element<'_, bool>, bool); 5] = [
            (widgets::action_button("Check").on_press(true).into(), true),
            (widgets::switch(false, Some(|value| value)), true),
            (
                widgets::checkbox(false).on_toggle(|value| value).into(),
                true,
            ),
            (
                widgets::toggle_card("watchdog.enable", false, Some(|value| value)),
                true,
            ),
            (widgets::action_button("Disabled").into(), false),
        ];

        for (mut element, enabled) in controls {
            let mut tree = Tree::new(&element);
            let widget = element.as_widget_mut();
            let node = widget.layout(&mut tree, &renderer, &layout::Limits::new(Size::ZERO, size));
            let mut tab_stops = TabStops::default();
            widget.operate(&mut tree, Layout::new(&node), &renderer, &mut tab_stops);
            assert_eq!(tab_stops.0, usize::from(enabled));
            for key in [keyboard::key::Named::Enter, keyboard::key::Named::Space] {
                let key = keyboard::Key::Named(key);
                for repeat in [false, true] {
                    let event = Event::Keyboard(keyboard::Event::KeyPressed {
                        key: key.clone(),
                        modified_key: key.clone(),
                        physical_key: keyboard::key::Physical::Unidentified(
                            keyboard::key::NativeCode::Unidentified,
                        ),
                        location: keyboard::Location::Standard,
                        modifiers: keyboard::Modifiers::empty(),
                        text: None,
                        repeat,
                    });
                    let mut messages = Vec::new();
                    let mut shell = Shell::new(&mut messages);
                    widget.update(
                        &mut tree,
                        &event,
                        Layout::new(&node),
                        mouse::Cursor::Unavailable,
                        &renderer,
                        &mut iced::advanced::clipboard::Null,
                        &mut shell,
                        &bounds,
                    );
                    assert_eq!(shell.is_event_captured(), enabled);
                    if enabled && !repeat {
                        assert_eq!(messages, [true]);
                    } else {
                        assert!(messages.is_empty());
                    }
                }
            }
        }
    }
}
