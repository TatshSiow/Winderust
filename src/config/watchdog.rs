use std::path::Path;

use serde::{Deserialize, Serialize};

use super::is_valid_executable_name;
pub use super::ExecutableSelector as WatchdogSelector;

pub(crate) const WATCHDOG_DURATION_RANGE: std::ops::RangeInclusive<u64> = 1..=3600;

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
    MemoryMb,
    PrivateCommitMb,
}

impl WatchdogMetric {
    pub fn maximum(self) -> u64 {
        match self {
            Self::CpuPercent | Self::MemoryPercent => 100,
            Self::MemoryMb | Self::PrivateCommitMb => 1_000_000,
        }
    }

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            Self::CpuPercent => "watchdog.cpu",
            Self::MemoryPercent => "watchdog.memory_percent",
            Self::MemoryMb => "watchdog.memory_mb",
            Self::PrivateCommitMb => "watchdog.private_commit_mb",
        }
    }

    pub(crate) fn unit(self) -> &'static str {
        match self {
            Self::CpuPercent | Self::MemoryPercent => "%",
            Self::MemoryMb | Self::PrivateCommitMb => "MB",
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
    Megabytes,
    PrivateCommitMb,
}

impl WatchdogMemoryUnit {
    pub const ALL: [Self; 3] = [Self::Percent, Self::Megabytes, Self::PrivateCommitMb];

    pub fn metric(self) -> WatchdogMetric {
        match self {
            Self::Percent => WatchdogMetric::MemoryPercent,
            Self::Megabytes => WatchdogMetric::MemoryMb,
            Self::PrivateCommitMb => WatchdogMetric::PrivateCommitMb,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogComparison {
    #[default]
    AtOrAbove,
    AtOrBelow,
}

impl WatchdogComparison {
    fn is_at_or_above(&self) -> bool {
        *self == Self::AtOrAbove
    }

    pub(crate) fn symbol(self) -> &'static str {
        match self {
            Self::AtOrAbove => "≥",
            Self::AtOrBelow => "≤",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchdogMonitor {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "WatchdogComparison::is_at_or_above")]
    pub comparison: WatchdogComparison,
    pub warning: WatchdogResponse,
    pub critical: WatchdogResponse,
}

impl WatchdogMonitor {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            comparison: WatchdogComparison::default(),
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

    pub(crate) fn thresholds_valid(&self, metric: WatchdogMetric) -> bool {
        let warning = self.warning.threshold;
        let critical = self.critical.threshold;
        match self.comparison {
            WatchdogComparison::AtOrAbove => {
                0 < warning && warning < critical && critical <= metric.maximum()
            }
            WatchdogComparison::AtOrBelow => critical < warning && warning < metric.maximum(),
        }
    }
}

impl WatchdogSelector {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::ExactPath { executable_path } if Path::new(executable_path).is_absolute() => {
                Ok(())
            }
            Self::ExactPath { .. } => Err("Watchdog requires an absolute executable path.".into()),
            Self::ExactName { executable_name } if is_valid_executable_name(executable_name) => {
                Ok(())
            }
            Self::ExactName { .. } => Err("Watchdog requires an exact executable filename.".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogScope {
    #[default]
    Combined,
    PerInstance,
}

impl WatchdogScope {
    fn is_combined(&self) -> bool {
        *self == Self::Combined
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchdogRule {
    pub enabled: bool,
    #[serde(flatten)]
    pub selector: WatchdogSelector,
    #[serde(default, skip_serializing_if = "WatchdogScope::is_combined")]
    pub scope: WatchdogScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sustained_seconds: Option<u64>,
    pub cpu: WatchdogMonitor,
    pub memory: WatchdogMonitor,
    pub memory_unit: WatchdogMemoryUnit,
}

impl WatchdogRule {
    pub fn new(executable_path: String) -> Self {
        Self::with_selector(WatchdogSelector::ExactPath { executable_path })
    }

    pub fn with_selector(selector: WatchdogSelector) -> Self {
        Self {
            enabled: true,
            selector,
            scope: WatchdogScope::default(),
            sustained_seconds: None,
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

    pub fn allows_actions(&self, resource: WatchdogResource) -> bool {
        self.scope.is_combined()
            && matches!(self.selector, WatchdogSelector::ExactPath { .. })
            && self.monitor(resource).comparison.is_at_or_above()
    }

    pub fn validate(&self) -> Result<(), String> {
        self.selector.validate()?;
        if WatchdogResource::ALL.into_iter().any(|resource| {
            let monitor = self.monitor(resource);
            !self.allows_actions(resource)
                && (monitor.warning.action != WatchdogAction::None
                    || monitor.critical.action != WatchdogAction::None)
        }) {
            return Err(
                "Watchdog requires At or above, Combined by path, and an exact executable path for Terminate or Restart.".into(),
            );
        }
        if self
            .sustained_seconds
            .is_some_and(|seconds| !WATCHDOG_DURATION_RANGE.contains(&seconds))
        {
            return Err("Watchdog rule sustained usage must be between 1 and 3600 seconds.".into());
        }
        if (!self.cpu.enabled && !self.memory.enabled)
            || (self.cpu.enabled && !self.cpu.thresholds_valid(WatchdogMetric::CpuPercent))
            || (self.memory.enabled && !self.memory.thresholds_valid(self.memory_unit.metric()))
        {
            return Err("Watchdog requires an enabled monitor and valid thresholds. At or above requires 0 < warning < critical up to the metric maximum. At or below requires critical < warning below the maximum.".into());
        }
        Ok(())
    }
}

impl WatchdogSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !WATCHDOG_DURATION_RANGE.contains(&self.sustained_seconds) {
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
        rule.memory_unit = WatchdogMemoryUnit::Megabytes;
        rule.memory.warning.threshold = 1000;
        rule.memory.critical.threshold = 2000;
        rule.cpu.warning.action = WatchdogAction::Restart;
        rule.memory.critical.action = WatchdogAction::Terminate;
        assert!(rule.validate().is_ok());
        let raw = toml::to_string(&rule).unwrap();
        assert!(raw.contains("executable_path = "));
        assert!(!raw.contains("scope = "));
        assert!(!raw.contains("comparison = "));
        assert!(raw.contains("memory_unit = \"megabytes\""));
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

    #[test]
    fn name_rule_round_trip_requires_one_flat_selector_and_notification_only() {
        let mut rule = WatchdogRule::with_selector(WatchdogSelector::ExactName {
            executable_name: "Worker Tool.EXE".to_owned(),
        });
        assert!(rule.validate().is_ok());
        let raw = toml::to_string(&rule).unwrap();
        let name_field = "executable_name = \"Worker Tool.EXE\"\n";
        assert!(raw.contains(name_field));
        assert!(!raw.contains("executable_path"));
        assert_eq!(toml::from_str::<WatchdogRule>(&raw).unwrap(), rule);
        let both = format!("executable_path = 'C:\\Apps\\worker.exe'\n{raw}");
        assert!(toml::from_str::<WatchdogRule>(&both).is_err());
        let neither = raw.replace(name_field, "");
        assert!(toml::from_str::<WatchdogRule>(&neither).is_err());

        for action in [WatchdogAction::Terminate, WatchdogAction::Restart] {
            rule.memory.warning.action = action;
            let imported: WatchdogRule = toml::from_str(&toml::to_string(&rule).unwrap()).unwrap();
            assert!(!imported.memory.enabled);
            assert!(imported.validate().is_err());
        }
    }

    #[test]
    fn per_instance_scope_round_trips_and_rejects_automatic_actions() {
        let mut rule = WatchdogRule::new(r"C:\Apps\test.exe".into());
        rule.scope = WatchdogScope::PerInstance;
        assert!(rule.validate().is_ok());
        let raw = toml::to_string(&rule).unwrap();
        assert!(raw.contains("scope = \"per_instance\""));
        assert_eq!(toml::from_str::<WatchdogRule>(&raw).unwrap(), rule);
        assert!(toml::from_str::<WatchdogRule>(&raw.replace("per_instance", "unknown")).is_err());

        for (resource, action) in [
            (WatchdogResource::Cpu, WatchdogAction::Terminate),
            (WatchdogResource::Memory, WatchdogAction::Restart),
        ] {
            let mut invalid = rule.clone();
            invalid.monitor_mut(resource).warning.action = action;
            assert!(invalid.validate().is_err());
        }
    }

    #[test]
    fn low_thresholds_round_trip_and_disable_only_their_own_actions() {
        let mut rule = WatchdogRule::new(r"C:\Apps\worker.exe".into());
        rule.cpu.comparison = WatchdogComparison::AtOrBelow;
        rule.cpu.warning.threshold = 99;
        rule.cpu.critical.threshold = 0;
        rule.memory.enabled = true;
        rule.memory.warning.action = WatchdogAction::Restart;
        assert!(rule.validate().is_ok());
        let raw = toml::to_string(&rule).unwrap();
        assert!(raw.contains("comparison = \"at_or_below\""));
        assert_eq!(toml::from_str::<WatchdogRule>(&raw).unwrap(), rule);
        assert!(toml::from_str::<WatchdogRule>(&raw.replace("at_or_below", "unknown")).is_err());

        for (warning, critical) in [(100, 0), (0, 0), (50, 50), (50, 90)] {
            let mut invalid = rule.clone();
            invalid.cpu.warning.threshold = warning;
            invalid.cpu.critical.threshold = critical;
            assert!(invalid.validate().is_err(), "{warning}, {critical}");
        }
        rule.cpu.enabled = false;
        rule.cpu.critical.action = WatchdogAction::Terminate;
        assert!(rule.validate().is_err());
    }

    #[test]
    fn exact_executable_name_rejects_paths_patterns_and_invalid_names() {
        for invalid in [
            "",
            ".",
            "..",
            "worker.exe ",
            r"C:\Apps\worker.exe",
            "Apps/worker.exe",
            "worker*.exe",
            "worker?.exe",
            "work:er.exe",
            "work\0er.exe",
        ] {
            assert!(
                WatchdogSelector::ExactName {
                    executable_name: invalid.to_owned(),
                }
                .validate()
                .is_err(),
                "accepted {invalid:?}"
            );
        }
        assert!(WatchdogSelector::ExactName {
            executable_name: "worker.scr".into(),
        }
        .validate()
        .is_ok());
    }
}
