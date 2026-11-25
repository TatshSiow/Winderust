use std::time::{Duration, Instant};

use crate::{
    foreground::{same_executable_path, ProcessActionTarget},
    platform::windows::{process_restart, process_termination},
    win_util::WinHandle,
};

use super::process::{open_process_for_restart, ProcessControlTarget};

pub(crate) struct RestartResult {
    pub(crate) termination_handled: Vec<(u32, u64)>,
    pub(crate) result: Result<(), String>,
}

trait RestartPlatform {
    type Process;
    type Launch;

    fn open(
        &mut self,
        target: &ProcessActionTarget,
        allow_cross_session: bool,
    ) -> Result<Self::Process, String>;
    fn prepare(
        &mut self,
        processes: &[Self::Process],
        target: &ProcessActionTarget,
    ) -> Result<Self::Launch, String>;
    fn terminate(&mut self, process: &Self::Process) -> Result<(), String>;
    fn wait(&mut self, process: &Self::Process, milliseconds: u32) -> Result<(), String>;
    fn launch(&mut self, launch: Self::Launch) -> Result<(), String>;
}

struct WindowsRestartPlatform;

impl RestartPlatform for WindowsRestartPlatform {
    type Process = WinHandle;
    type Launch = process_restart::RestartLaunch;

    fn open(
        &mut self,
        target: &ProcessActionTarget,
        allow_cross_session: bool,
    ) -> Result<WinHandle, String> {
        open_process_for_restart(
            &ProcessControlTarget::from_action_target(target),
            allow_cross_session,
        )
        .map(|(_, process)| process)
        .map_err(|error| error.to_string())
    }
    fn prepare(
        &mut self,
        processes: &[WinHandle],
        target: &ProcessActionTarget,
    ) -> Result<Self::Launch, String> {
        process_restart::prepare(processes, &target.executable_path)
    }
    fn terminate(&mut self, process: &WinHandle) -> Result<(), String> {
        process_termination::terminate(process)
            .map_err(|error| super::process::ProcessControlError::from(error).to_string())
    }
    fn wait(&mut self, process: &WinHandle, milliseconds: u32) -> Result<(), String> {
        process_restart::wait(process, milliseconds)
    }
    fn launch(&mut self, launch: Self::Launch) -> Result<(), String> {
        process_restart::launch(launch)
    }
}

pub(crate) fn restart_app(
    targets: Vec<ProcessActionTarget>,
    allow_cross_session: bool,
    before_prepare: impl FnOnce(&ProcessActionTarget) -> Result<(), String>,
) -> RestartResult {
    restart_with_platform(
        &mut WindowsRestartPlatform,
        targets,
        allow_cross_session,
        before_prepare,
    )
}

fn restart_with_platform<P: RestartPlatform>(
    platform: &mut P,
    mut targets: Vec<ProcessActionTarget>,
    allow_cross_session: bool,
    before_prepare: impl FnOnce(&ProcessActionTarget) -> Result<(), String>,
) -> RestartResult {
    let mut termination_handled = Vec::new();
    let mut confirmed = 0;
    let result = (|| {
        // The oldest instance normally owns the app's launch token, rather than a sandboxed helper.
        targets.sort_by_key(|target| (target.creation_time, target.id));
        let first = targets
            .first()
            .ok_or("No process targets were available.")?;
        if targets
            .iter()
            .any(|target| !same_executable_path(&first.executable_path, &target.executable_path))
        {
            return Err("Restart requires one exact executable path.".to_owned());
        }
        let processes = targets
            .iter()
            .map(|target| platform.open(target, allow_cross_session))
            .collect::<Result<Vec<_>, _>>()?;
        before_prepare(first)?;
        let launch = platform.prepare(&processes, first)?;
        let mut failure = None;
        let mut pending_exits = Vec::new();
        for (target, process) in targets.iter().zip(&processes) {
            let key = (target.id, target.creation_time);
            // An app helper can exit after preflight, including when its parent stops.
            match platform.terminate(process) {
                Ok(()) => {
                    termination_handled.push(key);
                    pending_exits.push(process);
                }
                Err(_) if platform.wait(process, 0).is_ok() => {
                    termination_handled.push(key);
                    confirmed += 1;
                }
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        for process in pending_exits {
            let remaining = if failure.is_some() {
                0
            } else {
                deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis() as u32
            };
            match platform.wait(process, remaining) {
                Ok(()) => confirmed += 1,
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        platform.launch(launch)
    })()
    .map_err(|error| {
        format!(
            "Confirmed exit for {confirmed} of {} original processes. The app was not relaunched. {error}",
            targets.len()
        )
    });
    RestartResult {
        termination_handled,
        result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakePlatform {
        fail: Option<&'static str>,
        exited_on_poll: Option<u32>,
        calls: Vec<String>,
        zero_waits: Vec<u32>,
    }

    impl FakePlatform {
        fn call(&mut self, name: &str) -> Result<(), String> {
            self.calls.push(name.to_owned());
            if self.fail == Some(name) {
                Err(format!("{name} failed"))
            } else {
                Ok(())
            }
        }
    }

    impl RestartPlatform for FakePlatform {
        type Process = u32;
        type Launch = ();
        fn open(&mut self, target: &ProcessActionTarget, allow: bool) -> Result<u32, String> {
            assert!(!allow);
            self.call(&format!("open{}", target.id))?;
            Ok(target.id)
        }
        fn prepare(
            &mut self,
            processes: &[u32],
            target: &ProcessActionTarget,
        ) -> Result<(), String> {
            assert_eq!(processes, [1, 2]);
            assert_eq!(target.id, 1);
            self.call("prepare")
        }
        fn terminate(&mut self, process: &u32) -> Result<(), String> {
            self.call(&format!("stop{process}"))
        }
        fn wait(&mut self, process: &u32, milliseconds: u32) -> Result<(), String> {
            assert!(milliseconds <= 2000);
            if milliseconds == 0 {
                self.zero_waits.push(*process);
            }
            self.call(&format!("wait{process}"))?;
            if milliseconds == 0 && self.exited_on_poll != Some(*process) {
                Err("Process is still running.".to_owned())
            } else {
                Ok(())
            }
        }
        fn launch(&mut self, _: ()) -> Result<(), String> {
            self.call("launch")
        }
    }

    fn targets() -> Vec<ProcessActionTarget> {
        [2, 1]
            .map(|id| ProcessActionTarget {
                id,
                name: "app.exe".into(),
                executable_path: r"C:\Apps\app.exe".into(),
                creation_time: u64::from(id),
                session_id: Some(1),
                is_service_account: Some(false),
            })
            .into()
    }

    #[test]
    fn validates_prepares_stops_waits_then_launches_once() {
        let mut platform = FakePlatform::default();
        let mut launch_parent = None;
        let result = restart_with_platform(&mut platform, targets(), false, |parent| {
            launch_parent = Some((parent.id, parent.creation_time));
            Ok(())
        });
        assert_eq!(launch_parent, Some((1, 1)));
        assert_eq!(result.result, Ok(()));
        assert_eq!(result.termination_handled, [(1, 1), (2, 2)]);
        assert_eq!(
            platform.calls,
            ["open1", "open2", "prepare", "stop1", "stop2", "wait1", "wait2", "launch"]
        );
    }

    #[test]
    fn failed_identity_or_launch_preflight_leaves_every_process_running() {
        for fail in ["open1", "open2", "prepare"] {
            let mut platform = FakePlatform {
                fail: Some(fail),
                ..Default::default()
            };
            let mut preflights = 0;
            let result = restart_with_platform(&mut platform, targets(), false, |_| {
                preflights += 1;
                Ok(())
            });
            assert_eq!(preflights, usize::from(fail == "prepare"));
            assert!(result.result.is_err());
            assert!(result.termination_handled.is_empty());
            assert!(!platform
                .calls
                .iter()
                .any(|call| call.starts_with("stop") || call == "launch"));
        }
    }

    #[test]
    fn failed_parent_preparation_stops_before_creating_a_replacement() {
        let mut platform = FakePlatform::default();
        let result = restart_with_platform(&mut platform, targets(), false, |parent| {
            assert_eq!((parent.id, parent.creation_time), (1, 1));
            Err("Parent affinity could not be restored.".to_owned())
        });
        assert!(result.termination_handled.is_empty());
        assert!(result
            .result
            .unwrap_err()
            .contains("Parent affinity could not be restored."));
        assert_eq!(platform.calls, ["open1", "open2"]);
    }

    #[test]
    fn partial_stop_or_wait_failure_never_launches_a_second_instance() {
        for (fail, exited_on_poll, confirmed) in [
            ("stop1", Some(2), 1),
            ("stop2", None, 0),
            ("wait1", Some(2), 1),
            ("wait2", None, 1),
            ("launch", None, 2),
        ] {
            let mut platform = FakePlatform {
                fail: Some(fail),
                exited_on_poll,
                ..Default::default()
            };
            let result = restart_with_platform(&mut platform, targets(), false, |_| Ok(()));
            let error = result.result.unwrap_err();
            assert!(error.contains(fail));
            assert!(error.starts_with(&format!("Confirmed exit for {confirmed} of 2")));
            assert_eq!(
                result.termination_handled.len(),
                if fail.starts_with("stop") { 1 } else { 2 }
            );
            if fail != "launch" {
                assert!(!platform.calls.iter().any(|call| call == "launch"));
            }
            assert!(platform.calls.iter().any(|call| call == "stop2"));
            if fail == "wait1" {
                assert_eq!(platform.zero_waits, [2]);
            }
        }
    }

    #[test]
    fn process_exiting_after_preflight_does_not_cancel_restart() {
        for (fail, exited) in [("stop1", 1), ("stop2", 2)] {
            let mut platform = FakePlatform {
                fail: Some(fail),
                exited_on_poll: Some(exited),
                ..Default::default()
            };
            let result = restart_with_platform(&mut platform, targets(), false, |_| Ok(()));
            assert_eq!(result.result, Ok(()));
            assert_eq!(result.termination_handled, [(1, 1), (2, 2)]);
            assert_eq!(platform.calls.last().map(String::as_str), Some("launch"));
        }
    }

    #[test]
    fn empty_or_mixed_executables_never_open_processes() {
        let mut mixed = targets();
        mixed[0].executable_path = r"C:\Other\app.exe".into();
        for targets in [Vec::new(), mixed] {
            let mut platform = FakePlatform::default();
            assert!(
                restart_with_platform(&mut platform, targets, false, |_| Ok(()))
                    .result
                    .is_err()
            );
            assert!(platform.calls.is_empty());
        }
    }
}

#[cfg(test)]
mod live_tests;
