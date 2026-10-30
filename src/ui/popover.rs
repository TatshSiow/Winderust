//! Native context menus for existing page actions.

use windui::{event::MenuItem, prelude::Element};

pub(super) fn context_menu(
    anchor: Element,
    items: impl Fn() -> Vec<MenuItem> + 'static,
) -> Element {
    anchor.on_context_menu(items)
}
