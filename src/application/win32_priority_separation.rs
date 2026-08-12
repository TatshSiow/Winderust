use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

use crate::backend::win_registry::{
    try_read_registry_dword_root, write_registry_dword_create_root, write_registry_dword_root,
    RegistryError,
};

const PRIORITY_CONTROL_SUB_KEY: &str = "SYSTEM\\CurrentControlSet\\Control\\PriorityControl";
const PRIORITY_SEPARATION_VALUE: &str = "Win32PrioritySeparation";
const WINDERUST_REGISTRY_SUB_KEY: &str = "Software\\Winderust";
const PRIORITY_SEPARATION_BACKUP_VALUE: &str = "Win32PrioritySeparationBackup";
const PRIORITY_SEPARATION_MAX: u32 = 0x3f;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Win32PrioritySeparationError {
    ReadCurrent(RegistryError),
    ReadBackup(RegistryError),
    CurrentUnavailable,
    BackupUnavailable,
    WriteBackup(RegistryError),
    WriteCurrent(RegistryError),
}

impl std::fmt::Display for Win32PrioritySeparationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadCurrent(error) => {
                write!(
                    formatter,
                    "Failed to read Win32 Priority Separation: {error}"
                )
            }
            Self::ReadBackup(error) => write!(
                formatter,
                "Failed to read the Win32 Priority Separation backup: {error}"
            ),
            Self::CurrentUnavailable => {
                formatter.write_str("Win32 Priority Separation is unavailable.")
            }
            Self::BackupUnavailable => {
                formatter.write_str("No Win32 Priority Separation backup is available.")
            }
            Self::WriteBackup(error) => write!(
                formatter,
                "Failed to write the Win32 Priority Separation backup: {error}"
            ),
            Self::WriteCurrent(error) => {
                write!(
                    formatter,
                    "Failed to write Win32 Priority Separation: {error}"
                )
            }
        }
    }
}

impl std::error::Error for Win32PrioritySeparationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Win32PrioritySeparationSnapshot {
    pub current: Result<Option<u32>, Win32PrioritySeparationError>,
    pub backup: Result<Option<u32>, Win32PrioritySeparationError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Win32PrioritySeparationApplyOutcome {
    pub value: u32,
    pub backup: u32,
}

trait Win32PrioritySeparationStore: Send + Sync {
    fn read_current(&self) -> Result<Option<u32>, RegistryError>;
    fn read_backup(&self) -> Result<Option<u32>, RegistryError>;
    fn write_current(&self, value: u32) -> Result<(), RegistryError>;
    fn write_backup(&self, value: u32) -> Result<(), RegistryError>;
}

#[derive(Debug, Default)]
struct WindowsWin32PrioritySeparationStore;

impl Win32PrioritySeparationStore for WindowsWin32PrioritySeparationStore {
    fn read_current(&self) -> Result<Option<u32>, RegistryError> {
        try_read_registry_dword_root(
            HKEY_LOCAL_MACHINE,
            PRIORITY_CONTROL_SUB_KEY,
            PRIORITY_SEPARATION_VALUE,
        )
    }

    fn read_backup(&self) -> Result<Option<u32>, RegistryError> {
        try_read_registry_dword_root(
            HKEY_CURRENT_USER,
            WINDERUST_REGISTRY_SUB_KEY,
            PRIORITY_SEPARATION_BACKUP_VALUE,
        )
    }

    fn write_current(&self, value: u32) -> Result<(), RegistryError> {
        write_registry_dword_root(
            HKEY_LOCAL_MACHINE,
            PRIORITY_CONTROL_SUB_KEY,
            PRIORITY_SEPARATION_VALUE,
            value,
        )
    }

    fn write_backup(&self, value: u32) -> Result<(), RegistryError> {
        write_registry_dword_create_root(
            HKEY_CURRENT_USER,
            WINDERUST_REGISTRY_SUB_KEY,
            PRIORITY_SEPARATION_BACKUP_VALUE,
            value,
        )
    }
}

pub(crate) struct Win32PrioritySeparationService {
    store: Box<dyn Win32PrioritySeparationStore>,
}

impl Default for Win32PrioritySeparationService {
    fn default() -> Self {
        Self {
            store: Box::<WindowsWin32PrioritySeparationStore>::default(),
        }
    }
}

impl Win32PrioritySeparationService {
    #[cfg(test)]
    fn with_store(store: Box<dyn Win32PrioritySeparationStore>) -> Self {
        Self { store }
    }

    pub(crate) fn snapshot(&self) -> Win32PrioritySeparationSnapshot {
        Win32PrioritySeparationSnapshot {
            current: self
                .store
                .read_current()
                .map_err(Win32PrioritySeparationError::ReadCurrent),
            backup: self
                .store
                .read_backup()
                .map_err(Win32PrioritySeparationError::ReadBackup),
        }
    }

    pub(crate) fn save_backup(&self) -> Result<u32, Win32PrioritySeparationError> {
        let current = self
            .store
            .read_current()
            .map_err(Win32PrioritySeparationError::ReadCurrent)?
            .ok_or(Win32PrioritySeparationError::CurrentUnavailable)?;
        self.store
            .write_backup(current)
            .map_err(Win32PrioritySeparationError::WriteBackup)?;
        Ok(current)
    }

    pub(crate) fn apply(
        &self,
        value: u32,
    ) -> Result<Win32PrioritySeparationApplyOutcome, Win32PrioritySeparationError> {
        let backup = self.ensure_backup()?;
        let value = value.min(PRIORITY_SEPARATION_MAX);
        self.store
            .write_current(value)
            .map_err(Win32PrioritySeparationError::WriteCurrent)?;
        Ok(Win32PrioritySeparationApplyOutcome { value, backup })
    }

    pub(crate) fn restore_backup(&self) -> Result<u32, Win32PrioritySeparationError> {
        let backup = self
            .store
            .read_backup()
            .map_err(Win32PrioritySeparationError::ReadBackup)?
            .ok_or(Win32PrioritySeparationError::BackupUnavailable)?;
        self.store
            .write_current(backup)
            .map_err(Win32PrioritySeparationError::WriteCurrent)?;
        Ok(backup)
    }

    fn ensure_backup(&self) -> Result<u32, Win32PrioritySeparationError> {
        if let Some(backup) = self
            .store
            .read_backup()
            .map_err(Win32PrioritySeparationError::ReadBackup)?
        {
            return Ok(backup);
        }

        let current = self
            .store
            .read_current()
            .map_err(Win32PrioritySeparationError::ReadCurrent)?
            .ok_or(Win32PrioritySeparationError::CurrentUnavailable)?;
        self.store
            .write_backup(current)
            .map_err(Win32PrioritySeparationError::WriteBackup)?;
        Ok(current)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone)]
    struct FakeStore {
        state: Arc<Mutex<FakeStoreState>>,
    }

    #[derive(Debug, Clone)]
    struct FakeStoreState {
        current: Result<Option<u32>, RegistryError>,
        backup: Result<Option<u32>, RegistryError>,
        write_current_error: Option<RegistryError>,
        write_backup_error: Option<RegistryError>,
        current_writes: Vec<u32>,
        backup_writes: Vec<u32>,
    }

    impl FakeStore {
        fn new(current: Option<u32>, backup: Option<u32>) -> Self {
            Self {
                state: Arc::new(Mutex::new(FakeStoreState {
                    current: Ok(current),
                    backup: Ok(backup),
                    write_current_error: None,
                    write_backup_error: None,
                    current_writes: Vec::new(),
                    backup_writes: Vec::new(),
                })),
            }
        }

        fn snapshot(&self) -> FakeStoreState {
            self.state.lock().expect("fake registry lock").clone()
        }
    }

    impl Win32PrioritySeparationStore for FakeStore {
        fn read_current(&self) -> Result<Option<u32>, RegistryError> {
            self.state
                .lock()
                .expect("fake registry lock")
                .current
                .clone()
        }

        fn read_backup(&self) -> Result<Option<u32>, RegistryError> {
            self.state
                .lock()
                .expect("fake registry lock")
                .backup
                .clone()
        }

        fn write_current(&self, value: u32) -> Result<(), RegistryError> {
            let mut state = self.state.lock().expect("fake registry lock");
            if let Some(error) = state.write_current_error.clone() {
                return Err(error);
            }
            state.current_writes.push(value);
            state.current = Ok(Some(value));
            Ok(())
        }

        fn write_backup(&self, value: u32) -> Result<(), RegistryError> {
            let mut state = self.state.lock().expect("fake registry lock");
            if let Some(error) = state.write_backup_error.clone() {
                return Err(error);
            }
            state.backup_writes.push(value);
            state.backup = Ok(Some(value));
            Ok(())
        }
    }

    #[test]
    fn apply_preserves_an_existing_backup() {
        let store = FakeStore::new(Some(0x26), Some(0x18));
        let service = Win32PrioritySeparationService::with_store(Box::new(store.clone()));

        let outcome = service.apply(0x2a).expect("apply");

        assert_eq!(outcome.backup, 0x18);
        assert_eq!(outcome.value, 0x2a);
        let state = store.snapshot();
        assert!(state.backup_writes.is_empty());
        assert_eq!(state.current_writes, vec![0x2a]);
    }

    #[test]
    fn backup_write_failure_blocks_the_machine_write() {
        let store = FakeStore::new(Some(0x26), None);
        store
            .state
            .lock()
            .expect("fake registry lock")
            .write_backup_error = Some(RegistryError::WriteValue("denied".to_owned()));
        let service = Win32PrioritySeparationService::with_store(Box::new(store.clone()));

        assert!(matches!(
            service.apply(0x2a),
            Err(Win32PrioritySeparationError::WriteBackup(_))
        ));
        let state = store.snapshot();
        assert!(state.current_writes.is_empty());
        assert!(state.backup_writes.is_empty());
    }

    #[test]
    fn machine_write_failure_retains_the_new_backup_and_reports_its_stage() {
        let store = FakeStore::new(Some(0x26), None);
        store
            .state
            .lock()
            .expect("fake registry lock")
            .write_current_error = Some(RegistryError::OpenForWrite("denied".to_owned()));
        let service = Win32PrioritySeparationService::with_store(Box::new(store.clone()));

        assert!(matches!(
            service.apply(0x2a),
            Err(Win32PrioritySeparationError::WriteCurrent(_))
        ));
        let state = store.snapshot();
        assert_eq!(state.backup, Ok(Some(0x26)));
        assert_eq!(state.backup_writes, vec![0x26]);
        assert!(state.current_writes.is_empty());
    }

    #[test]
    fn restore_without_a_backup_is_typed_and_does_not_write() {
        let store = FakeStore::new(Some(0x2a), None);
        let service = Win32PrioritySeparationService::with_store(Box::new(store.clone()));

        assert_eq!(
            service.restore_backup(),
            Err(Win32PrioritySeparationError::BackupUnavailable)
        );
        assert!(store.snapshot().current_writes.is_empty());
    }

    #[test]
    fn snapshot_distinguishes_a_read_error_from_a_missing_value() {
        let store = FakeStore::new(None, None);
        store.state.lock().expect("fake registry lock").current =
            Err(RegistryError::OpenForRead("denied".to_owned()));
        let service = Win32PrioritySeparationService::with_store(Box::new(store));

        let snapshot = service.snapshot();

        assert!(matches!(
            snapshot.current,
            Err(Win32PrioritySeparationError::ReadCurrent(_))
        ));
        assert_eq!(snapshot.backup, Ok(None));
    }
}
