use rust_i18n::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Dashboard,
    PowerPlanAutomation,
    WinderustFeatures,
    ProcessorControls,
    PriorityControl,
    ProcessPolicies,
    MemoryControl,
    AppHome,
    AdvancedHome,
    Activity,
    CpuUsage,
    CoreParking,
    CpuPriority,
    ThreadPriority,
    DynamicPriorityBoost,
    CpuLimiter,
    BackgroundCpuRestriction,
    ProcessList,
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
const PROCESS_LIST_PAGES: [Page; 1] = [Page::ProcessList];
const POWER_AUTOMATION_PAGES: [Page; 6] = [
    Page::ForegroundRules,
    Page::PerformanceMode,
    Page::CpuUsage,
    Page::Activity,
    Page::Schedule,
    Page::CoreParking,
];
const CPU_CONTROL_PAGES: [Page; 3] = [
    Page::CpuLimiter,
    Page::BackgroundCpuRestriction,
    Page::CpuAffinity,
];
const PRIORITY_CONTROL_PAGES: [Page; 7] = [
    Page::CpuPriority,
    Page::ThreadPriority,
    Page::DynamicPriorityBoost,
    Page::IoPriority,
    Page::GpuPriority,
    Page::MemoryPriority,
    Page::LaunchPriority,
];
const PROCESS_POLICY_PAGES: [Page; 2] = [Page::EfficiencyMode, Page::Watchdog];
const WINDERUST_FEATURE_PAGES: [Page; 2] = [Page::ForegroundResponsiveness, Page::SmartTrim];
const ACTION_LOG_PAGES: [Page; 1] = [Page::ActionLog];
const APP_PAGES: [Page; 3] = [Page::Settings, Page::SettingsAppearance, Page::About];
const ADVANCED_PAGES: [Page; 3] = [
    Page::AppSuspension,
    Page::TimerResolution,
    Page::Win32PrioritySeparation,
];
const PAGE_SECTIONS: [PageSection; 10] = [
    PageSection {
        landing_page: Page::Dashboard,
        pages: &OVERVIEW_PAGES,
    },
    PageSection {
        landing_page: Page::ProcessList,
        pages: &PROCESS_LIST_PAGES,
    },
    PageSection {
        landing_page: Page::WinderustFeatures,
        pages: &WINDERUST_FEATURE_PAGES,
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
        landing_page: Page::PriorityControl,
        pages: &PRIORITY_CONTROL_PAGES,
    },
    PageSection {
        landing_page: Page::ProcessorControls,
        pages: &CPU_CONTROL_PAGES,
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
            Self::WinderustFeatures => t!("nav.winderust_features"),
            Self::ProcessorControls => t!("nav.processor_controls"),
            Self::PriorityControl => t!("nav.priority_control"),
            Self::ProcessPolicies => t!("nav.process_policies"),
            Self::MemoryControl => t!("nav.memory_control"),
            Self::AppHome => t!("nav.settings"),
            Self::AdvancedHome => t!("nav.advanced"),
            Self::Activity => t!("nav.activity"),
            Self::CpuUsage => t!("nav.cpu_usage"),
            Self::CoreParking => t!("nav.core_parking"),
            Self::CpuPriority => t!("nav.cpu_priority"),
            Self::ThreadPriority => t!("nav.thread_priority"),
            Self::DynamicPriorityBoost => t!("nav.dynamic_priority_boost"),
            Self::CpuLimiter => t!("nav.cpu_limiter"),
            Self::BackgroundCpuRestriction => t!("nav.background_cpu_restriction"),
            Self::ProcessList => t!("nav.process_list"),
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
            Self::Settings => t!("settings.winderust_behaviour"),
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
            Self::WinderustFeatures => t!("nav.winderust_features"),
            Self::ProcessorControls => t!("nav.processor_controls"),
            Self::PriorityControl => t!("nav.priority_control"),
            Self::ProcessPolicies => t!("nav.process_policies"),
            Self::MemoryControl => t!("nav.memory_control"),
            Self::AppHome => t!("nav.settings"),
            Self::AdvancedHome => t!("nav.advanced"),
            Self::ProcessList => t!("nav.process_list"),
            Self::Activity
            | Self::Schedule
            | Self::ForegroundRules
            | Self::PerformanceMode
            | Self::CpuUsage
            | Self::CoreParking => t!("nav.power_automation"),
            Self::CpuLimiter | Self::BackgroundCpuRestriction | Self::CpuAffinity => {
                t!("nav.processor_controls")
            }
            Self::CpuPriority
            | Self::ThreadPriority
            | Self::DynamicPriorityBoost
            | Self::IoPriority
            | Self::GpuPriority
            | Self::MemoryPriority
            | Self::LaunchPriority => t!("nav.priority_control"),
            Self::EfficiencyMode | Self::Watchdog => t!("nav.process_policies"),
            Self::ForegroundResponsiveness | Self::SmartTrim => t!("nav.winderust_features"),
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
            Self::ProcessList => Self::ProcessList,
            Self::WinderustFeatures | Self::ForegroundResponsiveness | Self::SmartTrim => {
                Self::WinderustFeatures
            }
            Self::PowerPlanAutomation
            | Self::Activity
            | Self::Schedule
            | Self::ForegroundRules
            | Self::PerformanceMode
            | Self::CpuUsage
            | Self::CoreParking => Self::PowerPlanAutomation,
            Self::ProcessorControls
            | Self::CpuLimiter
            | Self::BackgroundCpuRestriction
            | Self::CpuAffinity => Self::ProcessorControls,
            Self::PriorityControl
            | Self::CpuPriority
            | Self::ThreadPriority
            | Self::DynamicPriorityBoost
            | Self::IoPriority
            | Self::GpuPriority
            | Self::MemoryPriority
            | Self::LaunchPriority => Self::PriorityControl,
            Self::ProcessPolicies | Self::EfficiencyMode | Self::Watchdog => Self::ProcessPolicies,
            Self::MemoryControl => Self::MemoryControl,
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
            Self::ProcessList => Some(&PROCESS_LIST_PAGES),
            Self::WinderustFeatures => Some(&WINDERUST_FEATURE_PAGES),
            Self::PowerPlanAutomation => Some(&POWER_AUTOMATION_PAGES),
            Self::ProcessorControls => Some(&CPU_CONTROL_PAGES),
            Self::PriorityControl => Some(&PRIORITY_CONTROL_PAGES),
            Self::ProcessPolicies => Some(&PROCESS_POLICY_PAGES),
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
