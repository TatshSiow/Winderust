use super::design;
use super::widgets::{self, checkbox, Choice};
use super::widgets::{button, pick_list, slider, text_input};
use crate::{config::*, ui::Page};
use iced::widget::{column, row, scrollable, text};
use iced::{Color, Element, Fill, Theme};
use rust_i18n::t;

#[derive(Default)]
pub(super) struct Editor {
    color: Option<String>,
    failure_threshold: Option<String>,
    accent_collapsed: bool,
    custom_color_open: bool,
    pub(super) checking: bool,
    notify_on_update: bool,
    pub(super) latest: Option<String>,
    pub(super) download: Option<String>,
    pub(super) update_error: Option<String>,
    pub(super) show_update: bool,
}
#[derive(Debug, Clone, Copy)]
pub(super) enum Flag {
    Enabled,
    Startup,
    Minimized,
    Tray,
    CrossSession,
    PauseDashboard,
    PauseProcesses,
    FeatureCounts,
    CardStatus,
    AdvancedValues,
    AdvancedControls,
    UpdateCheck,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Flag(Flag, bool),
    Language(AppLanguage),
    Theme(AppThemeMode),
    AccentSource(AccentColorSource),
    AccentHex(String),
    ColorChannel(u32, u8),
    ToggleAccent,
    ToggleCustomColor,
    Accent(u32),
    SaveColor,
    RemoveColor(usize),
    FailureThreshold(String),
    LogMode(ActionLogMode),
    Channel(UpdateChannel),
    Check,
    CheckStartup,
    Checked(UpdateChannel, Result<(String, Option<String>), String>),
    Open(String),
    Export,
    Import,
    DismissUpdate,
}
impl Editor {
    pub(super) fn reset_drafts(&mut self) {
        self.color = None;
        self.failure_threshold = None;
        self.latest = None;
        self.download = None;
        self.update_error = None;
        self.show_update = false;
    }

    pub(super) fn begin_check(&mut self, _channel: UpdateChannel, automatic: bool) -> bool {
        if self.checking {
            return false;
        }
        self.checking = true;
        self.notify_on_update = automatic;
        self.update_error = None;
        true
    }

    pub(super) fn update(&mut self, s: &mut Settings, m: Message) {
        match m {
            Message::Flag(flag, value) => match flag {
                Flag::Enabled => s.general.enabled = value,
                Flag::Startup => s.general.startup_with_windows = value,
                Flag::Minimized => s.general.start_minimized = value,
                Flag::Tray => s.general.hide_to_tray = value,
                Flag::CrossSession => s.general.allow_cross_session_process_control = value,
                Flag::PauseDashboard => s.advanced.pause_dashboard_metrics = value,
                Flag::PauseProcesses => s.advanced.pause_process_population = value,
                Flag::FeatureCounts => s.general.show_enabled_feature_counts_in_sidebar = value,
                Flag::CardStatus => s.general.show_feature_status_on_cards = value,
                Flag::AdvancedControls => s.advanced.show_advanced_controls = value,
                Flag::AdvancedValues => {
                    s.advanced.expose_all_priority_values = value;
                    if !value {
                        sanitize_advanced(s);
                    }
                }
                Flag::UpdateCheck => s.general.check_for_updates = value,
            },
            Message::Language(value) => {
                s.general.language = value;
                rust_i18n::set_locale(value.locale());
            }
            Message::Theme(value) => s.general.theme_mode = value,
            Message::AccentSource(value) => {
                s.general.accent.source = value;
                self.accent_collapsed = value == AccentColorSource::Windows;
            }
            Message::ToggleCustomColor => self.custom_color_open = !self.custom_color_open,
            Message::ToggleAccent => self.accent_collapsed = !self.accent_collapsed,
            Message::ColorChannel(shift, value) => {
                if [0, 8, 16].contains(&shift) {
                    s.general.accent.custom_color = (s.general.accent.custom_color
                        & !(255 << shift))
                        | (u32::from(value) << shift);
                    s.general.accent.source = AccentColorSource::Custom;
                    self.color = None;
                }
            }
            Message::AccentHex(value) => {
                if let Some(color) = parse_color(&value) {
                    s.general.accent.custom_color = color;
                    s.general.accent.source = AccentColorSource::Custom;
                }
                self.color = Some(value);
            }
            Message::Accent(value) => {
                s.general.accent.custom_color = value;
                s.general.accent.source = AccentColorSource::Custom;
                self.color = None;
            }
            Message::SaveColor => {
                let color = s.general.accent.custom_color;
                if self
                    .color
                    .as_deref()
                    .is_none_or(|v| parse_color(v).is_some())
                {
                    s.general
                        .accent
                        .custom_colors
                        .retain(|stored| *stored != color);
                    s.general.accent.custom_colors.push(color);
                }
            }
            Message::RemoveColor(index) => {
                if index < s.general.accent.custom_colors.len() {
                    s.general.accent.custom_colors.remove(index);
                }
            }
            Message::FailureThreshold(value) => {
                self.failure_threshold = Some(value.clone());
                if let Ok(value) = value.parse::<u8>() {
                    if (1..=100).contains(&value) {
                        s.advanced.execution_failure_suppression_threshold = value;
                    }
                }
            }
            Message::LogMode(value) => s.advanced.action_log_mode = value,
            Message::Channel(value) => {
                s.general.update_channel = value;
                self.latest = None;
                self.download = None;
                self.update_error = None;
                self.show_update = false;
            }
            Message::Check | Message::CheckStartup => {}
            Message::Checked(channel, result) => {
                if channel != s.general.update_channel {
                    self.checking = false;
                    return;
                }
                self.checking = false;
                match result {
                    Ok((version, url)) => {
                        self.latest = Some(version);
                        self.show_update = self.notify_on_update && url.is_some();
                        self.download = url;
                        self.update_error = None;
                    }
                    Err(error) => self.update_error = Some(error),
                }
            }
            Message::DismissUpdate => self.show_update = false,
            Message::Open(_) | Message::Export | Message::Import => {}
        }
    }
    pub(super) fn view<'a>(&'a self, page: Page, s: &'a Settings) -> Element<'a, Message> {
        let flag = |key: &str, value, flag| {
            widgets::settings_card(widgets::setting_row(
                key,
                widgets::switch(value, Some(move |value| Message::Flag(flag, value))),
            ))
        };
        let body = match page {
            Page::WinderustBehaviour => {
                let check = |key: &str, value, flag| {
                    iced::widget::container(
                        checkbox(value)
                            .label(t!(key).to_string())
                            .on_toggle(move |value| Message::Flag(flag, value)),
                    )
                    .height(36)
                    .center_y(36)
                };
                column![
                    column![
                        check("settings.master_switch", s.general.enabled, Flag::Enabled),
                        check(
                            "settings.startup_windows",
                            s.general.startup_with_windows,
                            Flag::Startup
                        ),
                        check(
                            "settings.start_minimized",
                            s.general.start_minimized,
                            Flag::Minimized
                        ),
                        check("settings.hide_to_tray", s.general.hide_to_tray, Flag::Tray),
                    ],
                    widgets::heading(
                        t!("settings.advanced").to_string(),
                        design::typography::BODY
                    ),
                    flag(
                        "settings.allow_cross_session_process_control",
                        s.general.allow_cross_session_process_control,
                        Flag::CrossSession
                    ),
                    flag(
                        "settings.pause_dashboard_metrics",
                        s.advanced.pause_dashboard_metrics,
                        Flag::PauseDashboard
                    ),
                    flag(
                        "settings.pause_process_population",
                        s.advanced.pause_process_population,
                        Flag::PauseProcesses
                    ),
                    widgets::settings_card(widgets::setting_row(
                        "settings.failure_suppression_threshold",
                        text_input(
                            "",
                            self.failure_threshold.as_deref().unwrap_or(
                                &s.advanced
                                    .execution_failure_suppression_threshold
                                    .to_string()
                            )
                        )
                        .on_input(Message::FailureThreshold)
                        .align_x(iced::Center)
                        .width(design::STANDALONE_NUMERIC_WIDTH),
                    )),
                    widgets::settings_card(widgets::setting_row(
                        "settings.action_log_mode",
                        pick_list(
                            ActionLogMode::ALL
                                .into_iter()
                                .map(|value| Choice(value, log_label(value)))
                                .collect::<Vec<_>>(),
                            Some(Choice(
                                s.advanced.action_log_mode,
                                log_label(s.advanced.action_log_mode)
                            )),
                            |value| Message::LogMode(value.0),
                        )
                        .width(design::SELECT_WIDTH),
                    )),
                    widgets::heading(
                        t!("settings.settings_files").to_string(),
                        design::typography::BODY
                    ),
                    row![
                        button(text(t!("settings.export_settings").to_string()))
                            .on_press(Message::Export),
                        button(text(t!("settings.import_settings").to_string()))
                            .on_press(Message::Import),
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center),
                ]
                .spacing(super::widgets::CARD_GAP)
            }
            Page::LanguageAndAppearance => {
                let colors = column(ACCENT_PALETTE.chunks(8).map(|chunk| {
                    row(chunk.iter().map(|color| {
                        color_button(
                            *color,
                            s.general.accent.source == AccentColorSource::Custom
                                && s.general.accent.custom_color == *color,
                        )
                        .into()
                    }))
                    .spacing(design::space::CONTROL)
                    .into()
                }))
                .spacing(design::space::CONTROL);
                let saved = column(s.general.accent.custom_colors.chunks(8).enumerate().map(
                    |(chunk_index, chunk)| {
                        row(chunk.iter().enumerate().map(|(i, color)| {
                            column![
                                color_button(
                                    *color,
                                    s.general.accent.source == AccentColorSource::Custom
                                        && s.general.accent.custom_color == *color
                                ),
                                button(text(t!("common.remove").to_string()))
                                    .on_press(Message::RemoveColor(chunk_index * 8 + i))
                            ]
                            .spacing(design::space::TIGHT)
                            .into()
                        }))
                        .spacing(design::space::CONTROL)
                        .into()
                    },
                ))
                .spacing(design::space::CONTROL);
                let mut custom = column![
                    text(t!("accent.custom").to_string()),
                    button(super::navigation::glyph("icons/settings.svg"))
                        .style(widgets::quiet)
                        .on_press(Message::ToggleCustomColor),
                    text(t!("accent.color_palette").to_string()),
                    colors,
                    saved
                ]
                .spacing(design::space::COMPACT);
                if self.custom_color_open {
                    for (label, shift) in [("R", 16), ("G", 8), ("B", 0)] {
                        let value = ((s.general.accent.custom_color >> shift) & 255) as u8;
                        custom = custom.push(
                            row![
                                text(label).width(20),
                                slider(0..=255, value, move |value| Message::ColorChannel(
                                    shift, value
                                )),
                                text(value.to_string()).width(32)
                            ]
                            .spacing(design::space::SMALL),
                        );
                    }
                    custom = custom.push(
                        row![
                            text_input(
                                "#RRGGBB",
                                &self.color.clone().unwrap_or_else(|| format!(
                                    "#{:06X}",
                                    s.general.accent.custom_color
                                ))
                            )
                            .on_input(Message::AccentHex)
                            .width(130),
                            button(text(t!("common.save").to_string()))
                                .style(crate::ui::widgets::primary_button)
                                .on_press_maybe(
                                    self.color
                                        .as_deref()
                                        .is_none_or(|v| parse_color(v).is_some())
                                        .then_some(Message::SaveColor)
                                )
                        ]
                        .spacing(design::space::SMALL),
                    );
                }
                column![
                    super::widgets::settings_card(
                        row![
                            text(t!("common.language").to_string()).width(Fill),
                            pick_list(
                                AppLanguage::ALL
                                    .into_iter()
                                    .map(|v| Choice(v, v.native_label().to_owned()))
                                    .collect::<Vec<_>>(),
                                Some(Choice(
                                    s.general.language,
                                    s.general.language.native_label().to_owned()
                                )),
                                |v| Message::Language(v.0)
                            )
                            .width(design::SELECT_WIDTH)
                        ]
                        .spacing(design::space::MEDIUM)
                        .align_y(iced::Center)
                    ),
                    super::widgets::setting_group(
                        "accent.source".to_string(),
                        !self.accent_collapsed,
                        Message::ToggleAccent,
                        pick_list(
                            vec![
                                Choice(
                                    AccentColorSource::Windows,
                                    t!("accent.windows").to_string()
                                ),
                                Choice(AccentColorSource::Custom, t!("accent.custom").to_string())
                            ],
                            Some(Choice(
                                s.general.accent.source,
                                if s.general.accent.source == AccentColorSource::Windows {
                                    t!("accent.windows").to_string()
                                } else {
                                    t!("accent.custom").to_string()
                                }
                            )),
                            |v| Message::AccentSource(v.0)
                        )
                        .width(design::SELECT_WIDTH),
                        custom
                    ),
                    super::widgets::settings_card(
                        row![
                            text(t!("common.theme").to_string()).width(Fill),
                            pick_list(
                                AppThemeMode::ALL
                                    .into_iter()
                                    .map(|v| Choice(v, theme_label(v)))
                                    .collect::<Vec<_>>(),
                                Some(Choice(
                                    s.general.theme_mode,
                                    theme_label(s.general.theme_mode)
                                )),
                                |v| Message::Theme(v.0)
                            )
                            .width(design::SELECT_WIDTH)
                        ]
                        .spacing(design::space::MEDIUM)
                        .align_y(iced::Center)
                    ),
                    flag(
                        "settings.show_enabled_feature_counts_in_sidebar",
                        s.general.show_enabled_feature_counts_in_sidebar,
                        Flag::FeatureCounts
                    ),
                    flag(
                        "settings.show_feature_status_on_cards",
                        s.general.show_feature_status_on_cards,
                        Flag::CardStatus
                    ),
                ]
                .spacing(super::widgets::CARD_GAP)
            }
            Page::ExperimentalFeatures => column![
                flag(
                    "settings.expose_all_priority_values",
                    s.advanced.expose_all_priority_values,
                    Flag::AdvancedValues
                ),
                text(t!("settings.expose_all_priority_values_help").to_string())
                    .width(Fill)
                    .style(text::secondary),
                flag(
                    "settings.show_advanced_controls",
                    s.advanced.show_advanced_controls,
                    Flag::AdvancedControls
                ),
                text(t!("settings.show_advanced_controls_help").to_string())
                    .width(Fill)
                    .style(text::secondary)
            ]
            .spacing(super::widgets::CARD_GAP),
            Page::About => {
                let mut links = row![].spacing(design::space::SMALL);
                for (key, url) in [
                    ("about.github", "https://github.com/TatshSiow/Winderust"),
                    ("about.discord", "https://discord.gg/M7nctFZUxX"),
                    (
                        "about.documentation",
                        "https://github.com/TatshSiow/Winderust#readme",
                    ),
                    (
                        "about.license",
                        "https://github.com/TatshSiow/Winderust/blob/main/LICENSE",
                    ),
                ] {
                    links = links.push(
                        button(text(t!(key).to_string())).on_press(Message::Open(url.into())),
                    );
                }
                let identity = widgets::settings_card(
                    column![
                        row![
                            iced::widget::image(logo()).width(52).height(52),
                            column![
                                widgets::heading(
                                    t!("app.name").to_string(),
                                    design::typography::BODY
                                ),
                                text(t!("app.description").to_string())
                            ]
                            .spacing(design::space::SMALL)
                        ]
                        .spacing(design::space::LARGE)
                        .align_y(iced::Center),
                        text(format!(
                            "{} Wanderlust {} Windows Derust",
                            t!("about.inspired_by"),
                            t!("about.inspiration_joiner")
                        )),
                        text("Copyright (C) 2026 Tatsh Siow / GPL-3.0-only"),
                        links
                    ]
                    .spacing(design::space::LARGE),
                );
                let updates = widgets::settings_card(
                    column![
                        widgets::setting_row(
                            "about.update_channel",
                            row![
                                pick_list(
                                    UpdateChannel::ALL
                                        .into_iter()
                                        .map(|v| Choice(v, channel_label(v)))
                                        .collect::<Vec<_>>(),
                                    Some(Choice(
                                        s.general.update_channel,
                                        channel_label(s.general.update_channel)
                                    )),
                                    |v| Message::Channel(v.0)
                                )
                                .width(design::SELECT_WIDTH),
                                button(text(t!("about.check_for_updates").to_string()))
                                    .on_press_maybe((!self.checking).then_some(Message::Check))
                            ]
                            .spacing(design::space::MEDIUM)
                        ),
                        widgets::setting_row(
                            "about.latest_version",
                            text(self.latest.as_deref().unwrap_or("-"))
                        ),
                        widgets::setting_row(
                            "about.current_version",
                            text(env!("CARGO_PKG_VERSION"))
                        ),
                        widgets::setting_row(
                            "about.automatic_check_for_updates_on_startup",
                            widgets::switch(
                                s.general.check_for_updates,
                                Some(|v| Message::Flag(Flag::UpdateCheck, v))
                            )
                        )
                    ]
                    .spacing(design::space::SMALL),
                );
                let mut body = column![
                    widgets::heading(t!("nav.about").to_string(), design::typography::BODY),
                    identity,
                    widgets::heading(t!("about.updates").to_string(), design::typography::BODY),
                    updates
                ]
                .spacing(super::widgets::CARD_GAP);
                if self.latest.is_some() {
                    body = body.push(text(
                        t!(if self.download.is_some() {
                            "about.old"
                        } else {
                            "about.up_to_date"
                        })
                        .to_string(),
                    ));
                }
                if let Some(url) = &self.download {
                    body = body.push(
                        button(text(t!("about.download_update").to_string()))
                            .on_press(Message::Open(url.clone())),
                    );
                }
                if let Some(error) = &self.update_error {
                    body = body.push(text(error));
                }
                body
            }
            _ => column![],
        };
        scrollable(body).height(Fill).into()
    }
}
fn color_button(color: u32, selected: bool) -> iced::widget::Button<'static, Message> {
    button(iced::widget::Space::new())
        .width(42)
        .height(42)
        .on_press(Message::Accent(color))
        .style(move |theme, status| {
            let mut style = widgets::quiet(theme, status);
            style.background = Some(rgb(color).into());
            if selected {
                style.border.width = 2.0;
                style.border.color = theme.palette().text;
            }
            style
        })
}
pub(super) fn rgb(color: u32) -> Color {
    Color::from_rgb8((color >> 16) as u8, (color >> 8) as u8, color as u8)
}
fn theme_label(v: AppThemeMode) -> String {
    match v {
        AppThemeMode::System => t!("theme.system"),
        AppThemeMode::Light => t!("theme.light"),
        AppThemeMode::Dark => t!("theme.dark"),
    }
    .to_string()
}

fn channel_label(v: UpdateChannel) -> String {
    match v {
        UpdateChannel::Stable => t!("update_channel.stable"),
        UpdateChannel::PreRelease => t!("update_channel.pre_release"),
    }
    .to_string()
}
fn log_label(v: ActionLogMode) -> String {
    match v {
        ActionLogMode::Full => t!("settings.action_log_mode_full"),
        ActionLogMode::Warning => t!("settings.action_log_mode_warning"),
        ActionLogMode::Error => t!("settings.action_log_mode_error"),
        ActionLogMode::Off => t!("settings.action_log_mode_off"),
    }
    .to_string()
}

pub(super) fn theme(s: &GeneralSettings) -> Theme {
    let system =
        if s.theme_mode == AppThemeMode::System || s.accent.source == AccentColorSource::Windows {
            match crate::platform::windows::appearance::read() {
                Ok(appearance) => Some(appearance),
                Err(error) => {
                    eprintln!("Unable to read Windows appearance: {error}");
                    None
                }
            }
        } else {
            None
        };
    let light = match s.theme_mode {
        AppThemeMode::Light => true,
        AppThemeMode::Dark => false,
        AppThemeMode::System => system.as_ref().is_none_or(|appearance| appearance.light),
    };
    let mut palette = design::palette(light);
    if s.accent.source == AccentColorSource::Custom {
        palette.primary = rgb(s.accent.custom_color);
    } else if let Some(system) = system {
        palette.primary = rgb(system.accent);
    }
    Theme::custom_with_fn("Winderust", palette, design::extended_palette)
}

fn sanitize_advanced(settings: &mut Settings) {
    sanitize_visible_window_priority_values(settings);
    settings.process_priority.background_priority = settings
        .process_priority
        .background_priority
        .safe_when_advanced_disabled();
    settings.process_priority.foreground_priority = settings
        .process_priority
        .foreground_priority
        .safe_when_advanced_disabled();
    settings.thread_priority.background_priority = settings
        .thread_priority
        .background_priority
        .safe_when_advanced_disabled();
    settings.thread_priority.foreground_priority = settings
        .thread_priority
        .foreground_priority
        .safe_when_advanced_disabled();
    settings.io_priority.background_priority = settings
        .io_priority
        .background_priority
        .safe_when_advanced_disabled();
    settings.io_priority.foreground_priority = settings
        .io_priority
        .foreground_priority
        .safe_when_advanced_disabled();
    settings.gpu_priority.background_priority = settings
        .gpu_priority
        .background_priority
        .safe_when_advanced_disabled();
    settings.gpu_priority.foreground_priority = settings
        .gpu_priority
        .foreground_priority
        .safe_when_advanced_disabled();
    settings.cpu_scheduler.io_priority.background_priority = settings
        .cpu_scheduler
        .io_priority
        .background_priority
        .safe_when_advanced_disabled();
    settings.cpu_scheduler.background_priority = settings
        .cpu_scheduler
        .background_priority
        .safe_when_advanced_disabled();
    settings.cpu_scheduler.focus_process_priority = settings
        .cpu_scheduler
        .focus_process_priority
        .safe_when_advanced_disabled();
    settings.cpu_scheduler.io_priority.foreground_priority = settings
        .cpu_scheduler
        .io_priority
        .foreground_priority
        .safe_when_advanced_disabled();
    settings.cpu_scheduler.thread_priority.background_priority = settings
        .cpu_scheduler
        .thread_priority
        .background_priority
        .safe_when_advanced_disabled();
    settings.cpu_scheduler.thread_priority.foreground_priority = settings
        .cpu_scheduler
        .thread_priority
        .foreground_priority
        .safe_when_advanced_disabled();
    settings.cpu_scheduler.gpu_priority.background_priority = settings
        .cpu_scheduler
        .gpu_priority
        .background_priority
        .safe_when_advanced_disabled();
    settings.cpu_scheduler.gpu_priority.foreground_priority = settings
        .cpu_scheduler
        .gpu_priority
        .foreground_priority
        .safe_when_advanced_disabled();
    for rule in &mut settings.process_priority.exclusions {
        let foreground = rule
            .process_priority_override(true, false)
            .safe_when_advanced_disabled();
        let visible_window = rule
            .process_priority_override(false, true)
            .safe_when_advanced_disabled();
        let background = rule
            .process_priority_override(false, false)
            .safe_when_advanced_disabled();
        rule.set_process_priority_override(true, false, foreground);
        rule.set_process_priority_override(false, true, visible_window);
        rule.set_process_priority_override(false, false, background);
    }
    for rule in &mut settings.thread_priority.exclusions {
        let foreground = rule
            .thread_priority_override(true, false)
            .safe_when_advanced_disabled();
        let visible_window = rule
            .thread_priority_override(false, true)
            .safe_when_advanced_disabled();
        let background = rule
            .thread_priority_override(false, false)
            .safe_when_advanced_disabled();
        rule.set_thread_priority_override(true, false, foreground);
        rule.set_thread_priority_override(false, true, visible_window);
        rule.set_thread_priority_override(false, false, background);
    }
    for rule in &mut settings.io_priority.exclusions {
        let foreground = rule
            .io_priority_override(true, false)
            .safe_when_advanced_disabled();
        let visible_window = rule
            .io_priority_override(false, true)
            .safe_when_advanced_disabled();
        let background = rule
            .io_priority_override(false, false)
            .safe_when_advanced_disabled();
        rule.set_io_priority_override(true, false, foreground);
        rule.set_io_priority_override(false, true, visible_window);
        rule.set_io_priority_override(false, false, background);
    }
    for rule in &mut settings.gpu_priority.exclusions {
        let foreground = rule
            .gpu_priority_override(true, false)
            .safe_when_advanced_disabled();
        let visible_window = rule
            .gpu_priority_override(false, true)
            .safe_when_advanced_disabled();
        let background = rule
            .gpu_priority_override(false, false)
            .safe_when_advanced_disabled();
        rule.set_gpu_priority_override(true, false, foreground);
        rule.set_gpu_priority_override(false, true, visible_window);
        rule.set_gpu_priority_override(false, false, background);
    }
}
fn sanitize_visible_window_priority_values(settings: &mut Settings) {
    settings.process_priority.visible_window_priority = settings
        .process_priority
        .visible_window_priority
        .safe_when_advanced_disabled();
    settings.thread_priority.visible_window_priority = settings
        .thread_priority
        .visible_window_priority
        .safe_when_advanced_disabled();
    settings.io_priority.visible_window_priority = settings
        .io_priority
        .visible_window_priority
        .safe_when_advanced_disabled();
    settings.gpu_priority.visible_window_priority = settings
        .gpu_priority
        .visible_window_priority
        .safe_when_advanced_disabled();
    settings.cpu_scheduler.io_priority.visible_window_priority = settings
        .cpu_scheduler
        .io_priority
        .visible_window_priority
        .safe_when_advanced_disabled();
    settings.cpu_scheduler.visible_window_priority = settings
        .cpu_scheduler
        .visible_window_priority
        .safe_when_advanced_disabled();
    settings
        .cpu_scheduler
        .thread_priority
        .visible_window_priority = settings
        .cpu_scheduler
        .thread_priority
        .visible_window_priority
        .safe_when_advanced_disabled();
    settings.cpu_scheduler.gpu_priority.visible_window_priority = settings
        .cpu_scheduler
        .gpu_priority
        .visible_window_priority
        .safe_when_advanced_disabled();
}

const ACCENT_PALETTE: [u32; 48] = [
    0xa7e957, 0xc7f36d, 0x8fd14f, 0x65b741, 0x3f8f34, 0x2f6f34, 0xd8c75b, 0xffc857, 0xe0a93a,
    0xb9802f, 0x8d6128, 0xff8f5a, 0xe46845, 0xbb4c38, 0x8d382f, 0x6a2f2a, 0x4fc3a5, 0x2aa889,
    0x167c68, 0x0f5f54, 0x76d0b2, 0xa8d6a1, 0xd1e3a4, 0xf2e5a0, 0xe8d7b2, 0xc7b58f, 0xa8946d,
    0x786a50, 0x9bbf74, 0x7fa15d, 0x5d8048, 0x3f6038, 0xd9a441, 0xbf8033, 0xa45f31, 0x7d452e,
    0xd96f6a, 0xb85b58, 0x8d4645, 0x633839, 0x8aa49a, 0x6f877d, 0x53665f, 0x3d4d47, 0xc1b897,
    0xa8a07d, 0x837c61, 0x625d48,
];
fn parse_color(value: &str) -> Option<u32> {
    let value = value.trim().strip_prefix('#').unwrap_or(value.trim());
    (value.len() == 6)
        .then(|| u32::from_str_radix(value, 16).ok())
        .flatten()
}
fn logo() -> iced::widget::image::Handle {
    static HANDLE: std::sync::LazyLock<iced::widget::image::Handle> =
        std::sync::LazyLock::new(|| {
            iced::widget::image::Handle::from_bytes(
                include_bytes!("../../image/icon-design.png").as_slice(),
            )
        });
    HANDLE.clone()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failure_threshold_retains_drafts_without_publishing_invalid_values() {
        let mut editor = Editor::default();
        let mut settings = Settings::default();
        editor.update(&mut settings, Message::FailureThreshold("12".into()));
        for draft in ["", "0", "101", "invalid"] {
            editor.update(&mut settings, Message::FailureThreshold(draft.into()));
            assert_eq!(editor.failure_threshold.as_deref(), Some(draft));
            assert_eq!(
                settings.advanced.execution_failure_suppression_threshold,
                12
            );
        }
        editor.reset_drafts();
        assert!(editor.failure_threshold.is_none());
    }
    #[test]
    fn resetting_drafts_preserves_in_flight_update_check() {
        let mut e = Editor {
            color: Some("#123456".into()),
            checking: true,
            notify_on_update: true,
            latest: Some("1.0.0".into()),
            download: Some("https://example.com".into()),
            update_error: Some("error".into()),
            show_update: true,
            ..Editor::default()
        };
        e.reset_drafts();
        assert!(e.checking && e.notify_on_update);
        assert!(e.color.is_none() && e.latest.is_none() && e.download.is_none());
        assert!(e.update_error.is_none() && !e.show_update);
    }

    #[test]
    fn appearance_edits_validate_colors_and_advanced_mode_restores_safe_values() {
        let mut e = Editor::default();
        let mut s = Settings::default();
        assert_eq!(parse_color("#A7e957"), Some(0xa7e957));
        assert_eq!(parse_color("##A7e957"), None);
        let before = s.general.accent.custom_color;
        e.update(&mut s, Message::AccentHex("#xyzxyz".into()));
        assert_eq!(s.general.accent.custom_color, before);
        e.update(&mut s, Message::SaveColor);
        assert!(s.general.accent.custom_colors.is_empty());
        e.update(&mut s, Message::AccentHex("#123456".into()));
        e.update(&mut s, Message::SaveColor);
        e.update(&mut s, Message::SaveColor);
        assert_eq!(s.general.accent.custom_colors, vec![0x123456]);
        s.process_priority.visible_window_priority = ProcessPrioritySetting::Realtime;
        s.thread_priority.visible_window_priority = ProcessThreadPrioritySetting::TimeCritical;
        s.cpu_scheduler.gpu_priority.background_priority = ProcessGpuPrioritySetting::Realtime;
        e.update(&mut s, Message::Flag(Flag::AdvancedValues, false));
        assert_eq!(
            s.process_priority.visible_window_priority,
            ProcessPrioritySetting::AboveNormal
        );
        assert_eq!(
            s.thread_priority.visible_window_priority,
            ProcessThreadPrioritySetting::Highest
        );
        assert_eq!(
            s.cpu_scheduler.gpu_priority.background_priority,
            ProcessGpuPrioritySetting::AboveNormal
        );
        let once = s.clone();
        e.update(&mut s, Message::Flag(Flag::AdvancedValues, false));
        assert_eq!(s, once);
        assert_eq!(ACCENT_PALETTE.len(), 48);
    }
    #[test]
    fn only_current_channel_startup_checks_raise_update_notification() {
        let mut e = Editor::default();
        let mut s = Settings::default();
        let channel = s.general.update_channel;
        assert!(e.begin_check(channel, false));
        assert!(!e.begin_check(channel, true));
        e.update(
            &mut s,
            Message::Checked(
                channel,
                Ok(("1.0".into(), Some("https://example.com".into()))),
            ),
        );
        assert!(!e.show_update);
        assert!(e.begin_check(channel, true));
        e.update(
            &mut s,
            Message::Checked(
                channel,
                Ok(("1.0".into(), Some("https://example.com".into()))),
            ),
        );
        assert!(e.show_update);
        assert!(e.begin_check(channel, true));
        let other = if channel == UpdateChannel::Stable {
            UpdateChannel::PreRelease
        } else {
            UpdateChannel::Stable
        };
        e.update(&mut s, Message::Channel(other));
        e.update(
            &mut s,
            Message::Checked(channel, Ok(("wrong-channel".into(), None))),
        );
        assert!(e.latest.is_none());
        assert!(!e.show_update);
    }
}
