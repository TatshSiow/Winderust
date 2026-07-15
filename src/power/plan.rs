use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const ADAPTIVE_POWER_DEESCALATION_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdaptivePowerDemand {
    pub launch_boost: bool,
    pub workload_active: bool,
    pub total_cpu_percent: Option<f32>,
    pub peak_cpu_percent: Option<f32>,
    pub performance_peak_cpu_percent: Option<f32>,
    pub efficiency_peak_cpu_percent: Option<f32>,
    pub foreground_cpu_percent: Option<f32>,
    pub io_bytes_per_second: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AdaptivePowerProfile {
    Idle,
    Responsive,
    Sustained,
    Burst,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerPlan {
    pub guid: String,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerPlanPersonality {
    PowerSaver,
    HighPerformance,
    Balanced,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectivePowerMode {
    Unknown,
    BatterySaver,
    BetterBattery,
    Balanced,
    HighPerformance,
    MaxPerformance,
    GameMode,
    MixedReality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessorPowerPreset {
    Performance,
    Balanced,
    Saver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessorBoostMode {
    Disabled,
    Enabled,
    Aggressive,
    EfficientEnabled,
    EfficientAggressive,
    AggressiveAtGuaranteed,
    EfficientAggressiveAtGuaranteed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessorPowerValues {
    pub core_parking_min: u32,
    pub performance_min: u32,
    pub performance_max: u32,
    pub boost_policy: u32,
    pub boost_mode: ProcessorBoostMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessorPowerAcDcValues {
    pub ac: ProcessorPowerValues,
    pub dc: ProcessorPowerValues,
}

impl ProcessorPowerValues {
    pub const fn new_with_boost_mode(
        core_parking_min: u32,
        performance_min: u32,
        performance_max: u32,
        boost_policy: u32,
        boost_mode: ProcessorBoostMode,
    ) -> Self {
        Self {
            core_parking_min,
            performance_min,
            performance_max,
            boost_policy,
            boost_mode,
        }
    }

    pub const fn for_preset(preset: ProcessorPowerPreset) -> Self {
        match preset {
            ProcessorPowerPreset::Performance => Self {
                core_parking_min: 100,
                performance_min: 100,
                performance_max: 100,
                boost_policy: 100,
                boost_mode: ProcessorBoostMode::Aggressive,
            },
            ProcessorPowerPreset::Balanced => Self {
                core_parking_min: 50,
                performance_min: 5,
                performance_max: 100,
                boost_policy: 60,
                boost_mode: ProcessorBoostMode::Enabled,
            },
            ProcessorPowerPreset::Saver => Self {
                core_parking_min: 0,
                performance_min: 5,
                performance_max: 60,
                boost_policy: 0,
                boost_mode: ProcessorBoostMode::Disabled,
            },
        }
    }

    pub fn normalized(self) -> Self {
        let performance_min = self.performance_min.min(100);
        Self {
            core_parking_min: self.core_parking_min.min(100),
            performance_min,
            performance_max: self.performance_max.min(100).max(performance_min),
            boost_policy: self.boost_policy.min(100),
            boost_mode: self.boost_mode,
        }
    }
}

impl ProcessorBoostMode {
    pub const ALL: [Self; 7] = [
        Self::Disabled,
        Self::Enabled,
        Self::Aggressive,
        Self::EfficientEnabled,
        Self::EfficientAggressive,
        Self::AggressiveAtGuaranteed,
        Self::EfficientAggressiveAtGuaranteed,
    ];

    pub const fn from_power_value(value: u32) -> Self {
        match value {
            0 => Self::Disabled,
            1 => Self::Enabled,
            2 => Self::Aggressive,
            3 => Self::EfficientEnabled,
            4 => Self::EfficientAggressive,
            5 => Self::AggressiveAtGuaranteed,
            6 => Self::EfficientAggressiveAtGuaranteed,
            _ => Self::Enabled,
        }
    }

    pub const fn power_value(self) -> u32 {
        match self {
            Self::Disabled => 0,
            Self::Enabled => 1,
            Self::Aggressive => 2,
            Self::EfficientEnabled => 3,
            Self::EfficientAggressive => 4,
            Self::AggressiveAtGuaranteed => 5,
            Self::EfficientAggressiveAtGuaranteed => 6,
        }
    }
}

impl ProcessorPowerAcDcValues {
    pub const fn new(ac: ProcessorPowerValues, dc: ProcessorPowerValues) -> Self {
        Self { ac, dc }
    }

    pub const fn same(values: ProcessorPowerValues) -> Self {
        Self {
            ac: values,
            dc: values,
        }
    }

    pub fn normalized(self) -> Self {
        Self {
            ac: self.ac.normalized(),
            dc: self.dc.normalized(),
        }
    }
}

impl AdaptivePowerProfile {
    pub fn for_demand(demand: AdaptivePowerDemand) -> Self {
        if demand.launch_boost
            || demand.total_cpu_percent.is_some_and(|usage| usage >= 85.0)
            || demand.peak_cpu_percent.is_some_and(|usage| usage >= 85.0)
            || demand
                .performance_peak_cpu_percent
                .is_some_and(|usage| usage >= 85.0)
            || demand
                .foreground_cpu_percent
                .is_some_and(|usage| usage >= 25.0)
        {
            Self::Burst
        } else if demand.workload_active
            || demand.total_cpu_percent.is_some_and(|usage| usage >= 55.0)
            || demand.peak_cpu_percent.is_some_and(|usage| usage >= 55.0)
            || demand
                .performance_peak_cpu_percent
                .is_some_and(|usage| usage >= 55.0)
            || demand
                .efficiency_peak_cpu_percent
                .is_some_and(|usage| usage >= 85.0)
            || demand
                .foreground_cpu_percent
                .is_some_and(|usage| usage >= 8.0)
        {
            Self::Sustained
        } else if demand.total_cpu_percent.is_some_and(|usage| usage >= 20.0)
            || demand.peak_cpu_percent.is_some_and(|usage| usage >= 20.0)
            || demand
                .foreground_cpu_percent
                .is_some_and(|usage| usage >= 2.0)
            || demand
                .performance_peak_cpu_percent
                .is_some_and(|usage| usage >= 20.0)
            || demand
                .efficiency_peak_cpu_percent
                .is_some_and(|usage| usage >= 55.0)
            || demand
                .io_bytes_per_second
                .is_some_and(|throughput| throughput >= 8.0 * 1024.0 * 1024.0)
        {
            Self::Responsive
        } else {
            Self::Idle
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Responsive => "Responsive",
            Self::Sustained => "Sustained",
            Self::Burst => "Burst",
        }
    }

    pub const fn power_values(self) -> ProcessorPowerAcDcValues {
        match self {
            Self::Idle => ProcessorPowerAcDcValues::new(
                ProcessorPowerValues::new_with_boost_mode(
                    0,
                    5,
                    55,
                    0,
                    ProcessorBoostMode::Disabled,
                ),
                ProcessorPowerValues::new_with_boost_mode(
                    0,
                    5,
                    45,
                    0,
                    ProcessorBoostMode::Disabled,
                ),
            ),
            Self::Responsive => ProcessorPowerAcDcValues::new(
                ProcessorPowerValues::new_with_boost_mode(
                    25,
                    10,
                    85,
                    45,
                    ProcessorBoostMode::EfficientEnabled,
                ),
                ProcessorPowerValues::new_with_boost_mode(
                    10,
                    5,
                    70,
                    30,
                    ProcessorBoostMode::EfficientEnabled,
                ),
            ),
            Self::Sustained => ProcessorPowerAcDcValues::new(
                ProcessorPowerValues::new_with_boost_mode(
                    60,
                    20,
                    100,
                    80,
                    ProcessorBoostMode::EfficientAggressive,
                ),
                ProcessorPowerValues::new_with_boost_mode(
                    30,
                    10,
                    90,
                    60,
                    ProcessorBoostMode::EfficientEnabled,
                ),
            ),
            Self::Burst => ProcessorPowerAcDcValues::new(
                ProcessorPowerValues::new_with_boost_mode(
                    100,
                    35,
                    100,
                    100,
                    ProcessorBoostMode::Aggressive,
                ),
                ProcessorPowerValues::new_with_boost_mode(
                    60,
                    20,
                    100,
                    80,
                    ProcessorBoostMode::EfficientAggressive,
                ),
            ),
        }
    }

    pub fn calibrated_power_values(
        self,
        baseline: ProcessorPowerValues,
        has_efficiency_cores: bool,
    ) -> ProcessorPowerAcDcValues {
        let baseline = baseline.normalized();
        if self == Self::Idle {
            return ProcessorPowerAcDcValues::same(baseline);
        }

        fn apply_floor(
            values: ProcessorPowerValues,
            baseline: ProcessorPowerValues,
        ) -> ProcessorPowerValues {
            ProcessorPowerValues::new_with_boost_mode(
                values.core_parking_min.max(baseline.core_parking_min),
                values.performance_min.max(baseline.performance_min),
                values.performance_max.max(baseline.performance_max),
                values.boost_policy.max(baseline.boost_policy),
                values.boost_mode,
            )
            .normalized()
        }

        let mut values = self.power_values();
        if has_efficiency_cores {
            match self {
                Self::Idle | Self::Responsive => {}
                Self::Sustained => {
                    values.ac.core_parking_min = 40;
                    values.ac.performance_min = 15;
                }
                Self::Burst => {
                    values.ac.core_parking_min = 50;
                    values.ac.performance_min = 20;
                    values.dc.core_parking_min = 40;
                    values.dc.performance_min = 15;
                }
            }
        }
        ProcessorPowerAcDcValues::new(
            apply_floor(values.ac, baseline),
            apply_floor(values.dc, baseline),
        )
    }
}

pub fn adaptive_power_profile_transition(
    current: AdaptivePowerProfile,
    desired: AdaptivePowerProfile,
    time_in_current_profile: Duration,
) -> AdaptivePowerProfile {
    if desired > current
        || (desired < current && time_in_current_profile >= ADAPTIVE_POWER_DEESCALATION_DELAY)
    {
        desired
    } else {
        current
    }
}

impl PowerPlan {
    pub fn display_name(&self) -> String {
        if self.active {
            format!("{} (active)", self.name)
        } else {
            self.name.clone()
        }
    }
}

impl PowerPlanPersonality {
    pub const fn from_power_value(value: u32) -> Self {
        match value {
            0 => Self::PowerSaver,
            1 => Self::HighPerformance,
            2 => Self::Balanced,
            _ => Self::Unknown,
        }
    }
}

impl EffectivePowerMode {
    pub const fn from_raw(value: i32) -> Self {
        match value {
            0 => Self::BatterySaver,
            1 => Self::BetterBattery,
            2 => Self::Balanced,
            3 => Self::HighPerformance,
            4 => Self::MaxPerformance,
            5 => Self::GameMode,
            6 => Self::MixedReality,
            _ => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod adaptive_tests {
    use super::*;

    fn demand() -> AdaptivePowerDemand {
        AdaptivePowerDemand {
            launch_boost: false,
            workload_active: false,
            total_cpu_percent: Some(0.0),
            peak_cpu_percent: Some(0.0),
            performance_peak_cpu_percent: None,
            efficiency_peak_cpu_percent: None,
            foreground_cpu_percent: Some(0.0),
            io_bytes_per_second: Some(0.0),
        }
    }

    #[test]
    fn adaptive_demand_selects_cpu_foreground_io_and_burst_profiles() {
        assert_eq!(
            AdaptivePowerProfile::for_demand(demand()),
            AdaptivePowerProfile::Idle
        );
        assert_eq!(
            AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
                io_bytes_per_second: Some(16.0 * 1024.0 * 1024.0),
                ..demand()
            }),
            AdaptivePowerProfile::Responsive
        );
        assert_eq!(
            AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
                io_bytes_per_second: Some(256.0 * 1024.0 * 1024.0),
                ..demand()
            }),
            AdaptivePowerProfile::Responsive
        );
        assert_eq!(
            AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
                foreground_cpu_percent: Some(9.0),
                ..demand()
            }),
            AdaptivePowerProfile::Sustained
        );
        assert_eq!(
            AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
                peak_cpu_percent: Some(90.0),
                ..demand()
            }),
            AdaptivePowerProfile::Burst
        );
        assert_eq!(
            AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
                efficiency_peak_cpu_percent: Some(90.0),
                peak_cpu_percent: None,
                ..demand()
            }),
            AdaptivePowerProfile::Sustained
        );
        assert_eq!(
            AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
                performance_peak_cpu_percent: Some(90.0),
                peak_cpu_percent: None,
                ..demand()
            }),
            AdaptivePowerProfile::Burst
        );
        assert_eq!(
            AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
                launch_boost: true,
                ..demand()
            }),
            AdaptivePowerProfile::Burst
        );
    }

    #[test]
    fn adaptive_profiles_scale_every_processor_control() {
        let idle = AdaptivePowerProfile::Idle.power_values().ac;
        let responsive = AdaptivePowerProfile::Responsive.power_values().ac;
        let sustained = AdaptivePowerProfile::Sustained.power_values().ac;
        let burst = AdaptivePowerProfile::Burst.power_values().ac;

        assert!(idle.core_parking_min < responsive.core_parking_min);
        assert!(responsive.core_parking_min < sustained.core_parking_min);
        assert!(sustained.core_parking_min < burst.core_parking_min);
        assert!(idle.performance_min < responsive.performance_min);
        assert!(responsive.performance_min < sustained.performance_min);
        assert!(sustained.performance_min < burst.performance_min);
        assert!(idle.performance_max < responsive.performance_max);
        assert!(responsive.performance_max < sustained.performance_max);
        assert!(idle.boost_policy < responsive.boost_policy);
        assert!(responsive.boost_policy < sustained.boost_policy);
        assert!(sustained.boost_policy < burst.boost_policy);
        assert_ne!(idle.boost_mode, responsive.boost_mode);
        assert_ne!(responsive.boost_mode, sustained.boost_mode);
        assert_ne!(sustained.boost_mode, burst.boost_mode);
    }

    #[test]
    fn adaptive_profiles_preserve_the_configured_baseline() {
        let baseline = ProcessorPowerValues::new_with_boost_mode(
            30,
            15,
            95,
            60,
            ProcessorBoostMode::EfficientEnabled,
        );

        assert_eq!(
            AdaptivePowerProfile::Idle.calibrated_power_values(baseline, false),
            ProcessorPowerAcDcValues::same(baseline)
        );
        let burst = AdaptivePowerProfile::Burst
            .calibrated_power_values(baseline, false)
            .ac;
        assert_eq!(burst.performance_max, 100);
        assert!(burst.core_parking_min >= baseline.core_parking_min);
        assert!(burst.performance_min >= baseline.performance_min);
        assert!(burst.boost_policy >= baseline.boost_policy);
    }

    #[test]
    fn adaptive_profiles_preserve_hybrid_turbo_headroom() {
        let baseline =
            ProcessorPowerValues::new_with_boost_mode(0, 5, 45, 0, ProcessorBoostMode::Disabled);
        let sustained = AdaptivePowerProfile::Sustained
            .calibrated_power_values(baseline, true)
            .ac;
        let burst = AdaptivePowerProfile::Burst
            .calibrated_power_values(baseline, true)
            .ac;

        assert_eq!(
            (sustained.core_parking_min, sustained.performance_min),
            (40, 15)
        );
        assert_eq!((burst.core_parking_min, burst.performance_min), (50, 20));
        assert!(
            burst.core_parking_min
                < AdaptivePowerProfile::Burst
                    .power_values()
                    .ac
                    .core_parking_min
        );
        assert_eq!(burst.performance_max, 100);
        assert_eq!(burst.boost_policy, 100);
        assert_eq!(burst.boost_mode, ProcessorBoostMode::Aggressive);
    }

    #[test]
    fn adaptive_profiles_rise_immediately_and_fall_after_hysteresis() {
        assert_eq!(
            adaptive_power_profile_transition(
                AdaptivePowerProfile::Idle,
                AdaptivePowerProfile::Burst,
                Duration::ZERO,
            ),
            AdaptivePowerProfile::Burst
        );
        assert_eq!(
            adaptive_power_profile_transition(
                AdaptivePowerProfile::Burst,
                AdaptivePowerProfile::Idle,
                Duration::from_secs(4),
            ),
            AdaptivePowerProfile::Burst
        );
        assert_eq!(
            adaptive_power_profile_transition(
                AdaptivePowerProfile::Burst,
                AdaptivePowerProfile::Idle,
                ADAPTIVE_POWER_DEESCALATION_DELAY,
            ),
            AdaptivePowerProfile::Idle
        );
    }
}
