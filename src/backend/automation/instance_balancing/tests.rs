use super::*;
use crate::{
    config::CpuAllocationRule,
    control::{cpu_allocation::CpuAllocationRecoveryIntent, process::ProcessIdentity},
    cpu_topology::{test_selection, test_topology},
    platform::windows::cpu_allocation::CpuSetRecord,
};
use std::{cell::RefCell, rc::Rc};

#[test]
fn waiting_detail_preserves_capacity_and_only_its_diagnostic() {
    use InstanceBalancingDetail::{
        InsufficientCapacity, WaitingForInstances, WaitingForStableCohort,
    };
    use InstanceBalancingExclusion::{Diagnostic, ExplicitSelection};

    let cohorts = [
        (InsufficientCapacity, "capacity cohort"),
        (WaitingForStableCohort, "stable cohort"),
        (WaitingForInstances, "empty cohort"),
    ];
    for order in [[0, 1, 2], [2, 1, 0]] {
        let mut snapshot = InstanceBalancingSnapshot {
            detail: Some(WaitingForInstances),
            ..Default::default()
        };
        let mut waiting_exclusions = Vec::new();
        for index in order {
            let (detail, process_name) = &cohorts[index];
            snapshot.record_rule_detail(
                detail.clone(),
                vec![
                    ExplicitSelection {
                        process_name: (*process_name).to_owned(),
                        pid: index as u32 + 1,
                    },
                    Diagnostic((*process_name).to_owned()),
                ],
                &mut waiting_exclusions,
            );
        }
        assert_eq!(snapshot.detail, Some(InsufficientCapacity));
        assert_eq!(waiting_exclusions, [Diagnostic("capacity cohort".into())]);
        assert_eq!(snapshot.exclusions.len(), 3);
        assert!(snapshot.exclusions.iter().zip(order).all(|(row, index)| {
            matches!(row, ExplicitSelection { pid, .. } if *pid == index as u32 + 1)
        }));
    }
}

#[test]
fn explicit_cpu_rules_exclude_matching_instances_without_rejecting_unrelated_paths() {
    let explicit_rule = |path: &str| CpuAllocationRule {
        enabled: true,
        executable_path: path.to_owned(),
        focus_selection: test_selection(1),
        visible_window_selection: test_selection(0),
        background_selection: test_selection(0),
    };
    let mut settings = Settings {
        cpu_sets_soft: CpuAllocationSettings {
            enabled: true,
            rules: vec![explicit_rule(r"C:\Apps\.\other.exe")],
        },
        processor_affinity_hard: CpuAllocationSettings {
            enabled: true,
            rules: vec![],
        },
        ..Default::default()
    };
    let instance = r"C:\Apps\worker.exe";
    let excluded = |settings: &Settings| {
        explicit_selection(&explicit_cpu_allocation_paths(settings, None), instance)
    };
    assert!(!excluded(&settings));

    settings
        .processor_affinity_hard
        .rules
        .push(explicit_rule("C:/Apps/worker.exe"));
    assert!(excluded(&settings));

    settings.processor_affinity_hard.enabled = false;
    assert!(!excluded(&settings));
    settings.cpu_sets_soft.rules.push(explicit_rule(instance));
    assert!(excluded(&settings));
}

#[derive(Default)]
struct State {
    cpu_sets: BTreeMap<u32, Vec<u32>>,
    writes: Vec<(u32, Vec<u32>)>,
    begins: Vec<(u32, Vec<u32>, Vec<u32>)>,
    fail_next_for: Option<u32>,
}

struct Fake(Rc<RefCell<State>>);

struct Intent;

impl CpuAllocationRecoveryIntent for Intent {
    fn commit(self) -> Result<(), String> {
        Ok(())
    }

    fn retain_pending(self) -> Result<(), String> {
        Ok(())
    }
}

impl CpuAllocationPlatform for Fake {
    type Process = u32;
    type RecoveryIntent = Intent;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        _allow_cross_session: bool,
    ) -> Result<(ProcessIdentity, u32), ProcessControlError> {
        if !self.0.borrow().cpu_sets.contains_key(&target.id) {
            return Err(ProcessControlError::ProcessExited);
        }
        Ok((
            ProcessIdentity::new(
                target.id,
                target.name.clone(),
                target.executable_path.clone(),
                target.creation_time,
                None,
            ),
            target.id,
        ))
    }

    fn query_affinity(&mut self, _process: &u32) -> Result<(usize, usize), ProcessControlError> {
        Ok((0b111, 0b111))
    }

    fn validate_group_zero_affinity(&mut self, _process: &u32) -> Result<(), ProcessControlError> {
        Ok(())
    }

    fn query_cpu_sets(&mut self, process: &u32) -> Result<Vec<u32>, ProcessControlError> {
        self.0
            .borrow()
            .cpu_sets
            .get(process)
            .cloned()
            .ok_or(ProcessControlError::ProcessExited)
    }

    fn cpu_set_inventory(&mut self) -> Result<Vec<CpuSetRecord>, ProcessControlError> {
        Ok((0..64)
            .map(|number| CpuSetRecord::Unallocated {
                processor: crate::cpu_topology::ProcessorId { group: 0, number },
                id: 100 + u32::from(number),
            })
            .collect())
    }

    fn begin_affinity_change(
        &mut self,
        _process: &u32,
        _original: usize,
        _expected: usize,
    ) -> Result<Intent, ProcessControlError> {
        panic!("Instance Balancing must not change hard affinity")
    }

    fn begin_cpu_sets_change(
        &mut self,
        process: &u32,
        original: &[u32],
        expected: &[u32],
    ) -> Result<Intent, ProcessControlError> {
        self.0
            .borrow_mut()
            .begins
            .push((*process, original.to_vec(), expected.to_vec()));
        Ok(Intent)
    }

    fn apply_affinity(
        &mut self,
        _process: &u32,
        _affinity: usize,
    ) -> Result<(), ProcessControlError> {
        panic!("Instance Balancing must preserve hard affinity")
    }

    fn apply_cpu_sets(&mut self, process: &u32, ids: &[u32]) -> Result<(), ProcessControlError> {
        let mut state = self.0.borrow_mut();
        state.writes.push((*process, ids.to_vec()));
        if state.fail_next_for == Some(*process) && !ids.is_empty() {
            state.fail_next_for = None;
            return Err(ProcessControlError::Failed(
                "injected CPU Set failure".into(),
            ));
        }
        *state
            .cpu_sets
            .get_mut(process)
            .ok_or(ProcessControlError::ProcessExited)? = ids.to_vec();
        Ok(())
    }

    fn relinquish_affinity(
        &mut self,
        _identity: &ProcessIdentity,
    ) -> Result<(), ProcessControlError> {
        Ok(())
    }

    fn relinquish_cpu_sets(
        &mut self,
        _identity: &ProcessIdentity,
    ) -> Result<(), ProcessControlError> {
        Ok(())
    }
}

fn fixture() -> (
    CpuAllocationCoordinator<Fake>,
    Rc<RefCell<State>>,
    Vec<Assignment>,
) {
    let state = Rc::new(RefCell::new(State {
        cpu_sets: (1..=3).map(|id| (id, Vec::new())).collect(),
        ..Default::default()
    }));
    let assignments = (1..=3)
        .map(|id| Assignment {
            target: ProcessControlTarget::automatic(
                id,
                "worker.exe".into(),
                "C:/Apps/worker.exe".into(),
                100 + u64::from(id),
            ),
            selection: test_selection(1 << (id - 1)),
        })
        .collect();
    (
        CpuAllocationCoordinator::with_platform(Fake(Rc::clone(&state))),
        state,
        assignments,
    )
}

#[test]
fn a_partial_apply_stops_the_group_and_release_restores_original_defaults() {
    let (mut coordinator, state, assignments) = fixture();
    state.borrow_mut().fail_next_for = Some(2);
    let mut authorized = Vec::new();
    let error = apply_plan(
        &mut coordinator,
        &mut CpuSetInventory::default(),
        &test_topology(),
        &assignments,
        1,
        false,
        |target| {
            authorized.push(target.id);
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.outcome, ActionLogResult::Failed);
    assert!(error.reason.contains("injected CPU Set failure"));
    assert_eq!(authorized, [1, 2]);
    assert_eq!(state.borrow().cpu_sets[&1], [100]);
    assert!(state.borrow().writes.iter().all(|(id, _)| *id != 3));
    assert!(state
        .borrow()
        .begins
        .iter()
        .any(|(id, original, expected)| *id == 1 && original.is_empty() && expected == &[100]));

    assert_eq!(
        release(
            &Runtime::default(),
            &mut coordinator,
            &mut ActionLog::default()
        ),
        None
    );
    assert!(state.borrow().cpu_sets.values().all(Vec::is_empty));
    assert!(!coordinator.has_pending_reconciliation());
}

#[test]
fn revoking_context_mid_group_stops_before_the_next_native_change() {
    let (mut coordinator, state, assignments) = fixture();
    let error = apply_plan(
        &mut coordinator,
        &mut CpuSetInventory::default(),
        &test_topology(),
        &assignments,
        1,
        false,
        |target| {
            if target.id == 1 {
                Ok(())
            } else {
                Err("context revoked".into())
            }
        },
    )
    .unwrap_err();
    assert_eq!(error.outcome, ActionLogResult::Skipped);
    assert_eq!(error.reason, "context revoked");
    assert_eq!(state.borrow().writes, [(1, vec![100])]);
    assert_eq!(
        release(
            &Runtime::default(),
            &mut coordinator,
            &mut ActionLog::default()
        ),
        None
    );
    assert_eq!(state.borrow().writes, [(1, vec![100]), (1, vec![])]);
    assert!(state.borrow().cpu_sets.values().all(Vec::is_empty));
    assert!(!coordinator.has_pending_reconciliation());
}

#[test]
fn a_shadowing_explicit_owner_is_not_accepted_or_removed_by_group_cleanup() {
    let (mut coordinator, state, assignments) = fixture();
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            CpuAllocationClaim {
                target: assignments[1].target.clone(),
                owner: ControlOwner::CpuSetsSoft,
                request: CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(0b110),
                },
                balancing_generation: None,
            },
            false,
        )
        .unwrap();
    state.borrow_mut().writes.clear();
    assert!(apply_plan(
        &mut coordinator,
        &mut CpuSetInventory::default(),
        &test_topology(),
        &assignments,
        1,
        false,
        |_| Ok(()),
    )
    .is_err());
    assert!(state.borrow().writes.iter().all(|(id, _)| *id == 1));
    assert_eq!(
        release(
            &Runtime::default(),
            &mut coordinator,
            &mut ActionLog::default()
        ),
        None
    );
    assert!(state.borrow().cpu_sets[&1].is_empty());
    assert_eq!(state.borrow().cpu_sets[&2], [101, 102]);
    assert!(state.borrow().cpu_sets[&3].is_empty());

    assert!(coordinator
        .release_all_policy(ControlOwner::CpuSetsSoft)
        .failures
        .is_empty());
    assert!(state.borrow().cpu_sets.values().all(Vec::is_empty));
    assert!(!coordinator.has_pending_reconciliation());
}
