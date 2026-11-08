use crate::{
    action_log::{ActionLog, ActionLogFeature},
    cpu_topology::{test_selection, test_topology},
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};

use super::*;

#[derive(Clone)]
struct FakeProcessState {
    identity: ProcessIdentity,
    affinity: usize,
    system_affinity: usize,
    cpu_sets: Vec<u32>,
}

#[derive(Default)]
struct FakeState {
    exit_on_apply: bool,
    deny_open: bool,
    denied_processes: BTreeSet<u32>,
    processes: BTreeMap<u32, FakeProcessState>,
    events: Vec<String>,
    reject_disallowed_cross_session_open: bool,
    open_cross_session_flags: Vec<bool>,
    affinity_apply_failures_remaining: usize,
    next_affinity_error: Option<ProcessControlError>,
    fail_next_cpu_sets_apply: bool,
    fail_inventory: bool,
    cpu_set_entries: Option<Vec<(u8, u32)>>,
    reject_group_zero_affinity: bool,
    fail_next_commit: bool,
    fail_next_affinity_relinquish: bool,
    fail_next_cpu_sets_relinquish: bool,
    affinity_query_failures: VecDeque<bool>,
    cpu_sets_query_failures: VecDeque<bool>,
}

#[derive(Clone, Default)]
struct FakePlatform {
    state: Rc<RefCell<FakeState>>,
}

struct FakeIntent {
    state: Rc<RefCell<FakeState>>,
    fail: bool,
}

impl CpuAllocationRecoveryIntent for FakeIntent {
    fn commit(self) -> Result<(), String> {
        if self.fail {
            Err("injected commit failure".to_owned())
        } else {
            self.state.borrow_mut().events.push("commit".to_owned());
            Ok(())
        }
    }

    fn retain_pending(self) -> Result<(), String> {
        self.state
            .borrow_mut()
            .events
            .push("retain-pending".to_owned());
        Ok(())
    }
}

impl CpuAllocationPlatform for FakePlatform {
    type Process = u32;
    type RecoveryIntent = FakeIntent;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError> {
        let mut state = self.state.borrow_mut();
        if state.deny_open || state.denied_processes.contains(&target.key().id) {
            return Err(ProcessControlError::AccessDenied("open denied".into()));
        }
        state
            .open_cross_session_flags
            .push(allow_cross_session_process_control);
        if state.reject_disallowed_cross_session_open && !allow_cross_session_process_control {
            return Err(ProcessControlError::AccessDenied(
                "injected cross-session denial".to_owned(),
            ));
        }
        let Some(process) = state.processes.get(&target.id) else {
            return Err(ProcessControlError::ProcessExited);
        };
        if process.identity.creation_time != target.creation_time
            || !crate::foreground::same_executable_path(
                &process.identity.executable_path,
                &target.executable_path,
            )
        {
            return Err(ProcessControlError::ProcessExited);
        }
        Ok((process.identity.clone(), target.id))
    }

    fn query_affinity(
        &mut self,
        process: &Self::Process,
    ) -> Result<(usize, usize), ProcessControlError> {
        let mut state = self.state.borrow_mut();
        if state.affinity_query_failures.pop_front() == Some(true) {
            return Err(ProcessControlError::Failed(
                "injected affinity read failure".to_owned(),
            ));
        }
        let process = state
            .processes
            .get(process)
            .ok_or(ProcessControlError::ProcessExited)?;
        Ok((process.affinity, process.system_affinity))
    }

    fn validate_group_zero_affinity(
        &mut self,
        _process: &Self::Process,
    ) -> Result<(), ProcessControlError> {
        if self.state.borrow().reject_group_zero_affinity {
            Err(ProcessControlError::Failed(
                "target processor group is unsupported".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn query_cpu_sets(&mut self, process: &Self::Process) -> Result<Vec<u32>, ProcessControlError> {
        let mut state = self.state.borrow_mut();
        if state.cpu_sets_query_failures.pop_front() == Some(true) {
            return Err(ProcessControlError::Failed(
                "injected CPU Sets read failure".to_owned(),
            ));
        }
        state
            .processes
            .get(process)
            .map(|process| process.cpu_sets.clone())
            .ok_or(ProcessControlError::ProcessExited)
    }

    fn cpu_set_inventory(&mut self) -> Result<Vec<(u8, u32)>, ProcessControlError> {
        self.state
            .borrow_mut()
            .events
            .push("cpu-set-inventory".into());
        if self.state.borrow().fail_inventory {
            return Err(ProcessControlError::Failed(
                "CPU Set discovery failed".into(),
            ));
        }
        if let Some(entries) = &self.state.borrow().cpu_set_entries {
            return Ok(entries.clone());
        }
        Ok((0..64u8).map(|bit| (bit, 100 + u32::from(bit))).collect())
    }

    fn begin_affinity_change(
        &mut self,
        _process: &Self::Process,
        original: usize,
        expected: usize,
    ) -> Result<Self::RecoveryIntent, ProcessControlError> {
        let mut state = self.state.borrow_mut();
        state
            .events
            .push(format!("begin-affinity:{original:#x}->{expected:#x}"));
        let fail = std::mem::take(&mut state.fail_next_commit);
        Ok(FakeIntent {
            state: Rc::clone(&self.state),
            fail,
        })
    }

    fn begin_cpu_sets_change(
        &mut self,
        _process: &Self::Process,
        original: &[u32],
        expected: &[u32],
    ) -> Result<Self::RecoveryIntent, ProcessControlError> {
        let mut state = self.state.borrow_mut();
        state
            .events
            .push(format!("begin-cpu-sets:{original:?}->{expected:?}"));
        let fail = std::mem::take(&mut state.fail_next_commit);
        Ok(FakeIntent {
            state: Rc::clone(&self.state),
            fail,
        })
    }

    fn apply_affinity(
        &mut self,
        process: &Self::Process,
        affinity: usize,
    ) -> Result<(), ProcessControlError> {
        let mut state = self.state.borrow_mut();
        if std::mem::take(&mut state.exit_on_apply) {
            state.processes.remove(process);
            state.deny_open = true;
            return Err(ProcessControlError::ProcessExited);
        }

        state.events.push(format!("apply-affinity:{affinity:#x}"));
        if let Some(error) = state.next_affinity_error.take() {
            return Err(error);
        }
        if state.affinity_apply_failures_remaining > 0 {
            state.affinity_apply_failures_remaining -= 1;
            return Err(ProcessControlError::Failed(
                "injected affinity apply failure".to_owned(),
            ));
        }
        state
            .processes
            .get_mut(process)
            .ok_or(ProcessControlError::ProcessExited)?
            .affinity = affinity;
        Ok(())
    }

    fn apply_cpu_sets(
        &mut self,
        process: &Self::Process,
        ids: &[u32],
    ) -> Result<(), ProcessControlError> {
        let mut state = self.state.borrow_mut();
        if std::mem::take(&mut state.exit_on_apply) {
            state.processes.remove(process);
            state.deny_open = true;
            return Err(ProcessControlError::ProcessExited);
        }

        state.events.push(format!("apply-cpu-sets:{ids:?}"));
        if std::mem::take(&mut state.fail_next_cpu_sets_apply) {
            return Err(ProcessControlError::Failed(
                "injected CPU Sets apply failure".to_owned(),
            ));
        }
        state
            .processes
            .get_mut(process)
            .ok_or(ProcessControlError::ProcessExited)?
            .cpu_sets = ids.to_vec();
        Ok(())
    }

    fn relinquish_affinity(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<(), ProcessControlError> {
        let mut state = self.state.borrow_mut();
        state
            .events
            .push(format!("forget-affinity:{}", identity.creation_time));
        if std::mem::take(&mut state.fail_next_affinity_relinquish) {
            return Err(ProcessControlError::Failed(
                "injected affinity relinquish failure".to_owned(),
            ));
        }
        Ok(())
    }

    fn relinquish_cpu_sets(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<(), ProcessControlError> {
        let mut state = self.state.borrow_mut();
        state
            .events
            .push(format!("forget-cpu-sets:{}", identity.creation_time));
        if std::mem::take(&mut state.fail_next_cpu_sets_relinquish) {
            return Err(ProcessControlError::Failed(
                "injected CPU Sets relinquish failure".to_owned(),
            ));
        }
        Ok(())
    }
}

fn identity(id: u32, creation_time: u64) -> ProcessIdentity {
    ProcessIdentity::new(
        id,
        "worker.exe".to_owned(),
        PathBuf::from(r"C:\Apps\worker.exe"),
        creation_time,
        Some(1),
    )
}

fn target(id: u32, creation_time: u64) -> ProcessControlTarget {
    ProcessControlTarget::automatic(
        id,
        "worker.exe".to_owned(),
        PathBuf::from(r"C:\Apps\worker.exe"),
        creation_time,
    )
}

fn coordinator() -> CpuAllocationCoordinator<FakePlatform> {
    let platform = FakePlatform::default();
    platform.state.borrow_mut().processes.insert(
        42,
        FakeProcessState {
            identity: identity(42, 7),
            affinity: 0b1111,
            system_affinity: 0b1111,
            cpu_sets: Vec::new(),
        },
    );
    CpuAllocationCoordinator::with_platform(platform)
}

fn claim(
    owner: ControlOwner,
    request: CpuAllocationRequest,
    creation_time: u64,
) -> CpuAllocationClaim {
    claim_for(42, owner, request, creation_time)
}

fn claim_for(
    process_id: u32,
    owner: ControlOwner,
    request: CpuAllocationRequest,
    creation_time: u64,
) -> CpuAllocationClaim {
    CpuAllocationClaim {
        target: target(process_id, creation_time),
        owner,
        request,
    }
}

fn process_state(coordinator: &CpuAllocationCoordinator<FakePlatform>) -> FakeProcessState {
    coordinator
        .platform
        .state
        .borrow()
        .processes
        .get(&42)
        .cloned()
        .expect("fake process should exist")
}

fn allocation_target(id: u32, mask: u64, hard: bool) -> crate::cpu_allocation::CpuAllocationTarget {
    crate::cpu_allocation::CpuAllocationTarget {
        process_id: id,
        process_name: "worker.exe".into(),
        executable_path: r"C:\Apps\worker.exe".into(),
        creation_time: 7,
        selection: test_selection(mask),
        mode: if hard {
            crate::cpu_allocation::CpuAllocationMode::HardAffinity
        } else {
            crate::cpu_allocation::CpuAllocationMode::SoftCpuSets
        },
    }
}

fn changed_topology() -> CpuTopology {
    let original = test_topology();
    let mut processors = original.processors().to_vec();
    processors[0].efficiency_class = 1;
    CpuTopology::new(
        processors,
        original.groups().to_vec(),
        Vec::new(),
        Vec::new(),
    )
    .unwrap()
}

#[test]
fn stale_selection_restores_its_allocation_without_suppressing_apps_or_releasing_valid_owners() {
    use crate::cpu_allocation::CpuAllocationManager;
    let mut coordinator = coordinator();
    let changed = changed_topology();
    let mut second = process_state(&coordinator);
    second.identity = identity(43, 7);
    coordinator
        .platform
        .state
        .borrow_mut()
        .processes
        .insert(43, second);
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(1),
                },
                7,
            ),
            true,
        )
        .unwrap();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&changed),
            claim_for(
                43,
                ControlOwner::ProcessorAffinityHard,
                CpuAllocationRequest::HardAffinity {
                    selection: CpuSelection::from_group_zero_mask(&changed, 3).unwrap(),
                },
                7,
            ),
            true,
        )
        .unwrap();
    let mut manager = CpuAllocationManager::default();
    let mut log = ActionLog::default();
    for _ in 0..4 {
        let result = manager.reconcile_targets(
            &mut coordinator,
            Ok(&changed),
            ControlOwner::CpuSetsSoft,
            vec![allocation_target(42, 1, false)],
            None,
            2,
            String::new(),
            true,
            &mut log,
        );
        assert_eq!(result.snapshot.failed_processes, 0);
        assert!(result.snapshot.last_error.is_some());
        assert!(result.snapshot.auto_excluded_processes.is_empty());
        assert!(process_state(&coordinator).cpu_sets.is_empty());
        assert_eq!(
            coordinator.platform.state.borrow().processes[&43].affinity,
            3
        );
    }
    assert_eq!(
        coordinator.policy_managed_process_count(ControlOwner::CpuSetsSoft),
        0
    );
    assert_eq!(
        coordinator.policy_managed_process_count(ControlOwner::ProcessorAffinityHard),
        1
    );
}

#[test]
fn unavailable_topology_cancels_a_handoff_and_retries_restoration_without_discovery() {
    let mut coordinator = coordinator();
    for (owner, request) in [
        (
            ControlOwner::ProcessorAffinityHard,
            CpuAllocationRequest::HardAffinity {
                selection: test_selection(3),
            },
        ),
        (
            ControlOwner::AdaptiveEngine,
            CpuAllocationRequest::SoftCpuSets {
                selection: test_selection(1),
            },
        ),
    ] {
        coordinator
            .apply_policy_claim(
                &mut CpuSetInventory::default(),
                Ok(&test_topology()),
                claim(owner, request, 7),
                true,
            )
            .unwrap();
    }
    coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
    coordinator
        .platform
        .state
        .borrow_mut()
        .affinity_apply_failures_remaining = 1;
    let failed = coordinator.reconcile_pending(Err("topology unavailable"), true, false);
    assert!(failed.applications.is_empty());
    assert!(!failed.failures.is_empty());
    assert!(coordinator.has_pending_release_retry());
    assert_eq!(process_state(&coordinator).affinity, 3);
    assert!(coordinator.claims.values().all(BTreeMap::is_empty));

    let retry = coordinator.reconcile_pending(Err("topology unavailable"), true, true);
    assert!(retry.applications.is_empty());
    assert!(retry.releases.failures.is_empty());
    assert_eq!(process_state(&coordinator).affinity, 0b1111);
    assert!(!coordinator.has_managed_state());
    assert!(!coordinator.has_pending_reconciliation());
}

#[test]
fn an_invalid_lower_claim_does_not_release_a_valid_higher_allocation() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(1),
                },
                7,
            ),
            true,
        )
        .unwrap();
    let changed = changed_topology();
    let result = coordinator.apply_policy_claim(
        &mut CpuSetInventory::default(),
        Ok(&test_topology()),
        claim(
            ControlOwner::AdaptiveEngine,
            CpuAllocationRequest::HardAffinity {
                selection: CpuSelection::from_group_zero_mask(&changed, 3).unwrap(),
            },
            7,
        ),
        true,
    );
    assert!(matches!(result, Err(CpuAllocationApplyError::Topology(_))));
    assert_eq!(process_state(&coordinator).cpu_sets, vec![100]);
    assert_eq!(
        coordinator.policy_managed_process_count(ControlOwner::CpuSetsSoft),
        1
    );
}

#[test]
fn disabling_adaptive_allocation_preserves_an_explicit_cpu_owner() {
    use crate::cpu_allocation::CpuAllocationManager;
    let mut coordinator = coordinator();
    for (owner, request) in [
        (
            ControlOwner::CpuSetsSoft,
            CpuAllocationRequest::SoftCpuSets {
                selection: test_selection(1),
            },
        ),
        (
            ControlOwner::AdaptiveEngine,
            CpuAllocationRequest::HardAffinity {
                selection: test_selection(3),
            },
        ),
    ] {
        coordinator
            .apply_policy_claim(
                &mut CpuSetInventory::default(),
                Ok(&test_topology()),
                claim(owner, request, 7),
                true,
            )
            .unwrap();
    }
    let mut manager =
        CpuAllocationManager::with_action_log_feature(ActionLogFeature::AdaptiveEngine);
    let result = manager.clear_discovered_targets(
        &mut coordinator,
        ControlOwner::AdaptiveEngine,
        "Adaptive Engine disabled.",
        &mut ActionLog::default(),
    );
    assert_eq!(result.failed_processes, 0);
    assert!(result.last_error.is_none());
    assert_eq!(process_state(&coordinator).cpu_sets, vec![100]);
    assert_eq!(
        coordinator.policy_managed_process_count(ControlOwner::CpuSetsSoft),
        1
    );
    assert!(coordinator.claims[&ControlOwner::AdaptiveEngine].is_empty());
    assert!(!coordinator.has_pending_reconciliation());
}

#[test]
fn unsupported_hard_group_does_not_release_the_previous_soft_assignment() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::AdaptiveEngine,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(0b0010),
                },
                7,
            ),
            true,
        )
        .unwrap();
    {
        let mut state = coordinator.platform.state.borrow_mut();
        state.events.clear();
        state.reject_group_zero_affinity = true;
    }
    let result = coordinator.apply_policy_claim(
        &mut CpuSetInventory::default(),
        Ok(&test_topology()),
        claim(
            ControlOwner::ProcessorAffinityHard,
            CpuAllocationRequest::HardAffinity {
                selection: test_selection(0b0110),
            },
            7,
        ),
        true,
    );
    assert!(matches!(result, Err(CpuAllocationApplyError::Target(_))));
    assert_eq!(process_state(&coordinator).cpu_sets, vec![101]);
    assert_eq!(process_state(&coordinator).affinity, 0b1111);
    assert_eq!(coordinator.managed_cpu_sets.len(), 1);
    assert!(coordinator.platform.state.borrow().events.is_empty());
}

#[test]
fn hard_group_acquisition_guard_does_not_block_release_or_shutdown() {
    for shutdown in [false, true] {
        let mut coordinator = coordinator();
        coordinator
            .apply_policy_claim(
                &mut CpuSetInventory::default(),
                Ok(&test_topology()),
                claim(
                    ControlOwner::ProcessorAffinityHard,
                    CpuAllocationRequest::HardAffinity {
                        selection: test_selection(0b0011),
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        coordinator
            .platform
            .state
            .borrow_mut()
            .reject_group_zero_affinity = true;
        if shutdown {
            coordinator.shutdown().unwrap();
        } else {
            let release = coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
            assert!(release.failures.is_empty());
        }
        assert_eq!(process_state(&coordinator).affinity, 0b1111);
        assert!(!coordinator.has_managed_state());
    }
}

#[test]
fn missing_soft_processors_do_not_shrink_a_selection_or_release_hard_affinity() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::AdaptiveEngine,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0011),
                },
                7,
            ),
            true,
        )
        .unwrap();
    {
        let mut state = coordinator.platform.state.borrow_mut();
        state.events.clear();
        state.cpu_set_entries = Some(vec![(0, 100)]);
    }
    let mut inventory = CpuSetInventory::default();
    let mut soft = claim(
        ControlOwner::CpuSetsSoft,
        CpuAllocationRequest::SoftCpuSets {
            selection: test_selection(0b0011),
        },
        7,
    );
    assert!(matches!(
        coordinator.apply_policy_claim(&mut inventory, Ok(&test_topology()), soft.clone(), true),
        Err(CpuAllocationApplyError::Discovery(_))
    ));
    assert!(!inventory.failed());
    assert_eq!(process_state(&coordinator).affinity, 0b0011);
    assert!(process_state(&coordinator).cpu_sets.is_empty());
    assert_eq!(
        coordinator.platform.state.borrow().events,
        vec!["cpu-set-inventory"]
    );

    soft.request = CpuAllocationRequest::SoftCpuSets {
        selection: test_selection(0b0001),
    };
    coordinator
        .apply_policy_claim(&mut inventory, Ok(&test_topology()), soft, true)
        .unwrap();
    assert_eq!(process_state(&coordinator).affinity, 0b1111);
    assert_eq!(process_state(&coordinator).cpu_sets, vec![100]);
    assert!(inventory
        .ids(&mut coordinator.platform, 0)
        .unwrap()
        .is_empty());
}

#[test]
fn invalid_soft_inventory_is_cached_before_any_mode_switch() {
    for entries in [
        vec![(0, 100), (0, 101)],
        vec![(0, 100), (1, 100)],
        vec![(64, 100)],
    ] {
        let mut coordinator = coordinator();
        coordinator
            .apply_policy_claim(
                &mut CpuSetInventory::default(),
                Ok(&test_topology()),
                claim(
                    ControlOwner::AdaptiveEngine,
                    CpuAllocationRequest::HardAffinity {
                        selection: test_selection(0b0011),
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        {
            let mut state = coordinator.platform.state.borrow_mut();
            state.events.clear();
            state.cpu_set_entries = Some(entries);
        }
        let mut inventory = CpuSetInventory::default();
        assert!(matches!(
            coordinator.apply_policy_claim(
                &mut inventory,
                Ok(&test_topology()),
                claim(
                    ControlOwner::CpuSetsSoft,
                    CpuAllocationRequest::SoftCpuSets {
                        selection: test_selection(1),
                    },
                    7,
                ),
                true,
            ),
            Err(CpuAllocationApplyError::Discovery(_))
        ));
        assert!(inventory.failed());
        coordinator.platform.state.borrow_mut().cpu_set_entries = None;
        assert!(inventory.ids(&mut coordinator.platform, 1).is_err());
        assert_eq!(process_state(&coordinator).affinity, 0b0011);
        assert!(process_state(&coordinator).cpu_sets.is_empty());
        assert_eq!(
            coordinator.platform.state.borrow().events,
            vec!["cpu-set-inventory"]
        );
    }
}

#[test]
fn pending_handoffs_keep_error_origin_after_shared_discovery_failure() {
    let mut coordinator = coordinator();
    let mut second = process_state(&coordinator);
    second.identity = identity(43, 7);
    coordinator
        .platform
        .state
        .borrow_mut()
        .processes
        .insert(43, second);
    for id in [42, 43] {
        coordinator
            .apply_policy_claim(
                &mut CpuSetInventory::default(),
                Ok(&test_topology()),
                claim_for(
                    id,
                    ControlOwner::AdaptiveEngine,
                    CpuAllocationRequest::HardAffinity {
                        selection: test_selection(3),
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        let next = claim_for(
            id,
            ControlOwner::AdaptiveEngine,
            if id == 42 {
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(3),
                }
            } else {
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(12),
                }
            },
            7,
        );
        coordinator
            .claims
            .entry(ControlOwner::AdaptiveEngine)
            .or_default()
            .insert(next.target.key(), next.clone());
        coordinator
            .pending_reconciliations
            .insert(next.target.key(), PendingReconciliation::Handoff);
    }
    coordinator.platform.state.borrow_mut().fail_inventory = true;
    coordinator
        .platform
        .state
        .borrow_mut()
        .affinity_apply_failures_remaining = 1;
    let outcome = coordinator.reconcile_pending(Ok(&test_topology()), true, false);
    assert_eq!(outcome.failures.len(), 2);
    assert_eq!(
        coordinator.platform.state.borrow().processes[&42].affinity,
        3
    );
    assert_eq!(
        coordinator.platform.state.borrow().processes[&43].affinity,
        15
    );
    assert_eq!(coordinator.pending_reconciliations.len(), 1);
    assert!(matches!(
        coordinator.pending_reconciliations[&target(42, 7).key()],
        PendingReconciliation::HandoffRetry
    ));
    coordinator.platform.state.borrow_mut().fail_inventory = false;
    assert!(coordinator
        .reconcile_pending(Ok(&test_topology()), true, true)
        .failures
        .is_empty());
    coordinator.release_all_policy(ControlOwner::AdaptiveEngine);
}

#[test]
fn cached_discovery_failure_does_not_reclassify_later_target_errors() {
    let mut coordinator = coordinator();
    let mut inventory = CpuSetInventory::default();
    coordinator.platform.state.borrow_mut().fail_inventory = true;
    let soft = claim(
        ControlOwner::AdaptiveEngine,
        CpuAllocationRequest::SoftCpuSets {
            selection: test_selection(3),
        },
        7,
    );
    let hard = claim(
        ControlOwner::AdaptiveEngine,
        CpuAllocationRequest::HardAffinity {
            selection: test_selection(3),
        },
        7,
    );
    assert!(matches!(
        coordinator.apply_policy_claim(&mut inventory, Ok(&test_topology()), soft.clone(), true),
        Err(CpuAllocationApplyError::Discovery(_))
    ));
    coordinator
        .platform
        .state
        .borrow_mut()
        .affinity_apply_failures_remaining = 1;
    assert!(matches!(
        coordinator.apply_policy_claim(&mut inventory, Ok(&test_topology()), hard.clone(), true),
        Err(CpuAllocationApplyError::Target(
            ProcessControlError::Failed(_)
        ))
    ));
    coordinator.platform.state.borrow_mut().deny_open = true;
    assert!(matches!(
        coordinator.apply_policy_claim(&mut inventory, Ok(&test_topology()), soft, true),
        Err(CpuAllocationApplyError::Target(
            ProcessControlError::AccessDenied(_)
        ))
    ));
    coordinator.platform.state.borrow_mut().deny_open = false;
    assert!(coordinator
        .apply_policy_claim(&mut inventory, Ok(&test_topology()), hard, true)
        .is_ok());
    assert_eq!(process_state(&coordinator).affinity, 3);
    coordinator.release_all_policy(ControlOwner::AdaptiveEngine);
    assert_eq!(process_state(&coordinator).affinity, 15);
}

#[test]
fn mapping_failure_is_shared_without_suppressing_apps_or_losing_assignments() {
    use crate::{action_log::ActionLog, cpu_allocation::CpuAllocationManager};
    let mut coordinator = coordinator();
    let mut second = process_state(&coordinator);
    second.identity = identity(43, 7);
    coordinator
        .platform
        .state
        .borrow_mut()
        .processes
        .insert(43, second);
    let mut manager = CpuAllocationManager::default();
    let mut log = ActionLog::default();
    let targets = || {
        vec![
            allocation_target(42, 3, false),
            allocation_target(43, 12, false),
        ]
    };
    let reconcile = |manager: &mut CpuAllocationManager,
                     coordinator: &mut CpuAllocationCoordinator<FakePlatform>,
                     log: &mut ActionLog| {
        manager.reconcile_targets(
            coordinator,
            Ok(&test_topology()),
            ControlOwner::CpuSetsSoft,
            targets(),
            None,
            2,
            String::new(),
            true,
            log,
        )
    };
    reconcile(&mut manager, &mut coordinator, &mut log);
    assert_eq!(
        coordinator
            .platform
            .state
            .borrow()
            .events
            .iter()
            .filter(|e| *e == "cpu-set-inventory")
            .count(),
        1
    );
    coordinator.platform.state.borrow_mut().fail_inventory = true;
    for _ in 0..4 {
        coordinator.platform.state.borrow_mut().events.clear();
        let result = reconcile(&mut manager, &mut coordinator, &mut log);
        assert!(result.snapshot.auto_excluded_processes.is_empty());
        assert!(result.snapshot.last_error.is_some());
        assert_eq!(
            coordinator
                .platform
                .state
                .borrow()
                .events
                .iter()
                .filter(|e| *e == "cpu-set-inventory")
                .count(),
            1
        );
        assert_eq!(process_state(&coordinator).cpu_sets, vec![100, 101]);
        assert_eq!(
            coordinator.policy_managed_process_count(ControlOwner::CpuSetsSoft),
            2
        );
    }
    let key = process_state(&coordinator).identity.key();
    coordinator
        .pending_reconciliations
        .insert(key, PendingReconciliation::Handoff);
    assert!(!coordinator
        .reconcile_pending(Ok(&test_topology()), true, false)
        .failures
        .is_empty());
    assert!(!coordinator.has_pending_immediate_reconciliation());
    assert!(coordinator.has_pending_release_retry());
    reconcile(&mut manager, &mut coordinator, &mut log);
    assert!(coordinator
        .pending_reconciliations
        .values()
        .all(|pending| { matches!(pending, PendingReconciliation::HandoffRetry) }));
    coordinator.platform.state.borrow_mut().events.clear();
    coordinator.reconcile_pending(Ok(&test_topology()), true, false);
    assert!(coordinator.platform.state.borrow().events.is_empty());
    coordinator.platform.state.borrow_mut().fail_inventory = false;
    coordinator.reconcile_pending(Ok(&test_topology()), true, true);
    assert!(!coordinator.has_pending_reconciliation());
    assert!(reconcile(&mut manager, &mut coordinator, &mut log)
        .snapshot
        .last_error
        .is_none());
    coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
    coordinator.reconcile_pending(Ok(&test_topology()), true, true);
    assert!(process_state(&coordinator).cpu_sets.is_empty());
}

#[test]
fn zone_generation_orders_background_first_and_preserves_handoff_baselines() {
    use crate::{
        action_log::{ActionLog, ActionLogFeature},
        cpu_allocation::CpuAllocationManager,
    };
    let mut coordinator = coordinator();
    let mut foreground = process_state(&coordinator);
    foreground.identity = identity(43, 7);
    coordinator
        .platform
        .state
        .borrow_mut()
        .processes
        .insert(43, foreground);
    let mut manager =
        CpuAllocationManager::with_action_log_feature(ActionLogFeature::AdaptiveEngine);
    let mut log = ActionLog::default();
    let fallback = || vec![allocation_target(42, 3, true)];
    manager.reconcile_targets(
        &mut coordinator,
        Ok(&test_topology()),
        ControlOwner::AdaptiveEngine,
        fallback(),
        None,
        2,
        String::new(),
        true,
        &mut log,
    );
    assert_eq!(process_state(&coordinator).affinity, 3);
    let zones = || {
        Some((
            vec![allocation_target(42, 1, false)],
            vec![allocation_target(43, 14, false)],
        ))
    };
    let result = manager.reconcile_targets(
        &mut coordinator,
        Ok(&test_topology()),
        ControlOwner::AdaptiveEngine,
        fallback(),
        zones(),
        2,
        String::new(),
        true,
        &mut log,
    );
    assert_eq!(result.zone_counts, Some((1, 1)));
    assert_eq!(process_state(&coordinator).affinity, 15);
    let events = coordinator.platform.state.borrow().events.clone();
    assert!(
        events
            .iter()
            .position(|e| e == "apply-cpu-sets:[100]")
            .unwrap()
            < events
                .iter()
                .position(|e| e == "apply-cpu-sets:[101, 102, 103]")
                .unwrap()
    );
    let writes = events.iter().filter(|e| *e != "cpu-set-inventory").count();
    manager.reconcile_targets(
        &mut coordinator,
        Ok(&test_topology()),
        ControlOwner::AdaptiveEngine,
        fallback(),
        zones(),
        2,
        String::new(),
        true,
        &mut log,
    );
    assert_eq!(
        coordinator
            .platform
            .state
            .borrow()
            .events
            .iter()
            .filter(|e| *e != "cpu-set-inventory")
            .count(),
        writes,
        "unchanged placement must not write again"
    );
    manager.reconcile_targets(
        &mut coordinator,
        Ok(&test_topology()),
        ControlOwner::AdaptiveEngine,
        fallback(),
        None,
        2,
        String::new(),
        true,
        &mut log,
    );
    assert_eq!(process_state(&coordinator).affinity, 3);
    assert!(coordinator.platform.state.borrow().processes[&43]
        .cpu_sets
        .is_empty());
    manager.reconcile_targets(
        &mut coordinator,
        Ok(&test_topology()),
        ControlOwner::AdaptiveEngine,
        Vec::new(),
        None,
        2,
        String::new(),
        true,
        &mut log,
    );
    assert_eq!(process_state(&coordinator).affinity, 15);
    assert!(process_state(&coordinator).cpu_sets.is_empty());
}

#[test]
fn failed_or_shadowed_background_never_narrows_foreground() {
    use crate::{
        action_log::{ActionLog, ActionLogFeature},
        cpu_allocation::CpuAllocationManager,
    };
    for failure in 0..4 {
        let mut coordinator = coordinator();
        let mut foreground = process_state(&coordinator);
        foreground.identity = identity(43, 7);
        coordinator
            .platform
            .state
            .borrow_mut()
            .processes
            .insert(43, foreground);
        match failure {
            0 => {
                coordinator
                    .platform
                    .state
                    .borrow_mut()
                    .fail_next_cpu_sets_apply = true
            }
            1 => coordinator.platform.state.borrow_mut().fail_next_commit = true,
            2 => {
                coordinator
                    .apply_policy_claim(
                        &mut CpuSetInventory::default(),
                        Ok(&test_topology()),
                        claim(
                            ControlOwner::CpuSetsSoft,
                            CpuAllocationRequest::SoftCpuSets {
                                selection: test_selection(3),
                            },
                            7,
                        ),
                        true,
                    )
                    .unwrap();
            }
            _ => {
                coordinator
                    .platform
                    .state
                    .borrow_mut()
                    .denied_processes
                    .insert(42);
            }
        }
        let mut manager =
            CpuAllocationManager::with_action_log_feature(ActionLogFeature::AdaptiveEngine);
        let result = manager.reconcile_targets(
            &mut coordinator,
            Ok(&test_topology()),
            ControlOwner::AdaptiveEngine,
            Vec::new(),
            Some((
                vec![allocation_target(42, 1, false)],
                vec![allocation_target(43, 14, false)],
            )),
            2,
            String::new(),
            true,
            &mut ActionLog::default(),
        );
        assert_eq!(result.zone_counts, None);
        assert!(coordinator.platform.state.borrow().processes[&43]
            .cpu_sets
            .is_empty());
        assert!(!coordinator
            .platform
            .state
            .borrow()
            .events
            .iter()
            .any(|e| e == "apply-cpu-sets:[101, 102, 103]"));
    }
}

#[test]
fn partial_foreground_failure_withdraws_successful_zone_roles() {
    use crate::{
        action_log::{ActionLog, ActionLogFeature},
        cpu_allocation::CpuAllocationManager,
    };
    let mut coordinator = coordinator();
    for id in [43, 44] {
        let mut process = process_state(&coordinator);
        process.identity = identity(id, 7);
        coordinator
            .platform
            .state
            .borrow_mut()
            .processes
            .insert(id, process);
    }
    coordinator
        .platform
        .state
        .borrow_mut()
        .denied_processes
        .insert(44);
    let mut manager =
        CpuAllocationManager::with_action_log_feature(ActionLogFeature::AdaptiveEngine);
    let result = manager.reconcile_targets(
        &mut coordinator,
        Ok(&test_topology()),
        ControlOwner::AdaptiveEngine,
        vec![allocation_target(42, 3, true)],
        Some((
            vec![allocation_target(42, 1, false)],
            vec![
                allocation_target(43, 14, false),
                allocation_target(44, 14, false),
            ],
        )),
        3,
        String::new(),
        true,
        &mut ActionLog::default(),
    );
    assert_eq!(result.zone_counts, None);
    assert!(coordinator.platform.state.borrow().processes[&43]
        .cpu_sets
        .is_empty());
    // The failed foreground executable is suppressed; no successful zone claim is retained.
    assert!(coordinator
        .claims
        .get(&ControlOwner::AdaptiveEngine)
        .is_none_or(|claims| claims.keys().all(|key| *key == target(42, 7).key())));
}

#[test]
fn pass_end_reconciliation_hands_off_precedence_without_resubmission() {
    let mut coordinator = coordinator();
    let workload_key = target(42, 7).key();
    assert_eq!(
        coordinator.apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::AdaptiveEngine,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0011),
                },
                7,
            ),
            true,
        ),
        Ok(CpuAllocationApplyOutcome::Applied)
    );
    assert_eq!(process_state(&coordinator).affinity, 0b0011);

    assert_eq!(
        coordinator.apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(0b0010),
                },
                7,
            ),
            true,
        ),
        Ok(CpuAllocationApplyOutcome::Applied)
    );
    let state = process_state(&coordinator);
    assert_eq!(state.affinity, 0b1111);
    assert_eq!(state.cpu_sets, vec![101]);
    assert!(coordinator.managed_affinity.is_empty());
    assert_eq!(coordinator.managed_cpu_sets.len(), 1);

    assert_eq!(
        coordinator.apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::ProcessorAffinityHard,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0110),
                },
                7,
            ),
            true,
        ),
        Ok(CpuAllocationApplyOutcome::Shadowed)
    );
    let summary = coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
    assert!(summary.restored_owners.is_empty());
    assert!(coordinator.has_pending_immediate_reconciliation());
    let state = process_state(&coordinator);
    assert_eq!(state.cpu_sets, vec![101]);
    assert_eq!(state.affinity, 0b1111);

    let reconciliation = coordinator.reconcile_pending(Ok(&test_topology()), true, true);
    assert!(reconciliation.failures.is_empty());
    assert_eq!(reconciliation.applications.len(), 1);
    assert_eq!(
        reconciliation.applications[0].owner,
        ControlOwner::ProcessorAffinityHard
    );
    let state = process_state(&coordinator);
    assert!(state.cpu_sets.is_empty());
    assert_eq!(state.affinity, 0b0110);
    assert!(!coordinator.has_pending_immediate_reconciliation());

    coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
    assert_eq!(process_state(&coordinator).affinity, 0b0110);
    let reconciliation = coordinator.reconcile_pending(Ok(&test_topology()), true, true);
    assert!(reconciliation.failures.is_empty());
    assert_eq!(reconciliation.applications.len(), 1);
    assert_eq!(
        reconciliation.applications[0].owner,
        ControlOwner::AdaptiveEngine
    );
    assert_eq!(process_state(&coordinator).affinity, 0b0011);

    let summary = coordinator.release_all_policy(ControlOwner::AdaptiveEngine);
    assert!(summary.failures.is_empty());
    assert_eq!(process_state(&coordinator).affinity, 0b1111);
    assert!(coordinator.effective_claim(&workload_key).is_none());
    assert!(!coordinator.has_managed_state());
    assert!(!coordinator.has_pending_reconciliation());
}

#[test]
fn immediate_handoff_skips_an_unrelated_release_retry() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(0b0010),
                },
                7,
            ),
            true,
        )
        .unwrap();
    {
        let mut state = coordinator.platform.state.borrow_mut();
        state.processes.get_mut(&42).unwrap().cpu_sets = vec![103];
        state.fail_next_cpu_sets_relinquish = true;
    }
    let release = coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
    assert_eq!(release.failures.len(), 1);
    assert!(coordinator.has_pending_release_retry());

    coordinator.platform.state.borrow_mut().processes.insert(
        43,
        FakeProcessState {
            identity: identity(43, 8),
            affinity: 0b1111,
            system_affinity: 0b1111,
            cpu_sets: Vec::new(),
        },
    );
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim_for(
                43,
                ControlOwner::AdaptiveEngine,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0001),
                },
                8,
            ),
            true,
        )
        .unwrap();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim_for(
                43,
                ControlOwner::ProcessorAffinityHard,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0110),
                },
                8,
            ),
            true,
        )
        .unwrap();
    coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
    assert!(coordinator.has_pending_immediate_reconciliation());
    coordinator
        .platform
        .state
        .borrow_mut()
        .fail_next_cpu_sets_relinquish = true;

    let handoff = coordinator.reconcile_pending(Ok(&test_topology()), true, false);
    assert_eq!(handoff.applications.len(), 1);
    assert_eq!(handoff.applications[0].process_id, 43);
    assert!(
        coordinator
            .platform
            .state
            .borrow()
            .fail_next_cpu_sets_relinquish
    );
    assert!(coordinator.has_pending_release_retry());

    let failed_retry = coordinator.reconcile_pending(Ok(&test_topology()), true, true);
    assert!(failed_retry.releases.failures.is_empty());
    assert!(coordinator.has_pending_release_retry());
    let completed_retry = coordinator.reconcile_pending(Ok(&test_topology()), true, true);
    assert!(completed_retry.releases.failures.is_empty());
    assert!(!coordinator.has_pending_release_retry());
    coordinator.release_all_policy(ControlOwner::AdaptiveEngine);
}

#[test]
fn changed_effective_claim_promotes_a_cleanup_retry_to_handoff() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::AdaptiveEngine,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0001),
                },
                7,
            ),
            true,
        )
        .unwrap();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(0b0010),
                },
                7,
            ),
            true,
        )
        .unwrap();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::ProcessorAffinityHard,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0110),
                },
                7,
            ),
            true,
        )
        .unwrap();

    coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
    {
        let mut state = coordinator.platform.state.borrow_mut();
        state.reject_disallowed_cross_session_open = true;
        state.fail_next_commit = true;
        state.fail_next_cpu_sets_relinquish = true;
    }
    let failed_handoff = coordinator.reconcile_pending(Ok(&test_topology()), false, false);
    assert_eq!(failed_handoff.failures.len(), 1);
    assert_eq!(failed_handoff.releases.failures.len(), 1);
    assert!(coordinator.has_pending_release_retry());
    assert!(!coordinator.has_pending_immediate_reconciliation());

    coordinator
        .platform
        .state
        .borrow_mut()
        .reject_disallowed_cross_session_open = false;
    coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
    assert!(coordinator.has_pending_immediate_reconciliation());
    assert!(!coordinator.has_pending_release_retry());

    let handoff = coordinator.reconcile_pending(Ok(&test_topology()), true, false);
    assert!(handoff.failures.is_empty());
    assert_eq!(handoff.applications.len(), 1);
    assert_eq!(handoff.applications[0].owner, ControlOwner::AdaptiveEngine);
    assert_eq!(process_state(&coordinator).affinity, 0b0001);
    assert!(process_state(&coordinator).cpu_sets.is_empty());
    coordinator.release_all_policy(ControlOwner::AdaptiveEngine);
}

#[test]
fn final_lower_claim_removal_restores_the_actual_managed_owner() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::AdaptiveEngine,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0011),
                },
                7,
            ),
            true,
        )
        .unwrap();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(0b0010),
                },
                7,
            ),
            true,
        )
        .unwrap();

    let soft_summary = coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
    assert!(soft_summary.restored_owners.is_empty());
    assert_eq!(process_state(&coordinator).cpu_sets, vec![101]);

    let adaptive_summary = coordinator.release_all_policy(ControlOwner::AdaptiveEngine);
    assert_eq!(
        adaptive_summary.restored_owners,
        vec![ControlOwner::CpuSetsSoft]
    );
    let state = process_state(&coordinator);
    assert!(state.cpu_sets.is_empty());
    assert_eq!(state.affinity, 0b1111);
    assert!(!coordinator.has_managed_state());
    assert!(!coordinator.has_pending_reconciliation());
}

#[test]
fn releasing_a_shadowed_claim_does_not_reconcile_the_effective_owner() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(0b0010),
                },
                7,
            ),
            true,
        )
        .unwrap();
    assert_eq!(
        coordinator.apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::ProcessorAffinityHard,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0011),
                },
                7,
            ),
            true,
        ),
        Ok(CpuAllocationApplyOutcome::Shadowed)
    );

    let summary = coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
    assert!(summary.restored_owners.is_empty());
    assert!(summary.failures.is_empty());
    assert!(!coordinator.has_pending_reconciliation());
    let state = process_state(&coordinator);
    assert_eq!(state.cpu_sets, vec![101]);
    assert_eq!(state.affinity, 0b1111);
}

#[test]
fn failed_soft_mode_switch_restores_the_previous_hard_claim() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::AdaptiveEngine,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0011),
                },
                7,
            ),
            true,
        )
        .unwrap();
    coordinator
        .platform
        .state
        .borrow_mut()
        .fail_next_cpu_sets_apply = true;

    let result = coordinator.apply_policy_claim(
        &mut CpuSetInventory::default(),
        Ok(&test_topology()),
        claim(
            ControlOwner::CpuSetsSoft,
            CpuAllocationRequest::SoftCpuSets {
                selection: test_selection(0b0100),
            },
            7,
        ),
        true,
    );
    assert!(result.is_err());
    let state = process_state(&coordinator);
    assert_eq!(state.affinity, 0b0011);
    assert!(state.cpu_sets.is_empty());
    assert_eq!(coordinator.managed_affinity.len(), 1);
    assert!(coordinator.managed_cpu_sets.is_empty());
    assert!(coordinator
        .claims
        .get(&ControlOwner::CpuSetsSoft)
        .is_none_or(BTreeMap::is_empty));
}

#[test]
fn failed_hard_mode_switch_restores_the_previous_soft_claim() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::AdaptiveEngine,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(0b0010),
                },
                7,
            ),
            true,
        )
        .unwrap();
    coordinator
        .platform
        .state
        .borrow_mut()
        .affinity_apply_failures_remaining = 1;

    let result = coordinator.apply_policy_claim(
        &mut CpuSetInventory::default(),
        Ok(&test_topology()),
        claim(
            ControlOwner::ProcessorAffinityHard,
            CpuAllocationRequest::HardAffinity {
                selection: test_selection(0b0110),
            },
            7,
        ),
        true,
    );
    assert!(result.is_err());
    let state = process_state(&coordinator);
    assert_eq!(state.affinity, 0b1111);
    assert_eq!(state.cpu_sets, vec![101]);
    assert!(coordinator.managed_affinity.is_empty());
    assert_eq!(coordinator.managed_cpu_sets.len(), 1);
}

#[test]
fn uncertain_switch_reapply_keeps_restoration_state() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::AdaptiveEngine,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0011),
                },
                7,
            ),
            true,
        )
        .unwrap();
    let identity = identity(42, 7);
    let previous = coordinator
        .release_affinity_for_switch(&identity, &42)
        .unwrap()
        .unwrap();
    coordinator
        .platform
        .state
        .borrow_mut()
        .affinity_apply_failures_remaining = 2;

    let result = coordinator.reapply_affinity(&identity, &42, previous);
    assert!(result.is_err());
    assert_eq!(process_state(&coordinator).affinity, 0b1111);
    assert_eq!(coordinator.managed_affinity.len(), 1);
    assert_eq!(
        coordinator.managed_affinity.values().next().unwrap().owner,
        ControlOwner::AdaptiveEngine
    );

    let summary = coordinator.release_all_policy(ControlOwner::AdaptiveEngine);
    assert!(summary.failures.is_empty());
    assert!(!coordinator.has_managed_state());
}

#[test]
fn external_affinity_break_is_relinquished_without_overwrite() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::ProcessorAffinityHard,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0011),
                },
                7,
            ),
            true,
        )
        .unwrap();
    coordinator
        .platform
        .state
        .borrow_mut()
        .processes
        .get_mut(&42)
        .unwrap()
        .affinity = 0b0101;

    let summary = coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
    assert!(summary.failures.is_empty());
    assert!(summary.restored_owners.is_empty());
    assert_eq!(process_state(&coordinator).affinity, 0b0101);
    assert!(coordinator.managed_affinity.is_empty());
    assert!(coordinator
        .platform
        .state
        .borrow()
        .events
        .iter()
        .any(|event| event == "forget-affinity:7"));
}

#[test]
fn external_cpu_sets_break_is_relinquished_without_overwrite() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(0b0010),
                },
                7,
            ),
            true,
        )
        .unwrap();
    coordinator
        .platform
        .state
        .borrow_mut()
        .processes
        .get_mut(&42)
        .unwrap()
        .cpu_sets = vec![103];

    let summary = coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
    assert!(summary.failures.is_empty());
    assert!(summary.restored_owners.is_empty());
    assert_eq!(process_state(&coordinator).cpu_sets, vec![103]);
    assert!(coordinator.managed_cpu_sets.is_empty());
    assert!(coordinator
        .platform
        .state
        .borrow()
        .events
        .iter()
        .any(|event| event == "forget-cpu-sets:7"));
}

#[test]
fn failed_release_is_retried_without_reapplying_a_claim() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(0b0010),
                },
                7,
            ),
            true,
        )
        .unwrap();
    {
        let mut state = coordinator.platform.state.borrow_mut();
        state.processes.get_mut(&42).unwrap().cpu_sets = vec![103];
        state.fail_next_cpu_sets_relinquish = true;
    }

    let release = coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
    assert_eq!(release.failures.len(), 1);
    assert!(coordinator.has_pending_reconciliation());
    assert!(!coordinator.has_pending_immediate_reconciliation());
    assert_eq!(process_state(&coordinator).cpu_sets, vec![103]);

    coordinator
        .platform
        .state
        .borrow_mut()
        .fail_next_cpu_sets_relinquish = true;
    let failed_retry = coordinator.reconcile_pending(Ok(&test_topology()), true, true);
    assert!(failed_retry.applications.is_empty());
    assert!(failed_retry.failures.is_empty());
    assert!(failed_retry.releases.failures.is_empty());
    assert!(coordinator.has_pending_reconciliation());

    let retry = coordinator.reconcile_pending(Ok(&test_topology()), true, true);
    assert!(retry.applications.is_empty());
    assert!(retry.failures.is_empty());
    assert!(retry.releases.failures.is_empty());
    assert_eq!(process_state(&coordinator).cpu_sets, vec![103]);
    assert!(!coordinator.has_managed_state());
    assert!(!coordinator.has_pending_reconciliation());
}

#[test]
fn apply_policy_claim_forwards_cross_session_setting() {
    let mut coordinator = coordinator();
    coordinator
        .platform
        .state
        .borrow_mut()
        .reject_disallowed_cross_session_open = true;

    let result = coordinator.apply_policy_claim(
        &mut CpuSetInventory::default(),
        Ok(&test_topology()),
        claim(
            ControlOwner::ProcessorAffinityHard,
            CpuAllocationRequest::HardAffinity {
                selection: test_selection(0b0100),
            },
            7,
        ),
        false,
    );
    assert!(matches!(
        result,
        Err(CpuAllocationApplyError::Target(
            ProcessControlError::AccessDenied(_)
        ))
    ));
    assert_eq!(process_state(&coordinator).affinity, 0b1111);
    assert!(coordinator.managed_affinity.is_empty());
    assert_eq!(
        coordinator.platform.state.borrow().open_cross_session_flags,
        vec![false]
    );
}

#[test]
fn failed_pending_handoff_restores_outgoing_state_without_retry_spam() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::AdaptiveEngine,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0011),
                },
                7,
            ),
            true,
        )
        .unwrap();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(0b0010),
                },
                7,
            ),
            true,
        )
        .unwrap();
    coordinator
        .platform
        .state
        .borrow_mut()
        .reject_disallowed_cross_session_open = true;

    coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
    let reconciliation = coordinator.reconcile_pending(Ok(&test_topology()), false, true);
    assert_eq!(reconciliation.failures.len(), 1);
    assert_eq!(
        reconciliation.failures[0].owner,
        ControlOwner::AdaptiveEngine
    );
    assert_eq!(
        reconciliation.releases.restored_owners,
        vec![ControlOwner::CpuSetsSoft]
    );
    let state = process_state(&coordinator);
    assert!(state.cpu_sets.is_empty());
    assert_eq!(state.affinity, 0b1111);
    assert!(!coordinator.has_managed_state());
    assert!(!coordinator.has_pending_reconciliation());
    let open_count = coordinator
        .platform
        .state
        .borrow()
        .open_cross_session_flags
        .len();
    assert_eq!(
        &coordinator.platform.state.borrow().open_cross_session_flags[open_count - 2..],
        &[false, true]
    );

    let empty = coordinator.reconcile_pending(Ok(&test_topology()), false, true);
    assert!(empty.applications.is_empty());
    assert!(empty.failures.is_empty());
    assert_eq!(
        coordinator
            .platform
            .state
            .borrow()
            .open_cross_session_flags
            .len(),
        open_count
    );
}

#[test]
fn pid_reuse_relinquishes_the_old_identity_before_applying_the_new_process() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::ProcessorAffinityHard,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0011),
                },
                7,
            ),
            true,
        )
        .unwrap();
    coordinator.platform.state.borrow_mut().processes.insert(
        42,
        FakeProcessState {
            identity: identity(42, 8),
            affinity: 0b1110,
            system_affinity: 0b1111,
            cpu_sets: Vec::new(),
        },
    );

    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::ProcessorAffinityHard,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0100),
                },
                8,
            ),
            true,
        )
        .unwrap();
    assert_eq!(process_state(&coordinator).affinity, 0b0100);
    assert_eq!(coordinator.managed_affinity.len(), 1);
    assert_eq!(
        coordinator
            .managed_affinity
            .keys()
            .next()
            .unwrap()
            .creation_time,
        8
    );
    assert!(coordinator
        .platform
        .state
        .borrow()
        .events
        .iter()
        .any(|event| event == "forget-affinity:7"));
}

#[test]
fn commit_failure_retains_the_verified_affinity_and_original_baseline() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::ProcessorAffinityHard,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0011),
                },
                7,
            ),
            true,
        )
        .unwrap();
    coordinator.platform.state.borrow_mut().events.clear();
    coordinator.platform.state.borrow_mut().fail_next_commit = true;
    let result = coordinator.apply_policy_claim(
        &mut CpuSetInventory::default(),
        Ok(&test_topology()),
        claim(
            ControlOwner::ProcessorAffinityHard,
            CpuAllocationRequest::HardAffinity {
                selection: test_selection(0b0101),
            },
            7,
        ),
        true,
    );
    assert!(result.is_err());
    assert_eq!(process_state(&coordinator).affinity, 0b0101);
    let managed = &coordinator.managed_affinity[&identity(42, 7)];
    assert_eq!(managed.baseline, 0b1111);
    assert_eq!(managed.expected, 0b0101);
    assert!(!coordinator
        .platform
        .state
        .borrow()
        .events
        .iter()
        .any(|event| event == "forget-affinity:7"));
    assert_eq!(
        coordinator
            .platform
            .state
            .borrow()
            .events
            .iter()
            .filter(|event| event.starts_with("apply-affinity:"))
            .count(),
        1
    );
    coordinator.shutdown().unwrap();
    assert_eq!(process_state(&coordinator).affinity, 0b1111);
}

#[test]
fn commit_failure_retains_the_verified_cpu_sets_and_original_baseline() {
    let mut coordinator = coordinator();
    for (mask, fail) in [(0b0011, false), (0b0101, true)] {
        if fail {
            let mut state = coordinator.platform.state.borrow_mut();
            state.events.clear();
            state.fail_next_commit = true;
        }
        let result = coordinator.apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(mask),
                },
                7,
            ),
            true,
        );
        assert_eq!(result.is_err(), fail);
    }
    assert_eq!(process_state(&coordinator).cpu_sets, [100, 102]);
    let managed = &coordinator.managed_cpu_sets[&identity(42, 7)];
    assert!(managed.baseline.is_empty());
    assert_eq!(managed.expected, [100, 102]);
    assert!(!coordinator
        .platform
        .state
        .borrow()
        .events
        .iter()
        .any(|event| event.starts_with("forget-cpu-sets:")));
    assert_eq!(
        coordinator
            .platform
            .state
            .borrow()
            .events
            .iter()
            .filter(|event| event.starts_with("apply-cpu-sets:"))
            .count(),
        1
    );
    coordinator.shutdown().unwrap();
    assert!(process_state(&coordinator).cpu_sets.is_empty());
}

#[test]
fn uncertain_compensation_keeps_the_intent_pending_and_the_first_baseline() {
    for soft in [false, true] {
        let mut coordinator = coordinator();
        let owner = if soft {
            ControlOwner::CpuSetsSoft
        } else {
            ControlOwner::ProcessorAffinityHard
        };
        let request = |mask| {
            if soft {
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(mask),
                }
            } else {
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(mask),
                }
            }
        };
        coordinator
            .apply_policy_claim(
                &mut CpuSetInventory::default(),
                Ok(&test_topology()),
                claim(owner, request(3), 7),
                true,
            )
            .unwrap();
        {
            let mut state = coordinator.platform.state.borrow_mut();
            state.events.clear();
            // Initial observation succeeds; both the target and compensation readbacks fail.
            let failures = VecDeque::from([false, true, true]);
            if soft {
                state.cpu_sets_query_failures = failures;
            } else {
                state.affinity_query_failures = failures;
            }
        }
        assert!(coordinator
            .apply_policy_claim(
                &mut CpuSetInventory::default(),
                Ok(&test_topology()),
                claim(owner, request(5), 7),
                true
            )
            .is_err());
        if soft {
            assert_eq!(process_state(&coordinator).cpu_sets, [100, 101]);
            let managed = &coordinator.managed_cpu_sets[&identity(42, 7)];
            assert!(managed.baseline.is_empty());
            assert_eq!(managed.expected, [100, 102]);
        } else {
            assert_eq!(process_state(&coordinator).affinity, 3);
            let managed = &coordinator.managed_affinity[&identity(42, 7)];
            assert_eq!(managed.baseline, 15);
            assert_eq!(managed.expected, 5);
        }
        let state = coordinator.platform.state.borrow();
        assert_eq!(
            state
                .events
                .iter()
                .filter(|event| *event == "retain-pending")
                .count(),
            1
        );
        assert!(!state
            .events
            .iter()
            .any(|event| event == "commit" || event.starts_with("forget-")));
    }
}

#[test]
fn invalid_affinity_restore_keeps_ownership_until_a_successful_retry() {
    let error = ProcessOperationError::Failed {
        operation: "restore affinity",
        code: 87, // ERROR_INVALID_PARAMETER from a retained process-handle operation.
    };

    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::ProcessorAffinityHard,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0011),
                },
                7,
            ),
            true,
        )
        .unwrap();
    {
        let mut state = coordinator.platform.state.borrow_mut();
        state.events.clear();
        state.next_affinity_error = Some(map_cpu_allocation_error(error));
    }

    let release = coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
    assert_eq!(release.failures.len(), 1);
    assert!(matches!(
        release.failures[0].error,
        ProcessControlError::Failed(_)
    ));
    let managed = &coordinator.managed_affinity[&identity(42, 7)];
    assert_eq!(managed.baseline, 0b1111);
    assert_eq!(managed.expected, 0b0011);
    assert_eq!(process_state(&coordinator).affinity, 0b0011);
    assert!(!coordinator
        .platform
        .state
        .borrow()
        .events
        .iter()
        .any(|event| event.starts_with("forget-affinity:")));

    let retry = coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
    assert!(retry.failures.is_empty());
    assert_eq!(process_state(&coordinator).affinity, 0b1111);
    assert!(!coordinator.has_managed_state());
}

#[test]
fn shutdown_keeps_confirmed_exit_when_reopening_is_denied() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(0b0010),
                },
                7,
            ),
            true,
        )
        .unwrap();
    coordinator.platform.state.borrow_mut().exit_on_apply = true;
    assert!(coordinator.shutdown().is_ok());
    assert!(coordinator.managed_cpu_sets.is_empty());
}

#[test]
fn release_commit_failure_keeps_the_cpu_sets_baseline_and_relinquishes() {
    let mut coordinator = coordinator();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(0b0010),
                },
                7,
            ),
            true,
        )
        .unwrap();
    coordinator.platform.state.borrow_mut().fail_next_commit = true;

    let summary = coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
    assert!(summary.failures.is_empty());
    assert_eq!(summary.restored_owners, vec![ControlOwner::CpuSetsSoft]);
    assert!(process_state(&coordinator).cpu_sets.is_empty());
    assert!(coordinator.managed_cpu_sets.is_empty());
    assert!(coordinator
        .platform
        .state
        .borrow()
        .events
        .iter()
        .any(|event| event == "forget-cpu-sets:7"));
}

#[test]
fn shutdown_restores_properties_in_reverse_apply_order() {
    let mut coordinator = coordinator();
    coordinator.platform.state.borrow_mut().processes.insert(
        43,
        FakeProcessState {
            identity: identity(43, 9),
            affinity: 0b1111,
            system_affinity: 0b1111,
            cpu_sets: Vec::new(),
        },
    );
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(
                ControlOwner::ProcessorAffinityHard,
                CpuAllocationRequest::HardAffinity {
                    selection: test_selection(0b0011),
                },
                7,
            ),
            true,
        )
        .unwrap();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim_for(
                43,
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(0b0010),
                },
                9,
            ),
            true,
        )
        .unwrap();
    coordinator.platform.state.borrow_mut().events.clear();

    coordinator.shutdown().unwrap();
    let state = coordinator.platform.state.borrow();
    let cpu_sets_release = state
        .events
        .iter()
        .position(|event| event.starts_with("begin-cpu-sets:"))
        .unwrap();
    let affinity_release = state
        .events
        .iter()
        .position(|event| event.starts_with("begin-affinity:"))
        .unwrap();
    assert!(cpu_sets_release < affinity_release);
    assert!(state.processes[&43].cpu_sets.is_empty());
    assert_eq!(state.processes[&42].affinity, 0b1111);
}

#[test]
fn target_affinity_mask_preserves_group_zero() {
    assert_eq!(target_affinity_mask(0b1110, 0b0110), None);
    assert_eq!(target_affinity_mask(0b1000, 0b0111), None);
    assert!(Path::new(r"C:\Apps\worker.exe").is_absolute());
}

#[test]
#[ignore = "modifies and restores a disposable Windows process; run in explicit integration QA"]
#[allow(unknown_lints, clippy::manual_isolate_lowest_one)]
fn cpu_allocation_live_apply_and_clean_release() -> Result<(), String> {
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
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not start disposable process: {error}"))?;
    let child = DisposableProcess(child);
    let deadline = Instant::now() + Duration::from_secs(2);
    let action_target = loop {
        match crate::foreground::capture_process_action_target(child.0.id(), &executable_path, true)
        {
            Ok(target) => break target,
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error.to_string()),
        }
    };
    let target = ProcessControlTarget::from_action_target(&action_target);
    let mut coordinator = CpuAllocationCoordinator::default();

    let (_, process) = coordinator
        .platform
        .open(&target, true)
        .map_err(|error| error.to_string())?;
    let (baseline_affinity, system_affinity) = coordinator
        .platform
        .query_affinity(&process)
        .map_err(|error| error.to_string())?;
    let available_affinity = baseline_affinity & system_affinity;
    if available_affinity.count_ones() > 1 {
        let expected_affinity = available_affinity & available_affinity.wrapping_neg();
        coordinator
            .apply_policy_claim(
                &mut CpuSetInventory::default(),
                Ok(&test_topology()),
                CpuAllocationClaim {
                    target: target.clone(),
                    owner: ControlOwner::ProcessorAffinityHard,
                    request: CpuAllocationRequest::HardAffinity {
                        selection: test_selection(expected_affinity as u64),
                    },
                },
                true,
            )
            .map_err(|error| error.to_string())?;
        let (_, process) = coordinator
            .platform
            .open(&target, true)
            .map_err(|error| error.to_string())?;
        assert_eq!(
            coordinator
                .platform
                .query_affinity(&process)
                .map_err(|error| error.to_string())?
                .0,
            expected_affinity
        );
        let summary = coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
        if let Some(failure) = summary.failures.first() {
            return Err(failure.error.to_string());
        }
        let (_, process) = coordinator
            .platform
            .open(&target, true)
            .map_err(|error| error.to_string())?;
        assert_eq!(
            coordinator
                .platform
                .query_affinity(&process)
                .map_err(|error| error.to_string())?
                .0,
            baseline_affinity
        );
    }

    let (_, process) = coordinator
        .platform
        .open(&target, true)
        .map_err(|error| error.to_string())?;
    let mut baseline_cpu_sets = coordinator
        .platform
        .query_cpu_sets(&process)
        .map_err(|error| error.to_string())?;
    normalize_cpu_set_ids(&mut baseline_cpu_sets);
    let soft_target = (0..64).find_map(|bit| {
        let mut ids = CpuSetInventory::default()
            .ids(&mut coordinator.platform, 1_u64 << bit)
            .ok()?;
        normalize_cpu_set_ids(&mut ids);
        (!ids.is_empty() && ids != baseline_cpu_sets).then_some((1_u64 << bit, ids))
    });
    if let Some((logical_processor_mask, expected_cpu_sets)) = soft_target {
        coordinator
            .apply_policy_claim(
                &mut CpuSetInventory::default(),
                Ok(&test_topology()),
                CpuAllocationClaim {
                    target: target.clone(),
                    owner: ControlOwner::CpuSetsSoft,
                    request: CpuAllocationRequest::SoftCpuSets {
                        selection: test_selection(logical_processor_mask),
                    },
                },
                true,
            )
            .map_err(|error| error.to_string())?;
        let (_, process) = coordinator
            .platform
            .open(&target, true)
            .map_err(|error| error.to_string())?;
        let mut observed = coordinator
            .platform
            .query_cpu_sets(&process)
            .map_err(|error| error.to_string())?;
        normalize_cpu_set_ids(&mut observed);
        assert_eq!(observed, expected_cpu_sets);
        let summary = coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
        if let Some(failure) = summary.failures.first() {
            return Err(failure.error.to_string());
        }
        let (_, process) = coordinator
            .platform
            .open(&target, true)
            .map_err(|error| error.to_string())?;
        let mut observed = coordinator
            .platform
            .query_cpu_sets(&process)
            .map_err(|error| error.to_string())?;
        normalize_cpu_set_ids(&mut observed);
        assert_eq!(observed, baseline_cpu_sets);
    }
    Ok(())
}
