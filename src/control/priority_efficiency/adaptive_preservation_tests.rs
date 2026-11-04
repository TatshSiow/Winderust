use super::*;

#[test]
fn adaptive_preservation_keeps_direction_and_high_priority_protection() {
    for policy in [
        PriorityClassPreservation::PreserveHighOrRealtime,
        PriorityClassPreservation::PreserveHigherOrHighOrRealtime,
        PriorityClassPreservation::PreserveLowerOrHighOrRealtime,
    ] {
        for baseline in [PriorityClassValue::High, PriorityClassValue::Realtime] {
            assert!(priority_is_preserved(
                policy,
                baseline.raw(),
                PriorityClassValue::Realtime.raw()
            ));
        }
    }
    for (policy, kept, changed) in [
        (
            PriorityClassPreservation::PreserveHigherOrHighOrRealtime,
            PriorityClassValue::AboveNormal,
            PriorityClassValue::BelowNormal,
        ),
        (
            PriorityClassPreservation::PreserveLowerOrHighOrRealtime,
            PriorityClassValue::BelowNormal,
            PriorityClassValue::AboveNormal,
        ),
    ] {
        assert!(priority_is_preserved(
            policy,
            kept.raw(),
            PriorityClassValue::Normal.raw()
        ));
        assert!(priority_is_preserved(
            policy,
            PriorityClassValue::Normal.raw(),
            PriorityClassValue::Normal.raw()
        ));
        assert!(!priority_is_preserved(
            policy,
            changed.raw(),
            PriorityClassValue::Normal.raw()
        ));
    }
}
