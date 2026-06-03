#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Dashboard,
    Activity,
    CpuUsage,
    EfficiencyMode,
    AppSuspension,
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
const PROCESS_CONTROL_PAGES: [Page; 3] =
    [Page::EfficiencyMode, Page::AppSuspension, Page::CpuAffinity];
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
            Self::CpuUsage => "CPU Load Rules",
            Self::EfficiencyMode => "Efficiency Mode",
            Self::AppSuspension => "App Suspension",
            Self::CpuAffinity => "CPU Affinity",
            Self::ForegroundRules => "Foreground Rules",
            Self::Schedule => "Time Rules",
            Self::Settings => "Settings",
            Self::About => "About",
        }
    }

    pub const fn section_label(self) -> &'static str {
        match self {
            Self::Dashboard => "Overview",
            Self::Activity | Self::CpuUsage | Self::Schedule | Self::ForegroundRules => {
                "Power Plan Controls"
            }
            Self::EfficiencyMode | Self::AppSuspension | Self::CpuAffinity => "Process Controls",
            Self::Settings | Self::About => "App",
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
