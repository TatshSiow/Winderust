use super::*;

impl WinderustApp {
    pub(super) fn render_about_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let github_url = "https://github.com/TatshSiow/Winderust";
        let discord_url = "https://discord.gg/M7nctFZUxX";
        let documentation_url = "https://github.com/TatshSiow/Winderust#readme";
        let license_url = "https://github.com/TatshSiow/Winderust/blob/main/LICENSE";
        let latest_version = self
            .latest_version
            .as_deref()
            .map(|version| format!("v{version}"))
            .unwrap_or_else(|| "—".to_string());
        let current_status = self.latest_version.as_ref().map(|_| {
            if self.available_update.is_some() {
                t!("about.old").to_string()
            } else {
                t!("about.up_to_date").to_string()
            }
        });

        self.page_shell(Page::About, cx)
            .child(section_title_text(t!("nav.about").to_string()))
            .child(
                branded_panel()
                    .p_4()
                    .gap_3()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_3()
                            .child(img("image/icon-design.png").size(px(64.0)))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .gap_1()
                                    .child(section_title_text(t!("app.name").to_string()))
                                    .child(text_muted(t!("app.description").to_string())),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .flex_wrap()
                            .text_size(px(TEXT_BODY_SIZE))
                            .line_height(px(TEXT_BODY_LINE_HEIGHT))
                            .text_color(rgb(dim_text_color()))
                            .child("Inspired by ")
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("Wanderlust"),
                            )
                            .child(" and ")
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("Windows Derust"),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(text_muted(t!("about.author").to_string()))
                            .child("Tatsh Siow"),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("about-github")
                                    .small()
                                    .label(t!("about.github").to_string())
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.open_url(github_url);
                                    })),
                            )
                            .child(text_muted("·"))
                            .child(
                                Button::new("about-discord")
                                    .small()
                                    .label(t!("about.discord").to_string())
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.open_url(discord_url);
                                    })),
                            )
                            .child(text_muted("·"))
                            .child(
                                Button::new("about-documentation")
                                    .small()
                                    .label(t!("about.documentation").to_string())
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.open_url(documentation_url);
                                    })),
                            )
                            .child(text_muted("·"))
                            .child(
                                Button::new("about-license")
                                    .small()
                                    .label(t!("about.license").to_string())
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.open_url(license_url);
                                    })),
                            ),
                    ),
            )
            .child(section_title_text(t!("about.updates").to_string()))
            .child(
                branded_panel()
                    .p_4()
                    .gap_3()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_3()
                            .child(div().flex_1().child(t!("about.update_channel").to_string()))
                            .child(self.render_update_channel_selector(window, cx))
                            .child(
                                control_button(Button::new("check-for-updates-now"))
                                    .label(t!("about.check_for_updates").to_string())
                                    .disabled(self.update_check_in_progress)
                                    .on_click(cx.listener(|app, _, _, cx| {
                                        app.check_for_updates(true, cx);
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(t!("about.latest_version").to_string())
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(text_muted(latest_version))
                                    .when(self.latest_version.is_some(), |row| {
                                        if let Some(update) = self.available_update.clone() {
                                            let url = update.url;
                                            row.child(
                                                primary_control_button(
                                                    Button::new("download-update"),
                                                    cx,
                                                )
                                                .label(t!("about.download_update").to_string())
                                                .on_click(cx.listener(move |_, _, _, cx| {
                                                    cx.open_url(&url);
                                                })),
                                            )
                                        } else {
                                            row.child(text_muted(format!(
                                                "({})",
                                                t!("about.up_to_date")
                                            )))
                                        }
                                    })
                                    .when_some(
                                        self.update_check_message.clone(),
                                        |row, message| {
                                            row.child(text_muted(format!("({message})")))
                                        },
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .child(t!("about.current_version").to_string())
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(text_muted(format!("v{}", env!("CARGO_PKG_VERSION"))))
                                    .when_some(current_status, |row, status| {
                                        row.child(text_muted(format!("({status})")))
                                    }),
                            ),
                    )
                    .child(checkbox(
                        "check-for-updates",
                        t!("about.automatic_check_for_updates").to_string(),
                        self.settings.general.check_for_updates,
                        cx.listener(|app, checked, _, cx| {
                            app.settings.general.check_for_updates = *checked;
                            cx.notify();
                        }),
                    )),
            )
            .into_any_element()
    }
}
