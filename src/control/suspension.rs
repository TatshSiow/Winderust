use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    crash_recovery::{self, RecoveryIntent},
    foreground::{
        contains_process_name, process_runs_as_service_account_from_handle, ProcessActionTarget,
        EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    platform::windows::suspension::{self as windows_suspension, JobObjectError},
    win_util::WinHandle,
};

use super::process::{
    open_process_for_suspension, ProcessControlError, ProcessControlTarget, ProcessIdentity,
    ProcessTargetKey,
};

const APP_SUSPENSION_ONLY_BUILT_IN_EXCLUSIONS: &[&str] = &[
    "appactions.exe",
    "applicationframehost.exe",
    "backgroundtaskhost.exe",
    "crossdeviceresume.exe",
    "dllhost.exe",
    "lockapp.exe",
    "runtimebroker.exe",
    "shellhost.exe",
    "svchost.exe",
    "systemsettingsbroker.exe",
    "taskhostw.exe",
    "unsecapp.exe",
    "useroobebroker.exe",
    "wmiprvse.exe",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SuspensionError {
    AccessDenied,
    ProcessExited,
    NotSupported,
    Unsupported,
    RetryPending(String),
    Failed(String),
}

impl SuspensionError {
    pub(crate) fn should_report(&self) -> bool {
        !matches!(self, Self::RetryPending(_))
    }
}

impl fmt::Display for SuspensionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccessDenied => formatter.write_str("Access denied."),
            Self::ProcessExited => formatter.write_str("Process exited."),
            Self::NotSupported => formatter.write_str("Operation not supported for this process."),
            Self::Unsupported => formatter.write_str("Windows Job Object freeze is unsupported."),
            Self::RetryPending(message) | Self::Failed(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for SuspensionError {}

pub(crate) fn suspension_error_message(error: &SuspensionError) -> String {
    error.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SuspensionTarget {
    pub(crate) process: ProcessControlTarget,
    pub(crate) is_service_account: Option<bool>,
}

impl SuspensionTarget {
    pub(crate) fn automatic(
        id: u32,
        name: String,
        executable_path: PathBuf,
        creation_time: u64,
        is_service_account: Option<bool>,
    ) -> Self {
        Self {
            process: ProcessControlTarget::automatic(id, name, executable_path, creation_time),
            is_service_account,
        }
    }

    pub(crate) fn from_action_target(target: &ProcessActionTarget) -> Self {
        Self {
            process: ProcessControlTarget::from_action_target(target),
            is_service_account: target.is_service_account,
        }
    }

    pub(crate) fn key(&self) -> ProcessTargetKey {
        self.process.key()
    }
}

pub(crate) fn is_builtin_excluded(process_name: &str) -> bool {
    let process_name = Path::new(process_name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(process_name);
    contains_process_name(EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS, process_name)
        || contains_process_name(APP_SUSPENSION_ONLY_BUILT_IN_EXCLUSIONS, process_name)
}

pub(crate) fn process_is_suspendable(target: &ProcessActionTarget) -> bool {
    !is_builtin_excluded(&target.name)
        && target.session_id.is_some_and(|session_id| session_id != 0)
        && target.is_service_account == Some(false)
}

pub(crate) trait SuspensionPlatform {
    type Handle;
    type Intent;

    fn assign(
        &mut self,
        target: &SuspensionTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<(ProcessIdentity, Self::Handle), SuspensionError>;
    fn begin_freeze(
        &mut self,
        identity: &ProcessIdentity,
        handle: &Self::Handle,
    ) -> Result<Self::Intent, SuspensionError>;
    fn set_frozen(&mut self, handle: &Self::Handle, frozen: bool) -> Result<(), SuspensionError>;
    fn commit_freeze(&mut self, intent: Self::Intent) -> Result<(), SuspensionError>;
    fn forget(&mut self, handle: &Self::Handle) -> Result<(), SuspensionError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SuspensionState {
    Thawed,
    Frozen,
    FrozenCompensationPending,
    ThawedCleanupPending { release_after_cleanup: bool },
}

struct RetryState {
    next_attempt: Instant,
    delay: Duration,
    message: String,
}

struct ManagedSuspension<H> {
    identity: ProcessIdentity,
    handle: H,
    state: SuspensionState,
    retry: Option<RetryState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SuspensionFreezeOutcome {
    Frozen,
    Unchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SuspensionThawOutcome {
    Thawed,
    Unchanged,
}

const RETRY_INITIAL: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(60);

pub(crate) struct SuspensionController<P: SuspensionPlatform = WindowsSuspensionPlatform> {
    platform: P,
    managed: BTreeMap<ProcessTargetKey, ManagedSuspension<P::Handle>>,
}

impl Default for SuspensionController<WindowsSuspensionPlatform> {
    fn default() -> Self {
        Self::new(WindowsSuspensionPlatform)
    }
}

impl<P> SuspensionController<P>
where
    P: SuspensionPlatform,
{
    pub(crate) fn new(platform: P) -> Self {
        Self {
            platform,
            managed: BTreeMap::new(),
        }
    }

    pub(crate) fn freeze(
        &mut self,
        target: &SuspensionTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<SuspensionFreezeOutcome, SuspensionError> {
        let key = target.key();
        let stale_keys = self
            .managed
            .keys()
            .filter(|candidate| candidate.id == target.process.id && **candidate != key)
            .cloned()
            .collect::<Vec<_>>();
        for stale_key in stale_keys {
            self.release_key(&stale_key, true)?;
        }

        if !self.managed.contains_key(&key) {
            let (identity, handle) = self
                .platform
                .assign(target, allow_cross_session_process_control)?;
            self.managed.insert(
                key.clone(),
                ManagedSuspension {
                    identity,
                    handle,
                    state: SuspensionState::Thawed,
                    retry: None,
                },
            );
        }

        self.prepare_for_freeze(&key)?;
        if self
            .managed
            .get(&key)
            .is_some_and(|managed| managed.state == SuspensionState::Frozen)
        {
            return Ok(SuspensionFreezeOutcome::Unchanged);
        }

        let intent_result = {
            let managed = self
                .managed
                .get(&key)
                .ok_or(SuspensionError::ProcessExited)?;
            self.platform
                .begin_freeze(&managed.identity, &managed.handle)
        };
        let intent = match intent_result {
            Ok(intent) => intent,
            Err(error) => {
                self.managed.remove(&key);
                return Err(error);
            }
        };
        let apply_result = {
            let managed = self
                .managed
                .get(&key)
                .ok_or(SuspensionError::ProcessExited)?;
            self.platform.set_frozen(&managed.handle, true)
        };
        if let Err(error) = apply_result {
            drop(intent);
            self.managed.remove(&key);
            return Err(error);
        }

        match self.platform.commit_freeze(intent) {
            Ok(()) => {
                let managed = self
                    .managed
                    .get_mut(&key)
                    .ok_or(SuspensionError::ProcessExited)?;
                managed.state = SuspensionState::Frozen;
                managed.retry = None;
                Ok(SuspensionFreezeOutcome::Frozen)
            }
            Err(commit_error) => {
                let thaw_result = {
                    let managed = self
                        .managed
                        .get(&key)
                        .ok_or(SuspensionError::ProcessExited)?;
                    self.platform.set_frozen(&managed.handle, false)
                };
                match thaw_result {
                    Err(thaw_error) => {
                        let message =
                            format!("{} Compensation also failed: {}", commit_error, thaw_error);
                        let managed = self
                            .managed
                            .get_mut(&key)
                            .ok_or(SuspensionError::ProcessExited)?;
                        managed.state = SuspensionState::FrozenCompensationPending;
                        schedule_retry(managed, message.clone());
                        Err(SuspensionError::Failed(message))
                    }
                    Ok(()) => {
                        let forget_result = {
                            let managed = self
                                .managed
                                .get(&key)
                                .ok_or(SuspensionError::ProcessExited)?;
                            self.platform.forget(&managed.handle)
                        };
                        if let Err(forget_error) = forget_result {
                            let message = format!(
                                "{} Recovery cleanup also failed: {}",
                                commit_error, forget_error
                            );
                            let managed = self
                                .managed
                                .get_mut(&key)
                                .ok_or(SuspensionError::ProcessExited)?;
                            managed.state = SuspensionState::ThawedCleanupPending {
                                release_after_cleanup: true,
                            };
                            schedule_retry(managed, message.clone());
                            return Err(SuspensionError::Failed(message));
                        }
                        self.managed.remove(&key);
                        Err(commit_error)
                    }
                }
            }
        }
    }

    pub(crate) fn thaw_keep_process(
        &mut self,
        process_id: u32,
        force: bool,
    ) -> Result<SuspensionThawOutcome, SuspensionError> {
        let Some(key) = self.key_for_process_id(process_id) else {
            return Ok(SuspensionThawOutcome::Unchanged);
        };
        self.thaw_keep_key(&key, force)
    }

    pub(crate) fn thaw_keep_target(
        &mut self,
        target: &SuspensionTarget,
        force: bool,
    ) -> Result<SuspensionThawOutcome, SuspensionError> {
        let key = target.key();
        if !self.managed.contains_key(&key) {
            return Err(SuspensionError::ProcessExited);
        }
        self.thaw_keep_key(&key, force)
    }

    pub(crate) fn release_process(
        &mut self,
        process_id: u32,
        force: bool,
    ) -> Result<bool, SuspensionError> {
        let keys = self
            .managed
            .keys()
            .filter(|key| key.id == process_id)
            .cloned()
            .collect::<Vec<_>>();
        if keys.is_empty() {
            return Ok(false);
        }
        for key in keys {
            self.release_key(&key, force)?;
        }
        Ok(true)
    }

    pub(crate) fn managed_process_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.managed.keys().map(|key| key.id)
    }

    pub(crate) fn contains_process(&self, process_id: u32) -> bool {
        self.managed.keys().any(|key| key.id == process_id)
    }

    pub(crate) fn matches_target(&self, target: &SuspensionTarget) -> bool {
        self.managed.contains_key(&target.key())
    }

    pub(crate) fn matches_process_path(&self, process_id: u32, executable_path: &Path) -> bool {
        self.managed.iter().any(|(key, managed)| {
            key.id == process_id
                && crate::foreground::same_executable_path(
                    &managed.identity.executable_path,
                    executable_path,
                )
        })
    }

    pub(crate) fn is_frozen_process(&self, process_id: u32) -> bool {
        self.managed.iter().any(|(key, managed)| {
            key.id == process_id
                && matches!(
                    managed.state,
                    SuspensionState::Frozen | SuspensionState::FrozenCompensationPending
                )
        })
    }

    pub(crate) fn is_frozen_target(&self, target: &SuspensionTarget) -> bool {
        self.managed.get(&target.key()).is_some_and(|managed| {
            matches!(
                managed.state,
                SuspensionState::Frozen | SuspensionState::FrozenCompensationPending
            )
        })
    }

    pub(crate) fn reconcile_pending(&mut self, force: bool) {
        let pending_keys = self
            .managed
            .iter()
            .filter(|(_key, managed)| {
                matches!(
                    managed.state,
                    SuspensionState::FrozenCompensationPending
                        | SuspensionState::ThawedCleanupPending { .. }
                )
            })
            .map(|(key, _managed)| key.clone())
            .collect::<Vec<_>>();

        for key in pending_keys {
            let state = self.managed.get(&key).map(|managed| managed.state);
            let result = match state {
                Some(SuspensionState::FrozenCompensationPending) => self.release_key(&key, force),
                Some(SuspensionState::ThawedCleanupPending { .. }) => {
                    self.finish_cleanup(&key, force)
                }
                Some(SuspensionState::Thawed | SuspensionState::Frozen) | None => continue,
            };
            if let Err(error) = result {
                debug_assert!(
                    matches!(error, SuspensionError::RetryPending(_)),
                    "pending suspension reconciliation returned an unexpected error: {error}"
                );
            }
        }
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        let keys = self.managed.keys().cloned().collect::<Vec<_>>();
        let mut errors = Vec::new();
        for key in keys {
            if let Err(error) = self.release_key(&key, true) {
                errors.push(format!("PID {}: {error}", key.id));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join(" "))
        }
    }

    fn prepare_for_freeze(&mut self, key: &ProcessTargetKey) -> Result<(), SuspensionError> {
        let Some(state) = self.managed.get(key).map(|managed| managed.state) else {
            return Err(SuspensionError::ProcessExited);
        };
        match state {
            SuspensionState::Thawed | SuspensionState::Frozen => Ok(()),
            SuspensionState::FrozenCompensationPending => {
                self.release_key(key, false)?;
                Err(SuspensionError::RetryPending(
                    "The previous failed suspension transaction was cleaned up; retry the request."
                        .to_owned(),
                ))
            }
            SuspensionState::ThawedCleanupPending {
                release_after_cleanup,
            } => {
                self.finish_cleanup(key, false)?;
                if release_after_cleanup || !self.managed.contains_key(key) {
                    Err(SuspensionError::RetryPending(
                        "The previous failed suspension transaction was cleaned up; retry the request."
                            .to_owned(),
                    ))
                } else {
                    Ok(())
                }
            }
        }
    }

    fn thaw_keep_key(
        &mut self,
        key: &ProcessTargetKey,
        force: bool,
    ) -> Result<SuspensionThawOutcome, SuspensionError> {
        let Some(state) = self.managed.get(key).map(|managed| managed.state) else {
            return Ok(SuspensionThawOutcome::Unchanged);
        };
        if let SuspensionState::Thawed = state {
            return Ok(SuspensionThawOutcome::Unchanged);
        }
        if matches!(state, SuspensionState::ThawedCleanupPending { .. }) {
            self.finish_cleanup(key, force)?;
            return Ok(SuspensionThawOutcome::Thawed);
        }
        self.ensure_retry_due(key, force)?;
        let was_retry = self
            .managed
            .get(key)
            .is_some_and(|managed| managed.retry.is_some());
        let thaw_result = {
            let managed = self
                .managed
                .get(key)
                .ok_or(SuspensionError::ProcessExited)?;
            self.platform.set_frozen(&managed.handle, false)
        };
        if let Err(error) = thaw_result {
            return Err(self.retain_failure(key, error.to_string(), was_retry));
        }
        {
            let managed = self
                .managed
                .get_mut(key)
                .ok_or(SuspensionError::ProcessExited)?;
            managed.state = SuspensionState::ThawedCleanupPending {
                release_after_cleanup: false,
            };
            managed.retry = None;
        }
        self.finish_cleanup(key, true)?;
        Ok(SuspensionThawOutcome::Thawed)
    }

    fn release_key(&mut self, key: &ProcessTargetKey, force: bool) -> Result<(), SuspensionError> {
        let Some(state) = self.managed.get(key).map(|managed| managed.state) else {
            return Ok(());
        };
        if state == SuspensionState::Thawed {
            self.managed.remove(key);
            return Ok(());
        }
        if matches!(state, SuspensionState::ThawedCleanupPending { .. }) {
            if let Some(managed) = self.managed.get_mut(key) {
                managed.state = SuspensionState::ThawedCleanupPending {
                    release_after_cleanup: true,
                };
            }
            return self.finish_cleanup(key, force);
        }

        self.ensure_retry_due(key, force)?;
        let was_retry = self
            .managed
            .get(key)
            .is_some_and(|managed| managed.retry.is_some());
        let thaw_result = {
            let managed = self
                .managed
                .get(key)
                .ok_or(SuspensionError::ProcessExited)?;
            self.platform.set_frozen(&managed.handle, false)
        };
        if let Err(error) = thaw_result {
            return Err(self.retain_failure(key, error.to_string(), was_retry));
        }
        {
            let managed = self
                .managed
                .get_mut(key)
                .ok_or(SuspensionError::ProcessExited)?;
            managed.state = SuspensionState::ThawedCleanupPending {
                release_after_cleanup: true,
            };
            managed.retry = None;
        }
        self.finish_cleanup(key, true)
    }

    fn finish_cleanup(
        &mut self,
        key: &ProcessTargetKey,
        force: bool,
    ) -> Result<(), SuspensionError> {
        self.ensure_retry_due(key, force)?;
        let was_retry = self
            .managed
            .get(key)
            .is_some_and(|managed| managed.retry.is_some());
        let forget_result = {
            let managed = self
                .managed
                .get(key)
                .ok_or(SuspensionError::ProcessExited)?;
            self.platform.forget(&managed.handle)
        };
        if let Err(error) = forget_result {
            return Err(self.retain_failure(key, error.to_string(), was_retry));
        }
        let release_after_cleanup = self.managed.get(key).is_some_and(|managed| {
            matches!(
                managed.state,
                SuspensionState::ThawedCleanupPending {
                    release_after_cleanup: true
                }
            )
        });
        if release_after_cleanup {
            self.managed.remove(key);
        } else if let Some(managed) = self.managed.get_mut(key) {
            managed.state = SuspensionState::Thawed;
            managed.retry = None;
        }
        Ok(())
    }

    fn ensure_retry_due(&self, key: &ProcessTargetKey, force: bool) -> Result<(), SuspensionError> {
        let Some(retry) = self
            .managed
            .get(key)
            .and_then(|managed| managed.retry.as_ref())
        else {
            return Ok(());
        };
        if force || Instant::now() >= retry.next_attempt {
            Ok(())
        } else {
            Err(SuspensionError::RetryPending(retry.message.clone()))
        }
    }

    fn retain_failure(
        &mut self,
        key: &ProcessTargetKey,
        message: String,
        was_retry: bool,
    ) -> SuspensionError {
        if let Some(managed) = self.managed.get_mut(key) {
            schedule_retry(managed, message.clone());
        }
        if was_retry {
            SuspensionError::RetryPending(message)
        } else {
            SuspensionError::Failed(message)
        }
    }

    fn key_for_process_id(&self, process_id: u32) -> Option<ProcessTargetKey> {
        self.managed
            .keys()
            .find(|key| key.id == process_id)
            .cloned()
    }
}

impl<P> Drop for SuspensionController<P>
where
    P: SuspensionPlatform,
{
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn schedule_retry<H>(managed: &mut ManagedSuspension<H>, message: String) {
    let delay = managed
        .retry
        .as_ref()
        .map_or(RETRY_INITIAL, |retry| (retry.delay * 2).min(RETRY_MAX));
    managed.retry = Some(RetryState {
        next_attempt: Instant::now() + delay,
        delay,
        message,
    });
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct WindowsSuspensionPlatform;

pub(crate) struct WindowsSuspensionHandle {
    job_handle: Option<windows_suspension::JobHandle>,
    job_name: Option<String>,
    process_handle: Option<WinHandle>,
}

impl SuspensionPlatform for WindowsSuspensionPlatform {
    type Handle = WindowsSuspensionHandle;
    type Intent = RecoveryIntent;

    fn assign(
        &mut self,
        target: &SuspensionTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<(ProcessIdentity, Self::Handle), SuspensionError> {
        if is_builtin_excluded(&target.process.name) || target.is_service_account != Some(false) {
            return Err(SuspensionError::AccessDenied);
        }
        let (identity, process_handle) =
            open_process_for_suspension(&target.process, allow_cross_session_process_control)
                .map_err(map_process_control_error)?;
        if identity.session_id.is_none_or(|session_id| session_id == 0) {
            return Err(SuspensionError::AccessDenied);
        }
        if process_runs_as_service_account_from_handle(&process_handle) != Some(false) {
            return Err(SuspensionError::AccessDenied);
        }

        let job_name = crash_recovery::suspension_job_name(identity.id, identity.creation_time);
        let created = windows_suspension::create_job(&job_name).map_err(map_job_object_error)?;
        let job_handle = created.handle;
        if created.already_existed {
            match windows_suspension::process_is_in_job(&process_handle, Some(&job_handle)) {
                Some(true) => {}
                Some(false) => return Err(SuspensionError::NotSupported),
                None => {
                    return Err(SuspensionError::Failed(
                        "Could not verify the existing suspension job.".to_owned(),
                    ));
                }
            }
        } else {
            if let Err(error) =
                windows_suspension::assign_process(&job_handle, &process_handle, identity.id)
            {
                if windows_suspension::process_is_in_job(&process_handle, None) == Some(true) {
                    return Err(SuspensionError::NotSupported);
                }
                return Err(map_job_object_error(error));
            }
        }

        Ok((
            identity,
            WindowsSuspensionHandle {
                job_handle: Some(job_handle),
                job_name: Some(job_name),
                process_handle: Some(process_handle),
            },
        ))
    }

    fn begin_freeze(
        &mut self,
        _identity: &ProcessIdentity,
        handle: &Self::Handle,
    ) -> Result<Self::Intent, SuspensionError> {
        let job_name = handle
            .job_name
            .as_deref()
            .ok_or(SuspensionError::ProcessExited)?;
        let process_handle = handle
            .process_handle
            .as_ref()
            .ok_or(SuspensionError::ProcessExited)?;
        crash_recovery::record_suspended_job(job_name, process_handle.raw())
            .map_err(SuspensionError::Failed)
    }

    fn set_frozen(&mut self, handle: &Self::Handle, frozen: bool) -> Result<(), SuspensionError> {
        let Some(job_handle) = &handle.job_handle else {
            #[cfg(test)]
            return Ok(());
            #[cfg(not(test))]
            return Err(SuspensionError::ProcessExited);
        };
        windows_suspension::set_frozen(job_handle, frozen).map_err(map_job_object_error)
    }

    fn commit_freeze(&mut self, intent: Self::Intent) -> Result<(), SuspensionError> {
        intent.commit().map_err(SuspensionError::Failed)
    }

    fn forget(&mut self, handle: &Self::Handle) -> Result<(), SuspensionError> {
        let Some(job_name) = &handle.job_name else {
            #[cfg(test)]
            return Ok(());
            #[cfg(not(test))]
            return Err(SuspensionError::ProcessExited);
        };
        crash_recovery::forget_suspended_job(job_name).map_err(SuspensionError::Failed)
    }
}

#[cfg(test)]
impl SuspensionController<WindowsSuspensionPlatform> {
    pub(crate) fn insert_inert(&mut self, target: SuspensionTarget, frozen: bool) {
        let identity = ProcessIdentity::new(
            target.process.id,
            target.process.name.clone(),
            target.process.executable_path.clone(),
            target.process.creation_time,
            Some(1),
        );
        self.managed.insert(
            target.key(),
            ManagedSuspension {
                identity,
                handle: WindowsSuspensionHandle {
                    job_handle: None,
                    job_name: None,
                    process_handle: None,
                },
                state: if frozen {
                    SuspensionState::Frozen
                } else {
                    SuspensionState::Thawed
                },
                retry: None,
            },
        );
    }

    pub(crate) fn insert_inert_compensation_pending(&mut self, target: SuspensionTarget) {
        let key = target.key();
        self.insert_inert(target, true);
        if let Some(managed) = self.managed.get_mut(&key) {
            managed.state = SuspensionState::FrozenCompensationPending;
            managed.retry = Some(RetryState {
                next_attempt: Instant::now(),
                delay: RETRY_INITIAL,
                message: "test compensation pending".to_owned(),
            });
        }
    }
}

fn map_process_control_error(error: ProcessControlError) -> SuspensionError {
    match error {
        ProcessControlError::AccessDenied(_) => SuspensionError::AccessDenied,
        ProcessControlError::ProcessExited => SuspensionError::ProcessExited,
        ProcessControlError::Unavailable(_) | ProcessControlError::Failed(_) => {
            SuspensionError::Failed(error.to_string())
        }
    }
}

fn map_job_object_error(error: JobObjectError) -> SuspensionError {
    match error {
        JobObjectError::AccessDenied => SuspensionError::AccessDenied,
        JobObjectError::ProcessExited => SuspensionError::ProcessExited,
        JobObjectError::NotSupported => SuspensionError::NotSupported,
        JobObjectError::Unsupported => SuspensionError::Unsupported,
        JobObjectError::Failed(message) => SuspensionError::Failed(message),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::VecDeque,
        process::{Child, Command, Stdio},
        rc::Rc,
    };

    use super::*;

    #[derive(Default)]
    struct FakeState {
        allow_cross_session: Vec<bool>,
        freeze_calls: Vec<bool>,
        forget_calls: usize,
        begin_results: VecDeque<Result<(), SuspensionError>>,
        freeze_results: VecDeque<Result<(), SuspensionError>>,
        commit_results: VecDeque<Result<(), SuspensionError>>,
        forget_results: VecDeque<Result<(), SuspensionError>>,
    }

    #[derive(Clone)]
    struct FakePlatform(Rc<RefCell<FakeState>>);

    impl SuspensionPlatform for FakePlatform {
        type Handle = u32;
        type Intent = ();

        fn assign(
            &mut self,
            target: &SuspensionTarget,
            allow_cross_session_process_control: bool,
        ) -> Result<(ProcessIdentity, Self::Handle), SuspensionError> {
            self.0
                .borrow_mut()
                .allow_cross_session
                .push(allow_cross_session_process_control);
            Ok((
                ProcessIdentity::new(
                    target.process.id,
                    target.process.name.clone(),
                    target.process.executable_path.clone(),
                    target.process.creation_time,
                    Some(1),
                ),
                target.process.id,
            ))
        }

        fn begin_freeze(
            &mut self,
            _identity: &ProcessIdentity,
            _handle: &Self::Handle,
        ) -> Result<Self::Intent, SuspensionError> {
            self.0
                .borrow_mut()
                .begin_results
                .pop_front()
                .unwrap_or(Ok(()))
        }

        fn set_frozen(
            &mut self,
            _handle: &Self::Handle,
            frozen: bool,
        ) -> Result<(), SuspensionError> {
            let mut state = self.0.borrow_mut();
            state.freeze_calls.push(frozen);
            state.freeze_results.pop_front().unwrap_or(Ok(()))
        }

        fn commit_freeze(&mut self, _intent: Self::Intent) -> Result<(), SuspensionError> {
            self.0
                .borrow_mut()
                .commit_results
                .pop_front()
                .unwrap_or(Ok(()))
        }

        fn forget(&mut self, _handle: &Self::Handle) -> Result<(), SuspensionError> {
            let mut state = self.0.borrow_mut();
            state.forget_calls += 1;
            state.forget_results.pop_front().unwrap_or(Ok(()))
        }
    }

    fn target(creation_time: u64) -> SuspensionTarget {
        SuspensionTarget::automatic(
            42,
            "app.exe".to_owned(),
            PathBuf::from(r"C:\Apps\app.exe"),
            creation_time,
            Some(false),
        )
    }

    fn controller() -> (SuspensionController<FakePlatform>, Rc<RefCell<FakeState>>) {
        let state = Rc::new(RefCell::new(FakeState::default()));
        (
            SuspensionController::new(FakePlatform(Rc::clone(&state))),
            state,
        )
    }

    #[test]
    fn freeze_propagates_cross_session_policy_and_is_idempotent() {
        let (mut controller, state) = controller();

        assert_eq!(
            controller.freeze(&target(7), false).unwrap(),
            SuspensionFreezeOutcome::Frozen
        );
        assert_eq!(
            controller.freeze(&target(7), true).unwrap(),
            SuspensionFreezeOutcome::Unchanged
        );
        assert_eq!(state.borrow().allow_cross_session, vec![false]);
        assert_eq!(state.borrow().freeze_calls, vec![true]);
    }

    #[test]
    fn failed_recovery_begin_releases_a_new_thawed_job() {
        let (mut controller, state) = controller();
        state
            .borrow_mut()
            .begin_results
            .push_back(Err(SuspensionError::Failed("begin failed".to_owned())));

        assert!(controller.freeze(&target(7), true).is_err());
        assert!(!controller.contains_process(42));
        assert!(state.borrow().freeze_calls.is_empty());
    }

    #[test]
    fn failed_initial_freeze_releases_a_new_job() {
        let (mut controller, state) = controller();
        state
            .borrow_mut()
            .freeze_results
            .push_back(Err(SuspensionError::Failed("freeze failed".to_owned())));

        assert!(controller.freeze(&target(7), true).is_err());
        assert!(!controller.contains_process(42));
        assert_eq!(state.borrow().freeze_calls, vec![true]);
    }

    #[test]
    fn commit_failure_is_compensated_before_returning() {
        let (mut controller, state) = controller();
        state
            .borrow_mut()
            .commit_results
            .push_back(Err(SuspensionError::Failed("commit failed".to_owned())));

        assert!(controller.freeze(&target(7), true).is_err());
        assert_eq!(state.borrow().freeze_calls, vec![true, false]);
        assert_eq!(state.borrow().forget_calls, 1);
        assert!(!controller.contains_process(42));
    }

    #[test]
    fn failed_commit_compensation_is_retried_without_another_freeze_request() {
        let (mut controller, state) = controller();
        {
            let mut state = state.borrow_mut();
            state
                .commit_results
                .push_back(Err(SuspensionError::Failed("commit failed".to_owned())));
            state.freeze_results.push_back(Ok(()));
            state
                .freeze_results
                .push_back(Err(SuspensionError::Failed("thaw failed".to_owned())));
        }

        assert!(controller.freeze(&target(7), true).is_err());
        assert!(controller.contains_process(42));
        assert!(controller.is_frozen_process(42));
        assert_eq!(state.borrow().freeze_calls, vec![true, false]);

        assert!(matches!(
            controller.freeze(&target(7), true),
            Err(SuspensionError::RetryPending(_))
        ));
        assert_eq!(state.borrow().freeze_calls, vec![true, false]);

        controller.reconcile_pending(true);
        assert_eq!(state.borrow().freeze_calls, vec![true, false, false]);
        assert!(!controller.contains_process(42));
    }

    #[test]
    fn failed_freeze_on_a_reusable_thawed_job_does_not_leave_managed_state() {
        let (mut controller, state) = controller();
        controller.freeze(&target(7), true).unwrap();
        controller.thaw_keep_process(42, true).unwrap();
        state
            .borrow_mut()
            .begin_results
            .push_back(Err(SuspensionError::Failed("begin failed".to_owned())));

        assert!(controller.freeze(&target(7), true).is_err());
        assert!(!controller.contains_process(42));
    }

    #[test]
    fn failed_set_on_a_reusable_thawed_job_does_not_leave_managed_state() {
        let (mut controller, state) = controller();
        controller.freeze(&target(7), true).unwrap();
        controller.thaw_keep_process(42, true).unwrap();
        state
            .borrow_mut()
            .freeze_results
            .push_back(Err(SuspensionError::Failed("freeze failed".to_owned())));

        assert!(controller.freeze(&target(7), true).is_err());
        assert!(!controller.contains_process(42));
    }

    #[test]
    fn pending_forget_is_retried_without_another_thaw() {
        let (mut controller, state) = controller();
        controller.freeze(&target(7), true).unwrap();
        state
            .borrow_mut()
            .forget_results
            .push_back(Err(SuspensionError::Failed("forget failed".to_owned())));

        assert!(controller.thaw_keep_process(42, true).is_err());
        let calls_after_failure = state.borrow().freeze_calls.clone();

        controller.reconcile_pending(true);
        assert_eq!(state.borrow().freeze_calls, calls_after_failure);
        assert!(controller.contains_process(42));
        assert!(!controller.is_frozen_process(42));
    }

    #[test]
    fn failed_thaw_retains_frozen_state_for_retry() {
        let (mut controller, state) = controller();
        controller.freeze(&target(7), true).unwrap();
        state
            .borrow_mut()
            .freeze_results
            .push_back(Err(SuspensionError::Failed("thaw failed".to_owned())));

        assert!(controller.release_process(42, true).is_err());
        assert!(controller.contains_process(42));
        assert!(controller.is_frozen_process(42));

        controller.release_process(42, true).unwrap();
        assert!(!controller.contains_process(42));
    }

    #[test]
    fn forget_failure_retries_cleanup_without_refreezing_or_rethawing() {
        let (mut controller, state) = controller();
        controller.freeze(&target(7), true).unwrap();
        state
            .borrow_mut()
            .forget_results
            .push_back(Err(SuspensionError::Failed("forget failed".to_owned())));

        assert!(controller.release_process(42, true).is_err());
        assert!(!controller.is_frozen_process(42));
        let calls_after_failure = state.borrow().freeze_calls.clone();

        controller.release_process(42, true).unwrap();
        assert_eq!(state.borrow().freeze_calls, calls_after_failure);
        assert!(!controller.contains_process(42));
    }

    #[test]
    fn pid_reuse_releases_the_old_job_before_assigning_the_new_instance() {
        let (mut controller, state) = controller();
        controller.freeze(&target(7), true).unwrap();

        controller.freeze(&target(8), true).unwrap();

        assert_eq!(state.borrow().allow_cross_session, vec![true, true]);
        assert_eq!(state.borrow().freeze_calls, vec![true, false, true]);
        assert!(controller.matches_target(&target(8)));
        assert!(!controller.matches_target(&target(7)));
    }

    #[test]
    fn exact_target_thaw_rejects_a_reused_pid() {
        let (mut controller, state) = controller();
        controller.freeze(&target(7), true).unwrap();

        assert_eq!(
            controller.thaw_keep_target(&target(8), true),
            Err(SuspensionError::ProcessExited)
        );
        assert!(controller.is_frozen_process(42));
        assert_eq!(state.borrow().freeze_calls, vec![true]);
    }

    #[test]
    fn shutdown_is_idempotent() {
        let (mut controller, state) = controller();
        controller.freeze(&target(7), true).unwrap();

        controller.shutdown().unwrap();
        controller.shutdown().unwrap();

        assert_eq!(state.borrow().freeze_calls, vec![true, false]);
        assert_eq!(state.borrow().forget_calls, 1);
    }

    #[test]
    #[ignore = "uses the undocumented Job Object freeze contract; run in explicit Windows integration QA"]
    fn released_live_process_can_reuse_its_exact_named_job() -> Result<(), String> {
        struct DisposableChild(Child);

        impl Drop for DisposableChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let system_root = std::env::var_os("SystemRoot")
            .ok_or_else(|| "SystemRoot is unavailable.".to_owned())?;
        let executable = Path::new(&system_root).join("System32").join("ping.exe");
        let child = Command::new(&executable)
            .args(["127.0.0.1", "-n", "120", "-w", "1000"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Failed to start disposable test process: {error}"))?;
        let child = DisposableChild(child);
        let action_target =
            crate::foreground::capture_process_action_target(child.0.id(), &executable, true)
                .map_err(|error| error.to_string())?;
        let target = SuspensionTarget::from_action_target(&action_target);
        let mut controller = SuspensionController::default();

        controller
            .freeze(&target, true)
            .map_err(|error| error.to_string())?;
        controller
            .release_process(action_target.id, true)
            .map_err(|error| error.to_string())?;
        controller
            .freeze(&target, true)
            .map_err(|error| error.to_string())?;
        controller.shutdown()?;
        Ok(())
    }
}
