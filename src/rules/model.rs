#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::time::Instant;

pub const PRIORITY_MANUAL_OVERRIDE: i32 = 1000;
pub const PRIORITY_SAFETY: i32 = 900;
pub const PRIORITY_WATCHDOG: i32 = 850;
pub const PRIORITY_FOREGROUND_RESPONSIVENESS: i32 = 825;
pub const PRIORITY_FOCUSED_APP: i32 = 800;
pub const PRIORITY_RUNNING_APP: i32 = 700;
pub const PRIORITY_BACKGROUND_APP: i32 = 600;
pub const PRIORITY_CPU_LOAD: i32 = 500;
pub const PRIORITY_ACTIVITY: i32 = 400;
pub const PRIORITY_SCHEDULE: i32 = 300;
pub const PRIORITY_FALLBACK: i32 = 100;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuleId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: RuleId,
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub trigger: Trigger,
    pub actions: Vec<Action>,
    pub restore_actions: Vec<Action>,
    pub cooldown_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenericAppConfig {
    pub version: u32,
    pub rules: Vec<Rule>,
    pub app_profiles: Vec<AppResourcePolicy>,
    pub power_plan_profiles: Vec<PowerPlanProfile>,
}

impl Default for GenericAppConfig {
    fn default() -> Self {
        Self {
            version: 1,
            rules: Vec::new(),
            app_profiles: Vec::new(),
            power_plan_profiles: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppResourcePolicy {
    pub app: AppMatcher,
    pub foreground: AppStatePolicy,
    pub background: AppStatePolicy,
    pub background_idle: Option<AppStatePolicy>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppStatePolicy {
    pub efficiency_mode: Option<bool>,
    pub priority: Option<RuleProcessPriority>,
    pub affinity: Option<AffinityPolicy>,
    pub logical_processor_limit_percent: Option<u8>,
    pub suspend_after_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerPlanProfile {
    pub plan_guid: String,
    pub core_parking_min_percent: Option<u8>,
    pub processor_performance_min_percent: Option<u8>,
    pub processor_performance_max_percent: Option<u8>,
}

impl Rule {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        trigger: Trigger,
        actions: Vec<Action>,
    ) -> Self {
        let priority = trigger.default_priority();
        Self {
            id: RuleId(id.into()),
            name: name.into(),
            enabled: true,
            priority,
            trigger,
            actions,
            restore_actions: Vec::new(),
            cooldown_secs: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trigger {
    ManualOverride,
    SafetyProtection,
    AppFocused {
        app: AppMatcher,
    },
    AppRunning {
        app: AppMatcher,
    },
    AppBackground {
        app: AppMatcher,
        duration_secs: u64,
    },
    AppBackgroundIdle {
        app: AppMatcher,
        duration_secs: u64,
    },
    CpuLoadAbove {
        percent: u8,
        duration_secs: u64,
    },
    CpuLoadBelow {
        percent: u8,
        duration_secs: u64,
    },
    UserIdle {
        duration_secs: u64,
    },
    UserActive,
    Schedule {
        schedule_id: String,
    },
    ForegroundCpuPressure {
        foreground: AppMatcher,
        total_cpu_above_percent: u8,
        background_process_above_percent: u8,
        duration_secs: u64,
    },
    ProcessStarted {
        app: AppMatcher,
    },
    ProcessExited {
        app: AppMatcher,
    },
    ProcessMissing {
        app: AppMatcher,
        duration_secs: u64,
    },
}

impl Trigger {
    pub const fn default_priority(&self) -> i32 {
        match self {
            Self::ManualOverride => PRIORITY_MANUAL_OVERRIDE,
            Self::SafetyProtection => PRIORITY_SAFETY,
            Self::ProcessStarted { .. }
            | Self::ProcessExited { .. }
            | Self::ProcessMissing { .. } => PRIORITY_WATCHDOG,
            Self::ForegroundCpuPressure { .. } => PRIORITY_FOREGROUND_RESPONSIVENESS,
            Self::AppFocused { .. } => PRIORITY_FOCUSED_APP,
            Self::AppRunning { .. } => PRIORITY_RUNNING_APP,
            Self::AppBackground { .. } | Self::AppBackgroundIdle { .. } => PRIORITY_BACKGROUND_APP,
            Self::CpuLoadAbove { .. } | Self::CpuLoadBelow { .. } => PRIORITY_CPU_LOAD,
            Self::UserIdle { .. } | Self::UserActive => PRIORITY_ACTIVITY,
            Self::Schedule { .. } => PRIORITY_SCHEDULE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppMatcher {
    ProcessName(String),
    Path(String),
    Pattern(String),
}

impl AppMatcher {
    pub fn identity_key(&self) -> ProcessIdentity {
        match self {
            Self::ProcessName(name) | Self::Path(name) | Self::Pattern(name) => {
                ProcessIdentity(name.trim().to_ascii_lowercase())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProcessIdentity(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeProcessInfo {
    pub process_id: u32,
    pub process_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RuntimeState {
    pub foreground_app: Option<RuntimeProcessInfo>,
    pub running_processes: Vec<RuntimeProcessInfo>,
    pub cpu_load_percent: Option<f32>,
    pub user_idle_secs: Option<u64>,
    pub active_schedule_ids: Vec<String>,
    pub active_rules: Vec<RuleId>,
    #[serde(skip)]
    pub applied_actions: Vec<AppliedAction>,
}

impl RuntimeState {
    pub fn apply_event(&mut self, event: DetectorEvent) {
        match event {
            DetectorEvent::Foreground(process) => {
                self.foreground_app = process;
            }
            DetectorEvent::ProcessList(processes) => {
                self.running_processes = processes;
            }
            DetectorEvent::CpuLoad(percent) => {
                self.cpu_load_percent = Some(percent.clamp(0.0, 100.0));
            }
            DetectorEvent::UserIdle { idle_secs } => {
                self.user_idle_secs = Some(idle_secs);
            }
            DetectorEvent::ActiveSchedules(schedule_ids) => {
                self.active_schedule_ids = schedule_ids;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DetectorEvent {
    Foreground(Option<RuntimeProcessInfo>),
    ProcessList(Vec<RuntimeProcessInfo>),
    CpuLoad(f32),
    UserIdle { idle_secs: u64 },
    ActiveSchedules(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppliedAction {
    pub rule_id: RuleId,
    pub action: Action,
    pub conflict_group: ConflictGroup,
    pub previous: Option<PreviousValue>,
    pub applied_at: Instant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PreviousValue {
    PowerPlanGuid(String),
    ProcessPriority(RuleProcessPriority),
    Affinity(AffinityPolicy),
    EfficiencyMode(bool),
    Suspended(bool),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    SwitchPowerPlan {
        plan_guid: String,
    },
    SetCoreParking {
        plan_guid: String,
        min_cores_percent: u8,
        max_cores_percent: u8,
    },
    SetProcessorPowerValues {
        plan_guid: String,
        ac_core_parking_min_percent: u8,
        ac_performance_min_percent: u8,
        ac_performance_max_percent: u8,
        ac_boost_mode: u32,
        dc_core_parking_min_percent: u8,
        dc_performance_min_percent: u8,
        dc_performance_max_percent: u8,
        dc_boost_mode: u32,
    },
    SetSystemCpuLimit {
        logical_processor_percent: u8,
    },
    SetAppEfficiencyMode {
        app: AppMatcher,
        enabled: bool,
    },
    ConfigureBackgroundEfficiencyPolicy {
        exclusions: Vec<AppMatcher>,
        prefer_efficiency_cores: bool,
        logical_processor_percent: Option<u8>,
    },
    SetAppPriority {
        app: AppMatcher,
        priority: RuleProcessPriority,
    },
    BoostForegroundPriority {
        app: AppMatcher,
        priority: RuleProcessPriority,
    },
    LowerBackgroundApps {
        priority: RuleProcessPriority,
        exclusions: Vec<AppMatcher>,
    },
    SetAppAffinity {
        app: AppMatcher,
        affinity: AffinityPolicy,
    },
    SetAppCpuLimit {
        app: AppMatcher,
        logical_processor_percent: u8,
    },
    AutoBalanceBackgroundApps {
        cpu_threshold_percent: u8,
        restore_threshold_percent: u8,
    },
    SuspendApp {
        app: AppMatcher,
    },
    ResumeApp {
        app: AppMatcher,
    },
    TerminateApp {
        app: AppMatcher,
    },
    RestartApp {
        app: AppMatcher,
        launch_path: String,
        args: Vec<String>,
    },
}

impl Action {
    pub fn conflict_group(&self) -> ConflictGroup {
        match self {
            Self::SwitchPowerPlan { .. } => ConflictGroup::PowerPlan,
            Self::SetCoreParking { plan_guid, .. }
            | Self::SetProcessorPowerValues { plan_guid, .. } => {
                ConflictGroup::CoreParking(plan_guid.clone())
            }
            Self::SetSystemCpuLimit { .. } => ConflictGroup::SystemCpuLimit,
            Self::SetAppEfficiencyMode { app, .. } => {
                ConflictGroup::AppEfficiencyMode(app.identity_key())
            }
            Self::ConfigureBackgroundEfficiencyPolicy { .. } => {
                ConflictGroup::BackgroundEfficiencyPolicy
            }
            Self::SetAppPriority { app, .. } | Self::BoostForegroundPriority { app, .. } => {
                ConflictGroup::AppPriority(app.identity_key())
            }
            Self::LowerBackgroundApps { .. } => ConflictGroup::BackgroundAppPriorityPolicy,
            Self::SetAppAffinity { app, .. } | Self::SetAppCpuLimit { app, .. } => {
                ConflictGroup::AppAffinity(app.identity_key())
            }
            Self::AutoBalanceBackgroundApps { .. } => ConflictGroup::BackgroundAppAffinityPolicy,
            Self::SuspendApp { app } | Self::ResumeApp { app } => {
                ConflictGroup::AppSuspension(app.identity_key())
            }
            Self::TerminateApp { app } | Self::RestartApp { app, .. } => {
                ConflictGroup::AppLifecycle(app.identity_key())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConflictGroup {
    PowerPlan,
    CoreParking(String),
    SystemCpuLimit,
    AppEfficiencyMode(ProcessIdentity),
    BackgroundEfficiencyPolicy,
    AppPriority(ProcessIdentity),
    AppAffinity(ProcessIdentity),
    AppSuspension(ProcessIdentity),
    AppLifecycle(ProcessIdentity),
    BackgroundAppPriorityPolicy,
    BackgroundAppAffinityPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleProcessPriority {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AffinityPolicy {
    AllLogicalProcessors,
    LogicalProcessorPercent(u8),
    FirstLogicalProcessors(u8),
    PreferPerformanceCores,
    PreferEfficiencyCores,
    CustomMask(u64),
    CpuSetMask(u64),
    DisableEfficiencyMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchdog_triggers_have_defined_precedence() {
        let rule = Rule::new(
            "watchdog-browser",
            "Restart browser",
            Trigger::ProcessMissing {
                app: AppMatcher::ProcessName("browser.exe".to_owned()),
                duration_secs: 30,
            },
            vec![Action::RestartApp {
                app: AppMatcher::ProcessName("browser.exe".to_owned()),
                launch_path: "C:\\Program Files\\Browser\\browser.exe".to_owned(),
                args: Vec::new(),
            }],
        );

        assert_eq!(rule.priority, PRIORITY_WATCHDOG);
    }

    #[test]
    fn foreground_responsiveness_has_higher_precedence_than_focused_app_rules() {
        let responsiveness = Trigger::ForegroundCpuPressure {
            foreground: AppMatcher::ProcessName("game.exe".to_owned()),
            total_cpu_above_percent: 90,
            background_process_above_percent: 25,
            duration_secs: 5,
        };
        let focused = Trigger::AppFocused {
            app: AppMatcher::ProcessName("game.exe".to_owned()),
        };

        assert!(responsiveness.default_priority() > focused.default_priority());
    }

    #[test]
    fn per_app_actions_share_conflict_groups_for_same_process() {
        let priority = Action::SetAppPriority {
            app: AppMatcher::ProcessName("Editor.EXE".to_owned()),
            priority: RuleProcessPriority::BelowNormal,
        };
        let boost = Action::BoostForegroundPriority {
            app: AppMatcher::ProcessName("editor.exe".to_owned()),
            priority: RuleProcessPriority::AboveNormal,
        };

        assert_eq!(priority.conflict_group(), boost.conflict_group());
    }

    #[test]
    fn watchdog_lifecycle_actions_conflict_per_process() {
        let terminate = Action::TerminateApp {
            app: AppMatcher::ProcessName("worker.exe".to_owned()),
        };
        let restart = Action::RestartApp {
            app: AppMatcher::ProcessName("WORKER.EXE".to_owned()),
            launch_path: "worker.exe".to_owned(),
            args: Vec::new(),
        };

        assert_eq!(terminate.conflict_group(), restart.conflict_group());
    }

    #[test]
    fn runtime_state_applies_detector_events() {
        let mut state = RuntimeState::default();

        state.apply_event(DetectorEvent::Foreground(Some(RuntimeProcessInfo {
            process_id: 42,
            process_name: "editor.exe".to_owned(),
        })));
        state.apply_event(DetectorEvent::CpuLoad(120.0));
        state.apply_event(DetectorEvent::UserIdle { idle_secs: 15 });
        state.apply_event(DetectorEvent::ActiveSchedules(vec!["night".to_owned()]));

        assert_eq!(
            state
                .foreground_app
                .as_ref()
                .map(|process| process.process_id),
            Some(42)
        );
        assert_eq!(state.cpu_load_percent, Some(100.0));
        assert_eq!(state.user_idle_secs, Some(15));
        assert_eq!(state.active_schedule_ids, vec!["night"]);
    }

    #[test]
    fn applied_action_records_conflict_group() {
        let action = Action::SwitchPowerPlan {
            plan_guid: "target-guid".to_owned(),
        };
        let applied = AppliedAction {
            rule_id: RuleId("rule".to_owned()),
            conflict_group: action.conflict_group(),
            action,
            previous: Some(PreviousValue::PowerPlanGuid("old-guid".to_owned())),
            applied_at: Instant::now(),
        };

        assert_eq!(applied.conflict_group, ConflictGroup::PowerPlan);
    }

    #[test]
    fn generic_app_config_defaults_to_version_one() {
        let config = GenericAppConfig::default();

        assert_eq!(config.version, 1);
        assert!(config.rules.is_empty());
        assert!(config.app_profiles.is_empty());
        assert!(config.power_plan_profiles.is_empty());
    }

    #[test]
    fn app_resource_policy_can_describe_foreground_background_and_idle() {
        let policy = AppResourcePolicy {
            app: AppMatcher::ProcessName("browser.exe".to_owned()),
            foreground: AppStatePolicy {
                priority: Some(RuleProcessPriority::Normal),
                ..Default::default()
            },
            background: AppStatePolicy {
                efficiency_mode: Some(true),
                priority: Some(RuleProcessPriority::BelowNormal),
                logical_processor_limit_percent: Some(50),
                ..Default::default()
            },
            background_idle: Some(AppStatePolicy {
                suspend_after_secs: Some(600),
                ..Default::default()
            }),
        };

        assert_eq!(
            policy.background.priority,
            Some(RuleProcessPriority::BelowNormal)
        );
        assert_eq!(
            policy
                .background_idle
                .as_ref()
                .and_then(|state| state.suspend_after_secs),
            Some(600)
        );
    }
}
