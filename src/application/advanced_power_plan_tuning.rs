use crate::power::{
    apply_processor_power_values_staged, powercfg::validate_tuning_plan, read_plan_personality,
    read_processor_power_values, AdvancedPowerPlanTuningReadback,
    AdvancedPowerPlanTuningSourceValues, PowerPlanPersonality, ProcessorPowerApplyError,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AdvancedPowerPlanTuningError {
    #[error("Failed to read processor power values: {0}")]
    ReadValues(String),
    #[error("Failed to read power plan personality: {0}")]
    ReadPersonality(String),
    #[error("Power plan values changed after review.")]
    ValuesChanged,
    #[error("A requested processor power setting is unavailable.")]
    SettingUnavailable,
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
    pub actual_values: Option<AdvancedPowerPlanTuningReadback>,
    pub error: Option<AdvancedPowerPlanTuningApplyError>,
}

trait AdvancedPowerPlanTuningStore: Send + Sync {
    fn read_values(
        &self,
        guid: &str,
    ) -> Result<AdvancedPowerPlanTuningReadback, AdvancedPowerPlanTuningError>;
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
    ) -> Result<AdvancedPowerPlanTuningReadback, AdvancedPowerPlanTuningError> {
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
        validate_tuning_plan(guid)?;
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
    ) -> Result<AdvancedPowerPlanTuningReadback, AdvancedPowerPlanTuningError> {
        self.store.read_values(guid)
    }

    pub(crate) fn read_personality(
        &self,
        guid: &str,
    ) -> Result<PowerPlanPersonality, AdvancedPowerPlanTuningError> {
        self.store.read_personality(guid)
    }

    pub(crate) fn apply_reviewed_values(
        &self,
        guid: &str,
        reviewed_current: AdvancedPowerPlanTuningReadback,
        values: AdvancedPowerPlanTuningSourceValues,
    ) -> Result<AdvancedPowerPlanTuningApplyOutcome, AdvancedPowerPlanTuningError> {
        self.validate_reviewed_values(guid, reviewed_current, values)?;
        Ok(self.apply_values(guid, values))
    }

    pub(crate) fn validate_reviewed_values(
        &self,
        guid: &str,
        reviewed_current: AdvancedPowerPlanTuningReadback,
        values: AdvancedPowerPlanTuningSourceValues,
    ) -> Result<(), AdvancedPowerPlanTuningError> {
        if self.read_values(guid)? != reviewed_current {
            return Err(AdvancedPowerPlanTuningError::ValuesChanged);
        }
        let available = reviewed_current.writable();
        if [
            (values.ac, available.ac),
            (values.battery, available.battery),
        ]
        .into_iter()
        .any(|(requested, current)| {
            (requested.processor.boost_mode.is_some() && current.processor.boost_mode.is_none())
                || (requested.scheduling_policy.is_some() && current.scheduling_policy.is_none())
                || (requested.short_scheduling_policy.is_some()
                    && current.short_scheduling_policy.is_none())
        }) {
            return Err(AdvancedPowerPlanTuningError::SettingUnavailable);
        }
        Ok(())
    }

    fn apply_values(
        &self,
        guid: &str,
        values: AdvancedPowerPlanTuningSourceValues,
    ) -> AdvancedPowerPlanTuningApplyOutcome {
        let values = values.normalized();
        let apply_error = self.store.apply_values(guid, values).err();
        let readback = self.read_values(guid);

        match (apply_error, readback) {
            (None, Ok(actual_values)) => {
                let applied = actual_values == actual_values.with_requested(values);
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

#[cfg(test)]
mod tests;
