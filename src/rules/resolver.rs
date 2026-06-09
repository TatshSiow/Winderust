#![allow(dead_code)]

use crate::rules::model::{Action, ConflictGroup, Rule, RuleId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAction {
    pub rule_id: RuleId,
    pub rule_name: String,
    pub priority: i32,
    pub action: Action,
    pub conflict_group: ConflictGroup,
}

#[derive(Debug, Default)]
pub struct PriorityResolver;

impl PriorityResolver {
    pub fn resolve(&self, rules: &[Rule]) -> Vec<ResolvedAction> {
        let mut resolved = Vec::<ResolvedAction>::new();

        for rule in rules.iter().filter(|rule| rule.enabled) {
            for action in &rule.actions {
                let candidate = ResolvedAction {
                    rule_id: rule.id.clone(),
                    rule_name: rule.name.clone(),
                    priority: rule.priority,
                    action: action.clone(),
                    conflict_group: action.conflict_group(),
                };

                match resolved
                    .iter_mut()
                    .find(|existing| existing.conflict_group == candidate.conflict_group)
                {
                    Some(existing) if candidate.priority > existing.priority => {
                        *existing = candidate;
                    }
                    Some(_) => {}
                    None => resolved.push(candidate),
                }
            }
        }

        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::model::{
        AppMatcher, RuleProcessPriority, Trigger, PRIORITY_BACKGROUND_APP, PRIORITY_FOCUSED_APP,
        PRIORITY_RUNNING_APP,
    };

    #[test]
    fn higher_priority_rule_wins_same_conflict_group() {
        let background_rule = Rule::new(
            "background",
            "Background editor",
            Trigger::AppBackground {
                app: AppMatcher::ProcessName("editor.exe".to_owned()),
                duration_secs: 10,
            },
            vec![Action::SetAppPriority {
                app: AppMatcher::ProcessName("editor.exe".to_owned()),
                priority: RuleProcessPriority::BelowNormal,
            }],
        );
        let focused_rule = Rule::new(
            "focused",
            "Focused editor",
            Trigger::AppFocused {
                app: AppMatcher::ProcessName("editor.exe".to_owned()),
            },
            vec![Action::SetAppPriority {
                app: AppMatcher::ProcessName("EDITOR.EXE".to_owned()),
                priority: RuleProcessPriority::Normal,
            }],
        );

        let resolved = PriorityResolver.resolve(&[background_rule, focused_rule]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].rule_id.0, "focused");
        assert_eq!(resolved[0].priority, PRIORITY_FOCUSED_APP);
    }

    #[test]
    fn different_conflict_groups_are_kept() {
        let rule = Rule::new(
            "focused",
            "Focused editor",
            Trigger::AppFocused {
                app: AppMatcher::ProcessName("editor.exe".to_owned()),
            },
            vec![
                Action::SwitchPowerPlan {
                    plan_guid: "performance-guid".to_owned(),
                },
                Action::SetAppPriority {
                    app: AppMatcher::ProcessName("editor.exe".to_owned()),
                    priority: RuleProcessPriority::AboveNormal,
                },
            ],
        );

        let resolved = PriorityResolver.resolve(&[rule]);

        assert_eq!(resolved.len(), 2);
        assert!(resolved
            .iter()
            .any(|action| action.conflict_group == ConflictGroup::PowerPlan));
        assert!(resolved
            .iter()
            .any(|action| matches!(action.conflict_group, ConflictGroup::AppPriority(_))));
    }

    #[test]
    fn disabled_rules_are_ignored() {
        let mut disabled = Rule::new(
            "disabled",
            "Disabled runner",
            Trigger::AppRunning {
                app: AppMatcher::ProcessName("runner.exe".to_owned()),
            },
            vec![Action::SwitchPowerPlan {
                plan_guid: "performance-guid".to_owned(),
            }],
        );
        disabled.enabled = false;

        assert!(PriorityResolver.resolve(&[disabled]).is_empty());
    }

    #[test]
    fn equal_priority_keeps_first_configured_rule() {
        let first = Rule {
            id: RuleId("first".to_owned()),
            name: "First".to_owned(),
            enabled: true,
            priority: PRIORITY_RUNNING_APP,
            trigger: Trigger::AppRunning {
                app: AppMatcher::ProcessName("tool.exe".to_owned()),
            },
            actions: vec![Action::SwitchPowerPlan {
                plan_guid: "first-guid".to_owned(),
            }],
            restore_actions: Vec::new(),
            cooldown_secs: 0,
        };
        let second = Rule {
            id: RuleId("second".to_owned()),
            name: "Second".to_owned(),
            enabled: true,
            priority: PRIORITY_RUNNING_APP,
            trigger: Trigger::AppRunning {
                app: AppMatcher::ProcessName("tool.exe".to_owned()),
            },
            actions: vec![Action::SwitchPowerPlan {
                plan_guid: "second-guid".to_owned(),
            }],
            restore_actions: Vec::new(),
            cooldown_secs: 0,
        };

        let resolved = PriorityResolver.resolve(&[first, second]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].rule_id.0, "first");
        assert_eq!(resolved[0].priority, PRIORITY_RUNNING_APP);
    }

    #[test]
    fn explicit_priority_can_override_trigger_default() {
        let mut background_rule = Rule::new(
            "background",
            "Background editor",
            Trigger::AppBackground {
                app: AppMatcher::ProcessName("editor.exe".to_owned()),
                duration_secs: 10,
            },
            vec![Action::SetAppPriority {
                app: AppMatcher::ProcessName("editor.exe".to_owned()),
                priority: RuleProcessPriority::BelowNormal,
            }],
        );
        background_rule.priority = PRIORITY_FOCUSED_APP + 1;
        let focused_rule = Rule::new(
            "focused",
            "Focused editor",
            Trigger::AppFocused {
                app: AppMatcher::ProcessName("editor.exe".to_owned()),
            },
            vec![Action::SetAppPriority {
                app: AppMatcher::ProcessName("editor.exe".to_owned()),
                priority: RuleProcessPriority::Normal,
            }],
        );

        let resolved = PriorityResolver.resolve(&[background_rule, focused_rule]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].rule_id.0, "background");
        assert!(resolved[0].priority > PRIORITY_BACKGROUND_APP);
    }
}
