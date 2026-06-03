use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use rust_i18n::t;

use gpui::{
    deferred, div, prelude::*, px, rgb, AnyElement, App, Context, Entity, Focusable, IntoElement,
    SharedString, Subscription, Task, Timer, Window, WindowControlArea,
};
use gpui_component::{
    badge::Badge,
    button::{Button, ButtonVariants},
    description_list::DescriptionList,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::{Escape as InputEscape, Input, InputEvent, InputState},
    label::Label,
    scroll::ScrollableElement,
    tag::Tag,
    v_flex, ActiveTheme, Disableable, Icon, IconNamed, Sizable,
};

use crate::{
    activity::{ActivitySnapshot, ActivityState, IdleDetector, InputHook},
    affinity::{self, CpuAffinitySnapshot, LogicalProcessorInfo, LogicalProcessorKind},
    automation::BackgroundAutomation,
    config::{
        self, AppLanguage, AppSuspensionRule, AppSuspensionSettings, AppThemeMode, CpuAffinityRule,
        CpuAffinitySettings, CpuUsageComparison, CpuUsageRule, EcoQosSettings, ForegroundRule,
        NetworkThresholdUnit, ScheduleRule, Settings, WeekdaySetting,
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
const APP_TICK_INTERVAL: Duration = Duration::from_secs(2);
const HIDDEN_APP_TICK_INTERVAL: Duration = Duration::from_secs(1);
const CPU_USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PROCESS_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const TITLE_BAR_HEIGHT: f32 = 36.0;
const PAGE_HEADER_HEIGHT: f32 = 42.0;
const PROCESS_PICKER_LAYER_PRIORITY: usize = 2;
const SWITCH_RETRY_INTERVAL: Duration = Duration::from_secs(15);
const MAX_NETWORK_THRESHOLD_BYTES: u64 = 1_000_000_000;

const COLOR_PANEL_ACTIVE: u32 = 0x454a56;
const COLOR_BORDER: u32 = 0x464b57;
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
    cpu_affinity_status: CpuAffinitySnapshot,
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
    affinity_process: Entity<InputState>,
}

enum TickOutcome {
    Continue { changed: bool },
    Stop,
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
            affinity_process: make_input(window, cx, "", "Search running apps..."),
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
        apply_language(settings.general.language);
        apply_theme_mode(settings.general.theme_mode, window, cx);

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
            cpu_affinity_status: CpuAffinitySnapshot::default(),
            foreground_app: None,
            decision: DecisionOutcome {
                target_guid: None,
                state: DecisionState::NoTargetPlan,
                reason: t!("status.waiting_first_check").to_string(),
            },
            next_schedule: t!("status.no_active_time_rules").to_string(),
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
            status_message: t!("status.ready").to_string(),
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
        let tick_interval = self.tick_interval();
        self._tick_task = cx.spawn_in(window, async move |this, cx| {
            Timer::after(tick_interval).await;
            let _ = cx.update(move |window, app_cx| {
                if let Some(this) = this.upgrade() {
                    let _ = this.update(app_cx, |app, cx| match app.tick(window) {
                        TickOutcome::Continue { changed } => {
                            app.schedule_tick(window, cx);
                            if changed {
                                cx.notify();
                            }
                        }
                        TickOutcome::Stop => {}
                    });
                }
            });
        });
    }

    fn tick_interval(&self) -> Duration {
        if tray::is_hidden_to_tray() {
            HIDDEN_APP_TICK_INTERVAL
        } else {
            APP_TICK_INTERVAL
        }
    }

    fn refresh_power_plans(&mut self) {
        match self.power.list_plans() {
            Ok(plans) => {
                self.plans = plans;
                self.current_plan = self.plans.iter().find(|plan| plan.active).cloned();
                self.next_active_plan_refresh = Instant::now() + ACTIVE_PLAN_REFRESH_INTERVAL;
                self.status_message =
                    t!("status.loaded_power_plans", count = self.plans.len()).to_string();
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

    fn run_check_changed(&mut self) -> bool {
        let activity_state = self.activity.state;
        let activity_idle_for = self.activity.idle_for;
        let cpu_usage_percent = self.cpu_usage.percent;
        let foreground_app = self.foreground_app.clone();
        let decision_target_guid = self.decision.target_guid.clone();
        let decision_state = self.decision.state;
        let decision_reason = self.decision.reason.clone();
        let next_schedule = self.next_schedule.clone();
        let plans = self.plans.clone();
        let current_plan = self.current_plan.clone();
        let status_message = self.status_message.clone();

        self.run_check();

        self.activity.state != activity_state
            || self.activity.idle_for != activity_idle_for
            || self.cpu_usage.percent != cpu_usage_percent
            || self.foreground_app != foreground_app
            || self.decision.target_guid != decision_target_guid
            || self.decision.state != decision_state
            || self.decision.reason != decision_reason
            || self.next_schedule != next_schedule
            || self.plans != plans
            || self.current_plan != current_plan
            || self.status_message != status_message
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
                self.status_message =
                    t!("status.switched_power_plan", reason = self.decision.reason).to_string();
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
                    Ok(()) => t!(
                        "status.saved_settings",
                        path = config::storage::config_path().display()
                    )
                    .to_string(),
                    Err(err) => t!("status.saved_settings_with_error", error = err).to_string(),
                };
            }
            Err(err) => self.status_message = err,
        }
    }

    fn export_settings_toml(&mut self) {
        match choose_settings_file(self.hwnd, FileDialogMode::Save) {
            Ok(Some(path)) => match config::storage::export_toml_to(&path, &self.settings) {
                Ok(()) => {
                    self.status_message =
                        t!("status.exported_settings", path = path.display()).to_string();
                }
                Err(err) => self.status_message = err,
            },
            Ok(None) => {
                self.status_message = t!("status.export_canceled").to_string();
            }
            Err(err) => self.status_message = err,
        }
    }

    fn import_settings_toml(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match choose_settings_file(self.hwnd, FileDialogMode::Open) {
            Ok(Some(path)) => match config::storage::import_toml_from(&path) {
                Ok(settings) => {
                    self.settings = settings;
                    apply_language(self.settings.general.language);
                    apply_theme_mode(self.settings.general.theme_mode, window, cx);
                    match config::storage::save(&self.settings) {
                        Ok(()) => {
                            self.saved_settings = self.settings.clone();
                            self.status_message = match startup::set_startup_with_windows(
                                self.saved_settings.general.startup_with_windows,
                            ) {
                                Ok(()) => t!("status.imported_settings", path = path.display())
                                    .to_string(),
                                Err(err) => t!("status.imported_settings_with_error", error = err)
                                    .to_string(),
                            };
                            self.rebuild_inputs(window, cx);
                        }
                        Err(err) => self.status_message = err,
                    }
                }
                Err(err) => self.status_message = err,
            },
            Ok(None) => {
                self.status_message = t!("status.import_canceled").to_string();
            }
            Err(err) => self.status_message = err,
        }
    }

    fn refresh_process_candidates(&mut self, report_status: bool) -> bool {
        self.next_process_refresh = Instant::now() + PROCESS_REFRESH_INTERVAL;
        match list_process_names() {
            Ok(processes) => {
                let changed = self.process_candidates != processes;
                self.process_candidates = processes;
                if report_status {
                    let message = t!(
                        "status.loaded_running_apps",
                        count = self.process_candidates.len()
                    )
                    .to_string();
                    let status_changed = self.status_message != message;
                    self.status_message = message;
                    changed || status_changed
                } else {
                    changed
                }
            }
            Err(err) => {
                let changed = self.status_message != err;
                self.status_message = err;
                changed
            }
        }
    }

    fn sync_tray_icon(&mut self) {
        let tray_required =
            self.settings.general.hide_to_tray || self.saved_settings.general.start_minimized;

        if tray_required {
            if self.tray_icon.is_none() {
                let Some(hwnd) = self.hwnd else {
                    tray::set_hide_on_close(false);
                    self.status_message = t!("status.system_tray_unavailable").to_string();
                    return;
                };

                match TrayIcon::install(hwnd) {
                    Ok(icon) => {
                        self.tray_icon = Some(icon);
                        self.status_message = t!("status.system_tray_enabled").to_string();
                    }
                    Err(err) => self.status_message = err,
                }
            }
            tray::set_hide_on_close(self.settings.general.hide_to_tray && self.tray_icon.is_some());
        } else if self.tray_icon.take().is_some() {
            tray::set_hide_on_close(false);
            self.status_message = t!("status.system_tray_disabled").to_string();
        } else {
            tray::set_hide_on_close(false);
        }
    }

    fn apply_start_minimized(&mut self, window: &mut Window) -> bool {
        if self.start_minimized_applied {
            return false;
        }
        self.start_minimized_applied = true;

        if !self.saved_settings.general.start_minimized {
            return false;
        }

        if self.tray_icon.is_some() {
            if let Some(hwnd) = self.hwnd {
                tray::hide_window(hwnd);
                self.status_message = t!("status.started_in_tray").to_string();
                return true;
            }
        }

        window.minimize_window();
        self.status_message = t!("status.started_minimized").to_string();
        true
    }

    fn tick(&mut self, window: &mut Window) -> TickOutcome {
        if tray::take_quit_requested() {
            tray::set_hide_on_close(false);
            self.tray_icon = None;
            window.remove_window();
            return TickOutcome::Stop;
        }

        let mut changed = self.apply_start_minimized(window);
        if tray::is_hidden_to_tray() {
            self.background_automation
                .update_settings(&self.background_settings());
            return TickOutcome::Continue { changed: false };
        }

        let eco_qos_status = self.background_automation.eco_qos_status();
        if self.eco_qos_status != eco_qos_status {
            self.eco_qos_status = eco_qos_status;
            changed = true;
        }

        let app_suspension_status = self.background_automation.app_suspension_status();
        if self.app_suspension_status != app_suspension_status {
            self.app_suspension_status = app_suspension_status;
            changed = true;
        }

        let cpu_affinity_status = self.background_automation.cpu_affinity_status();
        if self.cpu_affinity_status != cpu_affinity_status {
            self.cpu_affinity_status = cpu_affinity_status;
            changed = true;
        }

        if self.page_uses_process_candidates() && Instant::now() >= self.next_process_refresh {
            changed |= self.refresh_process_candidates(false);
        }

        let _input_events = self
            .input_hook
            .as_ref()
            .map(InputHook::take_events)
            .unwrap_or_default();
        let should_check_now = Instant::now() >= self.next_check;

        if should_check_now {
            changed |= self.run_check_changed();
            self.next_check = Instant::now()
                + Duration::from_millis(self.settings.general.check_interval_ms.max(250));
        }

        let tray_present = self.tray_icon.is_some();
        let status_message = self.status_message.clone();
        self.sync_tray_icon();
        changed |=
            tray_present != self.tray_icon.is_some() || status_message != self.status_message;

        self.background_automation
            .update_settings(&self.background_settings());
        TickOutcome::Continue { changed }
    }

    fn page_uses_process_candidates(&self) -> bool {
        matches!(
            self.page,
            Page::ForegroundRules | Page::EfficiencyMode | Page::AppSuspension | Page::CpuAffinity
        )
    }

    fn cancel_settings_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings = self.saved_settings.clone();
        apply_language(self.settings.general.language);
        apply_theme_mode(self.settings.general.theme_mode, window, cx);
        self.status_message = t!("status.unsaved_canceled").to_string();
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
        settings.cpu_affinity = self.saved_settings.cpu_affinity.clone();
        settings
    }

    fn nav_status(&self, page: Page) -> Option<NavStatus> {
        let settings = &self.saved_settings;

        match page {
            Page::Dashboard => None,
            Page::Activity => {
                if !settings.activity_mode.enabled
                    || !settings.activity_mode.input_detection.any_enabled()
                {
                    Some(NavStatus::Disabled)
                } else {
                    Some(NavStatus::Enabled)
                }
            }
            Page::CpuUsage => Some(enabled_nav_status(settings.cpu_usage_mode.enabled)),
            Page::EfficiencyMode => Some(feature_nav_status(
                settings.eco_qos.enabled,
                self.eco_qos_status.unsupported,
                self.eco_qos_status.failed_processes,
                self.eco_qos_status.last_error.is_some(),
            )),
            Page::AppSuspension => Some(feature_nav_status(
                settings.app_suspension.enabled,
                self.app_suspension_status.unsupported,
                self.app_suspension_status.failed_actions,
                self.app_suspension_status.last_error.is_some(),
            )),
            Page::CpuAffinity => Some(process_nav_status(
                settings.cpu_affinity.enabled,
                self.cpu_affinity_status.failed_processes,
                self.cpu_affinity_status.last_error.is_some(),
            )),
            Page::ForegroundRules => Some(enabled_nav_status(settings.foreground_rules.enabled)),
            Page::Schedule => Some(enabled_nav_status(settings.schedule_mode.enabled)),
            Page::Settings | Page::About => None,
        }
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
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(window, cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .w_full()
                    .min_w(px(0.0))
                    .min_h(px(0.0))
                    .items_start()
                    .overflow_hidden()
                    .child(self.render_navigation(cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .h_full()
                            .min_w(px(0.0))
                            .min_h(px(0.0))
                            .overflow_hidden()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .h_full()
                                    .min_w(px(0.0))
                                    .min_h(px(0.0))
                                    .overflow_y_scrollbar()
                                    .p_4()
                                    .gap_3()
                                    .child(page),
                            )
                            .child(self.render_status_bar(cx)),
                    ),
            )
            .child(if unsaved {
                self.render_unsaved_popup(window, cx).into_any_element()
            } else {
                div().into_any_element()
            })
    }
}

impl PowerLeafApp {
    fn render_title_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .id("powerleaf-title-bar")
            .window_control_area(WindowControlArea::Drag)
            .flex_none()
            .w_full()
            .h(px(TITLE_BAR_HEIGHT))
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .bg(cx.theme().title_bar)
            .child(
                h_flex()
                    .h_full()
                    .flex_1()
                    .min_w(px(0.0))
                    .items_center()
                    .gap_2()
                    .px_3()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_none()
                            .text_sm()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(cx.theme().foreground)
                            .child(t!("app.name").to_string()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("app.description").to_string()),
                    ),
            )
            .child(title_bar_controls(window, cx))
            .into_any_element()
    }

    fn render_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut nav = v_flex()
            .w(px(258.0))
            .min_w(px(258.0))
            .h_full()
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .bg(cx.theme().sidebar);

        let mut drawer = v_flex().gap_3().p_2();

        for section in Page::sections() {
            let mut group = v_flex().gap_1();
            group = group.child(
                div()
                    .px_2()
                    .pt_1()
                    .text_xs()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(cx.theme().muted_foreground)
                    .child(ui::section_label(section.label)),
            );
            for page in section.pages {
                let selected = self.page == *page;
                let target = *page;
                let status = self.nav_status(*page);
                group = group.child(
                    nav_row(*page, selected, status, cx)
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

    fn render_status_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .h(px(38.0))
            .items_center()
            .gap_2()
            .px_4()
            .border_t_1()
            .border_color(cx.theme().title_bar_border)
            .bg(cx.theme().title_bar)
            .text_sm()
            .child(text_muted(&self.status_message))
            .child(div().text_color(cx.theme().muted_foreground).child("|"))
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
            .border_color(cx.theme().warning)
            .bg(cx.theme().popover)
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(8.0)).rounded_full().bg(cx.theme().warning))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(cx.theme().popover_foreground)
                            .child(t!("unsaved.title").to_string()),
                    ),
            )
            .child(text_muted(t!("unsaved.message").to_string()))
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("discard-settings")
                            .small()
                            .label(t!("common.discard").to_string())
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.cancel_settings_changes(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("save-settings")
                            .small()
                            .primary()
                            .label(t!("common.save").to_string())
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.sync_input_values(cx);
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
            Page::CpuAffinity => self.render_affinity_page(window, cx),
            Page::Settings => self.render_settings_page(window, cx),
            Page::About => self.render_about_page(),
        }
    }

    fn render_dashboard(&self) -> AnyElement {
        let settings = self.runtime_settings();
        page_shell(Page::Dashboard)
            .child(info_card(vec![
                t!("dashboard.intro_1").to_string(),
                t!("dashboard.intro_2").to_string(),
            ]))
            .child(
                stat_grid(vec![
                    (
                        t!("dashboard.current_power_plan").to_string(),
                        self.current_plan
                            .as_ref()
                            .map(|plan| plan.name.as_str())
                            .map(str::to_owned)
                            .unwrap_or_else(|| t!("common.unknown").to_string()),
                    ),
                    (
                        t!("dashboard.current_mode").to_string(),
                        self.decision.state.label().to_owned(),
                    ),
                    (
                        t!("dashboard.automation").to_string(),
                        if settings.general.enabled {
                            t!("common.enabled").to_string()
                        } else {
                            t!("common.disabled").to_string()
                        },
                    ),
                    (
                        t!("dashboard.foreground_app").to_string(),
                        self.foreground_app
                            .as_deref()
                            .map(str::to_owned)
                            .unwrap_or_else(|| t!("common.unknown").to_string()),
                    ),
                    (
                        t!("dashboard.activity_state").to_string(),
                        format!("{:?}", self.activity.state),
                    ),
                    (
                        t!("dashboard.cpu_usage").to_string(),
                        cpu_usage_label(self.cpu_usage.percent),
                    ),
                    (
                        t!("dashboard.efficiency_mode").to_string(),
                        eco_qos_label(&self.eco_qos_status),
                    ),
                    (
                        t!("dashboard.app_suspension").to_string(),
                        app_suspension_label(&self.app_suspension_status),
                    ),
                    (
                        t!("dashboard.cpu_affinity").to_string(),
                        cpu_affinity_label(&self.cpu_affinity_status),
                    ),
                    (
                        t!("dashboard.idle_time").to_string(),
                        self.activity
                            .idle_for
                            .map(|duration| ui::duration_label(duration.as_secs()))
                            .unwrap_or_else(|| t!("common.unknown").to_string()),
                    ),
                    (
                        t!("dashboard.time_rules").to_string(),
                        self.next_schedule.clone(),
                    ),
                    (
                        t!("dashboard.decision_reason").to_string(),
                        self.decision.reason.clone(),
                    ),
                ])
                .into_any_element(),
            )
            .into_any_element()
    }

    fn render_activity_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let enabled = self.settings.activity_mode.enabled;
        page_shell(Page::Activity)
            .child(info_card(vec![
                t!("activity.intro_1").to_string(),
                t!("activity.intro_2").to_string(),
            ]))
            .child(checkbox(
                "activity-enabled",
                t!("activity.enable").to_string(),
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.activity_mode.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(text_muted(format!(
                "{}",
                t!(
                    "activity.current_active_plan",
                    plan = self
                        .current_plan
                        .as_ref()
                        .map(|plan| plan.name.as_str())
                        .map(str::to_owned)
                        .unwrap_or_else(|| t!("common.unknown").to_string())
                )
            )))
            .child(
                self.render_power_plan_picker(
                    "activity-idle-plan",
                    &t!("activity.idle_plan"),
                    self.settings
                        .activity_mode
                        .power_plans
                        .power_save_guid
                        .clone(),
                    PowerPlanField::ActivityKind(PowerPlanKind::Idle),
                    cx,
                ),
            )
            .child(
                self.render_power_plan_picker(
                    "activity-active-plan",
                    &t!("activity.active_plan"),
                    self.settings
                        .activity_mode
                        .power_plans
                        .performance_guid
                        .clone(),
                    PowerPlanField::ActivityKind(PowerPlanKind::Active),
                    cx,
                ),
            )
            .child(checkbox(
                "keyboard-input",
                t!("activity.keyboard_input").to_string(),
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
                t!("activity.mouse_input").to_string(),
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
                &t!("activity.idle_timeout"),
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
                &t!("activity.check_interval"),
                self.settings.general.check_interval_ms,
                " ms",
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    app.settings.general.check_interval_ms =
                        apply_u64_step(app.settings.general.check_interval_ms, change, 250, 60_000);
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
                t!("foreground.intro_1").to_string(),
                t!("foreground.intro_2").to_string(),
            ]))
            .child(checkbox(
                "foreground-enabled",
                t!("foreground.enable").to_string(),
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
                    .label(t!("foreground.add_rule").to_string())
                    .on_click(cx.listener(|app, _, window, cx| {
                        app.settings.foreground_rules.rules.push(ForegroundRule {
                            enabled: true,
                            name: t!("foreground.new_rule").to_string(),
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
            self.render_rule_title(&rule_card_title(&rule.name), &name_input, title_target, cx),
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
            rule_card_collapse_indicator(collapsed),
            card_target.clone(),
            cx,
        );
        if !collapsed {
            card = card
                .child(labeled_element(
                    &t!("foreground.focused_app"),
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
                    &t!("foreground.target_power_plan"),
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
                    .label(t!("common.remove").to_string())
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
                .id(SharedString::from(format!("rule-title-editor-{target:?}")))
                .flex_1()
                .min_w(px(180.0))
                .max_w(px(460.0))
                .items_center()
                .gap_2()
                .on_click(|_, _, cx| {
                    cx.stop_propagation();
                })
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
                    .label(t!("common.done").to_string())
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
                    .cursor_pointer()
                    .child(title.to_owned()),
            )
            .child(
                div()
                    .id(SharedString::from(format!(
                        "edit-rule-title-target-{target:?}"
                    )))
                    .on_click(|_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        Button::new(SharedString::from(format!("edit-rule-title-{target:?}")))
                            .small()
                            .ghost()
                            .label(t!("common.edit").to_string())
                            .tooltip(t!("common.rename_rule").to_string())
                            .on_click(cx.listener(move |app, _, window, cx| {
                                app.begin_rule_title_edit(target, window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_schedule_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let mut content = page_shell(Page::Schedule)
            .child(info_card(vec![
                t!("schedule.intro_1").to_string(),
                t!("schedule.intro_2").to_string(),
            ]))
            .child(checkbox(
                "schedule-enabled",
                t!("schedule.enable").to_string(),
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
                    .label(t!("schedule.add_rule").to_string())
                    .on_click(cx.listener(|app, _, window, cx| {
                        app.settings.schedule_mode.rules.push(ScheduleRule {
                            enabled: true,
                            name: t!("schedule.new_rule").to_string(),
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
            self.render_rule_title(&rule_card_title(&rule.name), &name_input, title_target, cx),
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
            rule_card_collapse_indicator(collapsed),
            card_target.clone(),
            cx,
        );
        if !collapsed {
            card = card
                .child(labeled_element(
                    &t!("schedule.days"),
                    days.into_any_element(),
                ))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .child(match self.inputs.schedule_start_times.get(index).cloned() {
                            Some(input) => {
                                input_row(&t!("schedule.start"), input).into_any_element()
                            }
                            None => syncing_input_message().into_any_element(),
                        })
                        .child(match self.inputs.schedule_end_times.get(index).cloned() {
                            Some(input) => input_row(&t!("schedule.end"), input).into_any_element(),
                            None => syncing_input_message().into_any_element(),
                        })
                        .child(if rule.parsed_times().is_none() {
                            text_danger(t!("schedule.use_hhmm").to_string()).into_any_element()
                        } else {
                            div().into_any_element()
                        }),
                )
                .child(self.render_power_plan_picker(
                    format!("schedule-rule-plan-{index}"),
                    &t!("schedule.target_power_plan"),
                    rule.power_plan_guid.clone(),
                    PowerPlanField::ScheduleRule(index),
                    cx,
                ))
                .child(
                    Button::new(SharedString::from(format!("remove-schedule-rule-{index}")))
                        .small()
                        .danger()
                        .label(t!("common.remove").to_string())
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
                t!("cpu_rules.intro_1").to_string(),
                t!("cpu_rules.intro_2").to_string(),
            ]))
            .child(checkbox(
                "cpu-usage-enabled",
                t!("cpu_rules.enable").to_string(),
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
                    .label(t!("cpu_rules.add_rule").to_string())
                    .on_click(cx.listener(|app, _, window, cx| {
                        app.settings.cpu_usage_mode.rules.push(CpuUsageRule {
                            enabled: true,
                            name: t!("cpu_rules.new_rule").to_string(),
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
            self.render_rule_title(&rule_card_title(&rule.name), &name_input, title_target, cx),
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
            rule_card_collapse_indicator(collapsed),
            card_target.clone(),
            cx,
        );
        if !collapsed {
            card = card
                .child(labeled_element(
                    &t!("cpu_rules.when_cpu_load"),
                    comparisons.into_any_element(),
                ))
                .child(stepper_u8(
                    format!("cpu-rule-threshold-{index}"),
                    &t!("cpu_rules.threshold"),
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
                        &t!("cpu_rules.upper_threshold"),
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
                    &t!("cpu_rules.duration"),
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
                    &t!("cpu_rules.use"),
                    rule.power_plan_guid.clone(),
                    PowerPlanField::CpuRule(index),
                    cx,
                ))
                .child(checkbox(
                    format!("cpu-rule-else-{index}"),
                    t!("cpu_rules.else").to_string(),
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
                        &t!("cpu_rules.else_use"),
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
                        .label(t!("common.remove").to_string())
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
                t!("efficiency.intro_1").to_string(),
                t!("efficiency.intro_2").to_string(),
                t!("efficiency.intro_3").to_string(),
            ]))
            .child(checkbox(
                "eco-qos-enabled",
                t!("efficiency.enable").to_string(),
                self.settings.eco_qos.enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.eco_qos.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "eco-qos-foreground",
                t!("efficiency.focus_detection").to_string(),
                self.settings.eco_qos.exclude_foreground_app,
                cx.listener(|app, checked, _, cx| {
                    app.settings.eco_qos.exclude_foreground_app = *checked;
                    cx.notify();
                }),
            ))
            .child(stat_grid(vec![
                (
                    t!("common.status").to_string(),
                    self.eco_qos_status.message.clone(),
                ),
                (
                    t!("efficiency.throttled_processes").to_string(),
                    self.eco_qos_status.throttled_processes.to_string(),
                ),
                (
                    t!("efficiency.scanned_processes").to_string(),
                    self.eco_qos_status.scanned_processes.to_string(),
                ),
                (
                    t!("efficiency.skipped_processes").to_string(),
                    self.eco_qos_status.skipped_processes.to_string(),
                ),
                (
                    t!("efficiency.failed_actions").to_string(),
                    self.eco_qos_status.failed_processes.to_string(),
                ),
                (
                    t!("common.last_failure").to_string(),
                    self.eco_qos_status
                        .last_error
                        .as_deref()
                        .map(str::to_owned)
                        .unwrap_or_else(|| t!("common.none").to_string()),
                ),
            ]))
            .child(
                section_card(&t!("efficiency.whitelist"))
                    .child(text_muted(t!("efficiency.whitelist_help").to_string()))
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
                                    .label(t!("common.add").to_string())
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
                                        if can_add_eco_qos_process(&app.settings.eco_qos, &process)
                                        {
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
                            .label(t!("common.remove").to_string())
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
            list = list.child(text_muted(t!("efficiency.no_whitelist").to_string()));
        }
        list.into_any_element()
    }

    fn render_suspension_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let input_value = self.inputs.suspension_process.read(cx).value().to_string();
        page_shell(Page::AppSuspension)
            .child(info_card(vec![
                t!("suspension.intro_1").to_string(),
                t!("suspension.intro_2").to_string(),
                t!("suspension.intro_3").to_string(),
            ]))
            .child(checkbox(
                "app-suspension-enabled",
                t!("suspension.enable").to_string(),
                self.settings.app_suspension.enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.app_suspension.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(stepper_u64(
                "suspension-background-delay",
                &t!("suspension.background_delay"),
                self.settings.app_suspension.background_delay_seconds,
                " sec",
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    app.settings.app_suspension.background_delay_seconds = apply_u64_step(
                        app.settings.app_suspension.background_delay_seconds,
                        change,
                        1,
                        86_400,
                    );
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "temporary-thaw",
                t!("suspension.temporary_thaw").to_string(),
                self.settings.app_suspension.temporary_thaw_enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.app_suspension.temporary_thaw_enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(stepper_u64(
                "suspension-thaw-interval",
                &t!("suspension.thaw_every"),
                self.settings.app_suspension.temporary_thaw_interval_seconds,
                " sec",
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    app.settings.app_suspension.temporary_thaw_interval_seconds = apply_u64_step(
                        app.settings.app_suspension.temporary_thaw_interval_seconds,
                        change,
                        1,
                        86_400,
                    );
                    cx.notify();
                }),
            ))
            .child(stepper_u64(
                "suspension-thaw-duration",
                &t!("suspension.thaw_duration"),
                self.settings.app_suspension.temporary_thaw_duration_seconds,
                " sec",
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    app.settings.app_suspension.temporary_thaw_duration_seconds = apply_u64_step(
                        app.settings.app_suspension.temporary_thaw_duration_seconds,
                        change,
                        1,
                        3_600,
                    );
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "audio-wake",
                t!("suspension.audio_detection").to_string(),
                self.settings.app_suspension.audio_wake_enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.app_suspension.audio_wake_enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(stepper_u64(
                "suspension-audio-refreeze",
                &t!("suspension.audio_refreeze"),
                self.settings.app_suspension.audio_wake_duration_seconds,
                " sec quiet",
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    app.settings.app_suspension.audio_wake_duration_seconds = apply_u64_step(
                        app.settings.app_suspension.audio_wake_duration_seconds,
                        change,
                        1,
                        3_600,
                    );
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "network-wake",
                t!("suspension.network_detection").to_string(),
                self.settings.app_suspension.network_wake_enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.app_suspension.network_wake_enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(stepper_u64(
                "suspension-network-refreeze",
                &t!("suspension.network_refreeze"),
                self.settings.app_suspension.network_wake_duration_seconds,
                " sec quiet",
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    app.settings.app_suspension.network_wake_duration_seconds = apply_u64_step(
                        app.settings.app_suspension.network_wake_duration_seconds,
                        change,
                        1,
                        3_600,
                    );
                    cx.notify();
                }),
            ))
            .child(stat_grid(vec![
                (
                    t!("common.status").to_string(),
                    self.app_suspension_status.message.clone(),
                ),
                (
                    t!("suspension.tracked_processes").to_string(),
                    self.app_suspension_status.tracked_processes.to_string(),
                ),
                (
                    t!("suspension.suspended_processes").to_string(),
                    self.app_suspension_status.suspended_processes.to_string(),
                ),
                (
                    t!("suspension.temporary_thawed").to_string(),
                    self.app_suspension_status
                        .temporary_thawed_processes
                        .to_string(),
                ),
                (
                    t!("suspension.network_wake").to_string(),
                    self.app_suspension_status
                        .network_wake_processes
                        .to_string(),
                ),
                (
                    t!("suspension.audio_wake").to_string(),
                    self.app_suspension_status.audio_wake_processes.to_string(),
                ),
                (
                    t!("efficiency.skipped_processes").to_string(),
                    self.app_suspension_status.skipped_processes.to_string(),
                ),
                (
                    t!("efficiency.failed_actions").to_string(),
                    self.app_suspension_status.failed_actions.to_string(),
                ),
                (
                    t!("common.last_failure").to_string(),
                    self.app_suspension_status
                        .last_error
                        .as_deref()
                        .map(str::to_owned)
                        .unwrap_or_else(|| t!("common.none").to_string()),
                ),
            ]))
            .child(
                section_card(&t!("suspension.suspendable_apps"))
                    .child(text_muted(t!("suspension.suspendable_help").to_string()))
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
                                    .label(t!("common.add").to_string())
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
                rule_card_collapse_indicator(collapsed),
                card_target.clone(),
                cx,
            );
            if !collapsed {
                card = card
                    .child(text_muted(indicator.hover))
                    .child(
                        Button::new(SharedString::from(format!("freeze-suspension-{index}")))
                            .small()
                            .label(t!("suspension.freeze").to_string())
                            .disabled(!can_manual_freeze(&self.app_suspension_status, &process))
                            .on_click(cx.listener({
                                let process = process.clone();
                                move |app, _, _, cx| {
                                    app.background_automation
                                        .request_app_suspension_freeze(&process);
                                    app.status_message =
                                        t!("suspension.manual_freeze_requested", process = process)
                                            .to_string();
                                    cx.notify();
                                }
                            })),
                    )
                    .child(checkbox(
                        format!("suspension-network-rule-{index}"),
                        t!("suspension.network_detection").to_string(),
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
                        t!("suspension.audio_detection").to_string(),
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
                        &t!("suspension.download_threshold"),
                        rule.network_download_threshold_bytes,
                        rule.network_download_threshold_unit,
                        ThresholdField::Download(index),
                        cx,
                    ))
                    .child(self.render_network_threshold(
                        index,
                        false,
                        &t!("suspension.upload_threshold"),
                        rule.network_upload_threshold_bytes,
                        rule.network_upload_threshold_unit,
                        ThresholdField::Upload(index),
                        cx,
                    ))
                    .child(
                        Button::new(SharedString::from(format!("remove-suspension-{index}")))
                            .small()
                            .danger()
                            .label(t!("common.remove").to_string())
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
            list = list.child(text_muted(t!("suspension.no_suspendable").to_string()));
        }
        list.into_any_element()
    }

    fn render_affinity_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let input_value = self.inputs.affinity_process.read(cx).value().to_string();
        page_shell(Page::CpuAffinity)
            .child(info_card(vec![
                t!("affinity.intro_1").to_string(),
                t!("affinity.intro_2").to_string(),
                t!("affinity.intro_3").to_string(),
            ]))
            .child(checkbox(
                "cpu-affinity-enabled",
                t!("affinity.enable").to_string(),
                self.settings.cpu_affinity.enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.cpu_affinity.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "cpu-affinity-foreground",
                t!("affinity.focus_detection").to_string(),
                self.settings.cpu_affinity.exclude_foreground_app,
                cx.listener(|app, checked, _, cx| {
                    app.settings.cpu_affinity.exclude_foreground_app = *checked;
                    cx.notify();
                }),
            ))
            .child(stat_grid(vec![
                (
                    t!("common.status").to_string(),
                    self.cpu_affinity_status.message.clone(),
                ),
                (
                    t!("affinity.adjusted_processes").to_string(),
                    self.cpu_affinity_status.adjusted_processes.to_string(),
                ),
                (
                    t!("affinity.scanned_processes").to_string(),
                    self.cpu_affinity_status.scanned_processes.to_string(),
                ),
                (
                    t!("affinity.skipped_processes").to_string(),
                    self.cpu_affinity_status.skipped_processes.to_string(),
                ),
                (
                    t!("affinity.failed_actions").to_string(),
                    self.cpu_affinity_status.failed_processes.to_string(),
                ),
                (
                    t!("common.last_failure").to_string(),
                    self.cpu_affinity_status
                        .last_error
                        .as_deref()
                        .map(str::to_owned)
                        .unwrap_or_else(|| t!("common.none").to_string()),
                ),
            ]))
            .child(
                section_card(&t!("affinity.rules"))
                    .child(text_muted(t!("affinity.rules_help").to_string()))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_start()
                            .flex_wrap()
                            .child(self.render_process_picker(
                                "affinity-suggestion",
                                &self.inputs.affinity_process,
                                SuggestionTarget::Affinity,
                                window,
                                cx,
                            ))
                            .child(
                                Button::new("add-affinity-process")
                                    .small()
                                    .label(t!("common.add").to_string())
                                    .disabled(!can_add_affinity_process(
                                        &self.settings.cpu_affinity,
                                        &input_value,
                                    ))
                                    .on_click(cx.listener(|app, _, window, cx| {
                                        let process = app
                                            .inputs
                                            .affinity_process
                                            .read(cx)
                                            .value()
                                            .to_string();
                                        if can_add_affinity_process(
                                            &app.settings.cpu_affinity,
                                            &process,
                                        ) {
                                            app.settings
                                                .cpu_affinity
                                                .rules
                                                .push(new_affinity_rule(&process));
                                            clear_input(&app.inputs.affinity_process, window, cx);
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(self.render_affinity_rules(cx)),
            )
            .into_any_element()
    }

    fn render_affinity_rules(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut list = rule_list();
        for (index, rule) in self.settings.cpu_affinity.rules.iter().enumerate() {
            let process = rule.process_name.clone();
            let indicator = affinity_indicator(&self.cpu_affinity_status, &process);
            let card_target = RuleCardTarget::Affinity(process.clone());
            let collapsed = self.is_rule_card_collapsed(&card_target);
            let mut card = rule_card(
                static_rule_title(&process),
                rule_enable_checkbox(
                    format!("affinity-rule-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        if let Some(rule) = app.settings.cpu_affinity.rules.get_mut(index) {
                            rule.enabled = *checked;
                        }
                        cx.notify();
                    }),
                ),
                rule_card_collapse_indicator(collapsed),
                card_target.clone(),
                cx,
            );
            if !collapsed {
                card = card
                    .child(status_pill(indicator.label, indicator.bg, indicator.fg))
                    .child(text_muted(indicator.hover))
                    .child(value_pill(affinity_mask_label(rule.core_mask)))
                    .child(self.render_affinity_core_selector(index, rule.core_mask, cx))
                    .child(
                        Button::new(SharedString::from(format!("remove-affinity-{index}")))
                            .small()
                            .danger()
                            .label(t!("common.remove").to_string())
                            .on_click(cx.listener({
                                let card_target = card_target.clone();
                                move |app, _, _, cx| {
                                    if index < app.settings.cpu_affinity.rules.len() {
                                        app.settings.cpu_affinity.rules.remove(index);
                                    }
                                    app.collapsed_rule_cards.remove(&card_target);
                                    cx.notify();
                                }
                            })),
                    );
            }
            list = list.child(card);
        }
        if self.settings.cpu_affinity.rules.is_empty() {
            list = list.child(text_muted(t!("affinity.no_rules").to_string()));
        }
        list.into_any_element()
    }

    fn render_affinity_core_selector(
        &self,
        index: usize,
        core_mask: u64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let processors = affinity::logical_processors();
        let all_mask = affinity_processors_mask(&processors);
        let performance_mask =
            affinity_processors_kind_mask(&processors, LogicalProcessorKind::Performance);
        let efficiency_mask =
            affinity_processors_kind_mask(&processors, LogicalProcessorKind::Efficiency);

        let mut presets = h_flex().gap_1().flex_wrap();
        for (label, mask, tooltip, enabled) in [
            (
                t!("affinity.all").to_string(),
                all_mask,
                t!("affinity.all_help").to_string(),
                all_mask != 0,
            ),
            (
                t!("affinity.p_cores").to_string(),
                performance_mask,
                t!("affinity.p_cores_help").to_string(),
                performance_mask != 0,
            ),
            (
                t!("affinity.e_cores").to_string(),
                efficiency_mask,
                t!("affinity.e_cores_help").to_string(),
                efficiency_mask != 0,
            ),
        ] {
            presets = presets.child(
                toggle_button(
                    format!("affinity-core-preset-{index}-{label}"),
                    label,
                    enabled && core_mask == mask,
                )
                .tooltip(tooltip)
                .disabled(!enabled)
                .on_click(cx.listener(move |app, _, _, cx| {
                    if mask != 0 {
                        if let Some(rule) = app.settings.cpu_affinity.rules.get_mut(index) {
                            rule.core_mask = mask;
                        }
                        cx.notify();
                    }
                })),
            );
        }

        let mut row = h_flex().gap_1().flex_wrap();
        for processor in processors {
            let core = processor.index;
            let selected = affinity_mask_contains(core_mask, core);
            row = row.child(
                toggle_button(
                    format!("affinity-core-{index}-{core}"),
                    affinity_processor_label(&processor),
                    selected,
                )
                .tooltip(affinity_processor_tooltip(&processor))
                .on_click(cx.listener(move |app, _, _, cx| {
                    if let Some(rule) = app.settings.cpu_affinity.rules.get_mut(index) {
                        toggle_affinity_core(&mut rule.core_mask, core);
                    }
                    cx.notify();
                })),
            );
        }

        labeled_element(
            &t!("affinity.allowed_cpus"),
            v_flex()
                .gap_2()
                .child(presets)
                .child(row)
                .into_any_element(),
        )
        .into_any_element()
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
            t!("affinity.unlimited").to_string()
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

    fn render_theme_selector(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut row = h_flex().gap_1().flex_wrap();
        for mode in AppThemeMode::ALL {
            let label = match mode {
                AppThemeMode::System => t!("theme.system"),
                AppThemeMode::Light => t!("theme.light"),
                AppThemeMode::Dark => t!("theme.dark"),
            };
            row = row.child(
                toggle_button(
                    format!("theme-mode-{:?}", mode),
                    label.to_string(),
                    self.settings.general.theme_mode == mode,
                )
                .on_click(cx.listener(move |app, _, window, cx| {
                    app.settings.general.theme_mode = mode;
                    apply_theme_mode(mode, window, cx);
                    cx.notify();
                })),
            );
        }
        labeled_element(&t!("common.theme"), row.into_any_element()).into_any_element()
    }

    fn render_language_selector(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut row = h_flex().gap_1().flex_wrap();
        for language in AppLanguage::ALL {
            row = row.child(
                toggle_button(
                    format!("language-{:?}", language),
                    t!(language.label_key()).to_string(),
                    self.settings.general.language == language,
                )
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.settings.general.language = language;
                    apply_language(language);
                    cx.notify();
                })),
            );
        }
        labeled_element(&t!("common.language"), row.into_any_element()).into_any_element()
    }

    fn render_settings_page(&self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        page_shell(Page::Settings)
            .child(info_card(vec![
                t!("settings.intro_1").to_string(),
                t!("settings.intro_2").to_string(),
            ]))
            .child(checkbox(
                "general-enabled",
                t!("settings.master_switch").to_string(),
                self.settings.general.enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "startup-windows",
                t!("settings.startup_windows").to_string(),
                self.settings.general.startup_with_windows,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.startup_with_windows = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "start-minimized",
                t!("settings.start_minimized").to_string(),
                self.settings.general.start_minimized,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.start_minimized = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "pause-plugged",
                t!("settings.pause_plugged").to_string(),
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
                t!("settings.hide_to_tray").to_string(),
                self.settings.general.hide_to_tray,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.hide_to_tray = *checked;
                    cx.notify();
                }),
            ))
            .child(
                section_card(&t!("settings.appearance"))
                    .child(self.render_theme_selector(cx))
                    .child(self.render_language_selector(cx)),
            )
            .child(
                section_card(&t!("settings.settings_files")).child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            Button::new("export-settings")
                                .small()
                                .label(t!("settings.export_settings").to_string())
                                .on_click(cx.listener(|app, _, _, cx| {
                                    app.export_settings_toml();
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("import-settings")
                                .small()
                                .label(t!("settings.import_settings").to_string())
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
                t!("about.intro_1").to_string(),
                t!("about.intro_2").to_string(),
            ]))
            .child(
                section_card(&t!("app.name"))
                    .child(text_muted(t!("app.description").to_string()))
                    .child(stat_grid(vec![
                        (t!("about.author").to_string(), "Tatsh Siow".to_owned()),
                        (
                            t!("about.version").to_string(),
                            env!("CARGO_PKG_VERSION").to_owned(),
                        ),
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
                .unwrap_or_else(|| t!("common.selected_plan_unavailable").to_string()),
            None => t!("common.use_inherited_default_plan").to_string(),
        };

        let mut options = v_flex()
            .w_full()
            .max_h(px(244.0))
            .overflow_y_scrollbar()
            .gap_1()
            .p_1()
            .rounded_sm()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover);

        options = options.child(power_plan_option_row(
            format!("{id}-default"),
            t!("common.use_inherited_default_plan").to_string(),
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
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("common.no_power_plans_loaded").to_string()),
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
                    .border_color(cx.theme().input)
                    .bg(cx.theme().background)
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .hover(|style| style.bg(cx.theme().secondary_hover))
                    .cursor_pointer()
                    .child(div().flex_1().min_w(px(0.0)).child(selected_text))
                    .child(div().text_color(cx.theme().muted_foreground).child("v"))
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
            .border_color(cx.theme().border)
            .bg(cx.theme().popover);
        if matches.is_empty() {
            suggestions = suggestions.child(
                div()
                    .px_2()
                    .py_2()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.process_candidates.is_empty() {
                        t!("common.no_running_apps_loaded").to_string()
                    } else {
                        t!("common.no_matching_apps").to_string()
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
                    .text_color(cx.theme().popover_foreground)
                    .when(count == 0, |row| {
                        row.bg(cx.theme().accent)
                            .text_color(cx.theme().accent_foreground)
                    })
                    .hover(|style| style.bg(cx.theme().secondary_hover))
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
            SuggestionTarget::Affinity => {
                clear_input_to(&self.inputs.affinity_process, process, window, cx);
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
    let text_color = cx.theme().popover_foreground;
    let selected_bg = cx.theme().accent;
    let selected_text_color = cx.theme().accent_foreground;
    let hover_bg = cx.theme().secondary_hover;

    h_flex()
        .id(SharedString::from(id))
        .h(px(24.0))
        .items_center()
        .px_2()
        .rounded_sm()
        .text_sm()
        .text_color(text_color)
        .when(selected, |row| {
            row.bg(selected_bg).text_color(selected_text_color)
        })
        .hover(move |style| style.bg(hover_bg))
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
    Affinity,
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
    Affinity(String),
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

fn apply_theme_mode(mode: AppThemeMode, window: &mut Window, cx: &mut App) {
    match mode {
        AppThemeMode::System => gpui_component::Theme::sync_system_appearance(Some(window), cx),
        AppThemeMode::Light => {
            gpui_component::Theme::change(gpui_component::ThemeMode::Light, Some(window), cx)
        }
        AppThemeMode::Dark => {
            gpui_component::Theme::change(gpui_component::ThemeMode::Dark, Some(window), cx)
        }
    }
}

fn apply_language(language: AppLanguage) {
    rust_i18n::set_locale(language.locale());
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
                    .opacity(0.72)
                    .truncate()
                    .child(page.section_label()),
            )
            .child(
                div()
                    .text_size(px(22.0))
                    .line_height(px(30.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .opacity(0.48)
                    .child("›"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(24.0))
                    .line_height(px(32.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .truncate()
                    .child(page.label()),
            ),
    )
}

fn section_card(title: &str) -> GroupBox {
    GroupBox::new()
        .outline()
        .title(Label::new(title.to_owned()))
}

fn rule_card(
    title: AnyElement,
    leading: AnyElement,
    collapse_indicator: AnyElement,
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
        .border_color(cx.theme().border)
        .bg(cx.theme().group_box)
        .child(
            div()
                .relative()
                .w_full()
                .min_w(px(0.0))
                .min_h(px(30.0))
                .id(SharedString::from(format!(
                    "rule-card-header-{card_target:?}"
                )))
                .child(
                    h_flex()
                        .w_full()
                        .min_w(px(0.0))
                        .items_start()
                        .gap_2()
                        .pr(px(36.0))
                        .id(SharedString::from(format!(
                            "rule-card-header-action-{card_target:?}"
                        )))
                        .cursor_pointer()
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.toggle_rule_card(card_target.clone(), cx);
                        }))
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
                        .child(collapse_indicator),
                ),
        )
}

fn rule_card_collapse_indicator(collapsed: bool) -> AnyElement {
    div()
        .w(px(28.0))
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(14.0))
        .line_height(px(18.0))
        .opacity(0.72)
        .child(if collapsed { ">" } else { "v" })
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
        .border_1()
        .opacity(0.92)
}

fn stat_grid(rows: Vec<(String, String)>) -> GroupBox {
    let mut list = DescriptionList::vertical()
        .columns(1)
        .bordered(false)
        .label_width(px(160.0));
    for (label, value) in rows {
        list = list.item(label, text_muted(value).into_any_element(), 1);
    }
    GroupBox::new().outline().child(list)
}

fn info_card(lines: impl IntoIterator<Item = impl Into<SharedString>>) -> GroupBox {
    let mut card = GroupBox::new().fill();
    for line in lines {
        card = card.child(
            div()
                .text_size(px(13.0))
                .line_height(px(18.0))
                .child(line.into()),
        );
    }
    card
}

fn labeled_element(label: &str, element: AnyElement) -> gpui::Div {
    v_flex()
        .w_full()
        .min_w(px(0.0))
        .gap_1()
        .child(Label::new(label.to_owned()).text_size(px(12.0)))
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

fn rule_card_title(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        t!("common.unnamed_rule").to_string()
    } else {
        name.to_owned()
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
        .child(title.to_owned())
        .into_any_element()
}

fn status_pill(label: impl Into<SharedString>, _bg: u32, fg: u32) -> AnyElement {
    let label: SharedString = label.into();
    let tag = match fg {
        COLOR_SUCCESS => Tag::success(),
        COLOR_WARNING => Tag::warning(),
        COLOR_DANGER => Tag::danger(),
        COLOR_ACCENT => Tag::info(),
        _ => Tag::secondary(),
    };

    tag.flex_shrink_0().child(label).into_any_element()
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
        .hover(|style| style.opacity(0.86))
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
            cx.stop_propagation();
            let next = !checked;
            handler(&next, window, cx);
        })
        .into_any_element()
}

fn syncing_input_message() -> gpui::Div {
    text_muted(t!("common.syncing_rule_editor").to_string())
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
        .hover(|style| style.opacity(0.86))
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

#[derive(Clone, Copy)]
enum NavStatus {
    Enabled,
    Disabled,
    Failed,
    Unsupported,
}

fn title_bar_controls(window: &Window, cx: &mut Context<PowerLeafApp>) -> AnyElement {
    let (maximize_id, maximize_icon) = if window.is_maximized() {
        ("titlebar-restore", "\u{e923}")
    } else {
        ("titlebar-maximize", "\u{e922}")
    };

    h_flex()
        .id("titlebar-controls")
        .h_full()
        .flex_none()
        .font_family("Segoe MDL2 Assets")
        .child(title_bar_control_button(
            "titlebar-minimize",
            "\u{e921}",
            WindowControlArea::Min,
            false,
            cx,
        ))
        .child(title_bar_control_button(
            maximize_id,
            maximize_icon,
            WindowControlArea::Max,
            false,
            cx,
        ))
        .child(title_bar_control_button(
            "titlebar-close",
            "\u{e8bb}",
            WindowControlArea::Close,
            true,
            cx,
        ))
        .into_any_element()
}

fn title_bar_control_button(
    id: &'static str,
    icon: &'static str,
    control_area: WindowControlArea,
    is_close: bool,
    cx: &mut Context<PowerLeafApp>,
) -> AnyElement {
    let hover_bg = if is_close {
        cx.theme().danger_hover
    } else {
        cx.theme().secondary_hover
    };
    let active_bg = if is_close {
        cx.theme().danger_active
    } else {
        cx.theme().secondary_active
    };

    h_flex()
        .id(id)
        .window_control_area(control_area)
        .occlude()
        .flex_none()
        .w(px(46.0))
        .h(px(TITLE_BAR_HEIGHT))
        .items_center()
        .justify_center()
        .text_size(px(10.0))
        .text_color(cx.theme().muted_foreground)
        .hover(move |style| style.bg(hover_bg))
        .active(move |style| style.bg(active_bg))
        .child(icon)
        .into_any_element()
}

fn nav_row(
    page: Page,
    selected: bool,
    status: Option<NavStatus>,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Stateful<gpui::Div> {
    let row_bg = if selected {
        cx.theme().sidebar_accent
    } else {
        cx.theme().transparent
    };
    let indicator = if selected {
        cx.theme().sidebar_primary
    } else {
        cx.theme().transparent
    };
    let text_color = if selected {
        cx.theme().sidebar_accent_foreground
    } else {
        cx.theme().sidebar_foreground
    };
    let hover_bg = cx.theme().sidebar_accent;

    let row = h_flex()
        .id(SharedString::from(format!("nav-row-{:?}", page)))
        .h(px(32.0))
        .w_full()
        .items_center()
        .gap_2()
        .px_2()
        .rounded_sm()
        .bg(row_bg)
        .text_color(text_color)
        .hover(move |style| style.bg(hover_bg))
        .cursor_pointer()
        .child(div().w(px(2.0)).h(px(16.0)).rounded_sm().bg(indicator))
        .child(nav_icon(page, selected, cx))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_sm()
                .truncate()
                .child(page.label()),
        );

    if let Some(status) = status {
        row.child(nav_status_indicator(status, cx))
    } else {
        row
    }
}

fn nav_status_indicator(status: NavStatus, cx: &mut Context<PowerLeafApp>) -> AnyElement {
    let color = match status {
        NavStatus::Enabled => cx.theme().success_foreground,
        NavStatus::Failed => cx.theme().danger_foreground,
        NavStatus::Disabled => cx.theme().muted_foreground,
        NavStatus::Unsupported => cx.theme().muted_foreground,
    };

    Badge::new()
        .dot()
        .color(color)
        .child(div().size(px(8.0)))
        .into_any_element()
}

fn enabled_nav_status(enabled: bool) -> NavStatus {
    if enabled {
        NavStatus::Enabled
    } else {
        NavStatus::Disabled
    }
}

fn process_nav_status(enabled: bool, failed_count: usize, has_error: bool) -> NavStatus {
    if failed_count > 0 || has_error {
        NavStatus::Failed
    } else {
        enabled_nav_status(enabled)
    }
}

fn feature_nav_status(
    enabled: bool,
    unsupported: bool,
    failed_count: usize,
    has_error: bool,
) -> NavStatus {
    if unsupported {
        NavStatus::Unsupported
    } else {
        process_nav_status(enabled, failed_count, has_error)
    }
}

fn nav_icon(page: Page, selected: bool, cx: &mut Context<PowerLeafApp>) -> AnyElement {
    let color = if selected {
        cx.theme().sidebar_primary
    } else {
        cx.theme().muted_foreground
    };

    div()
        .w(px(18.0))
        .h(px(18.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .child(
            Icon::new(nav_icon_name(page))
                .with_size(px(16.0))
                .text_color(color),
        )
        .into_any_element()
}

fn nav_icon_name(page: Page) -> NavIcon {
    match page {
        Page::Dashboard => NavIcon::Dashboard,
        Page::Activity => NavIcon::Activity,
        Page::CpuUsage => NavIcon::Chart,
        Page::EfficiencyMode => NavIcon::Zap,
        Page::AppSuspension => NavIcon::PauseCircle,
        Page::CpuAffinity => NavIcon::Chip,
        Page::ForegroundRules => NavIcon::Frame,
        Page::Schedule => NavIcon::Calendar,
        Page::Settings => NavIcon::Settings,
        Page::About => NavIcon::Info,
    }
}

#[derive(Clone, Copy)]
enum NavIcon {
    Activity,
    Calendar,
    Chart,
    Chip,
    Dashboard,
    Frame,
    Info,
    PauseCircle,
    Settings,
    Zap,
}

impl IconNamed for NavIcon {
    fn path(self) -> SharedString {
        match self {
            Self::Activity => "icons/activity.svg",
            Self::Calendar => "icons/calendar.svg",
            Self::Chart => "icons/chart.svg",
            Self::Chip => "icons/chip.svg",
            Self::Dashboard => "icons/dashboard.svg",
            Self::Frame => "icons/frame.svg",
            Self::Info => "icons/info.svg",
            Self::PauseCircle => "icons/pause-circle.svg",
            Self::Settings => "icons/settings.svg",
            Self::Zap => "icons/zap.svg",
        }
        .into()
    }
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
    div().child(
        Tag::info()
            .outline()
            .text_size(px(13.0))
            .child(value.into()),
    )
}

fn text_muted(value: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(13.0))
        .line_height(px(18.0))
        .opacity(0.72)
        .child(value.into())
}

fn text_danger(value: impl Into<SharedString>) -> gpui::Div {
    div().child(
        Tag::danger()
            .outline()
            .text_size(px(13.0))
            .child(value.into()),
    )
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
        .unwrap_or_else(|| t!("dashboard.collecting").to_string())
}

fn eco_qos_label(status: &EcoQosSnapshot) -> String {
    if status.enabled {
        t!(
            "dashboard.throttled_suffix",
            message = status.message.clone(),
            count = status.throttled_processes
        )
        .to_string()
    } else {
        status.message.clone()
    }
}

fn app_suspension_label(status: &AppSuspensionSnapshot) -> String {
    if status.enabled {
        t!(
            "dashboard.suspended_suffix",
            message = status.message.clone(),
            count = status.suspended_processes
        )
        .to_string()
    } else {
        status.message.clone()
    }
}

fn cpu_affinity_label(status: &CpuAffinitySnapshot) -> String {
    if status.enabled {
        t!(
            "dashboard.adjusted_suffix",
            message = status.message.clone(),
            count = status.adjusted_processes
        )
        .to_string()
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
        SuggestionTarget::Affinity => can_add_affinity_process(&settings.cpu_affinity, process),
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

fn can_add_affinity_process(settings: &CpuAffinitySettings, process: &str) -> bool {
    let process = process.trim();
    !process.is_empty()
        && !settings.contains_rule_for(process)
        && !affinity::is_builtin_excluded(process)
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

fn new_affinity_rule(process: &str) -> CpuAffinityRule {
    CpuAffinityRule {
        enabled: true,
        process_name: process.trim().to_ascii_lowercase(),
        core_mask: default_affinity_mask(),
    }
}

struct SuspensionIndicator {
    label: String,
    bg: u32,
    fg: u32,
    hover: String,
}

struct AffinityIndicator {
    label: String,
    bg: u32,
    fg: u32,
    hover: String,
}

fn suspension_indicator(status: &AppSuspensionSnapshot, process: &str) -> SuspensionIndicator {
    if suspension::is_builtin_excluded(process) {
        SuspensionIndicator {
            label: t!("suspension.indicator.protected").to_string(),
            bg: COLOR_ACCENT_BG,
            fg: COLOR_ACCENT,
            hover: t!("suspension.indicator.protected_help").to_string(),
        }
    } else if suspension::contains_process(&status.network_wake_apps, process) {
        SuspensionIndicator {
            label: t!("suspension.indicator.network").to_string(),
            bg: COLOR_ACCENT_BG,
            fg: COLOR_ACCENT,
            hover: t!("suspension.indicator.network_help").to_string(),
        }
    } else if suspension::contains_process(&status.audio_wake_apps, process) {
        SuspensionIndicator {
            label: t!("suspension.indicator.audio").to_string(),
            bg: COLOR_ACCENT_BG,
            fg: COLOR_ACCENT,
            hover: t!("suspension.indicator.audio_help").to_string(),
        }
    } else if suspension::contains_process(&status.suspended_apps, process) {
        SuspensionIndicator {
            label: t!("suspension.indicator.frozen").to_string(),
            bg: COLOR_SUCCESS_BG,
            fg: COLOR_SUCCESS,
            hover: t!("suspension.indicator.frozen_help").to_string(),
        }
    } else if suspension::contains_process(&status.temporary_thawed_apps, process) {
        SuspensionIndicator {
            label: t!("suspension.indicator.thawed").to_string(),
            bg: COLOR_ACCENT_BG,
            fg: COLOR_ACCENT,
            hover: t!("suspension.indicator.thawed_help").to_string(),
        }
    } else if suspension::contains_process(&status.tracked_apps, process) {
        SuspensionIndicator {
            label: t!("suspension.indicator.waiting").to_string(),
            bg: COLOR_WARNING_BG,
            fg: COLOR_WARNING,
            hover: t!("suspension.indicator.waiting_help").to_string(),
        }
    } else if status.enabled {
        SuspensionIndicator {
            label: t!("suspension.indicator.not_suspended").to_string(),
            bg: COLOR_PANEL_ACTIVE,
            fg: COLOR_MUTED,
            hover: t!("suspension.indicator.not_suspended_help").to_string(),
        }
    } else {
        SuspensionIndicator {
            label: t!("suspension.indicator.off").to_string(),
            bg: COLOR_PANEL_ACTIVE,
            fg: COLOR_DIM,
            hover: t!("suspension.indicator.off_help").to_string(),
        }
    }
}

fn affinity_indicator(status: &CpuAffinitySnapshot, process: &str) -> AffinityIndicator {
    if affinity::is_builtin_excluded(process) {
        AffinityIndicator {
            label: t!("affinity.indicator.protected").to_string(),
            bg: COLOR_ACCENT_BG,
            fg: COLOR_ACCENT,
            hover: t!("affinity.indicator.protected_help").to_string(),
        }
    } else if affinity::contains_process(&status.adjusted_apps, process) {
        AffinityIndicator {
            label: t!("affinity.indicator.pinned").to_string(),
            bg: COLOR_SUCCESS_BG,
            fg: COLOR_SUCCESS,
            hover: t!("affinity.indicator.pinned_help").to_string(),
        }
    } else if status.enabled {
        AffinityIndicator {
            label: t!("affinity.indicator.ready").to_string(),
            bg: COLOR_PANEL_ACTIVE,
            fg: COLOR_MUTED,
            hover: t!("affinity.indicator.ready_help").to_string(),
        }
    } else {
        AffinityIndicator {
            label: t!("affinity.indicator.off").to_string(),
            bg: COLOR_PANEL_ACTIVE,
            fg: COLOR_DIM,
            hover: t!("affinity.indicator.off_help").to_string(),
        }
    }
}

fn can_manual_freeze(status: &AppSuspensionSnapshot, process: &str) -> bool {
    status.enabled && !suspension::contains_process(&status.suspended_apps, process)
}

fn logical_core_count() -> usize {
    affinity::logical_processors().len().clamp(1, 64)
}

fn default_affinity_mask() -> u64 {
    let processors = affinity::logical_processors();
    let mask = affinity_processors_mask(&processors);
    if mask == 0 {
        let core_count = logical_core_count();
        if core_count >= 64 {
            u64::MAX
        } else {
            (1_u64 << core_count) - 1
        }
    } else {
        mask
    }
}

fn affinity_mask_contains(mask: u64, core: usize) -> bool {
    core < 64 && (mask & (1_u64 << core)) != 0
}

fn toggle_affinity_core(mask: &mut u64, core: usize) {
    if core >= 64 {
        return;
    }

    let bit = 1_u64 << core;
    if (*mask & bit) == 0 {
        *mask |= bit;
    } else if mask.count_ones() > 1 {
        *mask &= !bit;
    }
}

fn affinity_mask_label(mask: u64) -> String {
    let processors = affinity::logical_processors();
    let all_mask = affinity_processors_mask(&processors);
    if all_mask != 0 && (mask & all_mask) == all_mask {
        return t!("affinity.all_logical_cpus").to_string();
    }

    let cores = processors
        .iter()
        .filter(|processor| affinity_mask_contains(mask, processor.index))
        .map(affinity_processor_label)
        .collect::<Vec<_>>();

    if cores.is_empty() {
        t!("affinity.no_logical_cpus").to_string()
    } else {
        t!("affinity.logical_cpus", cores = cores.join(", ")).to_string()
    }
}

fn affinity_processors_mask(processors: &[LogicalProcessorInfo]) -> u64 {
    processors
        .iter()
        .filter_map(|processor| affinity_processor_bit(processor.index))
        .fold(0, |mask, bit| mask | bit)
}

fn affinity_processors_kind_mask(
    processors: &[LogicalProcessorInfo],
    kind: LogicalProcessorKind,
) -> u64 {
    processors
        .iter()
        .filter(|processor| processor.kind == kind)
        .filter_map(|processor| affinity_processor_bit(processor.index))
        .fold(0, |mask, bit| mask | bit)
}

fn affinity_processor_bit(index: usize) -> Option<u64> {
    (index < 64).then_some(1_u64 << index)
}

fn affinity_processor_label(processor: &LogicalProcessorInfo) -> String {
    match processor.kind {
        LogicalProcessorKind::Performance => {
            t!("affinity.p_core", index = processor.index).to_string()
        }
        LogicalProcessorKind::Efficiency => {
            t!("affinity.e_core", index = processor.index).to_string()
        }
        LogicalProcessorKind::Standard => {
            t!("affinity.cpu_core", index = processor.index).to_string()
        }
    }
}

fn affinity_processor_tooltip(processor: &LogicalProcessorInfo) -> String {
    let kind = match processor.kind {
        LogicalProcessorKind::Performance => t!("affinity.performance_core_kind").to_string(),
        LogicalProcessorKind::Efficiency => t!("affinity.efficiency_core_kind").to_string(),
        LogicalProcessorKind::Standard => t!("affinity.logical_cpu_kind").to_string(),
    };

    if processor.kind == LogicalProcessorKind::Standard {
        t!(
            "affinity.standard_cpu_tooltip",
            kind = kind,
            index = processor.index,
            core = processor.core_index
        )
        .to_string()
    } else {
        t!(
            "affinity.hybrid_cpu_tooltip",
            kind = kind,
            index = processor.index,
            core = processor.core_index,
            class = processor.efficiency_class
        )
        .to_string()
    }
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
