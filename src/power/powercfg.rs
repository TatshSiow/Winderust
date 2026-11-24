use crate::platform::windows::power_plan::{self as windows_power, PowerSetting};

use super::{
    AdvancedPowerPlanTuningReadback, AdvancedPowerPlanTuningSourceReadback,
    AdvancedPowerPlanTuningSourceValues, EditableTuningValues, EffectivePowerMode,
    EnergyPerformancePreference, PowerPlan, PowerPlanPersonality, PowerSettingReadback,
    ProcessorBoostMode, ProcessorPowerSourceValues, ProcessorPowerValues,
    ProcessorSchedulingPolicy,
};

pub(crate) const ADAPTIVE_PLAN_NAME: &str = "Winderust Adaptive";
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
            // The reserved name is selectable only when its description rules out ownership.
            let managed_adaptive = scheme.name == ADAPTIVE_PLAN_NAME
                && windows_power::read_scheme_description(&scheme.guid).map_or(
                    true,
                    |description| {
                        managed_adaptive_restore_guid(&scheme.name, &description).is_some()
                    },
                );

            PowerPlan {
                guid: scheme.guid,
                name: scheme.name,
                active,
                managed_adaptive,
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
        managed_adaptive: false,
    })
}

pub fn set_active(guid: &str) -> Result<(), String> {
    windows_power::set_active(guid)
}

pub fn duplicate_adaptive_plan(source_guid: &str) -> Result<String, String> {
    // Return the resource immediately. The controller must retain this GUID before
    // initialization; even naming or idle-state verification can fail.
    windows_power::duplicate_scheme(source_guid)
}

pub fn initialize_adaptive_plan(guid: &str, source_guid: &str) -> Result<(), String> {
    initialize_adaptive_plan_with(
        source_guid,
        |name| windows_power::write_scheme_name(guid, name),
        |description| windows_power::write_scheme_description(guid, description),
        |battery, value| {
            if battery {
                windows_power::write_dc_value(guid, PowerSetting::IdleDisable, value)
            } else {
                windows_power::write_ac_value(guid, PowerSetting::IdleDisable, value)
            }
        },
        |battery| {
            if battery {
                windows_power::read_dc_value(guid, PowerSetting::IdleDisable)
            } else {
                windows_power::read_ac_value(guid, PowerSetting::IdleDisable)
            }
        },
    )
}

fn initialize_adaptive_plan_with(
    source_guid: &str,
    write_name: impl FnOnce(&str) -> Result<(), String>,
    write_description: impl FnOnce(&str) -> Result<(), String>,
    write_idle: impl FnMut(bool, u32) -> Result<(), String>,
    read_idle: impl FnMut(bool) -> Result<u32, String>,
) -> Result<(), String> {
    let description = format!("{ADAPTIVE_PLAN_DESCRIPTION_PREFIX}{source_guid}");
    write_name(ADAPTIVE_PLAN_NAME)?;
    write_description(&description)?;
    // Cleanup belongs to the controller even when metadata is only partially written.
    enable_adaptive_idle_states(write_idle, read_idle)
}

// IdleDisable=0 permits CPU idle states, regardless of the cloned plan.
// Set both sources before activation; profile changes preserve this policy.
fn enable_adaptive_idle_states(
    mut write: impl FnMut(bool, u32) -> Result<(), String>,
    mut read: impl FnMut(bool) -> Result<u32, String>,
) -> Result<(), String> {
    for battery in [false, true] {
        let source = if battery { "battery" } else { "A/C" };
        write(battery, 0)
            .map_err(|error| format!("Failed to enable {source} CPU idle states: {error}"))?;
        let value = read(battery)
            .map_err(|error| format!("Failed to verify {source} CPU idle states: {error}"))?;
        if value != 0 {
            return Err(format!(
                "{source} CPU idle states remain disabled (IdleDisable={value})."
            ));
        }
    }
    Ok(())
}

pub fn delete_plan(guid: &str) -> Result<(), String> {
    windows_power::delete_scheme(guid)
}

pub fn restore_stale_adaptive_plans() -> Result<(), String> {
    let plans = list_plans()?;
    restore_stale_adaptive_plans_with(
        &plans,
        windows_power::read_scheme_description,
        windows_power::active_scheme_guid,
        set_active,
        delete_plan,
    )
}

fn restore_stale_adaptive_plans_with(
    plans: &[PowerPlan],
    mut description: impl FnMut(&str) -> Result<String, String>,
    mut active_guid: impl FnMut() -> Result<String, String>,
    mut activate: impl FnMut(&str) -> Result<(), String>,
    mut delete: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    for plan in plans.iter().filter(|plan| plan.name == ADAPTIVE_PLAN_NAME) {
        if !windows_power::is_valid_guid(&plan.guid) {
            return Err("Invalid managed power plan GUID.".to_owned());
        }
        let description = description(&plan.guid)?;
        let Some(restore_guid) = managed_adaptive_restore_guid(&plan.name, &description) else {
            continue;
        };
        let restore_exists = plans
            .iter()
            .any(|candidate| candidate.guid.eq_ignore_ascii_case(restore_guid));

        // Enumeration is only a snapshot; metadata reads and other actors can
        // change the active scheme before recovery reaches this plan.
        if active_guid()?.eq_ignore_ascii_case(&plan.guid) {
            if !restore_exists {
                continue;
            }
            activate(restore_guid)?;
            let actual = active_guid()?;
            if !actual.eq_ignore_ascii_case(restore_guid) {
                return Err(format!(
                    "Adaptive plan recovery expected {restore_guid}, but Windows reported {actual}."
                ));
            }
        }
        delete(&plan.guid)?;
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
    BatteryCoreParkingMinimum,
    AcPerformanceMinimum,
    BatteryPerformanceMinimum,
    AcPerformanceMaximum,
    BatteryPerformanceMaximum,
    AcEnergyPerformancePreference,
    BatteryEnergyPerformancePreference,
    AcBoostPolicy,
    BatteryBoostPolicy,
    AcBoostMode,
    BatteryBoostMode,
    AcSchedulingPolicy,
    BatterySchedulingPolicy,
    AcShortSchedulingPolicy,
    BatteryShortSchedulingPolicy,
    QueryActivePlan,
    ReactivatePlan,
}

impl std::fmt::Display for ProcessorPowerApplyStage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ValidatePlan => "validate plan",
            Self::AcCoreParkingMinimum => "write A/C core parking minimum",
            Self::BatteryCoreParkingMinimum => "write battery core parking minimum",
            Self::AcPerformanceMinimum => "write A/C processor minimum",
            Self::BatteryPerformanceMinimum => "write battery processor minimum",
            Self::AcPerformanceMaximum => "write A/C processor maximum",
            Self::BatteryPerformanceMaximum => "write battery processor maximum",
            Self::AcEnergyPerformancePreference => "write A/C energy performance preference",
            Self::BatteryEnergyPerformancePreference => {
                "write battery energy performance preference"
            }
            Self::AcBoostPolicy => "write A/C boost policy",
            Self::BatteryBoostPolicy => "write battery boost policy",
            Self::AcBoostMode => "write A/C boost mode",
            Self::BatteryBoostMode => "write battery boost mode",
            Self::AcSchedulingPolicy => "write A/C long-running thread scheduling policy",
            Self::BatterySchedulingPolicy => "write battery long-running thread scheduling policy",
            Self::AcShortSchedulingPolicy => "write A/C short-running thread scheduling policy",
            Self::BatteryShortSchedulingPolicy => {
                "write battery short-running thread scheduling policy"
            }
            Self::QueryActivePlan => "query the active plan",
            Self::ReactivatePlan => "refresh the active plan",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProcessorPowerApplyError {
    PlanUnavailable,
    Stage {
        stage: ProcessorPowerApplyStage,
        detail: String,
    },
}

impl ProcessorPowerApplyError {
    pub(crate) fn at(stage: ProcessorPowerApplyStage, detail: String) -> Self {
        Self::Stage { stage, detail }
    }

    pub(crate) fn stage(&self) -> ProcessorPowerApplyStage {
        match self {
            Self::PlanUnavailable => ProcessorPowerApplyStage::ValidatePlan,
            Self::Stage { stage, .. } => *stage,
        }
    }
}

impl std::fmt::Display for ProcessorPowerApplyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PlanUnavailable => formatter.write_str(
                "Failed to validate plan: The selected plan is unavailable for persistent tuning.",
            ),
            Self::Stage { stage, detail } => write!(formatter, "Failed to {stage}: {detail}"),
        }
    }
}

impl std::error::Error for ProcessorPowerApplyError {}

pub(crate) fn validate_tuning_plan(guid: &str) -> Result<(), ProcessorPowerApplyError> {
    if list_plans()
        .map_err(|detail| {
            ProcessorPowerApplyError::at(ProcessorPowerApplyStage::ValidatePlan, detail)
        })?
        .iter()
        .any(|plan| !plan.managed_adaptive && plan.guid.eq_ignore_ascii_case(guid))
    {
        Ok(())
    } else {
        Err(ProcessorPowerApplyError::PlanUnavailable)
    }
}

pub fn apply_processor_power_values(
    guid: &str,
    values: ProcessorPowerSourceValues,
) -> Result<(), String> {
    apply_processor_power_values_staged(guid, values.normalized().into(), None)
        .map_err(|error| error.to_string())
}

pub(crate) fn apply_processor_power_values_staged(
    guid: &str,
    values: AdvancedPowerPlanTuningSourceValues,
    previous: Option<AdvancedPowerPlanTuningSourceValues>,
) -> Result<(), ProcessorPowerApplyError> {
    validate_plan_guid(guid).map_err(|error| {
        ProcessorPowerApplyError::at(ProcessorPowerApplyStage::ValidatePlan, error)
    })?;
    write_tuning_values(values.normalized(), previous, |battery, setting, value| {
        if battery {
            windows_power::write_dc_value(guid, setting, value)
        } else {
            windows_power::write_ac_value(guid, setting, value)
        }
    })?;
    reactivate_if_active(guid, windows_power::active_scheme_guid, set_active)
}

fn reactivate_if_active(
    guid: &str,
    active_guid: impl FnOnce() -> Result<String, String>,
    activate: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), ProcessorPowerApplyError> {
    let active = active_guid().map_err(|error| {
        ProcessorPowerApplyError::at(ProcessorPowerApplyStage::QueryActivePlan, error)
    })?;
    if active.eq_ignore_ascii_case(guid) {
        activate(guid).map_err(|error| {
            ProcessorPowerApplyError::at(ProcessorPowerApplyStage::ReactivatePlan, error)
        })?;
    }
    Ok(())
}

pub(crate) fn read_processor_power_values(
    guid: &str,
) -> Result<AdvancedPowerPlanTuningReadback, String> {
    validate_plan_guid(guid)?;
    Ok(AdvancedPowerPlanTuningReadback {
        ac: read_tuning_source(guid, false)?,
        battery: read_tuning_source(guid, true)?,
    })
}

fn read_tuning_source(
    guid: &str,
    battery: bool,
) -> Result<AdvancedPowerPlanTuningSourceReadback, String> {
    let read = |setting| {
        if battery {
            windows_power::read_dc_value(guid, setting)
        } else {
            windows_power::read_ac_value(guid, setting)
        }
    };
    let read_percent = |setting| {
        let value = read(setting)?;
        if value > 100 {
            return Err(format!(
                "Processor power percentage is outside 0-100: {value}."
            ));
        }
        Ok(value)
    };
    let read_optional = |setting| windows_power::read_optional_value(guid, setting, battery);
    let processor = ProcessorPowerValues::new_with_boost_mode(
        read_percent(PowerSetting::CoreParkingMinimum)?,
        read_percent(PowerSetting::PerformanceMinimum)?,
        read_percent(PowerSetting::PerformanceMaximum)?,
        read_percent(PowerSetting::BoostPolicy)?,
        PowerSettingReadback::from_optional_read(
            read_optional(PowerSetting::BoostMode),
            ProcessorBoostMode::from_power_value,
        ),
    );
    let scheduling = |setting| {
        PowerSettingReadback::from_optional_read(read_optional(setting), |value| {
            ProcessorSchedulingPolicy::ALL.get(value as usize).copied()
        })
    };
    Ok(AdvancedPowerPlanTuningSourceReadback {
        processor,
        energy_performance_preference: PowerSettingReadback::from_optional_read(
            read_optional(PowerSetting::EnergyPerformancePreference),
            |value| EnergyPerformancePreference::try_from(value).ok(),
        ),
        scheduling_policy: scheduling(PowerSetting::SchedulingPolicy),
        short_scheduling_policy: scheduling(PowerSetting::ShortSchedulingPolicy),
    })
}

fn write_tuning_values(
    values: AdvancedPowerPlanTuningSourceValues,
    previous: Option<AdvancedPowerPlanTuningSourceValues>,
    mut write: impl FnMut(bool, PowerSetting, u32) -> Result<(), String>,
) -> Result<(), ProcessorPowerApplyError> {
    let mut write_setting = |setting,
                             field: fn(EditableTuningValues) -> Option<u32>,
                             ac_stage,
                             battery_stage|
     -> Result<(), ProcessorPowerApplyError> {
        for (battery, current, previous, stage) in [
            (false, values.ac, previous.map(|values| values.ac), ac_stage),
            (
                true,
                values.battery,
                previous.map(|values| values.battery),
                battery_stage,
            ),
        ] {
            if let Some(value) =
                field(current).filter(|&value| Some(value) != previous.and_then(field))
            {
                write(battery, setting, value)
                    .map_err(|error| ProcessorPowerApplyError::at(stage, error))?;
            }
        }
        Ok(())
    };
    write_setting(
        PowerSetting::CoreParkingMinimum,
        |values| Some(values.processor.core_parking_min),
        ProcessorPowerApplyStage::AcCoreParkingMinimum,
        ProcessorPowerApplyStage::BatteryCoreParkingMinimum,
    )?;
    write_setting(
        PowerSetting::PerformanceMinimum,
        |values| Some(values.processor.performance_min),
        ProcessorPowerApplyStage::AcPerformanceMinimum,
        ProcessorPowerApplyStage::BatteryPerformanceMinimum,
    )?;
    write_setting(
        PowerSetting::PerformanceMaximum,
        |values| Some(values.processor.performance_max),
        ProcessorPowerApplyStage::AcPerformanceMaximum,
        ProcessorPowerApplyStage::BatteryPerformanceMaximum,
    )?;
    write_setting(
        PowerSetting::EnergyPerformancePreference,
        |values| values.energy_performance_preference.map(u32::from),
        ProcessorPowerApplyStage::AcEnergyPerformancePreference,
        ProcessorPowerApplyStage::BatteryEnergyPerformancePreference,
    )?;
    write_setting(
        PowerSetting::BoostPolicy,
        |values| Some(values.processor.boost_policy),
        ProcessorPowerApplyStage::AcBoostPolicy,
        ProcessorPowerApplyStage::BatteryBoostPolicy,
    )?;
    write_setting(
        PowerSetting::BoostMode,
        |values| {
            values
                .processor
                .boost_mode
                .map(ProcessorBoostMode::power_value)
        },
        ProcessorPowerApplyStage::AcBoostMode,
        ProcessorPowerApplyStage::BatteryBoostMode,
    )?;
    write_setting(
        PowerSetting::SchedulingPolicy,
        |values| {
            values
                .scheduling_policy
                .map(ProcessorSchedulingPolicy::power_value)
        },
        ProcessorPowerApplyStage::AcSchedulingPolicy,
        ProcessorPowerApplyStage::BatterySchedulingPolicy,
    )?;
    write_setting(
        PowerSetting::ShortSchedulingPolicy,
        |values| {
            values
                .short_scheduling_policy
                .map(ProcessorSchedulingPolicy::power_value)
        },
        ProcessorPowerApplyStage::AcShortSchedulingPolicy,
        ProcessorPowerApplyStage::BatteryShortSchedulingPolicy,
    )
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

fn managed_adaptive_restore_guid<'a>(plan_name: &str, description: &'a str) -> Option<&'a str> {
    if plan_name != ADAPTIVE_PLAN_NAME {
        return None;
    }
    let restore_guid = description.strip_prefix(ADAPTIVE_PLAN_DESCRIPTION_PREFIX)?;
    windows_power::is_valid_guid(restore_guid).then_some(restore_guid)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use crate::power::{EffectivePowerMode, PowerPlanPersonality, ProcessorPowerPreset};

    use super::*;

    const RECOVERY_ORIGINAL: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
    const RECOVERY_MANAGED: &str = "00000000-0000-0000-0000-000000000001";
    const RECOVERY_EXTERNAL: &str = "00000000-0000-0000-0000-000000000002";

    fn stale_plan_fixture(snapshot_active: bool) -> [PowerPlan; 2] {
        [
            PowerPlan {
                guid: RECOVERY_ORIGINAL.to_owned(),
                name: "Original".to_owned(),
                active: !snapshot_active,
                managed_adaptive: false,
            },
            PowerPlan {
                guid: RECOVERY_MANAGED.to_owned(),
                name: ADAPTIVE_PLAN_NAME.to_owned(),
                active: snapshot_active,
                managed_adaptive: true,
            },
        ]
    }

    #[test]
    fn stale_adaptive_cleanup_preserves_an_external_choice_after_enumeration() {
        let events = RefCell::new(Vec::new());
        restore_stale_adaptive_plans_with(
            &stale_plan_fixture(true),
            |_| {
                Ok(format!(
                    "{ADAPTIVE_PLAN_DESCRIPTION_PREFIX}{RECOVERY_ORIGINAL}"
                ))
            },
            || {
                events.borrow_mut().push("query".to_owned());
                Ok(RECOVERY_EXTERNAL.to_owned())
            },
            |_| panic!("an external selection must not be overwritten"),
            |guid| {
                events.borrow_mut().push(format!("delete:{guid}"));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            events.into_inner(),
            ["query", &format!("delete:{RECOVERY_MANAGED}")]
        );
    }

    #[test]
    fn stale_adaptive_cleanup_uses_fresh_activation_and_verifies_before_deleting() {
        let events = RefCell::new(Vec::new());
        let mut queries = [RECOVERY_MANAGED, RECOVERY_ORIGINAL].into_iter();
        restore_stale_adaptive_plans_with(
            &stale_plan_fixture(false),
            |_| {
                Ok(format!(
                    "{ADAPTIVE_PLAN_DESCRIPTION_PREFIX}{RECOVERY_ORIGINAL}"
                ))
            },
            || {
                events.borrow_mut().push("query".to_owned());
                Ok(queries.next().unwrap().to_owned())
            },
            |guid| {
                events.borrow_mut().push(format!("activate:{guid}"));
                Ok(())
            },
            |guid| {
                events.borrow_mut().push(format!("delete:{guid}"));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            events.into_inner(),
            [
                "query",
                &format!("activate:{RECOVERY_ORIGINAL}"),
                "query",
                &format!("delete:{RECOVERY_MANAGED}")
            ]
        );
    }

    #[test]
    fn stale_adaptive_cleanup_retains_the_plan_when_restoration_is_unverified() {
        for readback in [
            Ok(RECOVERY_MANAGED.to_owned()),
            Ok(RECOVERY_EXTERNAL.to_owned()),
            Err("readback failed".to_owned()),
        ] {
            let mut queries = [Ok(RECOVERY_MANAGED.to_owned()), readback].into_iter();
            let mut activations = Vec::new();
            assert!(restore_stale_adaptive_plans_with(
                &stale_plan_fixture(true),
                |_| Ok(format!(
                    "{ADAPTIVE_PLAN_DESCRIPTION_PREFIX}{RECOVERY_ORIGINAL}"
                )),
                || queries.next().unwrap(),
                |guid| {
                    activations.push(guid.to_owned());
                    Ok(())
                },
                |_| panic!("unverified restoration must not delete the managed plan"),
            )
            .is_err());
            assert_eq!(activations, [RECOVERY_ORIGINAL]);
        }
        assert!(restore_stale_adaptive_plans_with(
            &stale_plan_fixture(true),
            |_| Ok(format!(
                "{ADAPTIVE_PLAN_DESCRIPTION_PREFIX}{RECOVERY_ORIGINAL}"
            )),
            || Err("active query failed".to_owned()),
            |_| panic!("a failed ownership query must block mutation"),
            |_| panic!("a failed ownership query must block deletion"),
        )
        .is_err());
    }

    #[test]
    fn stale_adaptive_cleanup_never_acts_without_the_current_name_and_description_pair() {
        for (name, description) in [
            (
                "User plan",
                format!("{ADAPTIVE_PLAN_DESCRIPTION_PREFIX}{RECOVERY_ORIGINAL}"),
            ),
            (ADAPTIVE_PLAN_NAME, "User description".to_owned()),
            (
                ADAPTIVE_PLAN_NAME,
                format!("{ADAPTIVE_PLAN_DESCRIPTION_PREFIX}invalid-guid"),
            ),
        ] {
            let mut plans = stale_plan_fixture(true);
            plans[1].name = name.to_owned();
            restore_stale_adaptive_plans_with(
                &plans,
                |_| Ok(description.clone()),
                || panic!("unrecognized plans must not be targeted"),
                |_| panic!("unrecognized plans must not be activated"),
                |_| panic!("unrecognized plans must not be deleted"),
            )
            .unwrap();
        }
    }

    #[test]
    fn tuning_writes_preserve_order_and_skip_unselected_optional_values() {
        let adaptive: AdvancedPowerPlanTuningSourceValues = ProcessorPowerSourceValues::new(
            ProcessorPowerValues::new_with_boost_mode(10, 20, 30, 40, ProcessorBoostMode::Enabled),
            ProcessorPowerValues::new_with_boost_mode(
                11,
                21,
                31,
                41,
                ProcessorBoostMode::EfficientEnabled,
            ),
        )
        .into();
        let mut values = adaptive;
        values.ac.scheduling_policy = Some(ProcessorSchedulingPolicy::PerformanceProcessors);
        values.battery.scheduling_policy =
            Some(ProcessorSchedulingPolicy::PreferEfficiencyProcessors);
        values.ac.short_scheduling_policy = Some(ProcessorSchedulingPolicy::Automatic);
        values.battery.short_scheduling_policy = Some(ProcessorSchedulingPolicy::AllProcessors);
        let expected = [
            (false, PowerSetting::CoreParkingMinimum, 10),
            (true, PowerSetting::CoreParkingMinimum, 11),
            (false, PowerSetting::PerformanceMinimum, 20),
            (true, PowerSetting::PerformanceMinimum, 21),
            (false, PowerSetting::PerformanceMaximum, 30),
            (true, PowerSetting::PerformanceMaximum, 31),
            (false, PowerSetting::BoostPolicy, 40),
            (true, PowerSetting::BoostPolicy, 41),
            (false, PowerSetting::BoostMode, 1),
            (true, PowerSetting::BoostMode, 3),
            (false, PowerSetting::SchedulingPolicy, 1),
            (true, PowerSetting::SchedulingPolicy, 4),
            (false, PowerSetting::ShortSchedulingPolicy, 5),
            (true, PowerSetting::ShortSchedulingPolicy, 0),
        ];
        for (requested, expected) in [(adaptive, &expected[..10]), (values, &expected[..])] {
            let mut calls = vec![];
            write_tuning_values(requested, None, |battery, setting, value| {
                calls.push((battery, setting, value));
                Ok(())
            })
            .unwrap();
            assert_eq!(calls, expected);
        }
        for (failure, stage) in [
            ProcessorPowerApplyStage::AcBoostMode,
            ProcessorPowerApplyStage::BatteryBoostMode,
            ProcessorPowerApplyStage::AcSchedulingPolicy,
            ProcessorPowerApplyStage::BatterySchedulingPolicy,
            ProcessorPowerApplyStage::AcShortSchedulingPolicy,
            ProcessorPowerApplyStage::BatteryShortSchedulingPolicy,
        ]
        .into_iter()
        .enumerate()
        {
            let mut calls = vec![];
            let failure = failure + 8;
            let error = write_tuning_values(values, None, |battery, setting, value| {
                calls.push((battery, setting, value));
                if calls.len() == failure + 1 {
                    Err("denied".into())
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
            assert_eq!(error.stage(), stage);
            assert_eq!(calls, expected[..=failure]);
        }
        values.ac.processor.boost_mode = None;
        values.ac.scheduling_policy = None;
        values.battery.short_scheduling_policy = None;
        let mut calls = vec![];
        write_tuning_values(values, None, |battery, setting, value| {
            calls.push((battery, setting, value));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            calls,
            [&expected[..8], &[expected[9], expected[11], expected[12]]].concat()
        );
    }

    #[test]
    fn reviewed_tuning_writes_only_changed_settings_and_reports_the_first_failure() {
        let mut previous: AdvancedPowerPlanTuningSourceValues = ProcessorPowerSourceValues::same(
            ProcessorPowerValues::new_with_boost_mode(10, 20, 30, 40, ProcessorBoostMode::Enabled),
        )
        .into();
        previous.ac.scheduling_policy = Some(ProcessorSchedulingPolicy::Automatic);
        let mut requested = previous;
        requested.ac.processor.performance_max = 50;

        let mut calls = vec![];
        let mut write = |battery, setting, value| {
            calls.push((battery, setting, value));
            if battery {
                Err("denied".into())
            } else {
                Ok(())
            }
        };
        write_tuning_values(requested, Some(previous), &mut write).unwrap();
        requested.battery.processor.performance_max = 60;
        requested.ac.scheduling_policy = Some(ProcessorSchedulingPolicy::PerformanceProcessors);
        let failed = write_tuning_values(requested, Some(previous), write).unwrap_err();
        assert_eq!(
            failed.stage(),
            ProcessorPowerApplyStage::BatteryPerformanceMaximum
        );
        assert_eq!(
            calls,
            [
                (false, PowerSetting::PerformanceMaximum, 50),
                (false, PowerSetting::PerformanceMaximum, 50),
                (true, PowerSetting::PerformanceMaximum, 60),
            ]
        );
    }

    #[test]
    fn adaptive_initialization_stops_at_each_failed_stage() {
        use std::cell::RefCell;

        let stages = [
            "name",
            "description",
            "ac-write",
            "ac-read",
            "dc-write",
            "dc-read",
        ];
        for fail_at in 0..=stages.len() {
            let events = RefCell::new(Vec::new());
            let step = |stage: &str| -> Result<(), String> {
                let mut events = events.borrow_mut();
                events.push(stage.to_owned());
                if events.len() - 1 == fail_at {
                    Err(format!("injected {stage} failure"))
                } else {
                    Ok(())
                }
            };
            let result = initialize_adaptive_plan_with(
                "original",
                |name| {
                    assert_eq!(name, ADAPTIVE_PLAN_NAME);
                    step("name")
                },
                |description| {
                    assert_eq!(
                        description,
                        format!("{ADAPTIVE_PLAN_DESCRIPTION_PREFIX}original")
                    );
                    step("description")
                },
                |battery, value| {
                    assert_eq!(value, 0);
                    step(if battery { "dc-write" } else { "ac-write" })
                },
                |battery| {
                    step(if battery { "dc-read" } else { "ac-read" })?;
                    Ok(0)
                },
            );
            let expected_count = (fail_at + 1).min(stages.len());
            assert_eq!(
                events.borrow().as_slice(),
                stages[..expected_count]
                    .iter()
                    .map(|stage| (*stage).to_owned())
                    .collect::<Vec<_>>()
                    .as_slice()
            );
            if fail_at < stages.len() {
                assert!(result.unwrap_err().contains(stages[fail_at]));
            } else {
                assert!(result.is_ok());
            }
        }
    }

    #[test]
    fn adaptive_idle_states_override_both_sources_and_propagate_failures() {
        let values = std::cell::Cell::new([1, 1]);
        enable_adaptive_idle_states(
            |battery, value| {
                let mut current = values.get();
                current[usize::from(battery)] = value;
                values.set(current);
                Ok(())
            },
            |battery| Ok(values.get()[usize::from(battery)]),
        )
        .unwrap();
        assert_eq!(values.get(), [0, 0]);
        for battery_failure in [false, true] {
            assert!(enable_adaptive_idle_states(
                |battery, _| {
                    if battery == battery_failure {
                        Err("write failed".into())
                    } else {
                        Ok(())
                    }
                },
                |_| Ok(0)
            )
            .is_err());
            assert!(enable_adaptive_idle_states(
                |_, _| Ok(()),
                |battery| { Ok(u32::from(battery == battery_failure)) }
            )
            .is_err());
            assert!(enable_adaptive_idle_states(
                |_, _| Ok(()),
                |battery| {
                    if battery == battery_failure {
                        Err("read failed".into())
                    } else {
                        Ok(0)
                    }
                }
            )
            .is_err());
        }
    }

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
    fn processor_power_values_normalize_each_power_source() {
        let values = ProcessorPowerSourceValues::new(
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
        assert_eq!(values.battery.core_parking_min, 10);
        assert_eq!(values.battery.performance_min, 20);
        assert_eq!(values.battery.performance_max, 20);
        assert_eq!(values.battery.boost_policy, 30);
    }
    #[test]
    fn active_query_failure_is_incomplete_and_retry_does_not_activate_an_inactive_plan() {
        let error = reactivate_if_active(
            "plan",
            || Err("query failed".into()),
            |_| panic!("must not activate an unknown plan"),
        )
        .unwrap_err();
        assert_eq!(error.stage(), ProcessorPowerApplyStage::QueryActivePlan);
        assert!(reactivate_if_active(
            "plan",
            || Ok("other".into()),
            |_| panic!("must not activate an inactive plan")
        )
        .is_ok());
        let error = reactivate_if_active(
            "plan",
            || Ok("PLAN".into()),
            |_| Err("activation failed".into()),
        )
        .unwrap_err();
        assert_eq!(error.stage(), ProcessorPowerApplyStage::ReactivatePlan);
        let mut activated = false;
        reactivate_if_active(
            "plan",
            || Ok("PLAN".into()),
            |guid| {
                assert_eq!(guid, "plan");
                activated = true;
                Ok(())
            },
        )
        .unwrap();
        assert!(activated);
    }
}
