use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{DefaultHasher, Hasher},
    mem::take,
};

use crate::{
    backend::crash_recovery::{
        forget_affinity_change, forget_cpu_sets_change, record_process_change, ProcessValue,
        RecoveryIntent,
    },
    cpu_topology::{CpuSelection, CpuTopology, ProcessorId, SelectionError},
    platform::windows::{cpu_allocation as windows_cpu_allocation, ProcessOperationError},
    win_util::WinHandle,
};

use super::process::{
    open_process_for_set_information, transition_failure_error, ControlOwner, ProcessControlError,
    ProcessControlTarget, ProcessIdentity, ProcessTargetKey,
};
use windows_cpu_allocation::CpuSetRecord;

mod eligibility;
mod instance_balancing;
mod restart_affinity;
pub(crate) use eligibility::EligibilityError as InstanceBalancingSelectionError;
pub(crate) use instance_balancing::InstanceBalancingEligibilityError;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CpuAllocationApplyError {
    #[error("{0}")]
    Topology(ProcessControlError),
    #[error("{0}")]
    Discovery(ProcessControlError),
    #[error("{0}")]
    Eligibility(InstanceBalancingEligibilityError),
    #[error("{0}")]
    Target(ProcessControlError),
}

impl From<ProcessControlError> for CpuAllocationApplyError {
    fn from(error: ProcessControlError) -> Self {
        Self::Target(error)
    }
}

#[derive(Default)]
pub(crate) struct CpuSetInventory {
    result: Option<Result<BTreeMap<ProcessorId, CpuSetRecord>, ProcessControlError>>,
}
impl CpuSetInventory {
    pub(crate) fn failed(&self) -> bool {
        matches!(self.result, Some(Err(_)))
    }
    fn ids<P: CpuAllocationPlatform>(
        &mut self,
        platform: &mut P,
        selection: &[ProcessorId],
    ) -> Result<Vec<u32>, ProcessControlError> {
        let entries = self.entries(platform)?;
        selection
            .iter()
            .map(|processor| {
                entries.get(processor).map(CpuSetRecord::id).ok_or_else(|| {
                    ProcessControlError::Failed(format!(
                        "CPU Sets (Soft) selection contains unavailable G{}:CPU #{}.",
                        processor.group, processor.number
                    ))
                })
            })
            .collect()
    }

    fn entries<P: CpuAllocationPlatform>(
        &mut self,
        platform: &mut P,
    ) -> Result<&BTreeMap<ProcessorId, CpuSetRecord>, ProcessControlError> {
        match self.result.get_or_insert_with(|| {
            let entries = platform.cpu_set_inventory()?;
            let mut processors = BTreeMap::new();
            let mut ids = BTreeSet::new();
            for entry in entries {
                let processor = entry.processor();
                if processor.number >= 64
                    || processors.insert(processor, entry).is_some()
                    || !ids.insert(entry.id())
                {
                    return Err(ProcessControlError::Failed(
                        "CPU Set discovery returned duplicate or invalid processor mappings."
                            .to_owned(),
                    ));
                }
            }
            Ok(processors)
        }) {
            Ok(entries) => Ok(entries),
            Err(error) => Err(error.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CpuAllocationRequest {
    SoftCpuSets {
        selection: CpuSelection,
    },
    HardAffinity {
        selection: CpuSelection,
    },
    AffinityLimit {
        selection: CpuSelection,
    },
    PreserveHardAffinity {
        selection: CpuSelection,
    },
    ReduceAffinity {
        selection: CpuSelection,
        processors_to_remove: u8,
        preserve_custom_affinity: bool,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct CpuAllocationClaim {
    pub(crate) target: ProcessControlTarget,
    pub(crate) owner: ControlOwner,
    pub(crate) request: CpuAllocationRequest,
    pub(crate) balancing_generation: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CpuAllocationApplyOutcome {
    Applied,
    Unchanged,
    Shadowed,
    NoUsableTarget,
}

#[derive(Debug)]
pub(crate) struct CpuAllocationReleaseFailure {
    pub(crate) owner: ControlOwner,
    pub(crate) process_id: u32,
    pub(crate) process_name: String,
    pub(crate) property: &'static str,
    pub(crate) error: ProcessControlError,
}

#[derive(Debug, Default)]
pub(crate) struct CpuAllocationReleaseSummary {
    pub(crate) restored_owners: Vec<ControlOwner>,
    pub(crate) failures: Vec<CpuAllocationReleaseFailure>,
}

#[derive(Debug)]
pub(crate) struct CpuAllocationReconciledApplication {
    pub(crate) owner: ControlOwner,
    pub(crate) process_id: u32,
    pub(crate) process_name: String,
    pub(crate) request: CpuAllocationRequest,
}

#[derive(Debug)]
pub(crate) struct CpuAllocationReconciliationFailure {
    pub(crate) owner: ControlOwner,
    pub(crate) process_id: u32,
    pub(crate) process_name: String,
    pub(crate) error: ProcessControlError,
}

#[derive(Debug, Default)]
pub(crate) struct CpuAllocationReconciliationSummary {
    pub(crate) applications: Vec<CpuAllocationReconciledApplication>,
    pub(crate) failures: Vec<CpuAllocationReconciliationFailure>,
    pub(crate) releases: CpuAllocationReleaseSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CpuAllocationClaimFingerprint {
    owner: ControlOwner,
    request: CpuAllocationRequest,
    balancing_generation: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingReconciliation {
    Handoff,
    HandoffRetry,
    ReleaseOnlyFirstAttempt {
        failed_claim: Option<CpuAllocationClaimFingerprint>,
    },
    ReleaseOnlyRetry {
        failed_claim: Option<CpuAllocationClaimFingerprint>,
    },
}

#[derive(Debug, Clone)]
struct ManagedValue<T> {
    baseline: T,
    expected: T,
    owner: ControlOwner,
    apply_sequence: u64,
    confirmed: bool,
}

pub(crate) trait CpuAllocationRecoveryIntent {
    fn commit(self) -> Result<(), String>;
    fn retain_pending(self) -> Result<(), String>;
}

impl CpuAllocationRecoveryIntent for RecoveryIntent {
    fn commit(self) -> Result<(), String> {
        RecoveryIntent::commit(self)
    }

    fn retain_pending(self) -> Result<(), String> {
        RecoveryIntent::retain_pending(self)
    }
}

pub(crate) trait CpuAllocationPlatform {
    type Process;
    type RecoveryIntent: CpuAllocationRecoveryIntent;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError>;
    fn query_affinity(
        &mut self,
        process: &Self::Process,
    ) -> Result<(usize, usize), ProcessControlError>;
    fn validate_group_zero_affinity(
        &mut self,
        process: &Self::Process,
    ) -> Result<(), ProcessControlError>;
    fn query_cpu_sets(&mut self, process: &Self::Process) -> Result<Vec<u32>, ProcessControlError>;
    fn cpu_set_inventory(&mut self) -> Result<Vec<CpuSetRecord>, ProcessControlError>;
    fn begin_affinity_change(
        &mut self,
        process: &Self::Process,
        original: usize,
        expected: usize,
    ) -> Result<Self::RecoveryIntent, ProcessControlError>;
    fn begin_cpu_sets_change(
        &mut self,
        process: &Self::Process,
        original: &[u32],
        expected: &[u32],
    ) -> Result<Self::RecoveryIntent, ProcessControlError>;
    fn apply_affinity(
        &mut self,
        process: &Self::Process,
        affinity: usize,
    ) -> Result<(), ProcessControlError>;
    fn apply_cpu_sets(
        &mut self,
        process: &Self::Process,
        ids: &[u32],
    ) -> Result<(), ProcessControlError>;
    fn relinquish_affinity(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<(), ProcessControlError>;
    fn relinquish_cpu_sets(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<(), ProcessControlError>;
}

#[derive(Default)]
pub(crate) struct WindowsCpuAllocationPlatform;

impl CpuAllocationPlatform for WindowsCpuAllocationPlatform {
    type Process = WinHandle;
    type RecoveryIntent = RecoveryIntent;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError> {
        open_process_for_set_information(target, allow_cross_session_process_control)
    }

    fn query_affinity(
        &mut self,
        process: &Self::Process,
    ) -> Result<(usize, usize), ProcessControlError> {
        windows_cpu_allocation::query_affinity(process).map_err(map_cpu_allocation_error)
    }

    fn validate_group_zero_affinity(
        &mut self,
        process: &Self::Process,
    ) -> Result<(), ProcessControlError> {
        windows_cpu_allocation::validate_group_zero_affinity(process)
            .map_err(map_cpu_allocation_error)
    }

    fn query_cpu_sets(&mut self, process: &Self::Process) -> Result<Vec<u32>, ProcessControlError> {
        windows_cpu_allocation::query_cpu_sets(process).map_err(map_cpu_allocation_error)
    }

    fn cpu_set_inventory(&mut self) -> Result<Vec<CpuSetRecord>, ProcessControlError> {
        windows_cpu_allocation::cpu_set_inventory().map_err(map_cpu_allocation_error)
    }

    fn begin_affinity_change(
        &mut self,
        process: &Self::Process,
        original: usize,
        expected: usize,
    ) -> Result<Self::RecoveryIntent, ProcessControlError> {
        record_process_change(
            process.raw(),
            ProcessValue::Affinity(original as u64),
            ProcessValue::Affinity(expected as u64),
        )
        .map_err(ProcessControlError::Failed)
    }

    fn begin_cpu_sets_change(
        &mut self,
        process: &Self::Process,
        original: &[u32],
        expected: &[u32],
    ) -> Result<Self::RecoveryIntent, ProcessControlError> {
        record_process_change(
            process.raw(),
            ProcessValue::CpuSets(original.to_vec()),
            ProcessValue::CpuSets(expected.to_vec()),
        )
        .map_err(ProcessControlError::Failed)
    }

    fn apply_affinity(
        &mut self,
        process: &Self::Process,
        affinity: usize,
    ) -> Result<(), ProcessControlError> {
        windows_cpu_allocation::set_affinity(process, affinity).map_err(map_cpu_allocation_error)
    }

    fn apply_cpu_sets(
        &mut self,
        process: &Self::Process,
        ids: &[u32],
    ) -> Result<(), ProcessControlError> {
        windows_cpu_allocation::set_cpu_sets(process, ids).map_err(map_cpu_allocation_error)
    }

    fn relinquish_affinity(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<(), ProcessControlError> {
        forget_affinity_change(identity.id, identity.creation_time)
            .map_err(ProcessControlError::Failed)
    }

    fn relinquish_cpu_sets(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<(), ProcessControlError> {
        forget_cpu_sets_change(identity.id, identity.creation_time)
            .map_err(ProcessControlError::Failed)
    }
}

pub(crate) struct CpuAllocationCoordinator<P: CpuAllocationPlatform = WindowsCpuAllocationPlatform>
{
    platform: P,
    claims: BTreeMap<ControlOwner, BTreeMap<ProcessTargetKey, CpuAllocationClaim>>,
    adaptive_episode_keys: BTreeSet<ProcessTargetKey>,
    managed_affinity: BTreeMap<ProcessIdentity, ManagedValue<usize>>,
    managed_cpu_sets: BTreeMap<ProcessIdentity, ManagedValue<Vec<u32>>>,
    pending_reconciliations: BTreeMap<ProcessTargetKey, PendingReconciliation>,
    next_apply_sequence: u64,
    instance_balancing: BTreeMap<ProcessIdentity, instance_balancing::GuardState>,
}

impl Default for CpuAllocationCoordinator {
    fn default() -> Self {
        Self::with_platform(WindowsCpuAllocationPlatform)
    }
}

impl<P: CpuAllocationPlatform> CpuAllocationCoordinator<P> {
    pub(crate) fn with_platform(platform: P) -> Self {
        Self {
            platform,
            claims: BTreeMap::new(),
            adaptive_episode_keys: BTreeSet::new(),
            managed_affinity: BTreeMap::new(),
            managed_cpu_sets: BTreeMap::new(),
            pending_reconciliations: BTreeMap::new(),
            next_apply_sequence: 1,
            instance_balancing: BTreeMap::new(),
        }
    }

    fn next_sequence(&mut self) -> u64 {
        let sequence = self.next_apply_sequence;
        self.next_apply_sequence = self.next_apply_sequence.wrapping_add(1).max(1);
        sequence
    }

    pub(crate) fn apply_policy_claim(
        &mut self,
        inventory: &mut CpuSetInventory,
        topology: Result<&CpuTopology, &str>,
        claim: CpuAllocationClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<CpuAllocationApplyOutcome, CpuAllocationApplyError> {
        validate_owner(claim.owner)?;
        instance_balancing::validate_claim(&claim).map_err(CpuAllocationApplyError::Eligibility)?;
        let owner = claim.owner;
        let key = claim.target.key();
        if let Some(generation) = claim.balancing_generation {
            self.register_instance_balancing_generation(&claim.target, generation)
                .map_err(CpuAllocationApplyError::Eligibility)?;
        }
        if let Err(mut error) = resolve_request(&claim, topology) {
            if let Some(claims) = self.claims.get_mut(&owner) {
                claims.remove(&key);
            }
            let mut release = CpuAllocationReleaseSummary::default();
            let mut invalid_owners = self
                .remove_unresolved_claims(&key, topology)
                .into_iter()
                .map(|(claim, _)| claim.owner)
                .collect::<BTreeSet<_>>();
            invalid_owners.insert(owner);
            if self.release_unresolved_assignment(&key, &invalid_owners, &mut release) {
                self.reconcile_policy_key(&key, &mut release);
            }
            if let Some(failure) = release.failures.first() {
                error = ProcessControlError::Failed(format!(
                    "{error} Restoration remains pending: {}",
                    failure.error
                ));
            }
            return Err(CpuAllocationApplyError::Topology(error));
        }
        let invalid_owners = self
            .remove_unresolved_claims(&key, topology)
            .into_iter()
            .map(|(claim, _)| claim.owner)
            .collect();
        let mut release = CpuAllocationReleaseSummary::default();
        if !self.release_unresolved_assignment(&key, &invalid_owners, &mut release) {
            return Err(CpuAllocationApplyError::Topology(
                ProcessControlError::Failed(
                    "CPU topology changed; restoration of the previous allocation remains pending."
                        .to_owned(),
                ),
            ));
        }
        let previous = self
            .claims
            .entry(owner)
            .or_default()
            .insert(key.clone(), claim);
        let effective = self.effective_claim(&key).cloned().ok_or_else(|| {
            ProcessControlError::Failed("CPU allocation claim arbitration failed.".to_owned())
        })?;
        if effective.owner != owner {
            return Ok(CpuAllocationApplyOutcome::Shadowed);
        }

        let withdraw_on_failure = owner == ControlOwner::InstanceBalancing
            || matches!(
                effective.request,
                CpuAllocationRequest::PreserveHardAffinity { .. }
                    | CpuAllocationRequest::ReduceAffinity { .. }
            );
        let failed_claim = claim_fingerprint(&effective);
        let mut result = self.apply_effective_claim(
            inventory,
            topology,
            effective,
            allow_cross_session_process_control,
        );
        if withdraw_on_failure && result.is_err() {
            if let Some(claims) = self.claims.get_mut(&owner) {
                claims.remove(&key);
            }
            let mut release = CpuAllocationReleaseSummary::default();
            self.reconcile_policy_key(&key, &mut release);
            if !release.failures.is_empty() {
                let detail = release
                    .failures
                    .iter()
                    .map(|failure| failure.error.to_string())
                    .collect::<Vec<_>>()
                    .join("; ");
                result = result.map_err(|error| match error {
                    CpuAllocationApplyError::Eligibility(reason) => {
                        CpuAllocationApplyError::Eligibility(
                            InstanceBalancingEligibilityError::RestorationPending {
                                reason: Box::new(reason),
                                detail,
                            },
                        )
                    }
                    error => CpuAllocationApplyError::Target(ProcessControlError::Failed(format!(
                        "{error} Restoration remains pending: {detail}"
                    ))),
                });
            }
        } else if result.is_err() {
            let owner_claims = self.claims.entry(owner).or_default();
            if let Some(previous) = previous {
                owner_claims.insert(key.clone(), previous);
            } else {
                owner_claims.remove(&key);
            }
            if self.pending_reconciliations.contains_key(&key) {
                self.pending_reconciliations.insert(
                    key,
                    if matches!(&result, Err(CpuAllocationApplyError::Discovery(_))) {
                        PendingReconciliation::HandoffRetry
                    } else {
                        PendingReconciliation::ReleaseOnlyFirstAttempt {
                            failed_claim: Some(failed_claim),
                        }
                    },
                );
            }
        } else {
            self.pending_reconciliations.remove(&key);
            if matches!(
                result,
                Ok(CpuAllocationApplyOutcome::Applied | CpuAllocationApplyOutcome::Unchanged)
            ) {
                if owner != ControlOwner::AdaptiveEngine {
                    self.withdraw_adaptive_episode_claim(&key);
                }
                self.adaptive_episode_keys.remove(&key);
            }
        }
        result
    }

    pub(crate) fn apply_adaptive_episode_claim(
        &mut self,
        inventory: &mut CpuSetInventory,
        topology: Result<&CpuTopology, &str>,
        claim: CpuAllocationClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<CpuAllocationApplyOutcome, CpuAllocationApplyError> {
        if claim.owner != ControlOwner::AdaptiveEngine {
            return Err(ProcessControlError::Failed(
                "Only Adaptive Engine can apply an episode-scoped CPU allocation.".to_owned(),
            )
            .into());
        }
        let key = claim.target.key();
        let result = self.apply_policy_claim(
            inventory,
            topology,
            claim,
            allow_cross_session_process_control,
        );
        // Keep the exact generation key for unsettled writes until the shared restore settles.
        self.adaptive_episode_keys.insert(key.clone());
        if matches!(
            result,
            Ok(CpuAllocationApplyOutcome::Applied | CpuAllocationApplyOutcome::Unchanged)
        ) && self.verified_adaptive_episode_placement(&key).is_none()
        {
            return Ok(CpuAllocationApplyOutcome::NoUsableTarget);
        }
        result
    }

    pub(crate) fn adaptive_episode_placement_processes(
        &self,
    ) -> impl Iterator<Item = &ProcessIdentity> + Clone {
        self.adaptive_episode_keys
            .iter()
            .filter_map(|key| self.verified_adaptive_episode_placement(key))
    }

    pub(crate) fn verified_adaptive_episode_placement(
        &self,
        key: &ProcessTargetKey,
    ) -> Option<&ProcessIdentity> {
        if !self.adaptive_episode_keys.contains(key)
            || self.pending_reconciliations.contains_key(key)
            || self
                .effective_claim(key)
                .is_none_or(|claim| claim.owner != ControlOwner::AdaptiveEngine)
        {
            return None;
        }
        let owner = ControlOwner::AdaptiveEngine;
        self.managed_affinity
            .iter()
            .find_map(|(identity, managed)| {
                (managed.owner == owner
                    && managed.confirmed
                    && managed.expected != managed.baseline
                    && identity.matches_target_key(key))
                .then_some(identity)
            })
            .or_else(|| {
                self.managed_cpu_sets
                    .iter()
                    .find_map(|(identity, managed)| {
                        (managed.owner == owner
                            && managed.confirmed
                            && managed.expected != managed.baseline
                            && identity.matches_target_key(key))
                        .then_some(identity)
                    })
            })
    }

    fn apply_effective_claim(
        &mut self,
        inventory: &mut CpuSetInventory,
        topology: Result<&CpuTopology, &str>,
        claim: CpuAllocationClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<CpuAllocationApplyOutcome, CpuAllocationApplyError> {
        let (identity, process) = match self
            .platform
            .open(&claim.target, allow_cross_session_process_control)
        {
            Ok(process) => process,
            Err(ProcessControlError::ProcessExited) => {
                let key = claim.target.key();
                self.instance_balancing
                    .retain(|identity, _| identity.key() != key);
                return Err(ProcessControlError::ProcessExited.into());
            }
            Err(error) => return Err(error.into()),
        };
        self.relinquish_stale_identities(&identity)?;
        instance_balancing::validate_claim(&claim).map_err(CpuAllocationApplyError::Eligibility)?;

        let request =
            resolve_request(&claim, topology).map_err(CpuAllocationApplyError::Topology)?;
        match request {
            ResolvedCpuAllocation::SoftCpuSets(processors) => {
                let desired = inventory
                    .ids(&mut self.platform, processors)
                    .map_err(CpuAllocationApplyError::Discovery)?;
                self.apply_soft_claim(&identity, &process, claim.owner, desired, None)
            }
            ResolvedCpuAllocation::InstanceBalancing {
                processors,
                generation,
            } => {
                let topology = topology.map_err(|error| {
                    CpuAllocationApplyError::Topology(ProcessControlError::Failed(error.to_owned()))
                })?;
                let entries = inventory
                    .entries(&mut self.platform)
                    .map_err(CpuAllocationApplyError::Discovery)?;
                let expectation = self.prepare_instance_balancing(
                    &identity, &process, topology, processors, generation, entries,
                )?;
                let desired = processors
                    .iter()
                    .map(|processor| entries[processor].id())
                    .collect();
                self.apply_soft_claim(&identity, &process, claim.owner, desired, Some(expectation))
            }
            ResolvedCpuAllocation::HardAffinity(request) => self
                .apply_hard_claim(&identity, &process, claim.owner, request)
                .map_err(Into::into),
        }
    }

    fn apply_soft_claim(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
        owner: ControlOwner,
        mut desired: Vec<u32>,
        balancing: Option<instance_balancing::Expectation>,
    ) -> Result<CpuAllocationApplyOutcome, CpuAllocationApplyError> {
        normalize_cpu_set_ids(&mut desired);
        let released_affinity = self.release_affinity_for_switch(identity, process)?;
        if desired.is_empty() {
            if let Err(error) = self.release_cpu_sets_if_managed(identity, process) {
                return Err(self
                    .finish_switch_failure(
                        error,
                        released_affinity.map(SwitchCompensation::Affinity),
                        identity,
                        process,
                    )
                    .into());
            }
            return Ok(CpuAllocationApplyOutcome::NoUsableTarget);
        }

        if let Some(expectation) = &balancing {
            let current = match self.platform.query_affinity(process) {
                Ok((current, _)) => current,
                Err(error) => return Err(error.into()),
            };
            if current != expectation.affinity {
                self.suppress_instance_balancing(identity);
                return Err(CpuAllocationApplyError::Eligibility(
                    InstanceBalancingEligibilityError::ExternalChange,
                ));
            }
        }
        match self.apply_cpu_sets_value(identity, process, owner, desired, balancing.as_ref()) {
            Ok(outcome) => Ok(outcome),
            Err(failure) => {
                if let Some(eligibility) = failure.eligibility {
                    return Err(CpuAllocationApplyError::Eligibility(eligibility));
                }
                if balancing.is_some() {
                    return Err(failure.error.into());
                }
                let compensation = (!failure.uncertain)
                    .then_some(released_affinity)
                    .flatten()
                    .map(SwitchCompensation::Affinity);
                Err(self
                    .finish_switch_failure(failure.error, compensation, identity, process)
                    .into())
            }
        }
    }

    fn apply_hard_claim(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
        owner: ControlOwner,
        request: HardAffinityRequest,
    ) -> Result<CpuAllocationApplyOutcome, ProcessControlError> {
        self.platform.validate_group_zero_affinity(process)?;
        let (current, system) = self.platform.query_affinity(process)?;
        let desired =
            match request {
                HardAffinityRequest::Exact(mask) => target_affinity_mask(mask, system),
                HardAffinityRequest::PreserveExact(mask) => self
                    .guarded_affinity_baseline(identity, process, (current, system), true)?
                    .and_then(|_| target_affinity_mask(mask, system)),
                HardAffinityRequest::Reduce {
                    full_mask,
                    processors_to_remove,
                    preserve_custom_affinity,
                } => {
                    if target_affinity_mask(full_mask, system) == Some(system) {
                        self.guarded_affinity_baseline(
                            identity,
                            process,
                            (current, system),
                            preserve_custom_affinity,
                        )?
                        .and_then(|baseline| {
                            reduced_affinity_mask(identity, baseline, processors_to_remove)
                        })
                    } else {
                        None
                    }
                }
                HardAffinityRequest::Limit(mask) => {
                    if self.managed_affinity.get(identity).is_some_and(|managed| {
                        managed.owner == owner && managed.expected != current
                    }) {
                        self.release_affinity_if_managed(identity, process)?;
                        return Ok(CpuAllocationApplyOutcome::NoUsableTarget);
                    }
                    let mut cpu_sets = self.platform.query_cpu_sets(process)?;
                    normalize_cpu_set_ids(&mut cpu_sets);
                    let baseline_sets = self
                        .managed_cpu_sets
                        .get(identity)
                        .filter(|managed| managed.expected == cpu_sets)
                        .map_or(cpu_sets.as_slice(), |managed| managed.baseline.as_slice());
                    if !baseline_sets.is_empty() {
                        return Ok(CpuAllocationApplyOutcome::NoUsableTarget);
                    }
                    let Some(desired) = target_affinity_mask(mask, system)
                        .map(|selection| selection & current)
                        .filter(|desired| *desired != 0)
                    else {
                        return Ok(CpuAllocationApplyOutcome::NoUsableTarget);
                    };
                    if desired == current
                        && !self
                            .managed_affinity
                            .get(identity)
                            .is_some_and(|managed| managed.owner == owner)
                    {
                        return Ok(CpuAllocationApplyOutcome::NoUsableTarget);
                    }
                    Some(desired)
                }
            };
        let released_cpu_sets = self.release_cpu_sets_for_switch(identity, process)?;
        let Some(desired) = desired else {
            if let Err(error) = self.release_affinity_if_managed(identity, process) {
                return Err(self.finish_switch_failure(
                    error,
                    released_cpu_sets.map(SwitchCompensation::CpuSets),
                    identity,
                    process,
                ));
            }
            return Ok(CpuAllocationApplyOutcome::NoUsableTarget);
        };

        match self.apply_affinity_value(identity, process, owner, desired, Some((current, system)))
        {
            Ok(outcome) => Ok(outcome),
            Err(failure) => {
                let compensation = (!failure.uncertain)
                    .then_some(released_cpu_sets)
                    .flatten()
                    .map(SwitchCompensation::CpuSets);
                Err(self.finish_switch_failure(failure.error, compensation, identity, process))
            }
        }
    }

    fn guarded_affinity_baseline(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
        (current, system): (usize, usize),
        preserve_custom_affinity: bool,
    ) -> Result<Option<usize>, ProcessControlError> {
        // A verified assignment made by Winderust is not a custom Windows baseline.
        let affinity = match self.managed_affinity.get(identity) {
            Some(managed) if managed.confirmed && managed.expected == current => managed.baseline,
            Some(_) => return Ok(None),
            None => current,
        };
        if affinity == 0
            || affinity & system != affinity
            || (preserve_custom_affinity && affinity != system)
        {
            return Ok(None);
        }

        let mut current_sets = self.platform.query_cpu_sets(process)?;
        normalize_cpu_set_ids(&mut current_sets);
        let baseline_sets = match self.managed_cpu_sets.get(identity) {
            Some(managed) if managed.confirmed && managed.expected == current_sets => {
                &managed.baseline
            }
            Some(_) => return Ok(None),
            None => &current_sets,
        };
        Ok(baseline_sets.is_empty().then_some(affinity))
    }

    fn finish_switch_failure(
        &mut self,
        primary: ProcessControlError,
        compensation: Option<SwitchCompensation>,
        identity: &ProcessIdentity,
        process: &P::Process,
    ) -> ProcessControlError {
        let Some(compensation) = compensation else {
            return primary;
        };
        let result = match compensation {
            SwitchCompensation::Affinity(previous) => {
                self.reapply_affinity(identity, process, previous)
            }
            SwitchCompensation::CpuSets(previous) => {
                self.reapply_cpu_sets(identity, process, previous)
            }
        };
        match result {
            Ok(()) => primary,
            Err(error) => ProcessControlError::Failed(format!(
                "{primary} Previous CPU allocation state could not be restored: {error}."
            )),
        }
    }

    fn apply_affinity_value(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
        owner: ControlOwner,
        desired: usize,
        observed: Option<(usize, usize)>,
    ) -> Result<CpuAllocationApplyOutcome, PropertyApplyFailure> {
        let mut managed = self.managed_affinity.remove(identity);
        let current = match observed {
            Some((current, _)) => current,
            None => match self.platform.query_affinity(process) {
                Ok((current, _)) => current,
                Err(error) => {
                    if let Some(managed) = managed {
                        self.managed_affinity.insert(identity.clone(), managed);
                    }
                    return Err(PropertyApplyFailure::certain(error));
                }
            },
        };
        if managed
            .as_ref()
            .is_some_and(|managed| managed.expected != current)
        {
            if let Err(error) = self.platform.relinquish_affinity(identity) {
                if let Some(managed) = managed {
                    self.managed_affinity.insert(identity.clone(), managed);
                }
                return Err(PropertyApplyFailure::certain(error));
            }
            managed = None;
        }
        let baseline = managed.as_ref().map_or(current, |managed| managed.baseline);
        if current == desired {
            if let Some(mut managed) = managed {
                managed.owner = owner;
                managed.expected = current;
                self.managed_affinity.insert(identity.clone(), managed);
            }
            return Ok(CpuAllocationApplyOutcome::Unchanged);
        }

        let sequence = self.next_sequence();
        match apply_affinity_transition(
            &mut self.platform,
            process,
            current,
            desired,
            CommitFailureBehavior::KeepManagedTarget,
        ) {
            Ok(()) => {
                self.managed_affinity.insert(
                    identity.clone(),
                    ManagedValue {
                        baseline,
                        expected: desired,
                        owner,
                        apply_sequence: sequence,
                        confirmed: managed.as_ref().is_none_or(|managed| managed.confirmed),
                    },
                );
                Ok(CpuAllocationApplyOutcome::Applied)
            }
            Err(mut failure) => {
                if failure.relinquish_recovery {
                    if let Err(error) = self.platform.relinquish_affinity(identity) {
                        failure.error = ProcessControlError::Failed(format!(
                            "{} Recovery journal relinquish failed: {error}.",
                            failure.error
                        ));
                        failure.uncertain = true;
                    }
                }
                if failure.uncertain {
                    self.managed_affinity.insert(
                        identity.clone(),
                        ManagedValue {
                            baseline,
                            expected: desired,
                            owner,
                            apply_sequence: sequence,
                            confirmed: false,
                        },
                    );
                } else if let Some(managed) = managed {
                    self.managed_affinity.insert(identity.clone(), managed);
                }
                Err(PropertyApplyFailure {
                    error: failure.error,
                    uncertain: failure.uncertain,
                    eligibility: None,
                })
            }
        }
    }

    fn apply_cpu_sets_value(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
        owner: ControlOwner,
        mut desired: Vec<u32>,
        balancing: Option<&instance_balancing::Expectation>,
    ) -> Result<CpuAllocationApplyOutcome, PropertyApplyFailure> {
        normalize_cpu_set_ids(&mut desired);
        let mut managed = self.managed_cpu_sets.remove(identity);
        let current = match self.platform.query_cpu_sets(process) {
            Ok(mut current) => {
                normalize_cpu_set_ids(&mut current);
                current
            }
            Err(error) => {
                if let Some(managed) = managed {
                    self.managed_cpu_sets.insert(identity.clone(), managed);
                }
                return Err(PropertyApplyFailure::certain(error));
            }
        };
        if balancing.is_some_and(|expected| expected.cpu_sets != current) {
            if let Some(managed) = managed {
                self.managed_cpu_sets.insert(identity.clone(), managed);
            }
            self.suppress_instance_balancing(identity);
            return Err(PropertyApplyFailure::ineligible(
                InstanceBalancingEligibilityError::ExternalChange,
            ));
        }
        if managed
            .as_ref()
            .is_some_and(|managed| managed.expected != current)
        {
            if let Err(error) = self.platform.relinquish_cpu_sets(identity) {
                if let Some(managed) = managed {
                    self.managed_cpu_sets.insert(identity.clone(), managed);
                }
                return Err(PropertyApplyFailure::certain(error));
            }
            managed = None;
        }
        let baseline = managed
            .as_ref()
            .map_or_else(|| current.clone(), |managed| managed.baseline.clone());
        if current == desired {
            if let Some(mut managed) = managed {
                managed.owner = owner;
                managed.expected = current;
                self.managed_cpu_sets.insert(identity.clone(), managed);
            }
            return Ok(CpuAllocationApplyOutcome::Unchanged);
        }

        let sequence = self.next_sequence();
        match apply_cpu_sets_transition(
            &mut self.platform,
            process,
            &current,
            &desired,
            CommitFailureBehavior::KeepManagedTarget,
        ) {
            Ok(()) => {
                self.managed_cpu_sets.insert(
                    identity.clone(),
                    ManagedValue {
                        baseline,
                        expected: desired,
                        owner,
                        apply_sequence: sequence,
                        confirmed: managed.as_ref().is_none_or(|managed| managed.confirmed),
                    },
                );
                Ok(CpuAllocationApplyOutcome::Applied)
            }
            Err(mut failure) => {
                if failure.relinquish_recovery {
                    if let Err(error) = self.platform.relinquish_cpu_sets(identity) {
                        failure.error = ProcessControlError::Failed(format!(
                            "{} Recovery journal relinquish failed: {error}.",
                            failure.error
                        ));
                        failure.uncertain = true;
                    }
                }
                if failure.uncertain {
                    self.managed_cpu_sets.insert(
                        identity.clone(),
                        ManagedValue {
                            baseline,
                            expected: desired,
                            owner,
                            apply_sequence: sequence,
                            confirmed: false,
                        },
                    );
                } else if let Some(managed) = managed {
                    self.managed_cpu_sets.insert(identity.clone(), managed);
                }
                Err(PropertyApplyFailure {
                    error: failure.error,
                    uncertain: failure.uncertain,
                    eligibility: None,
                })
            }
        }
    }

    fn release_affinity_for_switch(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
    ) -> Result<Option<ManagedValue<usize>>, ProcessControlError> {
        let Some(previous) = self.managed_affinity.get(identity).cloned() else {
            return Ok(None);
        };
        let restored = self.release_affinity_with_process(identity, process)?;
        self.managed_affinity.remove(identity);
        Ok(restored.then_some(previous))
    }

    fn release_cpu_sets_for_switch(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
    ) -> Result<Option<ManagedValue<Vec<u32>>>, ProcessControlError> {
        let Some(previous) = self.managed_cpu_sets.get(identity).cloned() else {
            return Ok(None);
        };
        let restored = self.release_cpu_sets_with_process(identity, process)?;
        self.managed_cpu_sets.remove(identity);
        Ok(restored.then_some(previous))
    }

    fn release_affinity_if_managed(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
    ) -> Result<(), ProcessControlError> {
        if self.managed_affinity.contains_key(identity) {
            self.release_affinity_with_process(identity, process)?;
            self.managed_affinity.remove(identity);
        }
        Ok(())
    }

    fn release_cpu_sets_if_managed(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
    ) -> Result<(), ProcessControlError> {
        if self.managed_cpu_sets.contains_key(identity) {
            self.release_cpu_sets_with_process(identity, process)?;
            self.managed_cpu_sets.remove(identity);
        }
        Ok(())
    }

    fn reapply_affinity(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
        mut previous: ManagedValue<usize>,
    ) -> Result<(), ProcessControlError> {
        let confirmed = previous.confirmed;
        previous.confirmed = false;
        let current = self.platform.query_affinity(process)?.0;
        if current != previous.expected {
            if let Err(mut failure) = apply_affinity_transition(
                &mut self.platform,
                process,
                current,
                previous.expected,
                CommitFailureBehavior::KeepManagedTarget,
            ) {
                if failure.relinquish_recovery {
                    if let Err(error) = self.platform.relinquish_affinity(identity) {
                        previous.confirmed = false;
                        failure.error = ProcessControlError::Failed(format!(
                            "{} Recovery journal relinquish failed: {error}.",
                            failure.error
                        ));
                    }
                }
                self.managed_affinity.insert(identity.clone(), previous);
                return Err(failure.error);
            }
            previous.confirmed = confirmed && current == previous.baseline;
        }
        self.managed_affinity.insert(identity.clone(), previous);
        Ok(())
    }

    fn reapply_cpu_sets(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
        mut previous: ManagedValue<Vec<u32>>,
    ) -> Result<(), ProcessControlError> {
        let confirmed = previous.confirmed;
        previous.confirmed = false;
        let mut current = self.platform.query_cpu_sets(process)?;
        normalize_cpu_set_ids(&mut current);
        if current != previous.expected {
            if let Err(mut failure) = apply_cpu_sets_transition(
                &mut self.platform,
                process,
                &current,
                &previous.expected,
                CommitFailureBehavior::KeepManagedTarget,
            ) {
                if failure.relinquish_recovery {
                    if let Err(error) = self.platform.relinquish_cpu_sets(identity) {
                        previous.confirmed = false;
                        failure.error = ProcessControlError::Failed(format!(
                            "{} Recovery journal relinquish failed: {error}.",
                            failure.error
                        ));
                    }
                }
                self.managed_cpu_sets.insert(identity.clone(), previous);
                return Err(failure.error);
            }
            previous.confirmed = confirmed && current == previous.baseline;
        }
        self.managed_cpu_sets.insert(identity.clone(), previous);
        Ok(())
    }

    fn relinquish_stale_identities(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<(), ProcessControlError> {
        let stale_affinity = self
            .managed_affinity
            .keys()
            .filter(|managed| managed.id == identity.id && **managed != *identity)
            .cloned()
            .collect::<Vec<_>>();
        for stale in stale_affinity {
            self.platform.relinquish_affinity(&stale)?;
            self.managed_affinity.remove(&stale);
        }

        let stale_cpu_sets = self
            .managed_cpu_sets
            .keys()
            .filter(|managed| managed.id == identity.id && **managed != *identity)
            .cloned()
            .collect::<Vec<_>>();
        for stale in stale_cpu_sets {
            self.platform.relinquish_cpu_sets(&stale)?;
            self.managed_cpu_sets.remove(&stale);
        }
        self.instance_balancing
            .retain(|managed, _| managed.id != identity.id || *managed == *identity);
        Ok(())
    }

    pub(crate) fn release_policy_except(
        &mut self,
        owner: ControlOwner,
        active_targets: &BTreeSet<ProcessTargetKey>,
    ) -> CpuAllocationReleaseSummary {
        if let Err(error) = validate_owner(owner) {
            return CpuAllocationReleaseSummary {
                failures: vec![CpuAllocationReleaseFailure {
                    owner,
                    process_id: 0,
                    process_name: "CPU allocation".to_owned(),
                    property: "CPU allocation",
                    error,
                }],
                ..Default::default()
            };
        }

        let stale_keys = self
            .claims
            .get(&owner)
            .into_iter()
            .flat_map(|claims| claims.keys())
            .filter(|key| !active_targets.contains(*key))
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut reconcile_keys = stale_keys
            .iter()
            .filter(|key| {
                self.effective_claim(key)
                    .is_some_and(|claim| claim.owner == owner)
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        if let Some(claims) = self.claims.get_mut(&owner) {
            claims.retain(|key, _| !stale_keys.contains(key));
        }

        reconcile_keys.extend(
            self.managed_affinity
                .iter()
                .filter_map(|(identity, managed)| {
                    let effective_owner = self
                        .effective_claim(&identity.key())
                        .map(|claim| claim.owner);
                    (managed.owner == owner && effective_owner != Some(owner))
                        .then(|| identity.key())
                }),
        );
        reconcile_keys.extend(
            self.managed_cpu_sets
                .iter()
                .filter_map(|(identity, managed)| {
                    let effective_owner = self
                        .effective_claim(&identity.key())
                        .map(|claim| claim.owner);
                    (managed.owner == owner && effective_owner != Some(owner))
                        .then(|| identity.key())
                }),
        );

        let mut summary = CpuAllocationReleaseSummary::default();
        for key in reconcile_keys {
            self.reconcile_policy_key(&key, &mut summary);
        }
        self.prune_adaptive_episode_keys();
        summary
    }

    pub(crate) fn release_adaptive_episode_placements_except(
        &mut self,
        active_targets: &BTreeSet<ProcessTargetKey>,
    ) -> (CpuAllocationReleaseSummary, bool) {
        let stale = self
            .adaptive_episode_keys
            .difference(active_targets)
            .cloned()
            .collect::<Vec<_>>();
        let mut summary = CpuAllocationReleaseSummary::default();
        for key in stale {
            self.withdraw_adaptive_episode_claim(&key);
            self.reconcile_policy_key(&key, &mut summary);
        }
        self.prune_adaptive_episode_keys();
        (
            summary,
            !self.adaptive_episode_keys.is_subset(active_targets),
        )
    }

    pub(crate) fn release_adaptive_episode_placement(
        &mut self,
        key: &ProcessTargetKey,
    ) -> (CpuAllocationReleaseSummary, bool) {
        let mut summary = CpuAllocationReleaseSummary::default();
        if self.adaptive_episode_keys.contains(key) {
            self.withdraw_adaptive_episode_claim(key);
            self.reconcile_policy_key(key, &mut summary);
            self.prune_adaptive_episode_keys();
        }
        (summary, self.adaptive_episode_keys.contains(key))
    }

    fn withdraw_adaptive_episode_claim(&mut self, key: &ProcessTargetKey) {
        if self.adaptive_episode_keys.contains(key) {
            if let Some(claims) = self.claims.get_mut(&ControlOwner::AdaptiveEngine) {
                claims.remove(key);
            }
        }
    }

    fn prune_adaptive_episode_keys(&mut self) {
        if self.adaptive_episode_keys.is_empty() {
            return;
        }
        let owner = ControlOwner::AdaptiveEngine;
        let claims = self.claims.get(&owner);
        let affinity = &self.managed_affinity;
        let cpu_sets = &self.managed_cpu_sets;
        self.adaptive_episode_keys.retain(|key| {
            claims.is_some_and(|claims| claims.contains_key(key))
                || affinity.iter().any(|(identity, managed)| {
                    managed.owner == owner && identity.matches_target_key(key)
                })
                || cpu_sets.iter().any(|(identity, managed)| {
                    managed.owner == owner && identity.matches_target_key(key)
                })
        });
    }

    pub(crate) fn release_all_policy(
        &mut self,
        owner: ControlOwner,
    ) -> CpuAllocationReleaseSummary {
        self.release_policy_except(owner, &BTreeSet::new())
    }

    pub(crate) fn reconcile_pending(
        &mut self,
        topology: Result<&CpuTopology, &str>,
        allow_cross_session_process_control: bool,
        include_release_retries: bool,
    ) -> CpuAllocationReconciliationSummary {
        let mut inventory = CpuSetInventory::default();
        let mut summary = self.reconcile_topology(topology);
        let pending = take(&mut self.pending_reconciliations);
        for (key, reconciliation) in pending {
            if matches!(
                reconciliation,
                PendingReconciliation::ReleaseOnlyRetry { .. }
                    | PendingReconciliation::HandoffRetry
            ) && !include_release_retries
            {
                self.pending_reconciliations.insert(key, reconciliation);
                continue;
            }
            let first_release_attempt = matches!(
                &reconciliation,
                PendingReconciliation::ReleaseOnlyFirstAttempt { .. }
            );
            if let PendingReconciliation::ReleaseOnlyFirstAttempt { failed_claim }
            | PendingReconciliation::ReleaseOnlyRetry { failed_claim } = reconciliation
            {
                let mut release = CpuAllocationReleaseSummary::default();
                let completed = self.release_managed_key(&key, &mut release);
                summary
                    .releases
                    .restored_owners
                    .append(&mut release.restored_owners);
                if first_release_attempt {
                    summary.releases.failures.append(&mut release.failures);
                }
                if !completed {
                    self.pending_reconciliations.insert(
                        key,
                        PendingReconciliation::ReleaseOnlyRetry { failed_claim },
                    );
                } else {
                    self.queue_changed_effective_claim(&key, failed_claim);
                }
                continue;
            }

            let invalid = self.remove_unresolved_claims(&key, topology);
            let invalid_owners = invalid.iter().map(|(claim, _)| claim.owner).collect();
            for (claim, error) in invalid {
                summary
                    .failures
                    .push(reconciliation_failure_for_claim(&claim, error));
            }
            if !self.release_unresolved_assignment(&key, &invalid_owners, &mut summary.releases) {
                continue;
            }
            self.discard_replayed_episode_claim(&key);
            let Some(claim) = self.effective_claim(&key).cloned() else {
                if !self.release_managed_key(&key, &mut summary.releases) {
                    self.pending_reconciliations.insert(
                        key,
                        PendingReconciliation::ReleaseOnlyRetry { failed_claim: None },
                    );
                }
                continue;
            };
            let failed_claim = claim_fingerprint(&claim);
            match self.apply_effective_claim(
                &mut inventory,
                topology,
                claim.clone(),
                allow_cross_session_process_control,
            ) {
                Ok(CpuAllocationApplyOutcome::Applied) => {
                    summary
                        .applications
                        .push(reconciled_application_for_claim(&claim));
                }
                Ok(
                    CpuAllocationApplyOutcome::Unchanged
                    | CpuAllocationApplyOutcome::NoUsableTarget,
                ) => {}
                Ok(CpuAllocationApplyOutcome::Shadowed) => {
                    summary.failures.push(reconciliation_failure_for_claim(
                        &claim,
                        ProcessControlError::Failed(
                            "CPU allocation handoff unexpectedly became shadowed.".to_owned(),
                        ),
                    ));
                    if !self.release_managed_key(&key, &mut summary.releases) {
                        self.pending_reconciliations.insert(
                            key,
                            PendingReconciliation::ReleaseOnlyRetry {
                                failed_claim: Some(failed_claim),
                            },
                        );
                    }
                }
                Err(CpuAllocationApplyError::Discovery(error)) => {
                    summary
                        .failures
                        .push(reconciliation_failure_for_claim(&claim, error));
                    if claim.owner == ControlOwner::InstanceBalancing {
                        if let Some(claims) = self.claims.get_mut(&claim.owner) {
                            claims.remove(&key);
                        }
                        self.reconcile_policy_key(&key, &mut summary.releases);
                    } else {
                        self.pending_reconciliations
                            .insert(key, PendingReconciliation::HandoffRetry);
                    }
                }
                Err(CpuAllocationApplyError::Topology(error)) => {
                    if let Some(claims) = self.claims.get_mut(&claim.owner) {
                        claims.remove(&key);
                    }
                    summary
                        .failures
                        .push(reconciliation_failure_for_claim(&claim, error));
                    self.reconcile_policy_key(&key, &mut summary.releases);
                }
                Err(CpuAllocationApplyError::Target(error)) => {
                    summary
                        .failures
                        .push(reconciliation_failure_for_claim(&claim, error));
                    if claim.owner == ControlOwner::InstanceBalancing {
                        if let Some(claims) = self.claims.get_mut(&claim.owner) {
                            claims.remove(&key);
                        }
                        self.reconcile_policy_key(&key, &mut summary.releases);
                    } else if !self.release_managed_key(&key, &mut summary.releases) {
                        self.pending_reconciliations.insert(
                            key,
                            PendingReconciliation::ReleaseOnlyRetry {
                                failed_claim: Some(failed_claim),
                            },
                        );
                    }
                }
                Err(CpuAllocationApplyError::Eligibility(error)) => {
                    if let Some(claims) = self.claims.get_mut(&claim.owner) {
                        claims.remove(&key);
                    }
                    summary.failures.push(reconciliation_failure_for_claim(
                        &claim,
                        ProcessControlError::Failed(error.to_string()),
                    ));
                    self.reconcile_policy_key(&key, &mut summary.releases);
                }
            }
        }
        self.prune_adaptive_episode_keys();
        summary
    }

    pub(crate) fn reconcile_topology(
        &mut self,
        topology: Result<&CpuTopology, &str>,
    ) -> CpuAllocationReconciliationSummary {
        let keys = self
            .claims
            .values()
            .flat_map(|claims| claims.keys().cloned())
            .collect::<BTreeSet<_>>();
        let mut summary = CpuAllocationReconciliationSummary::default();
        for key in keys {
            let invalid = self.remove_unresolved_claims(&key, topology);
            if invalid.is_empty() {
                continue;
            }
            let invalid_owners = invalid.iter().map(|(claim, _)| claim.owner).collect();
            for (claim, error) in invalid {
                summary
                    .failures
                    .push(reconciliation_failure_for_claim(&claim, error));
            }
            if self.release_unresolved_assignment(&key, &invalid_owners, &mut summary.releases) {
                self.reconcile_policy_key(&key, &mut summary.releases);
            }
        }
        summary
    }

    pub(crate) fn has_pending_reconciliation(&self) -> bool {
        !self.pending_reconciliations.is_empty()
    }

    pub(crate) fn has_pending_immediate_reconciliation(&self) -> bool {
        self.pending_reconciliations.values().any(|pending| {
            matches!(
                pending,
                PendingReconciliation::Handoff
                    | PendingReconciliation::ReleaseOnlyFirstAttempt { .. }
            )
        })
    }

    pub(crate) fn has_pending_release_retry(&self) -> bool {
        self.pending_reconciliations.values().any(|pending| {
            matches!(
                pending,
                PendingReconciliation::ReleaseOnlyRetry { .. }
                    | PendingReconciliation::HandoffRetry
            )
        })
    }

    fn remove_unresolved_claims(
        &mut self,
        key: &ProcessTargetKey,
        topology: Result<&CpuTopology, &str>,
    ) -> Vec<(CpuAllocationClaim, ProcessControlError)> {
        let mut invalid = Vec::new();
        for claims in self.claims.values_mut() {
            if let Some((claim, error)) = claims.get(key).and_then(|claim| {
                resolve_request(claim, topology)
                    .err()
                    .map(|error| (claim.clone(), error))
            }) {
                claims.remove(key);
                invalid.push((claim, error));
            }
        }
        invalid
    }

    fn release_unresolved_assignment(
        &mut self,
        key: &ProcessTargetKey,
        invalid_owners: &BTreeSet<ControlOwner>,
        summary: &mut CpuAllocationReleaseSummary,
    ) -> bool {
        let owned = self.managed_affinity.iter().any(|(identity, value)| {
            identity.key() == *key && invalid_owners.contains(&value.owner)
        }) || self.managed_cpu_sets.iter().any(|(identity, value)| {
            identity.key() == *key && invalid_owners.contains(&value.owner)
        });
        if !owned || self.release_managed_key(key, summary) {
            return true;
        }
        self.pending_reconciliations.insert(
            key.clone(),
            PendingReconciliation::ReleaseOnlyRetry { failed_claim: None },
        );
        false
    }

    fn reconcile_policy_key(
        &mut self,
        key: &ProcessTargetKey,
        summary: &mut CpuAllocationReleaseSummary,
    ) {
        self.discard_replayed_episode_claim(key);
        if let Some(effective) = self.effective_claim(key) {
            let effective = claim_fingerprint(effective);
            let should_queue_handoff = match self.pending_reconciliations.get(key) {
                Some(PendingReconciliation::Handoff | PendingReconciliation::HandoffRetry) => false,
                Some(
                    PendingReconciliation::ReleaseOnlyFirstAttempt {
                        failed_claim: Some(failed),
                    }
                    | PendingReconciliation::ReleaseOnlyRetry {
                        failed_claim: Some(failed),
                    },
                ) => failed != &effective,
                Some(
                    PendingReconciliation::ReleaseOnlyFirstAttempt { failed_claim: None }
                    | PendingReconciliation::ReleaseOnlyRetry { failed_claim: None },
                )
                | None => true,
            };
            if should_queue_handoff {
                self.pending_reconciliations
                    .insert(key.clone(), PendingReconciliation::Handoff);
            }
            return;
        }

        if self.release_managed_key(key, summary) {
            self.pending_reconciliations.remove(key);
        } else {
            self.pending_reconciliations.insert(
                key.clone(),
                PendingReconciliation::ReleaseOnlyRetry { failed_claim: None },
            );
        }
    }

    fn queue_changed_effective_claim(
        &mut self,
        key: &ProcessTargetKey,
        failed_claim: Option<CpuAllocationClaimFingerprint>,
    ) {
        self.discard_replayed_episode_claim(key);
        let effective = self.effective_claim(key).map(claim_fingerprint);
        if effective.is_some() && effective != failed_claim {
            self.pending_reconciliations
                .insert(key.clone(), PendingReconciliation::Handoff);
        }
    }

    fn discard_replayed_episode_claim(&mut self, key: &ProcessTargetKey) {
        if self.adaptive_episode_keys.contains(key)
            && self
                .effective_claim(key)
                .is_some_and(|claim| claim.owner == ControlOwner::AdaptiveEngine)
        {
            // A handoff cannot start a timed allocation without a fresh eligibility pass.
            self.withdraw_adaptive_episode_claim(key);
        }
    }

    fn release_managed_key(
        &mut self,
        key: &ProcessTargetKey,
        summary: &mut CpuAllocationReleaseSummary,
    ) -> bool {
        let affinity_identity = self
            .managed_affinity
            .iter()
            .find(|(identity, _)| identity.key() == *key)
            .map(|(identity, managed)| (identity.clone(), managed.owner));
        if let Some((identity, managed_owner)) = affinity_identity {
            match self.release_affinity_identity(&identity) {
                Ok(restored) => {
                    if restored {
                        summary.restored_owners.push(managed_owner);
                    }
                    self.managed_affinity.remove(&identity);
                }
                Err(ProcessControlError::ProcessExited) => {
                    self.drop_exited_affinity(&identity, managed_owner, summary)
                }
                Err(error) => summary.failures.push(release_failure_for_identity(
                    &identity,
                    managed_owner,
                    "Processor Affinity (Hard)",
                    error,
                )),
            }
        }

        let cpu_sets_identity = self
            .managed_cpu_sets
            .iter()
            .find(|(identity, _)| identity.key() == *key)
            .map(|(identity, managed)| (identity.clone(), managed.owner));
        if let Some((identity, managed_owner)) = cpu_sets_identity {
            match self.release_cpu_sets_identity(&identity) {
                Ok(restored) => {
                    if restored {
                        summary.restored_owners.push(managed_owner);
                    }
                    self.managed_cpu_sets.remove(&identity);
                }
                Err(ProcessControlError::ProcessExited) => {
                    self.drop_exited_cpu_sets(&identity, managed_owner, summary)
                }
                Err(error) => summary.failures.push(release_failure_for_identity(
                    &identity,
                    managed_owner,
                    "CPU Sets (Soft)",
                    error,
                )),
            }
        }

        !self
            .managed_affinity
            .keys()
            .chain(self.managed_cpu_sets.keys())
            .any(|identity| identity.key() == *key)
    }

    fn drop_exited_affinity(
        &mut self,
        identity: &ProcessIdentity,
        owner: ControlOwner,
        summary: &mut CpuAllocationReleaseSummary,
    ) {
        self.instance_balancing.remove(identity);
        match self.platform.relinquish_affinity(identity) {
            Ok(()) => {
                self.managed_affinity.remove(identity);
            }
            Err(error) => summary.failures.push(release_failure_for_identity(
                identity,
                owner,
                "Processor Affinity (Hard)",
                error,
            )),
        }
    }

    fn drop_exited_cpu_sets(
        &mut self,
        identity: &ProcessIdentity,
        owner: ControlOwner,
        summary: &mut CpuAllocationReleaseSummary,
    ) {
        self.instance_balancing.remove(identity);
        match self.platform.relinquish_cpu_sets(identity) {
            Ok(()) => {
                self.managed_cpu_sets.remove(identity);
            }
            Err(error) => summary.failures.push(release_failure_for_identity(
                identity,
                owner,
                "CPU Sets (Soft)",
                error,
            )),
        }
    }

    fn release_affinity_identity(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<bool, ProcessControlError> {
        let target = identity.target();
        let (_, process) = self.platform.open(&target, true)?;
        self.release_affinity_with_process(identity, &process)
    }

    fn release_cpu_sets_identity(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<bool, ProcessControlError> {
        let target = identity.target();
        let (_, process) = self.platform.open(&target, true)?;
        self.release_cpu_sets_with_process(identity, &process)
    }

    fn release_affinity_with_process(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
    ) -> Result<bool, ProcessControlError> {
        let Some(managed) = self.managed_affinity.get(identity).cloned() else {
            return Ok(false);
        };
        let current = self.platform.query_affinity(process)?.0;
        if current != managed.expected || current == managed.baseline {
            self.platform.relinquish_affinity(identity)?;
            return Ok(false);
        }
        match apply_affinity_transition(
            &mut self.platform,
            process,
            current,
            managed.baseline,
            CommitFailureBehavior::KeepRestoredBaseline,
        ) {
            Ok(()) => Ok(true),
            // Preserve a confirmed exit; otherwise recheck for exit during restoration.
            Err(failure)
                if failure.error == ProcessControlError::ProcessExited
                    || matches!(
                        self.platform.open(&identity.target(), true),
                        Err(ProcessControlError::ProcessExited)
                    ) =>
            {
                Err(ProcessControlError::ProcessExited)
            }
            Err(failure) if failure.expected_preserved => {
                match self.platform.relinquish_affinity(identity) {
                    Ok(()) => Ok(true),
                    Err(error) => {
                        if let Some(managed) = self.managed_affinity.get_mut(identity) {
                            managed.expected = managed.baseline;
                            managed.confirmed = false;
                        }
                        Err(ProcessControlError::Failed(format!(
                            "{} Recovery journal relinquish failed: {error}.",
                            failure.error
                        )))
                    }
                }
            }
            Err(failure) => {
                if failure.uncertain {
                    if let Some(managed) = self.managed_affinity.get_mut(identity) {
                        managed.confirmed = false;
                    }
                }
                Err(failure.error)
            }
        }
    }

    fn release_cpu_sets_with_process(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
    ) -> Result<bool, ProcessControlError> {
        let Some(managed) = self.managed_cpu_sets.get(identity).cloned() else {
            return Ok(false);
        };
        let mut current = self.platform.query_cpu_sets(process)?;
        normalize_cpu_set_ids(&mut current);
        if current != managed.expected || current == managed.baseline {
            if current != managed.expected && managed.owner == ControlOwner::InstanceBalancing {
                self.suppress_instance_balancing(identity);
            }
            self.platform.relinquish_cpu_sets(identity)?;
            return Ok(false);
        }
        match apply_cpu_sets_transition(
            &mut self.platform,
            process,
            &current,
            &managed.baseline,
            CommitFailureBehavior::KeepRestoredBaseline,
        ) {
            Ok(()) => Ok(true),
            // Preserve a confirmed exit; otherwise recheck for exit during restoration.
            Err(failure)
                if failure.error == ProcessControlError::ProcessExited
                    || matches!(
                        self.platform.open(&identity.target(), true),
                        Err(ProcessControlError::ProcessExited)
                    ) =>
            {
                Err(ProcessControlError::ProcessExited)
            }
            Err(failure) if failure.expected_preserved => {
                match self.platform.relinquish_cpu_sets(identity) {
                    Ok(()) => Ok(true),
                    Err(error) => {
                        if let Some(managed) = self.managed_cpu_sets.get_mut(identity) {
                            managed.expected = managed.baseline.clone();
                            managed.confirmed = false;
                        }
                        Err(ProcessControlError::Failed(format!(
                            "{} Recovery journal relinquish failed: {error}.",
                            failure.error
                        )))
                    }
                }
            }
            Err(failure) => {
                if failure.uncertain {
                    if let Some(managed) = self.managed_cpu_sets.get_mut(identity) {
                        managed.confirmed = false;
                    }
                }
                Err(failure.error)
            }
        }
    }

    pub(crate) fn policy_managed_process_count(&self, owner: ControlOwner) -> usize {
        self.policy_managed_processes(owner).count()
    }

    pub(crate) fn effective_affinity_limit_count(&self, key: &ProcessTargetKey) -> Option<usize> {
        self.managed_affinity
            .iter()
            .find_map(|(identity, managed)| {
                (managed.owner == ControlOwner::CpuLimiter
                    && managed.confirmed
                    && identity.key() == *key)
                    .then_some(managed.expected.count_ones() as usize)
            })
    }

    pub(crate) fn policy_managed_processes(
        &self,
        owner: ControlOwner,
    ) -> impl Iterator<Item = &ProcessIdentity> {
        let affinity = self
            .managed_affinity
            .iter()
            .filter(move |(_, managed)| managed.owner == owner)
            .map(|(identity, _)| identity);
        let cpu_sets_only = self
            .managed_cpu_sets
            .iter()
            .filter(move |(identity, managed)| {
                managed.owner == owner
                    && self
                        .managed_affinity
                        .get(*identity)
                        .is_none_or(|affinity| affinity.owner != owner)
            })
            .map(|(identity, _)| identity);
        affinity.chain(cpu_sets_only)
    }

    pub(crate) fn needs_topology_observation(&self) -> bool {
        self.claims.values().any(|claims| !claims.is_empty())
            || self.has_managed_state()
            || self.has_pending_reconciliation()
    }

    pub(crate) fn has_managed_state(&self) -> bool {
        !self.managed_affinity.is_empty() || !self.managed_cpu_sets.is_empty()
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        self.claims.clear();
        self.adaptive_episode_keys.clear();
        self.pending_reconciliations.clear();
        let mut releases = self
            .managed_affinity
            .iter()
            .map(|(identity, managed)| {
                (
                    managed.apply_sequence,
                    ManagedProperty::Affinity,
                    identity.clone(),
                    managed.owner,
                )
            })
            .chain(self.managed_cpu_sets.iter().map(|(identity, managed)| {
                (
                    managed.apply_sequence,
                    ManagedProperty::CpuSets,
                    identity.clone(),
                    managed.owner,
                )
            }))
            .collect::<Vec<_>>();
        releases.sort_by_key(|release| std::cmp::Reverse(release.0));

        let mut summary = CpuAllocationReleaseSummary::default();
        for (_, property, identity, owner) in releases {
            match property {
                ManagedProperty::Affinity => match self.release_affinity_identity(&identity) {
                    Ok(restored) => {
                        if restored {
                            summary.restored_owners.push(owner);
                        }
                        self.managed_affinity.remove(&identity);
                    }
                    Err(ProcessControlError::ProcessExited) => {
                        self.drop_exited_affinity(&identity, owner, &mut summary)
                    }
                    Err(error) => summary.failures.push(release_failure_for_identity(
                        &identity,
                        owner,
                        "Processor Affinity (Hard)",
                        error,
                    )),
                },
                ManagedProperty::CpuSets => match self.release_cpu_sets_identity(&identity) {
                    Ok(restored) => {
                        if restored {
                            summary.restored_owners.push(owner);
                        }
                        self.managed_cpu_sets.remove(&identity);
                    }
                    Err(ProcessControlError::ProcessExited) => {
                        self.drop_exited_cpu_sets(&identity, owner, &mut summary)
                    }
                    Err(error) => summary.failures.push(release_failure_for_identity(
                        &identity,
                        owner,
                        "CPU Sets (Soft)",
                        error,
                    )),
                },
            }
        }

        if summary.failures.is_empty() {
            Ok(())
        } else {
            Err(summary
                .failures
                .into_iter()
                .map(|failure| {
                    format!(
                        "{} {} ({}): {}",
                        failure.property, failure.process_name, failure.process_id, failure.error
                    )
                })
                .collect::<Vec<_>>()
                .join("; "))
        }
    }

    fn effective_claim(&self, key: &ProcessTargetKey) -> Option<&CpuAllocationClaim> {
        cpu_allocation_owner_precedence()
            .iter()
            .find_map(|owner| self.claims.get(owner).and_then(|claims| claims.get(key)))
    }
}

impl<P: CpuAllocationPlatform> Drop for CpuAllocationCoordinator<P> {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[derive(Clone, Copy)]
enum ManagedProperty {
    Affinity,
    CpuSets,
}

#[derive(Clone, Copy)]
enum HardAffinityRequest {
    Exact(u64),
    Limit(u64),
    PreserveExact(u64),
    Reduce {
        full_mask: u64,
        processors_to_remove: u8,
        preserve_custom_affinity: bool,
    },
}

enum SwitchCompensation {
    Affinity(ManagedValue<usize>),
    CpuSets(ManagedValue<Vec<u32>>),
}

struct PropertyApplyFailure {
    error: ProcessControlError,
    uncertain: bool,
    eligibility: Option<InstanceBalancingEligibilityError>,
}

impl PropertyApplyFailure {
    fn certain(error: ProcessControlError) -> Self {
        Self {
            error,
            uncertain: false,
            eligibility: None,
        }
    }

    fn ineligible(error: InstanceBalancingEligibilityError) -> Self {
        Self {
            error: ProcessControlError::Failed(error.to_string()),
            uncertain: false,
            eligibility: Some(error),
        }
    }
}

struct TransitionFailure {
    error: ProcessControlError,
    uncertain: bool,
    relinquish_recovery: bool,
    expected_preserved: bool,
}

#[derive(Clone, Copy)]
enum CommitFailureBehavior {
    KeepManagedTarget,
    KeepRestoredBaseline,
}

fn apply_affinity_transition<P: CpuAllocationPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: usize,
    expected: usize,
    commit_failure_behavior: CommitFailureBehavior,
) -> Result<(), TransitionFailure> {
    let intent = platform
        .begin_affinity_change(process, original, expected)
        .map_err(transition_begin_failure)?;
    if let Err(error) = platform.apply_affinity(process, expected) {
        return Err(compensate_affinity_with_intent(
            platform, process, original, intent, error,
        ));
    }
    match platform.query_affinity(process) {
        Ok((actual, _)) if actual == expected => {}
        Ok((actual, _)) => {
            return Err(compensate_affinity_with_intent(
                platform,
                process,
                original,
                intent,
                ProcessControlError::Failed(format!(
                    "Processor Affinity (Hard) verification returned {actual:#x}, expected {expected:#x}."
                )),
            ));
        }
        Err(error) => {
            return Err(compensate_affinity_with_intent(
                platform, process, original, intent, error,
            ));
        }
    }

    if let Err(error) = intent.commit() {
        let message = format!("Crash recovery commit failed: {error}");
        // Commit may already have compacted the earlier baseline. Keep the
        // verified target so either the pending or committed helper edge can unwind.
        return match commit_failure_behavior {
            CommitFailureBehavior::KeepManagedTarget => Err(TransitionFailure {
                error: ProcessControlError::Failed(message),
                uncertain: true,
                relinquish_recovery: false,
                expected_preserved: true,
            }),
            CommitFailureBehavior::KeepRestoredBaseline => Err(TransitionFailure {
                error: ProcessControlError::Failed(message),
                uncertain: false,
                relinquish_recovery: true,
                expected_preserved: true,
            }),
        };
    }
    Ok(())
}

fn apply_cpu_sets_transition<P: CpuAllocationPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: &[u32],
    expected: &[u32],
    commit_failure_behavior: CommitFailureBehavior,
) -> Result<(), TransitionFailure> {
    let intent = platform
        .begin_cpu_sets_change(process, original, expected)
        .map_err(transition_begin_failure)?;
    if let Err(error) = platform.apply_cpu_sets(process, expected) {
        return Err(compensate_cpu_sets_with_intent(
            platform, process, original, intent, error,
        ));
    }
    match platform.query_cpu_sets(process) {
        Ok(mut actual) => {
            normalize_cpu_set_ids(&mut actual);
            if actual != expected {
                return Err(compensate_cpu_sets_with_intent(
                    platform,
                    process,
                    original,
                    intent,
                    ProcessControlError::Failed(format!(
                        "CPU Sets (Soft) verification returned {actual:?}, expected {expected:?}."
                    )),
                ));
            }
        }
        Err(error) => {
            return Err(compensate_cpu_sets_with_intent(
                platform, process, original, intent, error,
            ));
        }
    }

    if let Err(error) = intent.commit() {
        let message = format!("Crash recovery commit failed: {error}");
        // A confirmed setter must not be rolled back after an unconfirmed Commit.
        return match commit_failure_behavior {
            CommitFailureBehavior::KeepManagedTarget => Err(TransitionFailure {
                error: ProcessControlError::Failed(message),
                uncertain: true,
                relinquish_recovery: false,
                expected_preserved: true,
            }),
            CommitFailureBehavior::KeepRestoredBaseline => Err(TransitionFailure {
                error: ProcessControlError::Failed(message),
                uncertain: false,
                relinquish_recovery: true,
                expected_preserved: true,
            }),
        };
    }
    Ok(())
}

fn compensate_affinity_with_intent<P: CpuAllocationPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: usize,
    intent: P::RecoveryIntent,
    primary_error: ProcessControlError,
) -> TransitionFailure {
    match restore_and_verify_affinity(platform, process, original) {
        Ok(()) => TransitionFailure {
            error: primary_error,
            uncertain: false,
            relinquish_recovery: false,
            expected_preserved: false,
        },
        Err(compensation_error) => {
            let recovery_error = intent.retain_pending().err();
            TransitionFailure {
                error: transition_failure_error(primary_error, compensation_error, recovery_error),
                uncertain: true,
                relinquish_recovery: false,
                expected_preserved: false,
            }
        }
    }
}

fn compensate_cpu_sets_with_intent<P: CpuAllocationPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: &[u32],
    intent: P::RecoveryIntent,
    primary_error: ProcessControlError,
) -> TransitionFailure {
    match restore_and_verify_cpu_sets(platform, process, original) {
        Ok(()) => TransitionFailure {
            error: primary_error,
            uncertain: false,
            relinquish_recovery: false,
            expected_preserved: false,
        },
        Err(compensation_error) => {
            let recovery_error = intent.retain_pending().err();
            TransitionFailure {
                error: transition_failure_error(primary_error, compensation_error, recovery_error),
                uncertain: true,
                relinquish_recovery: false,
                expected_preserved: false,
            }
        }
    }
}

fn restore_and_verify_affinity<P: CpuAllocationPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: usize,
) -> Result<(), ProcessControlError> {
    platform.apply_affinity(process, original)?;
    let actual = platform.query_affinity(process)?.0;
    if actual == original {
        Ok(())
    } else {
        Err(ProcessControlError::Failed(format!(
            "Processor Affinity (Hard) compensation returned {actual:#x}, expected {original:#x}."
        )))
    }
}

fn restore_and_verify_cpu_sets<P: CpuAllocationPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: &[u32],
) -> Result<(), ProcessControlError> {
    platform.apply_cpu_sets(process, original)?;
    let mut actual = platform.query_cpu_sets(process)?;
    normalize_cpu_set_ids(&mut actual);
    if actual == original {
        Ok(())
    } else {
        Err(ProcessControlError::Failed(format!(
            "CPU Sets (Soft) compensation returned {actual:?}, expected {original:?}."
        )))
    }
}

fn transition_begin_failure(error: ProcessControlError) -> TransitionFailure {
    TransitionFailure {
        error,
        uncertain: false,
        relinquish_recovery: false,
        expected_preserved: false,
    }
}

fn cpu_allocation_owner_precedence() -> &'static [ControlOwner] {
    &[
        ControlOwner::CpuSetsSoft,
        ControlOwner::ProcessorAffinityHard,
        ControlOwner::InstanceBalancing,
        ControlOwner::CpuLimiter,
        ControlOwner::AdaptiveEngine,
    ]
}

fn validate_owner(owner: ControlOwner) -> Result<(), ProcessControlError> {
    if cpu_allocation_owner_precedence().contains(&owner) {
        Ok(())
    } else {
        Err(ProcessControlError::Failed(
            "This owner cannot control CPU allocation policy.".to_owned(),
        ))
    }
}

fn target_affinity_mask(rule_mask: u64, system_affinity: usize) -> Option<usize> {
    let mask = usize::try_from(rule_mask).ok()?;
    (mask != 0 && mask & system_affinity == mask).then_some(mask)
}

fn reduced_affinity_mask(
    identity: &ProcessIdentity,
    baseline: usize,
    processors_to_remove: u8,
) -> Option<usize> {
    let count = u32::from(processors_to_remove).min(baseline.count_ones().saturating_sub(1));
    let mut choice = DefaultHasher::new();
    choice.write_u32(identity.id);
    choice.write_u64(identity.creation_time);
    let mut mask = baseline;
    for step in 0..count {
        choice.write_u32(step);
        let mut candidates = mask;
        for _ in 0..choice.finish() % u64::from(mask.count_ones()) {
            candidates &= candidates - 1;
        }
        mask &= !(1 << candidates.trailing_zeros());
    }
    (count > 0).then_some(mask)
}

enum ResolvedCpuAllocation<'a> {
    SoftCpuSets(&'a [ProcessorId]),
    HardAffinity(HardAffinityRequest),
    InstanceBalancing {
        processors: &'a [ProcessorId],
        generation: u64,
    },
}

fn resolve_request<'a>(
    claim: &'a CpuAllocationClaim,
    topology: Result<&CpuTopology, &str>,
) -> Result<ResolvedCpuAllocation<'a>, ProcessControlError> {
    instance_balancing::validate_claim(claim)
        .map_err(|error| ProcessControlError::Failed(error.to_string()))?;
    let owner = claim.owner;
    let topology = topology.map_err(|error| ProcessControlError::Failed(error.to_owned()))?;
    match &claim.request {
        CpuAllocationRequest::SoftCpuSets { selection } => selection
            .resolve_processors(topology)
            .and_then(|processors| {
                if owner == ControlOwner::AdaptiveEngine
                    && processors.iter().any(|processor| processor.group != 0)
                {
                    return Err(SelectionError::UnsupportedGroup.into());
                }
                Ok(match claim.balancing_generation {
                    Some(generation) => ResolvedCpuAllocation::InstanceBalancing {
                        processors,
                        generation,
                    },
                    None => ResolvedCpuAllocation::SoftCpuSets(processors),
                })
            }),
        CpuAllocationRequest::HardAffinity { selection } => selection
            .resolve_group_zero(topology)
            .map(|mask| ResolvedCpuAllocation::HardAffinity(HardAffinityRequest::Exact(mask))),
        CpuAllocationRequest::PreserveHardAffinity { selection } => {
            selection.resolve_group_zero(topology).map(|mask| {
                ResolvedCpuAllocation::HardAffinity(HardAffinityRequest::PreserveExact(mask))
            })
        }
        CpuAllocationRequest::ReduceAffinity {
            selection,
            processors_to_remove,
            preserve_custom_affinity,
        } => selection.resolve_group_zero(topology).map(|full_mask| {
            ResolvedCpuAllocation::HardAffinity(HardAffinityRequest::Reduce {
                full_mask,
                processors_to_remove: *processors_to_remove,
                preserve_custom_affinity: *preserve_custom_affinity,
            })
        }),
        CpuAllocationRequest::AffinityLimit { selection } => selection
            .resolve_group_zero(topology)
            .map(|mask| ResolvedCpuAllocation::HardAffinity(HardAffinityRequest::Limit(mask))),
    }
    .map_err(|error| ProcessControlError::Failed(error.to_string()))
}

fn normalize_cpu_set_ids(ids: &mut Vec<u32>) {
    ids.sort_unstable();
    ids.dedup();
}

fn release_failure_for_identity(
    identity: &ProcessIdentity,
    owner: ControlOwner,
    property: &'static str,
    error: ProcessControlError,
) -> CpuAllocationReleaseFailure {
    CpuAllocationReleaseFailure {
        owner,
        process_id: identity.id,
        process_name: identity.name.clone(),
        property,
        error,
    }
}

fn reconciled_application_for_claim(
    claim: &CpuAllocationClaim,
) -> CpuAllocationReconciledApplication {
    CpuAllocationReconciledApplication {
        owner: claim.owner,
        process_id: claim.target.id,
        process_name: claim.target.name.clone(),
        request: claim.request.clone(),
    }
}

fn claim_fingerprint(claim: &CpuAllocationClaim) -> CpuAllocationClaimFingerprint {
    CpuAllocationClaimFingerprint {
        owner: claim.owner,
        request: claim.request.clone(),
        balancing_generation: claim.balancing_generation,
    }
}

fn reconciliation_failure_for_claim(
    claim: &CpuAllocationClaim,
    error: ProcessControlError,
) -> CpuAllocationReconciliationFailure {
    CpuAllocationReconciliationFailure {
        owner: claim.owner,
        process_id: claim.target.id,
        process_name: claim.target.name.clone(),
        error,
    }
}

fn map_cpu_allocation_error(error: ProcessOperationError) -> ProcessControlError {
    match error {
        ProcessOperationError::AccessDenied { operation } => ProcessControlError::AccessDenied(
            format!("Windows denied CPU allocation access during {operation}."),
        ),
        ProcessOperationError::Failed { operation, code } => {
            ProcessControlError::Failed(format!("{operation} failed with Windows error {code}."))
        }
    }
}

#[cfg(test)]
mod tests;
