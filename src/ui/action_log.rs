use super::design;
use super::widgets::{button, checkbox};
use crate::action_log::{ActionLogEntry, ActionLogFeature, ActionLogResult};
use chrono::{Local, TimeZone};
use iced::widget::{column, container, row, scrollable, text};
use iced::{Element, Fill};
use rust_i18n::t;

pub(super) struct Editor {
    result: Vec<ActionLogResult>,
    feature: Vec<ActionLogFeature>,
    offset: f32,
    hovered: Option<u64>,
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            result: RESULTS.to_vec(),
            feature: FEATURES.to_vec(),
            offset: 0.0,
            hovered: None,
        }
    }
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Result(ActionLogResult, bool),
    Feature(ActionLogFeature, bool),
    AllResults(bool),
    AllFeatures(bool),
    Scrolled(f32),
    Hover(Option<u64>),
    Clear,
    Export,
}
impl Editor {
    pub(super) fn update(&mut self, message: Message) {
        match message {
            Message::Result(result, checked) => {
                self.result.retain(|value| *value != result);
                if checked {
                    self.result.push(result);
                }
                self.offset = 0.0;
            }
            Message::Feature(feature, checked) => {
                self.feature.retain(|value| *value != feature);
                if checked {
                    self.feature.push(feature);
                }
                self.offset = 0.0;
            }
            Message::AllResults(checked) => {
                self.result = if checked {
                    RESULTS.to_vec()
                } else {
                    Vec::new()
                };
                self.offset = 0.0;
            }
            Message::AllFeatures(checked) => {
                self.feature = if checked {
                    FEATURES.to_vec()
                } else {
                    Vec::new()
                };
                self.offset = 0.0;
            }
            Message::Scrolled(offset) => self.offset = offset,
            Message::Hover(sequence) => self.hovered = sequence,
            Message::Clear => {
                self.offset = 0.0;
                self.hovered = None;
            }
            Message::Export => {} // The application owns the native CSV destination picker.
        }
    }
    pub(super) fn side_panel(
        &self,
        has_entries: bool,
        has_summaries: bool,
    ) -> Element<'_, Message> {
        let mut results = column![row![
            button(container(text(t!("action_log.check_all").to_string())).center_x(Fill))
                .width(Fill)
                .height(32)
                .on_press(Message::AllResults(true)),
            button(container(text(t!("action_log.clear_all").to_string())).center_x(Fill))
                .width(Fill)
                .height(32)
                .on_press(Message::AllResults(false)),
        ]
        .spacing(design::space::SMALL),]
        .spacing(design::space::SMALL);
        for result in RESULTS {
            results = results.push(
                checkbox(self.result.contains(&result))
                    .label(action_log_result_label(result))
                    .on_toggle(move |checked| Message::Result(result, checked)),
            );
        }
        let mut features = column![row![
            button(container(text(t!("action_log.check_all").to_string())).center_x(Fill))
                .width(Fill)
                .height(32)
                .on_press(Message::AllFeatures(true)),
            button(container(text(t!("action_log.clear_all").to_string())).center_x(Fill))
                .width(Fill)
                .height(32)
                .on_press(Message::AllFeatures(false)),
        ]
        .spacing(design::space::SMALL),]
        .spacing(design::space::SMALL);
        for feature in FEATURES {
            features = features.push(
                checkbox(self.feature.contains(&feature))
                    .label(action_log_feature_label(feature))
                    .on_toggle(move |checked| Message::Feature(feature, checked)),
            );
        }
        let filters = scrollable(
            column![
                super::widgets::heading(
                    t!("nav.settings").to_string(),
                    design::typography::SUBTITLE
                ),
                super::widgets::heading(
                    t!("process_list.filter").to_string(),
                    design::typography::SECONDARY
                ),
                column![
                    super::widgets::heading(
                        t!("action_log.result_filter").to_string(),
                        design::typography::SECONDARY
                    ),
                    super::widgets::settings_card(results),
                ]
                .spacing(design::space::SMALL),
                column![
                    super::widgets::heading(
                        t!("action_log.feature_filter").to_string(),
                        design::typography::SECONDARY
                    ),
                    super::widgets::settings_card(features),
                ]
                .spacing(design::space::SMALL),
            ]
            .spacing(design::space::MEDIUM),
        )
        .height(Fill);
        column![
            filters,
            iced::widget::rule::horizontal(1),
            container(
                row![
                    button(container(text(t!("action_log.clear").to_string())).center_x(Fill))
                        .width(Fill)
                        .height(32)
                        .on_press_maybe((has_entries || has_summaries).then_some(Message::Clear)),
                    button(container(text(t!("action_log.export_csv").to_string())).center_x(Fill))
                        .width(Fill)
                        .height(32)
                        .on_press_maybe(has_entries.then_some(Message::Export)),
                ]
                .spacing(design::space::SMALL)
            )
            .padding([design::space::MEDIUM as u16, 0]),
        ]
        .height(Fill)
        .into()
    }

    pub(super) fn view<'a>(&'a self, entries: &'a [ActionLogEntry]) -> Element<'a, Message> {
        iced::widget::responsive(move |size| self.view_at_size(entries, size)).into()
    }

    fn view_at_size<'a>(
        &'a self,
        entries: &'a [ActionLogEntry],
        size: iced::Size,
    ) -> Element<'a, Message> {
        let entries_filtered = action_log_filtered_entries(entries, &self.result, &self.feature);
        let count = entries_filtered.len();
        let range = visible_log_range(count, self.offset, size.height);
        let header = row![
            text("#").width(48),
            text(t!("action_log.time").to_string()).width(80),
            text(t!("action_log.feature").to_string()).width(140),
            text(t!("action_log.result").to_string()).width(90),
            text(t!("action_log.process").to_string()).width(160),
            text(t!("action_log.reason").to_string()).width(Fill)
        ]
        .spacing(design::space::SMALL);
        let mut entries_table =
            column![iced::widget::Space::new().height(range.start as f32 * ACTION_LOG_ROW_HEIGHT)]
                .spacing(0);
        if count == 0 {
            entries_table = entries_table.push(
                container(
                    text(if entries.is_empty() {
                        t!("action_log.empty").to_string()
                    } else {
                        t!("action_log.no_filter_matches").to_string()
                    })
                    .style(text::secondary),
                )
                .padding(design::space::LARGE as u16),
            );
        }
        for entry in entries_filtered[range.clone()].iter() {
            let result_style: fn(&iced::Theme) -> iced::widget::text::Style = match entry.result {
                ActionLogResult::Applied | ActionLogResult::Restored => text::success,
                ActionLogResult::Skipped => text::warning,
                ActionLogResult::Failed => text::danger,
            };
            let hovered = self.hovered == Some(entry.sequence);
            entries_table = entries_table.push(
                iced::widget::mouse_area(
                    container(
                        row![
                            text(format!("#{}", entry.sequence))
                                .size(design::typography::CAPTION)
                                .width(48),
                            text(action_log_time_label(entry.timestamp_epoch_ms))
                                .size(design::typography::CAPTION)
                                .width(80),
                            text(action_log_feature_label(entry.feature))
                                .size(design::typography::CAPTION)
                                .width(140),
                            text(action_log_result_text(entry.result))
                                .size(design::typography::CAPTION)
                                .style(result_style)
                                .width(90),
                            container(
                                text(action_log_process_label(entry))
                                    .size(design::typography::CAPTION)
                                    .wrapping(text::Wrapping::None)
                            )
                            .width(160)
                            .clip(true),
                            iced::widget::tooltip(
                                container(
                                    text(entry.reason.clone())
                                        .size(design::typography::CAPTION)
                                        .wrapping(text::Wrapping::None)
                                )
                                .width(Fill)
                                .clip(true),
                                text(entry.reason.clone()),
                                iced::widget::tooltip::Position::Top
                            )
                        ]
                        .spacing(design::space::SMALL)
                        .align_y(iced::Center),
                    )
                    .padding([0, design::space::MEDIUM as u16])
                    .center_y(ACTION_LOG_ROW_HEIGHT - 1.0)
                    .width(Fill)
                    .style(move |theme: &iced::Theme| {
                        iced::widget::container::Style {
                            background: hovered
                                .then(|| theme.palette().primary.scale_alpha(0.10).into()),
                            ..Default::default()
                        }
                    }),
                )
                .on_enter(Message::Hover(Some(entry.sequence)))
                .on_exit(Message::Hover(None)),
            );
            entries_table = entries_table.push(iced::widget::rule::horizontal(1));
        }
        entries_table = entries_table.push(
            iced::widget::Space::new().height((count - range.end) as f32 * ACTION_LOG_ROW_HEIGHT),
        );
        let table = column![
            container(header)
                .padding([0, design::space::MEDIUM as u16])
                .center_y(32),
            iced::widget::rule::horizontal(1),
            scrollable(entries_table)
                .id("action-log")
                .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y))
                .height(Fill),
        ]
        .height(Fill);
        let table = scrollable(
            container(table)
                .width(size.width.max(960.0))
                .height(Fill)
                .style(super::widgets::surface),
        )
        .direction(iced::widget::scrollable::Direction::Horizontal(
            iced::widget::scrollable::Scrollbar::default(),
        ))
        .width(Fill)
        .height(Fill);
        table.into()
    }
}
pub(super) fn action_log_feature_label(feature: ActionLogFeature) -> String {
    match feature {
        ActionLogFeature::AppSuspension => t!("nav.app_suspension").to_string(),
        ActionLogFeature::CpuSetsSoft => t!("nav.cpu_sets_soft").to_string(),
        ActionLogFeature::ProcessorAffinityHard => t!("nav.processor_affinity_hard").to_string(),
        ActionLogFeature::BackgroundEfficiency => t!("nav.background_efficiency").to_string(),
        ActionLogFeature::CpuLimiter => t!("nav.cpu_limiter").to_string(),
        ActionLogFeature::ByForeground => t!("nav.by_foreground").to_string(),
        ActionLogFeature::ByRunningApp => t!("nav.by_running_app").to_string(),
        ActionLogFeature::ByCpuLoad => t!("nav.by_cpu_load").to_string(),
        ActionLogFeature::ByActivity => t!("nav.by_activity").to_string(),
        ActionLogFeature::ByTime => t!("nav.by_time").to_string(),
        ActionLogFeature::CpuScheduler => t!("nav.cpu_scheduler").to_string(),
        ActionLogFeature::ProcessPriority => t!("nav.process_priority").to_string(),
        ActionLogFeature::ThreadPriority => t!("nav.thread_priority").to_string(),
        ActionLogFeature::DynamicPriorityBoost => t!("nav.dynamic_priority_boost").to_string(),
        ActionLogFeature::IoPriority => t!("nav.io_priority").to_string(),
        ActionLogFeature::GpuPriority => t!("nav.gpu_priority").to_string(),
        ActionLogFeature::MemoryPriority => t!("nav.memory_priority").to_string(),
        ActionLogFeature::MemoryTrim => t!("nav.memory_trim").to_string(),
        ActionLogFeature::TimerResolution => t!("nav.timer_resolution").to_string(),
    }
}

pub(super) fn action_log_result_label(result: ActionLogResult) -> String {
    action_log_result_text(result).into()
}

pub(super) fn action_log_result_text(result: ActionLogResult) -> &'static str {
    match result {
        ActionLogResult::Applied => "Applied",
        ActionLogResult::Restored => "Restored",
        ActionLogResult::Skipped => "Skipped",
        ActionLogResult::Failed => "Failed",
    }
}

pub(super) fn action_log_filtered_entries<'a>(
    entries: &'a [ActionLogEntry],
    result_filter: &[ActionLogResult],
    feature_filter: &[ActionLogFeature],
) -> Vec<&'a ActionLogEntry> {
    entries
        .iter()
        .rev()
        .filter(|entry| {
            result_filter.contains(&entry.result) && feature_filter.contains(&entry.feature)
        })
        .collect()
}

const RESULTS: [ActionLogResult; 4] = [
    ActionLogResult::Applied,
    ActionLogResult::Restored,
    ActionLogResult::Skipped,
    ActionLogResult::Failed,
];
const FEATURES: [ActionLogFeature; 19] = [
    ActionLogFeature::AppSuspension,
    ActionLogFeature::CpuSetsSoft,
    ActionLogFeature::ProcessorAffinityHard,
    ActionLogFeature::BackgroundEfficiency,
    ActionLogFeature::CpuLimiter,
    ActionLogFeature::ByForeground,
    ActionLogFeature::ByRunningApp,
    ActionLogFeature::ByCpuLoad,
    ActionLogFeature::ByActivity,
    ActionLogFeature::ByTime,
    ActionLogFeature::CpuScheduler,
    ActionLogFeature::ProcessPriority,
    ActionLogFeature::ThreadPriority,
    ActionLogFeature::DynamicPriorityBoost,
    ActionLogFeature::IoPriority,
    ActionLogFeature::GpuPriority,
    ActionLogFeature::MemoryPriority,
    ActionLogFeature::MemoryTrim,
    ActionLogFeature::TimerResolution,
];

const ACTION_LOG_ROW_HEIGHT: f32 = 36.0;

fn visible_log_range(count: usize, offset: f32, height: f32) -> std::ops::Range<usize> {
    let first = ((offset.max(0.0) / ACTION_LOG_ROW_HEIGHT).floor() as usize).min(count);
    let start = first.saturating_sub(8);
    let end = (first + (height.max(0.0) / ACTION_LOG_ROW_HEIGHT).ceil() as usize + 8).min(count);
    start..end
}
pub(super) fn action_log_action_label(result: ActionLogResult) -> &'static str {
    match result {
        ActionLogResult::Applied => "Apply",
        ActionLogResult::Restored => "Restore",
        ActionLogResult::Skipped => "Skip",
        ActionLogResult::Failed => "Fail",
    }
}

pub(super) fn action_log_entries_to_csv(entries: &[ActionLogEntry]) -> String {
    let mut csv = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(Vec::with_capacity(entries.len() * 128));
    csv.write_record([
        "sequence",
        "timestamp",
        "feature",
        "process_id",
        "process_name",
        "action",
        "result",
        "reason",
    ])
    .expect("writing CSV to memory cannot fail");
    for entry in entries {
        let sequence = entry.sequence.to_string();
        let timestamp = action_log_export_time_label(entry.timestamp_epoch_ms);
        let process_id = entry
            .process_id
            .map(|id| id.to_string())
            .unwrap_or_default();

        let feature = action_log_feature_label(entry.feature);
        csv.write_record([
            sequence.as_str(),
            timestamp.as_str(),
            feature.as_str(),
            process_id.as_str(),
            entry.process_name.as_str(),
            action_log_action_label(entry.result),
            action_log_result_text(entry.result),
            entry.reason.as_str(),
        ])
        .expect("writing CSV to memory cannot fail");
    }
    String::from_utf8(
        csv.into_inner()
            .expect("flushing CSV memory buffer cannot fail"),
    )
    .expect("CSV fields are valid UTF-8")
}

pub(super) fn action_log_export_time_label(timestamp_epoch_ms: u128) -> String {
    let timestamp = timestamp_epoch_ms.min(i64::MAX as u128) as i64;
    Local
        .timestamp_millis_opt(timestamp)
        .single()
        .map(|time| time.format("%Y-%m-%d %H:%M:%S%.3f %:z").to_string())
        .unwrap_or_else(|| timestamp_epoch_ms.to_string())
}

pub(super) fn action_log_process_label(entry: &ActionLogEntry) -> String {
    let name = if entry.process_name.trim().is_empty() {
        t!("common.none").to_string()
    } else {
        entry.process_name.clone()
    };
    match entry.process_id {
        Some(process_id) => format!("{name} ({})", process_id),
        None => name,
    }
}

pub(super) fn action_log_time_label(timestamp_epoch_ms: u128) -> String {
    let timestamp = timestamp_epoch_ms.min(i64::MAX as u128) as i64;
    Local
        .timestamp_millis_opt(timestamp)
        .single()
        .map(|time| time.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "--:--:--".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn virtual_log_rows_are_bounded_and_reach_the_end() {
        assert_eq!(visible_log_range(0, 0.0, 900.0), 0..0);
        let range = visible_log_range(10000, 3600.0, 900.0);
        assert_eq!(range, 92..133);
        assert_eq!(visible_log_range(52, 1500.0, 900.0).end, 52);
        let mut editor = Editor::default();
        editor.update(Message::Scrolled(3600.0));
        editor.update(Message::Result(ActionLogResult::Failed, false));
        assert_eq!(editor.offset, 0.0);
    }

    #[test]
    fn combined_filters_reverse_order_and_csv_preserves_quoted_unicode() {
        let mut entries = vec![ActionLogEntry {
            sequence: 1,
            timestamp_epoch_ms: 1_700_000_000_000,
            feature: ActionLogFeature::CpuLimiter,
            process_id: Some(42),
            process_name: "app.exe".into(),
            result: ActionLogResult::Applied,
            reason: "quoted \"value\", 測試\n".into(),
        }];
        entries.push(ActionLogEntry {
            sequence: 2,
            result: ActionLogResult::Failed,
            ..entries[0].clone()
        });
        assert_eq!(
            action_log_filtered_entries(
                &entries,
                &[ActionLogResult::Failed],
                &[ActionLogFeature::CpuLimiter]
            )[0]
            .sequence,
            2
        );
        assert_eq!(
            action_log_filtered_entries(&entries, &RESULTS, &FEATURES)[0].sequence,
            2
        );
        let mut editor = Editor::default();
        assert_eq!(
            action_log_filtered_entries(&entries, &editor.result, &editor.feature).len(),
            2
        );
        editor.update(Message::AllResults(false));
        assert!(action_log_filtered_entries(&entries, &editor.result, &editor.feature).is_empty());
        editor.update(Message::Result(ActionLogResult::Applied, true));
        editor.update(Message::Result(ActionLogResult::Failed, true));
        assert_eq!(
            action_log_filtered_entries(&entries, &editor.result, &editor.feature).len(),
            2
        );
        editor.update(Message::AllFeatures(false));
        assert!(action_log_filtered_entries(&entries, &editor.result, &editor.feature).is_empty());
        editor.update(Message::Feature(ActionLogFeature::CpuLimiter, true));
        assert_eq!(
            action_log_filtered_entries(&entries, &editor.result, &editor.feature).len(),
            2
        );
        editor.update(Message::AllResults(true));
        editor.update(Message::AllFeatures(true));
        assert_eq!(editor.result.len(), RESULTS.len());
        assert_eq!(editor.feature.len(), FEATURES.len());
        let csv = action_log_entries_to_csv(&entries);
        let records = csv::Reader::from_reader(csv.as_bytes())
            .records()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(&records[0][7], entries[0].reason);
    }
}
