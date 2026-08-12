use crate::power::{
    apply_processor_power_values_staged, read_plan_personality, read_processor_power_values,
    PowerPlanPersonality, ProcessorPowerAcDcValues, ProcessorPowerApplyError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AdvancedPowerPlanTuningError {
    ReadValues(String),
    ReadPersonality(String),
}

impl std::fmt::Display for AdvancedPowerPlanTuningError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadValues(error) => {
                write!(formatter, "Failed to read processor power values: {error}")
            }
            Self::ReadPersonality(error) => {
                write!(formatter, "Failed to read power plan personality: {error}")
            }
        }
    }
}

impl std::error::Error for AdvancedPowerPlanTuningError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AdvancedPowerPlanTuningApplyError {
    Apply(ProcessorPowerApplyError),
    Readback(AdvancedPowerPlanTuningError),
    ApplyAndReadback {
        apply: ProcessorPowerApplyError,
        readback: AdvancedPowerPlanTuningError,
    },
}

impl std::fmt::Display for AdvancedPowerPlanTuningApplyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Apply(error) => error.fmt(formatter),
            Self::Readback(error) => write!(
                formatter,
                "Processor power values were written, but readback failed: {error}"
            ),
            Self::ApplyAndReadback { apply, readback } => write!(
                formatter,
                "{apply} Reading the actual values after the partial apply also failed: {readback}"
            ),
        }
    }
}

impl std::error::Error for AdvancedPowerPlanTuningApplyError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdvancedPowerPlanTuningApplyOutcome {
    pub applied: bool,
    pub actual_values: Option<ProcessorPowerAcDcValues>,
    pub error: Option<AdvancedPowerPlanTuningApplyError>,
}

trait AdvancedPowerPlanTuningStore: Send + Sync {
    fn read_values(
        &self,
        guid: &str,
    ) -> Result<ProcessorPowerAcDcValues, AdvancedPowerPlanTuningError>;
    fn read_personality(
        &self,
        guid: &str,
    ) -> Result<PowerPlanPersonality, AdvancedPowerPlanTuningError>;
    fn apply_values(
        &self,
        guid: &str,
        values: ProcessorPowerAcDcValues,
    ) -> Result<(), ProcessorPowerApplyError>;
}

#[derive(Debug, Default)]
struct WindowsAdvancedPowerPlanTuningStore;

impl AdvancedPowerPlanTuningStore for WindowsAdvancedPowerPlanTuningStore {
    fn read_values(
        &self,
        guid: &str,
    ) -> Result<ProcessorPowerAcDcValues, AdvancedPowerPlanTuningError> {
        read_processor_power_values(guid).map_err(AdvancedPowerPlanTuningError::ReadValues)
    }

    fn read_personality(
        &self,
        guid: &str,
    ) -> Result<PowerPlanPersonality, AdvancedPowerPlanTuningError> {
        read_plan_personality(guid).map_err(AdvancedPowerPlanTuningError::ReadPersonality)
    }

    fn apply_values(
        &self,
        guid: &str,
        values: ProcessorPowerAcDcValues,
    ) -> Result<(), ProcessorPowerApplyError> {
        apply_processor_power_values_staged(guid, values)
    }
}

pub(crate) struct AdvancedPowerPlanTuningService {
    store: Box<dyn AdvancedPowerPlanTuningStore>,
}

impl Default for AdvancedPowerPlanTuningService {
    fn default() -> Self {
        Self {
            store: Box::<WindowsAdvancedPowerPlanTuningStore>::default(),
        }
    }
}

impl AdvancedPowerPlanTuningService {
    #[cfg(test)]
    fn with_store(store: Box<dyn AdvancedPowerPlanTuningStore>) -> Self {
        Self { store }
    }

    pub(crate) fn read_values(
        &self,
        guid: &str,
    ) -> Result<ProcessorPowerAcDcValues, AdvancedPowerPlanTuningError> {
        self.store
            .read_values(guid)
            .map(ProcessorPowerAcDcValues::normalized)
    }

    pub(crate) fn read_personality(
        &self,
        guid: &str,
    ) -> Result<PowerPlanPersonality, AdvancedPowerPlanTuningError> {
        self.store.read_personality(guid)
    }

    pub(crate) fn apply_values(
        &self,
        guid: &str,
        values: ProcessorPowerAcDcValues,
    ) -> AdvancedPowerPlanTuningApplyOutcome {
        let apply_error = self.store.apply_values(guid, values.normalized()).err();
        let readback = self.read_values(guid);
        let applied = apply_error.is_none();

        match (apply_error, readback) {
            (None, Ok(actual_values)) => AdvancedPowerPlanTuningApplyOutcome {
                applied,
                actual_values: Some(actual_values),
                error: None,
            },
            (Some(apply), Ok(actual_values)) => AdvancedPowerPlanTuningApplyOutcome {
                applied,
                actual_values: Some(actual_values),
                error: Some(AdvancedPowerPlanTuningApplyError::Apply(apply)),
            },
            (None, Err(readback)) => AdvancedPowerPlanTuningApplyOutcome {
                applied,
                actual_values: None,
                error: Some(AdvancedPowerPlanTuningApplyError::Readback(readback)),
            },
            (Some(apply), Err(readback)) => AdvancedPowerPlanTuningApplyOutcome {
                applied,
                actual_values: None,
                error: Some(AdvancedPowerPlanTuningApplyError::ApplyAndReadback {
                    apply,
                    readback,
                }),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::power::{ProcessorBoostMode, ProcessorPowerApplyStage, ProcessorPowerValues};

    use super::*;

    #[derive(Clone)]
    struct FakeStore {
        state: Arc<Mutex<FakeStoreState>>,
    }

    #[derive(Debug, Clone)]
    struct FakeStoreState {
        values: Result<ProcessorPowerAcDcValues, AdvancedPowerPlanTuningError>,
        personality: Result<PowerPlanPersonality, AdvancedPowerPlanTuningError>,
        apply_error: Option<ProcessorPowerApplyError>,
        calls: Vec<&'static str>,
    }

    impl FakeStore {
        fn new(values: ProcessorPowerAcDcValues) -> Self {
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
        ) -> Result<ProcessorPowerAcDcValues, AdvancedPowerPlanTuningError> {
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
            _values: ProcessorPowerAcDcValues,
        ) -> Result<(), ProcessorPowerApplyError> {
            let mut state = self.state.lock().expect("power tuning lock");
            state.calls.push("apply");
            match state.apply_error.clone() {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }
    }

    fn values(percent: u32) -> ProcessorPowerAcDcValues {
        ProcessorPowerAcDcValues::same(ProcessorPowerValues::new_with_boost_mode(
            percent,
            percent,
            percent,
            percent,
            ProcessorBoostMode::EfficientEnabled,
        ))
    }

    #[test]
    fn successful_apply_is_followed_by_readback() {
        let actual = values(42);
        let store = FakeStore::new(actual);
        let service = AdvancedPowerPlanTuningService::with_store(Box::new(store.clone()));

        let outcome = service.apply_values("plan", values(75));

        assert!(outcome.applied);
        assert_eq!(outcome.actual_values, Some(actual));
        assert!(outcome.error.is_none());
        assert_eq!(store.calls(), vec!["apply", "read"]);
    }

    #[test]
    fn partial_apply_failure_still_returns_actual_values_and_stage() {
        let actual = values(30);
        let store = FakeStore::new(actual);
        store.state.lock().expect("power tuning lock").apply_error =
            Some(ProcessorPowerApplyError::at(
                ProcessorPowerApplyStage::DcPerformanceMaximum,
                "write failed".to_owned(),
            ));
        let service = AdvancedPowerPlanTuningService::with_store(Box::new(store.clone()));

        let outcome = service.apply_values("plan", values(80));

        assert!(!outcome.applied);
        assert_eq!(outcome.actual_values, Some(actual));
        assert!(matches!(
            outcome.error,
            Some(AdvancedPowerPlanTuningApplyError::Apply(ref error))
                if error.stage() == ProcessorPowerApplyStage::DcPerformanceMaximum
        ));
        assert_eq!(store.calls(), vec!["apply", "read"]);
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
}
