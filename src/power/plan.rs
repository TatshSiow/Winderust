#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerPlan {
    pub guid: String,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessorPowerPreset {
    Performance,
    Balanced,
    Saver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessorBoostMode {
    Disabled,
    Enabled,
    Aggressive,
    EfficientEnabled,
    EfficientAggressive,
    AggressiveAtGuaranteed,
    EfficientAggressiveAtGuaranteed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessorPowerValues {
    pub core_parking_min: u32,
    pub performance_min: u32,
    pub performance_max: u32,
    pub boost_mode: ProcessorBoostMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessorPowerAcDcValues {
    pub ac: ProcessorPowerValues,
    pub dc: ProcessorPowerValues,
}

impl ProcessorPowerValues {
    pub const fn new(core_parking_min: u32, performance_min: u32, performance_max: u32) -> Self {
        Self::new_with_boost_mode(
            core_parking_min,
            performance_min,
            performance_max,
            ProcessorBoostMode::Enabled,
        )
    }

    pub const fn new_with_boost_mode(
        core_parking_min: u32,
        performance_min: u32,
        performance_max: u32,
        boost_mode: ProcessorBoostMode,
    ) -> Self {
        Self {
            core_parking_min,
            performance_min,
            performance_max,
            boost_mode,
        }
    }

    pub const fn for_preset(preset: ProcessorPowerPreset) -> Self {
        match preset {
            ProcessorPowerPreset::Performance => Self {
                core_parking_min: 100,
                performance_min: 100,
                performance_max: 100,
                boost_mode: ProcessorBoostMode::Aggressive,
            },
            ProcessorPowerPreset::Balanced => Self {
                core_parking_min: 50,
                performance_min: 5,
                performance_max: 100,
                boost_mode: ProcessorBoostMode::Enabled,
            },
            ProcessorPowerPreset::Saver => Self {
                core_parking_min: 0,
                performance_min: 5,
                performance_max: 80,
                boost_mode: ProcessorBoostMode::EfficientEnabled,
            },
        }
    }

    pub fn normalized(self) -> Self {
        let performance_min = self.performance_min.min(100);
        Self {
            core_parking_min: self.core_parking_min.min(100),
            performance_min,
            performance_max: self.performance_max.min(100).max(performance_min),
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

impl PowerPlan {
    pub fn display_name(&self) -> String {
        if self.active {
            format!("{} (active)", self.name)
        } else {
            self.name.clone()
        }
    }
}
