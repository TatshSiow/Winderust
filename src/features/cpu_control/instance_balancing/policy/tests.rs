use super::*;
use crate::cpu_topology::test_topology;

fn cohort(ids: &[u32]) -> Cohort {
    Cohort::new(
        ids.iter()
            .map(|id| {
                (
                    ProcessControlTarget::automatic(
                        *id,
                        "app.exe".into(),
                        "C:/Apps/app.exe".into(),
                        u64::from(*id),
                    ),
                    (0..4)
                        .map(|number| ProcessorId { group: 0, number })
                        .collect(),
                )
            })
            .collect(),
    )
}

#[test]
fn stable_cohorts_qualify_and_resubmissions_do_not_extend_the_hold() {
    let topology = test_topology();
    let start = Instant::now();
    let mut policy = Policy::default();
    let first = cohort(&[2, 1]);
    assert_eq!(policy.observe(&topology, &first, start), Ok(None));
    assert_eq!(
        policy.observe(&topology, &first, start + Duration::from_millis(999)),
        Ok(None)
    );
    let accepted = policy
        .observe(&topology, &first, start + Duration::from_secs(1))
        .unwrap()
        .unwrap();
    policy.accepted(accepted.clone(), start + Duration::from_secs(1));
    let resubmitted = policy
        .observe(&topology, &cohort(&[1, 2]), start + Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(resubmitted, accepted);
    policy.accepted(resubmitted, start + Duration::from_secs(2));
    let added = cohort(&[1, 2, 3]);
    for elapsed in [3, 4, 5] {
        assert_eq!(
            policy.observe(&topology, &added, start + Duration::from_secs(elapsed)),
            Ok(None)
        );
    }
    assert!(policy
        .observe(&topology, &added, start + Duration::from_secs(6))
        .unwrap()
        .is_some());
}

#[test]
fn changed_per_member_facts_restart_even_when_the_common_pool_is_unchanged() {
    let topology = test_topology();
    let start = Instant::now();
    let mut policy = Policy::default();
    let mut first = cohort(&[1, 2]);
    first.members[1].1.remove(&ProcessorId {
        group: 0,
        number: 3,
    });
    policy.observe(&topology, &first, start).unwrap();
    let accepted = policy
        .observe(&topology, &first, start + Duration::from_secs(1))
        .unwrap()
        .unwrap();
    policy.accepted(accepted, start + Duration::from_secs(1));
    let mut changed = first.clone();
    changed.members[0].1.remove(&ProcessorId {
        group: 0,
        number: 3,
    });
    assert_eq!(
        policy.observe(&topology, &changed, start + Duration::from_secs(6)),
        Ok(None)
    );
    assert_eq!(
        policy.observe(&topology, &first, start + Duration::from_secs(7)),
        Ok(None)
    );
    assert!(policy
        .observe(&topology, &first, start + Duration::from_secs(8))
        .unwrap()
        .is_some());
}

#[test]
fn failed_application_breaks_qualification_without_starting_a_plan_hold() {
    let topology = test_topology();
    let start = Instant::now();
    let mut policy = Policy::default();
    let first = cohort(&[1, 2]);
    policy.observe(&topology, &first, start).unwrap();
    assert!(policy
        .observe(&topology, &first, start + Duration::from_secs(1))
        .unwrap()
        .is_some());
    policy.invalidate();
    assert_eq!(
        policy.observe(&topology, &first, start + Duration::from_secs(2)),
        Ok(None)
    );
    let accepted = policy
        .observe(&topology, &first, start + Duration::from_secs(3))
        .unwrap()
        .unwrap();
    policy.accepted(accepted.clone(), start + Duration::from_secs(3));
    policy.invalidate();
    for elapsed in [4, 5, 7] {
        assert_eq!(
            policy.observe(&topology, &first, start + Duration::from_secs(elapsed)),
            Ok(None)
        );
    }
    assert_eq!(
        policy
            .observe(&topology, &first, start + Duration::from_secs(8))
            .unwrap(),
        Some(accepted)
    );
}

#[test]
fn topology_changes_require_explicit_rearm_even_after_the_layout_returns() {
    let topology = test_topology();
    let mut processors = topology.processors().to_vec();
    processors[0].efficiency_class = 1;
    let changed = CpuTopology::new(processors, topology.groups().to_vec(), vec![], vec![]).unwrap();
    let start = Instant::now();
    let first = cohort(&[1, 2]);
    let mut policy = Policy::default();
    policy.observe(&topology, &first, start).unwrap();
    assert_eq!(
        policy.observe(&changed, &first, start + Duration::from_secs(1)),
        Err(PlanError::TopologyChanged)
    );
    policy.invalidate();
    assert_eq!(
        policy.observe(&topology, &first, start + Duration::from_secs(2)),
        Err(PlanError::TopologyChanged)
    );
    policy = Policy::default();
    assert_eq!(
        policy.observe(&topology, &first, start + Duration::from_secs(3)),
        Ok(None)
    );
    assert!(policy
        .observe(&topology, &first, start + Duration::from_secs(4))
        .unwrap()
        .is_some());
}

#[test]
fn lost_capacity_and_single_members_yield_and_discard_previous_qualification() {
    let topology = test_topology();
    let start = Instant::now();
    let mut policy = Policy::default();
    let first = cohort(&[1, 2]);
    policy.observe(&topology, &first, start).unwrap();
    let accepted = policy
        .observe(&topology, &first, start + Duration::from_secs(1))
        .unwrap()
        .unwrap();
    policy.accepted(accepted, start + Duration::from_secs(1));
    assert_eq!(
        policy.observe(&topology, &cohort(&[1]), start + Duration::from_secs(6)),
        Err(PlanError::WaitingForInstances)
    );
    assert_eq!(
        policy.observe(&topology, &first, start + Duration::from_secs(7)),
        Ok(None)
    );
    let mut unavailable = first.clone();
    unavailable.members[0].1.clear();
    assert_eq!(
        policy.observe(&topology, &unavailable, start + Duration::from_secs(8)),
        Err(PlanError::InsufficientCapacity)
    );
    assert_eq!(
        policy.observe(&topology, &first, start + Duration::from_secs(9)),
        Ok(None)
    );
    assert!(policy
        .observe(&topology, &first, start + Duration::from_secs(10))
        .unwrap()
        .is_some());
}
