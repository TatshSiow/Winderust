use std::{
    cell::{Cell, OnceCell},
    sync::{Condvar, Mutex, OnceLock},
    time::Duration,
};

use super::*;

#[derive(Default)]
struct State {
    present: Vec<ProcessTargetKey>,
    held: bool,
    prepared: usize,
    committed: usize,
    disposed: usize,
    fail_cleanup: bool,
    incomplete_admission_scan: bool,
    scan_cost: u64,
    elapsed: u64,
    arrival_on_fresh_scan: Option<ProcessTargetKey>,
}
fn state() -> &'static (Mutex<State>, Condvar) {
    static STATE: OnceLock<(Mutex<State>, Condvar)> = OnceLock::new();
    STATE.get_or_init(|| (Mutex::new(State::default()), Condvar::new()))
}
fn edit(f: impl FnOnce(&mut State)) {
    let (lock, changed) = state();
    f(&mut lock.lock().unwrap());
    changed.notify_all();
}
fn wait_for(f: impl Fn(&State) -> bool) {
    let (lock, changed) = state();
    let guard = lock.lock().unwrap();
    let (guard, result) = changed
        .wait_timeout_while(guard, Duration::from_secs(5), |state| !f(state))
        .unwrap();
    assert!(!result.timed_out(), "fake worker did not finish");
    assert!(f(&guard));
}
fn key(id: u32, path: &str) -> ProcessTargetKey {
    crate::control::process::ProcessControlTarget::automatic(
        id,
        "app.exe".into(),
        path.into(),
        u64::from(id),
    )
    .key()
}
thread_local! {
    static IDENTITY_READS: Cell<&'static [(&'static str, Option<&'static str>)]> = const { Cell::new(&[]) };
    static CATALOG_CALLS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

struct Fake;
struct Launch {
    key: ProcessTargetKey,
    accepted: bool,
}
impl Platform for Fake {
    type Catalog = OnceCell<Vec<ProcessTargetKey>>;
    type Context = ();
    type Identity = String;
    type Member = ProcessTargetKey;
    type Launch = Launch;
    fn context() -> Result<(), Failure> {
        Ok(())
    }
    fn binding(_: &()) -> String {
        "desktop".into()
    }
    fn identity(rule: &KeepRunningRule) -> Result<String, Failure> {
        match IDENTITY_READS
            .get()
            .iter()
            .find(|(path, _)| *path == rule.executable_path)
        {
            Some((_, Some(identity))) => Ok((*identity).to_owned()),
            Some((_, None)) => Err(Failure {
                message: "temporary identity read failure".into(),
                kind: FailureKind::Retryable,
            }),
            None => Ok(rule.executable_path.clone()),
        }
    }
    fn scan(
        rule: &KeepRunningRule,
        _: &(),
        exclude: Option<&ProcessTargetKey>,
        catalog: Option<&Self::Catalog>,
    ) -> Result<Vec<ProcessTargetKey>, Failure> {
        let mut state = state().0.lock().unwrap();
        state.elapsed += state.scan_cost;
        let present = if let Some(catalog) = catalog {
            let (captures, scans) = CATALOG_CALLS.get();
            CATALOG_CALLS.set((captures + usize::from(catalog.get().is_none()), scans + 1));
            catalog.get_or_init(|| state.present.clone())
        } else {
            if let Some(member) = state.arrival_on_fresh_scan.take() {
                state.present.push(member);
            }
            &state.present
        };
        Ok(present
            .iter()
            .filter(|member| {
                Some(*member) != exclude && **member == key(member.id, &rule.executable_path)
            })
            .cloned()
            .collect())
    }
    fn scan_fresh(
        rule: &KeepRunningRule,
        context: &(),
        exclude: Option<&ProcessTargetKey>,
    ) -> Result<Vec<ProcessTargetKey>, Failure> {
        if exclude.is_some() && state().0.lock().unwrap().incomplete_admission_scan {
            return Err(Failure {
                message: "incomplete admission scan".into(),
                kind: FailureKind::Retryable,
            });
        }
        Self::scan(rule, context, exclude, None)
    }
    fn key(member: &ProcessTargetKey) -> ProcessTargetKey {
        member.clone()
    }
    fn exited(member: &ProcessTargetKey) -> Result<bool, Failure> {
        Ok(!state().0.lock().unwrap().present.contains(member))
    }
    fn prepare(
        rule: &KeepRunningRule,
        _: &str,
        _: &String,
    ) -> Result<Launch, PrepareFailure<Launch>> {
        let (lock, changed) = state();
        let mut state = lock.lock().unwrap();
        state.prepared += 1;
        changed.notify_all();
        while state.held {
            state = changed.wait(state).unwrap();
        }
        let key = key(100 + state.prepared as u32, &rule.executable_path);
        state.present.push(key.clone());
        Ok(Launch {
            key,
            accepted: false,
        })
    }
    fn launch_key(launch: &Launch) -> Option<ProcessTargetKey> {
        Some(launch.key.clone())
    }
    fn commit(launch: &mut Launch) -> Result<(), Failure> {
        launch.accepted = true;
        edit(|state| state.committed += 1);
        Ok(())
    }
    fn accepted(launch: &Launch) -> bool {
        launch.accepted
    }
    fn take_member(launch: &mut Launch) -> Result<Option<ProcessTargetKey>, Failure> {
        Ok(Some(launch.key.clone()))
    }
    fn dispose(launch: &mut Launch) -> Result<(), Failure> {
        assert!(!launch.accepted, "accepted child must survive");
        let mut result = Ok(());
        edit(|state| {
            if state.fail_cleanup {
                result = Err(Failure {
                    message: "cleanup unknown".into(),
                    kind: FailureKind::Retryable,
                });
            } else {
                state.present.retain(|key| key != &launch.key);
                state.disposed += 1;
            }
        });
        result
    }
}

fn tick(
    coordinator: &mut Coordinator<Fake>,
    settings: &KeepRunningSettings,
    now: Instant,
    clear: bool,
) -> KeepRunningSnapshot {
    coordinator.tick(
        settings,
        true,
        true,
        |_, manager, ticket, now, observation| {
            manager.try_reserve_commit(now, ticket, observation, clear)
        },
        &mut ActionLog::default(),
        || now + Duration::from_secs(state().0.lock().unwrap().elapsed),
    )
}
fn drain(
    coordinator: &mut Coordinator<Fake>,
    settings: &KeepRunningSettings,
    now: Instant,
    clear: bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while coordinator.has_pending() && Instant::now() < deadline {
        tick(coordinator, settings, now, clear);
        std::thread::yield_now();
    }
    assert!(
        !coordinator.has_pending(),
        "native ownership must be acknowledged"
    );
}

#[test]
fn terminal_launch_failure_is_logged_before_another_observation() {
    let path = r"C:\Apps\app.exe";
    let settings = KeepRunningSettings {
        enabled: true,
        rules: vec![KeepRunningRule::new(path.into())],
    };
    let mut coordinator = Coordinator::<Fake>::default();
    coordinator.sync_settings(&settings, true);
    let owner = &mut coordinator.owners[0];
    let context = DesktopBindingKey::new("desktop".into());
    let member = key(1, path);
    let start = Instant::now();
    owner.policy.observe(
        start,
        Observation::Complete {
            observed_at: start,
            context: &context,
            members: std::slice::from_ref(&member),
        },
    );
    owner.policy.exited(start, &member);
    let now = start + Duration::from_secs(1);
    let PrepareDisposition::Prepare(ticket) = owner.policy.try_prepare(
        now,
        Observation::Complete {
            observed_at: now,
            context: &context,
            members: &[],
        },
    ) else {
        panic!("expected a launch attempt");
    };

    let mut log = ActionLog::default();
    let message = "unsupported launch";
    owner.launch_failed(
        now,
        &ticket,
        Failure {
            message: message.into(),
            kind: FailureKind::UnsupportedContext,
        },
        &mut log,
    );
    let entries = log.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].reason, message);
    assert_eq!(entries[0].result, ActionLogResult::Failed);
    assert!(matches!(owner.last_error, Some(RuleError::Recorded(_))));
}

#[test]
fn bounded_worker_routes_late_results_and_never_disposes_accepted_children() {
    let mut settings = KeepRunningSettings {
        enabled: true,
        rules: vec![KeepRunningRule::new(r"C:\Apps\app.exe".into())],
    };
    edit(|state| *state = State::default());
    let now = Instant::now();
    let mut coordinator = Coordinator::<Fake>::default();
    assert_eq!(
        tick(&mut coordinator, &settings, now, true).rules[0].state,
        KeepRunningState::WaitingForApp
    );
    assert_eq!(state().0.lock().unwrap().prepared, 0);

    // A retained positive exit, not initial absence, opens the first retry.
    edit(|state| {
        state
            .present
            .push(key(1, &settings.rules[0].executable_path))
    });
    tick(&mut coordinator, &settings, now, true);
    edit(|state| {
        state.present.clear();
        state.held = true;
    });
    tick(&mut coordinator, &settings, now, true);
    tick(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(1),
        true,
    );
    wait_for(|state| state.prepared == 1);
    assert!(coordinator.has_pending());

    // Timeouts retain the global slot even if another rule becomes eligible.
    tick(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(7),
        true,
    );
    assert_eq!(
        coordinator.owners[0].policy.state(),
        KeepRunningState::Paused(PauseReason::PreparationTimedOut)
    );
    settings
        .rules
        .push(KeepRunningRule::new(r"C:\Apps\second.exe".into()));
    edit(|state| {
        state
            .present
            .push(key(9, &settings.rules[1].executable_path))
    });
    tick(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(7),
        true,
    );
    assert_eq!(
        coordinator.owners[1].policy.state(),
        KeepRunningState::Armed
    );
    edit(|state| state.present.retain(|key| key.id != 9));
    tick(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(8),
        true,
    );
    tick(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(20),
        true,
    );
    assert_eq!(state().0.lock().unwrap().prepared, 1);
    assert!(matches!(
        coordinator.owners[1].policy.state(),
        KeepRunningState::Retrying { .. }
    ));
    settings.rules[1].enabled = false;

    // Removal/re-addition cannot make the old ticket current or resume its late child.
    settings.rules.remove(0);
    tick(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(20),
        true,
    );
    settings
        .rules
        .insert(0, KeepRunningRule::new(r"C:\Apps\app.exe".into()));
    edit(|state| state.held = false);
    drain(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(20),
        true,
    );
    assert_eq!(state().0.lock().unwrap().committed, 0);
    assert_eq!(state().0.lock().unwrap().disposed, 1);
    drop(coordinator);

    // A queued Stop wins before reservation; failed cleanup remains quarantined across disable.
    settings.rules.truncate(1);
    edit(|state| {
        *state = State {
            present: vec![key(2, &settings.rules[0].executable_path)],
            held: true,
            ..Default::default()
        }
    });
    let mut coordinator = Coordinator::<Fake>::default();
    tick(&mut coordinator, &settings, now, true);
    edit(|state| state.present.clear());
    tick(&mut coordinator, &settings, now, true);
    tick(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(1),
        true,
    );
    wait_for(|state| state.prepared == 1);
    coordinator.pause_all(PauseReason::IntentionalStop);
    edit(|state| {
        state.fail_cleanup = true;
        state.held = false;
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while coordinator.owners[0].policy.state()
        != KeepRunningState::Paused(PauseReason::CleanupUnconfirmed)
        && Instant::now() < deadline
    {
        tick(
            &mut coordinator,
            &settings,
            now + Duration::from_secs(2),
            false,
        );
        std::thread::yield_now();
    }
    assert_eq!(
        coordinator.owners[0].policy.state(),
        KeepRunningState::Paused(PauseReason::CleanupUnconfirmed)
    );
    settings.rules[0].enabled = false;
    coordinator.invalidate_settings(&PendingResets {
        all: true,
        ..PendingResets::default()
    });
    coordinator.sync_settings(&settings, true);
    assert_eq!(
        coordinator.owners[0]
            .last_error
            .as_ref()
            .map(RuleError::message),
        Some("cleanup unknown")
    );
    assert!(coordinator.has_pending());
    settings.enabled = false;
    settings.rules[0].enabled = true;
    tick(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(3),
        false,
    );
    assert!(coordinator.has_pending());
    edit(|state| state.fail_cleanup = false);
    drain(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(3),
        false,
    );
    assert_eq!(state().0.lock().unwrap().committed, 0);
    drop(coordinator);

    // Fresh unknown observations block reservation. Later intents follow a committed launch.
    settings.enabled = true;
    edit(|state| {
        *state = State {
            present: vec![key(3, &settings.rules[0].executable_path)],
            held: true,
            ..Default::default()
        }
    });
    let mut coordinator = Coordinator::<Fake>::default();
    tick(&mut coordinator, &settings, now, true);
    edit(|state| state.present.clear());
    tick(&mut coordinator, &settings, now, true);
    tick(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(1),
        true,
    );
    wait_for(|state| state.prepared == 1);
    edit(|state| {
        state.held = false;
        state.incomplete_admission_scan = true;
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let unavailable = loop {
        let snapshot = tick(
            &mut coordinator,
            &settings,
            now + Duration::from_secs(2),
            true,
        );
        if matches!(
            coordinator.flight.as_ref().map(|flight| &flight.phase),
            Some(Phase::Ready(_))
        ) {
            break snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "fake worker did not become ready"
        );
        std::thread::yield_now();
    };
    assert!(!unavailable.observation_known);
    assert_eq!(
        unavailable.last_error.as_deref(),
        Some("incomplete admission scan")
    );
    assert_eq!(state().0.lock().unwrap().committed, 0);
    edit(|state| state.incomplete_admission_scan = false);
    let recovered = tick(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(2),
        false,
    );
    assert!(recovered.observation_known);
    assert!(recovered.last_error.is_none());
    assert_eq!(state().0.lock().unwrap().committed, 0);
    tick(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(2),
        true,
    );
    coordinator.pause_all(PauseReason::IntentionalStop);
    drain(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(2),
        true,
    );
    assert_eq!(state().0.lock().unwrap().committed, 1);
    assert_eq!(state().0.lock().unwrap().disposed, 0);
    coordinator.shutdown();
    assert_eq!(state().0.lock().unwrap().disposed, 0);
    drop(coordinator);

    // A slow final verification must use its completion time, not the cycle's old timestamp.
    edit(|state| {
        *state = State {
            present: vec![key(4, &settings.rules[0].executable_path)],
            held: true,
            ..Default::default()
        }
    });
    let mut coordinator = Coordinator::<Fake>::default();
    tick(&mut coordinator, &settings, now, true);
    edit(|state| state.present.clear());
    tick(&mut coordinator, &settings, now, true);
    tick(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(1),
        true,
    );
    wait_for(|state| state.prepared == 1);
    edit(|state| state.held = false);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(
        coordinator.flight.as_ref().map(|flight| &flight.phase),
        Some(Phase::Ready(_))
    ) && Instant::now() < deadline
    {
        tick(
            &mut coordinator,
            &settings,
            now + Duration::from_secs(2),
            false,
        );
        std::thread::yield_now();
    }
    edit(|state| state.scan_cost = 3);
    tick(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(2),
        true,
    );
    edit(|state| state.scan_cost = 0);
    drain(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(2),
        true,
    );
    assert_eq!(state().0.lock().unwrap().committed, 0);
    assert_eq!(state().0.lock().unwrap().disposed, 1);
    drop(coordinator);

    // Ordinary rules share a catalog. A member appearing before reservation prevents a duplicate launch.
    settings
        .rules
        .push(KeepRunningRule::new(r"C:\Apps\second.exe".into()));
    let path = &settings.rules[0].executable_path;
    edit(|state| {
        *state = State {
            present: vec![key(5, path)],
            held: true,
            ..State::default()
        }
    });
    let mut coordinator = Coordinator::<Fake>::default();
    CATALOG_CALLS.set((0, 0));
    tick(&mut coordinator, &settings, now, true);
    assert_eq!(CATALOG_CALLS.get(), (1, 2));
    edit(|state| state.present.clear());
    tick(&mut coordinator, &settings, now, true);
    tick(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(1),
        true,
    );
    wait_for(|state| state.prepared == 1);
    let external = key(6, path);
    edit(|state| {
        state.held = false;
        state.arrival_on_fresh_scan = Some(external.clone());
    });
    drain(
        &mut coordinator,
        &settings,
        now + Duration::from_secs(2),
        true,
    );
    let state = state().0.lock().unwrap();
    assert_eq!((state.prepared, state.committed, state.disposed), (1, 0, 1));
    assert_eq!(state.present, [external]);
}

#[test]
fn path_resets_preserve_the_other_rules_pending_retry() {
    let settings = KeepRunningSettings {
        enabled: true,
        rules: [r"C:\Apps\first.exe", r"C:\Apps\second.exe"]
            .map(|path| KeepRunningRule::new(path.into()))
            .into(),
    };
    let mut coordinator = Coordinator::<Fake>::default();
    coordinator.sync_settings(&settings, true);
    let now = Instant::now();
    let context = DesktopBindingKey::new("desktop".into());
    for (index, owner) in coordinator.owners.iter_mut().enumerate() {
        let member = key(index as u32 + 1, &owner.rule.executable_path);
        owner.policy.observe(
            now,
            Observation::Complete {
                observed_at: now,
                context: &context,
                members: std::slice::from_ref(&member),
            },
        );
        owner.policy.exited(now, &member);
    }
    let second = coordinator.owners[1].policy.state();
    assert!(matches!(second, KeepRunningState::Retrying { .. }));
    coordinator.invalidate_settings(&PendingResets {
        paths: [settings.rules[0].executable_path.clone()].into(),
        ..PendingResets::default()
    });
    coordinator.sync_settings(&settings, true);
    assert_eq!(
        coordinator.owners[0].policy.state(),
        KeepRunningState::WaitingForApp
    );
    assert_eq!(coordinator.owners[1].policy.state(), second);
}

#[test]
fn resume_keeps_a_verified_alias_paused_after_a_failed_identity_read() {
    const ORIGINAL: &str = r"C:\Keep Running Test\original.exe";
    const ALIAS: &str = r"C:\Keep Running Test\alias.exe";
    let settings = KeepRunningSettings {
        enabled: true,
        rules: [ORIGINAL, ALIAS]
            .map(|path| KeepRunningRule::new(path.into()))
            .into(),
    };
    let mut coordinator = Coordinator::<Fake>::default();
    coordinator.sync_settings(&settings, true);
    let now = Instant::now();
    let context = DesktopBindingKey::new("desktop".into());
    let member = key(31, ALIAS);
    coordinator.owners[1].policy.observe(
        now,
        Observation::Complete {
            observed_at: now,
            context: &context,
            members: std::slice::from_ref(&member),
        },
    );
    coordinator.owners[1].policy.exited(now, &member);
    let observe = |coordinator: &mut Coordinator<Fake>| {
        coordinator.tick(
            &settings,
            true,
            true,
            |_, _, _, _, _| CommitDisposition::Deferred,
            &mut ActionLog::default(),
            || now,
        )
    };
    IDENTITY_READS.set(&[(ORIGINAL, Some("original")), (ALIAS, Some("replacement"))]);
    observe(&mut coordinator);
    IDENTITY_READS.set(&[
        (ORIGINAL, Some("replacement")),
        (ALIAS, Some("replacement")),
    ]);
    let paused = KeepRunningState::Paused(PauseReason::UnsupportedContext);
    assert!(observe(&mut coordinator)
        .rules
        .iter()
        .all(|row| row.state == paused));
    IDENTITY_READS.set(&[(ORIGINAL, None), (ALIAS, Some("replacement"))]);
    coordinator.resume(ALIAS);
    assert!(matches!(
        coordinator.owners[1].policy.state(),
        KeepRunningState::Retrying { .. }
    ));
    let status = observe(&mut coordinator);
    IDENTITY_READS.set(&[]);
    assert_eq!(status.rules[1].state, paused);
    assert_eq!(coordinator.owners[0].identity.as_deref(), Some("original"));
    assert!(!coordinator.has_pending());
}
