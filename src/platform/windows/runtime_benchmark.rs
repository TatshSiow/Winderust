//! Read-only native diagnostics for disposable benchmark workers.

use std::{mem::size_of, os::windows::io::AsRawHandle, ptr::null_mut, thread::JoinHandle};

use serde_json::{json, Value};
use windows_sys::Win32::{
    Foundation::{ERROR_ARITHMETIC_OVERFLOW, FILETIME, HANDLE},
    System::{
        JobObjects::{
            IsProcessInJob, JobObjectCpuRateControlInformation, QueryInformationJobObject,
            JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE,
            JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOB_OBJECT_CPU_RATE_CONTROL_MIN_MAX_RATE,
            JOB_OBJECT_CPU_RATE_CONTROL_WEIGHT_BASED,
        },
        Threading::{GetCurrentProcess, GetCurrentThread, GetThreadTimes},
    },
};

use crate::win_util::{filetime_to_u64, last_error};

pub(crate) fn thread_cpu_ticks(thread: &JoinHandle<()>) -> Result<u64, u32> {
    read_thread_cpu_ticks(thread.as_raw_handle())
}

pub(crate) fn current_thread_cpu_ticks() -> Result<u64, u32> {
    // SAFETY: GetCurrentThread takes no arguments and returns a pseudo-handle
    // valid on this calling thread; it is neither owned nor closed here.
    let thread = unsafe { GetCurrentThread() };
    read_thread_cpu_ticks(thread)
}

fn read_thread_cpu_ticks(thread: HANDLE) -> Result<u64, u32> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: callers supply a borrowed live JoinHandle or this calling thread's
    // pseudo-handle; all four FILETIME outputs remain writable during the call.
    if unsafe { GetThreadTimes(thread, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        return Err(last_error());
    }
    filetime_to_u64(kernel)
        .checked_add(filetime_to_u64(user))
        .ok_or(ERROR_ARITHMETIC_OVERFLOW)
}

pub(crate) fn current_job_info() -> Value {
    let mut in_job = 0;
    // SAFETY: GetCurrentProcess returns a borrowed pseudo-handle; a null job asks
    // about any membership and in_job is writable. No handle is retained or closed.
    if unsafe { IsProcessInJob(GetCurrentProcess(), null_mut(), &mut in_job) } == 0 {
        let error = last_error();
        return json!({ "in_job": null, "membership_error_code": error });
    }
    if in_job == 0 {
        return json!({ "in_job": false });
    }
    let mut information = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
    // SAFETY: a null job queries the current process's immediate job; information
    // provides the exact writable structure size and no return-length buffer is needed.
    let ok = unsafe {
        QueryInformationJobObject(
            null_mut(),
            JobObjectCpuRateControlInformation,
            (&mut information as *mut JOBOBJECT_CPU_RATE_CONTROL_INFORMATION).cast(),
            size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
            null_mut(),
        )
    };
    let cpu_rate_control = if ok == 0 {
        let error = last_error();
        json!({ "error_code": error })
    } else {
        cpu_rate_info(&information)
    };
    json!({
        "in_job": true,
        "query_scope": "current process immediate job only",
        "outer_job_limits": "unknown; nested parent jobs are not inspected",
        "cpu_rate_control": cpu_rate_control,
    })
}

fn cpu_rate_info(information: &JOBOBJECT_CPU_RATE_CONTROL_INFORMATION) -> Value {
    let flags = information.ControlFlags;
    let enabled = flags & JOB_OBJECT_CPU_RATE_CONTROL_ENABLE != 0;
    let weight_based = flags & JOB_OBJECT_CPU_RATE_CONTROL_WEIGHT_BASED != 0;
    let min_max = flags & JOB_OBJECT_CPU_RATE_CONTROL_MIN_MAX_RATE != 0;
    let hard_cap = flags & JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP != 0;
    let mut result = json!({ "control_flags": flags, "enabled": enabled, "hard_cap": hard_cap });
    if !enabled {
        return result;
    }
    if (min_max && (weight_based || hard_cap)) || (weight_based && hard_cap) {
        result["interpretation_error"] = json!("Conflicting CPU rate control modes");
    } else if min_max {
        // SAFETY: MIN_MAX_RATE selects the documented MinRate/MaxRate union member.
        let rates = unsafe { information.Anonymous.Anonymous };
        result["min_rate"] = json!(rates.MinRate);
        result["max_rate"] = json!(rates.MaxRate);
    } else if weight_based {
        // SAFETY: WEIGHT_BASED selects the documented Weight union member.
        result["weight"] = json!(unsafe { information.Anonymous.Weight });
    } else {
        // SAFETY: neither WEIGHT_BASED nor MIN_MAX_RATE is set, selecting CpuRate.
        result["cpu_rate"] = json!(unsafe { information.Anonymous.CpuRate });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0_0,
    };

    #[test]
    fn job_rate_union_is_interpreted_only_for_its_enabled_mode() {
        let information = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
            ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
            Anonymous: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0 { CpuRate: 5_000 },
        };
        assert_eq!(cpu_rate_info(&information)["cpu_rate"], json!(5_000));
        let information = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
            ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE
                | JOB_OBJECT_CPU_RATE_CONTROL_WEIGHT_BASED,
            Anonymous: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0 { Weight: 3 },
        };
        let weight = cpu_rate_info(&information);
        assert_eq!(weight["weight"], json!(3));
        assert!(weight.get("cpu_rate").is_none());
        let mut conflicting = information;
        conflicting.ControlFlags |= JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
        assert!(cpu_rate_info(&conflicting)
            .get("interpretation_error")
            .is_some());
        let mut information = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
            ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE
                | JOB_OBJECT_CPU_RATE_CONTROL_MIN_MAX_RATE,
            Anonymous: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0 {
                Anonymous: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0_0 {
                    MinRate: 1_000,
                    MaxRate: 5_000,
                },
            },
        };
        let rates = cpu_rate_info(&information);
        assert_eq!(rates["min_rate"], json!(1_000));
        assert_eq!(rates["max_rate"], json!(5_000));
        assert!(rates.get("cpu_rate").is_none());
        information.ControlFlags |= JOB_OBJECT_CPU_RATE_CONTROL_WEIGHT_BASED;
        assert!(cpu_rate_info(&information)
            .get("interpretation_error")
            .is_some());
        information.ControlFlags = 0;
        assert_eq!(
            cpu_rate_info(&information),
            json!({ "control_flags": 0, "enabled": false, "hard_cap": false })
        );
    }
}
