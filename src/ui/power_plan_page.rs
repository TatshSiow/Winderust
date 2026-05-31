use eframe::egui;

use crate::{config::PowerPlanSettings, power::PowerPlan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerPlanAction {
    None,
    Refresh,
}

pub fn show_section(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    power_plans: &mut PowerPlanSettings,
    plans: &[PowerPlan],
    current_plan: Option<&PowerPlan>,
) -> PowerPlanAction {
    let mut action = PowerPlanAction::None;

    ui.group(|ui| {
        ui.heading(title);
        ui.label(description);
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
        });

        ui.add_space(12.0);

        plan_combo(ui, "Idle plan", &mut power_plans.power_save_guid, plans);
        ui.add_space(8.0);
        plan_combo(ui, "Active plan", &mut power_plans.performance_guid, plans);
    });

    action
}

pub fn plan_combo(
    ui: &mut egui::Ui,
    label: &str,
    selected_guid: &mut Option<String>,
    plans: &[PowerPlan],
) {
    plan_combo_with_id(ui, label, label, selected_guid, plans);
}

pub fn plan_combo_with_id(
    ui: &mut egui::Ui,
    label: &str,
    id_salt: impl std::hash::Hash,
    selected_guid: &mut Option<String>,
    plans: &[PowerPlan],
) {
    ui.horizontal(|ui| {
        ui.set_min_width(520.0);
        ui.label(label);

        let selected_text = selected_guid
            .as_deref()
            .and_then(|guid| {
                plans
                    .iter()
                    .find(|plan| plan.guid.eq_ignore_ascii_case(guid))
            })
            .map(PowerPlan::display_name)
            .unwrap_or_else(|| "Select a plan".to_owned());

        egui::ComboBox::from_id_salt(id_salt)
            .selected_text(selected_text)
            .width(320.0)
            .show_ui(ui, |ui| {
                for plan in plans {
                    ui.selectable_value(
                        selected_guid,
                        Some(plan.guid.clone()),
                        plan.display_name(),
                    );
                }
            });
    });
}
