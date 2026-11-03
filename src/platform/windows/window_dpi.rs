//! Keep restored windows reachable and preserve Windows' suggested DPI geometry.
use std::cell::Cell;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        MONITOR_DEFAULTTONULL,
    },
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GetWindowRect, IsIconic, IsZoomed, SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOSIZE, SWP_NOZORDER, WINDOWPOS, WM_DPICHANGED, WM_NCDESTROY, WM_WINDOWPOSCHANGING,
        },
    },
};

thread_local! {
    static DPI_RECT: Cell<Option<(HWND, RECT)>> = const { Cell::new(None) };
}

pub(crate) fn is_restored(hwnd: HWND) -> bool {
    // SAFETY: the UI supplies its live window; these queries retain no handles or pointers.
    !hwnd.is_null() && unsafe { IsZoomed(hwnd) == 0 && IsIconic(hwnd) == 0 }
}

pub(crate) fn is_maximized(hwnd: HWND) -> bool {
    // SAFETY: the UI supplies its live window; this query retains no handles or pointers.
    !hwnd.is_null() && unsafe { IsZoomed(hwnd) != 0 }
}

pub(crate) fn ensure_visible(hwnd: HWND) -> Result<(), String> {
    let mut rect = RECT::default();
    // SAFETY: the UI supplies its newly created window on its owner/DPI-aware thread;
    // rect is writable and no pointers are retained.
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return Err(format!(
            "Failed to read the restored window position: {}.",
            crate::win_util::last_error()
        ));
    }
    // SAFETY: rect contains screen coordinates from GetWindowRect and remains live for the call.
    if !unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONULL) }.is_null() {
        return Ok(());
    }
    // SAFETY: rect is initialized; the returned monitor handle is borrowed, not owned.
    let monitor = unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_null() {
        return Err("No connected monitor is available for the restored window.".into());
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: monitor came from MonitorFromRect and info has the required size and writable storage.
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return Err(format!(
            "Failed to read the monitor work area: {}.",
            crate::win_util::last_error()
        ));
    }
    // SAFETY: hwnd is owned by this UI thread. Move only, preserving size, stacking, and activation.
    if unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            info.rcWork.left,
            info.rcWork.top,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    } == 0
    {
        return Err(format!(
            "Failed to move the restored window onto a connected monitor: {}.",
            crate::win_util::last_error()
        ));
    }
    Ok(())
}

pub(crate) fn install(hwnd: HWND) -> Result<(), String> {
    // SAFETY: called on the UI thread with its live native window; the callback is static
    // and retains no application pointers. The procedure/ID pair makes installation idempotent.
    if hwnd.is_null() || unsafe { SetWindowSubclass(hwnd, Some(window_proc), 0, 0) } == 0 {
        return Err("Failed to install mixed-DPI window handling.".into());
    }
    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    _data: usize,
) -> LRESULT {
    if message == WM_DPICHANGED && lparam != 0 {
        // SAFETY: Windows supplies a readable RECT for this synchronous DPI message.
        let rect = unsafe { *(lparam as *const RECT) };
        let restored = is_restored(hwnd);
        if restored && rect.right > rect.left && rect.bottom > rect.top {
            let previous = DPI_RECT.replace(Some((hwnd, rect)));
            // SAFETY: forward the original message so Winit updates DPI and emits its events.
            // Nested SetWindowPos messages use the suggested rectangle below, avoiding #4600.
            let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
            DPI_RECT.set(previous);
            return result;
        }
    } else if message == WM_WINDOWPOSCHANGING && lparam != 0 {
        if let Some((owner, rect)) = DPI_RECT.get().filter(|(owner, _)| *owner == hwnd) {
            // SAFETY: Windows supplies a writable WINDOWPOS for this synchronous message.
            let position = unsafe { &mut *(lparam as *mut WINDOWPOS) };
            if position.hwnd == owner && position.flags & (SWP_NOMOVE | SWP_NOSIZE) == 0 {
                position.x = rect.left;
                position.y = rect.top;
                position.cx = rect.right.saturating_sub(rect.left);
                position.cy = rect.bottom.saturating_sub(rect.top);
            }
        }
    } else if message == WM_NCDESTROY {
        // SAFETY: this callback belongs to hwnd and is removed before its window is destroyed.
        unsafe { RemoveWindowSubclass(hwnd, Some(window_proc), id) };
    }
    // SAFETY: preserve the remaining subclass chain and Winit's normal message handling.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetWindowRect, SendMessageW, SetWindowPos, MINMAXINFO,
        SWP_NOACTIVATE, SWP_NOZORDER, WM_GETMINMAXINFO, WS_OVERLAPPEDWINDOW,
    };

    #[test]
    fn offscreen_windows_recover_without_resizing_or_moving_visible_windows() {
        let class = crate::win_util::wide_null("STATIC");
        // SAFETY: create an owned hidden window on this thread using a built-in class.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_OVERLAPPEDWINDOW,
                100,
                100,
                800,
                600,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null());
        let mut checks = Vec::new();
        for coordinate in [-100_000, 100_000] {
            let mut before = RECT::default();
            // SAFETY: move our owned hidden window far outside any ordinary desktop, then query it.
            unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    coordinate,
                    coordinate,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                GetWindowRect(hwnd, &mut before);
            }
            let recovered = ensure_visible(hwnd);
            let mut after = RECT::default();
            // SAFETY: hwnd remains live and after is writable for the call.
            let read = unsafe { GetWindowRect(hwnd, &mut after) } != 0;
            // SAFETY: after is initialized and borrowed only for this synchronous query.
            let visible = !unsafe { MonitorFromRect(&after, MONITOR_DEFAULTTONULL) }.is_null();
            let unchanged = ensure_visible(hwnd);
            let mut again = RECT::default();
            // SAFETY: hwnd remains live and again is writable for the call.
            let reread = unsafe { GetWindowRect(hwnd, &mut again) } != 0;
            checks.push((
                recovered.is_ok() && unchanged.is_ok() && read && reread && visible,
                after.right - after.left == before.right - before.left
                    && after.bottom - after.top == before.bottom - before.top,
                (after.left, after.top, after.right, after.bottom)
                    == (again.left, again.top, again.right, again.bottom),
            ));
        }
        // SAFETY: release our hidden window before checking the recorded results.
        unsafe { DestroyWindow(hwnd) };
        assert!(checks
            .iter()
            .all(|&(recovered, sized, stable)| recovered && sized && stable));
        assert!(ensure_visible(std::ptr::null_mut()).is_err());
    }

    unsafe extern "system" fn growing_window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        data: usize,
    ) -> LRESULT {
        if message == WM_GETMINMAXINFO {
            // Keep synthetic DPI rectangles independent of the CI runner's desktop size.
            // SAFETY: Windows supplies a writable MINMAXINFO for this synchronous message.
            let limits = unsafe { &mut *(lparam as *mut MINMAXINFO) };
            limits.ptMaxTrackSize.x = 3000;
            limits.ptMaxTrackSize.y = 3000;
            return 0;
        }
        if message == WM_DPICHANGED {
            // SAFETY: the test retains its counter until this callback is removed.
            unsafe { *(data as *mut usize) += 1 };
            // SAFETY: simulate Winit's oversized repositioning on our owned hidden window.
            unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    100,
                    100,
                    2500,
                    2000,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
            return 0;
        }
        // SAFETY: forward ordinary test-window messages to the remaining subclass chain.
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }

    #[test]
    fn dpi_round_trips_preserve_suggested_rect_and_forward_events() {
        let class = crate::win_util::wide_null("STATIC");
        // SAFETY: create an owned hidden window on this thread using a built-in class.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_OVERLAPPEDWINDOW,
                100,
                100,
                960,
                720,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null());
        let mut forwarded = 0usize;
        // SAFETY: the hidden window and counter remain live until we remove this callback.
        let chained = unsafe {
            SetWindowSubclass(
                hwnd,
                Some(growing_window_proc),
                0,
                &mut forwarded as *mut usize as usize,
            )
        };
        let installed = install(hwnd);
        let mut sizes = Vec::new();
        for dpi in [120, 96].into_iter().cycle().take(20) {
            let rect = RECT {
                left: 100,
                top: 100,
                right: 100 + 10 * dpi,
                bottom: 100 + 7 * dpi,
            };
            let mut actual = RECT::default();
            // SAFETY: synchronous messages borrow this valid RECT; the output is writable.
            unsafe {
                SendMessageW(
                    hwnd,
                    WM_DPICHANGED,
                    dpi as usize | ((dpi as usize) << 16),
                    &rect as *const RECT as isize,
                );
                GetWindowRect(hwnd, &mut actual);
            }
            sizes.push((actual.right - actual.left, actual.bottom - actual.top));
        }
        let mut ordinary = RECT::default();
        // SAFETY: resize our live test window, query its rectangle, remove the callback
        // retaining the counter, and destroy the window before examining the results.
        unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                100,
                100,
                1000,
                700,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            GetWindowRect(hwnd, &mut ordinary);
            RemoveWindowSubclass(hwnd, Some(growing_window_proc), 0);
            DestroyWindow(hwnd);
        }
        assert_ne!(chained, 0);
        assert!(installed.is_ok());
        assert_eq!(forwarded, 20);
        for (index, size) in sizes.into_iter().enumerate() {
            assert_eq!(
                size,
                if index % 2 == 0 {
                    (1200, 840)
                } else {
                    (960, 672)
                }
            );
        }
        assert_eq!(
            (
                ordinary.right - ordinary.left,
                ordinary.bottom - ordinary.top
            ),
            (1000, 700)
        );
        assert!(DPI_RECT.get().is_none());
    }
}
