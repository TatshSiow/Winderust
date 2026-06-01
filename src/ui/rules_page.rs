use eframe::egui;

use crate::{
    config::{ForegroundRule, ForegroundRules},
    power::PowerPlan,
    ui::power_plan_page::{self, PowerPlanAction},
};

const APP_INPUT_WIDTH: f32 = 320.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    None,
    RefreshPlans,
}

pub fn show(
    ui: &mut egui::Ui,
    rules: &mut ForegroundRules,
    plans: &[PowerPlan],
    current_plan: Option<&PowerPlan>,
    process_candidates: &[String],
    picker_open_rule: &mut Option<usize>,
    picker_highlighted: &mut Option<usize>,
) -> RuleAction {
    let mut action = RuleAction::None;

    ui.heading("Foreground Rules");
    ui.add_space(8.0);

    ui.checkbox(&mut rules.enabled, "Enable foreground rules");
    ui.label("Change power plan based on custom foreground rules.");
    ui.add_space(14.0);

    if show_power_plan_source(ui, plans, current_plan) == PowerPlanAction::Refresh {
        action = RuleAction::RefreshPlans;
    }
    ui.add_space(18.0);

    ui.add_enabled_ui(rules.enabled, |ui| {
        if ui.button("Add foreground rule").clicked() {
            rules.rules.push(ForegroundRule {
                enabled: true,
                name: "New Foreground Rule".to_owned(),
                process_name: String::new(),
                power_plan_guid: current_plan.map(|plan| plan.guid.clone()),
            });
        }

        ui.add_space(10.0);

        let mut remove_index = None;
        for index in 0..rules.rules.len() {
            ui.group(|ui| {
                let rule = &mut rules.rules[index];
                ui.horizontal(|ui| {
                    ui.checkbox(&mut rule.enabled, "Enable");
                    ui.label("Name");
                    ui.add_enabled_ui(rule.enabled, |ui| {
                        ui.text_edit_singleline(&mut rule.name);
                    });
                    if ui.button("Remove").clicked() {
                        remove_index = Some(index);
                    }
                });

                ui.add_enabled_ui(rule.enabled, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Focused app");
                        let input_width = ui.available_width().clamp(160.0, APP_INPUT_WIDTH);
                        if let Some(process) = searchable_rule_input(
                            ui,
                            index,
                            process_candidates,
                            &mut rule.process_name,
                            picker_open_rule,
                            picker_highlighted,
                            input_width,
                        ) {
                            rule.process_name = process;
                        }
                    });

                    power_plan_page::plan_combo_with_id(
                        ui,
                        "Target power plan",
                        ("foreground_rule_target", index),
                        &mut rule.power_plan_guid,
                        plans,
                    );
                });
            });

            ui.add_space(10.0);
        }

        if let Some(index) = remove_index {
            rules.rules.remove(index);
            if picker_open_rule.is_some_and(|open_index| open_index == index) {
                *picker_open_rule = None;
                *picker_highlighted = None;
            }
        }
    });

    action
}

fn show_power_plan_source(
    ui: &mut egui::Ui,
    plans: &[PowerPlan],
    current_plan: Option<&PowerPlan>,
) -> PowerPlanAction {
    let mut action = PowerPlanAction::None;

    ui.group(|ui| {
        ui.heading("Power Plans");
        ui.label("Each foreground rule can switch to any Windows power plan.");
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if ui.button("Refresh plans").clicked() {
                action = PowerPlanAction::Refresh;
            }
            ui.label(format!(
                "Current active plan: {}",
                current_plan
                    .map(|plan| plan.name.as_str())
                    .unwrap_or("Unknown")
            ));
            ui.label(format!("Available plans: {}", plans.len()));
        });
    });

    action
}

fn searchable_rule_input(
    ui: &mut egui::Ui,
    rule_index: usize,
    process_candidates: &[String],
    input: &mut String,
    picker_open_rule: &mut Option<usize>,
    picker_highlighted: &mut Option<usize>,
    input_width: f32,
) -> Option<String> {
    const POPUP_WIDTH: f32 = 360.0;
    const LIST_HEIGHT: f32 = 240.0;

    let input_id = ui.make_persistent_id(("foreground_rule_input", rule_index, "input"));
    let popup_id = ui.make_persistent_id(("foreground_rule_input", rule_index, "popup"));

    let input_response = ui
        .add_sized(
            [input_width, ui.spacing().interact_size.y],
            egui::TextEdit::singleline(input)
                .id(input_id)
                .hint_text("Search running apps...")
                .desired_width(input_width),
        )
        .on_hover_text("Search running apps or type an app name");

    if input_response.changed() {
        *picker_highlighted = None;
    }

    if input_response.clicked() || input_response.gained_focus() || input_response.changed() {
        *picker_open_rule = Some(rule_index);
    }

    let picker_is_open = picker_open_rule.is_some_and(|index| index == rule_index);
    let search = input.trim().to_ascii_lowercase();
    let filtered_processes: Vec<&String> = process_candidates
        .iter()
        .filter(|process| search.is_empty() || process.contains(&search))
        .collect();

    if filtered_processes.is_empty() {
        *picker_highlighted = None;
    } else if picker_is_open {
        let highlighted = picker_highlighted
            .unwrap_or(0)
            .min(filtered_processes.len() - 1);
        *picker_highlighted = Some(highlighted);
    }

    let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));
    let picker_has_keyboard =
        input_response.has_focus() || (input_response.lost_focus() && enter_pressed);
    let mut scroll_highlight_into_view = false;
    let mut keyboard_navigated = false;
    let pointer_moved = ui.input(|input| input.pointer.delta() != egui::Vec2::ZERO);

    if picker_is_open && picker_has_keyboard {
        if !filtered_processes.is_empty() {
            let last_index = filtered_processes.len() - 1;
            let highlighted = picker_highlighted.unwrap_or(0).min(last_index);

            let next_highlighted = ui.input_mut(|input| {
                if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    Some((highlighted + 1).min(last_index))
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    Some(highlighted.saturating_sub(1))
                } else {
                    None
                }
            });
            if let Some(next_highlighted) = next_highlighted {
                *picker_highlighted = Some(next_highlighted);
                scroll_highlight_into_view = true;
                keyboard_navigated = true;
            }
        }

        let escape_pressed =
            ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if escape_pressed {
            *picker_open_rule = None;
            return None;
        }

        if enter_pressed && !filtered_processes.is_empty() {
            *picker_open_rule = None;
            *picker_highlighted = None;
            let process = filtered_processes[picker_highlighted.unwrap_or(0)].clone();
            return Some(process);
        }
    }

    let mut picked_process = None;
    let mut popup_open = picker_is_open;
    let popup_response = egui::Popup::from_response(&input_response)
        .id(popup_id)
        .open_bool(&mut popup_open)
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .width(POPUP_WIDTH)
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .show(|ui| {
            ui.set_min_width(POPUP_WIDTH);

            ui.allocate_ui_with_layout(
                egui::vec2(POPUP_WIDTH, LIST_HEIGHT),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(LIST_HEIGHT)
                        .min_scrolled_height(LIST_HEIGHT)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());

                            for (index, process) in filtered_processes.iter().enumerate() {
                                let is_highlighted = picker_highlighted
                                    .is_some_and(|highlighted| highlighted == index);
                                let response =
                                    ui.selectable_label(is_highlighted, process.as_str());
                                if is_highlighted && scroll_highlight_into_view {
                                    response.scroll_to_me(Some(egui::Align::Center));
                                }
                                if response.hovered() && pointer_moved && !keyboard_navigated {
                                    *picker_highlighted = Some(index);
                                }
                                if response.clicked() {
                                    picked_process = Some((*process).clone());
                                    ui.close();
                                }
                            }
                        });

                    if filtered_processes.is_empty() {
                        ui.centered_and_justified(|ui| {
                            ui.label("No matching apps");
                        });
                    }
                },
            );
        });

    if let Some(process) = picked_process {
        *picker_open_rule = None;
        *picker_highlighted = None;
        return Some(process);
    }

    let clicked_elsewhere = popup_response
        .as_ref()
        .is_some_and(|response| response.response.clicked_elsewhere());
    if clicked_elsewhere && !input_response.clicked() && !input_response.gained_focus() {
        *picker_open_rule = None;
    }

    if !popup_open && picker_is_open {
        *picker_open_rule = None;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_foreground_rule_keeps_arbitrary_target_plan() {
        let rule = ForegroundRule {
            enabled: true,
            name: "Editor".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: Some("custom-guid".to_owned()),
        };

        assert_eq!(rule.power_plan_guid.as_deref(), Some("custom-guid"));
    }
}
