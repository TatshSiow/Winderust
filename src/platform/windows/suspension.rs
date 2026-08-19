use std::{ffi::c_void, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{
        SetLastError, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_INVALID_PARAMETER,
        ERROR_NOT_SUPPORTED,
    },
    System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, SetInformationJobObject,
    },
};

use crate::win_util::{last_error, wide_null, WinHandle};

pub(crate) const JOB_OBJECT_FREEZE_INFORMATION_CLASS: i32 = 18;
const JOB_OBJECT_FREEZE_OPERATION: u32 = 1;

#[repr(C)]
pub(crate) struct JobObjectFreezeInformation {
    pub(crate) flags: u32,
    pub(crate) freeze: u8,
    pub(crate) swap: u8,
    pub(crate) spare: u16,
    pub(crate) wake_filter_high: u32,
    pub(crate) wake_filter_low: u32,
}

impl JobObjectFreezeInformation {
    pub(crate) fn new(frozen: bool) -> Self {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JobObjectError {
    AccessDenied,
    ProcessExited,
    NotSupported,
    Unsupported,
    Failed(String),
}

pub(crate) struct JobHandle(WinHandle);

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
    let ok = unsafe { AssignProcessToJobObject(job.0.raw(), process.raw()) };
    if ok == 0 {
        Err(assign_error(process_id, last_error()))
    } else {
        Ok(())
    }
}

pub(crate) fn process_is_in_job(process: &WinHandle, job: Option<&JobHandle>) -> Option<bool> {
    let job_handle = job.map_or(null_mut(), |job| job.0.raw());
    let mut in_job = 0;
    // SAFETY: process is live; job_handle is either a live job or null to ask about any job, and
    // in_job is writable.
    let ok = unsafe { IsProcessInJob(process.raw(), job_handle, &mut in_job) };
    (ok != 0).then_some(in_job != 0)
}

pub(crate) fn set_frozen(job: &JobHandle, frozen: bool) -> Result<(), JobObjectError> {
    let mut info = JobObjectFreezeInformation::new(frozen);
    // SAFETY: job is live and info is writable for exactly the supplied structure size.
    let ok = unsafe {
        SetInformationJobObject(
            job.0.raw(),
            JOB_OBJECT_FREEZE_INFORMATION_CLASS,
            (&mut info as *mut JobObjectFreezeInformation).cast::<c_void>(),
            std::mem::size_of::<JobObjectFreezeInformation>() as u32,
        )
    };
    if ok == 0 {
        Err(freeze_error(frozen, last_error()))
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

fn freeze_error(frozen: bool, error: u32) -> JobObjectError {
    match error {
        ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED => JobObjectError::Unsupported,
        _ => JobObjectError::Failed(format!(
            "SetInformationJobObject freeze={frozen} failed with error {error}."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeze_information_layout_matches_the_windows_contract() {
        assert_eq!(std::mem::size_of::<JobObjectFreezeInformation>(), 16);
        assert_eq!(std::mem::align_of::<JobObjectFreezeInformation>(), 4);

        let frozen = JobObjectFreezeInformation::new(true);
        assert_eq!(frozen.flags, JOB_OBJECT_FREEZE_OPERATION);
        assert_eq!(frozen.freeze, 1);
        assert_eq!(frozen.swap, 0);
        assert_eq!(frozen.spare, 0);
        assert_eq!(frozen.wake_filter_high, 0);
        assert_eq!(frozen.wake_filter_low, 0);
        assert_eq!(JobObjectFreezeInformation::new(false).freeze, 0);
    }

    #[test]
    fn win32_errors_have_stable_adapter_classification() {
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
        assert_eq!(
            freeze_error(true, ERROR_INVALID_PARAMETER),
            JobObjectError::Unsupported
        );
        assert_eq!(
            freeze_error(false, ERROR_NOT_SUPPORTED),
            JobObjectError::Unsupported
        );
    }
}
