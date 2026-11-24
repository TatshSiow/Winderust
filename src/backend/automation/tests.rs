use super::*;
use chrono::{Datelike, Duration as ChronoDuration, Local};
use std::sync::Arc;

use crate::action_log::{ActionLogFeature, ActionLogResult};
use crate::application::settings::{RuntimeSettingsSnapshot, SettingsRevision};
use crate::config::{
    AppSuspensionRule, ByFocusRule, ByRunningAppRule, ByTimeRule, CpuAllocationRule,
    CpuLimiterRule, ExecutableSelector, ProcessRuleMode, ProcessThreadPrioritySetting,
    TimerResolutionRule, WeekdaySetting,
};

fn runtime_settings(snapshot: Settings) -> RuntimeSettingsSnapshot {
    runtime_settings_with_runtime_and_persisted_revisions(
        snapshot,
        SettingsRevision::initial(),
        SettingsRevision::initial(),
    )
}

#[test]
fn resource_thresholds_alone_keeps_automation_alive_without_power_plan_rules() {
    let mut settings = Settings::default();
    settings.general.enabled = true;
    assert!(!automation_worker_required(&settings));
    settings.resource_thresholds.enabled = true;
    settings
        .resource_thresholds
        .rules
        .push(crate::config::ResourceThresholdsRule::new(
            r"C:\Apps\test.exe".into(),
        ));
    assert!(automation_worker_required(&settings));
    assert!(windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::PowerChanged
    ));
    assert!(!power_plan_checks_required(&settings));
    settings.resource_thresholds.rules[0].enabled = false;
    assert!(!automation_worker_required(&settings));
    settings.resource_thresholds.rules[0].enabled = true;
    settings.general.enabled = false;
    assert!(!automation_worker_required(&settings));
}

#[test]
fn sleep_prevention_keeps_battery_only_automation_and_sleep_events_alive() {
    let mut settings = Settings::default();
    settings.general.enabled = true;
    let mut battery = settings.clone();
    battery.sleep_prevention.enabled = true;
    assert!(!automation_worker_required(&battery));
    battery
        .sleep_prevention
        .rules
        .push(crate::config::SleepPreventionRule {
            enabled: true,
            executable_path: r"C:\Apps\worker.exe".into(),
            keep_display_on: false,
        });
    settings.on_battery = Some(Box::new(battery));
    assert!(automation_worker_required(&settings));
    assert!(windows_event_watcher_required(&settings));
    for event in [
        WindowsAutomationEvent::PowerChanged,
        WindowsAutomationEvent::SuspendResume,
        WindowsAutomationEvent::SessionChanged,
        WindowsAutomationEvent::WindowCreated,
    ] {
        assert!(windows_event_wake_required(&settings, event));
    }
    assert!(!windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::ForegroundChanged,
    ));
    assert!(!power_plan_checks_required(settings.battery_profile()));
    assert_eq!(input_hook_config(&settings), InputHookConfig::default());
    settings.on_battery.as_mut().unwrap().sleep_prevention.rules[0].enabled = false;
    assert!(!automation_worker_required(&settings));
}

#[test]
fn instance_balancing_owns_a_worker_and_context_events_without_process_scans() {
    let mut settings = Settings::default();
    settings.general.theme_mode = AppThemeMode::Amoled;
    settings.general.accent.source = AccentColorSource::Custom;
    settings.instance_balancing.enabled = true;
    assert!(!instance_balancing_required(&settings));
    settings
        .instance_balancing
        .rules
        .push(crate::config::InstanceBalancingRule {
            enabled: true,
            executable_path: r"C:\Apps\worker.exe".into(),
        });
    assert!(instance_balancing_required(&settings));
    assert!(automation_worker_required(&settings));
    assert!(windows_event_watcher_required(&settings));
    assert!(!process_appearance_scan_required(&settings));
    assert!(!power_plan_checks_required(&settings));
    for event in [
        WindowsAutomationEvent::SuspendResume,
        WindowsAutomationEvent::SessionChanged,
    ] {
        assert!(windows_event_wake_required(&settings, event));
    }
    settings.instance_balancing.rules[0].enabled = false;
    assert!(!automation_worker_required(&settings));
    assert!(!windows_event_watcher_required(&settings));
    settings.instance_balancing.rules[0].enabled = true;
    settings.general.enabled = false;
    assert!(!instance_balancing_required(&settings));
    assert!(!automation_worker_required(&settings));
}

fn runtime_settings_with_runtime_and_persisted_revisions(
    snapshot: Settings,
    runtime_revision: SettingsRevision,
    persisted_revision: SettingsRevision,
) -> RuntimeSettingsSnapshot {
    RuntimeSettingsSnapshot {
        runtime_revision,
        persisted_revision,
        value: Arc::new(snapshot),
    }
}

fn app_suspension_rule(executable_path: &str) -> AppSuspensionRule {
    AppSuspensionRule {
        enabled: true,
        executable_path: executable_path.to_owned(),
        network_wake_enabled: false,
        audio_wake_enabled: false,
        network_download_threshold_bytes: 0,
        network_download_threshold_unit: Default::default(),
        network_upload_threshold_bytes: 0,
        network_upload_threshold_unit: Default::default(),
    }
}

// These scoped source assertions lock runtime ordering and ownership boundaries that span
// multiple managers.
fn source_scope<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let body_start = source
        .find(start)
        .expect("characterized function body should have its start marker");
    let body = &source[body_start..];
    let body_end = body
        .find(end)
        .expect("characterized function body should have its end marker");
    &body[..body_end]
}

fn assert_source_call_order(source: &str, start: &str, end: &str, calls: &[&str]) {
    let body = source_scope(source, start, end);

    let mut previous = 0;
    for call in calls {
        let index = body
            .find(call)
            .unwrap_or_else(|| panic!("characterized function body should call {call}"));
        assert!(
            index >= previous,
            "{call} should follow the preceding feature operation"
        );
        previous = index;
    }
}

#[test]
fn process_appearance_detector_ignores_initial_snapshot() {
    let mut known = Vec::new();

    assert!(!process_ids_have_new_entries(&mut known, vec![1, 2]));
    assert_eq!(known, [1, 2]);
}

#[test]
fn process_appearance_detector_reports_new_process_ids() {
    let mut known = vec![1, 2];

    assert!(process_ids_have_new_entries(&mut known, vec![1, 2, 3]));
    assert_eq!(known, [1, 2, 3]);
}

#[test]
fn automation_event_deadline_invalidations_are_characterized() {
    let source = include_str!("../automation.rs");
    assert!(source.contains("RefreshScheduler::new"));
    assert!(source.contains("SchedulerEvent::SettingsChanged"));
    assert!(source.contains("SchedulerEvent::ForegroundChanged"));
    assert!(source.contains("SchedulerEvent::WindowCreated"));
    assert!(source.contains("SchedulerEvent::ProcessAppeared"));
}

#[test]
fn poisoned_automation_mutex_is_recovered() {
    let mutex = Mutex::new(42);
    let _ = std::panic::catch_unwind(|| {
        let _guard = mutex.lock().expect("test mutex starts healthy");
        panic!("poison test mutex");
    });

    assert_eq!(*lock_unpoisoned(&mutex), 42);
}

#[test]
fn automation_worker_error_is_delivered_once() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
    update_worker_error(
        &automation.shared,
        Some("Background automation worker stopped unexpectedly.".to_owned()),
    );

    let snapshot = automation
        .status_snapshot_since(1)
        .expect("worker failure advances status generation");
    assert_eq!(
        snapshot.worker_error.as_deref(),
        Some("Background automation worker stopped unexpectedly.")
    );
    assert!(lock_unpoisoned(&automation.shared.state)
        .status
        .worker_error
        .is_none());
    assert!(automation
        .status_snapshot_since(snapshot.generation)
        .is_none());
}

#[test]
fn power_plan_status_is_published_as_an_independent_runtime_segment() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
    let status = PowerPlanStatus {
        active_plan_read_error: None,
        adaptive_processor_power_error: None,
        adaptive_processor_power_waiting_for_source: true,
        apply_failed: false,
        restoring_previous: false,
        rule_index: Some(0),
        owner: Some(crate::control::power_plan::PowerPlanOwner::OrdinaryAutomation),
        current_guid: Some("current".to_owned()),
        target_guid: Some("target".to_owned()),
        decision_state: Some(crate::rules::DecisionState::ByCpuLoad),
        reason: Some("test".to_owned()),
        cpu_load_match: Some(crate::features::power_plan_control::ByCpuLoadMatch::Condition),
        competing_modes: Vec::new(),
    };

    let configured = Arc::new(Settings::default());
    let context = PowerPlanRuleContext {
        settings: Arc::clone(&configured),
        source: PowerSourceProfile::PluggedIn,
    };
    update_power_plan_status(&automation.shared, status.clone(), Some(&context), None);

    let snapshot = automation
        .status_snapshot_since(1)
        .expect("power-plan status advances the status generation");
    assert_eq!(snapshot.power_plan_status.as_ref(), &status);
    assert_eq!(snapshot.power_plan_rule_context, Some(context.clone()));
    update_power_plan_status(&automation.shared, status.clone(), Some(&context), None);
    assert!(automation
        .status_snapshot_since(snapshot.generation)
        .is_none());

    let mut generation = snapshot.generation;
    let replacement = Arc::new((*configured).clone());
    for context in [
        Some(PowerPlanRuleContext {
            settings: Arc::clone(&replacement),
            source: PowerSourceProfile::PluggedIn,
        }),
        Some(PowerPlanRuleContext {
            settings: replacement,
            source: PowerSourceProfile::OnBattery,
        }),
        None,
    ] {
        update_power_plan_status(&automation.shared, status.clone(), context.as_ref(), None);
        let snapshot = automation
            .status_snapshot_since(generation)
            .expect("context-only changes advance the status generation");
        assert_eq!(snapshot.power_plan_status.as_ref(), &status);
        assert_eq!(snapshot.power_plan_rule_context, context);
        generation = snapshot.generation;
    }

    let mut settings = (*configured).clone();
    settings.general.enabled = true;
    settings.by_activity.enabled = true;
    settings.by_activity.switch_to_performance_on_resume = true;
    settings.by_activity.power_plans.performance_guid = Some("active".into());
    settings.by_activity.input_detection.mouse = false;
    settings.by_activity.input_detection.controller = false;
    settings.by_activity.idle_timeout_seconds = 30;
    let now = Instant::now();
    let mut runner = RuntimeCore::default();
    runner.input_activity.configure(
        InputHookConfig {
            keyboard: true,
            mouse: false,
        },
        now,
    );
    let source = PowerSourceProfile::PluggedIn;
    let observation = runner.by_activity_observation(&settings, source, now);
    update_power_plan_status(&automation.shared, status, None, observation);
    let aged = automation
        .status_snapshot_since(generation)
        .expect("the activity observation advances the status generation");
    assert!(Arc::ptr_eq(
        &aged.power_plan_status,
        &snapshot.power_plan_status
    ));
    let policy = &mut settings.by_activity;
    assert_eq!(
        aged.by_activity_state(source, policy, now),
        ActivityState::Active
    );
    assert_eq!(
        aged.by_activity_state(source, policy, now + Duration::from_secs(30)),
        ActivityState::Idle
    );
    assert_eq!(
        aged.by_activity_state(PowerSourceProfile::OnBattery, policy, now),
        ActivityState::Unknown
    );
    policy.idle_timeout_seconds = 60;
    assert_eq!(
        aged.by_activity_state(source, policy, now),
        ActivityState::Unknown
    );
}

#[test]
fn power_plan_action_logs_are_attributed_to_the_winning_rule_family() {
    assert_eq!(
        power_plan_action_log_feature(DecisionState::ByFocus),
        Some(ActionLogFeature::ByFocus)
    );
    assert_eq!(
        power_plan_action_log_feature(DecisionState::ByRunningApp),
        Some(ActionLogFeature::ByRunningApp)
    );
    assert_eq!(
        power_plan_action_log_feature(DecisionState::ByCpuLoad),
        Some(ActionLogFeature::ByCpuLoad)
    );
    assert_eq!(
        power_plan_action_log_feature(DecisionState::ByActivityIdle),
        Some(ActionLogFeature::ByActivity)
    );
    assert_eq!(
        power_plan_action_log_feature(DecisionState::ByTime),
        Some(ActionLogFeature::ByTime)
    );
    assert_eq!(
        power_plan_action_log_feature(DecisionState::PausedWhilePluggedIn),
        None
    );
}

#[test]
fn action_log_survives_worker_replacement_and_clear_prevents_stale_publication() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
    let record = |runner: &mut RuntimeCore| {
        runner.action_log.record(
            ActionLogFeature::ByTime,
            None,
            "",
            ActionLogResult::Applied,
            "applied",
        );
    };
    let mut first = RuntimeCore::for_worker(&automation.shared).unwrap();
    record(&mut first);
    drop(first);
    let retired = automation
        .status_snapshot_since(1)
        .expect("retiring worker publishes its final action summary");
    assert_eq!(retired.action_log_entries.len(), 1);
    assert_eq!(
        retired.action_log_summaries[&ActionLogFeature::ByTime].successful_actions,
        1
    );

    let mut second = RuntimeCore::for_worker(&automation.shared).unwrap();
    record(&mut second);
    second.publish_action_log_if_changed(&automation.shared);
    let published = automation
        .status_snapshot_since(retired.generation)
        .expect("published action summary advances status generation");
    assert_eq!(published.action_log_entries.len(), 2);
    assert_eq!(published.action_log_entries[1].sequence, 2);
    assert_eq!(
        published.action_log_summaries[&ActionLogFeature::ByTime].successful_actions,
        2
    );

    automation.clear_action_log();

    let cleared = automation
        .status_snapshot_since(published.generation)
        .expect("clearing action summaries advances status generation");
    assert!(cleared.action_log_entries.is_empty());
    assert!(cleared.action_log_summaries.is_empty());

    record(&mut second);
    second.publish_action_log_if_changed(&automation.shared);
    assert!(automation
        .status_snapshot_since(cleared.generation)
        .is_none());
    drop(second);
    let third = RuntimeCore::for_worker(&automation.shared).unwrap();
    assert!(third.action_log.entries().is_empty());
    assert!(third.action_log.summaries().is_empty());
}

#[test]
fn runtime_handle_default_settings_start_no_worker() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));

    assert!(lock_unpoisoned(&automation.thread).is_none());
}

#[test]
fn runtime_handle_rejects_commands_after_shutdown_starts() {
    let baseline = runtime_settings(Settings::default());
    let automation = RuntimeHandle::start(&baseline);
    automation.shutdown().expect("shutdown");

    let (memory_trim_result, _memory_trim_receiver) = sync_channel(1);
    let memory_trim_error = automation.request_memory_trim_now(memory_trim_result).err();
    let app_suspension_error = automation
        .request_app_suspension_path_action(r"C:\Apps\worker.exe", true)
        .err();
    let app_suspension_process_error = automation
        .request_app_suspension_process_action(Vec::new(), true)
        .err();
    let replacement = runtime_settings_with_runtime_and_persisted_revisions(
        Settings::default(),
        baseline.runtime_revision.next(),
        baseline.persisted_revision.next(),
    );
    automation.replace_settings(&replacement);
    automation.sync_worker(&replacement, true);
    let dynamic_priority_boost_error = automation
        .request_dynamic_priority_boost_action(Vec::new(), DynamicPriorityBoostState::Enabled)
        .err();
    let termination_error = automation.request_process_termination(Vec::new()).err();

    let state = lock_unpoisoned(&automation.shared.state);
    assert_eq!(state.runtime_revision, baseline.runtime_revision);
    drop(state);
    assert_eq!(
        dynamic_priority_boost_error,
        Some(RuntimeCommandError::RuntimeStopped)
    );
    assert_eq!(memory_trim_error, Some(RuntimeCommandError::RuntimeStopped));
    assert_eq!(
        app_suspension_error,
        Some(RuntimeCommandError::RuntimeStopped)
    );
    assert_eq!(
        app_suspension_process_error,
        Some(RuntimeCommandError::RuntimeStopped)
    );
    assert_eq!(termination_error, Some(RuntimeCommandError::RuntimeStopped));
    assert!(lock_unpoisoned(&automation.thread).is_none());
}

#[test]
fn manual_memory_trim_request_starts_worker_and_always_replies() {
    let mut settings = Settings::default();
    settings.general.enabled = false;
    settings.memory_trim.enabled = true;
    let automation = RuntimeHandle::start(&runtime_settings(settings));
    assert!(lock_unpoisoned(&automation.thread).is_none());

    let (result, receiver) = sync_channel(1);
    automation
        .request_memory_trim_now(result)
        .expect("request should queue");
    let status = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("worker should reply")
        .expect("runtime should process the request");

    assert!(!status.enabled);
    assert_eq!(
        status.status,
        crate::memory_trim::MemoryTrimStatus::AutomationDisabled
    );
    automation.shutdown().expect("shutdown");
}

#[test]
fn manual_memory_trim_requires_a_known_source_only_when_manual_policies_differ() {
    use crate::memory_trim::MemoryTrimStatus;

    let reply = |settings: Settings, plugged_in| {
        let configured = Arc::new(settings);
        let mut runner = RuntimeCore::default();
        runner.note_settings(&configured, AutomationWakeEvents::default(), plugged_in);
        let (result, receiver) = sync_channel(1);
        let mut observations = CycleObservations::with_adaptive_cpu_sample(Vec::new(), None, None);
        runner.run_process_control_commands(
            active_power_source_settings(&configured, plugged_in),
            false,
            VecDeque::from([ProcessControlCommand::MemoryTrim { result }]),
            &mut observations,
        );
        receiver.try_recv().unwrap().unwrap().status
    };
    let mut settings = Settings::default();
    settings.memory_trim.enabled = true;
    settings
        .battery_profile_mut()
        .memory_trim
        .process_idle_seconds += 1;
    assert_eq!(
        reply(settings.clone(), None),
        MemoryTrimStatus::ForegroundUnknown
    );

    settings
        .battery_profile_mut()
        .memory_trim
        .process_working_set_threshold_mb += 1;
    assert_eq!(
        reply(settings.clone(), None),
        MemoryTrimStatus::WaitingForPowerSource {
            manual_trim_blocked: true
        }
    );
    assert_eq!(
        reply(settings, Some(false)),
        MemoryTrimStatus::ForegroundUnknown
    );
}

#[test]
fn process_termination_request_starts_worker_and_replies() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));

    let receiver = automation
        .request_process_termination(Vec::new())
        .expect("request should queue");
    let outcome = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("worker should reply")
        .expect("runtime should process the request");

    assert_eq!(
        outcome.into_termination_result().unwrap_err(),
        "No process targets were available."
    );
    automation.shutdown().expect("shutdown");
}

#[test]
fn manual_dynamic_priority_boost_request_starts_worker_and_replies() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));

    let receiver = automation
        .request_dynamic_priority_boost_action(Vec::new(), DynamicPriorityBoostState::Enabled)
        .expect("request should queue");
    let batch = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("worker should reply")
        .expect("runtime should process the batch");

    assert!(batch.into_process_list_result().is_err());
    automation.shutdown().expect("shutdown");
}

#[test]
fn manual_process_priority_request_uses_the_shared_worker_queue_and_replies() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));

    let receiver = automation
        .request_process_priority_action(Vec::new(), ProcessPrioritySetting::Normal)
        .expect("request should queue");
    let batch = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("worker should reply")
        .expect("runtime should process the batch");

    assert!(batch.into_process_list_result().is_err());
    automation.shutdown().expect("shutdown");
}

#[test]
fn manual_efficiency_mode_request_uses_the_shared_worker_queue_and_replies() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));

    let receiver = automation
        .request_efficiency_mode_action(Vec::new(), true)
        .expect("request should queue");
    let batch = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("worker should reply")
        .expect("runtime should process the batch");

    assert!(batch.into_process_list_result().is_err());
    automation.shutdown().expect("shutdown");
}

#[test]
fn manual_thread_priority_request_uses_the_shared_worker_queue_and_replies() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));

    let receiver = automation
        .request_thread_priority_action(Vec::new(), ProcessThreadPrioritySetting::Normal)
        .expect("request should queue");
    let batch = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("worker should reply")
        .expect("runtime should process the batch");

    assert!(batch.into_process_list_result().is_err());
    automation.shutdown().expect("shutdown");
}

#[test]
fn manual_io_priority_request_uses_the_shared_worker_queue_and_replies() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));

    let receiver = automation
        .request_io_priority_action(Vec::new(), ProcessIoPriority::Normal)
        .expect("request should queue");
    let batch = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("worker should reply")
        .expect("runtime should process the batch");

    assert!(batch.into_process_list_result().is_err());
    automation.shutdown().expect("shutdown");
}

#[test]
fn manual_gpu_priority_request_uses_the_shared_worker_queue_and_replies() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));

    let receiver = automation
        .request_gpu_priority_action(Vec::new(), ProcessGpuPriority::Normal)
        .expect("request should queue");
    let batch = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("worker should reply")
        .expect("runtime should process the batch");

    assert!(batch.into_process_list_result().is_err());
    automation.shutdown().expect("shutdown");
}

#[test]
fn manual_memory_priority_request_uses_the_shared_worker_queue_and_replies() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));

    let receiver = automation
        .request_memory_priority_action(Vec::new(), ProcessMemoryPriority::Normal)
        .expect("request should queue");
    let batch = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("worker should reply")
        .expect("runtime should process the batch");

    assert!(batch.into_process_list_result().is_err());
    automation.shutdown().expect("shutdown");
}

#[test]
fn manual_app_suspension_request_uses_the_shared_worker_queue_and_replies() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));

    let receiver = automation
        .request_app_suspension_process_action(Vec::new(), true)
        .expect("request should queue");
    let batch = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("worker should reply")
        .expect("runtime should process the batch");

    assert!(batch.into_process_list_result().is_err());
    automation.shutdown().expect("shutdown");
}

#[test]
fn process_control_batch_summary_preserves_first_failure() {
    let result = ProcessControlBatchResult {
        results: vec![
            Ok(()),
            Err("first failure".to_owned()),
            Err("second failure".to_owned()),
        ],
    }
    .into_process_list_result()
    .expect_err("partial failure should be reported");

    assert_eq!(result, "2 of 3 process actions failed. first failure");
    assert!(ProcessControlBatchResult {
        results: vec![Ok(()), Ok(())],
    }
    .into_process_list_result()
    .is_ok());
    assert_eq!(
        ProcessControlBatchResult {
            results: Vec::new()
        }
        .into_process_list_result(),
        Err("No process targets were available.".to_owned())
    );
}

#[test]
fn process_control_command_queue_is_bounded_and_drained_on_shutdown() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
    let mut receivers = Vec::new();
    {
        let mut state = lock_unpoisoned(&automation.shared.state);
        for _ in 0..PROCESS_CONTROL_COMMAND_QUEUE_CAPACITY {
            let (result, receiver) = sync_channel(1);
            state
                .process_control_commands
                .push_back(ProcessControlCommand::DynamicPriorityBoost {
                    targets: Vec::new(),
                    state: DynamicPriorityBoostState::Enabled,
                    result,
                });
            receivers.push(receiver);
        }
    }

    assert_eq!(
        automation
            .request_dynamic_priority_boost_action(Vec::new(), DynamicPriorityBoostState::Enabled,)
            .err(),
        Some(RuntimeCommandError::QueueFull)
    );
    automation.shutdown().expect("shutdown");
    for receiver in receivers {
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(1)),
            Ok(Err(RuntimeCommandError::RuntimeStopped))
        ));
    }
}

#[test]
fn process_control_queue_preserves_cross_property_fifo_order() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
    let (priority_result, _priority_receiver) = sync_channel(1);
    let (efficiency_result, _efficiency_receiver) = sync_channel(1);
    let (boost_result, _boost_receiver) = sync_channel(1);
    let (thread_result, _thread_receiver) = sync_channel(1);
    let (io_result, _io_receiver) = sync_channel(1);
    let (gpu_result, _gpu_receiver) = sync_channel(1);
    let (memory_result, _memory_receiver) = sync_channel(1);
    let (app_suspension_result, _app_suspension_receiver) = sync_channel(1);
    let (app_suspension_path_result, _app_suspension_path_receiver) = sync_channel(1);
    let (memory_trim_result, _memory_trim_receiver) = sync_channel(1);
    let (termination_result, _termination_receiver) = sync_channel(1);
    {
        let mut state = lock_unpoisoned(&automation.shared.state);
        state
            .process_control_commands
            .push_back(ProcessControlCommand::ProcessPriority {
                targets: Vec::new(),
                priority: ProcessPrioritySetting::Normal,
                result: priority_result,
            });
        state
            .process_control_commands
            .push_back(ProcessControlCommand::EfficiencyMode {
                targets: Vec::new(),
                enabled: true,
                result: efficiency_result,
            });
        state
            .process_control_commands
            .push_back(ProcessControlCommand::DynamicPriorityBoost {
                targets: Vec::new(),
                state: DynamicPriorityBoostState::Enabled,
                result: boost_result,
            });
        state
            .process_control_commands
            .push_back(ProcessControlCommand::ThreadPriority {
                targets: Vec::new(),
                priority: ProcessThreadPrioritySetting::Normal,
                result: thread_result,
            });
        state
            .process_control_commands
            .push_back(ProcessControlCommand::IoPriority {
                targets: Vec::new(),
                priority: ProcessIoPriority::Normal,
                result: io_result,
            });
        state
            .process_control_commands
            .push_back(ProcessControlCommand::GpuPriority {
                targets: Vec::new(),
                priority: ProcessGpuPriority::Normal,
                result: gpu_result,
            });
        state
            .process_control_commands
            .push_back(ProcessControlCommand::MemoryPriority {
                targets: Vec::new(),
                priority: ProcessMemoryPriority::Normal,
                result: memory_result,
            });
        state
            .process_control_commands
            .push_back(ProcessControlCommand::AppSuspension {
                targets: Vec::new(),
                suspend: true,
                result: app_suspension_result,
            });
        state
            .process_control_commands
            .push_back(ProcessControlCommand::AppSuspensionPathAction {
                executable_path: r"C:\Apps\app.exe".to_owned(),
                freeze: true,
                result: app_suspension_path_result,
            });
        state
            .process_control_commands
            .push_back(ProcessControlCommand::MemoryTrim {
                result: memory_trim_result,
            });
        state
            .process_control_commands
            .push_back(ProcessControlCommand::StopProcesses {
                targets: Vec::new(),
                result: termination_result,
            });
    }

    let snapshot = automation_snapshot(&automation.shared).expect("runtime should be active");
    let commands = snapshot
        .process_control_commands
        .into_iter()
        .collect::<Vec<_>>();
    assert!(matches!(
        commands.first(),
        Some(ProcessControlCommand::ProcessPriority { .. })
    ));
    assert!(matches!(
        commands.get(1),
        Some(ProcessControlCommand::EfficiencyMode { .. })
    ));
    assert!(matches!(
        commands.get(2),
        Some(ProcessControlCommand::DynamicPriorityBoost { .. })
    ));
    assert!(matches!(
        commands.get(3),
        Some(ProcessControlCommand::ThreadPriority { .. })
    ));
    assert!(matches!(
        commands.get(4),
        Some(ProcessControlCommand::IoPriority { .. })
    ));
    assert!(matches!(
        commands.get(5),
        Some(ProcessControlCommand::GpuPriority { .. })
    ));
    assert!(matches!(
        commands.get(6),
        Some(ProcessControlCommand::MemoryPriority { .. })
    ));
    assert!(matches!(
        commands.get(7),
        Some(ProcessControlCommand::AppSuspension { .. })
    ));
    assert!(matches!(
        commands.get(8),
        Some(ProcessControlCommand::AppSuspensionPathAction { .. })
    ));
    assert!(matches!(
        commands.get(9),
        Some(ProcessControlCommand::MemoryTrim { .. })
    ));
    assert!(matches!(
        commands.get(10),
        Some(ProcessControlCommand::StopProcesses { .. })
    ));
    automation.shutdown().expect("shutdown");
}

#[test]
fn managed_process_state_keeps_an_otherwise_idle_worker_alive() {
    assert!(!automation_worker_can_exit(None, false, true));
    assert!(automation_worker_can_exit(None, false, false));
    assert!(!automation_worker_can_exit(
        Some(Duration::from_secs(1)),
        false,
        false,
    ));
    assert!(!automation_worker_can_exit(None, true, false));
}

#[test]
fn cpu_allocation_reconciliation_retry_backoff_is_bounded() {
    let mut interval = CPU_ALLOCATION_RECONCILIATION_RETRY_INITIAL;
    for expected in [2, 4, 8, 16, 32, 60, 60] {
        interval = next_cpu_allocation_reconciliation_retry_interval(interval);
        assert_eq!(interval, Duration::from_secs(expected));
    }
}

#[test]
fn worker_exit_commit_yields_to_a_concurrent_request_generation() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
    let original_generation = {
        let mut state = lock_unpoisoned(&automation.shared.state);
        state.worker_accepting_work = true;
        let generation = state.change_generation;
        state.change_generation = state.change_generation.wrapping_add(1);
        generation
    };

    assert!(!commit_worker_exit(&automation.shared, original_generation));
    assert!(lock_unpoisoned(&automation.shared.state).worker_accepting_work);
    let current_generation = lock_unpoisoned(&automation.shared.state).change_generation;
    assert!(commit_worker_exit(&automation.shared, current_generation));
    assert!(!lock_unpoisoned(&automation.shared.state).worker_accepting_work);
}

#[test]
fn unexpected_worker_exit_rejects_queued_commands() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
    let (result, receiver) = sync_channel(1);
    {
        let mut state = lock_unpoisoned(&automation.shared.state);
        state.worker_accepting_work = true;
        state
            .process_control_commands
            .push_back(ProcessControlCommand::DynamicPriorityBoost {
                targets: Vec::new(),
                state: DynamicPriorityBoostState::Enabled,
                result,
            });
    }

    drop(AutomationWorkerExitGuard {
        shared: &automation.shared,
    });

    assert!(!lock_unpoisoned(&automation.shared.state).worker_accepting_work);
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Err(RuntimeCommandError::WorkerExited))
    ));
}

#[test]
fn committed_worker_exit_leaves_newer_commands_for_the_replacement() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
    let (result, receiver) = sync_channel(1);
    {
        let mut state = lock_unpoisoned(&automation.shared.state);
        state.worker_accepting_work = false;
        state
            .process_control_commands
            .push_back(ProcessControlCommand::ThreadPriority {
                targets: Vec::new(),
                priority: ProcessThreadPrioritySetting::Normal,
                result,
            });
    }

    drop(AutomationWorkerExitGuard {
        shared: &automation.shared,
    });

    assert_eq!(
        lock_unpoisoned(&automation.shared.state)
            .process_control_commands
            .len(),
        1
    );
    assert!(matches!(
        receiver.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn worker_exit_clears_the_last_instance_balancing_rule_status() {
    let shared = shared_automation_state(Settings::default());
    let rule = crate::config::InstanceBalancingRule {
        enabled: true,
        executable_path: r"C:\Apps\example.exe".into(),
    };
    {
        let mut state = lock_unpoisoned(&shared.state);
        let status = &mut Arc::make_mut(&mut state.status.feature_status).instance_balancing;
        status.enabled = true;
        status.record_rule(
            Arc::new(rule.clone()),
            ActionLogResult::Applied,
            InstanceBalancingDetail::Assigned(2),
        );
    }
    drop(AutomationWorkerExitGuard { shared: &shared });
    let state = lock_unpoisoned(&shared.state);
    assert_eq!(
        state
            .status
            .feature_status
            .instance_balancing
            .rule_result(&rule),
        None
    );
    assert_eq!(shared.status_generation.load(Ordering::Acquire), 1);
}

#[test]
fn runtime_handle_shutdown_surfaces_thread_panic() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
    {
        let mut thread = lock_unpoisoned(&automation.thread);
        *thread = Some(std::thread::spawn(|| -> Result<(), String> {
            panic!("simulated worker panic");
        }));
    }

    let error = automation
        .shutdown()
        .expect_err("worker panic should surface on shutdown");
    assert!(error.contains("panicked during shutdown"));
}

#[test]
fn process_appearance_detector_does_not_report_only_exits() {
    let mut known = vec![1, 2, 3];

    assert!(!process_ids_have_new_entries(&mut known, vec![2, 0, 2, 1]));
    assert_eq!(known, [1, 2]);
}

#[test]
fn process_appearance_scan_sleeps_when_process_features_are_off() {
    let settings = Settings::default();

    assert!(!process_appearance_scan_required(&settings));
}

#[test]
fn foreground_lookup_runs_only_for_configured_by_focus() {
    let mut settings = Settings::default();

    assert!(!foreground_lookup_required(&settings));

    settings.by_focus.enabled = true;
    assert!(!foreground_lookup_required(&settings));

    settings.by_focus.rules.push(ByFocusRule {
        enabled: true,
        name: "editor.exe".to_owned(),
        selector: ExecutableSelector::ExactPath {
            executable_path: String::new(),
        },
        power_plan_guid: None,
    });
    assert!(!foreground_lookup_required(&settings));

    settings.by_focus.rules[0].power_plan_guid = Some("active-guid".to_owned());
    assert!(!foreground_lookup_required(&settings));

    settings.by_focus.rules[0].selector = ExecutableSelector::ExactPath {
        executable_path: "editor.exe".to_owned(),
    };
    assert!(!foreground_lookup_required(&settings));

    settings.by_focus.rules[0].selector = ExecutableSelector::ExactPath {
        executable_path: r"C:\Apps\editor.exe".to_owned(),
    };
    assert!(foreground_lookup_required(&settings));

    settings.by_focus.rules[0].selector = ExecutableSelector::ExactName {
        executable_name: "editor.exe".to_owned(),
    };
    assert!(foreground_lookup_required(&settings));

    settings.by_focus.rules[0].enabled = false;
    assert!(!foreground_lookup_required(&settings));
}

#[test]
fn enabled_empty_rule_features_do_not_poll() {
    let mut settings = Settings::default();
    settings.app_suspension.enabled = true;
    settings.cpu_sets_soft.enabled = true;
    settings.cpu_limiter.enabled = true;
    settings.by_running_app.enabled = true;
    settings.timer_resolution.enabled = true;
    settings.by_focus.enabled = true;

    assert!(!automation_worker_required(&settings));

    settings
        .app_suspension
        .suspendable_apps
        .push(app_suspension_rule(" "));
    settings.cpu_sets_soft.rules.push(CpuAllocationRule {
        enabled: true,
        executable_path: " ".to_owned(),
        focus_selection: crate::cpu_topology::test_selection(1),
        visible_window_selection: crate::cpu_topology::test_selection(1),
        background_selection: crate::cpu_topology::test_selection(1),
    });
    settings.cpu_limiter.rules.push(CpuLimiterRule {
        enabled: true,
        executable_path: " ".to_owned(),
        focus_mode: ProcessRuleMode::Disabled,
        visible_window_mode: ProcessRuleMode::Disabled,
        background_mode: ProcessRuleMode::Enabled,
        focus_allowed_cpu_time_percent: 50,
        visible_window_allowed_cpu_time_percent: 50,
        background_allowed_cpu_time_percent: 50,
    });
    settings.by_running_app.rules.push(ByRunningAppRule {
        enabled: true,
        name: "Empty".to_owned(),
        selector: ExecutableSelector::ExactPath {
            executable_path: " ".to_owned(),
        },
        power_plan_guid: Some("active-guid".to_owned()),
    });
    settings.timer_resolution.rules.push(TimerResolutionRule {
        enabled: true,
        executable_path: " ".to_owned(),
        desired_100ns: 5_000,
    });
    settings.by_focus.rules.push(ByFocusRule {
        enabled: true,
        name: "Empty".to_owned(),
        selector: ExecutableSelector::ExactPath {
            executable_path: " ".to_owned(),
        },
        power_plan_guid: Some("active-guid".to_owned()),
    });

    assert!(!app_suspension_required(&settings));
    assert!(!cpu_sets_soft_required(&settings));
    assert!(!cpu_limiter_required(&settings));
    assert!(!by_running_app_required(&settings));
    assert!(!timer_resolution_required(&settings));
    assert!(!foreground_lookup_required(&settings));
    assert!(!process_appearance_scan_required(&settings));
    assert!(!event_driven_process_work_required(&settings));
    assert!(!automation_worker_required(&settings));
}

#[test]
fn enabled_nonempty_rule_features_require_runtime_work() {
    let mut settings = Settings::default();
    settings.app_suspension.enabled = true;
    settings
        .app_suspension
        .suspendable_apps
        .push(app_suspension_rule(r"C:\Apps\chat.exe"));
    settings.cpu_sets_soft.enabled = true;
    settings.cpu_sets_soft.rules.push(CpuAllocationRule {
        enabled: true,
        executable_path: r"C:\Apps\chat.exe".to_owned(),
        focus_selection: crate::cpu_topology::test_selection(1),
        visible_window_selection: crate::cpu_topology::test_selection(1),
        background_selection: crate::cpu_topology::test_selection(1),
    });
    settings.cpu_limiter.enabled = true;
    settings.cpu_limiter.rules.push(CpuLimiterRule {
        enabled: true,
        executable_path: r"C:\Apps\chat.exe".to_owned(),
        focus_mode: ProcessRuleMode::Disabled,
        visible_window_mode: ProcessRuleMode::Disabled,
        background_mode: ProcessRuleMode::Enabled,
        focus_allowed_cpu_time_percent: 50,
        visible_window_allowed_cpu_time_percent: 50,
        background_allowed_cpu_time_percent: 50,
    });
    settings.by_running_app.enabled = true;
    settings.by_running_app.rules.push(ByRunningAppRule {
        enabled: true,
        name: "Chat".to_owned(),
        selector: ExecutableSelector::ExactPath {
            executable_path: r"C:\Apps\chat.exe".to_owned(),
        },
        power_plan_guid: Some("active-guid".to_owned()),
    });
    settings.timer_resolution.enabled = true;
    settings.timer_resolution.rules.push(TimerResolutionRule {
        enabled: true,
        executable_path: r"C:\Apps\chat.exe".to_owned(),
        desired_100ns: 5_000,
    });
    settings.by_focus.enabled = true;
    settings.by_focus.rules.push(ByFocusRule {
        enabled: true,
        name: "Chat".to_owned(),
        selector: ExecutableSelector::ExactPath {
            executable_path: r"C:\Apps\chat.exe".to_owned(),
        },
        power_plan_guid: Some("active-guid".to_owned()),
    });

    assert!(app_suspension_required(&settings));
    assert!(cpu_sets_soft_required(&settings));
    assert!(cpu_limiter_required(&settings));
    assert!(by_running_app_required(&settings));
    assert!(timer_resolution_required(&settings));
    assert!(foreground_lookup_required(&settings));
    assert!(process_appearance_scan_required(&settings));
    assert!(event_driven_process_work_required(&settings));
    assert!(automation_worker_required(&settings));

    settings.by_running_app.rules[0].selector = ExecutableSelector::ExactPath {
        executable_path: "chat.exe".to_owned(),
    };
    assert!(!by_running_app_required(&settings));

    settings.by_running_app.rules[0].selector = ExecutableSelector::ExactName {
        executable_name: "chat.exe".to_owned(),
    };
    assert!(by_running_app_required(&settings));
}

#[test]
fn all_unlimited_cpu_limiter_rules_do_not_require_runtime_work() {
    let mut settings = Settings::default();
    settings.cpu_limiter.enabled = true;
    settings.cpu_limiter.focus_allowed_cpu_time_percent = 100;
    settings.cpu_limiter.visible_window_allowed_cpu_time_percent = 100;
    settings.cpu_limiter.background_allowed_cpu_time_percent = 100;
    settings.cpu_limiter.rules.push(CpuLimiterRule {
        enabled: true,
        executable_path: r"C:\Apps\chat.exe".to_owned(),
        focus_mode: ProcessRuleMode::Default,
        visible_window_mode: ProcessRuleMode::Default,
        background_mode: ProcessRuleMode::Default,
        focus_allowed_cpu_time_percent: 50,
        visible_window_allowed_cpu_time_percent: 50,
        background_allowed_cpu_time_percent: 50,
    });

    assert!(!cpu_limiter_required(&settings));
    assert!(!process_appearance_scan_required(&settings));

    settings.cpu_limiter.rules[0].background_mode = ProcessRuleMode::Enabled;
    settings.cpu_limiter.rules[0].background_allowed_cpu_time_percent = 5;

    assert!(cpu_limiter_required(&settings));
    assert!(process_appearance_scan_required(&settings));
}

#[test]
fn cpu_sets_soft_owns_duplicate_cpu_allocation_rules() {
    let mut settings = Settings::default();
    let rule = CpuAllocationRule {
        enabled: true,
        executable_path: r"C:\Apps\chat.exe".to_owned(),
        focus_selection: crate::cpu_topology::test_selection(1),
        visible_window_selection: crate::cpu_topology::test_selection(1),
        background_selection: crate::cpu_topology::test_selection(1),
    };
    settings.cpu_sets_soft.rules.push(rule.clone());
    settings.processor_affinity_hard.rules.push(rule);

    assert!(processor_affinity_hard_settings(&settings).rules.is_empty());
    settings.processor_affinity_hard.enabled = true;
    assert!(!processor_affinity_hard_required(&settings));
}

#[test]
fn explicit_cpu_allocation_source_wait_accounts_for_configured_soft_precedence() {
    let mut settings = Settings::default();
    settings.general.enabled = true;
    let rule = CpuAllocationRule {
        enabled: true,
        executable_path: r"C:\Apps\chat.exe".into(),
        focus_selection: crate::cpu_topology::test_selection(1),
        visible_window_selection: crate::cpu_topology::test_selection(1),
        background_selection: crate::cpu_topology::test_selection(1),
    };
    let mut disabled_soft = rule.clone();
    disabled_soft.enabled = false;
    settings
        .battery_profile_mut()
        .cpu_sets_soft
        .rules
        .push(disabled_soft.clone());
    assert!(cpu_allocation_source_difference(&settings).is_none());
    settings.processor_affinity_hard.enabled = true;
    settings.processor_affinity_hard.rules.push(rule);
    settings.battery_profile_mut().processor_affinity_hard =
        settings.processor_affinity_hard.clone();
    let waiting = cpu_allocation_source_difference(&settings).unwrap();
    assert!(!waiting.soft && waiting.hard);
    assert!(processor_affinity_hard_required(&settings));
    assert!(!processor_affinity_hard_required(
        settings.battery_profile()
    ));
    assert!(!process_appearance_scan_required_for_runtime(
        &settings, false, false, false, false, false
    ));
    settings.cpu_sets_soft.rules.push(disabled_soft);
    assert!(cpu_allocation_source_difference(&settings).is_none());
}

#[test]
fn explicit_cpu_allocation_paths_exclude_only_active_usable_rules() {
    let mut settings = Settings::default();
    settings.cpu_sets_soft.enabled = true;
    settings.cpu_sets_soft.rules.push(CpuAllocationRule {
        enabled: true,
        executable_path: r"C:\Apps\soft.exe".to_owned(),
        focus_selection: crate::cpu_topology::test_selection(1),
        visible_window_selection: crate::cpu_topology::test_selection(1),
        background_selection: crate::cpu_topology::test_selection(1),
    });
    settings.processor_affinity_hard.enabled = true;
    settings
        .processor_affinity_hard
        .rules
        .push(CpuAllocationRule {
            enabled: false,
            executable_path: r"C:\Apps\disabled.exe".to_owned(),
            focus_selection: crate::cpu_topology::test_selection(1),
            visible_window_selection: crate::cpu_topology::test_selection(1),
            background_selection: crate::cpu_topology::test_selection(1),
        });
    settings
        .processor_affinity_hard
        .rules
        .push(CpuAllocationRule {
            enabled: true,
            executable_path: r"C:\Apps\empty.exe".to_owned(),
            focus_selection: crate::cpu_topology::test_selection(0),
            visible_window_selection: crate::cpu_topology::test_selection(0),
            background_selection: crate::cpu_topology::test_selection(0),
        });

    let paths = explicit_cpu_allocation_paths(&settings, None);

    assert_eq!(paths.len(), 1);
    assert!(paths.contains(&crate::foreground::process_failure_key(r"C:\Apps\soft.exe")));
}

#[test]
fn priority_source_schedule_preserves_verified_and_common_policy_cadence() {
    let mut settings = Settings::default();
    settings.general.enabled = true;
    settings.battery_profile_mut().process_priority.enabled = true;
    let schedule = |settings: &Settings, source, watcher, changed| {
        priority_source_schedule(
            settings,
            active_power_source_settings(settings, source),
            source,
            watcher,
            changed,
            |profile| (&profile.process_priority, profile.process_priority.enabled),
        )
    };
    assert!(!schedule(&settings, Some(true), true, false).source_recheck);
    assert!(schedule(&settings, Some(true), false, false).source_recheck);
    assert!(schedule(&settings, Some(false), false, false).refresh_required);
    assert!(!schedule(&settings, Some(false), false, false).source_recheck);

    let entering = schedule(&settings, None, true, true);
    assert!(entering.refresh_required && entering.waiting_for_source && entering.source_recheck);
    assert!(!entering.active);
    let stable = schedule(&settings, None, false, false);
    assert!(!stable.refresh_required && stable.waiting_for_source && stable.source_recheck);

    settings.process_priority.enabled = true;
    settings
        .battery_profile_mut()
        .process_priority
        .background_priority = ProcessPrioritySetting::Idle;
    let differing_enabled = schedule(&settings, None, true, true);
    assert!(process_appearance_scan_required(&settings));
    assert!(!process_appearance_scan_required_for_runtime(
        &settings,
        false,
        false,
        false,
        differing_enabled.active,
        false
    ));

    settings.process_priority = settings.battery_profile().process_priority.clone();
    let common = schedule(&settings, None, false, false);
    assert!(common.refresh_required && !common.waiting_for_source && !common.source_recheck);
    assert!(process_appearance_scan_required_for_runtime(
        &settings,
        false,
        false,
        false,
        common.active,
        false
    ));
}

#[test]
fn efficiency_mode_source_difference_needs_an_enabled_policy() {
    let mut settings = Settings::default();
    settings.general.theme_mode = AppThemeMode::Dark;
    settings.general.accent.source = AccentColorSource::Custom;
    assert!(!efficiency_mode_policy_varies_by_source(&settings));
    let efficiency = &mut settings.battery_profile_mut().efficiency_mode;
    efficiency.enabled = true;
    efficiency.background_efficiency_mode = false;
    assert!(!efficiency_mode_policy_varies_by_source(&settings));

    settings
        .battery_profile_mut()
        .efficiency_mode
        .background_efficiency_mode = true;
    assert!(efficiency_mode_policy_varies_by_source(&settings));
    assert!(automation_worker_required(&settings));
    assert_eq!(
        windows_event_watcher_mode(&settings),
        Some(WindowsEventWatcherMode::Full)
    );
    assert!(windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::PowerChanged
    ));

    settings.efficiency_mode.enabled = true;
    assert!(!efficiency_mode_policy_varies_by_source(&settings));
    settings
        .battery_profile_mut()
        .efficiency_mode
        .visible_window_detection_enabled = true;
    assert!(efficiency_mode_policy_varies_by_source(&settings));
}

#[test]
fn timer_resolution_requests_power_events_only_for_different_profile_enablement() {
    let mut settings = Settings::default();
    settings.general.theme_mode = AppThemeMode::Dark;
    settings.general.accent.source = AccentColorSource::Custom;
    settings.timer_resolution.enabled = true;
    settings.timer_resolution.rules.push(TimerResolutionRule {
        enabled: true,
        executable_path: r"C:\Apps\editor.exe".to_owned(),
        desired_100ns: 10_000,
    });
    assert_eq!(windows_event_watcher_mode(&settings), None);
    assert!(!timer_resolution_policy_varies_by_source(&settings));

    let battery = &mut settings.battery_profile_mut().timer_resolution;
    battery.rules[0].executable_path.insert(0, ' ');
    battery.rules.push(TimerResolutionRule {
        enabled: false,
        executable_path: r"C:\Apps\unused.exe".to_owned(),
        desired_100ns: 100_000,
    });
    assert!(!timer_resolution_policy_varies_by_source(&settings));

    settings.battery_profile_mut().timer_resolution.rules[0].desired_100ns = 100_000;
    assert!(timer_resolution_policy_varies_by_source(&settings));
    assert_eq!(windows_event_watcher_mode(&settings), None);

    let rule = &mut settings.battery_profile_mut().timer_resolution.rules[0];
    rule.desired_100ns = 10_000;
    rule.executable_path.push('\u{3000}');
    assert!(timer_resolution_policy_varies_by_source(&settings));
    assert_eq!(windows_event_watcher_mode(&settings), None);

    settings.battery_profile_mut().timer_resolution.enabled = false;
    assert!(timer_resolution_policy_varies_by_source(&settings));
    assert_eq!(
        windows_event_watcher_mode(&settings),
        Some(WindowsEventWatcherMode::Full)
    );
    assert!(windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::PowerChanged
    ));

    settings.timer_resolution.rules[0].enabled = false;
    assert_eq!(windows_event_watcher_mode(&settings), None);
    settings.battery_profile_mut().timer_resolution.enabled = true;
    assert_eq!(
        windows_event_watcher_mode(&settings),
        Some(WindowsEventWatcherMode::Full)
    );
}

#[test]
fn automation_worker_runs_for_adaptive_power_plan_alone() {
    let mut settings = Settings::default();
    settings.general.theme_mode = AppThemeMode::Dark;
    settings.general.accent.source = AccentColorSource::Custom;
    settings.adaptive_engine.adaptive_processor_power_enabled = true;

    assert!(automation_worker_required(&settings));
    assert!(!adaptive_engine_process_required(&settings));
    assert!(!process_appearance_scan_required(&settings));
    assert!(!bottleneck_classifier_required(&settings));
    assert_eq!(
        windows_event_watcher_mode(&settings),
        Some(WindowsEventWatcherMode::SuspendResumeOnly)
    );
    assert!(windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::SuspendResume
    ));

    settings
        .battery_profile_mut()
        .adaptive_engine
        .adaptive_processor_power_enabled = false;
    assert_eq!(
        windows_event_watcher_mode(&settings),
        Some(WindowsEventWatcherMode::Full)
    );
    assert!(windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::PowerChanged
    ));
    assert!(adaptive_power_plan_required(active_power_source_settings(
        &settings,
        Some(true)
    )));
    assert!(!adaptive_power_plan_required(active_power_source_settings(
        &settings,
        Some(false)
    )));

    settings.adaptive_engine.adaptive_processor_power_enabled = false;
    settings
        .battery_profile_mut()
        .adaptive_engine
        .adaptive_processor_power_enabled = true;
    assert_eq!(
        windows_event_watcher_mode(&settings),
        Some(WindowsEventWatcherMode::Full)
    );
    assert!(automation_worker_required(&settings));
    assert!(!adaptive_power_plan_required(active_power_source_settings(
        &settings,
        Some(true)
    )));
    assert!(adaptive_power_plan_required(active_power_source_settings(
        &settings,
        Some(false)
    )));
}

#[test]
fn adaptive_processor_power_waits_only_for_source_dependent_policy() {
    let mut settings = Settings::default();
    settings.general.enabled = true;
    settings.adaptive_engine.adaptive_processor_power_enabled = true;
    settings
        .adaptive_engine
        .base_processor_policy
        .core_parking_min = 100;
    assert!(!adaptive_power_plan_waits_for_source(&settings, None));

    let battery = &mut settings.battery_profile_mut().adaptive_engine;
    battery.enabled = true;
    battery.base_processor_policy.core_parking_min = 101;
    assert!(!adaptive_power_plan_waits_for_source(&settings, None));

    settings
        .battery_profile_mut()
        .adaptive_engine
        .base_processor_policy
        .performance_max = 60;
    assert!(adaptive_power_plan_waits_for_source(&settings, None));
    assert!(!adaptive_power_plan_waits_for_source(&settings, Some(true)));
    assert!(!adaptive_power_plan_waits_for_source(
        &settings,
        Some(false)
    ));

    settings.adaptive_engine.adaptive_processor_power_enabled = false;
    assert!(adaptive_power_plan_waits_for_source(&settings, None));
    settings.general.enabled = false;
    assert!(!adaptive_power_plan_waits_for_source(&settings, None));
    settings.general.enabled = true;
    settings
        .battery_profile_mut()
        .adaptive_engine
        .adaptive_processor_power_enabled = false;
    assert!(!adaptive_power_plan_waits_for_source(&settings, None));
}

#[test]
fn automation_worker_runs_for_bottleneck_classifier_alone() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = false;
    settings.by_focus.enabled = false;
    settings.adaptive_engine.enabled = true;
    settings.adaptive_engine.adaptive_processor_power_enabled = false;

    assert!(bottleneck_classifier_required(&settings));
    assert!(automation_worker_required(&settings));
}

#[test]
fn adaptive_engine_uses_low_power_refresh_cadence() {
    assert_eq!(
        automation_refresh_interval(false, true, Duration::from_secs(1)),
        ADAPTIVE_ENGINE_AUTOMATION_REFRESH_INTERVAL
    );
    assert_eq!(
        automation_refresh_interval(false, true, PROCESS_APPEARANCE_SCAN_INTERVAL),
        ADAPTIVE_ENGINE_AUTOMATION_REFRESH_INTERVAL
    );
    assert_eq!(
        automation_refresh_interval(false, true, APP_SUSPENSION_FOREGROUND_RELEASE_INTERVAL),
        ADAPTIVE_ENGINE_AUTOMATION_REFRESH_INTERVAL
    );
    assert_eq!(
        automation_refresh_interval(true, false, Duration::from_secs(1)),
        HIDDEN_AUTOMATION_REFRESH_INTERVAL
    );
}

#[test]
fn active_cpu_limiter_keeps_process_discovery_at_one_second() {
    let mut settings = Settings::default();
    settings.cpu_limiter.enabled = true;
    settings.cpu_limiter.rules.push(CpuLimiterRule {
        enabled: true,
        executable_path: r"C:\Apps\chat.exe".to_owned(),
        focus_mode: ProcessRuleMode::Disabled,
        visible_window_mode: ProcessRuleMode::Disabled,
        background_mode: ProcessRuleMode::Enabled,
        focus_allowed_cpu_time_percent: 50,
        visible_window_allowed_cpu_time_percent: 50,
        background_allowed_cpu_time_percent: 50,
    });

    for (hidden_to_tray, adaptive_engine_enabled) in [(false, false), (true, false), (false, true)]
    {
        assert_eq!(
            process_appearance_refresh_interval(
                hidden_to_tray,
                adaptive_engine_enabled,
                cpu_limiter_required(&settings),
            ),
            PROCESS_APPEARANCE_SCAN_INTERVAL
        );
    }

    assert!(process_appearance_scan_required(&settings));
    assert!(!process_appearance_scan_required_for_runtime(
        &settings, false, false, false, false, false
    ));
    assert_eq!(
        process_appearance_refresh_interval(false, true, false),
        ADAPTIVE_ENGINE_AUTOMATION_REFRESH_INTERVAL
    );

    settings.adaptive_engine.enabled = true;
    settings.general.theme_mode = AppThemeMode::Dark;
    settings.general.accent.source = AccentColorSource::Custom;
    let battery = settings.battery_profile_mut();
    battery.cpu_limiter.background_allowed_cpu_time_percent = 50;
    battery.cpu_limiter.rules[0].background_mode = ProcessRuleMode::Default;
    assert!(!cpu_limiter_policy_varies_by_source(&settings));

    settings.cpu_limiter.enabled = false;
    assert!(cpu_limiter_policy_varies_by_source(&settings));
    assert_eq!(
        windows_event_watcher_mode(&settings),
        Some(WindowsEventWatcherMode::Full)
    );
    assert!(windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::PowerChanged
    ));
    settings.general.enabled = false;
    assert!(!cpu_limiter_policy_varies_by_source(&settings));
}

#[test]
fn status_snapshot_since_skips_unchanged_status() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
    let snapshot = automation
        .status_snapshot_since(0)
        .expect("initial status snapshot should be visible");

    assert!(automation
        .status_snapshot_since(snapshot.generation)
        .is_none());
}

#[test]
fn runtime_settings_snapshot_revision_noop_update_does_not_bump_worker_settings() {
    let settings = Settings::default();
    let base = runtime_settings(settings.clone());
    let mut settings_v2 = settings.clone();
    settings_v2.general.check_interval_ms = 4_200;
    let revisioned = runtime_settings_with_runtime_and_persisted_revisions(
        settings_v2,
        base.runtime_revision.next(),
        base.persisted_revision,
    );

    let automation = RuntimeHandle::start(&base);
    let generation = {
        let state = lock_unpoisoned(&automation.shared.state);
        state.change_generation
    };
    automation.replace_settings(&base);

    let same_generation = {
        let state = lock_unpoisoned(&automation.shared.state);
        state.change_generation
    };
    assert_eq!(same_generation, generation);

    automation.replace_settings(&revisioned);
    let changed = {
        let state = lock_unpoisoned(&automation.shared.state);
        state.change_generation != generation
            && state.runtime_revision == revisioned.runtime_revision
            && state.settings.as_ref() == revisioned.value.as_ref()
    };
    assert!(changed);
}

#[test]
fn sleep_prevention_discards_the_whole_burst_when_intermediate_saves_overflow() {
    let settings = Arc::new(Settings::default());
    let mut pending = PendingSleepPreventionSettings::default();
    for _ in 0..4 {
        pending.record(&settings);
    }
    assert_eq!(pending.intermediate.len(), 4);

    pending.record(&settings);
    pending.record(&settings);
    assert!(std::mem::take(&mut pending).intermediate.is_empty());

    pending.record(&settings);
    assert_eq!(pending.intermediate.len(), 1);
    assert!(Arc::ptr_eq(&pending.intermediate[0], &settings));
}

#[test]
fn pending_auto_exclusions_are_taken_only_after_generation_change() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
    let wakes = Arc::new(AtomicU64::new(0));
    let wake_count = wakes.clone();
    automation.set_auto_exclusion_wake(Arc::new(move || {
        wake_count.fetch_add(1, Ordering::Relaxed);
    }));
    let mut generation = 0;

    assert!(automation
        .take_auto_exclusion_patch_since(&mut generation)
        .is_none());

    update_cpu_sets_soft_status(
        &automation.shared,
        CpuAllocationSnapshot {
            auto_excluded_processes: vec![r"D:\Games\Game.exe".to_owned()],
            ..CpuAllocationSnapshot::default()
        },
    );

    assert_eq!(wakes.load(Ordering::Relaxed), 1);
    let pending = automation
        .take_auto_exclusion_patch_since(&mut generation)
        .expect("new pending affinity exclusions should be visible");
    assert_eq!(pending.base_revision, SettingsRevision::initial());
    assert_eq!(pending.cpu_sets_soft, vec![r"D:\Games\Game.exe"]);
    assert!(automation
        .take_auto_exclusion_patch_since(&mut generation)
        .is_none());

    automation.requeue_auto_exclusion_patch(pending);
    assert!(automation
        .take_auto_exclusion_patch_since(&mut generation)
        .is_none());
    assert_eq!(wakes.load(Ordering::Relaxed), 1);
    lock_unpoisoned(&automation.shared.state).pending_auto_exclusions_retry_at =
        Some(Instant::now());
    assert!(automation
        .take_auto_exclusion_patch_since(&mut generation)
        .is_some());
}

#[test]
fn pending_auto_exclusion_patch_keeps_base_revision_for_coalesced_paths() {
    let mut baseline_settings = Settings::default();
    baseline_settings.app_suspension.enabled = true;
    baseline_settings
        .app_suspension
        .suspendable_apps
        .push(app_suspension_rule(r"C:\Apps\watch.exe"));
    let base = runtime_settings_with_runtime_and_persisted_revisions(
        baseline_settings.clone(),
        SettingsRevision::initial(),
        SettingsRevision::initial(),
    );
    let mut stale = baseline_settings.clone();
    stale.app_suspension.suspendable_apps[0].enabled = false;
    let revisioned = runtime_settings_with_runtime_and_persisted_revisions(
        stale,
        base.runtime_revision.next(),
        base.persisted_revision.next(),
    );

    let automation = RuntimeHandle::start(&base);
    automation.replace_settings(&revisioned);

    update_process_priority_status(
        &automation.shared,
        ProcessPrioritySnapshot {
            auto_excluded_processes: vec![r"C:\Apps\first.exe".to_owned()],
            ..ProcessPrioritySnapshot::default()
        },
    );

    update_app_suspension_status(
        &automation.shared,
        AppSuspensionSnapshot {
            auto_excluded_processes: vec![r"C:\Apps\second.exe".to_owned()],
            ..AppSuspensionSnapshot::default()
        },
    );

    let mut patch_generation = 0;
    let patch = automation
        .take_auto_exclusion_patch_since(&mut patch_generation)
        .expect("pending exclusions should combine paths");
    assert_eq!(patch.base_revision, base.persisted_revision.next());
    assert_eq!(patch.app_suspension, vec![r"C:\Apps\second.exe"]);
    assert_eq!(patch.process_priority, vec![r"C:\Apps\first.exe"]);
}

#[test]
fn pending_auto_exclusion_patch_rebases_when_persisted_settings_advance() {
    let persisted_revision = SettingsRevision::initial().next();
    let base = runtime_settings_with_runtime_and_persisted_revisions(
        Settings::default(),
        SettingsRevision::initial(),
        persisted_revision,
    );
    let automation = RuntimeHandle::start(&base);
    update_process_priority_status(
        &automation.shared,
        ProcessPrioritySnapshot {
            auto_excluded_processes: vec![r"C:\Apps\worker.exe".to_owned()],
            ..ProcessPrioritySnapshot::default()
        },
    );

    let next = runtime_settings_with_runtime_and_persisted_revisions(
        Settings::default(),
        base.runtime_revision,
        persisted_revision.next(),
    );
    automation.replace_settings(&next);

    let mut generation = 0;
    let patch = automation
        .take_auto_exclusion_patch_since(&mut generation)
        .expect("pending exclusion should remain queued");
    assert_eq!(patch.base_revision, next.persisted_revision);
    assert_eq!(patch.process_priority, vec![r"C:\Apps\worker.exe"]);
}

#[test]
fn failed_auto_exclusion_delivery_is_requeued_with_bounded_retry() {
    let base = runtime_settings_with_runtime_and_persisted_revisions(
        Settings::default(),
        SettingsRevision::initial(),
        SettingsRevision::initial().next(),
    );
    let automation = RuntimeHandle::start(&base);
    update_process_priority_status(
        &automation.shared,
        ProcessPrioritySnapshot {
            auto_excluded_processes: vec![r"C:\Apps\worker.exe".to_owned()],
            ..ProcessPrioritySnapshot::default()
        },
    );

    let mut generation = 0;
    let patch = automation
        .take_auto_exclusion_patch_since(&mut generation)
        .expect("first delivery");
    automation.requeue_auto_exclusion_patch(patch);

    assert!(automation
        .take_auto_exclusion_patch_since(&mut generation)
        .is_none());
    {
        let mut state = lock_unpoisoned(&automation.shared.state);
        state.pending_auto_exclusions_retry_at = Some(Instant::now());
    }

    let retry = automation
        .take_auto_exclusion_patch_since(&mut generation)
        .expect("delivery after retry deadline");
    assert_eq!(retry.base_revision, base.persisted_revision);
    assert_eq!(retry.process_priority, vec![r"C:\Apps\worker.exe"]);
}

#[test]
fn status_feature_status_arc_is_stable_when_feature_updates_are_no_op() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
    let snapshot = automation
        .status_snapshot_since(0)
        .expect("initial status snapshot should be visible");

    let snapshot_ptr = Arc::as_ptr(&snapshot.feature_status);

    update_cpu_sets_soft_status(
        &automation.shared,
        CpuAllocationSnapshot {
            auto_excluded_processes: vec![r"D:\Apps\game.exe".to_owned()],
            ..CpuAllocationSnapshot::default()
        },
    );

    let changed = automation
        .status_snapshot_since(snapshot.generation)
        .expect("feature changes should advance generation");
    let changed_ptr = Arc::as_ptr(&changed.feature_status);
    assert_ne!(snapshot_ptr, changed_ptr);

    update_cpu_sets_soft_status(
        &automation.shared,
        CpuAllocationSnapshot {
            auto_excluded_processes: vec![r"D:\Apps\game.exe".to_owned()],
            ..CpuAllocationSnapshot::default()
        },
    );
    let same = automation
        .status_snapshot_since(changed.generation)
        .is_none();
    assert!(same);
    let retained_ptr = {
        let state = lock_unpoisoned(&automation.shared.state);
        Arc::as_ptr(&state.status.feature_status)
    };
    assert_eq!(retained_ptr, changed_ptr);
}

#[test]
fn runtime_status_snapshot_default_dormancy() {
    let snapshot = RuntimeStatusSnapshot::default();
    assert_eq!(snapshot.generation, 0);
    assert_eq!(snapshot.appearance_change_generation, 0);
    assert_eq!(snapshot.worker_error, None);
    assert!(snapshot.action_log_entries.is_empty());
    assert!(snapshot.feature_status.as_ref() == &RuntimeFeatureStatus::default());
}

#[test]
fn runtime_settings_update_noop_uses_runtime_revision_only() {
    let settings = Settings::default();
    let baseline = runtime_settings_with_runtime_and_persisted_revisions(
        settings.clone(),
        SettingsRevision::initial(),
        SettingsRevision::initial(),
    );
    let runtime_unchanged_persisted_updated = runtime_settings_with_runtime_and_persisted_revisions(
        settings,
        baseline.runtime_revision,
        baseline.persisted_revision.next(),
    );

    let automation = RuntimeHandle::start(&baseline);
    let generation = {
        let state = lock_unpoisoned(&automation.shared.state);
        state.change_generation
    };

    automation.replace_settings(&runtime_unchanged_persisted_updated);

    let after = {
        let state = lock_unpoisoned(&automation.shared.state);
        (
            state.change_generation,
            state.persisted_revision,
            state.runtime_revision,
        )
    };
    assert_eq!(after.0, generation);
    assert_eq!(
        after.1,
        runtime_unchanged_persisted_updated.persisted_revision
    );
    assert_eq!(after.2, baseline.runtime_revision);
}

#[test]
fn pending_auto_exclusions_keep_same_named_executable_paths_distinct() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
    let mut generation = 0;

    update_process_priority_status(
        &automation.shared,
        ProcessPrioritySnapshot {
            auto_excluded_processes: vec![
                r"C:\Apps\Editor.exe".to_owned(),
                r"D:\Tools\Editor.exe".to_owned(),
            ],
            ..ProcessPrioritySnapshot::default()
        },
    );

    let pending = automation
        .take_auto_exclusion_patch_since(&mut generation)
        .expect("absolute executable paths should reach the pending queue");
    assert_eq!(
        pending.process_priority,
        vec![r"C:\Apps\Editor.exe", r"D:\Tools\Editor.exe"]
    );
}

#[test]
fn app_suspension_path_action_rejects_relative_paths_and_always_replies() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));

    assert!(matches!(
        automation.request_app_suspension_path_action("Editor.exe", true),
        Err(RuntimeCommandError::InvalidRequest(_))
    ));
    let receiver = automation
        .request_app_suspension_path_action(r"C:/Apps/Editor.exe", true)
        .expect("absolute path should be queued");
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(2)),
        Ok(Err(RuntimeCommandError::CommandFailed(_)))
    ));
    automation.shutdown().expect("shutdown");
}

#[test]
fn manual_app_suspension_request_starts_worker_without_automatic_rules() {
    let mut settings = Settings::default();
    settings.app_suspension.enabled = true;
    let automation = RuntimeHandle::start(&runtime_settings(settings));

    assert!(automation
        .thread
        .lock()
        .expect("automation thread state should remain available")
        .is_none());

    let _receiver = automation
        .request_app_suspension_path_action(r"C:\Apps\Editor.exe", true)
        .expect("request should be queued");

    assert!(automation
        .thread
        .lock()
        .expect("automation thread state should remain available")
        .is_some());
}

#[test]
fn automation_worker_runs_for_enabled_process_feature() {
    let mut settings = Settings::default();
    settings.efficiency_mode.enabled = true;

    assert!(automation_worker_required(&settings));
}

#[test]
fn automation_worker_runs_for_enabled_memory_trim() {
    let mut settings = Settings::default();
    settings.memory_trim.enabled = true;

    assert!(automation_worker_required(&settings));
}

#[test]
fn adaptive_engine_process_behaviours_independently_drive_polling() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.adaptive_engine_process.process_priority_enabled = false;
    settings
        .adaptive_engine_process
        .background_efficiency_enabled = false;
    settings
        .adaptive_engine_process
        .cpu_pressure_restraint_enabled = false;

    assert!(!adaptive_engine_process_required(&settings));

    settings
        .adaptive_engine_process
        .cpu_pressure_restraint_enabled = true;

    assert!(adaptive_engine_process_required(&settings));

    settings
        .adaptive_engine_process
        .cpu_pressure_restraint_enabled = false;
    settings
        .adaptive_engine_process
        .limit_background_processors_enabled = true;

    assert!(adaptive_engine_process_required(&settings));
}

#[test]
fn battery_only_feature_keeps_automation_and_power_events_available() {
    let mut settings = Settings::default();
    let mut battery = settings.clone();
    battery.process_priority.enabled = true;
    battery.on_battery = None;
    settings.on_battery = Some(Box::new(battery));

    assert!(automation_worker_required(&settings));
    assert!(windows_event_watcher_required(&settings));
}

#[test]
fn active_power_source_selects_the_matching_feature_profile() {
    let mut settings = Settings::default();
    settings.process_priority.enabled = false;
    settings.battery_profile_mut().process_priority.enabled = true;

    assert!(
        !active_power_source_settings(&settings, Some(true))
            .process_priority
            .enabled
    );
    assert!(
        active_power_source_settings(&settings, Some(false))
            .process_priority
            .enabled
    );
    assert!(
        !active_power_source_settings(&settings, None)
            .process_priority
            .enabled
    );
}

#[test]
fn default_settings_do_not_poll_power_plans_without_plan_targets() {
    let settings = Settings::default();

    assert!(!power_plan_checks_required(&settings));
}

#[test]
fn input_hook_is_needed_for_activity_input_or_app_suspension() {
    let mut settings = Settings::default();
    let inactive = InputHookConfig::default();

    assert_eq!(input_hook_config(&settings), inactive);

    settings.by_activity.enabled = true;
    settings.by_activity.power_plans.performance_guid = Some("active-guid".to_owned());
    assert_ne!(input_hook_config(&settings), inactive);

    settings.by_activity.enabled = false;
    assert_eq!(input_hook_config(&settings), inactive);

    settings.by_activity.enabled = true;
    settings.general.enabled = false;
    assert_eq!(input_hook_config(&settings), inactive);

    settings.general.enabled = true;
    settings.by_activity.switch_to_performance_on_resume = false;
    assert_eq!(input_hook_config(&settings), inactive);

    settings.by_activity.switch_to_performance_on_resume = true;
    settings.by_activity.input_detection.keyboard = false;
    settings.by_activity.input_detection.mouse = false;
    settings.by_activity.input_detection.controller = true;
    assert_eq!(input_hook_config(&settings), inactive);

    settings.app_suspension.enabled = true;
    assert_ne!(input_hook_config(&settings), inactive);

    settings.general.enabled = false;
    assert_eq!(input_hook_config(&settings), inactive);
}

#[test]
fn input_hook_config_tracks_enabled_input_devices() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = true;
    settings.by_activity.power_plans.power_save_guid = Some("idle-guid".to_owned());

    settings.by_activity.input_detection.keyboard = true;
    settings.by_activity.input_detection.mouse = false;
    assert_eq!(
        input_hook_config(&settings),
        InputHookConfig {
            keyboard: true,
            mouse: false,
        }
    );

    settings.by_activity.input_detection.keyboard = false;
    settings.by_activity.input_detection.mouse = true;
    assert_eq!(
        input_hook_config(&settings),
        InputHookConfig {
            keyboard: false,
            mouse: true,
        }
    );

    settings.by_activity.input_detection.mouse = false;
    settings.by_activity.input_detection.controller = true;
    assert_eq!(
        input_hook_config(&settings),
        InputHookConfig {
            keyboard: false,
            mouse: false,
        }
    );

    settings.app_suspension.enabled = true;
    assert_eq!(
        input_hook_config(&settings),
        InputHookConfig {
            keyboard: true,
            mouse: true,
        }
    );
}

#[test]
fn input_hook_config_ignores_inactive_power_profile_devices() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = true;
    settings.by_activity.power_plans.power_save_guid = Some("idle-guid".to_owned());
    settings.by_activity.input_detection.keyboard = true;
    settings.by_activity.input_detection.mouse = false;
    let battery = settings.battery_profile_mut();
    battery.by_activity.input_detection.keyboard = false;
    battery.by_activity.input_detection.mouse = true;
    battery.by_activity.power_plans.power_save_guid = None;
    let keyboard_only = InputHookConfig {
        keyboard: true,
        mouse: false,
    };
    assert_eq!(input_hook_config(&settings), keyboard_only);

    let battery = settings.battery_profile_mut();
    battery.by_activity.power_plans.power_save_guid = Some("battery-idle-guid".to_owned());
    battery.by_activity.enabled = false;
    assert_eq!(input_hook_config(&settings), keyboard_only);

    settings.battery_profile_mut().by_activity.enabled = true;
    let both_devices = InputHookConfig {
        keyboard: true,
        mouse: true,
    };
    assert_eq!(input_hook_config(&settings), both_devices);

    let battery = settings.battery_profile_mut();
    battery.general.enabled = false;
    battery.app_suspension.enabled = true;
    assert_eq!(input_hook_config(&settings), keyboard_only);

    settings.battery_profile_mut().general.enabled = true;
    settings.battery_profile_mut().by_activity.enabled = false;
    assert_eq!(input_hook_config(&settings), both_devices);
}

#[test]
fn app_suspension_uses_own_refresh_without_process_appearance_scan() {
    let mut settings = Settings::default();
    settings.app_suspension.enabled = true;
    settings
        .app_suspension
        .suspendable_apps
        .push(app_suspension_rule(r"C:\Apps\chat.exe"));

    assert!(feature_refresh_required(
        &settings,
        app_suspension_required(&settings)
    ));
    assert!(!process_appearance_scan_required(&settings));
}

#[test]
fn app_suspension_uses_windows_events_without_enabling_process_scan() {
    let mut settings = Settings::default();
    settings.app_suspension.enabled = true;
    settings
        .app_suspension
        .suspendable_apps
        .push(app_suspension_rule(r"C:\Apps\chat.exe"));

    assert!(windows_event_watcher_required(&settings));
    assert!(windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::WindowCreated
    ));
    assert!(!process_appearance_scan_required(&settings));
}

#[test]
fn app_suspension_source_checks_follow_actionable_rules() {
    let mut settings = Settings::default();
    let battery = &mut settings.battery_profile_mut().app_suspension;
    battery.enabled = true;
    battery
        .suspendable_apps
        .push(app_suspension_rule("chat.exe"));
    assert!(!app_suspension_policy_varies_by_source(&settings));

    settings
        .battery_profile_mut()
        .app_suspension
        .suspendable_apps[0]
        .executable_path = r"C:\Apps\chat.exe".to_owned();
    assert!(app_suspension_policy_varies_by_source(&settings));
    assert!(automation_worker_required(&settings));

    settings.app_suspension = settings.battery_profile().app_suspension.clone();
    settings
        .battery_profile_mut()
        .app_suspension
        .suspendable_apps[0]
        .executable_path = "  C:/Apps/chat.exe".to_owned();
    assert!(!app_suspension_policy_varies_by_source(&settings));
    settings
        .battery_profile_mut()
        .app_suspension
        .suspendable_apps[0]
        .executable_path = "C:/Apps/CHAT.EXE".to_owned();
    assert!(app_suspension_policy_varies_by_source(&settings));
}

#[test]
fn system_appearance_uses_windows_events_without_power_automation() {
    let mut settings = Settings::default();
    settings.general.enabled = false;
    settings.general.accent.source = AccentColorSource::Windows;

    assert!(windows_event_watcher_required(&settings));
    assert!(windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::AppearanceChanged
    ));
    assert!(!windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::PowerChanged
    ));

    settings.general.theme_mode = AppThemeMode::Amoled;
    assert!(appearance_events_required(&settings));
    settings.general.accent.source = AccentColorSource::Custom;
    assert!(!appearance_events_required(&settings));
}

#[test]
fn adaptive_engine_skips_appearance_only_windows_events() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.adaptive_engine.adaptive_processor_power_enabled = false;
    settings.general.accent.source = AccentColorSource::Windows;

    assert_eq!(windows_event_watcher_mode(&settings), None);
    assert!(!windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::AppearanceChanged
    ));

    settings
        .adaptive_engine_process
        .limit_background_processors_enabled = true;
    assert_eq!(
        windows_event_watcher_mode(&settings),
        Some(WindowsEventWatcherMode::SuspendResumeOnly)
    );
    assert!(windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::SuspendResume
    ));

    settings.app_suspension.enabled = true;
    settings
        .app_suspension
        .suspendable_apps
        .push(app_suspension_rule(r"C:\Apps\chat.exe"));

    assert!(automation_worker_required(&settings));
    assert_eq!(
        windows_event_watcher_mode(&settings),
        Some(WindowsEventWatcherMode::Full)
    );
    assert!(windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::WindowCreated
    ));
    assert!(!windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::AppearanceChanged
    ));

    let input_events = InputHookEvents {
        app_switch: true,
        mouse_click: true,
        ..InputHookEvents::default()
    };
    assert!(input_hook_should_check_app_switch(&settings, input_events));
    assert!(input_hook_should_check_app_switch_mouse_click(
        &settings,
        input_events
    ));
}

#[test]
fn event_driven_power_checks_drop_idle_polling_for_focus_only_rules() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = false;
    settings.by_focus.enabled = true;
    settings.by_focus.rules.push(ByFocusRule {
        enabled: true,
        name: "chat.exe".to_owned(),
        selector: ExecutableSelector::ExactName {
            executable_name: "chat.exe".to_owned(),
        },
        power_plan_guid: Some("active-guid".to_owned()),
    });

    assert!(power_plan_checks_required(&settings));
    assert!(windows_event_watcher_required(&settings));
    let wall_time = Local::now();
    assert!(power_plan_check_delay(&settings, true, None, wall_time, None).is_none());
    assert!(power_plan_check_delay(&settings, false, None, wall_time, None).is_some());
}

#[test]
fn paused_cpu_load_readback_keeps_earlier_deadlines_and_watcher_fallback() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = false;
    settings.by_cpu_load.enabled = true;
    settings
        .by_cpu_load
        .rules
        .push(crate::config::ByCpuLoadRule {
            enabled: true,
            power_plan_guid: Some("cpu-plan".into()),
            ..Default::default()
        });
    let wall_time = Local::now();
    let readback = Some(Duration::from_secs(10));
    assert_eq!(
        power_plan_check_delay(&settings, true, None, wall_time, readback),
        readback
    );
    assert_eq!(
        power_plan_check_delay(&settings, false, None, wall_time, readback),
        Some(configured_check_interval(&settings))
    );
    assert_eq!(
        power_plan_check_delay(&settings, true, None, wall_time, None),
        Some(CPU_USAGE_REFRESH_INTERVAL)
    );

    settings.by_activity.enabled = true;
    settings.by_activity.power_plans.power_save_guid = Some("idle-plan".into());
    let earlier = Duration::from_millis(250);
    let idle_for = Duration::from_secs(settings.by_activity.idle_timeout_seconds) - earlier;
    assert_eq!(
        power_plan_check_delay(&settings, true, Some(idle_for), wall_time, readback),
        Some(earlier)
    );
}

#[test]
fn activity_input_resume_waits_for_hook_event() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = true;
    settings.by_activity.power_plans.performance_guid = Some("active-guid".to_owned());

    assert!(power_plan_checks_required(&settings));
    assert!(windows_event_watcher_required(&settings));
    let wall_time = Local::now();
    assert!(power_plan_check_delay(&settings, true, None, wall_time, None).is_none());
    assert!(power_plan_check_delay(&settings, false, None, wall_time, None).is_some());
}

#[test]
fn configured_check_interval_clamps_imported_values() {
    let mut settings = Settings::default();
    settings.general.check_interval_ms = 0;
    assert_eq!(
        configured_check_interval(&settings),
        Duration::from_millis(CHECK_INTERVAL_MIN_MS)
    );

    settings.general.check_interval_ms = u64::MAX;
    assert_eq!(
        configured_check_interval(&settings),
        Duration::from_millis(CHECK_INTERVAL_MAX_MS)
    );
}

#[test]
fn schedule_checks_sleep_until_next_time_boundary() {
    let boundary_delay = Duration::from_secs(17);
    let delay = bounded_by_time_check_delay(Some(boundary_delay), Duration::from_secs(1));
    assert_eq!(delay, boundary_delay);
    let now = Instant::now();
    let mut scheduler = RefreshScheduler::new(now);
    assert_eq!(
        run_scheduled_power_plan_check(&mut scheduler, now, || Some(delay)),
        Some(delay)
    );
    assert!(!scheduler.is_due(RefreshDomain::PowerPlanCheck, now + delay / 2));
    assert!(scheduler.is_due(RefreshDomain::PowerPlanCheck, now + delay));
}

#[test]
fn schedule_checks_recheck_within_a_minute_without_clock_events() {
    assert_eq!(
        bounded_by_time_check_delay(None, Duration::from_secs(120)),
        Duration::from_secs(60)
    );
    let mut settings = Settings::default();
    settings.by_activity.enabled = false;
    settings.by_time.enabled = true;
    let wall_time = Local::now();
    let starts_at = wall_time + ChronoDuration::days(1);
    let ends_at = starts_at + ChronoDuration::minutes(1);
    settings.by_time.rules = vec![ByTimeRule {
        enabled: true,
        name: "Tomorrow".to_owned(),
        days: vec![WeekdaySetting::from_chrono(starts_at.weekday())],
        start_time: starts_at.format("%H:%M").to_string().parse().unwrap(),
        end_time: ends_at.format("%H:%M").to_string().parse().unwrap(),
        power_plan_guid: Some("scheduled-guid".to_owned()),
    }];

    let delay = power_plan_check_delay(&settings, true, None, wall_time, None).unwrap();
    assert_eq!(delay, Duration::from_secs(60));
    let now = Instant::now();
    let mut scheduler = RefreshScheduler::new(now);
    assert_eq!(
        run_scheduled_power_plan_check(&mut scheduler, now, || Some(delay)),
        Some(delay)
    );
    assert!(!scheduler.is_due(RefreshDomain::PowerPlanCheck, now + Duration::from_secs(59)));
    assert!(scheduler.is_due(RefreshDomain::PowerPlanCheck, now + Duration::from_secs(60)));
}

#[test]
fn by_activity_polls_when_it_can_target_a_power_plan() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = true;
    settings.by_activity.power_plans.power_save_guid = Some("idle-guid".to_owned());

    assert!(power_plan_checks_required(&settings));
}

#[test]
fn controller_activity_poll_requires_a_usable_plan() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = true;
    settings.by_activity.power_plans.power_save_guid = None;
    settings.by_activity.power_plans.performance_guid = Some("active-guid".to_owned());
    settings.by_activity.switch_to_performance_on_resume = false;

    assert!(!controller_activity_poll_required(&settings));

    settings.by_activity.switch_to_performance_on_resume = true;

    assert!(controller_activity_poll_required(&settings));
}

#[test]
fn process_appearance_scan_runs_for_enabled_process_features() {
    let mut settings = Settings::default();
    settings.efficiency_mode.enabled = true;

    assert!(process_appearance_scan_required(&settings));
    assert!(!power_plan_checks_required(&settings));
}

#[test]
fn disabled_automation_suppresses_worker_refreshes() {
    let mut settings = Settings::default();
    settings.general.enabled = false;
    settings.efficiency_mode.enabled = true;

    assert!(!feature_refresh_required(
        &settings,
        settings.efficiency_mode.enabled
    ));
    assert!(!process_appearance_scan_required(&settings));
    assert!(!power_plan_checks_required(&settings));
}

#[test]
fn adaptive_processor_power_and_process_policies_can_run_independently() {
    let mut settings = Settings::default();
    settings.adaptive_engine.adaptive_processor_power_enabled = true;
    settings
        .adaptive_engine_process
        .cpu_pressure_restraint_enabled = true;

    assert!(adaptive_power_plan_required(&settings));
    assert!(!adaptive_engine_process_required(&settings));

    settings.adaptive_engine.adaptive_processor_power_enabled = false;
    settings.adaptive_engine.enabled = true;
    assert!(!adaptive_power_plan_required(&settings));
    assert!(adaptive_engine_process_required(&settings));
}

#[test]
fn adaptive_processor_demand_separates_hybrid_core_classes() {
    let processors = [
        LogicalProcessorInfo {
            index: 0,
            core_index: 0,
            kind: LogicalProcessorKind::Performance,
            efficiency_class: 1,
        },
        LogicalProcessorInfo {
            index: 1,
            core_index: 1,
            kind: LogicalProcessorKind::Efficiency,
            efficiency_class: 0,
        },
    ];

    let demand = adaptive_processor_demand(&[72.0, 91.0], &processors);

    assert_eq!(demand.peak_cpu_percent, None);
    assert_eq!(demand.performance_peak_cpu_percent, Some(72.0));
    assert_eq!(demand.efficiency_peak_cpu_percent, Some(91.0));
}

#[test]
fn workload_reaction_interval_is_independent_from_adaptive_power_sampling() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.adaptive_engine.adaptive_processor_power_enabled = true;
    settings.adaptive_engine_process.reaction_time_ms = 1_500;

    assert_eq!(
        adaptive_engine_process_refresh_interval(&settings),
        Duration::from_millis(1_500)
    );
    assert_eq!(
        ADAPTIVE_POWER_PLAN_REFRESH_INTERVAL,
        Duration::from_millis(500)
    );
    assert!(ADAPTIVE_IO_REFRESH_INTERVAL > ADAPTIVE_POWER_PLAN_REFRESH_INTERVAL);

    settings.adaptive_engine_process.reaction_time_ms = 1;
    assert_eq!(
        adaptive_engine_process_refresh_interval(&settings),
        Duration::from_millis(crate::config::ADAPTIVE_ENGINE_PROCESS_REACTION_INTERVAL_MIN_MS)
    );
}

#[test]
fn adaptive_engine_process_requires_adaptive_engine() {
    let mut settings = Settings::default();
    settings.general.enabled = true;
    settings
        .adaptive_engine_process
        .cpu_pressure_restraint_enabled = true;

    assert!(!adaptive_engine_process_required(&settings));

    settings.adaptive_engine.enabled = true;
    assert!(adaptive_engine_process_required(&settings));

    settings
        .adaptive_engine_process
        .cpu_pressure_restraint_enabled = false;
    settings
        .adaptive_engine_process
        .limit_background_processors_enabled = true;
    assert!(adaptive_engine_process_required(&settings));
}
#[test]
fn automation_feature_execution_order_is_characterized() {
    assert_source_call_order(
        include_str!("../automation.rs"),
        "if efficiency_mode_refresh_required",
        "runner.publish_action_log_if_changed(&shared);",
        &[
            "runner.run_efficiency_mode_update(",
            "runner.run_process_priority_update(",
            "runner.run_adaptive_engine_process_update(",
            "runner.run_io_priority_update(",
            "runner.run_thread_priority_update(",
            "runner.run_dynamic_priority_boost_update(",
            "runner.run_gpu_priority_update(",
            "runner.run_memory_priority_update(",
            "runner.run_process_control_commands(",
            "runner.run_app_suspension_update(",
            "runner.run_cpu_sets_soft_update(",
            "runner.run_processor_affinity_hard_update(",
            "runner.run_instance_balancing_update(",
            "runner.run_cpu_limiter_update(",
            "runner.run_cpu_allocation_reconciliation(",
            "runner.run_by_running_app_update(",
            "runner.run_memory_trim_",
            "runner.run_timer_resolution_update(",
            "runner.run_sleep_prevention_update(",
            "runner.run_adaptive_power_plan_update(",
            "runner.run_check(",
        ],
    );
}

#[test]
fn cpu_allocation_handoffs_bypass_release_retry_deadlines() {
    let source = include_str!("../automation.rs");
    let route = source_scope(
        source,
        "let immediate_cpu_allocation_reconciliation =",
        "if by_running_app_due {",
    );
    assert!(route.contains("runner.cpu_allocation_immediate_reconciliation_pending()"));
    assert!(route.contains("cpu_allocation_release_retry_pending_at_pass_start"));
    assert!(route.contains("cpu_allocation_release_retry_due"));
}

#[test]
fn shared_property_precedence_inputs_are_characterized() {
    let source = include_str!("runner.rs");
    let workload = source_scope(
        source,
        "pub(super) fn run_adaptive_engine_process_update",
        "pub(super) fn run_adaptive_power_plan_update",
    );
    assert!(workload.contains("priority_efficiency_controller"));
    assert!(workload.contains("by_running_app_manager.active_process_id()"));
    assert!(workload
        .contains("explicit_cpu_allocation_paths(settings, alternate_cpu_allocation_profile)"));

    let process_priority = source_scope(
        source,
        "pub(super) fn run_process_priority_update",
        "pub(super) fn run_thread_priority_update",
    );
    assert!(process_priority.contains("priority_efficiency_controller"));

    let cpu_limiter = source_scope(
        source,
        "pub(super) fn run_cpu_limiter_update",
        "pub(super) fn run_cpu_allocation_reconciliation",
    );
    assert!(!cpu_limiter.contains("cpu_allocation_coordinator"));
    assert!(cpu_limiter.contains("cpu_limiter_controller"));
    assert!(cpu_limiter.contains("app_suspension_controller"));

    let cpu_allocation = include_str!("../../control/cpu_allocation.rs");
    assert_source_call_order(
        cpu_allocation,
        "fn cpu_allocation_owner_precedence()",
        "fn validate_owner(",
        &[
            "ControlOwner::CpuSetsSoft",
            "ControlOwner::ProcessorAffinityHard",
            "ControlOwner::InstanceBalancing",
            "ControlOwner::AdaptiveEngine",
        ],
    );
}

#[test]
fn power_plan_decisions_have_one_visibility_independent_runtime_route() {
    let ui_source = include_str!("../../ui/app.rs");
    assert!(!ui_source.contains("set_active("));

    let runner_source = include_str!("runner.rs");
    let runtime_decision = source_scope(
        runner_source,
        "pub(super) fn run_check",
        "pub(super) fn refresh_active_plan",
    );
    assert!(runtime_decision.contains("noncritical_foreground_process"));
    assert!(runtime_decision.contains("reconcile_ordinary"));

    let worker_source = include_str!("../automation.rs");
    let power_route = source_scope(
        worker_source,
        "let wait_now = Instant::now();",
        "wait_for = scheduler.minimum_wait",
    );
    assert!(power_route.contains("runner.run_check"));
    assert!(power_route.contains("power_plan_check_delay"));
    assert!(!power_route.contains("hidden_to_tray"));
    assert!(!power_route.contains("by_running_app_manager.is_active"));
}

#[test]
fn automation_shutdown_restores_reversible_features_in_reverse_order() {
    assert_source_call_order(
        include_str!("runner.rs"),
        "pub(super) fn shutdown(&mut self)",
        "pub(super) fn note_settings",
        &[
            "self.sleep_prevention_manager",
            "self.run_timer_resolution_update(",
            "self.run_by_running_app_update(",
            "self.run_cpu_limiter_update(",
            "self.cpu_limiter_controller.take()",
            "self.run_processor_affinity_hard_update(",
            "self.run_cpu_sets_soft_update(",
            "self.run_app_suspension_update(",
            "self.run_memory_priority_update(",
            "self.memory_priority_controller.shutdown(",
            "self.run_gpu_priority_update(",
            "self.gpu_priority_controller.shutdown(",
            "self.run_dynamic_priority_boost_update(",
            "self.dynamic_priority_boost_controller.shutdown(",
            "self.run_thread_priority_update(",
            "self.thread_priority_controller.shutdown(",
            "self.run_process_priority_update(",
            "self.run_io_priority_update(",
            "self.io_priority_controller.shutdown(",
            "self.run_adaptive_engine_process_update(",
            "self.cpu_allocation_coordinator.shutdown(",
            "self.run_efficiency_mode_update(",
            "self.priority_efficiency_controller.shutdown(",
            "self.power_plan_controller.shutdown(",
        ],
    );
}

#[test]
fn shutdown_retains_terminal_worker_results() {
    for failure in [false, true] {
        let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
        automation.shutdown().expect("initial idle shutdown");
        *lock_unpoisoned(&automation.shutdown_result) = None;
        *lock_unpoisoned(&automation.thread) = Some(thread::spawn(move || {
            if failure {
                Err("injected cleanup failure".into())
            } else {
                Ok(())
            }
        }));
        let first = automation.shutdown();
        assert_eq!(first.is_err(), failure);
        assert_eq!(automation.shutdown(), first);
        assert!(lock_unpoisoned(&automation.shared.state).stop_requested);
        if failure {
            assert!(first.unwrap_err().contains("injected cleanup failure"));
            let first_status = automation.status_snapshot_since(0).unwrap();
            let second_status = automation.status_snapshot_since(0).unwrap();
            assert_eq!(first_status.worker_error, second_status.worker_error);
            assert!(second_status.worker_error.is_some());
        }
    }
}

#[test]
fn unrelated_wakes_preserve_periodic_power_plan_checks() {
    let start = Instant::now();
    let mut scheduler = RefreshScheduler::new(start);
    let mut checks = Vec::new();
    for ms in [
        0, 250, 370, 500, 750, 1000, 1250, 1500, 1750, 2000, 2250, 2500, 2750, 3000,
    ] {
        let now = start + Duration::from_millis(ms);
        let wait = run_scheduled_power_plan_check(&mut scheduler, now, || {
            checks.push(ms);
            Some(Duration::from_secs(1))
        });
        assert_eq!(wait, Some(Duration::from_millis(1000 - ms % 1000)));
        scheduler.schedule_after(
            RefreshDomain::ControllerActivity,
            now,
            Duration::from_millis(250),
        );
        assert!(
            scheduler
                .minimum_wait(
                    wait,
                    now,
                    [(
                        true,
                        RefreshDomain::ControllerActivity,
                        Duration::from_millis(250)
                    )]
                )
                .unwrap()
                <= Duration::from_millis(250)
        );
    }
    assert_eq!(checks, [0, 1000, 2000, 3000]);
    let now = start + Duration::from_millis(3100);
    scheduler.invalidate(SchedulerEvent::SettingsChanged, now);
    assert_eq!(
        run_scheduled_power_plan_check(&mut scheduler, now, || None),
        None
    );
    assert!(scheduler.is_due(RefreshDomain::PowerPlanCheck, now));
}

fn shared_automation_state(settings: Settings) -> SharedAutomationState {
    SharedAutomationState {
        state: Mutex::new(AutomationWorkerState {
            settings: Arc::new(settings),
            runtime_revision: SettingsRevision::initial(),
            persisted_revision: SettingsRevision::initial(),
            change_generation: 0,
            status: RuntimeStatusSnapshot::default(),
            pending_auto_exclusions: AutoExclusionPatch::default(),
            pending_auto_exclusions_revision: None,
            pending_auto_exclusions_retry_at: None,
            process_control_commands: VecDeque::new(),
            idle_action_log: Some(ActionLog::default()),
            action_log_clear_requested: false,
            pending_events: AutomationWakeEvents::default(),
            pending_keep_running_resets: Default::default(),
            pending_instance_limits_resets: instance_limits::PendingResets::default(),
            pending_instance_balancing_resets: instance_balancing::PendingResets::default(),
            pending_sleep_prevention_settings: Default::default(),
            input_activity: InputActivityTracker::default(),
            windows_event_watcher_active: true,
            worker_accepting_work: false,
            stop_requested: false,
        }),
        changed: Condvar::new(),
        status_generation: AtomicU64::new(0),
        pending_auto_exclusions_generation: AtomicU64::new(0),
        auto_exclusion_wake: Mutex::new(None),
    }
}

#[test]
fn instance_balancing_status_rejects_samples_from_superseded_intake() {
    let mut settings = Settings::default();
    settings.general.enabled = true;
    settings.instance_balancing.enabled = true;
    settings
        .instance_balancing
        .rules
        .push(crate::config::InstanceBalancingRule {
            enabled: true,
            executable_path: r"C:\Apps\original.exe".into(),
        });
    settings
        .instance_balancing
        .rules
        .push(crate::config::InstanceBalancingRule {
            enabled: true,
            executable_path: r"C:\Before\original.exe".into(),
        });
    let shared = shared_automation_state(settings.clone());
    let old = automation_snapshot(&shared).unwrap();
    let mut observed = InstanceBalancingSnapshot {
        enabled: true,
        scanned_processes: 2,
        adjusted_processes: 2,
        detail: Some(InstanceBalancingDetail::Assigned(2)),
        ..Default::default()
    };
    let original_rule = settings.instance_balancing.rules[0].clone();
    observed.record_rule(
        Arc::new(original_rule.clone()),
        ActionLogResult::Applied,
        InstanceBalancingDetail::Assigned(2),
    );
    settings.instance_balancing.rules[1].executable_path = r"C:\Other\original.exe".into();
    let current_rule = settings.instance_balancing.rules[1].clone();
    {
        let mut state = lock_unpoisoned(&shared.state);
        instance_balancing::record_settings_changes(&mut state, &settings);
        state.settings = Arc::new(settings);
    }
    instance_balancing::publish_status(&shared, &old.settings, true, observed.clone());
    assert_eq!(shared.status_generation.load(Ordering::Acquire), 0);

    let current = automation_snapshot(&shared).unwrap();
    let error = "incomplete observation".to_owned();
    let detail = InstanceBalancingDetail::Diagnostic(error.clone());
    observed.last_error = Some(error);
    observed.detail = Some(detail.clone());
    observed.record_rule(
        Arc::new(current_rule.clone()),
        ActionLogResult::Skipped,
        detail,
    );
    instance_balancing::publish_status(&shared, &current.settings, true, observed);
    assert_eq!(shared.status_generation.load(Ordering::Acquire), 1);
    {
        let state = lock_unpoisoned(&shared.state);
        let status = &state.status.feature_status.instance_balancing;
        assert!(!status.process_activity_known);
        assert_eq!(
            status.rule_result(&original_rule),
            Some((
                ActionLogResult::Applied,
                &InstanceBalancingDetail::Assigned(2)
            ))
        );
        assert!(matches!(
            status.rule_result(&current_rule),
            Some((
                ActionLogResult::Skipped,
                InstanceBalancingDetail::Diagnostic(_)
            ))
        ));
        let unsaved = crate::config::InstanceBalancingRule {
            executable_path: r"C:\Draft\original.exe".into(),
            ..current_rule.clone()
        };
        assert_eq!(status.rule_result(&unsaved), None);
        let disabled = crate::config::InstanceBalancingRule {
            enabled: false,
            ..current_rule.clone()
        };
        assert_eq!(status.rule_result(&disabled), None);
    }

    set_windows_event_watcher_active(&shared, false);
    {
        let state = lock_unpoisoned(&shared.state);
        assert_eq!(
            state
                .status
                .feature_status
                .instance_balancing
                .rule_result(&original_rule),
            None
        );
    }
    set_windows_event_watcher_active(&shared, true);
    instance_balancing::publish_status(
        &shared,
        &current.settings,
        true,
        InstanceBalancingSnapshot::default(),
    );
    assert_eq!(shared.status_generation.load(Ordering::Acquire), 2);
}

#[test]
fn tray_visibility_edges_remain_pending_before_the_worker_waits() {
    let shared = shared_automation_state(Settings::default());
    lock_unpoisoned(&shared.state).worker_accepting_work = true;
    let before = automation_snapshot(&shared).unwrap().change_generation;

    notify_tray_visibility_changed(&shared);
    notify_tray_visibility_changed(&shared);

    assert_eq!(lock_unpoisoned(&shared.state).change_generation, before + 2);
    assert!(!wait_for_wake(&shared, None, before));
    let snapshot = automation_snapshot(&shared).unwrap();
    assert_eq!(
        snapshot.wake_events,
        AutomationWakeEvents {
            tray_visibility_changed: true,
            ..Default::default()
        }
    );
    assert_eq!(
        automation_snapshot(&shared).unwrap().wake_events,
        AutomationWakeEvents::default()
    );
}

#[test]
fn system_bottleneck_sampling_restarts_after_a_tray_restore() {
    use std::sync::mpsc;

    let mut settings = Settings::default();
    settings.general.enabled = true;
    settings.adaptive_engine.enabled = true;
    settings.adaptive_engine.adaptive_processor_power_enabled = false;
    assert!(automation_worker_required(&settings));
    assert!(!process_appearance_scan_required(&settings));
    assert!(!power_plan_checks_required(&settings));

    let shared = Arc::new(shared_automation_state(settings));
    lock_unpoisoned(&shared.state).worker_accepting_work = true;
    let (entered, passes) = mpsc::channel();
    let (advance, visibility) = mpsc::channel();
    let (sampled, samples) = mpsc::channel();
    let worker_shared = Arc::clone(&shared);
    let worker = thread::spawn(move || {
        run_background_automation(
            worker_shared,
            || Some(true),
            || {
                let _ = entered.send(());
                visibility.recv().unwrap_or(true)
            },
            |_| {
                let _ = sampled.send(());
                BottleneckSnapshot {
                    enabled: true,
                    ..BottleneckSnapshot::default()
                }
            },
        )
    });
    let timeout = Duration::from_secs(3);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        passes.recv_timeout(timeout).unwrap();
        for (hidden, sample_count) in [(true, 0), (false, 1), (true, 0), (false, 1)] {
            // Queue the next visibility edge before the current pass waits. The next pass
            // must observe that edge and can only begin after this one has published.
            notify_tray_visibility_changed(&shared);
            advance.send(hidden).unwrap();
            passes
                .recv_timeout(timeout)
                .expect("worker did not process the tray event");
            let state = lock_unpoisoned(&shared.state);
            assert_eq!(
                state.status.feature_status.bottleneck_classifier.enabled,
                !hidden
            );
            assert_eq!(samples.try_iter().count(), sample_count);
        }
    }));
    {
        let mut state = lock_unpoisoned(&shared.state);
        state.stop_requested = true;
        shared.changed.notify_one();
    }
    drop(advance);
    worker.join().unwrap().unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[test]
fn battery_only_system_bottleneck_detects_source_changes_without_full_event_coverage() {
    use std::sync::{atomic::AtomicBool, mpsc};

    let mut settings = Settings::default();
    settings.general.theme_mode = AppThemeMode::Dark;
    settings.general.accent.source = AccentColorSource::Custom;
    let battery = settings.battery_profile_mut();
    battery.adaptive_engine.enabled = true;
    assert!(!adaptive_engine_process_required(battery));
    assert!(!adaptive_power_plan_required(battery));

    let shared = Arc::new(shared_automation_state(settings));
    {
        let mut state = lock_unpoisoned(&shared.state);
        state.worker_accepting_work = true;
        state.windows_event_watcher_active = false;
    }
    let plugged_in = Arc::new(AtomicBool::new(true));
    let (pass, passes) = mpsc::channel();
    let worker_shared = Arc::clone(&shared);
    let worker_plugged_in = Arc::clone(&plugged_in);
    let worker = thread::spawn(move || {
        run_background_automation(
            worker_shared,
            || {
                let plugged_in = worker_plugged_in.load(Ordering::Acquire);
                let _ = pass.send(plugged_in);
                Some(plugged_in)
            },
            || false,
            |_| BottleneckSnapshot {
                enabled: true,
                ..BottleneckSnapshot::default()
            },
        )
    });
    let timeout = Duration::from_secs(4);
    let classifier_enabled = || {
        lock_unpoisoned(&shared.state)
            .status
            .feature_status
            .bottleneck_classifier
            .enabled
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Let the otherwise idle worker get past its startup refreshes.
        for _ in 0..3 {
            assert!(passes.recv_timeout(timeout).unwrap());
        }
        assert!(!classifier_enabled());

        for new_source in [false, true] {
            plugged_in.store(new_source, Ordering::Release);
            let deadline = Instant::now() + timeout;
            while classifier_enabled() == new_source {
                assert!(Instant::now() < deadline, "source change was not processed");
                thread::sleep(Duration::from_millis(5));
            }
        }
    }));
    {
        let mut state = lock_unpoisoned(&shared.state);
        state.stop_requested = true;
        shared.changed.notify_one();
    }
    worker.join().unwrap().unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[test]
fn resource_thresholds_actions_require_current_policy_and_no_shutdown_or_session_transition() {
    let mut settings = Settings::default();
    settings.general.enabled = true;
    settings.resource_thresholds.enabled = true;
    settings
        .resource_thresholds
        .rules
        .push(crate::config::ResourceThresholdsRule::new(
            r"C:\Apps\test.exe".into(),
        ));
    let shared = shared_automation_state(settings.clone());
    assert!(resource_thresholds_action_is_current(&shared, &settings));

    {
        let mut state = lock_unpoisoned(&shared.state);
        state.windows_event_watcher_active = false;
        state.pending_events.windows_event_watcher_change = Some(WindowsEventWatcherChange::Lost);
        state.pending_events.settings_changed = true;
        state.pending_events.power_changed = true;
        let current = Arc::make_mut(&mut state.settings);
        current.memory_trim.enabled = !current.memory_trim.enabled;
    }
    assert!(resource_thresholds_action_is_current(&shared, &settings));

    type Change<T> = (&'static str, fn(&mut T));
    let settings_changes: [Change<Settings>; 4] = [
        ("master disabled", |current| current.general.enabled = false),
        ("Resource Thresholds disabled", |current| {
            current.resource_thresholds.enabled = false
        }),
        ("rule disabled", |current| {
            current.resource_thresholds.rules[0].enabled = false
        }),
        ("cross-session permission changed", |current| {
            current.general.allow_cross_session_process_control =
                !current.general.allow_cross_session_process_control;
        }),
    ];
    for (name, change) in settings_changes {
        let mut current = settings.clone();
        change(&mut current);
        let shared = shared_automation_state(current);
        assert!(
            !resource_thresholds_action_is_current(&shared, &settings),
            "{name}"
        );
    }

    let intents: [Change<AutomationWorkerState>; 3] = [
        ("shutdown", |state| state.stop_requested = true),
        ("suspend or resume", |state| {
            state.pending_events.suspend_resume = true
        }),
        ("session change", |state| {
            state.pending_events.session_changed = true
        }),
    ];
    for (name, queue) in intents {
        let shared = shared_automation_state(settings.clone());
        queue(&mut lock_unpoisoned(&shared.state));
        assert!(
            !resource_thresholds_action_is_current(&shared, &settings),
            "{name}"
        );
    }
}

#[test]
fn resource_thresholds_notification_rechecks_the_saved_rule_without_blocking_actions() {
    use crate::config::ResourceThresholdsRule;

    let mut settings = Settings::default();
    settings.general.enabled = true;
    settings.resource_thresholds.enabled = true;
    settings.resource_thresholds.notify = true;
    settings.resource_thresholds.rules = [r"C:\Apps\worker.exe", r"C:\Apps\editor.exe"]
        .into_iter()
        .map(|path| ResourceThresholdsRule::new(path.into()))
        .collect();
    let shared = shared_automation_state(settings.clone());
    assert!(resource_thresholds_policy_is_current(
        &shared,
        &settings,
        Some(0)
    ));

    Arc::make_mut(&mut lock_unpoisoned(&shared.state).settings)
        .resource_thresholds
        .rules[0]
        .notify = false;
    assert!(!resource_thresholds_policy_is_current(
        &shared,
        &settings,
        Some(0)
    ));
    assert!(resource_thresholds_policy_is_current(
        &shared,
        &settings,
        Some(1)
    ));
    assert!(resource_thresholds_action_is_current(&shared, &settings));
}

#[test]
fn foreground_changes_skip_power_plan_rules_that_do_not_use_focus() {
    let defaults = Settings::default();
    let mut activity = defaults.clone();
    activity.by_activity.enabled = true;
    activity.by_activity.input_detection.keyboard = true;
    activity.by_activity.power_plans.power_save_guid = Some("idle-plan".into());
    activity.by_activity.power_plans.performance_guid = Some("active-plan".into());
    let mut time = defaults.clone();
    time.by_time.enabled = true;
    time.by_time.rules[0].power_plan_guid = Some("time-plan".into());
    let mut cpu = defaults;
    cpu.by_cpu_load.enabled = true;
    cpu.by_cpu_load.rules.push(crate::config::ByCpuLoadRule {
        enabled: true,
        power_plan_guid: Some("cpu-plan".into()),
        ..Default::default()
    });

    for (name, settings) in [
        ("By Activity", activity),
        ("By Time", time),
        ("By CPU Load", cpu),
    ] {
        assert!(power_plan_checks_required(&settings), "{name}");
        assert_eq!(
            windows_event_watcher_mode(&settings),
            Some(WindowsEventWatcherMode::Full)
        );
        let shared = shared_automation_state(settings);
        notify_windows_event(&shared, WindowsAutomationEvent::ForegroundChanged);
        let snapshot = automation_snapshot(&shared).unwrap();
        assert_eq!(snapshot.change_generation, 0, "{name}");
        assert_eq!(
            snapshot.wake_events,
            AutomationWakeEvents::default(),
            "{name}"
        );

        notify_windows_event(&shared, WindowsAutomationEvent::PowerChanged);
        let snapshot = automation_snapshot(&shared).unwrap();
        assert_eq!(snapshot.change_generation, 1, "{name}");
        assert!(snapshot.wake_events.power_changed, "{name}");
    }
}

#[test]
fn callbacks_deliver_events_for_either_profile_without_polling() {
    let shared = shared_automation_state(Settings::default());
    for battery_only in [true, false] {
        let mut settings = Settings::default();
        settings.battery_profile_mut();
        let profile = if battery_only {
            settings.battery_profile_mut()
        } else {
            &mut settings
        };
        profile.general.enabled = true;
        profile.by_time.enabled = true;
        profile.by_time.rules.push(crate::config::ByTimeRule {
            enabled: true,
            days: vec![WeekdaySetting::Mon],
            power_plan_guid: Some("time-plan".into()),
            ..Default::default()
        });
        profile.by_focus.enabled = true;
        profile.by_focus.rules.push(ByFocusRule {
            enabled: true,
            name: String::new(),
            selector: ExecutableSelector::ExactPath {
                executable_path: r"C:\Apps\test.exe".into(),
            },
            power_plan_guid: Some("test-plan".into()),
        });
        assert!(windows_event_watcher_required(&settings));
        lock_unpoisoned(&shared.state).settings = Arc::new(settings.clone());
        for event in [
            WindowsAutomationEvent::ForegroundChanged,
            WindowsAutomationEvent::PowerChanged,
            WindowsAutomationEvent::SuspendResume,
            WindowsAutomationEvent::SessionChanged,
            WindowsAutomationEvent::ClockChanged,
        ] {
            let before = lock_unpoisoned(&shared.state).change_generation;
            notify_windows_event(&shared, event);
            assert_eq!(lock_unpoisoned(&shared.state).change_generation, before + 1);
            assert!(!wait_for_wake(&shared, None, before));
            let snapshot = automation_snapshot(&shared).unwrap();
            let mut expected = AutomationWakeEvents::default();
            expected.insert_windows_event(event);
            assert_eq!(snapshot.wake_events, expected);
            assert_eq!(snapshot.change_generation, before + 1);
        }
        let profile = if battery_only {
            settings.battery_profile_mut()
        } else {
            &mut settings
        };
        profile.by_focus.enabled = false;
        profile.process_priority.enabled = true;
        lock_unpoisoned(&shared.state).settings = Arc::new(settings.clone());
        notify_windows_event(&shared, WindowsAutomationEvent::ForegroundChanged);
        notify_windows_event(&shared, WindowsAutomationEvent::PowerChanged);
        let events = automation_snapshot(&shared).unwrap().wake_events;
        assert!(events.foreground_changed && events.power_changed);

        let profile = if battery_only {
            settings.battery_profile_mut()
        } else {
            &mut settings
        };
        profile.by_activity.enabled = true;
        profile.by_activity.input_detection.keyboard = true;
        profile.by_activity.input_detection.mouse = false;
        profile.app_suspension.enabled = true;
        lock_unpoisoned(&shared.state).settings = Arc::new(settings.clone());
        let before = lock_unpoisoned(&shared.state).change_generation;
        notify_input_event(
            &shared,
            InputHookEvents {
                keyboard: true,
                ..Default::default()
            },
        );
        assert_eq!(lock_unpoisoned(&shared.state).change_generation, before);

        let profile = if battery_only {
            settings.battery_profile_mut()
        } else {
            &mut settings
        };
        profile.by_activity.power_plans.power_save_guid = Some("idle-guid".to_owned());
        lock_unpoisoned(&shared.state).settings = Arc::new(settings.clone());
        notify_input_event(
            &shared,
            InputHookEvents {
                mouse: true,
                ..Default::default()
            },
        );
        assert_eq!(
            automation_snapshot(&shared).unwrap().wake_events,
            AutomationWakeEvents::default()
        );
        notify_input_event(
            &shared,
            InputHookEvents {
                keyboard: true,
                app_switch: true,
                mouse_click: true,
                ..Default::default()
            },
        );
        let events = automation_snapshot(&shared).unwrap().wake_events;
        assert!(events.input_activity && events.app_switch && events.app_switch_mouse_click);

        settings.general.enabled = false;
        settings.battery_profile_mut().general.enabled = false;
        lock_unpoisoned(&shared.state).settings = Arc::new(settings);
        let before = lock_unpoisoned(&shared.state).change_generation;
        notify_windows_event(&shared, WindowsAutomationEvent::ForegroundChanged);
        notify_input_event(
            &shared,
            InputHookEvents {
                keyboard: true,
                app_switch: true,
                mouse_click: true,
                ..Default::default()
            },
        );
        assert_eq!(lock_unpoisoned(&shared.state).change_generation, before);
    }
    lock_unpoisoned(&shared.state).stop_requested = true;
    let before = lock_unpoisoned(&shared.state).change_generation;
    notify_windows_event(&shared, WindowsAutomationEvent::AppearanceChanged);
    assert_eq!(lock_unpoisoned(&shared.state).change_generation, before);
}

#[test]
fn idle_battery_worker_survives_source_changes_without_settings_updates() {
    use std::sync::{atomic::AtomicBool, mpsc};

    let mut settings = Settings::default();
    settings.general.theme_mode = AppThemeMode::Dark;
    settings.general.accent.source = AccentColorSource::Custom;
    settings.battery_profile_mut();
    let path = std::env::temp_dir().join(format!(
        "winderust-power-source-audit-{}.exe",
        std::process::id()
    ));
    assert!(path.is_absolute() && !path.exists());
    settings.by_focus.enabled = true;
    settings.by_focus.rules.push(ByFocusRule {
        enabled: true,
        name: "nonexistent audit target".into(),
        selector: ExecutableSelector::ExactPath {
            executable_path: path.to_str().unwrap().to_owned(),
        },
        power_plan_guid: Some("invalid-audit-plan".into()),
    });
    assert!(automation_worker_required(&settings));
    assert!(!automation_worker_required(settings.battery_profile()));
    assert!(windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::PowerChanged
    ));

    for watcher_active in [true, false] {
        let shared = Arc::new(shared_automation_state(settings.clone()));
        {
            let mut state = lock_unpoisoned(&shared.state);
            state.worker_accepting_work = true;
            state.windows_event_watcher_active = watcher_active;
        }
        let source = Arc::new(AtomicBool::new(false));
        let worker_source = Arc::clone(&source);
        let worker_shared = Arc::clone(&shared);
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_background_automation(
                worker_shared,
                || {
                    let plugged_in = worker_source.load(Ordering::Acquire);
                    let _ = tx.send(plugged_in);
                    Some(plugged_in)
                },
                || false,
                RuntimeCore::run_bottleneck_classifier_update,
            )
        });
        let timeout = Duration::from_secs(4);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // A profile change schedules one refresh of disabled controls.
            for _ in 0..2 {
                assert!(!rx.recv_timeout(timeout).unwrap());
            }
            for plugged_in in [true, false, true] {
                assert!(!worker.is_finished());
                source.store(plugged_in, Ordering::Release);
                if watcher_active {
                    notify_windows_event(&shared, WindowsAutomationEvent::PowerChanged);
                }
                let deadline = Instant::now() + timeout;
                loop {
                    let observed = rx
                        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        .unwrap_or_else(|error| {
                            panic!("source {plugged_in} not observed with watcher={watcher_active}: {error}")
                        });
                    if observed == plugged_in {
                        break;
                    }
                }
                if !plugged_in {
                    assert!(!rx.recv_timeout(timeout).unwrap());
                }
            }
        }));
        {
            let mut state = lock_unpoisoned(&shared.state);
            state.stop_requested = true;
            shared.changed.notify_one();
        }
        worker.join().unwrap().unwrap();
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}

#[test]
fn all_adaptive_allocation_gates_and_battery_only_zones_schedule_independently() {
    for master in [false, true] {
        for bits in 0..8 {
            let mut settings = Settings::default();
            settings.general.enabled = master;
            settings.adaptive_engine.enabled = true;
            settings.adaptive_engine.adaptive_processor_power_enabled = false;
            let process = &mut settings.adaptive_engine_process;
            process.cpu_pressure_restraint_enabled = bits & 1 != 0;
            process.limit_background_processors_enabled = bits & 2 != 0;
            process.dynamic_resource_zones_enabled = bits & 4 != 0;
            assert_eq!(
                feature_refresh_required(&settings, adaptive_engine_process_required(&settings)),
                master && bits != 0
            );
            settings.adaptive_engine.enabled = false;
            assert!(!adaptive_engine_process_required(&settings));
        }
    }
    let mut settings = Settings::default();
    settings.general.theme_mode = AppThemeMode::Dark;
    settings.general.accent.source = AccentColorSource::Custom;
    settings.adaptive_engine.enabled = false;
    let battery = settings.battery_profile_mut();
    battery.adaptive_engine.enabled = true;
    battery.adaptive_engine.adaptive_processor_power_enabled = false;
    battery
        .adaptive_engine_process
        .dynamic_resource_zones_enabled = true;
    assert!(automation_worker_required(&settings));
    assert!(!windows_event_watcher_required(&settings));
    assert_eq!(
        windows_event_watcher_mode(&settings),
        Some(WindowsEventWatcherMode::Full)
    );
    assert!(windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::PowerChanged
    ));
    assert!(!adaptive_engine_process_required(
        active_power_source_settings(&settings, Some(true))
    ));
    assert!(adaptive_engine_process_required(
        active_power_source_settings(&settings, Some(false))
    ));

    // The diagnostic pauses while hidden, so it cannot wake A/C when only Battery runs policies.
    settings.adaptive_engine.enabled = true;
    assert_eq!(
        windows_event_watcher_mode(&settings),
        Some(WindowsEventWatcherMode::Full)
    );

    settings
        .adaptive_engine_process
        .dynamic_resource_zones_enabled = true;
    assert_eq!(
        windows_event_watcher_mode(&settings),
        Some(WindowsEventWatcherMode::SuspendResumeOnly)
    );
}

#[test]
fn adaptive_engine_waits_only_for_unverified_source_dependent_cpu_behaviour() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings
        .adaptive_engine_process
        .cpu_pressure_restraint_enabled = true;
    let battery = settings.battery_profile_mut();
    battery.adaptive_engine.adaptive_processor_power_enabled = true;
    battery.adaptive_engine_process.processor_limit_percent = 99;
    battery.adaptive_engine_process.process_priority_enabled = true;
    assert!(!adaptive_engine_process_waits_for_source(&settings, None));
    assert!(adaptive_power_plan_waits_for_source(&settings, None));

    settings
        .battery_profile_mut()
        .adaptive_engine
        .adaptive_processor_power_enabled = false;
    settings
        .battery_profile_mut()
        .adaptive_engine_process
        .background_app_cpu_threshold_percent = 99;
    assert!(!adaptive_enablement_varies_by_source(&settings));
    assert_eq!(
        windows_event_watcher_mode(&settings),
        Some(WindowsEventWatcherMode::SuspendResumeOnly)
    );
    assert!(adaptive_engine_process_waits_for_source(&settings, None));
    assert!(!adaptive_power_plan_waits_for_source(&settings, None));
    assert!(process_appearance_scan_required(&settings));
    assert!(!process_appearance_scan_required_for_runtime(
        &settings, false, false, false, false, false
    ));
    assert!(!adaptive_engine_process_waits_for_source(
        &settings,
        Some(true)
    ));
    assert!(!adaptive_engine_process_waits_for_source(
        &settings,
        Some(false)
    ));

    settings.adaptive_engine.enabled = false;
    assert!(adaptive_engine_process_waits_for_source(&settings, None));
    assert!(adaptive_enablement_varies_by_source(&settings));
    settings.battery_profile_mut().adaptive_engine.enabled = false;
    assert!(!adaptive_engine_process_waits_for_source(&settings, None));
}

#[test]
fn adaptive_engine_waits_for_source_dependent_running_app_exclusions() {
    use crate::config::{ByRunningAppRule, ExecutableSelector};

    let name = |value: &str, plan: Option<&str>| ByRunningAppRule {
        enabled: true,
        name: value.to_owned(),
        selector: ExecutableSelector::ExactName {
            executable_name: value.to_owned(),
        },
        power_plan_guid: plan.map(str::to_owned),
    };
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings
        .adaptive_engine_process
        .cpu_pressure_restraint_enabled = true;
    settings.by_running_app.enabled = true;
    settings.by_running_app.rules = vec![
        name("game.exe", Some("ac-plan")),
        name("editor.exe", Some("ac-plan")),
    ];
    let battery = settings.battery_profile_mut();
    let mut relative = name("relative", Some("battery-plan"));
    relative.selector = ExecutableSelector::ExactPath {
        executable_path: "relative.exe".into(),
    };
    battery.by_running_app.rules = vec![
        name("unconfigured.exe", None),
        name("GAME.EXE", Some("battery-plan")),
        relative,
        name("editor.exe", Some("battery-plan")),
    ];
    assert!(!adaptive_engine_process_waits_for_source(&settings, None));
    assert!(ordinary_power_plan_waits_for_source(&settings, None));

    settings
        .battery_profile_mut()
        .by_running_app
        .rules
        .swap(1, 3);
    assert!(adaptive_engine_process_waits_for_source(&settings, None));
    for source in [Some(true), Some(false)] {
        assert!(!adaptive_engine_process_waits_for_source(&settings, source));
    }

    settings.adaptive_engine.enabled = false;
    settings.battery_profile_mut().adaptive_engine.enabled = false;
    assert!(!adaptive_engine_process_waits_for_source(&settings, None));
}
