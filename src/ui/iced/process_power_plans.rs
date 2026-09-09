use super::design;
use super::widgets::{button, checkbox, pick_list, text_input};
use crate::{config::Settings, power::PowerPlan, ui::process_rules::*};
use iced::widget::{column, row, scrollable, text};
use iced::{Element, Fill};
use rust_i18n::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Foreground,
    RunningApp,
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Path(String),
    Browse,
    Add,
    RuleEnabled(usize, bool),
    Name(usize, String),
    Plan(usize, Option<String>),
    Remove(usize),
    ConfirmRemove,
    CancelRemove,
}

#[derive(Default)]
pub(super) struct Editor {
    path: String,
    removing: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Choice(Option<String>, String);

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

impl Editor {
    pub(super) fn update(
        &mut self,
        kind: Kind,
        settings: &mut Settings,
        plans: &[PowerPlan],
        message: Message,
    ) {
        // Both pages have the same rule editor, but keep their distinct typed settings and policies.
        macro_rules! update_rules {
            ($field:ident, $can_add:ident, $new_rule:ident) => {{
                let settings = &mut settings.$field;
                match message {
                    Message::Enabled(value) => settings.enabled = value,
                    Message::Path(path) => self.path = path,
                    Message::Browse => {}
                    Message::Add => {
                        if settings.enabled && $can_add(settings, &self.path) {
                            settings.rules.push($new_rule(
                                &self.path,
                                plans
                                    .iter()
                                    .find(|plan| plan.active)
                                    .map(|plan| plan.guid.clone()),
                            ));
                            self.path.clear();
                        }
                    }
                    Message::RuleEnabled(index, value) => {
                        if let Some(rule) = settings.rules.get_mut(index) {
                            rule.enabled = value;
                        }
                    }
                    Message::Name(index, value) => {
                        if let Some(rule) = settings.rules.get_mut(index) {
                            rule.name = value;
                        }
                    }
                    Message::Plan(index, value) => {
                        if let Some(rule) = settings.rules.get_mut(index) {
                            rule.power_plan_guid = value;
                        }
                    }
                    Message::Remove(index) => {
                        self.removing = settings
                            .rules
                            .get(index)
                            .map(|rule| rule.executable_path.clone())
                    }
                    Message::CancelRemove => self.removing = None,
                    Message::ConfirmRemove => {
                        if let Some(path) = self.removing.take() {
                            if let Some(index) = settings
                                .rules
                                .iter()
                                .position(|rule| rule.executable_path == path)
                            {
                                settings.rules.remove(index);
                            }
                        }
                    }
                }
            }};
        }
        match kind {
            Kind::Foreground => update_rules!(
                by_foreground,
                can_add_foreground_process,
                new_foreground_rule
            ),
            Kind::RunningApp => update_rules!(
                by_running_app,
                can_add_by_running_app_process,
                new_by_running_app_rule
            ),
        }
    }

    pub(super) fn view<'a>(
        &'a self,
        kind: Kind,
        settings: &'a Settings,
        plans: &[PowerPlan],
        candidates: &[String],
    ) -> Element<'a, Message> {
        macro_rules! render_rules {
            ($field:ident, $can_add:ident, $enable:literal) => {{
                let settings = &settings.$field;
                let mut body = column![
 super::widgets::settings_card(super::widgets::setting_row($enable,super::widgets::switch(settings.enabled,Some(Message::Enabled)))),
                    text(t!("common.power_plan_priority").to_string()),
                    text(t!("common.power_plan_pause_priority").to_string()),
                ].spacing(super::widgets::CARD_GAP);
                let mut rules_body = column![
                    row![text_input(&t!("process_list.executable_path"), &self.path).on_input(Message::Path),
                        button(text(t!("common.browse_executable").to_string())).on_press_maybe(settings.enabled.then_some(Message::Browse)),
                        button(text(t!("common.add").to_string())).on_press_maybe((settings.enabled && $can_add(settings, &self.path)).then_some(Message::Add))].spacing(design::space::SMALL),
                ].spacing(super::widgets::CARD_GAP);
                let filter = self.path.to_lowercase();
                let candidates: Vec<_> = candidates.iter().filter(|path| path.to_lowercase().contains(&filter) && $can_add(settings, path)).cloned().collect();
                if settings.enabled && !candidates.is_empty() { rules_body = rules_body.push(pick_list(candidates, None::<String>, Message::Path).placeholder(t!("common.search_running_apps").to_string())); }
                let mut cards = Vec::new();
                for (index, rule) in settings.rules.iter().enumerate() {
                    let mut choices = vec![Choice(None, t!("common.none").to_string())];
                    choices.extend(plans.iter().map(|plan| Choice(Some(plan.guid.clone()), plan.display_name())));
                    let selected = choices.iter().find(|choice| choice.0 == rule.power_plan_guid).cloned().unwrap_or_else(|| Choice(rule.power_plan_guid.clone(), rule.power_plan_guid.clone().unwrap_or_default()));
                    let selector: Element<'_,Message> = if settings.enabled { pick_list(choices,Some(selected),move |choice|Message::Plan(index,choice.0)).into() } else { text(selected.1).into() };
                    let mut card = column![
                        row![checkbox(rule.enabled).on_toggle_maybe(settings.enabled.then_some(move |value| Message::RuleEnabled(index, value))),
                            text_input(&t!("process_list.app_name"), &rule.name).on_input_maybe(settings.enabled.then_some(move |value| Message::Name(index, value))),
                            button(text(t!("common.remove").to_string())).on_press_maybe(settings.enabled.then_some(Message::Remove(index)))].spacing(design::space::SMALL),
                        text(&rule.executable_path),
                        selector,
                    ].spacing(design::space::SMALL);
                    if self.removing.as_deref() == Some(rule.executable_path.as_str()) {
                        card = card.push(row![button(text(t!("common.remove").to_string())).on_press(Message::ConfirmRemove), button(text(t!("common.cancel").to_string())).on_press(Message::CancelRemove)].spacing(design::space::SMALL));
                    }
                    cards.push((super::widgets::stable_key(&rule.executable_path), super::widgets::settings_card(card).into()));
                }
                rules_body = rules_body.push(iced::widget::keyed_column(cards).spacing(super::widgets::CARD_GAP));
                body = body.push(text(t!("common.rules").to_string()).size(design::typography::SECTION)).push(rules_body);
                scrollable(body).height(Fill).into()
            }};
        }
        match kind {
            Kind::Foreground => render_rules!(
                by_foreground,
                can_add_foreground_process,
                "by_foreground.enable"
            ),
            Kind::RunningApp => render_rules!(
                by_running_app,
                can_add_by_running_app_process,
                "by_running_app.enable"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rules_stay_with_their_feature_and_duplicates_are_rejected() {
        let mut settings = Settings::default();
        let mut editor = Editor::default();
        editor.update(Kind::Foreground, &mut settings, &[], Message::Enabled(true));
        for _ in 0..2 {
            editor.update(
                Kind::Foreground,
                &mut settings,
                &[],
                Message::Path(r"C:\Apps\game.exe".into()),
            );
            editor.update(Kind::Foreground, &mut settings, &[], Message::Add);
        }
        assert_eq!(settings.by_foreground.rules.len(), 1);
        assert!(settings.by_running_app.rules.is_empty());
        assert_eq!(settings.by_foreground.rules[0].power_plan_guid, None);
        editor.update(
            Kind::Foreground,
            &mut settings,
            &[],
            Message::Plan(0, Some("selected".into())),
        );
        assert_eq!(
            settings.by_foreground.rules[0].power_plan_guid.as_deref(),
            Some("selected")
        );
    }
}
