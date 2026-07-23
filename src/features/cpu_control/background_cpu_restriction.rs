use crate::{
    action_log::{ActionLog, ActionLogFeature},
    config::{
        BackgroundCpuRestrictionSettings, CoreSteeringMode, CoreSteeringRule, CoreSteeringSettings,
        CpuRestrictionControlStyle, CpuRestrictionMode, CpuRestrictionStrategy,
    },
    core_steering::{self, CoreSteeringManager, CoreSteeringSnapshot, LogicalProcessorKind},
    foreground::list_processes,
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
        foreground_process_id: Option<u32>,
        action_log: &mut ActionLog,
    ) -> CoreSteeringSnapshot {
        let mut update_affinity = |enabled: bool, rules: Vec<CoreSteeringRule>, message: &str| {
            let affinity_settings = CoreSteeringSettings {
                enabled,
                exclude_foreground_app: settings.exclude_foreground_app,
                rules,
            };
            let mut snapshot = self.affinity.update(
                &affinity_settings,
                automation_enabled,
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
            return update_affinity(false, Vec::new(), message);
        }

        let Some(core_mask) = background_restriction_core_mask(settings) else {
            let mut snapshot =
                update_affinity(false, Vec::new(), "No usable CPU restriction target.");
            snapshot.enabled = true;
            return snapshot;
        };

        let mode = match settings.mode {
            CpuRestrictionMode::SoftCpuSets => CoreSteeringMode::Soft,
            CpuRestrictionMode::HardAffinity => CoreSteeringMode::Hard,
        };
        let processes = match list_processes() {
            Ok(processes) => processes,
            Err(error) => {
                let mut snapshot = update_affinity(false, Vec::new(), &error);
                snapshot.enabled = true;
                snapshot.last_error = Some(error);
                return snapshot;
            }
        };
        let rules = processes
            .into_iter()
            .filter(|process| {
                process.id != 0
                    && !core_steering::is_builtin_excluded(&process.name)
                    && !settings.exclusion_enabled_for(&process.name)
            })
            .map(|process| CoreSteeringRule {
                enabled: true,
                mode,
                process_name: process.name,
                core_mask,
            })
            .collect();

        update_affinity(true, rules, "Background CPU Restriction active.")
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
