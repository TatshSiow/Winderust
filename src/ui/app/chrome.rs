use super::*;

impl WinderustApp {
    pub(super) fn render_title_bar(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = h_flex()
            .flex_none()
            .items_center()
            .gap_2()
            .when_some(self.app_icon.as_ref(), |title, icon| {
                title.child(img(Arc::clone(icon)).size(px(18.0)))
            })
            .child(
                div()
                    .font_family(FONT_BRAND)
                    .text_size(px(TEXT_CONTROL_SIZE))
                    .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(cx.theme().foreground)
                    .child(t!("app.name").to_string()),
            );

        h_flex()
            .id("winderust-title-bar")
            .window_control_area(WindowControlArea::Drag)
            .flex_none()
            .w_full()
            .h(px(TITLE_BAR_HEIGHT))
            .items_center()
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .bg(cx.theme().title_bar)
            .child(
                h_flex()
                    .h_full()
                    .flex_1()
                    .min_w(px(0.0))
                    .items_center()
                    .gap_2()
                    .px_3()
                    .overflow_hidden()
                    .child(title)
                    .child(
                        div()
                            .text_size(px(TEXT_LABEL_SIZE))
                            .line_height(px(TEXT_LABEL_LINE_HEIGHT))
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("app.description").to_string()),
                    ),
            )
            .child(
                h_flex()
                    .h_full()
                    .flex_1()
                    .min_w(px(TITLE_BAR_CONTROL_WIDTH * 3.0))
                    .items_center()
                    .justify_end()
                    .child(title_bar_controls(window, cx)),
            )
            .into_any_element()
    }

    pub(super) fn render_sidebar_search(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let search_focused = self
            .inputs
            .dashboard_search
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);

        div()
            .id("sidebar-search")
            .occlude()
            .w_full()
            .h(px(CARD_ROW_HEIGHT))
            .min_w(px(0.0))
            .flex()
            .items_center()
            .on_mouse_down_out(cx.listener(|_, _: &gpui::MouseDownEvent, window, cx| {
                window.blur();
                cx.notify();
            }))
            .child(app_input(&self.inputs.dashboard_search, search_focused, cx))
            .into_any_element()
    }

    pub(super) fn render_navigation(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut nav = v_flex()
            .w(px(NAV_PANE_WIDTH))
            .min_w(px(NAV_PANE_WIDTH))
            .h_full()
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .bg(cx.theme().sidebar);

        let drawer = v_flex().flex_1().min_h(px(0.0)).overflow_y_scrollbar();
        let mut drawer_items = v_flex()
            .gap_1()
            .p_3()
            .child(self.render_sidebar_search(window, cx));
        let mut footer = v_flex()
            .flex_shrink_0()
            .gap_1()
            .p_3()
            .border_t_1()
            .border_color(cx.theme().sidebar_border);

        for section in Page::sections() {
            if !self.nav_section_visible(section.landing_page) {
                continue;
            }
            let page = section.landing_page;
            let selected = self.page.section_landing_page() == page;
            let target = page;
            let row = nav_row(page, selected, cx)
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.navigate_to(target, cx);
                }))
                .into_any_element();

            if nav_section_in_footer(section.landing_page) {
                footer = footer.child(row);
            } else {
                drawer_items = drawer_items.child(row);
            }
        }

        nav = nav.child(drawer.child(drawer_items)).child(footer);
        nav.into_any_element()
    }

    pub(super) fn nav_section_visible(&self, page: Page) -> bool {
        page != Page::AdvancedControls || self.settings.advanced.show_advanced_controls
    }

    pub(super) fn render_unsaved_popup(
        &self,
        vanish_progress: Option<f32>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let popup = v_flex()
            .absolute()
            .right(px(24.0))
            .bottom(px(54.0))
            .w(px(372.0))
            .occlude()
            .on_any_mouse_down(|_, _, cx| {
                cx.stop_propagation();
            })
            .gap_2()
            .p_3()
            .rounded(px(BRAND_RADIUS_OVERLAY))
            .border_1()
            .border_color(rgb(accent_color()))
            .bg(cx.theme().popover)
            .child(
                h_flex().items_center().child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(cx.theme().popover_foreground)
                        .child(t!("unsaved.title").to_string()),
                ),
            )
            .child(text_muted(t!("unsaved.message").to_string()))
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("discard-settings")
                            .small()
                            .label(t!("common.discard").to_string())
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.cancel_settings_changes(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("save-settings")
                            .small()
                            .primary()
                            .label(t!("common.save").to_string())
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.sync_input_values(cx);
                                let had_unsaved_changes = app.settings != app.saved_settings;
                                if app.save_settings() && had_unsaved_changes {
                                    app.start_unsaved_popup_vanish();
                                }
                                cx.notify();
                            })),
                    ),
            );

        if let Some(progress) = vanish_progress {
            let progress = progress.clamp(0.0, 1.0);
            return popup
                .block_mouse_except_scroll()
                .cursor_default()
                .bottom(px(54.0 - 8.0 * progress))
                .opacity(1.0 - progress)
                .into_any_element();
        }

        with_optional_motion(
            popup,
            "unsaved-popup",
            MotionSpeed::Standard,
            |popup| popup,
            |popup, delta| {
                popup
                    .bottom(px(46.0 + 8.0 * delta))
                    .opacity(0.18 + 0.82 * delta)
            },
        )
    }

    pub(super) fn render_admin_rights_prompt(
        &self,
        bottom: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let popup = v_flex()
            .absolute()
            .right(px(24.0))
            .bottom(px(bottom))
            .w(px(420.0))
            .occlude()
            .on_any_mouse_down(|_, _, cx| {
                cx.stop_propagation();
            })
            .gap_2()
            .p_3()
            .rounded(px(BRAND_RADIUS_OVERLAY))
            .border_1()
            .border_color(rgb(warning_text_color()))
            .bg(rgb(warning_bg_color()))
            .text_color(rgb(primary_text_color()))
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(warning_text_color()))
                    .child(t!("admin_rights.title").to_string()),
            )
            .child(
                div()
                    .text_size(px(TEXT_BODY_SIZE))
                    .line_height(px(TEXT_BODY_LINE_HEIGHT))
                    .child(t!("admin_rights.message").to_string()),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("ignore-admin-rights")
                            .small()
                            .label(t!("admin_rights.ignore").to_string())
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.admin_rights_prompt_visible = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("relaunch-admin-rights")
                            .small()
                            .primary()
                            .label(t!("admin_rights.relaunch").to_string())
                            .on_click(cx.listener(|app, _, _, cx| {
                                if privilege::relaunch_as_admin() {
                                    cx.quit();
                                } else {
                                    app.status_message =
                                        t!("status.admin_relaunch_failed").to_string();
                                    cx.notify();
                                }
                            })),
                    ),
            );

        with_optional_motion(
            popup,
            "admin-rights-prompt",
            MotionSpeed::Standard,
            |popup| popup,
            move |popup, delta| {
                popup
                    .bottom(px(bottom - 8.0 + 8.0 * delta))
                    .opacity(0.18 + 0.82 * delta)
            },
        )
    }

    pub(super) fn render_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.page {
            Page::Home => self.render_home_page(cx),
            Page::PowerPlanControl => self.render_section_landing_page(Page::PowerPlanControl, cx),
            Page::WinderustFeatures => {
                self.render_section_landing_page(Page::WinderustFeatures, cx)
            }
            Page::CpuControl => self.render_section_landing_page(Page::CpuControl, cx),
            Page::PriorityControl => self.render_section_landing_page(Page::PriorityControl, cx),
            Page::SettingsHome => self.render_section_landing_page(Page::SettingsHome, cx),
            Page::AdvancedControls => self.render_section_landing_page(Page::AdvancedControls, cx),
            Page::ByActivity => self.render_by_activity_page(window, cx),
            Page::ByForeground => self.render_by_foreground_page(window, cx),
            Page::ByTime => self.render_by_time_page(window, cx),
            Page::ByCpuLoad => self.render_by_cpu_load_page(window, cx),
            Page::AdvancedPowerPlanTuning => {
                self.render_advanced_power_plan_tuning_page(window, cx)
            }
            Page::ProcessPriority => self.render_process_priority_page(window, cx),
            Page::ThreadPriority => self.render_thread_priority_page(window, cx),
            Page::DynamicPriorityBoost => self.render_dynamic_priority_boost_page(window, cx),
            Page::CoreLimiter => self.render_core_limiter_page(window, cx),
            Page::BackgroundCpuRestriction => {
                self.render_background_cpu_restriction_page(window, cx)
            }
            Page::AdaptiveEngine => self.render_adaptive_engine_page(window, cx),
            Page::BackgroundEfficiency => self.render_background_efficiency_page(window, cx),
            Page::AppSuspension => self.render_app_suspension_page(window, cx),
            Page::ByRunningApp => self.render_by_running_app_page(window, cx),
            Page::ProcessList => self.render_process_list_page(window, cx),
            Page::IoPriority => self.render_io_priority_page(window, cx),
            Page::GpuPriority => self.render_gpu_priority_page(window, cx),
            Page::MemoryPriority => self.render_memory_priority_page(window, cx),
            Page::MemoryTrim => self.render_memory_trim_page(window, cx),
            Page::CoreSteering => self.render_core_steering_page(window, cx),
            Page::ActionLog => self.render_action_log_page(window, cx),
            Page::WinderustBehaviour => self.render_winderust_behaviour_page(window, cx),
            Page::LanguageAndAppearance => self.render_language_and_appearance_page(window, cx),
            Page::ExperimentalFeatures => self.render_experimental_features_page(window, cx),
            Page::TimerResolution => self.render_timer_resolution_page(window, cx),
            Page::Win32PrioritySeparation => self.render_win32_priority_separation_page(window, cx),
            Page::About => self.render_about_page(window, cx),
        }
    }
}
