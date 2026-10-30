//! Native virtual Action Log table; storage and export stay application-owned.
use super::widgets::{self, Choice, Ui};
use crate::action_log::{ActionLogEntry, ActionLogFeature, ActionLogResult};
use crate::config::ActionLogMode;
use crate::platform::windows::resident_shell::ui_pointer_capture_active;
use chrono::{Local, TimeZone};
use rust_i18n::t;
use std::{cell::RefCell, rc::Rc};
use windui::{
    event::MenuItem,
    prelude::{signal, Element, Role, Signal},
};

pub(super) struct Editor {
    result: Vec<ActionLogResult>,
    feature: Vec<ActionLogFeature>,
    native: Rc<RefCell<NativeRows>>,
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            result: RESULTS.to_vec(),
            feature: FEATURES.to_vec(),
            native: Rc::new(RefCell::new(NativeRows::default())),
        }
    }
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Result(ActionLogResult, bool),
    Feature(ActionLogFeature, bool),
    AllResults(bool),
    AllFeatures(bool),
    Clear,
    Export,
    LogMode(ActionLogMode),
}
#[derive(Default)]
struct NativeRows {
    refresh_pending: bool,
    groups: Vec<Vec<ActionLogEntry>>,
    source: Option<Signal<Vec<Vec<String>>>>,
    empty: Option<Signal<String>>,
}
impl Editor {
    pub(super) fn update(&mut self, message: Message) {
        match message {
            Message::Result(result, checked) => {
                self.result.retain(|value| *value != result);
                if checked {
                    self.result.push(result);
                }
            }
            Message::Feature(feature, checked) => {
                self.feature.retain(|value| *value != feature);
                if checked {
                    self.feature.push(feature);
                }
            }
            Message::AllResults(checked) => {
                self.result = if checked {
                    RESULTS.to_vec()
                } else {
                    Vec::new()
                }
            }
            Message::AllFeatures(checked) => {
                self.feature = if checked {
                    FEATURES.to_vec()
                } else {
                    Vec::new()
                }
            }
            Message::Clear | Message::Export | Message::LogMode(_) => {} // Application-owned persistence and export.
        }
    }

    pub(super) fn side_panel(
        &self,
        log_mode: ActionLogMode,
        has_entries: bool,
        has_summaries: bool,
        ui: Ui<Message>,
    ) -> Element {
        let mut results = widgets::col().child(
            widgets::row()
                .child(ui.button(
                    t!("action_log.check_all").to_string(),
                    Message::AllResults(true),
                ))
                .child(ui.button(
                    t!("action_log.clear_all").to_string(),
                    Message::AllResults(false),
                )),
        );
        for result in RESULTS {
            results = results.child(ui.checkbox(
                action_log_result_label(result),
                self.result.contains(&result),
                move |checked| Message::Result(result, checked),
            ));
        }
        let mut features = widgets::col().child(
            widgets::row()
                .child(ui.button(
                    t!("action_log.check_all").to_string(),
                    Message::AllFeatures(true),
                ))
                .child(ui.button(
                    t!("action_log.clear_all").to_string(),
                    Message::AllFeatures(false),
                )),
        );
        for feature in FEATURES {
            features = features.child(ui.checkbox(
                action_log_feature_label(feature),
                self.feature.contains(&feature),
                move |checked| Message::Feature(feature, checked),
            ));
        }
        let filters = widgets::col()
            .child(widgets::setting_row(
                t!("settings.action_log_mode").to_string(),
                ui.select(
                    ActionLogMode::ALL.map(|mode| Choice(mode, log_label(mode))),
                    Choice(log_mode, log_label(log_mode)),
                    |choice| Message::LogMode(choice.0),
                ),
            ))
            .child(widgets::card(
                t!("action_log.result_filter").to_string(),
                [results],
            ))
            .child(widgets::card(
                t!("action_log.feature_filter").to_string(),
                [features],
            ));
        widgets::col()
            .fill()
            .child(widgets::heading(t!("nav.settings").to_string()))
            .child(super::scrolling::scrollable(filters).fill().weight(1.0))
            .child(
                widgets::row()
                    .child(
                        ui.button(t!("action_log.clear").to_string(), Message::Clear)
                            .enabled(has_entries || has_summaries),
                    )
                    .child(
                        ui.button(t!("action_log.export_csv").to_string(), Message::Export)
                            .enabled(has_entries),
                    ),
            )
    }

    pub(super) fn refresh_view(&self, entries: &[ActionLogEntry]) {
        if ui_pointer_capture_active() {
            self.native.borrow_mut().refresh_pending = true;
            return;
        }
        if !self
            .native
            .borrow()
            .source
            .is_some_and(|source| source.is_alive())
        {
            self.release_presentation();
            return;
        }
        let filtered = action_log_filtered_entries(entries, &self.result, &self.feature);
        let groups = group_entries(&filtered);
        let values = groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                let first = group[0];
                vec![
                    format!("#{}", groups.len() - index),
                    action_log_time_label(first.timestamp_epoch_ms),
                    action_log_feature_label(first.feature),
                    action_log_result_label(first.result),
                    if group.len() == 1 {
                        action_log_process_label(first)
                    } else {
                        t!("action_log.affected_processes", count = group.len()).to_string()
                    },
                    first.reason.clone(),
                ]
            })
            .collect::<Vec<_>>();
        let mut native = self.native.borrow_mut();
        native.refresh_pending = false;
        native.groups = groups
            .iter()
            .map(|group| group.iter().map(|entry| (**entry).clone()).collect())
            .collect();
        if let Some(source) = native.source.filter(Signal::is_alive) {
            source.set(values);
        }
        if let Some(empty) = native.empty.filter(Signal::is_alive) {
            empty.set(
                t!(if entries.is_empty() {
                    "action_log.empty"
                } else {
                    "action_log.no_filter_matches"
                })
                .to_string(),
            );
        }
    }

    pub(super) fn flush_pending_refresh(&self, entries: &[ActionLogEntry]) {
        if self.native.borrow().refresh_pending {
            self.refresh_view(entries);
        }
    }

    /// The runtime owns log entries; these groups only support the current table.
    pub(super) fn release_presentation(&self) {
        *self.native.borrow_mut() = NativeRows::default();
    }

    pub(super) fn view(
        &self,
        entries: &[ActionLogEntry],
        candidates: &[super::app_picker::Candidate],
        ui: Ui<Message>,
    ) -> Element {
        let rows = signal(Vec::new());
        let empty = signal(
            t!(if entries.is_empty() {
                "action_log.empty"
            } else {
                "action_log.no_filter_matches"
            })
            .to_string(),
        );
        {
            let mut native = self.native.borrow_mut();
            native.source = Some(rows);
            native.empty = Some(empty);
        }
        self.refresh_view(entries);
        let cells = Rc::clone(&self.native);
        let menus = Rc::clone(&self.native);
        let candidates = candidates.to_vec();
        let table =
            Element::table_virtual(
                vec![
                    ("#".to_owned(), 48.0),
                    (t!("action_log.time").to_string(), 80.0),
                    (t!("action_log.feature").to_string(), 140.0),
                    (t!("action_log.result").to_string(), 90.0),
                    (t!("action_log.process").to_string(), 160.0),
                    (t!("action_log.reason").to_string(), 360.0),
                ],
                rows,
                ACTION_LOG_ROW_HEIGHT as i32,
            )
            .cell_render(move |index, column, value| {
                let native = cells.borrow();
                let group = native.groups.get(index)?;
                match column {
                    3 => Some(widgets::label(value).tooltip(value).fg_role(
                        match group[0].result {
                            ActionLogResult::Applied | ActionLogResult::Restored => Role::Success,
                            ActionLogResult::Skipped => Role::Warning,
                            ActionLogResult::Failed => Role::Danger,
                        },
                    )),
                    4 => Some(process_group(group, &ui.candidates(&candidates))),
                    5 => Some(widgets::label(value).tooltip(group[0].reason.clone())),
                    _ => Some(widgets::label(value).tooltip(value)),
                }
            })
            .on_row_context_menu(move |index| {
                menus
                    .borrow()
                    .groups
                    .get(index)
                    .map(|group| group_menu(group))
                    .unwrap_or_default()
            })
            .fill()
            .weight(1.0);
        widgets::col()
            .fill()
            .child(
                Element::label_signal(empty)
                    .fg_role(Role::TextMuted)
                    .visible_when(move || rows.get().is_empty())
                    .reactive(),
            )
            .child(super::scrolling::preserve_table_rows(table))
    }
}

fn group_menu(entries: &[ActionLogEntry]) -> Vec<MenuItem> {
    let mut menu = vec![MenuItem::run(
        t!("action_log.affected_processes", count = entries.len()).to_string(),
        |_| {},
        false,
    )
    .enabled(false)];
    for entry in entries {
        menu.push(
            MenuItem::run(action_log_process_label(entry), |_| {}, false)
                .enabled(false)
                .subtitle(entry.reason.clone()),
        );
    }
    menu
}

fn process_group(
    entries: &[ActionLogEntry],
    candidates: &[super::app_picker::Candidate],
) -> Element {
    if entries.len() == 1 && entries[0].process_id.is_none() && entries[0].process_name.is_empty() {
        return widgets::label("\u{2014}").fg_role(Role::TextMuted);
    }
    let mut strip = widgets::row();
    for entry in entries.iter().take(5) {
        if let Some(icon) = super::app_picker::icon_for_name(&entry.process_name, candidates) {
            strip = strip.child(
                Element::image_rgba(icon.width, icon.height, &icon.rgba)
                    .width(16)
                    .height(16),
            );
        }
    }
    let label = if entries.len() == 1 {
        action_log_process_label(&entries[0])
    } else {
        t!("action_log.affected_processes", count = entries.len()).to_string()
    };
    let items = entries.to_vec();
    strip.child(
        Element::button(label.clone())
            .small()
            .tooltip(label)
            .on_click(move |ctx| {
                widgets::flush_bindings(ctx);
                ctx.show_dropdown_menu(ctx.bounds(), group_menu(&items));
            })
            .weight(1.0),
    )
}

// Group matching actions throughout each automation pass, newest groups first.
fn group_entries<'a>(entries: &[&'a ActionLogEntry]) -> Vec<Vec<&'a ActionLogEntry>> {
    let mut groups: Vec<Vec<&ActionLogEntry>> = Vec::new();
    for &entry in entries {
        if let Some(group) = groups
            .iter_mut()
            .rev()
            .take_while(|group| group[0].batch_id == entry.batch_id)
            .find(|group| {
                let first = group[0];
                entry.process_id.is_some()
                    && first.process_id.is_some()
                    && entry.feature == first.feature
                    && entry.result == first.result
                    && entry.reason == first.reason
                    && !group.iter().any(|old| old.process_id == entry.process_id)
            })
        {
            group.push(entry);
            continue;
        }
        groups.push(vec![entry]);
    }
    groups
}

pub(super) fn action_log_feature_label(feature: ActionLogFeature) -> String {
    let page = match feature {
        ActionLogFeature::AppSuspension => super::Page::AppSuspension,
        ActionLogFeature::CpuSetsSoft => super::Page::CpuSetsSoft,
        ActionLogFeature::ProcessorAffinityHard => super::Page::ProcessorAffinityHard,
        ActionLogFeature::BackgroundEfficiency => super::Page::BackgroundEfficiency,
        ActionLogFeature::CpuLimiter => super::Page::CpuLimiter,
        ActionLogFeature::ByForeground => super::Page::ByForeground,
        ActionLogFeature::ByRunningApp => super::Page::ByRunningApp,
        ActionLogFeature::ByCpuLoad => super::Page::ByCpuLoad,
        ActionLogFeature::ByActivity => super::Page::ByActivity,
        ActionLogFeature::ByTime => super::Page::ByTime,
        ActionLogFeature::AdaptiveEngine => super::Page::AdaptiveEngine,
        ActionLogFeature::ProcessPriority => super::Page::ProcessPriority,
        ActionLogFeature::ThreadPriority => super::Page::ThreadPriority,
        ActionLogFeature::DynamicPriorityBoost => super::Page::DynamicPriorityBoost,
        ActionLogFeature::IoPriority => super::Page::IoPriority,
        ActionLogFeature::GpuPriority => super::Page::GpuPriority,
        ActionLogFeature::MemoryPriority => super::Page::MemoryPriority,
        ActionLogFeature::MemoryTrim => super::Page::MemoryTrim,
        ActionLogFeature::TimerResolution => super::Page::TimerResolution,
    };
    page.label()
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
    ActionLogFeature::AdaptiveEngine,
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

fn log_label(v: ActionLogMode) -> String {
    match v {
        ActionLogMode::Full => t!("settings.action_log_mode_full"),
        ActionLogMode::Warning => t!("settings.action_log_mode_warning"),
        ActionLogMode::Error => t!("settings.action_log_mode_error"),
        ActionLogMode::Off => t!("settings.action_log_mode_off"),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_log_source_tracks_filters_without_losing_export_entries() {
        let entries = vec![ActionLogEntry {
            batch_id: 1,
            sequence: 1,
            timestamp_epoch_ms: 0,
            feature: ActionLogFeature::CpuLimiter,
            process_id: Some(10),
            process_name: String::from("app.exe"),
            result: ActionLogResult::Applied,
            reason: String::from("Applied"),
        }];
        let mut editor = Editor::default();
        editor.refresh_view(&entries);
        assert!(editor.native.borrow().groups.is_empty());
        let mut scope = windui::signal::SignalScope::new();
        let source = scope.collect(|| signal(Vec::new()));
        editor.native.borrow_mut().source = Some(source);
        editor.refresh_view(&entries);
        assert_eq!(source.get().len(), 1);
        editor.update(Message::AllResults(false));
        editor.flush_pending_refresh(&entries);
        assert_eq!(source.get().len(), 1);
        editor.native.borrow_mut().refresh_pending = true;
        editor.flush_pending_refresh(&entries);
        assert!(source.get().is_empty());
        assert!(editor.native.borrow().groups.is_empty());
        assert!(!editor.native.borrow().refresh_pending);
        assert_eq!(action_log_entries_to_csv(&entries).lines().count(), 2);
        drop(scope);
        editor.refresh_view(&entries);
        assert!(editor.native.borrow().groups.is_empty());
        assert!(editor.native.borrow().source.is_none());
        assert_eq!(action_log_entries_to_csv(&entries).lines().count(), 2);
    }

    #[test]
    fn presentation_release_preserves_filters_and_rebuilds_the_next_table() {
        let entries = vec![ActionLogEntry {
            batch_id: 1,
            sequence: 1,
            timestamp_epoch_ms: 0,
            feature: ActionLogFeature::CpuLimiter,
            process_id: Some(10),
            process_name: "app.exe".into(),
            result: ActionLogResult::Applied,
            reason: "Applied".into(),
        }];
        let mut editor = Editor::default();
        editor.update(Message::Result(ActionLogResult::Failed, false));
        editor.update(Message::Feature(ActionLogFeature::AdaptiveEngine, false));
        let results = editor.result.clone();
        let features = editor.feature.clone();
        let mut scope = windui::signal::SignalScope::new();
        scope.collect(|| {
            let _ = editor.view(&entries, &[], Ui::new(|_: Message, _| {}));
            assert_eq!(editor.native.borrow().source.unwrap().get().len(), 1);
            assert_eq!(editor.native.borrow().groups[0][0], entries[0]);
        });
        drop(scope);
        editor.release_presentation();
        assert!(editor.native.borrow().groups.is_empty());
        assert_eq!(editor.result, results);
        assert_eq!(editor.feature, features);
        let mut reopened = windui::signal::SignalScope::new();
        reopened.collect(|| {
            let _ = editor.view(&entries, &[], Ui::new(|_: Message, _| {}));
            assert_eq!(editor.native.borrow().source.unwrap().get().len(), 1);
        });
    }

    #[test]
    fn batches_group_without_merging_other_operations_or_exports() {
        let mut log = crate::action_log::ActionLog::new(100);
        log.begin_batch();
        for pid in 1..=40 {
            log.record(
                ActionLogFeature::AdaptiveEngine,
                Some(pid),
                format!("app-{pid}.exe"),
                ActionLogResult::Applied,
                "Applied background restraint.",
            );
        }
        let entries = log.entries();
        let filtered = action_log_filtered_entries(&entries, &RESULTS, &FEATURES);
        assert_eq!(
            group_entries(&filtered)
                .iter()
                .map(Vec::len)
                .collect::<Vec<_>>(),
            vec![40]
        );
        assert_eq!(action_log_entries_to_csv(&entries).lines().count(), 41);
        log.begin_batch();
        log.record(
            ActionLogFeature::AdaptiveEngine,
            Some(41),
            "next.exe",
            ActionLogResult::Applied,
            "Applied background restraint.",
        );
        for pid in 42..=43 {
            log.record(
                ActionLogFeature::AdaptiveEngine,
                Some(pid),
                "failed.exe",
                ActionLogResult::Failed,
                "Access denied.",
            );
        }
        let entries = log.entries();
        let filtered = action_log_filtered_entries(&entries, &RESULTS, &FEATURES);
        assert_eq!(
            group_entries(&filtered)
                .iter()
                .map(Vec::len)
                .collect::<Vec<_>>(),
            vec![2, 1, 40]
        );
        for change in 0..4 {
            let mut other = entries[0].clone();
            other.sequence = 99;
            other.process_id = Some(99);
            match change {
                0 => other.feature = ActionLogFeature::CpuLimiter,
                1 => other.reason = "Different operation".into(),
                2 => other.result = ActionLogResult::Restored,
                _ => other.process_id = entries[0].process_id,
            }
            assert_eq!(group_entries(&[&entries[0], &other]).len(), 2);
        }
    }

    #[test]
    fn every_feature_and_result_groups_matching_process_actions() {
        for feature in FEATURES {
            for result in RESULTS {
                let mut log = crate::action_log::ActionLog::new(8);
                log.begin_batch();
                for pid in 1..=3 {
                    log.record(
                        feature,
                        Some(pid),
                        format!("app-{pid}.exe"),
                        result,
                        "Same action.",
                    );
                    log.record(
                        feature,
                        Some(pid + 3),
                        format!("other-{pid}.exe"),
                        result,
                        "Different action.",
                    );
                }
                let entries = log.entries();
                let filtered = action_log_filtered_entries(&entries, &RESULTS, &FEATURES);
                let groups = group_entries(&filtered);
                assert_eq!(groups.len(), 2, "{feature:?} {result:?}");
                assert_eq!(groups[0].len(), 3);
                assert_eq!(groups[1].len(), 3);
                assert_eq!(groups[0][0].reason, "Different action.");
                assert_eq!(action_log_entries_to_csv(&entries).lines().count(), 7);
            }
        }
    }

    #[test]
    fn combined_filters_reverse_order_and_csv_preserves_quoted_unicode() {
        let mut entries = vec![ActionLogEntry {
            batch_id: 1,
            sequence: 1,
            timestamp_epoch_ms: 1_700_000_000_000,
            feature: ActionLogFeature::CpuLimiter,
            process_id: Some(42),
            process_name: "app.exe".into(),
            result: ActionLogResult::Applied,
            reason: "quoted \"value\", 測試\n".into(),
        }];
        entries.push(ActionLogEntry {
            batch_id: 2,
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
