//! Shared running-app search with a native input and an anchored, scrollable menu.
use super::{design, navigation, widgets};
use crate::ui::scrolling::scrollable;
use iced::advanced::{
    layout, renderer,
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};
use iced::widget::{column, container, image, row, text};
use iced::{
    keyboard, mouse, overlay, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
};
use rust_i18n::t;
use std::{collections::HashMap, ffi::OsString, sync::Arc};

#[derive(Debug)]
pub(super) struct Candidate {
    pub info: crate::foreground::ProcessCandidateInfo,
    icon: Option<Arc<image::Handle>>,
}

pub(super) fn load(cached: &[Candidate]) -> Result<Arc<Vec<Candidate>>, String> {
    crate::foreground::list_process_candidates().map(|items| {
        let cached = cached
            .iter()
            .map(|candidate| (candidate.info.image_path.as_path(), candidate))
            .collect::<HashMap<_, _>>();
        let candidates = items
            .into_iter()
            .map(|info| {
                let icon = cached
                    .get(info.image_path.as_path())
                    .map(|candidate| candidate.icon.clone())
                    .unwrap_or_else(|| crate::process_icon::load_process_icon(&info.image_path));
                Candidate { info, icon }
            })
            .collect();
        Arc::new(candidates)
    })
}

pub(super) fn is_path_query(query: &str) -> bool {
    query.contains(['\\', '/', ':'])
}

pub(super) fn path_format_hint<'a>() -> iced::widget::Text<'a> {
    text(t!("common.executable_path_format_hint").to_string())
        .style(text::danger)
        .wrapping(text::Wrapping::WordOrGlyph)
}

pub(super) fn path_readd_hint<'a, M: 'a>() -> iced::widget::Container<'a, M> {
    container(
        text(t!("common.executable_path_readd").to_string())
            .style(text::secondary)
            .wrapping(text::Wrapping::WordOrGlyph),
    )
    .padding(iced::Padding::new(widgets::CARD_PADDING as f32).top(0.0))
}

type PathCallback<M> = fn(String) -> M;

pub(super) fn view<'a, M: Clone + 'static>(
    query: &str,
    candidates: &'a [Candidate],
    enabled: bool,
    select: fn(String) -> M,
    browse: M,
    add: Option<M>,
    allowed: impl Fn(&str) -> Option<bool>,
) -> Element<'a, M> {
    view_with_path_selection(
        query,
        candidates,
        enabled,
        (select, select),
        browse,
        add,
        allowed,
    )
}

pub(super) fn view_with_path_selection<'a, M: Clone + 'static>(
    query: &str,
    candidates: &'a [Candidate],
    enabled: bool,
    (edit, select): (PathCallback<M>, PathCallback<M>),
    browse: M,
    add: Option<M>,
    allowed: impl Fn(&str) -> Option<bool>,
) -> Element<'a, M> {
    let filter = query.trim().to_lowercase();
    let mut choices: Vec<_> = candidates
        .iter()
        .filter_map(|candidate| {
            let path = candidate.info.image_path.to_str()?;
            if !filter.is_empty()
                && !path.to_lowercase().contains(&filter)
                && !candidate.info.name.to_lowercase().contains(&filter)
            {
                return None;
            }
            allowed(path).map(|available| (candidate, available))
        })
        .collect();
    choices.sort_by_cached_key(|&(candidate, _)| {
        (
            candidate.info.name.to_lowercase(),
            candidate.info.image_path.as_path(),
        )
    });
    let input = widgets::text_input(&t!("common.search_running_apps"), query)
        .padding(widgets::SEARCH_INPUT_PADDING)
        .on_input_maybe(enabled.then_some(edit))
        .width(Length::Fill)
        .into();
    container(
        row![
            widgets::search_field(
                Element::new(Picker {
                    input,
                    choices,
                    select,
                    browse,
                    enabled,
                    menu: None,
                }),
                (enabled && !query.is_empty()).then(|| edit(String::new())),
            ),
            widgets::rule_add_button(add),
        ]
        .spacing(design::space::SMALL)
        .align_y(iced::Center),
    )
    .max_width(460)
    .into()
}

pub(super) fn icon_for_name<'a>(
    name: &str,
    candidates: &'a [Candidate],
) -> Option<&'a image::Handle> {
    let mut matches = candidates
        .iter()
        .filter(|candidate| candidate.info.name.eq_ignore_ascii_case(name));
    let first = matches.next()?;
    if matches.any(|candidate| {
        !crate::foreground::same_executable_path(&first.info.image_path, &candidate.info.image_path)
    }) {
        return None;
    }
    first.icon.as_deref()
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

type Choice<'a> = (&'a Candidate, bool);

fn choice_identity<'a>((candidate, available): &Choice<'a>) -> (&'a std::ffi::OsStr, bool) {
    (candidate.info.image_path.as_os_str(), *available)
}

enum Selection {
    App(OsString),
    Browse,
}

#[derive(Default)]
struct State {
    focused: bool,
    open: bool,
    selected: Option<Selection>,
    pressed_choices: Option<Vec<(OsString, bool)>>,
}

impl State {
    fn selected_index(&self, choices: &[Choice<'_>]) -> Option<usize> {
        match self.selected.as_ref()? {
            Selection::App(path) => choices.iter().position(|choice| {
                let (candidate, available) = choice_identity(choice);
                available && path == candidate
            }),
            Selection::Browse => Some(choices.len()),
        }
    }
}

struct Picker<'a, M> {
    input: Element<'a, M>,
    choices: Vec<Choice<'a>>,
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
            let path = candidate.info.image_path.to_string_lossy();
            let selection = available.then(|| (self.select)(path.as_ref().to_owned()));
            let detail = if *available {
                path.into_owned()
            } else {
                format!("{} · {path}", t!("app_suspension.indicator.unavailable"))
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
                    .on_press_maybe(selection),
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
        (container(
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
        }))
        .into()
    }
}

impl<M: Clone + 'static> Widget<M, Theme, Renderer> for Picker<'_, M> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
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
        let state = tree.state.downcast_mut::<State>();
        if state.selected_index(&self.choices).is_none() {
            state.selected = None;
        }
        // Button press state follows the menu row. Drop an in-flight press
        // when a refresh changes which executable that row represents.
        let changed_during_press = state.pressed_choices.as_ref().is_some_and(|pressed| {
            !pressed
                .iter()
                .map(|(path, available)| (path.as_os_str(), *available))
                .eq(self.choices.iter().map(choice_identity))
        });
        if changed_during_press || (!self.enabled && state.pressed_choices.is_some()) {
            tree.children[1] = Tree::empty();
            state.pressed_choices = None;
        }
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.input
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
            if !state.open {
                tree.children[1] = Tree::empty();
                state.pressed_choices = None;
            }
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
            if !state.open {
                tree.children[1] = Tree::empty();
                state.pressed_choices = None;
            }
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
        if !state.open || !self.enabled {
            return None;
        }
        let anchor = layout.bounds() + translation;
        if !viewport.intersects(&anchor) {
            state.open = false;
            return None;
        }
        // Keep native button status between overlay update and draw calls.
        let selected = state.selected_index(&self.choices);
        if self
            .menu
            .as_ref()
            .is_none_or(|(shown, _)| *shown != selected)
        {
            self.menu = Some((selected, self.menu(selected)));
        }
        let (_, menu) = self.menu.as_mut()?;
        tree.children[1].diff(&*menu);
        Some(overlay::Element::new(Box::new(Menu {
            content: menu,
            tree: &mut tree.children[1],
            state,
            anchor,
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
    choices: &'a [Choice<'a>],
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
                        .selected_index(self.choices)
                        .and_then(|selected| indices.iter().position(|i| *i == selected));
                    let next = if *key == keyboard::Key::Named(Named::ArrowUp) {
                        position.map_or(indices.len() - 1, |i| {
                            (i + indices.len() - 1) % indices.len()
                        })
                    } else {
                        position.map_or(0, |i| (i + 1) % indices.len())
                    };
                    let selected = indices[next];
                    self.state.selected = Some(match self.choices.get(selected) {
                        Some(choice) => Selection::App(choice_identity(choice).0.to_owned()),
                        None => Selection::Browse,
                    });
                    let mut scroll = iced::advanced::widget::operation::scrollable::scroll_to(
                        iced::advanced::widget::Id::new("running-app-options"),
                        iced::widget::scrollable::AbsoluteOffset {
                            x: None,
                            y: Some(
                                (selected as f32 * 40.0
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
                    if let Some(index) = self.state.selected_index(self.choices) {
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
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(iced::touch::Event::FingerPressed { .. })
                if local.is_event_captured() =>
            {
                self.state.pressed_choices = Some(
                    self.choices
                        .iter()
                        .map(choice_identity)
                        .map(|(path, available)| (path.to_owned(), available))
                        .collect(),
                );
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(
                iced::touch::Event::FingerLifted { .. } | iced::touch::Event::FingerLost { .. },
            ) => {
                self.state.pressed_choices = None;
            }
            _ => {}
        }
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
    fn log_icons_require_an_unambiguous_cached_executable() {
        let icon = Arc::new(image::Handle::from_rgba(1, 1, vec![255; 4]));
        let candidate = |path: &str| Candidate {
            info: crate::foreground::ProcessCandidateInfo {
                name: "app.exe".into(),
                image_path: path.into(),
                has_suspendable_instance: true,
            },
            icon: Some(Arc::clone(&icon)),
        };
        let mut candidates = vec![candidate(r"C:\Apps\app.exe")];
        assert!(icon_for_name("APP.EXE", &candidates).is_some());
        assert!(icon_for_name("missing.exe", &candidates).is_none());
        candidates.push(candidate(r"C:\Other\app.exe"));
        assert!(icon_for_name("app.exe", &candidates).is_none());
    }

    #[test]
    fn search_keeps_trimmed_unicode_matches_and_permission_callbacks() {
        for count in [50, 500] {
            let names = ["aLpHa.exe", "İ.exe", "Σ.exe", "ß.exe"];
            let candidates = (0..count)
                .map(|index| Candidate {
                    info: crate::foreground::ProcessCandidateInfo {
                        name: names[index % names.len()].into(),
                        image_path: format!(
                            r"C:\Apps\bucket{index:03}\{}",
                            names[index % names.len()]
                        )
                        .into(),
                        has_suspendable_instance: true,
                    },
                    icon: None,
                })
                .collect::<Vec<_>>();
            for query in ["", "   ", "alpha", "  Σ  ", "i\u{307}", "missing"] {
                let filter = query.trim().to_lowercase();
                let expected = candidates
                    .iter()
                    .filter(|candidate| {
                        candidate
                            .info
                            .image_path
                            .to_string_lossy()
                            .to_lowercase()
                            .contains(&filter)
                            || candidate.info.name.to_lowercase().contains(&filter)
                    })
                    .map(|candidate| candidate.info.image_path.to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                let checked = std::cell::RefCell::new(Vec::new());
                let _view = view(
                    query,
                    &candidates,
                    true,
                    Message::Select,
                    Message::Browse,
                    None,
                    |path| {
                        checked.borrow_mut().push(path.to_owned());
                        if path.contains("bucket000") {
                            None
                        } else {
                            Some(!path.contains("bucket001"))
                        }
                    },
                );
                assert_eq!(checked.into_inner(), expected, "query {query:?}");
            }
        }
    }

    #[test]
    fn narrow_picker_keeps_the_full_add_rule_label() {
        #[derive(Default)]
        struct AddLabel(Option<Rectangle>);

        impl Operation for AddLabel {
            fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
                visit(self);
            }

            fn text(
                &mut self,
                _: Option<&iced::advanced::widget::Id>,
                bounds: Rectangle,
                content: &str,
            ) {
                if content == t!("common.add_rule").as_ref() {
                    self.0 = Some(bounds);
                }
            }
        }

        let renderer = Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        for query in ["", r"C:\Apps\sample.exe"] {
            let add_label = |width| {
                let mut picker = view(
                    query,
                    &[],
                    true,
                    Message::Select,
                    Message::Browse,
                    Some(Message::Browse),
                    |_| None,
                );
                let mut tree = Tree::new(&picker);
                let limits = layout::Limits::new(Size::ZERO, Size::new(width, 32.0));
                let node = picker.as_widget_mut().layout(&mut tree, &renderer, &limits);
                let mut label = AddLabel::default();
                picker.as_widget_mut().operate(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &mut label,
                );
                label.0.unwrap()
            };
            let narrow = add_label(418.0);
            let wide = add_label(740.0);
            assert_eq!(narrow.size(), wide.size());
            assert!(narrow.x < wide.x);
            assert!(narrow.x + narrow.width <= 418.0);
        }
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
        let candidates = [candidate("blocked.exe"), candidate("allowed.exe")];
        let mut picker = Picker {
            input: widgets::text_input("", "").into(),
            choices: vec![(&candidates[0], false), (&candidates[1], true)],
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
        menu.state.selected = Some(Selection::Browse);
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
    #[test]
    fn catalog_refresh_keeps_the_highlight_and_cancels_a_stale_press() {
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let make_candidate = |path: &str| Candidate {
            info: crate::foreground::ProcessCandidateInfo {
                name: "app.exe".into(),
                image_path: path.into(),
                has_suspendable_instance: true,
            },
            icon: None,
        };
        let first = make_candidate(r"C:\First\app.exe");
        let selected = make_candidate(r"C:\Selected\app.exe");
        let mut picker = Picker {
            input: widgets::text_input("", "").into(),
            choices: vec![(&selected, true)],
            select: Message::Select,
            browse: Message::Browse,
            enabled: true,
            menu: None,
        };
        let mut tree = Tree::new(&picker as &dyn Widget<Message, Theme, Renderer>);
        tree.state.downcast_mut::<State>().open = true;
        let viewport = Rectangle::with_size(Size::new(500.0, 300.0));
        let send = |picker: &mut Picker<'_, Message>, tree: &mut Tree, event: Event, point| {
            let input = picker.layout(
                tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, viewport.size()),
            );
            let mut overlay = picker
                .overlay(
                    tree,
                    Layout::new(&input),
                    &renderer,
                    &viewport,
                    Vector::ZERO,
                )
                .unwrap();
            let overlay = overlay.as_overlay_mut();
            let node = overlay.layout(&renderer, viewport.size());
            let mut messages = Vec::new();
            overlay.update(
                &event,
                Layout::new(&node),
                if point {
                    mouse::Cursor::Available(iced::Point::new(
                        node.bounds().x + 50.0,
                        node.bounds().y + 24.0,
                    ))
                } else {
                    mouse::Cursor::Unavailable
                },
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
            );
            messages
        };
        assert!(send(
            &mut picker,
            &mut tree,
            key(keyboard::key::Named::ArrowDown),
            false
        )
        .is_empty());
        assert!(send(
            &mut picker,
            &mut tree,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            true
        )
        .is_empty());
        picker.choices.insert(0, (&first, true));
        picker.menu = None;
        tree.diff(&picker as &dyn Widget<Message, Theme, Renderer>);
        assert!(send(
            &mut picker,
            &mut tree,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            true
        )
        .is_empty());
        assert_eq!(
            send(
                &mut picker,
                &mut tree,
                key(keyboard::key::Named::Enter),
                false
            ),
            vec![Message::Select(r"C:\Selected\app.exe".into())]
        );
    }
}
