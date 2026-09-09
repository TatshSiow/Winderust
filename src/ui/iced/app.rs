use std::{cell::RefCell, path::PathBuf, time::Duration};
use widgets::{button, text_input};

use iced::widget::{column, container, row, scrollable, text};
use iced::{Element, Fill, Font, Subscription, Task, Theme};
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

mod action_log;
mod adaptive_engine;
mod advanced_power_plan_tuning;
mod app_suspension;
mod background_efficiency;
mod by_activity;
mod cpu_allocation;
mod cpu_limiter;
mod design;
mod home;
mod memory_trim;
mod navigation;
mod power_rules;
mod priority_control;
mod process_list;
mod process_power_plans;
mod settings_pages;
#[cfg(feature = "render-smoke")]
pub(crate) mod smoke;
mod status_rail;
mod tasks;
mod timer_resolution;
mod widgets;
mod win32_priority_separation;

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
            (
                WinderustApp {
                    appearance: settings_pages::theme(&settings.general),
                    #[cfg(feature = "render-smoke")]
                    smoke: smoke::Run::requested(),
                    navigation_search: String::new(),
                    collapsed_sections: Page::sections()
                        .iter()
                        .map(|section| section.landing_page)
                        .collect(),
                    status_collapsed: false,
                    compact_panel_open: false,
                    description_expanded: false,
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
                    runtime,
                    status: RuntimeStatusSnapshot::default(),
                    message: error
                        .or_else(crate::crash_recovery::startup_error)
                        .unwrap_or_default(),
                    page: Page::Home,
                    restore_event,
                    window: None,
                    auto_exclusion_generation: 0,
                    closing: false,
                    hwnd: None,
                    tray: None,
                    tray_attempt: None,
                    hidden: false,
                    processes: process_list::ProcessList::default(),
                    cpu_limiter: cpu_limiter::CpuLimiter::default(),
                    power_source: PowerSourceProfile::PluggedIn,
                    power_plans: Vec::new(),
                    power_plans_loading: false,
                    foreground_plans: process_power_plans::Editor::default(),
                    running_app_plans: process_power_plans::Editor::default(),
                    activity_inputs: by_activity::Inputs::default(),
                },
                iced::window::latest().map(Message::Window),
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
        default_font: Font::with_name(design::typography::FONT),
        default_text_size: design::typography::BODY.into(),
        ..Default::default()
    })
    .theme(|app: &WinderustApp| app.appearance.clone())
    .subscription(|_: &WinderustApp| {
        Subscription::batch([
            iced::time::every(Duration::from_millis(250)).map(|_| Message::Tick),
            iced::window::close_requests().map(|_| Message::WindowClose),
        ])
    })
    .run()
}

struct WinderustApp {
    #[cfg(feature = "render-smoke")]
    smoke: Option<smoke::Run>,
    appearance: Theme,
    navigation_search: String,
    collapsed_sections: std::collections::HashSet<Page>,
    status_collapsed: bool,
    compact_panel_open: bool,
    description_expanded: bool,
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
    candidates: Vec<String>,
    unavailable_candidates: Vec<String>,
    catalog_loading: bool,
    settings: SettingsEditor,
    runtime: RuntimeHandle,
    status: RuntimeStatusSnapshot,
    message: String,
    page: Page,
    restore_event: Option<SingleInstanceRestoreEvent>,
    window: Option<iced::window::Id>,
    auto_exclusion_generation: u64,
    closing: bool,
    hwnd: Option<usize>,
    tray: Option<tray::TrayIcon>,
    tray_attempt: Option<(bool, bool)>,
    hidden: bool,
    processes: process_list::ProcessList,
    cpu_limiter: cpu_limiter::CpuLimiter,
    power_source: PowerSourceProfile,
    power_plans: Vec<crate::power::PowerPlan>,
    power_plans_loading: bool,
    foreground_plans: process_power_plans::Editor,
    running_app_plans: process_power_plans::Editor,
    activity_inputs: by_activity::Inputs,
}

#[derive(Debug, Clone)]
enum Message {
    #[cfg(feature = "render-smoke")]
    SmokeScreenshot(iced::window::Screenshot),
    NavigationSearch(String),
    ToggleNavigation,
    ToggleSection(Page),
    ToggleDescription,
    ToggleStatus,
    ToggleCompactPanel,
    Preferences(settings_pages::Message),
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
    Catalog(Result<Vec<crate::foreground::ProcessCandidateInfo>, String>),
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
    Processes(process_list::Message),
    CpuLimiter(cpu_limiter::Message),
    PowerSource(PowerSourceProfile),
    ByActivity(by_activity::Message),
    ProcessPowerPlans(process_power_plans::Kind, process_power_plans::Message),
    PowerPlans(Result<Vec<crate::power::PowerPlan>, String>),
}

impl WinderustApp {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            #[cfg(feature = "render-smoke")]
            Message::SmokeScreenshot(screenshot) => return smoke::captured(self, screenshot),
            Message::NavigationSearch(value) => self.navigation_search = value,
            Message::ToggleNavigation => {
                let patch = crate::application::NavigationCollapsedPatch {
                    base_revision: self.settings.base_revision(),
                    navigation_collapsed: !self.settings.general.navigation_collapsed,
                };
                match self.settings.apply_navigation_collapsed_patch(patch) {
                    Ok(_) => self.publish_settings(),
                    Err(error) => self.message = error.to_string(),
                }
            }
            Message::ToggleDescription => self.description_expanded = !self.description_expanded,
            Message::ToggleSection(page) => {
                if self.page != page {
                    return self.update(Message::Page(page));
                }
                if !self.collapsed_sections.remove(&page) {
                    self.collapsed_sections.insert(page);
                }
            }
            Message::ToggleCompactPanel => self.compact_panel_open = !self.compact_panel_open,
            Message::ToggleStatus => self.status_collapsed = !self.status_collapsed,
            Message::Status(status_rail::Message::ActionLog) => {
                return self.update(Message::Page(Page::ActionLog))
            }
            Message::Status(status_rail::Message::RelaunchAdmin) => {
                if crate::privilege::relaunch_as_admin() {
                    return self.update(Message::Close);
                }
            }
            Message::Preferences(settings_pages::Message::Open(url)) => {
                if let Err(error) = crate::win_util::open_url(&url) {
                    self.message = error;
                }
            }
            Message::Home(home::Message::Navigate(page)) => {
                return self.update(Message::Page(page))
            }
            Message::Sample(result) => {
                self.sampling = false;
                match result {
                    Ok(sample) => self.home.record(sample),
                    Err(error) => self.message = error,
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
                let channel = self.settings.general.update_channel;
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
            Message::Preferences(message) => {
                self.preferences.update(&mut self.settings, message);
                self.appearance = settings_pages::theme(&self.settings.general);
                if self.settings.advanced.pause_process_population {
                    self.processes.clear();
                    self.candidates.clear();
                    self.unavailable_candidates.clear();
                }
            }
            Message::ActionLog(action_log::Message::Clear) => {
                self.runtime.clear_action_log();
                self.status.action_log_entries = Default::default();
                self.status.action_log_summaries = Default::default();
                self.action_log.update(action_log::Message::Clear);
            }
            Message::ActionLog(action_log::Message::Export) => {
                return Task::perform(
                    crate::file_dialog::choose_action_log_export_file(
                        self.hwnd.map(|h| h as windows_sys::Win32::Foundation::HWND),
                    ),
                    Message::ExportLog,
                )
            }
            Message::ActionLog(message) => self.action_log.update(message),
            Message::ExportLog(path) => {
                self.message = match path {
                    Some(path) => match crate::config::storage::write_bytes_atomically(
                        &path,
                        action_log::action_log_entries_to_csv(&self.status.action_log_entries)
                            .as_bytes(),
                    ) {
                        Ok(()) => {
                            t!("status.exported_action_log", path = path.display()).to_string()
                        }
                        Err(error) => t!(
                            "status.action_log_export_failed",
                            path = path.display(),
                            error = error
                        )
                        .to_string(),
                    },
                    None => t!("status.action_log_export_canceled").to_string(),
                };
            }
            Message::CommandFinished(result) => {
                self.message = result.err().unwrap_or_default();
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
                        Err(error) => self.message = error.to_string(),
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
                    Err(error) => self.message = error.to_string(),
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
            Message::PowerTuning(message) => self.power_tuning.update(
                &mut self.settings.advanced_power_plan_tuning_presets,
                &self.power_plans,
                message,
            ),

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
            Message::Allocation(kind, message) => match kind {
                cpu_allocation::Kind::Soft => {
                    self.soft_allocation
                        .update(&mut self.settings, kind, message)
                }
                cpu_allocation::Kind::Hard => {
                    self.hard_allocation
                        .update(&mut self.settings, kind, message)
                }
            },
            Message::Adaptive(adaptive_engine::Message::Status(message)) => {
                return self.update(Message::Status(message))
            }
            Message::Adaptive(adaptive_engine::Message::Browse) => {
                return self.browse(Page::AdaptiveEngine)
            }
            Message::Adaptive(message) => self.adaptive.update(&mut self.settings, message),
            Message::Catalog(result) => {
                self.catalog_loading = false;
                match result {
                    Ok(candidates) if !self.settings.advanced.pause_process_population => {
                        self.unavailable_candidates = candidates
                            .iter()
                            .filter(|candidate| !candidate.has_suspendable_instance)
                            .map(|candidate| candidate.image_path.to_string_lossy().into_owned())
                            .collect();
                        self.candidates = candidates
                            .into_iter()
                            .map(|candidate| candidate.image_path.to_string_lossy().into_owned())
                            .collect();
                    }
                    Ok(_) => {}
                    Err(error) => self.message = error,
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
                        if self.page == Page::AdvancedPowerPlanTuning {
                            self.power_tuning.ensure_plan(&self.power_plans);
                        }
                    }
                    Err(error) => self.message = error,
                }
            }
            Message::CpuLimiter(cpu_limiter::Message::Browse) => {
                return self.browse(Page::CpuLimiter)
            }
            Message::CpuLimiter(message) => self
                .cpu_limiter
                .update(&mut self.settings.cpu_limiter, message),
            Message::PowerSource(_)
                if self.pending_preset()
                    || self.adaptive.validation_error().is_some()
                    || self.cpu_limiter.has_invalid_inputs()
                    || !self.activity_inputs.valid()
                    || !self.time_rules.valid()
                    || !self.cpu_rules.valid() =>
            {
                self.message = t!("unsaved.message").to_string();
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
                let update = if self.settings.general.check_for_updates {
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
            Message::Page(page) => {
                self.description_expanded = false;
                self.page = page;
                self.collapsed_sections.remove(&page.section_landing_page());
                if page == Page::Win32PrioritySeparation {
                    self.priority_separation.refresh();
                }
                if page == Page::AdvancedPowerPlanTuning {
                    self.power_tuning.ensure_plan(&self.power_plans);
                }
                let mut tasks = Vec::new();
                if page.section_landing_page() == Page::PowerPlanControl
                    && !self.power_plans_loading
                {
                    self.power_plans_loading = true;
                    tasks.push(
                        tasks::run(crate::power::list_plans)
                            .map(|result| Message::PowerPlans(result.and_then(|result| result))),
                    );
                }
                if !self.settings.advanced.pause_process_population {
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
                    if page.supports_power_source_profiles() && !self.catalog_loading {
                        self.catalog_loading = true;
                        tasks.push(
                            tasks::run(crate::foreground::list_process_candidates)
                                .map(|result| Message::Catalog(result.and_then(|result| result))),
                        );
                    }
                }
                return Task::batch(tasks);
            }
            Message::Processes(message) => {
                if self.settings.advanced.pause_process_population
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
            Message::Tick => {
                #[cfg(feature = "render-smoke")]
                if let Some(task) = smoke::advance(self) {
                    return task;
                }
                self.sync_tray();
                if tray::take_quit_requested() {
                    return self.update(Message::Close);
                }
                let restore = tray::take_restore_requested();
                let hidden = tray::is_hidden_to_tray();
                if hidden != self.hidden || restore {
                    self.hidden = hidden;
                    if let Some(window) = self.window {
                        return iced::window::set_mode(
                            window,
                            if hidden {
                                iced::window::Mode::Hidden
                            } else {
                                iced::window::Mode::Windowed
                            },
                        );
                    }
                }
                if let Some(status) = self.runtime.status_snapshot_since(self.status.generation) {
                    if status.appearance_change_generation
                        != self.status.appearance_change_generation
                    {
                        self.appearance = settings_pages::theme(&self.settings.general);
                    }
                    self.status = status;
                }
                if let Some(patch) = self
                    .runtime
                    .take_auto_exclusion_patch_since(&mut self.auto_exclusion_generation)
                {
                    if let Err(error) = self.settings.apply_auto_exclusion_patch(&patch) {
                        self.message = error.to_string();
                        self.runtime.requeue_auto_exclusion_patch(patch);
                    } else {
                        self.publish_settings();
                    }
                }
                let mut work = Vec::new();
                if !self.hidden && !self.settings.advanced.pause_process_population {
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
                    if self.page.supports_power_source_profiles()
                        && !self.catalog_loading
                        && self.catalog_sampled_at.elapsed() >= Duration::from_secs(3)
                    {
                        self.catalog_loading = true;
                        self.catalog_sampled_at = std::time::Instant::now();
                        work.push(
                            tasks::run(crate::foreground::list_process_candidates)
                                .map(|r| Message::Catalog(r.and_then(|r| r))),
                        );
                    }
                }
                if self.page == Page::Home
                    && !self.hidden
                    && !self.settings.advanced.pause_dashboard_metrics
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
                if self.settings.general.hide_to_tray && self.tray.is_some() {
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
                if self.pending_changes() {
                    self.closing = true;
                    return self.show_window();
                } else {
                    return self.shutdown();
                }
            }
            Message::Save if self.pending_preset() => {
                self.message = t!("unsaved.message").to_string();
            }
            Message::Save if self.adaptive.validation_error().is_some() => {
                self.message = self.adaptive.validation_error().unwrap_or_default();
            }
            Message::Save if self.cpu_limiter.has_invalid_inputs() => {
                self.message = t!("cpu_limiter.intro_2").to_string();
            }
            Message::Save if !self.time_rules.valid() || !self.cpu_rules.valid() => {
                self.message = t!("unsaved.message").to_string();
            }
            Message::Save if !self.activity_inputs.valid() => {
                self.message = t!("by_activity.invalid_timing").to_string();
            }
            Message::Save => match self.settings.save() {
                Ok(outcome) => {
                    self.message = outcome.startup_registration_error().map_or_else(
                        || {
                            t!(
                                "status.saved_settings",
                                path = crate::config::storage::config_path().display()
                            )
                            .to_string()
                        },
                        ToString::to_string,
                    );
                    self.publish_settings();
                    if self.power_tuning.dirty && !self.power_tuning.apply() {
                        self.message = self.power_tuning.status.clone();
                        return Task::none();
                    }
                    self.reset_editors();
                    if self.closing {
                        return self.shutdown();
                    }
                }
                Err(error) => self.message = error.to_string(),
            },
            Message::Cancel => {
                self.settings.cancel();
                if self.power_tuning.dirty {
                    self.power_tuning.refresh();
                }
                self.reset_editors();
                rust_i18n::set_locale(self.settings.general.language.locale());
                self.publish_settings();
                self.closing = false;
            }
            Message::PausePowerPlans(value) => {
                self.settings
                    .general
                    .pause_power_plan_switching_while_plugged_in = value
            }
            Message::Stay => self.closing = false,
            Message::DiscardAndClose => {
                self.settings.cancel();
                if self.power_tuning.dirty {
                    self.power_tuning.refresh();
                }
                return self.shutdown();
            }
            Message::SettingsFile(_)
                if self.pending_preset()
                    || self.adaptive.validation_error().is_some()
                    || self.cpu_limiter.has_invalid_inputs()
                    || !self.activity_inputs.valid()
                    || !self.time_rules.valid()
                    || !self.cpu_rules.valid() =>
            {
                self.message = t!("unsaved.message").to_string();
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
                                self.message = outcome.startup_registration_error().map_or_else(
                                    || {
                                        t!("status.imported_settings", path = path.display())
                                            .to_string()
                                    },
                                    ToString::to_string,
                                );
                                rust_i18n::set_locale(self.settings.general.language.locale());
                                self.publish_settings();
                            }
                            Err(error) => self.message = error.to_string(),
                        },
                        FileDialogMode::Save => {
                            self.message = match self.settings.export_toml_to(&path) {
                                Ok(()) => t!("status.exported_settings", path = path.display())
                                    .to_string(),
                                Err(error) => error.to_string(),
                            };
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
            || self.pending_preset()
            || self.invalid_inputs()
    }
    fn invalid_inputs(&self) -> bool {
        self.cpu_limiter.has_invalid_inputs()
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
    fn pending_preset(&self) -> bool {
        self.power_tuning.has_pending_editor()
            || self.adaptive.has_pending_editor()
            || self.soft_allocation.has_pending_editor()
            || self.hard_allocation.has_pending_editor()
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
        self.soft_allocation = Default::default();
        self.hard_allocation = Default::default();
        self.adaptive = Default::default();
        self.time_rules = Default::default();
        self.cpu_rules = Default::default();
        self.suspension = Default::default();
        self.trim = Default::default();
        self.timer = Default::default();
        self.appearance = settings_pages::theme(&self.settings.general);
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
    }

    fn sync_tray(&mut self) {
        let intent = (
            self.settings.general.hide_to_tray,
            self.settings.persisted().general.start_minimized,
        );
        if !intent.0 && !intent.1 {
            tray::set_hide_on_close(false);
            self.tray = None;
            self.tray_attempt = None;
        } else if self.tray.is_none() && self.tray_attempt != Some(intent) {
            if let Some(hwnd) = self.hwnd {
                self.tray_attempt = Some(intent);
                match tray::TrayIcon::install(hwnd as windows_sys::Win32::Foundation::HWND) {
                    Ok(icon) => self.tray = Some(icon),
                    Err(error) => self.message = error,
                }
            }
        }
        tray::set_hide_on_close(intent.0 && self.tray.is_some());
    }

    fn shutdown(&mut self) -> Task<Message> {
        match self.runtime.shutdown() {
            Ok(()) => {
                tray::set_hide_on_close(false);
                self.tray = None;
                iced::exit()
            }
            Err(error) => {
                self.message = error;
                self.show_window()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        iced::widget::responsive(|size| self.view_at_width(size.width)).into()
    }

    fn view_at_width(&self, width: f32) -> Element<'_, Message> {
        if self.preferences.show_update && !self.closing {
            return container(
                column![
                    text(t!("about.updates").to_string()).size(design::typography::DIALOG_TITLE),
                    text(self.preferences.latest.clone().unwrap_or_default()),
                    button(text(t!("about.download_update").to_string())).on_press_maybe(
                        self.preferences
                            .download
                            .clone()
                            .map(|url| Message::Preferences(settings_pages::Message::Open(url)))
                    ),
                    button(text(t!("common.cancel").to_string()))
                        .on_press(Message::Preferences(settings_pages::Message::DismissUpdate))
                ]
                .spacing(design::space::LARGE),
            )
            .padding(design::space::SECTION as u16)
            .into();
        }
        if self.closing {
            return container(
                column![
                    text(t!("unsaved.title").to_string()).size(design::typography::DIALOG_TITLE),
                    text(t!("unsaved.message").to_string()),
                    text(&self.message),
                    row![
                        button(text(t!("common.save").to_string())).on_press(Message::Save),
                        button(text(t!("common.discard").to_string()))
                            .on_press(Message::DiscardAndClose),
                        button(text(t!("common.cancel").to_string())).on_press(Message::Stay),
                    ]
                    .spacing(design::space::SMALL),
                ]
                .spacing(design::space::LARGE),
            )
            .padding(design::space::SECTION as u16)
            .into();
        }
        let collapsed = self.settings.general.navigation_collapsed;
        let mut navigation = column![]
            .spacing(design::space::TINY)
            .padding([design::space::SMALL as u16, design::space::CONTROL as u16])
            .width(Fill);
        if collapsed {
            navigation = navigation.push(
                button(navigation::glyph("icons/search.svg"))
                    .on_press(Message::ToggleNavigation)
                    .style(widgets::quiet),
            );
        } else {
            navigation = navigation.push(
                text_input(&t!("home.search_placeholder"), &self.navigation_search)
                    .on_input(Message::NavigationSearch),
            );
        }
        let search_pages = navigation::dashboard_search_pages(
            &self.navigation_search,
            self.settings.advanced.show_advanced_controls,
        );
        let mut utilities = column![]
            .spacing(design::space::TIGHT)
            .padding([design::space::SMALL as u16, design::space::CONTROL as u16]);
        for section in Page::sections() {
            if section.landing_page == Page::AdvancedControls
                && !self.settings.advanced.show_advanced_controls
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
            if !matches(section.landing_page) && !section.pages.iter().copied().any(matches) {
                continue;
            }
            let mut label = row![
                container(iced::widget::Space::new())
                    .width(3)
                    .height(20)
                    .style(move |theme: &Theme| container::Style {
                        background: (self.page == section.landing_page)
                            .then_some(theme.palette().primary.into()),
                        ..Default::default()
                    }),
                navigation::icon(section.landing_page)
            ]
            .spacing(design::space::SMALL)
            .align_y(iced::Center);
            if !collapsed {
                label = label.push(navigation::label(section.landing_page));
                if self.settings.general.show_enabled_feature_counts_in_sidebar {
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
                                .style(container::transparent),
                        );
                    }
                }
            }
            let expandable = !collapsed && section.pages.iter().any(|p| *p != section.landing_page);
            if expandable {
                label = label.push(navigation::glyph(
                    if self.collapsed_sections.contains(&section.landing_page) {
                        "icons/chevron-right.svg"
                    } else {
                        "icons/chevron-down.svg"
                    },
                ));
            }
            let section_header = button(label.height(Fill))
                .height(design::NAVIGATION_ROW_HEIGHT)
                .padding([design::space::COMPACT as u16, design::space::MEDIUM as u16])
                .width(if collapsed {
                    iced::Length::Fixed(56.0)
                } else {
                    Fill
                })
                .on_press(if expandable {
                    Message::ToggleSection(section.landing_page)
                } else {
                    Message::Page(section.landing_page)
                })
                .style(if self.page == section.landing_page {
                    widgets::selected
                } else {
                    widgets::quiet
                });
            let mut children = Vec::new();
            for page in section
                .pages
                .iter()
                .filter(|p| **p != section.landing_page && matches(**p))
            {
                children.push(
                    button(
                        row![navigation::icon(*page), navigation::label(*page)]
                            .spacing(design::space::SMALL)
                            .height(Fill)
                            .align_y(iced::Center),
                    )
                    .width(Fill)
                    .height(design::NAVIGATION_CHILD_ROW_HEIGHT)
                    .on_press(Message::Page(*page))
                    .padding([design::space::SMALL as u16, design::space::MEDIUM as u16])
                    .style(if self.page == *page {
                        widgets::selected
                    } else {
                        widgets::quiet
                    })
                    .into(),
                );
            }
            let section_content = navigation::section(
                section_header,
                children,
                !collapsed && !self.collapsed_sections.contains(&section.landing_page),
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
        let mut toggle_content = row![navigation::glyph(if collapsed {
            "icons/panel-left-open.svg"
        } else {
            "icons/panel-left-close.svg"
        })]
        .spacing(design::space::MEDIUM)
        .align_y(iced::Center);
        if !collapsed {
            toggle_content = toggle_content.push(
                text(t!("nav.collapse_navigation").to_string()).size(design::typography::SECONDARY),
            );
        }
        let navigation_toggle = button(toggle_content.height(Fill))
            .width(Fill)
            .height(design::NAVIGATION_ROW_HEIGHT)
            .padding(design::space::COMPACT as u16)
            .on_press(Message::ToggleNavigation)
            .style(widgets::quiet);
        utilities = utilities
            .push(iced::widget::rule::horizontal(1))
            .push(iced::widget::tooltip(
                navigation_toggle,
                text(if collapsed {
                    t!("nav.expand_navigation").to_string()
                } else {
                    t!("nav.collapse_navigation").to_string()
                }),
                iced::widget::tooltip::Position::Right,
            ));
        const BREADCRUMB_TEXT_SIZE: u32 = design::typography::TITLE;
        let mut header = row![].spacing(design::space::COMPACT).align_y(iced::Center);
        if self.page != Page::Home {
            header = header
                .push(
                    button(widgets::heading(Page::Home.label(), BREADCRUMB_TEXT_SIZE))
                        .style(widgets::quiet)
                        .on_press(Message::Page(Page::Home)),
                )
                .push(navigation::glyph("icons/chevron-right.svg"));
        }
        let parent = self.page.section_landing_page();
        if parent != self.page && parent != Page::Home {
            header = header
                .push(
                    button(widgets::heading(parent.label(), BREADCRUMB_TEXT_SIZE))
                        .style(widgets::quiet)
                        .on_press(Message::Page(parent)),
                )
                .push(navigation::glyph("icons/chevron-right.svg"));
        }
        header = header.push(widgets::heading(self.page.label(), BREADCRUMB_TEXT_SIZE));
        if width >= design::SIDE_PANEL_BREAKPOINT {
            header = header.push(iced::widget::Space::new().width(Fill));
        }
        if self.page.supports_power_source_profiles() {
            header = header.push(
                row![
                    button(
                        text(t!("power_source.plugged_in").to_string())
                            .size(design::typography::CAPTION)
                    )
                    .on_press(Message::PowerSource(PowerSourceProfile::PluggedIn))
                    .style(
                        if self.power_source == PowerSourceProfile::PluggedIn {
                            widgets::selected
                        } else {
                            widgets::quiet
                        }
                    ),
                    button(
                        text(t!("power_source.on_battery").to_string())
                            .size(design::typography::CAPTION)
                    )
                    .on_press(Message::PowerSource(PowerSourceProfile::OnBattery))
                    .style(
                        if self.power_source == PowerSourceProfile::OnBattery {
                            widgets::selected
                        } else {
                            widgets::quiet
                        }
                    ),
                ]
                .spacing(design::space::TIGHT),
            );
        }
        let description = navigation::page_help(self.page);
        if !description.is_empty() {
            header = header.push(
                button(
                    row![
                        navigation::glyph("icons/info.svg"),
                        text(t!("common.how_it_works").to_string())
                            .size(design::typography::SECONDARY)
                    ]
                    .spacing(design::space::CONTROL)
                    .align_y(iced::Center),
                )
                .style(if self.description_expanded {
                    widgets::selected
                } else {
                    widgets::quiet
                })
                .on_press(Message::ToggleDescription),
            );
        }
        let header: Element<'_, Message> = if width >= design::SIDE_PANEL_BREAKPOINT {
            header.into()
        } else {
            header.wrap().into()
        };
        let mut body = column![container(header)
            .padding([design::space::SMALL as u16, 0])
            .width(Fill)]
        .spacing(design::space::MEDIUM)
        .height(Fill);
        if self.description_expanded && !description.is_empty() {
            body = body.push(
                container(scrollable(text(description).width(Fill)).height(iced::Length::Shrink))
                    .max_height(160)
                    .padding(design::space::MEDIUM as u16)
                    .width(Fill)
                    .style(widgets::surface),
            );
        }
        let content = self.page_view();
        let mut side_panel = if self.page == Page::AdaptiveEngine {
            Some(
                self.adaptive
                    .side_panel(&self.settings, &self.status)
                    .map(Message::Adaptive),
            )
        } else if self.page == Page::AdvancedPowerPlanTuning {
            Some(
                self.power_tuning
                    .side_panel(&self.settings.advanced_power_plan_tuning_presets)
                    .map(Message::PowerTuning),
            )
        } else if !matches!(self.page, Page::CpuSetsSoft | Page::ProcessorAffinityHard) {
            status_rail::view(self.page, &self.settings, &self.status, &self.power_plans)
                .map(|panel| panel.map(Message::Status))
        } else {
            None
        };
        body = body.push(content);
        if let Some(error) = &self.status.worker_error {
            body = body.push(text(error));
        }
        if !self.message.is_empty() {
            body = body.push(text(&self.message));
        }
        if side_panel.is_some() && width < design::SIDE_PANEL_BREAKPOINT {
            body = body.push(
                button(
                    row![
                        text(t!("common.status").to_string()),
                        text(t!("adaptive_engine.presets").to_string()),
                        navigation::glyph(if self.compact_panel_open {
                            "icons/chevron-down.svg"
                        } else {
                            "icons/chevron-right.svg"
                        })
                    ]
                    .spacing(design::space::MEDIUM)
                    .align_y(iced::Center),
                )
                .style(widgets::quiet)
                .on_press(Message::ToggleCompactPanel),
            );
            if self.compact_panel_open {
                if let Some(panel) = side_panel.take() {
                    body = body.push(container(panel).height(220));
                }
            }
        }
        let layout = row![
            container(
                container(column![scrollable(navigation).height(Fill), utilities].height(Fill))
                    .width(Fill)
                    .height(Fill)
                    .style(widgets::navigation_surface)
            )
            .width(if !collapsed {
                design::NAVIGATION_WIDTH
            } else {
                design::NAVIGATION_COLLAPSED_WIDTH
            }),
            container(
                container(body)
                    .max_width(design::CONTENT_WIDTH)
                    .width(Fill)
                    .height(Fill)
            )
            .padding([design::space::SECTION as u16, design::space::WIDE as u16])
            .center_x(Fill)
            .height(Fill)
        ]
        .spacing(design::space::SMALL)
        .height(Fill);
        let layout: Element<'_, Message> =
            if let Some(panel) = side_panel.filter(|_| width >= design::SIDE_PANEL_BREAKPOINT) {
                layout
                    .push(
                        container(
                            column![
                                container(widgets::optional_content(panel, !self.status_collapsed))
                                    .height(Fill),
                                button(
                                    row![
                                        navigation::glyph(if self.status_collapsed {
                                            "icons/panel-right-open.svg"
                                        } else {
                                            "icons/panel-right-close.svg"
                                        }),
                                        text(if self.status_collapsed {
                                            String::new()
                                        } else {
                                            t!("nav.collapse_side_panel").to_string()
                                        })
                                    ]
                                    .spacing(design::space::SMALL)
                                    .align_y(iced::Center)
                                )
                                .style(widgets::quiet)
                                .on_press(Message::ToggleStatus)
                            ]
                            .spacing(design::space::MEDIUM)
                            .padding(design::space::MEDIUM as u16)
                            .height(Fill),
                        )
                        .width(if !self.status_collapsed {
                            design::STATUS_WIDTH
                        } else {
                            design::STATUS_COLLAPSED_WIDTH
                        }),
                    )
                    .into()
            } else {
                layout.into()
            };
        if self.pending_changes() {
            iced::widget::stack![
                layout,
                container(
                    widgets::settings_card(
                        column![
                            widgets::heading(
                                t!("unsaved.title").to_string(),
                                design::typography::BODY
                            ),
                            text(t!("unsaved.message").to_string()),
                            row![
                                button(text(t!("common.discard").to_string()))
                                    .style(widgets::quiet)
                                    .on_press(Message::Cancel),
                                button(text(t!("common.save").to_string())).on_press(Message::Save)
                            ]
                            .spacing(design::space::SMALL)
                        ]
                        .spacing(design::space::MEDIUM)
                    )
                    .width(360)
                )
                .padding(design::space::LARGE as u16)
                .width(Fill)
                .height(Fill)
                .align_x(iced::Right)
                .align_y(iced::Bottom)
            ]
            .into()
        } else {
            layout
        }
    }

    fn page_view(&self) -> Element<'_, Message> {
        let general = &self.settings.general;
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
                .view(power_rules::Kind::Time, &self.settings, &self.power_plans)
                .map(|m| Message::PowerRules(power_rules::Kind::Time, m)),
            Page::ByCpuLoad => self
                .cpu_rules
                .view(
                    power_rules::Kind::CpuLoad,
                    &self.settings,
                    &self.power_plans,
                )
                .map(|m| Message::PowerRules(power_rules::Kind::CpuLoad, m)),
            Page::BackgroundEfficiency => self
                .efficiency
                .view(&self.settings, &self.candidates)
                .map(Message::Efficiency),
            Page::CpuSetsSoft => self
                .soft_allocation
                .view(
                    &self.settings,
                    cpu_allocation::Kind::Soft,
                    &self.candidates,
                    &self.status,
                )
                .map(|m| Message::Allocation(cpu_allocation::Kind::Soft, m)),
            Page::ProcessorAffinityHard => self
                .hard_allocation
                .view(
                    &self.settings,
                    cpu_allocation::Kind::Hard,
                    &self.candidates,
                    &self.status,
                )
                .map(|m| Message::Allocation(cpu_allocation::Kind::Hard, m)),
            Page::AdaptiveEngine => self
                .adaptive
                .view(&self.settings, &self.status, &self.candidates)
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
                .view(&self.settings.cpu_limiter, &self.candidates)
                .map(Message::CpuLimiter),
            Page::ProcessList => self
                .processes
                .view(&self.settings, &self.status, &self.power_plans)
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
                .view(self.page, &self.settings)
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
                .view(
                    &self.status.action_log_entries,
                    !self.status.action_log_summaries.is_empty(),
                )
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
                    navigation::icon(*page),
                    widgets::heading(page.label(), design::typography::BODY).width(Fill)
                ]
                .spacing(design::space::COMPACT)
                .align_y(iced::Center);
                if self.settings.general.show_feature_status_on_cards {
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
