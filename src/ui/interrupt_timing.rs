use super::{design, widgets};
use crate::backend::interrupt_timing::{
    InterruptKind, InterruptTimingCapture, InterruptTimingError, InterruptTimingReport,
};
use iced::widget::{column, row, text, Column, Row};
use iced::{Element, Fill, Subscription};
use rust_i18n::t;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub(super) enum Message {
    Run,
    Cancel,
}

#[derive(Default)]
pub(super) enum State {
    #[default]
    Idle,
    Capturing {
        capture: InterruptTimingCapture,
        cancelling: bool,
    },
    Finished(InterruptTimingReport),
    Failed(InterruptTimingError),
}

impl State {
    pub(super) fn update(&mut self, message: Message) {
        match message {
            Message::Run if !matches!(self, Self::Capturing { .. }) => {
                *self = match InterruptTimingCapture::start() {
                    Ok(capture) => Self::Capturing {
                        capture,
                        cancelling: false,
                    },
                    Err(error) => Self::failed(error),
                };
            }
            Message::Run => {}
            Message::Cancel => self.cancel(),
        }
    }

    pub(super) fn subscription(&self) -> Subscription<()> {
        if matches!(self, Self::Capturing { .. }) {
            iced::time::every(Duration::from_millis(100)).map(|_| ())
        } else {
            Subscription::none()
        }
    }

    fn cancel(&mut self) {
        let Self::Capturing {
            capture,
            cancelling,
        } = self
        else {
            return;
        };
        if !*cancelling {
            capture.cancel();
            *cancelling = true;
        }
    }

    pub(super) fn cancel_for_exit(&mut self) -> Option<InterruptTimingCapture> {
        self.cancel();
        match std::mem::take(self) {
            Self::Capturing { capture, .. } => Some(capture),
            _ => None,
        }
    }

    pub(super) fn poll(&mut self) {
        if let Self::Capturing { capture, .. } = self {
            if let Some(result) = capture.poll() {
                *self = match result {
                    Ok(report) => Self::Finished(report),
                    Err(error) => Self::failed(error),
                };
            }
        }
    }

    fn failed(error: InterruptTimingError) -> Self {
        if matches!(
            error,
            InterruptTimingError::Worker(_) | InterruptTimingError::Capture(_)
        ) {
            crate::backend::diagnostics::error(&format!("Interrupt Timing failed with {error}"));
        }
        Self::Failed(error)
    }

    pub(super) fn view(&self) -> Element<'_, Message> {
        let button = match self {
            Self::Capturing { cancelling, .. } => widgets::action_button(t!("common.cancel"))
                .on_press_maybe((!*cancelling).then_some(Message::Cancel)),
            _ => widgets::action_button(t!("interrupt_timing.run")).on_press(Message::Run),
        };
        let mut body = column![widgets::setting_row("interrupt_timing.title", button)]
            .spacing(design::space::SMALL);
        let status = match self {
            Self::Idle => None,
            Self::Capturing { cancelling, .. } => Some(if *cancelling {
                "interrupt_timing.cancelling"
            } else {
                "interrupt_timing.capturing"
            }),
            Self::Finished(report) => {
                body = body
                    .push(widgets::settings_divider())
                    .push(report_view(report));
                None
            }
            Self::Failed(error) => Some(match error {
                InterruptTimingError::AccessDenied => "interrupt_timing.access_denied",
                InterruptTimingError::InUse => "interrupt_timing.in_use",
                InterruptTimingError::Cancelled => "interrupt_timing.cancelled",
                InterruptTimingError::Worker(_) | InterruptTimingError::Capture(_) => {
                    "interrupt_timing.failed"
                }
            }),
        };
        if let Some(status) = status {
            body = body.push(text(t!(status)).style(text::secondary));
        }
        widgets::settings_card(body).into()
    }
}

fn report_view(report: &InterruptTimingReport) -> Column<'_, Message> {
    let mut body = column![].spacing(design::space::TIGHT);
    if report.modules.is_empty() {
        body = body.push(text(t!("interrupt_timing.no_samples")).style(text::secondary));
    } else {
        body = body
            .push(widgets::heading(
                t!("interrupt_timing.top_drivers").into_owned(),
                design::typography::BODY,
            ))
            .push(result_row(
                [
                    t!("interrupt_timing.driver").into_owned(),
                    t!("interrupt_timing.kind").into_owned(),
                    t!("interrupt_timing.count").into_owned(),
                    t!("interrupt_timing.longest").into_owned(),
                    t!("interrupt_timing.total").into_owned(),
                ],
                true,
            ));
        for driver in &report.modules {
            let kind = match driver.kind {
                InterruptKind::Dpc => "interrupt_timing.dpc",
                InterruptKind::Isr => "interrupt_timing.isr",
                InterruptKind::ThreadedDpc => "interrupt_timing.threaded_dpc",
            };
            body = body.push(result_row(
                [
                    driver
                        .module
                        .clone()
                        .unwrap_or_else(|| t!("interrupt_timing.unknown_driver").into_owned()),
                    t!(kind).into_owned(),
                    driver.count.to_string(),
                    format!("{:.3}", driver.max_inclusive_ms),
                    format!("{:.3}", driver.total_inclusive_ms),
                ],
                false,
            ));
        }
    }
    let issues: Vec<_> = [
        ("interrupt_timing.lost_events", report.events_lost as usize),
        (
            "interrupt_timing.lost_buffers",
            report.buffers_lost as usize,
        ),
        (
            "interrupt_timing.invalid_intervals",
            report.invalid_intervals,
        ),
        (
            "interrupt_timing.incomplete_intervals",
            report.incomplete_intervals,
        ),
        (
            "interrupt_timing.invalid_images",
            report.invalid_image_records,
        ),
        (
            "interrupt_timing.unresolved_drivers",
            report.unresolved_attribution,
        ),
    ]
    .into_iter()
    .filter(|(_, count)| *count != 0)
    .map(|(key, count)| t!(key, count = count.to_string()).into_owned())
    .collect();
    if !issues.is_empty() {
        body = body.push(text(issues.join(" · ")).style(text::secondary));
    }
    body
}

fn result_row<'a>(cells: [String; 5], header: bool) -> Row<'a, Message> {
    let [driver, kind, count, longest, total] = cells;
    let cell = |label| {
        let cell = text(label).size(design::typography::CAPTION);
        if header {
            cell.style(text::secondary)
        } else {
            cell
        }
    };
    row![
        cell(driver).width(Fill),
        cell(kind).width(86),
        cell(count).width(52).align_x(iced::Right),
        cell(longest).width(82).align_x(iced::Right),
        cell(total).width(82).align_x(iced::Right),
    ]
    .spacing(design::space::SMALL)
    .align_y(iced::Center)
}
