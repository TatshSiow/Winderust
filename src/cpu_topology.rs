use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ProcessorId {
    pub group: u16,
    pub number: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LogicalProcessor {
    pub id: ProcessorId,
    pub core: ProcessorId,
    pub efficiency_class: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ProcessorGroup {
    pub number: u16,
    pub active_mask: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CacheKind {
    Unified,
    Instruction,
    Data,
    Trace,
    Unknown(i32),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CacheDomain {
    pub level: u8,
    pub kind: CacheKind,
    pub size_bytes: u32,
    pub processors: Vec<ProcessorId>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct NumaNode {
    pub number: u32,
    pub processors: Vec<ProcessorId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CpuTopology {
    processors: Vec<LogicalProcessor>,
    groups: Vec<ProcessorGroup>,
    caches: Vec<CacheDomain>,
    numa_nodes: Vec<NumaNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum TopologyError {
    #[error("Processor groups are empty, duplicated, or inconsistent.")]
    Groups,
    #[error("Logical processors are empty, duplicated, or inconsistent with active groups.")]
    Processors,
    #[error("Physical core membership or efficiency classes are inconsistent.")]
    Cores,
    #[error("Cache-domain membership is empty, duplicated, or unknown.")]
    Caches,
    #[error("NUMA-node membership is empty, duplicated, or unknown.")]
    NumaNodes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum SelectionError {
    #[error("Select at least one logical processor.")]
    Empty,
    #[error("A logical processor is selected more than once.")]
    Duplicate,
    #[error("A selected logical processor is no longer present.")]
    Unknown,
    #[error("This control supports processor group 0 only.")]
    UnsupportedGroup,
}

impl CpuTopology {
    pub(crate) fn new(
        mut processors: Vec<LogicalProcessor>,
        mut groups: Vec<ProcessorGroup>,
        mut caches: Vec<CacheDomain>,
        mut numa_nodes: Vec<NumaNode>,
    ) -> Result<Self, TopologyError> {
        groups.sort_unstable();
        if groups.is_empty()
            || groups.iter().any(|group| group.active_mask == 0)
            || groups
                .windows(2)
                .any(|pair| pair[0].number == pair[1].number)
        {
            return Err(TopologyError::Groups);
        }
        processors.sort_unstable();
        let ids = processors
            .iter()
            .map(|processor| processor.id)
            .collect::<BTreeSet<_>>();
        if processors.is_empty() || ids.len() != processors.len() {
            return Err(TopologyError::Processors);
        }
        let mut observed_groups = BTreeMap::<u16, u64>::new();
        let mut cores = BTreeMap::<ProcessorId, (ProcessorId, u8)>::new();
        for processor in &processors {
            if processor.id.number >= 64 {
                return Err(TopologyError::Processors);
            }
            *observed_groups.entry(processor.id.group).or_default() |= 1_u64 << processor.id.number;
            let core = cores
                .entry(processor.core)
                .or_insert((processor.id, processor.efficiency_class));
            if processor.core.group != processor.id.group || core.1 != processor.efficiency_class {
                return Err(TopologyError::Cores);
            }
        }
        if observed_groups.len() != groups.len()
            || groups
                .iter()
                .any(|group| observed_groups.get(&group.number) != Some(&group.active_mask))
        {
            return Err(TopologyError::Processors);
        }
        if cores.iter().any(|(core, (first, _))| core != first) {
            return Err(TopologyError::Cores);
        }

        let mut cache_memberships = BTreeSet::new();
        for cache in &mut caches {
            validate_members(&mut cache.processors, &ids).map_err(|()| TopologyError::Caches)?;
            if cache.level == 0
                || cache
                    .processors
                    .iter()
                    .any(|id| !cache_memberships.insert((cache.level, cache.kind, *id)))
            {
                return Err(TopologyError::Caches);
            }
        }
        caches.sort_unstable();
        let mut numa_memberships = BTreeSet::new();
        for node in &mut numa_nodes {
            validate_members(&mut node.processors, &ids).map_err(|()| TopologyError::NumaNodes)?;
            if node
                .processors
                .iter()
                .any(|id| !numa_memberships.insert(*id))
            {
                return Err(TopologyError::NumaNodes);
            }
        }
        numa_nodes.sort_unstable();
        if numa_nodes
            .windows(2)
            .any(|pair| pair[0].number == pair[1].number)
        {
            return Err(TopologyError::NumaNodes);
        }
        Ok(Self {
            processors,
            groups,
            caches,
            numa_nodes,
        })
    }

    pub(crate) fn processors(&self) -> &[LogicalProcessor] {
        &self.processors
    }

    pub(crate) fn groups(&self) -> &[ProcessorGroup] {
        &self.groups
    }

    pub(crate) fn caches(&self) -> &[CacheDomain] {
        &self.caches
    }

    pub(crate) fn numa_nodes(&self) -> &[NumaNode] {
        &self.numa_nodes
    }

    pub(crate) fn group_zero_processors(&self) -> impl Iterator<Item = &LogicalProcessor> {
        self.processors
            .iter()
            .filter(|processor| processor.id.group == 0)
    }

    pub(crate) fn group_zero_mask(&self, selection: &[ProcessorId]) -> Result<u64, SelectionError> {
        if selection.is_empty() {
            return Err(SelectionError::Empty);
        }
        let mut seen = BTreeSet::new();
        let mut mask = 0;
        for id in selection {
            if !seen.insert(*id) {
                return Err(SelectionError::Duplicate);
            }
            if self
                .processors
                .binary_search_by_key(id, |processor| processor.id)
                .is_err()
            {
                return Err(SelectionError::Unknown);
            }
            if id.group != 0 {
                return Err(SelectionError::UnsupportedGroup);
            }
            mask |= 1_u64 << id.number;
        }
        Ok(mask)
    }
}

fn validate_members(members: &mut [ProcessorId], known: &BTreeSet<ProcessorId>) -> Result<(), ()> {
    members.sort_unstable();
    if members.is_empty()
        || members.windows(2).any(|pair| pair[0] == pair[1])
        || members.iter().any(|id| !known.contains(id))
    {
        Err(())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
