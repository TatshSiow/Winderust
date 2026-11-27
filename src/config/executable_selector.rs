use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::foreground::process_list::same_executable_name;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, try_from = "ExecutableSelectorFields")]
pub enum ExecutableSelector {
    ExactPath { executable_path: String },
    ExactName { executable_name: String },
}

#[derive(Deserialize)]
struct ExecutableSelectorFields {
    executable_path: Option<String>,
    executable_name: Option<String>,
}

impl TryFrom<ExecutableSelectorFields> for ExecutableSelector {
    type Error = &'static str;

    fn try_from(fields: ExecutableSelectorFields) -> Result<Self, Self::Error> {
        match (fields.executable_path, fields.executable_name) {
            (Some(executable_path), None) => Ok(Self::ExactPath { executable_path }),
            (None, Some(executable_name)) => Ok(Self::ExactName { executable_name }),
            _ => Err("Specify exactly one executable path or executable name."),
        }
    }
}

impl ExecutableSelector {
    pub fn value(&self) -> &str {
        match self {
            Self::ExactPath { executable_path } => executable_path,
            Self::ExactName { executable_name } => executable_name,
        }
    }

    pub fn exact_path(&self) -> Option<&str> {
        match self {
            Self::ExactPath { executable_path } => Some(executable_path),
            Self::ExactName { .. } => None,
        }
    }
}

pub(crate) fn is_valid_executable_name(name: &str) -> bool {
    name.trim() == name
        && !name
            .chars()
            .any(|character| character.is_ascii_control() || r#"<>:"/\|?*"#.contains(character))
        && Path::new(name).file_name().is_some()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct ExecutableNamePattern(String);

impl TryFrom<String> for ExecutableNamePattern {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim() == value
            && value.contains(['*', '?'])
            && value.chars().take(256).count() <= 255
            && !value
                .chars()
                .any(|character| character.is_ascii_control() || r#"<>:"/\|"#.contains(character))
        {
            Ok(Self(value))
        } else {
            Err(rust_i18n::t!("common.executable_name_pattern_invalid").into_owned())
        }
    }
}

impl ExecutableNamePattern {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn matches(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        let mut pattern = self.as_str();
        let mut remaining = name;
        // Keep the last star so it can consume another character if the suffix fails.
        let mut retry = None;
        while let Some(&actual) = remaining.as_bytes().first() {
            match pattern.as_bytes().first().copied() {
                Some(b'*') => {
                    pattern = &pattern[1..];
                    retry = Some((pattern, remaining));
                    continue;
                }
                Some(expected) if expected.is_ascii() && actual.is_ascii() => {
                    if expected == b'?' || expected.eq_ignore_ascii_case(&actual) {
                        pattern = &pattern[1..];
                        remaining = &remaining[1..];
                        continue;
                    }
                }
                Some(_) => {
                    let (Some(expected), Some(actual)) =
                        (pattern.chars().next(), remaining.chars().next())
                    else {
                        return false;
                    };
                    let expected_len = expected.len_utf8();
                    let actual_len = actual.len_utf8();
                    if expected == '?'
                        || same_executable_name(&pattern[..expected_len], &remaining[..actual_len])
                    {
                        pattern = &pattern[expected_len..];
                        remaining = &remaining[actual_len..];
                        continue;
                    }
                }
                None => {}
            }
            let Some((suffix, rest)) = retry else {
                return false;
            };
            let Some(next) = rest.chars().next() else {
                return false;
            };
            remaining = &rest[next.len_utf8()..];
            pattern = suffix;
            retry = Some((suffix, remaining));
        }
        pattern.bytes().all(|byte| byte == b'*')
    }
}

#[cfg(test)]
mod tests {
    use super::ExecutableNamePattern;

    #[test]
    fn executable_name_patterns_match_the_entire_name_and_retry_stars() {
        let pattern = ExecutableNamePattern::try_from("\u{00c9}diteur*-?.exe".to_owned()).unwrap();
        for name in ["\u{00e9}diteur-\u{1f9ea}.EXE", "\u{00e9}diteur-a-b.EXE"] {
            assert!(pattern.matches(name), "{name}");
        }
        for name in [
            "\u{00e9}diteur-.exe",
            "\u{00e9}diteur-xy.exe",
            "\u{00e9}diteur-a.exe.old",
            "my-\u{00e9}diteur-a.exe",
        ] {
            assert!(!pattern.matches(name), "{name}");
        }
    }

    #[test]
    fn executable_name_patterns_match_ascii_and_unicode_in_the_same_rule() {
        let pattern = ExecutableNamePattern::try_from("worker-**?b.exe".to_owned()).unwrap();
        for name in [
            "WORKER-Ab.EXE",
            "worker-\u{1f9ea}b.exe",
            "worker-\u{1f9ea}xb.exe",
        ] {
            assert!(pattern.matches(name), "{name}");
        }
        for name in ["", "worker-b.exe", "worker-\u{1f9ea}xb.exe.old"] {
            assert!(!pattern.matches(name), "{name}");
        }
    }
}
