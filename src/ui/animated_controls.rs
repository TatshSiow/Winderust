//! Native controls with interpolated styles; input behavior remains owned by Iced.
use super::{motion, widgets};
use iced::{Element, Length, Padding, Theme};
use std::{cell::Cell, rc::Rc};

pub(super) fn color(a: iced::Color, b: iced::Color, t: f32) -> iced::Color {
    iced::Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}
pub(super) fn background(a: iced::Background, b: iced::Background, t: f32) -> iced::Background {
    match (a, b) {
        (iced::Background::Color(a), iced::Background::Color(b)) => {
            // Interpolate premultiplied colors to avoid a bright pulse between alpha levels.
            let alpha = a.a + (b.a - a.a) * t;
            let weight = if alpha > 0.0 { b.a * t / alpha } else { t };
            let mut result = color(a, b, weight);
            result.a = alpha;
            result.into()
        }
        _ => b,
    }
}
pub(super) struct Button<'a, M> {
    inner: iced::widget::Button<'a, M>,
    progress: Rc<Cell<f32>>,
    selection: Option<(bool, Rc<Cell<f32>>)>,
}
impl<'a, M: 'a> Button<'a, M> {
    pub(super) fn new(content: impl Into<Element<'a, M>>) -> Self {
        Self {
            inner: iced::widget::button(content),
            progress: Rc::new(Cell::new(0.0)),
            selection: None,
        }
    }
    pub(super) fn selected(
        mut self,
        selected: bool,
        selected_style: fn(&Theme, iced::widget::button::Status) -> iced::widget::button::Style,
    ) -> Self {
        let blend = Rc::new(Cell::new(f32::from(selected)));
        self.selection = Some((selected, blend.clone()));
        self.style(move |theme, status| {
            let a = widgets::quiet(theme, status);
            let b = selected_style(theme, status);
            let mut result = a;
            result.background = Some(background(
                a.background.unwrap_or(iced::Color::TRANSPARENT.into()),
                b.background.unwrap_or(iced::Color::TRANSPARENT.into()),
                blend.get(),
            ));
            result.text_color = color(a.text_color, b.text_color, blend.get());
            result
        })
    }
    pub(super) fn clip(mut self, value: bool) -> Self {
        self.inner = self.inner.clip(value);
        self
    }
    pub(super) fn width(mut self, v: impl Into<Length>) -> Self {
        self.inner = self.inner.width(v);
        self
    }
    pub(super) fn height(mut self, v: impl Into<Length>) -> Self {
        self.inner = self.inner.height(v);
        self
    }
    pub(super) fn padding(mut self, v: impl Into<Padding>) -> Self {
        self.inner = self.inner.padding(v);
        self
    }
    pub(super) fn on_press(mut self, v: M) -> Self {
        self.inner = self.inner.on_press(v);
        self
    }
    pub(super) fn on_press_maybe(mut self, v: Option<M>) -> Self {
        self.inner = self.inner.on_press_maybe(v);
        self
    }
    pub(super) fn style(
        mut self,
        style: impl Fn(&Theme, iced::widget::button::Status) -> iced::widget::button::Style + 'a,
    ) -> Self {
        let progress = self.progress.clone();
        self.inner = self.inner.style(move |theme, status| {
            use iced::widget::button::Status;
            if matches!(status, Status::Disabled | Status::Pressed) {
                return style(theme, status);
            }
            let a = style(theme, Status::Active);
            let b = style(theme, Status::Hovered);
            let t = progress.get();
            let mut result = a;
            result.background = Some(background(
                a.background.unwrap_or(iced::Color::TRANSPARENT.into()),
                b.background.unwrap_or(iced::Color::TRANSPARENT.into()),
                t,
            ));
            result.text_color = color(a.text_color, b.text_color, t);
            result.border.color = color(a.border.color, b.border.color, t);
            result.border.width = a.border.width + (b.border.width - a.border.width) * t;
            result.shadow.color = color(a.shadow.color, b.shadow.color, t);
            result
        });
        self
    }
}
impl<'a, M: Clone + 'a> From<Button<'a, M>> for Element<'a, M> {
    fn from(value: Button<'a, M>) -> Self {
        let button = motion::wrap(value.inner, false, motion::Effect::Hover(value.progress));
        if let Some((selected, blend)) = value.selection {
            motion::wrap(button, selected, motion::Effect::Control(blend))
        } else {
            button
        }
    }
}
pub(super) struct Checkbox<'a, M> {
    inner: iced::widget::Checkbox<'a, M>,
    value: bool,
    interactive: bool,
    progress: Rc<Cell<f32>>,
}
impl<'a, M: 'a> Checkbox<'a, M> {
    pub(super) fn new(value: bool) -> Self {
        let progress = Rc::new(Cell::new(if value { 1.0 } else { 0.0 }));
        let blend = progress.clone();
        let inner = iced::widget::checkbox(value)
            .size(super::design::CHECKBOX_SIZE)
            .text_size(super::design::typography::BODY)
            .spacing(super::design::space::SMALL)
            .style(move |theme, status| {
                use iced::widget::checkbox::Status;
                if matches!(status, Status::Disabled { .. }) {
                    let mut result = widgets::checkbox_style(theme, status);
                    result.icon_color = iced::Color::TRANSPARENT;
                    return result;
                }
                let status = |is_checked| {
                    if matches!(status, Status::Hovered { .. }) {
                        Status::Hovered { is_checked }
                    } else {
                        Status::Active { is_checked }
                    }
                };
                let a = widgets::checkbox_style(theme, status(false));
                let b = widgets::checkbox_style(theme, status(true));
                let t = blend.get();
                let mut result = a;
                result.background = background(a.background, b.background, t);
                result.border.color = color(a.border.color, b.border.color, t);
                result.icon_color = iced::Color::TRANSPARENT;
                result
            });
        Self {
            inner,
            value,
            interactive: false,
            progress,
        }
    }
    pub(super) fn label(mut self, label: impl iced::widget::text::IntoFragment<'a>) -> Self {
        self.inner = self.inner.label(label);
        self
    }
    pub(super) fn on_toggle(mut self, f: impl Fn(bool) -> M + 'a) -> Self {
        self.interactive = true;
        self.inner = self.inner.on_toggle(f);
        self
    }
    pub(super) fn on_toggle_maybe(mut self, f: Option<impl Fn(bool) -> M + 'a>) -> Self {
        self.interactive = f.is_some();
        self.inner = self.inner.on_toggle_maybe(f);
        self
    }
}
impl<'a, M: 'a> From<Checkbox<'a, M>> for Element<'a, M> {
    fn from(value: Checkbox<'a, M>) -> Self {
        motion::wrap(
            value.inner,
            value.value,
            motion::Effect::Checkbox {
                blend: value.progress,
                interactive: value.interactive,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hover_color_blends_transparency_and_reaches_both_endpoints() {
        let a = iced::Color::TRANSPARENT;
        let b = iced::Color::from_rgba(0.2, 0.4, 0.8, 0.6);
        assert_eq!(color(a, b, 0.0), a);
        assert_eq!(color(a, b, 1.0), b);
        assert!((color(a, b, 0.5).a - 0.3).abs() < 0.0001);
    }
    #[test]
    fn background_fade_stays_between_its_composited_endpoints() {
        let a = iced::Color::from_rgba(0.1, 0.2, 0.3, 1.0);
        let b = iced::Color::from_rgba(0.6, 0.7, 0.8, 0.1);
        for step in 0..=10 {
            let t = step as f32 / 10.0;
            let iced::Background::Color(actual) = background(a.into(), b.into(), t) else {
                panic!("solid color")
            };
            for (start, end, channel) in [
                (a.r, b.r, actual.r),
                (a.g, b.g, actual.g),
                (a.b, b.b, actual.b),
            ] {
                let expected = start * a.a * (1.0 - t) + end * b.a * t;
                assert!((channel * actual.a - expected).abs() < 0.00001);
            }
        }
    }
}
