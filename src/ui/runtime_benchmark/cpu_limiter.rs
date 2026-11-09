//! Disposable-worker comparison of CPU Limiter duty cycles and fixed concurrency.
//! Request latency belongs to the restrained process, not an external foreground app.

use std::{
    collections::BTreeMap,
    hint::black_box,
    io::{BufRead, BufReader, Write},
    os::windows::process::CommandExt,
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::{
    backend::crash_recovery,
    control::{
        cpu_allocation::{
            CpuAllocationApplyOutcome, CpuAllocationClaim, CpuAllocationCoordinator,
            CpuAllocationRequest, CpuSetInventory,
        },
        cpu_limiter::{CpuLimiterController, CpuLimiterTarget},
        process::{open_process_for_set_information, ControlOwner},
        suspension::{SuspensionController, SuspensionTarget},
    },
    cpu::PerProcessorUsageMonitor,
    cpu_topology::{CpuSelection, CpuTopology, LogicalProcessor, ProcessorId},
    foreground::{process_runs_as_service_account_from_handle, query_process_image_path},
    platform::windows::{
        cpu_allocation as windows_cpu_allocation,
        priority_efficiency as windows_priority_efficiency,
        process::{self as windows_process, ProcessAccess},
        runtime_benchmark as diagnostics,
    },
    power,
    win_util::WinHandle,
    SingleInstanceGuard,
};

const WARMUP: Duration = Duration::from_secs(2);
const SAMPLE: Duration = Duration::from_secs(4);
const REPLY_TIMEOUT: Duration = Duration::from_secs(3);
const WORKER_LIFETIME: Duration = Duration::from_secs(30);
const PASSES: usize = 4;
const MAX_CAPTURE_SPAN_NS: u64 = 2_000_000;

pub(super) fn run_if_requested() -> bool {
    let args: Vec<_> = std::env::args_os().collect();
    match args.get(1).and_then(|arg| arg.to_str()) {
        Some("--cpu-limiter-benchmark-worker") => {
            let result = args
                .get(2)
                .and_then(|arg| arg.to_str())
                .and_then(|arg| arg.parse::<usize>().ok())
                .filter(|count| (1..=64).contains(count) && args.len() == 3)
                .ok_or_else(|| "Worker count must be 1..=64".to_owned())
                .and_then(run_worker);
            if let Err(error) = result {
                eprintln!("CPU Limiter benchmark worker: {error}");
                std::process::exit(1);
            }
        }
        Some("--cpu-limiter-benchmark") => {
            let Some(directory) = args.get(2).filter(|_| args.len() == 3) else {
                eprintln!("Expected --cpu-limiter-benchmark <existing-output-directory>");
                return true;
            };
            let path = Path::new(directory);
            let result = run(path);
            let status = match result {
                Ok(()) => "ok".to_owned(),
                Err(error) => format!("error: {error}"),
            };
            if let Err(error) = super::write(path, "finished.txt", &status) {
                eprintln!("{status}; {error}");
            }
        }
        _ => return false,
    }
    true
}

#[derive(Clone, Copy, Debug)]
enum Condition {
    Off,
    Duty50,
    HalfPool,
}

impl Condition {
    fn name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Duty50 => "CPU Limiter 50%",
            Self::HalfPool => "Fixed half-pool Processor Affinity (Hard)",
        }
    }
}

fn run(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err("Benchmark directory does not exist".to_owned());
    }
    let _guard = SingleInstanceGuard::acquire(false).ok_or_else(|| {
        "Close Winderust before benchmarking; another instance owns the runtime".to_owned()
    })?;
    let topology = windows_cpu_allocation::topology::query().map_err(|e| format!("{e:?}"))?;
    if topology.groups().len() != 1 || topology.groups()[0].number != 0 {
        return Err(
            "This fixed Processor Affinity (Hard) comparison requires group 0 only".to_owned(),
        );
    }
    let original_plan = power::active_plan()?.guid;
    if power::list_plans()?
        .iter()
        .any(|plan| plan.name == "Winderust Adaptive")
    {
        return Err(
            "A Winderust Adaptive plan is present; restore the normal plan before benchmarking"
                .to_owned(),
        );
    }
    let mut recovery = crash_recovery::RecoveryClient::start();
    let mut report = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "release_build": !cfg!(debug_assertions),
        "topology": topology,
        "warmup_seconds": WARMUP.as_secs(),
        "sample_seconds": SAMPLE.as_secs(),
        "passes": PASSES,
        "power_plan_before": original_plan,
        "host_job": diagnostics::current_job_info(),
        "probe_interval_ms": "13..=23, deterministic dither",
        "response_percentile_method": "nearest rank",
        "maximum_endpoint_capture_span_ns": MAX_CAPTURE_SPAN_NS,
        "half_pool_policy": "First ceil(eligible physical cores / 2) per efficiency class, retaining all eligible siblings of each chosen core",
        "limitations": [
            "Request/response latency is inside the restrained target; it does not measure an external foreground application.",
            "A fixed CPU pool bounds concurrency, not CPU time. A single thread may use a full core in either pool; hybrid cores have unequal capacity.",
            "Synthetic integer throughput and short samples do not establish application performance or an automatic restraint policy.",
            "Parent CPU includes its controller and probes; crash recovery helper CPU is not included. Power and thermal telemetry are not sampled.",
            "Per-thread and process CPU times share Windows accounting. System-wide processor usage does not establish target residency; immediate-job queries do not expose ancestor limits."
        ],
        "samples": [],
        "complete": false,
    });
    let result = (|| {
        if let Some(error) = crash_recovery::startup_error() {
            return Err(error);
        }
        check_kernel()?;
        let own_process = windows_process::open(std::process::id(), ProcessAccess::SafetyOnly)
            .map_err(|e| format!("Open benchmark host: {e:?}"))?;
        let mut baseline = None;
        for pass in 0..PASSES {
            // Alternate both condition and workload order to reduce order bias.
            let conditions = if pass % 2 == 0 {
                [Condition::Off, Condition::Duty50, Condition::HalfPool]
            } else {
                [Condition::HalfPool, Condition::Duty50, Condition::Off]
            };
            let workloads = if pass % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            };
            for multithreaded in workloads {
                for condition in conditions {
                    let sample = run_case(
                        &topology,
                        &own_process,
                        &mut baseline,
                        pass,
                        multithreaded,
                        condition,
                    )?;
                    report["samples"]
                        .as_array_mut()
                        .ok_or_else(|| "Invalid benchmark report samples".to_owned())?
                        .push(sample);
                    save_report(path, &report)?;
                }
            }
        }
        Ok(())
    })();
    let cleanup = recovery.finish();
    let plan_check = power::active_plan().and_then(|plan| {
        report["power_plan_after"] = json!(plan.guid);
        if plan.guid == original_plan {
            Ok(())
        } else {
            Err("Active power plan changed during the comparison".to_owned())
        }
    });
    let cleanup = combine_errors(cleanup, plan_check);
    match (&result, &cleanup) {
        (Ok(()), Ok(())) => report["complete"] = json!(true),
        _ => report["error"] = json!(combine_errors(result.clone(), cleanup.clone()).err()),
    }
    save_report(path, &report)?;
    combine_errors(result, cleanup)
}

fn save_report(path: &Path, report: &Value) -> Result<(), String> {
    super::write(
        path,
        "result.json",
        &serde_json::to_string_pretty(report).map_err(|e| e.to_string())?,
    )
}

fn combine_errors<T>(result: Result<T, String>, cleanup: Result<(), String>) -> Result<T, String> {
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!("{error}; cleanup: {cleanup}")),
    }
}

/// Select whole eligible physical cores so SMT siblings do not silently become
/// independent cores. A one-core efficiency class is retained in full.
fn half_pool(processors: &[LogicalProcessor], baseline: u64) -> Result<u64, String> {
    let mut classes = BTreeMap::<u8, BTreeMap<ProcessorId, u64>>::new();
    let mut discovered = 0;
    for processor in processors {
        if processor.id.group != 0 || processor.id.number >= 64 {
            return Err("Half-pool comparison supports processor group 0 only".to_owned());
        }
        let bit = 1_u64 << processor.id.number;
        if baseline & bit != 0 {
            discovered |= bit;
            *classes
                .entry(processor.efficiency_class)
                .or_default()
                .entry(processor.core)
                .or_default() |= bit;
        }
    }
    if baseline == 0 || discovered != baseline {
        return Err("Worker baseline is empty or absent from the captured topology".to_owned());
    }
    let selected = classes.values().fold(0, |mask, cores| {
        cores
            .values()
            .take(cores.len().div_ceil(2))
            .fold(mask, |mask, core| mask | core)
    });
    if selected == baseline {
        return Err(
            "Eligible physical cores cannot form a smaller whole-core half pool".to_owned(),
        );
    }
    Ok(selected)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct Placement {
    affinity_mask: usize,
    system_mask: usize,
    cpu_set_ids: Vec<u32>,
}

fn placement(process: &WinHandle) -> Result<Placement, String> {
    windows_cpu_allocation::validate_group_zero_affinity(process).map_err(|e| format!("{e:?}"))?;
    let (affinity_mask, system_mask) =
        windows_cpu_allocation::query_affinity(process).map_err(|e| format!("{e:?}"))?;
    Ok(Placement {
        affinity_mask,
        system_mask,
        cpu_set_ids: windows_cpu_allocation::query_cpu_sets(process)
            .map_err(|e| format!("{e:?}"))?,
    })
}

fn inspect_worker(worker: &Worker) -> Result<(SuspensionTarget, WinHandle, Placement), String> {
    let process = windows_process::open(worker.child.id(), ProcessAccess::SafetyOnly)
        .map_err(|e| format!("Open worker: {e:?}"))?;
    let path = query_process_image_path(&process).map_err(|e| format!("Worker image path: {e}"))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Worker image has no UTF-8 filename".to_owned())?
        .to_owned();
    let (creation, _) = process
        .process_times()
        .map_err(|e| format!("Worker times: {e}"))?;
    let target = SuspensionTarget::automatic(
        worker.child.id(),
        name,
        path,
        creation,
        process_runs_as_service_account_from_handle(&process),
    );
    // This shared boundary verifies identity, same-session access and protected
    // process rules before either production controller may mutate the worker.
    let (_, process) =
        open_process_for_set_information(&target.process, false).map_err(|e| e.to_string())?;
    let baseline = placement(&process)?;
    if !baseline.cpu_set_ids.is_empty() {
        return Err(
            "Worker inherited CPU Sets; clear the external constraint before comparing pools"
                .to_owned(),
        );
    }
    Ok((target, process, baseline))
}

fn run_case(
    topology: &CpuTopology,
    own_process: &WinHandle,
    expected_baseline: &mut Option<Placement>,
    pass: usize,
    multithreaded: bool,
    condition: Condition,
) -> Result<Value, String> {
    // Children inherit the host's placement. Verify that inheritance rather than
    // widening it to the system mask or changing any external constraint.
    let inherited = placement(own_process)?;
    let eligible = (inherited.affinity_mask & inherited.system_mask) as u64;
    let threads = if multithreaded {
        eligible.count_ones() as usize
    } else {
        1
    };
    let half = half_pool(topology.processors(), eligible)?;
    let mut worker = Worker::spawn(threads)?;
    let (target, process, baseline) = inspect_worker(&worker)?;
    let inherited_job = worker.job_info()?;
    let priority_class = match windows_priority_efficiency::query_priority(&process) {
        Ok(priority) => json!({ "value": priority }),
        Err(error) => json!({ "error": format!("{error:?}") }),
    };
    let power_throttling = match windows_priority_efficiency::query_power(&process) {
        Ok(state) => json!({
            "version": state.version,
            "control_mask": state.control_mask,
            "state_mask": state.state_mask,
        }),
        Err(error) => json!({ "error": format!("{error:?}") }),
    };
    if baseline != inherited {
        return Err("Worker did not inherit the host's exact placement".to_owned());
    }
    if expected_baseline
        .as_ref()
        .is_some_and(|expected| expected != &baseline)
    {
        return Err("Worker inherited placement changed between samples".to_owned());
    }
    *expected_baseline = Some(baseline.clone());
    let suspension = Arc::new(Mutex::new(SuspensionController::default()));
    let mut limiter = CpuLimiterController::new(Arc::clone(&suspension))?;
    let mut allocation = CpuAllocationCoordinator::default();
    let result = (|| {
        let backend = match condition {
            Condition::Off => "none",
            Condition::Duty50 => {
                limiter.replace_targets(vec![CpuLimiterTarget {
                    suspension_target: target.clone(),
                    allowed_cpu_time_percent: 50,
                    allow_cross_session_process_control: false,
                    ancestor_process_ids: Vec::new(),
                }])?;
                check_limiter(&mut limiter, &target)?;
                if limiter.fallback_target_keys()?.contains(&target.key()) {
                    "exact-thread fallback"
                } else {
                    "job freeze"
                }
            }
            Condition::HalfPool => {
                let selection = CpuSelection::from_group_zero_mask(topology, half)
                    .map_err(|e| e.to_string())?;
                let outcome = allocation
                    .apply_policy_claim(
                        &mut CpuSetInventory::default(),
                        Ok(topology),
                        CpuAllocationClaim {
                            target: target.process.clone(),
                            owner: ControlOwner::ProcessorAffinityHard,
                            request: CpuAllocationRequest::HardAffinity { selection },
                        },
                        false,
                    )
                    .map_err(|e| e.to_string())?;
                if !matches!(
                    outcome,
                    CpuAllocationApplyOutcome::Applied | CpuAllocationApplyOutcome::Unchanged
                ) {
                    return Err(format!("Hard placement was not applied: {outcome:?}"));
                }
                "Processor Affinity (Hard) coordinator"
            }
        };
        let expected = Placement {
            affinity_mask: if matches!(condition, Condition::HalfPool) {
                half as usize
            } else {
                baseline.affinity_mask
            },
            ..baseline.clone()
        };
        if placement(&process)? != expected {
            return Err("Applied worker placement did not match readback".to_owned());
        }
        let applied_job = worker.job_info()?;
        let warmup = Instant::now();
        while warmup.elapsed() < WARMUP {
            worker.probe()?;
            probe_pause(worker.sequence);
        }
        let parent_before = cpu_ticks(own_process)?;
        let mut ambient = PerProcessorUsageMonitor::default();
        let ambient_started = Instant::now();
        let _ = ambient.sample();
        let before = worker.coherent_endpoint()?;
        let mut latencies = Vec::new();
        let mut rejected_endpoints = 0;
        let mut maximum_capture_span_ns = before.capture_span_ns;
        let deadline = Instant::now() + SAMPLE + REPLY_TIMEOUT;
        let after = loop {
            if Instant::now() >= deadline {
                return Err(
                    "No coherent sample endpoint before the measurement deadline".to_owned(),
                );
            }
            probe_pause(worker.sequence);
            let (snapshot, latency) = worker.probe()?;
            let elapsed = snapshot
                .elapsed_ns
                .checked_sub(before.elapsed_ns)
                .ok_or_else(|| "Worker clock went backwards".to_owned())?;
            latencies.push(latency);
            maximum_capture_span_ns = maximum_capture_span_ns.max(snapshot.capture_span_ns);
            if snapshot.capture_span_ns > MAX_CAPTURE_SPAN_NS {
                rejected_endpoints += 1;
            } else if elapsed >= SAMPLE.as_nanos() as u64 {
                break snapshot;
            }
        };
        let parent_after = cpu_ticks(own_process)?;
        let ambient_usage = ambient
            .sample()
            .filter(|usage| usage.len() == topology.processors().len());
        let ambient_seconds = ambient_started.elapsed().as_secs_f64();
        if matches!(condition, Condition::Duty50) {
            check_limiter(&mut limiter, &target)?;
        }
        if placement(&process)? != expected {
            return Err("Worker placement changed during sampling".to_owned());
        }
        let elapsed_ns = after
            .elapsed_ns
            .checked_sub(before.elapsed_ns)
            .filter(|elapsed| *elapsed > 0)
            .ok_or_else(|| "Invalid sample duration".to_owned())?;
        let seconds = elapsed_ns as f64 / 1e9;
        let blocks = checked_work(&before, &after, threads)?;
        let cpu = after
            .cpu_100ns
            .checked_sub(before.cpu_100ns)
            .ok_or_else(|| "Worker CPU time went backwards".to_owned())?;
        let compute_cpu = thread_cpu_deltas(&before, &after, threads)?;
        let compute_cpu_total = compute_cpu
            .iter()
            .try_fold(0_u64, |sum, ticks| sum.checked_add(*ticks))
            .ok_or_else(|| "Compute-thread CPU time overflowed".to_owned())?;
        let handler_cpu = after
            .handler_cpu_100ns
            .checked_sub(before.handler_cpu_100ns)
            .ok_or_else(|| "Handler CPU time went backwards".to_owned())?;
        let parent_cpu = parent_after
            .checked_sub(parent_before)
            .ok_or_else(|| "Host CPU time went backwards".to_owned())?;
        let mut sorted = latencies.clone();
        sorted.sort_by(f64::total_cmp);
        Ok(json!({
            "pass": pass + 1,
            "condition": condition.name(),
            "backend": backend,
            "workload": if multithreaded { "MT" } else { "ST" },
            "worker_pid": worker.child.id(),
            "worker_threads": threads,
            "baseline": baseline,
            "applied": expected,
            "half_pool_mask": format!("{half:#018x}"),
            "sample_seconds": seconds,
            "completed_blocks": blocks,
            "blocks_per_second": blocks as f64 / seconds,
            "worker_cpu_ms": cpu as f64 / 10_000.0,
            "worker_cpu_percent_one_core": cpu as f64 / (seconds * 100_000.0),
            "compute_thread_cpu_ms": compute_cpu.iter().map(|ticks| *ticks as f64 / 10_000.0).collect::<Vec<_>>(),
            "compute_thread_cpu_total_ms": compute_cpu_total as f64 / 10_000.0,
            "handler_cpu_ms": handler_cpu as f64 / 10_000.0,
            "process_minus_known_thread_cpu_ms": (i128::from(cpu) - i128::from(compute_cpu_total) - i128::from(handler_cpu)) as f64 / 10_000.0,
            "ambient_per_processor_busy_percent": ambient_usage,
            "ambient_sampling_seconds": ambient_seconds,
            "ambient_usage_scope": "Whole-system group-0 processor indices; unavailable on missing or mismatched samples, not target residency",
            "inherited_job": inherited_job,
            "applied_job": applied_job,
            "worker_priority_class": priority_class,
            "worker_power_throttling": power_throttling,
            "parent_cpu_ms": parent_cpu as f64 / 10_000.0,
            "response_median_ms": percentile(&sorted, 50)?,
            "response_p95_ms": percentile(&sorted, 95)?,
            "response_samples_ms": latencies,
            "incoherent_counter_snapshots": rejected_endpoints,
            "maximum_counter_capture_span_ns": maximum_capture_span_ns,
            "first_reply": before,
            "last_reply": after,
            "restoration_verified": true,
        }))
    })();
    // Release every owned mechanism before asking the worker to stop, including
    // after failed measurements. A failed release invalidates the whole run.
    let cleanup = cleanup_case(
        &mut limiter,
        &suspension,
        &mut allocation,
        &target,
        &process,
        &baseline,
    );
    let restored = cleanup.is_ok();
    let result = combine_errors(result, cleanup);
    if restored {
        combine_errors(result, worker.stop())
    } else {
        // Drop terminates only this exact Child after all restoration attempts.
        result
    }
}

fn check_limiter(
    limiter: &mut CpuLimiterController,
    target: &SuspensionTarget,
) -> Result<(), String> {
    let failures = limiter.drain_failures()?;
    if !failures.is_empty() {
        return Err(format!("CPU Limiter failures: {failures:?}"));
    }
    if !limiter.managed_target_keys()?.contains(&target.key()) {
        return Err("CPU Limiter did not retain the worker".to_owned());
    }
    Ok(())
}

fn cleanup_case(
    limiter: &mut CpuLimiterController,
    suspension: &Mutex<SuspensionController>,
    allocation: &mut CpuAllocationCoordinator,
    target: &SuspensionTarget,
    process: &WinHandle,
    baseline: &Placement,
) -> Result<(), String> {
    let mut errors = Vec::new();
    for result in [
        limiter.replace_targets(Vec::new()),
        limiter.shutdown(),
        allocation.shutdown(),
    ] {
        if let Err(error) = result {
            errors.push(error);
        }
    }
    match suspension.lock() {
        Ok(mut suspension) => {
            if let Err(error) = suspension.shutdown() {
                errors.push(error);
            }
            if suspension.is_frozen_target(target) {
                errors.push("Worker remains frozen after CPU Limiter shutdown".to_owned());
            }
        }
        Err(_) => errors.push("Suspension controller state is poisoned".to_owned()),
    }
    if limiter.has_managed_state()
        || allocation.has_managed_state()
        || allocation.has_pending_reconciliation()
    {
        errors.push("Controller has pending worker state after shutdown".to_owned());
    }
    match placement(process) {
        Ok(current) if &current == baseline => {}
        Ok(current) => errors.push(format!(
            "Worker restoration mismatch: {current:?}, expected {baseline:?}"
        )),
        Err(error) => errors.push(error),
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn cpu_ticks(process: &WinHandle) -> Result<u64, String> {
    process
        .process_times()
        .map(|(_, cpu)| cpu)
        .map_err(|e| format!("Process CPU time: {e}"))
}

fn probe_pause(sequence: u64) {
    // Avoid locking probes to the production limiter's 100 ms cycle.
    thread::sleep(Duration::from_millis(13 + (sequence * 7) % 11));
}

fn percentile(sorted: &[f64], percent: usize) -> Result<f64, String> {
    if sorted.is_empty() || !(1..=100).contains(&percent) {
        return Err("Invalid response percentile input".to_owned());
    }
    Ok(sorted[(sorted.len() * percent).div_ceil(100) - 1])
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    sequence: u64,
    // CPU ticks and counters are bracketed by a worker clock interval; its
    // midpoint supplies the sample boundary and its span bounds read skew.
    elapsed_ns: u64,
    capture_span_ns: u64,
    cpu_100ns: u64,
    compute_cpu_100ns: Vec<u64>,
    handler_cpu_100ns: u64,
    blocks: Vec<u64>,
    checksums: Vec<u64>,
}

fn checked_work(before: &Snapshot, after: &Snapshot, threads: usize) -> Result<u64, String> {
    if before.blocks.len() != threads
        || after.blocks.len() != threads
        || before.checksums.len() != threads
        || after.checksums.len() != threads
        || before.checksums.contains(&0)
        || after.checksums.contains(&0)
    {
        return Err("Invalid worker counters or checksums".to_owned());
    }
    let total = before
        .blocks
        .iter()
        .zip(&after.blocks)
        .try_fold(0_u64, |total, (before, after)| {
            after
                .checked_sub(*before)
                .and_then(|delta| total.checked_add(delta))
        })
        .ok_or_else(|| "Worker counters went backwards or overflowed".to_owned())?;
    if total == 0 {
        Err("Worker completed no useful blocks".to_owned())
    } else {
        Ok(total)
    }
}

fn thread_cpu_deltas(
    before: &Snapshot,
    after: &Snapshot,
    threads: usize,
) -> Result<Vec<u64>, String> {
    if before.compute_cpu_100ns.len() != threads || after.compute_cpu_100ns.len() != threads {
        return Err("Invalid compute-thread CPU counter count".to_owned());
    }
    before
        .compute_cpu_100ns
        .iter()
        .zip(&after.compute_cpu_100ns)
        .map(|(before, after)| {
            after
                .checked_sub(*before)
                .ok_or_else(|| "Compute-thread CPU time went backwards".to_owned())
        })
        .collect()
}

struct Worker {
    child: Child,
    input: ChildStdin,
    replies: Receiver<Result<String, String>>,
    sequence: u64,
}

impl Worker {
    fn spawn(count: usize) -> Result<Self, String> {
        let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
            .arg("--cpu-limiter-benchmark-worker")
            .arg(count.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| "Missing worker stdin".to_owned())?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| "Missing worker stdout".to_owned())?;
        let (sender, replies) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if sender.send(line.map_err(|e| e.to_string())).is_err() {
                    break;
                }
            }
        });
        let worker = Self {
            child,
            input,
            replies,
            sequence: 0,
        };
        if worker.line()? != "READY" {
            return Err("Invalid worker handshake".to_owned());
        }
        Ok(worker)
    }

    fn line(&self) -> Result<String, String> {
        self.replies
            .recv_timeout(REPLY_TIMEOUT)
            .map_err(|e| format!("Worker reply: {e}"))?
    }

    fn probe(&mut self) -> Result<(Snapshot, f64), String> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| "Probe sequence overflow".to_owned())?;
        let started = Instant::now();
        writeln!(self.input, "PING {}", self.sequence)
            .and_then(|_| self.input.flush())
            .map_err(|e| e.to_string())?;
        let line = self.line()?;
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        let snapshot: Snapshot =
            serde_json::from_str(&line).map_err(|e| format!("Worker snapshot: {e}"))?;
        if snapshot.sequence != self.sequence {
            return Err("Worker reply sequence mismatch".to_owned());
        }
        Ok((snapshot, elapsed))
    }

    fn job_info(&mut self) -> Result<Value, String> {
        writeln!(self.input, "JOB")
            .and_then(|_| self.input.flush())
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&self.line()?).map_err(|e| format!("Worker job diagnostics: {e}"))
    }

    fn coherent_endpoint(&mut self) -> Result<Snapshot, String> {
        for _ in 0..8 {
            let (snapshot, _) = self.probe()?;
            if snapshot.capture_span_ns <= MAX_CAPTURE_SPAN_NS {
                return Ok(snapshot);
            }
        }
        Err("Counter reads could not establish a coherent starting endpoint".to_owned())
    }

    fn stop(&mut self) -> Result<(), String> {
        writeln!(self.input, "STOP")
            .and_then(|_| self.input.flush())
            .map_err(|e| e.to_string())?;
        if self.line()? != "STOPPED" {
            return Err("Worker did not acknowledge stop".to_owned());
        }
        let deadline = Instant::now() + REPLY_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().map_err(|e| e.to_string())? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("Worker exited: {status}"))
                };
            }
            if Instant::now() >= deadline {
                return Err("Worker exit timed out".to_owned());
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            if let Err(error) = self.child.kill() {
                eprintln!(
                    "Failed to terminate benchmark child {}: {error}",
                    self.child.id()
                );
                return;
            }
            let deadline = Instant::now() + REPLY_TIMEOUT;
            while !matches!(self.child.try_wait(), Ok(Some(_))) {
                if Instant::now() >= deadline {
                    eprintln!("Benchmark child {} exit is unconfirmed", self.child.id());
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

#[repr(align(64))]
struct Counter {
    blocks: AtomicU64,
    checksum: AtomicU64,
}

struct CpuWorkers {
    stop: Arc<AtomicBool>,
    counters: Arc<Vec<Counter>>,
    threads: Vec<JoinHandle<()>>,
}

impl CpuWorkers {
    fn start(count: usize) -> Result<Self, String> {
        if !(1..=64).contains(&count) {
            return Err("Worker count must be 1..=64".to_owned());
        }
        let counters = Arc::new(
            (0..count)
                .map(|index| Counter {
                    blocks: AtomicU64::new(0),
                    checksum: AtomicU64::new(index as u64 + 1),
                })
                .collect::<Vec<_>>(),
        );
        let mut workers = Self {
            stop: Arc::new(AtomicBool::new(false)),
            counters,
            threads: Vec::new(),
        };
        let deadline = Instant::now() + WORKER_LIFETIME;
        for index in 0..count {
            let counters = Arc::clone(&workers.counters);
            let stop = Arc::clone(&workers.stop);
            workers.threads.push(
                thread::Builder::new()
                    .name(format!("limiter-bench-{index}"))
                    .spawn(move || {
                        let mut value = index as u64 + 1;
                        let mut blocks = 0;
                        while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
                            value = block(black_box(value));
                            blocks += 1;
                            counters[index].checksum.store(value, Ordering::Relaxed);
                            counters[index].blocks.store(blocks, Ordering::Relaxed);
                        }
                    })
                    .map_err(|e| e.to_string())?,
            );
        }
        Ok(workers)
    }

    fn stop(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Relaxed);
        let mut failed = false;
        for worker in self.threads.drain(..) {
            failed |= worker.join().is_err();
        }
        if failed {
            Err("CPU worker panicked".to_owned())
        } else {
            Ok(())
        }
    }
}

impl Drop for CpuWorkers {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn run_worker(count: usize) -> Result<(), String> {
    check_kernel()?;
    let process = windows_process::open(std::process::id(), ProcessAccess::SafetyOnly)
        .map_err(|e| format!("Open worker CPU clock: {e:?}"))?;
    let started = Instant::now();
    let mut workers = CpuWorkers::start(count)?;
    let output = std::io::stdout();
    let mut output = output.lock();
    writeln!(output, "READY")
        .and_then(|_| output.flush())
        .map_err(|e| e.to_string())?;
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let parts: Vec<_> = line.split_whitespace().collect();
        match parts.as_slice() {
            ["PING", sequence] => {
                if started.elapsed() >= WORKER_LIFETIME {
                    return Err("Worker useful-work lifetime expired".to_owned());
                }
                let capture_started_ns =
                    u64::try_from(started.elapsed().as_nanos()).map_err(|e| e.to_string())?;
                let cpu_100ns = cpu_ticks(&process)?;
                let blocks = workers
                    .counters
                    .iter()
                    .map(|counter| counter.blocks.load(Ordering::Relaxed))
                    .collect();
                let checksums = workers
                    .counters
                    .iter()
                    .map(|counter| counter.checksum.load(Ordering::Relaxed))
                    .collect();
                let compute_cpu_100ns = workers
                    .threads
                    .iter()
                    .map(diagnostics::thread_cpu_ticks)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|code| format!("Compute-thread CPU time failed with error {code}"))?;
                let handler_cpu_100ns = diagnostics::current_thread_cpu_ticks()
                    .map_err(|code| format!("Handler CPU time failed with error {code}"))?;
                let capture_finished_ns =
                    u64::try_from(started.elapsed().as_nanos()).map_err(|e| e.to_string())?;
                let capture_span_ns = capture_finished_ns
                    .checked_sub(capture_started_ns)
                    .ok_or_else(|| "Worker capture clock went backwards".to_owned())?;
                let snapshot = Snapshot {
                    sequence: sequence.parse::<u64>().map_err(|e| e.to_string())?,
                    elapsed_ns: capture_started_ns + capture_span_ns / 2,
                    capture_span_ns,
                    cpu_100ns,
                    compute_cpu_100ns,
                    handler_cpu_100ns,
                    blocks,
                    checksums,
                };
                if started.elapsed() >= WORKER_LIFETIME {
                    return Err("Worker useful-work lifetime expired during snapshot".to_owned());
                }
                serde_json::to_writer(&mut output, &snapshot).map_err(|e| e.to_string())?;
                writeln!(output).map_err(|e| e.to_string())?;
            }
            ["JOB"] => {
                serde_json::to_writer(&mut output, &diagnostics::current_job_info())
                    .map_err(|e| e.to_string())?;
                writeln!(output).map_err(|e| e.to_string())?;
            }
            ["STOP"] => {
                workers.stop()?;
                writeln!(output, "STOPPED")
                    .and_then(|_| output.flush())
                    .map_err(|e| e.to_string())?;
                return Ok(());
            }
            _ => return Err("Invalid worker command".to_owned()),
        }
        output.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[inline(never)]
fn block(mut value: u64) -> u64 {
    // Same unsigned xorshift block as scripts/cpu_throughput_benchmark.ps1.
    for _ in 0..65_536 {
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
    }
    black_box(value)
}

fn check_kernel() -> Result<(), String> {
    if block(black_box(1)) == 3_937_720_911_837_777_011 {
        Ok(())
    } else {
        Err("Xorshift useful-work kernel verification failed".to_owned())
    }
}

#[cfg(test)]
#[path = "cpu_limiter/tests.rs"]
mod tests;
