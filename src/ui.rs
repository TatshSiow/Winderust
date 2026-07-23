use rust_i18n::t;

pub(crate) mod app;
pub(crate) mod assets;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Home,
    PowerPlanControl,
    WinderustFeatures,
    CpuControl,
    PriorityControl,
    SettingsHome,
    AdvancedControls,
    ByActivity,
    ByCpuLoad,
    AdvancedPowerPlanTuning,
    ProcessPriority,
    ThreadPriority,
    DynamicPriorityBoost,
    CoreLimiter,
    BackgroundCpuRestriction,
    ProcessList,
    AdaptiveEngine,
    BackgroundEfficiency,
    AppSuspension,
    ByRunningApp,
    IoPriority,
    GpuPriority,
    MemoryPriority,
    MemoryTrim,
    CoreSteering,
    ByForeground,
    ByTime,
    ActionLog,
    WinderustBehaviour,
    LanguageAndAppearance,
    ExperimentalFeatures,
    TimerResolution,
    Win32PrioritySeparation,
    About,
}

pub struct PageSection {
    pub landing_page: Page,
    pub pages: &'static [Page],
}

const OVERVIEW_PAGES: [Page; 1] = [Page::Home];
const PROCESS_LIST_PAGES: [Page; 1] = [Page::ProcessList];
const POWER_PLAN_CONTROL_PAGES: [Page; 6] = [
    Page::ByForeground,
    Page::ByRunningApp,
    Page::ByCpuLoad,
    Page::ByActivity,
    Page::ByTime,
    Page::AdvancedPowerPlanTuning,
];
const CPU_CONTROL_PAGES: [Page; 3] = [
    Page::CoreLimiter,
    Page::BackgroundCpuRestriction,
    Page::CoreSteering,
];
const PRIORITY_CONTROL_PAGES: [Page; 6] = [
    Page::ProcessPriority,
    Page::ThreadPriority,
    Page::DynamicPriorityBoost,
    Page::IoPriority,
    Page::GpuPriority,
    Page::MemoryPriority,
];
const WINDERUST_FEATURE_PAGES: [Page; 3] = [
    Page::AdaptiveEngine,
    Page::BackgroundEfficiency,
    Page::MemoryTrim,
];
const ACTION_LOG_PAGES: [Page; 1] = [Page::ActionLog];
const SETTINGS_PAGES: [Page; 3] = [
    Page::WinderustBehaviour,
    Page::LanguageAndAppearance,
    Page::ExperimentalFeatures,
];
const ABOUT_PAGES: [Page; 1] = [Page::About];
const ADVANCED_CONTROLS_PAGES: [Page; 3] = [
    Page::AppSuspension,
    Page::TimerResolution,
    Page::Win32PrioritySeparation,
];
const PAGE_SECTIONS: [PageSection; 10] = [
    PageSection {
        landing_page: Page::Home,
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
        landing_page: Page::PowerPlanControl,
        pages: &POWER_PLAN_CONTROL_PAGES,
    },
    PageSection {
        landing_page: Page::PriorityControl,
        pages: &PRIORITY_CONTROL_PAGES,
    },
    PageSection {
        landing_page: Page::CpuControl,
        pages: &CPU_CONTROL_PAGES,
    },
    PageSection {
        landing_page: Page::ActionLog,
        pages: &ACTION_LOG_PAGES,
    },
    PageSection {
        landing_page: Page::SettingsHome,
        pages: &SETTINGS_PAGES,
    },
    PageSection {
        landing_page: Page::About,
        pages: &ABOUT_PAGES,
    },
    PageSection {
        landing_page: Page::AdvancedControls,
        pages: &ADVANCED_CONTROLS_PAGES,
    },
];

impl Page {
    pub fn label(self) -> String {
        match self {
            Self::Home => t!("nav.home"),
            Self::PowerPlanControl => t!("nav.power_plan_control"),
            Self::WinderustFeatures => t!("nav.winderust_features"),
            Self::CpuControl => t!("nav.cpu_control"),
            Self::PriorityControl => t!("nav.priority_control"),
            Self::SettingsHome => t!("nav.settings"),
            Self::AdvancedControls => t!("nav.advanced"),
            Self::ByActivity => t!("nav.by_activity"),
            Self::ByCpuLoad => t!("nav.by_cpu_load"),
            Self::AdvancedPowerPlanTuning => t!("nav.advanced_power_plan_tuning"),
            Self::ProcessPriority => t!("nav.process_priority"),
            Self::ThreadPriority => t!("nav.thread_priority"),
            Self::DynamicPriorityBoost => t!("nav.dynamic_priority_boost"),
            Self::CoreLimiter => t!("nav.core_limiter"),
            Self::BackgroundCpuRestriction => t!("nav.background_cpu_restriction"),
            Self::ProcessList => t!("nav.process_list"),
            Self::AdaptiveEngine => t!("nav.adaptive_engine"),
            Self::BackgroundEfficiency => t!("nav.background_efficiency"),
            Self::AppSuspension => t!("nav.app_suspension"),
            Self::ByRunningApp => t!("nav.by_running_app"),
            Self::IoPriority => t!("nav.io_priority"),
            Self::GpuPriority => t!("nav.gpu_priority"),
            Self::MemoryPriority => t!("nav.memory_priority"),
            Self::MemoryTrim => t!("nav.memory_trim"),
            Self::CoreSteering => t!("nav.core_steering"),
            Self::ByForeground => t!("nav.by_foreground"),
            Self::ByTime => t!("nav.by_time"),
            Self::ActionLog => t!("nav.action_log"),
            Self::WinderustBehaviour => t!("settings.winderust_behaviour"),
            Self::LanguageAndAppearance => t!("settings.language_and_appearance"),
            Self::ExperimentalFeatures => t!("settings.experimental_features"),
            Self::TimerResolution => t!("nav.timer_resolution"),
            Self::Win32PrioritySeparation => t!("nav.win32_priority_separation"),
            Self::About => t!("nav.about"),
        }
        .to_string()
    }

    pub fn section_label(self) -> String {
        self.section_landing_page().label()
    }

    pub const fn section_landing_page(self) -> Page {
        match self {
            Self::Home => Self::Home,
            Self::ProcessList => Self::ProcessList,
            Self::WinderustFeatures
            | Self::AdaptiveEngine
            | Self::BackgroundEfficiency
            | Self::MemoryTrim => Self::WinderustFeatures,
            Self::PowerPlanControl
            | Self::ByActivity
            | Self::ByTime
            | Self::ByForeground
            | Self::ByRunningApp
            | Self::ByCpuLoad
            | Self::AdvancedPowerPlanTuning => Self::PowerPlanControl,
            Self::CpuControl
            | Self::CoreLimiter
            | Self::BackgroundCpuRestriction
            | Self::CoreSteering => Self::CpuControl,
            Self::PriorityControl
            | Self::ProcessPriority
            | Self::ThreadPriority
            | Self::DynamicPriorityBoost
            | Self::IoPriority
            | Self::GpuPriority
            | Self::MemoryPriority => Self::PriorityControl,
            Self::ActionLog => Self::ActionLog,
            Self::SettingsHome
            | Self::WinderustBehaviour
            | Self::LanguageAndAppearance
            | Self::ExperimentalFeatures => Self::SettingsHome,
            Self::About => Self::About,
            Self::AdvancedControls
            | Self::AppSuspension
            | Self::TimerResolution
            | Self::Win32PrioritySeparation => Self::AdvancedControls,
        }
    }

    pub fn child_pages(self) -> Option<&'static [Page]> {
        PAGE_SECTIONS
            .iter()
            .find(|section| section.landing_page == self)
            .map(|section| section.pages)
    }

    pub const fn sections() -> &'static [PageSection] {
        &PAGE_SECTIONS
    }
}

pub fn duration_label(seconds: u64) -> String {
    if seconds < 60 {
        t!("common.seconds_short", count = seconds).to_string()
    } else {
        t!(
            "common.minutes_seconds_short",
            minutes = seconds / 60,
            seconds = seconds % 60
        )
        .to_string()
    }
}
#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn page_sections_are_consistent() {
        let mut pages = HashSet::new();

        for section in Page::sections() {
            assert_eq!(section.landing_page.child_pages(), Some(section.pages));
            for page in section.pages {
                assert_eq!(page.section_landing_page(), section.landing_page);
                assert!(pages.insert(*page), "{page:?} belongs to multiple sections");
            }
        }
    }
}
