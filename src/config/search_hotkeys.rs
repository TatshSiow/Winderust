use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardShortcut {
    pub key: String,
    #[serde(default = "shortcut_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
}

impl std::fmt::Display for KeyboardShortcut {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (enabled, label) in [
            (self.ctrl, "Ctrl+"),
            (self.alt, "Alt+"),
            (self.shift, "Shift+"),
        ] {
            if enabled {
                formatter.write_str(label)?;
            }
        }
        formatter.write_str(&self.key.to_uppercase())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchHotkeys {
    pub primary: KeyboardShortcut,
    pub secondary: KeyboardShortcut,
}
impl Default for SearchHotkeys {
    fn default() -> Self {
        Self {
            primary: KeyboardShortcut {
                key: "k".into(),
                enabled: true,
                ctrl: true,
                alt: false,
                shift: false,
            },
            secondary: KeyboardShortcut {
                key: "/".into(),
                enabled: true,
                ctrl: false,
                alt: false,
                shift: false,
            },
        }
    }
}

fn shortcut_enabled() -> bool {
    true
}
