use super::*;
use crate::{
    control::{
        cpu_allocation::{
            CpuAllocationApplyOutcome, CpuAllocationClaim, CpuAllocationCoordinator,
            CpuAllocationPlatform, CpuAllocationRequest, CpuSetInventory,
            WindowsCpuAllocationPlatform,
        },
        process::ControlOwner,
    },
    cpu_topology::CpuSelection,
    features::cpu_control::cpu_allocation::processor_topology,
    foreground::{capture_process_action_target, list_processes_with_paths},
};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "compiles and restarts only its owned disposable fixture; run in Windows integration QA"]
fn live_restart_preserves_context_and_rejects_stale_identity() -> Result<(), String> {
    restart_owned_fixture(false)
}

#[test]
#[ignore = "requires two available processors in group 0 and changes only its disposable fixture"]
fn live_restart_inherits_restored_adaptive_affinity() -> Result<(), String> {
    restart_owned_fixture(true)
}

#[test]
#[ignore = "requires two processors in group 0 and changes only its disposable process family"]
fn live_restore_child_affinity_without_releasing_parent() -> Result<(), String> {
    let executable = compile_fixture(
        "adaptive-child-affinity",
        r##"
#![windows_subsystem = "windows"]
use std::{io::{self, Read, Write}, process::{Command, Stdio}};

fn main() -> io::Result<()> {
    if std::env::args_os().len() > 1 {
        // The parent owns this pipe. Its exit releases the child even after a failed test.
        io::copy(&mut io::stdin().lock(), &mut io::sink())?;
        return Ok(());
    }
    let executable = std::env::current_exe()?;
    let mut children = Vec::new();
    for command in io::stdin().lock().bytes() {
        if command? != b'S' {
            break;
        }
        let child = Command::new(&executable)
            .arg("child")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()?;
        writeln!(io::stdout().lock(), "{}", child.id())?;
        children.push(child);
    }
    for mut child in children {
        drop(child.stdin.take());
        child.wait()?;
    }
    Ok(())
}
"##,
    )?;
    let mut parent = ChildGuard(
        Command::new(&executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| error.to_string())?,
    );
    let parent_target = capture_process_action_target(parent.0.id(), &executable, false)
        .map_err(|error| error.to_string())?;
    let stdout = parent
        .0
        .stdout
        .take()
        .ok_or("Fixture stdout unavailable.")?;
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if send.send(line).is_err() {
                break;
            }
        }
    });
    let stdin = parent
        .0
        .stdin
        .as_mut()
        .ok_or("Fixture stdin unavailable.")?;
    let mut spawn_child = || -> Result<ProcessActionTarget, String> {
        stdin.write_all(b"S").map_err(|error| error.to_string())?;
        let pid = receive
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| format!("Fixture did not report the child PID: {error}"))?
            .map_err(|error| error.to_string())?
            .parse::<u32>()
            .map_err(|error| error.to_string())?;
        capture_process_action_target(pid, &executable, false).map_err(|error| error.to_string())
    };

    let baseline = fixture_affinity(&parent_target)?;
    if baseline.count_ones() < 2 {
        return Err("The parent fixture requires two available processors in group 0.".into());
    }
    let narrowed = 1_usize << baseline.trailing_zeros();
    let early_child = spawn_child()?;
    let mut platform = WindowsCpuAllocationPlatform;
    let (_, early_child_handle) = platform
        .open(
            &ProcessControlTarget::from_action_target(&early_child),
            false,
        )
        .map_err(|error| error.to_string())?;
    platform
        .validate_group_zero_affinity(&early_child_handle)
        .map_err(|error| error.to_string())?;
    platform
        .apply_affinity(&early_child_handle, narrowed)
        .map_err(|error| error.to_string())?;

    let topology = processor_topology()?;
    let selection = CpuSelection::from_group_zero_mask(&topology, narrowed as u64)
        .map_err(|error| error.to_string())?;
    let mut coordinator = CpuAllocationCoordinator::default();
    coordinator.set_restore_child_affinity(true);
    let outcome = coordinator
        .apply_adaptive_episode_claim(
            &mut CpuSetInventory::default(),
            Ok(&topology),
            CpuAllocationClaim {
                target: ProcessControlTarget::from_action_target(&parent_target),
                owner: ControlOwner::AdaptiveEngine,
                request: CpuAllocationRequest::AffinityLimit { selection },
                balancing_generation: None,
            },
            false,
        )
        .map_err(|error| error.to_string())?;
    assert_eq!(outcome, CpuAllocationApplyOutcome::Applied);
    assert_eq!(fixture_affinity(&parent_target)?, narrowed);
    assert!(coordinator.has_adaptive_child_affinity_candidates());

    // Put child creation beyond the coarse Windows FILETIME tick of the verified write.
    thread::sleep(Duration::from_millis(32));
    let child = spawn_child()?;
    let excluded_child = spawn_child()?;
    assert_eq!(fixture_affinity(&child)?, narrowed);
    assert_eq!(fixture_affinity(&excluded_child)?, narrowed);

    let processes = list_processes_with_paths()?;
    for fixture in [&early_child, &child, &excluded_child] {
        assert!(processes.iter().any(|process| {
            process.id == fixture.id
                && process.creation_time == Some(fixture.creation_time)
                && process.parent_id == Some(parent_target.id)
        }));
    }
    let restored = coordinator.restore_adaptive_child_affinity(
        &processes,
        |process, _| process.id != excluded_child.id,
        false,
    );
    assert!(restored.failures.is_empty());
    assert_eq!(restored.restored_owners, [ControlOwner::AdaptiveEngine]);
    assert_eq!(fixture_affinity(&child)?, baseline);
    assert_eq!(fixture_affinity(&early_child)?, narrowed);
    assert_eq!(fixture_affinity(&excluded_child)?, narrowed);
    assert_eq!(fixture_affinity(&parent_target)?, narrowed);

    coordinator.shutdown()?;
    assert_eq!(fixture_affinity(&parent_target)?, baseline);
    assert_eq!(fixture_affinity(&child)?, baseline);
    Ok(())
}

fn restart_owned_fixture(restore_affinity: bool) -> Result<(), String> {
    let executable = compile_fixture(
        &format!("resource-thresholds-restart-{restore_affinity}"),
        r##"
#![windows_subsystem = "windows"]
use std::{fs::OpenOptions, io::Write, time::Duration};
fn main() {
    let executable = std::env::current_exe().unwrap();
    let mut file = OpenOptions::new().create(true).append(true).open(executable.with_extension("pids")).unwrap();
    writeln!(file, "{}|{}|{}|{}", std::process::id(), std::env::var("USERNAME").unwrap(), std::env::current_dir().unwrap().display(), std::env::args().count()).unwrap();
    std::thread::sleep(Duration::from_secs(15));
}
"##,
    )?;
    let directory = executable.parent().ok_or("Missing fixture directory.")?;
    let log = executable.with_extension("pids");
    // A previous QA run cannot supply identities for this run.
    if log.exists() {
        fs::remove_file(&log).map_err(|e| e.to_string())?;
    }
    let mut child = ChildGuard(
        Command::new(&executable)
            .current_dir(directory)
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    let wait_for_lines = |count: usize| -> Result<Vec<String>, String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(text) = fs::read_to_string(&log) {
                let lines: Vec<_> = text.lines().map(str::to_owned).collect();
                if lines.len() >= count && text.ends_with('\n') {
                    return Ok(lines);
                }
            }
            if Instant::now() >= deadline {
                return Err("Fixture did not report its launch.".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    };
    let before = wait_for_lines(1)?;
    let target = crate::foreground::capture_process_action_target(child.0.id(), &executable, false)
        .map_err(|e| e.to_string())?;
    let mut stale = target.clone();
    stale.creation_time = stale.creation_time.wrapping_add(1);
    let rejected = restart_app(vec![target.clone(), stale], false, |_| Ok(()));
    assert!(rejected.result.is_err());
    assert!(rejected.termination_handled.is_empty());
    assert!(child.0.try_wait().map_err(|e| e.to_string())?.is_none());

    let (_, process) =
        open_process_for_restart(&ProcessControlTarget::from_action_target(&target), false)
            .map_err(|error| error.to_string())?;
    let prepared = process_restart::prepare(&[process], &executable)?;
    thread::sleep(Duration::from_millis(50));
    assert_eq!(
        fs::read_to_string(&log)
            .map_err(|e| e.to_string())?
            .lines()
            .count(),
        1
    );
    drop(prepared);
    assert!(child.0.try_wait().map_err(|e| e.to_string())?.is_none());

    let mut other = ChildGuard(
        Command::new(&executable)
            .current_dir(directory)
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    wait_for_lines(2)?;
    let other_target =
        crate::foreground::capture_process_action_target(other.0.id(), &executable, false)
            .map_err(|e| e.to_string())?;
    let mut coordinator = CpuAllocationCoordinator::default();
    let original_affinity = if restore_affinity {
        let baseline = fixture_affinity(&target)?;
        if baseline.count_ones() < 2 {
            return Err("The restart fixture requires two available processors in group 0.".into());
        }
        let mask = 1_usize << baseline.trailing_zeros();
        let topology = processor_topology()?;
        let selection = CpuSelection::from_group_zero_mask(&topology, mask as u64)
            .map_err(|error| error.to_string())?;
        let outcome = coordinator
            .apply_adaptive_episode_claim(
                &mut CpuSetInventory::default(),
                Ok(&topology),
                CpuAllocationClaim {
                    target: ProcessControlTarget::from_action_target(&target),
                    owner: ControlOwner::AdaptiveEngine,
                    request: CpuAllocationRequest::AffinityLimit { selection },
                    balancing_generation: None,
                },
                false,
            )
            .map_err(|error| error.to_string())?;
        assert_eq!(outcome, CpuAllocationApplyOutcome::Applied);
        assert_eq!(fixture_affinity(&target)?, mask);
        Some(baseline)
    } else {
        None
    };
    let restarted = restart_app(vec![other_target, target], false, |parent| {
        if restore_affinity {
            assert_eq!(parent.id, child.0.id());
            if !coordinator.restore_adaptive_affinity_before_restart(parent, false)? {
                return Err(
                    "The fixture's Adaptive Engine processor affinity was not restored.".into(),
                );
            }
        }
        Ok(())
    });
    restarted.result?;
    assert_eq!(restarted.termination_handled.len(), 2);
    other.0.wait().map_err(|e| e.to_string())?;
    child.0.wait().map_err(|e| e.to_string())?;
    let after = wait_for_lines(3)?;
    assert_eq!(after.len(), 3);
    let first: Vec<_> = before[0].split('|').collect();
    let second: Vec<_> = after[2].split('|').collect();
    let replacement_id = second[0].parse::<u32>().map_err(|e| e.to_string())?;
    let replacement =
        crate::foreground::capture_process_action_target(replacement_id, &executable, false)
            .map_err(|e| e.to_string())?;
    let replacement_affinity =
        original_affinity.map(|baseline| (baseline, fixture_affinity(&replacement)));
    // Clean up only the verified replacement from this fixture before asserting its launch context.
    super::super::process_termination::ProcessTerminationController::default()
        .terminate_batch(vec![replacement], false)
        .into_termination_result()?;
    assert_ne!(first[0], second[0]);
    assert_eq!(first[1..], second[1..]);
    assert_eq!(second[3], "1");
    if let Some((baseline, actual)) = replacement_affinity {
        assert_eq!(actual?, baseline);
    }
    Ok(())
}

fn compile_fixture(name: &str, source_text: &str) -> Result<PathBuf, String> {
    let directory = std::env::current_dir()
        .map_err(|error| error.to_string())?
        .join("target")
        .join(format!("{name}-{}", std::process::id()));
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let source = directory.join("fixture.rs");
    let executable = directory.join("fixture.exe");
    fs::write(&source, source_text).map_err(|error| error.to_string())?;
    let compiled = Command::new("rustc")
        .arg("--edition=2021")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| error.to_string())?;
    if !compiled.status.success() {
        return Err(String::from_utf8_lossy(&compiled.stderr).into_owned());
    }
    Ok(executable)
}

fn fixture_affinity(target: &ProcessActionTarget) -> Result<usize, String> {
    let mut platform = WindowsCpuAllocationPlatform;
    let (_, process) = platform
        .open(&ProcessControlTarget::from_action_target(target), false)
        .map_err(|error| error.to_string())?;
    platform
        .validate_group_zero_affinity(&process)
        .map_err(|error| error.to_string())?;
    platform
        .query_affinity(&process)
        .map(|(affinity, _)| affinity)
        .map_err(|error| error.to_string())
}
