use super::*;
use crate::cpu_topology::{
    CacheDomain, CpuTopology, LogicalProcessor, ProcessorGroup, ProcessorId,
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
fn group_qualified_processors_keep_repeated_numbers_and_machine_core_kinds() {
    let processors = logical_processors_for_topology(&topology(Vec::new()));
    assert_eq!(
        processors.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![
            ProcessorId {
                group: 0,
                number: 0
            },
            ProcessorId {
                group: 0,
                number: 1
            },
            ProcessorId {
                group: 1,
                number: 0
            },
        ]
    );
    assert_eq!(processors[0].1.core_index, processors[1].1.core_index);
    assert_ne!(processors[0].1.core_index, processors[2].1.core_index);
    assert_eq!(processors[0].1.kind, LogicalProcessorKind::Performance);
    assert_eq!(processors[2].1.kind, LogicalProcessorKind::Efficiency);
}
