//! Use Wind UI's native transitions; page/layout changes have no custom animation layer.

use crate::config::AnimationMode;

pub(super) fn set_enabled(value: bool) {
    windui::anim::set_enabled(value);
}

/// Resolve Winderust's persisted preference against the Windows preference.
pub(super) fn apply(mode: AnimationMode, system_enabled: bool) -> bool {
    let enabled = mode.enabled(system_enabled);
    set_enabled(enabled);
    enabled
}

#[cfg(test)]
mod tests {
    use super::*;
    use windui::anim::MotionLevel;

    #[test]
    fn explicit_motion_choices_override_system_preference() {
        let original = windui::anim::motion_level();
        for system in [false, true] {
            assert!(apply(AnimationMode::On, system));
            assert_eq!(windui::anim::motion_level(), MotionLevel::Full);
            assert!(!apply(AnimationMode::Off, system));
            assert_eq!(windui::anim::motion_level(), MotionLevel::Off);
            assert_eq!(apply(AnimationMode::System, system), system);
            assert_eq!(
                windui::anim::motion_level(),
                if system {
                    MotionLevel::Full
                } else {
                    MotionLevel::Off
                }
            );
        }
        windui::anim::set_motion_level(original);
    }
}
