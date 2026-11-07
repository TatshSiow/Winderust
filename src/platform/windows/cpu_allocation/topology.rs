use std::{
    collections::BTreeMap,
    mem::{offset_of, size_of, size_of_val},
    ptr::{null_mut, read_unaligned},
    slice,
};

use windows_sys::Win32::{
    Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_DATA},
    System::SystemInformation::{
        CacheData as CACHE_DATA, CacheInstruction as CACHE_INSTRUCTION, CacheTrace as CACHE_TRACE,
        CacheUnified as CACHE_UNIFIED, GetLogicalProcessorInformationEx,
        RelationAll as RELATION_ALL, RelationCache as RELATION_CACHE,
        RelationGroup as RELATION_GROUP, RelationNumaNode as RELATION_NUMA_NODE,
        RelationNumaNodeEx as RELATION_NUMA_NODE_EX,
        RelationProcessorCore as RELATION_PROCESSOR_CORE, CACHE_RELATIONSHIP, GROUP_AFFINITY,
        GROUP_RELATIONSHIP, LOGICAL_PROCESSOR_RELATIONSHIP, NUMA_NODE_RELATIONSHIP,
        PROCESSOR_GROUP_INFO, PROCESSOR_RELATIONSHIP, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    },
};

use crate::{
    cpu_topology::{
        CacheDomain, CacheKind, CpuTopology, LogicalProcessor, NumaNode, ProcessorGroup,
        ProcessorId,
    },
    platform::windows::ProcessOperationError,
    win_util::last_error,
};

const MAX_TOPOLOGY_BYTES: usize = 16 * 1024 * 1024;

#[repr(C)]
#[derive(Clone, Copy)]
struct Header {
    relationship: LOGICAL_PROCESSOR_RELATIONSHIP,
    size: u32,
}

pub(crate) fn query() -> Result<CpuTopology, ProcessOperationError> {
    query_with(|buffer, required| {
        let pointer = if buffer.is_empty() {
            null_mut()
        } else {
            buffer
                .as_mut_ptr()
                .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>()
        };
        *required = size_of_val(buffer) as u32;
        // SAFETY: the aligned buffer has the byte capacity in required; a null buffer probes size.
        let ok = unsafe { GetLogicalProcessorInformationEx(RELATION_ALL, pointer, required) };
        if ok != 0 {
            Ok(())
        } else {
            Err(last_error())
        }
    })
}

fn query_with(
    mut capture: impl FnMut(&mut [u64], &mut u32) -> Result<(), u32>,
) -> Result<CpuTopology, ProcessOperationError> {
    let mut buffer = Vec::<u64>::new();
    for _ in 0..4 {
        let mut required = 0;
        let capacity = size_of_val(buffer.as_slice());
        match capture(&mut buffer, &mut required) {
            Ok(()) => {
                let length = required as usize;
                if length == 0 || length > capacity {
                    return Err(invalid_data());
                }
                // SAFETY: the API initialized length bytes in the allocated, aligned output buffer.
                let bytes = unsafe { slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), length) };
                return from_bytes(bytes);
            }
            Err(ERROR_INSUFFICIENT_BUFFER) => {
                let length = required as usize;
                if length <= capacity || length > MAX_TOPOLOGY_BYTES {
                    return Err(invalid_data());
                }
                buffer.resize(length.div_ceil(size_of::<u64>()), 0);
            }
            Err(code) => {
                return Err(ProcessOperationError::Failed {
                    operation: "GetLogicalProcessorInformationEx",
                    code,
                })
            }
        }
    }
    Err(ProcessOperationError::Failed {
        operation: "GetLogicalProcessorInformationEx",
        code: ERROR_INSUFFICIENT_BUFFER,
    })
}

fn from_bytes(buffer: &[u8]) -> Result<CpuTopology, ProcessOperationError> {
    let mut processors = Vec::new();
    let mut groups = None;
    let mut caches = Vec::new();
    let mut nodes = BTreeMap::<u32, Vec<ProcessorId>>::new();
    let mut offset = 0;
    while offset < buffer.len() {
        let header: Header = read(buffer, offset)?;
        let record_end = offset
            .checked_add(header.size as usize)
            .ok_or_else(invalid_data)?;
        if (header.size as usize) < size_of::<Header>() || record_end > buffer.len() {
            return Err(invalid_data());
        }
        let record = &buffer[offset + size_of::<Header>()..record_end];
        match header.relationship {
            RELATION_PROCESSOR_CORE => {
                let core: PROCESSOR_RELATIONSHIP = read(record, 0)?;
                if core.GroupCount != 1 {
                    return Err(invalid_data());
                }
                let members =
                    affinity_members(record, offset_of!(PROCESSOR_RELATIONSHIP, GroupMask), 1)?;
                let core_id = members[0];
                processors.extend(members.into_iter().map(|id| LogicalProcessor {
                    id,
                    core: core_id,
                    efficiency_class: core.EfficiencyClass,
                }));
            }
            RELATION_CACHE => {
                let cache: CACHE_RELATIONSHIP = read(record, 0)?;
                // Legacy cache records use the same singleton mask offset with GroupCount reserved as zero.
                let count = usize::from(cache.GroupCount.max(1));
                caches.push(CacheDomain {
                    level: cache.Level,
                    kind: match cache.Type {
                        CACHE_UNIFIED => CacheKind::Unified,
                        CACHE_INSTRUCTION => CacheKind::Instruction,
                        CACHE_DATA => CacheKind::Data,
                        CACHE_TRACE => CacheKind::Trace,
                        other => CacheKind::Unknown(other),
                    },
                    size_bytes: cache.CacheSize,
                    processors: affinity_members(
                        record,
                        offset_of!(CACHE_RELATIONSHIP, Anonymous),
                        count,
                    )?,
                });
            }
            RELATION_NUMA_NODE | RELATION_NUMA_NODE_EX => {
                let node: NUMA_NODE_RELATIONSHIP = read(record, 0)?;
                let members = affinity_members(
                    record,
                    offset_of!(NUMA_NODE_RELATIONSHIP, Anonymous),
                    usize::from(node.GroupCount.max(1)),
                )?;
                if let Some(previous) = nodes.get_mut(&node.NodeNumber) {
                    if previous.iter().all(|id| members.binary_search(id).is_ok()) {
                        *previous = members;
                    } else if !members.iter().all(|id| previous.binary_search(id).is_ok()) {
                        return Err(invalid_data());
                    }
                } else {
                    nodes.insert(node.NodeNumber, members);
                }
            }
            RELATION_GROUP => {
                if groups.is_some() {
                    return Err(invalid_data());
                }
                let relationship: GROUP_RELATIONSHIP = read(record, 0)?;
                if relationship.ActiveGroupCount == 0
                    || relationship.ActiveGroupCount > relationship.MaximumGroupCount
                {
                    return Err(invalid_data());
                }
                let mut active = Vec::new();
                for number in 0..relationship.ActiveGroupCount {
                    let info: PROCESSOR_GROUP_INFO = read(
                        record,
                        offset_of!(GROUP_RELATIONSHIP, GroupInfo)
                            + usize::from(number) * size_of::<PROCESSOR_GROUP_INFO>(),
                    )?;
                    if info.ActiveProcessorCount == 0
                        || info.MaximumProcessorCount > 64
                        || info.ActiveProcessorCount > info.MaximumProcessorCount
                        || info.ActiveProcessorMask.count_ones()
                            != u32::from(info.ActiveProcessorCount)
                    {
                        return Err(invalid_data());
                    }
                    active.push(ProcessorGroup {
                        number,
                        active_mask: info.ActiveProcessorMask as u64,
                    });
                }
                groups = Some(active);
            }
            _ => {}
        }
        offset = record_end;
    }
    CpuTopology::new(
        processors,
        groups.ok_or_else(invalid_data)?,
        caches,
        nodes
            .into_iter()
            .map(|(number, processors)| NumaNode { number, processors })
            .collect(),
    )
    .map_err(|_| invalid_data())
}

fn affinity_members(
    record: &[u8],
    offset: usize,
    count: usize,
) -> Result<Vec<ProcessorId>, ProcessOperationError> {
    let length = count
        .checked_mul(size_of::<GROUP_AFFINITY>())
        .and_then(|length| offset.checked_add(length))
        .ok_or_else(invalid_data)?;
    // Native prefix offsets and affinity strides include ABI alignment; a tail would hide members.
    if count == 0 || length != record.len() {
        return Err(invalid_data());
    }
    let mut members = Vec::new();
    let mut seen_groups = std::collections::BTreeSet::new();
    for index in 0..count {
        let affinity: GROUP_AFFINITY = read(record, offset + index * size_of::<GROUP_AFFINITY>())?;
        if affinity.Mask == 0 || !seen_groups.insert(affinity.Group) {
            return Err(invalid_data());
        }
        for number in 0..usize::BITS {
            if affinity.Mask & (1_usize << number) != 0 {
                members.push(ProcessorId {
                    group: affinity.Group,
                    number: number as u8,
                });
            }
        }
    }
    members.sort_unstable();
    Ok(members)
}

fn read<T: Copy>(buffer: &[u8], offset: usize) -> Result<T, ProcessOperationError> {
    if offset
        .checked_add(size_of::<T>())
        .is_none_or(|end| end > buffer.len())
    {
        return Err(invalid_data());
    }
    // SAFETY: the complete T lies within the initialized native record; packed records may be unaligned.
    Ok(unsafe { read_unaligned(buffer.as_ptr().add(offset).cast::<T>()) })
}

fn invalid_data() -> ProcessOperationError {
    ProcessOperationError::Failed {
        operation: "CPU topology data",
        code: ERROR_INVALID_DATA,
    }
}

#[cfg(test)]
mod tests;
