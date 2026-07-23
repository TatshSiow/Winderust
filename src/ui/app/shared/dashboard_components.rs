use crate::ui::app::*;

pub(in crate::ui::app) fn stat_grid(rows: Vec<(String, String)>) -> gpui::Div {
    let mut list = v_flex().w_full().min_w(px(0.0));
    for (index, (label, value)) in rows.into_iter().enumerate() {
        list = list.child(
            h_flex()
                .w_full()
                .min_w(px(0.0))
                .min_h(px(36.0))
                .items_center()
                .gap_3()
                .px_4()
                .when(index > 0, |row| {
                    row.border_t_1().border_color(rgb(border_color()))
                })
                .child(
                    div()
                        .w(px(172.0))
                        .flex_shrink_0()
                        .truncate()
                        .text_color(rgb(dim_text_color()))
                        .text_size(px(TEXT_BODY_SIZE))
                        .line_height(px(TEXT_BODY_LINE_HEIGHT))
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_color(rgb(primary_text_color()))
                        .text_size(px(TEXT_BODY_SIZE))
                        .line_height(px(TEXT_BODY_LINE_HEIGHT))
                        .child(value),
                ),
        );
    }
    branded_panel().py_1().child(list)
}

pub(in crate::ui::app) fn dashboard_card_slot(card: AnyElement) -> gpui::Div {
    div()
        .w(relative(0.49))
        .min_w(px(320.0))
        .flex_1()
        .child(card)
}

pub(in crate::ui::app) fn dashboard_summary_card(
    title: impl Into<SharedString>,
    header_trailing: Option<AnyElement>,
    body: AnyElement,
) -> gpui::Div {
    let mut header = h_flex()
        .w_full()
        .min_w(px(0.0))
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .child(section_title_label(title)),
        );

    if let Some(header_trailing) = header_trailing {
        header = header.child(header_trailing);
    }

    v_flex()
        .w_full()
        .min_w(px(0.0))
        .h(px(DASHBOARD_SUMMARY_CARD_HEIGHT))
        .relative()
        .overflow_hidden()
        .p_3()
        .gap_2()
        .rounded(px(BRAND_RADIUS_SURFACE))
        .bg(rgb(settings_card_color()))
        .child(header)
        .child(
            div()
                .w_full()
                .min_w(px(0.0))
                .flex_1()
                .min_h(px(0.0))
                .child(body),
        )
}

pub(in crate::ui::app) fn dashboard_summary_header_value(
    value: impl Into<SharedString>,
) -> gpui::Div {
    div()
        .max_w(px(180.0))
        .flex_shrink_0()
        .truncate()
        .whitespace_nowrap()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(value.into())
}

pub(in crate::ui::app) fn dashboard_split_value_row(items: [gpui::Div; 2]) -> gpui::Div {
    items.into_iter().fold(
        h_flex()
            .w_full()
            .min_w(px(0.0))
            .items_center()
            .justify_center()
            .gap_2()
            .flex_wrap(),
        |row, item| row.child(item),
    )
}

pub(in crate::ui::app) fn dashboard_split_value(
    label: String,
    value: impl Into<SharedString>,
    color: Hsla,
) -> gpui::Div {
    h_flex()
        .w(px(DASHBOARD_SPLIT_ITEM_WIDTH))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .gap_1()
        .text_size(px(TEXT_CAPTION_SIZE))
        .line_height(px(TEXT_CAPTION_LINE_HEIGHT))
        .text_color(rgb(muted_text_color()))
        .child(div().size(px(7.0)).rounded_full().flex_shrink_0().bg(color))
        .child(div().flex_shrink_0().whitespace_nowrap().child(label))
        .child(
            div()
                .w(px(DASHBOARD_SPLIT_VALUE_WIDTH))
                .flex_shrink_0()
                .truncate()
                .whitespace_nowrap()
                .child(value.into()),
        )
}

pub(in crate::ui::app) fn io_usage_split_value(
    label: String,
    value: Option<f64>,
    color: Hsla,
) -> gpui::Div {
    dashboard_split_value(label, io_usage_label(value), color)
}

pub(in crate::ui::app) fn dashboard_graph_hover_overlay(
    graph_id: &'static str,
    tooltips: Vec<SharedString>,
) -> gpui::Div {
    let mut overlay = h_flex().absolute().inset_0().w_full().h_full();
    for (index, tooltip) in tooltips.into_iter().enumerate() {
        overlay = overlay.child(
            div()
                .id(SharedString::from(format!(
                    "{graph_id}-sample-hover-{index}"
                )))
                .h_full()
                .flex_1()
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx)),
        );
    }
    overlay
}

pub(in crate::ui::app) fn dashboard_graph_sample_tooltips(
    points: &[DashboardDualLinePoint],
    first_series_label: &str,
    second_series_label: &str,
) -> Vec<SharedString> {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            tooltip_lines([
                dashboard_sample_age_label(index),
                format!("{first_series_label}: {}", point.first_label),
                format!("{second_series_label}: {}", point.second_label),
            ])
        })
        .collect()
}

pub(in crate::ui::app) fn dashboard_sample_age_label(index: usize) -> String {
    let age = CPU_USAGE_HISTORY_LEN.saturating_sub(index + 1);
    if age == 0 {
        t!("common.latest_sample").to_string()
    } else {
        t!("common.seconds_ago", count = age).to_string()
    }
}

pub(in crate::ui::app) fn dashboard_cpu_dual_line_points(
    values: &VecDeque<CpuUsageHistorySample>,
    base_frequency_mhz: Option<u32>,
) -> Vec<DashboardDualLinePoint> {
    let sample_count = values.len().min(CPU_USAGE_HISTORY_LEN);
    let missing_samples = CPU_USAGE_HISTORY_LEN - sample_count;
    let start_index = values.len().saturating_sub(sample_count);
    let base_frequency_mhz = base_frequency_mhz
        .or_else(|| {
            values
                .iter()
                .skip(start_index)
                .filter_map(|sample| sample.frequency_mhz)
                .min()
        })
        .unwrap_or(0);
    let peak_frequency_mhz = values
        .iter()
        .skip(start_index)
        .filter_map(|sample| sample.frequency_mhz)
        .max()
        .filter(|peak| *peak > base_frequency_mhz);

    let mut points = Vec::with_capacity(CPU_USAGE_HISTORY_LEN);
    for index in 0..CPU_USAGE_HISTORY_LEN {
        let sample = if index < missing_samples {
            None
        } else {
            Some(values[start_index + index - missing_samples])
        };

        points.push(DashboardDualLinePoint {
            tick: dashboard_history_tick(index),
            first_value: f64::from(sample.map_or(0.0, |sample| sample.percent.max(0.0))),
            second_value: f64::from(normalize_cpu_frequency_percent(
                sample.and_then(|sample| sample.frequency_mhz),
                base_frequency_mhz,
                peak_frequency_mhz,
            )),
            first_label: cpu_usage_label(sample.map(|sample| sample.percent)),
            second_label: cpu_frequency_label(sample.and_then(|sample| sample.frequency_mhz)),
        });
    }
    points
}

pub(in crate::ui::app) fn normalize_cpu_frequency_percent(
    frequency_mhz: Option<u32>,
    base_frequency_mhz: u32,
    peak_frequency_mhz: Option<u32>,
) -> f32 {
    let Some(frequency_mhz) = frequency_mhz else {
        return 0.0;
    };
    let Some(peak_frequency_mhz) = peak_frequency_mhz else {
        return 0.0;
    };
    let range = peak_frequency_mhz.saturating_sub(base_frequency_mhz);
    if range == 0 || frequency_mhz <= base_frequency_mhz {
        return 0.0;
    }

    ((frequency_mhz.saturating_sub(base_frequency_mhz) as f32 / range as f32) * 100.0)
        .clamp(0.0, 100.0)
}

pub(in crate::ui::app) fn dashboard_dual_line_points(
    values: impl ExactSizeIterator<Item = (f32, f32)>,
    first_label: impl Fn(Option<f32>) -> String,
    second_label: impl Fn(Option<f32>) -> String,
) -> Vec<DashboardDualLinePoint> {
    let value_count = values.len();
    let sample_count = value_count.min(CPU_USAGE_HISTORY_LEN);
    let missing_samples = CPU_USAGE_HISTORY_LEN - sample_count;
    let mut values = values.skip(value_count.saturating_sub(sample_count));

    let mut points = Vec::with_capacity(CPU_USAGE_HISTORY_LEN);
    for index in 0..CPU_USAGE_HISTORY_LEN {
        let sample = if index < missing_samples {
            None
        } else {
            values.next()
        };
        let (first_value, second_value) = sample.unwrap_or((0.0, 0.0));

        points.push(DashboardDualLinePoint {
            tick: dashboard_history_tick(index),
            first_value: f64::from(first_value.max(0.0)),
            second_value: f64::from(second_value.max(0.0)),
            first_label: first_label(sample.map(|sample| sample.0)),
            second_label: second_label(sample.map(|sample| sample.1)),
        });
    }
    points
}

pub(in crate::ui::app) fn dashboard_history_tick(index: usize) -> String {
    format!("sample-{index:02}")
}

pub(in crate::ui::app) fn dashboard_primary_series_color() -> Hsla {
    Hsla::from(rgb(accent_color())).lighten(0.16)
}

pub(in crate::ui::app) fn dashboard_secondary_series_color() -> Hsla {
    Hsla::from(rgb(accent_color())).darken(0.18)
}

pub(in crate::ui::app) fn titled_status_list(
    title: &str,
    header_trailing: Option<AnyElement>,
    items: Vec<(String, String)>,
    empty_message: Option<String>,
) -> gpui::Div {
    let mut list = v_flex()
        .w_full()
        .min_w(px(0.0))
        .flex_1()
        .min_h(px(0.0))
        .gap_1()
        .overflow_y_scrollbar();

    if items.is_empty() {
        if let Some(message) = empty_message {
            list = list.child(text_muted(message).py_1());
        }
    }

    for (label, detail) in items {
        let mut content = h_flex()
            .flex_1()
            .min_w(px(0.0))
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(px(TEXT_BODY_SIZE))
                    .line_height(px(TEXT_BODY_LINE_HEIGHT))
                    .child(label),
            );

        if !detail.is_empty() {
            content = content.child(text_muted(detail).truncate().flex_shrink_0());
        }

        list = list.child(
            h_flex()
                .w_full()
                .min_w(px(0.0))
                .items_center()
                .gap_2()
                .py_1()
                .child(
                    div()
                        .size(px(6.0))
                        .rounded_full()
                        .flex_shrink_0()
                        .bg(rgb(accent_color())),
                )
                .child(content),
        );
    }

    dashboard_summary_card(title.to_owned(), header_trailing, list.into_any_element())
}

pub(in crate::ui::app) const PROCESS_LIST_NAME_MIN_WIDTH: f32 = 180.0;
pub(in crate::ui::app) const PROCESS_LIST_NAME_MAX_WIDTH: f32 = 340.0;
pub(in crate::ui::app) const PROCESS_LIST_PID_MIN_WIDTH: f32 = 56.0;
pub(in crate::ui::app) const PROCESS_LIST_PID_MAX_WIDTH: f32 = 90.0;
pub(in crate::ui::app) const PROCESS_LIST_COLUMN_MIN_WIDTH: f32 = 72.0;
pub(in crate::ui::app) const PROCESS_LIST_COLUMN_MAX_WIDTH: f32 = 250.0;
pub(in crate::ui::app) const PROCESS_LIST_TEXT_CELL_HORIZONTAL_PADDING: f32 = 24.0;
pub(in crate::ui::app) const PROCESS_LIST_NAME_CELL_NON_TEXT_WIDTH: f32 = 76.0;
pub(in crate::ui::app) const PROCESS_LIST_SORT_ICON_WIDTH: f32 = 18.0;
pub(in crate::ui::app) const PROCESS_LIST_SORT_HEADER_GAP: f32 = 4.0;
pub(in crate::ui::app) const PROCESS_LIST_SPLIT_LABEL_WIDTH: f32 = 18.0;
pub(in crate::ui::app) const PROCESS_LIST_SPLIT_LABEL_GAP: f32 = 4.0;
pub(in crate::ui::app) const PROCESS_LIST_CELL_EDITOR_WIDTH: f32 = 220.0;
pub(in crate::ui::app) const PROCESS_LIST_ROW_HORIZONTAL_PADDING: f32 = 32.0;
pub(in crate::ui::app) const PROCESS_LIST_COLUMN_GAP: f32 = 12.0;
pub(in crate::ui::app) const PROCESS_LIST_HEADER_HEIGHT: f32 = 32.0;
pub(in crate::ui::app) const PROCESS_LIST_ROW_HEIGHT: f32 = 52.0;
pub(in crate::ui::app) const PROCESS_LIST_TOOLBAR_HEIGHT: f32 = 40.0;
pub(in crate::ui::app) const PROCESS_LIST_VERTICAL_GAP_TOTAL: f32 = 8.0;
pub(in crate::ui::app) const PROCESS_LIST_SCROLLBAR_GUTTER: f32 = 16.0;
pub(in crate::ui::app) const PROCESS_LIST_TREE_TOGGLE_WIDTH: f32 = 16.0;
pub(in crate::ui::app) const PROCESS_LIST_COLUMN_VISIBILITY_DROPDOWN_ID: &str =
    "process-list-column-visibility";
pub(in crate::ui::app) const PROCESS_LIST_COLUMN_VISIBILITY_DROPDOWN_WIDTH: f32 = 360.0;
