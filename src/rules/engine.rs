#![allow(dead_code)]

use crate::rules::{AppliedAction, AppliedActionStore, PriorityResolver, ResolvedAction, Rule};

#[derive(Debug, Clone, PartialEq)]
pub struct EngineEvaluation {
    pub desired_actions: Vec<ResolvedAction>,
    pub actions_to_restore: Vec<AppliedAction>,
}

#[derive(Debug, Default)]
pub struct RuleEngine {
    resolver: PriorityResolver,
}

impl RuleEngine {
    pub fn evaluate_active_rules(
        &self,
        active_rules: &[Rule],
        applied_actions: &AppliedActionStore,
    ) -> EngineEvaluation {
        let desired_actions = self.resolver.resolve(active_rules);
        let actions_to_restore = applied_actions.actions_to_restore(&desired_actions);

        EngineEvaluation {
            desired_actions,
            actions_to_restore,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{
        Action, AppMatcher, PreviousValue, RuleId, RuleProcessPriority, Trigger,
        PRIORITY_BACKGROUND_APP, PRIORITY_FOCUSED_APP,
    };

    #[test]
    fn engine_resolves_active_rules() {
        let rules = vec![
            Rule {
                id: RuleId("background".to_owned()),
                name: "Background".to_owned(),
                enabled: true,
                priority: PRIORITY_BACKGROUND_APP,
                trigger: Trigger::AppBackground {
                    app: AppMatcher::ProcessName("app.exe".to_owned()),
                    duration_secs: 10,
                },
                actions: vec![Action::SetAppPriority {
                    app: AppMatcher::ProcessName("app.exe".to_owned()),
                    priority: RuleProcessPriority::BelowNormal,
                }],
                restore_actions: Vec::new(),
                cooldown_secs: 0,
            },
            Rule {
                id: RuleId("focused".to_owned()),
                name: "Focused".to_owned(),
                enabled: true,
                priority: PRIORITY_FOCUSED_APP,
                trigger: Trigger::AppFocused {
                    app: AppMatcher::ProcessName("app.exe".to_owned()),
                },
                actions: vec![Action::SetAppPriority {
                    app: AppMatcher::ProcessName("app.exe".to_owned()),
                    priority: RuleProcessPriority::Normal,
                }],
                restore_actions: Vec::new(),
                cooldown_secs: 0,
            },
        ];

        let evaluation =
            RuleEngine::default().evaluate_active_rules(&rules, &AppliedActionStore::default());

        assert_eq!(evaluation.desired_actions.len(), 1);
        assert_eq!(evaluation.desired_actions[0].rule_id.0, "focused");
        assert!(evaluation.actions_to_restore.is_empty());
    }

    #[test]
    fn engine_returns_obsolete_applied_actions() {
        let mut store = AppliedActionStore::default();
        store.record(
            RuleId("old".to_owned()),
            Action::SetAppPriority {
                app: AppMatcher::ProcessName("app.exe".to_owned()),
                priority: RuleProcessPriority::BelowNormal,
            },
            Some(PreviousValue::ProcessPriority(RuleProcessPriority::Normal)),
        );

        let evaluation = RuleEngine::default().evaluate_active_rules(&[], &store);

        assert_eq!(evaluation.desired_actions.len(), 0);
        assert_eq!(evaluation.actions_to_restore.len(), 1);
    }
}
