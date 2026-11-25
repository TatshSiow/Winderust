//! Small control transitions; page changes and layout changes are immediate.
use super::keyboard_focus::Focus;
use iced::advanced::{
    layout, renderer,
    widget::{operation::Focusable, tree, Id, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};
use iced::{
    keyboard::{key::Named, Modifiers},
    mouse, overlay, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

static ENABLED: AtomicBool = AtomicBool::new(false);
pub(super) fn set_enabled(value: bool) {
    ENABLED.store(value, Ordering::Relaxed);
}
pub(super) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub(super) enum Effect {
    Hover(std::rc::Rc<std::cell::Cell<f32>>),
    Control(std::rc::Rc<std::cell::Cell<f32>>),
    Checkbox {
        blend: std::rc::Rc<std::cell::Cell<f32>>,
        interactive: bool,
    },
    Switch {
        interactive: bool,
    },
    Chevron,
    DisclosureChevron,
    Visible,
    // Keep independent widget state when changing pages or tabs.
    Content(u64),
}
struct State {
    animation: iced_anim::Transition<f32>,
    content_key: Option<u64>,
    focus: Focus,
}
pub(super) fn animation(value: bool) -> iced_anim::Transition<f32> {
    iced_anim::Transition::new(f32::from(value))
        .with_easing(iced_anim::Easing::EASE_OUT.with_duration(Duration::from_millis(150)))
}
impl State {
    fn value(&self) -> f32 {
        *self.animation.value()
    }
}
struct Motion<'a, M> {
    content: Element<'a, M>,
    target: bool,
    effect: Effect,
    action: Option<KeyboardAction<'a, M>>,
}
pub(super) type Action<'a, M> = Box<dyn Fn() -> M + 'a>;
type KeyHandler<'a, M> = Box<dyn Fn(Named, Modifiers, Rectangle) -> Option<(M, bool)> + 'a>;

enum KeyboardAction<'a, M> {
    Activate {
        id: Option<Id>,
        handle: Action<'a, M>,
    },
    Keys {
        id: Id,
        handle: KeyHandler<'a, M>,
    },
}

pub(super) fn wrap<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    target: bool,
    effect: Effect,
) -> Element<'a, M> {
    wrap_action(content, target, effect, None, None)
}

pub(super) fn wrap_action<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    target: bool,
    effect: Effect,
    action: Option<Action<'a, M>>,
    focus_id: Option<&'static str>,
) -> Element<'a, M> {
    Element::new(Motion {
        content: content.into(),
        target,
        effect,
        action: action.map(|handle| KeyboardAction::Activate {
            id: focus_id.map(Id::new),
            handle,
        }),
    })
}

pub(super) fn wrap_keys<'a, M: 'a>(
    content: Element<'a, M>,
    id: &'static str,
    enabled: bool,
    handle: impl Fn(Named, Modifiers, Rectangle) -> Option<(M, bool)> + 'a,
) -> Element<'a, M> {
    Element::new(Motion {
        content,
        target: true,
        effect: Effect::Visible,
        action: enabled.then(|| KeyboardAction::Keys {
            id: Id::new(id),
            handle: Box::new(handle),
        }),
    })
}
impl<M> Motion<'_, M> {
    fn animated(&self) -> bool {
        matches!(
            self.effect,
            Effect::Hover(_)
                | Effect::Control(_)
                | Effect::Checkbox { .. }
                | Effect::Switch { .. }
                | Effect::Chevron
                | Effect::DisclosureChevron
        )
    }
    fn hidden(&self) -> bool {
        matches!(self.effect, Effect::Visible) && !self.target
    }
}
impl<M> Widget<M, Theme, Renderer> for Motion<'_, M> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State {
            animation: animation(self.target),
            content_key: if let Effect::Content(key) = self.effect {
                Some(key)
            } else {
                None
            },
            focus: Focus::default(),
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if self.action.is_none() {
            state.focus.unfocus();
        }
        if let Effect::Content(key) = self.effect {
            if state.content_key != Some(key) {
                tree.children[0] = Tree::new(&self.content);
                state.content_key = Some(key);
            } else {
                tree.children[0].diff(&self.content);
            }
        } else {
            tree.children[0].diff(&self.content);
        }
        if matches!(self.effect, Effect::Hover(_)) {
            return;
        }
        if enabled() && self.animated() {
            state.animation.set_target(f32::from(self.target));
        } else {
            state.animation.settle_at(f32::from(self.target));
        }
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        if self.hidden() {
            return layout::Node::with_children(Size::ZERO, vec![layout::Node::new(Size::ZERO)]);
        }
        let child = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        layout::Node::with_children(child.size(), vec![child])
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
        use iced::advanced::Renderer as _;
        if self.hidden() {
            return;
        }
        let state = tree.state.downcast_ref::<State>();
        let progress = state.value();
        let bounds = layout.bounds();
        let child = layout.children().next().unwrap();
        let quarter_turns = match self.effect {
            Effect::Chevron => Some(progress),
            Effect::DisclosureChevron => Some(1.0 + 2.0 * progress),
            _ => None,
        };
        if let Some(quarter_turns) = quarter_turns {
            super::assets::draw_icon(
                renderer,
                "icons/chevron-right.svg",
                bounds,
                super::widgets::muted_color(theme),
                quarter_turns,
            );
            return;
        }
        if let Effect::Hover(blend) | Effect::Control(blend) | Effect::Checkbox { blend, .. } =
            &self.effect
        {
            blend.set(progress);
        }
        if let Effect::Switch { interactive } = &self.effect {
            use iced::widget::toggler::Status;
            let status = |is_toggled| {
                if !*interactive {
                    Status::Disabled { is_toggled }
                } else if cursor.is_over(bounds) {
                    Status::Hovered { is_toggled }
                } else {
                    Status::Active { is_toggled }
                }
            };
            let a = super::widgets::toggle_style(theme, status(false));
            let b = super::widgets::toggle_style(theme, status(true));
            let mut visual = b;
            visual.background =
                super::animated_controls::background(a.background, b.background, progress);
            visual.foreground =
                super::animated_controls::background(a.foreground, b.foreground, progress);
            visual.background_border_width = a.background_border_width
                + (b.background_border_width - a.background_border_width) * progress;
            visual.background_border_color = super::animated_controls::color(
                a.background_border_color,
                b.background_border_color,
                progress,
            );
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::Border {
                        radius: (bounds.height / 2.0).into(),
                        width: visual.background_border_width,
                        color: visual.background_border_color,
                    },
                    ..Default::default()
                },
                visual.background,
            );
            let padding = (visual.padding_ratio * bounds.height).round();
            let knob = Rectangle {
                x: bounds.x + padding + progress * (bounds.width - bounds.height),
                y: bounds.y + padding,
                width: bounds.height - 2.0 * padding,
                height: bounds.height - 2.0 * padding,
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds: knob,
                    border: iced::Border {
                        radius: (knob.height / 2.0).into(),
                        width: visual.foreground_border_width,
                        color: visual.foreground_border_color,
                    },
                    ..Default::default()
                },
                visual.foreground,
            );
        } else if let Effect::Checkbox { interactive, .. } = self.effect {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                child,
                cursor,
                viewport,
            );
            let checkbox = child.children().next().unwrap().bounds();
            let status = if !interactive {
                iced::widget::checkbox::Status::Disabled { is_checked: true }
            } else if cursor.is_over(child.bounds()) {
                iced::widget::checkbox::Status::Hovered { is_checked: true }
            } else {
                iced::widget::checkbox::Status::Active { is_checked: true }
            };
            let color = super::widgets::checkbox_style(theme, status).icon_color;
            let inset = 2.0;
            super::assets::draw_icon(
                renderer,
                "icons/check.svg",
                Rectangle {
                    x: checkbox.x + inset,
                    y: checkbox.y + inset,
                    width: checkbox.width - 2.0 * inset,
                    height: checkbox.height - 2.0 * inset,
                },
                color.scale_alpha(progress),
                0.0,
            );
        } else {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                child,
                cursor,
                viewport,
            );
        }
        state.focus.draw(renderer, theme, bounds);
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
        if self.hidden() {
            return;
        }
        let state = tree.state.downcast_mut::<State>();
        if let Event::Window(iced::window::Event::RedrawRequested(now)) = event {
            state.animation.tick(*now);
        }
        let target = if matches!(self.effect, Effect::Hover(_)) {
            let hovered = cursor.is_over(layout.bounds())
                && cursor.position().is_some_and(|p| viewport.contains(p));
            state.animation.set_target(f32::from(hovered));
            hovered
        } else {
            self.target
        };
        if !enabled() || !self.animated() {
            state.animation.settle_at(f32::from(target));
        }
        if state.animation.is_animating() {
            shell.request_redraw();
        }
        let keep_pointer_focus = matches!(&self.action, Some(KeyboardAction::Keys { .. }))
            && cursor.position().is_some_and(|position| {
                layout.bounds().contains(position) && viewport.contains(position)
            });
        if !keep_pointer_focus {
            state.focus.clear_on_pointer(event, shell);
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout.children().next().unwrap(),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        match &self.action {
            Some(KeyboardAction::Activate { handle, .. }) => {
                if state.focus.activate(event, shell) {
                    shell.publish(handle());
                }
            }
            Some(KeyboardAction::Keys { handle, .. }) => {
                let key = state.focus.pressed_named_key(event, shell);
                if let Some((message, publish)) = key.and_then(|(key, modifiers, repeat)| {
                    handle(key, modifiers, layout.bounds())
                        .map(|(message, repeatable)| (message, !repeat || repeatable))
                }) {
                    shell.capture_event();
                    if publish {
                        shell.publish(message);
                    }
                }
            }
            None => {}
        }
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        if !self.hidden() {
            if let Some(action) = &self.action {
                operation.focusable(
                    match action {
                        KeyboardAction::Activate { id, .. } => id.as_ref(),
                        KeyboardAction::Keys { id, .. } => Some(id),
                    },
                    layout.bounds(),
                    &mut tree.state.downcast_mut::<State>().focus,
                );
            }
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout.children().next().unwrap(),
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
        if self.hidden() {
            return mouse::Interaction::default();
        }
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout.children().next().unwrap(),
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        if self.hidden() {
            return None;
        }
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout.children().next().unwrap(),
            renderer,
            viewport,
            translation,
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    #[test]
    fn chevron_rotations_render_with_upstream_renderer() {
        use iced::advanced::renderer::Headless as _;
        let mut shots = Vec::new();
        for quarter_turns in [0.0, 0.5, 1.0, 2.0, 3.0] {
            let mut renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
            let bounds = Rectangle::with_size(Size::new(24.0, 24.0));
            super::super::assets::draw_icon(
                &mut renderer,
                "icons/chevron-right.svg",
                bounds,
                iced::Color::WHITE,
                quarter_turns,
            );
            let shot = renderer.screenshot(Size::new(24, 24), 1.0, iced::Color::BLACK);
            assert!(
                shot.as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|pixel| pixel[0] > 0)
                    .count()
                    > 10
            );
            shots.push(shot);
        }
        assert!(shots.windows(2).all(|pair| pair[0] != pair[1]));
    }
    #[test]
    fn layout_changes_are_immediate_and_do_not_schedule_animation() {
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        for effect in [Effect::Visible, Effect::Content(1)] {
            let mut widget = Motion::<()> {
                action: None,
                content: iced::widget::Space::new()
                    .width(Length::Fill)
                    .height(100)
                    .into(),
                target: false,
                effect,
            };
            assert!(!widget.animated());
            let mut tree = Tree::new(&widget as &dyn Widget<(), Theme, Renderer>);
            widget.target = true;
            widget.diff(&mut tree);
            let node = widget.layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(500.0, 500.0)),
            );
            assert_eq!(node.size().height, 100.0);
            let state = tree.state.downcast_ref::<State>();
            assert!(!state.animation.is_animating());
        }
    }
    #[test]
    fn switch_paints_intermediate_knob_positions() {
        let mut renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut widget = Motion::<()> {
            action: None,
            content: iced::widget::toggler(false).size(20).into(),
            target: true,
            effect: Effect::Switch { interactive: true },
        };
        let mut tree = Tree::new(&widget as &dyn Widget<(), Theme, Renderer>);
        tree.state.downcast_mut::<State>().animation = animation(false).to(1.0);
        let start = Instant::now();
        let limits = layout::Limits::new(Size::ZERO, Size::new(100.0, 40.0));
        let node = widget.layout(&mut tree, &renderer, &limits);
        let mut positions = Vec::new();
        for elapsed in [0, 60, 300] {
            if elapsed > 0 {
                tree.state
                    .downcast_mut::<State>()
                    .animation
                    .tick(start + Duration::from_millis(elapsed));
            }
            renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
            widget.draw(
                &tree,
                &mut renderer,
                &Theme::Dark,
                &renderer::Style::default(),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &Rectangle::with_size(Size::new(100.0, 40.0)),
            );
            positions.push(
                renderer
                    .layers()
                    .iter()
                    .flat_map(|layer| &layer.quads)
                    .last()
                    .unwrap()
                    .0
                    .bounds
                    .x,
            );
        }
        assert!(positions[0] < positions[1] && positions[1] < positions[2]);
    }
    #[test]
    fn checkbox_paints_intermediate_check_opacity() {
        use iced::advanced::renderer::Headless as _;
        let mut renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut widget: Element<'_, ()> = super::super::animated_controls::Checkbox::new(false)
            .on_toggle(|_| ())
            .into();
        let mut tree = Tree::new(&widget);
        tree.state.downcast_mut::<State>().animation = animation(false).to(1.0);
        let start = Instant::now();
        let node = widget.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(100.0, 40.0)),
        );
        let mut check_contrasts = Vec::new();
        for elapsed in [0, 60, 300] {
            if elapsed > 0 {
                tree.state
                    .downcast_mut::<State>()
                    .animation
                    .tick(start + Duration::from_millis(elapsed));
            }
            renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
            widget.as_widget().draw(
                &tree,
                &mut renderer,
                &Theme::Dark,
                &renderer::Style::default(),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &Rectangle::with_size(Size::new(100.0, 40.0)),
            );
            let size = super::super::design::CHECKBOX_SIZE;
            let shot = renderer.screenshot(Size::new(size, size), 1.0, iced::Color::BLACK);
            // The checkbox fill animates too. Compare the check against its own interior.
            let interior = shot
                .as_chunks::<4>()
                .0
                .chunks_exact(size as usize)
                .skip(3)
                .take((size - 6) as usize)
                .flat_map(|row| row.iter().skip(3).take((size - 6) as usize))
                .map(|pixel| pixel[0]);
            let low = interior.clone().min().unwrap();
            let high = interior.max().unwrap();
            check_contrasts.push(high - low);
        }
        assert_eq!(check_contrasts[0], 0);
        assert!(check_contrasts[0] < check_contrasts[1] && check_contrasts[1] < check_contrasts[2]);
    }
    #[test]
    fn transition_retargets_without_jumping_and_can_settle_immediately() {
        let mut value = animation(false).to(1.0);
        value.tick(Instant::now() + Duration::from_millis(60));
        let before = *value.value();
        assert!(before > 0.0 && before < 1.0);
        value.set_target(0.0);
        assert_eq!(*value.value(), before);
        value.tick(Instant::now() + Duration::from_secs(1));
        assert_eq!(*value.value(), 0.0);
        assert!(!value.is_animating());
        value.set_target(1.0);
        value.settle_at(1.0);
        assert_eq!(*value.value(), 1.0);
        assert!(!value.is_animating());
    }
    #[test]
    fn changing_tabs_does_not_inherit_expanded_cards() {
        let card = |expanded| wrap::<()>(iced::widget::Space::new(), expanded, Effect::Visible);
        let mut widget = Motion {
            action: None,
            content: card(true),
            target: true,
            effect: Effect::Content(0),
        };
        let mut tree = Tree::new(&widget as &dyn Widget<(), Theme, Renderer>);
        widget.content = card(false);
        widget.effect = Effect::Content(1);
        widget.diff(&mut tree);
        let child = tree.children[0].state.downcast_ref::<State>();
        assert_eq!(*child.animation.value(), 0.0);
        assert!(!child.animation.is_animating());
    }
}
