use super::{design, widgets};
use crate::backend::interrupt_timing::{
    export_report, CaptureUpdate, InterruptKind, InterruptTimingCapture, InterruptTimingError,
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
    ToggleTimeline,
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
        phase: CapturePhase,
    },
    Failed(InterruptTimingError),
}

enum CapturePhase {
    Recording,
    Cancelling,
    WaitingForCleanup(InterruptTimingError),
}

struct CompletedReport {
    report: InterruptTimingReport,
    timeline_expanded: bool,
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
                        phase: CapturePhase::Recording,
                    },
                    Err(error) => Self::failed(error),
                };
            }
            Message::Run => {}
            Message::Cancel => self.cancel(),
            Message::Export if self.can_export() => {
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
            Message::ToggleTimeline | Message::TogglePerCpu if !self.is_capturing() => {
                if let Some(completed) = &mut self.completed {
                    let expanded = if matches!(message, Message::ToggleTimeline) {
                        &mut completed.timeline_expanded
                    } else {
                        &mut completed.per_cpu_expanded
                    };
                    *expanded = !*expanded;
                }
            }
            Message::ToggleTimeline | Message::TogglePerCpu => {}
        }
        Task::none()
    }

    fn is_capturing(&self) -> bool {
        matches!(
            self.capture,
            CaptureStatus::Capturing {
                phase: CapturePhase::Recording | CapturePhase::Cancelling,
                ..
            }
        )
    }

    fn can_run(&self) -> bool {
        !matches!(self.capture, CaptureStatus::Capturing { .. }) && self.can_export()
    }

    fn can_export(&self) -> bool {
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
        let period = match self.capture {
            CaptureStatus::Capturing {
                phase: CapturePhase::WaitingForCleanup(_),
                ..
            } => Duration::from_secs(1),
            CaptureStatus::Capturing { .. } => Duration::from_millis(100),
            _ => return Subscription::none(),
        };
        iced::time::every(period).map(|_| ())
    }

    fn cancel(&mut self) {
        if let CaptureStatus::Capturing { capture, phase } = &mut self.capture {
            if matches!(phase, CapturePhase::Recording) {
                capture.cancel();
                *phase = CapturePhase::Cancelling;
            }
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
        if let CaptureStatus::Capturing { capture, phase } = &mut self.capture {
            match capture.poll() {
                None => {}
                Some(CaptureUpdate::CleanupPending(error)) => {
                    Self::log_failure(&error);
                    *phase = CapturePhase::WaitingForCleanup(error);
                }
                Some(CaptureUpdate::Finished(result)) => {
                    self.capture = match (result, phase) {
                        (
                            Ok(_) | Err(InterruptTimingError::Cancelled),
                            CapturePhase::WaitingForCleanup(error),
                        ) => CaptureStatus::Failed(error.clone()),
                        (Ok(_), CapturePhase::Cancelling) => {
                            Self::failed(InterruptTimingError::Cancelled)
                        }
                        (Ok(report), CapturePhase::Recording) => {
                            self.completed = Some(CompletedReport {
                                report,
                                timeline_expanded: false,
                                per_cpu_expanded: false,
                                export: ExportStatus::Ready,
                            });
                            CaptureStatus::Idle
                        }
                        (Err(error), _) => Self::failed(error),
                    }
                }
            }
        }
    }

    fn failed(error: InterruptTimingError) -> CaptureStatus {
        Self::log_failure(&error);
        CaptureStatus::Failed(error)
    }

    fn log_failure(error: &InterruptTimingError) {
        if matches!(
            error,
            InterruptTimingError::Worker(_)
                | InterruptTimingError::Capture(_)
                | InterruptTimingError::Cleanup(_)
                | InterruptTimingError::TimedOut(_)
        ) {
            crate::backend::diagnostics::error(&format!("Interrupt Timing failed with {error}"));
        }
    }

    pub(super) fn view(&self) -> Element<'_, Message> {
        let button = match &self.capture {
            CaptureStatus::Capturing {
                phase: phase @ (CapturePhase::Recording | CapturePhase::Cancelling),
                ..
            } => widgets::action_button(t!("common.cancel")).on_press_maybe(
                matches!(phase, CapturePhase::Recording).then_some(Message::Cancel),
            ),
            _ => widgets::action_button(t!("interrupt_timing.run"))
                .on_press_maybe(self.can_run().then_some(Message::Run)),
        };
        let mut body = column![widgets::setting_row("interrupt_timing.title", button)]
            .spacing(design::space::SMALL);
        let status = match &self.capture {
            CaptureStatus::Idle => None,
            CaptureStatus::Capturing { phase, .. } => Some(match phase {
                CapturePhase::Recording => "interrupt_timing.capturing",
                CapturePhase::Cancelling => "interrupt_timing.cancelling",
                CapturePhase::WaitingForCleanup(_) => "interrupt_timing.timed_out_cleanup",
            }),
            CaptureStatus::Failed(error) => Some(match error {
                InterruptTimingError::AccessDenied => "interrupt_timing.access_denied",
                InterruptTimingError::InUse => "interrupt_timing.in_use",
                InterruptTimingError::Cancelled => "interrupt_timing.cancelled",
                InterruptTimingError::TimedOut(_) => "interrupt_timing.timed_out",
                InterruptTimingError::Cleanup(_) => "interrupt_timing.cleanup_failed",
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
            timeline_expanded,
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
                *timeline_expanded,
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
    timeline_expanded: bool,
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
    if !report.modules.is_empty() {
        body = body.push(widgets::settings_divider()).push(detail_button(
            "interrupt_timing.timeline",
            timeline_expanded,
            Message::ToggleTimeline,
        ));
        if timeline_expanded {
            body = body.push(timeline_row(
                [
                    t!("interrupt_timing.elapsed"),
                    t!("interrupt_timing.dpc_ms"),
                    t!("interrupt_timing.isr_ms"),
                    t!("interrupt_timing.threaded_dpc_ms"),
                ],
                true,
            ));
            for (second, timing) in report.timeline.iter().enumerate() {
                body = body.push(timeline_row(
                    [
                        t!(
                            "interrupt_timing.second_range",
                            start = second,
                            end = second + 1
                        ),
                        format!("{:.3}", timing.dpc_ms).into(),
                        format!("{:.3}", timing.isr_ms).into(),
                        format!("{:.3}", timing.threaded_dpc_ms).into(),
                    ],
                    false,
                ));
            }
        }
    }
    if let Some(rows) = report.per_cpu.as_deref().filter(|rows| {
        rows.iter()
            .any(|row| row.dpc_count != 0 || row.isr_count != 0)
    }) {
        body = body.push(widgets::settings_divider()).push(detail_button(
            "interrupt_timing.per_cpu",
            per_cpu_expanded,
            Message::TogglePerCpu,
        ));
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

fn detail_button(key: &str, expanded: bool, message: Message) -> Element<'static, Message> {
    let chevron = if expanded {
        "icons/chevron-down.svg"
    } else {
        "icons/chevron-right.svg"
    };
    widgets::button(
        row![
            widgets::heading(t!(key).into_owned(), design::typography::BODY).width(Fill),
            super::navigation::glyph(chevron),
        ]
        .height(Fill)
        .align_y(iced::Center),
    )
    .width(Fill)
    .height(design::BUTTON_HEIGHT)
    .padding(0)
    .style(widgets::quiet)
    .on_press(message)
    .into()
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

fn timeline_row<'a>(cells: [Cow<'a, str>; 4], header: bool) -> Row<'a, Message> {
    let [elapsed, dpc, isr, threaded_dpc] = cells;
    let cell = |label| result_cell(label, header).align_x(iced::Right);
    row![
        result_cell(elapsed, header).width(Fill),
        cell(dpc).width(Fill),
        cell(isr).width(Fill),
        cell(threaded_dpc).width(Length::FillPortion(2)),
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

    fn report(events_lost: u32) -> InterruptTimingReport {
        InterruptTimingReport {
            capture_start: None,
            events_lost,
            buffers_lost: 0,
            invalid_intervals: 0,
            incomplete_intervals: 0,
            invalid_image_records: 0,
            unresolved_attribution: 0,
            modules: Vec::new(),
            per_cpu: None,
            timeline: Default::default(),
        }
    }

    #[test]
    fn timeout_keeps_cleanup_pending_and_preserves_the_previous_report() {
        let previous = report(1);
        let cleanup_error = InterruptTimingError::Cleanup("ETW cleanup failed".into());
        for (outcome, expected_error) in [
            (Ok(report(2)), None),
            (Err(InterruptTimingError::Cancelled), None),
            (Err(cleanup_error.clone()), Some(cleanup_error)),
        ] {
            let (capture, sender) = InterruptTimingCapture::expired_for_test();
            let mut state = State {
                capture: CaptureStatus::Capturing {
                    capture,
                    phase: CapturePhase::Cancelling,
                },
                completed: Some(CompletedReport {
                    report: previous.clone(),
                    timeline_expanded: true,
                    per_cpu_expanded: true,
                    export: ExportStatus::Ready,
                }),
            };

            state.poll();
            assert!(!state.can_run());
            assert!(!state.is_capturing());
            let CaptureStatus::Capturing {
                capture,
                phase: CapturePhase::WaitingForCleanup(timeout),
            } = &mut state.capture
            else {
                panic!("timeout must retain the capture receiver");
            };
            assert!(matches!(timeout, InterruptTimingError::TimedOut(_)));
            assert!(capture.poll().is_none());
            let expected_error = expected_error.unwrap_or_else(|| timeout.clone());

            assert!(state.update(Message::Export, None).units() > 0);
            drop(state.update(Message::ExportChosen(None), None));
            sender.send(outcome).unwrap();
            state.poll();
            assert!(state.can_run());
            assert!(
                matches!(&state.capture, CaptureStatus::Failed(error) if error == &expected_error)
            );
            assert!(matches!(
                &state.completed,
                Some(CompletedReport { report, timeline_expanded: true, per_cpu_expanded: true, export: ExportStatus::Ready })
                    if report == &previous
            ));
        }
    }

    #[test]
    fn report_export_is_single_flight_and_keeps_the_capture() {
        assert_eq!(State::default().update(Message::Export, None).units(), 0);
        let report = report(1);
        let mut state = State {
            completed: Some(CompletedReport {
                report: report.clone(),
                timeline_expanded: false,
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
            Some(CompletedReport { report: current, per_cpu_expanded: true, export: ExportStatus::Ready, .. })
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
                Some(CompletedReport { report: current, per_cpu_expanded: true, export, .. })
                    if current == &report && export == &expected
            ));
        }
    }
}
