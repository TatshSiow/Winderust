use std::path::Path;

use serde::{Deserialize, Serialize};

use super::is_valid_executable_name;
pub use super::ExecutableSelector as ResourceThresholdsSelector;

pub(crate) const RESOURCE_THRESHOLDS_DURATION_RANGE: std::ops::RangeInclusive<u64> = 1..=3600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceThresholdsSettings {
    pub enabled: bool,
    pub sustained_seconds: u64,
    pub notify: bool,
    pub rules: Vec<ResourceThresholdsRule>,
}

impl Default for ResourceThresholdsSettings {
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
pub enum ResourceThresholdsMetric {
    CpuPercent,
    MemoryPercent,
    MemoryMb,
    PrivateCommitMb,
}

impl ResourceThresholdsMetric {
    pub fn maximum(self) -> u64 {
        match self {
            Self::CpuPercent | Self::MemoryPercent => 100,
            Self::MemoryMb | Self::PrivateCommitMb => 1_000_000,
        }
    }

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            Self::CpuPercent => "resource_thresholds.cpu",
            Self::MemoryPercent => "resource_thresholds.memory_percent",
            Self::MemoryMb => "resource_thresholds.memory_mb",
            Self::PrivateCommitMb => "resource_thresholds.private_commit_mb",
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
pub enum ResourceThresholdsAction {
    None,
    Terminate,
    Restart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceThresholdsResponse {
    pub threshold: u64,
    pub action: ResourceThresholdsAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceThresholdsResource {
    Cpu,
    Memory,
}

impl ResourceThresholdsResource {
    pub const ALL: [Self; 2] = [Self::Cpu, Self::Memory];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceThresholdsMemoryUnit {
    Percent,
    Megabytes,
    PrivateCommitMb,
}

impl ResourceThresholdsMemoryUnit {
    pub const ALL: [Self; 3] = [Self::Percent, Self::Megabytes, Self::PrivateCommitMb];

    pub fn metric(self) -> ResourceThresholdsMetric {
        match self {
            Self::Percent => ResourceThresholdsMetric::MemoryPercent,
            Self::Megabytes => ResourceThresholdsMetric::MemoryMb,
            Self::PrivateCommitMb => ResourceThresholdsMetric::PrivateCommitMb,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceThresholdsComparison {
    #[default]
    AtOrAbove,
    AtOrBelow,
}

impl ResourceThresholdsComparison {
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
pub struct ResourceThresholdsMonitor {
    pub enabled: bool,
    #[serde(
        default,
        skip_serializing_if = "ResourceThresholdsComparison::is_at_or_above"
    )]
    pub comparison: ResourceThresholdsComparison,
    pub warning: ResourceThresholdsResponse,
    pub critical: ResourceThresholdsResponse,
}

impl ResourceThresholdsMonitor {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            comparison: ResourceThresholdsComparison::default(),
            warning: ResourceThresholdsResponse {
                threshold: 70,
                action: ResourceThresholdsAction::None,
            },
            critical: ResourceThresholdsResponse {
                threshold: 90,
                action: ResourceThresholdsAction::None,
            },
        }
    }

    pub(crate) fn thresholds_valid(&self, metric: ResourceThresholdsMetric) -> bool {
        let warning = self.warning.threshold;
        let critical = self.critical.threshold;
        match self.comparison {
            ResourceThresholdsComparison::AtOrAbove => {
                0 < warning && warning < critical && critical <= metric.maximum()
            }
            ResourceThresholdsComparison::AtOrBelow => {
                critical < warning && warning < metric.maximum()
            }
        }
    }
}

impl ResourceThresholdsSelector {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::ExactPath { executable_path } if Path::new(executable_path).is_absolute() => {
                Ok(())
            }
            Self::ExactPath { .. } => {
                Err("Resource Thresholds requires an absolute executable path.".into())
            }
            Self::ExactName { executable_name } if is_valid_executable_name(executable_name) => {
                Ok(())
            }
            Self::ExactName { .. } => {
                Err("Resource Thresholds requires an exact executable filename.".into())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceThresholdsScope {
    #[default]
    Combined,
    PerInstance,
}

impl ResourceThresholdsScope {
    fn is_combined(&self) -> bool {
        *self == Self::Combined
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceThresholdsRule {
    pub enabled: bool,
    #[serde(flatten)]
    pub selector: ResourceThresholdsSelector,
    #[serde(default, skip_serializing_if = "ResourceThresholdsScope::is_combined")]
    pub scope: ResourceThresholdsScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sustained_seconds: Option<u64>,
    pub cpu: ResourceThresholdsMonitor,
    pub memory: ResourceThresholdsMonitor,
    pub memory_unit: ResourceThresholdsMemoryUnit,
}

impl ResourceThresholdsRule {
    pub fn new(executable_path: String) -> Self {
        Self::with_selector(ResourceThresholdsSelector::ExactPath { executable_path })
    }

    pub fn with_selector(selector: ResourceThresholdsSelector) -> Self {
        Self {
            enabled: true,
            selector,
            scope: ResourceThresholdsScope::default(),
            sustained_seconds: None,
            cpu: ResourceThresholdsMonitor::new(true),
            memory: ResourceThresholdsMonitor::new(false),
            memory_unit: ResourceThresholdsMemoryUnit::Percent,
        }
    }

    pub fn monitor(&self, resource: ResourceThresholdsResource) -> &ResourceThresholdsMonitor {
        match resource {
            ResourceThresholdsResource::Cpu => &self.cpu,
            ResourceThresholdsResource::Memory => &self.memory,
        }
    }

    pub fn monitor_mut(
        &mut self,
        resource: ResourceThresholdsResource,
    ) -> &mut ResourceThresholdsMonitor {
        match resource {
            ResourceThresholdsResource::Cpu => &mut self.cpu,
            ResourceThresholdsResource::Memory => &mut self.memory,
        }
    }

    pub fn metric(&self, resource: ResourceThresholdsResource) -> ResourceThresholdsMetric {
        match resource {
            ResourceThresholdsResource::Cpu => ResourceThresholdsMetric::CpuPercent,
            ResourceThresholdsResource::Memory => self.memory_unit.metric(),
        }
    }

    pub fn allows_actions(&self, resource: ResourceThresholdsResource) -> bool {
        self.scope.is_combined()
            && matches!(self.selector, ResourceThresholdsSelector::ExactPath { .. })
            && self.monitor(resource).comparison.is_at_or_above()
    }

    pub fn validate(&self) -> Result<(), String> {
        self.selector.validate()?;
        if ResourceThresholdsResource::ALL.into_iter().any(|resource| {
            let monitor = self.monitor(resource);
            !self.allows_actions(resource)
                && (monitor.warning.action != ResourceThresholdsAction::None
                    || monitor.critical.action != ResourceThresholdsAction::None)
        }) {
            return Err(
                "Resource Thresholds requires At or above, Combined by path, and an exact executable path for Terminate or Restart.".into(),
            );
        }
        if self
            .sustained_seconds
            .is_some_and(|seconds| !RESOURCE_THRESHOLDS_DURATION_RANGE.contains(&seconds))
        {
            return Err(
                "Resource Thresholds rule sustained usage must be between 1 and 3600 seconds."
                    .into(),
            );
        }
        if (!self.cpu.enabled && !self.memory.enabled)
            || (self.cpu.enabled
                && !self
                    .cpu
                    .thresholds_valid(ResourceThresholdsMetric::CpuPercent))
            || (self.memory.enabled && !self.memory.thresholds_valid(self.memory_unit.metric()))
        {
            return Err("Resource Thresholds requires an enabled monitor and valid thresholds. At or above requires 0 < warning < critical up to the metric maximum. At or below requires critical < warning below the maximum.".into());
        }
        Ok(())
    }
}

impl ResourceThresholdsSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !RESOURCE_THRESHOLDS_DURATION_RANGE.contains(&self.sustained_seconds) {
            return Err(
                "Resource Thresholds sustained usage must be between 1 and 3600 seconds.".into(),
            );
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
                    && ResourceThresholdsResource::ALL.into_iter().any(|resource| {
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
        let mut rule = ResourceThresholdsRule::new(r"C:\Apps\test.exe".into());
        assert_eq!(rule.cpu.warning.action, ResourceThresholdsAction::None);
        assert_eq!(rule.memory.critical.action, ResourceThresholdsAction::None);
        rule.memory.enabled = true;
        rule.memory_unit = ResourceThresholdsMemoryUnit::Megabytes;
        rule.memory.warning.threshold = 1000;
        rule.memory.critical.threshold = 2000;
        rule.cpu.warning.action = ResourceThresholdsAction::Restart;
        rule.memory.critical.action = ResourceThresholdsAction::Terminate;
        assert!(rule.validate().is_ok());
        let raw = toml::to_string(&rule).unwrap();
        assert!(raw.contains("executable_path = "));
        assert!(!raw.contains("scope = "));
        assert!(!raw.contains("comparison = "));
        assert!(raw.contains("memory_unit = \"megabytes\""));
        assert_eq!(
            toml::from_str::<ResourceThresholdsRule>(&raw).unwrap(),
            rule
        );
        rule.cpu.warning.threshold = 90;
        assert!(rule.validate().is_err());
        rule.cpu.warning.threshold = 70;
        rule.memory_unit = ResourceThresholdsMemoryUnit::Percent;
        assert!(rule.validate().is_err());
        rule.memory.warning.threshold = 70;
        rule.memory.critical.threshold = 90;
        rule.cpu.enabled = false;
        rule.memory.enabled = false;
        assert!(rule.validate().is_err());
        rule.memory.enabled = true;
        let mut settings = ResourceThresholdsSettings {
            enabled: true,
            rules: vec![rule],
            ..Default::default()
        };
        assert!(settings.notifications_required());
        settings.notify = false;
        assert!(!settings.notifications_required());
        let raw = toml::to_string(&settings).unwrap();
        assert_eq!(
            toml::from_str::<ResourceThresholdsSettings>(&raw).unwrap(),
            settings
        );
        settings.sustained_seconds = 0;
        assert!(settings.validate().is_err());
    }

    #[test]
    fn name_rule_round_trip_requires_one_flat_selector_and_notification_only() {
        let mut rule =
            ResourceThresholdsRule::with_selector(ResourceThresholdsSelector::ExactName {
                executable_name: "Worker Tool.EXE".to_owned(),
            });
        assert!(rule.validate().is_ok());
        let raw = toml::to_string(&rule).unwrap();
        let name_field = "executable_name = \"Worker Tool.EXE\"\n";
        assert!(raw.contains(name_field));
        assert!(!raw.contains("executable_path"));
        assert_eq!(
            toml::from_str::<ResourceThresholdsRule>(&raw).unwrap(),
            rule
        );
        let both = format!("executable_path = 'C:\\Apps\\worker.exe'\n{raw}");
        assert!(toml::from_str::<ResourceThresholdsRule>(&both).is_err());
        let neither = raw.replace(name_field, "");
        assert!(toml::from_str::<ResourceThresholdsRule>(&neither).is_err());

        for action in [
            ResourceThresholdsAction::Terminate,
            ResourceThresholdsAction::Restart,
        ] {
            rule.memory.warning.action = action;
            let imported: ResourceThresholdsRule =
                toml::from_str(&toml::to_string(&rule).unwrap()).unwrap();
            assert!(!imported.memory.enabled);
            assert!(imported.validate().is_err());
        }
    }

    #[test]
    fn per_instance_scope_round_trips_and_rejects_automatic_actions() {
        let mut rule = ResourceThresholdsRule::new(r"C:\Apps\test.exe".into());
        rule.scope = ResourceThresholdsScope::PerInstance;
        assert!(rule.validate().is_ok());
        let raw = toml::to_string(&rule).unwrap();
        assert!(raw.contains("scope = \"per_instance\""));
        assert_eq!(
            toml::from_str::<ResourceThresholdsRule>(&raw).unwrap(),
            rule
        );
        assert!(
            toml::from_str::<ResourceThresholdsRule>(&raw.replace("per_instance", "unknown"))
                .is_err()
        );

        for (resource, action) in [
            (
                ResourceThresholdsResource::Cpu,
                ResourceThresholdsAction::Terminate,
            ),
            (
                ResourceThresholdsResource::Memory,
                ResourceThresholdsAction::Restart,
            ),
        ] {
            let mut invalid = rule.clone();
            invalid.monitor_mut(resource).warning.action = action;
            assert!(invalid.validate().is_err());
        }
    }

    #[test]
    fn low_thresholds_round_trip_and_disable_only_their_own_actions() {
        let mut rule = ResourceThresholdsRule::new(r"C:\Apps\worker.exe".into());
        rule.cpu.comparison = ResourceThresholdsComparison::AtOrBelow;
        rule.cpu.warning.threshold = 99;
        rule.cpu.critical.threshold = 0;
        rule.memory.enabled = true;
        rule.memory.warning.action = ResourceThresholdsAction::Restart;
        assert!(rule.validate().is_ok());
        let raw = toml::to_string(&rule).unwrap();
        assert!(raw.contains("comparison = \"at_or_below\""));
        assert_eq!(
            toml::from_str::<ResourceThresholdsRule>(&raw).unwrap(),
            rule
        );
        assert!(
            toml::from_str::<ResourceThresholdsRule>(&raw.replace("at_or_below", "unknown"))
                .is_err()
        );

        for (warning, critical) in [(100, 0), (0, 0), (50, 50), (50, 90)] {
            let mut invalid = rule.clone();
            invalid.cpu.warning.threshold = warning;
            invalid.cpu.critical.threshold = critical;
            assert!(invalid.validate().is_err(), "{warning}, {critical}");
        }
        rule.cpu.enabled = false;
        rule.cpu.critical.action = ResourceThresholdsAction::Terminate;
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
                ResourceThresholdsSelector::ExactName {
                    executable_name: invalid.to_owned(),
                }
                .validate()
                .is_err(),
                "accepted {invalid:?}"
            );
        }
        assert!(ResourceThresholdsSelector::ExactName {
            executable_name: "worker.scr".into(),
        }
        .validate()
        .is_ok());
    }
}
