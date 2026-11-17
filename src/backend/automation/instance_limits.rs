use super::*;
use crate::{
    action_log::{ActionLogFeature, ActionLogResult},
    control::instance_limits::{self, CountObservation},
};
use rust_i18n::t;

#[derive(Default)]
pub(super) struct PendingResets {
    pub(super) all: bool,
    pub(super) paths: Vec<String>,
}

pub(super) fn record_settings_changes(state: &mut AutomationWorkerState, settings: &Settings) {
    let previous = &state.settings;
    state.pending_instance_limits_resets.all |= previous.general.enabled
        != settings.general.enabled
        || previous.instance_limits.enabled != settings.instance_limits.enabled;
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
        for rule in rules.iter().filter(|rule| !other.contains(rule)) {
            if !state
                .pending_instance_limits_resets
                .paths
                .contains(&rule.executable_path)
            {
                state
                    .pending_instance_limits_resets
                    .paths
                    .push(rule.executable_path.clone());
            }
        }
    }
}

impl RuntimeCore {
    pub(super) fn run_instance_limits_update(
        &mut self,
        shared: &SharedAutomationState,
        settings: &Arc<Settings>,
        watcher_active: bool,
    ) -> InstanceLimitsSnapshot {
        if !instance_limits_required(settings) {
            return self.instance_limits.snapshot(Instant::now());
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
        let observed = if watcher_active {
            instance_limits::collect(&paths)
        } else {
            vec![
                CountObservation::Unavailable {
                    reason: t!("instance_limits.waiting_context").to_string(),
                };
                paths.len()
            ]
        };
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
        let snapshot = self.instance_limits.snapshot(Instant::now());
        for event in events {
            let index = match &event {
                InstanceLimitsEvent::Excess(index)
                | InstanceLimitsEvent::Recovered(index)
                | InstanceLimitsEvent::Notify(index)
                | InstanceLimitsEvent::Error(index, _) => *index,
            };
            let rule = &snapshot.rules[index];
            let count = match rule.count {
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
                    );
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
        self.instance_limits.snapshot(Instant::now())
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
        let mut state = lock_unpoisoned(&runtime.shared.state);
        let expected = Arc::clone(&state.settings);
        let watcher = state.windows_event_watcher_active;
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
        state.stop_requested = true;
        assert!(!sample_is_current(&state, &expected, watcher));
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
        assert!(!input_hook_required(&settings));
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
