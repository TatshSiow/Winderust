use eframe::egui;

use crate::{config::EcoQosSettings, ecoqos, ui::help_popup_label};

const APP_INPUT_WIDTH: f32 = 320.0;

pub fn show(
    ui: &mut egui::Ui,
    settings: &mut EcoQosSettings,
    status: &ecoqos::EcoQosSnapshot,
    process_candidates: &[String],
    exclusion_input: &mut String,
    picker_open: &mut bool,
    picker_highlighted: &mut Option<usize>,
) {
    ui.horizontal(|ui| {
        help_marker(ui);
    });
    ui.add_space(8.0);

    ui.checkbox(&mut settings.enabled, "Enable Windows EcoQoS");
    ui.label("Set app to efficiency mode and lower the process priority.");
    ui.checkbox(
        &mut settings.exclude_foreground_app,
        "Exclude foreground app",
    );
    ui.checkbox(
        &mut settings.exclude_suspended_processes,
        "Exclude Windows-suspended apps",
    );
    ui.label("Skip apps that Windows already reports as suspended.");
    ui.add_space(12.0);

    egui::Grid::new("eco_qos_status_grid")
        .num_columns(2)
        .spacing([24.0, 10.0])
        .striped(true)
        .show(ui, |ui| {
            row(ui, "Status", &status.message);
            row(
                ui,
                "Throttled processes",
                &status.throttled_processes.to_string(),
            );
            row(
                ui,
                "Scanned processes",
                &status.scanned_processes.to_string(),
            );
            row(
                ui,
                "Skipped processes",
                &status.skipped_processes.to_string(),
            );
            row(ui, "Failed actions", &status.failed_processes.to_string());
            row(
                ui,
                "Last failure",
                status.last_error.as_deref().unwrap_or("None"),
            );
        });

    ui.add_space(18.0);
    ui.separator();
    ui.add_space(10.0);

    ui.add_enabled_ui(settings.enabled, |ui| {
        ui.group(|ui| {
            ui.heading("Efficiency Whitelist");
            ui.label("Apps in this whitelist will never be put into Efficiency Mode.");
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                let add_button_width = 58.0;
                let input_width =
                    (ui.available_width() - add_button_width - ui.spacing().item_spacing.x)
                        .clamp(160.0, APP_INPUT_WIDTH);

                if let Some(process) = searchable_exclusion_input(
                    ui,
                    process_candidates,
                    settings,
                    exclusion_input,
                    picker_open,
                    picker_highlighted,
                    input_width,
                ) {
                    *exclusion_input = process;
                }

                let add_size = egui::vec2(add_button_width, ui.spacing().interact_size.y);
                if ui
                    .add_enabled(
                        can_add_process(settings, exclusion_input),
                        egui::Button::new("Add").min_size(add_size),
                    )
                    .clicked()
                {
                    add_process(settings, exclusion_input);
                    *picker_open = false;
                    *picker_highlighted = None;
                }
            });

            ui.separator();
            show_efficiency_whitelist(ui, settings);
        });
    });
}

fn help_marker(ui: &mut egui::Ui) {
    help_popup_label(
        ui,
        "Efficiency Mode",
        "efficiency_mode_help_popup",
        help_contents,
    );
}

fn help_contents(ui: &mut egui::Ui) {
    ui.set_max_width(360.0);
    ui.label("Efficiency Mode applies Windows EcoQoS to background apps to reduce CPU power use.");
    ui.label(
        "PowerLeaf also lowers the target app's process priority while Efficiency Mode is active, then restores the previous state when the app is no longer targeted.",
    );
    ui.label(
        "This is safer than App Suspension because apps keep running, but Windows may schedule them more efficiently.",
    );
}

fn row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.strong(label);
    ui.label(value);
    ui.end_row();
}

fn show_efficiency_whitelist(ui: &mut egui::Ui, settings: &mut EcoQosSettings) {
    let mut remove_index = None;
    for (index, process) in settings.efficiency_whitelist.iter().enumerate() {
        ui.horizontal(|ui| {
            let button_width = 74.0;
            let label_width =
                (ui.available_width() - button_width - ui.spacing().item_spacing.x).max(80.0);
            ui.add_sized(
                [label_width, ui.spacing().interact_size.y],
                egui::Label::new(process).truncate(),
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
        settings.efficiency_whitelist.remove(index);
    }
}

fn searchable_exclusion_input(
    ui: &mut egui::Ui,
    process_candidates: &[String],
    settings: &EcoQosSettings,
    input: &mut String,
    picker_open: &mut bool,
    picker_highlighted: &mut Option<usize>,
    input_width: f32,
) -> Option<String> {
    const POPUP_WIDTH: f32 = 360.0;
    const LIST_HEIGHT: f32 = 240.0;

    let input_id = ui.make_persistent_id(("eco_qos_exclusion_input", "input"));
    let popup_id = ui.make_persistent_id(("eco_qos_exclusion_input", "popup"));

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
                && !ecoqos::is_process_excluded(process, settings)
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

fn add_process(settings: &mut EcoQosSettings, input: &mut String) {
    add_process_name(settings, input);
    input.clear();
}

fn add_process_name(settings: &mut EcoQosSettings, process: &str) {
    if can_add_process(settings, process) {
        settings
            .efficiency_whitelist
            .push(process.trim().to_ascii_lowercase());
    }
}

fn can_add_process(settings: &EcoQosSettings, process: &str) -> bool {
    let process = process.trim();
    !process.is_empty() && !ecoqos::is_process_excluded(process, settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_process_rejects_builtin_and_duplicate_entries() {
        let mut settings = EcoQosSettings {
            enabled: true,
            exclude_foreground_app: true,
            exclude_suspended_processes: false,
            efficiency_whitelist: vec!["mouse.exe".to_owned()],
        };

        add_process_name(&mut settings, "explorer.exe");
        add_process_name(&mut settings, "MOUSE.EXE");
        add_process_name(&mut settings, "browser.exe");

        assert_eq!(
            settings.efficiency_whitelist,
            vec!["mouse.exe", "browser.exe"]
        );
    }
}
