use std::time::{SystemTime, UNIX_EPOCH};
use std::{collections::BTreeMap, ops::RangeInclusive, ptr::null, sync::mpsc::Sender};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use windows_sys::{
    core::GUID,
    Win32::{
        Foundation::ERROR_SUCCESS,
        System::{
            Diagnostics::Etw::{
                CloseTrace, ImageLoadGuid, OpenTraceW, PerfInfoGuid, ProcessTrace,
                EVENT_HEADER_FLAG_64_BIT_HEADER, EVENT_HEADER_FLAG_PROCESSOR_INDEX, EVENT_RECORD,
                EVENT_TRACE_LOGFILEW, PROCESSTRACE_HANDLE, PROCESS_TRACE_MODE_EVENT_RECORD,
                PROCESS_TRACE_MODE_RAW_TIMESTAMP, PROCESS_TRACE_MODE_REAL_TIME,
            },
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
        },
    },
};

use crate::win_util::{last_error, wide_null};

const MAX_INTERRUPTS: usize = 262_144;
const MAX_IMAGE_RECORDS: usize = 4_096;
const MAX_MODULE_ROWS: usize = 5;
const KERNEL_ADDRESS: u64 = 0xffff_0000_0000_0000;
pub(super) const CAPTURE_SECONDS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum InterruptKind {
    #[serde(rename = "DPC")]
    Dpc,
    #[serde(rename = "ISR")]
    Isr,
    #[serde(rename = "Threaded DPC")]
    ThreadedDpc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct InterruptTimingRow {
    pub(crate) kind: InterruptKind,
    pub(crate) module: Option<String>,
    pub(crate) count: usize,
    pub(crate) total_inclusive_ms: f64,
    pub(crate) max_inclusive_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct InterruptTimingCpuRow {
    pub(crate) cpu: u16,
    pub(crate) dpc_count: usize,
    pub(crate) isr_count: usize,
    pub(crate) dpc_ms: f64,
    pub(crate) isr_ms: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct InterruptTimingSecond {
    pub(crate) dpc_ms: f64,
    pub(crate) isr_ms: f64,
    pub(crate) threaded_dpc_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct InterruptTimingCaptureStart {
    utc_sample: String,
    qpc_sample_span_ns: u64,
}

pub(super) struct CaptureClockSample {
    pub(super) utc: SystemTime,
    pub(super) after_tick: i64,
}

impl CaptureClockSample {
    fn with_origin(self, start_tick: i64, frequency: i64) -> Option<InterruptTimingCaptureStart> {
        let span = u128::try_from(self.after_tick.checked_sub(start_tick)?).ok()?;
        let frequency = u128::try_from(frequency).ok().filter(|value| *value != 0)?;
        let qpc_sample_span_ns = u64::try_from((span * 1_000_000_000).div_ceil(frequency)).ok()?;
        let utc = self.utc.duration_since(UNIX_EPOCH).ok()?;
        let utc =
            DateTime::<Utc>::from_timestamp(utc.as_secs().try_into().ok()?, utc.subsec_nanos())?;
        Some(InterruptTimingCaptureStart {
            utc_sample: utc.to_rfc3339_opts(SecondsFormat::AutoSi, true),
            qpc_sample_span_ns,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct InterruptTimingReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) capture_start: Option<Box<InterruptTimingCaptureStart>>,
    pub(crate) events_lost: u32,
    pub(crate) buffers_lost: u32,
    pub(crate) invalid_intervals: usize,
    pub(crate) incomplete_intervals: usize,
    pub(crate) invalid_image_records: usize,
    pub(crate) unresolved_attribution: usize,
    pub(crate) modules: Vec<InterruptTimingRow>,
    pub(crate) per_cpu: Option<Vec<InterruptTimingCpuRow>>,
    pub(crate) timeline: [InterruptTimingSecond; CAPTURE_SECONDS],
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DecodeError {
    #[error("OpenTraceW failed with Win32 error {0}.")]
    Open(u32),
    #[error("ProcessTrace failed with Win32 error {0}.")]
    Read(u32),
    #[error("The Windows performance counter failed.")]
    Clock,
    #[error("The capture controller closed before tracing started.")]
    ControllerStopped,
    #[error("The capture reached the interrupt or image record limit.")]
    Capacity,
}

pub(super) fn decode_live(
    session_name: &str,
    ready: Sender<()>,
    capture_start: i64,
    clock: Option<CaptureClockSample>,
    cpu_count: Option<u16>,
) -> Result<InterruptTimingReport, DecodeError> {
    let mut frequency = 0;
    // SAFETY: The pointer refers to writable i64 storage and the logger uses QPC timestamps.
    if unsafe { QueryPerformanceFrequency(&mut frequency) } == 0 || frequency <= 0 {
        return Err(DecodeError::Clock);
    }
    let capture_end = frequency
        .checked_mul(CAPTURE_SECONDS as i64)
        .and_then(|ticks| capture_start.checked_add(ticks))
        .ok_or(DecodeError::Clock)?;
    let mut name = wide_null(session_name);
    let mut events = Events::default();
    let mut logfile = EVENT_TRACE_LOGFILEW {
        LoggerName: name.as_mut_ptr(),
        Context: (&raw mut events).cast(),
        ..EVENT_TRACE_LOGFILEW::default()
    };
    logfile.Anonymous1.ProcessTraceMode = PROCESS_TRACE_MODE_EVENT_RECORD
        | PROCESS_TRACE_MODE_RAW_TIMESTAMP
        | PROCESS_TRACE_MODE_REAL_TIME;
    logfile.Anonymous2.EventRecordCallback = Some(on_event);

    // SAFETY: The logfile contains a valid callback, context and NUL-terminated session name.
    let handle = unsafe { OpenTraceW(&mut logfile) };
    if handle.Value == u64::MAX {
        return Err(DecodeError::Open(last_error()));
    }
    let handle = TraceHandle(handle);
    ready.send(()).map_err(|_| DecodeError::ControllerStopped)?;
    // SAFETY: The handle, callback context and name remain live during synchronous processing.
    let status = unsafe { ProcessTrace(&handle.0, 1, null(), null()) };
    if status != ERROR_SUCCESS {
        return Err(DecodeError::Read(status));
    }
    let mut drain_end = 0;
    // SAFETY: The output points to writable i64 storage.
    if unsafe { QueryPerformanceCounter(&mut drain_end) } == 0 || drain_end < capture_start {
        return Err(DecodeError::Clock);
    }
    drop(handle);
    let mut report = events.report(capture_start..=capture_end, drain_end, frequency, cpu_count)?;
    report.capture_start = clock
        .and_then(|clock| clock.with_origin(capture_start, frequency))
        .map(Box::new);
    Ok(report)
}

struct TraceHandle(PROCESSTRACE_HANDLE);

impl Drop for TraceHandle {
    fn drop(&mut self) {
        // SAFETY: This owner closes its successful OpenTraceW handle once.
        unsafe { CloseTrace(self.0) };
    }
}

struct Interrupt {
    kind: InterruptKind,
    cpu: u16,
    entry: i64,
    end: i64,
    routine: u64,
}

enum RecordKind {
    Interrupt(InterruptKind),
    Image(ImageKind),
}

enum ImageKind {
    Initial,
    Load,
    Unload,
}

#[derive(PartialEq, Eq)]
struct Mapping {
    end: u64,
    name: Option<String>,
}

fn merge_mapping(current: &mut Option<Mapping>, evidence: Mapping) {
    match current {
        Some(existing) if *existing != evidence => {
            existing.end = existing.end.max(evidence.end);
            existing.name = None;
        }
        Some(_) => {}
        None => *current = Some(evidence),
    }
}

#[derive(Default)]
struct ImageHistory {
    initial: Option<Mapping>,
    changes: Vec<(i64, Option<Mapping>)>,
}

struct Lifetime {
    from: i64,
    until: Option<i64>,
    mapping: Mapping,
}

impl ImageHistory {
    fn lifetimes(mut self) -> Vec<Lifetime> {
        self.changes.sort_unstable_by_key(|(time, _)| *time);
        let mut changes = self.changes.into_iter().peekable();
        let mut lifetimes = Vec::new();
        let mut current = self.initial;
        let mut from = i64::MIN;
        while let Some((at, change)) = changes.next() {
            let previous_end = current.as_ref().map_or(0, |mapping| mapping.end);
            if let Some(mapping) = current.take() {
                lifetimes.push(Lifetime {
                    from,
                    until: Some(at),
                    mapping,
                });
            }
            let mut missing_mapping = change.is_none();
            current = change;
            while let Some((_, change)) = changes.next_if(|(time, _)| *time == at) {
                match change {
                    Some(load) => merge_mapping(&mut current, load),
                    None => missing_mapping = true,
                }
            }
            // A load at the same timestamp as an unload or unreadable range is ambiguous.
            if let Some(mapping) = current.as_mut().filter(|_| missing_mapping) {
                mapping.end = mapping.end.max(previous_end);
                mapping.name = None;
            }
            from = at;
        }
        if let Some(mapping) = current {
            lifetimes.push(Lifetime {
                from,
                until: None,
                mapping,
            });
        }
        lifetimes
    }
}

#[derive(Default)]
struct Events {
    interrupts: Vec<Interrupt>,
    images: BTreeMap<u64, ImageHistory>,
    image_records: usize,
    invalid_intervals: usize,
    invalid_image_records: usize,
    capacity_exceeded: bool,
}

#[derive(Default)]
struct Totals {
    count: usize,
    total_ticks: i128,
    max_ticks: i64,
}

impl Totals {
    fn add(&mut self, ticks: i64) {
        self.count += 1;
        self.total_ticks += i128::from(ticks);
        self.max_ticks = self.max_ticks.max(ticks);
    }

    fn row(
        self,
        kind: InterruptKind,
        module: Option<&str>,
        millis_per_tick: f64,
    ) -> InterruptTimingRow {
        InterruptTimingRow {
            kind,
            module: module.map(str::to_owned),
            count: self.count,
            total_inclusive_ms: self.total_ticks as f64 * millis_per_tick,
            max_inclusive_ms: self.max_ticks as f64 * millis_per_tick,
        }
    }
}

impl Events {
    fn add_image(&mut self, kind: ImageKind, at: i64, payload: &[u8]) -> Option<()> {
        let base = read_u64(payload, 0)?;
        if base < KERNEL_ADDRESS {
            return Some(());
        }
        if self.image_records == MAX_IMAGE_RECORDS {
            self.capacity_exceeded = true;
            return Some(());
        }
        self.image_records += 1;
        let history = self.images.entry(base).or_default();
        let initial = matches!(kind, ImageKind::Initial);
        if !initial && at <= 0 {
            return None;
        }
        if matches!(kind, ImageKind::Unload) {
            history.changes.push((at, None));
            return Some(());
        }
        let mapping = image_mapping(base, payload);
        let name_valid = mapping
            .as_ref()
            .is_some_and(|mapping| mapping.name.is_some());
        if initial {
            // DCStart describes the beginning of the trace even when the callback arrives later.
            merge_mapping(&mut history.initial, mapping?);
        } else {
            // Even an unreadable load ends the previously verified lifetime at this base.
            history.changes.push((at, mapping));
        }
        // Report unreadable ranges and names after recording the available evidence.
        name_valid.then_some(())
    }

    fn report(
        self,
        window: RangeInclusive<i64>,
        drain_end: i64,
        frequency: i64,
        cpu_count: Option<u16>,
    ) -> Result<InterruptTimingReport, DecodeError> {
        if self.capacity_exceeded {
            return Err(DecodeError::Capacity);
        }
        let images: BTreeMap<_, _> = self
            .images
            .into_iter()
            .map(|(base, history)| (base, history.lifetimes()))
            .collect();
        let max_image_size = images
            .iter()
            .flat_map(|(&base, lifetimes)| {
                lifetimes.iter().map(move |life| life.mapping.end - base)
            })
            .max()
            .unwrap_or(0);
        let mut modules = BTreeMap::<(InterruptKind, Option<&str>), Totals>::new();
        let mut cpu_totals = cpu_count.map(|count| {
            (0..count)
                .map(|_| (Totals::default(), Totals::default()))
                .collect::<Vec<_>>()
        });
        let mut invalid_intervals = self.invalid_intervals;
        let mut incomplete_intervals = 0;
        let mut unresolved_attribution = 0;
        let mut timeline = [[0_i128; 3]; CAPTURE_SECONDS];
        for event in self.interrupts {
            if event.entry < *window.start() {
                incomplete_intervals += usize::from(event.end >= *window.start());
                continue;
            }
            if event.end > drain_end {
                invalid_intervals += 1;
                continue;
            }
            if event.entry > *window.end() {
                continue;
            }
            if event.end > *window.end() {
                incomplete_intervals += 1;
                continue;
            }
            let module = module_name(&images, max_image_size, &event);
            unresolved_attribution += usize::from(module.is_none());
            let ticks = event.end - event.entry;
            modules.entry((event.kind, module)).or_default().add(ticks);
            if ticks > 0 {
                let start = event.entry - window.start();
                let end = event.end - window.start();
                let kind = match event.kind {
                    InterruptKind::Dpc => 0,
                    InterruptKind::Isr => 1,
                    InterruptKind::ThreadedDpc => 2,
                };
                for second in start / frequency..=(end - 1) / frequency {
                    let bucket_start = second * frequency;
                    let bucket_end = bucket_start + frequency;
                    let overlap = end.min(bucket_end) - start.max(bucket_start);
                    timeline[second as usize][kind] += i128::from(overlap);
                }
            }
            if let Some(cpus) = cpu_totals.as_mut().filter(|_| ticks > 0) {
                match (event.kind, cpus.get_mut(usize::from(event.cpu))) {
                    (InterruptKind::ThreadedDpc, _) => {}
                    (InterruptKind::Dpc, Some((dpc, _))) => dpc.add(ticks),
                    (InterruptKind::Isr, Some((_, isr))) => isr.add(ticks),
                    (_, None) => cpu_totals = None,
                }
            }
        }
        let millis_per_tick = 1_000.0 / frequency as f64;
        let mut modules: Vec<_> = modules
            .into_iter()
            .map(|((kind, module), totals)| totals.row(kind, module, millis_per_tick))
            .collect();
        modules.sort_by(|left, right| {
            right
                .total_inclusive_ms
                .total_cmp(&left.total_inclusive_ms)
                .then_with(|| right.max_inclusive_ms.total_cmp(&left.max_inclusive_ms))
        });
        modules.truncate(MAX_MODULE_ROWS);
        let per_cpu = cpu_totals.map(|totals| {
            (0..)
                .zip(totals)
                .map(|(cpu, (dpc, isr))| InterruptTimingCpuRow {
                    cpu,
                    dpc_count: dpc.count,
                    isr_count: isr.count,
                    dpc_ms: dpc.total_ticks as f64 * millis_per_tick,
                    isr_ms: isr.total_ticks as f64 * millis_per_tick,
                })
                .collect()
        });
        Ok(InterruptTimingReport {
            capture_start: None,
            events_lost: 0,
            buffers_lost: 0,
            invalid_intervals,
            incomplete_intervals,
            invalid_image_records: self.invalid_image_records,
            unresolved_attribution,
            modules,
            per_cpu,
            timeline: timeline.map(|[dpc, isr, threaded_dpc]| InterruptTimingSecond {
                dpc_ms: dpc as f64 * millis_per_tick,
                isr_ms: isr as f64 * millis_per_tick,
                threaded_dpc_ms: threaded_dpc as f64 * millis_per_tick,
            }),
        })
    }
}

fn module_name<'a>(
    images: &'a BTreeMap<u64, Vec<Lifetime>>,
    max_image_size: u64,
    event: &Interrupt,
) -> Option<&'a str> {
    let mut name = None;
    for (_, lifetimes) in images.range(event.routine.saturating_sub(max_image_size)..=event.routine)
    {
        let first =
            lifetimes.partition_point(|life| life.until.is_some_and(|until| until <= event.entry));
        for life in lifetimes[first..]
            .iter()
            .take_while(|life| life.from <= event.end)
        {
            if event.routine >= life.mapping.end {
                continue;
            }
            if name.is_some()
                || event.entry < life.from
                || life.until.is_some_and(|until| event.end >= until)
            {
                return None;
            }
            name = Some(life.mapping.name.as_deref()?);
        }
    }
    name
}

unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    // SAFETY: ProcessTrace supplies a valid record for the duration of the callback.
    let Some(record) = (unsafe { record.as_ref() }) else {
        return;
    };
    // SAFETY: This ProcessTrace call invokes the callback serially with the exclusive Events context.
    let Some(events) = (unsafe { record.UserContext.cast::<Events>().as_mut() }) else {
        return;
    };
    if events.capacity_exceeded {
        return;
    }
    let header = &record.EventHeader;
    let descriptor = header.EventDescriptor;
    let kind = if guid_matches(header.ProviderId, PerfInfoGuid) {
        RecordKind::Interrupt(match descriptor.Opcode {
            66 => InterruptKind::ThreadedDpc,
            // TraceEvent also recognizes opcode 50 as an ISR.
            50 | 67 => InterruptKind::Isr,
            68 | 69 => InterruptKind::Dpc,
            _ => return,
        })
    } else if guid_matches(header.ProviderId, ImageLoadGuid) {
        RecordKind::Image(match descriptor.Opcode {
            2 => ImageKind::Unload,
            3 => ImageKind::Initial,
            10 => ImageKind::Load,
            // DCEnd confirms residency at trace end and cannot date a load.
            _ => return,
        })
    } else {
        return;
    };
    let payload = if record.UserData.is_null() || record.UserDataLength == 0 {
        &[]
    } else {
        // SAFETY: ETW supplies at least UserDataLength bytes for this nonempty callback payload.
        unsafe {
            std::slice::from_raw_parts(record.UserData.cast(), usize::from(record.UserDataLength))
        }
    };
    let is_64_bit = u32::from(header.Flags) & EVENT_HEADER_FLAG_64_BIT_HEADER != 0;
    match kind {
        RecordKind::Interrupt(kind) => {
            let interrupt = (is_64_bit && descriptor.Version == 2)
                .then(|| interrupt(kind, logical_cpu(record), header.TimeStamp, payload))
                .flatten();
            if let Some(interrupt) = interrupt {
                if events.interrupts.len() == MAX_INTERRUPTS {
                    events.capacity_exceeded = true;
                } else {
                    events.interrupts.push(interrupt);
                }
            } else {
                events.invalid_intervals += 1;
            }
        }
        RecordKind::Image(kind) => {
            if !is_64_bit
                || !matches!(descriptor.Version, 2 | 3)
                || events.add_image(kind, header.TimeStamp, payload).is_none()
            {
                events.invalid_image_records += 1;
            }
        }
    }
}

fn guid_matches(left: GUID, right: GUID) -> bool {
    left.data1 == right.data1
        && left.data2 == right.data2
        && left.data3 == right.data3
        && left.data4 == right.data4
}

fn logical_cpu(record: &EVENT_RECORD) -> u16 {
    let context = record.BufferContext.Anonymous;
    if u32::from(record.EventHeader.Flags) & EVENT_HEADER_FLAG_PROCESSOR_INDEX != 0 {
        // SAFETY: ETW selects the 16-bit union member with this flag.
        unsafe { context.ProcessorIndex }
    } else {
        // SAFETY: the classic union member stores the CPU number and a separate alignment byte.
        let number = unsafe { context.Anonymous.ProcessorNumber };
        u16::from(number)
    }
}

fn interrupt(kind: InterruptKind, cpu: u16, end: i64, payload: &[u8]) -> Option<Interrupt> {
    // The x64 classic PerfInfo events start with the entry QPC and routine pointer.
    let entry = i64::try_from(read_u64(payload, 0)?).ok()?;
    (entry > 0 && end >= entry).then_some(Interrupt {
        kind,
        cpu,
        entry,
        end,
        routine: read_u64(payload, 8)?,
    })
}

fn image_mapping(base: u64, payload: &[u8]) -> Option<Mapping> {
    let size = read_u64(payload, 8).filter(|&size| size != 0)?;
    Some(Mapping {
        end: base.checked_add(size)?,
        name: image_basename(payload),
    })
}

fn image_basename(payload: &[u8]) -> Option<String> {
    // The x64 Image_Load filename starts at byte 56. Retain only its basename.
    let mut path = payload.get(56..)?.as_chunks::<2>().0.iter();
    let mut basename = Vec::new();
    loop {
        match u16::from_le_bytes(*path.next()?) {
            0 => break,
            0x2f | 0x5c => basename.clear(),
            unit => basename.push(unit),
        }
    }
    if basename.is_empty() || basename.len() > 255 {
        return None;
    }
    Some(String::from_utf16(&basename).ok()?.to_lowercase())
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Diagnostics::Etw::{
        ETW_BUFFER_CONTEXT_0, ETW_BUFFER_CONTEXT_0_0,
    };

    fn sample(routine: u64, entry: i64, end: i64) -> Interrupt {
        Interrupt {
            kind: InterruptKind::Dpc,
            cpu: 0,
            entry,
            end,
            routine,
        }
    }

    fn image(base: u64, size: u64, name: &str) -> Vec<u8> {
        let mut payload = vec![0; 56];
        payload[..8].copy_from_slice(&base.to_le_bytes());
        payload[8..16].copy_from_slice(&size.to_le_bytes());
        payload.extend(name.encode_utf16().chain([0]).flat_map(u16::to_le_bytes));
        payload
    }

    #[test]
    fn rejects_zero_entry_timestamps_and_excludes_out_of_window_samples() {
        assert!(interrupt(InterruptKind::Dpc, 0, 864_358_955_482, &[0; 16]).is_none());
        let events = Events {
            interrupts: vec![
                sample(0, 90, 95),
                sample(0, 99, 105),
                sample(0, 110, 112),
                sample(0, 115, 121),
                sample(0, 117, 120),
                sample(0, 121, 125),
                sample(0, 116, 127),
            ],
            ..Events::default()
        };
        let report = events.report(100..=120, 126, 1_000, Some(1)).unwrap();
        assert_eq!(report.invalid_intervals, 1);
        assert_eq!(report.incomplete_intervals, 2);
        assert_eq!(report.modules[0].count, 2);
        assert_eq!(report.modules[0].total_inclusive_ms, 5.0);
        assert_eq!(report.per_cpu.unwrap()[0].dpc_ms, 5.0);
    }

    #[test]
    fn timeline_splits_complete_intervals_at_capture_relative_second_boundaries() {
        let events = Events {
            interrupts: vec![
                sample(0, 1_123, 1_129),
                Interrupt {
                    kind: InterruptKind::Isr,
                    cpu: 1,
                    ..sample(0, 4_623, 5_125)
                },
                Interrupt {
                    kind: InterruptKind::ThreadedDpc,
                    ..sample(0, 2_125, 3_125)
                },
                sample(0, 120, 125),
                sample(0, 5_124, 5_126),
                sample(0, 5_125, 5_125),
            ],
            ..Events::default()
        };
        let report = events.report(125..=5_125, 5_130, 1_000, None).unwrap();
        assert_eq!(report.incomplete_intervals, 2);
        assert_eq!(
            report
                .timeline
                .map(|second| (second.dpc_ms, second.isr_ms, second.threaded_dpc_ms)),
            [
                (2.0, 0.0, 0.0),
                (4.0, 0.0, 0.0),
                (0.0, 0.0, 1_000.0),
                (0.0, 0.0, 0.0),
                (0.0, 502.0, 0.0),
            ]
        );
    }

    #[test]
    fn reads_cpu_label_from_the_field_selected_by_the_etw_header() {
        let mut record = EVENT_RECORD::default();
        record.BufferContext.Anonymous = ETW_BUFFER_CONTEXT_0 {
            Anonymous: ETW_BUFFER_CONTEXT_0_0 {
                ProcessorNumber: 7,
                Alignment: 8,
            },
        };
        assert_eq!(logical_cpu(&record), 7);

        record.EventHeader.Flags = EVENT_HEADER_FLAG_PROCESSOR_INDEX as u16;
        record.BufferContext.Anonymous = ETW_BUFFER_CONTEXT_0 {
            ProcessorIndex: 300,
        };
        assert_eq!(logical_cpu(&record), 300);
    }

    #[test]
    fn per_cpu_rows_include_idle_cpus_and_omit_threaded_and_zero_duration_samples() {
        let events = Events {
            interrupts: vec![
                sample(0, 101, 104),
                sample(0, 104, 104),
                Interrupt {
                    kind: InterruptKind::Isr,
                    cpu: 1,
                    ..sample(0, 110, 117)
                },
                Interrupt {
                    kind: InterruptKind::ThreadedDpc,
                    cpu: 1,
                    ..sample(0, 130, 132)
                },
            ],
            ..Events::default()
        };
        let report = events.report(100..=140, 140, 1_000, Some(3)).unwrap();
        assert_eq!(report.modules.iter().map(|row| row.count).sum::<usize>(), 4);
        let rows = report.per_cpu.unwrap();
        assert_eq!(
            rows.iter().map(|row| row.cpu).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!((rows[0].dpc_count, rows[0].dpc_ms), (1, 3.0));
        assert_eq!((rows[1].isr_count, rows[1].isr_ms), (1, 7.0));
        assert_eq!(rows[1].dpc_count, 0);
        assert_eq!(rows[2].dpc_ms + rows[2].isr_ms, 0.0);
    }

    #[test]
    fn unverified_cpu_label_suppresses_only_the_per_cpu_breakdown() {
        let events = Events {
            interrupts: vec![
                sample(0, 101, 104),
                Interrupt {
                    cpu: 1,
                    ..sample(0, 108, 110)
                },
            ],
            ..Events::default()
        };
        let report = events.report(100..=120, 120, 1_000, Some(1)).unwrap();
        assert!(report.per_cpu.is_none());
        assert_eq!(report.modules[0].count, 2);
        assert_eq!(report.modules[0].total_inclusive_ms, 5.0);
    }

    #[test]
    fn resolves_address_reuse_after_out_of_order_rundown_and_changes() {
        let base = KERNEL_ADDRESS;
        let old = image(base, 128, r"\SystemRoot\OLD.SYS");
        let new = image(base, 128, "new.sys");
        let mut events = Events {
            interrupts: vec![
                sample(base, 150, 199),
                sample(base, 199, 200),
                sample(base, 200, 299),
                sample(base, 300, 309),
            ],
            ..Events::default()
        };
        events.add_image(ImageKind::Load, 300, &new).unwrap();
        events.add_image(ImageKind::Unload, 200, &old).unwrap();
        events.add_image(ImageKind::Initial, 350, &old).unwrap();

        let report = events.report(100..=400, 400, 1_000, None).unwrap();
        let count = |name: &str| {
            report
                .modules
                .iter()
                .find(|row| row.module.as_deref() == Some(name))
                .unwrap()
                .count
        };
        assert_eq!(count("old.sys"), 1);
        assert_eq!(count("new.sys"), 1);
        assert_eq!(report.unresolved_attribution, 2);
    }

    #[test]
    fn unreadable_image_load_does_not_reuse_the_old_driver_name() {
        let base = KERNEL_ADDRESS;
        let old = image(base, 128, "old.sys");
        let mut unreadable = image(base, 128, "x");
        unreadable[56..58].copy_from_slice(&0xd800_u16.to_le_bytes());
        let mut events = Events {
            interrupts: vec![sample(base + 8, 130, 134), sample(base + 8, 220, 225)],
            ..Events::default()
        };
        events.add_image(ImageKind::Initial, 110, &old).unwrap();
        assert!(events
            .add_image(ImageKind::Load, 200, &unreadable)
            .is_none());

        let report = events.report(100..=300, 300, 1_000, None).unwrap();
        assert_eq!(report.unresolved_attribution, 1);
        assert_eq!(report.modules.len(), 2);
        assert_eq!(report.modules[0].module, None);
        assert_eq!(report.modules[0].total_inclusive_ms, 5.0);
        assert_eq!(report.modules[1].module.as_deref(), Some("old.sys"));
        assert_eq!(report.modules[1].total_inclusive_ms, 4.0);
    }

    #[test]
    fn malformed_image_range_ends_the_previous_mapping() {
        let base = KERNEL_ADDRESS;
        let old = image(base, 128, "old.sys");
        let new = image(base, 128, "new.sys");
        let disjoint = image(base + 256, 128, "disjoint.sys");
        let mut events = Events {
            interrupts: vec![
                sample(base + 8, 130, 134),
                sample(base + 8, 195, 205),
                sample(base + 8, 220, 225),
                sample(base + 8, 250, 260),
                sample(base + 256, 220, 223),
            ],
            ..Events::default()
        };
        events.add_image(ImageKind::Initial, 110, &old).unwrap();
        events
            .add_image(ImageKind::Initial, 110, &disjoint)
            .unwrap();
        assert!(events.add_image(ImageKind::Load, 200, &new[..8]).is_none());
        events.add_image(ImageKind::Load, 240, &new).unwrap();

        let report = events.report(100..=300, 300, 1_000, None).unwrap();
        assert_eq!(report.unresolved_attribution, 2);
        let modules = report
            .modules
            .iter()
            .map(|row| (row.module.as_deref(), row.count, row.total_inclusive_ms))
            .collect::<Vec<_>>();
        assert_eq!(
            modules,
            [
                (None, 2, 15.0),
                (Some("new.sys"), 1, 10.0),
                (Some("old.sys"), 1, 4.0),
                (Some("disjoint.sys"), 1, 3.0),
            ]
        );
    }

    #[test]
    fn overlapping_and_simultaneously_changed_mappings_remain_unresolved() {
        let base = KERNEL_ADDRESS;
        let old = image(base, 128, "old.sys");
        let overlapping = image(base + 64, 128, "new.sys");
        let mut events = Events {
            interrupts: vec![
                sample(base + 64, 200, 220),
                sample(base + 64, 300, 310),
                sample(base + 64, 400, 410),
            ],
            ..Events::default()
        };
        events.add_image(ImageKind::Initial, 105, &old).unwrap();
        events
            .add_image(ImageKind::Load, 200, &overlapping)
            .unwrap();
        events.add_image(ImageKind::Unload, 300, &old).unwrap();
        events.add_image(ImageKind::Load, 300, &old).unwrap();
        events.add_image(ImageKind::Unload, 400, &old).unwrap();

        let report = events.report(100..=500, 500, 1_000, None).unwrap();
        assert_eq!(report.unresolved_attribution, 2);
        let row = report
            .modules
            .iter()
            .find(|row| row.module.is_some())
            .unwrap();
        assert_eq!(row.module.as_deref(), Some("new.sys"));
        assert_eq!(row.count, 1);
    }
}
