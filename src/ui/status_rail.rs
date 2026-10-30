use super::action_log::{action_log_process_label, action_log_time_label};
use super::design;
use super::widgets::{self, Ui};
use crate::ui::scrolling::scrollable;
use crate::{
    action_log::{ActionLogEntry, ActionLogFeature},
    automation::RuntimeStatusSnapshot,
    config::Settings,
    power::PowerPlan,
    ui::Page,
};
use rust_i18n::t;
use windui::{
    prelude::{Align, Element, Role},
    theme::Intent,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FeatureRunState {
    Running,
    Waiting,
    Paused,
    Blocked,
    Error,
    Inactive,
    Unknown,
}
struct FeatureStatusSummary {
    scanned: Option<usize>,
    adjusted: Option<usize>,
    protected_or_denied: Option<usize>,
    skipped: Option<usize>,
    last_error: Option<String>,
    action_log_feature: ActionLogFeature,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    RelaunchAdmin,
}

pub(super) fn view(
    page: Page,
    settings: &crate::application::SettingsEditor,
    runtime: &RuntimeStatusSnapshot,
    plans: &[PowerPlan],
    ui: Ui<Message>,
) -> Option<Element> {
    let settings = saved_runtime_settings(settings, crate::backend::power_source::is_plugged_in());
    let power_feature = match page {
        Page::ByActivity => Some(ActionLogFeature::ByActivity),
        Page::ByForeground => Some(ActionLogFeature::ByForeground),
        Page::ByRunningApp => Some(ActionLogFeature::ByRunningApp),
        Page::ByCpuLoad => Some(ActionLogFeature::ByCpuLoad),
        Page::ByTime => Some(ActionLogFeature::ByTime),
        _ => None,
    };
    let summary = if let Some(feature) = power_feature {
        FeatureStatusSummary {
            scanned: None,
            adjusted: None,
            protected_or_denied: None,
            skipped: None,
            last_error: None,
            action_log_feature: feature,
        }
    } else {
        feature_status_summary(page, runtime)?
    };
    let available = runtime.generation != 0;
    let state = sidebar_state(page, settings, runtime, &summary);
    let (state_label, detail, color) = match state {
        FeatureRunState::Running => (
            "common.running",
            "sidebar_status.running_help",
            "common.applied",
        ),
        FeatureRunState::Waiting => (
            "common.waiting",
            "sidebar_status.waiting_help",
            "common.waiting",
        ),
        FeatureRunState::Paused => (
            "sidebar_status.paused",
            "sidebar_status.paused_help",
            "common.waiting",
        ),
        FeatureRunState::Blocked => (
            "sidebar_status.blocked",
            "sidebar_status.blocked_help",
            "common.error",
        ),
        FeatureRunState::Error => ("common.error", "sidebar_status.error_help", "common.error"),
        FeatureRunState::Inactive => (
            "common.inactive",
            "sidebar_status.inactive_help",
            "common.inactive",
        ),
        FeatureRunState::Unknown => (
            "common.unknown",
            "sidebar_status.unknown_help",
            "common.unknown",
        ),
    };
    let mut body = widgets::col()
        .child(chip(state_label, color))
        .child(caption(t!(detail).to_string()).fg_role(Role::TextMuted));
    if power_feature.is_some() {
        let plan = runtime
            .power_plan_status
            .current_guid
            .as_ref()
            .map(|guid| {
                plans
                    .iter()
                    .find(|plan| plan.guid.eq_ignore_ascii_case(guid))
                    .map(PowerPlan::display_name)
                    .unwrap_or_else(|| guid.clone())
            })
            .unwrap_or_else(|| t!("common.unknown").to_string());
        body = body.child(section("common.current_power_plan", caption(plan)));
    } else {
        body = body.child(section(
            "common.process_activity",
            metric_group()
                .child(metric(
                    "common.scanned_processes",
                    count(available.then_some(summary.scanned).flatten()),
                ))
                .child(metric(
                    "common.adjusted_processes",
                    count(available.then_some(summary.adjusted).flatten()),
                ))
                .child(metric(
                    "common.protected_or_denied_processes",
                    count(available.then_some(summary.protected_or_denied).flatten()),
                ))
                .child(metric(
                    "common.skipped_processes",
                    count(available.then_some(summary.skipped).flatten()),
                )),
        ));
    }
    let actions = runtime
        .action_log_summaries
        .get(&summary.action_log_feature);
    body = body
        .child(section(
            "common.successful_actions",
            metric_group()
                .child(metric(
                    "common.success_count",
                    count(
                        available.then(|| actions.map_or(0, |summary| summary.successful_actions)),
                    ),
                ))
                .child(log(
                    "common.last_success",
                    actions.and_then(|summary| summary.last_success.as_ref()),
                    None,
                )),
        ))
        .child(section(
            "common.failed_actions",
            metric_group()
                .child(metric(
                    "common.failed_count",
                    count(available.then(|| actions.map_or(0, |summary| summary.failed_actions))),
                ))
                .child(log(
                    "common.last_failed",
                    actions.and_then(|summary| summary.last_failed.as_ref()),
                    summary.last_error.as_deref(),
                )),
        ));
    if page == Page::AdaptiveEngine {
        use crate::adaptive_engine_process::ZoneStatus;
        let zones = &runtime.feature_status.adaptive_engine_process.zones;
        let (key, color) = match zones.status {
            ZoneStatus::Disabled => ("common.inactive", "common.inactive"),
            ZoneStatus::Waiting => ("common.waiting", "common.waiting"),
            ZoneStatus::Applying => ("adaptive_engine_process.zone_applying", "common.waiting"),
            ZoneStatus::Active => ("common.applied", "common.applied"),
            ZoneStatus::Overridden => ("adaptive_engine_process.zone_overridden", "common.waiting"),
            ZoneStatus::Degraded => ("adaptive_engine_process.zone_degraded", "common.error"),
            ZoneStatus::Unavailable => ("common.unavailable", "common.unknown"),
        };
        let reason = if zones.reason.is_empty() {
            String::new()
        } else {
            let key = format!("adaptive_engine_process.{}", zones.reason);
            t!(&key).to_string()
        };
        let mut zone_metrics = metric_group().child(chip(key, color));
        if !reason.is_empty() {
            zone_metrics = zone_metrics.child(caption(reason));
        }
        body = body.child(section(
            "adaptive_engine_process.dynamic_resource_zones",
            zone_metrics
                .child(metric(
                    "adaptive_engine_process.zone_foreground_cpus",
                    zones.foreground_processors.to_string(),
                ))
                .child(metric(
                    "adaptive_engine_process.zone_background_cpus",
                    zones.background_processors.to_string(),
                ))
                .child(metric(
                    "adaptive_engine_process.zone_foreground_targets",
                    zones.foreground_targets.to_string(),
                ))
                .child(metric(
                    "adaptive_engine_process.zone_background_targets",
                    zones.background_targets.to_string(),
                )),
        ));
        if settings
            .adaptive_engine_process
            .limit_background_processors_enabled
        {
            body = body.child(section(
                "adaptive_engine_process.limit_background_processors",
                caption(
                    t!(if zones.status == ZoneStatus::Active {
                        "adaptive_engine_process.limit_waiting_zones"
                    } else if zones.background_limit_targets > 0 {
                        "common.applied"
                    } else {
                        "common.waiting"
                    })
                    .to_string(),
                ),
            ));
        }
        use crate::bottleneck_classifier::BottleneckState;
        let status = &runtime.feature_status.bottleneck_classifier;
        let key = match status.state {
            BottleneckState::Unknown => "adaptive_engine.bottleneck_unknown",
            BottleneckState::Headroom => "adaptive_engine.bottleneck_headroom",
            BottleneckState::CpuBound => "adaptive_engine.bottleneck_cpu_bound",
            BottleneckState::GpuBound => "adaptive_engine.bottleneck_gpu_bound",
            BottleneckState::Mixed => "adaptive_engine.bottleneck_mixed",
        };
        body = body.child(section(
            "adaptive_engine.bottleneck_classifier",
            metric_group()
                .child(caption(t!(key).to_string()).font_weight(600))
                .child(metric(
                    "adaptive_engine.total_cpu",
                    percent(status.total_cpu_tenths),
                ))
                .child(metric(
                    "adaptive_engine.busiest_cpu",
                    percent(status.busiest_cpu_tenths),
                ))
                .child(metric(
                    "adaptive_engine.busiest_gpu",
                    percent(status.busiest_gpu_tenths),
                )),
        ));
    }
    if page == Page::BackgroundEfficiency
        && runtime
            .feature_status
            .background_efficiency
            .access_denied_processes
            > 0
        && !crate::privilege::is_running_as_admin()
    {
        body = body.child(ui.button(
            t!("admin_rights.relaunch").to_string(),
            Message::RelaunchAdmin,
        ));
    }
    let content = scrollable(body.padding_xy(design::space::MEDIUM as i32, 0)).fill();
    Some(
        if matches!(
            page,
            Page::AdaptiveEngine | Page::CpuSetsSoft | Page::ProcessorAffinityHard
        ) {
            content
        } else {
            widgets::col()
                .fill()
                .child(
                    Element::row()
                        .width_match()
                        .height(design::NAVIGATION_ROW_HEIGHT as i32)
                        .cross(Align::Center)
                        .padding_xy(design::space::MEDIUM as i32, 0)
                        .child(widgets::heading(t!("common.status").to_string())),
                )
                .child(content.weight(1.0))
        },
    )
}

fn chip(label: &str, color: &str) -> Element {
    Element::badge_intent(
        t!(label).to_string(),
        match color {
            "common.applied" => Intent::Success,
            "common.waiting" => Intent::Warning,
            "common.error" => Intent::Danger,
            "common.unknown" => Intent::CustomRole(Role::Accent),
            _ => Intent::Neutral,
        },
    )
    .font_size(design::typography::SECONDARY as f32)
}

fn saved_runtime_settings(
    editor: &crate::application::SettingsEditor,
    plugged_in: Option<bool>,
) -> &Settings {
    let saved = editor.persisted();
    if plugged_in == Some(false) {
        saved.battery_profile()
    } else {
        saved
    }
}

fn sidebar_state(
    page: Page,
    settings: &Settings,
    runtime: &RuntimeStatusSnapshot,
    summary: &FeatureStatusSummary,
) -> FeatureRunState {
    use crate::control::power_plan::PowerPlanOwner;
    use crate::rules::DecisionState;
    if !settings.general.enabled
        || super::navigation::feature_page_enabled(settings, page) == Some(false)
    {
        return FeatureRunState::Inactive;
    }
    if runtime.worker_error.is_some() {
        return FeatureRunState::Error;
    }
    if runtime.generation == 0 {
        return FeatureRunState::Unknown;
    }
    if summary.last_error.is_some() {
        return FeatureRunState::Error;
    }
    let plan = &runtime.power_plan_status;
    let plan_applied = plan
        .target_guid
        .as_deref()
        .zip(plan.current_guid.as_deref())
        .is_some_and(|(target, current)| target.eq_ignore_ascii_case(current));
    let power_state = match page {
        Page::ByActivity => Some(matches!(
            plan.decision_state,
            Some(DecisionState::ByActivityIdle | DecisionState::ByActivityActive)
        )),
        Page::ByForeground => Some(plan.decision_state == Some(DecisionState::ByForeground)),
        Page::ByRunningApp => Some(plan.decision_state == Some(DecisionState::ByRunningApp)),
        Page::ByCpuLoad => Some(plan.decision_state == Some(DecisionState::ByCpuLoad)),
        Page::ByTime => Some(plan.decision_state == Some(DecisionState::ByTime)),
        _ => None,
    };
    if let Some(selected) = power_state {
        if plan.current_guid.is_none() {
            return FeatureRunState::Unknown;
        }
        if plan.owner == Some(PowerPlanOwner::AdaptiveEngine)
            || plan.decision_state == Some(DecisionState::PausedWhilePluggedIn)
        {
            return FeatureRunState::Paused;
        }
        if selected && plan.apply_failed {
            return FeatureRunState::Error;
        }
        return if selected && plan_applied && plan.owner == Some(PowerPlanOwner::OrdinaryAutomation)
        {
            FeatureRunState::Running
        } else {
            FeatureRunState::Waiting
        };
    }
    let features = &runtime.feature_status;
    if (page == Page::BackgroundEfficiency && features.background_efficiency.unsupported)
        || (page == Page::AppSuspension && features.app_suspension.unsupported)
    {
        return FeatureRunState::Blocked;
    }
    if page == Page::AppSuspension && features.app_suspension.status_unknown {
        return FeatureRunState::Unknown;
    }
    if page == Page::MemoryTrim
        && matches!(
            features.memory_trim.status,
            crate::features::winderust_features::memory_trim::MemoryTrimStatus::ForegroundUnknown
        )
    {
        return FeatureRunState::Paused;
    }
    let active = summary.adjusted.is_some_and(|count| count > 0)
        || (page == Page::TimerResolution && features.timer_resolution.requested_100ns.is_some())
        || (page == Page::AdaptiveEngine
            && plan.owner == Some(PowerPlanOwner::AdaptiveEngine)
            && plan_applied);
    if active {
        FeatureRunState::Running
    } else if summary.protected_or_denied.is_some_and(|count| count > 0) {
        FeatureRunState::Blocked
    } else {
        FeatureRunState::Waiting
    }
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
fn metric(label: &str, value: String) -> Element {
    widgets::row()
        .padding_xy(0, 3)
        .child(
            caption(t!(label).to_string())
                .fg_role(Role::TextMuted)
                .weight(1.0),
        )
        .child(
            widgets::label(value)
                .font_size(design::typography::SECONDARY as f32)
                .font_weight(600)
                .text_align(Align::End)
                .min_width(40)
                .max_width(76),
        )
}
fn section(title: &str, body: Element) -> Element {
    widgets::col()
        .spacing(design::space::SMALL as i32)
        .child(Element::divider())
        .child(
            widgets::label(t!(title).to_string())
                .font_size(design::typography::BODY as f32)
                .font_weight(600)
                .width_match(),
        )
        .child(body)
}
fn metric_group() -> Element {
    widgets::col().spacing(design::space::CONTROL as i32)
}
fn caption(text: impl Into<String>) -> Element {
    widgets::label(text)
        .font_size(design::typography::CAPTION as f32)
        .width_match()
}
fn log(label: &str, entry: Option<&ActionLogEntry>, fallback: Option<&str>) -> Element {
    let value = entry
        .map(|entry| {
            format!(
                "[{}] {} - {}",
                action_log_time_label(entry.timestamp_epoch_ms),
                action_log_process_label(entry),
                entry.reason
            )
        })
        .or_else(|| fallback.map(str::to_owned))
        .unwrap_or_else(|| t!("common.none").to_string());
    widgets::col()
        .spacing(design::space::TIGHT as i32)
        .child(caption(t!(label).to_string()).fg_role(Role::TextMuted))
        .child(caption(value))
}
fn feature_status_summary(
    page: Page,
    runtime: &RuntimeStatusSnapshot,
) -> Option<FeatureStatusSummary> {
    let summary = match page {
        Page::AdaptiveEngine => {
            let status = &runtime.feature_status.adaptive_engine_process;
            FeatureStatusSummary {
                scanned: Some(status.scanned_processes),
                adjusted: Some(status.adjusted_processes),
                protected_or_denied: None,
                skipped: Some(status.skipped_processes),
                last_error: status.last_error.clone(),
                action_log_feature: ActionLogFeature::AdaptiveEngine,
            }
        }
        Page::BackgroundEfficiency => {
            let status = &runtime.feature_status.background_efficiency;
            FeatureStatusSummary {
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
    fn unsaved_toggle_edits_do_not_change_sidebar_settings() {
        let mut saved = Settings::default();
        saved.general.enabled = true;
        saved.process_priority.enabled = true;
        let mut editor = crate::application::SettingsEditor::with_settings(saved);
        editor.general.enabled = false;
        editor.process_priority.enabled = false;
        assert!(saved_runtime_settings(&editor, Some(true)).general.enabled);
        assert!(
            saved_runtime_settings(&editor, Some(true))
                .process_priority
                .enabled
        );
        editor.process_priority.enabled = true;
        assert!(
            saved_runtime_settings(&editor, Some(true))
                .process_priority
                .enabled
        );
    }

    #[test]
    fn sidebar_distinguishes_enabled_from_actual_work() {
        let mut settings = Settings::default();
        settings.general.enabled = true;
        settings.process_priority.enabled = true;
        let mut runtime = RuntimeStatusSnapshot::default();
        let state = |settings: &Settings, runtime: &RuntimeStatusSnapshot| {
            let summary = feature_status_summary(Page::ProcessPriority, runtime).unwrap();
            sidebar_state(Page::ProcessPriority, settings, runtime, &summary)
        };
        assert_eq!(state(&settings, &runtime), FeatureRunState::Unknown);
        runtime.generation = 1;
        assert_eq!(state(&settings, &runtime), FeatureRunState::Waiting);
        std::sync::Arc::get_mut(&mut runtime.feature_status)
            .unwrap()
            .process_priority
            .adjusted_processes = 1;
        assert_eq!(state(&settings, &runtime), FeatureRunState::Running);
        std::sync::Arc::get_mut(&mut runtime.feature_status)
            .unwrap()
            .process_priority
            .last_error = Some("apply failed".into());
        assert_eq!(state(&settings, &runtime), FeatureRunState::Error);
        settings.process_priority.enabled = false;
        assert_eq!(state(&settings, &runtime), FeatureRunState::Inactive);
    }

    #[test]
    fn power_sidebar_requires_ownership_and_confirmed_target() {
        use crate::{
            control::power_plan::{PowerPlanOwner, PowerPlanStatus},
            rules::DecisionState,
        };
        let mut settings = Settings::default();
        settings.general.enabled = true;
        settings.by_time.enabled = true;
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            ..Default::default()
        };
        let summary = FeatureStatusSummary {
            scanned: None,
            adjusted: None,
            protected_or_denied: None,
            skipped: None,
            last_error: None,
            action_log_feature: ActionLogFeature::ByTime,
        };
        let state = |runtime: &RuntimeStatusSnapshot| {
            sidebar_state(Page::ByTime, &settings, runtime, &summary)
        };
        let plan = std::sync::Arc::make_mut(&mut runtime.power_plan_status);
        *plan = PowerPlanStatus {
            owner: Some(PowerPlanOwner::OrdinaryAutomation),
            decision_state: Some(DecisionState::ByTime),
            current_guid: Some("old".into()),
            target_guid: Some("new".into()),
            ..Default::default()
        };
        assert_eq!(state(&runtime), FeatureRunState::Waiting);
        std::sync::Arc::make_mut(&mut runtime.power_plan_status).apply_failed = true;
        assert_eq!(state(&runtime), FeatureRunState::Error);
        let plan = std::sync::Arc::make_mut(&mut runtime.power_plan_status);
        plan.apply_failed = false;
        plan.current_guid = Some("NEW".into());
        assert_eq!(state(&runtime), FeatureRunState::Running);
        std::sync::Arc::make_mut(&mut runtime.power_plan_status).owner =
            Some(PowerPlanOwner::AdaptiveEngine);
        assert_eq!(state(&runtime), FeatureRunState::Paused);
    }

    #[test]
    fn unavailable_metrics_are_not_reported_as_zero() {
        assert_eq!(count(None), "\u{2014}");
        assert_eq!(count(Some(0)), "0");
        let runtime = RuntimeStatusSnapshot::default();
        let summary = feature_status_summary(Page::MemoryPriority, &runtime).unwrap();
        assert_eq!(summary.scanned, None);
        assert_eq!(summary.protected_or_denied, None);
    }
}
