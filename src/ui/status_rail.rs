use super::action_log::{action_log_process_label, action_log_time_label};
use super::widgets::{action_button, power_plan_label};
use super::{design, power_plan_control};
use crate::ui::scrolling::scrollable;
use crate::{
    action_log::{ActionLogEntry, ActionLogFeature},
    adaptive_engine_process::AdaptiveEngineProcessSnapshot,
    automation::RuntimeStatusSnapshot,
    config::{PowerSourceProfile, Settings},
    control::{
        instance_limits::CountObservation, power_plan::PowerPlanOwner,
        priority_efficiency::AdaptiveRestraintState,
    },
    features::watchdog::{
        app_suspension::AppSuspensionStatus,
        instance_limits::{InstanceLimitsSnapshot, NotificationStatus},
        memory_trim::MemoryTrimStatus,
        sleep_prevention::SleepPreventionSnapshot,
    },
    power::PowerPlan,
    rules::DecisionState,
    ui::Page,
};
use iced::widget::{column, container, row, text};
use iced::{Element, Fill};
use rust_i18n::t;
use std::borrow::Cow;

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

impl FeatureRunState {
    fn status_keys(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Running => (
                "common.running",
                "sidebar_status.running_help",
                "common.applied",
            ),
            Self::Waiting => (
                "common.waiting",
                "sidebar_status.waiting_help",
                "common.waiting",
            ),
            Self::Paused => (
                "sidebar_status.paused",
                "sidebar_status.paused_help",
                "common.waiting",
            ),
            Self::Blocked => (
                "sidebar_status.blocked",
                "sidebar_status.blocked_help",
                "common.error",
            ),
            Self::Error => ("common.error", "sidebar_status.error_help", "common.error"),
            Self::Inactive => (
                "common.inactive",
                "sidebar_status.inactive_help",
                "common.inactive",
            ),
            Self::Unknown => (
                "common.unknown",
                "sidebar_status.unknown_help",
                "common.unknown",
            ),
        }
    }
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
    plugged_in: Option<bool>,
) -> Option<Element<'a, Message>> {
    if page == Page::PowerPlanControl {
        return Some(power_plan_control::status_view(
            &runtime.power_plan_status,
            plans,
        ));
    }
    if page == Page::AdaptiveProcessorPower {
        return Some(adaptive_processor_power_view(settings, runtime, plans));
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
    let current_source = plugged_in.map(|plugged_in| {
        if plugged_in {
            PowerSourceProfile::PluggedIn
        } else {
            PowerSourceProfile::OnBattery
        }
    });
    let sleep = (page == Page::SleepPrevention)
        .then_some(&runtime.feature_status.sleep_prevention)
        .filter(|snapshot| current_source.is_none_or(|source| snapshot.source == Some(source)));
    let adaptive = (page == Page::AdaptiveEngine)
        .then_some(&runtime.feature_status.adaptive_engine_process)
        .filter(|snapshot| {
            if snapshot.waiting_for_power_source {
                current_source.is_none() || snapshot.last_error.is_some()
            } else {
                snapshot.source.is_some()
                    && current_source.is_none_or(|source| snapshot.source == Some(source))
            }
        });
    let summary = if let Some(limits) = &instance_limits {
        FeatureStatusSummary {
            scanned: None,
            adjusted: None,
            protected_or_denied: None,
            skipped: None,
            last_error: limits.last_error,
            action_log_feature: ActionLogFeature::InstanceLimits,
        }
    } else if page == Page::SleepPrevention {
        FeatureStatusSummary {
            scanned: None,
            adjusted: None,
            protected_or_denied: None,
            skipped: None,
            last_error: sleep.and_then(|snapshot| snapshot.last_error.as_deref()),
            action_log_feature: ActionLogFeature::SleepPrevention,
        }
    } else if page == Page::AdaptiveEngine {
        let observed = adaptive.filter(|status| status.enabled && !status.observation_paused);
        FeatureStatusSummary {
            scanned: observed.map(|status| status.scanned_processes),
            adjusted: observed.map(|status| status.adjusted_processes),
            protected_or_denied: None,
            skipped: observed.map(|status| status.skipped_processes),
            last_error: adaptive.and_then(|status| status.last_error.as_deref()),
            action_log_feature: ActionLogFeature::AdaptiveEngine,
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
    let observed_state = match page {
        Page::SleepPrevention => Some(sleep_prevention_state(sleep)),
        Page::AdaptiveEngine if settings.adaptive_engine_process.has_enabled_policies() => {
            match adaptive {
                Some(status) if status.observation_paused => Some(FeatureRunState::Paused),
                Some(status) if status.enabled => None,
                _ => Some(FeatureRunState::Unknown),
            }
        }
        _ => instance_limits.as_ref().map(|limits| limits.state),
    };
    let state = sidebar_state(page, settings, runtime, &summary, observed_state);
    let (state_label, detail, color) = match (page, state) {
        (_, FeatureRunState::Paused) if power_feature.is_some() => {
            let plan = &runtime.power_plan_status;
            let (label, fallback, color) = state.status_keys();
            let detail = match (plan.owner, plan.decision_state) {
                (Some(PowerPlanOwner::AdaptiveProcessorPower), _)
                    if power_plan_control::managed_by_adaptive_processor_power(plan, plans) =>
                {
                    "power_plan_control.switch_managed_active"
                }
                (Some(PowerPlanOwner::AdaptiveProcessorPower), _) => fallback,
                (_, Some(DecisionState::PausedWhilePluggedIn)) => "power_plan_control.paused_on_ac",
                (_, Some(DecisionState::WaitingForObservation)) => {
                    "power_plan_control.waiting_for_observation"
                }
                _ => fallback,
            };
            (label, detail, color)
        }
        (_, FeatureRunState::Paused) if waiting_for_power_source(page, runtime) => (
            "sidebar_status.paused",
            "home.power_source_unavailable",
            "common.waiting",
        ),
        (Page::CpuLimiter, FeatureRunState::Paused)
            if runtime.feature_status.cpu_limiter.waiting_for_power_source =>
        {
            (
                "sidebar_status.paused",
                "cpu_limiter.waiting_for_power_source",
                "common.waiting",
            )
        }
        (Page::AppSuspension, FeatureRunState::Paused)
            if runtime.feature_status.app_suspension.status
                == AppSuspensionStatus::PowerSourceUnknown =>
        {
            (
                "sidebar_status.paused",
                "app_suspension.status.power_source_unknown",
                "common.waiting",
            )
        }
        (Page::SleepPrevention, FeatureRunState::Running)
            if sleep.is_some_and(|status| status.waiting_for_power_source) =>
        {
            (
                "sleep_prevention.system_request_accepted",
                "sleep_prevention.power_source_unavailable",
                "common.applied",
            )
        }
        (Page::SleepPrevention, FeatureRunState::Waiting)
            if sleep.is_some_and(|status| status.waiting_for_power_source) =>
        {
            (
                "common.waiting",
                "sleep_prevention.power_source_unavailable",
                "common.waiting",
            )
        }
        (Page::SleepPrevention, FeatureRunState::Running) => (
            "sleep_prevention.request_accepted",
            "sleep_prevention.request_status_help",
            "common.applied",
        ),
        (Page::SleepPrevention, FeatureRunState::Paused) => (
            "sleep_prevention.requests_paused",
            "sleep_prevention.paused_help",
            "common.waiting",
        ),
        (Page::InstanceLimits, FeatureRunState::Running) => (
            "instance_limits.monitoring",
            "instance_limits.intro_1",
            "common.applied",
        ),
        _ => state.status_keys(),
    };
    let detail = if page == Page::InstanceBalancing
        && available
        && !matches!(state, FeatureRunState::Inactive | FeatureRunState::Unknown)
        && runtime.worker_error.is_none()
    {
        runtime.feature_status.instance_balancing.message()
    } else if page == Page::MemoryTrim {
        let trim = &runtime.feature_status.memory_trim;
        match (&trim.status, trim.memory_load_percent) {
            (
                MemoryTrimStatus::WaitingForPowerSource {
                    manual_trim_blocked,
                },
                _,
            ) if state == FeatureRunState::Paused => Some(
                t!(if *manual_trim_blocked {
                    "memory_trim.waiting_for_power_source"
                } else {
                    "memory_trim.automatic_waiting_for_power_source"
                })
                .into_owned(),
            ),
            (MemoryTrimStatus::WaitingForMemoryLoad { threshold_percent }, Some(load_percent))
                if state == FeatureRunState::Waiting =>
            {
                Some(
                    t!(
                        "memory_trim.waiting_for_memory_load",
                        load = load_percent,
                        threshold = threshold_percent
                    )
                    .into_owned(),
                )
            }
            _ => None,
        }
    } else {
        None
    }
    .unwrap_or_else(|| t!(detail).into_owned());
    let mut body = status_body(state_label, detail, color);
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
        let plan = power_plan_control::current_plan_guid(&runtime.power_plan_status, plans)
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
        let sleep = sleep.filter(|_| available);
        let observed = sleep.filter(|snapshot| snapshot.observation_known);
        let accepted = |requested| {
            t!(match requested {
                Some(true) => "sleep_prevention.request_accepted",
                Some(false) => "sleep_prevention.not_requested",
                None => "common.unknown",
            })
            .into_owned()
        };
        body = body.push(section(
            "sleep_prevention.requests",
            column![
                metric(
                    "sleep_prevention.matching_rules",
                    count(observed.map(|snapshot| snapshot.matched_rules))
                ),
                metric(
                    "sleep_prevention.matching_processes",
                    count(observed.map(|snapshot| snapshot.matched_processes))
                ),
                metric(
                    "sleep_prevention.system_request",
                    accepted(sleep.map(|snapshot| snapshot.system_requested))
                ),
                metric(
                    "sleep_prevention.display_request",
                    accepted(sleep.map(|snapshot| snapshot.display_requested))
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
        let shadowed =
            (page == Page::ProcessPriority && activity_available && summary.scanned.is_some())
                .then_some(
                    runtime
                        .feature_status
                        .process_priority
                        .efficiency_mode_shadowed_processes,
                )
                .filter(|&count| count != 0);
        body = body.push(section(
            "common.process_activity",
            column![
                metric(
                    if page == Page::ResourceThresholds {
                        "resource_thresholds.monitored"
                    } else {
                        "common.scanned_processes"
                    },
                    count(activity_available.then_some(summary.scanned).flatten())
                ),
                metric(
                    if page == Page::ResourceThresholds {
                        "resource_thresholds.alerting"
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
            .extend(shadowed.map(|count| {
                metric(
                    "process_priority.shadowed_by_efficiency_mode",
                    count.to_string(),
                )
            }))
            .spacing(design::space::CONTROL),
        ));
    }
    if page == Page::AdaptiveEngine {
        let configured = settings.general.enabled
            && settings.adaptive_engine.enabled
            && settings
                .adaptive_engine_process
                .cpu_pressure_restraint_enabled;
        let verified_available = available
            && configured
            && adaptive.is_some_and(|status| status.enabled && !status.observation_paused);
        body = body.extend(adaptive_restraint_section(
            &runtime.feature_status.adaptive_engine_process,
            configured,
            verified_available,
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
        let enabled = settings.general.enabled && settings.adaptive_engine.enabled;
        let zones_enabled = enabled
            && settings
                .adaptive_engine_process
                .dynamic_resource_zones_enabled;
        let zones = adaptive.map(|status| &status.zones);
        let waits_for_source =
            zones_enabled && adaptive.is_some_and(|status| status.waiting_for_power_source);
        let (key, color) = match zones.map(|zones| zones.status) {
            _ if waits_for_source => ("sidebar_status.paused", "common.waiting"),
            None if zones_enabled => ("common.unknown", "common.unknown"),
            None | Some(ZoneStatus::Disabled) => ("common.inactive", "common.inactive"),
            Some(ZoneStatus::Waiting) => ("common.waiting", "common.waiting"),
            Some(ZoneStatus::Applying) => {
                ("adaptive_engine_process.zone_applying", "common.waiting")
            }
            Some(ZoneStatus::Active) => ("common.applied", "common.applied"),
            Some(ZoneStatus::Overridden) => {
                ("adaptive_engine_process.zone_overridden", "common.waiting")
            }
            Some(ZoneStatus::Degraded) => ("adaptive_engine_process.zone_degraded", "common.error"),
            Some(ZoneStatus::Unavailable) => ("common.unavailable", "common.unknown"),
        };
        let reason = zones
            .filter(|zones| !zones.reason.is_empty())
            .map(|zones| {
                let key = format!("adaptive_engine_process.{}", zones.reason);
                t!(&key).into_owned()
            })
            .unwrap_or_default();
        body = body.push(section(
            "adaptive_engine_process.dynamic_resource_zones",
            column![
                container(text(t!(key).to_string()).size(design::typography::CAPTION))
                    .padding([4, 8])
                    .style(move |theme| super::widgets::rule_status_chip(theme, color)),
                text(reason).size(design::typography::CAPTION),
                metric(
                    "adaptive_engine_process.zone_foreground_cpus",
                    count(zones.map(|zones| zones.foreground_processors as usize))
                ),
                metric(
                    "adaptive_engine_process.zone_background_cpus",
                    count(zones.map(|zones| zones.background_processors as usize))
                ),
                metric(
                    "adaptive_engine_process.zone_foreground_targets",
                    count(zones.map(|zones| zones.foreground_targets))
                ),
                metric(
                    "adaptive_engine_process.zone_background_targets",
                    count(zones.map(|zones| zones.background_targets))
                ),
            ]
            .spacing(design::space::CONTROL),
        ));
        if settings
            .adaptive_engine_process
            .limit_background_processors_enabled
        {
            let key = match adaptive.filter(|status| status.enabled) {
                Some(status) if status.observation_paused => "sidebar_status.paused",
                Some(status) if status.zones.status == ZoneStatus::Active => {
                    "adaptive_engine_process.limit_waiting_zones"
                }
                Some(status) if status.zones.background_limit_targets > 0 => "common.applied",
                Some(_) => "common.waiting",
                _ if enabled => "common.unknown",
                _ => "common.inactive",
            };
            body = body.push(section(
                "adaptive_engine_process.limit_background_processors",
                column![text(t!(key).to_string()).size(design::typography::CAPTION)],
            ));
        }
    }
    if page == Page::AdaptiveEngine {
        use crate::bottleneck_classifier::BottleneckState;
        let enabled = settings.general.enabled && settings.adaptive_engine.enabled;
        let status = enabled
            .then_some(&runtime.feature_status.bottleneck_classifier)
            .filter(|status| status.enabled);
        let error = status.and_then(|status| status.last_error.as_deref());
        let key = if !enabled {
            "common.inactive"
        } else if error.is_some() {
            "common.unavailable"
        } else {
            match status.map(|status| status.state).unwrap_or_default() {
                BottleneckState::Unknown => "adaptive_engine.bottleneck_unknown",
                BottleneckState::Headroom => "adaptive_engine.bottleneck_headroom",
                BottleneckState::CpuBound => "adaptive_engine.bottleneck_cpu_bound",
                BottleneckState::GpuBound => "adaptive_engine.bottleneck_gpu_bound",
                BottleneckState::Mixed => "adaptive_engine.bottleneck_mixed",
            }
        };
        let mut readings = column![
            metric("common.status", t!(key).to_string()),
            metric(
                "adaptive_engine.total_cpu",
                percent(status.and_then(|status| status.total_cpu_tenths))
            ),
            metric(
                "adaptive_engine.busiest_cpu",
                percent(status.and_then(|status| status.busiest_cpu_tenths))
            ),
            metric(
                "adaptive_engine.busiest_gpu",
                percent(status.and_then(|status| status.busiest_gpu_tenths))
            )
        ]
        .spacing(design::space::CONTROL);
        if let Some(error) = error {
            readings = readings.push(detail_row("sidebar_status.current_error", error.to_owned()));
        }
        body = body.push(section("adaptive_engine.bottleneck_classifier", readings));
    }
    if page == Page::EfficiencyMode
        && runtime
            .feature_status
            .efficiency_mode
            .access_denied_processes
            > 0
        && !crate::privilege::is_running_as_admin()
    {
        body = body.push(
            action_button(t!("admin_rights.relaunch").to_string()).on_press(Message::RelaunchAdmin),
        );
    }
    Some(scroll_status(body))
}

fn adaptive_restraint_section(
    snapshot: &AdaptiveEngineProcessSnapshot,
    configured: bool,
    verified_available: bool,
) -> Option<Element<'static, Message>> {
    let mut rows = snapshot
        .restraint_processes
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|row| row.state != AdaptiveRestraintState::Verified || verified_available)
        .peekable();
    let mut details = column![].spacing(design::space::CONTROL);
    if rows.peek().is_none() {
        if !configured {
            return None;
        }
        let key = if verified_available {
            "adaptive_engine_process.restraint_none"
        } else {
            "common.unknown"
        };
        details = details.push(text(t!(key).into_owned()).size(design::typography::CAPTION));
    }
    for row in rows {
        let key = match row.state {
            AdaptiveRestraintState::Verified => "adaptive_engine_process.restraint_verified",
            AdaptiveRestraintState::RestorationPending => {
                "adaptive_engine_process.restraint_restoration_pending"
            }
            AdaptiveRestraintState::ConfirmationPending => {
                "adaptive_engine_process.restraint_confirmation_pending"
            }
        };
        details = details.push(column![
            text(format!("{} ({})", row.process.name, row.process.id))
                .size(design::typography::SECONDARY)
                .wrapping(text::Wrapping::WordOrGlyph),
            text(t!(key).into_owned())
                .size(design::typography::CAPTION)
                .style(text::secondary)
        ]);
    }
    Some(section("adaptive_engine.cpu_pressure", details))
}

fn adaptive_processor_power_view(
    settings: &Settings,
    runtime: &RuntimeStatusSnapshot,
    plans: &[PowerPlan],
) -> Element<'static, Message> {
    let enabled =
        settings.general.enabled && settings.adaptive_engine.adaptive_processor_power_enabled;
    let (state, error) = adaptive_processor_power_state(enabled, runtime, plans);
    let plan_status = &runtime.power_plan_status;
    let (label, mut detail, color) = state.status_keys();
    if state == FeatureRunState::Waiting {
        if plan_status.adaptive_processor_power_waiting_for_source {
            detail = "adaptive_processor_power.waiting_for_power_source";
        } else if !enabled {
            detail = "adaptive_processor_power.releasing_plan";
        }
    }
    let mut body = status_body(label, t!(detail).into_owned(), color);
    if let Some(error) = error {
        body = body.push(detail_row(
            "sidebar_status.current_error",
            error.into_owned(),
        ));
    }
    let plan = power_plan_control::current_plan_name(plan_status, plans);
    body = body.push(section(
        "common.current_power_plan",
        column![text(plan).wrapping(text::Wrapping::WordOrGlyph)],
    ));
    scroll_status(body)
}

fn adaptive_processor_power_state<'a>(
    enabled: bool,
    runtime: &'a RuntimeStatusSnapshot,
    plans: &[PowerPlan],
) -> (FeatureRunState, Option<Cow<'a, str>>) {
    let plan = &runtime.power_plan_status;
    let managed = power_plan_control::managed_by_adaptive_processor_power(plan, plans);
    let waiting_for_source = plan.adaptive_processor_power_waiting_for_source;
    let error = plan
        .adaptive_processor_power_error
        .as_ref()
        .map(power_plan_control::adaptive_error_message)
        .or_else(|| {
            plan.active_plan_read_error
                .as_deref()
                .filter(|_| managed || (enabled && !waiting_for_source))
                .map(Cow::Borrowed)
        });
    let state = if error.is_some() {
        FeatureRunState::Error
    } else if waiting_for_source {
        FeatureRunState::Waiting
    } else if !enabled {
        if managed {
            FeatureRunState::Waiting
        } else {
            FeatureRunState::Inactive
        }
    } else if managed {
        FeatureRunState::Running
    } else if runtime.generation == 0
        || power_plan_control::current_plan_guid(plan, plans).is_none()
    {
        FeatureRunState::Unknown
    } else {
        FeatureRunState::Waiting
    };
    (state, error)
}

fn status_body(
    label: &str,
    detail: String,
    color: &'static str,
) -> iced::widget::Column<'static, Message> {
    column![
        container(text(t!(label).to_string()).size(design::typography::SECONDARY))
            .padding([4, 8])
            .style(move |theme| super::widgets::rule_status_chip(theme, color)),
        text(detail)
            .size(design::typography::CAPTION)
            .style(text::secondary)
    ]
    .spacing(design::space::MEDIUM)
}

fn scroll_status(body: iced::widget::Column<'_, Message>) -> Element<'_, Message> {
    scrollable(
        container(body.width(Fill))
            .width(Fill)
            .padding([0, design::space::MEDIUM as u16]),
    )
    .width(Fill)
    .height(Fill)
    .into()
}
pub(super) fn saved_rail_settings<'a>(
    editor: &'a crate::application::SettingsEditor,
    page: Page,
    runtime: &RuntimeStatusSnapshot,
    plugged_in: Option<bool>,
) -> &'a Settings {
    let plugged_in = plugged_in.or_else(|| {
        let features = &runtime.feature_status;
        let source = match page {
            Page::AdaptiveEngine => features.adaptive_engine_process.source,
            Page::SleepPrevention => features.sleep_prevention.source,
            _ => None,
        };
        source.map(|source| source == PowerSourceProfile::PluggedIn)
    });
    saved_runtime_settings(editor, plugged_in)
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
        let NotificationStatus::Failed(error) = &report.notification else {
            continue;
        };
        if settings.instance_limits.notify && rule.notify {
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

fn sleep_prevention_state(sleep: Option<&SleepPreventionSnapshot>) -> FeatureRunState {
    let Some(sleep) = sleep.filter(|snapshot| snapshot.observation_known) else {
        return FeatureRunState::Unknown;
    };
    if sleep.system_requested {
        FeatureRunState::Running
    } else if !sleep.suppressed_apps.is_empty() && sleep.requesting_apps.is_empty() {
        FeatureRunState::Paused
    } else {
        FeatureRunState::Waiting
    }
}

fn waiting_for_power_source(page: Page, runtime: &RuntimeStatusSnapshot) -> bool {
    match page {
        Page::AdaptiveEngine => {
            runtime
                .feature_status
                .adaptive_engine_process
                .waiting_for_power_source
        }
        Page::ProcessPriority => {
            runtime
                .feature_status
                .process_priority
                .waiting_for_power_source
        }
        Page::ThreadPriority => {
            runtime
                .feature_status
                .thread_priority
                .waiting_for_power_source
        }
        Page::DynamicPriorityBoost => {
            runtime
                .feature_status
                .dynamic_priority_boost
                .waiting_for_power_source
        }
        Page::IoPriority => runtime.feature_status.io_priority.waiting_for_power_source,
        Page::GpuPriority => runtime.feature_status.gpu_priority.waiting_for_power_source,
        Page::MemoryPriority => {
            runtime
                .feature_status
                .memory_priority
                .waiting_for_power_source
        }
        Page::EfficiencyMode => {
            runtime
                .feature_status
                .efficiency_mode
                .waiting_for_power_source
        }
        Page::TimerResolution => {
            runtime
                .feature_status
                .timer_resolution
                .waiting_for_power_source
        }
        Page::CpuSetsSoft => {
            runtime
                .feature_status
                .cpu_sets_soft
                .waiting_for_power_source
        }
        Page::ProcessorAffinityHard => {
            runtime
                .feature_status
                .processor_affinity_hard
                .waiting_for_power_source
        }
        _ => false,
    }
}

fn sidebar_state(
    page: Page,
    settings: &Settings,
    runtime: &RuntimeStatusSnapshot,
    summary: &FeatureStatusSummary<'_>,
    observed_state: Option<FeatureRunState>,
) -> FeatureRunState {
    if waiting_for_power_source(page, runtime) {
        return if runtime.worker_error.is_some() || summary.last_error.is_some() {
            FeatureRunState::Error
        } else {
            FeatureRunState::Paused
        };
    }
    if page == Page::TimerResolution {
        let timer = &runtime.feature_status.timer_resolution;
        if !timer.enabled && timer.requested_100ns.is_some() && timer.last_error.is_some() {
            return FeatureRunState::Error;
        }
    } else if page == Page::ProcessPriority {
        let priority = &runtime.feature_status.process_priority;
        if !priority.enabled && priority.last_error.is_some() {
            return FeatureRunState::Error;
        }
    } else if page == Page::AppSuspension {
        let suspension = &runtime.feature_status.app_suspension;
        if settings.general.enabled && suspension.status == AppSuspensionStatus::PowerSourceUnknown
        {
            return if runtime.worker_error.is_some() || summary.last_error.is_some() {
                FeatureRunState::Error
            } else {
                FeatureRunState::Paused
            };
        }
        if !suspension.status_unknown
            && matches!(
                suspension.status,
                AppSuspensionStatus::AutomationDisabled | AppSuspensionStatus::Disabled
            )
            && suspension.last_error.is_some()
        {
            return FeatureRunState::Error;
        }
    }
    if page == Page::CpuLimiter {
        let limiter = &runtime.feature_status.cpu_limiter;
        if limiter.release_pending && summary.last_error.is_some() {
            return FeatureRunState::Error;
        }
        if limiter.waiting_for_power_source {
            return if runtime.worker_error.is_some() || summary.last_error.is_some() {
                FeatureRunState::Error
            } else {
                FeatureRunState::Paused
            };
        }
    }
    if page == Page::MemoryTrim
        && matches!(
            runtime.feature_status.memory_trim.status,
            MemoryTrimStatus::WaitingForPowerSource { .. }
        )
    {
        return if runtime.worker_error.is_some() {
            FeatureRunState::Error
        } else {
            FeatureRunState::Paused
        };
    }
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
    if let Some(state) = observed_state {
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
        if plan.owner == Some(PowerPlanOwner::AdaptiveProcessorPower)
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
    if page == Page::ResourceThresholds
        && !features.resource_thresholds.observation_known
        && settings
            .resource_thresholds
            .rules
            .iter()
            .any(|rule| rule.enabled)
    {
        return FeatureRunState::Unknown;
    }
    if page == Page::InstanceBalancing
        && (!features.instance_balancing.enabled || features.instance_balancing.detail.is_none())
    {
        return FeatureRunState::Unknown;
    }
    if page == Page::KeepRunning {
        use crate::features::watchdog::keep_running::KeepRunningState;
        let keep_running = &features.keep_running;
        if !keep_running.observation_known {
            return FeatureRunState::Unknown;
        }
        let mut running = false;
        let mut awaiting_observation = false;
        for rule in settings
            .keep_running
            .rules
            .iter()
            .filter(|rule| rule.enabled)
        {
            match keep_running.rule_status(rule).map(|report| report.state) {
                Some(KeepRunningState::Paused(_)) => return FeatureRunState::Paused,
                Some(KeepRunningState::Armed | KeepRunningState::Starting) => running = true,
                None | Some(KeepRunningState::Disabled) => awaiting_observation = true,
                Some(KeepRunningState::WaitingForApp | KeepRunningState::Retrying { .. }) => {}
            }
        }
        return if running {
            FeatureRunState::Running
        } else if awaiting_observation {
            FeatureRunState::Unknown
        } else {
            FeatureRunState::Waiting
        };
    }
    if (page == Page::EfficiencyMode && features.efficiency_mode.unsupported)
        || (page == Page::AppSuspension && features.app_suspension.unsupported)
    {
        return FeatureRunState::Blocked;
    }
    let process_observation = match page {
        Page::EfficiencyMode => Some((
            features.efficiency_mode.enabled,
            features.efficiency_mode.observation_paused,
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
    if (page == Page::CpuLimiter && super::cpu_limiter::observations_paused(&features.cpu_limiter))
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
    let active = (page == Page::ResourceThresholds
        && summary.scanned.is_some_and(|count| count > 0))
        || summary.adjusted.is_some_and(|count| count > 0)
        || (page == Page::TimerResolution && features.timer_resolution.requested_100ns.is_some());
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
        Page::EfficiencyMode => {
            let status = &runtime.feature_status.efficiency_mode;
            let observed = status.enabled && !status.observation_paused;
            FeatureStatusSummary {
                scanned: observed.then_some(status.scanned_processes),
                adjusted: observed.then_some(status.throttled_processes),
                protected_or_denied: observed.then_some(status.access_denied_processes),
                skipped: observed.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::EfficiencyMode,
            }
        }
        Page::ResourceThresholds => {
            let status = &runtime.feature_status.resource_thresholds;
            let observed = status.observation_known;
            FeatureStatusSummary {
                scanned: observed.then_some(status.monitored_processes),
                adjusted: observed.then_some(status.alerting_processes),
                protected_or_denied: None,
                skipped: observed.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
                action_log_feature: ActionLogFeature::ResourceThresholds,
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
                MemoryTrimStatus::ForegroundUnknown
                    | MemoryTrimStatus::WaitingForPowerSource { .. }
                    | MemoryTrimStatus::WaitingForMemoryLoad { .. }
                    | MemoryTrimStatus::Error(_)
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
            let observed = !super::cpu_limiter::observations_paused(status);
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
            let observed = !status.status_unknown;
            // A paused scan still reports freeze ownership from the locked controller.
            let ownership_known = observed || status.enabled;
            FeatureStatusSummary {
                scanned: None,
                adjusted: ownership_known.then_some(status.suspended_processes),
                protected_or_denied: None,
                skipped: observed.then_some(status.skipped_processes),
                last_error: status.last_error.as_deref(),
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
    fn adaptive_processor_power_status_tracks_confirmed_ownership_and_release() {
        use crate::control::power_plan::PowerPlanOwner;

        let mut runtime = RuntimeStatusSnapshot::default();
        assert_eq!(
            adaptive_processor_power_state(true, &runtime, &[]),
            (FeatureRunState::Unknown, None)
        );

        runtime.generation = 1;
        std::sync::Arc::make_mut(&mut runtime.power_plan_status).current_guid =
            Some("balanced".to_owned());
        assert_eq!(
            adaptive_processor_power_state(true, &runtime, &[]),
            (FeatureRunState::Waiting, None)
        );

        let plan = std::sync::Arc::make_mut(&mut runtime.power_plan_status);
        plan.current_guid = Some("adaptive".to_owned());
        plan.target_guid = Some("adaptive".to_owned());
        plan.owner = Some(PowerPlanOwner::AdaptiveProcessorPower);
        assert_eq!(
            adaptive_processor_power_state(true, &runtime, &[]),
            (FeatureRunState::Running, None)
        );
        assert_eq!(
            adaptive_processor_power_state(false, &runtime, &[]),
            (FeatureRunState::Waiting, None)
        );

        let plan = std::sync::Arc::make_mut(&mut runtime.power_plan_status);
        plan.adaptive_processor_power_error = Some(
            crate::control::power_plan::AdaptiveProcessorPowerError::Operation(
                "Could not restore previous plan".to_owned(),
            ),
        );
        assert_eq!(
            adaptive_processor_power_state(false, &runtime, &[]),
            (
                FeatureRunState::Error,
                Some(Cow::Borrowed("Could not restore previous plan"))
            )
        );

        let plan = std::sync::Arc::make_mut(&mut runtime.power_plan_status);
        plan.current_guid = Some("balanced".to_owned());
        plan.target_guid = None;
        plan.owner = None;
        plan.adaptive_processor_power_error = None;
        assert_eq!(
            adaptive_processor_power_state(false, &runtime, &[]),
            (FeatureRunState::Inactive, None)
        );
    }

    #[test]
    fn adaptive_processor_power_rail_distinguishes_source_wait_from_restoration() {
        use crate::control::power_plan::{PowerPlanOwner, PowerPlanStatus};

        let settings = Settings::default();
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            power_plan_status: PowerPlanStatus {
                current_guid: Some("adaptive".into()),
                target_guid: Some("adaptive".into()),
                owner: Some(PowerPlanOwner::AdaptiveProcessorPower),
                adaptive_processor_power_waiting_for_source: true,
                ..Default::default()
            }
            .into(),
            ..Default::default()
        };
        let labels = rail_text(adaptive_processor_power_view(&settings, &runtime, &[]));
        assert_eq!(labels[0], t!("common.waiting"));
        assert_eq!(
            labels[1],
            t!("adaptive_processor_power.waiting_for_power_source")
        );
        assert!(labels.iter().any(|label| label == "Winderust Adaptive"));

        std::sync::Arc::make_mut(&mut runtime.power_plan_status)
            .adaptive_processor_power_waiting_for_source = false;
        let labels = rail_text(adaptive_processor_power_view(&settings, &runtime, &[]));
        assert_eq!(labels[0], t!("common.waiting"));
        assert_eq!(labels[1], t!("adaptive_processor_power.releasing_plan"));
    }

    #[test]
    fn instance_limits_status_ignores_reports_for_superseded_rules() {
        use crate::config::InstanceLimitsRule;
        use crate::features::watchdog::instance_limits::InstanceLimitsRuleSnapshot;

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

        settings.instance_limits.rules[0].notify = false;
        let status = instance_limits_status(&settings, &snapshot);
        assert_eq!(status.state, FeatureRunState::Running);
        assert_eq!(status.incidents, 1);
        assert_eq!(status.last_error, None);

        settings.instance_limits.rules[0].max_instances = 2;
        let status = instance_limits_status(&settings, &snapshot);
        assert_eq!(status.state, FeatureRunState::Unknown);
        assert_eq!(status.enabled_rules, 1);
        assert_eq!(status.incidents, 0);
        assert_eq!(status.last_error, None);
    }

    #[test]
    fn resource_thresholds_counts_require_a_completed_observation() {
        use crate::{
            config::ResourceThresholdsRule,
            features::watchdog::resource_thresholds::ResourceThresholdsManager,
        };

        let mut settings = Settings::default();
        settings.general.enabled = true;
        settings.resource_thresholds.enabled = true;
        settings.resource_thresholds.rules =
            vec![ResourceThresholdsRule::new(r"C:\Apps\worker.exe".into())];
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            ..Default::default()
        };
        let status = |runtime: &RuntimeStatusSnapshot| {
            let summary = feature_status_summary(Page::ResourceThresholds, runtime).unwrap();
            (
                sidebar_state(Page::ResourceThresholds, &settings, runtime, &summary, None),
                [summary.scanned, summary.adjusted, summary.skipped],
            )
        };
        assert_eq!(status(&runtime), (FeatureRunState::Unknown, [None; 3]));

        let (sample, events) = ResourceThresholdsManager::default().update(
            &settings.resource_thresholds,
            true,
            &mut crate::runtime::observations::CycleObservations::with_processes(Vec::new()),
        );
        assert!(events.is_empty());
        assert_ne!(sample, Default::default());
        std::sync::Arc::make_mut(&mut runtime.feature_status).resource_thresholds = sample;
        assert_eq!(status(&runtime), (FeatureRunState::Waiting, [Some(0); 3]));
    }

    #[test]
    fn app_suspension_sidebar_reports_pending_restore_when_disabled() {
        let mut settings = Settings::default();
        settings.general.enabled = true;
        settings.app_suspension.enabled = false;
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            ..Default::default()
        };

        for (frozen, error, unknown, expected) in [
            (1, Some("thaw failed"), false, FeatureRunState::Error),
            (0, None, false, FeatureRunState::Inactive),
            (0, Some("lock error"), true, FeatureRunState::Inactive),
        ] {
            let features = std::sync::Arc::make_mut(&mut runtime.feature_status);
            features.app_suspension.suspended_processes = frozen;
            features.app_suspension.last_error = error.map(str::to_owned);
            features.app_suspension.status_unknown = unknown;

            let summary = feature_status_summary(Page::AppSuspension, &runtime).unwrap();
            assert_eq!(
                sidebar_state(Page::AppSuspension, &settings, &runtime, &summary, None),
                expected
            );
        }
    }

    fn rail_text(mut rail: Element<'_, Message>) -> Vec<String> {
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

        let renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let mut tree = Tree::new(&rail);
        let size = iced::Size::new(216.0, 560.0);
        let node =
            rail.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut details = Details::default();
        rail.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut details);
        details.0
    }

    #[test]
    fn app_suspension_rail_explains_source_wait_and_preserves_release_error() {
        let settings = Settings::default();
        assert!(!settings.app_suspension.enabled);
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            ..Default::default()
        };
        let status = &mut std::sync::Arc::make_mut(&mut runtime.feature_status).app_suspension;
        status.enabled = true;
        status.status_unknown = true;
        status.status = AppSuspensionStatus::PowerSourceUnknown;
        let text = |runtime: &RuntimeStatusSnapshot| {
            rail_text(view(Page::AppSuspension, &settings, runtime, &[], None).unwrap())
        };
        let rail = text(&runtime);
        assert!(rail
            .iter()
            .any(|line| line == t!("sidebar_status.paused").as_ref()));
        assert!(rail
            .iter()
            .any(|line| line == t!("app_suspension.status.power_source_unknown").as_ref()));

        std::sync::Arc::make_mut(&mut runtime.feature_status)
            .app_suspension
            .last_error = Some("Exact job thaw failed.".to_owned());
        let rail = text(&runtime);
        assert!(rail.iter().any(|line| line == "Exact job thaw failed."));
    }

    #[test]
    fn adaptive_engine_restraint_rail_preserves_only_pending_rows_across_sources() {
        use crate::control::{
            priority_efficiency::AdaptiveRestraintProcess, process::ProcessIdentity,
        };
        use std::sync::Arc;

        let mut settings = Settings::default();
        settings.adaptive_engine.enabled = true;
        settings
            .adaptive_engine_process
            .cpu_pressure_restraint_enabled = true;
        let row = |id, name: &str, state| AdaptiveRestraintProcess {
            process: ProcessIdentity::new(
                id,
                name.into(),
                format!("C:\\Apps\\{name}").into(),
                1,
                None,
            ),
            state,
        };
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            ..Default::default()
        };
        Arc::make_mut(&mut runtime.feature_status).adaptive_engine_process =
            AdaptiveEngineProcessSnapshot {
                source: Some(PowerSourceProfile::PluggedIn),
                enabled: true,
                restraint_processes: Some(Arc::from([
                    row(1, "verified.exe", AdaptiveRestraintState::Verified),
                    row(2, "restore.exe", AdaptiveRestraintState::RestorationPending),
                    row(
                        3,
                        "uncertain.exe",
                        AdaptiveRestraintState::ConfirmationPending,
                    ),
                ])),
                ..Default::default()
            };
        let render = |settings, plugged_in| {
            rail_text(view(Page::AdaptiveEngine, settings, &runtime, &[], plugged_in).unwrap())
        };
        let live = render(&settings, Some(true));
        let mismatched = render(&settings, Some(false));
        let mut disabled = settings.clone();
        disabled.adaptive_engine.enabled = false;
        let inactive = render(&disabled, Some(true));

        let item_visible = |rail: &[String], name: &str, key: &str| {
            rail.windows(2)
                .any(|pair| pair[0] == name && pair[1] == t!(key))
        };
        assert!(item_visible(
            &live,
            "verified.exe (1)",
            "adaptive_engine_process.restraint_verified",
        ));
        for rail in [&live, &mismatched, &inactive] {
            assert!(item_visible(
                rail,
                "restore.exe (2)",
                "adaptive_engine_process.restraint_restoration_pending",
            ));
            assert!(item_visible(
                rail,
                "uncertain.exe (3)",
                "adaptive_engine_process.restraint_confirmation_pending",
            ));
        }
        for rail in [mismatched, inactive] {
            assert!(!rail.iter().any(|line| line == "verified.exe (1)"));
        }
    }

    #[test]
    fn adaptive_engine_policy_status_follows_live_source_and_preserves_cleanup_failures() {
        use crate::action_log::{ActionLog, ActionLogResult};
        use crate::adaptive_engine_process::{
            AdaptiveEngineProcessSnapshot, DynamicResourceZoneSnapshot, ZoneStatus,
        };

        fn value<'a>(rail: &'a [String], key: &str) -> &'a str {
            let label = t!(key);
            rail.windows(2)
                .find(|pair| pair[0] == label)
                .map(|pair| pair[1].as_str())
                .expect("status metric")
        }

        let mut saved = Settings::default();
        let battery = saved.battery_profile_mut();
        battery.adaptive_engine.enabled = true;
        battery
            .adaptive_engine_process
            .dynamic_resource_zones_enabled = true;
        battery
            .adaptive_engine_process
            .limit_background_processors_enabled = true;
        let editor = crate::application::SettingsEditor::with_settings(saved);
        let mut action_log = ActionLog::new(4);
        action_log.record(
            ActionLogFeature::AdaptiveEngine,
            None,
            "test.exe",
            ActionLogResult::Failed,
            "earlier process error",
        );
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            action_log_summaries: action_log.summaries().into(),
            ..Default::default()
        };
        let features = std::sync::Arc::make_mut(&mut runtime.feature_status);
        features.adaptive_engine_process = AdaptiveEngineProcessSnapshot {
            source: Some(PowerSourceProfile::OnBattery),
            zones: DynamicResourceZoneSnapshot {
                status: ZoneStatus::Active,
                foreground_targets: 3,
                ..Default::default()
            },
            enabled: true,
            adjusted_processes: 4,
            last_error: Some("reported process error".into()),
            ..Default::default()
        };
        features.bottleneck_classifier.enabled = true;
        features.bottleneck_classifier.total_cpu_tenths = Some(420);
        let render = |observed, runtime: &RuntimeStatusSnapshot| {
            let page = Page::AdaptiveEngine;
            let settings = saved_rail_settings(&editor, page, runtime, observed);
            rail_text(view(page, settings, runtime, &[], observed).unwrap())
        };
        for observed in [Some(false), None] {
            let rail = render(observed, &runtime);
            assert_eq!(value(&rail, "common.adjusted_processes"), "4");
            assert_eq!(
                value(&rail, "adaptive_engine_process.zone_foreground_targets"),
                "3"
            );
            assert!(rail.iter().any(|text| text == "reported process error"));
        }

        let rail = render(Some(true), &runtime);
        assert_eq!(rail[0], t!("common.inactive"));
        assert_eq!(
            value(&rail, "adaptive_engine_process.dynamic_resource_zones"),
            t!("common.inactive")
        );
        assert_eq!(
            value(&rail, "adaptive_engine_process.zone_foreground_targets"),
            count(None)
        );
        assert_eq!(value(&rail, "common.adjusted_processes"), count(None));
        assert!(!rail.iter().any(|text| text == "reported process error"));
        assert_eq!(value(&rail, "common.failed_count"), "1");

        std::sync::Arc::make_mut(&mut runtime.feature_status).adaptive_engine_process =
            AdaptiveEngineProcessSnapshot {
                source: Some(PowerSourceProfile::PluggedIn),
                zones: DynamicResourceZoneSnapshot {
                    status: ZoneStatus::Degraded,
                    reason: "zone_retry",
                    ..Default::default()
                },
                last_error: Some("could not restore process allocation".into()),
                ..Default::default()
            };
        let rail = render(Some(true), &runtime);
        assert_eq!(
            value(&rail, "adaptive_engine_process.dynamic_resource_zones"),
            t!("adaptive_engine_process.zone_degraded")
        );
        assert!(rail
            .iter()
            .any(|text| text == "could not restore process allocation"));
        let rail = render(Some(false), &runtime);
        assert_eq!(rail[0], t!("common.unknown"));
        assert_eq!(
            value(&rail, "adaptive_engine_process.dynamic_resource_zones"),
            t!("common.unknown")
        );
        assert_eq!(
            value(&rail, "adaptive_engine_process.limit_background_processors"),
            t!("common.unknown")
        );
        assert_eq!(
            value(&rail, "adaptive_engine_process.zone_foreground_targets"),
            count(None)
        );
        assert!(!rail
            .iter()
            .any(|text| text == "could not restore process allocation"));
        assert!(rail.iter().any(|text| text == "42.0%"));
    }

    #[test]
    fn bottleneck_rail_distinguishes_inactive_priming_and_unavailable_readings() {
        use crate::bottleneck_classifier::{BottleneckSnapshot, BottleneckState};

        let mut settings = Settings::default();
        settings.general.enabled = true;
        settings.adaptive_engine.enabled = true;
        let status_text = |settings: &Settings, snapshot, key| {
            let mut runtime = RuntimeStatusSnapshot {
                generation: 1,
                ..Default::default()
            };
            std::sync::Arc::make_mut(&mut runtime.feature_status).bottleneck_classifier = snapshot;
            let rail = view(Page::AdaptiveEngine, settings, &runtime, &[], Some(true)).unwrap();
            let title = t!("adaptive_engine.bottleneck_classifier");
            let mut values = rail_text(rail)
                .into_iter()
                .skip_while(|value| value != title.as_ref())
                .skip(1);
            assert_eq!(values.next(), Some(t!("common.status").into_owned()));
            assert_eq!(values.next(), Some(t!(key).into_owned()));
            values.collect::<Vec<_>>()
        };

        let collecting = "adaptive_engine.bottleneck_unknown";
        let startup = status_text(&settings, BottleneckSnapshot::default(), collecting);
        assert_eq!(
            startup
                .iter()
                .filter(|value| value.as_str() == "\u{2014}")
                .count(),
            3
        );

        let priming = BottleneckSnapshot {
            enabled: true,
            total_cpu_tenths: Some(420),
            ..Default::default()
        };
        let pending = status_text(&settings, priming.clone(), collecting);
        assert!(pending.iter().any(|value| value == "42.0%"));

        let pdh_error = "PDH GPU Engine query failed with status 0xC0000BB8.";
        let failure = BottleneckSnapshot {
            last_error: Some(pdh_error.into()),
            ..priming.clone()
        };
        let unavailable = status_text(&settings, failure.clone(), "common.unavailable");
        assert!(unavailable.iter().any(|value| value == "42.0%"));
        assert!(unavailable.iter().any(|value| value == pdh_error));

        let recovered = BottleneckSnapshot {
            state: BottleneckState::GpuBound,
            busiest_gpu_tenths: Some(875),
            ..priming
        };
        let healthy = status_text(
            &settings,
            recovered.clone(),
            "adaptive_engine.bottleneck_gpu_bound",
        );
        assert!(healthy.iter().any(|value| value == "87.5%"));
        assert!(!healthy.iter().any(|value| value == pdh_error));

        settings.adaptive_engine.enabled = false;
        let inactive = status_text(&settings, failure, "common.inactive");
        assert!(!inactive.iter().any(|value| value == pdh_error));
        assert_eq!(
            inactive
                .iter()
                .filter(|value| value.as_str() == "\u{2014}")
                .count(),
            3
        );

        settings.adaptive_engine.enabled = true;
        settings.general.enabled = false;
        assert_eq!(
            status_text(&settings, recovered, "common.inactive"),
            inactive
        );
    }

    #[test]
    fn sidebar_shows_a_current_error_alongside_an_older_failed_action() {
        use crate::action_log::{ActionLog, ActionLogResult};

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

        let details = rail_text(view(Page::IoPriority, &settings, &runtime, &[], None).unwrap());
        assert!(details.iter().any(|text| text == "current restore failure"));
        assert!(details
            .iter()
            .any(|text| text.ends_with(" - older apply failure")));
    }

    #[test]
    fn sleep_prevention_rail_waits_for_the_current_power_source_and_keeps_history() {
        use crate::action_log::{ActionLog, ActionLogResult};

        fn value<'a>(rail: &'a [String], key: &str) -> &'a str {
            let label = t!(key);
            rail.windows(2)
                .find(|pair| pair[0] == label)
                .map(|pair| pair[1].as_str())
                .expect("status metric")
        }

        let mut settings = Settings::default();
        settings.general.enabled = true;
        settings.sleep_prevention.enabled = true;
        let mut log = ActionLog::new(4);
        for (result, reason) in [
            (ActionLogResult::Applied, "previous request accepted"),
            (ActionLogResult::Failed, "older clear failure"),
        ] {
            log.record(
                ActionLogFeature::SleepPrevention,
                None,
                "System",
                result,
                reason,
            );
        }
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            action_log_summaries: log.summaries().into(),
            ..Default::default()
        };
        for (source, plugged_in) in [
            (PowerSourceProfile::PluggedIn, true),
            (PowerSourceProfile::OnBattery, false),
        ] {
            std::sync::Arc::make_mut(&mut runtime.feature_status).sleep_prevention =
                SleepPreventionSnapshot {
                    source: Some(source),
                    enabled: true,
                    observation_known: true,
                    matched_rules: 2,
                    matched_processes: 3,
                    system_requested: true,
                    display_requested: true,
                    last_error: Some("current source error".into()),
                    ..Default::default()
                };
            let capture = |observed| {
                rail_text(view(Page::SleepPrevention, &settings, &runtime, &[], observed).unwrap())
            };

            for observed in [Some(plugged_in), None] {
                let current = capture(observed);
                assert_eq!(value(&current, "sleep_prevention.matching_rules"), "2");
                assert_eq!(value(&current, "sleep_prevention.matching_processes"), "3");
                for request in [
                    "sleep_prevention.system_request",
                    "sleep_prevention.display_request",
                ] {
                    assert_eq!(
                        value(&current, request),
                        t!("sleep_prevention.request_accepted")
                    );
                }
                assert!(current.iter().any(|text| text == "current source error"));
            }

            let stale = capture(Some(!plugged_in));
            assert_eq!(stale[0], t!("common.unknown"));
            assert_eq!(
                value(&stale, "sleep_prevention.matching_rules"),
                count(None)
            );
            assert_eq!(
                value(&stale, "sleep_prevention.matching_processes"),
                count(None)
            );
            for request in [
                "sleep_prevention.system_request",
                "sleep_prevention.display_request",
            ] {
                assert_eq!(value(&stale, request), t!("common.unknown"));
            }
            assert!(!stale.iter().any(|text| text == "current source error"));
            assert_eq!(value(&stale, "common.success_count"), "1");
            assert_eq!(value(&stale, "common.failed_count"), "1");
        }
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

        let limiter = &mut std::sync::Arc::make_mut(&mut runtime.feature_status).cpu_limiter;
        limiter.limited_processes = 1;
        limiter.active_affinity.push((
            r"C:\Apps\test.exe".into(),
            crate::features::cpu_control::cpu_limiter::CpuLimiterAffinityStatus::Active(2),
        ));
        assert_eq!(
            activity(&runtime)[0],
            (FeatureRunState::Running, [Some(0), Some(1), Some(0)])
        );

        let features = std::sync::Arc::make_mut(&mut runtime.feature_status);
        features.cpu_limiter.active_affinity.clear();
        features.cpu_limiter.limited_processes = 0;
        features.cpu_limiter.last_error = Some("restore failed".into());
        features.cpu_sets_soft.last_error = Some("restore failed".into());
        features.processor_affinity_hard.last_error = Some("restore failed".into());
        assert_eq!(activity(&runtime), [(FeatureRunState::Error, [None; 3]); 3]);

        settings.cpu_limiter.enabled = false;
        std::sync::Arc::make_mut(&mut runtime.feature_status)
            .cpu_limiter
            .waiting_for_power_source = true;
        let labels = |runtime: &RuntimeStatusSnapshot| {
            rail_text(view(Page::CpuLimiter, &settings, runtime, &[], None).unwrap())
        };
        assert_eq!(labels(&runtime)[0], t!("common.error"));
        std::sync::Arc::make_mut(&mut runtime.feature_status)
            .cpu_limiter
            .last_error = None;
        let waiting = labels(&runtime);
        assert_eq!(waiting[0], t!("sidebar_status.paused"));
        assert_eq!(waiting[1], t!("cpu_limiter.waiting_for_power_source"));

        let limiter = &mut std::sync::Arc::make_mut(&mut runtime.feature_status).cpu_limiter;
        limiter.waiting_for_power_source = false;
        limiter.release_pending = true;
        limiter.last_error = Some("release failed".into());
        let releasing = labels(&runtime);
        assert_eq!(releasing[0], t!("common.error"));
        assert!(releasing.iter().any(|text| text == "release failed"));

        let limiter = &mut std::sync::Arc::make_mut(&mut runtime.feature_status).cpu_limiter;
        limiter.release_pending = false;
        assert_eq!(labels(&runtime)[0], t!("common.inactive"));
    }

    #[test]
    fn process_control_rail_shows_source_wait_and_restore_failure() {
        let settings = Settings::default();
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            ..Default::default()
        };
        let features = std::sync::Arc::make_mut(&mut runtime.feature_status);
        features.adaptive_engine_process.waiting_for_power_source = true;
        features.process_priority.waiting_for_power_source = true;
        features.thread_priority.waiting_for_power_source = true;
        features.dynamic_priority_boost.waiting_for_power_source = true;
        features.io_priority.waiting_for_power_source = true;
        features.gpu_priority.waiting_for_power_source = true;
        features.memory_priority.waiting_for_power_source = true;
        features.cpu_sets_soft.waiting_for_power_source = true;
        features.processor_affinity_hard.waiting_for_power_source = true;
        for page in [
            Page::AdaptiveEngine,
            Page::ProcessPriority,
            Page::ThreadPriority,
            Page::DynamicPriorityBoost,
            Page::IoPriority,
            Page::GpuPriority,
            Page::MemoryPriority,
            Page::CpuSetsSoft,
            Page::ProcessorAffinityHard,
        ] {
            let paused = rail_text(view(page, &settings, &runtime, &[], None).unwrap());
            assert_eq!(paused[0], t!("sidebar_status.paused"));
            assert_eq!(paused[1], t!("home.power_source_unavailable"));
        }

        let features = std::sync::Arc::make_mut(&mut runtime.feature_status);
        features.adaptive_engine_process.last_error = Some("Adaptive restore failed".into());
        features.process_priority.last_error = Some("priority restore failed".into());
        features.thread_priority.last_error = Some("thread restore failed".into());
        features.dynamic_priority_boost.last_error = Some("boost restore failed".into());
        features.io_priority.last_error = Some("I/O restore failed".into());
        features.gpu_priority.last_error = Some("GPU restore failed".into());
        features.memory_priority.last_error = Some("memory restore failed".into());
        features.cpu_sets_soft.last_error = Some("CPU Sets restore failed".into());
        features.processor_affinity_hard.last_error = Some("affinity restore failed".into());
        for (page, reason) in [
            (Page::AdaptiveEngine, "Adaptive restore failed"),
            (Page::ProcessPriority, "priority restore failed"),
            (Page::ThreadPriority, "thread restore failed"),
            (Page::DynamicPriorityBoost, "boost restore failed"),
            (Page::IoPriority, "I/O restore failed"),
            (Page::GpuPriority, "GPU restore failed"),
            (Page::MemoryPriority, "memory restore failed"),
            (Page::CpuSetsSoft, "CPU Sets restore failed"),
            (Page::ProcessorAffinityHard, "affinity restore failed"),
        ] {
            let error = rail_text(view(page, &settings, &runtime, &[], None).unwrap());
            assert_eq!(error[0], t!("common.error"));
            assert!(error.iter().any(|text| text == reason));
        }
    }

    #[test]
    fn efficiency_mode_sidebar_keeps_power_source_pause_and_restore_errors_across_drafts() {
        let mut saved = Settings::default();
        saved.general.enabled = true;
        saved.efficiency_mode.enabled = false;
        saved.battery_profile_mut().efficiency_mode.enabled = true;
        let mut editor = crate::application::SettingsEditor::with_settings(saved);
        editor.select_power_source(PowerSourceProfile::OnBattery);
        editor.efficiency_mode.enabled = false;
        let page = Page::EfficiencyMode;
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            ..Default::default()
        };
        let render = |runtime: &RuntimeStatusSnapshot, plugged_in| {
            let settings = saved_rail_settings(&editor, page, runtime, plugged_in);
            rail_text(view(page, settings, runtime, &[], plugged_in).unwrap())
        };
        assert_eq!(render(&runtime, Some(true))[0], t!("common.inactive"));

        let efficiency = &mut std::sync::Arc::make_mut(&mut runtime.feature_status).efficiency_mode;
        efficiency.enabled = true;
        efficiency.observation_paused = true;
        efficiency.waiting_for_power_source = true;
        let paused = render(&runtime, None);
        assert_eq!(paused[0], t!("sidebar_status.paused"));
        assert_eq!(paused[1], t!("home.power_source_unavailable"));

        let efficiency = &mut std::sync::Arc::make_mut(&mut runtime.feature_status).efficiency_mode;
        efficiency.last_error = Some("restore failed".into());
        let error = render(&runtime, None);
        assert_eq!(error[0], t!("common.error"));
        assert!(error.iter().any(|text| text == "restore failed"));

        let efficiency = &mut std::sync::Arc::make_mut(&mut runtime.feature_status).efficiency_mode;
        efficiency.last_error = None;
        efficiency.waiting_for_power_source = false;
        efficiency.observation_paused = false;
        efficiency.throttled_processes = 1;
        let summary = feature_status_summary(page, &runtime).unwrap();
        let settings = saved_rail_settings(&editor, page, &runtime, Some(false));
        assert_eq!(
            sidebar_state(page, settings, &runtime, &summary, None),
            FeatureRunState::Running
        );
    }

    #[test]
    fn timer_resolution_sidebar_shows_power_source_pause_and_release_failure() {
        let mut settings = Settings::default();
        settings.general.enabled = true;
        settings.timer_resolution.enabled = false;
        let battery = &mut settings.battery_profile_mut().timer_resolution;
        battery.enabled = true;
        battery.rules.push(crate::config::TimerResolutionRule {
            enabled: true,
            executable_path: r"C:\Apps\timer.exe".into(),
            desired_100ns: 100_000,
        });
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            ..Default::default()
        };
        let labels = |runtime: &RuntimeStatusSnapshot| {
            rail_text(view(Page::TimerResolution, &settings, runtime, &[], None).unwrap())
        };
        assert_eq!(labels(&runtime)[0], t!("common.inactive"));

        let timer = &mut std::sync::Arc::make_mut(&mut runtime.feature_status).timer_resolution;
        timer.enabled = true;
        timer.observation_paused = true;
        timer.waiting_for_power_source = true;
        let paused = labels(&runtime);
        assert_eq!(paused[0], t!("sidebar_status.paused"));
        assert_eq!(paused[1], t!("home.power_source_unavailable"));

        let timer = &mut std::sync::Arc::make_mut(&mut runtime.feature_status).timer_resolution;
        timer.requested_100ns = Some(100_000);
        timer.last_error = Some("release failed".into());
        let failed = labels(&runtime);
        assert_eq!(failed[0], t!("common.error"));
        assert!(failed.iter().any(|text| text == "release failed"));
        assert!(failed
            .iter()
            .any(|text| text == &crate::timer_resolution::format_resolution_ms(100_000)));
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
    fn ordinary_power_rule_rails_explain_pause_reasons() {
        use crate::control::power_plan::PowerPlanStatus;

        let pages = [
            Page::ByActivity,
            Page::ByFocus,
            Page::ByRunningApp,
            Page::ByCpuLoad,
            Page::ByTime,
        ];
        let mut settings = Settings::default();
        settings.general.enabled = true;
        for page in pages {
            *super::super::navigation::feature_toggle(page).unwrap()(&mut settings) = true;
        }
        let mut runtime = RuntimeStatusSnapshot {
            generation: 1,
            ..Default::default()
        };
        let assert_rail = |page,
                           settings: &Settings,
                           runtime: &RuntimeStatusSnapshot,
                           label: &str,
                           detail: &str| {
            let text = rail_text(view(page, settings, runtime, &[], None).unwrap());
            assert_eq!(text[0], t!(label), "{page:?}");
            assert_eq!(text[1], t!(detail), "{page:?}");
        };
        for (owner, decision, detail) in [
            (
                None,
                DecisionState::PausedWhilePluggedIn,
                "power_plan_control.paused_on_ac",
            ),
            (
                None,
                DecisionState::WaitingForObservation,
                "power_plan_control.waiting_for_observation",
            ),
            (
                Some(PowerPlanOwner::AdaptiveProcessorPower),
                DecisionState::PausedWhilePluggedIn,
                "power_plan_control.switch_managed_active",
            ),
        ] {
            *std::sync::Arc::make_mut(&mut runtime.power_plan_status) = PowerPlanStatus {
                owner,
                decision_state: Some(decision),
                current_guid: Some("active".into()),
                target_guid: owner.map(|_| "active".into()),
                ..Default::default()
            };
            for page in pages {
                assert_rail(page, &settings, &runtime, "sidebar_status.paused", detail);
            }
        }

        settings.by_time.enabled = false;
        assert_rail(
            Page::ByTime,
            &settings,
            &runtime,
            "common.inactive",
            "sidebar_status.inactive_help",
        );
        settings.by_time.enabled = true;
        std::sync::Arc::make_mut(&mut runtime.power_plan_status).active_plan_read_error =
            Some("Could not read the active plan.".into());
        assert_rail(
            Page::ByTime,
            &settings,
            &runtime,
            "common.error",
            "sidebar_status.error_help",
        );
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
        plan.owner = Some(PowerPlanOwner::AdaptiveProcessorPower);
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
            (
                MemoryTrimStatus::WaitingForPowerSource {
                    manual_trim_blocked: true,
                },
                None,
            ),
            (MemoryTrimStatus::Error("query failed".into()), None),
            (
                MemoryTrimStatus::WaitingForMemoryLoad {
                    threshold_percent: 80,
                },
                None,
            ),
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

        for (enabled, unknown, [frozen, skipped], expected) in [
            (true, true, [2, 0], [Some(2), None]),
            (true, true, [0, 0], [Some(0), None]),
            (false, true, [0, 0], [None, None]),
            (true, false, [0, 0], [Some(0); 2]),
            (true, false, [1, 3], [Some(1), Some(3)]),
        ] {
            let suspension =
                &mut std::sync::Arc::make_mut(&mut runtime.feature_status).app_suspension;
            suspension.enabled = enabled;
            suspension.status_unknown = unknown;
            suspension.suspended_processes = frozen;
            suspension.skipped_processes = skipped;
            let summary = feature_status_summary(Page::AppSuspension, &runtime).unwrap();
            assert_eq!([summary.adjusted, summary.skipped], expected);
        }
    }
}
