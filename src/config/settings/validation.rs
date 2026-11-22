use super::{
    AppSuspensionSettings, ByCpuLoadRule, CpuLoadPercent, CpuUsageComparison, Settings,
    SleepPreventionSettings, NESTED_BATTERY_PROFILE_ERROR,
};
use crate::config::{is_valid_executable_name, ExecutableSelector};

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        self.validate_power_source_profiles()?;
        self.validate_watchdog()?;
        self.validate_keep_running()?;
        self.validate_instance_limits()?;
        self.validate_instance_balancing()?;
        self.validate_dynamic_resource_zones()?;
        self.validate_by_cpu_load_rules()?;
        for profile in self.profiles() {
            profile.app_suspension.validate()?;
            profile.sleep_prevention.validate()?;
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

    pub fn validate_watchdog(&self) -> Result<(), String> {
        for profile in self.profiles() {
            profile.watchdog.validate()?;
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
