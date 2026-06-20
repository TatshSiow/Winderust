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
    let x = i32::from(x);
    let y = i32::from(y);
    let deadzone = i32::from(deadzone);
    x.saturating_mul(x) + y.saturating_mul(y) > deadzone.saturating_mul(deadzone)
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
}
