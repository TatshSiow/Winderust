use std::{collections::BTreeSet, path::Path};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HWND, LPARAM, MAX_PATH, POINT},
    System::{
        ProcessStatus::K32GetModuleFileNameExW,
        Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
    },
    UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON},
    UI::WindowsAndMessaging::{
        EnumWindows, GetAncestor, GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindow,
        GetWindowThreadProcessId, WindowFromPoint, GA_ROOT, GW_OWNER,
    },
};

#[derive(Debug, Default)]
pub struct ForegroundDetector;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundProcess {
    pub id: u32,
    pub name: String,
}

impl ForegroundDetector {
    pub fn process(&self) -> Option<ForegroundProcess> {
        unsafe { foreground_process() }
    }

    pub fn cursor_process(&self) -> Option<ForegroundProcess> {
        unsafe { cursor_process() }
    }

    pub fn cursor_process_id(&self) -> Option<u32> {
        unsafe { cursor_process_id() }
    }

    pub fn shell_window_mouse_pressed(&self) -> bool {
        unsafe { mouse_button_pressed() && cursor_is_shell_window() }
    }

    pub fn cursor_is_shell_window(&self) -> bool {
        unsafe { cursor_is_shell_window() }
    }

    pub fn process_id(&self) -> Option<u32> {
        unsafe { foreground_process_id() }
    }

    pub fn process_name(&self) -> Option<String> {
        self.process().map(|process| process.name)
    }
}

pub fn top_level_window_process_ids() -> BTreeSet<u32> {
    unsafe {
        let mut process_ids = BTreeSet::new();
        EnumWindows(
            Some(collect_top_level_window_process),
            &mut process_ids as *mut BTreeSet<u32> as LPARAM,
        );
        process_ids
    }
}

unsafe fn foreground_process() -> Option<ForegroundProcess> {
    let process_id = foreground_process_id()?;
    process_from_id(process_id)
}

unsafe fn cursor_process() -> Option<ForegroundProcess> {
    let process_id = cursor_process_id()?;
    process_from_id(process_id)
}

unsafe fn cursor_process_id() -> Option<u32> {
    process_id_from_window(cursor_root_window()?)
}

unsafe fn process_from_id(process_id: u32) -> Option<ForegroundProcess> {
    let process = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, process_id);
    if process.is_null() {
        return None;
    }

    let mut buffer = [0u16; MAX_PATH as usize];
    let len = K32GetModuleFileNameExW(
        process,
        std::ptr::null_mut(),
        buffer.as_mut_ptr(),
        buffer.len() as u32,
    );
    CloseHandle(process);

    if len == 0 {
        return None;
    }

    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    let name = Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())?;

    Some(ForegroundProcess {
        id: process_id,
        name,
    })
}

unsafe fn foreground_process_id() -> Option<u32> {
    let window = GetForegroundWindow();
    if window.is_null() {
        return None;
    }

    process_id_from_window(window)
}

unsafe fn process_id_from_window(window: windows_sys::Win32::Foundation::HWND) -> Option<u32> {
    let mut process_id = 0;
    GetWindowThreadProcessId(window, &mut process_id);
    (process_id != 0).then_some(process_id)
}

unsafe fn cursor_is_shell_window() -> bool {
    let Some(window) = cursor_root_window() else {
        return false;
    };

    let class_name = window_class_name(window);
    is_shell_window_class(&class_name)
}

unsafe fn cursor_root_window() -> Option<windows_sys::Win32::Foundation::HWND> {
    let mut point = POINT::default();
    if GetCursorPos(&mut point) == 0 {
        return None;
    }

    let window = WindowFromPoint(point);
    if window.is_null() {
        return None;
    }

    let root_window = GetAncestor(window, GA_ROOT);
    Some(if root_window.is_null() {
        window
    } else {
        root_window
    })
}

unsafe fn window_class_name(window: HWND) -> String {
    let mut buffer = [0u16; 128];
    let len = GetClassNameW(window, buffer.as_mut_ptr(), buffer.len() as i32);
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
    if !GetWindow(hwnd, GW_OWNER).is_null() {
        return 1;
    }

    let mut process_id = 0;
    GetWindowThreadProcessId(hwnd, &mut process_id);
    if process_id != 0 {
        let process_ids = &mut *(lparam as *mut BTreeSet<u32>);
        process_ids.insert(process_id);
    }

    1
}

unsafe fn mouse_button_pressed() -> bool {
    [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
        .into_iter()
        .any(|button| GetAsyncKeyState(i32::from(button)) < 0)
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
