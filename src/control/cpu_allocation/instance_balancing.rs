use super::*;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum InstanceBalancingEligibilityError {
    #[error("Instance Balancing requires its CPU Set owner and a rule generation.")]
    IncompatibleRequest,
    #[error("Instance Balancing preserves an existing external CPU Set assignment.")]
    ExternalAssignment,
    #[error("Instance Balancing stopped after an external CPU allocation change.")]
    ExternalChange,
    #[error("Instance Balancing requires confirmed CPU allocation recovery state.")]
    UncertainState,
    #[error(
        "Instance Balancing remains suppressed until this process exits or the rule is rearmed."
    )]
    Suppressed,
    #[error("Instance Balancing rejected an older rule generation.")]
    StaleGeneration,
    #[error("{reason} Restoration remains pending: {detail}")]
    RestorationPending { reason: Box<Self>, detail: String },
    #[error("{0}")]
    Selection(#[from] InstanceBalancingSelectionError),
}

pub(super) struct GuardState {
    generation: u64,
    prepared_generation: Option<u64>,
    affinity: Option<usize>,
    suppressed: bool,
}

pub(super) struct Expectation {
    pub(super) affinity: usize,
    pub(super) cpu_sets: Vec<u32>,
}

pub(super) fn validate_claim(
    claim: &CpuAllocationClaim,
) -> Result<(), InstanceBalancingEligibilityError> {
    if (claim.owner == ControlOwner::InstanceBalancing
        && claim.balancing_generation.is_some()
        && matches!(claim.request, CpuAllocationRequest::SoftCpuSets { .. }))
        || (claim.owner != ControlOwner::InstanceBalancing && claim.balancing_generation.is_none())
    {
        Ok(())
    } else {
        Err(InstanceBalancingEligibilityError::IncompatibleRequest)
    }
}

impl<P: CpuAllocationPlatform> CpuAllocationCoordinator<P> {
    pub(crate) fn retire_exited_instance_balancing(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> bool {
        if !matches!(
            self.platform
                .open(target, allow_cross_session_process_control),
            Err(ProcessControlError::ProcessExited)
        ) {
            return false;
        }
        let key = target.key();
        self.instance_balancing
            .retain(|identity, _| identity.key() != key);
        true
    }

    pub(crate) fn instance_balancing_allowed_processors(
        &mut self,
        inventory: &mut CpuSetInventory,
        topology: &CpuTopology,
        target: &ProcessControlTarget,
        generation: u64,
        allow_cross_session_process_control: bool,
    ) -> Result<BTreeSet<ProcessorId>, CpuAllocationApplyError> {
        self.register_instance_balancing_generation(target, generation)
            .map_err(CpuAllocationApplyError::Eligibility)?;
        let (identity, process) = match self
            .platform
            .open(target, allow_cross_session_process_control)
        {
            Ok(opened) => opened,
            Err(ProcessControlError::ProcessExited) => {
                let key = target.key();
                self.instance_balancing
                    .retain(|identity, _| identity.key() != key);
                return Err(ProcessControlError::ProcessExited.into());
            }
            Err(error) => return Err(error.into()),
        };
        self.relinquish_stale_identities(&identity)?;
        let entries = inventory
            .entries(&mut self.platform)
            .map_err(CpuAllocationApplyError::Discovery)?;
        self.prepare_instance_balancing_with(&identity, &process, generation, |original, system| {
            eligibility::allowed_processors(topology, original, system, entries)
        })
        .map(|(_, allowed)| allowed)
    }

    pub(super) fn register_instance_balancing_generation(
        &mut self,
        target: &ProcessControlTarget,
        generation: u64,
    ) -> Result<(), InstanceBalancingEligibilityError> {
        // A staged generation is not mutation authority; native preflight still opens the exact target.
        let identity = ProcessIdentity::new(
            target.id,
            target.name.clone(),
            target.executable_path.clone(),
            target.creation_time,
            None,
        );
        let state = self
            .instance_balancing
            .entry(identity)
            .or_insert(GuardState {
                generation,
                prepared_generation: None,
                affinity: None,
                suppressed: false,
            });
        if generation < state.generation {
            return Err(InstanceBalancingEligibilityError::StaleGeneration);
        }
        state.generation = generation;
        Ok(())
    }

    pub(super) fn suppress_instance_balancing(&mut self, identity: &ProcessIdentity) {
        if let Some(state) = self.instance_balancing.get_mut(identity) {
            state.suppressed = true;
        }
    }

    pub(super) fn prepare_instance_balancing(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
        topology: &CpuTopology,
        selected: &[ProcessorId],
        generation: u64,
        inventory: &BTreeMap<ProcessorId, CpuSetRecord>,
    ) -> Result<Expectation, CpuAllocationApplyError> {
        self.prepare_instance_balancing_with(identity, process, generation, |original, system| {
            eligibility::validate_selection(topology, selected, original, system, inventory)
        })
        .map(|(expectation, ())| expectation)
    }

    fn prepare_instance_balancing_with<T>(
        &mut self,
        identity: &ProcessIdentity,
        process: &P::Process,
        generation: u64,
        validate: impl FnOnce(usize, usize) -> Result<T, InstanceBalancingSelectionError>,
    ) -> Result<(Expectation, T), CpuAllocationApplyError> {
        let state = self
            .instance_balancing
            .entry(identity.clone())
            .or_insert(GuardState {
                generation,
                prepared_generation: None,
                affinity: None,
                suppressed: false,
            });
        if generation < state.generation {
            return Err(CpuAllocationApplyError::Eligibility(
                InstanceBalancingEligibilityError::StaleGeneration,
            ));
        }
        let rearming = state.prepared_generation != Some(generation);
        if state.suppressed && !rearming {
            return Err(CpuAllocationApplyError::Eligibility(
                InstanceBalancingEligibilityError::Suppressed,
            ));
        }
        let affinity = self.managed_affinity.get(identity);
        let cpu_sets = self.managed_cpu_sets.get(identity);
        if affinity.is_some_and(|managed| !managed.confirmed)
            || cpu_sets.is_some_and(|managed| !managed.confirmed)
        {
            return Err(CpuAllocationApplyError::Eligibility(
                InstanceBalancingEligibilityError::UncertainState,
            ));
        }
        self.platform.validate_group_zero_affinity(process)?;
        let (current_affinity, system_affinity) = self.platform.query_affinity(process)?;
        let mut current_cpu_sets = self.platform.query_cpu_sets(process)?;
        normalize_cpu_set_ids(&mut current_cpu_sets);
        if rearming {
            state.generation = generation;
            state.prepared_generation = Some(generation);
            state.affinity = None;
            state.suppressed = false;
        }
        if affinity.is_some_and(|managed| managed.expected != current_affinity)
            || cpu_sets.is_some_and(|managed| managed.expected != current_cpu_sets)
        {
            state.suppressed = true;
            return Err(CpuAllocationApplyError::Eligibility(
                InstanceBalancingEligibilityError::ExternalChange,
            ));
        }
        let original_affinity = affinity.map_or(current_affinity, |managed| managed.baseline);
        if state
            .affinity
            .is_some_and(|expected| expected != original_affinity)
        {
            state.suppressed = true;
            return Err(CpuAllocationApplyError::Eligibility(
                InstanceBalancingEligibilityError::ExternalChange,
            ));
        }
        let original_cpu_sets = cpu_sets.map_or(&current_cpu_sets, |managed| &managed.baseline);
        if !original_cpu_sets.is_empty() {
            if state.affinity.is_some() {
                state.suppressed = true;
                return Err(CpuAllocationApplyError::Eligibility(
                    InstanceBalancingEligibilityError::ExternalChange,
                ));
            }
            return Err(CpuAllocationApplyError::Eligibility(
                InstanceBalancingEligibilityError::ExternalAssignment,
            ));
        }
        let validated = validate(original_affinity, system_affinity)
            .map_err(|error| CpuAllocationApplyError::Eligibility(error.into()))?;
        state.affinity = Some(original_affinity);
        Ok((
            Expectation {
                affinity: original_affinity,
                cpu_sets: current_cpu_sets,
            },
            validated,
        ))
    }
}
