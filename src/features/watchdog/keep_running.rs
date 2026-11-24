//! Per-rule policy only. Native ownership, the shared worker slot and intake ordering belong to
//! the runtime adapter; a complete observation here is caller-verified evidence, not a scan.
use std::{
    collections::BTreeSet,
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{config::KeepRunningRule, control::process::ProcessTargetKey};

const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(30),
];
const STABLE_RUNTIME: Duration = Duration::from_secs(60);
const PREPARATION_TIMEOUT: Duration = Duration::from_secs(5);

/// Caller-canonicalized user/logon/session binding, separate from process-instance identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesktopBindingKey(String);

impl DesktopBindingKey {
    pub(crate) fn new(canonical_key: String) -> Self {
        Self(canonical_key)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LaunchTicket(Arc<()>);

impl PartialEq for LaunchTicket {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for LaunchTicket {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PauseReason {
    IntentionalStop,
    ResourceThresholds,
    Suspend,
    SessionChanged,
    UnsupportedContext,
    RetryBudgetExhausted,
    PreparationTimedOut,
    CleanupUnconfirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeepRunningState {
    Disabled,
    WaitingForApp,
    Armed,
    Retrying { next_attempt: Instant },
    Starting,
    Paused(PauseReason),
}

impl KeepRunningState {
    pub(crate) fn label_key(self) -> &'static str {
        match self {
            Self::Disabled => "common.off",
            Self::WaitingForApp => "keep_running.waiting",
            Self::Armed => "keep_running.armed",
            Self::Retrying { .. } => "keep_running.retrying",
            Self::Starting => "keep_running.starting",
            Self::Paused(reason) => match reason {
                PauseReason::IntentionalStop => "keep_running.paused_stop",
                PauseReason::ResourceThresholds => "keep_running.paused_resource_thresholds",
                PauseReason::Suspend => "keep_running.paused_suspend",
                PauseReason::SessionChanged => "keep_running.paused_session",
                PauseReason::UnsupportedContext => "keep_running.paused_context",
                PauseReason::RetryBudgetExhausted => "keep_running.paused_budget",
                PauseReason::PreparationTimedOut => "keep_running.paused_timeout",
                PauseReason::CleanupUnconfirmed => "keep_running.paused_cleanup",
            },
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct KeepRunningSnapshot {
    pub(crate) enabled: bool,
    pub(crate) observation_known: bool,
    pub(crate) rules: Vec<KeepRunningRuleSnapshot>,
    pub(crate) last_error: Option<String>,
}

impl KeepRunningSnapshot {
    pub(crate) fn rule_status(&self, rule: &KeepRunningRule) -> Option<&KeepRunningRuleSnapshot> {
        self.rules.iter().find(|entry| entry.rule.as_ref() == rule)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct KeepRunningRuleSnapshot {
    pub(crate) rule: Arc<KeepRunningRule>,
    pub(crate) state: KeepRunningState,
    pub(crate) attempts: usize,
    pub(crate) next_retry_seconds: Option<u64>,
    pub(crate) last_error: Option<String>,
}

impl fmt::Debug for KeepRunningRuleSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Launch arguments and working directory are not status diagnostics.
        f.debug_struct("KeepRunningRuleSnapshot")
            .field("executable_path", &self.rule.executable_path)
            .field("state", &self.state)
            .field("attempts", &self.attempts)
            .field("next_retry_seconds", &self.next_retry_seconds)
            .field("last_error", &self.last_error)
            .finish()
    }
}

#[derive(Clone, Copy)]
/// Eligible members are caller-verified for this rule and exclude its owned prepared/provisional child.
pub(crate) enum Observation<'a> {
    Complete {
        observed_at: Instant,
        context: &'a DesktopBindingKey,
        members: &'a [ProcessTargetKey],
    },
    Partial,
    Unavailable,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CleanupAction {
    None,
    DisposeProvisional(LaunchTicket),
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PrepareDisposition {
    Wait,
    Prepare(LaunchTicket),
    DisposeProvisional(LaunchTicket),
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PreparedDisposition {
    Ready,
    DisposeProvisional,
    Ignore,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CommitDisposition {
    Reserved,
    Deferred,
    DisposeProvisional,
    Ignore,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AcceptanceDisposition {
    Accepted,
    Ignore,
}
#[derive(Clone, Copy)]
pub(crate) enum FailureKind {
    Retryable,
    UnsupportedContext,
}
#[derive(Clone, Copy)]
pub(crate) enum CleanupOutcome {
    Disposed,
    NoChildCreated,
    Unknown,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CleanupAcknowledgment {
    Released,
    Retained,
    Ignore,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AttemptPhase {
    Preparing,
    Canceled,
    Ready,
    Committed,
    Cleaning,
}
struct Attempt {
    ticket: LaunchTicket,
    intent_valid: bool,
    started: Instant,
    phase: AttemptPhase,
}

pub(crate) struct KeepRunningManager {
    state: KeepRunningState,
    binding: Option<DesktopBindingKey>,
    members: BTreeSet<ProcessTargetKey>,
    attempts: usize,
    stable_credit: Duration,
    stable_cohort: BTreeSet<ProcessTargetKey>,
    last_verified: Option<Instant>,
    attempt: Option<Attempt>,
}

impl Default for KeepRunningManager {
    fn default() -> Self {
        Self {
            state: KeepRunningState::Disabled,
            binding: None,
            members: BTreeSet::new(),
            attempts: 0,
            stable_credit: Duration::ZERO,
            stable_cohort: BTreeSet::new(),
            last_verified: None,
            attempt: None,
        }
    }
}

impl KeepRunningManager {
    pub(crate) fn state(&self) -> KeepRunningState {
        self.state
    }

    pub(crate) fn attempts(&self) -> usize {
        self.attempts
    }

    pub(crate) fn disposal_required(&self, ticket: &LaunchTicket) -> bool {
        self.attempt.as_ref().is_some_and(|attempt| {
            &attempt.ticket == ticket
                && matches!(
                    attempt.phase,
                    AttemptPhase::Canceled | AttemptPhase::Cleaning
                )
        })
    }

    pub(crate) fn enable(&mut self) {
        if self.state == KeepRunningState::Disabled {
            self.state = KeepRunningState::WaitingForApp;
        }
    }

    pub(crate) fn disable(&mut self) -> CleanupAction {
        let action = self.invalidate();
        self.drop_binding();
        self.state = KeepRunningState::Disabled;
        action
    }

    pub(crate) fn launch_settings_changed(&mut self) -> CleanupAction {
        let action = self.invalidate();
        self.drop_binding();
        if !matches!(
            self.state,
            KeepRunningState::Disabled | KeepRunningState::Paused(_)
        ) {
            self.state = KeepRunningState::WaitingForApp;
        }
        action
    }

    pub(crate) fn pause(&mut self, reason: PauseReason) -> CleanupAction {
        let action = self.invalidate();
        self.last_verified = None;
        if reason == PauseReason::SessionChanged {
            self.drop_binding();
        }
        if self.state != KeepRunningState::Disabled {
            self.state = KeepRunningState::Paused(reason);
        }
        action
    }

    pub(crate) fn resume(&mut self, now: Instant, verified_context: Option<&DesktopBindingKey>) {
        if !matches!(self.state, KeepRunningState::Paused(_)) {
            return;
        }
        let Some(binding) = self.binding.as_ref() else {
            self.state = KeepRunningState::WaitingForApp;
            return;
        };
        let Some(context) = verified_context else {
            return;
        };
        if context != binding {
            self.drop_binding();
            self.state = KeepRunningState::WaitingForApp;
            return;
        }
        self.attempts = 0;
        self.reset_stability();
        if self.members.is_empty() {
            self.schedule_retry(now);
        } else {
            self.state = KeepRunningState::Armed;
        }
    }

    pub(crate) fn observe(&mut self, now: Instant, observation: Observation<'_>) -> CleanupAction {
        if self.state == KeepRunningState::Disabled {
            return CleanupAction::None;
        }
        let Observation::Complete {
            observed_at,
            context,
            members,
        } = observation
        else {
            self.last_verified = None;
            return CleanupAction::None;
        };
        if observed_at != now {
            self.last_verified = None;
            return CleanupAction::None;
        }
        if self
            .binding
            .as_ref()
            .is_some_and(|binding| binding != context)
        {
            return self.pause(PauseReason::SessionChanged);
        }
        if members.is_empty() {
            self.last_verified = None;
            return CleanupAction::None;
        }
        if self.binding.is_none() {
            if matches!(self.state, KeepRunningState::Paused(_)) {
                return CleanupAction::None;
            }
            self.binding = Some(context.clone());
        }
        let verified: BTreeSet<_> = members.iter().cloned().collect();
        if self.stable_cohort.is_disjoint(&verified) {
            self.reset_stability();
        }
        if !matches!(self.state, KeepRunningState::Paused(_)) {
            if let Some(last) = self.last_verified {
                self.stable_credit = self
                    .stable_credit
                    .saturating_add(now.saturating_duration_since(last));
                if self.stable_credit >= STABLE_RUNTIME {
                    self.attempts = 0;
                }
            }
            self.last_verified = Some(now);
        }
        self.stable_cohort = verified;
        self.members.extend(members.iter().cloned());
        if matches!(self.state, KeepRunningState::Paused(_)) {
            return CleanupAction::None;
        }
        self.state = KeepRunningState::Armed;
        if self.attempt.as_ref().is_some_and(|attempt| {
            matches!(attempt.phase, AttemptPhase::Preparing | AttemptPhase::Ready)
        }) {
            return self.invalidate();
        }
        CleanupAction::None
    }

    /// Only positive exit evidence from the retained exact process handle may call this method.
    pub(crate) fn exited(&mut self, now: Instant, key: &ProcessTargetKey) {
        if !self.members.remove(key) {
            return;
        }
        if self.members.is_empty() {
            self.reset_stability();
            if self.state == KeepRunningState::Armed {
                self.schedule_retry(now);
            }
        }
    }

    /// Reserve the shared worker slot first; this spends the attempt before any native side effect.
    /// This per-rule owner cannot establish global capacity or serialize other rules.
    pub(crate) fn try_prepare(
        &mut self,
        now: Instant,
        observation: Observation<'_>,
    ) -> PrepareDisposition {
        if let CleanupAction::DisposeProvisional(ticket) = self.observe(now, observation) {
            return PrepareDisposition::DisposeProvisional(ticket);
        }
        let KeepRunningState::Retrying { next_attempt } = self.state else {
            return PrepareDisposition::Wait;
        };
        if now < next_attempt || self.attempt.is_some() || !self.fresh_absence(now, observation) {
            return PrepareDisposition::Wait;
        }
        let ticket = LaunchTicket(Arc::new(()));
        self.attempts += 1;
        self.attempt = Some(Attempt {
            ticket: ticket.clone(),
            intent_valid: true,
            started: now,
            phase: AttemptPhase::Preparing,
        });
        self.state = KeepRunningState::Starting;
        PrepareDisposition::Prepare(ticket)
    }

    pub(crate) fn prepared(&mut self, now: Instant, ticket: &LaunchTicket) -> PreparedDisposition {
        if let CleanupAction::DisposeProvisional(_) = self.timeout(now, ticket) {
            return PreparedDisposition::DisposeProvisional;
        }
        let valid = self.current(ticket);
        let Some(attempt) = self
            .attempt
            .as_mut()
            .filter(|attempt| &attempt.ticket == ticket)
        else {
            return PreparedDisposition::Ignore;
        };
        if !matches!(
            attempt.phase,
            AttemptPhase::Preparing | AttemptPhase::Canceled
        ) {
            return PreparedDisposition::Ignore;
        }
        if valid {
            attempt.phase = AttemptPhase::Ready;
            PreparedDisposition::Ready
        } else {
            attempt.phase = AttemptPhase::Cleaning;
            PreparedDisposition::DisposeProvisional
        }
    }

    /// The caller must hold the future shared intake reservation while supplying current_intents_clear.
    /// Reserving here is one-shot; later intents cannot turn this committed launch into stale cleanup.
    pub(crate) fn try_reserve_commit(
        &mut self,
        now: Instant,
        ticket: &LaunchTicket,
        observation: Observation<'_>,
        current_intents_clear: bool,
    ) -> CommitDisposition {
        if !self.attempt.as_ref().is_some_and(|attempt| {
            &attempt.ticket == ticket && attempt.phase == AttemptPhase::Ready
        }) {
            return CommitDisposition::Ignore;
        }
        if let CleanupAction::DisposeProvisional(_) = self.timeout(now, ticket) {
            return CommitDisposition::DisposeProvisional;
        }
        if !current_intents_clear {
            return CommitDisposition::Deferred;
        }
        if let CleanupAction::DisposeProvisional(_) = self.observe(now, observation) {
            return CommitDisposition::DisposeProvisional;
        }
        if !self.current(ticket) {
            return CommitDisposition::Ignore;
        }
        if !self.fresh_absence(now, observation) {
            return CommitDisposition::Deferred;
        }
        if let Some(attempt) = self.attempt.as_mut() {
            attempt.phase = AttemptPhase::Committed;
            CommitDisposition::Reserved
        } else {
            CommitDisposition::Ignore
        }
    }

    pub(crate) fn accepted(
        &mut self,
        ticket: &LaunchTicket,
        child: ProcessTargetKey,
    ) -> AcceptanceDisposition {
        if !self.attempt.as_ref().is_some_and(|attempt| {
            &attempt.ticket == ticket && attempt.phase == AttemptPhase::Committed
        }) {
            return AcceptanceDisposition::Ignore;
        }
        if self.current(ticket) {
            if self.members.is_empty() {
                self.reset_stability();
            }
            self.members.insert(child);
            self.state = KeepRunningState::Armed;
        }
        // Terminal native acceptance transfers ownership; late intents/results must never dispose it.
        self.attempt = None;
        AcceptanceDisposition::Accepted
    }

    pub(crate) fn failed(
        &mut self,
        now: Instant,
        ticket: &LaunchTicket,
        failure: FailureKind,
    ) -> CleanupAction {
        if let action @ CleanupAction::DisposeProvisional(_) = self.timeout(now, ticket) {
            return action;
        }
        let current = self.current(ticket);
        let Some(attempt) = self
            .attempt
            .as_mut()
            .filter(|attempt| &attempt.ticket == ticket)
        else {
            return CleanupAction::None;
        };
        if attempt.phase == AttemptPhase::Cleaning {
            return CleanupAction::None;
        }
        attempt.phase = AttemptPhase::Cleaning;
        if current {
            match failure {
                FailureKind::Retryable => {
                    if self.members.is_empty() {
                        self.schedule_retry(now);
                    } else {
                        self.state = KeepRunningState::Armed;
                    }
                }
                FailureKind::UnsupportedContext => {
                    self.pause(PauseReason::UnsupportedContext);
                }
            }
        }
        CleanupAction::DisposeProvisional(ticket.clone())
    }

    pub(crate) fn timeout(&mut self, now: Instant, ticket: &LaunchTicket) -> CleanupAction {
        if self.current(ticket)
            && self.attempt.as_ref().is_some_and(|attempt| {
                &attempt.ticket == ticket
                    && matches!(attempt.phase, AttemptPhase::Preparing | AttemptPhase::Ready)
                    && now.saturating_duration_since(attempt.started) >= PREPARATION_TIMEOUT
            })
        {
            return self.pause(PauseReason::PreparationTimedOut);
        }
        CleanupAction::None
    }

    pub(crate) fn cleanup_ack(
        &mut self,
        ticket: &LaunchTicket,
        outcome: CleanupOutcome,
    ) -> CleanupAcknowledgment {
        let Some(attempt) = self
            .attempt
            .as_ref()
            .filter(|attempt| &attempt.ticket == ticket)
        else {
            return CleanupAcknowledgment::Ignore;
        };
        if !matches!(
            attempt.phase,
            AttemptPhase::Cleaning | AttemptPhase::Canceled
        ) {
            return CleanupAcknowledgment::Ignore;
        }
        if matches!(outcome, CleanupOutcome::Unknown) {
            self.pause(PauseReason::CleanupUnconfirmed);
            return CleanupAcknowledgment::Retained;
        }
        self.attempt = None;
        CleanupAcknowledgment::Released
    }

    fn fresh_absence(&self, now: Instant, observation: Observation<'_>) -> bool {
        matches!(observation, Observation::Complete { observed_at, context, members }
            if observed_at == now && self.binding.as_ref() == Some(context) && members.is_empty() && self.members.is_empty())
    }

    fn current(&self, ticket: &LaunchTicket) -> bool {
        self.attempt
            .as_ref()
            .is_some_and(|attempt| &attempt.ticket == ticket && attempt.intent_valid)
    }

    fn invalidate(&mut self) -> CleanupAction {
        let Some(attempt) = self.attempt.as_mut() else {
            return CleanupAction::None;
        };
        // Never restore intent validity on Resume; only a newly issued ticket can be valid.
        attempt.intent_valid = false;
        match attempt.phase {
            AttemptPhase::Preparing => {
                attempt.phase = AttemptPhase::Canceled;
                CleanupAction::None
            }
            AttemptPhase::Ready => {
                attempt.phase = AttemptPhase::Cleaning;
                CleanupAction::DisposeProvisional(attempt.ticket.clone())
            }
            _ => CleanupAction::None,
        }
    }

    fn schedule_retry(&mut self, now: Instant) {
        self.state = match RETRY_DELAYS.get(self.attempts) {
            Some(delay) => KeepRunningState::Retrying {
                next_attempt: now + *delay,
            },
            None => KeepRunningState::Paused(PauseReason::RetryBudgetExhausted),
        };
    }

    fn reset_stability(&mut self) {
        self.stable_credit = Duration::ZERO;
        self.stable_cohort.clear();
        self.last_verified = None;
    }

    fn drop_binding(&mut self) {
        // Explicit disable/edit/session invalidation ends this binding; scan absence never does.
        self.binding = None;
        self.members.clear();
        self.attempts = 0;
        self.reset_stability();
    }
}

#[cfg(test)]
mod tests;
