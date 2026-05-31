use eframe::egui;

use crate::{
    config::{CpuUsageComparison, CpuUsageModeSettings, CpuUsageRule, CpuUsageTarget},
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

    ui.heading("CPU usage-based Scheduler");
    ui.add_space(8.0);
    ui.checkbox(&mut cpu_usage.enabled, "Enable CPU usage-based Scheduler");
    ui.label("Change power plan based on CPU usage level.");
    ui.add_space(14.0);

    if power_plan_page::show_section(
        ui,
        "Power Plans",
        "Used when this page switches based on CPU usage rules.",
        &mut cpu_usage.power_plans,
        plans,
        current_plan,
    ) == PowerPlanAction::Refresh
    {
        action = PowerPlanAction::Refresh;
    }
    ui.add_space(18.0);

    ui.add_enabled_ui(cpu_usage.enabled, |ui| {
        if ui.button("Add CPU usage rule").clicked() {
            cpu_usage.rules.push(CpuUsageRule {
                name: "New CPU Rule".to_owned(),
                comparison: CpuUsageComparison::AtOrBelow,
                threshold_percent: 20,
                duration_seconds: 30,
                target: CpuUsageTarget::Idle,
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
                    ui.label("When CPU is");
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
                        });
                    ui.add(
                        egui::DragValue::new(&mut rule.threshold_percent)
                            .speed(1.0)
                            .range(0..=100)
                            .suffix("%"),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("For");
                    ui.add(
                        egui::DragValue::new(&mut rule.duration_seconds)
                            .speed(1.0)
                            .range(0..=86_400)
                            .suffix(" sec"),
                    );
                    ui.label("switch to");
                    egui::ComboBox::from_id_salt(("cpu_target", index))
                        .selected_text(rule.target.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut rule.target,
                                CpuUsageTarget::Idle,
                                CpuUsageTarget::Idle.label(),
                            );
                            ui.selectable_value(
                                &mut rule.target,
                                CpuUsageTarget::Active,
                                CpuUsageTarget::Active.label(),
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
