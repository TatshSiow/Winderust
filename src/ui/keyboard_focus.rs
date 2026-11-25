use iced::advanced::{
    renderer,
    widget::{operation, operation::Focusable, Id, Operation},
    Renderer as _, Shell,
};
use iced::{
    keyboard, keyboard::key::Named, mouse, touch, Event, Rectangle, Renderer, Theme, Vector,
};

pub(super) const SAVE_CARD: &str = "floating-save-card";

pub(super) fn move_focus<M: Send + 'static>(forward: bool, wrap: bool) -> iced::Task<M> {
    focus_step(forward, wrap).chain(reveal(None))
}

pub(super) fn move_focus_with_save_card<M: Send + 'static>(
    forward: bool,
    wrap: bool,
    move_to_header: M,
) -> iced::Task<M> {
    let mut move_to_header = Some(move_to_header);
    focus_step(forward, wrap).chain(iced::advanced::widget::operate(Reveal::default()).then(
        move |()| {
            move_to_header
                .take()
                .map_or_else(iced::Task::none, iced::Task::done)
        },
    ))
}

fn focus_step<M: Send + 'static>(forward: bool, wrap: bool) -> iced::Task<M> {
    if wrap {
        let target = operation::map(operation::focusable::count(), move |count| {
            FocusAt::wrapping(count, forward)
        });
        iced::advanced::widget::operate(operation::then(target, std::convert::identity))
    } else if forward {
        iced::widget::operation::focus_next()
    } else {
        iced::widget::operation::focus_previous()
    }
}

pub(super) fn reveal<M: Send + 'static>(target: Option<Id>) -> iced::Task<M> {
    iced::advanced::widget::operate(Reveal {
        target,
        ..Reveal::default()
    })
    .then(|()| iced::Task::none())
}

pub(super) fn reveal_within<M: Send + 'static>(scope: Id, fallback: Option<Id>) -> iced::Task<M> {
    iced::advanced::widget::operate(Reveal {
        target: fallback,
        focus_scope: Some(scope),
        ..Reveal::default()
    })
    .then(|()| iced::Task::none())
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
    is_focus_scope: bool,
}

#[derive(Default)]
struct Reveal {
    target: Option<Id>,
    focus_scope: Option<Id>,
    found_scoped_focus: bool,
    ordinal: usize,
    pending: Option<Scroll>,
    ancestors: Vec<Scroll>,
    targets: Vec<(Rectangle, Vec<Scroll>)>,
    save_card: Option<Rectangle>,
    entering_save_card: bool,
    in_save_card: bool,
    changes: Vec<(usize, Vector)>,
    apply: bool,
    covered: bool,
}

impl Operation for Reveal {
    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
        // Native Scrollable registers itself immediately before traversing its content.
        let entered = self.pending.take();
        if let Some(scroll) = entered {
            self.ancestors.push(scroll);
        }
        let entered_save_card = std::mem::take(&mut self.entering_save_card);
        self.in_save_card |= entered_save_card;
        visit(self);
        if entered_save_card {
            self.in_save_card = false;
        }
        if entered.is_some() {
            self.ancestors.pop();
        }
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&Id::new(SAVE_CARD)) {
            self.save_card = Some(bounds);
            self.entering_save_card = true;
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
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
                is_focus_scope: self
                    .focus_scope
                    .as_ref()
                    .is_some_and(|scope| id == Some(scope)),
            });
        }
    }

    fn focusable(&mut self, id: Option<&Id>, target: Rectangle, state: &mut dyn Focusable) {
        if self.apply || self.in_save_card {
            return;
        }
        let focused = state.is_focused();
        if focused && self.ancestors.iter().any(|scroll| scroll.is_focus_scope) {
            self.found_scoped_focus = true;
            self.targets.clear();
        } else if self.found_scoped_focus
            || !self.target.as_ref().map_or_else(
                || self.focus_scope.is_none() && focused,
                |target| id == Some(target),
            )
        {
            return;
        }
        self.targets.push((target, self.ancestors.clone()));
    }

    fn finish(&self) -> operation::Outcome<()> {
        if self.apply {
            return cover_result(self.covered);
        }
        let mut changes = Vec::new();
        let mut covered = false;
        for (target, ancestors) in &self.targets {
            let visible = reveal_changes(*target, ancestors, self.save_card, &mut changes);
            covered |= visible
                .zip(self.save_card)
                .is_some_and(|(target, card)| target.intersects(&card));
        }
        if changes.is_empty() {
            cover_result(covered)
        } else {
            operation::Outcome::Chain(Box::new(Self {
                changes,
                apply: true,
                covered,
                ..Self::default()
            }))
        }
    }
}

fn cover_result(covered: bool) -> operation::Outcome<()> {
    if covered {
        operation::Outcome::Some(())
    } else {
        operation::Outcome::None
    }
}

fn reveal_changes(
    mut target: Rectangle,
    ancestors: &[Scroll],
    save_card: Option<Rectangle>,
    changes: &mut Vec<(usize, Vector)>,
) -> Option<Rectangle> {
    // The card is fixed to the page. Only its outermost vertical scroller moves
    // a nested target clear of it.
    let covered = save_card.and_then(|card| {
        let scroll = ancestors
            .iter()
            .find(|scroll| scroll.content.height > scroll.bounds.height)?;
        Some((scroll.ordinal, scroll.bounds.intersection(&card)?))
    });
    let mut visible = Some(target);
    for scroll in ancestors.iter().rev() {
        target = target - scroll.translation;
        let x = reveal_axis(
            target.x,
            target.width,
            scroll.bounds.x,
            scroll.bounds.width,
            scroll.translation.x,
            scroll.content.width - scroll.bounds.width,
        );
        let visible_height = match covered {
            Some((ordinal, card))
                if ordinal == scroll.ordinal
                    && target.x - x < card.x + card.width
                    && target.x - x + target.width > card.x
                    && target.height <= card.y - scroll.bounds.y =>
            {
                card.y - scroll.bounds.y
            }
            _ => scroll.bounds.height,
        };
        let y = reveal_axis(
            target.y,
            target.height,
            scroll.bounds.y,
            visible_height,
            scroll.translation.y,
            scroll.content.height - scroll.bounds.height,
        );
        let delta = Vector::new(x, y);
        target = target - delta;
        visible = visible
            .and_then(|rect| (rect - scroll.translation - delta).intersection(&scroll.bounds));
        if delta != Vector::ZERO {
            changes.push((scroll.ordinal, delta));
        }
    }
    visible
}

fn reveal_axis(
    start: f32,
    size: f32,
    viewport: f32,
    visible: f32,
    offset: f32,
    max_offset: f32,
) -> f32 {
    let delta = (start + size - viewport - visible)
        .max(0.0)
        .min(start - viewport);
    (offset + delta).clamp(0.0, max_offset.max(0.0)) - offset
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
            draw_ring(renderer, theme, bounds);
        }
    }
}

pub(super) fn draw_ring(renderer: &mut Renderer, theme: &Theme, mut bounds: Rectangle) {
    let palette = theme.palette();
    let mut ring = |bounds, color| {
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: iced::Border {
                    color,
                    width: 2.0,
                    radius: super::design::CONTROL_RADIUS.into(),
                },
                ..Default::default()
            },
            iced::Color::TRANSPARENT,
        );
    };
    if super::design::is_contrast(theme) && bounds.width > 8.0 && bounds.height > 8.0 {
        let inset = |bounds: Rectangle| Rectangle {
            x: bounds.x + 2.0,
            y: bounds.y + 2.0,
            width: bounds.width - 4.0,
            height: bounds.height - 4.0,
        };
        bounds = inset(bounds);
        ring(bounds, palette.background);
        bounds = inset(bounds);
    }
    ring(bounds, palette.text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{design, widgets};
    use iced::advanced::{layout, widget::Tree, Layout};
    use iced::{Element, Size};

    fn run(
        view: &mut Element<'_, ()>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &Renderer,
        mut operation: Box<dyn Operation>,
    ) -> bool {
        loop {
            view.as_widget_mut()
                .operate(tree, Layout::new(node), renderer, operation.as_mut());
            match operation.finish() {
                operation::Outcome::Chain(next) => operation = next,
                operation::Outcome::Some(()) => return true,
                operation::Outcome::None => return false,
            }
        }
    }

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
    fn reveals_nested_controls_by_focus_or_id_without_scrolling_siblings() {
        use crate::ui::{navigation, navigation_flyout, Page};

        const FOCUS_SCOPE: &str = "test-focus-scope";

        #[derive(Default)]
        struct Snapshot {
            count: usize,
            focused: Option<(usize, Rectangle)>,
            marked: Option<Rectangle>,
            scrolls: Vec<(Rectangle, Vector)>,
        }
        impl Operation for Snapshot {
            fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
                visit(self);
            }
            fn focusable(&mut self, id: Option<&Id>, bounds: Rectangle, state: &mut dyn Focusable) {
                if state.is_focused() {
                    self.focused = Some((self.count, bounds));
                }
                if id == Some(&navigation::rail_id(Some(Page::About))) {
                    self.marked = Some(bounds);
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
                assert!(id.is_none() || id == Some(&Id::from(FOCUS_SCOPE)));
                self.scrolls.push((bounds, translation));
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
                navigation_flyout::view(
                    Page::About,
                    navigation::rail_item(
                        Page::About,
                        true,
                        navigation::Event::Select(Page::About)
                    ),
                    None,
                    false,
                )
                .map(|_| ()),
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
            .id(FOCUS_SCOPE)
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
            let before = snapshot(&mut view, &mut tree, &node, &renderer);
            run(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                Box::new(Reveal {
                    target: Some(navigation::rail_id(Some(Page::About))),
                    focus_scope: Some(FOCUS_SCOPE.into()),
                    ..Reveal::default()
                }),
            );
            let after = snapshot(&mut view, &mut tree, &node, &renderer);
            assert_eq!(after.focused, before.focused);
            assert_eq!(after.scrolls[0], before.scrolls[0]);
            let (outer, offset) = after.scrolls[1];
            let (mut inner, inner_offset) = after.scrolls[2];
            let mut target = after.marked.unwrap();
            target.y -= offset.y + inner_offset.y;
            inner.y -= offset.y;
            let visible = outer.intersection(&inner).unwrap();
            assert!(target.y >= visible.y);
            assert!(target.y + target.height <= visible.y + visible.height);

            run(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                Box::new(operation::focusable::focus_next::<()>()),
            );
            let before = snapshot(&mut view, &mut tree, &node, &renderer);
            run(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                Box::new(Reveal {
                    target: Some(navigation::rail_id(Some(Page::About))),
                    focus_scope: Some(FOCUS_SCOPE.into()),
                    ..Reveal::default()
                }),
            );
            let after = snapshot(&mut view, &mut tree, &node, &renderer);
            assert_eq!(after.focused, before.focused);
            assert_eq!(after.focused.unwrap().0, 1);
            assert_eq!(after.scrolls[0], before.scrolls[0]);
            assert_eq!(after.scrolls[2].1.y, 0.0);
        }
    }

    #[test]
    fn focus_moves_the_save_card_when_natural_scrolling_leaves_the_control_covered() {
        use iced::widget::{column, container, row, stack, text, Space};
        use iced::{Bottom, Fill, Right};

        let renderer = Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let size = Size::new(500.0, 180.0);
        for (remaining_height, expected_offset, should_move) in
            [(0, 0.0, true), (50, 20.0, true), (100, 50.0, false)]
        {
            let other_pane = crate::ui::scrolling::scrollable(column![
                Space::new().height(118),
                widgets::action_button("Other pane").on_press(()),
                Space::new().height(100),
            ])
            .width(120)
            .height(Fill);
            let mut body = column![
                Space::new().height(118),
                row![
                    widgets::action_button("Left action").on_press(()),
                    Space::new().width(Fill),
                    widgets::action_button("Right action").on_press(()),
                ],
            ];
            if remaining_height > 0 {
                body = body.push(container(text("More settings")).height(remaining_height));
            }
            let editor = crate::ui::scrolling::scrollable(body)
                .width(Fill)
                .height(Fill);
            let card = container(
                container(widgets::action_button("Save").on_press(()))
                    .center_x(180)
                    .center_y(80)
                    .id(SAVE_CARD),
            )
            .width(Fill)
            .height(Fill)
            .align_x(Right)
            .align_y(Bottom);
            let mut view: Element<'_, ()> = stack![row![other_pane, editor], card].into();
            let mut tree = Tree::new(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            for index in 0..4 {
                run(
                    &mut view,
                    &mut tree,
                    &node,
                    &renderer,
                    Box::new(operation::focusable::focus_next::<()>()),
                );
                let moved = run(
                    &mut view,
                    &mut tree,
                    &node,
                    &renderer,
                    Box::new(Reveal::default()),
                );
                let mut snapshot = Reveal::default();
                view.as_widget_mut().operate(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &mut snapshot,
                );
                assert_eq!(moved, index == 2 && should_move);
                if index == 3 {
                    assert!(snapshot.targets.is_empty());
                } else {
                    let card = snapshot.save_card.unwrap();
                    let (target, scrolls) = &snapshot.targets[0];
                    let offset = scrolls[0].translation.y;
                    assert_eq!(offset, if index == 2 { expected_offset } else { 0.0 });
                    let screen_bounds = *target - Vector::new(0.0, offset);
                    assert_eq!(screen_bounds.intersects(&card), moved);
                }
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
