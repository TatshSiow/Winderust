use super::super::cpu_allocation::{
    self,
    Kind::{Hard, Soft},
    Tier as AllocationTier,
};
use super::super::cpu_limiter;
use super::super::design;
use super::super::priority_control::{Kind, Tier, Value};
use super::super::widgets::{self, action_button, pick_list, setting_row, settings_card, switch};
use super::PlanCatalog;
use crate::application::SettingsEditor;
use crate::config::*;
use crate::foreground::executable_path_key;
use crate::ui::process_rules::{
    new_by_focus_rule, new_by_running_app_rule, new_cpu_limiter_rule, process_setting_matches,
    selector_path_matches,
};
use iced::widget::{column, container, row, text};
use iced::{Element, Fill};
use rust_i18n::t;
#[derive(Debug, Clone)]
pub(in crate::ui) enum Message {
    ReloadPlans,
    Priority(Kind, Tier, Value),
    ResetPriority(Kind),
    Adaptive(bool),
    Efficiency(bool),
    Power(bool, Option<String>),
    CpuEnabled(bool, bool),
    CpuMask(bool, Tier, crate::cpu_topology::CpuSelection),
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
pub(super) fn update(s: &mut SettingsEditor, path: &str, m: Message) -> Option<String> {
    if !std::path::Path::new(path).is_absolute() {
        return None;
    }
    let path = executable_path_key(std::path::Path::new(path));
    match m {
        Message::ReloadPlans => unreachable!("The application owns power-plan loading"),
        Message::Priority(kind, tier, value) => {
            if !kind
                .choices(s.advanced.expose_all_priority_values)
                .contains(&value)
            {
                return None;
            }
            let existing = kind
                .rules(s)
                .iter()
                .enumerate()
                .find(|(_, rule)| process_setting_matches(&rule.executable_path, &path));
            let mut rule = existing.map_or_else(
                || ProcessExclusionRule {
                    executable_path: path,
                    ..Default::default()
                },
                |(_, rule)| rule.clone(),
            );
            rule.enabled = true;
            value.set_rule(&mut rule, tier);
            match existing {
                Some((index, original)) if rule != *original => kind.rules_mut(s)[index] = rule,
                None => kind.rules_mut(s).push(rule),
                Some(_) => {}
            }
        }
        Message::ResetPriority(kind) => {
            let matches_path =
                |rule: &ProcessExclusionRule| process_setting_matches(&rule.executable_path, &path);
            if kind.rules(s).iter().any(matches_path) {
                kind.rules_mut(s).retain(|rule| !matches_path(rule));
            }
        }
        Message::Adaptive(included) => {
            let matches_path =
                |rule: &ProcessExclusionRule| process_setting_matches(&rule.executable_path, &path);
            if included
                && !s
                    .adaptive_engine_process
                    .custom_rules
                    .iter()
                    .any(matches_path)
            {
                return None;
            }
            let rules = &mut s.adaptive_engine_process.custom_rules;
            if included {
                rules.retain(|rule| !matches_path(rule));
            } else if let Some(rule) = rules.iter_mut().find(|rule| matches_path(rule)) {
                rule.enabled = true;
            } else {
                rules.push(ProcessExclusionRule {
                    executable_path: path,
                    ..Default::default()
                });
            }
        }
        Message::Efficiency(included) => {
            let matches_path = |rule: &BackgroundEfficiencyRule| {
                process_setting_matches(&rule.executable_path, &path)
            };
            if included
                && !s
                    .background_efficiency
                    .custom_rules
                    .iter()
                    .any(matches_path)
            {
                return None;
            }
            let rules = &mut s.background_efficiency.custom_rules;
            rules.retain(|rule| !matches_path(rule));
            if !included {
                rules.push(BackgroundEfficiencyRule {
                    enabled: true,
                    executable_path: path,
                    focus_efficiency_mode: ProcessRuleMode::Disabled,
                    visible_window_efficiency_mode: ProcessRuleMode::Disabled,
                    background_efficiency_mode: ProcessRuleMode::Disabled,
                });
            }
        }
        Message::Power(focus, guid) => {
            macro_rules! update_path_plan {
                ($field:ident, $new_rule:ident) => {{
                    let index = s
                        .$field
                        .rules
                        .iter()
                        .position(|rule| selector_path_matches(&rule.selector, &path));
                    match (index, guid) {
                        (None, None) => {}
                        (Some(_), None) => s
                            .$field
                            .rules
                            .retain(|rule| !selector_path_matches(&rule.selector, &path)),
                        (None, Some(guid)) => {
                            s.$field.rules.push($new_rule(&path, Some(guid)));
                        }
                        (Some(index), Some(guid)) => {
                            let rule = &s.$field.rules[index];
                            if !rule.enabled || rule.power_plan_guid.as_ref() != Some(&guid) {
                                let rule = &mut s.$field.rules[index];
                                rule.enabled = true;
                                rule.power_plan_guid = Some(guid);
                            }
                        }
                    }
                }};
            }
            if focus {
                update_path_plan!(by_focus, new_by_focus_rule);
            } else {
                update_path_plan!(by_running_app, new_by_running_app_rule);
            }
        }
        Message::CpuEnabled(soft, enabled) => {
            let (kind, other) = if soft { (Soft, Hard) } else { (Hard, Soft) };
            let has_rule = cpu_allocation::settings(s, kind).contains_rule_for(&path);
            if !enabled && has_rule {
                cpu_allocation::settings_mut(s, kind)
                    .rules
                    .retain(|rule| !process_setting_matches(&rule.executable_path, &path));
            } else if enabled
                && !has_rule
                && !cpu_allocation::settings(s, other).contains_rule_for(&path)
                && !crate::cpu_allocation::is_builtin_excluded(&path)
            {
                let selection =
                    match cpu_allocation::SelectionCatalog::capture(kind).all_selection() {
                        Ok(selection) => selection,
                        Err(error) => return Some(error),
                    };
                cpu_allocation::settings_mut(s, kind)
                    .rules
                    .push(CpuAllocationRule {
                        enabled: true,
                        executable_path: path,
                        focus_selection: selection.clone(),
                        visible_window_selection: selection.clone(),
                        background_selection: selection,
                    });
            }
        }
        Message::CpuMask(soft, tier, selection) => {
            let kind = if soft { Soft } else { Hard };
            if let Some((index, rule)) = cpu_allocation::settings(s, kind)
                .rules
                .iter()
                .enumerate()
                .find(|(_, rule)| process_setting_matches(&rule.executable_path, &path))
            {
                if let Err(error) = cpu_allocation::verify_selection(&selection, kind) {
                    return Some(error);
                }
                let tier = match tier {
                    Tier::Focus => AllocationTier::Focus,
                    Tier::VisibleWindow => AllocationTier::Visible,
                    Tier::Background => AllocationTier::Background,
                };
                if selection != *tier.selection(rule) {
                    tier.set(
                        &mut cpu_allocation::settings_mut(s, kind).rules[index],
                        selection,
                    );
                }
            }
        }
        Message::Limiter(enabled) => {
            let has_rule = s
                .cpu_limiter
                .rules
                .iter()
                .any(|r| process_setting_matches(&r.executable_path, &path));
            if !enabled && has_rule {
                s.cpu_limiter
                    .rules
                    .retain(|r| !process_setting_matches(&r.executable_path, &path));
            } else if enabled && !has_rule && !crate::cpu_limiter::is_builtin_excluded(&path) {
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
                cpu_limiter::CpuLimiter::default().update_profile(
                    s,
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
                cpu_limiter::CpuLimiter::default().update_profile(
                    s,
                    cpu_limiter::Message::RuleLimit(i, limiter_tier(tier), value),
                );
            }
        }
    }
    None
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
    plans: PlanCatalog<'_>,
) -> Element<'a, Message> {
    let adaptive = !s
        .adaptive_engine_process
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
    let mut body = column![widgets::heading(
        t!("process_list.details_exclusions").to_string(),
        design::typography::BODY
    ),]
    .spacing(widgets::CARD_GAP);
    let mut exclusions = Vec::new();
    for (key, included, adaptive_rule) in [
        ("nav.adaptive_engine", adaptive, true),
        ("nav.background_efficiency", efficiency, false),
    ] {
        let options = [
            t!("process_list.include").to_string(),
            t!("process_list.exclude").to_string(),
        ];
        let selected = options[usize::from(!included)].clone();
        let include = options[0].clone();
        exclusions.push(
            setting_row(
                key,
                pick_list(options, Some(selected), move |value| {
                    if adaptive_rule {
                        Message::Adaptive(value == include)
                    } else {
                        Message::Efficiency(value == include)
                    }
                })
                .width(design::SELECT_WIDTH),
            )
            .into(),
        );
    }
    body = body.push(settings_card(widgets::settings_rows(exclusions)));
    body = body.push(widgets::heading(
        t!("process_list.details_power").to_string(),
        design::typography::BODY,
    ));
    if let PlanCatalog::Loaded(plans) = plans {
        let mut power = Vec::new();
        for focus in [true, false] {
            let selected = if focus {
                s.by_focus
                    .rules
                    .iter()
                    .find(|r| r.enabled && selector_path_matches(&r.selector, path))
                    .and_then(|r| r.power_plan_guid.clone())
            } else {
                s.by_running_app
                    .rules
                    .iter()
                    .find(|r| r.enabled && selector_path_matches(&r.selector, path))
                    .and_then(|r| r.power_plan_guid.clone())
            };
            let mut options = vec![Plan(
                None,
                t!("process_list.no_path_specific_plan").to_string(),
            )];
            options.extend(
                plans
                    .iter()
                    .filter(|plan| !plan.managed_adaptive)
                    .map(|p| Plan(Some(p.guid.clone()), p.name.clone())),
            );
            let selected = options
                .iter()
                .find(|p| p.0 == selected)
                .cloned()
                .or_else(|| {
                    selected.map(|guid| {
                        Plan(
                            Some(guid),
                            t!("common.selected_plan_unavailable").to_string(),
                        )
                    })
                });
            power.push(
                setting_row(
                    if focus {
                        "nav.by_focus"
                    } else {
                        "nav.by_running_app"
                    },
                    pick_list(options, selected, move |p| Message::Power(focus, p.0))
                        .width(design::SELECT_WIDTH),
                )
                .into(),
            );
        }
        body = body.push(settings_card(widgets::settings_rows(power)));
    } else {
        body = body.push(text(
            t!(match plans {
                PlanCatalog::Loading => "common.loading",
                _ => "process_list.plans_not_loaded",
            })
            .to_string(),
        ));
        if matches!(plans, PlanCatalog::Unavailable) {
            body = body.push(
                action_button(t!("settings.refresh").to_string()).on_press(Message::ReloadPlans),
            );
        }
    }
    let mut header = row![text(t!("common.control").to_string())
        .size(design::typography::CAPTION)
        .width(Fill)]
    .spacing(design::space::SMALL)
    .align_y(iced::Center);
    for tier in Tier::ALL {
        header = header.push(
            container(text(tier.label()).size(design::typography::CAPTION))
                .width(iced::Length::FillPortion(1)),
        );
    }
    // Reserve the same reset-control space in the header and every row.
    header = header.push(iced::widget::Space::new().width(32));
    let mut priorities =
        column![container(header).padding(widgets::CARD_PADDING as u16)].spacing(0);
    let default_rule = ProcessExclusionRule::default();
    for kind in [
        Kind::Process,
        Kind::Thread,
        Kind::DynamicBoost,
        Kind::Io,
        Kind::Gpu,
        Kind::Memory,
    ] {
        let key = format!("nav.{}", kind.key());
        let rule = kind
            .rules(s)
            .iter()
            .find(|r| process_setting_matches(&r.executable_path, path));
        let reset = rule.is_some().then_some(Message::ResetPriority(kind));
        let rule = rule.unwrap_or(&default_rule);
        let mut controls = row![text(t!(&key).to_string()).width(Fill)]
            .spacing(design::space::SMALL)
            .align_y(iced::Center)
            .height(widgets::SETTING_ROW_HEIGHT);
        for tier in Tier::ALL {
            controls = controls.push(
                pick_list(
                    kind.choices(s.advanced.expose_all_priority_values),
                    Some(kind.rule_value(rule, tier)),
                    move |v| Message::Priority(kind, tier, v),
                )
                .width(Fill),
            );
        }
        controls = controls.push(widgets::tooltip(
            widgets::icon_button_maybe("icons/rotate-ccw.svg", reset),
            text(t!("process_list.use_page_defaults").to_string()),
            iced::widget::tooltip::Position::Top,
        ));
        priorities = priorities
            .push(widgets::settings_divider())
            .push(container(controls).padding(widgets::CARD_PADDING as u16));
    }
    body = body
        .push(widgets::heading(
            t!("process_list.details_priority").to_string(),
            design::typography::BODY,
        ))
        .push(container(priorities).width(Fill).style(widgets::surface))
        .push(widgets::heading(
            t!("nav.cpu_control").to_string(),
            design::typography::BODY,
        ));
    if crate::cpu_allocation::has_multiple_processor_groups() {
        body = body.push(text(t!("cpu_sets_soft.warning").to_string()));
    }
    body = body.push(text(t!("processor_affinity_hard.warning").to_string()));
    for soft in [true, false] {
        let (settings, other) = if soft {
            (&s.cpu_sets_soft, &s.processor_affinity_hard)
        } else {
            (&s.processor_affinity_hard, &s.cpu_sets_soft)
        };
        let rule = settings
            .rules
            .iter()
            .find(|r| process_setting_matches(&r.executable_path, path));
        let blocked = rule.is_none() && other.contains_rule_for(path);
        let catalog = rule.map(|_| {
            cpu_allocation::SelectionCatalog::capture(if soft {
                cpu_allocation::Kind::Soft
            } else {
                cpu_allocation::Kind::Hard
            })
        });
        let mut card = column![setting_row(
            if soft {
                "nav.cpu_sets_soft"
            } else {
                "nav.processor_affinity_hard"
            },
            switch(
                rule.is_some(),
                (!blocked).then_some(move |v| Message::CpuEnabled(soft, v)),
            ),
        )]
        .spacing(widgets::CARD_GAP);
        if let Some(rule) = rule {
            for tier in Tier::ALL {
                let selection = match tier {
                    Tier::Focus => &rule.focus_selection,
                    Tier::VisibleWindow => &rule.visible_window_selection,
                    Tier::Background => &rule.background_selection,
                };
                let Some(catalog) = &catalog else { continue };
                card = card
                    .push(widgets::settings_divider())
                    .push(text(tier.label()))
                    .push(cpu_allocation::mask_selector(
                        selection,
                        catalog,
                        &s.cpu_allocation_presets,
                        move |v| Message::CpuMask(soft, tier, v),
                    ));
            }
        } else if blocked {
            let owner = t!(if soft {
                "nav.processor_affinity_hard"
            } else {
                "nav.cpu_sets_soft"
            });
            card = card.push(
                text(t!("cpu_allocation.ownership_conflict", owner = owner).to_string())
                    .size(design::typography::SECONDARY)
                    .style(text::secondary)
                    .width(Fill)
                    .wrapping(text::Wrapping::WordOrGlyph),
            );
        }
        body = body.push(settings_card(card));
    }
    let limiter = s
        .cpu_limiter
        .rules
        .iter()
        .find(|r| process_setting_matches(&r.executable_path, path));
    let mut card = column![setting_row(
        "nav.cpu_limiter",
        switch(limiter.is_some(), Some(Message::Limiter))
    )]
    .spacing(widgets::CARD_GAP);
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
            let mut controls = column![row![
                text(tier.label()).width(Fill),
                pick_list(ProcessRuleMode::ALL.map(Mode), Some(Mode(mode)), move |m| {
                    Message::LimiterMode(tier, m.0)
                })
            ]]
            .spacing(widgets::CARD_GAP);
            if mode == ProcessRuleMode::Enabled {
                controls = controls.push(
                    row![
                        cpu_limiter::limit_slider(limit, move |v| Message::LimiterLimit(tier, v)),
                        text(format!("{limit}%"))
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center),
                );
            }
            card = card.push(widgets::settings_divider()).push(controls);
        }
    }
    body = body.push(settings_card(card));
    body.into()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_focus_plan_only_edits_the_exact_path() {
        let mut settings = Settings::default();
        let path = r"C:\Apps\worker.exe";
        settings.by_focus.rules.push(ByFocusRule {
            enabled: true,
            name: "worker.exe".into(),
            selector: ExecutableSelector::ExactName {
                executable_name: "worker.exe".into(),
            },
            power_plan_guid: Some("name-plan".into()),
        });
        let mut settings = SettingsEditor::with_settings(settings);
        settings.select_power_source(PowerSourceProfile::OnBattery);
        let _ = update(&mut settings, path, Message::Power(true, None));
        assert!(settings.global().on_battery.is_none());

        let _ = update(
            &mut settings,
            path,
            Message::Power(true, Some("path-plan".into())),
        );
        assert_eq!(settings.by_focus.rules.len(), 2);
        assert_eq!(settings.global().by_focus.rules.len(), 1);
        assert!(selector_path_matches(
            &settings.by_focus.rules[1].selector,
            path
        ));

        let _ = update(&mut settings, path, Message::Power(true, None));
        assert_eq!(settings.by_focus.rules.len(), 1);
        assert_eq!(
            settings.by_focus.rules[0].power_plan_guid.as_deref(),
            Some("name-plan")
        );
    }

    #[test]
    fn details_priority_edits_do_not_enable_master_or_other_tiers() {
        let mut s = SettingsEditor::with_settings(Settings::default());
        s.select_power_source(PowerSourceProfile::OnBattery);
        let path = r"C:\App\app.exe";
        let _ = update(&mut s, path, Message::ResetPriority(Kind::Process));
        assert!(s.global().on_battery.is_none());

        let _ = update(
            &mut s,
            path,
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
        let _ = update(&mut s, path, Message::ResetPriority(Kind::Process));
        assert!(Kind::Process.rules(&s).is_empty());
    }

    #[test]
    fn selecting_stored_priority_preserves_battery_inheritance() {
        let mut s = SettingsEditor::with_settings(Settings::default());
        let path = r"C:\App\app.exe";
        let message = Message::Priority(
            Kind::Process,
            Tier::Focus,
            Value::Process(ProcessPrioritySetting::AboveNormal),
        );
        let _ = update(&mut s, path, message.clone());

        s.select_power_source(PowerSourceProfile::OnBattery);
        let _ = update(&mut s, path, message);
        assert!(s.global().on_battery.is_none());
    }

    #[test]
    fn including_process_without_an_exclusion_preserves_battery_inheritance() {
        for message in [Message::Adaptive(true), Message::Efficiency(true)] {
            let mut s = SettingsEditor::with_settings(Settings::default());
            s.select_power_source(PowerSourceProfile::OnBattery);
            let _ = update(&mut s, r"C:\App\app.exe", message);
            assert!(s.global().on_battery.is_none());
        }
    }

    #[test]
    fn cpu_rule_ownership_and_noops_preserve_battery_inheritance() {
        let path = r"C:\App\app.exe";
        let mut s = Settings::default();
        s.cpu_sets_soft.rules.push(CpuAllocationRule {
            enabled: true,
            executable_path: path.into(),
            focus_selection: crate::cpu_topology::test_selection(1),
            visible_window_selection: crate::cpu_topology::test_selection(1),
            background_selection: crate::cpu_topology::test_selection(1),
        });
        let mut s = SettingsEditor::with_settings(s);
        s.select_power_source(PowerSourceProfile::OnBattery);
        for message in [
            Message::CpuEnabled(false, true),
            Message::CpuEnabled(true, true),
            Message::CpuEnabled(false, false),
        ] {
            let _ = update(&mut s, path, message);
        }
        assert!(s.global().on_battery.is_none());
        assert!(s.processor_affinity_hard.rules.is_empty());
    }

    #[test]
    fn limiter_edits_override_battery_only_when_the_rule_changes() {
        let path = r"C:\App\app.exe";
        let mut settings = Settings::default();
        settings.cpu_limiter.rules.push(new_cpu_limiter_rule(path));
        let mut settings = SettingsEditor::with_settings(settings);
        settings.select_power_source(PowerSourceProfile::OnBattery);

        let _ = update(
            &mut settings,
            path,
            Message::LimiterMode(Tier::Focus, ProcessRuleMode::Default),
        );
        let _ = update(&mut settings, r"C:\App\other.exe", Message::Limiter(false));
        assert!(settings.global().on_battery.is_none());

        let _ = update(
            &mut settings,
            path,
            Message::LimiterMode(Tier::Focus, ProcessRuleMode::Enabled),
        );
        assert_eq!(
            settings.cpu_limiter.rules[0].focus_mode,
            ProcessRuleMode::Enabled
        );
        assert_eq!(
            settings.global().cpu_limiter.rules[0].focus_mode,
            ProcessRuleMode::Default
        );
    }
}
