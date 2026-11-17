use super::*;
use std::path::Path;

fn context(name: &str) -> DesktopBindingKey {
    DesktopBindingKey::new(name.into())
}
fn key(pid: u32, created: u64) -> ProcessTargetKey {
    ProcessTargetKey::new(pid, created, Path::new(r"C:\Apps\app.exe"))
}
fn at(base: Instant, seconds: u64) -> Instant {
    base + Duration::from_secs(seconds)
}
fn complete<'a>(
    now: Instant,
    context: &'a DesktopBindingKey,
    members: &'a [ProcessTargetKey],
) -> Observation<'a> {
    Observation::Complete {
        observed_at: now,
        context,
        members,
    }
}
fn armed(
    now: Instant,
    context: &DesktopBindingKey,
    members: &[ProcessTargetKey],
) -> KeepRunningManager {
    let mut manager = KeepRunningManager::default();
    manager.enable();
    manager.observe(now, complete(now, context, members));
    assert_eq!(manager.state(), KeepRunningState::Armed);
    manager
}
fn retrying(now: Instant, context: &DesktopBindingKey) -> KeepRunningManager {
    let member = key(10, 1);
    let mut manager = armed(now, context, std::slice::from_ref(&member));
    manager.exited(now, &member);
    manager
}
fn prepare(
    manager: &mut KeepRunningManager,
    now: Instant,
    context: &DesktopBindingKey,
) -> LaunchTicket {
    match manager.try_prepare(now, complete(now, context, &[])) {
        PrepareDisposition::Prepare(ticket) => ticket,
        other => panic!("Expected preparation, got {other:?}"),
    }
}
fn commit(
    manager: &mut KeepRunningManager,
    now: Instant,
    context: &DesktopBindingKey,
    ticket: &LaunchTicket,
) {
    assert_eq!(manager.prepared(now, ticket), PreparedDisposition::Ready);
    assert_eq!(
        manager.try_reserve_commit(now, ticket, complete(now, context, &[]), true),
        CommitDisposition::Reserved
    );
}
fn launched(now: Instant, context: &DesktopBindingKey) -> (KeepRunningManager, ProcessTargetKey) {
    let mut manager = retrying(now, context);
    let launch = at(now, 1);
    let ticket = prepare(&mut manager, launch, context);
    commit(&mut manager, launch, context, &ticket);
    let child = key(20, 2);
    assert_eq!(
        manager.accepted(&ticket, child.clone()),
        AcceptanceDisposition::Accepted
    );
    (manager, child)
}

#[test]
fn enabling_absent_apps_and_resuming_disabled_rules_never_launches() {
    let now = Instant::now();
    let context = context("desktop-a");
    let mut manager = KeepRunningManager::default();
    assert_eq!(
        manager.try_prepare(now, complete(now, &context, &[])),
        PrepareDisposition::Wait
    );
    manager.enable();
    for seconds in [0, 1, 100] {
        let time = at(now, seconds);
        manager.enable();
        assert_eq!(
            manager.try_prepare(time, complete(time, &context, &[])),
            PrepareDisposition::Wait
        );
        assert_eq!(manager.state(), KeepRunningState::WaitingForApp);
    }
    manager.pause(PauseReason::IntentionalStop);
    manager.enable();
    assert_eq!(
        manager.state(),
        KeepRunningState::Paused(PauseReason::IntentionalStop)
    );
    manager.disable();
    manager.resume(at(now, 101), Some(&context));
    assert_eq!(manager.state(), KeepRunningState::Disabled);
}

#[test]
fn missing_scan_entries_and_reused_pids_cannot_remove_exact_members() {
    let now = Instant::now();
    let context = context("desktop-a");
    let original = key(10, 1);
    let other = key(11, 1);
    let reused = key(10, 2);
    let mut manager = armed(now, &context, &[original.clone(), other.clone()]);
    manager.observe(at(now, 1), complete(at(now, 1), &context, &[]));
    manager.exited(at(now, 1), &key(10, 99));
    manager.exited(
        at(now, 1),
        &ProcessTargetKey::new(11, 1, Path::new(r"C:\Other\app.exe")),
    );
    assert_eq!(manager.members.len(), 2);
    manager.observe(
        at(now, 2),
        complete(at(now, 2), &context, std::slice::from_ref(&reused)),
    );
    manager.exited(at(now, 3), &original);
    manager.exited(at(now, 3), &other);
    assert_eq!(
        manager.try_prepare(at(now, 20), complete(at(now, 20), &context, &[])),
        PrepareDisposition::Wait
    );
    manager.exited(at(now, 20), &reused);
    assert_eq!(
        manager.state(),
        KeepRunningState::Retrying {
            next_attempt: at(now, 21)
        }
    );
    prepare(&mut manager, at(now, 21), &context);
}

#[test]
fn attempts_are_spent_before_preparation_and_follow_one_five_thirty_second_delays() {
    let now = Instant::now();
    let context = context("desktop-a");
    let mut manager = retrying(now, &context);
    for (index, seconds) in [1, 6, 36].into_iter().enumerate() {
        let due = at(now, seconds);
        assert_eq!(
            manager.try_prepare(
                at(now, seconds - 1),
                complete(at(now, seconds - 1), &context, &[])
            ),
            PrepareDisposition::Wait
        );
        let ticket = prepare(&mut manager, due, &context);
        assert_eq!(manager.attempts, index + 1);
        assert_eq!(manager.state(), KeepRunningState::Starting);
        assert_eq!(
            manager.try_prepare(due, complete(due, &context, &[])),
            PrepareDisposition::Wait
        );
        assert_eq!(
            manager.cleanup_ack(&ticket, CleanupOutcome::NoChildCreated),
            CleanupAcknowledgment::Ignore
        );
        assert_eq!(
            manager.failed(due, &ticket, FailureKind::Retryable),
            CleanupAction::DisposeProvisional(ticket.clone())
        );
        assert_eq!(
            manager.try_prepare(due, complete(due, &context, &[])),
            PrepareDisposition::Wait
        );
        assert_eq!(
            manager.cleanup_ack(&ticket, CleanupOutcome::NoChildCreated),
            CleanupAcknowledgment::Released
        );
    }
    assert_eq!(
        manager.state(),
        KeepRunningState::Paused(PauseReason::RetryBudgetExhausted)
    );
    manager.observe(at(now, 40), complete(at(now, 40), &context, &[key(30, 1)]));
    manager.enable();
    assert_eq!(
        manager.state(),
        KeepRunningState::Paused(PauseReason::RetryBudgetExhausted)
    );
}

#[test]
fn external_replacement_cancels_preparation_without_immediately_resetting_budget() {
    let now = Instant::now();
    let context = context("desktop-a");
    let mut manager = retrying(now, &context);
    let ticket = prepare(&mut manager, at(now, 1), &context);
    let external = key(30, 1);
    manager.observe(
        at(now, 2),
        complete(at(now, 2), &context, std::slice::from_ref(&external)),
    );
    assert_eq!(manager.state(), KeepRunningState::Armed);
    assert_eq!(manager.attempts, 1);
    assert_eq!(
        manager.prepared(at(now, 10), &ticket),
        PreparedDisposition::DisposeProvisional
    );
    assert_eq!(
        manager.prepared(at(now, 10), &ticket),
        PreparedDisposition::Ignore
    );
    assert_eq!(
        manager.cleanup_ack(&ticket, CleanupOutcome::Disposed),
        CleanupAcknowledgment::Released
    );
    manager.observe(
        at(now, 61),
        complete(at(now, 61), &context, std::slice::from_ref(&external)),
    );
    assert_eq!(manager.attempts, 1);
    manager.observe(
        at(now, 62),
        complete(at(now, 62), &context, std::slice::from_ref(&external)),
    );
    assert_eq!(manager.attempts, 0);
    manager.exited(at(now, 63), &external);
    assert_eq!(
        manager.state(),
        KeepRunningState::Retrying {
            next_attempt: at(now, 64)
        }
    );
}

#[test]
fn short_lived_accepted_launches_exhaust_the_same_episode_until_explicit_resume() {
    let now = Instant::now();
    let context = context("desktop-a");
    let mut manager = retrying(now, &context);
    for (index, seconds) in [1, 6, 36].into_iter().enumerate() {
        let time = at(now, seconds);
        let ticket = prepare(&mut manager, time, &context);
        commit(&mut manager, time, &context, &ticket);
        let child = key(20, index as u64 + 2);
        assert_eq!(
            manager.accepted(&ticket, child.clone()),
            AcceptanceDisposition::Accepted
        );
        assert_eq!(manager.attempts, index + 1);
        manager.exited(time, &child);
    }
    assert_eq!(
        manager.state(),
        KeepRunningState::Paused(PauseReason::RetryBudgetExhausted)
    );
    assert_eq!(
        manager.try_prepare(at(now, 40), complete(at(now, 40), &context, &[])),
        PrepareDisposition::Wait
    );
    let external = key(40, 1);
    for seconds in [40, 140] {
        manager.observe(
            at(now, seconds),
            complete(at(now, seconds), &context, std::slice::from_ref(&external)),
        );
        manager.enable();
        assert_eq!(manager.attempts, 3);
        assert_eq!(
            manager.state(),
            KeepRunningState::Paused(PauseReason::RetryBudgetExhausted)
        );
    }
    manager.resume(at(now, 140), Some(&context));
    assert_eq!(manager.state(), KeepRunningState::Armed);
    manager.exited(at(now, 141), &external);
    assert_eq!(
        manager.state(),
        KeepRunningState::Retrying {
            next_attempt: at(now, 142)
        }
    );
    prepare(&mut manager, at(now, 142), &context);
    assert_eq!(manager.attempts, 1);
}

#[test]
fn accepting_committed_child_preserves_verified_overlapping_external_runtime() {
    let now = Instant::now();
    let context = context("desktop-a");
    let mut manager = retrying(now, &context);
    let ticket = prepare(&mut manager, at(now, 1), &context);
    commit(&mut manager, at(now, 1), &context, &ticket);
    let external = key(40, 1);
    manager.observe(
        at(now, 2),
        complete(at(now, 2), &context, std::slice::from_ref(&external)),
    );
    manager.observe(
        at(now, 32),
        complete(at(now, 32), &context, std::slice::from_ref(&external)),
    );
    let child = key(20, 2);
    assert_eq!(
        manager.accepted(&ticket, child.clone()),
        AcceptanceDisposition::Accepted
    );
    assert_eq!(manager.stable_credit, Duration::from_secs(30));
    assert_eq!(manager.attempts, 1);
    manager.observe(
        at(now, 62),
        complete(at(now, 62), &context, &[external, child]),
    );
    assert_eq!(manager.attempts, 0);
}

#[test]
fn partial_and_unknown_intervals_freeze_credit_without_counting_the_gap() {
    let now = Instant::now();
    let context = context("desktop-a");
    let (mut manager, child) = launched(now, &context);
    manager.observe(
        at(now, 2),
        complete(at(now, 2), &context, std::slice::from_ref(&child)),
    );
    manager.observe(
        at(now, 32),
        complete(at(now, 32), &context, std::slice::from_ref(&child)),
    );
    for observation in [Observation::Partial, Observation::Unavailable] {
        manager.observe(at(now, 62), observation);
        assert_eq!(manager.stable_credit, Duration::from_secs(30));
        assert_eq!(manager.attempts, 1);
    }
    manager.observe(
        at(now, 122),
        complete(at(now, 122), &context, std::slice::from_ref(&child)),
    );
    assert_eq!(manager.stable_credit, Duration::from_secs(30));
    manager.observe(
        at(now, 151),
        complete(at(now, 151), &context, std::slice::from_ref(&child)),
    );
    assert_eq!(manager.attempts, 1);
    manager.observe(
        at(now, 152),
        complete(at(now, 152), &context, std::slice::from_ref(&child)),
    );
    assert_eq!(manager.attempts, 0);
}

#[test]
fn full_verified_cohort_replacement_resets_credit_without_dropping_retained_members() {
    let now = Instant::now();
    let context = context("desktop-a");
    let (mut manager, child) = launched(now, &context);
    manager.observe(
        at(now, 2),
        complete(at(now, 2), &context, std::slice::from_ref(&child)),
    );
    manager.observe(
        at(now, 32),
        complete(at(now, 32), &context, std::slice::from_ref(&child)),
    );
    let replacement = key(child.id, 99);
    manager.observe(
        at(now, 42),
        complete(at(now, 42), &context, std::slice::from_ref(&replacement)),
    );
    assert_eq!(manager.stable_credit, Duration::ZERO);
    assert_eq!(manager.members.len(), 2);
    manager.observe(
        at(now, 101),
        complete(at(now, 101), &context, std::slice::from_ref(&replacement)),
    );
    assert_eq!(manager.attempts, 1);
    manager.exited(at(now, 101), &child);
    manager.exited(at(now, 101), &replacement);
    assert_eq!(manager.stable_credit, Duration::ZERO);
    assert_eq!(
        manager.state(),
        KeepRunningState::Retrying {
            next_attempt: at(now, 106)
        }
    );
}

#[test]
fn desktop_binding_is_independent_of_an_identical_process_key() {
    let now = Instant::now();
    let first = context("desktop-a");
    let second = context("desktop-b");
    let member = key(10, 1);
    let mut manager = armed(now, &first, std::slice::from_ref(&member));
    manager.observe(
        at(now, 1),
        complete(at(now, 1), &second, std::slice::from_ref(&member)),
    );
    assert_eq!(
        manager.state(),
        KeepRunningState::Paused(PauseReason::SessionChanged)
    );
    assert!(manager.binding.is_none());
    manager.resume(at(now, 2), Some(&second));
    assert_eq!(manager.state(), KeepRunningState::WaitingForApp);
    assert_eq!(
        manager.try_prepare(at(now, 3), complete(at(now, 3), &second, &[])),
        PrepareDisposition::Wait
    );
    manager.observe(at(now, 4), complete(at(now, 4), &second, &[member]));
    assert_eq!(manager.state(), KeepRunningState::Armed);
}

#[test]
fn timed_out_work_remains_owned_across_disable_resume_edit_and_new_observations() {
    let now = Instant::now();
    let context = context("desktop-a");
    let mut manager = retrying(now, &context);
    let ticket = prepare(&mut manager, at(now, 1), &context);
    assert_eq!(manager.timeout(at(now, 5), &ticket), CleanupAction::None);
    manager.timeout(at(now, 6), &ticket);
    assert_eq!(
        manager.state(),
        KeepRunningState::Paused(PauseReason::PreparationTimedOut)
    );
    assert_eq!(
        manager.cleanup_ack(&ticket, CleanupOutcome::Unknown),
        CleanupAcknowledgment::Retained
    );
    manager.disable();
    manager.resume(at(now, 7), Some(&context));
    assert_eq!(manager.state(), KeepRunningState::Disabled);
    manager.enable();
    let external = key(30, 1);
    manager.observe(
        at(now, 8),
        complete(at(now, 8), &context, std::slice::from_ref(&external)),
    );
    manager.pause(PauseReason::Suspend);
    manager.resume(at(now, 9), Some(&context));
    manager.launch_settings_changed();
    manager.observe(
        at(now, 10),
        complete(at(now, 10), &context, std::slice::from_ref(&external)),
    );
    assert_eq!(
        manager.prepared(at(now, 11), &ticket),
        PreparedDisposition::DisposeProvisional
    );
    assert_eq!(
        manager.prepared(at(now, 12), &ticket),
        PreparedDisposition::Ignore
    );
    manager.exited(at(now, 12), &external);
    assert_eq!(
        manager.try_prepare(at(now, 13), complete(at(now, 13), &context, &[])),
        PrepareDisposition::Wait
    );
    assert_eq!(
        manager.cleanup_ack(&ticket, CleanupOutcome::Disposed),
        CleanupAcknowledgment::Released
    );
    assert_eq!(
        manager.cleanup_ack(&ticket, CleanupOutcome::Disposed),
        CleanupAcknowledgment::Ignore
    );
    let next = prepare(&mut manager, at(now, 13), &context);
    assert_ne!(ticket, next);
    assert_eq!(
        manager.cleanup_ack(&ticket, CleanupOutcome::Disposed),
        CleanupAcknowledgment::Ignore
    );
    assert!(manager.attempt.is_some());
    assert_eq!(manager.timeout(at(now, 20), &ticket), CleanupAction::None);
}

#[test]
fn tickets_from_other_rule_owners_cannot_commit_cancel_accept_or_acknowledge_work() {
    let now = Instant::now();
    let context = context("desktop-a");
    let mut first = retrying(now, &context);
    let mut second = retrying(now, &context);
    let first_ticket = prepare(&mut first, at(now, 1), &context);
    let second_ticket = prepare(&mut second, at(now, 1), &context);
    assert_ne!(first_ticket, second_ticket);
    assert_eq!(
        first.prepared(at(now, 1), &second_ticket),
        PreparedDisposition::Ignore
    );
    assert_eq!(
        first.try_reserve_commit(
            at(now, 1),
            &second_ticket,
            complete(at(now, 1), &context, &[]),
            true
        ),
        CommitDisposition::Ignore
    );
    assert_eq!(
        first.failed(at(now, 1), &second_ticket, FailureKind::Retryable),
        CleanupAction::None
    );
    assert_eq!(
        first.accepted(&second_ticket, key(90, 1)),
        AcceptanceDisposition::Ignore
    );
    first.pause(PauseReason::IntentionalStop);
    assert_eq!(
        first.cleanup_ack(&second_ticket, CleanupOutcome::NoChildCreated),
        CleanupAcknowledgment::Ignore
    );
    assert_eq!(
        first.cleanup_ack(&first_ticket, CleanupOutcome::NoChildCreated),
        CleanupAcknowledgment::Released
    );
    assert!(second.attempt.is_some());
}

#[test]
fn destructive_intents_dispose_ready_work_before_commit_and_do_not_rearm_on_enable() {
    let now = Instant::now();
    let context = context("desktop-a");
    for reason in [
        PauseReason::IntentionalStop,
        PauseReason::Watchdog,
        PauseReason::Suspend,
    ] {
        let mut manager = retrying(now, &context);
        let ticket = prepare(&mut manager, at(now, 1), &context);
        assert_eq!(
            manager.prepared(at(now, 1), &ticket),
            PreparedDisposition::Ready
        );
        assert_eq!(
            manager.pause(reason),
            CleanupAction::DisposeProvisional(ticket.clone())
        );
        assert_eq!(manager.pause(reason), CleanupAction::None);
        assert_eq!(
            manager.try_reserve_commit(
                at(now, 1),
                &ticket,
                complete(at(now, 1), &context, &[]),
                true
            ),
            CommitDisposition::Ignore
        );
        assert_eq!(
            manager.prepared(at(now, 10), &ticket),
            PreparedDisposition::Ignore
        );
        manager.enable();
        assert_eq!(manager.state(), KeepRunningState::Paused(reason));
        manager.resume(at(now, 11), None);
        assert_eq!(manager.state(), KeepRunningState::Paused(reason));
        assert_eq!(
            manager.cleanup_ack(&ticket, CleanupOutcome::Disposed),
            CleanupAcknowledgment::Released
        );
    }
}

#[test]
fn commit_requires_fresh_complete_absence_matching_context_and_current_intent() {
    let now = Instant::now();
    let context = context("desktop-a");
    let mut manager = retrying(now, &context);
    let ticket = prepare(&mut manager, at(now, 1), &context);
    assert_eq!(
        manager.prepared(at(now, 1), &ticket),
        PreparedDisposition::Ready
    );
    for observation in [
        Observation::Partial,
        Observation::Unavailable,
        complete(at(now, 1), &context, &[]),
    ] {
        assert_eq!(
            manager.try_reserve_commit(at(now, 2), &ticket, observation, true),
            CommitDisposition::Deferred
        );
    }
    assert_eq!(
        manager.try_reserve_commit(
            at(now, 2),
            &ticket,
            complete(at(now, 2), &context, &[]),
            false
        ),
        CommitDisposition::Deferred
    );
    assert_eq!(
        manager.cleanup_ack(&ticket, CleanupOutcome::Disposed),
        CleanupAcknowledgment::Ignore
    );
    assert_eq!(
        manager.try_reserve_commit(
            at(now, 2),
            &ticket,
            complete(at(now, 2), &context, &[]),
            true
        ),
        CommitDisposition::Reserved
    );
    assert_eq!(
        manager.try_reserve_commit(
            at(now, 2),
            &ticket,
            complete(at(now, 2), &context, &[]),
            true
        ),
        CommitDisposition::Ignore
    );
    assert_eq!(
        manager.cleanup_ack(&ticket, CleanupOutcome::Disposed),
        CleanupAcknowledgment::Ignore
    );
    manager.resume(at(now, 3), Some(&context));
    assert_eq!(manager.attempts, 1);
}

#[test]
fn compatible_external_instance_at_final_check_disposes_prepared_child_once() {
    let now = Instant::now();
    let context = context("desktop-a");
    let mut manager = retrying(now, &context);
    let ticket = prepare(&mut manager, at(now, 1), &context);
    manager.prepared(at(now, 1), &ticket);
    let external = key(40, 1);
    assert_eq!(
        manager.try_reserve_commit(
            at(now, 2),
            &ticket,
            complete(at(now, 2), &context, &[external]),
            true
        ),
        CommitDisposition::DisposeProvisional
    );
    assert_eq!(manager.state(), KeepRunningState::Armed);
    assert_eq!(manager.attempts, 1);
    assert_eq!(
        manager.prepared(at(now, 2), &ticket),
        PreparedDisposition::Ignore
    );
    assert_eq!(
        manager.cleanup_ack(&ticket, CleanupOutcome::Disposed),
        CleanupAcknowledgment::Released
    );
}

#[test]
fn accepted_committed_work_is_never_disposed_by_later_pause_edit_disable_or_results() {
    let now = Instant::now();
    let context = context("desktop-a");
    for mutation in 0..3 {
        let mut manager = retrying(now, &context);
        let ticket = prepare(&mut manager, at(now, 1), &context);
        commit(&mut manager, at(now, 1), &context, &ticket);
        let action = match mutation {
            0 => manager.pause(PauseReason::IntentionalStop),
            1 => manager.launch_settings_changed(),
            _ => manager.disable(),
        };
        assert_eq!(action, CleanupAction::None);
        let state = manager.state();
        assert_eq!(
            manager.prepared(at(now, 10), &ticket),
            PreparedDisposition::Ignore
        );
        assert_eq!(manager.timeout(at(now, 10), &ticket), CleanupAction::None);
        assert_eq!(
            manager.accepted(&ticket, key(20, 2)),
            AcceptanceDisposition::Accepted
        );
        assert_eq!(manager.state(), state);
        assert_eq!(
            manager.accepted(&ticket, key(20, 2)),
            AcceptanceDisposition::Ignore
        );
        assert_eq!(
            manager.failed(at(now, 10), &ticket, FailureKind::Retryable),
            CleanupAction::None
        );
        assert_eq!(
            manager.cleanup_ack(&ticket, CleanupOutcome::Disposed),
            CleanupAcknowledgment::Ignore
        );
    }
}

#[test]
fn native_failure_after_commit_still_requires_cleanup_even_after_pause_and_resume() {
    let now = Instant::now();
    let context = context("desktop-a");
    let mut manager = retrying(now, &context);
    let ticket = prepare(&mut manager, at(now, 1), &context);
    commit(&mut manager, at(now, 1), &context, &ticket);
    manager.pause(PauseReason::Watchdog);
    assert_eq!(
        manager.failed(at(now, 2), &ticket, FailureKind::Retryable),
        CleanupAction::DisposeProvisional(ticket.clone())
    );
    assert_eq!(
        manager.cleanup_ack(&ticket, CleanupOutcome::Unknown),
        CleanupAcknowledgment::Retained
    );
    manager.resume(at(now, 2), Some(&context));
    assert_eq!(
        manager.try_prepare(at(now, 3), complete(at(now, 3), &context, &[])),
        PrepareDisposition::Wait
    );
    assert_eq!(
        manager.accepted(&ticket, key(30, 1)),
        AcceptanceDisposition::Ignore
    );
    assert_eq!(
        manager.cleanup_ack(&ticket, CleanupOutcome::Disposed),
        CleanupAcknowledgment::Released
    );
    prepare(&mut manager, at(now, 3), &context);
}

#[test]
fn late_duplicate_prepared_notification_delivers_timeout_cleanup_exactly_once() {
    let now = Instant::now();
    let context = context("desktop-a");
    let mut manager = retrying(now, &context);
    let ticket = prepare(&mut manager, at(now, 1), &context);
    assert_eq!(
        manager.prepared(at(now, 1), &ticket),
        PreparedDisposition::Ready
    );
    assert_eq!(
        manager.prepared(at(now, 6), &ticket),
        PreparedDisposition::DisposeProvisional
    );
    assert_eq!(
        manager.state(),
        KeepRunningState::Paused(PauseReason::PreparationTimedOut)
    );
    assert_eq!(
        manager.prepared(at(now, 7), &ticket),
        PreparedDisposition::Ignore
    );
    assert_eq!(manager.timeout(at(now, 8), &ticket), CleanupAction::None);
    assert_eq!(
        manager.cleanup_ack(&ticket, CleanupOutcome::Disposed),
        CleanupAcknowledgment::Released
    );
}

#[test]
fn late_precommit_failure_obeys_deadline_even_before_timer_dispatch() {
    let now = Instant::now();
    let context = context("desktop-a");
    for ready in [false, true] {
        let mut manager = retrying(now, &context);
        let ticket = prepare(&mut manager, at(now, 1), &context);
        if ready {
            assert_eq!(
                manager.prepared(at(now, 1), &ticket),
                PreparedDisposition::Ready
            );
        }
        assert_eq!(
            manager.failed(at(now, 6), &ticket, FailureKind::Retryable),
            CleanupAction::DisposeProvisional(ticket.clone())
        );
        assert_eq!(
            manager.state(),
            KeepRunningState::Paused(PauseReason::PreparationTimedOut)
        );
        assert_eq!(
            manager.failed(at(now, 7), &ticket, FailureKind::Retryable),
            CleanupAction::None
        );
        assert_eq!(
            manager.cleanup_ack(&ticket, CleanupOutcome::NoChildCreated),
            CleanupAcknowledgment::Released
        );
        assert_eq!(
            manager.try_prepare(at(now, 100), complete(at(now, 100), &context, &[])),
            PrepareDisposition::Wait
        );
    }
}

#[test]
fn launch_edit_requires_new_observation_and_security_failure_pauses_without_retrying() {
    let now = Instant::now();
    let context = context("desktop-a");
    let mut manager = retrying(now, &context);
    let ticket = prepare(&mut manager, at(now, 1), &context);
    manager.prepared(at(now, 1), &ticket);
    assert_eq!(
        manager.launch_settings_changed(),
        CleanupAction::DisposeProvisional(ticket.clone())
    );
    assert_eq!(
        manager.try_prepare(at(now, 2), complete(at(now, 2), &context, &[])),
        PrepareDisposition::Wait
    );
    assert_eq!(manager.state(), KeepRunningState::WaitingForApp);
    manager.cleanup_ack(&ticket, CleanupOutcome::Disposed);
    let fresh = key(50, 1);
    manager.observe(
        at(now, 3),
        complete(at(now, 3), &context, std::slice::from_ref(&fresh)),
    );
    manager.exited(at(now, 3), &fresh);
    let next = prepare(&mut manager, at(now, 4), &context);
    assert_eq!(
        manager.failed(at(now, 4), &next, FailureKind::UnsupportedContext),
        CleanupAction::DisposeProvisional(next.clone())
    );
    manager.cleanup_ack(&next, CleanupOutcome::NoChildCreated);
    assert_eq!(
        manager.state(),
        KeepRunningState::Paused(PauseReason::UnsupportedContext)
    );
    assert_eq!(
        manager.try_prepare(at(now, 100), complete(at(now, 100), &context, &[])),
        PrepareDisposition::Wait
    );
}
