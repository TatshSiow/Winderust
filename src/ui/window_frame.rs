//! Themed window chrome; Iced retains ownership of native window operations.
use super::{assets, design, navigation, widgets};
use iced::widget::{column, container, mouse_area, row, text, tooltip, Space, Stack};
use iced::{mouse, window, Element, Fill, Theme};
use rust_i18n::t;

const HEIGHT: u32 = 36;
const BUTTON_WIDTH: u32 = 46;
const EDGE: u32 = 5;
const CORNER: u32 = 10;

#[derive(Debug, Clone, Copy)]
pub(super) enum Event {
    Drag,
    Minimize,
    ToggleMaximize,
    Resize(window::Direction),
    Close,
}

pub(super) fn view<'a, M: Clone + 'a>(
    content: Element<'a, M>,
    maximized: bool,
    message: impl Fn(Event) -> M + Copy + 'a,
) -> Element<'a, M> {
    let caption = mouse_area(
        container(
            row![
                iced::widget::image(assets::logo()).width(18).height(18),
                text(t!("app.name").to_string())
                    .size(design::typography::SECONDARY)
                    .style(|theme: &Theme| text::Style {
                        color: Some(theme.palette().text)
                    })
            ]
            .spacing(design::space::SMALL)
            .align_y(iced::Center),
        )
        .padding([0, design::space::LARGE as u16])
        .width(Fill)
        .height(HEIGHT)
        .align_y(iced::Center),
    )
    .on_press(message(Event::Drag))
    .on_double_click(message(Event::ToggleMaximize));
    let title = container(row![
        caption,
        control("icons/minus.svg", "window.minimize", Event::Minimize).map(message),
        control(
            if maximized {
                "icons/copy.svg"
            } else {
                "icons/square.svg"
            },
            if maximized {
                "window.restore"
            } else {
                "window.maximize"
            },
            Event::ToggleMaximize,
        )
        .map(message),
        control("icons/x.svg", "common.close", Event::Close).map(message),
    ])
    .width(Fill)
    .style(widgets::navigation_surface);
    let frame = column![title, widgets::settings_divider(), content]
        .width(Fill)
        .height(Fill);
    let mut layers = Stack::new().push(frame);
    if !maximized {
        use window::Direction::*;
        let edge = |direction, width, height| {
            mouse_area(Space::new().width(width).height(height))
                .on_press(message(Event::Resize(direction)))
                .interaction(match direction {
                    West | East => mouse::Interaction::ResizingHorizontally,
                    North | South => mouse::Interaction::ResizingVertically,
                    NorthWest | SouthEast => mouse::Interaction::ResizingDiagonallyDown,
                    NorthEast | SouthWest => mouse::Interaction::ResizingDiagonallyUp,
                })
        };
        layers = layers
            .push(
                row![
                    edge(West, EDGE.into(), Fill),
                    Space::new().width(Fill),
                    edge(East, EDGE.into(), Fill),
                ]
                .height(Fill),
            )
            .push(column![
                row![
                    edge(NorthWest, CORNER.into(), EDGE.into()),
                    edge(North, Fill, EDGE.into()),
                    edge(NorthEast, CORNER.into(), EDGE.into()),
                ],
                Space::new().height(Fill),
                row![
                    edge(SouthWest, CORNER.into(), EDGE.into()),
                    edge(South, Fill, EDGE.into()),
                    edge(SouthEast, CORNER.into(), EDGE.into()),
                ],
            ]);
    }
    layers.into()
}

fn control(icon: &'static str, label: &str, event: Event) -> Element<'static, Event> {
    tooltip(
        widgets::button(
            container(navigation::glyph(icon))
                .center_x(Fill)
                .center_y(Fill),
        )
        .width(BUTTON_WIDTH)
        .height(HEIGHT)
        .padding(0)
        .style(move |theme, status| control_style(theme, status, matches!(event, Event::Close)))
        .on_press(event),
        container(text(t!(label).to_string()))
            .padding(design::space::SMALL as u16)
            .style(widgets::surface),
        tooltip::Position::Bottom,
    )
    .into()
}

fn control_style(
    theme: &Theme,
    status: iced::widget::button::Status,
    close: bool,
) -> iced::widget::button::Style {
    let mut style = widgets::quiet(theme, status);
    style.border.radius = 0.0.into();
    if close {
        style.background = match status {
            iced::widget::button::Status::Hovered => {
                Some(theme.palette().danger.scale_alpha(0.25).into())
            }
            iced::widget::button::Status::Pressed => {
                Some(theme.palette().danger.scale_alpha(0.4).into())
            }
            _ => None,
        };
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::{layout, widget::Tree, Layout, Shell};
    use iced::{Event as Input, Point, Size};

    #[derive(Debug, Clone)]
    enum Message {
        Chrome(Event),
        Body,
    }

    fn click(maximized: bool, point: Point, count: usize) -> (Vec<String>, mouse::Interaction) {
        let renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let mut view = view(
            widgets::button(Space::new())
                .width(Fill)
                .height(Fill)
                .on_press(Message::Body)
                .into(),
            maximized,
            Message::Chrome,
        );
        let mut tree = Tree::new(&view);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(900.0, 500.0)),
        );
        let cursor = mouse::Cursor::Available(point);
        let interaction = view.as_widget().mouse_interaction(
            &tree,
            Layout::new(&node),
            cursor,
            &node.bounds(),
            &renderer,
        );
        let mut messages = Vec::new();
        for _ in 0..count {
            for event in [
                Input::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Input::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ] {
                view.as_widget_mut().update(
                    &mut tree,
                    &event,
                    Layout::new(&node),
                    cursor,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut Shell::new(&mut messages),
                    &node.bounds(),
                );
            }
        }
        (
            messages
                .into_iter()
                .map(|message| match message {
                    Message::Chrome(event) => format!("{event:?}"),
                    Message::Body => "Body".into(),
                })
                .collect(),
            interaction,
        )
    }

    #[test]
    fn caption_and_controls_dispatch_only_their_own_actions() {
        for maximized in [false, true] {
            for (x, y, action) in [
                (100.0, 18.0, "Drag"),
                (785.0, 18.0, "Minimize"),
                (831.0, 18.0, "ToggleMaximize"),
                (877.0, 18.0, "Close"),
                (400.0, 100.0, "Body"),
            ] {
                assert_eq!(click(maximized, Point::new(x, y), 1).0, [action]);
            }
            assert_eq!(
                click(maximized, Point::new(100.0, 18.0), 2).0,
                ["Drag", "Drag", "ToggleMaximize"]
            );
        }
    }

    #[test]
    fn resize_edges_capture_input_only_while_restored() {
        for (x, y, direction, cursor) in [
            (
                1.0,
                1.0,
                "NorthWest",
                mouse::Interaction::ResizingDiagonallyDown,
            ),
            (450.0, 1.0, "North", mouse::Interaction::ResizingVertically),
            (
                899.0,
                1.0,
                "NorthEast",
                mouse::Interaction::ResizingDiagonallyUp,
            ),
            (1.0, 250.0, "West", mouse::Interaction::ResizingHorizontally),
            (
                899.0,
                250.0,
                "East",
                mouse::Interaction::ResizingHorizontally,
            ),
            (
                1.0,
                499.0,
                "SouthWest",
                mouse::Interaction::ResizingDiagonallyUp,
            ),
            (
                450.0,
                499.0,
                "South",
                mouse::Interaction::ResizingVertically,
            ),
            (
                899.0,
                499.0,
                "SouthEast",
                mouse::Interaction::ResizingDiagonallyDown,
            ),
        ] {
            let (messages, interaction) = click(false, Point::new(x, y), 1);
            assert_eq!(messages, [format!("Resize({direction})")]);
            assert_eq!(interaction, cursor);
            assert!(!click(true, Point::new(x, y), 1)
                .0
                .iter()
                .any(|message| message.starts_with("Resize")));
        }
    }

    #[test]
    fn title_bar_renders_the_current_theme_surface() {
        use iced::advanced::renderer::{Headless as _, Renderer as _};
        for (name, theme) in [
            (
                "light",
                Theme::custom_with_fn(
                    "Light",
                    design::palette(true, None),
                    design::extended_palette,
                ),
            ),
            (
                "dark",
                Theme::custom_with_fn(
                    "Dark",
                    design::palette(false, None),
                    design::extended_palette,
                ),
            ),
            (
                "amoled",
                design::amoled_theme(Some(iced::Color::from_rgb8(70, 190, 140))),
            ),
        ] {
            let mut renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
            let mut view = view(
                Space::new().width(Fill).height(Fill).into(),
                false,
                std::convert::identity,
            );
            let mut tree = Tree::new(&view);
            let bounds = iced::Rectangle::with_size(Size::new(900.0, 140.0));
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, bounds.size()),
            );
            renderer.reset(bounds);
            view.as_widget().draw(
                &tree,
                &mut renderer,
                &theme,
                &iced::advanced::renderer::Style::default(),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &bounds,
            );
            let pixels = renderer.screenshot(Size::new(900, 140), 1.0, theme.palette().background);
            let expected = theme
                .extended_palette()
                .background
                .weaker
                .color
                .into_rgba8();
            assert_eq!(&pixels[(18 * 900 + 450) * 4..][..4], &expected, "{name}");
            std::fs::create_dir_all("target/window-frame").unwrap();
            image::save_buffer(
                format!("target/window-frame/{name}.png"),
                &pixels,
                900,
                140,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
}
