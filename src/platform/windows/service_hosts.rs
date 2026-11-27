use std::{
    collections::BTreeSet,
    io,
    mem::{size_of, size_of_val, MaybeUninit},
    ptr::null,
};

use windows_sys::Win32::{
    Foundation::ERROR_MORE_DATA,
    System::Services::{
        CloseServiceHandle, EnumServicesStatusExW, OpenSCManagerW, ENUM_SERVICE_STATUS_PROCESSW,
        SC_ENUM_PROCESS_INFO, SC_HANDLE, SC_MANAGER_ENUMERATE_SERVICE, SERVICE_ACTIVE,
        SERVICE_WIN32,
    },
};

const MAX_SERVICE_BUFFER_BYTES: usize = 256 * 1024;

struct ServiceManager(SC_HANDLE);

impl Drop for ServiceManager {
    fn drop(&mut self) {
        // SAFETY: This wrapper owns the non-null SCM handle returned by OpenSCManagerW.
        unsafe { CloseServiceHandle(self.0) };
    }
}

/// Returns positive process IDs for active Win32 services visible to this caller.
/// Windows can omit services when the caller lacks their SERVICE_QUERY_STATUS right.
pub(crate) fn active_service_process_ids() -> Result<BTreeSet<u32>, String> {
    // SAFETY: Null names select the local computer's active services database.
    let manager = unsafe { OpenSCManagerW(null(), null(), SC_MANAGER_ENUMERATE_SERVICE) };
    if manager.is_null() {
        return Err(format!("OpenSCManagerW: {}", io::Error::last_os_error()));
    }
    let manager = ServiceManager(manager);
    let mut buffer = vec![
        MaybeUninit::<ENUM_SERVICE_STATUS_PROCESSW>::uninit();
        MAX_SERVICE_BUFFER_BYTES / size_of::<ENUM_SERVICE_STATUS_PROCESSW>()
    ];
    let buffer_bytes = size_of_val(buffer.as_slice()) as u32;
    let mut pids = BTreeSet::new();
    let mut resume = 0;
    loop {
        let previous = resume;
        let mut bytes_needed = 0;
        let mut returned = 0;
        // SAFETY: The handle is live, the aligned buffer has its stated byte length,
        // the DWORD outputs are writable, and a null group includes every group.
        let success = unsafe {
            EnumServicesStatusExW(
                manager.0,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_ACTIVE,
                buffer.as_mut_ptr().cast(),
                buffer_bytes,
                &mut bytes_needed,
                &mut returned,
                &mut resume,
                null(),
            )
        } != 0;
        if !success {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_MORE_DATA as i32) {
                return Err(format!("EnumServicesStatusExW: {error}"));
            }
            if resume <= previous {
                return Err("EnumServicesStatusExW did not advance the enumeration.".to_owned());
            }
        }
        let entries = buffer
            .get(..returned as usize)
            .ok_or_else(|| "EnumServicesStatusExW returned too many service entries.".to_owned())?;
        pids.extend(
            entries
                .iter()
                .map(|entry| {
                    // SAFETY: The API initialized all entries within the checked returned count.
                    unsafe { entry.assume_init_ref() }
                        .ServiceStatusProcess
                        .dwProcessId
                })
                .filter(|pid| *pid != 0),
        );
        if success {
            return Ok(pids);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "reads the local Windows services database; run on an interactive Windows host"]
    fn native_active_service_snapshot_contains_process_ids() -> Result<(), String> {
        let pids = active_service_process_ids()?;
        assert!(!pids.is_empty());
        assert!(!pids.contains(&0));
        assert!(!pids.contains(&std::process::id()));
        Ok(())
    }
}
