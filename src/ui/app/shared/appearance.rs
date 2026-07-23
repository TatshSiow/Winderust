use crate::ui::app::*;

pub(in crate::ui::app) fn apply_appearance_settings(
    general: &config::GeneralSettings,
    window: &mut Window,
    cx: &mut App,
) {
    match general.theme_mode {
        AppThemeMode::System => gpui_component::Theme::sync_system_appearance(Some(window), cx),
        AppThemeMode::Light => {
            gpui_component::Theme::change(gpui_component::ThemeMode::Light, Some(window), cx)
        }
        AppThemeMode::Dark => {
            gpui_component::Theme::change(gpui_component::ThemeMode::Dark, Some(window), cx)
        }
    }
    apply_accent_color(&general.accent, cx);
    UI_ANIMATIONS_ENABLED.store(
        resolve_animation_enabled(general.animation_mode),
        Ordering::Relaxed,
    );
    window.refresh();
}

pub(in crate::ui::app) fn apply_accent_color(settings: &AccentSettings, cx: &mut App) {
    let accent_color = resolve_accent_color(settings);
    UI_ACCENT_COLOR.store(accent_color, Ordering::Relaxed);
    UI_ACCENT_TINT_SURFACES.store(
        settings.source == AccentColorSource::Custom,
        Ordering::Relaxed,
    );
    let accent: gpui::Hsla = rgb(accent_color).into();

    let theme = gpui_component::Theme::global_mut(cx);
    let is_dark = theme.is_dark();
    UI_DARK_MODE.store(is_dark, Ordering::Relaxed);
    let foreground = if !is_dark || accent_contrast_prefers_light(accent_color) {
        rgb(0xffffff).into()
    } else {
        rgb(0x111111).into()
    };

    let hover = if is_dark {
        accent.lighten(0.10)
    } else {
        accent.darken(0.10)
    };
    let active = if is_dark {
        accent.darken(0.12)
    } else {
        accent.darken(0.18)
    };

    if is_dark {
        theme.background = rgb(accent_surface_color(COLOR_APP_BG, 0.04)).into();
        theme.foreground = rgb(COLOR_TEXT).into();
        theme.muted_foreground = rgb(COLOR_MUTED).into();
        theme.title_bar = rgb(accent_surface_color(COLOR_TITLE_BAR, 0.05)).into();
        theme.title_bar_border = rgb(COLOR_BORDER).into();
        theme.sidebar = rgb(accent_surface_color(COLOR_TITLE_BAR, 0.06)).into();
        theme.sidebar_foreground = rgb(COLOR_TEXT).into();
        theme.sidebar_border = rgb(COLOR_BORDER).into();
        theme.group_box = rgb(settings_card_color()).into();
        theme.border = rgb(COLOR_BORDER).into();
        theme.popover = rgb(settings_card_color()).into();
        theme.popover_foreground = rgb(COLOR_TEXT).into();
        theme.success_foreground = rgb(COLOR_SUCCESS).into();
        theme.danger_foreground = rgb(0xff8a73).into();
    } else {
        theme.background = rgb(accent_surface_color(COLOR_LIGHT_APP_BG, 0.04)).into();
        theme.foreground = rgb(COLOR_LIGHT_TEXT).into();
        theme.muted_foreground = rgb(COLOR_LIGHT_MUTED).into();
        theme.title_bar = rgb(accent_surface_color(COLOR_LIGHT_TITLE_BAR, 0.05)).into();
        theme.title_bar_border = rgb(COLOR_LIGHT_BORDER).into();
        theme.sidebar = rgb(accent_surface_color(COLOR_LIGHT_TITLE_BAR, 0.06)).into();
        theme.sidebar_foreground = rgb(COLOR_LIGHT_TEXT).into();
        theme.sidebar_border = rgb(COLOR_LIGHT_BORDER).into();
        theme.group_box = rgb(settings_card_color()).into();
        theme.border = rgb(COLOR_LIGHT_BORDER).into();
        theme.popover = rgb(settings_card_color()).into();
        theme.popover_foreground = rgb(COLOR_LIGHT_TEXT).into();
        theme.success_foreground = rgb(0x366b22).into();
        theme.danger_foreground = rgb(0x9b2f1f).into();
    }
    theme.primary = accent;
    theme.primary_hover = hover;
    theme.primary_active = active;
    theme.primary_foreground = foreground;
    if is_dark {
        theme.secondary = rgb(panel_active_color()).into();
        theme.secondary_hover = rgb(settings_card_hover_color()).into();
        theme.secondary_active = rgb(accent_surface_color(COLOR_PANEL_ACTIVE, 0.28)).into();
        theme.secondary_foreground = rgb(COLOR_TEXT).into();
    } else {
        theme.secondary = rgb(panel_active_color()).into();
        theme.secondary_hover = rgb(settings_card_hover_color()).into();
        theme.secondary_active = rgb(accent_surface_color(COLOR_LIGHT_PANEL_ACTIVE, 0.22)).into();
        theme.secondary_foreground = rgb(COLOR_LIGHT_TEXT).into();
    }
    theme.accent = accent;
    theme.accent_foreground = foreground;
    theme.sidebar_accent = accent;
    theme.sidebar_accent_foreground = foreground;
    theme.ring = accent;
    theme.progress_bar = accent;
    theme.slider_thumb = accent;
    theme.caret = accent;
    theme.selection = accent.opacity(0.26);
    theme.input = accent.opacity(0.72);
}

pub(in crate::ui::app) fn resolve_accent_color(settings: &AccentSettings) -> u32 {
    match settings.source {
        AccentColorSource::Windows => read_windows_accent_color().unwrap_or(COLOR_ACCENT),
        AccentColorSource::Custom => settings.custom_color,
    }
}

pub(in crate::ui::app) fn read_windows_accent_color() -> Option<u32> {
    read_windows_accent_palette_tint().or_else(|| {
        read_registry_dword_root(
            HKEY_CURRENT_USER,
            DWM_REGISTRY_SUB_KEY,
            DWM_ACCENT_COLOR_VALUE,
        )
        .map(windows_abgr_to_rgb)
    })
}

pub(in crate::ui::app) fn read_windows_accent_palette_tint() -> Option<u32> {
    read_registry_binary_root(
        HKEY_CURRENT_USER,
        EXPLORER_ACCENT_REGISTRY_SUB_KEY,
        EXPLORER_ACCENT_PALETTE_VALUE,
    )
    .and_then(|palette| windows_accent_palette_tint(&palette))
}

pub(in crate::ui::app) fn windows_accent_palette_tint(palette: &[u8]) -> Option<u32> {
    let color = palette.get(4..8)?;
    Some(((color[0] as u32) << 16) | ((color[1] as u32) << 8) | color[2] as u32)
}

pub(in crate::ui::app) fn windows_abgr_to_rgb(color: u32) -> u32 {
    ((color & 0xff) << 16) | (color & 0xff00) | ((color >> 16) & 0xff)
}

pub(in crate::ui::app) fn accent_contrast_prefers_light(color: u32) -> bool {
    let red = ((color >> 16) & 0xff) as f32;
    let green = ((color >> 8) & 0xff) as f32;
    let blue = (color & 0xff) as f32;
    (0.299 * red + 0.587 * green + 0.114 * blue) < 140.0
}

pub(in crate::ui::app) fn accent_color() -> u32 {
    UI_ACCENT_COLOR.load(Ordering::Relaxed)
}

pub(in crate::ui::app) fn accent_tints_surfaces() -> bool {
    UI_ACCENT_TINT_SURFACES.load(Ordering::Relaxed)
}

pub(in crate::ui::app) fn ui_is_dark() -> bool {
    UI_DARK_MODE.load(Ordering::Relaxed)
}

pub(in crate::ui::app) fn resolve_animation_enabled(mode: AnimationMode) -> bool {
    match mode {
        AnimationMode::System => windows_client_area_animation_enabled().unwrap_or(true),
        AnimationMode::On => true,
        AnimationMode::Off => false,
    }
}

pub(in crate::ui::app) fn windows_client_area_animation_enabled() -> Option<bool> {
    let mut enabled = 0;

    // SAFETY: enabled is writable storage of the documented BOOL size for
    // SPI_GETCLIENTAREAANIMATION and no flags are required.
    let result = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut enabled as *mut i32).cast(),
            0,
        )
    };

    (result != 0).then_some(enabled != 0)
}

pub(in crate::ui::app) fn ui_animations_enabled() -> bool {
    UI_ANIMATIONS_ENABLED.load(Ordering::Relaxed)
}

pub(in crate::ui::app) fn settings_card_color() -> u32 {
    if ui_is_dark() {
        accent_surface_color(COLOR_SETTINGS_CARD, 0.06)
    } else {
        accent_surface_color(COLOR_LIGHT_SETTINGS_CARD, 0.05)
    }
}

pub(in crate::ui::app) fn settings_card_hover_color() -> u32 {
    if ui_is_dark() {
        accent_surface_color(COLOR_SETTINGS_CARD_HOVER, 0.1)
    } else {
        accent_surface_color(COLOR_LIGHT_SETTINGS_CARD_HOVER, 0.08)
    }
}

pub(in crate::ui::app) fn windows_slider_thumb_color() -> u32 {
    if ui_is_dark() {
        0xd9d9d9
    } else {
        0xffffff
    }
}

pub(in crate::ui::app) fn disabled_slider_track_color() -> u32 {
    if ui_is_dark() {
        0x4a4a4a
    } else {
        0xd0d0d0
    }
}

pub(in crate::ui::app) fn disabled_slider_thumb_color() -> u32 {
    if ui_is_dark() {
        0x707070
    } else {
        0xf2f2f2
    }
}

pub(in crate::ui::app) fn border_color() -> u32 {
    if ui_is_dark() {
        COLOR_BORDER
    } else {
        COLOR_LIGHT_BORDER
    }
}

pub(in crate::ui::app) fn primary_text_color() -> u32 {
    if ui_is_dark() {
        COLOR_TEXT
    } else {
        COLOR_LIGHT_TEXT
    }
}

pub(in crate::ui::app) fn muted_text_color() -> u32 {
    if ui_is_dark() {
        COLOR_MUTED
    } else {
        COLOR_LIGHT_MUTED
    }
}

pub(in crate::ui::app) fn dim_text_color() -> u32 {
    if ui_is_dark() {
        COLOR_DIM
    } else {
        COLOR_LIGHT_DIM
    }
}

pub(in crate::ui::app) fn sidebar_selected_color() -> u32 {
    if ui_is_dark() {
        accent_surface_color(COLOR_SIDEBAR_SELECTED, 0.18)
    } else {
        accent_surface_color(COLOR_LIGHT_SIDEBAR_SELECTED, 0.16)
    }
}

pub(in crate::ui::app) fn sidebar_hover_color() -> u32 {
    if ui_is_dark() {
        accent_surface_color(COLOR_SIDEBAR_HOVER, 0.1)
    } else {
        accent_surface_color(COLOR_LIGHT_SIDEBAR_HOVER, 0.08)
    }
}

pub(in crate::ui::app) fn panel_active_color() -> u32 {
    if ui_is_dark() {
        accent_surface_color(COLOR_PANEL_ACTIVE, 0.2)
    } else {
        accent_surface_color(COLOR_LIGHT_PANEL_ACTIVE, 0.16)
    }
}

pub(in crate::ui::app) fn success_bg_color() -> u32 {
    if ui_is_dark() {
        COLOR_SUCCESS_BG
    } else {
        0xdfeccb
    }
}

pub(in crate::ui::app) fn success_text_color() -> u32 {
    if ui_is_dark() {
        COLOR_SUCCESS
    } else {
        0x356b22
    }
}

pub(in crate::ui::app) fn warning_bg_color() -> u32 {
    if ui_is_dark() {
        COLOR_WARNING_BG
    } else {
        0xf8e6b8
    }
}

pub(in crate::ui::app) fn warning_text_color() -> u32 {
    if ui_is_dark() {
        COLOR_WARNING
    } else {
        0x87611c
    }
}

pub(in crate::ui::app) fn accent_glyph_color(accent: u32) -> u32 {
    if !ui_is_dark() || accent_contrast_prefers_light(accent) {
        0xffffff
    } else {
        0x111111
    }
}

pub(in crate::ui::app) fn lerp_rgb(from: u32, to: u32, delta: f32) -> u32 {
    let delta = delta.clamp(0.0, 1.0);
    let from_r = ((from >> 16) & 0xff) as f32;
    let from_g = ((from >> 8) & 0xff) as f32;
    let from_b = (from & 0xff) as f32;
    let to_r = ((to >> 16) & 0xff) as f32;
    let to_g = ((to >> 8) & 0xff) as f32;
    let to_b = (to & 0xff) as f32;
    let r = (from_r + (to_r - from_r) * delta).round() as u32;
    let g = (from_g + (to_g - from_g) * delta).round() as u32;
    let b = (from_b + (to_b - from_b) * delta).round() as u32;

    (r << 16) | (g << 8) | b
}

pub(in crate::ui::app) fn accent_surface_color(base: u32, amount: f32) -> u32 {
    accent_surface_color_with_tint(base, amount, accent_color(), accent_tints_surfaces())
}

pub(in crate::ui::app) fn accent_surface_color_with_tint(
    base: u32,
    amount: f32,
    accent: u32,
    tint: bool,
) -> u32 {
    if tint {
        lerp_rgb(base, accent, amount)
    } else {
        base
    }
}

pub(in crate::ui::app) fn switch_accent_color() -> u32 {
    accent_color()
}

pub(in crate::ui::app) fn read_win32_priority_separation() -> Option<u32> {
    read_registry_dword_root(
        HKEY_LOCAL_MACHINE,
        WIN32_PRIORITY_CONTROL_SUB_KEY,
        WIN32_PRIORITY_SEPARATION_VALUE,
    )
}

pub(in crate::ui::app) fn read_win32_priority_separation_with_status() -> (Option<u32>, String) {
    match read_win32_priority_separation() {
        Some(value) => (
            Some(value),
            t!(
                "settings.win32_priority_separation_loaded",
                value = format_win32_priority_separation_with_description(value)
            )
            .to_string(),
        ),
        None => (
            None,
            t!("settings.win32_priority_separation_load_failed").to_string(),
        ),
    }
}

pub(in crate::ui::app) fn write_win32_priority_separation(value: u32) -> Result<(), String> {
    write_registry_dword_root(
        HKEY_LOCAL_MACHINE,
        WIN32_PRIORITY_CONTROL_SUB_KEY,
        WIN32_PRIORITY_SEPARATION_VALUE,
        value,
    )
}

pub(in crate::ui::app) fn read_win32_priority_separation_backup() -> Option<u32> {
    read_registry_dword_root(
        HKEY_CURRENT_USER,
        WINDERUST_REGISTRY_SUB_KEY,
        WIN32_PRIORITY_SEPARATION_BACKUP_VALUE,
    )
}

pub(in crate::ui::app) fn write_win32_priority_separation_backup(value: u32) -> Result<(), String> {
    write_registry_dword_create_root(
        HKEY_CURRENT_USER,
        WINDERUST_REGISTRY_SUB_KEY,
        WIN32_PRIORITY_SEPARATION_BACKUP_VALUE,
        value,
    )
}

pub(in crate::ui::app) fn format_win32_priority_separation(value: u32) -> String {
    format!("0x{value:02X} ({value})")
}

pub(in crate::ui::app) fn format_win32_priority_separation_with_description(value: u32) -> String {
    format!(
        "{} - {}",
        format_win32_priority_separation(value),
        win32_priority_separation_description(value)
    )
}

pub(in crate::ui::app) fn win32_priority_separation_description(value: u32) -> String {
    match value {
        0x14 => t!("settings.win32_priority_separation_desc_long_variable_none").to_string(),
        0x15 => t!("settings.win32_priority_separation_desc_long_variable_medium").to_string(),
        0x16 => t!("settings.win32_priority_separation_desc_long_variable_high").to_string(),
        0x18 => t!("settings.win32_priority_separation_desc_long_fixed_none").to_string(),
        0x19 => t!("settings.win32_priority_separation_desc_long_fixed_medium").to_string(),
        0x1A => t!("settings.win32_priority_separation_desc_long_fixed_high").to_string(),
        0x24 => t!("settings.win32_priority_separation_desc_short_variable_none").to_string(),
        0x25 => t!("settings.win32_priority_separation_desc_short_variable_medium").to_string(),
        0x26 => t!("settings.win32_priority_separation_desc_short_variable_high").to_string(),
        0x28 => t!("settings.win32_priority_separation_desc_short_fixed_none").to_string(),
        0x29 => t!("settings.win32_priority_separation_desc_short_fixed_medium").to_string(),
        0x2A => t!("settings.win32_priority_separation_desc_short_fixed_high").to_string(),
        _ => t!("settings.win32_priority_separation_desc_custom").to_string(),
    }
}

pub(in crate::ui::app) fn normalize_win32_priority_separation_value(value: u32) -> u32 {
    win32_priority_separation_field_bits(value, Win32PrioritySeparationField::QuantumDuration)
        | win32_priority_separation_field_bits(
            value,
            Win32PrioritySeparationField::QuantumBehaviour,
        )
        | win32_priority_separation_field_bits(value, Win32PrioritySeparationField::ForegroundBoost)
}

pub(in crate::ui::app) fn win32_priority_separation_field_bits(
    value: u32,
    field: Win32PrioritySeparationField,
) -> u32 {
    match field {
        Win32PrioritySeparationField::QuantumDuration => match value & 0x30 {
            0x10 | 0x20 => value & 0x30,
            _ => 0x20,
        },
        Win32PrioritySeparationField::QuantumBehaviour => match value & 0x0C {
            0x04 | 0x08 => value & 0x0C,
            _ => 0x04,
        },
        Win32PrioritySeparationField::ForegroundBoost => match value & 0x03 {
            0x00..=0x02 => value & 0x03,
            _ => 0x02,
        },
    }
}

pub(in crate::ui::app) fn win32_priority_separation_field_picker_id(
    field: Win32PrioritySeparationField,
) -> &'static str {
    match field {
        Win32PrioritySeparationField::QuantumDuration => {
            "win32-priority-separation-quantum-duration"
        }
        Win32PrioritySeparationField::QuantumBehaviour => {
            "win32-priority-separation-quantum-behaviour"
        }
        Win32PrioritySeparationField::ForegroundBoost => {
            "win32-priority-separation-foreground-boost"
        }
    }
}

pub(in crate::ui::app) fn win32_priority_separation_field_options(
    field: Win32PrioritySeparationField,
) -> Vec<Win32PrioritySeparationFieldOption> {
    match field {
        Win32PrioritySeparationField::QuantumDuration => vec![
            Win32PrioritySeparationFieldOption { bits: 0x20 },
            Win32PrioritySeparationFieldOption { bits: 0x10 },
        ],
        Win32PrioritySeparationField::QuantumBehaviour => vec![
            Win32PrioritySeparationFieldOption { bits: 0x04 },
            Win32PrioritySeparationFieldOption { bits: 0x08 },
        ],
        Win32PrioritySeparationField::ForegroundBoost => vec![
            Win32PrioritySeparationFieldOption { bits: 0x00 },
            Win32PrioritySeparationFieldOption { bits: 0x01 },
            Win32PrioritySeparationFieldOption { bits: 0x02 },
        ],
    }
}

pub(in crate::ui::app) fn win32_priority_separation_field_option_label(
    field: Win32PrioritySeparationField,
    bits: u32,
) -> String {
    match (field, bits) {
        (Win32PrioritySeparationField::QuantumDuration, 0x20) => {
            t!("settings.win32_priority_separation_quantum_duration_short").to_string()
        }
        (Win32PrioritySeparationField::QuantumDuration, 0x10) => {
            t!("settings.win32_priority_separation_quantum_duration_long").to_string()
        }
        (Win32PrioritySeparationField::QuantumBehaviour, 0x04) => {
            t!("settings.win32_priority_separation_quantum_behaviour_variable").to_string()
        }
        (Win32PrioritySeparationField::QuantumBehaviour, 0x08) => {
            t!("settings.win32_priority_separation_quantum_behaviour_fixed").to_string()
        }
        (Win32PrioritySeparationField::ForegroundBoost, 0x00) => {
            t!("settings.win32_priority_separation_foreground_boost_none").to_string()
        }
        (Win32PrioritySeparationField::ForegroundBoost, 0x01) => {
            t!("settings.win32_priority_separation_foreground_boost_medium").to_string()
        }
        (Win32PrioritySeparationField::ForegroundBoost, 0x02) => {
            t!("settings.win32_priority_separation_foreground_boost_high").to_string()
        }
        _ => t!("settings.win32_priority_separation_unavailable").to_string(),
    }
}

pub(in crate::ui::app) fn win32_priority_separation_field_label(
    field: Win32PrioritySeparationField,
    value: u32,
) -> String {
    let selected_bits = win32_priority_separation_field_bits(value, field);
    win32_priority_separation_field_options(field)
        .into_iter()
        .find(|option| option.bits == selected_bits)
        .map(|option| win32_priority_separation_field_option_label(field, option.bits))
        .unwrap_or_else(|| t!("settings.win32_priority_separation_unavailable").to_string())
}
