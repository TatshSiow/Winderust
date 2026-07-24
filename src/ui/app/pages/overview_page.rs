use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_section_landing_page(
        &self,
        section_page: Page,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut cards = v_flex().w_full().min_w(px(0.0)).gap_2();

        if section_page == Page::PowerPlanControl {
            cards = cards.child(setting_action_card(
                "pause-power-plan-switching-while-plugged-in",
                t!("power_plan_control.pause_plugged").to_string(),
                switch_toggle_action(
                    "pause-power-plan-switching-while-plugged-in-toggle",
                    self.settings
                        .general
                        .pause_power_plan_switching_while_plugged_in,
                    cx.listener(|app, checked, _, cx| {
                        app.settings
                            .general
                            .pause_power_plan_switching_while_plugged_in = *checked;
                        cx.notify();
                    }),
                ),
            ));
            cards = cards.child(section_title_text(t!("nav.automation_group").to_string()));
            for target in [
                Page::ByForeground,
                Page::ByRunningApp,
                Page::ByCpuLoad,
                Page::ByActivity,
                Page::ByTime,
            ] {
                cards = cards.child(
                    section_landing_card(target, cx)
                        .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                            app.navigate_to(target, cx);
                        }))
                        .into_any_element(),
                );
            }

            cards = cards.child(section_title_text(t!("nav.advanced_group").to_string()));
            let target = Page::AdvancedPowerPlanTuning;
            cards = cards.child(
                section_landing_card(target, cx)
                    .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                        app.navigate_to(target, cx);
                    }))
                    .into_any_element(),
            );
        } else if let Some(pages) = section_page.child_pages() {
            for page in pages {
                let target = *page;
                cards = cards.child(
                    section_landing_card(target, cx)
                        .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                            app.navigate_to(target, cx);
                        }))
                        .into_any_element(),
                );
            }
        }

        self.page_shell(section_page, cx)
            .child(cards)
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_home_page_page_card(
        &self,
        target: Page,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        dashboard_card_slot(
            section_landing_card(target, cx)
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.navigate_to(target, cx);
                }))
                .into_any_element(),
        )
    }

    pub(in crate::ui::app) fn render_search_result_page_card(
        &self,
        target: Page,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        dashboard_card_slot(
            section_landing_card(target, cx)
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, window, cx| {
                    clear_input(&app.inputs.dashboard_search, window, cx);
                    window.blur();
                    app.navigate_to(target, cx);
                    cx.notify();
                }))
                .into_any_element(),
        )
    }

    pub(in crate::ui::app) fn dashboard_search_query(&self, cx: &mut Context<Self>) -> String {
        self.inputs
            .dashboard_search
            .read(cx)
            .value()
            .trim()
            .to_string()
    }

    pub(in crate::ui::app) fn render_search_results_page(
        &self,
        search_query: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let search_results =
            dashboard_search_pages(search_query, self.settings.advanced.show_advanced_controls);
        let mut search_result_cards = h_flex()
            .w_full()
            .min_w(px(0.0))
            .items_start()
            .gap_2()
            .flex_wrap();

        if search_query.is_empty() {
            search_result_cards = search_result_cards.child(div().w_full().min_h(px(8.0)));
        } else if search_results.is_empty() {
            search_result_cards = search_result_cards.child(animated_presence_child(
                SharedString::from(format!("search-results-empty-{search_query}")),
                div()
                    .w_full()
                    .min_w(px(0.0))
                    .py_2()
                    .child(text_muted(t!("home.no_matching_functions").to_string()))
                    .into_any_element(),
            ));
        } else {
            for target in search_results {
                search_result_cards = search_result_cards.child(with_optional_motion(
                    self.render_search_result_page_card(target, cx),
                    SharedString::from(format!("search-result-card-{target:?}")),
                    MotionSpeed::Fast,
                    |card| card,
                    |card, delta| card.opacity(0.18 + 0.82 * delta),
                ));
            }
        }

        page_body_shell()
            .child(search_result_cards)
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_home_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let settings = &self.saved_settings;
        let mut section_cards = h_flex()
            .w_full()
            .min_w(px(0.0))
            .items_start()
            .gap_2()
            .flex_wrap();

        for section in
            dashboard_sections_in_nav_order(self.settings.advanced.show_advanced_controls)
        {
            section_cards =
                section_cards.child(self.render_home_page_page_card(section.landing_page, cx));
        }

        let summary = h_flex()
            .w_full()
            .min_w(px(0.0))
            .items_start()
            .gap_2()
            .flex_wrap()
            .child(dashboard_card_slot(
                self.render_cpu_usage_summary().into_any_element(),
            ))
            .child(dashboard_card_slot(
                self.render_memory_usage_summary().into_any_element(),
            ))
            .child(dashboard_card_slot(
                self.render_io_usage_summary().into_any_element(),
            ))
            .child(dashboard_card_slot(
                self.render_network_usage_summary().into_any_element(),
            ))
            .child(dashboard_card_slot(
                titled_status_list(
                    &t!("home.enabled_features"),
                    Some(if settings.general.enabled {
                        status_pill(
                            t!("home.master_switch_enabled").to_string(),
                            success_bg_color(),
                            success_text_color(),
                        )
                    } else {
                        status_pill(
                            t!("home.master_switch_disabled").to_string(),
                            warning_bg_color(),
                            warning_text_color(),
                        )
                    }),
                    self.dashboard_enabled_function_items(settings),
                    Some(t!("home.no_enabled_features").to_string()),
                )
                .into_any_element(),
            ));

        self.page_shell(Page::Home, cx)
            .child(section_title_text(t!("home.home").to_string()))
            .child(summary)
            .child(section_title_text(t!("home.main_sections").to_string()))
            .child(section_cards)
            .into_any_element()
    }

    pub(in crate::ui::app) fn dashboard_enabled_function_items(
        &self,
        settings: &Settings,
    ) -> Vec<(Option<Page>, String, String)> {
        let mut items = Vec::with_capacity(16);
        if settings.by_foreground.enabled {
            items.push((
                Some(Page::ByForeground),
                t!("nav.by_foreground").to_string(),
                rule_count_label(settings.by_foreground.rules.len()),
            ));
        }
        if settings.by_running_app.enabled {
            items.push((
                Some(Page::ByRunningApp),
                t!("nav.by_running_app").to_string(),
                self.by_running_app_status
                    .active_process
                    .clone()
                    .unwrap_or_else(|| rule_count_label(settings.by_running_app.rules.len())),
            ));
        }
        if settings.by_cpu_load.enabled {
            items.push((
                Some(Page::ByCpuLoad),
                t!("nav.by_cpu_load").to_string(),
                cpu_usage_label(self.cpu_usage.percent),
            ));
        }
        if settings.by_activity.enabled {
            items.push((
                Some(Page::ByActivity),
                t!("nav.by_activity").to_string(),
                activity_state_label(self.activity.state),
            ));
        }
        if settings.by_time.enabled {
            items.push((
                Some(Page::ByTime),
                t!("nav.by_time").to_string(),
                self.next_schedule.clone(),
            ));
        }
        if settings.core_limiter.enabled {
            items.push((
                Some(Page::CoreLimiter),
                t!("nav.core_limiter").to_string(),
                t!(
                    "home.limited_count",
                    count = self.core_limiter_status.limited_processes
                )
                .to_string(),
            ));
        }
        if settings.background_cpu_restriction.enabled {
            items.push((
                Some(Page::BackgroundCpuRestriction),
                t!("nav.background_cpu_restriction").to_string(),
                t!(
                    "home.adjusted_count",
                    count = self.background_cpu_restriction_status.adjusted_processes
                )
                .to_string(),
            ));
        }
        if settings.background_efficiency.enabled {
            items.push((
                Some(Page::BackgroundEfficiency),
                t!("nav.background_efficiency").to_string(),
                t!(
                    "home.throttled_count",
                    count = self.background_efficiency_status.throttled_processes
                )
                .to_string(),
            ));
        }
        if settings.app_suspension.enabled {
            items.push((
                Some(Page::AppSuspension),
                t!("nav.app_suspension").to_string(),
                t!(
                    "home.suspended_count",
                    count = self.app_suspension_status.suspended_processes
                )
                .to_string(),
            ));
        }
        if settings.workload_engine.enabled {
            let workload_engine_status = if self.workload_engine_status.launch_boost_active {
                t!("home.launch_boost").to_string()
            } else {
                t!(
                    "home.adjusted_count",
                    count = self.workload_engine_status.background_adjusted_processes
                )
                .to_string()
            };
            items.push((
                Some(Page::AdaptiveEngine),
                t!("nav.workload_engine").to_string(),
                workload_engine_status,
            ));
        }
        if settings.io_priority.enabled {
            items.push((
                Some(Page::IoPriority),
                t!("nav.io_priority").to_string(),
                t!(
                    "home.adjusted_count",
                    count = self.io_priority_status.adjusted_processes
                )
                .to_string(),
            ));
        }
        if settings.memory_priority.enabled {
            items.push((
                Some(Page::MemoryPriority),
                t!("nav.memory_priority").to_string(),
                t!(
                    "home.adjusted_count",
                    count = self.memory_priority_status.adjusted_processes
                )
                .to_string(),
            ));
        }
        if settings.memory_trim.enabled {
            items.push((
                Some(Page::MemoryTrim),
                t!("nav.memory_trim").to_string(),
                t!(
                    "home.trimmed_count",
                    count = self.memory_trim_status.trimmed_processes
                )
                .to_string(),
            ));
        }
        if settings.core_steering.enabled {
            items.push((
                Some(Page::CoreSteering),
                t!("nav.core_steering").to_string(),
                t!(
                    "home.adjusted_count",
                    count = self.core_steering_status.adjusted_processes
                )
                .to_string(),
            ));
        }

        items
    }

    pub(in crate::ui::app) fn render_cpu_usage_summary(&self) -> gpui::Div {
        let graph = self.render_cpu_history_graph("cpu", &self.cpu_usage_history);
        let body = v_flex()
            .w_full()
            .h_full()
            .min_w(px(0.0))
            .flex_1()
            .min_h(px(0.0))
            .gap_2()
            .justify_end()
            .child(dashboard_split_value_row([
                dashboard_split_value(
                    t!("home.cpu_load").to_string(),
                    cpu_usage_label(self.cpu_usage.percent),
                    dashboard_primary_series_color(),
                ),
                dashboard_split_value(
                    t!("home.cpu_frequency").to_string(),
                    cpu_frequency_label(self.cpu_usage.frequency_mhz),
                    dashboard_secondary_series_color(),
                ),
            ]))
            .child(graph);

        dashboard_summary_card(
            t!("home.by_cpu_load").to_string(),
            Some(
                dashboard_summary_header_value(cpu_usage_label(self.cpu_usage.percent))
                    .into_any_element(),
            ),
            body.into_any_element(),
        )
    }

    pub(in crate::ui::app) fn render_memory_usage_summary(&self) -> gpui::Div {
        let graph = self.render_memory_history_graph("memory", &self.memory_usage_history);
        let body = v_flex()
            .w_full()
            .h_full()
            .min_w(px(0.0))
            .flex_1()
            .min_h(px(0.0))
            .gap_2()
            .justify_end()
            .child(dashboard_split_value_row([
                dashboard_split_value(
                    t!("home.memory_used").to_string(),
                    memory_usage_value_label(self.memory_usage),
                    dashboard_primary_series_color(),
                ),
                dashboard_split_value(
                    t!("home.memory_cache").to_string(),
                    memory_cache_value_label(self.memory_usage),
                    dashboard_secondary_series_color(),
                ),
            ]))
            .child(graph);

        dashboard_summary_card(
            t!("home.memory_usage").to_string(),
            Some(
                dashboard_summary_header_value(memory_usage_label(self.memory_usage.percent))
                    .into_any_element(),
            ),
            body.into_any_element(),
        )
    }

    pub(in crate::ui::app) fn render_io_usage_summary(&self) -> gpui::Div {
        let graph = self.render_io_history_graph("io", &self.io_usage_history);

        dashboard_summary_card(
            t!("home.io_usage").to_string(),
            Some(
                dashboard_summary_header_value(io_usage_label(self.io_usage.bytes_per_second))
                    .into_any_element(),
            ),
            v_flex()
                .w_full()
                .h_full()
                .min_w(px(0.0))
                .flex_1()
                .min_h(px(0.0))
                .gap_2()
                .justify_end()
                .child(dashboard_split_value_row([
                    io_usage_split_value(
                        t!("home.io_read").to_string(),
                        self.io_usage.read_bytes_per_second,
                        dashboard_primary_series_color(),
                    ),
                    io_usage_split_value(
                        t!("home.io_write").to_string(),
                        self.io_usage.write_bytes_per_second,
                        dashboard_secondary_series_color(),
                    ),
                ]))
                .child(graph)
                .into_any_element(),
        )
    }

    pub(in crate::ui::app) fn render_network_usage_summary(&self) -> gpui::Div {
        let graph = self.render_network_history_graph("network", &self.network_usage_history);

        dashboard_summary_card(
            t!("home.network_usage").to_string(),
            Some(
                dashboard_summary_header_value(io_usage_label(self.network_usage.bytes_per_second))
                    .into_any_element(),
            ),
            v_flex()
                .w_full()
                .h_full()
                .min_w(px(0.0))
                .flex_1()
                .min_h(px(0.0))
                .gap_2()
                .justify_end()
                .child(dashboard_split_value_row([
                    io_usage_split_value(
                        t!("home.network_download").to_string(),
                        self.network_usage.download_bytes_per_second,
                        dashboard_primary_series_color(),
                    ),
                    io_usage_split_value(
                        t!("home.network_upload").to_string(),
                        self.network_usage.upload_bytes_per_second,
                        dashboard_secondary_series_color(),
                    ),
                ]))
                .child(graph)
                .into_any_element(),
        )
    }

    pub(in crate::ui::app) fn render_cpu_history_graph(
        &self,
        graph_id: &'static str,
        history: &VecDeque<CpuUsageHistorySample>,
    ) -> gpui::Div {
        self.render_dual_line_history_graph(
            graph_id,
            dashboard_cpu_dual_line_points(history, self.cpu_usage.base_frequency_mhz),
            dashboard_primary_series_color(),
            dashboard_secondary_series_color(),
            t!("home.cpu_load").to_string(),
            t!("home.cpu_frequency").to_string(),
            Some(DASHBOARD_PERCENT_CHART_MAX),
        )
    }

    pub(in crate::ui::app) fn render_memory_history_graph(
        &self,
        graph_id: &'static str,
        history: &VecDeque<MemoryUsageHistorySample>,
    ) -> gpui::Div {
        self.render_dual_line_history_graph(
            graph_id,
            dashboard_dual_line_points(
                history
                    .iter()
                    .map(|sample| (sample.usage_percent, sample.cache_percent)),
                memory_usage_label,
                memory_usage_label,
            ),
            dashboard_primary_series_color(),
            dashboard_secondary_series_color(),
            t!("home.memory_used").to_string(),
            t!("home.memory_cache").to_string(),
            Some(DASHBOARD_PERCENT_CHART_MAX),
        )
    }

    pub(in crate::ui::app) fn render_io_history_graph(
        &self,
        graph_id: &'static str,
        history: &VecDeque<IoUsageHistorySample>,
    ) -> gpui::Div {
        self.render_dual_line_history_graph(
            graph_id,
            dashboard_dual_line_points(
                history
                    .iter()
                    .map(|sample| (sample.read_bytes_per_second, sample.write_bytes_per_second)),
                |value| io_usage_label(value.map(f64::from)),
                |value| io_usage_label(value.map(f64::from)),
            ),
            dashboard_primary_series_color(),
            dashboard_secondary_series_color(),
            t!("home.io_read").to_string(),
            t!("home.io_write").to_string(),
            None,
        )
    }

    pub(in crate::ui::app) fn render_network_history_graph(
        &self,
        graph_id: &'static str,
        history: &VecDeque<NetworkUsageHistorySample>,
    ) -> gpui::Div {
        self.render_dual_line_history_graph(
            graph_id,
            dashboard_dual_line_points(
                history.iter().map(|sample| {
                    (
                        sample.download_bytes_per_second,
                        sample.upload_bytes_per_second,
                    )
                }),
                |value| io_usage_label(value.map(f64::from)),
                |value| io_usage_label(value.map(f64::from)),
            ),
            dashboard_primary_series_color(),
            dashboard_secondary_series_color(),
            t!("home.network_download").to_string(),
            t!("home.network_upload").to_string(),
            None,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "dashboard graph labels and scaling stay clearer at each call site"
    )]
    pub(in crate::ui::app) fn render_dual_line_history_graph(
        &self,
        graph_id: &'static str,
        data: Vec<DashboardDualLinePoint>,
        first_stroke: Hsla,
        second_stroke: Hsla,
        first_label: String,
        second_label: String,
        scale_max: Option<f64>,
    ) -> gpui::Div {
        let tooltips = dashboard_graph_sample_tooltips(&data, &first_label, &second_label);
        let mut chart = AreaChart::new(data)
            .x(|point: &DashboardDualLinePoint| point.tick.clone())
            .y(|point: &DashboardDualLinePoint| point.first_value)
            .stroke(first_stroke)
            .fill(gpui::transparent_black())
            .linear()
            .y(|point: &DashboardDualLinePoint| point.second_value)
            .stroke(second_stroke)
            .fill(gpui::transparent_black())
            .linear();

        if let Some(scale_max) = scale_max {
            chart = chart
                .y(move |_point: &DashboardDualLinePoint| scale_max)
                .stroke(gpui::transparent_black())
                .fill(gpui::transparent_black())
                .linear();
        }

        div()
            .w_full()
            .h(px(DASHBOARD_LINE_CHART_HEIGHT))
            .min_w(px(0.0))
            .px_1()
            .py_1()
            .relative()
            .child(chart.tick_margin(DASHBOARD_LINE_CHART_TICK_MARGIN))
            .child(dashboard_graph_hover_overlay(graph_id, tooltips))
    }
}
