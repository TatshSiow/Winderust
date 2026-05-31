use eframe::egui;

use crate::{
    activity::ActivitySnapshot, config::Settings, cpu::CpuUsageSnapshot, ecoqos::EcoQosSnapshot,
    power::PowerPlan, rules::DecisionOutcome, ui::duration_label,
};

pub fn show(
    ui: &mut egui::Ui,
    settings: &Settings,
    current_plan: Option<&PowerPlan>,
    foreground_app: Option<&str>,
    activity: &ActivitySnapshot,
    cpu_usage: &CpuUsageSnapshot,
    eco_qos: &EcoQosSnapshot,
    decision: &DecisionOutcome,
    next_schedule: &str,
) -> bool {
    let mut check_now = false;

    ui.heading("Dashboard");
    ui.horizontal(|ui| {
        if ui.button("Check now").clicked() {
            check_now = true;
        }
    });
    ui.add_space(12.0);

    egui::Grid::new("dashboard_grid")
        .num_columns(2)
        .spacing([24.0, 12.0])
        .striped(true)
        .show(ui, |ui| {
            row(
                ui,
                "Current power plan",
                current_plan
                    .map(|plan| plan.name.as_str())
                    .unwrap_or("Unknown"),
            );
            row(ui, "Current mode", decision.state.label());
            row(
                ui,
                "Automation",
                if settings.general.enabled {
                    "Enabled"
                } else {
                    "Disabled"
                },
            );
            row(ui, "Foreground app", foreground_app.unwrap_or("Unknown"));
            row(ui, "Activity state", &format!("{:?}", activity.state));
            row(ui, "CPU usage", &cpu_usage_label(cpu_usage.percent));
            row(ui, "Efficiency Mode", &eco_qos_label(eco_qos));
            row(
                ui,
                "Idle time",
                &activity
                    .idle_for
                    .map(|duration| duration_label(duration.as_secs()))
                    .unwrap_or_else(|| "Unknown".to_owned()),
            );
            row(ui, "Next scheduled switch", next_schedule);
            row(ui, "Decision reason", &decision.reason);
        });

    check_now
}

fn row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.strong(label);
    ui.label(value);
    ui.end_row();
}

fn cpu_usage_label(percent: Option<f32>) -> String {
    percent
        .map(|percent| format!("{percent:.1}%"))
        .unwrap_or_else(|| "Collecting".to_owned())
}

fn eco_qos_label(status: &EcoQosSnapshot) -> String {
    if status.enabled {
        format!(
            "{} ({} throttled)",
            status.message, status.throttled_processes
        )
    } else {
        status.message.clone()
    }
}
