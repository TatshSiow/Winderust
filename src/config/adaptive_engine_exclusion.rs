use serde::{Deserialize, Serialize};

use crate::foreground::process_list::same_executable_name;

use super::{settings::default_true, ExecutableNamePattern, ExecutableSelector};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdaptiveEngineExclusionRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(flatten)]
    pub selector: AdaptiveEngineExclusionSelector,
}

impl AdaptiveEngineExclusionRule {
    pub(crate) fn exact_path(path: impl Into<String>) -> Self {
        Self {
            enabled: true,
            selector: AdaptiveEngineExclusionSelector::Exact(ExecutableSelector::ExactPath {
                executable_path: path.into(),
            }),
        }
    }

    pub(crate) fn exact_name(name: impl Into<String>) -> Self {
        Self {
            enabled: true,
            selector: AdaptiveEngineExclusionSelector::Exact(ExecutableSelector::ExactName {
                executable_name: name.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, try_from = "ExclusionSelectorFields")]
pub enum AdaptiveEngineExclusionSelector {
    Exact(ExecutableSelector),
    NamePattern {
        executable_name_pattern: ExecutableNamePattern,
    },
}

#[derive(Deserialize)]
struct ExclusionSelectorFields {
    executable_path: Option<String>,
    executable_name: Option<String>,
    executable_name_pattern: Option<ExecutableNamePattern>,
}

impl TryFrom<ExclusionSelectorFields> for AdaptiveEngineExclusionSelector {
    type Error = &'static str;

    fn try_from(fields: ExclusionSelectorFields) -> Result<Self, Self::Error> {
        match (
            fields.executable_path,
            fields.executable_name,
            fields.executable_name_pattern,
        ) {
            (Some(executable_path), None, None) => Ok(Self::Exact(ExecutableSelector::ExactPath {
                executable_path,
            })),
            (None, Some(executable_name), None) => Ok(Self::Exact(ExecutableSelector::ExactName {
                executable_name,
            })),
            (None, None, Some(executable_name_pattern)) => Ok(Self::NamePattern {
                executable_name_pattern,
            }),
            _ => Err("Specify exactly one executable path or executable name or name pattern."),
        }
    }
}

impl AdaptiveEngineExclusionSelector {
    pub(crate) fn exact(&self) -> Option<&ExecutableSelector> {
        match self {
            Self::Exact(selector) => Some(selector),
            Self::NamePattern { .. } => None,
        }
    }

    pub(crate) fn exact_path(&self) -> Option<&str> {
        self.exact().and_then(ExecutableSelector::exact_path)
    }

    pub(crate) fn matches_name(&self, name: &str) -> bool {
        match self {
            Self::Exact(ExecutableSelector::ExactPath { .. }) => false,
            Self::Exact(ExecutableSelector::ExactName { executable_name }) => {
                same_executable_name(executable_name, name)
            }
            Self::NamePattern {
                executable_name_pattern,
            } => executable_name_pattern.matches(name),
        }
    }
}
