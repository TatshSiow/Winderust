use std::time::Duration;

use windows_sys::Win32::{
    System::SystemInformation::GetTickCount,
    UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
};

pub(crate) fn idle_for() -> Option<Duration> {
    let mut input = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: input is writable for the declared size and lives through the call.
    if unsafe { GetLastInputInfo(&mut input) } == 0 {
        return None;
    }
    // SAFETY: this read-only tick query takes no parameters.
    let now = unsafe { GetTickCount() };
    elapsed_since_input(now, input.dwTime)
}

fn elapsed_since_input(now: u32, last_input: u32) -> Option<Duration> {
    let elapsed = now.wrapping_sub(last_input);
    // Past and future ticks cannot be distinguished beyond half the counter's range.
    (elapsed <= i32::MAX as u32).then(|| Duration::from_millis(u64::from(elapsed)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_idle_handles_tick_wraparound_and_rejects_future_ticks() {
        assert_eq!(elapsed_since_input(10, 10), Some(Duration::ZERO));
        assert_eq!(elapsed_since_input(60, 10), Some(Duration::from_millis(50)));
        assert_eq!(
            elapsed_since_input(30, u32::MAX - 19),
            Some(Duration::from_millis(50))
        );
        assert_eq!(elapsed_since_input(10, 11), None);
    }
}
