use std::time::Instant;

use crate::{
    cpu::ProcessCpuSample,
    foreground::{contains_process_name, EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS},
    platform::windows::memory_trim,
    win_util::WinHandle,
};

use super::process::{
    open_process_for_working_set_trim, ProcessControlError, ProcessControlTarget,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MemoryTrimProcessSample {
    pub(crate) working_set_bytes: u64,
    pub(crate) cpu: Option<ProcessCpuSample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MemoryTrimOutcome {
    pub(crate) freed_bytes: Option<u64>,
}

pub(crate) trait MemoryTrimPlatform {
    type Process;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<Self::Process, ProcessControlError>;
    fn working_set_bytes(&mut self, process: &Self::Process) -> Result<u64, ProcessControlError>;
    fn cpu_sample(
        &mut self,
        process: &Self::Process,
    ) -> Result<ProcessCpuSample, ProcessControlError>;
    fn trim(&mut self, process: &Self::Process) -> Result<(), ProcessControlError>;
}

#[derive(Default)]
pub(crate) struct WindowsMemoryTrimPlatform;

impl MemoryTrimPlatform for WindowsMemoryTrimPlatform {
    type Process = WinHandle;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<Self::Process, ProcessControlError> {
        if contains_process_name(EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS, &target.name) {
            return Err(ProcessControlError::AccessDenied(
                "Built-in Windows processes cannot be modified.".to_owned(),
            ));
        }
        open_process_for_working_set_trim(target, allow_cross_session_process_control)
            .map(|(_, process)| process)
    }

    fn working_set_bytes(&mut self, process: &Self::Process) -> Result<u64, ProcessControlError> {
        memory_trim::working_set_bytes(process).map_err(ProcessControlError::from)
    }

    fn cpu_sample(
        &mut self,
        process: &Self::Process,
    ) -> Result<ProcessCpuSample, ProcessControlError> {
        memory_trim::cpu_time_100ns(process)
            .map(|cpu_time_100ns| ProcessCpuSample {
                cpu_time_100ns,
                sampled_at: Instant::now(),
            })
            .map_err(ProcessControlError::from)
    }

    fn trim(&mut self, process: &Self::Process) -> Result<(), ProcessControlError> {
        memory_trim::trim(process).map_err(ProcessControlError::from)
    }
}

pub(crate) struct MemoryTrimController<P: MemoryTrimPlatform = WindowsMemoryTrimPlatform> {
    platform: P,
}

impl Default for MemoryTrimController {
    fn default() -> Self {
        Self::with_platform(WindowsMemoryTrimPlatform)
    }
}

impl<P: MemoryTrimPlatform> MemoryTrimController<P> {
    fn with_platform(platform: P) -> Self {
        Self { platform }
    }

    pub(crate) fn sample(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
        include_cpu: bool,
    ) -> Result<MemoryTrimProcessSample, ProcessControlError> {
        let process = self
            .platform
            .open(target, allow_cross_session_process_control)?;
        let working_set_bytes = self.platform.working_set_bytes(&process)?;
        let cpu = include_cpu
            .then(|| self.platform.cpu_sample(&process))
            .transpose()?;
        Ok(MemoryTrimProcessSample {
            working_set_bytes,
            cpu,
        })
    }

    pub(crate) fn trim(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<MemoryTrimOutcome, ProcessControlError> {
        let process = self
            .platform
            .open(target, allow_cross_session_process_control)?;
        let before = self.platform.working_set_bytes(&process)?;
        self.platform.trim(&process)?;
        let freed_bytes = self
            .platform
            .working_set_bytes(&process)
            .ok()
            .map(|after| before.saturating_sub(after));
        Ok(MemoryTrimOutcome { freed_bytes })
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, path::PathBuf};

    use super::*;

    #[derive(Default)]
    struct FakePlatform {
        opens: Vec<(ProcessControlTarget, bool)>,
        working_sets: VecDeque<Result<u64, ProcessControlError>>,
        cpu_samples: VecDeque<Result<ProcessCpuSample, ProcessControlError>>,
        trim_results: VecDeque<Result<(), ProcessControlError>>,
        trim_calls: usize,
    }

    impl MemoryTrimPlatform for FakePlatform {
        type Process = ();

        fn open(
            &mut self,
            target: &ProcessControlTarget,
            allow_cross_session_process_control: bool,
        ) -> Result<Self::Process, ProcessControlError> {
            self.opens
                .push((target.clone(), allow_cross_session_process_control));
            Ok(())
        }

        fn working_set_bytes(
            &mut self,
            _process: &Self::Process,
        ) -> Result<u64, ProcessControlError> {
            self.working_sets
                .pop_front()
                .unwrap_or_else(|| Err(ProcessControlError::Failed("No sample.".to_owned())))
        }

        fn cpu_sample(
            &mut self,
            _process: &Self::Process,
        ) -> Result<ProcessCpuSample, ProcessControlError> {
            self.cpu_samples
                .pop_front()
                .unwrap_or_else(|| Err(ProcessControlError::Failed("No CPU sample.".to_owned())))
        }

        fn trim(&mut self, _process: &Self::Process) -> Result<(), ProcessControlError> {
            self.trim_calls += 1;
            self.trim_results.pop_front().unwrap_or(Ok(()))
        }
    }

    fn target() -> ProcessControlTarget {
        ProcessControlTarget::automatic(
            42,
            "app.exe".to_owned(),
            PathBuf::from(r"C:\Apps\app.exe"),
            7,
        )
    }

    #[test]
    fn sample_preserves_exact_target_and_cross_session_policy() {
        let cpu = ProcessCpuSample {
            cpu_time_100ns: 11,
            sampled_at: Instant::now(),
        };
        let platform = FakePlatform {
            working_sets: VecDeque::from([Ok(64)]),
            cpu_samples: VecDeque::from([Ok(cpu)]),
            ..Default::default()
        };
        let mut controller = MemoryTrimController::with_platform(platform);

        let sample = controller.sample(&target(), false, true).unwrap();

        assert_eq!(sample.working_set_bytes, 64);
        assert_eq!(sample.cpu, Some(cpu));
        assert_eq!(controller.platform.opens, vec![(target(), false)]);
    }

    #[test]
    fn successful_trim_keeps_an_unavailable_freed_estimate_unknown() {
        let platform = FakePlatform {
            working_sets: VecDeque::from([
                Ok(128),
                Err(ProcessControlError::Failed("sample failed".to_owned())),
            ]),
            trim_results: VecDeque::from([Ok(())]),
            ..Default::default()
        };
        let mut controller = MemoryTrimController::with_platform(platform);

        let outcome = controller.trim(&target(), true).unwrap();

        assert_eq!(outcome.freed_bytes, None);
        assert_eq!(controller.platform.trim_calls, 1);
    }

    #[test]
    fn failed_trim_is_reported_without_a_post_action_sample() {
        let platform = FakePlatform {
            working_sets: VecDeque::from([Ok(128), Ok(0)]),
            trim_results: VecDeque::from([Err(ProcessControlError::Failed(
                "trim failed".to_owned(),
            ))]),
            ..Default::default()
        };
        let mut controller = MemoryTrimController::with_platform(platform);

        let error = controller.trim(&target(), true).unwrap_err();

        assert_eq!(error.to_string(), "trim failed");
        assert_eq!(controller.platform.working_sets.len(), 1);
    }
}
