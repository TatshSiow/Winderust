use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::sync::Arc;

use crate::backend::startup::{self, StartupRegistrationError};
use crate::config::{self, Settings};
use crate::foreground::{executable_path_key, same_executable_path};

pub(crate) type SettingsResult<T> = Result<T, SettingsCoordinatorError>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct SettingsRevision(u64);

impl SettingsRevision {
    pub const fn initial() -> Self {
        Self(0)
    }

    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SettingsDraft {
    base_revision: SettingsRevision,
    value: Settings,
    edit_revision: u64,
}

impl SettingsDraft {
    fn base_revision(&self) -> SettingsRevision {
        self.base_revision
    }

    fn mark_changed(&mut self) {
        self.edit_revision = self.edit_revision.wrapping_add(1);
    }
}

impl Deref for SettingsDraft {
    type Target = Settings;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl DerefMut for SettingsDraft {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.mark_changed();
        &mut self.value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoExclusionPatch {
    pub base_revision: SettingsRevision,
    pub app_suspension: Vec<String>,
    pub cpu_sets_soft: Vec<String>,
    pub processor_affinity_hard: Vec<String>,
    pub core_limiter: Vec<String>,
    pub workload_engine: Vec<String>,
    pub io_priority: Vec<String>,
    pub process_priority: Vec<String>,
    pub thread_priority: Vec<String>,
    pub dynamic_priority_boost: Vec<String>,
    pub gpu_priority: Vec<String>,
    pub memory_priority: Vec<String>,
    pub memory_trim: Vec<String>,
}

impl Default for AutoExclusionPatch {
    fn default() -> Self {
        Self {
            base_revision: SettingsRevision::initial(),
            app_suspension: Vec::new(),
            cpu_sets_soft: Vec::new(),
            processor_affinity_hard: Vec::new(),
            core_limiter: Vec::new(),
            workload_engine: Vec::new(),
            io_priority: Vec::new(),
            process_priority: Vec::new(),
            thread_priority: Vec::new(),
            dynamic_priority_boost: Vec::new(),
            gpu_priority: Vec::new(),
            memory_priority: Vec::new(),
            memory_trim: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavigationCollapsedPatch {
    pub base_revision: SettingsRevision,
    pub navigation_collapsed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSettingsSnapshot {
    pub runtime_revision: SettingsRevision,
    pub persisted_revision: SettingsRevision,
    pub value: Arc<Settings>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PersistentSettingsOutcome {
    startup_registration_error: Option<StartupRegistrationError>,
}

impl PersistentSettingsOutcome {
    pub(crate) fn startup_registration_error(&self) -> Option<&StartupRegistrationError> {
        self.startup_registration_error.as_ref()
    }
}

trait StartupRegistration: Send + Sync {
    fn set_enabled(&self, enabled: bool) -> Result<(), StartupRegistrationError>;
}

#[derive(Debug, Default)]
struct WindowsStartupRegistration;

impl StartupRegistration for WindowsStartupRegistration {
    fn set_enabled(&self, enabled: bool) -> Result<(), StartupRegistrationError> {
        startup::set_startup_with_windows(enabled)
    }
}

struct SettingsCoordinator {
    persisted: Settings,
    persisted_revision: SettingsRevision,
    runtime_snapshot: RuntimeSettingsSnapshot,
    projected_draft_revision: u64,
    storage: Box<dyn SettingsStorage>,
}

pub(crate) struct SettingsEditor {
    coordinator: SettingsCoordinator,
    draft: SettingsDraft,
    startup_registration: Box<dyn StartupRegistration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SettingsCoordinatorError {
    StaleDraft {
        draft_revision: SettingsRevision,
        persisted_revision: SettingsRevision,
    },
    StalePatch {
        patch_revision: SettingsRevision,
        persisted_revision: SettingsRevision,
    },
    Load(String),
    Save(String),
    Export(String),
    Import(String),
}

impl std::fmt::Display for SettingsCoordinatorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StaleDraft {
                draft_revision,
                persisted_revision,
            } => write!(
                formatter,
                "Settings changed after this draft was created (draft revision {draft_revision:?}, persisted revision {persisted_revision:?})."
            ),
            Self::StalePatch {
                patch_revision,
                persisted_revision,
            } => write!(
                formatter,
                "Settings changed before a runtime patch could be applied (patch revision {patch_revision:?}, persisted revision {persisted_revision:?})."
            ),
            Self::Load(error) | Self::Save(error) | Self::Export(error) | Self::Import(error) => {
                formatter.write_str(error)
            }
        }
    }
}

impl std::error::Error for SettingsCoordinatorError {}

impl SettingsCoordinator {
    fn load_from(storage: Box<dyn SettingsStorage>) -> SettingsResult<(Self, SettingsDraft)> {
        let settings = storage.load().map_err(SettingsCoordinatorError::Load)?;
        Ok(Self::from_loaded_settings(settings, storage))
    }

    fn from_loaded_settings(
        settings: Settings,
        storage: Box<dyn SettingsStorage>,
    ) -> (Self, SettingsDraft) {
        let revision = SettingsRevision::initial().next();
        let runtime_snapshot = RuntimeSettingsSnapshot {
            runtime_revision: SettingsRevision::initial(),
            persisted_revision: revision,
            value: Arc::new(runtime_settings_for(&settings, &settings)),
        };
        (
            Self {
                persisted: settings.clone(),
                persisted_revision: revision,
                runtime_snapshot,
                projected_draft_revision: 0,
                storage,
            },
            SettingsDraft {
                base_revision: revision,
                value: settings,
                edit_revision: 0,
            },
        )
    }

    fn with_settings(settings: Settings) -> (Self, SettingsDraft) {
        Self::from_loaded_settings(settings, Box::new(ConfigStorage))
    }

    fn runtime_settings_snapshot(&mut self, draft: &SettingsDraft) -> RuntimeSettingsSnapshot {
        self.refresh_runtime_settings_snapshot(draft)
    }

    fn refresh_runtime_settings_snapshot(
        &mut self,
        draft: &SettingsDraft,
    ) -> RuntimeSettingsSnapshot {
        let projection_changed = self.projected_draft_revision != draft.edit_revision
            || self.runtime_snapshot.persisted_revision != self.persisted_revision;
        if projection_changed {
            let projected = runtime_settings_for(&draft.value, &self.persisted);
            if projected != *self.runtime_snapshot.value {
                self.runtime_snapshot.runtime_revision =
                    self.runtime_snapshot.runtime_revision.next();
                self.runtime_snapshot.value = Arc::new(projected);
            }
            self.projected_draft_revision = draft.edit_revision;
        }
        self.runtime_snapshot.persisted_revision = self.persisted_revision;
        self.runtime_snapshot.clone()
    }

    fn apply_navigation_collapsed_patch(
        &mut self,
        draft: &mut SettingsDraft,
        patch: NavigationCollapsedPatch,
    ) -> SettingsResult<bool> {
        self.apply_runtime_patch(draft, patch.base_revision, |settings| {
            set_enabled_value(
                &mut settings.general.navigation_collapsed,
                patch.navigation_collapsed,
            )
        })
    }

    fn apply_auto_exclusion_patch(
        &mut self,
        draft: &mut SettingsDraft,
        patch: &AutoExclusionPatch,
    ) -> SettingsResult<bool> {
        self.apply_runtime_patch(draft, patch.base_revision, |settings| {
            apply_auto_exclusion_patch_to(settings, patch)
        })
    }

    fn apply_runtime_patch(
        &mut self,
        draft: &mut SettingsDraft,
        patch_revision: SettingsRevision,
        apply: impl Fn(&mut Settings) -> bool,
    ) -> SettingsResult<bool> {
        ensure_patch_revision(self.persisted_revision, patch_revision)?;
        ensure_draft_revision(self.persisted_revision, draft.base_revision)?;

        let mut persisted = self.persisted.clone();
        let persisted_changed = apply(&mut persisted);
        let mut draft_value = draft.value.clone();
        let draft_changed = apply(&mut draft_value);
        if !persisted_changed && !draft_changed {
            return Ok(false);
        }

        if persisted_changed {
            self.storage
                .save(&persisted)
                .map_err(SettingsCoordinatorError::Save)?;
            self.persisted = persisted;
            self.persisted_revision = self.persisted_revision.next();
        }

        draft.value = draft_value;
        draft.mark_changed();
        draft.base_revision = self.persisted_revision;
        self.refresh_runtime_settings_snapshot(draft);
        Ok(true)
    }

    fn save(&mut self, draft: &mut SettingsDraft) -> SettingsResult<SettingsRevision> {
        ensure_draft_revision(self.persisted_revision, draft.base_revision)?;
        let candidate = draft.value.clone();
        self.storage
            .save(&candidate)
            .map_err(SettingsCoordinatorError::Save)?;
        let next_revision = self.persisted_revision.next();
        self.persisted = candidate;
        self.persisted_revision = next_revision;
        draft.base_revision = next_revision;
        self.refresh_runtime_settings_snapshot(draft);
        Ok(next_revision)
    }

    fn import_toml_from(
        &mut self,
        path: &Path,
        draft: &mut SettingsDraft,
    ) -> SettingsResult<SettingsRevision> {
        let imported = self
            .storage
            .import(path)
            .map_err(SettingsCoordinatorError::Import)?;
        self.storage
            .save(&imported)
            .map_err(SettingsCoordinatorError::Save)?;
        let next_revision = self.persisted_revision.next();
        self.persisted = imported.clone();
        self.persisted_revision = next_revision;
        draft.base_revision = next_revision;
        draft.value = imported;
        draft.mark_changed();
        self.refresh_runtime_settings_snapshot(draft);
        Ok(next_revision)
    }

    fn cancel(&mut self, draft: &mut SettingsDraft) {
        draft.base_revision = self.persisted_revision;
        draft.value = self.persisted.clone();
        draft.mark_changed();
        self.runtime_settings_snapshot(draft);
    }

    fn export_toml_to(&self, path: &Path, draft: &SettingsDraft) -> SettingsResult<()> {
        self.storage
            .export(path, &draft.value)
            .map_err(SettingsCoordinatorError::Export)
    }

    fn persisted(&self) -> &Settings {
        &self.persisted
    }

    fn has_unsaved_changes(&self, draft: &SettingsDraft) -> bool {
        draft.value != self.persisted
    }

    #[cfg(test)]
    fn persisted_revision(&self) -> SettingsRevision {
        self.persisted_revision
    }
}

impl SettingsEditor {
    fn from_parts(
        coordinator: SettingsCoordinator,
        draft: SettingsDraft,
        startup_registration: Box<dyn StartupRegistration>,
    ) -> Self {
        Self {
            coordinator,
            draft,
            startup_registration,
        }
    }

    fn load_from(
        storage: Box<dyn SettingsStorage>,
        startup_registration: Box<dyn StartupRegistration>,
    ) -> SettingsResult<(Self, PersistentSettingsOutcome)> {
        let (coordinator, draft) = SettingsCoordinator::load_from(storage)?;
        let editor = Self::from_parts(coordinator, draft, startup_registration);
        let outcome = editor.reconcile_startup_registration();
        Ok((editor, outcome))
    }

    pub(crate) fn load() -> SettingsResult<(Self, PersistentSettingsOutcome)> {
        Self::load_from(
            Box::new(ConfigStorage),
            Box::<WindowsStartupRegistration>::default(),
        )
    }

    pub(crate) fn with_settings(settings: Settings) -> Self {
        let (coordinator, draft) = SettingsCoordinator::with_settings(settings);
        Self::from_parts(
            coordinator,
            draft,
            Box::<WindowsStartupRegistration>::default(),
        )
    }

    pub(crate) fn base_revision(&self) -> SettingsRevision {
        self.draft.base_revision()
    }

    pub(crate) fn runtime_settings_snapshot(&mut self) -> RuntimeSettingsSnapshot {
        self.coordinator.runtime_settings_snapshot(&self.draft)
    }

    pub(crate) fn apply_navigation_collapsed_patch(
        &mut self,
        patch: NavigationCollapsedPatch,
    ) -> SettingsResult<bool> {
        self.coordinator
            .apply_navigation_collapsed_patch(&mut self.draft, patch)
    }

    pub(crate) fn apply_auto_exclusion_patch(
        &mut self,
        patch: &AutoExclusionPatch,
    ) -> SettingsResult<bool> {
        self.coordinator
            .apply_auto_exclusion_patch(&mut self.draft, patch)
    }

    pub(crate) fn save(&mut self) -> SettingsResult<PersistentSettingsOutcome> {
        let _ = self.coordinator.save(&mut self.draft)?;
        Ok(self.reconcile_startup_registration())
    }

    pub(crate) fn import_toml_from(
        &mut self,
        path: &Path,
    ) -> SettingsResult<PersistentSettingsOutcome> {
        let _ = self.coordinator.import_toml_from(path, &mut self.draft)?;
        Ok(self.reconcile_startup_registration())
    }

    pub(crate) fn cancel(&mut self) {
        self.coordinator.cancel(&mut self.draft);
    }

    pub(crate) fn export_toml_to(&self, path: &Path) -> SettingsResult<()> {
        self.coordinator.export_toml_to(path, &self.draft)
    }

    pub(crate) fn persisted(&self) -> &Settings {
        self.coordinator.persisted()
    }

    pub(crate) fn has_unsaved_changes(&self) -> bool {
        self.coordinator.has_unsaved_changes(&self.draft)
    }

    fn reconcile_startup_registration(&self) -> PersistentSettingsOutcome {
        PersistentSettingsOutcome {
            startup_registration_error: self
                .startup_registration
                .set_enabled(self.coordinator.persisted().general.startup_with_windows)
                .err(),
        }
    }
}

impl Deref for SettingsEditor {
    type Target = Settings;

    fn deref(&self) -> &Self::Target {
        self.draft.deref()
    }
}

impl DerefMut for SettingsEditor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.draft.deref_mut()
    }
}

fn ensure_draft_revision(
    persisted_revision: SettingsRevision,
    draft_revision: SettingsRevision,
) -> SettingsResult<()> {
    if persisted_revision == draft_revision {
        return Ok(());
    }
    Err(SettingsCoordinatorError::StaleDraft {
        draft_revision,
        persisted_revision,
    })
}

fn ensure_patch_revision(
    persisted_revision: SettingsRevision,
    patch_revision: SettingsRevision,
) -> SettingsResult<()> {
    if persisted_revision == patch_revision {
        return Ok(());
    }
    Err(SettingsCoordinatorError::StalePatch {
        patch_revision,
        persisted_revision,
    })
}

fn set_enabled_value(current: &mut bool, enabled: bool) -> bool {
    if *current == enabled {
        return false;
    }
    *current = enabled;
    true
}

fn apply_auto_exclusion_patch_to(settings: &mut Settings, patch: &AutoExclusionPatch) -> bool {
    let mut changed = false;
    changed |= apply_auto_exclusion_paths(
        &mut settings.app_suspension.suspendable_apps,
        &patch.app_suspension,
        false,
        app_suspension_rule,
        |rule| &rule.executable_path,
        |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
    );
    changed |= apply_auto_exclusion_paths(
        &mut settings.cpu_sets_soft.rules,
        &patch.cpu_sets_soft,
        false,
        cpu_allocation_rule,
        |rule| &rule.executable_path,
        |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
    );
    changed |= apply_auto_exclusion_paths(
        &mut settings.processor_affinity_hard.rules,
        &patch.processor_affinity_hard,
        false,
        cpu_allocation_rule,
        |rule| &rule.executable_path,
        |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
    );
    changed |= apply_auto_exclusion_paths(
        &mut settings.core_limiter.rules,
        &patch.core_limiter,
        false,
        core_limiter_rule,
        |rule| &rule.executable_path,
        |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
    );
    changed |= apply_auto_exclusion_paths(
        &mut settings.workload_engine.workload_engine_exclusions,
        &patch.workload_engine,
        true,
        process_exclusion_rule,
        |rule| &rule.executable_path,
        |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
    );
    changed |= apply_auto_exclusion_paths(
        &mut settings.io_priority.exclusions,
        &patch.io_priority,
        true,
        process_exclusion_rule,
        |rule| &rule.executable_path,
        |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
    );
    changed |= apply_auto_exclusion_paths(
        &mut settings.process_priority.exclusions,
        &patch.process_priority,
        true,
        process_exclusion_rule,
        |rule| &rule.executable_path,
        |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
    );
    changed |= apply_auto_exclusion_paths(
        &mut settings.thread_priority.exclusions,
        &patch.thread_priority,
        true,
        process_exclusion_rule,
        |rule| &rule.executable_path,
        |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
    );
    changed |= apply_auto_exclusion_paths(
        &mut settings.dynamic_priority_boost.exclusions,
        &patch.dynamic_priority_boost,
        true,
        process_exclusion_rule,
        |rule| &rule.executable_path,
        |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
    );
    changed |= apply_auto_exclusion_paths(
        &mut settings.gpu_priority.exclusions,
        &patch.gpu_priority,
        true,
        process_exclusion_rule,
        |rule| &rule.executable_path,
        |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
    );
    changed |= apply_auto_exclusion_paths(
        &mut settings.memory_priority.exclusions,
        &patch.memory_priority,
        true,
        process_exclusion_rule,
        |rule| &rule.executable_path,
        |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
    );
    changed |= apply_auto_exclusion_paths(
        &mut settings.memory_trim.exclusions,
        &patch.memory_trim,
        true,
        process_exclusion_rule,
        |rule| &rule.executable_path,
        |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
    );
    changed
}

fn apply_auto_exclusion_paths<T>(
    rules: &mut Vec<T>,
    incoming: &[String],
    enabled_value: bool,
    new_rule: impl Fn(&str) -> T,
    executable_path: impl Fn(&T) -> &str,
    set_enabled: impl Fn(&mut T, bool) -> bool,
) -> bool {
    let mut changed = false;

    for incoming_path in incoming {
        let path = executable_path_key(Path::new(incoming_path));
        if !Path::new(&path).is_absolute() {
            continue;
        }
        if let Some(rule) = rules
            .iter_mut()
            .find(|rule| same_executable_path(Path::new(executable_path(*rule)), Path::new(&path)))
        {
            changed |= set_enabled(rule, enabled_value);
            continue;
        }

        let mut created = new_rule(&path);
        let _ = set_enabled(&mut created, enabled_value);
        rules.push(created);
        changed = true;
    }

    changed
}

fn app_suspension_rule(path: &str) -> crate::config::AppSuspensionRule {
    crate::config::AppSuspensionRule {
        enabled: false,
        executable_path: path.to_owned(),
        network_wake_enabled: true,
        audio_wake_enabled: true,
        network_download_threshold_bytes: 1,
        network_download_threshold_unit: crate::config::NetworkThresholdUnit::Bytes,
        network_upload_threshold_bytes: 0,
        network_upload_threshold_unit: crate::config::NetworkThresholdUnit::Bytes,
    }
}

fn cpu_allocation_rule(path: &str) -> crate::config::CpuAllocationRule {
    let core_mask = crate::features::cpu_control::cpu_allocation::default_cpu_mask();
    crate::config::CpuAllocationRule {
        enabled: false,
        executable_path: path.to_owned(),
        focus_core_mask: core_mask,
        visible_window_core_mask: core_mask,
        background_core_mask: core_mask,
    }
}

fn core_limiter_rule(path: &str) -> crate::config::CoreLimiterRule {
    crate::config::CoreLimiterRule {
        enabled: false,
        executable_path: path.to_owned(),
        focus_mode: crate::config::ProcessRuleMode::Default,
        visible_window_mode: crate::config::ProcessRuleMode::Default,
        background_mode: crate::config::ProcessRuleMode::Default,
        threshold_percent: 75,
        sustain_seconds: 5,
        cooldown_seconds: 10,
        max_logical_processors: 1,
    }
}

fn process_exclusion_rule(path: &str) -> crate::config::ProcessExclusionRule {
    crate::config::ProcessExclusionRule {
        enabled: true,
        executable_path: path.to_owned(),
        ..Default::default()
    }
}

pub fn runtime_settings_for(current: &Settings, persisted: &Settings) -> Settings {
    let mut projected = persisted.clone();
    projected.general = current.general.clone();
    projected.general.enabled = persisted.general.enabled;
    projected.advanced = current.advanced.clone();
    projected.cpu_allocation_presets.clear();
    projected
}

trait SettingsStorage: Send + Sync {
    fn load(&self) -> Result<Settings, String>;
    fn save(&self, settings: &Settings) -> Result<(), String>;
    fn import(&self, path: &Path) -> Result<Settings, String>;
    fn export(&self, path: &Path, settings: &Settings) -> Result<(), String>;
}

#[derive(Debug)]
struct ConfigStorage;

impl SettingsStorage for ConfigStorage {
    fn load(&self) -> Result<Settings, String> {
        config::storage::load()
    }

    fn save(&self, settings: &Settings) -> Result<(), String> {
        config::storage::save(settings)
    }

    fn import(&self, path: &Path) -> Result<Settings, String> {
        config::storage::import_toml_from(path)
    }

    fn export(&self, path: &Path, settings: &Settings) -> Result<(), String> {
        config::storage::export_toml_to(path, settings)
    }
}

#[cfg(test)]
mod tests {
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

    fn load_ok_with_fixtures() -> (SettingsCoordinator, SettingsDraft, FakeStorage) {
        let storage =
            FakeStorage::new(Settings::default(), Ok(Settings::default()), Ok(()), Ok(()));
        let storage_copy = storage.clone();
        let (coordinator, draft) = SettingsCoordinator::load_from(Box::new(storage)).expect("load");
        (coordinator, draft, storage_copy)
    }

    fn editor_with_fixtures(
        storage: FakeStorage,
        startup: FakeStartupRegistration,
    ) -> (SettingsEditor, FakeStorage, FakeStartupRegistration) {
        let storage_probe = storage.clone();
        let startup_probe = startup.clone();
        let (coordinator, draft) = SettingsCoordinator::load_from(Box::new(storage)).expect("load");
        (
            SettingsEditor::from_parts(coordinator, draft, Box::new(startup)),
            storage_probe,
            startup_probe,
        )
    }

    #[test]
    fn stale_patch_revision_is_rejected_and_keeps_draft() {
        let (mut coordinator, mut draft, _storage) = load_ok_with_fixtures();
        draft.general.check_interval_ms = 1_337;
        let stale = coordinator.persisted_revision().next();
        let patch = NavigationCollapsedPatch {
            base_revision: stale,
            navigation_collapsed: true,
        };

        let result = coordinator.apply_navigation_collapsed_patch(&mut draft, patch);
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
    fn navigation_patch_persists_only_navigation_and_preserves_other_draft_edits() {
        let (mut coordinator, mut draft, storage) = load_ok_with_fixtures();
        draft.general.check_interval_ms = 1_337;
        let patch = NavigationCollapsedPatch {
            base_revision: draft.base_revision,
            navigation_collapsed: true,
        };

        assert!(coordinator
            .apply_navigation_collapsed_patch(&mut draft, patch)
            .expect("apply navigation patch"));

        let saved = storage.saved_payloads();
        assert_eq!(saved.len(), 1);
        assert!(saved[0].general.navigation_collapsed);
        assert_eq!(
            saved[0].general.check_interval_ms,
            Settings::default().general.check_interval_ms
        );
        assert_eq!(draft.general.check_interval_ms, 1_337);
        assert!(draft.general.navigation_collapsed);
    }

    #[test]
    fn failed_runtime_patch_does_not_mutate_persisted_settings_or_draft() {
        let storage = FakeStorage::new(
            Settings::default(),
            Ok(Settings::default()),
            Err("permission denied".to_owned()),
            Ok(()),
        );
        let (mut coordinator, mut draft) =
            SettingsCoordinator::load_from(Box::new(storage)).expect("load");
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
    fn runtime_snapshot_updates_with_full_equality_for_priority_sections() {
        let (mut coordinator, mut draft, _storage) = load_ok_with_fixtures();

        let s1 = coordinator.runtime_settings_snapshot(&draft);
        draft.process_priority.foreground_priority =
            crate::config::ProcessPrioritySetting::AboveNormal;
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
        let (mut coordinator, mut draft) =
            SettingsCoordinator::load_from(Box::new(storage.clone())).expect("load");

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
            let (coordinator, draft) =
                SettingsCoordinator::load_from(Box::new(storage_copy.clone())).expect("load");
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
    fn runtime_snapshot_has_persisted_revision_projection() {
        let (mut coordinator, draft, _storage) = load_ok_with_fixtures();
        let snapshot = coordinator.runtime_settings_snapshot(&draft);
        assert_eq!(
            snapshot.persisted_revision,
            coordinator.persisted_revision()
        );
    }

    #[test]
    fn settings_editor_applies_persisted_startup_intent_after_save() {
        let storage =
            FakeStorage::new(Settings::default(), Ok(Settings::default()), Ok(()), Ok(()));
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
        let storage =
            FakeStorage::new(Settings::default(), Ok(Settings::default()), Ok(()), Ok(()));
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

        let (editor, outcome) =
            SettingsEditor::load_from(Box::new(storage), Box::new(startup)).expect("load");

        assert!(outcome.startup_registration_error().is_none());
        assert!(editor.persisted().general.startup_with_windows);
        assert_eq!(startup_probe.applied_values(), vec![true]);
    }
}
