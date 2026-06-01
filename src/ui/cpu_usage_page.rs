use eframe::egui;

use crate::{
    config::{CpuUsageComparison, CpuUsageModeSettings, CpuUsageRule},
    power::PowerPlan,
    ui::power_plan_page::{self, PowerPlanAction},
};

pub fn show(
    ui: &mut egui::Ui,
    cpu_usage: &mut CpuUsageModeSettings,
    plans: &[PowerPlan],
    current_plan: Option<&PowerPlan>,
) -> PowerPlanAction {
    let mut action = PowerPlanAction::None;

    ui.heading("CPU Load Rules");
    ui.add_space(8.0);
    ui.checkbox(&mut cpu_usage.enabled, "Enable CPU load rules");
    ui.label("Change power plan when a CPU load rule is active.");
    ui.add_space(14.0);

    if show_power_plan_source(ui, plans, current_plan) == PowerPlanAction::Refresh {
        action = PowerPlanAction::Refresh;
    }
    ui.add_space(18.0);

    ui.add_enabled_ui(cpu_usage.enabled, |ui| {
        if ui.button("Add CPU load rule").clicked() {
            cpu_usage.rules.push(CpuUsageRule {
                name: "New CPU Load Rule".to_owned(),
                comparison: CpuUsageComparison::AtOrBelow,
                threshold_percent: 20,
                upper_threshold_percent: None,
                duration_seconds: 30,
                power_plan_guid: current_plan.map(|plan| plan.guid.clone()),
                else_enabled: false,
                else_power_plan_guid: current_plan.map(|plan| plan.guid.clone()),
                target: None,
            });
        }

        ui.add_space(10.0);

        let mut remove_index = None;
        for (index, rule) in cpu_usage.rules.iter_mut().enumerate() {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut rule.name);
                    if ui.button("Remove").clicked() {
                        remove_index = Some(index);
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("When CPU load");
                    egui::ComboBox::from_id_salt(("cpu_comparison", index))
                        .selected_text(rule.comparison.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut rule.comparison,
                                CpuUsageComparison::AtOrBelow,
                                CpuUsageComparison::AtOrBelow.label(),
                            );
                            ui.selectable_value(
                                &mut rule.comparison,
                                CpuUsageComparison::AtOrAbove,
                                CpuUsageComparison::AtOrAbove.label(),
                            );
                            ui.selectable_value(
                                &mut rule.comparison,
                                CpuUsageComparison::Between,
                                CpuUsageComparison::Between.label(),
                            );
                        });
                    show_cpu_condition_inputs(ui, rule);
                    ui.label("for");
                    ui.add(
                        egui::DragValue::new(&mut rule.duration_seconds)
                            .speed(1.0)
                            .range(0..=86_400)
                            .suffix(" sec"),
                    );
                });

                power_plan_page::plan_combo_with_id(
                    ui,
                    "Use",
                    ("cpu_load_rule_target", index),
                    &mut rule.power_plan_guid,
                    plans,
                );

                ui.horizontal(|ui| {
                    if ui.checkbox(&mut rule.else_enabled, "Else").changed()
                        && rule.else_enabled
                        && rule.else_power_plan_guid.is_none()
                    {
                        rule.else_power_plan_guid = current_plan.map(|plan| plan.guid.clone());
                    }

                    ui.add_enabled_ui(rule.else_enabled, |ui| {
                        power_plan_page::plan_combo_with_id(
                            ui,
                            "Use",
                            ("cpu_load_rule_else_target", index),
                            &mut rule.else_power_plan_guid,
                            plans,
                        );
                    });
                });
            });

            ui.add_space(10.0);
        }

        if let Some(index) = remove_index {
            cpu_usage.rules.remove(index);
        }
    });

    action
}

fn show_cpu_condition_inputs(ui: &mut egui::Ui, rule: &mut CpuUsageRule) {
    match rule.comparison {
        CpuUsageComparison::AtOrBelow => {
            percent_input(ui, &mut rule.threshold_percent);
        }
        CpuUsageComparison::AtOrAbove => {
            percent_input(ui, &mut rule.threshold_percent);
        }
        CpuUsageComparison::Between => {
            rule.upper_threshold_percent.get_or_insert(100);
            percent_input(ui, &mut rule.threshold_percent);
            ui.label("and");
            if let Some(upper) = &mut rule.upper_threshold_percent {
                percent_input(ui, upper);
            }
        }
        CpuUsageComparison::Else => {
            percent_input(ui, &mut rule.threshold_percent);
        }
    }
}

fn percent_input(ui: &mut egui::Ui, value: &mut u8) {
    ui.add(
        egui::DragValue::new(value)
            .speed(1.0)
            .range(0..=100)
            .suffix("%"),
    );
}

fn show_power_plan_source(
    ui: &mut egui::Ui,
    plans: &[PowerPlan],
    current_plan: Option<&PowerPlan>,
) -> PowerPlanAction {
    let mut action = PowerPlanAction::None;

    ui.group(|ui| {
        ui.heading("Power Plans");
        ui.label("Each CPU load rule can switch to any Windows power plan.");
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
