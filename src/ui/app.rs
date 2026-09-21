use super::{
    action_log, adaptive_engine, advanced_power_plan_tuning, app_suspension, background_efficiency,
    by_activity, cpu_allocation, cpu_limiter, design, home, memory_trim, navigation, power_rules,
    priority_control, process_list, process_power_plans, settings_pages, status_rail, tasks,
    timer_resolution, widgets, win32_priority_separation,
};
use crate::ui::scrolling::scrollable;
use std::{cell::RefCell, path::PathBuf, time::Duration};
use widgets::{button, text_input};

use iced::widget::{column, container, row, text};
use iced::{Element, Fill, Subscription, Task, Theme};
use rust_i18n::t;

use crate::application::SettingsEditor;
use crate::automation::{RuntimeHandle, RuntimeStatusSnapshot};
use crate::config::PowerSourceProfile;
use crate::ui::Page;
use crate::SingleInstanceRestoreEvent;
use crate::{
    file_dialog::{choose_settings_file, FileDialogMode},
    tray,
};

#[cfg(feature = "render-smoke")]
#[path = "smoke.rs"]
pub(crate) mod smoke;

pub(crate) fn run(
    settings: SettingsEditor,
    error: Option<String>,
    runtime: RuntimeHandle,
    restore_event: Option<SingleInstanceRestoreEvent>,
) -> iced::Result {
    let startup = RefCell::new(Some((settings, error, runtime, restore_event)));
    iced::application(
        move || {
            let Some((settings, error, runtime, restore_event)) = startup.borrow_mut().take()
            else {
                unreachable!("Iced must initialize the application exactly once");
            };
            rust_i18n::set_locale(settings.general.language.locale());
            let (wake, events) = ui_wake_channel();
            runtime.set_auto_exclusion_wake(wake.clone());
            tray::set_ui_wake(Some(wake));
            (
                WinderustApp {
                    appearance: settings_pages::theme(&settings.general),
                    #[cfg(feature = "render-smoke")]
                    smoke: smoke::Run::requested(),
                    navigation_search: String::new(),
                    expanded_section: None,
                    status_collapsed: false,
                    feature_info_expanded: false,
                    preferences: settings_pages::Editor::default(),
                    home: home::Model::default(),
                    action_log: action_log::Editor::default(),
                    sampler: std::sync::Arc::new(std::sync::Mutex::new(home::Sampler::default())),
                    sampled_at: std::time::Instant::now(),
                    sampling: false,
                    process_sampled_at: std::time::Instant::now(),
                    catalog_sampled_at: std::time::Instant::now(),
                    suspension: app_suspension::Editor::default(),
                    trim: memory_trim::Editor::default(),
                    timer: timer_resolution::Editor::default(),
                    priority_separation: win32_priority_separation::Editor::default(),
                    power_tuning: advanced_power_plan_tuning::Editor::default(),
                    effective_power_mode: crate::power::EffectivePowerModeMonitor::new().ok(),
                    time_rules: power_rules::Editor::default(),
                    cpu_rules: power_rules::Editor::default(),
                    priority: priority_control::Editor::default(),
                    efficiency: background_efficiency::Editor::default(),
                    soft_allocation: cpu_allocation::Editor::default(),
                    hard_allocation: cpu_allocation::Editor::default(),
                    adaptive: adaptive_engine::Editor::default(),
                    candidates: Vec::new(),
                    unavailable_candidates: Vec::new(),
                    catalog_loading: false,
                    settings,
                    runtime: std::sync::Arc::new(runtime),
                    status: RuntimeStatusSnapshot::default(),
                    error_message: error
                        .or_else(crate::crash_recovery::startup_error)
                        .unwrap_or_default(),
                    page: Page::Home,
                    navigation_history: navigation::History::default(),
                    breadcrumb: vec![Page::Home],
                    restore_event,
                    window: None,
                    auto_exclusion_generation: 0,
                    auto_exclusion_retry: false,
                    closing: false,
                    hwnd: None,
                    color_dialog_open: false,
                    tray: None,
                    tray_attempt: None,
                    shutdown_failed: false,
                    exiting: false,
                    hidden: false,
                    processes: process_list::ProcessList::default(),
                    cpu_limiter: cpu_limiter::CpuLimiter::default(),
                    power_source: PowerSourceProfile::PluggedIn,
                    power_plans: Vec::new(),
                    power_plans_loading: false,
                    power_plans_loaded: false,
                    foreground_plans: process_power_plans::Editor::default(),
                    running_app_plans: process_power_plans::Editor::default(),
                    activity_inputs: by_activity::Inputs::default(),
                },
                Task::batch([
                    iced::window::latest().map(Message::Window),
                    Task::run(events, |_| Message::Wake),
                ]),
            )
        },
        WinderustApp::update,
        WinderustApp::view,
    )
    .title("Winderust")
    .window(iced::window::Settings {
        size: iced::Size::new(1120.0, 760.0),
        min_size: Some(iced::Size::new(900.0, 620.0)),
        decorations: true,
        exit_on_close_request: false,
        ..Default::default()
    })
    .settings(iced::Settings {
        default_font: design::typography::FONT,
        default_text_size: design::typography::BODY.into(),
        ..Default::default()
    })
    .theme(|app: &WinderustApp| app.appearance.clone())
    .subscription(|app: &WinderustApp| {
        Subscription::batch([
            ui_tick_interval(
                app.hidden,
                app.auto_exclusion_retry || app.tray_retry_pending(),
            )
            .map(|interval| iced::time::every(interval).map(|_| Message::Tick))
            .unwrap_or_else(Subscription::none),
            iced::window::close_requests().map(|_| Message::WindowClose),
            iced::event::listen_with(|event, status, _| {
                if status != iced::event::Status::Ignored {
                    return None;
                }
                match event {
                    iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                        iced::mouse::Button::Back,
                    )) => Some(Message::NavigateHistory(false)),
                    iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                        iced::mouse::Button::Forward,
                    )) => Some(Message::NavigateHistory(true)),
                    _ => None,
                }
            }),
            if app.page == Page::ProcessList && app.processes.resizing_columns() {
                iced::event::listen_raw(|event, _, _| match event {
                    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => Some(
                        Message::Processes(process_list::Message::ResizeMoved(position.x)),
                    ),
                    iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                        iced::mouse::Button::Left,
                    ))
                    | iced::Event::Window(iced::window::Event::Unfocused) => {
                        Some(Message::Processes(process_list::Message::ResizeEnd))
                    }
                    _ => None,
                })
            } else {
                Subscription::none()
            },
            if app.page == Page::ProcessList {
                iced::event::listen_with(|event, status, _| match event {
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Delete),
                        modifiers,
                        ..
                    }) if status == iced::event::Status::Ignored && modifiers.is_empty() => {
                        Some(Message::Processes(process_list::Message::DeleteFocused))
                    }
                    _ => None,
                })
            } else {
                Subscription::none()
            },
        ])
    })
    .run()
}

struct WinderustApp {
    #[cfg(feature = "render-smoke")]
    smoke: Option<smoke::Run>,
    appearance: Theme,
    navigation_search: String,
    expanded_section: Option<Page>,
    status_collapsed: bool,
    feature_info_expanded: bool,
    preferences: settings_pages::Editor,
    home: home::Model,
    action_log: action_log::Editor,
    sampler: std::sync::Arc<std::sync::Mutex<home::Sampler>>,
    sampled_at: std::time::Instant,
    sampling: bool,
    process_sampled_at: std::time::Instant,
    catalog_sampled_at: std::time::Instant,
    suspension: app_suspension::Editor,
    trim: memory_trim::Editor,
    timer: timer_resolution::Editor,
    priority_separation: win32_priority_separation::Editor,
    power_tuning: advanced_power_plan_tuning::Editor,
    effective_power_mode: Option<crate::power::EffectivePowerModeMonitor>,
    time_rules: power_rules::Editor,
    cpu_rules: power_rules::Editor,
    priority: priority_control::Editor,
    efficiency: background_efficiency::Editor,
    soft_allocation: cpu_allocation::Editor,
    hard_allocation: cpu_allocation::Editor,
    adaptive: adaptive_engine::Editor,
    candidates: Vec<super::app_picker::Candidate>,
    unavailable_candidates: Vec<String>,
    catalog_loading: bool,
    settings: SettingsEditor,
    runtime: std::sync::Arc<RuntimeHandle>,
    status: RuntimeStatusSnapshot,
    error_message: String,
    page: Page,
    navigation_history: navigation::History,
    breadcrumb: Vec<Page>,
    restore_event: Option<SingleInstanceRestoreEvent>,
    window: Option<iced::window::Id>,
    auto_exclusion_generation: u64,
    auto_exclusion_retry: bool,
    closing: bool,
    hwnd: Option<usize>,
    color_dialog_open: bool,
    tray: Option<tray::TrayIcon>,
    tray_attempt: Option<((bool, bool), std::time::Instant)>,
    shutdown_failed: bool,
    exiting: bool,
    hidden: bool,
    processes: process_list::ProcessList,
    cpu_limiter: cpu_limiter::CpuLimiter,
    power_source: PowerSourceProfile,
    power_plans: Vec<crate::power::PowerPlan>,
    power_plans_loading: bool,
    power_plans_loaded: bool,
    foreground_plans: process_power_plans::Editor,
    running_app_plans: process_power_plans::Editor,
    activity_inputs: by_activity::Inputs,
}

#[derive(Debug, Clone)]
enum Message {
    Wake,
    #[cfg(feature = "render-smoke")]
    SmokeScreenshot(iced::window::Screenshot),
    NavigationSearch(String),
    DismissError,
    NavigateHistory(bool),
    ToggleNavigation,
    ToggleSection(Page),
    ToggleFeatureInfo,
    ToggleStatus,
    Preferences(settings_pages::Message),
    ColorChosen(Option<u32>, Result<Option<u32>, String>),
    Status(status_rail::Message),
    Home(home::Message),
    Sample(Result<home::Sample, String>),
    ActionLog(action_log::Message),
    ExportLog(Option<PathBuf>),
    Suspension(app_suspension::Message),
    Trim(memory_trim::Message),
    Timer(timer_resolution::Message),
    PrioritySeparation(win32_priority_separation::Message),
    PowerTuning(advanced_power_plan_tuning::Message),
    CommandFinished(Result<(), String>),

    PowerRules(power_rules::Kind, power_rules::Message),
    Priority(priority_control::Kind, priority_control::Message),
    Efficiency(background_efficiency::Message),
    Allocation(cpu_allocation::Kind, cpu_allocation::Message),
    Adaptive(adaptive_engine::Message),
    Catalog(Result<Vec<super::app_picker::Candidate>, String>),
    ExecutableChosen(Page, PowerSourceProfile, Option<PathBuf>),
    Page(Page),
    Tick,
    Window(Option<iced::window::Id>),
    NativeWindow(Option<usize>),
    WindowClose,
    Close,
    Save,
    Cancel,
    PausePowerPlans(bool),
    SettingsFile(FileDialogMode),
    SettingsFileChosen(FileDialogMode, Option<PathBuf>),
    Stay,
    DiscardAndClose,
    ShutdownFinished(Result<(), String>),
    Processes(process_list::Message),
    CpuLimiter(cpu_limiter::Message),
    PowerSource(PowerSourceProfile),
    ByActivity(by_activity::Message),
    ProcessPowerPlans(process_power_plans::Kind, process_power_plans::Message),
    PowerPlans(Result<Vec<crate::power::PowerPlan>, String>),
}

impl Message {
    fn allowed_during_exit(&self) -> bool {
        matches!(
            self,
            Self::ShutdownFinished(_)
                | Self::Sample(_)
                | Self::Catalog(_)
                | Self::PowerPlans(_)
                | Self::Processes(process_list::Message::Loaded(_))
                | Self::Preferences(settings_pages::Message::Checked(..))
        )
    }
}

impl WinderustApp {
    fn update(&mut self, message: Message) -> Task<Message> {
        if self.exiting && !message.allowed_during_exit() {
            // Discard native picker edits, but release its pending flag.
            if matches!(message, Message::ColorChosen(..)) {
                self.color_dialog_open = false;
            }
            return Task::none();
        }
        match message {
            #[cfg(feature = "render-smoke")]
            Message::SmokeScreenshot(screenshot) => return smoke::captured(self, screenshot),
            Message::DismissError => self.error_message.clear(),
            Message::NavigationSearch(value) => self.navigation_search = value,
            Message::ToggleNavigation => {
                let patch = crate::application::NavigationCollapsedPatch {
                    base_revision: self.settings.base_revision(),
                    navigation_collapsed: !self.settings.global().general.navigation_collapsed,
                };
                match self.settings.apply_navigation_collapsed_patch(patch) {
                    Ok(_) => self.publish_settings(),
                    Err(error) => self.error_message = error.to_string(),
                }
            }
            Message::ToggleFeatureInfo => self.feature_info_expanded = !self.feature_info_expanded,
            Message::ToggleSection(page) => {
                if self.page != page {
                    return self.update(Message::Page(page));
                }
                self.expanded_section = (self.expanded_section != Some(page)).then_some(page);
            }
            Message::ToggleStatus => self.status_collapsed = !self.status_collapsed,
            Message::Status(status_rail::Message::RelaunchAdmin) => {
                if crate::privilege::relaunch_as_admin() {
                    return self.update(Message::Close);
                }
            }
            Message::Preferences(settings_pages::Message::Open(url)) => {
                if let Err(error) = crate::win_util::open_url(&url) {
                    self.error_message = error;
                }
            }
            Message::Home(home::Message::Navigate(page)) => {
                return self.update(Message::Page(page))
            }
            Message::Home(home::Message::PauseMetrics(value)) => {
                self.home.metrics_paused = value;
            }
            Message::Sample(result) => {
                self.sampling = false;
                match result {
                    Ok(sample) if !self.home.metrics_paused => self.home.record(sample),
                    Ok(_) => {}
                    Err(error) => self.error_message = error,
                }
            }
            Message::Preferences(settings_pages::Message::Export) => {
                return self.update(Message::SettingsFile(FileDialogMode::Save))
            }
            Message::Preferences(settings_pages::Message::Import) => {
                return self.update(Message::SettingsFile(FileDialogMode::Open))
            }
            Message::Preferences(
                message @ (settings_pages::Message::Check | settings_pages::Message::CheckStartup),
            ) => {
                let automatic = matches!(message, settings_pages::Message::CheckStartup);
                let channel = self.settings.global().general.update_channel;
                if !self.preferences.begin_check(channel, automatic) {
                    return Task::none();
                }
                return tasks::run(move || {
                    crate::update_checker::check(channel)
                        .map(|r| (r.latest_version, r.available_update.map(|u| u.url)))
                        .map_err(|_| t!("about.update_check_failed").to_string())
                })
                .map(move |r| {
                    Message::Preferences(settings_pages::Message::Checked(
                        channel,
                        r.and_then(|r| r),
                    ))
                });
            }
            Message::Preferences(
                message @ (settings_pages::Message::OpenColorPicker
                | settings_pages::Message::EditColor(_)),
            ) => {
                let original = match message {
                    settings_pages::Message::EditColor(color) => Some(color),
                    _ => None,
                };
                if self.color_dialog_open {
                    return Task::none();
                }
                if let Some(owner) = self.hwnd {
                    let accent = &self.settings.global().general.accent;
                    let color = original.unwrap_or(accent.custom_color);
                    let custom_colors = accent.custom_colors.clone();
                    self.color_dialog_open = true;
                    return tasks::run(move || {
                        crate::file_dialog::choose_color(owner as isize, color, &custom_colors)
                    })
                    .map(move |result| {
                        Message::ColorChosen(original, result.and_then(|color| color))
                    });
                }
            }
            Message::ColorChosen(original, result) => {
                self.color_dialog_open = false;
                match result {
                    Ok(Some(color)) => {
                        return self.update(Message::Preferences(match original {
                            Some(original) => {
                                settings_pages::Message::ReplaceColor(original, color)
                            }
                            None => settings_pages::Message::Accent(color),
                        }))
                    }
                    Ok(None) => {}
                    Err(error) => self.error_message = error,
                }
            }
            Message::Preferences(message) => {
                self.settings
                    .edit_global(|settings| self.preferences.update(settings, message));
                self.appearance = settings_pages::theme(&self.settings.global().general);
                self.sync_tray();
            }
            Message::ActionLog(action_log::Message::LogMode(value)) => {
                self.settings
                    .edit_global(|settings| settings.advanced.action_log_mode = value);
            }
            Message::ActionLog(action_log::Message::Clear) => {
                self.runtime.clear_action_log();
                self.status.action_log_entries = Default::default();
                self.status.action_log_summaries = Default::default();
                self.action_log.update(action_log::Message::Clear);
                return iced::widget::operation::scroll_to(
                    "action-log",
                    iced::widget::scrollable::AbsoluteOffset {
                        x: None,
                        y: Some(0.0),
                    },
                );
            }
            Message::ActionLog(action_log::Message::Export) => {
                return Task::perform(
                    crate::file_dialog::choose_action_log_export_file(
                        self.hwnd.map(|h| h as windows_sys::Win32::Foundation::HWND),
                    ),
                    Message::ExportLog,
                )
            }
            Message::ActionLog(message) => {
                let reset = matches!(
                    message,
                    action_log::Message::Result(..)
                        | action_log::Message::Feature(..)
                        | action_log::Message::AllResults(_)
                        | action_log::Message::AllFeatures(_)
                );
                self.action_log.update(message);
                if reset {
                    return iced::widget::operation::scroll_to(
                        "action-log",
                        iced::widget::scrollable::AbsoluteOffset {
                            x: None,
                            y: Some(0.0),
                        },
                    );
                }
            }
            Message::ExportLog(path) => {
                if let Some(path) = path {
                    if let Err(error) = crate::config::storage::write_bytes_atomically(
                        &path,
                        action_log::action_log_entries_to_csv(&self.status.action_log_entries)
                            .as_bytes(),
                    ) {
                        self.error_message = t!(
                            "status.action_log_export_failed",
                            path = path.display(),
                            error = error
                        )
                        .to_string();
                    }
                }
            }

            Message::CommandFinished(result) => {
                if let Err(error) = result {
                    self.error_message = error;
                }
            }
            Message::Suspension(app_suspension::Message::Browse) => {
                return self.browse(Page::AppSuspension)
            }
            Message::Suspension(message) => {
                if let Some((path, freeze)) = self.suspension.update(
                    &mut self.settings.app_suspension,
                    &self.status.feature_status.app_suspension,
                    &self.unavailable_candidates,
                    message,
                ) {
                    match self
                        .runtime
                        .request_app_suspension_path_action(&path, freeze)
                    {
                        Ok(receiver) => {
                            return tasks::run(move || {
                                receiver
                                    .recv()
                                    .map_err(|e| e.to_string())
                                    .and_then(|r| r.map(|_| ()).map_err(|e| e.to_string()))
                            })
                            .map(|r| Message::CommandFinished(r.and_then(|r| r)))
                        }
                        Err(error) => self.error_message = error.to_string(),
                    }
                }
            }
            Message::Trim(memory_trim::Message::Browse) => return self.browse(Page::MemoryTrim),
            Message::Trim(memory_trim::Message::TrimNow) => {
                match self.runtime.request_memory_trim_now() {
                    Ok(receiver) => {
                        return tasks::run(move || {
                            receiver
                                .recv()
                                .map_err(|e| e.to_string())
                                .and_then(|r| r.map(|_| ()).map_err(|e| e.to_string()))
                        })
                        .map(|r| Message::CommandFinished(r.and_then(|r| r)))
                    }
                    Err(error) => self.error_message = error.to_string(),
                }
            }
            Message::Trim(message) => self.trim.update(&mut self.settings.memory_trim, message),
            Message::Timer(timer_resolution::Message::Browse) => {
                return self.browse(Page::TimerResolution)
            }
            Message::Timer(message) => self.timer.update(
                &mut self.settings.timer_resolution,
                &self.status.feature_status.timer_resolution,
                message,
            ),
            Message::PrioritySeparation(message) => self.priority_separation.update(message),
            Message::PowerTuning(message) => self.settings.edit_with_presets(|settings| {
                self.power_tuning.update(
                    &mut settings.advanced_power_plan_tuning_presets,
                    &self.power_plans,
                    message,
                )
            }),

            Message::PowerRules(kind, message) => {
                let editor = match kind {
                    power_rules::Kind::Time => &mut self.time_rules,
                    power_rules::Kind::CpuLoad => &mut self.cpu_rules,
                };
                editor.update(kind, &mut self.settings, &self.power_plans, message);
            }
            Message::Priority(kind, priority_control::Message::Browse) => {
                return self.browse(priority_page(kind))
            }
            Message::Priority(kind, message) => {
                self.priority.update(&mut self.settings, kind, message)
            }
            Message::Efficiency(background_efficiency::Message::Browse) => {
                return self.browse(Page::BackgroundEfficiency)
            }
            Message::Efficiency(message) => self.efficiency.update(&mut self.settings, message),
            Message::Allocation(_, cpu_allocation::Message::Status(message)) => {
                return self.update(Message::Status(message))
            }
            Message::Allocation(kind, cpu_allocation::Message::Browse) => {
                return self.browse(match kind {
                    cpu_allocation::Kind::Soft => Page::CpuSetsSoft,
                    cpu_allocation::Kind::Hard => Page::ProcessorAffinityHard,
                })
            }
            Message::Allocation(kind, message) => {
                self.settings.edit_with_presets(|settings| match kind {
                    cpu_allocation::Kind::Soft => {
                        self.soft_allocation.update(settings, kind, message)
                    }
                    cpu_allocation::Kind::Hard => {
                        self.hard_allocation.update(settings, kind, message)
                    }
                })
            }
            Message::Adaptive(adaptive_engine::Message::Status(message)) => {
                return self.update(Message::Status(message))
            }
            Message::Adaptive(adaptive_engine::Message::Browse) => {
                return self.browse(Page::AdaptiveEngine)
            }
            Message::Adaptive(message) => self
                .settings
                .edit_with_presets(|settings| self.adaptive.update(settings, message)),
            Message::Catalog(result) => {
                self.catalog_loading = false;
                match result {
                    Ok(candidates) if !self.processes.population_paused => {
                        self.unavailable_candidates = candidates
                            .iter()
                            .filter(|candidate| !candidate.info.has_suspendable_instance)
                            .map(|candidate| {
                                candidate.info.image_path.to_string_lossy().into_owned()
                            })
                            .collect();
                        self.candidates = candidates;
                    }
                    Ok(_) => {}
                    Err(error) => self.error_message = error,
                }
            }
            Message::ExecutableChosen(page, profile, path) => {
                if profile == self.power_source {
                    if let Some(path) = path {
                        let path = path.to_string_lossy().into_owned();
                        if let Some(kind) = priority_kind(page) {
                            return self.update(Message::Priority(
                                kind,
                                priority_control::Message::Path(path),
                            ));
                        }
                        match page {
                            Page::AppSuspension => {
                                return self.update(Message::Suspension(
                                    app_suspension::Message::Path(path),
                                ))
                            }
                            Page::MemoryTrim => {
                                return self.update(Message::Trim(memory_trim::Message::Path(path)))
                            }
                            Page::TimerResolution => {
                                return self
                                    .update(Message::Timer(timer_resolution::Message::Path(path)))
                            }
                            Page::CpuSetsSoft => {
                                return self.update(Message::Allocation(
                                    cpu_allocation::Kind::Soft,
                                    cpu_allocation::Message::Path(path),
                                ))
                            }
                            Page::ProcessorAffinityHard => {
                                return self.update(Message::Allocation(
                                    cpu_allocation::Kind::Hard,
                                    cpu_allocation::Message::Path(path),
                                ))
                            }
                            Page::BackgroundEfficiency => {
                                return self.update(Message::Efficiency(
                                    background_efficiency::Message::Path(path),
                                ))
                            }
                            Page::AdaptiveEngine => {
                                return self.update(Message::Adaptive(
                                    adaptive_engine::Message::Path(path),
                                ))
                            }
                            Page::CpuLimiter => {
                                return self
                                    .update(Message::CpuLimiter(cpu_limiter::Message::Path(path)))
                            }
                            Page::ByForeground => {
                                return self.update(Message::ProcessPowerPlans(
                                    process_power_plans::Kind::Foreground,
                                    process_power_plans::Message::Path(path),
                                ))
                            }
                            Page::ByRunningApp => {
                                return self.update(Message::ProcessPowerPlans(
                                    process_power_plans::Kind::RunningApp,
                                    process_power_plans::Message::Path(path),
                                ))
                            }
                            _ => {}
                        }
                    }
                }
            }
            Message::ProcessPowerPlans(kind, process_power_plans::Message::Browse) => {
                return self.browse(match kind {
                    process_power_plans::Kind::Foreground => Page::ByForeground,
                    process_power_plans::Kind::RunningApp => Page::ByRunningApp,
                })
            }
            Message::ProcessPowerPlans(kind, message) => {
                let editor = match kind {
                    process_power_plans::Kind::Foreground => &mut self.foreground_plans,
                    process_power_plans::Kind::RunningApp => &mut self.running_app_plans,
                };
                editor.update(kind, &mut self.settings, &self.power_plans, message);
            }
            Message::ByActivity(message) => {
                self.activity_inputs.edit(&message);
                by_activity::update(&mut self.settings, message);
            }
            Message::PowerPlans(result) => {
                self.power_plans_loading = false;
                match result {
                    Ok(plans) => {
                        self.power_plans = plans;
                        self.power_plans_loaded = true;
                        if self.page == Page::AdvancedPowerPlanTuning {
                            self.power_tuning.ensure_plan(&self.power_plans);
                        }
                    }
                    Err(error) => self.error_message = error,
                }
            }
            Message::CpuLimiter(cpu_limiter::Message::Browse) => {
                return self.browse(Page::CpuLimiter)
            }
            Message::CpuLimiter(message) => self
                .cpu_limiter
                .update(&mut self.settings.cpu_limiter, message),
            Message::PowerSource(_) if self.pending_editor() || self.invalid_inputs() => {
                self.error_message = t!("unsaved.message").to_string();
            }
            Message::PowerSource(source) => {
                self.power_source = source;
                self.settings.select_power_source(source);
                self.reset_editors();
            }
            Message::Window(window) => {
                self.window = window;
                if let Some(window) = window {
                    return iced::window::run(window, |window| {
                        use raw_window_handle::RawWindowHandle;
                        match window.window_handle().ok()?.as_raw() {
                            RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as usize),
                            _ => None,
                        }
                    })
                    .map(Message::NativeWindow);
                }
            }
            Message::NativeWindow(hwnd) => {
                self.hwnd = hwnd;
                if let (Some(event), Some(hwnd)) = (self.restore_event.take(), hwnd) {
                    event.listen(hwnd as windows_sys::Win32::Foundation::HWND);
                }
                self.sync_tray();
                let update = if self.settings.global().general.check_for_updates {
                    self.update(Message::Preferences(settings_pages::Message::CheckStartup))
                } else {
                    Task::none()
                };
                if self.settings.persisted().general.start_minimized {
                    if let (Some(hwnd), Some(_)) = (self.hwnd, &self.tray) {
                        tray::hide_window(hwnd as windows_sys::Win32::Foundation::HWND);
                    } else if let Some(window) = self.window {
                        return Task::batch([iced::window::minimize(window, true), update]);
                    }
                }
                return update;
            }
            Message::NavigateHistory(forward) => {
                if !self.closing && self.error_message.is_empty() && !self.pending_editor() {
                    if let Some(page) = self.navigation_history.travel(forward) {
                        return self.update(Message::Page(page));
                    }
                }
            }
            Message::Page(page) => {
                self.navigation_history.visit(page);
                self.feature_info_expanded = false;
                let path = navigation::breadcrumb_path(page);
                self.breadcrumb = path;
                self.page = page;
                self.expanded_section = Some(page.section_landing_page());
                if page == Page::Win32PrioritySeparation {
                    self.priority_separation.refresh();
                }
                if page == Page::AdvancedPowerPlanTuning {
                    self.power_tuning.ensure_plan(&self.power_plans);
                }
                let mut tasks = Vec::new();
                if page_needs_power_plans(page) {
                    tasks.push(self.load_power_plans());
                }
                if !self.processes.population_paused {
                    if page == Page::ProcessList {
                        tasks.push(
                            self.processes
                                .update(
                                    process_list::Message::Refresh,
                                    &mut self.settings,
                                    &self.runtime,
                                )
                                .map(Message::Processes),
                        );
                    }
                    if (page.supports_power_source_profiles() || page == Page::ActionLog)
                        && !self.catalog_loading
                    {
                        self.catalog_loading = true;
                        tasks.push(
                            tasks::run({
                                let cached = self.candidates.clone();
                                move || super::app_picker::load(cached)
                            })
                            .map(|result| Message::Catalog(result.and_then(|result| result))),
                        );
                    }
                }
                return Task::batch(tasks);
            }
            Message::Processes(process_list::Message::Details(
                process_list::details::Message::ReloadPlans,
            )) => {
                return self.load_power_plans();
            }
            Message::Processes(message) => {
                if self.processes.population_paused
                    && matches!(
                        message,
                        process_list::Message::Refresh | process_list::Message::Loaded(_)
                    )
                {
                    self.processes.discard(message);
                } else {
                    return self
                        .processes
                        .update(message, &mut self.settings, &self.runtime)
                        .map(Message::Processes);
                }
            }
            Message::Wake => {
                self.sync_tray();
                return self.update(Message::Tick);
            }
            Message::Tick => {
                #[cfg(feature = "render-smoke")]
                if let Some(task) = smoke::advance(self) {
                    return task;
                }
                let mut tray_error = false;
                for action in tray::take_menu_actions() {
                    let result = match action {
                        tray::MenuAction::MasterSwitch(enabled) => {
                            self.settings.set_master_enabled(enabled)
                        }
                        tray::MenuAction::Feature {
                            id,
                            profile,
                            enabled,
                        } => {
                            let field = Page::sections()
                                .iter()
                                .flat_map(|section| section.pages)
                                .find(|page| **page as usize == id)
                                .and_then(|page| navigation::feature_toggle(*page));
                            let Some(field) = field else {
                                self.error_message = "Unknown tray feature toggle.".into();
                                tray_error = true;
                                continue;
                            };
                            self.settings.set_feature_enabled(profile, field, enabled)
                        }
                    };
                    match result {
                        Ok(_) => self.publish_settings(),
                        Err(error) => {
                            self.error_message = error.to_string();
                            tray_error = true;
                        }
                    }
                }
                if self.tray_retry_pending() {
                    self.sync_tray();
                }
                if tray_error {
                    return self.show_window();
                }
                if tray::take_exit_requested() {
                    return self.update(Message::Close);
                }
                let restore = tray::take_restore_requested();
                let hidden = tray::is_hidden_to_tray();
                let mut work = Vec::new();
                if hidden != self.hidden || restore {
                    self.hidden = hidden;
                    if let Some(window) = self.window {
                        work.push(iced::window::set_mode(
                            window,
                            if hidden {
                                iced::window::Mode::Hidden
                            } else {
                                iced::window::Mode::Windowed
                            },
                        ));
                    }
                }
                if let Some(status) = self.runtime.status_snapshot_since(self.status.generation) {
                    if status.appearance_change_generation
                        != self.status.appearance_change_generation
                    {
                        self.appearance = settings_pages::theme(&self.settings.global().general);
                    }
                    self.processes.sync_suspended_processes(
                        &status.feature_status.app_suspension.suspended_process_ids,
                    );
                    self.status = status;
                }
                if let Some(patch) = self
                    .runtime
                    .take_auto_exclusion_patch_since(&mut self.auto_exclusion_generation)
                {
                    if let Err(error) = self.settings.apply_auto_exclusion_patch(&patch) {
                        self.error_message = error.to_string();
                        self.runtime.requeue_auto_exclusion_patch(patch);
                        self.auto_exclusion_retry = true;
                    } else {
                        self.auto_exclusion_retry = false;
                        self.publish_settings();
                    }
                }
                if !self.hidden && !self.processes.population_paused {
                    if self.page == Page::ProcessList
                        && self.process_sampled_at.elapsed() >= Duration::from_secs(1)
                    {
                        self.process_sampled_at = std::time::Instant::now();
                        work.push(
                            self.processes
                                .update(
                                    process_list::Message::Refresh,
                                    &mut self.settings,
                                    &self.runtime,
                                )
                                .map(Message::Processes),
                        );
                    }
                    if (self.page.supports_power_source_profiles() || self.page == Page::ActionLog)
                        && !self.catalog_loading
                        && self.catalog_sampled_at.elapsed() >= Duration::from_secs(3)
                    {
                        self.catalog_loading = true;
                        self.catalog_sampled_at = std::time::Instant::now();
                        work.push(
                            tasks::run({
                                let cached = self.candidates.clone();
                                move || super::app_picker::load(cached)
                            })
                            .map(|r| Message::Catalog(r.and_then(|r| r))),
                        );
                    }
                }
                if self.page == Page::Home
                    && !self.hidden
                    && !self.home.metrics_paused
                    && !self.sampling
                    && self.sampled_at.elapsed() >= Duration::from_secs(1)
                {
                    self.sampling = true;
                    self.sampled_at = std::time::Instant::now();
                    let sampler = self.sampler.clone();
                    work.push(
                        tasks::run(move || {
                            sampler
                                .lock()
                                .map_err(|e| e.to_string())
                                .and_then(|mut sampler| sampler.sample())
                        })
                        .map(|r| Message::Sample(r.and_then(|r| r))),
                    );
                }
                return Task::batch(work);
            }
            Message::WindowClose => {
                if self.settings.global().general.hide_to_tray
                    && self.tray.as_ref().is_some_and(|icon| icon.is_registered())
                {
                    if let Some(hwnd) = self.hwnd {
                        tray::hide_window(hwnd as windows_sys::Win32::Foundation::HWND);
                    }
                    self.hidden = true;
                    if let Some(window) = self.window {
                        return iced::window::set_mode(window, iced::window::Mode::Hidden);
                    }
                } else {
                    return self.update(Message::Close);
                }
            }
            Message::Close => {
                if !self.closing {
                    self.error_message.clear();
                }
                self.closing = true;
                return self.show_window();
            }
            Message::Save if self.pending_editor() => {
                self.error_message = t!("unsaved.message").to_string();
            }
            Message::Save if self.adaptive.validation_error().is_some() => {
                self.error_message = self.adaptive.validation_error().unwrap_or_default();
            }
            Message::Save if self.cpu_limiter.has_invalid_inputs() => {
                self.error_message = t!("cpu_limiter.invalid_limit").to_string();
            }
            Message::Save if !self.time_rules.valid() || !self.cpu_rules.valid() => {
                self.error_message = t!("unsaved.message").to_string();
            }
            Message::Save if !self.activity_inputs.valid() => {
                self.error_message = t!("by_activity.invalid_timing").to_string();
            }
            Message::Save
                if self.timer.has_invalid_inputs() || self.preferences.has_invalid_inputs() =>
            {
                self.error_message = t!("unsaved.message").to_string();
            }
            Message::Save => match self.settings.save() {
                Ok(outcome) => {
                    if let Some(error) = outcome.startup_registration_error() {
                        self.error_message = error.to_string();
                    }
                    self.publish_settings();
                    if self.power_tuning.dirty && !self.power_tuning.apply() {
                        self.error_message = self.power_tuning.status.clone();
                        return Task::none();
                    }
                    self.reset_editors();
                    if self.closing {
                        return self.shutdown();
                    }
                }
                Err(error) => self.error_message = error.to_string(),
            },
            Message::Cancel => {
                self.settings.cancel();
                if self.power_tuning.dirty {
                    self.power_tuning.refresh();
                }
                self.reset_editors();
                rust_i18n::set_locale(self.settings.global().general.language.locale());
                self.publish_settings();
                self.closing = false;
            }
            Message::PausePowerPlans(value) => self.settings.edit_global(|settings| {
                settings.general.pause_power_plan_switching_while_plugged_in = value
            }),
            Message::ShutdownFinished(result) => {
                self.exiting = false;
                if let Err(error) = result {
                    crate::backend::diagnostics::error(&error);
                    self.shutdown_failed = true;
                    self.error_message = error;
                    return self.show_window();
                }
                return self.finish_exit();
            }
            Message::Stay => {
                self.closing = false;
                self.error_message.clear();
            }
            Message::DiscardAndClose => {
                self.settings.cancel();
                if self.power_tuning.dirty {
                    self.power_tuning.refresh();
                }
                return self.shutdown();
            }
            Message::SettingsFile(_) if self.pending_editor() || self.invalid_inputs() => {
                self.error_message = t!("unsaved.message").to_string();
            }
            Message::SettingsFile(mode) => {
                let hwnd = self
                    .hwnd
                    .map(|hwnd| hwnd as windows_sys::Win32::Foundation::HWND);
                return Task::perform(choose_settings_file(hwnd, mode), move |path| {
                    Message::SettingsFileChosen(mode, path)
                });
            }
            Message::SettingsFileChosen(mode, path) => {
                if let Some(path) = path {
                    match mode {
                        FileDialogMode::Open => match self.settings.import_toml_from(&path) {
                            Ok(outcome) => {
                                self.reset_editors();
                                if let Some(error) = outcome.startup_registration_error() {
                                    self.error_message = error.to_string();
                                }
                                rust_i18n::set_locale(
                                    self.settings.global().general.language.locale(),
                                );
                                self.publish_settings();
                            }
                            Err(error) => self.error_message = error.to_string(),
                        },
                        FileDialogMode::Save => {
                            if let Err(error) = self.settings.export_toml_to(&path) {
                                self.error_message = error.to_string();
                            }
                        }
                    }
                }
            }
        }
        Task::none()
    }

    fn pending_changes(&self) -> bool {
        self.settings.has_unsaved_changes()
            || self.power_tuning.dirty
            || self.pending_editor()
            || self.invalid_inputs()
    }
    fn invalid_inputs(&self) -> bool {
        self.cpu_limiter.has_invalid_inputs()
            || self.timer.has_invalid_inputs()
            || self.preferences.has_invalid_inputs()
            || self.adaptive.validation_error().is_some()
            || !self.activity_inputs.valid()
            || !self.time_rules.valid()
            || !self.cpu_rules.valid()
    }
    fn show_window(&mut self) -> Task<Message> {
        self.hidden = false;
        if let Some(hwnd) = self.hwnd {
            tray::show_window(hwnd as windows_sys::Win32::Foundation::HWND);
        }
        self.window
            .map(|window| iced::window::set_mode(window, iced::window::Mode::Windowed))
            .unwrap_or_else(Task::none)
    }
    fn pending_editor(&self) -> bool {
        self.power_tuning.has_pending_editor()
            || self.adaptive.has_pending_editor()
            || self.soft_allocation.has_pending_editor()
            || self.hard_allocation.has_pending_editor()
            || self.cpu_limiter.has_pending_editor()
            || self.time_rules.has_pending_editor()
            || self.cpu_rules.has_pending_editor()
    }
    fn reset_editors(&mut self) {
        self.preferences.reset_drafts();
        self.power_tuning.discard_editor();
        self.cpu_limiter = Default::default();
        self.activity_inputs = Default::default();
        self.foreground_plans = Default::default();
        self.running_app_plans = Default::default();
        self.priority = Default::default();
        self.efficiency = Default::default();
        self.soft_allocation.reset_drafts();
        self.hard_allocation.reset_drafts();
        self.adaptive.reset_drafts();
        self.time_rules = Default::default();
        self.cpu_rules = Default::default();
        self.suspension = Default::default();
        self.trim = Default::default();
        self.timer = Default::default();
        self.appearance = settings_pages::theme(&self.settings.global().general);
    }
    fn browse(&self, page: Page) -> Task<Message> {
        let profile = self.power_source;
        Task::perform(
            crate::file_dialog::choose_executable_file(
                self.hwnd.map(|h| h as windows_sys::Win32::Foundation::HWND),
            ),
            move |path| Message::ExecutableChosen(page, profile, path),
        )
    }

    fn publish_settings(&mut self) {
        self.runtime
            .replace_settings(&self.settings.runtime_settings_snapshot());
        self.sync_tray();
    }

    fn load_power_plans(&mut self) -> Task<Message> {
        if self.power_plans_loading {
            return Task::none();
        }
        self.power_plans_loading = true;
        self.power_plans_loaded = false;
        tasks::run(crate::power::list_plans)
            .map(|result| Message::PowerPlans(result.and_then(|result| result)))
    }

    fn tray_retry_pending(&self) -> bool {
        (self.settings.global().general.hide_to_tray
            || self.settings.persisted().general.start_minimized)
            && self.tray.as_ref().is_none_or(|icon| !icon.is_registered())
    }

    fn sync_tray(&mut self) {
        let profile = if crate::backend::power_source::is_plugged_in() != Some(false) {
            PowerSourceProfile::PluggedIn
        } else {
            PowerSourceProfile::OnBattery
        };
        let saved = self.settings.persisted();
        let settings = if profile == PowerSourceProfile::OnBattery {
            saved.battery_profile()
        } else {
            saved
        };
        tray::set_menu_state(tray::MenuState {
            enabled: saved.general.enabled,
            profile,
            groups: Page::sections()
                .iter()
                .filter(|section| {
                    section.landing_page != Page::AdvancedControls
                        || saved.advanced.show_advanced_controls
                })
                .filter_map(|section| {
                    let items: Vec<_> = section
                        .pages
                        .iter()
                        .filter_map(|page| {
                            navigation::feature_page_enabled(settings, *page).map(|enabled| {
                                tray::FeatureToggle {
                                    id: *page as usize,
                                    label: page.label(),
                                    enabled,
                                }
                            })
                        })
                        .collect();
                    (!items.is_empty()).then(|| (section.landing_page.label(), items))
                })
                .collect(),
        });
        let intent = (
            self.settings.global().general.hide_to_tray,
            self.settings.persisted().general.start_minimized,
        );
        if !intent.0 && !intent.1 {
            tray::set_hide_on_close(false);
            self.tray = None;
            self.tray_attempt = None;
        } else {
            if tray::take_taskbar_created() {
                self.tray_attempt = None;
                if let Some(icon) = &mut self.tray {
                    icon.invalidate();
                }
            }
            let missing = self.tray.as_ref().is_none_or(|icon| !icon.is_registered());
            let now = std::time::Instant::now();
            if missing && tray_retry_due(self.tray_attempt, intent, now) {
                if let Some(hwnd) = self.hwnd {
                    let first_attempt = self.tray_attempt.is_none();
                    self.tray_attempt = Some((intent, now));
                    let result = if let Some(icon) = &mut self.tray {
                        icon.register()
                    } else {
                        tray::TrayIcon::install(hwnd as windows_sys::Win32::Foundation::HWND)
                            .map(|icon| self.tray = Some(icon))
                    };
                    if let Err(error) = result {
                        if first_attempt {
                            self.error_message = error;
                        }
                    }
                }
            }
        }
        tray::set_hide_on_close(
            intent.0 && self.tray.as_ref().is_some_and(|icon| icon.is_registered()),
        );
    }

    fn shutdown(&mut self) -> Task<Message> {
        // A confirmed subsequent exit hands the retained failure to the watchdog in main.
        if self.shutdown_failed {
            return self.finish_exit();
        }
        crate::backend::diagnostics::event("Exit requested; restoring managed state.");
        self.exiting = true;
        self.closing = true;
        self.error_message.clear();
        let runtime = self.runtime.clone();
        tasks::run(move || runtime.shutdown())
            .map(|result| Message::ShutdownFinished(result.and_then(|result| result)))
    }

    fn finish_exit(&mut self) -> Task<Message> {
        tray::set_hide_on_close(false);
        self.tray = None;
        tray::set_ui_wake(None);
        iced::exit()
    }

    fn view(&self) -> Element<'_, Message> {
        let content = self.view_content();
        if self.closing || !self.error_message.is_empty() {
            let mut actions =
                row![iced::widget::Space::new().width(Fill)].spacing(design::space::SMALL);
            let mut body = column![widgets::heading(
                t!(if self.exiting {
                    "exit_prompt.exiting"
                } else if self.closing {
                    "exit_prompt.title"
                } else {
                    "common.error"
                })
                .to_string(),
                design::typography::DIALOG_TITLE,
            )]
            .spacing(design::space::LARGE);
            if self.shutdown_failed {
                body = body.push(text(t!("exit_prompt.recovery_handoff").to_string()));
            }
            if self.exiting {
                body = body.push(text(t!("exit_prompt.processing").to_string()));
            } else if self.closing {
                body = body.push(text(
                    t!(if self.pending_changes() {
                        "exit_prompt.unsaved"
                    } else {
                        "exit_prompt.message"
                    })
                    .to_string(),
                ));
                actions = actions.push(
                    button(text(t!("common.cancel").to_string()))
                        .style(widgets::tertiary_button)
                        .on_press(Message::Stay),
                );
                if self.pending_changes() {
                    actions = actions
                        .push(
                            button(text(t!("exit_prompt.save_and_exit").to_string()))
                                .style(widgets::primary_button)
                                .on_press(Message::Save),
                        )
                        .push(
                            button(text(t!("exit_prompt.without_saving").to_string()))
                                .style(widgets::danger_button)
                                .on_press(Message::DiscardAndClose),
                        );
                } else {
                    actions = actions.push(
                        button(text(t!("tray.exit").to_string()))
                            .style(widgets::danger_button)
                            .on_press(Message::DiscardAndClose),
                    );
                }
            } else {
                actions = actions.push(
                    button(text(t!("common.done").to_string()))
                        .style(widgets::tertiary_button)
                        .on_press(Message::DismissError),
                );
            }
            if !self.error_message.is_empty() {
                body = body.push(text(&self.error_message));
            }
            let dialog = container(body.push(actions))
                .padding(design::space::LARGE as u16)
                .max_width(640)
                .style(widgets::surface);
            return iced::widget::stack![
                content,
                iced::widget::opaque(iced::widget::stack![
                    iced::widget::opaque(
                        container(iced::widget::Space::new())
                            .width(Fill)
                            .height(Fill)
                            .style(|_| container::Style {
                                background: Some(
                                    iced::Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()
                                ),
                                ..Default::default()
                            })
                    ),
                    container(iced::widget::opaque(dialog))
                        .padding(16)
                        .center_x(Fill)
                        .center_y(Fill),
                ]),
            ]
            .into();
        }
        if self.page == Page::ProcessList && self.processes.context_open() {
            iced::widget::stack![
                content,
                iced::widget::opaque(
                    iced::widget::mouse_area(iced::widget::Space::new().width(Fill).height(Fill))
                        .on_press(Message::Processes(process_list::Message::CloseSelection))
                        .on_right_press(Message::Processes(process_list::Message::CloseSelection))
                        .on_middle_press(Message::Processes(process_list::Message::CloseSelection))
                ),
            ]
            .into()
        } else {
            // Preserve the app subtree when the context-menu dismissal layer changes.
            iced::widget::stack![content].into()
        }
    }

    fn side_panel(&self, page: Page) -> Option<Element<'_, Message>> {
        if page == Page::ProcessList {
            Some(self.processes.side_panel().map(Message::Processes))
        } else if page == Page::ActionLog {
            Some(
                self.action_log
                    .side_panel(
                        self.settings.global().advanced.action_log_mode,
                        !self.status.action_log_entries.is_empty(),
                        !self.status.action_log_summaries.is_empty(),
                    )
                    .map(Message::ActionLog),
            )
        } else if page == Page::AdaptiveEngine {
            Some(
                self.adaptive
                    .side_panel(&self.settings, &self.status)
                    .map(Message::Adaptive),
            )
        } else if page == Page::AdvancedPowerPlanTuning {
            Some(
                self.power_tuning
                    .side_panel(&self.settings.advanced_power_plan_tuning_presets)
                    .map(Message::PowerTuning),
            )
        } else if page == Page::CpuSetsSoft {
            Some(
                self.soft_allocation
                    .side_panel(&self.settings, cpu_allocation::Kind::Soft, &self.status)
                    .map(|m| Message::Allocation(cpu_allocation::Kind::Soft, m)),
            )
        } else if page == Page::ProcessorAffinityHard {
            Some(
                self.hard_allocation
                    .side_panel(&self.settings, cpu_allocation::Kind::Hard, &self.status)
                    .map(|m| Message::Allocation(cpu_allocation::Kind::Hard, m)),
            )
        } else {
            status_rail::view(page, &self.settings, &self.status, &self.power_plans).map(|panel| {
                let panel = panel.map(Message::Status);
                if page == Page::MemoryTrim {
                    column![
                        panel,
                        iced::widget::rule::horizontal(1),
                        container(
                            button(
                                container(text(t!("memory_trim.trim_now").to_string()))
                                    .center_x(Fill)
                            )
                            .width(Fill)
                            .height(32)
                            .style(widgets::primary_button)
                            .on_press_maybe(
                                self.settings
                                    .memory_trim
                                    .enabled
                                    .then_some(Message::Trim(memory_trim::Message::TrimNow))
                            )
                        )
                        .padding([design::space::MEDIUM as u16, 0])
                    ]
                    .height(Fill)
                    .into()
                } else if matches!(page, Page::ByTime | Page::ByCpuLoad) {
                    let kind = if page == Page::ByTime {
                        power_rules::Kind::Time
                    } else {
                        power_rules::Kind::CpuLoad
                    };
                    column![
                        panel,
                        iced::widget::rule::horizontal(1),
                        container(
                            button(container(text(t!("common.create").to_string())).center_x(Fill))
                                .width(Fill)
                                .height(32)
                                .style(widgets::primary_button)
                                .on_press(Message::PowerRules(kind, power_rules::Message::Add))
                        )
                        .padding([design::space::MEDIUM as u16, 0])
                    ]
                    .height(Fill)
                    .into()
                } else {
                    panel
                }
            })
        }
    }

    fn view_content(&self) -> Element<'_, Message> {
        if self.preferences.show_update && !self.closing {
            return (container(
                column![
                    text(t!("about.updates").to_string()).size(design::typography::DIALOG_TITLE),
                    text(self.preferences.latest.clone().unwrap_or_default()),
                    row![
                        iced::widget::Space::new().width(Fill),
                        button(text(t!("about.download_update").to_string())).on_press_maybe(
                            self.preferences
                                .download
                                .clone()
                                .map(|url| Message::Preferences(settings_pages::Message::Open(
                                    url
                                )))
                        ),
                        button(text(t!("common.cancel").to_string()))
                            .style(crate::ui::widgets::tertiary_button)
                            .on_press(Message::Preferences(settings_pages::Message::DismissUpdate))
                    ]
                    .spacing(design::space::SMALL)
                ]
                .spacing(design::space::LARGE),
            )
            .padding(design::space::SECTION as u16))
            .into();
        }
        let collapsed = self.settings.global().general.navigation_collapsed;
        let mut navigation = column![]
            .spacing(design::space::TINY)
            .padding([design::space::SMALL as u16, design::space::CONTROL as u16])
            .width(Fill);
        if collapsed {
            navigation = navigation.push(
                button(
                    container(navigation::glyph("icons/search.svg"))
                        .center_x(Fill)
                        .center_y(Fill),
                )
                .width(Fill)
                .height(design::NAVIGATION_ROW_HEIGHT)
                .on_press(Message::ToggleNavigation)
                .style(widgets::quiet),
            );
        } else {
            navigation = navigation.push(
                container(widgets::search_field(
                    text_input(&t!("home.search_placeholder"), &self.navigation_search)
                        .padding(widgets::SEARCH_INPUT_PADDING)
                        .on_input(Message::NavigationSearch),
                    (!self.navigation_search.is_empty())
                        .then(|| Message::NavigationSearch(String::new())),
                ))
                .center_y(design::NAVIGATION_ROW_HEIGHT),
            );
        }
        let search_pages = navigation::dashboard_search_pages(
            &self.navigation_search,
            self.settings.global().advanced.show_advanced_controls,
        );
        let mut utilities = column![]
            .spacing(design::space::TINY)
            .padding([design::space::SMALL as u16, design::space::CONTROL as u16]);
        for section in Page::sections() {
            if section.landing_page == Page::AdvancedControls
                && !self.settings.global().advanced.show_advanced_controls
            {
                continue;
            }
            let matches = |page: Page| {
                self.navigation_search.is_empty()
                    || search_pages.contains(&page)
                    || page
                        .label()
                        .to_lowercase()
                        .contains(&self.navigation_search.to_lowercase())
            };
            let visible =
                matches(section.landing_page) || section.pages.iter().copied().any(matches);
            let mut label = row![
                widgets::active_indicator(self.page == section.landing_page),
                navigation::icon(section.landing_page, self.page == section.landing_page)
            ]
            .spacing(design::space::SMALL)
            .align_y(iced::Center);
            if collapsed {
                // Balance the selection marker so the icon remains centered.
                label = label.push(iced::widget::Space::new().width(3));
            } else {
                label = label.push(navigation::label(section.landing_page));
                if self
                    .settings
                    .global()
                    .general
                    .show_enabled_feature_counts_in_sidebar
                {
                    if let Some(count) = navigation::section_enabled_feature_count(
                        &self.settings,
                        section.landing_page,
                    ) {
                        label = label.push(
                            container(text(count.to_string()).size(design::typography::BADGE))
                                .padding([
                                    design::space::TINY as u16,
                                    design::space::CONTROL as u16,
                                ])
                                .style(move |theme| widgets::indicator_chip(theme, count > 0)),
                        );
                    }
                }
            }
            let expandable = !collapsed && section.pages.iter().any(|p| *p != section.landing_page);
            if expandable {
                label = label.push(super::motion::wrap(
                    navigation::glyph("icons/chevron-right.svg"),
                    self.expanded_section == Some(section.landing_page),
                    super::motion::Effect::Chevron,
                ));
            }
            let section_header = button(container(label.height(Fill)).width(Fill).align_x(
                if collapsed {
                    iced::alignment::Horizontal::Center
                } else {
                    iced::Left
                },
            ))
            .height(design::NAVIGATION_ROW_HEIGHT)
            .padding(design::NAVIGATION_ROW_PADDING)
            .width(Fill)
            .on_press(if expandable {
                Message::ToggleSection(section.landing_page)
            } else {
                Message::Page(section.landing_page)
            })
            .selected(self.page == section.landing_page, widgets::selected);
            let section_header: Element<'_, Message> = if collapsed {
                iced::widget::tooltip(
                    section_header,
                    text(section.landing_page.label()),
                    iced::widget::tooltip::Position::Right,
                )
                .style(container::bordered_box)
                .into()
            } else {
                section_header.into()
            };
            let mut children = Vec::new();
            for page in section.pages.iter().filter(|p| **p != section.landing_page) {
                let mut label = row![
                    widgets::active_indicator(self.page == *page),
                    navigation::icon(*page, self.page == *page),
                    navigation::label(*page),
                ]
                .spacing(design::space::SMALL)
                .height(Fill)
                .align_y(iced::Center);
                if navigation::feature_page_enabled(&self.settings, *page) == Some(true) {
                    label = label.push(
                        container(iced::widget::Space::new())
                            .width(3)
                            .height(18)
                            .style(move |theme: &Theme| container::Style {
                                background: Some(theme.palette().success.into()),
                                border: iced::border::rounded(2),
                                ..Default::default()
                            }),
                    );
                }
                children.push(super::motion::wrap(
                    button(label)
                        .width(Fill)
                        .height(design::NAVIGATION_CHILD_ROW_HEIGHT)
                        .on_press(Message::Page(*page))
                        .padding([design::space::SMALL as u16, design::space::MEDIUM as u16])
                        .selected(self.page == *page, widgets::selected),
                    matches(*page),
                    super::motion::Effect::Visible,
                ));
            }
            let section_content = super::motion::wrap(
                navigation::section(
                    section_header,
                    children,
                    !collapsed
                        && (self.expanded_section == Some(section.landing_page)
                            || !self.navigation_search.is_empty()),
                ),
                visible,
                super::motion::Effect::Visible,
            );
            if matches!(
                section.landing_page,
                Page::ActionLog | Page::SettingsHome | Page::About
            ) {
                utilities = utilities.push(section_content);
            } else {
                navigation = navigation.push(section_content);
            }
        }
        let mut toggle_content = row![];
        toggle_content = toggle_content
            .push(navigation::glyph(if collapsed {
                "icons/panel-left-open.svg"
            } else {
                "icons/panel-left-close.svg"
            }))
            .spacing(design::space::SMALL)
            .align_y(iced::Center);
        if !collapsed {
            toggle_content = toggle_content.push(
                text(t!("nav.collapse_navigation").to_string()).size(design::typography::SECONDARY),
            );
        }
        let navigation_toggle =
            widgets::sidebar_toggle(container(toggle_content.height(Fill)).width(Fill).align_x(
                if collapsed {
                    iced::alignment::Horizontal::Center
                } else {
                    iced::Left
                },
            ))
            .on_press(Message::ToggleNavigation);
        let navigation_toggle: Element<'_, Message> = if collapsed {
            iced::widget::tooltip(
                navigation_toggle,
                text(t!("nav.expand_navigation").to_string()),
                iced::widget::tooltip::Position::Right,
            )
            .into()
        } else {
            navigation_toggle.into()
        };
        utilities = utilities
            .push(iced::widget::rule::horizontal(1))
            .push(navigation_toggle);
        const BREADCRUMB_TEXT_SIZE: u32 = design::typography::TITLE;
        let mut header = row![].align_y(iced::Center);
        for (index, page) in self.breadcrumb.iter().copied().enumerate() {
            let mut item = row![].spacing(design::space::COMPACT).align_y(iced::Center);
            if index > 0 {
                item = item.push(navigation::glyph("icons/chevron-right.svg"));
            }
            let label: Element<'_, Message> = if index + 1 < self.breadcrumb.len() {
                button(widgets::heading(page.label(), BREADCRUMB_TEXT_SIZE))
                    .padding(0)
                    .style(widgets::quiet)
                    .on_press(Message::Page(page))
                    .into()
            } else {
                widgets::heading(page.label(), BREADCRUMB_TEXT_SIZE).into()
            };
            item = item.push(label);
            header = header.push(super::motion::wrap(
                container(item).padding(iced::Padding {
                    left: if index > 0 {
                        design::space::COMPACT as f32
                    } else {
                        0.0
                    },
                    ..Default::default()
                }),
                index < self.breadcrumb.len(),
                super::motion::Effect::Visible,
            ));
        }
        let breadcrumb = scrollable(header)
            .direction(iced::widget::scrollable::Direction::Horizontal(
                iced::widget::scrollable::Scrollbar::default(),
            ))
            .width(Fill);
        let mut header = row![breadcrumb]
            .spacing(design::space::COMPACT)
            .align_y(iced::Center);
        if self.page.supports_power_source_profiles() {
            let plugged_in = crate::backend::power_source::is_plugged_in();
            let mut tabs = row![].spacing(design::space::TIGHT);
            for (profile, key, live) in [
                (
                    PowerSourceProfile::PluggedIn,
                    "power_source.plugged_in",
                    plugged_in == Some(true),
                ),
                (
                    PowerSourceProfile::OnBattery,
                    "power_source.on_battery",
                    plugged_in == Some(false),
                ),
            ] {
                let mut label = row![text(t!(key).to_string()).size(design::typography::BODY)]
                    .spacing(design::space::TIGHT)
                    .align_y(iced::Center);
                label = label.push(
                    container(iced::widget::Space::new())
                        .width(6)
                        .height(6)
                        .style(move |theme: &Theme| container::Style {
                            background: live.then(|| theme.palette().primary.into()),
                            border: iced::border::rounded(3),
                            ..Default::default()
                        }),
                );
                tabs = tabs.push(
                    button(container(label).center_y(Fill))
                        .height(32)
                        .on_press(Message::PowerSource(profile))
                        .style(if self.power_source == profile {
                            widgets::selected_control
                        } else {
                            widgets::quiet
                        }),
                );
            }
            header = header.push(
                container(tabs)
                    .padding(design::space::TIGHT as u16)
                    .style(widgets::surface),
            );
        }
        let description = navigation::page_feature_info(self.page);
        if !description.is_empty() {
            header = header.push(
                button(
                    row![
                        navigation::glyph("icons/info.svg"),
                        text(t!("common.feature_info").to_string())
                            .size(design::typography::SECONDARY)
                    ]
                    .spacing(design::space::CONTROL)
                    .align_y(iced::Center),
                )
                .style(if self.feature_info_expanded {
                    widgets::selected
                } else {
                    widgets::quiet
                })
                .on_press(Message::ToggleFeatureInfo),
            );
        }
        let header: Element<'_, Message> = header
            .width(Fill)
            .height(32 + 2 * design::space::TIGHT)
            .into();
        let mut heading = column![container(header)
            .padding([design::space::SMALL as u16, 0])
            .width(Fill)]
        .spacing(0);
        if !description.is_empty() {
            heading = heading.push(widgets::optional_content(
                container(
                    row![
                        navigation::glyph("icons/info.svg"),
                        scrollable(text(description).width(Fill))
                            .height(iced::Length::Shrink)
                            .width(Fill),
                        iced::widget::tooltip(
                            button(navigation::glyph("icons/x.svg"))
                                .style(widgets::quiet)
                                .on_press(Message::ToggleFeatureInfo),
                            text(t!("common.close").to_string()),
                            iced::widget::tooltip::Position::Left,
                        ),
                    ]
                    .spacing(design::space::MEDIUM),
                )
                .max_height(160)
                .padding(design::space::MEDIUM as u16)
                .width(Fill)
                .style(widgets::surface),
                self.feature_info_expanded,
            ));
        }
        let mut body = column![heading].spacing(design::space::MEDIUM).height(Fill);
        let content = self.page_view();
        let side_panel = self.side_panel(self.page);
        body = body.push(
            container(
                container(super::motion::wrap(
                    super::motion::wrap(
                        content,
                        true,
                        super::motion::Effect::Content(self.power_source as u64),
                    ),
                    true,
                    super::motion::Effect::Content(widgets::stable_key(&self.page)),
                ))
                .max_width(design::CONTENT_WIDTH)
                .width(Fill)
                .height(Fill),
            )
            .center_x(Fill)
            .height(Fill),
        );
        let layout = row![
            super::motion::wrap(
                container(
                    container(column![scrollable(navigation).height(Fill), utilities].height(Fill))
                        .width(Fill)
                        .height(Fill)
                        .style(widgets::navigation_surface)
                )
                .width(Fill),
                !collapsed,
                super::motion::Effect::Width {
                    min: design::SIDEBAR_COLLAPSED_WIDTH,
                    max: design::NAVIGATION_WIDTH
                }
            ),
            container(body)
                .padding([design::space::SECTION as u16, design::space::WIDE as u16])
                .center_x(Fill)
                .height(Fill)
        ]
        .spacing(design::space::SMALL)
        .height(Fill);
        let has_side_panel = side_panel.is_some();
        let panel: Element<'_, Message> = if let Some(panel) = side_panel {
            super::motion::wrap(
                container(
                    column![
                        container(widgets::optional_content(panel, !self.status_collapsed))
                            .height(Fill)
                            .padding([0, design::space::CONTROL as u16]),
                        iced::widget::rule::horizontal(1),
                        widgets::sidebar_toggle(
                            row![
                                text(if self.status_collapsed {
                                    String::new()
                                } else {
                                    t!("nav.collapse_side_panel").to_string()
                                })
                                .size(design::typography::SECONDARY)
                                .width(Fill),
                                navigation::glyph(if self.status_collapsed {
                                    "icons/panel-right-open.svg"
                                } else {
                                    "icons/panel-right-close.svg"
                                }),
                            ]
                            .height(Fill)
                            .align_y(iced::Center)
                        )
                        .on_press(Message::ToggleStatus)
                    ]
                    .spacing(design::space::TINY)
                    .padding([design::space::SMALL as u16, design::space::CONTROL as u16])
                    .height(Fill),
                )
                .width(Fill),
                !self.status_collapsed,
                super::motion::Effect::Width {
                    min: design::SIDEBAR_COLLAPSED_WIDTH,
                    max: design::SIDE_PANEL_WIDTH,
                },
            )
        } else {
            iced::widget::Space::new().into()
        };
        let layout: Element<'_, Message> = layout
            .push(super::motion::wrap(
                panel,
                has_side_panel,
                super::motion::Effect::Visible,
            ))
            .into();
        let modal = match self.page {
            Page::CpuSetsSoft => self
                .soft_allocation
                .modal(&self.settings)
                .map(|m| m.map(|m| Message::Allocation(cpu_allocation::Kind::Soft, m))),
            Page::ProcessorAffinityHard => self
                .hard_allocation
                .modal(&self.settings)
                .map(|m| m.map(|m| Message::Allocation(cpu_allocation::Kind::Hard, m))),
            Page::CpuLimiter => self
                .cpu_limiter
                .modal(&self.candidates, &self.status.feature_status.cpu_limiter)
                .map(|modal| modal.map(Message::CpuLimiter)),
            Page::AdvancedPowerPlanTuning => self
                .power_tuning
                .preset_modal(&self.settings.advanced_power_plan_tuning_presets)
                .map(|modal| modal.map(Message::PowerTuning)),
            Page::AdaptiveEngine if self.adaptive.has_pending_editor() => Some(
                self.adaptive
                    .preset_modal(&self.settings, &self.candidates)
                    .map(Message::Adaptive),
            ),
            Page::ByTime => self
                .time_rules
                .modal(power_rules::Kind::Time, &self.power_plans)
                .map(|modal| modal.map(|m| Message::PowerRules(power_rules::Kind::Time, m))),
            Page::ByCpuLoad => self
                .cpu_rules
                .modal(power_rules::Kind::CpuLoad, &self.power_plans)
                .map(|modal| modal.map(|m| Message::PowerRules(power_rules::Kind::CpuLoad, m))),
            _ => None,
        };
        if let Some(modal) = modal {
            return iced::widget::stack![
                layout,
                iced::widget::opaque(
                    container(iced::widget::Space::new())
                        .width(Fill)
                        .height(Fill)
                        .style(|_| container::Style {
                            background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()),
                            ..Default::default()
                        })
                ),
                container(iced::widget::opaque(modal))
                    .padding(16)
                    .center_x(Fill)
                    .center_y(Fill)
            ]
            .into();
        }
        iced::widget::stack![
            layout,
            container(super::motion::wrap(
                widgets::settings_card(
                    column![
                        widgets::heading(t!("unsaved.title").to_string(), design::typography::BODY),
                        text(t!("unsaved.message").to_string()),
                        row![
                            iced::widget::Space::new().width(Fill),
                            button(text(t!("common.discard").to_string()))
                                .style(crate::ui::widgets::tertiary_button)
                                .on_press(Message::Cancel),
                            button(text(t!("common.save").to_string()))
                                .style(crate::ui::widgets::primary_button)
                                .on_press(Message::Save)
                        ]
                        .spacing(design::space::SMALL)
                    ]
                    .spacing(design::space::MEDIUM)
                )
                .width(360),
                self.pending_changes(),
                super::motion::Effect::Visible
            ))
            .padding(design::space::LARGE as u16)
            .width(Fill)
            .height(Fill)
            .align_x(iced::Right)
            .align_y(iced::Bottom)
        ]
        .into()
    }

    fn page_view(&self) -> Element<'_, Message> {
        let general = &self.settings.global().general;
        let toggle = |label: &'static str, value, action: fn(bool) -> Message| {
            widgets::settings_card(widgets::setting_row(
                label,
                widgets::switch(value, Some(action)),
            ))
        };
        match self.page {
            Page::AppSuspension => self
                .suspension
                .view(
                    &self.settings.app_suspension,
                    &self.status.feature_status.app_suspension,
                    &self.unavailable_candidates,
                    &self.candidates,
                )
                .map(Message::Suspension),
            Page::MemoryTrim => self
                .trim
                .view(&self.settings.memory_trim, &self.candidates)
                .map(Message::Trim),
            Page::TimerResolution => self
                .timer
                .view(
                    &self.settings.timer_resolution,
                    &self.status.feature_status.timer_resolution,
                    &self.candidates,
                )
                .map(Message::Timer),
            Page::Win32PrioritySeparation => self
                .priority_separation
                .view()
                .map(Message::PrioritySeparation),
            Page::AdvancedPowerPlanTuning => self
                .power_tuning
                .view(
                    &self.settings.advanced_power_plan_tuning_presets,
                    &self.power_plans,
                    self.effective_power_mode
                        .as_ref()
                        .map_or(crate::power::EffectivePowerMode::Unknown, |monitor| {
                            monitor.snapshot()
                        }),
                )
                .map(Message::PowerTuning),

            Page::ByTime => self
                .time_rules
                .view(
                    power_rules::Kind::Time,
                    &self.settings,
                    &self.status.power_plan_status,
                )
                .map(|m| Message::PowerRules(power_rules::Kind::Time, m)),
            Page::ByCpuLoad => self
                .cpu_rules
                .view(
                    power_rules::Kind::CpuLoad,
                    &self.settings,
                    &self.status.power_plan_status,
                )
                .map(|m| Message::PowerRules(power_rules::Kind::CpuLoad, m)),
            Page::BackgroundEfficiency => self
                .efficiency
                .view(&self.settings, &self.candidates)
                .map(Message::Efficiency),
            Page::CpuSetsSoft => self
                .soft_allocation
                .view(&self.settings, cpu_allocation::Kind::Soft, &self.candidates)
                .map(|m| Message::Allocation(cpu_allocation::Kind::Soft, m)),
            Page::ProcessorAffinityHard => self
                .hard_allocation
                .view(&self.settings, cpu_allocation::Kind::Hard, &self.candidates)
                .map(|m| Message::Allocation(cpu_allocation::Kind::Hard, m)),
            Page::AdaptiveEngine => self
                .adaptive
                .view(&self.settings, &self.candidates)
                .map(Message::Adaptive),
            Page::ProcessPriority => self
                .priority
                .view(
                    &self.settings,
                    priority_control::Kind::Process,
                    &self.candidates,
                )
                .map(|m| Message::Priority(priority_control::Kind::Process, m)),
            Page::ThreadPriority => self
                .priority
                .view(
                    &self.settings,
                    priority_control::Kind::Thread,
                    &self.candidates,
                )
                .map(|m| Message::Priority(priority_control::Kind::Thread, m)),
            Page::IoPriority => self
                .priority
                .view(&self.settings, priority_control::Kind::Io, &self.candidates)
                .map(|m| Message::Priority(priority_control::Kind::Io, m)),
            Page::GpuPriority => self
                .priority
                .view(
                    &self.settings,
                    priority_control::Kind::Gpu,
                    &self.candidates,
                )
                .map(|m| Message::Priority(priority_control::Kind::Gpu, m)),
            Page::MemoryPriority => self
                .priority
                .view(
                    &self.settings,
                    priority_control::Kind::Memory,
                    &self.candidates,
                )
                .map(|m| Message::Priority(priority_control::Kind::Memory, m)),
            Page::DynamicPriorityBoost => self
                .priority
                .view(
                    &self.settings,
                    priority_control::Kind::DynamicBoost,
                    &self.candidates,
                )
                .map(|m| Message::Priority(priority_control::Kind::DynamicBoost, m)),

            Page::ByForeground => self
                .foreground_plans
                .view(
                    process_power_plans::Kind::Foreground,
                    &self.settings,
                    &self.power_plans,
                    &self.candidates,
                    &self.status.power_plan_status,
                )
                .map(|message| {
                    Message::ProcessPowerPlans(process_power_plans::Kind::Foreground, message)
                }),
            Page::ByRunningApp => self
                .running_app_plans
                .view(
                    process_power_plans::Kind::RunningApp,
                    &self.settings,
                    &self.power_plans,
                    &self.candidates,
                    &self.status.power_plan_status,
                )
                .map(|message| {
                    Message::ProcessPowerPlans(process_power_plans::Kind::RunningApp, message)
                }),
            Page::ByActivity => {
                by_activity::view(&self.settings, &self.power_plans, &self.activity_inputs)
                    .map(Message::ByActivity)
            }
            Page::CpuLimiter => self
                .cpu_limiter
                .view(
                    &self.settings.cpu_limiter,
                    &self.candidates,
                    self.settings.global().general.enabled,
                    &self.status.feature_status.cpu_limiter,
                )
                .map(Message::CpuLimiter),
            Page::ProcessList => self
                .processes
                .view(
                    &self.settings,
                    &self.status,
                    if self.power_plans_loading {
                        process_list::PlanCatalog::Loading
                    } else if self.power_plans_loaded {
                        process_list::PlanCatalog::Loaded(&self.power_plans)
                    } else {
                        process_list::PlanCatalog::Unavailable
                    },
                )
                .map(Message::Processes),
            Page::Home => self
                .home
                .view(&self.settings, &self.status.feature_status)
                .map(Message::Home),
            Page::WinderustBehaviour
            | Page::LanguageAndAppearance
            | Page::ExperimentalFeatures
            | Page::About => self
                .preferences
                .view(self.page, self.settings.global())
                .map(Message::Preferences),
            Page::PowerPlanControl => column![
                toggle(
                    "power_plan_control.pause_plugged",
                    general.pause_power_plan_switching_while_plugged_in,
                    Message::PausePowerPlans
                ),
                self.child_pages(),
            ]
            .spacing(widgets::CARD_GAP)
            .into(),
            Page::ActionLog => self
                .action_log
                .view(&self.status.action_log_entries, &self.candidates)
                .map(Message::ActionLog),
            Page::WinderustFeatures
            | Page::CpuControl
            | Page::PriorityControl
            | Page::SettingsHome
            | Page::AdvancedControls => self.child_pages(),
        }
    }

    fn child_pages(&self) -> Element<'_, Message> {
        let mut pages = column![].spacing(widgets::CARD_GAP);
        if self.page == Page::PowerPlanControl {
            pages = pages.push(widgets::heading(
                t!("power_plan_control.automation").to_string(),
                design::typography::BODY,
            ));
        }
        if let Some(children) = self.page.child_pages() {
            for page in children.iter().filter(|page| **page != self.page) {
                if self.page == Page::PowerPlanControl && *page == Page::AdvancedPowerPlanTuning {
                    pages = pages.push(widgets::heading(
                        t!("settings.advanced").to_string(),
                        design::typography::BODY,
                    ));
                }
                let mut heading = row![
                    navigation::icon(*page, self.page == *page),
                    widgets::heading(page.label(), design::typography::BODY).width(Fill)
                ]
                .spacing(design::space::COMPACT)
                .align_y(iced::Center);
                if self.settings.global().general.show_feature_status_on_cards {
                    if let Some(enabled) = navigation::feature_page_enabled(&self.settings, *page) {
                        heading = heading.push(
                            text(
                                if enabled {
                                    t!("common.enabled")
                                } else {
                                    t!("common.disabled")
                                }
                                .to_string(),
                            )
                            .size(design::typography::CAPTION)
                            .style(if enabled {
                                text::success
                            } else {
                                text::secondary
                            }),
                        );
                    }
                }
                heading = heading.push(navigation::glyph("icons/chevron-right.svg"));
                pages = pages.push(widgets::card_button(heading).on_press(Message::Page(*page)));
            }
        }
        scrollable(pages).height(Fill).into()
    }
}

impl Drop for WinderustApp {
    fn drop(&mut self) {
        if let Err(error) = self.runtime.shutdown() {
            crate::backend::diagnostics::error(&error);
            eprintln!("{error}");
        }
    }
}

fn priority_page(kind: priority_control::Kind) -> Page {
    match kind {
        priority_control::Kind::Process => Page::ProcessPriority,
        priority_control::Kind::Thread => Page::ThreadPriority,
        priority_control::Kind::Io => Page::IoPriority,
        priority_control::Kind::Gpu => Page::GpuPriority,
        priority_control::Kind::Memory => Page::MemoryPriority,
        priority_control::Kind::DynamicBoost => Page::DynamicPriorityBoost,
    }
}
fn priority_kind(page: Page) -> Option<priority_control::Kind> {
    match page {
        Page::ProcessPriority => Some(priority_control::Kind::Process),
        Page::ThreadPriority => Some(priority_control::Kind::Thread),
        Page::IoPriority => Some(priority_control::Kind::Io),
        Page::GpuPriority => Some(priority_control::Kind::Gpu),
        Page::MemoryPriority => Some(priority_control::Kind::Memory),
        Page::DynamicPriorityBoost => Some(priority_control::Kind::DynamicBoost),
        _ => None,
    }
}

fn tray_retry_due(
    attempt: Option<((bool, bool), std::time::Instant)>,
    intent: (bool, bool),
    now: std::time::Instant,
) -> bool {
    attempt.is_none_or(|(previous, attempted)| {
        previous != intent || now.duration_since(attempted) >= Duration::from_secs(5)
    })
}

fn page_needs_power_plans(page: Page) -> bool {
    page == Page::ProcessList || page.section_landing_page() == Page::PowerPlanControl
}

fn ui_wake_channel() -> (
    std::sync::Arc<dyn Fn() + Send + Sync>,
    iced::futures::channel::mpsc::Receiver<()>,
) {
    let (sender, events) = iced::futures::channel::mpsc::channel(1);
    let sender = std::sync::Mutex::new(sender);
    let wake = std::sync::Arc::new(move || {
        let _ = sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .try_send(());
    });
    (wake, events)
}

fn ui_tick_interval(hidden: bool, retry_pending: bool) -> Option<Duration> {
    if !hidden {
        Some(Duration::from_millis(250))
    } else if retry_pending {
        Some(Duration::from_secs(5))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn exit_guard_keeps_pending_completions_but_rejects_new_work() {
        assert!(
            Message::Processes(process_list::Message::Loaded(Err("fixture".into())))
                .allowed_during_exit()
        );
        let mut editor = settings_pages::Editor::default();
        let mut settings = crate::config::Settings::default();
        let channel = settings.general.update_channel;
        assert!(editor.begin_check(channel, false));
        let completed = Message::Preferences(settings_pages::Message::Checked(
            channel,
            Err("fixture".into()),
        ));
        assert!(completed.allowed_during_exit());
        if let Message::Preferences(message) = completed {
            editor.update(&mut settings, message);
        }
        assert!(editor.begin_check(channel, false));
        assert!(Message::ShutdownFinished(Err("fixture".into())).allowed_during_exit());
        for message in [
            Message::Processes(process_list::Message::Refresh),
            Message::Processes(process_list::Message::ConfirmStop),
            Message::Preferences(settings_pages::Message::SaveColor),
            Message::Preferences(settings_pages::Message::Check),
            Message::ColorChosen(None, Ok(Some(0xffffff))),
            Message::Save,
            Message::Stay,
            Message::DiscardAndClose,
        ] {
            assert!(!message.allowed_during_exit());
        }
    }

    #[test]
    fn hidden_ui_sleeps_until_events_but_keeps_pending_retries() {
        use iced::futures::{FutureExt, StreamExt};
        assert_eq!(ui_tick_interval(true, false), None);
        assert_eq!(ui_tick_interval(true, true), Some(Duration::from_secs(5)));
        assert_eq!(
            ui_tick_interval(false, false),
            Some(Duration::from_millis(250))
        );
        let (wake, mut events) = ui_wake_channel();
        assert!(events.next().now_or_never().is_none());
        for _ in 0..100 {
            wake();
        }
        assert_eq!(events.next().now_or_never(), Some(Some(())));
        // Bursts coalesce in a bounded queue rather than rebuilding a page per event.
        let mut queued = 0;
        while events.next().now_or_never().is_some() {
            queued += 1;
        }
        assert!(queued <= 1);
        wake();
        assert_eq!(events.next().now_or_never(), Some(Some(())));
    }

    #[test]
    fn process_list_requests_its_power_plan_dependency() {
        assert!(page_needs_power_plans(Page::ProcessList));
        assert!(page_needs_power_plans(Page::PowerPlanControl));
        assert!(!page_needs_power_plans(Page::Home));
    }

    use super::*;

    #[test]
    fn unchanged_tray_intent_retries_without_polling_or_error_spam() {
        let now = std::time::Instant::now();
        let intent = (true, false);
        assert!(tray_retry_due(None, intent, now));
        let failed = Some((intent, now));
        assert!(!tray_retry_due(
            failed,
            intent,
            now + Duration::from_millis(250)
        ));
        assert!(tray_retry_due(failed, intent, now + Duration::from_secs(5)));
        assert!(tray_retry_due(failed, (true, true), now));
    }
}
