//! Wind UI application owner. Editor and runtime state outlive every UI window.
use super::{
    action_log, adaptive_engine, advanced_power_plan_tuning, app_suspension, background_efficiency,
    by_activity, cpu_allocation, cpu_limiter, design, home, memory_trim, navigation, power_rules,
    priority_control, process_list, process_power_plans, settings_pages, status_rail, tasks,
    timer_resolution, widgets, win32_priority_separation,
};
use crate::{
    application::{settings::SettingsRevision, SettingsEditor},
    automation::{RuntimeHandle, RuntimeStatusSnapshot},
    backend::{diagnostics, window_visibility},
    config::PowerSourceProfile,
    file_dialog::{choose_settings_file, FileDialogMode},
    platform::windows::resident_shell::{
        close_ui_windows, ui_pointer_capture_active, NavigationDirection, ResidentShell,
        ResidentShellError, RestoreListener,
    },
    tray,
    ui::Page,
    SingleInstanceRestoreEvent,
};
use rust_i18n::t;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::{Rc, Weak},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tasks::Task;
use widgets::{card, col, heading, label, row, setting_row, Ui};
use windui::{core::EventCtx, prelude::*, signal::SignalScope};
#[cfg(feature = "render-smoke")]
#[path = "smoke.rs"]
pub(crate) mod smoke;
struct Intent {
    message: Message,
    refresh: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortcutAction {
    Save,
    History(bool),
    DeleteFocused,
}
#[derive(Clone, Copy)]
struct ViewState {
    candidate_catalog: Signal<Vec<super::app_picker::Candidate>>,
    plan_catalog: Signal<Vec<crate::power::PowerPlan>>,
    shortcut: Signal<(u64, ShortcutAction)>,
    compact: Signal<bool>,
    width: Signal<i32>,
    layout: Signal<Vec<u64>>,
    page: Signal<Vec<u64>>,
    navigation: Signal<Vec<u64>>,
    header: Signal<Vec<u64>>,
    side: Signal<Vec<u64>>,
    overlay: Signal<Vec<u64>>,
    footer: Signal<Vec<u64>>,
    pending_refresh: Signal<u8>,
    can_hide: Signal<bool>,
    startup_hide: Signal<bool>,
    hide_to_tray: Signal<bool>,
    editable: Signal<bool>,
    confirming: Signal<bool>,
}
impl ViewState {
    fn new() -> Self {
        Self {
            candidate_catalog: signal(Vec::new()),
            plan_catalog: signal(Vec::new()),
            shortcut: signal((0, ShortcutAction::Save)),
            compact: signal(false),
            width: signal(1120),
            layout: signal(vec![0]),
            page: signal(vec![0]),
            navigation: signal(vec![0]),
            header: signal(vec![0]),
            side: signal(vec![0]),
            overlay: signal(vec![0]),
            footer: signal(vec![0]),
            pending_refresh: signal(0),
            can_hide: signal(false),
            startup_hide: signal(false),
            hide_to_tray: signal(false),
            editable: signal(true),
            confirming: signal(false),
        }
    }

    fn invalidate(self, part: ViewPart) {
        self.pending_refresh
            .update(|pending| *pending |= part as u8);
    }

    fn flush(self, capture_active: impl FnOnce() -> bool) {
        let pending = self.pending_refresh.get();
        if pending == 0 || capture_active() {
            return;
        }
        self.pending_refresh.set(0);
        for (part, revision) in [
            (ViewPart::Layout, self.layout),
            (ViewPart::Page, self.page),
            (ViewPart::Navigation, self.navigation),
            (ViewPart::Header, self.header),
            (ViewPart::SidePanel, self.side),
            (ViewPart::Overlay, self.overlay),
            (ViewPart::Footer, self.footer),
        ] {
            if pending & part as u8 != 0 {
                revision.update(|value| value[0] = value[0].wrapping_add(1));
            }
        }
    }
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum ViewPart {
    Layout = 1,
    Page = 2,
    Navigation = 4,
    Header = 8,
    SidePanel = 16,
    Overlay = 32,
    Footer = 64,
}
pub(crate) fn run(
    settings: SettingsEditor,
    error: Option<String>,
    runtime: RuntimeHandle,
    restore_event: Option<SingleInstanceRestoreEvent>,
    recovery_client: Arc<Mutex<crate::crash_recovery::RecoveryClient>>,
) -> Result<(), ResidentShellError> {
    rust_i18n::set_locale(settings.global().general.language.locale());
    let runtime = Arc::new(runtime);
    let mut scope = SignalScope::new();
    let view = scope.collect(ViewState::new);
    let state = Rc::new(RefCell::new(WinderustApp {
        restore_listener: None,
        shutdown_failed: false,
        page_offsets: Page::sections()
            .iter()
            .flat_map(|section| {
                std::iter::once(section.landing_page).chain(section.pages.iter().copied())
            })
            .map(|page| (page, Rc::new(Cell::new(0))))
            .collect(),
        weak_self: Weak::new(),
        #[cfg(feature = "render-smoke")]
        smoke: smoke::LifecycleDriver::requested(),
        #[cfg(feature = "render-smoke")]
        resource_benchmark: smoke::ResourceDriver::requested(),
        #[cfg(feature = "render-smoke")]
        wake_count: 0,
        appearance: settings_pages::theme(&settings.general),
        theme_handle: None,
        locale_handle: windui::i18n::LocaleHandle::new(),
        native: None,
        sender: None,
        view,
        ready: false,
        wake_pending: Arc::new(AtomicBool::new(false)),
        start_hidden_pending: settings.persisted().general.start_minimized,
        tray_unavailable: false,
        tray_menu_revision: None,
        navigation_search: String::new(),
        expanded_section: None,
        status_collapsed: false,
        feature_info_expanded: false,
        live_power_source: crate::backend::power_source::is_plugged_in(),
        preferences: settings_pages::Editor::default(),
        home: home::Model::default(),
        action_log: action_log::Editor::default(),
        sampler: std::sync::Arc::new(std::sync::Mutex::new(home::Sampler::default())),
        sampled_at: std::time::Instant::now(),
        sampling: false,
        ui_generation: 0,
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
        runtime: Arc::clone(&runtime),
        status: RuntimeStatusSnapshot::default(),
        error_message: error
            .or_else(crate::crash_recovery::startup_error)
            .unwrap_or_default(),
        page: Page::Home,
        navigation_history: navigation::History::default(),
        breadcrumb: vec![Page::Home],
        restore_event,
        auto_exclusion_generation: 0,
        auto_exclusion_retry: false,
        closing: false,
        hwnd: None,
        color_dialog_open: false,
        recovery_client,
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
    }));
    let mut app = App::resident("Winderust")
        .motion(windui::anim::motion_level())
        .reduced_motion(windui::anim::MotionLevel::Off)
        .theme(state.borrow().appearance.clone())
        .locales(settings_pages::native_locales(
            state.borrow().settings.global().general.language,
        ));
    state.borrow_mut().locale_handle = app.locale_handle();
    state.borrow_mut().theme_handle = Some(app.theme_handle());
    let weak = Rc::downgrade(&state);
    let sender = app.channel(move |ctx, intent: Intent| {
        if let Some(state) = weak.upgrade() {
            state.borrow_mut().handle(ctx, intent);
        }
    });
    state.borrow_mut().sender = Some(sender.clone());
    let restore_event = state.borrow_mut().restore_event.take();
    if let Some(event) = restore_event {
        let restore = sender.clone();
        match RestoreListener::start(event, move || send(&restore, Message::Show, false)) {
            Ok(listener) => state.borrow_mut().restore_listener = Some(listener),
            Err(error) => state.borrow_mut().error_message = error.to_string(),
        }
    }
    let pending = Arc::clone(&state.borrow().wake_pending);
    let wake_sender = sender.clone();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || enqueue_wake(&wake_sender, &pending));
    runtime.set_auto_exclusion_wake(Arc::clone(&wake));
    tray::set_ui_wake(Some(wake));
    let show = sender.clone();
    let quit = sender.clone();
    let weak = Rc::downgrade(&state);
    let tray = Tray::new()
        .tooltip("Winderust")
        .on_left_click(move |_| send(&show, Message::Show, false))
        .menu(vec![TrayMenuItem::item(
            t!("tray.exit").to_string(),
            move |ctx| {
                if weak.upgrade().is_some_and(|s| s.borrow().ready) {
                    ctx.quit();
                } else {
                    send(&quit, Message::Close, true);
                }
            },
        )]);
    if let Some(icon) = app_icon() {
        app = app
            .icon(icon.clone())
            .tray(tray.icon_rgba(icon.width(), icon.height(), icon.rgba()));
    } else {
        app = app.tray(tray);
    }
    state.borrow_mut().weak_self = Rc::downgrade(&state);
    #[cfg(feature = "render-smoke")]
    {
        app = smoke::configure(&state, app);
    }
    #[cfg(feature = "render-smoke")]
    let smoke_requested = smoke::requested();
    #[cfg(not(feature = "render-smoke"))]
    let smoke_requested = false;
    if !smoke_requested {
        app = app.start_window(main_window(&state));
    }
    #[cfg(feature = "render-smoke")]
    let initial_wake = !smoke::requested()
        || smoke::lifecycle_requested()
        || smoke::background_fixture_requested()
        || smoke::resource_requested();
    #[cfg(not(feature = "render-smoke"))]
    let initial_wake = true;
    if initial_wake {
        send(&sender, Message::Wake, false);
    }
    if !smoke_requested && state.borrow().settings.global().general.check_for_updates {
        send(
            &sender,
            Message::Preferences(settings_pages::Message::CheckStartup),
            false,
        );
    }
    app.run_resident();
    #[cfg(feature = "render-smoke")]
    smoke::finish(&state.borrow());
    if let Err(error) = runtime.shutdown() {
        diagnostics::error(&error);
    }
    tray::set_ui_wake(None);
    state.borrow_mut().restore_listener = None;
    state.borrow_mut().native = None;
    window_visibility::set_hidden(false);
    Ok(())
}
struct WinderustApp {
    restore_listener: Option<RestoreListener>,
    page_offsets: HashMap<Page, Rc<Cell<i32>>>,
    shutdown_failed: bool,
    #[cfg(feature = "render-smoke")]
    smoke: Option<smoke::LifecycleDriver>,
    #[cfg(feature = "render-smoke")]
    resource_benchmark: Option<smoke::ResourceDriver>,
    #[cfg(feature = "render-smoke")]
    wake_count: u64,
    weak_self: Weak<RefCell<WinderustApp>>,
    appearance: windui::theme::Theme,
    theme_handle: Option<windui::app::ThemeHandle>,
    locale_handle: windui::i18n::LocaleHandle,
    native: Option<ResidentShell>,
    sender: Option<Sender<Intent>>,
    view: ViewState,
    ready: bool,
    wake_pending: Arc<AtomicBool>,
    start_hidden_pending: bool,
    tray_unavailable: bool,
    tray_menu_revision: Option<(
        SettingsRevision,
        PowerSourceProfile,
        crate::config::AppLanguage,
    )>,
    navigation_search: String,
    expanded_section: Option<Page>,
    status_collapsed: bool,
    feature_info_expanded: bool,
    live_power_source: Option<bool>,
    preferences: settings_pages::Editor,
    home: home::Model,
    action_log: action_log::Editor,
    sampler: std::sync::Arc<std::sync::Mutex<home::Sampler>>,
    sampled_at: std::time::Instant,
    sampling: bool,
    ui_generation: u64,
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
    auto_exclusion_generation: u64,
    auto_exclusion_retry: bool,
    closing: bool,
    hwnd: Option<usize>,
    color_dialog_open: bool,
    recovery_client: Arc<Mutex<crate::crash_recovery::RecoveryClient>>,
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
    Show,
    WorkerFailed(String),
    TrayAvailability(Result<bool, ResidentShellError>),
    NavigationSearch(String),
    DismissError,
    NavigateHistory(bool),
    DeleteFocused,
    ToggleNavigation,
    ToggleSection(Page),
    ToggleFeatureInfo,
    ToggleStatus,
    Preferences(settings_pages::Message),
    ColorChosen(Option<u32>, Result<Option<u32>, String>),
    Status(status_rail::Message),
    Home(home::Message),
    Sample(u64, Result<home::Sample, String>),
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
    Catalog(u64, Result<Vec<super::app_picker::Candidate>, String>),
    ProcessListLoaded(u64, Result<process_list::Population, String>),
    ExecutableChosen(Page, PowerSourceProfile, Option<PathBuf>),
    Page(Page),
    Tick,
    Close,
    Save,
    Cancel,
    PausePowerPlans(bool),
    SettingsFile(FileDialogMode),
    SettingsFileChosen(FileDialogMode, Option<PathBuf>),
    Stay,
    DiscardAndClose,
    ShutdownFinished(Result<(), String>),
    ShutdownStartFailed(String),
    RetryShutdown,
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
                | Self::ShutdownStartFailed(_)
                | Self::RetryShutdown
                | Self::Sample(..)
                | Self::Catalog(..)
                | Self::ProcessListLoaded(..)
                | Self::PowerPlans(_)
                | Self::Processes(process_list::Message::Loaded(_))
                | Self::Preferences(settings_pages::Message::Checked(..))
        )
    }
}
impl WinderustApp {
    fn process_task(&self, task: Task<process_list::Message>) -> Task<Message> {
        let generation = self.ui_generation;
        task.map(move |message| match message {
            process_list::Message::Loaded(result) => Message::ProcessListLoaded(generation, result),
            message => Message::Processes(message),
        })
    }

    fn release_window_resources(&mut self) {
        self.ui_generation = self.ui_generation.wrapping_add(1);
        let released = self
            .sampler
            .try_lock()
            .map(|mut sampler| sampler.release())
            .is_ok();
        if !released {
            // A running sample owns its old Arc until completion. Never wait for it on
            // the UI thread or allow that completion to refill a recreated window.
            self.sampler = Arc::new(Mutex::new(home::Sampler::default()));
        }
        self.processes.release_presentation();
        self.action_log.release_presentation();
        self.candidates = Vec::new();
        self.unavailable_candidates = Vec::new();
        self.view.candidate_catalog.set(Vec::new());
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        if self.exiting && !message.allowed_during_exit() {
            // Discard native picker edits, but release its pending flag.
            if matches!(message, Message::ColorChosen(..)) {
                self.color_dialog_open = false;
            }
            return Task::none();
        }
        match message {
            Message::Show
            | Message::TrayAvailability(_)
            | Message::ShutdownFinished(_)
            | Message::ShutdownStartFailed(_)
            | Message::RetryShutdown => {
                unreachable!("Shell intents are handled before editor dispatch")
            }
            Message::WorkerFailed(error) => self.error_message = error,
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
            Message::ToggleFeatureInfo => {
                self.feature_info_expanded = !self.feature_info_expanded;
                if self.feature_info_expanded {
                    self.page_offsets
                        .get(&self.page)
                        .unwrap_or_else(|| unreachable!("Every page must have a scroll state"))
                        .set(0);
                }
            }
            Message::ToggleSection(page) => {
                if self.page != page {
                    return self.update(Message::Page(page));
                }
                self.expanded_section = (self.expanded_section != Some(page)).then_some(page);
            }
            Message::ToggleStatus => {
                self.status_collapsed = !self.status_collapsed;
            }
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
                return self.update(Message::Page(page));
            }
            Message::Home(home::Message::PauseMetrics(value)) => {
                self.home.metrics_paused = value;
            }
            Message::Sample(generation, result) => {
                self.sampling = false;
                if generation != self.ui_generation || self.hidden {
                    return Task::none();
                }
                match result {
                    Ok(sample) if !self.home.metrics_paused => self.home.record(sample),
                    Ok(_) => {}
                    Err(error) => self.error_message = error,
                }
            }
            Message::Preferences(settings_pages::Message::Export) => {
                return self.update(Message::SettingsFile(FileDialogMode::Save));
            }
            Message::Preferences(settings_pages::Message::Import) => {
                return self.update(Message::SettingsFile(FileDialogMode::Open));
            }
            Message::Preferences(
                message @ (settings_pages::Message::Check | settings_pages::Message::CheckStartup),
            ) => {
                let automatic = matches!(message, settings_pages::Message::CheckStartup);
                let channel = self.settings.global().general.update_channel;
                if !self.preferences.begin_check(automatic) {
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
            Message::Preferences(settings_pages::Message::EditColor(color)) => {
                let original = Some(color);
                if self.color_dialog_open {
                    return Task::none();
                }
                if let Some(owner) = self.hwnd {
                    let accent = &self.settings.global().general.accent;
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
                        }));
                    }
                    Ok(None) => {}
                    Err(error) => self.error_message = error,
                }
            }
            Message::Preferences(
                message @ (settings_pages::Message::ToggleAccent
                | settings_pages::Message::DismissUpdate
                | settings_pages::Message::Checked(..)),
            ) => {
                self.preferences
                    .update_ui(self.settings.global().general.update_channel, message);
            }
            Message::Preferences(message) => {
                let language_changed = matches!(message, settings_pages::Message::Language(_));
                let appearance_changed = matches!(
                    message,
                    settings_pages::Message::Theme(_)
                        | settings_pages::Message::Animation(_)
                        | settings_pages::Message::AccentSource(_)
                        | settings_pages::Message::AccentHex(_)
                        | settings_pages::Message::Accent(_)
                        | settings_pages::Message::ReplaceColor(..)
                );
                let tray_changed = matches!(
                    message,
                    settings_pages::Message::Language(_)
                        | settings_pages::Message::Flag(
                            settings_pages::Flag::Tray
                                | settings_pages::Flag::Minimized
                                | settings_pages::Flag::AdvancedControls,
                            _
                        )
                );
                self.settings
                    .edit_global(|settings| self.preferences.update(settings, message));
                if language_changed {
                    self.sync_language();
                }
                if appearance_changed {
                    self.appearance = settings_pages::theme(&self.settings.global().general);
                }
                if tray_changed {
                    self.sync_tray();
                }
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
            }
            Message::ActionLog(action_log::Message::Export) => {
                return Task::perform(
                    crate::file_dialog::choose_action_log_export_file(
                        self.hwnd.map(|h| h as windows_sys::Win32::Foundation::HWND),
                    ),
                    Message::ExportLog,
                );
            }
            Message::ActionLog(message) => {
                self.action_log.update(message);
                self.action_log
                    .refresh_view(&self.status.action_log_entries);
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
                return self.browse(Page::AppSuspension);
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
                            .map(|r| Message::CommandFinished(r.and_then(|r| r)));
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
                        .map(|r| Message::CommandFinished(r.and_then(|r| r)));
                    }
                    Err(error) => self.error_message = error.to_string(),
                }
            }
            Message::Trim(message) => self.trim.update(&mut self.settings.memory_trim, message),
            Message::Timer(timer_resolution::Message::Browse) => {
                return self.browse(Page::TimerResolution);
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
                return self.browse(priority_page(kind));
            }
            Message::Priority(kind, message) => {
                self.priority.update(&mut self.settings, kind, message)
            }
            Message::Efficiency(background_efficiency::Message::Browse) => {
                return self.browse(Page::BackgroundEfficiency);
            }
            Message::Efficiency(message) => self.efficiency.update(&mut self.settings, message),
            Message::Allocation(_, cpu_allocation::Message::Status(message)) => {
                return self.update(Message::Status(message));
            }
            Message::Allocation(kind, cpu_allocation::Message::Browse) => {
                return self.browse(match kind {
                    cpu_allocation::Kind::Soft => Page::CpuSetsSoft,
                    cpu_allocation::Kind::Hard => Page::ProcessorAffinityHard,
                });
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
                return self.update(Message::Status(message));
            }
            Message::Adaptive(adaptive_engine::Message::Browse) => {
                return self.browse(Page::AdaptiveEngine);
            }
            Message::Adaptive(message) => self
                .settings
                .edit_with_presets(|settings| self.adaptive.update(settings, message)),
            Message::Catalog(generation, result) => {
                self.catalog_loading = false;
                if generation != self.ui_generation || self.hwnd.is_none() {
                    return Task::none();
                }
                match result {
                    Ok(candidates) if !self.processes.population_paused => {
                        self.unavailable_candidates = candidates
                            .iter()
                            .filter(|candidate| !candidate.info.has_suspendable_instance)
                            .map(|candidate| {
                                candidate.info.image_path.to_string_lossy().into_owned()
                            })
                            .collect();
                        self.view.candidate_catalog.set(candidates.clone());
                        self.candidates = candidates;
                        self.action_log
                            .refresh_view(&self.status.action_log_entries);
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
                                ));
                            }
                            Page::MemoryTrim => {
                                return self
                                    .update(Message::Trim(memory_trim::Message::Path(path)));
                            }
                            Page::TimerResolution => {
                                return self
                                    .update(Message::Timer(timer_resolution::Message::Path(path)));
                            }
                            Page::CpuSetsSoft => {
                                return self.update(Message::Allocation(
                                    cpu_allocation::Kind::Soft,
                                    cpu_allocation::Message::Path(path),
                                ));
                            }
                            Page::ProcessorAffinityHard => {
                                return self.update(Message::Allocation(
                                    cpu_allocation::Kind::Hard,
                                    cpu_allocation::Message::Path(path),
                                ));
                            }
                            Page::BackgroundEfficiency => {
                                return self.update(Message::Efficiency(
                                    background_efficiency::Message::Path(path),
                                ));
                            }
                            Page::AdaptiveEngine => {
                                return self.update(Message::Adaptive(
                                    adaptive_engine::Message::Path(path),
                                ));
                            }
                            Page::CpuLimiter => {
                                return self
                                    .update(Message::CpuLimiter(cpu_limiter::Message::Path(path)));
                            }
                            Page::ByForeground => {
                                return self.update(Message::ProcessPowerPlans(
                                    process_power_plans::Kind::Foreground,
                                    process_power_plans::Message::Path(path),
                                ));
                            }
                            Page::ByRunningApp => {
                                return self.update(Message::ProcessPowerPlans(
                                    process_power_plans::Kind::RunningApp,
                                    process_power_plans::Message::Path(path),
                                ));
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
                });
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
                        self.view.plan_catalog.set(plans.clone());
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
                return self.browse(Page::CpuLimiter);
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
            Message::DeleteFocused => {
                if self.page == Page::ProcessList && !self.closing && self.error_message.is_empty()
                {
                    return self.update(Message::Processes(process_list::Message::DeleteFocused));
                }
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
                        let task = self.processes.update(
                            process_list::Message::Refresh,
                            &mut self.settings,
                            &self.runtime,
                        );
                        tasks.push(self.process_task(task));
                    }
                    if (page.supports_power_source_profiles() || page == Page::ActionLog)
                        && !self.catalog_loading
                    {
                        self.catalog_loading = true;
                        let generation = self.ui_generation;
                        tasks.push(
                            tasks::run({
                                let cached = self.candidates.clone();
                                move || super::app_picker::load(cached)
                            })
                            .map(move |result| {
                                Message::Catalog(generation, result.and_then(|result| result))
                            }),
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
            Message::ProcessListLoaded(generation, result) => {
                if generation != self.ui_generation || self.hwnd.is_none() {
                    self.processes.discard_population_result();
                    return Task::none();
                }
                return self.update(Message::Processes(process_list::Message::Loaded(result)));
            }
            Message::Processes(message) => {
                if self.hwnd.is_none() && matches!(message, process_list::Message::Loaded(_)) {
                    self.processes.discard_population_result();
                    return Task::none();
                }
                if self.processes.population_paused
                    && matches!(
                        message,
                        process_list::Message::Refresh | process_list::Message::Loaded(_)
                    )
                {
                    self.processes.discard(message);
                } else {
                    let task = self
                        .processes
                        .update(message, &mut self.settings, &self.runtime);
                    if self.hwnd.is_some() {
                        self.processes.refresh_view();
                    }
                    return self.process_task(task);
                }
            }
            Message::Wake => {
                return self.update(Message::Tick);
            }
            Message::Tick => {
                let live_power_source = crate::backend::power_source::is_plugged_in();
                if self.live_power_source != live_power_source {
                    self.live_power_source = live_power_source;
                    if self.page.supports_power_source_profiles() {
                        self.view.invalidate(ViewPart::Header);
                    }
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
                if tray::take_exit_requested() {
                    return self.update(Message::Close);
                }
                if tray::take_restore_requested() {
                    return self.show_window();
                }
                if tray_error {
                    return self.show_window();
                }
                self.sync_tray();
                let mut work = Vec::new();
                if let Some(status) = self.runtime.status_snapshot_since(self.status.generation) {
                    let log_availability_changed = self.status.action_log_entries.is_empty()
                        != status.action_log_entries.is_empty()
                        || self.status.action_log_summaries.is_empty()
                            != status.action_log_summaries.is_empty();
                    if status.appearance_change_generation
                        != self.status.appearance_change_generation
                    {
                        self.appearance = settings_pages::theme(&self.settings.global().general);
                        if let Some(theme) = &self.theme_handle {
                            theme.set(self.appearance.clone());
                        }
                    }
                    self.processes.sync_suspended_processes(
                        &status.feature_status.app_suspension.suspended_process_ids,
                    );
                    self.status = status;
                    if self.side_panel_shows_live_status()
                        || (self.page == Page::ActionLog && log_availability_changed)
                    {
                        self.view.invalidate(ViewPart::SidePanel);
                    }
                    self.action_log
                        .refresh_view(&self.status.action_log_entries);
                    if self.hwnd.is_some() {
                        self.processes.refresh_view();
                    }
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
                        let task = self.processes.update(
                            process_list::Message::Refresh,
                            &mut self.settings,
                            &self.runtime,
                        );
                        work.push(self.process_task(task));
                    }
                    if (self.page.supports_power_source_profiles() || self.page == Page::ActionLog)
                        && !self.catalog_loading
                        && self.catalog_sampled_at.elapsed() >= Duration::from_secs(3)
                    {
                        self.catalog_loading = true;
                        self.catalog_sampled_at = std::time::Instant::now();
                        let generation = self.ui_generation;
                        work.push(
                            tasks::run({
                                let cached = self.candidates.clone();
                                move || super::app_picker::load(cached)
                            })
                            .map(move |r| Message::Catalog(generation, r.and_then(|r| r))),
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
                    let generation = self.ui_generation;
                    work.push(
                        tasks::run(move || {
                            sampler
                                .lock()
                                .map_err(|e| e.to_string())
                                .and_then(|mut sampler| sampler.sample())
                        })
                        .map(move |r| Message::Sample(generation, r.and_then(|r| r))),
                    );
                }
                return Task::batch(work);
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
                if self.timer.has_invalid_inputs()
                    || self.preferences.has_invalid_inputs()
                    || self.trim.has_invalid_inputs()
                    || self.suspension.has_invalid_inputs() =>
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
                self.sync_language();
                self.publish_settings();
                self.closing = false;
            }
            Message::PausePowerPlans(value) => self.settings.edit_global(|settings| {
                settings.general.pause_power_plan_switching_while_plugged_in = value
            }),
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
                                self.sync_language();
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
    fn sync_language(&self) {
        let locale = self.settings.global().general.language.locale();
        rust_i18n::set_locale(locale);
        if !self.locale_handle.set(locale) {
            diagnostics::error(&format!(
                "Wind UI locale is missing from the bundled catalog: {locale}"
            ));
        }
    }
    fn pending_changes(&self) -> bool {
        self.settings.has_unsaved_changes()
            || self.power_tuning.dirty
            || self.pending_editor()
            || self.invalid_inputs()
    }
    fn invalid_inputs(&self) -> bool {
        self.cpu_limiter.has_invalid_inputs()
            || self.trim.has_invalid_inputs()
            || self.suspension.has_invalid_inputs()
            || self.timer.has_invalid_inputs()
            || self.preferences.has_invalid_inputs()
            || self.adaptive.validation_error().is_some()
            || !self.activity_inputs.valid()
            || !self.time_rules.valid()
            || !self.cpu_rules.valid()
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
    fn load_power_plans(&mut self) -> Task<Message> {
        if self.power_plans_loading {
            return Task::none();
        }
        self.power_plans_loading = true;
        self.power_plans_loaded = false;
        tasks::run(crate::power::list_plans)
            .map(|result| Message::PowerPlans(result.and_then(|result| result)))
    }
    fn sync_tray(&mut self) {
        let profile = if crate::backend::power_source::is_plugged_in() != Some(false) {
            PowerSourceProfile::PluggedIn
        } else {
            PowerSourceProfile::OnBattery
        };
        let revision = (
            self.settings.base_revision(),
            profile,
            self.settings.global().general.language,
        );
        if self.tray_menu_revision == Some(revision) {
            return;
        }
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
        self.tray_menu_revision = Some(revision);
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
fn page_needs_power_plans(page: Page) -> bool {
    page == Page::ProcessList || page.section_landing_page() == Page::PowerPlanControl
}
impl WinderustApp {
    fn sender(&self) -> Sender<Intent> {
        self.sender
            .as_ref()
            .unwrap_or_else(|| {
                unreachable!("Application channel must be installed before dispatch")
            })
            .clone()
    }
    fn open(&mut self, ctx: &mut EventCtx) {
        self.start_hidden_pending = false;
        self.view.startup_hide.set(false);
        ctx.open_window(window_request(
            self.weak_self.clone(),
            self.view,
            self.sender(),
        ));
    }
    fn handle(&mut self, ctx: &mut EventCtx, intent: Intent) {
        let Intent { message, refresh } = intent;
        if self.exiting
            && !message.allowed_during_exit()
            && !matches!(
                message,
                Message::Wake | Message::Tick | Message::TrayAvailability(_)
            )
        {
            if matches!(message, Message::ColorChosen(..)) {
                self.color_dialog_open = false;
            }
            return;
        }
        let page_before = self.page;
        let error_before = self.error_message.clone();
        let pending_before = self.pending_changes();
        let closing_before = self.closing;
        let update_notice_before = self.preferences.show_update;
        let refresh_overlay = self.page == Page::ProcessList
            && self.processes.shows_immediate_selection()
            && matches!(
                &message,
                Message::Processes(process_list::Message::Current(_))
            );
        let refresh_page = refresh
            || (self.page == Page::About
                && matches!(
                    &message,
                    Message::Preferences(settings_pages::Message::Checked(..))
                ));
        let refresh_page = refresh_page
            || (self.page == Page::Home
                && !self.hidden
                && matches!(message, Message::Sample(generation, _) if generation == self.ui_generation))
            || (self.page == Page::AdvancedPowerPlanTuning
                && matches!(message, Message::PowerPlans(_))
                && !self.power_tuning.dirty
                && !self.pending_editor()
                && !self.invalid_inputs());
        let navigation_search = matches!(message, Message::NavigationSearch(_));
        let layout_changed = matches!(
            message,
            Message::ToggleNavigation
                | Message::ToggleStatus
                | Message::Page(_)
                | Message::Preferences(settings_pages::Message::Language(_))
                | Message::Cancel
                | Message::SettingsFileChosen(..)
        );
        let appearance_changed = matches!(
            &message,
            Message::Preferences(
                settings_pages::Message::Theme(_)
                    | settings_pages::Message::Animation(_)
                    | settings_pages::Message::AccentSource(_)
                    | settings_pages::Message::AccentHex(_)
                    | settings_pages::Message::Accent(_)
                    | settings_pages::Message::ReplaceColor(..)
            ) | Message::ColorChosen(..)
                | Message::Cancel
                | Message::SettingsFileChosen(..)
        );
        let task = match message {
            Message::Show => {
                if let Some(native) = &self.native {
                    native.refresh_tray();
                }
                self.open(ctx);
                Task::none()
            }
            Message::TrayAvailability(result) => {
                match result {
                    Ok(true) => {
                        self.view.can_hide.set(true);
                        if self.tray_unavailable {
                            self.tray_unavailable = false;
                            if self.error_message == t!("tray.unavailable").as_ref() {
                                self.error_message.clear();
                            }
                        }
                        if self.start_hidden_pending {
                            self.start_hidden_pending = false;
                            self.view.startup_hide.set(true);
                            if let Err(error) = close_ui_windows() {
                                self.view.startup_hide.set(false);
                                self.error_message = error.to_string();
                            }
                        }
                    }
                    Ok(false) => self.tray_unavailable(ctx),
                    Err(error) => {
                        self.tray_unavailable(ctx);
                        diagnostics::error(&error.to_string());
                    }
                }
                Task::none()
            }
            Message::ShutdownStartFailed(error) => {
                self.shutdown_failed = true;
                self.error_message = error;
                self.open(ctx);
                Task::none()
            }
            Message::RetryShutdown => self.shutdown(),
            Message::ShutdownFinished(result) => {
                if let Err(error) = result {
                    diagnostics::error(&error);
                    self.error_message = error;
                }
                self.ready = true;
                if let Some(native) = &self.native {
                    if let Err(error) = native.request_quit() {
                        self.ready = false;
                        self.shutdown_failed = true;
                        self.error_message = error.to_string();
                        self.open(ctx);
                    }
                } else {
                    self.ready = false;
                    self.shutdown_failed = true;
                    self.error_message = ResidentShellError::HostNotFound.to_string();
                    self.open(ctx);
                }
                Task::none()
            }
            Message::Wake => {
                #[cfg(feature = "render-smoke")]
                {
                    self.wake_count += 1;
                }
                self.wake_pending.store(false, Ordering::Release);
                self.native_tick(ctx);
                if self.exiting {
                    Task::none()
                } else {
                    self.update(Message::Wake)
                }
            }
            Message::Tick => {
                self.native_tick(ctx);
                if self.exiting {
                    Task::none()
                } else {
                    self.update(Message::Tick)
                }
            }
            message => self.update(message),
        };
        self.view.editable.set(!self.exiting);
        self.view.confirming.set(self.closing);
        self.view
            .hide_to_tray
            .set(self.settings.global().general.hide_to_tray);
        if self.closing {
            self.start_hidden_pending = false;
            self.view.startup_hide.set(false);
        }
        if appearance_changed {
            if let Some(theme) = &self.theme_handle {
                theme.set(self.appearance.clone());
            }
        }
        if navigation_search {
            self.view.invalidate(ViewPart::Navigation);
        }
        if layout_changed || page_before != self.page {
            self.view.invalidate(ViewPart::Layout);
        }
        if refresh_page {
            self.view.invalidate(ViewPart::Page);
        }
        if refresh {
            self.view.invalidate(ViewPart::Navigation);
            self.view.invalidate(ViewPart::Header);
            self.view.invalidate(ViewPart::SidePanel);
        }
        if refresh
            || closing_before != self.closing
            || error_before != self.error_message
            || update_notice_before != self.preferences.show_update
            || refresh_overlay
        {
            self.view.invalidate(ViewPart::Overlay);
        }
        if refresh || pending_before != self.pending_changes() || closing_before != self.closing {
            self.view.invalidate(ViewPart::Footer);
        }
        if let Some(native) = &self.native {
            if let Err(error) = native.set_retry_pending(self.auto_exclusion_retry && !self.exiting)
            {
                self.error_message = error.to_string();
                self.view.can_hide.set(false);
                self.open(ctx);
            }
        }
        // Replacing a pressed native control leaves Wind UI 0.28 holding a dead capture.
        // The existing UI Tick drains coalesced refreshes after release/capture loss.
        self.processes.flush_pending_refresh();
        self.action_log
            .flush_pending_refresh(&self.status.action_log_entries);
        self.view.flush(ui_pointer_capture_active);
        let sender = self.sender();
        let failed = sender.clone();
        task.dispatch(
            move |message| send(&sender, message, false),
            move |error| send(&failed, Message::WorkerFailed(error), true),
        );
    }
    fn native_tick(&mut self, ctx: &mut EventCtx) {
        let had_window = self.hwnd.is_some();
        if self.native.is_none() {
            let sender = self.sender();
            match ResidentShell::attach(move || send(&sender, Message::Close, true)) {
                Ok(mut native) => {
                    let navigation = self.sender();
                    if let Err(error) = native.listen_navigation(move |direction| {
                        send(
                            &navigation,
                            Message::NavigateHistory(direction == NavigationDirection::Forward),
                            true,
                        );
                    }) {
                        self.error_message = error.to_string();
                    }
                    let visibility_ready = match native.listen_visibility(tray::wake_ui) {
                        Ok(()) => true,
                        Err(error) => {
                            self.error_message = error.to_string();
                            self.view.can_hide.set(false);
                            self.open(ctx);
                            false
                        }
                    };
                    #[cfg(feature = "render-smoke")]
                    let probing_enabled = !smoke::requested()
                        || smoke::background_fixture_requested()
                        || smoke::resource_requested();
                    #[cfg(not(feature = "render-smoke"))]
                    let probing_enabled = true;
                    if probing_enabled && visibility_ready {
                        let sender = self.sender();
                        if let Err(error) = native.listen_tray(move |result| {
                            send(&sender, Message::TrayAvailability(result), false)
                        }) {
                            self.error_message = error.to_string();
                            self.view.can_hide.set(false);
                            self.open(ctx);
                        }
                    }
                    self.native = Some(native);
                }
                Err(error) => {
                    self.error_message = error.to_string();
                    self.open(ctx);
                    return;
                }
            }
        }
        if let Some(native) = &self.native {
            match native.visibility() {
                Ok(visibility) => {
                    self.hidden = !visibility.is_visible();
                    window_visibility::set_hidden(self.hidden);
                }
                Err(error) => self.error_message = error.to_string(),
            }
            match native.ui_window() {
                Ok(hwnd) => self.hwnd = hwnd,
                Err(error) => self.error_message = error.to_string(),
            }
        }
        if had_window && self.hwnd.is_none() {
            self.release_window_resources();
            if let Some(native) = &self.native {
                native.refresh_tray();
            }
        }
        #[cfg(feature = "render-smoke")]
        self.smoke_tick(ctx);
    }
    #[cfg(feature = "render-smoke")]
    fn smoke_tick(&mut self, ctx: &mut EventCtx) {
        smoke::tick(self, ctx);
    }
    fn tray_unavailable(&mut self, ctx: &mut EventCtx) {
        self.view.can_hide.set(false);
        self.view.startup_hide.set(false);
        if !self.tray_unavailable {
            self.tray_unavailable = true;
            self.error_message = t!("tray.unavailable").to_string();
        }
        if self.hidden {
            self.open(ctx);
        }
    }
    fn publish_settings(&mut self) {
        self.runtime
            .replace_settings(&self.settings.runtime_settings_snapshot());
        self.sync_tray();
    }
    fn show_window(&mut self) -> Task<Message> {
        Task::done(Message::Show)
    }
    fn shutdown(&mut self) -> Task<Message> {
        diagnostics::event("Exit requested; restoring managed state.");
        self.exiting = true;
        self.closing = true;
        self.shutdown_failed = false;
        self.error_message.clear();
        let runtime = Arc::clone(&self.runtime);
        let recovery = Arc::clone(&self.recovery_client);
        let sender = self.sender();
        let result = std::thread::Builder::new()
            .name("winderust-shutdown".into())
            .spawn(move || {
                let message = complete_shutdown(
                    || runtime.shutdown(),
                    || {
                        recovery
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .finish()
                    },
                );
                send(&sender, message, true);
            });
        if let Err(error) = result {
            self.shutdown_failed = true;
            self.error_message = format!("Could not start managed-state cleanup: {error}");
        }
        Task::none()
    }
    fn page_view(&self, ui: Ui<Message>) -> Element {
        let settings = &self.settings;
        match self.page {
            Page::AppSuspension => self.suspension.view(
                &settings.app_suspension,
                &self.status.feature_status.app_suspension,
                &self.unavailable_candidates,
                &self.candidates,
                ui.map(Message::Suspension),
            ),
            Page::MemoryTrim => self.trim.view(
                &settings.memory_trim,
                &self.candidates,
                ui.map(Message::Trim),
            ),
            Page::TimerResolution => self.timer.view(
                &settings.timer_resolution,
                &self.status.feature_status.timer_resolution,
                &self.candidates,
                ui.map(Message::Timer),
            ),
            Page::Win32PrioritySeparation => self
                .priority_separation
                .view(ui.map(Message::PrioritySeparation)),
            Page::AdvancedPowerPlanTuning => self.power_tuning.view(
                &settings.advanced_power_plan_tuning_presets,
                &self.power_plans,
                self.effective_power_mode
                    .as_ref()
                    .map_or(crate::power::EffectivePowerMode::Unknown, |monitor| {
                        monitor.snapshot()
                    }),
                ui.map(Message::PowerTuning),
            ),
            Page::ByTime => self.time_rules.view(
                power_rules::Kind::Time,
                settings,
                &self.status.power_plan_status,
                ui.map(|message| Message::PowerRules(power_rules::Kind::Time, message)),
            ),
            Page::ByCpuLoad => self.cpu_rules.view(
                power_rules::Kind::CpuLoad,
                settings,
                &self.status.power_plan_status,
                ui.map(|message| Message::PowerRules(power_rules::Kind::CpuLoad, message)),
            ),
            Page::BackgroundEfficiency => {
                self.efficiency
                    .view(settings, &self.candidates, ui.map(Message::Efficiency))
            }
            Page::CpuSetsSoft => self.soft_allocation.view(
                settings,
                cpu_allocation::Kind::Soft,
                &self.candidates,
                ui.map(|message| Message::Allocation(cpu_allocation::Kind::Soft, message)),
            ),
            Page::ProcessorAffinityHard => self.hard_allocation.view(
                settings,
                cpu_allocation::Kind::Hard,
                &self.candidates,
                ui.map(|message| Message::Allocation(cpu_allocation::Kind::Hard, message)),
            ),
            Page::AdaptiveEngine => {
                self.adaptive
                    .view(settings, &self.candidates, ui.map(Message::Adaptive))
            }
            Page::ProcessPriority
            | Page::ThreadPriority
            | Page::DynamicPriorityBoost
            | Page::IoPriority
            | Page::GpuPriority
            | Page::MemoryPriority => {
                let kind = priority_kind(self.page).unwrap_or_else(|| {
                    unreachable!("Priority page dispatch must have a priority kind")
                });
                self.priority.view(
                    settings,
                    kind,
                    &self.candidates,
                    ui.map(move |message| Message::Priority(kind, message)),
                )
            }
            Page::ByForeground | Page::ByRunningApp => {
                let kind = if self.page == Page::ByForeground {
                    process_power_plans::Kind::Foreground
                } else {
                    process_power_plans::Kind::RunningApp
                };
                let editor = if self.page == Page::ByForeground {
                    &self.foreground_plans
                } else {
                    &self.running_app_plans
                };
                editor.view(
                    kind,
                    settings,
                    &self.power_plans,
                    &self.candidates,
                    &self.status.power_plan_status,
                    ui.map(move |message| Message::ProcessPowerPlans(kind, message)),
                )
            }
            Page::ByActivity => by_activity::view(
                settings,
                &self.power_plans,
                &self.activity_inputs,
                ui.map(Message::ByActivity),
            ),
            Page::CpuLimiter => self.cpu_limiter.view(
                &settings.cpu_limiter,
                &self.candidates,
                settings.global().general.enabled,
                &self.status.feature_status.cpu_limiter,
                ui.map(Message::CpuLimiter),
            ),
            Page::ProcessList => self.processes.view(
                settings,
                &self.status,
                if self.power_plans_loading {
                    process_list::PlanCatalog::Loading
                } else if self.power_plans_loaded {
                    process_list::PlanCatalog::Loaded(&self.power_plans)
                } else {
                    process_list::PlanCatalog::Unavailable
                },
                ui.map(Message::Processes),
            ),
            Page::Home => {
                let weak = self.weak_self.clone();
                super::viewport::home(
                    self.view.width,
                    settings.global().general.navigation_collapsed,
                    move |wide| {
                        let Some(state) = weak.upgrade() else {
                            return Element::col();
                        };
                        let app = state.borrow();
                        app.home.view(
                            &app.settings,
                            &app.status.feature_status,
                            ui.map(Message::Home),
                            wide,
                        )
                    },
                )
            }
            Page::WinderustBehaviour
            | Page::LanguageAndAppearance
            | Page::ExperimentalFeatures
            | Page::About => {
                self.preferences
                    .view(self.page, settings.global(), ui.map(Message::Preferences))
            }
            Page::ActionLog => self.action_log.view(
                &self.status.action_log_entries,
                &self.candidates,
                ui.map(Message::ActionLog),
            ),
            Page::PowerPlanControl => col()
                .fill()
                .child(card(
                    "",
                    [setting_row(
                        t!("power_plan_control.pause_plugged").to_string(),
                        ui.toggle(
                            settings
                                .global()
                                .general
                                .pause_power_plan_switching_while_plugged_in,
                            Message::PausePowerPlans,
                        ),
                    )],
                ))
                .child(self.child_pages(ui)),
            Page::WinderustFeatures
            | Page::CpuControl
            | Page::PriorityControl
            | Page::SettingsHome
            | Page::AdvancedControls => self.child_pages(ui),
        }
    }
    fn child_pages(&self, ui: Ui<Message>) -> Element {
        let mut children = Vec::new();
        if self.page == Page::PowerPlanControl {
            children.push(label(t!("power_plan_control.automation").to_string()));
        }
        if let Some(pages) = self.page.child_pages() {
            for page in pages.iter().copied().filter(|page| *page != self.page) {
                if self.page == Page::PowerPlanControl && page == Page::AdvancedPowerPlanTuning {
                    children.push(label(t!("settings.advanced").to_string()));
                }
                let mut heading = row()
                    .child(navigation::glyph(
                        navigation::icon_path(page),
                        Role::TextMuted,
                    ))
                    .child(label(page.label()).max_lines(1).weight(1.0));
                if self.settings.global().general.show_feature_status_on_cards {
                    if let Some(enabled) = navigation::feature_page_enabled(&self.settings, page) {
                        heading = heading.child(Element::badge_intent(
                            t!(if enabled {
                                "common.enabled"
                            } else {
                                "common.disabled"
                            })
                            .to_string(),
                            if enabled {
                                windui::theme::Intent::Success
                            } else {
                                windui::theme::Intent::Neutral
                            },
                        ));
                    }
                }
                heading = heading.child(navigation::glyph(
                    "icons/chevron-right.svg",
                    Role::TextMuted,
                ));
                let go = ui.clone();
                children.push(
                    card("", [heading])
                        .height(58)
                        .clickable()
                        .on_click(move |ctx| {
                            widgets::flush_bindings(ctx);
                            go.emit(Message::Page(page));
                        }),
                );
            }
        }
        widgets::page(children)
    }
    fn side_panel(&self, ui: Ui<Message>) -> Option<Element> {
        let panel = match self.page {
            Page::ProcessList => self.processes.side_panel(ui.map(Message::Processes)),
            Page::ActionLog => self.action_log.side_panel(
                self.settings.global().advanced.action_log_mode,
                !self.status.action_log_entries.is_empty(),
                !self.status.action_log_summaries.is_empty(),
                ui.map(Message::ActionLog),
            ),
            Page::AdaptiveEngine => {
                self.adaptive
                    .side_panel(&self.settings, &self.status, ui.map(Message::Adaptive))
            }
            Page::AdvancedPowerPlanTuning => self.power_tuning.side_panel(
                &self.settings.advanced_power_plan_tuning_presets,
                ui.map(Message::PowerTuning),
            ),
            Page::CpuSetsSoft => self.soft_allocation.side_panel(
                &self.settings,
                cpu_allocation::Kind::Soft,
                &self.status,
                ui.map(|message| Message::Allocation(cpu_allocation::Kind::Soft, message)),
            ),
            Page::ProcessorAffinityHard => self.hard_allocation.side_panel(
                &self.settings,
                cpu_allocation::Kind::Hard,
                &self.status,
                ui.map(|message| Message::Allocation(cpu_allocation::Kind::Hard, message)),
            ),
            page => status_rail::view(
                page,
                &self.settings,
                &self.status,
                &self.power_plans,
                ui.map(Message::Status),
            )?,
        };
        Some(panel)
    }
    fn modal(&self, ui: Ui<Message>) -> Option<(Element, Message)> {
        Some(match self.page {
            Page::CpuSetsSoft => (
                self.soft_allocation.modal(
                    &self.settings,
                    ui.map(|message| Message::Allocation(cpu_allocation::Kind::Soft, message)),
                )?,
                Message::Allocation(
                    cpu_allocation::Kind::Soft,
                    cpu_allocation::Message::ClosePreset,
                ),
            ),
            Page::ProcessorAffinityHard => (
                self.hard_allocation.modal(
                    &self.settings,
                    ui.map(|message| Message::Allocation(cpu_allocation::Kind::Hard, message)),
                )?,
                Message::Allocation(
                    cpu_allocation::Kind::Hard,
                    cpu_allocation::Message::ClosePreset,
                ),
            ),
            Page::CpuLimiter => (
                self.cpu_limiter.modal(
                    &self.candidates,
                    &self.status.feature_status.cpu_limiter,
                    ui.map(Message::CpuLimiter),
                )?,
                Message::CpuLimiter(cpu_limiter::Message::CancelRule),
            ),
            Page::AdvancedPowerPlanTuning => (
                self.power_tuning.preset_modal(
                    &self.settings.advanced_power_plan_tuning_presets,
                    ui.map(Message::PowerTuning),
                )?,
                Message::PowerTuning(advanced_power_plan_tuning::Message::ClosePreset),
            ),
            Page::AdaptiveEngine if self.adaptive.has_pending_editor() => (
                self.adaptive.preset_modal(
                    &self.settings,
                    &self.candidates,
                    ui.map(Message::Adaptive),
                ),
                Message::Adaptive(adaptive_engine::Message::Cancel),
            ),
            Page::ByTime => (
                self.time_rules.modal(
                    power_rules::Kind::Time,
                    &self.power_plans,
                    ui.map(|message| Message::PowerRules(power_rules::Kind::Time, message)),
                )?,
                Message::PowerRules(power_rules::Kind::Time, power_rules::Message::Cancel),
            ),
            Page::ByCpuLoad => (
                self.cpu_rules.modal(
                    power_rules::Kind::CpuLoad,
                    &self.power_plans,
                    ui.map(|message| Message::PowerRules(power_rules::Kind::CpuLoad, message)),
                )?,
                Message::PowerRules(power_rules::Kind::CpuLoad, power_rules::Message::Cancel),
            ),
            _ => return None,
        })
    }
}
impl WinderustApp {
    fn navigation_row(&self, page: Page, child: bool, ui: &Ui<Message>) -> Element {
        let collapsed = self.settings.global().general.navigation_collapsed;
        let expandable = !child && page.child_pages().is_some_and(|pages| pages.len() > 1);
        navigation::row(
            navigation::Row {
                page,
                child,
                collapsed,
                selected: self.page == page || (!child && self.page.section_landing_page() == page),
                expanded: expandable.then_some(self.expanded_section == Some(page)),
                enabled: child
                    .then(|| navigation::feature_page_enabled(&self.settings, page))
                    .flatten(),
                count: self
                    .settings
                    .global()
                    .general
                    .show_enabled_feature_counts_in_sidebar
                    .then(|| navigation::section_enabled_feature_count(&self.settings, page))
                    .flatten(),
            },
            ui.clone(),
            if expandable && !collapsed {
                Message::ToggleSection(page)
            } else {
                Message::Page(page)
            },
        )
    }
    fn navigation_rows(&self, ui: Ui<Message>, pinned: bool) -> Element {
        let advanced = self.settings.global().advanced.show_advanced_controls;
        if !pinned
            && !self.settings.global().general.navigation_collapsed
            && !self.navigation_search.trim().is_empty()
        {
            return Element::col().width_match().spacing(4).children(
                navigation::dashboard_search_pages(&self.navigation_search, advanced)
                    .into_iter()
                    .map(|page| self.navigation_row(page, true, &ui)),
            );
        }
        let mut rows = Vec::new();
        let collapsed = self.settings.global().general.navigation_collapsed;
        for section in Page::sections().iter().filter(|section| {
            (advanced || section.landing_page != Page::AdvancedControls)
                && matches!(
                    section.landing_page,
                    Page::ActionLog | Page::SettingsHome | Page::About
                ) == pinned
        }) {
            let page = section.landing_page;
            let expanded =
                !collapsed && section.pages.len() > 1 && self.expanded_section == Some(page);
            rows.push(navigation::section(
                self.navigation_row(page, false, &ui),
                if expanded {
                    section
                        .pages
                        .iter()
                        .map(|page| self.navigation_row(*page, true, &ui))
                        .collect()
                } else {
                    Vec::new()
                },
                expanded,
            ));
        }
        Element::col().width_match().spacing(2).children(rows)
    }
    fn header(&self, ui: Ui<Message>) -> Element {
        let collapsed = self.settings.global().general.navigation_collapsed;
        let has_side_panel = self.has_side_panel();
        let status_collapsed = self.status_collapsed;
        let is_narrow = move |width| {
            let compact = !collapsed
                && width < design::NAVIGATION_WIDTH as i32 + design::SIDE_PANEL_WIDTH as i32 + 480;
            let navigation = if collapsed {
                design::SIDEBAR_COLLAPSED_WIDTH
            } else {
                design::NAVIGATION_WIDTH
            } as i32;
            let panel_width = if !has_side_panel {
                0
            } else if status_collapsed || compact {
                design::SIDEBAR_COLLAPSED_WIDTH as i32
            } else {
                design::SIDE_PANEL_WIDTH as i32
            };
            width - navigation - panel_width - 48 < 600
        };
        let narrow = signal(is_narrow(self.view.width.get()));
        let weak = self.weak_self.clone();
        widgets::observe(
            Element::host_signal(narrow.map(|narrow| vec![*narrow]), move |narrow| {
                let Some(state) = weak.upgrade() else {
                    return Element::col();
                };
                let app = state.borrow();
                app.header_content(ui.clone(), narrow)
            })
            .width_match(),
            self.view.width,
            move |width| {
                let next = is_narrow(width);
                if narrow.get() != next {
                    narrow.set(next);
                }
            },
        )
    }
    fn header_content(&self, ui: Ui<Message>, narrow: bool) -> Element {
        let back = ui.clone();
        let forward = ui.clone();
        let history = Element::row()
            .spacing(4)
            .cross(Align::Center)
            .child(
                Element::icon_button("\u{2039}")
                    .size(28, 28)
                    .tooltip(t!("nav.back").to_string())
                    .on_click(move |ctx| {
                        widgets::flush_bindings(ctx);
                        back.emit(Message::NavigateHistory(false));
                    }),
            )
            .child(
                Element::icon_button("\u{203a}")
                    .size(28, 28)
                    .tooltip(t!("nav.forward").to_string())
                    .on_click(move |ctx| {
                        widgets::flush_bindings(ctx);
                        forward.emit(Message::NavigateHistory(true));
                    }),
            );
        let title = label(self.page.label())
            .font_size(design::typography::TITLE as f32)
            .font_weight(600)
            .max_lines(2)
            .width_match()
            .weight(1.0)
            .tooltip(
                self.breadcrumb
                    .iter()
                    .map(|page| page.label())
                    .collect::<Vec<_>>()
                    .join(" / "),
            );
        let mut header = Element::col()
            .width_match()
            .spacing(8)
            .padding_xy(24, 8)
            .child(
                Element::row()
                    .width_match()
                    .spacing(10)
                    .cross(Align::Center)
                    .child(history)
                    .child(title),
            );
        let info = navigation::page_feature_info(self.page);
        if self.page.supports_power_source_profiles() || !info.is_empty() || self.has_side_panel() {
            let mut profiles = None;
            let mut actions = Element::flow().spacing(8).run_spacing(8);
            if self.page.supports_power_source_profiles() {
                let plugged_in = self.live_power_source;
                let choices: Vec<_> = [
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
                ]
                .into_iter()
                .map(|(profile, key, live)| {
                    widgets::Choice(
                        profile,
                        format!("{}{}", t!(key), if live { " \u{2022}" } else { "" }),
                    )
                })
                .collect();
                let selected = choices
                    .iter()
                    .find(|choice| choice.0 == self.power_source)
                    .unwrap_or_else(|| unreachable!("Both power profiles must be represented"))
                    .clone();
                profiles =
                    Some(ui.segmented(choices, selected, |choice| Message::PowerSource(choice.0)));
            }
            if !info.is_empty() {
                let toggle = ui.clone();
                let color = if self.feature_info_expanded {
                    Role::Accent
                } else {
                    Role::TextMuted
                };
                actions = actions.child(
                    Element::row()
                        .height(32)
                        .spacing(6)
                        .padding_xy(8, 0)
                        .cross(Align::Center)
                        .corner(design::CONTROL_RADIUS)
                        .tooltip(t!("common.feature_info").to_string())
                        .child(navigation::glyph("icons/info.svg", color))
                        .child(
                            label(t!("common.feature_info").to_string())
                                .font_size(design::typography::SECONDARY as f32)
                                .fg_role(color),
                        )
                        .clickable()
                        .on_click(move |ctx| {
                            widgets::flush_bindings(ctx);
                            toggle.emit(Message::ToggleFeatureInfo);
                        }),
                );
            }
            match self.page {
                Page::MemoryTrim => {
                    actions = actions.child(
                        ui.button(
                            t!("memory_trim.trim_now").to_string(),
                            Message::Trim(memory_trim::Message::TrimNow),
                        )
                        .small()
                        .enabled(self.settings.memory_trim.enabled),
                    );
                }
                Page::ByTime | Page::ByCpuLoad => {
                    let kind = if self.page == Page::ByTime {
                        power_rules::Kind::Time
                    } else {
                        power_rules::Kind::CpuLoad
                    };
                    actions = actions.child(
                        ui.button(
                            t!("common.create").to_string(),
                            Message::PowerRules(kind, power_rules::Message::Add),
                        )
                        .small(),
                    );
                }
                _ => {}
            }
            if self.has_side_panel() {
                let title = match self.page {
                    Page::ProcessList | Page::ActionLog => t!("nav.settings").to_string(),
                    Page::AdvancedPowerPlanTuning => t!("adaptive_engine.presets").to_string(),
                    Page::AdaptiveEngine | Page::CpuSetsSoft | Page::ProcessorAffinityHard => {
                        format!(
                            "{} / {}",
                            t!("common.status"),
                            t!("adaptive_engine.presets")
                        )
                    }
                    _ => t!("common.status").to_string(),
                };
                actions = actions.child(
                    ui.button(title.clone(), Message::ToggleStatus)
                        .small()
                        .tooltip(title),
                );
            }
            header = header.child(header_toolbar(profiles, actions, narrow));
        }
        header
    }
    fn footer(&self, ui: Ui<Message>) -> Element {
        let text = if self.exiting {
            t!("exit_prompt.exiting").to_string()
        } else if self.pending_changes() {
            t!("unsaved.message").to_string()
        } else {
            format!("Winderust {}", env!("CARGO_PKG_VERSION"))
        };
        let exit = ui.clone();
        row()
            .padding_xy(16, 10)
            .bg_role(Role::SurfaceAlt)
            .border_role(Role::Border, 1)
            .border_edges(windui::style::Edges::TOP)
            .child(
                Element::row()
                    .height(32)
                    .padding_xy(4, 0)
                    .cross(Align::Center)
                    .corner(design::CONTROL_RADIUS)
                    .child(label(t!("tray.exit").to_string()).fg_role(Role::TextMuted))
                    .clickable()
                    .on_click(move |ctx| {
                        widgets::flush_bindings(ctx);
                        exit.emit(Message::Close);
                    }),
            )
            .child(
                label(text)
                    .font_size(12.0)
                    .fg_role(if self.pending_changes() {
                        Role::Text
                    } else {
                        Role::TextMuted
                    })
                    .max_lines(2)
                    .weight(1.0),
            )
            .child(
                ui.button(t!("common.discard").to_string(), Message::Cancel)
                    .enabled(self.pending_changes() && !self.exiting),
            )
            .child(
                ui.primary_button(t!("common.save").to_string(), Message::Save)
                    .enabled(self.pending_changes() && !self.exiting),
            )
    }
    fn overlay(&self, ui: Ui<Message>) -> Option<Element> {
        if self.closing || !self.error_message.is_empty() {
            let key = if self.exiting {
                "exit_prompt.exiting"
            } else if self.closing {
                "exit_prompt.title"
            } else {
                "common.error"
            };
            let mut body = col()
                .width(540)
                .max_width(640)
                .bg_role(Role::Surface)
                .corner(design::CARD_RADIUS)
                .margin(20)
                .padding(20)
                .child(heading(t!(key).to_string()));
            if self.closing && !self.exiting {
                let key = if self.pending_changes() {
                    "exit_prompt.unsaved"
                } else {
                    "exit_prompt.message"
                };
                body = body.child(label(t!(key).to_string()));
                let mut actions =
                    row().child(ui.button(t!("common.cancel").to_string(), Message::Stay));
                if self.pending_changes() {
                    actions = actions.child(ui.primary_button(
                        t!("exit_prompt.save_and_exit").to_string(),
                        Message::Save,
                    ));
                }
                actions = actions.child(
                    ui.button(
                        t!(if self.pending_changes() {
                            "exit_prompt.without_saving"
                        } else {
                            "tray.exit"
                        })
                        .to_string(),
                        Message::DiscardAndClose,
                    ),
                );
                body = body.child(actions);
            }
            if !self.error_message.is_empty() {
                body = body.child(widgets::error(self.error_message.clone()));
            }
            if !self.closing {
                body = body.child(ui.button(t!("common.done").to_string(), Message::DismissError));
            }
            if self.exiting {
                if self.shutdown_failed {
                    body = body
                        .child(ui.button(t!("common.retry").to_string(), Message::RetryShutdown));
                }
                return Some(Element::dialog(signal(true), body));
            }
            return Some(widgets::dialog(
                signal(true),
                body,
                ui,
                if self.closing {
                    Message::Stay
                } else {
                    Message::DismissError
                },
            ));
        }
        if self.preferences.show_update {
            let mut body = col()
                .width(500)
                .bg_role(Role::Surface)
                .corner(design::CARD_RADIUS)
                .padding(20)
                .child(heading(t!("about.updates").to_string()))
                .child(label(self.preferences.latest.clone().unwrap_or_default()));
            if let Some(url) = &self.preferences.download {
                body = body.child(ui.button(
                    t!("about.download_update").to_string(),
                    Message::Preferences(settings_pages::Message::Open(url.clone())),
                ));
            }
            body = body.child(ui.button(
                t!("common.cancel").to_string(),
                Message::Preferences(settings_pages::Message::DismissUpdate),
            ));
            return Some(widgets::dialog(
                signal(true),
                body,
                ui,
                Message::Preferences(settings_pages::Message::DismissUpdate),
            ));
        }
        if self.page == Page::ProcessList {
            if let Some(modal) = self.processes.modal(
                &self.settings,
                &self.status,
                if self.power_plans_loading {
                    process_list::PlanCatalog::Loading
                } else if self.power_plans_loaded {
                    process_list::PlanCatalog::Loaded(&self.power_plans)
                } else {
                    process_list::PlanCatalog::Unavailable
                },
                ui.map(Message::Processes),
            ) {
                return Some(modal.fill().weight(1.0));
            }
        }
        if let Some((modal, cancel)) = self.modal(ui.clone()) {
            return Some(widgets::dialog(signal(true), modal, ui, cancel));
        }
        None
    }
    fn side_panel_shows_live_status(&self) -> bool {
        match self.page {
            Page::ProcessList | Page::ActionLog | Page::AdvancedPowerPlanTuning => false,
            Page::AdaptiveEngine => self.adaptive.shows_live_status(),
            Page::CpuSetsSoft => self.soft_allocation.shows_live_status(),
            Page::ProcessorAffinityHard => self.hard_allocation.shows_live_status(),
            _ => self.has_side_panel(),
        }
    }
    fn has_side_panel(&self) -> bool {
        self.page.supports_power_source_profiles()
            || matches!(
                self.page,
                Page::ProcessList | Page::ActionLog | Page::AdvancedPowerPlanTuning
            )
    }
    fn shell(&self, ui: Ui<Message>) -> Element {
        let weak = self.weak_self.clone();
        let collapsed = self.settings.global().general.navigation_collapsed;
        let mut navigation = Element::col()
            .fill()
            .spacing(4)
            .padding_xy(6, 8)
            .bg_role(Role::SurfaceAlt)
            .border_role(Role::Border, 1)
            .border_edges(windui::style::Edges::RIGHT);
        if collapsed {
            let mut search_icon = windui::ui::ImageContent::from_svg_bytes(
                super::assets::svg("icons/search.svg")
                    .unwrap_or_else(|| unreachable!("Navigation icons must be embedded")),
                None,
            );
            search_icon.set_tint_role(Role::TextMuted);
            let expand = ui.clone();
            navigation = navigation.child(
                Element::icon_button_content(search_icon)
                    .size(40, 36)
                    .tooltip(t!("home.search_placeholder").to_string())
                    .on_click(move |ctx| {
                        widgets::flush_bindings(ctx);
                        expand.emit(Message::ToggleNavigation);
                    }),
            );
        } else {
            navigation = navigation.child(ui.input(
                self.navigation_search.clone(),
                t!("home.search_placeholder").to_string(),
                Message::NavigationSearch,
            ));
        }
        let toggle = ui.clone();
        let toggle_title = t!(if collapsed {
            "nav.expand_navigation"
        } else {
            "nav.collapse_navigation"
        })
        .to_string();
        let mut toggle_navigation = Element::row()
            .width_match()
            .height(32)
            .cross(Align::Center)
            .spacing(8)
            .padding_xy(8, 0)
            .corner(design::CONTROL_RADIUS)
            .tooltip(toggle_title.clone())
            .child(navigation::glyph(
                if collapsed {
                    "icons/panel-left-open.svg"
                } else {
                    "icons/panel-left-close.svg"
                },
                Role::TextMuted,
            ));
        if !collapsed {
            toggle_navigation = toggle_navigation.child(
                label(toggle_title)
                    .font_size(design::typography::SECONDARY as f32)
                    .fg_role(Role::TextMuted),
            );
        }
        let navigation = navigation
            .child(
                super::scrolling::scrollable(fragment(
                    weak.clone(),
                    self.view.navigation,
                    |app, ui| {
                        // Native scrollbars overlay content; reserve space beside expanded row actions.
                        app.navigation_rows(ui, false)
                            .padding_edges(windui::geometry::Insets::new(
                                0,
                                0,
                                if app.settings.global().general.navigation_collapsed {
                                    0
                                } else {
                                    12
                                },
                                0,
                            ))
                    },
                ))
                .weight(1.0),
            )
            .child(Element::divider())
            .child(
                Element::scroll()
                    .width_match()
                    .max_height(300)
                    .child(fragment(weak.clone(), self.view.navigation, |app, ui| {
                        app.navigation_rows(ui, true)
                    })),
            )
            .child(toggle_navigation.clickable().on_click(move |ctx| {
                widgets::flush_bindings(ctx);
                toggle.emit(Message::ToggleNavigation);
            }))
            .width(if collapsed {
                design::SIDEBAR_COLLAPSED_WIDTH as i32
            } else {
                design::NAVIGATION_WIDTH as i32
            });
        let center = col()
            .fill()
            .weight(1.0)
            .spacing(0)
            .child(fragment(weak.clone(), self.view.header, |app, ui| {
                app.header(ui)
            }))
            .child(
                fragment(weak.clone(), self.view.page, |app, ui| {
                    let offset = app
                        .page_offsets
                        .get(&app.page)
                        .unwrap_or_else(|| unreachable!("Every page must have a scroll state"));
                    let mut page = app.page_view(ui);
                    if app.feature_info_expanded {
                        let info = navigation::page_feature_info(app.page);
                        if !info.is_empty() {
                            page = feature_info_page(page, info);
                        }
                    }
                    super::scrolling::remember(page, Rc::clone(offset)).weight(1.0)
                })
                .fill()
                .weight(1.0)
                .enabled_signal(self.view.editable),
            );
        let mut body = row()
            .fill()
            .weight(1.0)
            .spacing(0)
            .child(navigation)
            .child(center);
        if self.has_side_panel() {
            let compact = self.view.compact;
            let expanded = !self.status_collapsed;
            let panel = Element::col()
                .height_match()
                .width(if expanded {
                    design::SIDE_PANEL_WIDTH as i32
                } else {
                    design::SIDEBAR_COLLAPSED_WIDTH as i32
                })
                .spacing(8)
                .padding_xy(6, 8)
                .bg_role(Role::Bg)
                .border_role(Role::Border, 1)
                .border_edges(windui::style::Edges::LEFT)
                .child(if expanded {
                    fragment(weak.clone(), self.view.side, |app, ui| {
                        app.side_panel(ui).unwrap_or_else(Element::col).weight(1.0)
                    })
                    .fill()
                    .weight(1.0)
                } else {
                    Element::flex_spacer()
                })
                .child(Element::divider())
                .child(side_panel_toggle(ui.clone(), !expanded));
            body = body_with_side_panel(body, panel, move || compact.get() && !collapsed, expanded);
        }
        Element::col()
            .fill()
            .weight(1.0)
            .spacing(0)
            .child(body)
            .child(fragment(weak, self.view.footer, |app, ui| app.footer(ui)))
    }
}
fn side_panel_toggle(ui: Ui<Message>, collapsed: bool) -> Element {
    let title = t!(if collapsed {
        "nav.expand_side_panel"
    } else {
        "nav.collapse_side_panel"
    })
    .to_string();
    let mut toggle = Element::row()
        .width_match()
        .height(32)
        .cross(Align::Center)
        .spacing(8)
        .padding_xy(8, 0)
        .corner(design::CONTROL_RADIUS)
        .tooltip(title.clone())
        .child(navigation::glyph(
            if collapsed {
                "icons/panel-right-open.svg"
            } else {
                "icons/panel-right-close.svg"
            },
            Role::TextMuted,
        ));
    if !collapsed {
        toggle = toggle.child(
            label(title)
                .font_size(design::typography::SECONDARY as f32)
                .fg_role(Role::TextMuted),
        );
    }
    toggle.clickable().on_click(move |ctx| {
        widgets::flush_bindings(ctx);
        ui.emit(Message::ToggleStatus);
    })
}
fn body_with_side_panel(
    body: Element,
    panel: Element,
    compact: impl Fn() -> bool + Copy + 'static,
    expanded: bool,
) -> Element {
    // A narrow drawer overlays the same editor instead of squeezing its controls.
    Element::stack()
        .fill()
        .weight(1.0)
        .child(
            body.child(Element::col().width(design::SIDEBAR_COLLAPSED_WIDTH as i32))
                .child(
                    Element::col()
                        .width((design::SIDE_PANEL_WIDTH - design::SIDEBAR_COLLAPSED_WIDTH) as i32)
                        .visible_when(move || expanded && !compact()),
                ),
        )
        .child(panel.align_xy(Align::End, Align::Start))
}
fn feature_info_page(page: Element, info: String) -> Element {
    // Native unbounded scroll measurement expands the editor to its content height.
    super::scrolling::scrollable(
        Element::col().width_match().children([
            Element::col()
                .width_match()
                .cross(Align::Center)
                .padding_xy(24, 16)
                .child(
                    card(
                        t!("common.feature_info").to_string(),
                        [label(info).width_match()],
                    )
                    .max_width(design::CONTENT_WIDTH as i32),
                ),
            page,
        ]),
    )
}

fn header_toolbar(profiles: Option<Element>, actions: Element, narrow: bool) -> Element {
    let actions = Element::stack()
        .width_match()
        .child(actions.align_xy(Align::End, Align::Start));
    if narrow {
        Element::col()
            .width_match()
            .spacing(8)
            .children(profiles)
            .child(actions)
    } else {
        Element::row()
            .width_match()
            .spacing(8)
            .cross(Align::Start)
            .children(profiles)
            .child(actions.weight(1.0))
    }
}

fn fragment(
    weak: Weak<RefCell<WinderustApp>>,
    revision: Signal<Vec<u64>>,
    build: impl Fn(&WinderustApp, Ui<Message>) -> Element + 'static,
) -> Element {
    Element::host_signal(revision, move |_| {
        let Some(state) = weak.upgrade() else {
            return Element::col();
        };
        let app = state.borrow();
        let sender = app.sender();
        build(
            &app,
            Ui::new(move |message, refresh| send(&sender, message, refresh))
                .with_plan_catalog(app.view.plan_catalog)
                .with_candidate_catalog(app.view.candidate_catalog),
        )
    })
    .width_match()
}
fn main_window(state: &Rc<RefCell<WinderustApp>>) -> WindowRequest {
    let app = state.borrow();
    window_request(Rc::downgrade(state), app.view, app.sender())
}
fn overlay_host(
    revision: Signal<Vec<u64>>,
    build: impl Fn() -> Option<Element> + 'static,
) -> Element {
    let visible = signal(false);
    Element::host_signal(revision, move |_| {
        let content = build();
        if visible.get() != content.is_some() {
            visible.set(content.is_some());
        }
        content.unwrap_or_else(Element::col).fill().weight(1.0)
    })
    .fill()
    // A native dynamic host intercepts pointers even when its child is hidden.
    .visible_signal(visible)
}
fn window_request(
    weak: Weak<RefCell<WinderustApp>>,
    view: ViewState,
    sender: Sender<Intent>,
) -> WindowRequest {
    let close = sender.clone();
    let tick = sender.clone();
    #[cfg(feature = "render-smoke")]
    let pointer = weak.clone();
    let keys = sender;
    let mut window = Window::new("Winderust", 1120, 760)
        .single("main")
        .min_size(900, 620)
        .centered(true)
        .on_interval(Duration::from_millis(250), move |ctx| {
            #[cfg(feature = "render-smoke")]
            smoke::check_pointer_input(&pointer, ctx);
            #[cfg(feature = "render-smoke")]
            smoke::resource_window_tick(&pointer, ctx);
            #[cfg(not(feature = "render-smoke"))]
            let _ = ctx;
            send(&tick, Message::Tick, false);
        })
        .on_close_request(move |ctx| {
            widgets::flush_bindings(ctx);
            if view.editable.get()
                && view.can_hide.get()
                && (view.hide_to_tray.get() || view.startup_hide.get())
                && !view.confirming.get()
            {
                view.startup_hide.set(false);
                true
            } else {
                send(&close, Message::Close, true);
                false
            }
        })
        .on_shortcut(move |ctx, event| {
            if let Some(action) = shortcut_action(event, ctx.focus_accepts_text()) {
                view.shortcut.update(|value| {
                    value.0 = value.0.wrapping_add(1);
                    value.1 = action;
                });
                true
            } else {
                false
            }
        })
        .content(move || {
            let shortcut = keys.clone();
            let overlay = weak.clone();
            Element::stack()
                .fill()
                .bg_role(Role::Bg)
                .child(super::viewport::bind(view.compact, view.width))
                .child(
                    fragment(weak.clone(), view.layout, |app, ui| {
                        app.shell(ui).weight(1.0)
                    })
                    .fill(),
                )
                .child(overlay_host(view.overlay, move || {
                    let state = overlay.upgrade()?;
                    let app = state.borrow();
                    let sender = app.sender();
                    app.overlay(
                        Ui::new(move |message, refresh| send(&sender, message, refresh))
                            .with_plan_catalog(app.view.plan_catalog)
                            .with_candidate_catalog(app.view.candidate_catalog),
                    )
                }))
                .child(
                    widgets::observe_action(
                        Element::leaf().size(0, 0),
                        view.shortcut,
                        move |(_, action)| {
                            send(
                                &shortcut,
                                match action {
                                    ShortcutAction::Save => Message::Save,
                                    ShortcutAction::History(forward) => {
                                        Message::NavigateHistory(forward)
                                    }
                                    ShortcutAction::DeleteFocused => Message::DeleteFocused,
                                },
                                true,
                            );
                        },
                    )
                    .size(0, 0),
                )
        });
    window.icon = app_icon().map(Into::into);
    window
}
#[cfg(test)]
#[path = "app/pointer_tests.rs"]
mod pointer_tests;
fn shortcut_action(event: windui::event::KeyEvent, accepts_text: bool) -> Option<ShortcutAction> {
    use windui::event::Key;
    if !event.pressed {
        return None;
    }
    match event.key {
        Key::Left if event.alt => Some(ShortcutAction::History(false)),
        Key::Right if event.alt => Some(ShortcutAction::History(true)),
        Key::Delete
            if !accepts_text && !event.ctrl && !event.alt && !event.shift && !event.meta =>
        {
            Some(ShortcutAction::DeleteFocused)
        }
        // Win32 sends virtual-key letters as Other; Char is the text-input path.
        Key::Other(0x53) | Key::Char('s' | 'S') if event.ctrl => Some(ShortcutAction::Save),
        _ => None,
    }
}

/// Always finalize recovery after the runtime attempt; panics leave exit unauthorized.
fn complete_shutdown(
    restore: impl FnOnce() -> Result<(), String>,
    finish: impl FnOnce() -> Result<(), String>,
) -> Message {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let restored = catch_unwind(AssertUnwindSafe(restore));
    let finished = catch_unwind(AssertUnwindSafe(finish));
    match (restored, finished) {
        (Ok(restored), Ok(finished)) => {
            let errors: Vec<_> = restored.err().into_iter().chain(finished.err()).collect();
            Message::ShutdownFinished(if errors.is_empty() {
                Ok(())
            } else {
                Err(errors.join("\n"))
            })
        }
        _ => Message::ShutdownStartFailed(
            "Cleanup worker panicked; managed-state cleanup must be retried before exiting.".into(),
        ),
    }
}

fn send(sender: &Sender<Intent>, message: Message, refresh: bool) {
    if sender.send(Intent { message, refresh }).is_err() {
        diagnostics::error("Winderust UI command channel disconnected.");
    }
}
fn enqueue_wake(sender: &Sender<Intent>, pending: &AtomicBool) {
    if !pending.swap(true, Ordering::AcqRel)
        && sender
            .send(Intent {
                message: Message::Wake,
                refresh: false,
            })
            .is_err()
    {
        pending.store(false, Ordering::Release);
        diagnostics::error("Winderust UI wake channel disconnected.");
    }
}
fn app_icon() -> Option<WindowIcon> {
    match image::load_from_memory_with_format(
        include_bytes!("../../resources/winderust.ico"),
        image::ImageFormat::Ico,
    ) {
        Ok(image) => {
            let image = image
                .resize_exact(32, 32, image::imageops::FilterType::Lanczos3)
                .to_rgba8();
            WindowIcon::from_rgba(32, 32, image.into_raw())
        }
        Err(error) => {
            diagnostics::error(&format!("Winderust icon decode failed: {error}"));
            None
        }
    }
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;

    #[test]
    fn compact_side_panel_overlays_without_rebuilding_or_squeezing_the_editor() {
        use windui::{core::Tree, geometry::Size, text::NullTextEngine};

        for (navigation_width, navigation_collapsed) in [
            (design::NAVIGATION_WIDTH as i32, false),
            (design::SIDEBAR_COLLAPSED_WIDTH as i32, true),
        ] {
            for expanded in [true, false] {
                let mut scope = SignalScope::new();
                scope.collect(|| {
                    let compact = signal(false);
                    let text = signal("Unsaved editor text".to_owned());
                    let panel_width = if expanded {
                        design::SIDE_PANEL_WIDTH as i32
                    } else {
                        design::SIDEBAR_COLLAPSED_WIDTH as i32
                    };
                    let mut tree = Tree::new();
                    let root = body_with_side_panel(
                        Element::row()
                            .fill()
                            .child(Element::col().width(navigation_width))
                            .child(
                                Element::col()
                                    .fill()
                                    .weight(1.0)
                                    .child(Element::text_input(text, "Editor")),
                            ),
                        Element::col()
                            .height_match()
                            .width(panel_width)
                            .bg_role(Role::Surface),
                        move || compact.get() && !navigation_collapsed,
                        expanded,
                    )
                    .build(&mut tree);
                    tree.root = Some(root);
                    let body = tree.get(root).unwrap().children[0];
                    let editor = tree.get(body).unwrap().children[1];
                    let panel = tree.get(root).unwrap().children[1];
                    tree.layout_root(Size::new(1200, 600), &mut NullTextEngine);
                    let input = tree.focusable_order()[0];
                    tree.set_focused(Some(input), None);
                    for (width, is_compact) in [(900, true), (1200, false), (900, true)] {
                        compact.set(is_compact);
                        tree.layout_root(Size::new(width, 600), &mut NullTextEngine);
                        let reserved = if is_compact && !navigation_collapsed {
                            design::SIDEBAR_COLLAPSED_WIDTH as i32
                        } else {
                            panel_width
                        };
                        assert_eq!(
                            tree.abs_bounds(editor).w,
                            width - navigation_width - reserved
                        );
                        assert!(tree.get(panel).unwrap().effective_visible());
                        assert_eq!(tree.abs_bounds(panel).w, panel_width);
                        assert_eq!(tree.abs_bounds(panel).right(), width);
                        assert_eq!(tree.abs_bounds(panel).h, 600);
                        assert_eq!(tree.focused_node(), Some(input));
                        assert_eq!(tree.focusable_order()[0], input);
                        assert_eq!(text.get(), "Unsaved editor text");
                    }
                });
            }
        }
    }

    #[test]
    fn side_panel_toggle_uses_native_pointer_and_keyboard_actions_in_both_states() {
        use windui::{
            core::Tree,
            event::{Key, KeyEvent, MouseButton, PointerEvent, PointerKind},
            geometry::{Point, Size},
            text::NullTextEngine,
        };

        for collapsed in [false, true] {
            let calls = Rc::new(Cell::new(0));
            let received = Rc::clone(&calls);
            let mut tree = Tree::new();
            let root = side_panel_toggle(
                Ui::new(move |message, refresh| {
                    assert!(matches!(message, Message::ToggleStatus));
                    assert!(refresh);
                    received.set(received.get() + 1);
                }),
                collapsed,
            )
            .build(&mut tree);
            tree.root = Some(root);
            tree.layout_root(
                Size::new(
                    if collapsed {
                        design::SIDEBAR_COLLAPSED_WIDTH as i32 - 12
                    } else {
                        design::SIDE_PANEL_WIDTH as i32 - 12
                    },
                    32,
                ),
                &mut NullTextEngine,
            );
            assert_eq!(tree.focusable_order(), vec![root]);
            for child in tree.get(root).unwrap().children.clone() {
                let bounds = tree.abs_bounds(child);
                let point = Point::new(bounds.x + bounds.w / 2, bounds.y + bounds.h / 2);
                assert_eq!(tree.hit_test(point), Some(child));
                assert_eq!(
                    tree.node_tooltip(child),
                    Some(
                        t!(if collapsed {
                            "nav.expand_side_panel"
                        } else {
                            "nav.collapse_side_panel"
                        })
                        .to_string()
                    )
                );
                let (mut hover, mut capture) = (None, None);
                let before = calls.get();
                for kind in [PointerKind::Down, PointerKind::Up] {
                    tree.dispatch_pointer(
                        PointerEvent::single(kind, point, MouseButton::Left),
                        &mut hover,
                        &mut capture,
                    );
                }
                assert_eq!(calls.get(), before + 1);
            }
            tree.set_focused(Some(root), None);
            let before = calls.get();
            tree.dispatch_key(
                KeyEvent {
                    key: Key::Enter,
                    pressed: true,
                    ctrl: false,
                    shift: false,
                    alt: false,
                    meta: false,
                },
                Some(root),
            );
            assert_eq!(calls.get(), before + 1);
        }
    }

    #[test]
    fn header_actions_remain_reachable_when_profiles_stack_in_a_narrow_editor() {
        use windui::{core::Tree, geometry::Size, text::NullTextEngine};

        let mut scope = SignalScope::new();
        scope.collect(|| {
            let narrow = signal(false);
            let mut tree = Tree::new();
            let root = Element::host_signal(narrow.map(|value| vec![*value]), |narrow| {
                header_toolbar(
                    Some(Element::segmented(
                        vec!["Plugged In", "On Battery"],
                        signal(0),
                    )),
                    Element::flow()
                        .spacing(8)
                        .run_spacing(8)
                        .child(Element::button("Feature info").small())
                        .child(Element::button("Status / Presets").small()),
                    narrow,
                )
            })
            .width_match()
            .build(&mut tree);
            tree.root = Some(root);
            tree.layout_root(Size::new(900, 300), &mut NullTextEngine);
            let controls = tree.focusable_order();
            assert_eq!(controls.len(), 3);
            assert_eq!(
                tree.abs_bounds(controls[0]).y,
                tree.abs_bounds(controls[1]).y
            );

            narrow.set(true);
            tree.layout_root(Size::new(268, 300), &mut NullTextEngine);
            let controls = tree.focusable_order();
            assert_eq!(controls.len(), 3);
            let profiles = tree.abs_bounds(controls[0]);
            assert!(tree.abs_bounds(controls[1]).y >= profiles.bottom());
            for control in controls {
                let bounds = tree.abs_bounds(control);
                assert!(bounds.w > 0 && bounds.h > 0);
                assert!(bounds.x >= 0 && bounds.right() <= 268);
                assert!(bounds.y >= 0 && bounds.bottom() <= 300);
                if bounds.y != profiles.y {
                    assert!(bounds.y >= profiles.bottom());
                }
            }
        });
    }

    #[test]
    fn feature_help_and_editor_share_one_active_scroll_area() {
        use windui::{
            core::Tree,
            event::{MouseButton, PointerEvent, PointerKind},
            geometry::{Point, Size},
            spec::Axis,
            text::NullTextEngine,
        };

        let mut tree = Tree::new();
        let root = feature_info_page(
            widgets::page([Element::leaf().width_match().height(1_000)]),
            "Feature explanation".into(),
        )
        .build(&mut tree);
        tree.root = Some(root);
        tree.layout_root(Size::new(800, 400), &mut NullTextEngine);
        let body = tree.get(root).unwrap().children[0];
        let editor = tree.get(body).unwrap().children[1];
        assert!(tree.scroll_range(root, Axis::Vertical).unwrap().1 > 0);
        assert_eq!(tree.scroll_range(editor, Axis::Vertical).unwrap().1, 0);

        let bounds = tree.abs_bounds(editor);
        tree.dispatch_pointer(
            PointerEvent::single(
                PointerKind::Wheel(-120),
                Point::new(bounds.x + 20, bounds.y + 20),
                MouseButton::Left,
            ),
            &mut None,
            &mut None,
        );
        tree.layout_root(Size::new(800, 400), &mut NullTextEngine);
        assert!(tree.scroll_range(root, Axis::Vertical).unwrap().0 > 0);
        assert_eq!(tree.scroll_range(editor, Axis::Vertical).unwrap().0, 0);
    }

    #[test]
    fn native_virtual_key_shortcuts_save_without_taking_delete_from_text_inputs() {
        use windui::event::{Key, KeyEvent};
        let mut key = KeyEvent {
            key: Key::Other(0x53),
            pressed: true,
            ctrl: true,
            shift: false,
            alt: false,
            meta: false,
        };
        assert_eq!(shortcut_action(key, true), Some(ShortcutAction::Save));
        key.pressed = false;
        assert_eq!(shortcut_action(key, true), None);
        key = KeyEvent {
            key: Key::Delete,
            pressed: true,
            ctrl: false,
            ..key
        };
        assert_eq!(shortcut_action(key, true), None);
        assert_eq!(
            shortcut_action(key, false),
            Some(ShortcutAction::DeleteFocused)
        );
        key.shift = true;
        assert_eq!(shortcut_action(key, false), None);
    }

    #[test]
    fn cleanup_restores_then_finalizes_recovery_and_reports_both_failures() {
        let calls = RefCell::new(Vec::new());
        let result = complete_shutdown(
            || {
                calls.borrow_mut().push("restore");
                Err("restore failed".into())
            },
            || {
                calls.borrow_mut().push("finish");
                Err("recovery failed".into())
            },
        );
        assert_eq!(*calls.borrow(), ["restore", "finish"]);
        assert!(
            matches!(result, Message::ShutdownFinished(Err(error)) if error == "restore failed\nrecovery failed")
        );
    }

    #[test]
    fn cleanup_panics_still_attempt_recovery_and_never_authorize_exit() {
        let finalized = Cell::new(false);
        let result = complete_shutdown(
            || panic!("restoration panic"),
            || {
                finalized.set(true);
                Ok(())
            },
        );
        assert!(finalized.get());
        assert!(matches!(result, Message::ShutdownStartFailed(_)));
        assert!(matches!(
            complete_shutdown(|| Ok(()), || panic!("recovery panic")),
            Message::ShutdownStartFailed(_)
        ));
        assert!(matches!(
            complete_shutdown(|| Ok(()), || Ok(())),
            Message::ShutdownFinished(Ok(()))
        ));
    }
}
