use super::{AdaptiveEngineSettings, PowerSourceProfile};
use crate::power::{AdaptivePowerBoostValues, ProcessorBoostMode, ProcessorPowerValues};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdaptiveProcessorPowerPreset {
    pub name: String,
    base_processor_policy: ProcessorPowerValues,
    background_pressure_profile: PresetBoost,
    focus_and_launch_profile: PresetBoost,
}

impl AdaptiveProcessorPowerPreset {
    pub fn capture(
        name: String,
        settings: &AdaptiveEngineSettings,
        source: PowerSourceProfile,
    ) -> Self {
        Self {
            name,
            base_processor_policy: settings.base_processor_policy.normalized(),
            background_pressure_profile: PresetBoost::capture(
                settings.background_pressure_profile,
                source,
            ),
            focus_and_launch_profile: PresetBoost::capture(
                settings.focus_and_launch_profile,
                source,
            ),
        }
    }

    pub fn apply(&self, settings: &mut AdaptiveEngineSettings, source: PowerSourceProfile) {
        settings.base_processor_policy = self.base_processor_policy;
        self.background_pressure_profile
            .apply(&mut settings.background_pressure_profile, source);
        self.focus_and_launch_profile
            .apply(&mut settings.focus_and_launch_profile, source);
    }

    pub fn matches(&self, settings: &AdaptiveEngineSettings, source: PowerSourceProfile) -> bool {
        let mut candidate = settings.clone();
        self.apply(&mut candidate, source);
        candidate == *settings
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Adaptive Processor Power presets require a name.".into());
        }
        if self.base_processor_policy != self.base_processor_policy.normalized()
            || self.background_pressure_profile.policy > 100
            || self.focus_and_launch_profile.policy > 100
        {
            return Err(format!(
                "Adaptive Processor Power preset '{}' has out-of-range processor settings.",
                self.name
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetBoost {
    policy: u32,
    mode: ProcessorBoostMode,
}

impl PresetBoost {
    fn capture(values: AdaptivePowerBoostValues, source: PowerSourceProfile) -> Self {
        let (policy, mode) = match source {
            PowerSourceProfile::PluggedIn => (values.ac_policy, values.ac_mode),
            PowerSourceProfile::OnBattery => (values.battery_policy, values.battery_mode),
        };
        Self {
            policy: policy.min(100),
            mode,
        }
    }

    fn apply(self, values: &mut AdaptivePowerBoostValues, source: PowerSourceProfile) {
        match source {
            PowerSourceProfile::PluggedIn => {
                values.ac_policy = self.policy;
                values.ac_mode = self.mode;
            }
            PowerSourceProfile::OnBattery => {
                values.battery_policy = self.policy;
                values.battery_mode = self.mode;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_preset_applies_to_ac_without_touching_battery_values_or_enable_flags() {
        let mut battery = AdaptiveEngineSettings::default();
        battery.base_processor_policy.performance_max = 31;
        battery.background_pressure_profile.battery_policy = 44;
        battery.background_pressure_profile.battery_mode = ProcessorBoostMode::Enabled;
        battery.focus_and_launch_profile.battery_policy = 55;
        battery.focus_and_launch_profile.battery_mode = ProcessorBoostMode::EfficientEnabled;
        let preset = AdaptiveProcessorPowerPreset::capture(
            "Quiet".into(),
            &battery,
            PowerSourceProfile::OnBattery,
        );
        assert!(preset.matches(&battery, PowerSourceProfile::OnBattery));

        let mut ac = AdaptiveEngineSettings {
            adaptive_processor_power_enabled: true,
            ..AdaptiveEngineSettings::default()
        };
        let original = ac.clone();
        assert!(!preset.matches(&ac, PowerSourceProfile::PluggedIn));
        preset.apply(&mut ac, PowerSourceProfile::PluggedIn);

        assert_eq!(ac.base_processor_policy, battery.base_processor_policy);
        assert_eq!(ac.background_pressure_profile.ac_policy, 44);
        assert_eq!(
            ac.background_pressure_profile.ac_mode,
            ProcessorBoostMode::Enabled
        );
        assert_eq!(ac.focus_and_launch_profile.ac_policy, 55);
        assert_eq!(
            ac.focus_and_launch_profile.ac_mode,
            ProcessorBoostMode::EfficientEnabled
        );
        assert_eq!(
            PresetBoost::capture(
                ac.background_pressure_profile,
                PowerSourceProfile::OnBattery
            ),
            PresetBoost::capture(
                original.background_pressure_profile,
                PowerSourceProfile::OnBattery,
            )
        );
        assert_eq!(
            PresetBoost::capture(ac.focus_and_launch_profile, PowerSourceProfile::OnBattery),
            PresetBoost::capture(
                original.focus_and_launch_profile,
                PowerSourceProfile::OnBattery
            )
        );
        assert!(ac.adaptive_processor_power_enabled);
        assert!(!ac.enabled);
        assert!(preset.matches(&ac, PowerSourceProfile::PluggedIn));
    }
}
