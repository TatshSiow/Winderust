use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER};

use crate::win_util::last_error;

pub(crate) mod cpu_allocation;
pub(crate) mod cpu_limiter;
pub(crate) mod dynamic_priority_boost;
pub(crate) mod gpu_priority;
pub(crate) mod gpu_usage;
pub(crate) mod io_priority;
pub(crate) mod job;
pub(crate) mod memory_priority;
pub(crate) mod memory_trim;
pub(crate) mod power_plan;
pub(crate) mod priority_efficiency;
pub(crate) mod process;
pub(crate) mod process_termination;
pub(crate) mod self_power;
pub(crate) mod suspension;
pub(crate) mod thread_priority;
pub(crate) mod thread_suspension;
pub(crate) mod timer_resolution;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessOperationError {
    AccessDenied { operation: &'static str },
    ProcessExited,
    Failed { operation: &'static str, code: u32 },
}

impl ProcessOperationError {
    fn capture(operation: &'static str) -> Self {
        Self::from_code(operation, last_error())
    }

    fn from_code(operation: &'static str, code: u32) -> Self {
        match code {
            ERROR_ACCESS_DENIED => Self::AccessDenied { operation },
            ERROR_INVALID_PARAMETER => Self::ProcessExited,
            code => Self::Failed { operation, code },
        }
    }
}
