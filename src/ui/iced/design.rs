//! Shared visual measurements. Table data geometry stays with its table.
pub(super) mod space {
    pub const TINY: u32 = 2;
    pub const TIGHT: u32 = 4;
    pub const CONTROL: u32 = 6;
    pub const SMALL: u32 = 8;
    pub const COMPACT: u32 = 10;
    pub const MEDIUM: u32 = 12;
    pub const CHART_INSET: u32 = 14;
    pub const LARGE: u32 = 16;
    pub const WIDE: u32 = 20;
    pub const SECTION: u32 = 24;
}

pub(super) mod typography {
    pub const FONT: &str = "Segoe UI";
    pub const BADGE: u32 = 11;
    pub const CAPTION: u32 = 12;
    pub const SECONDARY: u32 = 13;
    pub const BODY: u32 = 14;
    pub const SECTION: u32 = 16;
    pub const SUBTITLE: u32 = 18;
    pub const DIALOG_TITLE: u32 = 24;
    pub const TITLE: u32 = 28;
}

pub(super) const CONTROL_PADDING: [u16; 2] = [5, 10];
pub(super) const INPUT_PADDING: u16 = 5;
pub(super) const CHECKBOX_SIZE: u32 = 16;
pub(super) const SWITCH_SIZE: u32 = 20;
pub(super) const ICON_SIZE: u32 = 18;
pub(super) const SLIDER_HEIGHT: u32 = 16;
pub(super) const NUMERIC_WIDTH: u32 = 80;
pub(super) const STANDALONE_NUMERIC_WIDTH: u32 = 96;
pub(super) const SELECT_WIDTH: u32 = 240;
pub(super) const STEPPER_BUTTON_WIDTH: u32 = 32;
pub(super) const STEPPER_UNIT_WIDTH: u32 = 24;
pub(super) const CARD_RADIUS: f32 = 6.0;
pub(super) const CONTROL_RADIUS: f32 = 4.0;
pub(super) const CONTENT_WIDTH: u32 = 1040;
pub(super) const SIDE_PANEL_BREAKPOINT: f32 = 1400.0;
pub(super) const NAVIGATION_WIDTH: f32 = 264.0;
pub(super) const NAVIGATION_COLLAPSED_WIDTH: f32 = 72.0;
pub(super) const NAVIGATION_ROW_HEIGHT: u32 = 40;
pub(super) const NAVIGATION_CHILD_ROW_HEIGHT: u32 = 34;
pub(super) const STATUS_WIDTH: f32 = 320.0;
pub(super) const STATUS_COLLAPSED_WIDTH: f32 = 48.0;

pub(super) fn palette(light: bool) -> iced::theme::Palette {
    let rgb =
        |color: u32| iced::Color::from_rgb8((color >> 16) as u8, (color >> 8) as u8, color as u8);
    iced::theme::Palette {
        background: rgb(if light { 0xf3f4f5 } else { 0x0f1011 }),
        text: rgb(if light { 0x202327 } else { 0xf0f0f2 }),
        primary: rgb(0x35bfff),
        success: rgb(if light { 0x477d23 } else { 0xa4db61 }),
        warning: rgb(0xe8b45b),
        danger: rgb(0xe56d76),
    }
}
