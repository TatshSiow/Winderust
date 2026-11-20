use std::sync::Arc;

use iced::widget::{column, row, text, Column};
use iced::{Element, Fill, Task};
use rust_i18n::t;

use crate::{
    config::{ByRunningAppRule, ByRunningAppSettings},
    features::power_plan_control::by_running_app::{
        preview_matches, rule_name, ByRunningAppPreview, ByRunningAppPreviewMatch,
        ByRunningAppPreviewRow,
    },
    power::PowerPlan,
    ui::{design, scrolling::scrollable, tasks, widgets},
};

#[derive(Debug, Clone)]
pub(crate) enum PreviewMessage {
    Load,
    Loaded(
        Arc<ByRunningAppSettings>,
        Result<Arc<ByRunningAppPreview>, String>,
    ),
    Close,
}

#[derive(Default)]
pub(super) struct Preview {
    open: Option<OpenPreview>,
}

struct OpenPreview {
    settings: Arc<ByRunningAppSettings>,
    result: Option<Result<Arc<ByRunningAppPreview>, String>>,
}

impl Preview {
    pub(super) fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub(super) fn close(&mut self) {
        self.open = None;
    }

    pub(super) fn update(
        &mut self,
        settings: &ByRunningAppSettings,
        message: PreviewMessage,
    ) -> Task<PreviewMessage> {
        match message {
            PreviewMessage::Load => {
                if self.open.as_ref().is_some_and(|open| open.result.is_none()) {
                    return Task::none();
                }
                let request = Arc::new(settings.clone());
                self.open = Some(OpenPreview {
                    settings: Arc::clone(&request),
                    result: None,
                });
                let snapshot = Arc::clone(&request);
                return tasks::run(move || preview_matches(&snapshot)).map(move |result| {
                    PreviewMessage::Loaded(
                        Arc::clone(&request),
                        result.and_then(|result| result).map(Arc::new),
                    )
                });
            }
            PreviewMessage::Loaded(request, result) => {
                if let Some(open) = self
                    .open
                    .as_mut()
                    .filter(|open| open.result.is_none() && Arc::ptr_eq(&open.settings, &request))
                {
                    open.result = Some(result);
                }
            }
            PreviewMessage::Close => self.close(),
        }
        Task::none()
    }

    pub(super) fn view(&self, plans: &[PowerPlan]) -> Option<Element<'_, PreviewMessage>> {
        let open = self.open.as_ref()?;
        let mut body = column![text(t!("by_running_app.preview_scope").to_string())
            .style(text::secondary)
            .wrapping(text::Wrapping::WordOrGlyph)]
        .spacing(widgets::CARD_GAP)
        .width(Fill);
        match &open.result {
            None => body = body.push(text(t!("by_running_app.preview_loading").to_string())),
            Some(Err(error)) => {
                body = body.push(
                    text(t!("by_running_app.preview_failed", error = error).to_string())
                        .style(text::danger)
                        .wrapping(text::Wrapping::WordOrGlyph),
                );
            }
            Some(Ok(preview)) => {
                let summary = preview
                    .winner
                    .and_then(|index| open.settings.rules.get(index))
                    .map_or_else(
                        || t!("by_running_app.preview_no_winner").to_string(),
                        |rule| {
                            let plan = rule
                                .power_plan_guid
                                .as_deref()
                                .and_then(|guid| {
                                    plans.iter().find(|plan| {
                                        !plan.managed_adaptive
                                            && plan.guid.eq_ignore_ascii_case(guid)
                                    })
                                })
                                .map_or_else(
                                    || t!("common.selected_plan_unavailable").to_string(),
                                    |plan| plan.name.clone(),
                                );
                            t!(
                                "by_running_app.preview_winner",
                                rule = rule.executable_path.trim(),
                                plan = plan
                            )
                            .to_string()
                        },
                    );
                body = body.push(text(summary).wrapping(text::Wrapping::WordOrGlyph));
                for (rule, matches) in open.settings.rules.iter().zip(&preview.rules) {
                    body = body.push(rule_matches(rule, matches));
                }
            }
        }

        Some(widgets::modal_frame(
            widgets::heading(
                t!("by_running_app.preview").to_string(),
                design::typography::SECTION,
            ),
            scrollable(body).height(Fill),
            row![
                widgets::action_button(t!("settings.refresh").to_string())
                    .style(widgets::secondary_button)
                    .on_press_maybe(open.result.is_some().then_some(PreviewMessage::Load)),
                widgets::action_button(t!("common.done").to_string())
                    .style(widgets::primary_button)
                    .on_press(PreviewMessage::Close),
            ]
            .spacing(design::space::SMALL),
            (760, 520),
        ))
    }
}

fn rule_matches<'a>(
    rule: &ByRunningAppRule,
    matches: &[ByRunningAppPreviewRow],
) -> Element<'a, PreviewMessage> {
    use ByRunningAppPreviewMatch::{Match, SameNameDifferentPath, Unavailable};

    let path = rule.executable_path.trim();
    let name = rule_name(rule);
    let mut details = column![widgets::heading(name.to_owned(), design::typography::BODY)
        .wrapping(text::Wrapping::WordOrGlyph)]
    .spacing(design::space::SMALL);
    if path != name {
        details = details.push(
            text(path.to_owned())
                .style(text::secondary)
                .wrapping(text::Wrapping::WordOrGlyph),
        );
    }
    if !rule.enabled {
        details = details.push(text(t!("common.disabled").to_string()).style(text::secondary));
    }
    for (kind, heading) in [
        (Match, "by_running_app.preview_matches"),
        (SameNameDifferentPath, "by_running_app.preview_same_name"),
        (Unavailable, "by_running_app.preview_unavailable"),
    ] {
        let rows: Vec<Element<'_, PreviewMessage>> = matches
            .iter()
            .filter(|entry| entry.kind == kind)
            .map(|entry| {
                let path = entry.path.as_ref().map_or_else(
                    || t!("by_running_app.preview_path_unavailable").to_string(),
                    |path| path.display().to_string(),
                );
                row![
                    text(t!("by_running_app.preview_pid", pid = entry.pid).to_string()).width(112),
                    text(path)
                        .width(Fill)
                        .style(text::secondary)
                        .wrapping(text::Wrapping::WordOrGlyph),
                ]
                .spacing(design::space::MEDIUM)
                .into()
            })
            .collect();
        if !rows.is_empty() {
            details = details.push(text(t!(heading).to_string()));
            details = details.push(Column::with_children(rows).spacing(design::space::SMALL));
        } else if kind == Match {
            details = details.push(
                text(t!("by_running_app.preview_no_matches").to_string()).style(text::secondary),
            );
        }
    }
    widgets::settings_card(details).into()
}

#[test]
fn stale_result_cannot_finish_a_reopened_preview() {
    let settings = ByRunningAppSettings::default();
    let first_request = Arc::new(settings.clone());
    let mut preview = Preview {
        open: Some(OpenPreview {
            settings: Arc::clone(&first_request),
            result: None,
        }),
    };
    preview.close();
    let second_request = Arc::new(settings.clone());
    preview.open = Some(OpenPreview {
        settings: Arc::clone(&second_request),
        result: None,
    });

    let _ = preview.update(
        &settings,
        PreviewMessage::Loaded(first_request, Err("stale".to_owned())),
    );
    assert!(preview.open.as_ref().unwrap().result.is_none());
    let _ = preview.update(
        &settings,
        PreviewMessage::Loaded(second_request, Err("current".to_owned())),
    );
    assert!(matches!(
        preview.open.as_ref().unwrap().result.as_ref(),
        Some(Err(error)) if error == "current"
    ));
}
