use std::{collections::BTreeMap, time::Instant};

use crate::{foreground::for_each_process_id, win_util::WinHandle};
use windows_sys::Win32::{
    NetworkManagement::{
        IpHelper::{FreeMibTable, GetIfTable2, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_TABLE2},
        Ndis::{IfOperStatusUp, MediaConnectStateConnected},
    },
    System::{
        Performance::{
            PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
            PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE,
            PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
        },
        ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION},
        SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
        Threading::{
            GetProcessIoCounters, OpenProcess, IO_COUNTERS, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MemoryUsageSnapshot {
    pub percent: Option<f32>,
    pub used_physical_bytes: Option<u64>,
    pub total_physical_bytes: Option<u64>,
    pub cached_physical_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct IoUsageSnapshot {
    pub bytes_per_second: Option<f64>,
    pub read_bytes_per_second: Option<f64>,
    pub write_bytes_per_second: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct NetworkUsageSnapshot {
    pub bytes_per_second: Option<f64>,
    pub download_bytes_per_second: Option<f64>,
    pub upload_bytes_per_second: Option<f64>,
}

#[derive(Debug, Default)]
pub struct IoUsageMonitor {
    previous: Option<IoCounterSample>,
}

#[derive(Debug, Default)]
pub struct NetworkUsageMonitor {
    previous: Option<NetworkCounterSample>,
}

#[derive(Debug)]
struct IoCounterSample {
    processes: BTreeMap<(u32, u64), (u64, u64)>,
    sampled_at: Instant,
}

#[derive(Debug, Clone, Copy)]
struct NetworkCounterSample {
    download_bytes: u64,
    upload_bytes: u64,
    sampled_at: Instant,
}

pub fn sample_memory_usage() -> MemoryUsageSnapshot {
    let Some(status) = system_memory_status() else {
        return MemoryUsageSnapshot::default();
    };
    let total_physical_bytes = status.ullTotalPhys;
    let available_physical_bytes = status.ullAvailPhys;
    let used_physical_bytes = total_physical_bytes.saturating_sub(available_physical_bytes);

    MemoryUsageSnapshot {
        percent: Some(status.dwMemoryLoad.min(100) as f32),
        used_physical_bytes: Some(used_physical_bytes),
        total_physical_bytes: Some(total_physical_bytes),
        cached_physical_bytes: system_cache_bytes(),
    }
}

impl IoUsageMonitor {
    pub fn sample(&mut self) -> IoUsageSnapshot {
        self.update(read_system_io_counters())
    }

    fn update(&mut self, current: Option<IoCounterSample>) -> IoUsageSnapshot {
        let previous = std::mem::replace(&mut self.previous, current);
        let (Some(previous), Some(current)) = (previous, self.previous.as_ref()) else {
            return IoUsageSnapshot::default();
        };
        let elapsed = current
            .sampled_at
            .duration_since(previous.sampled_at)
            .as_secs_f64();
        if elapsed <= 0.0 {
            return IoUsageSnapshot::default();
        }
        let mut totals = None;
        for (identity, &(read, write)) in &current.processes {
            let Some(&(old_read, old_write)) = previous.processes.get(identity) else {
                continue;
            };
            let (Some(read), Some(write)) =
                (read.checked_sub(old_read), write.checked_sub(old_write))
            else {
                continue;
            };
            let (total_read, total_write) = totals.get_or_insert((0u64, 0u64));
            *total_read = total_read.saturating_add(read);
            *total_write = total_write.saturating_add(write);
        }
        let Some((read, write)) = totals else {
            return IoUsageSnapshot::default();
        };
        let read = read as f64 / elapsed;
        let write = write as f64 / elapsed;
        IoUsageSnapshot {
            bytes_per_second: Some(read + write),
            read_bytes_per_second: Some(read),
            write_bytes_per_second: Some(write),
        }
    }
}

#[derive(Debug, Default)]
pub struct DiskUsageMonitor {
    counters: Option<DiskCounters>,
}

#[derive(Debug)]
struct DiskCounters {
    query: PDH_HQUERY,
    read: PDH_HCOUNTER,
    write: PDH_HCOUNTER,
}

impl DiskUsageMonitor {
    pub fn sample(&mut self) -> IoUsageSnapshot {
        if self.counters.is_none() {
            self.counters = DiskCounters::open();
            return IoUsageSnapshot::default();
        }
        let Some(counters) = self.counters.as_ref() else {
            return IoUsageSnapshot::default();
        };
        // SAFETY: The query is owned by counters and remains live throughout collection.
        if unsafe { PdhCollectQueryData(counters.query) } != 0 {
            self.counters = None;
            return IoUsageSnapshot::default();
        }
        let read = disk_counter_value(counters.read);
        let write = disk_counter_value(counters.write);
        IoUsageSnapshot {
            read_bytes_per_second: read,
            write_bytes_per_second: write,
            bytes_per_second: read.zip(write).map(|(read, write)| read + write),
        }
    }
}

impl DiskCounters {
    fn open() -> Option<Self> {
        let mut query = std::ptr::null_mut();
        // SAFETY: query is writable and a null source selects live local counters.
        if unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut query) } != 0 || query.is_null() {
            return None;
        }
        let mut counters = Self {
            query,
            read: std::ptr::null_mut(),
            write: std::ptr::null_mut(),
        };
        for (path, counter) in [
            (
                r"\PhysicalDisk(_Total)\Disk Read Bytes/sec",
                &mut counters.read,
            ),
            (
                r"\PhysicalDisk(_Total)\Disk Write Bytes/sec",
                &mut counters.write,
            ),
        ] {
            let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            // SAFETY: query is live, path is terminated UTF-16, and counter is writable.
            if unsafe { PdhAddEnglishCounterW(query, path.as_ptr(), 0, counter) } != 0
                || counter.is_null()
            {
                return None;
            }
        }
        // SAFETY: query is live; this first collection establishes the rate baseline.
        if unsafe { PdhCollectQueryData(query) } != 0 {
            return None;
        }
        Some(counters)
    }
}

impl Drop for DiskCounters {
    fn drop(&mut self) {
        // SAFETY: This wrapper owns query and closes it exactly once, including partial setup.
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

fn disk_counter_value(counter: PDH_HCOUNTER) -> Option<f64> {
    let mut value = PDH_FMT_COUNTERVALUE::default();
    // SAFETY: counter belongs to a live query and value is writable; the type output is optional.
    let status = unsafe {
        PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, std::ptr::null_mut(), &mut value)
    };
    if status != 0 || !matches!(value.CStatus, PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA) {
        return None;
    }
    // SAFETY: PDH_FMT_DOUBLE initializes the doubleValue member on a successful valid result.
    let rate = unsafe { value.Anonymous.doubleValue };
    (rate.is_finite() && rate >= 0.0).then_some(rate)
}

impl NetworkUsageMonitor {
    pub fn sample(&mut self) -> NetworkUsageSnapshot {
        let Some(current) = read_system_network_counters() else {
            return NetworkUsageSnapshot::default();
        };

        let (download_bytes_per_second, upload_bytes_per_second) =
            self.previous.map_or((None, None), |previous| {
                let elapsed = current.sampled_at.duration_since(previous.sampled_at);
                let elapsed_seconds = elapsed.as_secs_f64();
                if elapsed_seconds > 0.0 {
                    (
                        Some(
                            current
                                .download_bytes
                                .saturating_sub(previous.download_bytes)
                                as f64
                                / elapsed_seconds,
                        ),
                        Some(
                            current.upload_bytes.saturating_sub(previous.upload_bytes) as f64
                                / elapsed_seconds,
                        ),
                    )
                } else {
                    (None, None)
                }
            });
        let bytes_per_second = match (download_bytes_per_second, upload_bytes_per_second) {
            (Some(download), Some(upload)) => Some(download + upload),
            _ => None,
        };

        self.previous = Some(current);
        NetworkUsageSnapshot {
            bytes_per_second,
            download_bytes_per_second,
            upload_bytes_per_second,
        }
    }
}

fn system_memory_status() -> Option<MEMORYSTATUSEX> {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: status declares its size and remains writable for the call.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0).then_some(status)
}

fn system_cache_bytes() -> Option<u64> {
    let mut info = PERFORMANCE_INFORMATION {
        cb: std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32,
        ..Default::default()
    };
    // SAFETY: info is writable and info.cb is exactly its initialized structure size.
    let ok = unsafe { GetPerformanceInfo(&mut info, info.cb) };
    if ok == 0 {
        return None;
    }

    Some(cache_bytes_from_pages(info.SystemCache, info.PageSize))
}

fn cache_bytes_from_pages(page_count: usize, page_size: usize) -> u64 {
    (page_count as u64).saturating_mul(page_size as u64)
}

fn read_system_io_counters() -> Option<IoCounterSample> {
    let mut processes = BTreeMap::new();
    for_each_process_id(|process_id| {
        if let Some((creation_time, counters)) = process_io_counters(process_id) {
            processes.insert(
                (process_id, creation_time),
                (counters.ReadTransferCount, counters.WriteTransferCount),
            );
        }
    })
    .ok()?;
    (!processes.is_empty()).then_some(IoCounterSample {
        processes,
        sampled_at: Instant::now(),
    })
}

fn process_io_counters(process_id: u32) -> Option<(u64, IO_COUNTERS)> {
    if process_id == 0 {
        return None;
    }

    // SAFETY: process_id came from the system snapshot and no inherited handle is requested.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return None;
    }

    let process = WinHandle::new(process);
    let creation_time = process.process_creation_time()?;
    let mut counters = IO_COUNTERS::default();
    // SAFETY: process remains open through its owned handle and counters is writable for the call.
    let ok = unsafe { GetProcessIoCounters(process.raw(), &mut counters) };
    (ok != 0).then_some((creation_time, counters))
}

fn read_system_network_counters() -> Option<NetworkCounterSample> {
    let mut table = std::ptr::null_mut::<MIB_IF_TABLE2>();
    // SAFETY: table is a writable out-pointer that is either left null or populated with a
    // Windows-owned allocation released below with FreeMibTable.
    let result = unsafe { GetIfTable2(&mut table) };
    if result != 0 || table.is_null() {
        return None;
    }

    let counters = network_counters_from_table(table);
    // SAFETY: table was allocated successfully by GetIfTable2 and is released exactly once.
    unsafe {
        FreeMibTable(table.cast());
    }
    counters
}

fn network_counters_from_table(table: *const MIB_IF_TABLE2) -> Option<NetworkCounterSample> {
    // SAFETY: The only caller passes a non-null GetIfTable2 allocation that remains live until
    // this function returns.
    let table = unsafe { &*table };
    // SAFETY: GetIfTable2 allocated NumEntries contiguous table rows.
    let rows =
        unsafe { std::slice::from_raw_parts(table.Table.as_ptr(), table.NumEntries as usize) };
    let mut download_bytes = 0u64;
    let mut upload_bytes = 0u64;
    let mut sampled_any = false;

    for row in rows {
        if row.Type == IF_TYPE_SOFTWARE_LOOPBACK
            || row.OperStatus != IfOperStatusUp
            || row.MediaConnectState != MediaConnectStateConnected
        {
            continue;
        }

        download_bytes = download_bytes.saturating_add(row.InOctets);
        upload_bytes = upload_bytes.saturating_add(row.OutOctets);
        sampled_any = true;
    }

    sampled_any.then_some(NetworkCounterSample {
        download_bytes,
        upload_bytes,
        sampled_at: Instant::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires Windows PhysicalDisk performance counters"]
    fn disk_monitor_collects_live_rates() {
        let mut monitor = DiskUsageMonitor::default();
        assert_eq!(monitor.sample(), IoUsageSnapshot::default());
        assert!(monitor.counters.is_some());
        std::thread::sleep(std::time::Duration::from_secs(1));
        let sample = monitor.sample();
        let read = sample.read_bytes_per_second.expect("valid disk read rate");
        let write = sample
            .write_bytes_per_second
            .expect("valid disk write rate");
        assert!(read.is_finite() && read >= 0.0);
        assert!(write.is_finite() && write >= 0.0);
        assert_eq!(sample.bytes_per_second, Some(read + write));
    }

    #[test]
    fn io_rates_ignore_process_churn_and_reset_after_sampling_failure() {
        let now = Instant::now();
        type ProcessCounters = ((u32, u64), (u64, u64));
        let sample = |seconds, processes: &[ProcessCounters]| {
            Some(IoCounterSample {
                processes: processes.iter().copied().collect(),
                sampled_at: now + std::time::Duration::from_secs(seconds),
            })
        };
        let mut monitor = IoUsageMonitor::default();
        assert_eq!(
            monitor.update(sample(0, &[((1, 10), (100, 200)), ((2, 20), (9000, 9000))])),
            IoUsageSnapshot::default()
        );
        // Process 2 exited; its reused PID and a newly accessible process must establish baselines.
        let rates = monitor.update(sample(
            2,
            &[
                ((1, 10), (300, 600)),
                ((2, 30), (50000, 50000)),
                ((3, 40), (80000, 80000)),
            ],
        ));
        assert_eq!(rates.read_bytes_per_second, Some(100.0));
        assert_eq!(rates.write_bytes_per_second, Some(200.0));
        assert_eq!(rates.bytes_per_second, Some(300.0));
        assert_eq!(monitor.update(None), IoUsageSnapshot::default());
        assert_eq!(
            monitor.update(sample(4, &[((1, 10), (900, 900))])),
            IoUsageSnapshot::default()
        );
        assert_eq!(
            monitor.update(sample(5, &[((1, 10), (1, 1))])),
            IoUsageSnapshot::default()
        );
        assert_eq!(
            monitor
                .update(sample(6, &[((1, 10), (1, 1))]))
                .bytes_per_second,
            Some(0.0)
        );
    }
}
