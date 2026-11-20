use std::path::Path;

use serde::{Deserialize, Serialize};

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
#[serde(untagged, try_from = "WatchdogSelectorFields")]
pub enum WatchdogSelector {
    ExactPath { executable_path: String },
    ExactName { executable_name: String },
}

#[derive(Deserialize)]
struct WatchdogSelectorFields {
    executable_path: Option<String>,
    executable_name: Option<String>,
}

impl TryFrom<WatchdogSelectorFields> for WatchdogSelector {
    type Error = &'static str;

    fn try_from(fields: WatchdogSelectorFields) -> Result<Self, Self::Error> {
        match (fields.executable_path, fields.executable_name) {
            (Some(executable_path), None) => Ok(Self::ExactPath { executable_path }),
            (None, Some(executable_name)) => Ok(Self::ExactName { executable_name }),
            _ => Err("Watchdog requires exactly one executable path or executable name."),
        }
    }
}

impl WatchdogSelector {
    pub fn value(&self) -> &str {
        match self {
            Self::ExactPath { executable_path } => executable_path,
            Self::ExactName { executable_name } => executable_name,
        }
    }

    pub fn allows_actions(&self) -> bool {
        matches!(self, Self::ExactPath { .. })
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::ExactPath { executable_path } if Path::new(executable_path).is_absolute() => {
                Ok(())
            }
            Self::ExactPath { .. } => Err("Watchdog requires an absolute executable path.".into()),
            Self::ExactName { executable_name }
                if executable_name.trim() == executable_name
                    && !executable_name.chars().any(|character| {
                        character.is_ascii_control() || r#"<>:"/\|?*"#.contains(character)
                    })
                    && Path::new(executable_name).file_name().is_some() =>
            {
                Ok(())
            }
            Self::ExactName { .. } => Err("Watchdog requires an exact executable filename.".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchdogRule {
    pub enabled: bool,
    #[serde(flatten)]
    pub selector: WatchdogSelector,
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

    pub fn validate(&self) -> Result<(), String> {
        self.selector.validate()?;
        if !self.selector.allows_actions()
            && [&self.cpu, &self.memory].iter().any(|monitor| {
                monitor.warning.action != WatchdogAction::None
                    || monitor.critical.action != WatchdogAction::None
            })
        {
            return Err(
                "Watchdog requires an exact executable path for Terminate or Restart.".into(),
            );
        }
        if self
            .sustained_seconds
            .is_some_and(|seconds| !WATCHDOG_DURATION_RANGE.contains(&seconds))
        {
            return Err("Watchdog rule sustained usage must be between 1 and 3600 seconds.".into());
        }
        if (!self.cpu.enabled && !self.memory.enabled)
            || (self.cpu.enabled && !self.cpu.validate(WatchdogMetric::CpuPercent))
            || (self.memory.enabled && !self.memory.validate(self.memory_unit.metric()))
        {
            return Err("Watchdog requires CPU or memory monitoring and thresholds with 0 < warning < critical within each resource's range.".into());
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
