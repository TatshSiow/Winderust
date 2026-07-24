use crate::ui::app::*;

pub(in crate::ui::app) fn accent_swatch(
    id_prefix: &'static str,
    color: u32,
    selected: bool,
) -> gpui::Stateful<gpui::Div> {
    let border = if selected {
        primary_text_color()
    } else {
        color
    };

    div()
        .id(SharedString::from(format!("{id_prefix}-{color:06x}")))
        .size(px(ACCENT_SWATCH_SIZE))
        .flex_shrink_0()
        .rounded(px(BRAND_RADIUS_CONTROL))
        .border_1()
        .border_color(rgb(border))
        .bg(rgb(color))
        .hover(|style| style.border_color(rgb(primary_text_color())))
        .cursor_pointer()
        .when(selected, |style| style.border_2())
}

pub(in crate::ui::app) fn accent_color_group(
    title: impl Into<SharedString>,
    swatches: AnyElement,
) -> gpui::Div {
    v_flex()
        .w_full()
        .min_w(px(0.0))
        .gap_2()
        .child(section_title_label(title))
        .child(swatches)
}

pub(in crate::ui::app) fn accent_picker_featured_colors() -> Vec<Hsla> {
    ACCENT_PALETTE
        .iter()
        .copied()
        .map(|color| rgb(color).into())
        .collect()
}

pub(in crate::ui::app) fn add_custom_accent_color(settings: &mut AccentSettings, color: u32) {
    settings
        .custom_colors
        .retain(|stored_color| *stored_color != color);
    settings.custom_colors.push(color);
}

pub(in crate::ui::app) fn remove_custom_accent_color(settings: &mut AccentSettings, color: u32) {
    settings
        .custom_colors
        .retain(|stored_color| *stored_color != color);
}

pub(in crate::ui::app) fn hsla_to_rgb_u32(color: Hsla) -> Option<u32> {
    let hex = color.to_hex();
    let hex = hex.trim_start_matches('#');
    if hex.len() < 6 {
        return None;
    }
    u32::from_str_radix(&hex[..6], 16).ok()
}

pub(in crate::ui::app) fn accent_palette_animation_height(custom_color_count: usize) -> f32 {
    let palette_rows = accent_swatch_row_count(ACCENT_PALETTE.len());
    px_spacing(6)
        + accent_custom_color_group_height(custom_color_count)
        + px_spacing(4)
        + accent_color_group_height(palette_rows)
}

pub(in crate::ui::app) fn accent_custom_color_group_height(custom_color_count: usize) -> f32 {
    let swatch_rows = accent_swatch_row_count(custom_color_count);
    let swatch_height = if swatch_rows == 0 {
        0.0
    } else {
        ACCENT_SWATCH_SIZE * swatch_rows as f32
            + px_spacing(2) * swatch_rows.saturating_sub(1) as f32
    };
    TEXT_BODY_LINE_HEIGHT + px_spacing(2) + swatch_height.max(ACCENT_COLOR_PICKER_WRAPPER_SIZE)
}

pub(in crate::ui::app) fn accent_color_group_height(row_count: usize) -> f32 {
    let row_count = row_count.max(1);
    TEXT_BODY_LINE_HEIGHT
        + px_spacing(2)
        + ACCENT_SWATCH_SIZE * row_count as f32
        + px_spacing(2) * row_count.saturating_sub(1) as f32
}

pub(in crate::ui::app) fn accent_swatch_row_count(swatch_count: usize) -> usize {
    swatch_count.div_ceil(ACCENT_SWATCHES_PER_ROW)
}
