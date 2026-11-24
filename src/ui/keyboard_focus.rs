use iced::advanced::{
    renderer,
    widget::{operation, operation::Focusable, Id, Operation},
    Renderer as _, Shell,
};
use iced::{
    keyboard, keyboard::key::Named, mouse, touch, Event, Rectangle, Renderer, Theme, Vector,
};

pub(super) fn move_focus<M: Send + 'static>(forward: bool, wrap: bool) -> iced::Task<M> {
    let focus = if wrap {
        let target = operation::map(operation::focusable::count(), move |count| {
            FocusAt::wrapping(count, forward)
        });
        iced::advanced::widget::operate(operation::then(target, std::convert::identity))
    } else if forward {
        iced::widget::operation::focus_next()
    } else {
        iced::widget::operation::focus_previous()
    };
    focus.chain(iced::advanced::widget::operate(Reveal::default()))
}

struct FocusAt {
    target: Option<usize>,
    ordinal: usize,
}

impl FocusAt {
    fn wrapping(count: operation::focusable::Count, forward: bool) -> Self {
        let target = count
            .total
            .checked_sub(1)
            .map(|last| match (count.focused, forward) {
                (Some(index), true) if index < last => index + 1,
                (Some(index), false) if index > 0 => index - 1,
                (_, true) => 0,
                (_, false) => last,
            });
        Self { target, ordinal: 0 }
    }
}

impl<T> Operation<T> for FocusAt {
    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation<T>)) {
        visit(self);
    }

    fn focusable(&mut self, _: Option<&Id>, _: Rectangle, state: &mut dyn Focusable) {
        if self.target == Some(self.ordinal) {
            state.focus();
        } else {
            state.unfocus();
        }
        self.ordinal += 1;
    }
}

#[derive(Clone, Copy)]
struct Scroll {
    ordinal: usize,
    bounds: Rectangle,
    content: Rectangle,
    translation: Vector,
}

#[derive(Default)]
struct Reveal {
    ordinal: usize,
    pending: Option<Scroll>,
    ancestors: Vec<Scroll>,
    changes: Vec<(usize, Vector)>,
    apply: bool,
}

impl<T: Send + 'static> Operation<T> for Reveal {
    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation<T>)) {
        // Native Scrollable registers itself immediately before traversing its content.
        let entered = self.pending.take();
        if let Some(scroll) = entered {
            self.ancestors.push(scroll);
        }
        visit(self);
        if entered.is_some() {
            self.ancestors.pop();
        }
    }

    fn scrollable(
        &mut self,
        _: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        state: &mut dyn operation::Scrollable,
    ) {
        let ordinal = self.ordinal;
        self.ordinal += 1;
        if self.apply {
            if let Some((_, delta)) = self.changes.iter().find(|(index, _)| *index == ordinal) {
                state.scroll_by(
                    operation::scrollable::AbsoluteOffset {
                        x: delta.x,
                        y: delta.y,
                    },
                    bounds,
                    content,
                );
            }
        } else {
            self.pending = Some(Scroll {
                ordinal,
                bounds,
                content,
                translation,
            });
        }
    }

    fn focusable(&mut self, _: Option<&Id>, mut target: Rectangle, state: &mut dyn Focusable) {
        if self.apply || !state.is_focused() {
            return;
        }
        for scroll in self.ancestors.iter().rev() {
            target.x -= scroll.translation.x;
            target.y -= scroll.translation.y;
            let delta = Vector::new(
                reveal_axis(
                    target.x,
                    target.width,
                    scroll.bounds.x,
                    scroll.bounds.width,
                    scroll.translation.x,
                    scroll.content.width,
                ),
                reveal_axis(
                    target.y,
                    target.height,
                    scroll.bounds.y,
                    scroll.bounds.height,
                    scroll.translation.y,
                    scroll.content.height,
                ),
            );
            if delta != Vector::ZERO {
                self.changes.push((scroll.ordinal, delta));
            }
            target.x -= delta.x;
            target.y -= delta.y;
        }
    }

    fn finish(&self) -> operation::Outcome<T> {
        if self.apply || self.changes.is_empty() {
            operation::Outcome::None
        } else {
            operation::Outcome::Chain(Box::new(Self {
                changes: self.changes.clone(),
                apply: true,
                ..Self::default()
            }))
        }
    }
}

fn reveal_axis(
    start: f32,
    size: f32,
    viewport: f32,
    visible: f32,
    offset: f32,
    content: f32,
) -> f32 {
    let delta = (start + size - viewport - visible)
        .max(0.0)
        .min(start - viewport);
    (offset + delta).clamp(0.0, (content - visible).max(0.0)) - offset
}

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
    pub(super) fn pressed_key<M>(
        &self,
        event: &Event,
        shell: &Shell<'_, M>,
    ) -> Option<(Named, bool)> {
        self.pressed_named_key(event, shell)
            .filter(|(_, modifiers, _)| modifiers.is_empty())
            .map(|(key, _, repeat)| (key, repeat))
    }

    pub(super) fn pressed_named_key<M>(
        &self,
        event: &Event,
        shell: &Shell<'_, M>,
    ) -> Option<(Named, keyboard::Modifiers, bool)> {
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key),
            modifiers,
            repeat,
            ..
        }) = event
        {
            if self.0 && !shell.is_event_captured() {
                return Some((*key, *modifiers, *repeat));
            }
        }
        None
    }

    pub(super) fn activate<M>(&self, event: &Event, shell: &mut Shell<'_, M>) -> bool {
        if let Some((Named::Enter | Named::Space, repeat)) = self.pressed_key(event, shell) {
            shell.capture_event();
            return !repeat;
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
    fn wrapped_focus_handles_zero_and_one_control() {
        use operation::focusable::Count;

        for forward in [true, false] {
            assert_eq!(FocusAt::wrapping(Count::default(), forward).target, None);
            for focused in [None, Some(0)] {
                let count = Count { focused, total: 1 };
                assert_eq!(FocusAt::wrapping(count, forward).target, Some(0));
            }
        }
    }

    #[test]
    fn tab_focus_reveals_nested_controls_without_scrolling_siblings() {
        #[derive(Default)]
        struct Snapshot {
            count: usize,
            focused: Option<(usize, Rectangle)>,
            scrolls: Vec<(Rectangle, Vector)>,
        }
        impl Operation for Snapshot {
            fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
                visit(self);
            }
            fn focusable(&mut self, _: Option<&Id>, bounds: Rectangle, state: &mut dyn Focusable) {
                if state.is_focused() {
                    self.focused = Some((self.count, bounds));
                }
                self.count += 1;
            }
            fn scrollable(
                &mut self,
                id: Option<&Id>,
                bounds: Rectangle,
                _: Rectangle,
                translation: Vector,
                _: &mut dyn operation::Scrollable,
            ) {
                assert!(id.is_none(), "reveal must work without widget IDs");
                self.scrolls.push((bounds, translation));
            }
        }
        fn run(
            view: &mut Element<'_, ()>,
            tree: &mut Tree,
            node: &layout::Node,
            renderer: &Renderer,
            mut operation: Box<dyn Operation>,
        ) {
            loop {
                view.as_widget_mut()
                    .operate(tree, Layout::new(node), renderer, operation.as_mut());
                match operation.finish() {
                    operation::Outcome::Chain(next) => operation = next,
                    _ => break,
                }
            }
        }
        fn snapshot(
            view: &mut Element<'_, ()>,
            tree: &mut Tree,
            node: &layout::Node,
            renderer: &Renderer,
        ) -> Snapshot {
            let mut snapshot = Snapshot::default();
            view.as_widget_mut()
                .operate(tree, Layout::new(node), renderer, &mut snapshot);
            snapshot
        }

        let renderer = Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let size = Size::new(360.0, 180.0);
        for pane in [false, true] {
            let inner = iced::widget::column![
                widgets::action_button("First action").on_press(()),
                iced::widget::Space::new().height(400),
                widgets::action_button("Last action").on_press(()),
            ]
            .spacing(8);
            let inner = if pane {
                crate::ui::scrolling::scrollable_pane(inner)
            } else {
                crate::ui::scrolling::scrollable(inner)
            }
            .height(120);
            let sibling = crate::ui::scrolling::scrollable(iced::widget::column![
                iced::widget::Space::new().height(400),
                widgets::action_button("Sibling action").on_press(()),
            ])
            .width(180)
            .height(iced::Fill);
            let parent = crate::ui::scrolling::scrollable(iced::widget::column![
                iced::widget::Space::new().height(300),
                inner,
                iced::widget::Space::new().height(100),
            ])
            .width(180)
            .height(iced::Fill);
            let mut view: Element<'_, ()> = iced::widget::row![sibling, parent].into();
            let mut tree = Tree::new(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            for (forward, index) in [(true, 0), (true, 1), (true, 2), (false, 1), (false, 0)] {
                let focus: Box<dyn Operation> = if forward {
                    Box::new(operation::focusable::focus_next::<()>())
                } else {
                    Box::new(operation::focusable::focus_previous::<()>())
                };
                run(&mut view, &mut tree, &node, &renderer, focus);
                let before = snapshot(&mut view, &mut tree, &node, &renderer);
                run(
                    &mut view,
                    &mut tree,
                    &node,
                    &renderer,
                    Box::new(Reveal::default()),
                );
                let after = snapshot(&mut view, &mut tree, &node, &renderer);
                assert_eq!(after.count, 3);
                let (focused, mut target) = after.focused.unwrap();
                assert_eq!(focused, index);
                let viewport = if index == 0 {
                    assert_eq!(after.scrolls[1..], before.scrolls[1..]);
                    target.y -= after.scrolls[0].1.y;
                    after.scrolls[0].0
                } else {
                    assert_eq!(after.scrolls[0], before.scrolls[0]);
                    let (outer, offset) = after.scrolls[1];
                    let (mut inner, inner_offset) = after.scrolls[2];
                    inner.y -= offset.y;
                    target.y -= offset.y + inner_offset.y;
                    outer.intersection(&inner).unwrap()
                };
                assert!(target.y >= viewport.y, "{target:?} above {viewport:?}");
                assert!(
                    target.y + target.height <= viewport.y + viewport.height,
                    "{target:?} below {viewport:?}"
                );
                run(
                    &mut view,
                    &mut tree,
                    &node,
                    &renderer,
                    Box::new(Reveal::default()),
                );
                assert_eq!(
                    snapshot(&mut view, &mut tree, &node, &renderer).scrolls,
                    after.scrolls
                );
            }
        }
    }

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
                widgets::toggle_card("resource_thresholds.enable", false, Some(|value| value)),
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
