use crate::power::{
    apply_processor_power_values_staged, read_plan_personality, read_processor_power_values,
    AdvancedPowerPlanTuningSourceValues, AdvancedPowerPlanTuningValues, PowerPlanPersonality,
    ProcessorPowerApplyError,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AdvancedPowerPlanTuningError {
    #[error("Failed to read processor power values: {0}")]
    ReadValues(String),
    #[error("Failed to read power plan personality: {0}")]
    ReadPersonality(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AdvancedPowerPlanTuningApplyError {
    #[error("{0}")]
    Apply(ProcessorPowerApplyError),
    #[error("Processor power values were written, but readback failed: {0}")]
    Readback(AdvancedPowerPlanTuningError),
    #[error("Processor power readback differs from the requested values. Review the actual values before applying again.")]
    Verification,
    #[error("{apply} Reading the actual values after the partial apply also failed: {readback}")]
    ApplyAndReadback {
        apply: ProcessorPowerApplyError,
        readback: AdvancedPowerPlanTuningError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdvancedPowerPlanTuningApplyOutcome {
    pub applied: bool,
    pub actual_values: Option<AdvancedPowerPlanTuningSourceValues>,
    pub error: Option<AdvancedPowerPlanTuningApplyError>,
}

trait AdvancedPowerPlanTuningStore: Send + Sync {
    fn read_values(
        &self,
        guid: &str,
    ) -> Result<AdvancedPowerPlanTuningSourceValues, AdvancedPowerPlanTuningError>;
    fn read_personality(
        &self,
        guid: &str,
    ) -> Result<PowerPlanPersonality, AdvancedPowerPlanTuningError>;
    fn apply_values(
        &self,
        guid: &str,
        values: AdvancedPowerPlanTuningSourceValues,
    ) -> Result<(), ProcessorPowerApplyError>;
}

#[derive(Debug, Default)]
struct WindowsAdvancedPowerPlanTuningStore;

impl AdvancedPowerPlanTuningStore for WindowsAdvancedPowerPlanTuningStore {
    fn read_values(
        &self,
        guid: &str,
    ) -> Result<AdvancedPowerPlanTuningSourceValues, AdvancedPowerPlanTuningError> {
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
        values: AdvancedPowerPlanTuningSourceValues,
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
    ) -> Result<AdvancedPowerPlanTuningSourceValues, AdvancedPowerPlanTuningError> {
        self.store.read_values(guid)
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
        values: AdvancedPowerPlanTuningSourceValues,
    ) -> AdvancedPowerPlanTuningApplyOutcome {
        let values = values.normalized();
        let apply_error = self.store.apply_values(guid, values).err();
        let readback = self.read_values(guid);

        match (apply_error, readback) {
            (None, Ok(actual_values)) => {
                let applied = tuning_matches(values.ac, actual_values.ac)
                    && tuning_matches(values.battery, actual_values.battery);
                AdvancedPowerPlanTuningApplyOutcome {
                    applied,
                    actual_values: Some(actual_values),
                    error: (!applied).then_some(AdvancedPowerPlanTuningApplyError::Verification),
                }
            }
            (Some(apply), Ok(actual_values)) => AdvancedPowerPlanTuningApplyOutcome {
                applied: false,
                actual_values: Some(actual_values),
                error: Some(AdvancedPowerPlanTuningApplyError::Apply(apply)),
            },
            (None, Err(readback)) => AdvancedPowerPlanTuningApplyOutcome {
                applied: false,
                actual_values: None,
                error: Some(AdvancedPowerPlanTuningApplyError::Readback(readback)),
            },
            (Some(apply), Err(readback)) => AdvancedPowerPlanTuningApplyOutcome {
                applied: false,
                actual_values: None,
                error: Some(AdvancedPowerPlanTuningApplyError::ApplyAndReadback {
                    apply,
                    readback,
                }),
            },
        }
    }
}

fn tuning_matches(
    requested: AdvancedPowerPlanTuningValues,
    actual: AdvancedPowerPlanTuningValues,
) -> bool {
    requested.processor == actual.processor
        && requested
            .scheduling_policy
            .is_none_or(|value| actual.scheduling_policy == Some(value))
        && requested
            .short_scheduling_policy
            .is_none_or(|value| actual.short_scheduling_policy == Some(value))
}

#[cfg(test)]
mod tests;
