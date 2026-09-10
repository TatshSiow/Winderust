//! Native pick-list field with a menu that distinguishes selection from hover.
use super::{design, widgets};
use iced::advanced::{
    layout, renderer,
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};
use iced::widget::{column, container, row, scrollable, text, PickList};
use iced::{
    keyboard, mouse, overlay, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
};
use std::rc::Rc;

type Field<'a, T, M> = PickList<'a, T, Vec<T>, T, M>;

pub(super) struct Select<'a, T: ToString + PartialEq + Clone, M> {
    field: Field<'a, T, M>,
    options: Vec<T>,
    selected: Option<usize>,
    choose: Rc<dyn Fn(T) -> M + 'a>,
    menu: Option<(Option<usize>, Element<'a, usize>)>,
    open: bool,
}

impl<'a, T: ToString + PartialEq + Clone + 'a, M: Clone + 'a> Select<'a, T, M> {
    pub(super) fn new(options: Vec<T>, selected: Option<T>, choose: impl Fn(T) -> M + 'a) -> Self {
        let index = selected
            .as_ref()
            .and_then(|value| options.iter().position(|option| option == value));
        let choose: Rc<dyn Fn(T) -> M> = Rc::new(choose);
        let callback = choose.clone();
        let field =
            iced::widget::pick_list(options.clone(), selected, move |value| callback(value))
                .text_size(design::typography::BODY)
                .padding(iced::Padding {
                    right: (design::SELECT_PADDING[1] as u32
                        + design::ICON_SIZE
                        + design::space::SMALL) as f32,
                    ..iced::Padding::from(design::SELECT_PADDING)
                })
                .handle(iced::widget::pick_list::Handle::None)
                .style(widgets::select_field);
        Self {
            field,
            options,
            selected: index,
            choose,
            menu: None,
            open: false,
        }
    }
    pub(super) fn width(mut self, width: impl Into<Length>) -> Self {
        self.field = self.field.width(width);
        self
    }
    pub(super) fn placeholder(mut self, label: impl Into<String>) -> Self {
        self.field = self.field.placeholder(label);
        self
    }
    fn menu(&self, keyboard: Option<usize>) -> Element<'a, usize> {
        let mut items = column![].spacing(2);
        for (index, option) in self.options.iter().enumerate() {
            let selected = self.selected == Some(index);
            let marker = container(iced::widget::Space::new())
                .width(3)
                .height(18)
                .style(move |theme: &Theme| iced::widget::container::Style {
                    background: selected.then(|| theme.palette().primary.into()),
                    border: iced::border::rounded(2),
                    ..Default::default()
                });
            items = items.push(
                widgets::button(
                    row![
                        marker,
                        text(option.to_string())
                            .style(text::base)
                            .wrapping(iced::widget::text::Wrapping::None)
                    ]
                    .spacing(9)
                    .height(Length::Fill)
                    .align_y(iced::Center),
                )
                .width(Length::Fill)
                .height(40)
                .clip(true)
                .padding([0, 0])
                .style(move |theme, status| {
                    if selected {
                        widgets::selected_control(theme, status)
                    } else if keyboard == Some(index) {
                        widgets::quiet(theme, iced::widget::button::Status::Hovered)
                    } else {
                        widgets::quiet(theme, status)
                    }
                })
                .on_press(index),
            );
        }
        container(scrollable(items).id("select-options").height(Length::Fill))
            .padding(8)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|theme| {
                let menu = widgets::select_menu(theme);
                iced::widget::container::Style {
                    background: Some(menu.background),
                    border: menu.border,
                    ..Default::default()
                }
            })
            .into()
    }
}

impl<'a, T: ToString + PartialEq + Clone + 'a, M: Clone + 'a> Widget<M, Theme, Renderer>
    for Select<'a, T, M>
{
    fn size(&self) -> Size<Length> {
        self.field.size()
    }
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Option<usize>>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(None::<usize>)
    }
    fn children(&self) -> Vec<Tree> {
        vec![
            Tree::new(&self.field as &dyn Widget<M, Theme, Renderer>),
            Tree::empty(),
        ]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.children[0].diff(&self.field as &dyn Widget<M, Theme, Renderer>);
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.field.layout(&mut tree.children[0], renderer, limits)
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
        self.field.draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        use iced::advanced::svg::Renderer as _;
        let bounds = layout.bounds();
        let size = design::ICON_SIZE as f32;
        let icon = crate::ui::assets::iced_icon("icons/chevron-down.svg")
            .expect("Every UI icon is bundled");
        renderer.draw_svg(
            iced::advanced::svg::Svg::new(icon)
                .color(widgets::muted_color(theme))
                .rotation(if self.open { std::f32::consts::PI } else { 0.0 }),
            Rectangle {
                x: bounds.x + bounds.width - design::SELECT_PADDING[1] as f32 - size,
                y: bounds.center_y() - size / 2.0,
                width: size,
                height: size,
            },
            *viewport,
        );
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.field
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
        self.field.update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.field
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        if self
            .field
            .overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
            .is_none()
        {
            *tree.state.downcast_mut::<Option<usize>>() = None;
            self.menu = None;
            self.open = false;
            return None;
        }
        let reveal = self.menu.is_none();
        self.open = true;
        let keyboard = *tree.state.downcast_ref::<Option<usize>>();
        if self.menu.as_ref().is_none_or(|(old, _)| *old != keyboard) {
            self.menu = Some((keyboard, self.menu(keyboard)));
        }
        let (_, content) = self.menu.as_mut()?;
        let (field_tree, menu_tree) = tree.children.split_at_mut(1);
        menu_tree[0].diff(&*content);
        Some(overlay::Element::new(Box::new(Menu {
            field: &self.field,
            field_tree: &mut field_tree[0],
            tree: &mut menu_tree[0],
            content,
            keyboard: tree.state.downcast_mut(),
            selected: self.selected,
            options: &self.options,
            choose: &*self.choose,
            anchor: layout.bounds() + translation,
            reveal,
        })))
    }
}
impl<'a, T: ToString + PartialEq + Clone + 'a, M: Clone + 'a> From<Select<'a, T, M>>
    for Element<'a, M>
{
    fn from(select: Select<'a, T, M>) -> Self {
        Element::new(select)
    }
}

struct Menu<'a, 'b, T: ToString + PartialEq + Clone, M> {
    field: &'b Field<'a, T, M>,
    field_tree: &'b mut Tree,
    tree: &'b mut Tree,
    content: &'b mut Element<'a, usize>,
    keyboard: &'b mut Option<usize>,
    selected: Option<usize>,
    options: &'b [T],
    choose: &'b (dyn Fn(T) -> M + 'a),
    anchor: Rectangle,
    reveal: bool,
}
impl<'a, T: ToString + PartialEq + Clone + 'a, M: Clone + 'a> Menu<'a, '_, T, M> {
    fn close(&mut self, shell: &mut Shell<'_, M>) {
        *self.field_tree = Tree::new(self.field as &dyn Widget<M, Theme, Renderer>);
        *self.keyboard = None;
        shell.invalidate_layout();
        shell.request_redraw();
    }
}
impl<'a, T: ToString + PartialEq + Clone + 'a, M: Clone + 'a>
    iced::advanced::Overlay<M, Theme, Renderer> for Menu<'a, '_, T, M>
{
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let below = (bounds.height - self.anchor.y - self.anchor.height - 4.0).max(0.0);
        let above = (self.anchor.y - 4.0).max(0.0);
        let height = (self.options.len() as f32 * 42.0 + 14.0)
            .min(400.0)
            .min(below.max(above));
        let y = if below >= height {
            self.anchor.y + self.anchor.height + 4.0
        } else {
            self.anchor.y - height - 4.0
        };
        let node = self
            .content
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
            ));
        if self.reveal {
            if let Some(index) = self.selected {
                let mut operation = iced::advanced::widget::operation::scrollable::scroll_to(
                    iced::advanced::widget::Id::new("select-options"),
                    iced::widget::scrollable::AbsoluteOffset {
                        x: None,
                        y: Some((index as f32 * 42.0 - height / 2.0).max(0.0)),
                    },
                );
                self.content.as_widget_mut().operate(
                    self.tree,
                    Layout::new(&node),
                    renderer,
                    &mut operation,
                );
            }
            self.reveal = false;
        }
        node
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
        if matches!(event, Event::Window(iced::window::Event::Unfocused)) {
            self.close(shell);
            return;
        }
        if let Event::Touch(iced::touch::Event::FingerPressed { position, .. }) = event {
            if !layout.bounds().contains(*position) {
                self.close(shell);
                if self.anchor.contains(*position) {
                    shell.capture_event();
                }
                return;
            }
        }
        if matches!(event, Event::Mouse(mouse::Event::CursorMoved { .. }))
            && self.keyboard.take().is_some()
        {
            shell.invalidate_layout();
            shell.request_redraw();
        }
        if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)))
            && !cursor.is_over(layout.bounds())
        {
            self.close(shell);
            if cursor.is_over(self.anchor) {
                shell.capture_event();
            }
            return;
        }
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key),
            ..
        }) = event
        {
            use keyboard::key::Named;
            match key {
                Named::Escape | Named::Tab => {
                    self.close(shell);
                    if *key == Named::Escape {
                        shell.capture_event();
                    }
                    return;
                }
                Named::ArrowUp | Named::ArrowDown | Named::Home | Named::End
                    if !self.options.is_empty() =>
                {
                    let current = self.keyboard.or(self.selected);
                    let next = if *key == Named::Home {
                        0
                    } else if *key == Named::End {
                        self.options.len() - 1
                    } else if *key == Named::ArrowDown {
                        current.map_or(0, |i| (i + 1).min(self.options.len() - 1))
                    } else {
                        current.map_or(self.options.len() - 1, |i| i.saturating_sub(1))
                    };
                    *self.keyboard = Some(next);
                    let mut operation = iced::advanced::widget::operation::scrollable::scroll_to(
                        iced::advanced::widget::Id::new("select-options"),
                        iced::widget::scrollable::AbsoluteOffset {
                            x: None,
                            y: Some((next as f32 * 42.0 - layout.bounds().height / 2.0).max(0.0)),
                        },
                    );
                    self.content.as_widget_mut().operate(
                        self.tree,
                        layout,
                        renderer,
                        &mut operation,
                    );
                    shell.invalidate_layout();
                    shell.request_redraw();
                    shell.capture_event();
                    return;
                }
                Named::Enter => {
                    if let Some(option) = self
                        .keyboard
                        .or(self.selected)
                        .and_then(|i| self.options.get(i))
                    {
                        shell.publish((self.choose)(option.clone()));
                    }
                    self.close(shell);
                    shell.capture_event();
                    return;
                }
                _ => {}
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
        shell.merge(local, |index| (self.choose)(self.options[index].clone()));
        if chosen {
            self.close(shell);
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

    #[test]
    fn menu_selection_and_dismissal_preserve_the_selected_value() {
        let renderer = Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let select = Select::new(vec!["System", "Light", "Dark"], Some("System"), |value| {
            value
        });
        let mut field_tree = Tree::new(&select.field as &dyn Widget<&str, Theme, Renderer>);
        let mut content = select.menu(None);
        let mut tree = Tree::new(&content);
        let mut keyboard = None;
        let mut menu = Menu {
            field: &select.field,
            field_tree: &mut field_tree,
            tree: &mut tree,
            content: &mut content,
            keyboard: &mut keyboard,
            selected: select.selected,
            options: &select.options,
            choose: &*select.choose,
            anchor: Rectangle {
                x: 20.0,
                y: 220.0,
                width: 240.0,
                height: 32.0,
            },
            reveal: true,
        };
        let node = menu.layout(&renderer, Size::new(400.0, 280.0));
        assert!(node.bounds().y >= 0.0);
        assert!(node.bounds().y + node.bounds().height <= 220.0);
        let layout = Layout::new(&node);
        let position = iced::Point::new(layout.bounds().x + 40.0, layout.bounds().y + 70.0);
        let mut messages = Vec::new();
        let mut clipboard = iced::advanced::clipboard::Null;
        menu.update(
            &Event::Mouse(mouse::Event::CursorMoved { position }),
            layout,
            mouse::Cursor::Available(position),
            &renderer,
            &mut clipboard,
            &mut Shell::new(&mut messages),
        );
        assert_eq!(menu.selected, Some(0));
        assert!(messages.is_empty());
        for key in [keyboard::key::Named::End, keyboard::key::Named::Enter] {
            menu.update(
                &Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(key),
                    modified_key: keyboard::Key::Named(key),
                    physical_key: keyboard::key::Physical::Unidentified(
                        keyboard::key::NativeCode::Unidentified,
                    ),
                    location: keyboard::Location::Standard,
                    modifiers: keyboard::Modifiers::empty(),
                    text: None,
                    repeat: false,
                }),
                layout,
                mouse::Cursor::Unavailable,
                &renderer,
                &mut clipboard,
                &mut Shell::new(&mut messages),
            );
        }
        assert_eq!(messages, vec!["Dark"]);
        messages.clear();
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            menu.update(
                &Event::Mouse(event),
                layout,
                mouse::Cursor::Available(position),
                &renderer,
                &mut clipboard,
                &mut Shell::new(&mut messages),
            );
        }
        assert_eq!(messages, vec!["Light"]);
        assert_eq!(*menu.keyboard, None);
        messages.clear();
        menu.update(
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            layout,
            mouse::Cursor::Available(iced::Point::new(390.0, 270.0)),
            &renderer,
            &mut clipboard,
            &mut Shell::new(&mut messages),
        );
        assert!(messages.is_empty());
        assert_eq!(menu.selected, Some(0));
    }
}
