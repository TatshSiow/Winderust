#![allow(dead_code)]

use crate::rules::{Action, AffinityPolicy, AppMatcher, RuleProcessPriority};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessorPowerValueSet {
    pub ac_core_parking_min_percent: u8,
    pub ac_performance_min_percent: u8,
    pub ac_performance_max_percent: u8,
    pub ac_boost_mode: u32,
    pub dc_core_parking_min_percent: u8,
    pub dc_performance_min_percent: u8,
    pub dc_performance_max_percent: u8,
    pub dc_boost_mode: u32,
}

pub trait PowerPlanActionBackend {
    fn active_power_plan_guid(&mut self) -> Result<Option<String>, String>;
    fn set_active_power_plan(&mut self, plan_guid: &str) -> Result<(), String>;
    fn set_core_parking(
        &mut self,
        plan_guid: &str,
        min_cores_percent: u8,
        max_cores_percent: u8,
    ) -> Result<(), String>;
    fn set_processor_power_values(
        &mut self,
        plan_guid: &str,
        values: ProcessorPowerValueSet,
    ) -> Result<(), String>;
}

pub trait AppPriorityActionBackend {
    fn app_priority(&mut self, app: &AppMatcher) -> Result<Option<RuleProcessPriority>, String>;
    fn set_app_priority(
        &mut self,
        app: &AppMatcher,
        priority: RuleProcessPriority,
    ) -> Result<(), String>;
    fn lower_background_apps(
        &mut self,
        priority: RuleProcessPriority,
        exclusions: &[AppMatcher],
    ) -> Result<(), String>;
}

pub trait AppLifecycleActionBackend {
    fn terminate_app(&mut self, app: &AppMatcher) -> Result<(), String>;
    fn restart_app(
        &mut self,
        app: &AppMatcher,
        launch_path: &str,
        args: &[String],
    ) -> Result<(), String>;
}

pub trait AppResourceActionBackend {
    fn set_app_efficiency_mode(&mut self, app: &AppMatcher, enabled: bool) -> Result<(), String>;
    fn set_app_affinity(
        &mut self,
        app: &AppMatcher,
        affinity: &AffinityPolicy,
    ) -> Result<(), String>;
    fn set_app_cpu_limit(
        &mut self,
        app: &AppMatcher,
        logical_processor_percent: u8,
    ) -> Result<(), String>;
    fn suspend_app(&mut self, app: &AppMatcher) -> Result<(), String>;
    fn resume_app(&mut self, app: &AppMatcher) -> Result<(), String>;
    fn configure_background_efficiency_policy(
        &mut self,
        exclusions: &[AppMatcher],
        prefer_efficiency_cores: bool,
        logical_processor_percent: Option<u8>,
    ) -> Result<(), String>;
}

pub trait SystemCpuActionBackend {
    fn set_system_cpu_limit(&mut self, logical_processor_percent: u8) -> Result<(), String>;
}

pub trait GenericActionBackend:
    PowerPlanActionBackend
    + AppPriorityActionBackend
    + AppLifecycleActionBackend
    + AppResourceActionBackend
    + SystemCpuActionBackend
{
}

impl<T> GenericActionBackend for T where
    T: PowerPlanActionBackend
        + AppPriorityActionBackend
        + AppLifecycleActionBackend
        + AppResourceActionBackend
        + SystemCpuActionBackend
{
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionExecution {
    Applied,
    AlreadyApplied,
    Unsupported,
    Failed(String),
}

#[derive(Debug, Default)]
pub struct ActionExecutor;

impl ActionExecutor {
    pub fn apply_action(
        &self,
        action: &Action,
        backend: &mut impl GenericActionBackend,
    ) -> ActionExecution {
        match action {
            Action::SwitchPowerPlan { .. }
            | Action::SetCoreParking { .. }
            | Action::SetProcessorPowerValues { .. } => {
                self.apply_power_plan_action(action, backend)
            }
            Action::SetSystemCpuLimit { .. } => self.apply_system_cpu_action(action, backend),
            Action::SetAppPriority { .. }
            | Action::BoostForegroundPriority { .. }
            | Action::LowerBackgroundApps { .. } => self.apply_app_priority_action(action, backend),
            Action::TerminateApp { .. } | Action::RestartApp { .. } => {
                self.apply_app_lifecycle_action(action, backend)
            }
            Action::SetAppAffinity { .. }
            | Action::SetAppCpuLimit { .. }
            | Action::SetAppEfficiencyMode { .. }
            | Action::SuspendApp { .. }
            | Action::ResumeApp { .. }
            | Action::ConfigureBackgroundEfficiencyPolicy { .. }
            | Action::AutoBalanceBackgroundApps { .. } => {
                self.apply_app_resource_action(action, backend)
            }
        }
    }

    pub fn apply_power_plan_action(
        &self,
        action: &Action,
        backend: &mut impl PowerPlanActionBackend,
    ) -> ActionExecution {
        match action {
            Action::SwitchPowerPlan { plan_guid } => match backend.active_power_plan_guid() {
                Ok(Some(active)) if active.eq_ignore_ascii_case(plan_guid) => {
                    ActionExecution::AlreadyApplied
                }
                Ok(_) => match backend.set_active_power_plan(plan_guid) {
                    Ok(()) => ActionExecution::Applied,
                    Err(err) => ActionExecution::Failed(err),
                },
                Err(err) => ActionExecution::Failed(err),
            },
            Action::SetCoreParking {
                plan_guid,
                min_cores_percent,
                max_cores_percent,
            } => {
                match backend.set_core_parking(plan_guid, *min_cores_percent, *max_cores_percent) {
                    Ok(()) => ActionExecution::Applied,
                    Err(err) => ActionExecution::Failed(err),
                }
            }
            Action::SetProcessorPowerValues {
                plan_guid,
                ac_core_parking_min_percent,
                ac_performance_min_percent,
                ac_performance_max_percent,
                ac_boost_mode,
                dc_core_parking_min_percent,
                dc_performance_min_percent,
                dc_performance_max_percent,
                dc_boost_mode,
            } => match backend.set_processor_power_values(
                plan_guid,
                ProcessorPowerValueSet {
                    ac_core_parking_min_percent: *ac_core_parking_min_percent,
                    ac_performance_min_percent: *ac_performance_min_percent,
                    ac_performance_max_percent: *ac_performance_max_percent,
                    ac_boost_mode: *ac_boost_mode,
                    dc_core_parking_min_percent: *dc_core_parking_min_percent,
                    dc_performance_min_percent: *dc_performance_min_percent,
                    dc_performance_max_percent: *dc_performance_max_percent,
                    dc_boost_mode: *dc_boost_mode,
                },
            ) {
                Ok(()) => ActionExecution::Applied,
                Err(err) => ActionExecution::Failed(err),
            },
            _ => ActionExecution::Unsupported,
        }
    }

    pub fn apply_system_cpu_action(
        &self,
        action: &Action,
        backend: &mut impl SystemCpuActionBackend,
    ) -> ActionExecution {
        let Action::SetSystemCpuLimit {
            logical_processor_percent,
        } = action
        else {
            return ActionExecution::Unsupported;
        };

        match backend.set_system_cpu_limit(*logical_processor_percent) {
            Ok(()) => ActionExecution::Applied,
            Err(err) => ActionExecution::Failed(err),
        }
    }

    pub fn apply_app_priority_action(
        &self,
        action: &Action,
        backend: &mut impl AppPriorityActionBackend,
    ) -> ActionExecution {
        let (app, priority) = match action {
            Action::SetAppPriority { app, priority }
            | Action::BoostForegroundPriority { app, priority } => (app, *priority),
            Action::LowerBackgroundApps {
                priority,
                exclusions,
            } => {
                return match backend.lower_background_apps(*priority, exclusions) {
                    Ok(()) => ActionExecution::Applied,
                    Err(err) => ActionExecution::Failed(err),
                }
            }
            _ => return ActionExecution::Unsupported,
        };

        match backend.app_priority(app) {
            Ok(Some(current)) if current == priority => ActionExecution::AlreadyApplied,
            Ok(_) => match backend.set_app_priority(app, priority) {
                Ok(()) => ActionExecution::Applied,
                Err(err) => ActionExecution::Failed(err),
            },
            Err(err) => ActionExecution::Failed(err),
        }
    }

    pub fn apply_app_lifecycle_action(
        &self,
        action: &Action,
        backend: &mut impl AppLifecycleActionBackend,
    ) -> ActionExecution {
        match action {
            Action::TerminateApp { app } => match backend.terminate_app(app) {
                Ok(()) => ActionExecution::Applied,
                Err(err) => ActionExecution::Failed(err),
            },
            Action::RestartApp {
                app,
                launch_path,
                args,
            } => match backend.restart_app(app, launch_path, args) {
                Ok(()) => ActionExecution::Applied,
                Err(err) => ActionExecution::Failed(err),
            },
            _ => ActionExecution::Unsupported,
        }
    }

    pub fn apply_app_resource_action(
        &self,
        action: &Action,
        backend: &mut impl AppResourceActionBackend,
    ) -> ActionExecution {
        let result = match action {
            Action::SetAppEfficiencyMode { app, enabled } => {
                backend.set_app_efficiency_mode(app, *enabled)
            }
            Action::SetAppAffinity { app, affinity } => backend.set_app_affinity(app, affinity),
            Action::SetAppCpuLimit {
                app,
                logical_processor_percent,
            } => backend.set_app_cpu_limit(app, *logical_processor_percent),
            Action::SuspendApp { app } => backend.suspend_app(app),
            Action::ResumeApp { app } => backend.resume_app(app),
            Action::ConfigureBackgroundEfficiencyPolicy {
                exclusions,
                prefer_efficiency_cores,
                logical_processor_percent,
            } => backend.configure_background_efficiency_policy(
                exclusions,
                *prefer_efficiency_cores,
                *logical_processor_percent,
            ),
            Action::AutoBalanceBackgroundApps {
                cpu_threshold_percent,
                restore_threshold_percent,
            } => backend.configure_background_efficiency_policy(
                &[],
                false,
                Some((*cpu_threshold_percent).max(*restore_threshold_percent)),
            ),
            _ => return ActionExecution::Unsupported,
        };

        match result {
            Ok(()) => ActionExecution::Applied,
            Err(err) => ActionExecution::Failed(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::AppMatcher;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct FakePowerPlanBackend {
        active: Option<String>,
        set_calls: Vec<String>,
        active_error: Option<String>,
        set_error: Option<String>,
    }

    #[derive(Default)]
    struct FakeAppPriorityBackend {
        priorities: BTreeMap<String, RuleProcessPriority>,
        set_calls: Vec<(String, RuleProcessPriority)>,
        get_error: Option<String>,
        set_error: Option<String>,
    }

    #[derive(Default)]
    struct FakeLifecycleBackend {
        terminated: Vec<String>,
        restarted: Vec<(String, String, Vec<String>)>,
        terminate_error: Option<String>,
        restart_error: Option<String>,
    }

    #[derive(Default)]
    struct FakeResourceBackend {
        calls: Vec<String>,
        error: Option<String>,
    }

    impl FakeResourceBackend {
        fn record(&mut self, call: impl Into<String>) -> Result<(), String> {
            if let Some(err) = self.error.clone() {
                Err(err)
            } else {
                self.calls.push(call.into());
                Ok(())
            }
        }
    }

    impl AppResourceActionBackend for FakeResourceBackend {
        fn set_app_efficiency_mode(
            &mut self,
            app: &AppMatcher,
            enabled: bool,
        ) -> Result<(), String> {
            self.record(format!("efficiency-mode:{}:{enabled}", app_key(app)))
        }

        fn set_app_affinity(
            &mut self,
            app: &AppMatcher,
            affinity: &AffinityPolicy,
        ) -> Result<(), String> {
            self.record(format!("affinity:{}:{affinity:?}", app_key(app)))
        }

        fn set_app_cpu_limit(
            &mut self,
            app: &AppMatcher,
            logical_processor_percent: u8,
        ) -> Result<(), String> {
            self.record(format!(
                "cpu-limit:{}:{logical_processor_percent}",
                app_key(app)
            ))
        }

        fn suspend_app(&mut self, app: &AppMatcher) -> Result<(), String> {
            self.record(format!("suspend:{}", app_key(app)))
        }

        fn resume_app(&mut self, app: &AppMatcher) -> Result<(), String> {
            self.record(format!("resume:{}", app_key(app)))
        }

        fn configure_background_efficiency_policy(
            &mut self,
            exclusions: &[AppMatcher],
            prefer_efficiency_cores: bool,
            logical_processor_percent: Option<u8>,
        ) -> Result<(), String> {
            self.record(format!(
                "efficiency:{}:{}:{logical_processor_percent:?}",
                exclusions.len(),
                prefer_efficiency_cores
            ))
        }
    }

    #[derive(Default)]
    struct FakeGenericBackend {
        power: FakePowerPlanBackend,
        priority: FakeAppPriorityBackend,
        lifecycle: FakeLifecycleBackend,
        resource: FakeResourceBackend,
    }

    impl PowerPlanActionBackend for FakeGenericBackend {
        fn active_power_plan_guid(&mut self) -> Result<Option<String>, String> {
            self.power.active_power_plan_guid()
        }

        fn set_active_power_plan(&mut self, plan_guid: &str) -> Result<(), String> {
            self.power.set_active_power_plan(plan_guid)
        }

        fn set_core_parking(
            &mut self,
            plan_guid: &str,
            min_cores_percent: u8,
            max_cores_percent: u8,
        ) -> Result<(), String> {
            self.power
                .set_core_parking(plan_guid, min_cores_percent, max_cores_percent)
        }

        fn set_processor_power_values(
            &mut self,
            plan_guid: &str,
            values: ProcessorPowerValueSet,
        ) -> Result<(), String> {
            self.power.set_processor_power_values(plan_guid, values)
        }
    }

    impl AppPriorityActionBackend for FakeGenericBackend {
        fn app_priority(
            &mut self,
            app: &AppMatcher,
        ) -> Result<Option<RuleProcessPriority>, String> {
            self.priority.app_priority(app)
        }

        fn set_app_priority(
            &mut self,
            app: &AppMatcher,
            priority: RuleProcessPriority,
        ) -> Result<(), String> {
            self.priority.set_app_priority(app, priority)
        }

        fn lower_background_apps(
            &mut self,
            priority: RuleProcessPriority,
            exclusions: &[AppMatcher],
        ) -> Result<(), String> {
            self.priority.lower_background_apps(priority, exclusions)
        }
    }

    impl AppLifecycleActionBackend for FakeGenericBackend {
        fn terminate_app(&mut self, app: &AppMatcher) -> Result<(), String> {
            self.lifecycle.terminate_app(app)
        }

        fn restart_app(
            &mut self,
            app: &AppMatcher,
            launch_path: &str,
            args: &[String],
        ) -> Result<(), String> {
            self.lifecycle.restart_app(app, launch_path, args)
        }
    }

    impl AppResourceActionBackend for FakeGenericBackend {
        fn set_app_efficiency_mode(
            &mut self,
            app: &AppMatcher,
            enabled: bool,
        ) -> Result<(), String> {
            self.resource.set_app_efficiency_mode(app, enabled)
        }

        fn set_app_affinity(
            &mut self,
            app: &AppMatcher,
            affinity: &AffinityPolicy,
        ) -> Result<(), String> {
            self.resource.set_app_affinity(app, affinity)
        }

        fn set_app_cpu_limit(
            &mut self,
            app: &AppMatcher,
            logical_processor_percent: u8,
        ) -> Result<(), String> {
            self.resource
                .set_app_cpu_limit(app, logical_processor_percent)
        }

        fn suspend_app(&mut self, app: &AppMatcher) -> Result<(), String> {
            self.resource.suspend_app(app)
        }

        fn resume_app(&mut self, app: &AppMatcher) -> Result<(), String> {
            self.resource.resume_app(app)
        }

        fn configure_background_efficiency_policy(
            &mut self,
            exclusions: &[AppMatcher],
            prefer_efficiency_cores: bool,
            logical_processor_percent: Option<u8>,
        ) -> Result<(), String> {
            self.resource.configure_background_efficiency_policy(
                exclusions,
                prefer_efficiency_cores,
                logical_processor_percent,
            )
        }
    }

    impl SystemCpuActionBackend for FakeGenericBackend {
        fn set_system_cpu_limit(&mut self, logical_processor_percent: u8) -> Result<(), String> {
            self.resource
                .record(format!("system-cpu:{logical_processor_percent}"))
        }
    }

    impl AppLifecycleActionBackend for FakeLifecycleBackend {
        fn terminate_app(&mut self, app: &AppMatcher) -> Result<(), String> {
            if let Some(err) = self.terminate_error.clone() {
                return Err(err);
            }

            self.terminated.push(app_key(app));
            Ok(())
        }

        fn restart_app(
            &mut self,
            app: &AppMatcher,
            launch_path: &str,
            args: &[String],
        ) -> Result<(), String> {
            if let Some(err) = self.restart_error.clone() {
                return Err(err);
            }

            self.restarted
                .push((app_key(app), launch_path.to_owned(), args.to_vec()));
            Ok(())
        }
    }

    impl AppPriorityActionBackend for FakeAppPriorityBackend {
        fn app_priority(
            &mut self,
            app: &AppMatcher,
        ) -> Result<Option<RuleProcessPriority>, String> {
            if let Some(err) = self.get_error.clone() {
                return Err(err);
            }

            Ok(self.priorities.get(&app_key(app)).copied())
        }

        fn set_app_priority(
            &mut self,
            app: &AppMatcher,
            priority: RuleProcessPriority,
        ) -> Result<(), String> {
            if let Some(err) = self.set_error.clone() {
                return Err(err);
            }

            let key = app_key(app);
            self.priorities.insert(key.clone(), priority);
            self.set_calls.push((key, priority));
            Ok(())
        }

        fn lower_background_apps(
            &mut self,
            priority: RuleProcessPriority,
            exclusions: &[AppMatcher],
        ) -> Result<(), String> {
            if let Some(err) = self.set_error.clone() {
                return Err(err);
            }

            self.set_calls
                .push((format!("background:{}", exclusions.len()), priority));
            Ok(())
        }
    }

    fn app_key(app: &AppMatcher) -> String {
        match app {
            AppMatcher::ProcessName(value)
            | AppMatcher::Path(value)
            | AppMatcher::Pattern(value) => value.trim().to_ascii_lowercase(),
        }
    }

    impl PowerPlanActionBackend for FakePowerPlanBackend {
        fn active_power_plan_guid(&mut self) -> Result<Option<String>, String> {
            if let Some(err) = self.active_error.clone() {
                Err(err)
            } else {
                Ok(self.active.clone())
            }
        }

        fn set_active_power_plan(&mut self, plan_guid: &str) -> Result<(), String> {
            if let Some(err) = self.set_error.clone() {
                Err(err)
            } else {
                self.active = Some(plan_guid.to_owned());
                self.set_calls.push(plan_guid.to_owned());
                Ok(())
            }
        }

        fn set_core_parking(
            &mut self,
            plan_guid: &str,
            min_cores_percent: u8,
            max_cores_percent: u8,
        ) -> Result<(), String> {
            if let Some(err) = self.set_error.clone() {
                Err(err)
            } else {
                self.set_calls.push(format!(
                    "core-parking:{plan_guid}:{min_cores_percent}:{max_cores_percent}"
                ));
                Ok(())
            }
        }

        fn set_processor_power_values(
            &mut self,
            plan_guid: &str,
            values: ProcessorPowerValueSet,
        ) -> Result<(), String> {
            if let Some(err) = self.set_error.clone() {
                Err(err)
            } else {
                self.set_calls.push(format!(
                    "processor-power:{plan_guid}:{}:{}:{}:{}:{}:{}:{}:{}",
                    values.ac_core_parking_min_percent,
                    values.ac_performance_min_percent,
                    values.ac_performance_max_percent,
                    values.ac_boost_mode,
                    values.dc_core_parking_min_percent,
                    values.dc_performance_min_percent,
                    values.dc_performance_max_percent,
                    values.dc_boost_mode
                ));
                Ok(())
            }
        }
    }

    #[test]
    fn switch_power_plan_applies_when_target_differs() {
        let mut backend = FakePowerPlanBackend {
            active: Some("old-guid".to_owned()),
            ..Default::default()
        };
        let action = Action::SwitchPowerPlan {
            plan_guid: "new-guid".to_owned(),
        };

        assert_eq!(
            ActionExecutor.apply_power_plan_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(backend.active.as_deref(), Some("new-guid"));
        assert_eq!(backend.set_calls, vec!["new-guid"]);
    }

    #[test]
    fn switch_power_plan_is_idempotent() {
        let mut backend = FakePowerPlanBackend {
            active: Some("same-guid".to_owned()),
            ..Default::default()
        };
        let action = Action::SwitchPowerPlan {
            plan_guid: "SAME-GUID".to_owned(),
        };

        assert_eq!(
            ActionExecutor.apply_power_plan_action(&action, &mut backend),
            ActionExecution::AlreadyApplied
        );
        assert!(backend.set_calls.is_empty());
    }

    #[test]
    fn unsupported_action_is_skipped() {
        let mut backend = FakePowerPlanBackend::default();
        let action = Action::SetSystemCpuLimit {
            logical_processor_percent: 50,
        };

        assert_eq!(
            ActionExecutor.apply_power_plan_action(&action, &mut backend),
            ActionExecution::Unsupported
        );
    }

    #[test]
    fn backend_errors_are_reported() {
        let mut backend = FakePowerPlanBackend {
            active: Some("old-guid".to_owned()),
            set_error: Some("failed".to_owned()),
            ..Default::default()
        };
        let action = Action::SwitchPowerPlan {
            plan_guid: "new-guid".to_owned(),
        };

        assert_eq!(
            ActionExecutor.apply_power_plan_action(&action, &mut backend),
            ActionExecution::Failed("failed".to_owned())
        );
    }

    #[test]
    fn core_parking_action_uses_power_plan_backend() {
        let mut backend = FakePowerPlanBackend::default();
        let action = Action::SetCoreParking {
            plan_guid: "plan-guid".to_owned(),
            min_cores_percent: 10,
            max_cores_percent: 80,
        };

        assert_eq!(
            ActionExecutor.apply_power_plan_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(backend.set_calls, vec!["core-parking:plan-guid:10:80"]);
    }

    #[test]
    fn processor_power_action_preserves_ac_and_dc_values() {
        let mut backend = FakePowerPlanBackend::default();
        let action = Action::SetProcessorPowerValues {
            plan_guid: "plan-guid".to_owned(),
            ac_core_parking_min_percent: 10,
            ac_performance_min_percent: 20,
            ac_performance_max_percent: 90,
            ac_boost_mode: 2,
            dc_core_parking_min_percent: 5,
            dc_performance_min_percent: 15,
            dc_performance_max_percent: 70,
            dc_boost_mode: 3,
        };

        assert_eq!(
            ActionExecutor.apply_power_plan_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(
            backend.set_calls,
            vec!["processor-power:plan-guid:10:20:90:2:5:15:70:3"]
        );
    }

    #[test]
    fn set_app_priority_applies_when_target_differs() {
        let mut backend = FakeAppPriorityBackend::default();
        let action = Action::SetAppPriority {
            app: AppMatcher::ProcessName("worker.exe".to_owned()),
            priority: RuleProcessPriority::BelowNormal,
        };

        assert_eq!(
            ActionExecutor.apply_app_priority_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(
            backend.priorities.get("worker.exe"),
            Some(&RuleProcessPriority::BelowNormal)
        );
    }

    #[test]
    fn boost_foreground_priority_is_idempotent() {
        let mut backend = FakeAppPriorityBackend::default();
        backend
            .priorities
            .insert("game.exe".to_owned(), RuleProcessPriority::AboveNormal);
        let action = Action::BoostForegroundPriority {
            app: AppMatcher::ProcessName("GAME.EXE".to_owned()),
            priority: RuleProcessPriority::AboveNormal,
        };

        assert_eq!(
            ActionExecutor.apply_app_priority_action(&action, &mut backend),
            ActionExecution::AlreadyApplied
        );
        assert!(backend.set_calls.is_empty());
    }

    #[test]
    fn app_priority_backend_errors_are_reported() {
        let mut backend = FakeAppPriorityBackend {
            set_error: Some("priority failed".to_owned()),
            ..Default::default()
        };
        let action = Action::SetAppPriority {
            app: AppMatcher::ProcessName("worker.exe".to_owned()),
            priority: RuleProcessPriority::Idle,
        };

        assert_eq!(
            ActionExecutor.apply_app_priority_action(&action, &mut backend),
            ActionExecution::Failed("priority failed".to_owned())
        );
    }

    #[test]
    fn lower_background_apps_action_uses_priority_backend() {
        let mut backend = FakeAppPriorityBackend::default();
        let action = Action::LowerBackgroundApps {
            priority: RuleProcessPriority::BelowNormal,
            exclusions: vec![AppMatcher::ProcessName("game.exe".to_owned())],
        };

        assert_eq!(
            ActionExecutor.apply_app_priority_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(
            backend.set_calls,
            vec![("background:1".to_owned(), RuleProcessPriority::BelowNormal)]
        );
    }

    #[test]
    fn terminate_app_action_uses_lifecycle_backend() {
        let mut backend = FakeLifecycleBackend::default();
        let action = Action::TerminateApp {
            app: AppMatcher::ProcessName("Tool.EXE".to_owned()),
        };

        assert_eq!(
            ActionExecutor.apply_app_lifecycle_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(backend.terminated, vec!["tool.exe"]);
    }

    #[test]
    fn restart_app_action_uses_lifecycle_backend() {
        let mut backend = FakeLifecycleBackend::default();
        let action = Action::RestartApp {
            app: AppMatcher::ProcessName("tool.exe".to_owned()),
            launch_path: "C:\\Tools\\tool.exe".to_owned(),
            args: vec!["--minimized".to_owned()],
        };

        assert_eq!(
            ActionExecutor.apply_app_lifecycle_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(
            backend.restarted,
            vec![(
                "tool.exe".to_owned(),
                "C:\\Tools\\tool.exe".to_owned(),
                vec!["--minimized".to_owned()]
            )]
        );
    }

    #[test]
    fn lifecycle_backend_errors_are_reported() {
        let mut backend = FakeLifecycleBackend {
            terminate_error: Some("terminate failed".to_owned()),
            ..Default::default()
        };
        let action = Action::TerminateApp {
            app: AppMatcher::ProcessName("tool.exe".to_owned()),
        };

        assert_eq!(
            ActionExecutor.apply_app_lifecycle_action(&action, &mut backend),
            ActionExecution::Failed("terminate failed".to_owned())
        );
    }

    #[test]
    fn app_resource_affinity_action_uses_backend() {
        let mut backend = FakeResourceBackend::default();
        let action = Action::SetAppAffinity {
            app: AppMatcher::ProcessName("worker.exe".to_owned()),
            affinity: AffinityPolicy::CustomMask(3),
        };

        assert_eq!(
            ActionExecutor.apply_app_resource_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(backend.calls, vec!["affinity:worker.exe:CustomMask(3)"]);
    }

    #[test]
    fn app_resource_affinity_action_accepts_cpu_set_policy() {
        let mut backend = FakeResourceBackend::default();
        let action = Action::SetAppAffinity {
            app: AppMatcher::ProcessName("worker.exe".to_owned()),
            affinity: AffinityPolicy::CpuSetMask(3),
        };

        assert_eq!(
            ActionExecutor.apply_app_resource_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(backend.calls, vec!["affinity:worker.exe:CpuSetMask(3)"]);
    }

    #[test]
    fn app_resource_affinity_action_accepts_efficiency_mode_off_policy() {
        let mut backend = FakeResourceBackend::default();
        let action = Action::SetAppAffinity {
            app: AppMatcher::ProcessName("worker.exe".to_owned()),
            affinity: AffinityPolicy::DisableEfficiencyMode,
        };

        assert_eq!(
            ActionExecutor.apply_app_resource_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(
            backend.calls,
            vec!["affinity:worker.exe:DisableEfficiencyMode"]
        );
    }

    #[test]
    fn app_efficiency_mode_action_uses_resource_backend() {
        let mut backend = FakeResourceBackend::default();
        let action = Action::SetAppEfficiencyMode {
            app: AppMatcher::ProcessName("worker.exe".to_owned()),
            enabled: true,
        };

        assert_eq!(
            ActionExecutor.apply_app_resource_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(backend.calls, vec!["efficiency-mode:worker.exe:true"]);
    }

    #[test]
    fn app_resource_cpu_limit_action_uses_backend() {
        let mut backend = FakeResourceBackend::default();
        let action = Action::SetAppCpuLimit {
            app: AppMatcher::ProcessName("worker.exe".to_owned()),
            logical_processor_percent: 25,
        };

        assert_eq!(
            ActionExecutor.apply_app_resource_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(backend.calls, vec!["cpu-limit:worker.exe:25"]);
    }

    #[test]
    fn app_resource_suspend_and_resume_actions_use_backend() {
        let mut backend = FakeResourceBackend::default();
        let suspend = Action::SuspendApp {
            app: AppMatcher::ProcessName("chat.exe".to_owned()),
        };
        let resume = Action::ResumeApp {
            app: AppMatcher::ProcessName("chat.exe".to_owned()),
        };

        assert_eq!(
            ActionExecutor.apply_app_resource_action(&suspend, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(
            ActionExecutor.apply_app_resource_action(&resume, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(backend.calls, vec!["suspend:chat.exe", "resume:chat.exe"]);
    }

    #[test]
    fn background_efficiency_policy_action_uses_backend() {
        let mut backend = FakeResourceBackend::default();
        let action = Action::ConfigureBackgroundEfficiencyPolicy {
            exclusions: vec![AppMatcher::ProcessName("game.exe".to_owned())],
            prefer_efficiency_cores: true,
            logical_processor_percent: Some(50),
        };

        assert_eq!(
            ActionExecutor.apply_app_resource_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(backend.calls, vec!["efficiency:1:true:Some(50)"]);
    }

    #[test]
    fn app_resource_backend_errors_are_reported() {
        let mut backend = FakeResourceBackend {
            error: Some("resource failed".to_owned()),
            ..Default::default()
        };
        let action = Action::SuspendApp {
            app: AppMatcher::ProcessName("chat.exe".to_owned()),
        };

        assert_eq!(
            ActionExecutor.apply_app_resource_action(&action, &mut backend),
            ActionExecution::Failed("resource failed".to_owned())
        );
    }

    #[test]
    fn system_cpu_limit_action_uses_system_backend() {
        let mut backend = FakeGenericBackend::default();
        let action = Action::SetSystemCpuLimit {
            logical_processor_percent: 60,
        };

        assert_eq!(
            ActionExecutor.apply_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(backend.resource.calls, vec!["system-cpu:60"]);
    }

    #[test]
    fn auto_balance_action_uses_resource_backend() {
        let mut backend = FakeResourceBackend::default();
        let action = Action::AutoBalanceBackgroundApps {
            cpu_threshold_percent: 25,
            restore_threshold_percent: 5,
        };

        assert_eq!(
            ActionExecutor.apply_app_resource_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(backend.calls, vec!["efficiency:0:false:Some(25)"]);
    }

    #[test]
    fn unified_dispatcher_routes_power_plan_actions() {
        let mut backend = FakeGenericBackend::default();
        let action = Action::SwitchPowerPlan {
            plan_guid: "target-guid".to_owned(),
        };

        assert_eq!(
            ActionExecutor.apply_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(backend.power.active.as_deref(), Some("target-guid"));
    }

    #[test]
    fn unified_dispatcher_routes_priority_actions() {
        let mut backend = FakeGenericBackend::default();
        let action = Action::SetAppPriority {
            app: AppMatcher::ProcessName("worker.exe".to_owned()),
            priority: RuleProcessPriority::Idle,
        };

        assert_eq!(
            ActionExecutor.apply_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(
            backend.priority.priorities.get("worker.exe"),
            Some(&RuleProcessPriority::Idle)
        );
    }

    #[test]
    fn unified_dispatcher_routes_lifecycle_actions() {
        let mut backend = FakeGenericBackend::default();
        let action = Action::TerminateApp {
            app: AppMatcher::ProcessName("tool.exe".to_owned()),
        };

        assert_eq!(
            ActionExecutor.apply_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(backend.lifecycle.terminated, vec!["tool.exe"]);
    }

    #[test]
    fn unified_dispatcher_routes_resource_actions() {
        let mut backend = FakeGenericBackend::default();
        let action = Action::SuspendApp {
            app: AppMatcher::ProcessName("chat.exe".to_owned()),
        };

        assert_eq!(
            ActionExecutor.apply_action(&action, &mut backend),
            ActionExecution::Applied
        );
        assert_eq!(backend.resource.calls, vec!["suspend:chat.exe"]);
    }
}
