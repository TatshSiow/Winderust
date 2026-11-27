use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceLimitsSettings {
    pub enabled: bool,
    #[serde(default = "default_notify")]
    pub notify: bool,
    pub rules: Vec<InstanceLimitsRule>,
}

fn default_notify() -> bool {
    true
}

impl Default for InstanceLimitsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            notify: true,
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceLimitsRule {
    pub enabled: bool,
    #[serde(default = "default_notify")]
    pub notify: bool,
    pub executable_path: String,
    pub max_instances: u32,
}

impl InstanceLimitsRule {
    pub fn new(executable_path: String) -> Self {
        Self {
            enabled: true,
            notify: default_notify(),
            executable_path,
            max_instances: 1,
        }
    }

    pub(crate) fn same_monitor(&self, other: &Self) -> bool {
        self.enabled == other.enabled
            && self.executable_path == other.executable_path
            && self.max_instances == other.max_instances
    }

    pub fn validate(&self) -> Result<(), String> {
        use crate::{control::instance_limits, foreground};
        instance_limits::validate_path(&self.executable_path)?;
        if self.max_instances == 0 {
            return Err("Instance Limits requires a maximum of at least one instance.".into());
        }
        let name = self
            .executable_path
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or_default();
        if name.eq_ignore_ascii_case("winderust.exe")
            || foreground::contains_process_name(foreground::CORE_BUILT_IN_PROCESS_EXCLUSIONS, name)
        {
            return Err(
                "Instance Limits cannot monitor Winderust or built-in protected processes.".into(),
            );
        }
        let own_path = std::env::current_exe()
            .map_err(|error| format!("Cannot verify Winderust's executable path: {error}"))?;
        let own_path = own_path
            .to_str()
            .ok_or("Winderust's executable path contains unsupported Unicode.")?;
        if instance_limits::paths_equal(&self.executable_path, own_path)? {
            return Err("Instance Limits cannot monitor Winderust itself.".into());
        }
        Ok(())
    }
}

impl InstanceLimitsSettings {
    pub fn validate(&self) -> Result<(), String> {
        for (index, rule) in self.rules.iter().enumerate() {
            rule.validate()?;
            for previous in &self.rules[..index] {
                if crate::control::instance_limits::paths_equal(
                    &rule.executable_path,
                    &previous.executable_path,
                )? {
                    return Err("Instance Limits permits only one rule per executable path.".into());
                }
            }
        }
        Ok(())
    }

    pub fn notifications_required(&self) -> bool {
        self.enabled && self.notify && self.rules.iter().any(|rule| rule.enabled && rule.notify)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_round_trip_and_reject_invalid_limits_paths_duplicates_and_self() {
        let rule = InstanceLimitsRule::new(r"C:\Apps\app.exe".into());
        let settings = InstanceLimitsSettings {
            enabled: true,
            rules: vec![rule.clone()],
            ..Default::default()
        };
        assert_eq!(
            toml::from_str::<InstanceLimitsSettings>(&toml::to_string(&settings).unwrap()).unwrap(),
            settings
        );
        assert!(settings.validate().is_ok());
        let mut invalid = rule.clone();
        invalid.max_instances = 0;
        assert!(invalid.validate().is_err());
        for path in [
            "relative.exe",
            r"C:\Apps\..\app.exe",
            r"C:\Apps\winderust.exe",
            r"C:\Windows\csrss.exe",
        ] {
            assert!(
                InstanceLimitsRule::new(path.into()).validate().is_err(),
                "{path}"
            );
        }
        let duplicate = InstanceLimitsRule::new("c:/apps/APP.exe".into());
        assert!(InstanceLimitsSettings {
            rules: vec![rule, duplicate],
            ..Default::default()
        }
        .validate()
        .is_err());
        let overflow =
            "enabled = true\nexecutable_path = 'C:\\Apps\\app.exe'\nmax_instances = 4294967296";
        assert!(toml::from_str::<InstanceLimitsRule>(overflow).is_err());
    }

    #[test]
    fn rule_notifications_default_on_and_require_an_enabled_opted_in_rule() {
        let rule = toml::from_str::<InstanceLimitsRule>(
            "enabled = true\nexecutable_path = 'C:\\Apps\\app.exe'\nmax_instances = 1",
        )
        .unwrap();
        assert!(rule.notify);
        let mut settings = InstanceLimitsSettings {
            enabled: true,
            rules: vec![rule],
            ..Default::default()
        };
        assert!(settings.notifications_required());
        settings.rules[0].notify = false;
        assert!(!settings.notifications_required());
        assert_eq!(
            toml::from_str::<InstanceLimitsSettings>(&toml::to_string(&settings).unwrap()).unwrap(),
            settings
        );
        settings.rules[0].notify = true;
        settings.rules[0].enabled = false;
        assert!(!settings.notifications_required());
        settings.rules[0].enabled = true;
        settings.notify = false;
        assert!(!settings.notifications_required());
    }

    #[test]
    fn settings_are_shared_across_power_sources_and_validate_the_shared_limit() {
        let mut settings = crate::config::Settings::default();
        settings
            .instance_limits
            .rules
            .push(InstanceLimitsRule::new(r"C:\Apps\app.exe".into()));
        settings.battery_profile_mut();
        settings.instance_limits.enabled = true;
        settings.sync_shared_settings_to_battery();
        assert_eq!(
            settings.instance_limits,
            settings.battery_profile().instance_limits
        );
        settings.battery_profile_mut().instance_limits.rules[0].max_instances = 0;
        assert!(settings.validate_instance_limits().is_ok());
        settings.instance_limits.rules[0].max_instances = 0;
        assert!(settings.validate_instance_limits().is_err());
    }
}
