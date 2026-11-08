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

#[test]
fn saved_selections_round_trip_exact_members_and_canonical_signatures() {
    let topology = topology();
    let selection = CpuSelection::bind(&topology, vec![id(0, 63), id(0, 0)]).unwrap();
    assert_eq!(selection.processors(), &[id(0, 0), id(0, 63)]);
    assert_eq!(selection.topology(), Some(&topology.signature()));
    assert_eq!(
        selection.resolve_group_zero(&topology),
        Ok(1 | (1_u64 << 63))
    );
    let raw = toml::to_string(&selection).unwrap();
    assert!(raw.contains("0x8000000000000001"));
    assert_eq!(toml::from_str::<CpuSelection>(&raw).unwrap(), selection);
    let mut reordered = serde_json::to_value(&selection).unwrap();
    let signature = &mut reordered["topology"];
    signature["processors"].as_array_mut().unwrap().reverse();
    signature["groups"].as_array_mut().unwrap().reverse();
    signature["caches"][0]["processors"]
        .as_array_mut()
        .unwrap()
        .reverse();
    signature["numa_nodes"][0]["processors"]
        .as_array_mut()
        .unwrap()
        .reverse();
    reordered["processors"].as_array_mut().unwrap().reverse();
    assert_eq!(
        serde_json::from_value::<CpuSelection>(reordered).unwrap(),
        selection
    );
    assert_eq!(
        CpuSelection::from_group_zero_mask(&topology, 1 | (1_u64 << 63)).unwrap(),
        selection
    );
}

#[test]
fn saved_selections_preserve_unresolved_members_until_explicit_rebind() {
    let topology = topology();
    let selection = CpuSelection::bind(&topology, vec![id(0, 0), id(0, 63)]).unwrap();
    let smaller = CpuTopology::new(
        vec![topology.processors()[0].clone()],
        vec![ProcessorGroup {
            number: 0,
            active_mask: 1,
        }],
        Vec::new(),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(
        selection.resolve_group_zero(&smaller),
        Err(SavedSelectionError::TopologyChanged)
    );
    assert_eq!(selection.rebind(&smaller), Err(SelectionError::Unknown));
    assert_eq!(selection.processors(), &[id(0, 0), id(0, 63)]);
    assert_eq!(selection.group_zero_members_mask(), Ok(1 | (1_u64 << 63)));

    let mut changed = topology.clone();
    changed.caches[0].size_bytes += 1;
    assert_eq!(
        selection.resolve_group_zero(&changed),
        Err(SavedSelectionError::TopologyChanged)
    );
    let rebound = selection.rebind(&changed).unwrap();
    assert_eq!(rebound.processors(), selection.processors());
    assert_eq!(rebound.resolve_group_zero(&changed), Ok(1 | (1_u64 << 63)));

    let unbound = CpuSelection::new(vec![id(2, 31), id(0, 63)], None).unwrap();
    assert_eq!(
        unbound.resolve_group_zero(&topology),
        Err(SavedSelectionError::MissingTopology)
    );
    assert_eq!(unbound.rebind(&topology), Err(SelectionError::Unknown));
    let raw = toml::to_string(&unbound).unwrap();
    assert_eq!(toml::from_str::<CpuSelection>(&raw).unwrap(), unbound);
    assert!(!raw.contains("topology"));

    let mut without_signature = serde_json::to_value(&selection).unwrap();
    without_signature
        .as_object_mut()
        .unwrap()
        .remove("topology");
    let restored = serde_json::from_value::<CpuSelection>(without_signature).unwrap();
    assert_eq!(restored.processors(), selection.processors());
    assert_eq!(
        restored.resolve_group_zero(&topology),
        Err(SavedSelectionError::MissingTopology)
    );
}

#[test]
fn group_qualified_saved_selections_do_not_project_partially() {
    let topology = topology();
    for processors in [vec![id(1, 0)], vec![id(0, 0), id(1, 0)]] {
        let selection = CpuSelection::bind(&topology, processors.clone()).unwrap();
        assert_eq!(selection.processors(), processors);
        assert_eq!(
            selection.group_zero_members_mask(),
            Err(SelectionError::UnsupportedGroup)
        );
        assert_eq!(
            selection.resolve_group_zero(&topology),
            Err(SavedSelectionError::Selection(
                SelectionError::UnsupportedGroup
            ))
        );
        assert_eq!(selection.rebind(&topology).unwrap(), selection);
        let raw = toml::to_string(&selection).unwrap();
        assert_eq!(toml::from_str::<CpuSelection>(&raw).unwrap(), selection);
    }
    let empty = CpuSelection::default();
    assert!(empty.is_empty());
    assert!(empty.topology().is_none());
    assert_eq!(empty.resolve_group_zero(&topology), Ok(0));
    assert_eq!(empty.group_zero_members_mask(), Ok(0));
    assert_eq!(
        CpuSelection::from_group_zero_mask(&topology, 2),
        Err(SelectionError::Unknown)
    );
}

#[test]
fn saved_selection_deserialization_enforces_structural_invariants() {
    assert_eq!(
        CpuSelection::new(vec![id(0, 64)], None),
        Err(SelectionError::InvalidNumber)
    );
    assert_eq!(
        CpuSelection::new(vec![id(0, 1), id(0, 1)], None),
        Err(SelectionError::Duplicate)
    );
    let selection = CpuSelection::bind(&topology(), vec![id(0, 63)]).unwrap();
    for invalid in 0..9 {
        let mut value = serde_json::to_value(&selection).unwrap();
        match invalid {
            0 => value["processors"][0]["number"] = 64.into(),
            1 => {
                let member = value["processors"][0].clone();
                value["processors"].as_array_mut().unwrap().push(member);
            }
            2 => value["extra"] = true.into(),
            3 => value["processors"][0]["extra"] = true.into(),
            4 => value["topology"]["extra"] = true.into(),
            5 => value["topology"]["groups"][0]["active_mask"] = "0x0000000000000001".into(),
            6 => value["topology"]["groups"][0]["active_mask"] = "0x1".into(),
            7 => value["topology"]["processors"][0]["number"] = 64.into(),
            8 => value["topology"]["numa_nodes"][0]["processors"][0]["number"] = 1.into(),
            _ => unreachable!(),
        }
        assert!(serde_json::from_value::<CpuSelection>(value).is_err());
    }
}
