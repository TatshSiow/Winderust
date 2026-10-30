use crate::config::{KeyboardShortcut, SearchHotkeys};
use iced::{
    event::Status,
    keyboard::{key::Named, Key, Modifiers},
};

pub(super) fn capture(key: &Key, modifiers: Modifiers) -> Option<KeyboardShortcut> {
    if modifiers.logo() {
        return None;
    }
    let key = match key.as_ref() {
        Key::Character(value)
            if value.chars().count() == 1 && !value.chars().any(char::is_whitespace) =>
        {
            value.to_lowercase()
        }
        Key::Named(named)
            if matches!(
                named,
                Named::F1
                    | Named::F2
                    | Named::F3
                    | Named::F4
                    | Named::F5
                    | Named::F6
                    | Named::F7
                    | Named::F8
                    | Named::F9
                    | Named::F10
                    | Named::F11
                    | Named::F12
                    | Named::F13
                    | Named::F14
                    | Named::F15
                    | Named::F16
                    | Named::F17
                    | Named::F18
                    | Named::F19
                    | Named::F20
                    | Named::F21
                    | Named::F22
                    | Named::F23
                    | Named::F24
            ) =>
        {
            format!("{named:?}").to_lowercase()
        }
        _ => return None,
    };
    Some(KeyboardShortcut {
        key,
        enabled: true,
        ctrl: modifiers.control(),
        alt: modifiers.alt(),
        shift: modifiers.shift(),
    })
}

pub(super) fn opens_search(
    settings: &SearchHotkeys,
    key: &Key,
    modifiers: Modifiers,
    status: Status,
) -> bool {
    // Captured keystrokes belong to text fields, menus, or another active control.
    if status != Status::Ignored {
        return false;
    }
    let Some(shortcut) = capture(key, modifiers) else {
        return false;
    };
    settings.primary == shortcut || settings.secondary == shortcut
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcuts_match_exact_modifiers_and_respect_editing() {
        let defaults = SearchHotkeys::default();
        let key = Key::Character("k".into());
        assert!(opens_search(
            &defaults,
            &key,
            Modifiers::CTRL,
            Status::Ignored
        ));
        assert!(!opens_search(
            &defaults,
            &key,
            Modifiers::CTRL | Modifiers::SHIFT,
            Status::Ignored
        ));
        let slash = Key::Character("/".into());
        assert!(opens_search(
            &defaults,
            &slash,
            Modifiers::empty(),
            Status::Ignored
        ));
        assert!(!opens_search(
            &defaults,
            &slash,
            Modifiers::empty(),
            Status::Captured
        ));
        let mut custom = defaults.clone();
        custom.primary = capture(&Key::Named(Named::F2), Modifiers::ALT).unwrap();
        custom.secondary.enabled = false;
        assert!(opens_search(
            &custom,
            &Key::Named(Named::F2),
            Modifiers::ALT,
            Status::Ignored
        ));
        assert!(!opens_search(
            &custom,
            &key,
            Modifiers::CTRL,
            Status::Ignored
        ));
        assert!(capture(&Key::Named(Named::Escape), Modifiers::empty()).is_none());
        assert!(capture(&key, Modifiers::LOGO).is_none());
    }
    #[test]
    fn custom_shortcuts_and_disabled_slots_round_trip_with_settings() {
        let mut settings = crate::config::Settings::default();
        settings.general.search_hotkeys.primary = capture(
            &Key::Character("j".into()),
            Modifiers::CTRL | Modifiers::SHIFT,
        )
        .unwrap();
        settings.general.search_hotkeys.secondary.enabled = false;
        let encoded = toml::to_string(&settings).unwrap();
        let decoded: crate::config::Settings = toml::from_str(&encoded).unwrap();
        assert_eq!(
            decoded.general.search_hotkeys,
            settings.general.search_hotkeys
        );
        assert_eq!(
            decoded.general.search_hotkeys.primary.to_string(),
            "Ctrl+Shift+J"
        );
    }
}
