use super::*;
use crate::foreground::ProcessInfo;

fn reduce(processors_to_remove: u8, preserve_custom_affinity: bool) -> CpuAllocationRequest {
    CpuAllocationRequest::ReduceAffinity {
        selection: test_selection(0b1111),
        processors_to_remove,
        preserve_custom_affinity,
    }
}

fn apply(
    coordinator: &mut CpuAllocationCoordinator<FakePlatform>,
    owner: ControlOwner,
    request: CpuAllocationRequest,
) -> CpuAllocationApplyOutcome {
    coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(owner, request, 7),
            true,
        )
        .unwrap()
}

fn add_affinity_child(
    coordinator: &CpuAllocationCoordinator<FakePlatform>,
    creation_time: u64,
) -> ProcessInfo {
    let identity = identity(43, creation_time);
    let observation = ProcessInfo {
        id: identity.id,
        parent_id: Some(42),
        name: identity.name.clone(),
        creation_time: Some(identity.creation_time),
        session_id: identity.session_id,
        image_path: Some(identity.executable_path.clone()),
        is_service_account: Some(false),
        is_critical: Some(false),
        can_set_information: true,
        user_name: None,
    };
    coordinator.platform.state.borrow_mut().processes.insert(
        identity.id,
        FakeProcessState {
            identity,
            affinity: 0b0011,
            system_affinity: 0b1111,
            cpu_sets: Vec::new(),
        },
    );
    observation
}

fn apply_parent_narrowing(coordinator: &mut CpuAllocationCoordinator<FakePlatform>) {
    assert_eq!(
        apply(
            coordinator,
            ControlOwner::AdaptiveEngine,
            CpuAllocationRequest::HardAffinity {
                selection: test_selection(0b0011),
            },
        ),
        CpuAllocationApplyOutcome::Applied
    );
}

#[test]
fn inherited_affinity_repair_journals_parent_baseline_and_ends_child_ownership() {
    let mut coordinator = coordinator();
    let owner = ControlOwner::AdaptiveEngine;
    coordinator.set_restore_child_affinity(true);
    apply_parent_narrowing(&mut coordinator);
    let child = add_affinity_child(
        &coordinator,
        child_affinity::windows_filetime_now().unwrap() + 1,
    );
    assert!(coordinator.has_adaptive_child_affinity_candidates());
    coordinator.platform.state.borrow_mut().events.clear();

    let restore = coordinator.restore_adaptive_child_affinity_with(
        std::slice::from_ref(&child),
        |_, _| true,
        true,
        |parent, child, retained| Ok((parent, child, *retained) == (42, 43, 43)),
    );
    assert!(restore.failures.is_empty());
    assert_eq!(restore.restored_owners, vec![owner]);
    {
        let mut state = coordinator.platform.state.borrow_mut();
        assert_eq!(state.processes[&42].affinity, 0b0011);
        assert_eq!(state.processes[&43].affinity, 0b1111);
        assert_eq!(
            state.events,
            [
                "begin-affinity:0xf->0x3",
                "commit",
                "begin-affinity:0x3->0xf",
                "apply-affinity:0xf",
                "commit",
            ]
        );
        state.processes.get_mut(&43).unwrap().affinity = 0b0011;
    }
    assert!(coordinator
        .restore_adaptive_child_affinity_with(&[child], |_, _| true, true, |_, _, _| Ok(true))
        .restored_owners
        .is_empty());
    assert!(coordinator.shutdown().is_ok());
    let state = coordinator.platform.state.borrow();
    assert_eq!(state.processes[&42].affinity, 0b1111);
    assert_eq!(state.processes[&43].affinity, 0b0011);
}

#[test]
fn inherited_affinity_repair_preserves_old_children_and_rejects_changed_ancestry() {
    let mut coordinator = coordinator();
    coordinator.set_restore_child_affinity(true);
    apply_parent_narrowing(&mut coordinator);
    let before_apply = add_affinity_child(&coordinator, 8);
    let restore = coordinator.restore_adaptive_child_affinity_with(
        &[before_apply],
        |_, _| unreachable!("old child reached the eligibility check"),
        true,
        |_, _, _| unreachable!("old child reached native ancestry"),
    );
    assert!(restore.failures.is_empty());
    assert!(restore.restored_owners.is_empty());

    let child = add_affinity_child(
        &coordinator,
        child_affinity::windows_filetime_now().unwrap() + 1,
    );
    let restore = coordinator.restore_adaptive_child_affinity_with(
        std::slice::from_ref(&child),
        |_, _| true,
        true,
        |_, _, _| Ok(false),
    );
    assert!(restore.failures.is_empty());
    assert!(restore.restored_owners.is_empty());
    assert_eq!(
        coordinator.platform.state.borrow().processes[&43].affinity,
        0b0011
    );
    let restore = coordinator.restore_adaptive_child_affinity_with(
        &[child],
        |_, _| true,
        true,
        |_, _, _| unreachable!("a previous rejection is final for this narrowed parent"),
    );
    assert!(restore.restored_owners.is_empty());
}

#[test]
fn inherited_affinity_repair_checks_for_parent_drift_after_confirming_ancestry() {
    let mut coordinator = coordinator();
    coordinator.set_restore_child_affinity(true);
    apply_parent_narrowing(&mut coordinator);
    let child = add_affinity_child(
        &coordinator,
        child_affinity::windows_filetime_now().unwrap() + 1,
    );
    let state = Rc::clone(&coordinator.platform.state);
    let restore = coordinator.restore_adaptive_child_affinity_with(
        &[child],
        |_, _| true,
        true,
        |_, _, _| {
            state.borrow_mut().processes.get_mut(&42).unwrap().affinity = 0b1100;
            Ok(true)
        },
    );
    assert!(restore.failures.is_empty());
    assert!(restore.restored_owners.is_empty());
    assert!(!coordinator.has_adaptive_child_affinity_candidates());
    assert_eq!(state.borrow().processes[&43].affinity, 0b0011);
}

#[test]
fn inherited_affinity_repair_can_arm_during_unchanged_parent_restraint() {
    let mut coordinator = coordinator();
    apply_parent_narrowing(&mut coordinator);
    assert!(!coordinator.has_adaptive_child_affinity_candidates());
    let old_child = add_affinity_child(
        &coordinator,
        child_affinity::windows_filetime_now().unwrap(),
    );

    coordinator.set_restore_child_affinity(true);
    assert!(!coordinator.has_adaptive_child_affinity_candidates());
    assert_eq!(
        apply(
            &mut coordinator,
            ControlOwner::AdaptiveEngine,
            CpuAllocationRequest::HardAffinity {
                selection: test_selection(0b0011),
            },
        ),
        CpuAllocationApplyOutcome::Unchanged
    );
    assert!(coordinator.has_adaptive_child_affinity_candidates());
    let restore = coordinator.restore_adaptive_child_affinity_with(
        &[old_child],
        |_, _| unreachable!("a child born before enabling was considered"),
        true,
        |_, _, _| Ok(true),
    );
    assert!(restore.restored_owners.is_empty());
    assert_eq!(
        coordinator.platform.state.borrow().processes[&43].affinity,
        0b0011
    );

    let new_child = add_affinity_child(
        &coordinator,
        child_affinity::windows_filetime_now().unwrap() + 1,
    );
    let restore = coordinator.restore_adaptive_child_affinity_with(
        &[new_child],
        |_, _| true,
        true,
        |_, _, _| Ok(true),
    );
    assert!(restore.failures.is_empty());
    assert_eq!(restore.restored_owners, vec![ControlOwner::AdaptiveEngine]);
}

#[test]
fn inherited_affinity_uncertain_adoption_retries_after_discovery_is_disabled() {
    let mut coordinator = coordinator();
    coordinator.set_restore_child_affinity(true);
    apply_parent_narrowing(&mut coordinator);
    let child = add_affinity_child(
        &coordinator,
        child_affinity::windows_filetime_now().unwrap() + 1,
    );
    coordinator.platform.state.borrow_mut().fail_next_commit = true;
    let restore = coordinator.restore_adaptive_child_affinity_with(
        &[child],
        |_, _| true,
        true,
        |_, _, _| Ok(true),
    );
    assert!(restore.restored_owners.is_empty());
    assert_eq!(restore.failures.len(), 1);
    assert_eq!(
        coordinator.platform.state.borrow().processes[&43].affinity,
        0b0011
    );
    coordinator.set_restore_child_affinity(false);
    let retry = coordinator.reconcile_pending(Ok(&test_topology()), true, true);
    assert!(retry.releases.failures.is_empty());
    assert_eq!(
        retry.releases.restored_owners,
        vec![ControlOwner::AdaptiveEngine]
    );
    assert_eq!(
        coordinator.platform.state.borrow().processes[&43].affinity,
        0b1111
    );
}

#[test]
fn reduction_uses_captured_affinity_and_restores_full_baseline() {
    let mut coordinator = coordinator();
    let owner = ControlOwner::AdaptiveEngine;
    let baseline: usize = 0b0111;
    coordinator
        .platform
        .state
        .borrow_mut()
        .processes
        .get_mut(&42)
        .unwrap()
        .affinity = baseline;

    let request = reduce(63, false);
    assert_eq!(
        apply(&mut coordinator, owner, request.clone()),
        CpuAllocationApplyOutcome::Applied
    );
    let reduced = process_state(&coordinator).affinity;
    assert_eq!(reduced.count_ones(), 1);
    assert_eq!(reduced & baseline, reduced);
    assert_eq!(
        apply(&mut coordinator, owner, request),
        CpuAllocationApplyOutcome::Unchanged
    );
    assert_eq!(process_state(&coordinator).affinity, reduced);

    assert_eq!(
        apply(&mut coordinator, owner, reduce(1, false)),
        CpuAllocationApplyOutcome::Applied
    );
    let changed = process_state(&coordinator).affinity;
    assert_eq!(changed.count_ones(), 2);
    assert_eq!(changed & baseline, changed);
    assert_eq!(changed & reduced, reduced);
    let released = coordinator.release_all_policy(owner);
    assert!(released.failures.is_empty());
    assert_eq!(released.restored_owners, [owner]);
    assert_eq!(process_state(&coordinator).affinity, baseline);
}

#[test]
fn reduction_relinquishes_instead_of_overwriting_live_affinity_drift() {
    let mut coordinator = coordinator();
    let owner = ControlOwner::AdaptiveEngine;
    let request = reduce(1, false);
    assert_eq!(
        apply(&mut coordinator, owner, request.clone()),
        CpuAllocationApplyOutcome::Applied
    );
    coordinator
        .platform
        .state
        .borrow_mut()
        .processes
        .get_mut(&42)
        .unwrap()
        .affinity = 0b1100;
    assert_eq!(
        apply(&mut coordinator, owner, request),
        CpuAllocationApplyOutcome::NoUsableTarget
    );
    assert_eq!(process_state(&coordinator).affinity, 0b1100);
    assert!(coordinator.managed_affinity.is_empty());
    assert!(coordinator.release_all_policy(owner).failures.is_empty());
    assert_eq!(process_state(&coordinator).affinity, 0b1100);
}

#[test]
fn guarded_affinity_yields_to_preexisting_custom_placement() {
    for (request, affinity, cpu_sets) in [
        (
            CpuAllocationRequest::PreserveHardAffinity {
                selection: test_selection(1),
            },
            0b0011,
            vec![],
        ),
        (
            CpuAllocationRequest::PreserveHardAffinity {
                selection: test_selection(1),
            },
            0b1111,
            vec![103],
        ),
        (reduce(1, true), 0b0011, vec![]),
        (reduce(1, false), 0b1111, vec![103]),
        (reduce(1, false), 0b0001, vec![]),
    ] {
        let mut coordinator = coordinator();
        {
            let mut state = coordinator.platform.state.borrow_mut();
            let target = state.processes.get_mut(&42).unwrap();
            target.affinity = affinity;
            target.cpu_sets = cpu_sets.clone();
        }
        assert_eq!(
            apply(&mut coordinator, ControlOwner::AdaptiveEngine, request),
            CpuAllocationApplyOutcome::NoUsableTarget
        );
        let observed = process_state(&coordinator);
        assert_eq!(observed.affinity, affinity);
        assert_eq!(observed.cpu_sets, cpu_sets);
        assert!(!coordinator.has_managed_state());
    }
}

#[test]
fn guarded_exact_restores_its_mask_when_external_cpu_sets_are_added() {
    let mut coordinator = coordinator();
    let owner = ControlOwner::AdaptiveEngine;
    let request = CpuAllocationRequest::PreserveHardAffinity {
        selection: test_selection(0b0011),
    };
    assert_eq!(
        apply(&mut coordinator, owner, request.clone()),
        CpuAllocationApplyOutcome::Applied
    );
    assert_eq!(
        apply(&mut coordinator, owner, request.clone()),
        CpuAllocationApplyOutcome::Unchanged
    );
    assert_eq!(process_state(&coordinator).affinity, 0b0011);

    coordinator
        .platform
        .state
        .borrow_mut()
        .processes
        .get_mut(&42)
        .unwrap()
        .cpu_sets = vec![103];
    assert_eq!(
        apply(&mut coordinator, owner, request),
        CpuAllocationApplyOutcome::NoUsableTarget
    );
    let observed = process_state(&coordinator);
    assert_eq!(observed.affinity, 0b1111);
    assert_eq!(observed.cpu_sets, [103]);
    assert!(!coordinator.has_managed_state());
}

#[test]
fn guarded_handoff_restores_outgoing_cpu_sets_and_external_affinity() {
    let mut coordinator = coordinator();
    let higher_owner = ControlOwner::CpuSetsSoft;
    let adaptive = ControlOwner::AdaptiveEngine;
    let affinity = 0b0101;
    {
        let mut state = coordinator.platform.state.borrow_mut();
        let target = state.processes.get_mut(&42).unwrap();
        target.affinity = affinity;
        target.cpu_sets = vec![102];
    }
    let preserve = CpuAllocationRequest::PreserveHardAffinity {
        selection: test_selection(1),
    };
    assert_eq!(
        apply(
            &mut coordinator,
            higher_owner,
            CpuAllocationRequest::SoftCpuSets {
                selection: test_selection(0b1000)
            }
        ),
        CpuAllocationApplyOutcome::Applied
    );
    assert_eq!(
        apply(&mut coordinator, adaptive, preserve),
        CpuAllocationApplyOutcome::Shadowed
    );
    assert_eq!(process_state(&coordinator).cpu_sets, [103]);

    assert!(coordinator
        .release_all_policy(higher_owner)
        .failures
        .is_empty());
    let handoff = coordinator.reconcile_pending(Ok(&test_topology()), true, false);
    assert!(handoff.failures.is_empty());
    assert!(handoff.applications.is_empty());
    assert_eq!(process_state(&coordinator).affinity, affinity);
    assert_eq!(process_state(&coordinator).cpu_sets, [102]);
    assert!(!coordinator.has_managed_state());
}

#[test]
fn reduction_after_hard_affinity_handoff_uses_original_baseline() {
    let mut coordinator = coordinator();
    let higher_owner = ControlOwner::ProcessorAffinityHard;
    let adaptive = ControlOwner::AdaptiveEngine;
    assert_eq!(
        apply(
            &mut coordinator,
            higher_owner,
            CpuAllocationRequest::HardAffinity {
                selection: test_selection(1)
            }
        ),
        CpuAllocationApplyOutcome::Applied
    );
    assert_eq!(
        apply(&mut coordinator, adaptive, reduce(1, true)),
        CpuAllocationApplyOutcome::Shadowed
    );

    assert!(coordinator
        .release_all_policy(higher_owner)
        .failures
        .is_empty());
    let handoff = coordinator.reconcile_pending(Ok(&test_topology()), true, false);
    assert!(handoff.failures.is_empty());
    assert_eq!(handoff.applications.len(), 1);
    assert_eq!(handoff.applications[0].owner, adaptive);
    assert_eq!(process_state(&coordinator).affinity.count_ones(), 3);

    assert_eq!(
        coordinator.release_all_policy(adaptive).restored_owners,
        [adaptive]
    );
    assert_eq!(process_state(&coordinator).affinity, 0b1111);
}

#[test]
fn guard_read_error_withdraws_previous_assignment_and_retries_failed_restore() {
    let mut coordinator = coordinator();
    let owner = ControlOwner::AdaptiveEngine;
    assert_eq!(
        apply(
            &mut coordinator,
            owner,
            CpuAllocationRequest::HardAffinity {
                selection: test_selection(0b0011)
            }
        ),
        CpuAllocationApplyOutcome::Applied
    );
    {
        let mut state = coordinator.platform.state.borrow_mut();
        state.cpu_sets_query_failures.push_back(true);
        state.affinity_apply_failures_remaining = 1;
    }
    let request = CpuAllocationRequest::PreserveHardAffinity {
        selection: test_selection(0b0110),
    };
    let error = coordinator
        .apply_policy_claim(
            &mut CpuSetInventory::default(),
            Ok(&test_topology()),
            claim(owner, request, 7),
            true,
        )
        .unwrap_err();
    assert!(error.to_string().contains("injected CPU Sets read failure"));
    assert!(error.to_string().contains("Restoration remains pending"));
    assert_eq!(process_state(&coordinator).affinity, 0b0011);
    assert!(coordinator.has_pending_release_retry());

    let retry = coordinator.reconcile_pending(Ok(&test_topology()), true, true);
    assert!(retry.applications.is_empty());
    assert_eq!(retry.releases.restored_owners, [owner]);
    assert_eq!(process_state(&coordinator).affinity, 0b1111);
    assert!(!coordinator.has_managed_state());
}

#[test]
fn guard_read_error_during_handoff_restores_outgoing_higher_owner_mask() {
    let mut coordinator = coordinator();
    let higher_owner = ControlOwner::ProcessorAffinityHard;
    let adaptive = ControlOwner::AdaptiveEngine;
    assert_eq!(
        apply(
            &mut coordinator,
            higher_owner,
            CpuAllocationRequest::HardAffinity {
                selection: test_selection(0b0011)
            }
        ),
        CpuAllocationApplyOutcome::Applied
    );
    assert_eq!(
        apply(&mut coordinator, adaptive, reduce(1, true)),
        CpuAllocationApplyOutcome::Shadowed
    );
    assert!(coordinator
        .release_all_policy(higher_owner)
        .failures
        .is_empty());
    coordinator
        .platform
        .state
        .borrow_mut()
        .cpu_sets_query_failures
        .push_back(true);

    let handoff = coordinator.reconcile_pending(Ok(&test_topology()), true, false);
    assert!(handoff.applications.is_empty());
    assert_eq!(handoff.failures.len(), 1);
    assert_eq!(handoff.failures[0].owner, adaptive);
    assert_eq!(handoff.releases.restored_owners, [higher_owner]);
    assert_eq!(process_state(&coordinator).affinity, 0b1111);
    assert!(!coordinator.has_managed_state());
}
