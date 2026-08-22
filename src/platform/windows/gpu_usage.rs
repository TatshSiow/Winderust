use std::{collections::HashMap, fmt, mem::size_of, ptr};

use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_CSTATUS_INVALID_DATA, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA,
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};

const GPU_ENGINE_COUNTER: &[u16] = &[
    92, 71, 80, 85, 32, 69, 110, 103, 105, 110, 101, 40, 42, 41, 92, 85, 116, 105, 108, 105, 122,
    97, 116, 105, 111, 110, 32, 80, 101, 114, 99, 101, 110, 116, 97, 103, 101, 0,
];

#[derive(Debug, Clone, Copy)]
pub(crate) struct GpuUsageError(u32);

impl fmt::Display for GpuUsageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "PDH GPU Engine query failed with status 0x{:08X}.",
            self.0
        )
    }
}

pub(crate) struct GpuUsageQuery {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
}

impl GpuUsageQuery {
    pub(crate) fn open() -> Result<Self, GpuUsageError> {
        let mut query = ptr::null_mut();
        // SAFETY: The output pointer is valid and PDH owns the returned query handle.
        let status = unsafe { PdhOpenQueryW(ptr::null(), 0, &mut query) };
        if status != 0 {
            return Err(GpuUsageError(status));
        }

        let mut counter = ptr::null_mut();
        // SAFETY: `query` is an open PDH query and the counter path is NUL-terminated.
        let status =
            unsafe { PdhAddEnglishCounterW(query, GPU_ENGINE_COUNTER.as_ptr(), 0, &mut counter) };
        if status != 0 {
            // SAFETY: `query` was successfully opened above and is not used after closing.
            unsafe { PdhCloseQuery(query) };
            return Err(GpuUsageError(status));
        }

        Ok(Self { query, counter })
    }

    pub(crate) fn sample(&self) -> Result<Option<f32>, GpuUsageError> {
        // SAFETY: `self.query` stays open for the lifetime of this object.
        let status = unsafe { PdhCollectQueryData(self.query) };
        if status == PDH_CSTATUS_INVALID_DATA {
            return Ok(None);
        }
        if status != 0 {
            return Err(GpuUsageError(status));
        }

        let mut byte_count = 0;
        let mut item_count = 0;
        // SAFETY: A null buffer asks PDH for the required byte count.
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut byte_count,
                &mut item_count,
                ptr::null_mut(),
            )
        };
        if status == PDH_CSTATUS_INVALID_DATA {
            return Ok(None);
        }
        if status != PDH_MORE_DATA {
            return if status == 0 {
                Ok(None)
            } else {
                Err(GpuUsageError(status))
            };
        }

        let words = (byte_count as usize).div_ceil(size_of::<usize>());
        let mut buffer = vec![0usize; words];
        // SAFETY: The aligned buffer has the byte capacity requested by PDH.
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut byte_count,
                &mut item_count,
                buffer.as_mut_ptr().cast(),
            )
        };
        if status == PDH_CSTATUS_INVALID_DATA {
            return Ok(None);
        }
        if status != 0 {
            return Err(GpuUsageError(status));
        }

        // SAFETY: PDH wrote `item_count` formatted items at the start of the aligned buffer.
        let items = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(),
                item_count as usize,
            )
        };
        let start = buffer.as_ptr().addr();
        let end = start.saturating_add(buffer.len() * size_of::<usize>());
        let mut samples = Vec::with_capacity(items.len());
        for item in items {
            if !matches!(
                item.FmtValue.CStatus,
                PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA
            ) {
                continue;
            }
            // SAFETY: PDH_FMT_DOUBLE selects the `doubleValue` union field.
            let value = unsafe { item.FmtValue.Anonymous.doubleValue };
            if !value.is_finite() || value < 0.0 {
                continue;
            }
            let Some(name) = bounded_wide_string(item.szName, start, end) else {
                continue;
            };
            samples.push((name, value));
        }
        Ok(busiest_engine(&samples))
    }
}

fn busiest_engine(samples: &[(String, f64)]) -> Option<f32> {
    let mut engines = HashMap::<&str, f64>::new();
    for (name, value) in samples {
        let key = name
            .strip_prefix("pid_")
            .and_then(|rest| rest.find('_').map(|split| &rest[split + 1..]))
            .unwrap_or(name);
        *engines.entry(key).or_default() += value;
    }
    engines
        .values()
        .copied()
        .map(|value| value.clamp(0.0, 100.0) as f32)
        .reduce(f32::max)
}

fn bounded_wide_string(pointer: *const u16, start: usize, end: usize) -> Option<String> {
    let address = pointer.addr();
    if pointer.is_null() || address < start || address >= end || !address.is_multiple_of(2) {
        return None;
    }
    let max_len = (end - address) / size_of::<u16>();
    // SAFETY: The pointer was validated inside the PDH-owned output buffer and max_len stays within it.
    let wide = unsafe { std::slice::from_raw_parts(pointer, max_len) };
    let len = wide.iter().position(|unit| *unit == 0)?;
    Some(String::from_utf16_lossy(&wide[..len]))
}

impl Drop for GpuUsageQuery {
    fn drop(&mut self) {
        // SAFETY: This object uniquely owns the open query handle.
        unsafe { PdhCloseQuery(self.query) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_process_samples_are_summed_by_physical_engine() {
        let samples = vec![
            ("pid_1_luid_0_phys_0_eng_0_engtype_3D".to_owned(), 40.0),
            ("pid_2_luid_0_phys_0_eng_0_engtype_3D".to_owned(), 35.0),
            ("pid_1_luid_0_phys_0_eng_1_engtype_Copy".to_owned(), 20.0),
        ];
        assert_eq!(busiest_engine(&samples), Some(75.0));
    }

    #[test]
    #[ignore = "reads live Windows GPU Engine performance counters"]
    fn live_gpu_engine_query_returns_a_sample() {
        let query = GpuUsageQuery::open().unwrap_or_else(|error| panic!("{error}"));
        let _ = query.sample().unwrap_or_else(|error| panic!("{error}"));
        std::thread::sleep(std::time::Duration::from_secs(1));
        let sample = query.sample().unwrap_or_else(|error| panic!("{error}"));
        assert!(sample.is_some(), "GPU Engine returned no valid sample");
    }
}
