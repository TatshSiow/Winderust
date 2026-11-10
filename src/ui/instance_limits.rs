use super::{app_picker, design, widgets};
use crate::{
    config::{InstanceLimitsRule, InstanceLimitsSettings},
    control::instance_limits::{paths_equal, CountObservation},
    features::advanced_controls::instance_limits::{InstanceLimitsSnapshot, NotificationStatus},
};
use iced::{
    widget::{column, row, text},
    Element, Fill,
};
use rust_i18n::t;

#[derive(Default)]
pub(super) struct Editor {
    path: String,
    draft: Option<(Option<usize>, InstanceLimitsRule, String)>,
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Notify(bool),
    Path(String),
    Browse,
    Add,
    Edit(usize),
    Remove(usize),
    RuleEnabled(usize, bool),
    Maximum(String),
    SaveRule,
    CancelRule,
}

impl Editor {
    fn can_add(settings: &InstanceLimitsSettings, path: &str) -> bool {
        InstanceLimitsRule::new(path.to_owned()).validate().is_ok()
            && settings
                .rules
                .iter()
                .all(|rule| matches!(paths_equal(&rule.executable_path, path), Ok(false)))
    }

    pub(super) fn has_pending_editor(&self) -> bool {
        self.draft.is_some()
    }

    fn validated_draft(&self) -> Option<InstanceLimitsRule> {
        let (_, rule, maximum) = self.draft.as_ref()?;
        let mut rule = rule.clone();
        rule.max_instances = maximum.parse().ok()?;
        rule.validate().ok()?;
        Some(rule)
    }

    pub(super) fn update(&mut self, settings: &mut InstanceLimitsSettings, message: Message) {
        match message {
            Message::Enabled(value) => settings.enabled = value,
            Message::Notify(value) => settings.notify = value,
            Message::Path(path) => self.path = path,
            Message::Add if Self::can_add(settings, &self.path) => {
                self.draft = Some((None, InstanceLimitsRule::new(self.path.clone()), "1".into()));
            }
            Message::Edit(index) => {
                self.draft = settings.rules.get(index).cloned().map(|rule| {
                    let maximum = rule.max_instances.to_string();
                    (Some(index), rule, maximum)
                });
            }
            Message::Remove(index) if index < settings.rules.len() => {
                settings.rules.remove(index);
                self.draft = None;
            }
            Message::RuleEnabled(index, enabled) => {
                if let Some(rule) = settings.rules.get_mut(index) {
                    rule.enabled = enabled;
                }
            }
            Message::Maximum(value) => {
                if let Some((_, _, maximum)) = &mut self.draft {
                    *maximum = value;
                }
            }
            Message::SaveRule => {
                if let Some(rule) = self.validated_draft() {
                    if let Some((Some(index), _, _)) = &self.draft {
                        if let Some(target) = settings.rules.get_mut(*index) {
                            *target = rule;
                        }
                    } else if Self::can_add(settings, &rule.executable_path) {
                        settings.rules.push(rule);
                        self.path.clear();
                    }
                    self.draft = None;
                }
            }
            Message::CancelRule => self.draft = None,
            Message::Add | Message::Remove(_) | Message::Browse => {}
        }
    }

    pub(super) fn view<'a>(
        &'a self,
        settings: &'a InstanceLimitsSettings,
        status: &'a InstanceLimitsSnapshot,
        candidates: &'a [app_picker::Candidate],
    ) -> Element<'a, Message> {
        let rules = settings
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| {
                // An unsaved rule must never inherit counts or incidents from a different maximum.
                let runtime = status.rules.iter().find(|entry| {
                    entry.executable_path == rule.executable_path
                        && entry.max_instances == rule.max_instances
                        && entry.enabled == rule.enabled
                });
                let label = if !settings.enabled || !status.enabled || !rule.enabled {
                    t!("common.off").to_string()
                } else {
                    count_label(
                        runtime
                            .filter(|entry| sample_fresh(entry.sampled_at))
                            .map(|entry| &entry.count),
                        rule.max_instances,
                    )
                };
                let header = widgets::process_rule_header(
                    &rule.executable_path,
                    candidates,
                    widgets::checkbox(rule.enabled)
                        .on_toggle(move |value| Message::RuleEnabled(index, value))
                        .into(),
                    Some(
                        text(label)
                            .size(design::typography::SECONDARY)
                            .style(
                                if settings.enabled
                                    && status.enabled
                                    && rule.enabled
                                    && runtime.is_some_and(|entry| {
                                        sample_fresh(entry.sampled_at)
                                            && exceeds(&entry.count, rule.max_instances)
                                    })
                                {
                                    text::danger
                                } else {
                                    text::default
                                },
                            )
                            .into(),
                    ),
                    Vec::new(),
                    row![
                        widgets::tooltip(
                            widgets::icon_button("icons/pencil.svg").on_press(Message::Edit(index)),
                            text(t!("common.edit").to_string()),
                            iced::widget::tooltip::Position::Top
                        ),
                        widgets::rule_delete_button(Some(Message::Remove(index))),
                    ]
                    .spacing(design::space::SMALL)
                    .into(),
                );
                let mut details = column![text(
                    t!(
                        "instance_limits.maximum_value",
                        maximum = rule.max_instances
                    )
                    .to_string()
                )
                .size(design::typography::SECONDARY)]
                .spacing(design::space::SMALL);
                if settings.enabled && status.enabled && rule.enabled {
                    if let Some(runtime) = runtime {
                        if let Some(reason) = count_reason(&runtime.count) {
                            details =
                                details.push(text(reason).size(design::typography::SECONDARY));
                        }
                        if runtime.incident {
                            details = details.push(
                                text(t!("instance_limits.incident").to_string())
                                    .style(text::danger),
                            );
                        }
                        let notification = match &runtime.notification {
                            NotificationStatus::NotAttempted => None,
                            NotificationStatus::Pending => {
                                Some(t!("instance_limits.notification_pending").to_string())
                            }
                            NotificationStatus::Accepted => {
                                Some(t!("instance_limits.notification_accepted").to_string())
                            }
                            NotificationStatus::Failed(error) => Some(
                                t!("instance_limits.notification_failed", error = error)
                                    .to_string(),
                            ),
                        };
                        if let Some(notification) = notification {
                            details = details
                                .push(text(notification).size(design::typography::SECONDARY));
                        }
                        if let Some(sampled_at) = runtime.sampled_at {
                            let seconds = sampled_at.elapsed().as_secs();
                            details = details.push(
                                text(
                                    t!(
                                        if sample_fresh(Some(sampled_at)) {
                                            "instance_limits.sample_age"
                                        } else {
                                            "instance_limits.stale_sample"
                                        },
                                        seconds = seconds
                                    )
                                    .to_string(),
                                )
                                .size(design::typography::SECONDARY),
                            );
                        }
                    }
                }
                (
                    widgets::stable_key(&rule.executable_path),
                    column![
                        header,
                        iced::widget::container(details).padding(iced::Padding {
                            top: 0.0,
                            right: widgets::CARD_PADDING as f32,
                            bottom: widgets::CARD_PADDING as f32,
                            left: widgets::CARD_PADDING as f32
                        }),
                        widgets::settings_divider(),
                    ]
                    .into(),
                )
            })
            .collect();
        let mut picker = column![app_picker::view(
            &self.path,
            candidates,
            true,
            Message::Path,
            Message::Browse,
            Self::can_add(settings, &self.path).then_some(Message::Add),
            |path| Self::can_add(settings, path).then_some(true)
        )]
        .spacing(design::space::SMALL);
        if !self.path.is_empty() {
            if let Err(error) = InstanceLimitsRule::new(self.path.clone()).validate() {
                picker = picker
                    .push(text(t!("instance_limits.invalid").to_string()).style(text::danger))
                    .push(text(error).size(design::typography::SECONDARY));
            }
        }
        super::scrolling::scrollable(
            column![
                widgets::toggle_card(
                    "instance_limits.enable",
                    settings.enabled,
                    Some(Message::Enabled)
                ),
                text(t!("instance_limits.shared").to_string()).size(design::typography::SECONDARY),
                text(t!("instance_limits.intro_1").to_string()).size(design::typography::SECONDARY),
                widgets::toggle_card(
                    "instance_limits.notify",
                    settings.notify,
                    Some(Message::Notify)
                ),
                widgets::setting_title("common.rules"),
                picker,
                widgets::process_rules_table_with_actions(
                    [],
                    rules,
                    t!("instance_limits.empty").to_string(),
                    80,
                    true
                ),
            ]
            .spacing(widgets::CARD_GAP),
        )
        .height(Fill)
        .into()
    }

    pub(super) fn modal<'a>(
        &'a self,
        candidates: &[app_picker::Candidate],
    ) -> Option<Element<'a, Message>> {
        let (index, rule, maximum) = self.draft.as_ref()?;
        let valid = self.validated_draft().is_some();
        let mut body = column![
            app_picker::app_name(&rule.executable_path, candidates),
            text(rule.executable_path.clone()).style(text::secondary),
            widgets::heading(
                t!("common.rule_details").to_string(),
                design::typography::BODY
            ),
            widgets::settings_card(
                column![
                    widgets::setting_title("instance_limits.maximum"),
                    widgets::stepper(maximum, 1..=u64::from(u32::MAX), 1, Some(Message::Maximum)),
                ]
                .spacing(design::space::SMALL)
            ),
            text(t!("instance_limits.intro_2").to_string()).size(design::typography::SECONDARY),
        ]
        .spacing(widgets::CARD_GAP);
        if !valid {
            body = body.push(text(t!("instance_limits.invalid").to_string()).style(text::danger));
        }
        Some(widgets::modal_frame(
            row![
                widgets::heading(
                    t!(if index.is_some() {
                        "instance_limits.edit_rule"
                    } else {
                        "instance_limits.add_rule"
                    })
                    .to_string(),
                    design::typography::SECTION
                )
                .width(Fill),
                widgets::icon_button("icons/x.svg").on_press(Message::CancelRule)
            ]
            .align_y(iced::Center),
            super::scrolling::scrollable_pane(body).height(Fill),
            row![
                iced::widget::Space::new().width(Fill),
                widgets::action_button(t!("common.cancel").to_string())
                    .style(widgets::tertiary_button)
                    .on_press(Message::CancelRule),
                widgets::action_button(t!("common.ok").to_string())
                    .style(widgets::primary_button)
                    .on_press_maybe(valid.then_some(Message::SaveRule))
            ]
            .spacing(design::space::SMALL),
            (760, 460),
        ))
    }
}

fn count_label(count: Option<&CountObservation>, maximum: u32) -> String {
    match count {
        Some(CountObservation::Complete(count)) => format!("{count} / {maximum}"),
        Some(CountObservation::Incomplete {
            known_lower_bound: 0,
            ..
        }) => t!("instance_limits.incomplete").to_string(),
        Some(CountObservation::Incomplete {
            known_lower_bound, ..
        }) => t!(
            "instance_limits.at_least",
            count = known_lower_bound,
            maximum = maximum
        )
        .to_string(),
        Some(CountObservation::Conflict) => t!("instance_limits.conflict").to_string(),
        Some(CountObservation::Unavailable { .. }) | None => {
            t!("instance_limits.unknown").to_string()
        }
    }
}

fn count_reason(count: &CountObservation) -> Option<&str> {
    match count {
        CountObservation::Incomplete { reason, .. } | CountObservation::Unavailable { reason } => {
            Some(reason)
        }
        _ => None,
    }
}

pub(super) fn sample_fresh(sampled_at: Option<std::time::Instant>) -> bool {
    sampled_at.is_some_and(|sampled_at| sampled_at.elapsed() <= std::time::Duration::from_secs(3))
}

fn exceeds(count: &CountObservation, maximum: u32) -> bool {
    match count {
        CountObservation::Complete(count)
        | CountObservation::Incomplete {
            known_lower_bound: count,
            ..
        } => *count > maximum,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncertain_counts_never_appear_as_complete_zero() {
        assert!(!sample_fresh(None));
        assert!(sample_fresh(Some(std::time::Instant::now())));
        assert!(!sample_fresh(Some(
            std::time::Instant::now() - std::time::Duration::from_secs(4)
        )));
        let incomplete = CountObservation::Incomplete {
            known_lower_bound: 0,
            reason: "Access denied".into(),
        };
        assert_eq!(
            count_label(Some(&CountObservation::Complete(0)), 1),
            "0 / 1"
        );
        assert_ne!(count_label(Some(&incomplete), 1), "0 / 1");
        assert_eq!(count_reason(&incomplete), Some("Access denied"));
        assert!(!exceeds(&incomplete, 1));
        assert!(exceeds(
            &CountObservation::Incomplete {
                known_lower_bound: 2,
                reason: "Access denied".into(),
            },
            1
        ));
        assert!(!exceeds(
            &CountObservation::Unavailable {
                reason: "No catalog".into()
            },
            1
        ));
    }

    #[test]
    fn invalid_maximum_stays_in_draft_and_cancel_preserves_saved_rule() {
        let mut editor = Editor::default();
        let mut settings = InstanceLimitsSettings::default();
        editor.update(&mut settings, Message::Path(r"C:\Apps\app.exe".into()));
        editor.update(&mut settings, Message::Add);
        for value in ["0", "4294967296", "", "-1"] {
            editor.update(&mut settings, Message::Maximum(value.into()));
            editor.update(&mut settings, Message::SaveRule);
            assert!(settings.rules.is_empty());
            assert!(editor.has_pending_editor());
        }
        editor.update(&mut settings, Message::Maximum("2".into()));
        editor.update(&mut settings, Message::SaveRule);
        assert_eq!(settings.rules[0].max_instances, 2);
        assert!(!Editor::can_add(&settings, "c:/apps/APP.exe"));
        editor.update(&mut settings, Message::Edit(0));
        editor.update(&mut settings, Message::Maximum("3".into()));
        editor.update(&mut settings, Message::CancelRule);
        assert_eq!(settings.rules[0].max_instances, 2);
    }
}
