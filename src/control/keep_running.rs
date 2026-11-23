//! Keep Running's process safety boundary; scheduling and intent ordering stay in the runtime.
use std::path::Path;

use crate::{
    foreground::process_runs_as_service_account_from_handle,
    platform::windows::keep_running as native,
};

use super::process::{
    open_process_for_observation, ProcessControlError, ProcessControlTarget, ProcessTargetKey,
};

pub(crate) use native::{DisposeOutcome, ErrorKind, ExecutableIdentity};

#[derive(Debug, thiserror::Error)]
pub(crate) enum KeepRunningError {
    #[error("{0}")]
    Process(#[from] ProcessControlError),
    #[error("{0}")]
    Native(#[from] native::KeepRunningError),
}

impl KeepRunningError {
    pub(crate) fn kind(&self) -> ErrorKind {
        match self {
            Self::Native(error) => error.kind(),
            Self::Process(ProcessControlError::AccessDenied(_)) => ErrorKind::Unsupported,
            Self::Process(_) => ErrorKind::Retryable,
        }
    }
}

pub(crate) struct DesktopContext(native::DesktopContext);

impl DesktopContext {
    pub(crate) fn capture() -> Result<Self, KeepRunningError> {
        native::DesktopContext::capture()
            .map(Self)
            .map_err(Into::into)
    }

    /// Opaque canonical identity; never include this key in logs or UI.
    pub(crate) fn binding_key(&self) -> String {
        self.0.binding_key().to_owned()
    }
}

#[derive(Clone)]
pub(crate) struct LaunchSpec(native::LaunchSpec);

impl LaunchSpec {
    pub(crate) fn new(
        executable: &Path,
        arguments: &str,
        working_directory: &Path,
    ) -> Result<Self, KeepRunningError> {
        native::LaunchSpec::new(executable, arguments, working_directory)
            .map(Self)
            .map_err(Into::into)
    }

    pub(crate) fn identity(&self) -> Result<ExecutableIdentity, KeepRunningError> {
        self.0.identity().map_err(Into::into)
    }
}

pub(crate) struct Member {
    target: ProcessControlTarget,
    native: native::Member,
}

impl Member {
    pub(crate) fn key(&self) -> ProcessTargetKey {
        self.target.key()
    }
    pub(crate) fn exit_confirmed(&self) -> Result<bool, KeepRunningError> {
        self.native.exit_confirmed().map_err(Into::into)
    }
}

pub(crate) fn retain_member(
    target: &ProcessControlTarget,
    context: &DesktopContext,
) -> Result<Member, KeepRunningError> {
    let (_, process) = open_process_for_observation(target, false)?;
    if process_runs_as_service_account_from_handle(&process) != Some(false) {
        return Err(native::KeepRunningError::Unsupported(
            "Keep Running requires a verified non-service desktop app.",
        )
        .into());
    }
    let member = native::Member::retain(process, &context.0)?;
    Ok(Member {
        target: target.clone(),
        native: member,
    })
}

/// Fresh enumeration, distinct from cached cycle membership. Any plausible unverifiable match fails
/// the observation instead of certifying absence. Retained members must still match the scan's
/// verified file identity. Callers exclude only their exact prepared child.
pub(crate) fn scan_members(
    spec: &LaunchSpec,
    context: &DesktopContext,
    exclude: Option<&ProcessTargetKey>,
) -> Result<Vec<Member>, KeepRunningError> {
    let (identity, candidates) = native::matching_processes(&spec.0)?;
    let mut members = Vec::new();
    for candidate in candidates {
        let target = ProcessControlTarget::automatic(
            candidate.id,
            candidate.name,
            candidate.path,
            candidate.creation_time,
        );
        if exclude.is_some_and(|key| *key == target.key()) {
            continue;
        }
        members.push(retain_member(&target, context)?);
    }
    if members
        .iter()
        .any(|member| member.native.executable_identity() != &identity)
    {
        return Err(native::KeepRunningError::Unsupported(
            "Keep Running executable identity changed during observation.",
        )
        .into());
    }
    Ok(members)
}

pub(crate) struct PreparedLaunch {
    native: Box<native::PreparedLaunch>,
    target: Option<ProcessControlTarget>,
}

// SAFETY: this value exclusively owns its Windows handles; Windows permits process/thread/job
// handles to move between threads. No operation is concurrent and no references are shared.
unsafe impl Send for PreparedLaunch {}

impl PreparedLaunch {
    fn from_native(native: Box<native::PreparedLaunch>) -> Self {
        let target = native.identity().map(|identity| {
            ProcessControlTarget::automatic(
                identity.id,
                identity.name.clone(),
                identity.path.clone(),
                identity.creation_time,
            )
        });
        Self { native, target }
    }
    pub(crate) fn key(&self) -> Option<ProcessTargetKey> {
        self.target.as_ref().map(ProcessControlTarget::key)
    }
    pub(crate) fn is_accepted(&self) -> bool {
        self.native.is_accepted()
    }
    pub(crate) fn resume_accept(&mut self) -> Result<(), KeepRunningError> {
        self.native.resume_accept().map_err(Into::into)
    }
    pub(crate) fn dispose(&mut self) -> Result<DisposeOutcome, KeepRunningError> {
        self.native.dispose().map_err(Into::into)
    }
    pub(crate) fn take_member(&mut self) -> Result<Option<Member>, KeepRunningError> {
        let Some(target) = self.target.as_ref() else {
            return Ok(None);
        };
        self.native
            .take_member()
            .map(|member| {
                member.map(|native| Member {
                    target: target.clone(),
                    native,
                })
            })
            .map_err(Into::into)
    }
}

pub(crate) struct PrepareFailure {
    pub(crate) error: KeepRunningError,
    pub(crate) cleanup: Option<PreparedLaunch>,
}

pub(crate) fn prepare(
    spec: &LaunchSpec,
    expected_binding: &str,
) -> Result<PreparedLaunch, PrepareFailure> {
    native::prepare(&spec.0, expected_binding)
        .map(PreparedLaunch::from_native)
        .map_err(|failure| PrepareFailure {
            error: failure.error.into(),
            cleanup: failure.cleanup.map(PreparedLaunch::from_native),
        })
}
