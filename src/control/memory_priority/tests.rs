use std::{
    path::PathBuf,
    process::{Child, Command},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use super::*;

#[test]
fn unrelated_owner_releases_preserve_the_pending_cleanup_deadline() {
    let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
    controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::MemoryPriority,
                ProcessMemoryPriority::Low,
            ),
            true,
        )
        .unwrap();
    controller.platform.deny_query = true;
    assert_eq!(
        controller
            .release_all_policy(ControlOwner::MemoryPriority)
            .failures
            .len(),
        1
    );
    let due = controller.next_release_retry.unwrap();
    for _ in 0..8 {
        assert!(controller
            .release_all_policy(ControlOwner::AdaptiveEngine)
            .failures
            .is_empty());
        assert_eq!(controller.next_release_retry, Some(due));
    }
    controller.platform.deny_query = false;
    assert_eq!(controller.retry_pending_releases(due).restored_processes, 1);
    assert_eq!(controller.platform.processes[&7].value, 5);
    assert!(controller.release_retry_delay(due).is_none());
}

#[test]
fn disabled_cleanup_retries_with_bounded_cadence_and_respects_manual_replacement() {
    for replace in [false, true] {
        let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
        controller
            .apply_policy_claim(
                claim(
                    7,
                    1,
                    ControlOwner::MemoryPriority,
                    ProcessMemoryPriority::Low,
                ),
                true,
            )
            .unwrap();
        controller.platform.deny_query = true;
        assert_eq!(
            controller
                .release_all_policy(ControlOwner::MemoryPriority)
                .failures
                .len(),
            1
        );
        assert!(controller.static_claims.is_empty());
        assert_eq!(controller.pending_releases.len(), 1);
        let due = controller.next_release_retry.unwrap();
        let now = due - Duration::from_millis(1);
        assert!(controller.retry_pending_releases(now).failures.is_empty());
        assert_eq!(controller.retry_pending_releases(due).failures.len(), 1);
        assert_eq!(
            controller.release_retry_delay(due),
            Some(Duration::from_secs(1))
        );
        assert!(controller
            .retry_pending_releases(due + Duration::from_millis(999))
            .failures
            .is_empty());
        controller.platform.deny_query = false;
        for fail_commit in [false, true] {
            controller.platform.fail_next_begin = !fail_commit;
            controller.platform.fail_next_commit = fail_commit;
            assert!(controller
                .apply_process_list_action(
                    &action_target(7, 1),
                    ProcessMemoryPriority::Medium,
                    true
                )
                .is_err());
            assert_eq!(controller.pending_releases.len(), 1);
        }
        if replace {
            controller
                .apply_process_list_action(
                    &action_target(7, 1),
                    ProcessMemoryPriority::Medium,
                    true,
                )
                .unwrap();
        }
        let summary = controller.retry_pending_releases(due + Duration::from_secs(1));
        assert!(summary.failures.is_empty());
        assert_eq!(
            controller.platform.processes[&7].value,
            if replace { 3 } else { 5 }
        );
        assert!(controller.release_retry_delay(now).is_none());
        controller.shutdown().unwrap();
    }
}

#[test]
fn pending_release_resolves_the_surviving_memory_claim() {
    let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
    controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::MemoryPriority,
                ProcessMemoryPriority::Medium,
            ),
            true,
        )
        .unwrap();
    controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::AdaptiveEngine,
                ProcessMemoryPriority::Low,
            ),
            true,
        )
        .unwrap();
    controller.platform.fail_next_begin = true;
    assert_eq!(
        controller
            .release_all_policy(ControlOwner::MemoryPriority)
            .failures
            .len(),
        1
    );
    assert_eq!(controller.platform.processes[&7].value, 3);
    let result = controller.retry_pending_releases(Instant::now() + Duration::from_secs(2));
    assert!(result.failures.is_empty());
    assert_eq!(controller.platform.processes[&7].value, 2);
    assert_eq!(
        controller.managed[&identity(7, 1)].owner,
        ControlOwner::AdaptiveEngine
    );
    assert!(controller.release_retry_delay(Instant::now()).is_none());
    controller.retry_pending_releases(Instant::now() + Duration::from_secs(4));
    assert_eq!(controller.platform.processes[&7].value, 2);
    controller.release_all_policy(ControlOwner::AdaptiveEngine);
    assert_eq!(controller.platform.processes[&7].value, 5);
}

fn action_target(id: u32, creation_time: u64) -> ProcessActionTarget {
    let target = identity(id, creation_time);
    ProcessActionTarget {
        id: target.id,
        name: target.name,
        executable_path: target.executable_path,
        creation_time: target.creation_time,
        session_id: Some(1),
        is_service_account: Some(false),
    }
}

#[derive(Clone)]
struct FakeProcess {
    identity: ProcessIdentity,
    value: u32,
}

#[derive(Default)]
struct FakePlatform {
    exit_on_begin: bool,
    deny_query: bool,
    exit_on_apply: bool,
    processes: BTreeMap<u32, FakeProcess>,
    applied: Vec<(u32, u32)>,
    relinquished: Vec<(u32, u64)>,
    fail_next_begin: bool,
    fail_next_apply: bool,
    fail_next_commit: bool,
    fail_relinquish: bool,
    fail_query_after: Option<usize>,
    recovery_events: Arc<Mutex<Vec<String>>>,
}

struct FakeIntent {
    fail: bool,
    events: Arc<Mutex<Vec<String>>>,
}

impl MemoryPriorityRecoveryIntent for FakeIntent {
    fn commit(self) -> Result<(), String> {
        self.events.lock().unwrap().push("commit".to_owned());
        if self.fail {
            Err("injected commit failure".to_owned())
        } else {
            Ok(())
        }
    }

    fn retain_pending(self) -> Result<(), String> {
        self.events
            .lock()
            .unwrap()
            .push("retain_pending".to_owned());
        Ok(())
    }
}

impl MemoryPriorityPlatform for FakePlatform {
    type Process = u32;
    type RecoveryIntent = FakeIntent;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        _allow_cross_session_process_control: bool,
    ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError> {
        let process = self
            .processes
            .get(&target.id)
            .ok_or(ProcessControlError::ProcessExited)?;
        if process.identity.key() != target.key() {
            return Err(ProcessControlError::ProcessExited);
        }
        Ok((process.identity.clone(), target.id))
    }

    fn query(&mut self, process: &Self::Process) -> Result<u32, ProcessControlError> {
        if let Some(remaining) = self.fail_query_after.as_mut() {
            if *remaining == 0 {
                return Err(ProcessControlError::Failed("query failed".to_owned()));
            }
            *remaining -= 1;
        }
        if self.deny_query {
            return Err(ProcessControlError::AccessDenied("query denied".into()));
        }
        self.processes
            .get(process)
            .map(|process| process.value)
            .ok_or(ProcessControlError::ProcessExited)
    }

    fn begin_change(
        &mut self,
        _process: &Self::Process,
        _original: u32,
        _expected: u32,
    ) -> Result<Self::RecoveryIntent, ProcessControlError> {
        if std::mem::take(&mut self.exit_on_begin) {
            self.deny_query = true;
            return Err(ProcessControlError::ProcessExited);
        }
        if std::mem::take(&mut self.fail_next_begin) {
            return Err(ProcessControlError::Failed(
                "injected begin failure".to_owned(),
            ));
        }
        Ok(FakeIntent {
            fail: std::mem::take(&mut self.fail_next_commit),
            events: Arc::clone(&self.recovery_events),
        })
    }

    fn apply(&mut self, process: &Self::Process, priority: u32) -> Result<(), ProcessControlError> {
        if std::mem::take(&mut self.exit_on_apply) {
            self.processes.remove(process);
            return Err(ProcessControlError::ProcessExited);
        }

        if std::mem::take(&mut self.fail_next_apply) {
            return Err(ProcessControlError::Failed(
                "injected apply failure".to_owned(),
            ));
        }
        let state = self
            .processes
            .get_mut(process)
            .ok_or(ProcessControlError::ProcessExited)?;
        state.value = priority;
        self.applied.push((*process, priority));
        Ok(())
    }

    fn relinquish(&mut self, identity: &ProcessIdentity) -> Result<(), ProcessControlError> {
        if self.fail_relinquish {
            return Err(ProcessControlError::Failed(
                "injected relinquish failure".to_owned(),
            ));
        }
        self.relinquished
            .push((identity.id, identity.creation_time));
        Ok(())
    }
}

fn identity(id: u32, creation_time: u64) -> ProcessIdentity {
    ProcessIdentity::new(
        id,
        format!("app{id}.exe"),
        PathBuf::from(format!(r"C:\Apps\app{id}.exe")),
        creation_time,
        Some(1),
    )
}

fn target(id: u32, creation_time: u64) -> ProcessControlTarget {
    identity(id, creation_time).target()
}

fn platform_with(id: u32, creation_time: u64, value: u32) -> FakePlatform {
    let mut platform = FakePlatform::default();
    platform.processes.insert(
        id,
        FakeProcess {
            identity: identity(id, creation_time),
            value,
        },
    );
    platform
}

fn claim(
    id: u32,
    creation_time: u64,
    owner: ControlOwner,
    priority: ProcessMemoryPriority,
) -> MemoryPriorityClaim {
    MemoryPriorityClaim {
        target: target(id, creation_time),
        owner,
        priority,
        preservation: MemoryPriorityPreservation::Exact,
    }
}

#[test]
fn known_raw_memory_priorities_round_trip_and_unknown_values_do_not_collapse() {
    for priority in ProcessMemoryPriority::ALL {
        assert_eq!(
            memory_priority_from_raw(memory_priority_raw(priority)),
            Some(priority)
        );
    }
    assert_eq!(memory_priority_from_raw(99), None);
}

#[test]
fn static_policy_overrides_adaptive_then_reveals_it_without_baseline_bounce() {
    let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
    controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::AdaptiveEngine,
                ProcessMemoryPriority::Low,
            ),
            true,
        )
        .unwrap();
    controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::MemoryPriority,
                ProcessMemoryPriority::Medium,
            ),
            true,
        )
        .unwrap();

    let summary = controller.release_all_policy(ControlOwner::MemoryPriority);
    assert!(summary.failures.is_empty());
    assert_eq!(controller.platform.processes[&7].value, 2);
    assert_eq!(controller.platform.applied, vec![(7, 2), (7, 3), (7, 2)]);

    controller.release_all_policy(ControlOwner::AdaptiveEngine);
    assert_eq!(controller.platform.processes[&7].value, 5);
}

#[test]
fn non_overlapping_static_and_adaptive_claims_remain_independently_effective() {
    let mut platform = platform_with(7, 1, 5);
    platform.processes.insert(
        8,
        FakeProcess {
            identity: identity(8, 1),
            value: 5,
        },
    );
    let mut controller = MemoryPriorityController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::AdaptiveEngine,
                ProcessMemoryPriority::Low,
            ),
            true,
        )
        .unwrap();
    controller
        .apply_policy_claim(
            claim(
                8,
                1,
                ControlOwner::MemoryPriority,
                ProcessMemoryPriority::Medium,
            ),
            true,
        )
        .unwrap();

    controller.release_all_policy(ControlOwner::MemoryPriority);
    assert_eq!(controller.platform.processes[&7].value, 2);
    assert_eq!(controller.platform.processes[&8].value, 5);
    assert_eq!(
        controller.policy_managed_process_count(ControlOwner::AdaptiveEngine),
        1
    );
}

#[test]
fn lower_precedence_claim_is_stored_without_touching_static_state() {
    let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
    controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::MemoryPriority,
                ProcessMemoryPriority::Medium,
            ),
            true,
        )
        .unwrap();
    let outcome = controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::AdaptiveEngine,
                ProcessMemoryPriority::VeryLow,
            ),
            true,
        )
        .unwrap();

    assert_eq!(outcome, MemoryPriorityApplyOutcome::Shadowed);
    assert_eq!(controller.platform.processes[&7].value, 3);
    assert_eq!(controller.platform.applied, vec![(7, 3)]);

    let summary = controller.release_all_policy(ControlOwner::AdaptiveEngine);
    assert_eq!(summary.restored_processes, 0);
    assert_eq!(controller.platform.processes[&7].value, 3);
    assert_eq!(controller.platform.applied, vec![(7, 3)]);
}

#[test]
fn failed_owner_replacement_is_retried_after_the_removed_claim_is_gone() {
    let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
    controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::AdaptiveEngine,
                ProcessMemoryPriority::Low,
            ),
            true,
        )
        .unwrap();
    controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::MemoryPriority,
                ProcessMemoryPriority::Medium,
            ),
            true,
        )
        .unwrap();
    controller.platform.fail_next_apply = true;

    let first = controller.release_all_policy(ControlOwner::MemoryPriority);
    assert_eq!(first.failures.len(), 1);
    assert_eq!(controller.platform.processes[&7].value, 3);

    let second = controller.release_all_policy(ControlOwner::MemoryPriority);
    assert!(second.failures.is_empty());
    assert_eq!(controller.platform.processes[&7].value, 2);
}

#[test]
fn process_list_action_is_immediate_then_automatic_policy_supersedes_it() {
    let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
    let automatic = claim(
        7,
        1,
        ControlOwner::AdaptiveEngine,
        ProcessMemoryPriority::Low,
    );
    controller
        .apply_policy_claim(automatic.clone(), true)
        .unwrap();
    let action_target = ProcessActionTarget {
        id: 7,
        name: "app7.exe".to_owned(),
        executable_path: PathBuf::from(r"C:\Apps\app7.exe"),
        creation_time: 1,
        session_id: Some(1),
        is_service_account: Some(false),
    };
    controller
        .apply_process_list_action(&action_target, ProcessMemoryPriority::Medium, true)
        .unwrap();
    assert_eq!(controller.platform.processes[&7].value, 3);

    controller.apply_policy_claim(automatic, true).unwrap();
    assert_eq!(controller.platform.processes[&7].value, 2);
    controller.shutdown().unwrap();
    assert_eq!(controller.platform.processes[&7].value, 5);
}

#[test]
fn preservation_uses_the_original_baseline() {
    let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
    let mut lower = claim(
        7,
        1,
        ControlOwner::AdaptiveEngine,
        ProcessMemoryPriority::Low,
    );
    lower.preservation = MemoryPriorityPreservation::PreserveHigher;
    assert_eq!(
        controller.apply_policy_claim(lower, true).unwrap(),
        MemoryPriorityApplyOutcome::Preserved
    );
    assert!(!controller.has_managed_state());
    assert!(controller.platform.applied.is_empty());
}

#[test]
fn external_state_break_is_relinquished_without_overwrite_on_release() {
    let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
    controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::AdaptiveEngine,
                ProcessMemoryPriority::Low,
            ),
            true,
        )
        .unwrap();
    controller.platform.processes.get_mut(&7).unwrap().value = 4;
    controller.release_all_policy(ControlOwner::AdaptiveEngine);

    assert_eq!(controller.platform.processes[&7].value, 4);
    assert_eq!(controller.platform.applied, vec![(7, 2)]);
    assert_eq!(controller.platform.relinquished, vec![(7, 1)]);
    assert!(!controller.has_managed_state());
}

#[test]
fn pid_reuse_relinquishes_the_old_identity_before_applying_the_new_claim() {
    let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
    controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::AdaptiveEngine,
                ProcessMemoryPriority::Low,
            ),
            true,
        )
        .unwrap();
    controller.platform.processes.insert(
        7,
        FakeProcess {
            identity: identity(7, 2),
            value: 5,
        },
    );
    controller
        .apply_policy_claim(
            claim(
                7,
                2,
                ControlOwner::MemoryPriority,
                ProcessMemoryPriority::Medium,
            ),
            true,
        )
        .unwrap();

    assert_eq!(controller.platform.relinquished, vec![(7, 1)]);
    assert_eq!(controller.platform.processes[&7].value, 3);
}

#[test]
fn begin_failure_leaves_the_process_and_controller_unmanaged() {
    let mut platform = platform_with(7, 1, 5);
    platform.fail_next_begin = true;
    let mut controller = MemoryPriorityController::with_platform(platform);
    assert!(controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::MemoryPriority,
                ProcessMemoryPriority::Low,
            ),
            true,
        )
        .is_err());
    assert_eq!(controller.platform.processes[&7].value, 5);
    assert!(!controller.has_managed_state());
}

#[test]
fn apply_failure_compensates_to_the_original_value() {
    let mut platform = platform_with(7, 1, 5);
    platform.fail_next_apply = true;
    let mut controller = MemoryPriorityController::with_platform(platform);
    assert!(controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::MemoryPriority,
                ProcessMemoryPriority::Low,
            ),
            true,
        )
        .is_err());
    assert_eq!(controller.platform.processes[&7].value, 5);
    assert!(!controller.has_managed_state());
}

#[test]
fn commit_failure_retains_verified_target_and_the_first_baseline() {
    for replacing in [false, true] {
        let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
        if replacing {
            controller
                .apply_policy_claim(
                    claim(
                        7,
                        1,
                        ControlOwner::MemoryPriority,
                        ProcessMemoryPriority::Low,
                    ),
                    true,
                )
                .unwrap();
        }
        controller.platform.fail_next_commit = true;
        let error = controller
            .apply_policy_claim(
                claim(
                    7,
                    1,
                    ControlOwner::MemoryPriority,
                    ProcessMemoryPriority::Medium,
                ),
                true,
            )
            .unwrap_err();
        assert!(error.to_string().contains("commit failed"));
        assert_eq!(controller.platform.processes[&7].value, 3);
        let managed = controller.managed.values().next().unwrap();
        assert_eq!((managed.baseline, managed.expected), (5, 3));
        assert_eq!(
            controller.platform.applied.len(),
            if replacing { 2 } else { 1 }
        );
        assert!(controller.platform.relinquished.is_empty());
        controller.shutdown().unwrap();
        assert_eq!(controller.platform.processes[&7].value, 5);
        assert!(!controller.has_managed_state());
    }
}

#[test]
fn unverified_compensation_retains_a_pending_edge_to_the_first_baseline() {
    let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
    controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::MemoryPriority,
                ProcessMemoryPriority::Low,
            ),
            true,
        )
        .unwrap();
    controller.platform.recovery_events.lock().unwrap().clear();
    controller.platform.fail_query_after = Some(1);
    assert!(controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::MemoryPriority,
                ProcessMemoryPriority::Medium
            ),
            true,
        )
        .is_err());
    assert_eq!(controller.platform.processes[&7].value, 2);
    let managed = controller.managed.values().next().unwrap();
    assert_eq!((managed.baseline, managed.expected), (5, 3));
    assert_eq!(
        *controller.platform.recovery_events.lock().unwrap(),
        ["retain_pending"]
    );
    assert!(controller.platform.relinquished.is_empty());
}

#[test]
fn shutdown_keeps_confirmed_exit_when_followup_query_is_denied() {
    let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
    controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::MemoryPriority,
                ProcessMemoryPriority::Low,
            ),
            true,
        )
        .unwrap();
    controller.platform.exit_on_begin = true;
    assert!(controller.shutdown().is_ok());
    assert!(!controller.has_managed_state());
}

#[test]
fn shutdown_cleans_up_exit_during_restoration() {
    let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
    controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::MemoryPriority,
                ProcessMemoryPriority::Low,
            ),
            true,
        )
        .unwrap();
    controller.platform.exit_on_apply = true;
    assert!(controller.shutdown().is_ok());
    assert!(!controller.has_managed_state());
}

#[test]
fn release_commit_failure_keeps_the_verified_baseline_and_relinquishes() {
    let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
    controller
        .apply_policy_claim(
            claim(
                7,
                1,
                ControlOwner::MemoryPriority,
                ProcessMemoryPriority::Low,
            ),
            true,
        )
        .unwrap();
    controller.platform.fail_next_commit = true;
    let summary = controller.release_all_policy(ControlOwner::MemoryPriority);

    assert!(summary.failures.is_empty());
    assert_eq!(controller.platform.processes[&7].value, 5);
    assert_eq!(controller.platform.relinquished, vec![(7, 1)]);
    assert!(!controller.has_managed_state());
}

#[test]
fn shutdown_restores_in_reverse_successful_application_order() {
    let mut platform = platform_with(7, 1, 5);
    platform.processes.insert(
        8,
        FakeProcess {
            identity: identity(8, 1),
            value: 5,
        },
    );
    let mut controller = MemoryPriorityController::with_platform(platform);
    for id in [7, 8] {
        controller
            .apply_policy_claim(
                claim(
                    id,
                    1,
                    ControlOwner::MemoryPriority,
                    ProcessMemoryPriority::Low,
                ),
                true,
            )
            .unwrap();
    }
    controller.shutdown().unwrap();

    assert_eq!(
        controller.platform.applied,
        vec![(7, 2), (8, 2), (8, 5), (7, 5)]
    );
}

#[test]
#[ignore = "modifies and restores a disposable Windows process; run in explicit integration QA"]
fn memory_priority_live_apply_and_clean_release() -> Result<(), String> {
    struct DisposableProcess(Child);

    impl Drop for DisposableProcess {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let mut executable_path = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .ok_or_else(|| "SystemRoot is unavailable.".to_owned())?;
    executable_path.push(r"System32\PING.EXE");
    let child = Command::new(&executable_path)
        .args(["-t", "127.0.0.1"])
        .spawn()
        .map_err(|error| format!("Could not start disposable process: {error}"))?;
    let child = DisposableProcess(child);
    let deadline = Instant::now() + Duration::from_secs(2);
    let target = loop {
        match crate::foreground::capture_process_action_target(child.0.id(), &executable_path, true)
        {
            Ok(target) => break target,
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error.to_string()),
        }
    };
    let baseline =
        current_process_memory_priority(&target, true).map_err(|error| error.to_string())?;
    let expected = if baseline == ProcessMemoryPriority::VeryLow {
        ProcessMemoryPriority::Low
    } else {
        ProcessMemoryPriority::VeryLow
    };
    let mut controller = MemoryPriorityController::default();

    controller
        .apply_process_list_action(&target, expected, true)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        current_process_memory_priority(&target, true).map_err(|error| error.to_string())?,
        expected
    );
    controller.shutdown()?;
    assert_eq!(
        current_process_memory_priority(&target, true).map_err(|error| error.to_string())?,
        baseline
    );
    Ok(())
}
