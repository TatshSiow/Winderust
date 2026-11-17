use std::sync::{Arc, Mutex};

use crate::power::{ProcessorBoostMode, ProcessorPowerApplyStage, ProcessorPowerValues};

use super::*;

#[derive(Clone)]
struct FakeStore {
    state: Arc<Mutex<FakeStoreState>>,
}

#[derive(Debug, Clone)]
struct FakeStoreState {
    values: Result<AdvancedPowerPlanTuningSourceValues, AdvancedPowerPlanTuningError>,
    personality: Result<PowerPlanPersonality, AdvancedPowerPlanTuningError>,
    apply_error: Option<ProcessorPowerApplyError>,
    calls: Vec<&'static str>,
}

impl FakeStore {
    fn new(values: AdvancedPowerPlanTuningSourceValues) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeStoreState {
                values: Ok(values),
                personality: Ok(PowerPlanPersonality::Balanced),
                apply_error: None,
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
    ) -> Result<AdvancedPowerPlanTuningSourceValues, AdvancedPowerPlanTuningError> {
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
        _values: AdvancedPowerPlanTuningSourceValues,
    ) -> Result<(), ProcessorPowerApplyError> {
        let mut state = self.state.lock().expect("power tuning lock");
        state.calls.push("apply");
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
    assert_eq!(outcome.actual_values, Some(actual));
    assert!(outcome.error.is_none());
    assert_eq!(store.calls(), vec!["apply", "read"]);
}

#[test]
fn apply_requires_matching_values_for_both_power_sources() {
    use crate::power::ProcessorSchedulingPolicy;

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
                4 => actual_source.processor.boost_mode = ProcessorBoostMode::Disabled,
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
            assert_eq!(outcome.actual_values, Some(actual));
            assert!(matches!(
                outcome.error,
                Some(AdvancedPowerPlanTuningApplyError::Verification)
            ));
        }
    }
}

#[test]
fn apply_verifies_requested_scheduling_but_preserves_unowned_fields() {
    use crate::power::ProcessorSchedulingPolicy;

    let mut actual = values(75);
    actual.ac.scheduling_policy = Some(ProcessorSchedulingPolicy::PerformanceProcessors);
    actual.battery.short_scheduling_policy = Some(ProcessorSchedulingPolicy::Automatic);
    let service = AdvancedPowerPlanTuningService::with_store(Box::new(FakeStore::new(actual)));
    for requested in [values(75), actual] {
        let outcome = service.apply_values("plan", requested);
        assert!(outcome.applied);
        assert_eq!(outcome.actual_values, Some(actual));
        assert!(outcome.error.is_none());
    }
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
    use crate::power::ProcessorSchedulingPolicy;
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
        assert_eq!(outcome.actual_values, Some(actual));
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
