use super::*;

fn cpu(number: u8, core: u8, efficiency_class: u8) -> LogicalProcessor {
    LogicalProcessor {
        id: ProcessorId { group: 0, number },
        core: ProcessorId {
            group: 0,
            number: core,
        },
        efficiency_class,
    }
}

#[test]
fn useful_work_matches_existing_throughput_kernel() {
    assert_eq!(block(1), 3_937_720_911_837_777_011);
    assert!(check_kernel().is_ok());
}

#[test]
fn half_pool_keeps_siblings_and_both_efficiency_classes_within_baseline() {
    let processors = [
        cpu(0, 0, 8),
        cpu(1, 0, 8),
        cpu(2, 2, 8),
        cpu(3, 2, 8),
        cpu(4, 4, 0),
        cpu(5, 5, 0),
        cpu(6, 6, 0),
        cpu(7, 7, 0),
    ];
    assert_eq!(half_pool(&processors, 0xff), Ok(0x33));
    // An inherited exclusion of one SMT sibling must never be widened.
    assert_eq!(half_pool(&processors, 0xfe), Ok(0x32));
    assert!(half_pool(&processors, 0x1ff).is_err());
}

#[test]
fn half_pool_rejects_empty_or_non_reducible_baselines() {
    let processors = [cpu(0, 0, 0), cpu(1, 0, 0)];
    assert!(half_pool(&processors, 0).is_err());
    assert!(half_pool(&processors, 3).is_err());
    assert!(half_pool(
        &[LogicalProcessor {
            id: ProcessorId {
                group: 1,
                number: 0
            },
            core: ProcessorId {
                group: 1,
                number: 0
            },
            efficiency_class: 0,
        }],
        1
    )
    .is_err());
}

fn snapshot(blocks: Vec<u64>) -> Snapshot {
    Snapshot {
        sequence: 1,
        elapsed_ns: 1,
        capture_span_ns: 0,
        cpu_100ns: 1,
        checksums: vec![1; blocks.len()],
        blocks,
    }
}

#[test]
fn work_measurements_reject_regression_overflow_and_invalid_workers() {
    assert_eq!(
        checked_work(&snapshot(vec![7, 5]), &snapshot(vec![12, 8]), 2),
        Ok(8)
    );
    assert!(checked_work(&snapshot(vec![7]), &snapshot(vec![6]), 1).is_err());
    assert!(checked_work(&snapshot(vec![7]), &snapshot(vec![7]), 1).is_err());
    assert!(checked_work(&snapshot(vec![0, 0]), &snapshot(vec![u64::MAX, 1]), 2).is_err());
    assert!(checked_work(&snapshot(vec![0]), &snapshot(vec![1]), 2).is_err());
    let mut invalid = snapshot(vec![1]);
    invalid.checksums[0] = 0;
    assert!(checked_work(&snapshot(vec![0]), &invalid, 1).is_err());
}

#[test]
fn response_tail_uses_nearest_rank_without_discarding_worst_sample() {
    let sorted = [0.1, 0.3, 4.0, 51.0];
    assert_eq!(percentile(&sorted, 50), Ok(0.3));
    assert_eq!(percentile(&sorted, 95), Ok(51.0));
    assert!(percentile(&[], 95).is_err());
}
