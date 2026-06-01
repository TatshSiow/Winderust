use eframe::egui;

use crate::{
    config::{AppSuspensionRule, AppSuspensionSettings, NetworkThresholdUnit},
    suspension::{self, AppSuspensionSnapshot},
    ui::help_popup_label,
};

const APP_INPUT_WIDTH: f32 = 320.0;
const MAX_NETWORK_THRESHOLD_BYTES: u64 = 1_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuspensionPageAction {
    None,
    ManualFreeze(String),
}

pub fn show(
    ui: &mut egui::Ui,
    settings: &mut AppSuspensionSettings,
    status: &AppSuspensionSnapshot,
    process_candidates: &[String],
    suspend_input: &mut String,
    picker_open: &mut bool,
    picker_highlighted: &mut Option<usize>,
) -> SuspensionPageAction {
    let mut action = SuspensionPageAction::None;

    ui.horizontal(|ui| {
        help_marker(ui);
    });
    ui.add_space(8.0);

    ui.checkbox(&mut settings.enabled, "Enable app suspension");
    ui.label("Completely suspend an app after a delay.");
    ui.add_enabled_ui(settings.enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("Background delay");
            ui.add(
                egui::DragValue::new(&mut settings.background_delay_seconds)
                    .speed(1.0)
                    .range(1..=86_400)
                    .suffix(" sec"),
            );
        });
        ui.checkbox(
            &mut settings.temporary_thaw_enabled,
            "Temporary thaw fallback",
        );
        ui.add_enabled_ui(settings.temporary_thaw_enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label("Thaw every");
                ui.add(
                    egui::DragValue::new(&mut settings.temporary_thaw_interval_seconds)
                        .speed(1.0)
                        .range(1..=86_400)
                        .suffix(" sec"),
                );
                ui.label("for");
                ui.add(
                    egui::DragValue::new(&mut settings.temporary_thaw_duration_seconds)
                        .speed(1.0)
                        .range(1..=3_600)
                        .suffix(" sec"),
                );
            });
        });
        ui.checkbox(&mut settings.audio_wake_enabled, "Audio playback detection");
        ui.add_enabled_ui(settings.audio_wake_enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label("Refreeze after");
                ui.add(
                    egui::DragValue::new(&mut settings.audio_wake_duration_seconds)
                        .speed(1.0)
                        .range(1..=3_600)
                        .suffix(" sec"),
                );
                ui.label("quiet");
            });
        });
        ui.checkbox(
            &mut settings.network_wake_enabled,
            "Network intent detection",
        );
        ui.add_enabled_ui(settings.network_wake_enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label("Refreeze after");
                ui.add(
                    egui::DragValue::new(&mut settings.network_wake_duration_seconds)
                        .speed(1.0)
                        .range(1..=3_600)
                        .suffix(" sec"),
                );
                ui.label("quiet");
            });
        });
    });
    ui.add_space(12.0);

    egui::Grid::new("app_suspension_status_grid")
        .num_columns(2)
        .spacing([24.0, 10.0])
        .striped(true)
        .show(ui, |ui| {
            row(ui, "Status", &status.message);
            row(
                ui,
                "Tracked processes",
                &status.tracked_processes.to_string(),
            );
            row(
                ui,
                "Suspended processes",
                &status.suspended_processes.to_string(),
            );
            row(
                ui,
                "Temporary thawed",
                &status.temporary_thawed_processes.to_string(),
            );
            row(
                ui,
                "Network wake",
                &status.network_wake_processes.to_string(),
            );
            row(ui, "Audio wake", &status.audio_wake_processes.to_string());
            row(
                ui,
                "Skipped processes",
                &status.skipped_processes.to_string(),
            );
            row(ui, "Failed actions", &status.failed_actions.to_string());
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
            ui.heading("Suspendable Apps");
            ui.label("Only apps in this list can be suspended after the background delay.");
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                let add_button_width = 58.0;
                let input_width =
                    (ui.available_width() - add_button_width - ui.spacing().item_spacing.x)
                        .clamp(160.0, APP_INPUT_WIDTH);

                if let Some(process) = searchable_suspend_input(
                    ui,
                    process_candidates,
                    settings,
                    suspend_input,
                    picker_open,
                    picker_highlighted,
                    input_width,
                ) {
                    *suspend_input = process;
                }

                let add_size = egui::vec2(add_button_width, ui.spacing().interact_size.y);
                if ui
                    .add_enabled(
                        can_add_process(settings, suspend_input),
                        egui::Button::new("Add").min_size(add_size),
                    )
                    .clicked()
                {
                    add_process(settings, suspend_input);
                    *picker_open = false;
                    *picker_highlighted = None;
                }
            });

            ui.separator();
            if let Some(process_name) = show_suspendable_apps(ui, settings, status) {
                action = SuspensionPageAction::ManualFreeze(process_name);
            }
        });
    });

    action
}

fn help_marker(ui: &mut egui::Ui) {
    help_popup_label(
        ui,
        "App Suspension",
        "app_suspension_help_popup",
        help_contents,
    );
}

fn help_contents(ui: &mut egui::Ui) {
    ui.set_max_width(360.0);
    ui.label("App Suspension pauses selected background apps after a delay to reduce CPU usage.");
    ui.label(
        "Suspended apps are resumed automatically when you switch back to them, remove them from Suspendable Apps, disable App Suspension, or quit PowerLeaf.",
    );
    ui.label(
        "This is more aggressive than Efficiency Mode. Use it only for apps that are safe to pause in the background.",
    );
}

fn row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.strong(label);
    ui.label(value);
    ui.end_row();
}

fn show_suspendable_apps(
    ui: &mut egui::Ui,
    settings: &mut AppSuspensionSettings,
    status: &AppSuspensionSnapshot,
) -> Option<String> {
    let mut remove_index = None;
    let mut manual_freeze = None;

    egui::Grid::new("app_suspension_rules_grid")
        .num_columns(8)
        .spacing([12.0, 8.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("State");
            ui.strong("Process");
            ui.strong("Audio Detection");
            ui.strong("Network Detection");
            ui.strong("Download Threshold");
            ui.strong("Upload Threshold");
            ui.strong("Manual Freeze");
            ui.strong("Remove");
            ui.end_row();

            for (index, rule) in settings.suspendable_apps.iter_mut().enumerate() {
                let process = rule.process_name.clone();
                let indicator = suspension_indicator(status, &process);

                ui.add_sized(
                    [108.0, ui.spacing().interact_size.y],
                    egui::Label::new(
                        egui::RichText::new(indicator.label)
                            .color(indicator.color)
                            .strong(),
                    )
                    .truncate(),
                )
                .on_hover_text(indicator.hover);
                ui.add_sized(
                    [220.0, ui.spacing().interact_size.y],
                    egui::Label::new(process.as_str()).truncate(),
                );
                ui.add(egui::Checkbox::without_text(&mut rule.network_wake_enabled))
                    .on_hover_text(
                        "Allow inbound network activity to temporarily thaw this app when the watcher is enabled.",
                    );
                ui.add(egui::Checkbox::without_text(&mut rule.audio_wake_enabled))
                    .on_hover_text(
                        "Keep this app awake while Windows reports active audio playback.",
                    );
                network_threshold_editor(
                    ui,
                    rule.network_wake_enabled,
                    ("download_threshold", index),
                    &mut rule.network_download_threshold_bytes,
                    &mut rule.network_download_threshold_unit,
                )
                .on_hover_text("Wake when downloaded traffic since the previous watcher tick meets this threshold. Unlimited disables download detection.");
                network_threshold_editor(
                    ui,
                    rule.network_wake_enabled,
                    ("upload_threshold", index),
                    &mut rule.network_upload_threshold_bytes,
                    &mut rule.network_upload_threshold_unit,
                )
                .on_hover_text("Wake when uploaded traffic since the previous watcher tick meets this threshold. Unlimited disables upload detection.");
                if ui
                    .add_enabled(
                        can_manual_freeze(status, &process),
                        egui::Button::new("Freeze"),
                    )
                    .on_hover_text("Freeze this app now if it is running in the background.")
                    .clicked()
                {
                    manual_freeze = Some(process.clone());
                }
                if ui.button("Remove").clicked() {
                    remove_index = Some(index);
                }
                ui.end_row();
            }
        });

    if let Some(index) = remove_index {
        settings.suspendable_apps.remove(index);
    }

    manual_freeze
}

struct SuspensionIndicator {
    label: &'static str,
    color: egui::Color32,
    hover: &'static str,
}

fn suspension_indicator(status: &AppSuspensionSnapshot, process: &str) -> SuspensionIndicator {
    if suspension::is_builtin_excluded(process) {
        SuspensionIndicator {
            label: "Protected",
            color: egui::Color32::from_rgb(80, 135, 190),
            hover: "PowerLeaf does not suspend this app because it can fail to restore correctly.",
        }
    } else if suspension::contains_process(&status.network_wake_apps, process) {
        SuspensionIndicator {
            label: "Network",
            color: egui::Color32::from_rgb(80, 135, 190),
            hover:
                "PowerLeaf has thawed or kept this app awake because it owns network connections.",
        }
    } else if suspension::contains_process(&status.audio_wake_apps, process) {
        SuspensionIndicator {
            label: "Audio",
            color: egui::Color32::from_rgb(80, 135, 190),
            hover: "PowerLeaf has thawed or kept this app awake because it is playing audio.",
        }
    } else if suspension::contains_process(&status.suspended_apps, process) {
        SuspensionIndicator {
            label: "Frozen",
            color: egui::Color32::from_rgb(75, 155, 90),
            hover: "PowerLeaf has frozen this app with Windows Job Object freeze.",
        }
    } else if suspension::contains_process(&status.temporary_thawed_apps, process) {
        SuspensionIndicator {
            label: "Thawed",
            color: egui::Color32::from_rgb(80, 135, 190),
            hover: "PowerLeaf has temporarily thawed this app before freezing it again.",
        }
    } else if suspension::contains_process(&status.tracked_apps, process) {
        SuspensionIndicator {
            label: "Waiting",
            color: egui::Color32::from_rgb(190, 140, 40),
            hover: "This app is in the background and waiting for the delay.",
        }
    } else if status.enabled {
        SuspensionIndicator {
            label: "Not suspended",
            color: egui::Color32::from_gray(130),
            hover: "PowerLeaf is not currently suspending this app.",
        }
    } else {
        SuspensionIndicator {
            label: "Off",
            color: egui::Color32::from_gray(120),
            hover: "App Suspension is disabled.",
        }
    }
}

fn can_manual_freeze(status: &AppSuspensionSnapshot, process: &str) -> bool {
    status.enabled && !suspension::contains_process(&status.suspended_apps, process)
}

fn network_threshold_editor(
    ui: &mut egui::Ui,
    enabled: bool,
    id_salt: impl std::hash::Hash,
    threshold_bytes: &mut u64,
    unit: &mut NetworkThresholdUnit,
) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;

            let mut threshold_value = (*unit).threshold_value_from_bytes(*threshold_bytes);
            let max_threshold_value = (*unit)
                .threshold_value_from_bytes(MAX_NETWORK_THRESHOLD_BYTES)
                .max(1.0);

            if ui
                .add_sized(
                    [74.0, ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut threshold_value)
                        .speed(network_threshold_drag_speed(*unit))
                        .range(0.0..=max_threshold_value)
                        .max_decimals(3)
                        .custom_formatter(network_threshold_formatter)
                        .custom_parser(network_threshold_parser),
                )
                .changed()
            {
                *threshold_bytes = (*unit)
                    .threshold_bytes_from_value(threshold_value)
                    .min(MAX_NETWORK_THRESHOLD_BYTES);
            }

            egui::ComboBox::from_id_salt(id_salt)
                .selected_text(unit.label())
                .width(54.0)
                .show_ui(ui, |ui| {
                    for option in NetworkThresholdUnit::ALL {
                        ui.selectable_value(unit, option, option.label());
                    }
                });
        });
    })
    .response
}

fn network_threshold_formatter(value: f64, decimals: std::ops::RangeInclusive<usize>) -> String {
    if value <= 0.0 {
        "Unlimited".to_owned()
    } else {
        let max_decimals = *decimals.end();
        format!("{value:.max_decimals$}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    }
}

fn network_threshold_parser(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("unlimited") {
        Some(0.0)
    } else {
        value.parse().ok()
    }
}

fn network_threshold_drag_speed(unit: NetworkThresholdUnit) -> f64 {
    match unit {
        NetworkThresholdUnit::Bytes => 64.0,
        NetworkThresholdUnit::Kilobytes | NetworkThresholdUnit::Kilobits => 1.0,
        NetworkThresholdUnit::Megabytes | NetworkThresholdUnit::Megabits => 0.1,
        NetworkThresholdUnit::Gigabytes | NetworkThresholdUnit::Gigabits => 0.01,
        NetworkThresholdUnit::Bits => 512.0,
    }
}

fn searchable_suspend_input(
    ui: &mut egui::Ui,
    process_candidates: &[String],
    settings: &AppSuspensionSettings,
    input: &mut String,
    picker_open: &mut bool,
    picker_highlighted: &mut Option<usize>,
    input_width: f32,
) -> Option<String> {
    const POPUP_WIDTH: f32 = 360.0;
    const LIST_HEIGHT: f32 = 240.0;

    let input_id = ui.make_persistent_id(("app_suspension_input", "input"));
    let popup_id = ui.make_persistent_id(("app_suspension_input", "popup"));

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
                && !settings.contains_suspendable_app(process)
                && !suspension::is_builtin_excluded(process)
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

fn add_process(settings: &mut AppSuspensionSettings, input: &mut String) {
    add_process_name(settings, input);
    input.clear();
}

fn add_process_name(settings: &mut AppSuspensionSettings, process: &str) {
    if can_add_process(settings, process) {
        settings.suspendable_apps.push(AppSuspensionRule {
            process_name: process.trim().to_ascii_lowercase(),
            network_wake_enabled: true,
            audio_wake_enabled: true,
            network_download_threshold_bytes: 1,
            network_download_threshold_unit: NetworkThresholdUnit::Bytes,
            network_upload_threshold_bytes: 0,
            network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
        });
    }
}

fn can_add_process(settings: &AppSuspensionSettings, process: &str) -> bool {
    let process = process.trim();
    !process.is_empty()
        && !settings.contains_suspendable_app(process)
        && !suspension::is_builtin_excluded(process)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_process_rejects_builtin_and_duplicate_entries() {
        let mut settings = AppSuspensionSettings {
            enabled: true,
            background_delay_seconds: 60,
            temporary_thaw_enabled: false,
            temporary_thaw_interval_seconds: 900,
            temporary_thaw_duration_seconds: 20,
            network_wake_enabled: false,
            network_wake_duration_seconds: 30,
            audio_wake_enabled: false,
            audio_wake_duration_seconds: 10,
            suspendable_apps: vec![AppSuspensionRule {
                process_name: "chat.exe".to_owned(),
                network_wake_enabled: true,
                audio_wake_enabled: true,
                network_download_threshold_bytes: 1,
                network_download_threshold_unit: NetworkThresholdUnit::Bytes,
                network_upload_threshold_bytes: 0,
                network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
            }],
        };

        add_process_name(&mut settings, "explorer.exe");
        add_process_name(&mut settings, "CHAT.EXE");
        add_process_name(&mut settings, "browser.exe");

        assert_eq!(
            settings.suspendable_apps,
            vec![
                AppSuspensionRule {
                    process_name: "chat.exe".to_owned(),
                    network_wake_enabled: true,
                    audio_wake_enabled: true,
                    network_download_threshold_bytes: 1,
                    network_download_threshold_unit: NetworkThresholdUnit::Bytes,
                    network_upload_threshold_bytes: 0,
                    network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
                },
                AppSuspensionRule {
                    process_name: "browser.exe".to_owned(),
                    network_wake_enabled: true,
                    audio_wake_enabled: true,
                    network_download_threshold_bytes: 1,
                    network_download_threshold_unit: NetworkThresholdUnit::Bytes,
                    network_upload_threshold_bytes: 0,
                    network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
                }
            ]
        );
    }

    #[test]
    fn network_threshold_zero_formats_as_unlimited() {
        assert_eq!(network_threshold_formatter(0.0, 0..=3), "Unlimited");
        assert_eq!(network_threshold_parser("Unlimited"), Some(0.0));
        assert_eq!(network_threshold_parser("12.5"), Some(12.5));
    }
}
