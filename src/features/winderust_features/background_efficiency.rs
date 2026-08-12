use std::{collections::BTreeSet, path::PathBuf};

#[cfg(test)]
use std::path::Path;

use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    audio_activity::active_audio_process_ids,
    config::{BackgroundEfficiencyAggressiveness, BackgroundEfficiencySettings},
    control::{
        priority_efficiency::{
            EfficiencyModeClaim, PriorityEfficiencyController, PriorityEfficiencyReleaseSummary,
            ProcessPropertyApplyOutcome,
        },
        process::{ControlOwner, ProcessControlError, ProcessControlTarget, ProcessTargetKey},
    },
    foreground::{
        contains_process_name, process_executable_path, process_failure_key, process_session_id,
        ProtectedProcesses,
    },
    rules::{
        execution_failure_suppression_threshold, ExecutionFailureTracker, ExecutionSuppression,
    },
    runtime::observations::CycleObservations,
};

const BUILT_IN_EXCLUSIONS: &[&str] = &[
    "audiodg.exe",
    "conhost.exe",
    "csrss.exe",
    "ctfmon.exe",
    "dwm.exe",
    "explorer.exe",
    "fontdrvhost.exe",
    "lsaiso.exe",
    "lsass.exe",
    "registry",
    "searchhost.exe",
    "securityhealthservice.exe",
    "securityhealthsystray.exe",
    "services.exe",
    "shellexperiencehost.exe",
    "sihost.exe",
    "smss.exe",
    "startmenuexperiencehost.exe",
    "system",
    "taskmgr.exe",
    "textinputhost.exe",
    "wininit.exe",
    "winlogon.exe",
    "wudfhost.exe",
];

const BALANCED_BUILT_IN_EXCLUSIONS: &[&str] = &[
    "audiodg.exe",
    "conhost.exe",
    "csrss.exe",
    "ctfmon.exe",
    "dwm.exe",
    "explorer.exe",
    "fontdrvhost.exe",
    "lsaiso.exe",
    "lsass.exe",
    "registry",
    "securityhealthservice.exe",
    "securityhealthsystray.exe",
    "services.exe",
    "sihost.exe",
    "smss.exe",
    "startmenuexperiencehost.exe",
    "system",
    "taskmgr.exe",
    "textinputhost.exe",
    "wininit.exe",
    "winlogon.exe",
];

const AGGRESSIVE_BUILT_IN_EXCLUSIONS: &[&str] = &[
    "csrss.exe",
    "lsaiso.exe",
    "lsass.exe",
    "registry",
    "services.exe",
    "smss.exe",
    "system",
    "wininit.exe",
    "winlogon.exe",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundEfficiencySnapshot {
    pub enabled: bool,
    pub unsupported: bool,
    pub scanned_processes: usize,
    pub throttled_processes: usize,
    pub timer_resolution_ignored_processes: usize,
    pub skipped_processes: usize,
    pub access_denied_processes: usize,
    pub failed_processes: usize,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct BackgroundEfficiencyManager {
    failure_suppression: ExecutionFailureTracker,
}

struct BackgroundEfficiencyTarget {
    process_id: u32,
    process_name: String,
    executable_path: String,
    creation_time: u64,
    ignore_timer_resolution: bool,
}

impl BackgroundEfficiencyManager {
    #[expect(
        clippy::too_many_arguments,
        reason = "the policy boundary receives shared observations and its state controller"
    )]
    pub(crate) fn update(
        &mut self,
        controller: &mut PriorityEfficiencyController,
        settings: &BackgroundEfficiencySettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> BackgroundEfficiencySnapshot {
        if !automation_enabled {
            return self.disabled_snapshot(controller, action_log, "automation disabled");
        }
        if !settings.enabled {
            return self.disabled_snapshot(
                controller,
                action_log,
                "Background Efficiency disabled",
            );
        }
        if settings.protect_foreground_app && foreground_process_id.is_none() {
            return self.paused_snapshot(
                controller,
                action_log,
                "foreground app is unknown",
                "Paused: foreground app is unknown.",
            );
        }

        let visible_window_process_ids = if settings.protect_visible_window_apps {
            let Ok(process_ids) = observations.visible_window_process_ids() else {
                return self.paused_snapshot(
                    controller,
                    action_log,
                    "visible windows are unavailable",
                    "Paused: visible windows are unavailable.",
                );
            };
            process_ids
        } else {
            Default::default()
        };

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            return self.paused_snapshot(
                controller,
                action_log,
                "current Windows session is unknown",
                "Paused: current Windows session is unknown.",
            );
        };
        let processes = match observations.processes() {
            Ok(processes) => processes,
            Err(error) => {
                return self.paused_snapshot(
                    controller,
                    action_log,
                    "process list unavailable",
                    &error,
                );
            }
        };

        let scanned_processes = processes.len();
        let protected_processes = ProtectedProcesses::capture(
            processes.as_ref(),
            settings.protect_foreground_app,
            foreground_process_id,
            visible_window_process_ids,
        );
        let active_audio_process_ids = active_audio_process_ids().ok();
        let targets = processes
            .iter()
            .filter_map(|process| {
                if process.id == 0
                    || process.id == current_process_id
                    || process.is_critical != Some(false)
                    || !process.can_set_information
                    || is_builtin_excluded_for(&process.name, settings.aggressiveness)
                    || (!allow_cross_session_process_control
                        && process_session_id(process.id) != Some(current_session_id))
                {
                    return None;
                }
                let executable_path = process_executable_path(process)?;
                let creation_time = process.creation_time?;
                if protected_processes.contains(process.id, &executable_path)
                    || settings.custom_rule_enabled_for(executable_path.to_string_lossy().as_ref())
                {
                    return None;
                }
                Some(BackgroundEfficiencyTarget {
                    process_id: process.id,
                    process_name: process.name.clone(),
                    executable_path: executable_path.to_string_lossy().into_owned(),
                    creation_time,
                    ignore_timer_resolution: ignore_timer_resolution_allowed(
                        process.id,
                        active_audio_process_ids.as_ref(),
                    ),
                })
            })
            .collect::<Vec<_>>();

        let active_targets = targets
            .iter()
            .map(background_efficiency_target_key)
            .collect::<BTreeSet<_>>();
        let active_target_names = targets
            .iter()
            .map(|target| process_failure_key(&target.executable_path))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);

        let mut failures = BackgroundEfficiencyFailures::default();
        let mut unsupported = false;
        self.merge_release_summary(
            controller.release_efficiency_policy_except(
                ControlOwner::BackgroundEfficiency,
                &active_targets,
            ),
            action_log,
            "process no longer matches Background Efficiency",
            &mut failures,
            &mut unsupported,
        );

        let mut skipped_processes = 0;
        let mut access_denied_processes = 0;
        for target in targets {
            if self
                .check_process_suppression(
                    target.process_id,
                    &target.process_name,
                    &target.executable_path,
                    action_log,
                )
                .suppressed
            {
                skipped_processes += 1;
                continue;
            }
            let claim = EfficiencyModeClaim {
                target: ProcessControlTarget::automatic(
                    target.process_id,
                    target.process_name.clone(),
                    PathBuf::from(&target.executable_path),
                    target.creation_time,
                ),
                owner: ControlOwner::BackgroundEfficiency,
                ignore_timer_resolution: target.ignore_timer_resolution,
            };
            match controller.apply_efficiency_claim(claim, allow_cross_session_process_control) {
                Ok(ProcessPropertyApplyOutcome::Applied) => {
                    self.clear_process_failure(&target.executable_path);
                    action_log.record(
                        ActionLogFeature::BackgroundEfficiency,
                        Some(target.process_id),
                        target.process_name,
                        ActionLogResult::Applied,
                        "Applied Background Efficiency: enabled EcoQoS and lowered priority.",
                    );
                }
                Ok(
                    ProcessPropertyApplyOutcome::Unchanged
                    | ProcessPropertyApplyOutcome::Preserved
                    | ProcessPropertyApplyOutcome::Shadowed,
                ) => self.clear_process_failure(&target.executable_path),
                Err(ProcessControlError::ProcessExited) => skipped_processes += 1,
                Err(ProcessControlError::AccessDenied(message)) => {
                    skipped_processes += 1;
                    access_denied_processes += 1;
                    self.failure_suppression
                        .suppress_process_failure(&target.executable_path);
                    action_log.record(
                        ActionLogFeature::BackgroundEfficiency,
                        Some(target.process_id),
                        target.process_name,
                        ActionLogResult::Skipped,
                        message,
                    );
                }
                Err(ProcessControlError::Unavailable(message)) => {
                    skipped_processes += 1;
                    unsupported = true;
                    self.failure_suppression
                        .suppress_process_failure(&target.executable_path);
                    action_log.record(
                        ActionLogFeature::BackgroundEfficiency,
                        Some(target.process_id),
                        target.process_name,
                        ActionLogResult::Skipped,
                        message,
                    );
                }
                Err(error) => {
                    self.record_process_failure(&target.executable_path);
                    failures.record(
                        "Apply",
                        target.process_id,
                        &target.process_name,
                        error,
                        action_log,
                    );
                }
            }
        }

        BackgroundEfficiencySnapshot {
            enabled: true,
            unsupported,
            scanned_processes,
            throttled_processes: controller
                .policy_managed_process_count(ControlOwner::BackgroundEfficiency),
            timer_resolution_ignored_processes: controller
                .policy_ignore_timer_resolution_count(ControlOwner::BackgroundEfficiency),
            skipped_processes,
            access_denied_processes,
            failed_processes: failures.count,
            message: "Background Efficiency active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn disabled_snapshot(
        &mut self,
        controller: &mut PriorityEfficiencyController,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> BackgroundEfficiencySnapshot {
        let mut failures = BackgroundEfficiencyFailures::default();
        let mut unsupported = false;
        self.merge_release_summary(
            controller.release_all_efficiency_policy(ControlOwner::BackgroundEfficiency),
            action_log,
            reason,
            &mut failures,
            &mut unsupported,
        );
        self.failure_suppression.clear();
        BackgroundEfficiencySnapshot {
            enabled: false,
            unsupported,
            failed_processes: failures.count,
            message: if reason == "automation disabled" {
                "Automation disabled.".to_owned()
            } else {
                "Background Efficiency disabled.".to_owned()
            },
            last_error: failures.last_error,
            ..Default::default()
        }
    }

    fn paused_snapshot(
        &mut self,
        controller: &mut PriorityEfficiencyController,
        action_log: &mut ActionLog,
        reason: &str,
        message: &str,
    ) -> BackgroundEfficiencySnapshot {
        let mut failures = BackgroundEfficiencyFailures::default();
        let mut unsupported = false;
        self.merge_release_summary(
            controller.release_all_efficiency_policy(ControlOwner::BackgroundEfficiency),
            action_log,
            reason,
            &mut failures,
            &mut unsupported,
        );
        BackgroundEfficiencySnapshot {
            enabled: true,
            unsupported,
            failed_processes: failures.count,
            message: message.to_owned(),
            last_error: failures.last_error,
            ..Default::default()
        }
    }

    fn merge_release_summary(
        &mut self,
        summary: PriorityEfficiencyReleaseSummary,
        action_log: &mut ActionLog,
        reason: &str,
        failures: &mut BackgroundEfficiencyFailures,
        unsupported: &mut bool,
    ) {
        if summary.restored_processes > 0 {
            action_log.record(
                ActionLogFeature::BackgroundEfficiency,
                None,
                "Background Efficiency",
                ActionLogResult::Restored,
                format!(
                    "Restored Background Efficiency for {} process(es): {reason}.",
                    summary.restored_processes
                ),
            );
        }
        for failure in summary.failures {
            *unsupported |= matches!(failure.error, ProcessControlError::Unavailable(_));
            self.record_process_failure(&failure.executable_path);
            failures.record(
                &format!("Restore {}", failure.property),
                failure.process_id,
                &failure.process_name,
                failure.error,
                action_log,
            );
        }
    }

    fn check_process_suppression(
        &mut self,
        process_id: u32,
        process_name: &str,
        executable_path: &str,
        action_log: &mut ActionLog,
    ) -> ExecutionSuppression {
        let suppression = self
            .failure_suppression
            .process_suppression(executable_path);
        if suppression.newly_suppressed {
            action_log.record(
                ActionLogFeature::BackgroundEfficiency,
                Some(process_id),
                process_name.to_owned(),
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Background Efficiency after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }
        suppression
    }

    #[cfg(test)]
    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log: &mut ActionLog,
    ) -> bool {
        self.check_process_suppression(process_id, process_name, process_name, action_log)
            .suppressed
    }

    fn record_process_failure(&mut self, process_name: &str) {
        self.failure_suppression
            .record_process_failure(process_name);
    }

    fn clear_process_failure(&mut self, process_name: &str) {
        self.failure_suppression.clear_process_failure(process_name);
    }
}

impl Default for BackgroundEfficiencySnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            unsupported: false,
            scanned_processes: 0,
            throttled_processes: 0,
            timer_resolution_ignored_processes: 0,
            skipped_processes: 0,
            access_denied_processes: 0,
            failed_processes: 0,
            message: "Background Efficiency disabled.".to_owned(),
            last_error: None,
        }
    }
}

#[derive(Default)]
struct BackgroundEfficiencyFailures {
    count: usize,
    last_error: Option<String>,
}

impl BackgroundEfficiencyFailures {
    fn record(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: ProcessControlError,
        action_log: &mut ActionLog,
    ) {
        let message = error.to_string();
        if self.last_error.is_none() {
            self.last_error = Some(process_failure_message(
                action,
                process_id,
                process_name,
                &message,
            ));
        }
        self.count += 1;
        action_log.record(
            ActionLogFeature::BackgroundEfficiency,
            Some(process_id),
            process_name.to_owned(),
            ActionLogResult::Failed,
            message,
        );
    }
}

fn background_efficiency_target_key(target: &BackgroundEfficiencyTarget) -> ProcessTargetKey {
    ProcessControlTarget::automatic(
        target.process_id,
        target.process_name.clone(),
        PathBuf::from(&target.executable_path),
        target.creation_time,
    )
    .key()
}

fn process_failure_message(
    action: &str,
    process_id: u32,
    process_name: &str,
    message: &str,
) -> String {
    format!("{action} {process_name} ({process_id}): {message}")
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    is_builtin_excluded_for(process_name, BackgroundEfficiencyAggressiveness::Safe)
}

#[cfg(test)]
fn is_process_excluded(process: &str, settings: &BackgroundEfficiencySettings) -> bool {
    let process_name = Path::new(process)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(process);
    is_builtin_excluded_for(process_name, settings.aggressiveness)
        || settings.custom_rule_enabled_for(process)
}

fn is_builtin_excluded_for(
    process_name: &str,
    aggressiveness: BackgroundEfficiencyAggressiveness,
) -> bool {
    contains_process_name(built_in_exclusions_for(aggressiveness), process_name)
}

fn built_in_exclusions_for(
    aggressiveness: BackgroundEfficiencyAggressiveness,
) -> &'static [&'static str] {
    match aggressiveness {
        BackgroundEfficiencyAggressiveness::Safe => BUILT_IN_EXCLUSIONS,
        BackgroundEfficiencyAggressiveness::Balanced => BALANCED_BUILT_IN_EXCLUSIONS,
        BackgroundEfficiencyAggressiveness::Aggressive => AGGRESSIVE_BUILT_IN_EXCLUSIONS,
    }
}

fn ignore_timer_resolution_allowed(
    process_id: u32,
    active_audio_process_ids: Option<&BTreeSet<u32>>,
) -> bool {
    active_audio_process_ids.is_some_and(|ids| !ids.contains(&process_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusions_include_builtin_and_user_entries() {
        let settings = BackgroundEfficiencySettings {
            enabled: true,
            protect_foreground_app: true,
            protect_visible_window_apps: false,
            aggressiveness: BackgroundEfficiencyAggressiveness::Safe,
            custom_rules: vec![crate::config::BackgroundEfficiencyRule {
                enabled: true,
                executable_path: "mouse.exe".to_owned(),
            }],
        };

        assert!(is_process_excluded("EXPLORER.EXE", &settings));
        assert!(is_process_excluded("csrss.exe", &settings));
        assert!(is_process_excluded("winlogon.exe", &settings));
        assert!(is_process_excluded("Mouse.exe", &settings));
        assert!(!is_process_excluded("browser.exe", &settings));
    }

    #[test]
    fn aggressiveness_profiles_control_builtin_exclusions() {
        let mut settings = BackgroundEfficiencySettings {
            aggressiveness: BackgroundEfficiencyAggressiveness::Safe,
            ..Default::default()
        };

        assert!(is_process_excluded("SearchHost.exe", &settings));
        assert!(is_process_excluded("dwm.exe", &settings));
        assert!(is_process_excluded("winlogon.exe", &settings));

        settings.aggressiveness = BackgroundEfficiencyAggressiveness::Balanced;
        assert!(!is_process_excluded("SearchHost.exe", &settings));
        assert!(is_process_excluded("dwm.exe", &settings));
        assert!(is_process_excluded("winlogon.exe", &settings));

        settings.aggressiveness = BackgroundEfficiencyAggressiveness::Aggressive;
        assert!(!is_process_excluded("SearchHost.exe", &settings));
        assert!(!is_process_excluded("dwm.exe", &settings));
        assert!(is_process_excluded("winlogon.exe", &settings));
    }

    #[test]
    fn disabled_user_exclusions_do_not_exclude_processes() {
        let settings = BackgroundEfficiencySettings {
            custom_rules: vec![crate::config::BackgroundEfficiencyRule {
                enabled: false,
                executable_path: "mouse.exe".to_owned(),
            }],
            ..Default::default()
        };

        assert!(settings.contains_custom_rule("MOUSE.EXE"));
        assert!(!is_process_excluded("mouse.exe", &settings));
    }

    #[test]
    fn process_failure_message_includes_action_name_pid_and_error() {
        assert_eq!(
            process_failure_message("Restore", 42, "browser.exe", "OpenProcess failed."),
            "Restore browser.exe (42): OpenProcess failed."
        );
    }

    #[test]
    fn repeated_failures_suppress_future_efficiency_attempts_once() {
        let mut manager = BackgroundEfficiencyManager::default();
        let mut log = ActionLog::new(8);
        let executable_path = r"C:\Apps\app.exe";

        manager.record_process_failure(executable_path);
        manager.record_process_failure(executable_path);
        assert!(
            !manager
                .check_process_suppression(42, "app.exe", executable_path, &mut log)
                .suppressed
        );
        assert!(log.entries().is_empty());

        manager.record_process_failure(executable_path);
        assert!(
            manager
                .check_process_suppression(42, "app.exe", executable_path, &mut log)
                .suppressed
        );
        assert!(manager.is_process_suppressed(43, r"C:/Apps/app.exe", &mut log));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].process_name, "app.exe");
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
    }

    #[test]
    fn first_suppression_reports_once() {
        let mut manager = BackgroundEfficiencyManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");

        let first = manager.check_process_suppression(42, "app.exe", "app.exe", &mut log);
        let second = manager.check_process_suppression(42, "app.exe", "app.exe", &mut log);

        assert!(first.suppressed);
        assert!(first.newly_suppressed);
        assert!(second.suppressed);
        assert!(!second.newly_suppressed);
    }

    #[test]
    fn timer_ignore_guard_fails_closed_for_audio_detection() {
        let mut audio_processes = BTreeSet::new();
        audio_processes.insert(42);

        assert!(!ignore_timer_resolution_allowed(42, Some(&audio_processes)));
        assert!(ignore_timer_resolution_allowed(7, Some(&audio_processes)));
        assert!(!ignore_timer_resolution_allowed(7, None));
    }
}
