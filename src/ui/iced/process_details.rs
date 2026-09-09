use super::super::design;
use super::super::priority_control::{self, Kind, Tier, Value};
use super::super::widgets::{button, checkbox, pick_list, slider};
use super::super::{cpu_allocation, cpu_limiter};
use crate::config::*;
use crate::foreground::executable_path_key;
use crate::power::PowerPlan;
use crate::ui::process_rules::{new_cpu_limiter_rule, process_setting_matches};
use iced::widget::{column, row, text};
use iced::{Element, Fill};
use rust_i18n::t;
#[derive(Debug, Clone)]
pub(in crate::ui::app) enum Message {
    Priority(Kind, Tier, Value),
    ResetPriority(Kind),
    Adaptive(bool),
    Efficiency(bool),
    Power(bool, Option<String>),
    CpuEnabled(bool, bool),
    CpuMask(bool, Tier, u64),
    Limiter(bool),
    LimiterMode(Tier, ProcessRuleMode),
    LimiterLimit(Tier, u8),
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Plan(Option<String>, String);
impl std::fmt::Display for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.1.fmt(f)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mode(ProcessRuleMode);
impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        t!(match self.0 {
            ProcessRuleMode::Default => "common.default",
            ProcessRuleMode::Enabled => "common.custom",
            ProcessRuleMode::Disabled => "cpu_limiter.unlimited",
        })
        .fmt(f)
    }
}
pub(super) fn update(s: &mut Settings, path: &str, m: Message) {
    if !std::path::Path::new(path).is_absolute() {
        return;
    }
    let path = executable_path_key(std::path::Path::new(path));
    match m {
        Message::Priority(kind, tier, value) => {
            if !kind
                .choices(s.advanced.expose_all_priority_values)
                .contains(&value)
            {
                return;
            }
            let rules = kind.rules_mut(s);
            let i = rules
                .iter()
                .position(|r| process_setting_matches(&r.executable_path, &path))
                .unwrap_or_else(|| {
                    rules.push(ProcessExclusionRule {
                        executable_path: path.clone(),
                        ..Default::default()
                    });
                    rules.len() - 1
                });
            rules[i].enabled = true;
            priority_control::Editor::default().update(
                s,
                kind,
                priority_control::Message::RuleValue(i, tier, value),
            );
        }
        Message::ResetPriority(kind) => kind
            .rules_mut(s)
            .retain(|r| !process_setting_matches(&r.executable_path, &path)),
        Message::Adaptive(included) => {
            let rules = &mut s.cpu_scheduler.custom_rules;
            if included {
                rules.retain(|r| !process_setting_matches(&r.executable_path, &path));
            } else if let Some(r) = rules
                .iter_mut()
                .find(|r| process_setting_matches(&r.executable_path, &path))
            {
                r.enabled = true;
            } else {
                rules.push(ProcessExclusionRule {
                    executable_path: path,
                    ..Default::default()
                });
            }
        }
        Message::Efficiency(included) => {
            s.background_efficiency
                .custom_rules
                .retain(|r| !process_setting_matches(&r.executable_path, &path));
            if !included {
                s.background_efficiency
                    .custom_rules
                    .push(BackgroundEfficiencyRule {
                        enabled: true,
                        executable_path: path,
                        focus_efficiency_mode: ProcessRuleMode::Disabled,
                        visible_window_efficiency_mode: ProcessRuleMode::Disabled,
                        background_efficiency_mode: ProcessRuleMode::Disabled,
                    });
            }
        }
        Message::Power(foreground, guid) => {
            if foreground {
                let rules = &mut s.by_foreground.rules;
                if let Some(guid) = guid {
                    if let Some(r) = rules
                        .iter_mut()
                        .find(|r| process_setting_matches(&r.executable_path, &path))
                    {
                        r.enabled = true;
                        r.power_plan_guid = Some(guid);
                    } else {
                        rules.push(crate::ui::process_rules::new_foreground_rule(
                            &path,
                            Some(guid),
                        ));
                    }
                } else {
                    rules.retain(|r| !process_setting_matches(&r.executable_path, &path));
                }
            } else {
                let rules = &mut s.by_running_app.rules;
                if let Some(guid) = guid {
                    if let Some(r) = rules
                        .iter_mut()
                        .find(|r| process_setting_matches(&r.executable_path, &path))
                    {
                        r.enabled = true;
                        r.power_plan_guid = Some(guid);
                    } else {
                        rules.push(crate::ui::process_rules::new_by_running_app_rule(
                            &path,
                            Some(guid),
                        ));
                    }
                } else {
                    rules.retain(|r| !process_setting_matches(&r.executable_path, &path));
                }
            }
        }
        Message::CpuEnabled(soft, enabled) => {
            let other = if soft {
                &s.processor_affinity_hard
            } else {
                &s.cpu_sets_soft
            };
            if enabled && other.contains_rule_for(&path) {
                return;
            }
            let setting = if soft {
                &mut s.cpu_sets_soft
            } else {
                &mut s.processor_affinity_hard
            };
            if !enabled {
                setting
                    .rules
                    .retain(|r| !process_setting_matches(&r.executable_path, &path));
            } else if !setting.contains_rule_for(&path)
                && !crate::cpu_allocation::is_builtin_excluded(&path)
            {
                let all = crate::cpu_allocation::logical_processor_mask(
                    &crate::cpu_allocation::logical_processors(),
                );
                setting.rules.push(CpuAllocationRule {
                    enabled: true,
                    executable_path: path,
                    focus_core_mask: all,
                    visible_window_core_mask: all,
                    background_core_mask: all,
                });
            }
        }
        Message::CpuMask(soft, tier, mask) => {
            let setting = if soft {
                &mut s.cpu_sets_soft
            } else {
                &mut s.processor_affinity_hard
            };
            if let Some(r) = setting
                .rules
                .iter_mut()
                .find(|r| process_setting_matches(&r.executable_path, &path))
            {
                let mask = mask
                    & crate::cpu_allocation::logical_processor_mask(
                        &crate::cpu_allocation::logical_processors(),
                    );
                match tier {
                    Tier::Focus => r.focus_core_mask = mask,
                    Tier::VisibleWindow => r.visible_window_core_mask = mask,
                    Tier::Background => r.background_core_mask = mask,
                }
            }
        }
        Message::Limiter(enabled) => {
            if !enabled {
                s.cpu_limiter
                    .rules
                    .retain(|r| !process_setting_matches(&r.executable_path, &path));
            } else if !s
                .cpu_limiter
                .rules
                .iter()
                .any(|r| process_setting_matches(&r.executable_path, &path))
                && !crate::cpu_limiter::is_builtin_excluded(&path)
            {
                s.cpu_limiter.rules.push(new_cpu_limiter_rule(&path));
            }
        }
        Message::LimiterMode(tier, value) => {
            if let Some(i) = s
                .cpu_limiter
                .rules
                .iter()
                .position(|r| process_setting_matches(&r.executable_path, &path))
            {
                cpu_limiter::CpuLimiter::default().update(
                    &mut s.cpu_limiter,
                    cpu_limiter::Message::RuleMode(i, limiter_tier(tier), value),
                );
            }
        }
        Message::LimiterLimit(tier, value) => {
            if let Some(i) = s
                .cpu_limiter
                .rules
                .iter()
                .position(|r| process_setting_matches(&r.executable_path, &path))
            {
                cpu_limiter::CpuLimiter::default().update(
                    &mut s.cpu_limiter,
                    cpu_limiter::Message::RuleLimit(i, limiter_tier(tier), value),
                );
            }
        }
    }
}
fn limiter_tier(tier: Tier) -> cpu_limiter::Tier {
    match tier {
        Tier::Focus => cpu_limiter::Tier::Focus,
        Tier::VisibleWindow => cpu_limiter::Tier::VisibleWindow,
        Tier::Background => cpu_limiter::Tier::Background,
    }
}
pub(super) fn view<'a>(
    s: &'a Settings,
    path: &'a str,
    plans: &[PowerPlan],
) -> Element<'a, Message> {
    let adaptive = !s
        .cpu_scheduler
        .custom_rules
        .iter()
        .any(|r| r.enabled && process_setting_matches(&r.executable_path, path));
    let efficiency = s
        .background_efficiency
        .custom_rules
        .iter()
        .find(|r| r.enabled && process_setting_matches(&r.executable_path, path))
        .is_none_or(|r| {
            s.background_efficiency
                .custom_rule_applies_efficiency_mode(r)
        });
    let mut body = column![
        text(t!("process_list.open_rule_details").to_string()).size(design::typography::SUBTITLE),
        checkbox(adaptive)
            .label(t!("nav.adaptive_engine").to_string())
            .on_toggle(Message::Adaptive),
        checkbox(efficiency)
            .label(t!("nav.background_efficiency").to_string())
            .on_toggle(Message::Efficiency)
    ]
    .spacing(design::space::MEDIUM);
    for foreground in [true, false] {
        let selected = if foreground {
            s.by_foreground
                .rules
                .iter()
                .find(|r| r.enabled && process_setting_matches(&r.executable_path, path))
                .and_then(|r| r.power_plan_guid.clone())
        } else {
            s.by_running_app
                .rules
                .iter()
                .find(|r| r.enabled && process_setting_matches(&r.executable_path, path))
                .and_then(|r| r.power_plan_guid.clone())
        };
        let mut options = vec![Plan(None, t!("common.default").to_string())];
        options.extend(
            plans
                .iter()
                .map(|p| Plan(Some(p.guid.clone()), p.name.clone())),
        );
        if let Some(guid) = &selected {
            if !options.iter().any(|p| p.0.as_ref() == Some(guid)) {
                options.push(Plan(
                    selected.clone(),
                    t!("common.selected_plan_unavailable").to_string(),
                ));
            }
        }
        let selected = options.iter().find(|p| p.0 == selected).cloned();
        body = body
            .push(text(
                t!(if foreground {
                    "process_list.power_plan_foreground"
                } else {
                    "process_list.power_plan_running"
                })
                .to_string(),
            ))
            .push(pick_list(options, selected, move |p| {
                Message::Power(foreground, p.0)
            }));
    }
    for kind in [
        Kind::Process,
        Kind::Thread,
        Kind::Io,
        Kind::Gpu,
        Kind::Memory,
        Kind::DynamicBoost,
    ] {
        let key = format!("nav.{}", kind.key());
        body = body.push(row![
            text(t!(&key).to_string()).width(Fill),
            button(text(t!("common.default").to_string())).on_press(Message::ResetPriority(kind))
        ]);
        let rule = kind
            .rules(s)
            .iter()
            .find(|r| process_setting_matches(&r.executable_path, path))
            .cloned()
            .unwrap_or_default();
        for tier in Tier::ALL {
            body = body.push(
                row![
                    text(tier.label()).width(Fill),
                    pick_list(
                        kind.choices(s.advanced.expose_all_priority_values),
                        Some(kind.rule_value(&rule, tier)),
                        move |v| Message::Priority(kind, tier, v)
                    )
                ]
                .spacing(design::space::SMALL),
            );
        }
    }
    if crate::cpu_allocation::has_multiple_processor_groups() {
        body = body.push(text(t!("cpu_sets_soft.warning").to_string()));
    }
    body = body.push(text(t!("processor_affinity_hard.warning").to_string()));
    for soft in [true, false] {
        let settings = if soft {
            &s.cpu_sets_soft
        } else {
            &s.processor_affinity_hard
        };
        let rule = settings
            .rules
            .iter()
            .find(|r| process_setting_matches(&r.executable_path, path));
        body = body.push(
            checkbox(rule.is_some())
                .label(
                    t!(if soft {
                        "nav.cpu_sets_soft"
                    } else {
                        "nav.processor_affinity_hard"
                    })
                    .to_string(),
                )
                .on_toggle(move |v| Message::CpuEnabled(soft, v)),
        );
        if let Some(rule) = rule {
            for tier in Tier::ALL {
                let mask = match tier {
                    Tier::Focus => rule.focus_core_mask,
                    Tier::VisibleWindow => rule.visible_window_core_mask,
                    Tier::Background => rule.background_core_mask,
                };
                body = body
                    .push(text(tier.label()))
                    .push(cpu_allocation::mask_selector(
                        mask,
                        &crate::cpu_allocation::logical_processors(),
                        &s.cpu_allocation_presets,
                        move |v| Message::CpuMask(soft, tier, v),
                    ));
            }
        }
    }
    let limiter = s
        .cpu_limiter
        .rules
        .iter()
        .find(|r| process_setting_matches(&r.executable_path, path));
    body = body.push(
        checkbox(limiter.is_some())
            .label(t!("nav.cpu_limiter").to_string())
            .on_toggle(Message::Limiter),
    );
    if let Some(rule) = limiter {
        for tier in Tier::ALL {
            let (mode, limit) = match tier {
                Tier::Focus => (rule.focus_mode, rule.focus_allowed_cpu_time_percent),
                Tier::VisibleWindow => (
                    rule.visible_window_mode,
                    rule.visible_window_allowed_cpu_time_percent,
                ),
                Tier::Background => (
                    rule.background_mode,
                    rule.background_allowed_cpu_time_percent,
                ),
            };
            body = body.push(row![
                text(tier.label()).width(Fill),
                pick_list(ProcessRuleMode::ALL.map(Mode), Some(Mode(mode)), move |m| {
                    Message::LimiterMode(tier, m.0)
                })
            ]);
            if mode == ProcessRuleMode::Enabled {
                body = body.push(row![
                    slider(1..=100, limit, move |v| Message::LimiterLimit(tier, v)),
                    text(format!("{limit}%"))
                ]);
            }
        }
    }
    body.into()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn details_priority_edits_do_not_enable_master_or_other_tiers() {
        let mut s = Settings::default();
        update(
            &mut s,
            r"C:\App\app.exe",
            Message::Priority(
                Kind::Process,
                Tier::Focus,
                Value::Process(ProcessPrioritySetting::AboveNormal),
            ),
        );
        assert!(!s.process_priority.enabled);
        let r = &s.process_priority.exclusions[0];
        assert_eq!(
            r.process_priority_override(true, false),
            ProcessPrioritySetting::AboveNormal
        );
        assert_eq!(
            r.process_priority_override(false, false),
            ProcessPrioritySetting::Default
        );
    }
    #[test]
    fn cpu_rule_cannot_join_both_owners() {
        let mut s = Settings::default();
        s.cpu_sets_soft.rules.push(CpuAllocationRule {
            enabled: true,
            executable_path: r"C:\App\app.exe".into(),
            focus_core_mask: 1,
            visible_window_core_mask: 1,
            background_core_mask: 1,
        });
        update(&mut s, r"C:\App\app.exe", Message::CpuEnabled(false, true));
        assert!(s.processor_affinity_hard.rules.is_empty());
    }
}
