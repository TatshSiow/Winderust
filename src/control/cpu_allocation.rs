use std::{
    collections::{BTreeMap, BTreeSet},
    mem::take,
};

use crate::{
    backend::crash_recovery::{
        forget_affinity_change, forget_cpu_sets_change, record_process_change, ProcessValue,
        RecoveryIntent,
    },
    platform::windows::{cpu_allocation as windows_cpu_allocation, ProcessOperationError},
    win_util::WinHandle,
};

use super::process::{
    open_process_for_set_information, ControlOwner, ProcessControlError, ProcessControlTarget,
    ProcessIdentity, ProcessTargetKey,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CpuAllocationRequest {
    SoftCpuSets { logical_processor_mask: u64 },
    HardAffinity { logical_processor_mask: u64 },
    LimitLogicalProcessors { maximum: u8 },
}

#[derive(Debug, Clone)]
pub(crate) struct CpuAllocationClaim {
    pub(crate) target: ProcessControlTarget,
    pub(crate) owner: ControlOwner,
    pub(crate) request: CpuAllocationRequest,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CpuAllocationClaimFingerprint {
    owner: ControlOwner,
    request: CpuAllocationRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingReconciliation {
    Handoff,
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
}

pub(crate) trait CpuAllocationRecoveryIntent {
    fn commit(self) -> Result<(), String>;
}

impl CpuAllocationRecoveryIntent for RecoveryIntent {
    fn commit(self) -> Result<(), String> {
        RecoveryIntent::commit(self)
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
    fn query_cpu_sets(&mut self, process: &Self::Process) -> Result<Vec<u32>, ProcessControlError>;
    fn cpu_set_ids_for_mask(&mut self, mask: u64) -> Result<Vec<u32>, ProcessControlError>;
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

    fn query_cpu_sets(&mut self, process: &Self::Process) -> Result<Vec<u32>, ProcessControlError> {
        windows_cpu_allocation::query_cpu_sets(process).map_err(map_cpu_allocation_error)
    }

    fn cpu_set_ids_for_mask(&mut self, mask: u64) -> Result<Vec<u32>, ProcessControlError> {
        windows_cpu_allocation::cpu_set_ids_for_mask(mask).map_err(map_cpu_allocation_error)
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
    managed_affinity: BTreeMap<ProcessIdentity, ManagedValue<usize>>,
    managed_cpu_sets: BTreeMap<ProcessIdentity, ManagedValue<Vec<u32>>>,
    pending_reconciliations: BTreeMap<ProcessTargetKey, PendingReconciliation>,
    next_apply_sequence: u64,
}

impl Default for CpuAllocationCoordinator {
    fn default() -> Self {
        Self::with_platform(WindowsCpuAllocationPlatform)
    }
}

impl<P: CpuAllocationPlatform> CpuAllocationCoordinator<P> {
    fn with_platform(platform: P) -> Self {
        Self {
            platform,
            claims: BTreeMap::new(),
            managed_affinity: BTreeMap::new(),
            managed_cpu_sets: BTreeMap::new(),
            pending_reconciliations: BTreeMap::new(),
            next_apply_sequence: 1,
        }
    }

    fn next_sequence(&mut self) -> u64 {
        let sequence = self.next_apply_sequence;
        self.next_apply_sequence = self.next_apply_sequence.wrapping_add(1).max(1);
        sequence
    }

    pub(crate) fn apply_policy_claim(
        &mut self,
        claim: CpuAllocationClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<CpuAllocationApplyOutcome, ProcessControlError> {
        validate_owner(claim.owner)?;
        let owner = claim.owner;
        let key = claim.target.key();
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

        let failed_claim = claim_fingerprint(&effective);
        let result = self.apply_effective_claim(effective, allow_cross_session_process_control);
        if result.is_err() {
            let owner_claims = self.claims.entry(owner).or_default();
            if let Some(previous) = previous {
                owner_claims.insert(key.clone(), previous);
            } else {
                owner_claims.remove(&key);
            }
            if self.pending_reconciliations.contains_key(&key) {
                self.pending_reconciliations.insert(
                    key,
                    PendingReconciliation::ReleaseOnlyFirstAttempt {
                        failed_claim: Some(failed_claim),
                    },
                );
            }
        } else {
            self.pending_reconciliations.remove(&key);
        }
        result
    }

    fn apply_effective_claim(
        &mut self,
        claim: CpuAllocationClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<CpuAllocationApplyOutcome, ProcessControlError> {
        let (identity, process) = self
            .platform
            .open(&claim.target, allow_cross_session_process_control)?;
        self.relinquish_stale_identities(&identity)?;

        match claim.request {
            CpuAllocationRequest::SoftCpuSets {
                logical_processor_mask,
            } => self.apply_soft_claim(&identity, &process, claim.owner, logical_processor_mask),
            CpuAllocationRequest::HardAffinity {
                logical_processor_mask,
            } => self.apply_hard_claim(
                &identity,
                &process,
                claim.owner,
                HardAffinityRequest::Exact(logical_processor_mask),
            ),
            CpuAllocationRequest::LimitLogicalProcessors { maximum } => self.apply_hard_claim(
                &identity,
                &process,
                claim.owner,
                HardAffinityRequest::Limit(maximum),
            ),
        }
    }

    fn apply_soft_claim(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
        owner: ControlOwner,
        logical_processor_mask: u64,
    ) -> Result<CpuAllocationApplyOutcome, ProcessControlError> {
        let mut desired = self.platform.cpu_set_ids_for_mask(logical_processor_mask)?;
        normalize_cpu_set_ids(&mut desired);
        let released_affinity = self.release_affinity_for_switch(identity, process)?;
        if desired.is_empty() {
            if let Err(error) = self.release_cpu_sets_if_managed(identity, process) {
                return Err(self.finish_switch_failure(
                    error,
                    released_affinity.map(SwitchCompensation::Affinity),
                    identity,
                    process,
                ));
            }
            return Ok(CpuAllocationApplyOutcome::NoUsableTarget);
        }

        match self.apply_cpu_sets_value(identity, process, owner, desired) {
            Ok(outcome) => Ok(outcome),
            Err(failure) => {
                let compensation = (!failure.uncertain)
                    .then_some(released_affinity)
                    .flatten()
                    .map(SwitchCompensation::Affinity);
                Err(self.finish_switch_failure(failure.error, compensation, identity, process))
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
        let (current, system) = self.platform.query_affinity(process)?;
        let baseline = self
            .managed_affinity
            .get(identity)
            .map_or(current, |managed| managed.baseline);
        let desired = match request {
            HardAffinityRequest::Exact(mask) => target_affinity_mask(mask, system),
            HardAffinityRequest::Limit(maximum) => limited_affinity_mask(baseline, system, maximum),
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
            CommitFailureBehavior::Compensate,
        ) {
            Ok(()) => {
                self.managed_affinity.insert(
                    identity.clone(),
                    ManagedValue {
                        baseline,
                        expected: desired,
                        owner,
                        apply_sequence: sequence,
                    },
                );
                Ok(CpuAllocationApplyOutcome::Applied)
            }
            Err(mut failure) => {
                if failure.relinquish_recovery {
                    if let Err(error) = self.platform.relinquish_affinity(identity) {
                        failure
                            .message
                            .push_str(&format!(" Recovery journal relinquish failed: {error}."));
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
                        },
                    );
                } else if let Some(managed) = managed {
                    self.managed_affinity.insert(identity.clone(), managed);
                }
                Err(PropertyApplyFailure {
                    error: ProcessControlError::Failed(failure.message),
                    uncertain: failure.uncertain,
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
            CommitFailureBehavior::Compensate,
        ) {
            Ok(()) => {
                self.managed_cpu_sets.insert(
                    identity.clone(),
                    ManagedValue {
                        baseline,
                        expected: desired,
                        owner,
                        apply_sequence: sequence,
                    },
                );
                Ok(CpuAllocationApplyOutcome::Applied)
            }
            Err(mut failure) => {
                if failure.relinquish_recovery {
                    if let Err(error) = self.platform.relinquish_cpu_sets(identity) {
                        failure
                            .message
                            .push_str(&format!(" Recovery journal relinquish failed: {error}."));
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
                        },
                    );
                } else if let Some(managed) = managed {
                    self.managed_cpu_sets.insert(identity.clone(), managed);
                }
                Err(PropertyApplyFailure {
                    error: ProcessControlError::Failed(failure.message),
                    uncertain: failure.uncertain,
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
        previous: ManagedValue<usize>,
    ) -> Result<(), ProcessControlError> {
        let current = self.platform.query_affinity(process)?.0;
        if current != previous.expected {
            if let Err(mut failure) = apply_affinity_transition(
                &mut self.platform,
                process,
                current,
                previous.expected,
                CommitFailureBehavior::Compensate,
            ) {
                if failure.relinquish_recovery {
                    if let Err(error) = self.platform.relinquish_affinity(identity) {
                        failure
                            .message
                            .push_str(&format!(" Recovery journal relinquish failed: {error}."));
                    }
                }
                self.managed_affinity.insert(identity.clone(), previous);
                return Err(ProcessControlError::Failed(failure.message));
            }
        }
        self.managed_affinity.insert(identity.clone(), previous);
        Ok(())
    }

    fn reapply_cpu_sets(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
        previous: ManagedValue<Vec<u32>>,
    ) -> Result<(), ProcessControlError> {
        let mut current = self.platform.query_cpu_sets(process)?;
        normalize_cpu_set_ids(&mut current);
        if current != previous.expected {
            if let Err(mut failure) = apply_cpu_sets_transition(
                &mut self.platform,
                process,
                &current,
                &previous.expected,
                CommitFailureBehavior::Compensate,
            ) {
                if failure.relinquish_recovery {
                    if let Err(error) = self.platform.relinquish_cpu_sets(identity) {
                        failure
                            .message
                            .push_str(&format!(" Recovery journal relinquish failed: {error}."));
                    }
                }
                self.managed_cpu_sets.insert(identity.clone(), previous);
                return Err(ProcessControlError::Failed(failure.message));
            }
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
        summary
    }

    pub(crate) fn release_all_policy(
        &mut self,
        owner: ControlOwner,
    ) -> CpuAllocationReleaseSummary {
        self.release_policy_except(owner, &BTreeSet::new())
    }

    pub(crate) fn reconcile_pending(
        &mut self,
        allow_cross_session_process_control: bool,
        include_release_retries: bool,
    ) -> CpuAllocationReconciliationSummary {
        let pending = take(&mut self.pending_reconciliations);
        let mut summary = CpuAllocationReconciliationSummary::default();
        for (key, reconciliation) in pending {
            if matches!(
                reconciliation,
                PendingReconciliation::ReleaseOnlyRetry { .. }
            ) && !include_release_retries
            {
                self.pending_reconciliations.insert(key, reconciliation);
                continue;
            }
            if let PendingReconciliation::ReleaseOnlyFirstAttempt { failed_claim }
            | PendingReconciliation::ReleaseOnlyRetry { failed_claim } = reconciliation
            {
                let mut release = CpuAllocationReleaseSummary::default();
                let completed = self.release_managed_key(&key, &mut release);
                summary
                    .releases
                    .restored_owners
                    .append(&mut release.restored_owners);
                if matches!(
                    reconciliation,
                    PendingReconciliation::ReleaseOnlyFirstAttempt { .. }
                ) {
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
            match self.apply_effective_claim(claim.clone(), allow_cross_session_process_control) {
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
                Err(error) => {
                    summary
                        .failures
                        .push(reconciliation_failure_for_claim(&claim, error));
                    if !self.release_managed_key(&key, &mut summary.releases) {
                        self.pending_reconciliations.insert(
                            key,
                            PendingReconciliation::ReleaseOnlyRetry {
                                failed_claim: Some(failed_claim),
                            },
                        );
                    }
                }
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
        self.pending_reconciliations
            .values()
            .any(|pending| matches!(pending, PendingReconciliation::ReleaseOnlyRetry { .. }))
    }

    fn reconcile_policy_key(
        &mut self,
        key: &ProcessTargetKey,
        summary: &mut CpuAllocationReleaseSummary,
    ) {
        if let Some(effective) = self.effective_claim(key) {
            let effective = claim_fingerprint(effective);
            let should_queue_handoff = match self.pending_reconciliations.get(key) {
                Some(PendingReconciliation::Handoff) => false,
                Some(
                    PendingReconciliation::ReleaseOnlyFirstAttempt {
                        failed_claim: Some(failed),
                    }
                    | PendingReconciliation::ReleaseOnlyRetry {
                        failed_claim: Some(failed),
                    },
                ) => *failed != effective,
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
        let effective = self.effective_claim(key).map(claim_fingerprint);
        if effective.is_some() && effective != failed_claim {
            self.pending_reconciliations
                .insert(key.clone(), PendingReconciliation::Handoff);
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
            CommitFailureBehavior::KeepExpected,
        ) {
            Ok(()) => Ok(true),
            Err(failure) if failure.expected_preserved => {
                match self.platform.relinquish_affinity(identity) {
                    Ok(()) => Ok(true),
                    Err(error) => {
                        if let Some(managed) = self.managed_affinity.get_mut(identity) {
                            managed.expected = managed.baseline;
                        }
                        Err(ProcessControlError::Failed(format!(
                            "{} Recovery journal relinquish failed: {error}.",
                            failure.message
                        )))
                    }
                }
            }
            Err(failure) => Err(ProcessControlError::Failed(failure.message)),
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
            self.platform.relinquish_cpu_sets(identity)?;
            return Ok(false);
        }
        match apply_cpu_sets_transition(
            &mut self.platform,
            process,
            &current,
            &managed.baseline,
            CommitFailureBehavior::KeepExpected,
        ) {
            Ok(()) => Ok(true),
            Err(failure) if failure.expected_preserved => {
                match self.platform.relinquish_cpu_sets(identity) {
                    Ok(()) => Ok(true),
                    Err(error) => {
                        if let Some(managed) = self.managed_cpu_sets.get_mut(identity) {
                            managed.expected = managed.baseline.clone();
                        }
                        Err(ProcessControlError::Failed(format!(
                            "{} Recovery journal relinquish failed: {error}.",
                            failure.message
                        )))
                    }
                }
            }
            Err(failure) => Err(ProcessControlError::Failed(failure.message)),
        }
    }

    pub(crate) fn policy_managed_process_names(&self, owner: ControlOwner) -> Vec<String> {
        self.managed_identities(owner)
            .into_iter()
            .map(|identity| identity.name.clone())
            .collect()
    }

    pub(crate) fn policy_managed_process_paths(&self, owner: ControlOwner) -> Vec<String> {
        self.managed_identities(owner)
            .into_iter()
            .map(|identity| identity.executable_path.to_string_lossy().into_owned())
            .collect()
    }

    pub(crate) fn policy_managed_process_count(&self, owner: ControlOwner) -> usize {
        self.managed_identities(owner).len()
    }

    fn managed_identities(&self, owner: ControlOwner) -> BTreeSet<&ProcessIdentity> {
        self.managed_affinity
            .iter()
            .filter(|(_, managed)| managed.owner == owner)
            .map(|(identity, _)| identity)
            .chain(
                self.managed_cpu_sets
                    .iter()
                    .filter(|(_, managed)| managed.owner == owner)
                    .map(|(identity, _)| identity),
            )
            .collect()
    }

    pub(crate) fn has_managed_state(&self) -> bool {
        !self.managed_affinity.is_empty() || !self.managed_cpu_sets.is_empty()
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        self.claims.clear();
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
    Limit(u8),
}

enum SwitchCompensation {
    Affinity(ManagedValue<usize>),
    CpuSets(ManagedValue<Vec<u32>>),
}

struct PropertyApplyFailure {
    error: ProcessControlError,
    uncertain: bool,
}

impl PropertyApplyFailure {
    fn certain(error: ProcessControlError) -> Self {
        Self {
            error,
            uncertain: false,
        }
    }
}

struct TransitionFailure {
    message: String,
    uncertain: bool,
    relinquish_recovery: bool,
    expected_preserved: bool,
}

#[derive(Clone, Copy)]
enum CommitFailureBehavior {
    Compensate,
    KeepExpected,
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
            platform,
            process,
            original,
            intent,
            error.to_string(),
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
                format!(
                    "Processor Affinity (Hard) verification returned {actual:#x}, expected {expected:#x}."
                ),
            ));
        }
        Err(error) => {
            return Err(compensate_affinity_with_intent(
                platform,
                process,
                original,
                intent,
                format!("Processor Affinity (Hard) verification failed: {error}"),
            ));
        }
    }

    if let Err(error) = intent.commit() {
        let message = format!("Crash recovery commit failed: {error}");
        return match commit_failure_behavior {
            CommitFailureBehavior::Compensate => Err(compensate_affinity_without_intent(
                platform, process, original, message,
            )),
            CommitFailureBehavior::KeepExpected => Err(TransitionFailure {
                message,
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
            platform,
            process,
            original,
            intent,
            error.to_string(),
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
                    format!(
                        "CPU Sets (Soft) verification returned {actual:?}, expected {expected:?}."
                    ),
                ));
            }
        }
        Err(error) => {
            return Err(compensate_cpu_sets_with_intent(
                platform,
                process,
                original,
                intent,
                format!("CPU Sets (Soft) verification failed: {error}"),
            ));
        }
    }

    if let Err(error) = intent.commit() {
        let message = format!("Crash recovery commit failed: {error}");
        return match commit_failure_behavior {
            CommitFailureBehavior::Compensate => Err(compensate_cpu_sets_without_intent(
                platform, process, original, message,
            )),
            CommitFailureBehavior::KeepExpected => Err(TransitionFailure {
                message,
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
    primary_error: String,
) -> TransitionFailure {
    match restore_and_verify_affinity(platform, process, original) {
        Ok(()) => TransitionFailure {
            message: primary_error,
            uncertain: false,
            relinquish_recovery: false,
            expected_preserved: false,
        },
        Err(compensation_error) => {
            let recovery_error = intent.commit().err();
            TransitionFailure {
                message: transition_failure_message(
                    primary_error,
                    compensation_error.to_string(),
                    recovery_error,
                ),
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
    primary_error: String,
) -> TransitionFailure {
    match restore_and_verify_cpu_sets(platform, process, original) {
        Ok(()) => TransitionFailure {
            message: primary_error,
            uncertain: false,
            relinquish_recovery: false,
            expected_preserved: false,
        },
        Err(compensation_error) => {
            let recovery_error = intent.commit().err();
            TransitionFailure {
                message: transition_failure_message(
                    primary_error,
                    compensation_error.to_string(),
                    recovery_error,
                ),
                uncertain: true,
                relinquish_recovery: false,
                expected_preserved: false,
            }
        }
    }
}

fn compensate_affinity_without_intent<P: CpuAllocationPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: usize,
    primary_error: String,
) -> TransitionFailure {
    match restore_and_verify_affinity(platform, process, original) {
        Ok(()) => TransitionFailure {
            message: primary_error,
            uncertain: false,
            relinquish_recovery: true,
            expected_preserved: false,
        },
        Err(compensation_error) => TransitionFailure {
            message: transition_failure_message(
                primary_error,
                compensation_error.to_string(),
                None,
            ),
            uncertain: true,
            relinquish_recovery: false,
            expected_preserved: false,
        },
    }
}

fn compensate_cpu_sets_without_intent<P: CpuAllocationPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: &[u32],
    primary_error: String,
) -> TransitionFailure {
    match restore_and_verify_cpu_sets(platform, process, original) {
        Ok(()) => TransitionFailure {
            message: primary_error,
            uncertain: false,
            relinquish_recovery: true,
            expected_preserved: false,
        },
        Err(compensation_error) => TransitionFailure {
            message: transition_failure_message(
                primary_error,
                compensation_error.to_string(),
                None,
            ),
            uncertain: true,
            relinquish_recovery: false,
            expected_preserved: false,
        },
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
        message: error.to_string(),
        uncertain: false,
        relinquish_recovery: false,
        expected_preserved: false,
    }
}

fn transition_failure_message(
    primary_error: String,
    compensation_error: String,
    recovery_error: Option<String>,
) -> String {
    let mut message = format!("{primary_error} Compensation failed: {compensation_error}.");
    if let Some(recovery_error) = recovery_error {
        message.push_str(&format!(
            " Recovery journal commit failed: {recovery_error}."
        ));
    }
    message
}

fn cpu_allocation_owner_precedence() -> &'static [ControlOwner] {
    &[
        ControlOwner::CpuSetsSoft,
        ControlOwner::ProcessorAffinityHard,
        ControlOwner::CoreLimiter,
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
    let mut mask = (rule_mask & usize::MAX as u64) as usize;
    if system_affinity != 0 {
        mask &= system_affinity;
    }
    (mask != 0).then_some(mask)
}

fn limited_affinity_mask(
    baseline_affinity: usize,
    system_affinity: usize,
    maximum: u8,
) -> Option<usize> {
    let available = if baseline_affinity != 0 {
        baseline_affinity
    } else {
        system_affinity
    };
    let mut target = 0_usize;
    let mut selected = 0_usize;
    for bit in 0..usize::BITS as usize {
        let processor = 1_usize << bit;
        if available & processor != 0 {
            target |= processor;
            selected += 1;
            if selected >= usize::from(maximum.max(1)) {
                break;
            }
        }
    }
    (target != 0 && target != baseline_affinity).then_some(target)
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
        request: claim.request,
    }
}

fn claim_fingerprint(claim: &CpuAllocationClaim) -> CpuAllocationClaimFingerprint {
    CpuAllocationClaimFingerprint {
        owner: claim.owner,
        request: claim.request,
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
        ProcessOperationError::ProcessExited => ProcessControlError::ProcessExited,
        ProcessOperationError::Failed { operation, code } => {
            ProcessControlError::Failed(format!("{operation} failed with Windows error {code}."))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::BTreeMap,
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        rc::Rc,
        time::{Duration, Instant},
    };

    use super::*;

    #[derive(Clone)]
    struct FakeProcessState {
        identity: ProcessIdentity,
        affinity: usize,
        system_affinity: usize,
        cpu_sets: Vec<u32>,
    }

    #[derive(Default)]
    struct FakeState {
        processes: BTreeMap<u32, FakeProcessState>,
        events: Vec<String>,
        reject_disallowed_cross_session_open: bool,
        open_cross_session_flags: Vec<bool>,
        affinity_apply_failures_remaining: usize,
        fail_next_cpu_sets_apply: bool,
        fail_next_commit: bool,
        fail_next_affinity_relinquish: bool,
        fail_next_cpu_sets_relinquish: bool,
    }

    #[derive(Clone, Default)]
    struct FakePlatform {
        state: Rc<RefCell<FakeState>>,
    }

    struct FakeIntent {
        state: Rc<RefCell<FakeState>>,
        fail: bool,
    }

    impl CpuAllocationRecoveryIntent for FakeIntent {
        fn commit(self) -> Result<(), String> {
            if self.fail {
                Err("injected commit failure".to_owned())
            } else {
                self.state.borrow_mut().events.push("commit".to_owned());
                Ok(())
            }
        }
    }

    impl CpuAllocationPlatform for FakePlatform {
        type Process = u32;
        type RecoveryIntent = FakeIntent;

        fn open(
            &mut self,
            target: &ProcessControlTarget,
            allow_cross_session_process_control: bool,
        ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError> {
            let mut state = self.state.borrow_mut();
            state
                .open_cross_session_flags
                .push(allow_cross_session_process_control);
            if state.reject_disallowed_cross_session_open && !allow_cross_session_process_control {
                return Err(ProcessControlError::AccessDenied(
                    "injected cross-session denial".to_owned(),
                ));
            }
            let Some(process) = state.processes.get(&target.id) else {
                return Err(ProcessControlError::ProcessExited);
            };
            if process.identity.creation_time != target.creation_time
                || !crate::foreground::same_executable_path(
                    &process.identity.executable_path,
                    &target.executable_path,
                )
            {
                return Err(ProcessControlError::ProcessExited);
            }
            Ok((process.identity.clone(), target.id))
        }

        fn query_affinity(
            &mut self,
            process: &Self::Process,
        ) -> Result<(usize, usize), ProcessControlError> {
            let state = self.state.borrow();
            let process = state
                .processes
                .get(process)
                .ok_or(ProcessControlError::ProcessExited)?;
            Ok((process.affinity, process.system_affinity))
        }

        fn query_cpu_sets(
            &mut self,
            process: &Self::Process,
        ) -> Result<Vec<u32>, ProcessControlError> {
            self.state
                .borrow()
                .processes
                .get(process)
                .map(|process| process.cpu_sets.clone())
                .ok_or(ProcessControlError::ProcessExited)
        }

        fn cpu_set_ids_for_mask(&mut self, mask: u64) -> Result<Vec<u32>, ProcessControlError> {
            Ok((0..64)
                .filter(|bit| mask & (1_u64 << bit) != 0)
                .map(|bit| 100 + bit)
                .collect())
        }

        fn begin_affinity_change(
            &mut self,
            _process: &Self::Process,
            original: usize,
            expected: usize,
        ) -> Result<Self::RecoveryIntent, ProcessControlError> {
            let mut state = self.state.borrow_mut();
            state
                .events
                .push(format!("begin-affinity:{original:#x}->{expected:#x}"));
            let fail = std::mem::take(&mut state.fail_next_commit);
            Ok(FakeIntent {
                state: Rc::clone(&self.state),
                fail,
            })
        }

        fn begin_cpu_sets_change(
            &mut self,
            _process: &Self::Process,
            original: &[u32],
            expected: &[u32],
        ) -> Result<Self::RecoveryIntent, ProcessControlError> {
            let mut state = self.state.borrow_mut();
            state
                .events
                .push(format!("begin-cpu-sets:{original:?}->{expected:?}"));
            let fail = std::mem::take(&mut state.fail_next_commit);
            Ok(FakeIntent {
                state: Rc::clone(&self.state),
                fail,
            })
        }

        fn apply_affinity(
            &mut self,
            process: &Self::Process,
            affinity: usize,
        ) -> Result<(), ProcessControlError> {
            let mut state = self.state.borrow_mut();
            state.events.push(format!("apply-affinity:{affinity:#x}"));
            if state.affinity_apply_failures_remaining > 0 {
                state.affinity_apply_failures_remaining -= 1;
                return Err(ProcessControlError::Failed(
                    "injected affinity apply failure".to_owned(),
                ));
            }
            state
                .processes
                .get_mut(process)
                .ok_or(ProcessControlError::ProcessExited)?
                .affinity = affinity;
            Ok(())
        }

        fn apply_cpu_sets(
            &mut self,
            process: &Self::Process,
            ids: &[u32],
        ) -> Result<(), ProcessControlError> {
            let mut state = self.state.borrow_mut();
            state.events.push(format!("apply-cpu-sets:{ids:?}"));
            if std::mem::take(&mut state.fail_next_cpu_sets_apply) {
                return Err(ProcessControlError::Failed(
                    "injected CPU Sets apply failure".to_owned(),
                ));
            }
            state
                .processes
                .get_mut(process)
                .ok_or(ProcessControlError::ProcessExited)?
                .cpu_sets = ids.to_vec();
            Ok(())
        }

        fn relinquish_affinity(
            &mut self,
            identity: &ProcessIdentity,
        ) -> Result<(), ProcessControlError> {
            let mut state = self.state.borrow_mut();
            state
                .events
                .push(format!("forget-affinity:{}", identity.creation_time));
            if std::mem::take(&mut state.fail_next_affinity_relinquish) {
                return Err(ProcessControlError::Failed(
                    "injected affinity relinquish failure".to_owned(),
                ));
            }
            Ok(())
        }

        fn relinquish_cpu_sets(
            &mut self,
            identity: &ProcessIdentity,
        ) -> Result<(), ProcessControlError> {
            let mut state = self.state.borrow_mut();
            state
                .events
                .push(format!("forget-cpu-sets:{}", identity.creation_time));
            if std::mem::take(&mut state.fail_next_cpu_sets_relinquish) {
                return Err(ProcessControlError::Failed(
                    "injected CPU Sets relinquish failure".to_owned(),
                ));
            }
            Ok(())
        }
    }

    fn identity(id: u32, creation_time: u64) -> ProcessIdentity {
        ProcessIdentity::new(
            id,
            "worker.exe".to_owned(),
            PathBuf::from(r"C:\Apps\worker.exe"),
            creation_time,
            Some(1),
        )
    }

    fn target(id: u32, creation_time: u64) -> ProcessControlTarget {
        ProcessControlTarget::automatic(
            id,
            "worker.exe".to_owned(),
            PathBuf::from(r"C:\Apps\worker.exe"),
            creation_time,
        )
    }

    fn coordinator() -> CpuAllocationCoordinator<FakePlatform> {
        let platform = FakePlatform::default();
        platform.state.borrow_mut().processes.insert(
            42,
            FakeProcessState {
                identity: identity(42, 7),
                affinity: 0b1111,
                system_affinity: 0b1111,
                cpu_sets: Vec::new(),
            },
        );
        CpuAllocationCoordinator::with_platform(platform)
    }

    fn claim(
        owner: ControlOwner,
        request: CpuAllocationRequest,
        creation_time: u64,
    ) -> CpuAllocationClaim {
        claim_for(42, owner, request, creation_time)
    }

    fn claim_for(
        process_id: u32,
        owner: ControlOwner,
        request: CpuAllocationRequest,
        creation_time: u64,
    ) -> CpuAllocationClaim {
        CpuAllocationClaim {
            target: target(process_id, creation_time),
            owner,
            request,
        }
    }

    fn process_state(coordinator: &CpuAllocationCoordinator<FakePlatform>) -> FakeProcessState {
        coordinator
            .platform
            .state
            .borrow()
            .processes
            .get(&42)
            .cloned()
            .expect("fake process should exist")
    }

    #[test]
    fn pass_end_reconciliation_hands_off_precedence_without_resubmission() {
        let mut coordinator = coordinator();
        let workload_key = target(42, 7).key();
        assert_eq!(
            coordinator.apply_policy_claim(
                claim(
                    ControlOwner::AdaptiveEngine,
                    CpuAllocationRequest::HardAffinity {
                        logical_processor_mask: 0b0011,
                    },
                    7,
                ),
                true,
            ),
            Ok(CpuAllocationApplyOutcome::Applied)
        );
        assert_eq!(process_state(&coordinator).affinity, 0b0011);

        assert_eq!(
            coordinator.apply_policy_claim(
                claim(
                    ControlOwner::CoreLimiter,
                    CpuAllocationRequest::LimitLogicalProcessors { maximum: 1 },
                    7,
                ),
                true,
            ),
            Ok(CpuAllocationApplyOutcome::Applied)
        );
        assert_eq!(process_state(&coordinator).affinity, 0b0001);

        assert_eq!(
            coordinator.apply_policy_claim(
                claim(
                    ControlOwner::CpuSetsSoft,
                    CpuAllocationRequest::SoftCpuSets {
                        logical_processor_mask: 0b0010,
                    },
                    7,
                ),
                true,
            ),
            Ok(CpuAllocationApplyOutcome::Applied)
        );
        let state = process_state(&coordinator);
        assert_eq!(state.affinity, 0b1111);
        assert_eq!(state.cpu_sets, vec![101]);
        assert!(coordinator.managed_affinity.is_empty());
        assert_eq!(coordinator.managed_cpu_sets.len(), 1);

        assert_eq!(
            coordinator.apply_policy_claim(
                claim(
                    ControlOwner::ProcessorAffinityHard,
                    CpuAllocationRequest::HardAffinity {
                        logical_processor_mask: 0b0110,
                    },
                    7,
                ),
                true,
            ),
            Ok(CpuAllocationApplyOutcome::Shadowed)
        );
        let summary = coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
        assert!(summary.restored_owners.is_empty());
        assert!(coordinator.has_pending_immediate_reconciliation());
        let state = process_state(&coordinator);
        assert_eq!(state.cpu_sets, vec![101]);
        assert_eq!(state.affinity, 0b1111);

        let reconciliation = coordinator.reconcile_pending(true, true);
        assert!(reconciliation.failures.is_empty());
        assert_eq!(reconciliation.applications.len(), 1);
        assert_eq!(
            reconciliation.applications[0].owner,
            ControlOwner::ProcessorAffinityHard
        );
        let state = process_state(&coordinator);
        assert!(state.cpu_sets.is_empty());
        assert_eq!(state.affinity, 0b0110);
        assert!(!coordinator.has_pending_immediate_reconciliation());

        coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
        assert_eq!(process_state(&coordinator).affinity, 0b0110);
        let reconciliation = coordinator.reconcile_pending(true, true);
        assert!(reconciliation.failures.is_empty());
        assert_eq!(reconciliation.applications.len(), 1);
        assert_eq!(
            reconciliation.applications[0].owner,
            ControlOwner::CoreLimiter
        );
        assert_eq!(process_state(&coordinator).affinity, 0b0001);

        coordinator.release_all_policy(ControlOwner::CoreLimiter);
        assert_eq!(process_state(&coordinator).affinity, 0b0001);
        let reconciliation = coordinator.reconcile_pending(true, true);
        assert!(reconciliation.failures.is_empty());
        assert_eq!(reconciliation.applications.len(), 1);
        assert_eq!(
            reconciliation.applications[0].owner,
            ControlOwner::AdaptiveEngine
        );
        assert_eq!(process_state(&coordinator).affinity, 0b0011);

        coordinator.release_policy_except(ControlOwner::AdaptiveEngine, &BTreeSet::new());
        assert_eq!(process_state(&coordinator).affinity, 0b1111);
        assert!(coordinator.effective_claim(&workload_key).is_none());
        assert!(!coordinator.has_managed_state());
        assert!(!coordinator.has_pending_reconciliation());
    }

    #[test]
    fn immediate_handoff_skips_an_unrelated_release_retry() {
        let mut coordinator = coordinator();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::CpuSetsSoft,
                    CpuAllocationRequest::SoftCpuSets {
                        logical_processor_mask: 0b0010,
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        {
            let mut state = coordinator.platform.state.borrow_mut();
            state.processes.get_mut(&42).unwrap().cpu_sets = vec![103];
            state.fail_next_cpu_sets_relinquish = true;
        }
        let release = coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
        assert_eq!(release.failures.len(), 1);
        assert!(coordinator.has_pending_release_retry());

        coordinator.platform.state.borrow_mut().processes.insert(
            43,
            FakeProcessState {
                identity: identity(43, 8),
                affinity: 0b1111,
                system_affinity: 0b1111,
                cpu_sets: Vec::new(),
            },
        );
        coordinator
            .apply_policy_claim(
                claim_for(
                    43,
                    ControlOwner::CoreLimiter,
                    CpuAllocationRequest::LimitLogicalProcessors { maximum: 1 },
                    8,
                ),
                true,
            )
            .unwrap();
        coordinator
            .apply_policy_claim(
                claim_for(
                    43,
                    ControlOwner::ProcessorAffinityHard,
                    CpuAllocationRequest::HardAffinity {
                        logical_processor_mask: 0b0110,
                    },
                    8,
                ),
                true,
            )
            .unwrap();
        coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
        assert!(coordinator.has_pending_immediate_reconciliation());
        coordinator
            .platform
            .state
            .borrow_mut()
            .fail_next_cpu_sets_relinquish = true;

        let handoff = coordinator.reconcile_pending(true, false);
        assert_eq!(handoff.applications.len(), 1);
        assert_eq!(handoff.applications[0].process_id, 43);
        assert!(
            coordinator
                .platform
                .state
                .borrow()
                .fail_next_cpu_sets_relinquish
        );
        assert!(coordinator.has_pending_release_retry());

        let failed_retry = coordinator.reconcile_pending(true, true);
        assert!(failed_retry.releases.failures.is_empty());
        assert!(coordinator.has_pending_release_retry());
        let completed_retry = coordinator.reconcile_pending(true, true);
        assert!(completed_retry.releases.failures.is_empty());
        assert!(!coordinator.has_pending_release_retry());
        coordinator.release_all_policy(ControlOwner::CoreLimiter);
    }

    #[test]
    fn changed_effective_claim_promotes_a_cleanup_retry_to_handoff() {
        let mut coordinator = coordinator();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::CoreLimiter,
                    CpuAllocationRequest::LimitLogicalProcessors { maximum: 1 },
                    7,
                ),
                true,
            )
            .unwrap();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::CpuSetsSoft,
                    CpuAllocationRequest::SoftCpuSets {
                        logical_processor_mask: 0b0010,
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::ProcessorAffinityHard,
                    CpuAllocationRequest::HardAffinity {
                        logical_processor_mask: 0b0110,
                    },
                    7,
                ),
                true,
            )
            .unwrap();

        coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
        {
            let mut state = coordinator.platform.state.borrow_mut();
            state.reject_disallowed_cross_session_open = true;
            state.fail_next_commit = true;
            state.fail_next_cpu_sets_relinquish = true;
        }
        let failed_handoff = coordinator.reconcile_pending(false, false);
        assert_eq!(failed_handoff.failures.len(), 1);
        assert_eq!(failed_handoff.releases.failures.len(), 1);
        assert!(coordinator.has_pending_release_retry());
        assert!(!coordinator.has_pending_immediate_reconciliation());

        coordinator
            .platform
            .state
            .borrow_mut()
            .reject_disallowed_cross_session_open = false;
        coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
        assert!(coordinator.has_pending_immediate_reconciliation());
        assert!(!coordinator.has_pending_release_retry());

        let handoff = coordinator.reconcile_pending(true, false);
        assert!(handoff.failures.is_empty());
        assert_eq!(handoff.applications.len(), 1);
        assert_eq!(handoff.applications[0].owner, ControlOwner::CoreLimiter);
        assert_eq!(process_state(&coordinator).affinity, 0b0001);
        assert!(process_state(&coordinator).cpu_sets.is_empty());
        coordinator.release_all_policy(ControlOwner::CoreLimiter);
    }

    #[test]
    fn final_lower_claim_removal_restores_the_actual_managed_owner() {
        let mut coordinator = coordinator();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::AdaptiveEngine,
                    CpuAllocationRequest::HardAffinity {
                        logical_processor_mask: 0b0011,
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::CpuSetsSoft,
                    CpuAllocationRequest::SoftCpuSets {
                        logical_processor_mask: 0b0010,
                    },
                    7,
                ),
                true,
            )
            .unwrap();

        let soft_summary = coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
        assert!(soft_summary.restored_owners.is_empty());
        assert_eq!(process_state(&coordinator).cpu_sets, vec![101]);

        let adaptive_summary = coordinator.release_all_policy(ControlOwner::AdaptiveEngine);
        assert_eq!(
            adaptive_summary.restored_owners,
            vec![ControlOwner::CpuSetsSoft]
        );
        let state = process_state(&coordinator);
        assert!(state.cpu_sets.is_empty());
        assert_eq!(state.affinity, 0b1111);
        assert!(!coordinator.has_managed_state());
        assert!(!coordinator.has_pending_reconciliation());
    }

    #[test]
    fn releasing_a_shadowed_claim_does_not_reconcile_the_effective_owner() {
        let mut coordinator = coordinator();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::CpuSetsSoft,
                    CpuAllocationRequest::SoftCpuSets {
                        logical_processor_mask: 0b0010,
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        assert_eq!(
            coordinator.apply_policy_claim(
                claim(
                    ControlOwner::ProcessorAffinityHard,
                    CpuAllocationRequest::HardAffinity {
                        logical_processor_mask: 0b0011,
                    },
                    7,
                ),
                true,
            ),
            Ok(CpuAllocationApplyOutcome::Shadowed)
        );

        let summary = coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
        assert!(summary.restored_owners.is_empty());
        assert!(summary.failures.is_empty());
        assert!(!coordinator.has_pending_reconciliation());
        let state = process_state(&coordinator);
        assert_eq!(state.cpu_sets, vec![101]);
        assert_eq!(state.affinity, 0b1111);
    }

    #[test]
    fn failed_soft_mode_switch_restores_the_previous_hard_claim() {
        let mut coordinator = coordinator();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::AdaptiveEngine,
                    CpuAllocationRequest::HardAffinity {
                        logical_processor_mask: 0b0011,
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        coordinator
            .platform
            .state
            .borrow_mut()
            .fail_next_cpu_sets_apply = true;

        let result = coordinator.apply_policy_claim(
            claim(
                ControlOwner::CpuSetsSoft,
                CpuAllocationRequest::SoftCpuSets {
                    logical_processor_mask: 0b0100,
                },
                7,
            ),
            true,
        );
        assert!(result.is_err());
        let state = process_state(&coordinator);
        assert_eq!(state.affinity, 0b0011);
        assert!(state.cpu_sets.is_empty());
        assert_eq!(coordinator.managed_affinity.len(), 1);
        assert!(coordinator.managed_cpu_sets.is_empty());
        assert!(coordinator
            .claims
            .get(&ControlOwner::CpuSetsSoft)
            .is_none_or(BTreeMap::is_empty));
    }

    #[test]
    fn failed_hard_mode_switch_restores_the_previous_soft_claim() {
        let mut coordinator = coordinator();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::AdaptiveEngine,
                    CpuAllocationRequest::SoftCpuSets {
                        logical_processor_mask: 0b0010,
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        coordinator
            .platform
            .state
            .borrow_mut()
            .affinity_apply_failures_remaining = 1;

        let result = coordinator.apply_policy_claim(
            claim(
                ControlOwner::ProcessorAffinityHard,
                CpuAllocationRequest::HardAffinity {
                    logical_processor_mask: 0b0110,
                },
                7,
            ),
            true,
        );
        assert!(result.is_err());
        let state = process_state(&coordinator);
        assert_eq!(state.affinity, 0b1111);
        assert_eq!(state.cpu_sets, vec![101]);
        assert!(coordinator.managed_affinity.is_empty());
        assert_eq!(coordinator.managed_cpu_sets.len(), 1);
    }

    #[test]
    fn uncertain_switch_reapply_keeps_restoration_state() {
        let mut coordinator = coordinator();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::AdaptiveEngine,
                    CpuAllocationRequest::HardAffinity {
                        logical_processor_mask: 0b0011,
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        let identity = identity(42, 7);
        let previous = coordinator
            .release_affinity_for_switch(&identity, &42)
            .unwrap()
            .unwrap();
        coordinator
            .platform
            .state
            .borrow_mut()
            .affinity_apply_failures_remaining = 2;

        let result = coordinator.reapply_affinity(&identity, &42, previous);
        assert!(result.is_err());
        assert_eq!(process_state(&coordinator).affinity, 0b1111);
        assert_eq!(coordinator.managed_affinity.len(), 1);
        assert_eq!(
            coordinator.managed_affinity.values().next().unwrap().owner,
            ControlOwner::AdaptiveEngine
        );

        let summary = coordinator.release_all_policy(ControlOwner::AdaptiveEngine);
        assert!(summary.failures.is_empty());
        assert!(!coordinator.has_managed_state());
    }

    #[test]
    fn external_affinity_break_is_relinquished_without_overwrite() {
        let mut coordinator = coordinator();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::ProcessorAffinityHard,
                    CpuAllocationRequest::HardAffinity {
                        logical_processor_mask: 0b0011,
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        coordinator
            .platform
            .state
            .borrow_mut()
            .processes
            .get_mut(&42)
            .unwrap()
            .affinity = 0b0101;

        let summary = coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
        assert!(summary.failures.is_empty());
        assert!(summary.restored_owners.is_empty());
        assert_eq!(process_state(&coordinator).affinity, 0b0101);
        assert!(coordinator.managed_affinity.is_empty());
        assert!(coordinator
            .platform
            .state
            .borrow()
            .events
            .iter()
            .any(|event| event == "forget-affinity:7"));
    }

    #[test]
    fn external_cpu_sets_break_is_relinquished_without_overwrite() {
        let mut coordinator = coordinator();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::CpuSetsSoft,
                    CpuAllocationRequest::SoftCpuSets {
                        logical_processor_mask: 0b0010,
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        coordinator
            .platform
            .state
            .borrow_mut()
            .processes
            .get_mut(&42)
            .unwrap()
            .cpu_sets = vec![103];

        let summary = coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
        assert!(summary.failures.is_empty());
        assert!(summary.restored_owners.is_empty());
        assert_eq!(process_state(&coordinator).cpu_sets, vec![103]);
        assert!(coordinator.managed_cpu_sets.is_empty());
        assert!(coordinator
            .platform
            .state
            .borrow()
            .events
            .iter()
            .any(|event| event == "forget-cpu-sets:7"));
    }

    #[test]
    fn failed_release_is_retried_without_reapplying_a_claim() {
        let mut coordinator = coordinator();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::CpuSetsSoft,
                    CpuAllocationRequest::SoftCpuSets {
                        logical_processor_mask: 0b0010,
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        {
            let mut state = coordinator.platform.state.borrow_mut();
            state.processes.get_mut(&42).unwrap().cpu_sets = vec![103];
            state.fail_next_cpu_sets_relinquish = true;
        }

        let release = coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
        assert_eq!(release.failures.len(), 1);
        assert!(coordinator.has_pending_reconciliation());
        assert!(!coordinator.has_pending_immediate_reconciliation());
        assert_eq!(process_state(&coordinator).cpu_sets, vec![103]);

        coordinator
            .platform
            .state
            .borrow_mut()
            .fail_next_cpu_sets_relinquish = true;
        let failed_retry = coordinator.reconcile_pending(true, true);
        assert!(failed_retry.applications.is_empty());
        assert!(failed_retry.failures.is_empty());
        assert!(failed_retry.releases.failures.is_empty());
        assert!(coordinator.has_pending_reconciliation());

        let retry = coordinator.reconcile_pending(true, true);
        assert!(retry.applications.is_empty());
        assert!(retry.failures.is_empty());
        assert!(retry.releases.failures.is_empty());
        assert_eq!(process_state(&coordinator).cpu_sets, vec![103]);
        assert!(!coordinator.has_managed_state());
        assert!(!coordinator.has_pending_reconciliation());
    }

    #[test]
    fn apply_policy_claim_forwards_cross_session_setting() {
        let mut coordinator = coordinator();
        coordinator
            .platform
            .state
            .borrow_mut()
            .reject_disallowed_cross_session_open = true;

        let result = coordinator.apply_policy_claim(
            claim(
                ControlOwner::ProcessorAffinityHard,
                CpuAllocationRequest::HardAffinity {
                    logical_processor_mask: 0b0100,
                },
                7,
            ),
            false,
        );
        assert!(matches!(result, Err(ProcessControlError::AccessDenied(_))));
        assert_eq!(process_state(&coordinator).affinity, 0b1111);
        assert!(coordinator.managed_affinity.is_empty());
        assert_eq!(
            coordinator.platform.state.borrow().open_cross_session_flags,
            vec![false]
        );
    }

    #[test]
    fn failed_pending_handoff_restores_outgoing_state_without_retry_spam() {
        let mut coordinator = coordinator();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::AdaptiveEngine,
                    CpuAllocationRequest::HardAffinity {
                        logical_processor_mask: 0b0011,
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::CpuSetsSoft,
                    CpuAllocationRequest::SoftCpuSets {
                        logical_processor_mask: 0b0010,
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        coordinator
            .platform
            .state
            .borrow_mut()
            .reject_disallowed_cross_session_open = true;

        coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
        let reconciliation = coordinator.reconcile_pending(false, true);
        assert_eq!(reconciliation.failures.len(), 1);
        assert_eq!(
            reconciliation.failures[0].owner,
            ControlOwner::AdaptiveEngine
        );
        assert_eq!(
            reconciliation.releases.restored_owners,
            vec![ControlOwner::CpuSetsSoft]
        );
        let state = process_state(&coordinator);
        assert!(state.cpu_sets.is_empty());
        assert_eq!(state.affinity, 0b1111);
        assert!(!coordinator.has_managed_state());
        assert!(!coordinator.has_pending_reconciliation());
        let open_count = coordinator
            .platform
            .state
            .borrow()
            .open_cross_session_flags
            .len();
        assert_eq!(
            &coordinator.platform.state.borrow().open_cross_session_flags[open_count - 2..],
            &[false, true]
        );

        let empty = coordinator.reconcile_pending(false, true);
        assert!(empty.applications.is_empty());
        assert!(empty.failures.is_empty());
        assert_eq!(
            coordinator
                .platform
                .state
                .borrow()
                .open_cross_session_flags
                .len(),
            open_count
        );
    }

    #[test]
    fn pid_reuse_relinquishes_the_old_identity_before_applying_the_new_process() {
        let mut coordinator = coordinator();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::ProcessorAffinityHard,
                    CpuAllocationRequest::HardAffinity {
                        logical_processor_mask: 0b0011,
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        coordinator.platform.state.borrow_mut().processes.insert(
            42,
            FakeProcessState {
                identity: identity(42, 8),
                affinity: 0b1110,
                system_affinity: 0b1111,
                cpu_sets: Vec::new(),
            },
        );

        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::ProcessorAffinityHard,
                    CpuAllocationRequest::HardAffinity {
                        logical_processor_mask: 0b0100,
                    },
                    8,
                ),
                true,
            )
            .unwrap();
        assert_eq!(process_state(&coordinator).affinity, 0b0100);
        assert_eq!(coordinator.managed_affinity.len(), 1);
        assert_eq!(
            coordinator
                .managed_affinity
                .keys()
                .next()
                .unwrap()
                .creation_time,
            8
        );
        assert!(coordinator
            .platform
            .state
            .borrow()
            .events
            .iter()
            .any(|event| event == "forget-affinity:7"));
    }

    #[test]
    fn commit_failure_compensates_and_relinquishes_the_journal() {
        let mut coordinator = coordinator();
        coordinator.platform.state.borrow_mut().fail_next_commit = true;
        let result = coordinator.apply_policy_claim(
            claim(
                ControlOwner::ProcessorAffinityHard,
                CpuAllocationRequest::HardAffinity {
                    logical_processor_mask: 0b0011,
                },
                7,
            ),
            true,
        );
        assert!(result.is_err());
        assert_eq!(process_state(&coordinator).affinity, 0b1111);
        assert!(coordinator.managed_affinity.is_empty());
        assert!(coordinator
            .platform
            .state
            .borrow()
            .events
            .iter()
            .any(|event| event == "forget-affinity:7"));
    }

    #[test]
    fn release_commit_failure_keeps_the_cpu_sets_baseline_and_relinquishes() {
        let mut coordinator = coordinator();
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::CpuSetsSoft,
                    CpuAllocationRequest::SoftCpuSets {
                        logical_processor_mask: 0b0010,
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        coordinator.platform.state.borrow_mut().fail_next_commit = true;

        let summary = coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
        assert!(summary.failures.is_empty());
        assert_eq!(summary.restored_owners, vec![ControlOwner::CpuSetsSoft]);
        assert!(process_state(&coordinator).cpu_sets.is_empty());
        assert!(coordinator.managed_cpu_sets.is_empty());
        assert!(coordinator
            .platform
            .state
            .borrow()
            .events
            .iter()
            .any(|event| event == "forget-cpu-sets:7"));
    }

    #[test]
    fn shutdown_restores_properties_in_reverse_apply_order() {
        let mut coordinator = coordinator();
        coordinator.platform.state.borrow_mut().processes.insert(
            43,
            FakeProcessState {
                identity: identity(43, 9),
                affinity: 0b1111,
                system_affinity: 0b1111,
                cpu_sets: Vec::new(),
            },
        );
        coordinator
            .apply_policy_claim(
                claim(
                    ControlOwner::ProcessorAffinityHard,
                    CpuAllocationRequest::HardAffinity {
                        logical_processor_mask: 0b0011,
                    },
                    7,
                ),
                true,
            )
            .unwrap();
        coordinator
            .apply_policy_claim(
                claim_for(
                    43,
                    ControlOwner::CpuSetsSoft,
                    CpuAllocationRequest::SoftCpuSets {
                        logical_processor_mask: 0b0010,
                    },
                    9,
                ),
                true,
            )
            .unwrap();
        coordinator.platform.state.borrow_mut().events.clear();

        coordinator.shutdown().unwrap();
        let state = coordinator.platform.state.borrow();
        let cpu_sets_release = state
            .events
            .iter()
            .position(|event| event.starts_with("begin-cpu-sets:"))
            .unwrap();
        let affinity_release = state
            .events
            .iter()
            .position(|event| event.starts_with("begin-affinity:"))
            .unwrap();
        assert!(cpu_sets_release < affinity_release);
        assert!(state.processes[&43].cpu_sets.is_empty());
        assert_eq!(state.processes[&42].affinity, 0b1111);
    }

    #[test]
    fn mask_helpers_preserve_group_zero_and_core_limiter_semantics() {
        assert_eq!(target_affinity_mask(0b1110, 0b0110), Some(0b0110));
        assert_eq!(target_affinity_mask(0b1000, 0b0111), None);
        assert_eq!(limited_affinity_mask(0b1111, 0b1111, 2), Some(0b0011));
        assert_eq!(limited_affinity_mask(0b1010, 0b1111, 1), Some(0b0010));
        assert_eq!(limited_affinity_mask(0b0011, 0b1111, 2), None);
        assert!(Path::new(r"C:\Apps\worker.exe").is_absolute());
    }

    #[test]
    #[ignore = "modifies and restores a disposable Windows process; run in explicit integration QA"]
    fn cpu_allocation_live_apply_and_clean_release() -> Result<(), String> {
        struct DisposableProcess(Child);

        impl Drop for DisposableProcess {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let mut executable_path = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .ok_or_else(|| "SystemRoot is unavailable.".to_owned())?;
        executable_path.push(r"System32\PING.EXE");
        let child = Command::new(&executable_path)
            .args(["-t", "127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not start disposable process: {error}"))?;
        let child = DisposableProcess(child);
        let deadline = Instant::now() + Duration::from_secs(2);
        let action_target = loop {
            match crate::foreground::capture_process_action_target(
                child.0.id(),
                &executable_path,
                true,
            ) {
                Ok(target) => break target,
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => return Err(error.to_string()),
            }
        };
        let target = ProcessControlTarget::from_action_target(&action_target);
        let mut coordinator = CpuAllocationCoordinator::default();

        let (_, process) = coordinator
            .platform
            .open(&target, true)
            .map_err(|error| error.to_string())?;
        let (baseline_affinity, system_affinity) = coordinator
            .platform
            .query_affinity(&process)
            .map_err(|error| error.to_string())?;
        let available_affinity = baseline_affinity & system_affinity;
        if available_affinity.count_ones() > 1 {
            let expected_affinity = available_affinity & available_affinity.wrapping_neg();
            coordinator
                .apply_policy_claim(
                    CpuAllocationClaim {
                        target: target.clone(),
                        owner: ControlOwner::ProcessorAffinityHard,
                        request: CpuAllocationRequest::HardAffinity {
                            logical_processor_mask: expected_affinity as u64,
                        },
                    },
                    true,
                )
                .map_err(|error| error.to_string())?;
            let (_, process) = coordinator
                .platform
                .open(&target, true)
                .map_err(|error| error.to_string())?;
            assert_eq!(
                coordinator
                    .platform
                    .query_affinity(&process)
                    .map_err(|error| error.to_string())?
                    .0,
                expected_affinity
            );
            let summary = coordinator.release_all_policy(ControlOwner::ProcessorAffinityHard);
            if let Some(failure) = summary.failures.first() {
                return Err(failure.error.to_string());
            }
            let (_, process) = coordinator
                .platform
                .open(&target, true)
                .map_err(|error| error.to_string())?;
            assert_eq!(
                coordinator
                    .platform
                    .query_affinity(&process)
                    .map_err(|error| error.to_string())?
                    .0,
                baseline_affinity
            );
        }

        let (_, process) = coordinator
            .platform
            .open(&target, true)
            .map_err(|error| error.to_string())?;
        let mut baseline_cpu_sets = coordinator
            .platform
            .query_cpu_sets(&process)
            .map_err(|error| error.to_string())?;
        normalize_cpu_set_ids(&mut baseline_cpu_sets);
        let soft_target = (0..64).find_map(|bit| {
            let mut ids = coordinator
                .platform
                .cpu_set_ids_for_mask(1_u64 << bit)
                .ok()?;
            normalize_cpu_set_ids(&mut ids);
            (!ids.is_empty() && ids != baseline_cpu_sets).then_some((1_u64 << bit, ids))
        });
        if let Some((logical_processor_mask, expected_cpu_sets)) = soft_target {
            coordinator
                .apply_policy_claim(
                    CpuAllocationClaim {
                        target: target.clone(),
                        owner: ControlOwner::CpuSetsSoft,
                        request: CpuAllocationRequest::SoftCpuSets {
                            logical_processor_mask,
                        },
                    },
                    true,
                )
                .map_err(|error| error.to_string())?;
            let (_, process) = coordinator
                .platform
                .open(&target, true)
                .map_err(|error| error.to_string())?;
            let mut observed = coordinator
                .platform
                .query_cpu_sets(&process)
                .map_err(|error| error.to_string())?;
            normalize_cpu_set_ids(&mut observed);
            assert_eq!(observed, expected_cpu_sets);
            let summary = coordinator.release_all_policy(ControlOwner::CpuSetsSoft);
            if let Some(failure) = summary.failures.first() {
                return Err(failure.error.to_string());
            }
            let (_, process) = coordinator
                .platform
                .open(&target, true)
                .map_err(|error| error.to_string())?;
            let mut observed = coordinator
                .platform
                .query_cpu_sets(&process)
                .map_err(|error| error.to_string())?;
            normalize_cpu_set_ids(&mut observed);
            assert_eq!(observed, baseline_cpu_sets);
        }
        Ok(())
    }
}
