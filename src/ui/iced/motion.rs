//! Local widget-tree animations. No global identifiers or recurring subscriptions.
use iced::advanced::Renderer as _;
use iced::advanced::{
    layout, renderer,
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};
use iced::{
    animation, mouse, overlay, window, Animation, Element, Event, Length, Rectangle, Renderer,
    Size, Theme, Vector,
};
use std::time::{Duration, Instant};

pub(super) fn reveal<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    expanded: bool,
    enabled: bool,
) -> Element<'a, Message> {
    Element::new(Reveal {
        content: content.into(),
        expanded,
        enabled,
        on_hidden: None,
        mode: Mode::Vertical,
    })
}
/// Retain a visual clone until `on_removed`; commit the model deletion immediately.
pub(super) fn removal<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    removing: bool,
    enabled: bool,
    on_removed: Message,
) -> Element<'a, Message> {
    Element::new(Reveal {
        content: content.into(),
        expanded: !removing,
        enabled,
        on_hidden: Some(on_removed),
        mode: Mode::Vertical,
    })
}
/// Animate a retained panel between its full and compact widths.
pub(super) fn horizontal_reveal<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    expanded: bool,
    enabled: bool,
    expanded_width: f32,
    collapsed_width: f32,
) -> Element<'a, Message> {
    Element::new(Reveal {
        content: content.into(),
        expanded,
        enabled,
        on_hidden: None,
        mode: Mode::Horizontal {
            expanded_width,
            collapsed_width,
        },
    })
}

/// A short slide into place when the page identity changes, without changing layout.
pub(super) fn page_transition<'a, Message: Clone + 'a>(
    key: u64,
    content: impl Into<Element<'a, Message>>,
    enabled: bool,
) -> Element<'a, Message> {
    Element::new(Reveal {
        content: content.into(),
        expanded: true,
        enabled,
        on_hidden: None,
        mode: Mode::Entrance(key),
    })
}
#[derive(Clone, Copy)]
enum Mode {
    Vertical,
    Horizontal {
        expanded_width: f32,
        collapsed_width: f32,
    },
    Entrance(u64),
}
struct Reveal<'a, Message> {
    content: Element<'a, Message>,
    expanded: bool,
    enabled: bool,
    on_hidden: Option<Message>,
    mode: Mode,
}
struct State {
    animation: Animation<bool>,
    completed: bool,
    was_animating: bool,
    page_key: Option<u64>,
}
impl State {
    fn new(expanded: bool) -> Self {
        Self {
            animation: transition(expanded),
            completed: false,
            was_animating: false,
            page_key: None,
        }
    }
    fn reconcile(&mut self, expanded: bool, enabled: bool, now: Instant) {
        if !enabled {
            self.animation = transition(expanded);
        } else if self.animation.value() != expanded {
            self.animation.go_mut(expanded, now);
            self.was_animating = true;
            self.completed = false;
        }
    }
    fn progress(&self, now: Instant) -> f32 {
        self.animation.interpolate(0.0, 1.0, now)
    }
}
fn transition(expanded: bool) -> Animation<bool> {
    Animation::new(expanded)
        .duration(Duration::from_millis(180))
        .easing(animation::Easing::EaseOut)
}
impl<Message: Clone> Widget<Message, Theme, Renderer> for Reveal<'_, Message> {
    fn size(&self) -> Size<Length> {
        let size = self.content.as_widget().size();
        match self.mode {
            Mode::Vertical => Size::new(size.width, Length::Shrink),
            Mode::Horizontal { .. } => Size::new(Length::Shrink, size.height),
            Mode::Entrance(_) => size,
        }
    }
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        let mut state = State::new(self.expanded);
        if let Mode::Entrance(key) = self.mode {
            state.page_key = Some(key);
            if self.enabled {
                state.animation = transition(false);
                state.reconcile(true, true, Instant::now());
            }
        }
        tree::State::new(state)
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
        let state = tree.state.downcast_mut::<State>();
        if let Mode::Entrance(key) = self.mode {
            if state.page_key != Some(key) {
                state.page_key = Some(key);
                state.animation = transition(false);
            }
        }
        state.reconcile(self.expanded, self.enabled, Instant::now());
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let child_limits = match self.mode {
            Mode::Horizontal { expanded_width, .. } => limits.loose().width(expanded_width),
            _ => limits.loose(),
        };
        let mut child =
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, &child_limits);
        let size = child.size();
        let progress = tree.state.downcast_ref::<State>().progress(Instant::now());
        let outer = match self.mode {
            Mode::Vertical => Size::new(size.width, size.height * progress),
            Mode::Horizontal {
                expanded_width,
                collapsed_width,
            } => Size::new(
                collapsed_width + (expanded_width - collapsed_width) * progress,
                size.height,
            ),
            Mode::Entrance(_) => {
                child.move_to_mut(iced::Point::new(12.0 * (1.0 - progress), 0.0));
                size
            }
        };
        layout::Node::with_children(outer, vec![child])
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
        if let Some(clip) = layout
            .bounds()
            .intersection(viewport)
            .filter(|clip| clip.height > 0.0)
        {
            renderer.with_layer(clip, |renderer| {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    layout
                        .children()
                        .next()
                        .expect("Reveal always lays out its content"),
                    cursor,
                    &clip,
                )
            });
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
        let state = tree.state.downcast_mut::<State>();
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            let animating = state.animation.is_animating(*now);
            if animating || state.was_animating {
                shell.invalidate_layout();
            }
            state.was_animating = animating;
            if animating {
                shell.request_redraw();
            } else if !self.expanded && !state.completed {
                state.completed = true;
                shell.invalidate_layout();
                if let Some(message) = &self.on_hidden {
                    shell.publish(message.clone());
                }
            }
        }
        if self.expanded
            || matches!(self.mode, Mode::Horizontal { collapsed_width, .. } if collapsed_width > 0.0)
        {
            if let Some(clip) = layout.bounds().intersection(viewport) {
                let cursor = if cursor.is_over(clip) {
                    cursor
                } else {
                    mouse::Cursor::Unavailable
                };
                self.content.as_widget_mut().update(
                    &mut tree.children[0],
                    event,
                    layout
                        .children()
                        .next()
                        .expect("Reveal always lays out its content"),
                    cursor,
                    renderer,
                    clipboard,
                    shell,
                    &clip,
                );
            }
        }
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        if self.expanded
            || matches!(self.mode, Mode::Horizontal { collapsed_width, .. } if collapsed_width > 0.0)
        {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout
                    .children()
                    .next()
                    .expect("Reveal always lays out its content"),
                renderer,
                operation,
            );
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
        if (self.expanded
            || matches!(self.mode, Mode::Horizontal { collapsed_width, .. } if collapsed_width > 0.0))
            && cursor.is_over(layout.bounds())
        {
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                layout
                    .children()
                    .next()
                    .expect("Reveal always lays out its content"),
                cursor,
                viewport,
                renderer,
            )
        } else {
            mouse::Interaction::None
        }
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        if self.expanded
            || matches!(self.mode, Mode::Horizontal { collapsed_width, .. } if collapsed_width > 0.0)
        {
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                layout
                    .children()
                    .next()
                    .expect("Reveal always lays out its content"),
                renderer,
                viewport,
                translation,
            )
        } else {
            None
        }
    }
}
/// Stable local identity for keyed widget trees; no retained registry.
pub(super) fn key(value: &impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hash);
    hash.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reversing_reveal_is_continuous_and_disabled_motion_snaps() {
        let now = Instant::now();
        let mut state = State::new(true);
        state.reconcile(false, true, now);
        let halfway = now + Duration::from_millis(90);
        let progress = state.progress(halfway);
        assert!(progress > 0.0 && progress < 1.0);
        state.reconcile(true, true, halfway);
        assert!((state.progress(halfway) - progress).abs() < 0.01);
        state.reconcile(false, false, halfway);
        assert_eq!(state.progress(halfway), 0.0);
        assert!(!state.animation.is_animating(halfway));
    }
}
