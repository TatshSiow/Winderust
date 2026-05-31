use std::{
    ffi::{OsStr, OsString},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use eframe::egui;

use crate::{
    activity::{ActivitySnapshot, ActivityState, IdleDetector, InputHook, InputHookEvents},
    automation::BackgroundAutomation,
    config::{self, Settings},
    cpu::{CpuUsageMonitor, CpuUsageSnapshot},
    ecoqos::EcoQosSnapshot,
    foreground::{list_process_names, ForegroundDetector},
    power::{PowerPlan, PowerPlanManager},
    power_source,
    rules::{DecisionEngine, DecisionInput, DecisionOutcome, DecisionState},
    scheduler::{CpuUsageScheduler, Scheduler},
    suspension::AppSuspensionSnapshot,
    tray::{self, TrayIcon},
    ui::{self, Page},
};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY,
    OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

const ACTIVE_PLAN_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const CPU_USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PROCESS_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const SWITCH_RETRY_INTERVAL: Duration = Duration::from_secs(15);

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
    foreground_rule_picker_open: Option<usize>,
    foreground_rule_picker_highlighted: Option<usize>,
    eco_qos_exclusion_input: String,
    eco_qos_picker_open: bool,
    eco_qos_picker_highlighted: Option<usize>,
    suspension_input: String,
    suspension_picker_open: bool,
    suspension_picker_highlighted: Option<usize>,
}

impl PowerLeafApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);
        let hwnd = tray::hwnd_from_creation_context(cc);
        let settings = config::storage::load().unwrap_or_else(|err| {
            eprintln!("{err}");
            Settings::default()
        });
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
            next_schedule: "No active schedule".to_owned(),
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
            foreground_rule_picker_open: None,
            foreground_rule_picker_highlighted: None,
            eco_qos_exclusion_input: String::new(),
            eco_qos_picker_open: false,
            eco_qos_picker_highlighted: None,
            suspension_input: String::new(),
            suspension_picker_open: false,
            suspension_picker_highlighted: None,
        };

        app.sync_tray_icon();
        app.refresh_process_candidates(false);
        app.refresh_power_plans();
        app.run_check();
        app.install_input_hook(&cc.egui_ctx);
        app
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

        self.decision = self.decision_engine.decide(
            &self.settings,
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

    fn install_input_hook(&mut self, ctx: &egui::Context) {
        match InputHook::install(ctx) {
            Ok(input_hook) => {
                self.input_hook = Some(input_hook);
            }
            Err(err) => {
                self.status_message = err;
            }
        }
    }

    fn input_hook_should_check(&self, events: InputHookEvents) -> bool {
        self.settings.general.enabled
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
                self.status_message = format!(
                    "Saved settings to {}",
                    config::storage::config_path().display()
                )
            }
            Err(err) => self.status_message = err,
        }
    }

    fn export_settings_ini(&mut self) {
        match choose_ini_file(self.hwnd, FileDialogMode::Save) {
            Ok(Some(path)) => match config::storage::export_ini_to(&path, &self.settings) {
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

    fn import_settings_ini(&mut self) {
        match choose_ini_file(self.hwnd, FileDialogMode::Open) {
            Ok(Some(path)) => match config::storage::import_ini_from(&path) {
                Ok(settings) => {
                    self.settings = settings;
                    match config::storage::save(&self.settings) {
                        Ok(()) => {
                            self.saved_settings = self.settings.clone();
                            self.status_message =
                                format!("Imported settings from {}", path.display());
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
        if self.settings.general.hide_to_tray {
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
            tray::set_hide_on_close(self.tray_icon.is_some());
        } else if self.tray_icon.take().is_some() {
            tray::set_hide_on_close(false);
            self.status_message = "System tray icon disabled.".to_owned();
        } else {
            tray::set_hide_on_close(false);
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if tray::take_quit_requested() {
            tray::set_hide_on_close(false);
            self.tray_icon = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        let close_requested = ctx.input(|input| input.viewport().close_requested());
        if close_requested && self.settings.general.hide_to_tray && self.tray_icon.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if let Some(hwnd) = self.hwnd {
                tray::hide_window(hwnd);
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            self.status_message =
                "Hidden to system tray. Use the tray icon to show or quit.".to_owned();
        }
    }
}

impl eframe::App for PowerLeafApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_close_request(ctx);
        if tray::is_hidden_to_tray() {
            self.background_automation
                .update_settings(&self.background_settings());
            return;
        }
        self.eco_qos_status = self.background_automation.eco_qos_status();
        self.app_suspension_status = self.background_automation.app_suspension_status();

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

        let mut export_ini_requested = false;
        let mut import_ini_requested = false;

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("PowerLeaf");
            });
        });

        egui::SidePanel::left("navigation")
            .resizable(false)
            .exact_width(190.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                for (section_index, section) in Page::sections().iter().enumerate() {
                    if section_index > 0 {
                        ui.add_space(10.0);
                    }

                    ui.label(
                        egui::RichText::new(section.label)
                            .small()
                            .strong()
                            .color(ui.visuals().weak_text_color()),
                    );
                    ui.add_space(2.0);

                    for page in section.pages {
                        if ui
                            .selectable_label(self.page == *page, page.label())
                            .clicked()
                        {
                            self.page = *page;
                        }
                    }
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(8.0);
            match self.page {
                Page::Dashboard => {
                    ui::dashboard::show(
                        ui,
                        &self.settings,
                        self.current_plan.as_ref(),
                        self.foreground_app.as_deref(),
                        &self.activity,
                        &self.cpu_usage,
                        &self.eco_qos_status,
                        &self.app_suspension_status,
                        &self.decision,
                        &self.next_schedule,
                    );
                }
                Page::Activity => {
                    let action = show_activity_page(
                        ui,
                        &mut self.settings,
                        &self.plans,
                        self.current_plan.as_ref(),
                    );
                    self.handle_power_plan_action(action);
                }
                Page::ForegroundRules => {
                    if self.foreground_rule_picker_open.is_some()
                        && Instant::now() >= self.next_process_refresh
                    {
                        self.refresh_process_candidates(false);
                    }
                    let action = ui::rules_page::show(
                        ui,
                        &mut self.settings.foreground_rules,
                        &self.plans,
                        self.current_plan.as_ref(),
                        &self.process_candidates,
                        &mut self.foreground_rule_picker_open,
                        &mut self.foreground_rule_picker_highlighted,
                    );
                    match action {
                        ui::rules_page::RuleAction::None => {}
                        ui::rules_page::RuleAction::RefreshPlans => self.refresh_power_plans(),
                    }
                }
                Page::Schedule => {
                    let action = ui::schedule_page::show(
                        ui,
                        &mut self.settings.schedule_mode,
                        &self.plans,
                        self.current_plan.as_ref(),
                    );
                    self.handle_power_plan_action(action);
                }
                Page::CpuUsage => {
                    let action = ui::cpu_usage_page::show(
                        ui,
                        &mut self.settings.cpu_usage_mode,
                        &self.plans,
                        self.current_plan.as_ref(),
                    );
                    self.handle_power_plan_action(action);
                }
                Page::EfficiencyMode => {
                    if self.eco_qos_picker_open && Instant::now() >= self.next_process_refresh {
                        self.refresh_process_candidates(false);
                    }
                    ui::efficiency_page::show(
                        ui,
                        &mut self.settings.eco_qos,
                        &self.eco_qos_status,
                        &self.process_candidates,
                        &mut self.eco_qos_exclusion_input,
                        &mut self.eco_qos_picker_open,
                        &mut self.eco_qos_picker_highlighted,
                    );
                }
                Page::AppSuspension => {
                    if self.suspension_picker_open && Instant::now() >= self.next_process_refresh {
                        self.refresh_process_candidates(false);
                    }
                    ui::suspension_page::show(
                        ui,
                        &mut self.settings.app_suspension,
                        &self.app_suspension_status,
                        &self.process_candidates,
                        &mut self.suspension_input,
                        &mut self.suspension_picker_open,
                        &mut self.suspension_picker_highlighted,
                    );
                }
                Page::Settings => {
                    show_settings_page(
                        ui,
                        &mut self.settings,
                        &mut export_ini_requested,
                        &mut import_ini_requested,
                    );
                }
                Page::About => {
                    ui::about_page::show(ui);
                }
            }
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(&self.status_message);
                ui.separator();
                ui.label(&self.decision.reason);
            });
        });

        match show_unsaved_settings_popup(ctx, self.settings != self.saved_settings) {
            UnsavedSettingsAction::None => {}
            UnsavedSettingsAction::Save => self.save_settings(),
            UnsavedSettingsAction::Cancel => self.cancel_settings_changes(),
        }

        if export_ini_requested {
            self.export_settings_ini();
        }
        if import_ini_requested {
            self.import_settings_ini();
        }
        self.sync_tray_icon();
        self.background_automation
            .update_settings(&self.background_settings());

        ctx.request_repaint_after(self.next_repaint_after());
    }
}

impl PowerLeafApp {
    fn handle_power_plan_action(&mut self, action: ui::power_plan_page::PowerPlanAction) {
        match action {
            ui::power_plan_page::PowerPlanAction::None => {}
            ui::power_plan_page::PowerPlanAction::Refresh => self.refresh_power_plans(),
        }
    }

    fn next_repaint_after(&self) -> Duration {
        if self.foreground_rule_picker_open.is_some()
            || self.eco_qos_picker_open
            || self.suspension_picker_open
        {
            return Duration::from_millis(250);
        }
        if self.settings != self.saved_settings {
            return Duration::from_millis(500);
        }

        let until_check = self.next_check.saturating_duration_since(Instant::now());
        until_check
            .max(Duration::from_millis(250))
            .min(Duration::from_secs(5))
    }

    fn cancel_settings_changes(&mut self) {
        self.settings = self.saved_settings.clone();
        self.status_message = "Unsaved settings changes canceled.".to_owned();
    }

    fn background_settings(&self) -> Settings {
        let mut settings = self.settings.clone();
        if self.settings.eco_qos.enabled {
            settings.eco_qos = self.saved_settings.eco_qos.clone();
        }
        if self.settings.app_suspension.enabled {
            settings.app_suspension = self.saved_settings.app_suspension.clone();
        }
        settings
    }
}

fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    ctx.set_style(style);
}

fn show_activity_page(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    plans: &[PowerPlan],
    current_plan: Option<&PowerPlan>,
) -> ui::power_plan_page::PowerPlanAction {
    ui.heading("Action Based Scheduler");
    ui.add_space(8.0);
    ui.checkbox(
        &mut settings.activity_mode.enabled,
        "Enable action-based scheduler",
    );
    ui.label("Change power plan based on input detection.");
    ui.add_space(14.0);

    let action = ui::power_plan_page::show_section(
        ui,
        "Power Plans",
        "Used when this page switches between idle and active states.",
        &mut settings.activity_mode.power_plans,
        plans,
        current_plan,
    );
    ui.add_space(18.0);

    ui.add_enabled_ui(settings.activity_mode.enabled, |ui| {
        ui.strong("Input detection");
        ui.label("Selected input types can switch back to the Active plan.");
        let keyboard_is_only_enabled = settings.activity_mode.input_detection.keyboard
            && !settings.activity_mode.input_detection.mouse;
        ui.add_enabled(
            !keyboard_is_only_enabled,
            egui::Checkbox::new(
                &mut settings.activity_mode.input_detection.keyboard,
                "Keyboard input",
            ),
        );

        let mouse_is_only_enabled = settings.activity_mode.input_detection.mouse
            && !settings.activity_mode.input_detection.keyboard;
        ui.add_enabled(
            !mouse_is_only_enabled,
            egui::Checkbox::new(
                &mut settings.activity_mode.input_detection.mouse,
                "Mouse input",
            ),
        );
        settings.activity_mode.input_detection.ensure_any_enabled();
        settings.activity_mode.switch_to_performance_on_resume =
            settings.activity_mode.input_detection.any_enabled();
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label("Idle timeout");
            ui.add(
                egui::DragValue::new(&mut settings.activity_mode.idle_timeout_seconds)
                    .speed(1.0)
                    .range(1..=7_200)
                    .suffix(" sec"),
            );
        });

        ui.horizontal(|ui| {
            ui.label("Check interval");
            ui.add(
                egui::DragValue::new(&mut settings.general.check_interval_ms)
                    .speed(100.0)
                    .range(250..=60_000)
                    .suffix(" ms"),
            );
        });
    });

    action
}

fn show_settings_page(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    export_ini_requested: &mut bool,
    import_ini_requested: &mut bool,
) {
    ui.heading("Settings");
    ui.add_space(8.0);

    ui.checkbox(&mut settings.general.enabled, "Powerleaf master switch");
    ui.label("This control whether to enable or disable all PowerLeaf features on toggle.");
    ui.add_space(18.0);

    ui.checkbox(
        &mut settings.general.pause_power_plan_switching_while_plugged_in,
        "Stop power plan scheduler on A/C",
    );
    ui.label("Stop power plan switching while on A/C Power.");
    ui.add_space(18.0);

    ui.checkbox(
        &mut settings.general.hide_to_tray,
        "Hide to system tray on close",
    );
    ui.label("Keep Powerleaf running in the tray when closed.");
    ui.add_space(18.0);

    ui.separator();
    ui.heading("Settings Files");
    ui.add_space(8.0);
    ui.label("Export or import all app settings as an INI file.");
    ui.horizontal(|ui| {
        if ui.button("Export settings (.ini)").clicked() {
            *export_ini_requested = true;
        }
        if ui.button("Import settings (.ini)").clicked() {
            *import_ini_requested = true;
        }
    });
}

#[derive(Debug, Clone, Copy)]
enum FileDialogMode {
    Open,
    Save,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnsavedSettingsAction {
    None,
    Save,
    Cancel,
}

fn show_unsaved_settings_popup(ctx: &egui::Context, settings_dirty: bool) -> UnsavedSettingsAction {
    let animation_id = egui::Id::new("unsaved_settings_popup_opacity");
    let opacity = ctx.animate_bool_with_time(animation_id, settings_dirty, 0.18);
    if opacity <= 0.01 {
        return UnsavedSettingsAction::None;
    }

    let mut action = UnsavedSettingsAction::None;
    egui::Area::new(egui::Id::new("unsaved_settings_popup"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -38.0))
        .interactable(settings_dirty)
        .fade_in(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .multiply_with_opacity(opacity)
                .show(ui, |ui| {
                    ui.multiply_opacity(opacity);
                    ui.horizontal(|ui| {
                        ui.colored_label(egui::Color32::from_rgb(190, 120, 40), "Save changes?");
                        ui.add_space(10.0);
                        if ui.button("Discard").clicked() {
                            action = UnsavedSettingsAction::Cancel;
                        }
                        if ui.button("Save").clicked() {
                            action = UnsavedSettingsAction::Save;
                        }
                    });
                });
        });

    action
}

fn choose_ini_file(hwnd: Option<HWND>, mode: FileDialogMode) -> Result<Option<PathBuf>, String> {
    const FILE_BUFFER_LEN: usize = 4096;

    let default_path = match mode {
        FileDialogMode::Open => config::storage::ini_path(),
        FileDialogMode::Save => config::storage::default_export_ini_path(),
    };
    let mut file_buffer = path_to_wide_buffer(&default_path, FILE_BUFFER_LEN);
    let filter = wide_nulls("INI settings (*.ini)\0*.ini\0All files (*.*)\0*.*\0");
    let default_extension = wide_null("ini");
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
