use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchdogSettings {
    pub enabled: bool,
    pub sustained_seconds: u64,
    pub notify: bool,
    pub rules: Vec<WatchdogRule>,
}

impl Default for WatchdogSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            sustained_seconds: 5,
            notify: true,
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogMetric {
    CpuPercent,
    MemoryPercent,
    MemoryMib,
}

impl WatchdogMetric {
    pub fn maximum(self) -> u64 {
        match self {
            Self::CpuPercent | Self::MemoryPercent => 100,
            Self::MemoryMib => 1_048_576,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogAction {
    None,
    Terminate,
    Restart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchdogResponse {
    pub threshold: u64,
    pub action: WatchdogAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogResource {
    Cpu,
    Memory,
}

impl WatchdogResource {
    pub const ALL: [Self; 2] = [Self::Cpu, Self::Memory];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogMemoryUnit {
    Percent,
    Mib,
}

impl WatchdogMemoryUnit {
    pub const ALL: [Self; 2] = [Self::Percent, Self::Mib];

    pub fn metric(self) -> WatchdogMetric {
        match self {
            Self::Percent => WatchdogMetric::MemoryPercent,
            Self::Mib => WatchdogMetric::MemoryMib,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchdogMonitor {
    pub enabled: bool,
    pub warning: WatchdogResponse,
    pub critical: WatchdogResponse,
}

impl WatchdogMonitor {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            warning: WatchdogResponse {
                threshold: 70,
                action: WatchdogAction::None,
            },
            critical: WatchdogResponse {
                threshold: 90,
                action: WatchdogAction::None,
            },
        }
    }

    fn validate(&self, metric: WatchdogMetric) -> bool {
        self.warning.threshold > 0
            && self.warning.threshold < self.critical.threshold
            && self.critical.threshold <= metric.maximum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchdogRule {
    pub enabled: bool,
    pub executable_path: String,
    pub cpu: WatchdogMonitor,
    pub memory: WatchdogMonitor,
    pub memory_unit: WatchdogMemoryUnit,
}

impl WatchdogRule {
    pub fn new(executable_path: String) -> Self {
        Self {
            enabled: true,
            executable_path,
            cpu: WatchdogMonitor::new(true),
            memory: WatchdogMonitor::new(false),
            memory_unit: WatchdogMemoryUnit::Percent,
        }
    }

    pub fn monitor(&self, resource: WatchdogResource) -> &WatchdogMonitor {
        match resource {
            WatchdogResource::Cpu => &self.cpu,
            WatchdogResource::Memory => &self.memory,
        }
    }

    pub fn monitor_mut(&mut self, resource: WatchdogResource) -> &mut WatchdogMonitor {
        match resource {
            WatchdogResource::Cpu => &mut self.cpu,
            WatchdogResource::Memory => &mut self.memory,
        }
    }

    pub fn metric(&self, resource: WatchdogResource) -> WatchdogMetric {
        match resource {
            WatchdogResource::Cpu => WatchdogMetric::CpuPercent,
            WatchdogResource::Memory => self.memory_unit.metric(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !std::path::Path::new(&self.executable_path).is_absolute()
            || (!self.cpu.enabled && !self.memory.enabled)
            || (self.cpu.enabled && !self.cpu.validate(WatchdogMetric::CpuPercent))
            || (self.memory.enabled && !self.memory.validate(self.memory_unit.metric()))
        {
            return Err("Watchdog requires an absolute executable path, CPU or memory monitoring, and thresholds with 0 < warning < critical within each resource's range.".into());
        }
        Ok(())
    }
}

impl WatchdogSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=3600).contains(&self.sustained_seconds) {
            return Err("Watchdog sustained usage must be between 1 and 3600 seconds.".into());
        }
        for rule in &self.rules {
            rule.validate()?;
        }
        Ok(())
    }

    pub fn notifications_required(&self) -> bool {
        self.enabled
            && self.notify
            && self.rules.iter().any(|rule| {
                rule.enabled
                    && WatchdogResource::ALL.into_iter().any(|resource| {
                        let monitor = rule.monitor(resource);
                        monitor.enabled
                    })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_resources_round_trip_with_independent_thresholds_and_actions() {
        let mut rule = WatchdogRule::new(r"C:\Apps\test.exe".into());
        assert_eq!(rule.cpu.warning.action, WatchdogAction::None);
        assert_eq!(rule.memory.critical.action, WatchdogAction::None);
        rule.memory.enabled = true;
        rule.memory_unit = WatchdogMemoryUnit::Mib;
        rule.memory.warning.threshold = 1024;
        rule.memory.critical.threshold = 2048;
        rule.cpu.warning.action = WatchdogAction::Restart;
        rule.memory.critical.action = WatchdogAction::Terminate;
        assert!(rule.validate().is_ok());
        let raw = toml::to_string(&rule).unwrap();
        assert_eq!(toml::from_str::<WatchdogRule>(&raw).unwrap(), rule);
        rule.cpu.warning.threshold = 90;
        assert!(rule.validate().is_err());
        rule.cpu.warning.threshold = 70;
        rule.memory_unit = WatchdogMemoryUnit::Percent;
        assert!(rule.validate().is_err());
        rule.memory.warning.threshold = 70;
        rule.memory.critical.threshold = 90;
        rule.cpu.enabled = false;
        rule.memory.enabled = false;
        assert!(rule.validate().is_err());
        rule.memory.enabled = true;
        let mut settings = WatchdogSettings {
            enabled: true,
            rules: vec![rule],
            ..Default::default()
        };
        assert!(settings.notifications_required());
        settings.notify = false;
        assert!(!settings.notifications_required());
        let raw = toml::to_string(&settings).unwrap();
        assert_eq!(toml::from_str::<WatchdogSettings>(&raw).unwrap(), settings);
        settings.sustained_seconds = 0;
        assert!(settings.validate().is_err());
    }
}
