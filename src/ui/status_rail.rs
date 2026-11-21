use super::action_log::{action_log_process_label, action_log_time_label};
use super::design;
use super::widgets::{action_button, power_plan_label};
use crate::ui::scrolling::scrollable;
use crate::{
    action_log::{ActionLogEntry, ActionLogFeature},
    automation::RuntimeStatusSnapshot,
    config::Settings,
    control::instance_limits::CountObservation,
    features::{
        advanced_controls::instance_limits::{InstanceLimitsSnapshot, NotificationStatus},
        winderust_features::memory_trim::MemoryTrimStatus,
    },
    power::PowerPlan,
    ui::Page,
};
use iced::widget::{column, container, row, text};
use iced::{Element, Fill};
use rust_i18n::t;

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
struct FeatureStatusSummary<'a> {
    scanned: Option<usize>,
    adjusted: Option<usize>,
    protected_or_denied: Option<usize>,
    skipped: Option<usize>,
    last_error: Option<&'a str>,
    action_log_feature: ActionLogFeature,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    RelaunchAdmin,
}

pub(super) fn view<'a>(
    page: Page,
    settings: &Settings,
    runtime: &'a RuntimeStatusSnapshot,
    plans: &[PowerPlan],
) -> Option<Element<'a, Message>> {
    if page == Page::PowerPlanControl {
        return Some(super::power_plan_control::status_view(
            &runtime.power_plan_status,
            plans,
        ));
    }
    let power_feature = match page {
        Page::ByActivity => Some(ActionLogFeature::ByActivity),
        Page::ByFocus => Some(ActionLogFeature::ByFocus),
        Page::ByRunningApp => Some(ActionLogFeature::ByRunningApp),
        Page::ByCpuLoad => Some(ActionLogFeature::ByCpuLoad),
        Page::ByTime => Some(ActionLogFeature::ByTime),
        _ => None,
    };
    let instance_limits = (page == Page::InstanceLimits)
        .then(|| instance_limits_status(settings, &runtime.feature_status.instance_limits));
    let summary = if let Some(limits) = &instance_limits {
        FeatureStatusSummary {
            scanned: None,
            adjusted: None,
            protected_or_denied: None,
            skipped: None,
            last_error: limits.last_error,
            action_log_feature: ActionLogFeature::InstanceLimits,
        }
    } else if let Some(feature) = power_feature {
        FeatureStatusSummary {
            scanned: None,
            adjusted: None,
            protected_or_denied: None,
            skipped: None,
            last_error: runtime.power_plan_status.active_plan_read_error.as_deref(),
            action_log_feature: feature,
        }
    } else {
        feature_status_summary(page, runtime)?
    };
    let available = runtime.generation != 0;
    let state = sidebar_state(
        page,
        settings,
        runtime,
        &summary,
        instance_limits.as_ref().map(|limits| limits.state),
    );
    let (state_label, detail, color) = match state {
        FeatureRunState::Running if page == Page::SleepPrevention => (
            "sleep_prevention.request_accepted",
            "sleep_prevention.request_status_help",
            "common.applied",
        ),
        FeatureRunState::Paused if page == Page::SleepPrevention => (
            "sleep_prevention.requests_paused",
            "sleep_prevention.paused_help",
            "common.waiting",
        ),
        FeatureRunState::Running if page == Page::InstanceLimits => (
            "instance_limits.monitoring",
            "instance_limits.intro_1",
            "common.applied",
        ),
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
    let detail = if page == Page::InstanceBalancing
        && available
        && !matches!(state, FeatureRunState::Inactive | FeatureRunState::Unknown)
        && runtime.worker_error.is_none()
    {
        runtime.feature_status.instance_balancing.message()
    } else if page == Page::AdaptiveEngine
        && state == FeatureRunState::Running
        && settings.adaptive_engine_process.has_enabled_policies()
        && runtime
            .feature_status
            .adaptive_engine_process
            .observation_paused
    {
        Some(t!("adaptive_engine_process.power_running_process_paused").into_owned())
    } else {
        None
    }
    .unwrap_or_else(|| t!(detail).into_owned());
    let mut body = column![
        container(text(t!(state_label).to_string()).size(design::typography::SECONDARY))
            .padding([4, 8])
            .style(move |theme| super::widgets::rule_status_chip(theme, color)),
        text(detail)
            .size(design::typography::CAPTION)
            .style(text::secondary)
    ]
    .spacing(design::space::MEDIUM);
    let actions = runtime
        .action_log_summaries
        .get(&summary.action_log_feature);
    let last_failed = actions.and_then(|summary| summary.last_failed.as_ref());
    let current_error = summary.last_error.filter(|reason| {
        (page != Page::InstanceLimits || settings.instance_limits.notify)
            && last_failed.is_none_or(|entry| entry.reason.as_str() != *reason)
    });
    if let Some(error) = current_error {
        body = body.push(detail_row("sidebar_status.current_error", error.to_owned()));
    }
    if power_feature.is_some() {
        let plan = super::power_plan_control::current_plan_guid(&runtime.power_plan_status, plans)
            .map(|guid| {
                plans
                    .iter()
                    .find(|p| p.guid.eq_ignore_ascii_case(guid))
                    .map(power_plan_label)
                    .unwrap_or_else(|| guid.to_owned())
            })
            .unwrap_or_else(|| t!("common.unknown").to_string());
        body = body.push(section("common.current_power_plan", column![text(plan)]));
    } else if let Some(limits) = &instance_limits {
        body = body.push(section(
            "instance_limits.monitoring",
            column![
                metric(
                    "common.rules",
                    count(available.then_some(limits.enabled_rules))
                ),
                metric(
                    "instance_limits.incidents",
                    count(available.then_some(limits.incidents))
                ),
                text(t!("instance_limits.intro_2").to_string()).size(design::typography::CAPTION),
            ]
            .spacing(design::space::CONTROL),
        ));
    } else if page == Page::SleepPrevention {
        let sleep = &runtime.feature_status.sleep_prevention;
        let accepted = |requested| {
            if !available {
                t!("common.unknown")
            } else if requested {
                t!("sleep_prevention.request_accepted")
            } else {
                t!("sleep_prevention.not_requested")
            }
            .to_string()
        };
        body = body.push(section(
            "sleep_prevention.requests",
            column![
                metric(
                    "sleep_prevention.matching_rules",
                    count((available && sleep.observation_known).then_some(sleep.matched_rules))
                ),
                metric(
                    "sleep_prevention.matching_processes",
                    count(
                        (available && sleep.observation_known).then_some(sleep.matched_processes)
                    )
                ),
                metric(
                    "sleep_prevention.system_request",
                    accepted(sleep.system_requested)
                ),
                metric(
                    "sleep_prevention.display_request",
                    accepted(sleep.display_requested)
                ),
            ]
            .spacing(design::space::CONTROL),
        ));
    } else if page == Page::TimerResolution {
        let request = runtime
            .feature_status
            .timer_resolution
            .requested_100ns
            .map(crate::timer_resolution::format_resolution_ms)
            .unwrap_or_else(|| "\u{2014}".to_owned());
        body = body.push(section(
            "timer_resolution.requested",
            column![text(request)],
        ));
    } else {
        let activity_available =
            available && (page != Page::MemoryTrim || state != FeatureRunState::Unknown);
        body = body.push(section(
            "common.process_activity",
            column![
                metric(
                    if page == Page::Watchdog {
                        "watchdog.monitored"
                    } else {
                        "common.scanned_processes"
                    },
                    count(activity_available.then_some(summary.scanned).flatten())
                ),
                metric(
                    if page == Page::Watchdog {
                        "watchdog.alerting"
                    } else {
                        "common.adjusted_processes"
                    },
                    count(activity_available.then_some(summary.adjusted).flatten())
                ),
                metric(
                    "common.protected_or_denied_processes",
                    count(
                        activity_available
                            .then_some(summary.protected_or_denied)
                            .flatten()
                    )
                ),
                metric(
                    "common.skipped_processes",
                    count(activity_available.then_some(summary.skipped).flatten())
                )
            ]
            .spacing(design::space::CONTROL),
        ));
    }
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
                    actions.and_then(|s| s.last_success.as_ref())
                )
            ]
            .spacing(design::space::CONTROL),
        ))
        .push(section(
            "common.failed_actions",
            column![
                metric(
                    "common.failed_count",
                    count(available.then(|| actions.map_or(0, |s| s.failed_actions)))
                ),
                log("common.last_failed", last_failed)
            ]
            .spacing(design::space::CONTROL),
        ));
    if page == Page::AdaptiveEngine {
        use crate::adaptive_engine_process::ZoneStatus;
        let adaptive = &runtime.feature_status.adaptive_engine_process;
        let zones = &adaptive.zones;
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
        body = body.push(section(
            "adaptive_engine_process.dynamic_resource_zones",
            column![
                container(text(t!(key).to_string()).size(design::typography::CAPTION))
                    .padding([4, 8])
                    .style(move |theme| super::widgets::rule_status_chip(theme, color)),
                text(reason).size(design::typography::CAPTION),
                metric(
                    "adaptive_engine_process.zone_foreground_cpus",
                    zones.foreground_processors.to_string()
                ),
                metric(
                    "adaptive_engine_process.zone_background_cpus",
                    zones.background_processors.to_string()
                ),
                metric(
                    "adaptive_engine_process.zone_foreground_targets",
                    zones.foreground_targets.to_string()
                ),
                metric(
                    "adaptive_engine_process.zone_background_targets",
                    zones.background_targets.to_string()
                ),
            ]
            .spacing(design::space::CONTROL),
        ));
        if settings
            .adaptive_engine_process
            .limit_background_processors_enabled
        {
            body = body.push(section(
                "adaptive_engine_process.limit_background_processors",
                column![text(
                    t!(if adaptive.observation_paused {
                        "sidebar_status.paused"
                    } else if !adaptive.enabled {
                        "common.unknown"
                    } else if zones.status == ZoneStatus::Active {
                        "adaptive_engine_process.limit_waiting_zones"
                    } else if zones.background_limit_targets > 0 {
                        "common.applied"
                    } else {
                        "common.waiting"
                    })
                    .to_string()
                )
                .size(design::typography::CAPTION)],
            ));
        }
    }
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
            .spacing(design::space::CONTROL),
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
        body = body.push(
            action_button(t!("admin_rights.relaunch").to_string()).on_press(Message::RelaunchAdmin),
        );
    }
    let content: Element<'a, Message> = scrollable(
        container(body.width(Fill))
            .width(Fill)
            .padding([0, design::space::MEDIUM as u16]),
    )
    .width(Fill)
    .height(Fill)
    .into();
    Some(content)
}
pub(super) fn saved_runtime_settings(
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

struct InstanceLimitsStatus<'a> {
    state: FeatureRunState,
    enabled_rules: usize,
    incidents: usize,
    last_error: Option<&'a str>,
}

fn instance_limits_status<'a>(
    settings: &Settings,
    snapshot: &'a InstanceLimitsSnapshot,
) -> InstanceLimitsStatus<'a> {
    let mut enabled_rules = 0;
    let mut incidents = 0;
    let mut last_error = None;
    let mut observed = true;
    let active = settings.general.enabled && settings.instance_limits.enabled && snapshot.enabled;
    for rule in settings
        .instance_limits
        .rules
        .iter()
        .filter(|rule| rule.enabled)
    {
        enabled_rules += 1;
        let Some(report) = active
            .then(|| super::instance_limits::rule_status(snapshot, rule))
            .flatten()
        else {
            observed = false;
            continue;
        };
        observed &= super::instance_limits::sample_fresh(report.sampled_at)
            && matches!(report.count, CountObservation::Complete(_));
        incidents += usize::from(report.incident);
        if let NotificationStatus::Failed(error) = &report.notification {
            last_error.get_or_insert(error.as_str());
        }
    }
    let state = if enabled_rules == 0 {
        FeatureRunState::Waiting
    } else if observed {
        FeatureRunState::Running
    } else {
        FeatureRunState::Unknown
    };
    InstanceLimitsStatus {
        state,
        enabled_rules,
        incidents,
        last_error,
    }
}

fn sidebar_state(
    page: Page,
    settings: &Settings,
    runtime: &RuntimeStatusSnapshot,
    summary: &FeatureStatusSummary<'_>,
    instance_limits_state: Option<FeatureRunState>,
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
    if summary.last_error.is_some()
        && (page != Page::InstanceLimits || settings.instance_limits.notify)
    {
        return FeatureRunState::Error;
    }
    if let Some(state) = instance_limits_state {
        return state;
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
        Page::ByFocus => Some(plan.decision_state == Some(DecisionState::ByFocus)),
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
            || matches!(
                plan.decision_state,
                Some(DecisionState::PausedWhilePluggedIn | DecisionState::WaitingForObservation)
            )
        {
            return FeatureRunState::Paused;
        }
        if selected && plan.apply_failed {
            return FeatureRunState::Error;
        }
        return if selected
            && plan_applied
            && !plan.restoring_previous
            && plan.owner == Some(PowerPlanOwner::OrdinaryAutomation)
        {
            FeatureRunState::Running
        } else {
            FeatureRunState::Waiting
        };
    }
    let features = &runtime.feature_status;
    if page == Page::Watchdog
        && !features.watchdog.observation_known
        && settings.watchdog.rules.iter().any(|rule| rule.enabled)
    {
        return FeatureRunState::Unknown;
    }
    if page == Page::InstanceBalancing
        && (!features.instance_balancing.enabled || features.instance_balancing.detail.is_none())
    {
        return FeatureRunState::Unknown;
    }
    if page == Page::KeepRunning {
        use crate::features::advanced_controls::keep_running::KeepRunningState;
        let keep_running = &features.keep_running;
        return if !keep_running.observation_known {
            FeatureRunState::Unknown
        } else if keep_running
            .rules
            .iter()
            .any(|rule| matches!(rule.state, KeepRunningState::Paused(_)))
        {
            FeatureRunState::Paused
        } else if keep_running.rules.iter().any(|rule| {
            matches!(
                rule.state,
                KeepRunningState::Armed | KeepRunningState::Starting
            )
        }) {
            FeatureRunState::Running
        } else {
            FeatureRunState::Waiting
        };
    }
    if page == Page::SleepPrevention {
        let sleep = &features.sleep_prevention;
        return if !sleep.observation_known {
            FeatureRunState::Unknown
        } else if sleep.system_requested {
            FeatureRunState::Running
        } else if !sleep.suppressed_apps.is_empty() && sleep.requesting_apps.is_empty() {
            FeatureRunState::Paused
        } else {
            FeatureRunState::Waiting
        };
    }
    if (page == Page::BackgroundEfficiency && features.background_efficiency.unsupported)
        || (page == Page::AppSuspension && features.app_suspension.unsupported)
    {
        return FeatureRunState::Blocked;
    }
    let process_observation = match page {
        Page::BackgroundEfficiency => Some((
            features.background_efficiency.enabled,
            features.background_efficiency.observation_paused,
        )),
        Page::ProcessPriority => Some((
            features.process_priority.enabled,
            features.process_priority.observation_paused,
        )),
        Page::ThreadPriority => Some((
            features.thread_priority.enabled,
            features.thread_priority.observation_paused,
        )),
        Page::DynamicPriorityBoost => Some((
            features.dynamic_priority_boost.enabled,
            features.dynamic_priority_boost.observation_paused,
        )),
        Page::IoPriority => Some((
            features.io_priority.enabled,
            features.io_priority.observation_paused,
        )),
        Page::MemoryPriority => Some((
            features.memory_priority.enabled,
            features.memory_priority.observation_paused,
        )),
        _ => None,
    };
    if let Some((enabled, paused)) = process_observation {
        if paused {
            return FeatureRunState::Paused;
        }
        if !enabled {
            return FeatureRunState::Unknown;
        }
    }
    if page == Page::GpuPriority {
        if features.gpu_priority.observation_paused {
            return FeatureRunState::Paused;
        }
        if !features.gpu_priority.enabled {
            return FeatureRunState::Unknown;
        }
    }
    if page == Page::AppSuspension && features.app_suspension.status_unknown {
        return FeatureRunState::Unknown;
    }
    if (page == Page::CpuLimiter && features.cpu_limiter.observation_paused)
        || (page == Page::TimerResolution && features.timer_resolution.observation_paused)
        || (page == Page::CpuSetsSoft && features.cpu_sets_soft.observation_paused)
        || (page == Page::ProcessorAffinityHard
            && features.processor_affinity_hard.observation_paused)
    {
        return FeatureRunState::Paused;
    }
    if page == Page::MemoryTrim {
        match features.memory_trim.status {
            MemoryTrimStatus::Disabled | MemoryTrimStatus::AutomationDisabled => {
                return FeatureRunState::Unknown;
            }
            MemoryTrimStatus::ForegroundUnknown => return FeatureRunState::Paused,
            _ => {}
        }
    }
    let adaptive_power_applied = page == Page::AdaptiveEngine
        && plan.owner == Some(PowerPlanOwner::AdaptiveEngine)
        && plan_applied;
    if page == Page::AdaptiveEngine
        && !adaptive_power_applied
        && settings.adaptive_engine_process.has_enabled_policies()
    {
        let adaptive = &features.adaptive_engine_process;
        if adaptive.observation_paused {
            return FeatureRunState::Paused;
        }
        if !adaptive.enabled {
            return FeatureRunState::Unknown;
        }
    }
    let active = (page == Page::Watchdog && summary.scanned.is_some_and(|count| count > 0))
        || summary.adjusted.is_some_and(|count| count > 0)
        || (page == Page::TimerResolution && features.timer_resolution.requested_100ns.is_some())
        || adaptive_power_applied;
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
fn metric(label: &str, value: String) -> Element<'static, Message> {
    row![
        text(t!(label).to_string())
            .width(iced::Length::FillPortion(2))
            .size(design::typography::CAPTION),
        text(value)
            .width(iced::Length::FillPortion(1))
            .align_x(iced::Right)
            .size(design::typography::SECONDARY)
    ]
    .spacing(design::space::SMALL)
    .width(Fill)
    .into()
}
fn section<'a>(title: &str, body: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    column![
        super::widgets::heading(t!(title).to_string(), design::typography::SECONDARY),
        super::widgets::settings_card(body),
    ]
    .spacing(design::space::SMALL)
    .into()
}

fn log(label: &str, entry: Option<&ActionLogEntry>) -> Element<'static, Message> {
    let value = entry
        .map(|e| {
            format!(
                "[{}] {} - {}",
                action_log_time_label(e.timestamp_epoch_ms),
                action_log_process_label(e),
                e.reason
            )
        })
        .unwrap_or_else(|| t!("common.none").to_string());
    detail_row(label, value)
}

fn detail_row(label: &str, value: String) -> Element<'static, Message> {
    column![
        text(t!(label).to_string()).size(design::typography::CAPTION),
        text(value).width(Fill).size(design::typography::CAPTION)
    ]
    .width(Fill)
    .spacing(design::space::TIGHT)
    .into()
}
fn feature_status_summary(
    page: Page,
    runtime: &RuntimeStatusSnapshot,
) -> Option<FeatureStatusSummary<'_>> {
    let summary = match page {
        Page::AdaptiveEngine => {
            let status = &runtime.feature_status.adaptive_engine_process;
            let observed = status.enabled && !status.observation_paused;
            FeatureStatusSummary {
                scanned: observed.then_some(status.scanned_processes),
                adjusted: observed.then_some(status.adjusted_processes),
                protected_or_denied: None,
                skipped: observed.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::AdaptiveEngine,
            }
        }
        Page::BackgroundEfficiency => {
            let status = &runtime.feature_status.background_efficiency;
            let observed = status.enabled && !status.observation_paused;
            FeatureStatusSummary {
                scanned: observed.then_some(status.scanned_processes),
                adjusted: observed.then_some(status.throttled_processes),
                protected_or_denied: observed.then_some(status.access_denied_processes),
                skipped: observed.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::BackgroundEfficiency,
            }
        }
        Page::Watchdog => {
            let status = &runtime.feature_status.watchdog;
            let observed = status.observation_known;
            FeatureStatusSummary {
                scanned: observed.then_some(status.monitored_processes),
                adjusted: observed.then_some(status.alerting_processes),
                protected_or_denied: None,
                skipped: observed.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::Watchdog,
            }
        }
        Page::KeepRunning => FeatureStatusSummary {
            scanned: None,
            adjusted: None,
            protected_or_denied: None,
            skipped: None,
            last_error: runtime.feature_status.keep_running.last_error.as_deref(),
            action_log_feature: ActionLogFeature::KeepRunning,
        },
        Page::MemoryTrim => {
            let status = &runtime.feature_status.memory_trim;
            let activity_known = !matches!(
                status.status,
                MemoryTrimStatus::ForegroundUnknown | MemoryTrimStatus::Error(_)
            );
            FeatureStatusSummary {
                scanned: activity_known.then_some(status.scanned_processes),
                adjusted: activity_known.then_some(status.trimmed_processes),
                protected_or_denied: None,
                skipped: activity_known.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::MemoryTrim,
            }
        }
        Page::ProcessPriority => {
            let status = &runtime.feature_status.process_priority;
            let observed = status.enabled && !status.observation_paused;
            FeatureStatusSummary {
                scanned: observed.then_some(status.scanned_processes),
                adjusted: observed.then_some(status.adjusted_processes),
                protected_or_denied: None,
                skipped: observed.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::ProcessPriority,
            }
        }
        Page::ThreadPriority => {
            let status = &runtime.feature_status.thread_priority;
            let observed = status.enabled && !status.observation_paused;
            FeatureStatusSummary {
                scanned: observed.then_some(status.scanned_processes),
                adjusted: observed.then_some(status.adjusted_processes),
                protected_or_denied: None,
                skipped: observed.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::ThreadPriority,
            }
        }
        Page::DynamicPriorityBoost => {
            let status = &runtime.feature_status.dynamic_priority_boost;
            let observed = status.enabled && !status.observation_paused;
            FeatureStatusSummary {
                scanned: observed.then_some(status.scanned_processes),
                adjusted: observed.then_some(status.adjusted_processes),
                protected_or_denied: None,
                skipped: observed.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::DynamicPriorityBoost,
            }
        }
        Page::IoPriority => {
            let status = &runtime.feature_status.io_priority;
            let observed = status.enabled && !status.observation_paused;
            FeatureStatusSummary {
                scanned: observed.then_some(status.scanned_processes),
                adjusted: observed.then_some(status.adjusted_processes),
                protected_or_denied: None,
                skipped: observed.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::IoPriority,
            }
        }
        Page::GpuPriority => {
            let status = &runtime.feature_status.gpu_priority;
            let observed = status.enabled && !status.observation_paused;
            FeatureStatusSummary {
                scanned: observed.then_some(status.scanned_processes),
                adjusted: observed.then_some(status.adjusted_processes),
                protected_or_denied: observed.then_some(status.denied_processes),
                skipped: observed.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::GpuPriority,
            }
        }
        Page::MemoryPriority => {
            let status = &runtime.feature_status.memory_priority;
            let observed = status.enabled && !status.observation_paused;
            FeatureStatusSummary {
                scanned: None,
                adjusted: observed.then_some(status.adjusted_processes),
                protected_or_denied: None,
                skipped: observed.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::MemoryPriority,
            }
        }
        Page::CpuLimiter => {
            let status = &runtime.feature_status.cpu_limiter;
            let observed = !status.observation_paused;
            FeatureStatusSummary {
                scanned: observed.then_some(status.scanned_processes),
                adjusted: observed.then_some(status.limited_processes),
                protected_or_denied: None,
                skipped: observed.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::CpuLimiter,
            }
        }
        Page::CpuSetsSoft | Page::ProcessorAffinityHard => {
            let (status, feature) = match page {
                Page::CpuSetsSoft => (
                    &runtime.feature_status.cpu_sets_soft,
                    ActionLogFeature::CpuSetsSoft,
                ),
                _ => (
                    &runtime.feature_status.processor_affinity_hard,
                    ActionLogFeature::ProcessorAffinityHard,
                ),
            };
            let observed = !status.observation_paused;
            FeatureStatusSummary {
                scanned: observed.then_some(status.scanned_processes),
                adjusted: observed.then_some(status.adjusted_processes),
                protected_or_denied: None,
                skipped: observed.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: feature,
            }
        }
        Page::InstanceBalancing => {
            let status = &runtime.feature_status.instance_balancing;
            let observed = status.enabled && status.process_activity_known;
            let adjusted_known = observed || (status.enabled && status.adjusted_processes > 0);
            FeatureStatusSummary {
                scanned: observed.then_some(status.scanned_processes),
                adjusted: adjusted_known.then_some(status.adjusted_processes),
                protected_or_denied: None,
                skipped: observed.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::InstanceBalancing,
            }
        }
        Page::AppSuspension => {
            let status = &runtime.feature_status.app_suspension;
            FeatureStatusSummary {
                scanned: None,
                adjusted: Some(status.suspended_processes),
                protected_or_denied: None,
                skipped: Some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::AppSuspension,
            }
        }
        Page::SleepPrevention => {
            let status = &runtime.feature_status.sleep_prevention;
            FeatureStatusSummary {
                scanned: status.observation_known.then_some(status.matched_processes),
                adjusted: None,
                protected_or_denied: None,
                skipped: None,
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::SleepPrevention,
            }
        }
        Page::TimerResolution => {
            let status = &runtime.feature_status.timer_resolution;
            FeatureStatusSummary {
                scanned: None,
                adjusted: None,
                protected_or_denied: None,
                skipped: None,
                last_error: status.last_error.as_deref(),
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
    fn instance_limits_status_ignores_reports_for_superseded_rules() {
        use crate::config::InstanceLimitsRule;
        use crate::features::advanced_controls::instance_limits::InstanceLimitsRuleSnapshot;

        let mut settings = Settings::default();
        settings.general.enabled = true;
        settings.instance_limits.enabled = true;
        let rule = InstanceLimitsRule::new(r"C:\Apps\app.exe".into());
        let snapshot = InstanceLimitsSnapshot {
            enabled: true,
            rules: vec![InstanceLimitsRuleSnapshot {
                executable_path: rule.executable_path.clone(),
                max_instances: rule.max_instances,
                enabled: true,
                count: CountObservation::Complete(2),
                sampled_at: Some(std::time::Instant::now()),
                incident: true,
                notification: NotificationStatus::Failed("notification failed".into()),
            }],
        };
        settings.instance_limits.rules.push(rule);
        let status = instance_limits_status(&settings, &snapshot);
        assert_eq!(status.state, FeatureRunState::Running);
        assert_eq!(status.incidents, 1);
        assert_eq!(status.last_error, Some("notification failed"));

        settings.instance_limits.rules[0].max_instances = 2;
        let status = instance_limits_status(&settings, &snapshot);
        assert_eq!(status.state, FeatureRunState::Unknown);
        assert_eq!(status.enabled_rules, 1);
        assert_eq!(status.incidents, 0);
        assert_eq!(status.last_error, None);
    }

    #[test]
    fn watchdog_counts_require_a_completed_observation() {
        use crate::{
            config::WatchdogRule, features::winderust_features::watchdog::WatchdogManager,
        };

        let mut settings = Settings::default();
        settings.general.enabled = true;
        settings.watchdog.enabled = true;
        settings.watchdog.rules = vec![WatchdogRule::new(r"C:\Apps\worker.exe".into())];
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            ..Default::default()
        };
        let status = |runtime: &RuntimeStatusSnapshot| {
            let summary = feature_status_summary(Page::Watchdog, runtime).unwrap();
            (
                sidebar_state(Page::Watchdog, &settings, runtime, &summary, None),
                [summary.scanned, summary.adjusted, summary.skipped],
            )
        };
        assert_eq!(status(&runtime), (FeatureRunState::Unknown, [None; 3]));

        let (sample, events) = WatchdogManager::default().update(
            &settings.watchdog,
            true,
            &mut crate::runtime::observations::CycleObservations::with_processes(Vec::new()),
        );
        assert!(events.is_empty());
        assert_ne!(sample, Default::default());
        std::sync::Arc::make_mut(&mut runtime.feature_status).watchdog = sample;
        assert_eq!(status(&runtime), (FeatureRunState::Waiting, [Some(0); 3]));
    }

    #[test]
    fn sidebar_shows_a_current_error_alongside_an_older_failed_action() {
        use crate::action_log::{ActionLog, ActionLogResult};
        use iced::advanced::{
            layout,
            widget::{Id, Operation, Tree},
            Layout,
        };

        #[derive(Default)]
        struct Details(Vec<String>);

        impl Operation for Details {
            fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
                visit(self);
            }

            fn text(&mut self, _: Option<&Id>, _: iced::Rectangle, content: &str) {
                self.0.push(content.to_owned());
            }
        }

        let mut settings = Settings::default();
        settings.general.enabled = true;
        settings.io_priority.enabled = true;
        let mut log = ActionLog::new(4);
        log.record(
            ActionLogFeature::IoPriority,
            Some(42),
            "app.exe",
            ActionLogResult::Failed,
            "older apply failure",
        );
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            action_log_summaries: log.summaries().into(),
            ..Default::default()
        };
        let features = std::sync::Arc::make_mut(&mut runtime.feature_status);
        features.io_priority.last_error = Some("current restore failure".to_owned());

        let mut rail = view(Page::IoPriority, &settings, &runtime, &[]).unwrap();
        let renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let mut tree = Tree::new(&rail);
        let size = iced::Size::new(216.0, 560.0);
        let node =
            rail.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut details = Details::default();
        rail.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut details);
        assert!(details
            .0
            .iter()
            .any(|text| text == "current restore failure"));
        assert!(details
            .0
            .iter()
            .any(|text| text.ends_with(" - older apply failure")));
    }

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
            sidebar_state(Page::ProcessPriority, settings, runtime, &summary, None)
        };
        assert_eq!(state(&settings, &runtime), FeatureRunState::Unknown);
        runtime.generation = 1;
        assert_eq!(state(&settings, &runtime), FeatureRunState::Unknown);
        std::sync::Arc::make_mut(&mut runtime.feature_status)
            .process_priority
            .enabled = true;
        assert_eq!(state(&settings, &runtime), FeatureRunState::Waiting);
        std::sync::Arc::make_mut(&mut runtime.feature_status)
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
    fn cpu_control_sidebar_preserves_pause_and_restore_errors() {
        let mut settings = Settings::default();
        settings.general.enabled = true;
        settings.cpu_limiter.enabled = true;
        settings.cpu_sets_soft.enabled = true;
        settings.processor_affinity_hard.enabled = true;
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            ..Default::default()
        };
        let activity = |runtime: &RuntimeStatusSnapshot| {
            [
                Page::CpuLimiter,
                Page::CpuSetsSoft,
                Page::ProcessorAffinityHard,
            ]
            .map(|page| {
                let summary = feature_status_summary(page, runtime).unwrap();
                (
                    sidebar_state(page, &settings, runtime, &summary, None),
                    [summary.scanned, summary.adjusted, summary.skipped],
                )
            })
        };
        assert_eq!(
            activity(&runtime),
            [(FeatureRunState::Waiting, [Some(0); 3]); 3]
        );

        let features = std::sync::Arc::make_mut(&mut runtime.feature_status);
        features.cpu_limiter.observation_paused = true;
        features.cpu_sets_soft.observation_paused = true;
        features.processor_affinity_hard.observation_paused = true;
        assert_eq!(
            activity(&runtime),
            [(FeatureRunState::Paused, [None; 3]); 3]
        );

        let features = std::sync::Arc::make_mut(&mut runtime.feature_status);
        features.cpu_limiter.last_error = Some("restore failed".into());
        features.cpu_sets_soft.last_error = Some("restore failed".into());
        features.processor_affinity_hard.last_error = Some("restore failed".into());
        assert_eq!(activity(&runtime), [(FeatureRunState::Error, [None; 3]); 3]);
    }

    #[test]
    fn instance_balancing_sidebar_waits_for_accepted_runtime_work() {
        let mut settings = Settings::default();
        settings.general.enabled = true;
        settings.instance_balancing.enabled = true;
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            ..Default::default()
        };
        let state = |settings: &Settings, runtime: &RuntimeStatusSnapshot| {
            let summary = feature_status_summary(Page::InstanceBalancing, runtime).unwrap();
            assert_eq!(
                summary.action_log_feature,
                ActionLogFeature::InstanceBalancing
            );
            sidebar_state(Page::InstanceBalancing, settings, runtime, &summary, None)
        };
        assert_eq!(state(&settings, &runtime), FeatureRunState::Unknown);
        let pending = feature_status_summary(Page::InstanceBalancing, &runtime).unwrap();
        assert_eq!(
            (pending.scanned, pending.adjusted, pending.skipped),
            (None, None, None)
        );
        let balancing =
            &mut std::sync::Arc::make_mut(&mut runtime.feature_status).instance_balancing;
        balancing.enabled = true;
        balancing.detail = Some(crate::automation::InstanceBalancingDetail::WaitingForStableCohort);
        assert_eq!(state(&settings, &runtime), FeatureRunState::Waiting);
        assert_eq!(
            feature_status_summary(Page::InstanceBalancing, &runtime)
                .unwrap()
                .scanned,
            None
        );
        std::sync::Arc::make_mut(&mut runtime.feature_status)
            .instance_balancing
            .process_activity_known = true;
        assert_eq!(
            feature_status_summary(Page::InstanceBalancing, &runtime)
                .unwrap()
                .adjusted,
            Some(0)
        );
        std::sync::Arc::make_mut(&mut runtime.feature_status)
            .instance_balancing
            .adjusted_processes = 2;
        assert_eq!(state(&settings, &runtime), FeatureRunState::Running);
        std::sync::Arc::make_mut(&mut runtime.feature_status)
            .instance_balancing
            .last_error = Some("apply failed".into());
        assert_eq!(state(&settings, &runtime), FeatureRunState::Error);
        settings.instance_balancing.enabled = false;
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
            sidebar_state(Page::ByTime, &settings, runtime, &summary, None)
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
        std::sync::Arc::make_mut(&mut runtime.power_plan_status).restoring_previous = true;
        assert_eq!(state(&runtime), FeatureRunState::Waiting);
        std::sync::Arc::make_mut(&mut runtime.power_plan_status).apply_failed = true;
        assert_eq!(state(&runtime), FeatureRunState::Error);
        let plan = std::sync::Arc::make_mut(&mut runtime.power_plan_status);
        plan.apply_failed = false;
        plan.restoring_previous = false;
        plan.owner = Some(PowerPlanOwner::AdaptiveEngine);
        assert_eq!(state(&runtime), FeatureRunState::Paused);
    }

    #[test]
    fn unavailable_metrics_are_not_reported_as_zero() {
        assert_eq!(count(None), "\u{2014}");
        assert_eq!(count(Some(0)), "0");
        let mut runtime = RuntimeStatusSnapshot::default();
        let summary = feature_status_summary(Page::MemoryPriority, &runtime).unwrap();
        assert_eq!(summary.scanned, None);
        assert_eq!(summary.adjusted, None);
        assert_eq!(summary.protected_or_denied, None);
        assert_eq!(summary.skipped, None);

        for (status, expected) in [
            (MemoryTrimStatus::ForegroundUnknown, None),
            (MemoryTrimStatus::Error("query failed".into()), None),
            (MemoryTrimStatus::Active, Some(0)),
        ] {
            std::sync::Arc::make_mut(&mut runtime.feature_status)
                .memory_trim
                .status = status;
            let summary = feature_status_summary(Page::MemoryTrim, &runtime).unwrap();
            assert_eq!(summary.scanned, expected);
            assert_eq!(summary.adjusted, expected);
            assert_eq!(summary.skipped, expected);
        }
    }
}
