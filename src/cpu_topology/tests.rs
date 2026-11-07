use super::*;

fn id(group: u16, number: u8) -> ProcessorId {
    ProcessorId { group, number }
}

fn topology() -> CpuTopology {
    CpuTopology::new(
        vec![
            LogicalProcessor {
                id: id(1, 0),
                core: id(1, 0),
                efficiency_class: 0,
            },
            LogicalProcessor {
                id: id(0, 63),
                core: id(0, 0),
                efficiency_class: 1,
            },
            LogicalProcessor {
                id: id(0, 0),
                core: id(0, 0),
                efficiency_class: 1,
            },
        ],
        vec![
            ProcessorGroup {
                number: 1,
                active_mask: 1,
            },
            ProcessorGroup {
                number: 0,
                active_mask: 1 | (1_u64 << 63),
            },
        ],
        vec![CacheDomain {
            level: 3,
            kind: CacheKind::Unified,
            size_bytes: 32 * 1024 * 1024,
            processors: vec![id(0, 63), id(0, 0)],
        }],
        vec![NumaNode {
            number: 0,
            processors: vec![id(1, 0), id(0, 63), id(0, 0)],
        }],
    )
    .unwrap()
}

#[test]
fn topology_is_canonical_and_group_zero_projection_is_exact() {
    let topology = topology();
    let mut reordered = topology.clone();
    reordered.processors.reverse();
    reordered.groups.reverse();
    reordered.caches[0].processors.reverse();
    reordered.numa_nodes[0].processors.reverse();
    assert_eq!(
        topology,
        CpuTopology::new(
            reordered.processors,
            reordered.groups,
            reordered.caches,
            reordered.numa_nodes
        )
        .unwrap()
    );
    assert_eq!(topology.group_zero_processors().count(), 2);
    assert_eq!(
        topology.group_zero_mask(&[id(0, 0), id(0, 63)]),
        Ok(1 | (1_u64 << 63))
    );
    assert_eq!(topology.group_zero_mask(&[]), Err(SelectionError::Empty));
    assert_eq!(
        topology.group_zero_mask(&[id(0, 0), id(0, 0)]),
        Err(SelectionError::Duplicate)
    );
    assert_eq!(
        topology.group_zero_mask(&[id(0, 1)]),
        Err(SelectionError::Unknown)
    );
    assert_eq!(
        topology.group_zero_mask(&[id(1, 0)]),
        Err(SelectionError::UnsupportedGroup)
    );
    assert_eq!(
        topology.group_zero_mask(&[id(0, 0), id(1, 0)]),
        Err(SelectionError::UnsupportedGroup)
    );
}

#[test]
fn topology_rejects_inconsistent_core_group_and_domain_membership() {
    let original = topology();
    for invalid in 0..6 {
        let mut topology = original.clone();
        match invalid {
            0 => topology.processors.push(topology.processors[0].clone()),
            1 => topology.groups[0].active_mask |= 2,
            2 => topology.processors[0].core = id(0, 63),
            3 => topology.caches[0].processors.push(id(0, 1)),
            4 => topology.caches.push(topology.caches[0].clone()),
            5 => topology.numa_nodes.push(NumaNode {
                number: 1,
                processors: vec![id(0, 0)],
            }),
            _ => unreachable!(),
        }
        assert!(CpuTopology::new(
            topology.processors,
            topology.groups,
            topology.caches,
            topology.numa_nodes
        )
        .is_err());
    }
}
