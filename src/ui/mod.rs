use rust_i18n::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Dashboard,
    Activity,
    CpuUsage,
    CoreParking,
    CpuLimiter,
    EfficiencyMode,
    AppSuspension,
    Watchdog,
    PerformanceMode,
    ForegroundResponsiveness,
    CpuAffinity,
    ForegroundRules,
    Schedule,
    ActionLog,
    Settings,
    About,
}

pub struct PageSection {
    pub label: &'static str,
    pub pages: &'static [Page],
}

const OVERVIEW_PAGES: [Page; 1] = [Page::Dashboard];
const POWER_AUTOMATION_PAGES: [Page; 5] = [
    Page::ForegroundRules,
    Page::PerformanceMode,
    Page::CpuUsage,
    Page::Activity,
    Page::Schedule,
];
const CPU_CONTROL_PAGES: [Page; 3] = [Page::CoreParking, Page::CpuLimiter, Page::CpuAffinity];
const PROCESS_POLICY_PAGES: [Page; 4] = [
    Page::EfficiencyMode,
    Page::ForegroundResponsiveness,
    Page::AppSuspension,
    Page::Watchdog,
];
const APP_PAGES: [Page; 3] = [Page::ActionLog, Page::Settings, Page::About];
const PAGE_SECTIONS: [PageSection; 5] = [
    PageSection {
        label: "Overview",
        pages: &OVERVIEW_PAGES,
    },
    PageSection {
        label: "Power Plan Automation",
        pages: &POWER_AUTOMATION_PAGES,
    },
    PageSection {
        label: "Processor Controls",
        pages: &CPU_CONTROL_PAGES,
    },
    PageSection {
        label: "Process Policies",
        pages: &PROCESS_POLICY_PAGES,
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
            Self::CpuLimiter => t!("nav.cpu_limiter"),
            Self::EfficiencyMode => t!("nav.efficiency_mode"),
            Self::AppSuspension => t!("nav.app_suspension"),
            Self::Watchdog => t!("nav.watchdog"),
            Self::PerformanceMode => t!("nav.performance_mode"),
            Self::ForegroundResponsiveness => t!("nav.foreground_responsiveness"),
            Self::CpuAffinity => t!("nav.cpu_affinity"),
            Self::ForegroundRules => t!("nav.foreground_rules"),
            Self::Schedule => t!("nav.schedule"),
            Self::ActionLog => t!("nav.action_log"),
            Self::Settings => t!("nav.settings"),
            Self::About => t!("nav.about"),
        }
        .to_string()
    }

    pub fn section_label(self) -> String {
        match self {
            Self::Dashboard => t!("nav.overview"),
            Self::Activity
            | Self::Schedule
            | Self::ForegroundRules
            | Self::PerformanceMode
            | Self::CpuUsage => t!("nav.power_automation"),
            Self::CoreParking | Self::CpuLimiter | Self::CpuAffinity => {
                t!("nav.processor_controls")
            }
            Self::EfficiencyMode
            | Self::ForegroundResponsiveness
            | Self::AppSuspension
            | Self::Watchdog => t!("nav.process_policies"),
            Self::ActionLog | Self::Settings | Self::About => t!("nav.app"),
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
        "Power Plan Automation" => t!("nav.power_automation"),
        "Processor Controls" => t!("nav.processor_controls"),
        "Process Policies" => t!("nav.process_policies"),
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
