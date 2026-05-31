use eframe::egui;

use crate::config::ForegroundRules;

const APP_INPUT_WIDTH: f32 = 320.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    None,
}

pub fn show(
    ui: &mut egui::Ui,
    rules: &mut ForegroundRules,
    foreground_app: Option<&str>,
    process_candidates: &[String],
    whitelist_input: &mut String,
    whitelist_picker_open: &mut bool,
    whitelist_picker_highlighted: &mut Option<usize>,
    force_input: &mut String,
    force_picker_open: &mut bool,
    force_picker_highlighted: &mut Option<usize>,
) -> RuleAction {
    ui.heading("Foreground Rules");
    ui.add_space(8.0);
    ui.label(format!(
        "Current foreground app: {}",
        foreground_app.unwrap_or("Unknown")
    ));
    ui.add_space(14.0);

    ui.checkbox(&mut rules.enabled, "Enable foreground rules");
    ui.label(
        "When enabled, focused apps in these lists can override scheduler and activity decisions.",
    );
    ui.add_space(14.0);

    ui.add_enabled_ui(rules.enabled, |ui| {
        ui.columns(2, |columns| {
            rule_list(
                &mut columns[0],
                "Force Active Plan",
                "Switch to the Active plan while these apps are focused.",
                &mut rules.whitelist,
                &rules.force_power_save,
                whitelist_input,
                "force_active_rule_input",
                process_candidates,
                whitelist_picker_open,
                whitelist_picker_highlighted,
            );

            rule_list(
                &mut columns[1],
                "Force Idle Plan",
                "Switch to the Idle plan while these apps are focused.",
                &mut rules.force_power_save,
                &rules.whitelist,
                force_input,
                "force_idle_rule_input",
                process_candidates,
                force_picker_open,
                force_picker_highlighted,
            );
        });
    });

    let conflicts: Vec<_> = rules
        .whitelist
        .iter()
        .filter(|entry| {
            rules
                .force_power_save
                .iter()
                .any(|force| force.eq_ignore_ascii_case(entry))
        })
        .cloned()
        .collect();

    if !conflicts.is_empty() {
        ui.add_space(12.0);
        ui.colored_label(
            egui::Color32::from_rgb(190, 90, 40),
            format!(
                "Conflict: {:?} exists in both lists. Force Idle Plan wins.",
                conflicts
            ),
        );
    }

    RuleAction::None
}

fn rule_list(
    ui: &mut egui::Ui,
    title: &str,
    hint: &str,
    list: &mut Vec<String>,
    other_list: &[String],
    input: &mut String,
    input_id_salt: &'static str,
    process_candidates: &[String],
    picker_open: &mut bool,
    picker_highlighted: &mut Option<usize>,
) {
    ui.group(|ui| {
        ui.heading(title);
        ui.label(hint);
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            let add_button_width = 58.0;
            let input_width =
                (ui.available_width() - add_button_width - ui.spacing().item_spacing.x)
                    .clamp(160.0, APP_INPUT_WIDTH);

            if let Some(process) = searchable_rule_input(
                ui,
                input_id_salt,
                process_candidates,
                list,
                other_list,
                input,
                picker_open,
                picker_highlighted,
                input_width,
            ) {
                *input = process;
            }

            let add_size = egui::vec2(add_button_width, ui.spacing().interact_size.y);
            if ui
                .add_enabled(
                    can_add_process(list, other_list, input),
                    egui::Button::new("Add").min_size(add_size),
                )
                .clicked()
            {
                add_process(list, other_list, input);
                *picker_open = false;
                *picker_highlighted = None;
            }
        });

        ui.separator();

        let mut remove_index = None;
        for (index, item) in list.iter().enumerate() {
            ui.horizontal(|ui| {
                let button_width = 74.0;
                let label_width =
                    (ui.available_width() - button_width - ui.spacing().item_spacing.x).max(80.0);
                ui.add_sized(
                    [label_width, ui.spacing().interact_size.y],
                    egui::Label::new(item).truncate(),
                );
                if ui
                    .add_sized(
                        [button_width, ui.spacing().interact_size.y],
                        egui::Button::new("Remove"),
                    )
                    .clicked()
                {
                    remove_index = Some(index);
                }
            });
        }

        if let Some(index) = remove_index {
            list.remove(index);
        }
    });
}

fn searchable_rule_input(
    ui: &mut egui::Ui,
    id_salt: &'static str,
    process_candidates: &[String],
    current_list: &[String],
    other_list: &[String],
    input: &mut String,
    picker_open: &mut bool,
    picker_highlighted: &mut Option<usize>,
    input_width: f32,
) -> Option<String> {
    const POPUP_WIDTH: f32 = 360.0;
    const LIST_HEIGHT: f32 = 240.0;

    let input_id = ui.make_persistent_id((id_salt, "input"));
    let popup_id = ui.make_persistent_id((id_salt, "popup"));

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
        *picker_open = true;
    }

    let search = input.trim().to_ascii_lowercase();
    let filtered_processes: Vec<&String> = process_candidates
        .iter()
        .filter(|process| {
            (search.is_empty() || process.contains(&search))
                && !contains_process(current_list, process)
                && !contains_process(other_list, process)
        })
        .collect();

    if filtered_processes.is_empty() {
        *picker_highlighted = None;
    } else {
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

    if *picker_open && picker_has_keyboard {
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
            *picker_open = false;
            return None;
        }

        if enter_pressed && !filtered_processes.is_empty() {
            *picker_open = false;
            *picker_highlighted = None;
            let process = filtered_processes[picker_highlighted.unwrap_or(0)].clone();
            return Some(process);
        }
    }

    let mut picked_process = None;
    let popup_response = egui::Popup::from_response(&input_response)
        .id(popup_id)
        .open_bool(picker_open)
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
        *picker_open = false;
        *picker_highlighted = None;
        return Some(process);
    }

    let clicked_elsewhere = popup_response
        .as_ref()
        .is_some_and(|response| response.response.clicked_elsewhere());
    if clicked_elsewhere && !input_response.clicked() && !input_response.gained_focus() {
        *picker_open = false;
    }

    None
}

fn add_process(list: &mut Vec<String>, other_list: &[String], input: &mut String) {
    add_process_name(list, other_list, input);
    input.clear();
}

fn add_process_name(list: &mut Vec<String>, other_list: &[String], process: &str) {
    if !can_add_process(list, other_list, process) {
        return;
    }

    list.push(process.trim().to_ascii_lowercase());
}

fn can_add_process(list: &[String], other_list: &[String], process: &str) -> bool {
    let process = process.trim();
    !process.is_empty()
        && !contains_process(list, process)
        && !contains_process(other_list, process)
}

fn contains_process(list: &[String], process: &str) -> bool {
    list.iter()
        .any(|item| item.trim().eq_ignore_ascii_case(process.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_process_rejects_duplicates_across_both_lists() {
        let mut list = vec!["game.exe".to_owned()];
        let other_list = vec!["backup.exe".to_owned()];
        let mut input = "BACKUP.EXE".to_owned();

        add_process(&mut list, &other_list, &mut input);

        assert_eq!(list, vec!["game.exe"]);
        assert!(input.is_empty());
    }

    #[test]
    fn add_process_normalizes_manual_entry() {
        let mut list = Vec::new();
        let other_list = Vec::new();
        let mut input = "  Editor.EXE  ".to_owned();

        add_process(&mut list, &other_list, &mut input);

        assert_eq!(list, vec!["editor.exe"]);
        assert!(input.is_empty());
    }
}
