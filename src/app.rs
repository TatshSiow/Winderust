use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    deferred, div, prelude::*, px, rgb, AnyElement, App, Context, Entity, Focusable, IntoElement,
    SharedString, Subscription, Task, Timer, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Escape as InputEscape, Input, InputEvent, InputState},
    scroll::ScrollableElement,
    v_flex, Disableable, Sizable,
};

use crate::{
    activity::{ActivitySnapshot, ActivityState, IdleDetector, InputHook, InputHookEvents},
    automation::BackgroundAutomation,
    config::{
        self, AppSuspensionRule, AppSuspensionSettings, CpuUsageComparison, CpuUsageRule,
        EcoQosSettings, ForegroundRule, NetworkThresholdUnit, ScheduleRule, Settings,
        WeekdaySetting,
    },
    cpu::{CpuUsageMonitor, CpuUsageSnapshot},
    ecoqos::{self, EcoQosSnapshot},
    foreground::{list_process_names, ForegroundDetector},
    power::{PowerPlan, PowerPlanManager},
    power_source,
    rules::{DecisionEngine, DecisionInput, DecisionOutcome, DecisionState},
    scheduler::{CpuUsageScheduler, Scheduler},
    startup,
    suspension::{self, AppSuspensionSnapshot},
    tray::{self, TrayIcon},
    ui::{self, Page},
};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY,
    OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

const ACTIVE_PLAN_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const APP_TICK_INTERVAL: Duration = Duration::from_millis(250);
const CPU_USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PROCESS_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const PAGE_HEADER_HEIGHT: f32 = 42.0;
const PROCESS_PICKER_LAYER_PRIORITY: usize = 2;
const SWITCH_RETRY_INTERVAL: Duration = Duration::from_secs(15);
const MAX_NETWORK_THRESHOLD_BYTES: u64 = 1_000_000_000;

const COLOR_BG: u32 = 0x282c33;
const COLOR_CHROME: u32 = 0x3b414d;
const COLOR_PANEL: u32 = 0x2f343e;
const COLOR_PANEL_ALT: u32 = 0x363c46;
const COLOR_PANEL_ACTIVE: u32 = 0x454a56;
const COLOR_BORDER: u32 = 0x464b57;
const COLOR_BORDER_SUBTLE: u32 = 0x363c46;
const COLOR_TEXT: u32 = 0xdce0e5;
const COLOR_MUTED: u32 = 0xa9afbc;
const COLOR_DIM: u32 = 0x878a98;
const COLOR_ACCENT: u32 = 0x74ade8;
const COLOR_ACCENT_BG: u32 = 0x293b5b;
const COLOR_SUCCESS: u32 = 0xa1c181;
const COLOR_SUCCESS_BG: u32 = 0x38482f;
const COLOR_WARNING: u32 = 0xdec184;
const COLOR_WARNING_BG: u32 = 0x5d4c2f;
const COLOR_DANGER: u32 = 0xd07277;
const COLOR_DANGER_BG: u32 = 0x4c2b2c;

pub struct PowerLeafApp {
    settings: Settings,
    saved_settings: Settings,
    page: Page,
    plans: Vec<PowerPlan>,
    current_plan: Option<PowerPlan>,
    activity: ActivitySnapshot,
    cpu_usage: CpuUsageSnapshot,
    eco_qos_status: EcoQosSnapshot,
    app_suspension_status: AppSuspensionSnapshot,
    foreground_app: Option<String>,
    decision: DecisionOutcome,
    next_schedule: String,
    next_check: Instant,
    next_active_plan_refresh: Instant,
    next_cpu_usage_refresh: Instant,
    next_process_refresh: Instant,
    last_switch_attempt: Option<(String, Instant)>,
    power: PowerPlanManager,
    background_automation: BackgroundAutomation,
    cpu_monitor: CpuUsageMonitor,
    idle_detector: IdleDetector,
    input_hook: Option<InputHook>,
    foreground_detector: ForegroundDetector,
    scheduler: Scheduler,
    cpu_usage_scheduler: CpuUsageScheduler,
    decision_engine: DecisionEngine,
    hwnd: Option<HWND>,
    tray_icon: Option<TrayIcon>,
    status_message: String,
    process_candidates: Vec<String>,
    active_power_plan_picker: Option<String>,
    start_minimized_applied: bool,
    editing_rule_title: Option<RuleTitleTarget>,
    collapsed_rule_cards: HashSet<RuleCardTarget>,
    _rule_title_input_subscriptions: Vec<Subscription>,
    inputs: UiInputs,
    _tick_task: Task<()>,
}

struct UiInputs {
    cpu_rule_names: Vec<Entity<InputState>>,
    schedule_rule_names: Vec<Entity<InputState>>,
    schedule_start_times: Vec<Entity<InputState>>,
    schedule_end_times: Vec<Entity<InputState>>,
    foreground_rule_names: Vec<Entity<InputState>>,
    foreground_rule_processes: Vec<Entity<InputState>>,
    eco_qos_exclusion: Entity<InputState>,
    suspension_process: Entity<InputState>,
}

impl UiInputs {
    fn new(window: &mut Window, cx: &mut Context<PowerLeafApp>, settings: &Settings) -> Self {
        Self {
            cpu_rule_names: settings
                .cpu_usage_mode
                .rules
                .iter()
                .map(|rule| make_input(window, cx, &rule.name, "Rule name"))
                .collect(),
            schedule_rule_names: settings
                .schedule_mode
                .rules
                .iter()
                .map(|rule| make_input(window, cx, &rule.name, "Rule name"))
                .collect(),
            schedule_start_times: settings
                .schedule_mode
                .rules
                .iter()
                .map(|rule| make_input(window, cx, &rule.start_time, "HH:MM"))
                .collect(),
            schedule_end_times: settings
                .schedule_mode
                .rules
                .iter()
                .map(|rule| make_input(window, cx, &rule.end_time, "HH:MM"))
                .collect(),
            foreground_rule_names: settings
                .foreground_rules
                .rules
                .iter()
                .map(|rule| make_input(window, cx, &rule.name, "Rule name"))
                .collect(),
            foreground_rule_processes: settings
                .foreground_rules
                .rules
                .iter()
                .map(|rule| make_input(window, cx, &rule.process_name, "process.exe"))
                .collect(),
            eco_qos_exclusion: make_input(window, cx, "", "Search running apps..."),
            suspension_process: make_input(window, cx, "", "Search running apps..."),
        }
    }

    fn ensure_for_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<PowerLeafApp>,
        settings: &Settings,
    ) {
        sync_input_vec(
            &mut self.cpu_rule_names,
            settings.cpu_usage_mode.rules.len(),
            window,
            cx,
            |index| settings.cpu_usage_mode.rules[index].name.clone(),
            "Rule name",
        );
        sync_input_vec(
            &mut self.schedule_rule_names,
            settings.schedule_mode.rules.len(),
            window,
            cx,
            |index| settings.schedule_mode.rules[index].name.clone(),
            "Rule name",
        );
        sync_input_vec(
            &mut self.schedule_start_times,
            settings.schedule_mode.rules.len(),
            window,
            cx,
            |index| settings.schedule_mode.rules[index].start_time.clone(),
            "HH:MM",
        );
        sync_input_vec(
            &mut self.schedule_end_times,
            settings.schedule_mode.rules.len(),
            window,
            cx,
            |index| settings.schedule_mode.rules[index].end_time.clone(),
            "HH:MM",
        );
        sync_input_vec(
            &mut self.foreground_rule_names,
            settings.foreground_rules.rules.len(),
            window,
            cx,
            |index| settings.foreground_rules.rules[index].name.clone(),
            "Rule name",
        );
        sync_input_vec(
            &mut self.foreground_rule_processes,
            settings.foreground_rules.rules.len(),
            window,
            cx,
            |index| settings.foreground_rules.rules[index].process_name.clone(),
            "process.exe",
        );
    }
}

impl PowerLeafApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let hwnd = tray::hwnd_from_window(window);
        let settings = config::storage::load().unwrap_or_else(|err| {
            eprintln!("{err}");
            Settings::default()
        });
        let inputs = UiInputs::new(window, cx, &settings);
        let background_automation = BackgroundAutomation::start(&settings);

        let mut app = Self {
            saved_settings: settings.clone(),
            settings,
            page: Page::Dashboard,
            plans: Vec::new(),
            current_plan: None,
            activity: ActivitySnapshot {
                state: ActivityState::Unknown,
                idle_for: None,
            },
            cpu_usage: CpuUsageSnapshot::default(),
            eco_qos_status: EcoQosSnapshot::default(),
            app_suspension_status: AppSuspensionSnapshot::default(),
            foreground_app: None,
            decision: DecisionOutcome {
                target_guid: None,
                state: DecisionState::NoTargetPlan,
                reason: "Waiting for first check.".to_owned(),
            },
            next_schedule: "No active time rules".to_owned(),
            next_check: Instant::now(),
            next_active_plan_refresh: Instant::now(),
            next_cpu_usage_refresh: Instant::now(),
            next_process_refresh: Instant::now(),
            last_switch_attempt: None,
            power: PowerPlanManager,
            background_automation,
            cpu_monitor: CpuUsageMonitor::default(),
            idle_detector: IdleDetector,
            input_hook: None,
            foreground_detector: ForegroundDetector,
            scheduler: Scheduler,
            cpu_usage_scheduler: CpuUsageScheduler::default(),
            decision_engine: DecisionEngine,
            hwnd,
            tray_icon: None,
            status_message: "Ready".to_owned(),
            process_candidates: Vec::new(),
            active_power_plan_picker: None,
            start_minimized_applied: false,
            editing_rule_title: None,
            collapsed_rule_cards: HashSet::new(),
            _rule_title_input_subscriptions: Vec::new(),
            inputs,
            _tick_task: Task::ready(()),
        };

        app.rebuild_rule_title_input_subscriptions(window, cx);
        window.on_window_should_close(cx, |_, _| !tray::is_hidden_to_tray());
        app.sync_tray_icon();
        app.refresh_process_candidates(false);
        app.refresh_power_plans();
        app.run_check();
        app.install_input_hook();
        app.schedule_tick(window, cx);
        app
    }

    fn schedule_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._tick_task = cx.spawn_in(window, async move |this, cx| {
            Timer::after(APP_TICK_INTERVAL).await;
            let _ = cx.update(move |window, app_cx| {
                if let Some(this) = this.upgrade() {
                    let _ = this.update(app_cx, |app, cx| {
                        app.sync_input_values(cx);
                        if app.tick(window) {
                            app.schedule_tick(window, cx);
                            cx.notify();
                        }
                    });
                }
            });
        });
    }

    fn refresh_power_plans(&mut self) {
        match self.power.list_plans() {
            Ok(plans) => {
                self.plans = plans;
                self.current_plan = self.plans.iter().find(|plan| plan.active).cloned();
                self.next_active_plan_refresh = Instant::now() + ACTIVE_PLAN_REFRESH_INTERVAL;
                self.status_message = format!("Loaded {} power plans.", self.plans.len());
            }
            Err(err) => self.status_message = err,
        }
    }

    fn refresh_active_plan(&mut self) {
        self.next_active_plan_refresh = Instant::now() + ACTIVE_PLAN_REFRESH_INTERVAL;

        match self.power.active_plan() {
            Ok(active) => {
                if let Some(active) = active {
                    let active_guid = active.guid.clone();
                    for plan in &mut self.plans {
                        plan.active = plan.guid.eq_ignore_ascii_case(&active_guid);
                    }
                    self.current_plan = self
                        .plans
                        .iter()
                        .find(|plan| plan.guid.eq_ignore_ascii_case(&active_guid))
                        .cloned()
                        .or(Some(active));
                }
            }
            Err(err) => self.status_message = err,
        }
    }

    fn run_check(&mut self) {
        if Instant::now() >= self.next_active_plan_refresh {
            self.refresh_active_plan();
        }

        self.activity = self.idle_detector.snapshot(Duration::from_secs(
            self.settings.activity_mode.idle_timeout_seconds,
        ));
        if Instant::now() >= self.next_cpu_usage_refresh {
            self.cpu_usage = self.cpu_monitor.sample();
            self.next_cpu_usage_refresh = Instant::now() + CPU_USAGE_REFRESH_INTERVAL;
        }
        self.foreground_app = self.foreground_detector.process_name();
        let schedule = self
            .scheduler
            .current_decision(&self.settings.schedule_mode);
        let cpu_usage = self
            .cpu_usage_scheduler
            .current_decision(&self.settings.cpu_usage_mode, self.cpu_usage.percent);
        self.next_schedule = self
            .scheduler
            .next_switch_label(&self.settings.schedule_mode);

        let decision_settings = self.runtime_settings();
        self.decision = self.decision_engine.decide(
            &decision_settings,
            DecisionInput {
                activity_state: self.activity.state,
                foreground_app: self.foreground_app.clone(),
                plugged_in: power_source::is_plugged_in(),
                schedule,
                cpu_usage,
            },
        );

        self.apply_decision();
    }

    fn install_input_hook(&mut self) {
        match InputHook::install() {
            Ok(input_hook) => {
                self.input_hook = Some(input_hook);
            }
            Err(err) => {
                self.status_message = err;
            }
        }
    }

    fn input_hook_should_check(&self, events: InputHookEvents) -> bool {
        self.saved_settings.general.enabled
            && self.settings.activity_mode.enabled
            && ((events.keyboard && self.settings.activity_mode.input_detection.keyboard)
                || (events.mouse && self.settings.activity_mode.input_detection.mouse))
    }

    fn apply_decision(&mut self) {
        let Some(target_guid) = self.decision.target_guid.as_deref() else {
            return;
        };

        let already_active = self
            .current_plan
            .as_ref()
            .is_some_and(|plan| plan.guid.eq_ignore_ascii_case(target_guid));
        if already_active {
            return;
        }

        if let Some((last_guid, attempted_at)) = &self.last_switch_attempt {
            if last_guid.eq_ignore_ascii_case(target_guid)
                && attempted_at.elapsed() < SWITCH_RETRY_INTERVAL
            {
                return;
            }
        }

        self.last_switch_attempt = Some((target_guid.to_owned(), Instant::now()));

        match self.power.set_active(target_guid) {
            Ok(()) => {
                self.status_message = format!("Switched power plan: {}", self.decision.reason);
                self.refresh_power_plans();
            }
            Err(err) => self.status_message = err,
        }
    }

    fn save_settings(&mut self) {
        match config::storage::save(&self.settings) {
            Ok(()) => {
                self.saved_settings = self.settings.clone();
                self.status_message = match startup::set_startup_with_windows(
                    self.saved_settings.general.startup_with_windows,
                ) {
                    Ok(()) => format!(
                        "Saved settings to {}",
                        config::storage::config_path().display()
                    ),
                    Err(err) => format!("Saved settings, but {err}."),
                };
            }
            Err(err) => self.status_message = err,
        }
    }

    fn export_settings_toml(&mut self) {
        match choose_settings_file(self.hwnd, FileDialogMode::Save) {
            Ok(Some(path)) => match config::storage::export_toml_to(&path, &self.settings) {
                Ok(()) => {
                    self.status_message = format!("Exported settings to {}", path.display());
                }
                Err(err) => self.status_message = err,
            },
            Ok(None) => {
                self.status_message = "Export canceled.".to_owned();
            }
            Err(err) => self.status_message = err,
        }
    }

    fn import_settings_toml(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match choose_settings_file(self.hwnd, FileDialogMode::Open) {
            Ok(Some(path)) => match config::storage::import_toml_from(&path) {
                Ok(settings) => {
                    self.settings = settings;
                    match config::storage::save(&self.settings) {
                        Ok(()) => {
                            self.saved_settings = self.settings.clone();
                            self.status_message = match startup::set_startup_with_windows(
                                self.saved_settings.general.startup_with_windows,
                            ) {
                                Ok(()) => format!("Imported settings from {}", path.display()),
                                Err(err) => format!("Imported settings, but {err}."),
                            };
                            self.rebuild_inputs(window, cx);
                        }
                        Err(err) => self.status_message = err,
                    }
                }
                Err(err) => self.status_message = err,
            },
            Ok(None) => {
                self.status_message = "Import canceled.".to_owned();
            }
            Err(err) => self.status_message = err,
        }
    }

    fn refresh_process_candidates(&mut self, report_status: bool) {
        self.next_process_refresh = Instant::now() + PROCESS_REFRESH_INTERVAL;
        match list_process_names() {
            Ok(processes) => {
                self.process_candidates = processes;
                if report_status {
                    self.status_message = format!(
                        "Loaded {} running applications.",
                        self.process_candidates.len()
                    );
                }
            }
            Err(err) => self.status_message = err,
        }
    }

    fn sync_tray_icon(&mut self) {
        let tray_required =
            self.settings.general.hide_to_tray || self.saved_settings.general.start_minimized;

        if tray_required {
            if self.tray_icon.is_none() {
                let Some(hwnd) = self.hwnd else {
                    tray::set_hide_on_close(false);
                    self.status_message = "System tray unavailable: no window handle.".to_owned();
                    return;
                };

                match TrayIcon::install(hwnd) {
                    Ok(icon) => {
                        self.tray_icon = Some(icon);
                        self.status_message = "System tray icon enabled.".to_owned();
                    }
                    Err(err) => self.status_message = err,
                }
            }
            tray::set_hide_on_close(self.settings.general.hide_to_tray && self.tray_icon.is_some());
        } else if self.tray_icon.take().is_some() {
            tray::set_hide_on_close(false);
            self.status_message = "System tray icon disabled.".to_owned();
        } else {
            tray::set_hide_on_close(false);
        }
    }

    fn apply_start_minimized(&mut self, window: &mut Window) {
        if self.start_minimized_applied {
            return;
        }
        self.start_minimized_applied = true;

        if !self.saved_settings.general.start_minimized {
            return;
        }

        if self.tray_icon.is_some() {
            if let Some(hwnd) = self.hwnd {
                tray::hide_window(hwnd);
                self.status_message = "Started in system tray.".to_owned();
                return;
            }
        }

        window.minimize_window();
        self.status_message = "Started minimized.".to_owned();
    }

    fn tick(&mut self, window: &mut Window) -> bool {
        if tray::take_quit_requested() {
            tray::set_hide_on_close(false);
            self.tray_icon = None;
            window.remove_window();
            return false;
        }

        self.apply_start_minimized(window);
        if tray::is_hidden_to_tray() {
            self.background_automation
                .update_settings(&self.background_settings());
            return true;
        }

        self.eco_qos_status = self.background_automation.eco_qos_status();
        self.app_suspension_status = self.background_automation.app_suspension_status();

        if Instant::now() >= self.next_process_refresh {
            self.refresh_process_candidates(false);
        }

        let input_events = self
            .input_hook
            .as_ref()
            .map(InputHook::take_events)
            .unwrap_or_default();
        let should_check_now =
            Instant::now() >= self.next_check || self.input_hook_should_check(input_events);

        if should_check_now {
            self.run_check();
            self.next_check = Instant::now()
                + Duration::from_millis(self.settings.general.check_interval_ms.max(250));
        }

        self.sync_tray_icon();
        self.background_automation
            .update_settings(&self.background_settings());
        true
    }

    fn cancel_settings_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings = self.saved_settings.clone();
        self.status_message = "Unsaved settings changes canceled.".to_owned();
        self.editing_rule_title = None;
        self.collapsed_rule_cards.clear();
        self.rebuild_inputs(window, cx);
    }

    fn rebuild_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.settings.clone();
        self.editing_rule_title = None;
        self.collapsed_rule_cards.clear();
        self.inputs = UiInputs::new(window, cx, &settings);
        self.rebuild_rule_title_input_subscriptions(window, cx);
    }

    fn rule_title_input_count(&self) -> usize {
        self.inputs.foreground_rule_names.len()
            + self.inputs.schedule_rule_names.len()
            + self.inputs.cpu_rule_names.len()
    }

    fn ensure_rule_title_input_subscriptions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self._rule_title_input_subscriptions.len() != self.rule_title_input_count() {
            self.rebuild_rule_title_input_subscriptions(window, cx);
        }
    }

    fn rebuild_rule_title_input_subscriptions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut inputs = Vec::new();
        inputs.extend(
            self.inputs
                .foreground_rule_names
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, input)| (input, RuleTitleTarget::Foreground(index))),
        );
        inputs.extend(
            self.inputs
                .schedule_rule_names
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, input)| (input, RuleTitleTarget::Schedule(index))),
        );
        inputs.extend(
            self.inputs
                .cpu_rule_names
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, input)| (input, RuleTitleTarget::Cpu(index))),
        );

        self._rule_title_input_subscriptions.clear();
        for (input, target) in inputs {
            self.subscribe_to_rule_title_input(input, target, window, cx);
        }
    }

    fn subscribe_to_rule_title_input(
        &mut self,
        input: Entity<InputState>,
        target: RuleTitleTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._rule_title_input_subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |app, _, event: &InputEvent, _, cx| {
                app.handle_rule_title_input_event(target, event, cx);
            },
        ));
    }

    fn handle_rule_title_input_event(
        &mut self,
        target: RuleTitleTarget,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            self.finish_rule_title_edit(target, cx);
        }
    }

    fn rule_title_input(&self, target: RuleTitleTarget) -> Option<Entity<InputState>> {
        match target {
            RuleTitleTarget::Foreground(index) => self.inputs.foreground_rule_names.get(index),
            RuleTitleTarget::Schedule(index) => self.inputs.schedule_rule_names.get(index),
            RuleTitleTarget::Cpu(index) => self.inputs.cpu_rule_names.get(index),
        }
        .cloned()
    }

    fn begin_rule_title_edit(
        &mut self,
        target: RuleTitleTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editing_rule_title = Some(target);
        if let Some(input) = self.rule_title_input(target) {
            input.read(cx).focus_handle(cx).focus(window);
        }
        cx.notify();
    }

    fn finish_rule_title_edit(&mut self, target: RuleTitleTarget, cx: &mut Context<Self>) {
        self.sync_input_values(cx);
        if self.editing_rule_title == Some(target) {
            self.editing_rule_title = None;
        }
        cx.notify();
    }

    fn is_rule_card_collapsed(&self, target: &RuleCardTarget) -> bool {
        self.collapsed_rule_cards.contains(target)
    }

    fn toggle_rule_card(&mut self, target: RuleCardTarget, cx: &mut Context<Self>) {
        if !self.collapsed_rule_cards.remove(&target) {
            self.collapsed_rule_cards.insert(target);
        }
        cx.notify();
    }

    fn sync_input_values(&mut self, cx: &mut Context<Self>) {
        for (rule, input) in self
            .settings
            .cpu_usage_mode
            .rules
            .iter_mut()
            .zip(&self.inputs.cpu_rule_names)
        {
            rule.name = input.read(cx).value().to_string();
        }
        for (index, rule) in self.settings.schedule_mode.rules.iter_mut().enumerate() {
            if let Some(input) = self.inputs.schedule_rule_names.get(index) {
                rule.name = input.read(cx).value().to_string();
            }
            if let Some(input) = self.inputs.schedule_start_times.get(index) {
                rule.start_time = input.read(cx).value().to_string();
            }
            if let Some(input) = self.inputs.schedule_end_times.get(index) {
                rule.end_time = input.read(cx).value().to_string();
            }
        }
        for (index, rule) in self.settings.foreground_rules.rules.iter_mut().enumerate() {
            if let Some(input) = self.inputs.foreground_rule_names.get(index) {
                rule.name = input.read(cx).value().to_string();
            }
            if let Some(input) = self.inputs.foreground_rule_processes.get(index) {
                rule.process_name = input.read(cx).value().to_string();
            }
        }
    }

    fn background_settings(&self) -> Settings {
        self.runtime_settings()
    }

    fn runtime_settings(&self) -> Settings {
        let mut settings = self.settings.clone();
        settings.general.enabled = self.saved_settings.general.enabled;
        settings.eco_qos = self.saved_settings.eco_qos.clone();
        settings.app_suspension = self.saved_settings.app_suspension.clone();
        settings
    }
}

impl Render for PowerLeafApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.inputs.ensure_for_settings(window, cx, &self.settings);
        self.ensure_rule_title_input_subscriptions(window, cx);
        self.sync_input_values(cx);

        let page = self.render_page(window, cx);
        let unsaved = self.settings != self.saved_settings;

        div()
            .relative()
            .flex()
            .flex_row()
            .size_full()
            .bg(rgb(COLOR_BG))
            .text_color(rgb(COLOR_TEXT))
            .child(self.render_navigation(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.0))
                            .min_h(px(0.0))
                            .overflow_y_scrollbar()
                            .p_4()
                            .gap_3()
                            .child(page),
                    )
                    .child(self.render_status_bar()),
            )
            .child(if unsaved {
                self.render_unsaved_popup(window, cx).into_any_element()
            } else {
                div().into_any_element()
            })
    }
}

impl PowerLeafApp {
    fn render_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut nav = v_flex()
            .w(px(258.0))
            .min_w(px(258.0))
            .h_full()
            .border_r_1()
            .border_color(rgb(COLOR_BORDER_SUBTLE))
            .bg(rgb(COLOR_PANEL))
            .child(
                v_flex()
                    .h(px(72.0))
                    .justify_center()
                    .px_3()
                    .border_b_1()
                    .border_color(rgb(COLOR_BORDER_SUBTLE))
                    .bg(rgb(COLOR_CHROME))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(COLOR_TEXT))
                            .child("PowerLeaf"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(COLOR_DIM))
                            .child(env!("CARGO_PKG_DESCRIPTION")),
                    ),
            );

        let mut drawer = v_flex().gap_3().p_2();

        for section in Page::sections() {
            let mut group = v_flex().gap_1();
            group = group.child(
                div()
                    .px_2()
                    .pt_1()
                    .text_xs()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(COLOR_DIM))
                    .child(section.label),
            );
            for page in section.pages {
                let selected = self.page == *page;
                let target = *page;
                group = group.child(
                    nav_row(*page, selected)
                        .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                            app.page = target;
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
            drawer = drawer.child(group);
        }

        nav = nav.child(drawer);
        nav.into_any_element()
    }

    fn render_status_bar(&self) -> AnyElement {
        h_flex()
            .h(px(38.0))
            .items_center()
            .gap_2()
            .px_4()
            .border_t_1()
            .border_color(rgb(COLOR_BORDER_SUBTLE))
            .bg(rgb(COLOR_CHROME))
            .text_sm()
            .child(text_muted(&self.status_message))
            .child(div().text_color(rgb(COLOR_DIM)).child("|"))
            .child(text_muted(&self.decision.reason))
            .into_any_element()
    }

    fn render_unsaved_popup(&self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .absolute()
            .right(px(24.0))
            .bottom(px(54.0))
            .w(px(372.0))
            .occlude()
            .on_any_mouse_down(|_, _, cx| {
                cx.stop_propagation();
            })
            .gap_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(rgb(COLOR_WARNING_BG))
            .bg(rgb(COLOR_PANEL))
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(8.0)).rounded_full().bg(rgb(COLOR_WARNING)))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(COLOR_TEXT))
                            .child("Unsaved settings"),
                    ),
            )
            .child(text_muted(
                "Save these changes before leaving them in draft state.",
            ))
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("discard-settings")
                            .small()
                            .label("Discard")
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.cancel_settings_changes(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("save-settings")
                            .small()
                            .primary()
                            .label("Save")
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.save_settings();
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.page {
            Page::Dashboard => self.render_dashboard(),
            Page::Activity => self.render_activity_page(cx),
            Page::ForegroundRules => self.render_foreground_rules_page(window, cx),
            Page::Schedule => self.render_schedule_page(window, cx),
            Page::CpuUsage => self.render_cpu_usage_page(window, cx),
            Page::EfficiencyMode => self.render_efficiency_page(window, cx),
            Page::AppSuspension => self.render_suspension_page(window, cx),
            Page::Settings => self.render_settings_page(window, cx),
            Page::About => self.render_about_page(),
        }
    }

    fn render_dashboard(&self) -> AnyElement {
        let settings = self.runtime_settings();
        page_shell(Page::Dashboard)
            .child(info_card(vec![
                "Dashboard summarizes the current automation decision, detected state, and active power plan.",
                "Use it to verify what PowerLeaf is doing before changing page settings.",
            ]))
            .child(
                stat_grid(vec![
                    (
                        "Current power plan",
                        self.current_plan
                            .as_ref()
                            .map(|plan| plan.name.as_str())
                            .unwrap_or("Unknown")
                            .to_owned(),
                    ),
                    ("Current mode", self.decision.state.label().to_owned()),
                    (
                        "Automation",
                        if settings.general.enabled {
                            "Enabled"
                        } else {
                            "Disabled"
                        }
                        .to_owned(),
                    ),
                    (
                        "Foreground app",
                        self.foreground_app
                            .as_deref()
                            .unwrap_or("Unknown")
                            .to_owned(),
                    ),
                    ("Activity state", format!("{:?}", self.activity.state)),
                    ("CPU usage", cpu_usage_label(self.cpu_usage.percent)),
                    ("Efficiency Mode", eco_qos_label(&self.eco_qos_status)),
                    (
                        "App Suspension",
                        app_suspension_label(&self.app_suspension_status),
                    ),
                    (
                        "Idle time",
                        self.activity
                            .idle_for
                            .map(|duration| ui::duration_label(duration.as_secs()))
                            .unwrap_or_else(|| "Unknown".to_owned()),
                    ),
                    ("Time rules", self.next_schedule.clone()),
                    ("Decision reason", self.decision.reason.clone()),
                ])
                .into_any_element(),
            )
            .into_any_element()
    }

    fn render_activity_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let enabled = self.settings.activity_mode.enabled;
        page_shell(Page::Activity)
            .child(info_card(vec![
                "Action Based Scheduler switches power plans from keyboard and mouse activity.",
                "Idle and active plans are selected below; input detection controls when the app changes state.",
            ]))
            .child(checkbox(
                "activity-enabled",
                "Enable action-based scheduler",
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.activity_mode.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(text_muted(format!(
                "Current active plan: {}",
                self.current_plan
                    .as_ref()
                    .map(|plan| plan.name.as_str())
                    .unwrap_or("Unknown")
            )))
            .child(self.render_power_plan_picker(
                "activity-idle-plan",
                "Idle plan",
                self.settings.activity_mode.power_plans.power_save_guid.clone(),
                PowerPlanField::ActivityKind(PowerPlanKind::Idle),
                cx,
            ))
            .child(self.render_power_plan_picker(
                "activity-active-plan",
                "Active plan",
                self.settings
                    .activity_mode
                    .power_plans
                    .performance_guid
                    .clone(),
                PowerPlanField::ActivityKind(PowerPlanKind::Active),
                cx,
            ))
            .child(checkbox(
                "keyboard-input",
                "Keyboard input",
                self.settings.activity_mode.input_detection.keyboard,
                cx.listener(|app, checked: &bool, _, cx| {
                    if !*checked && !app.settings.activity_mode.input_detection.mouse {
                        return;
                    }
                    app.settings.activity_mode.input_detection.keyboard = *checked;
                    app.settings
                        .activity_mode
                        .input_detection
                        .ensure_any_enabled();
                    app.settings.activity_mode.switch_to_performance_on_resume =
                        app.settings.activity_mode.input_detection.any_enabled();
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "mouse-input",
                "Mouse input",
                self.settings.activity_mode.input_detection.mouse,
                cx.listener(|app, checked: &bool, _, cx| {
                    if !*checked && !app.settings.activity_mode.input_detection.keyboard {
                        return;
                    }
                    app.settings.activity_mode.input_detection.mouse = *checked;
                    app.settings
                        .activity_mode
                        .input_detection
                        .ensure_any_enabled();
                    app.settings.activity_mode.switch_to_performance_on_resume =
                        app.settings.activity_mode.input_detection.any_enabled();
                    cx.notify();
                }),
            ))
            .child(stepper_u64(
                "activity-idle-timeout",
                "Idle timeout",
                self.settings.activity_mode.idle_timeout_seconds,
                " sec",
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    app.settings.activity_mode.idle_timeout_seconds = apply_u64_step(
                        app.settings.activity_mode.idle_timeout_seconds,
                        change,
                        1,
                        7_200,
                    );
                    cx.notify();
                }),
            ))
            .child(stepper_u64(
                "general-check-interval",
                "Check interval",
                self.settings.general.check_interval_ms,
                " ms",
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    app.settings.general.check_interval_ms = apply_u64_step(
                        app.settings.general.check_interval_ms,
                        change,
                        250,
                        60_000,
                    );
                    cx.notify();
                }),
            ))
            .into_any_element()
    }

    fn render_foreground_rules_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut content = page_shell(Page::ForegroundRules)
            .child(info_card(vec![
                "Foreground Rules switch power plans when a specific app is focused.",
                "Rules are matched against running process names and can use any Windows power plan.",
            ]))
            .child(checkbox(
                "foreground-enabled",
                "Enable foreground rules",
                self.settings.foreground_rules.enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.foreground_rules.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(
                Button::new("add-foreground-rule")
                    .small()
                    .primary()
                    .label("Add foreground rule")
                    .on_click(cx.listener(|app, _, window, cx| {
                        app.settings.foreground_rules.rules.push(ForegroundRule {
                            enabled: true,
                            name: "New Foreground Rule".to_owned(),
                            process_name: String::new(),
                            power_plan_guid: app
                                .current_plan
                                .as_ref()
                                .map(|plan| plan.guid.clone()),
                        });
                        app.inputs.ensure_for_settings(window, cx, &app.settings);
                        cx.notify();
                    })),
            );

        let mut rules = rule_list();
        for (index, rule) in self.settings.foreground_rules.rules.iter().enumerate() {
            rules = rules.child(self.render_foreground_rule(index, rule, window, cx));
        }
        content = content.child(rules);

        content.into_any_element()
    }

    fn render_foreground_rule(
        &self,
        index: usize,
        rule: &ForegroundRule,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(name_input) = self.inputs.foreground_rule_names.get(index).cloned() else {
            return syncing_rule_card(index);
        };
        let Some(process_input) = self.inputs.foreground_rule_processes.get(index).cloned() else {
            return syncing_rule_card(index);
        };
        let title_target = RuleTitleTarget::Foreground(index);
        let card_target = RuleCardTarget::Foreground(index);
        let collapsed = self.is_rule_card_collapsed(&card_target);
        let mut card = rule_card(
            self.render_rule_title(rule_card_title(&rule.name), &name_input, title_target, cx),
            rule_enable_checkbox(
                format!("foreground-rule-enabled-{index}"),
                rule.enabled,
                cx.listener(move |app, checked, _, cx| {
                    if let Some(rule) = app.settings.foreground_rules.rules.get_mut(index) {
                        rule.enabled = *checked;
                    }
                    cx.notify();
                }),
            ),
            rule_card_toggle_button(
                format!("toggle-foreground-rule-{index}"),
                collapsed,
                cx.listener({
                    let card_target = card_target.clone();
                    move |app, _, _, cx| app.toggle_rule_card(card_target.clone(), cx)
                }),
            ),
            card_target.clone(),
            cx,
        );
        if !collapsed {
            card = card
                .child(labeled_element(
                    "Focused app",
                    self.render_process_picker(
                        format!("foreground-process-{index}"),
                        &process_input,
                        SuggestionTarget::ForegroundRule(index),
                        window,
                        cx,
                    ),
                ))
                .child(self.render_power_plan_picker(
                    format!("foreground-rule-plan-{index}"),
                    "Target power plan",
                    rule.power_plan_guid.clone(),
                    PowerPlanField::ForegroundRule(index),
                    cx,
                ))
                .child(
                    Button::new(SharedString::from(format!(
                        "remove-foreground-rule-{index}"
                    )))
                    .small()
                    .danger()
                    .label("Remove")
                    .on_click(cx.listener(move |app, _, _, cx| {
                        if index < app.settings.foreground_rules.rules.len() {
                            app.settings.foreground_rules.rules.remove(index);
                        }
                        app.editing_rule_title = None;
                        app.collapsed_rule_cards.clear();
                        cx.notify();
                    })),
                );
        }
        card.into_any_element()
    }

    fn render_rule_title(
        &self,
        title: &str,
        input: &Entity<InputState>,
        target: RuleTitleTarget,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.editing_rule_title == Some(target) {
            return h_flex()
                .flex_1()
                .min_w(px(180.0))
                .max_w(px(460.0))
                .items_center()
                .gap_2()
                .on_action(cx.listener(move |app, _: &InputEscape, _, cx| {
                    app.finish_rule_title_edit(target, cx);
                }))
                .on_mouse_down_out(cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                    app.finish_rule_title_edit(target, cx);
                }))
                .child(Input::new(input).w_full())
                .child(
                    Button::new(SharedString::from(format!(
                        "finish-rule-title-edit-{target:?}"
                    )))
                    .small()
                    .primary()
                    .label("Done")
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.finish_rule_title_edit(target, cx);
                    })),
                )
                .into_any_element();
        }

        h_flex()
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .items_center()
            .gap_1()
            .child(
                div()
                    .id(SharedString::from(format!("rule-title-{target:?}")))
                    .flex_none()
                    .max_w(px(420.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(16.0))
                    .line_height(px(22.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(COLOR_TEXT))
                    .cursor_pointer()
                    .child(title.to_owned()),
            )
            .child(
                Button::new(SharedString::from(format!("edit-rule-title-{target:?}")))
                    .small()
                    .ghost()
                    .label("Edit")
                    .tooltip("Rename rule")
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.begin_rule_title_edit(target, window, cx);
                    })),
            )
            .into_any_element()
    }

    fn render_schedule_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let mut content = page_shell(Page::Schedule)
            .child(info_card(vec![
                "Time rules switch power plans based on the current day and time.",
                "Each rule can use its own target plan; overnight ranges are supported.",
            ]))
            .child(checkbox(
                "schedule-enabled",
                "Enable time rules",
                self.settings.schedule_mode.enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.schedule_mode.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(
                Button::new("add-time-rule")
                    .small()
                    .primary()
                    .label("Add time rule")
                    .on_click(cx.listener(|app, _, window, cx| {
                        app.settings.schedule_mode.rules.push(ScheduleRule {
                            enabled: true,
                            name: "New Time Rule".to_owned(),
                            days: WeekdaySetting::all().to_vec(),
                            start_time: "22:00".to_owned(),
                            end_time: "08:00".to_owned(),
                            power_plan_guid: app
                                .current_plan
                                .as_ref()
                                .map(|plan| plan.guid.clone()),
                            power_save_guid: None,
                            performance_guid: None,
                        });
                        app.inputs.ensure_for_settings(window, cx, &app.settings);
                        cx.notify();
                    })),
            );

        let mut rules = rule_list();
        for (index, rule) in self.settings.schedule_mode.rules.iter().enumerate() {
            rules = rules.child(self.render_schedule_rule(index, rule, window, cx));
        }
        content = content.child(rules);

        content.into_any_element()
    }

    fn render_schedule_rule(
        &self,
        index: usize,
        rule: &ScheduleRule,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(name_input) = self.inputs.schedule_rule_names.get(index).cloned() else {
            return syncing_rule_card(index);
        };
        let mut days = h_flex().gap_1().flex_wrap();
        for day in WeekdaySetting::all() {
            let selected = rule.days.contains(&day);
            days = days.child(
                toggle_button(
                    format!("schedule-day-{index}-{}", day.short_label()),
                    day.short_label(),
                    selected,
                )
                .on_click(cx.listener(move |app, _, _, cx| {
                    if let Some(rule) = app.settings.schedule_mode.rules.get_mut(index) {
                        if rule.days.contains(&day) {
                            rule.days.retain(|existing| *existing != day);
                        } else {
                            rule.days.push(day);
                        }
                    }
                    cx.notify();
                })),
            );
        }

        let title_target = RuleTitleTarget::Schedule(index);
        let card_target = RuleCardTarget::Schedule(index);
        let collapsed = self.is_rule_card_collapsed(&card_target);
        let mut card = rule_card(
            self.render_rule_title(rule_card_title(&rule.name), &name_input, title_target, cx),
            rule_enable_checkbox(
                format!("schedule-rule-enabled-{index}"),
                rule.enabled,
                cx.listener(move |app, checked, _, cx| {
                    if let Some(rule) = app.settings.schedule_mode.rules.get_mut(index) {
                        rule.enabled = *checked;
                    }
                    cx.notify();
                }),
            ),
            rule_card_toggle_button(
                format!("toggle-schedule-rule-{index}"),
                collapsed,
                cx.listener({
                    let card_target = card_target.clone();
                    move |app, _, _, cx| app.toggle_rule_card(card_target.clone(), cx)
                }),
            ),
            card_target.clone(),
            cx,
        );
        if !collapsed {
            card = card
                .child(labeled_element("Days", days.into_any_element()))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .child(match self.inputs.schedule_start_times.get(index).cloned() {
                            Some(input) => input_row("Start", input).into_any_element(),
                            None => syncing_input_message().into_any_element(),
                        })
                        .child(match self.inputs.schedule_end_times.get(index).cloned() {
                            Some(input) => input_row("End", input).into_any_element(),
                            None => syncing_input_message().into_any_element(),
                        })
                        .child(if rule.parsed_times().is_none() {
                            text_danger("Use HH:MM").into_any_element()
                        } else {
                            div().into_any_element()
                        }),
                )
                .child(self.render_power_plan_picker(
                    format!("schedule-rule-plan-{index}"),
                    "Target power plan",
                    rule.power_plan_guid.clone(),
                    PowerPlanField::ScheduleRule(index),
                    cx,
                ))
                .child(
                    Button::new(SharedString::from(format!("remove-schedule-rule-{index}")))
                        .small()
                        .danger()
                        .label("Remove")
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if index < app.settings.schedule_mode.rules.len() {
                                app.settings.schedule_mode.rules.remove(index);
                            }
                            app.editing_rule_title = None;
                            app.collapsed_rule_cards.clear();
                            cx.notify();
                        })),
                );
        }
        card.into_any_element()
    }

    fn render_cpu_usage_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let mut content = page_shell(Page::CpuUsage)
            .child(info_card(vec![
                "CPU Load Rules switch power plans when processor usage crosses a threshold for a duration.",
                "Use else plans for fallback behavior when a CPU rule is no longer active.",
            ]))
            .child(checkbox(
                "cpu-usage-enabled",
                "Enable CPU load rules",
                self.settings.cpu_usage_mode.enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.cpu_usage_mode.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(
                Button::new("add-cpu-rule")
                    .small()
                    .primary()
                    .label("Add CPU load rule")
                    .on_click(cx.listener(|app, _, window, cx| {
                        app.settings.cpu_usage_mode.rules.push(CpuUsageRule {
                            enabled: true,
                            name: "New CPU Load Rule".to_owned(),
                            comparison: CpuUsageComparison::AtOrBelow,
                            threshold_percent: 20,
                            upper_threshold_percent: None,
                            duration_seconds: 30,
                            power_plan_guid: app
                                .current_plan
                                .as_ref()
                                .map(|plan| plan.guid.clone()),
                            else_enabled: false,
                            else_power_plan_guid: app
                                .current_plan
                                .as_ref()
                                .map(|plan| plan.guid.clone()),
                            target: None,
                        });
                        app.inputs.ensure_for_settings(window, cx, &app.settings);
                        cx.notify();
                    })),
            );

        let mut rules = rule_list();
        for (index, rule) in self.settings.cpu_usage_mode.rules.iter().enumerate() {
            rules = rules.child(self.render_cpu_rule(index, rule, window, cx));
        }
        content = content.child(rules);

        content.into_any_element()
    }

    fn render_cpu_rule(
        &self,
        index: usize,
        rule: &CpuUsageRule,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(name_input) = self.inputs.cpu_rule_names.get(index).cloned() else {
            return syncing_rule_card(index);
        };
        let mut comparisons = h_flex().gap_1().flex_wrap();
        for comparison in [
            CpuUsageComparison::AtOrBelow,
            CpuUsageComparison::AtOrAbove,
            CpuUsageComparison::Between,
        ] {
            comparisons = comparisons.child(
                toggle_button(
                    format!("cpu-comparison-{index}-{:?}", comparison),
                    comparison.label(),
                    rule.comparison == comparison,
                )
                .on_click(cx.listener(move |app, _, _, cx| {
                    if let Some(rule) = app.settings.cpu_usage_mode.rules.get_mut(index) {
                        rule.comparison = comparison;
                        if comparison == CpuUsageComparison::Between {
                            rule.upper_threshold_percent.get_or_insert(100);
                        }
                    }
                    cx.notify();
                })),
            );
        }

        let upper = rule.upper_threshold_percent.unwrap_or(100);
        let title_target = RuleTitleTarget::Cpu(index);
        let card_target = RuleCardTarget::Cpu(index);
        let collapsed = self.is_rule_card_collapsed(&card_target);
        let mut card = rule_card(
            self.render_rule_title(rule_card_title(&rule.name), &name_input, title_target, cx),
            rule_enable_checkbox(
                format!("cpu-rule-enabled-{index}"),
                rule.enabled,
                cx.listener(move |app, checked, _, cx| {
                    if let Some(rule) = app.settings.cpu_usage_mode.rules.get_mut(index) {
                        rule.enabled = *checked;
                    }
                    cx.notify();
                }),
            ),
            rule_card_toggle_button(
                format!("toggle-cpu-rule-{index}"),
                collapsed,
                cx.listener({
                    let card_target = card_target.clone();
                    move |app, _, _, cx| app.toggle_rule_card(card_target.clone(), cx)
                }),
            ),
            card_target.clone(),
            cx,
        );
        if !collapsed {
            card = card
                .child(labeled_element(
                    "When CPU load",
                    comparisons.into_any_element(),
                ))
                .child(stepper_u8(
                    format!("cpu-rule-threshold-{index}"),
                    "Threshold",
                    rule.threshold_percent,
                    "%",
                    cx.listener(move |app, change: &StepChange<u8>, _, cx| {
                        if let Some(rule) = app.settings.cpu_usage_mode.rules.get_mut(index) {
                            rule.threshold_percent =
                                apply_u8_step(rule.threshold_percent, change, 0, 100);
                        }
                        cx.notify();
                    }),
                ))
                .child(if rule.comparison == CpuUsageComparison::Between {
                    stepper_u8(
                        format!("cpu-rule-upper-threshold-{index}"),
                        "Upper threshold",
                        upper,
                        "%",
                        cx.listener(move |app, change: &StepChange<u8>, _, cx| {
                            if let Some(rule) = app.settings.cpu_usage_mode.rules.get_mut(index) {
                                let value = rule.upper_threshold_percent.unwrap_or(100);
                                rule.upper_threshold_percent =
                                    Some(apply_u8_step(value, change, 0, 100));
                            }
                            cx.notify();
                        }),
                    )
                    .into_any_element()
                } else {
                    div().into_any_element()
                })
                .child(stepper_u64(
                    format!("cpu-rule-duration-{index}"),
                    "Duration",
                    rule.duration_seconds,
                    " sec",
                    cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                        if let Some(rule) = app.settings.cpu_usage_mode.rules.get_mut(index) {
                            rule.duration_seconds =
                                apply_u64_step(rule.duration_seconds, change, 0, 86_400);
                        }
                        cx.notify();
                    }),
                ))
                .child(self.render_power_plan_picker(
                    format!("cpu-rule-plan-{index}"),
                    "Use",
                    rule.power_plan_guid.clone(),
                    PowerPlanField::CpuRule(index),
                    cx,
                ))
                .child(checkbox(
                    format!("cpu-rule-else-{index}"),
                    "Else",
                    rule.else_enabled,
                    cx.listener(move |app, checked, _, cx| {
                        let current_plan = app.current_plan.as_ref().map(|plan| plan.guid.clone());
                        if let Some(rule) = app.settings.cpu_usage_mode.rules.get_mut(index) {
                            rule.else_enabled = *checked;
                            if rule.else_enabled && rule.else_power_plan_guid.is_none() {
                                rule.else_power_plan_guid = current_plan;
                            }
                        }
                        cx.notify();
                    }),
                ))
                .child(if rule.else_enabled {
                    self.render_power_plan_picker(
                        format!("cpu-rule-else-plan-{index}"),
                        "Else use",
                        rule.else_power_plan_guid.clone(),
                        PowerPlanField::CpuRuleElse(index),
                        cx,
                    )
                } else {
                    div().into_any_element()
                })
                .child(
                    Button::new(SharedString::from(format!("remove-cpu-rule-{index}")))
                        .small()
                        .danger()
                        .label("Remove")
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if index < app.settings.cpu_usage_mode.rules.len() {
                                app.settings.cpu_usage_mode.rules.remove(index);
                            }
                            app.editing_rule_title = None;
                            app.collapsed_rule_cards.clear();
                            cx.notify();
                        })),
                );
        }
        card.into_any_element()
    }

    fn render_efficiency_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let input_value = self.inputs.eco_qos_exclusion.read(cx).value().to_string();
        page_shell(Page::EfficiencyMode)
            .child(info_card(vec![
                "Efficiency Mode applies Windows EcoQoS to background apps to reduce CPU power use.",
                "PowerLeaf also lowers the target app's process priority while Efficiency Mode is active.",
                "This is safer than App Suspension because apps keep running.",
            ]))
            .child(checkbox(
                "eco-qos-enabled",
                "Enable Windows EcoQoS",
                self.settings.eco_qos.enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.eco_qos.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "eco-qos-foreground",
                "App focus detection",
                self.settings.eco_qos.exclude_foreground_app,
                cx.listener(|app, checked, _, cx| {
                    app.settings.eco_qos.exclude_foreground_app = *checked;
                    cx.notify();
                }),
            ))
            .child(stat_grid(vec![
                ("Status", self.eco_qos_status.message.clone()),
                (
                    "Throttled processes",
                    self.eco_qos_status.throttled_processes.to_string(),
                ),
                (
                    "Scanned processes",
                    self.eco_qos_status.scanned_processes.to_string(),
                ),
                (
                    "Skipped processes",
                    self.eco_qos_status.skipped_processes.to_string(),
                ),
                (
                    "Failed actions",
                    self.eco_qos_status.failed_processes.to_string(),
                ),
                (
                    "Last failure",
                    self.eco_qos_status
                        .last_error
                        .as_deref()
                        .unwrap_or("None")
                        .to_owned(),
                ),
            ]))
            .child(
                section_card("Efficiency Whitelist")
                    .child(text_muted(
                        "Apps in this whitelist will never be put into Efficiency Mode.",
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_start()
                            .flex_wrap()
                            .child(self.render_process_picker(
                                "eco-qos-suggestion",
                                &self.inputs.eco_qos_exclusion,
                                SuggestionTarget::EcoQos,
                                window,
                                cx,
                            ))
                            .child(
                                Button::new("add-eco-qos-exclusion")
                                    .small()
                                    .label("Add")
                                    .disabled(!can_add_eco_qos_process(
                                        &self.settings.eco_qos,
                                        &input_value,
                                    ))
                                    .on_click(cx.listener(|app, _, window, cx| {
                                        let process = app
                                            .inputs
                                            .eco_qos_exclusion
                                            .read(cx)
                                            .value()
                                            .to_string();
                                        if can_add_eco_qos_process(&app.settings.eco_qos, &process) {
                                            app.settings
                                                .eco_qos
                                                .efficiency_whitelist
                                                .push(process.trim().to_ascii_lowercase());
                                            clear_input(&app.inputs.eco_qos_exclusion, window, cx);
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(self.render_eco_qos_whitelist(cx)),
            )
            .into_any_element()
    }

    fn render_eco_qos_whitelist(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut list = v_flex().gap_2();
        for (index, process) in self
            .settings
            .eco_qos
            .efficiency_whitelist
            .iter()
            .enumerate()
        {
            list = list.child(
                row_card()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .child(process.clone()),
                    )
                    .child(
                        Button::new(SharedString::from(format!("remove-eco-qos-{index}")))
                            .small()
                            .danger()
                            .label("Remove")
                            .on_click(cx.listener(move |app, _, _, cx| {
                                if index < app.settings.eco_qos.efficiency_whitelist.len() {
                                    app.settings.eco_qos.efficiency_whitelist.remove(index);
                                }
                                cx.notify();
                            })),
                    ),
            );
        }
        if self.settings.eco_qos.efficiency_whitelist.is_empty() {
            list = list.child(text_muted("No apps are whitelisted."));
        }
        list.into_any_element()
    }

    fn render_suspension_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let input_value = self.inputs.suspension_process.read(cx).value().to_string();
        page_shell(Page::AppSuspension)
            .child(info_card(vec![
                "App Suspension pauses selected background apps after a delay to reduce CPU usage.",
                "Suspended apps are resumed automatically when you switch back to them or quit PowerLeaf.",
                "Use it only for apps that are safe to pause in the background.",
            ]))
            .child(checkbox(
                "app-suspension-enabled",
                "Enable app suspension",
                self.settings.app_suspension.enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.app_suspension.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(stepper_u64(
                "suspension-background-delay",
                "Background delay",
                self.settings.app_suspension.background_delay_seconds,
                " sec",
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    app.settings.app_suspension.background_delay_seconds =
                        apply_u64_step(app.settings.app_suspension.background_delay_seconds, change, 1, 86_400);
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "temporary-thaw",
                "Temporary thaw fallback",
                self.settings.app_suspension.temporary_thaw_enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.app_suspension.temporary_thaw_enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(stepper_u64(
                "suspension-thaw-interval",
                "Thaw every",
                self.settings
                    .app_suspension
                    .temporary_thaw_interval_seconds,
                " sec",
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    app.settings.app_suspension.temporary_thaw_interval_seconds =
                        apply_u64_step(app.settings.app_suspension.temporary_thaw_interval_seconds, change, 1, 86_400);
                    cx.notify();
                }),
            ))
            .child(stepper_u64(
                "suspension-thaw-duration",
                "Thaw duration",
                self.settings
                    .app_suspension
                    .temporary_thaw_duration_seconds,
                " sec",
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    app.settings.app_suspension.temporary_thaw_duration_seconds =
                        apply_u64_step(app.settings.app_suspension.temporary_thaw_duration_seconds, change, 1, 3_600);
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "audio-wake",
                "Audio playback detection",
                self.settings.app_suspension.audio_wake_enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.app_suspension.audio_wake_enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(stepper_u64(
                "suspension-audio-refreeze",
                "Audio refreeze after",
                self.settings.app_suspension.audio_wake_duration_seconds,
                " sec quiet",
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    app.settings.app_suspension.audio_wake_duration_seconds =
                        apply_u64_step(app.settings.app_suspension.audio_wake_duration_seconds, change, 1, 3_600);
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "network-wake",
                "Network intent detection",
                self.settings.app_suspension.network_wake_enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.app_suspension.network_wake_enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(stepper_u64(
                "suspension-network-refreeze",
                "Network refreeze after",
                self.settings.app_suspension.network_wake_duration_seconds,
                " sec quiet",
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    app.settings.app_suspension.network_wake_duration_seconds =
                        apply_u64_step(app.settings.app_suspension.network_wake_duration_seconds, change, 1, 3_600);
                    cx.notify();
                }),
            ))
            .child(stat_grid(vec![
                ("Status", self.app_suspension_status.message.clone()),
                (
                    "Tracked processes",
                    self.app_suspension_status.tracked_processes.to_string(),
                ),
                (
                    "Suspended processes",
                    self.app_suspension_status
                        .suspended_processes
                        .to_string(),
                ),
                (
                    "Temporary thawed",
                    self.app_suspension_status
                        .temporary_thawed_processes
                        .to_string(),
                ),
                (
                    "Network wake",
                    self.app_suspension_status
                        .network_wake_processes
                        .to_string(),
                ),
                (
                    "Audio wake",
                    self.app_suspension_status.audio_wake_processes.to_string(),
                ),
                (
                    "Skipped processes",
                    self.app_suspension_status.skipped_processes.to_string(),
                ),
                (
                    "Failed actions",
                    self.app_suspension_status.failed_actions.to_string(),
                ),
                (
                    "Last failure",
                    self.app_suspension_status
                        .last_error
                        .as_deref()
                        .unwrap_or("None")
                        .to_owned(),
                ),
            ]))
            .child(
                section_card("Suspendable Apps")
                    .child(text_muted(
                        "Only apps in this list can be suspended after the background delay.",
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_start()
                            .flex_wrap()
                            .child(self.render_process_picker(
                                "suspension-suggestion",
                                &self.inputs.suspension_process,
                                SuggestionTarget::Suspension,
                                window,
                                cx,
                            ))
                            .child(
                                Button::new("add-suspension-process")
                                    .small()
                                    .label("Add")
                                    .disabled(!can_add_suspension_process(
                                        &self.settings.app_suspension,
                                        &input_value,
                                    ))
                                    .on_click(cx.listener(|app, _, window, cx| {
                                        let process = app
                                            .inputs
                                            .suspension_process
                                            .read(cx)
                                            .value()
                                            .to_string();
                                        if can_add_suspension_process(
                                            &app.settings.app_suspension,
                                            &process,
                                        ) {
                                            app.settings
                                                .app_suspension
                                                .suspendable_apps
                                                .push(new_suspension_rule(&process));
                                            clear_input(&app.inputs.suspension_process, window, cx);
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(self.render_suspendable_apps(cx)),
            )
            .into_any_element()
    }

    fn render_suspendable_apps(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut list = rule_list();
        for (index, rule) in self
            .settings
            .app_suspension
            .suspendable_apps
            .iter()
            .enumerate()
        {
            let process = rule.process_name.clone();
            let indicator = suspension_indicator(&self.app_suspension_status, &process);
            let card_target = RuleCardTarget::Suspension(process.clone());
            let collapsed = self.is_rule_card_collapsed(&card_target);
            let mut card = rule_card(
                static_rule_title(&process),
                status_pill(indicator.label, indicator.bg, indicator.fg),
                rule_card_toggle_button(
                    format!("toggle-suspension-rule-{index}"),
                    collapsed,
                    cx.listener({
                        let card_target = card_target.clone();
                        move |app, _, _, cx| app.toggle_rule_card(card_target.clone(), cx)
                    }),
                ),
                card_target.clone(),
                cx,
            );
            if !collapsed {
                card = card
                    .child(text_muted(indicator.hover))
                    .child(
                        Button::new(SharedString::from(format!("freeze-suspension-{index}")))
                            .small()
                            .label("Freeze")
                            .disabled(!can_manual_freeze(&self.app_suspension_status, &process))
                            .on_click(cx.listener({
                                let process = process.clone();
                                move |app, _, _, cx| {
                                    app.background_automation
                                        .request_app_suspension_freeze(&process);
                                    app.status_message =
                                        format!("Manual freeze requested for {process}.");
                                    cx.notify();
                                }
                            })),
                    )
                    .child(checkbox(
                        format!("suspension-network-rule-{index}"),
                        "Network Detection",
                        rule.network_wake_enabled,
                        cx.listener(move |app, checked, _, cx| {
                            if let Some(rule) =
                                app.settings.app_suspension.suspendable_apps.get_mut(index)
                            {
                                rule.network_wake_enabled = *checked;
                            }
                            cx.notify();
                        }),
                    ))
                    .child(checkbox(
                        format!("suspension-audio-rule-{index}"),
                        "Audio Detection",
                        rule.audio_wake_enabled,
                        cx.listener(move |app, checked, _, cx| {
                            if let Some(rule) =
                                app.settings.app_suspension.suspendable_apps.get_mut(index)
                            {
                                rule.audio_wake_enabled = *checked;
                            }
                            cx.notify();
                        }),
                    ))
                    .child(self.render_network_threshold(
                        index,
                        true,
                        "Download Threshold",
                        rule.network_download_threshold_bytes,
                        rule.network_download_threshold_unit,
                        ThresholdField::Download(index),
                        cx,
                    ))
                    .child(self.render_network_threshold(
                        index,
                        false,
                        "Upload Threshold",
                        rule.network_upload_threshold_bytes,
                        rule.network_upload_threshold_unit,
                        ThresholdField::Upload(index),
                        cx,
                    ))
                    .child(
                        Button::new(SharedString::from(format!("remove-suspension-{index}")))
                            .small()
                            .danger()
                            .label("Remove")
                            .on_click(cx.listener({
                                let card_target = card_target.clone();
                                move |app, _, _, cx| {
                                    if index < app.settings.app_suspension.suspendable_apps.len() {
                                        app.settings.app_suspension.suspendable_apps.remove(index);
                                    }
                                    app.collapsed_rule_cards.remove(&card_target);
                                    cx.notify();
                                }
                            })),
                    );
            }
            list = list.child(card);
        }
        if self.settings.app_suspension.suspendable_apps.is_empty() {
            list = list.child(text_muted("No apps are suspendable."));
        }
        list.into_any_element()
    }

    fn render_network_threshold(
        &self,
        _index: usize,
        _download: bool,
        label: &str,
        threshold_bytes: u64,
        unit: NetworkThresholdUnit,
        field: ThresholdField,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let value = unit.threshold_value_from_bytes(threshold_bytes);
        let value_label = if threshold_bytes == 0 {
            "Unlimited".to_owned()
        } else {
            format!("{value:.3} {}", unit.label())
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_owned()
        };
        labeled_element(
            label,
            h_flex()
                .gap_2()
                .items_center()
                .flex_wrap()
                .child(
                    Button::new(SharedString::from(format!("threshold-down-{:?}", field)))
                        .small()
                        .label("-")
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.adjust_threshold(field, false);
                            cx.notify();
                        })),
                )
                .child(value_pill(value_label))
                .child(
                    Button::new(SharedString::from(format!("threshold-up-{:?}", field)))
                        .small()
                        .label("+")
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.adjust_threshold(field, true);
                            cx.notify();
                        })),
                )
                .child(self.render_network_unit_picker(field, unit, cx))
                .into_any_element(),
        )
        .into_any_element()
    }

    fn adjust_threshold(&mut self, field: ThresholdField, increase: bool) {
        let Some(rule) = self.threshold_rule_mut(field) else {
            return;
        };
        let (bytes, unit) = match field {
            ThresholdField::Download(_) => (
                &mut rule.network_download_threshold_bytes,
                rule.network_download_threshold_unit,
            ),
            ThresholdField::Upload(_) => (
                &mut rule.network_upload_threshold_bytes,
                rule.network_upload_threshold_unit,
            ),
        };
        let current = unit.threshold_value_from_bytes(*bytes);
        let step = network_threshold_step(unit);
        let next = if increase {
            current + step
        } else {
            (current - step).max(0.0)
        };
        *bytes = unit
            .threshold_bytes_from_value(next)
            .min(MAX_NETWORK_THRESHOLD_BYTES);
    }

    fn threshold_rule_mut(&mut self, field: ThresholdField) -> Option<&mut AppSuspensionRule> {
        let index = match field {
            ThresholdField::Download(index) | ThresholdField::Upload(index) => index,
        };
        self.settings.app_suspension.suspendable_apps.get_mut(index)
    }

    fn render_network_unit_picker(
        &self,
        field: ThresholdField,
        selected: NetworkThresholdUnit,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut row = h_flex().gap_1().flex_wrap();
        for unit in NetworkThresholdUnit::ALL {
            row = row.child(
                toggle_button(
                    format!("network-unit-{:?}-{}", field, unit.label()),
                    unit.label(),
                    selected == unit,
                )
                .on_click(cx.listener(move |app, _, _, cx| {
                    if let Some(rule) = app.threshold_rule_mut(field) {
                        match field {
                            ThresholdField::Download(_) => {
                                rule.network_download_threshold_unit = unit
                            }
                            ThresholdField::Upload(_) => rule.network_upload_threshold_unit = unit,
                        }
                    }
                    cx.notify();
                })),
            );
        }
        row.into_any_element()
    }

    fn render_settings_page(&self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        page_shell(Page::Settings)
            .child(info_card(vec![
                "General settings control startup behavior, tray behavior, and the master automation switch.",
                "Export or import the settings file from this page.",
            ]))
            .child(checkbox(
                "general-enabled",
                "PowerLeaf master switch",
                self.settings.general.enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "startup-windows",
                "Start PowerLeaf when Windows starts",
                self.settings.general.startup_with_windows,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.startup_with_windows = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "start-minimized",
                "Start in system tray",
                self.settings.general.start_minimized,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.start_minimized = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "pause-plugged",
                "Stop power plan scheduler on A/C",
                self.settings
                    .general
                    .pause_power_plan_switching_while_plugged_in,
                cx.listener(|app, checked, _, cx| {
                    app.settings
                        .general
                        .pause_power_plan_switching_while_plugged_in = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "hide-to-tray",
                "Hide to system tray on close",
                self.settings.general.hide_to_tray,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.hide_to_tray = *checked;
                    cx.notify();
                }),
            ))
            .child(
                section_card("Settings Files").child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            Button::new("export-settings")
                                .small()
                                .label("Export settings (.toml)")
                                .on_click(cx.listener(|app, _, _, cx| {
                                    app.export_settings_toml();
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("import-settings")
                                .small()
                                .label("Import settings (.toml)")
                                .on_click(cx.listener(|app, _, window, cx| {
                                    app.import_settings_toml(window, cx);
                                    cx.notify();
                                })),
                        ),
                ),
            )
            .into_any_element()
    }

    fn render_about_page(&self) -> AnyElement {
        page_shell(Page::About)
            .child(info_card(vec![
                "PowerLeaf is a local Windows power automation utility.",
                "Version and project details are shown below.",
            ]))
            .child(
                section_card("PowerLeaf")
                    .child(text_muted(env!("CARGO_PKG_DESCRIPTION")))
                    .child(stat_grid(vec![
                        ("Author", "Tatsh Siow".to_owned()),
                        ("Version", env!("CARGO_PKG_VERSION").to_owned()),
                    ])),
            )
            .into_any_element()
    }

    fn render_power_plan_picker(
        &self,
        id: impl Into<String>,
        label: &str,
        selected_guid: Option<String>,
        field: PowerPlanField,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = id.into();
        let is_open = self.active_power_plan_picker.as_deref() == Some(id.as_str());
        let selected_text = match selected_guid.as_deref() {
            Some(guid) => self
                .plans
                .iter()
                .find(|plan| plan.guid.eq_ignore_ascii_case(guid))
                .map(PowerPlan::display_name)
                .unwrap_or_else(|| "Selected plan unavailable".to_owned()),
            None => "Use inherited/default plan".to_owned(),
        };

        let mut options = v_flex()
            .w_full()
            .max_h(px(244.0))
            .overflow_y_scrollbar()
            .gap_1()
            .p_1()
            .rounded_sm()
            .border_1()
            .border_color(rgb(COLOR_BORDER))
            .bg(rgb(0x1f2329));

        options = options.child(power_plan_option_row(
            format!("{id}-default"),
            "Use inherited/default plan".to_owned(),
            selected_guid.is_none(),
            None,
            field,
            cx,
        ));

        if self.plans.is_empty() {
            options = options.child(
                div()
                    .px_2()
                    .py_2()
                    .text_sm()
                    .text_color(rgb(COLOR_DIM))
                    .child("No power plans loaded"),
            );
        } else {
            for plan in &self.plans {
                let selected = selected_guid
                    .as_deref()
                    .is_some_and(|selected| selected.eq_ignore_ascii_case(&plan.guid));
                options = options.child(power_plan_option_row(
                    format!("{id}-{}", plan.guid),
                    plan.display_name(),
                    selected,
                    Some(plan.guid.clone()),
                    field,
                    cx,
                ));
            }
        }

        let control_id = id.clone();
        let picker = v_flex()
            .w_full()
            .max_w(px(372.0))
            .min_w(px(0.0))
            .relative()
            .min_h(px(32.0))
            .child(
                h_flex()
                    .id(SharedString::from(format!("{id}-select-control")))
                    .h(px(32.0))
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_2()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(COLOR_BORDER))
                    .bg(rgb(COLOR_PANEL))
                    .text_sm()
                    .text_color(rgb(COLOR_TEXT))
                    .hover(|style| style.bg(rgb(COLOR_PANEL_ALT)))
                    .cursor_pointer()
                    .child(div().flex_1().min_w(px(0.0)).child(selected_text))
                    .child(div().text_color(rgb(COLOR_DIM)).child("v"))
                    .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                        app.refresh_power_plans();
                        app.active_power_plan_picker = (app.active_power_plan_picker.as_deref()
                            != Some(control_id.as_str()))
                        .then_some(control_id.clone());
                        cx.notify();
                    })),
            )
            .child(if is_open {
                deferred(
                    div()
                        .absolute()
                        .top(px(34.0))
                        .left(px(0.0))
                        .right(px(0.0))
                        .occlude()
                        .on_mouse_down_out(cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                            app.active_power_plan_picker = None;
                            cx.notify();
                        }))
                        .child(options),
                )
                .with_priority(PROCESS_PICKER_LAYER_PRIORITY)
                .into_any_element()
            } else {
                div().into_any_element()
            });

        labeled_element(label, picker.into_any_element()).into_any_element()
    }

    fn set_power_plan_field(&mut self, field: PowerPlanField, guid: Option<String>) {
        match field {
            PowerPlanField::ActivityKind(PowerPlanKind::Idle) => {
                self.settings.activity_mode.power_plans.power_save_guid = guid
            }
            PowerPlanField::ActivityKind(PowerPlanKind::Active) => {
                self.settings.activity_mode.power_plans.performance_guid = guid
            }
            PowerPlanField::ForegroundRule(index) => {
                if let Some(rule) = self.settings.foreground_rules.rules.get_mut(index) {
                    rule.power_plan_guid = guid;
                }
            }
            PowerPlanField::ScheduleRule(index) => {
                if let Some(rule) = self.settings.schedule_mode.rules.get_mut(index) {
                    rule.power_plan_guid = guid;
                }
            }
            PowerPlanField::CpuRule(index) => {
                if let Some(rule) = self.settings.cpu_usage_mode.rules.get_mut(index) {
                    rule.power_plan_guid = guid;
                }
            }
            PowerPlanField::CpuRuleElse(index) => {
                if let Some(rule) = self.settings.cpu_usage_mode.rules.get_mut(index) {
                    rule.else_power_plan_guid = guid;
                }
            }
        }
    }

    fn render_process_suggestions(
        &self,
        id: impl Into<String>,
        query: &str,
        target: SuggestionTarget,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = id.into();
        let query = query.trim().to_ascii_lowercase();
        let mut matches = self
            .process_candidates
            .iter()
            .filter(|process| {
                query.is_empty() || process.to_ascii_lowercase().contains(query.as_str())
            })
            .filter(|process| process_target_can_accept(target, &self.settings, process))
            .cloned()
            .collect::<Vec<_>>();
        matches.sort();

        let mut suggestions = v_flex()
            .w_full()
            .max_h(px(244.0))
            .overflow_y_scrollbar()
            .gap_1()
            .p_1()
            .rounded_sm()
            .border_1()
            .border_color(rgb(COLOR_BORDER))
            .bg(rgb(0x1f2329));
        if matches.is_empty() {
            suggestions = suggestions.child(
                div()
                    .px_2()
                    .py_2()
                    .text_sm()
                    .text_color(rgb(COLOR_DIM))
                    .child(if self.process_candidates.is_empty() {
                        "No running apps loaded"
                    } else {
                        "No matching apps"
                    }),
            );
        }
        for (count, process) in matches.into_iter().enumerate() {
            suggestions = suggestions.child(
                h_flex()
                    .id(SharedString::from(format!("{id}-{count}")))
                    .h(px(24.0))
                    .items_center()
                    .px_2()
                    .rounded_sm()
                    .text_sm()
                    .text_color(rgb(COLOR_MUTED))
                    .when(count == 0, |row| {
                        row.bg(rgb(COLOR_ACCENT_BG)).text_color(rgb(COLOR_TEXT))
                    })
                    .hover(|style| style.bg(rgb(COLOR_PANEL_ALT)))
                    .cursor_pointer()
                    .child(process.clone())
                    .on_click(cx.listener(move |app, _: &gpui::ClickEvent, window, cx| {
                        app.apply_process_suggestion(target, &process, window, cx);
                        window.blur();
                        cx.notify();
                    })),
            );
        }

        suggestions.into_any_element()
    }

    fn render_process_picker(
        &self,
        id: impl Into<String>,
        input: &Entity<InputState>,
        target: SuggestionTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = id.into();
        let (query, is_open) = {
            let input = input.read(cx);
            (
                input.value().to_string(),
                input.focus_handle(cx).is_focused(window),
            )
        };

        v_flex()
            .w_full()
            .max_w(px(372.0))
            .min_w(px(0.0))
            .relative()
            .min_h(px(32.0))
            .child(Input::new(input).w_full())
            .child(if is_open {
                deferred(
                    div()
                        .absolute()
                        .top(px(34.0))
                        .left(px(0.0))
                        .right(px(0.0))
                        .occlude()
                        .on_mouse_down_out(cx.listener(
                            |_, _: &gpui::MouseDownEvent, window, cx| {
                                window.blur();
                                cx.notify();
                            },
                        ))
                        .child(self.render_process_suggestions(id, &query, target, cx)),
                )
                .with_priority(PROCESS_PICKER_LAYER_PRIORITY)
                .into_any_element()
            } else {
                div().into_any_element()
            })
            .into_any_element()
    }

    fn apply_process_suggestion(
        &mut self,
        target: SuggestionTarget,
        process: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match target {
            SuggestionTarget::ForegroundRule(index) => {
                if let Some(input) = self.inputs.foreground_rule_processes.get(index) {
                    clear_input_to(input, process, window, cx);
                }
            }
            SuggestionTarget::EcoQos => {
                clear_input_to(&self.inputs.eco_qos_exclusion, process, window, cx);
            }
            SuggestionTarget::Suspension => {
                clear_input_to(&self.inputs.suspension_process, process, window, cx);
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum PowerPlanKind {
    Idle,
    Active,
}

#[derive(Debug, Clone, Copy)]
enum PowerPlanField {
    ActivityKind(PowerPlanKind),
    ForegroundRule(usize),
    ScheduleRule(usize),
    CpuRule(usize),
    CpuRuleElse(usize),
}

fn power_plan_option_row(
    id: String,
    label: String,
    selected: bool,
    guid: Option<String>,
    field: PowerPlanField,
    cx: &mut Context<PowerLeafApp>,
) -> AnyElement {
    h_flex()
        .id(SharedString::from(id))
        .h(px(24.0))
        .items_center()
        .px_2()
        .rounded_sm()
        .text_sm()
        .text_color(rgb(COLOR_MUTED))
        .when(selected, |row| {
            row.bg(rgb(COLOR_ACCENT_BG)).text_color(rgb(COLOR_TEXT))
        })
        .hover(|style| style.bg(rgb(COLOR_PANEL_ALT)))
        .cursor_pointer()
        .child(label)
        .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
            app.set_power_plan_field(field, guid.clone());
            app.active_power_plan_picker = None;
            cx.notify();
        }))
        .into_any_element()
}

#[derive(Debug, Clone, Copy)]
enum SuggestionTarget {
    ForegroundRule(usize),
    EcoQos,
    Suspension,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleTitleTarget {
    Foreground(usize),
    Schedule(usize),
    Cpu(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RuleCardTarget {
    Foreground(usize),
    Schedule(usize),
    Cpu(usize),
    Suspension(String),
}

#[derive(Debug, Clone, Copy)]
enum ThresholdField {
    Download(usize),
    Upload(usize),
}

#[derive(Debug, Clone, Copy)]
struct StepChange<T> {
    delta: T,
    increase: bool,
}

fn make_input(
    window: &mut Window,
    cx: &mut Context<PowerLeafApp>,
    value: &str,
    placeholder: &str,
) -> Entity<InputState> {
    let value = SharedString::from(value.to_owned());
    let placeholder = SharedString::from(placeholder.to_owned());
    cx.new(|cx| {
        InputState::new(window, cx)
            .default_value(value)
            .placeholder(placeholder)
    })
}

fn sync_input_vec(
    inputs: &mut Vec<Entity<InputState>>,
    len: usize,
    window: &mut Window,
    cx: &mut Context<PowerLeafApp>,
    value_at: impl Fn(usize) -> String,
    placeholder: &str,
) {
    while inputs.len() < len {
        let index = inputs.len();
        inputs.push(make_input(window, cx, &value_at(index), placeholder));
    }
    inputs.truncate(len);
}

fn clear_input(input: &Entity<InputState>, window: &mut Window, cx: &mut Context<PowerLeafApp>) {
    clear_input_to(input, "", window, cx);
}

fn clear_input_to(
    input: &Entity<InputState>,
    value: &str,
    window: &mut Window,
    cx: &mut Context<PowerLeafApp>,
) {
    let value = SharedString::from(value.to_owned());
    let _ = input.update(cx, |input, cx| input.set_value(value, window, cx));
}

fn page_shell(page: Page) -> gpui::Div {
    v_flex().w_full().min_w(px(0.0)).gap_3().child(
        h_flex()
            .w_full()
            .min_h(px(PAGE_HEADER_HEIGHT))
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .overflow_hidden()
            .child(
                div()
                    .min_w(px(0.0))
                    .text_size(px(24.0))
                    .line_height(px(32.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(COLOR_MUTED))
                    .truncate()
                    .child(page.section_label()),
            )
            .child(
                div()
                    .text_size(px(22.0))
                    .line_height(px(30.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(COLOR_DIM))
                    .child("›"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(24.0))
                    .line_height(px(32.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(COLOR_TEXT))
                    .truncate()
                    .child(page.label()),
            ),
    )
}

fn section_card(title: &str) -> gpui::Div {
    v_flex()
        .w_full()
        .min_w(px(0.0))
        .gap_2()
        .p_3()
        .rounded_sm()
        .border_1()
        .border_color(rgb(COLOR_BORDER))
        .bg(rgb(COLOR_PANEL))
        .child(
            div()
                .w_full()
                .text_size(px(16.0))
                .line_height(px(22.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(COLOR_TEXT))
                .child(title.to_owned()),
        )
}

fn rule_card(
    title: AnyElement,
    leading: AnyElement,
    toggle_action: AnyElement,
    card_target: RuleCardTarget,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Stateful<gpui::Div> {
    v_flex()
        .id(SharedString::from(format!("rule-card-{card_target:?}")))
        .w_full()
        .min_w(px(0.0))
        .gap_2()
        .p_3()
        .rounded_sm()
        .border_1()
        .border_color(rgb(COLOR_BORDER))
        .bg(rgb(COLOR_PANEL))
        .cursor_pointer()
        .on_click(cx.listener(move |app, _, _, cx| {
            app.toggle_rule_card(card_target.clone(), cx);
        }))
        .child(
            div()
                .relative()
                .w_full()
                .min_w(px(0.0))
                .min_h(px(30.0))
                .child(
                    h_flex()
                        .w_full()
                        .min_w(px(0.0))
                        .items_start()
                        .gap_2()
                        .pr(px(36.0))
                        .child(leading)
                        .child(title),
                )
                .child(
                    h_flex()
                        .absolute()
                        .top(px(0.0))
                        .right(px(0.0))
                        .items_center()
                        .gap_1()
                        .child(toggle_action),
                ),
        )
}

fn rule_card_toggle_button(
    id: impl Into<SharedString>,
    collapsed: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id = id.into();
    Button::new(id)
        .small()
        .ghost()
        .label(if collapsed { ">" } else { "v" })
        .w(px(28.0))
        .tooltip(if collapsed { "Expand" } else { "Collapse" })
        .on_click(on_click)
        .into_any_element()
}

fn rule_list() -> gpui::Div {
    v_flex().w_full().min_w(px(0.0)).gap_2()
}

fn row_card() -> gpui::Div {
    h_flex()
        .w_full()
        .min_w(px(0.0))
        .items_center()
        .gap_2()
        .flex_wrap()
        .p_2()
        .rounded_sm()
        .bg(rgb(COLOR_PANEL_ALT))
        .border_1()
        .border_color(rgb(COLOR_BORDER_SUBTLE))
}

fn stat_grid(rows: Vec<(&'static str, String)>) -> gpui::Div {
    let mut grid = v_flex()
        .gap_1()
        .p_3()
        .rounded_sm()
        .border_1()
        .border_color(rgb(COLOR_BORDER))
        .bg(rgb(COLOR_PANEL));
    for (label, value) in rows {
        grid = grid.child(
            h_flex()
                .w_full()
                .min_w(px(0.0))
                .gap_2()
                .items_start()
                .flex_wrap()
                .py_1()
                .child(
                    div()
                        .w(px(160.0))
                        .min_w(px(120.0))
                        .text_size(px(13.0))
                        .line_height(px(18.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(COLOR_TEXT))
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .text_size(px(13.0))
                        .line_height(px(18.0))
                        .text_color(rgb(COLOR_MUTED))
                        .child(value),
                ),
        );
    }
    grid
}

fn info_card(lines: Vec<&'static str>) -> gpui::Div {
    let mut card = v_flex()
        .gap_1()
        .p_2()
        .rounded_sm()
        .border_1()
        .border_color(rgb(COLOR_ACCENT_BG))
        .bg(rgb(0x2b3544));
    for line in lines {
        card = card.child(
            div()
                .text_size(px(13.0))
                .line_height(px(18.0))
                .text_color(rgb(COLOR_MUTED))
                .child(line),
        );
    }
    card
}

fn labeled_element(label: &str, element: AnyElement) -> gpui::Div {
    v_flex()
        .w_full()
        .min_w(px(0.0))
        .gap_1()
        .child(
            div()
                .text_size(px(12.0))
                .line_height(px(16.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(COLOR_MUTED))
                .child(label.to_owned()),
        )
        .child(element)
}

fn input_row(label: &str, input: Entity<InputState>) -> gpui::Div {
    labeled_element(
        label,
        Input::new(&input)
            .w_full()
            .max_w(px(320.0))
            .into_any_element(),
    )
}

fn syncing_rule_card(index: usize) -> AnyElement {
    section_card(&format!("Rule {}", index + 1))
        .child(syncing_input_message())
        .into_any_element()
}

fn rule_card_title(name: &str) -> &str {
    let name = name.trim();
    if name.is_empty() {
        "Unnamed rule"
    } else {
        name
    }
}

fn static_rule_title(title: &str) -> AnyElement {
    div()
        .flex_1()
        .min_w(px(0.0))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_size(px(16.0))
        .line_height(px(22.0))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(COLOR_TEXT))
        .child(title.to_owned())
        .into_any_element()
}

fn status_pill(label: &'static str, bg: u32, fg: u32) -> AnyElement {
    div()
        .flex_shrink_0()
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(rgb(bg))
        .text_size(px(12.0))
        .line_height(px(16.0))
        .text_color(rgb(fg))
        .child(label)
        .into_any_element()
}

fn rule_enable_checkbox(
    id: impl Into<SharedString>,
    checked: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let border_color = if checked { COLOR_ACCENT } else { COLOR_BORDER };

    div()
        .id(id)
        .size(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .rounded_sm()
        .hover(|style| style.bg(rgb(COLOR_PANEL_ALT)))
        .cursor_pointer()
        .child(
            div()
                .size(px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .border_1()
                .border_color(rgb(border_color))
                .bg(rgb(COLOR_PANEL))
                .when(checked, |this| {
                    this.child(
                        div()
                            .text_size(px(12.0))
                            .line_height(px(16.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(COLOR_ACCENT))
                            .child("✓"),
                    )
                }),
        )
        .on_click(move |_, window, cx| {
            let next = !checked;
            handler(&next, window, cx);
        })
        .into_any_element()
}

fn syncing_input_message() -> gpui::Div {
    text_muted("Syncing rule editor state...")
}

fn checkbox(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    checked: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let label = label.into();
    let border_color = if checked { COLOR_ACCENT } else { COLOR_BORDER };
    let text_color = if checked { COLOR_TEXT } else { COLOR_MUTED };

    h_flex()
        .id(id)
        .min_w(px(0.0))
        .items_center()
        .gap_2()
        .py_1()
        .px_1()
        .rounded_sm()
        .text_color(rgb(text_color))
        .text_size(px(13.0))
        .line_height(px(18.0))
        .hover(|style| style.bg(rgb(COLOR_PANEL_ALT)))
        .cursor_pointer()
        .child(
            div()
                .size(px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .rounded_sm()
                .border_1()
                .border_color(rgb(border_color))
                .bg(rgb(COLOR_PANEL))
                .when(checked, |this| {
                    this.child(
                        div()
                            .text_size(px(12.0))
                            .line_height(px(16.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(COLOR_ACCENT))
                            .child("✓"),
                    )
                }),
        )
        .child(div().child(label))
        .on_click(move |_, window, cx| {
            let next = !checked;
            handler(&next, window, cx);
        })
        .into_any_element()
}

fn nav_row(page: Page, selected: bool) -> gpui::Stateful<gpui::Div> {
    let row_bg = if selected {
        COLOR_PANEL_ACTIVE
    } else {
        COLOR_PANEL
    };
    let indicator = if selected { COLOR_ACCENT } else { row_bg };
    let text_color = if selected { COLOR_TEXT } else { COLOR_MUTED };

    h_flex()
        .id(SharedString::from(format!("nav-row-{:?}", page)))
        .h(px(30.0))
        .w_full()
        .items_center()
        .gap_2()
        .px_2()
        .rounded_sm()
        .bg(rgb(row_bg))
        .text_color(rgb(text_color))
        .hover(|style| style.bg(rgb(COLOR_PANEL_ALT)))
        .cursor_pointer()
        .child(div().w(px(2.0)).h(px(16.0)).rounded_sm().bg(rgb(indicator)))
        .child(div().flex_1().text_sm().child(page.label()))
}

fn toggle_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    selected: bool,
) -> Button {
    let id: SharedString = id.into();
    Button::new(id)
        .label(label)
        .small()
        .when(selected, |button| button.primary())
}

fn value_pill(value: impl Into<SharedString>) -> gpui::Div {
    div()
        .px_2()
        .py_1()
        .text_size(px(13.0))
        .line_height(px(18.0))
        .rounded_sm()
        .bg(rgb(COLOR_PANEL_ALT))
        .border_1()
        .border_color(rgb(COLOR_BORDER))
        .text_color(rgb(COLOR_ACCENT))
        .child(value.into())
}

fn text_muted(value: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(13.0))
        .line_height(px(18.0))
        .text_color(rgb(COLOR_MUTED))
        .child(value.into())
}

fn text_danger(value: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(13.0))
        .line_height(px(18.0))
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(rgb(COLOR_DANGER_BG))
        .text_color(rgb(COLOR_DANGER))
        .child(value.into())
}

fn stepper_u64(
    id: impl Into<SharedString>,
    label: &str,
    value: u64,
    suffix: &'static str,
    handler: impl Fn(&StepChange<u64>, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    let id: SharedString = id.into();
    let handler: Rc<dyn Fn(&StepChange<u64>, &mut Window, &mut App)> = Rc::new(handler);
    let down = Rc::clone(&handler);
    labeled_element(
        label,
        h_flex()
            .gap_2()
            .items_center()
            .flex_wrap()
            .child(
                Button::new((gpui::ElementId::from(id.clone()), "down"))
                    .small()
                    .label("-")
                    .on_click(move |_, window, cx| {
                        down(
                            &StepChange {
                                delta: u64_step(value),
                                increase: false,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .child(value_pill(format!("{value}{suffix}")))
            .child(
                Button::new((gpui::ElementId::from(id), "up"))
                    .small()
                    .label("+")
                    .on_click(move |_, window, cx| {
                        handler(
                            &StepChange {
                                delta: u64_step(value),
                                increase: true,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .into_any_element(),
    )
}

fn stepper_u8(
    id: impl Into<SharedString>,
    label: &str,
    value: u8,
    suffix: &'static str,
    handler: impl Fn(&StepChange<u8>, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    let id: SharedString = id.into();
    let handler: Rc<dyn Fn(&StepChange<u8>, &mut Window, &mut App)> = Rc::new(handler);
    let down = Rc::clone(&handler);
    labeled_element(
        label,
        h_flex()
            .gap_2()
            .items_center()
            .flex_wrap()
            .child(
                Button::new((gpui::ElementId::from(id.clone()), "down"))
                    .small()
                    .label("-")
                    .on_click(move |_, window, cx| {
                        down(
                            &StepChange {
                                delta: 1,
                                increase: false,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .child(value_pill(format!("{value}{suffix}")))
            .child(
                Button::new((gpui::ElementId::from(id), "up"))
                    .small()
                    .label("+")
                    .on_click(move |_, window, cx| {
                        handler(
                            &StepChange {
                                delta: 1,
                                increase: true,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .into_any_element(),
    )
}

fn u64_step(value: u64) -> u64 {
    if value >= 1_000 {
        100
    } else if value >= 100 {
        10
    } else {
        1
    }
}

fn apply_u64_step(current: u64, change: &StepChange<u64>, min: u64, max: u64) -> u64 {
    let next = if change.increase {
        current.saturating_add(change.delta)
    } else {
        current.saturating_sub(change.delta)
    };
    next.clamp(min, max)
}

fn apply_u8_step(current: u8, change: &StepChange<u8>, min: u8, max: u8) -> u8 {
    let next = if change.increase {
        current.saturating_add(change.delta)
    } else {
        current.saturating_sub(change.delta)
    };
    next.clamp(min, max)
}

fn cpu_usage_label(percent: Option<f32>) -> String {
    percent
        .map(|percent| format!("{percent:.1}%"))
        .unwrap_or_else(|| "Collecting".to_owned())
}

fn eco_qos_label(status: &EcoQosSnapshot) -> String {
    if status.enabled {
        format!(
            "{} ({} throttled)",
            status.message, status.throttled_processes
        )
    } else {
        status.message.clone()
    }
}

fn app_suspension_label(status: &AppSuspensionSnapshot) -> String {
    if status.enabled {
        format!(
            "{} ({} suspended)",
            status.message, status.suspended_processes
        )
    } else {
        status.message.clone()
    }
}

fn process_target_can_accept(target: SuggestionTarget, settings: &Settings, process: &str) -> bool {
    match target {
        SuggestionTarget::ForegroundRule(_) => true,
        SuggestionTarget::EcoQos => can_add_eco_qos_process(&settings.eco_qos, process),
        SuggestionTarget::Suspension => {
            can_add_suspension_process(&settings.app_suspension, process)
        }
    }
}

fn can_add_eco_qos_process(settings: &EcoQosSettings, process: &str) -> bool {
    let process = process.trim();
    !process.is_empty() && !ecoqos::is_process_excluded(process, settings)
}

fn can_add_suspension_process(settings: &AppSuspensionSettings, process: &str) -> bool {
    let process = process.trim();
    !process.is_empty()
        && !settings.contains_suspendable_app(process)
        && !suspension::is_builtin_excluded(process)
}

fn new_suspension_rule(process: &str) -> AppSuspensionRule {
    AppSuspensionRule {
        process_name: process.trim().to_ascii_lowercase(),
        network_wake_enabled: true,
        audio_wake_enabled: true,
        network_download_threshold_bytes: 1,
        network_download_threshold_unit: NetworkThresholdUnit::Bytes,
        network_upload_threshold_bytes: 0,
        network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
    }
}

struct SuspensionIndicator {
    label: &'static str,
    bg: u32,
    fg: u32,
    hover: &'static str,
}

fn suspension_indicator(status: &AppSuspensionSnapshot, process: &str) -> SuspensionIndicator {
    if suspension::is_builtin_excluded(process) {
        SuspensionIndicator {
            label: "Protected",
            bg: COLOR_ACCENT_BG,
            fg: COLOR_ACCENT,
            hover: "PowerLeaf does not suspend this app because it can fail to restore correctly.",
        }
    } else if suspension::contains_process(&status.network_wake_apps, process) {
        SuspensionIndicator {
            label: "Network",
            bg: COLOR_ACCENT_BG,
            fg: COLOR_ACCENT,
            hover:
                "PowerLeaf has thawed or kept this app awake because it owns network connections.",
        }
    } else if suspension::contains_process(&status.audio_wake_apps, process) {
        SuspensionIndicator {
            label: "Audio",
            bg: COLOR_ACCENT_BG,
            fg: COLOR_ACCENT,
            hover: "PowerLeaf has thawed or kept this app awake because it is playing audio.",
        }
    } else if suspension::contains_process(&status.suspended_apps, process) {
        SuspensionIndicator {
            label: "Frozen",
            bg: COLOR_SUCCESS_BG,
            fg: COLOR_SUCCESS,
            hover: "PowerLeaf has frozen this app with Windows Job Object freeze.",
        }
    } else if suspension::contains_process(&status.temporary_thawed_apps, process) {
        SuspensionIndicator {
            label: "Thawed",
            bg: COLOR_ACCENT_BG,
            fg: COLOR_ACCENT,
            hover: "PowerLeaf has temporarily thawed this app before freezing it again.",
        }
    } else if suspension::contains_process(&status.tracked_apps, process) {
        SuspensionIndicator {
            label: "Waiting",
            bg: COLOR_WARNING_BG,
            fg: COLOR_WARNING,
            hover: "This app is in the background and waiting for the delay.",
        }
    } else if status.enabled {
        SuspensionIndicator {
            label: "Not suspended",
            bg: COLOR_PANEL_ACTIVE,
            fg: COLOR_MUTED,
            hover: "PowerLeaf is not currently suspending this app.",
        }
    } else {
        SuspensionIndicator {
            label: "Off",
            bg: COLOR_PANEL_ACTIVE,
            fg: COLOR_DIM,
            hover: "App Suspension is disabled.",
        }
    }
}

fn can_manual_freeze(status: &AppSuspensionSnapshot, process: &str) -> bool {
    status.enabled && !suspension::contains_process(&status.suspended_apps, process)
}

fn network_threshold_step(unit: NetworkThresholdUnit) -> f64 {
    match unit {
        NetworkThresholdUnit::Bytes => 64.0,
        NetworkThresholdUnit::Kilobytes | NetworkThresholdUnit::Kilobits => 1.0,
        NetworkThresholdUnit::Megabytes | NetworkThresholdUnit::Megabits => 0.1,
        NetworkThresholdUnit::Gigabytes | NetworkThresholdUnit::Gigabits => 0.01,
        NetworkThresholdUnit::Bits => 512.0,
    }
}

#[derive(Debug, Clone, Copy)]
enum FileDialogMode {
    Open,
    Save,
}

fn choose_settings_file(
    hwnd: Option<HWND>,
    mode: FileDialogMode,
) -> Result<Option<PathBuf>, String> {
    const FILE_BUFFER_LEN: usize = 4096;

    let default_path = match mode {
        FileDialogMode::Open => config::storage::config_path(),
        FileDialogMode::Save => config::storage::default_export_toml_path(),
    };
    let mut file_buffer = path_to_wide_buffer(&default_path, FILE_BUFFER_LEN);
    let filter = wide_nulls("TOML settings (*.toml)\0*.toml\0All files (*.*)\0*.*\0");
    let default_extension = wide_null("toml");
    let title = match mode {
        FileDialogMode::Open => wide_null("Import settings"),
        FileDialogMode::Save => wide_null("Export settings"),
    };

    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd.unwrap_or_default(),
        lpstrFilter: filter.as_ptr(),
        lpstrFile: file_buffer.as_mut_ptr(),
        nMaxFile: file_buffer.len() as u32,
        lpstrTitle: title.as_ptr(),
        lpstrDefExt: default_extension.as_ptr(),
        Flags: OFN_HIDEREADONLY | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST,
        ..Default::default()
    };

    if matches!(mode, FileDialogMode::Open) {
        dialog.Flags |= OFN_FILEMUSTEXIST;
    } else {
        dialog.Flags |= OFN_OVERWRITEPROMPT;
    }

    let selected = unsafe {
        match mode {
            FileDialogMode::Open => GetOpenFileNameW(&mut dialog),
            FileDialogMode::Save => GetSaveFileNameW(&mut dialog),
        }
    };

    if selected != 0 {
        return Ok(Some(path_from_wide_buffer(&file_buffer)));
    }

    let error = unsafe { CommDlgExtendedError() };
    if error == 0 {
        Ok(None)
    } else {
        Err(format!("File dialog failed with error code {error}"))
    }
}

fn path_to_wide_buffer(path: &Path, len: usize) -> Vec<u16> {
    let mut buffer: Vec<u16> = path.as_os_str().encode_wide().take(len - 1).collect();
    buffer.resize(len, 0);
    buffer
}

fn path_from_wide_buffer(buffer: &[u16]) -> PathBuf {
    let len = buffer
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(buffer.len());
    PathBuf::from(OsString::from_wide(&buffer[..len]))
}

fn wide_null(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain([0]).collect()
}

fn wide_nulls(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain([0]).collect()
}
