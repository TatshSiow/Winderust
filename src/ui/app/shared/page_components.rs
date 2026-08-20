use crate::ui::app::*;

pub(in crate::ui::app) const PAGE_SIDE_PANEL_WIDTH: f32 = 360.0;
pub(in crate::ui::app) const PAGE_SIDE_PANEL_COMPACT_WIDTH: f32 = NAV_PANE_COMPACT_WIDTH;

pub(in crate::ui::app) fn page_side_panel_width_at_progress(
    presence_progress: f32,
    expansion_progress: f32,
) -> f32 {
    let expanded_width = PAGE_SIDE_PANEL_COMPACT_WIDTH
        + (PAGE_SIDE_PANEL_WIDTH - PAGE_SIDE_PANEL_COMPACT_WIDTH)
            * expansion_progress.clamp(0.0, 1.0);
    expanded_width * presence_progress.clamp(0.0, 1.0)
}

pub(in crate::ui::app) fn animated_page_side_panel(
    panel: AnyElement,
    presence_progress: f32,
    expansion_progress: f32,
    collapsed: bool,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let width = page_side_panel_width_at_progress(presence_progress, expansion_progress);
    let content_opacity = expansion_progress.clamp(0.0, 1.0);
    let toggle = side_panel_toggle_row(collapsed, expansion_progress, cx);
    v_flex()
        .w(px(width))
        .min_w(px(width))
        .h_full()
        .overflow_hidden()
        .border_l_1()
        .border_color(cx.theme().sidebar_border)
        .bg(cx.theme().sidebar)
        .child(
            div()
                .flex_1()
                .w(px(PAGE_SIDE_PANEL_WIDTH))
                .min_w(px(PAGE_SIDE_PANEL_WIDTH))
                .min_h(px(0.0))
                .opacity(content_opacity)
                .when(expansion_progress < 0.999, |content| {
                    content.block_mouse_except_scroll()
                })
                .child(panel),
        )
        .child(
            v_flex()
                .flex_shrink_0()
                .gap_1()
                .p_3()
                .border_t_1()
                .border_color(cx.theme().sidebar_border)
                .child(toggle),
        )
        .into_any_element()
}

fn side_panel_toggle_row(
    collapsed: bool,
    expansion_progress: f32,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let (icon, label) = if collapsed {
        (
            NavIcon::PanelRightOpen,
            t!("nav.expand_side_panel").to_string(),
        )
    } else {
        (
            NavIcon::PanelRightClose,
            t!("nav.collapse_side_panel").to_string(),
        )
    };

    nav_action_row(
        "toggle-page-side-panel",
        icon,
        label,
        collapsed,
        expansion_progress,
        PAGE_SIDE_PANEL_WIDTH,
        cx,
    )
    .on_click(cx.listener(|app, _, _, cx| {
        app.side_panel_collapsed = !app.side_panel_collapsed;
        begin_control_motion("page-side-panel-expanded", !app.side_panel_collapsed, cx);
        cx.notify();
    }))
    .into_any_element()
}

pub(in crate::ui::app) fn page_side_panel(header: AnyElement, body: AnyElement) -> AnyElement {
    v_flex()
        .w(px(PAGE_SIDE_PANEL_WIDTH))
        .min_w(px(PAGE_SIDE_PANEL_WIDTH))
        .h_full()
        .overflow_hidden()
        .child(header)
        .child(body)
        .into_any_element()
}

#[derive(Clone, Copy)]
enum FeatureRunState {
    Running,
    NotRunning,
    Unknown,
}

struct FeatureStatusSummary {
    state: FeatureRunState,
    scanned: Option<usize>,
    adjusted: Option<usize>,
    protected_or_denied: Option<usize>,
    skipped: Option<usize>,
    last_error: Option<String>,
    action_log_feature: ActionLogFeature,
}

fn feature_run_state(enabled: bool, unknown: bool) -> FeatureRunState {
    if !enabled {
        FeatureRunState::NotRunning
    } else if unknown {
        FeatureRunState::Unknown
    } else {
        FeatureRunState::Running
    }
}

fn status_state_row(state: FeatureRunState) -> gpui::Div {
    let (label, color) = match state {
        FeatureRunState::Running => (t!("common.running").to_string(), success_text_color()),
        FeatureRunState::NotRunning => (t!("common.not_running").to_string(), dim_text_color()),
        FeatureRunState::Unknown => (t!("common.unknown").to_string(), warning_text_color()),
    };
    h_flex()
        .min_h(px(34.0))
        .gap_2()
        .rounded(px(BRAND_RADIUS_SURFACE))
        .bg(rgb(settings_card_color()))
        .p_3()
        .child(
            div()
                .flex_1()
                .text_color(rgb(dim_text_color()))
                .text_size(px(TEXT_CONTROL_SIZE))
                .child(t!("common.status").to_string()),
        )
        .child(div().size(px(8.0)).rounded_full().bg(rgb(color)))
        .child(
            div()
                .text_color(rgb(color))
                .text_size(px(TEXT_BODY_SIZE))
                .child(label),
        )
}

fn status_count_label(count: Option<usize>) -> String {
    count
        .map(|count| count.to_string())
        .unwrap_or_else(|| "—".to_owned())
}

fn status_metric_row(label: String, value: String) -> gpui::Div {
    h_flex()
        .w_full()
        .min_w(px(0.0))
        .min_h(px(32.0))
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_color(rgb(dim_text_color()))
                .text_size(px(TEXT_CONTROL_SIZE))
                .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
                .child(label),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_color(rgb(primary_text_color()))
                .text_size(px(TEXT_BODY_SIZE))
                .line_height(px(TEXT_BODY_LINE_HEIGHT))
                .child(value),
        )
}

fn status_log_row(
    label: String,
    entry: Option<&ActionLogEntry>,
    fallback: Option<&str>,
) -> gpui::Div {
    let value = entry
        .map(|entry| {
            format!(
                "[{}] {} — {}",
                action_log_time_label(entry.timestamp_epoch_ms),
                action_log_process_label(entry),
                entry.reason
            )
        })
        .or_else(|| fallback.map(|message| format!("[--:--:--] {message}")))
        .unwrap_or_else(|| t!("common.none").to_string());
    v_flex()
        .w_full()
        .min_w(px(0.0))
        .gap_1()
        .pt_1()
        .child(
            div()
                .text_color(rgb(dim_text_color()))
                .text_size(px(TEXT_CONTROL_SIZE))
                .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
                .child(label),
        )
        .child(
            div()
                .w_full()
                .min_w(px(0.0))
                .py_1()
                .text_size(px(TEXT_CONTROL_SIZE))
                .line_height(px(TEXT_BODY_LINE_HEIGHT))
                .child(value),
        )
}

fn status_section(title: String, body: AnyElement) -> gpui::Div {
    v_flex()
        .min_w(px(0.0))
        .gap_2()
        .rounded(px(BRAND_RADIUS_SURFACE))
        .bg(rgb(settings_card_color()))
        .p_3()
        .child(section_title_text(title))
        .child(body)
}

impl WinderustApp {
    fn render_side_panel_for_page(&self, page: Page, cx: &mut Context<Self>) -> Option<AnyElement> {
        if page == Page::AdaptiveEngine {
            Some(self.render_adaptive_engine_side_panel(cx))
        } else if matches!(page, Page::CpuSetsSoft | Page::ProcessorAffinityHard) {
            Some(self.render_cpu_allocation_side_panel(page, cx))
        } else if page == Page::AdvancedPowerPlanTuning {
            Some(self.render_advanced_power_plan_tuning_side_panel(cx))
        } else {
            self.render_page_status_panel(page, cx)
        }
    }

    pub(in crate::ui::app) fn render_animated_side_panel(
        &mut self,
        search_active: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let page = self.shell.page;
        let requested_panel = if search_active {
            None
        } else {
            self.render_side_panel_for_page(page, cx)
        };
        let visible = requested_panel.is_some();

        if visible {
            self.retained_side_panel_page = Some(page);
        }
        if visible != self.side_panel_visible {
            self.side_panel_visible = visible;
            begin_control_motion("page-side-panel", visible, cx);
        }

        let progress = control_motion_progress("page-side-panel", visible);
        if !visible && progress <= 0.001 {
            self.retained_side_panel_page = None;
            return None;
        }

        let panel = match requested_panel {
            Some(panel) => panel,
            None => self.render_side_panel_for_page(self.retained_side_panel_page?, cx)?,
        };
        let expanded = !self.side_panel_collapsed;
        let expansion_progress = control_motion_progress("page-side-panel-expanded", expanded);
        Some(animated_page_side_panel(
            panel,
            progress,
            expansion_progress,
            self.side_panel_collapsed,
            cx,
        ))
    }

    fn power_plan_name(&self, guid: Option<&str>) -> String {
        let Some(guid) = guid else {
            return t!("common.none").to_string();
        };
        self.plans
            .iter()
            .find(|plan| plan.guid.eq_ignore_ascii_case(guid))
            .map(|plan| plan.name.clone())
            .unwrap_or_else(|| guid.to_owned())
    }

    fn render_power_plan_status(&self, page: Page) -> Option<AnyElement> {
        let (feature_enabled, action_log_feature) = match page {
            Page::ByForeground => (
                self.settings.by_foreground.enabled,
                ActionLogFeature::ByForeground,
            ),
            Page::ByRunningApp => (
                self.settings.by_running_app.enabled,
                ActionLogFeature::ByRunningApp,
            ),
            Page::ByCpuLoad => (
                self.settings.by_cpu_load.enabled,
                ActionLogFeature::ByCpuLoad,
            ),
            Page::ByActivity => (
                self.settings.by_activity.enabled,
                ActionLogFeature::ByActivity,
            ),
            Page::ByTime => (self.settings.by_time.enabled, ActionLogFeature::ByTime),
            _ => return None,
        };
        let action_summary = self.action_log_summaries.get(&action_log_feature);
        let successful_actions = v_flex()
            .w_full()
            .child(status_metric_row(
                t!("common.success_count").to_string(),
                action_summary
                    .map_or(0, |summary| summary.successful_actions)
                    .to_string(),
            ))
            .child(status_log_row(
                t!("common.last_success").to_string(),
                action_summary.and_then(|summary| summary.last_success.as_ref()),
                None,
            ));
        let failed_actions = v_flex()
            .w_full()
            .child(status_metric_row(
                t!("common.failed_count").to_string(),
                action_summary
                    .map_or(0, |summary| summary.failed_actions)
                    .to_string(),
            ))
            .child(status_log_row(
                t!("common.last_failed").to_string(),
                action_summary.and_then(|summary| summary.last_failed.as_ref()),
                None,
            ));
        Some(
            v_flex()
                .min_w(px(0.0))
                .gap_4()
                .child(status_state_row(feature_run_state(
                    self.settings.general.enabled && feature_enabled,
                    self.power_plan_status.current_guid.is_none(),
                )))
                .child(status_section(
                    t!("common.current_power_plan").to_string(),
                    div()
                        .w_full()
                        .min_w(px(0.0))
                        .text_size(px(TEXT_BODY_SIZE))
                        .line_height(px(TEXT_BODY_LINE_HEIGHT))
                        .child(self.power_plan_name(self.power_plan_status.current_guid.as_deref()))
                        .into_any_element(),
                ))
                .child(status_section(
                    t!("common.successful_actions").to_string(),
                    successful_actions.into_any_element(),
                ))
                .child(status_section(
                    t!("common.failed_actions").to_string(),
                    failed_actions.into_any_element(),
                ))
                .into_any_element(),
        )
    }

    fn feature_status_summary(&self, page: Page) -> Option<FeatureStatusSummary> {
        let summary = match page {
            Page::AdaptiveEngine => {
                let status = &self.feature_status.cpu_scheduler;
                FeatureStatusSummary {
                    state: feature_run_state(
                        self.settings.general.enabled && self.settings.adaptive_engine.enabled,
                        false,
                    ),
                    scanned: Some(status.scanned_processes),
                    adjusted: Some(status.adjusted_processes),
                    protected_or_denied: None,
                    skipped: Some(status.skipped_processes),
                    last_error: status.last_error.clone(),
                    action_log_feature: ActionLogFeature::CpuScheduler,
                }
            }
            Page::BackgroundEfficiency => {
                let status = &self.feature_status.background_efficiency;
                FeatureStatusSummary {
                    state: feature_run_state(status.enabled, status.unsupported),
                    scanned: Some(status.scanned_processes),
                    adjusted: Some(status.throttled_processes),
                    protected_or_denied: Some(status.access_denied_processes),
                    skipped: Some(status.skipped_processes),
                    last_error: status.last_error.clone(),
                    action_log_feature: ActionLogFeature::BackgroundEfficiency,
                }
            }
            Page::MemoryTrim => {
                let status = &self.feature_status.memory_trim;
                FeatureStatusSummary {
                    state: feature_run_state(status.enabled, false),
                    scanned: Some(status.scanned_processes),
                    adjusted: Some(status.trimmed_processes),
                    protected_or_denied: None,
                    skipped: Some(status.skipped_processes),
                    last_error: status.last_error.clone(),
                    action_log_feature: ActionLogFeature::MemoryTrim,
                }
            }
            Page::ProcessPriority => {
                let status = &self.feature_status.process_priority;
                FeatureStatusSummary {
                    state: feature_run_state(status.enabled, false),
                    scanned: Some(status.scanned_processes),
                    adjusted: Some(status.adjusted_processes),
                    protected_or_denied: None,
                    skipped: Some(status.skipped_processes),
                    last_error: status.last_error.clone(),
                    action_log_feature: ActionLogFeature::ProcessPriority,
                }
            }
            Page::ThreadPriority => {
                let status = &self.feature_status.thread_priority;
                FeatureStatusSummary {
                    state: feature_run_state(status.enabled, false),
                    scanned: Some(status.scanned_processes),
                    adjusted: Some(status.adjusted_processes),
                    protected_or_denied: None,
                    skipped: Some(status.skipped_processes),
                    last_error: status.last_error.clone(),
                    action_log_feature: ActionLogFeature::ThreadPriority,
                }
            }
            Page::DynamicPriorityBoost => {
                let status = &self.feature_status.dynamic_priority_boost;
                FeatureStatusSummary {
                    state: feature_run_state(status.enabled, false),
                    scanned: Some(status.scanned_processes),
                    adjusted: Some(status.adjusted_processes),
                    protected_or_denied: None,
                    skipped: Some(status.skipped_processes),
                    last_error: status.last_error.clone(),
                    action_log_feature: ActionLogFeature::DynamicPriorityBoost,
                }
            }
            Page::IoPriority => {
                let status = &self.feature_status.io_priority;
                FeatureStatusSummary {
                    state: feature_run_state(status.enabled, false),
                    scanned: Some(status.scanned_processes),
                    adjusted: Some(status.adjusted_processes),
                    protected_or_denied: None,
                    skipped: Some(status.skipped_processes),
                    last_error: status.last_error.clone(),
                    action_log_feature: ActionLogFeature::IoPriority,
                }
            }
            Page::GpuPriority => {
                let status = &self.feature_status.gpu_priority;
                FeatureStatusSummary {
                    state: feature_run_state(status.enabled, false),
                    scanned: Some(status.scanned_processes),
                    adjusted: Some(status.adjusted_processes),
                    protected_or_denied: Some(status.denied_processes),
                    skipped: Some(status.skipped_processes),
                    last_error: status.last_error.clone(),
                    action_log_feature: ActionLogFeature::GpuPriority,
                }
            }
            Page::MemoryPriority => {
                let status = &self.feature_status.memory_priority;
                FeatureStatusSummary {
                    state: feature_run_state(status.enabled, false),
                    scanned: None,
                    adjusted: Some(status.adjusted_processes),
                    protected_or_denied: None,
                    skipped: Some(status.skipped_processes),
                    last_error: status.last_error.clone(),
                    action_log_feature: ActionLogFeature::MemoryPriority,
                }
            }
            Page::CoreLimiter => {
                let status = &self.feature_status.core_limiter;
                FeatureStatusSummary {
                    state: feature_run_state(status.enabled, false),
                    scanned: Some(status.scanned_processes),
                    adjusted: Some(status.limited_processes),
                    protected_or_denied: None,
                    skipped: Some(status.skipped_processes),
                    last_error: status.last_error.clone(),
                    action_log_feature: ActionLogFeature::CoreLimiter,
                }
            }
            Page::CpuSetsSoft | Page::ProcessorAffinityHard => {
                let (status, feature) = if page == Page::CpuSetsSoft {
                    (
                        &self.feature_status.cpu_sets_soft,
                        ActionLogFeature::CpuSetsSoft,
                    )
                } else {
                    (
                        &self.feature_status.processor_affinity_hard,
                        ActionLogFeature::ProcessorAffinityHard,
                    )
                };
                FeatureStatusSummary {
                    state: feature_run_state(status.enabled, false),
                    scanned: Some(status.scanned_processes),
                    adjusted: Some(status.adjusted_processes),
                    protected_or_denied: None,
                    skipped: Some(status.skipped_processes),
                    last_error: status.last_error.clone(),
                    action_log_feature: feature,
                }
            }
            Page::AppSuspension => {
                let status = &self.feature_status.app_suspension;
                FeatureStatusSummary {
                    state: feature_run_state(
                        status.enabled,
                        status.unsupported || status.status_unknown,
                    ),
                    scanned: None,
                    adjusted: Some(status.suspended_processes),
                    protected_or_denied: None,
                    skipped: Some(status.skipped_processes),
                    last_error: status.last_error.clone(),
                    action_log_feature: ActionLogFeature::AppSuspension,
                }
            }
            Page::TimerResolution => {
                let status = &self.feature_status.timer_resolution;
                FeatureStatusSummary {
                    state: feature_run_state(status.enabled, false),
                    scanned: None,
                    adjusted: None,
                    protected_or_denied: None,
                    skipped: None,
                    last_error: status.last_error.clone(),
                    action_log_feature: ActionLogFeature::TimerResolution,
                }
            }
            _ => return None,
        };
        Some(summary)
    }

    pub(in crate::ui::app) fn render_normalized_feature_status(
        &self,
        page: Page,
    ) -> Option<AnyElement> {
        let status = self.feature_status_summary(page)?;
        let action_summary = self.action_log_summaries.get(&status.action_log_feature);
        let process_activity = v_flex()
            .w_full()
            .child(status_metric_row(
                t!("common.scanned_processes").to_string(),
                status_count_label(status.scanned),
            ))
            .child(status_metric_row(
                t!("common.adjusted_processes").to_string(),
                status_count_label(status.adjusted),
            ))
            .child(status_metric_row(
                t!("common.protected_or_denied_processes").to_string(),
                status_count_label(status.protected_or_denied),
            ))
            .child(status_metric_row(
                t!("common.skipped_processes").to_string(),
                status_count_label(status.skipped),
            ));
        let successful_actions = v_flex()
            .w_full()
            .child(status_metric_row(
                t!("common.success_count").to_string(),
                action_summary
                    .map_or(0, |summary| summary.successful_actions)
                    .to_string(),
            ))
            .child(status_log_row(
                t!("common.last_success").to_string(),
                action_summary.and_then(|summary| summary.last_success.as_ref()),
                None,
            ));
        let failed_actions = v_flex()
            .w_full()
            .child(status_metric_row(
                t!("common.failed_count").to_string(),
                action_summary
                    .map_or(0, |summary| summary.failed_actions)
                    .to_string(),
            ))
            .child(status_log_row(
                t!("common.last_failed").to_string(),
                action_summary.and_then(|summary| summary.last_failed.as_ref()),
                status.last_error.as_deref(),
            ));
        Some(
            v_flex()
                .min_w(px(0.0))
                .gap_4()
                .child(status_state_row(status.state))
                .child(status_section(
                    t!("common.process_activity").to_string(),
                    process_activity.into_any_element(),
                ))
                .child(status_section(
                    t!("common.successful_actions").to_string(),
                    successful_actions.into_any_element(),
                ))
                .child(status_section(
                    t!("common.failed_actions").to_string(),
                    failed_actions.into_any_element(),
                ))
                .into_any_element(),
        )
    }

    pub(in crate::ui::app) fn render_page_status_panel(
        &self,
        page: Page,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let status = match page {
            Page::BackgroundEfficiency => self.render_background_efficiency_status_card(cx),
            Page::MemoryTrim => self.render_memory_trim_status_card(cx),
            _ => self
                .render_normalized_feature_status(page)
                .or_else(|| self.render_power_plan_status(page))?,
        };
        let header = h_flex()
            .min_h(px(48.0))
            .px_3()
            .child(section_title_text(t!("common.status").to_string()))
            .into_any_element();
        let body = v_flex()
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scrollbar()
            .p_3()
            .child(status)
            .into_any_element();
        Some(page_side_panel(header, body))
    }
}

pub(in crate::ui::app) fn action_log_page_help() -> SharedString {
    tooltip_lines(vec![
        t!("action_log.intro_1").to_string(),
        t!("action_log.intro_2").to_string(),
    ])
}

pub(in crate::ui::app) fn page_header_with_help(
    page: Page,
    help: Option<SharedString>,
    transition: Option<&BreadcrumbTransition>,
    cx: &mut Context<WinderustApp>,
) -> gpui::Div {
    let mut header = h_flex()
        .w_full()
        .min_h(px(PAGE_HEADER_HEIGHT))
        .flex_shrink_0()
        .items_center()
        .gap_2()
        .overflow_hidden();
    let mut breadcrumb_row = h_flex()
        .w_full()
        .min_w(px(0.0))
        .items_center()
        .gap_2()
        .overflow_hidden();

    let current_trail = breadcrumb_trail(page);
    let transition = transition.filter(|transition| transition.current == current_trail);
    let entering_start = transition
        .map(|transition| common_breadcrumb_prefix_len(&transition.previous, &current_trail))
        .unwrap_or(current_trail.len());

    if let Some(first) = current_trail.first() {
        breadcrumb_row = breadcrumb_row.child(breadcrumb_segment_element(
            first,
            current_trail.len() == 1,
            true,
            cx,
        ));
    }

    for (index, segment) in current_trail.iter().enumerate().skip(1) {
        let current = index + 1 == current_trail.len();
        let group = breadcrumb_segment_group(segment, current, true, cx);

        if transition.is_some() && index >= entering_start {
            breadcrumb_row = breadcrumb_row.child(breadcrumb_transition_group(
                SharedString::from(format!("breadcrumb-{:?}-{index}", segment.page)),
                true,
                group,
            ));
        } else {
            breadcrumb_row = breadcrumb_row.child(group);
        }
    }

    let mut breadcrumbs = div()
        .flex_1()
        .min_w(px(0.0))
        .relative()
        .overflow_hidden()
        .child(breadcrumb_row);

    if let Some(transition) = transition {
        if breadcrumb_starts_with(&transition.previous, &current_trail)
            && transition.previous.len() > current_trail.len()
        {
            breadcrumbs =
                breadcrumbs.child(breadcrumb_exit_overlay(transition, current_trail.len(), cx));
        }
    }

    header = header.child(breadcrumbs);

    if let Some(help) = help {
        header = header.child(title_info_button(
            SharedString::from(format!("page-info-{page:?}")),
            help,
        ));
    }

    header
}

pub(in crate::ui::app) fn tooltip_lines(
    lines: impl IntoIterator<Item = impl Into<SharedString>>,
) -> SharedString {
    let mut tooltip = String::new();
    for line in lines {
        let line: SharedString = line.into();
        if !tooltip.is_empty() {
            tooltip.push('\n');
        }
        tooltip.push_str(line.as_ref());
    }
    tooltip.into()
}

pub(in crate::ui::app) fn branded_panel() -> gpui::Div {
    v_flex()
        .w_full()
        .min_w(px(0.0))
        .relative()
        .overflow_hidden()
        .rounded(px(BRAND_RADIUS_SURFACE))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
}

pub(in crate::ui::app) fn section_card(title: &str) -> gpui::Div {
    branded_panel()
        .gap_3()
        .p_4()
        .child(section_title_text(title.to_owned()))
}

pub(in crate::ui::app) fn section_header(title: &str, help: impl Into<SharedString>) -> gpui::Div {
    let help = help.into();

    v_flex().w_full().min_w(px(0.0)).child(
        h_flex()
            .w_full()
            .min_h(px(26.0))
            .min_w(px(0.0))
            .items_center()
            .gap_1()
            .child(section_title_text(title.to_owned()))
            .child(title_info_button(
                SharedString::from(format!("section-info-{title}")),
                help,
            )),
    )
}

pub(in crate::ui::app) fn section_title_label(title: impl Into<SharedString>) -> Label {
    Label::new(title)
        .w_full()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::BOLD)
}

pub(in crate::ui::app) fn section_title_text(title: impl Into<SharedString>) -> Label {
    Label::new(title)
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::BOLD)
}

pub(in crate::ui::app) fn title_info_button(
    id: impl Into<SharedString>,
    tooltip: impl Into<SharedString>,
) -> AnyElement {
    div()
        .size(px(26.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .child(
            Button::new(id.into())
                .ghost()
                .rounded(px(999.0))
                .with_size(px(26.0))
                .icon(
                    Icon::new(NavIcon::Info)
                        .with_size(px(14.0))
                        .text_color(rgb(dim_text_color())),
                )
                .tooltip(tooltip),
        )
        .into_any_element()
}

pub(in crate::ui::app) fn rule_card(
    title: AnyElement,
    leading: AnyElement,
    collapse_indicator: AnyElement,
    card_target: RuleCardTarget,
    collapsed: bool,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    rule_card_with_header_action(
        title,
        leading,
        None,
        collapse_indicator,
        card_target,
        collapsed,
        cx,
    )
}

pub(in crate::ui::app) fn rule_card_with_header_action(
    title: AnyElement,
    leading: AnyElement,
    header_action: Option<AnyElement>,
    collapse_indicator: AnyElement,
    card_target: RuleCardTarget,
    _collapsed: bool,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    let header_padding = if header_action.is_some() {
        px(134.0)
    } else {
        px(52.0)
    };
    let card_id = SharedString::from(format!("rule-card-{card_target:?}"));
    let header_id = SharedString::from(format!("rule-card-header-{card_target:?}"));
    let header_action_id = SharedString::from(format!("rule-card-header-action-{card_target:?}"));
    let hover_id = format!("rule-card-hover-{card_target:?}");
    let header_card_target = card_target.clone();
    let trailing_card_target = card_target.clone();
    let trailing_hover_id = hover_id.clone();
    let mut trailing = h_flex()
        .id(SharedString::from(format!(
            "rule-card-trailing-{card_target:?}"
        )))
        .absolute()
        .top(px(0.0))
        .right(px(0.0))
        .h(px(CARD_ROW_HEIGHT))
        .items_center()
        .gap_1()
        .px_2()
        .block_mouse_except_scroll()
        .cursor_pointer()
        .capture_any_mouse_down(cx.listener(|app, event: &gpui::MouseDownEvent, _, cx| {
            handle_navigation_mouse_button(app, event.button, cx);
        }))
        .on_hover(move |hovered, _, cx| {
            set_card_hovered(trailing_hover_id.clone(), *hovered, cx);
        })
        .on_click(cx.listener(move |app, _, _, cx| {
            app.toggle_rule_card(trailing_card_target.clone(), cx);
        }));
    if let Some(header_action) = header_action {
        trailing = trailing.child(header_action);
    }
    trailing = trailing.child(collapse_indicator);

    v_flex()
        .id(card_id)
        .w_full()
        .min_w(px(0.0))
        .relative()
        .overflow_hidden()
        .border_b_1()
        .border_color(rgb(border_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .child(
            div()
                .relative()
                .w_full()
                .min_w(px(0.0))
                .h(px(CARD_ROW_HEIGHT))
                .id(header_id)
                .overflow_hidden()
                .child(animated_card_hover_layer(&hover_id))
                .child(
                    h_flex()
                        .w_full()
                        .min_w(px(0.0))
                        .h(px(CARD_ROW_HEIGHT))
                        .items_center()
                        .gap_2()
                        .pl_4()
                        .pr(header_padding)
                        .id(header_action_id)
                        .block_mouse_except_scroll()
                        .cursor_pointer()
                        .capture_any_mouse_down(cx.listener(
                            |app, event: &gpui::MouseDownEvent, _, cx| {
                                handle_navigation_mouse_button(app, event.button, cx);
                            },
                        ))
                        .on_hover({
                            let hover_id = hover_id.clone();
                            move |hovered, _, cx| {
                                set_card_hovered(hover_id.clone(), *hovered, cx);
                            }
                        })
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.toggle_rule_card(header_card_target.clone(), cx);
                        }))
                        .child(leading)
                        .child(title),
                )
                .child(trailing),
        )
}

pub(in crate::ui::app) fn rule_card_collapse_indicator(
    card_target: RuleCardTarget,
    collapsed: bool,
) -> AnyElement {
    div()
        .w(px(28.0))
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(dim_text_color()))
        .opacity(0.72)
        .cursor_pointer()
        .child(collapsible_chevron_icon(
            rule_card_body_motion_id(&card_target),
            collapsed,
        ))
        .into_any_element()
}
