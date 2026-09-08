use super::action_log::{action_log_process_label, action_log_time_label};
use crate::{
    action_log::{ActionLogEntry, ActionLogFeature},
    automation::RuntimeStatusSnapshot,
    config::Settings,
    power::PowerPlan,
    ui::Page,
};
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Fill};
use rust_i18n::t;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Debug, Clone)]
pub(super) enum Message {
    ActionLog,
    RelaunchAdmin,
}

pub(super) fn view<'a>(
    page: Page,
    settings: &Settings,
    runtime: &'a RuntimeStatusSnapshot,
    plans: &[PowerPlan],
) -> Option<Element<'a, Message>> {
    let power_feature = match page {
        Page::ByActivity => Some((settings.by_activity.enabled, ActionLogFeature::ByActivity)),
        Page::ByForeground => Some((
            settings.by_foreground.enabled,
            ActionLogFeature::ByForeground,
        )),
        Page::ByRunningApp => Some((
            settings.by_running_app.enabled,
            ActionLogFeature::ByRunningApp,
        )),
        Page::ByCpuLoad => Some((settings.by_cpu_load.enabled, ActionLogFeature::ByCpuLoad)),
        Page::ByTime => Some((settings.by_time.enabled, ActionLogFeature::ByTime)),
        _ => None,
    };
    let mut summary = if let Some((enabled, feature)) = power_feature {
        FeatureStatusSummary {
            state: feature_run_state(
                settings.general.enabled && enabled,
                runtime.power_plan_status.current_guid.is_none(),
            ),
            scanned: None,
            adjusted: None,
            protected_or_denied: None,
            skipped: None,
            last_error: None,
            action_log_feature: feature,
        }
    } else {
        feature_status_summary(page, settings, runtime)?
    };
    let available = runtime.generation != 0;
    if !available || runtime.worker_error.is_some() {
        summary.state = FeatureRunState::Unknown;
    }
    let state_label = match summary.state {
        FeatureRunState::Running => "common.running",
        FeatureRunState::NotRunning => "common.not_running",
        FeatureRunState::Unknown => "common.unknown",
    };
    let mut body = column![
        text(t!("common.status").to_string()).size(18),
        text(t!(state_label).to_string())
            .size(13)
            .style(match summary.state {
                FeatureRunState::Running => text::success,
                FeatureRunState::NotRunning | FeatureRunState::Unknown => text::secondary,
            })
    ]
    .spacing(12);
    if power_feature.is_some() {
        let plan = runtime
            .power_plan_status
            .current_guid
            .as_ref()
            .map(|guid| {
                plans
                    .iter()
                    .find(|p| p.guid.eq_ignore_ascii_case(guid))
                    .map(|p| p.display_name())
                    .unwrap_or_else(|| guid.clone())
            })
            .unwrap_or_else(|| t!("common.unknown").to_string());
        body = body.push(section("common.current_power_plan", column![text(plan)]));
    } else {
        body = body.push(section(
            "common.process_activity",
            column![
                metric(
                    "common.scanned_processes",
                    count(available.then_some(summary.scanned).flatten())
                ),
                metric(
                    "common.adjusted_processes",
                    count(available.then_some(summary.adjusted).flatten())
                ),
                metric(
                    "common.protected_or_denied_processes",
                    count(available.then_some(summary.protected_or_denied).flatten())
                ),
                metric(
                    "common.skipped_processes",
                    count(available.then_some(summary.skipped).flatten())
                )
            ]
            .spacing(6),
        ));
    }
    let actions = runtime
        .action_log_summaries
        .get(&summary.action_log_feature);
    body = body
        .push(section(
            "common.successful_actions",
            column![
                metric(
                    "common.success_count",
                    count(available.then(|| actions.map_or(0, |s| s.successful_actions)))
                ),
                log(
                    "common.last_success",
                    actions.and_then(|s| s.last_success.as_ref()),
                    None
                )
            ]
            .spacing(6),
        ))
        .push(section(
            "common.failed_actions",
            column![
                metric(
                    "common.failed_count",
                    count(available.then(|| actions.map_or(0, |s| s.failed_actions)))
                ),
                log(
                    "common.last_failed",
                    actions.and_then(|s| s.last_failed.as_ref()),
                    summary.last_error.as_deref()
                )
            ]
            .spacing(6),
        ));
    if page == Page::AdaptiveEngine {
        use crate::bottleneck_classifier::BottleneckState;
        let status = &runtime.feature_status.bottleneck_classifier;
        let key = match status.state {
            BottleneckState::Unknown => "adaptive_engine.bottleneck_unknown",
            BottleneckState::Headroom => "adaptive_engine.bottleneck_headroom",
            BottleneckState::CpuBound => "adaptive_engine.bottleneck_cpu_bound",
            BottleneckState::GpuBound => "adaptive_engine.bottleneck_gpu_bound",
            BottleneckState::Mixed => "adaptive_engine.bottleneck_mixed",
        };
        body = body.push(section(
            "adaptive_engine.bottleneck_classifier",
            column![
                metric("common.status", t!(key).to_string()),
                metric(
                    "adaptive_engine.total_cpu",
                    percent(status.total_cpu_tenths)
                ),
                metric(
                    "adaptive_engine.busiest_cpu",
                    percent(status.busiest_cpu_tenths)
                ),
                metric(
                    "adaptive_engine.busiest_gpu",
                    percent(status.busiest_gpu_tenths)
                )
            ]
            .spacing(6),
        ));
    }
    if let Some(error) = &runtime.worker_error {
        body = body.push(text(error));
    }
    if page == Page::BackgroundEfficiency
        && runtime
            .feature_status
            .background_efficiency
            .access_denied_processes
            > 0
        && !crate::privilege::is_running_as_admin()
    {
        body = body.push(
            button(text(t!("admin_rights.relaunch").to_string())).on_press(Message::RelaunchAdmin),
        );
    }
    body = body.push(
        button(text(Page::ActionLog.label()))
            .on_press(Message::ActionLog)
            .style(iced::widget::button::secondary),
    );
    Some(
        scrollable(container(body.width(Fill)).width(Fill).padding([0, 12]))
            .width(Fill)
            .height(Fill)
            .into(),
    )
}
fn count(value: Option<usize>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "\u{2014}".to_owned())
}
fn percent(value: Option<u16>) -> String {
    value
        .map(|v| format!("{}.{:01}%", v / 10, v % 10))
        .unwrap_or_else(|| "\u{2014}".to_owned())
}
fn metric(label: &str, value: String) -> Element<'static, Message> {
    row![
        text(t!(label).to_string())
            .width(iced::Length::FillPortion(2))
            .size(12),
        text(value)
            .width(iced::Length::FillPortion(1))
            .align_x(iced::Right)
            .size(13)
    ]
    .spacing(8)
    .width(Fill)
    .into()
}
fn section<'a>(title: &str, body: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    container(column![text(t!(title).to_string()).size(14), body].spacing(8))
        .width(Fill)
        .padding(10)
        .style(iced::widget::container::bordered_box)
        .into()
}
fn log(
    label: &str,
    entry: Option<&ActionLogEntry>,
    fallback: Option<&str>,
) -> Element<'static, Message> {
    let value = entry
        .map(|e| {
            format!(
                "[{}] {} - {}",
                action_log_time_label(e.timestamp_epoch_ms),
                action_log_process_label(e),
                e.reason
            )
        })
        .or_else(|| fallback.map(str::to_owned))
        .unwrap_or_else(|| t!("common.none").to_string());
    column![
        text(t!(label).to_string()).size(12),
        text(value).width(Fill).size(12)
    ]
    .width(Fill)
    .spacing(4)
    .into()
}
fn feature_status_summary(
    page: Page,
    settings: &Settings,
    runtime: &RuntimeStatusSnapshot,
) -> Option<FeatureStatusSummary> {
    let summary = match page {
        Page::AdaptiveEngine => {
            let status = &runtime.feature_status.cpu_scheduler;
            FeatureStatusSummary {
                state: feature_run_state(
                    settings.general.enabled && settings.adaptive_engine.enabled,
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
            let status = &runtime.feature_status.background_efficiency;
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
            let status = &runtime.feature_status.memory_trim;
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
            let status = &runtime.feature_status.process_priority;
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
            let status = &runtime.feature_status.thread_priority;
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
            let status = &runtime.feature_status.dynamic_priority_boost;
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
            let status = &runtime.feature_status.io_priority;
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
            let status = &runtime.feature_status.gpu_priority;
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
            let status = &runtime.feature_status.memory_priority;
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
        Page::CpuLimiter => {
            let status = &runtime.feature_status.cpu_limiter;
            FeatureStatusSummary {
                state: feature_run_state(status.enabled, false),
                scanned: Some(status.scanned_processes),
                adjusted: Some(status.limited_processes),
                protected_or_denied: None,
                skipped: Some(status.skipped_processes),
                last_error: status.last_error.clone(),
                action_log_feature: ActionLogFeature::CpuLimiter,
            }
        }
        Page::CpuSetsSoft | Page::ProcessorAffinityHard => {
            let (status, feature) = if page == Page::CpuSetsSoft {
                (
                    &runtime.feature_status.cpu_sets_soft,
                    ActionLogFeature::CpuSetsSoft,
                )
            } else {
                (
                    &runtime.feature_status.processor_affinity_hard,
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
            let status = &runtime.feature_status.app_suspension;
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
            let status = &runtime.feature_status.timer_resolution;
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_metrics_are_not_reported_as_zero() {
        assert_eq!(count(None), "\u{2014}");
        assert_eq!(count(Some(0)), "0");
        assert_eq!(feature_run_state(true, true), FeatureRunState::Unknown);
        assert_eq!(feature_run_state(false, true), FeatureRunState::NotRunning);
        let settings = Settings::default();
        let runtime = RuntimeStatusSnapshot::default();
        let summary = feature_status_summary(Page::MemoryPriority, &settings, &runtime).unwrap();
        assert_eq!(summary.scanned, None);
        assert_eq!(summary.protected_or_denied, None);
    }
}
