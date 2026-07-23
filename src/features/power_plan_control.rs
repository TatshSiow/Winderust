pub(crate) mod by_cpu_load;
pub(crate) mod by_running_app;
pub(crate) mod by_time;

pub(crate) use by_cpu_load::{ByCpuLoadDecision, ByCpuLoadScheduler};
pub(crate) use by_time::{
    current_decision as current_by_time_decision, next_change_delay as next_by_time_change_delay,
    next_switch_label as next_by_time_switch_label, ByTimeDecision,
};
