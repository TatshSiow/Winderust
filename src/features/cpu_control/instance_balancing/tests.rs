use super::*;
use crate::cpu_topology::{LogicalProcessor, ProcessorGroup};

fn target(id: u32, creation_time: u64) -> ProcessControlTarget {
    ProcessControlTarget::automatic(
        id,
        "app.exe".into(),
        "C:/Apps/app.exe".into(),
        creation_time,
    )
}

fn layout(classes: &[u8], siblings: u8) -> CpuTopology {
    let count = classes.len() as u8 * siblings;
    CpuTopology::new(
        classes
            .iter()
            .enumerate()
            .flat_map(|(core, class)| {
                (0..siblings).map(move |sibling| LogicalProcessor {
                    id: processor(core as u8 * siblings + sibling),
                    core: processor(core as u8 * siblings),
                    efficiency_class: *class,
                })
            })
            .collect(),
        vec![ProcessorGroup {
            number: 0,
            active_mask: if count == 64 {
                u64::MAX
            } else {
                (1_u64 << count) - 1
            },
        }],
        Vec::new(),
        Vec::new(),
    )
    .unwrap()
}

fn processor(number: u8) -> ProcessorId {
    ProcessorId { group: 0, number }
}

fn all(topology: &CpuTopology) -> BTreeSet<ProcessorId> {
    topology.processors().iter().map(|cpu| cpu.id).collect()
}

fn assigned_cores(topology: &CpuTopology, assignment: &Assignment) -> BTreeSet<ProcessorId> {
    topology
        .processors()
        .iter()
        .filter(|cpu| assignment.selection.processors().contains(&cpu.id))
        .map(|cpu| cpu.core)
        .collect()
}

fn assert_partition(topology: &CpuTopology, allowed: &BTreeSet<ProcessorId>, plan: &[Assignment]) {
    let mut assigned = BTreeSet::new();
    let mut counts = Vec::new();
    let mut class_counts = BTreeMap::<u8, Vec<usize>>::new();
    for assignment in plan {
        let selected = assignment.selection.resolve_processors(topology).unwrap();
        assert!(!selected.is_empty());
        assert!(selected
            .iter()
            .all(|id| allowed.contains(id) && assigned.insert(*id)));
        let cores = assigned_cores(topology, assignment);
        counts.push(cores.len());
        let mut classes = BTreeMap::<u8, BTreeSet<ProcessorId>>::new();
        for cpu in topology.processors() {
            if cores.contains(&cpu.core) {
                assert!(
                    selected.contains(&cpu.id),
                    "every SMT sibling must follow its core"
                );
                classes
                    .entry(cpu.efficiency_class)
                    .or_default()
                    .insert(cpu.core);
            }
        }
        for class in topology.processors().iter().map(|cpu| cpu.efficiency_class) {
            class_counts.entry(class).or_default();
        }
        for (class, counts) in &mut class_counts {
            counts.push(classes.get(class).map_or(0, BTreeSet::len));
        }
    }
    let eligible: BTreeSet<_> = topology
        .processors()
        .iter()
        .filter(|cpu| {
            topology
                .processors()
                .iter()
                .filter(|sibling| sibling.core == cpu.core)
                .all(|sibling| allowed.contains(&sibling.id))
        })
        .map(|cpu| cpu.id)
        .collect();
    assert_eq!(assigned, eligible);
    for counts in std::iter::once(&counts).chain(class_counts.values()) {
        assert!(counts.iter().max().unwrap() - counts.iter().min().unwrap() <= 1);
    }
}

#[test]
fn equal_class_quotas_cover_the_pool_and_remain_stable_through_churn() {
    for first in 0..=8 {
        for second in 0..=8 {
            if first + second == 0 {
                continue;
            }
            let classes = [vec![0; first], vec![1; second]].concat();
            let topology = layout(&classes, 2);
            let allowed = all(&topology);
            for count in 2..=6 {
                let members = (1..=count)
                    .map(|id| target(id, u64::from(id)))
                    .collect::<Vec<_>>();
                if classes.len() < count as usize {
                    assert_eq!(
                        plan(&topology, &allowed, &members, &[]),
                        Err(PlanError::InsufficientCapacity)
                    );
                    continue;
                }
                let before = plan(&topology, &allowed, &members, &[]).unwrap();
                assert_partition(&topology, &allowed, &before);
                assert_eq!(
                    plan(&topology, &allowed, &members, &before).unwrap(),
                    before
                );
                if classes.len() > count as usize {
                    let mut added = members.clone();
                    added.push(target(count + 1, u64::from(count + 1)));
                    let after = plan(&topology, &allowed, &added, &before).unwrap();
                    assert_partition(&topology, &allowed, &after);
                    for (old, new) in before.iter().zip(&after) {
                        let old_cores = assigned_cores(&topology, old);
                        let new_cores = assigned_cores(&topology, new);
                        for class in [0, 1] {
                            let class_cores: BTreeSet<_> = topology
                                .processors()
                                .iter()
                                .filter(|cpu| cpu.efficiency_class == class)
                                .map(|cpu| cpu.core)
                                .collect();
                            assert_eq!(
                                old_cores
                                    .intersection(&new_cores)
                                    .filter(|id| class_cores.contains(*id))
                                    .count(),
                                old_cores
                                    .intersection(&class_cores)
                                    .count()
                                    .min(new_cores.intersection(&class_cores).count())
                            );
                        }
                    }
                    let removed = plan(&topology, &allowed, &members, &after).unwrap();
                    assert_partition(&topology, &allowed, &removed);
                    assert_eq!(
                        plan(&topology, &allowed, &members, &removed).unwrap(),
                        removed
                    );
                }
            }
        }
    }
}

#[test]
fn admission_order_survives_arrival_exit_and_reused_pids() {
    let topology = layout(&[0; 7], 1);
    let allowed = all(&topology);
    let members = [target(10, 20), target(30, 10), target(20, 10)];
    let before = plan(&topology, &allowed, &members, &[]).unwrap();
    assert_eq!(
        before
            .iter()
            .map(|entry| entry.target.id)
            .collect::<Vec<_>>(),
        [20, 30, 10]
    );
    let next = [target(10, 30), target(1, 1), target(20, 10)];
    let after = plan(&topology, &allowed, &next, &before).unwrap();
    assert_eq!(
        after
            .iter()
            .map(|entry| entry.target.id)
            .collect::<Vec<_>>(),
        [20, 1, 10]
    );
    assert_eq!(after[2].target.creation_time, 30);
    assert_partition(&topology, &allowed, &after);
}

#[test]
fn numeric_class_remainders_rotate_across_three_classes() {
    let topology = layout(&[9, 9, 2, 7, 7], 1);
    let members = [target(1, 1), target(2, 2), target(3, 3)];
    let result = plan(&topology, &all(&topology), &members, &[]).unwrap();
    assert_partition(&topology, &all(&topology), &result);
    assert_eq!(
        result[0].selection.processors(),
        [processor(0), processor(2)]
    );
    assert_eq!(
        result[1].selection.processors(),
        [processor(1), processor(3)]
    );
    assert_eq!(result[2].selection.processors(), [processor(4)]);
}

#[test]
fn hybrid_cores_keep_their_different_sibling_counts() {
    let topology = CpuTopology::new(
        [
            (0, 0, 1),
            (1, 0, 1),
            (2, 2, 1),
            (3, 2, 1),
            (4, 4, 0),
            (5, 5, 0),
        ]
        .into_iter()
        .map(|(number, core, efficiency_class)| LogicalProcessor {
            id: processor(number),
            core: processor(core),
            efficiency_class,
        })
        .collect(),
        vec![ProcessorGroup {
            number: 0,
            active_mask: 0b111111,
        }],
        vec![],
        vec![],
    )
    .unwrap();
    let members = [target(1, 1), target(2, 2)];
    let result = plan(&topology, &all(&topology), &members, &[]).unwrap();
    assert_partition(&topology, &all(&topology), &result);
    assert!(result
        .iter()
        .all(|entry| entry.selection.processors().len() == 3));
}

#[test]
fn partial_smt_and_reserved_siblings_remove_the_whole_core() {
    let topology = layout(&[0, 0, 1, 1], 2);
    let members = [target(1, 1), target(2, 2)];
    let before = plan(&topology, &all(&topology), &members, &[]).unwrap();
    let allowed = [0, 1, 2, 4, 5, 7].into_iter().map(processor).collect();
    let after = plan(&topology, &allowed, &members, &before).unwrap();
    assert_partition(&topology, &allowed, &after);
    assert!(after.iter().all(|entry| entry
        .selection
        .processors()
        .iter()
        .all(|id| [0, 1, 4, 5].contains(&id.number))));
    let insufficient = [0, 1, 2, 4, 7].into_iter().map(processor).collect();
    assert_eq!(
        plan(&topology, &insufficient, &members, &after),
        Err(PlanError::InsufficientCapacity)
    );
}

#[test]
fn stale_partial_and_overlapping_hints_never_authorize_processors() {
    let topology = layout(&[0, 0, 1], 2);
    let members = [target(1, 1), target(2, 2)];
    let previous = [
        Assignment {
            target: members[0].clone(),
            selection: CpuSelection::bind(&topology, vec![processor(2)]).unwrap(),
        },
        Assignment {
            target: members[1].clone(),
            selection: CpuSelection::bind(&topology, vec![processor(2), processor(3)]).unwrap(),
        },
    ];
    let result = plan(&topology, &all(&topology), &members, &previous).unwrap();
    assert_partition(&topology, &all(&topology), &result);
    assert_eq!(
        result[1].selection.processors(),
        [processor(2), processor(3)]
    );

    let changed = layout(&[1, 1, 0], 2);
    assert_eq!(
        plan(&changed, &all(&changed), &members, &result).unwrap(),
        plan(&changed, &all(&changed), &members, &[]).unwrap()
    );
    let overlapping = [
        result[0].clone(),
        Assignment {
            target: members[1].clone(),
            selection: result[0].selection.clone(),
        },
    ];
    assert_partition(
        &topology,
        &all(&topology),
        &plan(&topology, &all(&topology), &members, &overlapping).unwrap(),
    );
}

#[test]
fn incomplete_capacity_invalid_pool_and_ambiguous_members_cannot_make_a_plan() {
    let topology = layout(&[0, 0], 1);
    let allowed = all(&topology);
    let member = target(1, 1);
    for members in [vec![], vec![member.clone()]] {
        assert_eq!(
            plan(&topology, &allowed, &members, &[]),
            Err(PlanError::WaitingForInstances)
        );
    }
    for members in [
        [member.clone(), member.clone()],
        [member.clone(), target(1, 2)],
    ] {
        assert_eq!(
            plan(&topology, &allowed, &members, &[]),
            Err(PlanError::DuplicateMember)
        );
    }
    let members = [member, target(2, 2)];
    assert_eq!(
        plan(&topology, &BTreeSet::new(), &members, &[]),
        Err(PlanError::InsufficientCapacity)
    );
    let invalid = [processor(0), processor(2)].into_iter().collect();
    assert_eq!(
        plan(&topology, &invalid, &members, &[]),
        Err(PlanError::InvalidPool)
    );

    for multiple in [false, true] {
        let group_one = ProcessorId {
            group: 1,
            number: 0,
        };
        let mut processors = vec![LogicalProcessor {
            id: group_one,
            core: group_one,
            efficiency_class: 0,
        }];
        let mut groups = vec![ProcessorGroup {
            number: 1,
            active_mask: 1,
        }];
        if multiple {
            processors.extend(topology.processors().iter().cloned());
            groups.extend(topology.groups().iter().cloned());
        }
        let unsupported = CpuTopology::new(processors, groups, vec![], vec![]).unwrap();
        assert_eq!(
            plan(&unsupported, &all(&unsupported), &members, &[]),
            Err(PlanError::UnsupportedTopology)
        );
    }
}
