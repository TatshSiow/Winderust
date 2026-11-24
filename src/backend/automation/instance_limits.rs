use super::*;
use crate::{
    action_log::{ActionLogFeature, ActionLogResult},
    control::instance_limits::{self, CountObservation},
    features::watchdog::instance_limits::InstanceLimitsRuleSnapshot,
};
use rust_i18n::t;

#[derive(Default)]
pub(super) struct PendingResets {
    pub(super) all: bool,
    pub(super) notification_opt_in: bool,
    pub(super) notification_paths: Vec<String>,
    pub(super) paths: Vec<String>,
}

pub(super) fn record_settings_changes(
    state: &mut AutomationWorkerState,
    settings: &Settings,
) -> bool {
    let previous = &state.settings;
    let pending = &mut state.pending_instance_limits_resets;
    pending.all |= previous.general.enabled != settings.general.enabled
        || previous.instance_limits.enabled != settings.instance_limits.enabled;
    pending.notification_opt_in |=
        !previous.instance_limits.notify && settings.instance_limits.notify;
    for (rules, other) in [
        (
            &previous.instance_limits.rules,
            &settings.instance_limits.rules,
        ),
        (
            &settings.instance_limits.rules,
            &previous.instance_limits.rules,
        ),
    ] {
        for rule in rules
            .iter()
            .filter(|rule| !other.iter().any(|other| rule.same_monitor(other)))
        {
            if !pending.paths.contains(&rule.executable_path) {
                pending.paths.push(rule.executable_path.clone());
            }
        }
    }
    if settings.instance_limits.notify {
        for rule in settings
            .instance_limits
            .rules
            .iter()
            .filter(|rule| rule.enabled && rule.notify)
        {
            if previous
                .instance_limits
                .rules
                .iter()
                .any(|old| !old.notify && rule.same_monitor(old))
                && !pending.notification_paths.contains(&rule.executable_path)
            {
                pending
                    .notification_paths
                    .push(rule.executable_path.clone());
            }
        }
    }

    let affected = |rule: &InstanceLimitsRuleSnapshot| {
        pending.all || pending.paths.contains(&rule.executable_path)
    };
    if !state
        .status
        .feature_status
        .instance_limits
        .rules
        .iter()
        .any(affected)
    {
        return false;
    }
    Arc::make_mut(&mut state.status.feature_status)
        .instance_limits
        .rules
        .retain(|rule| !affected(rule));
    true
}

pub(super) fn publish_status(
    shared: &SharedAutomationState,
    settings: &Arc<Settings>,
    watcher_active: bool,
    status: InstanceLimitsSnapshot,
) {
    let mut state = lock_unpoisoned(&shared.state);
    let current = if status.enabled {
        sample_is_current(&state, settings, watcher_active)
    } else {
        // Disabling clears observations before snapshotting. The watcher can stop independently.
        !state.stop_requested && Arc::ptr_eq(&state.settings, settings)
    };
    if !current || state.status.feature_status.instance_limits == status {
        return;
    }
    Arc::make_mut(&mut state.status.feature_status).instance_limits = status;
    bump_status_generation(shared, &mut state);
}

impl RuntimeCore {
    pub(super) fn run_instance_limits_update(
        &mut self,
        shared: &SharedAutomationState,
        settings: &Arc<Settings>,
        watcher_active: bool,
    ) -> InstanceLimitsSnapshot {
        if !instance_limits_required(settings) {
            return self.instance_limits.snapshot();
        }
        if !watcher_active {
            self.instance_limits
                .invalidate(&t!("instance_limits.waiting_context"));
            return self.instance_limits.snapshot();
        }
        let mut counts = vec![
            CountObservation::Unavailable {
                reason: "The rule is disabled.".into(),
            };
            settings.instance_limits.rules.len()
        ];
        let enabled: Vec<_> = settings
            .instance_limits
            .rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.enabled)
            .collect();
        let paths: Vec<_> = enabled
            .iter()
            .map(|(_, rule)| rule.executable_path.clone())
            .collect();
        // Native queries use one fresh catalog and never hold the shared intake mutex.
        let observed = instance_limits::collect(&paths);
        if observed.len() == enabled.len() {
            for ((index, _), count) in enabled.into_iter().zip(observed) {
                counts[index] = count;
            }
        } else {
            counts.fill(CountObservation::Unavailable {
                reason: "Instance Limits received a mismatched collector result.".into(),
            });
        }
        let events = {
            let state = lock_unpoisoned(&shared.state);
            if sample_is_current(&state, settings, watcher_active) {
                self.instance_limits.observe(&counts, Instant::now())
            } else {
                self.instance_limits
                    .invalidate(&t!("instance_limits.discarded_sample"));
                Vec::new()
            }
        };
        for event in events {
            let index = match &event {
                InstanceLimitsEvent::Excess(index)
                | InstanceLimitsEvent::Recovered(index)
                | InstanceLimitsEvent::Notify(index)
                | InstanceLimitsEvent::Error(index, _) => *index,
            };
            let rule = &settings.instance_limits.rules[index];
            let count = match counts[index] {
                CountObservation::Complete(count) => count,
                CountObservation::Incomplete {
                    known_lower_bound, ..
                } => known_lower_bound,
                CountObservation::Unavailable { .. } | CountObservation::Conflict => 0,
            };
            let maximum = rule.max_instances;
            let (outcome, reason) = match event {
                InstanceLimitsEvent::Excess(_) => (
                    ActionLogResult::Skipped,
                    t!(
                        "instance_limits.excess_log",
                        count = count,
                        maximum = maximum
                    )
                    .to_string(),
                ),
                InstanceLimitsEvent::Recovered(_) => (
                    ActionLogResult::Skipped,
                    t!(
                        "instance_limits.recovered_log",
                        count = count,
                        maximum = maximum
                    )
                    .to_string(),
                ),
                InstanceLimitsEvent::Error(_, error) => (ActionLogResult::Failed, error),
                InstanceLimitsEvent::Notify(_) => {
                    let result = tray::show_notification(
                        &t!("instance_limits.notification_title"),
                        &t!(
                            "instance_limits.notification_body",
                            path = rule.executable_path,
                            count = count,
                            maximum = maximum
                        ),
                        false,
                    )
                    .map_err(tray::NotificationError::localized);
                    let report = match &result {
                        Ok(()) => (
                            ActionLogResult::Applied,
                            t!("instance_limits.notification_accepted").to_string(),
                        ),
                        Err(error) => (
                            ActionLogResult::Failed,
                            t!("instance_limits.notification_failed", error = error).to_string(),
                        ),
                    };
                    if let Err(error) = self.instance_limits.notification_result(index, result) {
                        update_worker_error(shared, Some(error));
                    }
                    report
                }
            };
            self.action_log.record(
                ActionLogFeature::InstanceLimits,
                None,
                &rule.executable_path,
                outcome,
                reason,
            );
        }
        self.instance_limits.snapshot()
    }
}

fn sample_is_current(
    state: &AutomationWorkerState,
    settings: &Arc<Settings>,
    watcher_active: bool,
) -> bool {
    !state.stop_requested
        && Arc::ptr_eq(&state.settings, settings)
        && state.windows_event_watcher_active == watcher_active
        && !state.pending_events.suspend_resume
        && !state.pending_events.session_changed
        && state.pending_events.windows_event_watcher_change.is_none()
        && !state.pending_instance_limits_resets.all
        && state.pending_instance_limits_resets.paths.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::settings::RuntimeSettingsSnapshot;

    fn runtime() -> RuntimeHandle {
        RuntimeHandle::start(&RuntimeSettingsSnapshot {
            value: Arc::new(Settings::default()),
            runtime_revision: SettingsRevision::initial(),
            persisted_revision: SettingsRevision::initial(),
        })
    }

    #[test]
    fn samples_cannot_cross_settings_session_watcher_or_shutdown_changes() {
        let runtime = runtime();
        let snapshot = automation_snapshot(&runtime.shared).unwrap();
        let mut state = lock_unpoisoned(&runtime.shared.state);
        let expected = snapshot.settings;
        let watcher = snapshot.windows_event_watcher_active;
        assert!(sample_is_current(&state, &expected, watcher));
        state.settings = Arc::new((*expected).clone());
        assert!(!sample_is_current(&state, &expected, watcher));
        state.settings = Arc::clone(&expected);
        state.pending_events.session_changed = true;
        assert!(!sample_is_current(&state, &expected, watcher));
        state.pending_events.session_changed = false;
        state.pending_events.suspend_resume = true;
        assert!(!sample_is_current(&state, &expected, watcher));
        state.pending_events.suspend_resume = false;
        assert!(!sample_is_current(&state, &expected, !watcher));
        state.windows_event_watcher_active = true;
        drop(state);
        set_windows_event_watcher_active(&runtime.shared, false);
        set_windows_event_watcher_active(&runtime.shared, true);
        let state = lock_unpoisoned(&runtime.shared.state);
        assert!(state.windows_event_watcher_active);
        assert!(!sample_is_current(&state, &expected, true));
        drop(state);
        let snapshot = automation_snapshot(&runtime.shared).unwrap();
        assert_eq!(
            snapshot.wake_events.windows_event_watcher_change,
            Some(WindowsEventWatcherChange::Lost)
        );
        let mut state = lock_unpoisoned(&runtime.shared.state);
        assert!(sample_is_current(&state, &expected, true));
        state.stop_requested = true;
        assert!(!sample_is_current(&state, &expected, true));
    }

    #[test]
    fn watcher_outage_clears_sample_age() {
        let runtime = runtime();
        let settings = {
            let mut state = lock_unpoisoned(&runtime.shared.state);
            state.windows_event_watcher_active = false;
            state.pending_events = AutomationWakeEvents::default();
            let mut settings = (*state.settings).clone();
            settings.general.enabled = true;
            settings.instance_limits.enabled = true;
            settings
                .instance_limits
                .rules
                .push(crate::config::InstanceLimitsRule::new(
                    r"C:\Apps\app.exe".into(),
                ));
            state.settings = Arc::new(settings);
            Arc::clone(&state.settings)
        };
        let mut runner = RuntimeCore::default();
        runner.instance_limits.sync(&settings.instance_limits, true);
        runner
            .instance_limits
            .observe(&[CountObservation::Complete(1)], Instant::now());
        let paused = runner.run_instance_limits_update(&runtime.shared, &settings, false);
        assert_eq!(paused.rules[0].sampled_at, None);
        assert_eq!(
            paused.rules[0].count,
            CountObservation::Unavailable {
                reason: t!("instance_limits.waiting_context").to_string(),
            }
        );
    }

    #[test]
    fn brief_toggles_and_reverted_rule_edits_remain_reset_intents() {
        let runtime = runtime();
        let mut state = lock_unpoisoned(&runtime.shared.state);
        let mut settings = (*state.settings).clone();
        settings
            .instance_limits
            .rules
            .push(crate::config::InstanceLimitsRule::new(
                r"C:\Apps\app.exe".into(),
            ));
        state.settings = Arc::new(settings.clone());
        settings.instance_limits.notify = !settings.instance_limits.notify;
        record_settings_changes(&mut state, &settings);
        assert!(!state.pending_instance_limits_resets.all);
        assert!(state.pending_instance_limits_resets.paths.is_empty());
        state.settings = Arc::new(settings.clone());
        settings.instance_limits.rules[0].max_instances = 2;
        record_settings_changes(&mut state, &settings);
        state.settings = Arc::new(settings.clone());
        settings.instance_limits.rules[0].max_instances = 1;
        record_settings_changes(&mut state, &settings);
        assert_eq!(
            state.pending_instance_limits_resets.paths,
            [r"C:\Apps\app.exe"]
        );
        settings.general.enabled = !settings.general.enabled;
        record_settings_changes(&mut state, &settings);
        state.settings = Arc::new(settings.clone());
        settings.general.enabled = !settings.general.enabled;
        record_settings_changes(&mut state, &settings);
        assert!(state.pending_instance_limits_resets.all);
    }

    #[test]
    fn notify_opt_in_between_worker_snapshots_restarts_qualification() {
        let runtime = runtime();
        let mut state = lock_unpoisoned(&runtime.shared.state);
        let mut settings = (*state.settings).clone();
        settings.instance_limits.enabled = true;
        settings.instance_limits.notify = true;
        settings.instance_limits.rules = vec![crate::config::InstanceLimitsRule::new(
            r"C:\Apps\app.exe".into(),
        )];
        state.settings = Arc::new(settings.clone());
        let mut manager = InstanceLimitsManager::default();
        manager.sync(&settings.instance_limits, true);
        let now = Instant::now();
        let counts = [CountObservation::Complete(2)];
        assert!(manager.observe(&counts, now).is_empty());

        for notify in [false, true] {
            settings.instance_limits.notify = notify;
            record_settings_changes(&mut state, &settings);
            state.settings = Arc::new(settings.clone());
        }
        let pending = std::mem::take(&mut state.pending_instance_limits_resets);
        assert!(!pending.all && pending.paths.is_empty());
        if pending.notification_opt_in {
            manager.requalify_notifications();
        }
        manager.sync(&settings.instance_limits, true);
        assert!(manager
            .observe(&counts, now + Duration::from_secs(1))
            .is_empty());
        assert_eq!(
            manager.observe(&counts, now + Duration::from_secs(2)),
            [
                InstanceLimitsEvent::Excess(0),
                InstanceLimitsEvent::Notify(0)
            ]
        );
    }

    #[test]
    fn rapid_rule_notify_toggle_restarts_only_that_rules_notification_qualification() {
        let runtime = runtime();
        let mut state = lock_unpoisoned(&runtime.shared.state);
        let mut settings = (*state.settings).clone();
        settings.instance_limits.enabled = true;
        settings.instance_limits.rules = [r"C:\Apps\app.exe", r"C:\Apps\other.exe"]
            .into_iter()
            .map(|path| crate::config::InstanceLimitsRule::new(path.into()))
            .collect();
        state.settings = Arc::new(settings.clone());

        let mut manager = InstanceLimitsManager::default();
        manager.sync(&settings.instance_limits, true);
        let start = Instant::now();
        let counts = [CountObservation::Complete(2), CountObservation::Complete(2)];
        assert!(manager.observe(&counts, start).is_empty());

        for notify in [false, true] {
            settings.instance_limits.rules[0].notify = notify;
            record_settings_changes(&mut state, &settings);
            state.settings = Arc::new(settings.clone());
        }
        let pending = std::mem::take(&mut state.pending_instance_limits_resets);
        assert!(pending.paths.is_empty());
        assert_eq!(pending.notification_paths, [r"C:\Apps\app.exe"]);
        manager.requalify_rule_notifications(&pending.notification_paths);
        manager.sync(&settings.instance_limits, true);
        assert_eq!(
            manager.observe(&counts, start + Duration::from_secs(1)),
            [
                InstanceLimitsEvent::Excess(0),
                InstanceLimitsEvent::Excess(1),
                InstanceLimitsEvent::Notify(1),
            ],
        );
        assert_eq!(
            manager.observe(&counts, start + Duration::from_secs(2)),
            [InstanceLimitsEvent::Notify(0)],
        );
    }

    #[test]
    fn independent_monitor_keeps_one_second_deadline_in_tray_and_saver_modes() {
        let mut settings = Settings::default();
        settings.general.enabled = true;
        settings.instance_limits.enabled = true;
        settings
            .instance_limits
            .rules
            .push(crate::config::InstanceLimitsRule::new(
                r"C:\Apps\app.exe".into(),
            ));
        assert!(automation_worker_required(&settings));
        assert!(windows_event_watcher_required(&settings));
        for event in [
            WindowsAutomationEvent::SuspendResume,
            WindowsAutomationEvent::SessionChanged,
        ] {
            assert!(windows_event_wake_required(&settings, event));
        }
        assert!(!power_plan_checks_required(&settings));
        assert_eq!(input_hook_config(&settings), InputHookConfig::default());
        let now = Instant::now();
        let mut scheduler = RefreshScheduler::new(now);
        scheduler.schedule_after(
            RefreshDomain::InstanceLimits,
            now,
            INSTANCE_LIMITS_REFRESH_INTERVAL,
        );
        for (tray, adaptive) in [(false, false), (true, false), (true, true)] {
            let automation_interval =
                automation_refresh_interval(tray, adaptive, HIDDEN_AUTOMATION_REFRESH_INTERVAL);
            assert_eq!(
                scheduler.minimum_wait(
                    Some(automation_interval),
                    now,
                    [(
                        true,
                        RefreshDomain::InstanceLimits,
                        INSTANCE_LIMITS_REFRESH_INTERVAL
                    )]
                ),
                Some(Duration::from_secs(1))
            );
        }
        settings.general.enabled = false;
        assert!(!automation_worker_required(&settings));
    }
}
