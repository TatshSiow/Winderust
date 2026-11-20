use super::{design, widgets};
use crate::backend::interrupt_timing::{
    export_report, InterruptKind, InterruptTimingCapture, InterruptTimingError,
    InterruptTimingReport,
};
use iced::widget::{column, row, text, Column, Row, Text};
use iced::{Element, Fill, Length, Subscription, Task};
use rust_i18n::t;
use std::time::Duration;
use windows_sys::Win32::Foundation::HWND;

#[derive(Debug, Clone)]
pub(super) enum Message {
    Run,
    Cancel,
    TogglePerCpu,
    Export,
    ExportFinished(Option<Result<(), String>>),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ExportStatus {
    Ready,
    Saving,
    Saved,
    Failed(String),
}

#[derive(Default)]
pub(super) enum State {
    #[default]
    Idle,
    Capturing {
        capture: InterruptTimingCapture,
        cancelling: bool,
    },
    Finished {
        report: InterruptTimingReport,
        per_cpu_expanded: bool,
        export: ExportStatus,
    },
    Failed(InterruptTimingError),
}

impl State {
    pub(super) fn update(&mut self, message: Message, hwnd: Option<HWND>) -> Task<Message> {
        match message {
            Message::Run if self.can_run() => {
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
            Message::Export => {
                if let Self::Finished { report, export, .. } = self {
                    if *export != ExportStatus::Saving {
                        *export = ExportStatus::Saving;
                        let report = report.clone();
                        return Task::future(
                            crate::file_dialog::choose_interrupt_timing_export_file(hwnd),
                        )
                        .then(move |path| {
                            let Some(path) = path else {
                                return Task::done(Message::ExportFinished(None));
                            };
                            let report = report.clone();
                            super::tasks::run(move || export_report(&path, &report))
                                .map(|result| Message::ExportFinished(Some(result.and_then(|r| r))))
                        });
                    }
                }
            }
            Message::ExportFinished(result) => {
                if let Self::Finished { export, .. } = self {
                    if *export == ExportStatus::Saving {
                        *export = match result {
                            Some(Ok(())) => ExportStatus::Saved,
                            Some(Err(error)) => ExportStatus::Failed(error),
                            None => ExportStatus::Ready,
                        };
                    }
                }
            }
            Message::TogglePerCpu => {
                if let Self::Finished {
                    per_cpu_expanded, ..
                } = self
                {
                    *per_cpu_expanded = !*per_cpu_expanded;
                }
            }
        }
        Task::none()
    }

    fn can_run(&self) -> bool {
        !matches!(
            self,
            Self::Capturing { .. }
                | Self::Finished {
                    export: ExportStatus::Saving,
                    ..
                }
        )
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
                    Ok(report) => Self::Finished {
                        report,
                        per_cpu_expanded: false,
                        export: ExportStatus::Ready,
                    },
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
            _ => widgets::action_button(t!("interrupt_timing.run"))
                .on_press_maybe(self.can_run().then_some(Message::Run)),
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
            Self::Finished {
                report,
                per_cpu_expanded,
                export,
            } => {
                let feedback = match export {
                    ExportStatus::Ready => None,
                    ExportStatus::Saving => {
                        Some(text(t!("interrupt_timing.export_saving")).style(text::secondary))
                    }
                    ExportStatus::Saved => {
                        Some(text(t!("interrupt_timing.export_saved")).style(text::secondary))
                    }
                    ExportStatus::Failed(error) => Some(
                        text(t!("interrupt_timing.export_failed", error = error))
                            .style(text::danger),
                    ),
                };
                if let Some(feedback) = feedback {
                    body = body.push(
                        feedback
                            .size(design::typography::SECONDARY)
                            .wrapping(text::Wrapping::WordOrGlyph),
                    );
                }
                body = body.push(widgets::settings_divider()).push(report_view(
                    report,
                    *per_cpu_expanded,
                    export,
                ));
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

fn report_view<'a>(
    report: &'a InterruptTimingReport,
    per_cpu_expanded: bool,
    export: &ExportStatus,
) -> Column<'a, Message> {
    let label = if report.modules.is_empty() {
        text(t!("interrupt_timing.no_samples")).style(text::secondary)
    } else {
        widgets::heading(
            t!("interrupt_timing.top_drivers").into_owned(),
            design::typography::BODY,
        )
    };
    let mut body = column![row![
        label.width(Fill).wrapping(text::Wrapping::WordOrGlyph),
        widgets::action_button(t!("interrupt_timing.export_report"))
            .on_press_maybe((*export != ExportStatus::Saving).then_some(Message::Export)),
    ]
    .spacing(design::space::SMALL)
    .align_y(iced::Center)]
    .spacing(design::space::TIGHT);
    if !report.modules.is_empty() {
        body = body.push(result_row(
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
    if let Some(rows) = report.per_cpu.as_deref().filter(|rows| {
        rows.iter()
            .any(|row| row.dpc_count != 0 || row.isr_count != 0)
    }) {
        let chevron = if per_cpu_expanded {
            "icons/chevron-down.svg"
        } else {
            "icons/chevron-right.svg"
        };
        body = body.push(widgets::settings_divider()).push(
            widgets::button(
                row![
                    widgets::heading(
                        t!("interrupt_timing.per_cpu").into_owned(),
                        design::typography::BODY,
                    )
                    .width(Fill),
                    super::navigation::glyph(chevron),
                ]
                .height(Fill)
                .align_y(iced::Center),
            )
            .width(Fill)
            .height(design::BUTTON_HEIGHT)
            .padding(0)
            .style(widgets::quiet)
            .on_press(Message::TogglePerCpu),
        );
        if per_cpu_expanded {
            body = body.push(per_cpu_row(
                [
                    t!("interrupt_timing.logical_cpu").into_owned(),
                    t!("interrupt_timing.dpc_ms").into_owned(),
                    t!("interrupt_timing.isr_ms").into_owned(),
                ],
                true,
            ));
            for row in rows {
                body = body.push(per_cpu_row(
                    [
                        row.cpu.to_string(),
                        format!("{:.3}", row.dpc_ms),
                        format!("{:.3}", row.isr_ms),
                    ],
                    false,
                ));
            }
        }
    }
    body
}

fn result_row<'a>(cells: [String; 5], header: bool) -> Row<'a, Message> {
    let [driver, kind, count, longest, total] = cells;
    let cell = |label| result_cell(label, header);
    row![
        cell(driver).width(Length::FillPortion(3)),
        cell(kind).width(Length::FillPortion(2)),
        cell(count).width(48).align_x(iced::Right),
        cell(longest).width(70).align_x(iced::Right),
        cell(total).width(70).align_x(iced::Right),
    ]
    .spacing(design::space::SMALL)
    .align_y(iced::Center)
}

fn per_cpu_row<'a>(cells: [String; 3], header: bool) -> Row<'a, Message> {
    let [cpu, dpc, isr] = cells;
    let cell = |label| result_cell(label, header);
    row![
        cell(cpu).width(Fill),
        cell(dpc).width(82).align_x(iced::Right),
        cell(isr).width(82).align_x(iced::Right),
    ]
    .spacing(design::space::SMALL)
    .align_y(iced::Center)
}

fn result_cell<'a>(label: String, header: bool) -> Text<'a> {
    let cell = text(label)
        .size(design::typography::CAPTION)
        .wrapping(text::Wrapping::WordOrGlyph);
    if header {
        cell.style(text::secondary)
    } else {
        cell
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_export_is_single_flight_and_keeps_the_capture() {
        assert_eq!(State::Idle.update(Message::Export, None).units(), 0);
        let report = InterruptTimingReport {
            events_lost: 1,
            buffers_lost: 0,
            invalid_intervals: 0,
            incomplete_intervals: 0,
            invalid_image_records: 0,
            unresolved_attribution: 0,
            modules: Vec::new(),
            per_cpu: None,
        };
        let mut state = State::Finished {
            report: report.clone(),
            per_cpu_expanded: true,
            export: ExportStatus::Ready,
        };
        for (result, expected) in [
            (None, ExportStatus::Ready),
            (Some(Ok(())), ExportStatus::Saved),
            (
                Some(Err("Read-only destination".into())),
                ExportStatus::Failed("Read-only destination".into()),
            ),
        ] {
            assert!(state.update(Message::Export, None).units() > 0);
            assert!(!state.can_run());
            assert_eq!(state.update(Message::Export, None).units(), 0);
            drop(state.update(Message::ExportFinished(result), None));
            assert!(state.can_run());
            assert!(matches!(
                &state,
                State::Finished { report: current, per_cpu_expanded: true, export }
                    if current == &report && export == &expected
            ));
        }
    }
}
