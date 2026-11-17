use std::collections::{BTreeMap, BTreeSet};

use crate::{
    cpu_topology::{CpuTopology, ProcessorId},
    platform::windows::cpu_allocation::CpuSetRecord,
};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum EligibilityError {
    #[error("Instance Balancing requires one active processor group (group 0).")]
    UnsupportedGroups,
    #[error("Instance Balancing requires complete CPU Set mappings for the current topology.")]
    InventoryMismatch,
    #[error("Instance Balancing could not verify the original and system affinity masks.")]
    InvalidAffinity,
    #[error("Instance Balancing requires a nonempty selection of current processors.")]
    InvalidSelection,
    #[error("Instance Balancing requires every active SMT sibling of each selected core.")]
    PartialCore,
    #[error("Instance Balancing cannot select CPU Sets allocated by Windows.")]
    Allocated,
    #[error("Instance Balancing cannot select processors outside the original allowed affinity.")]
    OutsideAffinity,
}

fn validate_context(
    topology: &CpuTopology,
    original_affinity: usize,
    system_affinity: usize,
    inventory: &BTreeMap<ProcessorId, CpuSetRecord>,
) -> Result<(), EligibilityError> {
    let [group] = topology.groups() else {
        return Err(EligibilityError::UnsupportedGroups);
    };
    if group.number != 0 {
        return Err(EligibilityError::UnsupportedGroups);
    }
    let active_mask =
        usize::try_from(group.active_mask).map_err(|_| EligibilityError::InvalidAffinity)?;
    if original_affinity == 0
        || system_affinity == 0
        || original_affinity & !active_mask != 0
        || system_affinity & !active_mask != 0
    {
        return Err(EligibilityError::InvalidAffinity);
    }

    let mut ids = BTreeSet::new();
    if inventory.len() != topology.processors().len()
        || topology.processors().iter().any(|processor| {
            inventory
                .get(&processor.id)
                .is_none_or(|record| record.processor() != processor.id || !ids.insert(record.id()))
        })
    {
        return Err(EligibilityError::InventoryMismatch);
    }
    Ok(())
}

pub(super) fn allowed_processors(
    topology: &CpuTopology,
    original_affinity: usize,
    system_affinity: usize,
    inventory: &BTreeMap<ProcessorId, CpuSetRecord>,
) -> Result<BTreeSet<ProcessorId>, EligibilityError> {
    validate_context(topology, original_affinity, system_affinity, inventory)?;
    Ok(inventory
        .iter()
        .filter_map(|(processor, record)| {
            (original_affinity & system_affinity & (1 << processor.number) != 0
                && matches!(record, CpuSetRecord::Unallocated { .. }))
            .then_some(*processor)
        })
        .collect())
}

pub(super) fn validate_selection(
    topology: &CpuTopology,
    selected: &[ProcessorId],
    original_affinity: usize,
    system_affinity: usize,
    inventory: &BTreeMap<ProcessorId, CpuSetRecord>,
) -> Result<(), EligibilityError> {
    validate_context(topology, original_affinity, system_affinity, inventory)?;
    let members = selected.iter().copied().collect::<BTreeSet<_>>();
    if members.is_empty()
        || members.len() != selected.len()
        || members
            .iter()
            .any(|processor| !inventory.contains_key(processor))
    {
        return Err(EligibilityError::InvalidSelection);
    }
    let selected_mask = members
        .iter()
        .fold(0_usize, |mask, processor| mask | (1 << processor.number));
    if selected_mask & !(original_affinity & system_affinity) != 0 {
        return Err(EligibilityError::OutsideAffinity);
    }

    let mut cores = BTreeMap::<ProcessorId, usize>::new();
    for processor in topology.processors() {
        *cores.entry(processor.core).or_default() |= 1 << processor.id.number;
    }
    if cores.values().any(|core| {
        let selected = selected_mask & core;
        selected != 0 && selected != *core
    }) {
        return Err(EligibilityError::PartialCore);
    }
    if members.iter().any(|processor| {
        matches!(
            inventory.get(processor),
            Some(CpuSetRecord::Allocated { .. })
        )
    }) {
        return Err(EligibilityError::Allocated);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu_topology::{LogicalProcessor, ProcessorGroup};

    fn processor(number: u8) -> ProcessorId {
        ProcessorId { group: 0, number }
    }

    fn topology() -> CpuTopology {
        CpuTopology::new(
            (0..4)
                .map(|number| LogicalProcessor {
                    id: processor(number),
                    core: processor(number / 2 * 2),
                    efficiency_class: number / 2,
                })
                .collect(),
            vec![ProcessorGroup {
                number: 0,
                active_mask: 0b1111,
            }],
            Vec::new(),
            Vec::new(),
        )
        .unwrap()
    }

    fn inventory() -> BTreeMap<ProcessorId, CpuSetRecord> {
        (0..4)
            .map(|number| {
                let processor = processor(number);
                (
                    processor,
                    CpuSetRecord::Unallocated {
                        processor,
                        id: 100 + u32::from(number),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn whole_core_share_respects_original_affinity_and_unrelated_reservations() {
        let mut inventory = inventory();
        inventory.insert(
            processor(2),
            CpuSetRecord::Allocated {
                processor: processor(2),
                id: 102,
            },
        );
        assert_eq!(
            validate_selection(
                &topology(),
                &[processor(0), processor(1)],
                3,
                15,
                &inventory
            ),
            Ok(())
        );
        assert_eq!(
            validate_selection(
                &topology(),
                &[processor(0), processor(1)],
                15,
                3,
                &inventory
            ),
            Ok(())
        );
    }

    #[test]
    fn partial_or_disallowed_smt_siblings_are_rejected_without_trimming() {
        let topology = topology();
        let inventory = inventory();
        let full_core = &[processor(0), processor(1)];
        assert_eq!(
            validate_selection(&topology, &[processor(0)], 15, 15, &inventory),
            Err(EligibilityError::PartialCore)
        );
        for (original, system) in [(0b1101, 15), (15, 0b1101)] {
            assert_eq!(
                validate_selection(&topology, full_core, original, system, &inventory),
                Err(EligibilityError::OutsideAffinity)
            );
        }
    }

    #[test]
    fn either_allocated_sibling_rejects_a_requested_whole_core() {
        for number in 0..2 {
            let mut inventory = inventory();
            inventory.insert(
                processor(number),
                CpuSetRecord::Allocated {
                    processor: processor(number),
                    id: 100 + u32::from(number),
                },
            );
            assert_eq!(
                validate_selection(
                    &topology(),
                    &[processor(0), processor(1)],
                    15,
                    15,
                    &inventory,
                ),
                Err(EligibilityError::Allocated)
            );
        }
    }

    #[test]
    fn incomplete_or_inconsistent_inventory_cannot_prove_a_pool() {
        let mut missing = inventory();
        missing.remove(&processor(3));
        let mut extra = inventory();
        extra.insert(
            processor(4),
            CpuSetRecord::Unallocated {
                processor: processor(4),
                id: 104,
            },
        );
        let mut wrong_processor = inventory();
        wrong_processor.insert(
            processor(3),
            CpuSetRecord::Unallocated {
                processor: processor(2),
                id: 103,
            },
        );
        let mut duplicate_id = inventory();
        duplicate_id.insert(
            processor(3),
            CpuSetRecord::Unallocated {
                processor: processor(3),
                id: 100,
            },
        );
        for inventory in [missing, extra, wrong_processor, duplicate_id] {
            assert_eq!(
                validate_selection(
                    &topology(),
                    &[processor(0), processor(1)],
                    15,
                    15,
                    &inventory,
                ),
                Err(EligibilityError::InventoryMismatch)
            );
        }
    }

    #[test]
    fn empty_duplicate_unknown_and_invalid_mask_inputs_are_rejected() {
        for selected in [vec![], vec![processor(0), processor(0)], vec![processor(4)]] {
            assert_eq!(
                validate_selection(&topology(), &selected, 15, 15, &inventory()),
                Err(EligibilityError::InvalidSelection)
            );
        }
        for (original, system) in [(0, 15), (15, 0), (16, 15), (15, 16)] {
            assert_eq!(
                validate_selection(
                    &topology(),
                    &[processor(0), processor(1)],
                    original,
                    system,
                    &inventory(),
                ),
                Err(EligibilityError::InvalidAffinity)
            );
        }
    }

    #[test]
    fn nonzero_and_multiple_groups_are_rejected_even_for_a_group_zero_share() {
        let group_one = ProcessorId {
            group: 1,
            number: 0,
        };
        let cpu = LogicalProcessor {
            id: group_one,
            core: group_one,
            efficiency_class: 0,
        };
        let group = ProcessorGroup {
            number: 1,
            active_mask: 1,
        };
        let nonzero = CpuTopology::new(
            vec![cpu.clone()],
            vec![group.clone()],
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        let mut processors = topology().processors().to_vec();
        processors.push(cpu);
        let multiple = CpuTopology::new(
            processors,
            vec![topology().groups()[0].clone(), group],
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        for topology in [nonzero, multiple] {
            assert_eq!(
                validate_selection(
                    &topology,
                    &[processor(0), processor(1)],
                    15,
                    15,
                    &inventory(),
                ),
                Err(EligibilityError::UnsupportedGroups)
            );
        }
    }
}
