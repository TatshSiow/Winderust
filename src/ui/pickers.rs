//! Compact picker content; iced_aw places popups and the modal guard shields the workspace.
use super::{design, power_rules, settings_pages, widgets};
use iced::widget::{canvas, column, container, row, text};
use iced::{mouse, Color, Element, Fill, Point, Rectangle, Renderer, Size, Theme};
use iced_aw::core::color::Hsv;
use rust_i18n::t;

#[derive(Default)]
pub(super) struct TimeDraft {
    pub hour: String,
    pub minute: String,
    pub part: TimePart,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum TimePart {
    #[default]
    Hour,
    Minute,
}
impl TimeDraft {
    pub fn new(time: chrono::NaiveTime) -> Self {
        Self {
            hour: time.format("%H").to_string(),
            minute: time.format("%M").to_string(),
            part: TimePart::Hour,
        }
    }
    pub fn time(&self) -> Option<chrono::NaiveTime> {
        chrono::NaiveTime::from_hms_opt(self.hour.parse().ok()?, self.minute.parse().ok()?, 0)
    }
    pub fn select(&mut self, part: TimePart, value: u32, complete: bool) {
        match part {
            TimePart::Hour if value < 24 => self.hour = format!("{value:02}"),
            TimePart::Minute if value < 60 => self.minute = format!("{value:02}"),
            _ => return,
        }
        self.part = if complete && part == TimePart::Hour {
            TimePart::Minute
        } else {
            part
        };
    }
}

fn popup<'a, M: Clone + 'a>(
    title: String,
    content: Element<'a, M>,
    cancel: M,
    submit: Option<M>,
) -> Element<'a, M> {
    iced::widget::opaque(
        container(
            column![
                widgets::heading(title, design::typography::BODY),
                content,
                widgets::settings_divider(),
                row![
                    iced::widget::Space::new().width(Fill),
                    widgets::action_button(t!("common.cancel").to_string())
                        .style(widgets::tertiary_button)
                        .on_press(cancel),
                    widgets::action_button(t!("common.ok").to_string())
                        .style(widgets::primary_button)
                        .on_press_maybe(submit),
                ]
                .spacing(design::space::SMALL),
            ]
            .spacing(design::space::MEDIUM),
        )
        .padding(design::space::MEDIUM as u16)
        .width(Fill)
        .style(widgets::tooltip_surface),
    )
}

pub(super) fn time(draft: &TimeDraft, start: bool) -> Element<'_, power_rules::Message> {
    use power_rules::Message;
    popup(
        t!(if start {
            "by_time.start"
        } else {
            "by_time.end"
        })
        .to_string(),
        column![
            container(row![
                widgets::panel_tab(
                    t!("by_time.hour").to_string(),
                    draft.part == TimePart::Hour,
                    Message::PickerPart(TimePart::Hour)
                ),
                widgets::panel_tab(
                    t!("by_time.minute").to_string(),
                    draft.part == TimePart::Minute,
                    Message::PickerPart(TimePart::Minute)
                ),
            ])
            .style(widgets::tab_bar),
            canvas::Canvas::new(TimeDial {
                part: draft.part,
                value: match draft.part {
                    TimePart::Hour => draft.hour.parse::<u32>().ok().filter(|v| *v < 24),
                    TimePart::Minute => draft.minute.parse::<u32>().ok().filter(|v| *v < 60),
                }
                .unwrap_or(0),
            })
            .width(Fill)
            .height(216),
            row![
                widgets::stepper(&draft.hour, 0..=23, 1, Some(Message::PickerHour)),
                widgets::stepper(&draft.minute, 0..=59, 1, Some(Message::PickerMinute)),
            ]
            .spacing(design::space::MEDIUM),
        ]
        .spacing(design::space::MEDIUM)
        .into(),
        Message::CancelTimePicker,
        draft.time().map(|time| Message::TimePicked(start, time)),
    )
}

struct TimeDial {
    part: TimePart,
    value: u32,
}
#[derive(Default)]
struct TimeDialState {
    dragging: bool,
    value: u32,
}
impl TimeDial {
    fn radii(size: Size) -> (Point, f32, f32) {
        let outer = (size.width.min(size.height) / 2.0 - 24.0).max(1.0);
        (
            Point::new(size.width / 2.0, size.height / 2.0),
            outer,
            outer * 0.67,
        )
    }
    fn point(center: Point, radius: f32, slot: u32, count: u32) -> Point {
        let angle = slot as f32 / count as f32 * std::f32::consts::TAU;
        Point::new(
            center.x + radius * angle.sin(),
            center.y - radius * angle.cos(),
        )
    }
    fn value_at(&self, size: Size, point: Point) -> u32 {
        let (center, outer, inner) = Self::radii(size);
        let angle = (point.x - center.x)
            .atan2(center.y - point.y)
            .rem_euclid(std::f32::consts::TAU);
        let count = if self.part == TimePart::Hour { 12 } else { 60 };
        let slot = (angle / std::f32::consts::TAU * count as f32).round() as u32 % count;
        if self.part == TimePart::Minute {
            return slot;
        }
        if center.distance(point) < (outer + inner) / 2.0 {
            if slot == 0 {
                0
            } else {
                slot + 12
            }
        } else if slot == 0 {
            12
        } else {
            slot
        }
    }
    fn selected(&self, size: Size) -> Point {
        let (center, outer, inner) = Self::radii(size);
        let radius = if self.part == TimePart::Hour && (self.value == 0 || self.value > 12) {
            inner
        } else {
            outer
        };
        Self::point(
            center,
            radius,
            self.value,
            if self.part == TimePart::Hour { 12 } else { 60 },
        )
    }
}
impl canvas::Program<power_rules::Message> for TimeDial {
    type State = TimeDialState;
    fn update(
        &self,
        state: &mut TimeDialState,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<power_rules::Message>> {
        let position = match event {
            canvas::Event::Touch(
                iced::touch::Event::FingerPressed { position, .. }
                | iced::touch::Event::FingerMoved { position, .. },
            ) => Some(*position),
            _ => cursor.position(),
        }
        .map(|point| Point::new(point.x - bounds.x, point.y - bounds.y));
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | canvas::Event::Touch(iced::touch::Event::FingerPressed { .. }) => {
                let point = position?;
                let (center, outer, _) = Self::radii(bounds.size());
                if !bounds.contains(Point::new(point.x + bounds.x, point.y + bounds.y))
                    || center.distance(point) > outer + 16.0
                {
                    return None;
                }
                state.dragging = true;
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. })
            | canvas::Event::Touch(iced::touch::Event::FingerMoved { .. })
                if state.dragging => {}
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | canvas::Event::Touch(iced::touch::Event::FingerLifted { .. })
                if state.dragging =>
            {
                state.dragging = false;
                return Some(
                    iced::widget::Action::publish(power_rules::Message::PickerDial(
                        self.part,
                        state.value,
                        true,
                    ))
                    .and_capture(),
                );
            }
            canvas::Event::Window(iced::window::Event::Unfocused)
            | canvas::Event::Touch(iced::touch::Event::FingerLost { .. }) => {
                state.dragging = false;
                return None;
            }
            _ => return None,
        }
        state.value = self.value_at(bounds.size(), position?);
        Some(
            iced::widget::Action::publish(power_rules::Message::PickerDial(
                self.part,
                state.value,
                false,
            ))
            .and_capture(),
        )
    }
    fn draw(
        &self,
        _: &TimeDialState,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();
        let (center, outer, inner) = Self::radii(bounds.size());
        frame.fill(
            &canvas::Path::circle(center, outer + 16.0),
            palette.background.base.color,
        );
        let selected = self.selected(bounds.size());
        if self.part == TimePart::Minute {
            for minute in 0..60 {
                frame.stroke(
                    &canvas::Path::line(
                        Self::point(center, outer + 11.0, minute, 60),
                        Self::point(
                            center,
                            outer + if minute % 5 == 0 { 6.0 } else { 8.0 },
                            minute,
                            60,
                        ),
                    ),
                    canvas::Stroke::default().with_color(palette.background.strong.color),
                );
            }
        }
        frame.stroke(
            &canvas::Path::line(center, selected),
            canvas::Stroke::default()
                .with_width(2.0)
                .with_color(palette.primary.base.color),
        );
        frame.fill(
            &canvas::Path::circle(center, 3.0),
            palette.primary.base.color,
        );
        frame.fill(
            &canvas::Path::circle(selected, 14.0),
            palette.primary.base.color,
        );
        let count = if self.part == TimePart::Hour { 24 } else { 12 };
        for index in 0..count {
            let value = if self.part == TimePart::Hour {
                index
            } else {
                index * 5
            };
            let radius = if self.part == TimePart::Hour && (value == 0 || value > 12) {
                inner
            } else {
                outer
            };
            let point = Self::point(
                center,
                radius,
                value,
                if self.part == TimePart::Hour { 12 } else { 60 },
            );
            if value == self.value || point.distance(selected) < 25.0 {
                continue;
            }
            frame.fill_text(canvas::Text {
                content: if value == 0 || self.part == TimePart::Minute {
                    format!("{value:02}")
                } else {
                    value.to_string()
                },
                position: point,
                color: palette.background.base.text,
                size: 13.0.into(),
                font: design::typography::FONT,
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Center,
                ..Default::default()
            });
        }
        frame.fill_text(canvas::Text {
            content: format!("{:02}", self.value),
            position: selected,
            color: palette.primary.base.text,
            size: 14.0.into(),
            font: design::typography::FONT,
            align_x: iced::alignment::Horizontal::Center.into(),
            align_y: iced::alignment::Vertical::Center,
            ..Default::default()
        });
        vec![frame.into_geometry()]
    }
    fn mouse_interaction(
        &self,
        state: &TimeDialState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.dragging || cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }
}

pub(super) struct ColorDraft {
    hue: u16,
    saturation: f32,
    value: f32,
    pub hex: String,
}
impl ColorDraft {
    pub fn new(rgb: u32) -> Self {
        let color = settings_pages::rgb(rgb);
        let max = color.r.max(color.g).max(color.b);
        let min = color.r.min(color.g).min(color.b);
        let delta = max - min;
        let hue = if delta == 0.0 {
            0.0
        } else if max == color.r {
            60.0 * ((color.g - color.b) / delta).rem_euclid(6.0)
        } else if max == color.g {
            60.0 * ((color.b - color.r) / delta + 2.0)
        } else {
            60.0 * ((color.r - color.g) / delta + 4.0)
        };
        Self {
            hue: hue.round() as u16 % 360,
            saturation: if max == 0.0 { 0.0 } else { delta / max },
            value: max,
            hex: format!("#{rgb:06X}"),
        }
    }
    pub fn color(&self) -> Color {
        // Keep exact RGB values from hex entry; HSV uses integer degrees upstream.
        settings_pages::parse_color(&self.hex)
            .map(settings_pages::rgb)
            .unwrap_or_else(|| Hsv::from_hsv(self.hue, self.saturation, self.value).into())
    }
    fn sync_hex(&mut self) {
        let color: Color = Hsv::from_hsv(self.hue, self.saturation, self.value).into();
        let [r, g, b, _] = color.into_rgba8();
        self.hex = format!("#{r:02X}{g:02X}{b:02X}");
    }
    pub fn set_hue(&mut self, hue: u16) {
        self.hue = hue.min(359);
        self.sync_hex();
    }
    pub fn set_sv(&mut self, saturation: f32, value: f32) {
        self.saturation = saturation.clamp(0.0, 1.0);
        self.value = value.clamp(0.0, 1.0);
        self.sync_hex();
    }
    pub fn set_hex(&mut self, hex: String) {
        if let Some(color) = settings_pages::parse_color(&hex) {
            *self = Self::new(color);
        }
        self.hex = hex;
    }
}

pub(super) fn color(draft: &ColorDraft) -> Element<'_, settings_pages::Message> {
    use settings_pages::Message;
    let body = column![
        canvas::Canvas::new(ColorSquare {
            hue: draft.hue,
            saturation: draft.saturation,
            value: draft.value
        })
        .width(Fill)
        .height(144),
        row![
            text(t!("accent.hue").to_string()).width(92),
            widgets::slider(0..=359u16, draft.hue, Message::PickerHue)
        ]
        .align_y(iced::Center),
        row![
            text(t!("accent.saturation").to_string()).width(92),
            widgets::slider(
                0..=100u16,
                (draft.saturation * 100.0).round() as u16,
                move |s| Message::PickerSv(s as f32 / 100.0, draft.value)
            )
        ]
        .align_y(iced::Center),
        row![
            text(t!("accent.brightness").to_string()).width(92),
            widgets::slider(0..=100u16, (draft.value * 100.0).round() as u16, move |v| {
                Message::PickerSv(draft.saturation, v as f32 / 100.0)
            })
        ]
        .align_y(iced::Center),
        row![
            container(iced::widget::Space::new())
                .width(32)
                .height(32)
                .style(move |theme| iced::widget::container::Style {
                    background: Some(draft.color().into()),
                    ..widgets::surface(theme)
                }),
            widgets::text_input("#RRGGBB", &draft.hex)
                .on_input(Message::PickerHex)
                .width(Fill),
        ]
        .spacing(design::space::SMALL),
    ]
    .spacing(design::space::MEDIUM);
    popup(
        t!("accent.choose_color").to_string(),
        body.into(),
        Message::CancelColorPicker,
        settings_pages::parse_color(&draft.hex).map(|_| Message::ColorPicked(draft.color())),
    )
}

struct ColorSquare {
    hue: u16,
    saturation: f32,
    value: f32,
}
impl canvas::Program<settings_pages::Message> for ColorSquare {
    type State = bool;
    fn update(
        &self,
        dragging: &mut bool,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<settings_pages::Message>> {
        let position = match event {
            canvas::Event::Touch(
                iced::touch::Event::FingerPressed { position, .. }
                | iced::touch::Event::FingerMoved { position, .. },
            ) => Some(*position),
            _ => cursor.position(),
        };
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | canvas::Event::Touch(iced::touch::Event::FingerPressed { .. })
                if position.is_some_and(|p| bounds.contains(p)) =>
            {
                *dragging = true
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | canvas::Event::Touch(
                iced::touch::Event::FingerLifted { .. } | iced::touch::Event::FingerLost { .. },
            ) => {
                let was_dragging = std::mem::take(dragging);
                return was_dragging.then(iced::widget::Action::capture);
            }
            canvas::Event::Window(iced::window::Event::Unfocused) => {
                *dragging = false;
                return None;
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. })
            | canvas::Event::Touch(iced::touch::Event::FingerMoved { .. }) => {}
            _ => return None,
        }
        if !*dragging {
            return None;
        }
        position.map(|p| {
            iced::widget::Action::publish(settings_pages::Message::PickerSv(
                ((p.x - bounds.x) / bounds.width.max(1.0)).clamp(0.0, 1.0),
                (1.0 - (p.y - bounds.y) / bounds.height.max(1.0)).clamp(0.0, 1.0),
            ))
            .and_capture()
        })
    }
    fn draw(
        &self,
        _: &bool,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // Two gradient fills replace the upstream picker's per-pixel rectangle grid.
        let hue: Color = Hsv::from_hsv(self.hue, 1.0, 1.0).into();
        let horizontal =
            canvas::gradient::Linear::new(Point::ORIGIN, Point::new(bounds.width, 0.0))
                .add_stop(0.0, Color::WHITE)
                .add_stop(1.0, hue);
        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            canvas::Fill::from(canvas::Gradient::from(horizontal)),
        );
        let vertical = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(0.0, bounds.height))
            .add_stop(0.0, Color::TRANSPARENT)
            .add_stop(1.0, Color::BLACK);
        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            canvas::Fill::from(canvas::Gradient::from(vertical)),
        );
        let selected = Point::new(
            (self.saturation * bounds.width).clamp(5.0, bounds.width.max(10.0) - 5.0),
            ((1.0 - self.value) * bounds.height).clamp(5.0, bounds.height.max(10.0) - 5.0),
        );
        let marker = canvas::Path::circle(selected, 4.0);
        frame.stroke(
            &marker,
            canvas::Stroke::default()
                .with_color(Color::BLACK)
                .with_width(3.0),
        );
        frame.stroke(
            &marker,
            canvas::Stroke::default()
                .with_color(Color::WHITE)
                .with_width(1.5),
        );
        vec![frame.into_geometry()]
    }
    fn mouse_interaction(
        &self,
        dragging: &bool,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if *dragging || cursor.is_over(bounds) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas::Program;

    #[test]
    fn picker_drafts_keep_exact_hex_and_validate_time_bounds() {
        for rgb in [0, 0xffffff, 0x123456, 0xff0000, 0x00ff00, 0x0000ff] {
            let draft = ColorDraft::new(rgb);
            assert_eq!(draft.color(), settings_pages::rgb(rgb));
        }
        let mut color = ColorDraft::new(0xff0000);
        color.set_hue(120);
        assert_eq!(color.color(), Color::from_rgb(0.0, 1.0, 0.0));
        color.set_sv(0.0, 1.0);
        assert_eq!(color.color(), Color::WHITE);
        color.set_hex("#123456".into());
        assert_eq!(color.color(), settings_pages::rgb(0x123456));
        color.set_hex("#12".into());
        assert_eq!(color.hex, "#12");
        assert!(settings_pages::parse_color(&color.hex).is_none());
        for (hour, minute, valid) in [
            ("0", "0", true),
            ("23", "59", true),
            ("24", "0", false),
            ("0", "60", false),
            ("", "0", false),
            ("-1", "0", false),
        ] {
            assert_eq!(
                TimeDraft {
                    hour: hour.into(),
                    minute: minute.into(),
                    ..Default::default()
                }
                .time()
                .is_some(),
                valid
            );
        }
    }

    #[test]
    fn analog_dial_selects_all_hours_and_minutes_and_advances_after_hour_selection() {
        let size = Size::new(344.0, 216.0);
        for part in [TimePart::Hour, TimePart::Minute] {
            let count = if part == TimePart::Hour { 24 } else { 60 };
            for value in 0..count {
                let dial = TimeDial { part, value };
                assert_eq!(
                    dial.value_at(size, dial.selected(size)),
                    value,
                    "{part:?} {value}"
                );
            }
        }
        let mut draft = TimeDraft::new(chrono::NaiveTime::from_hms_opt(12, 34, 0).unwrap());
        draft.select(TimePart::Hour, 22, false);
        assert_eq!(draft.part, TimePart::Hour);
        draft.select(TimePart::Hour, 22, true);
        assert_eq!(draft.part, TimePart::Minute);
        draft.select(TimePart::Minute, 59, true);
        assert_eq!(draft.time().unwrap().format("%H:%M").to_string(), "22:59");
        draft.select(TimePart::Hour, 24, true);
        draft.select(TimePart::Minute, 60, true);
        assert_eq!(draft.time().unwrap().format("%H:%M").to_string(), "22:59");
    }

    #[test]
    fn analog_pointer_selection_captures_drag_and_finishes_on_release() {
        let dial = TimeDial {
            part: TimePart::Hour,
            value: 22,
        };
        let bounds = Rectangle {
            x: 10.0,
            y: 20.0,
            width: 344.0,
            height: 216.0,
        };
        let point = dial.selected(bounds.size()) + iced::Vector::new(bounds.x, bounds.y);
        let mut state = TimeDialState::default();
        let movement = canvas::Event::Mouse(mouse::Event::CursorMoved { position: point });
        assert!(dial
            .update(
                &mut state,
                &movement,
                bounds,
                mouse::Cursor::Available(point)
            )
            .is_none());
        let pressed = canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let (message, _, status) = dial
            .update(
                &mut state,
                &pressed,
                bounds,
                mouse::Cursor::Available(point),
            )
            .unwrap()
            .into_inner();
        assert!(matches!(
            message,
            Some(power_rules::Message::PickerDial(TimePart::Hour, 22, false))
        ));
        assert_eq!(status, iced::event::Status::Captured);
        let released = canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        let (message, _, _) = dial
            .update(&mut state, &released, bounds, mouse::Cursor::Unavailable)
            .unwrap()
            .into_inner();
        assert!(matches!(
            message,
            Some(power_rules::Message::PickerDial(TimePart::Hour, 22, true))
        ));
        assert!(!state.dragging);
        assert!(dial
            .update(
                &mut state,
                &movement,
                bounds,
                mouse::Cursor::Available(point)
            )
            .is_none());
    }

    #[test]
    fn color_square_only_publishes_during_drag_and_clamps_outside_coordinates() {
        let square = ColorSquare {
            hue: 0,
            saturation: 1.0,
            value: 1.0,
        };
        let bounds = Rectangle {
            x: 10.0,
            y: 20.0,
            width: 200.0,
            height: 100.0,
        };
        let mut dragging = false;
        let center = bounds.center();
        let movement = canvas::Event::Mouse(mouse::Event::CursorMoved { position: center });
        assert!(square
            .update(
                &mut dragging,
                &movement,
                bounds,
                mouse::Cursor::Available(center)
            )
            .is_none());
        let pressed = canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let (message, _, status) = square
            .update(
                &mut dragging,
                &pressed,
                bounds,
                mouse::Cursor::Available(center),
            )
            .unwrap()
            .into_inner();
        assert!(matches!(
            message,
            Some(settings_pages::Message::PickerSv(0.5, 0.5))
        ));
        assert_eq!(status, iced::event::Status::Captured);
        let outside = Point::new(300.0, -10.0);
        let (message, _, _) = square
            .update(
                &mut dragging,
                &movement,
                bounds,
                mouse::Cursor::Available(outside),
            )
            .unwrap()
            .into_inner();
        assert!(matches!(
            message,
            Some(settings_pages::Message::PickerSv(1.0, 1.0))
        ));
        square.update(
            &mut dragging,
            &canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Unavailable,
        );
        assert!(!dragging);
        assert!(square
            .update(
                &mut dragging,
                &movement,
                bounds,
                mouse::Cursor::Available(center)
            )
            .is_none());
    }
}
