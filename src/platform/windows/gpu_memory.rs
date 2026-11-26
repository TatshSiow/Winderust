use std::{
    collections::{BTreeMap, BTreeSet},
    mem::size_of,
    ptr,
};

use windows_sys::{
    w,
    Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W,
        PDH_FMT_LARGE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    },
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GpuMemorySample {
    pub(crate) dedicated_bytes: Option<u64>,
    pub(crate) shared_bytes: Option<u64>,
}

pub(crate) fn sample() -> Option<BTreeMap<u32, GpuMemorySample>> {
    let mut handle = ptr::null_mut();
    // SAFETY: PDH writes the newly opened local query handle to this valid output pointer.
    if unsafe { PdhOpenQueryW(ptr::null(), 0, &mut handle) } != 0 {
        return None;
    }
    let query = Query(handle);
    let dedicated = query.add(w!(r"\GPU Process Memory(*)\Dedicated Usage"));
    let shared = query.add(w!(r"\GPU Process Memory(*)\Shared Usage"));
    let adapter = dedicated.and_then(|_| query.add(w!(r"\GPU Adapter Memory(*)\Dedicated Usage")));
    // SAFETY: The owned query remains open and all added counters belong to it.
    if unsafe { PdhCollectQueryData(query.0) } != 0 {
        return None;
    }
    let adapter_usage = adapter
        .and_then(|counter| counter_values(counter, AdapterInstance::parse))
        .map_or_else(BTreeMap::new, adapter_totals);
    let dedicated = dedicated
        .and_then(|counter| counter_values(counter, ProcessInstance::parse))
        .map(|samples| process_totals(samples, &adapter_usage));
    let shared = shared
        .and_then(|counter| counter_values(counter, ProcessInstance::parse))
        .map(|samples| process_totals(samples, &BTreeMap::new()));
    if dedicated.is_none() && shared.is_none() {
        return None;
    }

    let mut samples = BTreeMap::<u32, GpuMemorySample>::new();
    for (pid, bytes) in dedicated.into_iter().flatten() {
        samples.entry(pid).or_default().dedicated_bytes = bytes;
    }
    for (pid, bytes) in shared.into_iter().flatten() {
        samples.entry(pid).or_default().shared_bytes = bytes;
    }
    Some(samples)
}

struct Query(PDH_HQUERY);

impl Query {
    fn add(&self, path: *const u16) -> Option<PDH_HCOUNTER> {
        let mut counter = ptr::null_mut();
        // SAFETY: This query stays open, its caller provides a static NUL-terminated path,
        // and the counter out-pointer is valid.
        (unsafe { PdhAddEnglishCounterW(self.0, path, 0, &mut counter) } == 0).then_some(counter)
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        // SAFETY: This object uniquely owns the open query handle and all its counters.
        unsafe { PdhCloseQuery(self.0) };
    }
}

fn counter_values<T>(
    counter: PDH_HCOUNTER,
    parse: fn(&str) -> Option<T>,
) -> Option<Vec<(T, Option<u64>)>> {
    // Defined in the Windows SDK's pdh.h and omitted by windows-sys.
    const PDH_FMT_NOSCALE: u32 = 0x1000;
    const PDH_FMT_NOCAP100: u32 = 0x8000;
    const FORMAT: u32 = PDH_FMT_LARGE | PDH_FMT_NOSCALE | PDH_FMT_NOCAP100;
    let mut bytes = 0;
    let mut count = 0;
    // SAFETY: The query still owns this counter, and a null buffer requests its required size.
    let status = unsafe {
        PdhGetFormattedCounterArrayW(counter, FORMAT, &mut bytes, &mut count, ptr::null_mut())
    };
    if status != PDH_MORE_DATA || bytes == 0 {
        return None;
    }

    const _: () = assert!(align_of::<u64>() >= align_of::<PDH_FMT_COUNTERVALUE_ITEM_W>());
    let mut storage = vec![0u64; (bytes as usize).div_ceil(size_of::<u64>())];
    // SAFETY: The owned query and counter remain live, and storage is aligned and has `bytes` bytes.
    let status = unsafe {
        PdhGetFormattedCounterArrayW(
            counter,
            FORMAT,
            &mut bytes,
            &mut count,
            storage.as_mut_ptr().cast(),
        )
    };
    let byte_count = bytes as usize;
    let item_count = count as usize;
    let array_bytes = item_count.checked_mul(size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>())?;
    if status != 0 || array_bytes > byte_count || byte_count > size_of_val(storage.as_slice()) {
        return None;
    }

    // SAFETY: PDH initialized this aligned item array, whose checked length is within storage.
    let items = unsafe {
        std::slice::from_raw_parts(
            storage.as_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(),
            item_count,
        )
    };
    let values = items.iter().filter_map(|item| {
        let offset = item.szName.addr().checked_sub(storage.as_ptr().addr())?;
        if offset < array_bytes || offset >= byte_count || !offset.is_multiple_of(size_of::<u16>())
        {
            return None;
        }
        // SAFETY: The aligned string range lies past the item array inside initialized storage.
        let name = unsafe {
            std::slice::from_raw_parts(
                storage.as_ptr().cast::<u8>().add(offset).cast::<u16>(),
                (byte_count - offset) / size_of::<u16>(),
            )
        };
        let name_end = name.iter().position(|unit| *unit == 0)?;
        let name = String::from_utf16(&name[..name_end]).ok()?;
        let instance = parse(&name)?;
        let value = if matches!(
            item.FmtValue.CStatus,
            PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA
        ) {
            // SAFETY: PDH_FMT_LARGE selects the `largeValue` union field.
            u64::try_from(unsafe { item.FmtValue.Anonymous.largeValue }).ok()
        } else {
            None
        };
        Some((instance, value))
    });
    Some(values.collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct AdapterInstance {
    luid: (u32, u32),
    physical_index: u32,
}

impl AdapterInstance {
    fn parse(name: &str) -> Option<Self> {
        let (luid, physical_index) = name.strip_prefix("luid_")?.split_once("_phys_")?;
        let (high, low) = luid.split_once('_')?;
        let physical_index = match physical_index.split_once('#') {
            Some((index, suffix)) if suffix.parse::<u32>().is_ok() => index,
            Some(_) => return None,
            None => physical_index,
        };
        Some(Self {
            luid: (
                u32::from_str_radix(high.strip_prefix("0x")?, 16).ok()?,
                u32::from_str_radix(low.strip_prefix("0x")?, 16).ok()?,
            ),
            physical_index: physical_index.parse().ok()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ProcessInstance {
    pid: u32,
    adapter: AdapterInstance,
}

impl ProcessInstance {
    fn parse(name: &str) -> Option<Self> {
        let (pid, adapter) = name.strip_prefix("pid_")?.split_once('_')?;
        Some(Self {
            pid: pid.parse().ok()?,
            adapter: AdapterInstance::parse(adapter)?,
        })
    }
}

fn adapter_totals(
    instances: impl IntoIterator<Item = (AdapterInstance, Option<u64>)>,
) -> BTreeMap<AdapterInstance, Option<u64>> {
    let mut totals = BTreeMap::new();
    for (adapter, bytes) in instances {
        totals
            .entry(adapter)
            .and_modify(|total| *total = None)
            .or_insert(bytes);
    }
    totals
}

fn process_totals(
    instances: impl IntoIterator<Item = (ProcessInstance, Option<u64>)>,
    adapter_usage: &BTreeMap<AdapterInstance, Option<u64>>,
) -> BTreeMap<u32, Option<u64>> {
    let mut totals = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for (instance, value) in instances {
        let total = totals.entry(instance.pid).or_insert(Some(0u64));
        // A duplicate cannot be attributed to a distinct GPU or reliably resolved by PID.
        *total = if seen.insert(instance) {
            let adapter_bytes = adapter_usage.get(&instance.adapter).copied().flatten();
            // A conflicting adapter sample makes the process's total unavailable.
            let value = value.filter(|bytes| adapter_bytes.is_none_or(|usage| *bytes <= usage));
            total
                .zip(value)
                .and_then(|(total, value)| total.checked_add(value))
        } else {
            None
        };
    }
    totals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_memory_groups_each_process_across_adapters_and_physical_nodes() {
        let instances = [
            ("pid_10_luid_0x00000000_0x00013E85_phys_0", Some(200)),
            ("pid_10_luid_0x00000000_0x00013E85_phys_1", Some(300)),
            ("pid_10_luid_0x00000000_0x00013A22_phys_0", Some(400)),
            ("pid_20_luid_0x00000000_0x00013A22_phys_0", Some(0)),
            ("pid_30_luid_0x00000000_0x00013E85_phys_0", Some(100)),
            ("pid_30_luid_0x00000000_0x00013A22_phys_0", None),
            ("pid_40_luid_0x00000000_0x00013E85_phys_0", Some(100)),
            ("pid_40_luid_0x00000000_0x00013E85_phys_0#1", Some(100)),
        ];
        assert_eq!(
            process_totals(
                instances
                    .into_iter()
                    .map(|(name, bytes)| (ProcessInstance::parse(name).unwrap(), bytes)),
                &BTreeMap::new(),
            ),
            BTreeMap::from([(10, Some(900)), (20, Some(0)), (30, None), (40, None)])
        );
    }

    #[test]
    fn gpu_memory_parses_counter_instances_and_duplicate_indices() {
        let adapter = "luid_0x00000000_0x00013E85_phys_0";
        let process = format!("pid_10_{adapter}");
        let instance = ProcessInstance::parse(&process).unwrap();
        assert_eq!(AdapterInstance::parse(adapter), Some(instance.adapter));
        assert_eq!(
            ProcessInstance::parse(&format!("{process}#1")),
            Some(instance)
        );
        assert!(ProcessInstance::parse(&format!("{process}_eng_0")).is_none());
    }

    #[test]
    fn dedicated_gpu_memory_rejects_conflicting_samples_per_adapter_node() {
        let adapter = |name| AdapterInstance::parse(name).unwrap();
        let process = |pid, adapter| ProcessInstance { pid, adapter };
        let discrete = adapter("luid_0x0_0x13e85_phys_0");
        let linked = adapter("luid_0x0_0x13e85_phys_1");
        let integrated = adapter("luid_0x0_0x13a22_phys_0");
        let invalid = adapter("luid_0x1_0x13e85_phys_0");
        let duplicate = adapter("luid_0x0_0x13e3e_phys_0");
        let missing = adapter("luid_0x0_0x13e3e_phys_1");
        let adapter_usage = adapter_totals([
            (discrete, Some(1_350_000_000)),
            (linked, Some(100)),
            (integrated, Some(0)),
            (invalid, None),
            (duplicate, Some(1)),
            (duplicate, Some(10)),
        ]);
        let instances = [
            (process(10, discrete), Some(7_600_000_000)),
            (process(11, discrete), Some(1_350_000_000)),
            (process(12, discrete), Some(1_000_000_000)),
            (process(12, linked), Some(100)),
            (process(13, discrete), Some(1)),
            (process(13, linked), Some(101)),
            (process(14, integrated), Some(0)),
            (process(15, integrated), Some(1)),
            (process(16, invalid), Some(10)),
            (process(17, duplicate), Some(10)),
            (process(18, missing), Some(10)),
        ];
        assert_eq!(
            process_totals(instances, &adapter_usage),
            BTreeMap::from([
                (10, None),
                (11, Some(1_350_000_000)),
                (12, Some(1_000_000_100)),
                (13, None),
                (14, Some(0)),
                (15, None),
                (16, Some(10)),
                (17, Some(10)),
                (18, Some(10)),
            ])
        );
    }

    #[test]
    #[ignore = "reads live Windows GPU Process Memory performance counters"]
    fn live_gpu_memory_query_returns_dedicated_and_shared_samples() {
        let started = std::time::Instant::now();
        let samples = sample().expect("GPU Process Memory counters are unavailable");
        let dedicated = samples
            .values()
            .filter(|sample| sample.dedicated_bytes.is_some())
            .count();
        let shared = samples
            .values()
            .filter(|sample| sample.shared_bytes.is_some())
            .count();
        eprintln!(
            "GPU memory collection took {:?}, {dedicated} dedicated and {shared} shared process samples",
            started.elapsed()
        );
        assert!(dedicated > 0 && shared > 0);
    }
}
