use super::*;

#[test]
fn host_topology_query_preserves_a_representable_group_zero_domain(
) -> Result<(), ProcessOperationError> {
    let topology = query()?;
    let selected = topology
        .group_zero_processors()
        .map(|processor| processor.id)
        .collect::<Vec<_>>();
    assert!(topology.group_zero_mask(&selected).is_ok());
    eprintln!(
        "Topology: {} groups, {} logical processors, {} cache domains, {} NUMA nodes",
        topology.groups().len(),
        topology.processors().len(),
        topology.caches().len(),
        topology.numa_nodes().len()
    );
    Ok(())
}

fn put<const N: usize>(record: &mut [u8], offset: usize, bytes: [u8; N]) {
    record[offset..offset + N].copy_from_slice(&bytes);
}

fn record(buffer: &mut Vec<u8>, relationship: i32, payload_size: usize) -> &mut [u8] {
    let start = buffer.len();
    let size = size_of::<Header>() + payload_size;
    buffer.resize(start + size, 0);
    put(buffer, start, relationship.to_ne_bytes());
    put(
        buffer,
        start + offset_of!(Header, size),
        (size as u32).to_ne_bytes(),
    );
    &mut buffer[start + size_of::<Header>()..]
}

fn affinity(record: &mut [u8], offset: usize, group: u16, mask: u64) {
    put(
        record,
        offset + offset_of!(GROUP_AFFINITY, Mask),
        (mask as usize).to_ne_bytes(),
    );
    put(
        record,
        offset + offset_of!(GROUP_AFFINITY, Group),
        group.to_ne_bytes(),
    );
}

fn core(buffer: &mut Vec<u8>, group: u16, mask: u64, efficiency: u8) {
    let payload = record(
        buffer,
        RELATION_PROCESSOR_CORE,
        size_of::<PROCESSOR_RELATIONSHIP>(),
    );
    payload[offset_of!(PROCESSOR_RELATIONSHIP, EfficiencyClass)] = efficiency;
    put(
        payload,
        offset_of!(PROCESSOR_RELATIONSHIP, GroupCount),
        1_u16.to_ne_bytes(),
    );
    affinity(
        payload,
        offset_of!(PROCESSOR_RELATIONSHIP, GroupMask),
        group,
        mask,
    );
}

fn groups(buffer: &mut Vec<u8>, masks: &[u64]) {
    let info_offset = offset_of!(GROUP_RELATIONSHIP, GroupInfo);
    let payload = record(
        buffer,
        RELATION_GROUP,
        info_offset + masks.len() * size_of::<PROCESSOR_GROUP_INFO>(),
    );
    put(
        payload,
        offset_of!(GROUP_RELATIONSHIP, MaximumGroupCount),
        (masks.len() as u16).to_ne_bytes(),
    );
    put(
        payload,
        offset_of!(GROUP_RELATIONSHIP, ActiveGroupCount),
        (masks.len() as u16).to_ne_bytes(),
    );
    for (index, mask) in masks.iter().enumerate() {
        let offset = info_offset + index * size_of::<PROCESSOR_GROUP_INFO>();
        payload[offset + offset_of!(PROCESSOR_GROUP_INFO, MaximumProcessorCount)] = 64;
        payload[offset + offset_of!(PROCESSOR_GROUP_INFO, ActiveProcessorCount)] =
            mask.count_ones() as u8;
        put(
            payload,
            offset + offset_of!(PROCESSOR_GROUP_INFO, ActiveProcessorMask),
            (*mask as usize).to_ne_bytes(),
        );
    }
}

fn cache(buffer: &mut Vec<u8>, count: u16, members: &[(u16, u64)], bytes: u32) {
    let offset = offset_of!(CACHE_RELATIONSHIP, Anonymous);
    let payload = record(
        buffer,
        RELATION_CACHE,
        offset + members.len() * size_of::<GROUP_AFFINITY>(),
    );
    payload[offset_of!(CACHE_RELATIONSHIP, Level)] = 3;
    put(
        payload,
        offset_of!(CACHE_RELATIONSHIP, CacheSize),
        bytes.to_ne_bytes(),
    );
    put(
        payload,
        offset_of!(CACHE_RELATIONSHIP, Type),
        CACHE_UNIFIED.to_ne_bytes(),
    );
    put(
        payload,
        offset_of!(CACHE_RELATIONSHIP, GroupCount),
        count.to_ne_bytes(),
    );
    for (index, (group, mask)) in members.iter().enumerate() {
        affinity(
            payload,
            offset + index * size_of::<GROUP_AFFINITY>(),
            *group,
            *mask,
        );
    }
}

fn numa(buffer: &mut Vec<u8>, relationship: i32, number: u32, count: u16, members: &[(u16, u64)]) {
    let offset = offset_of!(NUMA_NODE_RELATIONSHIP, Anonymous);
    let payload = record(
        buffer,
        relationship,
        offset + members.len() * size_of::<GROUP_AFFINITY>(),
    );
    put(
        payload,
        offset_of!(NUMA_NODE_RELATIONSHIP, NodeNumber),
        number.to_ne_bytes(),
    );
    put(
        payload,
        offset_of!(NUMA_NODE_RELATIONSHIP, GroupCount),
        count.to_ne_bytes(),
    );
    for (index, (group, mask)) in members.iter().enumerate() {
        affinity(
            payload,
            offset + index * size_of::<GROUP_AFFINITY>(),
            *group,
            *mask,
        );
    }
}

fn fixture() -> Vec<u8> {
    let mut buffer = Vec::new();
    groups(&mut buffer, &[0b1011, 0b11]);
    core(&mut buffer, 0, 0b11, 1);
    core(&mut buffer, 0, 0b1000, 0);
    core(&mut buffer, 1, 0b11, 0);
    cache(&mut buffer, 0, &[(0, 0b1011)], 64 * 1024 * 1024);
    cache(&mut buffer, 1, &[(1, 0b11)], 32 * 1024 * 1024);
    numa(&mut buffer, RELATION_NUMA_NODE, 0, 0, &[(0, 0b1011)]);
    numa(
        &mut buffer,
        RELATION_NUMA_NODE_EX,
        0,
        2,
        &[(0, 0b1011), (1, 0b11)],
    );
    buffer
}

#[test]
fn topology_preserves_groups_smt_efficiency_cache_and_numa_without_record_order_identity() {
    let bytes = fixture();
    let topology = from_bytes(&bytes).unwrap();
    let mut with_unknown = bytes.clone();
    record(&mut with_unknown, i32::MAX, 0);
    assert_eq!(topology, from_bytes(&with_unknown).unwrap());
    assert_eq!(topology.processors().len(), 5);
    assert_eq!(topology.groups().len(), 2);
    assert_eq!(topology.processors()[0].core, topology.processors()[1].core);
    assert_eq!(topology.processors()[0].efficiency_class, 1);
    assert_eq!(topology.processors()[2].efficiency_class, 0);
    assert_eq!(topology.caches().len(), 2);
    assert_eq!(
        topology
            .caches()
            .iter()
            .map(|cache| cache.size_bytes)
            .sum::<u32>(),
        96 * 1024 * 1024
    );
    assert_eq!(topology.numa_nodes().len(), 1);
    assert_eq!(topology.numa_nodes()[0].processors.len(), 5);
    let mut records = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let header: Header = read(&bytes, offset).unwrap();
        let end = offset + header.size as usize;
        records.push(bytes[offset..end].to_vec());
        offset = end;
    }
    records.reverse();
    assert_eq!(topology, from_bytes(&records.concat()).unwrap());
}

#[test]
fn minimal_core_records_do_not_fabricate_optional_metadata() {
    let mut bytes = Vec::new();
    groups(&mut bytes, &[1 | (1_u64 << 63)]);
    core(&mut bytes, 0, 1, 1);
    core(&mut bytes, 0, 1_u64 << 63, 0);
    let topology = from_bytes(&bytes).unwrap();
    assert!(topology.caches().is_empty());
    assert!(topology.numa_nodes().is_empty());
    assert_eq!(topology.processors()[1].id.number, 63);
}

#[test]
fn malformed_records_after_a_valid_prefix_are_rejected() {
    let prefix = fixture();
    let mut partial_header = prefix.clone();
    partial_header.extend_from_slice(&[0; 3]);
    let mut partial_record = prefix.clone();
    core(&mut partial_record, 0, 2, 0);
    partial_record.pop();
    let mut short_core = prefix.clone();
    record(&mut short_core, RELATION_PROCESSOR_CORE, 0);
    let mut zero_size = prefix.clone();
    zero_size.extend_from_slice(&[0; size_of::<Header>()]);
    for bytes in [partial_header, partial_record, short_core, zero_size] {
        assert_eq!(from_bytes(&bytes), Err(invalid_data()));
    }
    for count in [0_u16, 2] {
        let mut bytes = prefix.clone();
        let offset =
            bytes.len() + size_of::<Header>() + offset_of!(PROCESSOR_RELATIONSHIP, GroupCount);
        core(&mut bytes, 0, 2, 0);
        put(&mut bytes, offset, count.to_ne_bytes());
        assert_eq!(from_bytes(&bytes), Err(invalid_data()));
    }
}

#[test]
fn counts_duplicate_processors_and_inconsistent_domain_memberships_are_rejected() {
    for invalid in 0..9 {
        let mut bytes = fixture();
        match invalid {
            0 => core(&mut bytes, 0, 1, 0),
            1 => core(&mut bytes, 0, 0, 0),
            2 => cache(&mut bytes, 2, &[(0, 0b1011)], 1),
            3 => cache(&mut bytes, 2, &[(0, 0b1011), (0, 0b100)], 1),
            4 => cache(&mut bytes, 1, &[(0, 0b100)], 1),
            5 => numa(&mut bytes, RELATION_NUMA_NODE_EX, 1, 1, &[(0, 1)]),
            6 => numa(&mut bytes, RELATION_NUMA_NODE_EX, 0, 1, &[(2, 1)]),
            7 => groups(&mut bytes, &[1]),
            8 => {
                let count_offset =
                    size_of::<Header>() + offset_of!(GROUP_RELATIONSHIP, ActiveGroupCount);
                put(&mut bytes, count_offset, 3_u16.to_ne_bytes());
            }
            _ => unreachable!(),
        }
        assert_eq!(from_bytes(&bytes), Err(invalid_data()));
    }
}

#[test]
fn cache_and_numa_affinity_counts_cannot_hide_trailing_members() {
    let mut complete = Vec::new();
    groups(&mut complete, &[1, 1]);
    core(&mut complete, 0, 1, 0);
    core(&mut complete, 1, 1, 0);
    cache(&mut complete, 2, &[(0, 1), (1, 1)], 1);
    let topology = from_bytes(&complete).unwrap();
    assert_eq!(topology.caches()[0].processors.len(), 2);
    assert!(topology
        .group_zero_mask(&topology.caches()[0].processors)
        .is_err());
    for count in [0, 1] {
        let mut bytes = fixture();
        cache(&mut bytes, count, &[(0, 0b1011), (1, 0b11)], 1);
        assert_eq!(from_bytes(&bytes), Err(invalid_data()));
        for relationship in [RELATION_NUMA_NODE, RELATION_NUMA_NODE_EX] {
            let mut bytes = fixture();
            numa(
                &mut bytes,
                relationship,
                0,
                count,
                &[(0, 0b1011), (1, 0b11)],
            );
            assert_eq!(from_bytes(&bytes), Err(invalid_data()));
        }
    }
}

#[test]
fn topology_query_retries_growth_and_bounds_allocation_and_native_output() {
    let bytes = fixture();
    let mut attempts = 0;
    let result = query_with(|buffer, required| {
        attempts += 1;
        if attempts < 3 {
            *required = if attempts == 1 { 8 } else { bytes.len() as u32 };
            return Err(ERROR_INSUFFICIENT_BUFFER);
        }
        for (word, chunk) in buffer.iter_mut().zip(bytes.chunks(size_of::<u64>())) {
            let mut value = [0; size_of::<u64>()];
            value[..chunk.len()].copy_from_slice(chunk);
            *word = u64::from_ne_bytes(value);
        }
        *required = bytes.len() as u32;
        Ok(())
    });
    assert_eq!(result.unwrap(), from_bytes(&bytes).unwrap());
    assert_eq!(attempts, 3);
    assert_eq!(
        query_with(|_, required| {
            *required = MAX_TOPOLOGY_BYTES as u32 + 1;
            Err(ERROR_INSUFFICIENT_BUFFER)
        }),
        Err(invalid_data())
    );
    assert_eq!(
        query_with(|_, required| {
            *required = 1;
            Ok(())
        }),
        Err(invalid_data())
    );
    let error = 5_u32;
    assert_eq!(
        query_with(|_, _| Err(error)),
        Err(ProcessOperationError::Failed {
            operation: "GetLogicalProcessorInformationEx",
            code: error
        })
    );
    let mut size = 0;
    assert_eq!(
        query_with(|_, required| {
            size += 8;
            *required = size;
            Err(ERROR_INSUFFICIENT_BUFFER)
        }),
        Err(ProcessOperationError::Failed {
            operation: "GetLogicalProcessorInformationEx",
            code: ERROR_INSUFFICIENT_BUFFER
        })
    );
}
