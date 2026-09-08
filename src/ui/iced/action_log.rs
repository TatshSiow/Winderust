use crate::action_log::{ActionLogEntry, ActionLogFeature, ActionLogResult};
use chrono::{Local, TimeZone};
use iced::widget::{button, column, container, pick_list, row, scrollable, text};
use iced::{Element, Fill};
use rust_i18n::t;

pub(super) struct Editor {
    result: ActionLogResultFilter,
    feature: ActionLogFeatureFilter,
    page: usize,
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            result: ActionLogResultFilter::All,
            feature: ActionLogFeatureFilter::All,
            page: 0,
        }
    }
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Result(ActionLogResultFilter),
    Feature(ActionLogFeatureFilter),
    Page(usize),
    Clear,
    Export,
}
impl Editor {
    pub(super) fn update(&mut self, message: Message) {
        match message {
            Message::Result(result) => {
                self.result = result;
                self.page = 0;
            }
            Message::Feature(feature) => {
                self.feature = feature;
                self.page = 0;
            }
            Message::Page(page) => self.page = page,
            Message::Clear => self.page = 0,
            Message::Export => {} // The application owns the native CSV destination picker.
        }
    }
    pub(super) fn view<'a>(
        &'a self,
        entries: &'a [ActionLogEntry],
        has_summaries: bool,
    ) -> Element<'a, Message> {
        let entries_filtered = action_log_filtered_entries(entries, self.result, self.feature);
        let count = entries_filtered.len();
        let pages = action_log_page_count(count);
        let page = self.page.min(pages.saturating_sub(1));
        let start = page * ACTION_LOG_PAGE_SIZE;
        let end = (start + ACTION_LOG_PAGE_SIZE).min(count);
        let mut body = column![
            row![
                text(t!("action_log.feature_filter").to_string()).width(160),
                pick_list(
                    ActionLogFeatureFilter::ALL,
                    Some(self.feature),
                    Message::Feature
                )
            ]
            .spacing(8),
            row![
                text(t!("action_log.result_filter").to_string()).width(160),
                pick_list(
                    ActionLogResultFilter::ALL,
                    Some(self.result),
                    Message::Result
                )
            ]
            .spacing(8),
            row![
                button(text(t!("action_log.clear").to_string())).on_press_maybe(
                    (!entries.is_empty() || has_summaries).then_some(Message::Clear)
                ),
                button(text(t!("action_log.export_csv").to_string()))
                    .on_press_maybe((!entries.is_empty()).then_some(Message::Export)),
                button(text(t!("action_log.previous").to_string()))
                    .on_press_maybe((page > 0).then_some(Message::Page(page.saturating_sub(1)))),
                button(text(t!("action_log.next").to_string()))
                    .on_press_maybe((page + 1 < pages).then_some(Message::Page(page + 1)))
            ]
            .spacing(8),
            text(t!("action_log.recent_entries").to_string()).size(18),
            text(action_log_pagination_label(count, page, pages, start, end)),
        ]
        .spacing(12);
        if count == 0 {
            body = body.push(text(
                if entries.is_empty() {
                    t!("action_log.empty")
                } else {
                    t!("action_log.no_filter_matches")
                }
                .to_string(),
            ));
        }
        for entry in entries_filtered
            .iter()
            .skip(start)
            .take(ACTION_LOG_PAGE_SIZE)
        {
            let result_color = match entry.result {
                ActionLogResult::Applied | ActionLogResult::Restored => {
                    iced::Color::from_rgb8(40, 150, 100)
                }
                ActionLogResult::Skipped => iced::Color::from_rgb8(180, 120, 30),
                ActionLogResult::Failed => iced::Color::from_rgb8(210, 60, 50),
            };
            body = body.push(
                container(
                    column![
                        row![
                            text(format!("#{}", entry.sequence)).width(56),
                            text(action_log_time_label(entry.timestamp_epoch_ms)).width(80),
                            text(action_log_feature_label(entry.feature)).width(Fill),
                            text(action_log_result_text(entry.result)).color(result_color)
                        ]
                        .spacing(8),
                        text(action_log_process_label(entry)),
                        text(entry.reason.clone())
                    ]
                    .spacing(6),
                )
                .padding(10)
                .width(Fill)
                .style(container::rounded_box),
            );
        }
        scrollable(body).height(Fill).into()
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

pub(super) fn action_log_filter_label(filter: ActionLogResultFilter) -> String {
    match filter {
        ActionLogResultFilter::All => t!("action_log.filter_all").to_string(),
        ActionLogResultFilter::Applied => {
            action_log_result_label(ActionLogResult::Applied).to_string()
        }
        ActionLogResultFilter::Restored => {
            action_log_result_label(ActionLogResult::Restored).to_string()
        }
        ActionLogResultFilter::Skipped => {
            action_log_result_label(ActionLogResult::Skipped).to_string()
        }
        ActionLogResultFilter::Failed => {
            action_log_result_label(ActionLogResult::Failed).to_string()
        }
    }
}

pub(super) fn action_log_feature_filter_label(filter: ActionLogFeatureFilter) -> String {
    match filter {
        ActionLogFeatureFilter::All => t!("action_log.filter_all").to_string(),
        ActionLogFeatureFilter::Feature(feature) => action_log_feature_label(feature),
    }
}

pub(super) fn action_log_filtered_entries(
    entries: &[ActionLogEntry],
    result_filter: ActionLogResultFilter,
    feature_filter: ActionLogFeatureFilter,
) -> Vec<&ActionLogEntry> {
    entries
        .iter()
        .rev()
        .filter(|entry| {
            result_filter.matches(entry.result) && feature_filter.matches(entry.feature)
        })
        .collect()
}

pub(super) fn action_log_page_count(total_entries: usize) -> usize {
    total_entries.div_ceil(ACTION_LOG_PAGE_SIZE)
}

pub(super) fn action_log_pagination_label(
    total_entries: usize,
    current_page: usize,
    page_count: usize,
    page_start: usize,
    page_end: usize,
) -> String {
    if total_entries == 0 {
        t!("action_log.pagination_empty").to_string()
    } else {
        t!(
            "action_log.pagination",
            start = page_start + 1,
            end = page_end,
            total = total_entries,
            current = current_page + 1,
            pages = page_count.max(1)
        )
        .to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActionLogResultFilter {
    All,
    Applied,
    Restored,
    Skipped,
    Failed,
}

impl ActionLogResultFilter {
    const ALL: [Self; 5] = [
        Self::All,
        Self::Applied,
        Self::Restored,
        Self::Skipped,
        Self::Failed,
    ];

    fn matches(self, result: ActionLogResult) -> bool {
        match self {
            Self::All => true,
            Self::Applied => result == ActionLogResult::Applied,
            Self::Restored => result == ActionLogResult::Restored,
            Self::Skipped => result == ActionLogResult::Skipped,
            Self::Failed => result == ActionLogResult::Failed,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActionLogFeatureFilter {
    All,
    Feature(ActionLogFeature),
}

impl ActionLogFeatureFilter {
    const ALL: [Self; 20] = [
        Self::All,
        Self::Feature(ActionLogFeature::AppSuspension),
        Self::Feature(ActionLogFeature::CpuSetsSoft),
        Self::Feature(ActionLogFeature::ProcessorAffinityHard),
        Self::Feature(ActionLogFeature::BackgroundEfficiency),
        Self::Feature(ActionLogFeature::CpuLimiter),
        Self::Feature(ActionLogFeature::ByForeground),
        Self::Feature(ActionLogFeature::ByRunningApp),
        Self::Feature(ActionLogFeature::ByCpuLoad),
        Self::Feature(ActionLogFeature::ByActivity),
        Self::Feature(ActionLogFeature::ByTime),
        Self::Feature(ActionLogFeature::CpuScheduler),
        Self::Feature(ActionLogFeature::ProcessPriority),
        Self::Feature(ActionLogFeature::ThreadPriority),
        Self::Feature(ActionLogFeature::DynamicPriorityBoost),
        Self::Feature(ActionLogFeature::IoPriority),
        Self::Feature(ActionLogFeature::GpuPriority),
        Self::Feature(ActionLogFeature::MemoryPriority),
        Self::Feature(ActionLogFeature::MemoryTrim),
        Self::Feature(ActionLogFeature::TimerResolution),
    ];

    fn matches(self, feature: ActionLogFeature) -> bool {
        match self {
            Self::All => true,
            Self::Feature(filter_feature) => filter_feature == feature,
        }
    }
}

const ACTION_LOG_PAGE_SIZE: usize = 15;
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

impl std::fmt::Display for ActionLogFeatureFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&action_log_feature_filter_label(*self))
    }
}
impl std::fmt::Display for ActionLogResultFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&action_log_filter_label(*self))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
                ActionLogResultFilter::Failed,
                ActionLogFeatureFilter::Feature(ActionLogFeature::CpuLimiter)
            )[0]
            .sequence,
            2
        );
        assert_eq!(
            action_log_filtered_entries(
                &entries,
                ActionLogResultFilter::All,
                ActionLogFeatureFilter::All
            )[0]
            .sequence,
            2
        );
        let csv = action_log_entries_to_csv(&entries);
        let records = csv::Reader::from_reader(csv.as_bytes())
            .records()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(&records[0][7], entries[0].reason);
        assert_eq!(action_log_page_count(16), 2);
    }
}
