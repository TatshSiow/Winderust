use super::*;
use crate::platform::windows::thread_suspension;
use std::{
    io::Cursor,
    os::windows::io::AsRawHandle,
    process::{Child, Command, Stdio},
};

use windows_sys::Win32::{
    Foundation::{LocalFree, ERROR_ACCESS_DENIED, INVALID_HANDLE_VALUE, WAIT_OBJECT_0},
    Security::{
        Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SetKernelObjectSecurity, DACL_SECURITY_INFORMATION,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
            QueryInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        },
        Threading::{
            CreateEventW, CreateProcessW, ResumeThread, WaitForSingleObject,
            BELOW_NORMAL_PRIORITY_CLASS, CREATE_SUSPENDED, IDLE_PRIORITY_CLASS,
            PROCESS_INFORMATION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            PROCESS_QUERY_INFORMATION, STARTUPINFOW, THREAD_PRIORITY_BELOW_NORMAL,
            THREAD_PRIORITY_LOWEST,
        },
    },
};

#[test]
fn recovery_helper_is_rejected_by_every_process_control_acquisition() {
    use crate::control::process::{self, ProcessControlError, ProcessControlTarget};
    // Holding the journal lock also verifies protection never tries to re-lock it.
    let _runtime = RUNTIME.lock().unwrap();
    struct RestoreId(u32);
    impl Drop for RestoreId {
        fn drop(&mut self) {
            WATCHDOG_PROCESS_ID.store(self.0, Ordering::Relaxed);
        }
    }
    let _restore = RestoreId(WATCHDOG_PROCESS_ID.swap(u32::MAX, Ordering::Relaxed));
    let target = ProcessControlTarget::automatic(
        u32::MAX,
        "renamed-helper.exe".into(),
        r"C:/Apps/renamed-helper.exe".into(),
        1,
    );
    assert!(is_watchdog_process(target.id));
    assert!(!is_watchdog_process(0));
    assert!(!is_watchdog_process(std::process::id()));
    for acquire in [
        process::open_process_for_set_information,
        process::open_process_for_thread_control,
        process::open_process_for_thread_snapshot,
        process::open_process_for_working_set_trim,
        process::open_process_for_termination,
        process::open_process_for_job_assignment,
    ] {
        for cross_session in [false, true] {
            assert!(matches!(
                acquire(&target, cross_session),
                Err(ProcessControlError::AccessDenied(_))
            ));
        }
    }
}

struct FrozenJobCleanup {
    job: WinHandle,
}

impl Drop for FrozenJobCleanup {
    fn drop(&mut self) {
        let mut thaw = JobObjectFreezeInformation::new(false);
        // SAFETY: the test owns this live Job Object handle and thaw has the exact buffer
        // layout required by the same contract exercised by the test.
        let _ = unsafe {
            SetInformationJobObject(
                self.job.raw(),
                JOB_OBJECT_FREEZE_INFORMATION_CLASS,
                (&mut thaw as *mut JobObjectFreezeInformation).cast(),
                std::mem::size_of::<JobObjectFreezeInformation>() as u32,
            )
        };
        // SAFETY: the test owns this disposable Job Object and terminates only its test tree.
        let _ = unsafe { TerminateJobObject(self.job.raw(), 1) };
    }
}

fn resume_disposable_root(process: &WinHandle, thread: &WinHandle) {
    // SAFETY: both handles belong to the suspended disposable process created by the test.
    let _ = unsafe { ResumeThread(thread.raw()) };
    // SAFETY: process is a live disposable handle; this bounded wait retains no pointer.
    let _ = unsafe { WaitForSingleObject(process.raw(), 5_000) };
}

struct DisposableChild {
    child: Child,
}

impl DisposableChild {
    fn spawn() -> Result<Self, String> {
        let system_root = std::env::var_os("SystemRoot")
            .ok_or_else(|| "SystemRoot is unavailable.".to_owned())?;
        let executable = Path::new(&system_root).join("System32").join("ping.exe");
        let child = Command::new(executable)
            .args(["127.0.0.1", "-n", "120", "-w", "1000"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Failed to start disposable test process: {error}"))?;
        Ok(Self { child })
    }

    fn process_handle(&self) -> Result<WinHandle, String> {
        // SAFETY: child.id identifies the disposable process created by this test.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_INFORMATION
                    | PROCESS_QUERY_LIMITED_INFORMATION
                    | PROCESS_SET_INFORMATION,
                0,
                self.child.id(),
            )
        };
        (!handle.is_null())
            .then(|| WinHandle::new(handle))
            .ok_or_else(|| {
                format!(
                    "OpenProcess(test child) failed with error {}.",
                    last_error()
                )
            })
    }
}

impl Drop for DisposableChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn test_runtime(mut child: Child) -> Result<RecoveryRuntime, String> {
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "The recovery helper stdin pipe is unavailable.".to_owned())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "The recovery helper stdout pipe is unavailable.".to_owned())?;
    Ok(RecoveryRuntime {
        child,
        transport: RecoveryTransport::start(stdin, BufReader::new(stdout), Duration::from_secs(2))?,
        entries: Vec::new(),
        next_intent_id: 1,
    })
}

fn spawn_test_helper(command: &str, args: &[&str]) -> Result<Child, String> {
    Command::new("cmd")
        .arg("/C")
        .arg(format!("{command} {}", args.join(" ")))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Failed to spawn test recovery helper: {error}"))
}

fn test_thread_handle(process_id: u32) -> Result<WinHandle, String> {
    // SAFETY: a thread snapshot takes no borrowed pointers and returns an owned handle.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(format!(
            "CreateToolhelp32Snapshot failed with error {}.",
            last_error()
        ));
    }
    let snapshot = WinHandle::new(snapshot);
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    // SAFETY: snapshot is live and entry has its required initialized size.
    let mut present = unsafe { Thread32First(snapshot.raw(), &mut entry) } != 0;
    while present {
        if entry.th32OwnerProcessID == process_id {
            // SAFETY: the thread ID came from the current system snapshot.
            let thread = unsafe {
                OpenThread(
                    THREAD_QUERY_INFORMATION | THREAD_SET_INFORMATION,
                    0,
                    entry.th32ThreadID,
                )
            };
            if !thread.is_null() {
                return Ok(WinHandle::new(thread));
            }
        }
        // SAFETY: snapshot remains live and entry remains writable for the next record.
        present = unsafe { Thread32Next(snapshot.raw(), &mut entry) } != 0;
    }
    Err("No queryable thread was found for the disposable test process.".to_owned())
}

fn identity() -> ProcessIdentity {
    ProcessIdentity {
        id: 7,
        creation_time: 11,
        executable_path: "C:\\app.exe".to_owned(),
    }
}

fn captured_test_thread(process: &WinHandle) -> Result<thread_suspension::CapturedThread, String> {
    thread_suspension::capture_threads(process.raw())
        .map_err(|error| format!("{error:?}"))?
        .into_iter()
        .next()
        .ok_or_else(|| "No thread was captured for the disposable test process.".to_owned())
}

#[test]
fn recovery_client_finish_is_idempotent_without_an_active_runtime() {
    let mut client = RecoveryClient { active: false };
    assert!(client.finish().is_ok());
    assert!(client.finish().is_ok());
}

#[test]
fn recovery_runtime_finish_accepts_successful_helper_exit() -> Result<(), String> {
    let runtime = test_runtime(spawn_test_helper("exit", &["0"])?)?;
    finish_recovery_runtime(runtime)
}

#[test]
fn recovery_runtime_finish_reports_helper_exit_code() -> Result<(), String> {
    let helper = spawn_test_helper("exit", &["11"])?;
    let result = finish_recovery_runtime(test_runtime(helper)?);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("Crash recovery helper exited unexpectedly with status code 11."));
    Ok(())
}

#[test]
fn deferred_power_intent_resolves_the_exact_id_only_after_acknowledgement() -> Result<(), String> {
    for applied in [false, true] {
        let mut runtime = test_runtime(spawn_test_helper("exit", &["0"])?)?;
        let baseline = RecoveryEntry::PowerPlan {
            original_guid: "original".to_owned(),
            expected_guid: "first".to_owned(),
        };
        runtime.entries.push(baseline.clone());
        let pending_entry = RecoveryEntry::PowerPlan {
            original_guid: "first".to_owned(),
            expected_guid: "second".to_owned(),
        };
        let mut deferred = DeferredRecoveryIntent {
            id: 42,
            entry: Some(pending_entry.clone()),
        };
        let (requests, incoming) = mpsc::channel::<RecoveryRequest>();
        runtime.transport = RecoveryTransport {
            requests,
            failed: None,
            timeout: Duration::from_secs(1),
        };
        let helper_baseline = baseline.clone();
        let helper = std::thread::spawn(move || {
            let mut entries = vec![helper_baseline];
            let mut pending = vec![(42, pending_entry)];
            let mut jobs = HashMap::new();
            for rejected in [true, false] {
                let (command, response) = incoming.recv().unwrap();
                assert_eq!(
                    command,
                    if applied {
                        RecoveryCommand::Commit { id: 42 }
                    } else {
                        RecoveryCommand::Cancel { id: 42 }
                    }
                );
                let result = if rejected {
                    Err(RecoveryTransportError::Rejected(
                        "injected rejection".to_owned(),
                    ))
                } else {
                    apply_watchdog_command(command, &mut entries, &mut pending, &mut jobs)
                        .map_err(RecoveryTransportError::Rejected)
                };
                response.send(result).unwrap();
            }
            let current = if applied { "second" } else { "first" };
            for command in [
                RecoveryCommand::Begin {
                    id: 43,
                    entry: RecoveryEntry::PowerPlan {
                        original_guid: current.to_owned(),
                        expected_guid: "third".to_owned(),
                    },
                },
                RecoveryCommand::Commit { id: 43 },
            ] {
                apply_watchdog_command(command, &mut entries, &mut pending, &mut jobs).unwrap();
            }
            let committed_len = append_pending_intents(&mut entries, pending);
            assert_eq!(
                unwind_power_plan_guid("power_plan", "third", &entries, committed_len),
                "original"
            );
        });
        assert!(deferred
            .resolve_with_runtime(&mut runtime, applied)
            .is_err());
        assert!(deferred.entry.is_some());
        assert_eq!(runtime.entries, [baseline]);
        deferred.resolve_with_runtime(&mut runtime, applied)?;
        assert!(deferred.entry.is_none());
        let current = if applied { "second" } else { "first" };
        assert_eq!(
            unwind_power_plan_guid(
                "power_plan",
                current,
                &runtime.entries,
                runtime.entries.len()
            ),
            "original"
        );
        helper.join().unwrap();
        finish_recovery_runtime(runtime)?;
    }
    Ok(())
}

#[test]
fn rejected_generic_commit_blocks_later_mutations_and_preserves_eof_recovery() -> Result<(), String>
{
    let mut runtime = test_runtime(spawn_test_helper("exit", &["0"])?)?;
    let baseline = RecoveryEntry::PowerPlan {
        original_guid: "original".to_owned(),
        expected_guid: "first".to_owned(),
    };
    let transition = RecoveryEntry::PowerPlan {
        original_guid: "first".to_owned(),
        expected_guid: "second".to_owned(),
    };
    runtime.entries.push(baseline.clone());
    let (requests, incoming) = mpsc::channel::<RecoveryRequest>();
    runtime.transport = RecoveryTransport {
        requests,
        failed: None,
        timeout: Duration::from_secs(1),
    };
    let helper_transition = transition.clone();
    let helper = std::thread::spawn(move || {
        let (command, response) = incoming.recv().unwrap();
        assert_eq!(command, RecoveryCommand::Commit { id: 42 });
        response
            .send(Err(RecoveryTransportError::Rejected(
                "injected Commit rejection".to_owned(),
            )))
            .unwrap();
        assert!(matches!(
            incoming.recv_timeout(Duration::from_secs(1)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        let mut entries = vec![baseline];
        let committed_len = append_pending_intents(&mut entries, vec![(42, helper_transition)]);
        assert_eq!(
            unwind_power_plan_guid("power_plan", "second", &entries, committed_len),
            "original"
        );
    });
    let commit_error = commit_recovery_entry(&mut runtime, 42, transition).unwrap_err();
    assert!(commit_error.contains("further recovery commands are blocked"));
    let later_error = runtime
        .transport
        .send(&RecoveryCommand::Begin {
            id: 43,
            entry: RecoveryEntry::PowerPlan {
                original_guid: "second".to_owned(),
                expected_guid: "third".to_owned(),
            },
        })
        .unwrap_err();
    assert_eq!(later_error, commit_error);
    assert_eq!(
        unwind_power_plan_guid(
            "power_plan",
            "second",
            &runtime.entries,
            runtime.entries.len()
        ),
        "original"
    );
    finish_recovery_runtime(runtime)?;
    helper.join().unwrap();
    Ok(())
}

#[test]
fn retaining_uncertain_compensation_keeps_pending_edges_and_blocks_new_mutations(
) -> Result<(), String> {
    let mut runtime = test_runtime(spawn_test_helper("exit", &["0"])?)?;
    let baseline = RecoveryEntry::Process {
        identity: identity(),
        original: ProcessValue::PriorityClass(1),
        expected: ProcessValue::PriorityClass(2),
    };
    let uncertain = RecoveryEntry::Process {
        identity: identity(),
        original: ProcessValue::PriorityClass(2),
        expected: ProcessValue::PriorityClass(3),
    };
    let key = baseline.key();
    runtime.entries.push(baseline.clone());
    retain_pending_recovery(&mut runtime);
    assert_eq!(runtime.entries.as_slice(), std::slice::from_ref(&baseline));
    assert!(runtime
        .transport
        .send(&RecoveryCommand::Begin {
            id: 43,
            entry: uncertain.clone(),
        })
        .unwrap_err()
        .contains("intent remains pending"));
    let mut helper_entries = vec![baseline];
    let committed_len = append_pending_intents(&mut helper_entries, vec![(42, uncertain)]);
    for current in [2, 3] {
        assert_eq!(
            unwind_process_value(
                &key,
                &ProcessValue::PriorityClass(current),
                &helper_entries,
                committed_len
            ),
            ProcessValue::PriorityClass(1)
        );
    }
    assert_eq!(
        unwind_process_value(
            &key,
            &ProcessValue::PriorityClass(4),
            &helper_entries,
            committed_len
        ),
        ProcessValue::PriorityClass(4)
    );
    finish_recovery_runtime(runtime)
}

#[test]
fn power_plan_replay_preserves_an_external_selection_before_the_final_setter() {
    let entries = [RecoveryEntry::PowerPlan {
        original_guid: "original".to_owned(),
        expected_guid: "managed".to_owned(),
    }];
    let mut queries = [Ok("managed".to_owned()), Ok("external".to_owned())].into_iter();
    recover_power_plan_key_with(
        "power_plan",
        &entries,
        1,
        || queries.next().unwrap(),
        |_| panic!("an external selection must not be overwritten"),
    )
    .unwrap();
}

#[test]
fn power_plan_replay_verifies_restoration_and_preserves_failures_for_retry() {
    let entries = [RecoveryEntry::PowerPlan {
        original_guid: "original".to_owned(),
        expected_guid: "managed".to_owned(),
    }];
    for readback in [
        Ok("original".to_owned()),
        Ok("managed".to_owned()),
        Err("readback failed".to_owned()),
    ] {
        let success = matches!(&readback, Ok(guid) if guid == "original");
        let mut queries =
            [Ok("managed".to_owned()), Ok("managed".to_owned()), readback].into_iter();
        let mut applied = Vec::new();
        let result = recover_power_plan_key_with(
            "power_plan",
            &entries,
            1,
            || queries.next().unwrap(),
            |guid| {
                applied.push(guid.to_owned());
                Ok(())
            },
        );
        assert_eq!(result.is_ok(), success);
        assert_eq!(applied, ["original"]);
    }
    for queries in [
        vec![Err("initial query failed".to_owned())],
        vec![
            Ok("managed".to_owned()),
            Err("final query failed".to_owned()),
        ],
    ] {
        let mut queries = queries.into_iter();
        assert!(recover_power_plan_key_with(
            "power_plan",
            &entries,
            1,
            || queries.next().unwrap(),
            |_| panic!("a failed ownership query must block mutation"),
        )
        .is_err());
    }
}

#[test]
fn stale_adaptive_cleanup_waits_until_the_older_journal_baseline_is_recovered() {
    let current = std::cell::RefCell::new("adaptive");
    let recovery_calls = std::cell::Cell::new(0);
    let cleanup_calls = std::cell::Cell::new(0);
    recover_with_retry_using(
        || {
            let calls = recovery_calls.get() + 1;
            recovery_calls.set(calls);
            assert_eq!(*current.borrow(), "adaptive");
            if calls == 1 {
                return Err("injected journal query failure".to_owned());
            }
            *current.borrow_mut() = "original";
            Ok(())
        },
        || {
            cleanup_calls.set(cleanup_calls.get() + 1);
            assert_eq!(*current.borrow(), "original");
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(recovery_calls.get(), 2);
    assert_eq!(cleanup_calls.get(), 1);
    assert_eq!(*current.borrow(), "original");
    let cleanup_calls = std::cell::Cell::new(0);
    assert!(recover_with_retry_using(
        || Err("journal remains unavailable".to_owned()),
        || {
            cleanup_calls.set(cleanup_calls.get() + 1);
            Ok(())
        },
    )
    .is_err());
    assert_eq!(cleanup_calls.get(), 0);
}

#[test]
fn external_state_break_starts_a_new_recovery_segment() {
    let mut entries = Vec::new();
    compact_or_push(
        &mut entries,
        RecoveryEntry::Process {
            identity: identity(),
            original: ProcessValue::PriorityClass(1),
            expected: ProcessValue::PriorityClass(2),
        },
    );
    compact_or_push(
        &mut entries,
        RecoveryEntry::Process {
            identity: identity(),
            original: ProcessValue::PriorityClass(4),
            expected: ProcessValue::PriorityClass(3),
        },
    );
    assert_eq!(entries.len(), 2);
    let key = entries[0].key();
    assert_eq!(
        unwind_process_value(
            &key,
            &ProcessValue::PriorityClass(3),
            &entries,
            entries.len()
        ),
        ProcessValue::PriorityClass(4)
    );
    assert_eq!(
        unwind_process_value(
            &key,
            &ProcessValue::PriorityClass(2),
            &entries,
            entries.len()
        ),
        ProcessValue::PriorityClass(2)
    );
}

#[test]
fn every_process_value_kind_compacts_and_returns_to_its_baseline() {
    let cases = [
        (
            ProcessValue::PriorityClass(1),
            ProcessValue::PriorityClass(2),
            ProcessValue::PriorityClass(3),
        ),
        (
            ProcessValue::PowerThrottling {
                version: 1,
                control_mask: 1,
                state_mask: 0,
            },
            ProcessValue::PowerThrottling {
                version: 1,
                control_mask: 1,
                state_mask: 1,
            },
            ProcessValue::PowerThrottling {
                version: 1,
                control_mask: 3,
                state_mask: 1,
            },
        ),
        (
            ProcessValue::Affinity(1),
            ProcessValue::Affinity(3),
            ProcessValue::Affinity(7),
        ),
        (
            ProcessValue::CpuSets(vec![]),
            ProcessValue::CpuSets(vec![1]),
            ProcessValue::CpuSets(vec![1, 2]),
        ),
        (
            ProcessValue::DynamicPriorityBoostDisabled(false),
            ProcessValue::DynamicPriorityBoostDisabled(true),
            ProcessValue::DynamicPriorityBoostDisabled(true),
        ),
        (
            ProcessValue::IoPriority(0),
            ProcessValue::IoPriority(1),
            ProcessValue::IoPriority(2),
        ),
        (
            ProcessValue::GpuPriority(0),
            ProcessValue::GpuPriority(1),
            ProcessValue::GpuPriority(2),
        ),
        (
            ProcessValue::MemoryPriority(1),
            ProcessValue::MemoryPriority(2),
            ProcessValue::MemoryPriority(3),
        ),
    ];

    for (baseline, middle, final_value) in cases {
        let mut entries = Vec::new();
        compact_or_push(
            &mut entries,
            RecoveryEntry::Process {
                identity: identity(),
                original: baseline.clone(),
                expected: middle.clone(),
            },
        );
        compact_or_push(
            &mut entries,
            RecoveryEntry::Process {
                identity: identity(),
                original: middle,
                expected: final_value.clone(),
            },
        );
        assert_eq!(
            entries,
            vec![RecoveryEntry::Process {
                identity: identity(),
                original: baseline.clone(),
                expected: final_value.clone(),
            }]
        );
        compact_or_push(
            &mut entries,
            RecoveryEntry::Process {
                identity: identity(),
                original: final_value,
                expected: baseline,
            },
        );
        assert!(entries.is_empty());
    }
}

#[test]
fn thread_and_power_plan_changes_compact_to_the_original_baseline() {
    let mut thread_entries = Vec::new();
    compact_or_push(
        &mut thread_entries,
        RecoveryEntry::ThreadPriority {
            process: identity(),
            thread_id: 13,
            thread_creation_time: 17,
            original: 0,
            expected: 1,
        },
    );
    compact_or_push(
        &mut thread_entries,
        RecoveryEntry::ThreadPriority {
            process: identity(),
            thread_id: 13,
            thread_creation_time: 17,
            original: 1,
            expected: 2,
        },
    );
    assert!(matches!(
        thread_entries.as_slice(),
        [RecoveryEntry::ThreadPriority {
            original: 0,
            expected: 2,
            ..
        }]
    ));
    compact_or_push(
        &mut thread_entries,
        RecoveryEntry::ThreadPriority {
            process: identity(),
            thread_id: 13,
            thread_creation_time: 17,
            original: 2,
            expected: 0,
        },
    );
    assert!(thread_entries.is_empty());

    let mut plan_entries = Vec::new();
    compact_or_push(
        &mut plan_entries,
        RecoveryEntry::PowerPlan {
            original_guid: "plan-a".to_owned(),
            expected_guid: "plan-b".to_owned(),
        },
    );
    compact_or_push(
        &mut plan_entries,
        RecoveryEntry::PowerPlan {
            original_guid: "PLAN-B".to_owned(),
            expected_guid: "plan-c".to_owned(),
        },
    );
    assert!(matches!(
        plan_entries.as_slice(),
        [RecoveryEntry::PowerPlan {
            original_guid,
            expected_guid,
        }] if original_guid == "plan-a" && expected_guid == "plan-c"
    ));
    compact_or_push(
        &mut plan_entries,
        RecoveryEntry::PowerPlan {
            original_guid: "PLAN-C".to_owned(),
            expected_guid: "PLAN-A".to_owned(),
        },
    );
    assert!(plan_entries.is_empty());
}

#[test]
fn thread_suspension_resume_gate_requires_exactly_one_owned_count() {
    assert!(should_resume_thread_suspension(4, 5, 5));
    assert!(!should_resume_thread_suspension(4, 5, 4));
    assert!(!should_resume_thread_suspension(4, 5, 6));
    assert!(!should_resume_thread_suspension(4, 6, 6));
    assert!(!should_resume_thread_suspension(u16::MAX, 0, 0));
}

#[test]
fn thread_suspension_begin_is_pending_before_mutation_commit() {
    let entry = RecoveryEntry::ThreadSuspension {
        process: identity(),
        thread_id: 13,
        thread_creation_time: 17,
        original_suspend_count: 0,
        expected_suspend_count: 1,
    };
    let mut entries = Vec::new();
    let mut pending = Vec::new();
    let mut jobs = HashMap::new();

    apply_watchdog_command_with_open_job(
        RecoveryCommand::Begin {
            id: 1,
            entry: entry.clone(),
        },
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| Err("thread suspension must not open jobs".to_owned()),
    )
    .unwrap();
    assert!(entries.is_empty());
    assert_eq!(pending, vec![(1, entry.clone())]);

    apply_watchdog_command_with_open_job(
        RecoveryCommand::Commit { id: 1 },
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| Err("commit must not open jobs".to_owned()),
    )
    .unwrap();
    assert_eq!(entries, vec![entry]);
    assert!(pending.is_empty());
}

#[test]
fn record_thread_suspension_rejects_suspend_count_overflow_before_handle_queries() {
    match record_thread_suspension(std::ptr::null_mut(), std::ptr::null_mut(), u16::MAX) {
        Ok(_) => panic!("overflow must be rejected before either handle is queried"),
        Err(error) => assert_eq!(error, "Thread suspend count cannot exceed 65535."),
    }
}

#[test]
fn pending_intents_preserve_committed_baselines_at_eof() {
    for (kind, pending_value) in (0..3).flat_map(|kind| [1, 3].map(|value| (kind, value))) {
        let mut entries = Vec::new();
        let mut pending = Vec::new();
        let mut jobs = HashMap::new();
        let entry = |original: u32, expected: u32| match kind {
            0 => RecoveryEntry::Process {
                identity: identity(),
                original: ProcessValue::PriorityClass(original),
                expected: ProcessValue::PriorityClass(expected),
            },
            1 => RecoveryEntry::ThreadPriority {
                process: identity(),
                thread_id: 7,
                thread_creation_time: 8,
                original: original as i32,
                expected: expected as i32,
            },
            _ => RecoveryEntry::PowerPlan {
                original_guid: original.to_string(),
                expected_guid: expected.to_string(),
            },
        };
        for command in [
            RecoveryCommand::Begin {
                id: 1,
                entry: entry(1, 2),
            },
            RecoveryCommand::Commit { id: 1 },
            RecoveryCommand::Begin {
                id: 2,
                entry: entry(2, pending_value),
            },
        ] {
            apply_watchdog_command(command, &mut entries, &mut pending, &mut jobs).unwrap();
        }
        let committed_len = append_pending_intents(&mut entries, pending);
        let key = entries[0].key();
        for (actual, expected) in [(2, 1), (pending_value, 1), (4, 4)] {
            match kind {
                0 => assert_eq!(
                    unwind_process_value(
                        &key,
                        &ProcessValue::PriorityClass(actual),
                        &entries,
                        committed_len
                    ),
                    ProcessValue::PriorityClass(expected)
                ),
                1 => assert_eq!(
                    unwind_thread_priority(&key, actual as i32, &entries, committed_len),
                    expected as i32
                ),
                _ => assert_eq!(
                    unwind_power_plan_guid(&key, &actual.to_string(), &entries, committed_len),
                    expected.to_string()
                ),
            }
        }
    }
}

#[test]
fn watchdog_commands_cover_begin_cancel_commit_replacement_and_clean_release() {
    let entry = RecoveryEntry::Process {
        identity: identity(),
        original: ProcessValue::PriorityClass(1),
        expected: ProcessValue::PriorityClass(2),
    };
    let mut entries = Vec::new();
    let mut pending = Vec::new();
    let mut jobs = HashMap::new();

    apply_watchdog_command_with_open_job(
        RecoveryCommand::Begin {
            id: 1,
            entry: entry.clone(),
        },
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| Err("process entries must not open jobs".to_owned()),
    )
    .unwrap();
    assert_eq!(pending, vec![(1, entry.clone())]);
    apply_watchdog_command_with_open_job(
        RecoveryCommand::Cancel { id: 1 },
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| Err("cancel must not open jobs".to_owned()),
    )
    .unwrap();
    assert!(pending.is_empty());
    assert!(entries.is_empty());

    apply_watchdog_command_with_open_job(
        RecoveryCommand::Begin {
            id: 2,
            entry: entry.clone(),
        },
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| Err("process entries must not open jobs".to_owned()),
    )
    .unwrap();
    apply_watchdog_command_with_open_job(
        RecoveryCommand::Commit { id: 2 },
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| Err("commit must not open jobs".to_owned()),
    )
    .unwrap();
    assert_eq!(entries.len(), 1);

    let release = RecoveryEntry::Process {
        identity: identity(),
        original: ProcessValue::PriorityClass(2),
        expected: ProcessValue::PriorityClass(1),
    };
    apply_watchdog_command_with_open_job(
        RecoveryCommand::Begin {
            id: 3,
            entry: release,
        },
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| Err("process entries must not open jobs".to_owned()),
    )
    .unwrap();
    apply_watchdog_command_with_open_job(
        RecoveryCommand::Commit { id: 3 },
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| Err("commit must not open jobs".to_owned()),
    )
    .unwrap();
    assert!(entries.is_empty());
    assert!(pending.is_empty());
}

#[test]
fn suspended_job_begin_requires_a_retained_handle_before_acknowledgement() {
    let entry = RecoveryEntry::SuspendedJob {
        name: "Local\\Winderust.Suspend.test".to_owned(),
        process: identity(),
    };
    let mut entries = Vec::new();
    let mut pending = Vec::new();
    let mut jobs = HashMap::new();

    let error = apply_watchdog_command_with_open_job(
        RecoveryCommand::Begin {
            id: 1,
            entry: entry.clone(),
        },
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| Err("job open failed".to_owned()),
    )
    .unwrap_err();
    assert_eq!(error, "job open failed");
    assert!(pending.is_empty());
    assert!(jobs.is_empty());

    apply_watchdog_command_with_open_job(
        RecoveryCommand::Begin {
            id: 2,
            entry: entry.clone(),
        },
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| {
            // SAFETY: null security attributes/name are allowed; the returned owned event
            // handle is used only as a deterministic stand-in for the retained Job handle.
            let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
            (!handle.is_null())
                .then(|| WinHandle::new(handle))
                .ok_or_else(|| "CreateEventW failed".to_owned())
        },
    )
    .unwrap();
    assert_eq!(pending, vec![(2, entry)]);
    assert!(jobs.contains_key("Local\\Winderust.Suspend.test"));
    apply_watchdog_command_with_open_job(
        RecoveryCommand::Commit { id: 2 },
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| Err("commit must not open jobs".to_owned()),
    )
    .unwrap();
    assert_eq!(entries.len(), 1);
    apply_watchdog_command_with_open_job(
        RecoveryCommand::ForgetJob {
            name: "Local\\Winderust.Suspend.test".to_owned(),
        },
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| Err("forget must not open jobs".to_owned()),
    )
    .unwrap();
    assert!(entries.is_empty());
    assert!(jobs.is_empty());
}

#[test]
fn forget_power_plan_removes_committed_and_pending_edges_without_other_ownership() {
    let property = RecoveryEntry::Process {
        identity: identity(),
        original: ProcessValue::PriorityClass(1),
        expected: ProcessValue::PriorityClass(2),
    };
    let old_plan = RecoveryEntry::PowerPlan {
        original_guid: "original".into(),
        expected_guid: "managed".into(),
    };
    let mut entries = vec![old_plan.clone(), property.clone()];
    let mut pending = vec![(1, old_plan), (2, property.clone())];
    let mut jobs = HashMap::new();

    apply_watchdog_command_with_open_job(
        RecoveryCommand::ForgetPowerPlan,
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| Err("forget power plan must not open jobs".into()),
    )
    .unwrap();

    assert_eq!(entries, vec![property.clone()]);
    assert_eq!(pending, vec![(2, property)]);
    let committed_len = append_pending_intents(&mut entries, pending);
    assert_eq!(
        unwind_power_plan_guid("power_plan", "managed", &entries, committed_len),
        "managed"
    );
}

#[test]
fn forget_process_removes_only_the_exact_instance_property() {
    let process = identity();
    let boost = RecoveryEntry::Process {
        identity: process.clone(),
        original: ProcessValue::DynamicPriorityBoostDisabled(false),
        expected: ProcessValue::DynamicPriorityBoostDisabled(true),
    };
    let priority = RecoveryEntry::Process {
        identity: process.clone(),
        original: ProcessValue::PriorityClass(1),
        expected: ProcessValue::PriorityClass(2),
    };
    let mut replacement = process.clone();
    replacement.creation_time += 1;
    let replacement_boost = RecoveryEntry::Process {
        identity: replacement,
        original: ProcessValue::DynamicPriorityBoostDisabled(false),
        expected: ProcessValue::DynamicPriorityBoostDisabled(true),
    };
    let mut entries = vec![boost.clone(), priority.clone()];
    let mut pending = vec![(7, boost), (8, replacement_boost.clone())];
    let mut jobs = HashMap::new();

    apply_watchdog_command_with_open_job(
        RecoveryCommand::ForgetProcess {
            process_id: process.id,
            creation_time: process.creation_time,
            value: ProcessValue::DynamicPriorityBoostDisabled(false),
        },
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| Err("forget process must not open jobs".to_owned()),
    )
    .unwrap();

    assert_eq!(entries, vec![priority]);
    assert_eq!(pending, vec![(8, replacement_boost)]);
}

#[test]
fn forget_thread_priority_removes_only_the_exact_thread_instance() {
    let process = identity();
    let target = RecoveryEntry::ThreadPriority {
        process: process.clone(),
        thread_id: 13,
        thread_creation_time: 17,
        original: 0,
        expected: 1,
    };
    let replacement_thread = RecoveryEntry::ThreadPriority {
        process: process.clone(),
        thread_id: 13,
        thread_creation_time: 18,
        original: 0,
        expected: 1,
    };
    let other_thread = RecoveryEntry::ThreadPriority {
        process: process.clone(),
        thread_id: 14,
        thread_creation_time: 17,
        original: 0,
        expected: 1,
    };
    let mut entries = vec![target.clone(), replacement_thread.clone()];
    let mut pending = vec![(7, target), (8, other_thread.clone())];
    let mut jobs = HashMap::new();

    apply_watchdog_command_with_open_job(
        RecoveryCommand::ForgetThreadPriority {
            process_id: process.id,
            process_creation_time: process.creation_time,
            thread_id: 13,
            thread_creation_time: 17,
        },
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| Err("forget thread priority must not open jobs".to_owned()),
    )
    .unwrap();

    assert_eq!(entries, vec![replacement_thread]);
    assert_eq!(pending, vec![(8, other_thread)]);
}

#[test]
fn forget_thread_suspension_preserves_reused_thread_and_thread_priority() {
    let process = identity();
    let target = RecoveryEntry::ThreadSuspension {
        process: process.clone(),
        thread_id: 13,
        thread_creation_time: 17,
        original_suspend_count: 0,
        expected_suspend_count: 1,
    };
    let reused = RecoveryEntry::ThreadSuspension {
        process: process.clone(),
        thread_id: 13,
        thread_creation_time: 18,
        original_suspend_count: 0,
        expected_suspend_count: 1,
    };
    let priority = RecoveryEntry::ThreadPriority {
        process: process.clone(),
        thread_id: 13,
        thread_creation_time: 17,
        original: 0,
        expected: 1,
    };
    let mut entries = vec![target.clone(), reused.clone(), priority.clone()];
    let mut pending = vec![(7, target), (8, reused.clone()), (9, priority.clone())];
    let mut jobs = HashMap::new();

    apply_watchdog_command_with_open_job(
        RecoveryCommand::ForgetThreadSuspension {
            process_id: process.id,
            process_creation_time: process.creation_time,
            thread_id: 13,
            thread_creation_time: 17,
        },
        &mut entries,
        &mut pending,
        &mut jobs,
        |_, _| Err("forget thread suspension must not open jobs".to_owned()),
    )
    .unwrap();

    assert_eq!(entries, vec![reused.clone(), priority.clone()]);
    assert_eq!(pending, vec![(8, reused), (9, priority)]);
}

#[test]
fn live_nonreplying_watchdog_cannot_block_transport_or_shutdown() {
    let mut child = DisposableChild {
        child: Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap(),
    };
    let mut transport = RecoveryTransport::start(
        child.child.stdin.take().unwrap(),
        BufReader::new(child.child.stdout.take().unwrap()),
        Duration::from_millis(50),
    )
    .unwrap();
    let started = Instant::now();
    let command = RecoveryCommand::Cancel { id: 1 };
    let error = transport.send(&command).unwrap_err();
    assert!(error.contains("outcome is unknown"));
    assert_eq!(transport.send(&command).unwrap_err(), error);
    assert!(child.child.try_wait().unwrap().is_none());
    drop(transport);
    assert!(wait_for_watchdog(&mut child.child, Duration::from_millis(50)).is_err());
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(child.child.try_wait().unwrap().is_none());
    // Only this disposable test helper is terminated by DisposableChild's Drop.
}

#[test]
fn blocked_write_times_out_and_late_ack_cannot_acknowledge_another_command() {
    struct GatedWriter {
        gate: Option<mpsc::Receiver<()>>,
        writes: std::sync::Arc<Mutex<Vec<u8>>>,
        dropped: mpsc::Sender<()>,
    }
    impl Write for GatedWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if let Some(gate) = self.gate.take() {
                gate.recv().map_err(std::io::Error::other)?;
            }
            self.writes.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Drop for GatedWriter {
        fn drop(&mut self) {
            let _ = self.dropped.send(());
        }
    }
    let (release, gate) = mpsc::channel();
    let (dropped, closed) = mpsc::channel();
    let writes = std::sync::Arc::new(Mutex::new(Vec::new()));
    let mut transport = RecoveryTransport::start(
        GatedWriter {
            gate: Some(gate),
            writes: writes.clone(),
            dropped,
        },
        Cursor::new(b"ok\n"),
        Duration::from_millis(50),
    )
    .unwrap();
    let command = RecoveryCommand::Cancel { id: 9 };
    assert!(transport
        .send(&command)
        .unwrap_err()
        .contains("outcome is unknown"));
    release.send(()).unwrap();
    assert!(transport.send(&RecoveryCommand::Cancel { id: 10 }).is_err());
    assert!(closed.recv_timeout(Duration::from_millis(50)).is_err());
    drop(transport);
    closed.recv_timeout(Duration::from_secs(3)).unwrap();
    let written = writes.lock().unwrap();
    assert_eq!(
        serde_json::from_slice::<RecoveryCommand>(&written).unwrap(),
        command
    );
}

#[test]
fn explicit_watchdog_rejection_does_not_poison_a_synchronized_transport() {
    let mut transport = RecoveryTransport::start(
        Vec::new(),
        Cursor::new(b"error:rejected\nok\n"),
        Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(
        transport
            .send(&RecoveryCommand::Cancel { id: 1 })
            .unwrap_err(),
        "rejected"
    );
    assert!(transport.send(&RecoveryCommand::Cancel { id: 2 }).is_ok());
}

#[test]
fn recovery_transport_requires_an_acknowledgement_before_returning_success() {
    let command = RecoveryCommand::Begin {
        id: 9,
        entry: RecoveryEntry::Process {
            identity: identity(),
            original: ProcessValue::PriorityClass(1),
            expected: ProcessValue::PriorityClass(2),
        },
    };
    let mut written = Vec::new();
    let mut ok = Cursor::new(b"ok\n");
    send_command(&mut written, &mut ok, &command).unwrap();
    let encoded = String::from_utf8(written).unwrap();
    assert!(encoded.ends_with('\n'));
    assert_eq!(
        serde_json::from_str::<RecoveryCommand>(encoded.trim()).unwrap(),
        command
    );

    let mut written = Vec::new();
    let mut rejected = Cursor::new(b"error:watchdog rejected Begin\n");
    assert!(
        matches!(send_command(&mut written, &mut rejected, &command), Err(RecoveryTransportError::Rejected(error)) if error == "watchdog rejected Begin")
    );

    let mut written = Vec::new();
    let mut invalid = Cursor::new(b"maybe\n");
    assert!(
        matches!(send_command(&mut written, &mut invalid, &command), Err(RecoveryTransportError::Uncertain(error)) if error == "Invalid recovery watchdog response: maybe")
    );
}

#[test]
fn failed_clean_process_release_keeps_journal_for_helper_recovery() -> Result<(), String> {
    let child = DisposableChild::spawn()?;
    let process = child.process_handle()?;
    let original = query_process_value(process.raw(), &ProcessValue::PriorityClass(0))?;
    let original_priority = match original {
        ProcessValue::PriorityClass(priority) => priority,
        _ => unreachable!("priority query must produce a priority value"),
    };
    let expected_priority = if original_priority != BELOW_NORMAL_PRIORITY_CLASS {
        BELOW_NORMAL_PRIORITY_CLASS
    } else {
        IDLE_PRIORITY_CLASS
    };
    let expected = ProcessValue::PriorityClass(expected_priority);
    let original = ProcessValue::PriorityClass(original_priority);
    let entry = RecoveryEntry::Process {
        identity: process_identity(process.raw())?,
        original: original.clone(),
        expected: expected.clone(),
    };

    apply_process_value(process.raw(), &expected)?;
    assert_eq!(
        query_process_value(process.raw(), &expected)?,
        expected,
        "the disposable process must reach the journal's expected state"
    );

    let mut entries = Vec::new();
    let mut pending = Vec::new();
    let mut jobs = HashMap::new();
    apply_watchdog_command(
        RecoveryCommand::Begin {
            id: 1,
            entry: entry.clone(),
        },
        &mut entries,
        &mut pending,
        &mut jobs,
    )?;
    apply_watchdog_command(
        RecoveryCommand::Commit { id: 1 },
        &mut entries,
        &mut pending,
        &mut jobs,
    )?;

    let failed_release = RecoveryEntry::Process {
        identity: process_identity(process.raw())?,
        original: expected.clone(),
        expected: original.clone(),
    };
    apply_watchdog_command(
        RecoveryCommand::Begin {
            id: 2,
            entry: failed_release,
        },
        &mut entries,
        &mut pending,
        &mut jobs,
    )?;
    // A clean restore that fails drops its uncommitted intent, which sends Cancel. The
    // earlier committed mutation must remain available to the helper when its pipe closes.
    apply_watchdog_command(
        RecoveryCommand::Cancel { id: 2 },
        &mut entries,
        &mut pending,
        &mut jobs,
    )?;
    assert_eq!(entries, vec![entry]);
    assert!(pending.is_empty());

    recover_journal(&entries)?;
    assert_eq!(
        query_process_value(process.raw(), &expected)?,
        original,
        "recovery must restore the captured process baseline"
    );
    Ok(())
}

#[test]
fn power_throttling_recovery_restores_a_disposable_process() -> Result<(), String> {
    let child = DisposableChild::spawn()?;
    let process = child.process_handle()?;
    let probe = ProcessValue::PowerThrottling {
        version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        control_mask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        state_mask: 0,
    };
    let original = query_process_value(process.raw(), &probe)?;
    let ProcessValue::PowerThrottling {
        version,
        control_mask,
        state_mask,
    } = original
    else {
        unreachable!("power-throttling query must produce a power value")
    };
    let expected = ProcessValue::PowerThrottling {
        version,
        control_mask: control_mask | PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        state_mask: state_mask ^ PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    };
    let original = ProcessValue::PowerThrottling {
        version,
        control_mask,
        state_mask,
    };
    let entry = RecoveryEntry::Process {
        identity: process_identity(process.raw())?,
        original: original.clone(),
        expected: expected.clone(),
    };

    apply_process_value(process.raw(), &expected)?;
    assert_eq!(query_process_value(process.raw(), &expected)?, expected);
    recover_journal(&[entry])?;
    assert_eq!(query_process_value(process.raw(), &original)?, original);
    Ok(())
}

#[test]
fn dynamic_priority_boost_recovery_restores_a_disposable_process() -> Result<(), String> {
    let child = DisposableChild::spawn()?;
    let process = child.process_handle()?;
    let probe = ProcessValue::DynamicPriorityBoostDisabled(false);
    let original = query_process_value(process.raw(), &probe)?;
    let ProcessValue::DynamicPriorityBoostDisabled(original_disabled) = original else {
        unreachable!("dynamic priority boost query must produce a boost value");
    };
    let original = ProcessValue::DynamicPriorityBoostDisabled(original_disabled);
    let expected = ProcessValue::DynamicPriorityBoostDisabled(!original_disabled);
    let entry = RecoveryEntry::Process {
        identity: process_identity(process.raw())?,
        original: original.clone(),
        expected: expected.clone(),
    };

    apply_process_value(process.raw(), &expected)?;
    assert_eq!(query_process_value(process.raw(), &expected)?, expected);

    recover_journal(&[entry])?;

    assert_eq!(query_process_value(process.raw(), &original)?, original);
    Ok(())
}

#[test]
#[allow(unknown_lints, clippy::manual_isolate_lowest_one)]
fn affinity_recovery_restores_a_disposable_process() -> Result<(), String> {
    let child = DisposableChild::spawn()?;
    let process = child.process_handle()?;
    let probe = ProcessValue::Affinity(0);
    let original = query_process_value(process.raw(), &probe)?;
    let ProcessValue::Affinity(original_mask) = original else {
        unreachable!("affinity query must produce an affinity value");
    };
    if original_mask.count_ones() <= 1 {
        return Ok(());
    }
    let expected = ProcessValue::Affinity(original_mask & original_mask.wrapping_neg());
    let original = ProcessValue::Affinity(original_mask);
    let entry = RecoveryEntry::Process {
        identity: process_identity(process.raw())?,
        original: original.clone(),
        expected: expected.clone(),
    };

    apply_process_value(process.raw(), &expected)?;
    let changed = query_process_value(process.raw(), &expected);
    let recovery_result = recover_journal(&[entry]);
    let observed = query_process_value(process.raw(), &original);
    let cleanup_result = match &observed {
        Ok(value) if value == &original => Ok(()),
        _ => apply_process_value(process.raw(), &original),
    };
    recovery_result?;
    let observed = observed?;
    cleanup_result?;
    let changed = changed?;
    if changed != expected {
        return Err(format!(
            "Processor Affinity setup returned {changed:?}, expected {expected:?}."
        ));
    }
    if observed != original {
        return Err(format!(
            "Processor Affinity recovery returned {observed:?}, expected {original:?}."
        ));
    }
    Ok(())
}

#[test]
fn cpu_sets_recovery_restores_a_disposable_process() -> Result<(), String> {
    let child = DisposableChild::spawn()?;
    let process = child.process_handle()?;
    let probe = ProcessValue::CpuSets(Vec::new());
    let original = query_process_value(process.raw(), &probe)?;
    let ProcessValue::CpuSets(mut original_ids) = original else {
        unreachable!("CPU Sets query must produce CPU Set IDs");
    };
    original_ids.sort_unstable();
    original_ids.dedup();
    let inventory = crate::platform::windows::cpu_allocation::cpu_set_inventory()
        .map_err(|error| format!("Recovery fixture CPU Set inventory failed: {error:?}."))?;
    let expected_ids = (0..64).find_map(|bit| {
        let mut ids = inventory
            .iter()
            .filter(|(index, _)| *index == bit)
            .map(|(_, id)| *id)
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        (!ids.is_empty() && ids != original_ids).then_some(ids)
    });
    let Some(expected_ids) = expected_ids else {
        return Ok(());
    };
    let original = ProcessValue::CpuSets(original_ids);
    let expected = ProcessValue::CpuSets(expected_ids);
    let entry = RecoveryEntry::Process {
        identity: process_identity(process.raw())?,
        original: original.clone(),
        expected: expected.clone(),
    };

    apply_process_value(process.raw(), &expected)?;
    let changed = query_process_value(process.raw(), &expected);
    let recovery_result = recover_journal(&[entry]);
    let observed = query_process_value(process.raw(), &original);
    let cleanup_result = match &observed {
        Ok(value) if value == &original => Ok(()),
        _ => apply_process_value(process.raw(), &original),
    };
    recovery_result?;
    let observed = observed?;
    cleanup_result?;
    let changed = changed?;
    if changed != expected {
        return Err(format!(
            "CPU Sets setup returned {changed:?}, expected {expected:?}."
        ));
    }
    if observed != original {
        return Err(format!(
            "CPU Sets recovery returned {observed:?}, expected {original:?}."
        ));
    }
    Ok(())
}

#[test]
fn io_priority_recovery_restores_a_disposable_process() -> Result<(), String> {
    let child = DisposableChild::spawn()?;
    let process = child.process_handle()?;
    let probe = ProcessValue::IoPriority(0);
    let original = query_process_value(process.raw(), &probe)?;
    let ProcessValue::IoPriority(original_priority) = original else {
        unreachable!("I/O priority query must produce an I/O priority value");
    };
    let original = ProcessValue::IoPriority(original_priority);
    let expected = ProcessValue::IoPriority(if original_priority == 0 { 1 } else { 0 });
    let entry = RecoveryEntry::Process {
        identity: process_identity(process.raw())?,
        original: original.clone(),
        expected: expected.clone(),
    };

    apply_process_value(process.raw(), &expected)?;
    assert_eq!(query_process_value(process.raw(), &expected)?, expected);

    recover_journal(&[entry])?;

    assert_eq!(query_process_value(process.raw(), &original)?, original);
    Ok(())
}

#[test]
#[ignore = "modifies and restores the explicit WINDERUST_GPU_TEST_PID target; run in integration QA"]
fn gpu_priority_recovery_restores_an_explicit_gpu_process() -> Result<(), String> {
    let process_id = std::env::var("WINDERUST_GPU_TEST_PID")
        .map_err(|_| "Set WINDERUST_GPU_TEST_PID to a disposable GPU-using process.".to_owned())?
        .parse::<u32>()
        .map_err(|error| format!("WINDERUST_GPU_TEST_PID is invalid: {error}"))?;
    let executable_path = std::env::var_os("WINDERUST_GPU_TEST_PATH").ok_or_else(|| {
        "Set WINDERUST_GPU_TEST_PATH to that process's absolute executable path.".to_owned()
    })?;
    let target = crate::foreground::capture_process_action_target(
        process_id,
        Path::new(&executable_path),
        true,
    )
    .map_err(|error| error.to_string())?;
    // SAFETY: target is an exact, validated live process instance captured for this test.
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
            0,
            target.id,
        )
    };
    if handle.is_null() {
        return Err(format!(
            "OpenProcess(GPU test target) failed with error {}.",
            last_error()
        ));
    }
    let process = WinHandle::new(handle);
    let probe = ProcessValue::GpuPriority(0);
    let original = query_process_value(process.raw(), &probe)?;
    let ProcessValue::GpuPriority(original_priority) = original else {
        unreachable!("GPU priority query must produce a GPU priority value");
    };
    let original = ProcessValue::GpuPriority(original_priority);
    let expected = ProcessValue::GpuPriority(if original_priority == 0 { 1 } else { 0 });
    let entry = RecoveryEntry::Process {
        identity: process_identity(process.raw())?,
        original: original.clone(),
        expected: expected.clone(),
    };

    apply_process_value(process.raw(), &expected)?;
    let changed = query_process_value(process.raw(), &expected);
    let recovery_result = recover_journal(&[entry]);
    let observed = query_process_value(process.raw(), &original);
    let cleanup_result = match &observed {
        Ok(value) if value == &original => Ok(()),
        _ => apply_process_value(process.raw(), &original),
    };
    recovery_result?;
    let observed = observed?;
    cleanup_result?;
    let changed = changed?;
    if changed != expected {
        return Err(format!(
            "GPU priority setup returned {changed:?}, expected {expected:?}."
        ));
    }
    if observed != original {
        return Err(format!(
            "GPU priority recovery returned {observed:?}, expected {original:?}."
        ));
    }
    Ok(())
}

#[test]
#[ignore = "modifies and restores a disposable Windows process; run in explicit integration QA"]
fn memory_priority_recovery_restores_an_explicit_process() -> Result<(), String> {
    let child = DisposableChild::spawn()?;
    let process = child.process_handle()?;
    let probe = ProcessValue::MemoryPriority(0);
    let original = query_process_value(process.raw(), &probe)?;
    let ProcessValue::MemoryPriority(original_priority) = original else {
        unreachable!("Memory Priority query must produce a Memory Priority value");
    };
    let original = ProcessValue::MemoryPriority(original_priority);
    let expected = ProcessValue::MemoryPriority(if original_priority == 1 { 2 } else { 1 });
    let entry = RecoveryEntry::Process {
        identity: process_identity(process.raw())?,
        original: original.clone(),
        expected: expected.clone(),
    };

    apply_process_value(process.raw(), &expected)?;
    let changed = query_process_value(process.raw(), &expected);
    let recovery_result = recover_journal(&[entry]);
    let observed = query_process_value(process.raw(), &original);
    let cleanup_result = match &observed {
        Ok(value) if value == &original => Ok(()),
        _ => apply_process_value(process.raw(), &original),
    };
    recovery_result?;
    let observed = observed?;
    cleanup_result?;
    let changed = changed?;
    if changed != expected {
        return Err(format!(
            "Memory Priority setup returned {changed:?}, expected {expected:?}."
        ));
    }
    if observed != original {
        return Err(format!(
            "Memory Priority recovery returned {observed:?}, expected {original:?}."
        ));
    }
    Ok(())
}

#[test]
fn thread_priority_recovery_restores_without_process_set_information() -> Result<(), String> {
    let child = DisposableChild::spawn()?;
    let descriptor_text = crate::win_util::wide_null(&format!(
        "D:(D;;0x{PROCESS_SET_INFORMATION:x};;;WD)(A;;0x{PROCESS_QUERY_LIMITED_INFORMATION:x};;;WD)"
    ));
    let mut descriptor = null_mut();
    // SAFETY: descriptor_text is terminated SDDL and descriptor is a writable allocation output.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor_text.as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        )
    } == 0
    {
        return Err(format!(
            "Test DACL conversion failed with error {}.",
            last_error()
        ));
    }
    // SAFETY: Child retains its original owned handle with WRITE_DAC; only this disposable
    // process is changed, and its threads retain their independent access permissions.
    let changed = unsafe {
        SetKernelObjectSecurity(
            child.child.as_raw_handle(),
            DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    let change_error = (changed == 0).then(last_error);
    // SAFETY: successful SDDL conversion allocated descriptor, which is no longer used.
    unsafe { LocalFree(descriptor) };
    if let Some(error) = change_error {
        return Err(format!(
            "Test process DACL change failed with error {error}."
        ));
    }
    // SAFETY: the test owns this child's PID and requests only the right denied above.
    let denied = unsafe { OpenProcess(PROCESS_SET_INFORMATION, 0, child.child.id()) };
    if !denied.is_null() {
        let _unexpected_access = WinHandle::new(denied);
        return Err("The test process DACL did not deny PROCESS_SET_INFORMATION.".to_owned());
    }
    assert_eq!(last_error(), ERROR_ACCESS_DENIED);
    let identity = process_identity(child.child.as_raw_handle())?;
    let target = crate::control::process::ProcessControlTarget::automatic(
        identity.id,
        "ping.exe".to_owned(),
        identity.executable_path.clone().into(),
        identity.creation_time,
    );
    let (_, process) = crate::control::process::open_process_for_thread_control(&target, true)
        .map_err(|error| format!("Thread Priority acquisition failed: {error:?}"))?;
    let thread = test_thread_handle(child.child.id())?;
    // SAFETY: thread is a live, queryable thread owned by the disposable test process.
    let original = unsafe { GetThreadPriority(thread.raw()) };
    if original == THREAD_PRIORITY_ERROR_RETURN {
        return Err(format!(
            "GetThreadPriority failed with error {}.",
            last_error()
        ));
    }
    let expected = if original != THREAD_PRIORITY_BELOW_NORMAL {
        THREAD_PRIORITY_BELOW_NORMAL
    } else {
        THREAD_PRIORITY_LOWEST
    };
    let entry = RecoveryEntry::ThreadPriority {
        process: process_identity(process.raw())?,
        thread_id: thread_id(thread.raw())?,
        thread_creation_time: thread_creation_time(thread.raw())?,
        original,
        expected,
    };
    let mut entries = Vec::new();
    let mut pending = Vec::new();
    let mut jobs = HashMap::new();
    apply_watchdog_command(
        RecoveryCommand::Begin {
            id: 1,
            entry: entry.clone(),
        },
        &mut entries,
        &mut pending,
        &mut jobs,
    )?;
    assert_eq!(pending, [(1, entry)]);

    // SAFETY: thread is a live, queryable thread owned by the disposable test process.
    if unsafe { SetThreadPriority(thread.raw(), expected) } == 0 {
        return Err(format!(
            "SetThreadPriority failed with error {}.",
            last_error()
        ));
    }
    // SAFETY: thread remains live and is still owned by the disposable process.
    assert_eq!(unsafe { GetThreadPriority(thread.raw()) }, expected);
    apply_watchdog_command(
        RecoveryCommand::Commit { id: 1 },
        &mut entries,
        &mut pending,
        &mut jobs,
    )?;
    assert!(pending.is_empty());
    recover_journal(&entries)?;
    // SAFETY: thread remains live and is still owned by the disposable process.
    assert_eq!(unsafe { GetThreadPriority(thread.raw()) }, original);
    Ok(())
}

#[test]
fn thread_suspension_recovery_resumes_exact_owned_count_once() -> Result<(), String> {
    let child = DisposableChild::spawn()?;
    let process = child.process_handle()?;
    let captured = captured_test_thread(&process)?;
    let thread = thread_suspension::open_exact_thread(child.child.id(), captured)
        .map_err(|error| format!("{error:?}"))?;
    let original = captured.suspend_count;
    let expected = original
        .checked_add(1)
        .ok_or_else(|| "Disposable thread suspend count overflowed.".to_owned())?;
    assert_eq!(
        thread_suspension::suspend_once(&thread).map_err(|error| format!("{error:?}"))?,
        u32::from(original)
    );
    let entry = RecoveryEntry::ThreadSuspension {
        process: process_identity(process.raw())?,
        thread_id: captured.id,
        thread_creation_time: captured.creation_time,
        original_suspend_count: original,
        expected_suspend_count: expected,
    };

    recover_journal(&[entry])?;

    assert_eq!(
        thread_suspension::exact_suspend_count(process.raw(), captured.id, captured.creation_time,)
            .map_err(|error| format!("{error:?}"))?,
        original
    );
    Ok(())
}

#[test]
fn thread_suspension_recovery_leaves_identity_mismatch_unchanged() -> Result<(), String> {
    let child = DisposableChild::spawn()?;
    let process = child.process_handle()?;
    let captured = captured_test_thread(&process)?;
    let thread = thread_suspension::open_exact_thread(child.child.id(), captured)
        .map_err(|error| format!("{error:?}"))?;
    let original = captured.suspend_count;
    let expected = original
        .checked_add(1)
        .ok_or_else(|| "Disposable thread suspend count overflowed.".to_owned())?;
    thread_suspension::suspend_once(&thread).map_err(|error| format!("{error:?}"))?;

    recover_journal(&[RecoveryEntry::ThreadSuspension {
        process: process_identity(process.raw())?,
        thread_id: captured.id,
        thread_creation_time: captured.creation_time.wrapping_add(1),
        original_suspend_count: original,
        expected_suspend_count: expected,
    }])?;
    assert_eq!(
        thread_suspension::exact_suspend_count(process.raw(), captured.id, captured.creation_time,)
            .map_err(|error| format!("{error:?}"))?,
        expected
    );
    Ok(())
}

#[test]
fn thread_suspension_recovery_leaves_conflicting_count_unchanged() -> Result<(), String> {
    let child = DisposableChild::spawn()?;
    let process = child.process_handle()?;
    let captured = captured_test_thread(&process)?;
    let thread = thread_suspension::open_exact_thread(child.child.id(), captured)
        .map_err(|error| format!("{error:?}"))?;
    let original = captured.suspend_count;
    let expected = original
        .checked_add(1)
        .ok_or_else(|| "Disposable thread suspend count overflowed.".to_owned())?;
    let conflicting = original
        .checked_add(2)
        .ok_or_else(|| "Disposable thread suspend count cannot be raised twice.".to_owned())?;
    thread_suspension::suspend_once(&thread).map_err(|error| format!("{error:?}"))?;
    thread_suspension::suspend_once(&thread).map_err(|error| format!("{error:?}"))?;
    recover_journal(&[RecoveryEntry::ThreadSuspension {
        process: process_identity(process.raw())?,
        thread_id: captured.id,
        thread_creation_time: captured.creation_time,
        original_suspend_count: original,
        expected_suspend_count: expected,
    }])?;
    assert_eq!(
        thread_suspension::exact_suspend_count(process.raw(), captured.id, captured.creation_time,)
            .map_err(|error| format!("{error:?}"))?,
        conflicting
    );
    Ok(())
}

#[test]
fn thread_suspension_recovery_ignores_an_exited_process() -> Result<(), String> {
    let entry = {
        let child = DisposableChild::spawn()?;
        let process = child.process_handle()?;
        let captured = captured_test_thread(&process)?;
        let original = captured.suspend_count;
        RecoveryEntry::ThreadSuspension {
            process: process_identity(process.raw())?,
            thread_id: captured.id,
            thread_creation_time: captured.creation_time,
            original_suspend_count: original,
            expected_suspend_count: original
                .checked_add(1)
                .ok_or_else(|| "Disposable thread suspend count overflowed.".to_owned())?,
        }
    };

    recover_journal(&[entry])
}

#[test]
#[ignore = "uses the undocumented JobObjectFreezeInformation contract; run in explicit Windows integration QA"]
fn suspended_job_recovery_thaws_descendants_after_the_recorded_root_exits() -> Result<(), String> {
    let system_root =
        std::env::var_os("SystemRoot").ok_or_else(|| "SystemRoot is unavailable.".to_owned())?;
    let command = Path::new(&system_root).join("System32").join("cmd.exe");
    let wide_command = command
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut command_line = "cmd.exe /D /C start \"\" /B ping.exe 127.0.0.1 -n 15 -w 1000"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut process_information = PROCESS_INFORMATION::default();
    // SAFETY: all pointers reference live, correctly sized buffers; the mutable command line
    // is terminated UTF-16 and the returned handles are owned by this test.
    let created = unsafe {
        CreateProcessW(
            wide_command.as_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_SUSPENDED | CREATE_NO_WINDOW,
            std::ptr::null(),
            std::ptr::null(),
            &startup,
            &mut process_information,
        )
    };
    if created == 0 {
        return Err(format!(
            "CreateProcessW(test root) failed with error {}.",
            last_error()
        ));
    }
    let process = WinHandle::new(process_information.hProcess);
    let thread = WinHandle::new(process_information.hThread);
    let identity = match process_identity(process.raw()) {
        Ok(identity) => identity,
        Err(error) => {
            resume_disposable_root(&process, &thread);
            return Err(error);
        }
    };
    let name = suspension_job_name(identity.id, identity.creation_time);
    let wide_name = name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: wide_name is terminated UTF-16 and the returned handle is owned by this test.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), wide_name.as_ptr()) };
    if job.is_null() {
        let error = last_error();
        resume_disposable_root(&process, &thread);
        return Err(format!("CreateJobObjectW failed with error {error}."));
    }
    let job = WinHandle::new(job);
    // SAFETY: job and process are live handles owned by this test.
    if unsafe { AssignProcessToJobObject(job.raw(), process.raw()) } == 0 {
        let error = last_error();
        resume_disposable_root(&process, &thread);
        return Err(format!(
            "AssignProcessToJobObject failed with error {error}."
        ));
    }
    let cleanup = FrozenJobCleanup { job };
    // SAFETY: thread is the suspended primary thread created above and has not been resumed.
    if unsafe { ResumeThread(thread.raw()) } == u32::MAX {
        return Err(format!(
            "ResumeThread(test root) failed with error {}.",
            last_error()
        ));
    }
    // SAFETY: process is a live disposable handle; this bounded wait retains no pointer.
    if unsafe { WaitForSingleObject(process.raw(), 5_000) } != WAIT_OBJECT_0 {
        return Err("The disposable Job Object root did not exit in time.".to_owned());
    }
    let active_processes = || -> Result<u32, String> {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: cleanup owns the live Job Object handle and accounting is writable for the
        // exact information-class buffer size.
        if unsafe {
            QueryInformationJobObject(
                cleanup.job.raw(),
                JobObjectBasicAccountingInformation,
                (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(format!(
                "QueryInformationJobObject failed with error {}.",
                last_error()
            ));
        }
        Ok(accounting.ActiveProcesses)
    };
    if active_processes()? == 0 {
        return Err("The disposable root did not leave an inherited child in the job.".to_owned());
    }
    let mut freeze = JobObjectFreezeInformation::new(true);
    // SAFETY: job is live and freeze is writable for exactly the supplied structure size.
    if unsafe {
        SetInformationJobObject(
            cleanup.job.raw(),
            JOB_OBJECT_FREEZE_INFORMATION_CLASS,
            (&mut freeze as *mut JobObjectFreezeInformation).cast(),
            std::mem::size_of::<JobObjectFreezeInformation>() as u32,
        )
    } == 0
    {
        return Err(format!(
            "SetInformationJobObject(freeze) failed with error {}.",
            last_error()
        ));
    }
    if active_processes()? == 0 {
        return Err("The disposable descendant exited before the job was frozen.".to_owned());
    }

    recover_journal(&[RecoveryEntry::SuspendedJob {
        name,
        process: identity,
    }])?;
    // The frozen descendant can finish only after recovery thaws it. Observe that progress
    // while cleanup still owns the job, before its Drop implementation can thaw anything.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if active_processes()? == 0 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("The recovered Job Object descendant did not finish in time.".to_owned());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct PowerPlanCleanup {
    original_guid: String,
    disposable_guid: Option<String>,
}

impl PowerPlanCleanup {
    fn restore_and_delete(&mut self) -> Result<(), String> {
        set_active(&self.original_guid)?;
        if let Some(guid) = self.disposable_guid.as_deref() {
            crate::power::powercfg::delete_plan(guid)?;
            self.disposable_guid = None;
        }
        Ok(())
    }
}

impl Drop for PowerPlanCleanup {
    fn drop(&mut self) {
        let _ = set_active(&self.original_guid);
        if let Some(guid) = self.disposable_guid.take() {
            let _ = crate::power::powercfg::delete_plan(&guid);
        }
    }
}

#[test]
#[ignore = "changes the machine-wide active Windows power plan; run manually on a disposable Windows session"]
fn power_plan_recovery_restores_the_original_plan_and_deletes_the_disposable_plan(
) -> Result<(), String> {
    let original_guid = active_plan()?.guid;
    let disposable_guid = crate::power::powercfg::duplicate_adaptive_plan(&original_guid)?;
    let mut cleanup = PowerPlanCleanup {
        original_guid: original_guid.clone(),
        disposable_guid: Some(disposable_guid.clone()),
    };
    crate::power::powercfg::initialize_adaptive_plan(&disposable_guid, &original_guid)?;

    set_active(&disposable_guid)?;
    recover_journal(&[RecoveryEntry::PowerPlan {
        original_guid: original_guid.clone(),
        expected_guid: disposable_guid,
    }])?;
    assert!(active_plan()?.guid.eq_ignore_ascii_case(&original_guid));
    cleanup.restore_and_delete()?;
    Ok(())
}
