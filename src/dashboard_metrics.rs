use std::time::Instant;

use crate::{foreground::list_process_ids, win_util::WinHandle};
use windows_sys::Win32::{
    Foundation::HANDLE,
    NetworkManagement::{
        IpHelper::{FreeMibTable, GetIfTable2, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_TABLE2},
        Ndis::{IfOperStatusUp, MediaConnectStateConnected},
    },
    System::{
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
pub struct MemoryUsageMonitor;

#[derive(Debug, Default)]
pub struct IoUsageMonitor {
    previous: Option<IoCounterSample>,
}

#[derive(Debug, Default)]
pub struct NetworkUsageMonitor {
    previous: Option<NetworkCounterSample>,
}

#[derive(Debug, Clone, Copy)]
struct IoCounterSample {
    read_bytes: u64,
    write_bytes: u64,
    sampled_at: Instant,
}

#[derive(Debug, Clone, Copy)]
struct NetworkCounterSample {
    download_bytes: u64,
    upload_bytes: u64,
    sampled_at: Instant,
}

impl MemoryUsageMonitor {
    pub fn sample(&mut self) -> MemoryUsageSnapshot {
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
}

impl IoUsageMonitor {
    pub fn sample(&mut self) -> IoUsageSnapshot {
        let Some(current) = read_system_io_counters() else {
            return IoUsageSnapshot::default();
        };

        let (read_bytes_per_second, write_bytes_per_second) =
            self.previous.map_or((None, None), |previous| {
                let elapsed = current.sampled_at.duration_since(previous.sampled_at);
                let elapsed_seconds = elapsed.as_secs_f64();
                if elapsed_seconds > 0.0 {
                    (
                        Some(
                            current.read_bytes.saturating_sub(previous.read_bytes) as f64
                                / elapsed_seconds,
                        ),
                        Some(
                            current.write_bytes.saturating_sub(previous.write_bytes) as f64
                                / elapsed_seconds,
                        ),
                    )
                } else {
                    (None, None)
                }
            });
        let bytes_per_second = match (read_bytes_per_second, write_bytes_per_second) {
            (Some(read), Some(write)) => Some(read + write),
            _ => None,
        };

        self.previous = Some(current);
        IoUsageSnapshot {
            bytes_per_second,
            read_bytes_per_second,
            write_bytes_per_second,
        }
    }
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
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0).then_some(status)
}

fn system_cache_bytes() -> Option<u64> {
    let mut info = PERFORMANCE_INFORMATION {
        cb: std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32,
        ..Default::default()
    };
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
    let mut read_bytes = 0u64;
    let mut write_bytes = 0u64;
    let mut sampled_any = false;

    for process_id in list_process_ids().ok()? {
        let Some(counters) = process_io_counters(process_id) else {
            continue;
        };
        read_bytes = read_bytes.saturating_add(counters.ReadTransferCount);
        write_bytes = write_bytes.saturating_add(counters.WriteTransferCount);
        sampled_any = true;
    }

    sampled_any.then_some(IoCounterSample {
        read_bytes,
        write_bytes,
        sampled_at: Instant::now(),
    })
}

fn process_io_counters(process_id: u32) -> Option<IO_COUNTERS> {
    if process_id == 0 {
        return None;
    }

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return None;
    }

    let process = WinHandle::new(process);
    process_io_counters_for_handle(process.raw())
}

fn process_io_counters_for_handle(process: HANDLE) -> Option<IO_COUNTERS> {
    let mut counters = IO_COUNTERS::default();
    let ok = unsafe { GetProcessIoCounters(process, &mut counters) };
    (ok != 0).then_some(counters)
}

fn read_system_network_counters() -> Option<NetworkCounterSample> {
    let mut table = std::ptr::null_mut::<MIB_IF_TABLE2>();
    let result = unsafe { GetIfTable2(&mut table) };
    if result != 0 || table.is_null() {
        return None;
    }

    let counters = network_counters_from_table(table);
    unsafe {
        FreeMibTable(table.cast());
    }
    counters
}

fn network_counters_from_table(table: *const MIB_IF_TABLE2) -> Option<NetworkCounterSample> {
    let table = unsafe { &*table };
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
    fn cache_bytes_from_pages_uses_page_size() {
        assert_eq!(cache_bytes_from_pages(4, 4096), 16_384);
    }
}
