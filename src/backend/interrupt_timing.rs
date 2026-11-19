pub(crate) use crate::platform::windows::interrupt_timing::{
    is_worker_process, run_worker_if_requested, InterruptKind, InterruptTimingCapture,
    InterruptTimingError, InterruptTimingReport,
};
