use std::{
    collections::BTreeMap,
    mem::take,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::foreground::{process_executable_path, process_list::list_process_entries, ProcessInfo};

use super::{
    ControlOwner, CpuAllocationCoordinator, CpuAllocationPlatform, CpuAllocationRecoveryIntent,
    CpuAllocationReleaseFailure, CpuAllocationReleaseSummary, ManagedValue, PendingReconciliation,
    ProcessControlError, ProcessControlTarget, ProcessIdentity,
};

const ADAPTIVE_ENGINE: ControlOwner = ControlOwner::AdaptiveEngine;

#[derive(Default)]
pub(super) struct ChildAffinityRestoration {
    enabled: bool,
    parents: BTreeMap<ProcessIdentity, ParentAffinityProof>,
}

struct ParentAffinityProof {
    apply_sequence: u64,
    considered_through: u64,
}

enum ChildAffinityOutcome {
    Restored,
    Preserved,
    ParentChanged,
}

impl ChildAffinityRestoration {
    pub(super) fn invalidate(&mut self, parent: &ProcessIdentity) {
        if self.enabled {
            self.parents.remove(parent);
        }
    }

    pub(super) fn record_application(
        &mut self,
        parent: &ProcessIdentity,
        managed: &ManagedValue<usize>,
    ) {
        if !self.enabled {
            return;
        }
        let start = (managed.owner == ADAPTIVE_ENGINE
            && managed.confirmed
            && parent.session_id.is_some()
            && managed.expected != managed.baseline
            && managed.expected != 0
            && managed.expected & managed.baseline == managed.expected)
            .then(windows_filetime_now)
            .flatten();
        if let Some(considered_through) = start {
            self.parents.insert(
                parent.clone(),
                ParentAffinityProof {
                    apply_sequence: managed.apply_sequence,
                    considered_through,
                },
            );
        } else {
            self.invalidate(parent);
        }
    }

    pub(super) fn record_unchanged(
        &mut self,
        parent: &ProcessIdentity,
        managed: &ManagedValue<usize>,
    ) {
        if self.enabled
            && self
                .parents
                .get(parent)
                .is_none_or(|proof| proof.apply_sequence != managed.apply_sequence)
        {
            self.record_application(parent, managed);
        }
    }
}

pub(super) fn windows_filetime_now() -> Option<u64> {
    const UNIX_EPOCH_FILETIME: u128 = 116_444_736_000_000_000;
    let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    u64::try_from(UNIX_EPOCH_FILETIME + since_epoch.as_nanos() / 100).ok()
}

impl CpuAllocationCoordinator {
    pub(crate) fn restore_adaptive_child_affinity(
        &mut self,
        processes: &[ProcessInfo],
        is_child_eligible: impl FnMut(&ProcessInfo, &Path) -> bool,
        allow_cross_session: bool,
    ) -> CpuAllocationReleaseSummary {
        self.restore_adaptive_child_affinity_with(
            processes,
            is_child_eligible,
            allow_cross_session,
            |parent, child, _retained_child| {
                // Metadata enrichment can race PID reuse. Recheck the ancestry with the exact
                // child and parent handles still held so neither PID can be reassigned.
                list_process_entries()
                    .map(|entries| {
                        entries
                            .iter()
                            .any(|entry| entry.id == child && entry.parent_id == Some(parent))
                    })
                    .map_err(ProcessControlError::Unavailable)
            },
        )
    }
}

impl<P: CpuAllocationPlatform> CpuAllocationCoordinator<P> {
    pub(crate) fn set_restore_child_affinity(&mut self, enabled: bool) {
        self.child_affinity.enabled = enabled;
        if !enabled {
            self.child_affinity.parents.clear();
        }
    }

    pub(crate) fn has_adaptive_child_affinity_candidates(&self) -> bool {
        self.child_affinity
            .parents
            .iter()
            .any(|(parent, proof)| self.active_parent_affinity(parent, proof).is_some())
    }

    fn active_parent_affinity(
        &self,
        parent: &ProcessIdentity,
        proof: &ParentAffinityProof,
    ) -> Option<(usize, usize)> {
        let managed = self.managed_affinity.get(parent)?;
        if managed.apply_sequence != proof.apply_sequence
            || !managed.confirmed
            || managed.owner != ADAPTIVE_ENGINE
        {
            return None;
        }
        let key = parent.key();
        if self.pending_reconciliations.contains_key(&key)
            || self.effective_claim(&key)?.owner != ADAPTIVE_ENGINE
        {
            return None;
        }
        Some((managed.baseline, managed.expected))
    }

    pub(super) fn restore_adaptive_child_affinity_with(
        &mut self,
        processes: &[ProcessInfo],
        mut is_child_eligible: impl FnMut(&ProcessInfo, &Path) -> bool,
        allow_cross_session: bool,
        mut is_direct_child: impl FnMut(u32, u32, &P::Process) -> Result<bool, ProcessControlError>,
    ) -> CpuAllocationReleaseSummary {
        let mut summary = CpuAllocationReleaseSummary::default();
        for (parent, mut proof) in take(&mut self.child_affinity.parents) {
            let Some(affinity) = self.active_parent_affinity(&parent, &proof) else {
                continue;
            };
            let considered_before = proof.considered_through.max(parent.creation_time);
            let mut parent_handle = None;
            let mut proof_valid = true;
            for child in processes.iter().filter(|child| {
                child.id != parent.id
                    && child.parent_id == Some(parent.id)
                    && child.session_id == parent.session_id
                    && child
                        .creation_time
                        .is_some_and(|created| created > considered_before)
            }) {
                let Some(created) = child.creation_time else {
                    continue;
                };
                // Each exact child gets one eligibility check, so later affinity drift cannot
                // become evidence of inheritance. The high-water mark also bounds state.
                proof.considered_through = proof.considered_through.max(created);
                let Some(path) = process_executable_path(child) else {
                    continue;
                };
                if !is_child_eligible(child, &path) {
                    continue;
                }
                if parent_handle.is_none() {
                    parent_handle =
                        self.open_verified_affinity_parent(&parent, allow_cross_session);
                }
                let Some(parent_handle) = parent_handle.as_ref() else {
                    proof_valid = false;
                    break;
                };
                let target =
                    ProcessControlTarget::automatic(child.id, child.name.clone(), path, created);
                match self.restore_inherited_affinity(
                    &target,
                    (&parent, parent_handle),
                    affinity,
                    allow_cross_session,
                    &mut is_direct_child,
                ) {
                    Ok(ChildAffinityOutcome::Restored) => {
                        summary.restored_owners.push(ADAPTIVE_ENGINE)
                    }
                    Ok(ChildAffinityOutcome::Preserved)
                    | Err(ProcessControlError::ProcessExited) => {}
                    Ok(ChildAffinityOutcome::ParentChanged) => {
                        proof_valid = false;
                        break;
                    }
                    Err(error) => summary.failures.push(CpuAllocationReleaseFailure {
                        owner: ADAPTIVE_ENGINE,
                        process_id: child.id,
                        process_name: child.name.clone(),
                        property: "inherited child affinity",
                        error,
                    }),
                }
            }
            if proof_valid {
                self.child_affinity.parents.insert(parent, proof);
            }
        }
        summary
    }

    fn open_verified_affinity_parent(
        &mut self,
        parent: &ProcessIdentity,
        allow_cross_session: bool,
    ) -> Option<P::Process> {
        let (identity, handle) = self
            .platform
            .open(&parent.target(), allow_cross_session)
            .ok()?;
        if &identity != parent || identity.session_id != parent.session_id {
            return None;
        }
        Some(handle)
    }

    fn restore_inherited_affinity(
        &mut self,
        target: &ProcessControlTarget,
        (parent, parent_handle): (&ProcessIdentity, &P::Process),
        (baseline, narrowed): (usize, usize),
        allow_cross_session: bool,
        is_direct_child: &mut impl FnMut(u32, u32, &P::Process) -> Result<bool, ProcessControlError>,
    ) -> Result<ChildAffinityOutcome, ProcessControlError> {
        let key = target.key();
        if self.effective_claim(&key).is_some() || self.pending_reconciliations.contains_key(&key) {
            return Ok(ChildAffinityOutcome::Preserved);
        }
        let (identity, handle) = self.platform.open(target, allow_cross_session)?;
        if identity.session_id != parent.session_id
            || self.managed_affinity.contains_key(&identity)
            || self.managed_cpu_sets.contains_key(&identity)
        {
            return Ok(ChildAffinityOutcome::Preserved);
        }
        let (current, system) = self.platform.query_affinity(&handle)?;
        if current != narrowed
            || baseline & system != baseline
            || !is_direct_child(parent.id, identity.id, &handle)?
        {
            return Ok(ChildAffinityOutcome::Preserved);
        }
        self.platform.validate_group_zero_affinity(&handle)?;
        if !self.platform.query_cpu_sets(&handle)?.is_empty() {
            return Ok(ChildAffinityOutcome::Preserved);
        }
        if self
            .platform
            .validate_group_zero_affinity(parent_handle)
            .is_err()
            || !self
                .platform
                .query_affinity(parent_handle)
                .is_ok_and(|(current, _)| current == narrowed)
        {
            return Ok(ChildAffinityOutcome::ParentChanged);
        }

        // Adopt the already-inherited effect as an edge from the parent's baseline. Recovery
        // must keep the restored mask, even if Winderust exits during the next transaction.
        let inherited = self
            .platform
            .begin_affinity_change(&handle, baseline, narrowed)?;
        let apply_sequence = self.next_sequence();
        let committed = inherited.commit().map_err(ProcessControlError::Failed);
        self.managed_affinity.insert(
            identity.clone(),
            ManagedValue {
                baseline,
                expected: narrowed,
                owner: ADAPTIVE_ENGINE,
                apply_sequence,
                confirmed: committed.is_ok(),
            },
        );
        let restored =
            committed.and_then(|()| self.release_affinity_with_process(&identity, &handle));
        match restored {
            Ok(restored) => {
                self.managed_affinity.remove(&identity);
                Ok(if restored {
                    ChildAffinityOutcome::Restored
                } else {
                    ChildAffinityOutcome::Preserved
                })
            }
            Err(error) => {
                self.pending_reconciliations.insert(
                    key,
                    PendingReconciliation::ReleaseOnlyRetry { failed_claim: None },
                );
                Err(error)
            }
        }
    }
}
