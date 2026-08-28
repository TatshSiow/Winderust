use crate::ui::app::*;

pub(in crate::ui::app) fn priority_level_label(
    target: PriorityDefaultTarget,
    priority_type: String,
) -> String {
    let target = match target {
        PriorityDefaultTarget::Foreground => t!("common.focus_process"),
        PriorityDefaultTarget::VisibleWindow => t!("common.visible_window"),
        PriorityDefaultTarget::Background => t!("common.background_process"),
    };
    format!("{target} {priority_type} {}", t!("common.level"))
}

pub(in crate::ui::app) fn theme_mode_label(mode: AppThemeMode) -> String {
    match mode {
        AppThemeMode::System => t!("theme.system").to_string(),
        AppThemeMode::Light => t!("theme.light").to_string(),
        AppThemeMode::Dark => t!("theme.dark").to_string(),
    }
}

pub(in crate::ui::app) fn update_channel_label(channel: UpdateChannel) -> String {
    match channel {
        UpdateChannel::Stable => t!("update_channel.stable").to_string(),
        UpdateChannel::PreRelease => t!("update_channel.pre_release").to_string(),
    }
}

pub(in crate::ui::app) fn animation_mode_label(mode: AnimationMode) -> String {
    match mode {
        AnimationMode::System => t!("animation.system").to_string(),
        AnimationMode::On => t!("common.on").to_string(),
        AnimationMode::Off => t!("common.off").to_string(),
    }
}

pub(in crate::ui::app) fn accent_source_label(source: AccentColorSource) -> String {
    match source {
        AccentColorSource::Windows => t!("theme.system").to_string(),
        AccentColorSource::Custom => t!("accent.custom").to_string(),
    }
}

pub(in crate::ui::app) fn action_log_action_label(result: ActionLogResult) -> &'static str {
    match result {
        ActionLogResult::Applied => "Apply",
        ActionLogResult::Restored => "Restore",
        ActionLogResult::Skipped => "Skip",
        ActionLogResult::Failed => "Fail",
    }
}

pub(in crate::ui::app) fn action_log_entries_to_csv(entries: &[ActionLogEntry]) -> String {
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

pub(in crate::ui::app) fn action_log_export_time_label(timestamp_epoch_ms: u128) -> String {
    let timestamp = timestamp_epoch_ms.min(i64::MAX as u128) as i64;
    Local
        .timestamp_millis_opt(timestamp)
        .single()
        .map(|time| time.format("%Y-%m-%d %H:%M:%S%.3f %:z").to_string())
        .unwrap_or_else(|| timestamp_epoch_ms.to_string())
}

pub(in crate::ui::app) fn action_log_process_label(entry: &ActionLogEntry) -> String {
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

pub(in crate::ui::app) fn action_log_time_label(timestamp_epoch_ms: u128) -> String {
    let timestamp = timestamp_epoch_ms.min(i64::MAX as u128) as i64;
    Local
        .timestamp_millis_opt(timestamp)
        .single()
        .map(|time| time.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "--:--:--".to_owned())
}

pub(in crate::ui::app) fn rule_count_label(count: usize) -> String {
    t!("common.rule_count", count = count).to_string()
}

pub(in crate::ui::app) fn schedule_days_label(days: &[WeekdaySetting]) -> String {
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

pub(in crate::ui::app) fn activity_state_label(state: ActivityState) -> String {
    match state {
        ActivityState::Active => t!("home.activity_active"),
        ActivityState::Idle => t!("home.activity_idle"),
        ActivityState::Unknown => t!("home.activity_unknown"),
    }
    .to_string()
}

pub(in crate::ui::app) fn localized_memory_trim_status(
    status: &memory_trim::MemoryTrimStatus,
) -> String {
    match status {
        memory_trim::MemoryTrimStatus::AutomationDisabled => {
            t!("runtime_status.automation_disabled").to_string()
        }
        memory_trim::MemoryTrimStatus::Disabled => {
            t!("runtime_status.memory_trim_disabled").to_string()
        }
        memory_trim::MemoryTrimStatus::ForegroundUnknown => {
            t!("runtime_status.foreground_unknown").to_string()
        }
        memory_trim::MemoryTrimStatus::WaitingForMemoryLoad { threshold_percent } => t!(
            "runtime_status.memory_trim_waiting",
            threshold = threshold_percent
        )
        .to_string(),
        memory_trim::MemoryTrimStatus::Active => {
            t!("runtime_status.memory_trim_active").to_string()
        }
        memory_trim::MemoryTrimStatus::ManualCompleted => {
            t!("runtime_status.memory_trim_manual_completed").to_string()
        }
        memory_trim::MemoryTrimStatus::Error(error) => error.clone(),
    }
}

pub(in crate::ui::app) fn localized_app_suspension_status(
    status: &app_suspension::AppSuspensionStatus,
) -> String {
    match status {
        app_suspension::AppSuspensionStatus::AutomationDisabled => {
            t!("runtime_status.automation_disabled").to_string()
        }
        app_suspension::AppSuspensionStatus::Disabled => {
            t!("runtime_status.app_suspension_disabled").to_string()
        }
        app_suspension::AppSuspensionStatus::NoRulesConfigured => {
            t!("runtime_status.app_suspension_no_rules").to_string()
        }
        app_suspension::AppSuspensionStatus::Unsupported => {
            t!("runtime_status.app_suspension_unsupported").to_string()
        }
        app_suspension::AppSuspensionStatus::ForegroundUnknown => {
            t!("runtime_status.foreground_unknown").to_string()
        }
        app_suspension::AppSuspensionStatus::SessionUnknown => {
            t!("runtime_status.session_unknown").to_string()
        }
        app_suspension::AppSuspensionStatus::Active => {
            t!("runtime_status.app_suspension_active").to_string()
        }
        app_suspension::AppSuspensionStatus::Error(error) => error.clone(),
    }
}

pub(in crate::ui::app) fn weekday_short_label(day: WeekdaySetting) -> String {
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

pub(in crate::ui::app) fn cpu_usage_comparison_label(comparison: CpuUsageComparison) -> String {
    match comparison {
        CpuUsageComparison::AtOrAbove => t!("by_cpu_load.comparison_at_or_above"),
        CpuUsageComparison::AtOrBelow => t!("by_cpu_load.comparison_at_or_below"),
        CpuUsageComparison::Between => t!("by_cpu_load.comparison_between"),
        CpuUsageComparison::Else => t!("by_cpu_load.comparison_else"),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_log_entries_export_as_csv() {
        let entries = vec![ActionLogEntry {
            sequence: 7,
            timestamp_epoch_ms: 1_700_000_000_000,
            feature: ActionLogFeature::CpuLimiter,
            process_id: Some(42),
            process_name: "worker.exe".to_owned(),
            result: ActionLogResult::Failed,
            reason: "Restart failed, access denied".to_owned(),
        }];

        let csv = action_log_entries_to_csv(&entries);

        assert!(csv.starts_with(
            "sequence,timestamp,feature,process_id,process_name,action,result,reason\r\n"
        ));
        assert!(csv.contains(
            ",CPU Limiter,42,worker.exe,Fail,Failed,\"Restart failed, access denied\"\r\n"
        ));
    }
}
