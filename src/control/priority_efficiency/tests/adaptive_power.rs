use super::*;

const PID: u32 = 7;
const CREATION_TIME: u64 = 1;

fn unmanaged_speed_with_timer_policy() -> PowerThrottlingState {
    PowerThrottlingState {
        version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        control_mask: PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        state_mask: PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
    }
}

fn controller(baseline: PowerThrottlingState) -> PriorityEfficiencyController<FakePlatform> {
    let mut platform = platform_with(PID, CREATION_TIME, NORMAL_PRIORITY_CLASS);
    platform.processes.get_mut(&PID).unwrap().power = baseline;
    PriorityEfficiencyController::with_platform(platform)
}

#[test]
fn hint_requires_owned_priority_and_restores_without_ending_cpu_restraint() {
    let baseline = unmanaged_speed_with_timer_policy();
    let mut controller = controller(baseline);
    let target = target(PID, CREATION_TIME);
    assert!(!controller.has_adaptive_restraint_power_state());
    assert_eq!(
        controller
            .apply_adaptive_restraint_power(target.clone(), true)
            .unwrap(),
        ProcessPropertyApplyOutcome::Shadowed
    );
    controller
        .apply_priority_claim(restraint_claim(PID, CREATION_TIME), true)
        .unwrap();
    assert_eq!(
        controller
            .apply_adaptive_restraint_power(target, true)
            .unwrap(),
        ProcessPropertyApplyOutcome::Applied
    );
    assert!(controller.has_adaptive_restraint_power_state());
    assert_eq!(
        controller.platform.processes[&PID].power,
        baseline.with_execution_speed_enabled()
    );
    let release = controller.release_adaptive_restraint_power_except(&BTreeSet::new());
    assert_eq!(release.restored_processes, 1);
    assert!(release.failures.is_empty());
    assert!(!controller.has_adaptive_restraint_power_state());
    assert_eq!(controller.platform.processes[&PID].power, baseline);
    assert_eq!(
        controller.platform.processes[&PID].priority,
        BELOW_NORMAL_PRIORITY_CLASS
    );
    assert_eq!(
        controller
            .adaptive_restraint_processes()
            .next()
            .map(|(_, state)| state),
        Some(AdaptiveRestraintState::Verified)
    );
}

#[test]
fn explicitly_managed_native_qos_is_unmodified_and_unowned() {
    for (baseline, expected) in [
        (baseline_power(), ProcessPropertyApplyOutcome::Preserved),
        (
            baseline_power().with_execution_speed_enabled(),
            ProcessPropertyApplyOutcome::Unchanged,
        ),
    ] {
        let mut controller = controller(baseline);
        controller
            .apply_priority_claim(restraint_claim(PID, CREATION_TIME), true)
            .unwrap();
        assert_eq!(
            controller
                .apply_adaptive_restraint_power(target(PID, CREATION_TIME), true)
                .unwrap(),
            expected
        );
        assert_eq!(controller.platform.processes[&PID].power, baseline);
        assert!(!controller.has_adaptive_restraint_power_state());
        assert!(controller.managed_power.is_empty());
    }
}

#[test]
fn external_reset_yields_until_the_next_verified_episode() {
    let baseline = unmanaged_speed_with_timer_policy();
    let mut controller = controller(baseline);
    let target = target(PID, CREATION_TIME);
    let active = BTreeSet::from([target.key()]);
    controller
        .apply_priority_claim(restraint_claim(PID, CREATION_TIME), true)
        .unwrap();
    controller
        .apply_adaptive_restraint_power(target.clone(), true)
        .unwrap();
    controller.platform.processes.get_mut(&PID).unwrap().power = baseline;
    assert_eq!(
        controller
            .apply_adaptive_restraint_power(target.clone(), true)
            .unwrap(),
        ProcessPropertyApplyOutcome::Preserved
    );
    assert!(controller.managed_power.is_empty());
    controller.release_adaptive_restraint_power_except(&active);
    assert_eq!(
        controller
            .apply_adaptive_restraint_power(target.clone(), true)
            .unwrap(),
        ProcessPropertyApplyOutcome::Shadowed
    );
    assert_eq!(controller.platform.processes[&PID].power, baseline);

    controller.release_all_priority_policy(ControlOwner::AdaptiveEngine);
    controller.release_adaptive_restraint_power_except(&BTreeSet::new());
    assert!(!controller.has_adaptive_restraint_power_state());
    controller
        .apply_priority_claim(restraint_claim(PID, CREATION_TIME), true)
        .unwrap();
    assert_eq!(
        controller
            .apply_adaptive_restraint_power(target, true)
            .unwrap(),
        ProcessPropertyApplyOutcome::Applied
    );
}

#[test]
fn higher_or_external_cpu_priority_withdraws_hint_even_with_old_active_key() {
    let baseline = unmanaged_speed_with_timer_policy();
    for external in [false, true] {
        let mut controller = controller(baseline);
        let target = target(PID, CREATION_TIME);
        let active = BTreeSet::from([target.key()]);
        controller
            .apply_priority_claim(restraint_claim(PID, CREATION_TIME), true)
            .unwrap();
        controller
            .apply_adaptive_restraint_power(target.clone(), true)
            .unwrap();
        if external {
            controller
                .platform
                .processes
                .get_mut(&PID)
                .unwrap()
                .priority = ABOVE_NORMAL_PRIORITY_CLASS;
        } else {
            controller
                .apply_priority_claim(
                    priority_claim(
                        PID,
                        CREATION_TIME,
                        ControlOwner::ProcessPriority,
                        PriorityClassValue::AboveNormal,
                    ),
                    true,
                )
                .unwrap();
        }
        assert_eq!(
            controller
                .apply_adaptive_restraint_power(target, true)
                .unwrap(),
            ProcessPropertyApplyOutcome::Shadowed
        );
        let release = controller.release_adaptive_restraint_power_except(&active);
        assert_eq!(release.restored_processes, 1);
        assert_eq!(controller.platform.processes[&PID].power, baseline);
        assert_eq!(
            controller.platform.processes[&PID].priority,
            ABOVE_NORMAL_PRIORITY_CLASS
        );
    }
}

#[test]
fn failed_hint_cleanup_retries_independently_of_cpu_restraint() {
    let baseline = unmanaged_speed_with_timer_policy();
    let mut controller = controller(baseline);
    let target = target(PID, CREATION_TIME);
    controller
        .apply_priority_claim(restraint_claim(PID, CREATION_TIME), true)
        .unwrap();
    controller
        .apply_adaptive_restraint_power(target.clone(), true)
        .unwrap();
    controller.platform.fail_next_power_apply = true;
    let release = controller.release_adaptive_restraint_power_except(&BTreeSet::new());
    assert_eq!(release.failures.len(), 1);
    assert_eq!(
        controller
            .adaptive_restraint_power_release_failures()
            .count(),
        1
    );
    assert_eq!(
        controller
            .apply_adaptive_restraint_power(target, true)
            .unwrap(),
        ProcessPropertyApplyOutcome::Shadowed
    );
    let retry = controller.retry_pending_releases(controller.release_retry_at.unwrap());
    assert_eq!(
        retry.adaptive_restraint_power.unwrap().restored_processes,
        1
    );
    assert_eq!(retry.other.restored_processes, 0);
    assert!(retry.efficiency_mode.is_none());
    assert_eq!(
        controller
            .adaptive_restraint_power_release_failures()
            .count(),
        0
    );
    assert_eq!(controller.platform.processes[&PID].power, baseline);
    assert_eq!(
        controller.platform.processes[&PID].priority,
        BELOW_NORMAL_PRIORITY_CLASS
    );
}

#[test]
fn successful_manual_and_standalone_takeovers_clear_pending_cleanup() {
    let baseline = unmanaged_speed_with_timer_policy();
    for manual in [false, true] {
        let mut controller = controller(baseline);
        let mut restraint = restraint_claim(PID, CREATION_TIME);
        restraint.priority = PriorityClassValue::Idle;
        controller.apply_priority_claim(restraint, true).unwrap();
        controller
            .apply_adaptive_restraint_power(target(PID, CREATION_TIME), true)
            .unwrap();
        controller.platform.fail_next_power_apply = true;
        let failed = controller.release_adaptive_restraint_power_except(&BTreeSet::new());
        assert_eq!(failed.failures.len(), 1);
        let retry_at = controller.release_retry_at.unwrap();

        let outcome = if manual {
            controller.apply_process_list_efficiency_mode(
                &action_target(PID, CREATION_TIME),
                true,
                true,
            )
        } else {
            controller.apply_efficiency_claim(efficiency_claim(PID, CREATION_TIME), true)
        };
        assert_eq!(outcome.unwrap(), ProcessPropertyApplyOutcome::Unchanged);
        assert_eq!(
            controller
                .adaptive_restraint_power_release_failures()
                .count(),
            0
        );
        assert!(controller
            .retry_pending_releases(retry_at)
            .adaptive_restraint_power
            .is_none());
        controller.release_adaptive_restraint_power_except(&BTreeSet::new());
        assert_eq!(
            controller.platform.processes[&PID].power,
            baseline.with_execution_speed_enabled()
        );
        assert!(!controller.has_adaptive_restraint_power_state());

        if !manual {
            controller.release_all_efficiency_policy(ControlOwner::EfficiencyMode);
            assert_eq!(controller.platform.processes[&PID].power, baseline);
        }
        controller.shutdown().unwrap();
        assert_eq!(controller.platform.processes[&PID].power, baseline);
    }
}
