//! Small control transitions; page changes and layout changes are immediate.
use iced::advanced::{
    layout, renderer,
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};
use iced::{mouse, overlay, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};
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
    Underline,
    Width {
        min: f32,
        max: f32,
    },
    Visible,
    // Keep independent widget state when changing pages or tabs.
    Content(u64),
}
struct State {
    animation: iced_anim::Transition<f32>,
    content_key: Option<u64>,
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
}
pub(super) fn wrap<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    target: bool,
    effect: Effect,
) -> Element<'a, M> {
    Element::new(Motion {
        content: content.into(),
        target,
        effect,
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
                | Effect::Underline
        )
    }
    fn hidden(&self) -> bool {
        matches!(self.effect, Effect::Visible) && !self.target
    }
}
impl<M> Widget<M, Theme, Renderer> for Motion<'_, M> {
    fn size(&self) -> Size<Length> {
        let mut size = self.content.as_widget().size();
        if let Effect::Width { min, max } = self.effect {
            size.width = Length::Fixed(if self.target { max } else { min });
        }
        size
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
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
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
        let adjusted = if let Effect::Width { min, max } = self.effect {
            let width = if self.target { max } else { min };
            limits.width(width).min_width(width).max_width(width)
        } else {
            *limits
        };
        let child = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &adjusted);
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
        let progress = tree.state.downcast_ref::<State>().value();
        let bounds = layout.bounds();
        let child = layout.children().next().unwrap();
        if matches!(self.effect, Effect::Chevron) {
            use iced::advanced::svg::Renderer as _;
            renderer.draw_svg(
                iced::advanced::svg::Svg::new(super::assets::chevron_frame(progress))
                    .color(super::widgets::muted_color(theme)),
                bounds,
                *viewport,
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
            return;
        }
        if let Effect::Checkbox { interactive, .. } = self.effect {
            use iced::advanced::svg::Renderer as _;
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
            let status = if interactive {
                iced::widget::checkbox::Status::Active { is_checked: true }
            } else {
                iced::widget::checkbox::Status::Disabled { is_checked: true }
            };
            let color = super::widgets::checkbox_style(theme, status)
                .icon_color
                .scale_alpha(progress);
            let inset = 3.0;
            renderer.draw_svg(
                iced::advanced::svg::Svg::new(
                    super::assets::iced_icon("icons/check.svg").expect("Check icon is bundled"),
                )
                .color(color),
                Rectangle {
                    x: checkbox.x + inset,
                    y: checkbox.y + inset,
                    width: checkbox.width - 2.0 * inset,
                    height: checkbox.height - 2.0 * inset,
                },
                *viewport,
            );
            return;
        }

        if matches!(self.effect, Effect::Underline) {
            let Some(mut clip) = bounds.intersection(viewport) else {
                return;
            };
            clip.width *= progress;
            if clip.width > 0.0 && clip.height > 0.0 {
                renderer.with_layer(clip, |renderer| {
                    self.content.as_widget().draw(
                        &tree.children[0],
                        renderer,
                        theme,
                        style,
                        child,
                        cursor,
                        &clip,
                    )
                });
            }
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
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        if !self.hidden() {
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
    fn chevron_frames_render_with_upstream_renderer() {
        use iced::advanced::{renderer::Headless as _, svg::Renderer as _};
        let mut shots = Vec::new();
        for progress in [0.0, 0.5, 1.0] {
            let mut renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
            let bounds = Rectangle::with_size(Size::new(24.0, 24.0));
            renderer.draw_svg(
                iced::advanced::svg::Svg::new(super::super::assets::chevron_frame(progress))
                    .color(iced::Color::WHITE),
                bounds,
                bounds,
            );
            let shot = renderer.screenshot(Size::new(24, 24), 1.0, iced::Color::BLACK);
            assert!(shot.chunks_exact(4).filter(|pixel| pixel[0] > 0).count() > 10);
            shots.push(shot);
        }
        assert_ne!(shots[0], shots[1]);
        assert_ne!(shots[1], shots[2]);
    }
    #[test]
    fn layout_changes_are_immediate_and_do_not_schedule_animation() {
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        for effect in [
            Effect::Visible,
            Effect::Width {
                min: 56.0,
                max: 320.0,
            },
            Effect::Content(1),
        ] {
            let mut widget = Motion::<()> {
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
            if matches!(widget.effect, Effect::Width { .. }) {
                assert_eq!(node.size().width, 320.0);
            }
            let state = tree.state.downcast_ref::<State>();
            assert!(!state.animation.is_animating());
        }
    }
    #[test]
    fn switch_paints_intermediate_knob_positions() {
        let mut renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut widget = Motion::<()> {
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
        let mut alphas = Vec::new();
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
            let alpha = renderer
                .layers()
                .iter()
                .flat_map(|layer| &layer.images)
                .find_map(|image| {
                    if let iced::advanced::graphics::Image::Vector { svg, .. } = image {
                        svg.color.map(|color| color.a)
                    } else {
                        None
                    }
                })
                .unwrap();
            alphas.push(alpha);
        }
        assert_eq!(alphas[0], 0.0);
        assert!(alphas[0] < alphas[1] && alphas[1] < alphas[2]);
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
