//! Real resident message-loop assertions. No failure unwinds through a Win32 callback.
use super::super::{enqueue_wake, Intent, Message, Page, WinderustApp};
use crate::ui::{app_picker, design, home, process_list};
use crate::{
    backend::{diagnostics, window_visibility},
    platform::windows::resident_shell::{
        close_ui_windows, ui_pointer_capture_active, WindowVisibility,
    },
};
use std::{
    cell::RefCell,
    rc::Weak,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    System::SystemServices::MK_LBUTTON,
    UI::WindowsAndMessaging::{GetClientRect, PostMessageW, WM_LBUTTONDOWN, WM_LBUTTONUP},
};
use windui::{
    core::{EventCtx, NodeId, Tree},
    event::{MouseButton, PointerEvent, PointerKind},
    geometry::Point,
    prelude::{App, Sender},
};

const DEADLINE: Duration = Duration::from_secs(25);
const REOPEN_CYCLES: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Start,
    ZeroWindowWake,
    Visible,
    Absent,
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerPhase {
    NavigateAbout,
    VerifyAboutCapture,
    VerifyAboutRefresh,
    VerifyAbout,
    VerifyExitCapture,
    VerifyExitRefresh,
    VerifyModal,
    VerifyBlockedNavigation,
    VerifyCancel,
    VerifyHome,
    VerifyDefaultStatus,
    VerifyStatusOpen,
    VerifyStatusClosed,
    VerifyHeaderStatusClosed,
    VerifyHeaderStatusOpen,
    VerifyFinalStatusClosed,
    VerifyFeatureInfoOpen,
    VerifyFeatureInfoClosed,
    VerifyReturnHome,
    Done,
}

pub(in crate::ui::app) struct LifecycleDriver {
    background: Option<BackgroundDriver>,
    started: Instant,
    phase: Phase,
    failures: Vec<String>,
    deadline_reached: bool,
    cycles: u8,
    expected_navigation: bool,
    zero_window_baseline: u64,
    zero_window_delivered: bool,
    worker: Option<JoinHandle<Result<(), String>>>,
    late_wake_baseline: Option<u64>,
    late_wake_during_cleanup: bool,
    coalescing_probe: Option<WakeCoalescingProbe>,
    exit_cancel_verified: bool,
    edits_rejected: bool,
    pointer_phase: PointerPhase,
    pressed: Option<NativePress>,
    resources: Option<ResourceProbe>,
    resource_checks: u8,
    stale_completion_checks: u8,
}

struct ResourceProbe {
    generation: u64,
    population: process_list::Population,
    state: process_list::SmokeResourceState,
    paused: bool,
    absent_verified: bool,
}

struct NativePress {
    node: NodeId,
    hwnd: usize,
    position: Point,
}

struct BackgroundDriver {
    duration: Duration,
    shutdown_requested: bool,
}

struct WakeCoalescingProbe {
    sender: Sender<Intent>,
    pending: AtomicBool,
    received: mpsc::Receiver<Intent>,
}

impl LifecycleDriver {
    pub(in crate::ui::app) fn requested() -> Option<Self> {
        let background = super::background_duration().map(|duration| BackgroundDriver {
            duration,
            shutdown_requested: false,
        });
        (super::lifecycle_requested() || background.is_some()).then(|| Self {
            background,
            started: Instant::now(),
            phase: Phase::Start,
            failures: Vec::new(),
            deadline_reached: false,
            cycles: 0,
            expected_navigation: false,
            zero_window_baseline: 0,
            zero_window_delivered: false,
            worker: None,
            late_wake_baseline: None,
            late_wake_during_cleanup: false,
            coalescing_probe: None,
            exit_cancel_verified: false,
            edits_rejected: false,
            pointer_phase: PointerPhase::NavigateAbout,
            pressed: None,
            resources: None,
            resource_checks: 0,
            stale_completion_checks: 0,
        })
    }

    pub(super) fn tick(&mut self, app: &mut WinderustApp, ctx: &mut EventCtx) {
        if let Some(background) = &mut self.background {
            if self.started.elapsed() >= background.duration && !app.exiting {
                background.shutdown_requested = true;
                deliver(app, ctx, Message::DiscardAndClose);
            }
            return;
        }
        if !self.deadline_reached && self.started.elapsed() > DEADLINE {
            self.deadline_reached = true;
            self.failures.push(format!(
                "Lifecycle smoke timed out in {:?} (pointer phase {:?}).",
                self.phase, self.pointer_phase
            ));
            self.cleanup(app, ctx);
        }
        if let Err(error) = self.advance(app, ctx) {
            self.failures.push(error);
            self.cleanup(app, ctx);
        }
    }

    fn cleanup(&mut self, app: &mut WinderustApp, ctx: &mut EventCtx) {
        self.phase = Phase::Cleanup;
        if !app.exiting {
            deliver(app, ctx, Message::DiscardAndClose);
        }
    }

    fn advance(&mut self, app: &mut WinderustApp, ctx: &mut EventCtx) -> Result<(), String> {
        if self.phase == Phase::Cleanup {
            if self
                .late_wake_baseline
                .is_some_and(|baseline| app.wake_count > baseline)
                && app.exiting
                && !app.ready
            {
                self.late_wake_during_cleanup = true;
            }
            return Ok(());
        }
        let Some(native) = app.native.as_ref() else {
            return Ok(());
        };
        let visibility = native.visibility().map_err(|error| error.to_string())?;
        match self.phase {
            Phase::Start => {
                require(
                    visibility == WindowVisibility::Absent && window_visibility::is_hidden(),
                    "Resident startup did not remain alive with zero UI windows.",
                )?;
                require(
                    !app.settings.global().general.enabled,
                    "Smoke runtime enabled automation.",
                )?;
                self.expected_navigation = !app.settings.persisted().general.navigation_collapsed;
                app.settings.edit_global(|settings| {
                    settings.general.navigation_collapsed = self.expected_navigation;
                });
                self.verify_draft(app)?;
                self.zero_window_baseline = app.wake_count;
                let sender = app.sender();
                self.worker = Some(
                    std::thread::Builder::new()
                        .name("resident-smoke-wake".into())
                        .spawn(move || {
                            sender
                                .send(Intent {
                                    message: Message::Wake,
                                    refresh: false,
                                })
                                .map_err(|_| "Zero-window wake channel disconnected.".to_owned())
                        })
                        .map_err(|error| error.to_string())?,
                );
                self.phase = Phase::ZeroWindowWake;
            }
            Phase::ZeroWindowWake => {
                if self
                    .worker
                    .as_ref()
                    .is_some_and(|worker| !worker.is_finished())
                {
                    return Ok(());
                }
                if let Some(worker) = self.worker.take() {
                    worker
                        .join()
                        .map_err(|_| "Zero-window wake worker panicked.".to_owned())??;
                }
                if app.wake_count == self.zero_window_baseline {
                    return Ok(());
                }
                require(
                    visibility == WindowVisibility::Absent,
                    "An application wake unexpectedly created a UI window.",
                )?;
                self.zero_window_delivered = true;
                app.open(ctx);
                self.phase = Phase::Visible;
            }
            Phase::Visible => {
                if visibility != WindowVisibility::Visible {
                    return Ok(());
                }
                require(
                    !window_visibility::is_hidden(),
                    "Reopened visible window retained background visibility policy.",
                )?;
                self.verify_draft(app)?;
                // The main window's own interval exercises its real tree before
                // this resident driver destroys it. App-level timers have no UI tree.
                if self.pointer_phase != PointerPhase::Done {
                    return Ok(());
                }
                require(
                    app.status_collapsed,
                    "The collapsed side-panel preference changed across window recreation.",
                )?;
                if let Some(probe) = self.resources.take() {
                    require(
                        probe.absent_verified,
                        "Resource smoke reopened before checking the absent window.",
                    )?;
                    self.check_stale_completions(app, ctx, &probe)?;
                    self.resource_checks += 1;
                }
                if self.cycles < REOPEN_CYCLES {
                    if app.sampling || app.catalog_loading {
                        return Ok(());
                    }
                    self.resources = Some(self.warm_resources(app)?);
                    let count = close_ui_windows().map_err(|error| error.to_string())?;
                    require(
                        count == 1,
                        "Expected one keyed UI window before native destruction.",
                    )?;
                    self.phase = Phase::Absent;
                } else {
                    self.check_exit(app, ctx)?;
                }
            }
            Phase::Absent => {
                if visibility != WindowVisibility::Absent {
                    return Ok(());
                }
                require(
                    window_visibility::is_hidden(),
                    "Destroyed UI window did not enable background visibility policy.",
                )?;
                self.verify_draft(app)?;
                let mut probe = self.resources.take().ok_or_else(|| {
                    "Resource smoke destroyed a window without warmed caches.".to_owned()
                })?;
                let mut expected = probe.state.clone();
                expected.native_rows = 0;
                if !probe.paused {
                    expected.population = 0;
                    expected.icons = 0;
                }
                require(
                    app.ui_generation != probe.generation
                        && app.processes.smoke_resources() == expected
                        && app.candidates.is_empty()
                        && app.unavailable_candidates.is_empty()
                        && app.view.candidate_catalog.get().is_empty(),
                    "Window destruction retained reconstructable caches or changed a paused snapshot/preferences.",
                )?;
                self.check_stale_completions(app, ctx, &probe)?;
                probe.absent_verified = true;
                self.resources = Some(probe);
                self.cycles += 1;
                app.open(ctx);
                self.phase = Phase::Visible;
            }
            Phase::Cleanup => {}
        }
        Ok(())
    }

    fn warm_resources(&self, app: &mut WinderustApp) -> Result<ResourceProbe, String> {
        // Populate only synthetic targets. Dropping the refresh task leaves a
        // pending query flag without dispatching process enumeration or actions.
        app.processes.smoke_population();
        for message in [
            process_list::Message::Search("Example".into()),
            process_list::Message::Group(false),
            process_list::Message::Sort(process_list::Sort::Memory),
            process_list::Message::ColumnWidth(process_list::Sort::Name, 267.0),
        ] {
            let _ = app
                .processes
                .update(message, &mut app.settings, &app.runtime);
        }
        let paused = self.cycles == 1;
        app.processes.population_paused = paused;
        app.processes.refresh_view();
        let _ = app.processes.update(
            process_list::Message::Refresh,
            &mut app.settings,
            &app.runtime,
        );
        app.candidates = app_picker::smoke_candidates();
        app.view.candidate_catalog.set(app.candidates.clone());
        app.unavailable_candidates = vec!["synthetic unavailable app".into()];
        let state = app.processes.smoke_resources();
        require(
            state.population == 24
                && state.icons == 24
                && state.native_rows == 24
                && state.refreshing
                && app.candidates.len() == 4
                && app.view.candidate_catalog.get().len() == 4,
            "Resource smoke did not warm synthetic population, icon, row and picker caches.",
        )?;
        Ok(ResourceProbe {
            generation: app.ui_generation,
            population: app.processes.smoke_population_result(),
            state,
            paused,
            absent_verified: false,
        })
    }

    fn check_stale_completions(
        &mut self,
        app: &mut WinderustApp,
        ctx: &mut EventCtx,
        probe: &ResourceProbe,
    ) -> Result<(), String> {
        require(
            probe.generation != app.ui_generation,
            "Resource smoke completion did not originate from a destroyed window.",
        )?;
        let history = app.home.smoke_history();
        let error = app.error_message.clone();
        let mut state = app.processes.smoke_resources();
        state.refreshing = false;
        app.sampling = true;
        app.catalog_loading = true;
        for message in [
            Message::Sample(probe.generation, Ok(home::Sample::smoke_sample())),
            Message::Catalog(probe.generation, Ok(app_picker::smoke_candidates())),
            Message::ProcessListLoaded(probe.generation, Ok(probe.population.clone())),
            Message::Sample(probe.generation, Err("stale sample failure".into())),
            Message::Catalog(probe.generation, Err("stale catalog failure".into())),
            Message::ProcessListLoaded(probe.generation, Err("stale population failure".into())),
        ] {
            deliver(app, ctx, message);
        }
        require(
            !app.sampling
                && !app.catalog_loading
                && app.processes.smoke_resources() == state
                && app.candidates.is_empty()
                && app.view.candidate_catalog.get().is_empty()
                && app.home.smoke_history() == history
                && app.error_message == error,
            "A stale completion refilled caches/history, changed preferences, retained pending work or showed an error.",
        )?;
        self.verify_draft(app)?;
        self.stale_completion_checks += 1;
        Ok(())
    }

    fn verify_draft(&self, app: &WinderustApp) -> Result<(), String> {
        require(
            app.settings.has_unsaved_changes()
                && app.settings.global().general.navigation_collapsed == self.expected_navigation
                && app.settings.persisted().general.navigation_collapsed
                    != self.expected_navigation
                && app.settings.global().general.hide_to_tray
                && !app.settings.global().general.enabled,
            "The in-memory draft changed or was persisted across window recreation.",
        )
    }

    fn check_pointer_input(
        &mut self,
        app: &WinderustApp,
        ctx: &mut EventCtx,
    ) -> Result<(), String> {
        if self.background.is_some() || self.phase != Phase::Visible {
            return Ok(());
        }
        let tree = ctx.tree_mut();
        match self.pointer_phase {
            PointerPhase::NavigateAbout => {
                require(
                    app.page == Page::Home && !app.closing,
                    "Pointer smoke did not start on the unblocked Home page.",
                )?;
                let about = visible_tooltip(tree, &Page::About.label()).ok_or_else(|| {
                    "The production navigation has no visible About row.".to_owned()
                })?;
                self.pressed = Some(begin_native_press(app, tree, about)?);
                self.pointer_phase = PointerPhase::VerifyAboutCapture;
            }
            PointerPhase::VerifyAboutCapture => {
                require(
                    app.page == Page::Home && !app.closing,
                    "The About action fired before the native mouse release.",
                )?;
                self.verify_held_press(tree, false)?;
                force_refresh(app)?;
                self.pointer_phase = PointerPhase::VerifyAboutRefresh;
            }
            PointerPhase::VerifyAboutRefresh => {
                self.verify_held_press(tree, true)?;
                require(
                    app.page == Page::Home && !app.closing,
                    "Refreshing during the About press delivered navigation before release.",
                )?;
                require(
                    app.view.pending_refresh.get() != 0,
                    "The refreshing Tick did not defer fragment replacement during native capture.",
                )?;
                self.release_native_press(app)?;
                self.pointer_phase = PointerPhase::VerifyAbout;
            }
            PointerPhase::VerifyAbout => {
                require(
                    app.page == Page::About && !app.closing,
                    "Clicking the real About navigation row did not deliver its page intent.",
                )?;
                verify_released_press(app)?;
                self.pressed = None;
                let exit = tree
                    .focusable_order()
                    .into_iter()
                    .filter(|id| {
                        let bounds = tree.abs_bounds(*id);
                        bounds.y >= tree.layout_size.h - 60 && bounds.h > 0
                    })
                    .min_by_key(|id| tree.abs_bounds(*id).x)
                    .ok_or_else(|| {
                        "The production footer has no visible Exit action.".to_owned()
                    })?;
                self.pressed = Some(begin_native_press(app, tree, exit)?);
                self.pointer_phase = PointerPhase::VerifyExitCapture;
            }
            PointerPhase::VerifyExitCapture => {
                require(
                    app.page == Page::About && !app.closing,
                    "The Exit action fired before the native mouse release.",
                )?;
                self.verify_held_press(tree, false)?;
                force_refresh(app)?;
                self.pointer_phase = PointerPhase::VerifyExitRefresh;
            }
            PointerPhase::VerifyExitRefresh => {
                self.verify_held_press(tree, true)?;
                require(
                    app.page == Page::About && !app.closing,
                    "Refreshing during the Exit press opened confirmation before release.",
                )?;
                require(
                    app.view.pending_refresh.get() != 0,
                    "The refreshing Tick did not defer the footer replacement during native capture.",
                )?;
                self.release_native_press(app)?;
                self.pointer_phase = PointerPhase::VerifyModal;
            }
            PointerPhase::VerifyModal => {
                require(
                    app.page == Page::About
                        && app.closing
                        && !app.exiting
                        && app.view.confirming.get(),
                    "Clicking the production Exit action did not open exit confirmation.",
                )?;
                verify_released_press(app)?;
                self.pressed = None;
                click_navigation(tree, Page::Home, true)?;
                self.pointer_phase = PointerPhase::VerifyBlockedNavigation;
            }
            PointerPhase::VerifyBlockedNavigation => {
                require(
                    app.page == Page::About && app.closing && !app.exiting,
                    "The visible exit modal allowed a click through to Home navigation.",
                )?;
                // Native focus collection is scoped to the topmost modal. The
                // unsaved-exit dialog's first action is Cancel, followed by the
                // two explicit save/discard choices.
                let actions = tree.focusable_order();
                require(
                    actions.len() == 3,
                    "The unsaved exit modal did not expose its three native actions.",
                )?;
                let cancel = actions
                    .first()
                    .copied()
                    .ok_or_else(|| "The native exit modal has no Cancel action.".to_owned())?;
                click_control(tree, cancel)?;
                self.pointer_phase = PointerPhase::VerifyCancel;
            }
            PointerPhase::VerifyCancel => {
                require(
                    app.page == Page::About
                        && !app.closing
                        && !app.exiting
                        && !app.view.confirming.get(),
                    "Clicking the native Cancel action did not dismiss exit confirmation.",
                )?;
                self.verify_draft(app)?;
                click_navigation(tree, Page::Home, false)?;
                self.pointer_phase = PointerPhase::VerifyHome;
            }
            PointerPhase::VerifyHome => {
                require(
                    app.page == Page::Home && !app.closing && !app.exiting,
                    "Home navigation remained blocked after cancelling the exit modal.",
                )?;
                self.verify_draft(app)?;
                app.sender()
                    .send(Intent {
                        message: Message::Page(Page::BackgroundEfficiency),
                        refresh: true,
                    })
                    .map_err(|_| {
                        "The status-panel smoke navigation channel disconnected.".to_owned()
                    })?;
                self.pointer_phase = PointerPhase::VerifyDefaultStatus;
            }
            PointerPhase::VerifyDefaultStatus => {
                require(
                    app.page == Page::BackgroundEfficiency && !app.status_collapsed,
                    "The feature page did not start with its side panel expanded.",
                )?;
                let toggle = side_panel_toggle(tree, false)?;
                click_control(tree, toggle)?;
                self.pointer_phase = PointerPhase::VerifyStatusClosed;
            }
            PointerPhase::VerifyStatusClosed => {
                require(
                    app.page == Page::BackgroundEfficiency && app.status_collapsed,
                    "Clicking the bottom Hide action did not collapse its side panel.",
                )?;
                require(
                    visible_tooltip(tree, &rust_i18n::t!("nav.collapse_side_panel")).is_none(),
                    "The collapsed side panel retained its Hide action.",
                )?;
                self.verify_draft(app)?;
                let toggle = side_panel_toggle(tree, true)?;
                click_control(tree, toggle)?;
                self.pointer_phase = PointerPhase::VerifyStatusOpen;
            }
            PointerPhase::VerifyStatusOpen => {
                require(
                    app.page == Page::BackgroundEfficiency && !app.status_collapsed,
                    "Clicking the bottom Show action did not expand its side panel.",
                )?;
                side_panel_toggle(tree, false)?;
                self.verify_draft(app)?;
                let toggle =
                    visible_tooltip(tree, &rust_i18n::t!("common.status")).ok_or_else(|| {
                        "Opening the panel removed the header Status action.".to_owned()
                    })?;
                click_control(tree, toggle)?;
                self.pointer_phase = PointerPhase::VerifyHeaderStatusClosed;
            }
            PointerPhase::VerifyHeaderStatusClosed => {
                require(
                    app.page == Page::BackgroundEfficiency && app.status_collapsed,
                    "The header Status action did not collapse the side panel.",
                )?;
                side_panel_toggle(tree, true)?;
                let toggle =
                    visible_tooltip(tree, &rust_i18n::t!("common.status")).ok_or_else(|| {
                        "The collapsed panel removed the header Status action.".to_owned()
                    })?;
                click_control(tree, toggle)?;
                self.pointer_phase = PointerPhase::VerifyHeaderStatusOpen;
            }
            PointerPhase::VerifyHeaderStatusOpen => {
                require(
                    app.page == Page::BackgroundEfficiency && !app.status_collapsed,
                    "The header Status action did not expand the side panel.",
                )?;
                let toggle = side_panel_toggle(tree, false)?;
                click_control(tree, toggle)?;
                self.pointer_phase = PointerPhase::VerifyFinalStatusClosed;
            }
            PointerPhase::VerifyFinalStatusClosed => {
                require(
                    app.page == Page::BackgroundEfficiency && app.status_collapsed,
                    "The bottom Hide action did not retain the collapsed state for reopen checks.",
                )?;
                side_panel_toggle(tree, true)?;
                self.verify_draft(app)?;
                app.page_offsets
                    .get(&app.page)
                    .ok_or_else(|| "Feature info page has no saved scroll offset.".to_owned())?
                    .set(160);
                let info = visible_tooltip(tree, &rust_i18n::t!("common.feature_info"))
                    .ok_or_else(|| "The header has no visible Feature info action.".to_owned())?;
                click_control(tree, info)?;
                self.pointer_phase = PointerPhase::VerifyFeatureInfoOpen;
            }
            PointerPhase::VerifyFeatureInfoOpen => {
                require(
                    app.feature_info_expanded
                        && app
                            .page_offsets
                            .get(&app.page)
                            .is_some_and(|offset| offset.get() == 0),
                    "Opening Feature info did not reveal it at the top of the main page.",
                )?;
                self.verify_draft(app)?;
                let info = visible_tooltip(tree, &rust_i18n::t!("common.feature_info"))
                    .ok_or_else(|| "Opening help removed the Feature info action.".to_owned())?;
                click_control(tree, info)?;
                self.pointer_phase = PointerPhase::VerifyFeatureInfoClosed;
            }
            PointerPhase::VerifyFeatureInfoClosed => {
                require(!app.feature_info_expanded, "Feature info did not close.")?;
                self.verify_draft(app)?;
                click_navigation(tree, Page::Home, false)?;
                self.pointer_phase = PointerPhase::VerifyReturnHome;
            }
            PointerPhase::VerifyReturnHome => {
                require(
                    app.page == Page::Home && !app.closing && !app.exiting,
                    "Home navigation failed after opening and closing the status panel.",
                )?;
                self.verify_draft(app)?;
                self.pointer_phase = PointerPhase::Done;
            }
            PointerPhase::Done => return Ok(()),
        }
        ctx.mark_dirty_all();
        Ok(())
    }

    fn verify_held_press(&self, tree: &Tree, refreshed: bool) -> Result<(), String> {
        let pressed = self
            .pressed
            .as_ref()
            .ok_or_else(|| "The native held-press fixture has no pressed node.".to_owned())?;
        require(
            ui_pointer_capture_active(),
            "The real UI host did not retain own-window native mouse capture.",
        )?;
        require(
            tree.get(pressed.node).is_some(),
            if refreshed {
                "The refreshing Tick removed the pressed native node before mouse release."
            } else {
                "The pressed native node disappeared before the refreshing Tick."
            },
        )
    }

    fn release_native_press(&self, app: &WinderustApp) -> Result<(), String> {
        let pressed = self
            .pressed
            .as_ref()
            .ok_or_else(|| "The native held-press fixture has no release target.".to_owned())?;
        require(
            own_ui_window(app)? == pressed.hwnd,
            "The owned UI window changed during the native held press.",
        )?;
        post_native_button(pressed.hwnd, pressed.position, false)
    }

    fn check_exit(&mut self, app: &mut WinderustApp, ctx: &mut EventCtx) -> Result<(), String> {
        deliver(app, ctx, Message::Close);
        require(
            app.closing && !app.exiting && app.view.confirming.get(),
            "Exit did not enter confirmation.",
        )?;
        deliver(app, ctx, Message::Stay);
        require(
            !app.closing && !app.exiting && !app.view.confirming.get(),
            "Cancelling exit did not restore running state.",
        )?;
        self.verify_draft(app)?;
        self.exit_cancel_verified = true;
        deliver(app, ctx, Message::Close);
        // Public channels register application-wide pumps. The running app's
        // next wake drains this separate channel without another window or loop.
        let (forward, received) = mpsc::channel();
        let mut channel_app = App::new("Lifecycle wake coalescing", 1, 1);
        let probe = WakeCoalescingProbe {
            sender: channel_app.channel(move |_, intent| {
                if forward.send(intent).is_err() {
                    diagnostics::error("Lifecycle coalescing probe receiver disconnected.");
                }
            }),
            pending: AtomicBool::new(false),
            received,
        };
        for _ in 0..16 {
            enqueue_wake(&probe.sender, &probe.pending);
        }
        self.coalescing_probe = Some(probe);
        self.late_wake_baseline = Some(app.wake_count);
        for _ in 0..16 {
            enqueue_wake(&app.sender(), &app.wake_pending);
        }
        deliver(app, ctx, Message::DiscardAndClose);
        require(
            app.exiting && !app.ready && !app.view.editable.get(),
            "Confirmed exit did not begin asynchronous restoration.",
        )?;
        let navigation = app.settings.global().general.navigation_collapsed;
        deliver(app, ctx, Message::Close);
        deliver(app, ctx, Message::DiscardAndClose);
        deliver(
            app,
            ctx,
            Message::Preferences(super::super::settings_pages::Message::Flag(
                super::super::settings_pages::Flag::CrossSession,
                false,
            )),
        );
        deliver(
            app,
            ctx,
            Message::Preferences(super::super::settings_pages::Message::Flag(
                super::super::settings_pages::Flag::Enabled,
                true,
            )),
        );
        require(
            app.exiting
                && !app.ready
                && app.settings.global().general.navigation_collapsed == navigation
                && !app.settings.global().general.enabled
                && app
                    .settings
                    .global()
                    .general
                    .allow_cross_session_process_control
                && !app.settings.has_unsaved_changes(),
            "A late edit or repeated exit request changed restoration state.",
        )?;
        self.edits_rejected = true;
        self.phase = Phase::Cleanup;
        Ok(())
    }

    /// Runs after the native loop returns, where assertion unwinding is safe.
    pub(super) fn finish(&self, app: &WinderustApp) {
        if let Some(background) = &self.background {
            assert!(
                background.shutdown_requested,
                "Timed background fixture ended before its deadline"
            );
            assert!(
                app.ready && app.exiting,
                "Timed background fixture did not complete restoration"
            );
            assert!(app.error_message.is_empty(), "{}", app.error_message);
            println!(
                "Wind UI background fixture completed after {}ms using production timers and tray probing.",
                self.started.elapsed().as_millis()
            );
            return;
        }
        assert!(self.failures.is_empty(), "{}", self.failures.join("\n"));
        assert_eq!(
            self.phase,
            Phase::Cleanup,
            "Lifecycle checks did not complete."
        );
        assert!(
            self.zero_window_delivered,
            "Zero-window delivery was not observed."
        );
        assert_eq!(
            self.cycles, REOPEN_CYCLES,
            "Reopen cycles did not complete."
        );
        assert_eq!(
            self.resource_checks, REOPEN_CYCLES,
            "Warm cache release and paused snapshot checks did not complete."
        );
        assert_eq!(
            self.stale_completion_checks,
            REOPEN_CYCLES * 2,
            "Stale completions were not checked while absent and after reopening."
        );
        assert!(
            self.exit_cancel_verified,
            "Exit cancellation was not verified."
        );
        assert!(self.edits_rejected, "Late edits were not rejected.");
        assert_eq!(
            self.pointer_phase,
            PointerPhase::Done,
            "Production-window pointer navigation and modal checks did not complete."
        );
        assert!(
            self.late_wake_during_cleanup,
            "No wake was delivered during cleanup."
        );
        assert!(
            self.late_wake_baseline
                .is_some_and(|baseline| app.wake_count > baseline),
            "Late notification burst was not delivered."
        );
        // Native visibility notifications may legitimately add application wakes
        // during teardown. Check exact coalescing on the isolated channel instead.
        let Some(probe) = &self.coalescing_probe else {
            panic!("The isolated wake coalescing probe was not installed.");
        };
        assert!(
            probe.pending.load(Ordering::Acquire),
            "The isolated wake flag was reset before verification."
        );
        let received: Vec<_> = probe.received.try_iter().collect();
        assert_eq!(
            received.len(),
            1,
            "Sixteen pending notifications did not coalesce into one message."
        );
        assert!(
            matches!(
                received.first(),
                Some(Intent {
                    message: Message::Wake,
                    refresh: false
                })
            ),
            "The coalesced message was not a non-refreshing wake."
        );
        assert!(
            app.ready && app.exiting,
            "Cleanup completion was not delivered."
        );
        assert!(
            app.native.is_some(),
            "Native lifecycle bridge was never attached."
        );
        assert!(app.error_message.is_empty(), "{}", app.error_message);
        println!(
            "Resident resource checks passed: {} warmed destroy/reopen cycles; {} stale completion batches; paused snapshot, preferences and drafts preserved.",
            self.resource_checks, self.stale_completion_checks
        );
    }
}

/// Called only by the production main window's interval, with its actual tree.
/// Failures enter normal asynchronous cleanup and are asserted after the loop.
pub(in crate::ui::app) fn check_pointer_input(
    weak: &Weak<RefCell<WinderustApp>>,
    ctx: &mut EventCtx,
) {
    let Some(state) = weak.upgrade() else {
        return;
    };
    let Ok(mut app) = state.try_borrow_mut() else {
        return;
    };
    let Some(mut driver) = app.smoke.take() else {
        return;
    };
    if let Err(error) = driver.check_pointer_input(&app, ctx) {
        driver
            .failures
            .push(format!("{:?}: {error}", driver.pointer_phase));
        driver.cleanup(&mut app, ctx);
    }
    app.smoke = Some(driver);
}

fn visible_tooltip(tree: &Tree, title: &str) -> Option<NodeId> {
    let mut pending: Vec<_> = tree.root.into_iter().collect();
    while let Some(id) = pending.pop() {
        let Some(node) = tree.get(id) else {
            continue;
        };
        if !node.effective_visible() {
            continue;
        }
        if node
            .tooltip
            .as_ref()
            .is_some_and(|text| text.resolve() == title)
        {
            return Some(id);
        }
        pending.extend(node.children.iter().rev().copied());
    }
    None
}

fn own_ui_window(app: &WinderustApp) -> Result<usize, String> {
    app.native
        .as_ref()
        .ok_or_else(|| "The native held-press fixture has no resident shell.".to_owned())?
        .ui_window()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The native held-press fixture has no owned UI window.".to_owned())
}

fn begin_native_press(
    app: &WinderustApp,
    tree: &Tree,
    node: NodeId,
) -> Result<NativePress, String> {
    require(
        !ui_pointer_capture_active(),
        "A previous gesture retained native capture before the held-press fixture.",
    )?;
    require(
        tree.node_enabled(node),
        "The native press target is disabled.",
    )?;
    let logical = control_center(tree, node)?;
    require(
        tree.hit_test(logical)
            .is_some_and(|hit| is_descendant(tree, hit, node)),
        "Another native node covers the held-press target.",
    )?;
    let hwnd = own_ui_window(app)?;
    let mut client = RECT::default();
    // SAFETY: the resident shell returned this live, owned UI HWND; client is a valid output.
    if unsafe { GetClientRect(hwnd as HWND, &mut client) } == 0 {
        return Err(format!(
            "Querying the owned UI client rectangle failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    require(
        client.right > client.left
            && client.bottom > client.top
            && tree.layout_size.w > 0
            && tree.layout_size.h > 0,
        "The owned native UI has no usable client geometry.",
    )?;
    // Wind UI receives physical WM mouse coordinates, then maps them to logical tree space.
    let position = Point::new(
        (f64::from(logical.x) * f64::from(client.right - client.left)
            / f64::from(tree.layout_size.w))
        .round() as i32,
        (f64::from(logical.y) * f64::from(client.bottom - client.top)
            / f64::from(tree.layout_size.h))
        .round() as i32,
    );
    post_native_button(hwnd, position, true)?;
    Ok(NativePress {
        node,
        hwnd,
        position,
    })
}

fn post_native_button(hwnd: usize, position: Point, down: bool) -> Result<(), String> {
    let x = i16::try_from(position.x)
        .map_err(|_| "The native mouse X coordinate exceeds signed WM coordinates.".to_owned())?;
    let y = i16::try_from(position.y)
        .map_err(|_| "The native mouse Y coordinate exceeds signed WM coordinates.".to_owned())?;
    let packed = (u32::from(x as u16) | (u32::from(y as u16) << 16)) as isize;
    let message = if down { WM_LBUTTONDOWN } else { WM_LBUTTONUP };
    let buttons = if down { MK_LBUTTON as usize } else { 0 };
    // SAFETY: callers validate this owned UI HWND; the message contains only packed coordinates.
    if unsafe { PostMessageW(hwnd as HWND, message, buttons, packed) } == 0 {
        return Err(format!(
            "Posting native mouse {} failed: {}",
            if down { "Down" } else { "Up" },
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn force_refresh(app: &WinderustApp) -> Result<(), String> {
    app.sender()
        .send(Intent {
            message: Message::Tick,
            refresh: true,
        })
        .map_err(|_| "The native held-press refresh channel disconnected.".to_owned())
}

fn verify_released_press(app: &WinderustApp) -> Result<(), String> {
    require(
        !ui_pointer_capture_active(),
        "The native UI retained mouse capture after the held press was released.",
    )?;
    require(
        app.view.pending_refresh.get() == 0,
        "Coalesced fragment refreshes did not flush after native mouse release.",
    )
}

fn side_panel_toggle(tree: &Tree, collapsed: bool) -> Result<NodeId, String> {
    let title = rust_i18n::t!(if collapsed {
        "nav.expand_side_panel"
    } else {
        "nav.collapse_side_panel"
    });
    let toggle = visible_tooltip(tree, &title)
        .ok_or_else(|| format!("The side panel has no visible {title} action."))?;
    control_center(tree, toggle)?;
    let panel = tree
        .get(toggle)
        .and_then(|node| node.parent)
        .ok_or_else(|| "The bottom side-panel action has no containing panel.".to_owned())?;
    let bounds = tree.abs_bounds(panel);
    let action = tree.abs_bounds(toggle);
    let expected_width = if collapsed {
        design::SIDEBAR_COLLAPSED_WIDTH
    } else {
        design::SIDE_PANEL_WIDTH
    } as i32;
    require(
        bounds.w == expected_width && bounds.right() == tree.layout_size.w,
        &format!(
            "The side panel is not anchored at the right edge with width {expected_width}: {bounds:?}."
        ),
    )?;
    require(
        bounds.h > action.h
            && action.x >= bounds.x
            && action.right() <= bounds.right()
            && action.y >= bounds.y
            && (0..=16).contains(&(bounds.bottom() - action.bottom()))
            && bounds.bottom() <= tree.layout_size.h
            && bounds.bottom() >= tree.layout_size.h - 100,
        &format!(
            "The side-panel action is not at the bottom of its visible panel: action {action:?}, panel {bounds:?}."
        ),
    )?;
    Ok(toggle)
}

fn click_navigation(tree: &mut Tree, page: Page, blocked: bool) -> Result<(), String> {
    let navigation = visible_tooltip(tree, &page.label())
        .ok_or_else(|| format!("The production navigation has no visible {page:?} row."))?;
    let pos = control_center(tree, navigation)?;
    let hit = tree.hit_test(pos);
    require(
        hit.is_some_and(|id| is_descendant(tree, id, navigation)) != blocked,
        &format!(
            "The {page:?} row hit test was {} (hit {hit:?}, row {navigation:?}).",
            if blocked {
                "not blocked by the modal"
            } else {
                "intercepted above the navigation"
            }
        ),
    )?;
    click_at(tree, pos)
}

fn click_control(tree: &mut Tree, id: NodeId) -> Result<(), String> {
    require(tree.node_enabled(id), "A pointer smoke action is disabled.")?;
    let pos = control_center(tree, id)?;
    require(
        tree.hit_test(pos)
            .is_some_and(|hit| is_descendant(tree, hit, id)),
        "A production pointer action is covered by another native node.",
    )?;
    click_at(tree, pos)
}

fn control_center(tree: &Tree, id: NodeId) -> Result<Point, String> {
    let bounds = tree.abs_bounds(id);
    require(
        bounds.w > 0 && bounds.h > 0,
        &format!(
            "Production pointer target {id:?} has invalid native bounds {bounds:?}; layout {:?}, root {:?}.",
            tree.layout_size,
            tree.root.map(|root| tree.abs_bounds(root)),
        ),
    )?;
    let pos = Point::new(bounds.x + bounds.w / 2, bounds.y + bounds.h / 2);
    require(
        pos.x >= 0 && pos.y >= 0 && pos.x < tree.layout_size.w && pos.y < tree.layout_size.h,
        "A production pointer target is outside the client viewport.",
    )?;
    Ok(pos)
}

fn is_descendant(tree: &Tree, mut id: NodeId, ancestor: NodeId) -> bool {
    loop {
        if id == ancestor {
            return true;
        }
        match tree.get(id).and_then(|node| node.parent) {
            Some(parent) => id = parent,
            None => return false,
        }
    }
}

fn click_at(tree: &mut Tree, pos: Point) -> Result<(), String> {
    let (mut hover, mut capture) = (None, None);
    let down = tree.dispatch_pointer(
        PointerEvent::single(PointerKind::Down, pos, MouseButton::Left),
        &mut hover,
        &mut capture,
    );
    let up = tree.dispatch_pointer(
        PointerEvent::single(PointerKind::Up, pos, MouseButton::Left),
        &mut hover,
        &mut capture,
    );
    require(
        down.consumed && up.consumed && capture.is_none(),
        "Native pointer Down/Up did not complete through the production tree.",
    )
}

fn deliver(app: &mut WinderustApp, ctx: &mut EventCtx, message: Message) {
    app.handle(
        ctx,
        Intent {
            message,
            refresh: true,
        },
    );
}

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}
