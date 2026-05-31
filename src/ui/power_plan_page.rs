use eframe::egui;

use crate::{config::Settings, power::PowerPlan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerPlanAction {
    None,
    Refresh,
}

pub fn show(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    plans: &[PowerPlan],
    current_plan: Option<&PowerPlan>,
) -> PowerPlanAction {
    let mut action = PowerPlanAction::None;

    ui.heading("Power Plan Mapping");
    ui.add_space(8.0);
    ui.label("Map the logical app roles to any Windows power plans available on this PC.");
    ui.add_space(14.0);

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

    ui.add_space(18.0);

    plan_combo(
        ui,
        "Idle plan",
        &mut settings.power_plans.power_save_guid,
        plans,
    );
    ui.add_space(8.0);
    plan_combo(
        ui,
        "Active plan",
        &mut settings.power_plans.performance_guid,
        plans,
    );

    action
}

pub fn plan_combo(
    ui: &mut egui::Ui,
    label: &str,
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

        egui::ComboBox::from_id_salt(label)
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
