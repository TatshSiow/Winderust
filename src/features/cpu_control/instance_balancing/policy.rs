use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use super::{plan, Assignment, PlanError};
use crate::{
    control::process::ProcessControlTarget,
    cpu_topology::{CpuTopology, ProcessorId, TopologySignature},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cohort {
    pub(crate) members: Vec<(ProcessControlTarget, BTreeSet<ProcessorId>)>,
}

impl Cohort {
    pub(crate) fn new(mut members: Vec<(ProcessControlTarget, BTreeSet<ProcessorId>)>) -> Self {
        members.sort_by_key(|(target, _)| target.key());
        Self { members }
    }
}

#[derive(Default)]
pub(crate) struct Policy {
    topology: Option<TopologySignature>,
    topology_changed: bool,
    candidate: Option<(Cohort, Instant)>,
    active: bool,
    previous: Vec<Assignment>,
    last_apply: Option<Instant>,
}

impl Policy {
    pub(crate) fn check_topology(&mut self, topology: &CpuTopology) -> Result<(), PlanError> {
        let signature = topology.signature();
        if self
            .topology
            .as_ref()
            .is_some_and(|previous| *previous != signature)
        {
            self.topology_changed = true;
        }
        if self.topology_changed {
            self.invalidate();
            return Err(PlanError::TopologyChanged);
        }
        self.topology = Some(signature);
        Ok(())
    }

    pub(crate) fn observe(
        &mut self,
        topology: &CpuTopology,
        cohort: &Cohort,
        now: Instant,
    ) -> Result<Option<Vec<Assignment>>, PlanError> {
        self.check_topology(topology)?;
        if self.active
            && self
                .candidate
                .as_ref()
                .is_some_and(|(accepted, _)| accepted == cohort)
        {
            // The accepted plan is stable for this exact topology and cohort.
            return Ok(Some(self.previous.clone()));
        }
        let mut allowed = cohort
            .members
            .first()
            .map_or_else(BTreeSet::new, |(_, allowed)| allowed.clone());
        for (_, member_allowed) in cohort.members.iter().skip(1) {
            allowed.retain(|processor| member_allowed.contains(processor));
        }
        let targets = cohort
            .members
            .iter()
            .map(|(target, _)| target.clone())
            .collect::<Vec<_>>();
        let desired = match plan(topology, &allowed, &targets, &self.previous) {
            Ok(plan) => plan,
            Err(error) => {
                self.invalidate();
                return Err(error);
            }
        };
        let since = match &self.candidate {
            Some((previous, since)) if previous == cohort => *since,
            _ => {
                self.candidate = Some((cohort.clone(), now));
                self.active = false;
                return Ok(None);
            }
        };
        if now.saturating_duration_since(since) < Duration::from_secs(1)
            || self.last_apply.is_some_and(|previous| {
                now.saturating_duration_since(previous) < Duration::from_secs(5)
            })
        {
            return Ok(None);
        }
        Ok(Some(desired))
    }

    pub(crate) fn accepted(&mut self, plan: Vec<Assignment>, now: Instant) {
        if !self.active {
            self.last_apply = Some(now);
        }
        self.previous = plan;
        self.active = true;
    }

    pub(crate) fn invalidate(&mut self) {
        self.candidate = None;
        self.active = false;
    }
}

#[cfg(test)]
mod tests;
