use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
    time::Instant,
};

use serde::Serialize;

use super::windows_events::WindowsAutomationEvent;

const OUTPUT_PATH_ENV: &str = "WINDERUST_ARCHITECTURE_DIAGNOSTICS_PATH";

struct DiagnosticRun {
    output_path: Option<PathBuf>,
    started_at: Instant,
    started_at_utc: String,
}

#[derive(Serialize)]
struct ArchitectureDiagnostics<'a> {
    schema_version: u8,
    purpose: &'a str,
    winderust_version: &'a str,
    started_at_utc: &'a str,
    finished_at_utc: String,
    duration_ms: u64,
    worker: WorkerDiagnostics,
    inventory: InventoryDiagnostics,
    accepted_events: EventDiagnostics,
    process_priority: ProcessPriorityDiagnostics,
}

#[derive(Serialize)]
struct WorkerDiagnostics {
    reconciliation_passes: u64,
    signal_wakes: u64,
    timeout_wakes: u64,
    total_wakes: u64,
    wake_frequency_hz: f64,
}

#[derive(Serialize)]
struct InventoryDiagnostics {
    process_snapshot_scans: u64,
    process_path_enrichment_scans: u64,
    foreground_process_queries: u64,
    visible_window_scans: u64,
    top_level_window_scans: u64,
}

#[derive(Serialize)]
struct EventDiagnostics {
    foreground_changed: u64,
    window_created: u64,
    power_changed: u64,
    session_changed: u64,
    appearance_changed: u64,
    input_notifications: u64,
}

#[derive(Serialize)]
struct ProcessPriorityDiagnostics {
    cycles: u64,
    scanned_process_observations: u64,
    selected_target_observations: u64,
    applied_changes: u64,
    already_applied_observations: u64,
    preserved_observations: u64,
    process_exit_failures: u64,
    access_denied_failures: u64,
    other_failures: u64,
}

static RUN: OnceLock<DiagnosticRun> = OnceLock::new();
static WORKER_PASSES: AtomicU64 = AtomicU64::new(0);
static SIGNAL_WAKES: AtomicU64 = AtomicU64::new(0);
static TIMEOUT_WAKES: AtomicU64 = AtomicU64::new(0);
static PROCESS_SNAPSHOT_SCANS: AtomicU64 = AtomicU64::new(0);
static PROCESS_PATH_ENRICHMENT_SCANS: AtomicU64 = AtomicU64::new(0);
static FOREGROUND_PROCESS_QUERIES: AtomicU64 = AtomicU64::new(0);
static VISIBLE_WINDOW_SCANS: AtomicU64 = AtomicU64::new(0);
static TOP_LEVEL_WINDOW_SCANS: AtomicU64 = AtomicU64::new(0);
static FOREGROUND_CHANGED_EVENTS: AtomicU64 = AtomicU64::new(0);
static WINDOW_CREATED_EVENTS: AtomicU64 = AtomicU64::new(0);
static POWER_CHANGED_EVENTS: AtomicU64 = AtomicU64::new(0);
static SESSION_CHANGED_EVENTS: AtomicU64 = AtomicU64::new(0);
static APPEARANCE_CHANGED_EVENTS: AtomicU64 = AtomicU64::new(0);
static INPUT_NOTIFICATIONS: AtomicU64 = AtomicU64::new(0);
static PROCESS_PRIORITY_CYCLES: AtomicU64 = AtomicU64::new(0);
static PROCESS_PRIORITY_SCANNED: AtomicU64 = AtomicU64::new(0);
static PROCESS_PRIORITY_SELECTED: AtomicU64 = AtomicU64::new(0);
static PROCESS_PRIORITY_APPLIED: AtomicU64 = AtomicU64::new(0);
static PROCESS_PRIORITY_ALREADY_APPLIED: AtomicU64 = AtomicU64::new(0);
static PROCESS_PRIORITY_PRESERVED: AtomicU64 = AtomicU64::new(0);
static PROCESS_PRIORITY_EXIT_FAILURES: AtomicU64 = AtomicU64::new(0);
static PROCESS_PRIORITY_ACCESS_FAILURES: AtomicU64 = AtomicU64::new(0);
static PROCESS_PRIORITY_OTHER_FAILURES: AtomicU64 = AtomicU64::new(0);

pub fn initialize() {
    let _ = run();
}

pub fn finish() -> Result<(), String> {
    let run = run();
    let Some(output_path) = run.output_path.as_deref() else {
        return Ok(());
    };

    let duration = run.started_at.elapsed();
    let signal_wakes = SIGNAL_WAKES.load(Ordering::Relaxed);
    let timeout_wakes = TIMEOUT_WAKES.load(Ordering::Relaxed);
    let total_wakes = signal_wakes.saturating_add(timeout_wakes);
    let duration_seconds = duration.as_secs_f64();
    let report = ArchitectureDiagnostics {
        schema_version: 1,
        purpose: "Phase 0 behavior-preserving architecture baseline diagnostics",
        winderust_version: env!("CARGO_PKG_VERSION"),
        started_at_utc: &run.started_at_utc,
        finished_at_utc: chrono::Utc::now().to_rfc3339(),
        duration_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
        worker: WorkerDiagnostics {
            reconciliation_passes: WORKER_PASSES.load(Ordering::Relaxed),
            signal_wakes,
            timeout_wakes,
            total_wakes,
            wake_frequency_hz: if duration_seconds > 0.0 {
                total_wakes as f64 / duration_seconds
            } else {
                0.0
            },
        },
        inventory: InventoryDiagnostics {
            process_snapshot_scans: PROCESS_SNAPSHOT_SCANS.load(Ordering::Relaxed),
            process_path_enrichment_scans: PROCESS_PATH_ENRICHMENT_SCANS.load(Ordering::Relaxed),
            foreground_process_queries: FOREGROUND_PROCESS_QUERIES.load(Ordering::Relaxed),
            visible_window_scans: VISIBLE_WINDOW_SCANS.load(Ordering::Relaxed),
            top_level_window_scans: TOP_LEVEL_WINDOW_SCANS.load(Ordering::Relaxed),
        },
        accepted_events: EventDiagnostics {
            foreground_changed: FOREGROUND_CHANGED_EVENTS.load(Ordering::Relaxed),
            window_created: WINDOW_CREATED_EVENTS.load(Ordering::Relaxed),
            power_changed: POWER_CHANGED_EVENTS.load(Ordering::Relaxed),
            session_changed: SESSION_CHANGED_EVENTS.load(Ordering::Relaxed),
            appearance_changed: APPEARANCE_CHANGED_EVENTS.load(Ordering::Relaxed),
            input_notifications: INPUT_NOTIFICATIONS.load(Ordering::Relaxed),
        },
        process_priority: ProcessPriorityDiagnostics {
            cycles: PROCESS_PRIORITY_CYCLES.load(Ordering::Relaxed),
            scanned_process_observations: PROCESS_PRIORITY_SCANNED.load(Ordering::Relaxed),
            selected_target_observations: PROCESS_PRIORITY_SELECTED.load(Ordering::Relaxed),
            applied_changes: PROCESS_PRIORITY_APPLIED.load(Ordering::Relaxed),
            already_applied_observations: PROCESS_PRIORITY_ALREADY_APPLIED.load(Ordering::Relaxed),
            preserved_observations: PROCESS_PRIORITY_PRESERVED.load(Ordering::Relaxed),
            process_exit_failures: PROCESS_PRIORITY_EXIT_FAILURES.load(Ordering::Relaxed),
            access_denied_failures: PROCESS_PRIORITY_ACCESS_FAILURES.load(Ordering::Relaxed),
            other_failures: PROCESS_PRIORITY_OTHER_FAILURES.load(Ordering::Relaxed),
        },
    };

    let bytes = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("Failed to serialize architecture diagnostics: {error}"))?;
    crate::config::storage::write_bytes_atomically(output_path, &bytes).map_err(|error| {
        format!(
            "Failed to write architecture diagnostics to {}: {error}",
            output_path.display()
        )
    })
}

pub fn record_worker_pass() {
    increment(&WORKER_PASSES);
}

pub fn record_signal_wake() {
    increment(&SIGNAL_WAKES);
}

pub fn record_timeout_wake() {
    increment(&TIMEOUT_WAKES);
}

pub fn record_process_snapshot_scan() {
    increment(&PROCESS_SNAPSHOT_SCANS);
}

pub fn record_process_path_enrichment_scan() {
    increment(&PROCESS_PATH_ENRICHMENT_SCANS);
}

pub fn record_foreground_process_query() {
    increment(&FOREGROUND_PROCESS_QUERIES);
}

pub fn record_visible_window_scan() {
    increment(&VISIBLE_WINDOW_SCANS);
}

pub fn record_top_level_window_scan() {
    increment(&TOP_LEVEL_WINDOW_SCANS);
}

pub fn record_windows_event(event: WindowsAutomationEvent) {
    increment(match event {
        WindowsAutomationEvent::ForegroundChanged => &FOREGROUND_CHANGED_EVENTS,
        WindowsAutomationEvent::WindowCreated => &WINDOW_CREATED_EVENTS,
        WindowsAutomationEvent::PowerChanged => &POWER_CHANGED_EVENTS,
        WindowsAutomationEvent::SessionChanged => &SESSION_CHANGED_EVENTS,
        WindowsAutomationEvent::AppearanceChanged => &APPEARANCE_CHANGED_EVENTS,
    });
}

pub fn record_input_notification() {
    increment(&INPUT_NOTIFICATIONS);
}

pub fn record_process_priority_cycle(scanned_processes: usize, selected_targets: usize) {
    increment(&PROCESS_PRIORITY_CYCLES);
    add(&PROCESS_PRIORITY_SCANNED, scanned_processes);
    add(&PROCESS_PRIORITY_SELECTED, selected_targets);
}

pub fn record_process_priority_applied() {
    increment(&PROCESS_PRIORITY_APPLIED);
}

pub fn record_process_priority_already_applied() {
    increment(&PROCESS_PRIORITY_ALREADY_APPLIED);
}

pub fn record_process_priority_preserved() {
    increment(&PROCESS_PRIORITY_PRESERVED);
}

pub fn record_process_priority_exit_failure() {
    increment(&PROCESS_PRIORITY_EXIT_FAILURES);
}

pub fn record_process_priority_access_failure() {
    increment(&PROCESS_PRIORITY_ACCESS_FAILURES);
}

pub fn record_process_priority_other_failure() {
    increment(&PROCESS_PRIORITY_OTHER_FAILURES);
}

fn run() -> &'static DiagnosticRun {
    RUN.get_or_init(|| DiagnosticRun {
        output_path: std::env::var_os(OUTPUT_PATH_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
        started_at: Instant::now(),
        started_at_utc: chrono::Utc::now().to_rfc3339(),
    })
}

fn increment(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

fn add(counter: &AtomicU64, value: usize) {
    counter.fetch_add(value.min(u64::MAX as usize) as u64, Ordering::Relaxed);
}
