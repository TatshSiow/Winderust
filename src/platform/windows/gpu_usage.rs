use std::{collections::HashMap, mem::size_of, ptr};

use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_CSTATUS_INVALID_DATA, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA,
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};

const GPU_ENGINE_COUNTER: *const u16 = windows_sys::w!(r"\GPU Engine(*)\Utilization Percentage");

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("PDH GPU Engine query failed with status 0x{0:08X}.")]
pub(crate) struct GpuUsageError(u32);

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
        let status = unsafe { PdhAddEnglishCounterW(query, GPU_ENGINE_COUNTER, 0, &mut counter) };
        if status != 0 {
            // SAFETY: `query` was successfully opened above and is not used after closing.
            unsafe { PdhCloseQuery(query) };
            return Err(GpuUsageError(status));
        }

        Ok(Self { query, counter })
    }

    pub(crate) fn sample(&self) -> Result<Option<f32>, GpuUsageError> {
        self.formatted_samples()
            .map(|samples| samples.as_deref().and_then(busiest_engine))
    }

    pub(crate) fn sample_processes(&self) -> Result<Option<HashMap<u32, f32>>, GpuUsageError> {
        self.formatted_samples()
            .map(|samples| samples.as_deref().map(busiest_engines_by_process))
    }

    fn formatted_samples(&self) -> Result<Option<Vec<(String, f64)>>, GpuUsageError> {
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

        const _: () = assert!(align_of::<u64>() >= align_of::<PDH_FMT_COUNTERVALUE_ITEM_W>());
        let words = (byte_count as usize).div_ceil(size_of::<u64>());
        let mut buffer = vec![0u64; words];
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

        let byte_count = byte_count as usize;
        let item_count = item_count as usize;
        let Some(array_bytes) = item_count.checked_mul(size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>())
        else {
            return Ok(None);
        };
        if array_bytes > byte_count || byte_count > size_of_val(buffer.as_slice()) {
            return Ok(None);
        }
        // SAFETY: PDH initialized this aligned item array, whose checked length is within storage.
        let items = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(),
                item_count,
            )
        };
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
            let Some(name) = bounded_wide_string(item.szName, &buffer, array_bytes, byte_count)
            else {
                continue;
            };
            samples.push((name, value));
        }
        Ok(Some(samples))
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

fn busiest_engines_by_process(samples: &[(String, f64)]) -> HashMap<u32, f32> {
    let mut processes = HashMap::<u32, f32>::new();
    for (name, value) in samples {
        let Some(pid) = name
            .strip_prefix("pid_")
            .and_then(|rest| rest.split_once('_'))
            .and_then(|(pid, _)| pid.parse().ok())
        else {
            continue;
        };
        let engine = value.clamp(0.0, 100.0) as f32;
        let busiest = processes.entry(pid).or_insert(engine);
        *busiest = busiest.max(engine);
    }
    processes
}

fn bounded_wide_string(
    pointer: *const u16,
    buffer: &[u64],
    array_bytes: usize,
    byte_count: usize,
) -> Option<String> {
    let offset = pointer.addr().checked_sub(buffer.as_ptr().addr())?;
    if offset < array_bytes || offset >= byte_count || !offset.is_multiple_of(align_of::<u16>()) {
        return None;
    }
    // SAFETY: The aligned string range lies past the item array inside initialized storage.
    let wide = unsafe {
        std::slice::from_raw_parts(
            buffer.as_ptr().cast::<u8>().add(offset).cast::<u16>(),
            (byte_count - offset) / size_of::<u16>(),
        )
    };
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
    fn per_process_usage_uses_the_busiest_engine_across_adapters() {
        let samples = [
            ("pid_1_luid_0_phys_0_eng_0_engtype_3D".into(), 40.0),
            ("pid_1_luid_1_phys_0_eng_1_engtype_Copy".into(), 55.0),
            ("pid_2_luid_0_phys_0_eng_0_engtype_3D".into(), 101.0),
            ("pid_3_luid_0_phys_0_eng_0_engtype_3D".into(), 0.0),
            ("pid_1x_luid_0_phys_0_eng_0_engtype_3D".into(), 99.0),
            ("_Total".into(), 99.0),
        ];
        assert_eq!(
            busiest_engines_by_process(&samples),
            HashMap::from([(1, 55.0), (2, 100.0), (3, 0.0)])
        );
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

    #[test]
    #[ignore = "reads live Windows GPU Engine performance counters"]
    fn live_gpu_engine_query_returns_process_samples() {
        let query = GpuUsageQuery::open().unwrap();
        query.sample_processes().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        let processes = query.sample_processes().unwrap().unwrap();
        assert!(
            !processes.is_empty(),
            "GPU Engine returned no process samples"
        );
        assert!(processes
            .values()
            .all(|value| (0.0..=100.0).contains(value)));
        eprintln!(
            "Read valid GPU engine utilization for {} PIDs.",
            processes.len()
        );
    }
}
