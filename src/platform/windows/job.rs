use std::ptr::null_mut;

use windows_sys::Win32::{
    Foundation::{
        SetLastError, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_INVALID_PARAMETER,
        ERROR_NOT_SUPPORTED, HANDLE,
    },
    System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob},
};

#[cfg(test)]
use windows_sys::Win32::System::JobObjects::{
    JobObjectBasicUIRestrictions, SetInformationJobObject, JOBOBJECT_BASIC_UI_RESTRICTIONS,
    JOB_OBJECT_UILIMIT_HANDLES,
};

use crate::win_util::{last_error, wide_null, WinHandle};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JobObjectError {
    AccessDenied,
    ProcessExited,
    NotSupported,
    Unsupported,
    Failed(String),
}

pub(crate) struct JobHandle(WinHandle);

impl JobHandle {
    pub(crate) fn raw(&self) -> HANDLE {
        self.0.raw()
    }
}

pub(crate) struct CreatedJob {
    pub(crate) handle: JobHandle,
    pub(crate) already_existed: bool,
}

pub(crate) fn create_job(name: &str) -> Result<CreatedJob, JobObjectError> {
    let wide_name = wide_null(name);
    // SAFETY: SetLastError updates only this thread's Win32 error slot so the documented
    // ERROR_ALREADY_EXISTS success signal cannot be confused with a stale prior error.
    unsafe { SetLastError(0) };
    // SAFETY: null security attributes request defaults, wide_name is terminated UTF-16, and the
    // returned handle is owned here.
    let handle = unsafe { CreateJobObjectW(null_mut(), wide_name.as_ptr()) };
    let error = last_error();
    if handle.is_null() {
        return Err(JobObjectError::Failed(format!(
            "CreateJobObjectW failed with error {error}."
        )));
    }
    Ok(CreatedJob {
        handle: JobHandle(WinHandle::new(handle)),
        already_existed: error == ERROR_ALREADY_EXISTS,
    })
}

pub(crate) fn assign_process(
    job: &JobHandle,
    process: &WinHandle,
    process_id: u32,
) -> Result<(), JobObjectError> {
    // SAFETY: both handles are live and assignment retains no borrowed Rust pointer.
    let ok = unsafe { AssignProcessToJobObject(job.raw(), process.raw()) };
    if ok == 0 {
        Err(assign_error(process_id, last_error()))
    } else {
        Ok(())
    }
}

pub(crate) fn process_is_in_job(process: &WinHandle, job: Option<&JobHandle>) -> Option<bool> {
    let job_handle = job.map_or(null_mut(), JobHandle::raw);
    let mut in_job = 0;
    // SAFETY: process is live; job_handle is either a live job or null to ask about any job, and
    // in_job is writable.
    let ok = unsafe { IsProcessInJob(process.raw(), job_handle, &mut in_job) };
    (ok != 0).then_some(in_job != 0)
}

#[cfg(test)]
pub(crate) fn set_ui_restriction_for_test(job: &JobHandle) -> Result<(), JobObjectError> {
    let restriction = JOBOBJECT_BASIC_UI_RESTRICTIONS {
        UIRestrictionsClass: JOB_OBJECT_UILIMIT_HANDLES,
    };
    // SAFETY: job is live and restriction is a readable buffer with the exact information-class
    // layout. This test-only restriction makes a disposable foreign job incompatible with nesting.
    let ok = unsafe {
        SetInformationJobObject(
            job.raw(),
            JobObjectBasicUIRestrictions,
            (&restriction as *const JOBOBJECT_BASIC_UI_RESTRICTIONS).cast(),
            std::mem::size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
        )
    };
    if ok == 0 {
        Err(JobObjectError::Failed(format!(
            "Foreign job UI restriction failed with error {}.",
            last_error()
        )))
    } else {
        Ok(())
    }
}

fn assign_error(process_id: u32, error: u32) -> JobObjectError {
    match error {
        ERROR_ACCESS_DENIED => JobObjectError::AccessDenied,
        ERROR_INVALID_PARAMETER => JobObjectError::ProcessExited,
        ERROR_NOT_SUPPORTED => JobObjectError::NotSupported,
        _ => JobObjectError::Failed(format!(
            "AssignProcessToJobObject({process_id}) failed with error {error}."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignment_errors_have_stable_adapter_classification() {
        assert_eq!(
            assign_error(42, ERROR_ACCESS_DENIED),
            JobObjectError::AccessDenied
        );
        assert_eq!(
            assign_error(42, ERROR_INVALID_PARAMETER),
            JobObjectError::ProcessExited
        );
        assert_eq!(
            assign_error(42, ERROR_NOT_SUPPORTED),
            JobObjectError::NotSupported
        );
    }
}
