pub mod plan;
pub mod powercfg;

pub use plan::{
    PowerPlan, ProcessorBoostMode, ProcessorPowerAcDcValues, ProcessorPowerPreset,
    ProcessorPowerValues,
};
pub use powercfg::PowerPlanManager;
