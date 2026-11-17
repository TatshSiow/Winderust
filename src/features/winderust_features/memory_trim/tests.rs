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

    manager.record_process_failure(executable_path);
    manager.record_process_failure(r"C:/Apps/app.exe");
    assert!(!manager.is_process_suppressed(
        42,
        "app.exe",
        executable_path,
        &mut log,
        &mut BTreeSet::new()
    ));

    manager.record_process_failure(executable_path);
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
fn successful_process_clears_memory_trim_failure_suppression() {
    let mut manager = MemoryTrimManager::default();
    let mut log = ActionLog::new(8);
    let executable_path = r"C:\Apps\app.exe";

    manager.record_process_failure(executable_path);
    manager.record_process_failure(executable_path);
    manager.record_process_failure(executable_path);
    assert!(manager.is_process_suppressed(
        42,
        "app.exe",
        executable_path,
        &mut log,
        &mut BTreeSet::new()
    ));

    manager.clear_process_failure(r"C:/Apps/app.exe");
    assert!(!manager.is_process_suppressed(
        42,
        "app.exe",
        executable_path,
        &mut log,
        &mut BTreeSet::new()
    ));
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
