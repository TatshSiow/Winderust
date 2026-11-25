use windows_sys::Win32::{
    Foundation::ERROR_ACCESS_DENIED,
    Globalization::{CompareStringOrdinal, CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN},
    Security::{GetLengthSid, IsValidSid, PSID, SID},
};

use crate::win_util::last_error;

pub(crate) fn sid_bytes(sid: PSID, storage: &[usize]) -> Result<&[u8], &'static str> {
    let start = storage.as_ptr() as usize;
    let end = start + std::mem::size_of_val(storage);
    let address = sid as usize;
    if !address.is_multiple_of(std::mem::align_of::<SID>())
        || address < start
        || address.checked_add(8).is_none_or(|value| value > end)
    {
        return Err("Windows returned an invalid token SID.");
    }
    // SAFETY: the verified offset is in bounds and the pointer derives from retained storage.
    let data = unsafe { storage.as_ptr().cast::<u8>().add(address - start) };
    // SAFETY: the SID header lies inside the retained native response.
    let count = unsafe { *data.add(1) } as usize;
    if address
        .checked_add(8 + count * 4)
        .is_none_or(|value| value > end)
    {
        return Err("Windows returned a truncated token SID.");
    }
    let sid = data.cast_mut().cast();
    // SAFETY: the complete, aligned SID lies in the retained allocation. The call only reads it.
    if unsafe { IsValidSid(sid) } == 0 {
        return Err("Windows returned an invalid token SID.");
    }
    // SAFETY: the SID is valid and its complete storage remains alive.
    let bytes = unsafe { GetLengthSid(sid) } as usize;
    // SAFETY: GetLengthSid's validated SID bytes lie within storage.
    Ok(unsafe { std::slice::from_raw_parts(data, bytes) })
}

pub(crate) fn ordinal_equal(left: &[u16], right: &[u16]) -> Result<bool, String> {
    let left_len = i32::try_from(left.len()).map_err(|_| "The path is too long.")?;
    let right_len = i32::try_from(right.len()).map_err(|_| "The path is too long.")?;
    // SAFETY: both UTF-16 slices remain readable for their explicit lengths during the call.
    match unsafe { CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) } {
        CSTR_EQUAL => Ok(true),
        CSTR_LESS_THAN | CSTR_GREATER_THAN => Ok(false),
        _ => Err(format!(
            "CompareStringOrdinal failed (Windows error {}).",
            last_error()
        )),
    }
}

pub(crate) mod appearance;
pub(crate) mod cpu_allocation;
pub(crate) mod cpu_limiter;
pub(crate) mod dynamic_priority_boost;
pub(crate) mod gpu_memory;
pub(crate) mod gpu_priority;
pub(crate) mod gpu_usage;
pub(crate) mod input_idle;
pub(crate) mod instance_limits;
pub(crate) mod interrupt_timing;
pub(crate) mod io_priority;
pub(crate) mod job;
pub(crate) mod keep_running;
pub(crate) mod memory_priority;
pub(crate) mod memory_trim;
pub(crate) mod menu_theme;
pub(crate) mod power_plan;
pub(crate) mod priority_efficiency;
pub(crate) mod process;
pub(crate) mod process_restart;
pub(crate) mod process_termination;
#[cfg(feature = "runtime-benchmark")]
pub(crate) mod runtime_benchmark;
pub(crate) mod self_power;
pub(crate) mod sleep_prevention;
pub(crate) mod suspension;
pub(crate) mod thread_priority;
pub(crate) mod thread_suspension;
pub(crate) mod timer_resolution;
pub(crate) mod window_dpi;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessOperationError {
    AccessDenied { operation: &'static str },
    Failed { operation: &'static str, code: u32 },
}

impl ProcessOperationError {
    fn capture(operation: &'static str) -> Self {
        Self::from_code(operation, last_error())
    }

    fn from_code(operation: &'static str, code: u32) -> Self {
        match code {
            ERROR_ACCESS_DENIED => Self::AccessDenied { operation },
            code => Self::Failed { operation, code },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER;

    #[test]
    fn sid_bytes_require_complete_aligned_storage() {
        let local_system = [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];
        let mut storage = [0usize; 3];
        let data = storage.as_mut_ptr().cast::<u8>();
        // SAFETY: the aligned allocation contains at least 12 writable bytes on Windows.
        unsafe { std::ptr::copy_nonoverlapping(local_system.as_ptr(), data, local_system.len()) };
        assert_eq!(
            sid_bytes(data.cast(), &storage),
            Ok(local_system.as_slice())
        );
        assert_eq!(
            sid_bytes(data.wrapping_add(1).cast(), &storage),
            Err("Windows returned an invalid token SID.")
        );
        // SAFETY: the count field is inside the allocation and has no active borrowed view.
        unsafe { *data.add(1) = u8::MAX };
        assert_eq!(
            sid_bytes(data.cast(), &storage),
            Err("Windows returned a truncated token SID.")
        );
    }

    #[test]
    fn invalid_retained_handle_operation_keeps_the_native_error() {
        assert_eq!(
            ProcessOperationError::from_code("retained operation", ERROR_INVALID_PARAMETER),
            ProcessOperationError::Failed {
                operation: "retained operation",
                code: ERROR_INVALID_PARAMETER,
            }
        );
        assert_eq!(
            ProcessOperationError::from_code("retained operation", ERROR_ACCESS_DENIED),
            ProcessOperationError::AccessDenied {
                operation: "retained operation",
            }
        );
    }
}
