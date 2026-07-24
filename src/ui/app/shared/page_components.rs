use crate::ui::app::*;

pub(in crate::ui::app) fn action_log_page_help() -> SharedString {
    tooltip_lines(vec![
        t!("action_log.intro_1").to_string(),
        t!("action_log.intro_2").to_string(),
    ])
}

pub(in crate::ui::app) fn page_header_with_help(
    page: Page,
    help: Option<SharedString>,
    transition: Option<&BreadcrumbTransition>,
    cx: &mut Context<WinderustApp>,
) -> gpui::Div {
    let mut header = h_flex()
        .w_full()
        .min_h(px(PAGE_HEADER_HEIGHT))
        .flex_shrink_0()
        .items_center()
        .gap_2()
        .overflow_hidden();
    let mut breadcrumb_row = h_flex()
        .w_full()
        .min_w(px(0.0))
        .items_center()
        .gap_2()
        .overflow_hidden();

    let current_trail = breadcrumb_trail(page);
    let transition = transition.filter(|transition| transition.current == current_trail);
    let entering_start = transition
        .map(|transition| common_breadcrumb_prefix_len(&transition.previous, &current_trail))
        .unwrap_or(current_trail.len());

    if let Some(first) = current_trail.first() {
        breadcrumb_row = breadcrumb_row.child(breadcrumb_segment_element(
            first,
            current_trail.len() == 1,
            true,
            cx,
        ));
    }

    for (index, segment) in current_trail.iter().enumerate().skip(1) {
        let current = index + 1 == current_trail.len();
        let group = breadcrumb_segment_group(segment, current, true, cx);

        if transition.is_some() && index >= entering_start {
            breadcrumb_row = breadcrumb_row.child(breadcrumb_transition_group(
                SharedString::from(format!("breadcrumb-{:?}-{index}", segment.page)),
                true,
                group,
            ));
        } else {
            breadcrumb_row = breadcrumb_row.child(group);
        }
    }

    let mut breadcrumbs = div()
        .flex_1()
        .min_w(px(0.0))
        .relative()
        .overflow_hidden()
        .child(breadcrumb_row);

    if let Some(transition) = transition {
        if breadcrumb_starts_with(&transition.previous, &current_trail)
            && transition.previous.len() > current_trail.len()
        {
            breadcrumbs =
                breadcrumbs.child(breadcrumb_exit_overlay(transition, current_trail.len(), cx));
        }
    }

    header = header.child(breadcrumbs);

    if let Some(help) = help {
        header = header.child(title_info_button(
            SharedString::from(format!("page-info-{page:?}")),
            help,
        ));
    }

    header
}

pub(in crate::ui::app) fn tooltip_lines(
    lines: impl IntoIterator<Item = impl Into<SharedString>>,
) -> SharedString {
    let mut tooltip = String::new();
    for line in lines {
        let line: SharedString = line.into();
        if !tooltip.is_empty() {
            tooltip.push('\n');
        }
        tooltip.push_str(line.as_ref());
    }
    tooltip.into()
}

pub(in crate::ui::app) fn branded_panel() -> gpui::Div {
    v_flex()
        .w_full()
        .min_w(px(0.0))
        .relative()
        .overflow_hidden()
        .rounded(px(BRAND_RADIUS_SURFACE))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
}

pub(in crate::ui::app) fn section_card(title: &str) -> gpui::Div {
    branded_panel()
        .gap_3()
        .p_4()
        .child(section_title_text(title.to_owned()))
}

pub(in crate::ui::app) fn section_header(title: &str, help: impl Into<SharedString>) -> gpui::Div {
    let help = help.into();

    v_flex().w_full().min_w(px(0.0)).child(
        h_flex()
            .w_full()
            .min_h(px(26.0))
            .min_w(px(0.0))
            .items_center()
            .gap_1()
            .child(section_title_text(title.to_owned()))
            .child(title_info_button(
                SharedString::from(format!("section-info-{title}")),
                help,
            )),
    )
}

pub(in crate::ui::app) fn section_title_label(title: impl Into<SharedString>) -> Label {
    Label::new(title)
        .w_full()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::BOLD)
}

pub(in crate::ui::app) fn section_title_text(title: impl Into<SharedString>) -> Label {
    Label::new(title)
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::BOLD)
}

pub(in crate::ui::app) fn title_info_button(
    id: impl Into<SharedString>,
    tooltip: impl Into<SharedString>,
) -> AnyElement {
    div()
        .size(px(26.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .child(
            Button::new(id.into())
                .ghost()
                .rounded(px(999.0))
                .with_size(px(26.0))
                .icon(
                    Icon::new(NavIcon::Info)
                        .with_size(px(14.0))
                        .text_color(rgb(dim_text_color())),
                )
                .tooltip(tooltip),
        )
        .into_any_element()
}

pub(in crate::ui::app) fn rule_card(
    title: AnyElement,
    leading: AnyElement,
    collapse_indicator: AnyElement,
    card_target: RuleCardTarget,
    collapsed: bool,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    rule_card_with_header_action(
        title,
        leading,
        None,
        collapse_indicator,
        card_target,
        collapsed,
        cx,
    )
}

pub(in crate::ui::app) fn rule_card_with_header_action(
    title: AnyElement,
    leading: AnyElement,
    header_action: Option<AnyElement>,
    collapse_indicator: AnyElement,
    card_target: RuleCardTarget,
    _collapsed: bool,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    let header_padding = if header_action.is_some() {
        px(134.0)
    } else {
        px(52.0)
    };
    let card_id = SharedString::from(format!("rule-card-{card_target:?}"));
    let header_id = SharedString::from(format!("rule-card-header-{card_target:?}"));
    let header_action_id = SharedString::from(format!("rule-card-header-action-{card_target:?}"));
    let hover_id = format!("rule-card-hover-{card_target:?}");
    let header_card_target = card_target.clone();
    let trailing_card_target = card_target.clone();
    let trailing_hover_id = hover_id.clone();
    let mut trailing = h_flex()
        .id(SharedString::from(format!(
            "rule-card-trailing-{card_target:?}"
        )))
        .absolute()
        .top(px(0.0))
        .right(px(0.0))
        .h(px(CARD_ROW_HEIGHT))
        .items_center()
        .gap_1()
        .px_2()
        .block_mouse_except_scroll()
        .cursor_pointer()
        .capture_any_mouse_down(cx.listener(|app, event: &gpui::MouseDownEvent, _, cx| {
            handle_navigation_mouse_button(app, event.button, cx);
        }))
        .on_hover(move |hovered, _, cx| {
            set_card_hovered(trailing_hover_id.clone(), *hovered, cx);
        })
        .on_click(cx.listener(move |app, _, _, cx| {
            app.toggle_rule_card(trailing_card_target.clone(), cx);
        }));
    if let Some(header_action) = header_action {
        trailing = trailing.child(header_action);
    }
    trailing = trailing.child(collapse_indicator);

    v_flex()
        .id(card_id)
        .w_full()
        .min_w(px(0.0))
        .relative()
        .overflow_hidden()
        .border_b_1()
        .border_color(rgb(border_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .child(
            div()
                .relative()
                .w_full()
                .min_w(px(0.0))
                .h(px(CARD_ROW_HEIGHT))
                .id(header_id)
                .overflow_hidden()
                .child(animated_card_hover_layer(&hover_id))
                .child(
                    h_flex()
                        .w_full()
                        .min_w(px(0.0))
                        .h(px(CARD_ROW_HEIGHT))
                        .items_center()
                        .gap_2()
                        .pl_4()
                        .pr(header_padding)
                        .id(header_action_id)
                        .block_mouse_except_scroll()
                        .cursor_pointer()
                        .capture_any_mouse_down(cx.listener(
                            |app, event: &gpui::MouseDownEvent, _, cx| {
                                handle_navigation_mouse_button(app, event.button, cx);
                            },
                        ))
                        .on_hover({
                            let hover_id = hover_id.clone();
                            move |hovered, _, cx| {
                                set_card_hovered(hover_id.clone(), *hovered, cx);
                            }
                        })
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.toggle_rule_card(header_card_target.clone(), cx);
                        }))
                        .child(leading)
                        .child(title),
                )
                .child(trailing),
        )
}

pub(in crate::ui::app) fn rule_card_collapse_indicator(
    card_target: RuleCardTarget,
    collapsed: bool,
) -> AnyElement {
    div()
        .w(px(28.0))
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(dim_text_color()))
        .opacity(0.72)
        .cursor_pointer()
        .child(collapsible_chevron_icon(
            rule_card_body_motion_id(&card_target),
            collapsed,
        ))
        .into_any_element()
}
