//! Nonvisual viewport binding; Wind UI retains layout, input and painting.

use windui::{
    core::{EventCtx, Widget},
    prelude::{signal, Element, Signal},
};

// Expanded navigation + status rail + a usable editor, in logical pixels.
const COMPACT_BELOW: i32 =
    super::design::NAVIGATION_WIDTH as i32 + super::design::SIDE_PANEL_WIDTH as i32 + 480;

pub(super) fn bind(is_compact: Signal<bool>, width: Signal<i32>) -> Element {
    Element::leaf()
        .size(0, 0)
        .widget(Viewport { is_compact, width })
        .reactive()
}

/// Change Home's column count only at its breakpoint, retaining controls on other resizes.
pub(super) fn home(
    width: Signal<i32>,
    navigation_collapsed: bool,
    build: impl Fn(bool) -> Element + 'static,
) -> Element {
    let navigation = if navigation_collapsed {
        super::design::SIDEBAR_COLLAPSED_WIDTH
    } else {
        super::design::NAVIGATION_WIDTH
    } as i32;
    let is_wide = move |width| width - navigation - 24 >= 760;
    let wide = signal(is_wide(width.get()));
    super::widgets::observe(
        Element::host_signal(wide.map(|wide| vec![*wide]), build),
        width,
        move |width| {
            let next = is_wide(width);
            if wide.get() != next {
                wide.set(next);
            }
        },
    )
    .fill()
}

struct Viewport {
    is_compact: Signal<bool>,
    width: Signal<i32>,
}

impl Widget for Viewport {
    fn on_update(&mut self, ctx: &mut EventCtx) {
        // Wind UI records the current logical client size before reactive
        // updates. Node bounds still describe the previous layout here.
        let width = ctx.tree_mut().layout_size.w;
        if self.width.get() != width
            && crate::platform::windows::resident_shell::ui_pointer_capture_active()
        {
            // Breakpoint hosts must not dispose a control during its native press.
            return;
        }
        if self.width.get() != width {
            self.width.set(width);
        }
        let compact = width < COMPACT_BELOW;
        if self.is_compact.get() != compact {
            self.is_compact.set(compact);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    use windui::{
        core::Tree,
        event::{Key, KeyEvent},
        prelude::{signal, Size},
        signal::SignalScope,
        text::NullTextEngine,
    };

    #[test]
    fn home_retains_focused_controls_until_its_column_count_changes() {
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let width = signal(1120);
            let compact = signal(false);
            let builds = Rc::new(RefCell::new(Vec::new()));
            let observed = Rc::clone(&builds);
            let mut tree = Tree::new();
            let root = Element::stack()
                .fill()
                .child(bind(compact, width))
                .child(home(width, false, move |wide| {
                    observed.borrow_mut().push(wide);
                    Element::button(if wide { "Wide" } else { "Narrow" })
                }))
                .build(&mut tree);
            tree.root = Some(root);
            tree.layout_root(Size::new(1120, 760), &mut NullTextEngine);
            let button = tree.focusable_order()[0];
            tree.set_focused(Some(button), None);

            tree.layout_root(Size::new(1200, 760), &mut NullTextEngine);
            assert_eq!(tree.focusable_order()[0], button);
            assert_eq!(tree.focused_node(), Some(button));
            assert_eq!(&*builds.borrow(), &[true]);

            tree.layout_root(Size::new(900, 760), &mut NullTextEngine);
            assert_eq!(&*builds.borrow(), &[true, false]);
            let narrow_button = tree.focusable_order()[0];
            tree.set_focused(Some(narrow_button), None);
            tree.layout_root(Size::new(940, 760), &mut NullTextEngine);
            assert_eq!(tree.focusable_order()[0], narrow_button);
            assert_eq!(tree.focused_node(), Some(narrow_button));
            assert_eq!(&*builds.borrow(), &[true, false]);
        });
    }

    #[test]
    fn native_layout_updates_compact_state_for_the_current_viewport() {
        let mut scope = SignalScope::new();
        let (is_compact, width) = scope.collect(|| (signal(false), signal(0)));
        let mut tree = Tree::new();
        let root = Element::stack()
            .fill()
            .child(bind(is_compact, width))
            .build(&mut tree);
        tree.root = Some(root);
        assert!(!is_compact.get());

        tree.layout_root(Size::new(900, 620), &mut NullTextEngine);
        assert!(is_compact.get());
        assert_eq!(width.get(), 900);
        let narrow_version = is_compact.version();
        let narrow_width_version = width.version();

        tree.layout_root(Size::new(900, 760), &mut NullTextEngine);
        assert!(is_compact.get());
        assert_eq!(is_compact.version(), narrow_version);
        assert_eq!(width.get(), 900);
        assert_eq!(width.version(), narrow_width_version);

        tree.layout_root(Size::new(COMPACT_BELOW, 760), &mut NullTextEngine);
        assert!(!is_compact.get());
        assert_eq!(width.get(), COMPACT_BELOW);
        assert_ne!(width.version(), narrow_width_version);
        let wide_version = is_compact.version();
        let wide_width_version = width.version();

        tree.layout_root(Size::new(1400, 760), &mut NullTextEngine);
        assert!(!is_compact.get());
        assert_eq!(is_compact.version(), wide_version);
        assert_eq!(width.get(), 1400);
        assert_ne!(width.version(), wide_width_version);
        let wider_width_version = width.version();

        tree.layout_root(Size::new(1400, 900), &mut NullTextEngine);
        assert_eq!(width.version(), wider_width_version);

        tree.layout_root(Size::new(COMPACT_BELOW - 1, 760), &mut NullTextEngine);
        assert!(is_compact.get());
        assert_ne!(is_compact.version(), wide_version);
        assert_eq!(width.get(), COMPACT_BELOW - 1);
    }

    #[test]
    fn native_rail_changes_on_resize_without_replacing_the_focused_input() {
        let mut scope = SignalScope::new();
        let (is_compact, width, text) =
            scope.collect(|| (signal(false), signal(0), signal(String::new())));
        let mut tree = Tree::new();
        let rail_width = super::super::design::SIDE_PANEL_WIDTH as i32;
        let compact_width = super::super::design::SIDEBAR_COLLAPSED_WIDTH as i32;
        let root = Element::stack()
            .fill()
            .child(bind(is_compact, width))
            .child(
                Element::row()
                    .fill()
                    .child(Element::text_input(text, "").weight(1.0))
                    .child(
                        Element::col()
                            .width(rail_width)
                            .height_match()
                            .visible_when(move || !is_compact.get()),
                    )
                    .child(
                        Element::col()
                            .width(compact_width)
                            .height_match()
                            .visible_when(move || is_compact.get()),
                    ),
            )
            .build(&mut tree);
        tree.root = Some(root);
        let body = tree.get(root).unwrap().children[1];
        let children = tree.get(body).unwrap().children.clone();
        let (input, wide_rail, narrow_rail) = (children[0], children[1], children[2]);
        tree.set_focused(Some(input), None);

        let type_character = |tree: &mut Tree, value| {
            let focused = tree.focused_node();
            tree.dispatch_key(
                KeyEvent {
                    key: Key::Char(value),
                    pressed: true,
                    ctrl: false,
                    shift: false,
                    alt: false,
                    meta: false,
                },
                focused,
            );
        };

        tree.layout_root(Size::new(900, 620), &mut NullTextEngine);
        assert_eq!(width.get(), 900);
        assert!(tree.get(narrow_rail).unwrap().effective_visible());
        assert!(!tree.get(wide_rail).unwrap().effective_visible());
        assert_eq!(tree.abs_bounds(narrow_rail).w, compact_width);
        assert_eq!(tree.abs_bounds(input).w, 900 - compact_width);
        type_character(&mut tree, 'a');
        assert_eq!(text.get(), "a");

        tree.layout_root(Size::new(1400, 760), &mut NullTextEngine);
        assert_eq!(width.get(), 1400);
        assert!(tree.get(wide_rail).unwrap().effective_visible());
        assert!(!tree.get(narrow_rail).unwrap().effective_visible());
        assert_eq!(tree.abs_bounds(wide_rail).w, rail_width);
        assert_eq!(tree.abs_bounds(input).w, 1400 - rail_width);
        assert_eq!(tree.get(body).unwrap().children[0], input);
        assert_eq!(tree.focused_node(), Some(input));
        assert_eq!(text.get(), "a");
        type_character(&mut tree, 'b');
        assert_eq!(text.get(), "ab");

        tree.layout_root(Size::new(900, 620), &mut NullTextEngine);
        assert_eq!(width.get(), 900);
        assert!(tree.get(narrow_rail).unwrap().effective_visible());
        assert!(!tree.get(wide_rail).unwrap().effective_visible());
        assert_eq!(tree.abs_bounds(narrow_rail).w, compact_width);
        assert_eq!(tree.abs_bounds(input).w, 900 - compact_width);
        assert_eq!(tree.get(body).unwrap().children[0], input);
        assert_eq!(tree.focused_node(), Some(input));
        assert_eq!(text.get(), "ab");
    }
}
