use super::{
    action_log, adaptive_engine, advanced_power_plan_tuning, app_suspension, background_efficiency,
    by_activity, cpu_allocation, cpu_limiter, design, home, instance_balancing, instance_limits,
    interrupt_timing, keep_running, memory_trim, navigation, power_plan_control, power_rules,
    priority_control, process_list, process_power_plans, settings_pages, sleep_prevention,
    status_rail, tasks, timer_resolution, watchdog, widgets, win32_priority_separation,
    window_frame,
};
use crate::ui::scrolling::scrollable;
use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use widgets::{action_button, button, text_input};

use iced::widget::{column, container, row, text};
use iced::{Element, Fill, Subscription, Task, Theme};
use rust_i18n::t;

use crate::application::SettingsEditor;
use crate::automation::{RuntimeCommandError, RuntimeHandle, RuntimeStatusSnapshot};
use crate::config::{PowerSourceProfile, UpdateChannel};
use crate::control::power_plan::ManualPowerPlanError;
use crate::features::cpu_control::cpu_limiter::CpuLimiterSnapshot;
use crate::ui::Page;
use crate::SingleInstanceRestoreEvent;
use crate::{
    file_dialog::{choose_settings_file, FileDialogMode},
    tray,
};

// Iced supplies logical pixels; do not apply the monitor DPI a second time.
fn shell_widths(width: f32) -> (f32, f32, u16) {
    (
        design::NAVIGATION_RAIL_WIDTH + 1.0,
        design::SIDE_PANEL_WIDTH,
        if width < 1280.0 {
            design::space::MEDIUM
        } else {
            design::space::WIDE
        } as u16,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidePanelTab {
    Info,
    Status,
    Presets,
}
impl SidePanelTab {
    fn for_page(feature_page: Page) -> &'static [Self] {
        match feature_page {
            Page::AdaptiveEngine | Page::CpuSetsSoft | Page::ProcessorAffinityHard => {
                &[Self::Info, Self::Status, Self::Presets]
            }
            Page::AdvancedPowerPlanTuning => &[Self::Info, Self::Presets],
            Page::Win32PrioritySeparation => &[Self::Info],
            _ => &[Self::Info, Self::Status],
        }
    }
    fn label(self) -> String {
        t!(match self {
            Self::Status => "common.status",
            Self::Presets => "common.presets",
            Self::Info => "common.info",
        })
        .to_string()
    }
}

fn page_header<'a>(
    breadcrumb: Element<'a, Message>,
    profiles: Option<Element<'a, Message>>,
    panel_button: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut toolbar = row![profiles.unwrap_or_else(|| iced::widget::Space::new().into())]
        .push(iced::widget::Space::new().width(Fill))
        .height(design::BUTTON_HEIGHT)
        .align_y(iced::Center);
    if let Some(button) = panel_button {
        toolbar = toolbar.push(button);
    }
    column![
        container(breadcrumb).center_y(design::PAGE_BREADCRUMB_HEIGHT),
        toolbar,
    ]
    .spacing(design::space::SMALL)
    .width(Fill)
    .into()
}

fn page_canvas<'a>(
    heading: Element<'a, Message>,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    container(
        container(
            column![heading, content]
                .spacing(design::space::WIDE)
                .height(Fill),
        )
        .max_width(design::CONTENT_WIDTH)
        .width(Fill)
        .height(Fill),
    )
    .center_x(Fill)
    .height(Fill)
    .into()
}

fn side_panel_dock<'a>(
    workspace: Element<'a, Message>,
    panel: Option<Element<'a, Message>>,
    width: f32,
) -> Element<'a, Message> {
    let dock: Element<'a, Message> = if let Some(panel) = panel {
        let close = super::widgets::tooltip(
            widgets::icon_button("icons/x.svg").on_press(Message::CloseSidePanel),
            text(t!("common.close").to_string()),
            iced::widget::tooltip::Position::Left,
        );
        let panel_surface = container(
            column![
                row![
                    widgets::heading(t!("nav.side_panel").to_string(), design::typography::BODY),
                    iced::widget::Space::new().width(Fill),
                    close,
                ]
                .align_y(iced::Center),
                panel,
            ]
            .spacing(design::space::MEDIUM)
            .height(Fill),
        )
        .padding(design::space::MEDIUM as u16)
        .width(width)
        .height(Fill)
        .style(widgets::navigation_surface);
        row![
            iced::widget::rule::vertical(1),
            super::scrolling::repaint_group(panel_surface),
        ]
        .into()
    } else {
        iced::widget::Space::new().into()
    };
    row![container(workspace).width(Fill).height(Fill), dock]
        .width(Fill)
        .height(Fill)
        .into()
}

fn feature_info_sidebar<'a>(
    feature_page: Page,
    selected: SidePanelTab,
    build_panel: impl FnOnce() -> Option<Element<'a, Message>>,
) -> Option<Element<'a, Message>> {
    if navigation::page_feature_info_intro(feature_page).is_none() {
        return build_panel();
    }
    let info = || -> Element<'a, Message> {
        scrollable(
            container(
                text(navigation::page_feature_info(feature_page))
                    .size(design::typography::SECONDARY)
                    .width(Fill),
            )
            .width(Fill)
            .padding([0, design::space::MEDIUM as u16]),
        )
        .width(Fill)
        .height(Fill)
        .into()
    };
    let tabs = SidePanelTab::for_page(feature_page);
    let body = if tabs.len() == 1 {
        None
    } else if selected == SidePanelTab::Info {
        Some(info())
    } else {
        build_panel()
    };
    Some(if let Some(body) = body {
        let tabs = row(tabs.iter().map(|&tab| {
            widgets::panel_tab(tab.label(), tab == selected, Message::SidePanelTab(tab)).into()
        }));
        column![
            container(tabs).style(widgets::tab_bar),
            super::motion::wrap(body, true, super::motion::Effect::Content(selected as u64)),
        ]
        .spacing(design::space::MEDIUM)
        .height(Fill)
        .into()
    } else {
        column![
            widgets::panel_heading(t!("common.feature_info").to_string()),
            info(),
        ]
        .spacing(design::space::MEDIUM)
        .height(Fill)
        .into()
    })
}

#[cfg(feature = "render-smoke")]
#[path = "smoke.rs"]
pub(crate) mod smoke;

#[cfg(feature = "render-smoke")]
#[path = "window_lifecycle_smoke.rs"]
pub(crate) mod window_lifecycle_smoke;

pub(crate) fn run(
    settings: SettingsEditor,
    startup_warnings: Vec<String>,
    runtime: RuntimeHandle,
    restore_event: Option<SingleInstanceRestoreEvent>,
    recovery_client: Arc<Mutex<crate::crash_recovery::RecoveryClient>>,
) -> iced::Result {
    let startup = RefCell::new(Some((
        settings,
        startup_warnings,
        runtime,
        restore_event,
        recovery_client,
    )));
    iced::daemon(
        move || {
            let Some((settings, mut startup_warnings, runtime, restore_event, recovery_client)) =
                startup.borrow_mut().take()
            else {
                unreachable!("Iced must initialize the application exactly once");
            };
            rust_i18n::set_locale(settings.general.language.locale());
            let (wake, events) = ui_wake_channel();
            runtime.set_auto_exclusion_wake(wake.clone());
            tray::set_ui_wake(Some(wake));
            if let Some(event) = restore_event {
                event.listen();
            }
            let mut app = WinderustApp {
                appearance: settings_pages::theme(&settings.general),
                #[cfg(feature = "render-smoke")]
                smoke: smoke::Run::requested(),
                #[cfg(feature = "render-smoke")]
                lifecycle_smoke: window_lifecycle_smoke::Run::requested(),
                navigation_search: String::new(),
                navigation_search_open: false,
                navigation_search_scope: SearchScope(None),
                navigation_search_selected: 0,
                navigation: navigation::Flyout::default(),
                side_panel_open: false,
                side_panel_tab: SidePanelTab::Info,
                preferences: settings_pages::Editor::default(),
                home: home::Model::default(),
                action_log: action_log::Editor::default(),
                sampler: std::sync::Arc::new(std::sync::Mutex::new(home::Sampler::default())),
                sampled_at: std::time::Instant::now(),
                sampling: false,
                catalog_sampled_at: std::time::Instant::now(),
                suspension: app_suspension::Editor::default(),
                sleep_prevention: sleep_prevention::Editor::default(),
                trim: memory_trim::Editor::default(),
                manual_trim_pending: false,
                watchdog: watchdog::Editor::default(),
                keep_running: keep_running::Editor::default(),
                instance_limits: instance_limits::Editor::default(),
                instance_balancing: instance_balancing::Editor::default(),
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
                interrupt_timing: interrupt_timing::State::default(),
                candidates: Arc::default(),
                unavailable_candidates: Vec::new(),
                catalog_loading: false,
                settings,
                runtime: std::sync::Arc::new(runtime),
                status: RuntimeStatusSnapshot::default(),
                error_message: String::new(),
                page: Page::Home,
                navigation_history: navigation::History::default(),
                breadcrumb: vec![Page::Home],
                window: None,
                window_settings: iced::window::Settings {
                    size: iced::Size::new(1120.0, 760.0),
                    min_size: Some(iced::Size::new(900.0, 620.0)),
                    decorations: false,
                    exit_on_close_request: false,
                    ..Default::default()
                },
                maximized: false,
                auto_exclusion_generation: 0,
                auto_exclusion_retry: false,
                closing: false,
                exit_export_failed: false,
                hwnd: None,
                tray: None,
                tray_attempt: None,
                recovery_client,
                exiting: false,
                hidden: false,
                processes: process_list::ProcessList::default(),
                cpu_limiter: cpu_limiter::CpuLimiter::default(),
                power_source: PowerSourceProfile::PluggedIn,
                power_plans: Vec::new(),
                power_plans_loading: false,
                power_plans_loaded: false,
                power_plans_reload_pending: false,
                power_plan_control: power_plan_control::Editor::default(),
                focus_plans: process_power_plans::Editor::default(),
                running_app_plans: process_power_plans::Editor::default(),
                activity_inputs: by_activity::Inputs::default(),
            };
            startup_warnings.extend(app.sync_tray_warning());
            app.error_message = startup_warnings.join("\n");
            let window = if app.settings.persisted().general.start_minimized
                && app.tray.as_ref().is_some_and(|icon| icon.is_registered())
            {
                app.hidden = true;
                tray::set_hidden_to_tray(true);
                Task::none()
            } else {
                app.show_window()
            };
            let update = if app.settings.global().general.check_for_updates {
                app.update(Message::Preferences(settings_pages::Message::CheckStartup))
            } else {
                Task::none()
            };
            (
                app,
                Task::batch([window, update, Task::run(events, |_| Message::Wake)]),
            )
        },
        WinderustApp::update,
        WinderustApp::view,
    )
    .title("Winderust")
    .settings(iced::Settings {
        default_font: design::typography::DEFAULT_FONT,
        default_text_size: design::typography::BODY.into(),
        ..Default::default()
    })
    .theme(|app: &WinderustApp, _| app.appearance.clone())
    .subscription(|app: &WinderustApp| {
        Subscription::batch([
            ui_tick_interval(
                {
                    #[cfg(feature = "render-smoke")]
                    {
                        app.hidden && app.lifecycle_smoke.is_none()
                    }
                    #[cfg(not(feature = "render-smoke"))]
                    {
                        app.hidden
                    }
                },
                app.auto_exclusion_retry || app.tray_retry_pending(),
            )
            .map(|interval| iced::time::every(interval).map(|_| Message::Tick))
            .unwrap_or_else(Subscription::none),
            app.interrupt_timing
                .subscription()
                .map(|()| Message::PollInterruptTiming),
            app.navigation
                .timer()
                .filter(|_| !app.hidden)
                .map(|(due, delay)| {
                    iced::time::every(delay)
                        .with(due)
                        .map(|(_, now)| Message::Navigation(navigation::Event::Tick(now)))
                })
                .unwrap_or_else(Subscription::none),
            iced::window::close_requests().map(Message::WindowCloseRequested),
            iced::event::listen_raw(|event, _, _| match event {
                iced::Event::Window(iced::window::Event::Focused) => Some(Message::WindowFocused),
                iced::Event::Window(iced::window::Event::Unfocused)
                | iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::DismissPickers),
                _ => None,
            }),
            iced::event::listen_with(|event, _, window| match event {
                iced::Event::Window(iced::window::Event::Resized(size)) => {
                    Some(Message::WindowResized(window, size))
                }
                iced::Event::Window(iced::window::Event::Moved(position)) => {
                    Some(Message::WindowMoved(window, position))
                }
                _ => None,
            }),
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
                    iced::Event::Window(iced::window::Event::Unfocused) => {
                        Some(Message::Navigation(navigation::Event::Dismiss))
                    }
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
            iced::event::listen_with(|event, status, _| match event {
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key,
                    modifiers,
                    repeat,
                    ..
                }) => Some(Message::SearchKeyboard {
                    key,
                    modifiers,
                    repeat,
                    status,
                }),
                _ => None,
            }),
        ])
    })
    .run()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SearchScope(Option<Page>);
impl std::fmt::Display for SearchScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            &self
                .0
                .map(Page::label)
                .unwrap_or_else(|| t!("common.all").to_string()),
        )
    }
}

fn navigation_search_choices(query: &str, advanced: bool, scope: SearchScope) -> Vec<Page> {
    let mut pages = if query.trim().is_empty() {
        navigation::dashboard_sections_in_nav_order(advanced)
            .filter(|section| scope.0 == Some(section.landing_page))
            .flat_map(|section| {
                std::iter::once(section.landing_page).chain(section.pages.iter().copied())
            })
            .collect::<Vec<_>>()
    } else {
        navigation::dashboard_search_pages(query, advanced)
    };
    if let Some(section) = scope.0 {
        pages.retain(|page| page.section_landing_page() == section);
    }
    pages.dedup();
    pages
}

fn search_selection(selected: usize, count: usize, down: bool) -> usize {
    if count == 0 {
        0
    } else if down {
        selected.saturating_add(1).min(count - 1)
    } else {
        selected.saturating_sub(1).min(count - 1)
    }
}

fn navigation_search_panel<'a>(
    content: impl Into<Element<'a, Message>>,
) -> iced::widget::Container<'a, Message> {
    // Software-rendered quad shadows bypass partial repaint clipping.
    container(content)
        .width(Fill)
        .max_width(640)
        .clip(true)
        .style(widgets::surface)
}

fn centered_modal<'a>(
    background: Element<'a, Message>,
    modal: Element<'a, Message>,
) -> Element<'a, Message> {
    iced::widget::stack![
        super::modal::background(background),
        iced::widget::opaque(super::scrolling::modal_backdrop()),
        container(iced::widget::opaque(modal))
            .padding(16)
            .center_x(Fill)
            .center_y(Fill)
    ]
    .into()
}

fn navigation_search_overlay<'a>(
    background: Element<'a, Message>,
    search: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    iced::widget::stack![
        super::modal::background(background),
        iced::widget::opaque(
            iced::widget::mouse_area(
                container(iced::widget::Space::new())
                    .width(Fill)
                    .height(Fill)
                    .style(|_| container::Style {
                        background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.3).into()),
                        ..Default::default()
                    }),
            )
            .on_press(Message::CloseNavigationSearch)
        ),
        container(iced::widget::opaque(super::modal::dismiss_on_escape(
            search,
            Message::CloseNavigationSearch,
        )))
        .padding(iced::Padding {
            top: 120.0,
            right: 24.0,
            bottom: 24.0,
            left: 24.0
        })
        .center_x(Fill),
    ]
    .into()
}

struct WinderustApp {
    #[cfg(feature = "render-smoke")]
    smoke: Option<smoke::Run>,
    #[cfg(feature = "render-smoke")]
    lifecycle_smoke: Option<window_lifecycle_smoke::Run>,
    appearance: Theme,
    navigation_search: String,
    navigation_search_open: bool,
    navigation_search_scope: SearchScope,
    navigation_search_selected: usize,
    navigation: navigation::Flyout,
    side_panel_open: bool,
    side_panel_tab: SidePanelTab,
    preferences: settings_pages::Editor,
    home: home::Model,
    action_log: action_log::Editor,
    sampler: std::sync::Arc<std::sync::Mutex<home::Sampler>>,
    sampled_at: std::time::Instant,
    sampling: bool,
    catalog_sampled_at: std::time::Instant,
    suspension: app_suspension::Editor,
    sleep_prevention: sleep_prevention::Editor,
    trim: memory_trim::Editor,
    manual_trim_pending: bool,
    watchdog: watchdog::Editor,
    keep_running: keep_running::Editor,
    instance_limits: instance_limits::Editor,
    instance_balancing: instance_balancing::Editor,
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
    interrupt_timing: interrupt_timing::State,
    candidates: Arc<Vec<super::app_picker::Candidate>>,
    unavailable_candidates: Vec<String>,
    catalog_loading: bool,
    settings: SettingsEditor,
    runtime: std::sync::Arc<RuntimeHandle>,
    status: RuntimeStatusSnapshot,
    error_message: String,
    page: Page,
    navigation_history: navigation::History,
    breadcrumb: Vec<Page>,
    window: Option<iced::window::Id>,
    window_settings: iced::window::Settings,
    maximized: bool,
    auto_exclusion_generation: u64,
    auto_exclusion_retry: bool,
    closing: bool,
    exit_export_failed: bool,
    hwnd: Option<usize>,
    tray: Option<tray::TrayIcon>,
    tray_attempt: Option<((bool, bool), std::time::Instant)>,
    recovery_client: Arc<Mutex<crate::crash_recovery::RecoveryClient>>,
    exiting: bool,
    hidden: bool,
    processes: process_list::ProcessList,
    cpu_limiter: cpu_limiter::CpuLimiter,
    power_source: PowerSourceProfile,
    power_plans: Vec<crate::power::PowerPlan>,
    power_plans_loading: bool,
    power_plans_loaded: bool,
    power_plans_reload_pending: bool,
    power_plan_control: power_plan_control::Editor,
    focus_plans: process_power_plans::Editor,
    running_app_plans: process_power_plans::Editor,
    activity_inputs: by_activity::Inputs,
}

#[derive(Debug, Clone)]
enum Message {
    Wake,
    #[cfg(feature = "render-smoke")]
    SmokeScreenshot(iced::window::Screenshot),
    NavigationSearch(String),
    OpenNavigationSearch,
    CloseNavigationSearch,
    NavigationSearchScope(SearchScope),
    SearchKeyboard {
        key: iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
        repeat: bool,
        status: iced::event::Status,
    },
    ConfirmSearch,
    DismissError,
    NavigateHistory(bool),
    Navigation(navigation::Event),
    SidePanelTab(SidePanelTab),
    ToggleSidePanel,
    CloseSidePanel,
    Preferences(settings_pages::Message),
    DismissPickers,
    Status(status_rail::Message),
    Home(home::Message),
    Sample(Result<home::Sample, String>),
    ActionLog(action_log::Message),
    ExportLog(Option<PathBuf>),
    Suspension(app_suspension::Message),
    SleepPrevention(sleep_prevention::Message),
    Trim(memory_trim::Message),
    TrimFinished(Result<(), String>),
    Watchdog(watchdog::Message),
    KeepRunning(keep_running::Message),
    InstanceLimits(instance_limits::Message),
    InstanceBalancing(instance_balancing::Message),
    InstanceBalancingExecutableChosen(Arc<()>, Option<PathBuf>),
    Timer(timer_resolution::Message),
    PrioritySeparation(win32_priority_separation::Message),
    PowerTuning(advanced_power_plan_tuning::Message),
    CommandFinished(Result<(), String>),

    PowerRules(power_rules::Kind, power_rules::Message),
    Priority(priority_control::Kind, priority_control::Message),
    Efficiency(background_efficiency::Message),
    Allocation(cpu_allocation::Kind, cpu_allocation::Message),
    Adaptive(adaptive_engine::Message),
    PollInterruptTiming,
    Catalog(Result<Arc<Vec<super::app_picker::Candidate>>, String>),
    ExecutableChosen(Page, PowerSourceProfile, Option<PathBuf>),
    Page(Page),
    Tick,
    Window(iced::window::Id),
    WindowAction(window_frame::Event),
    WindowFocused,
    WindowResized(iced::window::Id, iced::Size),
    WindowMoved(iced::window::Id, iced::Point),
    NativeWindow(iced::window::Id, Option<usize>),
    WindowCloseRequested(iced::window::Id),
    WindowClose,
    Close,
    Save,
    Cancel,
    PausePowerPlans(bool),
    RestorePreviousPowerPlan(bool),
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
    PowerPlanControl(power_plan_control::Message),
    PowerPlanSwitchFinished(Result<(), RuntimeCommandError>),
    PowerPlans(Result<Vec<crate::power::PowerPlan>, String>),
}

impl Message {
    fn allowed_during_exit(&self) -> bool {
        matches!(
            self,
            Self::ShutdownFinished(_)
                | Self::WindowAction(
                    window_frame::Event::Drag
                        | window_frame::Event::Minimize
                        | window_frame::Event::ToggleMaximize
                        | window_frame::Event::Resize(_)
                )
                | Self::WindowResized(..)
                | Self::WindowMoved(..)
                | Self::Sample(_)
                | Self::Catalog(_)
                | Self::PowerPlans(_)
                | Self::Processes(process_list::Message::Loaded(_))
                | Self::Processes(process_list::Message::IconsLoaded(_))
                | Self::Preferences(settings_pages::Message::Checked(..))
        )
    }
}

impl WinderustApp {
    fn search_choices(&self) -> Vec<Page> {
        navigation_search_choices(
            &self.navigation_search,
            self.settings.global().advanced.show_advanced_controls,
            self.navigation_search_scope,
        )
    }

    fn scroll_search_selection(&self) -> Task<Message> {
        iced::widget::operation::scroll_to(
            "navigation-search-results",
            iced::widget::scrollable::AbsoluteOffset {
                x: None,
                y: Some((self.navigation_search_selected as f32 * 50.0 - 125.0).max(0.0)),
            },
        )
    }

    fn search_keyboard(
        &mut self,
        key: iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
        repeat: bool,
        status: iced::event::Status,
    ) -> Task<Message> {
        use iced::keyboard::{key::Named, Key};
        let review_open = self.power_tuning.review_origin().is_some();
        if self.hidden {
            return Task::none();
        }
        let tab = status == iced::event::Status::Ignored
            && key == Key::Named(Named::Tab)
            && (modifiers.is_empty() || modifiers == iced::keyboard::Modifiers::SHIFT);
        let wrap_focus = self.picker_open();
        let move_focus = || super::keyboard_focus::move_focus(!modifiers.shift(), wrap_focus);
        if !review_open
            && (self.closing
                || !self.error_message.is_empty()
                || self.preferences.replace_settings_open
                || self.preferences.show_update && self.process_power_plan_preview().is_none())
        {
            if tab {
                return move_focus();
            }
            if self.exiting
                || repeat
                || !modifiers.is_empty()
                || status != iced::event::Status::Ignored
                || key != Key::Named(Named::Escape)
            {
                return Task::none();
            }
            return self.update(if self.closing {
                Message::Stay
            } else if !self.error_message.is_empty() {
                Message::DismissError
            } else if self.preferences.replace_settings_open {
                Message::Preferences(settings_pages::Message::ReplaceSettingsPrompt(false))
            } else {
                Message::Preferences(settings_pages::Message::DismissUpdate)
            });
        }
        if !review_open && self.page == Page::WinderustBehaviour {
            if let Some(slot) = self.preferences.hotkey_capture {
                if !repeat {
                    if key == Key::Named(Named::Escape) {
                        return self.update(Message::Preferences(
                            settings_pages::Message::CancelSearchHotkey,
                        ));
                    }
                    if status != iced::event::Status::Ignored {
                        return Task::none();
                    }
                    if let Some(shortcut) = super::hotkeys::capture(&key, modifiers) {
                        return self.update(Message::Preferences(
                            settings_pages::Message::SetSearchHotkey(slot, Some(shortcut)),
                        ));
                    }
                }
                return Task::none();
            }
        }
        if !review_open && self.navigation_search_open {
            match key.as_ref() {
                Key::Named(Named::Tab) if tab => return move_focus(),
                Key::Named(Named::ArrowUp | Named::ArrowDown)
                    if status == iced::event::Status::Ignored && modifiers.is_empty() =>
                {
                    let count = self.search_choices().len();
                    self.navigation_search_selected = search_selection(
                        self.navigation_search_selected,
                        count,
                        key == Key::Named(Named::ArrowDown),
                    );
                    return if count == 0 {
                        Task::none()
                    } else {
                        Task::batch([
                            iced::widget::operation::focus("navigation-search"),
                            self.scroll_search_selection(),
                        ])
                    };
                }
                Key::Named(Named::Enter) if status == iced::event::Status::Ignored && !repeat => {
                    return self.update(Message::ConfirmSearch);
                }
                _ => return Task::none(),
            }
        }
        let cancel_dialog = match self.page {
            _ if review_open => Some(Message::PowerTuning(
                advanced_power_plan_tuning::Message::CancelApply,
            )),
            Page::ProcessList
                if key == Key::Named(Named::Escape)
                    && modifiers.is_empty()
                    && (self.processes.modal_open() || self.processes.context_open()) =>
            {
                Some(Message::Processes(process_list::Message::CloseSelection))
            }
            Page::ByFocus | Page::ByRunningApp => {
                self.process_power_plan_preview().map(|(kind, _)| {
                    Message::ProcessPowerPlans(
                        kind,
                        process_power_plans::Message::Preview(
                            process_power_plans::PreviewMessage::Close,
                        ),
                    )
                })
            }
            Page::Watchdog if self.watchdog.has_pending_editor() => {
                Some(Message::Watchdog(watchdog::Message::CancelRule))
            }
            Page::InstanceLimits if self.instance_limits.has_pending_editor() => Some(
                Message::InstanceLimits(instance_limits::Message::CancelRule),
            ),
            Page::InstanceBalancing if self.instance_balancing.has_pending_editor() => Some(
                Message::InstanceBalancing(instance_balancing::Message::CancelRule),
            ),
            Page::KeepRunning if self.keep_running.has_pending_editor() => {
                Some(Message::KeepRunning(keep_running::Message::CancelRule))
            }
            Page::ByTime if self.time_rules.has_pending_editor() => Some(Message::PowerRules(
                power_rules::Kind::Time,
                power_rules::Message::Cancel,
            )),
            Page::ByCpuLoad if self.cpu_rules.has_pending_editor() => Some(Message::PowerRules(
                power_rules::Kind::CpuLoad,
                power_rules::Message::Cancel,
            )),
            Page::CpuLimiter if self.cpu_limiter.has_pending_editor() && modifiers.is_empty() => {
                Some(Message::CpuLimiter(cpu_limiter::Message::CancelRule))
            }
            _ => self.preset_close_message(),
        };
        if let Some(cancel_dialog) = cancel_dialog {
            let close_focus_preview = matches!(
                cancel_dialog,
                Message::ProcessPowerPlans(process_power_plans::Kind::Focus, _)
            );
            return match key {
                Key::Named(Named::Escape)
                    if !repeat
                        && (close_focus_preview || status == iced::event::Status::Ignored) =>
                {
                    self.update(cancel_dialog)
                }
                Key::Named(Named::Tab) if tab => move_focus(),
                _ => Task::none(),
            };
        }
        if !(repeat
            || self.blocking_editor_open()
            || self.page == Page::ProcessList && self.processes.modal_open())
            && super::hotkeys::opens_search(
                &self.settings.global().general.search_hotkeys,
                &key,
                modifiers,
                status,
            )
        {
            return self.update(Message::OpenNavigationSearch);
        }
        if status == iced::event::Status::Ignored {
            if key == Key::Named(Named::Escape)
                && (self.navigation.section.is_some() || self.navigation.timer().is_some())
            {
                return self.update_navigation(navigation::Event::Dismiss);
            }
            if self.navigation.selected.is_some() && self.navigation.section.is_some() {
                return self.update_navigation(navigation::Event::Key(key, modifiers.shift()));
            }
            if tab {
                return move_focus();
            }
        }
        Task::none()
    }

    fn focus_navigation(&self, page: Option<Page>) -> Task<Message> {
        let focus = iced::widget::operation::focus(navigation::rail_id(page));
        if let Some(page) = page.filter(|page| !navigation::is_utility(*page)) {
            let primary: Vec<_> =
                navigation::rail_pages(self.settings.global().advanced.show_advanced_controls)
                    .into_iter()
                    .filter(|page| !navigation::is_utility(*page))
                    .collect();
            if let Some(index) = primary.iter().position(|p| *p == page) {
                return Task::batch([
                    focus,
                    iced::widget::operation::snap_to(
                        "primary-navigation",
                        iced::widget::scrollable::RelativeOffset {
                            x: 0.0,
                            y: index as f32 / primary.len().saturating_sub(1).max(1) as f32,
                        },
                    ),
                ]);
            }
        }
        focus
    }

    fn update_navigation(&mut self, event: navigation::Event) -> Task<Message> {
        use iced::keyboard::{key::Named, Key};
        use navigation::Event as Nav;
        let tabbing = matches!(event, Nav::Tab(..));
        match event {
            Nav::Select(page) => return self.update(Message::Page(page)),
            Nav::Search => return self.update(Message::OpenNavigationSearch),
            Nav::Blur => self.navigation.selected = None,
            Nav::OpenKeyboard(page) => {
                self.navigation
                    .update(&Nav::OpenKeyboard(page), std::time::Instant::now());
                return self.focus_navigation(Some(page));
            }
            Nav::FocusMove(page, forward) | Nav::Tab(page, forward) => {
                let rail_pages =
                    navigation::rail_pages(self.settings.global().advanced.show_advanced_controls);
                let pages: Vec<_> = std::iter::once(None)
                    .chain(rail_pages.iter().copied().map(Some))
                    .collect();
                if let Some(index) = pages.iter().position(|p| *p == page) {
                    if tabbing
                        && ((forward && index + 1 == pages.len()) || (!forward && index == 0))
                    {
                        return super::keyboard_focus::move_focus(forward, false);
                    }
                    let index = if forward {
                        (index + 1) % pages.len()
                    } else {
                        (index + pages.len() - 1) % pages.len()
                    };
                    self.navigation.selected = None;
                    return self.focus_navigation(pages[index]);
                }
            }
            Nav::Key(key, shift) => {
                if let Some(section) = self.navigation.section {
                    let pages = navigation::children(section);
                    let count = pages.len();
                    if count == 0 {
                        return Task::none();
                    }
                    let selected = self.navigation.selected.unwrap_or(0);
                    self.navigation.selected = Some(match key {
                        Key::Named(Named::Escape | Named::ArrowLeft) => {
                            self.navigation.dismiss();
                            return self.focus_navigation(Some(section));
                        }
                        Key::Named(Named::ArrowDown) => self
                            .navigation
                            .selected
                            .map_or(0, |index| (index + 1) % count),
                        Key::Named(Named::Tab) if !shift => self
                            .navigation
                            .selected
                            .map_or(0, |index| (index + 1) % count),
                        Key::Named(Named::ArrowUp | Named::Tab) => self
                            .navigation
                            .selected
                            .map_or(count - 1, |index| (index + count - 1) % count),
                        Key::Named(Named::Home) => 0,
                        Key::Named(Named::End) => count - 1,
                        Key::Named(Named::Enter | Named::Space) => {
                            return self.update_navigation(Nav::Select(pages[selected]));
                        }
                        _ => selected,
                    });
                    if let Some(index) = self.navigation.selected {
                        return iced::widget::operation::snap_to(
                            "secondary-navigation",
                            iced::widget::scrollable::RelativeOffset {
                                x: 0.0,
                                y: index as f32 / pages.len().saturating_sub(1).max(1) as f32,
                            },
                        );
                    }
                }
            }
            event => {
                let now = match event {
                    Nav::Tick(now) => now,
                    _ => std::time::Instant::now(),
                };
                self.navigation.update(&event, now);
            }
        }
        Task::none()
    }

    fn navigation_panel(&self, section: Page) -> Element<'_, navigation::Event> {
        use navigation::Event as Nav;
        let selected = self.navigation.selected;
        let focused = move |index| selected == Some(index);
        let mut header =
            row![widgets::heading(section.label(), design::typography::SECTION).width(Fill)]
                .spacing(design::space::TIGHT)
                .align_y(iced::Center);
        if self
            .settings
            .global()
            .general
            .show_enabled_feature_counts_in_sidebar
        {
            if let Some(count) = navigation::section_enabled_feature_count(&self.settings, section)
            {
                header = header.push(
                    container(text(count.to_string()).size(design::typography::BADGE))
                        .padding([2, 6])
                        .style(move |theme| widgets::indicator_chip(theme, count > 0)),
                );
            }
        }
        let mut pages = column![].spacing(design::space::TIGHT);
        for (index, page) in navigation::children(section).into_iter().enumerate() {
            let mut entry = row![text(page.label())
                .size(design::typography::BODY)
                .width(Fill)]
            .spacing(design::space::SMALL)
            .align_y(iced::Center);
            if navigation::feature_page_enabled(&self.settings, page) == Some(true) {
                entry = entry.push(
                    container(iced::widget::Space::new())
                        .width(6)
                        .height(6)
                        .style(|theme: &Theme| container::Style {
                            background: Some(theme.palette().success.into()),
                            border: iced::border::rounded(3),
                            ..Default::default()
                        }),
                );
            }
            pages = pages.push(
                button(entry)
                    .pointer_only()
                    .width(Fill)
                    .padding([design::space::MEDIUM as u16, design::space::SMALL as u16])
                    .style(move |theme, status| {
                        navigation::item_style(theme, status, self.page == page, focused(index))
                    })
                    .on_press(Nav::Select(page)),
            );
        }
        let height = iced::Shrink;
        super::scrolling::repaint_popup(
            container(
                column![
                    header,
                    iced::widget::rule::horizontal(1),
                    super::scrolling::popup_scrollable(pages)
                        .id("secondary-navigation")
                        .height(height)
                ]
                .spacing(design::space::MEDIUM)
                .height(height),
            )
            .padding(design::space::MEDIUM as u16)
            .width(Fill)
            .height(height)
            .style(widgets::surface),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        if self.exiting && !message.allowed_during_exit() {
            return Task::none();
        }
        match message {
            Message::DismissPickers => {
                self.preferences.dismiss_picker();
                self.time_rules.dismiss_picker();
            }
            #[cfg(feature = "render-smoke")]
            Message::SmokeScreenshot(screenshot) => return smoke::captured(self, screenshot),
            Message::DismissError => {
                if let Some(after_save) = &mut self.power_plan_control.pending {
                    let _ = after_save
                        .warning
                        .take_if(|warning| warning.as_str() == self.error_message);
                }
                self.error_message.clear();
            }
            Message::SearchKeyboard {
                key,
                modifiers,
                repeat,
                status,
            } => return self.search_keyboard(key, modifiers, repeat, status),
            Message::NavigationSearch(value) => {
                self.navigation_search = value;
                self.navigation_search_selected = 0;
                return self.scroll_search_selection();
            }
            Message::NavigationSearchScope(scope) => {
                self.navigation_search_scope = scope;
                self.navigation_search_selected = 0;
                return self.scroll_search_selection();
            }
            Message::ConfirmSearch => {
                if self.navigation_search_open {
                    if let Some(page) = self
                        .search_choices()
                        .get(self.navigation_search_selected)
                        .copied()
                    {
                        return self.update(Message::Page(page));
                    }
                }
            }
            Message::OpenNavigationSearch => {
                self.navigation.dismiss();
                self.navigation_search_open = true;
                self.navigation_search_scope = SearchScope(None);
                self.navigation_search_selected = 0;
                self.preferences.hotkey_capture = None;
                return iced::widget::operation::focus("navigation-search");
            }
            Message::CloseNavigationSearch => {
                self.navigation_search_open = false;
                self.navigation_search.clear();
            }
            Message::Navigation(event) => return self.update_navigation(event),
            Message::SidePanelTab(tab) => self.side_panel_tab = tab,
            Message::ToggleSidePanel => {
                self.side_panel_open = !self.side_panel_open;
                self.navigation.dismiss();
            }
            Message::CloseSidePanel => self.side_panel_open = false,
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
            Message::Preferences(settings_pages::Message::OpenUpdate(url)) => {
                if self.preferences.show_update
                    && self.preferences.download.as_deref() == Some(url.as_str())
                {
                    match crate::win_util::open_url(&url) {
                        Ok(()) => self.preferences.show_update = false,
                        Err(error) => self.error_message = error,
                    }
                }
            }
            Message::Home(home::Message::Navigate(page)) => {
                return self.update(Message::Page(page));
            }
            Message::Home(home::Message::HoverChart(kind, slot)) => {
                self.home.hover_chart(kind, slot)
            }
            Message::Sample(result) => {
                self.sampling = false;
                match result {
                    Ok(sample) => self.home.record(sample),
                    Err(error) => {
                        self.home.record_unavailable();
                        self.error_message = error;
                    }
                }
            }
            Message::Preferences(settings_pages::Message::Export) => {
                return self.update(Message::SettingsFile(FileDialogMode::Save));
            }
            Message::Preferences(settings_pages::Message::Import) => {
                return self.update(Message::SettingsFile(FileDialogMode::Open));
            }
            Message::Preferences(settings_pages::Message::ReplaceSettingsPrompt(open)) => {
                self.preferences.replace_settings_open = open && self.can_replace_settings();
                if self.preferences.replace_settings_open {
                    self.navigation.dismiss();
                }
            }
            Message::Preferences(settings_pages::Message::ConfirmReplaceSettings) => {
                if std::mem::take(&mut self.preferences.replace_settings_open)
                    && self.can_replace_settings()
                {
                    return self.update(Message::Save);
                }
            }
            Message::Preferences(
                message @ (settings_pages::Message::Check | settings_pages::Message::CheckStartup),
            ) => {
                let automatic = matches!(message, settings_pages::Message::CheckStartup);
                let channel = self.settings.global().general.update_channel;
                let Some(check) = self.preferences.begin_check(channel, automatic) else {
                    return Task::none();
                };
                return tasks::run(move || {
                    crate::update_checker::check(channel)
                        .map(|r| (r.latest_version, r.available_update.map(|u| u.url)))
                        .map_err(|error| error.to_string())
                })
                .map(move |result| {
                    let result = result.and_then(|result| result).map_err(|error| {
                        crate::backend::diagnostics::error(&format!(
                            "Update check failed: {error}"
                        ));
                    });
                    Message::Preferences(settings_pages::Message::Checked(
                        Arc::clone(&check),
                        result,
                    ))
                });
            }
            Message::Preferences(
                message @ (settings_pages::Message::OpenColorPicker(_)
                | settings_pages::Message::EditColor(_)
                | settings_pages::Message::CancelColorPicker
                | settings_pages::Message::PickerHue(_)
                | settings_pages::Message::PickerSv(..)
                | settings_pages::Message::PickerHex(_)
                | settings_pages::Message::RecordSearchHotkey(_)
                | settings_pages::Message::CancelSearchHotkey
                | settings_pages::Message::ToggleAccent
                | settings_pages::Message::DismissUpdate
                | settings_pages::Message::Checked(..)),
            ) => {
                if matches!(
                    message,
                    settings_pages::Message::OpenColorPicker(_)
                        | settings_pages::Message::EditColor(_)
                ) {
                    self.navigation.dismiss();
                }
                self.preferences
                    .update_ui(self.settings.global().general.update_channel, message);
            }
            Message::Preferences(message) => {
                let appearance_changed = matches!(
                    message,
                    settings_pages::Message::Theme(_)
                        | settings_pages::Message::Animation(_)
                        | settings_pages::Message::AccentSource(_)
                        | settings_pages::Message::AccentHex(_)
                        | settings_pages::Message::Accent(_)
                        | settings_pages::Message::ReplaceColor(..)
                        | settings_pages::Message::ColorPicked(_)
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
                return action_log::scroll_to(0.0);
            }
            Message::ActionLog(action_log::Message::Export) => {
                if !self
                    .action_log
                    .begin_export(&self.status.action_log_entries)
                {
                    return Task::none();
                }
                return Task::perform(
                    crate::file_dialog::choose_action_log_export_file(
                        self.hwnd.map(|h| h as windows_sys::Win32::Foundation::HWND),
                    ),
                    Message::ExportLog,
                );
            }
            Message::ActionLog(action_log::Message::KeyboardScroll(direction, height)) => {
                return self.action_log.keyboard_scroll(
                    &self.status.action_log_entries,
                    direction,
                    height,
                );
            }
            Message::ActionLog(message) => {
                let reset = matches!(
                    message,
                    action_log::Message::Level(..)
                        | action_log::Message::Feature(..)
                        | action_log::Message::AllLevels(_)
                        | action_log::Message::AllFeatures(_)
                );
                self.action_log.update(message);
                if reset {
                    return action_log::scroll_to(0.0);
                }
            }
            Message::ExportLog(path) => {
                if let (Some(path), Some(entries)) = (path, self.action_log.finish_export()) {
                    if let Err(error) = crate::config::storage::export_bytes(
                        &path,
                        action_log::action_log_entries_to_csv(&entries).as_bytes(),
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
                    &mut self.settings,
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
                if self.manual_trim_pending {
                    return Task::none();
                }
                let (reply, receiver): (_, crate::automation::MemoryTrimActionReceiver) =
                    std::sync::mpsc::sync_channel(1);
                // Start the waiter before the worker can accept the command.
                let task = match tasks::try_run(move || {
                    receiver
                        .recv()
                        .map_err(|error| error.to_string())?
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                }) {
                    Ok(task) => task,
                    Err(error) => {
                        self.error_message = error.to_string();
                        return Task::none();
                    }
                };
                if let Err(error) = self.runtime.request_memory_trim_now(reply) {
                    self.error_message = error.to_string();
                } else {
                    self.manual_trim_pending = true;
                    return task
                        .map(|result| Message::TrimFinished(result.and_then(|result| result)));
                }
            }
            Message::TrimFinished(result) => {
                self.manual_trim_pending = false;
                if let Err(error) = result {
                    self.error_message = error;
                }
            }
            Message::Trim(message) => self.trim.update(&mut self.settings, message),
            Message::Watchdog(watchdog::Message::Browse) => return self.browse(Page::Watchdog),
            Message::InstanceBalancing(instance_balancing::Message::Browse) => {
                let request = self.instance_balancing.begin_browse();
                return Task::perform(self.choose_executable_file(), move |path| {
                    Message::InstanceBalancingExecutableChosen(request, path)
                });
            }
            Message::InstanceBalancingExecutableChosen(request, path) => {
                if !self.instance_balancing.take_browse(&request) {
                    return Task::none();
                }
                let Some(path) = path else {
                    return Task::none();
                };
                let Some(path) = path.to_str() else {
                    self.error_message = t!("common.unsupported_unicode_path").to_string();
                    return Task::none();
                };
                return self.update(Message::InstanceBalancing(
                    instance_balancing::Message::Path(path.to_owned()),
                ));
            }
            Message::InstanceBalancing(message) => {
                self.settings.edit_global(|settings| {
                    self.instance_balancing
                        .update(&mut settings.instance_balancing, message)
                });
            }
            Message::InstanceLimits(instance_limits::Message::Browse) => {
                return self.browse(Page::InstanceLimits);
            }
            Message::InstanceLimits(message) => {
                self.settings.edit_global(|settings| {
                    self.instance_limits
                        .update(&mut settings.instance_limits, message)
                });
            }
            Message::KeepRunning(keep_running::Message::Browse) => {
                return self.browse(Page::KeepRunning);
            }
            Message::KeepRunning(keep_running::Message::Resume(path)) => {
                if self.pending_settings_changes() {
                    self.error_message = t!("keep_running.save_before_resume").to_string();
                } else if let Err(error) = self.runtime.request_keep_running_resume(path) {
                    self.error_message = error.to_string();
                }
            }
            Message::KeepRunning(message) => {
                self.settings.edit_global(|settings| {
                    self.keep_running
                        .update(&mut settings.keep_running, message)
                });
            }
            Message::Watchdog(watchdog::Message::Tab(resource)) => {
                self.watchdog.select_tab(resource);
            }
            Message::Watchdog(watchdog::Message::PreviewMatches) => {
                return self.watchdog.preview_matches().map(Message::Watchdog);
            }
            Message::Watchdog(watchdog::Message::MatchPreviewLoaded(request, result)) => {
                self.watchdog.complete_match_preview(request, result);
            }
            Message::Watchdog(message) => {
                self.settings
                    .edit_global(|settings| self.watchdog.update(&mut settings.watchdog, message));
            }
            Message::SleepPrevention(sleep_prevention::Message::Browse) => {
                return self.browse(Page::SleepPrevention);
            }
            Message::SleepPrevention(message) => {
                self.sleep_prevention.update(&mut self.settings, message)
            }
            Message::Timer(timer_resolution::Message::Browse) => {
                return self.browse(Page::TimerResolution);
            }
            Message::Timer(message) => self.timer.update(
                &mut self.settings,
                &self.status.feature_status.timer_resolution,
                message,
            ),
            Message::PrioritySeparation(message) => self.priority_separation.update(message),
            Message::PowerTuning(advanced_power_plan_tuning::Message::ConfirmApply)
                if self.power_tuning.review_origin()
                    == Some(advanced_power_plan_tuning::ApplyOrigin::Save) =>
            {
                return self.save_settings();
            }
            Message::PowerTuning(message) => {
                self.power_tuning
                    .update(&mut self.settings, &self.power_plans, message)
            }

            Message::PowerRules(kind, message) => {
                if matches!(message, power_rules::Message::OpenTimePicker(_)) {
                    self.navigation.dismiss();
                }
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
            Message::Allocation(kind, cpu_allocation::Message::Browse) => {
                return self.browse(match kind {
                    cpu_allocation::Kind::Soft => Page::CpuSetsSoft,
                    cpu_allocation::Kind::Hard => Page::ProcessorAffinityHard,
                });
            }
            Message::Allocation(kind, message) => {
                if matches!(
                    message,
                    cpu_allocation::Message::NewPreset | cpu_allocation::Message::EditPreset(_)
                ) {
                    self.side_panel_tab = SidePanelTab::Presets;
                }
                let editor = match kind {
                    cpu_allocation::Kind::Soft => &mut self.soft_allocation,
                    cpu_allocation::Kind::Hard => &mut self.hard_allocation,
                };
                editor.update(&mut self.settings, kind, message);
            }
            Message::Adaptive(adaptive_engine::Message::Browse) => {
                return self.browse(Page::AdaptiveEngine);
            }
            Message::Adaptive(adaptive_engine::Message::InterruptTiming(message)) => {
                self.exit_export_failed |= self.closing
                    && self.interrupt_timing.export_writing()
                    && matches!(&message, interrupt_timing::Message::ExportFinished(Err(_)));
                let hwnd = self
                    .hwnd
                    .map(|hwnd| hwnd as windows_sys::Win32::Foundation::HWND);
                return self.interrupt_timing.update(message, hwnd).map(|message| {
                    Message::Adaptive(adaptive_engine::Message::InterruptTiming(message))
                });
            }
            Message::PollInterruptTiming => self.interrupt_timing.poll(),
            Message::Adaptive(message) => {
                if matches!(
                    message,
                    adaptive_engine::Message::ViewBuiltIn(_)
                        | adaptive_engine::Message::Edit(_)
                        | adaptive_engine::Message::New
                ) {
                    self.side_panel_tab = SidePanelTab::Presets;
                }
                self.adaptive.update(&mut self.settings, message);
            }
            Message::Catalog(result) => {
                self.catalog_loading = false;
                match result {
                    Ok(candidates) => {
                        self.unavailable_candidates = candidates
                            .iter()
                            .filter(|candidate| !candidate.info.has_suspendable_instance)
                            .filter_map(|candidate| {
                                candidate.info.image_path.to_str().map(str::to_owned)
                            })
                            .collect();
                        self.candidates = candidates;
                    }
                    Err(error) => self.error_message = error,
                }
            }
            Message::ExecutableChosen(page, profile, path) => {
                if profile == self.power_source {
                    if let Some(path) = path {
                        let Ok(path) = path.into_os_string().into_string() else {
                            self.error_message = t!("common.unsupported_unicode_path").to_string();
                            return Task::none();
                        };
                        if let Some(kind) = priority_kind(page) {
                            return self.update(Message::Priority(
                                kind,
                                priority_control::Message::Path(path),
                            ));
                        }
                        match page {
                            Page::SleepPrevention => {
                                return self.update(Message::SleepPrevention(
                                    sleep_prevention::Message::Path(path),
                                ));
                            }
                            Page::AppSuspension => {
                                return self.update(Message::Suspension(
                                    app_suspension::Message::Path(path),
                                ));
                            }
                            Page::Watchdog => {
                                return self.update(Message::Watchdog(watchdog::Message::Path(
                                    super::process_rules::picked_path_input(path),
                                )));
                            }
                            Page::InstanceLimits => {
                                return self.update(Message::InstanceLimits(
                                    instance_limits::Message::Path(path),
                                ));
                            }
                            Page::KeepRunning => {
                                return self.update(Message::KeepRunning(
                                    keep_running::Message::Path(path),
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
                            Page::ByFocus => {
                                return self.update(Message::ProcessPowerPlans(
                                    process_power_plans::Kind::Focus,
                                    process_power_plans::Message::Path(
                                        super::process_rules::picked_path_input(path),
                                    ),
                                ));
                            }
                            Page::ByRunningApp => {
                                return self.update(Message::ProcessPowerPlans(
                                    process_power_plans::Kind::RunningApp,
                                    process_power_plans::Message::Path(
                                        super::process_rules::picked_path_input(path),
                                    ),
                                ));
                            }
                            _ => {}
                        }
                    }
                }
            }
            Message::ProcessPowerPlans(kind, process_power_plans::Message::Browse) => {
                return self.browse(match kind {
                    process_power_plans::Kind::Focus => Page::ByFocus,
                    process_power_plans::Kind::RunningApp => Page::ByRunningApp,
                });
            }
            Message::ProcessPowerPlans(kind, process_power_plans::Message::Preview(message)) => {
                let editor = match (kind, self.page) {
                    (process_power_plans::Kind::Focus, Page::ByFocus) => {
                        Some(&mut self.focus_plans)
                    }
                    (process_power_plans::Kind::RunningApp, Page::ByRunningApp) => {
                        Some(&mut self.running_app_plans)
                    }
                    _ => None,
                };
                if let Some(editor) = editor {
                    return editor
                        .update_preview(&self.settings, message)
                        .map(move |message| Message::ProcessPowerPlans(kind, message));
                }
            }
            Message::ProcessPowerPlans(kind, message) => {
                let editor = match kind {
                    process_power_plans::Kind::Focus => &mut self.focus_plans,
                    process_power_plans::Kind::RunningApp => &mut self.running_app_plans,
                };
                editor.update(kind, &mut self.settings, &self.power_plans, message);
            }
            Message::ByActivity(message) => {
                self.activity_inputs.edit(&message);
                by_activity::update(&mut self.settings, message);
            }
            Message::PowerPlanControl(power_plan_control::Message::Select(guid)) => {
                let status = &self.status.power_plan_status;
                if self.power_plans_loaded
                    && self.power_plan_control.pending.is_none()
                    && !power_plan_control::managed_by_adaptive(status, &self.power_plans)
                    && power_plan_control::is_selectable(&self.power_plans, &guid)
                {
                    let current = power_plan_control::current_plan_guid(status, &self.power_plans);
                    self.power_plan_control.selected_guid = (!current
                        .is_some_and(|current| current.eq_ignore_ascii_case(&guid)))
                    .then_some(guid);
                }
            }
            Message::PowerPlanControl(power_plan_control::Message::ProcessorPower) => {
                let profile = if crate::backend::power_source::is_plugged_in() == Some(false) {
                    PowerSourceProfile::OnBattery
                } else {
                    PowerSourceProfile::PluggedIn
                };
                if !self.select_power_source(profile) {
                    return Task::none();
                }
                self.adaptive.show_processor_power();
                return self.update(Message::Page(Page::AdaptiveEngine));
            }
            Message::PowerPlanSwitchFinished(result) => {
                let after_save = self.power_plan_control.pending.take();
                match result {
                    Ok(()) => {
                        self.power_plan_control.selected_guid = None;
                        if after_save.is_some_and(|save| save.exit) && self.closing {
                            return self.shutdown();
                        }
                    }
                    Err(error) => {
                        if matches!(
                            error,
                            RuntimeCommandError::PowerPlanUnavailable
                                | RuntimeCommandError::PowerPlan(
                                    ManualPowerPlanError::ManagedTarget
                                )
                        ) {
                            self.power_plan_control.selected_guid = None;
                        }
                        let warning = after_save.and_then(|save| save.warning);
                        self.error_message = power_plan_switch_error(error, warning.as_deref());
                    }
                }
                self.power_plans_reload_pending |= self.power_plans_loading;
                return self.load_power_plans();
            }
            Message::PowerPlans(result) => {
                self.power_plans_loading = false;
                match accept_power_plan_catalog(
                    &mut self.power_plans,
                    result,
                    &mut self.power_plans_reload_pending,
                ) {
                    Ok(false) => return self.load_power_plans(),
                    Ok(true) => {
                        self.power_plans_loaded = true;
                        if self
                            .power_plan_control
                            .discard_unavailable_plan(&self.power_plans)
                            && self.error_message.is_empty()
                        {
                            self.error_message = t!("common.selected_plan_unavailable").to_string();
                        }
                        if self.page == Page::AdvancedPowerPlanTuning {
                            self.power_tuning.ensure_plan(&self.power_plans);
                        }
                    }
                    Err(error) => {
                        crate::backend::diagnostics::error(&format!(
                            "Loading power plans failed: {error}"
                        ));
                        if self.error_message.is_empty() {
                            self.error_message =
                                t!("power_plan_control.catalog_unavailable").to_string();
                        }
                    }
                }
            }
            Message::CpuLimiter(cpu_limiter::Message::Browse) => {
                return self.browse(Page::CpuLimiter);
            }
            Message::CpuLimiter(message) => {
                self.cpu_limiter.update_profile(&mut self.settings, message)
            }
            Message::PowerSource(source) => {
                self.select_power_source(source);
            }
            Message::WindowAction(window_frame::Event::Close) => {
                return self.update(Message::WindowClose);
            }
            Message::WindowFocused => {
                if !self.hidden && page_needs_power_plans(self.page, self.processes.rulesets_open())
                {
                    return self.load_power_plans();
                }
            }
            Message::WindowAction(action) => {
                if let Some(window) = self.window {
                    return match action {
                        window_frame::Event::Drag => iced::window::drag(window),
                        window_frame::Event::Minimize => iced::window::minimize(window, true),
                        window_frame::Event::ToggleMaximize => {
                            iced::window::toggle_maximize(window)
                        }
                        window_frame::Event::Resize(direction) => {
                            iced::window::drag_resize(window, direction)
                        }
                        window_frame::Event::Close => {
                            unreachable!("Close is routed through WindowClose")
                        }
                    };
                }
            }
            Message::WindowResized(window, size) => {
                if self.window == Some(window) {
                    if let Some(hwnd) = self.hwnd {
                        self.maximized = crate::platform::windows::window_dpi::is_maximized(
                            hwnd as windows_sys::Win32::Foundation::HWND,
                        );
                    }
                    if self.window_is_restored() && size.width > 0.0 && size.height > 0.0 {
                        self.window_settings.size = size;
                    }
                }
            }
            Message::WindowMoved(window, position) => {
                if self.window == Some(window) && self.window_is_restored() {
                    self.window_settings.position = iced::window::Position::Specific(position);
                }
            }
            Message::Window(window) => {
                if self.window == Some(window) {
                    return iced::window::run(window, |window| {
                        use raw_window_handle::RawWindowHandle;
                        match window.window_handle().ok()?.as_raw() {
                            RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as usize),
                            _ => None,
                        }
                    })
                    .map(move |hwnd| Message::NativeWindow(window, hwnd));
                }
            }
            Message::NativeWindow(window, hwnd) => {
                if self.window != Some(window) {
                    return Task::none();
                }
                self.hwnd = hwnd;
                if let Some(hwnd) = hwnd {
                    let maximize = self.maximized;
                    if let Err(error) = crate::platform::windows::window_dpi::install(
                        hwnd as windows_sys::Win32::Foundation::HWND,
                    ) {
                        crate::backend::diagnostics::error(&error);
                    }
                    if let Err(error) = crate::platform::windows::window_dpi::ensure_visible(
                        hwnd as windows_sys::Win32::Foundation::HWND,
                    ) {
                        crate::backend::diagnostics::error(&error);
                        self.error_message = error;
                    }
                    self.maximized = crate::platform::windows::window_dpi::is_maximized(
                        hwnd as windows_sys::Win32::Foundation::HWND,
                    );
                    if let Err(error) =
                        tray::set_window_icon(hwnd as windows_sys::Win32::Foundation::HWND)
                    {
                        crate::backend::diagnostics::error(&error);
                    }
                    tray::show_window(hwnd as windows_sys::Win32::Foundation::HWND);
                    if maximize {
                        return iced::window::maximize(window, true);
                    }
                }
            }
            Message::NavigateHistory(forward) => {
                if !self.closing
                    && self.error_message.is_empty()
                    && !self.blocking_editor_open()
                    && self.process_power_plan_preview().is_none()
                {
                    if let Some(page) = self.navigation_history.travel(forward) {
                        return self.update(Message::Page(page));
                    }
                }
            }
            Message::Page(page) => {
                let entering_page = self.page != page;
                self.preferences.dismiss_picker();
                self.time_rules.dismiss_picker();
                self.focus_plans.close_preview();
                self.running_app_plans.close_preview();
                self.preferences.hotkey_capture = None;
                self.preferences.replace_settings_open = false;
                self.navigation_history.visit(page);
                self.side_panel_tab = SidePanelTab::for_page(page)[0];
                let path = navigation::breadcrumb_path(page);
                self.breadcrumb = path;
                if entering_page && page == Page::Home {
                    self.home
                        .set_power_source(home::PowerSourceReading::Collecting);
                }
                self.page = page;
                self.navigation_search_open = false;
                self.navigation_search.clear();
                self.navigation.dismiss();
                if entering_page && page == Page::Win32PrioritySeparation {
                    self.priority_separation.refresh();
                }
                if page == Page::AdvancedPowerPlanTuning {
                    self.power_tuning.ensure_plan(&self.power_plans);
                }
                let mut tasks = Vec::new();
                if page_needs_power_plans(page, self.processes.rulesets_open()) {
                    tasks.push(self.load_power_plans());
                }
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
                if (page.supports_power_source_profiles()
                    || matches!(
                        page,
                        Page::ActionLog
                            | Page::Watchdog
                            | Page::KeepRunning
                            | Page::InstanceLimits
                            | Page::InstanceBalancing
                    ))
                    && !self.catalog_loading
                {
                    tasks.push(self.load_catalog());
                }
                return Task::batch(tasks);
            }
            Message::Processes(process_list::Message::Details(
                process_list::details::Message::ReloadPlans,
            )) => {
                return self.load_power_plans();
            }
            Message::Processes(message) => {
                let load_plans = matches!(
                    message,
                    process_list::Message::ProcessTab(process_list::ProcessTab::Rulesets)
                        | process_list::Message::OpenDetails
                );
                let task = self
                    .processes
                    .update(message, &mut self.settings, &self.runtime)
                    .map(Message::Processes);
                if self.processes.modal_open() {
                    self.navigation.dismiss();
                }
                return if load_plans && self.processes.rulesets_open() {
                    Task::batch([task, self.load_power_plans()])
                } else {
                    task
                };
            }
            Message::Wake => {
                self.sync_tray();
                return self.update(Message::Tick);
            }
            Message::Tick => {
                #[cfg(feature = "render-smoke")]
                if let Some(task) = window_lifecycle_smoke::advance(self) {
                    return task;
                }
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
                        tray::MenuAction::Feature { id, enabled, .. }
                            if id == Page::Watchdog as usize =>
                        {
                            self.settings.set_watchdog_enabled(enabled)
                        }
                        tray::MenuAction::Feature { id, enabled, .. }
                            if id == Page::InstanceLimits as usize =>
                        {
                            self.settings.set_instance_limits_enabled(enabled)
                        }
                        tray::MenuAction::Feature { id, enabled, .. }
                            if id == Page::InstanceBalancing as usize =>
                        {
                            self.settings.set_instance_balancing_enabled(enabled)
                        }
                        tray::MenuAction::Feature { id, enabled, .. }
                            if id == Page::KeepRunning as usize =>
                        {
                            self.settings.set_keep_running_enabled(enabled)
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
                let mut work = Vec::new();
                if restore || (self.hidden && self.tray_retry_pending()) {
                    work.push(self.show_window());
                }
                if let Some(status) = self.runtime.status_snapshot_since(self.status.generation) {
                    if sync_active_power_plan(
                        &mut self.power_plans,
                        self.status.power_plan_status.current_guid.as_deref(),
                        status.power_plan_status.current_guid.as_deref(),
                    ) {
                        if self.power_plans_loading {
                            self.power_plans_reload_pending = true;
                        } else if !self.hidden
                            && page_needs_power_plans(self.page, self.processes.rulesets_open())
                            && !self.power_plans.iter().any(|plan| plan.active)
                        {
                            work.push(self.load_power_plans());
                        }
                    }
                    if status.appearance_change_generation
                        != self.status.appearance_change_generation
                    {
                        self.appearance = settings_pages::theme(&self.settings.global().general);
                    }
                    self.processes.sync_suspended_processes(
                        &status
                            .feature_status
                            .app_suspension
                            .suspended_process_instances,
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
                if !self.hidden {
                    if self.page == Page::ProcessList {
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
                    if (self.page.supports_power_source_profiles()
                        || matches!(
                            self.page,
                            Page::ActionLog
                                | Page::Watchdog
                                | Page::KeepRunning
                                | Page::InstanceLimits
                                | Page::InstanceBalancing
                        ))
                        && !self.catalog_loading
                        && self.catalog_sampled_at.elapsed() >= Duration::from_secs(3)
                    {
                        self.catalog_sampled_at = std::time::Instant::now();
                        work.push(self.load_catalog());
                    }
                }
                if self.page == Page::Home
                    && !self.hidden
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
            Message::WindowCloseRequested(window) => {
                if self.window == Some(window) {
                    return self.update(Message::WindowClose);
                }
            }
            Message::WindowClose => {
                if self.settings.global().general.hide_to_tray
                    && self.tray.as_ref().is_some_and(|icon| icon.is_registered())
                    && !self.hwnd.is_some_and(|hwnd| {
                        tray::close_needs_prompt(hwnd as windows_sys::Win32::Foundation::HWND)
                    })
                {
                    return self.close_window();
                } else {
                    return self.update(Message::Close);
                }
            }
            Message::Close => {
                self.power_tuning.cancel_apply_review();
                if !self.closing {
                    self.error_message.clear();
                    self.exit_export_failed = false;
                }
                self.closing = true;
                return self.show_window();
            }
            Message::Save if self.closing && self.exit_blocking_status().is_some() => {}
            Message::Save | Message::Cancel if self.power_plan_control.pending.is_some() => {}
            Message::Save
                if self.power_plan_control.selected_guid.is_some()
                    && !self.pending_settings_changes() =>
            {
                return self.finish_save(self.closing, None);
            }
            Message::Save if self.pending_editor() => {
                self.error_message = t!("unsaved.message").to_string();
            }
            Message::Save if self.settings.validate_watchdog().is_err() => {
                self.error_message = t!("watchdog.invalid").to_string();
            }
            Message::Save if self.settings.validate_instance_limits().is_err() => {
                self.error_message = t!("instance_limits.invalid").to_string();
            }
            Message::Save if self.settings.validate_instance_balancing().is_err() => {
                self.error_message = t!("instance_balancing.invalid").to_string();
            }
            Message::Save if self.settings.validate_keep_running().is_err() => {
                self.error_message = t!("keep_running.invalid").to_string();
            }
            Message::Save if self.watchdog.has_invalid_inputs() => {
                self.error_message = t!("watchdog.invalid_duration").to_string();
            }
            Message::Save if self.adaptive.validation_error().is_some() => {
                self.error_message = self.adaptive.validation_error().unwrap_or_default();
            }
            Message::Save if self.cpu_limiter.has_invalid_inputs() => {
                self.error_message = t!("cpu_limiter.invalid_limit").to_string();
            }
            Message::Save if self.power_tuning.has_invalid_inputs() => {
                self.error_message = t!("processor_power.invalid_percentage").to_string();
            }
            Message::Save if !self.time_rules.valid() || !self.cpu_rules.valid() => {
                self.error_message = t!("unsaved.message").to_string();
            }
            Message::Save if !self.activity_inputs.valid() => {
                self.error_message = t!("by_activity.invalid_timing").to_string();
            }
            Message::Save if self.preferences.has_invalid_failure_threshold() => {
                self.error_message =
                    t!("settings.invalid_failure_suppression_threshold").to_string();
            }
            Message::Save if self.timer.has_invalid_inputs() => {
                self.error_message = t!("timer_resolution.invalid_request").to_string();
            }
            Message::Save if self.preferences.has_invalid_accent() => {
                self.error_message = t!("accent.invalid_hex").to_string();
            }
            Message::Save => {
                if let Some(delay) = self.suspension.invalid_delay() {
                    self.error_message = delay.error_message();
                } else if self.suspension.has_invalid_thresholds() {
                    self.error_message = t!("app_suspension.invalid_network_threshold").to_string();
                } else if let Some(threshold) = self.trim.invalid_threshold() {
                    self.error_message = threshold.error_message();
                } else if self.power_tuning.dirty {
                    match self.power_tuning.review_apply(
                        &self.power_plans,
                        advanced_power_plan_tuning::ApplyOrigin::Save,
                    ) {
                        Ok(()) => {
                            self.error_message.clear();
                            if !self.power_tuning.dirty {
                                return if self.pending_settings_changes() {
                                    self.save_settings()
                                } else {
                                    self.finish_save(self.closing, None)
                                };
                            }
                        }
                        Err(error) => self.error_message = error,
                    }
                } else {
                    return self.save_settings();
                }
            }
            Message::Cancel => {
                self.power_plan_control.selected_guid = None;
                let previous_channel = self.settings.global().general.update_channel;
                self.settings.cancel();
                if self.power_tuning.dirty {
                    self.power_tuning.discard_values(&self.power_plans);
                }
                if let Some(error) = self.settings_replaced(previous_channel) {
                    self.error_message = error;
                }
                self.closing = false;
            }
            Message::PausePowerPlans(value) => self.settings.edit_global(|settings| {
                settings.general.pause_power_plan_switching_while_plugged_in = value
            }),
            Message::RestorePreviousPowerPlan(value) => self
                .settings
                .edit_global(|settings| settings.general.restore_previous_power_plan = value),
            Message::ShutdownFinished(result) => {
                if let Err(error) = result {
                    crate::backend::diagnostics::error(&error);
                }
                return self.finish_exit();
            }
            Message::Stay => {
                if let Some(after_save) = &mut self.power_plan_control.pending {
                    *after_save = power_plan_control::AfterSave::default();
                }
                self.closing = false;
                self.exit_export_failed = false;
                self.error_message.clear();
            }
            Message::DiscardAndClose if self.exit_blocking_status().is_some() => {}
            Message::DiscardAndClose => {
                self.settings.cancel();
                if self.power_tuning.dirty {
                    self.power_tuning.discard_values(&self.power_plans);
                }
                return self.shutdown();
            }
            Message::SettingsFile(_) if self.blocking_editor_open() || self.invalid_inputs() => {
                self.error_message = t!("unsaved.message").to_string();
            }
            Message::SettingsFile(FileDialogMode::Open)
            | Message::SettingsFileChosen(FileDialogMode::Open, Some(_))
                if self.settings.has_unsaved_changes() =>
            {
                self.error_message = t!("settings.import_unsaved").to_string();
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
                        FileDialogMode::Open => {
                            let previous_channel = self.settings.global().general.update_channel;
                            match self.settings.import_toml_from(&path) {
                                Ok(outcome) => {
                                    let tray_warning = self.settings_replaced(previous_channel);
                                    let startup_warning =
                                        outcome.startup_registration_error().map(|error| {
                                            t!("settings.imported_startup_failed", error = error)
                                                .into_owned()
                                        });
                                    if let Some(warning) =
                                        combine_warnings(startup_warning, tray_warning)
                                    {
                                        self.error_message = warning;
                                    }
                                }
                                Err(error) => {
                                    self.error_message =
                                        t!("settings.import_failed", error = error).into_owned();
                                }
                            }
                        }
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
        self.power_plan_control.selected_guid.is_some() || self.pending_settings_changes()
    }

    fn pending_settings_changes(&self) -> bool {
        self.settings.has_unsaved_changes()
            || self.power_tuning.dirty
            || self.pending_editor()
            || self.invalid_inputs()
    }

    fn can_replace_settings(&self) -> bool {
        self.page == Page::WinderustBehaviour
            && !self.closing
            && self.settings.load_error().is_some()
            && !self.pending_changes()
    }

    fn save_settings(&mut self) -> Task<Message> {
        if (self.power_tuning.dirty || self.power_tuning.has_invalid_inputs())
            && !self.power_tuning.preflight_save(&self.power_plans)
        {
            self.error_message = self.power_tuning.status().into_owned();
            return Task::none();
        }
        let settings_changed = self.power_tuning.dirty && self.settings.has_unsaved_changes();
        let outcome = match self.settings.save() {
            Ok(outcome) => outcome,
            Err(error) => {
                self.power_tuning.cancel_apply_review();
                self.error_message = error.to_string();
                return Task::none();
            }
        };
        let startup_warning = outcome
            .startup_registration_error()
            .map(|error| t!("settings.saved_startup_failed", error = error).into_owned());
        let exit_after_save = self.closing && startup_warning.is_none();
        let reported_saved = startup_warning.is_some();
        let tray_warning = self.publish_settings_with_tray_warning();
        let warning = combine_warnings(startup_warning, tray_warning);
        if self.power_tuning.dirty && !self.power_tuning.confirm_apply(&self.power_plans) {
            let status = self.power_tuning.status();
            let tuning_error = if reported_saved {
                t!("processor_power.apply_failed", error = status.as_ref()).into_owned()
            } else if settings_changed {
                t!(
                    "processor_power.settings_saved_tuning_failed",
                    error = status.as_ref()
                )
                .into_owned()
            } else {
                status.into_owned()
            };
            self.error_message = match warning {
                Some(warning) => format!("{warning}\n{tuning_error}"),
                None => tuning_error,
            };
            return Task::none();
        }
        if let Some(warning) = &warning {
            self.error_message.clone_from(warning);
        }
        self.reset_editors();
        self.finish_save(exit_after_save, warning)
    }

    fn finish_save(&mut self, exit_after_save: bool, warning: Option<String>) -> Task<Message> {
        let Some(guid) = self.power_plan_control.selected_guid.clone() else {
            return if exit_after_save {
                self.shutdown()
            } else {
                Task::none()
            };
        };
        let receiver = match self.runtime.request_power_plan_switch(guid) {
            Ok(receiver) => receiver,
            Err(error) => {
                self.error_message = power_plan_switch_error(error, warning.as_deref());
                return Task::none();
            }
        };
        self.power_plan_control.pending = Some(power_plan_control::AfterSave {
            exit: exit_after_save,
            warning,
        });
        tasks::run(move || {
            receiver
                .recv()
                .unwrap_or(Err(RuntimeCommandError::WorkerExited))
        })
        .map(|result| {
            Message::PowerPlanSwitchFinished(
                result
                    .map_err(RuntimeCommandError::CommandFailed)
                    .and_then(|result| result),
            )
        })
    }
    fn invalid_inputs(&self) -> bool {
        self.settings.validate_watchdog().is_err()
            || self.settings.validate_keep_running().is_err()
            || self.settings.validate_instance_limits().is_err()
            || self.settings.validate_instance_balancing().is_err()
            || self.watchdog.has_invalid_inputs()
            || self.cpu_limiter.has_invalid_inputs()
            || self.power_tuning.has_invalid_inputs()
            || self.suspension.invalid_delay().is_some()
            || self.suspension.has_invalid_thresholds()
            || self.trim.invalid_threshold().is_some()
            || self.timer.has_invalid_inputs()
            || self.preferences.has_invalid_inputs()
            || self.adaptive.validation_error().is_some()
            || !self.activity_inputs.valid()
            || !self.time_rules.valid()
            || !self.cpu_rules.valid()
    }
    fn show_window(&mut self) -> Task<Message> {
        if self.hidden && self.page == Page::Home {
            self.home
                .set_power_source(home::PowerSourceReading::Collecting);
        }
        self.hidden = false;
        tray::set_hidden_to_tray(false);
        if let Some(hwnd) = self.hwnd {
            tray::show_window(hwnd as windows_sys::Win32::Foundation::HWND);
        }
        if let Some(window) = self.window {
            return iced::window::set_mode(window, iced::window::Mode::Windowed);
        }
        let (window, open) = iced::window::open(self.window_settings.clone());
        // Record pending creation too, so repeated restore requests cannot open two windows.
        self.window = Some(window);
        open.map(Message::Window)
    }

    fn window_is_restored(&self) -> bool {
        self.hwnd.is_some_and(|hwnd| {
            crate::platform::windows::window_dpi::is_restored(
                hwnd as windows_sys::Win32::Foundation::HWND,
            )
        })
    }

    fn close_window(&mut self) -> Task<Message> {
        if let Some(hwnd) = self.hwnd {
            self.maximized = crate::platform::windows::window_dpi::is_maximized(
                hwnd as windows_sys::Win32::Foundation::HWND,
            );
        }
        self.hidden = true;
        self.hwnd = None;
        self.navigation.dismiss();
        self.navigation_search_open = false;
        self.preferences.dismiss_picker();
        self.preferences.hotkey_capture = None;
        self.time_rules.dismiss_picker();
        tray::set_hidden_to_tray(true);
        // Closing the last Iced window releases its renderer/compositor, while the daemon
        // retains settings drafts, background automation, tray actions, and wake subscriptions.
        self.window
            .take()
            .map(iced::window::close)
            .unwrap_or_else(Task::none)
    }
    fn pending_editor(&self) -> bool {
        self.power_tuning.has_pending_editor()
            || self.adaptive.has_pending_editor()
            || self.soft_allocation.has_pending_editor()
            || self.hard_allocation.has_pending_editor()
            || self.cpu_limiter.has_pending_editor()
            || self.watchdog.has_pending_editor()
            || self.keep_running.has_pending_editor()
            || self.instance_limits.has_pending_editor()
            || self.instance_balancing.has_pending_editor()
            || self.time_rules.has_pending_editor()
            || self.cpu_rules.has_pending_editor()
    }

    fn blocking_editor_open(&self) -> bool {
        self.preferences.replace_settings_open
            || self.adaptive.preset_open()
            || self.preset_close_message().is_some()
            || self.pending_editor()
    }

    fn preset_close_message(&self) -> Option<Message> {
        match self.page {
            Page::AdaptiveEngine if self.adaptive.preset_open() => {
                Some(Message::Adaptive(adaptive_engine::Message::Cancel))
            }
            Page::CpuSetsSoft if self.soft_allocation.preset_open() => Some(Message::Allocation(
                cpu_allocation::Kind::Soft,
                cpu_allocation::Message::ClosePreset,
            )),
            Page::ProcessorAffinityHard if self.hard_allocation.preset_open() => {
                Some(Message::Allocation(
                    cpu_allocation::Kind::Hard,
                    cpu_allocation::Message::ClosePreset,
                ))
            }
            Page::AdvancedPowerPlanTuning if self.power_tuning.preset_open() => Some(
                Message::PowerTuning(advanced_power_plan_tuning::Message::ClosePreset),
            ),
            _ => None,
        }
    }

    fn process_power_plan_preview(
        &self,
    ) -> Option<(process_power_plans::Kind, &process_power_plans::Editor)> {
        let (kind, editor) = match self.page {
            Page::ByFocus => (process_power_plans::Kind::Focus, &self.focus_plans),
            Page::ByRunningApp => (
                process_power_plans::Kind::RunningApp,
                &self.running_app_plans,
            ),
            _ => return None,
        };
        editor.preview_open().then_some((kind, editor))
    }

    fn select_power_source(&mut self, source: PowerSourceProfile) -> bool {
        if source == self.power_source {
            return true;
        }
        if self.blocking_editor_open() || self.invalid_inputs() {
            self.error_message = t!("unsaved.message").to_string();
            return false;
        }
        self.power_source = source;
        self.settings.select_power_source(source);
        self.reset_editors();
        true
    }

    fn settings_replaced(&mut self, previous_channel: UpdateChannel) -> Option<String> {
        self.reset_editors();
        let general = &self.settings.global().general;
        if general.update_channel != previous_channel {
            self.preferences.reset_update_check();
        }
        rust_i18n::set_locale(general.language.locale());
        self.publish_settings_with_tray_warning()
    }

    fn reset_editors(&mut self) {
        self.preferences.reset_drafts();
        self.power_tuning.discard_editor();
        self.cpu_limiter = Default::default();
        self.activity_inputs = Default::default();
        self.focus_plans = Default::default();
        self.running_app_plans = Default::default();
        self.priority = Default::default();
        self.efficiency = Default::default();
        self.soft_allocation.reset_drafts();
        self.hard_allocation.reset_drafts();
        self.adaptive.reset_drafts();
        self.time_rules = Default::default();
        self.cpu_rules = Default::default();
        self.suspension = Default::default();
        self.sleep_prevention = Default::default();
        self.trim = Default::default();
        self.watchdog = Default::default();
        self.keep_running = Default::default();
        self.instance_limits = Default::default();
        self.instance_balancing = Default::default();
        self.timer = Default::default();
        self.appearance = settings_pages::theme(&self.settings.global().general);
    }
    fn browse(&self, page: Page) -> Task<Message> {
        let profile = self.power_source;
        Task::perform(self.choose_executable_file(), move |path| {
            Message::ExecutableChosen(page, profile, path)
        })
    }

    fn choose_executable_file(&self) -> impl std::future::Future<Output = Option<PathBuf>> + Send {
        crate::file_dialog::choose_executable_file(
            self.hwnd.map(|h| h as windows_sys::Win32::Foundation::HWND),
        )
    }

    fn publish_settings(&mut self) {
        if let Some(error) = self.publish_settings_with_tray_warning() {
            self.error_message = error;
        }
    }

    fn publish_settings_with_tray_warning(&mut self) -> Option<String> {
        self.runtime
            .replace_settings(&self.settings.runtime_settings_snapshot());
        self.sync_tray_warning()
    }

    fn load_catalog(&mut self) -> Task<Message> {
        self.catalog_loading = true;
        let cached = Arc::clone(&self.candidates);
        tasks::run(move || super::app_picker::load(&cached))
            .map(|result| Message::Catalog(result.and_then(|result| result)))
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
            || self.settings.persisted().general.start_minimized
            || notifications_required(self.settings.persisted()))
            && self.tray.as_ref().is_none_or(|icon| !icon.is_registered())
    }

    fn sync_tray(&mut self) {
        if let Some(error) = self.sync_tray_warning() {
            self.error_message = error;
        }
    }

    fn sync_tray_warning(&mut self) -> Option<String> {
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
                            navigation::feature_page_enabled(
                                if matches!(
                                    *page,
                                    Page::Watchdog
                                        | Page::KeepRunning
                                        | Page::InstanceLimits
                                        | Page::InstanceBalancing
                                ) {
                                    saved
                                } else {
                                    settings
                                },
                                *page,
                            )
                            .map(|enabled| tray::FeatureToggle {
                                id: *page as usize,
                                label: page.label(),
                                enabled,
                            })
                        })
                        .collect();
                    (!items.is_empty()).then(|| (section.landing_page.label(), items))
                })
                .collect(),
        });
        let intent = (
            self.settings.global().general.hide_to_tray,
            self.settings.persisted().general.start_minimized
                || notifications_required(self.settings.persisted()),
        );
        if !intent.0 && !intent.1 {
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
                let first_attempt = self.tray_attempt.is_none();
                self.tray_attempt = Some((intent, now));
                let result = if let Some(icon) = &mut self.tray {
                    icon.register()
                } else {
                    tray::TrayIcon::install().map(|icon| self.tray = Some(icon))
                };
                if first_attempt {
                    return result.err();
                }
            }
        }
        None
    }

    fn exit_blocking_status(&self) -> Option<&'static str> {
        if self.interrupt_timing.export_writing() {
            Some("interrupt_timing.export_saving")
        } else {
            self.power_plan_control
                .pending
                .is_some()
                .then_some("power_plan_control.switching")
        }
    }

    fn shutdown(&mut self) -> Task<Message> {
        if self.exit_blocking_status().is_some() {
            return Task::none();
        }
        crate::backend::diagnostics::event("Exit requested; restoring managed state.");
        self.exiting = true;
        self.closing = true;
        self.error_message.clear();
        let interrupt_timing = self.interrupt_timing.cancel_for_exit();
        let runtime = self.runtime.clone();
        let recovery_client = Arc::clone(&self.recovery_client);
        tasks::run(move || {
            if let Err(error) = runtime.shutdown() {
                crate::backend::diagnostics::error(&error);
            }
            if let Err(error) = recovery_client
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .finish()
            {
                crate::backend::diagnostics::error(&error);
            }
            if let Some(capture) = interrupt_timing {
                if let Err(error) = capture.finish() {
                    crate::backend::diagnostics::error(&format!(
                        "Interrupt Timing cleanup failed with {error}"
                    ));
                }
            }
        })
        .map(Message::ShutdownFinished)
    }

    fn finish_exit(&mut self) -> Task<Message> {
        self.tray = None;
        tray::set_ui_wake(None);
        iced::exit()
    }

    fn picker_open(&self) -> bool {
        !self.navigation_search_open
            && !self.closing
            && !self.preferences.show_update
            && self.power_tuning.review_origin().is_none()
            && self.error_message.is_empty()
            && match self.page {
                Page::ByTime => self.time_rules.picker_open(),
                Page::LanguageAndAppearance => self.preferences.picker_open(),
                _ => false,
            }
    }

    fn view(&self, _window: iced::window::Id) -> Element<'_, Message> {
        let picker_open = self.picker_open();
        window_frame::view(
            super::modal::picker(self.view_workspace(), picker_open, Message::DismissPickers),
            self.maximized,
            !picker_open,
            Message::WindowAction,
        )
    }

    fn view_workspace(&self) -> Element<'_, Message> {
        let content = self.view_content();
        if let Some(modal) = self
            .power_tuning
            .apply_review_modal(&self.power_plans, &self.status.power_plan_status)
        {
            return centered_modal(content, modal.map(Message::PowerTuning));
        }
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
            if !self.exiting && self.closing {
                let pending_changes = self.pending_changes();
                let blocking_status = self.exit_blocking_status();
                let can_exit = blocking_status.is_none();
                let status = blocking_status.unwrap_or(if pending_changes {
                    "exit_prompt.unsaved"
                } else {
                    "exit_prompt.message"
                });
                body = body.push(text(t!(status).to_string()));
                actions = actions.push(
                    action_button(t!("common.cancel").to_string())
                        .style(widgets::tertiary_button)
                        .on_press(Message::Stay),
                );
                if pending_changes {
                    actions = actions
                        .push(
                            action_button(t!("exit_prompt.save_and_exit").to_string())
                                .style(widgets::primary_button)
                                .on_press_maybe(can_exit.then_some(Message::Save)),
                        )
                        .push(
                            action_button(t!("exit_prompt.without_saving").to_string())
                                .style(widgets::danger_button)
                                .on_press_maybe(can_exit.then_some(Message::DiscardAndClose)),
                        );
                } else {
                    actions = actions.push(
                        action_button(t!("tray.exit").to_string())
                            .style(widgets::danger_button)
                            .on_press_maybe(can_exit.then_some(Message::DiscardAndClose)),
                    );
                }
            } else if !self.exiting {
                actions = actions.push(
                    action_button(t!("common.done").to_string())
                        .style(widgets::tertiary_button)
                        .on_press(Message::DismissError),
                );
            }
            let export_failure = (self.closing && self.exit_export_failed)
                .then(|| self.interrupt_timing.export_failure())
                .flatten();
            if !self.exiting && (!self.error_message.is_empty() || export_failure.is_some()) {
                let mut details = column![].spacing(design::space::SMALL);
                if !self.error_message.is_empty() {
                    details = details
                        .push(text(&self.error_message).wrapping(text::Wrapping::WordOrGlyph));
                }
                if let Some(error) = export_failure {
                    details = details.push(text(error).wrapping(text::Wrapping::WordOrGlyph));
                }
                body = body.push(container(scrollable(details)).max_height(320));
            }
            if !self.exiting {
                body = body.push(actions);
            }
            let dialog = container(body)
                .padding(design::space::LARGE as u16)
                .width(Fill)
                .max_width(640)
                .style(widgets::surface);
            return iced::widget::stack![
                super::modal::background(content),
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
        if self.preferences.replace_settings_open {
            return centered_modal(
                content,
                self.preferences
                    .replace_settings_modal()
                    .map(Message::Preferences),
            );
        }
        if let Some((kind, editor)) = self.process_power_plan_preview() {
            if let Some(preview) = editor.preview_modal(&self.power_plans) {
                return centered_modal(
                    content,
                    preview.map(move |message| Message::ProcessPowerPlans(kind, message)),
                );
            }
        }
        if self.preferences.show_update {
            let dialog = container(
                column![
                    widgets::heading(
                        t!("about.update_available").to_string(),
                        design::typography::DIALOG_TITLE,
                    ),
                    column![
                        text(t!("about.latest_version").to_string())
                            .size(design::typography::SECONDARY)
                            .style(text::secondary),
                        text(self.preferences.latest.as_deref().unwrap_or_default())
                            .wrapping(text::Wrapping::WordOrGlyph),
                    ]
                    .spacing(design::space::TIGHT),
                    row![
                        iced::widget::Space::new().width(Fill),
                        action_button(t!("common.cancel").to_string())
                            .style(widgets::tertiary_button)
                            .on_press(Message::Preferences(settings_pages::Message::DismissUpdate)),
                        action_button(t!("about.open_release_page").to_string())
                            .style(widgets::primary_button)
                            .on_press_maybe(self.preferences.download.clone().map(|url| {
                                Message::Preferences(settings_pages::Message::OpenUpdate(url))
                            })),
                    ]
                    .spacing(design::space::SMALL),
                ]
                .spacing(design::space::LARGE),
            )
            .padding(design::space::LARGE as u16)
            .width(Fill)
            .max_width(460)
            .style(widgets::surface);
            return centered_modal(content, dialog.into());
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
        feature_info_sidebar(page, self.side_panel_tab, || self.side_panel_body(page))
    }

    fn side_panel_body(&self, page: Page) -> Option<Element<'_, Message>> {
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
        } else if page == Page::AdaptiveEngine && self.side_panel_tab == SidePanelTab::Presets {
            Some(
                self.adaptive
                    .presets_panel(&self.settings)
                    .map(Message::Adaptive),
            )
        } else if page == Page::AdvancedPowerPlanTuning {
            Some(
                self.power_tuning
                    .presets_panel(&self.settings.global().advanced_power_plan_tuning_presets)
                    .map(Message::PowerTuning),
            )
        } else if page == Page::CpuSetsSoft && self.side_panel_tab == SidePanelTab::Presets {
            Some(
                self.soft_allocation
                    .presets_panel(&self.settings, cpu_allocation::Kind::Soft)
                    .map(|m| Message::Allocation(cpu_allocation::Kind::Soft, m)),
            )
        } else if page == Page::ProcessorAffinityHard
            && self.side_panel_tab == SidePanelTab::Presets
        {
            Some(
                self.hard_allocation
                    .presets_panel(&self.settings, cpu_allocation::Kind::Hard)
                    .map(|m| Message::Allocation(cpu_allocation::Kind::Hard, m)),
            )
        } else {
            let plugged_in = crate::backend::power_source::is_plugged_in().or_else(|| {
                self.status
                    .feature_status
                    .sleep_prevention
                    .source
                    .filter(|_| page == Page::SleepPrevention)
                    .map(|source| source == PowerSourceProfile::PluggedIn)
            });
            let settings = status_rail::saved_runtime_settings(&self.settings, plugged_in);
            status_rail::view(page, settings, &self.status, &self.power_plans).map(|panel| {
                let panel = panel.map(Message::Status);
                if page == Page::MemoryTrim {
                    let can_trim = settings.general.enabled
                        && settings.memory_trim.enabled
                        && !self.manual_trim_pending;
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
                                can_trim.then_some(Message::Trim(memory_trim::Message::TrimNow))
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
        iced::widget::responsive(|size| self.view_content_at_width(size.width)).into()
    }

    fn view_content_at_width(&self, width: f32) -> Element<'_, Message> {
        let shield = |content| {
            if self.page == Page::ProcessList && self.processes.modal_open() {
                super::modal::background(content)
            } else {
                content
            }
        };
        let (navigation_width, side_panel_width, page_padding) = shell_widths(width);
        let mut primary = column![].spacing(design::space::TIGHT).width(Fill);
        let mut utilities = column![].spacing(design::space::TIGHT).width(Fill);
        for page in navigation::rail_pages(self.settings.global().advanced.show_advanced_controls) {
            let item = super::navigation_flyout::view(
                page,
                navigation::rail_item(
                    page,
                    self.page.section_landing_page() == page,
                    navigation::Event::Select(page),
                ),
                (self.navigation.section == Some(page)).then(|| self.navigation_panel(page)),
                self.navigation.selected.is_some(),
            )
            .map(Message::Navigation);
            if navigation::is_utility(page) {
                utilities = utilities.push(item);
            } else {
                primary = primary.push(item);
            }
        }
        let search = super::navigation_flyout::search(
            button(
                container(
                    column![
                        navigation::glyph("icons/search.svg"),
                        text(t!("home.search").to_string())
                            .size(design::typography::BADGE)
                            .width(Fill)
                            .align_x(iced::Center),
                    ]
                    .spacing(design::space::TIGHT)
                    .align_x(iced::Center),
                )
                .center_x(Fill)
                .center_y(Fill),
            )
            .pointer_only()
            .width(Fill)
            .height(design::NAVIGATION_ITEM_HEIGHT)
            .padding([design::space::CONTROL as u16, design::space::TIGHT as u16])
            .selected(self.navigation_search_open, |theme, status| {
                let mut style = widgets::quiet(theme, status);
                if status == iced::widget::button::Status::Active {
                    style.background = Some(theme.palette().primary.scale_alpha(0.05).into());
                }
                style
            })
            .on_press(navigation::Event::Search)
            .into(),
        )
        .map(Message::Navigation);
        let rail = navigation::rail(search, primary.into(), utilities.into());
        let navigation = row![rail, iced::widget::rule::vertical(1)];
        let navigation = container(navigation)
            .width(navigation_width)
            .height(Fill)
            .style(widgets::navigation_surface);
        let navigation = shield(navigation.into());
        let side_panel_available = matches!(self.page, Page::ProcessList | Page::ActionLog)
            || navigation::page_feature_info_intro(self.page).is_some();
        let side_panel = (self.side_panel_open && side_panel_available)
            .then(|| self.side_panel(self.page))
            .flatten()
            .map(shield);
        const BREADCRUMB_TEXT_SIZE: u32 = design::typography::SUBTITLE;
        let mut header = row![].spacing(design::space::COMPACT).align_y(iced::Center);
        for (index, page) in self.breadcrumb.iter().copied().enumerate() {
            let mut item = row![].spacing(design::space::COMPACT).align_y(iced::Center);
            if index > 0 {
                item = item.push(navigation::glyph("icons/chevron-right.svg"));
            }
            item = if page == self.page {
                item.push(widgets::heading(page.label(), BREADCRUMB_TEXT_SIZE))
            } else {
                item.push(
                    button(text(page.label()).size(BREADCRUMB_TEXT_SIZE))
                        .padding(0)
                        .style(widgets::quiet)
                        .on_press(Message::Page(page)),
                )
            };
            header = header.push(item);
        }
        let breadcrumb =
            container(header.wrap().vertical_spacing(design::space::TIGHT)).width(Fill);
        let mut profile_controls = None;
        if self.page.supports_power_source_profiles() {
            let plugged_in = crate::backend::power_source::is_plugged_in();
            let mut tabs = row![];
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
                let selected = self.power_source == profile;
                let mut label = row![widgets::heading(
                    t!(key).to_string(),
                    design::typography::BODY
                )]
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
                        .padding([0, design::space::MEDIUM as u16])
                        .on_press(Message::PowerSource(profile))
                        .style(move |theme, status| widgets::tab_style(theme, status, selected)),
                );
            }
            profile_controls = Some(container(tabs).style(widgets::tab_bar));
        }
        let panel_button = side_panel_available.then(|| {
            button(
                row![
                    navigation::glyph("icons/panel-right-open.svg"),
                    text(t!("nav.side_panel").to_string()),
                ]
                .spacing(design::space::CONTROL)
                .height(Fill)
                .align_y(iced::Center),
            )
            .height(design::BUTTON_HEIGHT)
            .padding([0, design::space::MEDIUM as u16])
            .style(move |theme, status| {
                let mut style = if self.side_panel_open {
                    widgets::selected(theme, status)
                } else {
                    widgets::secondary_button(theme, status)
                };
                style.text_color = widgets::quiet(theme, status).text_color;
                style
            })
            .on_press(Message::ToggleSidePanel)
            .into()
        });
        let heading = page_header(
            breadcrumb.into(),
            profile_controls.map(Into::into),
            panel_button,
        );
        let content = container(super::motion::wrap(
            super::motion::wrap(
                super::scrolling::repaint_group(self.page_view()),
                true,
                super::motion::Effect::Content(self.power_source as u64),
            ),
            true,
            super::motion::Effect::Content(widgets::stable_key(&self.page)),
        ))
        .width(Fill)
        .height(Fill);
        let body = page_canvas(shield(heading), content.into());
        let panel_visible = side_panel.is_some();
        let layout = row![
            navigation,
            container(body)
                .padding([design::space::SECTION as u16, page_padding])
                .width(Fill)
                .height(Fill),
        ]
        .height(Fill)
        .into();
        let layout = side_panel_dock(layout, side_panel, side_panel_width);
        if self.navigation_search_open {
            let searching = !self.navigation_search.trim().is_empty();
            let choices = navigation_search_choices(
                &self.navigation_search,
                self.settings.global().advanced.show_advanced_controls,
                self.navigation_search_scope,
            );
            let show_results = searching || self.navigation_search_scope.0.is_some();
            let mut results = column![].spacing(design::space::TINY);
            if show_results && choices.is_empty() {
                results = results.push(
                    container(text(t!("home.no_matching_functions").to_string()))
                        .padding(design::space::MEDIUM as u16),
                );
            }
            let selected_result = choices.get(self.navigation_search_selected).copied();
            for page in choices {
                let mut label = column![text(page.label()).size(design::typography::BODY)];
                let section = page.section_landing_page();
                if section != page {
                    label = label.push(
                        text(section.label())
                            .size(design::typography::SECONDARY)
                            .style(text::secondary),
                    );
                }
                results = results.push(
                    button(
                        row![
                            navigation::icon(page, false),
                            label.width(Fill),
                            navigation::glyph("icons/chevron-right.svg"),
                        ]
                        .spacing(design::space::MEDIUM)
                        .height(Fill)
                        .align_y(iced::Center),
                    )
                    .pointer_only()
                    .width(Fill)
                    .height(48)
                    .padding([6, 12])
                    .style(move |theme, status| {
                        if Some(page) == selected_result {
                            widgets::selected(theme, status)
                        } else {
                            widgets::quiet(theme, status)
                        }
                    })
                    .on_press(Message::Page(page)),
                );
            }
            let mut input = text_input(&t!("home.search_placeholder"), &self.navigation_search)
                .id("navigation-search")
                .size(design::typography::SECTION)
                .padding([12, 0])
                .style(|theme, status| {
                    let mut style = iced::widget::text_input::default(theme, status);
                    style.background = iced::Background::Color(iced::Color::TRANSPARENT);
                    style.border = iced::Border::default();
                    style
                })
                .on_input(Message::NavigationSearch);
            if selected_result.is_some() {
                input = input.on_submit(Message::ConfirmSearch);
            }
            let scopes: Vec<_> = std::iter::once(SearchScope(None))
                .chain(
                    navigation::dashboard_sections_in_nav_order(
                        self.settings.global().advanced.show_advanced_controls,
                    )
                    .map(|section| SearchScope(Some(section.landing_page))),
                )
                .collect();
            let mut search_content = column![
                container(
                    row![
                        navigation::glyph("icons/search.svg"),
                        input,
                        button(text("Esc").size(design::typography::CAPTION))
                            .on_press(Message::CloseNavigationSearch),
                    ]
                    .spacing(design::space::MEDIUM)
                    .align_y(iced::Center)
                )
                .padding([4, 16]),
                iced::widget::rule::horizontal(1),
                container(
                    row![
                        text(t!("home.search_filter").to_string())
                            .size(design::typography::CAPTION)
                            .style(text::secondary),
                        super::select::Select::new(
                            scopes,
                            Some(self.navigation_search_scope),
                            Message::NavigationSearchScope
                        )
                        .width(168),
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                )
                .padding([6, 12]),
            ];
            if show_results {
                search_content = search_content.push(iced::widget::rule::horizontal(1)).push(
                    container(
                        super::scrolling::popup_scrollable(results).id("navigation-search-results"),
                    )
                    .max_height(320)
                    .padding(6),
                );
            }
            let search = navigation_search_panel(search_content);
            return navigation_search_overlay(layout, search);
        }
        let modal = match self.page {
            Page::InstanceBalancing => self
                .instance_balancing
                .modal(&self.settings.global().instance_balancing, &self.candidates)
                .map(|modal| modal.map(Message::InstanceBalancing)),
            Page::InstanceLimits => self
                .instance_limits
                .modal(&self.candidates)
                .map(|modal| modal.map(Message::InstanceLimits)),
            Page::KeepRunning => self
                .keep_running
                .modal(&self.candidates)
                .map(|modal| modal.map(Message::KeepRunning)),
            Page::Watchdog => self
                .watchdog
                .modal(&self.candidates)
                .map(|modal| modal.map(Message::Watchdog)),
            Page::CpuSetsSoft => self
                .soft_allocation
                .modal(&self.settings, cpu_allocation::Kind::Soft)
                .map(|m| m.map(|m| Message::Allocation(cpu_allocation::Kind::Soft, m))),
            Page::ProcessorAffinityHard => self
                .hard_allocation
                .modal(&self.settings, cpu_allocation::Kind::Hard)
                .map(|m| m.map(|m| Message::Allocation(cpu_allocation::Kind::Hard, m))),
            Page::CpuLimiter => self
                .cpu_limiter
                .modal(
                    &self.candidates,
                    self.settings.global().general.enabled,
                    self.cpu_limiter_rule_status(),
                )
                .map(|modal| modal.map(Message::CpuLimiter)),
            Page::AdvancedPowerPlanTuning => self
                .power_tuning
                .preset_modal(&self.settings.global().advanced_power_plan_tuning_presets)
                .map(|modal| modal.map(Message::PowerTuning)),
            Page::AdaptiveEngine if self.adaptive.preset_open() => Some(
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
            return centered_modal(layout, modal);
        }
        let settings_are_pending = self.pending_settings_changes();
        iced::widget::stack![
            layout,
            shield(
                container(super::motion::wrap(
                    widgets::settings_card(self.save_notice(settings_are_pending)).width(360),
                    settings_are_pending || self.power_plan_control.selected_guid.is_some(),
                    super::motion::Effect::Visible
                ))
                .padding(iced::Padding {
                    right: design::space::LARGE as f32
                        + if panel_visible {
                            side_panel_width + 1.0
                        } else {
                            0.0
                        },
                    ..iced::Padding::new(design::space::LARGE as f32)
                })
                .width(Fill)
                .height(Fill)
                .align_x(iced::Right)
                .align_y(iced::Bottom)
                .into()
            )
        ]
        .into()
    }

    fn save_notice(&self, settings: bool) -> iced::widget::Column<'static, Message> {
        let plan = self.power_plan_control.selected_guid.as_deref();
        let title = if plan.is_some() && !settings {
            "power_plan_control.save_title"
        } else {
            "unsaved.title"
        };
        let mut notice = column![widgets::heading(
            t!(title).to_string(),
            design::typography::BODY
        )];
        let ready = self.power_plan_control.pending.is_none();
        if !ready {
            notice = notice.push(text(t!("power_plan_control.switching").to_string()));
        } else if let Some(guid) = plan {
            let plan = power_plan_control::plan_name(&self.power_plans, guid);
            notice = notice.push(text(
                t!("power_plan_control.save_message", plan = plan).to_string(),
            ));
        }
        notice
            .push(
                row![
                    iced::widget::Space::new().width(Fill),
                    action_button(t!("common.discard").to_string())
                        .style(widgets::tertiary_button)
                        .on_press_maybe(ready.then_some(Message::Cancel)),
                    action_button(t!("common.save").to_string())
                        .style(widgets::primary_button)
                        .on_press_maybe(ready.then_some(Message::Save)),
                ]
                .spacing(design::space::SMALL),
            )
            .spacing(design::space::MEDIUM)
    }

    fn cpu_limiter_rule_status(&self) -> Option<&CpuLimiterSnapshot> {
        let status = &self.status.feature_status.cpu_limiter;
        let selected_is_plugged_in = self.power_source == PowerSourceProfile::PluggedIn;
        (status.source == Some(self.power_source)
            && crate::backend::power_source::is_plugged_in() == Some(selected_is_plugged_in))
        .then_some(status)
    }

    fn page_view(&self) -> Element<'_, Message> {
        let general = &self.settings.global().general;
        let toggle = |label: &'static str, value, action: fn(bool) -> Message| {
            widgets::toggle_card(label, value, Some(action))
        };
        let runtime_power_rules = || {
            let context = self.status.power_plan_rule_context.as_ref()?;
            (context.source == self.power_source).then(|| match context.source {
                PowerSourceProfile::PluggedIn => context.settings.as_ref(),
                PowerSourceProfile::OnBattery => context.settings.battery_profile(),
            })
        };
        match self.page {
            Page::SleepPrevention => {
                let status = &self.status.feature_status.sleep_prevention;
                let selected_is_plugged_in = self.power_source == PowerSourceProfile::PluggedIn;
                let status = (status.source == Some(self.power_source)
                    && crate::backend::power_source::is_plugged_in()
                        .is_none_or(|plugged_in| plugged_in == selected_is_plugged_in))
                .then_some(status);
                self.sleep_prevention
                    .view(&self.settings.sleep_prevention, status, &self.candidates)
                    .map(Message::SleepPrevention)
            }
            Page::AppSuspension => self
                .suspension
                .view(
                    &self.settings,
                    &self.status.feature_status.app_suspension,
                    &self.unavailable_candidates,
                    &self.candidates,
                )
                .map(Message::Suspension),
            Page::Watchdog => self
                .watchdog
                .view(&self.settings.global().watchdog, &self.candidates)
                .map(Message::Watchdog),
            Page::InstanceLimits => self
                .instance_limits
                .view(
                    &self.settings.global().instance_limits,
                    &self.status.feature_status.instance_limits,
                    &self.candidates,
                )
                .map(Message::InstanceLimits),
            Page::InstanceBalancing => self
                .instance_balancing
                .view(&self.settings.global().instance_balancing, &self.candidates)
                .map(Message::InstanceBalancing),
            Page::KeepRunning => self
                .keep_running
                .view(
                    &self.settings.global().keep_running,
                    &self.status.feature_status.keep_running,
                    &self.candidates,
                )
                .map(Message::KeepRunning),
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
                    &self.settings.global().advanced_power_plan_tuning_presets,
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
                    runtime_power_rules(),
                    &self.status.power_plan_status,
                )
                .map(|m| Message::PowerRules(power_rules::Kind::Time, m)),
            Page::ByCpuLoad => self
                .cpu_rules
                .view(
                    power_rules::Kind::CpuLoad,
                    &self.settings,
                    runtime_power_rules(),
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
                .view(&self.settings, &self.candidates, &self.interrupt_timing)
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

            Page::ByFocus => self
                .focus_plans
                .view(
                    process_power_plans::Kind::Focus,
                    &self.settings,
                    &self.power_plans,
                    &self.candidates,
                    runtime_power_rules(),
                    &self.status.power_plan_status,
                )
                .map(|message| {
                    Message::ProcessPowerPlans(process_power_plans::Kind::Focus, message)
                }),
            Page::ByRunningApp => self
                .running_app_plans
                .view(
                    process_power_plans::Kind::RunningApp,
                    &self.settings,
                    &self.power_plans,
                    &self.candidates,
                    runtime_power_rules(),
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
                    self.cpu_limiter_rule_status(),
                )
                .map(Message::CpuLimiter),
            Page::ProcessList => self
                .processes
                .view(
                    &self.settings,
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
                .view(self.settings.persisted(), &self.status)
                .map(Message::Home),
            Page::WinderustBehaviour
            | Page::LanguageAndAppearance
            | Page::ExperimentalFeatures
            | Page::About => self
                .preferences
                .view(
                    self.page,
                    self.settings.global(),
                    self.can_replace_settings(),
                )
                .map(Message::Preferences),
            Page::PowerPlanControl => column![
                power_plan_control::view(
                    &self.status.power_plan_status,
                    &self.power_plans,
                    self.power_plans_loaded,
                    self.power_plans_loading,
                    &self.power_plan_control,
                )
                .map(Message::PowerPlanControl),
                toggle(
                    "power_plan_control.pause_plugged",
                    general.pause_power_plan_switching_while_plugged_in,
                    Message::PausePowerPlans
                ),
                toggle(
                    "power_plan_control.restore_previous_plan",
                    general.restore_previous_power_plan,
                    Message::RestorePreviousPowerPlan
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

fn notifications_required(settings: &crate::config::Settings) -> bool {
    settings.general.enabled
        && (settings.watchdog.notifications_required()
            || settings.instance_limits.notifications_required())
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

fn page_needs_power_plans(page: Page, process_rulesets_open: bool) -> bool {
    (page == Page::ProcessList && process_rulesets_open)
        || page.section_landing_page() == Page::PowerPlanControl
}

fn combine_warnings(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(mut first), Some(second)) => {
            first.push('\n');
            first.push_str(&second);
            Some(first)
        }
        (first, second) => first.or(second),
    }
}

fn power_plan_switch_error(error: RuntimeCommandError, warning: Option<&str>) -> String {
    let key = match error {
        RuntimeCommandError::PowerPlanUnavailable => "common.selected_plan_unavailable",
        RuntimeCommandError::PowerPlan(ManualPowerPlanError::ManagedByAdaptive) => {
            "power_plan_control.switch_managed_active"
        }
        RuntimeCommandError::PowerPlan(ManualPowerPlanError::ManagedTarget) => {
            "power_plan_control.switch_managed_target"
        }
        RuntimeCommandError::PowerPlan(ManualPowerPlanError::ActiveRecoveryPending(detail)) => {
            crate::backend::diagnostics::error(&format!(
                "Manual power plan recovery update is unconfirmed: {detail}"
            ));
            "power_plan_control.switch_active_recovery_pending"
        }
        RuntimeCommandError::RuntimeStopped | RuntimeCommandError::WorkerExited => {
            "power_plan_control.switch_runtime_unavailable"
        }
        RuntimeCommandError::QueueFull => "power_plan_control.switch_busy",
        RuntimeCommandError::InvalidRequest(detail)
        | RuntimeCommandError::CommandFailed(detail)
        | RuntimeCommandError::PowerPlan(ManualPowerPlanError::SwitchFailed(detail)) => {
            crate::backend::diagnostics::error(&format!("Power plan switch failed: {detail}"));
            "power_plan_control.switch_failed"
        }
    };
    let error = t!(key);
    if let Some(warning) = warning {
        format!("{warning}\n{error}")
    } else {
        error.into_owned()
    }
}

fn sync_active_power_plan(
    plans: &mut [crate::power::PowerPlan],
    previous_guid: Option<&str>,
    current_guid: Option<&str>,
) -> bool {
    let Some(current_guid) = current_guid else {
        return false;
    };
    // An unchanged cached status must not overwrite a freshly enumerated catalog.
    if previous_guid.is_some_and(|previous| previous.eq_ignore_ascii_case(current_guid)) {
        return false;
    }
    for plan in plans {
        plan.active = plan.guid.eq_ignore_ascii_case(current_guid);
    }
    true
}

fn accept_power_plan_catalog(
    plans: &mut Vec<crate::power::PowerPlan>,
    result: Result<Vec<crate::power::PowerPlan>, String>,
    reload_pending: &mut bool,
) -> Result<bool, String> {
    if std::mem::take(reload_pending) {
        return Ok(false);
    }
    *plans = result?;
    Ok(true)
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
    fn search_overlay_blocks_background_hover_scroll_and_clicks_but_keeps_search_editable() {
        use super::*;
        use iced::advanced::{layout, widget::Tree, Layout, Shell};
        use iced::keyboard::{key::Named, Key};
        use iced::{mouse, Event, Point, Rectangle, Size};
        let renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let bounds = Rectangle::with_size(Size::new(900.0, 620.0));
        let background = iced::widget::mouse_area(
            button("Home")
                .width(Fill)
                .height(Fill)
                .on_press(Message::Page(Page::Home)),
        )
        .on_enter(Message::Page(Page::About))
        .on_scroll(|_| Message::Page(Page::ActionLog));
        let mut view = navigation_search_overlay(
            background.into(),
            navigation_search_panel(text_input("Search", "").on_input(Message::NavigationSearch)),
        );
        let mut tree = Tree::new(&view);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, bounds.size()),
        );
        let outside = Point::new(20.0, 40.0);
        for event in [
            Event::Mouse(mouse::Event::CursorMoved { position: outside }),
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
            }),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)),
        ] {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(outside),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &bounds,
            );
            if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_))) {
                assert!(shell.is_event_captured());
            }
            assert!(messages
                .iter()
                .all(|message| matches!(message, Message::CloseNavigationSearch)));
        }
        let input = node.children()[2].children()[0].bounds().center();
        let mut messages = Vec::new();
        let escape = Key::Named(Named::Escape);
        for event in [
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Character("x".into()),
                modified_key: iced::keyboard::Key::Character("x".into()),
                physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyX),
                location: iced::keyboard::Location::Standard,
                modifiers: iced::keyboard::Modifiers::default(),
                text: Some("x".into()),
                repeat: false,
            }),
            Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: escape.clone(),
                modified_key: escape,
                physical_key: iced::keyboard::key::Physical::Code(
                    iced::keyboard::key::Code::Escape,
                ),
                location: iced::keyboard::Location::Standard,
                modifiers: iced::keyboard::Modifiers::empty(),
                text: None,
                repeat: false,
            }),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(input),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &bounds,
            );
        }
        assert!(
            matches!(messages.as_slice(), [Message::NavigationSearch(text), Message::CloseNavigationSearch] if text == "x")
        );
    }

    #[test]
    fn dock_reserves_content_width_and_keeps_header_alignment() {
        use iced::advanced::{layout, widget::Tree};
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        for width in [780.0, 1440.0, 1800.0] {
            let size = iced::Size::new(width, 600.0);
            let mut expected = None;
            for profiles in [false, true] {
                for available in [false, true] {
                    for open in [false, true] {
                        let heading = page_header(
                            text("Home > Section > Feature").into(),
                            profiles
                                .then(|| button(text("Plugged In / On Battery")).height(32).into()),
                            available.then(|| button(text("Side panel")).height(32).into()),
                        );
                        let workspace = page_canvas(
                            heading,
                            iced::widget::Space::new().width(Fill).height(Fill).into(),
                        );
                        let panel = (available && open)
                            .then(|| iced::widget::Space::new().height(Fill).into());
                        let mut view = side_panel_dock(workspace, panel, 320.0);
                        let mut tree = Tree::new(&view);
                        let node = view.as_widget_mut().layout(
                            &mut tree,
                            &renderer,
                            &layout::Limits::new(iced::Size::ZERO, size),
                        );
                        let available_width = width - if available && open { 321.0 } else { 0.0 };
                        assert_eq!(node.children()[0].size().width, available_width);
                        if available && open {
                            assert_eq!(node.children()[1].bounds().x, available_width);
                        }
                        let workspace = &node.children()[0].children()[0];
                        let canvas = &workspace.children()[0];
                        let column = &canvas.children()[0];
                        let mut bounds = column.children()[1].bounds();
                        assert_eq!(column.children()[0].bounds().x, bounds.x);
                        bounds.x += canvas.bounds().x;
                        let content_width = available_width.min(design::CONTENT_WIDTH as f32);
                        assert_eq!(bounds.x, (available_width - content_width) / 2.0);
                        assert_eq!(bounds.width, content_width);
                        if let Some(expected) = expected {
                            assert_eq!(bounds.y, expected);
                        } else {
                            expected = Some(bounds.y);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn dock_and_page_controls_remain_interactive_together() {
        use iced::advanced::{layout, widget::Tree, Layout, Shell};
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let workspace: Element<'_, Message> = button(text("Background action"))
            .width(Fill)
            .height(Fill)
            .on_press(Message::Save)
            .into();
        let panel = button(text("Panel action"))
            .width(Fill)
            .height(Fill)
            .on_press(Message::Cancel)
            .into();
        let mut view = side_panel_dock(workspace, Some(panel), 320.0);
        let mut tree = Tree::new(&view);
        let size = iced::Size::new(900.0, 620.0);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, size),
        );
        for (cursor, outside) in [
            (iced::Point::new(100.0, 200.0), true),
            (iced::Point::new(800.0, 200.0), false),
        ] {
            let mut messages = Vec::new();
            for event in [
                iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
                iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
            ] {
                view.as_widget_mut().update(
                    &mut tree,
                    &iced::Event::Mouse(event),
                    Layout::new(&node),
                    iced::mouse::Cursor::Available(cursor),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut Shell::new(&mut messages),
                    &node.bounds(),
                );
            }
            assert_eq!(messages.len(), 1);
            assert!(matches!(messages[0], Message::Save) == outside);
            assert!(matches!(messages[0], Message::Cancel) != outside);
        }
    }

    #[test]
    fn feature_info_fits_the_sidebar_without_changing_its_bounds() {
        use super::*;
        use iced::advanced::{layout, widget::Tree};

        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        assert!(feature_info_sidebar(Page::Home, SidePanelTab::Info, || None).is_none());
        for page in
            std::iter::once(Page::SettingsHome).chain(navigation::children(Page::SettingsHome))
        {
            assert!(feature_info_sidebar(page, SidePanelTab::Info, || None).is_none());
        }
        for width in [216.0, 296.0] {
            for height in [500.0, 1000.0] {
                let size = iced::Size::new(width, height);
                for feature_page in [
                    Page::CpuLimiter,
                    Page::AdaptiveEngine,
                    Page::AdvancedPowerPlanTuning,
                ] {
                    for &selected in SidePanelTab::for_page(feature_page) {
                        let panel = iced::widget::Space::new().width(Fill).height(Fill).into();
                        let mut built_panel = false;
                        let mut view = feature_info_sidebar(feature_page, selected, || {
                            built_panel = true;
                            Some(panel)
                        })
                        .expect("feature page has information");
                        assert_eq!(built_panel, selected != SidePanelTab::Info);
                        let mut tree = Tree::new(&view);
                        let node = view.as_widget_mut().layout(
                            &mut tree,
                            &renderer,
                            &layout::Limits::new(iced::Size::ZERO, size),
                        );
                        assert_eq!(node.size(), size);
                        let tabs = &node.children()[0];
                        assert!(tabs.children()[0].size().width <= width);
                        let body = &node.children()[1];
                        assert!(body.size().height >= height - 60.0);
                    }
                }
                let mut view =
                    feature_info_sidebar(Page::TimerResolution, SidePanelTab::Status, || None)
                        .expect("feature information is available without a runtime panel");
                let mut tree = Tree::new(&view);
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, size),
                );
                assert_eq!(node.size(), size);
            }
        }
    }

    #[test]
    fn search_navigation_stays_bounded_and_tracks_selected_result() {
        assert_eq!(search_selection(0, 0, true), 0);
        assert_eq!(search_selection(0, 3, false), 0);
        assert_eq!(search_selection(0, 3, true), 1);
        assert_eq!(search_selection(2, 3, true), 2);
        assert_eq!(search_selection(8, 2, true), 1);
        let pages = navigation_search_choices("process", false, SearchScope(None));
        assert!(pages.len() > 1);
        let selected = search_selection(0, pages.len(), true);
        assert_eq!(pages.get(selected), pages.get(1));
        assert_ne!(pages.get(selected), pages.first());
    }

    #[test]
    fn dock_preserves_page_scroll_and_repaints_cleanly() {
        use iced::advanced::{layout, renderer::Renderer as _, widget::Tree, Layout};
        struct ReadScroll(f32);
        impl iced::advanced::widget::Operation<()> for ReadScroll {
            fn traverse(
                &mut self,
                operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<()>),
            ) {
                operate(self);
            }
            fn scrollable(
                &mut self,
                id: Option<&iced::advanced::widget::Id>,
                _bounds: iced::Rectangle,
                _content_bounds: iced::Rectangle,
                translation: iced::Vector,
                _state: &mut dyn iced::advanced::widget::operation::Scrollable,
            ) {
                if id == Some(&iced::advanced::widget::Id::from("workspace")) {
                    self.0 = translation.y;
                }
            }
        }
        let mut palette = design::palette(false, None);
        palette.background = iced::Color::BLACK;
        let theme = Theme::custom_with_fn("AMOLED test", palette, design::extended_palette);
        let size = iced::Size::new(900, 620);
        let bounds =
            iced::Rectangle::with_size(iced::Size::new(size.width as f32, size.height as f32));
        let viewport = iced::advanced::graphics::Viewport::with_physical_size(size, 1.0);
        let mut renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let mut pixels = tiny_skia::Pixmap::new(size.width, size.height).unwrap();
        let mut mask = tiny_skia::Mask::new(size.width, size.height).unwrap();
        let mut previous = Vec::new();
        let mut tree = Tree::empty();
        for (step, open) in [false, true, true, true, false, true]
            .into_iter()
            .enumerate()
        {
            let workspace: Element<'_, Message> = scrollable(
                container(text(format!("Page {step}")))
                    .width(Fill)
                    .height(2000)
                    .style(widgets::surface),
            )
            .id("workspace")
            .width(Fill)
            .height(Fill)
            .into();
            let panel = open.then(|| {
                scrollable(
                    iced::widget::column(
                        (0..step * 5).map(|item| text(format!("Panel {step}, item {item}")).into()),
                    )
                    .spacing(12),
                )
                .height(Fill)
                .into()
            });
            let mut view = side_panel_dock(workspace, panel, design::SIDE_PANEL_WIDTH);
            tree.diff(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, bounds.size()),
            );
            if step == 0 {
                let mut scroll = iced::advanced::widget::operation::scrollable::scroll_to::<()>(
                    "workspace".into(),
                    iced::widget::scrollable::AbsoluteOffset {
                        x: None,
                        y: Some(250.0),
                    },
                );
                view.as_widget_mut()
                    .operate(&mut tree, Layout::new(&node), &renderer, &mut scroll);
            }
            let mut offset = ReadScroll(0.0);
            view.as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut offset);
            assert_eq!(
                offset.0.abs(),
                250.0,
                "Dock reset the page scroll at step {step}"
            );
            renderer.reset(bounds);
            view.as_widget().draw(
                &tree,
                &mut renderer,
                &theme,
                &iced::advanced::renderer::Style {
                    text_color: theme.palette().text,
                },
                Layout::new(&node),
                iced::mouse::Cursor::Unavailable,
                &bounds,
            );
            let damage = if previous.is_empty() {
                vec![bounds]
            } else {
                iced::advanced::graphics::damage::group(
                    iced::advanced::graphics::damage::diff(
                        &previous,
                        renderer.layers(),
                        |layer| vec![layer.bounds],
                        iced_tiny_skia::Layer::damage,
                    ),
                    bounds,
                )
            };
            renderer.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &viewport,
                &damage,
                theme.palette().background,
            );
            let mut full = tiny_skia::Pixmap::new(size.width, size.height).unwrap();
            renderer.draw(
                &mut full.as_mut(),
                &mut mask,
                &viewport,
                &[bounds],
                theme.palette().background,
            );
            assert!(
                pixels.data() == full.data(),
                "Dock left stale pixels at step {step}"
            );
            previous = renderer.layers().to_vec();
        }
    }

    #[test]
    fn search_panel_partial_repaints_match_full_frames() {
        use iced::advanced::{layout, renderer::Renderer as _, widget::Tree, Layout};
        let theme = Theme::custom_with_fn(
            "test",
            design::palette(false, None),
            design::extended_palette,
        );
        let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 400.0));
        let viewport =
            iced::advanced::graphics::Viewport::with_physical_size(iced::Size::new(800, 400), 1.0);
        let mut renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let mut pixels = tiny_skia::Pixmap::new(800, 400).unwrap();
        let mut mask = tiny_skia::Mask::new(800, 400).unwrap();
        let mut previous = Vec::new();
        let mut tree = Tree::empty();
        for query in ["", "c", "cp", "cpu", ""] {
            let panel = navigation_search_panel(iced::widget::column![
                text_input("Search", query).padding(12),
                iced::widget::rule::horizontal(1),
                container(
                    iced::widget::row![
                        text("Filter")
                            .size(design::typography::CAPTION)
                            .style(text::secondary),
                        super::super::select::Select::new(
                            vec![SearchScope(None), SearchScope(Some(Page::PowerPlanControl))],
                            Some(SearchScope(None)),
                            Message::NavigationSearchScope,
                        )
                        .width(168),
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                )
                .padding(12),
            ]);
            let mut view: Element<'_, Message> = container(panel).padding(60).into();
            tree.diff(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, bounds.size()),
            );
            renderer.reset(bounds);
            view.as_widget().draw(
                &tree,
                &mut renderer,
                &theme,
                &iced::advanced::renderer::Style {
                    text_color: theme.palette().text,
                },
                Layout::new(&node),
                iced::mouse::Cursor::Unavailable,
                &bounds,
            );
            let damage = if previous.is_empty() {
                vec![bounds]
            } else {
                iced::advanced::graphics::damage::group(
                    iced::advanced::graphics::damage::diff(
                        &previous,
                        renderer.layers(),
                        |layer| vec![layer.bounds],
                        iced_tiny_skia::Layer::damage,
                    ),
                    bounds,
                )
            };
            renderer.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &viewport,
                &damage,
                theme.palette().background,
            );
            let mut full = tiny_skia::Pixmap::new(800, 400).unwrap();
            renderer.draw(
                &mut full.as_mut(),
                &mut mask,
                &viewport,
                &[bounds],
                theme.palette().background,
            );
            assert!(
                pixels.data() == full.data(),
                "Search panel accumulated stale pixels for query {query:?}"
            );
            previous = renderer.layers().to_vec();
        }
    }

    #[test]
    fn search_scopes_filter_results_and_keep_the_empty_all_dialog_compact() {
        assert!(navigation_search_choices("", true, SearchScope(None)).is_empty());
        let scope = SearchScope(Some(Page::PowerPlanControl));
        let pages = navigation_search_choices("", false, scope);
        assert!(pages.contains(&Page::PowerPlanControl));
        assert!(pages.contains(&Page::ByActivity));
        assert!(pages
            .iter()
            .all(|page| page.section_landing_page() == Page::PowerPlanControl));
        assert!(navigation_search_choices("memory", false, scope).is_empty());
        assert!(!navigation_search_choices("memory", false, SearchScope(None)).is_empty());
    }

    #[test]
    fn search_returns_matching_section_landing_before_children() {
        use Page::PowerPlanControl;
        for scope in [SearchScope(None), SearchScope(Some(PowerPlanControl))] {
            let pages = navigation_search_choices("restore previous plan", false, scope);
            assert_eq!(pages.first(), Some(&PowerPlanControl));
            assert!(pages.contains(&Page::ByFocus));
            assert!(!pages[1..].contains(&PowerPlanControl));
        }
    }

    #[test]
    fn exit_guard_keeps_pending_completions_but_rejects_new_work() {
        assert!(
            Message::Processes(process_list::Message::Loaded(Err("fixture".into())))
                .allowed_during_exit()
        );
        assert!(
            Message::Processes(process_list::Message::IconsLoaded(Err("fixture".into())))
                .allowed_during_exit()
        );
        let mut editor = settings_pages::Editor::default();
        let mut settings = crate::config::Settings::default();
        let channel = settings.general.update_channel;
        let check = editor.begin_check(channel, false).unwrap();
        let completed = Message::Preferences(settings_pages::Message::Checked(check, Err(())));
        assert!(completed.allowed_during_exit());
        if let Message::Preferences(message) = completed {
            editor.update(&mut settings, message);
        }
        assert!(editor.begin_check(channel, false).is_some());
        assert!(Message::ShutdownFinished(Err("fixture".into())).allowed_during_exit());
        for message in [
            Message::Processes(process_list::Message::Refresh),
            Message::Processes(process_list::Message::ConfirmStop),
            Message::Preferences(settings_pages::Message::AddColor),
            Message::Preferences(settings_pages::Message::Check),
            Message::Preferences(settings_pages::Message::ColorPicked(iced::Color::WHITE)),
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
    fn shell_preserves_content_space_at_scaled_desktop_sizes() {
        // Physical widths become logical widths before they reach the shell.
        for (physical_width, scale) in [
            (1920.0, 1.0),
            (1920.0, 1.25),
            (1920.0, 1.5),
            (1920.0, 2.0),
            (2560.0, 1.5),
        ] {
            let width = physical_width / scale;
            let (navigation, panel, padding) = shell_widths(width);
            assert_eq!(navigation, design::NAVIGATION_RAIL_WIDTH + 1.0);
            assert!(panel <= design::SIDE_PANEL_WIDTH);
            let content = width - navigation - 2.0 * padding as f32;
            assert!(content >= 800.0, "{width}: {content}");
        }
        assert_eq!(
            shell_widths(1920.0),
            (
                design::NAVIGATION_RAIL_WIDTH + 1.0,
                design::SIDE_PANEL_WIDTH,
                design::space::WIDE as u16
            )
        );
        let (navigation, _, padding) = shell_widths(900.0);
        assert!(900.0 - navigation - 2.0 * padding as f32 >= 780.0);
    }

    #[test]
    fn power_plan_catalog_is_requested_on_pages_that_use_it() {
        assert!(!page_needs_power_plans(Page::ProcessList, false));
        assert!(page_needs_power_plans(Page::ProcessList, true));
        for process_rulesets_open in [false, true] {
            assert!(page_needs_power_plans(
                Page::PowerPlanControl,
                process_rulesets_open
            ));
            for &page in Page::PowerPlanControl.child_pages().unwrap() {
                assert!(page_needs_power_plans(page, process_rulesets_open));
            }
            assert!(!page_needs_power_plans(Page::Home, process_rulesets_open));
        }
    }

    #[test]
    fn runtime_plan_changes_refresh_catalog_markers_without_overwriting_fresh_queries() {
        let mut plans = vec![
            crate::power::PowerPlan {
                guid: "balanced".into(),
                name: "Balanced".into(),
                active: true,
                managed_adaptive: false,
            },
            crate::power::PowerPlan {
                guid: "ultimate".into(),
                name: "Ultimate".into(),
                active: false,
                managed_adaptive: false,
            },
        ];
        assert!(!sync_active_power_plan(&mut plans, None, None));
        assert!(plans[0].active);

        assert!(sync_active_power_plan(
            &mut plans,
            Some("balanced"),
            Some("ULTIMATE")
        ));
        assert!(!plans[0].active);
        assert!(plans[1].active);

        // A later native enumeration may be newer than the cached runtime GUID.
        plans[0].active = true;
        plans[1].active = false;
        assert!(!sync_active_power_plan(
            &mut plans,
            Some("ULTIMATE"),
            Some("ultimate")
        ));
        assert!(!sync_active_power_plan(&mut plans, Some("ULTIMATE"), None));
        assert!(plans[0].active);
        assert!(!plans[1].active);
        assert_eq!(plans[0].name, "Balanced");
        assert_eq!(plans[1].name, "Ultimate");
    }

    #[test]
    fn catalog_completion_after_a_runtime_switch_requires_a_fresh_query() {
        let mut balanced = crate::power::PowerPlan {
            guid: "balanced".into(),
            name: "Balanced".into(),
            active: true,
            managed_adaptive: false,
        };
        let mut plans = vec![balanced.clone()];
        let mut reload_pending =
            sync_active_power_plan(&mut plans, Some("balanced"), Some("adaptive"));
        assert!(!accept_power_plan_catalog(
            &mut plans,
            Ok(vec![balanced.clone()]),
            &mut reload_pending,
        )
        .unwrap());
        assert!(!plans[0].active);
        assert!(!reload_pending);

        let adaptive = crate::power::PowerPlan {
            guid: "adaptive".into(),
            name: "Winderust Adaptive".into(),
            active: true,
            managed_adaptive: true,
        };
        balanced.active = false;
        assert!(accept_power_plan_catalog(
            &mut plans,
            Ok(vec![balanced, adaptive]),
            &mut reload_pending,
        )
        .unwrap());
        assert!(!sync_active_power_plan(
            &mut plans,
            Some("adaptive"),
            Some("ADAPTIVE")
        ));
        assert!(plans[1].active && plans[1].managed_adaptive);

        reload_pending |= sync_active_power_plan(&mut plans, Some("adaptive"), Some("balanced"));
        reload_pending |= sync_active_power_plan(&mut plans, Some("balanced"), Some("adaptive"));
        assert!(
            !accept_power_plan_catalog(&mut plans, Ok(Vec::new()), &mut reload_pending).unwrap()
        );
        assert_eq!(plans.len(), 2);
        assert!(plans[1].active);
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
