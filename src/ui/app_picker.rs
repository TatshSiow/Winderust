//! Shared running-app search with a native input and an anchored, scrollable menu.
use super::{design, navigation, widgets};
use iced::advanced::{
    layout, renderer,
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};
use iced::widget::{column, container, image, row, scrollable, text};
use iced::{
    keyboard, mouse, overlay, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
};
use rust_i18n::t;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(super) struct Candidate {
    pub info: crate::foreground::ProcessCandidateInfo,
    icon: Option<Arc<image::Handle>>,
}

pub(super) fn load(cached: Vec<Candidate>) -> Result<Vec<Candidate>, String> {
    crate::foreground::list_process_candidates().map(|items| {
        items
            .into_iter()
            .map(|info| {
                let icon = cached
                    .iter()
                    .find(|old| old.info.image_path == info.image_path)
                    .map(|old| old.icon.clone())
                    .unwrap_or_else(|| crate::process_icon::load_process_icon(&info.image_path));
                Candidate { info, icon }
            })
            .collect()
    })
}

pub(super) fn view<'a, M: Clone + 'static>(
    query: &str,
    candidates: &[Candidate],
    enabled: bool,
    select: fn(String) -> M,
    browse: M,
    add: Option<M>,
    allowed: impl Fn(&str) -> Option<bool>,
) -> Element<'a, M> {
    let filter = query.trim().to_lowercase();
    let mut choices: Vec<_> = candidates
        .iter()
        .filter_map(|candidate| {
            let path = candidate.info.image_path.to_string_lossy();
            if !path.to_lowercase().contains(&filter)
                && !candidate.info.name.to_lowercase().contains(&filter)
            {
                return None;
            }
            allowed(&path).map(|available| (candidate.clone(), available))
        })
        .collect();
    choices.sort_by_cached_key(|(candidate, _)| {
        (
            candidate.info.name.to_lowercase(),
            candidate.info.image_path.clone(),
        )
    });
    let input = widgets::text_input(&t!("common.search_running_apps"), query)
        .on_input_maybe(enabled.then_some(select))
        .width(Length::Fill)
        .into();
    row![
        Element::new(Picker {
            input,
            choices,
            select,
            browse,
            enabled,
            menu: None,
        }),
        widgets::button(text(t!("common.add").to_string())).on_press_maybe(add),
    ]
    .spacing(design::space::SMALL)
    .align_y(iced::Center)
    .into()
}

pub(super) fn app_name<'a, M: 'a>(path: &str, candidates: &[Candidate]) -> Element<'a, M> {
    let candidate = candidates.iter().find(|candidate| {
        crate::foreground::same_executable_path(
            &candidate.info.image_path,
            std::path::Path::new(path),
        )
    });
    let icon: Element<'a, M> = candidate
        .and_then(|candidate| candidate.icon.as_ref())
        .map(|icon| image((**icon).clone()).width(20).height(20).into())
        .unwrap_or_else(|| {
            container(navigation::glyph("icons/app-window.svg"))
                .width(20)
                .height(20)
                .center_x(20)
                .center_y(20)
                .into()
        });
    let name = candidate
        .map(|candidate| candidate.info.name.clone())
        .unwrap_or_else(|| {
            std::path::Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_owned())
        });
    row![
        icon,
        text(name)
            .style(text::base)
            .wrapping(iced::widget::text::Wrapping::None)
    ]
    .spacing(design::space::SMALL)
    .align_y(iced::Center)
    .into()
}

#[derive(Default)]
struct State {
    focused: bool,
    open: bool,
    selected: Option<usize>,
}

struct Picker<'a, M> {
    input: Element<'a, M>,
    choices: Vec<(Candidate, bool)>,
    select: fn(String) -> M,
    browse: M,
    enabled: bool,
    menu: Option<(Option<usize>, Element<'static, M>)>,
}

impl<M: Clone> Picker<'_, M> {
    fn menu(&self, selected: Option<usize>) -> Element<'static, M>
    where
        M: 'static,
    {
        let mut items = column![].spacing(0);
        for (index, (candidate, available)) in self.choices.iter().enumerate() {
            let icon: Element<'_, M> = candidate
                .icon
                .as_ref()
                .map(|icon| image((**icon).clone()).width(20).height(20).into())
                .unwrap_or_else(|| {
                    container(navigation::glyph("icons/app-window.svg"))
                        .width(20)
                        .height(20)
                        .center_x(20)
                        .center_y(20)
                        .into()
                });
            let path = candidate.info.image_path.to_string_lossy().into_owned();
            let detail = if *available {
                path.clone()
            } else {
                format!("{} ? {}", t!("app_suspension.indicator.unavailable"), path)
            };
            let content = row![
                icon,
                column![
                    text(candidate.info.name.clone())
                        .size(design::typography::SECONDARY)
                        .style(text::base)
                        .line_height(1.2)
                        .wrapping(iced::widget::text::Wrapping::None),
                    text(detail)
                        .size(design::typography::CAPTION)
                        .line_height(1.2)
                        .style(text::secondary)
                        .wrapping(iced::widget::text::Wrapping::None),
                ]
                .width(Length::Fill)
            ]
            .spacing(design::space::SMALL)
            .align_y(iced::Center);
            items = items.push(
                widgets::button(content)
                    .width(Length::Fill)
                    .height(40)
                    .clip(true)
                    .padding([4, 8])
                    .style(if selected == Some(index) {
                        widgets::selected
                    } else {
                        widgets::quiet
                    })
                    .on_press_maybe(available.then(|| (self.select)(path))),
            );
        }
        let footer = widgets::button(
            row![
                navigation::glyph("icons/plus.svg"),
                text(t!("common.browse_local_executable").to_string()),
            ]
            .spacing(design::space::SMALL)
            .align_y(iced::Center),
        )
        .width(Length::Fill)
        .style(if selected == Some(self.choices.len()) {
            widgets::selected
        } else {
            widgets::quiet
        })
        .height(32)
        .on_press(self.browse.clone());
        container(
            column![
                scrollable(items)
                    .id("running-app-options")
                    .height(Length::Fill),
                footer
            ]
            .spacing(4),
        )
        .padding(4)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|theme| {
            let mut style = widgets::surface(theme);
            style.border.width = 1.0;
            style.border.color = theme.extended_palette().background.strong.color;
            style
        })
        .into()
    }
}

impl<M: Clone + 'static> Widget<M, Theme, Renderer> for Picker<'_, M> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(372.0), Length::Shrink)
    }
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.input), Tree::empty()]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.children[0].diff(&self.input);
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.input
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &limits.width(372))
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
        self.input.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
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
        self.input
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        let state = tree.state.downcast_mut::<State>();
        if self.enabled
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            )
            && cursor.is_over(layout.bounds())
        {
            state.open = true;
            state.selected = None;
            shell.invalidate_layout();
            shell.request_redraw();
        }
        self.input.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        let focused =
            tree.children[0]
                .state
                .downcast_ref::<iced::widget::text_input::State<
                    <Renderer as iced::advanced::text::Renderer>::Paragraph,
                >>()
                .is_focused();
        if self.enabled
            && focused
            && (!state.focused
                || matches!(
                    event,
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Character(_)
                            | keyboard::Key::Named(keyboard::key::Named::ArrowDown),
                        ..
                    })
                ))
        {
            state.open = true;
            state.selected = None;
            shell.invalidate_layout();
            shell.request_redraw();
        }
        state.focused = focused;
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.input.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        if !state.open || !self.enabled || !layout.bounds().intersects(viewport) {
            return None;
        }
        // Keep native button status between overlay update and draw calls.
        if self
            .menu
            .as_ref()
            .is_none_or(|(selected, _)| *selected != state.selected)
        {
            self.menu = Some((state.selected, self.menu(state.selected)));
        }
        let (_, menu) = self.menu.as_mut()?;
        tree.children[1].diff(&*menu);
        Some(overlay::Element::new(Box::new(Menu {
            content: menu,
            tree: &mut tree.children[1],
            state,
            anchor: layout.bounds() + translation,
            choices: &self.choices,
            select: self.select,
            browse: self.browse.clone(),
        })))
    }
}

struct Menu<'a, M> {
    content: &'a mut Element<'static, M>,
    tree: &'a mut Tree,
    state: &'a mut State,
    anchor: Rectangle,
    choices: &'a [(Candidate, bool)],
    select: fn(String) -> M,
    browse: M,
}

impl<M: Clone + 'static> iced::advanced::Overlay<M, Theme, Renderer> for Menu<'_, M> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let below = (bounds.height - self.anchor.y - self.anchor.height - 4.0).max(0.0);
        let above = (self.anchor.y - 4.0).max(0.0);
        let height = ((self.choices.len() as f32 * 40.0) + 44.0)
            .min(600.0)
            .min(below.max(above));
        let y = if below >= height {
            self.anchor.y + self.anchor.height + 4.0
        } else {
            self.anchor.y - height - 4.0
        };
        self.content
            .as_widget_mut()
            .layout(
                self.tree,
                renderer,
                &layout::Limits::new(Size::ZERO, Size::new(self.anchor.width, height)),
            )
            .move_to(iced::Point::new(
                self.anchor
                    .x
                    .min((bounds.width - self.anchor.width).max(0.0)),
                y,
            ))
    }
    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.content.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &layout.bounds(),
        );
    }
    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
    ) {
        if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)))
            && !cursor.is_over(layout.bounds())
            && !cursor.is_over(self.anchor)
        {
            self.state.open = false;
            shell.invalidate_layout();
            shell.request_redraw();
            return;
        }
        if let Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) = event {
            use keyboard::key::Named;
            match key {
                keyboard::Key::Named(Named::Escape | Named::Tab) => {
                    self.state.open = false;
                    shell.invalidate_layout();
                    shell.request_redraw();
                    if *key == keyboard::Key::Named(Named::Escape) {
                        shell.capture_event();
                    }
                    return;
                }
                keyboard::Key::Named(Named::ArrowDown | Named::ArrowUp) => {
                    let indices: Vec<_> = self
                        .choices
                        .iter()
                        .enumerate()
                        .filter_map(|(i, (_, available))| available.then_some(i))
                        .chain(std::iter::once(self.choices.len()))
                        .collect();
                    let position = self
                        .state
                        .selected
                        .and_then(|selected| indices.iter().position(|i| *i == selected));
                    let next = if *key == keyboard::Key::Named(Named::ArrowUp) {
                        position.map_or(indices.len() - 1, |i| {
                            (i + indices.len() - 1) % indices.len()
                        })
                    } else {
                        position.map_or(0, |i| (i + 1) % indices.len())
                    };
                    self.state.selected = Some(indices[next]);
                    let mut scroll = iced::advanced::widget::operation::scrollable::scroll_to(
                        iced::advanced::widget::Id::new("running-app-options"),
                        iced::widget::scrollable::AbsoluteOffset {
                            x: None,
                            y: Some(
                                (indices[next] as f32 * 40.0
                                    - (layout.bounds().height - 84.0).max(0.0) / 2.0)
                                    .max(0.0),
                            ),
                        },
                    );
                    self.content
                        .as_widget_mut()
                        .operate(self.tree, layout, renderer, &mut scroll);

                    shell.invalidate_layout();
                    shell.request_redraw();
                    shell.capture_event();
                    return;
                }
                keyboard::Key::Named(Named::Enter) => {
                    if let Some(index) = self.state.selected {
                        if index == self.choices.len() {
                            shell.publish(self.browse.clone());
                        } else if let Some((candidate, true)) = self.choices.get(index) {
                            shell.publish((self.select)(
                                candidate.info.image_path.to_string_lossy().into_owned(),
                            ));
                        }
                        self.state.open = false;
                        shell.invalidate_layout();
                        shell.request_redraw();
                        shell.capture_event();
                        return;
                    }
                }
                _ => {
                    self.state.selected = None;
                }
            }
        }
        let mut messages = Vec::new();
        let mut local = Shell::new(&mut messages);
        self.content.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            &mut local,
            &layout.bounds(),
        );
        let chosen = !local.is_empty();
        shell.merge(local, |message| message);
        if chosen {
            self.state.open = false;
            shell.invalidate_layout();
            shell.request_redraw();
        }
    }
    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            self.tree,
            layout,
            cursor,
            &layout.bounds(),
            renderer,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::Overlay;

    #[derive(Debug, Clone, PartialEq)]
    enum Message {
        Select(String),
        Browse,
    }

    fn key(key: keyboard::key::Named) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key),
            modified_key: keyboard::Key::Named(key),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        })
    }

    #[test]
    fn menu_stays_in_bounds_and_skips_unavailable_apps_on_keyboard_selection() {
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let candidate = |name: &str| Candidate {
            info: crate::foreground::ProcessCandidateInfo {
                name: name.into(),
                image_path: format!(r"C:\Apps\{name}").into(),
                has_suspendable_instance: true,
            },
            icon: None,
        };
        let mut picker = Picker {
            input: widgets::text_input("", "").into(),
            choices: vec![
                (candidate("blocked.exe"), false),
                (candidate("allowed.exe"), true),
            ],
            select: Message::Select,
            browse: Message::Browse,
            enabled: true,
            menu: None,
        };
        let mut content = picker.menu(None);
        let mut tree = Tree::new(&content);
        let mut state = State {
            open: true,
            ..State::default()
        };
        let mut menu = Menu {
            content: &mut content,
            tree: &mut tree,
            state: &mut state,
            anchor: Rectangle {
                x: 20.0,
                y: 250.0,
                width: 372.0,
                height: 32.0,
            },
            choices: &picker.choices,
            select: Message::Select,
            browse: Message::Browse,
        };
        let node = menu.layout(&renderer, Size::new(500.0, 300.0));
        let layout = Layout::new(&node);
        assert_eq!(layout.bounds().width, 372.0);
        assert!(layout.bounds().y >= 0.0 && layout.bounds().y + layout.bounds().height <= 250.0);
        let mut messages = Vec::new();
        let mut clipboard = iced::advanced::clipboard::Null;
        for event in [
            key(keyboard::key::Named::ArrowDown),
            key(keyboard::key::Named::Enter),
        ] {
            menu.update(
                &event,
                layout,
                mouse::Cursor::Unavailable,
                &renderer,
                &mut clipboard,
                &mut Shell::new(&mut messages),
            );
        }
        assert_eq!(
            messages,
            vec![Message::Select(r"C:\Apps\allowed.exe".into())]
        );
        assert!(!menu.state.open);
        menu.state.open = true;
        menu.update(
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            layout,
            mouse::Cursor::Available(iced::Point::new(450.0, 290.0)),
            &renderer,
            &mut clipboard,
            &mut Shell::new(&mut messages),
        );
        assert!(!menu.state.open);
        assert_eq!(messages.len(), 1);
        menu.state.open = true;
        menu.state.selected = Some(picker.choices.len());
        menu.update(
            &key(keyboard::key::Named::Enter),
            layout,
            mouse::Cursor::Unavailable,
            &renderer,
            &mut clipboard,
            &mut Shell::new(&mut messages),
        );
        assert_eq!(messages.last(), Some(&Message::Browse));
        assert!(!menu.state.open);
        menu.state.open = true;
        messages.clear();
        let cursor = mouse::Cursor::Available(iced::Point::new(
            layout.bounds().x + 50.0,
            layout.bounds().y + 64.0,
        ));
        for event in [
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ] {
            menu.update(
                &event,
                layout,
                cursor,
                &renderer,
                &mut clipboard,
                &mut Shell::new(&mut messages),
            );
        }
        assert_eq!(
            messages,
            vec![Message::Select(r"C:\Apps\allowed.exe".into())]
        );
        assert!(!menu.state.open);
        drop(menu);

        let mut picker_tree = Tree::new(&picker as &dyn Widget<Message, Theme, Renderer>);
        picker_tree.state.downcast_mut::<State>().open = true;
        let viewport = Rectangle::with_size(Size::new(500.0, 300.0));
        let input = picker.layout(
            &mut picker_tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, viewport.size()),
        );
        for redraw in [true, false] {
            let mut overlay = picker
                .overlay(
                    &mut picker_tree,
                    Layout::new(&input),
                    &renderer,
                    &viewport,
                    Vector::ZERO,
                )
                .unwrap();
            let overlay = overlay.as_overlay_mut();
            let node = overlay.layout(&renderer, viewport.size());
            let position = iced::Point::new(node.bounds().x + 50.0, node.bounds().y + 64.0);
            let event = if redraw {
                Event::Window(iced::window::Event::RedrawRequested(
                    iced::time::Instant::now(),
                ))
            } else {
                Event::Mouse(mouse::Event::CursorMoved { position })
            };
            let mut shell = Shell::new(&mut messages);
            overlay.update(
                &event,
                Layout::new(&node),
                if redraw {
                    mouse::Cursor::Unavailable
                } else {
                    mouse::Cursor::Available(position)
                },
                &renderer,
                &mut clipboard,
                &mut shell,
            );
            if !redraw {
                assert_eq!(
                    shell.redraw_request(),
                    iced::window::RedrawRequest::NextFrame
                );
            }
        }
    }
}
