use std::time::Duration;

use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

pub fn last_input_elapsed() -> Option<Duration> {
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };

    // SAFETY: info has the required size in cbSize and remains writable for the call.
    let ok = unsafe { GetLastInputInfo(&mut info) };
    if ok == 0 {
        return None;
    }

    // SAFETY: GetTickCount takes no arguments and has no caller requirements.
    let tick = unsafe { GetTickCount() };
    let elapsed_ms = tick.wrapping_sub(info.dwTime);
    Some(Duration::from_millis(u64::from(elapsed_ms)))
}
