pub mod plan;
pub mod powercfg;

pub use plan::{
    adaptive_power_profile_transition, AdaptivePowerDemand, AdaptivePowerProfile,
    EffectivePowerMode, PowerPlan, PowerPlanPersonality, ProcessorBoostMode,
    ProcessorPowerAcDcValues, ProcessorPowerPreset, ProcessorPowerValues,
};
pub use powercfg::{
    active_plan, apply_processor_power_values, create_adaptive_plan, delete_plan, list_plans,
    read_plan_personality, read_processor_power_values, restore_stale_adaptive_plans, set_active,
    EffectivePowerModeMonitor,
};
