use windows::{
    Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        System::WinRT::{
            RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED, RO_INIT_SINGLETHREADED,
        },
    },
    UI::ViewManagement::{UIColorType, UISettings},
};
use windows_sys::Win32::{
    Graphics::Gdi::{
        GetSysColor, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT,
    },
    UI::{
        Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
        WindowsAndMessaging::{SystemParametersInfoW, SPI_GETHIGHCONTRAST},
    },
};

#[derive(Clone, Copy)]
pub(crate) struct ContrastColors {
    pub window: u32,
    pub window_text: u32,
    pub highlight: u32,
    pub highlight_text: u32,
}

pub(super) fn high_contrast_active() -> Option<bool> {
    let mut contrast = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: the query writes to a correctly sized structure. Its scheme pointer
    // is system-owned on this GET path and is neither read nor freed here.
    let queried = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            (&mut contrast as *mut HIGHCONTRASTW).cast(),
            0,
        )
    };
    (queried != 0).then_some(contrast.dwFlags & HCF_HIGHCONTRASTON != 0)
}

pub(crate) fn contrast_colors() -> Option<ContrastColors> {
    if high_contrast_active() != Some(true) {
        return None;
    }
    fn color(index: windows_sys::Win32::Graphics::Gdi::SYS_COLOR_INDEX) -> u32 {
        // SAFETY: the caller supplies a defined system color index and no pointers.
        let bgr = unsafe { GetSysColor(index) };
        bgr.swap_bytes() >> 8
    }
    Some(ContrastColors {
        window: color(COLOR_WINDOW),
        window_text: color(COLOR_WINDOWTEXT),
        highlight: color(COLOR_HIGHLIGHT),
        highlight_text: color(COLOR_HIGHLIGHTTEXT),
    })
}

pub(crate) struct Appearance {
    pub light: bool,
    pub accent: u32,
    pub animations: bool,
}

pub(crate) fn read() -> windows::core::Result<Appearance> {
    // SAFETY: Initialize this thread only; each successful initialization is balanced below.
    let initialized = unsafe { RoInitialize(RO_INIT_SINGLETHREADED) };
    if initialized
        .as_ref()
        .is_err_and(|error| error.code() == RPC_E_CHANGED_MODE)
    {
        // SAFETY: The thread already uses MTA; request that same apartment model.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }?;
    } else {
        initialized?;
    }
    let result = (|| {
        let settings = UISettings::new()?;
        let foreground = settings.GetColorValue(UIColorType::Foreground)?;
        // Winderust uses the second light accent shade in both app themes.
        let accent = settings.GetColorValue(UIColorType::AccentLight2)?;
        Ok(Appearance {
            animations: settings.AnimationsEnabled()?,
            light: u32::from(foreground.R) + u32::from(foreground.G) + u32::from(foreground.B)
                < 384,
            accent: (u32::from(accent.R) << 16) | (u32::from(accent.G) << 8) | u32::from(accent.B),
        })
    })();
    // SAFETY: All UISettings objects were released above; balance this call's successful initialization.
    unsafe { RoUninitialize() };
    result
}
