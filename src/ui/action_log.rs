use std::{num::NonZeroU32, sync::Arc};

use super::popover;

use super::design;
use super::widgets::{
    button, checkbox, pick_list, search_field, text_input, Choice, SEARCH_INPUT_PADDING,
};
use crate::action_log::{ActionLogEntry, ActionLogFeature, ActionLogLevel, ActionLogResult};
use crate::config::ActionLogMode;
use crate::ui::scrolling::scrollable;
use chrono::{Local, TimeZone};
use iced::widget::{column, container, row, text};
use iced::{keyboard::key::Named, Element, Fill, Task};
use rust_i18n::t;

pub(super) struct Editor {
    level: Vec<ActionLogLevel>,
    feature: Vec<ActionLogFeature>,
    search: String,
    offset: std::cell::Cell<f32>,
    pending_export: Option<Arc<Vec<ActionLogEntry>>>,
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            level: ActionLogLevel::ALL.to_vec(),
            feature: FEATURES.to_vec(),
            search: String::new(),
            offset: std::cell::Cell::new(0.0),
            pending_export: None,
        }
    }
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Level(ActionLogLevel, bool),
    Feature(ActionLogFeature, bool),
    AllLevels(bool),
    AllFeatures(bool),
    Search(String),
    Scrolled(f32),
    KeyboardScroll(Scroll, f32),
    Clear,
    Export,
    LogMode(ActionLogMode),
}
#[derive(Debug, Clone, Copy)]
pub(super) enum Scroll {
    PageUp,
    PageDown,
    Start,
    End,
    Horizontal(f32),
}

impl Scroll {
    fn from_key(key: Named, height: f32) -> Option<(Message, bool)> {
        let (direction, repeatable) = match key {
            Named::PageUp => (Self::PageUp, true),
            Named::PageDown => (Self::PageDown, true),
            Named::Home => (Self::Start, false),
            Named::End => (Self::End, false),
            Named::ArrowLeft => (Self::Horizontal(-ACTION_LOG_HORIZONTAL_STEP), true),
            Named::ArrowRight => (Self::Horizontal(ACTION_LOG_HORIZONTAL_STEP), true),
            _ => return None,
        };
        Some((Message::KeyboardScroll(direction, height), repeatable))
    }
}

pub(super) fn scroll_to<M: Send + 'static>(offset: f32) -> Task<M> {
    iced::widget::operation::scroll_to(
        ACTION_LOG_SCROLL_ID,
        iced::widget::scrollable::AbsoluteOffset {
            x: None,
            y: Some(offset),
        },
    )
}

fn scroll_horizontal<M>(delta: f32) -> impl iced::advanced::widget::Operation<M> {
    use iced::advanced::widget::operation::scrollable::{scroll_by, AbsoluteOffset};
    scroll_by(
        ACTION_LOG_HORIZONTAL_SCROLL_ID.into(),
        AbsoluteOffset { x: delta, y: 0.0 },
    )
}

impl Editor {
    pub(super) fn keyboard_scroll<M: Send + 'static>(
        &self,
        entries: &[ActionLogEntry],
        direction: Scroll,
        height: f32,
    ) -> Task<M> {
        let max_offset = || {
            let count = group_entries(action_log_filtered_entries(
                entries,
                &self.level,
                &self.feature,
                &self.search,
            ))
            .len();
            (count as f32 * ACTION_LOG_ROW_HEIGHT - height).max(0.0)
        };
        let page = (height - ACTION_LOG_ROW_HEIGHT).max(ACTION_LOG_ROW_HEIGHT);
        let previous = self.offset.get();
        let offset = match direction {
            Scroll::PageUp => (previous - page).clamp(0.0, max_offset()),
            Scroll::PageDown => (previous + page).clamp(0.0, max_offset()),
            Scroll::Start => 0.0,
            Scroll::End => max_offset(),
            Scroll::Horizontal(delta) => {
                return iced::advanced::widget::operate(scroll_horizontal(delta));
            }
        };
        // Rebuild the virtual rows before jumping the native scrollable.
        self.offset.set(offset);
        scroll_to(offset)
    }

    pub(super) fn begin_export(&mut self, entries: &Arc<Vec<ActionLogEntry>>) -> bool {
        if entries.is_empty() || self.pending_export.is_some() {
            return false;
        }
        self.pending_export = Some(Arc::clone(entries));
        true
    }

    pub(super) fn finish_export(&mut self) -> Option<Arc<Vec<ActionLogEntry>>> {
        self.pending_export.take()
    }

    pub(super) fn update(&mut self, message: Message) {
        match message {
            Message::Level(level, checked) => {
                self.level.retain(|value| *value != level);
                if checked {
                    self.level.push(level);
                }
                self.offset.set(0.0);
            }
            Message::Feature(feature, checked) => {
                self.feature.retain(|value| *value != feature);
                if checked {
                    self.feature.push(feature);
                }
                self.offset.set(0.0);
            }
            Message::AllLevels(checked) => {
                self.level = if checked {
                    ActionLogLevel::ALL.to_vec()
                } else {
                    Vec::new()
                };
                self.offset.set(0.0);
            }
            Message::AllFeatures(checked) => {
                self.feature = if checked {
                    FEATURES.to_vec()
                } else {
                    Vec::new()
                };
                self.offset.set(0.0);
            }
            Message::Search(query) => {
                self.search = query;
                self.offset.set(0.0);
            }
            Message::Scrolled(offset) => self.offset.set(offset),
            Message::Clear => {
                self.offset.set(0.0);
            }
            // The application handles keyboard scrolling, CSV export and persisted settings.
            Message::KeyboardScroll(..) | Message::Export | Message::LogMode(_) => {}
        }
    }
    pub(super) fn side_panel(
        &self,
        log_mode: ActionLogMode,
        has_entries: bool,
        has_summaries: bool,
    ) -> Element<'_, Message> {
        let mut levels = column![row![
            button(container(text(t!("action_log.check_all").to_string())).center_x(Fill))
                .width(Fill)
                .height(32)
                .on_press(Message::AllLevels(true)),
            button(container(text(t!("action_log.clear_all").to_string())).center_x(Fill))
                .width(Fill)
                .height(32)
                .on_press(Message::AllLevels(false)),
        ]
        .spacing(design::space::SMALL),]
        .spacing(design::space::SMALL);
        for level in ActionLogLevel::ALL {
            levels = levels.push(
                checkbox(self.level.contains(&level))
                    .label(action_log_level_label(level))
                    .on_toggle(move |checked| Message::Level(level, checked)),
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
                super::widgets::settings_card(
                    column![
                        super::widgets::setting_title("settings.action_log_mode"),
                        pick_list(
                            ActionLogMode::ALL.map(|v| Choice(v, log_label(v))).to_vec(),
                            Some(Choice(log_mode, log_label(log_mode))),
                            |v| Message::LogMode(v.0),
                        )
                        .width(Fill),
                    ]
                    .spacing(design::space::SMALL)
                ),
                super::widgets::heading(
                    t!("process_list.filter").to_string(),
                    design::typography::SECONDARY
                ),
                super::widgets::settings_card(
                    column![
                        super::widgets::setting_title("action_log.level_filter"),
                        levels
                    ]
                    .spacing(design::space::SMALL)
                ),
                super::widgets::settings_card(
                    column![
                        super::widgets::setting_title("action_log.feature_filter"),
                        features
                    ]
                    .spacing(design::space::SMALL)
                ),
            ]
            .spacing(design::space::MEDIUM)
            .padding([0, design::space::MEDIUM as u16]),
        )
        .height(Fill);
        column![
            super::widgets::panel_heading(t!("nav.settings").to_string()),
            filters,
            iced::widget::rule::horizontal(1),
            container(
                row![
                    button(container(text(t!("action_log.clear").to_string())).center_x(Fill))
                        .width(Fill)
                        .height(32)
                        .style(super::widgets::danger_button)
                        .on_press_maybe((has_entries || has_summaries).then_some(Message::Clear)),
                    button(container(text(t!("action_log.export_csv").to_string())).center_x(Fill))
                        .width(Fill)
                        .height(32)
                        .on_press_maybe(
                            (has_entries && self.pending_export.is_none())
                                .then_some(Message::Export)
                        ),
                ]
                .spacing(design::space::SMALL)
            )
            .padding([design::space::MEDIUM as u16, 0]),
        ]
        .spacing(design::space::MEDIUM)
        .height(Fill)
        .into()
    }

    pub(super) fn view<'a>(
        &'a self,
        entries: &'a [ActionLogEntry],
        candidates: &'a [super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        column![
            super::scrolling::repaint_group(search_field(
                text_input(&t!("action_log.search_placeholder"), &self.search)
                    .padding(SEARCH_INPUT_PADDING)
                    .on_input(Message::Search)
                    .width(280),
                (!self.search.is_empty()).then(|| Message::Search(String::new())),
            )),
            super::widgets::table_frame(iced::widget::responsive(move |size| {
                self.view_at_size(entries, candidates, size)
            })),
        ]
        .spacing(design::space::SMALL)
        .height(Fill)
        .into()
    }

    fn view_at_size<'a>(
        &'a self,
        entries: &'a [ActionLogEntry],
        candidates: &'a [super::app_picker::Candidate],
        size: iced::Size,
    ) -> Element<'a, Message> {
        let groups = group_entries(action_log_filtered_entries(
            entries,
            &self.level,
            &self.feature,
            &self.search,
        ));
        let count = groups.len();
        let body_height =
            size.height - ACTION_LOG_HEADER_HEIGHT - super::scrolling::SCROLLBAR_GUTTER;
        let range = visible_log_range(count, self.offset.get(), body_height);
        let header = row![
            text("#").width(48),
            text(t!("action_log.time").to_string()).width(80),
            text(t!("action_log.feature").to_string()).width(140),
            text(t!("common.level").to_string()).width(64),
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
        for (index, group) in groups
            .into_iter()
            .skip(range.start)
            .take(range.len())
            .enumerate()
        {
            let entry = group.first;
            let level = entry.result.level();
            entries_table = entries_table.push(
                container(
                    row![
                        text(format!("#{}", count - range.start - index))
                            .size(design::typography::CAPTION)
                            .width(48),
                        text(action_log_time_label(entry.timestamp_epoch_ms))
                            .size(design::typography::CAPTION)
                            .width(80),
                        text(action_log_feature_label(entry.feature))
                            .size(design::typography::CAPTION)
                            .width(140),
                        text(action_log_level_label(level))
                            .size(design::typography::CAPTION)
                            .style(move |theme| action_log_level_style(theme, level))
                            .width(64),
                        text(action_log_result_label(entry.result))
                            .size(design::typography::CAPTION)
                            .width(90),
                        process_group(group, candidates),
                        {
                            let fitted = super::widgets::fitted_text_with_font(
                                &entry.reason,
                                size.width.max(1024.0) - 672.0,
                                design::typography::CAPTION,
                                design::typography::DEFAULT_FONT,
                            );
                            let truncated = fitted != entry.reason;
                            let content: Element<'_, Message> = container(
                                text(fitted)
                                    .size(design::typography::CAPTION)
                                    .font(design::typography::DEFAULT_FONT)
                                    .wrapping(text::Wrapping::None),
                            )
                            .width(Fill)
                            .clip(true)
                            .into();
                            if truncated {
                                popover::view(entry.sequence, content, move || {
                                    container(super::scrolling::popup_scrollable(
                                        text(entry.reason.as_str())
                                            .font(design::typography::DEFAULT_FONT)
                                            .size(design::typography::SECONDARY)
                                            .wrapping(text::Wrapping::WordOrGlyph),
                                    ))
                                    .padding(design::space::SMALL as u16)
                                    .style(super::widgets::tooltip_surface)
                                    .into()
                                })
                            } else {
                                content
                            }
                        }
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center),
                )
                .padding([0, design::space::MEDIUM as u16])
                .center_y(ACTION_LOG_ROW_HEIGHT - 1.0)
                .width(Fill),
            );
            entries_table = entries_table.push(iced::widget::rule::horizontal(1));
        }
        entries_table = entries_table.push(
            iced::widget::Space::new().height((count - range.end) as f32 * ACTION_LOG_ROW_HEIGHT),
        );
        let table = column![
            container(header)
                .padding([0, design::space::MEDIUM as u16])
                .center_y(ACTION_LOG_HEADER_HEIGHT - 1.0),
            iced::widget::rule::horizontal(1),
            super::motion::wrap_keys(
                super::scrolling::buffered(
                    scrollable(entries_table)
                        .id(ACTION_LOG_SCROLL_ID)
                        .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y))
                        .height(Fill)
                        .into(),
                    &self.offset,
                    range.start as f32 * ACTION_LOG_ROW_HEIGHT,
                    range.end as f32 * ACTION_LOG_ROW_HEIGHT,
                    range.start == 0,
                    range.end == count,
                    |message| match message {
                        Message::Scrolled(offset) => Some(*offset),
                        _ => None,
                    }
                ),
                ACTION_LOG_TABLE_ID,
                count != 0,
                |key, modifiers, bounds| {
                    if modifiers.is_empty() {
                        Scroll::from_key(key, bounds.height)
                    } else {
                        None
                    }
                },
            ),
        ]
        .height(Fill);
        let table = super::scrolling::horizontal_scrollable(super::scrolling::table_surface(
            container(table).width(size.width.max(1024.0)).height(Fill),
        ))
        .id(ACTION_LOG_HORIZONTAL_SCROLL_ID)
        .width(Fill)
        .height(Fill);
        table.into()
    }
}

struct ActionLogGroup<'a> {
    first: &'a ActionLogEntry,
    rest: Vec<&'a ActionLogEntry>,
}

impl<'a> ActionLogGroup<'a> {
    fn iter(&self) -> impl Iterator<Item = &'a ActionLogEntry> + '_ {
        std::iter::once(self.first).chain(self.rest.iter().copied())
    }

    fn len(&self) -> usize {
        1 + self.rest.len()
    }
}

// Group matching actions throughout each automation pass, newest groups first.
fn group_entries<'a>(
    entries: impl IntoIterator<Item = &'a ActionLogEntry>,
) -> Vec<ActionLogGroup<'a>> {
    let mut groups: Vec<ActionLogGroup<'_>> = Vec::new();
    for entry in entries {
        if let Some(group) = groups
            .iter_mut()
            .rev()
            .take_while(|group| group.first.batch_id == entry.batch_id)
            .find(|group| {
                let first = group.first;
                entry.process_id.is_some()
                    && first.process_id.is_some()
                    && entry.feature == first.feature
                    && entry.result == first.result
                    && entry.reason == first.reason
                    && !group.iter().any(|old| old.process_id == entry.process_id)
            })
        {
            group.rest.push(entry);
            continue;
        }
        groups.push(ActionLogGroup {
            first: entry,
            rest: Vec::new(),
        });
    }
    groups
}

fn process_icon<'a>(
    name: &str,
    candidates: &[super::app_picker::Candidate],
) -> Element<'a, Message> {
    super::app_picker::icon_for_name(name, candidates)
        .map(|icon| {
            iced::widget::image(icon.clone())
                .width(16)
                .height(16)
                .into()
        })
        .unwrap_or_else(|| super::navigation::glyph("icons/app-window.svg"))
}

fn process_group<'a>(
    entries: ActionLogGroup<'a>,
    candidates: &'a [super::app_picker::Candidate],
) -> Element<'a, Message> {
    let first = entries.first;
    let count = entries.len();
    if count == 1 && first.process_id.is_none() && first.process_name.is_empty() {
        return container(text("\u{2014}").style(text::secondary))
            .width(160)
            .into();
    }
    let mut strip = row![].spacing(4).align_y(iced::Center);
    for entry in entries.iter().take(5) {
        strip = strip.push(process_icon(&entry.process_name, candidates));
    }
    let mut names_hidden = count > 1;
    if count == 1 {
        let label = action_log_process_label(first);
        let fitted = super::widgets::fitted_text_with_font(
            &label,
            134.0,
            design::typography::CAPTION,
            design::typography::DEFAULT_FONT,
        );
        names_hidden = fitted != label;
        strip = strip.push(
            text(fitted)
                .size(design::typography::CAPTION)
                .font(design::typography::DEFAULT_FONT)
                .wrapping(text::Wrapping::None),
        );
    } else if count > 5 {
        strip = strip.push(text(format!("+{}", count - 5)).size(design::typography::CAPTION));
    }
    let anchor = container(strip)
        .width(160)
        .center_y(ACTION_LOG_ROW_HEIGHT - 1.0)
        .clip(true)
        .into();
    if !names_hidden {
        return anchor;
    }
    popover::view(first.sequence, anchor, move || {
        let mut details = column![text(
            t!("action_log.affected_processes", count = count).to_string()
        )]
        .spacing(8);
        for entry in entries.iter() {
            details = details.push(
                row![
                    process_icon(&entry.process_name, candidates),
                    text(action_log_process_label(entry))
                ]
                .spacing(8)
                .align_y(iced::Center),
            );
        }
        container(
            super::scrolling::popup_scrollable(details)
                .width(iced::Length::Shrink)
                .height(iced::Length::Shrink),
        )
        .padding(12)
        .width(iced::Length::Shrink)
        .max_height(300)
        .style(|theme| {
            let menu = super::widgets::select_menu(theme);
            iced::widget::container::Style {
                background: Some(menu.background),
                border: menu.border,
                ..Default::default()
            }
        })
        .into()
    })
}

pub(super) fn action_log_feature_label(feature: ActionLogFeature) -> String {
    let page = match feature {
        ActionLogFeature::InstanceBalancing => super::Page::InstanceBalancing,
        ActionLogFeature::AppSuspension => super::Page::AppSuspension,
        ActionLogFeature::SleepPrevention => super::Page::SleepPrevention,
        ActionLogFeature::KeepRunning => super::Page::KeepRunning,
        ActionLogFeature::InstanceLimits => super::Page::InstanceLimits,
        ActionLogFeature::CpuSetsSoft => super::Page::CpuSetsSoft,
        ActionLogFeature::ProcessorAffinityHard => super::Page::ProcessorAffinityHard,
        ActionLogFeature::EfficiencyMode => super::Page::EfficiencyMode,
        ActionLogFeature::CpuLimiter => super::Page::CpuLimiter,
        ActionLogFeature::ByFocus => super::Page::ByFocus,
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
        ActionLogFeature::ResourceThresholds => super::Page::ResourceThresholds,
        ActionLogFeature::TimerResolution => super::Page::TimerResolution,
    };
    page.label()
}

pub(super) fn action_log_result_label(result: ActionLogResult) -> String {
    t!(match result {
        ActionLogResult::Applied => "action_log.applied",
        ActionLogResult::Restored => "action_log.restored",
        ActionLogResult::Skipped => "action_log.skipped",
        ActionLogResult::Failed => "action_log.failed",
    })
    .to_string()
}

pub(super) fn action_log_level_label(level: ActionLogLevel) -> String {
    t!(match level {
        ActionLogLevel::Information => "action_log.information",
        ActionLogLevel::Warning => "action_log.warning",
        ActionLogLevel::Error => "action_log.error",
    })
    .to_string()
}

pub(super) fn action_log_level_style(
    theme: &iced::Theme,
    level: ActionLogLevel,
) -> iced::widget::text::Style {
    match level {
        ActionLogLevel::Information => iced::widget::text::Style::default(),
        ActionLogLevel::Warning => text::warning(theme),
        ActionLogLevel::Error => text::danger(theme),
    }
}

pub(super) fn action_log_result_text(result: ActionLogResult) -> &'static str {
    match result {
        ActionLogResult::Applied => "Applied",
        ActionLogResult::Restored => "Restored",
        ActionLogResult::Skipped => "Skipped",
        ActionLogResult::Failed => "Failed",
    }
}

fn action_log_filtered_entries<'a>(
    entries: &'a [ActionLogEntry],
    level_filter: &'a [ActionLogLevel],
    feature_filter: &'a [ActionLogFeature],
    search: &str,
) -> impl Iterator<Item = &'a ActionLogEntry> {
    let search = search.trim().to_lowercase();
    let pid = search.parse::<NonZeroU32>().ok().map(NonZeroU32::get);
    entries.iter().rev().filter(move |entry| {
        level_filter.contains(&entry.result.level())
            && feature_filter.contains(&entry.feature)
            && (search.is_empty()
                || pid.is_some_and(|pid| entry.process_id == Some(pid))
                || entry.process_name.to_lowercase().contains(search.as_str()))
    })
}

#[cfg(test)]
const RESULTS: [ActionLogResult; 4] = [
    ActionLogResult::Applied,
    ActionLogResult::Restored,
    ActionLogResult::Skipped,
    ActionLogResult::Failed,
];
const FEATURES: [ActionLogFeature; 24] = [
    ActionLogFeature::AppSuspension,
    ActionLogFeature::SleepPrevention,
    ActionLogFeature::KeepRunning,
    ActionLogFeature::InstanceLimits,
    ActionLogFeature::CpuSetsSoft,
    ActionLogFeature::InstanceBalancing,
    ActionLogFeature::ProcessorAffinityHard,
    ActionLogFeature::EfficiencyMode,
    ActionLogFeature::CpuLimiter,
    ActionLogFeature::ByFocus,
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
    ActionLogFeature::ResourceThresholds,
    ActionLogFeature::TimerResolution,
];

const ACTION_LOG_SCROLL_ID: &str = "action-log";
const ACTION_LOG_HORIZONTAL_SCROLL_ID: &str = "action-log-columns";
const ACTION_LOG_TABLE_ID: &str = "action-log-table";
const ACTION_LOG_HORIZONTAL_STEP: f32 = 160.0;
const ACTION_LOG_ROW_HEIGHT: f32 = 36.0;
const ACTION_LOG_HEADER_HEIGHT: f32 = 33.0;

fn visible_log_range(count: usize, offset: f32, height: f32) -> std::ops::Range<usize> {
    let height = height.max(0.0);
    let max_offset = (count as f32 * ACTION_LOG_ROW_HEIGHT - height).max(0.0);
    let first =
        ((offset.clamp(0.0, max_offset) / ACTION_LOG_ROW_HEIGHT).floor() as usize).min(count);
    let start = first.saturating_sub(8);
    let end = (first + (height / ACTION_LOG_ROW_HEIGHT).ceil() as usize + 8).min(count);
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
        "level",
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
            entry.result.level().name(),
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
    fn export_retains_one_snapshot_until_the_picker_closes() {
        let mut log = crate::action_log::ActionLog::new(1);
        let mut editor = Editor::default();
        assert!(!editor.begin_export(&Arc::new(log.entries())));
        for reason in ["At click", "During picker"] {
            log.record(
                ActionLogFeature::CpuLimiter,
                None,
                "app.exe",
                ActionLogResult::Applied,
                reason,
            );
            assert_eq!(
                editor.begin_export(&Arc::new(log.entries())),
                reason == "At click"
            );
        }
        assert_eq!(editor.finish_export().unwrap()[0].reason, "At click");
        assert!(editor.begin_export(&Arc::new(log.entries())));
        assert_eq!(editor.finish_export().unwrap()[0].reason, "During picker");
        assert!(editor.finish_export().is_none());
    }

    #[test]
    fn level_filters_and_csv_preserve_distinct_outcomes() {
        let mut log = crate::action_log::ActionLog::new(4);
        log.begin_batch();
        for (pid, result) in RESULTS.into_iter().enumerate() {
            log.record(
                ActionLogFeature::CpuLimiter,
                Some(pid as u32),
                "app.exe",
                result,
                "Same reason.",
            );
        }
        let entries = log.entries();
        for (level, expected) in [
            (
                ActionLogLevel::Information,
                vec![ActionLogResult::Restored, ActionLogResult::Applied],
            ),
            (ActionLogLevel::Warning, vec![ActionLogResult::Skipped]),
            (ActionLogLevel::Error, vec![ActionLogResult::Failed]),
        ] {
            let levels = [level];
            let filtered: Vec<_> =
                action_log_filtered_entries(&entries, &levels, &FEATURES, "").collect();
            assert_eq!(
                filtered
                    .iter()
                    .map(|entry| entry.result)
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(group_entries(filtered).len(), expected.len());
        }
        let csv = action_log_entries_to_csv(&entries);
        let mut reader = csv::Reader::from_reader(csv.as_bytes());
        assert_eq!(&reader.headers().unwrap()[5], "level");
        for (record, (level, result)) in reader.records().zip([
            ("INFO", "Applied"),
            ("INFO", "Restored"),
            ("WARN", "Skipped"),
            ("ERROR", "Failed"),
        ]) {
            let record = record.unwrap();
            assert_eq!(&record[5], level);
            assert_eq!(&record[7], result);
        }
    }

    #[test]
    fn process_popup_opens_when_names_are_hidden() {
        use iced::advanced::{layout, widget::Tree, Layout, Shell};
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        for (name, count, expected) in [
            ("a.exe", 1, false),
            ("a.exe", 2, true),
            ("a.exe", 5, true),
            ("a.exe", 6, true),
            ("a.exe", 40, true),
            ("a-very-long-process-name.exe", 1, true),
        ] {
            let mut log = crate::action_log::ActionLog::new(40);
            log.begin_batch();
            for pid in 0..count {
                log.record(
                    ActionLogFeature::AdaptiveEngine,
                    Some(pid),
                    name,
                    ActionLogResult::Applied,
                    "Applied",
                );
            }
            let entries = log.entries();
            let group = group_entries(&entries).pop().unwrap();
            let mut element = process_group(group, &[]);
            let mut tree = Tree::new(&element);
            let size = iced::Size::new(500.0, 500.0);
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, size),
            );
            assert_eq!(node.size().height, ACTION_LOG_ROW_HEIGHT - 1.0);
            for y in [1.0, node.size().height / 2.0, node.size().height - 1.0] {
                let position = iced::Point::new(node.bounds().center().x, y);
                element.as_widget_mut().update(
                    &mut tree,
                    &iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }),
                    Layout::new(&node),
                    iced::mouse::Cursor::Available(position),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut Shell::new(&mut Vec::new()),
                    &iced::Rectangle::with_size(size),
                );
                assert_eq!(
                    element
                        .as_widget_mut()
                        .overlay(
                            &mut tree,
                            Layout::new(&node),
                            &renderer,
                            &iced::Rectangle::with_size(size),
                            iced::Vector::ZERO
                        )
                        .is_some(),
                    expected
                );
            }
        }
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
        let filtered = action_log_filtered_entries(&entries, &ActionLogLevel::ALL, &FEATURES, "");
        assert_eq!(
            group_entries(filtered)
                .iter()
                .map(ActionLogGroup::len)
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
        let filtered = action_log_filtered_entries(&entries, &ActionLogLevel::ALL, &FEATURES, "");
        assert_eq!(
            group_entries(filtered)
                .iter()
                .map(ActionLogGroup::len)
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
            assert_eq!(group_entries([&entries[0], &other]).len(), 2);
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
                let filtered =
                    action_log_filtered_entries(&entries, &ActionLogLevel::ALL, &FEATURES, "");
                let groups = group_entries(filtered);
                assert_eq!(groups.len(), 2, "{feature:?} {result:?}");
                assert_eq!(groups[0].len(), 3);
                assert_eq!(groups[1].len(), 3);
                assert_eq!(groups[0].first.reason, "Different action.");
                assert_eq!(action_log_entries_to_csv(&entries).lines().count(), 7);
            }
        }
    }

    #[test]
    fn log_scroll_damage_stays_bounded() {
        let entries: Vec<_> = (0..10000).map(|sequence| ActionLogEntry {
            batch_id: sequence,
            sequence, timestamp_epoch_ms: 1_700_000_000_000,
            feature: ActionLogFeature::CpuLimiter, process_id: Some(1234),
            process_name: "long-running-application-name.exe".into(),
            result: ActionLogResult::Applied,
            reason: "Applied configured process policy because the foreground and background conditions changed; additional details for the log entry.".into(),
        }).collect();
        let editor = Editor::default();
        let bounds = iced::Rectangle::with_size(iced::Size::new(1920.0, 1080.0));
        crate::ui::scrolling::check_scroll_damage(
            editor.view_at_size(&entries, &[], bounds.size()),
            bounds,
        );
    }

    #[test]
    fn virtual_log_rows_are_bounded_and_reach_the_end() {
        assert_eq!(visible_log_range(0, 0.0, 900.0), 0..0);
        let range = visible_log_range(10000, 3600.0, 900.0);
        assert_eq!(range, 92..133);
        assert_eq!(visible_log_range(52, 1500.0, 900.0).end, 52);
        assert_eq!(visible_log_range(11, 1500.0, 450.0), 0..11);
        let mut editor = Editor::default();
        editor.update(Message::Scrolled(3600.0));
        editor.update(Message::Level(ActionLogLevel::Error, false));
        assert_eq!(editor.offset.get(), 0.0);
    }

    #[test]
    fn process_search_filters_batch_members_and_keyboard_scroll() {
        use ActionLogFeature::{CpuLimiter, SleepPrevention};
        use ActionLogResult::{Applied, Failed};

        let mut log = crate::action_log::ActionLog::new(6);
        log.begin_batch();
        for (feature, result, pid, name) in [
            (CpuLimiter, Failed, 1000, "ÄPP.exe"),
            (CpuLimiter, Failed, 420, "Other.exe"),
            (CpuLimiter, Failed, 42, "äpp-helper.exe"),
            (CpuLimiter, Applied, 41, "Äpp.exe"),
            (SleepPrevention, Failed, 40, "Äpp.exe"),
        ] {
            log.record(feature, Some(pid), name, result, "Same reason.");
        }
        log.begin_batch();
        log.record(CpuLimiter, Some(43), "Äpp.exe", Failed, "Same reason.");
        let entries = Arc::new(log.entries());
        let mut editor = Editor {
            level: vec![ActionLogLevel::Error],
            feature: vec![CpuLimiter],
            ..Editor::default()
        };

        editor.update(Message::Search(" ÄPP ".into()));
        let groups = group_entries(action_log_filtered_entries(
            &entries,
            &editor.level,
            &editor.feature,
            &editor.search,
        ));
        let grouped_pids: Vec<Vec<_>> = groups
            .iter()
            .map(|group| group.iter().map(|entry| entry.process_id).collect())
            .collect();
        assert_eq!(
            grouped_pids,
            vec![vec![Some(43)], vec![Some(42), Some(1000)]]
        );
        let _ = editor.keyboard_scroll::<Message>(&entries, Scroll::End, ACTION_LOG_ROW_HEIGHT);
        assert_eq!(editor.offset.get(), ACTION_LOG_ROW_HEIGHT);

        editor.update(Message::Search("42".into()));
        assert_eq!(editor.offset.get(), 0.0);
        let matches: Vec<_> =
            action_log_filtered_entries(&entries, &editor.level, &editor.feature, &editor.search)
                .map(|entry| entry.process_id)
                .collect();
        assert_eq!(matches, vec![Some(42)]);
        let _ = editor.keyboard_scroll::<Message>(&entries, Scroll::End, ACTION_LOG_ROW_HEIGHT);
        assert_eq!(editor.offset.get(), 0.0);

        assert!(editor.begin_export(&entries));
        assert!(Arc::ptr_eq(&entries, &editor.finish_export().unwrap()));

        editor.update(Message::Search(String::new()));
        assert_eq!(
            action_log_filtered_entries(&entries, &editor.level, &editor.feature, &editor.search)
                .count(),
            4
        );
    }

    #[test]
    fn focused_compact_log_scrolls_hidden_columns_without_moving_rows() {
        use iced::advanced::{layout, widget::operation, widget::Id, widget::Tree, Layout, Shell};
        use iced::{keyboard, mouse, Event, Pixels, Rectangle, Renderer, Size, Vector};

        #[derive(Default)]
        struct Position {
            x: f32,
            y: f32,
            focused: bool,
        }

        impl operation::Operation for Position {
            fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn operation::Operation)) {
                visit(self);
            }

            fn focusable(
                &mut self,
                id: Option<&Id>,
                _: Rectangle,
                state: &mut dyn operation::Focusable,
            ) {
                if id == Some(&ACTION_LOG_TABLE_ID.into()) {
                    self.focused = state.is_focused();
                }
            }

            fn scrollable(
                &mut self,
                id: Option<&Id>,
                _: Rectangle,
                _: Rectangle,
                translation: Vector,
                _: &mut dyn operation::Scrollable,
            ) {
                match id {
                    Some(id) if *id == ACTION_LOG_HORIZONTAL_SCROLL_ID.into() => {
                        self.x = translation.x
                    }
                    Some(id) if *id == ACTION_LOG_SCROLL_ID.into() => self.y = translation.y,
                    _ => {}
                }
            }
        }

        let mut log = crate::action_log::ActionLog::new(20);
        for _ in 0..20 {
            log.begin_batch();
            log.record(
                ActionLogFeature::MemoryTrim,
                Some(1234),
                "app.exe",
                ActionLogResult::Applied,
                "Trim completed.",
            );
        }
        let entries = log.entries();
        let editor = Editor::default();
        let size = Size::new(465.0, 380.0);
        let renderer = Renderer::new(super::design::typography::FONT, Pixels(14.0));
        let mut table = editor.view_at_size(&entries, &[], size);
        let mut tree = Tree::new(&table);
        let node = table.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        let layout = Layout::new(&node);
        let viewport = Rectangle::with_size(size);
        let operate = |table: &mut Element<'_, Message>,
                       tree: &mut Tree,
                       operation: &mut dyn operation::Operation| {
            table
                .as_widget_mut()
                .operate(tree, layout, &renderer, operation);
        };
        let press = |table: &mut Element<'_, Message>, tree: &mut Tree, key| {
            let key = keyboard::Key::Named(key);
            let event = Event::Keyboard(keyboard::Event::KeyPressed {
                modified_key: key.clone(),
                key,
                physical_key: keyboard::key::Physical::Unidentified(
                    keyboard::key::NativeCode::Unidentified,
                ),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::empty(),
                text: None,
                repeat: false,
            });
            let mut messages = Vec::new();
            table.as_widget_mut().update(
                tree,
                &event,
                layout,
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &viewport,
            );
            messages
        };

        assert!(press(&mut table, &mut tree, Named::ArrowRight).is_empty());
        operate(
            &mut table,
            &mut tree,
            &mut operation::focusable::focus(ACTION_LOG_TABLE_ID.into()),
        );
        operate(
            &mut table,
            &mut tree,
            &mut operation::scrollable::scroll_to(
                ACTION_LOG_SCROLL_ID.into(),
                operation::scrollable::AbsoluteOffset {
                    x: None,
                    y: Some(72.0),
                },
            ),
        );

        for (key, expected_x) in [(Named::ArrowRight, 160.0), (Named::ArrowLeft, 0.0)] {
            let messages = press(&mut table, &mut tree, key);
            let [Message::KeyboardScroll(Scroll::Horizontal(delta), _)] = messages.as_slice()
            else {
                panic!("expected a horizontal scroll, got {messages:?}");
            };
            operate(&mut table, &mut tree, &mut scroll_horizontal(*delta));
            let mut position = Position::default();
            operate(&mut table, &mut tree, &mut position);
            assert_eq!(position.x, expected_x);
            assert_eq!(position.y, 72.0);
            assert!(position.focused);
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
                &[ActionLogLevel::Error],
                &[ActionLogFeature::CpuLimiter],
                "",
            )
            .next()
            .map(|entry| entry.sequence),
            Some(2)
        );
        assert_eq!(
            action_log_filtered_entries(&entries, &ActionLogLevel::ALL, &FEATURES, "")
                .next()
                .map(|entry| entry.sequence),
            Some(2)
        );
        let mut editor = Editor::default();
        assert_eq!(
            action_log_filtered_entries(&entries, &editor.level, &editor.feature, "").count(),
            2
        );
        editor.update(Message::AllLevels(false));
        assert!(
            action_log_filtered_entries(&entries, &editor.level, &editor.feature, "")
                .next()
                .is_none()
        );
        editor.update(Message::Level(ActionLogLevel::Information, true));
        editor.update(Message::Level(ActionLogLevel::Error, true));
        assert_eq!(
            action_log_filtered_entries(&entries, &editor.level, &editor.feature, "").count(),
            2
        );
        editor.update(Message::AllFeatures(false));
        assert!(
            action_log_filtered_entries(&entries, &editor.level, &editor.feature, "")
                .next()
                .is_none()
        );
        editor.update(Message::Feature(ActionLogFeature::CpuLimiter, true));
        assert_eq!(
            action_log_filtered_entries(&entries, &editor.level, &editor.feature, "").count(),
            2
        );
        editor.update(Message::AllLevels(true));
        editor.update(Message::AllFeatures(true));
        assert_eq!(editor.level.len(), ActionLogLevel::ALL.len());
        assert_eq!(editor.feature.len(), FEATURES.len());
        let csv = action_log_entries_to_csv(&entries);
        let records = csv::Reader::from_reader(csv.as_bytes())
            .records()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(&records[0][8], entries[0].reason);
    }
}
