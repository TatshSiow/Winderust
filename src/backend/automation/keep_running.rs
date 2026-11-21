use std::{
    collections::BTreeMap,
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    time::Instant,
};

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{KeepRunningRule, KeepRunningSettings},
    control::process::ProcessTargetKey,
    features::advanced_controls::keep_running::{
        CleanupAcknowledgment, CleanupOutcome, CommitDisposition, DesktopBindingKey, FailureKind,
        KeepRunningManager, KeepRunningRuleSnapshot, KeepRunningSnapshot, KeepRunningState,
        LaunchTicket, Observation, PauseReason, PrepareDisposition, PreparedDisposition,
    },
    foreground::{same_executable_path, ProcessActionTarget},
};

pub(super) struct Failure {
    message: String,
    kind: FailureKind,
}

pub(super) struct PrepareFailure<L> {
    error: Failure,
    cleanup: Option<L>,
}

// This seam exercises real ownership/routing with a held fake native operation in tests.
pub(super) trait Platform: Send + 'static {
    type Context;
    type Identity: Clone + Eq + Send;
    type Member;
    type Launch: Send;
    fn context() -> Result<Self::Context, Failure>;
    fn binding(context: &Self::Context) -> String;
    fn identity(rule: &KeepRunningRule) -> Result<Self::Identity, Failure>;
    fn scan(
        rule: &KeepRunningRule,
        context: &Self::Context,
        exclude: Option<&ProcessTargetKey>,
    ) -> Result<Vec<Self::Member>, Failure>;
    fn key(member: &Self::Member) -> ProcessTargetKey;
    fn exited(member: &Self::Member) -> Result<bool, Failure>;
    fn prepare(
        rule: &KeepRunningRule,
        binding: &str,
        identity: &Self::Identity,
    ) -> Result<Self::Launch, PrepareFailure<Self::Launch>>;
    fn launch_key(launch: &Self::Launch) -> Option<ProcessTargetKey>;
    fn commit(launch: &mut Self::Launch) -> Result<(), Failure>;
    fn accepted(launch: &Self::Launch) -> bool;
    fn take_member(launch: &mut Self::Launch) -> Result<Option<Self::Member>, Failure>;
    fn dispose(launch: &mut Self::Launch) -> Result<(), Failure>;
}

enum Work<P: Platform> {
    Prepare(KeepRunningRule, String, P::Identity),
    Commit(P::Launch),
    Dispose(P::Launch),
}
enum Completion<P: Platform> {
    Prepared(Result<P::Launch, PrepareFailure<P::Launch>>),
    Committed(P::Launch, Result<(), Failure>),
    Disposed(P::Launch, Result<(), Failure>),
}

struct Worker<P: Platform> {
    work: SyncSender<Work<P>>,
    completions: Receiver<Completion<P>>,
}

impl<P: Platform> Worker<P> {
    fn start() -> Result<Self, String> {
        let (work, incoming) = mpsc::sync_channel(1);
        let (outgoing, completions) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("Keep Running".into())
            .spawn(move || {
                while let Ok(work) = incoming.recv() {
                    let completion = match work {
                        Work::Prepare(rule, binding, identity) => {
                            Completion::Prepared(P::prepare(&rule, &binding, &identity))
                        }
                        Work::Commit(mut launch) => {
                            let result = P::commit(&mut launch);
                            Completion::Committed(launch, result)
                        }
                        Work::Dispose(mut launch) => {
                            let result = P::dispose(&mut launch);
                            Completion::Disposed(launch, result)
                        }
                    };
                    // Receiver loss drops only this worker's provisional job; accepted roots survive.
                    if let Err(error) = outgoing.send(completion) {
                        let launch = match error.0 {
                            Completion::Prepared(Ok(launch))
                            | Completion::Committed(launch, _)
                            | Completion::Disposed(launch, _) => Some(launch),
                            Completion::Prepared(Err(failure)) => failure.cleanup,
                        };
                        if let Some(mut launch) = launch {
                            while !P::accepted(&launch) && P::dispose(&mut launch).is_err() {
                                std::thread::sleep(std::time::Duration::from_secs(1));
                            }
                        }
                        break;
                    }
                }
            })
            .map_err(|error| format!("Keep Running worker could not start: {error}"))?;
        Ok(Self { work, completions })
    }
}

struct Owner<P: Platform> {
    id: Arc<()>,
    rule: KeepRunningRule,
    active: bool,
    policy: KeepRunningManager,
    identity: Option<P::Identity>,
    members: BTreeMap<ProcessTargetKey, P::Member>,
    last_error: Option<String>,
    retry_error: Option<String>,
    published: Option<(KeepRunningState, Option<String>)>,
}

impl<P: Platform> Owner<P> {
    fn launch_failed(
        &mut self,
        now: Instant,
        ticket: &LaunchTicket,
        failure: Failure,
        log: &mut ActionLog,
    ) {
        let was_starting = self.policy.state() == KeepRunningState::Starting;
        self.policy.failed(now, ticket, failure.kind);
        if was_starting && matches!(self.policy.state(), KeepRunningState::Retrying { .. }) {
            log.record(
                ActionLogFeature::KeepRunning,
                None,
                &self.rule.executable_path,
                ActionLogResult::Failed,
                failure.message.clone(),
            );
            self.retry_error = Some(failure.message);
        } else {
            self.last_error = Some(failure.message);
        }
    }
}

enum Phase<L> {
    Preparing,
    Ready(L),
    Committing,
    Cleaning,
    CleanupRetry(L),
    Accepted(L),
    Quarantined,
}
struct InFlight<L> {
    owner: Arc<()>,
    ticket: LaunchTicket,
    phase: Phase<L>,
}

pub(super) type KeepRunningCoordinator = Coordinator<Windows>;

fn lexical_path_bytes(path: &str) -> impl Iterator<Item = u8> + '_ {
    path.trim().bytes().map(|byte| match byte {
        b'/' => std::path::MAIN_SEPARATOR as u8,
        _ => byte.to_ascii_lowercase(),
    })
}

pub(super) struct Coordinator<P: Platform> {
    owners: Vec<Owner<P>>,
    worker: Option<Worker<P>>,
    flight: Option<InFlight<P::Launch>>,
}

impl<P: Platform> Default for Coordinator<P> {
    fn default() -> Self {
        Self {
            owners: Vec::new(),
            worker: None,
            flight: None,
        }
    }
}

impl<P: Platform> Coordinator<P> {
    pub(super) fn sync_settings(&mut self, settings: &KeepRunningSettings, general_enabled: bool) {
        for owner in &mut self.owners {
            owner.active = false;
        }
        for rule in &settings.rules {
            let index = self.owners.iter().position(|owner| {
                lexical_path_bytes(&owner.rule.executable_path)
                    .eq(lexical_path_bytes(&rule.executable_path))
            });
            let owner = if let Some(index) = index {
                &mut self.owners[index]
            } else {
                self.owners.push(Owner {
                    id: Arc::new(()),
                    rule: rule.clone(),
                    active: true,
                    policy: KeepRunningManager::default(),
                    identity: None,
                    members: BTreeMap::new(),
                    last_error: None,
                    retry_error: None,
                    published: None,
                });
                let Some(owner) = self.owners.last_mut() else {
                    unreachable!("just inserted Keep Running owner")
                };
                owner
            };
            owner.active = true;
            if owner.rule != *rule {
                owner.policy.launch_settings_changed();
                owner.rule = rule.clone();
                owner.members.clear();
                owner.identity = None;
                owner.last_error = None;
                owner.retry_error = None;
            }
            if general_enabled && settings.enabled && rule.enabled {
                owner.policy.enable();
            } else {
                owner.policy.disable();
                owner.members.clear();
            }
        }
        for owner in &mut self.owners {
            if !owner.active {
                owner.policy.disable();
                owner.members.clear();
            }
        }
        let flight = self.flight.as_ref().map(|flight| &flight.owner);
        self.owners
            .retain(|owner| owner.active || flight.is_some_and(|id| Arc::ptr_eq(id, &owner.id)));
    }

    pub(super) fn has_pending(&self) -> bool {
        self.flight.is_some()
    }

    pub(super) fn invalidate_settings(&mut self) {
        for owner in &mut self.owners {
            owner.policy.launch_settings_changed();
            owner.members.clear();
            owner.identity = None;
            owner.retry_error = None;
        }
    }

    pub(super) fn pause_targets(&mut self, targets: &[ProcessActionTarget], reason: PauseReason) {
        for owner in &mut self.owners {
            if targets.iter().any(|target| {
                owner.members.contains_key(
                    &crate::control::process::ProcessControlTarget::from_action_target(target)
                        .key(),
                ) || same_executable_path(
                    &target.executable_path,
                    std::path::Path::new(&owner.rule.executable_path),
                )
            }) {
                owner.policy.pause(reason);
            }
        }
    }

    pub(super) fn pause_all(&mut self, reason: PauseReason) {
        for owner in &mut self.owners {
            owner.policy.pause(reason);
        }
    }

    pub(super) fn resume(&mut self, path: &str) {
        let binding = P::context()
            .ok()
            .map(|context| DesktopBindingKey::new(P::binding(&context)));
        for owner in &mut self.owners {
            if owner.active
                && same_executable_path(
                    std::path::Path::new(path),
                    std::path::Path::new(&owner.rule.executable_path),
                )
            {
                owner.last_error = None;
                owner.retry_error = None;
                owner.policy.resume(Instant::now(), binding.as_ref());
            }
        }
    }

    fn send(&mut self, work: Work<P>) -> Result<(), (String, Box<Work<P>>)> {
        if self.worker.is_none() {
            match Worker::start() {
                Ok(worker) => self.worker = Some(worker),
                Err(error) => return Err((error, Box::new(work))),
            }
        }
        let Some(worker) = self.worker.as_ref() else {
            unreachable!("worker initialized")
        };
        // Only the sole occupied slot may submit its next operation; never block RuntimeCore.
        worker.work.try_send(work).map_err(|error| {
            let work = match error {
                mpsc::TrySendError::Full(work) | mpsc::TrySendError::Disconnected(work) => work,
            };
            (
                "Keep Running worker ownership is unavailable; launches are paused.".into(),
                Box::new(work),
            )
        })
    }

    fn completion(&mut self, now: Instant, log: &mut ActionLog) {
        let Some(mut flight) = self.flight.take() else {
            return;
        };
        let Some(index) = self
            .owners
            .iter()
            .position(|owner| Arc::ptr_eq(&owner.id, &flight.owner))
        else {
            self.flight = Some(InFlight {
                phase: Phase::Quarantined,
                ..flight
            });
            return;
        };
        self.owners[index].policy.timeout(now, &flight.ticket);
        if self.owners[index].policy.disposal_required(&flight.ticket) {
            if let Phase::Ready(launch) = flight.phase {
                flight.phase = Phase::CleanupRetry(launch);
            }
        }
        let completion = self
            .worker
            .as_ref()
            .map(|worker| worker.completions.try_recv());
        let mut released = false;
        if let Some(Ok(completion)) = completion {
            let owner = &mut self.owners[index];
            match completion {
                Completion::Prepared(Ok(launch)) => {
                    flight.phase = match owner.policy.prepared(now, &flight.ticket) {
                        PreparedDisposition::Ready => Phase::Ready(launch),
                        PreparedDisposition::DisposeProvisional | PreparedDisposition::Ignore => {
                            Phase::CleanupRetry(launch)
                        }
                    };
                }
                Completion::Prepared(Err(failure)) => {
                    owner.launch_failed(now, &flight.ticket, failure.error, log);
                    if let Some(launch) = failure.cleanup {
                        flight.phase = Phase::CleanupRetry(launch);
                    } else {
                        released = owner
                            .policy
                            .cleanup_ack(&flight.ticket, CleanupOutcome::NoChildCreated)
                            == CleanupAcknowledgment::Released;
                    }
                }
                Completion::Committed(launch, result) => {
                    if P::accepted(&launch) {
                        if let Some(key) = P::launch_key(&launch) {
                            let process_id = key.id;
                            owner.policy.accepted(&flight.ticket, key);
                            if let Err(error) = result {
                                owner.last_error = Some(error.message);
                            } else {
                                log.record(
                                    ActionLogFeature::KeepRunning,
                                    Some(process_id),
                                    &owner.rule.executable_path,
                                    ActionLogResult::Applied,
                                    rust_i18n::t!("keep_running.relaunched").to_string(),
                                );
                            }
                            flight.phase = Phase::Accepted(launch);
                        } else {
                            owner.last_error = Some(
                                "Accepted Keep Running process identity is unavailable.".into(),
                            );
                            owner.policy.pause(PauseReason::CleanupUnconfirmed);
                            // Retain the accepted handle; it must never enter provisional disposal.
                            flight.phase = Phase::Accepted(launch);
                        }
                    } else {
                        let error = result.err().unwrap_or(Failure {
                            kind: FailureKind::Retryable,
                            message: "Keep Running launch was not accepted.".into(),
                        });
                        owner.launch_failed(now, &flight.ticket, error, log);
                        flight.phase = Phase::CleanupRetry(launch);
                    }
                }
                Completion::Disposed(launch, result) => match result {
                    Ok(()) => {
                        released = owner
                            .policy
                            .cleanup_ack(&flight.ticket, CleanupOutcome::Disposed)
                            == CleanupAcknowledgment::Released;
                    }
                    Err(error) => {
                        owner.last_error = Some(error.message);
                        owner
                            .policy
                            .cleanup_ack(&flight.ticket, CleanupOutcome::Unknown);
                        flight.phase = Phase::CleanupRetry(launch);
                    }
                },
            }
        } else if matches!(completion, Some(Err(mpsc::TryRecvError::Disconnected))) {
            self.owners[index].last_error =
                Some("Keep Running worker disconnected; restart Winderust to recover.".into());
            self.owners[index]
                .policy
                .pause(PauseReason::CleanupUnconfirmed);
            flight.phase = Phase::Quarantined;
        }
        if !released {
            flight.phase = match flight.phase {
                Phase::CleanupRetry(launch) => match self.send(Work::Dispose(launch)) {
                    Ok(()) => Phase::Cleaning,
                    Err((error, work)) => {
                        self.owners[index].last_error = Some(error);
                        let Work::Dispose(launch) = *work else {
                            unreachable!("dispose submission returns its own command")
                        };
                        Phase::CleanupRetry(launch)
                    }
                },
                Phase::Accepted(mut launch) => match P::take_member(&mut launch) {
                    Ok(Some(member)) => {
                        self.owners[index].members.insert(P::key(&member), member);
                        released = true;
                        Phase::Quarantined
                    }
                    Ok(None) => {
                        self.owners[index].last_error =
                            Some("Keep Running accepted process handle is unavailable.".into());
                        Phase::Accepted(launch)
                    }
                    Err(error) => {
                        self.owners[index].last_error = Some(error.message);
                        Phase::Accepted(launch)
                    }
                },
                other => other,
            };
            if !released {
                self.flight = Some(flight);
            }
        }
    }

    pub(super) fn update(
        &mut self,
        settings: &KeepRunningSettings,
        general_enabled: bool,
        watcher_active: bool,
        mut reserve: impl FnMut(
            &KeepRunningRule,
            &mut KeepRunningManager,
            &LaunchTicket,
            Instant,
            Observation<'_>,
        ) -> CommitDisposition,
        log: &mut ActionLog,
    ) -> KeepRunningSnapshot {
        self.tick(
            settings,
            general_enabled,
            watcher_active,
            &mut reserve,
            log,
            Instant::now,
        )
    }

    fn tick(
        &mut self,
        settings: &KeepRunningSettings,
        general_enabled: bool,
        watcher_active: bool,
        mut reserve: impl FnMut(
            &KeepRunningRule,
            &mut KeepRunningManager,
            &LaunchTicket,
            Instant,
            Observation<'_>,
        ) -> CommitDisposition,
        log: &mut ActionLog,
        clock: impl Fn() -> Instant,
    ) -> KeepRunningSnapshot {
        let now = clock();
        self.sync_settings(settings, general_enabled);
        self.completion(now, log);
        let enabled = general_enabled && settings.enabled;
        let context = P::context();
        let mut known = watcher_active && context.is_ok();
        if !watcher_active {
            self.pause_all(PauseReason::UnsupportedContext);
        }
        let mut evidence = BTreeMap::new();
        if let Ok(context) = context {
            let binding = DesktopBindingKey::new(P::binding(&context));
            for (index, owner) in self
                .owners
                .iter_mut()
                .enumerate()
                .filter(|(_, owner)| owner.active)
            {
                if owner.policy.state() == KeepRunningState::Disabled {
                    continue;
                }
                let mut exits_known = true;
                owner.members.retain(|key, member| match P::exited(member) {
                    Ok(true) => {
                        owner.policy.exited(now, key);
                        false
                    }
                    Ok(false) => true,
                    Err(error) => {
                        owner.last_error = Some(error.message);
                        exits_known = false;
                        true
                    }
                });
                if self.flight.as_ref().is_some_and(|flight| {
                    Arc::ptr_eq(&flight.owner, &owner.id)
                        && !matches!(flight.phase, Phase::Ready(_))
                }) {
                    owner.policy.observe(now, Observation::Partial);
                    known = false;
                    continue;
                }
                match P::identity(&owner.rule) {
                    Ok(identity) if owner.identity.as_ref().is_none_or(|old| old == &identity) => {
                        owner.identity = Some(identity)
                    }
                    Ok(_) => {
                        owner.policy.pause(PauseReason::UnsupportedContext);
                        owner.last_error = Some(
                            "The Keep Running executable changed; edit the rule before resuming."
                                .into(),
                        );
                        exits_known = false;
                    }
                    Err(error) => {
                        if matches!(error.kind, FailureKind::UnsupportedContext) {
                            owner.policy.pause(PauseReason::UnsupportedContext);
                        }
                        owner.last_error = Some(error.message);
                        exits_known = false;
                    }
                }
                let exclude = self
                    .flight
                    .as_ref()
                    .filter(|flight| Arc::ptr_eq(&flight.owner, &owner.id))
                    .and_then(|flight| match &flight.phase {
                        Phase::Ready(launch) => P::launch_key(launch),
                        _ => None,
                    });
                let scan = P::scan(&owner.rule, &context, exclude.as_ref());
                let now = clock();
                match scan {
                    Ok(members) if exits_known && watcher_active => {
                        if !matches!(owner.policy.state(), KeepRunningState::Paused(_)) {
                            owner.last_error = None;
                        }
                        let keys: Vec<_> = members.iter().map(P::key).collect();
                        owner.policy.observe(
                            now,
                            Observation::Complete {
                                observed_at: now,
                                context: &binding,
                                members: &keys,
                            },
                        );
                        for member in members {
                            owner.members.entry(P::key(&member)).or_insert(member);
                        }
                        evidence.insert(index, keys);
                    }
                    Ok(_) => {
                        known = false;
                        owner.policy.observe(now, Observation::Partial);
                    }
                    Err(error) => {
                        known = false;
                        if matches!(error.kind, FailureKind::UnsupportedContext) {
                            owner.policy.pause(PauseReason::UnsupportedContext);
                        }
                        owner.last_error = Some(error.message);
                        owner.policy.observe(now, Observation::Unavailable);
                    }
                }
            }
            // File identities catch hard links and reparse aliases; no rule wins by list order.
            for i in 0..self.owners.len() {
                for j in i + 1..self.owners.len() {
                    let overlapping_members = evidence
                        .get(&i)
                        .zip(evidence.get(&j))
                        .is_some_and(|(a, b)| a.iter().any(|key| b.contains(key)));
                    if self.owners[i].active
                        && self.owners[j].active
                        && self.owners[i].rule.enabled
                        && self.owners[j].rule.enabled
                        && ((self.owners[i].identity.is_some()
                            && self.owners[i].identity == self.owners[j].identity)
                            || overlapping_members)
                    {
                        for index in [i, j] {
                            self.owners[index]
                                .policy
                                .pause(PauseReason::UnsupportedContext);
                            self.owners[index].last_error = Some("Keep Running rules refer to the same executable; remove the duplicate rule.".into());
                            evidence.remove(&index);
                        }
                    }
                }
            }
            if let Some(mut flight) = self.flight.take() {
                if let Some(index) = self
                    .owners
                    .iter()
                    .position(|owner| Arc::ptr_eq(&owner.id, &flight.owner))
                {
                    if let Phase::Ready(launch) = flight.phase {
                        // Re-enumerate immediately before the shared intake reservation, never use the cycle cache.
                        let exclude = P::launch_key(&launch);
                        let fresh = P::scan(&self.owners[index].rule, &context, exclude.as_ref());
                        let now = clock();
                        let disposition = if let Ok(members) = fresh {
                            let keys: Vec<_> = members.iter().map(P::key).collect();
                            let owner = &mut self.owners[index];
                            let observation = Observation::Complete {
                                observed_at: now,
                                context: &binding,
                                members: &keys,
                            };
                            let disposition = reserve(
                                &owner.rule,
                                &mut owner.policy,
                                &flight.ticket,
                                now,
                                observation,
                            );
                            for member in members {
                                owner.members.entry(P::key(&member)).or_insert(member);
                            }
                            disposition
                        } else if let Err(error) = fresh {
                            let owner = &mut self.owners[index];
                            owner.last_error = Some(error.message);
                            if matches!(error.kind, FailureKind::UnsupportedContext) {
                                owner.policy.pause(PauseReason::UnsupportedContext);
                            }
                            owner.policy.timeout(now, &flight.ticket);
                            if owner.policy.disposal_required(&flight.ticket) {
                                CommitDisposition::DisposeProvisional
                            } else {
                                CommitDisposition::Deferred
                            }
                        } else {
                            unreachable!("fresh observation is either members or an error")
                        };
                        flight.phase = match disposition {
                            CommitDisposition::Reserved => match self.send(Work::Commit(launch)) {
                                Ok(()) => Phase::Committing,
                                Err((error, work)) => {
                                    let Work::Commit(launch) = *work else {
                                        unreachable!("commit submission returns its own command")
                                    };
                                    self.owners[index].launch_failed(
                                        now,
                                        &flight.ticket,
                                        Failure {
                                            message: error,
                                            kind: FailureKind::Retryable,
                                        },
                                        log,
                                    );
                                    Phase::CleanupRetry(launch)
                                }
                            },
                            CommitDisposition::DisposeProvisional | CommitDisposition::Ignore => {
                                Phase::CleanupRetry(launch)
                            }
                            CommitDisposition::Deferred => Phase::Ready(launch),
                        };
                    }
                }
                self.flight = Some(flight);
            } else if enabled && watcher_active {
                for index in evidence.keys().copied() {
                    let now = clock();
                    if !matches!(self.owners[index].policy.state(), KeepRunningState::Retrying { next_attempt } if now >= next_attempt)
                    {
                        continue;
                    }
                    let members = match P::scan(&self.owners[index].rule, &context, None) {
                        Ok(members) => members,
                        Err(error) => {
                            let owner = &mut self.owners[index];
                            if matches!(error.kind, FailureKind::UnsupportedContext) {
                                owner.policy.pause(PauseReason::UnsupportedContext);
                            }
                            owner.last_error = Some(error.message);
                            owner.policy.observe(clock(), Observation::Unavailable);
                            known = false;
                            continue;
                        }
                    };
                    let keys: Vec<_> = members.iter().map(P::key).collect();
                    for member in members {
                        self.owners[index]
                            .members
                            .entry(P::key(&member))
                            .or_insert(member);
                    }
                    let now = clock();
                    if let PrepareDisposition::Prepare(ticket) =
                        self.owners[index].policy.try_prepare(
                            now,
                            Observation::Complete {
                                observed_at: now,
                                context: &binding,
                                members: &keys,
                            },
                        )
                    {
                        let rule = self.owners[index].rule.clone();
                        let owner = self.owners[index].id.clone();
                        let Some(identity) = self.owners[index].identity.clone() else {
                            self.owners[index].policy.failed(
                                now,
                                &ticket,
                                FailureKind::UnsupportedContext,
                            );
                            self.owners[index]
                                .policy
                                .cleanup_ack(&ticket, CleanupOutcome::NoChildCreated);
                            continue;
                        };
                        let phase =
                            match self.send(Work::Prepare(rule, P::binding(&context), identity)) {
                                Ok(()) => Phase::Preparing,
                                Err((error, _)) => {
                                    self.owners[index].launch_failed(
                                        now,
                                        &ticket,
                                        Failure {
                                            message: error,
                                            kind: FailureKind::Retryable,
                                        },
                                        log,
                                    );
                                    self.owners[index]
                                        .policy
                                        .cleanup_ack(&ticket, CleanupOutcome::NoChildCreated);
                                    continue;
                                }
                            };
                        self.flight = Some(InFlight {
                            owner,
                            ticket,
                            phase,
                        });
                        break;
                    }
                }
            }
        } else if let Err(error) = context {
            if enabled {
                self.pause_all(PauseReason::UnsupportedContext);
                for owner in self
                    .owners
                    .iter_mut()
                    .filter(|owner| owner.active && owner.rule.enabled)
                {
                    owner.last_error = Some(error.message.clone());
                }
            }
        }
        let now = clock();
        let mut snapshot = KeepRunningSnapshot {
            enabled,
            observation_known: known,
            ..Default::default()
        };
        let flight_owner = self.flight.as_ref().map(|flight| &flight.owner);
        for owner in self.owners.iter_mut().filter(|owner| {
            owner.active || flight_owner.is_some_and(|id| Arc::ptr_eq(id, &owner.id))
        }) {
            let state = owner.policy.state();
            if state == KeepRunningState::Disabled
                && !flight_owner.is_some_and(|id| Arc::ptr_eq(id, &owner.id))
            {
                owner.last_error = None;
            }
            if !matches!(state, KeepRunningState::Retrying { .. }) {
                owner.retry_error = None;
            }
            let published = (state, owner.last_error.clone());
            if owner.published.as_ref() != Some(&published) {
                if let Some(error) = &owner.last_error {
                    log.record(
                        ActionLogFeature::KeepRunning,
                        None,
                        &owner.rule.executable_path,
                        ActionLogResult::Failed,
                        error,
                    );
                }
                owner.published = Some(published);
            }
            let last_error = owner.last_error.as_ref().or(owner.retry_error.as_ref());
            if snapshot.last_error.is_none() {
                snapshot.last_error = last_error.cloned();
            }
            if !owner.active {
                continue;
            }
            snapshot.rules.push(KeepRunningRuleSnapshot {
                executable_path: owner.rule.executable_path.clone(),
                state,
                attempts: owner.policy.attempts(),
                next_retry_seconds: if let KeepRunningState::Retrying { next_attempt } = state {
                    Some(next_attempt.saturating_duration_since(now).as_secs())
                } else {
                    None
                },
                last_error: last_error.cloned(),
            });
        }
        snapshot
    }

    pub(super) fn shutdown(&mut self) {
        for owner in &mut self.owners {
            owner.policy.disable();
        }
        if let Some(flight) = self.flight.take() {
            if let Phase::Ready(launch) | Phase::CleanupRetry(launch) = flight.phase {
                let _ = self.send(Work::Dispose(launch));
            }
        }
        // Never join a potentially blocked Win32 call. Its owned job survives until worker cleanup/process exit.
        self.worker = None;
    }
}

impl<P: Platform> Drop for Coordinator<P> {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub(super) struct Windows;

mod native;

#[cfg(test)]
mod tests;
