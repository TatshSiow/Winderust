use super::design;
use super::widgets::{self, Choice, Ui};
use crate::{config::*, ui::Page};
use rust_i18n::t;
use windui::prelude::{
    signal, Align, Color, ColorPickerOpts, Element, Intent, MenuItem, Role, Theme,
};

#[derive(Default)]
pub(super) struct Editor {
    color: Option<String>,
    failure_threshold: Option<String>,
    accent_expanded: bool,
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
    Animation(AnimationMode),
    Theme(AppThemeMode),
    AccentSource(AccentColorSource),
    AccentHex(String),
    ToggleAccent,
    EditColor(u32),
    ReplaceColor(u32, u32),
    Accent(u32),
    SaveColor,
    RemoveColor(usize),
    FailureThreshold(String),
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
    pub(super) fn has_invalid_inputs(&self) -> bool {
        self.failure_threshold.as_ref().is_some_and(|value| {
            value
                .parse::<u8>()
                .ok()
                .is_none_or(|value| !(1..=100).contains(&value))
        }) || self
            .color
            .as_ref()
            .is_some_and(|value| parse_color(value).is_none())
    }

    pub(super) fn reset_drafts(&mut self) {
        self.color = None;
        self.failure_threshold = None;
        self.latest = None;
        self.download = None;
        self.update_error = None;
        self.show_update = false;
    }

    pub(super) fn begin_check(&mut self, automatic: bool) -> bool {
        if self.checking {
            return false;
        }
        self.checking = true;
        self.notify_on_update = automatic;
        self.update_error = None;
        true
    }

    pub(super) fn update_ui(&mut self, current_channel: UpdateChannel, message: Message) {
        match message {
            Message::ToggleAccent => self.accent_expanded = !self.accent_expanded,
            Message::DismissUpdate => self.show_update = false,
            Message::Checked(channel, result) => {
                if channel != current_channel {
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
            _ => unreachable!("Only UI preference messages belong in update_ui"),
        }
    }

    pub(super) fn update(&mut self, s: &mut Settings, m: Message) {
        match m {
            Message::Flag(flag, value) => match flag {
                Flag::Enabled => s.general.enabled = value,
                Flag::Startup => s.general.startup_with_windows = value,
                Flag::Minimized => s.general.start_minimized = value,
                Flag::Tray => s.general.hide_to_tray = value,
                Flag::CrossSession => s.general.allow_cross_session_process_control = value,
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
            Message::Animation(value) => s.general.animation_mode = value,
            Message::Theme(value) => s.general.theme_mode = value,
            Message::AccentSource(value) => {
                s.general.accent.source = value;
                self.accent_expanded = value != AccentColorSource::Windows;
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
            Message::ReplaceColor(original, color) => {
                if let Some(slot) = s
                    .general
                    .accent
                    .custom_colors
                    .iter_mut()
                    .find(|v| **v == original)
                {
                    *slot = color;
                    let mut seen = std::collections::HashSet::new();
                    s.general
                        .accent
                        .custom_colors
                        .retain(|value| seen.insert(*value));
                    if s.general.accent.source == AccentColorSource::Custom
                        && s.general.accent.custom_color == original
                    {
                        self.update(s, Message::Accent(color));
                    }
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
            Message::Channel(value) => {
                s.general.update_channel = value;
                self.latest = None;
                self.download = None;
                self.update_error = None;
                self.show_update = false;
            }
            Message::Check | Message::CheckStartup => {}
            Message::Checked(..) | Message::DismissUpdate | Message::ToggleAccent => {
                self.update_ui(s.general.update_channel, m);
            }
            Message::Open(_) | Message::Export | Message::Import | Message::EditColor(_) => {}
        }
    }
    pub(super) fn view(&self, page: Page, s: &Settings, ui: Ui<Message>) -> Element {
        let flag = |key: &str, value, flag| {
            widgets::card(
                "",
                [widgets::setting(
                    key,
                    ui.toggle(value, move |value| Message::Flag(flag, value)),
                )],
            )
        };
        let body = match page {
            Page::WinderustBehaviour => vec![
                widgets::heading(t!("settings.functionality").to_string()),
                flag("settings.master_switch", s.general.enabled, Flag::Enabled),
                widgets::heading(t!("settings.launch_settings").to_string()),
                flag(
                    "settings.startup_windows",
                    s.general.startup_with_windows,
                    Flag::Startup,
                ),
                flag(
                    "settings.start_minimized",
                    s.general.start_minimized,
                    Flag::Minimized,
                ),
                flag("settings.hide_to_tray", s.general.hide_to_tray, Flag::Tray),
                widgets::heading(t!("settings.advanced").to_string()),
                flag(
                    "settings.allow_cross_session_process_control",
                    s.general.allow_cross_session_process_control,
                    Flag::CrossSession,
                ),
                widgets::card(
                    "",
                    [widgets::setting(
                        "settings.failure_suppression_threshold",
                        ui.input(
                            self.failure_threshold.clone().unwrap_or_else(|| {
                                s.advanced
                                    .execution_failure_suppression_threshold
                                    .to_string()
                            }),
                            "",
                            Message::FailureThreshold,
                        )
                        .width(96),
                    )],
                ),
                widgets::heading(t!("settings.settings_files").to_string()),
                widgets::row().children([
                    ui.button(t!("settings.export_settings").to_string(), Message::Export),
                    ui.button(t!("settings.import_settings").to_string(), Message::Import),
                ]),
            ],
            Page::LanguageAndAppearance => {
                let colors = widgets::col().children(ACCENT_PALETTE.chunks(4).map(|chunk| {
                    widgets::row().children(chunk.iter().map(|color| {
                        color_button(
                            *color,
                            s.general.accent.source == AccentColorSource::Custom
                                && s.general.accent.custom_color == *color,
                            &ui,
                        )
                    }))
                }));
                let saved = widgets::col().children(
                    s.general.accent.custom_colors.chunks(4).enumerate().map(
                        |(chunk_index, chunk)| {
                            widgets::row().children(chunk.iter().enumerate().map(|(i, color)| {
                                let color = *color;
                                let index = chunk_index * 4 + i;
                                let edit = ui.clone();
                                let remove = ui.clone();
                                super::popover::context_menu(
                                    color_button(
                                        color,
                                        s.general.accent.source == AccentColorSource::Custom
                                            && s.general.accent.custom_color == color,
                                        &ui,
                                    ),
                                    move || {
                                        let edit = edit.clone();
                                        let remove = remove.clone();
                                        vec![
                                            MenuItem::run(
                                                t!("common.edit").to_string(),
                                                move |ctx| {
                                                    widgets::flush_bindings(ctx);
                                                    edit.emit(Message::EditColor(color));
                                                },
                                                false,
                                            ),
                                            MenuItem::run(
                                                t!("common.remove").to_string(),
                                                move |ctx| {
                                                    widgets::flush_bindings(ctx);
                                                    remove.emit(Message::RemoveColor(index));
                                                },
                                                false,
                                            ),
                                        ]
                                    },
                                )
                            }))
                        },
                    ),
                );
                let chosen = signal(rgb(s.general.accent.custom_color));
                let draft = signal(
                    self.color
                        .clone()
                        .unwrap_or_else(|| format!("#{:06X}", s.general.accent.custom_color)),
                );
                let picker_ui = ui.clone();
                let picker = widgets::observe(
                    Element::color_picker_opts(
                        chosen,
                        ColorPickerOpts::default()
                            .alpha(false)
                            .presets(ACCENT_PALETTE.into_iter().map(rgb).collect()),
                    ),
                    chosen,
                    move |color| {
                        let color =
                            u32::from(color.r) << 16 | u32::from(color.g) << 8 | u32::from(color.b);
                        if parse_color(&draft.get()) != Some(color) {
                            draft.set(format!("#{color:06X}"));
                            picker_ui.edit(Message::Accent(color));
                        }
                    },
                );
                let input_ui = ui.clone();
                let hex = widgets::observe(
                    Element::text_input(draft, "#RRGGBB").width(130),
                    draft,
                    move |text| {
                        if let Some(color) = parse_color(&text) {
                            chosen.set(rgb(color));
                        }
                        input_ui.edit(Message::AccentHex(text));
                    },
                );
                let custom = widgets::col().children([
                    widgets::label(t!("accent.color_palette").to_string()),
                    colors,
                    widgets::label(t!("accent.custom_colors").to_string()),
                    widgets::row().children([
                        picker,
                        hex,
                        ui.button(t!("common.save").to_string(), Message::SaveColor)
                            .enabled_when(move || parse_color(&draft.get()).is_some()),
                    ]),
                    saved,
                ]);
                let accent_choices = [
                    Choice(AccentColorSource::Windows, t!("accent.windows").to_string()),
                    Choice(AccentColorSource::Custom, t!("accent.custom").to_string()),
                ];
                let accent = accent_choices
                    .iter()
                    .find(|choice| choice.0 == s.general.accent.source)
                    .cloned()
                    .unwrap_or_else(|| accent_choices[0].clone());
                vec![
                    widgets::card(
                        "",
                        [
                            widgets::setting_row(
                                t!("common.language").to_string(),
                                ui.select(
                                    AppLanguage::ALL
                                        .into_iter()
                                        .map(|v| Choice(v, v.native_label().to_owned())),
                                    Choice(
                                        s.general.language,
                                        s.general.language.native_label().to_owned(),
                                    ),
                                    |v| Message::Language(v.0),
                                )
                                .width(240),
                            ),
                            Element::divider(),
                            widgets::setting_row(
                                t!("accent.source").to_string(),
                                ui.select(accent_choices, accent, |v| Message::AccentSource(v.0))
                                    .width(240),
                            ),
                            ui.collapsible(
                                t!("accent.custom_colors").to_string(),
                                self.accent_expanded,
                                custom,
                                |_| Message::ToggleAccent,
                            ),
                            Element::divider(),
                            widgets::setting_row(
                                t!("common.theme").to_string(),
                                ui.select(
                                    AppThemeMode::ALL
                                        .into_iter()
                                        .map(|v| Choice(v, theme_label(v))),
                                    Choice(s.general.theme_mode, theme_label(s.general.theme_mode)),
                                    |v| Message::Theme(v.0),
                                )
                                .width(240),
                            ),
                            Element::divider(),
                            widgets::setting_row(
                                t!("settings.animation_mode").to_string(),
                                ui.select(
                                    AnimationMode::ALL
                                        .into_iter()
                                        .map(|v| Choice(v, animation_label(v))),
                                    Choice(
                                        s.general.animation_mode,
                                        animation_label(s.general.animation_mode),
                                    ),
                                    |v| Message::Animation(v.0),
                                )
                                .width(240),
                            ),
                        ],
                    )
                    .spacing(0),
                    widgets::card(
                        "",
                        [
                            widgets::setting(
                                "settings.show_enabled_feature_counts_in_sidebar",
                                ui.toggle(s.general.show_enabled_feature_counts_in_sidebar, |v| {
                                    Message::Flag(Flag::FeatureCounts, v)
                                }),
                            ),
                            Element::divider(),
                            widgets::setting(
                                "settings.show_feature_status_on_cards",
                                ui.toggle(s.general.show_feature_status_on_cards, |v| {
                                    Message::Flag(Flag::CardStatus, v)
                                }),
                            ),
                        ],
                    )
                    .spacing(0),
                ]
            }
            Page::ExperimentalFeatures => vec![
                flag(
                    "settings.expose_all_priority_values",
                    s.advanced.expose_all_priority_values,
                    Flag::AdvancedValues,
                ),
                flag(
                    "settings.show_advanced_controls",
                    s.advanced.show_advanced_controls,
                    Flag::AdvancedControls,
                ),
            ],
            Page::About => {
                let links = Element::flow()
                    .width_match()
                    .spacing(8)
                    .run_spacing(8)
                    .children(
                        [
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
                        ]
                        .into_iter()
                        .map(|(key, url)| {
                            ui.button(t!(key).to_string(), Message::Open(url.into()))
                                .tooltip(url)
                        }),
                    );
                let identity = widgets::card(
                    "",
                    [
                        widgets::row()
                            .children([
                                logo(),
                                widgets::col().weight(1.0).spacing(8).children([
                                    widgets::heading(t!("app.name").to_string()),
                                    widgets::label(t!("app.description").to_string()).width_match(),
                                ]),
                            ])
                            .spacing(16),
                        widgets::label(format!(
                            "{} Wanderlust {} Windows Derust",
                            t!("about.inspired_by"),
                            t!("about.inspiration_joiner"),
                        ))
                        .fg_role(Role::TextMuted),
                        widgets::label("Copyright (C) 2026 Tatsh Siow / GPL-3.0-only")
                            .fg_role(Role::TextMuted),
                        links,
                    ],
                )
                .spacing(16);
                let mut update_rows = vec![
                    Element::setting_row(
                        t!("about.update_channel").to_string(),
                        Element::row().cross(Align::Center).spacing(8).children([
                            ui.select(
                                UpdateChannel::ALL
                                    .into_iter()
                                    .map(|v| Choice(v, channel_label(v))),
                                Choice(
                                    s.general.update_channel,
                                    channel_label(s.general.update_channel),
                                ),
                                |v| Message::Channel(v.0),
                            )
                            .width(240),
                            ui.button(
                                t!(if self.checking {
                                    "common.loading"
                                } else {
                                    "about.check_for_updates"
                                })
                                .to_string(),
                                Message::Check,
                            )
                            .enabled(!self.checking),
                        ]),
                    )
                    .width_match(),
                    Element::divider(),
                    widgets::setting_row(
                        t!("about.latest_version").to_string(),
                        widgets::label(self.latest.clone().unwrap_or_else(|| "-".into())),
                    ),
                    Element::divider(),
                    widgets::setting_row(
                        t!("about.current_version").to_string(),
                        widgets::label(env!("CARGO_PKG_VERSION")),
                    ),
                    Element::divider(),
                    widgets::setting_row(
                        t!("about.automatic_check_for_updates_on_startup").to_string(),
                        ui.toggle(s.general.check_for_updates, |v| {
                            Message::Flag(Flag::UpdateCheck, v)
                        }),
                    ),
                ];
                if self.latest.is_some() {
                    update_rows.push(Element::badge_intent(
                        t!(if self.download.is_some() {
                            "about.old"
                        } else {
                            "about.up_to_date"
                        })
                        .to_string(),
                        if self.download.is_some() {
                            Intent::Warning
                        } else {
                            Intent::Success
                        },
                    ));
                }
                if let Some(url) = &self.download {
                    update_rows.push(ui.button(
                        t!("about.download_update").to_string(),
                        Message::Open(url.clone()),
                    ));
                }
                if let Some(error) = &self.update_error {
                    update_rows.push(widgets::error(error.clone()));
                }
                vec![
                    identity,
                    widgets::card(t!("about.updates").to_string(), update_rows),
                ]
            }
            _ => Vec::new(),
        };
        widgets::page(body)
    }
}
fn color_button(color: u32, selected: bool, ui: &Ui<Message>) -> Element {
    let color_value = rgb(color);
    ui.primary_button(format!("#{color:06X}"), Message::Accent(color))
        .width(88)
        .height(36)
        .font_size(11.0)
        .bg(color_value)
        .fg(super::design::accent_foreground(color_value))
        .border_role(Role::Text, if selected { 2 } else { 0 })
}
pub(super) fn rgb(color: u32) -> Color {
    Color::hex(color)
}
fn animation_label(value: AnimationMode) -> String {
    t!(match value {
        AnimationMode::On => "common.on",
        AnimationMode::Off => "common.off",
        AnimationMode::System => "theme.system",
    })
    .to_string()
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
/// Native text and window menus follow the same language as Winderust's pages.
/// Wind UI 0.24 bundles English and Simplified Chinese; supply its complete
/// eight-key menu vocabulary for Winderust's Traditional Chinese selection.
pub(super) fn native_locales(language: AppLanguage) -> windui::i18n::Locales {
    windui::i18n::Locales::builder()
        .embed(
            r#"
[meta]
locale = "zh-TW"
name = "繁體中文（台灣）"
[windui.menu]
cut = "剪下"
copy = "複製"
paste = "貼上"
select_all = "全選"
[windui.window]
restore = "還原"
minimize = "最小化"
maximize = "最大化"
close = "關閉"
"#,
        )
        .initial(windui::i18n::Initial::Fixed(language.locale().into()))
        .build()
}

pub(super) fn theme(s: &GeneralSettings) -> Theme {
    let system = match crate::platform::windows::appearance::read() {
        Ok(appearance) => Some(appearance),
        Err(error) => {
            eprintln!("Unable to read Windows appearance: {error}");
            None
        }
    };
    super::motion::apply(
        s.animation_mode,
        system
            .as_ref()
            .is_some_and(|appearance| appearance.animations),
    );
    let light = match s.theme_mode {
        AppThemeMode::Light => true,
        AppThemeMode::Dark => false,
        AppThemeMode::System => system.as_ref().is_none_or(|appearance| appearance.light),
    };
    let accent = if s.accent.source == AccentColorSource::Custom {
        Some(rgb(s.accent.custom_color))
    } else {
        system.map(|appearance| rgb(appearance.accent))
    };
    design::theme(light, accent)
}

fn sanitize_advanced(settings: &mut Settings) {
    if let Some(battery) = &mut settings.on_battery {
        sanitize_advanced(battery);
    }
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
    settings
        .adaptive_engine_process
        .io_priority
        .background_priority = settings
        .adaptive_engine_process
        .io_priority
        .background_priority
        .safe_when_advanced_disabled();
    settings.adaptive_engine_process.background_priority = settings
        .adaptive_engine_process
        .background_priority
        .safe_when_advanced_disabled();
    settings.adaptive_engine_process.focus_process_priority = settings
        .adaptive_engine_process
        .focus_process_priority
        .safe_when_advanced_disabled();
    settings
        .adaptive_engine_process
        .io_priority
        .foreground_priority = settings
        .adaptive_engine_process
        .io_priority
        .foreground_priority
        .safe_when_advanced_disabled();
    settings
        .adaptive_engine_process
        .thread_priority
        .background_priority = settings
        .adaptive_engine_process
        .thread_priority
        .background_priority
        .safe_when_advanced_disabled();
    settings
        .adaptive_engine_process
        .thread_priority
        .foreground_priority = settings
        .adaptive_engine_process
        .thread_priority
        .foreground_priority
        .safe_when_advanced_disabled();
    settings
        .adaptive_engine_process
        .gpu_priority
        .background_priority = settings
        .adaptive_engine_process
        .gpu_priority
        .background_priority
        .safe_when_advanced_disabled();
    settings
        .adaptive_engine_process
        .gpu_priority
        .foreground_priority = settings
        .adaptive_engine_process
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
    settings
        .adaptive_engine_process
        .io_priority
        .visible_window_priority = settings
        .adaptive_engine_process
        .io_priority
        .visible_window_priority
        .safe_when_advanced_disabled();
    settings.adaptive_engine_process.visible_window_priority = settings
        .adaptive_engine_process
        .visible_window_priority
        .safe_when_advanced_disabled();
    settings
        .adaptive_engine_process
        .thread_priority
        .visible_window_priority = settings
        .adaptive_engine_process
        .thread_priority
        .visible_window_priority
        .safe_when_advanced_disabled();
    settings
        .adaptive_engine_process
        .gpu_priority
        .visible_window_priority = settings
        .adaptive_engine_process
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
fn logo() -> Element {
    Element::image_bytes(include_bytes!("../../image/icon-design.png")).size(52, 52)
}
#[cfg(test)]
mod tests {
    #[test]
    fn native_menu_catalog_covers_both_product_languages() {
        let locales = native_locales(AppLanguage::English);
        let english = locales.catalog("en");
        let traditional = locales.catalog("zh-TW");
        assert_eq!(english.lang(), "en");
        assert_eq!(traditional.lang(), "zh-TW");
        assert_eq!(
            english.keys().collect::<Vec<_>>(),
            traditional.keys().collect::<Vec<_>>()
        );
        for (key, translated) in [
            ("windui.menu.cut", "剪下"),
            ("windui.menu.copy", "複製"),
            ("windui.menu.paste", "貼上"),
            ("windui.menu.select_all", "全選"),
            ("windui.window.restore", "還原"),
            ("windui.window.minimize", "最小化"),
            ("windui.window.maximize", "最大化"),
            ("windui.window.close", "關閉"),
        ] {
            assert_eq!(traditional.text(key), translated);
        }
        assert_eq!(english.text("windui.menu.copy"), "Copy");
    }

    #[test]
    fn ui_only_preferences_need_no_mutable_settings() {
        let settings = Settings::default();
        let channel = settings.general.update_channel;
        let mut editor = Editor::default();
        editor.update_ui(channel, Message::ToggleAccent);
        assert!(editor.accent_expanded);
        assert!(editor.begin_check(true));
        editor.update_ui(
            channel,
            Message::Checked(channel, Ok(("next".into(), Some("url".into())))),
        );
        assert!(!editor.checking);
        assert!(editor.show_update);
        editor.update_ui(channel, Message::DismissUpdate);
        assert!(!editor.show_update);
        assert!(editor.begin_check(false));
    }

    #[test]
    fn replacing_with_an_existing_color_keeps_the_palette_unique() {
        let mut editor = Editor::default();
        let mut settings = Settings::default();
        settings.general.accent.custom_colors = vec![0x123456, 0xabcdef];
        editor.update(&mut settings, Message::Accent(0x123456));
        editor.update(&mut settings, Message::ReplaceColor(0x123456, 0xabcdef));
        assert_eq!(settings.general.accent.custom_colors, vec![0xabcdef]);
        assert_eq!(settings.general.accent.custom_color, 0xabcdef);
        editor.update(&mut settings, Message::ReplaceColor(0xabcdef, 0x112233));
        assert_eq!(settings.general.accent.custom_colors, vec![0x112233]);
        assert_eq!(settings.general.accent.custom_color, 0x112233);
    }

    #[test]
    fn editing_saved_colors_updates_only_the_matching_swatch_and_active_accent() {
        let mut editor = Editor::default();
        let mut settings = Settings::default();
        settings.general.accent.custom_colors = vec![0x123456, 0xabcdef];
        editor.update(&mut settings, Message::Accent(0x123456));
        editor.update(&mut settings, Message::ReplaceColor(0xabcdef, 0x112233));
        assert_eq!(settings.general.accent.custom_color, 0x123456);
        editor.update(&mut settings, Message::ReplaceColor(0x123456, 0x445566));
        assert_eq!(settings.general.accent.custom_color, 0x445566);
        assert_eq!(
            settings.general.accent.custom_colors,
            vec![0x445566, 0x112233]
        );
        editor.update(&mut settings, Message::RemoveColor(0));
        assert_eq!(settings.general.accent.custom_colors, vec![0x112233]);
        editor.update(&mut settings, Message::ReplaceColor(0x445566, 0));
        assert_eq!(settings.general.accent.custom_colors, vec![0x112233]);
    }

    #[test]
    fn global_preference_messages_ignore_the_selected_feature_profile() {
        for existing_battery in [false, true] {
            let mut initial = Settings::default();
            initial.general.enabled = true;
            if existing_battery {
                initial.battery_profile_mut().cpu_limiter.enabled = true;
            }
            let mut settings = crate::application::SettingsEditor::with_settings(initial.clone());
            settings.select_power_source(PowerSourceProfile::OnBattery);
            let mut editor = Editor::default();
            for message in [
                Message::Flag(Flag::Enabled, false),
                Message::Flag(Flag::Startup, true),
                Message::FailureThreshold("12".into()),
            ] {
                settings.edit_global(|root| editor.update(root, message));
            }
            assert!(!settings.global().general.enabled);
            assert!(!settings.general.enabled);
            assert!(settings.global().general.startup_with_windows);
            assert_eq!(
                settings
                    .global()
                    .advanced
                    .execution_failure_suppression_threshold,
                12
            );
            assert_eq!(settings.cpu_limiter.enabled, existing_battery);
            settings.cancel();
            assert_eq!(settings.global(), &initial);
        }
    }

    #[test]
    fn animation_modes_override_or_follow_system() {
        let mut settings = crate::config::Settings::default();
        let mut editor = super::Editor::default();
        assert_eq!(settings.general.animation_mode, AnimationMode::System);
        for mode in AnimationMode::ALL {
            editor.update(&mut settings, super::Message::Animation(mode));
            assert_eq!(settings.general.animation_mode, mode);
            for system in [false, true] {
                assert_eq!(
                    mode.enabled(system),
                    match mode {
                        AnimationMode::On => true,
                        AnimationMode::Off => false,
                        AnimationMode::System => system,
                    }
                );
            }
        }
    }

    use super::*;
    #[test]
    fn failure_threshold_retains_drafts_without_publishing_invalid_values() {
        let mut editor = Editor::default();
        let mut settings = Settings::default();
        editor.update(&mut settings, Message::FailureThreshold("12".into()));
        for draft in ["", "0", "101", "invalid"] {
            editor.update(&mut settings, Message::FailureThreshold(draft.into()));
            assert_eq!(editor.failure_threshold.as_deref(), Some(draft));
            assert!(editor.has_invalid_inputs());
            assert_eq!(
                settings.advanced.execution_failure_suppression_threshold,
                12
            );
        }
        editor.reset_drafts();
        assert!(editor.failure_threshold.is_none());
        assert!(!editor.has_invalid_inputs());
        editor.update(&mut settings, Message::AccentHex("invalid".into()));
        assert!(editor.has_invalid_inputs());
        editor.update(&mut settings, Message::AccentHex("#123456".into()));
        assert!(!editor.has_invalid_inputs());
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
        s.adaptive_engine_process.gpu_priority.background_priority =
            ProcessGpuPrioritySetting::Realtime;
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
            s.adaptive_engine_process.gpu_priority.background_priority,
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
        assert!(e.begin_check(false));
        assert!(!e.begin_check(true));
        e.update(
            &mut s,
            Message::Checked(
                channel,
                Ok(("1.0".into(), Some("https://example.com".into()))),
            ),
        );
        assert!(!e.show_update);
        assert!(e.begin_check(true));
        e.update(
            &mut s,
            Message::Checked(
                channel,
                Ok(("1.0".into(), Some("https://example.com".into()))),
            ),
        );
        assert!(e.show_update);
        assert!(e.begin_check(true));
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
