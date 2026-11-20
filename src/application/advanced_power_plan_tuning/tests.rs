use std::sync::{Arc, Mutex};

use crate::power::{
    PowerSettingReadback, ProcessorBoostMode, ProcessorPowerApplyStage, ProcessorPowerValues,
    ProcessorSchedulingPolicy,
};

use super::*;

#[derive(Clone)]
struct FakeStore {
    state: Arc<Mutex<FakeStoreState>>,
}

#[derive(Debug, Clone)]
struct FakeStoreState {
    values: Result<AdvancedPowerPlanTuningReadback, AdvancedPowerPlanTuningError>,
    personality: Result<PowerPlanPersonality, AdvancedPowerPlanTuningError>,
    apply_error: Option<ProcessorPowerApplyError>,
    apply_readback: Option<AdvancedPowerPlanTuningReadback>,
    requests: Vec<AdvancedPowerPlanTuningSourceValues>,
    calls: Vec<&'static str>,
}

impl FakeStore {
    fn new(values: impl Into<AdvancedPowerPlanTuningReadback>) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeStoreState {
                values: Ok(values.into()),
                personality: Ok(PowerPlanPersonality::Balanced),
                apply_error: None,
                apply_readback: None,
                requests: Vec::new(),
                calls: Vec::new(),
            })),
        }
    }

    fn calls(&self) -> Vec<&'static str> {
        self.state.lock().expect("power tuning lock").calls.clone()
    }
}

impl AdvancedPowerPlanTuningStore for FakeStore {
    fn read_values(
        &self,
        _guid: &str,
    ) -> Result<AdvancedPowerPlanTuningReadback, AdvancedPowerPlanTuningError> {
        let mut state = self.state.lock().expect("power tuning lock");
        state.calls.push("read");
        state.values.clone()
    }

    fn read_personality(
        &self,
        _guid: &str,
    ) -> Result<PowerPlanPersonality, AdvancedPowerPlanTuningError> {
        let mut state = self.state.lock().expect("power tuning lock");
        state.calls.push("personality");
        state.personality.clone()
    }

    fn apply_values(
        &self,
        _guid: &str,
        values: AdvancedPowerPlanTuningSourceValues,
    ) -> Result<(), ProcessorPowerApplyError> {
        let mut state = self.state.lock().expect("power tuning lock");
        state.calls.push("apply");
        state.requests.push(values);
        if let Some(readback) = state.apply_readback.take() {
            state.values = Ok(readback);
        }
        match state.apply_error.clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

fn values(percent: u32) -> AdvancedPowerPlanTuningSourceValues {
    AdvancedPowerPlanTuningSourceValues::same(
        ProcessorPowerValues::new_with_boost_mode(
            percent,
            percent,
            percent,
            percent,
            ProcessorBoostMode::EfficientEnabled,
        )
        .into(),
    )
}

#[test]
fn successful_apply_is_followed_by_readback() {
    let actual = values(75);
    let store = FakeStore::new(actual);
    let service = AdvancedPowerPlanTuningService::with_store(Box::new(store.clone()));

    let outcome = service.apply_values("plan", values(75));

    assert!(outcome.applied);
    assert_eq!(outcome.actual_values, Some(actual.into()));
    assert!(outcome.error.is_none());
    assert_eq!(store.calls(), vec!["apply", "read"]);
}

#[test]
fn reviewed_apply_rejects_stale_values_and_rechecks_before_writing() {
    let actual = values(75);
    let store = FakeStore::new(actual);
    let service = AdvancedPowerPlanTuningService::with_store(Box::new(store.clone()));

    assert_eq!(
        service.apply_reviewed_values("plan", values(70).into(), actual),
        Err(AdvancedPowerPlanTuningError::ValuesChanged)
    );
    assert_eq!(store.calls(), vec!["read"]);

    let outcome = service
        .apply_reviewed_values("plan", actual.into(), actual)
        .unwrap();
    assert!(outcome.applied);
    assert_eq!(store.calls(), vec!["read", "read", "apply", "read"]);

    let deleted_plan = AdvancedPowerPlanTuningError::ReadValues("Plan deleted".into());
    store.state.lock().expect("power tuning lock").values = Err(deleted_plan.clone());
    assert_eq!(
        service.apply_reviewed_values("plan", actual.into(), actual),
        Err(deleted_plan)
    );
    assert_eq!(store.calls(), vec!["read", "read", "apply", "read", "read"]);
}

#[test]
fn apply_requires_matching_values_for_both_power_sources() {
    let requested = values(75);
    for battery in [false, true] {
        for field in 0..7 {
            let mut expected = requested;
            let mut actual = requested;
            let expected_source = if battery {
                &mut expected.battery
            } else {
                &mut expected.ac
            };
            let actual_source = if battery {
                &mut actual.battery
            } else {
                &mut actual.ac
            };
            match field {
                0 => actual_source.processor.core_parking_min = 70,
                1 => actual_source.processor.performance_min = 70,
                2 => actual_source.processor.performance_max = 101,
                3 => actual_source.processor.boost_policy = 70,
                4 => actual_source.processor.boost_mode = Some(ProcessorBoostMode::Disabled),
                5 => {
                    expected_source.scheduling_policy = Some(ProcessorSchedulingPolicy::Automatic);
                    actual_source.scheduling_policy =
                        Some(ProcessorSchedulingPolicy::EfficiencyProcessors);
                }
                _ => {
                    expected_source.short_scheduling_policy =
                        Some(ProcessorSchedulingPolicy::Automatic)
                }
            }
            let service =
                AdvancedPowerPlanTuningService::with_store(Box::new(FakeStore::new(actual)));
            let outcome = service.apply_values("plan", expected);
            assert!(!outcome.applied, "source={battery}, field={field}");
            assert_eq!(outcome.actual_values, Some(actual.into()));
            assert!(matches!(
                outcome.error,
                Some(AdvancedPowerPlanTuningApplyError::Verification)
            ));
        }
    }
}

#[test]
fn apply_verifies_requested_settings_but_preserves_unowned_fields() {
    let mut actual = values(75);
    actual.ac.scheduling_policy = Some(ProcessorSchedulingPolicy::PerformanceProcessors);
    actual.battery.short_scheduling_policy = Some(ProcessorSchedulingPolicy::Automatic);
    let service = AdvancedPowerPlanTuningService::with_store(Box::new(FakeStore::new(actual)));
    let mut unowned = values(75);
    unowned.ac.processor.boost_mode = None;
    for requested in [unowned, actual] {
        let outcome = service.apply_values("plan", requested);
        assert!(outcome.applied);
        assert_eq!(outcome.actual_values, Some(actual.into()));
        assert!(outcome.error.is_none());
    }
}

#[test]
fn reviewed_apply_preserves_unknown_ac_policy_and_can_edit_battery_policy() {
    let mut current = AdvancedPowerPlanTuningReadback::from(values(75));
    current.ac.scheduling_policy = PowerSettingReadback::Unknown(42);
    current.battery.scheduling_policy =
        PowerSettingReadback::Known(ProcessorSchedulingPolicy::Automatic);
    let mut actual = current;
    actual.battery.scheduling_policy =
        PowerSettingReadback::Known(ProcessorSchedulingPolicy::EfficiencyProcessors);
    let store = FakeStore::new(current);
    store
        .state
        .lock()
        .expect("power tuning lock")
        .apply_readback = Some(actual);
    let service = AdvancedPowerPlanTuningService::with_store(Box::new(store.clone()));
    let requested = actual.writable();

    let outcome = service
        .apply_reviewed_values("plan", current, requested)
        .unwrap();

    assert_eq!(requested.ac.scheduling_policy, None);
    assert!(outcome.applied);
    assert_eq!(outcome.actual_values, Some(actual));
    assert_eq!(store.calls(), vec!["read", "apply", "read"]);
}

#[test]
fn reviewed_apply_rejects_changed_or_unavailable_scheduling_policy() {
    let mut current = AdvancedPowerPlanTuningReadback::from(values(75));
    current.ac.scheduling_policy = PowerSettingReadback::Unknown(42);
    let store = FakeStore::new(current);
    let service = AdvancedPowerPlanTuningService::with_store(Box::new(store.clone()));
    let mut stale = current;
    stale.ac.scheduling_policy = PowerSettingReadback::Unknown(43);
    assert_eq!(
        service.apply_reviewed_values("plan", stale, current.writable()),
        Err(AdvancedPowerPlanTuningError::ValuesChanged)
    );

    let mut requested = current.writable();
    requested.ac.scheduling_policy = Some(ProcessorSchedulingPolicy::Automatic);
    assert_eq!(
        service.apply_reviewed_values("plan", current, requested),
        Err(AdvancedPowerPlanTuningError::SettingUnavailable)
    );
    assert_eq!(store.calls(), vec!["read", "read"]);
}

#[test]
fn apply_and_restore_preserve_unknown_ac_boost_mode() {
    let mut original = AdvancedPowerPlanTuningReadback::from(values(75));
    original.ac.processor.boost_mode = PowerSettingReadback::Unknown(42);
    let mut edited = original;
    edited.battery.processor.boost_mode = PowerSettingReadback::Known(ProcessorBoostMode::Disabled);
    let store = FakeStore::new(original);
    let service = AdvancedPowerPlanTuningService::with_store(Box::new(store.clone()));

    let mut stale = original;
    stale.ac.processor.boost_mode = PowerSettingReadback::Unknown(43);
    assert_eq!(
        service.apply_reviewed_values("plan", stale, edited.writable()),
        Err(AdvancedPowerPlanTuningError::ValuesChanged)
    );
    let mut unavailable = edited.writable();
    unavailable.ac.processor.boost_mode = Some(ProcessorBoostMode::Enabled);
    assert_eq!(
        service.apply_reviewed_values("plan", original, unavailable),
        Err(AdvancedPowerPlanTuningError::SettingUnavailable)
    );

    for (reviewed, expected) in [(original, edited), (edited, original)] {
        store
            .state
            .lock()
            .expect("power tuning lock")
            .apply_readback = Some(expected);
        let requested = expected.writable();
        assert_eq!(requested.ac.processor.boost_mode, None);
        let outcome = service
            .apply_reviewed_values("plan", reviewed, requested)
            .unwrap();
        assert!(outcome.applied);
        assert_eq!(outcome.actual_values, Some(expected));
    }
    assert_eq!(
        store.state.lock().expect("power tuning lock").requests,
        [edited.writable(), original.writable()]
    );
}

#[test]
fn successful_write_with_failed_readback_is_not_verified() {
    let store = FakeStore::new(values(75));
    store.state.lock().expect("power tuning lock").values = Err(
        AdvancedPowerPlanTuningError::ReadValues("read failed".to_owned()),
    );
    let service = AdvancedPowerPlanTuningService::with_store(Box::new(store));
    let outcome = service.apply_values("plan", values(75));
    assert!(!outcome.applied);
    assert!(outcome.actual_values.is_none());
    assert!(matches!(
        outcome.error,
        Some(AdvancedPowerPlanTuningApplyError::Readback(_))
    ));
}

#[test]
fn partial_apply_failure_still_returns_actual_values_and_stage() {
    for stage in [
        ProcessorPowerApplyStage::BatteryPerformanceMaximum,
        ProcessorPowerApplyStage::AcSchedulingPolicy,
        ProcessorPowerApplyStage::BatterySchedulingPolicy,
        ProcessorPowerApplyStage::AcShortSchedulingPolicy,
        ProcessorPowerApplyStage::BatteryShortSchedulingPolicy,
    ] {
        let mut actual = values(30);
        actual.ac.scheduling_policy = Some(ProcessorSchedulingPolicy::AllProcessors);
        actual.battery.short_scheduling_policy = Some(ProcessorSchedulingPolicy::Automatic);
        let store = FakeStore::new(actual);
        store.state.lock().expect("power tuning lock").apply_error = Some(
            ProcessorPowerApplyError::at(stage, "write failed".to_owned()),
        );
        let service = AdvancedPowerPlanTuningService::with_store(Box::new(store.clone()));
        let outcome = service.apply_values("plan", values(80));
        assert!(!outcome.applied);
        assert_eq!(outcome.actual_values, Some(actual.into()));
        assert!(
            matches!(outcome.error, Some(AdvancedPowerPlanTuningApplyError::Apply(ref error)) if error.stage() == stage)
        );
        assert_eq!(store.calls(), vec!["apply", "read"]);
    }
}

#[test]
fn apply_and_readback_failures_are_both_retained() {
    let store = FakeStore::new(values(30));
    {
        let mut state = store.state.lock().expect("power tuning lock");
        state.apply_error = Some(ProcessorPowerApplyError::at(
            ProcessorPowerApplyStage::AcBoostMode,
            "write failed".to_owned(),
        ));
        state.values = Err(AdvancedPowerPlanTuningError::ReadValues(
            "read failed".to_owned(),
        ));
    }
    let service = AdvancedPowerPlanTuningService::with_store(Box::new(store));

    let outcome = service.apply_values("plan", values(80));

    assert!(!outcome.applied);
    assert!(outcome.actual_values.is_none());
    assert_eq!(
        outcome.error.as_ref().map(ToString::to_string).as_deref(),
        Some("Failed to write A/C boost mode: write failed Reading the actual values after the partial apply also failed: Failed to read processor power values: read failed")
    );
    assert!(matches!(
        outcome.error,
        Some(AdvancedPowerPlanTuningApplyError::ApplyAndReadback { .. })
    ));
}

#[test]
fn read_errors_remain_typed_by_operation() {
    let store = FakeStore::new(values(30));
    store.state.lock().expect("power tuning lock").personality = Err(
        AdvancedPowerPlanTuningError::ReadPersonality("unsupported".to_owned()),
    );
    let service = AdvancedPowerPlanTuningService::with_store(Box::new(store));

    assert!(matches!(
        service.read_personality("plan"),
        Err(AdvancedPowerPlanTuningError::ReadPersonality(_))
    ));
}
