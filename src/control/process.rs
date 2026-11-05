use std::{
    cmp::Ordering,
    path::{Path, PathBuf},
};

use crate::{
    foreground::{
        ensure_process_action_target_access_on_handle, executable_path_key, process_session_id,
        query_process_image_path, same_executable_path, same_process_name, ProcessActionAccess,
        ProcessActionTarget,
    },
    platform::windows::{
        process::{self as windows_process, ProcessAccess, ProcessOpenError},
        ProcessOperationError,
    },
    win_util::WinHandle,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ControlOwner {
    BackgroundEfficiency,
    CpuSetsSoft,
    DynamicPriorityBoost,
    GpuPriority,
    IoPriority,
    MemoryPriority,
    ProcessPriority,
    ProcessorAffinityHard,
    ThreadPriority,
    AdaptiveEngine,
    AdaptiveEngineProcessFocusPriority,
    ProcessList,
}

impl ControlOwner {
    pub(crate) fn is_automatic(self) -> bool {
        !matches!(self, Self::ProcessList)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessControlTarget {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) executable_path: PathBuf,
    pub(crate) creation_time: u64,
}

impl ProcessControlTarget {
    pub(crate) fn automatic(
        id: u32,
        name: String,
        executable_path: PathBuf,
        creation_time: u64,
    ) -> Self {
        Self {
            id,
            name,
            executable_path,
            creation_time,
        }
    }

    pub(crate) fn from_action_target(target: &ProcessActionTarget) -> Self {
        Self {
            id: target.id,
            name: target.name.clone(),
            executable_path: target.executable_path.clone(),
            creation_time: target.creation_time,
        }
    }

    pub(crate) fn key(&self) -> ProcessTargetKey {
        ProcessTargetKey::new(self.id, self.creation_time, &self.executable_path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ProcessTargetKey {
    pub(crate) id: u32,
    creation_time: u64,
    executable_path: String,
}

impl ProcessTargetKey {
    pub(crate) fn new(id: u32, creation_time: u64, executable_path: &Path) -> Self {
        Self {
            id,
            creation_time,
            executable_path: executable_path_key(executable_path).to_ascii_lowercase(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessIdentity {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) executable_path: PathBuf,
    pub(crate) creation_time: u64,
    pub(crate) session_id: Option<u32>,
    executable_path_key: String,
}

impl PartialEq for ProcessIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.creation_time == other.creation_time
            && self.executable_path_key == other.executable_path_key
    }
}

impl Eq for ProcessIdentity {}

impl PartialOrd for ProcessIdentity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ProcessIdentity {
    fn cmp(&self, other: &Self) -> Ordering {
        (
            self.id,
            self.creation_time,
            self.executable_path_key.as_str(),
        )
            .cmp(&(
                other.id,
                other.creation_time,
                other.executable_path_key.as_str(),
            ))
    }
}

impl ProcessIdentity {
    pub(crate) fn new(
        id: u32,
        name: String,
        executable_path: PathBuf,
        creation_time: u64,
        session_id: Option<u32>,
    ) -> Self {
        let executable_path_key = executable_path_key(&executable_path).to_ascii_lowercase();
        Self {
            id,
            name,
            executable_path,
            creation_time,
            session_id,
            executable_path_key,
        }
    }

    pub(crate) fn target(&self) -> ProcessControlTarget {
        ProcessControlTarget {
            id: self.id,
            name: self.name.clone(),
            executable_path: self.executable_path.clone(),
            creation_time: self.creation_time,
        }
    }

    pub(crate) fn key(&self) -> ProcessTargetKey {
        ProcessTargetKey {
            id: self.id,
            creation_time: self.creation_time,
            executable_path: self.executable_path_key.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ProcessControlError {
    #[error("{0}")]
    AccessDenied(String),
    #[error("Process exited.")]
    ProcessExited,
    #[error("{0}")]
    Unavailable(String),
    #[error("{0}")]
    Failed(String),
}

// Keep confirmed exits typed so callers can relinquish the exact recovery record.
pub(super) fn transition_failure_error(
    primary_error: ProcessControlError,
    compensation_error: ProcessControlError,
    recovery_error: Option<String>,
) -> ProcessControlError {
    if primary_error == ProcessControlError::ProcessExited
        || compensation_error == ProcessControlError::ProcessExited
    {
        return ProcessControlError::ProcessExited;
    }
    let mut message = format!("{primary_error} Compensation failed: {compensation_error}.");
    if let Some(recovery_error) = recovery_error {
        message.push_str(&format!(
            " Recovery journal update failed: {recovery_error}."
        ));
    }
    ProcessControlError::Failed(message)
}

impl From<ProcessOperationError> for ProcessControlError {
    fn from(error: ProcessOperationError) -> Self {
        match error {
            ProcessOperationError::AccessDenied { .. } => {
                Self::AccessDenied("Access denied.".to_owned())
            }
            ProcessOperationError::Failed { operation, code } => {
                Self::Failed(format!("{operation} failed with error {code}."))
            }
        }
    }
}

pub(crate) fn open_process_for_set_information(
    target: &ProcessControlTarget,
    allow_cross_session_process_control: bool,
) -> Result<(ProcessIdentity, WinHandle), ProcessControlError> {
    open_process_with_access(
        target,
        allow_cross_session_process_control,
        ProcessAccess::SetInformation,
        ProcessActionAccess::SetInformation,
    )
}

pub(crate) fn open_process_for_thread_control(
    target: &ProcessControlTarget,
    allow_cross_session_process_control: bool,
) -> Result<(ProcessIdentity, WinHandle), ProcessControlError> {
    open_process_with_access(
        target,
        allow_cross_session_process_control,
        ProcessAccess::SafetyOnly,
        ProcessActionAccess::SafetyOnly,
    )
}

pub(crate) fn open_process_for_thread_snapshot(
    target: &ProcessControlTarget,
    allow_cross_session_process_control: bool,
) -> Result<(ProcessIdentity, WinHandle), ProcessControlError> {
    open_process_with_access(
        target,
        allow_cross_session_process_control,
        ProcessAccess::ThreadSnapshot,
        ProcessActionAccess::SafetyOnly,
    )
}

pub(crate) fn open_process_for_working_set_trim(
    target: &ProcessControlTarget,
    allow_cross_session_process_control: bool,
) -> Result<(ProcessIdentity, WinHandle), ProcessControlError> {
    open_process_with_access(
        target,
        allow_cross_session_process_control,
        ProcessAccess::WorkingSetTrim,
        ProcessActionAccess::TrimWorkingSet,
    )
}

pub(crate) fn open_process_for_termination(
    target: &ProcessControlTarget,
    allow_cross_session_process_control: bool,
) -> Result<(ProcessIdentity, WinHandle), ProcessControlError> {
    open_process_with_access(
        target,
        allow_cross_session_process_control,
        ProcessAccess::Termination,
        ProcessActionAccess::Terminate,
    )
}

pub(crate) fn open_process_for_restart(
    target: &ProcessControlTarget,
    allow_cross_session_process_control: bool,
) -> Result<(ProcessIdentity, WinHandle), ProcessControlError> {
    let (identity, process) = open_process_with_access(
        target,
        allow_cross_session_process_control,
        ProcessAccess::Restart,
        ProcessActionAccess::Terminate,
    )?;
    let current_session =
        process_session_id(windows_process::current_process_id()).ok_or_else(|| {
            ProcessControlError::Unavailable(
                "Winderust's Windows session is unavailable.".to_owned(),
            )
        })?;
    if identity.session_id != Some(current_session)
        || crate::foreground::process_runs_as_service_account_from_handle(&process) != Some(false)
    {
        return Err(ProcessControlError::AccessDenied(
            "Restart requires a non-service app in Winderust's Windows session.".to_owned(),
        ));
    }
    Ok((identity, process))
}

pub(crate) fn open_process_for_job_assignment(
    target: &ProcessControlTarget,
    allow_cross_session_process_control: bool,
) -> Result<(ProcessIdentity, WinHandle), ProcessControlError> {
    open_process_with_access(
        target,
        allow_cross_session_process_control,
        ProcessAccess::JobAssignment,
        ProcessActionAccess::AssignToJob,
    )
}

fn open_process_with_access(
    target: &ProcessControlTarget,
    allow_cross_session_process_control: bool,
    desired_access: ProcessAccess,
    action_access: ProcessActionAccess,
) -> Result<(ProcessIdentity, WinHandle), ProcessControlError> {
    let current_process_id = windows_process::current_process_id();
    if target.id == 0
        || target.id == current_process_id
        || crate::crash_recovery::is_watchdog_process(target.id)
    {
        return Err(ProcessControlError::AccessDenied(
            "Winderust cannot modify this process.".to_owned(),
        ));
    }
    if !target.executable_path.is_absolute() {
        return Err(ProcessControlError::ProcessExited);
    }

    let handle = windows_process::open(target.id, desired_access)
        .map_err(|error| open_process_error(target.id, error))?;
    let creation_time = validate_identity_observation(
        target,
        handle.process_times().map(|(creation, _)| creation),
        || query_process_image_path(&handle),
    )
    .map_err(|error| classify_identity_failure(error, windows_process::exit_confirmed(&handle)))?;

    let session_id = process_session_id(target.id);
    if !allow_cross_session_process_control {
        let current_session_id = process_session_id(current_process_id).ok_or_else(|| {
            ProcessControlError::Failed(
                "Could not determine Winderust's current Windows session.".to_owned(),
            )
        })?;
        if session_id != Some(current_session_id) {
            return Err(ProcessControlError::AccessDenied(
                "Processes in another Windows session cannot be modified.".to_owned(),
            ));
        }
    }

    let name = target
        .executable_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or(ProcessControlError::ProcessExited)?
        .to_ascii_lowercase();
    if !same_process_name(&name, &target.name) {
        return Err(ProcessControlError::ProcessExited);
    }
    let identity = ProcessIdentity::new(
        target.id,
        name,
        target.executable_path.clone(),
        creation_time,
        session_id,
    );
    let action_target = ProcessActionTarget {
        id: identity.id,
        name: identity.name.clone(),
        executable_path: identity.executable_path.clone(),
        creation_time: identity.creation_time,
        session_id: identity.session_id,
        is_service_account: None,
    };
    ensure_process_action_target_access_on_handle(&action_target, action_access, &handle)
        .map_err(ProcessControlError::AccessDenied)?;
    Ok((identity, handle))
}

fn validate_identity_observation(
    target: &ProcessControlTarget,
    creation: Result<u64, u32>,
    path: impl FnOnce() -> Result<PathBuf, u32>,
) -> Result<u64, ProcessControlError> {
    let unavailable = |operation: &str, code| {
        ProcessControlError::Unavailable(format!(
            "{operation}({}) failed with error {code}.",
            target.id
        ))
    };
    let creation = creation.map_err(|code| unavailable("GetProcessTimes", code))?;
    if creation != target.creation_time {
        return Err(ProcessControlError::ProcessExited);
    }
    let path = path().map_err(|code| unavailable("QueryFullProcessImageNameW", code))?;
    if !same_executable_path(&path, &target.executable_path) {
        return Err(ProcessControlError::ProcessExited);
    }
    Ok(creation)
}

fn classify_identity_failure(
    error: ProcessControlError,
    exit_confirmed: Result<bool, ProcessOperationError>,
) -> ProcessControlError {
    if exit_confirmed == Ok(true) {
        ProcessControlError::ProcessExited
    } else {
        error
    }
}

fn open_process_error(process_id: u32, error: ProcessOpenError) -> ProcessControlError {
    match error {
        ProcessOpenError::AccessDenied => {
            ProcessControlError::AccessDenied("Access denied.".to_owned())
        }
        ProcessOpenError::ProcessExited => ProcessControlError::ProcessExited,
        ProcessOpenError::Failed { code } => ProcessControlError::Failed(format!(
            "OpenProcess({process_id}) failed with error {code}."
        )),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn transition_errors_preserve_exits_and_report_other_failures() {
        use super::{transition_failure_error, ProcessControlError};
        let denied = ProcessControlError::AccessDenied("access denied".into());
        for (primary, compensation) in [
            (ProcessControlError::ProcessExited, denied.clone()),
            (denied.clone(), ProcessControlError::ProcessExited),
        ] {
            assert_eq!(
                transition_failure_error(primary, compensation, Some("journal failed".into())),
                ProcessControlError::ProcessExited
            );
        }
        let error = transition_failure_error(
            denied,
            ProcessControlError::Failed("restore failed".into()),
            Some("journal failed".into()),
        );
        assert_eq!(error, ProcessControlError::Failed("access denied Compensation failed: restore failed. Recovery journal update failed: journal failed.".into()));
    }

    use super::*;

    #[test]
    fn identity_query_failures_require_positive_exit_evidence_for_cleanup() {
        let unavailable =
            ProcessControlError::Unavailable("path query failed with error 31".into());
        assert_eq!(
            classify_identity_failure(unavailable.clone(), Ok(true)),
            ProcessControlError::ProcessExited
        );
        for observation in [
            Ok(false),
            Err(ProcessOperationError::AccessDenied {
                operation: "GetExitCodeProcess",
            }),
            Err(ProcessOperationError::Failed {
                operation: "GetExitCodeProcess",
                code: 31,
            }),
        ] {
            assert_eq!(
                classify_identity_failure(unavailable.clone(), observation),
                unavailable
            );
            assert_eq!(
                classify_identity_failure(ProcessControlError::ProcessExited, observation),
                ProcessControlError::ProcessExited
            );
        }
    }

    #[test]
    #[ignore = "opens and terminates its own disposable process; run in integration QA"]
    fn retained_exited_process_is_classified_for_cleanup() -> Result<(), String> {
        use std::{
            os::windows::process::CommandExt,
            process::{Child, Command, Stdio},
        };

        struct OwnedProcess(Child);
        impl Drop for OwnedProcess {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let executable = PathBuf::from(std::env::var_os("SystemRoot").ok_or("SystemRoot missing")?)
            .join(r"System32\PING.EXE");
        let mut child = OwnedProcess(
            Command::new(&executable)
                .args(["-t", "127.0.0.1"])
                .creation_flags(0x0800_0000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| error.to_string())?,
        );
        let target =
            crate::foreground::capture_process_action_target(child.0.id(), &executable, false)
                .map_err(|error| error.to_string())?;
        let target = ProcessControlTarget::from_action_target(&target);
        let (_, retained) =
            open_process_for_set_information(&target, false).map_err(|error| error.to_string())?;
        assert_eq!(windows_process::exit_confirmed(&retained), Ok(false));
        child.0.kill().map_err(|error| error.to_string())?;
        child.0.wait().map_err(|error| error.to_string())?;
        assert_eq!(windows_process::exit_confirmed(&retained), Ok(true));
        assert!(matches!(
            open_process_for_set_information(&target, false),
            Err(ProcessControlError::ProcessExited)
        ));
        Ok(())
    }

    #[test]
    fn unavailable_identity_is_not_an_exit_but_verified_mismatch_is() {
        let target = ProcessControlTarget::automatic(
            42,
            "app.exe".into(),
            PathBuf::from(r"C:\Apps\app.exe"),
            7,
        );
        assert!(
            matches!(validate_identity_observation(&target, Err(5), || panic!("no path query after timing failure")), Err(ProcessControlError::Unavailable(message)) if message.contains("5"))
        );
        assert!(matches!(
            validate_identity_observation(&target, Ok(7), || Err(5)),
            Err(ProcessControlError::Unavailable(_))
        ));
        assert_eq!(
            validate_identity_observation(&target, Ok(8), || panic!("known replacement")),
            Err(ProcessControlError::ProcessExited)
        );
        assert_eq!(
            validate_identity_observation(&target, Ok(7), || Ok(PathBuf::from(r"C:\other.exe"))),
            Err(ProcessControlError::ProcessExited)
        );
        assert_eq!(
            validate_identity_observation(&target, Ok(7), || Ok(target.executable_path.clone())),
            Ok(7)
        );
    }

    #[test]
    fn process_identity_keys_include_the_instance_path_but_not_policy_metadata() {
        let target = ProcessControlTarget::automatic(
            42,
            "APP.EXE".to_owned(),
            PathBuf::from(r"C:\Apps\app.exe"),
            7,
        );
        let identity = ProcessIdentity::new(
            target.id,
            "app.exe".to_owned(),
            target.executable_path.clone(),
            7,
            Some(1),
        );

        assert_eq!(target.key(), identity.key());
        assert_eq!(
            target.key(),
            ProcessTargetKey::new(target.id, target.creation_time, &target.executable_path)
        );
        assert_eq!(identity.target().creation_time, 7);

        let metadata_drift = ProcessIdentity::new(
            target.id,
            "APP.EXE".to_owned(),
            PathBuf::from(r"c:/apps/APP.exe"),
            7,
            None,
        );
        assert_eq!(identity, metadata_drift);
        assert_eq!(identity.cmp(&metadata_drift), Ordering::Equal);
        assert_eq!(
            target.key(),
            ProcessTargetKey::new(42, 7, &metadata_drift.executable_path)
        );
        assert_ne!(
            target.key(),
            ProcessTargetKey::new(43, 7, &target.executable_path)
        );
        assert_ne!(
            target.key(),
            ProcessTargetKey::new(42, 7, Path::new(r"C:\Other\app.exe"))
        );

        let replacement = ProcessControlTarget::automatic(
            42,
            "app.exe".to_owned(),
            PathBuf::from(r"C:\Apps\app.exe"),
            8,
        );
        assert_ne!(target.key(), replacement.key());
    }

    #[test]
    fn only_process_list_is_a_non_automatic_owner() {
        assert!(ControlOwner::DynamicPriorityBoost.is_automatic());
        assert!(ControlOwner::GpuPriority.is_automatic());
        assert!(ControlOwner::IoPriority.is_automatic());
        assert!(ControlOwner::ProcessPriority.is_automatic());
        assert!(ControlOwner::BackgroundEfficiency.is_automatic());
        assert!(ControlOwner::CpuSetsSoft.is_automatic());
        assert!(ControlOwner::ProcessorAffinityHard.is_automatic());
        assert!(ControlOwner::AdaptiveEngineProcessFocusPriority.is_automatic());
        assert!(ControlOwner::ThreadPriority.is_automatic());
        assert!(ControlOwner::AdaptiveEngine.is_automatic());
        assert!(!ControlOwner::ProcessList.is_automatic());
    }
}
