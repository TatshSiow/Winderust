use crate::{
    action_log::ActionLog,
    affinity::{self, CpuAffinityManager, CpuAffinitySnapshot, LogicalProcessorKind},
    config::{
        BackgroundCpuRestrictionSettings, CpuAffinityMode, CpuAffinityRule, CpuAffinitySettings,
        EcoQosCpuRestrictionControlStyle, EcoQosCpuRestrictionMode, EcoQosCpuRestrictionStrategy,
    },
    foreground::list_processes,
};

#[derive(Default)]
pub struct BackgroundCpuRestrictionManager {
    affinity: CpuAffinityManager,
}

impl BackgroundCpuRestrictionManager {
    pub fn update(
        &mut self,
        settings: &BackgroundCpuRestrictionSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        action_log: &mut ActionLog,
    ) -> CpuAffinitySnapshot {
        if !automation_enabled || !settings.enabled {
            let affinity_settings = CpuAffinitySettings {
                enabled: false,
                exclude_foreground_app: settings.exclude_foreground_app,
                rules: Vec::new(),
            };
            let mut snapshot = self.affinity.update(
                &affinity_settings,
                automation_enabled,
                foreground_process_id,
                action_log,
            );
            snapshot.message = if automation_enabled {
                "Background CPU Restriction disabled.".to_owned()
            } else {
                "Automation disabled.".to_owned()
            };
            return snapshot;
        }

        let Some(core_mask) = background_restriction_core_mask(settings) else {
            let affinity_settings = CpuAffinitySettings {
                enabled: false,
                exclude_foreground_app: settings.exclude_foreground_app,
                rules: Vec::new(),
            };
            let mut snapshot = self.affinity.update(
                &affinity_settings,
                automation_enabled,
                foreground_process_id,
                action_log,
            );
            snapshot.enabled = true;
            snapshot.message = "No usable CPU restriction target.".to_owned();
            return snapshot;
        };

        let mode = match settings.mode {
            EcoQosCpuRestrictionMode::SoftCpuSets => CpuAffinityMode::Soft,
            EcoQosCpuRestrictionMode::HardAffinity => CpuAffinityMode::Hard,
        };
        let rules = list_processes()
            .map(|processes| {
                processes
                    .into_iter()
                    .filter(|process| {
                        process.id != 0
                            && !affinity::is_builtin_excluded(&process.name)
                            && !settings.exclusion_enabled_for(&process.name)
                    })
                    .map(|process| CpuAffinityRule {
                        enabled: true,
                        mode,
                        process_name: process.name,
                        core_mask,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let affinity_settings = CpuAffinitySettings {
            enabled: true,
            exclude_foreground_app: settings.exclude_foreground_app,
            rules,
        };
        let mut snapshot = self.affinity.update(
            &affinity_settings,
            automation_enabled,
            foreground_process_id,
            action_log,
        );
        snapshot.message = "Background CPU Restriction active.".to_owned();
        snapshot
    }
}

fn background_restriction_core_mask(settings: &BackgroundCpuRestrictionSettings) -> Option<u64> {
    if settings.strategy == EcoQosCpuRestrictionStrategy::Off {
        return None;
    }

    let processors = affinity::logical_processors();
    if processors.is_empty() {
        return None;
    }

    if settings.control_style == EcoQosCpuRestrictionControlStyle::CoreToggle {
        let mask = settings.core_mask & affinity_processors_mask(&processors);
        return (mask != 0).then_some(mask);
    }

    let mut selected = match settings.strategy {
        EcoQosCpuRestrictionStrategy::Off => Vec::new(),
        EcoQosCpuRestrictionStrategy::Auto => {
            let e_core_mask =
                affinity_processors_kind_mask(&processors, LogicalProcessorKind::Efficiency);
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
        EcoQosCpuRestrictionStrategy::PreferEfficiencyCores => processors
            .iter()
            .filter(|processor| processor.kind == LogicalProcessorKind::Efficiency)
            .map(|processor| processor.index)
            .collect(),
        EcoQosCpuRestrictionStrategy::LimitLogicalCpus => {
            processors.iter().map(|processor| processor.index).collect()
        }
    };

    selected.sort_unstable();
    selected.dedup();
    logical_indices_to_limited_mask(&selected, settings.percent, settings.max_logical_processors)
}

fn affinity_processors_mask(processors: &[affinity::LogicalProcessorInfo]) -> u64 {
    processors.iter().fold(0_u64, |mask, processor| {
        if processor.index < u64::BITS as usize {
            mask | (1_u64 << processor.index)
        } else {
            mask
        }
    })
}

fn affinity_processors_kind_mask(
    processors: &[affinity::LogicalProcessorInfo],
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
