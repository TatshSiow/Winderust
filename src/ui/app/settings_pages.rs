use super::*;

impl WinderustApp {
    pub(super) fn render_theme_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.settings.general.theme_mode;
        let selected_label = theme_mode_label(selected);
        let dropdown = self.render_dropdown_select(
            "theme-mode",
            selected_label,
            true,
            DropdownSelectWidth::Standard,
            AppThemeMode::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for mode in AppThemeMode::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("theme-mode-option-{mode:?}")),
                            theme_mode_label(mode),
                            selected == mode,
                            cx,
                        )
                        .on_click(cx.listener(
                            move |app, _, window, cx| {
                                app.settings.general.theme_mode = mode;
                                app.active_power_plan_picker = None;
                                apply_appearance_settings(&app.settings.general, window, cx);
                                cx.notify();
                            },
                        )),
                    );
                }
                options
            },
        );

        setting_action_card("theme-mode-card", t!("common.theme").to_string(), dropdown)
            .into_any_element()
    }

    pub(super) fn render_update_channel_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.settings.general.update_channel;
        self.render_dropdown_select(
            "update-channel",
            update_channel_label(selected),
            true,
            DropdownSelectWidth::Standard,
            UpdateChannel::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for channel in UpdateChannel::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("update-channel-option-{channel:?}")),
                            update_channel_label(channel),
                            selected == channel,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.general.update_channel = channel;
                            app.latest_version = None;
                            app.available_update = None;
                            app.update_check_message = None;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(super) fn render_language_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.settings.general.language;
        let dropdown = self.render_dropdown_select(
            "language",
            selected.native_label().to_string(),
            true,
            DropdownSelectWidth::Standard,
            AppLanguage::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for language in AppLanguage::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("language-option-{language:?}")),
                            language.native_label().to_string(),
                            selected == language,
                            cx,
                        )
                        .on_click(cx.listener(
                            move |app, _, window, cx| {
                                app.settings.general.language = language;
                                app.active_power_plan_picker = None;
                                apply_language(language);
                                app.inputs.refresh_localized_placeholders(window, cx);
                                cx.notify();
                            },
                        )),
                    );
                }
                options
            },
        );

        setting_action_card("language-card", t!("common.language").to_string(), dropdown)
            .into_any_element()
    }

    pub(super) fn render_animation_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.settings.general.animation_mode;
        let dropdown = self.render_dropdown_select(
            "animation-mode",
            animation_mode_label(selected),
            true,
            DropdownSelectWidth::Standard,
            AnimationMode::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for mode in AnimationMode::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("animation-mode-option-{mode:?}")),
                            animation_mode_label(mode),
                            selected == mode,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.general.animation_mode = mode;
                            app.active_power_plan_picker = None;
                            UI_ANIMATIONS_ENABLED
                                .store(resolve_animation_enabled(mode), Ordering::Relaxed);
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        setting_action_card(
            "animation-mode-card",
            t!("common.animation").to_string(),
            dropdown,
        )
        .into_any_element()
    }

    pub(super) fn sync_accent_color_picker(&self, window: &mut Window, cx: &mut Context<Self>) {
        let color = self.settings.general.accent.custom_color;
        self.accent_color_picker.update(cx, |picker, cx| {
            if picker.value().and_then(hsla_to_rgb_u32) != Some(color) {
                picker.set_value(rgb(color), window, cx);
            }
        });
    }

    pub(super) fn render_accent_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected_source = self.settings.general.accent.source;
        self.sync_accent_color_picker(window, cx);
        let accent_target = SettingGroupTarget::AccentColor;
        let collapsed = self.is_setting_group_collapsed(accent_target);
        let accent_motion_id = format!("setting-group-{accent_target:?}");
        let accent_motion_progress = expandable_motion_progress(&accent_motion_id);
        if accent_motion_progress.is_some() {
            window.request_animation_frame();
        }
        let source_dropdown = self.render_dropdown_select(
            "accent-source",
            accent_source_label(selected_source),
            true,
            DropdownSelectWidth::Standard,
            AccentColorSource::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for source in AccentColorSource::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("accent-source-option-{source:?}")),
                            accent_source_label(source),
                            selected_source == source,
                            cx,
                        )
                        .on_click(cx.listener(
                            move |app, _, window, cx| {
                                app.settings.general.accent.source = source;
                                if source == AccentColorSource::Custom {
                                    app.set_setting_group_expanded(
                                        SettingGroupTarget::AccentColor,
                                        true,
                                    );
                                } else {
                                    app.set_setting_group_expanded(
                                        SettingGroupTarget::AccentColor,
                                        false,
                                    );
                                }
                                app.active_power_plan_picker = None;
                                apply_appearance_settings(&app.settings.general, window, cx);
                                cx.notify();
                            },
                        )),
                    );
                }
                options
            },
        );
        let mut color_palette = v_flex().gap_2();
        let mut color_row = h_flex().gap_2();
        let mut color_row_len = 0;
        for color in ACCENT_PALETTE {
            let selected = self.settings.general.accent.source == AccentColorSource::Custom
                && self.settings.general.accent.custom_color == color;
            color_row = color_row.child(
                accent_swatch("accent-palette-swatch", color, selected).on_click(cx.listener(
                    move |app, _, window, cx| {
                        app.settings.general.accent.source = AccentColorSource::Custom;
                        app.settings.general.accent.custom_color = color;
                        app.set_setting_group_expanded(SettingGroupTarget::AccentColor, true);
                        apply_appearance_settings(&app.settings.general, window, cx);
                        cx.notify();
                    },
                )),
            );
            color_row_len += 1;
            if color_row_len == ACCENT_SWATCHES_PER_ROW {
                color_palette = color_palette.child(color_row);
                color_row = h_flex().gap_2();
                color_row_len = 0;
            }
        }
        if color_row_len > 0 {
            color_palette = color_palette.child(color_row);
        }

        let mut custom_picker = h_flex().w_full().min_w(px(0.0)).gap_2().flex_wrap();
        for color in self.settings.general.accent.custom_colors.iter().copied() {
            let selected = self.settings.general.accent.source == AccentColorSource::Custom
                && self.settings.general.accent.custom_color == color;
            let app_entity = cx.entity().clone();
            custom_picker = custom_picker.child(
                accent_swatch("accent-custom-swatch", color, selected)
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.settings.general.accent.source = AccentColorSource::Custom;
                        app.settings.general.accent.custom_color = color;
                        app.set_setting_group_expanded(SettingGroupTarget::AccentColor, true);
                        apply_appearance_settings(&app.settings.general, window, cx);
                        cx.notify();
                    }))
                    .context_menu(move |menu, _, _| {
                        let app_entity = app_entity.clone();
                        menu.item(PopupMenuItem::new("Delete").on_click(move |_, _, cx| {
                            app_entity.update(cx, |app, cx| {
                                remove_custom_accent_color(&mut app.settings.general.accent, color);
                                cx.notify();
                            });
                        }))
                        .item(PopupMenuItem::new("Cancel"))
                    }),
            );
        }

        custom_picker = custom_picker.child(
            div()
                .id("accent-custom-picker-card")
                .size(px(ACCENT_COLOR_PICKER_WRAPPER_SIZE))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(BRAND_RADIUS_CONTROL))
                .bg(rgb(panel_active_color()))
                .child(
                    ColorPicker::new(&self.accent_color_picker)
                        .featured_colors(accent_picker_featured_colors())
                        .icon(IconName::Palette)
                        .with_size(px(ACCENT_COLOR_PICKER_INNER_SIZE))
                        .into_any_element(),
                ),
        );

        let mut palette_content = v_flex().w_full().min_w(px(0.0)).gap_4();
        palette_content = palette_content.child(accent_color_group(
            t!("accent.custom").to_string(),
            custom_picker.into_any_element(),
        ));
        palette_content = palette_content.child(accent_color_group(
            t!("accent.color_palette").to_string(),
            color_palette.into_any_element(),
        ));
        let header_toggle_target = accent_target;
        let accent_hover_id = "accent-source-card".to_string();

        let mut accent_card = v_flex()
            .id("accent-color-card")
            .w_full()
            .min_w(px(0.0))
            .overflow_hidden()
            .rounded(px(BRAND_RADIUS_CONTROL))
            .bg(rgb(settings_card_color()))
            .text_color(rgb(primary_text_color()))
            .text_size(px(TEXT_BODY_SIZE))
            .line_height(px(TEXT_BODY_LINE_HEIGHT))
            .child(
                h_flex()
                    .id("accent-source-card")
                    .w_full()
                    .min_w(px(0.0))
                    .h(px(CARD_ROW_HEIGHT))
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .py_3()
                    .px_4()
                    .relative()
                    .overflow_hidden()
                    .block_mouse_except_scroll()
                    .cursor_pointer()
                    .capture_any_mouse_down(cx.listener(
                        |app, event: &gpui::MouseDownEvent, _, cx| {
                            handle_navigation_mouse_button(app, event.button, cx);
                        },
                    ))
                    .on_hover({
                        let accent_hover_id = accent_hover_id.clone();
                        move |hovered, _, cx| {
                            set_card_hovered(accent_hover_id.clone(), *hovered, cx);
                        }
                    })
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.toggle_setting_group(header_toggle_target, cx);
                    }))
                    .child(animated_card_hover_layer(&accent_hover_id))
                    .child(
                        div()
                            .id("accent-color-title")
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .child(t!("accent.source").to_string()),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .justify_end()
                            .gap_1()
                            .flex_shrink_0()
                            .child(source_dropdown)
                            .child(
                                div()
                                    .id("accent-color-chevron")
                                    .w(px(28.0))
                                    .h(px(24.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .flex_shrink_0()
                                    .rounded(px(BRAND_RADIUS_CONTROL))
                                    .text_color(rgb(dim_text_color()))
                                    .opacity(0.72)
                                    .hover(|style| style.opacity(1.0))
                                    .cursor_pointer()
                                    .child(collapsible_chevron_icon_with_progress(
                                        "accent-color",
                                        collapsed,
                                        accent_motion_progress,
                                    )),
                            ),
                    ),
            );

        if !collapsed || accent_motion_progress.is_some() {
            let palette = div()
                .id("accent-palette-subcard")
                .w_full()
                .min_w(px(0.0))
                .border_t_1()
                .border_color(rgb(border_color()))
                .py_3()
                .px_4()
                .child(palette_content)
                .into_any_element();
            accent_card = accent_card.child(if let Some(progress) = accent_motion_progress {
                expanded_child_at_progress(
                    palette,
                    Some(accent_palette_animation_height(
                        self.settings.general.accent.custom_colors.len(),
                    )),
                    progress,
                )
            } else {
                palette
            });
        }

        accent_card.into_any_element()
    }

    pub(super) fn render_winderust_behaviour_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.page_shell(Page::WinderustBehaviour, cx)
            .child(checkbox(
                "general-enabled",
                t!("settings.master_switch").to_string(),
                self.settings.general.enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "startup-windows",
                t!("settings.startup_windows").to_string(),
                self.settings.general.startup_with_windows,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.startup_with_windows = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "start-minimized",
                t!("settings.start_minimized").to_string(),
                self.settings.general.start_minimized,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.start_minimized = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "hide-to-tray",
                t!("settings.hide_to_tray").to_string(),
                self.settings.general.hide_to_tray,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.hide_to_tray = *checked;
                    cx.notify();
                }),
            ))
            .child(section_title_text(t!("settings.advanced").to_string()))
            .child(self.render_failure_suppression_threshold_setting(cx))
            .child(self.render_action_log_mode_selector(window, cx))
            .child(section_title_text(
                t!("settings.settings_files").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("export-settings")
                            .small()
                            .label(t!("settings.export_settings").to_string())
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.export_settings_toml();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("import-settings")
                            .small()
                            .label(t!("settings.import_settings").to_string())
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.import_settings_toml(window, cx);
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_language_and_appearance_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.page_shell(Page::LanguageAndAppearance, cx)
            .child(self.render_theme_selector(window, cx))
            .child(self.render_accent_selector(window, cx))
            .child(self.render_language_selector(window, cx))
            .child(self.render_animation_selector(window, cx))
            .into_any_element()
    }

    pub(super) fn render_experimental_features_page(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.page_shell(Page::ExperimentalFeatures, cx)
            .child(setting_action_card_with_help(
                "experimental-priority-values",
                t!("settings.expose_all_priority_values").to_string(),
                t!("settings.expose_all_priority_values_help").to_string(),
                setting_group_switch_action(
                    "experimental-priority-values-toggle",
                    self.settings.advanced.expose_all_priority_values,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.advanced.expose_all_priority_values = *checked;
                        if !*checked {
                            app.settings.process_priority.background_priority = app
                                .settings
                                .process_priority
                                .background_priority
                                .safe_when_advanced_disabled();
                            app.settings.process_priority.foreground_priority = app
                                .settings
                                .process_priority
                                .foreground_priority
                                .safe_when_advanced_disabled();
                            app.settings.thread_priority.background_priority = app
                                .settings
                                .thread_priority
                                .background_priority
                                .safe_when_advanced_disabled();
                            app.settings.thread_priority.foreground_priority = app
                                .settings
                                .thread_priority
                                .foreground_priority
                                .safe_when_advanced_disabled();
                            app.settings.io_priority.background_priority = app
                                .settings
                                .io_priority
                                .background_priority
                                .safe_when_advanced_disabled();
                            app.settings.io_priority.foreground_priority = app
                                .settings
                                .io_priority
                                .foreground_priority
                                .safe_when_advanced_disabled();
                            app.settings.gpu_priority.background_priority = app
                                .settings
                                .gpu_priority
                                .background_priority
                                .safe_when_advanced_disabled();
                            app.settings.gpu_priority.foreground_priority = app
                                .settings
                                .gpu_priority
                                .foreground_priority
                                .safe_when_advanced_disabled();
                            app.settings
                                .workload_engine
                                .workload_engine_io_priority
                                .background_priority = app
                                .settings
                                .workload_engine
                                .workload_engine_io_priority
                                .background_priority
                                .safe_when_advanced_disabled();
                            app.settings
                                .workload_engine
                                .workload_engine_io_priority
                                .foreground_priority = app
                                .settings
                                .workload_engine
                                .workload_engine_io_priority
                                .foreground_priority
                                .safe_when_advanced_disabled();
                            app.settings
                                .workload_engine
                                .workload_engine_thread_priority
                                .background_priority = app
                                .settings
                                .workload_engine
                                .workload_engine_thread_priority
                                .background_priority
                                .safe_when_advanced_disabled();
                            app.settings
                                .workload_engine
                                .workload_engine_thread_priority
                                .foreground_priority = app
                                .settings
                                .workload_engine
                                .workload_engine_thread_priority
                                .foreground_priority
                                .safe_when_advanced_disabled();
                            app.settings
                                .workload_engine
                                .workload_engine_gpu_priority
                                .background_priority = app
                                .settings
                                .workload_engine
                                .workload_engine_gpu_priority
                                .background_priority
                                .safe_when_advanced_disabled();
                            app.settings
                                .workload_engine
                                .workload_engine_gpu_priority
                                .foreground_priority = app
                                .settings
                                .workload_engine
                                .workload_engine_gpu_priority
                                .foreground_priority
                                .safe_when_advanced_disabled();
                            for rule in &mut app.settings.process_priority.exclusions {
                                let foreground = rule
                                    .process_priority_override(true)
                                    .safe_when_advanced_disabled();
                                let background = rule
                                    .process_priority_override(false)
                                    .safe_when_advanced_disabled();
                                rule.set_process_priority_override(true, foreground);
                                rule.set_process_priority_override(false, background);
                            }
                            for rule in &mut app.settings.thread_priority.exclusions {
                                let foreground = rule
                                    .thread_priority_override(true)
                                    .safe_when_advanced_disabled();
                                let background = rule
                                    .thread_priority_override(false)
                                    .safe_when_advanced_disabled();
                                rule.set_thread_priority_override(true, foreground);
                                rule.set_thread_priority_override(false, background);
                            }
                            for rule in &mut app.settings.io_priority.exclusions {
                                let foreground = rule
                                    .io_priority_override(true)
                                    .safe_when_advanced_disabled();
                                let background = rule
                                    .io_priority_override(false)
                                    .safe_when_advanced_disabled();
                                rule.set_io_priority_override(true, foreground);
                                rule.set_io_priority_override(false, background);
                            }
                            for rule in &mut app.settings.gpu_priority.exclusions {
                                let foreground = rule
                                    .gpu_priority_override(true)
                                    .safe_when_advanced_disabled();
                                let background = rule
                                    .gpu_priority_override(false)
                                    .safe_when_advanced_disabled();
                                rule.set_gpu_priority_override(true, foreground);
                                rule.set_gpu_priority_override(false, background);
                            }
                        }
                        cx.notify();
                    }),
                ),
            ))
            .child(setting_action_card_with_help(
                "experimental-advanced-controls",
                t!("settings.show_advanced_controls").to_string(),
                t!("settings.show_advanced_controls_help").to_string(),
                setting_group_switch_action(
                    "experimental-advanced-controls-toggle",
                    self.settings.advanced.show_advanced_controls,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.advanced.show_advanced_controls = *checked;
                        if !*checked && app.page.section_landing_page() == Page::AdvancedControls {
                            app.page = Page::ExperimentalFeatures;
                        }
                        cx.notify();
                    }),
                ),
            ))
            .into_any_element()
    }

    pub(super) fn render_action_log_mode_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.settings.advanced.action_log_mode;
        let dropdown = self.render_dropdown_select(
            "action-log-mode",
            action_log_mode_label(selected),
            true,
            DropdownSelectWidth::Standard,
            ActionLogMode::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for mode in ActionLogMode::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("action-log-mode-option-{mode:?}")),
                            action_log_mode_label(mode),
                            selected == mode,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.advanced.action_log_mode = mode;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        setting_action_card_with_help(
            "action-log-mode-card",
            t!("settings.action_log_mode").to_string(),
            action_log_mode_help(selected),
            dropdown,
        )
        .into_any_element()
    }

    pub(super) fn render_failure_suppression_threshold_setting(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let threshold = self
            .settings
            .advanced
            .execution_failure_suppression_threshold();
        setting_action_card_with_help(
            "execution-failure-suppression-threshold",
            t!("settings.failure_suppression_threshold").to_string(),
            t!("settings.failure_suppression_threshold_help").to_string(),
            self.render_numeric_value(
                NumericField::ExecutionFailureSuppressionThreshold,
                threshold.to_string(),
                threshold.to_string(),
                cx,
            ),
        )
        .into_any_element()
    }
}
