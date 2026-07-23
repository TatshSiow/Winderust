use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_action_log_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let visible_entries = action_log_filtered_entries(
            self.action_log_entries.as_slice(),
            self.action_log_result_filter,
            self.action_log_feature_filter,
        );
        let visible_count = visible_entries.len();
        let page_count = action_log_page_count(visible_count);
        let current_page = self.action_log_page.min(page_count.saturating_sub(1));
        let page_start = current_page * ACTION_LOG_PAGE_SIZE;
        let page_end = (page_start + ACTION_LOG_PAGE_SIZE).min(visible_count);
        let page_entries = if page_start < page_end {
            &visible_entries[page_start..page_end]
        } else {
            &[]
        };

        let mut list = action_log_list_surface();
        if self.action_log_entries.is_empty() {
            list = list.child(action_log_empty_row(t!("action_log.empty").to_string()));
        } else if visible_count == 0 {
            list = list.child(action_log_empty_row(
                t!("action_log.no_filter_matches").to_string(),
            ));
        } else {
            list = list.child(action_log_header_row());
            for (index, entry) in page_entries.iter().enumerate() {
                list = list.child(action_log_entry_row(entry, index > 0));
            }
        }

        let action_controls = h_flex()
            .gap_2()
            .items_center()
            .flex_wrap()
            .child(
                control_button(Button::new("clear-action-log"))
                    .label(t!("action_log.clear").to_string())
                    .disabled(self.action_log_entries.is_empty())
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.background_automation.clear_action_log();
                        app.action_log_entries = Arc::new(Vec::new());
                        app.action_log_page = 0;
                        cx.notify();
                    })),
            )
            .child(
                control_button(Button::new("export-action-log"))
                    .label(t!("action_log.export_csv").to_string())
                    .disabled(self.action_log_entries.is_empty())
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.export_action_log_csv();
                        cx.notify();
                    })),
            );
        let page_controls = action_log_page_controls(visible_count, current_page, page_count, cx);

        self.page_shell(Page::ActionLog, cx)
            .child(self.render_action_log_feature_filter(window, cx))
            .child(self.render_action_log_result_filter(window, cx))
            .child(action_log_command_row(
                action_controls.into_any_element(),
                page_controls.into_any_element(),
            ))
            .child(
                v_flex()
                    .w_full()
                    .min_w(px(0.0))
                    .gap_2()
                    .child(action_log_table_summary(
                        visible_count,
                        current_page,
                        page_count,
                        page_start,
                        page_end,
                    ))
                    .child(list),
            )
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_action_log_result_filter(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.action_log_result_filter;
        let dropdown = self.render_dropdown_select(
            "action-log-result-filter",
            action_log_filter_label(selected),
            true,
            DropdownSelectWidth::Standard,
            ActionLogResultFilter::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for filter in ActionLogResultFilter::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "action-log-result-filter-option-{filter:?}"
                            )),
                            action_log_filter_label(filter),
                            selected == filter,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.action_log_result_filter = filter;
                            app.action_log_page = 0;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        setting_action_card(
            "action-log-result-filter-card",
            t!("action_log.result_filter").to_string(),
            dropdown,
        )
        .into_any_element()
    }

    pub(in crate::ui::app) fn render_action_log_feature_filter(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.action_log_feature_filter;
        let dropdown = self.render_dropdown_select(
            "action-log-feature-filter",
            action_log_feature_filter_label(selected),
            true,
            DropdownSelectWidth::Standard,
            ActionLogFeatureFilter::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for filter in ActionLogFeatureFilter::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "action-log-feature-filter-option-{filter:?}"
                            )),
                            action_log_feature_filter_label(filter),
                            selected == filter,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.action_log_feature_filter = filter;
                            app.action_log_page = 0;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        setting_action_card(
            "action-log-feature-filter-card",
            t!("action_log.feature_filter").to_string(),
            dropdown,
        )
        .into_any_element()
    }
}

pub(in crate::ui::app) const ACTION_LOG_SEQUENCE_WIDTH: f32 = 56.0;
pub(in crate::ui::app) const ACTION_LOG_TIME_WIDTH: f32 = 96.0;
pub(in crate::ui::app) const ACTION_LOG_FEATURE_WIDTH: f32 = 156.0;
pub(in crate::ui::app) const ACTION_LOG_RESULT_WIDTH: f32 = 88.0;
pub(in crate::ui::app) const ACTION_LOG_PROCESS_WIDTH: f32 = 176.0;
pub(in crate::ui::app) const ACTION_LOG_PAGINATION_WIDTH: f32 = 320.0;

pub(in crate::ui::app) fn action_log_command_row(
    actions: AnyElement,
    page_controls: AnyElement,
) -> gpui::Div {
    h_flex()
        .w_full()
        .min_w(px(0.0))
        .min_h(px(32.0))
        .items_center()
        .justify_between()
        .gap_2()
        .flex_wrap()
        .child(div().flex_1().min_w(px(0.0)).child(actions))
        .child(div().flex_shrink_0().child(page_controls))
}

pub(in crate::ui::app) fn action_log_page_controls(
    total_entries: usize,
    current_page: usize,
    page_count: usize,
    cx: &mut Context<WinderustApp>,
) -> gpui::Div {
    let has_entries = total_entries > 0;
    h_flex()
        .flex_shrink_0()
        .items_center()
        .gap_2()
        .child(
            control_button(Button::new("action-log-prev-page"))
                .label(t!("action_log.previous").to_string())
                .disabled(!has_entries || current_page == 0)
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.action_log_page = current_page.saturating_sub(1);
                    cx.notify();
                })),
        )
        .child(
            control_button(Button::new("action-log-next-page"))
                .label(t!("action_log.next").to_string())
                .disabled(!has_entries || current_page + 1 >= page_count)
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.action_log_page = current_page.saturating_add(1);
                    cx.notify();
                })),
        )
}

pub(in crate::ui::app) fn action_log_list_surface() -> gpui::Div {
    v_flex()
        .w_full()
        .min_w(px(0.0))
        .relative()
        .overflow_hidden()
        .rounded(px(BRAND_RADIUS_SURFACE))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
}

pub(in crate::ui::app) fn action_log_empty_row(message: impl Into<SharedString>) -> gpui::Div {
    h_flex()
        .w_full()
        .min_w(px(0.0))
        .h(px(CARD_ROW_HEIGHT))
        .items_center()
        .px_4()
        .py_3()
        .child(text_muted(message.into()))
}

pub(in crate::ui::app) fn action_log_table_summary(
    total_entries: usize,
    current_page: usize,
    page_count: usize,
    page_start: usize,
    page_end: usize,
) -> gpui::Div {
    v_flex()
        .w_full()
        .min_w(px(0.0))
        .min_h(px(38.0))
        .gap(px(2.0))
        .child(
            div()
                .min_h(px(TEXT_BODY_LINE_HEIGHT))
                .child(section_title_text(
                    t!("action_log.recent_entries").to_string(),
                )),
        )
        .child(
            div()
                .w(px(ACTION_LOG_PAGINATION_WIDTH))
                .min_h(px(TEXT_BODY_LINE_HEIGHT))
                .truncate()
                .child(text_muted(action_log_pagination_label(
                    total_entries,
                    current_page,
                    page_count,
                    page_start,
                    page_end,
                ))),
        )
}

pub(in crate::ui::app) fn action_log_header_row() -> gpui::Div {
    h_flex()
        .w_full()
        .min_w(px(0.0))
        .h(px(32.0))
        .items_center()
        .gap_3()
        .px_4()
        .py_2()
        .border_b_1()
        .border_color(rgb(border_color()))
        .bg(rgb(panel_active_color()))
        .text_size(px(TEXT_LABEL_SIZE))
        .line_height(px(TEXT_LABEL_LINE_HEIGHT))
        .text_color(rgb(muted_text_color()))
        .child(
            div()
                .w(px(ACTION_LOG_SEQUENCE_WIDTH))
                .flex_shrink_0()
                .child(t!("action_log.sequence").to_string()),
        )
        .child(
            div()
                .w(px(ACTION_LOG_TIME_WIDTH))
                .flex_shrink_0()
                .child(t!("action_log.time").to_string()),
        )
        .child(
            div()
                .w(px(ACTION_LOG_FEATURE_WIDTH))
                .flex_shrink_0()
                .child(t!("action_log.feature").to_string()),
        )
        .child(
            div()
                .w(px(ACTION_LOG_RESULT_WIDTH))
                .flex_shrink_0()
                .child(t!("action_log.result").to_string()),
        )
        .child(
            div()
                .w(px(ACTION_LOG_PROCESS_WIDTH))
                .flex_shrink_0()
                .child(t!("action_log.process").to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(120.0))
                .child(t!("action_log.reason").to_string()),
        )
}

pub(in crate::ui::app) fn action_log_entry_row(
    entry: &ActionLogEntry,
    divided: bool,
) -> gpui::Stateful<gpui::Div> {
    let row_id = SharedString::from(format!("action-log-entry-{}", entry.sequence));
    let content = h_flex()
        .w_full()
        .min_w(px(0.0))
        .h(px(40.0))
        .items_center()
        .gap_3()
        .px_4()
        .py_2()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .child(
            div()
                .w(px(ACTION_LOG_SEQUENCE_WIDTH))
                .flex_shrink_0()
                .truncate()
                .text_color(rgb(dim_text_color()))
                .child(format!("#{}", entry.sequence)),
        )
        .child(
            div()
                .w(px(ACTION_LOG_TIME_WIDTH))
                .flex_shrink_0()
                .truncate()
                .text_color(rgb(muted_text_color()))
                .child(action_log_time_label(entry.timestamp_epoch_ms)),
        )
        .child(
            div()
                .w(px(ACTION_LOG_FEATURE_WIDTH))
                .flex_shrink_0()
                .truncate()
                .child(action_log_feature_label(entry.feature)),
        )
        .child(
            div()
                .w(px(ACTION_LOG_RESULT_WIDTH))
                .flex_shrink_0()
                .child(action_log_result_tag(entry.result)),
        )
        .child(
            div()
                .w(px(ACTION_LOG_PROCESS_WIDTH))
                .min_w(px(0.0))
                .flex_shrink_0()
                .truncate()
                .child(action_log_process_label(entry)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(120.0))
                .text_color(rgb(muted_text_color()))
                .truncate()
                .child(entry.reason.clone()),
        );

    h_flex()
        .id(row_id.clone())
        .w_full()
        .min_w(px(0.0))
        .h(px(40.0))
        .relative()
        .overflow_hidden()
        .when(divided, |row| {
            row.border_t_1().border_color(rgb(border_color()))
        })
        .child(
            div()
                .w_full()
                .min_w(px(0.0))
                .overflow_hidden()
                .child(content),
        )
}

pub(in crate::ui::app) fn action_log_result_tag(result: ActionLogResult) -> AnyElement {
    let label = action_log_result_label(result);
    match result {
        ActionLogResult::Applied | ActionLogResult::Restored => {
            status_pill(label, success_bg_color(), success_text_color())
        }
        ActionLogResult::Skipped => status_pill(label, warning_bg_color(), warning_text_color()),
        ActionLogResult::Failed => status_pill(
            label,
            if ui_is_dark() { 0x4a211b } else { 0xf5d2c7 },
            if ui_is_dark() { 0xff8a73 } else { 0x9b2f1f },
        ),
    }
}

pub(in crate::ui::app) fn action_log_feature_label(feature: ActionLogFeature) -> String {
    match feature {
        ActionLogFeature::AppSuspension => t!("nav.app_suspension").to_string(),
        ActionLogFeature::BackgroundCpuRestriction => {
            t!("nav.background_cpu_restriction").to_string()
        }
        ActionLogFeature::CoreSteering => t!("nav.core_steering").to_string(),
        ActionLogFeature::BackgroundEfficiency => t!("nav.background_efficiency").to_string(),
        ActionLogFeature::CoreLimiter => t!("nav.core_limiter").to_string(),
        ActionLogFeature::ByRunningApp => t!("nav.by_running_app").to_string(),
        ActionLogFeature::WorkloadEngine => t!("nav.workload_engine").to_string(),
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

pub(in crate::ui::app) fn action_log_result_label(result: ActionLogResult) -> SharedString {
    action_log_result_text(result).into()
}

pub(in crate::ui::app) fn action_log_result_text(result: ActionLogResult) -> &'static str {
    match result {
        ActionLogResult::Applied => "Applied",
        ActionLogResult::Restored => "Restored",
        ActionLogResult::Skipped => "Skipped",
        ActionLogResult::Failed => "Failed",
    }
}

pub(in crate::ui::app) fn action_log_filter_label(filter: ActionLogResultFilter) -> String {
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

pub(in crate::ui::app) fn action_log_feature_filter_label(
    filter: ActionLogFeatureFilter,
) -> String {
    match filter {
        ActionLogFeatureFilter::All => t!("action_log.filter_all").to_string(),
        ActionLogFeatureFilter::Feature(feature) => action_log_feature_label(feature),
    }
}

pub(in crate::ui::app) fn action_log_filtered_entries(
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

pub(in crate::ui::app) fn action_log_page_count(total_entries: usize) -> usize {
    total_entries.div_ceil(ACTION_LOG_PAGE_SIZE)
}

pub(in crate::ui::app) fn action_log_pagination_label(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_log_filtering_matches_result_and_feature() {
        let entries = vec![
            ActionLogEntry {
                sequence: 1,
                timestamp_epoch_ms: 1_700_000_000_000,
                feature: ActionLogFeature::CoreLimiter,
                process_id: Some(42),
                process_name: "worker.exe".to_owned(),
                action: ActionLogAction::Fail,
                result: ActionLogResult::Failed,
                reason: "restart failed".to_owned(),
            },
            ActionLogEntry {
                sequence: 2,
                timestamp_epoch_ms: 1_700_000_000_100,
                feature: ActionLogFeature::GpuPriority,
                process_id: Some(43),
                process_name: "game.exe".to_owned(),
                action: ActionLogAction::Apply,
                result: ActionLogResult::Applied,
                reason: "priority applied".to_owned(),
            },
        ];

        let filtered_entries = action_log_filtered_entries(
            &entries,
            ActionLogResultFilter::Failed,
            ActionLogFeatureFilter::Feature(ActionLogFeature::CoreLimiter),
        );

        assert_eq!(filtered_entries.len(), 1);
        assert_eq!(filtered_entries[0].sequence, 1);
    }

    #[test]
    fn action_log_page_count_rounds_up() {
        assert_eq!(action_log_page_count(0), 0);
        assert_eq!(action_log_page_count(1), 1);
        assert_eq!(action_log_page_count(ACTION_LOG_PAGE_SIZE), 1);
        assert_eq!(action_log_page_count(ACTION_LOG_PAGE_SIZE + 1), 2);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::app) enum ActionLogResultFilter {
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
pub(in crate::ui::app) enum ActionLogFeatureFilter {
    All,
    Feature(ActionLogFeature),
}

impl ActionLogFeatureFilter {
    const ALL: [Self; 16] = [
        Self::All,
        Self::Feature(ActionLogFeature::AppSuspension),
        Self::Feature(ActionLogFeature::BackgroundCpuRestriction),
        Self::Feature(ActionLogFeature::CoreSteering),
        Self::Feature(ActionLogFeature::BackgroundEfficiency),
        Self::Feature(ActionLogFeature::CoreLimiter),
        Self::Feature(ActionLogFeature::ByRunningApp),
        Self::Feature(ActionLogFeature::WorkloadEngine),
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
