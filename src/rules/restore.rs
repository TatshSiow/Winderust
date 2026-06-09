#![allow(dead_code)]

use std::{collections::HashMap, time::Instant};

use crate::rules::{Action, AppliedAction, ConflictGroup, PreviousValue, ResolvedAction, RuleId};

#[derive(Debug, Default)]
pub struct AppliedActionStore {
    applied: HashMap<ConflictGroup, AppliedAction>,
}

impl AppliedActionStore {
    pub fn record(&mut self, rule_id: RuleId, action: Action, previous: Option<PreviousValue>) {
        let conflict_group = action.conflict_group();
        self.applied.insert(
            conflict_group.clone(),
            AppliedAction {
                rule_id,
                action,
                conflict_group,
                previous,
                applied_at: Instant::now(),
            },
        );
    }

    pub fn record_resolved(&mut self, resolved: &ResolvedAction, previous: Option<PreviousValue>) {
        self.record(resolved.rule_id.clone(), resolved.action.clone(), previous);
    }

    pub fn actions_to_restore(&self, desired: &[ResolvedAction]) -> Vec<AppliedAction> {
        self.applied
            .iter()
            .filter(|(group, applied)| {
                !desired.iter().any(|desired| {
                    &desired.conflict_group == *group && desired.action == applied.action
                })
            })
            .map(|(_, applied)| applied.clone())
            .collect()
    }

    pub fn remove(&mut self, conflict_group: &ConflictGroup) -> Option<AppliedAction> {
        self.applied.remove(conflict_group)
    }

    pub fn get(&self, conflict_group: &ConflictGroup) -> Option<&AppliedAction> {
        self.applied.get(conflict_group)
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&ConflictGroup, &AppliedAction) -> bool) {
        self.applied.retain(|group, action| keep(group, action));
    }

    pub fn len(&self) -> usize {
        self.applied.len()
    }

    pub fn is_empty(&self) -> bool {
        self.applied.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{
        AppMatcher, PriorityResolver, Rule, RuleProcessPriority, Trigger, PRIORITY_BACKGROUND_APP,
    };

    #[test]
    fn records_latest_action_per_conflict_group() {
        let mut store = AppliedActionStore::default();
        store.record(
            RuleId("first".to_owned()),
            Action::SetAppPriority {
                app: AppMatcher::ProcessName("app.exe".to_owned()),
                priority: RuleProcessPriority::BelowNormal,
            },
            Some(PreviousValue::ProcessPriority(RuleProcessPriority::Normal)),
        );
        store.record(
            RuleId("second".to_owned()),
            Action::SetAppPriority {
                app: AppMatcher::ProcessName("APP.EXE".to_owned()),
                priority: RuleProcessPriority::Idle,
            },
            Some(PreviousValue::ProcessPriority(RuleProcessPriority::Normal)),
        );

        assert_eq!(store.len(), 1);
        let applied = store
            .get(&ConflictGroup::AppPriority(crate::rules::ProcessIdentity(
                "app.exe".to_owned(),
            )))
            .expect("applied action");
        assert_eq!(applied.rule_id.0, "second");
    }

    #[test]
    fn actions_to_restore_excludes_matching_desired_actions() {
        let mut store = AppliedActionStore::default();
        let action = Action::SwitchPowerPlan {
            plan_guid: "target-guid".to_owned(),
        };
        store.record(
            RuleId("power".to_owned()),
            action.clone(),
            Some(PreviousValue::PowerPlanGuid("old-guid".to_owned())),
        );
        let rule = Rule {
            id: RuleId("power".to_owned()),
            name: "Power".to_owned(),
            enabled: true,
            priority: PRIORITY_BACKGROUND_APP,
            trigger: Trigger::UserActive,
            actions: vec![action],
            restore_actions: Vec::new(),
            cooldown_secs: 0,
        };
        let desired = PriorityResolver.resolve(&[rule]);

        assert!(store.actions_to_restore(&desired).is_empty());
    }

    #[test]
    fn actions_to_restore_returns_obsolete_actions() {
        let mut store = AppliedActionStore::default();
        store.record(
            RuleId("power".to_owned()),
            Action::SwitchPowerPlan {
                plan_guid: "target-guid".to_owned(),
            },
            Some(PreviousValue::PowerPlanGuid("old-guid".to_owned())),
        );

        let obsolete = store.actions_to_restore(&[]);

        assert_eq!(obsolete.len(), 1);
        assert_eq!(obsolete[0].conflict_group, ConflictGroup::PowerPlan);
    }
}
