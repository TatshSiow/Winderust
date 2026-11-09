use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessorId {
    pub group: u16,
    pub number: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LogicalProcessor {
    pub id: ProcessorId,
    pub core: ProcessorId,
    pub efficiency_class: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProcessorGroup {
    pub number: u16,
    #[serde(with = "group_mask")]
    pub active_mask: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CacheKind {
    Unified,
    Instruction,
    Data,
    Trace,
    Unknown(i32),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CacheDomain {
    pub level: u8,
    pub kind: CacheKind,
    pub size_bytes: u32,
    pub processors: Vec<ProcessorId>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NumaNode {
    pub number: u32,
    pub processors: Vec<ProcessorId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TopologyWire", into = "TopologyWire")]
pub(crate) struct CpuTopology {
    processors: Vec<LogicalProcessor>,
    groups: Vec<ProcessorGroup>,
    caches: Vec<CacheDomain>,
    numa_nodes: Vec<NumaNode>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TopologyWire {
    processors: Vec<LogicalProcessor>,
    groups: Vec<ProcessorGroup>,
    caches: Vec<CacheDomain>,
    numa_nodes: Vec<NumaNode>,
}

impl TryFrom<TopologyWire> for CpuTopology {
    type Error = TopologyError;

    fn try_from(wire: TopologyWire) -> Result<Self, Self::Error> {
        Self::new(wire.processors, wire.groups, wire.caches, wire.numa_nodes)
    }
}

impl From<CpuTopology> for TopologyWire {
    fn from(topology: CpuTopology) -> Self {
        Self {
            processors: topology.processors,
            groups: topology.groups,
            caches: topology.caches,
            numa_nodes: topology.numa_nodes,
        }
    }
}

/// An exact canonical layout snapshot, rather than a hash or a machine identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TopologySignature(CpuTopology);

impl From<&CpuTopology> for TopologySignature {
    fn from(topology: &CpuTopology) -> Self {
        Self(topology.clone())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SelectionWire", into = "SelectionWire")]
pub struct CpuSelection {
    processors: Vec<ProcessorId>,
    topology: Option<TopologySignature>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionWire {
    #[serde(default)]
    processors: Vec<ProcessorId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    topology: Option<TopologySignature>,
}

impl TryFrom<SelectionWire> for CpuSelection {
    type Error = SelectionError;

    fn try_from(wire: SelectionWire) -> Result<Self, Self::Error> {
        Self::new(wire.processors, wire.topology)
    }
}

impl From<CpuSelection> for SelectionWire {
    fn from(selection: CpuSelection) -> Self {
        Self {
            processors: selection.processors,
            topology: selection.topology,
        }
    }
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
pub enum SelectionError {
    #[error("Select at least one logical processor.")]
    Empty,
    #[error("A logical processor is selected more than once.")]
    Duplicate,
    #[error("A logical processor number must be between 0 and 63.")]
    InvalidNumber,
    #[error("A selected logical processor is no longer present.")]
    Unknown,
    #[error("This control supports processor group 0 only.")]
    UnsupportedGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SavedSelectionError {
    #[error("The saved CPU selection has no topology signature. Review and save it again.")]
    MissingTopology,
    #[error("The CPU topology has changed. Review and save the selection again.")]
    TopologyChanged,
    #[error(transparent)]
    Selection(#[from] SelectionError),
}

impl CpuSelection {
    pub(crate) fn new(
        mut processors: Vec<ProcessorId>,
        topology: Option<TopologySignature>,
    ) -> Result<Self, SelectionError> {
        if processors.iter().any(|id| id.number >= 64) {
            return Err(SelectionError::InvalidNumber);
        }
        processors.sort_unstable();
        if processors.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(SelectionError::Duplicate);
        }
        Ok(Self {
            processors,
            topology,
        })
    }

    pub(crate) fn bind(
        topology: &CpuTopology,
        processors: Vec<ProcessorId>,
    ) -> Result<Self, SelectionError> {
        let selection = Self::new(processors, Some(topology.signature()))?;
        if selection.processors.iter().any(|id| {
            topology
                .processors
                .binary_search_by_key(id, |processor| processor.id)
                .is_err()
        }) {
            return Err(SelectionError::Unknown);
        }
        Ok(selection)
    }

    pub(crate) fn from_group_zero_mask(
        topology: &CpuTopology,
        mask: u64,
    ) -> Result<Self, SelectionError> {
        let processors = (0..64)
            .filter(|number| mask & (1_u64 << number) != 0)
            .map(|number| ProcessorId {
                group: 0,
                number: number as u8,
            })
            .collect();
        Self::bind(topology, processors)
    }

    pub(crate) fn processors(&self) -> &[ProcessorId] {
        &self.processors
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.processors.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn topology(&self) -> Option<&TopologySignature> {
        self.topology.as_ref()
    }

    pub(crate) fn resolve_processors(
        &self,
        topology: &CpuTopology,
    ) -> Result<&[ProcessorId], SavedSelectionError> {
        if self.is_empty() {
            return Ok(&self.processors);
        }
        let signature = self
            .topology
            .as_ref()
            .ok_or(SavedSelectionError::MissingTopology)?;
        if signature.0 != *topology {
            return Err(SavedSelectionError::TopologyChanged);
        }
        if self.processors.iter().any(|id| {
            topology
                .processors
                .binary_search_by_key(id, |processor| processor.id)
                .is_err()
        }) {
            return Err(SelectionError::Unknown.into());
        }
        Ok(&self.processors)
    }

    pub(crate) fn resolve_group_zero(
        &self,
        topology: &CpuTopology,
    ) -> Result<u64, SavedSelectionError> {
        let processors = self.resolve_processors(topology)?;
        if processors.is_empty() {
            return Ok(0);
        }
        topology.group_zero_mask(processors).map_err(Into::into)
    }

    pub(crate) fn rebind(&self, topology: &CpuTopology) -> Result<Self, SelectionError> {
        Self::bind(topology, self.processors.clone())
    }
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

    pub(crate) fn signature(&self) -> TopologySignature {
        TopologySignature::from(self)
    }

    #[cfg(test)]
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

mod group_mask {
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(mask: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("0x{mask:016x}"))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let value = String::deserialize(deserializer)?;
        let digits = value
            .strip_prefix("0x")
            .filter(|digits| {
                digits.len() == 16 && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            .ok_or_else(|| {
                <D::Error as serde::de::Error>::custom(
                    "Active processor masks require 0x followed by 16 hex digits",
                )
            })?;
        u64::from_str_radix(digits, 16).map_err(serde::de::Error::custom)
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
pub(crate) fn test_topology() -> CpuTopology {
    CpuTopology::new(
        (0..64)
            .map(|number| {
                let id = ProcessorId { group: 0, number };
                LogicalProcessor {
                    id,
                    core: id,
                    efficiency_class: 0,
                }
            })
            .collect(),
        vec![ProcessorGroup {
            number: 0,
            active_mask: u64::MAX,
        }],
        Vec::new(),
        Vec::new(),
    )
    .unwrap()
}

#[cfg(test)]
pub(crate) fn test_selection(mask: u64) -> CpuSelection {
    CpuSelection::from_group_zero_mask(&test_topology(), mask).unwrap()
}

#[cfg(test)]
mod tests;
