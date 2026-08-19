use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const ADAPTIVE_POWER_DEESCALATION_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdaptivePowerDemand {
    pub focus_and_launch_profile_active: bool,
    pub background_pressure_active: bool,
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
    BackgroundPressure,
    FocusAndLaunch,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdaptivePowerBoostValues {
    pub ac_policy: u32,
    pub ac_mode: ProcessorBoostMode,
    pub battery_policy: u32,
    pub battery_mode: ProcessorBoostMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessorPowerSourceValues {
    pub ac: ProcessorPowerValues,
    pub battery: ProcessorPowerValues,
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

impl AdaptivePowerBoostValues {
    pub const BACKGROUND_PRESSURE: Self = Self::new(
        80,
        ProcessorBoostMode::EfficientAggressive,
        60,
        ProcessorBoostMode::EfficientEnabled,
    );
    pub const FOCUS_AND_LAUNCH: Self = Self::new(
        100,
        ProcessorBoostMode::Aggressive,
        80,
        ProcessorBoostMode::EfficientAggressive,
    );

    pub const fn new(
        ac_policy: u32,
        ac_mode: ProcessorBoostMode,
        battery_policy: u32,
        battery_mode: ProcessorBoostMode,
    ) -> Self {
        Self {
            ac_policy,
            ac_mode,
            battery_policy,
            battery_mode,
        }
    }

    pub fn normalized(self) -> Self {
        Self {
            ac_policy: self.ac_policy.min(100),
            battery_policy: self.battery_policy.min(100),
            ..self
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

impl ProcessorPowerSourceValues {
    pub const fn new(ac: ProcessorPowerValues, battery: ProcessorPowerValues) -> Self {
        Self { ac, battery }
    }

    pub const fn same(values: ProcessorPowerValues) -> Self {
        Self {
            ac: values,
            battery: values,
        }
    }

    pub fn normalized(self) -> Self {
        Self {
            ac: self.ac.normalized(),
            battery: self.battery.normalized(),
        }
    }
}

impl AdaptivePowerProfile {
    pub fn for_demand(demand: AdaptivePowerDemand) -> Self {
        if demand.focus_and_launch_profile_active
            || demand
                .foreground_cpu_percent
                .is_some_and(|usage| usage >= 25.0)
        {
            Self::FocusAndLaunch
        } else if demand.background_pressure_active && demand.foreground_cpu_percent.is_some() {
            Self::BackgroundPressure
        } else if demand.total_cpu_percent.is_some_and(|usage| usage >= 85.0)
            || demand.peak_cpu_percent.is_some_and(|usage| usage >= 85.0)
            || demand
                .performance_peak_cpu_percent
                .is_some_and(|usage| usage >= 85.0)
        {
            Self::FocusAndLaunch
        } else if demand.total_cpu_percent.is_some_and(|usage| usage >= 55.0)
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
            Self::BackgroundPressure
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
            Self::BackgroundPressure => "Background Pressure",
            Self::FocusAndLaunch => "Focus and Launch",
        }
    }

    pub const fn power_values(self) -> ProcessorPowerSourceValues {
        match self {
            Self::Idle => ProcessorPowerSourceValues::new(
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
            Self::Responsive => ProcessorPowerSourceValues::new(
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
            Self::BackgroundPressure => ProcessorPowerSourceValues::new(
                ProcessorPowerValues::new_with_boost_mode(
                    60,
                    20,
                    100,
                    AdaptivePowerBoostValues::BACKGROUND_PRESSURE.ac_policy,
                    AdaptivePowerBoostValues::BACKGROUND_PRESSURE.ac_mode,
                ),
                ProcessorPowerValues::new_with_boost_mode(
                    30,
                    10,
                    90,
                    AdaptivePowerBoostValues::BACKGROUND_PRESSURE.battery_policy,
                    AdaptivePowerBoostValues::BACKGROUND_PRESSURE.battery_mode,
                ),
            ),
            Self::FocusAndLaunch => ProcessorPowerSourceValues::new(
                ProcessorPowerValues::new_with_boost_mode(
                    100,
                    35,
                    100,
                    AdaptivePowerBoostValues::FOCUS_AND_LAUNCH.ac_policy,
                    AdaptivePowerBoostValues::FOCUS_AND_LAUNCH.ac_mode,
                ),
                ProcessorPowerValues::new_with_boost_mode(
                    60,
                    20,
                    100,
                    AdaptivePowerBoostValues::FOCUS_AND_LAUNCH.battery_policy,
                    AdaptivePowerBoostValues::FOCUS_AND_LAUNCH.battery_mode,
                ),
            ),
        }
    }

    pub fn calibrated_power_values(
        self,
        baseline: ProcessorPowerValues,
        has_efficiency_cores: bool,
        background_pressure_profile: AdaptivePowerBoostValues,
        focus_and_launch_profile: AdaptivePowerBoostValues,
    ) -> ProcessorPowerSourceValues {
        let baseline = baseline.normalized();
        if self == Self::Idle {
            return ProcessorPowerSourceValues::same(baseline);
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
        let boost = match self {
            Self::BackgroundPressure => Some(background_pressure_profile.normalized()),
            Self::FocusAndLaunch => Some(focus_and_launch_profile.normalized()),
            Self::Idle | Self::Responsive => None,
        };
        if let Some(boost) = boost {
            values.ac.boost_policy = boost.ac_policy;
            values.ac.boost_mode = boost.ac_mode;
            values.battery.boost_policy = boost.battery_policy;
            values.battery.boost_mode = boost.battery_mode;
        }
        if has_efficiency_cores {
            match self {
                Self::Idle | Self::Responsive => {}
                Self::BackgroundPressure => {
                    values.ac.core_parking_min = 40;
                    values.ac.performance_min = 15;
                }
                Self::FocusAndLaunch => {
                    values.ac.core_parking_min = 50;
                    values.ac.performance_min = 20;
                    values.battery.core_parking_min = 40;
                    values.battery.performance_min = 15;
                }
            }
        }
        ProcessorPowerSourceValues::new(
            apply_floor(values.ac, baseline),
            apply_floor(values.battery, baseline),
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
            focus_and_launch_profile_active: false,
            background_pressure_active: false,
            total_cpu_percent: Some(0.0),
            peak_cpu_percent: Some(0.0),
            performance_peak_cpu_percent: None,
            efficiency_peak_cpu_percent: None,
            foreground_cpu_percent: Some(0.0),
            io_bytes_per_second: Some(0.0),
        }
    }

    #[test]
    fn adaptive_demand_selects_cpu_foreground_io_and_focus_and_launch_profiles() {
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
            AdaptivePowerProfile::BackgroundPressure
        );
        assert_eq!(
            AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
                peak_cpu_percent: Some(90.0),
                ..demand()
            }),
            AdaptivePowerProfile::FocusAndLaunch
        );
        assert_eq!(
            AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
                background_pressure_active: true,
                total_cpu_percent: Some(95.0),
                peak_cpu_percent: Some(100.0),
                foreground_cpu_percent: Some(5.0),
                ..demand()
            }),
            AdaptivePowerProfile::BackgroundPressure
        );
        assert_eq!(
            AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
                background_pressure_active: true,
                total_cpu_percent: Some(95.0),
                foreground_cpu_percent: None,
                ..demand()
            }),
            AdaptivePowerProfile::FocusAndLaunch
        );
        assert_eq!(
            AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
                background_pressure_active: true,
                foreground_cpu_percent: Some(25.0),
                ..demand()
            }),
            AdaptivePowerProfile::FocusAndLaunch
        );
        assert_eq!(
            AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
                efficiency_peak_cpu_percent: Some(90.0),
                peak_cpu_percent: None,
                ..demand()
            }),
            AdaptivePowerProfile::BackgroundPressure
        );
        assert_eq!(
            AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
                performance_peak_cpu_percent: Some(90.0),
                peak_cpu_percent: None,
                ..demand()
            }),
            AdaptivePowerProfile::FocusAndLaunch
        );
        assert_eq!(
            AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
                focus_and_launch_profile_active: true,
                ..demand()
            }),
            AdaptivePowerProfile::FocusAndLaunch
        );
    }

    #[test]
    fn adaptive_profiles_scale_every_processor_control() {
        let idle = AdaptivePowerProfile::Idle.power_values().ac;
        let responsive = AdaptivePowerProfile::Responsive.power_values().ac;
        let background_pressure = AdaptivePowerProfile::BackgroundPressure.power_values().ac;
        let focus_and_launch = AdaptivePowerProfile::FocusAndLaunch.power_values().ac;

        assert!(idle.core_parking_min < responsive.core_parking_min);
        assert!(responsive.core_parking_min < background_pressure.core_parking_min);
        assert!(background_pressure.core_parking_min < focus_and_launch.core_parking_min);
        assert!(idle.performance_min < responsive.performance_min);
        assert!(responsive.performance_min < background_pressure.performance_min);
        assert!(background_pressure.performance_min < focus_and_launch.performance_min);
        assert!(idle.performance_max < responsive.performance_max);
        assert!(responsive.performance_max < background_pressure.performance_max);
        assert!(idle.boost_policy < responsive.boost_policy);
        assert!(responsive.boost_policy < background_pressure.boost_policy);
        assert!(background_pressure.boost_policy < focus_and_launch.boost_policy);
        assert_ne!(idle.boost_mode, responsive.boost_mode);
        assert_ne!(responsive.boost_mode, background_pressure.boost_mode);
        assert_ne!(background_pressure.boost_mode, focus_and_launch.boost_mode);
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
            AdaptivePowerProfile::Idle.calibrated_power_values(
                baseline,
                false,
                AdaptivePowerBoostValues::BACKGROUND_PRESSURE,
                AdaptivePowerBoostValues::FOCUS_AND_LAUNCH,
            ),
            ProcessorPowerSourceValues::same(baseline)
        );
        let focus_and_launch = AdaptivePowerProfile::FocusAndLaunch
            .calibrated_power_values(
                baseline,
                false,
                AdaptivePowerBoostValues::BACKGROUND_PRESSURE,
                AdaptivePowerBoostValues::FOCUS_AND_LAUNCH,
            )
            .ac;
        assert_eq!(focus_and_launch.performance_max, 100);
        assert!(focus_and_launch.core_parking_min >= baseline.core_parking_min);
        assert!(focus_and_launch.performance_min >= baseline.performance_min);
        assert!(focus_and_launch.boost_policy >= baseline.boost_policy);
    }

    #[test]
    fn adaptive_profiles_preserve_hybrid_turbo_headroom() {
        let baseline =
            ProcessorPowerValues::new_with_boost_mode(0, 5, 45, 0, ProcessorBoostMode::Disabled);
        let background_pressure = AdaptivePowerProfile::BackgroundPressure
            .calibrated_power_values(
                baseline,
                true,
                AdaptivePowerBoostValues::BACKGROUND_PRESSURE,
                AdaptivePowerBoostValues::FOCUS_AND_LAUNCH,
            )
            .ac;
        let focus_and_launch = AdaptivePowerProfile::FocusAndLaunch
            .calibrated_power_values(
                baseline,
                true,
                AdaptivePowerBoostValues::BACKGROUND_PRESSURE,
                AdaptivePowerBoostValues::FOCUS_AND_LAUNCH,
            )
            .ac;

        assert_eq!(
            (
                background_pressure.core_parking_min,
                background_pressure.performance_min
            ),
            (40, 15)
        );
        assert_eq!(
            (
                focus_and_launch.core_parking_min,
                focus_and_launch.performance_min
            ),
            (50, 20)
        );
        assert!(
            focus_and_launch.core_parking_min
                < AdaptivePowerProfile::FocusAndLaunch
                    .power_values()
                    .ac
                    .core_parking_min
        );
        assert_eq!(focus_and_launch.performance_max, 100);
        assert_eq!(focus_and_launch.boost_policy, 100);
        assert_eq!(focus_and_launch.boost_mode, ProcessorBoostMode::Aggressive);
    }

    #[test]
    fn adaptive_pressure_and_focus_boost_values_are_tunable() {
        let baseline =
            ProcessorPowerValues::new_with_boost_mode(0, 5, 45, 0, ProcessorBoostMode::Disabled);
        let background = AdaptivePowerBoostValues::new(
            72,
            ProcessorBoostMode::EfficientEnabled,
            42,
            ProcessorBoostMode::Disabled,
        );
        let focus = AdaptivePowerBoostValues::new(
            91,
            ProcessorBoostMode::AggressiveAtGuaranteed,
            63,
            ProcessorBoostMode::EfficientAggressiveAtGuaranteed,
        );

        let background_pressure = AdaptivePowerProfile::BackgroundPressure
            .calibrated_power_values(baseline, false, background, focus);
        let focus_and_launch = AdaptivePowerProfile::FocusAndLaunch
            .calibrated_power_values(baseline, false, background, focus);

        assert_eq!(
            (
                background_pressure.ac.boost_policy,
                background_pressure.ac.boost_mode
            ),
            (72, ProcessorBoostMode::EfficientEnabled)
        );
        assert_eq!(
            (
                background_pressure.battery.boost_policy,
                background_pressure.battery.boost_mode
            ),
            (42, ProcessorBoostMode::Disabled)
        );
        assert_eq!(
            (
                focus_and_launch.ac.boost_policy,
                focus_and_launch.ac.boost_mode
            ),
            (91, ProcessorBoostMode::AggressiveAtGuaranteed)
        );
        assert_eq!(
            (
                focus_and_launch.battery.boost_policy,
                focus_and_launch.battery.boost_mode
            ),
            (63, ProcessorBoostMode::EfficientAggressiveAtGuaranteed)
        );
    }

    #[test]
    fn adaptive_profiles_rise_immediately_and_fall_after_hysteresis() {
        assert_eq!(
            adaptive_power_profile_transition(
                AdaptivePowerProfile::Idle,
                AdaptivePowerProfile::FocusAndLaunch,
                Duration::ZERO,
            ),
            AdaptivePowerProfile::FocusAndLaunch
        );
        assert_eq!(
            adaptive_power_profile_transition(
                AdaptivePowerProfile::FocusAndLaunch,
                AdaptivePowerProfile::Idle,
                Duration::from_secs(4),
            ),
            AdaptivePowerProfile::FocusAndLaunch
        );
        assert_eq!(
            adaptive_power_profile_transition(
                AdaptivePowerProfile::FocusAndLaunch,
                AdaptivePowerProfile::Idle,
                ADAPTIVE_POWER_DEESCALATION_DELAY,
            ),
            AdaptivePowerProfile::Idle
        );
    }
}
