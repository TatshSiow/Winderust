use super::*;

fn adaptive_claim(priority: ProcessIoPriority) -> IoPriorityClaim {
    claim(
        ControlOwner::AdaptiveEngine,
        priority,
        IoPriorityPreservation::PreserveLower,
    )
}

fn active_episode(controller: &mut IoPriorityController<FakePlatform>) {
    let active = BTreeSet::from([target(42, 7).key()]);
    assert!(controller
        .reconcile_adaptive_restraint_targets(&active, &active)
        .failures
        .is_empty());
}

#[test]
fn exact_static_reservation_shadows_adaptive_even_without_a_write() {
    for preservation in [
        None,
        Some(IoPriorityPreservation::Exact),
        Some(IoPriorityPreservation::PreserveHigher),
    ] {
        let mut controller = IoPriorityController::with_platform(FakePlatform::new(2));
        active_episode(&mut controller);
        let reserved = BTreeSet::from([target(42, 7).key()]);
        controller.release_policy_except(ControlOwner::IoPriority, &reserved);
        if let Some(preservation) = preservation {
            controller
                .apply_policy_claim(
                    claim(
                        ControlOwner::IoPriority,
                        ProcessIoPriority::Normal,
                        preservation,
                    ),
                    true,
                )
                .unwrap();
        }
        assert!(!controller.has_managed_state());
        let before = controller.platform.events.lock().unwrap().len();
        assert_eq!(
            controller
                .apply_policy_claim(adaptive_claim(ProcessIoPriority::Low), true)
                .unwrap(),
            IoPriorityApplyOutcome::Shadowed
        );
        assert_eq!(controller.platform.events.lock().unwrap().len(), before);

        // Reserving the same PID at another creation time cannot block this episode.
        controller.release_policy_except(
            ControlOwner::IoPriority,
            &BTreeSet::from([target(42, 8).key()]),
        );
        assert_eq!(
            controller
                .apply_policy_claim(adaptive_claim(ProcessIoPriority::Low), true)
                .unwrap(),
            IoPriorityApplyOutcome::Applied
        );
        assert_eq!(
            controller
                .release_all_policy(ControlOwner::AdaptiveEngine)
                .restored_processes,
            1
        );
        assert_eq!(controller.platform.processes[&42].priority, 2);
    }
}

#[test]
fn static_reservation_withdraws_adaptive_even_when_static_application_fails() {
    let mut controller = IoPriorityController::with_platform(FakePlatform::new(2));
    active_episode(&mut controller);
    controller
        .apply_policy_claim(adaptive_claim(ProcessIoPriority::VeryLow), true)
        .unwrap();
    let active = BTreeSet::from([target(42, 7).key()]);
    controller.release_policy_except(ControlOwner::IoPriority, &active);
    controller.platform.fail_next(FakeFailure::Open);
    assert!(controller
        .apply_policy_claim(
            claim(
                ControlOwner::IoPriority,
                ProcessIoPriority::Low,
                IoPriorityPreservation::Exact,
            ),
            true,
        )
        .is_err());
    let release = controller.reconcile_adaptive_restraint_targets(&active, &active);
    assert_eq!(release.restored_processes, 1);
    assert_eq!(controller.platform.processes[&42].priority, 2);
}

#[test]
fn explicit_static_takeover_keeps_baseline_and_survives_adaptive_release() {
    let mut controller = IoPriorityController::with_platform(FakePlatform::new(2));
    active_episode(&mut controller);
    controller
        .apply_policy_claim(adaptive_claim(ProcessIoPriority::Low), true)
        .unwrap();
    assert_eq!(
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::IoPriority,
                    ProcessIoPriority::Low,
                    IoPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap(),
        IoPriorityApplyOutcome::Unchanged
    );
    assert_eq!(
        controller
            .release_all_policy(ControlOwner::AdaptiveEngine)
            .restored_processes,
        0
    );
    assert_eq!(
        controller.policy_managed_process_count(ControlOwner::IoPriority),
        1
    );
    assert_eq!(controller.platform.processes[&42].priority, 1);
    assert_eq!(
        controller
            .release_all_policy(ControlOwner::IoPriority)
            .restored_processes,
        1
    );
    assert_eq!(controller.platform.processes[&42].priority, 2);
}

#[test]
fn no_op_manual_takes_ownership_and_adaptive_does_not_restore_over_it() {
    let mut controller = IoPriorityController::with_platform(FakePlatform::new(2));
    active_episode(&mut controller);
    controller
        .apply_policy_claim(adaptive_claim(ProcessIoPriority::Low), true)
        .unwrap();
    assert_eq!(
        controller
            .apply_process_list_action(&action_target(), ProcessIoPriority::Low, true)
            .unwrap(),
        IoPriorityApplyOutcome::Unchanged
    );
    assert_eq!(
        controller
            .apply_policy_claim(adaptive_claim(ProcessIoPriority::VeryLow), true)
            .unwrap(),
        IoPriorityApplyOutcome::Shadowed
    );
    assert_eq!(
        controller.policy_managed_process_count(ControlOwner::AdaptiveEngine),
        0
    );
    assert_eq!(
        controller
            .release_all_policy(ControlOwner::AdaptiveEngine)
            .restored_processes,
        0
    );
    assert_eq!(controller.platform.processes[&42].priority, 1);

    active_episode(&mut controller);
    assert_eq!(
        controller
            .apply_policy_claim(adaptive_claim(ProcessIoPriority::VeryLow), true)
            .unwrap(),
        IoPriorityApplyOutcome::Shadowed
    );
    controller.shutdown().unwrap();
    assert_eq!(controller.platform.processes[&42].priority, 2);
}

#[test]
fn manual_no_op_on_unowned_property_yields_until_verified_episode_ends() {
    let mut controller = IoPriorityController::with_platform(FakePlatform::new(2));
    let qualified = BTreeSet::from([target(42, 7).key()]);
    controller.reconcile_adaptive_restraint_targets(&BTreeSet::new(), &qualified);
    assert_eq!(
        controller
            .apply_process_list_action(&action_target(), ProcessIoPriority::Normal, true)
            .unwrap(),
        IoPriorityApplyOutcome::Unchanged
    );
    assert!(!controller.has_managed_state());
    active_episode(&mut controller);
    assert_eq!(
        controller
            .apply_policy_claim(adaptive_claim(ProcessIoPriority::Low), true)
            .unwrap(),
        IoPriorityApplyOutcome::Shadowed
    );
    controller.release_all_policy(ControlOwner::AdaptiveEngine);
    active_episode(&mut controller);
    assert_eq!(
        controller
            .apply_policy_claim(adaptive_claim(ProcessIoPriority::Low), true)
            .unwrap(),
        IoPriorityApplyOutcome::Applied
    );
}

#[test]
fn delayed_releases_keep_exact_owners_and_block_reapply_until_restored() {
    let platform = FakePlatform::with_processes(&[(42, 7, 2), (43, 8, 2)]);
    let mut controller = IoPriorityController::with_platform(platform);
    active_episode(&mut controller);
    controller
        .apply_policy_claim(adaptive_claim(ProcessIoPriority::Low), true)
        .unwrap();
    controller
        .apply_policy_claim(
            IoPriorityClaim {
                target: target(43, 8),
                owner: ControlOwner::IoPriority,
                priority: ProcessIoPriority::VeryLow,
                preservation: IoPriorityPreservation::Exact,
            },
            true,
        )
        .unwrap();
    controller.platform.fail_next(FakeFailure::Open);
    assert_eq!(
        controller
            .release_all_policy(ControlOwner::AdaptiveEngine)
            .failures[0]
            .process_id,
        42
    );
    controller.platform.fail_next(FakeFailure::Open);
    assert_eq!(
        controller
            .release_all_policy(ControlOwner::IoPriority)
            .failures[0]
            .process_id,
        43
    );
    for (owner, id) in [
        (ControlOwner::AdaptiveEngine, 42),
        (ControlOwner::IoPriority, 43),
    ] {
        assert_eq!(
            controller
                .policy_release_failures(owner)
                .map(|(identity, _)| identity.id)
                .collect::<Vec<_>>(),
            [id]
        );
    }
    active_episode(&mut controller);
    assert_eq!(
        controller
            .apply_policy_claim(adaptive_claim(ProcessIoPriority::VeryLow), true)
            .unwrap(),
        IoPriorityApplyOutcome::Shadowed
    );
    let retry = controller.retry_pending_releases(controller.next_release_retry.unwrap());
    assert_eq!(retry.static_policy.restored_processes, 1);
    assert_eq!(retry.adaptive_engine.restored_processes, 1);
    assert_eq!(
        controller
            .policy_release_failures(ControlOwner::IoPriority)
            .count(),
        0
    );
    assert_eq!(
        controller
            .policy_release_failures(ControlOwner::AdaptiveEngine)
            .count(),
        0
    );
    assert_eq!(controller.platform.processes[&42].priority, 2);
    assert_eq!(controller.platform.processes[&43].priority, 2);
}

#[test]
fn adaptive_candidate_preserves_lower_and_unknown_external_values() {
    for existing in [0, 1, 5] {
        let mut controller = IoPriorityController::with_platform(FakePlatform::new(existing));
        active_episode(&mut controller);
        assert_eq!(
            controller
                .apply_policy_claim(adaptive_claim(ProcessIoPriority::Low), true)
                .unwrap(),
            IoPriorityApplyOutcome::Preserved
        );
        assert_eq!(controller.platform.processes[&42].priority, existing);
        assert!(!controller.has_managed_state());
    }
}

#[test]
fn uncertain_adaptive_application_queues_only_adaptive_recovery() {
    let mut controller = IoPriorityController::with_platform(FakePlatform::new(2));
    active_episode(&mut controller);
    controller.platform.fail_next(FakeFailure::Commit);
    assert!(controller
        .apply_policy_claim(adaptive_claim(ProcessIoPriority::Low), true)
        .is_err());
    assert_eq!(
        controller.policy_managed_process_count(ControlOwner::AdaptiveEngine),
        0
    );
    assert_eq!(
        controller
            .policy_release_failures(ControlOwner::AdaptiveEngine)
            .count(),
        1
    );
    let retry = controller.retry_pending_releases(controller.next_release_retry.unwrap());
    assert_eq!(retry.static_policy.restored_processes, 0);
    assert_eq!(retry.adaptive_engine.restored_processes, 1);
    assert_eq!(controller.platform.processes[&42].priority, 2);
}
