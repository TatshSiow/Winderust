use std::{
    io,
    marker::PhantomData,
    mem::{offset_of, size_of},
    ptr::{null, null_mut},
    rc::Rc,
};

use windows_sys::{
    core::PCWSTR,
    w,
    Win32::{
        Foundation::{
            LocalFree, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_INVALID_OWNER,
            ERROR_MORE_DATA, ERROR_PRIVILEGE_NOT_HELD, ERROR_SUCCESS, WAIT_ABANDONED, WAIT_FAILED,
            WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
                SDDL_REVISION_1, SE_REGISTRY_KEY,
            },
            GetAce, GetSecurityDescriptorControl, IsWellKnownSid, WinBuiltinAdministratorsSid,
            WinLocalSystemSid, ACCESS_ALLOWED_ACE, ACE_HEADER, DACL_SECURITY_INFORMATION,
            OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
            SE_DACL_PROTECTED,
        },
        Storage::FileSystem::READ_CONTROL,
        System::{
            Registry::{
                RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
                RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, KEY_QUERY_VALUE,
                KEY_SET_VALUE, KEY_WOW64_64KEY, REG_BINARY, REG_CREATED_NEW_KEY,
                REG_OPTION_OPEN_LINK, REG_OPTION_VOLATILE, REG_SAM_FLAGS,
            },
            SystemServices::ACCESS_ALLOWED_ACE_TYPE,
            Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
        },
    },
};

use crate::win_util::{last_error, WinHandle};

use super::{CaptureIdentity, InterruptTimingError, STARTUP_RECORD_LEN};

const LEASE_KEY: PCWSTR = w!(r"SOFTWARE\Winderust Interrupt Timing");
const LEASE_VALUE: PCWSTR = w!("Capture");
const LEASE_MUTEX: PCWSTR = w!(r"Global\Winderust.InterruptTiming.Lease");
const KEY_SECURITY: PCWSTR = w!("O:BAD:P(A;;KA;;;SY)(A;;KA;;;BA)");
const MUTEX_SECURITY: PCWSTR = w!("O:BAD:P(A;;GA;;;SY)(A;;GA;;;BA)");

pub(super) struct LeaseGuard {
    mutex: WinHandle,
    thread: PhantomData<Rc<()>>,
}

impl LeaseGuard {
    pub(super) fn try_acquire() -> Result<Option<Self>, InterruptTimingError> {
        let security = SecurityDescriptor::parse(MUTEX_SECURITY)?;
        // SAFETY: the name is terminated and the non-inheritable attributes remain valid.
        let mutex = unsafe { CreateMutexW(&security.attributes(), 0, LEASE_MUTEX) };
        if mutex.is_null() {
            return Err(windows_error("CreateMutexW", last_error()));
        }
        let mutex = WinHandle::new(mutex);
        // SAFETY: mutex is live and this zero-timeout wait grants ownership to this thread.
        match unsafe { WaitForSingleObject(mutex.raw(), 0) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Some(Self {
                mutex,
                thread: PhantomData,
            })),
            WAIT_TIMEOUT => Ok(None),
            WAIT_FAILED => Err(windows_error("WaitForSingleObject", last_error())),
            status => Err(windows_error("WaitForSingleObject", status)),
        }
    }

    pub(super) fn read(&self) -> Result<Option<CaptureIdentity>, InterruptTimingError> {
        let Some(key) = RegistryKey::open(KEY_QUERY_VALUE)? else {
            return Ok(None);
        };
        let mut bytes = [0; STARTUP_RECORD_LEN];
        let mut len = bytes.len() as u32;
        let mut value_type = 0;
        // SAFETY: key and name are valid and all output buffers have their stated capacity.
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                LEASE_VALUE,
                null(),
                &mut value_type,
                bytes.as_mut_ptr(),
                &mut len,
            )
        };
        match status {
            ERROR_FILE_NOT_FOUND => Ok(None),
            ERROR_SUCCESS if value_type == REG_BINARY && len as usize == bytes.len() => {
                CaptureIdentity::read(bytes.as_slice())
                    .map(Some)
                    .ok_or_else(invalid_lease)
            }
            ERROR_SUCCESS | ERROR_MORE_DATA => Err(invalid_lease()),
            code => Err(windows_error("RegQueryValueExW", code)),
        }
    }

    pub(super) fn publish(&self, identity: CaptureIdentity) -> Result<(), InterruptTimingError> {
        let key = RegistryKey::create()?;
        let bytes = identity.startup_record();
        // SAFETY: key and name are valid, and bytes remains readable for the call.
        let status = unsafe {
            RegSetValueExW(
                key.0,
                LEASE_VALUE,
                0,
                REG_BINARY,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        };
        check_status("RegSetValueExW", status)
    }

    pub(super) fn clear(&self) -> Result<(), InterruptTimingError> {
        let Some(key) = RegistryKey::open(KEY_SET_VALUE)? else {
            return Ok(());
        };
        // SAFETY: key is live and the value name is terminated.
        let status = unsafe { RegDeleteValueW(key.0, LEASE_VALUE) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        check_status("RegDeleteValueW", status)
    }
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        // SAFETY: this thread owns the mutex, and the handle remains live until after Drop.
        unsafe { ReleaseMutex(self.mutex.raw()) };
    }
}

struct RegistryKey(HKEY);

impl RegistryKey {
    fn open(access: REG_SAM_FLAGS) -> Result<Option<Self>, InterruptTimingError> {
        let mut key = null_mut();
        // SAFETY: the predefined root and terminated path are valid, and key is writable.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                LEASE_KEY,
                REG_OPTION_OPEN_LINK,
                access | READ_CONTROL | KEY_WOW64_64KEY,
                &mut key,
            )
        };
        match status {
            ERROR_SUCCESS => {
                let key = Self(key);
                key.check_security()?;
                Ok(Some(key))
            }
            ERROR_FILE_NOT_FOUND => Ok(None),
            code => Err(windows_error("RegOpenKeyExW", code)),
        }
    }

    fn create() -> Result<Self, InterruptTimingError> {
        if let Some(key) = Self::open(KEY_SET_VALUE)? {
            return Ok(key);
        }

        let security = SecurityDescriptor::parse(KEY_SECURITY)?;
        let mut key = null_mut();
        let mut disposition = 0;
        // SAFETY: root, path and security attributes remain valid, and outputs are writable.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_LOCAL_MACHINE,
                LEASE_KEY,
                0,
                null(),
                REG_OPTION_VOLATILE,
                KEY_SET_VALUE | KEY_WOW64_64KEY,
                &security.attributes(),
                &mut key,
                &mut disposition,
            )
        };
        check_status("RegCreateKeyExW", status)?;
        let key = Self(key);
        // Windows ignores the requested protection and volatility when reopening a key.
        if disposition != REG_CREATED_NEW_KEY {
            return Err(InterruptTimingError::Capture(
                "The interrupt timing recovery key appeared during creation.".into(),
            ));
        }
        Ok(key)
    }

    fn check_security(&self) -> Result<(), InterruptTimingError> {
        let mut owner = null_mut();
        let mut acl = null_mut();
        let mut descriptor = null_mut();
        // SAFETY: this live key has READ_CONTROL, and the requested outputs are writable.
        let status = unsafe {
            GetSecurityInfo(
                self.0,
                SE_REGISTRY_KEY,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut acl,
                null_mut(),
                &mut descriptor,
            )
        };
        check_status("GetSecurityInfo", status)?;
        let descriptor = SecurityDescriptor(descriptor);
        let (mut control, mut revision) = (0, 0);
        // SAFETY: the returned descriptor is live and both outputs are writable.
        if unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) } == 0 {
            return Err(windows_error("GetSecurityDescriptorControl", last_error()));
        }
        if control & SE_DACL_PROTECTED == 0 || owner.is_null() || acl.is_null() {
            return Err(invalid_security());
        }
        // SAFETY: the owner SID and ACL point into the live descriptor returned by Windows.
        if unsafe {
            IsWellKnownSid(owner, WinBuiltinAdministratorsSid) == 0 || (*acl).AceCount != 2
        } {
            return Err(invalid_security());
        }

        let (mut system, mut administrators) = (false, false);
        for index in 0..2 {
            let mut ace = null_mut();
            // SAFETY: the ACL has two entries, and the entry pointer output is writable.
            if unsafe { GetAce(acl, index, &mut ace) } == 0 {
                return Err(windows_error("GetAce", last_error()));
            }
            // SAFETY: GetAce succeeded and returns an entry beginning with ACE_HEADER.
            let header = unsafe { &*ace.cast::<ACE_HEADER>() };
            if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE
                || header.AceFlags != 0
                || usize::from(header.AceSize) < size_of::<ACCESS_ALLOWED_ACE>()
            {
                return Err(invalid_security());
            }
            // SAFETY: the checked entry is large enough and is an access-allowed ACE.
            let entry = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
            if entry.Mask != KEY_ALL_ACCESS {
                return Err(invalid_security());
            }
            // SAFETY: a Windows-provided access-allowed ACE stores its SID at SidStart.
            unsafe {
                let sid = ace.byte_add(offset_of!(ACCESS_ALLOWED_ACE, SidStart));
                system |= IsWellKnownSid(sid, WinLocalSystemSid) != 0;
                administrators |= IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0;
            }
        }
        if !system || !administrators {
            return Err(invalid_security());
        }
        Ok(())
    }
}

impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY: this key was opened or created successfully and is owned by self.
        unsafe { RegCloseKey(self.0) };
    }
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl SecurityDescriptor {
    fn parse(text: PCWSTR) -> Result<Self, InterruptTimingError> {
        let mut descriptor = null_mut();
        // SAFETY: callers pass a terminated SDDL constant, and descriptor is writable.
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text,
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        };
        if converted == 0 {
            return Err(windows_error(
                "ConvertStringSecurityDescriptorToSecurityDescriptorW",
                last_error(),
            ));
        }
        Ok(Self(descriptor))
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: this descriptor is owned and the generating API specifies LocalFree.
        unsafe { LocalFree(self.0) };
    }
}

fn invalid_lease() -> InterruptTimingError {
    InterruptTimingError::Capture("The interrupt timing recovery lease has invalid data.".into())
}

fn invalid_security() -> InterruptTimingError {
    InterruptTimingError::Capture(
        "The interrupt timing recovery key has unexpected permissions.".into(),
    )
}

fn check_status(operation: &str, code: u32) -> Result<(), InterruptTimingError> {
    if code == ERROR_SUCCESS {
        return Ok(());
    }
    Err(windows_error(operation, code))
}

fn windows_error(operation: &str, code: u32) -> InterruptTimingError {
    match code {
        ERROR_ACCESS_DENIED | ERROR_INVALID_OWNER | ERROR_PRIVILEGE_NOT_HELD => {
            InterruptTimingError::AccessDenied
        }
        code => InterruptTimingError::Capture(format!(
            "Interrupt timing recovery lease: {operation} failed ({})",
            io::Error::from_raw_os_error(code as i32),
        )),
    }
}
