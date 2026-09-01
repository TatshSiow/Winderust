use std::{cmp::Ordering, fmt, path::PathBuf};

use crate::{
    foreground::{
        ensure_process_action_target_access_on_handle, executable_path_key,
        process_handle_matches_executable_path, process_session_id, same_process_name,
        ProcessActionAccess, ProcessActionTarget,
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
    CpuSchedulerFocusPriority,
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
        ProcessTargetKey {
            id: self.id,
            creation_time: self.creation_time,
            executable_path: executable_path_key(&self.executable_path).to_ascii_lowercase(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ProcessTargetKey {
    pub(crate) id: u32,
    creation_time: u64,
    executable_path: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProcessControlError {
    AccessDenied(String),
    ProcessExited,
    Unavailable(String),
    Failed(String),
}

impl fmt::Display for ProcessControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccessDenied(message) | Self::Unavailable(message) | Self::Failed(message) => {
                formatter.write_str(message)
            }
            Self::ProcessExited => formatter.write_str("Process exited."),
        }
    }
}

impl std::error::Error for ProcessControlError {}

impl From<ProcessOperationError> for ProcessControlError {
    fn from(error: ProcessOperationError) -> Self {
        match error {
            ProcessOperationError::AccessDenied { .. } => {
                Self::AccessDenied("Access denied.".to_owned())
            }
            ProcessOperationError::ProcessExited => Self::ProcessExited,
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
    if target.id == 0 || target.id == current_process_id {
        return Err(ProcessControlError::AccessDenied(
            "Winderust cannot modify this process.".to_owned(),
        ));
    }
    if !target.executable_path.is_absolute() {
        return Err(ProcessControlError::ProcessExited);
    }

    let handle = windows_process::open(target.id, desired_access)
        .map_err(|error| open_process_error(target.id, error))?;
    let creation_time = handle
        .process_creation_time()
        .ok_or(ProcessControlError::ProcessExited)?;
    if target.creation_time != creation_time
        || !process_handle_matches_executable_path(&handle, &target.executable_path)
    {
        return Err(ProcessControlError::ProcessExited);
    }

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
    use super::*;

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
        assert!(ControlOwner::CpuSchedulerFocusPriority.is_automatic());
        assert!(ControlOwner::ThreadPriority.is_automatic());
        assert!(ControlOwner::AdaptiveEngine.is_automatic());
        assert!(!ControlOwner::ProcessList.is_automatic());
    }
}
