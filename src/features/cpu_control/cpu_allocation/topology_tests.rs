use super::*;
use crate::cpu_topology::{
    CacheDomain, CacheKind, CpuTopology, LogicalProcessor, ProcessorGroup, ProcessorId,
};

fn topology(caches: Vec<CacheDomain>) -> CpuTopology {
    let id = |group, number| ProcessorId { group, number };
    CpuTopology::new(
        vec![
            LogicalProcessor {
                id: id(0, 0),
                core: id(0, 0),
                efficiency_class: 1,
            },
            LogicalProcessor {
                id: id(0, 1),
                core: id(0, 0),
                efficiency_class: 1,
            },
            LogicalProcessor {
                id: id(1, 0),
                core: id(1, 0),
                efficiency_class: 0,
            },
        ],
        vec![
            ProcessorGroup {
                number: 0,
                active_mask: 3,
            },
            ProcessorGroup {
                number: 1,
                active_mask: 1,
            },
        ],
        caches,
        Vec::new(),
    )
    .unwrap()
}

#[test]
fn group_zero_projection_keeps_smt_and_uses_machine_efficiency_classes() {
    let processors = logical_processors_for_group_zero(&topology(Vec::new()));
    assert_eq!(
        processors.iter().map(|cpu| cpu.index).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(logical_processor_mask(&processors), 3);
    assert_eq!(logical_processor_no_smt_mask(&processors), 1);
    assert!(processors
        .iter()
        .all(|cpu| cpu.kind == LogicalProcessorKind::Performance));

    let source = topology(Vec::new());
    let mut processors = source.processors().to_vec();
    for processor in &mut processors {
        processor.efficiency_class = 0;
    }
    let homogeneous =
        CpuTopology::new(processors, source.groups().to_vec(), Vec::new(), Vec::new()).unwrap();
    assert!(logical_processors_for_group_zero(&homogeneous)
        .iter()
        .all(|cpu| cpu.kind == LogicalProcessorKind::Standard));
}

#[test]
fn cache_presets_require_the_entire_verified_l3_domain() {
    let group_zero = vec![
        ProcessorId {
            group: 0,
            number: 0,
        },
        ProcessorId {
            group: 0,
            number: 1,
        },
    ];
    let cache = CacheDomain {
        level: 3,
        kind: CacheKind::Unified,
        size_bytes: 32_000_000,
        processors: group_zero.clone(),
    };
    assert_eq!(
        cache_domain_masks_for_topology(&topology(vec![cache.clone()])),
        vec![CacheDomainMask {
            index: 1,
            group: 0,
            size_bytes: 32_000_000,
            mask: 3,
        }]
    );
    let mut spanning = cache.clone();
    spanning.processors.push(ProcessorId {
        group: 1,
        number: 0,
    });
    let mut other_group = cache.clone();
    other_group.processors = vec![ProcessorId {
        group: 1,
        number: 0,
    }];
    let mut instructions = cache.clone();
    instructions.kind = CacheKind::Instruction;
    let mut lower_level = cache;
    lower_level.level = 2;
    for unavailable in [spanning, other_group, instructions, lower_level] {
        assert!(cache_domain_masks_for_topology(&topology(vec![unavailable])).is_empty());
    }
    assert!(cache_domain_masks_for_topology(&topology(Vec::new())).is_empty());
}
