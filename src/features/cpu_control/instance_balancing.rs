use std::collections::{BTreeMap, BTreeSet};

use crate::{
    control::process::ProcessControlTarget,
    cpu_topology::{CpuSelection, CpuTopology, ProcessorId, SelectionError},
};

mod policy;
pub(crate) use policy::{Cohort, Policy};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Assignment {
    pub(crate) target: ProcessControlTarget,
    pub(crate) selection: CpuSelection,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PlanError {
    #[error("Instance Balancing is waiting for at least two eligible instances.")]
    WaitingForInstances,
    #[error("Instance Balancing requires one active processor group (group 0).")]
    UnsupportedTopology,
    #[error("The CPU topology changed. Rearm Instance Balancing after reviewing the new layout.")]
    TopologyChanged,
    #[error("Instance Balancing received processors outside the current topology.")]
    InvalidPool,
    #[error("Instance Balancing requires unique exact processes and process IDs.")]
    DuplicateMember,
    #[error("Instance Balancing has fewer eligible whole cores than instances.")]
    InsufficientCapacity,
    #[error("Instance Balancing could not construct a complete nonempty plan.")]
    InconsistentPlan,
    #[error(transparent)]
    Selection(#[from] SelectionError),
}

pub(crate) fn plan(
    topology: &CpuTopology,
    allowed: &BTreeSet<ProcessorId>,
    members: &[ProcessControlTarget],
    previous: &[Assignment],
) -> Result<Vec<Assignment>, PlanError> {
    let mut remaining = BTreeMap::new();
    let mut pids = BTreeSet::new();
    for target in members {
        if remaining.insert(target.key(), target).is_some() || !pids.insert(target.id) {
            return Err(PlanError::DuplicateMember);
        }
    }
    if members.len() < 2 {
        return Err(PlanError::WaitingForInstances);
    }
    if !matches!(topology.groups(), [group] if group.number == 0) {
        return Err(PlanError::UnsupportedTopology);
    }
    if allowed.iter().any(|id| {
        topology
            .processors()
            .binary_search_by_key(id, |processor| processor.id)
            .is_err()
    }) {
        return Err(PlanError::InvalidPool);
    }

    let mut ordered = Vec::with_capacity(members.len());
    for prior in previous {
        if let Some(target) = remaining.remove(&prior.target.key()) {
            ordered.push(target);
        }
    }
    let mut added = remaining.into_values().collect::<Vec<_>>();
    added.sort_by_key(|target| (target.creation_time, target.id, target.key()));
    ordered.extend(added);

    let mut cores = BTreeMap::<ProcessorId, (u8, Vec<ProcessorId>)>::new();
    for processor in topology.processors() {
        cores
            .entry(processor.core)
            .or_insert_with(|| (processor.efficiency_class, Vec::new()))
            .1
            .push(processor.id);
    }
    cores.retain(|_, (_, processors)| processors.iter().all(|id| allowed.contains(id)));
    if cores.len() < ordered.len() {
        return Err(PlanError::InsufficientCapacity);
    }
    let mut classes = BTreeMap::<u8, BTreeSet<ProcessorId>>::new();
    for (id, (class, _)) in &cores {
        classes.entry(*class).or_default().insert(*id);
    }

    let mut shares = vec![BTreeSet::new(); ordered.len()];
    let mut offset = 0;
    for ids in classes.values() {
        let quotas = (0..ordered.len())
            .map(|index| {
                ids.len() / ordered.len()
                    + usize::from(
                        (index + ordered.len() - offset) % ordered.len()
                            < ids.len() % ordered.len(),
                    )
            })
            .collect::<Vec<_>>();
        let mut free = ids.clone();
        for ((target, share), quota) in ordered.iter().zip(&mut shares).zip(&quotas) {
            let hint = previous
                .iter()
                .find(|prior| prior.target.key() == target.key())
                .and_then(|prior| prior.selection.resolve_processors(topology).ok());
            let Some(hint) = hint else { continue };
            for id in ids
                .iter()
                .filter(|id| {
                    cores[*id]
                        .1
                        .iter()
                        .all(|processor| hint.contains(processor))
                })
                .take(*quota)
            {
                if free.remove(id) {
                    share.insert(*id);
                }
            }
        }
        for (share, quota) in shares.iter_mut().zip(quotas) {
            for _ in share.intersection(ids).count()..quota {
                share.insert(free.pop_first().ok_or(PlanError::InconsistentPlan)?);
            }
        }
        offset = (offset + ids.len()) % ordered.len();
    }

    ordered
        .into_iter()
        .zip(shares)
        .map(|(target, share)| {
            let processors = share
                .iter()
                .flat_map(|id| cores[id].1.iter().copied())
                .collect::<Vec<_>>();
            if processors.is_empty() {
                return Err(PlanError::InconsistentPlan);
            }
            Ok(Assignment {
                target: target.clone(),
                selection: CpuSelection::bind(topology, processors)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
