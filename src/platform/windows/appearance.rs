use windows::{
    Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        System::WinRT::{
            RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED, RO_INIT_SINGLETHREADED,
        },
    },
    UI::ViewManagement::{UIColorType, UISettings},
};

pub(crate) struct Appearance {
    pub light: bool,
    pub accent: u32,
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
            light: u32::from(foreground.R) + u32::from(foreground.G) + u32::from(foreground.B)
                < 384,
            accent: (u32::from(accent.R) << 16) | (u32::from(accent.G) << 8) | u32::from(accent.B),
        })
    })();
    // SAFETY: All UISettings objects were released above; balance this call's successful initialization.
    unsafe { RoUninitialize() };
    result
}
