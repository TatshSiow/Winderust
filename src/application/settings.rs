use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::sync::Arc;

use crate::backend::startup::{self, StartupRegistrationError};
use crate::config::{self, PowerSourceProfile, Settings};
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoExclusionPatch {
    pub base_revision: SettingsRevision,
    pub app_suspension: Vec<String>,
    pub cpu_sets_soft: Vec<String>,
    pub processor_affinity_hard: Vec<String>,
    pub cpu_limiter: Vec<String>,
    pub adaptive_engine_process: Vec<String>,
    pub io_priority: Vec<String>,
    pub process_priority: Vec<String>,
    pub thread_priority: Vec<String>,
    pub dynamic_priority_boost: Vec<String>,
    pub gpu_priority: Vec<String>,
    pub memory_priority: Vec<String>,
    pub memory_trim: Vec<String>,
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
    load_error: Option<String>,
    storage: Box<dyn SettingsStorage>,
}

pub(crate) struct SettingsEditor {
    coordinator: SettingsCoordinator,
    draft: SettingsDraft,
    selected_power_source: PowerSourceProfile,
    startup_registration: Box<dyn StartupRegistration>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum SettingsCoordinatorError {
    #[error("Settings changed after this draft was created (draft revision {draft_revision:?}, persisted revision {persisted_revision:?}).")]
    StaleDraft {
        draft_revision: SettingsRevision,
        persisted_revision: SettingsRevision,
    },
    #[error("Settings changed before a runtime patch could be applied (patch revision {patch_revision:?}, persisted revision {persisted_revision:?}).")]
    StalePatch {
        patch_revision: SettingsRevision,
        persisted_revision: SettingsRevision,
    },
    #[error("Automatic saving is disabled because settings could not be loaded. Repair settings.toml and restart, or explicitly Save or Import settings to replace it.")]
    RecoveryRequired,
    #[error("{0}")]
    Save(String),
    #[error("{0}")]
    Export(String),
    #[error("{0}")]
    Import(String),
}

impl SettingsCoordinator {
    fn load_from(storage: Box<dyn SettingsStorage>) -> (Self, SettingsDraft) {
        let (settings, load_error) = match storage.load() {
            Ok(settings) => (settings, None),
            Err(error) => (Settings::default(), Some(error)),
        };
        let (mut coordinator, draft) = Self::from_loaded_settings(settings, storage);
        coordinator.load_error = load_error;
        (coordinator, draft)
    }

    fn from_loaded_settings(
        mut settings: Settings,
        storage: Box<dyn SettingsStorage>,
    ) -> (Self, SettingsDraft) {
        settings.sync_shared_settings_to_battery();
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
                load_error: None,
                storage,
            },
            SettingsDraft {
                base_revision: revision,
                value: settings,
                edit_revision: 0,
            },
        )
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

        if self.load_error.is_some() {
            return Err(SettingsCoordinatorError::RecoveryRequired);
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
        let mut candidate = draft.value.clone();
        candidate
            .validate()
            .map_err(SettingsCoordinatorError::Save)?;
        candidate.sync_shared_settings_to_battery();
        self.storage
            .save(&candidate)
            .map_err(SettingsCoordinatorError::Save)?;
        self.load_error = None;
        let next_revision = self.persisted_revision.next();
        self.persisted = candidate;
        self.persisted_revision = next_revision;
        draft.value = self.persisted.clone();
        draft.base_revision = next_revision;
        self.refresh_runtime_settings_snapshot(draft);
        Ok(next_revision)
    }

    fn import_toml_from(
        &mut self,
        path: &Path,
        draft: &mut SettingsDraft,
    ) -> SettingsResult<SettingsRevision> {
        let mut imported = self
            .storage
            .import(path)
            .map_err(SettingsCoordinatorError::Import)?;
        imported
            .validate()
            .map_err(SettingsCoordinatorError::Import)?;
        imported.sync_shared_settings_to_battery();
        self.storage
            .save(&imported)
            .map_err(SettingsCoordinatorError::Save)?;
        self.load_error = None;
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
        draft
            .value
            .validate()
            .map_err(SettingsCoordinatorError::Export)?;
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
            selected_power_source: PowerSourceProfile::PluggedIn,
            startup_registration,
        }
    }

    fn load_from(
        storage: Box<dyn SettingsStorage>,
        startup_registration: Box<dyn StartupRegistration>,
    ) -> (Self, PersistentSettingsOutcome) {
        let (coordinator, draft) = SettingsCoordinator::load_from(storage);
        let editor = Self::from_parts(coordinator, draft, startup_registration);
        let outcome = if editor.load_error().is_some() {
            PersistentSettingsOutcome {
                startup_registration_error: None,
            }
        } else {
            editor.reconcile_startup_registration()
        };
        (editor, outcome)
    }

    pub(crate) fn load() -> (Self, PersistentSettingsOutcome) {
        Self::load_from(
            Box::new(ConfigStorage),
            Box::<WindowsStartupRegistration>::default(),
        )
    }

    #[cfg(any(test, feature = "render-smoke"))]
    pub(crate) fn with_settings(settings: Settings) -> Self {
        let (coordinator, draft) =
            SettingsCoordinator::from_loaded_settings(settings, Box::new(ConfigStorage));
        Self::from_parts(
            coordinator,
            draft,
            Box::<WindowsStartupRegistration>::default(),
        )
    }

    pub(crate) fn load_error(&self) -> Option<&str> {
        self.coordinator.load_error.as_deref()
    }

    pub(crate) fn select_power_source(&mut self, profile: PowerSourceProfile) {
        self.selected_power_source = profile;
    }

    pub(crate) fn global(&self) -> &Settings {
        &self.draft.value
    }

    pub(crate) fn edit_global(&mut self, edit: impl FnOnce(&mut Settings)) {
        self.draft.mark_changed();
        edit(&mut self.draft.value);
        self.draft.value.sync_shared_settings_to_battery();
    }

    pub(crate) fn edit_with_presets(&mut self, edit: impl FnOnce(&mut Settings)) {
        self.draft.value.sync_shared_settings_to_battery();
        edit(self);
        // Preset collections are shared even when tuning the battery profile.
        if self.selected_power_source == PowerSourceProfile::OnBattery {
            let root = &mut self.draft.value;
            if let Some(battery) = root.on_battery.as_mut() {
                root.adaptive_engine_presets
                    .clone_from(&battery.adaptive_engine_presets);
                root.cpu_allocation_presets
                    .clone_from(&battery.cpu_allocation_presets);
                root.advanced_power_plan_tuning_presets
                    .clone_from(&battery.advanced_power_plan_tuning_presets);
            }
        }
        self.draft.value.sync_shared_settings_to_battery();
    }

    pub(crate) fn runtime_settings_snapshot(&mut self) -> RuntimeSettingsSnapshot {
        self.coordinator.runtime_settings_snapshot(&self.draft)
    }

    pub(crate) fn set_feature_enabled(
        &mut self,
        profile: PowerSourceProfile,
        field: fn(&mut Settings) -> &mut bool,
        enabled: bool,
    ) -> SettingsResult<bool> {
        self.coordinator.apply_runtime_patch(
            &mut self.draft,
            self.coordinator.persisted_revision,
            |settings| {
                // Preserve the other profile when battery settings still inherit the root.
                settings.battery_profile_mut();
                let settings = match profile {
                    PowerSourceProfile::PluggedIn => settings,
                    PowerSourceProfile::OnBattery => settings.battery_profile_mut(),
                };
                set_enabled_value(field(settings), enabled)
            },
        )
    }

    pub(crate) fn set_watchdog_enabled(&mut self, enabled: bool) -> SettingsResult<bool> {
        self.coordinator.apply_runtime_patch(
            &mut self.draft,
            self.coordinator.persisted_revision,
            |settings| {
                let changed = set_enabled_value(&mut settings.watchdog.enabled, enabled);
                settings.sync_shared_settings_to_battery();
                changed
            },
        )
    }

    pub(crate) fn set_master_enabled(&mut self, enabled: bool) -> SettingsResult<bool> {
        self.coordinator.apply_runtime_patch(
            &mut self.draft,
            self.coordinator.persisted_revision,
            |settings| {
                let mut changed = set_enabled_value(&mut settings.general.enabled, enabled);
                if let Some(battery) = settings.on_battery.as_deref_mut() {
                    changed |= set_enabled_value(&mut battery.general.enabled, enabled);
                }
                changed
            },
        )
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
        match self.selected_power_source {
            PowerSourceProfile::PluggedIn => &self.draft.value,
            PowerSourceProfile::OnBattery => self.draft.value.battery_profile(),
        }
    }
}

impl DerefMut for SettingsEditor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.draft.mark_changed();
        match self.selected_power_source {
            PowerSourceProfile::PluggedIn => &mut self.draft.value,
            PowerSourceProfile::OnBattery => self.draft.value.battery_profile_mut(),
        }
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
    let mut changed = apply_auto_exclusion_patch_to_profile(settings, patch);
    if let Some(battery) = settings.on_battery.as_deref_mut() {
        changed |= apply_auto_exclusion_patch_to_profile(battery, patch);
    }
    changed
}

fn apply_auto_exclusion_patch_to_profile(
    settings: &mut Settings,
    patch: &AutoExclusionPatch,
) -> bool {
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
        &mut settings.cpu_limiter.rules,
        &patch.cpu_limiter,
        false,
        cpu_limiter_rule,
        |rule| &rule.executable_path,
        |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
    );
    changed |= apply_auto_exclusion_paths(
        &mut settings.adaptive_engine_process.custom_rules,
        &patch.adaptive_engine_process,
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

fn cpu_limiter_rule(path: &str) -> crate::config::CpuLimiterRule {
    crate::config::CpuLimiterRule {
        enabled: false,
        executable_path: path.to_owned(),
        focus_mode: crate::config::ProcessRuleMode::Default,
        visible_window_mode: crate::config::ProcessRuleMode::Default,
        background_mode: crate::config::ProcessRuleMode::Default,
        focus_allowed_cpu_time_percent: 50,
        visible_window_allowed_cpu_time_percent: 50,
        background_allowed_cpu_time_percent: 50,
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
    projected.adaptive_engine_presets.clear();
    projected.cpu_allocation_presets.clear();
    projected.advanced_power_plan_tuning_presets.clear();
    projected.sync_shared_settings_to_battery();
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
mod tests;
