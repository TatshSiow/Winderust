use std::{
    ffi::c_void,
    iter::once,
    mem::size_of,
    ptr::{null, null_mut},
    time::Instant,
};

use windows_sys::{
    Wdk::System::SystemInformation::{
        NtQuerySystemInformation, SystemProcessorPerformanceInformation,
    },
    Win32::{
        Foundation::FILETIME,
        System::{
            Performance::{
                PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
                PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_CSTATUS_NEW_DATA,
                PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_HCOUNTER,
                PDH_HQUERY,
            },
            Power::{CallNtPowerInformation, ProcessorInformation, PROCESSOR_POWER_INFORMATION},
            Threading::GetSystemTimes,
            WindowsProgramming::SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION,
        },
    },
};

use crate::win_util::filetime_to_u64;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CpuUsageSnapshot {
    pub percent: Option<f32>,
    pub frequency_mhz: Option<u32>,
    pub base_frequency_mhz: Option<u32>,
}

#[derive(Debug, Default)]
pub struct CpuUsageMonitor {
    previous: Option<SystemCpuTimes>,
    frequency_counter: Option<CpuFrequencyCounter>,
    frequency_counter_unavailable: bool,
}

#[derive(Debug, Default)]
pub struct PerProcessorUsageMonitor {
    previous: Option<Vec<ProcessorCpuTimes>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessCpuSample {
    pub cpu_time_100ns: u64,
    pub sampled_at: Instant,
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

#[derive(Debug)]
struct CpuFrequencyCounter {
    query: PDH_HQUERY,
    frequency_counter: PDH_HCOUNTER,
    performance_counter: Option<PDH_HCOUNTER>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CpuFrequencySample {
    frequency_mhz: u32,
    nominal_frequency_mhz: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProcessorPowerFrequencySample {
    current_mhz: u32,
    base_mhz: Option<u32>,
}

impl CpuUsageMonitor {
    pub fn sample(&mut self) -> CpuUsageSnapshot {
        let live_frequency = self.sample_live_cpu_frequency();
        let processor_power_frequency = read_processor_power_frequency();
        let frequency_mhz = live_frequency
            .map(|sample| sample.frequency_mhz)
            .or_else(|| processor_power_frequency.map(|sample| sample.current_mhz));
        let base_frequency_mhz = live_frequency
            .and_then(|sample| sample.nominal_frequency_mhz)
            .or_else(|| processor_power_frequency.and_then(|sample| sample.base_mhz));

        let Some(current) = read_system_cpu_times() else {
            return CpuUsageSnapshot {
                percent: None,
                frequency_mhz,
                base_frequency_mhz,
            };
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
        CpuUsageSnapshot {
            percent,
            frequency_mhz,
            base_frequency_mhz,
        }
    }

    fn sample_live_cpu_frequency(&mut self) -> Option<CpuFrequencySample> {
        if self.frequency_counter.is_none() && !self.frequency_counter_unavailable {
            self.frequency_counter = CpuFrequencyCounter::open();
            self.frequency_counter_unavailable = self.frequency_counter.is_none();
        }

        self.frequency_counter
            .as_ref()
            .and_then(CpuFrequencyCounter::sample)
    }
}

impl CpuFrequencyCounter {
    fn open() -> Option<Self> {
        let mut query = null_mut();
        let status = unsafe { PdhOpenQueryW(null(), 0, &mut query) };
        if status != 0 || query.is_null() {
            return None;
        }

        let frequency_path = wide_null(r"\Processor Information(_Total)\Processor Frequency");
        let mut frequency_counter = null_mut();
        let status = unsafe {
            PdhAddEnglishCounterW(query, frequency_path.as_ptr(), 0, &mut frequency_counter)
        };
        if status != 0 || frequency_counter.is_null() {
            unsafe {
                PdhCloseQuery(query);
            }
            return None;
        }

        let performance_path = wide_null(r"\Processor Information(_Total)\% Processor Performance");
        let mut performance_counter = null_mut();
        let performance_counter = match unsafe {
            PdhAddEnglishCounterW(
                query,
                performance_path.as_ptr(),
                0,
                &mut performance_counter,
            )
        } {
            0 if !performance_counter.is_null() => Some(performance_counter),
            _ => None,
        };

        unsafe {
            PdhCollectQueryData(query);
        }

        Some(Self {
            query,
            frequency_counter,
            performance_counter,
        })
    }

    fn sample(&self) -> Option<CpuFrequencySample> {
        let status = unsafe { PdhCollectQueryData(self.query) };
        if status != 0 {
            return None;
        }

        let nominal_frequency_mhz = read_pdh_counter_double(self.frequency_counter);
        let nominal_frequency_mhz_u32 = nominal_frequency_mhz.and_then(frequency_mhz_to_u32);
        let performance_percent = self.performance_counter.and_then(read_pdh_counter_double);

        let frequency_mhz = performance_percent
            .zip(nominal_frequency_mhz)
            .and_then(|(performance_percent, nominal_frequency_mhz)| {
                effective_frequency_mhz(nominal_frequency_mhz, performance_percent)
            })
            .or(nominal_frequency_mhz_u32)?;

        Some(CpuFrequencySample {
            frequency_mhz,
            nominal_frequency_mhz: nominal_frequency_mhz_u32,
        })
    }
}

impl Drop for CpuFrequencyCounter {
    fn drop(&mut self) {
        if !self.query.is_null() {
            unsafe {
                PdhCloseQuery(self.query);
            }
        }
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

pub fn process_cpu_usage_percent(
    previous: ProcessCpuSample,
    current: ProcessCpuSample,
) -> Option<f32> {
    let elapsed = current.sampled_at.duration_since(previous.sampled_at);
    let elapsed_100ns = elapsed.as_nanos() / 100;
    if elapsed_100ns == 0 {
        return None;
    }

    let cpu_delta = current
        .cpu_time_100ns
        .saturating_sub(previous.cpu_time_100ns) as f64;
    let processor_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .max(1) as f64;
    Some(((cpu_delta / (elapsed_100ns as f64 * processor_count)) * 100.0).clamp(0.0, 100.0) as f32)
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

fn read_processor_power_frequency() -> Option<ProcessorPowerFrequencySample> {
    let processor_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .max(1);
    let mut records = vec![PROCESSOR_POWER_INFORMATION::default(); processor_count];
    let byte_len = records
        .len()
        .checked_mul(size_of::<PROCESSOR_POWER_INFORMATION>())?;
    let byte_len = u32::try_from(byte_len).ok()?;
    let status = unsafe {
        CallNtPowerInformation(
            ProcessorInformation,
            null::<c_void>(),
            0,
            records.as_mut_ptr().cast::<c_void>(),
            byte_len,
        )
    };
    if status != 0 {
        return None;
    }

    average_processor_power_frequency(&records)
}

fn read_pdh_counter_double(counter: PDH_HCOUNTER) -> Option<f64> {
    let mut value_type = 0;
    let mut value = PDH_FMT_COUNTERVALUE::default();
    let status = unsafe {
        PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, &mut value_type, &mut value)
    };
    if status != 0 || !matches!(value.CStatus, PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA) {
        return None;
    }

    Some(unsafe { value.Anonymous.doubleValue })
}

fn effective_frequency_mhz(nominal_frequency_mhz: f64, performance_percent: f64) -> Option<u32> {
    if !nominal_frequency_mhz.is_finite()
        || !performance_percent.is_finite()
        || nominal_frequency_mhz <= 0.0
        || performance_percent <= 0.0
    {
        return None;
    }

    frequency_mhz_to_u32(nominal_frequency_mhz * (performance_percent / 100.0))
}

fn frequency_mhz_to_u32(frequency_mhz: f64) -> Option<u32> {
    if !frequency_mhz.is_finite() || frequency_mhz <= 0.0 || frequency_mhz > u32::MAX as f64 {
        return None;
    }

    Some(frequency_mhz.round() as u32)
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(once(0)).collect()
}

fn average_processor_power_frequency(
    records: &[PROCESSOR_POWER_INFORMATION],
) -> Option<ProcessorPowerFrequencySample> {
    let mut total = 0u64;
    let mut count = 0u64;
    let mut max_total = 0u64;
    let mut max_count = 0u64;

    for record in records {
        if record.CurrentMhz != 0 {
            total = total.saturating_add(u64::from(record.CurrentMhz));
            count += 1;
        }
        if record.MaxMhz != 0 {
            max_total = max_total.saturating_add(u64::from(record.MaxMhz));
            max_count += 1;
        }
    }

    (count > 0).then_some(ProcessorPowerFrequencySample {
        current_mhz: (total / count) as u32,
        base_mhz: (max_count > 0).then_some((max_total / max_count) as u32),
    })
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

    #[test]
    fn averages_non_zero_processor_frequency_samples() {
        let records = [
            PROCESSOR_POWER_INFORMATION {
                CurrentMhz: 3200,
                MaxMhz: 5000,
                ..Default::default()
            },
            PROCESSOR_POWER_INFORMATION {
                CurrentMhz: 0,
                MaxMhz: 0,
                ..Default::default()
            },
            PROCESSOR_POWER_INFORMATION {
                CurrentMhz: 3400,
                MaxMhz: 5200,
                ..Default::default()
            },
        ];

        assert_eq!(
            average_processor_power_frequency(&records),
            Some(ProcessorPowerFrequencySample {
                current_mhz: 3300,
                base_mhz: Some(5100),
            })
        );
    }

    #[test]
    fn rejects_invalid_live_frequency_samples() {
        assert_eq!(frequency_mhz_to_u32(3199.6), Some(3200));
        assert_eq!(frequency_mhz_to_u32(0.0), None);
        assert_eq!(frequency_mhz_to_u32(f64::NAN), None);
    }

    #[test]
    fn computes_effective_frequency_from_processor_performance() {
        assert_eq!(effective_frequency_mhz(2200.0, 215.31), Some(4737));
        assert_eq!(effective_frequency_mhz(2200.0, 0.0), None);
        assert_eq!(effective_frequency_mhz(0.0, 215.31), None);
    }
}
