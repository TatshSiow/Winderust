use super::*;

pub(super) fn theme_mode_label(mode: AppThemeMode) -> String {
    match mode {
        AppThemeMode::System => t!("theme.system").to_string(),
        AppThemeMode::Light => t!("theme.light").to_string(),
        AppThemeMode::Dark => t!("theme.dark").to_string(),
    }
}

pub(super) fn update_channel_label(channel: UpdateChannel) -> String {
    match channel {
        UpdateChannel::Stable => t!("update_channel.stable").to_string(),
        UpdateChannel::PreRelease => t!("update_channel.pre_release").to_string(),
    }
}

pub(super) fn animation_mode_label(mode: AnimationMode) -> String {
    match mode {
        AnimationMode::System => t!("animation.system").to_string(),
        AnimationMode::On => t!("animation.on").to_string(),
        AnimationMode::Off => t!("animation.off").to_string(),
    }
}

pub(super) fn accent_source_label(source: AccentColorSource) -> String {
    match source {
        AccentColorSource::Windows => t!("theme.system").to_string(),
        AccentColorSource::Custom => t!("accent.custom").to_string(),
    }
}

pub(super) fn action_log_action_label(action: ActionLogAction) -> &'static str {
    match action {
        ActionLogAction::Apply => "Apply",
        ActionLogAction::Restore => "Restore",
        ActionLogAction::Skip => "Skip",
        ActionLogAction::Fail => "Fail",
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
            action_log_action_label(entry.action),
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

#[cfg(test)]
pub(super) fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
pub(super) fn push_csv_field(csv: &mut String, value: &str) {
    if value.contains([',', '"', '\n', '\r']) {
        csv.push('"');
        for character in value.chars() {
            if character == '"' {
                csv.push('"');
            }
            csv.push(character);
        }
        csv.push('"');
    } else {
        csv.push_str(value);
    }
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

pub(super) fn rule_count_label(count: usize) -> String {
    t!("common.rule_count", count = count).to_string()
}

pub(super) fn yes_no_label(value: bool) -> String {
    if value {
        t!("common.yes")
    } else {
        t!("common.no")
    }
    .to_string()
}

pub(super) fn schedule_days_label(days: &[WeekdaySetting]) -> String {
    if days.len() == WeekdaySetting::all().len() {
        return t!("common.all").to_string();
    }
    if days.is_empty() {
        return t!("common.none").to_string();
    }
    WeekdaySetting::all()
        .into_iter()
        .filter(|day| days.contains(day))
        .map(weekday_short_label)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn activity_state_label(state: ActivityState) -> String {
    match state {
        ActivityState::Active => t!("home.activity_active"),
        ActivityState::Idle => t!("home.activity_idle"),
        ActivityState::Unknown => t!("home.activity_unknown"),
    }
    .to_string()
}

pub(super) fn localized_runtime_status(message: &str) -> String {
    let key = match message {
        "Automation disabled." => "runtime_status.automation_disabled",
        "Paused: foreground app is unknown." => "runtime_status.foreground_unknown",
        "Paused: current Windows session is unknown." => "runtime_status.session_unknown",
        "Core Steering disabled." => "runtime_status.core_steering_disabled",
        "Core Limiter disabled." => "runtime_status.core_limiter_disabled",
        "Core Limiter active." => "runtime_status.core_limiter_active",
        "Timer resolution query failed." => "runtime_status.timer_resolution_query_failed",
        "App Suspension disabled." => "runtime_status.app_suspension_disabled",
        "App Suspension unavailable: Windows Job Object freeze is not supported on this system." => {
            "runtime_status.app_suspension_unsupported"
        }
        "By Running App disabled." => "runtime_status.by_running_app_disabled",
        "By Running App waiting for a matching process." => {
            "runtime_status.by_running_app_waiting"
        }
        "By Running App active." => "runtime_status.by_running_app_active",
        "GPU priority defaults disabled." => "runtime_status.gpu_priority_disabled",
        "I/O priority defaults disabled." => "runtime_status.io_priority_disabled",
        "I/O priority defaults active." => "runtime_status.io_priority_active",
        "Background Efficiency disabled." => "runtime_status.background_efficiency_disabled",
        "Background Efficiency active." => "runtime_status.background_efficiency_active",
        "Dynamic priority boost defaults disabled." => {
            "runtime_status.dynamic_priority_boost_disabled"
        }
        "Dynamic priority boost defaults active." => {
            "runtime_status.dynamic_priority_boost_active"
        }
        "Memory Trim disabled." => "runtime_status.memory_trim_disabled",
        "Thread Priority disabled." => "runtime_status.thread_priority_disabled",
        "Thread Priority active." => "runtime_status.thread_priority_active",
        "Process priority defaults disabled." => "runtime_status.process_priority_disabled",
        "Process priority defaults active." => "runtime_status.process_priority_active",
        "Workload Engine disabled." => "runtime_status.workload_engine_disabled",
        "Workload Engine active." => "runtime_status.workload_engine_active",
        "No usable CPU restriction target." => "runtime_status.background_cpu_no_target",
        "Background CPU Restriction active." => "runtime_status.background_cpu_active",
        _ => return message.to_owned(),
    };
    t!(key).to_string()
}

pub(super) fn adaptive_power_profile_label(profile: &str) -> String {
    match profile {
        "Idle" => t!("adaptive_engine.profile_idle"),
        "Responsive" => t!("adaptive_engine.profile_responsive"),
        "Sustained" => t!("adaptive_engine.profile_sustained"),
        "Burst" => t!("adaptive_engine.profile_burst"),
        _ => t!("common.unknown"),
    }
    .to_string()
}

pub(super) fn weekday_short_label(day: WeekdaySetting) -> String {
    match day {
        WeekdaySetting::Mon => t!("weekday.mon"),
        WeekdaySetting::Tue => t!("weekday.tue"),
        WeekdaySetting::Wed => t!("weekday.wed"),
        WeekdaySetting::Thu => t!("weekday.thu"),
        WeekdaySetting::Fri => t!("weekday.fri"),
        WeekdaySetting::Sat => t!("weekday.sat"),
        WeekdaySetting::Sun => t!("weekday.sun"),
    }
    .to_string()
}

pub(super) fn cpu_usage_comparison_label(comparison: CpuUsageComparison) -> String {
    match comparison {
        CpuUsageComparison::AtOrAbove => t!("by_cpu_load.comparison_at_or_above"),
        CpuUsageComparison::AtOrBelow => t!("by_cpu_load.comparison_at_or_below"),
        CpuUsageComparison::Between => t!("by_cpu_load.comparison_between"),
        CpuUsageComparison::Else => t!("by_cpu_load.comparison_else"),
    }
    .to_string()
}
