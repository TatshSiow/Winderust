use super::*;

fn balancing(mask: u64, generation: u64, creation_time: u64) -> CpuAllocationClaim {
    let mut claim = claim(
        ControlOwner::InstanceBalancing,
        CpuAllocationRequest::SoftCpuSets {
            selection: test_selection(mask),
        },
        creation_time,
    );
    claim.balancing_generation = Some(generation);
    claim
}

fn apply(
    coordinator: &mut CpuAllocationCoordinator<FakePlatform>,
    claim: CpuAllocationClaim,
) -> Result<CpuAllocationApplyOutcome, CpuAllocationApplyError> {
    coordinator.apply_policy_claim(
        &mut CpuSetInventory::default(),
        Ok(&test_topology()),
        claim,
        true,
    )
}

fn allowed(
    coordinator: &mut CpuAllocationCoordinator<FakePlatform>,
    generation: u64,
) -> Result<BTreeSet<ProcessorId>, CpuAllocationApplyError> {
    coordinator.instance_balancing_allowed_processors(
        &mut CpuSetInventory::default(),
        &test_topology(),
        &target(42, 7),
        generation,
        true,
    )
}

#[test]
fn planning_uses_confirmed_originals_without_changing_the_current_allocation() {
    let expected = test_selection(0b1111)
        .processors()
        .iter()
        .copied()
        .collect();
    for hard in [false, true] {
        let mut coordinator = coordinator();
        let request = if hard {
            CpuAllocationRequest::HardAffinity {
                selection: test_selection(0b0011),
            }
        } else {
            CpuAllocationRequest::SoftCpuSets {
                selection: test_selection(0b0011),
            }
        };
        apply(
            &mut coordinator,
            claim(ControlOwner::AdaptiveEngine, request, 7),
        )
        .unwrap();
        let before = process_state(&coordinator);
        coordinator.platform.state.borrow_mut().events.clear();
        assert_eq!(allowed(&mut coordinator, 1).unwrap(), expected);
        assert_eq!(process_state(&coordinator).affinity, before.affinity);
        assert_eq!(process_state(&coordinator).cpu_sets, before.cpu_sets);
        assert_eq!(
            coordinator.platform.state.borrow().events,
            ["cpu-set-inventory"]
        );
        apply(&mut coordinator, balancing(0b1100, 1, 7)).unwrap();
        assert_eq!(allowed(&mut coordinator, 1).unwrap(), expected);
        assert_eq!(process_state(&coordinator).cpu_sets, [102, 103]);
    }
}

#[test]
fn planning_returns_individual_unallocated_processors_for_whole_core_intersection() {
    use crate::cpu_topology::{LogicalProcessor, ProcessorGroup};
    let topology = CpuTopology::new(
        (0..4)
            .map(|number| LogicalProcessor {
                id: ProcessorId { group: 0, number },
                core: ProcessorId {
                    group: 0,
                    number: number / 2 * 2,
                },
                efficiency_class: number / 2,
            })
            .collect(),
        vec![ProcessorGroup {
            number: 0,
            active_mask: 0b1111,
        }],
        Vec::new(),
        Vec::new(),
    )
    .unwrap();
    let mut coordinator = coordinator();
    {
        let mut state = coordinator.platform.state.borrow_mut();
        state.processes.get_mut(&42).unwrap().affinity = 0b1101;
        state.cpu_set_entries = Some(
            (0..4)
                .map(|number| {
                    let processor = ProcessorId { group: 0, number };
                    if number == 3 {
                        CpuSetRecord::Allocated {
                            processor,
                            id: 100 + u32::from(number),
                        }
                    } else {
                        CpuSetRecord::Unallocated {
                            processor,
                            id: 100 + u32::from(number),
                        }
                    }
                })
                .collect(),
        );
    }
    let result = coordinator
        .instance_balancing_allowed_processors(
            &mut CpuSetInventory::default(),
            &topology,
            &target(42, 7),
            1,
            true,
        )
        .unwrap();
    assert_eq!(
        result,
        [0, 2].map(|number| ProcessorId { group: 0, number }).into()
    );
    coordinator.platform.state.borrow_mut().cpu_set_entries = Some(
        (0..4)
            .map(|number| CpuSetRecord::Allocated {
                processor: ProcessorId { group: 0, number },
                id: 100 + u32::from(number),
            })
            .collect(),
    );
    assert!(coordinator
        .instance_balancing_allowed_processors(
            &mut CpuSetInventory::default(),
            &topology,
            &target(42, 7),
            1,
            true,
        )
        .unwrap()
        .is_empty());
    assert!(!coordinator.has_managed_state());
}

#[test]
fn planning_observes_drift_and_keeps_the_same_generation_suppressed() {
    for cpu_sets in [false, true] {
        let mut coordinator = coordinator();
        allowed(&mut coordinator, 1).unwrap();
        {
            let mut state = coordinator.platform.state.borrow_mut();
            let process = state.processes.get_mut(&42).unwrap();
            if cpu_sets {
                process.cpu_sets = vec![100];
            } else {
                process.affinity = 0b0011;
            }
        }
        assert_eq!(
            allowed(&mut coordinator, 1),
            Err(CpuAllocationApplyError::Eligibility(
                InstanceBalancingEligibilityError::ExternalChange,
            ))
        );
        {
            let mut state = coordinator.platform.state.borrow_mut();
            let process = state.processes.get_mut(&42).unwrap();
            process.cpu_sets.clear();
            process.affinity = 0b1111;
        }
        assert_eq!(
            allowed(&mut coordinator, 1),
            Err(CpuAllocationApplyError::Eligibility(
                InstanceBalancingEligibilityError::Suppressed,
            ))
        );
        assert!(allowed(&mut coordinator, 2).is_ok());
        assert!(!coordinator.has_managed_state());
    }
}

#[test]
fn unknown_planning_facts_cannot_authorize_or_suppress_a_later_valid_observation() {
    for failure in 0..4 {
        let mut coordinator = coordinator();
        {
            let mut state = coordinator.platform.state.borrow_mut();
            match failure {
                0 => state.affinity_query_failures.push_back(true),
                1 => state.cpu_sets_query_failures.push_back(true),
                2 => state.fail_inventory = true,
                _ => state.deny_open = true,
            }
        }
        let result = allowed(&mut coordinator, 1);
        if failure == 2 {
            assert!(matches!(result, Err(CpuAllocationApplyError::Discovery(_))));
        } else {
            assert!(matches!(result, Err(CpuAllocationApplyError::Target(_))));
        }
        {
            let mut state = coordinator.platform.state.borrow_mut();
            state.fail_inventory = false;
            state.deny_open = false;
            assert!(state
                .events
                .iter()
                .all(|event| event == "cpu-set-inventory"));
        }
        assert!(allowed(&mut coordinator, 1).is_ok());
        assert!(!coordinator.has_managed_state());
    }
}

#[test]
fn exited_unclaimed_planning_members_do_not_accumulate_guard_records() {
    let mut coordinator = coordinator();
    let targets = (100..116).map(|id| target(id, 7)).collect::<Vec<_>>();
    let mut inventory = CpuSetInventory::default();
    for target in &targets {
        coordinator.platform.state.borrow_mut().processes.insert(
            target.id,
            FakeProcessState {
                identity: identity(target.id, target.creation_time),
                affinity: 0b1111,
                system_affinity: 0b1111,
                cpu_sets: Vec::new(),
            },
        );
        coordinator
            .instance_balancing_allowed_processors(
                &mut inventory,
                &test_topology(),
                target,
                1,
                true,
            )
            .unwrap();
    }
    assert_eq!(coordinator.instance_balancing.len(), targets.len());
    assert!(!coordinator.has_managed_state());
    for target in &targets {
        coordinator
            .platform
            .state
            .borrow_mut()
            .processes
            .remove(&target.id);
        assert!(coordinator.retire_exited_instance_balancing(target, true));
    }
    assert!(coordinator.instance_balancing.is_empty());
}

#[test]
fn exit_retirement_preserves_live_suppression_and_unknown_or_replacement_guards() {
    let mut coordinator = coordinator();
    allowed(&mut coordinator, 1).unwrap();
    coordinator.suppress_instance_balancing(&identity(42, 7));
    assert!(!coordinator.retire_exited_instance_balancing(&target(42, 7), true));
    assert_eq!(
        allowed(&mut coordinator, 1),
        Err(CpuAllocationApplyError::Eligibility(
            InstanceBalancingEligibilityError::Suppressed,
        ))
    );
    coordinator.platform.state.borrow_mut().deny_open = true;
    assert!(!coordinator.retire_exited_instance_balancing(&target(42, 7), true));
    assert!(coordinator
        .instance_balancing
        .contains_key(&identity(42, 7)));
    coordinator.platform.state.borrow_mut().deny_open = false;
    coordinator.platform.state.borrow_mut().processes.insert(
        42,
        FakeProcessState {
            identity: identity(42, 8),
            affinity: 0b1111,
            system_affinity: 0b1111,
            cpu_sets: Vec::new(),
        },
    );
    coordinator
        .register_instance_balancing_generation(&target(42, 8), 1)
        .unwrap();
    assert!(coordinator.retire_exited_instance_balancing(&target(42, 7), true));
    assert!(!coordinator
        .instance_balancing
        .contains_key(&identity(42, 7)));
    assert!(coordinator
        .instance_balancing
        .contains_key(&identity(42, 8)));
    assert_eq!(process_state(&coordinator).affinity, 0b1111);
    assert!(process_state(&coordinator).cpu_sets.is_empty());
}

#[test]
fn verified_originals_allow_balancing_beyond_a_prior_adaptive_share() {
    for hard in [false, true] {
        let mut coordinator = coordinator();
        let request = if hard {
            CpuAllocationRequest::HardAffinity {
                selection: test_selection(0b0011),
            }
        } else {
            CpuAllocationRequest::SoftCpuSets {
                selection: test_selection(0b0011),
            }
        };
        apply(
            &mut coordinator,
            claim(ControlOwner::AdaptiveEngine, request, 7),
        )
        .unwrap();
        assert_eq!(
            apply(&mut coordinator, balancing(0b1100, 1, 7)).unwrap(),
            CpuAllocationApplyOutcome::Applied
        );
        let process = process_state(&coordinator);
        assert_eq!(process.affinity, 0b1111);
        assert_eq!(process.cpu_sets, vec![102, 103]);
        let managed = &coordinator.managed_cpu_sets[&identity(42, 7)];
        assert!(managed.baseline.is_empty());
        assert!(managed.confirmed);
        coordinator.shutdown().unwrap();
        assert!(process_state(&coordinator).cpu_sets.is_empty());
    }
}

#[test]
fn unmanaged_defaults_are_preserved_even_when_they_equal_the_desired_share() {
    let mut coordinator = coordinator();
    coordinator
        .platform
        .state
        .borrow_mut()
        .processes
        .get_mut(&42)
        .unwrap()
        .cpu_sets = vec![100, 101];
    assert_eq!(
        allowed(&mut coordinator, 1),
        Err(CpuAllocationApplyError::Eligibility(
            InstanceBalancingEligibilityError::ExternalAssignment
        ))
    );
    assert_eq!(
        apply(&mut coordinator, balancing(0b0011, 1, 7)),
        Err(CpuAllocationApplyError::Eligibility(
            InstanceBalancingEligibilityError::ExternalAssignment
        ))
    );
    assert_eq!(process_state(&coordinator).cpu_sets, vec![100, 101]);
    assert!(!coordinator.has_managed_state());
}

#[test]
fn drift_stays_suppressed_across_withdrawal_until_a_newer_explicit_generation() {
    for released in [false, true] {
        for cpu_sets in [false, true] {
            let mut coordinator = coordinator();
            apply(&mut coordinator, balancing(0b0011, 2, 7)).unwrap();
            if released {
                assert!(coordinator
                    .release_all_policy(ControlOwner::InstanceBalancing)
                    .failures
                    .is_empty());
            }
            {
                let mut state = coordinator.platform.state.borrow_mut();
                let process = state.processes.get_mut(&42).unwrap();
                if cpu_sets {
                    process.cpu_sets = vec![103];
                } else {
                    process.affinity = 0b0101;
                }
            }
            assert_eq!(
                apply(&mut coordinator, balancing(0b0011, 2, 7)),
                Err(CpuAllocationApplyError::Eligibility(
                    InstanceBalancingEligibilityError::ExternalChange
                ))
            );
            coordinator.release_all_policy(ControlOwner::InstanceBalancing);
            {
                let mut state = coordinator.platform.state.borrow_mut();
                let process = state.processes.get_mut(&42).unwrap();
                assert_eq!(process.affinity, if cpu_sets { 0b1111 } else { 0b0101 });
                if cpu_sets {
                    assert_eq!(process.cpu_sets, vec![103]);
                }
                process.affinity = 0b1111;
                process.cpu_sets.clear();
            }
            assert_eq!(
                apply(&mut coordinator, balancing(0b0011, 2, 7)),
                Err(CpuAllocationApplyError::Eligibility(
                    InstanceBalancingEligibilityError::Suppressed
                ))
            );
            assert_eq!(
                apply(&mut coordinator, balancing(0b0011, 1, 7)),
                Err(CpuAllocationApplyError::Eligibility(
                    InstanceBalancingEligibilityError::StaleGeneration
                ))
            );
            assert!(apply(&mut coordinator, balancing(0b0011, 3, 7)).is_ok());
        }
    }
}

#[test]
fn shadowed_generations_cannot_regress_and_handoff_rechecks_external_state() {
    let mut coordinator = coordinator();
    apply(
        &mut coordinator,
        claim(
            ControlOwner::CpuSetsSoft,
            CpuAllocationRequest::SoftCpuSets {
                selection: test_selection(0b0011),
            },
            7,
        ),
    )
    .unwrap();
    assert_eq!(
        apply(&mut coordinator, balancing(0b1100, 3, 7)).unwrap(),
        CpuAllocationApplyOutcome::Shadowed
    );
    assert_eq!(
        apply(&mut coordinator, balancing(0b1100, 2, 7)),
        Err(CpuAllocationApplyError::Eligibility(
            InstanceBalancingEligibilityError::StaleGeneration
        ))
    );
    coordinator
        .platform
        .state
        .borrow_mut()
        .processes
        .get_mut(&42)
        .unwrap()
        .cpu_sets = vec![103];
    coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
    let summary = coordinator.reconcile_pending(Ok(&test_topology()), true, false);
    assert!(summary.applications.is_empty());
    assert_eq!(summary.failures.len(), 1);
    assert_eq!(process_state(&coordinator).cpu_sets, vec![103]);
    assert!(!coordinator.has_managed_state());
    coordinator
        .platform
        .state
        .borrow_mut()
        .processes
        .get_mut(&42)
        .unwrap()
        .cpu_sets
        .clear();
    assert_eq!(
        apply(&mut coordinator, balancing(0b1100, 3, 7)),
        Err(CpuAllocationApplyError::Eligibility(
            InstanceBalancingEligibilityError::Suppressed
        ))
    );
}

#[test]
fn matching_but_unconfirmed_state_cannot_authorize_balancing() {
    for hard in [false, true] {
        let mut coordinator = coordinator();
        coordinator.platform.state.borrow_mut().fail_next_commit = true;
        let request = if hard {
            CpuAllocationRequest::HardAffinity {
                selection: test_selection(0b0011),
            }
        } else {
            CpuAllocationRequest::SoftCpuSets {
                selection: test_selection(0b0011),
            }
        };
        assert!(apply(
            &mut coordinator,
            claim(ControlOwner::AdaptiveEngine, request, 7)
        )
        .is_err());
        assert_eq!(
            allowed(&mut coordinator, 1),
            Err(CpuAllocationApplyError::Eligibility(
                InstanceBalancingEligibilityError::UncertainState
            ))
        );
        assert_eq!(
            apply(&mut coordinator, balancing(0b1100, 1, 7)),
            Err(CpuAllocationApplyError::Eligibility(
                InstanceBalancingEligibilityError::UncertainState
            ))
        );
        assert_ne!(process_state(&coordinator).cpu_sets, vec![102, 103]);
    }
}

#[test]
fn final_default_set_read_does_not_recapture_drift_as_a_baseline() {
    let mut coordinator = coordinator();
    let selection = test_selection(0b0011);
    let mut inventory = CpuSetInventory::default();
    let entries = inventory.entries(&mut coordinator.platform).unwrap();
    let expectation = coordinator
        .prepare_instance_balancing(
            &identity(42, 7),
            &42,
            &test_topology(),
            selection.processors(),
            1,
            entries,
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
    assert_eq!(
        coordinator.apply_soft_claim(
            &identity(42, 7),
            &42,
            ControlOwner::InstanceBalancing,
            vec![100, 101],
            Some(expectation)
        ),
        Err(CpuAllocationApplyError::Eligibility(
            InstanceBalancingEligibilityError::ExternalChange
        ))
    );
    assert_eq!(process_state(&coordinator).cpu_sets, vec![103]);
    assert!(!coordinator.has_managed_state());
}

#[test]
fn a_final_read_failure_queues_a_fresh_lower_owner_handoff() {
    let mut coordinator = coordinator();
    apply(
        &mut coordinator,
        claim(
            ControlOwner::AdaptiveEngine,
            CpuAllocationRequest::HardAffinity {
                selection: test_selection(0b0011),
            },
            7,
        ),
    )
    .unwrap();
    coordinator
        .platform
        .state
        .borrow_mut()
        .affinity_query_failures = [false, false, false, true].into();
    assert!(apply(&mut coordinator, balancing(0b1100, 1, 7)).is_err());
    assert_eq!(process_state(&coordinator).affinity, 0b1111);
    assert!(coordinator.managed_affinity.is_empty());
    assert!(coordinator.has_pending_reconciliation());
    coordinator.reconcile_pending(Ok(&test_topology()), true, false);
    assert_eq!(process_state(&coordinator).affinity, 0b0011);
    let managed = &coordinator.managed_affinity[&identity(42, 7)];
    assert_eq!(managed.owner, ControlOwner::AdaptiveEngine);
    assert_eq!(managed.baseline, 0b1111);
    assert!(managed.confirmed);
    assert!(process_state(&coordinator).cpu_sets.is_empty());
}

#[test]
fn refusal_reports_failed_cleanup_and_retains_restoration_ownership() {
    let mut coordinator = coordinator();
    apply(&mut coordinator, balancing(0b0011, 1, 7)).unwrap();
    {
        let mut state = coordinator.platform.state.borrow_mut();
        state.processes.get_mut(&42).unwrap().cpu_sets = vec![103];
        state.fail_next_cpu_sets_relinquish = true;
    }
    assert!(matches!(
        apply(&mut coordinator, balancing(0b0011, 1, 7)),
        Err(CpuAllocationApplyError::Eligibility(
            InstanceBalancingEligibilityError::RestorationPending { .. }
        ))
    ));
    assert!(coordinator.has_managed_state());
    assert!(coordinator.has_pending_reconciliation());
    coordinator.reconcile_pending(Ok(&test_topology()), true, true);
    assert_eq!(process_state(&coordinator).cpu_sets, vec![103]);
    assert!(!coordinator.has_managed_state());
}

#[test]
fn denied_refresh_and_failed_inventory_withdraw_prior_balancing_claims() {
    for discovery in [false, true] {
        let mut coordinator = coordinator();
        apply(&mut coordinator, balancing(0b0011, 1, 7)).unwrap();
        {
            let mut state = coordinator.platform.state.borrow_mut();
            if discovery {
                state.fail_inventory = true;
            } else {
                state.deny_open = true;
            }
        }
        assert!(apply(&mut coordinator, balancing(0b1100, 1, 7)).is_err());
        assert!(coordinator.claims[&ControlOwner::InstanceBalancing].is_empty());
        coordinator.platform.state.borrow_mut().deny_open = false;
        coordinator.reconcile_pending(Ok(&test_topology()), true, true);
        assert!(process_state(&coordinator).cpu_sets.is_empty());
        assert!(!coordinator.has_managed_state());
    }
}

#[test]
fn failed_handoff_does_not_leave_an_old_balancing_plan_to_retry() {
    for discovery in [false, true] {
        let mut coordinator = coordinator();
        apply(
            &mut coordinator,
            claim(
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    selection: test_selection(0b0011),
                },
                7,
            ),
        )
        .unwrap();
        assert_eq!(
            apply(&mut coordinator, balancing(0b1100, 1, 7)).unwrap(),
            CpuAllocationApplyOutcome::Shadowed
        );
        coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
        {
            let mut state = coordinator.platform.state.borrow_mut();
            state.fail_inventory = discovery;
            state.deny_open = !discovery;
        }
        let summary = coordinator.reconcile_pending(Ok(&test_topology()), true, false);
        assert!(summary.applications.is_empty());
        assert!(!summary.failures.is_empty());
        assert!(coordinator.claims[&ControlOwner::InstanceBalancing].is_empty());
        coordinator.platform.state.borrow_mut().deny_open = false;
        let retry = coordinator.reconcile_pending(Ok(&test_topology()), true, true);
        assert!(retry.applications.is_empty());
        assert!(process_state(&coordinator).cpu_sets.is_empty());
        assert!(!coordinator.has_managed_state());
    }
}

#[test]
fn verified_exit_or_pid_replacement_retires_only_the_old_guard() {
    for replacement in [false, true] {
        let mut coordinator = coordinator();
        apply(&mut coordinator, balancing(0b0011, 8, 7)).unwrap();
        if replacement {
            coordinator.platform.state.borrow_mut().processes.insert(
                42,
                FakeProcessState {
                    identity: identity(42, 8),
                    affinity: 0b1111,
                    system_affinity: 0b1111,
                    cpu_sets: Vec::new(),
                },
            );
            assert!(apply(&mut coordinator, balancing(0b1100, 0, 8)).is_ok());
            assert_eq!(coordinator.instance_balancing.len(), 1);
            assert!(coordinator
                .instance_balancing
                .contains_key(&identity(42, 8)));
        } else {
            coordinator
                .platform
                .state
                .borrow_mut()
                .processes
                .remove(&42);
            let release = coordinator.release_all_policy(ControlOwner::InstanceBalancing);
            assert!(release.failures.is_empty());
            assert!(coordinator.instance_balancing.is_empty());
            assert!(!coordinator.has_managed_state());
        }
    }
}

#[test]
fn reapply_cannot_confirm_an_unverified_restoration_chain() {
    for hard in [false, true] {
        for (external, failed) in [(false, false), (true, false), (true, true)] {
            let mut coordinator = coordinator();
            if hard {
                coordinator
                    .platform
                    .state
                    .borrow_mut()
                    .processes
                    .get_mut(&42)
                    .unwrap()
                    .affinity = if external { 0b0101 } else { 0b0011 };
                coordinator
                    .platform
                    .state
                    .borrow_mut()
                    .affinity_apply_failures_remaining = usize::from(failed);
                let result = coordinator.reapply_affinity(
                    &identity(42, 7),
                    &42,
                    ManagedValue {
                        baseline: 0b1111,
                        expected: 0b0011,
                        owner: ControlOwner::AdaptiveEngine,
                        apply_sequence: 1,
                        confirmed: true,
                    },
                );
                assert_eq!(result.is_err(), failed);
                if failed {
                    coordinator
                        .platform
                        .state
                        .borrow_mut()
                        .processes
                        .get_mut(&42)
                        .unwrap()
                        .affinity = 0b0011;
                }
                assert!(!coordinator.managed_affinity[&identity(42, 7)].confirmed);
            } else {
                coordinator
                    .platform
                    .state
                    .borrow_mut()
                    .processes
                    .get_mut(&42)
                    .unwrap()
                    .cpu_sets = if external { vec![103] } else { vec![100, 101] };
                coordinator
                    .platform
                    .state
                    .borrow_mut()
                    .fail_next_cpu_sets_apply = failed;
                let result = coordinator.reapply_cpu_sets(
                    &identity(42, 7),
                    &42,
                    ManagedValue {
                        baseline: Vec::new(),
                        expected: vec![100, 101],
                        owner: ControlOwner::AdaptiveEngine,
                        apply_sequence: 1,
                        confirmed: true,
                    },
                );
                assert_eq!(result.is_err(), failed);
                if failed {
                    coordinator
                        .platform
                        .state
                        .borrow_mut()
                        .processes
                        .get_mut(&42)
                        .unwrap()
                        .cpu_sets = vec![100, 101];
                }
                assert!(!coordinator.managed_cpu_sets[&identity(42, 7)].confirmed);
            }
            assert_eq!(
                coordinator
                    .platform
                    .state
                    .borrow()
                    .events
                    .iter()
                    .any(|event| event == "commit"),
                external && !failed
            );
            if hard {
                assert!(coordinator
                    .apply_affinity_value(
                        &identity(42, 7),
                        &42,
                        ControlOwner::ProcessorAffinityHard,
                        0b1100,
                        None,
                    )
                    .is_ok());
                assert!(!coordinator.managed_affinity[&identity(42, 7)].confirmed);
            } else {
                assert!(coordinator
                    .apply_cpu_sets_value(
                        &identity(42, 7),
                        &42,
                        ControlOwner::CpuSetsSoft,
                        vec![102, 103],
                        None,
                    )
                    .is_ok());
                assert!(!coordinator.managed_cpu_sets[&identity(42, 7)].confirmed);
            }
            assert_eq!(
                apply(&mut coordinator, balancing(0b1100, 1, 7)),
                Err(CpuAllocationApplyError::Eligibility(
                    InstanceBalancingEligibilityError::UncertainState
                ))
            );
        }
    }
}
