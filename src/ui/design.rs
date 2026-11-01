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

pub(super) fn palette(light: bool) -> iced::theme::Palette {
    let rgb =
        |color: u32| iced::Color::from_rgb8((color >> 16) as u8, (color >> 8) as u8, color as u8);
    iced::theme::Palette {
        background: rgb(if light { 0xf5f7fb } else { 0x11151d }),
        text: rgb(if light { 0x182039 } else { 0xe9edf6 }),
        primary: rgb(if light { 0x246bfe } else { 0x78a6ff }),
        success: rgb(if light { 0x168343 } else { 0x75d59b }),
        warning: rgb(if light { 0x946200 } else { 0xe8b45b }),
        danger: rgb(if light { 0xc43e51 } else { 0xf18b99 }),
    }
}

/// Neutral surface roles shared by native controls and application components.
pub(super) fn extended_palette(palette: iced::theme::Palette) -> iced::theme::palette::Extended {
    use iced::theme::palette::{Extended, Pair};
    let mut colors = Extended::generate(palette);
    let color = |dark, light| {
        let value = if colors.is_dark { dark } else { light };
        iced::Color::from_rgb8((value >> 16) as u8, (value >> 8) as u8, value as u8)
    };
    colors.background.weaker = Pair::new(color(0x171d28, 0xebf0f7), palette.text); // navigation
    colors.background.weak = Pair::new(color(0x1b2230, 0xffffff), palette.text); // cards / fields
    colors.background.neutral = Pair::new(color(0x293345, 0xeef2f8), palette.text); // buttons
    colors.background.strong = Pair::new(color(0x39465b, 0xdce3ef), palette.text); // borders
    colors.secondary.base = Pair::new(color(0xa2afc5, 0x626f88), palette.background);
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
