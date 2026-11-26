use std::{collections::BTreeSet, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeepRunningSettings {
    pub enabled: bool,
    pub rules: Vec<KeepRunningRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeepRunningRule {
    pub enabled: bool,
    #[serde(default)]
    pub notify: bool,
    pub executable_path: String,
    #[serde(default)]
    pub arguments: String,
}

impl KeepRunningRule {
    pub fn new(executable_path: String) -> Self {
        Self {
            enabled: true,
            notify: false,
            executable_path,
            arguments: String::new(),
        }
    }

    pub(crate) fn same_policy(&self, other: &Self) -> bool {
        self.enabled == other.enabled && self.same_launch_command(other)
    }

    pub(crate) fn same_launch_command(&self, other: &Self) -> bool {
        self.executable_path == other.executable_path && self.arguments == other.arguments
    }

    pub fn validate(&self) -> Result<(), String> {
        if !Path::new(&self.executable_path).is_absolute()
            || self.executable_path.contains(['\0', '"'])
            || self.executable_path.trim() != self.executable_path
            || self.arguments.contains('\0')
        {
            return Err("Keep Running requires an absolute executable path without quotes or NUL characters, and arguments without NUL characters.".into());
        }
        // Quoted executable, optional separating space, literal argument tail, and terminating NUL.
        let command_line_units = self.executable_path.encode_utf16().count()
            + 3
            + usize::from(!self.arguments.is_empty())
            + self.arguments.encode_utf16().count();
        if command_line_units > 32_767 {
            return Err("Keep Running command lines must fit within 32767 UTF-16 code units, including the terminating NUL.".into());
        }
        Ok(())
    }
}

impl KeepRunningSettings {
    pub(crate) fn notifications_required(&self) -> bool {
        self.enabled && self.rules.iter().any(|rule| rule.enabled && rule.notify)
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut paths = BTreeSet::new();
        for rule in &self.rules {
            rule.validate()?;
            let key = crate::foreground::executable_path_key(Path::new(&rule.executable_path))
                .to_ascii_lowercase();
            if !paths.insert(key) {
                return Err("Keep Running permits only one rule per executable path.".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_fields_round_trip_and_reject_invalid_paths_and_duplicate_rules() {
        let mut rule = KeepRunningRule::new(r"C:\Apps\app.exe".into());
        rule.arguments = r#"--document "C:\Documents\example.txt""#.into();
        assert!(rule.validate().is_ok());
        let settings = KeepRunningSettings {
            enabled: true,
            rules: vec![rule.clone()],
        };
        assert_eq!(
            toml::from_str::<KeepRunningSettings>(&toml::to_string(&settings).unwrap()).unwrap(),
            settings
        );
        for invalid in [
            "relative.exe",
            "C:\\Apps\\app.exe\0",
            "C:\\Apps\\app.exe\"",
            " C:\\Apps\\app.exe",
        ] {
            let mut invalid_rule = rule.clone();
            invalid_rule.executable_path = invalid.into();
            assert!(invalid_rule.validate().is_err());
        }
        let mut duplicate = rule.clone();
        duplicate.executable_path = "c:/apps/APP.exe".into();
        assert!(KeepRunningSettings {
            rules: vec![rule, duplicate],
            ..Default::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn command_line_limit_counts_utf16_and_the_quoted_prefix() {
        let mut rule = KeepRunningRule::new(r"C:\Apps\app.exe".into());
        let prefix = rule.executable_path.encode_utf16().count() + 4;
        rule.arguments = "x".repeat(32_767 - prefix);
        assert!(rule.validate().is_ok());
        rule.arguments.push('x');
        assert!(rule.validate().is_err());
        rule.arguments = "\u{1f600}".repeat((32_767 - prefix) / 2 + 1);
        assert!(rule.validate().is_err());
        rule.arguments = "--value\0hidden".into();
        assert!(rule.validate().is_err());
    }
}
