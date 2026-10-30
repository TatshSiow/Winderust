use super::widgets::{self, Ui};
use crate::config::TimerResolutionSettings;
use crate::timer_resolution::TimerResolutionSnapshot;
use crate::ui::process_rules::{can_add_timer_resolution_process, new_timer_resolution_rule};
use rust_i18n::t;
use windui::prelude::{signal, Element};
#[derive(Default)]
pub(super) struct Editor {
    pub(super) path: String,
    editing: Option<(usize, String)>,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Path(String),
    Add,
    RuleEnabled(usize, bool),
    Resolution(usize, String),
    Commit(usize),
    Remove(usize),

    Browse,
}
impl Editor {
    pub(super) fn has_invalid_inputs(&self) -> bool {
        self.editing
            .as_ref()
            .is_some_and(|(_, value)| parse_resolution(value, 1_000, 10_000_000).is_none())
    }

    pub(super) fn update(
        &mut self,
        s: &mut TimerResolutionSettings,
        status: &TimerResolutionSnapshot,
        m: Message,
    ) {
        match m {
            Message::Enabled(v) => s.enabled = v,
            Message::Path(v) => self.path = v,
            Message::Add if can_add_timer_resolution_process(s, &self.path) => {
                s.rules
                    .push(new_timer_resolution_rule(&self.path, s.desired_100ns));
                self.path.clear();
            }
            Message::RuleEnabled(i, v) => {
                if let Some(r) = s.rules.get_mut(i) {
                    r.enabled = v;
                }
            }
            Message::Resolution(i, v) => {
                self.editing = Some((i, v.clone()));
                self.update(s, status, Message::Commit(i));
                self.editing = Some((i, v));
            }
            Message::Commit(i) => {
                if let Some((index, value)) = &self.editing {
                    if *index == i {
                        if let (Some(r), Some(v)) = (
                            s.rules.get_mut(i),
                            parse_resolution(
                                value,
                                status.minimum_100ns.unwrap_or(1_000),
                                status.maximum_100ns.unwrap_or(10_000_000),
                            ),
                        ) {
                            r.desired_100ns = v;
                            self.editing = None;
                        }
                    }
                }
            }
            Message::Remove(i) => {
                if i < s.rules.len() {
                    s.rules.remove(i);
                    self.editing = None;
                }
            }

            Message::Add | Message::Browse => {}
        }
    }
    pub(super) fn view(
        &self,
        settings: &TimerResolutionSettings,
        status: &TimerResolutionSnapshot,
        candidates: &[super::app_picker::Candidate],
        ui: Ui<Message>,
    ) -> Element {
        let candidate_settings = settings.clone();
        let mut body = vec![
            widgets::setting_row(
                t!("timer_resolution.enable").to_string(),
                ui.toggle(settings.enabled, Message::Enabled),
            ),
            widgets::heading(t!("common.rules").to_string()),
            super::app_picker::view(
                &self.path,
                candidates,
                true,
                Message::Path,
                Message::Browse,
                Some(Message::Add),
                move |candidate| {
                    can_add_timer_resolution_process(
                        &candidate_settings,
                        &candidate.info.image_path.to_string_lossy(),
                    )
                    .then_some(true)
                },
                ui.clone(),
            ),
        ];
        for (index, rule) in settings.rules.iter().enumerate() {
            let value = self
                .editing
                .as_ref()
                .filter(|(i, _)| *i == index)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| {
                    crate::timer_resolution::format_resolution_ms(rule.desired_100ns)
                        .trim_end_matches(" ms")
                        .to_owned()
                });
            let raw = signal(value);
            let edits = ui.clone();
            let submit = ui.clone();
            let input = widgets::observe(
                Element::text_input(raw, "0.1-1000")
                    .width_match()
                    .on_submit(move |ctx| {
                        widgets::flush_bindings(ctx);
                        submit.emit(Message::Commit(index));
                    }),
                raw,
                move |value| edits.edit(Message::Resolution(index, value)),
            );
            body.push(widgets::card(
                rule.executable_path.clone(),
                [
                    ui.checkbox(
                        t!("common.enable").to_string(),
                        rule.enabled,
                        move |value| Message::RuleEnabled(index, value),
                    ),
                    widgets::setting_row(
                        format!("{} (ms)", t!("timer_resolution.requested")),
                        input,
                    ),
                    ui.button(t!("common.remove").to_string(), Message::Remove(index)),
                ],
            ));
        }
        if settings.rules.is_empty() {
            body.push(widgets::label(t!("timer_resolution.no_rules").to_string()));
        }
        for (key, value) in [
            ("timer_resolution.minimum", status.minimum_100ns),
            ("timer_resolution.maximum", status.maximum_100ns),
            ("timer_resolution.requested", status.requested_100ns),
        ] {
            if let Some(value) = value {
                body.push(widgets::label(format!(
                    "{}: {}",
                    t!(key),
                    crate::timer_resolution::format_resolution_ms(value)
                )));
            }
        }
        if let Some(error) = &status.last_error {
            body.push(widgets::error(error.clone()));
        }
        widgets::page(body)
    }
}
fn parse_resolution(value: &str, min: u32, max: u32) -> Option<u32> {
    let value = value.trim();
    let value = value
        .strip_suffix("ms")
        .or_else(|| value.strip_suffix("MS"))
        .or_else(|| value.strip_suffix("Ms"))
        .or_else(|| value.strip_suffix("mS"))
        .unwrap_or(value)
        .trim()
        .parse::<f64>()
        .ok()?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    Some(crate::timer_resolution::normalize_desired_resolution(
        (value.clamp(0.1, 1000.0) * 10_000.0).round() as u32,
        min,
        max,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn valid_edits_update_the_draft_without_an_apply_button() {
        let mut editor = Editor::default();
        let mut settings = TimerResolutionSettings::default();
        settings
            .rules
            .push(new_timer_resolution_rule(r"C:\Apps\test.exe", 10_000));
        let status = TimerResolutionSnapshot::default();
        editor.update(
            &mut settings,
            &status,
            Message::Resolution(0, "2.00".into()),
        );
        assert_eq!(settings.rules[0].desired_100ns, 20_000);
        assert!(!editor.has_invalid_inputs());
        editor.update(&mut settings, &status, Message::Resolution(0, "".into()));
        assert!(editor.has_invalid_inputs());
        assert_eq!(settings.rules[0].desired_100ns, 20_000);
    }
    #[test]
    fn timer_input_is_bounded_and_rejects_nonfinite() {
        assert_eq!(parse_resolution("0.5 ms", 10_000, 160_000), Some(10_000));
        assert_eq!(
            parse_resolution("15.625 MS", 10_000, 160_000),
            Some(160_000)
        );
        assert_eq!(parse_resolution("NaN", 10_000, 160_000), None);
    }
}
