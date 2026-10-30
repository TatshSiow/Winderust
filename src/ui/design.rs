//! Shared measurements and theme roles for Wind UI's built-in controls.

use windui::{geometry::Color, theme::Theme};

pub(super) mod space {
    pub const TINY: u32 = 2;
    pub const TIGHT: u32 = 4;
    pub const CONTROL: u32 = 6;
    pub const SMALL: u32 = 8;
    pub const COMPACT: u32 = 10;
    pub const MEDIUM: u32 = 12;
    pub const SECTION: u32 = 20;
}

pub(super) mod typography {
    pub const BADGE: u32 = 11;
    pub const CAPTION: u32 = 12;
    pub const SECONDARY: u32 = 13;
    pub const BODY: u32 = 14;
    pub const SECTION: u32 = 16;
    pub const SUBTITLE: u32 = 18;
    pub const TITLE: u32 = 22;
}

pub(super) const CARD_PADDING: u32 = 16;
pub(super) const CARD_GAP: u32 = space::SMALL;
pub(super) const SETTING_ROW_HEIGHT: u32 = 34;
pub(super) const CONTENT_WIDTH: u32 = 1040;
pub(super) const ICON_SIZE: u32 = 18;
pub(super) const NAVIGATION_WIDTH: f32 = 264.0;
pub(super) const SIDE_PANEL_WIDTH: f32 = 320.0;
pub(super) const SIDEBAR_COLLAPSED_WIDTH: f32 = 56.0;
pub(super) const NAVIGATION_ROW_HEIGHT: u32 = 36;
pub(super) const NAVIGATION_CHILD_ROW_HEIGHT: u32 = 32;
pub(super) const CARD_RADIUS: f32 = 8.0;
pub(super) const CONTROL_RADIUS: f32 = 4.0;

/// Apply Winderust's shared semantic colors to the native widget theme.
/// The application resolves System/Light/Dark and the chosen accent before calling.
pub(super) fn theme(light: bool, accent: Option<Color>) -> Theme {
    let mut theme = if light {
        Theme::default()
    } else {
        Theme::dark()
    };
    let palette = &mut theme.palette;
    palette.bg = Color::hex(if light { 0xf4f6fa } else { 0x131a26 });
    palette.text = Color::hex(if light { 0x202b3b } else { 0xedf1f7 });
    palette.surface = Color::hex(if light { 0xffffff } else { 0x1b2534 });
    palette.surface_alt = Color::hex(if light { 0xeef1f6 } else { 0x10161f });
    palette.surface_inverse = palette.text;
    palette.on_surface_inverse = palette.bg;
    palette.border = Color::hex(if light { 0xd6deea } else { 0x2c384b });
    palette.divider = Color::hex(if light { 0xdce3ed } else { 0x2b3748 });
    palette.track = Color::hex(if light { 0xb9c4d6 } else { 0x3c4960 });
    palette.text_muted = Color::hex(if light { 0x56667d } else { 0x9ba9c0 });
    palette.text_subtle = Color::hex(if light { 0x627189 } else { 0x91a0b8 });
    palette.text_disabled = Color::hex(if light { 0x9d9d9d } else { 0x808080 });
    palette.placeholder = palette.text_muted;
    palette.success = Color::hex(if light { 0x477d23 } else { 0xa4db61 });
    palette.warning = Color::hex(0xe8b45b);
    palette.danger = Color::hex(0xe56d76);
    palette.accent = accent.unwrap_or_else(|| Color::hex(if light { 0x2f6bed } else { 0x5487f5 }));
    palette.accent_hover = palette.accent.lighten(0.1);
    palette.accent_active = palette.accent.darken(0.1);
    palette.on_accent = accent_foreground(palette.accent);
    theme.font_family = Some("Segoe UI".to_owned());
    theme.metrics.font_sm = typography::SECONDARY as f32;
    theme.metrics.font_md = typography::BODY as f32;
    theme.metrics.font_lg = typography::SECTION as f32;
    theme.metrics.corner_sm = CONTROL_RADIUS;
    theme.metrics.corner_md = CARD_RADIUS;
    theme.metrics.corner_lg = CARD_RADIUS;
    theme.button.corner = Some(CONTROL_RADIUS);
    theme.link.color = Some(palette.text);
    theme.link.hover = Some(palette.text);
    theme.link.pressed = Some(palette.text_muted);
    let field_bg = palette.surface;
    theme.input.bg = Some(field_bg);
    theme.input.corner = Some(CONTROL_RADIUS);
    theme.dropdown.bg = Some(field_bg);
    theme.dropdown.corner = Some(CONTROL_RADIUS);
    theme.segment.bg = Some(field_bg);
    theme.segment.selected_bg = Some(Color::rgba(
        palette.accent.r,
        palette.accent.g,
        palette.accent.b,
        36,
    ));
    theme.segment.selected_text = Some(palette.text);
    theme.segment.corner = Some(CONTROL_RADIUS);
    theme.accordion.header_bg = Some(palette.surface);
    theme.accordion.corner = Some(CARD_RADIUS);
    theme.card.corner = Some(CARD_RADIUS);
    theme.card.pad = Some(CARD_PADDING as i32);
    theme.card.gap = Some(CARD_GAP as i32);
    theme.card.title_size = Some(typography::SECTION as f32);
    theme.card.title_weight = Some(600);
    theme.form.row_height = Some(SETTING_ROW_HEIGHT as i32);
    theme.form.gap = Some(space::MEDIUM as i32);
    theme.form.row_pad_y = Some(8);
    theme.form.label_size = Some(typography::BODY as f32);
    theme.form.label_weight = Some(400);
    theme.form.desc_size = Some(typography::CAPTION as f32);
    theme.form.label_max_lines = Some(2);
    theme
}

pub(super) fn accent_foreground(accent: Color) -> Color {
    let dark = Color::hex(0x111111);
    if contrast_ratio(accent, dark) >= contrast_ratio(accent, Color::WHITE) {
        dark
    } else {
        Color::WHITE
    }
}

fn contrast_ratio(left: Color, right: Color) -> f32 {
    let left = relative_luminance(left);
    let right = relative_luminance(right);
    (left.max(right) + 0.05) / (left.min(right) + 0.05)
}

/// WCAG contrast uses linear-light sRGB, independent of the surrounding theme.
fn relative_luminance(color: Color) -> f32 {
    let linear = |channel: u8| {
        let value = f32::from(channel) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_text_chooses_the_better_contrast_in_both_themes() {
        let dark = Color::hex(0x111111);
        // Reference ratios were computed independently with f64 sRGB arithmetic.
        for (accent, foreground, expected_contrast) in [
            (Color::BLACK, Color::WHITE, 21.0),
            (Color::WHITE, dark, 18.883_060_965),
            (Color::hex(0x35bfff), dark, 9.034_823_042),
            (Color::hex(0x4cc2ff), dark, 9.412_769_350),
            (Color::hex(0x3e6037), Color::WHITE, 7.159_557_564),
            (Color::hex(0x101720), Color::WHITE, 18.018_834_501),
            (Color::hex(0xff0000), dark, 4.722_563_628),
            (Color::hex(0x00ff00), dark, 13.761_255_476),
            (Color::hex(0x0000ff), Color::WHITE, 8.592_471_358),
            // Near the crossover, neither allowed foreground reaches 4.5:1.
            (Color::hex(0x777777), Color::WHITE, 4.478_089_454),
            (Color::hex(0x808080), dark, 4.781_200_030),
        ] {
            for light in [false, true] {
                let palette = theme(light, Some(accent)).palette;
                assert_eq!(palette.accent, accent, "light={light}: accent changed");
                assert_eq!(
                    palette.on_accent, foreground,
                    "accent={accent:?}, light={light}: unexpected text color"
                );
                let contrast = contrast_ratio(accent, palette.on_accent);
                assert!(
                    (f64::from(contrast) - expected_contrast).abs() < 0.000_01,
                    "accent={accent:?}, light={light}, foreground={foreground:?}: \
                     contrast={contrast:.9}, expected={expected_contrast:.9}"
                );
                for alternative in [Color::WHITE, dark] {
                    let alternative_contrast = contrast_ratio(accent, alternative);
                    assert!(
                        contrast >= alternative_contrast,
                        "accent={accent:?}, light={light}: selected contrast={contrast:.9} \
                         is below {alternative:?} contrast={alternative_contrast:.9}"
                    );
                }
            }
        }
    }

    #[test]
    fn relative_luminance_matches_known_contrast_extremes() {
        assert!((contrast_ratio(Color::WHITE, Color::BLACK) - 21.0).abs() < 0.001);
        assert!((contrast_ratio(Color::BLACK, Color::WHITE) - 21.0).abs() < 0.001);
        assert!((contrast_ratio(Color::hex(0x111111), Color::WHITE) - 18.883).abs() < 0.001);
        assert_eq!(
            contrast_ratio(Color::hex(0x777777), Color::hex(0x777777)),
            1.0
        );
    }
}
