use super::widgets::{self, Ui};
use crate::config::MemoryTrimSettings;
use crate::ui::process_rules::{can_add_memory_trim_exclusion, new_process_exclusion_rule};
use rust_i18n::t;
use windui::prelude::Element;

#[derive(Default)]
pub(super) struct Editor {
    pub(super) path: String,
    expanded: [bool; 3],
    inputs: [Option<String>; 3],
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Collapse(usize),
    Path(String),
    Add,
    Load(String),
    WorkingSet(String),
    Idle(String),
    RuleEnabled(usize, bool),
    Remove(usize),

    TrimNow,
    Browse,
}
impl Editor {
    pub(super) fn has_invalid_inputs(&self) -> bool {
        self.inputs.iter().enumerate().any(|(index, raw)| {
            raw.as_ref().is_some_and(|raw| {
                !raw.parse::<u64>()
                    .is_ok_and(|value| (1..=[100, 1_048_576, 86_400][index]).contains(&value))
            })
        })
    }

    fn input(&self, index: usize, value: impl ToString) -> String {
        self.inputs[index]
            .clone()
            .unwrap_or_else(|| value.to_string())
    }

    pub(super) fn update(&mut self, settings: &mut MemoryTrimSettings, message: Message) {
        match message {
            Message::Collapse(i) => {
                if let Some(value) = self.expanded.get_mut(i) {
                    *value = !*value;
                }
            }
            Message::Enabled(v) => settings.enabled = v,
            Message::Path(v) => self.path = v,
            Message::Add if can_add_memory_trim_exclusion(settings, &self.path) => {
                settings
                    .exclusions
                    .push(new_process_exclusion_rule(&self.path));
                self.path.clear();
            }
            Message::Load(v) => {
                self.inputs[0] = Some(v.clone());
                if let Ok(v) = v.parse::<u8>() {
                    if (1..=100).contains(&v) {
                        settings.system_memory_load_threshold_percent = v;
                    }
                }
            }
            Message::WorkingSet(v) => {
                self.inputs[1] = Some(v.clone());
                if let Ok(v) = v.parse::<u64>() {
                    if (1..=1_048_576).contains(&v) {
                        settings.process_working_set_threshold_mb = v;
                    }
                }
            }
            Message::Idle(v) => {
                self.inputs[2] = Some(v.clone());
                if let Ok(v) = v.parse::<u64>() {
                    if (1..=86_400).contains(&v) {
                        settings.process_idle_seconds = v;
                    }
                }
            }
            Message::RuleEnabled(i, v) => {
                if let Some(r) = settings.exclusions.get_mut(i) {
                    r.enabled = v;
                }
            }
            Message::Remove(i) => {
                if i < settings.exclusions.len() {
                    settings.exclusions.remove(i);
                }
            }

            Message::Add | Message::TrimNow | Message::Browse => {}
        }
    }
    pub(super) fn view(
        &self,
        settings: &MemoryTrimSettings,
        candidates: &[super::app_picker::Candidate],
        ui: Ui<Message>,
    ) -> Element {
        let mut body = vec![widgets::setting_row(
            t!("memory_trim.enable").to_string(),
            ui.toggle(settings.enabled, Message::Enabled),
        )];
        for (index, title) in [
            "memory_trim.category_thresholds",
            "memory_trim.category_when_to_trim",
            "memory_trim.category_safety",
        ]
        .into_iter()
        .enumerate()
        {
            let mut controls = Vec::new();
            match index {
                0 => {
                    controls.push(widgets::setting_row(
                        format!("{} (%)", t!("memory_trim.memory_threshold")),
                        ui.input(
                            self.input(0, settings.system_memory_load_threshold_percent),
                            "1-100",
                            Message::Load,
                        ),
                    ));
                    controls.push(widgets::setting_row(
                        format!("{} (MiB)", t!("memory_trim.working_set_threshold")),
                        ui.input(
                            self.input(1, settings.process_working_set_threshold_mb),
                            "1-1048576",
                            Message::WorkingSet,
                        ),
                    ));
                }
                1 => controls.push(widgets::setting_row(
                    format!("{} (s)", t!("memory_trim.idle_time")),
                    ui.input(
                        self.input(2, settings.process_idle_seconds),
                        "1-86400",
                        Message::Idle,
                    ),
                )),
                _ => {
                    let candidate_settings = settings.clone();
                    controls.push(super::app_picker::view(
                        &self.path,
                        candidates,
                        true,
                        Message::Path,
                        Message::Browse,
                        Some(Message::Add),
                        move |candidate| {
                            can_add_memory_trim_exclusion(
                                &candidate_settings,
                                &candidate.info.image_path.to_string_lossy(),
                            )
                            .then_some(true)
                        },
                        ui.clone(),
                    ));
                    for (index, rule) in settings.exclusions.iter().enumerate() {
                        controls.push(widgets::row().children([
                            ui.checkbox(rule.executable_path.clone(), rule.enabled, move |value| {
                                Message::RuleEnabled(index, value)
                            }),
                            ui.button(t!("common.remove").to_string(), Message::Remove(index)),
                        ]));
                    }
                    if settings.exclusions.is_empty() {
                        controls.push(widgets::label(t!("memory_trim.no_exclusions").to_string()));
                    }
                }
            }
            let help_key = format!("{title}_help");
            body.push(
                ui.collapsible(
                    format!("{} ⓘ", t!(title)),
                    self.expanded[index],
                    widgets::col().children(controls),
                    move |_| Message::Collapse(index),
                )
                .tooltip(t!(&help_key).to_string()),
            );
        }
        widgets::page(body)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_help_resolves_from_native_headers_when_collapsed_or_expanded() {
        use windui::{
            core::Tree,
            geometry::{Point, Size},
            signal::SignalScope,
            text::NullTextEngine,
        };

        for expanded in [false, true] {
            let mut scope = SignalScope::new();
            scope.collect(|| {
                let editor = Editor {
                    expanded: [expanded; 3],
                    ..Default::default()
                };
                let mut tree = Tree::new();
                let root = editor
                    .view(&MemoryTrimSettings::default(), &[], Ui::new(|_, _| {}))
                    .build(&mut tree);
                tree.root = Some(root);
                tree.layout_root(Size::new(900, 760), &mut NullTextEngine);
                for key in [
                    "memory_trim.category_thresholds_help",
                    "memory_trim.category_when_to_trim_help",
                    "memory_trim.category_safety_help",
                ] {
                    let help = t!(key).to_string();
                    let mut pending = vec![root];
                    let owner = loop {
                        let id = pending.pop().expect("Category help must be wired");
                        let node = tree.get(id).unwrap();
                        if node
                            .tooltip
                            .as_ref()
                            .is_some_and(|text| text.resolve() == help)
                        {
                            break id;
                        }
                        pending.extend(node.children.iter().copied());
                    };
                    let bounds = tree.abs_bounds(owner);
                    let hit = tree
                        .hit_test(Point::new(bounds.x + 12, bounds.y + 10))
                        .expect("Native category header must be hittable");
                    assert_ne!(hit, owner);
                    assert_eq!(tree.node_tooltip(hit).as_deref(), Some(help.as_str()));
                }
            });
        }
    }

    #[test]
    fn groups_start_collapsed_and_toggle_without_changing_settings() {
        let mut editor = Editor::default();
        let mut settings = MemoryTrimSettings::default();
        let before = settings.clone();
        assert_eq!(editor.expanded, [false; 3]);
        editor.update(&mut settings, Message::Collapse(0));
        editor.update(&mut settings, Message::Collapse(2));
        assert_eq!(editor.expanded, [true, false, true]);
        editor.update(&mut settings, Message::Collapse(0));
        assert_eq!(editor.expanded, [false, false, true]);
        assert_eq!(settings, before);
    }
    #[test]
    fn invalid_thresholds_and_missing_removals_do_not_mutate_settings() {
        let mut e = Editor::default();
        let mut s = crate::config::Settings::default().memory_trim;
        let before = s.clone();
        e.update(&mut s, Message::Load("0".into()));
        e.update(&mut s, Message::Idle("86401".into()));
        e.update(&mut s, Message::WorkingSet("-1".into()));
        e.update(&mut s, Message::Remove(0));
        assert_eq!(s, before);
        assert!(e.has_invalid_inputs());
        assert_eq!(e.input(0, s.system_memory_load_threshold_percent), "0");
        e.update(&mut s, Message::Load("70".into()));
        e.update(&mut s, Message::WorkingSet("100".into()));
        e.update(&mut s, Message::Idle("60".into()));
        assert!(!e.has_invalid_inputs());
        let path = r"C:\Apps\test.exe";
        s.exclusions.push(new_process_exclusion_rule(path));
        e.update(&mut s, Message::Remove(0));
        assert!(s.exclusions.is_empty());
    }
}
