use super::{design, widgets};
use crate::backend::interrupt_timing::{
    export_report, InterruptKind, InterruptTimingCapture, InterruptTimingError,
    InterruptTimingReport,
};
use iced::widget::{column, row, text, Column, Row, Text};
use iced::{Element, Fill, Length, Subscription, Task};
use rust_i18n::t;
use std::{borrow::Cow, path::PathBuf, time::Duration};
use windows_sys::Win32::Foundation::HWND;

#[derive(Debug, Clone)]
pub(super) enum Message {
    Run,
    Cancel,
    TogglePerCpu,
    Export,
    ExportChosen(Option<PathBuf>),
    ExportFinished(Result<(), String>),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ExportStatus {
    Ready,
    ChoosingDestination,
    Writing,
    Saved,
    Failed(String),
}

impl ExportStatus {
    fn pending(&self) -> bool {
        matches!(self, Self::ChoosingDestination | Self::Writing)
    }
}

#[derive(Default)]
pub(super) struct State {
    capture: CaptureStatus,
    completed: Option<CompletedReport>,
}

#[derive(Default)]
enum CaptureStatus {
    #[default]
    Idle,
    Capturing {
        capture: InterruptTimingCapture,
        cancelling: bool,
    },
    Failed(InterruptTimingError),
}

struct CompletedReport {
    report: InterruptTimingReport,
    per_cpu_expanded: bool,
    export: ExportStatus,
}

impl State {
    pub(super) fn update(&mut self, message: Message, hwnd: Option<HWND>) -> Task<Message> {
        match message {
            Message::Run if self.can_run() => {
                self.capture = match InterruptTimingCapture::start() {
                    Ok(capture) => CaptureStatus::Capturing {
                        capture,
                        cancelling: false,
                    },
                    Err(error) => Self::failed(error),
                };
            }
            Message::Run => {}
            Message::Cancel => self.cancel(),
            Message::Export if self.can_run() => {
                if let Some(completed) = &mut self.completed {
                    completed.export = ExportStatus::ChoosingDestination;
                    return Task::perform(
                        crate::file_dialog::choose_interrupt_timing_export_file(hwnd),
                        Message::ExportChosen,
                    );
                }
            }
            Message::Export => {}
            Message::ExportChosen(path) => {
                if let Some(CompletedReport { report, export, .. }) = &mut self.completed {
                    if *export == ExportStatus::ChoosingDestination {
                        let Some(path) = path else {
                            *export = ExportStatus::Ready;
                            return Task::none();
                        };
                        *export = ExportStatus::Writing;
                        let report = report.clone();
                        return super::tasks::run(move || export_report(&path, &report))
                            .map(|result| Message::ExportFinished(result.and_then(|r| r)));
                    }
                }
            }
            Message::ExportFinished(result) => {
                if let Some(CompletedReport { export, .. }) = &mut self.completed {
                    if *export == ExportStatus::Writing {
                        *export = match result {
                            Ok(()) => ExportStatus::Saved,
                            Err(error) => ExportStatus::Failed(error),
                        };
                    }
                }
            }
            Message::TogglePerCpu if !self.is_capturing() => {
                if let Some(completed) = &mut self.completed {
                    completed.per_cpu_expanded = !completed.per_cpu_expanded;
                }
            }
            Message::TogglePerCpu => {}
        }
        Task::none()
    }

    fn is_capturing(&self) -> bool {
        matches!(self.capture, CaptureStatus::Capturing { .. })
    }

    fn can_run(&self) -> bool {
        !self.is_capturing()
            && self
                .completed
                .as_ref()
                .is_none_or(|completed| !completed.export.pending())
    }

    pub(super) fn export_writing(&self) -> bool {
        self.completed
            .as_ref()
            .is_some_and(|completed| completed.export == ExportStatus::Writing)
    }

    pub(super) fn export_failure(&self) -> Option<String> {
        match &self.completed.as_ref()?.export {
            ExportStatus::Failed(error) => Some(export_failure_message(error)),
            _ => None,
        }
    }

    pub(super) fn subscription(&self) -> Subscription<()> {
        if self.is_capturing() {
            iced::time::every(Duration::from_millis(100)).map(|_| ())
        } else {
            Subscription::none()
        }
    }

    fn cancel(&mut self) {
        let CaptureStatus::Capturing {
            capture,
            cancelling,
        } = &mut self.capture
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
        match std::mem::take(self).capture {
            CaptureStatus::Capturing { capture, .. } => Some(capture),
            _ => None,
        }
    }

    pub(super) fn poll(&mut self) {
        if let CaptureStatus::Capturing {
            capture,
            cancelling,
        } = &mut self.capture
        {
            if let Some(result) = capture.poll() {
                self.capture = match result {
                    Ok(_) if *cancelling => Self::failed(InterruptTimingError::Cancelled),
                    Ok(report) => {
                        self.completed = Some(CompletedReport {
                            report,
                            per_cpu_expanded: false,
                            export: ExportStatus::Ready,
                        });
                        CaptureStatus::Idle
                    }
                    Err(error) => Self::failed(error),
                };
            }
        }
    }

    fn failed(error: InterruptTimingError) -> CaptureStatus {
        if matches!(
            error,
            InterruptTimingError::Worker(_)
                | InterruptTimingError::Capture(_)
                | InterruptTimingError::TimedOut(_)
        ) {
            crate::backend::diagnostics::error(&format!("Interrupt Timing failed with {error}"));
        }
        CaptureStatus::Failed(error)
    }

    pub(super) fn view(&self) -> Element<'_, Message> {
        let button = match &self.capture {
            CaptureStatus::Capturing { cancelling, .. } => {
                widgets::action_button(t!("common.cancel"))
                    .on_press_maybe((!*cancelling).then_some(Message::Cancel))
            }
            _ => widgets::action_button(t!("interrupt_timing.run"))
                .on_press_maybe(self.can_run().then_some(Message::Run)),
        };
        let mut body = column![widgets::setting_row("interrupt_timing.title", button)]
            .spacing(design::space::SMALL);
        let status = match &self.capture {
            CaptureStatus::Idle => None,
            CaptureStatus::Capturing { cancelling, .. } => Some(if *cancelling {
                "interrupt_timing.cancelling"
            } else {
                "interrupt_timing.capturing"
            }),
            CaptureStatus::Failed(error) => Some(match error {
                InterruptTimingError::AccessDenied => "interrupt_timing.access_denied",
                InterruptTimingError::InUse => "interrupt_timing.in_use",
                InterruptTimingError::Cancelled => "interrupt_timing.cancelled",
                InterruptTimingError::TimedOut(_) => "interrupt_timing.timed_out",
                InterruptTimingError::Worker(_) | InterruptTimingError::Capture(_) => {
                    "interrupt_timing.failed"
                }
            }),
        };
        let completed = self.completed.as_ref().filter(|_| !self.is_capturing());
        if let Some(status) = status {
            let status = t!(status);
            let status = if completed.is_some() {
                t!("interrupt_timing.previous_report_status", status = status)
            } else {
                status
            };
            body = body.push(text(status).style(text::secondary));
        }
        if let Some(CompletedReport {
            report,
            per_cpu_expanded,
            export,
        }) = completed
        {
            let feedback = match export {
                ExportStatus::Ready | ExportStatus::ChoosingDestination => None,
                ExportStatus::Writing => {
                    Some(text(t!("interrupt_timing.export_saving")).style(text::secondary))
                }
                ExportStatus::Saved => {
                    Some(text(t!("interrupt_timing.export_saved")).style(text::secondary))
                }
                ExportStatus::Failed(error) => {
                    Some(text(export_failure_message(error)).style(text::danger))
                }
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
        }
        widgets::settings_card(body).into()
    }
}

fn export_failure_message(error: &str) -> String {
    t!("interrupt_timing.export_failed", error = error).into_owned()
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
            t!("interrupt_timing.top_routine_modules").into_owned(),
            design::typography::BODY,
        )
    };
    let mut body = column![row![
        label.width(Fill).wrapping(text::Wrapping::WordOrGlyph),
        widgets::action_button(t!("interrupt_timing.export_report"))
            .on_press_maybe((!export.pending()).then_some(Message::Export)),
    ]
    .spacing(design::space::SMALL)
    .align_y(iced::Center)]
    .spacing(design::space::TIGHT);
    if !report.modules.is_empty() {
        body = body.push(result_row(
            [
                t!("interrupt_timing.routine_module"),
                t!("interrupt_timing.kind"),
                t!("interrupt_timing.count"),
                t!("interrupt_timing.longest"),
                t!("interrupt_timing.total"),
            ],
            true,
        ));
        for timing in &report.modules {
            let kind = match timing.kind {
                InterruptKind::Dpc => "interrupt_timing.dpc",
                InterruptKind::Isr => "interrupt_timing.isr",
                InterruptKind::ThreadedDpc => "interrupt_timing.threaded_dpc",
            };
            body = body.push(result_row(
                [
                    timing
                        .module
                        .as_deref()
                        .map_or_else(|| t!("interrupt_timing.unknown_module"), Cow::Borrowed),
                    t!(kind),
                    timing.count.to_string().into(),
                    format!("{:.3}", timing.max_inclusive_ms).into(),
                    format!("{:.3}", timing.total_inclusive_ms).into(),
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
            "interrupt_timing.unresolved_modules",
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
                    t!("interrupt_timing.logical_cpu"),
                    t!("interrupt_timing.dpc_ms"),
                    t!("interrupt_timing.isr_ms"),
                ],
                true,
            ));
            for row in rows {
                body = body.push(per_cpu_row(
                    [
                        row.cpu.to_string().into(),
                        format!("{:.3}", row.dpc_ms).into(),
                        format!("{:.3}", row.isr_ms).into(),
                    ],
                    false,
                ));
            }
        }
    } else if report.per_cpu.is_none() && !report.modules.is_empty() {
        body = body
            .push(widgets::settings_divider())
            .push(widgets::setting_row(
                "interrupt_timing.per_cpu",
                text(t!("common.unavailable"))
                    .size(design::typography::SECONDARY)
                    .style(text::secondary),
            ));
    }
    body
}

fn result_row<'a>(cells: [Cow<'a, str>; 5], header: bool) -> Row<'a, Message> {
    let [module, kind, count, longest, total] = cells;
    let cell = |label| result_cell(label, header);
    row![
        cell(module).width(Length::FillPortion(3)),
        cell(kind).width(Length::FillPortion(2)),
        cell(count).width(48).align_x(iced::Right),
        cell(longest).width(70).align_x(iced::Right),
        cell(total).width(70).align_x(iced::Right),
    ]
    .spacing(design::space::SMALL)
    .align_y(iced::Center)
}

fn per_cpu_row<'a>(cells: [Cow<'a, str>; 3], header: bool) -> Row<'a, Message> {
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

fn result_cell<'a>(label: Cow<'a, str>, header: bool) -> Text<'a> {
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
        assert_eq!(State::default().update(Message::Export, None).units(), 0);
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
        let mut state = State {
            completed: Some(CompletedReport {
                report: report.clone(),
                per_cpu_expanded: true,
                export: ExportStatus::Ready,
            }),
            ..State::default()
        };
        assert!(state.update(Message::Export, None).units() > 0);
        assert!(!state.can_run());
        assert!(!state.export_writing());
        assert_eq!(state.update(Message::Export, None).units(), 0);
        drop(state.update(Message::ExportChosen(None), None));
        assert!(state.can_run());
        assert!(matches!(
            &state.completed,
            Some(CompletedReport { report: current, per_cpu_expanded: true, export: ExportStatus::Ready })
                if current == &report
        ));

        for (result, expected) in [
            (Ok(()), ExportStatus::Saved),
            (
                Err("Read-only destination".into()),
                ExportStatus::Failed("Read-only destination".into()),
            ),
        ] {
            let Some(CompletedReport { export, .. }) = &mut state.completed else {
                unreachable!();
            };
            *export = ExportStatus::Writing;
            assert!(state.export_writing());
            assert!(!state.can_run());
            assert_eq!(state.update(Message::Export, None).units(), 0);
            drop(state.update(Message::ExportFinished(result), None));
            assert!(state.can_run());
            assert!(matches!(
                &state.completed,
                Some(CompletedReport { report: current, per_cpu_expanded: true, export })
                    if current == &report && export == &expected
            ));
        }
    }
}
