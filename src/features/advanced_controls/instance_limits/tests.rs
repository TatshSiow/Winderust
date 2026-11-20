use super::*;
use crate::config::InstanceLimitsRule;

#[test]
fn duplicate_and_out_of_order_samples_break_qualification() {
    let mut manager = manager();
    let now = Instant::now();
    manager.observe(&[CountObservation::Complete(2)], now);
    manager.observe(&[CountObservation::Complete(2)], now);
    assert!(manager
        .observe(
            &[CountObservation::Complete(2)],
            now + Duration::from_secs(1)
        )
        .is_empty());
    manager.observe(&[CountObservation::Complete(2)], now);
    assert!(manager
        .observe(
            &[CountObservation::Complete(2)],
            now + Duration::from_secs(2)
        )
        .is_empty());
    assert_eq!(
        manager.observe(
            &[CountObservation::Complete(2)],
            now + Duration::from_secs(3)
        ),
        vec![
            InstanceLimitsEvent::Excess(0),
            InstanceLimitsEvent::Notify(0)
        ]
    );
}

fn manager() -> InstanceLimitsManager {
    let mut manager = InstanceLimitsManager::default();
    manager.sync(
        &InstanceLimitsSettings {
            enabled: true,
            notify: true,
            rules: vec![InstanceLimitsRule::new(r"C:\Apps\app.exe".into())],
        },
        true,
    );
    manager
}

#[test]
fn stale_sample_gaps_restart_excess_and_recovery_qualification() {
    let mut manager = manager();
    let now = Instant::now();
    let counts = |manager: &mut InstanceLimitsManager, count, seconds| {
        manager.observe(
            &[CountObservation::Complete(count)],
            now + Duration::from_secs(seconds),
        )
    };
    assert!(counts(&mut manager, 2, 0).is_empty());
    assert!(counts(&mut manager, 2, 4).is_empty());
    assert_eq!(
        counts(&mut manager, 2, 7),
        vec![
            InstanceLimitsEvent::Excess(0),
            InstanceLimitsEvent::Notify(0)
        ]
    );
    manager.notification_result(0, Ok(())).unwrap();

    assert!(counts(&mut manager, 1, 8).is_empty());
    assert!(counts(&mut manager, 1, 12).is_empty());
    let snapshot = manager.snapshot(now + Duration::from_secs(12));
    assert!(snapshot.rules[0].incident);
    assert_eq!(snapshot.rules[0].notification, NotificationStatus::Accepted);
    assert_eq!(
        counts(&mut manager, 1, 15),
        vec![InstanceLimitsEvent::Recovered(0)]
    );
}

#[test]
fn reset_intents_clear_only_affected_rules_even_when_settings_revert() {
    let mut manager = manager();
    let mut settings = manager.settings.clone();
    settings
        .rules
        .push(InstanceLimitsRule::new(r"C:\Apps\other.exe".into()));
    manager.sync(&settings, true);
    let now = Instant::now();
    manager.observe(
        &[CountObservation::Complete(2), CountObservation::Complete(2)],
        now,
    );
    manager.observe(
        &[CountObservation::Complete(2), CountObservation::Complete(2)],
        now + Duration::from_secs(1),
    );
    manager.reset_paths(&[settings.rules[0].executable_path.clone()]);
    manager.sync(&settings, true);
    let snapshot = manager.snapshot(now);
    assert!(!snapshot.rules[0].incident);
    assert!(snapshot.rules[0].sampled_at.is_none());
    assert!(snapshot.rules[1].incident);
}

#[test]
fn lower_bound_excess_notifies_once_and_only_complete_samples_recover() {
    let mut manager = manager();
    let now = Instant::now();
    let excess = CountObservation::Incomplete {
        known_lower_bound: 2,
        reason: "A matching process is inaccessible.".into(),
    };
    assert!(manager
        .observe(std::slice::from_ref(&excess), now)
        .iter()
        .all(|event| matches!(event, InstanceLimitsEvent::Error(..))));
    assert!(manager
        .observe(
            std::slice::from_ref(&excess),
            now + Duration::from_millis(999)
        )
        .is_empty());
    assert_eq!(
        manager.observe(std::slice::from_ref(&excess), now + Duration::from_secs(1)),
        vec![
            InstanceLimitsEvent::Excess(0),
            InstanceLimitsEvent::Notify(0)
        ]
    );
    manager
        .notification_result(0, Err("Notification rejected.".into()))
        .unwrap();
    assert!(manager
        .observe(&[excess], now + Duration::from_secs(2))
        .is_empty());
    manager.observe(
        &[CountObservation::Complete(1)],
        now + Duration::from_secs(3),
    );
    manager.observe(
        &[CountObservation::Incomplete {
            known_lower_bound: 0,
            reason: "Unknown target.".into(),
        }],
        now + Duration::from_secs(4),
    );
    manager.observe(
        &[CountObservation::Complete(0)],
        now + Duration::from_secs(5),
    );
    assert!(manager.snapshot(now).rules[0].incident);
    assert_eq!(
        manager.observe(
            &[CountObservation::Complete(0)],
            now + Duration::from_secs(6)
        ),
        vec![InstanceLimitsEvent::Recovered(0)]
    );
    assert_eq!(
        manager.snapshot(now).rules[0].notification,
        NotificationStatus::NotAttempted
    );
}

#[test]
fn gaps_break_continuity_preserve_incidents_and_notification_toggles_do_not_rearm() {
    let mut manager = manager();
    let now = Instant::now();
    manager.observe(&[CountObservation::Complete(2)], now);
    manager.invalidate("Session changed.");
    assert!(manager
        .observe(
            &[CountObservation::Complete(2)],
            now + Duration::from_secs(2)
        )
        .is_empty());
    manager.observe(
        &[CountObservation::Complete(2)],
        now + Duration::from_secs(3),
    );
    manager.notification_result(0, Ok(())).unwrap();
    manager.invalidate("Suspended.");
    assert!(manager.snapshot(now).rules[0].incident);
    let mut settings = manager.settings.clone();
    settings.notify = false;
    manager.sync(&settings, true);
    settings.notify = true;
    manager.sync(&settings, true);
    manager.observe(
        &[CountObservation::Complete(2)],
        now + Duration::from_secs(5),
    );
    assert!(manager
        .observe(
            &[CountObservation::Complete(3)],
            now + Duration::from_secs(6)
        )
        .is_empty());
    assert_eq!(
        manager.snapshot(now).rules[0].notification,
        NotificationStatus::Accepted
    );
}

#[test]
fn edits_reset_only_changed_rules_and_master_disable_clears_state() {
    let mut manager = manager();
    let now = Instant::now();
    manager.observe(&[CountObservation::Complete(2)], now);
    manager.observe(
        &[CountObservation::Complete(2)],
        now + Duration::from_secs(1),
    );
    let mut settings = manager.settings.clone();
    settings
        .rules
        .push(InstanceLimitsRule::new(r"C:\Apps\other.exe".into()));
    settings.rules.swap(0, 1);
    manager.sync(&settings, true);
    assert!(manager.snapshot(now).rules[1].incident);
    settings.rules[1].max_instances = 2;
    manager.sync(&settings, true);
    assert!(!manager.snapshot(now).rules[1].incident);
    manager.sync(&settings, false);
    assert!(!manager.snapshot(now).enabled);
    assert!(manager
        .snapshot(now)
        .rules
        .iter()
        .all(|rule| !rule.incident && rule.sampled_at.is_none()));
}

#[test]
fn notification_opt_in_during_an_unattempted_incident_needs_fresh_proof() {
    let mut manager = manager();
    let mut settings = manager.settings.clone();
    settings.notify = false;
    manager.sync(&settings, true);
    let now = Instant::now();
    manager.observe(&[CountObservation::Complete(2)], now);
    assert_eq!(
        manager.observe(
            &[CountObservation::Complete(2)],
            now + Duration::from_secs(1)
        ),
        vec![InstanceLimitsEvent::Excess(0)]
    );
    settings.notify = true;
    manager.sync(&settings, true);
    assert!(manager
        .observe(
            &[CountObservation::Complete(2)],
            now + Duration::from_secs(10)
        )
        .is_empty());
    assert_eq!(
        manager.observe(
            &[CountObservation::Complete(2)],
            now + Duration::from_secs(11)
        ),
        vec![InstanceLimitsEvent::Notify(0)]
    );
}
