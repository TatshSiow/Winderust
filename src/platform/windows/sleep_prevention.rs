use windows_sys::Win32::{
    Foundation::INVALID_HANDLE_VALUE,
    System::{
        Power::{
            PowerClearRequest, PowerCreateRequest, PowerRequestDisplayRequired,
            PowerRequestSystemRequired, PowerSetRequest, POWER_REQUEST_TYPE,
        },
        SystemServices::POWER_REQUEST_CONTEXT_VERSION,
        Threading::{POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0},
    },
};

use crate::win_util::{last_error, wide_null, WinHandle};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{operation} failed with Windows error {code}.")]
pub(crate) struct SleepPreventionError {
    pub(crate) operation: &'static str,
    pub(crate) code: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestKind {
    System,
    Display,
}

impl RequestKind {
    fn native(self) -> POWER_REQUEST_TYPE {
        match self {
            Self::System => PowerRequestSystemRequired,
            Self::Display => PowerRequestDisplayRequired,
        }
    }
}

pub(crate) trait SleepPreventionPlatform {
    type Handle;

    fn create(&self, reason: &str) -> Result<Self::Handle, SleepPreventionError>;
    fn set(&self, handle: &Self::Handle, kind: RequestKind) -> Result<(), SleepPreventionError>;
    fn clear(&self, handle: &Self::Handle, kind: RequestKind) -> Result<(), SleepPreventionError>;
}

#[derive(Default)]
pub(crate) struct WindowsSleepPreventionPlatform;

impl SleepPreventionPlatform for WindowsSleepPreventionPlatform {
    type Handle = WinHandle;

    fn create(&self, reason: &str) -> Result<Self::Handle, SleepPreventionError> {
        let mut reason = wide_null(reason);
        let context = REASON_CONTEXT {
            Version: POWER_REQUEST_CONTEXT_VERSION,
            Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
            Reason: REASON_CONTEXT_0 {
                SimpleReasonString: reason.as_mut_ptr(),
            },
        };
        // SAFETY: context selects a NUL-terminated UTF-16 string kept alive for
        // this synchronous call; a successful handle is uniquely owned below.
        let handle = unsafe { PowerCreateRequest(&context) };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            Err(SleepPreventionError {
                operation: "PowerCreateRequest",
                code: last_error(),
            })
        } else {
            Ok(WinHandle::new(handle))
        }
    }

    fn set(&self, handle: &Self::Handle, kind: RequestKind) -> Result<(), SleepPreventionError> {
        // SAFETY: handle owns a live power-request object and kind is one of the
        // documented system/display request types.
        if unsafe { PowerSetRequest(handle.raw(), kind.native()) } == 0 {
            Err(SleepPreventionError {
                operation: "PowerSetRequest",
                code: last_error(),
            })
        } else {
            Ok(())
        }
    }

    fn clear(&self, handle: &Self::Handle, kind: RequestKind) -> Result<(), SleepPreventionError> {
        // SAFETY: handle owns a live power-request object; the controller clears
        // only a type for which Windows previously accepted its matching set.
        if unsafe { PowerClearRequest(handle.raw(), kind.native()) } == 0 {
            Err(SleepPreventionError {
                operation: "PowerClearRequest",
                code: last_error(),
            })
        } else {
            Ok(())
        }
    }
}
