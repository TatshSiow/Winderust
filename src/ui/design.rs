//! Shared visual measurements. Table data geometry stays with its table.
pub(super) mod space {
    pub const TINY: u32 = 2;
    pub const TIGHT: u32 = 4;
    pub const CONTROL: u32 = 6;
    pub const SMALL: u32 = 8;
    pub const COMPACT: u32 = 10;
    pub const MEDIUM: u32 = 12;
    pub const LARGE: u32 = 16;
    pub const WIDE: u32 = 20;
    pub const SECTION: u32 = 24;
}

pub(super) mod typography {
    pub const FONT: iced::Font = iced::Font {
        weight: iced::font::Weight::Semibold,
        ..iced::Font::with_name("Segoe UI")
    };
    pub const BADGE: u32 = 11;
    pub const CAPTION: u32 = 12;
    pub const SECONDARY: u32 = 13;
    pub const BODY: u32 = 14;
    pub const SECTION: u32 = 16;
    pub const SUBTITLE: u32 = 18;
    pub const DIALOG_TITLE: u32 = 24;
    pub const TITLE: u32 = 28;
}

pub(super) const CONTROL_PADDING: [u16; 2] = [5, 12];
pub(super) const BUTTON_HEIGHT: u32 = 32;
pub(super) const SELECT_PADDING: [u16; 2] = [7, 12];
pub(super) const INPUT_PADDING: u16 = 5;
pub(super) const CHECKBOX_SIZE: u32 = 16;
pub(super) const SWITCH_SIZE: u32 = 20;
pub(super) const ICON_SIZE: u32 = 18;
pub(super) const SLIDER_HEIGHT: u32 = 16;
pub(super) const NUMERIC_WIDTH: u32 = 80;
pub(super) const STANDALONE_NUMERIC_WIDTH: u32 = 96;
pub(super) const SELECT_WIDTH: u32 = 240;
pub(super) const STEPPER_BUTTON_WIDTH: u32 = 32;
pub(super) const CARD_RADIUS: f32 = 10.0;
pub(super) const CONTROL_RADIUS: f32 = 6.0;
pub(super) const CONTENT_WIDTH: u32 = 1440;
pub(super) const NAVIGATION_RAIL_WIDTH: f32 = 88.0;
pub(super) const NAVIGATION_FLYOUT_WIDTH: f32 = 252.0;
pub(super) const NAVIGATION_ITEM_HEIGHT: u32 = 64;
pub(super) const SIDE_PANEL_WIDTH: f32 = 320.0;
pub(super) const SIDEBAR_COLLAPSED_WIDTH: f32 = 64.0;
pub(super) const NAVIGATION_ROW_HEIGHT: u32 = 44;

pub(super) fn palette(light: bool, accent: Option<iced::Color>) -> iced::theme::Palette {
    let rgb =
        |color: u32| iced::Color::from_rgb8((color >> 16) as u8, (color >> 8) as u8, color as u8);
    let accent = accent.unwrap_or_else(|| rgb(if light { 0x246bfe } else { 0x78a6ff }));
    iced::theme::Palette {
        background: tint(
            if light { 248 } else { 18 },
            accent,
            if light { 0.04 } else { 0.012 },
        ),
        text: tint(if light { 24 } else { 235 }, accent, 0.015),
        primary: accent,
        success: rgb(if light { 0x168343 } else { 0x75d59b }),
        warning: rgb(if light { 0x946200 } else { 0xe8b45b }),
        danger: rgb(if light { 0xc43e51 } else { 0xf18b99 }),
    }
}

fn tint(gray: u8, accent: iced::Color, amount: f32) -> iced::Color {
    iced::theme::palette::mix(iced::Color::from_rgb8(gray, gray, gray), accent, amount)
}

pub(super) fn amoled_theme(accent: Option<iced::Color>) -> iced::Theme {
    use iced::theme::palette::Pair;
    let palette = iced::theme::Palette {
        background: iced::Color::BLACK,
        ..palette(false, accent)
    };
    iced::Theme::custom_with_fn("Winderust AMOLED", palette, |palette| {
        let mut colors = extended_palette(palette);
        colors.background.weaker = Pair::new(iced::Color::BLACK, palette.text);
        colors.background.weak = Pair::new(tint(8, palette.primary, 0.002), palette.text);
        colors.background.neutral = Pair::new(tint(20, palette.primary, 0.006), palette.text);
        colors
    })
}

/// Low-chroma surfaces share the selected accent without competing with controls.
pub(super) fn extended_palette(palette: iced::theme::Palette) -> iced::theme::palette::Extended {
    use iced::theme::palette::{Extended, Pair};
    let mut colors = Extended::generate(palette);
    let color = |dark, light, dark_tint, light_tint| {
        tint(
            if colors.is_dark { dark } else { light },
            palette.primary,
            if colors.is_dark {
                dark_tint
            } else {
                light_tint
            },
        )
    };
    colors.background.weaker = Pair::new(color(26, 239, 0.018, 0.06), palette.text); // navigation
    colors.background.weak = Pair::new(color(30, 255, 0.018, 0.016), palette.text); // cards / fields
    colors.background.neutral = Pair::new(color(40, 242, 0.026, 0.045), palette.text); // buttons
    colors.background.strong = Pair::new(color(62, 223, 0.04, 0.065), palette.text); // borders
    colors.secondary.base = Pair::new(color(176, 94, 0.06, 0.045), palette.background);
    let foreground = accent_foreground(palette.primary, colors.is_dark);
    for pair in [
        &mut colors.primary.base,
        &mut colors.primary.weak,
        &mut colors.primary.strong,
    ] {
        pair.text = foreground;
    }
    colors
}

// Matches origin/main's accent_glyph_color and primary_foreground policy.
fn accent_foreground(accent: iced::Color, dark: bool) -> iced::Color {
    let brightness = 0.299 * accent.r + 0.587 * accent.g + 0.114 * accent.b;
    if !dark || brightness < 140.0 / 255.0 {
        iced::Color::WHITE
    } else {
        iced::Color::from_rgb8(17, 17, 17)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn accent_tints_stay_readable_and_preserve_semantic_colors() {
        use crate::config::AppThemeMode;
        use iced::{Color, Theme};
        for mode in [
            AppThemeMode::Light,
            AppThemeMode::Dark,
            AppThemeMode::Amoled,
        ] {
            let light = mode == AppThemeMode::Light;
            let accents = [
                Color::from_rgb8(215, 60, 80),
                Color::from_rgb8(40, 130, 225),
                Color::from_rgb8(45, 165, 90),
                Color::from_rgb8(145, 75, 200),
                Color::from_rgb8(255, 235, 20),
                Color::BLACK,
                Color::WHITE,
            ];
            let themes = accents.map(|accent| {
                if mode == AppThemeMode::Amoled {
                    super::amoled_theme(Some(accent))
                } else {
                    Theme::custom_with_fn(
                        "accent",
                        super::palette(light, Some(accent)),
                        super::extended_palette,
                    )
                }
            });
            for (index, theme) in themes.iter().enumerate() {
                let palette = theme.palette();
                let colors = theme.extended_palette();
                assert_eq!(colors.is_dark, !light);
                assert_eq!(palette.primary, accents[index]);
                if mode == AppThemeMode::Amoled {
                    assert_eq!(palette.background, Color::BLACK);
                    assert_eq!(colors.background.weaker.color, Color::BLACK);
                    assert!(colors.background.weak.color.relative_luminance() > 0.0);
                    assert!(
                        colors.background.weak.color.relative_luminance()
                            < colors.background.neutral.color.relative_luminance()
                    );
                    assert!(
                        colors.background.neutral.color.relative_luminance()
                            < colors.background.strong.color.relative_luminance()
                    );
                }
                for color in [
                    palette.background,
                    colors.background.weaker.color,
                    colors.background.weak.color,
                    colors.background.neutral.color,
                ] {
                    assert!(
                        palette.text.relative_contrast(color) >= 4.5,
                        "body text: {color:?}"
                    );
                    assert!(
                        colors.secondary.base.color.relative_contrast(color) >= 4.5,
                        "muted text: {color:?}"
                    );
                }
                assert_eq!(palette.success, themes[0].palette().success);
                assert_eq!(palette.warning, themes[0].palette().warning);
                assert_eq!(palette.danger, themes[0].palette().danger);
            }
            for pair in themes.windows(2) {
                if mode != AppThemeMode::Amoled {
                    assert_ne!(pair[0].palette().background, pair[1].palette().background);
                }
                assert_ne!(
                    pair[0].extended_palette().background.weak.color,
                    pair[1].extended_palette().background.weak.color
                );
                assert_ne!(
                    pair[0].extended_palette().background.strong.color,
                    pair[1].extended_palette().background.strong.color
                );
            }
        }
    }

    #[test]
    fn accent_foreground_matches_original_theme_and_brightness_rules() {
        use iced::Color;
        for (accent, dark_foreground) in [
            (Color::from_rgb8(76, 194, 255), Color::from_rgb8(17, 17, 17)),
            (Color::from_rgb8(62, 96, 55), Color::WHITE),
            (Color::from_rgb8(139, 139, 139), Color::WHITE),
            (
                Color::from_rgb8(141, 141, 141),
                Color::from_rgb8(17, 17, 17),
            ),
        ] {
            assert_eq!(super::accent_foreground(accent, false), Color::WHITE);
            assert_eq!(super::accent_foreground(accent, true), dark_foreground);
        }
    }
}
