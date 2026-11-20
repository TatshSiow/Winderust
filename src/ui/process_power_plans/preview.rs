use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use iced::widget::{column, row, text, Column};
use iced::{Element, Fill, Task};
use rust_i18n::t;

use crate::{
    config::{ByRunningAppRule, ByRunningAppSettings, Settings},
    control::instance_limits::validate_path,
    features::power_plan_control::by_running_app::{
        preview_matches, rule_name, ByRunningAppPreview, ByRunningAppPreviewMatch,
        ByRunningAppPreviewRow,
    },
    power::PowerPlan,
    rules::decision_engine::matching_by_focus_rule,
    ui::{design, scrolling::scrollable, tasks, widgets},
};

#[derive(Debug, Clone)]
pub(crate) enum PreviewMessage {
    Load,
    Loaded(
        Arc<ByRunningAppSettings>,
        Result<Arc<ByRunningAppPreview>, String>,
    ),
    OpenFocus,
    FocusPath(String),
    CheckFocus,
    FocusChecked(Arc<PathBuf>, Result<Option<FocusMatch>, String>),
    Close,
}

#[derive(Default)]
pub(super) struct Preview {
    open: Option<OpenPreview>,
}

enum OpenPreview {
    RunningApp {
        settings: Arc<ByRunningAppSettings>,
        result: Option<Result<Arc<ByRunningAppPreview>, String>>,
    },
    Focus(FocusPreview),
}

#[derive(Default)]
struct FocusPreview {
    path: String,
    check: Option<FocusCheck>,
}

struct FocusCheck {
    path: Arc<PathBuf>,
    result: Option<Result<Option<FocusMatch>, String>>,
}

#[derive(Debug, Clone)]
pub(crate) struct FocusMatch {
    rule: String,
    plan_guid: String,
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
        settings: &Settings,
        message: PreviewMessage,
    ) -> Task<PreviewMessage> {
        match message {
            PreviewMessage::Load => {
                if matches!(
                    self.open,
                    Some(OpenPreview::RunningApp { result: None, .. })
                ) {
                    return Task::none();
                }
                let request = Arc::new(settings.by_running_app.clone());
                self.open = Some(OpenPreview::RunningApp {
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
                if let Some(OpenPreview::RunningApp {
                    settings,
                    result: pending,
                }) = &mut self.open
                {
                    if pending.is_none() && Arc::ptr_eq(settings, &request) {
                        *pending = Some(result);
                    }
                }
            }
            PreviewMessage::OpenFocus => {
                self.open = Some(OpenPreview::Focus(FocusPreview::default()));
            }
            PreviewMessage::FocusPath(path) => {
                if let Some(OpenPreview::Focus(focus)) = &mut self.open {
                    if focus.path != path {
                        focus.path = path;
                        focus.check = None;
                    }
                }
            }
            PreviewMessage::CheckFocus => {
                let Some(OpenPreview::Focus(focus)) = &mut self.open else {
                    return Task::none();
                };
                let Some(path) = focus.checkable_path() else {
                    return Task::none();
                };
                let request = Arc::new(path.to_owned());
                focus.check = Some(FocusCheck {
                    path: Arc::clone(&request),
                    result: None,
                });
                let path = Arc::clone(&request);
                let settings = settings.by_focus.clone();
                return tasks::run(move || {
                    matching_by_focus_rule(&settings, &path).map(|(_, rule, plan_guid)| {
                        FocusMatch {
                            rule: rule.selector.value().to_owned(),
                            plan_guid: plan_guid.to_owned(),
                        }
                    })
                })
                .map(move |result| PreviewMessage::FocusChecked(Arc::clone(&request), result));
            }
            PreviewMessage::FocusChecked(request, result) => {
                if let Some(OpenPreview::Focus(focus)) = &mut self.open {
                    if let Some(check) = focus.check.as_mut().filter(|check| {
                        check.result.is_none() && Arc::ptr_eq(&check.path, &request)
                    }) {
                        check.result = Some(result);
                    }
                }
            }
            PreviewMessage::Close => self.close(),
        }
        Task::none()
    }

    pub(super) fn view(&self, plans: &[PowerPlan]) -> Option<Element<'_, PreviewMessage>> {
        let (settings, result) = match self.open.as_ref()? {
            OpenPreview::RunningApp { settings, result } => (settings, result),
            OpenPreview::Focus(focus) => return Some(focus.view(plans)),
        };
        let mut body = column![text(t!("by_running_app.preview_scope").to_string())
            .style(text::secondary)
            .wrapping(text::Wrapping::WordOrGlyph)]
        .spacing(widgets::CARD_GAP)
        .width(Fill);
        match result {
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
                    .and_then(|index| settings.rules.get(index))
                    .map_or_else(
                        || t!("power_plan_control.preview_no_winner").to_string(),
                        |rule| {
                            winner_label(
                                rule.selector.value(),
                                rule.power_plan_guid.as_deref(),
                                plans,
                            )
                        },
                    );
                body = body.push(text(summary).wrapping(text::Wrapping::WordOrGlyph));
                for (rule, matches) in settings.rules.iter().zip(&preview.rules) {
                    body = body.push(rule_matches(rule, matches));
                }
            }
        }

        Some(widgets::modal_frame(
            widgets::heading(
                t!("power_plan_control.preview").to_string(),
                design::typography::SECTION,
            ),
            scrollable(body).height(Fill),
            row![
                widgets::action_button(t!("settings.refresh").to_string())
                    .style(widgets::secondary_button)
                    .on_press_maybe(result.is_some().then_some(PreviewMessage::Load)),
                widgets::action_button(t!("common.done").to_string())
                    .style(widgets::primary_button)
                    .on_press(PreviewMessage::Close),
            ]
            .spacing(design::space::SMALL),
            (760, 520),
        ))
    }
}

impl FocusPreview {
    fn checkable_path(&self) -> Option<&Path> {
        let input = self.path.trim();
        let input = input
            .strip_prefix('"')
            .and_then(|path| path.strip_suffix('"'))
            .unwrap_or(input);
        (self
            .check
            .as_ref()
            .is_none_or(|check| check.result.is_some())
            && validate_path(input).is_ok())
        .then(|| Path::new(input))
    }

    fn view(&self, plans: &[PowerPlan]) -> Element<'_, PreviewMessage> {
        let check = self.checkable_path().map(|_| PreviewMessage::CheckFocus);
        let mut body = column![
            text(t!("by_focus.preview_scope").to_string())
                .style(text::secondary)
                .wrapping(text::Wrapping::WordOrGlyph),
            widgets::text_input(&t!("common.executable_path"), &self.path)
                .on_input(PreviewMessage::FocusPath)
                .on_submit_maybe(check.clone())
        ]
        .spacing(widgets::CARD_GAP);
        if check.is_none() && self.check.is_none() && !self.path.trim().is_empty() {
            body = body.push(super::path_format_hint());
        }
        if let Some(pending) = &self.check {
            let result = match &pending.result {
                None => text(t!("common.loading").to_string()).style(text::secondary),
                Some(Err(error)) => text(t!("by_focus.preview_failed", error = error).to_string())
                    .style(text::danger),
                Some(Ok(None)) => text(t!("power_plan_control.preview_no_winner").to_string()),
                Some(Ok(Some(matched))) => {
                    text(winner_label(&matched.rule, Some(&matched.plan_guid), plans))
                }
            };
            body = body.push(result.wrapping(text::Wrapping::WordOrGlyph));
        }
        widgets::modal_frame(
            widgets::heading(
                t!("power_plan_control.preview").to_string(),
                design::typography::SECTION,
            ),
            scrollable(body).height(Fill),
            row![
                widgets::action_button(t!("by_focus.preview_check").to_string())
                    .style(widgets::secondary_button)
                    .on_press_maybe(check),
                widgets::action_button(t!("common.done").to_string())
                    .style(widgets::primary_button)
                    .on_press(PreviewMessage::Close),
            ]
            .spacing(design::space::SMALL),
            (640, 280),
        )
    }
}

fn winner_label(rule: &str, plan_guid: Option<&str>, plans: &[PowerPlan]) -> String {
    let plan = plan_guid
        .and_then(|guid| {
            plans
                .iter()
                .find(|plan| !plan.managed_adaptive && plan.guid.eq_ignore_ascii_case(guid))
        })
        .map_or_else(
            || t!("common.selected_plan_unavailable").to_string(),
            |plan| plan.name.clone(),
        );
    t!(
        "power_plan_control.preview_winner",
        rule = rule.trim(),
        plan = plan
    )
    .to_string()
}

fn rule_matches<'a>(
    rule: &ByRunningAppRule,
    matches: &[ByRunningAppPreviewRow],
) -> Element<'a, PreviewMessage> {
    use ByRunningAppPreviewMatch::{Match, SameNameDifferentPath, Unavailable};

    let target = rule.selector.value().trim();
    let name = rule_name(rule);
    let mut details = column![widgets::heading(name.to_owned(), design::typography::BODY)
        .wrapping(text::Wrapping::WordOrGlyph)]
    .spacing(design::space::SMALL);
    if rule.selector.exact_path().is_none() || target != name {
        let location = if rule.selector.exact_path().is_some() {
            target.to_owned()
        } else {
            t!("common.any_path").to_string()
        };
        details = details.push(
            text(location)
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
    let settings = Settings::default();
    let first_request = Arc::new(settings.by_running_app.clone());
    let mut preview = Preview {
        open: Some(OpenPreview::RunningApp {
            settings: Arc::clone(&first_request),
            result: None,
        }),
    };
    preview.close();
    let second_request = Arc::new(settings.by_running_app.clone());
    preview.open = Some(OpenPreview::RunningApp {
        settings: Arc::clone(&second_request),
        result: None,
    });

    let _ = preview.update(
        &settings,
        PreviewMessage::Loaded(first_request, Err("stale".to_owned())),
    );
    assert!(matches!(
        preview.open,
        Some(OpenPreview::RunningApp { result: None, .. })
    ));
    let _ = preview.update(
        &settings,
        PreviewMessage::Loaded(second_request, Err("current".to_owned())),
    );
    assert!(matches!(
        preview.open,
        Some(OpenPreview::RunningApp { result: Some(Err(error)), .. }) if error == "current"
    ));
}

#[test]
fn focus_preview_checks_only_full_executable_paths() {
    for path in [
        "",
        "app.exe",
        r"Apps\app.exe",
        r"C:\",
        r"C:\Apps\",
        r"C:\Apps\.\app.exe",
        r"C:\Apps\old\..\app.exe",
        r"\\?\C:\Apps\app.exe",
    ] {
        let focus = FocusPreview {
            path: path.into(),
            ..Default::default()
        };
        assert!(focus.checkable_path().is_none());
    }
    for path in [r" C:\Apps\app.exe ", r#""C:\Apps\app.exe""#] {
        let focus = FocusPreview {
            path: path.into(),
            ..Default::default()
        };
        assert_eq!(focus.checkable_path(), Some(Path::new(r"C:\Apps\app.exe")));
    }
}

#[test]
fn a_focus_check_cannot_finish_after_editing_or_reopening() {
    let settings = Settings::default();
    let first = Arc::new(PathBuf::from(r"C:\Apps\app.exe"));
    let mut preview = Preview {
        open: Some(OpenPreview::Focus(FocusPreview {
            path: first.display().to_string(),
            check: Some(FocusCheck {
                path: Arc::clone(&first),
                result: None,
            }),
        })),
    };
    let _ = preview.update(
        &settings,
        PreviewMessage::FocusPath(r"C:\Apps\other.exe".into()),
    );
    let _ = preview.update(
        &settings,
        PreviewMessage::FocusChecked(Arc::clone(&first), Ok(None)),
    );
    assert!(matches!(
        preview.open,
        Some(OpenPreview::Focus(FocusPreview { check: None, .. }))
    ));

    preview.close();
    let _ = preview.update(&settings, PreviewMessage::OpenFocus);
    let Some(OpenPreview::Focus(focus)) = &mut preview.open else {
        panic!("Focus preview did not reopen")
    };
    assert!(focus.path.is_empty());
    let second = Arc::new((*first).clone());
    focus.path = second.display().to_string();
    focus.check = Some(FocusCheck {
        path: Arc::clone(&second),
        result: None,
    });
    let _ = preview.update(&settings, PreviewMessage::FocusChecked(first, Ok(None)));
    assert!(matches!(
        preview.open,
        Some(OpenPreview::Focus(FocusPreview {
            check: Some(FocusCheck { result: None, .. }),
            ..
        }))
    ));
    let _ = preview.update(&settings, PreviewMessage::FocusChecked(second, Ok(None)));
    assert!(matches!(
        preview.open,
        Some(OpenPreview::Focus(FocusPreview {
            check: Some(FocusCheck {
                result: Some(Ok(None)),
                ..
            }),
            ..
        }))
    ));
}
