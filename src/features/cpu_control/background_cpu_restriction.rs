use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::{
    action_log::{ActionLog, ActionLogFeature},
    config::{
        BackgroundCpuRestrictionSettings, CoreSteeringMode, CoreSteeringSettings,
        CpuRestrictionControlStyle, CpuRestrictionMode, CpuRestrictionStrategy,
    },
    core_steering::{
        self, CoreSteeringManager, CoreSteeringSnapshot, CoreSteeringTarget, LogicalProcessorKind,
    },
    foreground::{list_processes_with_paths, process_session_id, same_executable_path},
};

pub struct BackgroundCpuRestrictionManager {
    affinity: CoreSteeringManager,
}

impl Default for BackgroundCpuRestrictionManager {
    fn default() -> Self {
        Self {
            affinity: CoreSteeringManager::with_action_log_feature(
                ActionLogFeature::BackgroundCpuRestriction,
            ),
        }
    }
}

impl BackgroundCpuRestrictionManager {
    pub fn update(
        &mut self,
        settings: &BackgroundCpuRestrictionSettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        action_log: &mut ActionLog,
    ) -> CoreSteeringSnapshot {
        let mut disable_affinity = |message: &str| {
            let affinity_settings = CoreSteeringSettings {
                enabled: false,
                exclude_foreground_app: settings.exclude_foreground_app,
                rules: Vec::new(),
            };
            let mut snapshot = self.affinity.update(
                &affinity_settings,
                automation_enabled,
                allow_cross_session_process_control,
                foreground_process_id,
                action_log,
            );
            snapshot.message = message.to_owned();
            snapshot
        };

        if !automation_enabled || !settings.enabled {
            let message = if automation_enabled {
                "Background CPU Restriction disabled."
            } else {
                "Automation disabled."
            };
            return disable_affinity(message);
        }

        let Some(core_mask) = background_restriction_core_mask(settings) else {
            let mut snapshot = disable_affinity("No usable CPU restriction target.");
            snapshot.enabled = true;
            return snapshot;
        };

        let mode = match settings.mode {
            CpuRestrictionMode::SoftCpuSets => CoreSteeringMode::Soft,
            CpuRestrictionMode::HardAffinity => CoreSteeringMode::Hard,
        };
        let processes = match list_processes_with_paths() {
            Ok(processes) => processes,
            Err(error) => {
                let mut snapshot = disable_affinity(&error);
                snapshot.enabled = true;
                snapshot.last_error = Some(error);
                return snapshot;
            }
        };
        let scanned_processes = processes.len();
        if settings.exclude_foreground_app && foreground_process_id.is_none() {
            return self.affinity.update_discovered_targets(
                Vec::new(),
                scanned_processes,
                "Paused: foreground app is unknown.",
                action_log,
            );
        }
        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            return self.affinity.update_discovered_targets(
                Vec::new(),
                scanned_processes,
                "Paused: current Windows session is unknown.",
                action_log,
            );
        };
        let foreground_path = foreground_process_id.and_then(|id| {
            processes
                .iter()
                .find(|process| process.id == id)
                .and_then(|process| process.image_path.clone())
        });
        let targets = processes
            .into_iter()
            .filter(|process| {
                process.id != 0
                    && process.id != current_process_id
                    && !core_steering::is_builtin_excluded(&process.name)
                    && (allow_cross_session_process_control
                        || process_session_id(process.id) == Some(current_session_id))
                    && !(settings.exclude_foreground_app
                        && (Some(process.id) == foreground_process_id
                            || process
                                .image_path
                                .as_deref()
                                .zip(foreground_path.as_deref())
                                .is_some_and(|(path, foreground)| {
                                    same_executable_path(path, foreground)
                                })))
                    && process.image_path.as_deref().is_some_and(|path| {
                        !settings.exclusion_enabled_for(path.to_string_lossy().as_ref())
                    })
            })
            .filter_map(|process| {
                process.image_path.map(|path| CoreSteeringTarget {
                    process_id: process.id,
                    process_name: process.name,
                    executable_path: path.to_string_lossy().into_owned(),
                    mode,
                    core_mask,
                    expected_creation_time: None,
                })
            })
            .collect();

        self.affinity.update_discovered_targets(
            targets,
            scanned_processes,
            "Background CPU Restriction active.",
            action_log,
        )
    }
}
fn background_restriction_core_mask(settings: &BackgroundCpuRestrictionSettings) -> Option<u64> {
    if settings.strategy == CpuRestrictionStrategy::Off {
        return None;
    }

    let processors = core_steering::logical_processors();
    if processors.is_empty() {
        return None;
    }

    if settings.control_style == CpuRestrictionControlStyle::CoreToggle {
        let mask = settings.core_mask & core_steering_processors_mask(&processors);
        return (mask != 0).then_some(mask);
    }

    let mut selected = match settings.strategy {
        CpuRestrictionStrategy::Off => Vec::new(),
        CpuRestrictionStrategy::Auto => {
            let e_core_mask =
                core_steering_processors_kind_mask(&processors, LogicalProcessorKind::Efficiency);
            if e_core_mask != 0 {
                processors
                    .iter()
                    .filter(|processor| processor.kind == LogicalProcessorKind::Efficiency)
                    .map(|processor| processor.index)
                    .collect::<Vec<_>>()
            } else {
                processors.iter().map(|processor| processor.index).collect()
            }
        }
        CpuRestrictionStrategy::PreferEfficiencyCores => processors
            .iter()
            .filter(|processor| processor.kind == LogicalProcessorKind::Efficiency)
            .map(|processor| processor.index)
            .collect(),
        CpuRestrictionStrategy::LimitLogicalCpus => {
            processors.iter().map(|processor| processor.index).collect()
        }
    };

    selected.sort_unstable();
    selected.dedup();
    logical_indices_to_limited_mask(&selected, settings.percent, settings.max_logical_processors)
}

fn core_steering_processors_mask(processors: &[core_steering::LogicalProcessorInfo]) -> u64 {
    processors.iter().fold(0_u64, |mask, processor| {
        if processor.index < u64::BITS as usize {
            mask | (1_u64 << processor.index)
        } else {
            mask
        }
    })
}

fn core_steering_processors_kind_mask(
    processors: &[core_steering::LogicalProcessorInfo],
    kind: LogicalProcessorKind,
) -> u64 {
    processors.iter().fold(0_u64, |mask, processor| {
        if processor.kind == kind && processor.index < u64::BITS as usize {
            mask | (1_u64 << processor.index)
        } else {
            mask
        }
    })
}

fn logical_indices_to_limited_mask(
    indices: &[usize],
    percent: u8,
    max_logical_processors: u8,
) -> Option<u64> {
    if indices.is_empty() {
        return None;
    }
    let percent_count = (indices.len() * usize::from(percent.clamp(1, 100))).div_ceil(100);
    let max_count = usize::from(max_logical_processors);
    let limit = if max_count == 0 {
        percent_count
    } else {
        percent_count.min(max_count)
    }
    .clamp(1, indices.len());

    let mut mask = 0_u64;
    for index in indices.iter().take(limit) {
        if *index < u64::BITS as usize {
            mask |= 1_u64 << index;
        }
    }
    (mask != 0).then_some(mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limited_mask_respects_percent_and_maximum() {
        assert_eq!(
            logical_indices_to_limited_mask(&[0, 1, 2, 3], 75, 2),
            Some(0b0011)
        );
        assert_eq!(
            logical_indices_to_limited_mask(&[0, 1, 2, 3], 25, 0),
            Some(0b0001)
        );
    }
}
