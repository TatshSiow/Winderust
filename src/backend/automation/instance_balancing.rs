use super::*;
use crate::{
    action_log::{ActionLogFeature, ActionLogResult},
    config::{CpuAllocationSettings, InstanceBalancingRule, InstanceBalancingSettings},
    control::{
        cpu_allocation::{
            CpuAllocationApplyError, CpuAllocationApplyOutcome, CpuAllocationClaim,
            CpuAllocationPlatform, CpuAllocationReconciliationSummary, CpuAllocationReleaseSummary,
            CpuAllocationRequest, CpuSetInventory, InstanceBalancingEligibilityError,
        },
        instance_limits::{self as membership, Member, MemberState, MembershipObservation},
        process::{ControlOwner, ProcessControlError, ProcessControlTarget, ProcessTargetKey},
    },
    cpu_topology::CpuTopology,
    features::cpu_control::instance_balancing::{Assignment, Cohort, PlanError, Policy},
    foreground::same_executable_path,
};
use rust_i18n::t;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct InstanceBalancingSnapshot {
    pub(crate) enabled: bool,
    pub(crate) process_activity_known: bool,
    pub(crate) scanned_processes: usize,
    pub(crate) adjusted_processes: usize,
    pub(crate) skipped_processes: usize,
    pub(crate) failed_processes: usize,
    pub(crate) detail: Option<InstanceBalancingDetail>,
    pub(crate) exclusions: Vec<InstanceBalancingExclusion>,
    pub(crate) last_error: Option<String>,
}

impl InstanceBalancingSnapshot {
    pub(crate) fn message(&self) -> Option<String> {
        self.detail
            .as_ref()
            .map(|detail| detail.message(&self.exclusions))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InstanceBalancingDetail {
    Disabled,
    WaitingForContext,
    WaitingForInstances,
    WaitingForStableCohort,
    InsufficientCapacity,
    Assigned(usize),
    Diagnostic(String),
}

impl InstanceBalancingDetail {
    fn message(&self, exclusions: &[InstanceBalancingExclusion]) -> String {
        let mut message = match self {
            Self::Disabled => t!("instance_balancing.disabled"),
            Self::WaitingForContext => t!("instance_balancing.waiting_context"),
            Self::WaitingForInstances => t!("instance_balancing.waiting"),
            Self::WaitingForStableCohort => t!("instance_balancing.waiting_stable"),
            Self::InsufficientCapacity => t!("instance_balancing.insufficient_capacity"),
            Self::Assigned(count) => t!("instance_balancing.assigned", count = count),
            Self::Diagnostic(error) => std::borrow::Cow::Borrowed(error.as_str()),
        }
        .into_owned();
        if !exclusions.is_empty() {
            let details = exclusions
                .iter()
                .map(InstanceBalancingExclusion::message)
                .collect::<Vec<_>>();
            message.push(' ');
            message.push_str(&details.join("; "));
        }
        message
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InstanceBalancingExclusion {
    ExplicitSelection { process_name: String, pid: u32 },
    Diagnostic(String),
}

impl InstanceBalancingExclusion {
    fn is_explicit_selection(&self) -> bool {
        matches!(self, Self::ExplicitSelection { .. })
    }

    fn message(&self) -> String {
        match self {
            Self::ExplicitSelection { process_name, pid } => format!(
                "{process_name} ({pid}): {}",
                t!("instance_balancing.explicit_selection")
            ),
            Self::Diagnostic(message) => message.clone(),
        }
    }
}

#[derive(Default)]
pub(super) struct PendingResets {
    pub(super) all: bool,
    pub(super) paths: Vec<String>,
}

pub(super) fn record_settings_changes(state: &mut AutomationWorkerState, settings: &Settings) {
    let previous = &state.settings;
    let pending = &mut state.pending_instance_balancing_resets;
    pending.all |= previous.general.enabled != settings.general.enabled
        || previous.instance_balancing.enabled != settings.instance_balancing.enabled;
    for (rules, other) in [
        (
            &previous.instance_balancing.rules,
            &settings.instance_balancing.rules,
        ),
        (
            &settings.instance_balancing.rules,
            &previous.instance_balancing.rules,
        ),
    ] {
        for rule in rules.iter().filter(|rule| !other.contains(rule)) {
            if !pending.paths.contains(&rule.executable_path) {
                pending.paths.push(rule.executable_path.clone());
            }
        }
    }
}

struct Rule {
    settings: InstanceBalancingRule,
    generation: Option<u64>,
    policy: Policy,
    retained: BTreeMap<ProcessTargetKey, Member>,
    exited: BTreeMap<ProcessTargetKey, ProcessControlTarget>,
    accepted: BTreeSet<ProcessTargetKey>,
    last_action: Option<(
        ActionLogResult,
        InstanceBalancingDetail,
        Vec<InstanceBalancingExclusion>,
    )>,
}

#[derive(Default)]
pub(super) struct Runtime {
    enabled: bool,
    next_generation: u64,
    rules: Vec<Rule>,
    controls: Option<(CpuAllocationSettings, CpuAllocationSettings, bool)>,
    dirty: bool,
    status_after_release: Option<InstanceBalancingSnapshot>,
}

impl Runtime {
    pub(super) fn sync(
        &mut self,
        settings: &InstanceBalancingSettings,
        master_enabled: bool,
        resets: PendingResets,
    ) {
        let enabled = master_enabled && settings.enabled;
        let reset_all = resets.all || enabled != self.enabled;
        if !reset_all
            && resets.paths.is_empty()
            && self
                .rules
                .iter()
                .map(|rule| &rule.settings)
                .eq(settings.rules.iter())
        {
            return;
        }
        self.dirty = true;
        let mut previous = std::mem::take(&mut self.rules);
        self.rules = settings
            .rules
            .iter()
            .map(|settings| {
                if !reset_all && !resets.paths.contains(&settings.executable_path) {
                    if let Some(index) = previous.iter().position(|rule| rule.settings == *settings)
                    {
                        return previous.remove(index);
                    }
                }
                let generation = self.next_generation.checked_add(1);
                if let Some(generation) = generation {
                    self.next_generation = generation;
                }
                Rule {
                    settings: settings.clone(),
                    generation,
                    policy: Policy::default(),
                    retained: BTreeMap::new(),
                    exited: BTreeMap::new(),
                    accepted: BTreeSet::new(),
                    last_action: None,
                }
            })
            .collect();
        self.enabled = enabled;
    }

    pub(super) fn invalidate(&mut self) {
        self.dirty |= (self.enabled && self.rules.iter().any(|rule| rule.settings.enabled))
            || self.rules.iter().any(|rule| !rule.accepted.is_empty());
        for rule in &mut self.rules {
            rule.policy.invalidate();
            rule.accepted.clear();
        }
    }

    pub(super) fn active(&self) -> bool {
        self.dirty || self.rules.iter().any(|rule| !rule.accepted.is_empty())
    }

    pub(super) fn take_settled_release_status(
        &mut self,
        coordinator: &CpuAllocationCoordinator,
    ) -> Option<InstanceBalancingSnapshot> {
        if self.status_after_release.is_some()
            && coordinator.policy_managed_process_count(ControlOwner::InstanceBalancing) == 0
        {
            return self.status_after_release.take();
        }
        None
    }

    fn remember_release_only_status(&mut self, snapshot: &InstanceBalancingSnapshot) {
        if snapshot.last_error.is_some() {
            self.status_after_release = Some(InstanceBalancingSnapshot {
                enabled: snapshot.enabled,
                detail: snapshot.detail.clone(),
                ..Default::default()
            });
        }
    }

    fn accepted_targets(&self) -> BTreeSet<ProcessTargetKey> {
        self.rules
            .iter()
            .flat_map(|rule| rule.accepted.iter().cloned())
            .collect()
    }
}

impl Rule {
    fn observe_members(&mut self, observation: MembershipObservation) -> Result<(), String> {
        let (members, mut issue) = match observation {
            MembershipObservation::Complete(members) => (members, None),
            MembershipObservation::Incomplete { members, reason } => (members, Some(reason)),
            MembershipObservation::Unavailable { reason } => (Vec::new(), Some(reason)),
            MembershipObservation::Conflict => (
                Vec::new(),
                Some(t!("instance_balancing.conflict").to_string()),
            ),
        };
        let mut seen = BTreeSet::new();
        for member in members {
            let target = target(&member)?;
            seen.insert(target.key());
            self.retained.insert(target.key(), member);
        }
        self.retained
            .retain(|key, member| match membership::member_state(member) {
                Ok(state @ (MemberState::Exited | MemberState::OutOfScope)) => {
                    if state == MemberState::Exited {
                        match target(member) {
                            Ok(target) => {
                                self.exited.insert(target.key(), target);
                            }
                            Err(reason) => {
                                issue.get_or_insert(reason);
                            }
                        }
                    }
                    if seen.contains(key) {
                        issue.get_or_insert_with(|| t!("instance_balancing.changed").to_string());
                    }
                    false
                }
                Ok(MemberState::Current) => {
                    if !seen.contains(key) {
                        issue.get_or_insert_with(|| {
                            t!("instance_balancing.missing_live").to_string()
                        });
                    }
                    true
                }
                Err(reason) => {
                    issue.get_or_insert(reason);
                    true
                }
            });
        issue.map_or(Ok(()), Err)
    }
}

fn target(member: &Member) -> Result<ProcessControlTarget, String> {
    let path = PathBuf::from(&member.path);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "The observed process image name is unavailable.".to_owned())?
        .to_owned();
    Ok(ProcessControlTarget::automatic(
        member.id,
        name,
        path,
        member.creation_time,
    ))
}

fn explicit_selection(explicit_paths: &[String], path: &str) -> bool {
    explicit_paths
        .iter()
        .any(|selected| same_executable_path(Path::new(selected), Path::new(path)))
}

fn known_exclusion(error: &CpuAllocationApplyError) -> bool {
    matches!(
        error,
        CpuAllocationApplyError::Target(ProcessControlError::AccessDenied(_))
            | CpuAllocationApplyError::Eligibility(
                InstanceBalancingEligibilityError::ExternalAssignment
                    | InstanceBalancingEligibilityError::ExternalChange
                    | InstanceBalancingEligibilityError::Suppressed
            )
    )
}

fn prepare_cohort<P: CpuAllocationPlatform>(
    coordinator: &mut CpuAllocationCoordinator<P>,
    inventory: &mut CpuSetInventory,
    topology: &CpuTopology,
    rule: &Rule,
    settings: &Settings,
) -> Result<(Cohort, Vec<InstanceBalancingExclusion>), String> {
    let generation = rule
        .generation
        .ok_or_else(|| "Instance Balancing exhausted its rule generations.".to_owned())?;
    let explicit_paths = explicit_cpu_allocation_paths(settings);
    let mut members = Vec::new();
    let mut excluded = Vec::new();
    for member in rule.retained.values() {
        if membership::member_state(member)? != MemberState::Current {
            return Err(t!("instance_balancing.changed").to_string());
        }
        let target = target(member)?;
        if explicit_selection(&explicit_paths, &member.path) {
            excluded.push(InstanceBalancingExclusion::ExplicitSelection {
                process_name: target.name,
                pid: target.id,
            });
            continue;
        }
        match coordinator.instance_balancing_allowed_processors(
            inventory,
            topology,
            &target,
            generation,
            settings.general.allow_cross_session_process_control,
        ) {
            Ok(allowed) => members.push((target, allowed)),
            Err(error) if known_exclusion(&error) => {
                excluded.push(InstanceBalancingExclusion::Diagnostic(format!(
                    "{} ({}): {error}",
                    target.name, target.id
                )))
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok((Cohort::new(members), excluded))
}

fn context_is_current(
    shared: &SharedAutomationState,
    configured: &Arc<Settings>,
    active: &Settings,
) -> bool {
    let current = active_power_source_settings(configured, power_source::is_plugged_in());
    if current.general.enabled != active.general.enabled
        || current.general.allow_cross_session_process_control
            != active.general.allow_cross_session_process_control
        || current.cpu_sets_soft != active.cpu_sets_soft
        || current.processor_affinity_hard != active.processor_affinity_hard
    {
        return false;
    }
    {
        let state = lock_unpoisoned(&shared.state);
        if state.stop_requested
            || !Arc::ptr_eq(&state.settings, configured)
            || !state.windows_event_watcher_active
            || state.pending_events.suspend_resume
            || state.pending_events.session_changed
            || state.pending_events.windows_event_watcher_changed
            || state.pending_instance_balancing_resets.all
            || !state.pending_instance_balancing_resets.paths.is_empty()
        {
            return false;
        }
    }
    true
}

struct PlanRejection {
    outcome: ActionLogResult,
    reason: String,
}

impl From<String> for PlanRejection {
    fn from(reason: String) -> Self {
        Self {
            outcome: ActionLogResult::Skipped,
            reason,
        }
    }
}

fn apply_plan<P: CpuAllocationPlatform>(
    coordinator: &mut CpuAllocationCoordinator<P>,
    inventory: &mut CpuSetInventory,
    topology: &CpuTopology,
    assignments: &[Assignment],
    generation: u64,
    allow_cross_session: bool,
    mut authorize: impl FnMut(&ProcessControlTarget) -> Result<(), String>,
) -> Result<(), PlanRejection> {
    for assignment in assignments {
        authorize(&assignment.target)?;
        let result = coordinator
            .apply_policy_claim(
                inventory,
                Ok(topology),
                CpuAllocationClaim {
                    target: assignment.target.clone(),
                    owner: ControlOwner::InstanceBalancing,
                    request: CpuAllocationRequest::SoftCpuSets {
                        selection: assignment.selection.clone(),
                    },
                    balancing_generation: Some(generation),
                },
                allow_cross_session,
            )
            .map_err(|error| PlanRejection {
                outcome: match error {
                    CpuAllocationApplyError::Target(ProcessControlError::Failed(_)) => {
                        ActionLogResult::Failed
                    }
                    _ => ActionLogResult::Skipped,
                },
                reason: error.to_string(),
            })?;
        if !matches!(
            result,
            CpuAllocationApplyOutcome::Applied | CpuAllocationApplyOutcome::Unchanged
        ) {
            return Err(t!("instance_balancing.not_accepted").to_string().into());
        }
    }
    Ok(())
}

fn release<P: CpuAllocationPlatform>(
    runtime: &Runtime,
    coordinator: &mut CpuAllocationCoordinator<P>,
    log: &mut ActionLog,
) -> Option<String> {
    let summary = coordinator
        .release_policy_except(ControlOwner::InstanceBalancing, &runtime.accepted_targets());
    record_release(summary, log)
}

fn record_release(summary: CpuAllocationReleaseSummary, log: &mut ActionLog) -> Option<String> {
    let errors = summary
        .failures
        .iter()
        .filter(|failure| failure.owner == ControlOwner::InstanceBalancing)
        .map(|failure| failure.error.to_string())
        .collect::<Vec<_>>();
    cpu_allocation::record_cpu_allocation_reconciliation(
        CpuAllocationReconciliationSummary {
            releases: summary,
            ..Default::default()
        },
        log,
    );
    (!errors.is_empty()).then(|| errors.join("; "))
}

impl RuntimeCore {
    pub(super) fn run_instance_balancing_update(
        &mut self,
        shared: &SharedAutomationState,
        configured: &Arc<Settings>,
        active: &Settings,
        watcher_active: bool,
    ) -> InstanceBalancingSnapshot {
        let runtime = &mut self.instance_balancing;
        runtime.status_after_release = None;
        let controls = (
            &active.cpu_sets_soft,
            &active.processor_affinity_hard,
            active.general.allow_cross_session_process_control,
        );
        match runtime.controls.as_ref() {
            Some((cpu_sets, affinity, cross_session))
                if (cpu_sets, affinity, *cross_session) == controls => {}
            previous => {
                if previous.is_some() {
                    runtime.invalidate();
                }
                runtime.controls = Some((controls.0.clone(), controls.1.clone(), controls.2));
            }
        }
        let mut snapshot = InstanceBalancingSnapshot {
            enabled: runtime.enabled,
            detail: Some(InstanceBalancingDetail::WaitingForInstances),
            ..Default::default()
        };
        if !runtime.enabled || !active.general.enabled || !watcher_active {
            runtime.invalidate();
        }
        snapshot.last_error = release(
            runtime,
            &mut self.cpu_allocation_coordinator,
            &mut self.action_log,
        );
        runtime.dirty = false;
        if !runtime.enabled || !active.general.enabled || !watcher_active {
            snapshot.detail = Some(if runtime.enabled && active.general.enabled {
                InstanceBalancingDetail::WaitingForContext
            } else {
                InstanceBalancingDetail::Disabled
            });
            runtime.remember_release_only_status(&snapshot);
            return snapshot;
        }
        let topology = cpu_allocation::processor_topology();
        let enabled = runtime
            .rules
            .iter()
            .enumerate()
            .filter_map(|(index, rule)| rule.settings.enabled.then_some(index))
            .collect::<Vec<_>>();
        let paths = enabled
            .iter()
            .map(|index| runtime.rules[*index].settings.executable_path.clone())
            .collect::<Vec<_>>();
        let observations = membership::collect_members(&paths);
        if observations.len() != enabled.len() {
            runtime.invalidate();
            snapshot.last_error =
                Some("Instance Balancing received a mismatched collector result.".into());
            if let Some(error) = release(
                runtime,
                &mut self.cpu_allocation_coordinator,
                &mut self.action_log,
            ) {
                snapshot.last_error = Some(error);
            }
            runtime.dirty = false;
            return snapshot;
        }
        let last_rule = enabled.last().copied();
        let rule_count = enabled.len();
        let mut observed_rules = 0;
        for (index, observation) in enabled.into_iter().zip(observations) {
            let rule = &mut runtime.rules[index];
            if !rule.settings.enabled {
                rule.accepted.clear();
                continue;
            }
            let mut inventory = CpuSetInventory::default();
            let mut observed_counts = None;
            let mut exclusions = Vec::new();
            let mut waiting = InstanceBalancingDetail::WaitingForStableCohort;
            let result = (|| -> Result<Option<Vec<Assignment>>, PlanRejection> {
                rule.observe_members(observation)?;
                let topology = topology.as_ref().map_err(Clone::clone)?;
                rule.policy
                    .check_topology(topology)
                    .map_err(|error| error.to_string())?;
                let (cohort, excluded) = prepare_cohort(
                    &mut self.cpu_allocation_coordinator,
                    &mut inventory,
                    topology,
                    rule,
                    active,
                )?;
                let counts = (rule.retained.len(), excluded.len());
                observed_counts = Some(counts);
                exclusions = excluded;
                let now = Instant::now();
                let plan = match rule.policy.observe(topology, &cohort, now) {
                    Err(PlanError::WaitingForInstances) => {
                        waiting = InstanceBalancingDetail::WaitingForInstances;
                        return Ok(None);
                    }
                    Err(PlanError::InsufficientCapacity) => {
                        waiting = InstanceBalancingDetail::InsufficientCapacity;
                        return Ok(None);
                    }
                    result => result.map_err(|error| error.to_string())?,
                };
                let Some(plan) = plan else {
                    return Ok(None);
                };
                // Report complete coverage only after the additional preflight agrees.
                observed_counts = None;
                // The second catalog and fresh allocation facts reserve only this exact observed cohort.
                let mut fresh = membership::collect_members(std::slice::from_ref(
                    &rule.settings.executable_path,
                ));
                let observation = fresh.pop().ok_or_else(|| {
                    "Instance Balancing lost its membership observation.".to_owned()
                })?;
                rule.observe_members(observation)?;
                let fresh_topology = cpu_allocation::processor_topology()?;
                rule.policy
                    .check_topology(&fresh_topology)
                    .map_err(|error| error.to_string())?;
                let mut final_inventory = CpuSetInventory::default();
                let (fresh_cohort, fresh_excluded) = prepare_cohort(
                    &mut self.cpu_allocation_coordinator,
                    &mut final_inventory,
                    topology,
                    rule,
                    active,
                )?;
                if fresh_cohort != cohort || fresh_excluded != exclusions {
                    return Err(t!("instance_balancing.changed").to_string().into());
                }
                observed_counts = Some(counts);
                apply_plan(
                    &mut self.cpu_allocation_coordinator,
                    &mut final_inventory,
                    topology,
                    &plan,
                    rule.generation
                        .ok_or_else(|| "Instance Balancing has no rule generation.".to_owned())?,
                    active.general.allow_cross_session_process_control,
                    |target| {
                        let member = rule
                            .retained
                            .get(&target.key())
                            .ok_or_else(|| t!("instance_balancing.changed").to_string())?;
                        if membership::member_state(member)? != MemberState::Current {
                            return Err(t!("instance_balancing.changed").to_string());
                        }
                        if !context_is_current(shared, configured, active) {
                            return Err(t!("instance_balancing.changed").to_string());
                        }
                        Ok(())
                    },
                )?;
                rule.policy.accepted(plan.clone(), Instant::now());
                Ok(Some(plan))
            })();
            if let Some((scanned, skipped)) = observed_counts {
                observed_rules += 1;
                snapshot.scanned_processes += scanned;
                snapshot.skipped_processes += skipped;
            }
            rule.exited.retain(|_, target| {
                !self
                    .cpu_allocation_coordinator
                    .retire_exited_instance_balancing(
                        target,
                        active.general.allow_cross_session_process_control,
                    )
            });
            let (outcome, detail) = match result {
                Ok(Some(plan)) => {
                    rule.accepted = plan
                        .iter()
                        .map(|assignment| assignment.target.key())
                        .collect();
                    snapshot.adjusted_processes += plan.len();
                    (
                        ActionLogResult::Applied,
                        InstanceBalancingDetail::Assigned(plan.len()),
                    )
                }
                Ok(None) => {
                    rule.accepted.clear();
                    (ActionLogResult::Skipped, waiting)
                }
                Err(PlanRejection { outcome, reason }) => {
                    rule.accepted.clear();
                    rule.policy.invalidate();
                    snapshot.failed_processes += 1;
                    snapshot.last_error = Some(reason.clone());
                    (outcome, InstanceBalancingDetail::Diagnostic(reason))
                }
            };
            if rule.last_action.as_ref().is_none_or(
                |(previous_outcome, previous_detail, previous_exclusions)| {
                    *previous_outcome != outcome
                        || previous_detail != &detail
                        || previous_exclusions != &exclusions
                },
            ) {
                self.action_log.record(
                    ActionLogFeature::InstanceBalancing,
                    None,
                    &rule.settings.executable_path,
                    outcome,
                    detail.message(&exclusions),
                );
                rule.last_action = Some((outcome, detail.clone(), exclusions.clone()));
            }
            snapshot.detail = Some(detail);
            if Some(index) != last_rule {
                exclusions.retain(InstanceBalancingExclusion::is_explicit_selection);
            }
            snapshot.exclusions.extend(exclusions);
            if let Some(error) = release(
                runtime,
                &mut self.cpu_allocation_coordinator,
                &mut self.action_log,
            ) {
                snapshot.last_error = Some(error);
            }
        }
        snapshot.process_activity_known = observed_rules != 0 && observed_rules == rule_count;
        if rule_count == 0 {
            runtime.remember_release_only_status(&snapshot);
        }
        let assigned = snapshot.adjusted_processes;
        if let Some(error) = &snapshot.last_error {
            snapshot.detail = Some(InstanceBalancingDetail::Diagnostic(error.clone()));
        } else if assigned > 0 {
            snapshot.detail = Some(InstanceBalancingDetail::Assigned(assigned));
        }
        if assigned > 0 {
            snapshot
                .exclusions
                .retain(InstanceBalancingExclusion::is_explicit_selection);
        } else if snapshot.last_error.is_some() {
            snapshot.exclusions.clear();
        }
        snapshot
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod intake_tests;

#[cfg(test)]
mod native_tests;
