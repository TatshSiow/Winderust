use std::path::Path;

use serde::{Deserialize, Serialize};

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
