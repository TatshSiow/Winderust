use crate::{
    foreground::ProcessActionTarget, platform::windows::process_termination, win_util::WinHandle,
};

use super::process::{open_process_for_termination, ProcessControlError, ProcessControlTarget};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProcessTerminationOutcome {
    Terminated,
    Failed(String),
    NotAttempted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessTerminationTargetResult {
    pub(crate) target: ProcessControlTarget,
    pub(crate) outcome: ProcessTerminationOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessTerminationBatchResult {
    pub(crate) results: Vec<ProcessTerminationTargetResult>,
    pub(crate) preflight_failed: bool,
}

impl ProcessTerminationBatchResult {
    pub(crate) fn into_process_list_result(self) -> Result<(), String> {
        if self.results.is_empty() {
            return Err("No process targets were available.".to_owned());
        }
        let total = self.results.len();
        let terminated = self
            .results
            .iter()
            .filter(|result| matches!(result.outcome, ProcessTerminationOutcome::Terminated))
            .count();
        let first_error =
            self.results
                .iter()
                .find_map(|result| match &result.outcome {
                    ProcessTerminationOutcome::Failed(error) => Some(error.as_str()),
                    ProcessTerminationOutcome::Terminated
                    | ProcessTerminationOutcome::NotAttempted => None,
                });
        if self.preflight_failed {
            return Err(format!(
                "Process termination preflight failed; no processes were stopped: {}",
                first_error.unwrap_or("A process target could not be validated.")
            ));
        }
        if terminated == total {
            Ok(())
        } else {
            Err(format!(
                "Stopped {terminated} of {total} processes; {} failed: {}",
                total.saturating_sub(terminated),
                first_error.unwrap_or("The process could not be stopped.")
            ))
        }
    }
}

pub(crate) trait ProcessTerminationPlatform {
    type Process;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<Self::Process, ProcessControlError>;
    fn terminate(&mut self, process: &Self::Process) -> Result<(), ProcessControlError>;
}

#[derive(Default)]
pub(crate) struct WindowsProcessTerminationPlatform;

impl ProcessTerminationPlatform for WindowsProcessTerminationPlatform {
    type Process = WinHandle;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<Self::Process, ProcessControlError> {
        open_process_for_termination(target, allow_cross_session_process_control)
            .map(|(_, process)| process)
    }

    fn terminate(&mut self, process: &Self::Process) -> Result<(), ProcessControlError> {
        process_termination::terminate(process).map_err(ProcessControlError::from)
    }
}

pub(crate) struct ProcessTerminationController<
    P: ProcessTerminationPlatform = WindowsProcessTerminationPlatform,
> {
    platform: P,
}

impl Default for ProcessTerminationController {
    fn default() -> Self {
        Self::with_platform(WindowsProcessTerminationPlatform)
    }
}

impl<P: ProcessTerminationPlatform> ProcessTerminationController<P> {
    fn with_platform(platform: P) -> Self {
        Self { platform }
    }

    pub(crate) fn terminate_batch(
        &mut self,
        targets: Vec<ProcessActionTarget>,
        allow_cross_session_process_control: bool,
    ) -> ProcessTerminationBatchResult {
        let targets = targets
            .iter()
            .map(ProcessControlTarget::from_action_target)
            .collect::<Vec<_>>();
        let mut processes = Vec::with_capacity(targets.len());
        let mut preflight_errors = Vec::with_capacity(targets.len());
        for target in &targets {
            match self
                .platform
                .open(target, allow_cross_session_process_control)
            {
                Ok(process) => {
                    processes.push(Some(process));
                    preflight_errors.push(None);
                }
                Err(error) => {
                    processes.push(None);
                    preflight_errors.push(Some(error.to_string()));
                }
            }
        }

        let preflight_failed = preflight_errors.iter().any(Option::is_some);
        if preflight_failed {
            let results = targets
                .into_iter()
                .zip(preflight_errors)
                .map(|(target, error)| ProcessTerminationTargetResult {
                    target,
                    outcome: error.map_or(
                        ProcessTerminationOutcome::NotAttempted,
                        ProcessTerminationOutcome::Failed,
                    ),
                })
                .collect();
            return ProcessTerminationBatchResult {
                results,
                preflight_failed: true,
            };
        }

        let results = targets
            .into_iter()
            .zip(processes)
            .map(|(target, process)| {
                let outcome = match process {
                    Some(process) => match self.platform.terminate(&process) {
                        Ok(()) => ProcessTerminationOutcome::Terminated,
                        Err(error) => ProcessTerminationOutcome::Failed(error.to_string()),
                    },
                    None => ProcessTerminationOutcome::Failed(
                        "The process target was not retained after preflight.".to_owned(),
                    ),
                };
                ProcessTerminationTargetResult { target, outcome }
            })
            .collect();
        ProcessTerminationBatchResult {
            results,
            preflight_failed: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use super::*;

    #[derive(Default)]
    struct FakePlatform {
        open_failures: BTreeMap<u32, String>,
        termination_failures: BTreeMap<u32, String>,
        opens: Vec<(ProcessControlTarget, bool)>,
        terminations: Vec<u32>,
    }

    impl ProcessTerminationPlatform for FakePlatform {
        type Process = u32;

        fn open(
            &mut self,
            target: &ProcessControlTarget,
            allow_cross_session_process_control: bool,
        ) -> Result<Self::Process, ProcessControlError> {
            self.opens
                .push((target.clone(), allow_cross_session_process_control));
            match self.open_failures.get(&target.id) {
                Some(error) => Err(ProcessControlError::AccessDenied(error.clone())),
                None => Ok(target.id),
            }
        }

        fn terminate(&mut self, process: &Self::Process) -> Result<(), ProcessControlError> {
            self.terminations.push(*process);
            match self.termination_failures.get(process) {
                Some(error) => Err(ProcessControlError::Failed(error.clone())),
                None => Ok(()),
            }
        }
    }

    fn target(id: u32) -> ProcessActionTarget {
        ProcessActionTarget {
            id,
            name: format!("app{id}.exe"),
            executable_path: PathBuf::from(format!(r"C:\Apps\app{id}.exe")),
            creation_time: u64::from(id),
            session_id: Some(1),
            is_service_account: Some(false),
        }
    }

    #[test]
    fn preflight_failure_opens_every_target_and_terminates_none() {
        let platform = FakePlatform {
            open_failures: BTreeMap::from([(2, "denied".to_owned())]),
            ..Default::default()
        };
        let mut controller = ProcessTerminationController::with_platform(platform);

        let result = controller.terminate_batch(vec![target(1), target(2), target(3)], false);

        assert!(result.preflight_failed);
        assert!(matches!(
            result.results[0].outcome,
            ProcessTerminationOutcome::NotAttempted
        ));
        assert!(matches!(
            result.results[1].outcome,
            ProcessTerminationOutcome::Failed(_)
        ));
        assert!(controller.platform.terminations.is_empty());
        assert_eq!(controller.platform.opens.len(), 3);
        assert!(controller.platform.opens.iter().all(|(_, allow)| !allow));
    }

    #[test]
    fn execution_failure_does_not_stop_later_targets() {
        let platform = FakePlatform {
            termination_failures: BTreeMap::from([(2, "failed".to_owned())]),
            ..Default::default()
        };
        let mut controller = ProcessTerminationController::with_platform(platform);

        let result = controller.terminate_batch(vec![target(3), target(2), target(1)], true);

        assert!(!result.preflight_failed);
        assert_eq!(controller.platform.terminations, vec![3, 2, 1]);
        assert_eq!(result.results[0].target.id, 3);
        assert_eq!(result.results[1].target.id, 2);
        assert_eq!(result.results[2].target.id, 1);
        assert!(matches!(
            result.results[2].outcome,
            ProcessTerminationOutcome::Terminated
        ));
        assert_eq!(
            result.into_process_list_result().unwrap_err(),
            "Stopped 2 of 3 processes; 1 failed: failed"
        );
    }

    #[test]
    fn empty_batch_is_reported_without_opening_or_terminating() {
        let mut controller = ProcessTerminationController::with_platform(FakePlatform::default());

        let result = controller.terminate_batch(Vec::new(), true);

        assert_eq!(
            result.into_process_list_result().unwrap_err(),
            "No process targets were available."
        );
        assert!(controller.platform.opens.is_empty());
        assert!(controller.platform.terminations.is_empty());
    }
}
