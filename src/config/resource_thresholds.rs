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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceThresholdsMetric {
    CpuPercent,
    MemoryPercent,
    MemoryMb,
}

impl ResourceThresholdsMetric {
    pub(crate) fn maximum(self) -> u64 {
        match self {
            Self::CpuPercent | Self::MemoryPercent => 100,
            Self::MemoryMb => 1_000_000,
        }
    }

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            Self::CpuPercent => "resource_thresholds.cpu",
            Self::MemoryPercent => "resource_thresholds.memory_percent",
            Self::MemoryMb => "resource_thresholds.memory_mb",
        }
    }

    pub(crate) fn unit(self) -> &'static str {
        match self {
            Self::CpuPercent | Self::MemoryPercent => "%",
            Self::MemoryMb => "MB",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceThresholdsMemoryUnit {
    Percent,
    Megabytes,
}

impl ResourceThresholdsMemoryUnit {
    pub const ALL: [Self; 2] = [Self::Percent, Self::Megabytes];

    pub fn metric(self) -> ResourceThresholdsMetric {
        match self {
            Self::Percent => ResourceThresholdsMetric::MemoryPercent,
            Self::Megabytes => ResourceThresholdsMetric::MemoryMb,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceThresholdsAction {
    None,
    Alert,
    Restart,
    Terminate,
}

impl ResourceThresholdsAction {
    pub(crate) fn mutates_process(self) -> bool {
        matches!(self, Self::Restart | Self::Terminate)
    }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceThresholdsMonitor {
    pub enabled: bool,
    pub warning: ResourceThresholdsResponse,
    pub critical: ResourceThresholdsResponse,
}

impl ResourceThresholdsMonitor {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            warning: ResourceThresholdsResponse {
                threshold: 70,
                action: ResourceThresholdsAction::Alert,
            },
            critical: ResourceThresholdsResponse {
                threshold: 90,
                action: ResourceThresholdsAction::Alert,
            },
        }
    }

    pub(crate) fn thresholds_valid(&self, metric: ResourceThresholdsMetric) -> bool {
        let warning = self.warning.threshold;
        let critical = self.critical.threshold;
        0 < warning && warning < critical && critical <= metric.maximum()
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "SavedResourceThresholdsRule<String, ResourceThresholdsMonitor>")]
pub struct ResourceThresholdsRule {
    pub enabled: bool,
    pub notify: bool,
    pub selector: ResourceThresholdsSelector,
    pub cpu: ResourceThresholdsMonitor,
    pub memory: ResourceThresholdsMonitor,
    pub memory_unit: ResourceThresholdsMemoryUnit,
}

// Explicit selector fields let the settings parser reject removed options.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedResourceThresholdsRule<S, M> {
    enabled: bool,
    #[serde(default = "default_notify")]
    notify: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    executable_path: Option<S>,
    #[serde(skip_serializing_if = "Option::is_none")]
    executable_name: Option<S>,
    memory_unit: ResourceThresholdsMemoryUnit,
    cpu: M,
    memory: M,
}

impl TryFrom<SavedResourceThresholdsRule<String, ResourceThresholdsMonitor>>
    for ResourceThresholdsRule
{
    type Error = &'static str;

    fn try_from(
        saved: SavedResourceThresholdsRule<String, ResourceThresholdsMonitor>,
    ) -> Result<Self, Self::Error> {
        let SavedResourceThresholdsRule {
            enabled,
            notify,
            executable_path,
            executable_name,
            memory_unit,
            cpu,
            memory,
        } = saved;
        let selector = match (executable_path, executable_name) {
            (Some(executable_path), None) => {
                ResourceThresholdsSelector::ExactPath { executable_path }
            }
            (None, Some(executable_name)) => {
                ResourceThresholdsSelector::ExactName { executable_name }
            }
            _ => return Err("Specify exactly one executable path or executable name."),
        };
        Ok(Self {
            enabled,
            notify,
            selector,
            cpu,
            memory,
            memory_unit,
        })
    }
}

impl Serialize for ResourceThresholdsRule {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (executable_path, executable_name) = match &self.selector {
            ResourceThresholdsSelector::ExactPath { executable_path } => {
                (Some(executable_path), None)
            }
            ResourceThresholdsSelector::ExactName { executable_name } => {
                (None, Some(executable_name))
            }
        };
        SavedResourceThresholdsRule {
            enabled: self.enabled,
            notify: self.notify,
            executable_path,
            executable_name,
            memory_unit: self.memory_unit,
            cpu: &self.cpu,
            memory: &self.memory,
        }
        .serialize(serializer)
    }
}

fn default_notify() -> bool {
    true
}

impl ResourceThresholdsRule {
    pub fn new(executable_path: String) -> Self {
        Self::with_selector(ResourceThresholdsSelector::ExactPath { executable_path })
    }

    pub fn with_selector(selector: ResourceThresholdsSelector) -> Self {
        Self {
            enabled: true,
            notify: true,
            selector,
            cpu: ResourceThresholdsMonitor::new(true),
            memory: ResourceThresholdsMonitor::new(false),
            memory_unit: ResourceThresholdsMemoryUnit::Percent,
        }
    }

    fn same_monitoring_policy(&self, other: &Self) -> bool {
        let Self {
            enabled,
            notify: _,
            selector,
            cpu,
            memory,
            memory_unit,
        } = self;
        *enabled == other.enabled
            && *selector == other.selector
            && *cpu == other.cpu
            && *memory == other.memory
            && *memory_unit == other.memory_unit
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

    pub fn allows_action(&self, action: ResourceThresholdsAction) -> bool {
        !action.mutates_process() || self.selector.exact_path().is_some()
    }

    pub(crate) fn responds_to(&self, action: ResourceThresholdsAction) -> bool {
        action != ResourceThresholdsAction::None && self.allows_action(action)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.selector.validate()?;
        if ResourceThresholdsResource::ALL.into_iter().any(|resource| {
            let monitor = self.monitor(resource);
            !self.allows_action(monitor.warning.action)
                || !self.allows_action(monitor.critical.action)
        }) {
            return Err(
                "Resource Thresholds requires an exact executable path for Terminate or Restart."
                    .into(),
            );
        }
        if !self.cpu.enabled && !self.memory.enabled {
            return Err("Resource Thresholds requires an enabled monitor.".into());
        }
        for resource in ResourceThresholdsResource::ALL {
            let monitor = self.monitor(resource);
            let metric = self.metric(resource);
            if monitor.enabled && !monitor.thresholds_valid(metric) {
                return Err(format!(
                    "Resource Thresholds requires 0 < warning < critical <= {} for {} thresholds.",
                    metric.maximum(),
                    metric.unit(),
                ));
            }
        }
        Ok(())
    }
}

impl ResourceThresholdsSettings {
    pub(crate) fn same_monitoring_policy(&self, other: &Self) -> bool {
        self.enabled == other.enabled
            && self.sustained_seconds == other.sustained_seconds
            && self.rules.len() == other.rules.len()
            && self
                .rules
                .iter()
                .zip(&other.rules)
                .all(|(rule, other)| rule.same_monitoring_policy(other))
    }

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
                    && rule.notify
                    && ResourceThresholdsResource::ALL.into_iter().any(|resource| {
                        let monitor = rule.monitor(resource);
                        monitor.enabled
                            && (rule.responds_to(monitor.warning.action)
                                || rule.responds_to(monitor.critical.action))
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
        assert_eq!(rule.cpu.warning.action, ResourceThresholdsAction::Alert);
        assert_eq!(rule.memory.critical.action, ResourceThresholdsAction::Alert);
        rule.memory.enabled = true;
        rule.memory.warning.threshold = 40;
        rule.memory.critical.threshold = 60;
        rule.cpu.warning.action = ResourceThresholdsAction::Restart;
        rule.memory.critical.action = ResourceThresholdsAction::Terminate;
        assert!(rule.validate().is_ok());
        let raw = toml::to_string(&rule).unwrap();
        assert!(raw.contains("executable_path = "));
        assert!(!raw.contains("scope = "));
        assert!(!raw.contains("comparison = "));
        assert!(raw.contains("memory_unit = \"percent\""));
        assert_eq!(
            toml::from_str::<ResourceThresholdsRule>(&raw).unwrap(),
            rule
        );
        rule.cpu.warning.threshold = 90;
        assert!(rule.validate().is_err());
        rule.cpu.warning.threshold = 70;
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
        settings.rules[0].notify = false;
        assert!(!settings.notifications_required());
        settings.rules[0].notify = true;
        settings.notify = false;
        assert!(!settings.notifications_required());
        let raw = toml::to_string(&settings).unwrap();
        assert_eq!(
            toml::from_str::<ResourceThresholdsSettings>(&raw).unwrap(),
            settings
        );
        settings.notify = true;
        settings.rules[0].memory.warning.action = ResourceThresholdsAction::None;
        settings.rules[0].memory.critical.action = ResourceThresholdsAction::None;
        assert!(!settings.notifications_required());
        settings.sustained_seconds = 0;
        assert!(settings.validate().is_err());
    }

    #[test]
    fn omitted_rule_notify_preserves_alerts_and_muted_rules_round_trip() {
        let mut rule = ResourceThresholdsRule::new(r"C:\Apps\test.exe".into());
        let raw = toml::to_string(&rule).unwrap();
        let saved_without_notify = raw.replace("notify = true\n", "");
        assert_ne!(raw, saved_without_notify);
        assert!(
            toml::from_str::<ResourceThresholdsRule>(&saved_without_notify)
                .unwrap()
                .notify
        );
        rule.notify = false;
        assert_eq!(
            toml::from_str::<ResourceThresholdsRule>(&toml::to_string(&rule).unwrap()).unwrap(),
            rule
        );
    }

    #[test]
    fn name_rule_round_trip_requires_one_flat_selector_and_no_process_actions() {
        let mut rule =
            ResourceThresholdsRule::with_selector(ResourceThresholdsSelector::ExactName {
                executable_name: "Worker Tool.EXE".to_owned(),
            });
        assert!(rule.validate().is_ok());
        let raw = toml::to_string(&rule).unwrap();
        let name_field = "executable_name = \"Worker Tool.EXE\"\n";
        assert!(raw.contains(name_field));
        assert!(raw.contains("action = \"alert\""));
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
    fn active_monitors_require_ordered_nonzero_percentages() {
        let mut rule = ResourceThresholdsRule::new(r"C:\Apps\worker.exe".into());
        rule.cpu.warning.threshold = 1;
        rule.cpu.critical.threshold = 100;
        rule.memory.enabled = true;
        assert!(rule.validate().is_ok());

        for resource in ResourceThresholdsResource::ALL {
            for (warning, critical) in [(90, 50), (0, 1), (50, 50), (50, 101)] {
                let mut invalid = rule.clone();
                let monitor = invalid.monitor_mut(resource);
                monitor.warning.threshold = warning;
                monitor.critical.threshold = critical;
                assert!(
                    invalid.validate().is_err(),
                    "{resource:?}: {warning}, {critical}"
                );
            }
        }
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
