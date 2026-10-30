//! Native scrolling and virtualization; Wind UI owns scroll input and damage tracking.

use std::{cell::Cell, collections::VecDeque, rc::Rc};

use crate::platform::windows::resident_shell::ui_pointer_capture_active;
use windui::{
    core::{EmptyWidget, EventCtx, Layout, NodeId, Tree, Widget},
    prelude::Element,
    spec::Axis,
};

pub(super) fn scrollable(content: Element) -> Element {
    Element::scroll().fill().child(content.width_match())
}

/// Keep native virtual row controls alive until their pointer capture is released.
/// Scroll input and layout remain native; row realization catches up on release.
pub(super) fn preserve_table_rows(table: Element) -> Element {
    preserve_table_rows_while(table, ui_pointer_capture_active)
}

fn preserve_table_rows_while(
    table: Element,
    capture_active: impl Fn() -> bool + 'static,
) -> Element {
    Element::stack()
        .fill()
        .weight(1.0)
        .widget(InstallTableCaptureGuard {
            capture_active: Some(Box::new(capture_active)),
        })
        .reactive()
        .child(table.fill())
}

struct InstallTableCaptureGuard {
    capture_active: Option<Box<dyn Fn() -> bool>>,
}

impl Widget for InstallTableCaptureGuard {
    fn on_update(&mut self, ctx: &mut EventCtx) {
        let Some(capture_active) = self.capture_active.take() else {
            return;
        };
        let root = ctx.id();
        let tree = ctx.tree_mut();
        // Pinned Wind UI 0.28 builds table[col header, divider, scroll[col body]].
        // This parent is registered before the native body's first reactive update.
        let parent = tree
            .get(root)
            .expect("native table guard parent must exist");
        assert_eq!(
            parent.children.len(),
            1,
            "native table guard needs one table"
        );
        let table = tree
            .get(parent.children[0])
            .expect("native table must exist");
        assert_eq!(
            table.children.len(),
            3,
            "Wind UI virtual table shape changed"
        );
        let scroll = tree
            .get(table.children[2])
            .expect("native table scroll must exist");
        assert!(matches!(scroll.layout, Layout::Scroll));
        assert_eq!(scroll.children.len(), 1, "native table needs one body");
        let body = tree
            .get_mut(scroll.children[0])
            .expect("native table body must exist");
        assert!(matches!(
            body.layout,
            Layout::Linear {
                axis: Axis::Vertical,
                ..
            }
        ));
        assert!(
            body.widget.as_any_mut().is_some(),
            "native table body type changed"
        );
        let inner = std::mem::replace(&mut body.widget, Box::new(EmptyWidget));
        body.widget = Box::new(TableCaptureGuard {
            inner,
            capture_active,
        });
    }

    fn hit_opaque(&self) -> bool {
        false
    }
}

struct TableCaptureGuard {
    inner: Box<dyn Widget>,
    capture_active: Box<dyn Fn() -> bool>,
}

// The pinned native VirtualTableBody implements only these two Widget methods.
impl Widget for TableCaptureGuard {
    fn on_update(&mut self, ctx: &mut EventCtx) {
        if !(self.capture_active)() {
            self.inner.on_update(ctx);
        }
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        self.inner.as_any_mut()
    }
}

/// Remember the first native scroll container in a page across structural rebuilds.
/// The application owns one offset cell per page; nested panes keep their own state.
pub(super) fn remember(content: Element, offset: Rc<Cell<i32>>) -> Element {
    Element::stack()
        .fill()
        .widget(ScrollMemory {
            offset,
            scroll: None,
        })
        .reactive()
        .child(content.fill())
}

/// Nonvisual state only. Wind UI retains its own ScrollWidget, wheel handling,
/// scrollbar dragging, clipping, layout and offset clamping.
struct ScrollMemory {
    offset: Rc<Cell<i32>>,
    scroll: Option<NodeId>,
}

impl Widget for ScrollMemory {
    fn on_update(&mut self, ctx: &mut EventCtx) {
        let root = ctx.id();
        let tree = ctx.tree_mut();
        if let Some(scroll) = self.scroll {
            if let Some((offset, _)) = tree.scroll_range(scroll, Axis::Vertical) {
                self.offset.set(offset);
                return;
            }
        }
        if let Some(scroll) = first_scroll(tree, root) {
            // Reactive updates run before measure/arrange. Native layout clamps
            // this saved offset against the newly measured content this frame.
            if tree.set_scroll_y(scroll, self.offset.get().max(0)) {
                self.scroll = Some(scroll);
            } else {
                eprintln!("Wind UI page scroll disappeared while restoring its position");
            }
        }
    }
}

fn first_scroll(tree: &Tree, root: NodeId) -> Option<NodeId> {
    let mut pending = VecDeque::from([root]);
    while let Some(id) = pending.pop_front() {
        let Some(node) = tree.get(id) else { continue };
        if !node.effective_visible() {
            continue;
        }
        if matches!(node.layout, Layout::Scroll) {
            return Some(id);
        }
        pending.extend(node.children.iter().copied());
    }
    None
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use windui::{
        event::{MouseButton, PointerEvent, PointerKind},
        prelude::{signal, Point, Size},
        signal::SignalScope,
        text::NullTextEngine,
    };

    use super::*;

    fn page(height: i32, offset: Rc<Cell<i32>>) -> (Tree, NodeId) {
        let mut tree = Tree::new();
        let root = remember(scrollable(Element::leaf().height(height)), offset).build(&mut tree);
        tree.root = Some(root);
        tree.layout_root(Size::new(320, 100), &mut NullTextEngine);
        let scroll = first_scroll(&tree, root).unwrap();
        (tree, scroll)
    }

    #[test]
    fn native_wheel_offset_survives_rebuild_and_clamps_to_new_content() {
        let offset = Rc::new(Cell::new(120));
        let (mut tree, scroll) = page(1000, Rc::clone(&offset));
        assert_eq!(tree.scroll_range(scroll, Axis::Vertical).unwrap().0, 120);
        tree.dispatch_pointer(
            PointerEvent::single(
                PointerKind::Wheel(-120),
                Point::new(20, 20),
                MouseButton::Left,
            ),
            &mut None,
            &mut None,
        );
        tree.layout_root(Size::new(320, 100), &mut NullTextEngine);
        let scrolled = tree.scroll_range(scroll, Axis::Vertical).unwrap().0;
        assert!(
            scrolled > 120,
            "native ScrollWidget must retain wheel handling"
        );
        assert_eq!(offset.get(), scrolled);
        drop(tree);
        let (tree, scroll) = page(1000, Rc::clone(&offset));
        assert_eq!(
            tree.scroll_range(scroll, Axis::Vertical).unwrap().0,
            scrolled
        );
        drop(tree);
        let (mut tree, scroll) = page(180, Rc::clone(&offset));
        assert_eq!(tree.scroll_range(scroll, Axis::Vertical).unwrap().0, 80);
        tree.layout_root(Size::new(320, 100), &mut NullTextEngine);
        assert_eq!(offset.get(), 80);
    }

    #[test]
    fn native_table_keeps_captured_row_alive_through_wheel_and_viewport_changes() {
        for wheel_during_press in [false, true] {
            let mut scope = SignalScope::new();
            let (rows, clicks) = scope.collect(|| {
                (
                    signal(
                        (0..1000)
                            .map(|row| vec![format!("Process {row}")])
                            .collect(),
                    ),
                    signal(0_usize),
                )
            });
            let capture_active = Rc::new(Cell::new(false));
            let capture_check = Rc::clone(&capture_active);
            let realized = Rc::new(RefCell::new(Vec::new()));
            let rendered = Rc::clone(&realized);
            let mut tree = Tree::new();
            let root = preserve_table_rows_while(
                Element::table_virtual(vec![("Process", 1.0)], rows, 52)
                    .cell_render(move |row, _, value| {
                        rendered.borrow_mut().push(row);
                        Some(
                            Element::button(value)
                                .small()
                                .on_click(move |_| clicks.update(|value| *value += 1)),
                        )
                    })
                    .fill(),
                move || capture_check.get(),
            )
            .build(&mut tree);
            tree.root = Some(root);
            let initial_size = Size::new(360, 220);
            tree.layout_root(initial_size, &mut NullTextEngine);
            let scroll = first_scroll(&tree, root).unwrap();
            let initial_controls = tree.focusable_order();
            assert!(!initial_controls.is_empty());
            let pressed = initial_controls[0];
            let bounds = tree.abs_bounds(pressed);
            let point = Point::new(bounds.x + bounds.w / 2, bounds.y + bounds.h / 2);
            let (mut hover, mut capture) = (None, None);
            tree.dispatch_pointer(
                PointerEvent::single(PointerKind::Down, point, MouseButton::Left),
                &mut hover,
                &mut capture,
            );
            assert_eq!(capture, Some(pressed));
            capture_active.set(true);
            realized.borrow_mut().clear();
            let held_size = if wheel_during_press {
                initial_size
            } else {
                Size::new(360, 420)
            };
            for _ in 0..3 {
                if wheel_during_press {
                    tree.dispatch_pointer(
                        PointerEvent::single(PointerKind::Wheel(-1200), point, MouseButton::Left),
                        &mut hover,
                        &mut capture,
                    );
                }
                tree.layout_root(held_size, &mut NullTextEngine);
                assert_eq!(capture, Some(pressed));
                assert!(
                    tree.get(pressed).is_some(),
                    "native row capture was disposed"
                );
                assert_eq!(tree.focusable_order(), initial_controls);
                assert!(realized.borrow().is_empty());
            }
            if wheel_during_press {
                assert!(tree.scroll_range(scroll, Axis::Vertical).unwrap().0 > 0);
            }
            tree.dispatch_pointer(
                PointerEvent::single(PointerKind::Up, point, MouseButton::Left),
                &mut hover,
                &mut capture,
            );
            assert!(
                capture.is_none(),
                "native row button did not release capture"
            );
            capture_active.set(false);
            tree.layout_root(held_size, &mut NullTextEngine);
            assert!(
                tree.get(pressed).is_none(),
                "deferred native rows did not catch up"
            );
            assert!(!realized.borrow().is_empty());
            if wheel_during_press {
                assert!(!realized.borrow().contains(&0));
            } else {
                assert!(tree.focusable_order().len() > initial_controls.len());
            }

            let bounds = tree.abs_bounds(scroll);
            let wheel_point = Point::new(bounds.x + 20, bounds.y + 20);
            let before = tree.scroll_range(scroll, Axis::Vertical).unwrap().0;
            tree.dispatch_pointer(
                PointerEvent::single(PointerKind::Wheel(-120), wheel_point, MouseButton::Left),
                &mut hover,
                &mut capture,
            );
            tree.layout_root(held_size, &mut NullTextEngine);
            assert!(tree.scroll_range(scroll, Axis::Vertical).unwrap().0 > before);
            let button = tree
                .focusable_order()
                .into_iter()
                .find_map(|id| {
                    let bounds = tree.abs_bounds(id);
                    let center = Point::new(bounds.x + bounds.w / 2, bounds.y + bounds.h / 2);
                    std::iter::successors(tree.hit_test(center), |id| {
                        tree.get(*id).and_then(|node| node.parent)
                    })
                    .any(|ancestor| ancestor == id)
                    .then_some(center)
                })
                .expect("native table must retain a visible row button");
            let before = clicks.get();
            for kind in [PointerKind::Down, PointerKind::Up] {
                tree.dispatch_pointer(
                    PointerEvent::single(kind, button, MouseButton::Left),
                    &mut hover,
                    &mut capture,
                );
                capture_active.set(capture.is_some());
            }
            assert!(capture.is_none());
            assert_eq!(clicks.get(), before + 1, "later native row click was lost");
        }
    }
}
