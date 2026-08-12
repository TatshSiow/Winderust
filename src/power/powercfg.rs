use crate::platform::windows::power_plan::{self as windows_power, PowerSetting};

use super::{
    EffectivePowerMode, PowerPlan, PowerPlanPersonality, ProcessorBoostMode,
    ProcessorPowerAcDcValues, ProcessorPowerValues,
};

const ADAPTIVE_PLAN_NAME: &str = "Winderust Adaptive";
const ADAPTIVE_PLAN_DESCRIPTION_PREFIX: &str = "Winderust managed adaptive plan; restore=";

#[derive(Debug)]
pub struct EffectivePowerModeMonitor {
    registration: windows_power::EffectivePowerModeRegistration,
}

pub fn list_plans() -> Result<Vec<PowerPlan>, String> {
    let active_guid = windows_power::active_scheme_guid().ok();
    let plans = windows_power::list_schemes()?
        .into_iter()
        .map(|scheme| {
            let active = active_guid
                .as_deref()
                .is_some_and(|active_guid| active_guid.eq_ignore_ascii_case(&scheme.guid));

            PowerPlan {
                guid: scheme.guid,
                name: scheme.name,
                active,
            }
        })
        .collect::<Vec<_>>();

    if plans.is_empty() {
        Err("No Windows power plans were detected.".to_owned())
    } else {
        Ok(plans)
    }
}

pub fn active_plan() -> Result<PowerPlan, String> {
    Ok(PowerPlan {
        guid: windows_power::active_scheme_guid()?,
        name: "Active power plan".to_owned(),
        active: true,
    })
}

pub fn set_active(guid: &str) -> Result<(), String> {
    windows_power::set_active(guid)
}

pub fn create_adaptive_plan(source_guid: &str) -> Result<String, String> {
    let duplicate_guid = windows_power::duplicate_scheme(source_guid)?;
    let description = format!("{ADAPTIVE_PLAN_DESCRIPTION_PREFIX}{source_guid}");

    if let Err(error) = windows_power::write_scheme_name(&duplicate_guid, ADAPTIVE_PLAN_NAME)
        .and_then(|()| windows_power::write_scheme_description(&duplicate_guid, &description))
    {
        let _ = windows_power::delete_scheme(&duplicate_guid);
        return Err(error);
    }

    Ok(duplicate_guid)
}

pub fn delete_plan(guid: &str) -> Result<(), String> {
    windows_power::delete_scheme(guid)
}

pub fn restore_stale_adaptive_plans() -> Result<(), String> {
    let plans = list_plans()?;
    for plan in plans.iter().filter(|plan| plan.name == ADAPTIVE_PLAN_NAME) {
        if !windows_power::is_valid_guid(&plan.guid) {
            return Err("Invalid managed power plan GUID.".to_owned());
        }
        let description = windows_power::read_scheme_description(&plan.guid)?;
        let Some(restore_guid) = managed_adaptive_restore_guid(&plan.name, &description) else {
            continue;
        };
        let restore_exists = plans
            .iter()
            .any(|candidate| candidate.guid.eq_ignore_ascii_case(restore_guid));

        if plan.active {
            if !restore_exists {
                continue;
            }
            set_active(restore_guid)?;
        }
        delete_plan(&plan.guid)?;
    }

    Ok(())
}

pub fn read_plan_personality(guid: &str) -> Result<PowerPlanPersonality, String> {
    validate_plan_guid(guid)?;
    Ok(PowerPlanPersonality::from_power_value(
        windows_power::read_ac_value(guid, PowerSetting::Personality)?,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessorPowerApplyStage {
    ValidatePlan,
    AcCoreParkingMinimum,
    DcCoreParkingMinimum,
    AcPerformanceMinimum,
    DcPerformanceMinimum,
    AcPerformanceMaximum,
    DcPerformanceMaximum,
    AcBoostPolicy,
    DcBoostPolicy,
    AcBoostMode,
    DcBoostMode,
    ReactivatePlan,
}

impl std::fmt::Display for ProcessorPowerApplyStage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ValidatePlan => "validate plan",
            Self::AcCoreParkingMinimum => "write A/C core parking minimum",
            Self::DcCoreParkingMinimum => "write battery core parking minimum",
            Self::AcPerformanceMinimum => "write A/C processor minimum",
            Self::DcPerformanceMinimum => "write battery processor minimum",
            Self::AcPerformanceMaximum => "write A/C processor maximum",
            Self::DcPerformanceMaximum => "write battery processor maximum",
            Self::AcBoostPolicy => "write A/C boost policy",
            Self::DcBoostPolicy => "write battery boost policy",
            Self::AcBoostMode => "write A/C boost mode",
            Self::DcBoostMode => "write battery boost mode",
            Self::ReactivatePlan => "refresh the active plan",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessorPowerApplyError {
    stage: ProcessorPowerApplyStage,
    source: String,
}

impl ProcessorPowerApplyError {
    pub(crate) fn at(stage: ProcessorPowerApplyStage, source: String) -> Self {
        Self { stage, source }
    }

    #[cfg(test)]
    pub(crate) fn stage(&self) -> ProcessorPowerApplyStage {
        self.stage
    }
}

impl std::fmt::Display for ProcessorPowerApplyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Failed to {}: {}", self.stage, self.source)
    }
}

impl std::error::Error for ProcessorPowerApplyError {}

pub fn apply_processor_power_values(
    guid: &str,
    values: ProcessorPowerAcDcValues,
) -> Result<(), String> {
    apply_processor_power_values_staged(guid, values).map_err(|error| error.to_string())
}

pub(crate) fn apply_processor_power_values_staged(
    guid: &str,
    values: ProcessorPowerAcDcValues,
) -> Result<(), ProcessorPowerApplyError> {
    validate_plan_guid(guid).map_err(|error| {
        ProcessorPowerApplyError::at(ProcessorPowerApplyStage::ValidatePlan, error)
    })?;
    let values = values.normalized();

    write_processor_value(
        guid,
        PowerSetting::CoreParkingMinimum,
        values.ac.core_parking_min,
        ProcessorPowerApplyStage::AcCoreParkingMinimum,
        windows_power::write_ac_value,
    )?;
    write_processor_value(
        guid,
        PowerSetting::CoreParkingMinimum,
        values.dc.core_parking_min,
        ProcessorPowerApplyStage::DcCoreParkingMinimum,
        windows_power::write_dc_value,
    )?;
    write_processor_value(
        guid,
        PowerSetting::PerformanceMinimum,
        values.ac.performance_min,
        ProcessorPowerApplyStage::AcPerformanceMinimum,
        windows_power::write_ac_value,
    )?;
    write_processor_value(
        guid,
        PowerSetting::PerformanceMinimum,
        values.dc.performance_min,
        ProcessorPowerApplyStage::DcPerformanceMinimum,
        windows_power::write_dc_value,
    )?;
    write_processor_value(
        guid,
        PowerSetting::PerformanceMaximum,
        values.ac.performance_max,
        ProcessorPowerApplyStage::AcPerformanceMaximum,
        windows_power::write_ac_value,
    )?;
    write_processor_value(
        guid,
        PowerSetting::PerformanceMaximum,
        values.dc.performance_max,
        ProcessorPowerApplyStage::DcPerformanceMaximum,
        windows_power::write_dc_value,
    )?;
    write_processor_value(
        guid,
        PowerSetting::BoostPolicy,
        values.ac.boost_policy,
        ProcessorPowerApplyStage::AcBoostPolicy,
        windows_power::write_ac_value,
    )?;
    write_processor_value(
        guid,
        PowerSetting::BoostPolicy,
        values.dc.boost_policy,
        ProcessorPowerApplyStage::DcBoostPolicy,
        windows_power::write_dc_value,
    )?;
    write_processor_value(
        guid,
        PowerSetting::BoostMode,
        values.ac.boost_mode.power_value(),
        ProcessorPowerApplyStage::AcBoostMode,
        windows_power::write_ac_value,
    )?;
    write_processor_value(
        guid,
        PowerSetting::BoostMode,
        values.dc.boost_mode.power_value(),
        ProcessorPowerApplyStage::DcBoostMode,
        windows_power::write_dc_value,
    )?;

    if windows_power::active_scheme_guid()
        .ok()
        .is_some_and(|active_guid| active_guid.eq_ignore_ascii_case(guid))
    {
        set_active(guid).map_err(|error| {
            ProcessorPowerApplyError::at(ProcessorPowerApplyStage::ReactivatePlan, error)
        })?;
    }

    Ok(())
}

pub fn read_processor_power_values(guid: &str) -> Result<ProcessorPowerAcDcValues, String> {
    validate_plan_guid(guid)?;
    Ok(ProcessorPowerAcDcValues::new(
        ProcessorPowerValues::new_with_boost_mode(
            windows_power::read_ac_value(guid, PowerSetting::CoreParkingMinimum)?,
            windows_power::read_ac_value(guid, PowerSetting::PerformanceMinimum)?,
            windows_power::read_ac_value(guid, PowerSetting::PerformanceMaximum)?,
            windows_power::read_ac_value(guid, PowerSetting::BoostPolicy)?,
            ProcessorBoostMode::from_power_value(windows_power::read_ac_value(
                guid,
                PowerSetting::BoostMode,
            )?),
        )
        .normalized(),
        ProcessorPowerValues::new_with_boost_mode(
            windows_power::read_dc_value(guid, PowerSetting::CoreParkingMinimum)?,
            windows_power::read_dc_value(guid, PowerSetting::PerformanceMinimum)?,
            windows_power::read_dc_value(guid, PowerSetting::PerformanceMaximum)?,
            windows_power::read_dc_value(guid, PowerSetting::BoostPolicy)?,
            ProcessorBoostMode::from_power_value(windows_power::read_dc_value(
                guid,
                PowerSetting::BoostMode,
            )?),
        )
        .normalized(),
    ))
}

impl EffectivePowerModeMonitor {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            registration: windows_power::EffectivePowerModeRegistration::new()?,
        })
    }

    pub fn snapshot(&self) -> EffectivePowerMode {
        EffectivePowerMode::from_raw(self.registration.snapshot_raw())
    }
}

fn validate_plan_guid(guid: &str) -> Result<(), String> {
    windows_power::is_valid_guid(guid)
        .then_some(())
        .ok_or_else(|| "Invalid power plan GUID.".to_owned())
}

fn write_processor_value(
    guid: &str,
    setting: PowerSetting,
    value: u32,
    stage: ProcessorPowerApplyStage,
    write: fn(&str, PowerSetting, u32) -> Result<(), String>,
) -> Result<(), ProcessorPowerApplyError> {
    write(guid, setting, value).map_err(|error| ProcessorPowerApplyError::at(stage, error))
}

fn managed_adaptive_restore_guid<'a>(plan_name: &str, description: &'a str) -> Option<&'a str> {
    if plan_name != ADAPTIVE_PLAN_NAME {
        return None;
    }
    let restore_guid = description.strip_prefix(ADAPTIVE_PLAN_DESCRIPTION_PREFIX)?;
    windows_power::is_valid_guid(restore_guid).then_some(restore_guid)
}

#[cfg(test)]
mod tests {
    use crate::power::{EffectivePowerMode, PowerPlanPersonality, ProcessorPowerPreset};

    use super::*;

    #[test]
    fn recognizes_only_winderust_managed_adaptive_plan() {
        let restore_guid = "381b4222-f694-41f0-9685-ff5bb260df2e";

        assert_eq!(
            managed_adaptive_restore_guid(
                ADAPTIVE_PLAN_NAME,
                &format!("{ADAPTIVE_PLAN_DESCRIPTION_PREFIX}{restore_guid}"),
            ),
            Some(restore_guid)
        );
        assert_eq!(
            managed_adaptive_restore_guid(
                "Another App Adaptive",
                &format!("Another App managed adaptive plan; restore={restore_guid}"),
            ),
            None
        );
    }

    #[test]
    fn maps_windows_power_modes() {
        assert_eq!(
            PowerPlanPersonality::from_power_value(2),
            PowerPlanPersonality::Balanced
        );
        assert_eq!(
            EffectivePowerMode::from_raw(4),
            EffectivePowerMode::MaxPerformance
        );
        assert_eq!(
            EffectivePowerMode::from_raw(-1),
            EffectivePowerMode::Unknown
        );
    }

    #[test]
    fn processor_power_presets_use_explicit_percentages() {
        let performance = ProcessorPowerValues::for_preset(ProcessorPowerPreset::Performance);
        assert_eq!(performance.core_parking_min, 100);
        assert_eq!(performance.performance_min, 100);
        assert_eq!(performance.performance_max, 100);
        assert_eq!(performance.boost_policy, 100);
        assert_eq!(performance.boost_mode, ProcessorBoostMode::Aggressive);

        let saver = ProcessorPowerValues::for_preset(ProcessorPowerPreset::Saver);
        assert_eq!(saver.core_parking_min, 0);
        assert_eq!(saver.performance_min, 5);
        assert_eq!(saver.performance_max, 60);
        assert_eq!(saver.boost_policy, 0);
        assert_eq!(saver.boost_mode, ProcessorBoostMode::Disabled);
    }

    #[test]
    fn processor_power_values_normalize_to_valid_percentages() {
        let values = ProcessorPowerValues::new_with_boost_mode(
            140,
            75,
            20,
            150,
            ProcessorBoostMode::Enabled,
        )
        .normalized();

        assert_eq!(values.core_parking_min, 100);
        assert_eq!(values.performance_min, 75);
        assert_eq!(values.performance_max, 75);
        assert_eq!(values.boost_policy, 100);
        assert_eq!(values.boost_mode, ProcessorBoostMode::Enabled);
    }

    #[test]
    fn processor_power_ac_dc_values_normalize_each_power_source() {
        let values = ProcessorPowerAcDcValues::new(
            ProcessorPowerValues::new_with_boost_mode(
                120,
                90,
                80,
                120,
                ProcessorBoostMode::Enabled,
            ),
            ProcessorPowerValues::new_with_boost_mode(10, 20, 15, 30, ProcessorBoostMode::Enabled),
        )
        .normalized();

        assert_eq!(values.ac.core_parking_min, 100);
        assert_eq!(values.ac.performance_min, 90);
        assert_eq!(values.ac.performance_max, 90);
        assert_eq!(values.ac.boost_policy, 100);
        assert_eq!(values.dc.core_parking_min, 10);
        assert_eq!(values.dc.performance_min, 20);
        assert_eq!(values.dc.performance_max, 20);
        assert_eq!(values.dc.boost_policy, 30);
    }
}
