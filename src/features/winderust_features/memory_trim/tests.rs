use std::{collections::VecDeque, path::PathBuf};

use crate::{control::memory_trim::MemoryTrimPlatform, foreground::ProcessInfo};

use super::*;

fn tracked_process() -> TrackedProcess {
    TrackedProcess {
        executable_path: r"C:\Apps\app.exe".to_owned(),
        creation_time: 1,
        previous_cpu_time: None,
        idle_since: None,
        trimmed_while_idle: false,
    }
}

fn cpu_sample(cpu_time_100ns: u64, sampled_at: Instant) -> ProcessCpuSample {
    ProcessCpuSample {
        cpu_time_100ns,
        sampled_at,
    }
}

struct BatchTestPlatform {
    sampled_at: Instant,
    fail_before_pid: u32,
    working_set_samples: VecDeque<u64>,
}

impl BatchTestPlatform {
    fn new(sampled_at: Instant, fail_before_pid: u32) -> Self {
        Self {
            sampled_at,
            fail_before_pid,
            working_set_samples: VecDeque::new(),
        }
    }
}

impl MemoryTrimPlatform for BatchTestPlatform {
    type Process = u32;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        _allow_cross_session_process_control: bool,
    ) -> Result<Self::Process, ProcessControlError> {
        Ok(target.id)
    }

    fn working_set_bytes(&mut self, _process: &Self::Process) -> Result<u64, ProcessControlError> {
        Ok(self.working_set_samples.pop_front().unwrap_or(MB))
    }

    fn cpu_sample(
        &mut self,
        _process: &Self::Process,
    ) -> Result<ProcessCpuSample, ProcessControlError> {
        Ok(cpu_sample(0, self.sampled_at))
    }

    fn trim(&mut self, process: &Self::Process) -> Result<(), ProcessControlError> {
        if *process < self.fail_before_pid {
            return Err(ProcessControlError::Failed("trim failed".to_owned()));
        }
        Ok(())
    }
}

fn batch_processes(count: usize) -> Vec<ProcessInfo> {
    let current_process_id = std::process::id();
    (1..)
        .filter(|&id| id != current_process_id)
        .take(count)
        .map(|id| {
            let name = format!("worker{id}.exe");
            ProcessInfo {
                id,
                creation_time: Some(1),
                parent_id: None,
                session_id: None,
                user_name: None,
                is_service_account: None,
                is_critical: Some(false),
                can_set_information: true,
                image_path: Some(PathBuf::from(r"C:\Apps").join(&name)),
                name,
            }
        })
        .collect()
}

fn run_batch_pass(
    manager: &mut MemoryTrimManager,
    processes: &[ProcessInfo],
    mode: MemoryTrimMode,
    platform: BatchTestPlatform,
    memory_load_percent: u8,
) -> MemoryTrimSnapshot {
    manager.update_with_mode(
        &mut MemoryTrimController::with_platform(platform),
        &MemoryTrimSettings {
            enabled: true,
            system_memory_load_threshold_percent: 80,
            process_working_set_threshold_mb: 1,
            process_idle_seconds: 0,
            ..Default::default()
        },
        true,
        true,
        Some(u32::MAX),
        mode,
        &mut CycleObservations::with_processes(processes.to_vec()),
        &mut ActionLog::new(16),
        || Ok(memory_load_percent),
    )
}

#[test]
fn automatic_trim_defers_after_failed_attempts_and_rechecks_pressure() {
    let mut manager = MemoryTrimManager::default();
    let limit = AUTOMATIC_TRIM_ATTEMPT_LIMIT;
    let processes = batch_processes(2 * limit + 1);
    let start = Instant::now();
    let run = |manager: &mut MemoryTrimManager, elapsed_seconds, pressure| {
        run_batch_pass(
            manager,
            &processes,
            MemoryTrimMode::Automatic,
            BatchTestPlatform::new(
                start + Duration::from_secs(elapsed_seconds),
                processes[limit].id,
            ),
            pressure,
        )
    };

    run(&mut manager, 0, 90);
    let first = run(&mut manager, 1, 90);
    assert_eq!(first.failed_processes, limit);
    assert_eq!(first.trimmed_processes, 0);
    assert!(first.has_deferred_candidates);

    let second = run(&mut manager, 2, 90);
    assert_eq!(second.failed_processes, 0);
    assert_eq!(second.trimmed_processes, limit);
    assert!(second.has_deferred_candidates);

    let low_pressure = run(&mut manager, 3, 70);
    assert_eq!(
        low_pressure.status,
        MemoryTrimStatus::WaitingForMemoryLoad {
            threshold_percent: 80
        }
    );
    assert_eq!(low_pressure.trimmed_processes, 0);
    assert!(!low_pressure.has_deferred_candidates);

    let pressure_returned = run(&mut manager, 4, 90);
    assert_eq!(pressure_returned.trimmed_processes, 0);
    assert_eq!(pressure_returned.failed_processes, 0);
}

#[test]
fn sibling_success_and_pressure_gaps_preserve_trim_failures() {
    let mut manager = MemoryTrimManager::default();
    let mut processes = batch_processes(2);
    processes[0].image_path = Some(r"C:\Apps\worker.exe".into());
    processes[1].image_path = Some("C:/Apps/worker.exe".into());
    let start = Instant::now();
    let run = |manager: &mut MemoryTrimManager, elapsed_seconds, pressure| {
        run_batch_pass(
            manager,
            &processes,
            MemoryTrimMode::Automatic,
            BatchTestPlatform::new(
                start + Duration::from_secs(elapsed_seconds),
                processes[1].id,
            ),
            pressure,
        )
    };

    run(&mut manager, 0, 90);
    let first = run(&mut manager, 1, 90);
    assert_eq!(first.failed_processes, 1);
    assert_eq!(first.trimmed_processes, 1);
    run(&mut manager, 2, 70);
    let sample_only = run(&mut manager, 3, 90);
    assert_eq!(sample_only.failed_processes, 0);
    assert_eq!(sample_only.trimmed_processes, 0);

    let limit = u64::from(execution_failure_suppression_threshold());
    for attempt in 1..limit {
        assert_eq!(run(&mut manager, 3 + attempt, 90).failed_processes, 1);
    }
    let suppressed = run(&mut manager, 3 + limit, 90);
    assert_eq!(suppressed.failed_processes, 0);
    assert_eq!(suppressed.skipped_processes, 2);
    assert_eq!(suppressed.auto_excluded_processes.len(), 1);
    assert_eq!(
        process_failure_key(&suppressed.auto_excluded_processes[0]),
        r"C:\Apps\worker.exe"
    );
    assert!(run(&mut manager, 4 + limit, 90)
        .auto_excluded_processes
        .is_empty());
}

#[test]
fn temporary_loss_of_process_metadata_does_not_rearm_memory_trim() {
    let mut manager = MemoryTrimManager::default();
    let mut processes = batch_processes(1);
    let process_id = processes[0].id;
    let start = Instant::now();
    let run = |manager: &mut MemoryTrimManager, processes: &[ProcessInfo], elapsed_seconds| {
        run_batch_pass(
            manager,
            processes,
            MemoryTrimMode::Automatic,
            BatchTestPlatform::new(start + Duration::from_secs(elapsed_seconds), 0),
            90,
        )
    };

    assert_eq!(run(&mut manager, &processes, 0).trimmed_processes, 0);
    assert_eq!(run(&mut manager, &processes, 1).trimmed_processes, 1);

    processes[0].creation_time = None;
    processes[0].is_critical = None;
    assert_eq!(run(&mut manager, &processes, 2).candidate_processes, 0);
    let tracked = &manager.tracked[&process_id];
    assert!(tracked.trimmed_while_idle);
    assert!(tracked.previous_cpu_time.is_none());
    assert!(tracked.idle_since.is_none());

    processes[0].creation_time = Some(1);
    processes[0].is_critical = Some(false);
    assert_eq!(run(&mut manager, &processes, 3).trimmed_processes, 0);
    assert_eq!(run(&mut manager, &processes, 4).trimmed_processes, 0);

    processes[0].creation_time = Some(2);
    assert_eq!(run(&mut manager, &processes, 5).trimmed_processes, 0);
    assert_eq!(run(&mut manager, &processes, 6).trimmed_processes, 1);

    run(&mut manager, &[], 7);
    assert!(manager.tracked.is_empty());
}

#[test]
fn exact_automatic_budget_does_not_defer_and_manual_trim_is_uncapped() {
    let mut manager = MemoryTrimManager::default();
    let limit = AUTOMATIC_TRIM_ATTEMPT_LIMIT;
    let processes = batch_processes(limit + 1);
    let start = Instant::now();
    for (elapsed_seconds, expected_trimmed) in [(0, 0), (1, limit)] {
        let snapshot = run_batch_pass(
            &mut manager,
            &processes[..limit],
            MemoryTrimMode::Automatic,
            BatchTestPlatform::new(start + Duration::from_secs(elapsed_seconds), 0),
            90,
        );
        assert_eq!(snapshot.trimmed_processes, expected_trimmed);
        assert!(!snapshot.has_deferred_candidates);
    }

    let manual = run_batch_pass(
        &mut manager,
        &processes,
        MemoryTrimMode::Manual,
        BatchTestPlatform::new(start, 0),
        70,
    );
    assert_eq!(manual.trimmed_processes, limit + 1);
    assert!(!manual.has_deferred_candidates);
}

#[test]
fn both_trim_modes_wait_after_working_set_falls_below_minimum() {
    let processes = batch_processes(1);
    let process_id = processes[0].id;
    let start = Instant::now();
    for mode in [MemoryTrimMode::Automatic, MemoryTrimMode::Manual] {
        let mut manager = MemoryTrimManager::default();
        run_batch_pass(
            &mut manager,
            &processes,
            MemoryTrimMode::Automatic,
            BatchTestPlatform::new(start, 0),
            90,
        );
        let snapshot = run_batch_pass(
            &mut manager,
            &processes,
            mode,
            BatchTestPlatform {
                working_set_samples: VecDeque::from([MB, MB - 1]),
                ..BatchTestPlatform::new(start + Duration::from_secs(1), u32::MAX)
            },
            90,
        );

        assert_eq!(snapshot.candidate_processes, 0);
        assert_eq!(snapshot.trimmed_processes, 0);
        assert_eq!(snapshot.failed_processes, 0);
        let tracked = &manager.tracked[&process_id];
        assert!(tracked.previous_cpu_time.is_none());
        assert!(tracked.idle_since.is_none());
        assert!(!tracked.trimmed_while_idle);
    }
}

#[test]
fn working_set_reduction_uses_decimal_byte_units() {
    assert_eq!(size_label(999), "999 B");
    assert_eq!(size_label(1_000), "1 KB");
    assert_eq!(size_label(999_999), "999 KB");
    assert_eq!(size_label(1_000_000), "1 MB");
    assert_eq!(size_label(256_000_000), "256 MB");
}

#[test]
fn repeated_process_failures_suppress_memory_trim_retries() {
    let mut manager = MemoryTrimManager::default();
    let mut log = ActionLog::new(8);
    let executable_path = r"C:\Apps\app.exe";

    manager
        .failure_suppression
        .record_process_failure(executable_path);
    manager
        .failure_suppression
        .record_process_failure(r"C:/Apps/app.exe");
    assert!(!manager.is_process_suppressed(
        42,
        "app.exe",
        executable_path,
        &mut log,
        &mut BTreeSet::new()
    ));

    manager
        .failure_suppression
        .record_process_failure(executable_path);
    assert!(manager.is_process_suppressed(
        42,
        "app.exe",
        executable_path,
        &mut log,
        &mut BTreeSet::new()
    ));
    assert!(manager.is_process_suppressed(
        43,
        "app.exe",
        r"C:/Apps/app.exe",
        &mut log,
        &mut BTreeSet::new()
    ));

    let entries = log.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].feature, ActionLogFeature::MemoryTrim);
    assert_eq!(entries[0].result, ActionLogResult::Skipped);
    assert!(entries[0].reason.contains("Stopped retrying Memory Trim"));
}

#[test]
fn successful_trim_clears_failures_after_deferred_sibling_is_attempted() {
    let mut manager = MemoryTrimManager::default();
    let limit = AUTOMATIC_TRIM_ATTEMPT_LIMIT;
    let mut processes = batch_processes(limit + 1);
    processes[0].image_path = Some(r"C:\Apps\worker.exe".into());
    processes[limit].image_path = Some("C:/Apps/worker.exe".into());
    let key = r"C:\Apps\worker.exe";
    let start = Instant::now();

    let failed = run_batch_pass(
        &mut manager,
        &processes[limit..],
        MemoryTrimMode::Manual,
        BatchTestPlatform::new(start, processes[limit].id + 1),
        70,
    );
    assert_eq!(failed.failed_processes, 1);
    assert!(manager.failure_suppression.has_key_failure(key));

    let run = |manager: &mut MemoryTrimManager, elapsed_seconds| {
        run_batch_pass(
            manager,
            &processes,
            MemoryTrimMode::Automatic,
            BatchTestPlatform::new(start + Duration::from_secs(elapsed_seconds), 0),
            90,
        )
    };

    run(&mut manager, 1);
    let deferred = run(&mut manager, 2);
    assert_eq!(deferred.trimmed_processes, limit);
    assert!(deferred.has_deferred_candidates);
    assert!(manager.failure_suppression.has_key_failure(key));

    let recovered = run(&mut manager, 3);
    assert_eq!(recovered.trimmed_processes, 1);
    assert!(!recovered.has_deferred_candidates);
    assert!(!manager.failure_suppression.has_key_failure(key));
}

#[test]
fn builtin_exclusions_cover_sensitive_windows_processes() {
    assert!(is_builtin_excluded("csrss.exe"));
    assert!(is_builtin_excluded("winlogon.exe"));
    assert!(!is_builtin_excluded("worker.exe"));
}

#[test]
fn foreground_skip_matches_pid_or_exact_path() {
    let foreground = Path::new(r"C:\Apps\Foreground\app.exe");

    assert!(should_ignore_foreground_process(
        true,
        42,
        Path::new(r"C:\Apps\helper.exe"),
        Some(42),
        Some(foreground),
    ));
    assert!(should_ignore_foreground_process(
        true,
        99,
        Path::new(r"c:\apps\foreground\APP.EXE"),
        Some(42),
        Some(foreground),
    ));
    assert!(!should_ignore_foreground_process(
        true,
        99,
        Path::new(r"D:\Other\app.exe"),
        Some(42),
        Some(foreground),
    ));
}

#[test]
fn trim_eligibility_rearms_only_after_process_activity() {
    let now = Instant::now();
    let mut process = tracked_process();
    let idle_duration = Duration::from_secs(300);

    assert!(!ready_to_trim(&mut process, 0.0, idle_duration, now));
    assert!(ready_to_trim(
        &mut process,
        0.0,
        idle_duration,
        now + idle_duration
    ));

    process.trimmed_while_idle = true;
    assert!(!ready_to_trim(
        &mut process,
        0.0,
        idle_duration,
        now + idle_duration
    ));
    assert!(!ready_to_trim(
        &mut process,
        2.0,
        idle_duration,
        now + idle_duration
    ));
    assert!(!process.trimmed_while_idle);
}

#[test]
fn missing_cpu_observation_requires_a_fresh_full_idle_interval() {
    let start = Instant::now();
    let idle_duration = Duration::from_secs(300);
    let mut process = tracked_process();
    assert!(!process.observe_cpu(cpu_sample(0, start), idle_duration, start));
    let first_idle = start + Duration::from_secs(1);
    assert!(!process.observe_cpu(cpu_sample(0, first_idle), idle_duration, first_idle));

    process.break_idle_interval();
    assert!(process.previous_cpu_time.is_none());
    assert!(process.idle_since.is_none());
    let resumed = start + Duration::from_secs(600);
    assert!(!process.observe_cpu(cpu_sample(0, resumed), idle_duration, resumed));
    let fresh_idle = resumed + Duration::from_secs(1);
    assert!(!process.observe_cpu(cpu_sample(0, fresh_idle), idle_duration, fresh_idle));
    let before_deadline = fresh_idle + idle_duration - Duration::from_secs(1);
    assert!(!process.observe_cpu(
        cpu_sample(0, before_deadline),
        idle_duration,
        before_deadline,
    ));
    let deadline = fresh_idle + idle_duration;
    assert!(process.observe_cpu(cpu_sample(0, deadline), idle_duration, deadline));
}

#[test]
fn missing_cpu_observation_cannot_rearm_a_trimmed_idle_process() {
    let start = Instant::now();
    let idle_duration = Duration::from_secs(300);
    let mut process = tracked_process();
    process.trimmed_while_idle = true;
    process.break_idle_interval();
    assert!(process.trimmed_while_idle);
    assert!(!process.observe_cpu(cpu_sample(0, start), idle_duration, start));
    let later = start + idle_duration;
    assert!(!process.observe_cpu(cpu_sample(0, later), idle_duration, later));
    assert!(process.trimmed_while_idle);

    let active = later + Duration::from_secs(1);
    let active_cpu_time = u64::MAX / 2;
    assert!(!process.observe_cpu(cpu_sample(active_cpu_time, active), idle_duration, active));
    assert!(!process.trimmed_while_idle);
    let fresh_idle = active + Duration::from_secs(1);
    assert!(!process.observe_cpu(
        cpu_sample(active_cpu_time, fresh_idle),
        idle_duration,
        fresh_idle,
    ));
    let deadline = fresh_idle + idle_duration;
    assert!(process.observe_cpu(
        cpu_sample(active_cpu_time, deadline),
        idle_duration,
        deadline,
    ));
}

#[test]
fn global_observation_gaps_preserve_identities_and_trimmed_episodes() {
    let start = Instant::now();
    let idle_duration = Duration::from_secs(300);
    let mut manager = MemoryTrimManager::default();
    for (id, trimmed) in [(42, false), (43, true)] {
        let mut process = tracked_process();
        process.creation_time = u64::from(id);
        process.previous_cpu_time = Some(cpu_sample(0, start));
        process.idle_since = Some(start);
        process.trimmed_while_idle = trimmed;
        manager.tracked.insert(id, process);
    }

    manager.break_idle_intervals();

    assert_eq!(manager.tracked.len(), 2);
    for (id, trimmed) in [(42, false), (43, true)] {
        let process = manager.tracked.get_mut(&id).unwrap();
        assert_eq!(process.creation_time, u64::from(id));
        assert_eq!(process.executable_path, r"C:\Apps\app.exe");
        assert!(process.previous_cpu_time.is_none());
        assert!(process.idle_since.is_none());
        assert_eq!(process.trimmed_while_idle, trimmed);
        let resumed = start + idle_duration;
        assert!(!process.observe_cpu(cpu_sample(0, resumed), idle_duration, resumed));
        let fresh_idle = resumed + Duration::from_secs(1);
        assert!(!process.observe_cpu(cpu_sample(0, fresh_idle), idle_duration, fresh_idle));
        let deadline = fresh_idle + idle_duration;
        assert_eq!(
            process.observe_cpu(cpu_sample(0, deadline), idle_duration, deadline),
            !trimmed,
        );
    }
}

#[test]
fn idle_working_set_refill_cannot_rearm_a_trimmed_process() {
    let start = Instant::now();
    let idle_duration = Duration::from_secs(300);
    for cpu_available in [false, true] {
        let mut process = tracked_process();
        process.previous_cpu_time = Some(cpu_sample(0, start));
        process.idle_since = Some(start);
        process.trimmed_while_idle = true;
        let below_threshold = start + Duration::from_secs(1);

        process.observe_below_working_set_threshold(
            cpu_available.then(|| cpu_sample(0, below_threshold)),
        );

        assert!(process.trimmed_while_idle);
        assert!(process.idle_since.is_none());
        let refilled = below_threshold + idle_duration;
        assert!(!process.observe_cpu(cpu_sample(0, refilled), idle_duration, refilled));
        let still_idle = refilled + idle_duration;
        assert!(!process.observe_cpu(cpu_sample(0, still_idle), idle_duration, still_idle));
        assert!(process.trimmed_while_idle);
    }
}

#[test]
fn activity_below_size_rearms_but_requires_full_idle_above_size() {
    let start = Instant::now();
    let idle_duration = Duration::from_secs(300);
    let mut process = tracked_process();
    process.previous_cpu_time = Some(cpu_sample(0, start));
    process.idle_since = Some(start);
    process.trimmed_while_idle = true;
    let active = start + Duration::from_secs(1);
    let active_cpu_time = u64::MAX / 2;

    process.observe_below_working_set_threshold(Some(cpu_sample(active_cpu_time, active)));

    assert!(!process.trimmed_while_idle);
    assert!(process.idle_since.is_none());
    let idle_below_threshold = active + idle_duration;
    process.observe_below_working_set_threshold(Some(cpu_sample(
        active_cpu_time,
        idle_below_threshold,
    )));
    assert!(process.idle_since.is_none());
    let above_threshold = idle_below_threshold + Duration::from_secs(1);
    assert!(!process.observe_cpu(
        cpu_sample(active_cpu_time, above_threshold),
        idle_duration,
        above_threshold,
    ));
    let before_deadline = above_threshold + idle_duration - Duration::from_secs(1);
    assert!(!process.observe_cpu(
        cpu_sample(active_cpu_time, before_deadline),
        idle_duration,
        before_deadline,
    ));
    let deadline = above_threshold + idle_duration;
    assert!(process.observe_cpu(
        cpu_sample(active_cpu_time, deadline),
        idle_duration,
        deadline,
    ));
}

#[test]
fn invalid_cpu_deltas_break_idleness_without_rearming() {
    let start = Instant::now();
    for current in [
        cpu_sample(100, start),
        cpu_sample(99, start + Duration::from_secs(1)),
    ] {
        for trimmed in [false, true] {
            let mut process = tracked_process();
            process.previous_cpu_time = Some(cpu_sample(100, start));
            process.idle_since = Some(start);
            process.trimmed_while_idle = trimmed;
            assert!(!process.observe_cpu(current, Duration::ZERO, current.sampled_at));
            assert!(process.previous_cpu_time.is_none());
            assert!(process.idle_since.is_none());
            assert_eq!(process.trimmed_while_idle, trimmed);
        }
    }
}

#[test]
fn trim_reason_reports_working_set_reduction_without_claiming_reclaimed_ram() {
    for mode in [MemoryTrimMode::Automatic, MemoryTrimMode::Manual] {
        assert!(trim_reason(mode, Some(MB)).ends_with("working-set reduction 1 MB."));
        assert!(trim_reason(mode, None).ends_with("working-set reduction unavailable."));
    }
}

#[test]
fn process_cpu_usage_percent_scales_by_processor_count() {
    let now = Instant::now();
    let previous = ProcessCpuSample {
        cpu_time_100ns: 0,
        sampled_at: now,
    };
    let current = ProcessCpuSample {
        cpu_time_100ns: 10_000_000,
        sampled_at: now + Duration::from_secs(1),
    };

    let usage = process_cpu_usage_percent(previous, current).unwrap();

    let processors = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    assert!((usage - 100.0 / processors as f32).abs() < 0.0001);
}
