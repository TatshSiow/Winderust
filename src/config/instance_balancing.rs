use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceBalancingSettings {
    pub enabled: bool,
    pub rules: Vec<InstanceBalancingRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceBalancingRule {
    pub enabled: bool,
    pub executable_path: String,
}

impl InstanceBalancingSettings {
    pub fn validate(&self) -> Result<(), String> {
        use crate::control::instance_limits::{paths_equal, validate_path};

        for (index, rule) in self.rules.iter().enumerate() {
            validate_path(&rule.executable_path)
                .map_err(|reason| format!("Instance Balancing rule: {reason}"))?;
            for previous in &self.rules[..index] {
                if paths_equal(&rule.executable_path, &previous.executable_path)? {
                    return Err(
                        "Instance Balancing permits only one rule per executable path.".into(),
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_round_trip_and_reject_unsupported_paths_and_unknown_fields() {
        let settings = InstanceBalancingSettings {
            enabled: true,
            rules: vec![InstanceBalancingRule {
                enabled: true,
                executable_path: r"C:\Apps\app.exe".into(),
            }],
        };
        assert!(settings.validate().is_ok());
        assert_eq!(
            toml::from_str::<InstanceBalancingSettings>(&toml::to_string(&settings).unwrap())
                .unwrap(),
            settings
        );
        for path in [
            "",
            "relative.exe",
            r"C:app.exe",
            r"C:\Apps\..\app.exe",
            r"\\?\C:\Apps\app.exe",
            "C:\\Apps\\app.exe\0",
        ] {
            let mut invalid = settings.clone();
            invalid.rules[0].enabled = false;
            invalid.rules[0].executable_path = path.into();
            assert!(invalid.validate().is_err(), "{path:?}");
        }
        let raw = toml::to_string(&settings).unwrap();
        assert!(
            toml::from_str::<InstanceBalancingSettings>(&format!("extra = true\n{raw}")).is_err()
        );
        assert!(
            toml::from_str::<InstanceBalancingSettings>(&format!("{raw}\nextra = true")).is_err()
        );
    }

    #[test]
    fn equivalent_unicode_paths_conflict_even_when_a_rule_or_feature_is_disabled() {
        let mut settings = InstanceBalancingSettings {
            enabled: false,
            rules: vec![
                InstanceBalancingRule {
                    enabled: true,
                    executable_path: "C:/Apps/\u{00e9}\u{6d4b}\u{8bd5}.exe".into(),
                },
                InstanceBalancingRule {
                    enabled: false,
                    executable_path: "c:\\apps\\\u{00c9}\u{6d4b}\u{8bd5}.EXE".into(),
                },
            ],
        };
        assert!(settings.validate().is_err());
        settings.rules[1].executable_path = r"C:\Other\app.exe".into();
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn settings_default_off_and_share_rules_across_power_sources() {
        let mut settings = crate::config::Settings::default();
        assert!(!settings.instance_balancing.enabled);
        assert!(settings.instance_balancing.rules.is_empty());
        settings.battery_profile_mut();
        settings.instance_balancing = InstanceBalancingSettings {
            enabled: true,
            rules: vec![InstanceBalancingRule {
                enabled: true,
                executable_path: r"C:\Apps\app.exe".into(),
            }],
        };
        settings.sync_shared_settings_to_battery();
        assert_eq!(
            settings.instance_balancing,
            settings.battery_profile().instance_balancing
        );
        assert_eq!(
            toml::from_str::<crate::config::Settings>(&toml::to_string(&settings).unwrap())
                .unwrap(),
            settings
        );
        assert!(settings.validate().is_ok());
        settings.battery_profile_mut().instance_balancing.rules[0].executable_path =
            "relative.exe".into();
        assert!(settings.validate().is_err());
    }
}
