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
pub struct ProcessorPowerValues {
    pub core_parking_min: u32,
    pub performance_min: u32,
    pub performance_max: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessorPowerAcDcValues {
    pub ac: ProcessorPowerValues,
    pub dc: ProcessorPowerValues,
}

impl ProcessorPowerValues {
    pub const fn new(core_parking_min: u32, performance_min: u32, performance_max: u32) -> Self {
        Self {
            core_parking_min,
            performance_min,
            performance_max,
        }
    }

    pub const fn for_preset(preset: ProcessorPowerPreset) -> Self {
        match preset {
            ProcessorPowerPreset::Performance => Self {
                core_parking_min: 100,
                performance_min: 100,
                performance_max: 100,
            },
            ProcessorPowerPreset::Balanced => Self {
                core_parking_min: 50,
                performance_min: 5,
                performance_max: 100,
            },
            ProcessorPowerPreset::Saver => Self {
                core_parking_min: 0,
                performance_min: 5,
                performance_max: 80,
            },
        }
    }

    pub fn normalized(self) -> Self {
        let performance_min = self.performance_min.min(100);
        Self {
            core_parking_min: self.core_parking_min.min(100),
            performance_min,
            performance_max: self.performance_max.min(100).max(performance_min),
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
