use std::time::Instant;

use crate::foreground::list_processes;
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::{
        SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
        Threading::{
            GetProcessIoCounters, OpenProcess, IO_COUNTERS, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryUsageSnapshot {
    pub percent: Option<f32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct IoUsageSnapshot {
    pub bytes_per_second: Option<f64>,
    pub read_bytes_per_second: Option<f64>,
    pub write_bytes_per_second: Option<f64>,
}

#[derive(Debug, Default)]
pub struct MemoryUsageMonitor;

#[derive(Debug, Default)]
pub struct IoUsageMonitor {
    previous: Option<IoCounterSample>,
}

#[derive(Debug, Clone, Copy)]
struct IoCounterSample {
    read_bytes: u64,
    write_bytes: u64,
    sampled_at: Instant,
}

impl MemoryUsageMonitor {
    pub fn sample(&mut self) -> MemoryUsageSnapshot {
        MemoryUsageSnapshot {
            percent: system_memory_load_percent().map(f32::from),
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

fn system_memory_load_percent() -> Option<u8> {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0).then_some(status.dwMemoryLoad.min(100) as u8)
}

fn read_system_io_counters() -> Option<IoCounterSample> {
    let mut read_bytes = 0u64;
    let mut write_bytes = 0u64;
    let mut sampled_any = false;

    for process in list_processes().ok()? {
        let Some(counters) = process_io_counters(process.id) else {
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

    let counters = process_io_counters_for_handle(process);
    unsafe {
        CloseHandle(process);
    }
    counters
}

fn process_io_counters_for_handle(process: HANDLE) -> Option<IO_COUNTERS> {
    let mut counters = IO_COUNTERS::default();
    let ok = unsafe { GetProcessIoCounters(process, &mut counters) };
    (ok != 0).then_some(counters)
}
