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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShowWinderustShortcutKey {
    Character(u8),
    Function(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "KeyboardShortcut", into = "KeyboardShortcut")]
pub struct ShowWinderustShortcut {
    key: ShowWinderustShortcutKey,
    ctrl: bool,
    alt: bool,
    shift: bool,
}

impl ShowWinderustShortcut {
    pub(crate) fn key(self) -> ShowWinderustShortcutKey {
        self.key
    }

    pub(crate) fn modifiers(self) -> (bool, bool, bool) {
        (self.ctrl, self.alt, self.shift)
    }

    pub(crate) fn conflicts_with_search(self, search: &SearchHotkeys) -> bool {
        [&search.primary, &search.secondary]
            .into_iter()
            .any(|binding| Self::try_from(binding.clone()) == Ok(self))
    }
}

impl TryFrom<KeyboardShortcut> for ShowWinderustShortcut {
    type Error = &'static str;

    fn try_from(binding: KeyboardShortcut) -> Result<Self, Self::Error> {
        const INVALID: &str = "Show Winderust requires at least two of Ctrl, Alt, and Shift plus A–Z, a digit, or a function key other than F12.";
        let modifiers = [binding.ctrl, binding.alt, binding.shift];
        if !binding.enabled || modifiers.into_iter().filter(|pressed| *pressed).count() < 2 {
            return Err(INVALID);
        }
        let key = match binding.key.as_bytes() {
            [key] if key.is_ascii_alphanumeric() => {
                ShowWinderustShortcutKey::Character(key.to_ascii_uppercase())
            }
            _ => {
                let Some(function) = binding
                    .key
                    .strip_prefix(['f', 'F'])
                    .filter(|number| {
                        matches!(
                            number.as_bytes(),
                            [b'1'..=b'9'] | [b'1'..=b'2', b'0'..=b'9']
                        )
                    })
                    .and_then(|number| number.parse::<u8>().ok())
                    .filter(|&number| (1..=24).contains(&number) && number != 12)
                else {
                    return Err(INVALID);
                };
                ShowWinderustShortcutKey::Function(function)
            }
        };
        Ok(Self {
            key,
            ctrl: binding.ctrl,
            alt: binding.alt,
            shift: binding.shift,
        })
    }
}

impl From<ShowWinderustShortcut> for KeyboardShortcut {
    fn from(binding: ShowWinderustShortcut) -> Self {
        let key = match binding.key {
            ShowWinderustShortcutKey::Character(key) => {
                char::from(key.to_ascii_lowercase()).to_string()
            }
            ShowWinderustShortcutKey::Function(number) => format!("f{number}"),
        };
        Self {
            key,
            enabled: true,
            ctrl: binding.ctrl,
            alt: binding.alt,
            shift: binding.shift,
        }
    }
}

impl std::fmt::Display for ShowWinderustShortcut {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&KeyboardShortcut::from(*self), formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    fn shortcut(key: &str) -> KeyboardShortcut {
        KeyboardShortcut {
            key: key.into(),
            enabled: true,
            ctrl: true,
            alt: false,
            shift: true,
        }
    }

    #[test]
    fn show_winderust_accepts_only_supported_keys_with_two_modifiers() {
        for key in ["a", "7", "f1", "F11", "F13", "f24"] {
            assert!(ShowWinderustShortcut::try_from(shortcut(key)).is_ok());
        }
        for key in ["/", "é", "f0", "f01", "f+1", "F12", "f25"] {
            assert!(ShowWinderustShortcut::try_from(shortcut(key)).is_err());
        }
        let mut unsupported = shortcut("a");
        unsupported.shift = false;
        assert!(ShowWinderustShortcut::try_from(unsupported).is_err());
    }

    #[test]
    fn show_winderust_round_trips_and_rejects_an_active_search_conflict() {
        let mut settings = Settings::default();
        let previous = toml::to_string(&settings).unwrap();
        assert!(!previous.contains("show_winderust_shortcut"));
        assert_eq!(
            toml::from_str::<Settings>(&previous)
                .unwrap()
                .general
                .show_winderust_shortcut,
            None
        );

        let chord = ShowWinderustShortcut::try_from(shortcut("F9")).unwrap();
        settings.general.show_winderust_shortcut = Some(chord);
        settings.battery_profile_mut();
        let saved: Settings = toml::from_str(&toml::to_string(&settings).unwrap()).unwrap();
        assert_eq!(saved.general.show_winderust_shortcut, Some(chord));
        assert_eq!(
            saved.battery_profile().general.show_winderust_shortcut,
            Some(chord)
        );

        settings.general.search_hotkeys.primary = shortcut("f9");
        assert!(settings.validate().is_err());
        settings.general.search_hotkeys.primary.enabled = false;
        assert!(settings.validate().is_ok());
    }
}
