use super::*;

fn idle_runtime() -> RuntimeHandle {
    RuntimeHandle::start(&RuntimeSettingsSnapshot {
        runtime_revision: SettingsRevision::initial(),
        persisted_revision: SettingsRevision::initial(),
        value: Arc::new(Settings::default()),
    })
}

#[test]
fn self_power_success_clears_only_its_own_failure_evidence() {
    let runtime = idle_runtime();
    let previous = Mutex::new(None);
    report_self_power_result(
        &runtime.shared,
        &previous,
        Err("self-power failure".to_owned()),
    );
    assert_eq!(
        lock_unpoisoned(&runtime.shared.state)
            .status
            .worker_error
            .as_deref(),
        Some("self-power failure")
    );
    report_self_power_result(&runtime.shared, &previous, Ok(()));
    assert!(lock_unpoisoned(&runtime.shared.state)
        .status
        .worker_error
        .is_none());

    report_self_power_result(
        &runtime.shared,
        &previous,
        Err("self-power failure".to_owned()),
    );
    update_worker_error(&runtime.shared, Some("unrelated failure".to_owned()));
    report_self_power_result(&runtime.shared, &previous, Ok(()));
    assert_eq!(
        lock_unpoisoned(&runtime.shared.state)
            .status
            .worker_error
            .as_deref(),
        Some("unrelated failure")
    );
}

#[test]
fn self_power_retry_failure_preserves_an_unrelated_runtime_error() {
    let runtime = idle_runtime();
    let previous = Mutex::new(None);
    update_worker_error(&runtime.shared, Some("unrelated failure".to_owned()));
    report_self_power_result(
        &runtime.shared,
        &previous,
        Err("self-power failure".to_owned()),
    );
    assert_eq!(
        lock_unpoisoned(&runtime.shared.state)
            .status
            .worker_error
            .as_deref(),
        Some("unrelated failure")
    );
    assert_eq!(
        previous.lock().unwrap().as_deref(),
        Some("self-power failure")
    );
}
