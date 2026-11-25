use super::{
    AdaptiveEngineProcessSettings, AppSuspensionSettings, ByCpuLoadRule, CpuLoadPercent,
    CpuUsageComparison, Settings, SleepPreventionSettings, ADAPTIVE_AFFINITY_REMOVAL_MAX,
    ADAPTIVE_RESTRAINT_IDLE_MAX_SECONDS, ADAPTIVE_RESTRAINT_IDLE_MIN_SECONDS,
    ADAPTIVE_RESTRAINT_MAX_TIME_MS, ADAPTIVE_RESTRAINT_TRIGGER_MAX_MS,
    ADAPTIVE_RESTRAINT_TRIGGER_MIN_MS, NESTED_BATTERY_PROFILE_ERROR,
};
use crate::config::{is_valid_executable_name, ExecutableSelector};
use std::collections::HashSet;

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        self.validate_power_source_profiles()?;
        self.validate_resource_thresholds()?;
        self.validate_keep_running()?;
        self.validate_instance_limits()?;
        self.validate_instance_balancing()?;
        self.validate_dynamic_resource_zones()?;
        self.validate_by_cpu_load_rules()?;
        for profile in self.profiles() {
            if profile.adaptive_engine.restraint_core_parking_min > 100 {
                return Err(rust_i18n::t!(
                    "adaptive_processor_power.restraint_core_parking_min_invalid"
                )
                .into_owned());
            }
            profile.adaptive_engine_process.validate_cpu_behaviour()?;
            for preset in &profile.adaptive_engine_presets {
                preset
                    .adaptive_engine_process
                    .validate_cpu_behaviour()
                    .map_err(|error| format!("Preset '{}': {error}", preset.name))?;
            }
            if profile
                .general
                .show_winderust_shortcut
                .is_some_and(|shortcut| {
                    shortcut.conflicts_with_search(&profile.general.search_hotkeys)
                })
            {
                return Err(
                    rust_i18n::t!("settings.show_winderust_shortcut_search_conflict").into_owned(),
                );
            }
            let mut preset_names = HashSet::new();
            for preset in &profile.adaptive_processor_power_presets {
                preset.validate()?;
                if !preset_names.insert(preset.name.trim().to_ascii_lowercase()) {
                    return Err(format!(
                        "Adaptive Processor Power preset name '{}' is already in use.",
                        preset.name
                    ));
                }
            }
            profile.app_suspension.validate()?;
            profile.sleep_prevention.validate()?;
            for rule in &profile.cpu_limiter.rules {
                if let Some(limit) = &rule.affinity_limit {
                    if !std::path::Path::new(&rule.executable_path).is_absolute()
                        || rule.executable_path.contains('\0')
                    {
                        return Err(
                            "CPU Limiter affinity rules require an absolute executable path."
                                .into(),
                        );
                    }
                    if !limit.is_valid() {
                        return Err(format!(
                            "CPU Limiter affinity rule '{}' requires a trigger of 1–100%, a duration of 1–600 seconds, and at least one processor in group 0.",
                            rule.executable_path
                        ));
                    }
                }
            }
            let focus = profile
                .by_focus
                .rules
                .iter()
                .map(|rule| ("By Focus", &rule.selector));
            let running = profile
                .by_running_app
                .rules
                .iter()
                .map(|rule| ("By Running App", &rule.selector));
            for (mode, selector) in focus.chain(running) {
                if let ExecutableSelector::ExactName { executable_name } = selector {
                    if !is_valid_executable_name(executable_name) {
                        return Err(format!("{mode} requires an exact executable filename."));
                    }
                }
            }
        }
        Ok(())
    }

    fn profiles(&self) -> impl Iterator<Item = &Self> {
        std::iter::once(self).chain(self.on_battery.as_deref())
    }

    pub fn validate_power_source_profiles(&self) -> Result<(), String> {
        if self
            .on_battery
            .as_deref()
            .is_some_and(|profile| profile.on_battery.is_some())
        {
            return Err(NESTED_BATTERY_PROFILE_ERROR.into());
        }
        Ok(())
    }

    pub fn validate_by_cpu_load_rules(&self) -> Result<(), String> {
        for profile in self.profiles() {
            for rule in &profile.by_cpu_load.rules {
                rule.validate()
                    .map_err(|error| format!("By CPU Load rule '{}': {error}", rule.name))?;
            }
        }
        Ok(())
    }

    pub fn validate_resource_thresholds(&self) -> Result<(), String> {
        for profile in self.profiles() {
            profile.resource_thresholds.validate()?;
        }
        Ok(())
    }

    pub fn validate_keep_running(&self) -> Result<(), String> {
        for profile in self.profiles() {
            profile.keep_running.validate()?;
        }
        Ok(())
    }

    pub fn validate_instance_limits(&self) -> Result<(), String> {
        for profile in self.profiles() {
            profile.instance_limits.validate()?;
        }
        Ok(())
    }

    pub fn validate_instance_balancing(&self) -> Result<(), String> {
        for profile in self.profiles() {
            profile.instance_balancing.validate()?;
        }
        Ok(())
    }

    pub fn validate_dynamic_resource_zones(&self) -> Result<(), String> {
        for profile in self.profiles() {
            let process = &profile.adaptive_engine_process;
            process
                .dynamic_resource_zone_settings
                .validate(process.dynamic_resource_zones_enabled)?;
            for preset in &profile.adaptive_engine_presets {
                let process = &preset.adaptive_engine_process;
                process
                    .dynamic_resource_zone_settings
                    .validate(process.dynamic_resource_zones_enabled)
                    .map_err(|error| format!("Preset '{}': {error}", preset.name))?;
            }
        }
        Ok(())
    }
}

impl AdaptiveEngineProcessSettings {
    pub(crate) fn validate_cpu_behaviour(&self) -> Result<(), String> {
        if self
            .foreground_cpu_threshold_percent
            .is_some_and(|threshold| !(1..=100).contains(&threshold))
        {
            return Err(
                rust_i18n::t!("adaptive_engine_process.foreground_cpu_threshold_invalid")
                    .into_owned(),
            );
        }
        if !(ADAPTIVE_RESTRAINT_TRIGGER_MIN_MS..=ADAPTIVE_RESTRAINT_TRIGGER_MAX_MS)
            .contains(&self.restraint_trigger_time_ms)
        {
            return Err(format!(
                "CPU Pressure Restraint time above threshold must be between {ADAPTIVE_RESTRAINT_TRIGGER_MIN_MS} and {ADAPTIVE_RESTRAINT_TRIGGER_MAX_MS} milliseconds."
            ));
        }
        if !(ADAPTIVE_RESTRAINT_IDLE_MIN_SECONDS..=ADAPTIVE_RESTRAINT_IDLE_MAX_SECONDS)
            .contains(&self.restraint_idle_timeout_seconds)
        {
            return Err(format!(
                "CPU Pressure Restraint idle timeout must be between {ADAPTIVE_RESTRAINT_IDLE_MIN_SECONDS} and {ADAPTIVE_RESTRAINT_IDLE_MAX_SECONDS} seconds."
            ));
        }
        let minimum = self.minimum_restraint_time_ms;
        let maximum = self.maximum_restraint_time_ms;
        if maximum != 0 && !(minimum..=ADAPTIVE_RESTRAINT_MAX_TIME_MS).contains(&maximum) {
            return Err(
                rust_i18n::t!("adaptive_engine_process.maximum_restraint_time_invalid")
                    .into_owned(),
            );
        }
        if !(1..=ADAPTIVE_AFFINITY_REMOVAL_MAX).contains(&self.processors_to_remove) {
            return Err(
                rust_i18n::t!("adaptive_engine_process.processors_to_remove_invalid").into_owned(),
            );
        }
        if self.custom_rules.iter().any(|rule| {
            matches!(&rule.selector, ExecutableSelector::ExactName { executable_name }
                if !is_valid_executable_name(executable_name))
        }) {
            return Err(
                rust_i18n::t!("adaptive_engine_process.custom_rule_name_invalid").into_owned(),
            );
        }
        Ok(())
    }
}

impl SleepPreventionSettings {
    pub fn validate(&self) -> Result<(), String> {
        for rule in &self.rules {
            let path = rule.executable_path.trim();
            if !std::path::Path::new(path).is_absolute() || path.contains('\0') {
                return Err("Sleep Prevention rules require an absolute executable path.".into());
            }
        }
        Ok(())
    }
}

impl AppSuspensionSettings {
    pub const MAX_DELAY_SECONDS: u64 = 86_400;
    pub const MAX_WAKE_DURATION_SECONDS: u64 = 3600;

    pub fn validate(&self) -> Result<(), String> {
        for (field, value, maximum) in [
            (
                "background_delay_seconds",
                self.background_delay_seconds,
                Self::MAX_DELAY_SECONDS,
            ),
            (
                "temporary_thaw_interval_seconds",
                self.temporary_thaw_interval_seconds,
                Self::MAX_DELAY_SECONDS,
            ),
            (
                "temporary_thaw_duration_seconds",
                self.temporary_thaw_duration_seconds,
                Self::MAX_WAKE_DURATION_SECONDS,
            ),
            (
                "network_wake_duration_seconds",
                self.network_wake_duration_seconds,
                Self::MAX_WAKE_DURATION_SECONDS,
            ),
            (
                "audio_wake_duration_seconds",
                self.audio_wake_duration_seconds,
                Self::MAX_WAKE_DURATION_SECONDS,
            ),
        ] {
            if !(1..=maximum).contains(&value) {
                return Err(format!(
                    "App Suspension {field} must be between 1 and {maximum} seconds."
                ));
            }
        }
        if self.suspendable_apps.iter().any(|rule| {
            rule.network_download_threshold_bytes
                .max(rule.network_upload_threshold_bytes)
                > i64::MAX as u64
        }) {
            return Err(
                "App Suspension network thresholds must fit in a signed 64-bit byte count.".into(),
            );
        }
        Ok(())
    }
}

impl ByCpuLoadRule {
    // TOML stores integers as signed 64-bit values.
    pub const MAX_DURATION_SECONDS: u64 = i64::MAX as u64;

    pub const fn duration_is_valid(seconds: u64) -> bool {
        seconds <= Self::MAX_DURATION_SECONDS
    }

    pub fn validate(&self) -> Result<(), String> {
        if !Self::duration_is_valid(self.duration_seconds) {
            return Err(format!(
                "duration must not exceed {} seconds",
                Self::MAX_DURATION_SECONDS
            ));
        }
        if self.comparison == CpuUsageComparison::Between
            && self.threshold_percent > self.upper_threshold_percent.unwrap_or(CpuLoadPercent::MAX)
        {
            return Err("Between threshold must not exceed the upper threshold".to_owned());
        }
        self.validate_stability()
    }

    pub fn validate_stability(&self) -> Result<(), String> {
        self.validate_stability_for_entry(
            Some(self.threshold_percent),
            Some(self.upper_threshold_percent.unwrap_or(CpuLoadPercent::MAX)),
        )
    }

    // An invalid editor field leaves that entry boundary unknown until corrected.
    pub(crate) fn validate_stability_for_entry(
        &self,
        lower: Option<CpuLoadPercent>,
        upper: Option<CpuLoadPercent>,
    ) -> Result<(), String> {
        let Some(stability) = &self.stability else {
            return Ok(());
        };
        if !Self::duration_is_valid(stability.minimum_hold_seconds) {
            return Err(format!(
                "minimum hold must not exceed {} seconds",
                Self::MAX_DURATION_SECONDS
            ));
        }
        let valid_thresholds = match self.comparison {
            CpuUsageComparison::AtOrAbove => {
                lower.is_none_or(|entry| stability.exit_threshold_percent <= entry)
                    && stability.exit_upper_threshold_percent.is_none()
            }
            CpuUsageComparison::AtOrBelow => {
                lower.is_none_or(|entry| stability.exit_threshold_percent >= entry)
                    && stability.exit_upper_threshold_percent.is_none()
            }
            CpuUsageComparison::Between => {
                lower.is_none_or(|entry| stability.exit_threshold_percent <= entry)
                    && stability
                        .exit_upper_threshold_percent
                        .is_some_and(|release| upper.is_none_or(|entry| release >= entry))
            }
            CpuUsageComparison::Else => false,
        };
        if !valid_thresholds {
            return Err("switching stability release thresholds must include the entry condition; Else rules cannot use stability".to_owned());
        }
        Ok(())
    }
}
