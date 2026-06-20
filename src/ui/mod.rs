use rust_i18n::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Dashboard,
    PowerPlanAutomation,
    ProcessorControls,
    ProcessPolicies,
    MemoryControl,
    AppHome,
    AdvancedHome,
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
    GpuPriority,
    MemoryPriority,
    LaunchPriority,
    SmartTrim,
    CpuAffinity,
    ForegroundRules,
    Schedule,
    ActionLog,
    Settings,
    SettingsAppearance,
    TimerResolution,
    Win32PrioritySeparation,
    About,
}

pub struct PageSection {
    pub landing_page: Page,
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
const PROCESS_POLICY_PAGES: [Page; 5] = [
    Page::EfficiencyMode,
    Page::IoPriority,
    Page::GpuPriority,
    Page::LaunchPriority,
    Page::Watchdog,
];
const AUTO_BALANCE_PAGES: [Page; 1] = [Page::ForegroundResponsiveness];
const MEMORY_CONTROL_PAGES: [Page; 2] = [Page::MemoryPriority, Page::SmartTrim];
const ACTION_LOG_PAGES: [Page; 1] = [Page::ActionLog];
const APP_PAGES: [Page; 3] = [Page::Settings, Page::SettingsAppearance, Page::About];
const ADVANCED_PAGES: [Page; 3] = [
    Page::AppSuspension,
    Page::TimerResolution,
    Page::Win32PrioritySeparation,
];
const PAGE_SECTIONS: [PageSection; 9] = [
    PageSection {
        landing_page: Page::Dashboard,
        pages: &OVERVIEW_PAGES,
    },
    PageSection {
        landing_page: Page::ForegroundResponsiveness,
        pages: &AUTO_BALANCE_PAGES,
    },
    PageSection {
        landing_page: Page::PowerPlanAutomation,
        pages: &POWER_AUTOMATION_PAGES,
    },
    PageSection {
        landing_page: Page::ProcessPolicies,
        pages: &PROCESS_POLICY_PAGES,
    },
    PageSection {
        landing_page: Page::ProcessorControls,
        pages: &CPU_CONTROL_PAGES,
    },
    PageSection {
        landing_page: Page::MemoryControl,
        pages: &MEMORY_CONTROL_PAGES,
    },
    PageSection {
        landing_page: Page::ActionLog,
        pages: &ACTION_LOG_PAGES,
    },
    PageSection {
        landing_page: Page::AppHome,
        pages: &APP_PAGES,
    },
    PageSection {
        landing_page: Page::AdvancedHome,
        pages: &ADVANCED_PAGES,
    },
];

impl Page {
    pub fn label(self) -> String {
        match self {
            Self::Dashboard => t!("nav.overview"),
            Self::PowerPlanAutomation => t!("nav.power_automation"),
            Self::ProcessorControls => t!("nav.processor_controls"),
            Self::ProcessPolicies => t!("nav.process_policies"),
            Self::MemoryControl => t!("nav.memory_control"),
            Self::AppHome => t!("nav.settings"),
            Self::AdvancedHome => t!("nav.advanced"),
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
            Self::GpuPriority => t!("nav.gpu_priority"),
            Self::MemoryPriority => t!("nav.memory_priority"),
            Self::LaunchPriority => t!("nav.launch_priority"),
            Self::SmartTrim => t!("nav.smart_trim"),
            Self::CpuAffinity => t!("nav.cpu_affinity"),
            Self::ForegroundRules => t!("nav.foreground_rules"),
            Self::Schedule => t!("nav.schedule"),
            Self::ActionLog => t!("nav.action_log"),
            Self::Settings => t!("settings.powerleaf_behaviour"),
            Self::SettingsAppearance => t!("settings.language_and_appearance"),
            Self::TimerResolution => t!("nav.timer_resolution"),
            Self::Win32PrioritySeparation => t!("nav.win32_priority_separation"),
            Self::About => t!("nav.about"),
        }
        .to_string()
    }

    pub fn section_label(self) -> String {
        match self {
            Self::Dashboard => t!("nav.overview"),
            Self::PowerPlanAutomation => t!("nav.power_automation"),
            Self::ProcessorControls => t!("nav.processor_controls"),
            Self::ProcessPolicies => t!("nav.process_policies"),
            Self::MemoryControl => t!("nav.memory_control"),
            Self::AppHome => t!("nav.settings"),
            Self::AdvancedHome => t!("nav.advanced"),
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
            | Self::IoPriority
            | Self::GpuPriority
            | Self::LaunchPriority
            | Self::Watchdog => t!("nav.process_policies"),
            Self::ForegroundResponsiveness => t!("nav.foreground_responsiveness"),
            Self::MemoryPriority | Self::SmartTrim => t!("nav.memory_control"),
            Self::ActionLog => t!("nav.action_log"),
            Self::Settings | Self::SettingsAppearance | Self::About => t!("nav.settings"),
            Self::AppSuspension | Self::TimerResolution | Self::Win32PrioritySeparation => {
                t!("nav.advanced")
            }
        }
        .to_string()
    }

    pub const fn section_landing_page(self) -> Page {
        match self {
            Self::Dashboard => Self::Dashboard,
            Self::PowerPlanAutomation
            | Self::Activity
            | Self::Schedule
            | Self::ForegroundRules
            | Self::PerformanceMode
            | Self::CpuUsage => Self::PowerPlanAutomation,
            Self::ProcessorControls
            | Self::CoreParking
            | Self::CpuLimiter
            | Self::BackgroundCpuRestriction
            | Self::CpuAffinity => Self::ProcessorControls,
            Self::ProcessPolicies
            | Self::EfficiencyMode
            | Self::IoPriority
            | Self::GpuPriority
            | Self::LaunchPriority
            | Self::Watchdog => Self::ProcessPolicies,
            Self::ForegroundResponsiveness => Self::ForegroundResponsiveness,
            Self::MemoryControl | Self::MemoryPriority | Self::SmartTrim => Self::MemoryControl,
            Self::ActionLog => Self::ActionLog,
            Self::AppHome | Self::Settings | Self::SettingsAppearance | Self::About => {
                Self::AppHome
            }
            Self::AdvancedHome
            | Self::AppSuspension
            | Self::TimerResolution
            | Self::Win32PrioritySeparation => Self::AdvancedHome,
        }
    }

    pub const fn child_pages(self) -> Option<&'static [Page]> {
        match self {
            Self::Dashboard => Some(&OVERVIEW_PAGES),
            Self::PowerPlanAutomation => Some(&POWER_AUTOMATION_PAGES),
            Self::ProcessorControls => Some(&CPU_CONTROL_PAGES),
            Self::ProcessPolicies => Some(&PROCESS_POLICY_PAGES),
            Self::ForegroundResponsiveness => Some(&AUTO_BALANCE_PAGES),
            Self::MemoryControl => Some(&MEMORY_CONTROL_PAGES),
            Self::AppHome => Some(&APP_PAGES),
            Self::AdvancedHome => Some(&ADVANCED_PAGES),
            _ => None,
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
