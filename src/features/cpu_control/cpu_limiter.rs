use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{CpuLimiterRule, CpuLimiterSettings, ProcessRuleMode},
    control::{
        cpu_limiter::{CpuLimiterController, CpuLimiterTarget},
        process::ProcessTargetKey,
        suspension::{self as suspension_control, SuspensionTarget},
    },
    foreground::{
        is_foreground_process, process_executable_path, process_failure_key, process_session_id,
        same_executable_path, ProcessInfo, ProtectedProcesses,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
    runtime::observations::CycleObservations,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CpuLimiterSnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub limited_processes: usize,
    pub tracked_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub limited_apps: Vec<String>,
    pub auto_excluded_processes: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct CpuLimiterManager {
    limited: BTreeMap<ProcessTargetKey, LimitedProcess>,
    failure_suppression: ExecutionFailureTracker,
}

#[derive(Clone)]
struct LimitedProcess {
    process_id: u32,
    process_name: String,
    executable_path: String,
}

struct SelectedTarget {
    control: CpuLimiterTarget,
    process_name: String,
    executable_path: String,
}

impl CpuLimiterManager {
    #[expect(
        clippy::too_many_arguments,
        reason = "the policy boundary receives current observations and the dedicated controller"
    )]
    pub fn update(
        &mut self,
        controller: &mut CpuLimiterController,
        settings: &CpuLimiterSettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> CpuLimiterSnapshot {
        if !automation_enabled {
            return self.clear(controller, action_log, false, "Automation disabled.");
        }
        if !settings.enabled {
            return self.clear(controller, action_log, false, "CPU Limiter disabled.");
        }
        if !settings.rules.iter().any(valid_rule) {
            self.failure_suppression.clear();
            return self.clear(
                controller,
                action_log,
                true,
                "No CPU Limiter rules configured.",
            );
        }

        let needs_foreground = requires_foreground_observation(settings);
        if needs_foreground && foreground_process_id.is_none() {
            return self.paused(controller, action_log, "Paused: foreground app is unknown.");
        }

        let needs_visible_windows = requires_visible_window_observation(settings);
        let visible_window_process_ids = if needs_visible_windows {
            match observations.visible_window_process_ids() {
                Ok(process_ids) => process_ids,
                Err(_) => {
                    return self.paused(
                        controller,
                        action_log,
                        "Paused: visible windows are unavailable.",
                    );
                }
            }
        } else {
            Default::default()
        };

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            return self.paused(
                controller,
                action_log,
                "Paused: current Windows session is unknown.",
            );
        };
        let processes = match observations.processes_with_paths() {
            Ok(processes) => processes,
            Err(error) => return self.paused(controller, action_log, &error),
        };

        let scanned_processes = processes.len();
        let foreground_executable_path = foreground_process_id.and_then(|id| {
            processes
                .iter()
                .find(|process| process.id == id)
                .and_then(process_executable_path)
        });
        let visible_processes = ProtectedProcesses::capture(
            processes.as_ref(),
            false,
            None,
            visible_window_process_ids,
        );
        let processes_by_id = processes
            .iter()
            .map(|process| (process.id, process))
            .collect::<BTreeMap<_, _>>();
        let mut skipped_processes = 0;
        let mut auto_excluded_processes = BTreeSet::new();
        let mut selected = BTreeMap::new();
        let mut active_failure_keys = BTreeSet::new();
        for process in processes.iter() {
            let Some(executable_path) = process_executable_path(process) else {
                continue;
            };
            let Some(rule) = matching_rule(settings, &executable_path) else {
                continue;
            };
            let (focus, visible_window) = detected_process_tier(
                process.id,
                &executable_path,
                foreground_process_id,
                foreground_executable_path.as_deref(),
                &visible_processes,
            );
            let executable_path = executable_path.to_string_lossy().into_owned();
            let failure_key = process_failure_key(&executable_path);
            active_failure_keys.insert(failure_key);
            if process.id == 0
                || process.id == current_process_id
                || process.is_critical != Some(false)
                || !process.can_set_information
                || process.is_service_account != Some(false)
                || process.session_id.is_none_or(|session_id| session_id == 0)
                || suspension_control::is_builtin_excluded(&process.name)
                || (!allow_cross_session_process_control
                    && process.session_id != Some(current_session_id))
            {
                skipped_processes += 1;
                continue;
            }
            let Some(creation_time) = process.creation_time else {
                skipped_processes += 1;
                continue;
            };
            let Some(allowed_cpu_time_percent) =
                resolved_allowed_cpu_time_percent(settings, rule, focus, visible_window)
            else {
                continue;
            };
            if self.is_suppressed(
                &process.name,
                &executable_path,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }
            let suspension_target = SuspensionTarget::automatic(
                process.id,
                process.name.clone(),
                Path::new(&executable_path).to_path_buf(),
                creation_time,
                process.is_service_account,
            );
            selected.insert(
                suspension_target.key(),
                SelectedTarget {
                    control: CpuLimiterTarget {
                        suspension_target,
                        allowed_cpu_time_percent,
                        allow_cross_session_process_control,
                        ancestor_process_ids: verified_ancestor_process_ids(
                            process,
                            &processes_by_id,
                        ),
                    },
                    process_name: process.name.clone(),
                    executable_path,
                },
            );
        }
        self.failure_suppression.retain_keys(&active_failure_keys);

        let controls = selected
            .values()
            .map(|target| target.control.clone())
            .collect();
        if let Err(error) = controller.replace_targets(controls) {
            self.limited.clear();
            return CpuLimiterSnapshot {
                enabled: true,
                scanned_processes,
                tracked_processes: selected.len(),
                skipped_processes,
                message: "CPU Limiter stopped because its timing worker failed.".to_owned(),
                last_error: Some(error),
                ..Default::default()
            };
        }

        let mut failed_keys = BTreeSet::new();
        let mut last_error = None;
        let mut failed_processes = 0;
        match controller.drain_failures() {
            Ok(failures) => {
                for failure in failures {
                    let key = failure.suspension_target.key();
                    let executable_path = failure
                        .suspension_target
                        .process
                        .executable_path
                        .to_string_lossy()
                        .into_owned();
                    let process_name = failure.suspension_target.process.name.clone();
                    if failure.error.is_access_denied() {
                        self.failure_suppression
                            .suppress_process_failure(&executable_path);
                    } else if failure.error.should_report() {
                        self.failure_suppression
                            .record_process_failure(&executable_path);
                    }
                    if failure.error.should_report() {
                        failed_processes += 1;
                        last_error = Some(failure.error.to_string());
                        action_log.record(
                            ActionLogFeature::CpuLimiter,
                            Some(failure.suspension_target.process.id),
                            process_name,
                            ActionLogResult::Failed,
                            failure.error.to_string(),
                        );
                    } else {
                        skipped_processes += 1;
                    }
                    failed_keys.insert(key);
                }
            }
            Err(error) => {
                self.limited.clear();
                return CpuLimiterSnapshot {
                    enabled: true,
                    scanned_processes,
                    tracked_processes: selected.len(),
                    skipped_processes,
                    message: "CPU Limiter stopped because its timing worker failed.".to_owned(),
                    last_error: Some(error),
                    ..Default::default()
                };
            }
        }

        let active_keys = selected
            .keys()
            .filter(|key| !failed_keys.contains(*key))
            .cloned()
            .collect::<BTreeSet<_>>();
        let managed_keys = controller
            .managed_target_keys()
            .unwrap_or_else(|_| self.limited.keys().cloned().collect());
        let fallback_keys = controller.fallback_target_keys().unwrap_or_default();
        let previous = std::mem::take(&mut self.limited);
        let previous_keys = previous.keys().cloned().collect::<BTreeSet<_>>();
        self.limited = retain_unreleased_limits(previous, &active_keys, &managed_keys, action_log);
        for key in &active_keys {
            let target = &selected[key];
            self.limited.insert(
                key.clone(),
                LimitedProcess {
                    process_id: target.control.suspension_target.process.id,
                    process_name: target.process_name.clone(),
                    executable_path: target.executable_path.clone(),
                },
            );
            if !previous_keys.contains(key) {
                action_log.record(
                    ActionLogFeature::CpuLimiter,
                    Some(target.control.suspension_target.process.id),
                    target.process_name.clone(),
                    ActionLogResult::Applied,
                    limiter_activation_detail(
                        target.control.allowed_cpu_time_percent,
                        fallback_keys.contains(key),
                    ),
                );
            }
        }

        CpuLimiterSnapshot {
            enabled: true,
            scanned_processes,
            limited_processes: active_keys.len(),
            tracked_processes: selected.len(),
            skipped_processes,
            failed_processes,
            limited_apps: self
                .limited
                .values()
                .map(|process| process.executable_path.clone())
                .collect(),
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            message: "CPU Limiter active.".to_owned(),
            last_error,
        }
    }

    fn paused(
        &mut self,
        controller: &mut CpuLimiterController,
        action_log: &mut ActionLog,
        message: &str,
    ) -> CpuLimiterSnapshot {
        self.clear(controller, action_log, true, message)
    }

    fn clear(
        &mut self,
        controller: &mut CpuLimiterController,
        action_log: &mut ActionLog,
        enabled: bool,
        message: &str,
    ) -> CpuLimiterSnapshot {
        let mut last_error = controller.replace_targets(Vec::new()).err();
        if let Ok(failures) = controller.drain_failures() {
            for failure in failures {
                let error = failure.error.to_string();
                last_error = Some(error.clone());
                action_log.record(
                    ActionLogFeature::CpuLimiter,
                    Some(failure.suspension_target.process.id),
                    failure.suspension_target.process.name,
                    ActionLogResult::Failed,
                    format!("Release retry pending: {error}"),
                );
            }
        }
        let managed_keys = controller
            .managed_target_keys()
            .unwrap_or_else(|_| self.limited.keys().cloned().collect());
        self.limited = retain_unreleased_limits(
            std::mem::take(&mut self.limited),
            &BTreeSet::new(),
            &managed_keys,
            action_log,
        );
        CpuLimiterSnapshot {
            enabled,
            message: message.to_owned(),
            last_error,
            ..Default::default()
        }
    }

    fn is_suppressed(
        &mut self,
        process_name: &str,
        executable_path: &str,
        action_log: &mut ActionLog,
        auto_excluded_processes: &mut BTreeSet<String>,
    ) -> bool {
        let suppression = self
            .failure_suppression
            .process_suppression(executable_path);
        if !suppression.suppressed {
            return false;
        }
        auto_excluded_processes.insert(executable_path.to_owned());
        if suppression.newly_suppressed {
            action_log.record(
                ActionLogFeature::CpuLimiter,
                None,
                process_name.to_owned(),
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying CPU Limiter after {} failed attempts.",
                    execution_failure_suppression_threshold()
                ),
            );
        }
        true
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    suspension_control::is_builtin_excluded(process_name)
}

fn limiter_activation_detail(allowed_cpu_time_percent: u8, used_thread_fallback: bool) -> String {
    let mut detail = format!(
        "Set the managed app group's Allowed CPU Time target to {allowed_cpu_time_percent}%."
    );
    if used_thread_fallback {
        detail.push_str(
            " Automatic thread fallback was used because Windows kept the app in another Job Object.",
        );
    }
    detail
}

fn valid_rule(rule: &CpuLimiterRule) -> bool {
    rule.enabled
        && Path::new(rule.executable_path.trim()).is_absolute()
        && rule.has_valid_allowed_cpu_time()
}

fn matching_rule<'a>(
    settings: &'a CpuLimiterSettings,
    executable_path: &Path,
) -> Option<&'a CpuLimiterRule> {
    settings.rules.iter().find(|rule| {
        valid_rule(rule)
            && same_executable_path(Path::new(rule.executable_path.trim()), executable_path)
    })
}

fn detected_process_tier(
    process_id: u32,
    executable_path: &Path,
    foreground_process_id: Option<u32>,
    foreground_executable_path: Option<&Path>,
    visible_processes: &ProtectedProcesses,
) -> (bool, bool) {
    let focus = is_foreground_process(
        process_id,
        executable_path,
        foreground_process_id,
        foreground_executable_path,
    );
    (
        focus,
        !focus && visible_processes.contains(process_id, executable_path),
    )
}

fn resolved_allowed_cpu_time_percent(
    settings: &CpuLimiterSettings,
    rule: &CpuLimiterRule,
    focus: bool,
    visible_window: bool,
) -> Option<u8> {
    let (mode, custom, page_default) = if focus {
        (
            rule.focus_mode,
            rule.focus_allowed_cpu_time_percent,
            Some(settings.focus_allowed_cpu_time_percent),
        )
    } else if visible_window {
        (
            rule.visible_window_mode,
            rule.visible_window_allowed_cpu_time_percent,
            Some(settings.visible_window_allowed_cpu_time_percent),
        )
    } else {
        (
            rule.background_mode,
            rule.background_allowed_cpu_time_percent,
            Some(settings.background_allowed_cpu_time_percent),
        )
    };
    match mode {
        ProcessRuleMode::Default => page_default,
        ProcessRuleMode::Enabled => Some(custom),
        ProcessRuleMode::Disabled => None,
    }
    // A 100% target is Unlimited, so it does not create a limiter schedule.
    .filter(|percent| (1..=99).contains(percent))
}

fn requires_foreground_observation(settings: &CpuLimiterSettings) -> bool {
    settings
        .rules
        .iter()
        .filter(|rule| valid_rule(rule))
        .any(|rule| {
            let focus = resolved_allowed_cpu_time_percent(settings, rule, true, false);
            focus != resolved_allowed_cpu_time_percent(settings, rule, false, true)
        })
}

pub(crate) fn rule_has_finite_limit(settings: &CpuLimiterSettings, rule: &CpuLimiterRule) -> bool {
    valid_rule(rule)
        && [(true, false), (false, true), (false, false)]
            .into_iter()
            .any(|(focus, visible_window)| {
                resolved_allowed_cpu_time_percent(settings, rule, focus, visible_window).is_some()
            })
}

fn requires_visible_window_observation(settings: &CpuLimiterSettings) -> bool {
    settings
        .rules
        .iter()
        .filter(|rule| valid_rule(rule))
        .any(|rule| {
            resolved_allowed_cpu_time_percent(settings, rule, false, true)
                != resolved_allowed_cpu_time_percent(settings, rule, false, false)
        })
}

fn retain_unreleased_limits(
    previous: BTreeMap<ProcessTargetKey, LimitedProcess>,
    active_keys: &BTreeSet<ProcessTargetKey>,
    managed_keys: &BTreeSet<ProcessTargetKey>,
    action_log: &mut ActionLog,
) -> BTreeMap<ProcessTargetKey, LimitedProcess> {
    let mut pending = BTreeMap::new();
    for (key, limited) in previous {
        if active_keys.contains(&key) {
            continue;
        }
        if managed_keys.contains(&key) {
            pending.insert(key, limited);
            continue;
        }
        action_log.record(
            ActionLogFeature::CpuLimiter,
            Some(limited.process_id),
            limited.process_name,
            ActionLogResult::Restored,
            "Released the CPU Limiter duty cycle.".to_owned(),
        );
    }
    pending
}

fn verified_ancestor_process_ids(
    process: &ProcessInfo,
    processes_by_id: &BTreeMap<u32, &ProcessInfo>,
) -> Vec<u32> {
    let mut ancestors = Vec::new();
    let mut seen = BTreeSet::from([process.id]);
    let mut child = process;
    while let Some(parent_id) = child.parent_id {
        if !seen.insert(parent_id) {
            break;
        }
        let Some(parent) = processes_by_id.get(&parent_id).copied() else {
            break;
        };
        let (Some(child_created), Some(parent_created)) =
            (child.creation_time, parent.creation_time)
        else {
            break;
        };
        if child_created < parent_created {
            break;
        }
        ancestors.push(parent_id);
        child = parent;
    }
    ancestors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProcessRuleMode;
    use crate::foreground::ProcessInfo;

    fn limited_process(process_id: u32) -> (ProcessTargetKey, LimitedProcess) {
        let executable_path = Path::new(r"C:\Apps\encoder.exe").to_path_buf();
        let target = SuspensionTarget::automatic(
            process_id,
            "encoder.exe".to_owned(),
            executable_path.clone(),
            u64::from(process_id),
            Some(false),
        );
        (
            target.key(),
            LimitedProcess {
                process_id,
                process_name: "encoder.exe".to_owned(),
                executable_path: executable_path.to_string_lossy().into_owned(),
            },
        )
    }

    fn rule() -> CpuLimiterRule {
        CpuLimiterRule {
            enabled: true,
            executable_path: r"C:\Apps\encoder.exe".to_owned(),
            focus_mode: ProcessRuleMode::Default,
            visible_window_mode: ProcessRuleMode::Default,
            background_mode: ProcessRuleMode::Default,
            focus_allowed_cpu_time_percent: 50,
            visible_window_allowed_cpu_time_percent: 50,
            background_allowed_cpu_time_percent: 50,
        }
    }

    #[test]
    fn detected_tier_selects_its_allowed_cpu_time() {
        let settings = CpuLimiterSettings {
            enabled: true,
            focus_allowed_cpu_time_percent: 100,
            visible_window_allowed_cpu_time_percent: 60,
            background_allowed_cpu_time_percent: 25,
            rules: Vec::new(),
        };
        let mut rule = rule();
        rule.focus_mode = ProcessRuleMode::Enabled;
        rule.visible_window_mode = ProcessRuleMode::Enabled;
        rule.background_mode = ProcessRuleMode::Enabled;
        rule.focus_allowed_cpu_time_percent = 75;
        rule.visible_window_allowed_cpu_time_percent = 50;
        rule.background_allowed_cpu_time_percent = 25;

        assert_eq!(
            resolved_allowed_cpu_time_percent(&settings, &rule, true, false),
            Some(75)
        );
        assert_eq!(
            resolved_allowed_cpu_time_percent(&settings, &rule, false, true),
            Some(50)
        );
        assert_eq!(
            resolved_allowed_cpu_time_percent(&settings, &rule, false, false),
            Some(25)
        );
    }

    #[test]
    fn detected_tier_groups_processes_from_the_same_app() {
        let app_path = Path::new(r"C:\Apps\Browser\browser.exe");
        let visible_process = ProcessInfo {
            id: 7,
            creation_time: Some(7),
            parent_id: None,
            session_id: Some(1),
            user_name: None,
            is_service_account: Some(false),
            is_critical: Some(false),
            can_set_information: true,
            name: "browser.exe".to_owned(),
            image_path: Some(app_path.to_path_buf()),
        };
        let visible_processes = crate::foreground::ProtectedProcesses::capture(
            &[visible_process],
            false,
            None,
            BTreeSet::from([7]),
        );

        assert_eq!(
            detected_process_tier(8, app_path, Some(7), Some(app_path), &visible_processes),
            (true, false)
        );
        assert_eq!(
            detected_process_tier(8, app_path, Some(9), None, &visible_processes),
            (false, true)
        );
    }

    #[test]
    fn tier_specific_percentages_require_tier_observations() {
        let mut rule = rule();
        rule.focus_mode = ProcessRuleMode::Enabled;
        rule.visible_window_mode = ProcessRuleMode::Enabled;
        rule.background_mode = ProcessRuleMode::Enabled;
        rule.focus_allowed_cpu_time_percent = 75;
        rule.visible_window_allowed_cpu_time_percent = 50;
        rule.background_allowed_cpu_time_percent = 25;
        let settings = CpuLimiterSettings {
            enabled: true,
            focus_allowed_cpu_time_percent: 100,
            visible_window_allowed_cpu_time_percent: 60,
            background_allowed_cpu_time_percent: 25,
            rules: vec![rule],
        };

        assert!(requires_foreground_observation(&settings));
        assert!(requires_visible_window_observation(&settings));
    }

    #[test]
    fn matching_focus_and_visible_tiers_do_not_require_foreground_observation() {
        let settings = CpuLimiterSettings {
            enabled: true,
            focus_allowed_cpu_time_percent: 60,
            visible_window_allowed_cpu_time_percent: 60,
            background_allowed_cpu_time_percent: 25,
            rules: vec![rule()],
        };

        assert!(!requires_foreground_observation(&settings));
        assert!(requires_visible_window_observation(&settings));
    }

    #[test]
    fn default_rule_uses_all_page_percentages_and_100_is_unlimited() {
        let settings = CpuLimiterSettings {
            enabled: true,
            focus_allowed_cpu_time_percent: 75,
            visible_window_allowed_cpu_time_percent: 100,
            background_allowed_cpu_time_percent: 25,
            rules: Vec::new(),
        };
        let rule = rule();

        assert_eq!(
            resolved_allowed_cpu_time_percent(&settings, &rule, true, false),
            Some(75)
        );
        assert_eq!(
            resolved_allowed_cpu_time_percent(&settings, &rule, false, true),
            None
        );
        assert_eq!(
            resolved_allowed_cpu_time_percent(&settings, &rule, false, false),
            Some(25)
        );
    }

    #[test]
    fn custom_and_unlimited_modes_override_page_percentages() {
        let settings = CpuLimiterSettings {
            enabled: true,
            focus_allowed_cpu_time_percent: 100,
            visible_window_allowed_cpu_time_percent: 60,
            background_allowed_cpu_time_percent: 25,
            rules: Vec::new(),
        };
        let mut rule = rule();

        rule.focus_mode = ProcessRuleMode::Enabled;
        rule.visible_window_mode = ProcessRuleMode::Disabled;
        rule.focus_allowed_cpu_time_percent = 100;

        assert_eq!(
            resolved_allowed_cpu_time_percent(&settings, &rule, true, false),
            None
        );
        assert_eq!(
            resolved_allowed_cpu_time_percent(&settings, &rule, false, true),
            None
        );
        assert_eq!(
            resolved_allowed_cpu_time_percent(&settings, &rule, false, false),
            Some(25)
        );
    }

    #[test]
    fn invalid_page_percentages_do_not_reach_the_limiter() {
        let settings = CpuLimiterSettings {
            enabled: true,
            focus_allowed_cpu_time_percent: 100,
            visible_window_allowed_cpu_time_percent: 0,
            background_allowed_cpu_time_percent: 101,
            rules: Vec::new(),
        };
        let rule = rule();

        assert_eq!(
            resolved_allowed_cpu_time_percent(&settings, &rule, false, true),
            None
        );
        assert_eq!(
            resolved_allowed_cpu_time_percent(&settings, &rule, false, false),
            None
        );
    }

    #[test]
    fn runnable_rule_percentages_are_limited_to_one_through_one_hundred() {
        let mut rule = rule();
        assert!(valid_rule(&rule));
        rule.focus_allowed_cpu_time_percent = 0;
        assert!(!valid_rule(&rule));
        rule.focus_allowed_cpu_time_percent = 1;
        rule.visible_window_allowed_cpu_time_percent = 100;
        assert!(valid_rule(&rule));
        rule.visible_window_allowed_cpu_time_percent = 101;
        assert!(!valid_rule(&rule));
        rule.visible_window_allowed_cpu_time_percent = 50;
        rule.background_allowed_cpu_time_percent = 0;
        assert!(!valid_rule(&rule));
    }

    #[test]
    fn pending_release_is_not_reported_as_restored() {
        let (key, process) = limited_process(42);
        let previous = BTreeMap::from([(key.clone(), process)]);
        let managed_keys = BTreeSet::from([key.clone()]);
        let mut action_log = ActionLog::default();

        let pending =
            retain_unreleased_limits(previous, &BTreeSet::new(), &managed_keys, &mut action_log);

        assert!(pending.contains_key(&key));
        assert!(action_log.entries().is_empty());
    }

    #[test]
    fn child_target_records_its_verified_ancestor_chain() {
        let process = |id, parent_id| ProcessInfo {
            id,
            creation_time: Some(u64::from(id)),
            parent_id,
            session_id: Some(1),
            user_name: None,
            is_service_account: Some(false),
            is_critical: Some(false),
            can_set_information: true,
            name: format!("process-{id}.exe"),
            image_path: Some(format!(r"C:\Apps\process-{id}.exe").into()),
        };
        let processes = [process(1, None), process(2, Some(1)), process(3, Some(2))];
        let processes_by_id = processes
            .iter()
            .map(|process| (process.id, process))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            verified_ancestor_process_ids(&processes[2], &processes_by_id),
            vec![2, 1]
        );
    }

    #[test]
    fn activation_detail_identifies_automatic_thread_fallback_once() {
        assert_eq!(
            limiter_activation_detail(1, false),
            "Set the managed app group's Allowed CPU Time target to 1%."
        );
        assert_eq!(
            limiter_activation_detail(1, true),
            "Set the managed app group's Allowed CPU Time target to 1%. Automatic thread fallback was used because Windows kept the app in another Job Object."
        );
    }
}
