use crate::foreground::ProcessActionTarget;

use super::{
    ControlOwner, CpuAllocationCoordinator, CpuAllocationPlatform, CpuAllocationRequest,
    PendingReconciliation, ProcessControlTarget,
};

impl<P: CpuAllocationPlatform> CpuAllocationCoordinator<P> {
    pub(crate) fn restore_adaptive_affinity_before_restart(
        &mut self,
        target: &ProcessActionTarget,
        allow_cross_session: bool,
    ) -> Result<bool, String> {
        let target = ProcessControlTarget::from_action_target(target);
        let key = target.key();
        let owner = ControlOwner::AdaptiveEngine;
        let claim = self.effective_claim(&key);
        if claim.is_some_and(|claim| claim.owner != owner) {
            return Ok(false);
        }
        let Some((managed_identity, managed)) =
            self.managed_affinity.iter().find(|(identity, managed)| {
                managed.owner == owner && identity.matches_target_key(&key)
            })
        else {
            return Ok(false);
        };
        if !managed.confirmed || self.pending_reconciliations.contains_key(&key) {
            return Err(
                "Adaptive Engine processor placement is awaiting verification or restoration."
                    .to_owned(),
            );
        }
        if managed.expected == managed.baseline
            || claim.is_none_or(|claim| {
                matches!(&claim.request, CpuAllocationRequest::SoftCpuSets { .. })
            })
        {
            return Ok(false);
        }

        let (identity, process) = self
            .platform
            .open(&target, allow_cross_session)
            .map_err(|error| error.to_string())?;
        if &identity != managed_identity {
            return Err(
                "The Adaptive Engine restart parent changed before restoration.".to_owned(),
            );
        }
        self.platform
            .validate_group_zero_affinity(&process)
            .map_err(|error| error.to_string())?;

        let result = self.release_affinity_with_process(&identity, &process);
        // A restart must not replay the withdrawn claim before the replacement inherits.
        if let Some(claims) = self.claims.get_mut(&owner) {
            claims.remove(&key);
        }
        match result {
            Ok(restored) => {
                self.managed_affinity.remove(&identity);
                self.adaptive_episode_keys.remove(&key);
                Ok(restored)
            }
            Err(error) => {
                self.pending_reconciliations.insert(
                    key,
                    PendingReconciliation::ReleaseOnlyRetry { failed_claim: None },
                );
                Err(error.to_string())
            }
        }
    }
}
