pub mod about_page;
pub mod cpu_usage_page;
pub mod dashboard;
pub mod power_plan_page;
pub mod rules_page;
pub mod schedule_page;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Activity,
    CpuUsage,
    ForegroundRules,
    Schedule,
    Settings,
    About,
}

impl Page {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Activity => "Action Based Scheduler",
            Self::CpuUsage => "CPU Usage Scheduler",
            Self::ForegroundRules => "Foreground Rules",
            Self::Schedule => "Time Based Scheduler",
            Self::Settings => "Settings",
            Self::About => "About",
        }
    }

    pub const fn all() -> [Self; 7] {
        [
            Self::Dashboard,
            Self::Activity,
            Self::CpuUsage,
            Self::Schedule,
            Self::ForegroundRules,
            Self::Settings,
            Self::About,
        ]
    }
}

pub fn duration_label(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {}s", seconds / 60, seconds % 60)
    }
}
