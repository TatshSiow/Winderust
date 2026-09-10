use super::*;
use chrono::{Datelike, Duration as ChronoDuration, Local};
use std::sync::Arc;

use crate::action_log::{
    ActionLogFeature, ActionLogFeatureSummary, ActionLogResult, ActionLogSummaries,
};
use crate::application::settings::{RuntimeSettingsSnapshot, SettingsRevision};
use crate::config::{
    AppSuspensionRule, ByForegroundRule, ByRunningAppRule, ByTimeRule, CpuAllocationRule,
    CpuLimiterRule, ProcessDynamicPriorityBoostSetting, ProcessExclusionRule,
    ProcessGpuPrioritySetting, ProcessRuleMode, ProcessThreadPrioritySetting, TimerResolutionRule,
    WeekdaySetting,
};

fn runtime_settings(snapshot: Settings) -> RuntimeSettingsSnapshot {
    runtime_settings_with_runtime_and_persisted_revisions(
        snapshot,
        SettingsRevision::initial(),
        SettingsRevision::initial(),
    )
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
    let mut known = BTreeSet::new();

    assert!(!process_ids_have_new_entries(
        &mut known,
        BTreeSet::from([1, 2])
    ));
    assert_eq!(known, BTreeSet::from([1, 2]));
}

#[test]
fn process_appearance_detector_reports_new_process_ids() {
    let mut known = BTreeSet::from([1, 2]);

    assert!(process_ids_have_new_entries(
        &mut known,
        BTreeSet::from([1, 2, 3])
    ));
    assert_eq!(known, BTreeSet::from([1, 2, 3]));
}

#[test]
fn automation_event_deadline_invalidations_are_characterized() {
    let source = include_str!("../automation.rs");
    assert!(source.contains("RefreshScheduler::new"));
    assert!(source.contains("SchedulerEvent::SettingsChanged"));
    assert!(source.contains("SchedulerEvent::ForegroundChanged"));
    assert!(source.contains("SchedulerEvent::WindowCreated"));
    assert!(source.contains("SchedulerEvent::ProcessAppeared"));
    assert!(!source.contains("let mut next_"));
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
        owner: Some(crate::control::power_plan::PowerPlanOwner::OrdinaryAutomation),
        current_guid: Some("current".to_owned()),
        target_guid: Some("target".to_owned()),
        decision_state: Some(crate::rules::DecisionState::ByTime),
        reason: Some("test".to_owned()),
    };

    update_power_plan_status(&automation.shared, status.clone());

    let snapshot = automation
        .status_snapshot_since(1)
        .expect("power-plan status advances the status generation");
    assert_eq!(snapshot.power_plan_status.as_ref(), &status);
}

#[test]
fn power_plan_action_logs_are_attributed_to_the_winning_rule_family() {
    assert_eq!(
        power_plan_action_log_feature(DecisionState::ByForeground),
        Some(ActionLogFeature::ByForeground)
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
fn clearing_action_log_immediately_clears_runtime_summaries() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
    let entry = ActionLogEntry {
        sequence: 1,
        timestamp_epoch_ms: 1,
        feature: ActionLogFeature::ByTime,
        process_id: None,
        process_name: String::new(),
        result: ActionLogResult::Applied,
        reason: "applied".to_owned(),
    };
    update_action_log(
        &automation.shared,
        vec![entry.clone()],
        ActionLogSummaries::from([(
            ActionLogFeature::ByTime,
            ActionLogFeatureSummary {
                successful_actions: 1,
                last_success: Some(entry),
                ..Default::default()
            },
        )]),
    );
    let published = automation
        .status_snapshot_since(1)
        .expect("published action summary advances status generation");

    automation.clear_action_log();

    let cleared = automation
        .status_snapshot_since(published.generation)
        .expect("clearing action summaries advances status generation");
    assert!(cleared.action_log_entries.is_empty());
    assert!(cleared.action_log_summaries.is_empty());
}

#[test]
fn runtime_handle_shutdown_is_idempotent() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
    assert!(automation.shutdown().is_ok());
    assert!(automation.shutdown().is_ok());
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

    let memory_trim_error = automation.request_memory_trim_now().err();
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

    let receiver = automation
        .request_memory_trim_now()
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
        outcome.into_process_list_result().unwrap_err(),
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

    assert_eq!(result, "2 of 3 process actions failed: first failure");
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
    let mut known = BTreeSet::from([1, 2, 3]);

    assert!(!process_ids_have_new_entries(
        &mut known,
        BTreeSet::from([1, 2])
    ));
    assert_eq!(known, BTreeSet::from([1, 2]));
}

#[test]
fn process_appearance_scan_sleeps_when_process_features_are_off() {
    let settings = Settings::default();

    assert!(!process_appearance_scan_required(&settings));
}

#[test]
fn foreground_lookup_runs_only_for_configured_by_foreground() {
    let mut settings = Settings::default();

    assert!(!foreground_lookup_required(&settings));

    settings.by_foreground.enabled = true;
    assert!(!foreground_lookup_required(&settings));

    settings.by_foreground.rules.push(ByForegroundRule {
        enabled: true,
        name: "editor.exe".to_owned(),
        executable_path: String::new(),
        power_plan_guid: None,
    });
    assert!(!foreground_lookup_required(&settings));

    settings.by_foreground.rules[0].power_plan_guid = Some("active-guid".to_owned());
    assert!(!foreground_lookup_required(&settings));

    settings.by_foreground.rules[0].executable_path = r"C:\Apps\editor.exe".to_owned();
    assert!(foreground_lookup_required(&settings));

    settings.by_foreground.rules[0].enabled = false;
    assert!(!foreground_lookup_required(&settings));
}

#[test]
fn automation_worker_sleeps_when_no_automation_work_exists() {
    let settings = Settings::default();

    assert!(!automation_worker_required(&settings));
}

#[test]
fn enabled_empty_rule_features_do_not_poll() {
    let mut settings = Settings::default();
    settings.app_suspension.enabled = true;
    settings.cpu_sets_soft.enabled = true;
    settings.cpu_limiter.enabled = true;
    settings.by_running_app.enabled = true;
    settings.timer_resolution.enabled = true;
    settings.by_foreground.enabled = true;

    assert!(!automation_worker_required(&settings));

    settings
        .app_suspension
        .suspendable_apps
        .push(app_suspension_rule(" "));
    settings.cpu_sets_soft.rules.push(CpuAllocationRule {
        enabled: true,
        executable_path: " ".to_owned(),
        focus_core_mask: 1,
        visible_window_core_mask: 1,
        background_core_mask: 1,
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
        executable_path: " ".to_owned(),
        power_plan_guid: Some("active-guid".to_owned()),
    });
    settings.timer_resolution.rules.push(TimerResolutionRule {
        enabled: true,
        executable_path: " ".to_owned(),
        desired_100ns: 5_000,
    });
    settings.by_foreground.rules.push(ByForegroundRule {
        enabled: true,
        name: "Empty".to_owned(),
        executable_path: " ".to_owned(),
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
        focus_core_mask: 1,
        visible_window_core_mask: 1,
        background_core_mask: 1,
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
        executable_path: r"C:\Apps\chat.exe".to_owned(),
        power_plan_guid: Some("active-guid".to_owned()),
    });
    settings.timer_resolution.enabled = true;
    settings.timer_resolution.rules.push(TimerResolutionRule {
        enabled: true,
        executable_path: r"C:\Apps\chat.exe".to_owned(),
        desired_100ns: 5_000,
    });
    settings.by_foreground.enabled = true;
    settings.by_foreground.rules.push(ByForegroundRule {
        enabled: true,
        name: "Chat".to_owned(),
        executable_path: r"C:\Apps\chat.exe".to_owned(),
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
        focus_core_mask: 1,
        visible_window_core_mask: 1,
        background_core_mask: 1,
    };
    settings.cpu_sets_soft.rules.push(rule.clone());
    settings.processor_affinity_hard.rules.push(rule);

    assert!(processor_affinity_hard_settings(&settings).rules.is_empty());
    settings.processor_affinity_hard.enabled = true;
    assert!(!processor_affinity_hard_required(&settings));
}

#[test]
fn explicit_cpu_allocation_paths_exclude_only_active_usable_rules() {
    let mut settings = Settings::default();
    settings.cpu_sets_soft.enabled = true;
    settings.cpu_sets_soft.rules.push(CpuAllocationRule {
        enabled: true,
        executable_path: r"C:\Apps\soft.exe".to_owned(),
        focus_core_mask: 1,
        visible_window_core_mask: 1,
        background_core_mask: 1,
    });
    settings.processor_affinity_hard.enabled = true;
    settings
        .processor_affinity_hard
        .rules
        .push(CpuAllocationRule {
            enabled: false,
            executable_path: r"C:\Apps\disabled.exe".to_owned(),
            focus_core_mask: 1,
            visible_window_core_mask: 1,
            background_core_mask: 1,
        });
    settings
        .processor_affinity_hard
        .rules
        .push(CpuAllocationRule {
            enabled: true,
            executable_path: r"C:\Apps\empty.exe".to_owned(),
            focus_core_mask: 0,
            visible_window_core_mask: 0,
            background_core_mask: 0,
        });

    let paths = explicit_cpu_allocation_paths(&settings);

    assert_eq!(paths.len(), 1);
    assert!(paths.contains(&crate::foreground::process_failure_key(r"C:\Apps\soft.exe")));
}

#[test]
fn automation_worker_runs_for_adaptive_power_plan_alone() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = false;
    settings.by_foreground.enabled = false;
    settings.adaptive_engine.enabled = true;
    settings.adaptive_engine.processor_power_policy_enabled = true;

    assert!(automation_worker_required(&settings));
}

#[test]
fn automation_worker_runs_for_bottleneck_classifier_alone() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = false;
    settings.by_foreground.enabled = false;
    settings.adaptive_engine.enabled = true;
    settings.adaptive_engine.processor_power_policy_enabled = false;

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
            process_appearance_refresh_interval(&settings, hidden_to_tray, adaptive_engine_enabled,),
            PROCESS_APPEARANCE_SCAN_INTERVAL
        );
    }

    settings.cpu_limiter.enabled = false;
    assert_eq!(
        process_appearance_refresh_interval(&settings, false, true),
        ADAPTIVE_ENGINE_AUTOMATION_REFRESH_INTERVAL
    );
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
fn pending_auto_exclusions_are_taken_only_after_generation_change() {
    let automation = RuntimeHandle::start(&runtime_settings(Settings::default()));
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

    let pending = automation
        .take_auto_exclusion_patch_since(&mut generation)
        .expect("new pending affinity exclusions should be visible");
    assert_eq!(pending.base_revision, SettingsRevision::initial());
    assert_eq!(pending.cpu_sets_soft, vec![r"D:\Games\Game.exe"]);
    assert!(automation
        .take_auto_exclusion_patch_since(&mut generation)
        .is_none());
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
    settings.background_efficiency.enabled = true;

    assert!(automation_worker_required(&settings));
}

#[test]
fn automation_worker_runs_for_enabled_memory_trim() {
    let mut settings = Settings::default();
    settings.memory_trim.enabled = true;

    assert!(automation_worker_required(&settings));
}

#[test]
fn cpu_scheduler_io_assist_waits_for_pressure() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.cpu_scheduler.io_priority.enabled = true;
    settings.cpu_scheduler.io_priority.foreground_priority = ProcessIoPriority::Normal.into();
    settings.cpu_scheduler.io_priority.background_priority = ProcessIoPriority::Low.into();

    assert!(!effective_io_priority_settings(&settings, false).enabled);

    let io_priority = effective_io_priority_settings(&settings, true);

    assert!(io_priority.enabled);
    assert_eq!(
        io_priority_control_owner(&settings, true),
        ControlOwner::AdaptiveEngine
    );
    assert_eq!(
        io_priority_control_owner(&settings, false),
        ControlOwner::IoPriority
    );
    assert!(io_priority.foreground_detection_enabled);
    assert_eq!(
        io_priority.foreground_priority.priority(),
        Some(ProcessIoPriority::Normal)
    );
    assert_eq!(
        io_priority.background_priority.priority(),
        Some(ProcessIoPriority::Low)
    );
}

#[test]
fn cpu_scheduler_pressure_feeds_priority_defaults() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.cpu_scheduler.cpu_pressure_restraint_enabled = true;
    settings.cpu_scheduler.io_priority.enabled = true;
    settings
        .cpu_scheduler
        .io_priority
        .foreground_detection_enabled = false;
    settings
        .cpu_scheduler
        .io_priority
        .preserve_foreground_priority = false;
    settings
        .cpu_scheduler
        .io_priority
        .preserve_background_priority = false;
    settings.cpu_scheduler.io_priority.background_priority = ProcessIoPriority::Low.into();
    settings
        .cpu_scheduler
        .thread_priority
        .foreground_detection_enabled = false;
    settings
        .cpu_scheduler
        .thread_priority
        .preserve_foreground_priority = false;
    settings
        .cpu_scheduler
        .thread_priority
        .preserve_background_priority = false;
    settings
        .cpu_scheduler
        .dynamic_priority_boost
        .foreground_detection_enabled = false;
    settings
        .cpu_scheduler
        .gpu_priority
        .foreground_detection_enabled = false;
    settings
        .cpu_scheduler
        .gpu_priority
        .preserve_foreground_priority = false;
    settings
        .cpu_scheduler
        .gpu_priority
        .preserve_background_priority = false;
    settings.cpu_scheduler.custom_rules = vec![ProcessExclusionRule {
        executable_path: "game.exe".to_owned(),
        ..Default::default()
    }];

    assert!(thread_priority_required(&settings));
    assert!(dynamic_priority_boost_required(&settings));
    assert!(gpu_priority_required(&settings));
    assert_eq!(
        thread_priority_control_owner(&settings, true),
        ControlOwner::AdaptiveEngine
    );
    assert_eq!(
        thread_priority_control_owner(&settings, false),
        ControlOwner::ThreadPriority
    );

    let thread_priority = effective_thread_priority_settings(&settings, true);
    assert!(thread_priority.enabled);
    assert!(thread_priority.foreground_detection_enabled);
    assert!(thread_priority.preserve_foreground_priority);
    assert!(thread_priority.preserve_background_priority);
    assert_eq!(
        thread_priority.background_priority,
        ProcessThreadPrioritySetting::BelowNormal
    );
    assert!(thread_priority.contains_exclusion("game.exe"));

    let dynamic_priority_boost = effective_dynamic_priority_boost_settings(&settings, true);
    assert!(dynamic_priority_boost.enabled);
    assert!(dynamic_priority_boost.foreground_detection_enabled);
    assert_eq!(
        dynamic_priority_boost.foreground_boost,
        ProcessDynamicPriorityBoostSetting::Enabled
    );
    assert_eq!(
        dynamic_priority_boost.background_boost,
        ProcessDynamicPriorityBoostSetting::Disabled
    );
    assert!(dynamic_priority_boost.contains_exclusion("game.exe"));

    let io_priority = effective_io_priority_settings(&settings, true);
    assert_eq!(
        io_priority_control_owner(&settings, true),
        ControlOwner::AdaptiveEngine
    );
    assert_eq!(
        io_priority.background_priority.priority(),
        Some(ProcessIoPriority::Low)
    );
    assert!(io_priority.foreground_detection_enabled);
    assert!(io_priority.preserve_foreground_priority);
    assert!(io_priority.preserve_background_priority);
    assert!(io_priority.contains_exclusion("game.exe"));

    let gpu_priority = effective_gpu_priority_settings(&settings, true);
    assert_eq!(
        gpu_priority_control_owner(&settings, true),
        ControlOwner::AdaptiveEngine
    );
    assert_eq!(
        gpu_priority_control_owner(&settings, false),
        ControlOwner::GpuPriority
    );
    assert!(gpu_priority.enabled);
    assert!(gpu_priority.foreground_detection_enabled);
    assert!(gpu_priority.preserve_foreground_priority);
    assert!(gpu_priority.preserve_background_priority);
    assert_eq!(
        gpu_priority.background_priority,
        ProcessGpuPrioritySetting::BelowNormal
    );
    assert!(gpu_priority.contains_exclusion("game.exe"));
}

#[test]
fn cpu_scheduler_behaviours_independently_drive_polling() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.cpu_scheduler.process_priority_enabled = false;
    settings.cpu_scheduler.background_efficiency_enabled = false;
    settings.cpu_scheduler.cpu_pressure_restraint_enabled = false;

    assert!(!cpu_scheduler_required(&settings));

    settings.cpu_scheduler.cpu_pressure_restraint_enabled = true;

    assert!(cpu_scheduler_required(&settings));

    settings.cpu_scheduler.cpu_pressure_restraint_enabled = false;
    settings.cpu_scheduler.limit_background_processors_enabled = true;

    assert!(cpu_scheduler_required(&settings));
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
fn cpu_scheduler_priority_assist_temporarily_overrides_global_priority_defaults() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.cpu_scheduler.cpu_pressure_restraint_enabled = true;
    settings.thread_priority.enabled = true;
    settings.thread_priority.background_priority = ProcessThreadPrioritySetting::Idle;
    settings.dynamic_priority_boost.enabled = true;
    settings.dynamic_priority_boost.background_boost = ProcessDynamicPriorityBoostSetting::Enabled;
    settings.gpu_priority.enabled = true;
    settings.gpu_priority.background_priority = ProcessGpuPrioritySetting::Idle;
    settings.cpu_scheduler.thread_priority.background_priority =
        ProcessThreadPrioritySetting::BelowNormal;
    settings
        .cpu_scheduler
        .dynamic_priority_boost
        .background_boost = ProcessDynamicPriorityBoostSetting::Disabled;
    settings.cpu_scheduler.gpu_priority.background_priority =
        ProcessGpuPrioritySetting::BelowNormal;

    assert_eq!(
        effective_thread_priority_settings(&settings, true).background_priority,
        ProcessThreadPrioritySetting::BelowNormal
    );
    assert_eq!(
        effective_dynamic_priority_boost_settings(&settings, true).background_boost,
        ProcessDynamicPriorityBoostSetting::Disabled
    );
    assert_eq!(
        dynamic_priority_boost_control_owner(&settings, true),
        ControlOwner::AdaptiveEngine
    );
    assert_eq!(
        effective_gpu_priority_settings(&settings, true).background_priority,
        ProcessGpuPrioritySetting::BelowNormal
    );
    assert_eq!(
        gpu_priority_control_owner(&settings, true),
        ControlOwner::AdaptiveEngine
    );
    assert_eq!(
        effective_thread_priority_settings(&settings, false).background_priority,
        ProcessThreadPrioritySetting::Idle
    );
    assert_eq!(
        effective_dynamic_priority_boost_settings(&settings, false).background_boost,
        ProcessDynamicPriorityBoostSetting::Enabled
    );
    assert_eq!(
        dynamic_priority_boost_control_owner(&settings, false),
        ControlOwner::DynamicPriorityBoost
    );
    assert_eq!(
        effective_gpu_priority_settings(&settings, false).background_priority,
        ProcessGpuPrioritySetting::Idle
    );
    assert_eq!(
        gpu_priority_control_owner(&settings, false),
        ControlOwner::GpuPriority
    );
}

#[test]
fn cpu_scheduler_without_io_assist_does_not_require_io_refresh() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.cpu_scheduler.cpu_pressure_restraint_enabled = true;

    assert!(!io_priority_required(&settings));
}

#[test]
fn default_settings_do_not_poll_power_plans_without_plan_targets() {
    let settings = Settings::default();

    assert!(!power_plan_checks_required(&settings));
}

#[test]
fn input_hook_is_needed_for_activity_input_or_app_suspension() {
    let mut settings = Settings::default();

    assert!(!input_hook_required(&settings));

    settings.by_activity.enabled = true;
    settings.by_activity.power_plans.performance_guid = Some("active-guid".to_owned());
    assert!(input_hook_required(&settings));

    settings.by_activity.enabled = false;
    assert!(!input_hook_required(&settings));

    settings.by_activity.enabled = true;
    settings.general.enabled = false;
    assert!(!input_hook_required(&settings));

    settings.general.enabled = true;
    settings.by_activity.switch_to_performance_on_resume = false;
    assert!(!input_hook_required(&settings));

    settings.by_activity.switch_to_performance_on_resume = true;
    settings.by_activity.input_detection.keyboard = false;
    settings.by_activity.input_detection.mouse = false;
    settings.by_activity.input_detection.controller = true;
    assert!(!input_hook_required(&settings));

    settings.app_suspension.enabled = true;
    assert!(input_hook_required(&settings));

    settings.general.enabled = false;
    assert!(!input_hook_required(&settings));
}

#[test]
fn input_hook_config_tracks_enabled_input_devices() {
    let mut settings = Settings::default();

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
}

#[test]
fn adaptive_engine_skips_appearance_only_windows_events() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.general.accent.source = AccentColorSource::Windows;

    assert!(!windows_event_watcher_required(&settings));
    assert!(!windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::AppearanceChanged
    ));

    settings.app_suspension.enabled = true;
    settings
        .app_suspension
        .suspendable_apps
        .push(app_suspension_rule(r"C:\Apps\chat.exe"));

    assert!(automation_worker_required(&settings));
    assert!(windows_event_watcher_required(&settings));
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
fn event_driven_power_checks_drop_idle_polling_for_foreground_only_rules() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = false;
    settings.by_foreground.enabled = true;
    settings.by_foreground.rules.push(ByForegroundRule {
        enabled: true,
        name: "chat.exe".to_owned(),
        executable_path: r"C:\Apps\chat.exe".to_owned(),
        power_plan_guid: Some("active-guid".to_owned()),
    });

    assert!(power_plan_checks_required(&settings));
    assert!(windows_event_watcher_required(&settings));
    assert!(power_plan_check_delay(&settings, true).is_none());
    assert!(power_plan_check_delay(&settings, false).is_some());
}

#[test]
fn activity_input_resume_waits_for_hook_event() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = true;
    settings.by_activity.power_plans.performance_guid = Some("active-guid".to_owned());

    assert!(power_plan_checks_required(&settings));
    assert!(windows_event_watcher_required(&settings));
    assert!(power_plan_check_delay(&settings, true).is_none());
    assert!(power_plan_check_delay(&settings, false).is_some());
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
    let mut settings = Settings::default();
    settings.by_activity.enabled = false;
    settings.by_time.enabled = true;
    let starts_at = Local::now() + ChronoDuration::minutes(3);
    let ends_at = starts_at + ChronoDuration::minutes(1);
    settings.by_time.rules = vec![ByTimeRule {
        enabled: true,
        name: "Soon".to_owned(),
        days: vec![WeekdaySetting::from_chrono(starts_at.weekday())],
        start_time: starts_at.format("%H:%M").to_string(),
        end_time: ends_at.format("%H:%M").to_string(),
        power_plan_guid: Some("scheduled-guid".to_owned()),
    }];

    let delay = power_plan_check_delay(&settings, true).unwrap();

    assert!(delay > configured_check_interval(&settings));
    assert!(delay <= Duration::from_secs(180));
}

#[test]
fn schedule_checks_cap_long_sleeps() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = false;
    settings.by_time.enabled = true;
    let starts_at = Local::now() + ChronoDuration::days(1);
    let ends_at = starts_at + ChronoDuration::minutes(1);
    settings.by_time.rules = vec![ByTimeRule {
        enabled: true,
        name: "Tomorrow".to_owned(),
        days: vec![WeekdaySetting::from_chrono(starts_at.weekday())],
        start_time: starts_at.format("%H:%M").to_string(),
        end_time: ends_at.format("%H:%M").to_string(),
        power_plan_guid: Some("scheduled-guid".to_owned()),
    }];

    assert_eq!(
        power_plan_check_delay(&settings, true),
        Some(SCHEDULE_RULE_MAX_SLEEP)
    );
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
    settings.background_efficiency.enabled = true;

    assert!(process_appearance_scan_required(&settings));
    assert!(!power_plan_checks_required(&settings));
}

#[test]
fn disabled_automation_suppresses_worker_refreshes() {
    let mut settings = Settings::default();
    settings.general.enabled = false;
    settings.background_efficiency.enabled = true;

    assert!(!feature_refresh_required(
        &settings,
        settings.background_efficiency.enabled
    ));
    assert!(!process_appearance_scan_required(&settings));
    assert!(!power_plan_checks_required(&settings));
}

#[test]
fn adaptive_plan_follows_adaptive_engine_processor_policy() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.adaptive_engine.processor_power_policy_enabled = true;

    assert!(adaptive_power_plan_required(&settings));

    settings.adaptive_engine.processor_power_policy_enabled = false;
    assert!(!adaptive_power_plan_required(&settings));
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
    settings.adaptive_engine.processor_power_policy_enabled = true;
    settings.cpu_scheduler.reaction_time_ms = 1_500;

    assert_eq!(
        cpu_scheduler_refresh_interval(&settings),
        Duration::from_millis(1_500)
    );
    assert_eq!(
        ADAPTIVE_POWER_PLAN_REFRESH_INTERVAL,
        Duration::from_millis(500)
    );
    assert!(ADAPTIVE_IO_REFRESH_INTERVAL > ADAPTIVE_POWER_PLAN_REFRESH_INTERVAL);

    settings.cpu_scheduler.reaction_time_ms = 1;
    assert_eq!(
        cpu_scheduler_refresh_interval(&settings),
        Duration::from_millis(crate::config::CPU_SCHEDULER_REACTION_INTERVAL_MIN_MS)
    );
}

#[test]
fn cpu_scheduler_requires_adaptive_engine() {
    let mut settings = Settings::default();
    settings.general.enabled = true;
    settings.cpu_scheduler.cpu_pressure_restraint_enabled = true;

    assert!(!cpu_scheduler_required(&settings));
    assert!(!cpu_scheduler_priority_assist_required(&settings));

    settings.adaptive_engine.enabled = true;
    assert!(cpu_scheduler_required(&settings));
    assert!(cpu_scheduler_priority_assist_required(&settings));

    settings.cpu_scheduler.cpu_pressure_restraint_enabled = false;
    settings.cpu_scheduler.limit_background_processors_enabled = true;
    assert!(cpu_scheduler_required(&settings));
    assert!(!cpu_scheduler_priority_assist_required(&settings));
}
#[test]
fn power_plan_checks_sleep_when_decision_features_are_off() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = false;
    settings.by_foreground.enabled = false;
    settings.by_time.enabled = false;
    settings.by_cpu_load.enabled = false;
    settings.by_running_app.enabled = false;

    assert!(!power_plan_checks_required(&settings));
}

#[test]
fn automation_feature_execution_order_is_characterized() {
    assert_source_call_order(
        include_str!("../automation.rs"),
        "if background_efficiency_refresh_required",
        "runner.publish_action_log_if_changed(&shared);",
        &[
            "runner.run_background_efficiency_update(",
            "runner.run_cpu_scheduler_update(",
            "runner.run_adaptive_power_plan_update(",
            "runner.run_io_priority_update(",
            "runner.run_process_priority_update(",
            "runner.run_thread_priority_update(",
            "runner.run_dynamic_priority_boost_update(",
            "runner.run_gpu_priority_update(",
            "runner.run_memory_priority_update(",
            "runner.run_process_control_commands(",
            "runner.run_app_suspension_update(",
            "runner.run_cpu_sets_soft_update(",
            "runner.run_processor_affinity_hard_update(",
            "runner.run_cpu_limiter_update(",
            "runner.run_cpu_allocation_reconciliation(",
            "runner.run_by_running_app_update(",
            "runner.run_memory_trim_",
            "runner.run_timer_resolution_update(",
        ],
    );
}

#[test]
fn cpu_allocation_handoffs_bypass_release_retry_deadlines() {
    let source = include_str!("../automation.rs");
    let route = source_scope(
        source,
        "let immediate_cpu_allocation_reconciliation =",
        "if by_running_app_refresh_required",
    );
    assert!(route.contains("runner.cpu_allocation_immediate_reconciliation_pending()"));
    assert!(route.contains("cpu_allocation_release_retry_pending_at_pass_start"));
    assert!(route.contains("cpu_allocation_release_retry_due"));
    assert!(route.contains(
        "runner.run_cpu_allocation_reconciliation(settings, cpu_allocation_release_retry_due)"
    ));
}

#[test]
fn shared_property_precedence_inputs_are_characterized() {
    let source = include_str!("runner.rs");
    let workload = source_scope(
        source,
        "pub(super) fn run_cpu_scheduler_update",
        "pub(super) fn run_adaptive_power_plan_update",
    );
    assert!(workload.contains("priority_efficiency_controller"));
    assert!(workload.contains("by_running_app_manager.active_process_ids()"));
    assert!(workload.contains("explicit_cpu_allocation_paths(settings)"));

    let process_priority = source_scope(
        source,
        "pub(super) fn run_process_priority_update",
        "pub(super) fn run_thread_priority_update",
    );
    assert!(process_priority.contains("priority_efficiency_controller"));
    assert!(process_priority.contains("ControlOwner::BackgroundEfficiency"));
    assert!(process_priority.contains("ControlOwner::AdaptiveEngine"));
    assert!(process_priority.contains("ControlOwner::CpuSchedulerFocusPriority"));

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
            "ControlOwner::AdaptiveEngine",
        ],
    );
}

#[test]
fn power_plan_decisions_have_one_visibility_independent_runtime_route() {
    let ui_source = include_str!("../../ui/app.rs");
    assert!(!ui_source.contains("decide("));
    assert!(!ui_source.contains("record_power_plan_change"));
    assert!(!ui_source.contains("set_active("));

    let runner_source = include_str!("runner.rs");
    let runtime_decision = source_scope(
        runner_source,
        "pub(super) fn run_check",
        "pub(super) fn refresh_active_plan",
    );
    assert!(runtime_decision.contains("process_is_critical"));
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
            "self.run_cpu_scheduler_update(",
            "self.cpu_allocation_coordinator.shutdown(",
            "self.run_background_efficiency_update(",
            "self.priority_efficiency_controller.shutdown(",
            "self.power_plan_controller.shutdown(",
        ],
    );
}
