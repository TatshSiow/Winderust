//! Opt-in resource workloads on the production resident shell and native widgets.
//! Readiness is checked against actual HWND visibility; failures never unwind in callbacks.
use super::super::{send, Intent, Message, Page, WinderustApp};
use crate::platform::windows::resident_shell::{
    close_ui_windows, ResidentShellError, WindowVisibility,
};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::{Rc, Weak},
    sync::{mpsc, Mutex},
    time::{Duration, Instant},
};
use windui::{core::EventCtx, prelude::App};

const DURATION: &str = "--framework-benchmark-ms";
static COMPLETION: Mutex<Option<Value>> = Mutex::new(None);

struct Options {
    duration_ms: u64,
    scenario: String,
    output: PathBuf,
    scheduler: Option<crate::ui::scheduler_benchmark::Options>,
}

impl Options {
    fn parse() -> Result<Option<Self>, String> {
        let Some(duration) = super::argument(DURATION) else {
            return Ok(None);
        };
        let duration_ms = duration.parse::<u64>().map_err(|error| error.to_string())?;
        if !(1_000..=600_000).contains(&duration_ms) {
            return Err("Benchmark duration must be 1000..600000 milliseconds".into());
        }
        let scenario = super::argument("--framework-benchmark-scenario")
            .ok_or("Missing benchmark scenario")?;
        if ![
            "background",
            "hidden",
            "minimized",
            "home",
            "settings",
            "process-list",
            "navigation",
            "scroll",
            "resize",
            "churn",
        ]
        .contains(&scenario.as_str())
        {
            return Err(format!("Unknown benchmark scenario: {scenario}"));
        }
        let output = super::argument("--framework-benchmark-output")
            .ok_or("Missing benchmark output directory")?
            .into();
        Ok(Some(Self {
            duration_ms,
            scenario,
            output,
            scheduler: crate::ui::scheduler_benchmark::Options::parse()?,
        }))
    }

    fn page(&self) -> Page {
        match self.scenario.as_str() {
            "settings" => Page::LanguageAndAppearance,
            "process-list" | "scroll" => Page::ProcessList,
            _ => Page::Home,
        }
    }

    fn visibility(&self) -> WindowVisibility {
        match self.scenario.as_str() {
            "background" => WindowVisibility::Absent,
            "hidden" => WindowVisibility::Hidden,
            "minimized" => WindowVisibility::Minimized,
            _ => WindowVisibility::Visible,
        }
    }
}

pub(super) fn requested() -> bool {
    std::env::args().any(|argument| argument == DURATION)
}

pub(super) fn fixture_settings() -> crate::config::Settings {
    let mut settings = crate::config::Settings::default();
    settings.general.enabled = false;
    settings.general.check_for_updates = false;
    settings.general.hide_to_tray = true;
    settings.general.start_minimized = false;
    settings.general.startup_with_windows = false;
    settings.general.language = crate::config::AppLanguage::English;
    settings.general.theme_mode = crate::config::AppThemeMode::Dark;
    settings.general.navigation_collapsed = false;
    settings.advanced.show_advanced_controls = true;
    settings
}

pub(in crate::ui::app) struct ResourceDriver {
    options: Options,
    created: Instant,
    ready_at: Option<Instant>,
    cleanup_at: Option<Instant>,
    window_seen: bool,
    visibility_requested: bool,
    actions: u64,
    next_action: Duration,
    failures: Vec<String>,
    measurement_runtime_status: Option<Value>,
    worker_running_at_measurement_end: Option<bool>,
    deadline: Option<mpsc::Sender<Instant>>,
}

impl ResourceDriver {
    pub(in crate::ui::app) fn requested() -> Option<Self> {
        // Parsing precedes App::run_resident: CLI errors cannot unwind a Win32 callback.
        Options::parse()
            .unwrap_or_else(|error| panic!("{error}"))
            .map(|options| Self {
                options,
                created: Instant::now(),
                ready_at: None,
                cleanup_at: None,
                window_seen: false,
                visibility_requested: false,
                actions: 0,
                next_action: Duration::from_millis(250),
                failures: Vec::new(),
                measurement_runtime_status: None,
                worker_running_at_measurement_end: None,
                deadline: None,
            })
    }

    pub(super) fn tick(&mut self, app: &mut WinderustApp, ctx: &mut EventCtx) {
        if app.exiting || self.cleanup_at.is_some() {
            return;
        }
        if let Err(error) = self.advance(app, ctx) {
            self.failures.push(error);
            self.cleanup(app, ctx);
        }
    }

    fn advance(&mut self, app: &mut WinderustApp, ctx: &mut EventCtx) -> Result<(), String> {
        if let Some(ready) = self.ready_at {
            let elapsed = ready.elapsed();
            if elapsed >= Duration::from_millis(self.options.duration_ms) {
                self.cleanup(app, ctx);
            } else if self.options.scenario == "churn" && elapsed >= self.next_action {
                self.actions += 1;
                self.next_action = Duration::from_secs((self.actions + 1) * 3);
                if self.actions % 2 == 1 {
                    if !app.view.can_hide.get() {
                        return Err("Production tray guard did not permit closing the UI".into());
                    }
                    close_ui_windows().map_err(|error| error.to_string())?;
                } else {
                    app.open(ctx);
                }
            }
            return Ok(());
        }
        if self.created.elapsed() > Duration::from_secs(20) {
            return Err("Timed out waiting for the requested native benchmark state".into());
        }
        let Some(native) = &app.native else {
            return Ok(());
        };
        let visibility = native.visibility().map_err(|error| error.to_string())?;
        if self.options.scenario == "background"
            && self.window_seen
            && app.view.can_hide.get()
            && !self.visibility_requested
        {
            let windows = close_ui_windows().map_err(|error| error.to_string())?;
            if windows != 1 {
                return Err(format!(
                    "Expected one native window to close, found {windows}"
                ));
            }
            self.visibility_requested = true;
            // WM_CLOSE was queued first. This single setup wake confirms actual
            // destruction without adding a polling timer to the measured background.
            send(&app.sender(), Message::Wake, false);
            return Ok(());
        }
        if !self.window_seen
            || visibility != self.options.visibility()
            || app.page != self.options.page()
        {
            return Ok(());
        }
        if let Some(options) = &self.options.scheduler {
            let enabled = app.settings.global().general.enabled;
            if app.runtime.benchmark_worker_running() != enabled {
                return Ok(());
            }
            if enabled && !options.initial_pass_observed(&app.status) {
                return Ok(());
            }
        }
        let hwnd = native.ui_window().map_err(|error| error.to_string())?;
        let mut value = json!({
            "framework": "windui", "framework_version": windui::VERSION, "renderer": "tiny-skia software",
            "source_commit": "d5c105067d7a31f0f14e388f1fe6394f1a043988",
            "scenario": self.options.scenario, "duration_ms": self.options.duration_ms,
            "process_id": std::process::id(), "hwnd": hwnd,
            "state": visibility_name(visibility), "automation_enabled": app.settings.global().general.enabled,
            "initial_page": format!("{:?}", app.page),
            "settings_io": "in_memory", "updates_enabled": false, "startup_registration": false,
            "logical_width": 1120, "logical_height": 760,
            "ui_startup_ms": self.created.elapsed().as_secs_f64() * 1000.0,
            "background_behavior": "destroys_native_ui_window", "hidden": app.hidden,
            "readiness": "laid_out_native_window_and_verified_visibility_not_first_paint",
            "deadline": "one_completion_wake_after_readiness; setup_retries_stop_when_ready",
        });
        if let Some(options) = &self.options.scheduler {
            value["scheduler_benchmark"] = options.evidence(
                app.settings.global(),
                app.runtime.benchmark_worker_running(),
                None,
            )?;
            value["runtime_status"] = crate::ui::scheduler_benchmark::status_evidence(&app.status);
        }
        write_json(&self.options.output, "ready.json", &value)?;
        let ready_at = Instant::now();
        self.ready_at = Some(ready_at);
        self.deadline
            .as_ref()
            .ok_or("Resource fixture deadline worker was not installed")?
            .send(ready_at + Duration::from_millis(self.options.duration_ms))
            .map_err(|error| format!("Resource fixture deadline worker stopped: {error}"))?;
        self.next_action = if self.options.scenario == "churn" {
            Duration::from_secs(3)
        } else {
            Duration::from_millis(250)
        };
        Ok(())
    }

    fn window_tick(&mut self, app: &mut WinderustApp, ctx: &mut EventCtx) {
        if app.exiting || self.cleanup_at.is_some() {
            return;
        }
        let size = ctx.tree_mut().layout_size;
        self.window_seen |= size.w > 0 && size.h > 0;
        if self.ready_at.is_none() && !self.visibility_requested && self.window_seen {
            match self.options.scenario.as_str() {
                "hidden" => {
                    self.visibility_requested = true;
                    ctx.hide_window();
                }
                "minimized" => {
                    self.visibility_requested = true;
                    ctx.minimize();
                }
                _ => {}
            }
        }
        if self.options.scenario == "navigation"
            && self
                .ready_at
                .is_some_and(|ready| ready.elapsed() >= self.next_action)
        {
            let page = [
                Page::Home,
                Page::ProcessList,
                Page::LanguageAndAppearance,
                Page::About,
            ][self.actions as usize % 4];
            self.actions += 1;
            self.next_action = Duration::from_millis((self.actions + 1) * 250);
            deliver(app, ctx, Message::Page(page));
        }
    }

    fn cleanup(&mut self, app: &mut WinderustApp, ctx: &mut EventCtx) {
        if self.cleanup_at.is_none() {
            self.deadline = None;
            if self.options.scheduler.is_some() {
                let status = app
                    .runtime
                    .status_snapshot_since(0)
                    .unwrap_or_else(|| app.status.clone());
                self.measurement_runtime_status =
                    Some(crate::ui::scheduler_benchmark::status_evidence(&status));
                self.worker_running_at_measurement_end =
                    Some(app.runtime.benchmark_worker_running());
            }
            self.cleanup_at = Some(Instant::now());
            deliver(app, ctx, Message::DiscardAndClose);
        }
    }

    pub(super) fn complete(&self, app: &WinderustApp) {
        let mut errors = self.failures.clone();
        if !app.ready || !app.exiting || app.shutdown_failed {
            errors.push("Native fixture exited without successful coordinated cleanup".into());
        }
        if !app.error_message.is_empty() {
            errors.push(app.error_message.clone());
        }
        let mut value = json!({
            "framework": "windui", "renderer": "tiny-skia software", "scenario": self.options.scenario,
            "requested_duration_ms": self.options.duration_ms,
            "ready_to_cleanup_ms": self.ready_at.zip(self.cleanup_at).map(|(start,end)| end.duration_since(start).as_secs_f64() * 1000.0),
            "cleanup_ms": self.cleanup_at.map(|start| start.elapsed().as_secs_f64() * 1000.0),
            "actions_completed": self.actions, "last_page": format!("{:?}", app.page),
            "automation_enabled": app.settings.global().general.enabled,
            "runtime_worker_error": app.status.worker_error, "cleanup_errors": errors,
            "measurement_runtime_status": self.measurement_runtime_status,
            "worker_running_at_measurement_end": self.worker_running_at_measurement_end,
        });
        if let Some(options) = &self.options.scheduler {
            match options.evidence(
                app.settings.global(),
                app.runtime.benchmark_worker_running(),
                None,
            ) {
                Ok(evidence) => value["scheduler_benchmark"] = evidence,
                Err(error) => value["cleanup_errors"]
                    .as_array_mut()
                    .map(|errors| errors.push(json!(error)))
                    .unwrap_or_default(),
            }
            value["runtime_status"] = crate::ui::scheduler_benchmark::status_evidence(&app.status);
        }
        *COMPLETION
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(value);
    }
}

pub(super) fn configure(state: &Rc<RefCell<WinderustApp>>, mut app: App) -> App {
    let mut model = state.borrow_mut();
    let sender = model.sender();
    let Some(driver) = model.resource_benchmark.as_mut() else {
        return app;
    };
    let page = driver.options.page();
    let output = driver.options.output.clone();
    // Setup runs outside native callbacks, before any benchmark is declared ready.
    fs::create_dir_all(output).unwrap_or_else(|error| panic!("Benchmark output: {error}"));
    send(&sender, Message::Page(page), true);
    let churn = driver.options.scenario == "churn";
    let (deadline, commands) = mpsc::channel();
    let setup_deadline = driver.created + Duration::from_secs(20);
    std::thread::Builder::new()
        .name("winderust-resource-deadline".into())
        .spawn(move || {
            wait_for_deadline(commands, setup_deadline, || {
                sender
                    .send(Intent {
                        message: Message::Wake,
                        refresh: false,
                    })
                    .is_ok()
            });
        })
        .unwrap_or_else(|error| panic!("Resource fixture deadline worker: {error}"));
    driver.deadline = Some(deadline);
    drop(model);
    if churn {
        let weak = Rc::downgrade(state);
        app = app.on_interval(Duration::from_secs(3), move |ctx| {
            if let Some(state) = weak.upgrade() {
                state.borrow_mut().handle(
                    ctx,
                    Intent {
                        message: Message::Tick,
                        refresh: false,
                    },
                );
            }
        });
    }
    app.start_window(super::super::main_window(state))
}

fn wait_for_deadline(
    commands: mpsc::Receiver<Instant>,
    setup_deadline: Instant,
    mut wake: impl FnMut() -> bool,
) {
    // Only setup retries poll; readiness replaces them with one blocking deadline wait.
    let mut completion = None;
    loop {
        let now = Instant::now();
        let wait = completion.map_or_else(
            || Duration::from_millis(100).min(setup_deadline.saturating_duration_since(now)),
            |deadline: Instant| deadline.saturating_duration_since(now),
        );
        match commands.recv_timeout(wait) {
            Ok(deadline) => completion = Some(deadline),
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !wake() || completion.is_some() || Instant::now() >= setup_deadline {
                    return;
                }
            }
        }
    }
}

pub(in crate::ui::app) fn resource_window_tick(
    weak: &Weak<RefCell<WinderustApp>>,
    ctx: &mut EventCtx,
) {
    let Some(state) = weak.upgrade() else { return };
    let Ok(mut app) = state.try_borrow_mut() else {
        return;
    };
    let Some(mut driver) = app.resource_benchmark.take() else {
        return;
    };
    driver.window_tick(&mut app, ctx);
    app.resource_benchmark = Some(driver);
}

pub(super) fn finish_run(
    result: &Result<(), ResidentShellError>,
    recovery: &Result<(), String>,
    elapsed: Duration,
) {
    if !requested() {
        return;
    }
    let options = match Options::parse() {
        Ok(Some(options)) => options,
        _ => return,
    };
    let mut value = COMPLETION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
        .unwrap_or_else(|| json!({"error": "UI exited before completion"}));
    value["total_process_run_ms"] = json!(elapsed.as_secs_f64() * 1000.0);
    value["loop_result"] = json!(result.as_ref().map(|_| "ok").map_err(ToString::to_string));
    if let Err(error) = recovery {
        if let Some(errors) = value["cleanup_errors"].as_array_mut() {
            errors.push(json!(error));
        }
    }
    if options.scheduler.is_some() {
        value["scheduler_benchmark"]["restoration_succeeded"] = json!(
            result.is_ok()
                && recovery.is_ok()
                && value["runtime_worker_error"].is_null()
                && value["cleanup_errors"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
                && value["scheduler_benchmark"]["worker_running"] == false
        );
    }
    if let Err(error) = write_json(&options.output, "finished.json", &value) {
        eprintln!("{error}");
    }
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

fn visibility_name(visibility: WindowVisibility) -> &'static str {
    match visibility {
        WindowVisibility::Absent => "absent",
        WindowVisibility::Hidden => "hidden",
        WindowVisibility::Minimized => "minimized",
        WindowVisibility::Visible => "visible",
    }
}

fn write_json(directory: &Path, name: &str, value: &Value) -> Result<(), String> {
    let temporary = directory.join(format!("{name}.tmp"));
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    fs::rename(temporary, directory.join(name)).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_fixture_waits_for_one_deadline_without_setup_polling() {
        let (commands, receiver) = mpsc::channel();
        let (wakes, observed) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            wait_for_deadline(receiver, Instant::now() + Duration::from_secs(2), || {
                wakes.send(Instant::now()).is_ok()
            });
        });
        let deadline = Instant::now() + Duration::from_millis(250);
        commands.send(deadline).unwrap();
        assert!(observed.recv_timeout(Duration::from_secs(2)).unwrap() >= deadline);
        worker.join().unwrap();
        assert!(observed.try_recv().is_err());
    }

    #[test]
    fn fixture_cleanup_cancels_a_pending_completion_wake() {
        let (commands, receiver) = mpsc::channel();
        let (wakes, observed) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            wait_for_deadline(receiver, Instant::now() + Duration::from_secs(2), || {
                wakes.send(()).is_ok()
            });
        });
        commands
            .send(Instant::now() + Duration::from_secs(1))
            .unwrap();
        drop(commands);
        worker.join().unwrap();
        assert!(observed.try_recv().is_err());
    }

    #[test]
    fn expired_setup_deadline_wakes_once_for_timeout_cleanup() {
        let (_commands, receiver) = mpsc::channel();
        let (wakes, observed) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            wait_for_deadline(receiver, Instant::now(), || wakes.send(()).is_ok());
        });
        observed.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.join().unwrap();
        assert!(observed.try_recv().is_err());
    }

    #[test]
    fn resource_settings_disable_automation_and_external_services() {
        let settings = fixture_settings();
        assert!(!settings.general.enabled);
        assert!(!settings.battery_profile().general.enabled);
        assert!(!settings.general.check_for_updates);
        assert!(!settings.general.startup_with_windows);
        assert!(settings.general.hide_to_tray);
        assert!(!settings.general.start_minimized);
    }

    #[test]
    fn background_and_hidden_workloads_have_different_native_lifetimes() {
        let mut options = Options {
            duration_ms: 1000,
            scenario: "background".into(),
            output: PathBuf::new(),
            scheduler: None,
        };
        assert_eq!(options.visibility(), WindowVisibility::Absent);
        options.scenario = "hidden".into();
        assert_eq!(options.visibility(), WindowVisibility::Hidden);
        options.scenario = "resize".into();
        assert_eq!(options.page(), Page::Home);
    }
}
