use super::*;
use std::{path::PathBuf, sync::Mutex};

#[derive(Clone)]
struct FakeStorage {
    load: Settings,
    import: Result<Settings, String>,
    save_result: Result<(), String>,
    export_result: Result<(), String>,
    saved: Arc<Mutex<Vec<Settings>>>,
    exported: Arc<Mutex<Vec<(PathBuf, Settings)>>>,
}

impl FakeStorage {
    fn new(
        load: Settings,
        import: Result<Settings, String>,
        save_result: Result<(), String>,
        export_result: Result<(), String>,
    ) -> Self {
        Self {
            load,
            import,
            save_result,
            export_result,
            saved: Arc::new(Mutex::new(Vec::new())),
            exported: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn saved_payloads(&self) -> Vec<Settings> {
        self.saved.lock().expect("saved lock").clone()
    }

    fn exported_payloads(&self) -> Vec<(PathBuf, Settings)> {
        self.exported.lock().expect("export lock").clone()
    }
}

impl SettingsStorage for FakeStorage {
    fn load(&self) -> Result<Settings, String> {
        Ok(self.load.clone())
    }

    fn save(&self, settings: &Settings) -> Result<(), String> {
        let mut saved = self.saved.lock().expect("saved lock");
        saved.push(settings.clone());
        self.save_result.clone()
    }

    fn import(&self, _path: &Path) -> Result<Settings, String> {
        self.import.clone()
    }

    fn export(&self, path: &Path, settings: &Settings) -> Result<(), String> {
        let mut exported = self.exported.lock().expect("export lock");
        exported.push((path.to_path_buf(), settings.clone()));
        self.export_result.clone()
    }
}

#[derive(Clone)]
struct FakeStartupRegistration {
    result: Result<(), StartupRegistrationError>,
    applied: Arc<Mutex<Vec<bool>>>,
}

impl FakeStartupRegistration {
    fn new(result: Result<(), StartupRegistrationError>) -> Self {
        Self {
            result,
            applied: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn applied_values(&self) -> Vec<bool> {
        self.applied.lock().expect("startup lock").clone()
    }
}

impl StartupRegistration for FakeStartupRegistration {
    fn set_enabled(&self, enabled: bool) -> Result<(), StartupRegistrationError> {
        self.applied.lock().expect("startup lock").push(enabled);
        self.result.clone()
    }
}

#[test]
fn watchdog_is_global_when_editing_or_toggling_from_battery() {
    let mut settings = Settings::default();
    settings.battery_profile_mut();
    let storage = FakeStorage::new(settings, Ok(Settings::default()), Ok(()), Ok(()));
    let (mut editor, _, _) = editor_with_fixtures(storage, FakeStartupRegistration::new(Ok(())));
    editor.select_power_source(PowerSourceProfile::OnBattery);
    editor.edit_global(|settings| {
        let mut rule = crate::config::WatchdogRule::new(r"C:\Apps\test.exe".into());
        rule.memory.enabled = true;
        settings.watchdog.rules.push(rule);
    });
    assert_eq!(editor.global().watchdog, editor.watchdog);
    assert!(editor.global().watchdog.rules[0].memory.enabled);
    editor.save().unwrap();
    editor.set_watchdog_enabled(true).unwrap();
    let runtime = editor.runtime_settings_snapshot();
    assert!(runtime.value.watchdog.enabled);
    assert_eq!(
        runtime.value.watchdog,
        runtime.value.battery_profile().watchdog
    );
    editor.select_power_source(PowerSourceProfile::PluggedIn);
    assert_eq!(editor.global().watchdog, editor.watchdog);
    editor.set_watchdog_enabled(false).unwrap();
    assert!(
        !editor
            .runtime_settings_snapshot()
            .value
            .battery_profile()
            .watchdog
            .enabled
    );
}

fn load_ok_with_fixtures() -> (SettingsCoordinator, SettingsDraft, FakeStorage) {
    let storage = FakeStorage::new(Settings::default(), Ok(Settings::default()), Ok(()), Ok(()));
    let storage_copy = storage.clone();
    let (coordinator, draft) = SettingsCoordinator::load_from(Box::new(storage));
    (coordinator, draft, storage_copy)
}

fn editor_with_fixtures(
    storage: FakeStorage,
    startup: FakeStartupRegistration,
) -> (SettingsEditor, FakeStorage, FakeStartupRegistration) {
    let storage_probe = storage.clone();
    let startup_probe = startup.clone();
    let (coordinator, draft) = SettingsCoordinator::load_from(Box::new(storage));
    (
        SettingsEditor::from_parts(coordinator, draft, Box::new(startup)),
        storage_probe,
        startup_probe,
    )
}

struct FileStorage {
    path: PathBuf,
    read_error: bool,
    write_error: Arc<std::sync::atomic::AtomicBool>,
}

impl SettingsStorage for FileStorage {
    fn load(&self) -> Result<Settings, String> {
        if self.read_error {
            Err("Access denied".into())
        } else {
            config::storage::load_from_path(&self.path)
        }
    }
    fn save(&self, settings: &Settings) -> Result<(), String> {
        if self.write_error.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Access denied".into());
        }
        config::storage::export_toml_to(&self.path, settings)
    }
    fn import(&self, path: &Path) -> Result<Settings, String> {
        config::storage::import_toml_from(path)
    }
    fn export(&self, path: &Path, settings: &Settings) -> Result<(), String> {
        config::storage::export_toml_to(path, settings)
    }
}

#[test]
fn failed_load_preserves_original_until_explicit_replacement_succeeds() {
    use std::sync::atomic::{AtomicBool, Ordering};
    for read_error in [false, true] {
        for import in [false, true] {
            let path = std::env::temp_dir().join(format!(
                "winderust-recovery-{}-{read_error}-{import}.toml",
                std::process::id()
            ));
            let import_path = path.with_extension("import.toml");
            let original = b"# Preserve this repairable configuration\ninvalid = [";
            std::fs::write(&path, original).unwrap();
            let write_error = Arc::new(AtomicBool::new(false));
            let startup = FakeStartupRegistration::new(Ok(()));
            let startup_probe = startup.clone();
            let (mut editor, _) = SettingsEditor::load_from(
                Box::new(FileStorage {
                    path: path.clone(),
                    read_error,
                    write_error: write_error.clone(),
                }),
                Box::new(startup),
            );
            assert!(editor.load_error().is_some());
            assert!(startup_probe.applied_values().is_empty());
            assert_eq!(
                editor.set_master_enabled(!editor.global().general.enabled),
                Err(SettingsCoordinatorError::RecoveryRequired)
            );
            assert_eq!(
                editor.set_feature_enabled(
                    PowerSourceProfile::PluggedIn,
                    |s| &mut s.cpu_limiter.enabled,
                    true
                ),
                Err(SettingsCoordinatorError::RecoveryRequired)
            );
            let mut exclusions = AutoExclusionPatch {
                base_revision: editor.draft.base_revision,
                ..Default::default()
            };
            exclusions.cpu_limiter.push(r"C:\Apps\test.exe".into());
            assert_eq!(
                editor.apply_auto_exclusion_patch(&exclusions),
                Err(SettingsCoordinatorError::RecoveryRequired)
            );
            editor.cancel();
            assert!(editor.load_error().is_some());
            assert_eq!(std::fs::read(&path).unwrap(), original);

            editor.edit_global(|settings| settings.general.check_interval_ms = 1234);
            editor.export_toml_to(&import_path).unwrap();
            write_error.store(true, Ordering::Relaxed);
            assert!(if import {
                editor.import_toml_from(&import_path)
            } else {
                editor.save()
            }
            .is_err());
            assert!(editor.load_error().is_some());
            assert!(startup_probe.applied_values().is_empty());
            assert_eq!(std::fs::read(&path).unwrap(), original);
            write_error.store(false, Ordering::Relaxed);
            if import {
                editor.import_toml_from(&import_path).unwrap();
            } else {
                editor.save().unwrap();
            }
            assert!(editor.load_error().is_none());
            assert_eq!(
                config::storage::load_from_path(&path)
                    .unwrap()
                    .general
                    .check_interval_ms,
                1234
            );
            let enabled = !editor.global().general.enabled;
            assert!(editor.set_master_enabled(enabled).unwrap());
            assert_eq!(
                config::storage::load_from_path(&path)
                    .unwrap()
                    .general
                    .enabled,
                enabled
            );
            std::fs::remove_file(path).unwrap();
            std::fs::remove_file(import_path).unwrap();
        }
    }
}

#[test]
fn missing_settings_allow_normal_automatic_persistence() {
    let path =
        std::env::temp_dir().join(format!("winderust-first-load-{}.toml", std::process::id()));
    assert!(!path.exists());
    let (mut editor, _) = SettingsEditor::load_from(
        Box::new(FileStorage {
            path: path.clone(),
            read_error: false,
            write_error: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }),
        Box::new(FakeStartupRegistration::new(Ok(()))),
    );
    assert!(editor.load_error().is_none());
    let enabled = !editor.global().general.enabled;
    assert!(editor.set_master_enabled(enabled).unwrap());
    assert_eq!(
        config::storage::load_from_path(&path)
            .unwrap()
            .general
            .enabled,
        enabled
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn global_edits_save_from_battery_without_overwriting_feature_policies() {
    let mut initial = Settings::default();
    initial.cpu_limiter.enabled = false;
    initial.battery_profile_mut().cpu_limiter.enabled = true;
    let storage = FakeStorage::new(initial.clone(), Ok(initial), Ok(()), Ok(()));
    let (mut editor, storage, _) =
        editor_with_fixtures(storage, FakeStartupRegistration::new(Ok(())));
    editor.select_power_source(PowerSourceProfile::OnBattery);
    editor.edit_global(|settings| {
        settings.general.enabled = false;
        settings.advanced.action_log_mode = config::ActionLogMode::Off;
    });
    editor.save().unwrap();
    let saved = &storage.saved_payloads()[0];
    assert!(!saved.general.enabled);
    assert!(!saved.battery_profile().general.enabled);
    assert_eq!(saved.advanced, saved.battery_profile().advanced);
    assert!(!saved.cpu_limiter.enabled);
    assert!(saved.battery_profile().cpu_limiter.enabled);
    assert_eq!(
        editor.runtime_settings_snapshot().value.general.enabled,
        saved.general.enabled
    );
    assert!(!editor.has_unsaved_changes());
}

#[test]
fn tray_feature_switch_only_saves_the_selected_profile_and_field() {
    for profile in [PowerSourceProfile::PluggedIn, PowerSourceProfile::OnBattery] {
        for fail in [false, true] {
            let mut initial = Settings::default();
            initial.cpu_limiter.enabled = false;
            let storage = FakeStorage::new(
                initial.clone(),
                Ok(initial),
                if fail {
                    Err("write failed".into())
                } else {
                    Ok(())
                },
                Ok(()),
            );
            let (mut editor, storage, _) =
                editor_with_fixtures(storage, FakeStartupRegistration::new(Ok(())));
            editor.general.check_interval_ms = 1_337;
            let original = editor.draft.clone();
            assert_eq!(
                editor
                    .set_feature_enabled(profile, |s| &mut s.cpu_limiter.enabled, true)
                    .is_err(),
                fail
            );
            if fail {
                assert_eq!(editor.draft, original);
                assert!(!editor.persisted().cpu_limiter.enabled);
                assert!(editor.persisted().on_battery.is_none());
            } else {
                let saved = &storage.saved_payloads()[0];
                assert_eq!(
                    saved.cpu_limiter.enabled,
                    profile == PowerSourceProfile::PluggedIn
                );
                assert_eq!(
                    saved.battery_profile().cpu_limiter.enabled,
                    profile == PowerSourceProfile::OnBattery
                );
                assert_eq!(
                    saved.general.check_interval_ms,
                    Settings::default().general.check_interval_ms
                );
                assert_eq!(editor.draft.value.general.check_interval_ms, 1_337);
            }
        }
    }
}

#[test]
fn tray_master_switch_preserves_drafts_and_rolls_back_on_save_failure() {
    for fail in [false, true] {
        let mut initial = Settings::default();
        initial.general.enabled = true;
        initial.battery_profile_mut();
        let storage = FakeStorage::new(
            initial.clone(),
            Ok(initial),
            if fail {
                Err("write failed".into())
            } else {
                Ok(())
            },
            Ok(()),
        );
        let (mut editor, storage, _) =
            editor_with_fixtures(storage, FakeStartupRegistration::new(Ok(())));
        editor.general.check_interval_ms = 1_337;
        editor.select_power_source(PowerSourceProfile::OnBattery);
        let original = editor.draft.clone();
        let result = editor.set_master_enabled(false);
        assert_eq!(result.is_err(), fail);
        assert_eq!(editor.persisted().general.enabled, fail);
        assert_eq!(editor.general.enabled, fail);
        assert_eq!(editor.draft.value.general.check_interval_ms, 1_337);
        if fail {
            assert_eq!(editor.draft, original);
        } else {
            assert!(!editor.runtime_settings_snapshot().value.general.enabled);
            assert_eq!(
                storage.saved_payloads()[0].general.check_interval_ms,
                Settings::default().general.check_interval_ms
            );
            assert!(
                !storage.saved_payloads()[0]
                    .battery_profile()
                    .general
                    .enabled
            );
        }
    }
}

#[test]
fn stale_patch_revision_is_rejected_and_keeps_draft() {
    let (mut coordinator, mut draft, _storage) = load_ok_with_fixtures();
    draft.general.check_interval_ms = 1_337;
    let stale = coordinator.persisted_revision().next();
    let patch = AutoExclusionPatch {
        base_revision: stale,
        ..Default::default()
    };
    let result = coordinator.apply_auto_exclusion_patch(&mut draft, &patch);
    assert!(matches!(
        result,
        Err(SettingsCoordinatorError::StalePatch { .. })
    ));
    assert_eq!(draft.general.check_interval_ms, 1_337);
}

#[test]
fn stale_draft_save_is_rejected_and_keeps_the_edited_value() {
    let (mut coordinator, mut draft, _storage) = load_ok_with_fixtures();
    draft.general.check_interval_ms = 1_337;
    draft.base_revision = draft.base_revision.next();

    assert!(matches!(
        coordinator.save(&mut draft),
        Err(SettingsCoordinatorError::StaleDraft { .. })
    ));
    assert_eq!(draft.general.check_interval_ms, 1_337);
}

#[test]
fn reversed_cpu_load_ranges_do_not_reach_storage_or_runtime() {
    for battery in [false, true] {
        let (mut coordinator, mut draft, storage) = load_ok_with_fixtures();
        let persisted = coordinator.persisted().clone();
        let revision = coordinator.persisted_revision();
        let runtime = coordinator.runtime_settings_snapshot(&draft);
        let profile = if battery {
            draft.battery_profile_mut()
        } else {
            &mut draft.value
        };
        let rule = &mut profile.by_cpu_load.rules[0];
        rule.comparison = config::CpuUsageComparison::Between;
        rule.threshold_percent = config::CpuLoadPercent::new(80).unwrap();
        rule.upper_threshold_percent = Some(config::CpuLoadPercent::new(20).unwrap());
        let edited = draft.clone();

        assert!(matches!(
            coordinator.save(&mut draft),
            Err(SettingsCoordinatorError::Save(_))
        ));
        assert!(storage.saved_payloads().is_empty());
        assert_eq!(coordinator.persisted(), &persisted);
        assert_eq!(coordinator.persisted_revision(), revision);
        assert_eq!(draft, edited);
        let after = coordinator.runtime_settings_snapshot(&draft);
        assert_eq!(after.runtime_revision, runtime.runtime_revision);
        assert!(Arc::ptr_eq(&after.value, &runtime.value));
    }
}

#[test]
fn patch_merge_keeps_unrelated_changes() {
    let (mut coordinator, mut draft, storage) = load_ok_with_fixtures();
    draft.general.check_interval_ms = 1_337;
    let patch = AutoExclusionPatch {
        base_revision: draft.base_revision,
        process_priority: vec!["C:\\Apps\\worker.exe".to_owned()],
        ..AutoExclusionPatch::default()
    };

    assert!(coordinator
        .apply_auto_exclusion_patch(&mut draft, &patch)
        .expect("apply"));
    assert_eq!(draft.general.check_interval_ms, 1_337);
    let expected_path = executable_path_key(Path::new(r"C:\Apps\worker.exe"));
    assert!(draft
        .process_priority
        .exclusions
        .iter()
        .any(|rule| rule.executable_path == expected_path));
    let saved = storage.saved_payloads();
    assert_eq!(saved.len(), 1);
    assert_eq!(
        saved[0].general.check_interval_ms,
        Settings::default().general.check_interval_ms
    );
    assert!(saved[0]
        .process_priority
        .exclusions
        .iter()
        .any(|rule| rule.executable_path == expected_path));
    assert_eq!(draft.base_revision, coordinator.persisted_revision());
}

#[test]
fn failed_runtime_patch_does_not_mutate_persisted_settings_or_draft() {
    let storage = FakeStorage::new(
        Settings::default(),
        Ok(Settings::default()),
        Err("permission denied".to_owned()),
        Ok(()),
    );
    let (mut coordinator, mut draft) = SettingsCoordinator::load_from(Box::new(storage));
    draft.general.check_interval_ms = 1_337;
    let original_draft = draft.clone();
    let patch = AutoExclusionPatch {
        base_revision: draft.base_revision,
        process_priority: vec!["C:\\Apps\\worker.exe".to_owned()],
        ..AutoExclusionPatch::default()
    };

    assert!(matches!(
        coordinator.apply_auto_exclusion_patch(&mut draft, &patch),
        Err(SettingsCoordinatorError::Save(_))
    ));
    assert_eq!(draft, original_draft);
    assert!(coordinator
        .persisted()
        .process_priority
        .exclusions
        .is_empty());
}

#[test]
fn auto_exclusion_patch_matches_paths_case_insensitively_and_ignores_relative_paths() {
    let (mut coordinator, mut draft, storage) = load_ok_with_fixtures();
    draft
        .process_priority
        .exclusions
        .push(process_exclusion_rule(r"C:\Apps\Worker.exe"));
    draft.process_priority.exclusions[0].enabled = false;
    draft.battery_profile_mut().process_priority.exclusions[0].enabled = false;
    coordinator.save(&mut draft).expect("save fixture rule");
    let patch = AutoExclusionPatch {
        base_revision: draft.base_revision,
        process_priority: vec![r"c:\apps\worker.exe".to_owned(), "worker.exe".to_owned()],
        ..AutoExclusionPatch::default()
    };

    assert!(coordinator
        .apply_auto_exclusion_patch(&mut draft, &patch)
        .expect("apply patch"));
    assert_eq!(draft.process_priority.exclusions.len(), 1);
    assert!(draft.process_priority.exclusions[0].enabled);
    assert!(draft.battery_profile().process_priority.exclusions[0].enabled);
    assert_eq!(storage.saved_payloads().len(), 2);
}

#[test]
fn runtime_settings_projection_keeps_live_general_and_advanced_only() {
    let mut current = Settings::default();
    let mut persisted = Settings::default();
    persisted.general.enabled = true;
    current.process_priority.enabled = true;
    persisted.process_priority.enabled = false;
    persisted
        .cpu_allocation_presets
        .push(crate::config::CpuAllocationPreset {
            name: "Gaming".to_owned(),
            core_mask: 0b11,
        });
    let runtime = runtime_settings_for(&current, &persisted);
    assert!(runtime.general.enabled);
    assert_eq!(
        runtime.process_priority.enabled,
        persisted.process_priority.enabled
    );
    assert_eq!(runtime.advanced, current.advanced);
    assert!(runtime.cpu_allocation_presets.is_empty());
}

#[test]
fn preset_edits_survive_profile_switching_and_save() {
    for profile in [PowerSourceProfile::PluggedIn, PowerSourceProfile::OnBattery] {
        let mut initial = Settings::default();
        initial.battery_profile_mut();
        let storage = FakeStorage::new(initial.clone(), Ok(initial), Ok(()), Ok(()));
        let (mut editor, _, _) =
            editor_with_fixtures(storage, FakeStartupRegistration::new(Ok(())));
        let runtime_before = editor.runtime_settings_snapshot();
        editor.select_power_source(profile);
        editor.edit_with_presets(|settings| {
            settings
                .adaptive_engine_presets
                .push(crate::config::AdaptiveEnginePreset {
                    name: "Saved".into(),
                    processor_power_policy_enabled: true,
                    base_processor_policy: settings.adaptive_engine.base_processor_policy,
                    background_pressure_profile: settings
                        .adaptive_engine
                        .background_pressure_profile,
                    focus_and_launch_profile: settings.adaptive_engine.focus_and_launch_profile,
                    adaptive_engine_process: settings.adaptive_engine_process.clone(),
                });
            settings
                .cpu_allocation_presets
                .push(crate::config::CpuAllocationPreset {
                    name: "Saved".into(),
                    core_mask: 3,
                });
            settings.advanced_power_plan_tuning_presets.push(
                crate::config::AdvancedPowerPlanTuningPreset {
                    name: "Saved".into(),
                    values: settings.adaptive_engine.base_processor_policy.into(),
                },
            );
        });
        editor.save().expect("save presets");
        let runtime_saved = editor.runtime_settings_snapshot();
        assert_eq!(
            runtime_saved.runtime_revision,
            runtime_before.runtime_revision
        );
        assert_ne!(
            runtime_saved.persisted_revision,
            runtime_before.persisted_revision
        );
        assert!(Arc::ptr_eq(&runtime_saved.value, &runtime_before.value));
        for runtime_profile in [
            runtime_saved.value.as_ref(),
            runtime_saved.value.battery_profile(),
        ] {
            assert!(runtime_profile.adaptive_engine_presets.is_empty());
            assert!(runtime_profile.cpu_allocation_presets.is_empty());
            assert!(runtime_profile
                .advanced_power_plan_tuning_presets
                .is_empty());
        }
        for selected in [PowerSourceProfile::PluggedIn, PowerSourceProfile::OnBattery] {
            editor.select_power_source(selected);
            assert_eq!(editor.adaptive_engine_presets[0].name, "Saved");
            assert_eq!(editor.cpu_allocation_presets[0].core_mask, 3);
            assert_eq!(editor.advanced_power_plan_tuning_presets[0].name, "Saved");
        }
        editor.select_power_source(profile);
        editor.edit_with_presets(|settings| {
            settings.adaptive_engine_presets[0].name = "Edited".into();
            settings.cpu_allocation_presets.clear();
            settings.advanced_power_plan_tuning_presets.clear();
        });
        editor.save().expect("save edits");
        assert_eq!(editor.persisted().adaptive_engine_presets[0].name, "Edited");
        assert!(editor.persisted().cpu_allocation_presets.is_empty());
        assert!(editor
            .persisted()
            .advanced_power_plan_tuning_presets
            .is_empty());
        assert_eq!(
            editor.persisted().adaptive_engine_presets,
            editor.persisted().battery_profile().adaptive_engine_presets
        );
    }
}

#[test]
fn settings_editor_keeps_power_source_profiles_independent() {
    let mut editor = SettingsEditor::with_settings(Settings::default());
    editor.process_priority.enabled = false;

    editor.select_power_source(PowerSourceProfile::OnBattery);
    editor.process_priority.enabled = true;
    assert!(editor.process_priority.enabled);

    editor.select_power_source(PowerSourceProfile::PluggedIn);
    assert!(!editor.process_priority.enabled);
    assert!(
        editor
            .draft
            .value
            .battery_profile()
            .process_priority
            .enabled
    );
}

#[test]
fn runtime_snapshot_updates_with_full_equality_for_priority_sections() {
    let (mut coordinator, mut draft, _storage) = load_ok_with_fixtures();

    let s1 = coordinator.runtime_settings_snapshot(&draft);
    draft.process_priority.foreground_priority = crate::config::ProcessPrioritySetting::AboveNormal;
    let unsaved = coordinator.runtime_settings_snapshot(&draft);
    assert_eq!(s1.runtime_revision, unsaved.runtime_revision);
    assert!(Arc::ptr_eq(&s1.value, &unsaved.value));
    coordinator.save(&mut draft).expect("save process priority");
    let s2 = coordinator.runtime_settings_snapshot(&draft);
    draft.thread_priority.foreground_priority =
        crate::config::ProcessThreadPrioritySetting::Highest;
    coordinator.save(&mut draft).expect("save thread priority");
    let s3 = coordinator.runtime_settings_snapshot(&draft);
    draft.dynamic_priority_boost.foreground_boost =
        crate::config::ProcessDynamicPriorityBoostSetting::Enabled;
    coordinator
        .save(&mut draft)
        .expect("save Dynamic Priority Boost");
    let s4 = coordinator.runtime_settings_snapshot(&draft);

    assert_ne!(s1.runtime_revision, s2.runtime_revision);
    assert_ne!(s2.runtime_revision, s3.runtime_revision);
    assert_ne!(s3.runtime_revision, s4.runtime_revision);
}

#[test]
fn saving_an_already_live_general_edit_only_advances_persisted_revision() {
    let (mut coordinator, mut draft, _storage) = load_ok_with_fixtures();
    draft.general.check_interval_ms = 1_337;
    let live = coordinator.runtime_settings_snapshot(&draft);

    coordinator.save(&mut draft).expect("save general settings");
    let saved = coordinator.runtime_settings_snapshot(&draft);

    assert_eq!(saved.runtime_revision, live.runtime_revision);
    assert_ne!(saved.persisted_revision, live.persisted_revision);
    assert_eq!(saved.persisted_revision, coordinator.persisted_revision());
}

#[test]
fn unchanged_draft_reuses_the_runtime_settings_allocation() {
    let (mut coordinator, draft, _storage) = load_ok_with_fixtures();
    let first = coordinator.runtime_settings_snapshot(&draft);
    let second = coordinator.runtime_settings_snapshot(&draft);

    assert_eq!(first.runtime_revision, second.runtime_revision);
    assert!(Arc::ptr_eq(&first.value, &second.value));
}

#[test]
fn save_failure_reports_typed_error_and_keeps_base_revision() {
    let storage = FakeStorage::new(
        Settings::default(),
        Ok(Settings::default()),
        Err("permission denied".to_owned()),
        Ok(()),
    );
    let (mut coordinator, mut draft) = SettingsCoordinator::load_from(Box::new(storage.clone()));

    let err = coordinator.save(&mut draft).unwrap_err();
    assert!(matches!(err, SettingsCoordinatorError::Save(_)));
    assert_eq!(draft.base_revision, SettingsRevision::initial().next());
    assert_eq!(storage.saved_payloads().len(), 1);
}

#[test]
fn cancel_discards_unsaved_edits() {
    let (mut coordinator, mut draft, _storage) = load_ok_with_fixtures();
    draft.general.check_interval_ms = 2_500;
    coordinator.cancel(&mut draft);
    assert_eq!(draft.base_revision, coordinator.persisted_revision());
    assert_eq!(
        draft.general.check_interval_ms,
        coordinator.persisted().general.check_interval_ms
    );
}

#[test]
fn import_writes_imported_settings_to_storage_and_applies_runtime_revision() {
    let imported = Settings {
        general: {
            let mut general = Settings::default().general;
            general.startup_with_windows = true;
            general
        },
        ..Settings::default()
    };
    let storage = FakeStorage::new(Settings::default(), Ok(imported.clone()), Ok(()), Ok(()));
    let (mut coordinator, mut draft, storage_probe) = {
        let storage_copy = storage.clone();
        let (coordinator, draft) = SettingsCoordinator::load_from(Box::new(storage_copy.clone()));
        (coordinator, draft, storage_copy)
    };

    let revision = coordinator
        .import_toml_from(Path::new("C:\\tmp\\import.toml"), &mut draft)
        .expect("import");

    assert_eq!(revision, coordinator.persisted_revision());
    assert_eq!(
        draft.general.startup_with_windows,
        imported.general.startup_with_windows
    );
    assert_eq!(storage_probe.saved_payloads(), vec![imported]);
}

#[test]
fn nested_battery_profiles_do_not_reach_storage_or_replace_settings() {
    let mut invalid = Settings::default();
    invalid.battery_profile_mut().battery_profile_mut();
    for importing in [false, true] {
        let storage = FakeStorage::new(Settings::default(), Ok(invalid.clone()), Ok(()), Ok(()));
        let (mut coordinator, mut draft) =
            SettingsCoordinator::load_from(Box::new(storage.clone()));
        let persisted = coordinator.persisted().clone();
        let revision = coordinator.persisted_revision();
        let runtime = coordinator.runtime_settings_snapshot(&draft);
        if !importing {
            draft.value = invalid.clone();
        }
        let original_draft = draft.clone();
        let result = if importing {
            coordinator.import_toml_from(Path::new("nested.toml"), &mut draft)
        } else {
            coordinator.save(&mut draft)
        };
        assert!(result.is_err());
        assert!(storage.saved_payloads().is_empty());
        assert_eq!(coordinator.persisted(), &persisted);
        assert_eq!(coordinator.persisted_revision(), revision);
        assert_eq!(draft, original_draft);
        let after = coordinator.runtime_settings_snapshot(&draft);
        assert_eq!(after.runtime_revision, runtime.runtime_revision);
        assert!(Arc::ptr_eq(&after.value, &runtime.value));
    }
}

#[test]
fn import_normalizes_shared_fields_without_flattening_feature_profiles() {
    let mut imported = Settings::default();
    imported.general.check_interval_ms = 2500;
    imported.battery_profile_mut().general.check_interval_ms = 5000;
    imported.battery_profile_mut().cpu_limiter.enabled = true;
    imported.cpu_limiter.enabled = false;
    let storage = FakeStorage::new(Settings::default(), Ok(imported.clone()), Ok(()), Ok(()));
    let (mut coordinator, mut draft) = SettingsCoordinator::load_from(Box::new(storage.clone()));
    coordinator
        .import_toml_from(Path::new("import.toml"), &mut draft)
        .unwrap();
    imported.sync_shared_settings_to_battery();
    assert_eq!(storage.saved_payloads(), vec![imported.clone()]);
    assert_eq!(draft.value, imported);
    assert_eq!(coordinator.persisted, imported);
    assert!(draft.value.battery_profile().cpu_limiter.enabled);
    assert!(!draft.value.cpu_limiter.enabled);
    coordinator
        .export_toml_to(Path::new("export.toml"), &draft)
        .unwrap();
    assert_eq!(storage.exported_payloads()[0].1, imported);
    coordinator.save(&mut draft).unwrap();
    assert_eq!(storage.saved_payloads()[1], imported);
}

#[test]
fn export_forwards_current_draft_to_storage() {
    let (coordinator, mut draft, storage_probe) = load_ok_with_fixtures();
    draft.general.check_interval_ms = 2_500;
    let path = Path::new("C:\\tmp\\settings.toml");
    coordinator.export_toml_to(path, &draft).expect("export");
    let payloads = storage_probe.exported_payloads();
    let (export_path, export_settings) = payloads.last().cloned().expect("export payload");
    assert_eq!(export_path, path.to_path_buf());
    assert_eq!(export_settings.general.check_interval_ms, 2_500);
}

#[test]
fn settings_editor_applies_persisted_startup_intent_after_save() {
    let storage = FakeStorage::new(Settings::default(), Ok(Settings::default()), Ok(()), Ok(()));
    let startup = FakeStartupRegistration::new(Ok(()));
    let (mut editor, storage, startup) = editor_with_fixtures(storage, startup);
    editor.general.startup_with_windows = true;

    let outcome = editor.save().expect("save settings");

    assert!(outcome.startup_registration_error().is_none());
    assert_eq!(startup.applied_values(), vec![true]);
    assert!(storage.saved_payloads()[0].general.startup_with_windows);
    assert!(editor.persisted().general.startup_with_windows);
}

#[test]
fn settings_editor_reports_startup_failure_without_rolling_back_saved_settings() {
    let storage = FakeStorage::new(Settings::default(), Ok(Settings::default()), Ok(()), Ok(()));
    let expected = StartupRegistrationError::WriteRegistryValue("access denied".to_owned());
    let startup = FakeStartupRegistration::new(Err(expected.clone()));
    let (mut editor, storage, startup) = editor_with_fixtures(storage, startup);
    editor.general.startup_with_windows = true;

    let outcome = editor.save().expect("settings remain committed");

    assert_eq!(outcome.startup_registration_error(), Some(&expected));
    assert_eq!(startup.applied_values(), vec![true]);
    assert!(storage.saved_payloads()[0].general.startup_with_windows);
    assert!(editor.persisted().general.startup_with_windows);
    assert!(!editor.has_unsaved_changes());
}

#[test]
fn settings_editor_does_not_apply_startup_when_settings_save_fails() {
    let storage = FakeStorage::new(
        Settings::default(),
        Ok(Settings::default()),
        Err("permission denied".to_owned()),
        Ok(()),
    );
    let startup = FakeStartupRegistration::new(Ok(()));
    let (mut editor, _storage, startup) = editor_with_fixtures(storage, startup);
    editor.general.startup_with_windows = true;

    assert!(matches!(
        editor.save(),
        Err(SettingsCoordinatorError::Save(_))
    ));
    assert!(startup.applied_values().is_empty());
    assert!(!editor.persisted().general.startup_with_windows);
}

#[test]
fn settings_editor_reconciles_persisted_startup_intent_on_load() {
    let mut loaded = Settings::default();
    loaded.general.startup_with_windows = true;
    let storage = FakeStorage::new(loaded, Ok(Settings::default()), Ok(()), Ok(()));
    let startup = FakeStartupRegistration::new(Ok(()));
    let startup_probe = startup.clone();

    let (editor, outcome) = SettingsEditor::load_from(Box::new(storage), Box::new(startup));

    assert!(outcome.startup_registration_error().is_none());
    assert!(editor.persisted().general.startup_with_windows);
    assert_eq!(startup_probe.applied_values(), vec![true]);
}
