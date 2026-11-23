use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};

use crate::{control::memory_trim::MemoryTrimPlatform, foreground::ProcessInfo};

use super::*;

fn tracked_process() -> TrackedProcess {
    TrackedProcess {
        executable_path: PathBuf::from(r"C:\Apps\app.exe"),
        creation_time: 1,
        previous_cpu_time: None,
        idle_since: None,
        trimmed_while_idle: false,
        last_automatic_attempt: None,
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

fn batch_observations(processes: &[ProcessInfo], foreground_path: &str) -> CycleObservations {
    let mut processes = processes.to_vec();
    processes.push(ProcessInfo {
        id: u32::MAX,
        creation_time: Some(1),
        parent_id: None,
        session_id: None,
        user_name: None,
        is_service_account: None,
        is_critical: Some(false),
        can_set_information: true,
        image_path: Some(foreground_path.into()),
        name: "foreground.exe".to_owned(),
    });
    CycleObservations::with_processes(processes)
}

fn run_batch_pass(
    manager: &mut MemoryTrimManager,
    processes: &[ProcessInfo],
    mode: MemoryTrimMode,
    platform: BatchTestPlatform,
    memory_load_percent: u8,
) -> MemoryTrimSnapshot {
    run_observed_batch_pass(
        manager,
        batch_observations(processes, r"C:\Foreground\foreground.exe"),
        mode,
        platform,
        memory_load_percent,
    )
}

fn run_observed_batch_pass(
    manager: &mut MemoryTrimManager,
    mut observations: CycleObservations,
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
        &mut observations,
        &mut ActionLog::new(16),
        || Ok(memory_load_percent),
    )
}

#[test]
fn automatic_trim_preserves_native_path_and_restarts_on_identity_change() {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};

    let native_path = |unit| {
        let mut path = r"C:\Apps\directory-".encode_utf16().collect::<Vec<_>>();
        path.push(unit);
        path.extend(r"\worker.exe".encode_utf16());
        PathBuf::from(OsString::from_wide(&path))
    };
    let start = Instant::now();
    let mut manager = MemoryTrimManager::default();
    let mut processes = batch_processes(1);
    processes[0].name = "worker.exe".to_owned();
    let mut run = |processes: &[ProcessInfo], elapsed_seconds| {
        run_batch_pass(
            &mut manager,
            processes,
            MemoryTrimMode::Automatic,
            BatchTestPlatform::new(start + Duration::from_secs(elapsed_seconds), 0),
            90,
        )
        .trimmed_processes
    };

    processes[0].image_path = Some(native_path(0xd800));
    let initial = [run(&processes, 0), run(&processes, 1), run(&processes, 2)];
    processes[0].image_path = Some(native_path(0xd801));
    let changed_path = [run(&processes, 3), run(&processes, 4), run(&processes, 5)];
    processes[0].creation_time = Some(2);
    let new_generation = [run(&processes, 6), run(&processes, 7), run(&processes, 8)];

    assert_eq!([initial, changed_path, new_generation], [[0, 1, 0]; 3]);
}

#[test]
fn unavailable_foreground_image_pauses_trim_and_breaks_idle_qualification() {
    let mut manager = MemoryTrimManager::default();
    let mut processes = batch_processes(1);
    let background = r"C:\Apps\worker.exe";
    let distinct_foreground = r"C:\Other\worker.exe";
    processes[0].image_path = Some(background.into());
    let start = Instant::now();
    let mut run = |focus: Option<&str>, mode, seconds| {
        let observations = focus.map_or_else(
            || CycleObservations::with_processes(processes.clone()),
            |path| batch_observations(&processes, path),
        );
        run_observed_batch_pass(
            &mut manager,
            observations,
            mode,
            BatchTestPlatform::new(start + Duration::from_secs(seconds), 0),
            90,
        )
    };

    let auto = MemoryTrimMode::Automatic;
    let manual = MemoryTrimMode::Manual;
    assert_eq!(run(Some(distinct_foreground), auto, 0).trimmed_processes, 0);

    for (mode, seconds) in [(auto, 1), (manual, 2)] {
        let paused = run(None, mode, seconds);
        assert_eq!(paused.status, MemoryTrimStatus::ForegroundUnknown);
        assert_eq!(paused.memory_load_percent, Some(90));
        assert_eq!(paused.trimmed_processes, 0);
    }

    assert_eq!(run(Some(distinct_foreground), auto, 3).trimmed_processes, 0);
    assert_eq!(run(Some(distinct_foreground), auto, 4).trimmed_processes, 1);
    assert_eq!(run(Some(background), manual, 5).trimmed_processes, 0);
}

#[test]
fn low_pressure_during_manual_trim_restarts_automatic_idle_sampling() {
    let mut manager = MemoryTrimManager::default();
    let processes = batch_processes(1);
    let start = Instant::now();
    let mut run = |mode, elapsed_seconds, load| {
        run_batch_pass(
            &mut manager,
            &processes,
            mode,
            BatchTestPlatform::new(start + Duration::from_secs(elapsed_seconds), 0),
            load,
        )
    };

    assert_eq!(run(MemoryTrimMode::Automatic, 0, 90).trimmed_processes, 0);
    let manual = run(MemoryTrimMode::Manual, 1, 79);
    assert_eq!(manual.status, MemoryTrimStatus::ManualCompleted);
    assert_eq!(manual.trimmed_processes, 1);
    assert_eq!(run(MemoryTrimMode::Automatic, 2, 90).trimmed_processes, 0);
    assert_eq!(run(MemoryTrimMode::Automatic, 3, 90).trimmed_processes, 1);
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
    let key = OsStr::new(r"C:\Apps\worker.exe");
    let start = Instant::now();
    let run = |manager: &mut MemoryTrimManager, process_count, elapsed_seconds, pressure| {
        run_batch_pass(
            manager,
            &processes[..process_count],
            MemoryTrimMode::Automatic,
            BatchTestPlatform::new(
                start + Duration::from_secs(elapsed_seconds),
                processes[1].id,
            ),
            pressure,
        )
    };

    run(&mut manager, 1, 0, 90);
    let first = run(&mut manager, 1, 1, 90);
    assert_eq!(first.failed_processes, 1);
    assert_eq!(first.trimmed_processes, 0);
    run(&mut manager, 2, 2, 70);
    let sample_only = run(&mut manager, 2, 3, 90);
    assert_eq!(sample_only.failed_processes, 0);
    assert_eq!(sample_only.trimmed_processes, 0);

    let sibling_trimmed = run(&mut manager, 2, 4, 90);
    assert_eq!(sibling_trimmed.failed_processes, 0);
    assert_eq!(sibling_trimmed.trimmed_processes, 1);
    assert!(!sibling_trimmed.has_deferred_candidates);
    assert!(manager.failure_suppression.has_key_failure(key));

    let cooldown_seconds = AUTOMATIC_TRIM_ATTEMPT_COOLDOWN.as_secs();
    let cooling_down = run(&mut manager, 2, cooldown_seconds, 90);
    assert_eq!(cooling_down.failed_processes, 0);
    assert_eq!(cooling_down.trimmed_processes, 0);
    assert!(!cooling_down.has_deferred_candidates);

    let limit = u64::from(execution_failure_suppression_threshold());
    for attempt in 1..limit {
        let retry = run(&mut manager, 2, 1 + attempt * cooldown_seconds, 90);
        assert_eq!(retry.failed_processes, 1);
    }
    let after_last_attempt = 2 + (limit - 1) * cooldown_seconds;
    let suppressed = run(&mut manager, 2, after_last_attempt, 90);
    assert_eq!(suppressed.failed_processes, 0);
    assert_eq!(suppressed.skipped_processes, 2);
    assert_eq!(suppressed.auto_excluded_processes.len(), 1);
    assert_eq!(
        executable_grouping_key(Path::new(&suppressed.auto_excluded_processes[0])),
        r"C:\Apps\worker.exe"
    );
    assert!(run(&mut manager, 2, after_last_attempt + 1, 90)
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
    let attempted_at = start + Duration::from_secs(1);
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
                ..BatchTestPlatform::new(attempted_at, u32::MAX)
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
        assert_eq!(
            tracked.last_automatic_attempt,
            (mode == MemoryTrimMode::Automatic).then_some(attempted_at)
        );
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
fn trim_failure_suppression_preserves_native_executable_paths() {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};

    let mut native = r"C:\Apps\directory-".encode_utf16().collect::<Vec<_>>();
    native.push(0xd800);
    native.extend(r"\worker.exe".encode_utf16());
    let native = PathBuf::from(OsString::from_wide(&native));
    let replacement = r"C:\Apps\directory-�\worker.exe";
    let normal = r"C:\Apps\worker.exe";
    assert!(native.to_str().is_none());
    assert_eq!(native.to_string_lossy(), replacement);
    let mut processes = batch_processes(5);
    for (process, path) in processes.iter_mut().zip([
        native.clone(),
        replacement.into(),
        native,
        normal.into(),
        "C:/Apps/worker.exe".into(),
    ]) {
        process.name = "worker.exe".into();
        process.image_path = Some(path);
    }

    let settings = MemoryTrimSettings {
        enabled: true,
        process_working_set_threshold_mb: 1,
        ..Default::default()
    };
    let run = |manager: &mut MemoryTrimManager,
               settings: &MemoryTrimSettings,
               processes: &[ProcessInfo],
               fail_before_pid| {
        let mut log = ActionLog::new(16);
        let snapshot = manager.update_with_mode(
            &mut MemoryTrimController::with_platform(BatchTestPlatform::new(
                Instant::now(),
                fail_before_pid,
            )),
            settings,
            true,
            false,
            Some(u32::MAX),
            MemoryTrimMode::Manual,
            &mut batch_observations(processes, r"C:\Foreground\foreground.exe"),
            &mut log,
            || Ok(90),
        );
        (snapshot, log)
    };

    let mut manager = MemoryTrimManager::default();
    let threshold = execution_failure_suppression_threshold();
    for _ in 0..threshold {
        let (failed, _) = run(
            &mut manager,
            &settings,
            &processes[..1],
            processes[0].id + 1,
        );
        assert_eq!(failed.failed_processes, 1);
    }
    let (mixed, log) = run(&mut manager, &settings, &processes[..3], 0);
    let (_, repeated_log) = run(&mut manager, &settings, &processes[..3], 0);
    let outcomes = |log: &ActionLog| {
        log.entries()
            .iter()
            .map(|entry| (entry.process_id, entry.result))
            .collect::<Vec<_>>()
    };

    let configured = MemoryTrimSettings {
        exclusions: vec![crate::config::ProcessExclusionRule {
            executable_path: replacement.to_owned(),
            ..Default::default()
        }],
        ..settings
    };
    let (configured, configured_log) = run(
        &mut MemoryTrimManager::default(),
        &configured,
        &processes[..2],
        0,
    );

    for _ in 0..threshold {
        let (failed, _) = run(
            &mut manager,
            &settings,
            &processes[3..4],
            processes[3].id + 1,
        );
        assert_eq!(failed.failed_processes, 1);
    }
    let (valid, valid_log) = run(&mut manager, &settings, &processes[3..], 0);
    assert_eq!((mixed.trimmed_processes, mixed.skipped_processes), (1, 2));
    assert!(mixed.auto_excluded_processes.is_empty());
    assert_eq!(
        outcomes(&log),
        [
            (Some(processes[0].id), ActionLogResult::Skipped),
            (Some(processes[1].id), ActionLogResult::Applied),
        ]
    );
    assert_eq!(
        outcomes(&repeated_log),
        [(Some(processes[1].id), ActionLogResult::Applied)]
    );
    assert_eq!(configured.trimmed_processes, 1);
    assert_eq!(
        outcomes(&configured_log),
        [(Some(processes[0].id), ActionLogResult::Applied)]
    );
    assert_eq!(valid.skipped_processes, 2);
    assert_eq!(valid.auto_excluded_processes, [normal]);
    assert_eq!(
        outcomes(&valid_log),
        [(Some(processes[3].id), ActionLogResult::Skipped)]
    );
}

#[test]
fn successful_trim_clears_failures_after_deferred_sibling_is_attempted() {
    let mut manager = MemoryTrimManager::default();
    let limit = AUTOMATIC_TRIM_ATTEMPT_LIMIT;
    let mut processes = batch_processes(limit + 1);
    processes[0].image_path = Some(r"C:\Apps\worker.exe".into());
    processes[limit].image_path = Some("C:/Apps/worker.exe".into());
    let key = OsStr::new(r"C:\Apps\worker.exe");
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
        assert_eq!(process.executable_path, Path::new(r"C:\Apps\app.exe"));
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
