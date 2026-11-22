//! Native producer smoke only: direct ticks bypass scheduling; cfg(test) recovery has no watchdog ACK.
use super::*;
use crate::platform::windows::cpu_allocation as native;
use std::{
    io::Write,
    os::windows::process::CommandExt,
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Image(PathBuf);

impl Drop for Image {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_dir(self.0.parent().unwrap());
    }
}

struct Worker(Child);

impl Worker {
    fn start(image: &Image) -> Self {
        Self(
            Command::new(&image.0)
                .args([
                    "--ignored",
                    "--exact",
                    "control::instance_limits::tests::native_worker",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x0800_0000)
                .spawn()
                .unwrap(),
        )
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(mut input) = self.0.stdin.take() {
            let _ = input.write_all(b"\n");
        }
        let _ = self.0.wait();
    }
}

fn members(image: &Image, workers: &[Worker], deadline: Instant) -> Vec<Member> {
    let expected: BTreeSet<_> = workers.iter().map(|worker| worker.0.id()).collect();
    loop {
        assert!(Instant::now() < deadline, "Owned cohort was not complete");
        let observation = membership::collect_members(&[image.0.to_str().unwrap().into()])
            .pop()
            .unwrap();
        if let MembershipObservation::Complete(members) = observation {
            let actual: BTreeSet<_> = members.iter().map(|member| member.id).collect();
            if actual == expected {
                return members;
            }
        }
        assert!(Instant::now() < deadline, "Owned cohort was not complete");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn readback(members: &[Member], assigned: bool, originals: &BTreeMap<u32, (usize, usize)>) {
    let mut used = BTreeSet::new();
    for member in members {
        assert_eq!(membership::member_state(member), Ok(MemberState::Current));
        assert_eq!(
            native::query_affinity(&member.process).unwrap(),
            originals[&member.id]
        );
        let sets = native::query_cpu_sets(&member.process).unwrap();
        assert_eq!(!sets.is_empty(), assigned, "PID {}: {sets:?}", member.id);
        for id in sets {
            assert!(used.insert(id), "CPU Set {id} was shared between instances");
        }
    }
}

fn shared(settings: &Arc<Settings>) -> Arc<SharedAutomationState> {
    Arc::new(SharedAutomationState {
        state: Mutex::new(AutomationWorkerState {
            settings: Arc::clone(settings),
            runtime_revision: SettingsRevision::initial(),
            persisted_revision: SettingsRevision::initial(),
            change_generation: 0,
            status: RuntimeStatusSnapshot::default(),
            pending_auto_exclusions: AutoExclusionPatch::default(),
            pending_auto_exclusions_revision: None,
            pending_auto_exclusions_retry_at: None,
            process_control_commands: VecDeque::new(),
            idle_action_log: Some(Default::default()),
            action_log_clear_requested: false,
            pending_events: AutomationWakeEvents::default(),
            pending_keep_running_resets: Default::default(),
            pending_instance_limits_resets: super::super::instance_limits::PendingResets::default(),
            pending_instance_balancing_resets: PendingResets::default(),
            input_activity: InputActivityTracker::default(),
            windows_event_watcher_active: false,
            worker_accepting_work: false,
            stop_requested: false,
        }),
        changed: Condvar::new(),
        status_generation: AtomicU64::new(0),
        pending_auto_exclusions_generation: AtomicU64::new(0),
        auto_exclusion_wake: Mutex::new(None),
    })
}

fn configure(core: &mut RuntimeCore, shared: &SharedAutomationState, settings: &Arc<Settings>) {
    let mut state = lock_unpoisoned(&shared.state);
    record_settings_changes(&mut state, settings);
    state.settings = Arc::clone(settings);
    let resets = std::mem::take(&mut state.pending_instance_balancing_resets);
    drop(state);
    core.instance_balancing.sync(
        &settings.instance_balancing,
        settings.general.enabled,
        resets,
    );
}

fn tick(
    core: &mut RuntimeCore,
    shared: &SharedAutomationState,
    settings: &Arc<Settings>,
    deadline: Instant,
) -> usize {
    assert!(
        Instant::now() < deadline,
        "Native smoke exceeded its 24-second deadline"
    );
    let watcher_active = lock_unpoisoned(&shared.state).windows_event_watcher_active;
    let status = core.run_instance_balancing_update(shared, settings, settings, watcher_active);
    assert!(
        Instant::now() < deadline,
        "Native producer exceeded its deadline"
    );
    assert_eq!(status.last_error, None, "{:?}", status.detail);
    assert_eq!(status.failed_processes, 0, "{:?}", status.detail);
    if status.enabled && status.adjusted_processes == 0 {
        assert_eq!(
            status.detail,
            Some(InstanceBalancingDetail::WaitingForStableCohort)
        );
    }
    status.adjusted_processes
}

fn await_assignment(
    core: &mut RuntimeCore,
    shared: &SharedAutomationState,
    settings: &Arc<Settings>,
    members: &[Member],
    originals: &BTreeMap<u32, (usize, usize)>,
    deadline: Instant,
) {
    loop {
        assert!(Instant::now() + Duration::from_secs(1) < deadline);
        std::thread::sleep(Duration::from_secs(1));
        let assigned = tick(core, shared, settings, deadline);
        assert!(
            assigned == 0 || assigned == members.len(),
            "Partial cohort was accepted"
        );
        readback(members, assigned != 0, originals);
        if assigned == members.len() {
            return;
        }
    }
}

#[test]
#[ignore = "Changes only bounded private workers' CPU Sets; no scheduler, crash recovery, or performance proof"]
fn native_producer_replans_complete_cohorts_and_restores_on_disable() {
    let topology = cpu_allocation::processor_topology().unwrap();
    let cores: BTreeSet<_> = topology
        .processors()
        .iter()
        .map(|processor| processor.core)
        .collect();
    if topology.groups().len() != 1 || topology.groups()[0].number != 0 || cores.len() < 3 {
        eprintln!(
            "SKIPPED: Instance Balancing smoke requires group 0 and at least three physical cores"
        );
        return;
    }
    let directory = std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!("instance-balancing-native-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let image = Image(directory.join("balancing worker.exe"));
    std::fs::hard_link(std::env::current_exe().unwrap(), &image.0).unwrap();
    let deadline = Instant::now() + Duration::from_secs(24);
    let mut workers = vec![Worker::start(&image), Worker::start(&image)];
    let mut current = members(&image, &workers, deadline);
    let mut originals: BTreeMap<_, _> = current
        .iter()
        .map(|member| (member.id, native::query_affinity(&member.process).unwrap()))
        .collect();
    readback(&current, false, &originals);
    let mut settings = Arc::new(Settings {
        instance_balancing: InstanceBalancingSettings {
            enabled: true,
            rules: vec![InstanceBalancingRule {
                enabled: true,
                executable_path: image.0.to_str().unwrap().into(),
            }],
        },
        ..Settings::default()
    });
    let shared = shared(&settings);
    let events = Arc::clone(&shared);
    let stopped = Arc::clone(&shared);
    let watcher = WindowsEventWatcher::start(
        crate::backend::windows_events::WindowsEventWatcherMode::Full,
        Arc::new(move |event| notify_windows_event(&events, event)),
        Arc::new(move || set_windows_event_watcher_active(&stopped, false)),
    )
    .unwrap();
    lock_unpoisoned(&shared.state).windows_event_watcher_active = watcher.is_running();
    assert!(watcher.is_running());
    let mut core = RuntimeCore::default();
    configure(&mut core, &shared, &settings);
    assert_eq!(tick(&mut core, &shared, &settings, deadline), 0);
    readback(&current, false, &originals);
    await_assignment(
        &mut core, &shared, &settings, &current, &originals, deadline,
    );
    let first_applied = Instant::now();

    workers.push(Worker::start(&image));
    current = members(&image, &workers, deadline);
    let newest = current
        .iter()
        .find(|member| !originals.contains_key(&member.id))
        .unwrap();
    assert!(native::query_cpu_sets(&newest.process).unwrap().is_empty());
    originals.insert(newest.id, native::query_affinity(&newest.process).unwrap());
    assert_eq!(tick(&mut core, &shared, &settings, deadline), 0);
    readback(&current, false, &originals);
    await_assignment(
        &mut core, &shared, &settings, &current, &originals, deadline,
    );
    assert!(first_applied.elapsed() >= Duration::from_secs(5));
    let second_applied = Instant::now();

    drop(workers.pop().unwrap());
    current = members(&image, &workers, deadline);
    assert_eq!(tick(&mut core, &shared, &settings, deadline), 0);
    readback(&current, false, &originals);
    await_assignment(
        &mut core, &shared, &settings, &current, &originals, deadline,
    );
    assert!(second_applied.elapsed() >= Duration::from_secs(5));

    Arc::make_mut(&mut settings).instance_balancing.enabled = false;
    configure(&mut core, &shared, &settings);
    assert_eq!(tick(&mut core, &shared, &settings, deadline), 0);
    readback(&current, false, &originals);
    core.shutdown().unwrap();
    assert!(Instant::now() < deadline);
    drop(current);
    drop(core);
    drop(watcher);
    drop(workers);
    drop(image);
}
