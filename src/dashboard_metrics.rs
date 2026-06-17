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

#[derive(Debug, Clone, Copy, Default)]
pub struct IoUsageSnapshot {
    pub bytes_per_second: Option<f64>,
}

#[derive(Debug, Default)]
pub struct MemoryUsageMonitor;

#[derive(Debug, Default)]
pub struct IoUsageMonitor {
    previous: Option<IoCounterSample>,
}

#[derive(Debug, Clone, Copy)]
struct IoCounterSample {
    bytes: u64,
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
            return IoUsageSnapshot {
                bytes_per_second: None,
            };
        };

        let bytes_per_second = self.previous.and_then(|previous| {
            let elapsed = current.sampled_at.duration_since(previous.sampled_at);
            let elapsed_seconds = elapsed.as_secs_f64();
            (elapsed_seconds > 0.0)
                .then(|| current.bytes.saturating_sub(previous.bytes) as f64 / elapsed_seconds)
        });

        self.previous = Some(current);
        IoUsageSnapshot { bytes_per_second }
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
    let mut bytes = 0u64;
    let mut sampled_any = false;

    for process in list_processes().ok()? {
        let Some(counters) = process_io_counters(process.id) else {
            continue;
        };
        bytes = bytes
            .saturating_add(counters.ReadTransferCount)
            .saturating_add(counters.WriteTransferCount);
        sampled_any = true;
    }

    sampled_any.then_some(IoCounterSample {
        bytes,
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
