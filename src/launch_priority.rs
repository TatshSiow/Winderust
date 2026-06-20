use std::{
    collections::{BTreeMap, BTreeSet},
    mem::size_of,
    ptr::null_mut,
};

use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
    System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegEnumKeyExW, RegOpenKeyExW,
        RegQueryValueExW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_CREATE_SUB_KEY,
        KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_DWORD,
        REG_OPTION_NON_VOLATILE,
    },
};

use crate::config::{
    LaunchPriorityRule, LaunchPrioritySettings, ProcessIoPrioritySetting,
    ProcessMemoryPrioritySetting,
};

const IFEO_SUB_KEY: &str =
    r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options";
const PERF_OPTIONS_SUB_KEY: &str = "PerfOptions";
const POWERLEAF_MANAGED_VALUE: &str = "PowerLeafManaged";
const CPU_PRIORITY_VALUE: &str = "CpuPriorityClass";
const IO_PRIORITY_VALUE: &str = "IoPriority";
const PAGE_PRIORITY_VALUE: &str = "PagePriority";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchPrioritySnapshot {
    pub enabled: bool,
    pub configured_rules: usize,
    pub applied_rules: usize,
    pub cleared_rules: usize,
    pub failed_actions: usize,
    pub last_error: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone)]
struct RegistryLaunchPriorityRule {
    process_name: String,
    cpu_priority: Option<u32>,
    io_priority: Option<u32>,
    memory_priority: Option<u32>,
}

pub fn apply_launch_priority_settings(settings: &LaunchPrioritySettings) -> LaunchPrioritySnapshot {
    let desired_rules = desired_registry_rules(settings);
    let configured_rules = desired_rules.len();

    let mut snapshot = LaunchPrioritySnapshot {
        enabled: settings.enabled,
        configured_rules,
        message: if settings.enabled {
            "Launch priority registry rules active.".to_owned()
        } else {
            "Launch priority registry rules disabled.".to_owned()
        },
        ..Default::default()
    };

    let root = match open_or_create_ifeo_root() {
        Ok(root) => root,
        Err(err) => {
            snapshot.failed_actions = 1;
            snapshot.last_error = Some(err);
            return snapshot;
        }
    };

    let managed_processes = match powerleaf_managed_processes(&root) {
        Ok(processes) => processes,
        Err(err) => {
            snapshot.failed_actions = 1;
            snapshot.last_error = Some(err);
            return snapshot;
        }
    };

    for process_name in managed_processes {
        if desired_rules.contains_key(&process_name) {
            continue;
        }

        match clear_process_launch_priority(&process_name) {
            Ok(()) => snapshot.cleared_rules += 1,
            Err(err) => snapshot.record_failure(err),
        }
    }

    for rule in desired_rules.values() {
        match apply_registry_rule(rule) {
            Ok(()) => snapshot.applied_rules += 1,
            Err(err) => snapshot.record_failure(format!("{}: {err}", rule.process_name)),
        }
    }

    if snapshot.failed_actions > 0 {
        snapshot.message = "Launch priority registry sync completed with errors.".to_owned();
    }

    snapshot
}

impl LaunchPrioritySnapshot {
    fn record_failure(&mut self, error: String) {
        if self.last_error.is_none() {
            self.last_error = Some(error);
        }
        self.failed_actions += 1;
    }
}

fn desired_registry_rules(
    settings: &LaunchPrioritySettings,
) -> BTreeMap<String, RegistryLaunchPriorityRule> {
    if !settings.enabled {
        return BTreeMap::new();
    }

    settings
        .rules
        .iter()
        .filter_map(registry_rule_from_settings_rule)
        .map(|rule| (rule.process_name.clone(), rule))
        .collect()
}

fn registry_rule_from_settings_rule(
    rule: &LaunchPriorityRule,
) -> Option<RegistryLaunchPriorityRule> {
    if !rule.enabled {
        return None;
    }

    let process_name = normalize_process_image_name(&rule.process_name)?;
    let cpu_priority = rule.cpu_priority.registry_value();
    let io_priority = io_priority_registry_value(rule.io_priority);
    let memory_priority = memory_priority_registry_value(rule.memory_priority);

    (cpu_priority.is_some() || io_priority.is_some() || memory_priority.is_some()).then_some(
        RegistryLaunchPriorityRule {
            process_name,
            cpu_priority,
            io_priority,
            memory_priority,
        },
    )
}

pub fn normalize_process_image_name(process_name: &str) -> Option<String> {
    let process_name = process_name.trim().to_ascii_lowercase();
    (!process_name.is_empty()
        && !process_name.contains('\\')
        && !process_name.contains('/')
        && !process_name.contains('*')
        && !process_name.contains('?')
        && !process_name.contains('\0'))
    .then_some(process_name)
}

fn open_or_create_ifeo_root() -> Result<RegistryKey, String> {
    create_registry_key(
        HKEY_LOCAL_MACHINE,
        IFEO_SUB_KEY,
        KEY_ENUMERATE_SUB_KEYS | KEY_CREATE_SUB_KEY | KEY_QUERY_VALUE | KEY_WOW64_64KEY,
    )
}

fn powerleaf_managed_processes(root: &RegistryKey) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    let mut index = 0;

    loop {
        let mut name_buffer = vec![0_u16; 260];
        let mut name_len = name_buffer.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                root.0,
                index,
                name_buffer.as_mut_ptr(),
                &mut name_len,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };

        match status {
            ERROR_SUCCESS => {
                let process_name = String::from_utf16_lossy(&name_buffer[..name_len as usize])
                    .trim()
                    .to_ascii_lowercase();
                if process_name_has_powerleaf_marker(&process_name) {
                    names.insert(process_name);
                }
                index += 1;
            }
            ERROR_NO_MORE_ITEMS => return Ok(names),
            status => {
                return Err(registry_error_message(
                    "enumerate IFEO registry keys",
                    status,
                ))
            }
        }
    }
}

fn process_name_has_powerleaf_marker(process_name: &str) -> bool {
    let path = perf_options_path(process_name);
    read_registry_dword(HKEY_LOCAL_MACHINE, &path, POWERLEAF_MANAGED_VALUE) == Some(1)
}

fn apply_registry_rule(rule: &RegistryLaunchPriorityRule) -> Result<(), String> {
    let path = perf_options_path(&rule.process_name);
    let key = create_registry_key(
        HKEY_LOCAL_MACHINE,
        &path,
        KEY_SET_VALUE | KEY_QUERY_VALUE | KEY_WOW64_64KEY,
    )?;

    set_or_delete_dword(&key, CPU_PRIORITY_VALUE, rule.cpu_priority)?;
    set_or_delete_dword(&key, IO_PRIORITY_VALUE, rule.io_priority)?;
    set_or_delete_dword(&key, PAGE_PRIORITY_VALUE, rule.memory_priority)?;
    set_or_delete_dword(&key, POWERLEAF_MANAGED_VALUE, Some(1))
}

fn clear_process_launch_priority(process_name: &str) -> Result<(), String> {
    let path = perf_options_path(process_name);
    let Some(key) = open_registry_key(HKEY_LOCAL_MACHINE, &path, KEY_SET_VALUE | KEY_WOW64_64KEY)?
    else {
        return Ok(());
    };

    delete_registry_value(&key, CPU_PRIORITY_VALUE)?;
    delete_registry_value(&key, IO_PRIORITY_VALUE)?;
    delete_registry_value(&key, PAGE_PRIORITY_VALUE)?;
    delete_registry_value(&key, POWERLEAF_MANAGED_VALUE)
}

fn perf_options_path(process_name: &str) -> String {
    format!(r"{IFEO_SUB_KEY}\{process_name}\{PERF_OPTIONS_SUB_KEY}")
}

fn io_priority_registry_value(priority: ProcessIoPrioritySetting) -> Option<u32> {
    match priority {
        ProcessIoPrioritySetting::Default => None,
        ProcessIoPrioritySetting::VeryLow => Some(0),
        ProcessIoPrioritySetting::Low => Some(1),
        ProcessIoPrioritySetting::Normal => Some(2),
    }
}

fn memory_priority_registry_value(priority: ProcessMemoryPrioritySetting) -> Option<u32> {
    match priority {
        ProcessMemoryPrioritySetting::Default => None,
        ProcessMemoryPrioritySetting::VeryLow => Some(1),
        ProcessMemoryPrioritySetting::Low => Some(2),
        ProcessMemoryPrioritySetting::Medium => Some(3),
        ProcessMemoryPrioritySetting::BelowNormal => Some(4),
        ProcessMemoryPrioritySetting::Normal => Some(5),
    }
}

fn create_registry_key(root: HKEY, sub_key: &str, access: u32) -> Result<RegistryKey, String> {
    let sub_key = wide_null(sub_key);
    let mut key: HKEY = null_mut();
    let mut disposition = 0_u32;
    let status = unsafe {
        RegCreateKeyExW(
            root,
            sub_key.as_ptr(),
            0,
            null_mut(),
            REG_OPTION_NON_VOLATILE,
            access,
            null_mut(),
            &mut key,
            &mut disposition,
        )
    };

    if status == ERROR_SUCCESS {
        Ok(RegistryKey(key))
    } else {
        Err(registry_error_message("create registry key", status))
    }
}

fn open_registry_key(
    root: HKEY,
    sub_key: &str,
    access: u32,
) -> Result<Option<RegistryKey>, String> {
    let sub_key = wide_null(sub_key);
    let mut key: HKEY = null_mut();
    let status = unsafe { RegOpenKeyExW(root, sub_key.as_ptr(), 0, access, &mut key) };

    match status {
        ERROR_SUCCESS => Ok(Some(RegistryKey(key))),
        ERROR_FILE_NOT_FOUND => Ok(None),
        status => Err(registry_error_message("open registry key", status)),
    }
}

fn read_registry_dword(root: HKEY, sub_key: &str, value_name: &str) -> Option<u32> {
    let key = open_registry_key(root, sub_key, KEY_QUERY_VALUE | KEY_WOW64_64KEY)
        .ok()
        .flatten()?;
    let value_name = wide_null(value_name);
    let mut value_type = 0;
    let mut value = 0_u32;
    let mut value_size = size_of::<u32>() as u32;
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            value_name.as_ptr(),
            null_mut(),
            &mut value_type,
            &mut value as *mut u32 as *mut u8,
            &mut value_size,
        )
    };

    (status == ERROR_SUCCESS && value_type == REG_DWORD && value_size == size_of::<u32>() as u32)
        .then_some(value)
}

fn set_or_delete_dword(
    key: &RegistryKey,
    value_name: &str,
    value: Option<u32>,
) -> Result<(), String> {
    match value {
        Some(value) => write_registry_dword(key, value_name, value),
        None => delete_registry_value(key, value_name),
    }
}

fn write_registry_dword(key: &RegistryKey, value_name: &str, value: u32) -> Result<(), String> {
    let value_name = wide_null(value_name);
    let status = unsafe {
        RegSetValueExW(
            key.0,
            value_name.as_ptr(),
            0,
            REG_DWORD,
            &value as *const u32 as *const u8,
            size_of::<u32>() as u32,
        )
    };

    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(registry_error_message("write registry value", status))
    }
}

fn delete_registry_value(key: &RegistryKey, value_name: &str) -> Result<(), String> {
    let value_name = wide_null(value_name);
    let status = unsafe { RegDeleteValueW(key.0, value_name.as_ptr()) };
    match status {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        status => Err(registry_error_message("delete registry value", status)),
    }
}

fn registry_error_message(action: &str, status: u32) -> String {
    format!("Failed to {action}: Windows error {status}.")
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProcessCpuPrioritySetting;

    #[test]
    fn cpu_priority_registry_values_match_process_priority_class() {
        assert_eq!(ProcessCpuPrioritySetting::Default.registry_value(), None);
        assert_eq!(ProcessCpuPrioritySetting::Idle.registry_value(), Some(1));
        assert_eq!(ProcessCpuPrioritySetting::Normal.registry_value(), Some(2));
        assert_eq!(ProcessCpuPrioritySetting::High.registry_value(), Some(3));
        assert_eq!(
            ProcessCpuPrioritySetting::BelowNormal.registry_value(),
            Some(5)
        );
        assert_eq!(
            ProcessCpuPrioritySetting::AboveNormal.registry_value(),
            Some(6)
        );
    }

    #[test]
    fn io_priority_registry_values_match_io_priority_hint() {
        assert_eq!(
            io_priority_registry_value(ProcessIoPrioritySetting::Default),
            None
        );
        assert_eq!(
            io_priority_registry_value(ProcessIoPrioritySetting::VeryLow),
            Some(0)
        );
        assert_eq!(
            io_priority_registry_value(ProcessIoPrioritySetting::Low),
            Some(1)
        );
        assert_eq!(
            io_priority_registry_value(ProcessIoPrioritySetting::Normal),
            Some(2)
        );
    }

    #[test]
    fn memory_priority_registry_values_match_page_priority() {
        assert_eq!(
            memory_priority_registry_value(ProcessMemoryPrioritySetting::Default),
            None
        );
        assert_eq!(
            memory_priority_registry_value(ProcessMemoryPrioritySetting::VeryLow),
            Some(1)
        );
        assert_eq!(
            memory_priority_registry_value(ProcessMemoryPrioritySetting::Low),
            Some(2)
        );
        assert_eq!(
            memory_priority_registry_value(ProcessMemoryPrioritySetting::Medium),
            Some(3)
        );
        assert_eq!(
            memory_priority_registry_value(ProcessMemoryPrioritySetting::BelowNormal),
            Some(4)
        );
        assert_eq!(
            memory_priority_registry_value(ProcessMemoryPrioritySetting::Normal),
            Some(5)
        );
    }

    #[test]
    fn process_image_names_reject_paths_and_patterns() {
        assert_eq!(
            normalize_process_image_name(" Game.EXE "),
            Some("game.exe".to_owned())
        );
        assert_eq!(normalize_process_image_name("folder\\game.exe"), None);
        assert_eq!(normalize_process_image_name("game*.exe"), None);
        assert_eq!(normalize_process_image_name(""), None);
    }
}
