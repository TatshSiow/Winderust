use std::{mem::size_of, sync::LazyLock};

use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
use windows_sys::Win32::{
    Foundation::HWND,
    System::{
        LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32},
        SystemInformation::OSVERSIONINFOW,
    },
    UI::{
        Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
        WindowsAndMessaging::{SystemParametersInfoW, SPI_GETHIGHCONTRAST},
    },
};

type AppMode = unsafe extern "system" fn(i32) -> i32;
type WindowMode = unsafe extern "system" fn(HWND, bool) -> bool;
type Refresh = unsafe extern "system" fn();

// These private UxTheme exports are also used by Microsoft PowerToys. Ordinal 135 had
// a different signature before Windows 10 1903; never call it on an older build.
static FUNCTIONS: LazyLock<Option<(AppMode, WindowMode, Refresh, Refresh)>> = LazyLock::new(|| {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: version is a writable SDK structure with its size initialized.
    if unsafe { RtlGetVersion(&mut version) } < 0
        || version.dwMajorVersion < 10
        || version.dwBuildNumber < 18362
    {
        return None;
    }
    let name = crate::win_util::wide_null("uxtheme.dll");
    // SAFETY: name is terminated UTF-16 and DLL lookup is restricted to System32.
    // Keep this single module reference for the process lifetime so cached pointers stay valid.
    let module = unsafe {
        LoadLibraryExW(
            name.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if module.is_null() {
        return None;
    }
    // SAFETY: module remains loaded. Integer resource pointers select known ordinals;
    // all four exports must exist before any are called.
    let (app, window, refresh, flush) = unsafe {
        (
            GetProcAddress(module, 135usize as *const u8)?,
            GetProcAddress(module, 133usize as *const u8)?,
            GetProcAddress(module, 104usize as *const u8)?,
            GetProcAddress(module, 136usize as *const u8)?,
        )
    };
    // SAFETY: signatures match the private UxTheme contract on the gated Windows builds.
    Some(unsafe {
        (
            std::mem::transmute::<unsafe extern "system" fn() -> isize, AppMode>(app),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, WindowMode>(window),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, Refresh>(refresh),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, Refresh>(flush),
        )
    })
});

pub(crate) fn apply(hwnd: HWND) {
    let Some((app, window, refresh, flush)) = *FUNCTIONS else {
        return;
    };
    if hwnd.is_null() {
        return;
    }
    let mut contrast = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: contrast is writable, correctly sized, and not retained by Windows.
    let queried = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            (&mut contrast as *mut HIGHCONTRASTW).cast(),
            0,
        )
    };
    // If accessibility settings cannot be read, preserve the standard system rendering.
    let allow_dark = queried != 0 && contrast.dwFlags & HCF_HIGHCONTRASTON == 0;
    // SAFETY: these cached exports passed the version/availability checks above, hwnd is
    // the live tray owner, and all calls run on its UI thread before menu creation.
    unsafe {
        refresh();
        app(i32::from(allow_dark)); // Default = 0; AllowDark = 1 follows the Windows app theme.
        window(hwnd, allow_dark);
        flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_menu_theme_follows_system_without_forcing_dark_or_light() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow};
        let Some((app, _, _, _)) = *FUNCTIONS else {
            return;
        };
        // SAFETY: the resolved export passed the version check; save the process-local mode
        // and restore it below. No Windows user setting is changed.
        let original = unsafe { app(0) };
        apply(std::ptr::null_mut());
        // SAFETY: same validated export; a null owner must not enable dark menus.
        let unchanged = unsafe { app(0) };
        let class = crate::win_util::wide_null("STATIC");
        // SAFETY: STATIC is a built-in class and the terminated name lives through the call.
        // This test owns the hidden window on the current thread and destroys it below.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                10,
                10,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        apply(hwnd);
        let mut contrast = HIGHCONTRASTW {
            cbSize: size_of::<HIGHCONTRASTW>() as u32,
            ..Default::default()
        };
        // SAFETY: writable, correctly sized accessibility query with no retained pointers.
        let queried = unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                contrast.cbSize,
                (&mut contrast as *mut HIGHCONTRASTW).cast(),
                0,
            )
        };
        // SAFETY: restore the original process-local mode and release our owned window.
        let selected = unsafe {
            let selected = app(original);
            if !hwnd.is_null() {
                DestroyWindow(hwnd);
            }
            selected
        };
        assert!(!hwnd.is_null());
        assert_eq!(unchanged, 0);
        assert_eq!(
            selected,
            i32::from(queried != 0 && contrast.dwFlags & HCF_HIGHCONTRASTON == 0)
        );
    }
}
