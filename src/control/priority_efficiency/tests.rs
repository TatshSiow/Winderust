use std::{
    cell::Cell,
    collections::VecDeque,
    path::PathBuf,
    process::{Child, Command},
    rc::Rc,
    time::{Duration, Instant},
};

use super::*;

mod adaptive_power;

use crate::platform::windows::priority_efficiency::{
    POWER_CURRENT_VERSION as PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    POWER_EXECUTION_SPEED as PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    POWER_IGNORE_TIMER_RESOLUTION as PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
    PRIORITY_ABOVE_NORMAL as ABOVE_NORMAL_PRIORITY_CLASS,
    PRIORITY_BELOW_NORMAL as BELOW_NORMAL_PRIORITY_CLASS, PRIORITY_HIGH as HIGH_PRIORITY_CLASS,
    PRIORITY_IDLE as IDLE_PRIORITY_CLASS, PRIORITY_NORMAL as NORMAL_PRIORITY_CLASS,
    PRIORITY_REALTIME as REALTIME_PRIORITY_CLASS,
};

#[derive(Clone)]
struct FakeProcess {
    identity: ProcessIdentity,
    priority: u32,
    power: PowerThrottlingState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeEvent {
    Priority(u32, u32),
    Power(u32, PowerThrottlingState),
    RelinquishPriority(u32, u64),
    RelinquishPower(u32, u64),
}

#[derive(Default)]
struct FakePlatform {
    exit_on_apply: bool,
    deny_open: bool,
    processes: BTreeMap<u32, FakeProcess>,
    events: Vec<FakeEvent>,
    fail_next_priority_begin: bool,
    fail_next_power_begin: bool,
    fail_next_priority_apply: bool,
    fail_next_power_apply: bool,
    fail_next_priority_commit: bool,
    fail_next_power_commit: bool,
    fail_priority_relinquish: bool,
    fail_power_relinquish: bool,
    power_unavailable: bool,
    priority_query_failures: VecDeque<bool>,
    retained_intents: Rc<Cell<usize>>,
    power_commit_failures: VecDeque<bool>,
}

struct FakeIntent {
    fail: bool,
    retained: Rc<Cell<usize>>,
}

impl PriorityEfficiencyRecoveryIntent for FakeIntent {
    fn commit(self) -> Result<(), String> {
        if self.fail {
            Err("injected commit failure".to_owned())
        } else {
            Ok(())
        }
    }

    fn retain_pending(self) -> Result<(), String> {
        self.retained.set(self.retained.get() + 1);
        Ok(())
    }
}

impl PriorityEfficiencyPlatform for FakePlatform {
    type Process = u32;
    type RecoveryIntent = FakeIntent;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        _allow_cross_session_process_control: bool,
    ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError> {
        if self.deny_open {
            return Err(ProcessControlError::AccessDenied("open denied".into()));
        }
        let process = self
            .processes
            .get(&target.id)
            .ok_or(ProcessControlError::ProcessExited)?;
        if process.identity.key() != target.key() {
            return Err(ProcessControlError::ProcessExited);
        }
        Ok((process.identity.clone(), target.id))
    }

    fn query_priority(&mut self, process: &Self::Process) -> Result<u32, ProcessControlError> {
        if self.priority_query_failures.pop_front() == Some(true) {
            return Err(ProcessControlError::Failed(
                "injected priority read failure".to_owned(),
            ));
        }
        self.processes
            .get(process)
            .map(|process| process.priority)
            .ok_or(ProcessControlError::ProcessExited)
    }

    fn query_power(
        &mut self,
        process: &Self::Process,
    ) -> Result<PowerThrottlingState, ProcessControlError> {
        if self.power_unavailable {
            return Err(ProcessControlError::Unavailable(
                "injected unavailable power state".to_owned(),
            ));
        }
        self.processes
            .get(process)
            .map(|process| process.power)
            .ok_or(ProcessControlError::ProcessExited)
    }

    fn begin_priority_change(
        &mut self,
        _process: &Self::Process,
        _original: u32,
        _expected: u32,
    ) -> Result<Self::RecoveryIntent, ProcessControlError> {
        if std::mem::take(&mut self.fail_next_priority_begin) {
            return Err(ProcessControlError::Failed(
                "injected priority begin failure".to_owned(),
            ));
        }
        Ok(FakeIntent {
            fail: std::mem::take(&mut self.fail_next_priority_commit),
            retained: Rc::clone(&self.retained_intents),
        })
    }

    fn begin_power_change(
        &mut self,
        _process: &Self::Process,
        _original: PowerThrottlingState,
        _expected: PowerThrottlingState,
    ) -> Result<Self::RecoveryIntent, ProcessControlError> {
        if std::mem::take(&mut self.fail_next_power_begin) {
            return Err(ProcessControlError::Failed(
                "injected power begin failure".to_owned(),
            ));
        }
        Ok(FakeIntent {
            fail: self
                .power_commit_failures
                .pop_front()
                .unwrap_or_else(|| std::mem::take(&mut self.fail_next_power_commit)),
            retained: Rc::clone(&self.retained_intents),
        })
    }

    fn apply_priority(
        &mut self,
        process: &Self::Process,
        priority: u32,
    ) -> Result<(), ProcessControlError> {
        if std::mem::take(&mut self.exit_on_apply) {
            self.processes.remove(process);
            self.deny_open = true;
            return Err(ProcessControlError::ProcessExited);
        }

        if std::mem::take(&mut self.fail_next_priority_apply) {
            return Err(ProcessControlError::Failed(
                "injected priority apply failure".to_owned(),
            ));
        }
        let state = self
            .processes
            .get_mut(process)
            .ok_or(ProcessControlError::ProcessExited)?;
        state.priority = priority;
        self.events.push(FakeEvent::Priority(*process, priority));
        Ok(())
    }

    fn apply_power(
        &mut self,
        process: &Self::Process,
        power: PowerThrottlingState,
    ) -> Result<(), ProcessControlError> {
        if std::mem::take(&mut self.exit_on_apply) {
            self.processes.remove(process);
            self.deny_open = true;
            return Err(ProcessControlError::ProcessExited);
        }

        if std::mem::take(&mut self.fail_next_power_apply) {
            return Err(ProcessControlError::Failed(
                "injected power apply failure".to_owned(),
            ));
        }
        let state = self
            .processes
            .get_mut(process)
            .ok_or(ProcessControlError::ProcessExited)?;
        state.power = power;
        self.events.push(FakeEvent::Power(*process, power));
        Ok(())
    }

    fn relinquish_priority(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<(), ProcessControlError> {
        if self.fail_priority_relinquish {
            return Err(ProcessControlError::Failed(
                "injected priority relinquish failure".to_owned(),
            ));
        }
        self.events.push(FakeEvent::RelinquishPriority(
            identity.id,
            identity.creation_time,
        ));
        Ok(())
    }

    fn relinquish_power(&mut self, identity: &ProcessIdentity) -> Result<(), ProcessControlError> {
        if self.fail_power_relinquish {
            return Err(ProcessControlError::Failed(
                "injected power relinquish failure".to_owned(),
            ));
        }
        self.events.push(FakeEvent::RelinquishPower(
            identity.id,
            identity.creation_time,
        ));
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

fn action_target(id: u32, creation_time: u64) -> ProcessActionTarget {
    let identity = identity(id, creation_time);
    ProcessActionTarget {
        id,
        name: identity.name,
        executable_path: identity.executable_path,
        creation_time,
        session_id: Some(1),
        is_service_account: Some(false),
    }
}

fn baseline_power() -> PowerThrottlingState {
    PowerThrottlingState {
        version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        control_mask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        state_mask: 0,
    }
}

fn platform_with(id: u32, creation_time: u64, priority: u32) -> FakePlatform {
    let mut platform = FakePlatform::default();
    platform.processes.insert(
        id,
        FakeProcess {
            identity: identity(id, creation_time),
            priority,
            power: baseline_power(),
        },
    );
    platform
}

fn priority_claim(
    id: u32,
    creation_time: u64,
    owner: ControlOwner,
    priority: PriorityClassValue,
) -> PriorityClassClaim {
    PriorityClassClaim {
        target: target(id, creation_time),
        owner,
        priority,
        preservation: PriorityClassPreservation::Exact,
    }
}

fn power_claim(id: u32, creation_time: u64, ignore_timer_resolution: bool) -> PowerThrottlingClaim {
    PowerThrottlingClaim {
        target: target(id, creation_time),
        owner: ControlOwner::EfficiencyMode,
        ignore_timer_resolution,
    }
}

fn efficiency_claim(id: u32, creation_time: u64) -> EfficiencyModeClaim {
    EfficiencyModeClaim {
        target: target(id, creation_time),
        owner: ControlOwner::EfficiencyMode,
        ignore_timer_resolution: false,
    }
}

fn restraint_claim(id: u32, creation_time: u64) -> PriorityClassClaim {
    let mut claim = priority_claim(
        id,
        creation_time,
        ControlOwner::AdaptiveEngine,
        PriorityClassValue::BelowNormal,
    );
    claim.preservation = PriorityClassPreservation::OnlyNormal;
    claim
}

#[test]
fn stored_priority_shadows_restraint_and_hands_back_to_it_when_released() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    controller
        .apply_priority_claim(
            priority_claim(
                7,
                1,
                ControlOwner::ProcessPriority,
                PriorityClassValue::AboveNormal,
            ),
            true,
        )
        .unwrap();
    assert_eq!(
        controller.can_apply_adaptive_restraint(&target(7, 1), true, None, true),
        Ok(false)
    );
    assert_eq!(
        controller.apply_priority_claim(restraint_claim(7, 1), true),
        Ok(ProcessPropertyApplyOutcome::Shadowed)
    );
    assert!(controller.adaptive_restraint_processes().next().is_none());
    assert_eq!(
        controller.platform.processes[&7].priority,
        ABOVE_NORMAL_PRIORITY_CLASS
    );

    controller.release_all_priority_policy(ControlOwner::ProcessPriority);
    assert_eq!(
        controller.adaptive_restraint_processes().next(),
        Some((&identity(7, 1), AdaptiveRestraintState::Verified))
    );
    assert_eq!(
        controller.platform.processes[&7].priority,
        BELOW_NORMAL_PRIORITY_CLASS
    );
    controller.release_all_priority_policy(ControlOwner::AdaptiveEngine);
    assert!(controller.adaptive_restraint_processes().next().is_none());
    assert_eq!(
        controller.platform.processes[&7].priority,
        NORMAL_PRIORITY_CLASS
    );
}

#[test]
fn efficiency_mode_shadows_process_priority_then_reveals_lower_claims() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    controller
        .apply_priority_claim(
            priority_claim(
                7,
                1,
                ControlOwner::AdaptiveEngine,
                PriorityClassValue::BelowNormal,
            ),
            true,
        )
        .unwrap();
    controller
        .apply_efficiency_claim(efficiency_claim(7, 1), true)
        .unwrap();

    assert_eq!(
        controller.apply_priority_claim(
            priority_claim(
                7,
                1,
                ControlOwner::ProcessPriority,
                PriorityClassValue::AboveNormal,
            ),
            true,
        ),
        Ok(ProcessPropertyApplyOutcome::Shadowed)
    );
    assert_eq!(
        controller.platform.processes[&7].priority,
        IDLE_PRIORITY_CLASS
    );
    assert!(controller.platform.processes[&7]
        .power
        .execution_speed_enabled());
    let summary = controller.release_all_efficiency_policy(ControlOwner::EfficiencyMode);
    assert!(summary.failures.is_empty());
    assert_eq!(
        controller.platform.processes[&7].priority,
        ABOVE_NORMAL_PRIORITY_CLASS
    );
    assert_eq!(controller.platform.processes[&7].power, baseline_power());
    assert_eq!(
        controller.policy_managed_process_count(ControlOwner::EfficiencyMode),
        0
    );

    controller.release_all_priority_policy(ControlOwner::ProcessPriority);
    assert_eq!(
        controller.platform.processes[&7].priority,
        BELOW_NORMAL_PRIORITY_CLASS
    );
    controller.release_all_priority_policy(ControlOwner::AdaptiveEngine);
    assert_eq!(
        controller.platform.processes[&7].priority,
        NORMAL_PRIORITY_CLASS
    );
    assert_eq!(controller.platform.processes[&7].power, baseline_power());
}

#[test]
fn process_list_action_is_immediate_then_automatic_policy_supersedes_it() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    let automatic = priority_claim(
        7,
        1,
        ControlOwner::ProcessPriority,
        PriorityClassValue::Idle,
    );
    controller
        .apply_priority_claim(automatic.clone(), true)
        .unwrap();
    controller
        .apply_process_list_priority(
            &action_target(7, 1),
            ProcessPrioritySetting::AboveNormal,
            true,
        )
        .unwrap();
    assert_eq!(
        controller.platform.processes[&7].priority,
        ABOVE_NORMAL_PRIORITY_CLASS
    );

    controller.apply_priority_claim(automatic, true).unwrap();
    assert_eq!(
        controller.platform.processes[&7].priority,
        IDLE_PRIORITY_CLASS
    );
    controller.shutdown().unwrap();
    assert_eq!(
        controller.platform.processes[&7].priority,
        NORMAL_PRIORITY_CLASS
    );
}

#[test]
fn process_list_priority_yields_restraint_for_the_current_target_episode() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    let restraint = restraint_claim(7, 1);
    assert_eq!(
        controller.can_apply_adaptive_restraint(&restraint.target, true, None, true),
        Ok(true)
    );
    assert_eq!(
        controller.apply_priority_claim(restraint.clone(), true),
        Ok(ProcessPropertyApplyOutcome::Applied)
    );
    controller
        .apply_process_list_priority(&action_target(7, 1), ProcessPrioritySetting::Normal, true)
        .unwrap();
    controller.reconcile_adaptive_restraint_targets(
        &BTreeSet::new(),
        &BTreeSet::from([restraint.target.key()]),
    );
    assert_eq!(
        controller.can_apply_adaptive_restraint(&restraint.target, true, None, true),
        Ok(false)
    );
    assert_eq!(
        controller.apply_priority_claim(restraint.clone(), true),
        Ok(ProcessPropertyApplyOutcome::Preserved)
    );
    assert_eq!(
        controller.platform.processes[&7].priority,
        NORMAL_PRIORITY_CLASS
    );

    controller.release_all_priority_policy(ControlOwner::AdaptiveEngine);
    assert_eq!(
        controller.apply_priority_claim(restraint, true),
        Ok(ProcessPropertyApplyOutcome::Applied)
    );
    assert_eq!(
        controller.platform.processes[&7].priority,
        BELOW_NORMAL_PRIORITY_CLASS
    );
}

#[test]
fn choosing_the_current_restrained_priority_holds_it_until_exit() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    controller
        .apply_priority_claim(restraint_claim(7, 1), true)
        .unwrap();
    assert_eq!(
        controller.apply_process_list_priority(
            &action_target(7, 1),
            ProcessPrioritySetting::BelowNormal,
            true,
        ),
        Ok(ProcessPropertyApplyOutcome::Unchanged)
    );
    assert!(controller.adaptive_restraint_processes().next().is_none());
    controller.release_all_priority_policy(ControlOwner::AdaptiveEngine);
    assert_eq!(
        controller.platform.processes[&7].priority,
        BELOW_NORMAL_PRIORITY_CLASS
    );
    controller.shutdown().unwrap();
    assert_eq!(
        controller.platform.processes[&7].priority,
        NORMAL_PRIORITY_CLASS
    );
}

#[test]
fn restraint_audit_distinguishes_uncertain_apply_from_pending_restore() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    let process = identity(7, 1);
    controller.platform.priority_query_failures = VecDeque::from([false, true, true]);
    assert!(controller
        .apply_priority_claim(restraint_claim(7, 1), true)
        .is_err());
    assert_eq!(
        controller
            .adaptive_restraint_processes()
            .collect::<Vec<_>>(),
        [(&process, AdaptiveRestraintState::ConfirmationPending)]
    );

    controller.platform.deny_open = true;
    let released = controller.release_all_priority_policy(ControlOwner::AdaptiveEngine);
    assert_eq!(released.failures.len(), 1);
    assert_eq!(
        controller
            .adaptive_restraint_processes()
            .collect::<Vec<_>>(),
        [(&process, AdaptiveRestraintState::RestorationPending)]
    );

    controller.platform.deny_open = false;
    let retries = controller.retry_pending_releases(controller.release_retry_at.unwrap());
    assert!(retries.other.failures.is_empty());
    assert!(controller.adaptive_restraint_processes().next().is_none());
}

#[test]
fn restraint_does_not_write_a_priority_class_other_than_normal() {
    for original in [
        IDLE_PRIORITY_CLASS,
        BELOW_NORMAL_PRIORITY_CLASS,
        ABOVE_NORMAL_PRIORITY_CLASS,
        HIGH_PRIORITY_CLASS,
        REALTIME_PRIORITY_CLASS,
    ] {
        let mut controller =
            PriorityEfficiencyController::with_platform(platform_with(7, 1, original));
        assert_eq!(
            controller.can_apply_adaptive_restraint(&target(7, 1), true, None, true),
            Ok(false)
        );
        assert_eq!(
            controller.apply_priority_claim(restraint_claim(7, 1), true),
            Ok(ProcessPropertyApplyOutcome::Preserved),
        );
        assert_eq!(controller.platform.processes[&7].priority, original);
        assert_eq!(
            controller.policy_managed_process_count(ControlOwner::AdaptiveEngine),
            0
        );
    }
}

#[test]
fn opt_in_restraint_lowers_known_priority_and_restores_the_exact_baseline() {
    for original in [
        NORMAL_PRIORITY_CLASS,
        ABOVE_NORMAL_PRIORITY_CLASS,
        HIGH_PRIORITY_CLASS,
        BELOW_NORMAL_PRIORITY_CLASS,
        IDLE_PRIORITY_CLASS,
        REALTIME_PRIORITY_CLASS,
        0,
    ] {
        let mut controller =
            PriorityEfficiencyController::with_platform(platform_with(7, 1, original));
        let mut claim = restraint_claim(7, 1);
        claim.preservation = PriorityClassPreservation::LowerKnownClass;
        let target = claim.target.clone();
        let can_lower = [
            NORMAL_PRIORITY_CLASS,
            ABOVE_NORMAL_PRIORITY_CLASS,
            HIGH_PRIORITY_CLASS,
        ]
        .contains(&original);

        assert_eq!(
            controller.can_apply_adaptive_restraint(&target, true, Some(claim.priority), false),
            Ok(can_lower)
        );
        assert_eq!(
            controller.can_apply_adaptive_restraint(&target, true, None, false),
            Ok(original != REALTIME_PRIORITY_CLASS && original != 0)
        );
        assert_eq!(
            controller.apply_priority_claim(claim.clone(), true),
            Ok(if can_lower {
                ProcessPropertyApplyOutcome::Applied
            } else {
                ProcessPropertyApplyOutcome::Preserved
            })
        );
        if can_lower {
            assert_eq!(
                controller.adaptive_restraint_processes().next(),
                Some((&identity(7, 1), AdaptiveRestraintState::Verified))
            );
            assert_eq!(
                controller.can_apply_adaptive_restraint(&target, true, Some(claim.priority), true),
                Ok(original == NORMAL_PRIORITY_CLASS)
            );
        } else {
            assert!(controller.adaptive_restraint_processes().next().is_none());
        }
        controller.release_all_priority_policy(ControlOwner::AdaptiveEngine);
        assert_eq!(controller.platform.processes[&7].priority, original);
    }
}

#[test]
fn external_priority_change_yields_until_the_current_qualification_ends() {
    for change_after_preflight in [false, true] {
        let mut controller = PriorityEfficiencyController::with_platform(platform_with(
            7,
            1,
            ABOVE_NORMAL_PRIORITY_CLASS,
        ));
        let mut claim = restraint_claim(7, 1);
        claim.preservation = PriorityClassPreservation::LowerKnownClass;
        assert_eq!(
            controller.apply_priority_claim(claim.clone(), true),
            Ok(ProcessPropertyApplyOutcome::Applied)
        );

        let check = |controller: &mut PriorityEfficiencyController<_>| {
            controller.can_apply_adaptive_restraint(
                &claim.target,
                true,
                Some(claim.priority),
                false,
            )
        };
        if change_after_preflight {
            assert_eq!(check(&mut controller), Ok(true));
        }
        controller.platform.processes.get_mut(&7).unwrap().priority = HIGH_PRIORITY_CLASS;
        if change_after_preflight {
            assert_eq!(
                controller.apply_priority_claim(claim.clone(), true),
                Ok(ProcessPropertyApplyOutcome::Preserved)
            );
        } else {
            assert_eq!(check(&mut controller), Ok(false));
        }

        let release = controller.reconcile_adaptive_restraint_targets(
            &BTreeSet::new(),
            &BTreeSet::from([claim.target.key()]),
        );
        assert!(release.failures.is_empty());
        assert_eq!(
            controller.platform.processes[&7].priority,
            HIGH_PRIORITY_CLASS
        );
        assert!(controller.adaptive_restraint_processes().next().is_none());
        assert_eq!(check(&mut controller), Ok(false));

        controller.reconcile_adaptive_restraint_targets(&BTreeSet::new(), &BTreeSet::new());
        assert_eq!(check(&mut controller), Ok(true));
    }
}

#[test]
fn idle_restraint_remains_eligible_and_verified_until_restored() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    let process = identity(7, 1);
    let mut claim = restraint_claim(7, 1);
    let target = claim.target.clone();

    assert_eq!(
        controller.can_apply_adaptive_restraint(&target, true, None, true),
        Ok(true)
    );
    assert_eq!(
        controller.apply_priority_claim(claim.clone(), true),
        Ok(ProcessPropertyApplyOutcome::Applied)
    );
    claim.priority = PriorityClassValue::Idle;
    assert_eq!(
        controller.can_apply_adaptive_restraint(&target, true, None, true),
        Ok(true)
    );
    assert_eq!(
        controller.apply_priority_claim(claim, true),
        Ok(ProcessPropertyApplyOutcome::Applied)
    );
    assert_eq!(
        controller.platform.processes[&7].priority,
        IDLE_PRIORITY_CLASS
    );
    assert_eq!(
        controller.can_apply_adaptive_restraint(&target, true, None, true),
        Ok(true)
    );
    assert_eq!(
        controller
            .adaptive_restraint_processes()
            .collect::<Vec<_>>(),
        [(&process, AdaptiveRestraintState::Verified)],
    );

    controller.platform.deny_open = true;
    assert_eq!(
        controller
            .release_all_priority_policy(ControlOwner::AdaptiveEngine)
            .failures
            .len(),
        1
    );
    controller.platform.deny_open = false;
    assert!(controller.adaptive_restraint_restore_pending(
        target.id,
        target.creation_time,
        &target.executable_path
    ));
    assert_eq!(
        controller.can_apply_adaptive_restraint(&target, true, None, true),
        Ok(false)
    );
    assert_eq!(
        controller.platform.processes[&7].priority,
        IDLE_PRIORITY_CLASS
    );

    let retries = controller.retry_pending_releases(controller.release_retry_at.unwrap());
    assert!(retries.other.failures.is_empty());
    assert_eq!(
        controller.platform.processes[&7].priority,
        NORMAL_PRIORITY_CLASS
    );
    assert!(controller.adaptive_restraint_processes().next().is_none());
    assert_eq!(
        controller.can_apply_adaptive_restraint(&target, true, None, true),
        Ok(true)
    );

    controller.platform.processes.get_mut(&7).unwrap().priority = IDLE_PRIORITY_CLASS;
    assert_eq!(
        controller.can_apply_adaptive_restraint(&target, true, None, true),
        Ok(false)
    );
}

#[test]
fn unavailable_power_state_does_not_block_an_independent_priority_claim() {
    let mut platform = platform_with(7, 1, NORMAL_PRIORITY_CLASS);
    platform.power_unavailable = true;
    let mut controller = PriorityEfficiencyController::with_platform(platform);

    assert!(matches!(
        controller.apply_effective_power(power_claim(7, 1, false), true,),
        Err(ProcessControlError::Unavailable(_))
    ));
    assert_eq!(
        controller
            .apply_priority_claim(
                priority_claim(
                    7,
                    1,
                    ControlOwner::AdaptiveEngine,
                    PriorityClassValue::BelowNormal,
                ),
                true,
            )
            .unwrap(),
        ProcessPropertyApplyOutcome::Applied
    );
    assert_eq!(
        controller.platform.processes[&7].priority,
        BELOW_NORMAL_PRIORITY_CLASS
    );
}

#[test]
fn compound_priority_failure_rolls_power_back_and_leaves_no_owner() {
    let mut platform = platform_with(7, 1, NORMAL_PRIORITY_CLASS);
    platform.fail_next_priority_apply = true;
    let mut controller = PriorityEfficiencyController::with_platform(platform);

    assert!(controller
        .apply_efficiency_claim(efficiency_claim(7, 1), true)
        .is_err());
    assert_eq!(
        controller.platform.processes[&7].priority,
        NORMAL_PRIORITY_CLASS
    );
    assert_eq!(controller.platform.processes[&7].power, baseline_power());
    assert!(!controller.has_managed_state());
    assert!(controller
        .priority_claims
        .get(&ControlOwner::EfficiencyMode)
        .is_none_or(BTreeMap::is_empty));
    assert!(controller
        .power_claims
        .get(&ControlOwner::EfficiencyMode)
        .is_none_or(BTreeMap::is_empty));
}

#[test]
fn compound_commit_failure_retains_both_verified_properties_and_baselines() {
    let mut platform = platform_with(7, 1, NORMAL_PRIORITY_CLASS);
    platform.fail_next_priority_commit = true;
    let mut controller = PriorityEfficiencyController::with_platform(platform);

    assert!(controller
        .apply_efficiency_claim(efficiency_claim(7, 1), true)
        .is_err());
    assert_eq!(
        controller.platform.processes[&7].priority,
        IDLE_PRIORITY_CLASS
    );
    let desired_power = baseline_power().with_efficiency_enabled(false);
    assert_eq!(controller.platform.processes[&7].power, desired_power);
    assert!(!controller
        .platform
        .events
        .contains(&FakeEvent::RelinquishPriority(7, 1)));
    let process = identity(7, 1);
    assert_eq!(
        controller.managed_priorities[&process].baseline,
        NORMAL_PRIORITY_CLASS
    );
    assert_eq!(
        controller.managed_priorities[&process].expected,
        IDLE_PRIORITY_CLASS
    );
    assert_eq!(
        controller.managed_power[&process].baseline,
        baseline_power()
    );
    assert_eq!(controller.managed_power[&process].expected, desired_power);
    let released = controller.release_all_efficiency_policy(ControlOwner::EfficiencyMode);
    assert!(released.failures.is_empty());
    assert_eq!(released.restored_processes, 1);
    assert!(!controller.has_managed_state());
    assert_eq!(
        controller.platform.processes[&7].priority,
        NORMAL_PRIORITY_CLASS
    );
    assert_eq!(controller.platform.processes[&7].power, baseline_power());
}

#[test]
fn compound_commit_failure_preserves_prior_priority_and_power_baselines() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    controller
        .apply_priority_claim(
            priority_claim(
                7,
                1,
                ControlOwner::AdaptiveEngine,
                PriorityClassValue::BelowNormal,
            ),
            true,
        )
        .unwrap();
    controller
        .apply_effective_power(power_claim(7, 1, false), true)
        .unwrap();
    controller.platform.events.clear();
    controller.platform.fail_next_priority_commit = true;
    let mut efficiency = efficiency_claim(7, 1);
    efficiency.ignore_timer_resolution = true;
    assert!(controller.apply_efficiency_claim(efficiency, true).is_err());

    let process = identity(7, 1);
    let desired_power = baseline_power().with_efficiency_enabled(true);
    assert_eq!(
        controller.platform.processes[&7].priority,
        IDLE_PRIORITY_CLASS
    );
    assert_eq!(controller.platform.processes[&7].power, desired_power);
    assert_eq!(
        controller.managed_priorities[&process].baseline,
        NORMAL_PRIORITY_CLASS
    );
    assert_eq!(
        controller.managed_priorities[&process].expected,
        IDLE_PRIORITY_CLASS
    );
    assert_eq!(
        controller.managed_power[&process].baseline,
        baseline_power()
    );
    assert_eq!(controller.managed_power[&process].expected, desired_power);
    assert_eq!(
        controller.platform.events,
        [
            FakeEvent::Power(7, desired_power),
            FakeEvent::Priority(7, IDLE_PRIORITY_CLASS)
        ]
    );
    controller.shutdown().unwrap();
    assert_eq!(
        controller.platform.processes[&7].priority,
        NORMAL_PRIORITY_CLASS
    );
    assert_eq!(controller.platform.processes[&7].power, baseline_power());
}

#[test]
fn power_commit_failure_preserves_the_prior_power_baseline() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    controller
        .apply_effective_power(power_claim(7, 1, false), true)
        .unwrap();
    controller.platform.events.clear();
    controller.platform.fail_next_power_commit = true;
    assert!(controller
        .apply_effective_power(power_claim(7, 1, true), true)
        .is_err());
    let desired_power = baseline_power().with_efficiency_enabled(true);
    let process = identity(7, 1);
    assert_eq!(controller.platform.processes[&7].power, desired_power);
    assert_eq!(
        controller.managed_power[&process].baseline,
        baseline_power()
    );
    assert_eq!(controller.managed_power[&process].expected, desired_power);
    assert_eq!(
        controller.platform.events,
        [FakeEvent::Power(7, desired_power)]
    );
    controller.shutdown().unwrap();
    assert_eq!(controller.platform.processes[&7].power, baseline_power());
}

#[test]
fn uncertain_priority_compensation_keeps_both_compound_baselines() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    controller
        .apply_priority_claim(
            priority_claim(
                7,
                1,
                ControlOwner::AdaptiveEngine,
                PriorityClassValue::BelowNormal,
            ),
            true,
        )
        .unwrap();
    controller
        .apply_effective_power(power_claim(7, 1, false), true)
        .unwrap();
    controller.platform.events.clear();
    controller.platform.priority_query_failures = VecDeque::from([false, true, true]);
    let mut efficiency = efficiency_claim(7, 1);
    efficiency.ignore_timer_resolution = true;
    assert!(controller.apply_efficiency_claim(efficiency, true).is_err());

    let process = identity(7, 1);
    let desired_power = baseline_power().with_efficiency_enabled(true);
    assert_eq!(
        controller.platform.processes[&7].priority,
        BELOW_NORMAL_PRIORITY_CLASS
    );
    assert_eq!(controller.platform.processes[&7].power, desired_power);
    assert_eq!(
        controller.managed_priorities[&process].baseline,
        NORMAL_PRIORITY_CLASS
    );
    assert_eq!(
        controller.managed_priorities[&process].expected,
        IDLE_PRIORITY_CLASS
    );
    assert_eq!(
        controller.managed_power[&process].baseline,
        baseline_power()
    );
    assert_eq!(controller.managed_power[&process].expected, desired_power);
    assert_eq!(controller.platform.retained_intents.get(), 1);
    assert_eq!(
        controller.platform.events,
        [
            FakeEvent::Power(7, desired_power),
            FakeEvent::Priority(7, IDLE_PRIORITY_CLASS),
            FakeEvent::Priority(7, BELOW_NORMAL_PRIORITY_CLASS)
        ]
    );
}

#[test]
fn compound_rollback_commit_failure_tracks_the_verified_rollback_target() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    controller
        .apply_priority_claim(
            priority_claim(
                7,
                1,
                ControlOwner::AdaptiveEngine,
                PriorityClassValue::BelowNormal,
            ),
            true,
        )
        .unwrap();
    controller
        .apply_effective_power(power_claim(7, 1, false), true)
        .unwrap();
    controller.platform.events.clear();
    controller.platform.fail_next_priority_apply = true;
    controller.platform.power_commit_failures = VecDeque::from([false, true]);
    let mut efficiency = efficiency_claim(7, 1);
    efficiency.ignore_timer_resolution = true;
    assert!(controller.apply_efficiency_claim(efficiency, true).is_err());
    let process = identity(7, 1);
    let previous_power = baseline_power().with_efficiency_enabled(false);
    assert_eq!(controller.platform.processes[&7].power, previous_power);
    assert_eq!(
        controller.platform.processes[&7].priority,
        BELOW_NORMAL_PRIORITY_CLASS
    );
    assert_eq!(
        controller.managed_power[&process].baseline,
        baseline_power()
    );
    assert_eq!(controller.managed_power[&process].expected, previous_power);
    assert_eq!(
        controller.managed_priorities[&process].baseline,
        NORMAL_PRIORITY_CLASS
    );
    assert!(!controller.platform.events.iter().any(|event| matches!(
        event,
        FakeEvent::RelinquishPower(..) | FakeEvent::RelinquishPriority(..)
    )));
    controller.shutdown().unwrap();
    assert_eq!(controller.platform.processes[&7].power, baseline_power());
    assert_eq!(
        controller.platform.processes[&7].priority,
        NORMAL_PRIORITY_CLASS
    );
}

#[test]
fn external_break_is_relinquished_without_overwriting_either_property() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    controller
        .apply_efficiency_claim(efficiency_claim(7, 1), true)
        .unwrap();
    let external_power = PowerThrottlingState {
        version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        control_mask: PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        state_mask: PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
    };
    let process = controller.platform.processes.get_mut(&7).unwrap();
    process.priority = ABOVE_NORMAL_PRIORITY_CLASS;
    process.power = external_power;

    controller.release_all_efficiency_policy(ControlOwner::EfficiencyMode);
    assert_eq!(
        controller.platform.processes[&7].priority,
        ABOVE_NORMAL_PRIORITY_CLASS
    );
    assert_eq!(controller.platform.processes[&7].power, external_power);
    assert!(controller
        .platform
        .events
        .contains(&FakeEvent::RelinquishPriority(7, 1)));
    assert!(controller
        .platform
        .events
        .contains(&FakeEvent::RelinquishPower(7, 1)));
    assert!(!controller.has_managed_state());
}

#[test]
fn pid_reuse_relinquishes_the_old_identity_before_mutating_the_new_process() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    controller
        .apply_efficiency_claim(efficiency_claim(7, 1), true)
        .unwrap();
    controller.platform.processes.insert(
        7,
        FakeProcess {
            identity: identity(7, 2),
            priority: NORMAL_PRIORITY_CLASS,
            power: baseline_power(),
        },
    );

    controller
        .apply_efficiency_claim(efficiency_claim(7, 2), true)
        .unwrap();
    assert!(controller
        .platform
        .events
        .contains(&FakeEvent::RelinquishPriority(7, 1)));
    assert!(controller
        .platform
        .events
        .contains(&FakeEvent::RelinquishPower(7, 1)));
    assert_eq!(
        controller.platform.processes[&7].priority,
        IDLE_PRIORITY_CLASS
    );
}

#[test]
fn session_metadata_drift_does_not_create_a_second_process_identity() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    controller
        .apply_priority_claim(
            priority_claim(
                7,
                1,
                ControlOwner::ProcessPriority,
                PriorityClassValue::BelowNormal,
            ),
            true,
        )
        .unwrap();
    controller
        .platform
        .processes
        .get_mut(&7)
        .unwrap()
        .identity
        .session_id = None;
    controller
        .apply_priority_claim(
            priority_claim(
                7,
                1,
                ControlOwner::ProcessPriority,
                PriorityClassValue::BelowNormal,
            ),
            true,
        )
        .unwrap();

    assert_eq!(controller.managed_priorities.len(), 1);
    assert!(!controller
        .platform
        .events
        .contains(&FakeEvent::RelinquishPriority(7, 1)));
}

#[test]
fn shutdown_keeps_confirmed_exit_when_reopening_is_denied() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    controller
        .apply_priority_claim(
            priority_claim(
                7,
                1,
                ControlOwner::ProcessPriority,
                PriorityClassValue::BelowNormal,
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
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    controller
        .apply_priority_claim(
            priority_claim(
                7,
                1,
                ControlOwner::ProcessPriority,
                PriorityClassValue::BelowNormal,
            ),
            true,
        )
        .unwrap();
    controller.platform.fail_next_priority_commit = true;

    let summary = controller.release_all_priority_policy(ControlOwner::ProcessPriority);
    assert!(summary.failures.is_empty());
    assert_eq!(
        controller.platform.processes[&7].priority,
        NORMAL_PRIORITY_CLASS
    );
    assert!(controller
        .platform
        .events
        .contains(&FakeEvent::RelinquishPriority(7, 1)));
    assert!(!controller.has_managed_state());
}

#[test]
fn shutdown_restores_priority_and_power_in_reverse_application_order() {
    let mut platform = platform_with(7, 1, NORMAL_PRIORITY_CLASS);
    platform.processes.insert(
        8,
        FakeProcess {
            identity: identity(8, 1),
            priority: NORMAL_PRIORITY_CLASS,
            power: baseline_power(),
        },
    );
    let mut controller = PriorityEfficiencyController::with_platform(platform);
    controller
        .apply_efficiency_claim(efficiency_claim(7, 1), true)
        .unwrap();
    controller
        .apply_priority_claim(
            priority_claim(
                8,
                1,
                ControlOwner::ProcessPriority,
                PriorityClassValue::BelowNormal,
            ),
            true,
        )
        .unwrap();
    controller.shutdown().unwrap();

    let tail = &controller.platform.events[controller.platform.events.len() - 3..];
    assert_eq!(
        tail,
        &[
            FakeEvent::Priority(8, NORMAL_PRIORITY_CLASS),
            FakeEvent::Priority(7, NORMAL_PRIORITY_CLASS),
            FakeEvent::Power(7, baseline_power()),
        ]
    );
}

#[test]
#[ignore = "modifies and restores a disposable Windows process; run in explicit integration QA"]
fn priority_efficiency_live_apply_and_clean_release() -> Result<(), String> {
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
    let baseline_priority =
        current_process_priority(&target, true).map_err(|error| error.to_string())?;
    let baseline_efficiency =
        current_efficiency_mode(&target, true).map_err(|error| error.to_string())?;
    let expected_efficiency = !baseline_efficiency;
    let mut controller = PriorityEfficiencyController::default();

    verify_live_adaptive_restraint_power(&mut controller, &target)?;

    controller
        .apply_process_list_efficiency_mode(&target, expected_efficiency, true)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        current_efficiency_mode(&target, true).map_err(|error| error.to_string())?,
        expected_efficiency
    );
    controller.shutdown()?;
    assert_eq!(
        current_process_priority(&target, true).map_err(|error| error.to_string())?,
        baseline_priority
    );
    assert_eq!(
        current_efficiency_mode(&target, true).map_err(|error| error.to_string())?,
        baseline_efficiency
    );
    Ok(())
}

fn verify_live_adaptive_restraint_power(
    controller: &mut PriorityEfficiencyController,
    target: &ProcessActionTarget,
) -> Result<(), String> {
    let control_target = ProcessControlTarget::from_action_target(target);
    let (_, process) = controller
        .platform
        .open(&control_target, true)
        .map_err(|error| error.to_string())?;
    let original = controller
        .platform
        .query_power(&process)
        .map_err(|error| error.to_string())?;
    let speed = PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    let baseline = PowerThrottlingState {
        control_mask: original.control_mask & !speed,
        state_mask: original.state_mask & !speed,
        ..original
    };
    if baseline != original {
        controller
            .platform
            .apply_power(&process, baseline)
            .map_err(|error| error.to_string())?;
    }

    let priority_claim = PriorityClassClaim {
        target: control_target.clone(),
        owner: ControlOwner::AdaptiveEngine,
        priority: PriorityClassValue::BelowNormal,
        preservation: PriorityClassPreservation::OnlyNormal,
    };
    let outcome = controller
        .apply_priority_claim(priority_claim, true)
        .map_err(|error| error.to_string())?;
    assert_eq!(outcome, ProcessPropertyApplyOutcome::Applied);
    let outcome = controller
        .apply_adaptive_restraint_power(control_target, true)
        .map_err(|error| error.to_string())?;
    assert_eq!(outcome, ProcessPropertyApplyOutcome::Applied);
    assert_eq!(
        controller
            .platform
            .query_power(&process)
            .map_err(|error| error.to_string())?,
        baseline.with_execution_speed_enabled()
    );
    let release = controller.release_adaptive_restraint_power_except(&BTreeSet::new());
    assert!(release.failures.is_empty(), "{release:?}");
    assert_eq!(release.restored_processes, 1);
    assert_eq!(
        controller
            .platform
            .query_power(&process)
            .map_err(|error| error.to_string())?,
        baseline
    );
    assert_eq!(
        controller
            .platform
            .query_priority(&process)
            .map_err(|error| error.to_string())?,
        BELOW_NORMAL_PRIORITY_CLASS
    );
    controller.shutdown()?;
    if baseline != original {
        controller
            .platform
            .apply_power(&process, original)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}
#[test]
fn failed_policy_releases_retry_without_claims_or_another_policy_update() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    controller
        .apply_efficiency_claim(efficiency_claim(7, 1), true)
        .unwrap();
    controller.platform.fail_next_priority_apply = true;
    controller.platform.fail_next_power_apply = true;
    let failed = controller.release_all_efficiency_policy(ControlOwner::EfficiencyMode);
    assert_eq!(failed.failures.len(), 2);
    assert!(controller.priority_claims.is_empty());
    assert!(controller.power_claims.is_empty());
    let early = controller.release_all_efficiency_policy(ControlOwner::EfficiencyMode);
    assert_eq!(early.restored_processes, 0);
    assert!(early.failures.is_empty());
    let due = controller.release_retry_at.unwrap();
    assert!(controller
        .retry_pending_releases(due - Duration::from_millis(1))
        .efficiency_mode
        .is_none());
    // A second transient failure must preserve the queue and its bounded retry deadline.
    controller.platform.fail_next_priority_apply = true;
    let partial = controller
        .retry_pending_releases(due)
        .efficiency_mode
        .unwrap();
    assert_eq!(partial.restored_processes, 0);
    assert!(partial.failures.is_empty());
    assert_eq!(controller.efficiency_mode_release_failures().count(), 1);
    assert_eq!(
        controller.release_retry_delay(due),
        Some(Duration::from_secs(1))
    );

    for (seconds, newly_reported) in [(1, true), (2, false)] {
        controller.platform.fail_next_priority_begin = true;
        let retries = controller.retry_pending_releases(due + Duration::from_secs(seconds));
        let background = retries.efficiency_mode.unwrap();
        assert_eq!(background.failures.len(), usize::from(newly_reported));
        let pending = controller
            .efficiency_mode_release_failures()
            .next()
            .unwrap();
        assert!(pending
            .error
            .to_string()
            .contains("injected priority begin failure"));
    }

    let restored = controller
        .retry_pending_releases(due + Duration::from_secs(3))
        .efficiency_mode
        .unwrap();
    assert!(restored.failures.is_empty());
    assert_eq!(restored.restored_processes, 1);
    assert_eq!(controller.efficiency_mode_release_failures().count(), 0);
    assert_eq!(
        controller.platform.processes[&7].priority,
        NORMAL_PRIORITY_CLASS
    );
    assert_eq!(controller.platform.processes[&7].power, baseline_power());
    assert!(!controller.has_managed_state());
    assert_eq!(controller.release_retry_delay(due), None);
}

#[test]
fn verified_baseline_is_not_reported_restored_while_journal_release_is_unconfirmed() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    controller
        .apply_efficiency_claim(efficiency_claim(7, 1), true)
        .unwrap();
    controller.platform.fail_next_power_commit = true;
    controller.platform.fail_power_relinquish = true;

    let released = controller.release_all_efficiency_policy(ControlOwner::EfficiencyMode);
    assert_eq!(controller.platform.processes[&7].power, baseline_power());
    assert_eq!(released.restored_processes, 0);
    assert_eq!(controller.efficiency_mode_release_failures().count(), 1);
    assert!(controller.has_managed_state());

    controller.platform.fail_power_relinquish = false;
    let due = controller.release_retry_at.unwrap();
    let report = controller
        .retry_pending_releases(due)
        .efficiency_mode
        .unwrap();
    assert_eq!(report.restored_processes, 0);
    assert!(report.failures.is_empty());
    assert_eq!(controller.efficiency_mode_release_failures().count(), 0);
    assert!(!controller.has_managed_state());
}

#[test]
fn pending_release_reconciles_the_current_claim_instead_of_restoring_over_it() {
    let mut controller =
        PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
    controller
        .apply_priority_claim(restraint_claim(7, 1), true)
        .unwrap();
    let stored = priority_claim(
        7,
        1,
        ControlOwner::ProcessPriority,
        PriorityClassValue::AboveNormal,
    );
    controller
        .apply_priority_claim(stored.clone(), true)
        .unwrap();
    controller.platform.fail_next_priority_apply = true;
    assert_eq!(
        controller
            .release_all_priority_policy(ControlOwner::ProcessPriority)
            .failures
            .len(),
        1
    );
    let due = controller.release_retry_at.unwrap();
    let retries = controller.retry_pending_releases(due);
    assert!(retries.other.failures.is_empty());
    assert!(retries.efficiency_mode.is_none());
    assert_eq!(retries.process_priority_restored, Some(0));
    assert_eq!(
        controller.platform.processes[&7].priority,
        BELOW_NORMAL_PRIORITY_CLASS
    );
    assert_eq!(controller.release_retry_delay(due), None);
    controller.release_all_priority_policy(ControlOwner::AdaptiveEngine);
    controller.apply_priority_claim(stored, true).unwrap();
    controller.platform.fail_next_priority_apply = true;
    let failed = controller.release_all_priority_policy(ControlOwner::ProcessPriority);
    assert_eq!(failed.failures.len(), 1);
    assert_eq!(controller.process_priority_release_failures().count(), 1);

    let due = controller.release_retry_at.unwrap();
    let retries = controller.retry_pending_releases(due);
    assert_eq!(retries.process_priority_restored, Some(1));
    assert!(retries.other.failures.is_empty());
    assert_eq!(controller.process_priority_release_failures().count(), 0);
    assert_eq!(
        controller.platform.processes[&7].priority,
        NORMAL_PRIORITY_CLASS
    );
}
