use std::{
    ffi::c_void,
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

use crate::win_util::{filetime_to_u64, wide_null};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CpuUsageSnapshot {
    pub percent: Option<f32>,
    pub frequency_mhz: Option<u32>,
    pub base_frequency_mhz: Option<u32>,
}

#[derive(Debug, Default)]
pub struct CpuUsageMonitor {
    previous_times: Option<CpuTimeCounters>,
    frequency_counter: Option<CpuFrequencyCounter>,
    frequency_counter_unavailable: bool,
}

#[derive(Debug, Default)]
pub struct PerProcessorUsageMonitor {
    previous_times: Option<Vec<CpuTimeCounters>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessCpuSample {
    pub cpu_time_100ns: u64,
    pub sampled_at: Instant,
}

#[derive(Debug, Clone, Copy)]
struct CpuTimeCounters {
    idle: u64,
    kernel: u64,
    user: u64,
}

#[derive(Debug)]
struct CpuFrequencyCounter {
    query: PDH_HQUERY,
    frequency: PDH_HCOUNTER,
    performance: Option<PDH_HCOUNTER>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CpuFrequencySample {
    frequency_mhz: u32,
    base_frequency_mhz: Option<u32>,
}

impl CpuUsageMonitor {
    pub fn sample(&mut self) -> CpuUsageSnapshot {
        self.sample_with_frequency(true)
    }

    pub fn sample_usage(&mut self) -> CpuUsageSnapshot {
        self.sample_with_frequency(false)
    }

    fn sample_with_frequency(&mut self, include_frequency: bool) -> CpuUsageSnapshot {
        let (frequency_mhz, base_frequency_mhz) = if include_frequency {
            self.sample_frequency()
        } else {
            (None, None)
        };

        let Some(current) = read_system_cpu_times() else {
            return CpuUsageSnapshot {
                percent: None,
                frequency_mhz,
                base_frequency_mhz,
            };
        };

        let percent = self
            .previous_times
            .and_then(|previous| cpu_usage_percent(previous, current));

        self.previous_times = Some(current);
        CpuUsageSnapshot {
            percent,
            frequency_mhz,
            base_frequency_mhz,
        }
    }

    fn sample_frequency(&mut self) -> (Option<u32>, Option<u32>) {
        let pdh_sample = self.sample_live_cpu_frequency();
        let frequency_mhz = pdh_sample.map(|sample| sample.frequency_mhz);
        let base_frequency_mhz = pdh_sample.and_then(|sample| sample.base_frequency_mhz);
        if frequency_mhz.is_some() && base_frequency_mhz.is_some() {
            return (frequency_mhz, base_frequency_mhz);
        }

        let power_sample = read_processor_power_frequency();
        let frequency_mhz =
            frequency_mhz.or_else(|| power_sample.map(|sample| sample.frequency_mhz));
        let base_frequency_mhz = base_frequency_mhz
            .or_else(|| power_sample.and_then(|sample| sample.base_frequency_mhz));

        (frequency_mhz, base_frequency_mhz)
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
        // SAFETY: No data source is specified and query is a writable out-pointer.
        let status = unsafe { PdhOpenQueryW(null(), 0, &mut query) };
        if status != 0 || query.is_null() {
            return None;
        }

        let frequency_path = wide_null(r"\Processor Information(_Total)\Processor Frequency");
        let mut frequency = null_mut();
        // SAFETY: query is live, frequency_path is terminated UTF-16, and frequency is a
        // writable out-pointer.
        let status =
            unsafe { PdhAddEnglishCounterW(query, frequency_path.as_ptr(), 0, &mut frequency) };
        if status != 0 || frequency.is_null() {
            // SAFETY: query was opened successfully and is closed exactly once on this path.
            unsafe {
                PdhCloseQuery(query);
            }
            return None;
        }

        let performance_path = wide_null(r"\Processor Information(_Total)\% Processor Performance");
        let mut performance = null_mut();
        // SAFETY: query is live, performance_path is terminated UTF-16, and performance
        // is a writable out-pointer.
        let performance = match unsafe {
            PdhAddEnglishCounterW(query, performance_path.as_ptr(), 0, &mut performance)
        } {
            0 if !performance.is_null() => Some(performance),
            _ => None,
        };

        // SAFETY: query is live; collection does not retain borrowed pointers.
        unsafe {
            PdhCollectQueryData(query);
        }

        Some(Self {
            query,
            frequency,
            performance,
        })
    }

    fn sample(&self) -> Option<CpuFrequencySample> {
        // SAFETY: self.query remains live for the lifetime of this counter wrapper.
        let status = unsafe { PdhCollectQueryData(self.query) };
        if status != 0 {
            return None;
        }

        let base_frequency_mhz = read_pdh_counter_double(self.frequency);
        let base_frequency_mhz_u32 = base_frequency_mhz.and_then(frequency_mhz_to_u32);
        let performance_percent = self.performance.and_then(read_pdh_counter_double);

        let frequency_mhz = performance_percent
            .zip(base_frequency_mhz)
            .and_then(|(performance_percent, base_frequency_mhz)| {
                effective_frequency_mhz(base_frequency_mhz, performance_percent)
            })
            .or(base_frequency_mhz_u32)?;

        Some(CpuFrequencySample {
            frequency_mhz,
            base_frequency_mhz: base_frequency_mhz_u32,
        })
    }
}

impl Drop for CpuFrequencyCounter {
    fn drop(&mut self) {
        // SAFETY: query is a non-null handle owned by this wrapper and closed exactly once.
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

impl PerProcessorUsageMonitor {
    pub fn sample(&mut self) -> Option<Vec<f32>> {
        let current = read_processor_cpu_times()?;
        let usage = self.previous_times.as_ref().and_then(|previous| {
            (previous.len() == current.len()).then(|| {
                previous
                    .iter()
                    .zip(current.iter())
                    .map(|(previous, current)| processor_usage_percent(*previous, *current))
                    .collect::<Vec<_>>()
            })
        });

        self.previous_times = Some(current);
        usage
    }
}

fn logical_processor_count() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
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
    let processor_count = logical_processor_count() as f64;
    Some(((cpu_delta / (elapsed_100ns as f64 * processor_count)) * 100.0).clamp(0.0, 100.0) as f32)
}

fn read_system_cpu_times() -> Option<CpuTimeCounters> {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();

    // SAFETY: All three FILETIME outputs are writable for the duration of the call.
    let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
    if ok == 0 {
        return None;
    }

    Some(CpuTimeCounters {
        idle: filetime_to_u64(idle),
        kernel: filetime_to_u64(kernel),
        user: filetime_to_u64(user),
    })
}

fn cpu_usage_percent(previous: CpuTimeCounters, current: CpuTimeCounters) -> Option<f32> {
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
}

fn read_processor_cpu_times() -> Option<Vec<CpuTimeCounters>> {
    let processor_count = logical_processor_count();
    let mut records = vec![SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION::default(); processor_count];
    let byte_len = records
        .len()
        .checked_mul(size_of::<SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION>())?;
    let byte_len = u32::try_from(byte_len).ok()?;
    // SAFETY: records provides byte_len writable bytes of the requested record type and no return
    // length is required.
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
            .map(|record| CpuTimeCounters {
                idle: record.IdleTime.max(0) as u64,
                kernel: record.KernelTime.max(0) as u64,
                user: record.UserTime.max(0) as u64,
            })
            .collect(),
    )
}

fn read_processor_power_frequency() -> Option<CpuFrequencySample> {
    let processor_count = logical_processor_count();
    let mut records = vec![PROCESSOR_POWER_INFORMATION::default(); processor_count];
    let byte_len = records
        .len()
        .checked_mul(size_of::<PROCESSOR_POWER_INFORMATION>())?;
    let byte_len = u32::try_from(byte_len).ok()?;
    // SAFETY: records provides byte_len writable bytes for processor power records; the input
    // buffer is intentionally null and empty.
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
    let mut _counter_type = 0;
    let mut value = PDH_FMT_COUNTERVALUE::default();
    // SAFETY: counter is a live PDH counter and both output values are writable for the call.
    let status = unsafe {
        PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, &mut _counter_type, &mut value)
    };
    if status != 0 || !matches!(value.CStatus, PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA) {
        return None;
    }

    // SAFETY: PDH_FMT_DOUBLE requests and initializes the doubleValue union member.
    Some(unsafe { value.Anonymous.doubleValue })
}

fn effective_frequency_mhz(base_frequency_mhz: f64, performance_percent: f64) -> Option<u32> {
    if !base_frequency_mhz.is_finite()
        || !performance_percent.is_finite()
        || base_frequency_mhz <= 0.0
        || performance_percent <= 0.0
    {
        return None;
    }

    frequency_mhz_to_u32(base_frequency_mhz * (performance_percent / 100.0))
}

fn frequency_mhz_to_u32(frequency_mhz: f64) -> Option<u32> {
    if !frequency_mhz.is_finite() || frequency_mhz <= 0.0 || frequency_mhz > u32::MAX as f64 {
        return None;
    }

    Some(frequency_mhz.round() as u32)
}

fn average_processor_power_frequency(
    records: &[PROCESSOR_POWER_INFORMATION],
) -> Option<CpuFrequencySample> {
    let mut total = 0u64;
    let mut count = 0u64;
    let mut max_frequency_total_mhz = 0u64;
    let mut max_frequency_count = 0u64;

    for record in records {
        if record.CurrentMhz != 0 {
            total = total.saturating_add(u64::from(record.CurrentMhz));
            count += 1;
        }
        if record.MaxMhz != 0 {
            max_frequency_total_mhz =
                max_frequency_total_mhz.saturating_add(u64::from(record.MaxMhz));
            max_frequency_count += 1;
        }
    }

    (count > 0).then_some(CpuFrequencySample {
        frequency_mhz: (total / count) as u32,
        base_frequency_mhz: (max_frequency_count > 0)
            .then_some((max_frequency_total_mhz / max_frequency_count) as u32),
    })
}

fn processor_usage_percent(previous: CpuTimeCounters, current: CpuTimeCounters) -> f32 {
    cpu_usage_percent(previous, current).unwrap_or_default()
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
        let previous = CpuTimeCounters {
            idle: 10,
            kernel: 20,
            user: 10,
        };
        let current = CpuTimeCounters {
            idle: 20,
            kernel: 50,
            user: 30,
        };

        assert_eq!(processor_usage_percent(previous, current), 80.0);
    }

    #[test]
    fn system_cpu_usage_scales_from_idle_kernel_user_deltas() {
        let previous = CpuTimeCounters {
            idle: 10,
            kernel: 20,
            user: 10,
        };
        let current = CpuTimeCounters {
            idle: 20,
            kernel: 50,
            user: 30,
        };

        assert_eq!(cpu_usage_percent(previous, current), Some(80.0));
        assert_eq!(cpu_usage_percent(current, current), None);
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
            Some(CpuFrequencySample {
                frequency_mhz: 3300,
                base_frequency_mhz: Some(5100),
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
