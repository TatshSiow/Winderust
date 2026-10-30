use super::{overlay_host, ViewPart, ViewState};
use windui::{
    core::{NodeId, Tree},
    event::{Key, KeyEvent, MouseButton, PointerEvent, PointerKind},
    prelude::{signal, Align, Element, Point, Signal, Size},
    signal::SignalScope,
    style::Role,
    text::NullTextEngine,
};

struct Shell {
    tree: Tree,
    navigation: NodeId,
    page_action: NodeId,
    footer_action: NodeId,
    revision: Signal<Vec<u64>>,
    dialog_open: Signal<bool>,
    calls: Signal<[usize; 3]>,
    dialog_closes: Signal<usize>,
    hover: Option<NodeId>,
    capture: Option<NodeId>,
    _scope: SignalScope,
}

impl Shell {
    fn new() -> Self {
        let mut scope = SignalScope::new();
        let (revision, dialog_open, calls, dialog_closes) = scope.collect(|| {
            (
                signal(vec![0_u64]),
                signal(false),
                signal([0_usize; 3]),
                signal(0_usize),
            )
        });
        let mut tree = Tree::new();
        let root = scope.collect(|| {
            Element::stack()
                .fill()
                .child(
                    Element::col()
                        .fill()
                        .padding(12)
                        .spacing(12)
                        .child(
                            Element::row()
                                .width(280)
                                .height(40)
                                .cross(Align::Center)
                                .padding_xy(10, 0)
                                .spacing(10)
                                .bg_role(Role::SurfaceAlt)
                                .child(
                                    Element::image_svg(
                                        br##"<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18"><rect width="18" height="18" fill="#000"/></svg>"##,
                                        None,
                                    )
                                    .size(18, 18),
                                )
                                .child(Element::label("Power Plan Control").weight(1.0))
                                .child(Element::badge("3"))
                                .clickable()
                                .on_click(move |_| calls.update(|value| value[0] += 1)),
                        )
                        .child(
                            Element::button("Page action")
                                .width(180)
                                .on_click(move |_| calls.update(|value| value[1] += 1)),
                        )
                        .child(Element::col().weight(1.0))
                        .child(
                            Element::button("Footer action")
                                .width(180)
                                .on_click(move |_| calls.update(|value| value[2] += 1)),
                        ),
                )
                .child(overlay_host(revision, move || {
                    dialog_open.get().then(|| {
                        Element::dialog(
                            dialog_open,
                            Element::col()
                                .width(240)
                                .padding(16)
                                .bg_role(Role::Surface)
                                .child(Element::button("Close dialog").on_click(move |_| {
                                    dialog_closes.update(|value| *value += 1);
                                    dialog_open.set(false);
                                    revision.update(|value| value[0] += 1);
                                })),
                        )
                    })
                }))
                .build(&mut tree)
        });
        tree.root = Some(root);
        let body = tree.get(root).unwrap().children[0];
        let children = tree.get(body).unwrap().children.clone();
        let mut shell = Self {
            tree,
            navigation: children[0],
            page_action: children[1],
            footer_action: children[3],
            revision,
            dialog_open,
            calls,
            dialog_closes,
            hover: None,
            capture: None,
            _scope: scope,
        };
        shell.layout();
        shell
    }

    fn layout(&mut self) {
        self.tree
            .layout_root(Size::new(600, 400), &mut NullTextEngine);
    }

    fn center(&self, id: NodeId) -> Point {
        let bounds = self.tree.abs_bounds(id);
        assert!(bounds.w > 0 && bounds.h > 0);
        Point::new(bounds.x + bounds.w / 2, bounds.y + bounds.h / 2)
    }

    fn click(&mut self, position: Point) {
        for kind in [PointerKind::Down, PointerKind::Up] {
            self.tree.dispatch_pointer(
                PointerEvent::single(kind, position, MouseButton::Left),
                &mut self.hover,
                &mut self.capture,
            );
        }
        assert!(self.capture.is_none());
    }

    fn click_node(&mut self, id: NodeId) {
        self.click(self.center(id));
    }

    fn assert_hit_within(&self, position: Point, ancestor: NodeId) {
        let hit = self.tree.hit_test(position);
        assert!(
            std::iter::successors(hit, |id| {
                self.tree.get(*id).and_then(|node| node.parent)
            })
            .any(|id| id == ancestor),
            "native hit {hit:?} must belong to {ancestor:?}"
        );
    }
}

#[test]
fn background_refresh_during_navigation_press_keeps_capture_live_and_exit_reachable() {
    for pressed_footer in [false, true] {
        let mut scope = SignalScope::new();
        let (view, calls) = scope.collect(|| (ViewState::new(), signal([0_usize; 2])));
        let mut tree = Tree::new();
        let root = scope.collect(|| {
            Element::col()
                .fill()
                .child(
                    Element::host_signal(view.navigation, move |_| {
                        Element::row()
                            .width_match()
                            .height(40)
                            .child(Element::label("Navigation"))
                            .clickable()
                            .on_click(move |_| calls.update(|value| value[0] += 1))
                    })
                    .height(40)
                    .width_match(),
                )
                .child(Element::flex_spacer())
                .child(
                    Element::host_signal(view.footer, move |_| {
                        Element::button("Exit")
                            .height(40)
                            .width_match()
                            .on_click(move |_| calls.update(|value| value[1] += 1))
                    })
                    .height(40)
                    .width_match(),
                )
                .build(&mut tree)
        });
        tree.root = Some(root);
        tree.layout_root(Size::new(400, 160), &mut NullTextEngine);
        let controls = tree.focusable_order();
        assert_eq!(controls.len(), 2);
        let pressed = controls[usize::from(pressed_footer)];
        let bounds = tree.abs_bounds(pressed);
        let point = Point::new(bounds.x + bounds.w / 2, bounds.y + bounds.h / 2);
        let (mut hover, mut capture) = (None, None);
        tree.dispatch_pointer(
            PointerEvent::single(PointerKind::Down, point, MouseButton::Left),
            &mut hover,
            &mut capture,
        );
        assert_eq!(capture, Some(pressed));
        for _ in 0..3 {
            view.invalidate(ViewPart::Navigation);
            view.invalidate(ViewPart::Footer);
            view.flush(|| capture.is_some());
            tree.layout_root(Size::new(400, 160), &mut NullTextEngine);
            assert!(
                tree.get(pressed).is_some(),
                "Refresh removed a pressed control"
            );
            assert_eq!(tree.focusable_order(), controls);
        }
        tree.dispatch_pointer(
            PointerEvent::single(PointerKind::Up, point, MouseButton::Left),
            &mut hover,
            &mut capture,
        );
        assert!(capture.is_none());
        view.flush(|| capture.is_some());
        tree.layout_root(Size::new(400, 160), &mut NullTextEngine);
        assert!(
            tree.get(pressed).is_none(),
            "Deferred refresh did not drain"
        );
        assert_eq!(view.navigation.get(), vec![1]);
        assert_eq!(view.footer.get(), vec![1]);
        assert_eq!(view.pending_refresh.get(), 0);
        for control in tree.focusable_order() {
            let bounds = tree.abs_bounds(control);
            let point = Point::new(bounds.x + bounds.w / 2, bounds.y + bounds.h / 2);
            let before = calls.get();
            for kind in [PointerKind::Down, PointerKind::Up] {
                tree.dispatch_pointer(
                    PointerEvent::single(kind, point, MouseButton::Left),
                    &mut hover,
                    &mut capture,
                );
            }
            assert!(capture.is_none());
            assert_ne!(calls.get(), before, "Later navigation/Exit click was lost");
        }
        assert_eq!(calls.get(), if pressed_footer { [1, 2] } else { [2, 1] });
    }
}

#[test]
fn empty_overlay_host_allows_compound_navigation_and_shell_actions() {
    let mut shell = Shell::new();
    let children = shell.tree.get(shell.navigation).unwrap().children.clone();
    // Image, label and badge are native hit targets; each event must bubble to
    // the clickable row rather than landing on the empty full-window host.
    for (index, child) in children.into_iter().enumerate() {
        let position = shell.center(child);
        shell.assert_hit_within(position, child);
        shell.click(position);
        assert_eq!(shell.calls.get()[0], index + 1);
    }
    let bounds = shell.tree.abs_bounds(shell.navigation);
    let padding = Point::new(bounds.x + 2, bounds.y + bounds.h / 2);
    assert_eq!(shell.tree.hit_test(padding), Some(shell.navigation));
    shell.click(padding);
    assert_eq!(shell.calls.get()[0], 4);

    shell.tree.set_focused(Some(shell.navigation), None);
    let focused = shell.tree.focused_node();
    let result = shell.tree.dispatch_key(
        KeyEvent {
            key: Key::Enter,
            pressed: true,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        },
        focused,
    );
    assert!(result.consumed);
    assert_eq!(shell.calls.get()[0], 5);

    shell.click_node(shell.page_action);
    shell.click_node(shell.footer_action);
    assert_eq!(shell.calls.get(), [5, 1, 1]);
}

#[test]
fn overlay_host_blocks_only_while_dialog_exists_across_reopen_cycles() {
    let mut shell = Shell::new();
    let label = shell.tree.get(shell.navigation).unwrap().children[1];
    for cycle in 0..3 {
        shell.dialog_open.set(true);
        shell.revision.update(|value| value[0] += 1);
        shell.layout();
        let before = shell.calls.get();
        shell.click_node(label);
        shell.click_node(shell.page_action);
        shell.click_node(shell.footer_action);
        assert_eq!(shell.calls.get(), before);
        assert!(shell.dialog_open.get());

        let modal_controls = shell.tree.focusable_order();
        assert_eq!(modal_controls.len(), 1);
        shell.click_node(modal_controls[0]);
        assert_eq!(shell.dialog_closes.get(), cycle + 1);
        assert!(!shell.dialog_open.get());
        shell.layout();

        // Rebuilding the overlay must neither replace nor block the shell.
        assert_eq!(shell.tree.hit_test(shell.center(label)), Some(label));
        shell.click_node(label);
        shell.click_node(shell.page_action);
        shell.click_node(shell.footer_action);
        assert_eq!(shell.calls.get(), [cycle + 1; 3]);
    }
}
