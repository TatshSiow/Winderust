use std::time::{Duration, Instant};

use windows_sys::Win32::{
    Foundation::ERROR_SUCCESS,
    UI::Input::XboxController::{
        XInputGetState, XINPUT_GAMEPAD, XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE,
        XINPUT_GAMEPAD_RIGHT_THUMB_DEADZONE, XINPUT_GAMEPAD_TRIGGER_THRESHOLD, XINPUT_STATE,
        XUSER_MAX_COUNT,
    },
};

pub const CONTROLLER_ACTIVITY_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Default)]
pub struct ControllerActivityDetector {
    last_activity: Option<Instant>,
}

impl ControllerActivityDetector {
    pub fn poll(&mut self, now: Instant) -> bool {
        let active = any_xinput_controller_active();
        if active {
            self.last_activity = Some(now);
        }
        active
    }

    pub fn idle_for(&self, now: Instant) -> Option<Duration> {
        self.last_activity
            .map(|last_activity| now.saturating_duration_since(last_activity))
    }

    pub fn clear(&mut self) {
        self.last_activity = None;
    }
}

fn any_xinput_controller_active() -> bool {
    (0..XUSER_MAX_COUNT).any(|index| {
        let mut state = XINPUT_STATE::default();
        // SAFETY: state is writable for the duration of the call and index is within
        // XInput's documented user range.
        let result = unsafe { XInputGetState(index, &mut state) };
        result == ERROR_SUCCESS && gamepad_has_activity(&state.Gamepad)
    })
}

fn gamepad_has_activity(gamepad: &XINPUT_GAMEPAD) -> bool {
    gamepad.wButtons != 0
        || trigger_active(gamepad.bLeftTrigger)
        || trigger_active(gamepad.bRightTrigger)
        || thumbstick_active(
            gamepad.sThumbLX,
            gamepad.sThumbLY,
            XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE,
        )
        || thumbstick_active(
            gamepad.sThumbRX,
            gamepad.sThumbRY,
            XINPUT_GAMEPAD_RIGHT_THUMB_DEADZONE,
        )
}

fn trigger_active(value: u8) -> bool {
    value > XINPUT_GAMEPAD_TRIGGER_THRESHOLD as u8
}

fn thumbstick_active(x: i16, y: i16, deadzone: u16) -> bool {
    let x = i64::from(x);
    let y = i64::from(y);
    let deadzone = i64::from(deadzone);
    x * x + y * y > deadzone * deadzone
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::Input::XboxController::XINPUT_GAMEPAD_A;

    #[test]
    fn default_gamepad_state_is_inactive() {
        assert!(!gamepad_has_activity(&XINPUT_GAMEPAD::default()));
    }

    #[test]
    fn gamepad_buttons_count_as_activity() {
        let gamepad = XINPUT_GAMEPAD {
            wButtons: XINPUT_GAMEPAD_A,
            ..Default::default()
        };

        assert!(gamepad_has_activity(&gamepad));
    }

    #[test]
    fn trigger_must_cross_threshold() {
        assert!(!trigger_active(XINPUT_GAMEPAD_TRIGGER_THRESHOLD as u8));
        assert!(trigger_active(
            XINPUT_GAMEPAD_TRIGGER_THRESHOLD.saturating_add(1) as u8
        ));
    }

    #[test]
    fn thumbstick_uses_radial_deadzone() {
        assert!(!thumbstick_active(
            XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE as i16,
            0,
            XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE,
        ));
        assert!(thumbstick_active(
            XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE.saturating_add(1) as i16,
            0,
            XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE,
        ));
    }
    #[test]
    fn thumbstick_handles_full_axis_range_and_deadzone_boundaries() {
        for deadzone in [
            XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE,
            XINPUT_GAMEPAD_RIGHT_THUMB_DEADZONE,
        ] {
            for x in [i16::MIN, i16::MAX] {
                for y in [i16::MIN, i16::MAX] {
                    assert!(thumbstick_active(x, y, deadzone));
                    assert!(gamepad_has_activity(&XINPUT_GAMEPAD {
                        sThumbLX: x,
                        sThumbLY: y,
                        ..Default::default()
                    }));
                    assert!(gamepad_has_activity(&XINPUT_GAMEPAD {
                        sThumbRX: x,
                        sThumbRY: y,
                        ..Default::default()
                    }));
                }
            }
            assert!(!thumbstick_active(0, 0, deadzone));
            for sign in [-1, 1] {
                for offset in [-1, 0, 1] {
                    let axis = sign * (deadzone as i16 + offset);
                    assert_eq!(thumbstick_active(axis, 0, deadzone), offset > 0);
                    assert_eq!(thumbstick_active(0, axis, deadzone), offset > 0);
                }
            }
        }
    }
}
