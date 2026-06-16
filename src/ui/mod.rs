use rust_i18n::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Dashboard,
    Activity,
    CpuUsage,
    CoreParking,
    CpuLimiter,
    BackgroundCpuRestriction,
    EfficiencyMode,
    AppSuspension,
    Watchdog,
    PerformanceMode,
    ForegroundResponsiveness,
    IoPriority,
    SmartTrim,
    CpuAffinity,
    ForegroundRules,
    Schedule,
    ActionLog,
    Settings,
    Win32PrioritySeparation,
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
const CPU_CONTROL_PAGES: [Page; 4] = [
    Page::CoreParking,
    Page::CpuLimiter,
    Page::BackgroundCpuRestriction,
    Page::CpuAffinity,
];
const PROCESS_POLICY_PAGES: [Page; 6] = [
    Page::EfficiencyMode,
    Page::ForegroundResponsiveness,
    Page::IoPriority,
    Page::SmartTrim,
    Page::AppSuspension,
    Page::Watchdog,
];
const APP_PAGES: [Page; 3] = [Page::ActionLog, Page::Settings, Page::About];
const ADVANCED_PAGES: [Page; 1] = [Page::Win32PrioritySeparation];
const PAGE_SECTIONS: [PageSection; 6] = [
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
    PageSection {
        label: "Advanced",
        pages: &ADVANCED_PAGES,
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
            Self::BackgroundCpuRestriction => t!("nav.background_cpu_restriction"),
            Self::EfficiencyMode => t!("nav.efficiency_mode"),
            Self::AppSuspension => t!("nav.app_suspension"),
            Self::Watchdog => t!("nav.watchdog"),
            Self::PerformanceMode => t!("nav.performance_mode"),
            Self::ForegroundResponsiveness => t!("nav.foreground_responsiveness"),
            Self::IoPriority => t!("nav.io_priority"),
            Self::SmartTrim => t!("nav.smart_trim"),
            Self::CpuAffinity => t!("nav.cpu_affinity"),
            Self::ForegroundRules => t!("nav.foreground_rules"),
            Self::Schedule => t!("nav.schedule"),
            Self::ActionLog => t!("nav.action_log"),
            Self::Settings => t!("nav.settings"),
            Self::Win32PrioritySeparation => t!("nav.win32_priority_separation"),
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
            Self::CoreParking
            | Self::CpuLimiter
            | Self::BackgroundCpuRestriction
            | Self::CpuAffinity => {
                t!("nav.processor_controls")
            }
            Self::EfficiencyMode
            | Self::ForegroundResponsiveness
            | Self::IoPriority
            | Self::SmartTrim
            | Self::AppSuspension
            | Self::Watchdog => t!("nav.process_policies"),
            Self::ActionLog | Self::Settings | Self::About => t!("nav.app"),
            Self::Win32PrioritySeparation => t!("nav.advanced"),
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
        "Advanced" => t!("nav.advanced"),
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
