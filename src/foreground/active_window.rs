use std::collections::BTreeSet;

use super::process_list::process_image_path;

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, POINT},
    UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON},
    UI::WindowsAndMessaging::{
        EnumWindows, GetAncestor, GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindow,
        GetWindowThreadProcessId, WindowFromPoint, GA_ROOT, GW_OWNER,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundProcess {
    pub id: u32,
    pub name: String,
}

pub fn foreground_process_name() -> Option<String> {
    foreground_process().map(|process| process.name)
}

pub fn shell_window_mouse_pressed() -> bool {
    mouse_button_pressed() && cursor_is_shell_window()
}

pub fn top_level_window_process_ids() -> BTreeSet<u32> {
    let mut process_ids = BTreeSet::new();
    // SAFETY: collect_top_level_window_process has the required callback ABI and lparam points to
    // process_ids, which remains live and exclusively borrowed for the synchronous enumeration.
    unsafe {
        EnumWindows(
            Some(collect_top_level_window_process),
            &mut process_ids as *mut BTreeSet<u32> as LPARAM,
        );
    }
    process_ids
}

pub fn foreground_process() -> Option<ForegroundProcess> {
    let process_id = foreground_process_id()?;
    process_from_id(process_id)
}

pub fn cursor_process() -> Option<ForegroundProcess> {
    let process_id = cursor_process_id()?;
    process_from_id(process_id)
}

pub fn cursor_process_id() -> Option<u32> {
    process_id_from_window(cursor_root_window()?)
}

fn process_from_id(process_id: u32) -> Option<ForegroundProcess> {
    let name = process_image_path(process_id)?
        .file_name()?
        .to_string_lossy()
        .to_ascii_lowercase();

    Some(ForegroundProcess {
        id: process_id,
        name,
    })
}

pub fn foreground_process_id() -> Option<u32> {
    // SAFETY: GetForegroundWindow takes no arguments and returns a borrowed HWND.
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        return None;
    }

    process_id_from_window(window)
}

fn process_id_from_window(window: windows_sys::Win32::Foundation::HWND) -> Option<u32> {
    let mut process_id = 0;
    // SAFETY: window is a borrowed HWND returned by Windows and process_id is writable.
    unsafe { GetWindowThreadProcessId(window, &mut process_id) };
    (process_id != 0).then_some(process_id)
}

pub fn cursor_is_shell_window() -> bool {
    let Some(window) = cursor_root_window() else {
        return false;
    };

    let class_name = window_class_name(window);
    is_shell_window_class(&class_name)
}

fn cursor_root_window() -> Option<windows_sys::Win32::Foundation::HWND> {
    let mut point = POINT::default();
    // SAFETY: point is writable for the duration of the call.
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return None;
    }

    // SAFETY: point was initialized by GetCursorPos.
    let window = unsafe { WindowFromPoint(point) };
    if window.is_null() {
        return None;
    }

    // SAFETY: window is a borrowed live HWND and GA_ROOT requests a borrowed ancestor.
    let root_window = unsafe { GetAncestor(window, GA_ROOT) };
    Some(if root_window.is_null() {
        window
    } else {
        root_window
    })
}

fn window_class_name(window: HWND) -> String {
    let mut buffer = [0u16; 128];
    // SAFETY: window is borrowed from Windows and buffer supplies its full writable length.
    let len = unsafe { GetClassNameW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
    if len <= 0 {
        String::new()
    } else {
        String::from_utf16_lossy(&buffer[..len as usize])
    }
}

fn is_shell_window_class(class_name: &str) -> bool {
    matches!(
        class_name,
        "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "Shell_TrayWndClass"
            | "NotifyIconOverflowWindow"
            | "TaskListThumbnailWnd"
            | "TaskListOverlayWnd"
            | "TaskSwitcherWnd"
            | "MultitaskingViewFrame"
            | "XamlExplorerHostIslandWindow"
    )
}

unsafe extern "system" fn collect_top_level_window_process(hwnd: HWND, lparam: LPARAM) -> i32 {
    // SAFETY: hwnd is supplied by EnumWindows and GW_OWNER returns a borrowed HWND.
    if !unsafe { GetWindow(hwnd, GW_OWNER) }.is_null() {
        return 1;
    }

    let mut process_id = 0;
    // SAFETY: hwnd is supplied by EnumWindows and process_id is writable.
    unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
    if process_id != 0 {
        // SAFETY: lparam points to the exclusively borrowed BTreeSet passed by
        // top_level_window_process_ids for this synchronous callback.
        let process_ids = unsafe { &mut *(lparam as *mut BTreeSet<u32>) };
        process_ids.insert(process_id);
    }

    1
}

fn mouse_button_pressed() -> bool {
    [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
        .into_iter()
        .any(|button| {
            // SAFETY: button is a documented Windows virtual-key code.
            unsafe { GetAsyncKeyState(i32::from(button)) < 0 }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_window_classes_include_taskbar_and_switcher_surfaces() {
        for class_name in [
            "Shell_TrayWnd",
            "Shell_SecondaryTrayWnd",
            "NotifyIconOverflowWindow",
            "TaskListThumbnailWnd",
            "TaskListOverlayWnd",
            "TaskSwitcherWnd",
            "MultitaskingViewFrame",
            "XamlExplorerHostIslandWindow",
        ] {
            assert!(is_shell_window_class(class_name), "{class_name}");
        }
    }

    #[test]
    fn shell_window_classes_exclude_regular_app_windows() {
        assert!(!is_shell_window_class("Chrome_WidgetWin_1"));
        assert!(!is_shell_window_class("CabinetWClass"));
    }
}
