use std::{
    mem::size_of,
    ptr::{null_mut, read_unaligned},
};

use windows_sys::Win32::{
    Foundation::ERROR_INSUFFICIENT_BUFFER,
    System::{
        SystemInformation::{GetSystemCpuSetInformation, SYSTEM_CPU_SET_INFORMATION},
        Threading::{
            GetProcessAffinityMask, GetProcessDefaultCpuSets, SetProcessAffinityMask,
            SetProcessDefaultCpuSets,
        },
    },
};

use crate::win_util::{last_error, WinHandle};

use super::ProcessOperationError;

pub(crate) fn query_affinity(process: &WinHandle) -> Result<(usize, usize), ProcessOperationError> {
    let mut process_affinity = 0;
    let mut system_affinity = 0;
    // SAFETY: process is a verified live handle and both outputs are writable.
    let ok = unsafe {
        GetProcessAffinityMask(process.raw(), &mut process_affinity, &mut system_affinity)
    };
    if ok == 0 {
        Err(ProcessOperationError::capture("GetProcessAffinityMask"))
    } else {
        Ok((process_affinity, system_affinity))
    }
}

pub(crate) fn query_cpu_sets(process: &WinHandle) -> Result<Vec<u32>, ProcessOperationError> {
    let mut required_id_count = 0;
    // SAFETY: a null buffer with zero capacity requests the required element count.
    let probe_ok =
        unsafe { GetProcessDefaultCpuSets(process.raw(), null_mut(), 0, &mut required_id_count) };
    if probe_ok == 0 {
        let error = last_error();
        if error != ERROR_INSUFFICIENT_BUFFER {
            return Err(ProcessOperationError::from_code(
                "GetProcessDefaultCpuSets",
                error,
            ));
        }
    }
    if required_id_count == 0 {
        return Ok(Vec::new());
    }

    let mut ids = vec![0_u32; required_id_count as usize];
    // SAFETY: ids contains required_id_count writable entries and the count output is valid.
    let ok = unsafe {
        GetProcessDefaultCpuSets(
            process.raw(),
            ids.as_mut_ptr(),
            ids.len() as u32,
            &mut required_id_count,
        )
    };
    if ok == 0 {
        Err(ProcessOperationError::capture("GetProcessDefaultCpuSets"))
    } else {
        ids.truncate(required_id_count as usize);
        normalize_cpu_set_ids(&mut ids);
        Ok(ids)
    }
}

pub(crate) fn set_affinity(
    process: &WinHandle,
    affinity: usize,
) -> Result<(), ProcessOperationError> {
    // SAFETY: process is a verified live handle and affinity was intersected with the process's
    // current system-affinity mask.
    let ok = unsafe { SetProcessAffinityMask(process.raw(), affinity) };
    if ok == 0 {
        Err(ProcessOperationError::capture("SetProcessAffinityMask"))
    } else {
        Ok(())
    }
}

pub(crate) fn set_cpu_sets(process: &WinHandle, ids: &[u32]) -> Result<(), ProcessOperationError> {
    let (ids_ptr, count) = if ids.is_empty() {
        (null_mut(), 0)
    } else {
        (ids.as_ptr() as *mut u32, ids.len() as u32)
    };
    // SAFETY: process is a verified live handle; ids_ptr is null for a zero count or points to
    // count initialized IDs for the duration of the call.
    let ok = unsafe { SetProcessDefaultCpuSets(process.raw(), ids_ptr, count) };
    if ok == 0 {
        Err(ProcessOperationError::capture("SetProcessDefaultCpuSets"))
    } else {
        Ok(())
    }
}

pub(crate) fn cpu_set_ids_for_mask(rule_mask: u64) -> Result<Vec<u32>, ProcessOperationError> {
    let mut returned_length = 0;
    // SAFETY: a null buffer with zero length requests the required byte count.
    let probe_ok =
        unsafe { GetSystemCpuSetInformation(null_mut(), 0, &mut returned_length, null_mut(), 0) };
    if probe_ok == 0 {
        let error = last_error();
        if error != ERROR_INSUFFICIENT_BUFFER {
            return Err(ProcessOperationError::Failed {
                operation: "GetSystemCpuSetInformation",
                code: error,
            });
        }
    }
    if returned_length == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0_u8; returned_length as usize];
    // SAFETY: buffer contains returned_length writable bytes and returned_length remains a valid
    // output for the call. Process and group filters are intentionally null/zero.
    let ok = unsafe {
        GetSystemCpuSetInformation(
            buffer.as_mut_ptr().cast::<SYSTEM_CPU_SET_INFORMATION>(),
            buffer.len() as u32,
            &mut returned_length,
            null_mut(),
            0,
        )
    };
    if ok == 0 {
        return Err(ProcessOperationError::Failed {
            operation: "GetSystemCpuSetInformation",
            code: last_error(),
        });
    }
    buffer.truncate(returned_length as usize);
    Ok(cpu_set_ids_for_mask_from_bytes(&buffer, rule_mask))
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CpuSetInformationHeader {
    size: u32,
    cpu_set_type: u32,
}

fn cpu_set_ids_for_mask_from_bytes(buffer: &[u8], rule_mask: u64) -> Vec<u32> {
    let mut ids = Vec::new();
    let mut offset = 0_usize;
    let header_size = size_of::<CpuSetInformationHeader>();
    while offset.saturating_add(header_size) <= buffer.len() {
        // SAFETY: the preceding bounds check guarantees a complete header; unaligned reads are
        // required because the API returns a packed variable-length byte buffer.
        let header = unsafe {
            read_unaligned(
                buffer
                    .as_ptr()
                    .add(offset)
                    .cast::<CpuSetInformationHeader>(),
            )
        };
        let record_size = header.size as usize;
        if record_size < header_size || offset.saturating_add(record_size) > buffer.len() {
            break;
        }
        if header.cpu_set_type == 0 && record_size >= size_of::<SYSTEM_CPU_SET_INFORMATION>() {
            // SAFETY: the record-size check guarantees a complete structure and the same packed
            // API buffer requires an unaligned read.
            let info = unsafe {
                read_unaligned(
                    buffer
                        .as_ptr()
                        .add(offset)
                        .cast::<SYSTEM_CPU_SET_INFORMATION>(),
                )
            };
            // SAFETY: cpu_set_type zero selects the CpuSet union member.
            let cpu_set = unsafe { info.Anonymous.CpuSet };
            if cpu_set.Group == 0 && cpu_set.LogicalProcessorIndex < 64 {
                let bit = 1_u64 << cpu_set.LogicalProcessorIndex;
                if rule_mask & bit != 0 {
                    ids.push(cpu_set.Id);
                }
            }
        }
        offset += record_size;
    }
    normalize_cpu_set_ids(&mut ids);
    ids
}

fn normalize_cpu_set_ids(ids: &mut Vec<u32>) {
    ids.sort_unstable();
    ids.dedup();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_cpu_set_buffer_yields_no_ids() {
        assert!(cpu_set_ids_for_mask_from_bytes(&[], 0b11).is_empty());
    }
}
