use std::{
    collections::VecDeque,
    path::PathBuf,
    process::{Child, Command},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use super::*;

#[test]
fn disabled_cleanup_survives_failed_replacement_and_respects_accepted_owner() {
    for replace in [false, true] {
        let mut controller = DynamicPriorityBoostController::with_platform(FakePlatform::new(
            DynamicPriorityBoostState::Enabled,
        ));
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
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
                .apply_process_list_action(
                    &action_target(),
                    DynamicPriorityBoostState::Enabled,
                    true
                )
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
                .apply_process_list_action(
                    &action_target(),
                    DynamicPriorityBoostState::Enabled,
                    true,
                )
                .unwrap();
        }
        assert_eq!(
            controller
                .retry_pending_releases(due + Duration::from_secs(1))
                .restored_processes,
            usize::from(!replace)
        );
        assert_eq!(
            controller.platform.processes[&42].state,
            DynamicPriorityBoostState::Enabled
        );
        assert!(controller.release_retry_delay(now).is_none());
        controller.shutdown().unwrap();
    }
}

#[test]
fn unacknowledged_replacement_keeps_cleanup_pending() {
    let mut controller = DynamicPriorityBoostController::with_platform(FakePlatform::new(
        DynamicPriorityBoostState::Enabled,
    ));
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .unwrap();
    controller.platform.fail_next(FakeFailure::Open);
    assert_eq!(controller.release_all_policy().failures.len(), 1);
    controller.platform.fail_next(FakeFailure::Commit);
    assert!(controller
        .apply_process_list_action(&action_target(), DynamicPriorityBoostState::Enabled, true)
        .is_err());
    assert_eq!(controller.pending_releases.len(), 1);
    assert!(controller
        .retry_pending_releases(Instant::now() + Duration::from_secs(2))
        .failures
        .is_empty());
    assert!(!controller.has_managed_state());
    assert!(controller.release_retry_delay(Instant::now()).is_none());
}

fn action_target() -> ProcessActionTarget {
    let target = identity(42, 7);
    ProcessActionTarget {
        id: target.id,
        name: target.name,
        executable_path: target.executable_path,
        creation_time: target.creation_time,
        session_id: Some(1),
        is_service_account: Some(false),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeFailure {
    BeginExited,
    QueryDenied,
    ApplyExited,
    Open,
    VerificationQuery,
    Begin,
    Apply,
    Commit,
    Relinquish,
}

struct FakeRecoveryIntent {
    fail: bool,
    events: Arc<Mutex<Vec<String>>>,
}

impl DynamicPriorityBoostRecoveryIntent for FakeRecoveryIntent {
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
    state: DynamicPriorityBoostState,
}

struct FakePlatform {
    processes: BTreeMap<u32, FakeProcess>,
    failures: VecDeque<FakeFailure>,
    events: Arc<Mutex<Vec<String>>>,
}

impl FakePlatform {
    fn new(state: DynamicPriorityBoostState) -> Self {
        let identity = identity(42, 7);
        Self {
            processes: BTreeMap::from([(42, FakeProcess { identity, state })]),
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

impl DynamicPriorityBoostPlatform for FakePlatform {
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

    fn query(
        &mut self,
        process: &Self::Process,
    ) -> Result<DynamicPriorityBoostState, ProcessControlError> {
        if self.take_failure(FakeFailure::QueryDenied) {
            return Err(ProcessControlError::AccessDenied("query denied".into()));
        }
        self.events.lock().unwrap().push("query".to_owned());
        let state = self
            .processes
            .get(process)
            .map(|process| process.state)
            .ok_or(ProcessControlError::ProcessExited)?;
        if state == DynamicPriorityBoostState::Disabled
            && self.take_failure(FakeFailure::VerificationQuery)
        {
            return Err(ProcessControlError::Failed("query failed".to_owned()));
        }
        Ok(state)
    }

    fn begin_change(
        &mut self,
        _: &Self::Process,
        _: DynamicPriorityBoostState,
        _: DynamicPriorityBoostState,
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

    fn apply(
        &mut self,
        process: &Self::Process,
        state: DynamicPriorityBoostState,
    ) -> Result<(), ProcessControlError> {
        self.events.lock().unwrap().push(format!("apply:{state:?}"));
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
            .state = state;
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
        "app.exe".to_owned(),
        PathBuf::from(r"C:\Apps\app.exe"),
        creation_time,
        Some(1),
    )
}

fn claim(owner: ControlOwner, state: DynamicPriorityBoostState) -> DynamicPriorityBoostClaim {
    claim_at(owner, state, 7)
}

fn claim_at(
    owner: ControlOwner,
    state: DynamicPriorityBoostState,
    creation_time: u64,
) -> DynamicPriorityBoostClaim {
    DynamicPriorityBoostClaim {
        target: ProcessControlTarget::automatic(
            42,
            "app.exe".to_owned(),
            PathBuf::from(r"C:\Apps\app.exe"),
            creation_time,
        ),
        owner,
        state,
    }
}

#[test]
fn unmanaged_matching_state_is_not_adopted() {
    let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);

    assert_eq!(
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Enabled,
                ),
                true,
            )
            .unwrap(),
        DynamicPriorityBoostApplyOutcome::Unchanged
    );
    assert!(!controller.has_managed_state());
}

#[test]
fn manual_action_is_superseded_by_the_next_automatic_claim() {
    let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);
    let target = ProcessActionTarget {
        id: 42,
        name: "app.exe".to_owned(),
        executable_path: PathBuf::from(r"C:\Apps\app.exe"),
        creation_time: 7,
        session_id: Some(1),
        is_service_account: Some(false),
    };

    controller
        .apply_process_list_action(&target, DynamicPriorityBoostState::Disabled, true)
        .unwrap();
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Enabled,
            ),
            true,
        )
        .unwrap();

    let managed = controller.managed.values().next().unwrap();
    assert_eq!(managed.owner, ControlOwner::DynamicPriorityBoost);
    assert_eq!(managed.baseline, DynamicPriorityBoostState::Enabled);
    assert_eq!(
        controller.platform.processes[&42].state,
        DynamicPriorityBoostState::Enabled
    );
}

#[test]
fn static_to_adaptive_replacement_preserves_the_first_baseline() {
    let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .unwrap();
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::AdaptiveEngine,
                DynamicPriorityBoostState::Enabled,
            ),
            true,
        )
        .unwrap();

    let managed = controller.managed.values().next().unwrap();
    assert_eq!(managed.owner, ControlOwner::AdaptiveEngine);
    assert_eq!(managed.baseline, DynamicPriorityBoostState::Enabled);
    assert_eq!(managed.expected, DynamicPriorityBoostState::Enabled);
}

#[test]
fn external_break_rebases_before_automatic_policy_is_reasserted() {
    let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .unwrap();
    controller.platform.processes.get_mut(&42).unwrap().state = DynamicPriorityBoostState::Enabled;

    controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .unwrap();

    assert_eq!(
        controller.managed.values().next().unwrap().baseline,
        DynamicPriorityBoostState::Enabled
    );
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
    let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .unwrap();
    controller.platform.processes.get_mut(&42).unwrap().state = DynamicPriorityBoostState::Enabled;
    controller.platform.fail_next(FakeFailure::Relinquish);

    assert!(controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .is_err());
    assert!(controller.has_managed_state());
}

#[test]
fn identity_metadata_drift_preserves_the_existing_baseline() {
    let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .unwrap();
    let identity = &mut controller.platform.processes.get_mut(&42).unwrap().identity;
    identity.name = "APP.EXE".to_owned();
    identity.executable_path = PathBuf::from(r"c:/apps/APP.exe");
    identity.session_id = None;

    controller
        .apply_policy_claim(
            claim(
                ControlOwner::AdaptiveEngine,
                DynamicPriorityBoostState::Enabled,
            ),
            true,
        )
        .unwrap();

    let managed = controller.managed.values().next().unwrap();
    assert_eq!(managed.baseline, DynamicPriorityBoostState::Enabled);
    assert!(!controller
        .platform
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|event| event.starts_with("relinquish:")));
}

#[test]
fn external_break_is_not_overwritten_during_release() {
    let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .unwrap();
    controller.platform.processes.get_mut(&42).unwrap().state = DynamicPriorityBoostState::Enabled;

    assert!(controller.release_all_policy().failures.is_empty());
    assert!(!controller.has_managed_state());
    assert_eq!(
        controller.platform.processes[&42].state,
        DynamicPriorityBoostState::Enabled
    );
}

#[test]
fn process_exit_and_pid_reuse_drop_the_old_identity() {
    let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .unwrap();
    controller.platform.processes.get_mut(&42).unwrap().identity = identity(42, 8);

    controller
        .apply_policy_claim(
            claim_at(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Enabled,
                8,
            ),
            true,
        )
        .unwrap();

    assert_eq!(controller.managed.len(), 1);
    assert_eq!(controller.managed.keys().next().unwrap().creation_time, 8);
}

#[test]
fn begin_failure_blocks_the_mutation() {
    let mut platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    platform.fail_next(FakeFailure::Begin);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);

    assert!(controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .is_err());
    assert_eq!(
        controller.platform.processes[&42].state,
        DynamicPriorityBoostState::Enabled
    );
    assert!(!controller.has_managed_state());
}

#[test]
fn apply_failure_compensates_and_cancels_the_intent() {
    let mut platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    platform.fail_next(FakeFailure::Apply);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);

    assert!(controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .is_err());
    assert_eq!(
        controller.platform.processes[&42].state,
        DynamicPriorityBoostState::Enabled
    );
    assert!(!controller.has_managed_state());
}

#[test]
fn verification_failure_compensates() {
    let mut platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    platform.fail_next(FakeFailure::VerificationQuery);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);

    assert!(controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .is_err());
    assert_eq!(
        controller.platform.processes[&42].state,
        DynamicPriorityBoostState::Enabled
    );
    assert!(!controller.has_managed_state());
}

#[test]
fn failed_compensation_keeps_uncertain_state_for_later_recovery() {
    let mut platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    platform.fail_next(FakeFailure::Apply);
    platform.fail_next(FakeFailure::Apply);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);

    assert!(controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .is_err());
    assert!(controller.has_managed_state());
}

#[test]
fn commit_failure_retains_verified_target_and_the_first_baseline() {
    for replacing in [false, true] {
        let mut controller = DynamicPriorityBoostController::with_platform(FakePlatform::new(
            DynamicPriorityBoostState::Enabled,
        ));
        if replacing {
            controller
                .apply_policy_claim(
                    claim(
                        ControlOwner::DynamicPriorityBoost,
                        DynamicPriorityBoostState::Disabled,
                    ),
                    true,
                )
                .unwrap();
        }
        let desired = if replacing {
            DynamicPriorityBoostState::Enabled
        } else {
            DynamicPriorityBoostState::Disabled
        };
        controller.platform.fail_next(FakeFailure::Commit);
        let error = controller
            .apply_policy_claim(claim(ControlOwner::DynamicPriorityBoost, desired), true)
            .unwrap_err();
        assert!(error.to_string().contains("commit failed"));
        assert_eq!(controller.platform.processes[&42].state, desired);
        let managed = controller.managed.values().next().unwrap();
        assert_eq!(managed.baseline, DynamicPriorityBoostState::Enabled);
        assert_eq!(managed.expected, desired);
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
        assert_eq!(
            controller.platform.processes[&42].state,
            DynamicPriorityBoostState::Enabled
        );
        assert!(!controller.has_managed_state());
    }
}

#[test]
fn unverified_compensation_retains_a_pending_edge_to_the_first_baseline() {
    let mut controller = DynamicPriorityBoostController::with_platform(FakePlatform::new(
        DynamicPriorityBoostState::Enabled,
    ));
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .unwrap();
    controller.platform.events.lock().unwrap().clear();
    controller.platform.fail_next(FakeFailure::Apply);
    controller
        .platform
        .fail_next(FakeFailure::VerificationQuery);
    assert!(controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Enabled
            ),
            true,
        )
        .is_err());
    assert_eq!(
        controller.platform.processes[&42].state,
        DynamicPriorityBoostState::Disabled
    );
    let managed = controller.managed.values().next().unwrap();
    assert_eq!(managed.baseline, DynamicPriorityBoostState::Enabled);
    assert_eq!(managed.expected, DynamicPriorityBoostState::Enabled);
    let events = controller.platform.events.lock().unwrap();
    assert!(events.iter().any(|event| event == "retain_pending"));
    assert!(!events
        .iter()
        .any(|event| event == "commit" || event.starts_with("relinquish:")));
}

#[test]
fn shutdown_keeps_confirmed_exit_when_followup_query_is_denied() {
    let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
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
    let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
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
    let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .unwrap();
    controller.platform.fail_next(FakeFailure::Commit);

    let summary = controller.release_all_policy();

    assert!(summary.failures.is_empty());
    assert_eq!(summary.restored_processes, 1);
    assert!(!controller.has_managed_state());
    assert_eq!(
        controller.platform.processes[&42].state,
        DynamicPriorityBoostState::Enabled
    );
    assert!(controller
        .platform
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|event| event == "relinquish:42:7"));
}

#[test]
fn failed_release_relinquish_is_retried_without_reapplying() {
    let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .unwrap();
    controller.platform.fail_next(FakeFailure::Commit);
    controller.platform.fail_next(FakeFailure::Relinquish);

    let first = controller.release_all_policy();
    assert_eq!(first.failures.len(), 1);
    assert!(controller.has_managed_state());
    assert_eq!(
        controller.platform.processes[&42].state,
        DynamicPriorityBoostState::Enabled
    );

    let second = controller.release_all_policy();
    assert!(second.failures.is_empty());
    assert!(!controller.has_managed_state());
}

#[test]
fn shutdown_restores_all_managed_processes() {
    let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
    let mut controller = DynamicPriorityBoostController::with_platform(platform);
    controller
        .apply_policy_claim(
            claim(
                ControlOwner::DynamicPriorityBoost,
                DynamicPriorityBoostState::Disabled,
            ),
            true,
        )
        .unwrap();

    controller.shutdown().unwrap();

    assert!(!controller.has_managed_state());
    assert_eq!(
        controller.platform.processes[&42].state,
        DynamicPriorityBoostState::Enabled
    );
}

#[test]
#[ignore = "modifies and restores a disposable Windows process; run in explicit integration QA"]
fn dynamic_priority_boost_live_apply_and_clean_release() -> Result<(), String> {
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
    let baseline = current_dynamic_priority_boost_state(&target)?;
    let expected = if baseline == DynamicPriorityBoostState::Enabled {
        DynamicPriorityBoostState::Disabled
    } else {
        DynamicPriorityBoostState::Enabled
    };
    let mut controller = DynamicPriorityBoostController::default();

    controller
        .apply_process_list_action(&target, expected, true)
        .map_err(|error| error.to_string())?;
    assert_eq!(current_dynamic_priority_boost_state(&target)?, expected);
    controller.shutdown()?;
    assert_eq!(current_dynamic_priority_boost_state(&target)?, baseline);
    Ok(())
}
