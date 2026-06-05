use rust_i18n::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Dashboard,
    Activity,
    CpuUsage,
    CoreParking,
    EfficiencyMode,
    AppSuspension,
    ForegroundResponsiveness,
    CpuAffinity,
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
const CPU_CONTROL_PAGES: [Page; 2] = [Page::CoreParking, Page::CpuAffinity];
const PROCESS_CONTROL_PAGES: [Page; 3] = [
    Page::EfficiencyMode,
    Page::AppSuspension,
    Page::ForegroundResponsiveness,
];
const APP_PAGES: [Page; 2] = [Page::Settings, Page::About];
const PAGE_SECTIONS: [PageSection; 5] = [
    PageSection {
        label: "Overview",
        pages: &OVERVIEW_PAGES,
    },
    PageSection {
        label: "Power Plan Controls",
        pages: &AUTOMATION_RULE_PAGES,
    },
    PageSection {
        label: "CPU Controls",
        pages: &CPU_CONTROL_PAGES,
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
    pub fn label(self) -> String {
        match self {
            Self::Dashboard => t!("nav.dashboard"),
            Self::Activity => t!("nav.activity"),
            Self::CpuUsage => t!("nav.cpu_usage"),
            Self::CoreParking => t!("nav.core_parking"),
            Self::EfficiencyMode => t!("nav.efficiency_mode"),
            Self::AppSuspension => t!("nav.app_suspension"),
            Self::ForegroundResponsiveness => t!("nav.foreground_responsiveness"),
            Self::CpuAffinity => t!("nav.cpu_affinity"),
            Self::ForegroundRules => t!("nav.foreground_rules"),
            Self::Schedule => t!("nav.schedule"),
            Self::Settings => t!("nav.settings"),
            Self::About => t!("nav.about"),
        }
        .to_string()
    }

    pub fn section_label(self) -> String {
        match self {
            Self::Dashboard => t!("nav.overview"),
            Self::Activity | Self::CpuUsage | Self::Schedule | Self::ForegroundRules => {
                t!("nav.power_plan_controls")
            }
            Self::CoreParking | Self::CpuAffinity => t!("nav.cpu_controls"),
            Self::EfficiencyMode | Self::AppSuspension | Self::ForegroundResponsiveness => {
                t!("nav.process_controls")
            }
            Self::Settings | Self::About => t!("nav.app"),
        }
        .to_string()
    }

    pub const fn sections() -> &'static [PageSection] {
        &PAGE_SECTIONS
    }
}

pub fn section_label(label: &str) -> String {
    match label {
        "Overview" => t!("nav.overview"),
        "Power Plan Controls" => t!("nav.power_plan_controls"),
        "CPU Controls" => t!("nav.cpu_controls"),
        "Process Controls" => t!("nav.process_controls"),
        "App" => t!("nav.app"),
        _ => label.into(),
    }
    .to_string()
}

pub fn duration_label(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {}s", seconds / 60, seconds % 60)
    }
}
