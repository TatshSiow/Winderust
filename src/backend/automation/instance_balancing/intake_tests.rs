use super::*;
use crate::application::settings::RuntimeSettingsSnapshot;

fn runtime_handle() -> RuntimeHandle {
    RuntimeHandle::start(&RuntimeSettingsSnapshot {
        value: Arc::new(Settings::default()),
        runtime_revision: SettingsRevision::initial(),
        persisted_revision: SettingsRevision::initial(),
    })
}

fn settings() -> Settings {
    Settings {
        instance_balancing: InstanceBalancingSettings {
            enabled: true,
            rules: [r"C:\Apps\first.exe", r"C:\Apps\second.exe"]
                .into_iter()
                .map(|path| InstanceBalancingRule {
                    enabled: true,
                    executable_path: path.into(),
                })
                .collect(),
        },
        ..Default::default()
    }
}

#[test]
fn rapid_toggles_and_reverted_edits_survive_until_snapshot_intake() {
    let runtime = runtime_handle();
    let original = settings();
    {
        let mut state = lock_unpoisoned(&runtime.shared.state);
        for toggle in [
            (|settings: &mut Settings| settings.instance_balancing.enabled = false)
                as fn(&mut Settings),
            |settings| settings.general.enabled = false,
        ] {
            state.settings = Arc::new(original.clone());
            state.pending_instance_balancing_resets = PendingResets::default();
            let mut changed = original.clone();
            toggle(&mut changed);
            record_settings_changes(&mut state, &changed);
            state.settings = Arc::new(changed);
            record_settings_changes(&mut state, &original);
            state.settings = Arc::new(original.clone());
            assert!(state.pending_instance_balancing_resets.all);
            assert!(state.pending_instance_balancing_resets.paths.is_empty());
        }
        state.pending_instance_balancing_resets = PendingResets::default();
        let mut edited = original.clone();
        edited.instance_balancing.rules[0].executable_path = r"C:\Apps\replacement.exe".into();
        record_settings_changes(&mut state, &edited);
        state.settings = Arc::new(edited);
        record_settings_changes(&mut state, &original);
        state.settings = Arc::new(original);
        assert!(!state.pending_instance_balancing_resets.all);
    }
    let snapshot = automation_snapshot(&runtime.shared).unwrap();
    assert_eq!(
        snapshot.instance_balancing_resets.paths,
        [r"C:\Apps\first.exe", r"C:\Apps\replacement.exe"]
    );
    let next = automation_snapshot(&runtime.shared).unwrap();
    assert!(!next.instance_balancing_resets.all);
    assert!(next.instance_balancing_resets.paths.is_empty());
}

#[test]
fn inactive_rules_do_not_keep_invalidated_runtime_polling() {
    let mut configured = settings();
    let mut runtime = Runtime::default();
    for rules in [Vec::new(), {
        let mut rules = configured.instance_balancing.rules.clone();
        for rule in &mut rules {
            rule.enabled = false;
        }
        rules
    }] {
        configured.instance_balancing.rules = rules;
        runtime.sync(
            &configured.instance_balancing,
            true,
            PendingResets::default(),
        );
        assert!(runtime.active(), "An explicit edit still needs one cleanup");
        runtime.dirty = false;
        for _ in 0..3 {
            runtime.invalidate();
            assert!(!runtime.active());
        }
    }
    configured.instance_balancing.rules = settings().instance_balancing.rules;
    runtime.sync(
        &configured.instance_balancing,
        true,
        PendingResets::default(),
    );
    runtime.dirty = false;
    runtime.invalidate();
    assert!(
        runtime.active(),
        "Enabled rules need a pass after the observation context changes"
    );
}

#[test]
fn only_explicitly_rearmed_rules_get_new_generations() {
    let mut configured = settings();
    let mut runtime = Runtime::default();
    runtime.sync(
        &configured.instance_balancing,
        configured.general.enabled,
        PendingResets::default(),
    );
    let initial = [runtime.rules[0].generation, runtime.rules[1].generation];
    assert!(initial.iter().all(Option::is_some));

    configured.general.check_for_updates = !configured.general.check_for_updates;
    configured.battery_profile_mut().cpu_limiter.enabled = true;
    for plugged_in in [Some(true), Some(false)] {
        let active = active_power_source_settings(&configured, plugged_in);
        assert_eq!(active.instance_balancing, configured.instance_balancing);
        runtime.sync(
            &configured.instance_balancing,
            configured.general.enabled,
            PendingResets::default(),
        );
        assert_eq!(
            [runtime.rules[0].generation, runtime.rules[1].generation],
            initial
        );
    }
    runtime.invalidate();
    assert_eq!(
        [runtime.rules[0].generation, runtime.rules[1].generation],
        initial
    );

    runtime.sync(
        &configured.instance_balancing,
        true,
        PendingResets {
            all: false,
            paths: vec![configured.instance_balancing.rules[0]
                .executable_path
                .clone()],
        },
    );
    let reverted_edit_generation = runtime.rules[0].generation.unwrap();
    assert!(reverted_edit_generation > initial[0].unwrap());
    assert_eq!(runtime.rules[1].generation, initial[1]);

    configured.instance_balancing.rules[0].enabled = false;
    runtime.sync(
        &configured.instance_balancing,
        true,
        PendingResets::default(),
    );
    let edited_generation = runtime.rules[0].generation.unwrap();
    assert!(edited_generation > reverted_edit_generation);
    assert_eq!(runtime.rules[1].generation, initial[1]);

    configured.instance_balancing.rules[0].enabled = true;
    runtime.sync(
        &configured.instance_balancing,
        true,
        PendingResets::default(),
    );
    assert!(runtime.rules[0].generation.unwrap() > edited_generation);
    assert_eq!(runtime.rules[1].generation, initial[1]);

    let before_global_rearm = [runtime.rules[0].generation, runtime.rules[1].generation];
    runtime.sync(
        &configured.instance_balancing,
        true,
        PendingResets {
            all: true,
            paths: Vec::new(),
        },
    );
    for (rule, previous) in runtime.rules.iter().zip(before_global_rearm) {
        assert!(rule.generation.unwrap() > previous.unwrap());
    }
}

#[test]
fn context_gate_rejects_pending_intents_and_watcher_interruptions() {
    let runtime = runtime_handle();
    let configured = {
        let mut configured = Settings::default();
        configured.battery_profile_mut();
        Arc::new(configured)
    };
    let reset = || {
        let mut state = lock_unpoisoned(&runtime.shared.state);
        state.settings = Arc::clone(&configured);
        state.windows_event_watcher_active = true;
        state.pending_events = AutomationWakeEvents::default();
        state.pending_instance_balancing_resets = PendingResets::default();
        state.stop_requested = false;
    };
    reset();
    assert!(context_is_current(
        &runtime.shared,
        &configured,
        &configured
    ));
    type Invalidate = fn(&mut AutomationWorkerState);
    let invalidations: [(&str, Invalidate); 7] = [
        ("suspend", |state| {
            state.pending_events.suspend_resume = true
        }),
        ("session", |state| {
            state.pending_events.session_changed = true
        }),
        ("watcher lost", |state| {
            state.windows_event_watcher_active = false
        }),
        ("global reset", |state| {
            state.pending_instance_balancing_resets.all = true
        }),
        ("rule reset", |state| {
            state
                .pending_instance_balancing_resets
                .paths
                .push(r"C:\Apps\first.exe".into())
        }),
        ("shutdown", |state| state.stop_requested = true),
        ("stale settings", |state| {
            state.settings = Arc::new((*state.settings).clone())
        }),
    ];
    for (reason, invalidate) in invalidations {
        reset();
        invalidate(&mut lock_unpoisoned(&runtime.shared.state));
        assert!(
            !context_is_current(&runtime.shared, &configured, &configured),
            "{reason} must invalidate a prepared cohort"
        );
    }
    reset();
    set_windows_event_watcher_active(&runtime.shared, false);
    set_windows_event_watcher_active(&runtime.shared, true);
    assert!(lock_unpoisoned(&runtime.shared.state).windows_event_watcher_active);
    assert!(!context_is_current(
        &runtime.shared,
        &configured,
        &configured
    ));
    reset();
}
