//! Opt-in subprocess fixtures. These modes never enter application startup or plan cleanup.

use std::{io::Read, os::windows::io::AsRawHandle};

use windows_sys::Win32::System::Threading::{
    BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
};

use super::*;
use crate::platform::windows::cpu_allocation::{cpu_set_inventory, CpuSetRecord};

const HELPER_ARGUMENT: &str = "--recovery-smoke-helper";
const PARENT_ARGUMENT: &str = "--recovery-smoke-parent";
const CPU_SETS_SCENARIO: &str = "cpu-sets-pending-applied";

pub(crate) fn run_if_requested() -> bool {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(mode) = arguments.first() else {
        return false;
    };
    if !mode.starts_with("--recovery-smoke") {
        return false;
    }
    let result = match (mode.as_str(), arguments.as_slice()) {
        (HELPER_ARGUMENT, [_, scenario, target]) => {
            parse_target(target).and_then(|target| run_helper(scenario, target))
        }
        (PARENT_ARGUMENT, [_, scenario, target]) => {
            parse_target(target).and_then(|target| run_parent(scenario, target))
        }
        _ => Err("Invalid recovery-smoke mode or arguments.".to_owned()),
    };
    if let Err(error) = result {
        eprintln!("Recovery smoke fixture failed: {error}");
        std::process::exit(2);
    }
    true
}

fn parse_target(value: &str) -> Result<ProcessIdentity, String> {
    serde_json::from_str(value).map_err(|error| format!("Invalid smoke target: {error}"))
}

fn fixture_process(target: &ProcessIdentity) -> Result<WinHandle, String> {
    let system_root =
        std::env::var_os("SystemRoot").ok_or_else(|| "SystemRoot is unavailable.".to_owned())?;
    let executable = Path::new(&system_root).join("System32").join("ping.exe");
    if target.id <= 4
        || target.creation_time == 0
        || !same_executable_path(Path::new(&target.executable_path), &executable)
    {
        return Err("Smoke target must be the exact disposable System32 ping process.".to_owned());
    }
    open_matching_process(target)?
        .ok_or_else(|| "The exact disposable smoke target has exited.".to_owned())
}

fn authorize(
    target: &ProcessIdentity,
    cpu_set_baseline: Option<u32>,
    command: &RecoveryCommand,
) -> Result<(), String> {
    let allowed = match command {
        RecoveryCommand::Begin {
            entry:
                RecoveryEntry::Process {
                    identity,
                    original: ProcessValue::PriorityClass(original),
                    expected: ProcessValue::PriorityClass(expected),
                },
            ..
        } => {
            cpu_set_baseline.is_none()
                && identity == target
                && permitted_priority(*original)
                && permitted_priority(*expected)
        }
        RecoveryCommand::Begin {
            entry:
                RecoveryEntry::Process {
                    identity,
                    original: ProcessValue::CpuSets(original),
                    expected: ProcessValue::CpuSets(expected),
                },
            ..
        } => {
            identity == target
                && cpu_set_baseline.is_some_and(|id| original.as_slice() == [id])
                && expected.is_empty()
        }
        RecoveryCommand::Commit { .. } | RecoveryCommand::Cancel { .. } => true,
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err("Smoke scope permits only the scenario's changes for its exact target.".to_owned())
    }
}

fn permitted_priority(value: u32) -> bool {
    [
        NORMAL_PRIORITY_CLASS,
        BELOW_NORMAL_PRIORITY_CLASS,
        IDLE_PRIORITY_CLASS,
    ]
    .contains(&value)
}

fn run_helper(scenario: &str, target: ProcessIdentity) -> Result<(), String> {
    let process = fixture_process(&target)?;
    let cpu_set_baseline = if scenario == CPU_SETS_SCENARIO {
        let ProcessValue::CpuSets(ids) =
            query_process_value(process.raw(), &ProcessValue::CpuSets(Vec::new()))?
        else {
            return Err("Smoke target CPU Sets could not be read.".to_owned());
        };
        let [id] = ids.as_slice() else {
            return Err("Smoke target must have exactly one CPU Set baseline.".to_owned());
        };
        Some(*id)
    } else {
        None
    };
    run_watchdog_loop(
        BufReader::new(std::io::stdin()),
        std::io::stdout(),
        |command| authorize(&target, cpu_set_baseline, command),
        || Ok(()),
    )
}

struct Helper {
    child: Child,
    transport: Option<RecoveryTransport>,
}

impl Drop for Helper {
    fn drop(&mut self) {
        drop(self.transport.take());
        drop(self.child.stdin.take());
        drop(self.child.stdout.take());
        if wait_for_watchdog(&mut self.child, Duration::from_secs(5)).is_err() {
            if let Err(error) = stop_owned_helper(&mut self.child) {
                eprintln!("Smoke helper cleanup failed: {error}");
            }
        }
    }
}

fn stop_owned_helper(child: &mut Child) -> Result<(), String> {
    if child
        .try_wait()
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Ok(());
    }
    let kill_error = child.kill().err();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Owned helper did not exit after termination: {kill_error:?}"
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn report(value: serde_json::Value) -> Result<(), String> {
    let mut output = std::io::stdout();
    writeln!(output, "{value}")
        .and_then(|()| output.flush())
        .map_err(|error| format!("Could not report smoke fixture state: {error}"))
}

fn run_parent(scenario: &str, target: ProcessIdentity) -> Result<(), String> {
    if ![
        "committed",
        "pending-applied",
        "pending-unapplied",
        CPU_SETS_SCENARIO,
    ]
    .contains(&scenario)
    {
        return Err("Unknown recovery-smoke scenario.".to_owned());
    }
    let process = fixture_process(&target)?;
    if query_process_value(process.raw(), &ProcessValue::PriorityClass(0))?
        != ProcessValue::PriorityClass(NORMAL_PRIORITY_CLASS)
    {
        return Err("Disposable smoke process must start at Normal priority.".to_owned());
    }
    let cpu_set_baseline = if scenario == CPU_SETS_SCENARIO {
        let id = cpu_set_inventory()
            .map_err(|error| format!("Smoke CPU Set inventory failed: {error:?}."))?
            .into_iter()
            .find_map(|record| match record {
                CpuSetRecord::Unallocated { id, .. } => Some(id),
                CpuSetRecord::Allocated { .. } => None,
            })
            .ok_or_else(|| "Smoke test requires one unallocated CPU Set.".to_owned())?;
        report(serde_json::json!({"state": "cpu_set_setup", "id": id}))?;
        let mut byte = [0];
        std::io::stdin()
            .read_exact(&mut byte)
            .map_err(|error| format!("Smoke CPU Set setup was not acknowledged: {error}"))?;
        if byte != *b"S" {
            return Err("Invalid smoke CPU Set setup acknowledgment.".to_owned());
        }
        Some(id)
    } else {
        None
    };
    let serialized_target = serde_json::to_string(&target).map_err(|error| error.to_string())?;
    let child = Command::new(std::env::current_exe().map_err(|error| error.to_string())?)
        .args([HELPER_ARGUMENT, scenario, &serialized_target])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| format!("Could not start smoke helper: {error}"))?;
    let mut helper = Helper {
        child,
        transport: None,
    };
    let helper_identity = process_identity(helper.child.as_raw_handle().cast())?;
    report(serde_json::json!({"state": "spawned", "helper": helper_identity}))?;
    let stdin = helper
        .child
        .stdin
        .take()
        .ok_or_else(|| "Smoke helper stdin is unavailable.".to_owned())?;
    let stdout = helper
        .child
        .stdout
        .take()
        .ok_or_else(|| "Smoke helper stdout is unavailable.".to_owned())?;
    helper.transport = Some(RecoveryTransport::start(
        stdin,
        BufReader::new(stdout),
        Duration::from_secs(2),
    )?);
    let transport = helper
        .transport
        .as_mut()
        .ok_or_else(|| "Smoke helper transport is unavailable.".to_owned())?;
    let rejected = verify_scope_rejections(transport, &target, cpu_set_baseline)?;
    let expected_priority = if let Some(id) = cpu_set_baseline {
        transition(
            transport,
            &process,
            &target,
            1,
            (
                ProcessValue::CpuSets(vec![id]),
                ProcessValue::CpuSets(Vec::new()),
            ),
            true,
            false,
        )?;
        NORMAL_PRIORITY_CLASS
    } else {
        transition(
            transport,
            &process,
            &target,
            1,
            (
                ProcessValue::PriorityClass(NORMAL_PRIORITY_CLASS),
                ProcessValue::PriorityClass(BELOW_NORMAL_PRIORITY_CLASS),
            ),
            true,
            true,
        )?;
        transition(
            transport,
            &process,
            &target,
            2,
            (
                ProcessValue::PriorityClass(BELOW_NORMAL_PRIORITY_CLASS),
                ProcessValue::PriorityClass(IDLE_PRIORITY_CLASS),
            ),
            scenario != "pending-unapplied",
            scenario == "committed",
        )?;
        if scenario == "pending-unapplied" {
            BELOW_NORMAL_PRIORITY_CLASS
        } else {
            IDLE_PRIORITY_CLASS
        }
    };
    let current = query_process_value(process.raw(), &ProcessValue::PriorityClass(0))?;
    if current != ProcessValue::PriorityClass(expected_priority) {
        return Err("Smoke target did not reach its verified pre-recovery state.".to_owned());
    }
    report(serde_json::json!({
        "state": "ready",
        "target": target.id,
        "priority": expected_priority,
        "rejected": rejected,
    }))?;
    let mut byte = [0_u8; 1];
    if std::io::stdin()
        .read(&mut byte)
        .map_err(|error| error.to_string())?
        != 0
    {
        return Err("Close fixture-parent stdin to request clean EOF.".to_owned());
    }
    drop(helper.transport.take());
    wait_for_watchdog(&mut helper.child, Duration::from_secs(5))
}

fn transition(
    transport: &mut RecoveryTransport,
    process: &WinHandle,
    target: &ProcessIdentity,
    id: u64,
    values: (ProcessValue, ProcessValue),
    apply: bool,
    commit: bool,
) -> Result<(), String> {
    let (original, expected) = values;
    transport.send(&RecoveryCommand::Begin {
        id,
        entry: RecoveryEntry::Process {
            identity: target.clone(),
            original,
            expected: expected.clone(),
        },
    })?;
    if apply {
        apply_process_value(process.raw(), &expected)?;
        if query_process_value(process.raw(), &expected)? != expected {
            return Err("Native smoke process value verification failed.".to_owned());
        }
    }
    if commit {
        transport.send(&RecoveryCommand::Commit { id })?;
    }
    Ok(())
}

fn verify_scope_rejections(
    transport: &mut RecoveryTransport,
    target: &ProcessIdentity,
    cpu_set_baseline: Option<u32>,
) -> Result<usize, String> {
    let mut stale = target.clone();
    stale.creation_time = stale.creation_time.wrapping_add(1);
    let entries = [
        RecoveryEntry::PowerPlan {
            original_guid: "forbidden".to_owned(),
            expected_guid: "forbidden".to_owned(),
        },
        RecoveryEntry::SuspendedJob {
            name: "forbidden".to_owned(),
            process: target.clone(),
        },
        RecoveryEntry::ThreadPriority {
            process: target.clone(),
            thread_id: 0,
            thread_creation_time: 0,
            original: 0,
            expected: 0,
        },
        RecoveryEntry::Process {
            identity: target.clone(),
            original: ProcessValue::Affinity(0),
            expected: ProcessValue::Affinity(0),
        },
        RecoveryEntry::Process {
            identity: stale.clone(),
            original: ProcessValue::PriorityClass(NORMAL_PRIORITY_CLASS),
            expected: ProcessValue::PriorityClass(IDLE_PRIORITY_CLASS),
        },
        RecoveryEntry::Process {
            identity: target.clone(),
            original: ProcessValue::PriorityClass(NORMAL_PRIORITY_CLASS),
            expected: ProcessValue::PriorityClass(HIGH_PRIORITY_CLASS),
        },
    ];
    let cpu_set_entries = cpu_set_baseline.into_iter().flat_map(|id| {
        let singleton = ProcessValue::CpuSets(vec![id]);
        let cleared = ProcessValue::CpuSets(Vec::new());
        [
            RecoveryEntry::Process {
                identity: stale.clone(),
                original: singleton.clone(),
                expected: cleared.clone(),
            },
            RecoveryEntry::Process {
                identity: target.clone(),
                original: singleton.clone(),
                expected: singleton,
            },
            RecoveryEntry::Process {
                identity: target.clone(),
                original: cleared.clone(),
                expected: cleared,
            },
        ]
    });
    let mut rejected = 0;
    for entry in entries.into_iter().chain(cpu_set_entries) {
        let id = 100 + rejected as u64;
        match transport.send(&RecoveryCommand::Begin { id, entry }) {
            Err(error) if error.contains("Smoke scope permits only") => rejected += 1,
            other => {
                let _ = transport.send(&RecoveryCommand::Cancel { id });
                return Err(format!(
                    "Smoke helper accepted or mishandled an out-of-scope command: {other:?}"
                ));
            }
        }
    }
    Ok(rejected)
}
