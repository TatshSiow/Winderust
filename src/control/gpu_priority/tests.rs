use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use super::*;

#[test]
fn disabled_cleanup_survives_failed_replacement_and_respects_accepted_owner() {
    for replace in [false, true] {
        let mut controller = GpuPriorityController::with_platform(FakePlatform::new(2));
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::GpuPriority,
                    ProcessGpuPriority::Idle,
                    GpuPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller.platform.fail_next(FakeFailure::Open);
        assert_eq!(controller.release_all_policy().failures.len(), 1);
        for failure in [
            FakeFailure::Open,
            FakeFailure::QueryDenied,
            FakeFailure::Apply,
        ] {
            controller.platform.fail_next(failure);
            assert!(controller
                .apply_process_list_action(&action_target(), ProcessGpuPriority::BelowNormal, true)
                .is_err());
            assert_eq!(controller.pending_releases.len(), 1);
        }
        let due = controller.next_release_retry.unwrap();
        for _ in 0..8 {
            assert!(controller
                .release_policy_except(&BTreeSet::from([identity(42, 7).key()]))
                .failures
                .is_empty());
            assert_eq!(controller.next_release_retry, Some(due));
        }
        let now = due - Duration::from_millis(1);
        assert_eq!(controller.retry_pending_releases(now).restored_processes, 0);
        controller.platform.fail_next(FakeFailure::Open);
        assert_eq!(controller.retry_pending_releases(due).failures.len(), 1);
        assert_eq!(
            controller.release_retry_delay(due),
            Some(Duration::from_secs(1))
        );
        assert!(controller
            .retry_pending_releases(due + Duration::from_millis(999))
            .failures
            .is_empty());
        if replace {
            controller
                .apply_process_list_action(&action_target(), ProcessGpuPriority::BelowNormal, true)
                .unwrap();
        }
        assert_eq!(
            controller
                .retry_pending_releases(due + Duration::from_secs(1))
                .restored_processes,
            usize::from(!replace)
        );
        assert_eq!(
            controller.platform.processes[&42].priority,
            if replace { 1 } else { 2 }
        );
        assert!(controller.release_retry_delay(now).is_none());
        controller.shutdown().unwrap();
    }
}

#[test]
fn accepted_replacement_cancels_only_its_exact_pending_cleanup() {
    let mut controller = GpuPriorityController::with_platform(FakePlatform::with_processes(&[
        (42, 7, 2),
        (43, 8, 2),
    ]));
    for id in [42, 43] {
        let mut request = claim(
            ControlOwner::GpuPriority,
            ProcessGpuPriority::Idle,
            GpuPriorityPreservation::Exact,
        );
        request.target = target(id, if id == 42 { 7 } else { 8 });
        controller.apply_policy_claim(request, true).unwrap();
    }
    controller.platform.fail_next(FakeFailure::Open);
    controller.platform.fail_next(FakeFailure::Open);
    assert_eq!(controller.release_all_policy().failures.len(), 2);
    controller.platform.fail_next(FakeFailure::Commit);
    assert!(controller
        .apply_process_list_action(&action_target(), ProcessGpuPriority::BelowNormal, true)
        .is_err());
    assert_eq!(controller.pending_releases.len(), 2);
    controller
        .apply_process_list_action(&action_target(), ProcessGpuPriority::BelowNormal, true)
        .unwrap();
    assert_eq!(
        controller.pending_releases,
        BTreeSet::from([identity(43, 8)])
    );
    let result = controller.retry_pending_releases(Instant::now() + Duration::from_secs(2));
    assert_eq!(result.restored_processes, 1);
    assert_eq!(controller.platform.processes[&42].priority, 1);
    assert_eq!(controller.platform.processes[&43].priority, 2);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeFailure {
    BeginExited,
    QueryDenied,
    ApplyExited,
    Open,
    Unavailable,
    Verify,
    Begin,
    Apply,
    Commit,
    Relinquish,
}

struct FakeRecoveryIntent {
    fail: bool,
    events: Arc<Mutex<Vec<String>>>,
}

impl GpuPriorityRecoveryIntent for FakeRecoveryIntent {
    fn commit(self) -> Result<(), String> {
        self.events.lock().unwrap().push("commit".to_owned());
        if self.fail {
            Err("commit failed".to_owned())
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

#[derive(Clone)]
struct FakeProcess {
    identity: ProcessIdentity,
    priority: u32,
}

struct FakePlatform {
    processes: BTreeMap<u32, FakeProcess>,
    failures: VecDeque<FakeFailure>,
    events: Arc<Mutex<Vec<String>>>,
}

impl FakePlatform {
    fn new(priority: u32) -> Self {
        Self::with_processes(&[(42, 7, priority)])
    }

    fn with_processes(processes: &[(u32, u64, u32)]) -> Self {
        Self {
            processes: processes
                .iter()
                .map(|(id, creation_time, priority)| {
                    (
                        *id,
                        FakeProcess {
                            identity: identity(*id, *creation_time),
                            priority: *priority,
                        },
                    )
                })
                .collect(),
            failures: VecDeque::new(),
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn fail_next(&mut self, failure: FakeFailure) {
        self.failures.push_back(failure);
    }

    fn take_failure(&mut self, failure: FakeFailure) -> bool {
        if self.failures.front() == Some(&failure) {
            self.failures.pop_front();
            true
        } else {
            false
        }
    }
}

impl GpuPriorityPlatform for FakePlatform {
    type Process = u32;
    type RecoveryIntent = FakeRecoveryIntent;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        _: bool,
    ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError> {
        self.events.lock().unwrap().push("open".to_owned());
        if self.take_failure(FakeFailure::Open) {
            return Err(ProcessControlError::AccessDenied("denied".to_owned()));
        }
        let process = self
            .processes
            .get(&target.id)
            .ok_or(ProcessControlError::ProcessExited)?;
        if target.creation_time != process.identity.creation_time {
            return Err(ProcessControlError::ProcessExited);
        }
        Ok((process.identity.clone(), target.id))
    }

    fn query(&mut self, process: &Self::Process) -> Result<u32, ProcessControlError> {
        if self.take_failure(FakeFailure::QueryDenied) {
            return Err(ProcessControlError::AccessDenied("query denied".into()));
        }
        let follows_apply = self
            .events
            .lock()
            .unwrap()
            .last()
            .is_some_and(|event| event.starts_with("apply:"));
        self.events.lock().unwrap().push("query".to_owned());
        if self.take_failure(FakeFailure::Unavailable) {
            return Err(ProcessControlError::Unavailable(
                "GPU context unavailable".to_owned(),
            ));
        }
        if follows_apply && self.take_failure(FakeFailure::Verify) {
            return Err(ProcessControlError::Failed("query failed".to_owned()));
        }
        self.processes
            .get(process)
            .map(|process| process.priority)
            .ok_or(ProcessControlError::ProcessExited)
    }

    fn begin_change(
        &mut self,
        _: &Self::Process,
        _: u32,
        _: u32,
    ) -> Result<Self::RecoveryIntent, ProcessControlError> {
        self.events.lock().unwrap().push("begin".to_owned());
        if self.take_failure(FakeFailure::BeginExited) {
            return Err(ProcessControlError::ProcessExited);
        }
        if self.take_failure(FakeFailure::Begin) {
            return Err(ProcessControlError::Failed("begin failed".to_owned()));
        }
        let fail = self.take_failure(FakeFailure::Commit);
        Ok(FakeRecoveryIntent {
            fail,
            events: Arc::clone(&self.events),
        })
    }

    fn apply(&mut self, process: &Self::Process, priority: u32) -> Result<(), ProcessControlError> {
        self.events
            .lock()
            .unwrap()
            .push(format!("apply:{process}:{priority}"));
        if self.take_failure(FakeFailure::ApplyExited) {
            self.processes.remove(process);
            return Err(ProcessControlError::ProcessExited);
        }
        if self.take_failure(FakeFailure::Apply) {
            return Err(ProcessControlError::Failed("apply failed".to_owned()));
        }
        self.processes
            .get_mut(process)
            .ok_or(ProcessControlError::ProcessExited)?
            .priority = priority;
        Ok(())
    }

    fn relinquish(&mut self, identity: &ProcessIdentity) -> Result<(), ProcessControlError> {
        self.events.lock().unwrap().push(format!(
            "relinquish:{}:{}",
            identity.id, identity.creation_time
        ));
        if self.take_failure(FakeFailure::Relinquish) {
            Err(ProcessControlError::Failed("relinquish failed".to_owned()))
        } else {
            Ok(())
        }
    }
}

fn identity(id: u32, creation_time: u64) -> ProcessIdentity {
    ProcessIdentity::new(
        id,
        format!("app-{id}.exe"),
        PathBuf::from(format!(r"C:\Apps\app-{id}.exe")),
        creation_time,
        Some(1),
    )
}

fn target(id: u32, creation_time: u64) -> ProcessControlTarget {
    identity(id, creation_time).target()
}

fn claim(
    owner: ControlOwner,
    priority: ProcessGpuPriority,
    preservation: GpuPriorityPreservation,
) -> GpuPriorityClaim {
    GpuPriorityClaim {
        target: target(42, 7),
        owner,
        priority,
        preservation,
    }
}

fn action_target() -> ProcessActionTarget {
    let target = target(42, 7);
    ProcessActionTarget {
        id: target.id,
        name: target.name,
        executable_path: target.executable_path,
        creation_time: target.creation_time,
        session_id: Some(1),
        is_service_account: Some(false),
    }
}

#[test]
fn raw_values_match_windows_priority_hint_order_and_unknown_values_fail_closed() {
    assert_eq!(gpu_priority_raw(ProcessGpuPriority::Idle), 0);
    assert_eq!(gpu_priority_raw(ProcessGpuPriority::BelowNormal), 1);
    assert_eq!(gpu_priority_raw(ProcessGpuPriority::Normal), 2);
    assert_eq!(gpu_priority_raw(ProcessGpuPriority::AboveNormal), 3);
    assert_eq!(gpu_priority_raw(ProcessGpuPriority::High), 4);
    assert_eq!(gpu_priority_raw(ProcessGpuPriority::Realtime), 5);
    assert_eq!(gpu_priority_from_raw(0), Some(ProcessGpuPriority::Idle));
    assert_eq!(
        gpu_priority_from_raw(1),
        Some(ProcessGpuPriority::BelowNormal)
    );
    assert_eq!(gpu_priority_from_raw(2), Some(ProcessGpuPriority::Normal));
    assert_eq!(
        gpu_priority_from_raw(3),
        Some(ProcessGpuPriority::AboveNormal)
    );
    assert_eq!(gpu_priority_from_raw(4), Some(ProcessGpuPriority::High));
    assert_eq!(gpu_priority_from_raw(5), Some(ProcessGpuPriority::Realtime));
    assert_eq!(gpu_priority_from_raw(6), None);
}

#[test]
fn gpu_status_classifies_exit_context_unavailable_and_other_failures() {
    assert_eq!(
        gpu_error(GpuPriorityError::ProcessExited),
        ProcessControlError::ProcessExited
    );
    assert!(matches!(
        gpu_error(GpuPriorityError::ContextUnavailable),
        ProcessControlError::Unavailable(_)
    ));
    assert!(matches!(
        gpu_error(GpuPriorityError::NtStatus(0xC000_0001)),
        ProcessControlError::Failed(message) if message.contains("0xC0000001")
    ));
}

#[test]
fn unmanaged_matching_state_is_not_adopted() {
    let platform = FakePlatform::new(2);
    let mut controller = GpuPriorityController::with_platform(platform);

    assert_eq!(
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::GpuPriority,
                    ProcessGpuPriority::Normal,
                    GpuPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap(),
        GpuPriorityApplyOutcome::Unchanged
    );
    assert!(!controller.has_managed_state());
}

#[test]
fn unavailable_gpu_context_is_typed_and_does_not_create_managed_state() {
    let mut platform = FakePlatform::new(2);
    platform.fail_next(FakeFailure::Unavailable);
    let mut controller = GpuPriorityController::with_platform(platform);

    assert!(matches!(
        controller.apply_policy_claim(
            claim(
                ControlOwner::GpuPriority,
                ProcessGpuPriority::BelowNormal,
                GpuPriorityPreservation::Exact,
            ),
            true,
        ),
        Err(ProcessControlError::Unavailable(message))
            if message == "GPU context unavailable"
    ));
    assert!(!controller.has_managed_state());
}

#[test]
fn process_list_action_is_superseded_without_losing_the_first_baseline() {
    let platform = FakePlatform::new(2);
    let mut controller = GpuPriorityController::with_platform(platform);
    controller
        .apply_process_list_action(&action_target(), ProcessGpuPriority::BelowNormal, true)
        .unwrap();
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::GpuPriority,
                ProcessGpuPriority::Idle,
                GpuPriorityPreservation::Exact,
            ),
            true,
        )
        .unwrap();

    let managed = controller.managed.values().next().unwrap();
    assert_eq!(managed.baseline, 2);
    assert_eq!(managed.expected, 0);
    assert_eq!(managed.owner, ControlOwner::GpuPriority);
}

#[test]
fn static_to_adaptive_replacement_keeps_the_first_baseline() {
    let platform = FakePlatform::new(2);
    let mut controller = GpuPriorityController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::GpuPriority,
                ProcessGpuPriority::BelowNormal,
                GpuPriorityPreservation::Exact,
            ),
            true,
        )
        .unwrap();
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::AdaptiveEngine,
                ProcessGpuPriority::Idle,
                GpuPriorityPreservation::Exact,
            ),
            true,
        )
        .unwrap();

    let managed = controller.managed.values().next().unwrap();
    assert_eq!(managed.baseline, 2);
    assert_eq!(managed.owner, ControlOwner::AdaptiveEngine);
}

#[test]
fn identity_metadata_drift_preserves_the_existing_baseline() {
    let platform = FakePlatform::new(2);
    let mut controller = GpuPriorityController::with_platform(platform);
    let claim = claim(
        ControlOwner::GpuPriority,
        ProcessGpuPriority::BelowNormal,
        GpuPriorityPreservation::Exact,
    );
    controller.apply_policy_claim(claim.clone(), true).unwrap();
    let process = controller.platform.processes.get_mut(&42).unwrap();
    process.identity = ProcessIdentity::new(
        42,
        "APP-42.EXE".to_owned(),
        PathBuf::from(r"c:/apps/APP-42.exe"),
        7,
        None,
    );
    controller.apply_policy_claim(claim, true).unwrap();

    assert_eq!(controller.managed.len(), 1);
    assert_eq!(controller.managed.values().next().unwrap().baseline, 2);
}

#[test]
fn unknown_windows_baseline_is_restored_exactly() {
    let platform = FakePlatform::new(9);
    let mut controller = GpuPriorityController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::GpuPriority,
                ProcessGpuPriority::BelowNormal,
                GpuPriorityPreservation::Exact,
            ),
            true,
        )
        .unwrap();

    controller.shutdown().unwrap();
    assert_eq!(controller.platform.processes[&42].priority, 9);
}

#[test]
fn preservation_uses_the_original_baseline_and_releases_managed_state() {
    let platform = FakePlatform::new(2);
    let mut controller = GpuPriorityController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::GpuPriority,
                ProcessGpuPriority::BelowNormal,
                GpuPriorityPreservation::Exact,
            ),
            true,
        )
        .unwrap();
    assert_eq!(
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::AdaptiveEngine,
                    ProcessGpuPriority::BelowNormal,
                    GpuPriorityPreservation::PreserveHigher,
                ),
                true,
            )
            .unwrap(),
        GpuPriorityApplyOutcome::Preserved
    );
    assert!(!controller.has_managed_state());
    assert_eq!(controller.platform.processes[&42].priority, 2);

    controller.platform.processes.get_mut(&42).unwrap().priority = 0;
    assert_eq!(
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::GpuPriority,
                    ProcessGpuPriority::BelowNormal,
                    GpuPriorityPreservation::PreserveLower,
                ),
                true,
            )
            .unwrap(),
        GpuPriorityApplyOutcome::Preserved
    );
    assert!(!controller.has_managed_state());
}

#[test]
fn external_break_rebases_before_policy_is_reasserted() {
    let platform = FakePlatform::new(2);
    let mut controller = GpuPriorityController::with_platform(platform);
    let claim = claim(
        ControlOwner::GpuPriority,
        ProcessGpuPriority::Idle,
        GpuPriorityPreservation::Exact,
    );
    controller.apply_policy_claim(claim.clone(), true).unwrap();
    controller.platform.processes.get_mut(&42).unwrap().priority = 3;
    controller.apply_policy_claim(claim, true).unwrap();

    assert_eq!(controller.managed.values().next().unwrap().baseline, 3);
    assert!(controller
        .platform
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|event| event == "relinquish:42:7"));
}

#[test]
fn failed_external_break_relinquish_retains_the_managed_chain() {
    let platform = FakePlatform::new(2);
    let mut controller = GpuPriorityController::with_platform(platform);
    let claim = claim(
        ControlOwner::GpuPriority,
        ProcessGpuPriority::BelowNormal,
        GpuPriorityPreservation::Exact,
    );
    controller.apply_policy_claim(claim.clone(), true).unwrap();
    controller.platform.processes.get_mut(&42).unwrap().priority = 3;
    controller.platform.fail_next(FakeFailure::Relinquish);

    assert!(controller.apply_policy_claim(claim, true).is_err());
    assert!(controller.has_managed_state());
}

#[test]
fn external_break_is_not_overwritten_during_release() {
    let platform = FakePlatform::new(2);
    let mut controller = GpuPriorityController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::GpuPriority,
                ProcessGpuPriority::BelowNormal,
                GpuPriorityPreservation::Exact,
            ),
            true,
        )
        .unwrap();
    controller.platform.processes.get_mut(&42).unwrap().priority = 3;

    let summary = controller.release_all_policy();
    assert!(summary.failures.is_empty());
    assert_eq!(summary.restored_processes, 0);
    assert_eq!(controller.platform.processes[&42].priority, 3);
}

#[test]
fn process_exit_and_pid_reuse_relinquish_the_old_identity() {
    let platform = FakePlatform::new(2);
    let mut controller = GpuPriorityController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::GpuPriority,
                ProcessGpuPriority::BelowNormal,
                GpuPriorityPreservation::Exact,
            ),
            true,
        )
        .unwrap();
    controller.platform.processes.insert(
        42,
        FakeProcess {
            identity: identity(42, 8),
            priority: 2,
        },
    );
    controller
        .apply_policy_claim(
            GpuPriorityClaim {
                target: target(42, 8),
                owner: ControlOwner::GpuPriority,
                priority: ProcessGpuPriority::Idle,
                preservation: GpuPriorityPreservation::Exact,
            },
            true,
        )
        .unwrap();

    assert_eq!(controller.managed.len(), 1);
    assert_eq!(controller.managed.keys().next().unwrap().creation_time, 8);
    assert!(controller
        .platform
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|event| event == "relinquish:42:7"));
}

#[test]
fn begin_apply_and_verification_failures_compensate() {
    for failure in [FakeFailure::Begin, FakeFailure::Apply, FakeFailure::Verify] {
        let mut platform = FakePlatform::new(2);
        platform.fail_next(failure);
        let mut controller = GpuPriorityController::with_platform(platform);
        assert!(controller
            .apply_policy_claim(
                claim(
                    ControlOwner::GpuPriority,
                    ProcessGpuPriority::BelowNormal,
                    GpuPriorityPreservation::Exact,
                ),
                true,
            )
            .is_err());
        assert_eq!(controller.platform.processes[&42].priority, 2);
        assert!(!controller.has_managed_state());
    }
}

#[test]
fn commit_failure_retains_verified_target_and_the_first_baseline() {
    for replacing in [false, true] {
        let mut controller = GpuPriorityController::with_platform(FakePlatform::new(2));
        if replacing {
            controller
                .apply_policy_claim(
                    claim(
                        ControlOwner::GpuPriority,
                        ProcessGpuPriority::BelowNormal,
                        GpuPriorityPreservation::Exact,
                    ),
                    true,
                )
                .unwrap();
        }
        controller.platform.fail_next(FakeFailure::Commit);
        let error = controller
            .apply_policy_claim(
                claim(
                    ControlOwner::GpuPriority,
                    ProcessGpuPriority::AboveNormal,
                    GpuPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap_err();
        assert!(error.to_string().contains("commit failed"));
        assert_eq!(controller.platform.processes[&42].priority, 3);
        let managed = controller.managed.values().next().unwrap();
        assert_eq!((managed.baseline, managed.expected), (2, 3));
        let events = controller.platform.events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.starts_with("apply:"))
                .count(),
            if replacing { 2 } else { 1 }
        );
        assert!(!events.iter().any(|event| event.starts_with("relinquish:")));
        drop(events);
        controller.shutdown().unwrap();
        assert_eq!(controller.platform.processes[&42].priority, 2);
        assert!(!controller.has_managed_state());
    }
}

#[test]
fn unverified_compensation_retains_a_pending_edge_to_the_first_baseline() {
    let mut controller = GpuPriorityController::with_platform(FakePlatform::new(2));
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::GpuPriority,
                ProcessGpuPriority::BelowNormal,
                GpuPriorityPreservation::Exact,
            ),
            true,
        )
        .unwrap();
    controller.platform.events.lock().unwrap().clear();
    controller.platform.fail_next(FakeFailure::Verify);
    controller.platform.fail_next(FakeFailure::Verify);
    assert!(controller
        .apply_policy_claim(
            claim(
                ControlOwner::GpuPriority,
                ProcessGpuPriority::AboveNormal,
                GpuPriorityPreservation::Exact
            ),
            true,
        )
        .is_err());
    assert_eq!(controller.platform.processes[&42].priority, 1);
    let managed = controller.managed.values().next().unwrap();
    assert_eq!((managed.baseline, managed.expected), (2, 3));
    let events = controller.platform.events.lock().unwrap();
    assert!(events.iter().any(|event| event == "retain_pending"));
    assert!(!events
        .iter()
        .any(|event| event == "commit" || event.starts_with("relinquish:")));
}

#[test]
fn failed_compensation_keeps_uncertain_state_for_recovery() {
    let mut platform = FakePlatform::new(2);
    platform.fail_next(FakeFailure::Apply);
    platform.fail_next(FakeFailure::Apply);
    let mut controller = GpuPriorityController::with_platform(platform);

    assert!(controller
        .apply_policy_claim(
            claim(
                ControlOwner::GpuPriority,
                ProcessGpuPriority::BelowNormal,
                GpuPriorityPreservation::Exact,
            ),
            true,
        )
        .is_err());
    assert!(controller.has_managed_state());
}

#[test]
fn shutdown_keeps_confirmed_exit_when_followup_query_is_denied() {
    let platform = FakePlatform::new(2);
    let mut controller = GpuPriorityController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::GpuPriority,
                ProcessGpuPriority::BelowNormal,
                GpuPriorityPreservation::Exact,
            ),
            true,
        )
        .unwrap();
    controller.platform.fail_next(FakeFailure::BeginExited);
    controller.platform.fail_next(FakeFailure::QueryDenied);
    assert!(controller.shutdown().is_ok());
    assert!(!controller.has_managed_state());
}

#[test]
fn shutdown_cleans_up_exit_during_restoration() {
    let platform = FakePlatform::new(2);
    let mut controller = GpuPriorityController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::GpuPriority,
                ProcessGpuPriority::BelowNormal,
                GpuPriorityPreservation::Exact,
            ),
            true,
        )
        .unwrap();
    controller.platform.fail_next(FakeFailure::ApplyExited);
    assert!(controller.shutdown().is_ok());
    assert!(!controller.has_managed_state());
}

#[test]
fn release_commit_failure_keeps_baseline_and_relinquishes_recovery() {
    let platform = FakePlatform::new(2);
    let mut controller = GpuPriorityController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::GpuPriority,
                ProcessGpuPriority::BelowNormal,
                GpuPriorityPreservation::Exact,
            ),
            true,
        )
        .unwrap();
    controller.platform.fail_next(FakeFailure::Commit);

    let summary = controller.release_all_policy();
    assert!(summary.failures.is_empty());
    assert_eq!(summary.restored_processes, 1);
    assert_eq!(controller.platform.processes[&42].priority, 2);
    assert!(!controller.has_managed_state());
}

#[test]
fn failed_release_relinquish_retries_without_reapplying() {
    let platform = FakePlatform::new(2);
    let mut controller = GpuPriorityController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::GpuPriority,
                ProcessGpuPriority::BelowNormal,
                GpuPriorityPreservation::Exact,
            ),
            true,
        )
        .unwrap();
    controller.platform.fail_next(FakeFailure::Commit);
    controller.platform.fail_next(FakeFailure::Relinquish);
    let first = controller.release_all_policy();
    assert_eq!(first.failures.len(), 1);
    assert_eq!(controller.platform.processes[&42].priority, 2);
    let apply_count = controller
        .platform
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|event| event.starts_with("apply:"))
        .count();

    let second = controller.release_all_policy();
    assert!(second.failures.is_empty());
    assert_eq!(
        controller
            .platform
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.starts_with("apply:"))
            .count(),
        apply_count
    );
}

#[test]
fn shutdown_restores_in_reverse_application_order() {
    let platform = FakePlatform::with_processes(&[(42, 7, 2), (43, 8, 2)]);
    let events = Arc::clone(&platform.events);
    let mut controller = GpuPriorityController::with_platform(platform);
    for (id, creation_time) in [(42, 7), (43, 8)] {
        controller
            .apply_policy_claim(
                GpuPriorityClaim {
                    target: target(id, creation_time),
                    owner: ControlOwner::GpuPriority,
                    priority: ProcessGpuPriority::BelowNormal,
                    preservation: GpuPriorityPreservation::Exact,
                },
                true,
            )
            .unwrap();
    }
    events.lock().unwrap().clear();
    controller.shutdown().unwrap();

    let restores = events
        .lock()
        .unwrap()
        .iter()
        .filter(|event| event.starts_with("apply:"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(restores, ["apply:43:2", "apply:42:2"]);
}

#[test]
#[ignore = "modifies and restores the explicit WINDERUST_GPU_TEST_PID target; run in integration QA"]
fn gpu_priority_live_apply_and_clean_release() -> Result<(), String> {
    let process_id = std::env::var("WINDERUST_GPU_TEST_PID")
        .map_err(|_| "Set WINDERUST_GPU_TEST_PID to a disposable GPU-using process.".to_owned())?
        .parse::<u32>()
        .map_err(|error| format!("WINDERUST_GPU_TEST_PID is invalid: {error}"))?;
    let executable_path = std::env::var_os("WINDERUST_GPU_TEST_PATH")
        .map(PathBuf::from)
        .ok_or_else(|| {
            "Set WINDERUST_GPU_TEST_PATH to that process's absolute executable path.".to_owned()
        })?;
    let target =
        crate::foreground::capture_process_action_target(process_id, &executable_path, true)
            .map_err(|error| error.to_string())?;
    let baseline = current_process_gpu_priority(&target, true)?;
    let expected = if baseline == ProcessGpuPriority::Idle {
        ProcessGpuPriority::BelowNormal
    } else {
        ProcessGpuPriority::Idle
    };
    let mut controller = GpuPriorityController::default();

    controller
        .apply_process_list_action(&target, expected, true)
        .map_err(|error| error.to_string())?;
    assert_eq!(current_process_gpu_priority(&target, true)?, expected);
    controller.shutdown()?;
    assert_eq!(current_process_gpu_priority(&target, true)?, baseline);
    Ok(())
}
