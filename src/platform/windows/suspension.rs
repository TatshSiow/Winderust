use std::ffi::c_void;

use windows_sys::Win32::{
    Foundation::{ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED},
    System::JobObjects::SetInformationJobObject,
};

use crate::{platform::windows::job::JobHandle, win_util::last_error};

use super::job::JobObjectError;

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

pub(crate) fn set_frozen(job: &JobHandle, frozen: bool) -> Result<(), JobObjectError> {
    let mut info = JobObjectFreezeInformation::new(frozen);
    // SAFETY: job is live and info is writable for exactly the supplied structure size.
    let ok = unsafe {
        SetInformationJobObject(
            job.raw(),
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
    fn freeze_errors_have_stable_adapter_classification() {
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
