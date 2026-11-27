use super::*;
use crate::config::{AdaptiveProcessorPowerPreset, Settings};

fn select_preset(editor: &mut Editor, settings: &mut SettingsEditor, message: Message) {
    editor.update(settings, message);
    editor.update(settings, Message::UsePreset);
}

#[test]
fn preview_cancel_preserves_the_editor_and_use_stages_only_the_selected_source() {
    let mut saved = Settings::default();
    saved.adaptive_engine.restraint_core_parking_min = 85;
    saved.adaptive_engine.restraint_core_parking_min_class1 = 80;
    let battery = &mut saved.battery_profile_mut().adaptive_engine;
    battery.restraint_core_parking_min = 65;
    battery.restraint_core_parking_min_class1 = 70;
    let mut settings = SettingsEditor::with_settings(saved);
    settings.select_power_source(PowerSourceProfile::OnBattery);
    let mut editor = Editor::default();
    let prior = settings.global().clone();
    editor.update(
        &mut settings,
        Message::Number(
            |settings, value| settings.base_processor_policy.performance_max = value as u32,
            String::new(),
            0,
            100,
            PROCESSOR_MAX_FIELD,
            "processor_power.processor_max",
        ),
    );
    let preview = Message::PreviewBuiltIn(BuiltInPreset::Balanced);
    editor.update(&mut settings, preview.clone());
    assert!(editor.preset_open());
    assert_eq!(settings.global(), &prior);
    editor.update(&mut settings, Message::CancelPreset);
    assert!(editor.validation_error().is_some());
    assert_eq!(settings.global(), &prior);

    select_preset(&mut editor, &mut settings, preview);
    assert!(!editor.preset_open());
    assert!(editor.validation_error().is_none());
    assert_eq!(
        settings
            .adaptive_engine
            .base_processor_policy
            .performance_max,
        95
    );
    assert_eq!(settings.adaptive_engine.restraint_core_parking_min, 0);
    assert_eq!(
        settings.adaptive_engine.restraint_core_parking_min_class1,
        0
    );
    assert_eq!(settings.global().adaptive_engine, prior.adaptive_engine);
    assert_eq!(settings.persisted(), &prior);
}

#[test]
fn restraint_parking_floors_can_be_edited_independently_on_battery() {
    let mut saved = Settings::default();
    saved.adaptive_engine.restraint_core_parking_min = 24;
    saved.adaptive_engine.restraint_core_parking_min_class1 = 36;
    let mut settings = SettingsEditor::with_settings(saved.clone());
    settings.select_power_source(PowerSourceProfile::OnBattery);
    let mut editor = Editor::default();

    let edit_class1 = |editor: &mut Editor, settings: &mut SettingsEditor, text: &str| {
        editor.update(
            settings,
            Message::Number(
                |settings, value| settings.restraint_core_parking_min_class1 = value as u8,
                text.into(),
                0,
                100,
                RESTRAINT_CORE_PARKING_MIN_CLASS1_FIELD,
                RESTRAINT_CORE_PARKING_MIN_CLASS1_LABEL,
            ),
        );
    };
    edit_class1(&mut editor, &mut settings, "101");
    assert!(editor.validation_error().is_some());
    assert!(!settings.has_unsaved_changes());

    edit_class1(&mut editor, &mut settings, "100");
    assert!(editor.validation_error().is_none());
    assert_eq!(settings.adaptive_engine.restraint_core_parking_min, 24);
    assert_eq!(
        settings.adaptive_engine.restraint_core_parking_min_class1,
        100
    );
    assert_eq!(settings.global().adaptive_engine, saved.adaptive_engine);
    assert_eq!(settings.persisted(), &saved);

    edit_class1(&mut editor, &mut settings, "0");
    assert_eq!(
        settings.adaptive_engine.restraint_core_parking_min_class1,
        0
    );
    settings.cancel();
    assert_eq!(settings.global(), &saved);
}

#[test]
fn equivalent_presets_keep_one_selected_row_per_source() {
    use BuiltInPreset::Balanced;
    use PowerSourceProfile::{OnBattery, PluggedIn};
    use PresetChoice::{BuiltIn, Custom};

    let mut saved = Settings::default();
    Balanced.apply(&mut saved.adaptive_engine, PluggedIn);
    Balanced.apply(&mut saved.battery_profile_mut().adaptive_engine, OnBattery);
    saved.adaptive_processor_power_presets = [
        ("AC Quiet", PluggedIn),
        ("AC Work", PluggedIn),
        ("Battery Quiet", OnBattery),
    ]
    .map(|(name, source)| {
        let settings = match source {
            PluggedIn => &saved,
            OnBattery => saved.battery_profile(),
        };
        AdaptiveProcessorPowerPreset::capture(name.into(), &settings.adaptive_engine, source)
    })
    .to_vec();
    saved.sync_shared_settings_to_battery();
    let mut settings = SettingsEditor::with_settings(saved);
    let mut editor = Editor::default();

    assert_eq!(editor.active_preset(&settings), Some(BuiltIn(Balanced)));
    for (message, active) in [
        (Message::PreviewCustom(0), Custom("AC Quiet")),
        (Message::PreviewCustom(1), Custom("AC Work")),
        (Message::PreviewBuiltIn(Balanced), BuiltIn(Balanced)),
        (Message::PreviewCustom(0), Custom("AC Quiet")),
    ] {
        select_preset(&mut editor, &mut settings, message);
        assert_eq!(editor.active_preset(&settings), Some(active));
        assert!(!settings.has_unsaved_changes());
    }

    settings.select_power_source(OnBattery);
    editor.reset_drafts();
    assert_eq!(editor.active_preset(&settings), Some(BuiltIn(Balanced)));
    select_preset(&mut editor, &mut settings, Message::PreviewCustom(2));
    assert_eq!(
        editor.active_preset(&settings),
        Some(Custom("Battery Quiet"))
    );
    assert!(!settings.has_unsaved_changes());

    settings.select_power_source(PluggedIn);
    editor.reset_drafts();
    assert_eq!(editor.active_preset(&settings), Some(Custom("AC Quiet")));
    editor.update(
        &mut settings,
        Message::BoostMode(
            |settings, mode| settings.base_processor_policy.boost_mode = mode,
            ProcessorBoostMode::Aggressive,
        ),
    );
    assert!(editor.active_preset(&settings).is_none());
    assert!(settings.has_unsaved_changes());
    select_preset(&mut editor, &mut settings, Message::PreviewCustom(0));
    assert!(!settings.has_unsaved_changes());

    editor.update(&mut settings, Message::EditPreset(0));
    editor.update(&mut settings, Message::PresetName("AC Renamed".into()));
    editor.update(&mut settings, Message::CancelPreset);
    assert_eq!(editor.active_preset(&settings), Some(Custom("AC Quiet")));
    assert!(!settings.has_unsaved_changes());
    editor.update(&mut settings, Message::EditPreset(0));
    editor.update(&mut settings, Message::PresetName(" AC RENAMED ".into()));
    editor.update(&mut settings, Message::SavePreset);
    assert_eq!(editor.active_preset(&settings), Some(Custom("AC RENAMED")));
    assert!(settings.has_unsaved_changes());

    editor.update(&mut settings, Message::DeletePreset(0));
    assert_eq!(editor.active_preset(&settings), Some(BuiltIn(Balanced)));
    settings.select_power_source(OnBattery);
    editor.reset_drafts();
    assert_eq!(
        editor.active_preset(&settings),
        Some(Custom("Battery Quiet"))
    );

    editor.update(&mut settings, Message::DeletePreset(1));
    assert_eq!(editor.active_preset(&settings), Some(BuiltIn(Balanced)));
    settings.cancel();
    editor.discard_preset_selection();
    editor.reset_drafts();
    assert!(!settings.has_unsaved_changes());
    assert_eq!(
        editor.active_preset(&settings),
        Some(Custom("Battery Quiet"))
    );
    settings.select_power_source(PluggedIn);
    editor.reset_drafts();
    assert_eq!(editor.active_preset(&settings), Some(Custom("AC Quiet")));

    for message in [
        Message::NewPreset,
        Message::PresetName("New AC choice".into()),
        Message::SavePreset,
    ] {
        editor.update(&mut settings, message);
    }
    select_preset(&mut editor, &mut settings, Message::PreviewCustom(3));
    assert_eq!(
        editor.active_preset(&settings),
        Some(Custom("New AC choice"))
    );
    settings.cancel();
    editor.discard_preset_selection();
    editor.reset_drafts();
    assert_eq!(editor.active_preset(&settings), Some(Custom("AC Quiet")));
}

#[test]
fn editing_a_preset_keeps_its_values_until_current_settings_are_requested() {
    let mut settings = SettingsEditor::with_settings(Settings::default());
    let original = settings.adaptive_engine.clone();
    let mut editor = Editor::default();
    let source = settings.selected_power_source();

    editor.update(&mut settings, Message::NewPreset);
    editor.update(&mut settings, Message::SavePreset);
    assert!(editor.preset_open());
    assert!(settings
        .global()
        .adaptive_processor_power_presets
        .is_empty());
    editor.update(&mut settings, Message::PresetName(" Quiet ".into()));
    editor.update(&mut settings, Message::SavePreset);
    assert_eq!(
        settings.global().adaptive_processor_power_presets[0].name,
        "Quiet"
    );

    select_preset(
        &mut editor,
        &mut settings,
        Message::PreviewBuiltIn(BuiltInPreset::Performance),
    );
    editor.update(
        &mut settings,
        Message::Number(
            |settings, value| settings.restraint_core_parking_min_class1 = value as u8,
            "50".into(),
            0,
            100,
            RESTRAINT_CORE_PARKING_MIN_CLASS1_FIELD,
            RESTRAINT_CORE_PARKING_MIN_CLASS1_LABEL,
        ),
    );
    editor.update(&mut settings, Message::NewPreset);
    editor.update(&mut settings, Message::PresetName("quiet".into()));
    editor.update(&mut settings, Message::SavePreset);
    assert!(editor.preset_open());
    assert_eq!(settings.global().adaptive_processor_power_presets.len(), 1);
    editor.update(&mut settings, Message::CancelPreset);

    editor.update(&mut settings, Message::EditPreset(0));
    editor.update(&mut settings, Message::PresetName("Fast".into()));
    editor.update(&mut settings, Message::SavePreset);
    let preset = &settings.global().adaptive_processor_power_presets[0];
    assert_eq!(preset.name, "Fast");
    assert!(preset.matches(&original, source));

    editor.update(&mut settings, Message::EditPreset(0));
    editor.update(&mut settings, Message::UseCurrentPresetValues(true));
    editor.update(&mut settings, Message::SavePreset);
    assert!(settings.global().adaptive_processor_power_presets[0]
        .matches(&settings.adaptive_engine, source));

    editor.update(&mut settings, Message::DeletePreset(0));
    assert!(settings
        .global()
        .adaptive_processor_power_presets
        .is_empty());
}

#[test]
fn processor_power_toggle_is_independent_and_profile_scoped() {
    let mut settings = SettingsEditor::with_settings(Settings::default());
    let mut editor = Editor::default();
    assert!(settings.global().on_battery.is_none());
    editor.update(&mut settings, Message::Enabled(false));
    assert!(!settings.has_unsaved_changes());
    assert!(settings.global().on_battery.is_none());
    editor.update(&mut settings, Message::Enabled(true));
    assert!(!settings.adaptive_engine.enabled);
    assert!(settings.adaptive_engine.adaptive_processor_power_enabled);
    assert!(settings.global().on_battery.is_some());
    settings.select_power_source(PowerSourceProfile::OnBattery);
    assert!(!settings.adaptive_engine.adaptive_processor_power_enabled);
    editor.update(&mut settings, Message::Enabled(true));
    assert!(!settings.adaptive_engine.enabled);
    settings.select_power_source(PowerSourceProfile::PluggedIn);
    assert!(settings.adaptive_engine.adaptive_processor_power_enabled);
    editor.update(&mut settings, Message::Enabled(false));
    settings.select_power_source(PowerSourceProfile::OnBattery);
    assert!(settings.adaptive_engine.adaptive_processor_power_enabled);
}

#[test]
fn processor_max_draft_is_rechecked_when_minimum_changes() {
    let mut settings = SettingsEditor::with_settings(Settings::default());
    settings
        .adaptive_engine
        .base_processor_policy
        .performance_min = 25;
    settings
        .adaptive_engine
        .base_processor_policy
        .performance_max = 100;
    let mut editor = Editor::default();
    let set_min = |text: &str| {
        Message::Number(
            |settings, value| settings.base_processor_policy.performance_min = value as u32,
            text.into(),
            0,
            100,
            PROCESSOR_MIN_FIELD,
            "processor_power.processor_min",
        )
    };
    let set_max = |text: &str| {
        Message::Number(
            |settings, value| settings.base_processor_policy.performance_max = value as u32,
            text.into(),
            0,
            100,
            PROCESSOR_MAX_FIELD,
            "processor_power.processor_max",
        )
    };
    let policy = |settings: &Settings| settings.adaptive_engine.base_processor_policy;

    editor.update(&mut settings, set_max("10"));
    assert_eq!(policy(&settings).performance_max, 100);
    assert_eq!(
        editor.numbers.drafts[PROCESSOR_MAX_FIELD].invalid,
        Some((25, 100, "processor_power.processor_max"))
    );
    editor.update(&mut settings, set_min("5"));
    assert_eq!(policy(&settings).performance_max, 10);
    assert_eq!(editor.numbers.drafts[PROCESSOR_MAX_FIELD].text, "10");
    assert!(editor.validation_error().is_none());

    editor.update(&mut settings, set_min("15"));
    assert_eq!(policy(&settings).performance_max, 15);
    assert!(!editor.numbers.drafts.contains_key(PROCESSOR_MAX_FIELD));
    editor.update(&mut settings, set_max(""));
    editor.update(&mut settings, set_min("20"));
    assert_eq!(policy(&settings).performance_max, 20);
    assert_eq!(editor.numbers.drafts[PROCESSOR_MAX_FIELD].text, "");
    assert_eq!(
        editor.numbers.drafts[PROCESSOR_MAX_FIELD].invalid,
        Some((20, 100, "processor_power.processor_max"))
    );
}
