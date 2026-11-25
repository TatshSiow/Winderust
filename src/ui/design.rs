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
    pub const DEFAULT_FONT: iced::Font = iced::Font {
        weight: iced::font::Weight::Normal,
        ..FONT
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
pub(super) const CARD_RADIUS: f32 = 10.0;
pub(super) const CONTROL_RADIUS: f32 = 6.0;
pub(super) const CONTENT_WIDTH: u32 = 1440;
pub(super) const NAVIGATION_RAIL_WIDTH: f32 = 88.0;
pub(super) const NAVIGATION_FLYOUT_WIDTH: f32 = 252.0;
pub(super) const NAVIGATION_ITEM_HEIGHT: u32 = 64;
pub(super) const SIDE_PANEL_WIDTH: f32 = 320.0;
pub(super) const PAGE_BREADCRUMB_HEIGHT: u32 = 48;
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

const CONTRAST_THEME_NAME: &str = "Winderust Contrast";

pub(super) fn is_contrast(theme: &iced::Theme) -> bool {
    use iced::theme::Base;
    theme.name() == CONTRAST_THEME_NAME
}

pub(super) fn contrast_theme(
    colors: crate::platform::windows::appearance::ContrastColors,
) -> iced::Theme {
    use iced::theme::palette::{Extended, Pair};
    let rgb =
        |color: u32| iced::Color::from_rgb8((color >> 16) as u8, (color >> 8) as u8, color as u8);
    let pair = |color, text| Pair {
        color,
        text: readable_on(text, color),
    };
    let window = pair(rgb(colors.window), rgb(colors.window_text));
    let highlight_fill = rgb(colors.highlight);
    let highlight = pair(highlight_fill, rgb(colors.highlight_text));
    let foreground = pair(window.text, window.color);
    // Iced draws selected input text with the same foreground as unselected text.
    let input_selection = (0..10)
        .map(|step| iced::theme::palette::mix(highlight_fill, window.color, step as f32 / 10.0))
        .find(|color| color.relative_contrast(window.text) >= 4.5)
        .unwrap_or(window.color);
    let palette = iced::theme::Palette {
        background: window.color,
        text: window.text,
        primary: highlight.color,
        success: window.text,
        warning: window.text,
        danger: window.text,
    };
    iced::Theme::custom_with_fn(CONTRAST_THEME_NAME, palette, move |palette| {
        let mut colors = Extended::generate(palette);
        let background = &mut colors.background;
        background.base = window;
        background.weakest = window;
        background.weaker = window;
        background.weak = window;
        background.neutral = window;
        background.strong = foreground; // shared control outlines and dividers
        background.stronger = foreground;
        background.strongest = foreground;
        colors.primary.base = highlight;
        colors.primary.weak = pair(input_selection, window.text);
        colors.primary.strong = highlight;
        colors.secondary.base = foreground;
        colors.secondary.weak = foreground;
        colors.secondary.strong = foreground;
        colors.danger.base = foreground;
        colors.danger.weak = foreground;
        colors.danger.strong = foreground;
        colors
    })
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
    colors.background.weak = Pair::new(color(30, 255, 0.018, 0.016), palette.text); // cards
    colors.background.neutral = Pair::new(color(40, 242, 0.026, 0.045), palette.text); // fields / buttons
    colors.background.strong = Pair::new(color(62, 223, 0.04, 0.065), palette.text); // borders
    colors.secondary.base = Pair::new(color(176, 94, 0.06, 0.045), palette.background);
    for pair in [
        &mut colors.primary.base,
        &mut colors.primary.weak,
        &mut colors.primary.strong,
    ] {
        pair.text = contrasting_neutral(pair.color);
    }
    colors
}

pub(super) fn readable_on(color: iced::Color, background: iced::Color) -> iced::Color {
    if color.relative_contrast(background) >= 4.5 {
        return color;
    }
    let candidate = iced::theme::palette::readable(background, color);
    if candidate.relative_contrast(background) >= 4.5 {
        candidate
    } else {
        contrasting_neutral(background)
    }
}

fn contrasting_neutral(background: iced::Color) -> iced::Color {
    use iced::Color;
    if Color::BLACK.relative_contrast(background) > Color::WHITE.relative_contrast(background) {
        Color::BLACK
    } else {
        Color::WHITE
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_contrast_preserves_opaque_window_and_highlight_pairs() {
        use crate::platform::windows::appearance::ContrastColors;
        use iced::Color;
        for (window, window_text, highlight, highlight_text) in [
            (0x000000, 0xffffff, 0xffff00, 0x000000),
            (0xffffff, 0x000000, 0x0055aa, 0xffffff),
        ] {
            let theme = super::contrast_theme(ContrastColors {
                window,
                window_text,
                highlight,
                highlight_text,
            });
            let rgb =
                |color: u32| Color::from_rgb8((color >> 16) as u8, (color >> 8) as u8, color as u8);
            let colors = theme.extended_palette();
            for background in [
                colors.background.base,
                colors.background.weaker,
                colors.background.weak,
                colors.background.neutral,
            ] {
                assert_eq!(background.color, rgb(window));
                assert_eq!(background.text, rgb(window_text));
            }
            assert_eq!(colors.primary.base.color, rgb(highlight));
            assert_eq!(colors.primary.base.text, rgb(highlight_text));
            assert!(
                colors
                    .primary
                    .weak
                    .color
                    .relative_contrast(rgb(window_text))
                    >= 4.5
            );
        }
    }

    #[test]
    fn accent_palette_preserves_fills_and_keeps_text_readable() {
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
                Color::from_rgb8(36, 107, 254),
                Color::from_rgb8(120, 166, 255),
                Color::from_rgb8(255, 235, 20),
                Color::from_rgb8(117, 117, 117),
                Color::from_rgb8(118, 118, 118),
                Color::from_rgb8(139, 139, 139),
                Color::from_rgb8(141, 141, 141),
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
                let original = iced::theme::palette::Extended::generate(palette);
                for (pair, original) in [
                    colors.primary.base,
                    colors.primary.weak,
                    colors.primary.strong,
                ]
                .into_iter()
                .zip([
                    original.primary.base,
                    original.primary.weak,
                    original.primary.strong,
                ]) {
                    assert_eq!(pair.color, original.color);
                    assert!(pair.text.relative_contrast(pair.color) >= 4.5);
                }
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
    fn readable_text_preserves_passing_colors_and_repairs_native_fallbacks() {
        use iced::Color;
        for (color, background) in [
            (Color::BLACK, Color::WHITE),
            (Color::WHITE, Color::BLACK),
            (Color::from_rgb8(0, 80, 160), Color::WHITE),
        ] {
            assert!(color.relative_contrast(background) >= 4.5);
            assert_eq!(super::readable_on(color, background), color);
        }
        for gray in 0..=255 {
            let background = Color::from_rgb8(gray, gray, gray);
            for color in [Color::BLACK, Color::WHITE, background] {
                let readable = super::readable_on(color, background);
                assert!(
                    readable.relative_contrast(background) >= 4.5,
                    "foreground {color:?} on {background:?} became {readable:?}"
                );
            }
        }
    }
}
