use eframe::egui;

use crate::{
    config::{ScheduleModeSettings, ScheduleRule, WeekdaySetting},
    power::PowerPlan,
    ui::power_plan_page::{self, PowerPlanAction},
};

pub fn show(
    ui: &mut egui::Ui,
    schedule: &mut ScheduleModeSettings,
    plans: &[PowerPlan],
    current_plan: Option<&PowerPlan>,
) -> PowerPlanAction {
    let mut action = PowerPlanAction::None;

    ui.heading("Time Based Scheduler");
    ui.add_space(8.0);
    ui.checkbox(&mut schedule.enabled, "Enable Time Based Scheduler");
    ui.label("Change power plan based on scheduled time.");
    ui.add_space(14.0);

    if power_plan_page::show_section(
        ui,
        "Power Plans",
        "Used when this page switches between idle and active periods.",
        &mut schedule.power_plans,
        plans,
        current_plan,
    ) == PowerPlanAction::Refresh
    {
        action = PowerPlanAction::Refresh;
    }
    ui.add_space(18.0);

    ui.add_enabled_ui(schedule.enabled, |ui| {
        if ui.button("Add schedule rule").clicked() {
            schedule.rules.push(ScheduleRule {
                name: "New Schedule".to_owned(),
                days: WeekdaySetting::all().to_vec(),
                start_time: "22:00".to_owned(),
                end_time: "08:00".to_owned(),
                power_save_guid: None,
                performance_guid: None,
            });
        }

        ui.add_space(10.0);

        let mut remove_index = None;
        for (index, rule) in schedule.rules.iter_mut().enumerate() {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut rule.name);
                    if ui.button("Remove").clicked() {
                        remove_index = Some(index);
                    }
                });

                ui.horizontal_wrapped(|ui| {
                    ui.label("Days");
                    for day in WeekdaySetting::all() {
                        let mut selected = rule.days.contains(&day);
                        if ui.toggle_value(&mut selected, day.short_label()).changed() {
                            if selected {
                                rule.days.push(day);
                            } else {
                                rule.days.retain(|existing| *existing != day);
                            }
                        }
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Start");
                    ui.text_edit_singleline(&mut rule.start_time);
                    ui.label("End");
                    ui.text_edit_singleline(&mut rule.end_time);
                    if rule.parsed_times().is_none() {
                        ui.colored_label(egui::Color32::from_rgb(190, 60, 50), "Use HH:MM");
                    }
                });
            });

            ui.add_space(10.0);
        }

        if let Some(index) = remove_index {
            schedule.rules.remove(index);
        }
    });

    action
}
