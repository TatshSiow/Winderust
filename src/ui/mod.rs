use eframe::egui;

pub mod about_page;
pub mod cpu_usage_page;
pub mod dashboard;
pub mod efficiency_page;
pub mod power_plan_page;
pub mod rules_page;
pub mod schedule_page;
pub mod suspension_page;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Activity,
    CpuUsage,
    EfficiencyMode,
    AppSuspension,
    ForegroundRules,
    Schedule,
    Settings,
    About,
}

pub struct PageSection {
    pub label: &'static str,
    pub pages: &'static [Page],
}

const OVERVIEW_PAGES: [Page; 1] = [Page::Dashboard];
const AUTOMATION_RULE_PAGES: [Page; 4] = [
    Page::Activity,
    Page::CpuUsage,
    Page::Schedule,
    Page::ForegroundRules,
];
const PROCESS_CONTROL_PAGES: [Page; 2] = [Page::EfficiencyMode, Page::AppSuspension];
const APP_PAGES: [Page; 2] = [Page::Settings, Page::About];
const PAGE_SECTIONS: [PageSection; 4] = [
    PageSection {
        label: "Overview",
        pages: &OVERVIEW_PAGES,
    },
    PageSection {
        label: "Power Plan Controls",
        pages: &AUTOMATION_RULE_PAGES,
    },
    PageSection {
        label: "Process Controls",
        pages: &PROCESS_CONTROL_PAGES,
    },
    PageSection {
        label: "App",
        pages: &APP_PAGES,
    },
];

impl Page {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Activity => "Action Based Scheduler",
            Self::CpuUsage => "CPU usage-based Scheduler",
            Self::EfficiencyMode => "Efficiency Mode",
            Self::AppSuspension => "App Suspension",
            Self::ForegroundRules => "Foreground Rules",
            Self::Schedule => "Time Based Scheduler",
            Self::Settings => "Settings",
            Self::About => "About",
        }
    }

    pub const fn sections() -> &'static [PageSection] {
        &PAGE_SECTIONS
    }
}

pub fn duration_label(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {}s", seconds / 60, seconds % 60)
    }
}

pub fn help_popup_label(
    ui: &mut egui::Ui,
    label: &'static str,
    _popup_salt: &'static str,
    add_contents: fn(&mut egui::Ui),
) {
    ui.label(egui::RichText::new(label).heading())
        .on_hover_cursor(egui::CursorIcon::Help)
        .on_hover_ui(add_contents);

    draw_help_indicator(ui);
}

fn draw_help_indicator(ui: &mut egui::Ui) {
    let size = egui::vec2(22.0, 22.0);
    let (rect, _response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let visuals = ui.visuals();
    ui.painter().circle_stroke(
        rect.center(),
        9.0,
        egui::Stroke::new(1.0, visuals.weak_text_color()),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "?",
        egui::FontId::proportional(12.0),
        visuals.weak_text_color(),
    );
}
