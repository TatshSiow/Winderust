use super::*;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SuspensionError {
    AccessDenied,
    ProcessExited,
    NotSupported,
    Unsupported,
    Failed(String),
}

pub(super) fn suspension_error_message(error: SuspensionError) -> String {
    match error {
        SuspensionError::AccessDenied => "Access denied.".to_owned(),
        SuspensionError::ProcessExited => "Process exited.".to_owned(),
        SuspensionError::NotSupported => "Operation not supported for this process.".to_owned(),
        SuspensionError::Unsupported => "Windows Job Object freeze is unsupported.".to_owned(),
        SuspensionError::Failed(message) => message,
    }
}

const JOB_OBJECT_FREEZE_INFORMATION_CLASS: i32 = 18;
pub(super) const JOB_OBJECT_FREEZE_OPERATION: u32 = 1;

#[repr(C)]
pub(super) struct JobObjectFreezeInformation {
    pub(super) flags: u32,
    pub(super) freeze: u8,
    pub(super) swap: u8,
    pub(super) spare: u16,
    pub(super) wake_filter_high: u32,
    pub(super) wake_filter_low: u32,
}

impl JobObjectFreezeInformation {
    pub(super) fn new(frozen: bool) -> Self {
        Self {
            flags: JOB_OBJECT_FREEZE_OPERATION,
            freeze: u8::from(frozen),
            swap: 0,
            spare: 0,
            wake_filter_high: 0,
            wake_filter_low: 0,
        }
    }
}

pub(super) struct ProcessFreezer {
    pub(super) job_handle: Option<WinHandle>,
    pub(super) process_handle: Option<WinHandle>,
    pub(super) process_creation_time: Option<u64>,
    pub(super) can_wait_for_process: bool,
}

impl ProcessFreezer {
    pub(super) fn assign(process_id: u32) -> Result<Self, SuspensionError> {
        let (process_handle, can_wait_for_process) = open_process_for_job_assignment(process_id)?;

        // SAFETY: Null security attributes and name request a private job object owned by the
        // returned handle.
        let job_handle = unsafe { CreateJobObjectW(null(), null()) };
        if job_handle.is_null() {
            let error = last_error();
            return Err(SuspensionError::Failed(format!(
                "CreateJobObjectW failed with error {error}."
            )));
        }
        let job_handle = WinHandle::new(job_handle);

        // SAFETY: both handles are live and owned by this function; assignment does not retain
        // borrowed Rust pointers.
        let assigned =
            unsafe { AssignProcessToJobObject(job_handle.raw(), process_handle.raw()) != 0 };
        if !assigned {
            let error = last_error();
            let assignment_error =
                assign_process_to_job_error_with_context(process_id, process_handle.raw(), error);
            return Err(assignment_error);
        }

        Ok(Self {
            job_handle: Some(job_handle),
            process_creation_time: process_handle.process_creation_time(),
            process_handle: Some(process_handle),
            can_wait_for_process,
        })
    }

    pub(super) fn set_frozen(&self, frozen: bool) -> Result<(), SuspensionError> {
        let mut info = JobObjectFreezeInformation::new(frozen);
        let Some(job_handle) = &self.job_handle else {
            return Ok(());
        };

        // SAFETY: job_handle is live and info is writable for exactly the supplied structure size.
        let ok = unsafe {
            SetInformationJobObject(
                job_handle.raw(),
                JOB_OBJECT_FREEZE_INFORMATION_CLASS,
                &mut info as *mut _ as *mut c_void,
                std::mem::size_of::<JobObjectFreezeInformation>() as u32,
            )
        };

        if ok == 0 {
            Err(job_freeze_error(frozen, last_error()))
        } else {
            Ok(())
        }
    }

    pub(super) fn is_process_alive(&self) -> bool {
        !self.can_wait_for_process
            || self.process_handle.as_ref().is_some_and(|process_handle| {
                // SAFETY: can_wait_for_process is true only when this live handle was opened
                // with PROCESS_SYNCHRONIZE.
                unsafe { WaitForSingleObject(process_handle.raw(), 0) == WAIT_TIMEOUT }
            })
    }

    pub(super) fn matches_process_id(&self, process_id: u32) -> bool {
        self.is_process_alive()
            && (self.can_wait_for_process
                || process_creation_time_matches(
                    self.process_creation_time,
                    current_process_creation_time(process_id),
                ))
    }

    pub(super) fn close(&mut self) {
        self.job_handle = None;
        self.process_handle = None;
    }
}

impl Drop for ProcessFreezer {
    fn drop(&mut self) {
        if self.job_handle.is_some() {
            let _ = self.set_frozen(false);
        }
        self.close();
    }
}

pub(super) fn null_mut_handle() -> HANDLE {
    std::ptr::null_mut()
}

pub(super) fn open_process_for_job_assignment(
    process_id: u32,
) -> Result<(WinHandle, bool), SuspensionError> {
    let access_masks = [
        PROCESS_SET_QUOTA
            | PROCESS_TERMINATE
            | PROCESS_SYNCHRONIZE
            | PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SET_QUOTA | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SET_QUOTA | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
        PROCESS_SET_QUOTA | PROCESS_TERMINATE,
    ];

    let mut last_open_error = 0;
    for access in access_masks {
        // SAFETY: process_id came from the current process snapshot, access is one of the
        // documented masks above, and no inherited handle is requested.
        let handle = unsafe { OpenProcess(access, 0, process_id) };
        if !handle.is_null() {
            return Ok((WinHandle::new(handle), access & PROCESS_SYNCHRONIZE != 0));
        }
        last_open_error = last_error();
    }

    Err(open_process_error(process_id, last_open_error))
}

pub(super) fn current_process_creation_time(process_id: u32) -> Option<u64> {
    // SAFETY: process_id came from the current process snapshot and no inherited handle is
    // requested.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return None;
    }
    WinHandle::new(handle).process_creation_time()
}
pub(super) fn process_creation_time_matches(recorded: Option<u64>, current: Option<u64>) -> bool {
    match recorded {
        Some(recorded) => current == Some(recorded),
        None => true,
    }
}

pub(super) fn open_process_error(process_id: u32, error: u32) -> SuspensionError {
    match error {
        ERROR_ACCESS_DENIED => SuspensionError::AccessDenied,
        ERROR_INVALID_PARAMETER => SuspensionError::ProcessExited,
        ERROR_NOT_SUPPORTED => SuspensionError::NotSupported,
        _ => SuspensionError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

pub(super) fn assign_process_to_job_error(process_id: u32, error: u32) -> SuspensionError {
    match error {
        ERROR_ACCESS_DENIED => SuspensionError::AccessDenied,
        ERROR_NOT_SUPPORTED => SuspensionError::NotSupported,
        _ => SuspensionError::Failed(format!(
            "AssignProcessToJobObject({process_id}) failed with error {error}."
        )),
    }
}

pub(super) fn assign_process_to_job_error_with_context(
    process_id: u32,
    process_handle: HANDLE,
    error: u32,
) -> SuspensionError {
    if process_is_in_job(process_handle) == Some(true) {
        return SuspensionError::Failed(format!(
            "AssignProcessToJobObject({process_id}) failed with error {error}; process is already in a job object."
        ));
    }

    assign_process_to_job_error(process_id, error)
}

pub(super) fn process_is_in_job(process_handle: HANDLE) -> Option<bool> {
    let mut in_job = 0;
    // SAFETY: process_handle is live, a null job asks about any job, and in_job is writable.
    let ok = unsafe { IsProcessInJob(process_handle, null_mut_handle(), &mut in_job) };
    (ok != 0).then_some(in_job != 0)
}

pub(super) fn job_freeze_error(frozen: bool, error: u32) -> SuspensionError {
    match error {
        ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED => SuspensionError::Unsupported,
        _ => SuspensionError::Failed(format!(
            "SetInformationJobObject freeze={frozen} failed with error {error}."
        )),
    }
}
