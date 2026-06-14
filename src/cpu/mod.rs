use std::{ffi::c_void, mem::size_of, ptr::null_mut};

use windows_sys::{
    Wdk::System::SystemInformation::{
        NtQuerySystemInformation, SystemProcessorPerformanceInformation,
    },
    Win32::{
        Foundation::FILETIME,
        System::{
            Threading::GetSystemTimes, WindowsProgramming::SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION,
        },
    },
};

#[derive(Debug, Clone, Copy, Default)]
pub struct CpuUsageSnapshot {
    pub percent: Option<f32>,
}

#[derive(Debug, Default)]
pub struct CpuUsageMonitor {
    previous: Option<SystemCpuTimes>,
}

#[derive(Debug, Default)]
pub struct PerProcessorUsageMonitor {
    previous: Option<Vec<ProcessorCpuTimes>>,
}

#[derive(Debug, Clone, Copy)]
struct SystemCpuTimes {
    idle: u64,
    kernel: u64,
    user: u64,
}

#[derive(Debug, Clone, Copy)]
struct ProcessorCpuTimes {
    idle: u64,
    kernel: u64,
    user: u64,
}

impl CpuUsageMonitor {
    pub fn sample(&mut self) -> CpuUsageSnapshot {
        let Some(current) = read_system_cpu_times() else {
            return CpuUsageSnapshot { percent: None };
        };

        let percent = self.previous.and_then(|previous| {
            let idle_delta = current.idle.saturating_sub(previous.idle);
            let kernel_delta = current.kernel.saturating_sub(previous.kernel);
            let user_delta = current.user.saturating_sub(previous.user);
            let total_delta = kernel_delta + user_delta;

            if total_delta == 0 {
                None
            } else {
                let used = total_delta.saturating_sub(idle_delta);
                Some(((used as f32 / total_delta as f32) * 100.0).clamp(0.0, 100.0))
            }
        });

        self.previous = Some(current);
        CpuUsageSnapshot { percent }
    }
}

impl PerProcessorUsageMonitor {
    pub fn sample(&mut self) -> Option<Vec<f32>> {
        let current = read_processor_cpu_times()?;
        let usage = self.previous.as_ref().and_then(|previous| {
            (previous.len() == current.len()).then(|| {
                previous
                    .iter()
                    .zip(current.iter())
                    .map(|(previous, current)| processor_usage_percent(*previous, *current))
                    .collect::<Vec<_>>()
            })
        });

        self.previous = Some(current);
        usage
    }
}

fn read_system_cpu_times() -> Option<SystemCpuTimes> {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();

    let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
    if ok == 0 {
        return None;
    }

    Some(SystemCpuTimes {
        idle: filetime_to_u64(idle),
        kernel: filetime_to_u64(kernel),
        user: filetime_to_u64(user),
    })
}

fn read_processor_cpu_times() -> Option<Vec<ProcessorCpuTimes>> {
    let processor_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .max(1);
    let mut records = vec![SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION::default(); processor_count];
    let byte_len = records
        .len()
        .checked_mul(size_of::<SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION>())?;
    let byte_len = u32::try_from(byte_len).ok()?;
    let status = unsafe {
        NtQuerySystemInformation(
            SystemProcessorPerformanceInformation,
            records.as_mut_ptr().cast::<c_void>(),
            byte_len,
            null_mut(),
        )
    };
    if status < 0 {
        return None;
    }

    Some(
        records
            .into_iter()
            .map(|record| ProcessorCpuTimes {
                idle: record.IdleTime.max(0) as u64,
                kernel: record.KernelTime.max(0) as u64,
                user: record.UserTime.max(0) as u64,
            })
            .collect(),
    )
}

fn processor_usage_percent(previous: ProcessorCpuTimes, current: ProcessorCpuTimes) -> f32 {
    let idle_delta = current.idle.saturating_sub(previous.idle);
    let kernel_delta = current.kernel.saturating_sub(previous.kernel);
    let user_delta = current.user.saturating_sub(previous.user);
    let total_delta = kernel_delta + user_delta;

    if total_delta == 0 {
        0.0
    } else {
        let used = total_delta.saturating_sub(idle_delta);
        ((used as f32 / total_delta as f32) * 100.0).clamp(0.0, 100.0)
    }
}

fn filetime_to_u64(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combines_filetime_parts() {
        let value = FILETIME {
            dwLowDateTime: 0x89ab_cdef,
            dwHighDateTime: 0x0123_4567,
        };

        assert_eq!(filetime_to_u64(value), 0x0123_4567_89ab_cdef);
    }

    #[test]
    fn processor_usage_scales_from_idle_kernel_user_deltas() {
        let previous = ProcessorCpuTimes {
            idle: 10,
            kernel: 20,
            user: 10,
        };
        let current = ProcessorCpuTimes {
            idle: 20,
            kernel: 50,
            user: 30,
        };

        assert_eq!(processor_usage_percent(previous, current), 80.0);
    }
}
