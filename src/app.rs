use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    ffi::{OsStr, OsString},
    fs,
    mem::size_of,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr::null_mut,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use rust_i18n::t;

use chrono::{Local, TimeZone};
use gpui::{
    canvas, deferred, div, img, prelude::*, px, relative, rgb, AnyElement, App, Bounds, Context,
    DragMoveEvent, Empty, Entity, EntityId, Focusable, Hsla, Image, IntoElement, MouseButton,
    NavigationDirection, Pixels, Point, Render, SharedString, Subscription, Task, Timer, Window,
    WindowControlArea,
};
use gpui_component::{
    button::{Button, ButtonCustomVariant, ButtonVariants},
    description_list::DescriptionList,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::{Escape as InputEscape, Input, InputEvent, InputState},
    label::Label,
    scroll::{Scrollable, ScrollableElement},
    slider::{SliderEvent, SliderState, SliderValue},
    tag::Tag,
    theme::Colorize,
    v_flex, ActiveTheme, Disableable, Icon, IconNamed, Sizable,
};

use crate::{
    action_log::{ActionLogAction, ActionLogEntry, ActionLogFeature, ActionLogResult},
    activity::{ActivitySnapshot, ActivityState, IdleDetector, InputHook, InputHookConfig},
    affinity::{self, CpuAffinitySnapshot, LogicalProcessorInfo, LogicalProcessorKind},
    automation::BackgroundAutomation,
    config::{
        self, AccentColorSource, AccentSettings, ActionLogMode, AppLanguage, AppSuspensionRule,
        AppSuspensionSettings, AppThemeMode, BackgroundCpuRestrictionSettings, CpuAffinityMode,
        CpuAffinityRule, CpuAffinitySettings, CpuLimiterRule, CpuLimiterSettings,
        CpuUsageComparison, CpuUsageRule, EcoQosAggressiveness, EcoQosCpuRestrictionControlStyle,
        EcoQosCpuRestrictionMode, EcoQosCpuRestrictionStrategy, EcoQosExclusionRule,
        EcoQosSettings, ForegroundBoostPriority, ForegroundResponsivenessSettings, ForegroundRule,
        ForegroundRules, IoPriorityRule, IoPrioritySettings, MemoryPriorityRule,
        MemoryPrioritySettings, NetworkThresholdUnit, PerformanceModeRule, PerformanceModeSettings,
        PriorityRule, ProcessExclusionRule, ProcessIoPriority, ProcessMemoryPriority,
        ProcessPriority, ScheduleRule, Settings, SmartTrimSettings, WatchdogAction, WatchdogRule,
        WatchdogSettings, WeekdaySetting,
    },
    cpu::{CpuUsageMonitor, CpuUsageSnapshot},
    cpu_limiter::{self, CpuLimiterSnapshot},
    dashboard_metrics::{IoUsageMonitor, IoUsageSnapshot, MemoryUsageMonitor, MemoryUsageSnapshot},
    ecoqos::{self, EcoQosSnapshot},
    foreground::{list_process_candidates, ForegroundDetector, ProcessCandidateInfo},
    io_priority::{self, IoPrioritySnapshot},
    memory_priority::{self, MemoryPrioritySnapshot},
    performance_mode::{self, PerformanceModeSnapshot},
    power::{
        PowerPlan, PowerPlanManager, ProcessorBoostMode, ProcessorPowerAcDcValues,
        ProcessorPowerPreset, ProcessorPowerValues,
    },
    power_source,
    process_icon::load_process_icon,
    responsiveness::{self, AutoBalanceProcessState, ForegroundResponsivenessSnapshot},
    rules::{
        Action, ActionExecution, ActionExecutor, DecisionEngine, DecisionInput, DecisionOutcome,
        DecisionState, PerformanceModeDecision, PowerPlanActionBackend,
        MAX_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD, MIN_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD,
    },
    scheduler::{CpuUsageScheduler, Scheduler},
    smart_trim::{self, SmartTrimSnapshot},
    startup,
    suspension::{self, AppSuspensionSnapshot},
    tray::{self, TrayIcon},
    ui::{self, Page},
    watchdog::{self, WatchdogSnapshot},
};
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HWND};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, REG_DWORD,
    REG_OPTION_NON_VOLATILE,
};
use windows_sys::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY,
    OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

const ACTIVE_PLAN_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const APP_TICK_INTERVAL: Duration = Duration::from_secs(1);
const CPU_USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const CPU_USAGE_HISTORY_LEN: usize = 30;
const DASHBOARD_SUMMARY_CARD_HEIGHT: f32 = 196.0;
const DASHBOARD_CPU_GRAPH_HEIGHT: f32 = 112.0;
const PROCESS_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const TITLE_BAR_HEIGHT: f32 = 40.0;
const PAGE_HEADER_HEIGHT: f32 = 48.0;
const CONTENT_MAX_WIDTH: f32 = 1040.0;
const NAV_PANE_WIDTH: f32 = 276.0;
const FLUENT_RADIUS_CONTROL: f32 = 4.0;
const FLUENT_RADIUS_OVERLAY: f32 = 8.0;
const PROCESS_PICKER_LAYER_PRIORITY: usize = 2;
const DROPDOWN_OPTION_ROW_HEIGHT: f32 = 40.0;
const DROPDOWN_CONTROL_HEIGHT: f32 = 32.0;
const DROPDOWN_SELECT_COMPACT_WIDTH: f32 = 96.0;
const DROPDOWN_SELECT_STANDARD_WIDTH: f32 = 240.0;
const DROPDOWN_SELECT_WIDE_WIDTH: f32 = 280.0;
const DROPDOWN_SURFACE_VERTICAL_PADDING: f32 = 16.0;
const DROPDOWN_OPTION_GAP: f32 = 4.0;
const DROPDOWN_MENU_OFFSET: f32 = 34.0;
const DROPDOWN_VIEWPORT_MARGIN: f32 = 12.0;
const SWITCH_RETRY_INTERVAL: Duration = Duration::from_secs(15);
const MAX_NETWORK_THRESHOLD_BYTES: u64 = 1_000_000_000;
const ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS: u64 = 1;
const ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS: u64 = 60 * 60;
const ACTIVITY_CHECK_INTERVAL_MIN_MS: u64 = 250;
const ACTIVITY_CHECK_INTERVAL_MAX_MS: u64 = 60 * 1000;
const ACTIVITY_CHECK_INTERVAL_STEP_MS: u64 = 250;
const AUTO_BALANCE_THRESHOLD_MIN_PERCENT: u64 = 1;
const AUTO_BALANCE_THRESHOLD_MAX_PERCENT: u64 = 100;
const AUTO_BALANCE_SECONDS_MIN: u64 = 1;
const AUTO_BALANCE_SECONDS_MAX: u64 = 3_600;
const WIN32_PRIORITY_SEPARATION_MIN: u64 = 0;
const WIN32_PRIORITY_SEPARATION_MAX: u64 = 63;
const WIN32_PRIORITY_SEPARATION_WINDOWS_DEFAULT: u32 = 0x26;
const WIN32_PRIORITY_CONTROL_SUB_KEY: &str = "SYSTEM\\CurrentControlSet\\Control\\PriorityControl";
const WIN32_PRIORITY_SEPARATION_VALUE: &str = "Win32PrioritySeparation";
const POWERLEAF_REGISTRY_SUB_KEY: &str = "Software\\PowerLeaf";
const WIN32_PRIORITY_SEPARATION_BACKUP_VALUE: &str = "Win32PrioritySeparationBackup";
const RULE_TITLE_TEXT_SIZE: f32 = 14.0;
const RULE_TITLE_LINE_HEIGHT: f32 = 20.0;
const TEXT_PAGE_TITLE_SIZE: f32 = 28.0;
const TEXT_PAGE_TITLE_LINE_HEIGHT: f32 = 36.0;
const TEXT_PAGE_CRUMB_SIZE: f32 = 20.0;
const TEXT_PAGE_CRUMB_LINE_HEIGHT: f32 = 28.0;
const TEXT_HEADER_SIZE: f32 = RULE_TITLE_TEXT_SIZE;
const TEXT_HEADER_LINE_HEIGHT: f32 = RULE_TITLE_LINE_HEIGHT;
const TEXT_BODY_SIZE: f32 = 14.0;
const TEXT_BODY_LINE_HEIGHT: f32 = 20.0;
const TEXT_CONTROL_SIZE: f32 = 14.0;
const TEXT_CONTROL_LINE_HEIGHT: f32 = 20.0;
const TEXT_LABEL_SIZE: f32 = 12.0;
const TEXT_LABEL_LINE_HEIGHT: f32 = 16.0;
const TEXT_CAPTION_SIZE: f32 = 12.0;
const TEXT_CAPTION_LINE_HEIGHT: f32 = 16.0;

const COLOR_SETTINGS_CARD: u32 = 0x2b2b2b;
const COLOR_SETTINGS_CARD_HOVER: u32 = 0x333333;
const COLOR_SIDEBAR_SELECTED: u32 = 0x303030;
const COLOR_SIDEBAR_HOVER: u32 = 0x2a2a2a;
const COLOR_PANEL_ACTIVE: u32 = 0x3a3a3a;
const COLOR_BORDER: u32 = 0x3f3f3f;
const COLOR_TEXT: u32 = 0xf3f3f3;
const COLOR_MUTED: u32 = 0xc8c8c8;
const COLOR_DIM: u32 = 0x8f8f8f;
const COLOR_ACCENT: u32 = 0x0078d4;
const COLOR_SUCCESS: u32 = 0x8fd17f;
const COLOR_SUCCESS_BG: u32 = 0x263b22;
const COLOR_WARNING: u32 = 0xf2cc60;
const COLOR_WARNING_BG: u32 = 0x4a3b18;

#[derive(Clone, Copy)]
struct DropdownPlacement {
    open_up: bool,
    max_height: Pixels,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActionLogResultFilter {
    All,
    Applied,
    Restored,
    Skipped,
    Failed,
}

impl ActionLogResultFilter {
    const ALL: [Self; 5] = [
        Self::All,
        Self::Applied,
        Self::Restored,
        Self::Skipped,
        Self::Failed,
    ];

    fn matches(self, result: ActionLogResult) -> bool {
        match self {
            Self::All => true,
            Self::Applied => result == ActionLogResult::Applied,
            Self::Restored => result == ActionLogResult::Restored,
            Self::Skipped => result == ActionLogResult::Skipped,
            Self::Failed => result == ActionLogResult::Failed,
        }
    }
}

const ACCENT_PALETTE: [u32; 48] = [
    0xffb900, 0xff8c00, 0xf7630c, 0xca5010, 0xda3b01, 0xef6950, 0xd13438, 0xff4343, 0xe74856,
    0xe81123, 0xea005e, 0xc30052, 0xe3008c, 0xbf0077, 0xc239b3, 0x9a0089, 0x0078d4, 0x0063b1,
    0x8e8cd8, 0x6b69d6, 0x8764b8, 0x744da9, 0xb146c2, 0x881798, 0x0099bc, 0x2d7d9a, 0x00b7c3,
    0x038387, 0x00b294, 0x018574, 0x00cc6a, 0x10893e, 0x107c10, 0x797775, 0x5d5a58, 0x68768a,
    0x567c73, 0x486860, 0x498205, 0x0b6a0b, 0x7a7574, 0x4c4a48, 0x69797e, 0x4a5459, 0x647c64,
    0x525e54, 0x5d5a4f, 0x847545,
];

static UI_ACCENT_COLOR: AtomicU32 = AtomicU32::new(COLOR_ACCENT);
static UI_DARK_MODE: AtomicBool = AtomicBool::new(true);

const NAV_HISTORY_LIMIT: usize = 64;

#[derive(Clone)]
struct ProcessCandidate {
    name: String,
    image_path: Option<PathBuf>,
    icon: Option<Arc<Image>>,
}

impl PartialEq for ProcessCandidate {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.image_path == other.image_path
    }
}

impl Eq for ProcessCandidate {}

pub struct PowerLeafApp {
    settings: Settings,
    saved_settings: Settings,
    page: Page,
    back_stack: Vec<Page>,
    forward_stack: Vec<Page>,
    plans: Vec<PowerPlan>,
    current_plan: Option<PowerPlan>,
    activity: ActivitySnapshot,
    cpu_usage: CpuUsageSnapshot,
    cpu_usage_history: VecDeque<f32>,
    memory_usage: MemoryUsageSnapshot,
    memory_usage_history: VecDeque<f32>,
    io_usage: IoUsageSnapshot,
    io_usage_history: VecDeque<f32>,
    eco_qos_status: EcoQosSnapshot,
    app_suspension_status: AppSuspensionSnapshot,
    cpu_limiter_status: CpuLimiterSnapshot,
    cpu_affinity_status: CpuAffinitySnapshot,
    background_cpu_restriction_status: CpuAffinitySnapshot,
    performance_mode_status: PerformanceModeSnapshot,
    watchdog_status: WatchdogSnapshot,
    foreground_responsiveness_status: ForegroundResponsivenessSnapshot,
    io_priority_status: IoPrioritySnapshot,
    memory_priority_status: MemoryPrioritySnapshot,
    smart_trim_status: SmartTrimSnapshot,
    action_log_entries: Vec<ActionLogEntry>,
    action_log_filter: ActionLogResultFilter,
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
    memory_monitor: MemoryUsageMonitor,
    io_monitor: IoUsageMonitor,
    idle_detector: IdleDetector,
    input_hook: Option<InputHook>,
    foreground_detector: ForegroundDetector,
    scheduler: Scheduler,
    cpu_usage_scheduler: CpuUsageScheduler,
    decision_engine: DecisionEngine,
    hwnd: Option<HWND>,
    tray_icon: Option<TrayIcon>,
    status_message: String,
    process_candidates: Vec<ProcessCandidate>,
    process_icon_cache: HashMap<PathBuf, Option<Arc<Image>>>,
    active_power_plan_picker: Option<String>,
    processor_power_ac_core_parking_min: u64,
    processor_power_ac_performance_min: u64,
    processor_power_ac_performance_max: u64,
    processor_power_ac_boost_mode: ProcessorBoostMode,
    processor_power_dc_core_parking_min: u64,
    processor_power_dc_performance_min: u64,
    processor_power_dc_performance_max: u64,
    processor_power_dc_boost_mode: ProcessorBoostMode,
    processor_power_target_plan_guid: Option<String>,
    processor_power_loaded_plan_guid: Option<String>,
    processor_power_link_ac_dc: bool,
    processor_power_dirty: bool,
    win32_priority_separation_value: Option<u32>,
    win32_priority_separation_edit_value: u32,
    win32_priority_separation_backup: Option<u32>,
    win32_priority_separation_status: String,
    start_minimized_applied: bool,
    editing_rule_title: Option<RuleTitleTarget>,
    editing_numeric: Option<NumericField>,
    expanded_rule_cards: HashSet<RuleCardTarget>,
    expanded_setting_groups: HashSet<SettingGroupTarget>,
    dropdown_anchor_bounds: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    _rule_title_input_subscriptions: Vec<Subscription>,
    _numeric_input_subscription: Option<Subscription>,
    _dashboard_search_subscription: Option<Subscription>,
    _processor_power_slider_subscriptions: Vec<Subscription>,
    _cpu_threshold_slider_subscriptions: Vec<Subscription>,
    _activity_slider_subscriptions: Vec<Subscription>,
    _window_activation_subscription: Subscription,
    inputs: UiInputs,
    _tick_task: Task<()>,
}

struct UiInputs {
    dashboard_search: Entity<InputState>,
    cpu_rule_names: Vec<Entity<InputState>>,
    cpu_rule_thresholds: Vec<Entity<SliderState>>,
    cpu_rule_upper_thresholds: Vec<Entity<SliderState>>,
    schedule_rule_names: Vec<Entity<InputState>>,
    schedule_start_times: Vec<Entity<InputState>>,
    schedule_end_times: Vec<Entity<InputState>>,
    foreground_rule_names: Vec<Entity<InputState>>,
    foreground_rule_processes: Vec<Entity<InputState>>,
    foreground_process: Entity<InputState>,
    eco_qos_exclusion: Entity<InputState>,
    background_cpu_exclusion: Entity<InputState>,
    smart_trim_exclusion: Entity<InputState>,
    suspension_process: Entity<InputState>,
    cpu_limiter_process: Entity<InputState>,
    watchdog_process: Entity<InputState>,
    watchdog_launch_paths: Vec<Entity<InputState>>,
    watchdog_launch_args: Vec<Entity<InputState>>,
    performance_process: Entity<InputState>,
    affinity_process: Entity<InputState>,
    responsiveness_process: Entity<InputState>,
    io_priority_process: Entity<InputState>,
    memory_priority_process: Entity<InputState>,
    numeric_value: Entity<InputState>,
    activity_idle_timeout: Entity<SliderState>,
    activity_check_interval: Entity<SliderState>,
    processor_power_ac_core_parking_min: Entity<SliderState>,
    processor_power_ac_performance_min: Entity<SliderState>,
    processor_power_ac_performance_max: Entity<SliderState>,
    processor_power_dc_core_parking_min: Entity<SliderState>,
    processor_power_dc_performance_min: Entity<SliderState>,
    processor_power_dc_performance_max: Entity<SliderState>,
}

struct InitialProcessorPowerState {
    plans: Vec<PowerPlan>,
    current_plan: Option<PowerPlan>,
    values: ProcessorPowerAcDcValues,
    target_plan_guid: Option<String>,
    loaded_plan_guid: Option<String>,
    status_message: String,
}

struct UiPowerPlanBackend<'a> {
    power: &'a PowerPlanManager,
}

impl PowerPlanActionBackend for UiPowerPlanBackend<'_> {
    fn active_power_plan_guid(&mut self) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn set_active_power_plan(&mut self, plan_guid: &str) -> Result<(), String> {
        self.power.set_active(plan_guid)
    }

    fn set_core_parking(
        &mut self,
        plan_guid: &str,
        min_cores_percent: u8,
        max_cores_percent: u8,
    ) -> Result<(), String> {
        let values = ProcessorPowerAcDcValues::same(ProcessorPowerValues::new(
            u32::from(min_cores_percent),
            u32::from(min_cores_percent),
            u32::from(max_cores_percent),
        ));
        self.power.apply_processor_power_values(plan_guid, values)
    }

    fn set_processor_power_values(
        &mut self,
        plan_guid: &str,
        ac_core_parking_min_percent: u8,
        ac_performance_min_percent: u8,
        ac_performance_max_percent: u8,
        ac_boost_mode: u32,
        dc_core_parking_min_percent: u8,
        dc_performance_min_percent: u8,
        dc_performance_max_percent: u8,
        dc_boost_mode: u32,
    ) -> Result<(), String> {
        let values = ProcessorPowerAcDcValues {
            ac: ProcessorPowerValues::new_with_boost_mode(
                u32::from(ac_core_parking_min_percent),
                u32::from(ac_performance_min_percent),
                u32::from(ac_performance_max_percent),
                ProcessorBoostMode::from_power_value(ac_boost_mode),
            ),
            dc: ProcessorPowerValues::new_with_boost_mode(
                u32::from(dc_core_parking_min_percent),
                u32::from(dc_performance_min_percent),
                u32::from(dc_performance_max_percent),
                ProcessorBoostMode::from_power_value(dc_boost_mode),
            ),
        };
        self.power.apply_processor_power_values(plan_guid, values)
    }
}

fn processor_power_action(plan_guid: &str, values: ProcessorPowerAcDcValues) -> Action {
    let values = values.normalized();
    Action::SetProcessorPowerValues {
        plan_guid: plan_guid.to_owned(),
        ac_core_parking_min_percent: values.ac.core_parking_min as u8,
        ac_performance_min_percent: values.ac.performance_min as u8,
        ac_performance_max_percent: values.ac.performance_max as u8,
        ac_boost_mode: values.ac.boost_mode.power_value(),
        dc_core_parking_min_percent: values.dc.core_parking_min as u8,
        dc_performance_min_percent: values.dc.performance_min as u8,
        dc_performance_max_percent: values.dc.performance_max as u8,
        dc_boost_mode: values.dc.boost_mode.power_value(),
    }
}

#[derive(Clone)]
struct DragStableSlider(EntityId);

impl Render for DragStableSlider {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

enum TickOutcome {
    Continue { changed: bool },
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Win32PrioritySeparationField {
    QuantumDuration,
    QuantumBehaviour,
    ForegroundBoost,
}

#[derive(Clone, Copy)]
struct Win32PrioritySeparationFieldOption {
    bits: u32,
}

impl UiInputs {
    fn new(
        window: &mut Window,
        cx: &mut Context<PowerLeafApp>,
        settings: &Settings,
        processor_power_values: ProcessorPowerAcDcValues,
    ) -> Self {
        let processor_power_values = processor_power_values.normalized();
        Self {
            dashboard_search: make_input(window, cx, "", &t!("dashboard.search_placeholder")),
            cpu_rule_names: settings
                .cpu_usage_mode
                .rules
                .iter()
                .map(|rule| make_input(window, cx, &rule.name, "Rule name"))
                .collect(),
            cpu_rule_thresholds: settings
                .cpu_usage_mode
                .rules
                .iter()
                .map(|rule| make_percent_slider(cx, rule.threshold_percent as u64))
                .collect(),
            cpu_rule_upper_thresholds: settings
                .cpu_usage_mode
                .rules
                .iter()
                .map(|rule| {
                    make_percent_slider(cx, rule.upper_threshold_percent.unwrap_or(100) as u64)
                })
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
            foreground_process: make_input(window, cx, "", "Search running apps..."),
            eco_qos_exclusion: make_input(window, cx, "", "Search running apps..."),
            background_cpu_exclusion: make_input(window, cx, "", "Search running apps..."),
            smart_trim_exclusion: make_input(window, cx, "", "Search running apps..."),
            suspension_process: make_input(window, cx, "", "Search running apps..."),
            cpu_limiter_process: make_input(window, cx, "", "Search running apps..."),
            watchdog_process: make_input(window, cx, "", "Search running apps..."),
            watchdog_launch_paths: settings
                .watchdog
                .rules
                .iter()
                .map(|rule| make_input(window, cx, &rule.launch_path, "Executable path"))
                .collect(),
            watchdog_launch_args: settings
                .watchdog
                .rules
                .iter()
                .map(|rule| make_input(window, cx, &rule.launch_args.join(" "), "Arguments"))
                .collect(),
            performance_process: make_input(window, cx, "", "Search running apps..."),
            affinity_process: make_input(window, cx, "", "Search running apps..."),
            responsiveness_process: make_input(window, cx, "", "Search running apps..."),
            io_priority_process: make_input(window, cx, "", "Search running apps..."),
            memory_priority_process: make_input(window, cx, "", "Search running apps..."),
            numeric_value: make_input(window, cx, "", "Value"),
            activity_idle_timeout: make_range_slider(
                cx,
                settings.activity_mode.idle_timeout_seconds,
                ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS,
                ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS,
                1,
            ),
            activity_check_interval: make_range_slider(
                cx,
                settings.general.check_interval_ms,
                ACTIVITY_CHECK_INTERVAL_MIN_MS,
                ACTIVITY_CHECK_INTERVAL_MAX_MS,
                ACTIVITY_CHECK_INTERVAL_STEP_MS,
            ),
            processor_power_ac_core_parking_min: make_processor_power_slider(
                cx,
                processor_power_values.ac.core_parking_min as u64,
            ),
            processor_power_ac_performance_min: make_processor_power_slider(
                cx,
                processor_power_values.ac.performance_min as u64,
            ),
            processor_power_ac_performance_max: make_processor_power_slider(
                cx,
                processor_power_values.ac.performance_max as u64,
            ),
            processor_power_dc_core_parking_min: make_processor_power_slider(
                cx,
                processor_power_values.dc.core_parking_min as u64,
            ),
            processor_power_dc_performance_min: make_processor_power_slider(
                cx,
                processor_power_values.dc.performance_min as u64,
            ),
            processor_power_dc_performance_max: make_processor_power_slider(
                cx,
                processor_power_values.dc.performance_max as u64,
            ),
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
        sync_slider_vec(
            &mut self.cpu_rule_thresholds,
            settings.cpu_usage_mode.rules.len(),
            cx,
            |index| settings.cpu_usage_mode.rules[index].threshold_percent as u64,
        );
        sync_slider_vec(
            &mut self.cpu_rule_upper_thresholds,
            settings.cpu_usage_mode.rules.len(),
            cx,
            |index| {
                settings.cpu_usage_mode.rules[index]
                    .upper_threshold_percent
                    .unwrap_or(100) as u64
            },
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
        sync_input_vec(
            &mut self.watchdog_launch_paths,
            settings.watchdog.rules.len(),
            window,
            cx,
            |index| settings.watchdog.rules[index].launch_path.clone(),
            "Executable path",
        );
        sync_input_vec(
            &mut self.watchdog_launch_args,
            settings.watchdog.rules.len(),
            window,
            cx,
            |index| settings.watchdog.rules[index].launch_args.join(" "),
            "Arguments",
        );
    }
}

fn default_processor_power_values() -> ProcessorPowerAcDcValues {
    ProcessorPowerAcDcValues::same(ProcessorPowerValues::for_preset(
        ProcessorPowerPreset::Balanced,
    ))
    .normalized()
}

fn load_initial_processor_power_state(power: &PowerPlanManager) -> InitialProcessorPowerState {
    let fallback_values = default_processor_power_values();

    match power.list_plans() {
        Ok(plans) => {
            let current_plan = plans.iter().find(|plan| plan.active).cloned();
            let target_plan = current_plan.as_ref().or_else(|| plans.first()).cloned();
            let status_loaded = t!("status.loaded_power_plans", count = plans.len()).to_string();

            let (values, loaded_plan_guid, status_message) = match target_plan.as_ref() {
                Some(plan) => match power.read_processor_power_values(&plan.guid) {
                    Ok(values) => (values.normalized(), Some(plan.guid.clone()), status_loaded),
                    Err(err) => (fallback_values, None, err),
                },
                None => (fallback_values, None, status_loaded),
            };

            InitialProcessorPowerState {
                plans,
                current_plan,
                values,
                target_plan_guid: target_plan.map(|plan| plan.guid),
                loaded_plan_guid,
                status_message,
            }
        }
        Err(err) => InitialProcessorPowerState {
            plans: Vec::new(),
            current_plan: None,
            values: fallback_values,
            target_plan_guid: None,
            loaded_plan_guid: None,
            status_message: err,
        },
    }
}

impl PowerLeafApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let hwnd = tray::hwnd_from_window(window);
        let settings = config::storage::load().unwrap_or_else(|err| {
            eprintln!("{err}");
            Settings::default()
        });
        let window_activation_subscription =
            cx.observe_window_activation(window, |app, window, cx| {
                if window.is_window_active() && tray::take_restore_requested() {
                    app.refresh_after_tray_restore(window, cx);
                }
            });
        let background_automation = BackgroundAutomation::start(&settings);
        apply_language(settings.general.language);
        apply_appearance_settings(&settings.general, window, cx);
        let power = PowerPlanManager;
        let initial_processor_power = load_initial_processor_power_state(&power);
        let inputs = UiInputs::new(window, cx, &settings, initial_processor_power.values);
        let (win32_priority_separation_value, win32_priority_separation_status) =
            read_win32_priority_separation_with_status();
        let win32_priority_separation_edit_value = win32_priority_separation_value
            .map(normalize_win32_priority_separation_value)
            .unwrap_or(WIN32_PRIORITY_SEPARATION_WINDOWS_DEFAULT);
        let win32_priority_separation_backup = read_win32_priority_separation_backup();

        let mut app = Self {
            saved_settings: settings.clone(),
            settings,
            page: Page::Dashboard,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            plans: initial_processor_power.plans,
            current_plan: initial_processor_power.current_plan,
            activity: ActivitySnapshot {
                state: ActivityState::Unknown,
                idle_for: None,
            },
            cpu_usage: CpuUsageSnapshot::default(),
            cpu_usage_history: VecDeque::with_capacity(CPU_USAGE_HISTORY_LEN),
            memory_usage: MemoryUsageSnapshot::default(),
            memory_usage_history: VecDeque::with_capacity(CPU_USAGE_HISTORY_LEN),
            io_usage: IoUsageSnapshot::default(),
            io_usage_history: VecDeque::with_capacity(CPU_USAGE_HISTORY_LEN),
            eco_qos_status: EcoQosSnapshot::default(),
            app_suspension_status: AppSuspensionSnapshot::default(),
            cpu_limiter_status: CpuLimiterSnapshot::default(),
            cpu_affinity_status: CpuAffinitySnapshot::default(),
            background_cpu_restriction_status: CpuAffinitySnapshot::default(),
            performance_mode_status: PerformanceModeSnapshot::default(),
            watchdog_status: WatchdogSnapshot::default(),
            foreground_responsiveness_status: ForegroundResponsivenessSnapshot::default(),
            io_priority_status: IoPrioritySnapshot::default(),
            memory_priority_status: MemoryPrioritySnapshot::default(),
            smart_trim_status: SmartTrimSnapshot::default(),
            action_log_entries: Vec::new(),
            action_log_filter: ActionLogResultFilter::All,
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
            power,
            background_automation,
            cpu_monitor: CpuUsageMonitor::default(),
            memory_monitor: MemoryUsageMonitor,
            io_monitor: IoUsageMonitor::default(),
            idle_detector: IdleDetector,
            input_hook: None,
            foreground_detector: ForegroundDetector,
            scheduler: Scheduler,
            cpu_usage_scheduler: CpuUsageScheduler::default(),
            decision_engine: DecisionEngine,
            hwnd,
            tray_icon: None,
            status_message: initial_processor_power.status_message,
            process_candidates: Vec::new(),
            process_icon_cache: HashMap::new(),
            active_power_plan_picker: None,
            processor_power_ac_core_parking_min: initial_processor_power.values.ac.core_parking_min
                as u64,
            processor_power_ac_performance_min: initial_processor_power.values.ac.performance_min
                as u64,
            processor_power_ac_performance_max: initial_processor_power.values.ac.performance_max
                as u64,
            processor_power_ac_boost_mode: initial_processor_power.values.ac.boost_mode,
            processor_power_dc_core_parking_min: initial_processor_power.values.dc.core_parking_min
                as u64,
            processor_power_dc_performance_min: initial_processor_power.values.dc.performance_min
                as u64,
            processor_power_dc_performance_max: initial_processor_power.values.dc.performance_max
                as u64,
            processor_power_dc_boost_mode: initial_processor_power.values.dc.boost_mode,
            processor_power_target_plan_guid: initial_processor_power.target_plan_guid,
            processor_power_loaded_plan_guid: initial_processor_power.loaded_plan_guid,
            processor_power_link_ac_dc: initial_processor_power.values.ac
                == initial_processor_power.values.dc,
            processor_power_dirty: false,
            win32_priority_separation_value,
            win32_priority_separation_edit_value,
            win32_priority_separation_backup,
            win32_priority_separation_status,
            start_minimized_applied: false,
            editing_rule_title: None,
            editing_numeric: None,
            expanded_rule_cards: HashSet::new(),
            expanded_setting_groups: HashSet::new(),
            dropdown_anchor_bounds: Rc::new(RefCell::new(HashMap::new())),
            _rule_title_input_subscriptions: Vec::new(),
            _numeric_input_subscription: None,
            _dashboard_search_subscription: None,
            _processor_power_slider_subscriptions: Vec::new(),
            _cpu_threshold_slider_subscriptions: Vec::new(),
            _activity_slider_subscriptions: Vec::new(),
            _window_activation_subscription: window_activation_subscription,
            inputs,
            _tick_task: Task::ready(()),
        };

        app.rebuild_rule_title_input_subscriptions(window, cx);
        app.subscribe_to_numeric_input(window, cx);
        app.subscribe_to_dashboard_search_input(window, cx);
        app.subscribe_to_processor_power_sliders(window, cx);
        app.rebuild_cpu_threshold_slider_subscriptions(window, cx);
        app.subscribe_to_activity_sliders(window, cx);
        window.on_window_should_close(cx, |_, _| !tray::is_hidden_to_tray());
        app.sync_tray_icon();
        app.refresh_process_candidates(false);
        app.run_check();
        app.sync_processor_power_slider_states(window, cx);
        app.sync_input_hook();
        app.schedule_tick(window, cx);
        app
    }

    fn navigate_to(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page == page {
            return;
        }

        Self::push_navigation_page(&mut self.back_stack, self.page);
        self.page = page;
        self.forward_stack.clear();
        cx.notify();
    }

    fn navigate_back(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.back_stack.pop() else {
            return;
        };

        Self::push_navigation_page(&mut self.forward_stack, self.page);
        self.page = page;
        cx.notify();
    }

    fn navigate_forward(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.forward_stack.pop() else {
            return;
        };

        Self::push_navigation_page(&mut self.back_stack, self.page);
        self.page = page;
        cx.notify();
    }

    fn push_navigation_page(stack: &mut Vec<Page>, page: Page) {
        if stack.last().copied() == Some(page) {
            return;
        }

        stack.push(page);
        if stack.len() > NAV_HISTORY_LIMIT {
            stack.remove(0);
        }
    }

    fn schedule_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._tick_task = cx.spawn_in(window, async move |this, cx| {
            Timer::after(APP_TICK_INTERVAL).await;
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

    fn refresh_power_plans(&mut self) {
        match self.power.list_plans() {
            Ok(plans) => {
                self.plans = plans;
                self.current_plan = self.plans.iter().find(|plan| plan.active).cloned();
                self.next_active_plan_refresh = Instant::now() + ACTIVE_PLAN_REFRESH_INTERVAL;
                self.status_message =
                    t!("status.loaded_power_plans", count = self.plans.len()).to_string();
                self.ensure_processor_power_target_plan();
                self.sync_processor_power_values_from_target_plan(false);
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
                    self.ensure_processor_power_target_plan();
                    self.sync_processor_power_values_from_target_plan(false);
                }
            }
            Err(err) => self.status_message = err,
        }
    }

    fn ensure_processor_power_target_plan(&mut self) {
        let target_still_available = self
            .processor_power_target_plan_guid
            .as_deref()
            .is_some_and(|target| {
                self.plans
                    .iter()
                    .any(|plan| plan.guid.eq_ignore_ascii_case(target))
            });
        if target_still_available {
            return;
        }

        self.processor_power_target_plan_guid = self
            .current_plan
            .as_ref()
            .or_else(|| self.plans.first())
            .map(|plan| plan.guid.clone());
    }

    fn processor_power_target_plan(&self) -> Option<PowerPlan> {
        self.processor_power_target_plan_guid
            .as_deref()
            .and_then(|target| {
                self.plans
                    .iter()
                    .find(|plan| plan.guid.eq_ignore_ascii_case(target))
            })
            .cloned()
            .or_else(|| self.current_plan.clone())
    }

    fn set_processor_power_target_plan(&mut self, guid: String) {
        self.processor_power_target_plan_guid = Some(guid);
        self.active_power_plan_picker = None;
        self.sync_processor_power_values_from_target_plan(true);
    }

    fn set_processor_power_target_plan_option(&mut self, guid: Option<String>) {
        if let Some(guid) = guid {
            self.set_processor_power_target_plan(guid);
        } else {
            self.active_power_plan_picker = None;
        }
    }

    fn sync_processor_power_values_from_target_plan(&mut self, force: bool) -> bool {
        let Some(plan) = self.processor_power_target_plan() else {
            self.processor_power_loaded_plan_guid = None;
            return false;
        };
        let same_plan = self
            .processor_power_loaded_plan_guid
            .as_deref()
            .is_some_and(|guid| guid.eq_ignore_ascii_case(&plan.guid));
        if !force && same_plan && self.processor_power_dirty {
            return true;
        }

        match self.power.read_processor_power_values(&plan.guid) {
            Ok(values) => {
                self.set_processor_power_values(values.normalized());
                self.processor_power_loaded_plan_guid = Some(plan.guid);
                self.processor_power_dirty = false;
                true
            }
            Err(err) => {
                self.status_message = err;
                false
            }
        }
    }

    fn processor_power_values(&self) -> ProcessorPowerAcDcValues {
        ProcessorPowerAcDcValues::new(
            ProcessorPowerValues::new_with_boost_mode(
                self.processor_power_ac_core_parking_min as u32,
                self.processor_power_ac_performance_min as u32,
                self.processor_power_ac_performance_max as u32,
                self.processor_power_ac_boost_mode,
            ),
            ProcessorPowerValues::new_with_boost_mode(
                self.processor_power_dc_core_parking_min as u32,
                self.processor_power_dc_performance_min as u32,
                self.processor_power_dc_performance_max as u32,
                self.processor_power_dc_boost_mode,
            ),
        )
        .normalized()
    }

    fn set_processor_power_values(&mut self, values: ProcessorPowerAcDcValues) {
        let values = values.normalized();
        self.processor_power_ac_core_parking_min = values.ac.core_parking_min as u64;
        self.processor_power_ac_performance_min = values.ac.performance_min as u64;
        self.processor_power_ac_performance_max = values.ac.performance_max as u64;
        self.processor_power_ac_boost_mode = values.ac.boost_mode;
        self.processor_power_dc_core_parking_min = values.dc.core_parking_min as u64;
        self.processor_power_dc_performance_min = values.dc.performance_min as u64;
        self.processor_power_dc_performance_max = values.dc.performance_max as u64;
        self.processor_power_dc_boost_mode = values.dc.boost_mode;
    }

    fn set_processor_power_boost_mode(
        &mut self,
        source: ProcessorPowerSource,
        boost_mode: ProcessorBoostMode,
    ) {
        self.assign_processor_power_boost_mode(source, boost_mode);
        if self.processor_power_link_ac_dc {
            self.assign_processor_power_boost_mode(source.paired(), boost_mode);
        }
        self.active_power_plan_picker = None;
        self.processor_power_dirty = true;
    }

    fn assign_processor_power_boost_mode(
        &mut self,
        source: ProcessorPowerSource,
        boost_mode: ProcessorBoostMode,
    ) {
        match source {
            ProcessorPowerSource::Ac => self.processor_power_ac_boost_mode = boost_mode,
            ProcessorPowerSource::Dc => self.processor_power_dc_boost_mode = boost_mode,
        }
    }

    fn set_processor_power_slider_value(&mut self, slider: ProcessorPowerSlider, value: u64) {
        let value = value.min(100);
        self.assign_processor_power_slider_value(slider, value);
        if self.processor_power_link_ac_dc {
            self.assign_processor_power_slider_value(slider.paired_power_source(), value);
        }
        self.processor_power_dirty = true;
    }

    fn assign_processor_power_slider_value(&mut self, slider: ProcessorPowerSlider, value: u64) {
        match slider {
            ProcessorPowerSlider::AcCoreParkingMin => {
                self.processor_power_ac_core_parking_min = value;
            }
            ProcessorPowerSlider::AcPerformanceMin => {
                self.processor_power_ac_performance_min = value;
            }
            ProcessorPowerSlider::AcPerformanceMax => {
                self.processor_power_ac_performance_max = value;
            }
            ProcessorPowerSlider::DcCoreParkingMin => {
                self.processor_power_dc_core_parking_min = value;
            }
            ProcessorPowerSlider::DcPerformanceMin => {
                self.processor_power_dc_performance_min = value;
            }
            ProcessorPowerSlider::DcPerformanceMax => {
                self.processor_power_dc_performance_max = value;
            }
        }
    }

    fn sync_processor_power_slider_states(&self, window: &mut Window, cx: &mut Context<Self>) {
        for (slider, value) in [
            (
                ProcessorPowerSlider::AcCoreParkingMin,
                self.processor_power_ac_core_parking_min,
            ),
            (
                ProcessorPowerSlider::AcPerformanceMin,
                self.processor_power_ac_performance_min,
            ),
            (
                ProcessorPowerSlider::AcPerformanceMax,
                self.processor_power_ac_performance_max,
            ),
            (
                ProcessorPowerSlider::DcCoreParkingMin,
                self.processor_power_dc_core_parking_min,
            ),
            (
                ProcessorPowerSlider::DcPerformanceMin,
                self.processor_power_dc_performance_min,
            ),
            (
                ProcessorPowerSlider::DcPerformanceMax,
                self.processor_power_dc_performance_max,
            ),
        ] {
            let input = processor_power_slider_input(&self.inputs, slider);
            let value = value.min(100) as f32;
            input.update(cx, |state, cx| {
                if (state.value().end() - value).abs() > f32::EPSILON {
                    state.set_value(value, window, cx);
                }
            });
        }
    }

    fn set_cpu_threshold_slider_value(&mut self, slider: CpuThresholdSlider, value: u8) {
        let value = value.min(100);
        match slider {
            CpuThresholdSlider::Lower(index) => {
                if let Some(rule) = self.settings.cpu_usage_mode.rules.get_mut(index) {
                    rule.threshold_percent = value;
                }
            }
            CpuThresholdSlider::Upper(index) => {
                if let Some(rule) = self.settings.cpu_usage_mode.rules.get_mut(index) {
                    rule.upper_threshold_percent = Some(value);
                }
            }
        }
    }

    fn sync_cpu_threshold_slider_states(&self, window: &mut Window, cx: &mut Context<Self>) {
        for (index, rule) in self.settings.cpu_usage_mode.rules.iter().enumerate() {
            self.sync_cpu_threshold_slider_state(
                CpuThresholdSlider::Lower(index),
                rule.threshold_percent,
                window,
                cx,
            );
            self.sync_cpu_threshold_slider_state(
                CpuThresholdSlider::Upper(index),
                rule.upper_threshold_percent.unwrap_or(100),
                window,
                cx,
            );
        }
    }

    fn sync_cpu_threshold_slider_state(
        &self,
        slider: CpuThresholdSlider,
        value: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = cpu_threshold_slider_input(&self.inputs, slider) else {
            return;
        };
        let value = value.min(100) as f32;
        input.update(cx, |state, cx| {
            if (state.value().end() - value).abs() > f32::EPSILON {
                state.set_value(value, window, cx);
            }
        });
    }

    fn set_activity_slider_value(&mut self, slider: ActivitySlider, value: u64) {
        match slider {
            ActivitySlider::IdleTimeout => {
                self.settings.activity_mode.idle_timeout_seconds = value.clamp(
                    ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS,
                    ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS,
                );
            }
            ActivitySlider::CheckInterval => {
                self.settings.general.check_interval_ms =
                    snap_to_step(value, ACTIVITY_CHECK_INTERVAL_STEP_MS).clamp(
                        ACTIVITY_CHECK_INTERVAL_MIN_MS,
                        ACTIVITY_CHECK_INTERVAL_MAX_MS,
                    );
            }
        }
    }

    fn sync_activity_slider_states(&self, window: &mut Window, cx: &mut Context<Self>) {
        for (slider, input, value) in [
            (
                ActivitySlider::IdleTimeout,
                self.inputs.activity_idle_timeout.clone(),
                self.settings.activity_mode.idle_timeout_seconds.clamp(
                    ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS,
                    ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS,
                ),
            ),
            (
                ActivitySlider::CheckInterval,
                self.inputs.activity_check_interval.clone(),
                self.settings.general.check_interval_ms.clamp(
                    ACTIVITY_CHECK_INTERVAL_MIN_MS,
                    ACTIVITY_CHECK_INTERVAL_MAX_MS,
                ),
            ),
        ] {
            let value = activity_slider_normalized_value(slider, value) as f32;
            input.update(cx, |state, cx| {
                if (state.value().end() - value).abs() > f32::EPSILON {
                    state.set_value(value, window, cx);
                }
            });
        }
    }

    fn refresh_processor_power_values(&mut self) {
        let Some(plan) = self.processor_power_target_plan() else {
            self.status_message = t!("processor_power.no_active_plan").to_string();
            return;
        };
        if self.sync_processor_power_values_from_target_plan(true) {
            self.status_message =
                t!("processor_power.loaded_values", plan = plan.display_name()).to_string();
        }
    }

    fn fill_processor_power_preset(&mut self, preset: ProcessorPowerPreset) {
        let values = ProcessorPowerValues::for_preset(preset);
        self.set_processor_power_values(ProcessorPowerAcDcValues::same(values));
        self.processor_power_dirty = true;
        self.status_message = t!(
            "processor_power.loaded_preset",
            preset = processor_power_preset_label(preset)
        )
        .to_string();
    }

    fn processor_power_matches_preset(&self, preset: ProcessorPowerPreset) -> bool {
        let values = ProcessorPowerValues::for_preset(preset);
        self.processor_power_values() == ProcessorPowerAcDcValues::same(values).normalized()
    }

    fn apply_processor_power_custom(&mut self) {
        let Some(plan) = self.processor_power_target_plan() else {
            self.status_message = t!("processor_power.no_active_plan").to_string();
            return;
        };

        let values = self.processor_power_values();
        self.set_processor_power_values(values);

        let action = processor_power_action(&plan.guid, values);
        let mut backend = UiPowerPlanBackend { power: &self.power };
        match ActionExecutor.apply_power_plan_action(&action, &mut backend) {
            ActionExecution::Applied | ActionExecution::AlreadyApplied => {
                self.processor_power_loaded_plan_guid = Some(plan.guid.clone());
                self.processor_power_dirty = false;
                self.status_message =
                    t!("processor_power.applied_custom", plan = plan.display_name()).to_string();
                self.refresh_active_plan();
            }
            ActionExecution::Failed(err) => self.status_message = err,
            ActionExecution::Unsupported => {
                self.status_message = "Processor power action is not supported.".to_owned();
            }
        }
    }

    fn run_check(&mut self) {
        if Instant::now() >= self.next_active_plan_refresh {
            self.refresh_active_plan();
        }

        let decision_settings = self.runtime_settings();
        self.activity = self.idle_detector.snapshot(Duration::from_secs(
            decision_settings.activity_mode.idle_timeout_seconds,
        ));
        self.refresh_dashboard_resource_samples();
        self.foreground_app = self.foreground_detector.process_name();
        let schedule = self
            .scheduler
            .current_decision(&decision_settings.schedule_mode);
        let cpu_usage = self
            .cpu_usage_scheduler
            .current_decision(&decision_settings.cpu_usage_mode, self.cpu_usage.percent);
        self.next_schedule = self
            .scheduler
            .next_switch_label(&decision_settings.schedule_mode);

        self.decision = self.decision_engine.decide(
            &decision_settings,
            DecisionInput {
                activity_state: self.activity.state,
                foreground_app: self.foreground_app.clone(),
                plugged_in: power_source::is_plugged_in(),
                performance_mode: performance_mode_decision(&self.performance_mode_status),
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
        let memory_usage_percent = self.memory_usage.percent;
        let io_bytes_per_second = self.io_usage.bytes_per_second;
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
            || self.memory_usage.percent != memory_usage_percent
            || self.io_usage.bytes_per_second != io_bytes_per_second
            || self.foreground_app != foreground_app
            || self.decision.target_guid != decision_target_guid
            || self.decision.state != decision_state
            || self.decision.reason != decision_reason
            || self.next_schedule != next_schedule
            || self.plans != plans
            || self.current_plan != current_plan
            || self.status_message != status_message
    }

    fn refresh_dashboard_resource_samples(&mut self) -> bool {
        if Instant::now() < self.next_cpu_usage_refresh {
            return false;
        }

        let previous_cpu_percent = self.cpu_usage.percent;
        let previous_memory_percent = self.memory_usage.percent;
        let previous_io_bytes_per_second = self.io_usage.bytes_per_second;

        self.cpu_usage = self.cpu_monitor.sample();
        self.memory_usage = self.memory_monitor.sample();
        self.io_usage = self.io_monitor.sample();

        let mut changed = self.cpu_usage.percent != previous_cpu_percent
            || self.memory_usage.percent != previous_memory_percent
            || self.io_usage.bytes_per_second != previous_io_bytes_per_second;

        if let Some(percent) = self.cpu_usage.percent {
            if self.cpu_usage_history.len() == CPU_USAGE_HISTORY_LEN {
                self.cpu_usage_history.pop_front();
            }
            self.cpu_usage_history.push_back(percent.clamp(0.0, 100.0));
            changed = true;
        }
        if let Some(percent) = self.memory_usage.percent {
            if self.memory_usage_history.len() == CPU_USAGE_HISTORY_LEN {
                self.memory_usage_history.pop_front();
            }
            self.memory_usage_history
                .push_back(percent.clamp(0.0, 100.0));
            changed = true;
        }
        if let Some(bytes_per_second) = self.io_usage.bytes_per_second {
            if self.io_usage_history.len() == CPU_USAGE_HISTORY_LEN {
                self.io_usage_history.pop_front();
            }
            self.io_usage_history
                .push_back(bytes_per_second.clamp(0.0, f32::MAX as f64) as f32);
            changed = true;
        }

        self.next_cpu_usage_refresh = Instant::now() + CPU_USAGE_REFRESH_INTERVAL;
        changed
    }

    fn install_input_hook(&mut self) {
        let settings = self.runtime_settings();
        match InputHook::install(
            input_hook_config(&settings),
            self.background_automation.input_event_callback(),
        ) {
            Ok(input_hook) => {
                self.input_hook = Some(input_hook);
            }
            Err(err) => {
                self.status_message = err;
            }
        }
    }

    fn sync_input_hook(&mut self) {
        let settings = self.runtime_settings();
        if input_hook_required(&settings) {
            let config = input_hook_config(&settings);
            if self
                .input_hook
                .as_ref()
                .is_none_or(|input_hook| input_hook.config() != config)
            {
                self.input_hook = None;
                self.install_input_hook();
            }
        } else {
            self.input_hook = None;
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

        let action = Action::SwitchPowerPlan {
            plan_guid: target_guid.to_owned(),
        };
        let mut backend = UiPowerPlanBackend { power: &self.power };
        match ActionExecutor.apply_power_plan_action(&action, &mut backend) {
            ActionExecution::Applied | ActionExecution::AlreadyApplied => {
                self.status_message =
                    t!("status.switched_power_plan", reason = self.decision.reason).to_string();
                self.refresh_power_plans();
            }
            ActionExecution::Failed(err) => self.status_message = err,
            ActionExecution::Unsupported => {
                self.status_message = "Power plan action is not supported.".to_owned();
            }
        }
    }

    fn save_settings(&mut self) {
        match config::storage::save(&self.settings) {
            Ok(()) => {
                self.saved_settings = self.settings.clone();
                self.sync_input_hook();
                self.background_automation
                    .update_settings(&self.background_settings());
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

    fn export_action_log_csv(&mut self) {
        if self.action_log_entries.is_empty() {
            self.status_message = t!("status.action_log_export_empty").to_string();
            return;
        }

        match choose_action_log_export_file(self.hwnd) {
            Ok(Some(path)) => {
                match fs::write(&path, action_log_entries_to_csv(&self.action_log_entries)) {
                    Ok(()) => {
                        self.status_message =
                            t!("status.exported_action_log", path = path.display()).to_string();
                    }
                    Err(err) => {
                        self.status_message =
                            format!("Failed to export log to {}: {err}", path.display());
                    }
                }
            }
            Ok(None) => {
                self.status_message = t!("status.action_log_export_canceled").to_string();
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
                    apply_appearance_settings(&self.settings.general, window, cx);
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
                            self.sync_input_hook();
                            self.background_automation
                                .update_settings(&self.background_settings());
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

    fn process_candidates_from_info(
        &mut self,
        processes: Vec<ProcessCandidateInfo>,
    ) -> Vec<ProcessCandidate> {
        processes
            .into_iter()
            .map(|process| {
                let icon = process
                    .image_path
                    .as_deref()
                    .and_then(|path| self.cached_process_icon(path));
                ProcessCandidate {
                    name: process.name,
                    image_path: process.image_path,
                    icon,
                }
            })
            .collect()
    }

    fn cached_process_icon(&mut self, path: &Path) -> Option<Arc<Image>> {
        if !self.process_icon_cache.contains_key(path) {
            let icon = load_process_icon(path);
            self.process_icon_cache.insert(path.to_path_buf(), icon);
        }

        self.process_icon_cache.get(path).and_then(Clone::clone)
    }

    fn refresh_process_candidates(&mut self, report_status: bool) -> bool {
        self.next_process_refresh = Instant::now() + PROCESS_REFRESH_INTERVAL;
        match list_process_candidates() {
            Ok(processes) => {
                let processes = self.process_candidates_from_info(processes);
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

    fn refresh_after_tray_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.next_check = Instant::now();
        match self.tick(window) {
            TickOutcome::Continue { changed } => {
                self.schedule_tick(window, cx);
                if changed {
                    cx.notify();
                }
            }
            TickOutcome::Stop => {}
        }
    }

    fn tick(&mut self, window: &mut Window) -> TickOutcome {
        if tray::take_quit_requested() {
            tray::set_hide_on_close(false);
            self.tray_icon = None;
            window.remove_window();
            return TickOutcome::Stop;
        }

        let mut changed = self.apply_start_minimized(window);
        changed |= self.apply_pending_auto_exclusions();
        if tray::is_hidden_to_tray() {
            self.sync_input_hook();
            self.background_automation
                .update_settings(&self.background_settings());
            return TickOutcome::Stop;
        }

        let background_status = self.background_automation.status_snapshot();
        if self.eco_qos_status != background_status.eco_qos {
            self.eco_qos_status = background_status.eco_qos;
            changed = true;
        }

        if self.app_suspension_status != background_status.app_suspension {
            self.app_suspension_status = background_status.app_suspension;
            changed = true;
        }

        if self.cpu_limiter_status != background_status.cpu_limiter {
            self.cpu_limiter_status = background_status.cpu_limiter;
            changed = true;
        }

        if self.cpu_affinity_status != background_status.cpu_affinity {
            self.cpu_affinity_status = background_status.cpu_affinity;
            changed = true;
        }

        if self.background_cpu_restriction_status != background_status.background_cpu_restriction {
            self.background_cpu_restriction_status = background_status.background_cpu_restriction;
            changed = true;
        }

        if self.performance_mode_status != background_status.performance_mode {
            self.performance_mode_status = background_status.performance_mode;
            changed = true;
        }

        if self.watchdog_status != background_status.watchdog {
            self.watchdog_status = background_status.watchdog;
            changed = true;
        }

        if self.foreground_responsiveness_status != background_status.foreground_responsiveness {
            self.foreground_responsiveness_status = background_status.foreground_responsiveness;
            changed = true;
        }

        if self.io_priority_status != background_status.io_priority {
            self.io_priority_status = background_status.io_priority;
            changed = true;
        }

        if self.memory_priority_status != background_status.memory_priority {
            self.memory_priority_status = background_status.memory_priority;
            changed = true;
        }

        if self.smart_trim_status != background_status.smart_trim {
            self.smart_trim_status = background_status.smart_trim;
            changed = true;
        }

        if self.action_log_entries != background_status.action_log_entries {
            self.action_log_entries = background_status.action_log_entries;
            changed = true;
        }

        if self.page_uses_process_candidates() && Instant::now() >= self.next_process_refresh {
            changed |= self.refresh_process_candidates(false);
        }

        changed |= self.refresh_dashboard_resource_samples();

        let should_check_now = Instant::now() >= self.next_check;

        if should_check_now {
            changed |= self.run_check_changed();
            self.next_check = Instant::now()
                + Duration::from_millis(
                    self.settings
                        .general
                        .check_interval_ms
                        .max(ACTIVITY_CHECK_INTERVAL_MIN_MS),
                );
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

    fn apply_pending_auto_exclusions(&mut self) -> bool {
        let pending = self.background_automation.take_pending_auto_exclusions();
        let mut changed = false;

        for process in pending.eco_qos {
            if can_add_eco_qos_process(&self.settings.eco_qos, &process) {
                self.settings
                    .eco_qos
                    .efficiency_whitelist
                    .push(new_eco_qos_exclusion_rule(&process));
                changed = true;
            }
        }

        for process in pending.background_cpu_restriction {
            if can_add_background_cpu_exclusion(&self.settings.background_cpu_restriction, &process)
            {
                self.settings
                    .background_cpu_restriction
                    .exclusions
                    .push(new_process_exclusion_rule(&process));
                changed = true;
            }
        }

        if changed {
            self.save_settings();
        }

        changed
    }

    fn page_uses_process_candidates(&self) -> bool {
        matches!(
            self.page,
            Page::ForegroundRules
                | Page::EfficiencyMode
                | Page::AppSuspension
                | Page::CpuLimiter
                | Page::BackgroundCpuRestriction
                | Page::Watchdog
                | Page::ForegroundResponsiveness
                | Page::IoPriority
                | Page::MemoryPriority
                | Page::PerformanceMode
                | Page::CpuAffinity
        )
    }

    fn cancel_settings_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings = self.saved_settings.clone();
        apply_language(self.settings.general.language);
        apply_appearance_settings(&self.settings.general, window, cx);
        self.status_message = t!("status.unsaved_canceled").to_string();
        self.editing_rule_title = None;
        self.expanded_rule_cards.clear();
        self.rebuild_inputs(window, cx);
    }

    fn rebuild_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.settings.clone();
        let processor_power_values = self.processor_power_values();
        self.editing_rule_title = None;
        self.expanded_rule_cards.clear();
        self.inputs = UiInputs::new(window, cx, &settings, processor_power_values);
        self.rebuild_rule_title_input_subscriptions(window, cx);
        self.subscribe_to_dashboard_search_input(window, cx);
        self.subscribe_to_processor_power_sliders(window, cx);
        self.rebuild_cpu_threshold_slider_subscriptions(window, cx);
        self.subscribe_to_activity_sliders(window, cx);
    }

    fn rule_title_input_count(&self) -> usize {
        self.inputs.schedule_rule_names.len() + self.inputs.cpu_rule_names.len()
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

    fn subscribe_to_numeric_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._numeric_input_subscription = Some(cx.subscribe_in(
            &self.inputs.numeric_value,
            window,
            move |app, _, event: &InputEvent, _, cx| {
                app.handle_numeric_input_event(event, cx);
            },
        ));
    }

    fn subscribe_to_dashboard_search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._dashboard_search_subscription = Some(cx.subscribe_in(
            &self.inputs.dashboard_search,
            window,
            move |_, _, _: &InputEvent, _, cx| {
                cx.notify();
            },
        ));
    }

    fn subscribe_to_processor_power_sliders(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._processor_power_slider_subscriptions.clear();
        for slider in [
            ProcessorPowerSlider::AcCoreParkingMin,
            ProcessorPowerSlider::AcPerformanceMin,
            ProcessorPowerSlider::AcPerformanceMax,
            ProcessorPowerSlider::DcCoreParkingMin,
            ProcessorPowerSlider::DcPerformanceMin,
            ProcessorPowerSlider::DcPerformanceMax,
        ] {
            let input = processor_power_slider_input(&self.inputs, slider);
            self._processor_power_slider_subscriptions
                .push(
                    cx.subscribe_in(&input, window, move |app, _, event, _, cx| {
                        app.handle_processor_power_slider_event(slider, event, cx);
                    }),
                );
        }
    }

    fn handle_processor_power_slider_event(
        &mut self,
        slider: ProcessorPowerSlider,
        event: &SliderEvent,
        cx: &mut Context<Self>,
    ) {
        let SliderEvent::Change(value) = event;
        self.set_processor_power_slider_value(slider, value.end().round() as u64);
        cx.notify();
    }

    fn cpu_threshold_slider_input_count(&self) -> usize {
        self.inputs.cpu_rule_thresholds.len() + self.inputs.cpu_rule_upper_thresholds.len()
    }

    fn ensure_cpu_threshold_slider_subscriptions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self._cpu_threshold_slider_subscriptions.len() != self.cpu_threshold_slider_input_count()
        {
            self.rebuild_cpu_threshold_slider_subscriptions(window, cx);
        }
    }

    fn rebuild_cpu_threshold_slider_subscriptions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut inputs = Vec::new();
        inputs.extend(
            self.inputs
                .cpu_rule_thresholds
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, input)| (input, CpuThresholdSlider::Lower(index))),
        );
        inputs.extend(
            self.inputs
                .cpu_rule_upper_thresholds
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, input)| (input, CpuThresholdSlider::Upper(index))),
        );

        self._cpu_threshold_slider_subscriptions.clear();
        for (input, slider) in inputs {
            self._cpu_threshold_slider_subscriptions
                .push(cx.subscribe_in(
                    &input,
                    window,
                    move |app, _, event: &SliderEvent, _, cx| {
                        app.handle_cpu_threshold_slider_event(slider, event, cx);
                    },
                ));
        }
    }

    fn handle_cpu_threshold_slider_event(
        &mut self,
        slider: CpuThresholdSlider,
        event: &SliderEvent,
        cx: &mut Context<Self>,
    ) {
        let SliderEvent::Change(value) = event;
        let value = value.end().round().clamp(0.0, 100.0) as u8;
        self.set_cpu_threshold_slider_value(slider, value);
        cx.notify();
    }

    fn subscribe_to_activity_sliders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._activity_slider_subscriptions.clear();
        for (slider, input) in [
            (
                ActivitySlider::IdleTimeout,
                self.inputs.activity_idle_timeout.clone(),
            ),
            (
                ActivitySlider::CheckInterval,
                self.inputs.activity_check_interval.clone(),
            ),
        ] {
            self._activity_slider_subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |app, _, event: &SliderEvent, _, cx| {
                    app.handle_activity_slider_event(slider, event, cx);
                },
            ));
        }
    }

    fn handle_activity_slider_event(
        &mut self,
        slider: ActivitySlider,
        event: &SliderEvent,
        cx: &mut Context<Self>,
    ) {
        let SliderEvent::Change(value) = event;
        self.set_activity_slider_value(slider, value.end().round() as u64);
        cx.notify();
    }

    fn handle_numeric_input_event(&mut self, event: &InputEvent, cx: &mut Context<Self>) {
        if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            self.finish_numeric_edit(cx);
        }
    }

    fn rule_title_input(&self, target: RuleTitleTarget) -> Option<Entity<InputState>> {
        match target {
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

    fn begin_numeric_edit(
        &mut self,
        field: NumericField,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editing_numeric = Some(field);
        clear_input_to(&self.inputs.numeric_value, &value, window, cx);
        self.inputs
            .numeric_value
            .read(cx)
            .focus_handle(cx)
            .focus(window);
        cx.notify();
    }

    fn finish_numeric_edit(&mut self, cx: &mut Context<Self>) {
        let Some(field) = self.editing_numeric.take() else {
            return;
        };
        let value = self.inputs.numeric_value.read(cx).value().to_string();
        self.apply_numeric_input(field, &value);
        cx.notify();
    }

    fn apply_numeric_input(&mut self, field: NumericField, value: &str) {
        let value = value.trim().replace(',', "");
        match field {
            NumericField::ActivityIdleTimeout => {
                if let Some(value) = parse_u64_input(
                    &value,
                    ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS,
                    ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS,
                ) {
                    self.set_activity_slider_value(ActivitySlider::IdleTimeout, value);
                }
            }
            NumericField::GeneralCheckInterval => {
                if let Some(value) = parse_u64_input(
                    &value,
                    ACTIVITY_CHECK_INTERVAL_MIN_MS,
                    ACTIVITY_CHECK_INTERVAL_MAX_MS,
                ) {
                    self.set_activity_slider_value(ActivitySlider::CheckInterval, value);
                }
            }
            NumericField::ExecutionFailureSuppressionThreshold => {
                if let Some(value) = parse_u64_input(
                    &value,
                    u64::from(MIN_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD),
                    u64::from(MAX_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD),
                ) {
                    self.settings
                        .advanced
                        .execution_failure_suppression_threshold = value as u8;
                }
            }
            NumericField::EcoQosRestrictionPercent => {
                if let Some(value) = parse_u64_input(&value, 1, 100) {
                    self.settings.eco_qos.cpu_restriction_percent = value as u8;
                }
            }
            NumericField::BackgroundCpuRestrictionPercent => {
                if let Some(value) = parse_u64_input(&value, 1, 100) {
                    self.settings.background_cpu_restriction.percent = value as u8;
                }
            }
            NumericField::SmartTrimCheckIntervalMinutes => {
                if let Some(value) = parse_u64_input(&value, 1, 1440) {
                    self.settings.smart_trim.check_interval_minutes = value;
                }
            }
            NumericField::SmartTrimMemoryLoadThreshold => {
                if let Some(value) = parse_u64_input(&value, 1, 100) {
                    self.settings
                        .smart_trim
                        .system_memory_load_threshold_percent = value as u8;
                }
            }
            NumericField::SmartTrimWorkingSetThreshold => {
                if let Some(value) = parse_u64_input(&value, 1, 1_048_576) {
                    self.settings.smart_trim.process_working_set_threshold_mb = value;
                }
            }
            NumericField::SmartTrimCpuIdleThreshold => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.settings.smart_trim.process_cpu_idle_threshold_percent = value as u8;
                }
            }
            NumericField::SmartTrimIdleSeconds => {
                if let Some(value) = parse_u64_input(&value, 1, 86_400) {
                    self.settings.smart_trim.process_idle_seconds = value;
                }
            }
            NumericField::SmartTrimCooldownSeconds => {
                if let Some(value) = parse_u64_input(&value, 1, 86_400) {
                    self.settings.smart_trim.trim_cooldown_seconds = value;
                }
            }
            NumericField::SmartTrimPurgeFreeRamThreshold => {
                if let Some(value) = parse_u64_input(&value, 0, 1_048_576) {
                    self.settings.smart_trim.purge_free_ram_threshold_mb = value;
                }
            }
            NumericField::SuspensionBackgroundDelay => {
                if let Some(value) = parse_u64_input(&value, 1, 86_400) {
                    self.settings.app_suspension.background_delay_seconds = value;
                }
            }
            NumericField::SuspensionThawInterval => {
                if let Some(value) = parse_u64_input(&value, 1, 86_400) {
                    self.settings.app_suspension.temporary_thaw_interval_seconds = value;
                }
            }
            NumericField::SuspensionThawDuration => {
                if let Some(value) = parse_u64_input(&value, 1, 3_600) {
                    self.settings.app_suspension.temporary_thaw_duration_seconds = value;
                }
            }
            NumericField::SuspensionAudioRefreeze => {
                if let Some(value) = parse_u64_input(&value, 1, 3_600) {
                    self.settings.app_suspension.audio_wake_duration_seconds = value;
                }
            }
            NumericField::SuspensionNetworkRefreeze => {
                if let Some(value) = parse_u64_input(&value, 1, 3_600) {
                    self.settings.app_suspension.network_wake_duration_seconds = value;
                }
            }
            NumericField::AutoBalanceThreshold => {
                if let Some(value) = parse_u64_input(
                    &value,
                    AUTO_BALANCE_THRESHOLD_MIN_PERCENT,
                    AUTO_BALANCE_THRESHOLD_MAX_PERCENT,
                ) {
                    self.settings
                        .foreground_responsiveness
                        .auto_balance_threshold_percent = value as u8;
                }
            }
            NumericField::AutoBalanceRestoreThreshold => {
                if let Some(value) = parse_u64_input(
                    &value,
                    AUTO_BALANCE_THRESHOLD_MIN_PERCENT,
                    AUTO_BALANCE_THRESHOLD_MAX_PERCENT,
                ) {
                    self.settings
                        .foreground_responsiveness
                        .auto_balance_restore_threshold_percent = value as u8;
                }
            }
            NumericField::AutoBalanceTotalThreshold => {
                if let Some(value) = parse_u64_input(
                    &value,
                    AUTO_BALANCE_THRESHOLD_MIN_PERCENT,
                    AUTO_BALANCE_THRESHOLD_MAX_PERCENT,
                ) {
                    self.settings
                        .foreground_responsiveness
                        .auto_balance_total_threshold_percent = value as u8;
                }
            }
            NumericField::AutoBalanceCpuPercent => {
                if let Some(value) = parse_u64_input(
                    &value,
                    AUTO_BALANCE_THRESHOLD_MIN_PERCENT,
                    AUTO_BALANCE_THRESHOLD_MAX_PERCENT,
                ) {
                    self.settings
                        .foreground_responsiveness
                        .auto_balance_cpu_percent = value as u8;
                }
            }
            NumericField::AutoBalanceSustain => {
                if let Some(value) =
                    parse_u64_input(&value, AUTO_BALANCE_SECONDS_MIN, AUTO_BALANCE_SECONDS_MAX)
                {
                    self.settings
                        .foreground_responsiveness
                        .auto_balance_sustain_seconds = value;
                }
            }
            NumericField::AutoBalanceMinimumRestraint => {
                if let Some(value) =
                    parse_u64_input(&value, AUTO_BALANCE_SECONDS_MIN, AUTO_BALANCE_SECONDS_MAX)
                {
                    self.settings
                        .foreground_responsiveness
                        .auto_balance_minimum_restraint_seconds = value;
                }
            }
            NumericField::AutoBalanceCooldown => {
                if let Some(value) =
                    parse_u64_input(&value, AUTO_BALANCE_SECONDS_MIN, AUTO_BALANCE_SECONDS_MAX)
                {
                    self.settings
                        .foreground_responsiveness
                        .auto_balance_cooldown_seconds = value;
                }
            }
            NumericField::ProcessorAcCoreParkingMin => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.set_processor_power_slider_value(
                        ProcessorPowerSlider::AcCoreParkingMin,
                        value,
                    );
                }
            }
            NumericField::ProcessorAcPerformanceMin => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.set_processor_power_slider_value(
                        ProcessorPowerSlider::AcPerformanceMin,
                        value,
                    );
                }
            }
            NumericField::ProcessorAcPerformanceMax => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.set_processor_power_slider_value(
                        ProcessorPowerSlider::AcPerformanceMax,
                        value,
                    );
                }
            }
            NumericField::ProcessorDcCoreParkingMin => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.set_processor_power_slider_value(
                        ProcessorPowerSlider::DcCoreParkingMin,
                        value,
                    );
                }
            }
            NumericField::ProcessorDcPerformanceMin => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.set_processor_power_slider_value(
                        ProcessorPowerSlider::DcPerformanceMin,
                        value,
                    );
                }
            }
            NumericField::ProcessorDcPerformanceMax => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.set_processor_power_slider_value(
                        ProcessorPowerSlider::DcPerformanceMax,
                        value,
                    );
                }
            }
            NumericField::CpuThreshold(index) => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.set_cpu_threshold_slider_value(
                        CpuThresholdSlider::Lower(index),
                        value as u8,
                    );
                }
            }
            NumericField::CpuUpperThreshold(index) => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.set_cpu_threshold_slider_value(
                        CpuThresholdSlider::Upper(index),
                        value as u8,
                    );
                }
            }
            NumericField::CpuDuration(index) => {
                if let (Some(rule), Some(value)) = (
                    self.settings.cpu_usage_mode.rules.get_mut(index),
                    parse_u64_input(&value, 0, 86_400),
                ) {
                    rule.duration_seconds = value;
                }
            }
            NumericField::CpuLimiterThreshold(index) => {
                if let (Some(rule), Some(value)) = (
                    self.settings.cpu_limiter.rules.get_mut(index),
                    parse_u64_input(&value, 1, 100),
                ) {
                    rule.threshold_percent = value as u8;
                }
            }
            NumericField::CpuLimiterSustain(index) => {
                if let (Some(rule), Some(value)) = (
                    self.settings.cpu_limiter.rules.get_mut(index),
                    parse_u64_input(&value, 1, 86_400),
                ) {
                    rule.sustain_seconds = value;
                }
            }
            NumericField::CpuLimiterCooldown(index) => {
                if let (Some(rule), Some(value)) = (
                    self.settings.cpu_limiter.rules.get_mut(index),
                    parse_u64_input(&value, 1, 86_400),
                ) {
                    rule.cooldown_seconds = value;
                }
            }
            NumericField::CpuLimiterMaxProcessors(index) => {
                if let (Some(rule), Some(value)) = (
                    self.settings.cpu_limiter.rules.get_mut(index),
                    parse_u64_input(&value, 1, max_logical_processor_count() as u64),
                ) {
                    rule.max_logical_processors = value as u8;
                }
            }
            NumericField::WatchdogRestartDelay(index) => {
                if let (Some(rule), Some(value)) = (
                    self.settings.watchdog.rules.get_mut(index),
                    parse_u64_input(&value, 0, 86_400),
                ) {
                    rule.restart_delay_seconds = value;
                }
            }
            NumericField::NetworkThreshold(field) => {
                let Ok(value) = value.parse::<f64>() else {
                    return;
                };
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
                *bytes = unit
                    .threshold_bytes_from_value(value.max(0.0))
                    .min(MAX_NETWORK_THRESHOLD_BYTES);
            }
        }
    }

    fn finish_rule_title_edit(&mut self, target: RuleTitleTarget, cx: &mut Context<Self>) {
        self.sync_input_values(cx);
        if self.editing_rule_title == Some(target) {
            self.editing_rule_title = None;
        }
        cx.notify();
    }

    fn is_rule_card_collapsed(&self, target: &RuleCardTarget) -> bool {
        !self.expanded_rule_cards.contains(target)
    }

    fn toggle_rule_card(&mut self, target: RuleCardTarget, cx: &mut Context<Self>) {
        if !self.expanded_rule_cards.remove(&target) {
            self.expanded_rule_cards.insert(target);
        }
        cx.notify();
    }

    fn is_setting_group_collapsed(&self, target: SettingGroupTarget) -> bool {
        !self.expanded_setting_groups.contains(&target)
    }

    fn toggle_setting_group(&mut self, target: SettingGroupTarget, cx: &mut Context<Self>) {
        if !self.expanded_setting_groups.remove(&target) {
            self.expanded_setting_groups.insert(target);
        }
        cx.notify();
    }

    fn dropdown_placement(
        &self,
        id: &str,
        full_list_height: Pixels,
        window: &Window,
    ) -> DropdownPlacement {
        let margin = px(DROPDOWN_VIEWPORT_MARGIN);
        let offset = px(DROPDOWN_MENU_OFFSET);
        let fallback_max_height =
            (window.viewport_size().height - offset - margin).max(Pixels::ZERO);
        let Some(bounds) = self.dropdown_anchor_bounds.borrow().get(id).copied() else {
            return DropdownPlacement {
                open_up: false,
                max_height: fallback_max_height,
            };
        };

        let below =
            (window.viewport_size().height - bounds.top() - offset - margin).max(Pixels::ZERO);
        let above = (bounds.bottom() - offset - margin).max(Pixels::ZERO);
        let open_up = full_list_height > below && above > below;
        let available_height = if open_up { above } else { below };

        DropdownPlacement {
            open_up,
            max_height: available_height,
        }
    }

    fn render_dropdown_select(
        &self,
        id: impl Into<String>,
        selected_label: impl Into<SharedString>,
        enabled: bool,
        width: DropdownSelectWidth,
        option_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
        build_options: impl FnOnce(Pixels, &mut Context<Self>) -> Scrollable<gpui::Div>,
    ) -> AnyElement {
        let id = id.into();
        let is_open = enabled && self.active_power_plan_picker.as_deref() == Some(id.as_str());
        let placement = self.dropdown_placement(&id, dropdown_list_height(option_count), window);
        let options = build_options(placement.max_height, cx);
        let control_id = SharedString::from(format!("{id}-control"));
        let toggle_id = id.clone();

        dropdown_select_container(width)
            .child(
                dropdown_select_control(control_id, selected_label, enabled, is_open, cx).when(
                    enabled,
                    |control| {
                        control.on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                            app.active_power_plan_picker =
                                (app.active_power_plan_picker.as_deref()
                                    != Some(toggle_id.as_str()))
                                .then_some(toggle_id.clone());
                            cx.notify();
                        }))
                    },
                ),
            )
            .child(dropdown_anchor_sensor(
                id.clone(),
                Rc::clone(&self.dropdown_anchor_bounds),
            ))
            .child(dropdown_popup_or_empty(is_open, placement, options, cx))
            .into_any_element()
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
        for (index, rule) in self.settings.watchdog.rules.iter_mut().enumerate() {
            if let Some(input) = self.inputs.watchdog_launch_paths.get(index) {
                let launch_path = input.read(cx).value().to_string();
                rule.launch_path = sanitize_watchdog_launch_path(&launch_path);
            }
            if let Some(input) = self.inputs.watchdog_launch_args.get(index) {
                rule.launch_args = split_watchdog_args(&input.read(cx).value());
            }
        }
    }

    fn background_settings(&self) -> Settings {
        self.runtime_settings()
    }

    fn runtime_settings(&self) -> Settings {
        let mut settings = self.settings.clone();
        settings.general.enabled = self.saved_settings.general.enabled;
        settings.power_plans = self.saved_settings.power_plans.clone();
        settings.activity_mode = self.saved_settings.activity_mode.clone();
        settings.schedule_mode = self.saved_settings.schedule_mode.clone();
        settings.cpu_usage_mode = self.saved_settings.cpu_usage_mode.clone();
        settings.foreground_rules = self.saved_settings.foreground_rules.clone();
        settings.performance_mode = self.saved_settings.performance_mode.clone();
        settings.eco_qos = self.saved_settings.eco_qos.clone();
        settings.app_suspension = self.saved_settings.app_suspension.clone();
        settings.cpu_affinity = self.saved_settings.cpu_affinity.clone();
        settings.cpu_limiter = self.saved_settings.cpu_limiter.clone();
        settings.watchdog = self.saved_settings.watchdog.clone();
        settings.foreground_responsiveness = self.saved_settings.foreground_responsiveness.clone();
        settings.smart_trim = self.saved_settings.smart_trim.clone();
        settings
    }

    fn section_nav_status(&self, pages: &[Page]) -> Option<NavStatus> {
        let mut has_failed = false;
        let mut has_unsupported = false;
        let mut has_needs_rules = false;
        let mut has_enabled = false;
        let mut has_disabled = false;

        for page in pages {
            match self.nav_status(*page) {
                Some(NavStatus::Failed) => has_failed = true,
                Some(NavStatus::Unsupported) => has_unsupported = true,
                Some(NavStatus::NeedsRules) => has_needs_rules = true,
                Some(NavStatus::Enabled) => has_enabled = true,
                Some(NavStatus::Disabled) => has_disabled = true,
                None => {}
            }
        }

        if has_failed {
            Some(NavStatus::Failed)
        } else if has_unsupported {
            Some(NavStatus::Unsupported)
        } else if has_needs_rules {
            Some(NavStatus::NeedsRules)
        } else if has_enabled {
            Some(NavStatus::Enabled)
        } else if has_disabled {
            Some(NavStatus::Disabled)
        } else {
            None
        }
    }

    fn nav_status(&self, page: Page) -> Option<NavStatus> {
        let settings = &self.saved_settings;

        match page {
            Page::Dashboard => None,
            Page::PowerPlanAutomation
            | Page::ProcessorControls
            | Page::ProcessPolicies
            | Page::MemoryControl
            | Page::AppHome
            | Page::AdvancedHome => page
                .child_pages()
                .and_then(|pages| self.section_nav_status(pages)),
            Page::Activity => {
                if !settings.activity_mode.enabled
                    || !settings.activity_mode.input_detection.any_enabled()
                {
                    Some(NavStatus::Disabled)
                } else {
                    Some(NavStatus::Enabled)
                }
            }
            Page::CpuUsage => Some(rule_based_nav_status(
                settings.cpu_usage_mode.enabled,
                settings.cpu_usage_mode.rules.len(),
            )),
            Page::CoreParking => None,
            Page::CpuLimiter => Some(process_nav_status(
                settings.cpu_limiter.enabled,
                self.cpu_limiter_status.failed_processes,
                self.cpu_limiter_status.last_error.is_some(),
            )),
            Page::BackgroundCpuRestriction => Some(process_nav_status(
                settings.background_cpu_restriction.enabled,
                self.background_cpu_restriction_status.failed_processes,
                self.background_cpu_restriction_status.last_error.is_some(),
            )),
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
            Page::PerformanceMode => Some(process_rule_nav_status(
                settings.performance_mode.enabled,
                settings.performance_mode.rules.len(),
                0,
                self.performance_mode_status.last_error.is_some(),
            )),
            Page::Watchdog => Some(process_nav_status(
                settings.watchdog.enabled,
                self.watchdog_status.failed_actions,
                self.watchdog_status.last_error.is_some(),
            )),
            Page::CpuAffinity => Some(process_nav_status(
                settings.cpu_affinity.enabled,
                self.cpu_affinity_status.failed_processes,
                self.cpu_affinity_status.last_error.is_some(),
            )),
            Page::ForegroundResponsiveness => Some(process_nav_status(
                settings.foreground_responsiveness.enabled,
                self.foreground_responsiveness_status.failed_processes,
                self.foreground_responsiveness_status.last_error.is_some(),
            )),
            Page::IoPriority => Some(process_nav_status(
                settings.io_priority.enabled,
                self.io_priority_status.failed_processes,
                self.io_priority_status.last_error.is_some(),
            )),
            Page::MemoryPriority => Some(process_nav_status(
                settings.memory_priority.enabled,
                self.memory_priority_status.failed_processes,
                self.memory_priority_status.last_error.is_some(),
            )),
            Page::SmartTrim => Some(process_nav_status(
                settings.smart_trim.enabled,
                self.smart_trim_status.failed_processes,
                self.smart_trim_status.last_error.is_some(),
            )),
            Page::ForegroundRules => Some(rule_based_nav_status(
                settings.foreground_rules.enabled,
                settings.foreground_rules.rules.len(),
            )),
            Page::Schedule => Some(rule_based_nav_status(
                settings.schedule_mode.enabled,
                settings.schedule_mode.rules.len(),
            )),
            Page::ActionLog => None,
            Page::Settings
            | Page::SettingsAppearance
            | Page::Win32PrioritySeparation
            | Page::About => None,
        }
    }
}

impl Render for PowerLeafApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.inputs.ensure_for_settings(window, cx, &self.settings);
        self.ensure_rule_title_input_subscriptions(window, cx);
        self.ensure_cpu_threshold_slider_subscriptions(window, cx);
        self.sync_input_values(cx);

        let search_query = self.dashboard_search_query(cx);
        let search_active = !search_query.is_empty();
        let page = if search_active {
            self.render_search_results_page(&search_query, cx)
        } else {
            self.render_page(window, cx)
        };
        let unsaved = self.settings != self.saved_settings;

        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .font_family("Segoe UI Variable")
            .on_action(cx.listener(|app, _: &InputEscape, window, cx| {
                clear_input(&app.inputs.dashboard_search, window, cx);
                window.blur();
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                    app.navigate_back(cx);
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                    app.navigate_forward(cx);
                    cx.stop_propagation();
                }),
            )
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
                                    .child(page_content_frame(page)),
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
                            .text_size(px(TEXT_CONTROL_SIZE))
                            .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(cx.theme().foreground)
                            .child(t!("app.name").to_string()),
                    )
                    .child(
                        div()
                            .text_size(px(TEXT_LABEL_SIZE))
                            .line_height(px(TEXT_LABEL_LINE_HEIGHT))
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("app.description").to_string()),
                    ),
            )
            .child(self.render_title_bar_search(window, cx))
            .child(
                h_flex()
                    .h_full()
                    .flex_1()
                    .min_w(px(138.0))
                    .items_center()
                    .justify_end()
                    .child(title_bar_controls(window, cx)),
            )
            .into_any_element()
    }

    fn render_title_bar_search(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let search_focused = self
            .inputs
            .dashboard_search
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);

        div()
            .id("titlebar-search")
            .occlude()
            .flex_1()
            .min_w(px(160.0))
            .max_w(px(420.0))
            .on_mouse_down_out(cx.listener(|_, _: &gpui::MouseDownEvent, window, cx| {
                window.blur();
                cx.notify();
            }))
            .child(app_input(&self.inputs.dashboard_search, search_focused, cx))
            .into_any_element()
    }

    fn render_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut nav = v_flex()
            .w(px(NAV_PANE_WIDTH))
            .min_w(px(NAV_PANE_WIDTH))
            .h_full()
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .bg(cx.theme().sidebar);

        let mut drawer = v_flex()
            .flex_1()
            .min_h(px(0.0))
            .gap_3()
            .p_2()
            .overflow_y_scrollbar();
        let mut footer = v_flex()
            .flex_shrink_0()
            .gap_1()
            .p_2()
            .border_t_1()
            .border_color(cx.theme().sidebar_border);

        for section in Page::sections() {
            let page = section.landing_page;
            let selected = self.page.section_landing_page() == page;
            let target = page;
            let status = self.section_nav_status(section.pages);
            let row = nav_row(page, selected, status, cx)
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.navigate_to(target, cx);
                }))
                .into_any_element();

            if nav_section_in_footer(section.landing_page) {
                footer = footer.child(row);
            } else {
                drawer = drawer.child(row);
            }
        }

        nav = nav.child(drawer).child(footer);
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
            .text_size(px(TEXT_BODY_SIZE))
            .line_height(px(TEXT_BODY_LINE_HEIGHT))
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
            .rounded(px(FLUENT_RADIUS_OVERLAY))
            .border_1()
            .border_color(rgb(accent_color()))
            .bg(cx.theme().popover)
            .child(
                h_flex().items_center().child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
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
            Page::Dashboard => self.render_dashboard(cx),
            Page::PowerPlanAutomation => {
                self.render_section_landing_page(Page::PowerPlanAutomation, cx)
            }
            Page::ProcessorControls => {
                self.render_section_landing_page(Page::ProcessorControls, cx)
            }
            Page::ProcessPolicies => self.render_section_landing_page(Page::ProcessPolicies, cx),
            Page::MemoryControl => self.render_section_landing_page(Page::MemoryControl, cx),
            Page::AppHome => self.render_section_landing_page(Page::AppHome, cx),
            Page::AdvancedHome => self.render_section_landing_page(Page::AdvancedHome, cx),
            Page::Activity => self.render_activity_page(window, cx),
            Page::ForegroundRules => self.render_foreground_rules_page(window, cx),
            Page::Schedule => self.render_schedule_page(window, cx),
            Page::CpuUsage => self.render_cpu_usage_page(window, cx),
            Page::CoreParking => self.render_core_parking_page(window, cx),
            Page::CpuLimiter => self.render_cpu_limiter_page(window, cx),
            Page::BackgroundCpuRestriction => {
                self.render_background_cpu_restriction_page(window, cx)
            }
            Page::EfficiencyMode => self.render_efficiency_page(window, cx),
            Page::AppSuspension => self.render_suspension_page(window, cx),
            Page::Watchdog => self.render_watchdog_page(window, cx),
            Page::PerformanceMode => self.render_performance_mode_page(window, cx),
            Page::ForegroundResponsiveness => {
                self.render_foreground_responsiveness_page(window, cx)
            }
            Page::IoPriority => self.render_io_priority_page(window, cx),
            Page::MemoryPriority => self.render_memory_priority_page(window, cx),
            Page::SmartTrim => self.render_smart_trim_page(window, cx),
            Page::CpuAffinity => self.render_affinity_page(window, cx),
            Page::ActionLog => self.render_action_log_page(window, cx),
            Page::Settings => self.render_powerleaf_behaviour_page(window, cx),
            Page::SettingsAppearance => self.render_settings_appearance_page(window, cx),
            Page::Win32PrioritySeparation => self.render_win32_priority_separation_page(window, cx),
            Page::About => self.render_about_page(cx),
        }
    }

    fn render_section_landing_page(
        &self,
        section_page: Page,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut cards = v_flex().w_full().min_w(px(0.0)).gap_2();

        if let Some(pages) = section_page.child_pages() {
            for page in pages {
                let target = *page;
                let status = self.nav_status(target);
                cards = cards.child(
                    section_landing_card(target, status, cx)
                        .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                            app.navigate_to(target, cx);
                        }))
                        .into_any_element(),
                );
            }
        }

        page_shell(section_page, cx).child(cards).into_any_element()
    }

    fn render_dashboard_page_card(&self, target: Page, cx: &mut Context<Self>) -> gpui::Div {
        let status = target
            .child_pages()
            .and_then(|pages| self.section_nav_status(pages))
            .or_else(|| self.nav_status(target));

        dashboard_card_slot(
            section_landing_card(target, status, cx)
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.navigate_to(target, cx);
                }))
                .into_any_element(),
        )
    }

    fn render_search_result_page_card(&self, target: Page, cx: &mut Context<Self>) -> gpui::Div {
        let status = target
            .child_pages()
            .and_then(|pages| self.section_nav_status(pages))
            .or_else(|| self.nav_status(target));

        dashboard_card_slot(
            section_landing_card(target, status, cx)
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, window, cx| {
                    clear_input(&app.inputs.dashboard_search, window, cx);
                    window.blur();
                    app.navigate_to(target, cx);
                    cx.notify();
                }))
                .into_any_element(),
        )
    }

    fn dashboard_search_query(&self, cx: &mut Context<Self>) -> String {
        self.inputs
            .dashboard_search
            .read(cx)
            .value()
            .trim()
            .to_string()
    }

    fn render_search_results_page(&self, search_query: &str, cx: &mut Context<Self>) -> AnyElement {
        let search_results = dashboard_search_pages(search_query);
        let mut search_result_cards = h_flex()
            .w_full()
            .min_w(px(0.0))
            .items_start()
            .gap_2()
            .flex_wrap();

        if search_query.is_empty() {
            search_result_cards = search_result_cards.child(div().w_full().min_h(px(8.0)));
        } else if search_results.is_empty() {
            search_result_cards =
                search_result_cards.child(div().w_full().min_w(px(0.0)).py_2().child(text_muted(
                    t!("dashboard.no_matching_functions").to_string(),
                )));
        } else {
            for target in search_results {
                search_result_cards =
                    search_result_cards.child(self.render_search_result_page_card(target, cx));
            }
        }

        search_results_page_shell(cx)
            .child(search_result_cards)
            .into_any_element()
    }

    fn render_dashboard(&self, cx: &mut Context<Self>) -> AnyElement {
        let settings = &self.saved_settings;
        let mut section_cards = h_flex()
            .w_full()
            .min_w(px(0.0))
            .items_start()
            .gap_2()
            .flex_wrap();

        for section in dashboard_sections_in_nav_order() {
            section_cards =
                section_cards.child(self.render_dashboard_page_card(section.landing_page, cx));
        }

        let summary = h_flex()
            .w_full()
            .min_w(px(0.0))
            .items_start()
            .gap_2()
            .flex_wrap()
            .child(dashboard_card_slot(
                self.render_cpu_usage_summary().into_any_element(),
            ))
            .child(dashboard_card_slot(
                self.render_memory_usage_summary().into_any_element(),
            ))
            .child(dashboard_card_slot(
                self.render_io_usage_summary().into_any_element(),
            ))
            .child(dashboard_card_slot(
                titled_status_list(
                    &t!("dashboard.enabled_rules"),
                    self.dashboard_enabled_function_items(settings),
                )
                .into_any_element(),
            ));

        page_shell(Page::Dashboard, cx)
            .child(section_title_text(t!("dashboard.overview").to_string()))
            .child(summary)
            .child(section_title_text(
                t!("dashboard.main_sections").to_string(),
            ))
            .child(section_cards)
            .into_any_element()
    }

    fn dashboard_enabled_function_items(&self, settings: &Settings) -> Vec<(String, String)> {
        let mut items = Vec::new();

        if settings.general.enabled {
            items.push((
                t!("dashboard.automation").to_string(),
                t!("common.enabled").to_string(),
            ));
        }
        if settings.foreground_rules.enabled {
            items.push((
                t!("nav.foreground_rules").to_string(),
                rule_count_label(settings.foreground_rules.rules.len()),
            ));
        }
        if settings.performance_mode.enabled {
            items.push((
                t!("nav.performance_mode").to_string(),
                self.performance_mode_status
                    .active_process
                    .clone()
                    .unwrap_or_else(|| rule_count_label(settings.performance_mode.rules.len())),
            ));
        }
        if settings.cpu_usage_mode.enabled {
            items.push((
                t!("nav.cpu_usage").to_string(),
                cpu_usage_label(self.cpu_usage.percent),
            ));
        }
        if settings.activity_mode.enabled {
            items.push((
                t!("nav.activity").to_string(),
                format!("{:?}", self.activity.state),
            ));
        }
        if settings.schedule_mode.enabled {
            items.push((t!("nav.schedule").to_string(), self.next_schedule.clone()));
        }
        if settings.cpu_limiter.enabled {
            items.push((
                t!("nav.cpu_limiter").to_string(),
                format!("{} limited", self.cpu_limiter_status.limited_processes),
            ));
        }
        if settings.background_cpu_restriction.enabled {
            items.push((
                t!("nav.background_cpu_restriction").to_string(),
                format!(
                    "{} adjusted",
                    self.background_cpu_restriction_status.adjusted_processes
                ),
            ));
        }
        if settings.eco_qos.enabled {
            items.push((
                t!("nav.efficiency_mode").to_string(),
                format!("{} throttled", self.eco_qos_status.throttled_processes),
            ));
        }
        if settings.app_suspension.enabled {
            items.push((
                t!("nav.app_suspension").to_string(),
                format!(
                    "{} suspended",
                    self.app_suspension_status.suspended_processes
                ),
            ));
        }
        if settings.watchdog.enabled {
            items.push((
                t!("nav.watchdog").to_string(),
                format!("{} matched", self.watchdog_status.matched_processes),
            ));
        }
        if settings.foreground_responsiveness.enabled {
            items.push((
                t!("nav.foreground_responsiveness").to_string(),
                format!(
                    "{} adjusted",
                    self.foreground_responsiveness_status
                        .background_adjusted_processes
                        + self
                            .foreground_responsiveness_status
                            .auto_balanced_processes
                ),
            ));
        }
        if settings.io_priority.enabled {
            items.push((
                t!("nav.io_priority").to_string(),
                format!("{} adjusted", self.io_priority_status.adjusted_processes),
            ));
        }
        if settings.memory_priority.enabled {
            items.push((
                t!("nav.memory_priority").to_string(),
                format!(
                    "{} adjusted",
                    self.memory_priority_status.adjusted_processes
                ),
            ));
        }
        if settings.smart_trim.enabled {
            items.push((
                t!("nav.smart_trim").to_string(),
                format!("{} trimmed", self.smart_trim_status.trimmed_processes),
            ));
        }
        if settings.cpu_affinity.enabled {
            items.push((
                t!("nav.cpu_affinity").to_string(),
                format!("{} adjusted", self.cpu_affinity_status.adjusted_processes),
            ));
        }

        if items.is_empty() {
            items.push((t!("common.none").to_string(), String::new()));
        }

        items
    }

    fn render_cpu_usage_summary(&self) -> gpui::Div {
        self.render_metric_summary(
            t!("dashboard.cpu_usage").to_string(),
            cpu_usage_label(self.cpu_usage.percent),
            &self.cpu_usage_history,
            100.0,
        )
    }

    fn render_memory_usage_summary(&self) -> gpui::Div {
        self.render_metric_summary(
            t!("dashboard.memory_usage").to_string(),
            memory_usage_label(self.memory_usage.percent),
            &self.memory_usage_history,
            100.0,
        )
    }

    fn render_io_usage_summary(&self) -> gpui::Div {
        let max_value = self
            .io_usage_history
            .iter()
            .copied()
            .fold(0.0_f32, f32::max)
            .max(1.0);

        self.render_metric_summary(
            t!("dashboard.io_usage").to_string(),
            io_usage_label(self.io_usage.bytes_per_second),
            &self.io_usage_history,
            max_value,
        )
    }

    fn render_metric_summary(
        &self,
        title: String,
        label: String,
        history: &VecDeque<f32>,
        max_value: f32,
    ) -> gpui::Div {
        let graph = self.render_metric_history_graph(history, max_value);

        dashboard_summary_card(
            title,
            v_flex()
                .w_full()
                .min_w(px(0.0))
                .flex_1()
                .min_h(px(0.0))
                .gap_2()
                .child(
                    div()
                        .text_size(px(TEXT_BODY_SIZE))
                        .line_height(px(TEXT_BODY_LINE_HEIGHT))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(label),
                )
                .child(graph)
                .into_any_element(),
        )
    }

    fn render_metric_history_graph(&self, history: &VecDeque<f32>, max_value: f32) -> gpui::Div {
        let mut graph = h_flex()
            .w_full()
            .h(px(DASHBOARD_CPU_GRAPH_HEIGHT))
            .items_center()
            .gap_1()
            .px_2()
            .py_2();

        let missing_samples = CPU_USAGE_HISTORY_LEN.saturating_sub(history.len());
        for _ in 0..missing_samples {
            graph = graph.child(
                v_flex().h_full().flex_1().justify_end().child(
                    div()
                        .w_full()
                        .h(px(8.0))
                        .rounded_sm()
                        .bg(rgb(border_color()))
                        .opacity(0.35),
                ),
            );
        }

        let max_value = max_value.max(1.0);
        for value in history {
            let bar_height = 8.0 + (value.clamp(0.0, max_value) / max_value) * 88.0;
            graph = graph.child(
                v_flex().h_full().flex_1().justify_end().child(
                    div()
                        .w_full()
                        .h(px(bar_height))
                        .rounded_sm()
                        .bg(rgb(accent_color())),
                ),
            );
        }

        graph
    }

    fn render_activity_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.sync_activity_slider_states(window, cx);
        let enabled = self.settings.activity_mode.enabled;
        let body = feature_body(enabled)
            .child(setting_action_card(
                "activity-idle-plan-card",
                t!("activity.idle_plan").to_string(),
                self.render_inline_power_plan_picker(
                    "activity-idle-plan",
                    self.settings
                        .activity_mode
                        .power_plans
                        .power_save_guid
                        .clone(),
                    PowerPlanField::ActivityKind(PowerPlanKind::Idle),
                    window,
                    cx,
                ),
            ))
            .child(setting_action_card(
                "activity-active-plan-card",
                t!("activity.active_plan").to_string(),
                self.render_inline_power_plan_picker(
                    "activity-active-plan",
                    self.settings
                        .activity_mode
                        .power_plans
                        .performance_guid
                        .clone(),
                    PowerPlanField::ActivityKind(PowerPlanKind::Active),
                    window,
                    cx,
                ),
            ))
            .child(feature_toggle_switch(
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
            .child(feature_toggle_switch(
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
            .child(activity_slider_card(
                "activity-idle-timeout",
                &t!("activity.idle_timeout"),
                self.render_numeric_value(
                    NumericField::ActivityIdleTimeout,
                    seconds_label(self.settings.activity_mode.idle_timeout_seconds),
                    self.settings.activity_mode.idle_timeout_seconds.to_string(),
                    cx,
                ),
                &self.inputs.activity_idle_timeout,
                enabled,
                ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS,
                ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS,
                1,
                window,
                cx,
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    let value = apply_u64_step(
                        app.settings.activity_mode.idle_timeout_seconds,
                        change,
                        ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS,
                        ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS,
                    );
                    app.set_activity_slider_value(ActivitySlider::IdleTimeout, value);
                    cx.notify();
                }),
            ))
            .child(activity_slider_card(
                "general-check-interval",
                &t!("activity.check_interval"),
                self.render_numeric_value(
                    NumericField::GeneralCheckInterval,
                    milliseconds_label(self.settings.general.check_interval_ms),
                    self.settings.general.check_interval_ms.to_string(),
                    cx,
                ),
                &self.inputs.activity_check_interval,
                enabled,
                ACTIVITY_CHECK_INTERVAL_MIN_MS,
                ACTIVITY_CHECK_INTERVAL_MAX_MS,
                ACTIVITY_CHECK_INTERVAL_STEP_MS,
                window,
                cx,
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    let value = apply_u64_step(
                        app.settings.general.check_interval_ms,
                        change,
                        ACTIVITY_CHECK_INTERVAL_MIN_MS,
                        ACTIVITY_CHECK_INTERVAL_MAX_MS,
                    );
                    app.set_activity_slider_value(ActivitySlider::CheckInterval, value);
                    cx.notify();
                }),
            ));

        let help = tooltip_lines(vec![
            t!("activity.intro_1").to_string(),
            t!("activity.intro_2").to_string(),
            t!("common.power_plan_priority").to_string(),
            t!("common.power_plan_pause_priority").to_string(),
        ]);

        page_shell(Page::Activity, cx)
            .child(feature_toggle_switch_with_help(
                "activity-enabled",
                t!("activity.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.activity_mode.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(body, enabled))
            .into_any_element()
    }

    fn render_foreground_rules_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self.inputs.foreground_process.read(cx).value().to_string();
        let enabled = self.settings.foreground_rules.enabled;
        let help = tooltip_lines(vec![
            t!("foreground.intro_1").to_string(),
            t!("foreground.intro_2").to_string(),
            t!("common.power_plan_priority").to_string(),
            t!("common.power_plan_pause_priority").to_string(),
        ]);
        let mut content =
            page_shell(Page::ForegroundRules, cx).child(feature_toggle_switch_with_help(
                "foreground-enabled",
                t!("foreground.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.foreground_rules.enabled = *checked;
                    cx.notify();
                }),
            ));

        let mut body =
            feature_body(enabled).child(section_title_text(t!("common.rules").to_string()));
        body = body.child(
            h_flex()
                .gap_2()
                .items_start()
                .flex_wrap()
                .child(self.render_process_picker(
                    "foreground-suggestion",
                    &self.inputs.foreground_process,
                    SuggestionTarget::Foreground,
                    window,
                    cx,
                ))
                .child(
                    primary_control_button(Button::new("add-foreground-rule"), cx)
                        .label(t!("common.add").to_string())
                        .disabled(
                            !self.settings.foreground_rules.enabled
                                || !can_add_foreground_process(
                                    &self.settings.foreground_rules,
                                    &input_value,
                                ),
                        )
                        .on_click(cx.listener(|app, _, window, cx| {
                            let process =
                                app.inputs.foreground_process.read(cx).value().to_string();
                            if can_add_foreground_process(&app.settings.foreground_rules, &process)
                            {
                                app.settings
                                    .foreground_rules
                                    .rules
                                    .push(app.new_foreground_rule(&process));
                                app.inputs.ensure_for_settings(window, cx, &app.settings);
                                clear_input(&app.inputs.foreground_process, window, cx);
                            }
                            cx.notify();
                        })),
                ),
        );
        let mut rules = rule_list();
        for (index, rule) in self.settings.foreground_rules.rules.iter().enumerate() {
            rules = rules.child(self.render_foreground_rule(index, rule, window, cx));
        }
        body = body.child(rules);
        content = content.child(disabled_feature_body(body, enabled));

        content.into_any_element()
    }

    fn render_foreground_rule(
        &self,
        index: usize,
        rule: &ForegroundRule,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        compact_rule_row(cx)
            .child(rule_enable_checkbox(
                format!("foreground-rule-enabled-{index}"),
                rule.enabled,
                cx.listener(move |app, checked, _, cx| {
                    if let Some(rule) = app.settings.foreground_rules.rules.get_mut(index) {
                        rule.enabled = *checked;
                    }
                    cx.notify();
                }),
            ))
            .child(self.process_rule_title(&rule.process_name, cx))
            .child(self.render_inline_power_plan_picker(
                format!("foreground-rule-plan-{index}"),
                rule.power_plan_guid.clone(),
                PowerPlanField::ForegroundRule(index),
                window,
                cx,
            ))
            .child(
                danger_control_button(Button::new(SharedString::from(format!(
                    "remove-foreground-rule-{index}"
                ))))
                .label(t!("common.remove").to_string())
                .on_click(cx.listener(move |app, _, _, cx| {
                    if index < app.settings.foreground_rules.rules.len() {
                        app.settings.foreground_rules.rules.remove(index);
                    }
                    app.editing_rule_title = None;
                    app.expanded_rule_cards.clear();
                    cx.notify();
                }))
                .into_any_element(),
            )
            .into_any_element()
    }

    fn new_foreground_rule(&self, process: &str) -> ForegroundRule {
        new_foreground_rule(
            process,
            self.current_plan.as_ref().map(|plan| plan.guid.clone()),
        )
    }

    fn new_performance_mode_rule(&self, process: &str) -> PerformanceModeRule {
        new_performance_mode_rule(
            process,
            self.current_plan.as_ref().map(|plan| plan.guid.clone()),
        )
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
                .child(app_input(input, true, cx))
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
                    .flex_1()
                    .min_w(px(0.0))
                    .max_w(px(420.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(RULE_TITLE_TEXT_SIZE))
                    .line_height(px(RULE_TITLE_LINE_HEIGHT))
                    .cursor_pointer()
                    .child(title.to_owned()),
            )
            .into_any_element()
    }

    fn render_schedule_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let enabled = self.settings.schedule_mode.enabled;
        let help = tooltip_lines(vec![
            t!("schedule.intro_1").to_string(),
            t!("schedule.intro_2").to_string(),
            t!("common.power_plan_priority").to_string(),
            t!("common.power_plan_pause_priority").to_string(),
        ]);
        let mut content = page_shell(Page::Schedule, cx).child(feature_toggle_switch_with_help(
            "schedule-enabled",
            t!("schedule.enable").to_string(),
            help,
            enabled,
            cx.listener(|app, checked, _, cx| {
                app.settings.schedule_mode.enabled = *checked;
                cx.notify();
            }),
        ));

        let mut body =
            feature_body(enabled).child(section_title_text(t!("common.rules").to_string()));
        body = body.child(create_rule_card(
            "create-time-rule-card",
            t!("schedule.rule_title").to_string(),
            primary_control_button(Button::new("add-time-rule"), cx)
                .label(t!("common.create").to_string())
                .disabled(!enabled)
                .on_click(cx.listener(|app, _, window, cx| {
                    app.settings.schedule_mode.rules.push(ScheduleRule {
                        enabled: true,
                        name: t!("schedule.new_rule").to_string(),
                        days: WeekdaySetting::all().to_vec(),
                        start_time: "22:00".to_owned(),
                        end_time: "08:00".to_owned(),
                        power_plan_guid: app.current_plan.as_ref().map(|plan| plan.guid.clone()),
                        power_save_guid: None,
                        performance_guid: None,
                    });
                    app.inputs.ensure_for_settings(window, cx, &app.settings);
                    cx.notify();
                }))
                .into_any_element(),
        ));
        let mut rules = rule_list();
        for (index, rule) in self.settings.schedule_mode.rules.iter().enumerate() {
            rules = rules.child(self.render_schedule_rule(index, rule, window, cx));
        }
        body = body.child(rules);
        content = content.child(disabled_feature_body(body, enabled));

        content.into_any_element()
    }

    fn render_schedule_rule(
        &self,
        index: usize,
        rule: &ScheduleRule,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(name_input) = self.inputs.schedule_rule_names.get(index).cloned() else {
            return syncing_rule_card(index);
        };
        let mut days = h_flex().gap_1().items_center().justify_end().flex_none();
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
            let mut condition_fields = vec![
                rule_action_row(
                    format!("schedule-rule-days-{index}"),
                    t!("schedule.days").to_string(),
                    days.into_any_element(),
                )
                .into_any_element(),
                match self.inputs.schedule_start_times.get(index).cloned() {
                    Some(input) => {
                        let focused = input.read(cx).focus_handle(cx).is_focused(window);
                        setting_input_card(
                            format!("schedule-rule-start-{index}"),
                            t!("schedule.start").to_string(),
                            input,
                            focused,
                            cx,
                        )
                        .into_any_element()
                    }
                    None => syncing_rule_card(index),
                },
                match self.inputs.schedule_end_times.get(index).cloned() {
                    Some(input) => {
                        let focused = input.read(cx).focus_handle(cx).is_focused(window);
                        setting_input_card(
                            format!("schedule-rule-end-{index}"),
                            t!("schedule.end").to_string(),
                            input,
                            focused,
                            cx,
                        )
                        .into_any_element()
                    }
                    None => syncing_rule_card(index),
                },
            ];

            if rule.parsed_times().is_none() {
                condition_fields.push(
                    setting_notice_card(
                        format!("schedule-rule-time-format-{index}"),
                        text_danger(t!("schedule.use_hhmm").to_string()).into_any_element(),
                    )
                    .into_any_element(),
                );
            }

            card = card
                .child(rule_card_body_row(condition_fields))
                .child(rule_card_body_row(vec![rule_action_row(
                    format!("schedule-rule-plan-{index}"),
                    t!("schedule.target_power_plan").to_string(),
                    self.render_inline_power_plan_picker(
                        format!("schedule-rule-plan-{index}"),
                        rule.power_plan_guid.clone(),
                        PowerPlanField::ScheduleRule(index),
                        window,
                        cx,
                    ),
                )
                .into_any_element()]))
                .child(rule_card_body_actions(vec![
                    rename_rule_button(title_target, cx),
                    danger_control_button(Button::new(SharedString::from(format!(
                        "remove-schedule-rule-{index}"
                    ))))
                    .label(t!("common.remove").to_string())
                    .on_click(cx.listener(move |app, _, _, cx| {
                        if index < app.settings.schedule_mode.rules.len() {
                            app.settings.schedule_mode.rules.remove(index);
                        }
                        app.editing_rule_title = None;
                        app.expanded_rule_cards.clear();
                        cx.notify();
                    }))
                    .into_any_element(),
                ]));
        }
        card.into_any_element()
    }

    fn render_cpu_usage_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.sync_cpu_threshold_slider_states(window, cx);

        let enabled = self.settings.cpu_usage_mode.enabled;
        let help = tooltip_lines(vec![
            t!("cpu_rules.intro_1").to_string(),
            t!("cpu_rules.intro_2").to_string(),
            t!("common.power_plan_priority").to_string(),
            t!("common.power_plan_pause_priority").to_string(),
        ]);
        let mut content = page_shell(Page::CpuUsage, cx).child(feature_toggle_switch_with_help(
            "cpu-usage-enabled",
            t!("cpu_rules.enable").to_string(),
            help,
            enabled,
            cx.listener(|app, checked, _, cx| {
                app.settings.cpu_usage_mode.enabled = *checked;
                cx.notify();
            }),
        ));

        let mut body =
            feature_body(enabled).child(section_title_text(t!("common.rules").to_string()));
        body = body.child(create_rule_card(
            "create-cpu-rule-card",
            t!("cpu_rules.rule_title").to_string(),
            primary_control_button(Button::new("add-cpu-rule"), cx)
                .label(t!("common.create").to_string())
                .disabled(!enabled)
                .on_click(cx.listener(|app, _, window, cx| {
                    app.settings.cpu_usage_mode.rules.push(CpuUsageRule {
                        enabled: true,
                        name: t!("cpu_rules.new_rule").to_string(),
                        comparison: CpuUsageComparison::AtOrBelow,
                        threshold_percent: 20,
                        upper_threshold_percent: None,
                        duration_seconds: 30,
                        power_plan_guid: app.current_plan.as_ref().map(|plan| plan.guid.clone()),
                        else_enabled: false,
                        else_power_plan_guid: app
                            .current_plan
                            .as_ref()
                            .map(|plan| plan.guid.clone()),
                        target: None,
                    });
                    app.inputs.ensure_for_settings(window, cx, &app.settings);
                    cx.notify();
                }))
                .into_any_element(),
        ));
        let mut rules = rule_list();
        for (index, rule) in self.settings.cpu_usage_mode.rules.iter().enumerate() {
            rules = rules.child(self.render_cpu_rule(index, rule, enabled, window, cx));
        }
        body = body.child(rules);
        content = content.child(disabled_feature_body(body, enabled));

        content.into_any_element()
    }

    fn render_cpu_rule(
        &self,
        index: usize,
        rule: &CpuUsageRule,
        feature_enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(name_input) = self.inputs.cpu_rule_names.get(index).cloned() else {
            return syncing_rule_card(index);
        };
        let Some(threshold_state) = self.inputs.cpu_rule_thresholds.get(index).cloned() else {
            return syncing_rule_card(index);
        };
        let Some(upper_threshold_state) = self.inputs.cpu_rule_upper_thresholds.get(index).cloned()
        else {
            return syncing_rule_card(index);
        };
        let comparison_options = [
            CpuUsageComparison::AtOrBelow,
            CpuUsageComparison::AtOrAbove,
            CpuUsageComparison::Between,
        ];
        let selected_comparison = rule.comparison;
        let comparison_dropdown = self.render_dropdown_select(
            format!("cpu-comparison-{index}"),
            selected_comparison.label(),
            true,
            DropdownSelectWidth::Wide,
            comparison_options.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for comparison in comparison_options {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "cpu-comparison-{index}-option-{comparison:?}"
                            )),
                            comparison.label().to_owned(),
                            selected_comparison == comparison,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if let Some(rule) = app.settings.cpu_usage_mode.rules.get_mut(index) {
                                rule.comparison = comparison;
                                if comparison == CpuUsageComparison::Between {
                                    rule.upper_threshold_percent.get_or_insert(100);
                                }
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

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
            let mut condition_fields =
                vec![
                    rule_action_row(
                        format!("cpu-rule-comparison-{index}"),
                        t!("cpu_rules.when_cpu_load").to_string(),
                        comparison_dropdown,
                    )
                    .into_any_element(),
                    threshold_level_slider(
                        format!("cpu-rule-threshold-{index}"),
                        &t!("cpu_rules.threshold"),
                        self.render_numeric_value(
                            NumericField::CpuThreshold(index),
                            format!("{}%", rule.threshold_percent),
                            rule.threshold_percent.to_string(),
                            cx,
                        ),
                        &threshold_state,
                        feature_enabled,
                        window,
                        cx,
                        cx.listener(move |app, change: &StepChange<u8>, _, cx| {
                            if let Some(value) =
                                app.settings.cpu_usage_mode.rules.get(index).map(|rule| {
                                    apply_u8_step(rule.threshold_percent, change, 0, 100)
                                })
                            {
                                app.set_cpu_threshold_slider_value(
                                    CpuThresholdSlider::Lower(index),
                                    value,
                                );
                            }
                            cx.notify();
                        }),
                    )
                    .into_any_element(),
                ];
            if rule.comparison == CpuUsageComparison::Between {
                condition_fields.push(
                    threshold_level_slider(
                        format!("cpu-rule-upper-threshold-{index}"),
                        &t!("cpu_rules.upper_threshold"),
                        self.render_numeric_value(
                            NumericField::CpuUpperThreshold(index),
                            format!("{upper}%"),
                            upper.to_string(),
                            cx,
                        ),
                        &upper_threshold_state,
                        feature_enabled,
                        window,
                        cx,
                        cx.listener(move |app, change: &StepChange<u8>, _, cx| {
                            if let Some(value) =
                                app.settings.cpu_usage_mode.rules.get(index).map(|rule| {
                                    apply_u8_step(
                                        rule.upper_threshold_percent.unwrap_or(100),
                                        change,
                                        0,
                                        100,
                                    )
                                })
                            {
                                app.set_cpu_threshold_slider_value(
                                    CpuThresholdSlider::Upper(index),
                                    value,
                                );
                            }
                            cx.notify();
                        }),
                    )
                    .into_any_element(),
                );
            }
            condition_fields.push(
                rule_stepper_row_u64(
                    format!("cpu-rule-duration-{index}"),
                    t!("cpu_rules.duration").to_string(),
                    rule.duration_seconds,
                    self.render_numeric_value(
                        NumericField::CpuDuration(index),
                        format!("{} sec", rule.duration_seconds),
                        rule.duration_seconds.to_string(),
                        cx,
                    ),
                    cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                        if let Some(rule) = app.settings.cpu_usage_mode.rules.get_mut(index) {
                            rule.duration_seconds =
                                apply_u64_step(rule.duration_seconds, change, 0, 86_400);
                        }
                        cx.notify();
                    }),
                )
                .into_any_element(),
            );

            let mut plan_fields = vec![
                rule_action_row(
                    format!("cpu-rule-plan-{index}"),
                    t!("cpu_rules.use").to_string(),
                    self.render_inline_power_plan_picker(
                        format!("cpu-rule-plan-{index}"),
                        rule.power_plan_guid.clone(),
                        PowerPlanField::CpuRule(index),
                        window,
                        cx,
                    ),
                )
                .into_any_element(),
                rule_checkbox_row(
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
                ),
            ];
            if rule.else_enabled {
                plan_fields.push(
                    rule_action_row(
                        format!("cpu-rule-else-plan-{index}"),
                        t!("cpu_rules.else_use").to_string(),
                        self.render_inline_power_plan_picker(
                            format!("cpu-rule-else-plan-{index}"),
                            rule.else_power_plan_guid.clone(),
                            PowerPlanField::CpuRuleElse(index),
                            window,
                            cx,
                        ),
                    )
                    .into_any_element(),
                );
            }

            card = card
                .child(rule_card_body_row(condition_fields))
                .child(rule_card_body_row(plan_fields))
                .child(rule_card_body_actions(vec![
                    rename_rule_button(title_target, cx),
                    danger_control_button(Button::new(SharedString::from(format!(
                        "remove-cpu-rule-{index}"
                    ))))
                    .label(t!("common.remove").to_string())
                    .on_click(cx.listener(move |app, _, _, cx| {
                        if index < app.settings.cpu_usage_mode.rules.len() {
                            app.settings.cpu_usage_mode.rules.remove(index);
                        }
                        app.editing_rule_title = None;
                        app.expanded_rule_cards.clear();
                        cx.notify();
                    }))
                    .into_any_element(),
                ]));
        }
        card.into_any_element()
    }

    fn render_efficiency_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let input_value = self.inputs.eco_qos_exclusion.read(cx).value().to_string();
        let enabled = self.settings.eco_qos.enabled;
        let body = feature_body(enabled)
            .child(feature_toggle_switch_with_help(
                "eco-qos-foreground",
                t!("efficiency.focus_detection").to_string(),
                t!("efficiency.focus_detection_help").to_string(),
                self.settings.eco_qos.exclude_foreground_app,
                cx.listener(|app, checked, _, cx| {
                    app.settings.eco_qos.exclude_foreground_app = *checked;
                    cx.notify();
                }),
            ))
            .child(self.render_efficiency_aggressiveness_selector(window, cx))
            .child(self.render_efficiency_cpu_set_preference(window, cx))
            .child(section_header(
                &t!("efficiency.whitelist"),
                t!("efficiency.whitelist_help").to_string(),
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
                        primary_control_button(Button::new("add-eco-qos-exclusion"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_eco_qos_process(
                                        &self.settings.eco_qos,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process =
                                    app.inputs.eco_qos_exclusion.read(cx).value().to_string();
                                if can_add_eco_qos_process(&app.settings.eco_qos, &process) {
                                    app.settings
                                        .eco_qos
                                        .efficiency_whitelist
                                        .push(new_eco_qos_exclusion_rule(&process));
                                    clear_input(&app.inputs.eco_qos_exclusion, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_eco_qos_whitelist(cx));

        let help = tooltip_lines(vec![
            t!("efficiency.intro_1").to_string(),
            t!("efficiency.intro_2").to_string(),
            t!("efficiency.intro_3").to_string(),
        ]);

        page_shell(Page::EfficiencyMode, cx)
            .child(feature_toggle_switch_with_help(
                "eco-qos-enabled",
                t!("efficiency.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.eco_qos.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(body, enabled))
            .into_any_element()
    }

    fn render_efficiency_aggressiveness_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.settings.eco_qos.aggressiveness;
        h_flex()
            .id("eco-qos-aggressiveness-row")
            .w_full()
            .min_w(px(0.0))
            .min_h(px(58.0))
            .items_center()
            .justify_between()
            .gap_3()
            .py_3()
            .px_4()
            .text_color(rgb(primary_text_color()))
            .text_size(px(TEXT_BODY_SIZE))
            .line_height(px(TEXT_BODY_LINE_HEIGHT))
            .border_t_1()
            .border_color(rgb(border_color()))
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .min_w(px(0.0))
                            .truncate()
                            .child(t!("efficiency.aggressiveness").to_string()),
                    )
                    .child(title_info_button(
                        "eco-qos-aggressiveness-info",
                        t!("efficiency.aggressiveness_help").to_string(),
                    )),
            )
            .child(self.render_efficiency_aggressiveness_picker(selected, window, cx))
            .into_any_element()
    }

    fn render_efficiency_aggressiveness_picker(
        &self,
        selected: EcoQosAggressiveness,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picker_id = "eco-qos-aggressiveness";
        let is_open = self.active_power_plan_picker.as_deref() == Some(picker_id);
        let placement = self.dropdown_placement(
            picker_id,
            dropdown_list_height(EcoQosAggressiveness::ALL.len()),
            window,
        );
        let mut options = dropdown_surface(cx, placement.max_height);
        for aggressiveness in EcoQosAggressiveness::ALL {
            options = options.child(
                dropdown_option_row(
                    SharedString::from(format!("{picker_id}-option-{aggressiveness:?}")),
                    efficiency_aggressiveness_label(aggressiveness),
                    selected == aggressiveness,
                    cx,
                )
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.settings.eco_qos.aggressiveness = aggressiveness;
                    app.active_power_plan_picker = None;
                    cx.notify();
                })),
            );
        }

        dropdown_select_container(DropdownSelectWidth::Standard)
            .child(
                dropdown_select_control(
                    "eco-qos-aggressiveness-control",
                    efficiency_aggressiveness_label(selected),
                    true,
                    is_open,
                    cx,
                )
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.active_power_plan_picker = (app.active_power_plan_picker.as_deref()
                        != Some(picker_id))
                    .then_some(picker_id.to_owned());
                    cx.notify();
                })),
            )
            .child(dropdown_anchor_sensor(
                picker_id,
                Rc::clone(&self.dropdown_anchor_bounds),
            ))
            .child(dropdown_popup_or_empty(is_open, placement, options, cx))
            .into_any_element()
    }

    fn render_efficiency_cpu_set_preference(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let processors = affinity::logical_processors();
        let has_efficiency_cores =
            affinity_processors_kind_mask(&processors, LogicalProcessorKind::Efficiency) != 0;
        let has_multiple_processors = processors.len() > 1;
        let selected = self.effective_eco_qos_cpu_restriction_strategy();
        let restriction_enabled = selected != EcoQosCpuRestrictionStrategy::Off;
        let collapsed =
            self.is_setting_group_collapsed(SettingGroupTarget::EfficiencyCpuRestriction);
        let selected_mode = self.settings.eco_qos.cpu_restriction_mode;
        let mode_dropdown = self.render_dropdown_select(
            "eco-qos-cpu-restriction-mode",
            efficiency_cpu_restriction_mode_label(selected_mode),
            restriction_enabled,
            DropdownSelectWidth::Standard,
            EcoQosCpuRestrictionMode::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for mode in EcoQosCpuRestrictionMode::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "eco-qos-cpu-restriction-mode-option-{mode:?}"
                            )),
                            efficiency_cpu_restriction_mode_label(mode),
                            selected_mode == mode,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.eco_qos.cpu_restriction_mode = mode;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        let strategy_options = [
            EcoQosCpuRestrictionStrategy::Auto,
            EcoQosCpuRestrictionStrategy::PreferEfficiencyCores,
            EcoQosCpuRestrictionStrategy::LimitLogicalCpus,
        ];
        let strategy_dropdown = self.render_dropdown_select(
            "eco-qos-cpu-restriction-strategy",
            efficiency_cpu_restriction_strategy_label(selected),
            restriction_enabled,
            DropdownSelectWidth::Wide,
            strategy_options.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for strategy in strategy_options {
                    let option_enabled = match strategy {
                        EcoQosCpuRestrictionStrategy::PreferEfficiencyCores => has_efficiency_cores,
                        EcoQosCpuRestrictionStrategy::LimitLogicalCpus => has_multiple_processors,
                        EcoQosCpuRestrictionStrategy::Auto | EcoQosCpuRestrictionStrategy::Off => {
                            true
                        }
                    };
                    let row = dropdown_option_row(
                        SharedString::from(format!(
                            "eco-qos-cpu-restriction-strategy-option-{strategy:?}"
                        )),
                        efficiency_cpu_restriction_strategy_label(strategy),
                        selected == strategy,
                        cx,
                    )
                    .when(!option_enabled, |row| row.opacity(0.48).cursor_default());
                    let row = if option_enabled {
                        row.on_click(cx.listener(move |app, _, _, cx| {
                            let (prefer_efficiency_cores, limit_cpu_sets_on_non_hybrid) =
                                strategy.legacy_flags();
                            app.settings.eco_qos.cpu_restriction_strategy = strategy;
                            app.settings.eco_qos.prefer_efficiency_cores = prefer_efficiency_cores;
                            app.settings.eco_qos.limit_cpu_sets_on_non_hybrid =
                                limit_cpu_sets_on_non_hybrid;
                            if app.settings.eco_qos.cpu_restriction_control_style
                                == EcoQosCpuRestrictionControlStyle::CoreToggle
                            {
                                let processors = affinity::logical_processors();
                                let mask = eco_qos_strategy_core_mask(&processors, strategy);
                                if mask != 0 {
                                    app.settings.eco_qos.cpu_restriction_core_mask = mask;
                                }
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        }))
                    } else {
                        row
                    };
                    options = options.child(row);
                }
                options
            },
        );

        let percent = self.settings.eco_qos.cpu_restriction_percent.clamp(1, 100);
        let percentage_control = h_flex()
            .gap_2()
            .items_center()
            .justify_end()
            .flex_wrap()
            .child(self.render_numeric_value(
                NumericField::EcoQosRestrictionPercent,
                format!("{percent}%"),
                percent.to_string(),
                cx,
            ));

        let selected_style = self.settings.eco_qos.cpu_restriction_control_style;
        let style_dropdown = self.render_dropdown_select(
            "eco-qos-cpu-restriction-style",
            efficiency_cpu_restriction_control_style_label(selected_style),
            restriction_enabled,
            DropdownSelectWidth::Standard,
            EcoQosCpuRestrictionControlStyle::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for style in EcoQosCpuRestrictionControlStyle::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "eco-qos-cpu-restriction-style-option-{style:?}"
                            )),
                            efficiency_cpu_restriction_control_style_label(style),
                            selected_style == style,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.eco_qos.cpu_restriction_control_style = style;
                            if style == EcoQosCpuRestrictionControlStyle::CoreToggle
                                && app.settings.eco_qos.cpu_restriction_core_mask == 0
                            {
                                let strategy = app.effective_eco_qos_cpu_restriction_strategy();
                                let processors = affinity::logical_processors();
                                app.settings.eco_qos.cpu_restriction_core_mask =
                                    eco_qos_strategy_core_mask(&processors, strategy);
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        let mut rows = vec![
            setting_group_action_row(
                "eco-qos-core-affinity-control",
                t!("efficiency.core_affinity_control").to_string(),
                mode_dropdown,
                true,
            )
            .when(!restriction_enabled, |row| {
                row.opacity(0.42).cursor_default()
            })
            .into_any_element(),
            setting_group_action_row(
                "eco-qos-core-suppression-rule",
                t!("efficiency.core_suppression_rule").to_string(),
                strategy_dropdown,
                true,
            )
            .into_any_element(),
            setting_group_action_row(
                "eco-qos-control-style",
                t!("efficiency.control_style").to_string(),
                style_dropdown,
                true,
            )
            .when(!restriction_enabled, |row| {
                row.opacity(0.42).cursor_default()
            })
            .into_any_element(),
        ];

        rows.push(match self.settings.eco_qos.cpu_restriction_control_style {
            EcoQosCpuRestrictionControlStyle::Percentage => setting_group_action_row(
                "eco-qos-core-allocation-percentage",
                t!("efficiency.core_allocation_percentage").to_string(),
                percentage_control.into_any_element(),
                true,
            )
            .when(!restriction_enabled, |row| {
                row.opacity(0.42).cursor_default()
            })
            .into_any_element(),
            EcoQosCpuRestrictionControlStyle::CoreToggle => setting_group_stacked_action_row(
                "eco-qos-core-toggle-list",
                t!("efficiency.selected_cores").to_string(),
                self.render_efficiency_core_toggle_selector(&processors, restriction_enabled, cx),
                true,
            )
            .when(!restriction_enabled, |row| {
                row.opacity(0.42).cursor_default()
            })
            .into_any_element(),
        });

        setting_group_with_title_element(
            SettingGroupTarget::EfficiencyCpuRestriction,
            h_flex()
                .flex_1()
                .min_w(px(0.0))
                .items_center()
                .gap_1()
                .child(
                    div()
                        .min_w(px(0.0))
                        .truncate()
                        .child(t!("efficiency.cpu_set_preference").to_string()),
                )
                .child(title_info_button(
                    "eco-qos-cpu-set-preference-info",
                    t!("efficiency.cpu_set_preference_help").to_string(),
                ))
                .into_any_element(),
            setting_group_switch_action(
                "eco-qos-cpu-restriction-enabled",
                restriction_enabled,
                cx.listener(|app, checked, _, cx| {
                    if *checked {
                        if app.effective_eco_qos_cpu_restriction_strategy()
                            == EcoQosCpuRestrictionStrategy::Off
                        {
                            app.settings.eco_qos.cpu_restriction_strategy =
                                EcoQosCpuRestrictionStrategy::Auto;
                            app.settings.eco_qos.prefer_efficiency_cores = true;
                            app.settings.eco_qos.limit_cpu_sets_on_non_hybrid = true;
                        }
                    } else {
                        app.settings.eco_qos.cpu_restriction_strategy =
                            EcoQosCpuRestrictionStrategy::Off;
                        app.settings.eco_qos.prefer_efficiency_cores = false;
                        app.settings.eco_qos.limit_cpu_sets_on_non_hybrid = false;
                    }
                    cx.notify();
                }),
            ),
            collapsed,
            rows,
            cx,
        )
        .into_any_element()
    }

    fn render_efficiency_core_toggle_selector(
        &self,
        processors: &[LogicalProcessorInfo],
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let available_mask = affinity_processors_mask(processors);
        self.render_core_tile_grid(
            processors,
            self.settings.eco_qos.cpu_restriction_core_mask,
            enabled,
            "eco-qos-core-toggle",
            CoreTileGridAction::EcoQosCpuRestriction { available_mask },
            cx,
        )
    }

    fn effective_eco_qos_cpu_restriction_strategy(&self) -> EcoQosCpuRestrictionStrategy {
        let legacy_strategy = EcoQosCpuRestrictionStrategy::from_legacy_flags(
            self.settings.eco_qos.prefer_efficiency_cores,
            self.settings.eco_qos.limit_cpu_sets_on_non_hybrid,
        );
        if self.settings.eco_qos.cpu_restriction_strategy == EcoQosCpuRestrictionStrategy::Auto
            && legacy_strategy != EcoQosCpuRestrictionStrategy::Auto
        {
            legacy_strategy
        } else {
            self.settings.eco_qos.cpu_restriction_strategy
        }
    }

    fn effective_background_cpu_restriction_strategy(&self) -> EcoQosCpuRestrictionStrategy {
        self.settings.background_cpu_restriction.strategy
    }

    fn render_eco_qos_whitelist(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut list = v_flex().gap_2();
        for (index, rule) in self
            .settings
            .eco_qos
            .efficiency_whitelist
            .iter()
            .enumerate()
        {
            let process = rule.process_name.clone();
            list = list.child(
                compact_rule_row(cx)
                    .child(rule_enable_checkbox(
                        format!("eco-qos-exclusion-enabled-{index}"),
                        rule.enabled,
                        cx.listener(move |app, checked, _, cx| {
                            if let Some(rule) =
                                app.settings.eco_qos.efficiency_whitelist.get_mut(index)
                            {
                                rule.enabled = *checked;
                            }
                            cx.notify();
                        }),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(160.0))
                            .text_size(px(RULE_TITLE_TEXT_SIZE))
                            .line_height(px(RULE_TITLE_LINE_HEIGHT))
                            .truncate()
                            .child(process),
                    )
                    .child(
                        danger_control_button(Button::new(SharedString::from(format!(
                            "remove-eco-qos-{index}"
                        ))))
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
        let enabled = self.settings.app_suspension.enabled;
        let thaw_group_collapsed =
            self.is_setting_group_collapsed(SettingGroupTarget::SuspensionThaw);
        let audio_group_collapsed =
            self.is_setting_group_collapsed(SettingGroupTarget::SuspensionAudio);
        let network_group_collapsed =
            self.is_setting_group_collapsed(SettingGroupTarget::SuspensionNetwork);
        let body = feature_body(enabled)
            .child(setting_stepper_card_u64(
                "suspension-background-delay",
                t!("suspension.background_delay").to_string(),
                self.settings.app_suspension.background_delay_seconds,
                self.render_numeric_value(
                    NumericField::SuspensionBackgroundDelay,
                    format!(
                        "{} sec",
                        self.settings.app_suspension.background_delay_seconds
                    ),
                    self.settings
                        .app_suspension
                        .background_delay_seconds
                        .to_string(),
                    cx,
                ),
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
            .child(setting_group(
                SettingGroupTarget::SuspensionThaw,
                t!("suspension.temporary_thaw").to_string(),
                setting_group_switch_action(
                    "temporary-thaw",
                    self.settings.app_suspension.temporary_thaw_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.app_suspension.temporary_thaw_enabled = *checked;
                        cx.notify();
                    }),
                ),
                thaw_group_collapsed,
                vec![
                    setting_group_stepper_row_u64(
                        "suspension-thaw-interval",
                        t!("suspension.thaw_every").to_string(),
                        self.settings.app_suspension.temporary_thaw_interval_seconds,
                        self.render_numeric_value(
                            NumericField::SuspensionThawInterval,
                            format!(
                                "{} sec",
                                self.settings.app_suspension.temporary_thaw_interval_seconds
                            ),
                            self.settings
                                .app_suspension
                                .temporary_thaw_interval_seconds
                                .to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            app.settings.app_suspension.temporary_thaw_interval_seconds =
                                apply_u64_step(
                                    app.settings.app_suspension.temporary_thaw_interval_seconds,
                                    change,
                                    1,
                                    86_400,
                                );
                            cx.notify();
                        }),
                    ),
                    setting_group_stepper_row_u64(
                        "suspension-thaw-duration",
                        t!("suspension.thaw_duration").to_string(),
                        self.settings.app_suspension.temporary_thaw_duration_seconds,
                        self.render_numeric_value(
                            NumericField::SuspensionThawDuration,
                            format!(
                                "{} sec",
                                self.settings.app_suspension.temporary_thaw_duration_seconds
                            ),
                            self.settings
                                .app_suspension
                                .temporary_thaw_duration_seconds
                                .to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            app.settings.app_suspension.temporary_thaw_duration_seconds =
                                apply_u64_step(
                                    app.settings.app_suspension.temporary_thaw_duration_seconds,
                                    change,
                                    1,
                                    3_600,
                                );
                            cx.notify();
                        }),
                    ),
                ],
                cx,
            ))
            .child(setting_group(
                SettingGroupTarget::SuspensionAudio,
                t!("suspension.audio_detection").to_string(),
                setting_group_switch_action(
                    "audio-wake",
                    self.settings.app_suspension.audio_wake_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.app_suspension.audio_wake_enabled = *checked;
                        cx.notify();
                    }),
                ),
                audio_group_collapsed,
                vec![setting_group_stepper_row_u64(
                    "suspension-audio-refreeze",
                    t!("suspension.audio_refreeze").to_string(),
                    self.settings.app_suspension.audio_wake_duration_seconds,
                    self.render_numeric_value(
                        NumericField::SuspensionAudioRefreeze,
                        format!(
                            "{} sec quiet",
                            self.settings.app_suspension.audio_wake_duration_seconds
                        ),
                        self.settings
                            .app_suspension
                            .audio_wake_duration_seconds
                            .to_string(),
                        cx,
                    ),
                    true,
                    cx.listener(|app, change: &StepChange<u64>, _, cx| {
                        app.settings.app_suspension.audio_wake_duration_seconds = apply_u64_step(
                            app.settings.app_suspension.audio_wake_duration_seconds,
                            change,
                            1,
                            3_600,
                        );
                        cx.notify();
                    }),
                )],
                cx,
            ))
            .child(setting_group(
                SettingGroupTarget::SuspensionNetwork,
                t!("suspension.network_detection").to_string(),
                setting_group_switch_action(
                    "network-wake",
                    self.settings.app_suspension.network_wake_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.app_suspension.network_wake_enabled = *checked;
                        cx.notify();
                    }),
                ),
                network_group_collapsed,
                vec![setting_group_stepper_row_u64(
                    "suspension-network-refreeze",
                    t!("suspension.network_refreeze").to_string(),
                    self.settings.app_suspension.network_wake_duration_seconds,
                    self.render_numeric_value(
                        NumericField::SuspensionNetworkRefreeze,
                        format!(
                            "{} sec quiet",
                            self.settings.app_suspension.network_wake_duration_seconds
                        ),
                        self.settings
                            .app_suspension
                            .network_wake_duration_seconds
                            .to_string(),
                        cx,
                    ),
                    true,
                    cx.listener(|app, change: &StepChange<u64>, _, cx| {
                        app.settings.app_suspension.network_wake_duration_seconds = apply_u64_step(
                            app.settings.app_suspension.network_wake_duration_seconds,
                            change,
                            1,
                            3_600,
                        );
                        cx.notify();
                    }),
                )],
                cx,
            ))
            .child(section_header(
                &t!("suspension.suspendable_apps"),
                t!("suspension.suspendable_help").to_string(),
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
                        primary_control_button(Button::new("add-suspension-process"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_suspension_process(
                                        &self.settings.app_suspension,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process =
                                    app.inputs.suspension_process.read(cx).value().to_string();
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
            .child(self.render_suspendable_apps(window, cx));

        let help = tooltip_lines(vec![
            t!("suspension.intro_1").to_string(),
            t!("suspension.intro_2").to_string(),
            t!("suspension.intro_3").to_string(),
        ]);

        page_shell(Page::AppSuspension, cx)
            .child(feature_toggle_switch_with_help(
                "app-suspension-enabled",
                t!("suspension.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.app_suspension.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(body, enabled))
            .into_any_element()
    }

    fn render_suspendable_apps(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
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
            let rule_enabled = rule.enabled;
            let network_thresholds_enabled = rule_enabled
                && self.settings.app_suspension.network_wake_enabled
                && rule.network_wake_enabled;
            let freeze_action = control_button(Button::new(SharedString::from(format!(
                "freeze-suspension-{index}"
            ))))
            .label(t!("suspension.freeze").to_string())
            .disabled(!rule_enabled || !can_manual_freeze(&self.app_suspension_status, &process))
            .on_click(cx.listener({
                let process = process.clone();
                move |app, _, _, cx| {
                    cx.stop_propagation();
                    app.background_automation
                        .request_app_suspension_freeze(&process);
                    app.status_message =
                        t!("suspension.manual_freeze_requested", process = process).to_string();
                    cx.notify();
                }
            }))
            .into_any_element();
            let title = h_flex()
                .flex_1()
                .min_w(px(0.0))
                .gap_2()
                .items_center()
                .child(self.process_rule_title(&process, cx))
                .child(status_pill(indicator.label, indicator.bg, indicator.fg))
                .into_any_element();
            let mut card = rule_card_with_header_action(
                title,
                rule_enable_checkbox(
                    format!("suspension-rule-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        if let Some(rule) =
                            app.settings.app_suspension.suspendable_apps.get_mut(index)
                        {
                            rule.enabled = *checked;
                        }
                        cx.notify();
                    }),
                ),
                Some(freeze_action),
                rule_card_collapse_indicator(collapsed),
                card_target.clone(),
                cx,
            );
            if !collapsed {
                card = card
                    .child(rule_card_body_row(vec![
                        rule_toggle_switch(
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
                        ),
                        rule_toggle_switch(
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
                        ),
                    ]))
                    .child(rule_card_body_row(vec![
                        self.render_network_threshold(
                            index,
                            true,
                            &t!("suspension.download_threshold"),
                            rule.network_download_threshold_bytes,
                            rule.network_download_threshold_unit,
                            ThresholdField::Download(index),
                            network_thresholds_enabled,
                            window,
                            cx,
                        ),
                        self.render_network_threshold(
                            index,
                            false,
                            &t!("suspension.upload_threshold"),
                            rule.network_upload_threshold_bytes,
                            rule.network_upload_threshold_unit,
                            ThresholdField::Upload(index),
                            network_thresholds_enabled,
                            window,
                            cx,
                        ),
                    ]))
                    .child(rule_card_body_action(
                        danger_control_button(Button::new(SharedString::from(format!(
                            "remove-suspension-{index}"
                        ))))
                        .label(t!("common.remove").to_string())
                        .on_click(cx.listener({
                            let card_target = card_target.clone();
                            move |app, _, _, cx| {
                                if index < app.settings.app_suspension.suspendable_apps.len() {
                                    app.settings.app_suspension.suspendable_apps.remove(index);
                                }
                                app.expanded_rule_cards.remove(&card_target);
                                cx.notify();
                            }
                        }))
                        .into_any_element(),
                    ));
            }
            list = list.child(card);
        }
        if self.settings.app_suspension.suspendable_apps.is_empty() {
            list = list.child(text_muted(t!("suspension.no_suspendable").to_string()));
        }
        list.into_any_element()
    }

    fn render_background_cpu_restriction_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .background_cpu_exclusion
            .read(cx)
            .value()
            .to_string();
        let settings = &self.settings.background_cpu_restriction;
        let enabled = settings.enabled;
        let processors = affinity::logical_processors();
        let has_efficiency_cores =
            affinity_processors_kind_mask(&processors, LogicalProcessorKind::Efficiency) != 0;
        let has_multiple_processors = processors.len() > 1;
        let selected = self.effective_background_cpu_restriction_strategy();
        let restriction_enabled = selected != EcoQosCpuRestrictionStrategy::Off;
        let available_mask = affinity_processors_mask(&processors);

        let selected_mode = settings.mode;
        let mode_dropdown = self.render_dropdown_select(
            "background-cpu-mode",
            efficiency_cpu_restriction_mode_label(selected_mode),
            restriction_enabled,
            DropdownSelectWidth::Standard,
            EcoQosCpuRestrictionMode::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for mode in EcoQosCpuRestrictionMode::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("background-cpu-mode-option-{mode:?}")),
                            efficiency_cpu_restriction_mode_label(mode),
                            selected_mode == mode,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.background_cpu_restriction.mode = mode;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        let strategy_options = [
            EcoQosCpuRestrictionStrategy::Auto,
            EcoQosCpuRestrictionStrategy::PreferEfficiencyCores,
            EcoQosCpuRestrictionStrategy::LimitLogicalCpus,
        ];
        let strategy_dropdown = self.render_dropdown_select(
            "background-cpu-strategy",
            efficiency_cpu_restriction_strategy_label(selected),
            restriction_enabled,
            DropdownSelectWidth::Wide,
            strategy_options.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for strategy in strategy_options {
                    let option_enabled = match strategy {
                        EcoQosCpuRestrictionStrategy::PreferEfficiencyCores => has_efficiency_cores,
                        EcoQosCpuRestrictionStrategy::LimitLogicalCpus => has_multiple_processors,
                        EcoQosCpuRestrictionStrategy::Auto | EcoQosCpuRestrictionStrategy::Off => {
                            true
                        }
                    };
                    let row = dropdown_option_row(
                        SharedString::from(format!("background-cpu-strategy-option-{strategy:?}")),
                        efficiency_cpu_restriction_strategy_label(strategy),
                        selected == strategy,
                        cx,
                    )
                    .when(!option_enabled, |row| row.opacity(0.48).cursor_default());
                    let row = if option_enabled {
                        row.on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.background_cpu_restriction.strategy = strategy;
                            if app.settings.background_cpu_restriction.control_style
                                == EcoQosCpuRestrictionControlStyle::CoreToggle
                            {
                                let processors = affinity::logical_processors();
                                let mask = eco_qos_strategy_core_mask(&processors, strategy);
                                if mask != 0 {
                                    app.settings.background_cpu_restriction.core_mask = mask;
                                }
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        }))
                    } else {
                        row
                    };
                    options = options.child(row);
                }
                options
            },
        );

        let selected_style = settings.control_style;
        let style_dropdown = self.render_dropdown_select(
            "background-cpu-style",
            efficiency_cpu_restriction_control_style_label(selected_style),
            restriction_enabled,
            DropdownSelectWidth::Standard,
            EcoQosCpuRestrictionControlStyle::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for style in EcoQosCpuRestrictionControlStyle::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("background-cpu-style-option-{style:?}")),
                            efficiency_cpu_restriction_control_style_label(style),
                            selected_style == style,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.background_cpu_restriction.control_style = style;
                            if style == EcoQosCpuRestrictionControlStyle::CoreToggle
                                && app.settings.background_cpu_restriction.core_mask == 0
                            {
                                let processors = affinity::logical_processors();
                                let strategy = app.effective_background_cpu_restriction_strategy();
                                app.settings.background_cpu_restriction.core_mask =
                                    eco_qos_strategy_core_mask(&processors, strategy);
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        let percent = settings.percent.clamp(1, 100);
        let percentage_control = self.render_numeric_value(
            NumericField::BackgroundCpuRestrictionPercent,
            format!("{percent}%"),
            percent.to_string(),
            cx,
        );

        let mut rows = vec![
            setting_group_action_row(
                "background-cpu-affinity-control",
                t!("background_cpu.core_affinity_control").to_string(),
                mode_dropdown,
                true,
            )
            .into_any_element(),
            setting_group_action_row(
                "background-cpu-suppression-rule",
                t!("background_cpu.core_suppression_rule").to_string(),
                strategy_dropdown,
                true,
            )
            .into_any_element(),
            setting_group_action_row(
                "background-cpu-control-style",
                t!("background_cpu.control_style").to_string(),
                style_dropdown,
                true,
            )
            .into_any_element(),
        ];
        rows.push(match settings.control_style {
            EcoQosCpuRestrictionControlStyle::Percentage => setting_group_action_row(
                "background-cpu-percent",
                t!("background_cpu.core_allocation_percentage").to_string(),
                percentage_control,
                true,
            )
            .into_any_element(),
            EcoQosCpuRestrictionControlStyle::CoreToggle => setting_group_stacked_action_row(
                "background-cpu-core-toggle-list",
                t!("background_cpu.selected_cores").to_string(),
                self.render_core_tile_grid(
                    &processors,
                    settings.core_mask,
                    restriction_enabled,
                    "background-cpu-core-toggle",
                    CoreTileGridAction::BackgroundCpuRestriction { available_mask },
                    cx,
                ),
                true,
            )
            .into_any_element(),
        });

        let body = feature_body(enabled)
            .child(feature_toggle_switch_with_help(
                "background-cpu-foreground",
                t!("background_cpu.focus_detection").to_string(),
                t!("background_cpu.focus_detection_help").to_string(),
                settings.exclude_foreground_app,
                cx.listener(|app, checked, _, cx| {
                    app.settings
                        .background_cpu_restriction
                        .exclude_foreground_app = *checked;
                    cx.notify();
                }),
            ))
            .child(setting_group_with_title_element(
                SettingGroupTarget::BackgroundCpuRestriction,
                div()
                    .min_w(px(0.0))
                    .truncate()
                    .child(t!("background_cpu.cpu_restriction").to_string())
                    .into_any_element(),
                setting_group_switch_action(
                    "background-cpu-restriction-enabled",
                    restriction_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.background_cpu_restriction.strategy = if *checked {
                            EcoQosCpuRestrictionStrategy::Auto
                        } else {
                            EcoQosCpuRestrictionStrategy::Off
                        };
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(SettingGroupTarget::BackgroundCpuRestriction),
                rows,
                cx,
            ))
            .child(stat_grid(vec![
                (
                    t!("background_cpu.adjusted_processes").to_string(),
                    self.background_cpu_restriction_status
                        .adjusted_processes
                        .to_string(),
                ),
                (
                    t!("background_cpu.scanned_processes").to_string(),
                    self.background_cpu_restriction_status
                        .scanned_processes
                        .to_string(),
                ),
                (
                    t!("background_cpu.skipped_processes").to_string(),
                    self.background_cpu_restriction_status
                        .skipped_processes
                        .to_string(),
                ),
                (
                    t!("background_cpu.failed_actions").to_string(),
                    self.background_cpu_restriction_status
                        .failed_processes
                        .to_string(),
                ),
            ]))
            .child(section_header(
                &t!("background_cpu.exclusions"),
                t!("background_cpu.exclusions_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "background-cpu-exclusion",
                        &self.inputs.background_cpu_exclusion,
                        SuggestionTarget::BackgroundCpu,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-background-cpu-exclusion"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_background_cpu_exclusion(
                                        &self.settings.background_cpu_restriction,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process = app
                                    .inputs
                                    .background_cpu_exclusion
                                    .read(cx)
                                    .value()
                                    .to_string();
                                if can_add_background_cpu_exclusion(
                                    &app.settings.background_cpu_restriction,
                                    &process,
                                ) {
                                    app.settings
                                        .background_cpu_restriction
                                        .exclusions
                                        .push(new_process_exclusion_rule(&process));
                                    clear_input(&app.inputs.background_cpu_exclusion, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_background_cpu_exclusions(cx));

        page_shell(Page::BackgroundCpuRestriction, cx)
            .child(feature_toggle_switch_with_help(
                "background-cpu-enabled",
                t!("background_cpu.enable").to_string(),
                tooltip_lines(vec![
                    t!("background_cpu.intro_1").to_string(),
                    t!("background_cpu.intro_2").to_string(),
                ]),
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.background_cpu_restriction.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(body, enabled))
            .into_any_element()
    }

    fn render_background_cpu_exclusions(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut list = v_flex().gap_2();
        for (index, rule) in self
            .settings
            .background_cpu_restriction
            .exclusions
            .iter()
            .enumerate()
        {
            let process = rule.process_name.clone();
            list = list.child(
                compact_rule_row(cx)
                    .child(rule_enable_checkbox(
                        format!("background-cpu-exclusion-enabled-{index}"),
                        rule.enabled,
                        cx.listener(move |app, checked, _, cx| {
                            if let Some(rule) = app
                                .settings
                                .background_cpu_restriction
                                .exclusions
                                .get_mut(index)
                            {
                                rule.enabled = *checked;
                            }
                            cx.notify();
                        }),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(160.0))
                            .text_size(px(RULE_TITLE_TEXT_SIZE))
                            .line_height(px(RULE_TITLE_LINE_HEIGHT))
                            .truncate()
                            .child(process),
                    )
                    .child(
                        danger_control_button(Button::new(SharedString::from(format!(
                            "remove-background-cpu-exclusion-{index}"
                        ))))
                        .label(t!("common.remove").to_string())
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if index < app.settings.background_cpu_restriction.exclusions.len() {
                                app.settings
                                    .background_cpu_restriction
                                    .exclusions
                                    .remove(index);
                            }
                            cx.notify();
                        })),
                    ),
            );
        }
        if self
            .settings
            .background_cpu_restriction
            .exclusions
            .is_empty()
        {
            list = list.child(text_muted(t!("background_cpu.no_exclusions").to_string()));
        }
        list.into_any_element()
    }

    fn render_cpu_limiter_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let input_value = self.inputs.cpu_limiter_process.read(cx).value().to_string();
        let enabled = self.settings.cpu_limiter.enabled;
        let body = feature_body(enabled)
            .child(feature_toggle_switch_with_help(
                "cpu-limiter-foreground",
                t!("cpu_limiter.focus_detection").to_string(),
                t!("cpu_limiter.focus_detection_help").to_string(),
                self.settings.cpu_limiter.exclude_foreground_app,
                cx.listener(|app, checked, _, cx| {
                    app.settings.cpu_limiter.exclude_foreground_app = *checked;
                    cx.notify();
                }),
            ))
            .child(section_header(
                &t!("cpu_limiter.rules"),
                t!("cpu_limiter.rules_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "cpu-limiter-suggestion",
                        &self.inputs.cpu_limiter_process,
                        SuggestionTarget::CpuLimiter,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-cpu-limiter-process"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_cpu_limiter_process(
                                        &self.settings.cpu_limiter,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process =
                                    app.inputs.cpu_limiter_process.read(cx).value().to_string();
                                if can_add_cpu_limiter_process(&app.settings.cpu_limiter, &process)
                                {
                                    app.settings
                                        .cpu_limiter
                                        .rules
                                        .push(new_cpu_limiter_rule(&process));
                                    clear_input(&app.inputs.cpu_limiter_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_cpu_limiter_rules(cx));

        let help = tooltip_lines(vec![
            t!("cpu_limiter.intro_1").to_string(),
            t!("cpu_limiter.intro_2").to_string(),
            t!("cpu_limiter.intro_3").to_string(),
        ]);

        page_shell(Page::CpuLimiter, cx)
            .child(feature_toggle_switch_with_help(
                "cpu-limiter-enabled",
                t!("cpu_limiter.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.cpu_limiter.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(body, enabled))
            .into_any_element()
    }

    fn render_cpu_limiter_rules(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut list = rule_list();
        for (index, rule) in self.settings.cpu_limiter.rules.iter().enumerate() {
            let process = rule.process_name.clone();
            let indicator = cpu_limiter_indicator(&self.cpu_limiter_status, &process);
            let card_target = RuleCardTarget::CpuLimiter(process.clone());
            let collapsed = self.is_rule_card_collapsed(&card_target);
            let mut card = rule_card(
                self.process_rule_title(&process, cx),
                rule_enable_checkbox(
                    format!("cpu-limiter-rule-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        if let Some(rule) = app.settings.cpu_limiter.rules.get_mut(index) {
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
                    .child(rule_card_body_row(vec![rule_action_row(
                        format!("cpu-limiter-rule-status-{index}"),
                        t!("common.status").to_string(),
                        status_pill(indicator.0, indicator.1, indicator.2).into_any_element(),
                    )
                    .into_any_element()]))
                    .child(rule_card_body_row(vec![
                        self.render_cpu_limiter_numeric_row(
                            index,
                            NumericField::CpuLimiterThreshold(index),
                            t!("cpu_limiter.threshold").to_string(),
                            format!("{}%", rule.threshold_percent),
                            rule.threshold_percent.to_string(),
                            cx,
                        ),
                        self.render_cpu_limiter_numeric_row(
                            index,
                            NumericField::CpuLimiterMaxProcessors(index),
                            t!("cpu_limiter.max_processors").to_string(),
                            rule.max_logical_processors.to_string(),
                            rule.max_logical_processors.to_string(),
                            cx,
                        ),
                    ]))
                    .child(rule_card_body_row(vec![
                        self.render_cpu_limiter_numeric_row(
                            index,
                            NumericField::CpuLimiterSustain(index),
                            t!("cpu_limiter.sustain").to_string(),
                            format!("{} sec", rule.sustain_seconds),
                            rule.sustain_seconds.to_string(),
                            cx,
                        ),
                        self.render_cpu_limiter_numeric_row(
                            index,
                            NumericField::CpuLimiterCooldown(index),
                            t!("cpu_limiter.cooldown").to_string(),
                            format!("{} sec", rule.cooldown_seconds),
                            rule.cooldown_seconds.to_string(),
                            cx,
                        ),
                    ]))
                    .child(rule_card_body_action(
                        danger_control_button(Button::new(SharedString::from(format!(
                            "remove-cpu-limiter-{index}"
                        ))))
                        .label(t!("common.remove").to_string())
                        .on_click(cx.listener({
                            let card_target = card_target.clone();
                            move |app, _, _, cx| {
                                if index < app.settings.cpu_limiter.rules.len() {
                                    app.settings.cpu_limiter.rules.remove(index);
                                }
                                app.expanded_rule_cards.remove(&card_target);
                                cx.notify();
                            }
                        }))
                        .into_any_element(),
                    ));
            }
            list = list.child(card);
        }
        if self.settings.cpu_limiter.rules.is_empty() {
            list = list.child(text_muted(t!("cpu_limiter.no_rules").to_string()));
        }
        list.into_any_element()
    }

    fn render_cpu_limiter_numeric_row(
        &self,
        index: usize,
        field: NumericField,
        label: String,
        display_value: String,
        edit_value: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        rule_action_row(
            format!("cpu-limiter-numeric-{index}-{field:?}"),
            label,
            self.render_numeric_value(field, display_value, edit_value, cx),
        )
        .into_any_element()
    }

    fn render_watchdog_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let input_value = self.inputs.watchdog_process.read(cx).value().to_string();
        let enabled = self.settings.watchdog.enabled;
        let can_add = enabled && can_add_watchdog_process(&self.settings.watchdog, &input_value);
        let body = feature_body(enabled)
            .child(section_header(
                &t!("watchdog.rules"),
                t!("watchdog.rules_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "watchdog-suggestion",
                        &self.inputs.watchdog_process,
                        SuggestionTarget::Watchdog,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-watchdog-terminate"), cx)
                            .label(t!("watchdog.add_terminate").to_string())
                            .disabled(!can_add)
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process =
                                    app.inputs.watchdog_process.read(cx).value().to_string();
                                if can_add_watchdog_process(&app.settings.watchdog, &process) {
                                    app.settings.watchdog.rules.push(new_watchdog_rule(
                                        &process,
                                        WatchdogAction::TerminateOnLaunch,
                                    ));
                                    clear_input(&app.inputs.watchdog_process, window, cx);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        primary_control_button(Button::new("add-watchdog-restart"), cx)
                            .label(t!("watchdog.add_restart").to_string())
                            .disabled(!can_add)
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process =
                                    app.inputs.watchdog_process.read(cx).value().to_string();
                                if can_add_watchdog_process(&app.settings.watchdog, &process) {
                                    app.settings.watchdog.rules.push(new_watchdog_rule(
                                        &process,
                                        WatchdogAction::RestartIfExited,
                                    ));
                                    clear_input(&app.inputs.watchdog_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_watchdog_rules(window, cx));

        let help = tooltip_lines(vec![
            t!("watchdog.intro_1").to_string(),
            t!("watchdog.intro_2").to_string(),
            t!("watchdog.intro_3").to_string(),
        ]);

        page_shell(Page::Watchdog, cx)
            .child(feature_toggle_switch_with_help(
                "watchdog-enabled",
                t!("watchdog.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.watchdog.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(body, enabled))
            .into_any_element()
    }

    fn render_watchdog_rules(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let mut list = rule_list();
        for (index, rule) in self.settings.watchdog.rules.iter().enumerate() {
            let process = rule.process_name.clone();
            let indicator = watchdog_indicator(&self.watchdog_status, &process);
            let card_target = RuleCardTarget::Watchdog(process.clone());
            let collapsed = self.is_rule_card_collapsed(&card_target);
            let mut card = rule_card(
                self.process_rule_title(&process, cx),
                rule_enable_checkbox(
                    format!("watchdog-rule-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        if let Some(rule) = app.settings.watchdog.rules.get_mut(index) {
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
                    .child(rule_card_body_row(vec![rule_action_row(
                        format!("watchdog-rule-status-{index}"),
                        t!("common.status").to_string(),
                        status_pill(indicator.0, indicator.1, indicator.2).into_any_element(),
                    )
                    .into_any_element()]))
                    .child(rule_card_body_row(vec![self
                        .render_watchdog_action_selector(
                            index,
                            rule.action,
                            window,
                            cx,
                        )]));

                if rule.action == WatchdogAction::RestartIfExited {
                    if let Some(input) = self.inputs.watchdog_launch_paths.get(index) {
                        card = card.child(rule_card_body_row(vec![rule_action_row(
                            format!("watchdog-launch-path-{index}"),
                            t!("watchdog.launch_path").to_string(),
                            app_input(
                                input,
                                input.read(cx).focus_handle(cx).is_focused(window),
                                cx,
                            )
                            .into_any_element(),
                        )
                        .into_any_element()]));
                    }
                    if let Some(input) = self.inputs.watchdog_launch_args.get(index) {
                        card = card.child(rule_card_body_row(vec![rule_action_row(
                            format!("watchdog-launch-args-{index}"),
                            t!("watchdog.launch_args").to_string(),
                            app_input(
                                input,
                                input.read(cx).focus_handle(cx).is_focused(window),
                                cx,
                            )
                            .into_any_element(),
                        )
                        .into_any_element()]));
                    }
                    card = card.child(rule_card_body_row(vec![rule_action_row(
                        format!("watchdog-restart-delay-{index}"),
                        t!("watchdog.restart_delay").to_string(),
                        self.render_numeric_value(
                            NumericField::WatchdogRestartDelay(index),
                            format!("{} sec", rule.restart_delay_seconds),
                            rule.restart_delay_seconds.to_string(),
                            cx,
                        ),
                    )
                    .into_any_element()]));
                }

                card = card.child(rule_card_body_action(
                    danger_control_button(Button::new(SharedString::from(format!(
                        "remove-watchdog-{index}"
                    ))))
                    .label(t!("common.remove").to_string())
                    .on_click(cx.listener({
                        let card_target = card_target.clone();
                        move |app, _, _, cx| {
                            if index < app.settings.watchdog.rules.len() {
                                app.settings.watchdog.rules.remove(index);
                            }
                            app.expanded_rule_cards.remove(&card_target);
                            cx.notify();
                        }
                    }))
                    .into_any_element(),
                ));
            }
            list = list.child(card);
        }
        if self.settings.watchdog.rules.is_empty() {
            list = list.child(text_muted(t!("watchdog.no_rules").to_string()));
        }
        list.into_any_element()
    }

    fn render_watchdog_action_selector(
        &self,
        index: usize,
        selected_action: WatchdogAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let action_options = [
            WatchdogAction::TerminateOnLaunch,
            WatchdogAction::RestartIfExited,
        ];
        let dropdown = self.render_dropdown_select(
            format!("watchdog-action-{index}"),
            watchdog_action_label(selected_action),
            true,
            DropdownSelectWidth::Standard,
            action_options.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for action in action_options {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "watchdog-action-{index}-option-{action:?}"
                            )),
                            watchdog_action_label(action),
                            selected_action == action,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if let Some(rule) = app.settings.watchdog.rules.get_mut(index) {
                                rule.action = action;
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );
        rule_action_row(
            format!("watchdog-action-row-{index}"),
            t!("watchdog.action").to_string(),
            dropdown,
        )
        .into_any_element()
    }

    fn render_performance_mode_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self.inputs.performance_process.read(cx).value().to_string();
        let enabled = self.settings.performance_mode.enabled;
        let body = feature_body(enabled)
            .child(section_title_text(t!("common.rules").to_string()))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "performance-mode-suggestion",
                        &self.inputs.performance_process,
                        SuggestionTarget::PerformanceMode,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-performance-mode-process"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_performance_mode_process(
                                        &self.settings.performance_mode,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process =
                                    app.inputs.performance_process.read(cx).value().to_string();
                                if can_add_performance_mode_process(
                                    &app.settings.performance_mode,
                                    &process,
                                ) {
                                    app.settings
                                        .performance_mode
                                        .rules
                                        .push(app.new_performance_mode_rule(&process));
                                    app.inputs.ensure_for_settings(window, cx, &app.settings);
                                    clear_input(&app.inputs.performance_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_performance_mode_rules(window, cx));

        let help = tooltip_lines(vec![
            t!("performance_mode.intro_1").to_string(),
            t!("performance_mode.intro_2").to_string(),
            t!("performance_mode.intro_3").to_string(),
            t!("common.power_plan_priority").to_string(),
            t!("common.power_plan_pause_priority").to_string(),
        ]);

        page_shell(Page::PerformanceMode, cx)
            .child(feature_toggle_switch_with_help(
                "performance-mode-enabled",
                t!("performance_mode.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.performance_mode.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(body, enabled))
            .into_any_element()
    }

    fn render_performance_mode_rules(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = rule_list();
        for (index, rule) in self.settings.performance_mode.rules.iter().enumerate() {
            let process = rule.process_name.clone();
            list = list.child(
                compact_rule_row(cx)
                    .child(rule_enable_checkbox(
                        format!("performance-mode-rule-enabled-{index}"),
                        rule.enabled,
                        cx.listener(move |app, checked, _, cx| {
                            if let Some(rule) = app.settings.performance_mode.rules.get_mut(index) {
                                rule.enabled = *checked;
                            }
                            cx.notify();
                        }),
                    ))
                    .child(self.process_rule_title(&process, cx))
                    .child(self.render_inline_power_plan_picker(
                        format!("performance-mode-plan-{index}"),
                        rule.power_plan_guid.clone(),
                        PowerPlanField::PerformanceModeRule(index),
                        window,
                        cx,
                    ))
                    .child(
                        danger_control_button(Button::new(SharedString::from(format!(
                            "remove-performance-mode-{index}"
                        ))))
                        .label(t!("common.remove").to_string())
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if index < app.settings.performance_mode.rules.len() {
                                app.settings.performance_mode.rules.remove(index);
                            }
                            app.editing_rule_title = None;
                            app.expanded_rule_cards.clear();
                            cx.notify();
                        }))
                        .into_any_element(),
                    ),
            );
        }
        list.into_any_element()
    }

    fn render_foreground_responsiveness_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .responsiveness_process
            .read(cx)
            .value()
            .to_string();
        let enabled = self.settings.foreground_responsiveness.enabled;
        let body = feature_body(enabled)
            .child(self.render_auto_balance_preset_selector(window, cx))
            .child(self.render_auto_balance_advanced_settings_toggle(cx))
            .when(
                self.settings
                    .foreground_responsiveness
                    .auto_balance_advanced_settings_enabled,
                |body| {
                    body.child(section_header(
                        &t!("responsiveness.auto_balance_advanced"),
                        t!("responsiveness.auto_balance_advanced_help").to_string(),
                    ))
                    .child(self.render_auto_balance_advanced_cards(
                        window,
                        cx,
                        &input_value,
                        enabled,
                    ))
                },
            );

        let help = tooltip_lines(vec![
            t!("responsiveness.intro_1").to_string(),
            t!("responsiveness.intro_2").to_string(),
            t!("responsiveness.intro_3").to_string(),
        ]);

        page_shell(Page::ForegroundResponsiveness, cx)
            .child(feature_toggle_switch_with_help(
                "foreground-responsiveness-enabled",
                t!("responsiveness.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.foreground_responsiveness.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(body, enabled))
            .into_any_element()
    }

    fn render_auto_io_priority_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self
            .settings
            .foreground_responsiveness
            .lower_background_io_priority_enabled;
        let priority = self
            .settings
            .foreground_responsiveness
            .lower_background_io_priority;
        self.render_dropdown_select(
            "responsiveness-background-io-priority",
            process_io_priority_label(priority),
            selected,
            DropdownSelectWidth::Standard,
            ProcessIoPriority::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for option in ProcessIoPriority::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "responsiveness-background-io-priority-option-{option:?}"
                            )),
                            process_io_priority_label(option),
                            priority == option,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings
                                .foreground_responsiveness
                                .lower_background_io_priority_enabled = true;
                            app.settings
                                .foreground_responsiveness
                                .lower_background_io_priority = option;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    fn render_auto_balance_preset_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = &self.settings.foreground_responsiveness;
        let selected_behavior = AutoBalanceBehavior::ALL
            .iter()
            .copied()
            .find(|behavior| auto_balance_matches_behavior(settings, *behavior));
        let dropdown = self.render_dropdown_select(
            "auto-balance-behavior",
            selected_behavior
                .map(auto_balance_behavior_label)
                .unwrap_or_else(|| "Custom".to_owned()),
            true,
            DropdownSelectWidth::Standard,
            AutoBalanceBehavior::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for behavior in AutoBalanceBehavior::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "auto-balance-behavior-option-{behavior:?}"
                            )),
                            auto_balance_behavior_label(behavior),
                            selected_behavior == Some(behavior),
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            apply_auto_balance_behavior(
                                &mut app.settings.foreground_responsiveness,
                                behavior,
                            );
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        setting_action_card_with_help(
            "auto-balance-preset",
            t!("responsiveness.auto_balance_preset").to_string(),
            t!("responsiveness.auto_balance_preset_help").to_string(),
            dropdown,
        )
        .into_any_element()
    }

    fn render_auto_balance_advanced_settings_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        setting_action_card_with_help(
            "auto-balance-advanced-settings-enabled",
            t!("responsiveness.auto_balance_advanced_settings").to_string(),
            t!("responsiveness.auto_balance_advanced_settings_help").to_string(),
            switch_toggle_action(
                "auto-balance-advanced-settings-toggle",
                self.settings
                    .foreground_responsiveness
                    .auto_balance_advanced_settings_enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings
                        .foreground_responsiveness
                        .auto_balance_advanced_settings_enabled = *checked;
                    cx.notify();
                }),
            ),
        )
        .into_any_element()
    }

    fn render_auto_balance_advanced_cards(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        input_value: &str,
        enabled: bool,
    ) -> AnyElement {
        let settings = &self.settings.foreground_responsiveness;
        let efficiency_action = if self.settings.eco_qos.enabled {
            value_pill(t!("responsiveness.background_efficiency_handled").to_string())
                .into_any_element()
        } else {
            setting_group_switch_action(
                "responsiveness-lower-background-toggle",
                settings.lower_background_apps,
                cx.listener(|app, checked, _, cx| {
                    app.settings.foreground_responsiveness.lower_background_apps = *checked;
                    cx.notify();
                }),
            )
        };
        let mut rows = vec![
            setting_group_with_help(
                SettingGroupTarget::AutoBalanceEfficiency,
                t!("responsiveness.auto_efficiency_adjustment").to_string(),
                t!("responsiveness.auto_efficiency_adjustment_help").to_string(),
                efficiency_action,
                self.is_setting_group_collapsed(SettingGroupTarget::AutoBalanceEfficiency),
                vec![
                    setting_group_action_row(
                        "responsiveness-auto-efficiency-level",
                        t!("responsiveness.auto_efficiency_level").to_string(),
                        self.render_efficiency_aggressiveness_picker(
                            self.settings.eco_qos.aggressiveness,
                            window,
                            cx,
                        ),
                        true,
                    )
                    .into_any_element(),
                    self.render_foreground_boost_selector(window, cx),
                ],
                cx,
            )
            .into_any_element(),
            setting_group_with_help(
                SettingGroupTarget::AutoBalanceIoPriority,
                t!("responsiveness.auto_io_priority").to_string(),
                t!("responsiveness.auto_io_priority_help").to_string(),
                setting_group_switch_action(
                    "responsiveness-lower-background-io-toggle",
                    settings.lower_background_io_priority_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings
                            .foreground_responsiveness
                            .lower_background_io_priority_enabled = *checked;
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(SettingGroupTarget::AutoBalanceIoPriority),
                vec![setting_group_action_row(
                    "responsiveness-auto-io-priority-level",
                    t!("responsiveness.auto_io_priority_level").to_string(),
                    self.render_auto_io_priority_selector(window, cx),
                    true,
                )
                .into_any_element()],
                cx,
            )
            .into_any_element(),
            setting_group_with_help(
                SettingGroupTarget::AutoBalanceAffinity,
                t!("responsiveness.auto_affinity_escalation").to_string(),
                t!("responsiveness.auto_affinity_escalation_help").to_string(),
                setting_group_switch_action(
                    "responsiveness-auto-affinity-escalation-switch",
                    settings.auto_balance_affinity_escalation_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings
                            .foreground_responsiveness
                            .auto_balance_affinity_escalation_enabled = *checked;
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(SettingGroupTarget::AutoBalanceAffinity),
                vec![
                    setting_group_action_row(
                        "responsiveness-auto-affinity-mode",
                        t!("responsiveness.auto_balance_affinity_mode").to_string(),
                        self.render_auto_balance_affinity_mode_selector(window, cx),
                        true,
                    )
                    .into_any_element(),
                    setting_group_action_row(
                        "responsiveness-auto-cpu-share-mode",
                        t!("responsiveness.dynamic_cpu_share").to_string(),
                        setting_group_switch_action(
                            "responsiveness-auto-cpu-share-mode-switch",
                            settings.lower_background_auto_cpu_percent,
                            cx.listener(|app, checked, _, cx| {
                                app.settings
                                    .foreground_responsiveness
                                    .lower_background_auto_cpu_percent = *checked;
                                cx.notify();
                            }),
                        ),
                        true,
                    )
                    .into_any_element(),
                    setting_group_stepper_row_u64(
                        "responsiveness-auto-cpu-percent",
                        t!("responsiveness.minimum_cpu_share").to_string(),
                        u64::from(settings.auto_balance_cpu_percent),
                        self.render_numeric_value(
                            NumericField::AutoBalanceCpuPercent,
                            format!("{}%", settings.auto_balance_cpu_percent),
                            settings.auto_balance_cpu_percent.to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            let current = u64::from(
                                app.settings
                                    .foreground_responsiveness
                                    .auto_balance_cpu_percent,
                            );
                            app.settings
                                .foreground_responsiveness
                                .auto_balance_cpu_percent = apply_u64_step(
                                current,
                                change,
                                AUTO_BALANCE_THRESHOLD_MIN_PERCENT,
                                AUTO_BALANCE_THRESHOLD_MAX_PERCENT,
                            ) as u8;
                            cx.notify();
                        }),
                    ),
                ],
                cx,
            )
            .into_any_element(),
            setting_group_with_help(
                SettingGroupTarget::AutoBalanceMemoryPriority,
                t!("responsiveness.auto_memory_priority").to_string(),
                t!("responsiveness.auto_memory_priority_help").to_string(),
                setting_group_switch_action(
                    "responsiveness-auto-memory-priority-switch",
                    settings.auto_balance_memory_priority_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings
                            .foreground_responsiveness
                            .auto_balance_memory_priority_enabled = *checked;
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(SettingGroupTarget::AutoBalanceMemoryPriority),
                vec![setting_group_action_row(
                    "responsiveness-auto-memory-priority-level",
                    t!("responsiveness.auto_balance_memory_priority_level").to_string(),
                    self.render_auto_balance_memory_priority_selector(window, cx),
                    true,
                )
                .into_any_element()],
                cx,
            )
            .into_any_element(),
            setting_group_with_help(
                SettingGroupTarget::AutoBalanceBehaviourTuning,
                t!("responsiveness.auto_balance_behaviour_tuning").to_string(),
                t!("responsiveness.auto_balance_behaviour_tuning_help").to_string(),
                div().into_any_element(),
                self.is_setting_group_collapsed(SettingGroupTarget::AutoBalanceBehaviourTuning),
                vec![
                    setting_group_stepper_row_u64(
                        "responsiveness-auto-total-threshold",
                        t!("responsiveness.auto_balance_total_threshold").to_string(),
                        u64::from(settings.auto_balance_total_threshold_percent),
                        self.render_numeric_value(
                            NumericField::AutoBalanceTotalThreshold,
                            format!("{}%", settings.auto_balance_total_threshold_percent),
                            settings.auto_balance_total_threshold_percent.to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            let current = u64::from(
                                app.settings
                                    .foreground_responsiveness
                                    .auto_balance_total_threshold_percent,
                            );
                            app.settings
                                .foreground_responsiveness
                                .auto_balance_total_threshold_percent = apply_u64_step(
                                current,
                                change,
                                AUTO_BALANCE_THRESHOLD_MIN_PERCENT,
                                AUTO_BALANCE_THRESHOLD_MAX_PERCENT,
                            )
                                as u8;
                            cx.notify();
                        }),
                    ),
                    setting_group_stepper_row_u64(
                        "responsiveness-auto-threshold",
                        t!("responsiveness.auto_balance_threshold").to_string(),
                        u64::from(settings.auto_balance_threshold_percent),
                        self.render_numeric_value(
                            NumericField::AutoBalanceThreshold,
                            format!("{}%", settings.auto_balance_threshold_percent),
                            settings.auto_balance_threshold_percent.to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            let current = u64::from(
                                app.settings
                                    .foreground_responsiveness
                                    .auto_balance_threshold_percent,
                            );
                            app.settings
                                .foreground_responsiveness
                                .auto_balance_threshold_percent = apply_u64_step(
                                current,
                                change,
                                AUTO_BALANCE_THRESHOLD_MIN_PERCENT,
                                AUTO_BALANCE_THRESHOLD_MAX_PERCENT,
                            )
                                as u8;
                            cx.notify();
                        }),
                    ),
                    setting_group_stepper_row_u64(
                        "responsiveness-auto-restore-threshold",
                        t!("responsiveness.auto_balance_restore_threshold").to_string(),
                        u64::from(settings.auto_balance_restore_threshold_percent),
                        self.render_numeric_value(
                            NumericField::AutoBalanceRestoreThreshold,
                            format!("{}%", settings.auto_balance_restore_threshold_percent),
                            settings.auto_balance_restore_threshold_percent.to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            let current = u64::from(
                                app.settings
                                    .foreground_responsiveness
                                    .auto_balance_restore_threshold_percent,
                            );
                            app.settings
                                .foreground_responsiveness
                                .auto_balance_restore_threshold_percent = apply_u64_step(
                                current,
                                change,
                                AUTO_BALANCE_THRESHOLD_MIN_PERCENT,
                                AUTO_BALANCE_THRESHOLD_MAX_PERCENT,
                            )
                                as u8;
                            cx.notify();
                        }),
                    ),
                    setting_group_stepper_row_u64(
                        "responsiveness-auto-sustain",
                        t!("responsiveness.auto_balance_sustain").to_string(),
                        settings.auto_balance_sustain_seconds,
                        self.render_numeric_value(
                            NumericField::AutoBalanceSustain,
                            format!("{} sec", settings.auto_balance_sustain_seconds),
                            settings.auto_balance_sustain_seconds.to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            app.settings
                                .foreground_responsiveness
                                .auto_balance_sustain_seconds = apply_u64_step(
                                app.settings
                                    .foreground_responsiveness
                                    .auto_balance_sustain_seconds,
                                change,
                                AUTO_BALANCE_SECONDS_MIN,
                                AUTO_BALANCE_SECONDS_MAX,
                            );
                            cx.notify();
                        }),
                    ),
                    setting_group_stepper_row_u64(
                        "responsiveness-auto-minimum-restraint",
                        t!("responsiveness.auto_balance_minimum_restraint").to_string(),
                        settings.auto_balance_minimum_restraint_seconds,
                        self.render_numeric_value(
                            NumericField::AutoBalanceMinimumRestraint,
                            format!("{} sec", settings.auto_balance_minimum_restraint_seconds),
                            settings.auto_balance_minimum_restraint_seconds.to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            app.settings
                                .foreground_responsiveness
                                .auto_balance_minimum_restraint_seconds = apply_u64_step(
                                app.settings
                                    .foreground_responsiveness
                                    .auto_balance_minimum_restraint_seconds,
                                change,
                                AUTO_BALANCE_SECONDS_MIN,
                                AUTO_BALANCE_SECONDS_MAX,
                            );
                            cx.notify();
                        }),
                    ),
                    setting_group_stepper_row_u64(
                        "responsiveness-auto-cooldown",
                        t!("responsiveness.auto_balance_cooldown").to_string(),
                        settings.auto_balance_cooldown_seconds,
                        self.render_numeric_value(
                            NumericField::AutoBalanceCooldown,
                            format!("{} sec", settings.auto_balance_cooldown_seconds),
                            settings.auto_balance_cooldown_seconds.to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            app.settings
                                .foreground_responsiveness
                                .auto_balance_cooldown_seconds = apply_u64_step(
                                app.settings
                                    .foreground_responsiveness
                                    .auto_balance_cooldown_seconds,
                                change,
                                AUTO_BALANCE_SECONDS_MIN,
                                AUTO_BALANCE_SECONDS_MAX,
                            );
                            cx.notify();
                        }),
                    ),
                ],
                cx,
            )
            .into_any_element(),
        ];

        let exclusion_editor = v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "responsiveness-exclusion-suggestion",
                        &self.inputs.responsiveness_process,
                        SuggestionTarget::Responsiveness,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-responsiveness-exclusion"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_responsiveness_exclusion(
                                        &self.settings.foreground_responsiveness,
                                        input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process = app
                                    .inputs
                                    .responsiveness_process
                                    .read(cx)
                                    .value()
                                    .to_string();
                                if can_add_responsiveness_exclusion(
                                    &app.settings.foreground_responsiveness,
                                    &process,
                                ) {
                                    app.settings
                                        .foreground_responsiveness
                                        .auto_balance_exclusions
                                        .push(new_process_exclusion_rule(&process));
                                    clear_input(&app.inputs.responsiveness_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_responsiveness_exclusions(cx));
        rows.push(
            setting_group_with_help(
                SettingGroupTarget::AutoBalanceExclusions,
                t!("responsiveness.auto_balance_exclusions").to_string(),
                t!("responsiveness.auto_balance_exclusions_help").to_string(),
                div().into_any_element(),
                self.is_setting_group_collapsed(SettingGroupTarget::AutoBalanceExclusions),
                vec![exclusion_editor.into_any_element()],
                cx,
            )
            .into_any_element(),
        );

        v_flex().gap_2().children(rows).into_any_element()
    }

    fn render_foreground_boost_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.settings.foreground_responsiveness.foreground_boost;
        let boost_enabled = self.settings.foreground_responsiveness.boost_foreground_app;
        let boost_options: [Option<ForegroundBoostPriority>; 4] = [
            None,
            Some(ForegroundBoostPriority::ALL[0]),
            Some(ForegroundBoostPriority::ALL[1]),
            Some(ForegroundBoostPriority::ALL[2]),
        ];
        let dropdown = self.render_dropdown_select(
            "foreground-boost-priority-select",
            if boost_enabled {
                foreground_boost_priority_label(selected)
            } else {
                t!("common.none").to_string()
            },
            true,
            DropdownSelectWidth::Standard,
            boost_options.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for option in boost_options {
                    let selected_option = match option {
                        Some(priority) => boost_enabled && selected == priority,
                        None => !boost_enabled,
                    };
                    let label = option
                        .map(foreground_boost_priority_label)
                        .unwrap_or_else(|| t!("common.none").to_string());
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("foreground-boost-option-{option:?}")),
                            label,
                            selected_option,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            match option {
                                Some(priority) => {
                                    app.settings.foreground_responsiveness.boost_foreground_app =
                                        true;
                                    app.settings.foreground_responsiveness.foreground_boost =
                                        priority;
                                }
                                None => {
                                    app.settings.foreground_responsiveness.boost_foreground_app =
                                        false;
                                }
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );
        setting_action_card_with_help(
            "foreground-boost-priority",
            t!("responsiveness.foreground_boost").to_string(),
            t!("responsiveness.foreground_boost_help").to_string(),
            dropdown,
        )
        .into_any_element()
    }

    fn render_auto_balance_affinity_mode_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self
            .settings
            .foreground_responsiveness
            .auto_balance_affinity_mode;
        self.render_dropdown_select(
            "responsiveness-auto-affinity-mode",
            efficiency_cpu_restriction_mode_label(selected),
            true,
            DropdownSelectWidth::Standard,
            EcoQosCpuRestrictionMode::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for mode in EcoQosCpuRestrictionMode::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "responsiveness-auto-affinity-mode-option-{mode:?}"
                            )),
                            efficiency_cpu_restriction_mode_label(mode),
                            selected == mode,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings
                                .foreground_responsiveness
                                .auto_balance_affinity_mode = mode;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    fn render_auto_balance_memory_priority_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self
            .settings
            .foreground_responsiveness
            .auto_balance_memory_priority;
        self.render_dropdown_select(
            "responsiveness-auto-memory-priority-level",
            process_memory_priority_label(selected),
            true,
            DropdownSelectWidth::Standard,
            ProcessMemoryPriority::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for priority in ProcessMemoryPriority::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "responsiveness-auto-memory-priority-option-{priority:?}"
                            )),
                            process_memory_priority_label(priority),
                            selected == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings
                                .foreground_responsiveness
                                .auto_balance_memory_priority = priority;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    #[allow(dead_code)]
    fn render_responsiveness_rules(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = rule_list();
        for (index, rule) in self
            .settings
            .foreground_responsiveness
            .rules
            .iter()
            .enumerate()
        {
            let process = rule.process_name.clone();
            let adjusted = responsiveness::contains_process(
                &self.foreground_responsiveness_status.adjusted_apps,
                &process,
            );
            let indicator = if responsiveness::is_builtin_excluded(&process) {
                (
                    t!("affinity.indicator.protected").to_string(),
                    settings_card_hover_color(),
                    accent_color(),
                )
            } else if adjusted {
                (
                    t!("responsiveness.indicator_lowered").to_string(),
                    success_bg_color(),
                    success_text_color(),
                )
            } else if self.foreground_responsiveness_status.enabled {
                (
                    t!("affinity.indicator.ready").to_string(),
                    panel_active_color(),
                    muted_text_color(),
                )
            } else {
                (
                    t!("affinity.indicator.off").to_string(),
                    panel_active_color(),
                    dim_text_color(),
                )
            };
            let card_target = RuleCardTarget::Responsiveness(process.clone());
            let collapsed = self.is_rule_card_collapsed(&card_target);
            let mut card = rule_card(
                self.process_rule_title(&process, cx),
                rule_enable_checkbox(
                    format!("responsiveness-rule-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        if let Some(rule) =
                            app.settings.foreground_responsiveness.rules.get_mut(index)
                        {
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
                    .child(rule_card_body_row(vec![rule_action_row(
                        format!("responsiveness-rule-status-{index}"),
                        t!("common.status").to_string(),
                        status_pill(indicator.0, indicator.1, indicator.2).into_any_element(),
                    )
                    .into_any_element()]))
                    .child(rule_card_body_row(vec![self.render_priority_selector(
                        index,
                        rule.priority,
                        window,
                        cx,
                    )]))
                    .child(rule_card_body_action(
                        danger_control_button(Button::new(SharedString::from(format!(
                            "remove-responsiveness-{index}"
                        ))))
                        .label(t!("common.remove").to_string())
                        .on_click(cx.listener({
                            let card_target = card_target.clone();
                            move |app, _, _, cx| {
                                if index < app.settings.foreground_responsiveness.rules.len() {
                                    app.settings.foreground_responsiveness.rules.remove(index);
                                }
                                app.expanded_rule_cards.remove(&card_target);
                                cx.notify();
                            }
                        }))
                        .into_any_element(),
                    ));
            }
            list = list.child(card);
        }
        if self.settings.foreground_responsiveness.rules.is_empty() {
            list = list.child(text_muted(t!("responsiveness.no_rules").to_string()));
        }
        list.into_any_element()
    }

    fn render_responsiveness_exclusions(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut list = v_flex().gap_2();
        for (index, rule) in self
            .settings
            .foreground_responsiveness
            .auto_balance_exclusions
            .iter()
            .enumerate()
        {
            let process = rule.process_name.clone();
            list = list.child(
                compact_rule_row(cx)
                    .child(rule_enable_checkbox(
                        format!("responsiveness-exclusion-enabled-{index}"),
                        rule.enabled,
                        cx.listener(move |app, checked, _, cx| {
                            if let Some(rule) = app
                                .settings
                                .foreground_responsiveness
                                .auto_balance_exclusions
                                .get_mut(index)
                            {
                                rule.enabled = *checked;
                            }
                            cx.notify();
                        }),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(160.0))
                            .text_size(px(RULE_TITLE_TEXT_SIZE))
                            .line_height(px(RULE_TITLE_LINE_HEIGHT))
                            .truncate()
                            .child(process),
                    )
                    .child(
                        danger_control_button(Button::new(SharedString::from(format!(
                            "remove-responsiveness-exclusion-{index}"
                        ))))
                        .label(t!("common.remove").to_string())
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if index
                                < app
                                    .settings
                                    .foreground_responsiveness
                                    .auto_balance_exclusions
                                    .len()
                            {
                                app.settings
                                    .foreground_responsiveness
                                    .auto_balance_exclusions
                                    .remove(index);
                            }
                            cx.notify();
                        })),
                    ),
            );
        }
        if self
            .settings
            .foreground_responsiveness
            .auto_balance_exclusions
            .is_empty()
        {
            list = list.child(text_muted(
                t!("responsiveness.no_auto_balance_exclusions").to_string(),
            ));
        }
        list.into_any_element()
    }

    #[allow(dead_code)]
    fn render_priority_selector(
        &self,
        index: usize,
        selected_priority: ProcessPriority,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let dropdown = self.render_dropdown_select(
            format!("responsiveness-priority-{index}"),
            process_priority_label(selected_priority),
            true,
            DropdownSelectWidth::Standard,
            ProcessPriority::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for priority in ProcessPriority::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "responsiveness-priority-{index}-option-{priority:?}"
                            )),
                            process_priority_label(priority),
                            selected_priority == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if let Some(rule) =
                                app.settings.foreground_responsiveness.rules.get_mut(index)
                            {
                                rule.priority = priority;
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );
        rule_action_row(
            format!("responsiveness-priority-row-{index}"),
            t!("responsiveness.background_priority").to_string(),
            dropdown,
        )
        .into_any_element()
    }

    fn render_io_priority_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let input_value = self.inputs.io_priority_process.read(cx).value().to_string();
        let enabled = self.settings.io_priority.enabled;
        let help = tooltip_lines(vec![
            t!("io_priority.intro_1").to_string(),
            t!("io_priority.intro_2").to_string(),
        ]);
        let body = feature_body(enabled)
            .child(setting_action_card_with_help(
                "io-priority-exclude-foreground",
                t!("io_priority.exclude_foreground").to_string(),
                t!("io_priority.exclude_foreground_help").to_string(),
                switch_toggle_action(
                    "io-priority-exclude-foreground-toggle",
                    self.settings.io_priority.exclude_foreground_app,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.io_priority.exclude_foreground_app = *checked;
                        cx.notify();
                    }),
                ),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "io-priority-process-suggestion",
                        &self.inputs.io_priority_process,
                        SuggestionTarget::IoPriority,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-io-priority-rule"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_io_priority_process(
                                        &self.settings.io_priority,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process =
                                    app.inputs.io_priority_process.read(cx).value().to_string();
                                if can_add_io_priority_process(&app.settings.io_priority, &process)
                                {
                                    app.settings
                                        .io_priority
                                        .rules
                                        .push(new_io_priority_rule(&process));
                                    clear_input(&app.inputs.io_priority_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_io_priority_rules(window, cx));

        page_shell(Page::IoPriority, cx)
            .child(feature_toggle_switch_with_help(
                "io-priority-enabled",
                t!("io_priority.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.io_priority.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(body, enabled))
            .into_any_element()
    }

    fn render_io_priority_rules(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let mut list = rule_list();
        for (index, rule) in self.settings.io_priority.rules.iter().enumerate() {
            let process = rule.process_name.clone();
            let adjusted =
                io_priority_contains_process(&self.io_priority_status.adjusted_apps, &process);
            let indicator = if io_priority::is_builtin_excluded(&process) {
                (
                    t!("affinity.indicator.protected").to_string(),
                    settings_card_hover_color(),
                    accent_color(),
                )
            } else if adjusted {
                (
                    t!("io_priority.indicator_adjusted").to_string(),
                    success_bg_color(),
                    success_text_color(),
                )
            } else if self.io_priority_status.enabled {
                (
                    t!("affinity.indicator.ready").to_string(),
                    panel_active_color(),
                    muted_text_color(),
                )
            } else {
                (
                    t!("affinity.indicator.off").to_string(),
                    panel_active_color(),
                    dim_text_color(),
                )
            };
            let card_target = RuleCardTarget::IoPriority(process.clone());
            let collapsed = self.is_rule_card_collapsed(&card_target);
            let mut card = rule_card(
                self.process_rule_title(&process, cx),
                rule_enable_checkbox(
                    format!("io-priority-rule-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        if let Some(rule) = app.settings.io_priority.rules.get_mut(index) {
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
                    .child(rule_card_body_row(vec![rule_action_row(
                        format!("io-priority-rule-status-{index}"),
                        t!("common.status").to_string(),
                        status_pill(indicator.0, indicator.1, indicator.2).into_any_element(),
                    )
                    .into_any_element()]))
                    .child(rule_card_body_row(vec![self.render_io_priority_selector(
                        index,
                        rule.priority,
                        window,
                        cx,
                    )]))
                    .child(rule_card_body_action(
                        danger_control_button(Button::new(SharedString::from(format!(
                            "remove-io-priority-{index}"
                        ))))
                        .label(t!("common.remove").to_string())
                        .on_click(cx.listener({
                            let card_target = card_target.clone();
                            move |app, _, _, cx| {
                                if index < app.settings.io_priority.rules.len() {
                                    app.settings.io_priority.rules.remove(index);
                                }
                                app.expanded_rule_cards.remove(&card_target);
                                cx.notify();
                            }
                        }))
                        .into_any_element(),
                    ));
            }
            list = list.child(card);
        }
        if self.settings.io_priority.rules.is_empty() {
            list = list.child(text_muted(t!("io_priority.no_rules").to_string()));
        }
        list.into_any_element()
    }

    fn render_io_priority_selector(
        &self,
        index: usize,
        selected_priority: ProcessIoPriority,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let dropdown = self.render_dropdown_select(
            format!("io-priority-{index}"),
            process_io_priority_label(selected_priority),
            true,
            DropdownSelectWidth::Standard,
            ProcessIoPriority::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for priority in ProcessIoPriority::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("io-priority-{index}-option-{priority:?}")),
                            process_io_priority_label(priority),
                            selected_priority == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if let Some(rule) = app.settings.io_priority.rules.get_mut(index) {
                                rule.priority = priority;
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );
        rule_action_row(
            format!("io-priority-row-{index}"),
            t!("io_priority.priority").to_string(),
            dropdown,
        )
        .into_any_element()
    }

    fn render_memory_priority_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .memory_priority_process
            .read(cx)
            .value()
            .to_string();
        let enabled = self.settings.memory_priority.enabled;
        let help = tooltip_lines(vec![
            t!("memory_priority.intro_1").to_string(),
            t!("memory_priority.intro_2").to_string(),
        ]);
        let body = feature_body(enabled)
            .child(setting_action_card_with_help(
                "memory-priority-exclude-foreground",
                t!("memory_priority.exclude_foreground").to_string(),
                t!("memory_priority.exclude_foreground_help").to_string(),
                switch_toggle_action(
                    "memory-priority-exclude-foreground-toggle",
                    self.settings.memory_priority.exclude_foreground_app,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.memory_priority.exclude_foreground_app = *checked;
                        cx.notify();
                    }),
                ),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "memory-priority-process-suggestion",
                        &self.inputs.memory_priority_process,
                        SuggestionTarget::MemoryPriority,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-memory-priority-rule"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_memory_priority_process(
                                        &self.settings.memory_priority,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process = app
                                    .inputs
                                    .memory_priority_process
                                    .read(cx)
                                    .value()
                                    .to_string();
                                if can_add_memory_priority_process(
                                    &app.settings.memory_priority,
                                    &process,
                                ) {
                                    app.settings
                                        .memory_priority
                                        .rules
                                        .push(new_memory_priority_rule(&process));
                                    clear_input(&app.inputs.memory_priority_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_memory_priority_rules(window, cx));

        page_shell(Page::MemoryPriority, cx)
            .child(feature_toggle_switch_with_help(
                "memory-priority-enabled",
                t!("memory_priority.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.memory_priority.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(body, enabled))
            .into_any_element()
    }

    fn render_memory_priority_rules(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = rule_list();
        for (index, rule) in self.settings.memory_priority.rules.iter().enumerate() {
            let process = rule.process_name.clone();
            list = list.child(
                compact_rule_row(cx)
                    .child(rule_enable_checkbox(
                        format!("memory-priority-rule-enabled-{index}"),
                        rule.enabled,
                        cx.listener(move |app, checked, _, cx| {
                            if let Some(rule) = app.settings.memory_priority.rules.get_mut(index) {
                                rule.enabled = *checked;
                            }
                            cx.notify();
                        }),
                    ))
                    .child(self.process_rule_title(&process, cx))
                    .child(self.render_memory_priority_selector(index, rule.priority, window, cx))
                    .child(
                        danger_control_button(Button::new(SharedString::from(format!(
                            "remove-memory-priority-{index}"
                        ))))
                        .label(t!("common.remove").to_string())
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if index < app.settings.memory_priority.rules.len() {
                                app.settings.memory_priority.rules.remove(index);
                            }
                            app.editing_rule_title = None;
                            app.expanded_rule_cards.clear();
                            cx.notify();
                        }))
                        .into_any_element(),
                    ),
            );
        }
        if self.settings.memory_priority.rules.is_empty() {
            list = list.child(text_muted(t!("memory_priority.no_rules").to_string()));
        }
        list.into_any_element()
    }

    fn render_memory_priority_selector(
        &self,
        index: usize,
        selected_priority: ProcessMemoryPriority,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let dropdown = self.render_dropdown_select(
            format!("memory-priority-{index}"),
            process_memory_priority_label(selected_priority),
            true,
            DropdownSelectWidth::Standard,
            ProcessMemoryPriority::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for priority in ProcessMemoryPriority::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "memory-priority-{index}-option-{priority:?}"
                            )),
                            process_memory_priority_label(priority),
                            selected_priority == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if let Some(rule) = app.settings.memory_priority.rules.get_mut(index) {
                                rule.priority = priority;
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );
        dropdown
    }

    fn render_smart_trim_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let input_value = self
            .inputs
            .smart_trim_exclusion
            .read(cx)
            .value()
            .to_string();
        let settings = &self.settings.smart_trim;
        let enabled = settings.enabled;

        let body = feature_body(enabled)
            .child(setting_group_action_row(
                "smart-trim-check-interval",
                t!("smart_trim.check_interval").to_string(),
                self.render_numeric_value(
                    NumericField::SmartTrimCheckIntervalMinutes,
                    format!("{} mins", settings.check_interval_minutes),
                    settings.check_interval_minutes.to_string(),
                    cx,
                ),
                false,
            ))
            .child(feature_toggle_switch_with_help(
                "smart-trim-foreground",
                t!("smart_trim.focus_detection").to_string(),
                t!("smart_trim.focus_detection_help").to_string(),
                settings.exclude_foreground_app,
                cx.listener(|app, checked, _, cx| {
                    app.settings.smart_trim.exclude_foreground_app = *checked;
                    cx.notify();
                }),
            ))
            .child(feature_toggle_switch_with_help(
                "smart-trim-working-sets",
                t!("smart_trim.trim_working_sets").to_string(),
                t!("smart_trim.trim_working_sets_help").to_string(),
                settings.trim_working_sets,
                cx.listener(|app, checked, _, cx| {
                    app.settings.smart_trim.trim_working_sets = *checked;
                    cx.notify();
                }),
            ))
            .child(setting_group_action_row(
                "smart-trim-memory-threshold",
                t!("smart_trim.memory_threshold").to_string(),
                self.render_numeric_value(
                    NumericField::SmartTrimMemoryLoadThreshold,
                    format!("{}%", settings.system_memory_load_threshold_percent),
                    settings.system_memory_load_threshold_percent.to_string(),
                    cx,
                ),
                false,
            ))
            .child(setting_group_action_row(
                "smart-trim-working-set-threshold",
                t!("smart_trim.working_set_threshold").to_string(),
                self.render_numeric_value(
                    NumericField::SmartTrimWorkingSetThreshold,
                    format!("{} MB", settings.process_working_set_threshold_mb),
                    settings.process_working_set_threshold_mb.to_string(),
                    cx,
                ),
                true,
            ))
            .child(setting_group_action_row(
                "smart-trim-cpu-idle-threshold",
                t!("smart_trim.cpu_idle_threshold").to_string(),
                self.render_numeric_value(
                    NumericField::SmartTrimCpuIdleThreshold,
                    format!("{}%", settings.process_cpu_idle_threshold_percent),
                    settings.process_cpu_idle_threshold_percent.to_string(),
                    cx,
                ),
                true,
            ))
            .child(setting_group_action_row(
                "smart-trim-idle-time",
                t!("smart_trim.idle_time").to_string(),
                self.render_numeric_value(
                    NumericField::SmartTrimIdleSeconds,
                    ui::duration_label(settings.process_idle_seconds),
                    settings.process_idle_seconds.to_string(),
                    cx,
                ),
                true,
            ))
            .child(setting_group_action_row(
                "smart-trim-cooldown",
                t!("smart_trim.cooldown").to_string(),
                self.render_numeric_value(
                    NumericField::SmartTrimCooldownSeconds,
                    ui::duration_label(settings.trim_cooldown_seconds),
                    settings.trim_cooldown_seconds.to_string(),
                    cx,
                ),
                true,
            ))
            .child(feature_toggle_switch_with_help(
                "smart-trim-purge-standby-list",
                t!("smart_trim.purge_standby_list").to_string(),
                t!("smart_trim.purge_standby_list_help").to_string(),
                settings.purge_standby_list,
                cx.listener(|app, checked, _, cx| {
                    app.settings.smart_trim.purge_standby_list = *checked;
                    cx.notify();
                }),
            ))
            .child(feature_toggle_switch_with_help(
                "smart-trim-purge-system-file-cache",
                t!("smart_trim.purge_system_file_cache").to_string(),
                t!("smart_trim.purge_system_file_cache_help").to_string(),
                settings.purge_system_file_cache,
                cx.listener(|app, checked, _, cx| {
                    app.settings.smart_trim.purge_system_file_cache = *checked;
                    cx.notify();
                }),
            ))
            .child(feature_toggle_switch_with_help(
                "smart-trim-purge-performance-mode",
                t!("smart_trim.only_purge_performance_mode").to_string(),
                t!("smart_trim.only_purge_performance_mode_help").to_string(),
                settings.purge_only_in_performance_mode,
                cx.listener(|app, checked, _, cx| {
                    app.settings.smart_trim.purge_only_in_performance_mode = *checked;
                    cx.notify();
                }),
            ))
            .child(setting_group_action_row(
                "smart-trim-purge-free-ram-threshold",
                t!("smart_trim.purge_free_ram_threshold").to_string(),
                self.render_numeric_value(
                    NumericField::SmartTrimPurgeFreeRamThreshold,
                    format!("{} MB", settings.purge_free_ram_threshold_mb),
                    settings.purge_free_ram_threshold_mb.to_string(),
                    cx,
                ),
                true,
            ))
            .child(
                h_flex().w_full().items_center().justify_end().child(
                    primary_control_button(Button::new("smart-trim-now"), cx)
                        .label(t!("smart_trim.trim_now").to_string())
                        .disabled(!enabled)
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.background_automation.request_smart_trim_now();
                            app.status_message = t!("smart_trim.trim_now_requested").to_string();
                            cx.notify();
                        })),
                ),
            )
            .child(stat_grid(vec![
                (
                    t!("smart_trim.status").to_string(),
                    self.smart_trim_status.message.clone(),
                ),
                (
                    t!("smart_trim.memory_load").to_string(),
                    self.smart_trim_status
                        .memory_load_percent
                        .map(|percent| format!("{percent}%"))
                        .unwrap_or_else(|| t!("common.unknown").to_string()),
                ),
                (
                    t!("smart_trim.free_ram_excluding_cache").to_string(),
                    self.smart_trim_status
                        .free_ram_excluding_cache_mb
                        .map(|mb| format!("{mb} MB"))
                        .unwrap_or_else(|| t!("common.unknown").to_string()),
                ),
                (
                    t!("smart_trim.trimmed_processes").to_string(),
                    self.smart_trim_status.trimmed_processes.to_string(),
                ),
                (
                    t!("smart_trim.purged_standby_list").to_string(),
                    yes_no_label(self.smart_trim_status.purged_standby_list),
                ),
                (
                    t!("smart_trim.purged_system_file_cache").to_string(),
                    yes_no_label(self.smart_trim_status.purged_system_file_cache),
                ),
                (
                    t!("smart_trim.candidate_processes").to_string(),
                    self.smart_trim_status.candidate_processes.to_string(),
                ),
                (
                    t!("smart_trim.scanned_processes").to_string(),
                    self.smart_trim_status.scanned_processes.to_string(),
                ),
                (
                    t!("smart_trim.skipped_processes").to_string(),
                    self.smart_trim_status.skipped_processes.to_string(),
                ),
                (
                    t!("smart_trim.failed_actions").to_string(),
                    self.smart_trim_status.failed_processes.to_string(),
                ),
                (
                    t!("common.last_failure").to_string(),
                    self.smart_trim_status
                        .last_error
                        .clone()
                        .unwrap_or_else(|| t!("common.none").to_string()),
                ),
            ]))
            .child(section_header(
                &t!("smart_trim.exclusions"),
                t!("smart_trim.exclusions_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "smart-trim-exclusion",
                        &self.inputs.smart_trim_exclusion,
                        SuggestionTarget::SmartTrim,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-smart-trim-exclusion"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_smart_trim_exclusion(
                                        &self.settings.smart_trim,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process =
                                    app.inputs.smart_trim_exclusion.read(cx).value().to_string();
                                if can_add_smart_trim_exclusion(&app.settings.smart_trim, &process)
                                {
                                    app.settings
                                        .smart_trim
                                        .exclusions
                                        .push(new_process_exclusion_rule(&process));
                                    clear_input(&app.inputs.smart_trim_exclusion, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_smart_trim_exclusions(cx));

        page_shell(Page::SmartTrim, cx)
            .child(feature_toggle_switch_with_help(
                "smart-trim-enabled",
                t!("smart_trim.enable").to_string(),
                tooltip_lines(vec![
                    t!("smart_trim.intro_1").to_string(),
                    t!("smart_trim.intro_2").to_string(),
                    t!("smart_trim.intro_3").to_string(),
                ]),
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.smart_trim.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(body, enabled))
            .into_any_element()
    }

    fn render_smart_trim_exclusions(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut list = v_flex().gap_2();
        for (index, rule) in self.settings.smart_trim.exclusions.iter().enumerate() {
            let process = rule.process_name.clone();
            list = list.child(
                compact_rule_row(cx)
                    .child(rule_enable_checkbox(
                        format!("smart-trim-exclusion-enabled-{index}"),
                        rule.enabled,
                        cx.listener(move |app, checked, _, cx| {
                            if let Some(rule) = app.settings.smart_trim.exclusions.get_mut(index) {
                                rule.enabled = *checked;
                            }
                            cx.notify();
                        }),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(160.0))
                            .text_size(px(RULE_TITLE_TEXT_SIZE))
                            .line_height(px(RULE_TITLE_LINE_HEIGHT))
                            .truncate()
                            .child(process),
                    )
                    .child(
                        danger_control_button(Button::new(SharedString::from(format!(
                            "remove-smart-trim-exclusion-{index}"
                        ))))
                        .label(t!("common.remove").to_string())
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if index < app.settings.smart_trim.exclusions.len() {
                                app.settings.smart_trim.exclusions.remove(index);
                            }
                            cx.notify();
                        })),
                    ),
            );
        }
        if self.settings.smart_trim.exclusions.is_empty() {
            list = list.child(text_muted(t!("smart_trim.no_exclusions").to_string()));
        }
        list.into_any_element()
    }

    fn render_affinity_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let input_value = self.inputs.affinity_process.read(cx).value().to_string();
        let enabled = self.settings.cpu_affinity.enabled;
        let body = feature_body(enabled)
            .child(feature_toggle_switch_with_help(
                "cpu-affinity-foreground",
                t!("affinity.focus_detection").to_string(),
                t!("affinity.focus_detection_help").to_string(),
                self.settings.cpu_affinity.exclude_foreground_app,
                cx.listener(|app, checked, _, cx| {
                    app.settings.cpu_affinity.exclude_foreground_app = *checked;
                    cx.notify();
                }),
            ))
            .child(section_header(
                &t!("affinity.rules"),
                t!("affinity.rules_help").to_string(),
            ))
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
                        primary_control_button(Button::new("add-affinity-process"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_affinity_process(
                                        &self.settings.cpu_affinity,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process =
                                    app.inputs.affinity_process.read(cx).value().to_string();
                                if can_add_affinity_process(&app.settings.cpu_affinity, &process) {
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
            .child(self.render_affinity_rules(window, cx));

        let help = tooltip_lines(vec![
            t!("affinity.intro_1").to_string(),
            t!("affinity.intro_2").to_string(),
            t!("affinity.intro_3").to_string(),
        ]);

        page_shell(Page::CpuAffinity, cx)
            .child(feature_toggle_switch_with_help(
                "cpu-affinity-enabled",
                t!("affinity.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.cpu_affinity.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(body, enabled))
            .into_any_element()
    }

    fn render_affinity_rules(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let mut list = rule_list();
        for (index, rule) in self.settings.cpu_affinity.rules.iter().enumerate() {
            let process = rule.process_name.clone();
            let indicator = affinity_indicator(&self.cpu_affinity_status, &process);
            let card_target = RuleCardTarget::Affinity(process.clone());
            let collapsed = self.is_rule_card_collapsed(&card_target);
            let mut card = rule_card(
                self.process_rule_title(&process, cx),
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
                card =
                    card.child(rule_card_body_row(vec![rule_action_row(
                        format!("affinity-rule-status-{index}"),
                        t!("common.status").to_string(),
                        h_flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .min_w(px(0.0))
                            .flex_wrap()
                            .child(status_pill(indicator.label, indicator.bg, indicator.fg))
                            .child(text_muted(indicator.hover))
                            .into_any_element(),
                    )
                    .into_any_element()]))
                        .child(rule_card_body_row(vec![
                            self.render_affinity_mode_selector(index, rule.mode, window, cx)
                        ]))
                        .when(rule.mode != CpuAffinityMode::EfficiencyOff, |card| {
                            card.child(rule_card_body_row(vec![self
                                .render_affinity_core_selector(index, rule.core_mask, window, cx)]))
                        })
                        .child(rule_card_body_action(
                            danger_control_button(Button::new(SharedString::from(format!(
                                "remove-affinity-{index}"
                            ))))
                            .label(t!("common.remove").to_string())
                            .on_click(cx.listener({
                                let card_target = card_target.clone();
                                move |app, _, _, cx| {
                                    if index < app.settings.cpu_affinity.rules.len() {
                                        app.settings.cpu_affinity.rules.remove(index);
                                    }
                                    app.expanded_rule_cards.remove(&card_target);
                                    cx.notify();
                                }
                            }))
                            .into_any_element(),
                        ));
            }
            list = list.child(card);
        }
        if self.settings.cpu_affinity.rules.is_empty() {
            list = list.child(text_muted(t!("affinity.no_rules").to_string()));
        }
        list.into_any_element()
    }

    fn render_affinity_mode_selector(
        &self,
        index: usize,
        selected_mode: CpuAffinityMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let dropdown = self.render_dropdown_select(
            format!("affinity-mode-{index}"),
            cpu_affinity_mode_label(selected_mode),
            true,
            DropdownSelectWidth::Standard,
            CpuAffinityMode::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for mode in CpuAffinityMode::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("affinity-mode-{index}-option-{mode:?}")),
                            cpu_affinity_mode_label(mode),
                            selected_mode == mode,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if let Some(rule) = app.settings.cpu_affinity.rules.get_mut(index) {
                                rule.mode = mode;
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );
        rule_action_row(
            format!("affinity-mode-row-{index}"),
            t!("affinity.mode").to_string(),
            dropdown,
        )
        .into_any_element()
    }

    fn render_affinity_core_selector(
        &self,
        index: usize,
        core_mask: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let processors = affinity::logical_processors();
        let all_mask = affinity_processors_mask(&processors);
        let performance_mask =
            affinity_processors_kind_mask(&processors, LogicalProcessorKind::Performance);
        let efficiency_mask =
            affinity_processors_kind_mask(&processors, LogicalProcessorKind::Efficiency);
        let no_smt_mask = affinity_processors_no_smt_mask(&processors);

        let preset_options = vec![
            (t!("affinity.all").to_string(), all_mask, all_mask != 0),
            (
                t!("affinity.p_cores").to_string(),
                performance_mask,
                performance_mask != 0,
            ),
            (
                t!("affinity.e_cores").to_string(),
                efficiency_mask,
                efficiency_mask != 0,
            ),
            (
                t!("affinity.no_smt").to_string(),
                no_smt_mask,
                no_smt_mask != 0 && no_smt_mask != all_mask,
            ),
        ];
        let selected_preset_label = preset_options
            .iter()
            .find(|(_, mask, enabled)| *enabled && core_mask == *mask)
            .map(|(label, _, _)| label.clone())
            .unwrap_or_else(|| "Custom".to_owned());
        let preset_count = preset_options.len();
        let presets_dropdown = self.render_dropdown_select(
            format!("affinity-core-preset-{index}"),
            selected_preset_label,
            true,
            DropdownSelectWidth::Standard,
            preset_count,
            window,
            cx,
            move |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for (option_index, (label, mask, enabled)) in preset_options.into_iter().enumerate()
                {
                    let row = dropdown_option_row(
                        SharedString::from(format!(
                            "affinity-core-preset-{index}-option-{option_index}"
                        )),
                        label,
                        enabled && core_mask == mask,
                        cx,
                    )
                    .when(!enabled, |row| row.opacity(0.48).cursor_default());
                    let row = if enabled {
                        row.on_click(cx.listener(move |app, _, _, cx| {
                            if mask != 0 {
                                if let Some(rule) = app.settings.cpu_affinity.rules.get_mut(index) {
                                    rule.core_mask = mask;
                                }
                                app.active_power_plan_picker = None;
                                cx.notify();
                            }
                        }))
                    } else {
                        row
                    };
                    options = options.child(row);
                }
                options
            },
        );

        let core_grid = self.render_core_tile_grid(
            &processors,
            core_mask,
            true,
            format!("affinity-core-{index}"),
            CoreTileGridAction::CpuAffinityRule { index },
            cx,
        );

        v_flex()
            .w_full()
            .min_w(px(0.0))
            .child(
                rule_action_row(
                    format!("affinity-core-presets-row-{index}"),
                    t!("affinity.core_presets").to_string(),
                    presets_dropdown,
                )
                .into_any_element(),
            )
            .child(
                setting_group_stacked_action_row(
                    format!("affinity-core-row-{index}"),
                    t!("affinity.allowed_cpus").to_string(),
                    core_grid,
                    true,
                )
                .into_any_element(),
            )
            .into_any_element()
    }

    fn render_core_tile_grid(
        &self,
        processors: &[LogicalProcessorInfo],
        core_mask: u64,
        enabled: bool,
        id_prefix: impl Into<String>,
        action: CoreTileGridAction,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        const CORE_GRID_COLUMNS: usize = 8;

        if processors.is_empty() {
            return text_muted(t!("affinity.no_logical_cpus").to_string()).into_any_element();
        }

        let id_prefix = id_prefix.into();
        let mut grid = v_flex().w_full().min_w(px(0.0)).gap_1();
        let mut current_row = h_flex().w_full().min_w(px(0.0)).gap_1();
        let mut cells_in_row = 0;

        for processor in processors {
            let core = processor.index;
            let selected = affinity_mask_contains(core_mask, core);
            let tile_text_color: Hsla = if selected {
                cx.theme().primary_foreground
            } else {
                rgb(primary_text_color()).into()
            };
            let tile_muted_text_color: Hsla = if selected {
                cx.theme().primary_foreground
            } else {
                rgb(muted_text_color()).into()
            };
            let tile_variant = ButtonCustomVariant::new(cx)
                .color(
                    rgb(if selected {
                        accent_color()
                    } else {
                        settings_card_color()
                    })
                    .into(),
                )
                .foreground(tile_text_color)
                .border(
                    rgb(if selected {
                        accent_color()
                    } else {
                        border_color()
                    })
                    .into(),
                )
                .hover(if selected {
                    cx.theme().primary_hover
                } else {
                    cx.theme().secondary_hover
                })
                .active(if selected {
                    cx.theme().primary_active
                } else {
                    cx.theme().secondary_active
                });
            current_row = current_row.child(
                div().flex_1().min_w(px(0.0)).child(
                    Button::new(SharedString::from(format!("{id_prefix}-{core}")))
                        .custom(tile_variant)
                        .rounded(px(4.0))
                        .w_full()
                        .min_w(px(0.0))
                        .h(px(54.0))
                        .disabled(!enabled)
                        .on_click(cx.listener(move |app, _, _, cx| {
                            match action {
                                CoreTileGridAction::EcoQosCpuRestriction { available_mask } => {
                                    toggle_affinity_core_with_available_mask(
                                        &mut app.settings.eco_qos.cpu_restriction_core_mask,
                                        core,
                                        available_mask,
                                    );
                                }
                                CoreTileGridAction::BackgroundCpuRestriction { available_mask } => {
                                    toggle_affinity_core_with_available_mask(
                                        &mut app.settings.background_cpu_restriction.core_mask,
                                        core,
                                        available_mask,
                                    );
                                }
                                CoreTileGridAction::CpuAffinityRule { index } => {
                                    if let Some(rule) =
                                        app.settings.cpu_affinity.rules.get_mut(index)
                                    {
                                        toggle_affinity_core(&mut rule.core_mask, core);
                                    }
                                }
                            }
                            cx.notify();
                        }))
                        .child(
                            v_flex()
                                .items_center()
                                .justify_center()
                                .gap(px(1.0))
                                .child(
                                    div()
                                        .text_size(px(10.0))
                                        .line_height(px(12.0))
                                        .text_color(tile_muted_text_color)
                                        .child(core_tile_kind_label(processor)),
                                )
                                .child(
                                    div()
                                        .text_size(px(TEXT_CONTROL_SIZE))
                                        .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(tile_text_color)
                                        .child(format!("CPU {}", processor.index)),
                                ),
                        ),
                ),
            );
            cells_in_row += 1;
            if cells_in_row == CORE_GRID_COLUMNS {
                grid = grid.child(current_row);
                current_row = h_flex().w_full().min_w(px(0.0)).gap_1();
                cells_in_row = 0;
            }
        }

        if cells_in_row > 0 {
            for _ in cells_in_row..CORE_GRID_COLUMNS {
                current_row = current_row.child(div().flex_1().min_w(px(0.0)));
            }
            grid = grid.child(current_row);
        }

        grid.into_any_element()
    }

    fn render_numeric_value(
        &self,
        field: NumericField,
        display_value: String,
        edit_value: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let width = numeric_value_width(field);
        if self.editing_numeric == Some(field) {
            return h_flex()
                .id(SharedString::from(format!("numeric-editor-{field:?}")))
                .w(px(width))
                .items_center()
                .on_click(|_, _, cx| {
                    cx.stop_propagation();
                })
                .on_action(cx.listener(|app, _: &InputEscape, _, cx| {
                    app.finish_numeric_edit(cx);
                }))
                .on_mouse_down_out(cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                    app.finish_numeric_edit(cx);
                }))
                .child(app_input(&self.inputs.numeric_value, true, cx))
                .into_any_element();
        }

        h_flex()
            .id(SharedString::from(format!("numeric-value-{field:?}")))
            .w(px(width))
            .cursor_pointer()
            .on_click(cx.listener(move |app, _: &gpui::ClickEvent, window, cx| {
                app.begin_numeric_edit(field, edit_value.clone(), window, cx);
            }))
            .child(value_pill(display_value).w_full())
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
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let value = unit.threshold_value_from_bytes(threshold_bytes);
        let value_label = if threshold_bytes == 0 {
            t!("affinity.unlimited").to_string()
        } else {
            network_threshold_value_label(value)
        };
        rule_action_row(
            format!("network-threshold-card-{field:?}"),
            label.to_owned(),
            h_flex()
                .gap_2()
                .items_center()
                .flex_wrap()
                .child(
                    control_button(Button::new(SharedString::from(format!(
                        "threshold-down-{:?}",
                        field
                    ))))
                    .label("-")
                    .disabled(!enabled)
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.adjust_threshold(field, false);
                        cx.notify();
                    })),
                )
                .child(if enabled {
                    self.render_numeric_value(
                        NumericField::NetworkThreshold(field),
                        value_label,
                        network_threshold_edit_value(threshold_bytes, unit),
                        cx,
                    )
                } else {
                    h_flex()
                        .w(px(numeric_value_width(NumericField::NetworkThreshold(
                            field,
                        ))))
                        .child(value_pill(value_label).w_full())
                        .into_any_element()
                })
                .child(
                    control_button(Button::new(SharedString::from(format!(
                        "threshold-up-{:?}",
                        field
                    ))))
                    .label("+")
                    .disabled(!enabled)
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.adjust_threshold(field, true);
                        cx.notify();
                    })),
                )
                .child(self.render_network_unit_picker(field, unit, enabled, window, cx))
                .into_any_element(),
        )
        .when(!enabled, |card| card.opacity(0.42).cursor_default())
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
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picker_id = format!("network-unit-{field:?}");
        let is_open =
            enabled && self.active_power_plan_picker.as_deref() == Some(picker_id.as_str());
        let placement = self.dropdown_placement(
            &picker_id,
            dropdown_list_height(NetworkThresholdUnit::ALL.len()),
            window,
        );
        let mut options = dropdown_surface(cx, placement.max_height);

        for unit in NetworkThresholdUnit::ALL {
            options = options.child(
                dropdown_option_row(
                    SharedString::from(format!("{picker_id}-{}", unit.label())),
                    unit.label().to_string(),
                    selected == unit,
                    cx,
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
                    app.active_power_plan_picker = None;
                    cx.notify();
                })),
            );
        }

        let control_id = SharedString::from(format!("{picker_id}-control"));
        let toggle_picker_id = picker_id.clone();

        dropdown_select_container(DropdownSelectWidth::Compact)
            .child(
                dropdown_select_control(
                    control_id,
                    selected.label().to_string(),
                    enabled,
                    is_open,
                    cx,
                )
                .when(enabled, |control| {
                    control.on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                        app.active_power_plan_picker = (app.active_power_plan_picker.as_deref()
                            != Some(toggle_picker_id.as_str()))
                        .then_some(toggle_picker_id.clone());
                        cx.notify();
                    }))
                }),
            )
            .child(dropdown_anchor_sensor(
                picker_id.clone(),
                Rc::clone(&self.dropdown_anchor_bounds),
            ))
            .child(dropdown_popup_or_empty(is_open, placement, options, cx))
            .into_any_element()
    }

    fn render_theme_selector(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.settings.general.theme_mode;
        let selected_label = theme_mode_label(selected);
        let dropdown = self.render_dropdown_select(
            "theme-mode",
            selected_label,
            true,
            DropdownSelectWidth::Standard,
            AppThemeMode::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for mode in AppThemeMode::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("theme-mode-option-{mode:?}")),
                            theme_mode_label(mode),
                            selected == mode,
                            cx,
                        )
                        .on_click(cx.listener(
                            move |app, _, window, cx| {
                                app.settings.general.theme_mode = mode;
                                app.active_power_plan_picker = None;
                                apply_appearance_settings(&app.settings.general, window, cx);
                                cx.notify();
                            },
                        )),
                    );
                }
                options
            },
        );

        setting_action_card("theme-mode-card", t!("common.theme").to_string(), dropdown)
            .into_any_element()
    }

    fn render_language_selector(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.settings.general.language;
        let dropdown = self.render_dropdown_select(
            "language",
            selected.native_label().to_string(),
            true,
            DropdownSelectWidth::Standard,
            AppLanguage::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for language in AppLanguage::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("language-option-{language:?}")),
                            language.native_label().to_string(),
                            selected == language,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.general.language = language;
                            app.active_power_plan_picker = None;
                            apply_language(language);
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        setting_action_card("language-card", t!("common.language").to_string(), dropdown)
            .into_any_element()
    }

    fn render_accent_selector(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let selected_source = self.settings.general.accent.source;
        let accent_target = SettingGroupTarget::AccentColor;
        let collapsed = self.is_setting_group_collapsed(accent_target);
        let source_dropdown = self.render_dropdown_select(
            "accent-source",
            accent_source_label(selected_source),
            true,
            DropdownSelectWidth::Standard,
            AccentColorSource::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for source in AccentColorSource::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("accent-source-option-{source:?}")),
                            accent_source_label(source),
                            selected_source == source,
                            cx,
                        )
                        .on_click(cx.listener(
                            move |app, _, window, cx| {
                                app.settings.general.accent.source = source;
                                if source == AccentColorSource::Custom {
                                    app.expanded_setting_groups
                                        .insert(SettingGroupTarget::AccentColor);
                                } else {
                                    app.expanded_setting_groups
                                        .remove(&SettingGroupTarget::AccentColor);
                                }
                                app.active_power_plan_picker = None;
                                apply_appearance_settings(&app.settings.general, window, cx);
                                cx.notify();
                            },
                        )),
                    );
                }
                options
            },
        );
        let mut color_palette = h_flex().gap_2().flex_wrap();
        for color in ACCENT_PALETTE {
            let selected = self.settings.general.accent.source == AccentColorSource::Custom
                && self.settings.general.accent.custom_color == color;
            color_palette = color_palette.child(accent_swatch(color, selected).on_click(
                cx.listener(move |app, _, window, cx| {
                    app.settings.general.accent.source = AccentColorSource::Custom;
                    app.settings.general.accent.custom_color = color;
                    app.expanded_setting_groups
                        .insert(SettingGroupTarget::AccentColor);
                    apply_appearance_settings(&app.settings.general, window, cx);
                    cx.notify();
                }),
            ));
        }

        let mut recent_colors = h_flex().gap_2().flex_wrap();
        let mut has_recent_colors = false;
        for color in self
            .settings
            .general
            .accent
            .custom_colors
            .iter()
            .copied()
            .filter(|color| !ACCENT_PALETTE.contains(color))
        {
            has_recent_colors = true;
            let selected = self.settings.general.accent.source == AccentColorSource::Custom
                && self.settings.general.accent.custom_color == color;
            recent_colors = recent_colors.child(accent_swatch(color, selected).on_click(
                cx.listener(move |app, _, window, cx| {
                    app.settings.general.accent.source = AccentColorSource::Custom;
                    app.settings.general.accent.custom_color = color;
                    app.expanded_setting_groups
                        .insert(SettingGroupTarget::AccentColor);
                    apply_appearance_settings(&app.settings.general, window, cx);
                    cx.notify();
                }),
            ));
        }

        let mut palette_content = v_flex().w_full().min_w(px(0.0)).gap_4();
        if has_recent_colors {
            palette_content = palette_content.child(accent_color_group(
                t!("accent.recent_colors").to_string(),
                recent_colors.into_any_element(),
            ));
        }
        palette_content = palette_content.child(accent_color_group(
            t!("accent.color_palette").to_string(),
            color_palette.into_any_element(),
        ));
        let title_toggle_target = accent_target;
        let chevron_toggle_target = accent_target;
        let chevron_icon = if collapsed {
            NavIcon::ChevronRight
        } else {
            NavIcon::ChevronDown
        };

        let mut accent_card = v_flex()
            .id("accent-color-card")
            .w_full()
            .min_w(px(0.0))
            .overflow_hidden()
            .rounded_sm()
            .border_1()
            .border_color(rgb(border_color()))
            .bg(rgb(settings_card_color()))
            .text_color(rgb(primary_text_color()))
            .text_size(px(TEXT_BODY_SIZE))
            .line_height(px(TEXT_BODY_LINE_HEIGHT))
            .child(
                h_flex()
                    .id("accent-source-card")
                    .w_full()
                    .min_w(px(0.0))
                    .min_h(px(58.0))
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .py_3()
                    .px_4()
                    .hover(|style| style.bg(rgb(settings_card_hover_color())))
                    .child(
                        div()
                            .id("accent-color-title")
                            .flex_1()
                            .min_w(px(0.0))
                            .cursor_pointer()
                            .on_click(cx.listener(move |app, _, _, cx| {
                                app.toggle_setting_group(title_toggle_target, cx);
                            }))
                            .truncate()
                            .child(t!("accent.source").to_string()),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .justify_end()
                            .gap_1()
                            .flex_shrink_0()
                            .child(source_dropdown)
                            .child(
                                div()
                                    .id("accent-color-chevron")
                                    .w(px(28.0))
                                    .h(px(24.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .flex_shrink_0()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .text_color(rgb(dim_text_color()))
                                    .opacity(0.72)
                                    .hover(|style| style.opacity(1.0))
                                    .on_click(cx.listener(move |app, _, _, cx| {
                                        app.toggle_setting_group(chevron_toggle_target, cx);
                                    }))
                                    .child(Icon::new(chevron_icon).with_size(px(16.0))),
                            ),
                    ),
            );

        if !collapsed {
            accent_card = accent_card.child(
                div()
                    .id("accent-palette-subcard")
                    .w_full()
                    .min_w(px(0.0))
                    .border_t_1()
                    .border_color(rgb(border_color()))
                    .py_3()
                    .px_4()
                    .child(palette_content),
            );
        }

        accent_card.into_any_element()
    }

    fn render_powerleaf_behaviour_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        page_shell(Page::Settings, cx)
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
            .child(section_title_text(t!("settings.advanced").to_string()))
            .child(self.render_failure_suppression_threshold_setting(cx))
            .child(self.render_action_log_mode_selector(window, cx))
            .child(section_title_text(
                t!("settings.settings_files").to_string(),
            ))
            .child(
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
            )
            .into_any_element()
    }

    fn render_settings_appearance_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        page_shell(Page::SettingsAppearance, cx)
            .child(self.render_theme_selector(window, cx))
            .child(self.render_accent_selector(window, cx))
            .child(self.render_language_selector(window, cx))
            .into_any_element()
    }

    fn render_action_log_mode_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.settings.advanced.action_log_mode;
        let dropdown = self.render_dropdown_select(
            "action-log-mode",
            action_log_mode_label(selected),
            true,
            DropdownSelectWidth::Standard,
            ActionLogMode::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for mode in ActionLogMode::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("action-log-mode-option-{mode:?}")),
                            action_log_mode_label(mode),
                            selected == mode,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.advanced.action_log_mode = mode;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        setting_action_card_with_help(
            "action-log-mode-card",
            t!("settings.action_log_mode").to_string(),
            action_log_mode_help(selected),
            dropdown,
        )
        .into_any_element()
    }

    fn render_failure_suppression_threshold_setting(&self, cx: &mut Context<Self>) -> AnyElement {
        let threshold = self
            .settings
            .advanced
            .execution_failure_suppression_threshold();
        setting_action_card_with_help(
            "execution-failure-suppression-threshold",
            t!("settings.failure_suppression_threshold").to_string(),
            t!("settings.failure_suppression_threshold_help").to_string(),
            self.render_numeric_value(
                NumericField::ExecutionFailureSuppressionThreshold,
                threshold.to_string(),
                threshold.to_string(),
                cx,
            ),
        )
        .into_any_element()
    }

    fn render_win32_priority_separation_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        page_shell(Page::Win32PrioritySeparation, cx)
            .child(self.render_win32_priority_separation_card(window, cx))
            .into_any_element()
    }

    fn render_win32_priority_separation_card(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let edit_value = self.win32_priority_separation_edit_value;
        let backup_value = self
            .win32_priority_separation_backup
            .map(format_win32_priority_separation_with_description)
            .unwrap_or_else(|| t!("settings.win32_priority_separation_no_backup").to_string());
        let has_backup = self.win32_priority_separation_backup.is_some();

        v_flex()
            .w_full()
            .min_w(px(0.0))
            .gap_2()
            .child(text_muted(
                t!("settings.win32_priority_separation_warning").to_string(),
            ))
            .child(self.render_win32_priority_separation_target_card())
            .child(
                v_flex()
                    .w_full()
                    .min_w(px(0.0))
                    .gap_1()
                    .child(processor_power_column_header(
                        t!("settings.win32_priority_separation_tuning").to_string(),
                    ))
                    .child(win32_priority_row(
                        "win32-priority-separation-duration-row",
                        t!("settings.win32_priority_separation_quantum_duration").to_string(),
                        Some(
                            t!("settings.win32_priority_separation_quantum_duration_help")
                                .to_string(),
                        ),
                        self.render_win32_priority_separation_field_picker(
                            Win32PrioritySeparationField::QuantumDuration,
                            window,
                            cx,
                        ),
                    ))
                    .child(win32_priority_row(
                        "win32-priority-separation-behaviour-row",
                        t!("settings.win32_priority_separation_quantum_behaviour").to_string(),
                        Some(
                            t!("settings.win32_priority_separation_quantum_behaviour_help")
                                .to_string(),
                        ),
                        self.render_win32_priority_separation_field_picker(
                            Win32PrioritySeparationField::QuantumBehaviour,
                            window,
                            cx,
                        ),
                    ))
                    .child(win32_priority_row(
                        "win32-priority-separation-boost-row",
                        t!("settings.win32_priority_separation_foreground_boost").to_string(),
                        Some(
                            t!("settings.win32_priority_separation_foreground_boost_help")
                                .to_string(),
                        ),
                        self.render_win32_priority_separation_field_picker(
                            Win32PrioritySeparationField::ForegroundBoost,
                            window,
                            cx,
                        ),
                    ))
                    .child(win32_priority_row(
                        "win32-priority-separation-result-row",
                        t!("settings.win32_priority_separation_resulting_value").to_string(),
                        Some(win32_priority_separation_description(edit_value)),
                        value_pill(format_win32_priority_separation(edit_value)).into_any_element(),
                    )),
            )
            .child(win32_priority_row(
                "win32-priority-separation-backup-row",
                t!("settings.win32_priority_separation_backup").to_string(),
                None,
                value_pill(backup_value).into_any_element(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .flex_wrap()
                    .child(
                        control_button(Button::new("refresh-win32-priority-separation"))
                            .label(t!("settings.refresh").to_string())
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.refresh_win32_priority_separation();
                                cx.notify();
                            })),
                    )
                    .child(
                        control_button(Button::new("save-win32-priority-separation-backup"))
                            .label(t!("settings.save_backup").to_string())
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.save_win32_priority_separation_backup();
                                cx.notify();
                            })),
                    )
                    .child(
                        control_button(Button::new("restore-win32-priority-separation-backup"))
                            .label(t!("settings.restore_backup").to_string())
                            .disabled(!has_backup)
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.restore_win32_priority_separation_backup();
                                cx.notify();
                            })),
                    )
                    .child(
                        control_button(
                            Button::new("apply-win32-priority-separation")
                                .primary()
                                .text_color(cx.theme().primary_foreground),
                        )
                        .label(t!("settings.apply").to_string())
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.apply_win32_priority_separation(
                                app.win32_priority_separation_edit_value,
                            );
                            cx.notify();
                        })),
                    ),
            )
            .child(text_muted(self.win32_priority_separation_status.clone()))
            .into_any_element()
    }

    fn render_win32_priority_separation_target_card(&self) -> AnyElement {
        let current_value = self
            .win32_priority_separation_value
            .map(format_win32_priority_separation_with_description)
            .unwrap_or_else(|| t!("settings.win32_priority_separation_unavailable").to_string());
        h_flex()
            .id("win32-priority-separation-target-card")
            .min_h(px(58.0))
            .w_full()
            .items_center()
            .justify_between()
            .gap_2()
            .py_3()
            .px_4()
            .rounded_sm()
            .border_1()
            .border_color(rgb(border_color()))
            .bg(rgb(settings_card_color()))
            .text_color(rgb(primary_text_color()))
            .text_size(px(TEXT_BODY_SIZE))
            .line_height(px(TEXT_BODY_LINE_HEIGHT))
            .hover(|style| style.bg(rgb(settings_card_hover_color())))
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .min_w(px(0.0))
                            .truncate()
                            .child(t!("settings.win32_priority_separation_current").to_string()),
                    )
                    .child(title_info_button(
                        "win32-priority-separation-current-info",
                        t!("settings.win32_priority_separation_warning").to_string(),
                    )),
            )
            .child(value_pill(current_value))
            .into_any_element()
    }

    fn render_win32_priority_separation_field_picker(
        &self,
        field: Win32PrioritySeparationField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picker_id = win32_priority_separation_field_picker_id(field);
        let options = win32_priority_separation_field_options(field);
        let is_open = self.active_power_plan_picker.as_deref() == Some(picker_id);
        let placement =
            self.dropdown_placement(picker_id, dropdown_list_height(options.len()), window);
        let mut options = dropdown_surface(cx, placement.max_height);
        for option in win32_priority_separation_field_options(field) {
            let selected = win32_priority_separation_field_bits(
                self.win32_priority_separation_edit_value,
                field,
            ) == option.bits;
            options = options.child(
                dropdown_option_row(
                    SharedString::from(format!("{}-option-{:02x}", picker_id, option.bits)),
                    win32_priority_separation_field_option_label(field, option.bits),
                    selected,
                    cx,
                )
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.set_win32_priority_separation_field(field, option.bits);
                    app.active_power_plan_picker = None;
                    cx.notify();
                })),
            );
        }

        let current_label =
            win32_priority_separation_field_label(field, self.win32_priority_separation_edit_value);
        dropdown_select_container(DropdownSelectWidth::Standard)
            .child(
                dropdown_select_control(
                    SharedString::from(format!("{picker_id}-control")),
                    current_label,
                    true,
                    is_open,
                    cx,
                )
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.active_power_plan_picker = (app.active_power_plan_picker.as_deref()
                        != Some(picker_id))
                    .then_some(picker_id.to_owned());
                    cx.notify();
                })),
            )
            .child(dropdown_anchor_sensor(
                picker_id,
                Rc::clone(&self.dropdown_anchor_bounds),
            ))
            .child(dropdown_popup_or_empty(is_open, placement, options, cx))
            .into_any_element()
    }
    fn refresh_win32_priority_separation(&mut self) {
        let (value, status) = read_win32_priority_separation_with_status();
        self.win32_priority_separation_value = value;
        if let Some(value) = value {
            self.win32_priority_separation_edit_value =
                normalize_win32_priority_separation_value(value);
        }
        self.win32_priority_separation_backup = read_win32_priority_separation_backup();
        self.win32_priority_separation_status = status.clone();
        self.status_message = status;
    }

    fn set_win32_priority_separation_field(
        &mut self,
        field: Win32PrioritySeparationField,
        bits: u32,
    ) {
        let value =
            normalize_win32_priority_separation_value(self.win32_priority_separation_edit_value);
        self.win32_priority_separation_edit_value = match field {
            Win32PrioritySeparationField::QuantumDuration => (value & !0x30) | bits,
            Win32PrioritySeparationField::QuantumBehaviour => (value & !0x0C) | bits,
            Win32PrioritySeparationField::ForegroundBoost => (value & !0x03) | bits,
        };
    }

    fn save_win32_priority_separation_backup(&mut self) {
        let Some(value) = read_win32_priority_separation() else {
            self.win32_priority_separation_status =
                t!("settings.win32_priority_separation_load_failed").to_string();
            self.status_message = self.win32_priority_separation_status.clone();
            return;
        };

        match write_win32_priority_separation_backup(value) {
            Ok(()) => {
                self.win32_priority_separation_backup = Some(value);
                self.win32_priority_separation_status = t!(
                    "settings.win32_priority_separation_backup_saved",
                    value = format_win32_priority_separation_with_description(value)
                )
                .to_string();
            }
            Err(err) => {
                self.win32_priority_separation_status = t!(
                    "settings.win32_priority_separation_backup_failed",
                    error = err
                )
                .to_string();
            }
        }
        self.status_message = self.win32_priority_separation_status.clone();
    }

    fn apply_win32_priority_separation(&mut self, value: u32) {
        let value = value.clamp(
            WIN32_PRIORITY_SEPARATION_MIN as u32,
            WIN32_PRIORITY_SEPARATION_MAX as u32,
        );
        if let Err(err) = self.ensure_win32_priority_separation_backup() {
            self.win32_priority_separation_status = t!(
                "settings.win32_priority_separation_backup_failed",
                error = err
            )
            .to_string();
            self.status_message = self.win32_priority_separation_status.clone();
            return;
        }
        match write_win32_priority_separation(value) {
            Ok(()) => {
                self.win32_priority_separation_value = Some(value);
                self.win32_priority_separation_edit_value = value;
                self.win32_priority_separation_status = t!(
                    "settings.win32_priority_separation_saved",
                    value = format_win32_priority_separation_with_description(value)
                )
                .to_string();
            }
            Err(err) => {
                self.win32_priority_separation_status = t!(
                    "settings.win32_priority_separation_save_failed",
                    error = err
                )
                .to_string();
            }
        }
        self.status_message = self.win32_priority_separation_status.clone();
    }

    fn restore_win32_priority_separation_backup(&mut self) {
        let Some(value) = self.win32_priority_separation_backup else {
            self.win32_priority_separation_status =
                t!("settings.win32_priority_separation_no_backup").to_string();
            self.status_message = self.win32_priority_separation_status.clone();
            return;
        };

        match write_win32_priority_separation(value) {
            Ok(()) => {
                self.win32_priority_separation_value = Some(value);
                self.win32_priority_separation_edit_value = value;
                self.win32_priority_separation_status = t!(
                    "settings.win32_priority_separation_restored",
                    value = format_win32_priority_separation_with_description(value)
                )
                .to_string();
            }
            Err(err) => {
                self.win32_priority_separation_status = t!(
                    "settings.win32_priority_separation_restore_failed",
                    error = err
                )
                .to_string();
            }
        }
        self.status_message = self.win32_priority_separation_status.clone();
    }

    fn ensure_win32_priority_separation_backup(&mut self) -> Result<(), String> {
        if self.win32_priority_separation_backup.is_some() {
            return Ok(());
        }
        let current = read_win32_priority_separation()
            .ok_or_else(|| "Current Win32PrioritySeparation value could not be read.".to_owned())?;
        write_win32_priority_separation_backup(current)?;
        self.win32_priority_separation_backup = Some(current);
        Ok(())
    }

    fn render_core_parking_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        page_shell(Page::CoreParking, cx)
            .child(self.render_processor_power_card(window, cx))
            .into_any_element()
    }

    fn render_processor_power_card(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_processor_power_slider_states(window, cx);
        let has_current_plan = self.current_plan.is_some();
        let processor_power_presets = [
            ProcessorPowerPreset::Performance,
            ProcessorPowerPreset::Balanced,
            ProcessorPowerPreset::Saver,
        ];
        let selected_preset = processor_power_presets
            .iter()
            .copied()
            .find(|preset| self.processor_power_matches_preset(*preset));
        let preset_dropdown = self.render_dropdown_select(
            "processor-power-preset",
            selected_preset
                .map(processor_power_preset_label)
                .unwrap_or_else(|| "Custom".to_owned()),
            true,
            DropdownSelectWidth::Standard,
            processor_power_presets.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for preset in processor_power_presets {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("processor-power-preset-option-{preset:?}")),
                            processor_power_preset_label(preset),
                            selected_preset == Some(preset),
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.fill_processor_power_preset(preset);
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        v_flex()
            .w_full()
            .min_w(px(0.0))
            .gap_2()
            .child(self.render_processor_power_plan_picker(window, cx))
            .child(feature_toggle_switch(
                "processor-power-link-ac-dc",
                t!("processor_power.link_ac_dc").to_string(),
                self.processor_power_link_ac_dc,
                cx.listener(|app, checked: &bool, _, cx| {
                    app.processor_power_link_ac_dc = *checked;
                    if *checked {
                        let values = app.processor_power_values();
                        app.set_processor_power_values(ProcessorPowerAcDcValues::same(values.ac));
                        app.processor_power_dirty = true;
                    }
                    cx.notify();
                }),
            ))
            .child(setting_action_card(
                "processor-power-presets-card",
                t!("processor_power.presets").to_string(),
                preset_dropdown,
            ))
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        v_flex()
                            .w_full()
                            .min_w(px(0.0))
                            .gap_1()
                            .child(processor_power_column_header(
                                t!("processor_power.ac_values").to_string(),
                            ))
                            .child(processor_power_slider(
                                "processor-power-ac-core-parking-min",
                                &t!("processor_power.core_parking_min"),
                                self.processor_power_ac_core_parking_min,
                                self.render_numeric_value(
                                    NumericField::ProcessorAcCoreParkingMin,
                                    format!("{}%", self.processor_power_ac_core_parking_min),
                                    self.processor_power_ac_core_parking_min.to_string(),
                                    cx,
                                ),
                                &self.inputs.processor_power_ac_core_parking_min,
                                window,
                                cx,
                                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                                    let value = apply_u64_step(
                                        app.processor_power_ac_core_parking_min,
                                        change,
                                        0,
                                        100,
                                    );
                                    app.set_processor_power_slider_value(
                                        ProcessorPowerSlider::AcCoreParkingMin,
                                        value,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(processor_power_slider(
                                "processor-power-ac-performance-min",
                                &t!("processor_power.processor_min"),
                                self.processor_power_ac_performance_min,
                                self.render_numeric_value(
                                    NumericField::ProcessorAcPerformanceMin,
                                    format!("{}%", self.processor_power_ac_performance_min),
                                    self.processor_power_ac_performance_min.to_string(),
                                    cx,
                                ),
                                &self.inputs.processor_power_ac_performance_min,
                                window,
                                cx,
                                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                                    let value = apply_u64_step(
                                        app.processor_power_ac_performance_min,
                                        change,
                                        0,
                                        100,
                                    );
                                    app.set_processor_power_slider_value(
                                        ProcessorPowerSlider::AcPerformanceMin,
                                        value,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(processor_power_slider(
                                "processor-power-ac-performance-max",
                                &t!("processor_power.processor_max"),
                                self.processor_power_ac_performance_max,
                                self.render_numeric_value(
                                    NumericField::ProcessorAcPerformanceMax,
                                    format!("{}%", self.processor_power_ac_performance_max),
                                    self.processor_power_ac_performance_max.to_string(),
                                    cx,
                                ),
                                &self.inputs.processor_power_ac_performance_max,
                                window,
                                cx,
                                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                                    let value = apply_u64_step(
                                        app.processor_power_ac_performance_max,
                                        change,
                                        0,
                                        100,
                                    );
                                    app.set_processor_power_slider_value(
                                        ProcessorPowerSlider::AcPerformanceMax,
                                        value,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(processor_power_setting_row(
                                "processor-power-ac-boost-mode",
                                t!("processor_power.boost_mode").to_string(),
                                self.render_processor_boost_mode_picker(
                                    ProcessorPowerSource::Ac,
                                    window,
                                    cx,
                                ),
                            )),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .min_w(px(0.0))
                            .gap_1()
                            .child(processor_power_column_header(
                                t!("processor_power.dc_values").to_string(),
                            ))
                            .child(processor_power_slider(
                                "processor-power-dc-core-parking-min",
                                &t!("processor_power.core_parking_min"),
                                self.processor_power_dc_core_parking_min,
                                self.render_numeric_value(
                                    NumericField::ProcessorDcCoreParkingMin,
                                    format!("{}%", self.processor_power_dc_core_parking_min),
                                    self.processor_power_dc_core_parking_min.to_string(),
                                    cx,
                                ),
                                &self.inputs.processor_power_dc_core_parking_min,
                                window,
                                cx,
                                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                                    let value = apply_u64_step(
                                        app.processor_power_dc_core_parking_min,
                                        change,
                                        0,
                                        100,
                                    );
                                    app.set_processor_power_slider_value(
                                        ProcessorPowerSlider::DcCoreParkingMin,
                                        value,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(processor_power_slider(
                                "processor-power-dc-performance-min",
                                &t!("processor_power.processor_min"),
                                self.processor_power_dc_performance_min,
                                self.render_numeric_value(
                                    NumericField::ProcessorDcPerformanceMin,
                                    format!("{}%", self.processor_power_dc_performance_min),
                                    self.processor_power_dc_performance_min.to_string(),
                                    cx,
                                ),
                                &self.inputs.processor_power_dc_performance_min,
                                window,
                                cx,
                                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                                    let value = apply_u64_step(
                                        app.processor_power_dc_performance_min,
                                        change,
                                        0,
                                        100,
                                    );
                                    app.set_processor_power_slider_value(
                                        ProcessorPowerSlider::DcPerformanceMin,
                                        value,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(processor_power_slider(
                                "processor-power-dc-performance-max",
                                &t!("processor_power.processor_max"),
                                self.processor_power_dc_performance_max,
                                self.render_numeric_value(
                                    NumericField::ProcessorDcPerformanceMax,
                                    format!("{}%", self.processor_power_dc_performance_max),
                                    self.processor_power_dc_performance_max.to_string(),
                                    cx,
                                ),
                                &self.inputs.processor_power_dc_performance_max,
                                window,
                                cx,
                                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                                    let value = apply_u64_step(
                                        app.processor_power_dc_performance_max,
                                        change,
                                        0,
                                        100,
                                    );
                                    app.set_processor_power_slider_value(
                                        ProcessorPowerSlider::DcPerformanceMax,
                                        value,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(processor_power_setting_row(
                                "processor-power-dc-boost-mode",
                                t!("processor_power.boost_mode").to_string(),
                                self.render_processor_boost_mode_picker(
                                    ProcessorPowerSource::Dc,
                                    window,
                                    cx,
                                ),
                            )),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        control_button(Button::new("processor-power-refresh-values"))
                            .label(t!("processor_power.refresh_values").to_string())
                            .disabled(self.current_plan.is_none())
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.refresh_processor_power_values();
                                cx.notify();
                            })),
                    )
                    .child(
                        control_button(
                            Button::new("processor-power-apply-custom")
                                .primary()
                                .text_color(cx.theme().primary_foreground),
                        )
                        .label(t!("processor_power.apply_custom").to_string())
                        .disabled(!has_current_plan)
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.apply_processor_power_custom();
                            cx.notify();
                        })),
                    ),
            )
            .into_any_element()
    }

    fn render_processor_boost_mode_picker(
        &self,
        source: ProcessorPowerSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picker_id = processor_boost_mode_picker_id(source);
        let is_open = self.active_power_plan_picker.as_deref() == Some(picker_id);
        let placement = self.dropdown_placement(
            picker_id,
            dropdown_list_height(ProcessorBoostMode::ALL.len()),
            window,
        );
        let selected = match source {
            ProcessorPowerSource::Ac => self.processor_power_ac_boost_mode,
            ProcessorPowerSource::Dc => self.processor_power_dc_boost_mode,
        };
        let mut options = dropdown_surface(cx, placement.max_height);
        for boost_mode in ProcessorBoostMode::ALL {
            options = options.child(
                dropdown_option_row(
                    SharedString::from(format!(
                        "processor-boost-mode-{source:?}-option-{boost_mode:?}"
                    )),
                    processor_boost_mode_label(boost_mode),
                    selected == boost_mode,
                    cx,
                )
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.set_processor_power_boost_mode(source, boost_mode);
                    app.active_power_plan_picker = None;
                    cx.notify();
                })),
            );
        }

        dropdown_select_container(DropdownSelectWidth::Wide)
            .child(
                dropdown_select_control(
                    SharedString::from(format!("{picker_id}-control")),
                    processor_boost_mode_label(selected),
                    true,
                    is_open,
                    cx,
                )
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.active_power_plan_picker = (app.active_power_plan_picker.as_deref()
                        != Some(picker_id))
                    .then_some(picker_id.to_owned());
                    cx.notify();
                })),
            )
            .child(dropdown_anchor_sensor(
                picker_id,
                Rc::clone(&self.dropdown_anchor_bounds),
            ))
            .child(dropdown_popup_or_empty(is_open, placement, options, cx))
            .into_any_element()
    }

    fn render_processor_power_plan_picker(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let id = "processor-power-target-plan";
        let is_open = self.active_power_plan_picker.as_deref() == Some(id);
        let option_count = self.plans.len().max(1);
        let placement = self.dropdown_placement(id, dropdown_list_height(option_count), window);
        let selected_guid = self
            .processor_power_target_plan_guid
            .as_deref()
            .or_else(|| self.current_plan.as_ref().map(|plan| plan.guid.as_str()));
        let selected_text = selected_guid
            .and_then(|guid| {
                self.plans
                    .iter()
                    .find(|plan| plan.guid.eq_ignore_ascii_case(guid))
            })
            .map(PowerPlan::display_name)
            .unwrap_or_else(|| t!("processor_power.no_active_plan").to_string());

        let mut options = dropdown_surface(cx, placement.max_height);

        if self.plans.is_empty() {
            options = options.child(dropdown_empty_row(
                t!("common.no_power_plans_loaded").to_string(),
                cx,
            ));
        } else {
            for plan in &self.plans {
                let selected =
                    selected_guid.is_some_and(|selected| selected.eq_ignore_ascii_case(&plan.guid));
                options = options.child(power_plan_option_row(
                    format!("{id}-{}", plan.guid),
                    plan.display_name(),
                    selected,
                    Some(plan.guid.clone()),
                    PowerPlanField::ProcessorPowerTarget,
                    cx,
                ));
            }
        }

        let target_plan_select = dropdown_select_container(DropdownSelectWidth::Wide)
            .child(
                dropdown_select_control(
                    "processor-power-target-plan-control",
                    selected_text,
                    true,
                    is_open,
                    cx,
                )
                .on_click(cx.listener(|app, _: &gpui::ClickEvent, _, cx| {
                    app.refresh_power_plans();
                    app.active_power_plan_picker = (app.active_power_plan_picker.as_deref()
                        != Some("processor-power-target-plan"))
                    .then_some("processor-power-target-plan".to_owned());
                    cx.notify();
                })),
            )
            .child(dropdown_anchor_sensor(
                id,
                Rc::clone(&self.dropdown_anchor_bounds),
            ))
            .child(dropdown_popup_or_empty(is_open, placement, options, cx));

        let picker = v_flex().w_full().min_w(px(0.0)).relative().child(
            h_flex()
                .id("processor-power-target-plan-card")
                .min_h(px(58.0))
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .py_3()
                .px_4()
                .rounded_sm()
                .border_1()
                .border_color(rgb(border_color()))
                .bg(rgb(settings_card_color()))
                .text_color(rgb(primary_text_color()))
                .text_size(px(TEXT_BODY_SIZE))
                .line_height(px(TEXT_BODY_LINE_HEIGHT))
                .hover(|style| style.bg(rgb(settings_card_hover_color())))
                .child(
                    h_flex()
                        .flex_1()
                        .min_w(px(0.0))
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .min_w(px(0.0))
                                .truncate()
                                .child(t!("processor_power.target_plan").to_string()),
                        )
                        .child(title_info_button(
                            "processor-power-target-plan-info",
                            t!("processor_power.help").to_string(),
                        )),
                )
                .child(target_plan_select),
        );

        picker
    }

    fn render_about_page(&self, cx: &mut Context<Self>) -> AnyElement {
        page_shell(Page::About, cx)
            .child(section_header(
                &t!("app.name"),
                t!("app.description").to_string(),
            ))
            .child(stat_grid(vec![
                (t!("about.author").to_string(), "Tatsh Siow".to_owned()),
                (
                    t!("about.version").to_string(),
                    env!("CARGO_PKG_VERSION").to_owned(),
                ),
            ]))
            .into_any_element()
    }

    fn render_action_log_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let help = tooltip_lines(vec![
            t!("action_log.intro_1").to_string(),
            t!("action_log.intro_2").to_string(),
        ]);
        let mut visible_entries = Vec::new();
        for entry in self
            .action_log_entries
            .iter()
            .rev()
            .filter(|entry| self.action_log_filter.matches(entry.result))
        {
            visible_entries.push(entry);
            if visible_entries.len() == 100 {
                break;
            }
        }

        let mut list = rule_list();
        if self.action_log_entries.is_empty() {
            list = list.child(
                GroupBox::new()
                    .outline()
                    .child(text_muted(t!("action_log.empty").to_string())),
            );
        } else if visible_entries.is_empty() {
            list = list.child(
                GroupBox::new()
                    .outline()
                    .child(text_muted(t!("action_log.no_filter_matches").to_string())),
            );
        } else {
            list = list.child(action_log_header_row());
            for entry in visible_entries {
                list = list.child(action_log_entry_row(entry, cx));
            }
        }

        page_shell_with_help(Page::ActionLog, Some(help), cx)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .flex_wrap()
                    .child(self.render_action_log_filter(window, cx))
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                control_button(Button::new("clear-action-log"))
                                    .label(t!("action_log.clear").to_string())
                                    .disabled(self.action_log_entries.is_empty())
                                    .on_click(cx.listener(|app, _, _, cx| {
                                        app.background_automation.clear_action_log();
                                        app.action_log_entries.clear();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                control_button(Button::new("export-action-log"))
                                    .label(t!("action_log.export_csv").to_string())
                                    .disabled(self.action_log_entries.is_empty())
                                    .on_click(cx.listener(|app, _, _, cx| {
                                        app.export_action_log_csv();
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                GroupBox::new()
                    .outline()
                    .title(section_title_label(
                        t!("action_log.recent_entries").to_string(),
                    ))
                    .child(list),
            )
            .into_any_element()
    }

    fn render_action_log_filter(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.action_log_filter;
        let dropdown = self.render_dropdown_select(
            "action-log-filter",
            action_log_filter_label(selected),
            true,
            DropdownSelectWidth::Standard,
            ActionLogResultFilter::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for filter in ActionLogResultFilter::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("action-log-filter-option-{filter:?}")),
                            action_log_filter_label(filter),
                            selected == filter,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.action_log_filter = filter;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        setting_action_card(
            "action-log-filter-card",
            t!("action_log.filter").to_string(),
            dropdown,
        )
        .into_any_element()
    }

    fn render_inline_power_plan_picker(
        &self,
        id: impl Into<String>,
        selected_guid: Option<String>,
        field: PowerPlanField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = id.into();
        let is_open = self.active_power_plan_picker.as_deref() == Some(id.as_str());
        let option_count = self.plans.len().max(1);
        let placement = self.dropdown_placement(&id, dropdown_list_height(option_count), window);
        let selected_text = match selected_guid.as_deref() {
            Some(guid) => self
                .plans
                .iter()
                .find(|plan| plan.guid.eq_ignore_ascii_case(guid))
                .map(PowerPlan::display_name)
                .unwrap_or_else(|| t!("common.selected_plan_unavailable").to_string()),
            None => t!("common.selected_plan_unavailable").to_string(),
        };

        let mut options = dropdown_surface(cx, placement.max_height);

        if self.plans.is_empty() {
            options = options.child(dropdown_empty_row(
                t!("common.no_power_plans_loaded").to_string(),
                cx,
            ));
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
        dropdown_select_container(DropdownSelectWidth::Standard)
            .child(
                dropdown_select_control(
                    SharedString::from(format!("{id}-select-control")),
                    selected_text,
                    true,
                    is_open,
                    cx,
                )
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.refresh_power_plans();
                    app.active_power_plan_picker = (app.active_power_plan_picker.as_deref()
                        != Some(control_id.as_str()))
                    .then_some(control_id.clone());
                    cx.notify();
                })),
            )
            .child(dropdown_anchor_sensor(
                id.clone(),
                Rc::clone(&self.dropdown_anchor_bounds),
            ))
            .child(dropdown_popup_or_empty(is_open, placement, options, cx))
            .into_any_element()
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
            PowerPlanField::PerformanceModeRule(index) => {
                if let Some(rule) = self.settings.performance_mode.rules.get_mut(index) {
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
            PowerPlanField::ProcessorPowerTarget => {
                self.set_processor_power_target_plan_option(guid);
            }
        }
    }

    fn render_process_suggestions(
        &self,
        id: impl Into<String>,
        query: &str,
        target: SuggestionTarget,
        max_height: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = id.into();
        let query = query.trim().to_ascii_lowercase();
        let mut matches = self
            .process_candidates
            .iter()
            .filter(|process| {
                query.is_empty() || process.name.to_ascii_lowercase().contains(query.as_str())
            })
            .filter(|process| process_target_can_accept(target, &self.settings, &process.name))
            .cloned()
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| left.name.cmp(&right.name));

        let mut suggestions = dropdown_surface(cx, max_height);
        if matches.is_empty() {
            suggestions = suggestions.child(dropdown_empty_row(
                if self.process_candidates.is_empty() {
                    t!("common.no_running_apps_loaded").to_string()
                } else {
                    t!("common.no_matching_apps").to_string()
                },
                cx,
            ));
        }
        for (count, process) in matches.into_iter().enumerate() {
            let process_name = process.name.clone();
            suggestions = suggestions.child(
                dropdown_process_option_row(
                    SharedString::from(format!("{id}-{count}")),
                    &process,
                    count == 0,
                    cx,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |app, _: &gpui::MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        app.apply_process_suggestion(target, &process_name, window, cx);
                        window.blur();
                        cx.notify();
                    }),
                ),
            );
        }

        suggestions.into_any_element()
    }

    fn process_icon_for_name(&self, process: &str) -> Option<&Arc<Image>> {
        let process = process.trim();
        self.process_candidates
            .iter()
            .find(|candidate| candidate.name.eq_ignore_ascii_case(process))
            .and_then(|candidate| candidate.icon.as_ref())
    }

    fn process_rule_title(&self, process: &str, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .items_center()
            .gap_2()
            .child(process_icon_cell(self.process_icon_for_name(process), cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(TEXT_HEADER_SIZE))
                    .line_height(px(TEXT_HEADER_LINE_HEIGHT))
                    .child(process.to_owned()),
            )
            .into_any_element()
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
        let normalized_query = query.trim().to_ascii_lowercase();
        let suggestion_count = self
            .process_candidates
            .iter()
            .filter(|process| {
                normalized_query.is_empty()
                    || process
                        .name
                        .to_ascii_lowercase()
                        .contains(normalized_query.as_str())
            })
            .filter(|process| process_target_can_accept(target, &self.settings, &process.name))
            .count()
            .max(1);
        let placement =
            self.dropdown_placement(&id, dropdown_list_height(suggestion_count), window);

        v_flex()
            .w_full()
            .max_w(px(372.0))
            .min_w(px(0.0))
            .relative()
            .min_h(px(32.0))
            .child(app_input(input, is_open, cx))
            .child(dropdown_anchor_sensor(
                id.clone(),
                Rc::clone(&self.dropdown_anchor_bounds),
            ))
            .child(if is_open {
                deferred(
                    dropdown_popup_layer(placement)
                        .occlude()
                        .on_mouse_down_out(cx.listener(
                            |_, _: &gpui::MouseDownEvent, window, cx| {
                                window.blur();
                                cx.notify();
                            },
                        ))
                        .child(self.render_process_suggestions(
                            id,
                            &query,
                            target,
                            placement.max_height,
                            cx,
                        )),
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
            SuggestionTarget::Foreground => {
                clear_input_to(&self.inputs.foreground_process, process, window, cx);
            }
            SuggestionTarget::EcoQos => {
                clear_input_to(&self.inputs.eco_qos_exclusion, process, window, cx);
            }
            SuggestionTarget::BackgroundCpu => {
                clear_input_to(&self.inputs.background_cpu_exclusion, process, window, cx);
            }
            SuggestionTarget::SmartTrim => {
                clear_input_to(&self.inputs.smart_trim_exclusion, process, window, cx);
            }
            SuggestionTarget::Suspension => {
                clear_input_to(&self.inputs.suspension_process, process, window, cx);
            }
            SuggestionTarget::CpuLimiter => {
                clear_input_to(&self.inputs.cpu_limiter_process, process, window, cx);
            }
            SuggestionTarget::Watchdog => {
                clear_input_to(&self.inputs.watchdog_process, process, window, cx);
            }
            SuggestionTarget::PerformanceMode => {
                clear_input_to(&self.inputs.performance_process, process, window, cx);
            }
            SuggestionTarget::Responsiveness => {
                clear_input_to(&self.inputs.responsiveness_process, process, window, cx);
            }
            SuggestionTarget::IoPriority => {
                clear_input_to(&self.inputs.io_priority_process, process, window, cx);
            }
            SuggestionTarget::MemoryPriority => {
                clear_input_to(&self.inputs.memory_priority_process, process, window, cx);
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
    PerformanceModeRule(usize),
    ScheduleRule(usize),
    CpuRule(usize),
    CpuRuleElse(usize),
    ProcessorPowerTarget,
}

fn power_plan_option_row(
    id: String,
    label: String,
    selected: bool,
    guid: Option<String>,
    field: PowerPlanField,
    cx: &mut Context<PowerLeafApp>,
) -> AnyElement {
    dropdown_option_row(SharedString::from(id), label, selected, cx)
        .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
            app.set_power_plan_field(field, guid.clone());
            app.active_power_plan_picker = None;
            cx.notify();
        }))
        .into_any_element()
}

fn dropdown_list_height(row_count: usize) -> Pixels {
    let row_count = row_count.max(1);
    px(DROPDOWN_SURFACE_VERTICAL_PADDING
        + (row_count as f32 * DROPDOWN_OPTION_ROW_HEIGHT)
        + (row_count.saturating_sub(1) as f32 * DROPDOWN_OPTION_GAP))
}

fn dropdown_anchor_sensor(
    id: impl Into<String>,
    anchor_bounds: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
) -> AnyElement {
    let id = id.into();
    canvas(
        move |bounds, _, _| {
            anchor_bounds.borrow_mut().insert(id, bounds);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
    .into_any_element()
}

#[derive(Clone, Copy)]
enum DropdownSelectWidth {
    Compact,
    Standard,
    Wide,
}

fn dropdown_select_container(width: DropdownSelectWidth) -> gpui::Div {
    let width = match width {
        DropdownSelectWidth::Compact => DROPDOWN_SELECT_COMPACT_WIDTH,
        DropdownSelectWidth::Standard => DROPDOWN_SELECT_STANDARD_WIDTH,
        DropdownSelectWidth::Wide => DROPDOWN_SELECT_WIDE_WIDTH,
    };

    v_flex()
        .w(px(width))
        .min_w(px(width))
        .max_w(px(width))
        .flex_shrink_0()
        .relative()
        .min_h(px(DROPDOWN_CONTROL_HEIGHT))
}

fn dropdown_popup_layer(placement: DropdownPlacement) -> gpui::Div {
    let layer = div().absolute().left(px(0.0)).right(px(0.0)).occlude();

    if placement.open_up {
        layer.bottom(px(DROPDOWN_MENU_OFFSET))
    } else {
        layer.top(px(DROPDOWN_MENU_OFFSET))
    }
}

fn dropdown_popup_or_empty(
    is_open: bool,
    placement: DropdownPlacement,
    options: Scrollable<gpui::Div>,
    cx: &mut Context<PowerLeafApp>,
) -> AnyElement {
    if is_open {
        deferred(
            dropdown_popup_layer(placement)
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
        Empty.into_any_element()
    }
}

fn dropdown_select_control(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    enabled: bool,
    open: bool,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Stateful<gpui::Div> {
    let label: SharedString = label.into();
    let border_color: Hsla = if enabled && open {
        cx.theme().accent
    } else {
        rgb(dropdown_control_border_color()).into()
    };
    let hover_border_color: Hsla = if enabled && open {
        cx.theme().accent
    } else {
        rgb(dropdown_control_hover_border_color()).into()
    };

    h_flex()
        .id(id.into())
        .h(px(DROPDOWN_CONTROL_HEIGHT))
        .w_full()
        .min_w(px(0.0))
        .items_center()
        .justify_between()
        .gap_2()
        .px_3()
        .rounded_sm()
        .border_1()
        .border_color(border_color)
        .bg(rgb(dropdown_control_color()))
        .text_size(px(TEXT_CONTROL_SIZE))
        .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
        .text_color(cx.theme().foreground)
        .hover(move |style| {
            if enabled {
                style
                    .bg(rgb(dropdown_control_hover_color()))
                    .border_color(hover_border_color)
            } else {
                style
            }
        })
        .when(enabled, |style| style.cursor_pointer())
        .when(!enabled, |style| style.cursor_default().opacity(0.48))
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .child(div().flex_1().min_w(px(0.0)).truncate().child(label))
        .child(dropdown_chevron(cx))
}

fn dropdown_surface(cx: &mut Context<PowerLeafApp>, max_height: Pixels) -> Scrollable<gpui::Div> {
    v_flex()
        .w_full()
        .max_h(max_height)
        .overflow_y_scrollbar()
        .gap_1()
        .p_2()
        .rounded_sm()
        .border_1()
        .border_color(cx.theme().border)
        .bg(rgb(dropdown_surface_color()))
}

fn dropdown_option_row(
    id: SharedString,
    label: String,
    selected: bool,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Stateful<gpui::Div> {
    h_flex()
        .id(SharedString::from(id))
        .relative()
        .min_h(px(40.0))
        .items_center()
        .pl_3()
        .pr_3()
        .rounded_sm()
        .text_size(px(TEXT_CONTROL_SIZE))
        .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
        .text_color(cx.theme().popover_foreground)
        .when(selected, |row| {
            row.bg(rgb(dropdown_selected_color())).child(
                div()
                    .absolute()
                    .left(px(0.0))
                    .top(px(11.0))
                    .bottom(px(11.0))
                    .w(px(3.0))
                    .rounded_sm()
                    .bg(cx.theme().accent),
            )
        })
        .hover(|style| style.bg(rgb(dropdown_option_hover_color())))
        .cursor_pointer()
        .child(label)
}

fn dropdown_process_option_row(
    id: SharedString,
    process: &ProcessCandidate,
    selected: bool,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Stateful<gpui::Div> {
    h_flex()
        .id(SharedString::from(id))
        .relative()
        .min_h(px(40.0))
        .items_center()
        .gap_2()
        .pl_3()
        .pr_3()
        .rounded_sm()
        .text_size(px(TEXT_CONTROL_SIZE))
        .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
        .text_color(cx.theme().popover_foreground)
        .when(selected, |row| {
            row.bg(rgb(dropdown_selected_color())).child(
                div()
                    .absolute()
                    .left(px(0.0))
                    .top(px(11.0))
                    .bottom(px(11.0))
                    .w(px(3.0))
                    .rounded_sm()
                    .bg(cx.theme().accent),
            )
        })
        .hover(|style| style.bg(rgb(dropdown_option_hover_color())))
        .cursor_pointer()
        .child(process_icon_cell(process.icon.as_ref(), cx))
        .child(div().min_w(px(0.0)).truncate().child(process.name.clone()))
}

fn process_icon_cell(icon: Option<&Arc<Image>>, cx: &mut Context<PowerLeafApp>) -> AnyElement {
    div()
        .size(px(20.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .child(match icon {
            Some(icon) => img(Arc::clone(icon)).size(px(18.0)).into_any_element(),
            None => div()
                .size(px(18.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .border_1()
                .border_color(cx.theme().border)
                .child(Icon::new(NavIcon::Frame).with_size(px(13.0)))
                .into_any_element(),
        })
        .into_any_element()
}

fn dropdown_empty_row(message: String, cx: &mut Context<PowerLeafApp>) -> gpui::Div {
    div()
        .min_h(px(40.0))
        .px_3()
        .flex()
        .relative()
        .items_center()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .text_color(cx.theme().muted_foreground)
        .child(message)
}

fn dropdown_chevron(cx: &mut Context<PowerLeafApp>) -> AnyElement {
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(px(18.0))
        .child(
            Icon::new(NavIcon::ChevronDown)
                .with_size(px(16.0))
                .text_color(cx.theme().muted_foreground),
        )
        .into_any_element()
}

fn dropdown_control_color() -> u32 {
    if ui_is_dark() {
        0x2f2f2f
    } else {
        0xffffff
    }
}

fn dropdown_control_border_color() -> u32 {
    if ui_is_dark() {
        COLOR_BORDER
    } else {
        0xdedede
    }
}

fn dropdown_control_hover_color() -> u32 {
    if ui_is_dark() {
        0x333333
    } else {
        0xf5f5f5
    }
}

fn dropdown_control_hover_border_color() -> u32 {
    if ui_is_dark() {
        0x6a6a6a
    } else {
        0x9a9a9a
    }
}

fn dropdown_surface_color() -> u32 {
    if ui_is_dark() {
        0x2b2b2b
    } else {
        0xffffff
    }
}

fn dropdown_selected_color() -> u32 {
    if ui_is_dark() {
        0x303030
    } else {
        0xeaeaea
    }
}

fn dropdown_option_hover_color() -> u32 {
    if ui_is_dark() {
        0x333333
    } else {
        0xf5f5f5
    }
}

#[derive(Debug, Clone, Copy)]
enum SuggestionTarget {
    Foreground,
    EcoQos,
    BackgroundCpu,
    SmartTrim,
    Suspension,
    CpuLimiter,
    Watchdog,
    PerformanceMode,
    Responsiveness,
    IoPriority,
    MemoryPriority,
    Affinity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleTitleTarget {
    Schedule(usize),
    Cpu(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RuleCardTarget {
    Schedule(usize),
    Cpu(usize),
    Suspension(String),
    CpuLimiter(String),
    Watchdog(String),
    #[allow(dead_code)]
    Responsiveness(String),
    IoPriority(String),
    Affinity(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SettingGroupTarget {
    AccentColor,
    AutoBalanceAffinity,
    AutoBalanceBehaviourTuning,
    AutoBalanceEfficiency,
    AutoBalanceExclusions,
    AutoBalanceIoPriority,
    AutoBalanceMemoryPriority,
    EfficiencyCpuRestriction,
    BackgroundCpuRestriction,
    SuspensionThaw,
    SuspensionAudio,
    SuspensionNetwork,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoBalancePreset {
    Gentle,
    Balanced,
    Responsive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoBalanceBehavior {
    Preset(AutoBalancePreset),
}

impl AutoBalanceBehavior {
    const ALL: [Self; 3] = [
        Self::Preset(AutoBalancePreset::Gentle),
        Self::Preset(AutoBalancePreset::Balanced),
        Self::Preset(AutoBalancePreset::Responsive),
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ThresholdField {
    Download(usize),
    Upload(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum NumericField {
    ActivityIdleTimeout,
    GeneralCheckInterval,
    ExecutionFailureSuppressionThreshold,
    EcoQosRestrictionPercent,
    BackgroundCpuRestrictionPercent,
    SmartTrimCheckIntervalMinutes,
    SmartTrimMemoryLoadThreshold,
    SmartTrimWorkingSetThreshold,
    SmartTrimCpuIdleThreshold,
    SmartTrimIdleSeconds,
    SmartTrimCooldownSeconds,
    SmartTrimPurgeFreeRamThreshold,
    SuspensionBackgroundDelay,
    SuspensionThawInterval,
    SuspensionThawDuration,
    SuspensionAudioRefreeze,
    SuspensionNetworkRefreeze,
    AutoBalanceTotalThreshold,
    AutoBalanceThreshold,
    AutoBalanceRestoreThreshold,
    AutoBalanceCpuPercent,
    AutoBalanceSustain,
    AutoBalanceMinimumRestraint,
    AutoBalanceCooldown,
    ProcessorAcCoreParkingMin,
    ProcessorAcPerformanceMin,
    ProcessorAcPerformanceMax,
    ProcessorDcCoreParkingMin,
    ProcessorDcPerformanceMin,
    ProcessorDcPerformanceMax,
    CpuThreshold(usize),
    CpuUpperThreshold(usize),
    CpuDuration(usize),
    CpuLimiterThreshold(usize),
    CpuLimiterSustain(usize),
    CpuLimiterCooldown(usize),
    CpuLimiterMaxProcessors(usize),
    WatchdogRestartDelay(usize),
    NetworkThreshold(ThresholdField),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ProcessorPowerSlider {
    AcCoreParkingMin,
    AcPerformanceMin,
    AcPerformanceMax,
    DcCoreParkingMin,
    DcPerformanceMin,
    DcPerformanceMax,
}

impl ProcessorPowerSlider {
    const fn paired_power_source(self) -> Self {
        match self {
            Self::AcCoreParkingMin => Self::DcCoreParkingMin,
            Self::AcPerformanceMin => Self::DcPerformanceMin,
            Self::AcPerformanceMax => Self::DcPerformanceMax,
            Self::DcCoreParkingMin => Self::AcCoreParkingMin,
            Self::DcPerformanceMin => Self::AcPerformanceMin,
            Self::DcPerformanceMax => Self::AcPerformanceMax,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ProcessorPowerSource {
    Ac,
    Dc,
}

impl ProcessorPowerSource {
    const fn paired(self) -> Self {
        match self {
            Self::Ac => Self::Dc,
            Self::Dc => Self::Ac,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CpuThresholdSlider {
    Lower(usize),
    Upper(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ActivitySlider {
    IdleTimeout,
    CheckInterval,
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

fn make_percent_slider(cx: &mut Context<PowerLeafApp>, value: u64) -> Entity<SliderState> {
    make_range_slider(cx, value, 0, 100, 1)
}

fn make_range_slider(
    cx: &mut Context<PowerLeafApp>,
    value: u64,
    min: u64,
    max: u64,
    step: u64,
) -> Entity<SliderState> {
    let (min, max) = if min <= max { (min, max) } else { (max, min) };
    let value = value.clamp(min, max);
    cx.new(|_| {
        SliderState::new()
            .max(max as f32)
            .min(min as f32)
            .step(step.max(1) as f32)
            .default_value(value as f32)
    })
}

fn make_processor_power_slider(cx: &mut Context<PowerLeafApp>, value: u64) -> Entity<SliderState> {
    make_percent_slider(cx, value)
}

fn processor_power_slider_input(
    inputs: &UiInputs,
    slider: ProcessorPowerSlider,
) -> Entity<SliderState> {
    match slider {
        ProcessorPowerSlider::AcCoreParkingMin => {
            inputs.processor_power_ac_core_parking_min.clone()
        }
        ProcessorPowerSlider::AcPerformanceMin => inputs.processor_power_ac_performance_min.clone(),
        ProcessorPowerSlider::AcPerformanceMax => inputs.processor_power_ac_performance_max.clone(),
        ProcessorPowerSlider::DcCoreParkingMin => {
            inputs.processor_power_dc_core_parking_min.clone()
        }
        ProcessorPowerSlider::DcPerformanceMin => inputs.processor_power_dc_performance_min.clone(),
        ProcessorPowerSlider::DcPerformanceMax => inputs.processor_power_dc_performance_max.clone(),
    }
}

fn cpu_threshold_slider_input(
    inputs: &UiInputs,
    slider: CpuThresholdSlider,
) -> Option<Entity<SliderState>> {
    match slider {
        CpuThresholdSlider::Lower(index) => inputs.cpu_rule_thresholds.get(index),
        CpuThresholdSlider::Upper(index) => inputs.cpu_rule_upper_thresholds.get(index),
    }
    .cloned()
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

fn sync_slider_vec(
    inputs: &mut Vec<Entity<SliderState>>,
    len: usize,
    cx: &mut Context<PowerLeafApp>,
    value_at: impl Fn(usize) -> u64,
) {
    while inputs.len() < len {
        let index = inputs.len();
        inputs.push(make_percent_slider(cx, value_at(index)));
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

fn apply_appearance_settings(general: &config::GeneralSettings, window: &mut Window, cx: &mut App) {
    match general.theme_mode {
        AppThemeMode::System => gpui_component::Theme::sync_system_appearance(Some(window), cx),
        AppThemeMode::Light => {
            gpui_component::Theme::change(gpui_component::ThemeMode::Light, Some(window), cx)
        }
        AppThemeMode::Dark => {
            gpui_component::Theme::change(gpui_component::ThemeMode::Dark, Some(window), cx)
        }
    }
    apply_accent_color(&general.accent, cx);
    window.refresh();
}

fn apply_accent_color(settings: &AccentSettings, cx: &mut App) {
    let accent_color = resolve_accent_color(settings);
    UI_ACCENT_COLOR.store(accent_color, Ordering::Relaxed);
    let accent: gpui::Hsla = rgb(accent_color).into();

    let theme = gpui_component::Theme::global_mut(cx);
    let is_dark = theme.is_dark();
    UI_DARK_MODE.store(is_dark, Ordering::Relaxed);
    let foreground = if !is_dark || accent_contrast_prefers_light(accent_color) {
        rgb(0xffffff).into()
    } else {
        rgb(0x111111).into()
    };

    let hover = if is_dark {
        accent.lighten(0.10)
    } else {
        accent.darken(0.10)
    };
    let active = if is_dark {
        accent.darken(0.12)
    } else {
        accent.darken(0.18)
    };

    if is_dark {
        theme.background = rgb(0x202020).into();
        theme.foreground = rgb(COLOR_TEXT).into();
        theme.muted_foreground = rgb(COLOR_MUTED).into();
        theme.title_bar = rgb(0x202020).into();
        theme.title_bar_border = rgb(0x303030).into();
        theme.sidebar = rgb(0x202020).into();
        theme.sidebar_foreground = rgb(COLOR_TEXT).into();
        theme.sidebar_border = rgb(0x303030).into();
        theme.group_box = rgb(COLOR_SETTINGS_CARD).into();
        theme.border = rgb(COLOR_BORDER).into();
        theme.popover = rgb(0x2b2b2b).into();
        theme.popover_foreground = rgb(COLOR_TEXT).into();
        theme.success_foreground = rgb(COLOR_SUCCESS).into();
        theme.danger_foreground = rgb(0xff8a8a).into();
    } else {
        theme.background = rgb(0xf9f9f9).into();
        theme.foreground = rgb(0x1f1f1f).into();
        theme.muted_foreground = rgb(0x616161).into();
        theme.title_bar = rgb(0xf3f3f3).into();
        theme.title_bar_border = rgb(0xe5e5e5).into();
        theme.sidebar = rgb(0xf3f3f3).into();
        theme.sidebar_foreground = rgb(0x1f1f1f).into();
        theme.sidebar_border = rgb(0xe5e5e5).into();
        theme.group_box = rgb(0xffffff).into();
        theme.border = rgb(0xdedede).into();
        theme.popover = rgb(0xffffff).into();
        theme.popover_foreground = rgb(0x1f1f1f).into();
        theme.success_foreground = rgb(0x107c10).into();
        theme.danger_foreground = rgb(0xc42b1c).into();
    }
    theme.primary = accent;
    theme.primary_hover = hover;
    theme.primary_active = active;
    theme.primary_foreground = foreground;
    if is_dark {
        theme.secondary = rgb(0x3a3a3a).into();
        theme.secondary_hover = rgb(0x454545).into();
        theme.secondary_active = rgb(0x505050).into();
        theme.secondary_foreground = rgb(0xf2f2f2).into();
    } else {
        theme.secondary = rgb(0xf3f3f3).into();
        theme.secondary_hover = rgb(0xe9e9e9).into();
        theme.secondary_active = rgb(0xdedede).into();
        theme.secondary_foreground = rgb(0x1f1f1f).into();
    }
    theme.accent = accent;
    theme.accent_foreground = foreground;
    theme.sidebar_accent = accent;
    theme.sidebar_accent_foreground = foreground;
    theme.ring = accent;
    theme.progress_bar = accent;
    theme.slider_thumb = accent;
    theme.caret = accent;
    theme.selection = accent.opacity(0.26);
    theme.input = accent.opacity(0.72);
}

fn resolve_accent_color(settings: &AccentSettings) -> u32 {
    match settings.source {
        AccentColorSource::Windows => windows_switch_accent_color().unwrap_or(COLOR_ACCENT),
        AccentColorSource::Custom => settings.custom_color,
    }
}

fn windows_accent_color() -> Option<u32> {
    read_registry_dword(
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent",
        "AccentColorMenu",
    )
    .or_else(|| read_registry_dword(r"Software\Microsoft\Windows\DWM", "AccentColor"))
    .map(bgr_dword_to_rgb)
    .or_else(|| {
        read_registry_dword(r"Software\Microsoft\Windows\DWM", "ColorizationColor")
            .map(|color| color & 0x00ff_ffff)
    })
    .filter(|color| *color != 0)
}

fn windows_switch_accent_color() -> Option<u32> {
    read_registry_bytes(
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent",
        "AccentPalette",
    )
    .and_then(|palette| accent_palette_rgb(&palette, 1))
    .or_else(windows_accent_color)
}

fn bgr_dword_to_rgb(color: u32) -> u32 {
    let red = color & 0xff;
    let green = (color >> 8) & 0xff;
    let blue = (color >> 16) & 0xff;
    (red << 16) | (green << 8) | blue
}

fn accent_palette_rgb(palette: &[u8], index: usize) -> Option<u32> {
    let offset = index.checked_mul(4)?;
    let red = *palette.get(offset)? as u32;
    let green = *palette.get(offset + 1)? as u32;
    let blue = *palette.get(offset + 2)? as u32;
    Some((red << 16) | (green << 8) | blue).filter(|color| *color != 0)
}

fn accent_contrast_prefers_light(color: u32) -> bool {
    let red = ((color >> 16) & 0xff) as f32;
    let green = ((color >> 8) & 0xff) as f32;
    let blue = (color & 0xff) as f32;
    (0.299 * red + 0.587 * green + 0.114 * blue) < 140.0
}

fn accent_color() -> u32 {
    UI_ACCENT_COLOR.load(Ordering::Relaxed)
}

fn ui_is_dark() -> bool {
    UI_DARK_MODE.load(Ordering::Relaxed)
}

fn settings_card_color() -> u32 {
    if ui_is_dark() {
        COLOR_SETTINGS_CARD
    } else {
        0xffffff
    }
}

fn settings_card_hover_color() -> u32 {
    if ui_is_dark() {
        COLOR_SETTINGS_CARD_HOVER
    } else {
        0xf5f5f5
    }
}

fn windows_slider_thumb_color() -> u32 {
    if ui_is_dark() {
        0xd9d9d9
    } else {
        0xffffff
    }
}

fn disabled_slider_track_color() -> u32 {
    if ui_is_dark() {
        0x4a4a4a
    } else {
        0xd0d0d0
    }
}

fn disabled_slider_thumb_color() -> u32 {
    if ui_is_dark() {
        0x707070
    } else {
        0xf2f2f2
    }
}

fn border_color() -> u32 {
    if ui_is_dark() {
        COLOR_BORDER
    } else {
        0xdedede
    }
}

fn primary_text_color() -> u32 {
    if ui_is_dark() {
        COLOR_TEXT
    } else {
        0x1f1f1f
    }
}

fn muted_text_color() -> u32 {
    if ui_is_dark() {
        COLOR_MUTED
    } else {
        0x616161
    }
}

fn dim_text_color() -> u32 {
    if ui_is_dark() {
        COLOR_DIM
    } else {
        0x777777
    }
}

fn sidebar_selected_color() -> u32 {
    if ui_is_dark() {
        COLOR_SIDEBAR_SELECTED
    } else {
        0xeaeaea
    }
}

fn sidebar_hover_color() -> u32 {
    if ui_is_dark() {
        COLOR_SIDEBAR_HOVER
    } else {
        0xf5f5f5
    }
}

fn panel_active_color() -> u32 {
    if ui_is_dark() {
        COLOR_PANEL_ACTIVE
    } else {
        0xf3f3f3
    }
}

fn success_bg_color() -> u32 {
    if ui_is_dark() {
        COLOR_SUCCESS_BG
    } else {
        0xe7f3df
    }
}

fn success_text_color() -> u32 {
    if ui_is_dark() {
        COLOR_SUCCESS
    } else {
        0x0f6c0f
    }
}

fn warning_bg_color() -> u32 {
    if ui_is_dark() {
        COLOR_WARNING_BG
    } else {
        0xfff4ce
    }
}

fn warning_text_color() -> u32 {
    if ui_is_dark() {
        COLOR_WARNING
    } else {
        0x8a6d1d
    }
}

fn accent_glyph_color(accent: u32) -> u32 {
    if !ui_is_dark() || accent_contrast_prefers_light(accent) {
        0xffffff
    } else {
        0x111111
    }
}

fn switch_accent_color() -> u32 {
    accent_color()
}

fn read_registry_dword(sub_key: &str, value_name: &str) -> Option<u32> {
    read_registry_dword_root(HKEY_CURRENT_USER, sub_key, value_name)
}

fn read_registry_dword_root(root: HKEY, sub_key: &str, value_name: &str) -> Option<u32> {
    let sub_key = wide_null(sub_key);
    let value_name = wide_null(value_name);
    let mut key: HKEY = null_mut();
    let status = unsafe { RegOpenKeyExW(root, sub_key.as_ptr(), 0, KEY_QUERY_VALUE, &mut key) };
    if status != ERROR_SUCCESS {
        return None;
    }

    let key = RegistryKey(key);
    let mut value_type = 0;
    let mut value = 0_u32;
    let mut value_size = size_of::<u32>() as u32;
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            value_name.as_ptr(),
            null_mut(),
            &mut value_type,
            &mut value as *mut u32 as *mut u8,
            &mut value_size,
        )
    };

    if status == ERROR_SUCCESS && value_type == REG_DWORD && value_size == size_of::<u32>() as u32 {
        Some(value)
    } else {
        None
    }
}

fn write_registry_dword_root(
    root: HKEY,
    sub_key: &str,
    value_name: &str,
    value: u32,
) -> Result<(), String> {
    let sub_key = wide_null(sub_key);
    let value_name = wide_null(value_name);
    let mut key: HKEY = null_mut();
    let status = unsafe { RegOpenKeyExW(root, sub_key.as_ptr(), 0, KEY_SET_VALUE, &mut key) };
    if status != ERROR_SUCCESS {
        return Err(registry_error_message(
            "open registry key for write",
            status,
        ));
    }

    let key = RegistryKey(key);
    let status = unsafe {
        RegSetValueExW(
            key.0,
            value_name.as_ptr(),
            0,
            REG_DWORD,
            &value as *const u32 as *const u8,
            size_of::<u32>() as u32,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(registry_error_message("write registry value", status))
    }
}

fn write_registry_dword_create_root(
    root: HKEY,
    sub_key: &str,
    value_name: &str,
    value: u32,
) -> Result<(), String> {
    let sub_key = wide_null(sub_key);
    let value_name = wide_null(value_name);
    let mut key: HKEY = null_mut();
    let mut disposition = 0_u32;
    let status = unsafe {
        RegCreateKeyExW(
            root,
            sub_key.as_ptr(),
            0,
            null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            null_mut(),
            &mut key,
            &mut disposition,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(registry_error_message(
            "create registry key for backup",
            status,
        ));
    }

    let key = RegistryKey(key);
    let status = unsafe {
        RegSetValueExW(
            key.0,
            value_name.as_ptr(),
            0,
            REG_DWORD,
            &value as *const u32 as *const u8,
            size_of::<u32>() as u32,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(registry_error_message("write registry backup", status))
    }
}

fn read_win32_priority_separation() -> Option<u32> {
    read_registry_dword_root(
        HKEY_LOCAL_MACHINE,
        WIN32_PRIORITY_CONTROL_SUB_KEY,
        WIN32_PRIORITY_SEPARATION_VALUE,
    )
}

fn read_win32_priority_separation_with_status() -> (Option<u32>, String) {
    match read_win32_priority_separation() {
        Some(value) => (
            Some(value),
            t!(
                "settings.win32_priority_separation_loaded",
                value = format_win32_priority_separation_with_description(value)
            )
            .to_string(),
        ),
        None => (
            None,
            t!("settings.win32_priority_separation_load_failed").to_string(),
        ),
    }
}

fn write_win32_priority_separation(value: u32) -> Result<(), String> {
    write_registry_dword_root(
        HKEY_LOCAL_MACHINE,
        WIN32_PRIORITY_CONTROL_SUB_KEY,
        WIN32_PRIORITY_SEPARATION_VALUE,
        value,
    )
}

fn read_win32_priority_separation_backup() -> Option<u32> {
    read_registry_dword_root(
        HKEY_CURRENT_USER,
        POWERLEAF_REGISTRY_SUB_KEY,
        WIN32_PRIORITY_SEPARATION_BACKUP_VALUE,
    )
}

fn write_win32_priority_separation_backup(value: u32) -> Result<(), String> {
    write_registry_dword_create_root(
        HKEY_CURRENT_USER,
        POWERLEAF_REGISTRY_SUB_KEY,
        WIN32_PRIORITY_SEPARATION_BACKUP_VALUE,
        value,
    )
}

fn format_win32_priority_separation(value: u32) -> String {
    format!("0x{value:02X} ({value})")
}

fn format_win32_priority_separation_with_description(value: u32) -> String {
    format!(
        "{} - {}",
        format_win32_priority_separation(value),
        win32_priority_separation_description(value)
    )
}

fn win32_priority_separation_description(value: u32) -> String {
    match value {
        0x14 => t!("settings.win32_priority_separation_desc_long_variable_none").to_string(),
        0x15 => t!("settings.win32_priority_separation_desc_long_variable_medium").to_string(),
        0x16 => t!("settings.win32_priority_separation_desc_long_variable_high").to_string(),
        0x18 => t!("settings.win32_priority_separation_desc_long_fixed_none").to_string(),
        0x19 => t!("settings.win32_priority_separation_desc_long_fixed_medium").to_string(),
        0x1A => t!("settings.win32_priority_separation_desc_long_fixed_high").to_string(),
        0x24 => t!("settings.win32_priority_separation_desc_short_variable_none").to_string(),
        0x25 => t!("settings.win32_priority_separation_desc_short_variable_medium").to_string(),
        0x26 => t!("settings.win32_priority_separation_desc_short_variable_high").to_string(),
        0x28 => t!("settings.win32_priority_separation_desc_short_fixed_none").to_string(),
        0x29 => t!("settings.win32_priority_separation_desc_short_fixed_medium").to_string(),
        0x2A => t!("settings.win32_priority_separation_desc_short_fixed_high").to_string(),
        _ => t!("settings.win32_priority_separation_desc_custom").to_string(),
    }
}

fn normalize_win32_priority_separation_value(value: u32) -> u32 {
    win32_priority_separation_field_bits(value, Win32PrioritySeparationField::QuantumDuration)
        | win32_priority_separation_field_bits(
            value,
            Win32PrioritySeparationField::QuantumBehaviour,
        )
        | win32_priority_separation_field_bits(value, Win32PrioritySeparationField::ForegroundBoost)
}

fn win32_priority_separation_field_bits(value: u32, field: Win32PrioritySeparationField) -> u32 {
    match field {
        Win32PrioritySeparationField::QuantumDuration => match value & 0x30 {
            0x10 | 0x20 => value & 0x30,
            _ => 0x20,
        },
        Win32PrioritySeparationField::QuantumBehaviour => match value & 0x0C {
            0x04 | 0x08 => value & 0x0C,
            _ => 0x04,
        },
        Win32PrioritySeparationField::ForegroundBoost => match value & 0x03 {
            0x00 | 0x01 | 0x02 => value & 0x03,
            _ => 0x02,
        },
    }
}

fn win32_priority_separation_field_picker_id(field: Win32PrioritySeparationField) -> &'static str {
    match field {
        Win32PrioritySeparationField::QuantumDuration => {
            "win32-priority-separation-quantum-duration"
        }
        Win32PrioritySeparationField::QuantumBehaviour => {
            "win32-priority-separation-quantum-behaviour"
        }
        Win32PrioritySeparationField::ForegroundBoost => {
            "win32-priority-separation-foreground-boost"
        }
    }
}

fn win32_priority_separation_field_options(
    field: Win32PrioritySeparationField,
) -> Vec<Win32PrioritySeparationFieldOption> {
    match field {
        Win32PrioritySeparationField::QuantumDuration => vec![
            Win32PrioritySeparationFieldOption { bits: 0x20 },
            Win32PrioritySeparationFieldOption { bits: 0x10 },
        ],
        Win32PrioritySeparationField::QuantumBehaviour => vec![
            Win32PrioritySeparationFieldOption { bits: 0x04 },
            Win32PrioritySeparationFieldOption { bits: 0x08 },
        ],
        Win32PrioritySeparationField::ForegroundBoost => vec![
            Win32PrioritySeparationFieldOption { bits: 0x00 },
            Win32PrioritySeparationFieldOption { bits: 0x01 },
            Win32PrioritySeparationFieldOption { bits: 0x02 },
        ],
    }
}

fn win32_priority_separation_field_option_label(
    field: Win32PrioritySeparationField,
    bits: u32,
) -> String {
    match (field, bits) {
        (Win32PrioritySeparationField::QuantumDuration, 0x20) => {
            t!("settings.win32_priority_separation_quantum_duration_short").to_string()
        }
        (Win32PrioritySeparationField::QuantumDuration, 0x10) => {
            t!("settings.win32_priority_separation_quantum_duration_long").to_string()
        }
        (Win32PrioritySeparationField::QuantumBehaviour, 0x04) => {
            t!("settings.win32_priority_separation_quantum_behaviour_variable").to_string()
        }
        (Win32PrioritySeparationField::QuantumBehaviour, 0x08) => {
            t!("settings.win32_priority_separation_quantum_behaviour_fixed").to_string()
        }
        (Win32PrioritySeparationField::ForegroundBoost, 0x00) => {
            t!("settings.win32_priority_separation_foreground_boost_none").to_string()
        }
        (Win32PrioritySeparationField::ForegroundBoost, 0x01) => {
            t!("settings.win32_priority_separation_foreground_boost_medium").to_string()
        }
        (Win32PrioritySeparationField::ForegroundBoost, 0x02) => {
            t!("settings.win32_priority_separation_foreground_boost_high").to_string()
        }
        _ => t!("settings.win32_priority_separation_unavailable").to_string(),
    }
}

fn win32_priority_separation_field_label(
    field: Win32PrioritySeparationField,
    value: u32,
) -> String {
    let selected_bits = win32_priority_separation_field_bits(value, field);
    win32_priority_separation_field_options(field)
        .into_iter()
        .find(|option| option.bits == selected_bits)
        .map(|option| win32_priority_separation_field_option_label(field, option.bits))
        .unwrap_or_else(|| t!("settings.win32_priority_separation_unavailable").to_string())
}

fn registry_error_message(action: &str, status: u32) -> String {
    format!("Failed to {action}: Windows error {status}.")
}

fn read_registry_bytes(sub_key: &str, value_name: &str) -> Option<Vec<u8>> {
    let sub_key = wide_null(sub_key);
    let value_name = wide_null(value_name);
    let mut key: HKEY = null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            sub_key.as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut key,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }

    let key = RegistryKey(key);
    let mut value_type = 0;
    let mut value_size = 0_u32;
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            value_name.as_ptr(),
            null_mut(),
            &mut value_type,
            null_mut(),
            &mut value_size,
        )
    };
    if status != ERROR_SUCCESS || value_type != REG_BINARY || value_size == 0 {
        return None;
    }

    let mut value = vec![0_u8; value_size as usize];
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            value_name.as_ptr(),
            null_mut(),
            &mut value_type,
            value.as_mut_ptr(),
            &mut value_size,
        )
    };

    if status == ERROR_SUCCESS && value_type == REG_BINARY {
        value.truncate(value_size as usize);
        Some(value)
    } else {
        None
    }
}

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn apply_language(language: AppLanguage) {
    rust_i18n::set_locale(language.locale());
}

fn breadcrumb_button(
    id: SharedString,
    target: Page,
    label: String,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Stateful<gpui::Div> {
    let hover_bg: Hsla = rgb(settings_card_hover_color()).into();

    h_flex()
        .id(id)
        .min_w(px(0.0))
        .max_w(px(360.0))
        .items_center()
        .px_1()
        .py(px(2.0))
        .rounded_sm()
        .text_size(px(TEXT_PAGE_TITLE_SIZE))
        .line_height(px(TEXT_PAGE_TITLE_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .opacity(0.68)
        .hover(move |style| style.bg(hover_bg))
        .cursor_pointer()
        .child(div().min_w(px(0.0)).truncate().child(label))
        .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
            app.navigate_to(target, cx);
        }))
}

fn breadcrumb_separator() -> gpui::Div {
    div()
        .text_size(px(TEXT_PAGE_CRUMB_SIZE))
        .line_height(px(TEXT_PAGE_CRUMB_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(dim_text_color()))
        .opacity(0.48)
        .child(Icon::new(NavIcon::ChevronRight).with_size(px(16.0)))
}

fn dashboard_sections_in_nav_order() -> Vec<&'static ui::PageSection> {
    Page::sections()
        .iter()
        .filter(|section| {
            section.landing_page != Page::Dashboard && !nav_section_in_footer(section.landing_page)
        })
        .chain(
            Page::sections()
                .iter()
                .filter(|section| nav_section_in_footer(section.landing_page)),
        )
        .collect()
}

fn dashboard_search_pages(query: &str) -> Vec<Page> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }

    let mut pages = Vec::new();
    let mut seen = HashSet::new();

    for section in dashboard_sections_in_nav_order() {
        let section_matches = dashboard_page_matches_query(section.landing_page, &query);

        for page in section.pages.iter().copied() {
            if page == Page::Dashboard || !seen.insert(page) {
                continue;
            }

            if section_matches || dashboard_page_matches_query(page, &query) {
                pages.push(page);
            }
        }
    }

    pages
}

fn dashboard_page_matches_query(page: Page, query: &str) -> bool {
    let text = dashboard_page_search_text(page).to_lowercase();
    query.split_whitespace().all(|term| text.contains(term))
}

fn dashboard_page_search_text(page: Page) -> String {
    let mut text = format!("{} {}", page.label(), page.section_label());

    let extra = match page {
        Page::Dashboard => vec![
            t!("dashboard.intro_1").to_string(),
            t!("dashboard.intro_2").to_string(),
            "overview summary current automation decision power plan cpu enabled rules".to_string(),
        ],
        Page::PowerPlanAutomation => vec![
            "power plan automation foreground focused app running app performance mode cpu load activity idle schedule time battery plugged ac dc".to_string(),
        ],
        Page::ProcessorControls => vec![
            "processor cpu controls core parking limiter background restriction affinity steering power boost ac dc battery e cores p cores".to_string(),
        ],
        Page::ProcessPolicies => vec![
            "process policies efficiency mode io priority watchdog terminate restart background foreground".to_string(),
        ],
        Page::MemoryControl => vec![
            "memory control memory priority smarttrim ram trim working set standby list file cache paging background process".to_string(),
        ],
        Page::ActionLog => vec![
            t!("action_log.intro_1").to_string(),
            t!("action_log.intro_2").to_string(),
            "log action history details csv export skipped failed applied restored reason".to_string(),
        ],
        Page::AppHome => vec![
            "settings powerleaf behaviour startup tray toggles action log detail fail suppression appearance language theme accent color palette about".to_string(),
        ],
        Page::AdvancedHome => vec![
            "advanced app suspension windows scheduler win32 priority separation quantum foreground boost registry".to_string(),
        ],
        Page::Activity => vec![
            t!("activity.intro_1").to_string(),
            t!("activity.intro_2").to_string(),
            t!("activity.enable").to_string(),
            "idle active input keyboard mouse activity power plan battery plugged".to_string(),
        ],
        Page::ForegroundRules => vec![
            t!("foreground.intro_1").to_string(),
            t!("foreground.intro_2").to_string(),
            t!("foreground.enable").to_string(),
            "foreground focused app process window power plan priority rule".to_string(),
        ],
        Page::Schedule => vec![
            t!("schedule.intro_1").to_string(),
            t!("schedule.intro_2").to_string(),
            t!("schedule.enable").to_string(),
            "time schedule clock date weekday overnight power plan".to_string(),
        ],
        Page::CpuUsage => vec![
            t!("cpu_rules.intro_1").to_string(),
            t!("cpu_rules.intro_2").to_string(),
            t!("cpu_rules.enable").to_string(),
            "cpu load usage threshold sustained power plan percent samples".to_string(),
        ],
        Page::CoreParking => vec![
            t!("processor_power.help").to_string(),
            t!("processor_power.link_ac_dc_help").to_string(),
            t!("processor_power.performance_help").to_string(),
            t!("processor_power.balanced_help").to_string(),
            t!("processor_power.saver_help").to_string(),
            "core parking processor power boost min max ac dc battery plugged performance saver balanced".to_string(),
        ],
        Page::CpuLimiter => vec![
            t!("cpu_limiter.intro_1").to_string(),
            t!("cpu_limiter.intro_2").to_string(),
            t!("cpu_limiter.intro_3").to_string(),
            t!("cpu_limiter.focus_detection_help").to_string(),
            t!("cpu_limiter.rules_help").to_string(),
            "cpu limiter limit core affinity threshold sustain cooldown background process".to_string(),
        ],
        Page::BackgroundCpuRestriction => vec![
            t!("background_cpu.intro_1").to_string(),
            t!("background_cpu.intro_2").to_string(),
            t!("background_cpu.focus_detection_help").to_string(),
            t!("background_cpu.exclusions_help").to_string(),
            "background cpu restriction cpu set affinity limit e cores foreground exclusion".to_string(),
        ],
        Page::EfficiencyMode => vec![
            t!("efficiency.intro_1").to_string(),
            t!("efficiency.intro_2").to_string(),
            t!("efficiency.intro_3").to_string(),
            t!("efficiency.focus_detection_help").to_string(),
            t!("efficiency.whitelist_help").to_string(),
            "efficiency mode ecoqos qos throttle background priority cpu set e cores exclusion whitelist".to_string(),
        ],
        Page::AppSuspension => vec![
            t!("suspension.intro_1").to_string(),
            t!("suspension.intro_2").to_string(),
            t!("suspension.intro_3").to_string(),
            t!("suspension.suspendable_help").to_string(),
            "suspend freeze thaw resume background app process job object delay network audio".to_string(),
        ],
        Page::Watchdog => vec![
            t!("watchdog.intro_1").to_string(),
            t!("watchdog.intro_2").to_string(),
            t!("watchdog.intro_3").to_string(),
            t!("watchdog.rules_help").to_string(),
            "watchdog terminate close kill launch restart relaunch process disappeared appeared".to_string(),
        ],
        Page::PerformanceMode => vec![
            t!("performance_mode.intro_1").to_string(),
            t!("performance_mode.intro_2").to_string(),
            t!("performance_mode.intro_3").to_string(),
            t!("performance_mode.rules_help").to_string(),
            "running app performance mode power plan process game gaming active restore".to_string(),
        ],
        Page::ForegroundResponsiveness => vec![
            t!("responsiveness.intro_1").to_string(),
            t!("responsiveness.intro_2").to_string(),
            t!("responsiveness.intro_3").to_string(),
            t!("responsiveness.lower_background_efficiency_help").to_string(),
            t!("responsiveness.auto_balance_preset_help").to_string(),
            t!("responsiveness.foreground_boost_help").to_string(),
            "responsiveness auto balance probalance foreground boost background priority cpu spike stutter".to_string(),
        ],
        Page::IoPriority => vec![
            t!("io_priority.intro_1").to_string(),
            t!("io_priority.intro_2").to_string(),
            t!("io_priority.exclude_foreground_help").to_string(),
            "io i/o disk storage priority low very low background process foreground exclusion".to_string(),
        ],
        Page::MemoryPriority => vec![
            t!("memory_priority.intro_1").to_string(),
            t!("memory_priority.intro_2").to_string(),
            t!("memory_priority.exclude_foreground_help").to_string(),
            "memory priority page priority ram paging working set very low low medium background process foreground exclusion".to_string(),
        ],
        Page::SmartTrim => vec![
            t!("smart_trim.intro_1").to_string(),
            t!("smart_trim.intro_2").to_string(),
            t!("smart_trim.intro_3").to_string(),
            t!("smart_trim.trim_working_sets_help").to_string(),
            t!("smart_trim.purge_standby_list_help").to_string(),
            t!("smart_trim.purge_system_file_cache_help").to_string(),
            "memory ram trim working set standby list file cache purge background exclusion".to_string(),
        ],
        Page::CpuAffinity => vec![
            t!("affinity.intro_1").to_string(),
            t!("affinity.intro_2").to_string(),
            t!("affinity.intro_3").to_string(),
            t!("affinity.rules_help").to_string(),
            t!("affinity.p_cores_help").to_string(),
            t!("affinity.e_cores_help").to_string(),
            t!("affinity.no_smt_help").to_string(),
            "core steering affinity cpu sets p cores e cores smt logical processor background process".to_string(),
        ],
        Page::Settings => vec![
            t!("settings.intro_1").to_string(),
            t!("settings.intro_2").to_string(),
            t!("settings.action_log_mode_full_help").to_string(),
            t!("settings.failure_suppression_threshold_help").to_string(),
            "powerleaf behaviour startup tray automation toggle action log detail fail failure suppression export import".to_string(),
        ],
        Page::SettingsAppearance => vec![
            "language appearance theme dark light system accent color palette localization display ui".to_string(),
        ],
        Page::Win32PrioritySeparation => vec![
            t!("settings.win32_priority_separation_quantum_duration_help").to_string(),
            t!("settings.win32_priority_separation_quantum_behaviour_help").to_string(),
            t!("settings.win32_priority_separation_foreground_boost_help").to_string(),
            "win32 priority separation windows scheduler quantum foreground boost games gaming registry".to_string(),
        ],
        Page::About => vec![
            t!("about.intro_1").to_string(),
            t!("about.intro_2").to_string(),
            "about version project powerleaf".to_string(),
        ],
    };

    for value in extra {
        text.push(' ');
        text.push_str(&value);
    }

    text
}

fn nav_section_in_footer(page: Page) -> bool {
    matches!(page, Page::ActionLog | Page::AppHome)
}

fn page_shell(page: Page, cx: &mut Context<PowerLeafApp>) -> gpui::Div {
    page_shell_with_help(page, None, cx)
}

fn search_results_page_shell(_cx: &mut Context<PowerLeafApp>) -> gpui::Div {
    let header = h_flex()
        .w_full()
        .min_h(px(PAGE_HEADER_HEIGHT))
        .flex_shrink_0()
        .items_center()
        .overflow_hidden()
        .child(
            div()
                .min_w(px(0.0))
                .text_size(px(TEXT_PAGE_TITLE_SIZE))
                .line_height(px(TEXT_PAGE_TITLE_LINE_HEIGHT))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .truncate()
                .child(t!("dashboard.search_results").to_string()),
        );

    v_flex().w_full().min_w(px(0.0)).gap_2().child(header)
}

fn page_content_frame(page: AnyElement) -> gpui::Div {
    h_flex()
        .w_full()
        .min_w(px(0.0))
        .justify_center()
        .px(px(24.0))
        .py(px(24.0))
        .child(
            div()
                .w_full()
                .max_w(px(CONTENT_MAX_WIDTH))
                .min_w(px(0.0))
                .child(page),
        )
}

fn page_shell_with_help(
    page: Page,
    help: Option<SharedString>,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Div {
    let mut header = h_flex()
        .w_full()
        .min_h(px(PAGE_HEADER_HEIGHT))
        .flex_shrink_0()
        .items_center()
        .gap_2()
        .overflow_hidden();

    if page == Page::Dashboard {
        header = header.child(
            div()
                .min_w(px(0.0))
                .text_size(px(TEXT_PAGE_TITLE_SIZE))
                .line_height(px(TEXT_PAGE_TITLE_LINE_HEIGHT))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .truncate()
                .child(page.label()),
        );
    } else {
        let section_page = page.section_landing_page();
        header = header
            .child(breadcrumb_button(
                SharedString::from(format!("breadcrumb-home-{page:?}")),
                Page::Dashboard,
                Page::Dashboard.label(),
                cx,
            ))
            .child(breadcrumb_separator());

        if page != section_page {
            header = header
                .child(breadcrumb_button(
                    SharedString::from(format!("breadcrumb-section-{page:?}")),
                    section_page,
                    page.section_label(),
                    cx,
                ))
                .child(breadcrumb_separator());
        }

        header = header.child(
            div()
                .min_w(px(0.0))
                .text_size(px(TEXT_PAGE_TITLE_SIZE))
                .line_height(px(TEXT_PAGE_TITLE_LINE_HEIGHT))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .truncate()
                .child(page.label()),
        );
    }

    if let Some(help) = help {
        header = header.child(title_info_button(
            SharedString::from(format!("page-info-{page:?}")),
            help,
        ));
    }

    v_flex().w_full().min_w(px(0.0)).gap_2().child(header)
}

fn tooltip_lines(lines: impl IntoIterator<Item = impl Into<SharedString>>) -> SharedString {
    let mut tooltip = String::new();
    for line in lines {
        let line: SharedString = line.into();
        if !tooltip.is_empty() {
            tooltip.push('\n');
        }
        tooltip.push_str(line.as_ref());
    }
    tooltip.into()
}

fn section_card(title: &str) -> GroupBox {
    GroupBox::new()
        .outline()
        .title(section_title_label(title.to_owned()))
}

fn section_header(title: &str, help: impl Into<SharedString>) -> gpui::Div {
    let help = help.into();

    v_flex().w_full().min_w(px(0.0)).child(
        h_flex()
            .w_full()
            .min_h(px(26.0))
            .min_w(px(0.0))
            .items_center()
            .gap_1()
            .child(section_title_text(title.to_owned()))
            .child(title_info_button(
                SharedString::from(format!("section-info-{title}")),
                help,
            )),
    )
}

fn section_title_label(title: impl Into<SharedString>) -> Label {
    Label::new(title)
        .w_full()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::BOLD)
}

fn section_title_text(title: impl Into<SharedString>) -> Label {
    Label::new(title)
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::BOLD)
}

fn title_info_button(id: impl Into<SharedString>, tooltip: impl Into<SharedString>) -> AnyElement {
    div()
        .size(px(26.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .child(
            Button::new(id.into())
                .ghost()
                .rounded(px(999.0))
                .with_size(px(26.0))
                .icon(
                    Icon::new(NavIcon::Info)
                        .with_size(px(14.0))
                        .text_color(rgb(dim_text_color())),
                )
                .tooltip(tooltip),
        )
        .into_any_element()
}

fn rule_card(
    title: AnyElement,
    leading: AnyElement,
    collapse_indicator: AnyElement,
    card_target: RuleCardTarget,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Stateful<gpui::Div> {
    rule_card_with_header_action(title, leading, None, collapse_indicator, card_target, cx)
}

fn rule_card_with_header_action(
    title: AnyElement,
    leading: AnyElement,
    header_action: Option<AnyElement>,
    collapse_indicator: AnyElement,
    card_target: RuleCardTarget,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Stateful<gpui::Div> {
    let header_padding = if header_action.is_some() {
        px(134.0)
    } else {
        px(52.0)
    };
    let card_id = SharedString::from(format!("rule-card-{card_target:?}"));
    let header_id = SharedString::from(format!("rule-card-header-{card_target:?}"));
    let header_action_id = SharedString::from(format!("rule-card-header-action-{card_target:?}"));
    let header_card_target = card_target.clone();
    let trailing_card_target = card_target.clone();
    let mut trailing = h_flex()
        .id(SharedString::from(format!(
            "rule-card-trailing-{card_target:?}"
        )))
        .absolute()
        .top(px(0.0))
        .right(px(0.0))
        .h(px(58.0))
        .items_center()
        .gap_1()
        .px_2()
        .block_mouse_except_scroll()
        .cursor_pointer()
        .on_click(cx.listener(move |app, _, _, cx| {
            app.toggle_rule_card(trailing_card_target.clone(), cx);
        }));
    if let Some(header_action) = header_action {
        trailing = trailing.child(header_action);
    }
    trailing = trailing.child(collapse_indicator);

    v_flex()
        .id(card_id)
        .w_full()
        .min_w(px(0.0))
        .overflow_hidden()
        .rounded_sm()
        .border_1()
        .border_color(rgb(border_color()))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .child(
            div()
                .relative()
                .w_full()
                .min_w(px(0.0))
                .min_h(px(58.0))
                .id(header_id)
                .child(
                    h_flex()
                        .w_full()
                        .min_w(px(0.0))
                        .h(px(58.0))
                        .items_center()
                        .gap_2()
                        .pl_4()
                        .pr(header_padding)
                        .id(header_action_id)
                        .block_mouse_except_scroll()
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(settings_card_hover_color())))
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.toggle_rule_card(header_card_target.clone(), cx);
                        }))
                        .child(leading)
                        .child(title),
                )
                .child(trailing),
        )
}

fn rule_card_collapse_indicator(collapsed: bool) -> AnyElement {
    let icon = if collapsed {
        NavIcon::ChevronRight
    } else {
        NavIcon::ChevronDown
    };

    div()
        .w(px(28.0))
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(dim_text_color()))
        .opacity(0.72)
        .child(Icon::new(icon).with_size(px(16.0)))
        .into_any_element()
}

fn rule_list() -> gpui::Div {
    v_flex().w_full().min_w(px(0.0)).gap_2()
}

fn feature_body(enabled: bool) -> gpui::Div {
    v_flex()
        .w_full()
        .min_w(px(0.0))
        .gap_2()
        .relative()
        .when(!enabled, |body| body.opacity(0.42).cursor_default())
}

fn disabled_feature_body(body: gpui::Div, enabled: bool) -> gpui::Div {
    body.when(!enabled, |body| body.child(disabled_interaction_shield()))
}

fn disabled_interaction_shield() -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .cursor_default()
        .capture_any_mouse_down(|_, _, cx| cx.stop_propagation())
        .capture_any_mouse_up(|_, _, cx| cx.stop_propagation())
        .into_any_element()
}

fn rule_card_body_row(children: Vec<AnyElement>) -> gpui::Div {
    let mut row = v_flex().w_full().min_w(px(0.0));
    for child in children {
        row = row.child(child);
    }
    row
}

fn rule_card_body_action(action: AnyElement) -> gpui::Div {
    rule_card_body_actions(vec![action])
}

fn rule_card_body_actions(actions: Vec<AnyElement>) -> gpui::Div {
    let mut row = h_flex().items_center().justify_end().gap_2();
    for action in actions {
        row = row.child(action);
    }

    h_flex()
        .w_full()
        .min_w(px(0.0))
        .min_h(px(58.0))
        .items_center()
        .justify_end()
        .gap_2()
        .border_t_1()
        .border_color(rgb(border_color()))
        .px_4()
        .py_3()
        .child(row)
}

fn rename_rule_button(target: RuleTitleTarget, cx: &mut Context<PowerLeafApp>) -> AnyElement {
    Button::new(SharedString::from(format!("rename-rule-{target:?}")))
        .small()
        .label("Rename")
        .tooltip(t!("common.rename_rule").to_string())
        .on_click(cx.listener(move |app, _, window, cx| {
            app.begin_rule_title_edit(target, window, cx);
        }))
        .into_any_element()
}

fn compact_rule_row(_cx: &mut Context<PowerLeafApp>) -> gpui::Div {
    h_flex()
        .w_full()
        .min_w(px(0.0))
        .min_h(px(58.0))
        .items_center()
        .justify_between()
        .gap_2()
        .py_3()
        .px_4()
        .rounded_sm()
        .border_1()
        .border_color(rgb(border_color()))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .hover(|style| style.bg(rgb(settings_card_hover_color())))
}

fn create_rule_card(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    action: AnyElement,
) -> gpui::Stateful<gpui::Div> {
    setting_action_card(id, title, action)
}

fn setting_group(
    target: SettingGroupTarget,
    title: impl Into<SharedString>,
    action: AnyElement,
    collapsed: bool,
    rows: Vec<AnyElement>,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Stateful<gpui::Div> {
    let title: SharedString = title.into();
    setting_group_with_title_element(
        target,
        div()
            .flex_1()
            .min_w(px(0.0))
            .truncate()
            .child(title)
            .into_any_element(),
        action,
        collapsed,
        rows,
        cx,
    )
}

fn setting_group_with_help(
    target: SettingGroupTarget,
    title: impl Into<SharedString>,
    help: impl Into<SharedString>,
    action: AnyElement,
    collapsed: bool,
    rows: Vec<AnyElement>,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Stateful<gpui::Div> {
    let title: SharedString = title.into();
    setting_group_with_title_element(
        target,
        h_flex()
            .flex_1()
            .min_w(px(0.0))
            .gap_1()
            .items_center()
            .child(div().truncate().child(title))
            .child(title_info_button(
                SharedString::from(format!("setting-group-info-{target:?}")),
                help,
            ))
            .into_any_element(),
        action,
        collapsed,
        rows,
        cx,
    )
}

fn setting_group_with_title_element(
    target: SettingGroupTarget,
    title: AnyElement,
    action: AnyElement,
    collapsed: bool,
    rows: Vec<AnyElement>,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Stateful<gpui::Div> {
    let chevron_target = target;
    let mut group = v_flex()
        .id(SharedString::from(format!("setting-group-{target:?}")))
        .w_full()
        .min_w(px(0.0))
        .overflow_hidden()
        .rounded_sm()
        .border_1()
        .border_color(rgb(border_color()))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .child(
            h_flex()
                .id(SharedString::from(format!(
                    "setting-group-header-{target:?}"
                )))
                .w_full()
                .min_w(px(0.0))
                .min_h(px(58.0))
                .items_center()
                .justify_between()
                .gap_2()
                .py_3()
                .pl_4()
                .pr_2()
                .block_mouse_except_scroll()
                .cursor_pointer()
                .hover(|style| style.bg(rgb(settings_card_hover_color())))
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.toggle_setting_group(target, cx);
                }))
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "setting-group-title-{target:?}"
                        )))
                        .flex_1()
                        .min_w(px(0.0))
                        .child(title),
                )
                .child(
                    h_flex()
                        .items_center()
                        .justify_end()
                        .gap_1()
                        .min_w(px(0.0))
                        .flex_shrink_0()
                        .child(action)
                        .child(setting_group_collapse_button(chevron_target, collapsed, cx)),
                ),
        );
    if !collapsed {
        for row in rows {
            group = group.child(row);
        }
    }
    group
}

fn setting_group_collapse_button(
    target: SettingGroupTarget,
    collapsed: bool,
    _cx: &mut Context<PowerLeafApp>,
) -> AnyElement {
    let icon = if collapsed {
        NavIcon::ChevronRight
    } else {
        NavIcon::ChevronDown
    };

    div()
        .id(SharedString::from(format!(
            "setting-group-chevron-{target:?}"
        )))
        .w(px(28.0))
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .rounded_sm()
        .text_color(rgb(dim_text_color()))
        .opacity(0.72)
        .hover(|style| style.opacity(1.0))
        .child(Icon::new(icon).with_size(px(16.0)))
        .into_any_element()
}

fn setting_group_switch_action(
    id: impl Into<SharedString>,
    enabled: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    switch_toggle_action(id, enabled, handler)
}

fn setting_group_action_row(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    action: AnyElement,
    divided: bool,
) -> gpui::Stateful<gpui::Div> {
    h_flex()
        .id(id.into())
        .w_full()
        .min_w(px(0.0))
        .min_h(px(58.0))
        .items_center()
        .justify_between()
        .gap_2()
        .py_3()
        .px_4()
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .when(divided, |row| {
            row.border_t_1().border_color(rgb(border_color()))
        })
        .child(div().flex_1().min_w(px(0.0)).truncate().child(title.into()))
        .child(
            h_flex()
                .items_center()
                .justify_end()
                .gap_2()
                .min_w(px(0.0))
                .flex_shrink_0()
                .child(action),
        )
}

fn setting_group_stacked_action_row(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    action: AnyElement,
    divided: bool,
) -> gpui::Stateful<gpui::Div> {
    v_flex()
        .id(id.into())
        .w_full()
        .min_w(px(0.0))
        .gap_2()
        .py_3()
        .px_4()
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .when(divided, |row| {
            row.border_t_1().border_color(rgb(border_color()))
        })
        .child(div().w_full().min_w(px(0.0)).child(title.into()))
        .child(
            div()
                .w_full()
                .min_w(px(0.0))
                .overflow_hidden()
                .child(action),
        )
}

fn setting_group_stepper_row_u64(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    value: u64,
    value_element: AnyElement,
    divided: bool,
    handler: impl Fn(&StepChange<u64>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let handler: Rc<dyn Fn(&StepChange<u64>, &mut Window, &mut App)> = Rc::new(handler);
    let down = Rc::clone(&handler);
    let delta = u64_step(value);

    setting_group_action_row(
        id.clone(),
        title,
        h_flex()
            .items_center()
            .justify_end()
            .gap_2()
            .min_w(px(0.0))
            .flex_shrink_0()
            .child(
                control_button(Button::new((gpui::ElementId::from(id.clone()), "down")))
                    .label("-")
                    .on_click(move |_, window, cx| {
                        down(
                            &StepChange {
                                delta,
                                increase: false,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .child(value_element)
            .child(
                control_button(Button::new((gpui::ElementId::from(id), "up")))
                    .label("+")
                    .on_click(move |_, window, cx| {
                        handler(
                            &StepChange {
                                delta,
                                increase: true,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .into_any_element(),
        divided,
    )
    .into_any_element()
}

fn rule_action_row(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    action: AnyElement,
) -> gpui::Stateful<gpui::Div> {
    rule_action_row_with_title_color(id, title, action, primary_text_color())
}

fn rule_action_row_with_title_color(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    action: AnyElement,
    title_color: u32,
) -> gpui::Stateful<gpui::Div> {
    h_flex()
        .id(id.into())
        .w_full()
        .min_w(px(0.0))
        .min_h(px(58.0))
        .items_center()
        .justify_between()
        .gap_2()
        .border_t_1()
        .border_color(rgb(border_color()))
        .py_3()
        .px_4()
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_color(rgb(title_color))
                .child(title.into()),
        )
        .child(
            h_flex()
                .flex_1()
                .min_w(px(0.0))
                .items_center()
                .justify_end()
                .gap_2()
                .child(action),
        )
}

fn rule_stepper_row_u64(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    value: u64,
    value_element: AnyElement,
    handler: impl Fn(&StepChange<u64>, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let id: SharedString = id.into();
    let handler: Rc<dyn Fn(&StepChange<u64>, &mut Window, &mut App)> = Rc::new(handler);
    let down = Rc::clone(&handler);
    let delta = u64_step(value);

    rule_action_row(
        id.clone(),
        title,
        h_flex()
            .items_center()
            .justify_end()
            .gap_2()
            .min_w(px(0.0))
            .flex_shrink_0()
            .child(
                control_button(Button::new((gpui::ElementId::from(id.clone()), "down")))
                    .label("-")
                    .on_click(move |_, window, cx| {
                        down(
                            &StepChange {
                                delta,
                                increase: false,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .child(value_element)
            .child(
                control_button(Button::new((gpui::ElementId::from(id), "up")))
                    .label("+")
                    .on_click(move |_, window, cx| {
                        handler(
                            &StepChange {
                                delta,
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

fn rule_checkbox_row(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    checked: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let title: SharedString = title.into();
    let handler: Rc<dyn Fn(&bool, &mut Window, &mut App)> = Rc::new(handler);
    let checkbox_handler = Rc::clone(&handler);
    let label_handler = Rc::clone(&handler);

    h_flex()
        .id(id.clone())
        .w_full()
        .min_w(px(0.0))
        .min_h(px(58.0))
        .items_center()
        .justify_between()
        .gap_2()
        .border_t_1()
        .border_color(rgb(border_color()))
        .py_3()
        .px_4()
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .child(
            div().flex_1().min_w(px(0.0)).child(
                div()
                    .id(SharedString::from(format!("{id}-label")))
                    .min_w(px(0.0))
                    .truncate()
                    .cursor_pointer()
                    .hover(|style| style.opacity(0.86))
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        let next = !checked;
                        label_handler(&next, window, cx);
                    })
                    .child(title),
            ),
        )
        .child(
            h_flex()
                .items_center()
                .justify_end()
                .gap_2()
                .min_w(px(0.0))
                .flex_shrink_0()
                .child(rule_enable_checkbox(
                    format!("{id}-check"),
                    checked,
                    move |next, window, cx| {
                        checkbox_handler(next, window, cx);
                    },
                )),
        )
        .into_any_element()
}

fn rule_toggle_switch(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    enabled: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();

    rule_action_row(
        id.clone(),
        label,
        switch_toggle_action(format!("{id}-switch"), enabled, handler),
    )
    .into_any_element()
}

fn rule_notice_row(id: impl Into<SharedString>, notice: AnyElement) -> gpui::Stateful<gpui::Div> {
    h_flex()
        .id(id.into())
        .w_full()
        .min_w(px(0.0))
        .min_h(px(58.0))
        .items_center()
        .gap_2()
        .border_t_1()
        .border_color(rgb(border_color()))
        .py_3()
        .px_4()
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .child(notice)
}

fn setting_action_card(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    action: AnyElement,
) -> gpui::Stateful<gpui::Div> {
    setting_action_card_element(
        id,
        div()
            .flex_1()
            .min_w(px(0.0))
            .truncate()
            .child(title.into())
            .into_any_element(),
        action,
    )
}

fn setting_action_card_with_help(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    help: impl Into<SharedString>,
    action: AnyElement,
) -> gpui::Stateful<gpui::Div> {
    let id: SharedString = id.into();
    setting_action_card_element(
        id.clone(),
        h_flex()
            .flex_1()
            .min_w(px(0.0))
            .gap_1()
            .items_center()
            .child(div().truncate().child(title.into()))
            .child(title_info_button(format!("{id}-info"), help))
            .into_any_element(),
        action,
    )
}

fn setting_action_card_element(
    id: impl Into<SharedString>,
    title: AnyElement,
    action: AnyElement,
) -> gpui::Stateful<gpui::Div> {
    h_flex()
        .id(id.into())
        .w_full()
        .min_w(px(0.0))
        .min_h(px(58.0))
        .items_center()
        .justify_between()
        .gap_2()
        .py_3()
        .px_4()
        .rounded_sm()
        .border_1()
        .border_color(rgb(border_color()))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .hover(|style| style.bg(rgb(settings_card_hover_color())))
        .child(title)
        .child(
            h_flex()
                .items_center()
                .justify_end()
                .flex_shrink_0()
                .child(action),
        )
}

fn setting_stepper_card_u64(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    value: u64,
    value_element: AnyElement,
    handler: impl Fn(&StepChange<u64>, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let id: SharedString = id.into();
    let handler: Rc<dyn Fn(&StepChange<u64>, &mut Window, &mut App)> = Rc::new(handler);
    let down = Rc::clone(&handler);
    let delta = u64_step(value);

    setting_action_card(
        id.clone(),
        title,
        h_flex()
            .items_center()
            .justify_end()
            .gap_2()
            .flex_shrink_0()
            .child(
                control_button(Button::new((gpui::ElementId::from(id.clone()), "down")))
                    .label("-")
                    .on_click(move |_, window, cx| {
                        down(
                            &StepChange {
                                delta,
                                increase: false,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .child(value_element)
            .child(
                control_button(Button::new((gpui::ElementId::from(id), "up")))
                    .label("+")
                    .on_click(move |_, window, cx| {
                        handler(
                            &StepChange {
                                delta,
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

fn setting_input_card(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    input: Entity<InputState>,
    focused: bool,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Stateful<gpui::Div> {
    rule_action_row(
        id,
        title,
        div()
            .w(px(132.0))
            .min_w(px(104.0))
            .child(app_input(&input, focused, cx))
            .into_any_element(),
    )
}

fn setting_notice_card(
    id: impl Into<SharedString>,
    notice: AnyElement,
) -> gpui::Stateful<gpui::Div> {
    rule_notice_row(id, notice)
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

fn dashboard_card_slot(card: AnyElement) -> gpui::Div {
    div()
        .w(relative(0.49))
        .min_w(px(320.0))
        .flex_1()
        .child(card)
}

fn dashboard_summary_card(title: impl Into<SharedString>, body: AnyElement) -> gpui::Div {
    v_flex()
        .w_full()
        .min_w(px(0.0))
        .h(px(DASHBOARD_SUMMARY_CARD_HEIGHT))
        .p_3()
        .gap_2()
        .rounded_sm()
        .border_1()
        .border_color(rgb(border_color()))
        .child(section_title_label(title))
        .child(
            div()
                .w_full()
                .min_w(px(0.0))
                .flex_1()
                .min_h(px(0.0))
                .child(body),
        )
}

fn titled_status_list(title: &str, items: Vec<(String, String)>) -> gpui::Div {
    let mut list = v_flex()
        .w_full()
        .min_w(px(0.0))
        .flex_1()
        .min_h(px(0.0))
        .gap_1()
        .overflow_y_scrollbar();

    for (label, detail) in items {
        let mut content = h_flex()
            .flex_1()
            .min_w(px(0.0))
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(px(TEXT_BODY_SIZE))
                    .line_height(px(TEXT_BODY_LINE_HEIGHT))
                    .child(label),
            );

        if !detail.is_empty() {
            content = content.child(text_muted(detail).truncate().flex_shrink_0());
        }

        list = list.child(
            h_flex()
                .w_full()
                .min_w(px(0.0))
                .items_center()
                .gap_2()
                .py_1()
                .child(
                    div()
                        .size(px(6.0))
                        .rounded_full()
                        .flex_shrink_0()
                        .bg(rgb(accent_color())),
                )
                .child(content),
        );
    }

    dashboard_summary_card(title.to_owned(), list.into_any_element())
}

fn action_log_header_row() -> gpui::Div {
    h_flex()
        .w_full()
        .min_w(px(0.0))
        .gap_3()
        .px_4()
        .pb_1()
        .text_size(px(TEXT_LABEL_SIZE))
        .line_height(px(TEXT_LABEL_LINE_HEIGHT))
        .text_color(rgb(muted_text_color()))
        .child(
            div()
                .w(px(56.0))
                .child(t!("action_log.sequence").to_string()),
        )
        .child(div().w(px(96.0)).child(t!("action_log.time").to_string()))
        .child(
            div()
                .w(px(156.0))
                .child(t!("action_log.feature").to_string()),
        )
        .child(div().w(px(88.0)).child(t!("action_log.result").to_string()))
        .child(
            div()
                .w(px(176.0))
                .child(t!("action_log.process").to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(120.0))
                .child(t!("action_log.reason").to_string()),
        )
}

fn action_log_entry_row(entry: &ActionLogEntry, cx: &mut Context<PowerLeafApp>) -> gpui::Div {
    compact_rule_row(cx)
        .gap_3()
        .child(
            div()
                .w(px(56.0))
                .flex_shrink_0()
                .text_color(rgb(dim_text_color()))
                .child(format!("#{}", entry.sequence)),
        )
        .child(
            div()
                .w(px(96.0))
                .flex_shrink_0()
                .text_color(rgb(muted_text_color()))
                .child(action_log_time_label(entry.timestamp_epoch_ms)),
        )
        .child(
            div()
                .w(px(156.0))
                .flex_shrink_0()
                .truncate()
                .child(action_log_feature_label(entry.feature)),
        )
        .child(
            div()
                .w(px(88.0))
                .flex_shrink_0()
                .child(action_log_result_tag(entry.result).into_any_element()),
        )
        .child(
            div()
                .w(px(176.0))
                .min_w(px(0.0))
                .flex_shrink_0()
                .truncate()
                .child(action_log_process_label(entry)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(120.0))
                .text_color(rgb(muted_text_color()))
                .child(entry.reason.clone()),
        )
}

fn action_log_result_tag(result: ActionLogResult) -> Tag {
    let label = action_log_result_label(result);
    match result {
        ActionLogResult::Applied | ActionLogResult::Restored => {
            Tag::success().outline().child(label)
        }
        ActionLogResult::Skipped => Tag::warning().outline().child(label),
        ActionLogResult::Failed => Tag::danger().outline().child(label),
    }
}

fn action_log_feature_label(feature: ActionLogFeature) -> &'static str {
    match feature {
        ActionLogFeature::AppSuspension => "App Suspension",
        ActionLogFeature::BackgroundCpuRestriction => "Background CPU Restriction",
        ActionLogFeature::CoreSteering => "Core Steering",
        ActionLogFeature::EcoQos => "Efficiency Mode",
        ActionLogFeature::CpuLimiter => "Core Limiter",
        ActionLogFeature::PerformanceMode => "By Running App",
        ActionLogFeature::Watchdog => "Watchdog Rules",
        ActionLogFeature::ForegroundResponsiveness => "Foreground Responsiveness",
        ActionLogFeature::IoPriority => "I/O Priority",
        ActionLogFeature::MemoryPriority => "Memory Priority",
        ActionLogFeature::SmartTrim => "SmartTrim",
    }
}

fn action_log_result_label(result: ActionLogResult) -> SharedString {
    action_log_result_text(result).into()
}

fn action_log_result_text(result: ActionLogResult) -> &'static str {
    match result {
        ActionLogResult::Applied => "Applied",
        ActionLogResult::Restored => "Restored",
        ActionLogResult::Skipped => "Skipped",
        ActionLogResult::Failed => "Failed",
    }
}

fn action_log_filter_label(filter: ActionLogResultFilter) -> String {
    match filter {
        ActionLogResultFilter::All => t!("action_log.filter_all").to_string(),
        ActionLogResultFilter::Applied => {
            action_log_result_label(ActionLogResult::Applied).to_string()
        }
        ActionLogResultFilter::Restored => {
            action_log_result_label(ActionLogResult::Restored).to_string()
        }
        ActionLogResultFilter::Skipped => {
            action_log_result_label(ActionLogResult::Skipped).to_string()
        }
        ActionLogResultFilter::Failed => {
            action_log_result_label(ActionLogResult::Failed).to_string()
        }
    }
}

fn theme_mode_label(mode: AppThemeMode) -> String {
    match mode {
        AppThemeMode::System => t!("theme.system").to_string(),
        AppThemeMode::Light => t!("theme.light").to_string(),
        AppThemeMode::Dark => t!("theme.dark").to_string(),
    }
}

fn accent_source_label(source: AccentColorSource) -> String {
    match source {
        AccentColorSource::Windows => t!("theme.system").to_string(),
        AccentColorSource::Custom => t!("accent.custom").to_string(),
    }
}

fn action_log_action_label(action: ActionLogAction) -> &'static str {
    match action {
        ActionLogAction::Apply => "Apply",
        ActionLogAction::Restore => "Restore",
        ActionLogAction::Skip => "Skip",
        ActionLogAction::Fail => "Fail",
    }
}

fn action_log_entries_to_csv(entries: &[ActionLogEntry]) -> String {
    let header = "sequence,timestamp,feature,process_id,process_name,action,result,reason\r\n";
    let mut csv = String::with_capacity(header.len() + entries.len() * 128);
    csv.push_str(header);
    for entry in entries {
        let sequence = entry.sequence.to_string();
        let timestamp = action_log_export_time_label(entry.timestamp_epoch_ms);
        let process_id = entry
            .process_id
            .map(|id| id.to_string())
            .unwrap_or_default();

        push_csv_field(&mut csv, &sequence);
        csv.push(',');
        push_csv_field(&mut csv, &timestamp);
        csv.push(',');
        push_csv_field(&mut csv, action_log_feature_label(entry.feature));
        csv.push(',');
        push_csv_field(&mut csv, &process_id);
        csv.push(',');
        push_csv_field(&mut csv, &entry.process_name);
        csv.push(',');
        push_csv_field(&mut csv, action_log_action_label(entry.action));
        csv.push(',');
        push_csv_field(&mut csv, action_log_result_text(entry.result));
        csv.push(',');
        push_csv_field(&mut csv, &entry.reason);
        csv.push_str("\r\n");
    }
    csv
}

fn action_log_export_time_label(timestamp_epoch_ms: u128) -> String {
    let timestamp = timestamp_epoch_ms.min(i64::MAX as u128) as i64;
    Local
        .timestamp_millis_opt(timestamp)
        .single()
        .map(|time| time.format("%Y-%m-%d %H:%M:%S%.3f %:z").to_string())
        .unwrap_or_else(|| timestamp_epoch_ms.to_string())
}

#[cfg(test)]
fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn push_csv_field(csv: &mut String, value: &str) {
    if value.contains([',', '"', '\n', '\r']) {
        csv.push('"');
        for character in value.chars() {
            if character == '"' {
                csv.push('"');
            }
            csv.push(character);
        }
        csv.push('"');
    } else {
        csv.push_str(value);
    }
}

fn action_log_process_label(entry: &ActionLogEntry) -> String {
    let name = if entry.process_name.trim().is_empty() {
        t!("common.none").to_string()
    } else {
        entry.process_name.clone()
    };
    match entry.process_id {
        Some(process_id) => format!("{name} ({})", process_id),
        None => name,
    }
}

fn action_log_time_label(timestamp_epoch_ms: u128) -> String {
    let timestamp = timestamp_epoch_ms.min(i64::MAX as u128) as i64;
    Local
        .timestamp_millis_opt(timestamp)
        .single()
        .map(|time| time.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "--:--:--".to_owned())
}

fn rule_count_label(count: usize) -> String {
    format!("{count} {}", t!("common.rules"))
}

fn yes_no_label(value: bool) -> String {
    if value { "Yes" } else { "No" }.to_owned()
}

#[allow(dead_code)]
fn format_percent_tenths(value: u16) -> String {
    format!("{}.{:01}%", value / 10, value % 10)
}

#[allow(dead_code)]
fn auto_balance_status_row(detail: &responsiveness::AutoBalanceProcessStatus) -> gpui::Div {
    let cpu_usage = detail
        .cpu_usage_tenths
        .map(format_percent_tenths)
        .unwrap_or_else(|| t!("common.unknown").to_string());
    let elapsed = detail
        .elapsed_seconds
        .map(|seconds| format!("{seconds}s"))
        .unwrap_or_else(|| "-".to_owned());
    let reaction = detail
        .reaction_millis
        .map(format_millis)
        .unwrap_or_else(|| "-".to_owned());

    h_flex()
        .w_full()
        .min_w(px(0.0))
        .min_h(px(58.0))
        .items_center()
        .justify_between()
        .gap_3()
        .py_3()
        .px_4()
        .rounded_sm()
        .border_1()
        .border_color(rgb(border_color()))
        .bg(rgb(settings_card_color()))
        .child(
            v_flex()
                .min_w(px(0.0))
                .gap_1()
                .child(
                    div()
                        .truncate()
                        .text_color(rgb(primary_text_color()))
                        .text_size(px(TEXT_BODY_SIZE))
                        .child(format!("{} ({})", detail.process_name, detail.process_id)),
                )
                .child(
                    text_muted(format!(
                        "{}: {cpu_usage} | {}: {elapsed} | {}: {reaction} | {}: {}",
                        t!("responsiveness.auto_balance_process_cpu"),
                        t!("responsiveness.auto_balance_elapsed"),
                        t!("responsiveness.auto_balance_reaction"),
                        t!("responsiveness.auto_balance_repeats"),
                        detail.restraint_count
                    ))
                    .truncate(),
                ),
        )
        .child(auto_balance_state_tag(detail.state))
}

fn format_millis(value: u64) -> String {
    if value >= 1_000 {
        format!("{:.1}s", value as f64 / 1_000.0)
    } else {
        format!("{value}ms")
    }
}

#[allow(dead_code)]
fn auto_balance_state_tag(state: AutoBalanceProcessState) -> Tag {
    match state {
        AutoBalanceProcessState::Watching => Tag::warning()
            .outline()
            .child(t!("responsiveness.auto_balance_state_watching").to_string()),
        AutoBalanceProcessState::Lowered => Tag::success()
            .outline()
            .child(t!("responsiveness.auto_balance_state_lowered").to_string()),
        AutoBalanceProcessState::AffinityRestrained => Tag::success()
            .outline()
            .child(t!("responsiveness.auto_balance_state_affinity").to_string()),
        AutoBalanceProcessState::CoolingDown => Tag::secondary()
            .outline()
            .child(t!("responsiveness.auto_balance_state_cooling").to_string()),
    }
}

fn app_input(
    input: &Entity<InputState>,
    focused: bool,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Div {
    div()
        .w_full()
        .h(px(32.0))
        .flex()
        .flex_col()
        .relative()
        .overflow_hidden()
        .rounded_sm()
        .border_1()
        .border_color(rgb(app_input_border_color(focused)))
        .bg(rgb(app_input_color(focused)))
        .hover(|style| style.border_color(rgb(app_input_hover_border_color())))
        .child(
            Input::new(input)
                .appearance(false)
                .bordered(false)
                .focus_bordered(false)
                .w_full()
                .h_full()
                .text_color(cx.theme().foreground)
                .into_any_element(),
        )
        .child(
            div()
                .absolute()
                .left(px(0.0))
                .right(px(0.0))
                .bottom(px(0.0))
                .h(px(if focused { 1.5 } else { 1.0 }))
                .bg(if focused {
                    cx.theme().accent
                } else {
                    Hsla::from(rgb(app_input_bottom_line_color()))
                }),
        )
}

fn app_input_color(focused: bool) -> u32 {
    if ui_is_dark() {
        if focused {
            0x1f1f1f
        } else {
            0x2f2f2f
        }
    } else if focused {
        0xffffff
    } else {
        0xffffff
    }
}

fn app_input_border_color(focused: bool) -> u32 {
    if ui_is_dark() {
        if focused {
            0x5c5c5c
        } else {
            COLOR_BORDER
        }
    } else if focused {
        0x757575
    } else {
        0xdedede
    }
}

fn app_input_hover_border_color() -> u32 {
    if ui_is_dark() {
        0x6a6a6a
    } else {
        0x9a9a9a
    }
}

fn app_input_bottom_line_color() -> u32 {
    if ui_is_dark() {
        0x9a9a9a
    } else {
        0x6d6d6d
    }
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

fn status_pill(label: impl Into<SharedString>, bg: u32, fg: u32) -> AnyElement {
    let label: SharedString = label.into();

    div()
        .flex_shrink_0()
        .px_2()
        .py(px(2.0))
        .rounded_sm()
        .bg(rgb(bg))
        .text_color(rgb(fg))
        .text_size(px(TEXT_LABEL_SIZE))
        .line_height(px(TEXT_LABEL_LINE_HEIGHT))
        .child(label)
        .into_any_element()
}

fn rule_enable_checkbox(
    id: impl Into<SharedString>,
    checked: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let accent = accent_color();
    let border_color = if checked { accent } else { border_color() };
    let check_color = accent_glyph_color(accent);

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
                .when(checked, |this| this.bg(rgb(accent)))
                .when(checked, |this| {
                    this.child(
                        div()
                            .text_size(px(TEXT_LABEL_SIZE))
                            .line_height(px(TEXT_LABEL_LINE_HEIGHT))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(check_color))
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
    let accent = accent_color();
    let border_color = if checked { accent } else { border_color() };
    let text_color = if checked {
        primary_text_color()
    } else {
        muted_text_color()
    };
    let check_color = accent_glyph_color(accent);
    let handler = Rc::new(handler);
    let box_handler = handler.clone();
    let label_handler = handler;

    h_flex()
        .w_full()
        .min_w(px(0.0))
        .child(
            h_flex()
                .id(id.clone())
                .flex_none()
                .items_center()
                .gap_2()
                .py_1()
                .px_1()
                .rounded_sm()
                .text_color(rgb(text_color))
                .text_size(px(TEXT_BODY_SIZE))
                .line_height(px(TEXT_BODY_LINE_HEIGHT))
                .child(
                    div()
                        .id(SharedString::from(format!("{id}-box")))
                        .size(px(16.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .flex_shrink_0()
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(border_color))
                        .when(checked, |this| this.bg(rgb(accent)))
                        .when(checked, |this| {
                            this.child(
                                div()
                                    .text_size(px(TEXT_LABEL_SIZE))
                                    .line_height(px(TEXT_LABEL_LINE_HEIGHT))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(check_color))
                                    .child("\u{2713}"),
                            )
                        })
                        .hover(|style| style.opacity(0.86))
                        .cursor_pointer()
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            let next = !checked;
                            box_handler(&next, window, cx);
                        }),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("{id}-label")))
                        .hover(|style| style.opacity(0.86))
                        .cursor_pointer()
                        .child(label)
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            let next = !checked;
                            label_handler(&next, window, cx);
                        }),
                ),
        )
        .into_any_element()
}

#[allow(dead_code)]
fn checkbox_with_help(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    help: impl Into<SharedString>,
    checked: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let label: SharedString = label.into();
    let help: SharedString = help.into();
    let accent = accent_color();
    let border_color = if checked { accent } else { border_color() };
    let text_color = if checked {
        primary_text_color()
    } else {
        muted_text_color()
    };
    let check_color = accent_glyph_color(accent);
    let handler = Rc::new(handler);
    let box_handler = handler.clone();
    let label_handler = handler;

    h_flex()
        .w_full()
        .min_w(px(0.0))
        .child(
            h_flex()
                .id(id.clone())
                .flex_none()
                .items_center()
                .gap_2()
                .py_1()
                .px_1()
                .rounded_sm()
                .text_color(rgb(text_color))
                .text_size(px(TEXT_BODY_SIZE))
                .line_height(px(TEXT_BODY_LINE_HEIGHT))
                .child(
                    div()
                        .id(SharedString::from(format!("{id}-box")))
                        .size(px(16.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .flex_shrink_0()
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(border_color))
                        .when(checked, |this| this.bg(rgb(accent)))
                        .when(checked, |this| {
                            this.child(
                                div()
                                    .text_size(px(TEXT_LABEL_SIZE))
                                    .line_height(px(TEXT_LABEL_LINE_HEIGHT))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(check_color))
                                    .child("\u{2713}"),
                            )
                        })
                        .hover(|style| style.opacity(0.86))
                        .cursor_pointer()
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            let next = !checked;
                            box_handler(&next, window, cx);
                        }),
                )
                .child(
                    h_flex()
                        .min_w(px(0.0))
                        .gap_1()
                        .child(
                            div()
                                .id(SharedString::from(format!("{id}-label")))
                                .truncate()
                                .hover(|style| style.opacity(0.86))
                                .cursor_pointer()
                                .child(label)
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    let next = !checked;
                                    label_handler(&next, window, cx);
                                }),
                        )
                        .child(title_info_button(format!("{id}-info"), help)),
                ),
        )
        .into_any_element()
}

#[derive(Clone, Copy)]
enum NavStatus {
    Enabled,
    Disabled,
    NeedsRules,
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
        .text_size(px(TEXT_CAPTION_SIZE))
        .line_height(px(TEXT_CAPTION_LINE_HEIGHT))
        .text_color(cx.theme().muted_foreground)
        .hover(move |style| style.bg(hover_bg))
        .active(move |style| style.bg(active_bg))
        .child(icon)
        .into_any_element()
}

fn section_landing_card(
    page: Page,
    status: Option<NavStatus>,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Stateful<gpui::Div> {
    let mut trailing = h_flex()
        .items_center()
        .justify_end()
        .gap_2()
        .flex_shrink_0();
    if let Some(status) = status {
        trailing = trailing.child(nav_status_indicator(status));
    }
    trailing = trailing.child(
        Icon::new(NavIcon::ChevronRight)
            .with_size(px(16.0))
            .text_color(cx.theme().muted_foreground),
    );

    h_flex()
        .id(SharedString::from(format!("section-card-{page:?}")))
        .w_full()
        .min_w(px(0.0))
        .min_h(px(58.0))
        .items_center()
        .justify_between()
        .gap_3()
        .py_3()
        .px_4()
        .rounded_sm()
        .border_1()
        .border_color(rgb(border_color()))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
        .hover(|style| style.bg(rgb(settings_card_hover_color())))
        .cursor_pointer()
        .child(nav_icon(page, true, cx))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(TEXT_BODY_SIZE))
                .line_height(px(TEXT_BODY_LINE_HEIGHT))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(primary_text_color()))
                .child(page.label()),
        )
        .child(trailing)
}

fn nav_row(
    page: Page,
    selected: bool,
    status: Option<NavStatus>,
    cx: &mut Context<PowerLeafApp>,
) -> gpui::Stateful<gpui::Div> {
    let row_bg = if selected {
        rgb(sidebar_selected_color()).into()
    } else {
        cx.theme().transparent
    };
    let indicator = if selected {
        rgb(accent_color()).into()
    } else {
        cx.theme().transparent
    };
    let text_color = if selected {
        cx.theme().sidebar_foreground
    } else {
        cx.theme().sidebar_foreground
    };
    let hover_bg: gpui::Hsla = if selected {
        rgb(sidebar_selected_color()).into()
    } else {
        rgb(sidebar_hover_color()).into()
    };

    let row = h_flex()
        .id(SharedString::from(format!("nav-row-{:?}", page)))
        .h(px(40.0))
        .w_full()
        .items_center()
        .gap_3()
        .pl(px(0.0))
        .pr(px(12.0))
        .rounded(px(FLUENT_RADIUS_CONTROL))
        .bg(row_bg)
        .text_color(text_color)
        .hover(move |style| style.bg(hover_bg))
        .cursor_pointer()
        .child(div().w(px(3.0)).h(px(20.0)).rounded_sm().bg(indicator))
        .child(nav_icon(page, selected, cx))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(TEXT_CONTROL_SIZE))
                .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
                .truncate()
                .child(page.label()),
        );

    if let Some(status) = status {
        row.child(nav_status_indicator(status))
    } else {
        row
    }
}

fn nav_status_indicator(status: NavStatus) -> AnyElement {
    let (label, bg, fg, border) = match status {
        NavStatus::Enabled => (
            "ON".to_owned(),
            success_bg_color(),
            success_text_color(),
            success_text_color(),
        ),
        NavStatus::Failed => (
            "ERR".to_owned(),
            if ui_is_dark() { 0x4a1f1f } else { 0xfde7e9 },
            if ui_is_dark() { 0xff8a8a } else { 0xc42b1c },
            if ui_is_dark() { 0x8f2f2f } else { 0xf1aeb5 },
        ),
        NavStatus::Disabled => (
            "OFF".to_owned(),
            if ui_is_dark() { 0x343434 } else { 0xf3f3f3 },
            dim_text_color(),
            border_color(),
        ),
        NavStatus::NeedsRules => (
            "?".to_owned(),
            warning_bg_color(),
            warning_text_color(),
            warning_text_color(),
        ),
        NavStatus::Unsupported => (
            "N/A".to_owned(),
            warning_bg_color(),
            warning_text_color(),
            warning_text_color(),
        ),
    };

    h_flex()
        .h(px(20.0))
        .min_w(px(38.0))
        .items_center()
        .justify_center()
        .rounded(px(10.0))
        .border_1()
        .border_color(rgb(border))
        .bg(rgb(bg))
        .px_2()
        .text_size(px(TEXT_CAPTION_SIZE))
        .line_height(px(TEXT_CAPTION_LINE_HEIGHT))
        .text_color(rgb(fg))
        .child(label)
        .into_any_element()
}

fn enabled_nav_status(enabled: bool) -> NavStatus {
    if enabled {
        NavStatus::Enabled
    } else {
        NavStatus::Disabled
    }
}

fn rule_based_nav_status(enabled: bool, rule_count: usize) -> NavStatus {
    if enabled && rule_count == 0 {
        NavStatus::NeedsRules
    } else {
        enabled_nav_status(enabled)
    }
}

fn process_nav_status(enabled: bool, failed_count: usize, has_error: bool) -> NavStatus {
    if failed_count > 0 || has_error {
        NavStatus::Failed
    } else {
        enabled_nav_status(enabled)
    }
}

fn process_rule_nav_status(
    enabled: bool,
    rule_count: usize,
    failed_count: usize,
    has_error: bool,
) -> NavStatus {
    if failed_count > 0 || has_error {
        NavStatus::Failed
    } else {
        rule_based_nav_status(enabled, rule_count)
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
        rgb(accent_color()).into()
    } else {
        cx.theme().muted_foreground
    };

    div()
        .w(px(22.0))
        .h(px(22.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .child(
            Icon::new(nav_icon_name(page))
                .with_size(px(18.0))
                .text_color(color),
        )
        .into_any_element()
}

fn nav_icon_name(page: Page) -> NavIcon {
    match page {
        Page::Dashboard => NavIcon::Dashboard,
        Page::PowerPlanAutomation => NavIcon::Zap,
        Page::ProcessorControls => NavIcon::Chip,
        Page::ProcessPolicies => NavIcon::Frame,
        Page::MemoryControl => NavIcon::Chip,
        Page::AppHome => NavIcon::Settings,
        Page::AdvancedHome => NavIcon::Chip,
        Page::Activity => NavIcon::Activity,
        Page::CpuUsage => NavIcon::Chart,
        Page::CoreParking => NavIcon::Chip,
        Page::CpuLimiter => NavIcon::Chart,
        Page::BackgroundCpuRestriction => NavIcon::Chip,
        Page::EfficiencyMode => NavIcon::Zap,
        Page::AppSuspension => NavIcon::PauseCircle,
        Page::Watchdog => NavIcon::Frame,
        Page::PerformanceMode => NavIcon::Zap,
        Page::ForegroundResponsiveness => NavIcon::Zap,
        Page::IoPriority => NavIcon::Chip,
        Page::MemoryPriority => NavIcon::Chip,
        Page::SmartTrim => NavIcon::Chip,
        Page::CpuAffinity => NavIcon::Chip,
        Page::ForegroundRules => NavIcon::Frame,
        Page::Schedule => NavIcon::Calendar,
        Page::ActionLog => NavIcon::Info,
        Page::Settings => NavIcon::Settings,
        Page::SettingsAppearance => NavIcon::Palette,
        Page::Win32PrioritySeparation => NavIcon::Chip,
        Page::About => NavIcon::Info,
    }
}

#[derive(Clone, Copy)]
enum NavIcon {
    Activity,
    Calendar,
    Chart,
    ChevronDown,
    ChevronRight,
    Chip,
    Dashboard,
    Frame,
    Info,
    Palette,
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
            Self::ChevronDown => "icons/chevron-down.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::Chip => "icons/chip.svg",
            Self::Dashboard => "icons/dashboard.svg",
            Self::Frame => "icons/frame.svg",
            Self::Info => "icons/info.svg",
            Self::Palette => "icons/palette.svg",
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

fn control_button(button: Button) -> Button {
    button
        .small()
        .h(px(32.0))
        .text_size(px(TEXT_CONTROL_SIZE))
        .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
}

fn primary_control_button(button: Button, cx: &mut Context<PowerLeafApp>) -> Button {
    control_button(button.primary()).text_color(cx.theme().primary_foreground)
}

fn danger_control_button(button: Button) -> Button {
    control_button(button.danger()).text_color(rgb(0xffffff))
}

fn accent_swatch(color: u32, selected: bool) -> gpui::Stateful<gpui::Div> {
    let border = if selected {
        primary_text_color()
    } else {
        color
    };

    div()
        .id(SharedString::from(format!("accent-swatch-{color:06x}")))
        .size(px(42.0))
        .flex_shrink_0()
        .rounded_sm()
        .border_1()
        .border_color(rgb(border))
        .bg(rgb(color))
        .hover(|style| style.border_color(rgb(primary_text_color())))
        .cursor_pointer()
        .when(selected, |style| style.border_2())
}

fn accent_color_group(title: impl Into<SharedString>, swatches: AnyElement) -> gpui::Div {
    v_flex()
        .w_full()
        .min_w(px(0.0))
        .gap_2()
        .child(section_title_label(title))
        .child(swatches)
}

fn feature_toggle_switch(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    enabled: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    feature_toggle_switch_inner(id, label, None, enabled, handler)
}

fn feature_toggle_switch_with_help(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    help: impl Into<SharedString>,
    enabled: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    feature_toggle_switch_inner(id, label, Some(help.into()), enabled, handler)
}

fn feature_toggle_switch_inner(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    help: Option<SharedString>,
    enabled: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let label = label.into();
    let label_id = id.clone();
    let mut label_row = h_flex()
        .flex_1()
        .min_w(px(0.0))
        .items_center()
        .gap_1()
        .child(div().min_w(px(0.0)).truncate().child(label));
    if let Some(help) = help {
        label_row = label_row.child(title_info_button(format!("{label_id}-info"), help));
    }

    h_flex()
        .id(id.clone())
        .w_full()
        .min_w(px(0.0))
        .min_h(px(58.0))
        .items_center()
        .justify_between()
        .gap_2()
        .py_3()
        .px_4()
        .rounded_sm()
        .border_1()
        .border_color(rgb(border_color()))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .hover(|style| style.bg(rgb(settings_card_hover_color())))
        .child(label_row)
        .child(switch_toggle_action(
            format!("{id}-switch"),
            enabled,
            handler,
        ))
        .into_any_element()
}

fn switch_toggle_action(
    id: impl Into<SharedString>,
    enabled: bool,
    handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    h_flex()
        .id(id.into())
        .items_center()
        .child(switch_indicator(enabled))
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            let next = !enabled;
            handler(&next, window, cx);
        })
        .into_any_element()
}

fn switch_indicator(enabled: bool) -> gpui::Div {
    let accent = switch_accent_color();
    let switch_bg = if enabled {
        accent
    } else {
        settings_card_color()
    };
    let switch_border = if enabled { accent } else { border_color() };
    let knob_bg = if enabled {
        accent_glyph_color(accent)
    } else if ui_is_dark() {
        0xd0d0d0
    } else {
        0x5f5f5f
    };
    let state_label = if enabled { "On" } else { "Off" };

    h_flex()
        .items_center()
        .justify_end()
        .gap_2()
        .flex_shrink_0()
        .child(
            div()
                .text_size(px(TEXT_BODY_SIZE))
                .line_height(px(TEXT_BODY_LINE_HEIGHT))
                .text_color(rgb(primary_text_color()))
                .child(state_label),
        )
        .child(
            h_flex()
                .w(px(40.0))
                .h(px(20.0))
                .items_center()
                .flex_shrink_0()
                .rounded_full()
                .border_1()
                .border_color(rgb(switch_border))
                .bg(rgb(switch_bg))
                .px(px(4.0))
                .when(enabled, |track| track.justify_end())
                .when(!enabled, |track| track.justify_start())
                .child(div().size(px(12.0)).rounded_full().bg(rgb(knob_bg))),
        )
}

fn value_pill(value: impl Into<SharedString>) -> gpui::Div {
    div()
        .min_w(px(56.0))
        .h(px(32.0))
        .px_3()
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .border_1()
        .border_color(rgb(app_input_border_color(false)))
        .bg(rgb(app_input_color(false)))
        .text_size(px(TEXT_CONTROL_SIZE))
        .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
        .text_color(rgb(primary_text_color()))
        .child(value.into())
        .child(
            div()
                .absolute()
                .left(px(0.0))
                .right(px(0.0))
                .bottom(px(0.0))
                .h(px(1.0))
                .bg(rgb(app_input_bottom_line_color())),
        )
}

fn numeric_value_width(field: NumericField) -> f32 {
    match field {
        NumericField::ProcessorAcCoreParkingMin
        | NumericField::ProcessorAcPerformanceMin
        | NumericField::ProcessorAcPerformanceMax
        | NumericField::ProcessorDcCoreParkingMin
        | NumericField::ProcessorDcPerformanceMin
        | NumericField::ProcessorDcPerformanceMax
        | NumericField::EcoQosRestrictionPercent
        | NumericField::BackgroundCpuRestrictionPercent
        | NumericField::SmartTrimMemoryLoadThreshold
        | NumericField::SmartTrimCpuIdleThreshold
        | NumericField::CpuLimiterThreshold(_)
        | NumericField::CpuLimiterMaxProcessors(_) => 76.0,
        NumericField::SmartTrimCheckIntervalMinutes
        | NumericField::SmartTrimPurgeFreeRamThreshold => 104.0,
        NumericField::SmartTrimWorkingSetThreshold
        | NumericField::SmartTrimIdleSeconds
        | NumericField::SmartTrimCooldownSeconds => 112.0,
        _ => 96.0,
    }
}

fn max_logical_processor_count() -> u8 {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .clamp(1, u8::MAX as usize) as u8
}

fn text_muted(value: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .opacity(0.72)
        .child(value.into())
}

fn text_danger(value: impl Into<SharedString>) -> gpui::Div {
    div().child(
        Tag::danger()
            .outline()
            .text_size(px(TEXT_BODY_SIZE))
            .child(value.into()),
    )
}

fn processor_power_column_header(value: impl Into<SharedString>) -> gpui::Div {
    div()
        .w_full()
        .min_w(px(0.0))
        .pb_1()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::BOLD)
        .child(value.into())
}

fn processor_power_slider(
    id: impl Into<SharedString>,
    label: &str,
    _value: u64,
    value_element: AnyElement,
    state: &Entity<SliderState>,
    window: &mut Window,
    cx: &mut Context<PowerLeafApp>,
    handler: impl Fn(&StepChange<u64>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    percent_slider_row(
        id,
        label,
        value_element,
        state,
        true,
        1_u64,
        window,
        cx,
        handler,
    )
}

fn processor_power_setting_row(
    id: &'static str,
    label: impl Into<SharedString>,
    value_element: AnyElement,
) -> AnyElement {
    h_flex()
        .id(id)
        .w_full()
        .min_w(px(0.0))
        .min_h(px(58.0))
        .items_center()
        .justify_between()
        .gap_2()
        .py_3()
        .px_4()
        .rounded_sm()
        .border_1()
        .border_color(rgb(border_color()))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .hover(|style| style.bg(rgb(settings_card_hover_color())))
        .child(
            div()
                .w(px(180.0))
                .min_w(px(120.0))
                .flex_shrink_0()
                .truncate()
                .child(label.into()),
        )
        .child(
            h_flex()
                .flex_1()
                .min_w(px(0.0))
                .justify_end()
                .child(value_element),
        )
        .into_any_element()
}

fn win32_priority_row(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    help: Option<String>,
    value_element: AnyElement,
) -> AnyElement {
    let id: SharedString = id.into();
    let mut label_row = h_flex()
        .flex_1()
        .min_w(px(0.0))
        .items_center()
        .gap_1()
        .child(div().min_w(px(0.0)).truncate().child(label.into()));
    if let Some(help) = help {
        label_row = label_row.child(title_info_button(format!("{id}-info"), help));
    }

    h_flex()
        .id(id)
        .w_full()
        .min_w(px(0.0))
        .min_h(px(58.0))
        .items_center()
        .justify_between()
        .gap_2()
        .py_3()
        .px_4()
        .rounded_sm()
        .border_1()
        .border_color(rgb(border_color()))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .hover(|style| style.bg(rgb(settings_card_hover_color())))
        .child(label_row)
        .child(
            h_flex()
                .w(px(260.0))
                .max_w(px(260.0))
                .items_center()
                .justify_end()
                .flex_shrink_0()
                .child(value_element),
        )
        .into_any_element()
}

fn threshold_level_slider(
    id: impl Into<SharedString>,
    label: &str,
    value_element: AnyElement,
    state: &Entity<SliderState>,
    enabled: bool,
    window: &mut Window,
    cx: &mut Context<PowerLeafApp>,
    handler: impl Fn(&StepChange<u8>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    rule_percent_slider_row(
        id,
        label,
        value_element,
        state,
        enabled,
        1_u8,
        window,
        cx,
        handler,
    )
}

fn stable_slider(
    state: &Entity<SliderState>,
    min: u64,
    max: u64,
    step: u64,
    enabled: bool,
    track_color: u32,
    thumb_color: u32,
    window: &mut Window,
    cx: &mut Context<PowerLeafApp>,
) -> AnyElement {
    let value = state.read(cx).value().end();
    let min = min.min(max);
    let max = max.max(min + u64::from(max == min));
    let step = step.max(1);
    let percentage = stable_slider_percentage(value, min, max);
    let track = Hsla::from(rgb(track_color));
    let bounds = Rc::new(RefCell::new(Bounds::<Pixels>::default()));
    let click_bounds = Rc::clone(&bounds);
    let drag_bounds = Rc::clone(&bounds);
    let canvas_bounds = Rc::clone(&bounds);
    let click_state = state.clone();
    let entity_id = state.entity_id();

    div()
        .id(("stable-slider", entity_id))
        .relative()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .w_full()
        .h(px(24.0))
        .when(enabled, |slider| {
            slider
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    cx.stop_propagation();
                    let bounds = *click_bounds.borrow();
                    click_state.update(cx, |state, cx| {
                        update_stable_slider_from_position(
                            state,
                            bounds,
                            event.position,
                            min,
                            max,
                            step,
                            window,
                            cx,
                        );
                    });
                })
                .on_drag(DragStableSlider(entity_id), |drag, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| drag.clone())
                })
                .on_drag_move(window.listener_for(
                    state,
                    move |state, event: &DragMoveEvent<DragStableSlider>, window, cx| {
                        match event.drag(cx) {
                            DragStableSlider(id) if *id == entity_id => {
                                update_stable_slider_from_position(
                                    state,
                                    *drag_bounds.borrow(),
                                    event.event.position,
                                    min,
                                    max,
                                    step,
                                    window,
                                    cx,
                                );
                            }
                            _ => {}
                        }
                    },
                ))
        })
        .child(
            div()
                .relative()
                .w_full()
                .h_1p5()
                .bg(track.opacity(0.2))
                .rounded_full()
                .child(
                    div()
                        .absolute()
                        .left(px(0.0))
                        .top(px(0.0))
                        .bottom(px(0.0))
                        .w(relative(percentage))
                        .bg(rgb(track_color))
                        .rounded_full(),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(-5.0))
                        .left(relative(percentage))
                        .ml(-px(8.0))
                        .size_4()
                        .p(px(1.0))
                        .rounded_full()
                        .bg(track.opacity(0.5))
                        .child(div().size_full().rounded_full().bg(rgb(thumb_color))),
                )
                .child(
                    canvas(
                        move |bounds, _, _| {
                            *canvas_bounds.borrow_mut() = bounds;
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                ),
        )
        .into_any_element()
}

fn stable_slider_percentage(value: f32, min: u64, max: u64) -> f32 {
    let min = min as f32;
    let max = max as f32;
    let range = max - min;
    if range <= 0.0 {
        0.0
    } else {
        ((value.clamp(min, max) - min) / range).clamp(0.0, 1.0)
    }
}

fn update_stable_slider_from_position(
    state: &mut SliderState,
    bounds: Bounds<Pixels>,
    position: Point<Pixels>,
    min: u64,
    max: u64,
    step: u64,
    window: &mut Window,
    cx: &mut Context<SliderState>,
) {
    let total_size = bounds.size.width;
    if total_size <= px(0.0) {
        return;
    }

    let percentage = (position.x - bounds.left()).clamp(px(0.0), total_size) / total_size;
    let min = min as f32;
    let max = max as f32;
    let step = step.max(1) as f32;
    let value = min + ((max - min) * percentage);
    let value = (((value - min) / step).round() * step + min).clamp(min, max);

    state.set_value(value, window, cx);
    cx.emit(SliderEvent::Change(SliderValue::Single(value)));
}

fn activity_slider_card(
    id: impl Into<SharedString>,
    label: &str,
    value_element: AnyElement,
    state: &Entity<SliderState>,
    enabled: bool,
    min: u64,
    max: u64,
    delta: u64,
    window: &mut Window,
    cx: &mut Context<PowerLeafApp>,
    handler: impl Fn(&StepChange<u64>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let handler: Rc<dyn Fn(&StepChange<u64>, &mut Window, &mut App)> = Rc::new(handler);
    let down = Rc::clone(&handler);
    let slider_track_color = if enabled {
        accent_color()
    } else {
        disabled_slider_track_color()
    };
    let slider_thumb_color = if enabled {
        windows_slider_thumb_color()
    } else {
        disabled_slider_thumb_color()
    };

    setting_action_card(
        id.clone(),
        label.to_owned(),
        h_flex()
            .items_center()
            .justify_end()
            .gap_2()
            .min_w(px(0.0))
            .flex_shrink_0()
            .child(
                control_button(Button::new((gpui::ElementId::from(id.clone()), "down")))
                    .label("-")
                    .disabled(!enabled)
                    .on_click(move |_, window, cx| {
                        down(
                            &StepChange {
                                delta,
                                increase: false,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .child(
                div()
                    .w(px(260.0))
                    .px(px(8.0))
                    .flex_none()
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(stable_slider(
                        state,
                        min,
                        max,
                        delta,
                        enabled,
                        slider_track_color,
                        slider_thumb_color,
                        window,
                        cx,
                    )),
            )
            .child(
                control_button(Button::new((gpui::ElementId::from(id), "up")))
                    .label("+")
                    .disabled(!enabled)
                    .on_click(move |_, window, cx| {
                        handler(
                            &StepChange {
                                delta,
                                increase: true,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .child(value_element)
            .into_any_element(),
    )
    .into_any_element()
}

fn rule_percent_slider_row<T>(
    id: impl Into<SharedString>,
    label: &str,
    value_element: AnyElement,
    state: &Entity<SliderState>,
    enabled: bool,
    delta: T,
    window: &mut Window,
    cx: &mut Context<PowerLeafApp>,
    handler: impl Fn(&StepChange<T>, &mut Window, &mut App) + 'static,
) -> AnyElement
where
    T: Copy + 'static,
{
    let id: SharedString = id.into();
    let handler: Rc<dyn Fn(&StepChange<T>, &mut Window, &mut App)> = Rc::new(handler);
    let down = Rc::clone(&handler);
    let down_delta = delta;
    let up_delta = delta;
    let label_color = if enabled {
        primary_text_color()
    } else {
        dim_text_color()
    };
    let slider_track_color = if enabled {
        accent_color()
    } else {
        disabled_slider_track_color()
    };
    let slider_thumb_color = if enabled {
        windows_slider_thumb_color()
    } else {
        disabled_slider_thumb_color()
    };

    rule_action_row_with_title_color(
        id.clone(),
        label.to_owned(),
        h_flex()
            .items_center()
            .justify_end()
            .gap_2()
            .min_w(px(0.0))
            .flex_shrink_0()
            .child(
                control_button(Button::new((gpui::ElementId::from(id.clone()), "down")))
                    .label("-")
                    .disabled(!enabled)
                    .on_click(move |_, window, cx| {
                        down(
                            &StepChange {
                                delta: down_delta,
                                increase: false,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .child(
                div()
                    .w(px(220.0))
                    .px(px(8.0))
                    .flex_none()
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(stable_slider(
                        state,
                        0,
                        100,
                        1,
                        enabled,
                        slider_track_color,
                        slider_thumb_color,
                        window,
                        cx,
                    )),
            )
            .child(
                control_button(Button::new((gpui::ElementId::from(id), "up")))
                    .label("+")
                    .disabled(!enabled)
                    .on_click(move |_, window, cx| {
                        handler(
                            &StepChange {
                                delta: up_delta,
                                increase: true,
                            },
                            window,
                            cx,
                        )
                    }),
            )
            .child(value_element)
            .into_any_element(),
        label_color,
    )
    .into_any_element()
}

fn percent_slider_row<T>(
    id: impl Into<SharedString>,
    label: &str,
    value_element: AnyElement,
    state: &Entity<SliderState>,
    enabled: bool,
    delta: T,
    window: &mut Window,
    cx: &mut Context<PowerLeafApp>,
    handler: impl Fn(&StepChange<T>, &mut Window, &mut App) + 'static,
) -> AnyElement
where
    T: Copy + 'static,
{
    let id: SharedString = id.into();
    let handler: Rc<dyn Fn(&StepChange<T>, &mut Window, &mut App)> = Rc::new(handler);
    let down = Rc::clone(&handler);
    let down_delta = delta;
    let up_delta = delta;
    let label_color = if enabled {
        primary_text_color()
    } else {
        dim_text_color()
    };
    let slider_track_color = if enabled {
        accent_color()
    } else {
        disabled_slider_track_color()
    };
    let slider_thumb_color = if enabled {
        windows_slider_thumb_color()
    } else {
        disabled_slider_thumb_color()
    };

    h_flex()
        .id(id.clone())
        .w_full()
        .min_h(px(58.0))
        .items_center()
        .justify_between()
        .gap_2()
        .py_3()
        .px_4()
        .rounded_sm()
        .border_1()
        .border_color(rgb(border_color()))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .hover(|style| style.bg(rgb(settings_card_hover_color())))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_color(rgb(label_color))
                .child(label.to_owned()),
        )
        .child(
            h_flex()
                .items_center()
                .justify_end()
                .gap_2()
                .min_w(px(0.0))
                .flex_shrink_0()
                .child(
                    control_button(Button::new((gpui::ElementId::from(id.clone()), "down")))
                        .label("-")
                        .disabled(!enabled)
                        .on_click(move |_, window, cx| {
                            down(
                                &StepChange {
                                    delta: down_delta,
                                    increase: false,
                                },
                                window,
                                cx,
                            )
                        }),
                )
                .child(
                    div()
                        .w(px(220.0))
                        .px(px(8.0))
                        .flex_none()
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .child(stable_slider(
                            state,
                            0,
                            100,
                            1,
                            enabled,
                            slider_track_color,
                            slider_thumb_color,
                            window,
                            cx,
                        )),
                )
                .child(
                    control_button(Button::new((gpui::ElementId::from(id), "up")))
                        .label("+")
                        .disabled(!enabled)
                        .on_click(move |_, window, cx| {
                            handler(
                                &StepChange {
                                    delta: up_delta,
                                    increase: true,
                                },
                                window,
                                cx,
                            )
                        }),
                )
                .child(value_element),
        )
        .into_any_element()
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

fn activity_slider_normalized_value(slider: ActivitySlider, value: u64) -> u64 {
    match slider {
        ActivitySlider::IdleTimeout => value.clamp(
            ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS,
            ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS,
        ),
        ActivitySlider::CheckInterval => snap_to_step(value, ACTIVITY_CHECK_INTERVAL_STEP_MS)
            .clamp(
                ACTIVITY_CHECK_INTERVAL_MIN_MS,
                ACTIVITY_CHECK_INTERVAL_MAX_MS,
            ),
    }
}

fn snap_to_step(value: u64, step: u64) -> u64 {
    if step == 0 {
        return value;
    }
    ((value + (step / 2)) / step) * step
}

fn seconds_label(seconds: u64) -> String {
    duration_label_ms(
        seconds
            .clamp(
                ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS,
                ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS,
            )
            .saturating_mul(1_000),
    )
}

fn milliseconds_label(milliseconds: u64) -> String {
    duration_label_ms(
        snap_to_step(milliseconds, ACTIVITY_CHECK_INTERVAL_STEP_MS).clamp(
            ACTIVITY_CHECK_INTERVAL_MIN_MS,
            ACTIVITY_CHECK_INTERVAL_MAX_MS,
        ),
    )
}

fn duration_label_ms(milliseconds: u64) -> String {
    if milliseconds < 1_000 {
        return format!("{milliseconds} ms");
    }

    let (value, unit) = if milliseconds < 60_000 {
        (milliseconds as f64 / 1_000.0, "sec")
    } else if milliseconds < 3_600_000 {
        (milliseconds as f64 / 60_000.0, "min")
    } else {
        (milliseconds as f64 / 3_600_000.0, "hr")
    };

    rounded_duration_value(value, unit)
}

fn rounded_duration_value(value: f64, unit: &str) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if (rounded - rounded.round()).abs() < f64::EPSILON {
        format!("{} {unit}", rounded.round() as u64)
    } else {
        format!("{rounded:.1} {unit}")
    }
}

fn parse_u64_input(value: &str, min: u64, max: u64) -> Option<u64> {
    value.parse::<u64>().ok().map(|value| value.clamp(min, max))
}

fn cpu_usage_label(percent: Option<f32>) -> String {
    percent
        .map(|percent| format!("{percent:.1}%"))
        .unwrap_or_else(|| t!("dashboard.collecting").to_string())
}

fn memory_usage_label(percent: Option<f32>) -> String {
    percent
        .map(|percent| format!("{percent:.1}%"))
        .unwrap_or_else(|| t!("dashboard.collecting").to_string())
}

fn io_usage_label(bytes_per_second: Option<f64>) -> String {
    bytes_per_second
        .map(format_bytes_per_second)
        .unwrap_or_else(|| t!("dashboard.collecting").to_string())
}

fn format_bytes_per_second(bytes_per_second: f64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    if bytes_per_second >= GIB {
        format!("{:.1} GB/s", bytes_per_second / GIB)
    } else if bytes_per_second >= MIB {
        format!("{:.1} MB/s", bytes_per_second / MIB)
    } else if bytes_per_second >= KIB {
        format!("{:.1} KB/s", bytes_per_second / KIB)
    } else {
        format!("{bytes_per_second:.0} B/s")
    }
}

fn input_hook_required(settings: &Settings) -> bool {
    settings.general.enabled
        && (activity_input_hook_required(settings) || settings.app_suspension.enabled)
}

fn input_hook_config(settings: &Settings) -> InputHookConfig {
    InputHookConfig {
        keyboard: settings.activity_mode.input_detection.keyboard
            || settings.app_suspension.enabled,
        mouse: settings.activity_mode.input_detection.mouse || settings.app_suspension.enabled,
    }
}

fn activity_input_hook_required(settings: &Settings) -> bool {
    settings.activity_mode.enabled
        && settings.activity_mode.switch_to_performance_on_resume
        && settings.activity_mode.input_detection.any_enabled()
        && (settings
            .activity_mode
            .power_plans
            .performance_guid
            .is_some()
            || settings.power_plans.performance_guid.is_some())
}

fn process_target_can_accept(target: SuggestionTarget, settings: &Settings, process: &str) -> bool {
    match target {
        SuggestionTarget::Foreground => {
            can_add_foreground_process(&settings.foreground_rules, process)
        }
        SuggestionTarget::EcoQos => can_add_eco_qos_process(&settings.eco_qos, process),
        SuggestionTarget::BackgroundCpu => {
            can_add_background_cpu_exclusion(&settings.background_cpu_restriction, process)
        }
        SuggestionTarget::SmartTrim => can_add_smart_trim_exclusion(&settings.smart_trim, process),
        SuggestionTarget::Suspension => {
            can_add_suspension_process(&settings.app_suspension, process)
        }
        SuggestionTarget::CpuLimiter => can_add_cpu_limiter_process(&settings.cpu_limiter, process),
        SuggestionTarget::Watchdog => can_add_watchdog_process(&settings.watchdog, process),
        SuggestionTarget::PerformanceMode => {
            can_add_performance_mode_process(&settings.performance_mode, process)
        }
        SuggestionTarget::Responsiveness => {
            can_add_responsiveness_process(&settings.foreground_responsiveness, process)
        }
        SuggestionTarget::IoPriority => can_add_io_priority_process(&settings.io_priority, process),
        SuggestionTarget::MemoryPriority => {
            can_add_memory_priority_process(&settings.memory_priority, process)
        }
        SuggestionTarget::Affinity => can_add_affinity_process(&settings.cpu_affinity, process),
    }
}

fn can_add_foreground_process(settings: &ForegroundRules, process: &str) -> bool {
    let process = process.trim();
    !process.is_empty()
        && !settings
            .rules
            .iter()
            .any(|rule| rule.process_name.trim().eq_ignore_ascii_case(process))
}

fn new_foreground_rule(process: &str, power_plan_guid: Option<String>) -> ForegroundRule {
    let process_name = process.trim().to_ascii_lowercase();
    ForegroundRule {
        enabled: true,
        name: process_name.clone(),
        process_name,
        power_plan_guid,
    }
}

fn can_add_eco_qos_process(settings: &EcoQosSettings, process: &str) -> bool {
    let process = process.trim();
    !process.is_empty()
        && !ecoqos::is_builtin_excluded(process)
        && !settings.contains_efficiency_exclusion(process)
}

fn can_add_background_cpu_exclusion(
    settings: &BackgroundCpuRestrictionSettings,
    process: &str,
) -> bool {
    let process = process.trim();
    !process.is_empty()
        && !affinity::is_builtin_excluded(process)
        && !settings.contains_exclusion(process)
}

fn can_add_smart_trim_exclusion(settings: &SmartTrimSettings, process: &str) -> bool {
    let process = process.trim();
    !process.is_empty()
        && !smart_trim::is_builtin_excluded(process)
        && !settings.exclusion_enabled_for(process)
        && !settings
            .exclusions
            .iter()
            .any(|rule| rule.process_name.trim().eq_ignore_ascii_case(process))
}

fn new_process_exclusion_rule(process: &str) -> ProcessExclusionRule {
    ProcessExclusionRule {
        enabled: true,
        process_name: process.trim().to_ascii_lowercase(),
    }
}

fn new_eco_qos_exclusion_rule(process: &str) -> EcoQosExclusionRule {
    EcoQosExclusionRule {
        enabled: true,
        process_name: process.trim().to_ascii_lowercase(),
    }
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

fn can_add_responsiveness_process(
    settings: &ForegroundResponsivenessSettings,
    process: &str,
) -> bool {
    let process = process.trim();
    !process.is_empty()
        && !settings.contains_rule_for(process)
        && !responsiveness::is_builtin_excluded(process)
}

fn can_add_io_priority_process(settings: &IoPrioritySettings, process: &str) -> bool {
    let process = process.trim();
    !process.is_empty()
        && !settings
            .rules
            .iter()
            .any(|rule| rule.process_name.trim().eq_ignore_ascii_case(process))
}

fn can_add_memory_priority_process(settings: &MemoryPrioritySettings, process: &str) -> bool {
    let process = process.trim();
    !process.is_empty()
        && !memory_priority::is_builtin_excluded(process)
        && !settings
            .rules
            .iter()
            .any(|rule| rule.process_name.trim().eq_ignore_ascii_case(process))
}

fn can_add_responsiveness_exclusion(
    settings: &ForegroundResponsivenessSettings,
    process: &str,
) -> bool {
    let process = process.trim();
    !process.is_empty() && !settings.contains_auto_balance_exclusion(process)
}

fn can_add_cpu_limiter_process(settings: &CpuLimiterSettings, process: &str) -> bool {
    let process = process.trim();
    !process.is_empty()
        && !settings
            .rules
            .iter()
            .any(|rule| rule.process_name.trim().eq_ignore_ascii_case(process))
        && !cpu_limiter::is_builtin_excluded(process)
}

fn can_add_watchdog_process(settings: &WatchdogSettings, process: &str) -> bool {
    let process = process.trim();
    !process.is_empty()
        && !settings
            .rules
            .iter()
            .any(|rule| rule.process_name.trim().eq_ignore_ascii_case(process))
        && !watchdog::is_builtin_excluded(process)
}

fn can_add_performance_mode_process(settings: &PerformanceModeSettings, process: &str) -> bool {
    let process = process.trim();
    !process.is_empty()
        && !settings
            .rules
            .iter()
            .any(|rule| rule.process_name.trim().eq_ignore_ascii_case(process))
        && !performance_mode::is_builtin_excluded(process)
}

fn new_suspension_rule(process: &str) -> AppSuspensionRule {
    AppSuspensionRule {
        enabled: true,
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
        mode: CpuAffinityMode::Soft,
        process_name: process.trim().to_ascii_lowercase(),
        core_mask: default_affinity_mask(),
    }
}

fn new_io_priority_rule(process: &str) -> IoPriorityRule {
    IoPriorityRule {
        enabled: true,
        process_name: process.trim().to_ascii_lowercase(),
        priority: ProcessIoPriority::VeryLow,
    }
}

fn new_memory_priority_rule(process: &str) -> MemoryPriorityRule {
    MemoryPriorityRule {
        enabled: true,
        process_name: process.trim().to_ascii_lowercase(),
        priority: ProcessMemoryPriority::Low,
    }
}

fn new_cpu_limiter_rule(process: &str) -> CpuLimiterRule {
    CpuLimiterRule {
        enabled: true,
        process_name: process.trim().to_ascii_lowercase(),
        threshold_percent: 75,
        sustain_seconds: 5,
        cooldown_seconds: 10,
        max_logical_processors: 1,
    }
}

fn new_watchdog_rule(process: &str, action: WatchdogAction) -> WatchdogRule {
    let process_name = process.trim().to_ascii_lowercase();
    let launch_path = if action == WatchdogAction::RestartIfExited {
        if Path::new(process).is_absolute() {
            process_name.clone()
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    WatchdogRule {
        enabled: true,
        name: process_name.clone(),
        process_name: process_name.clone(),
        action,
        launch_path,
        launch_args: Vec::new(),
        restart_delay_seconds: 5,
    }
}

fn split_watchdog_args(value: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut token = String::new();
    let mut in_quotes = false;
    let mut escaping = false;
    let mut token_started = false;

    for character in value.chars() {
        if in_quotes {
            if escaping {
                if matches!(character, '"' | '\\') {
                    token.push(character);
                } else {
                    token.push('\\');
                    token.push(character);
                }
                token_started = true;
                escaping = false;
                continue;
            }

            match character {
                '"' => {
                    in_quotes = false;
                    token_started = true;
                }
                '\\' => {
                    escaping = true;
                }
                _ => {
                    token.push(character);
                    token_started = true;
                }
            }
            continue;
        }

        match character {
            '"' => {
                in_quotes = true;
                token_started = true;
            }
            '\\' => {
                token.push('\\');
                token_started = true;
            }
            character if character.is_whitespace() => {
                if token_started {
                    args.push(sanitize_watchdog_arg(&token));
                    token = String::new();
                    token_started = false;
                }
            }
            _ => {
                token.push(character);
                token_started = true;
            }
        }
    }

    if escaping {
        token.push('\\');
    }

    if token_started {
        args.push(sanitize_watchdog_arg(&token));
    }

    args
}

fn sanitize_watchdog_launch_path(value: &str) -> String {
    value.trim().trim_matches('"').to_owned()
}

fn sanitize_watchdog_arg(value: &str) -> String {
    value.to_owned()
}

fn watchdog_action_label(action: WatchdogAction) -> String {
    match action {
        WatchdogAction::TerminateOnLaunch => t!("watchdog.action_terminate").to_string(),
        WatchdogAction::RestartIfExited => t!("watchdog.action_restart").to_string(),
    }
}

fn watchdog_indicator(status: &WatchdogSnapshot, process: &str) -> (String, u32, u32) {
    if watchdog::is_builtin_excluded(process) {
        (
            t!("affinity.indicator.protected").to_string(),
            settings_card_hover_color(),
            accent_color(),
        )
    } else if status.enabled {
        (
            t!("affinity.indicator.ready").to_string(),
            panel_active_color(),
            muted_text_color(),
        )
    } else {
        (
            t!("affinity.indicator.off").to_string(),
            panel_active_color(),
            dim_text_color(),
        )
    }
}

fn cpu_limiter_indicator(status: &CpuLimiterSnapshot, process: &str) -> (String, u32, u32) {
    if cpu_limiter::is_builtin_excluded(process) {
        (
            t!("affinity.indicator.protected").to_string(),
            settings_card_hover_color(),
            accent_color(),
        )
    } else if cpu_limiter_app_contains(&status.limited_apps, process) {
        (
            t!("cpu_limiter.indicator_limited").to_string(),
            success_bg_color(),
            success_text_color(),
        )
    } else if status.enabled {
        (
            t!("affinity.indicator.ready").to_string(),
            panel_active_color(),
            muted_text_color(),
        )
    } else {
        (
            t!("affinity.indicator.off").to_string(),
            panel_active_color(),
            dim_text_color(),
        )
    }
}

fn cpu_limiter_app_contains(apps: &[String], process: &str) -> bool {
    apps.iter()
        .any(|app| app.trim().eq_ignore_ascii_case(process.trim()))
}

#[allow(dead_code)]
fn new_responsiveness_rule(process: &str) -> PriorityRule {
    PriorityRule {
        enabled: true,
        process_name: process.trim().to_ascii_lowercase(),
        priority: ProcessPriority::BelowNormal,
    }
}

fn new_performance_mode_rule(
    process: &str,
    power_plan_guid: Option<String>,
) -> PerformanceModeRule {
    let process_name = process.trim().to_ascii_lowercase();
    PerformanceModeRule {
        enabled: true,
        name: process_name.clone(),
        process_name,
        power_plan_guid,
    }
}

fn performance_mode_decision(status: &PerformanceModeSnapshot) -> Option<PerformanceModeDecision> {
    Some(PerformanceModeDecision {
        rule_name: status.active_rule.clone()?,
        process_name: status.active_process.clone()?,
        power_plan_guid: status.target_guid.clone()?,
    })
}

#[allow(dead_code)]
fn process_priority_label(priority: ProcessPriority) -> String {
    match priority {
        ProcessPriority::Normal => t!("responsiveness.priority_normal").to_string(),
        ProcessPriority::BelowNormal => t!("responsiveness.priority_below_normal").to_string(),
        ProcessPriority::Idle => t!("responsiveness.priority_idle").to_string(),
    }
}

fn process_io_priority_label(priority: ProcessIoPriority) -> String {
    match priority {
        ProcessIoPriority::Normal => t!("io_priority.priority_normal").to_string(),
        ProcessIoPriority::Low => t!("io_priority.priority_low").to_string(),
        ProcessIoPriority::VeryLow => t!("io_priority.priority_very_low").to_string(),
    }
}

fn process_memory_priority_label(priority: ProcessMemoryPriority) -> String {
    match priority {
        ProcessMemoryPriority::VeryLow => t!("responsiveness.memory_priority_very_low").to_string(),
        ProcessMemoryPriority::Low => t!("responsiveness.memory_priority_low").to_string(),
        ProcessMemoryPriority::Medium => t!("responsiveness.memory_priority_medium").to_string(),
        ProcessMemoryPriority::BelowNormal => {
            t!("responsiveness.memory_priority_below_normal").to_string()
        }
        ProcessMemoryPriority::Normal => t!("responsiveness.memory_priority_normal").to_string(),
    }
}

fn cpu_affinity_mode_label(mode: CpuAffinityMode) -> String {
    match mode {
        CpuAffinityMode::Hard => t!("affinity.mode_hard").to_string(),
        CpuAffinityMode::Soft => t!("affinity.mode_soft").to_string(),
        CpuAffinityMode::EfficiencyOff => t!("affinity.mode_efficiency_off").to_string(),
    }
}

fn io_priority_contains_process(processes: &[String], process: &str) -> bool {
    processes
        .iter()
        .any(|name| name.trim().eq_ignore_ascii_case(process.trim()))
}

fn auto_balance_preset_label(preset: AutoBalancePreset) -> String {
    match preset {
        AutoBalancePreset::Gentle => t!("responsiveness.preset_gentle").to_string(),
        AutoBalancePreset::Balanced => t!("responsiveness.preset_balanced").to_string(),
        AutoBalancePreset::Responsive => t!("responsiveness.preset_responsive").to_string(),
    }
}

fn auto_balance_behavior_label(behavior: AutoBalanceBehavior) -> String {
    match behavior {
        AutoBalanceBehavior::Preset(preset) => auto_balance_preset_label(preset),
    }
}

fn apply_auto_balance_behavior(
    settings: &mut ForegroundResponsivenessSettings,
    behavior: AutoBalanceBehavior,
) {
    match behavior {
        AutoBalanceBehavior::Preset(preset) => {
            settings.auto_balance_enabled = true;
            apply_auto_balance_preset(settings, preset);
        }
    }
}

fn auto_balance_matches_behavior(
    settings: &ForegroundResponsivenessSettings,
    behavior: AutoBalanceBehavior,
) -> bool {
    match behavior {
        AutoBalanceBehavior::Preset(preset) => {
            settings.auto_balance_enabled && auto_balance_matches_preset(settings, preset)
        }
    }
}

fn apply_auto_balance_preset(
    settings: &mut ForegroundResponsivenessSettings,
    preset: AutoBalancePreset,
) {
    let values = auto_balance_preset_values(preset);
    settings.lower_background_apps = values.lower_background_apps;
    settings.lower_background_io_priority_enabled = values.lower_background_io_priority_enabled;
    settings.lower_background_io_priority = values.lower_background_io_priority;
    settings.auto_balance_memory_priority_enabled = values.auto_balance_memory_priority_enabled;
    settings.auto_balance_memory_priority = values.auto_balance_memory_priority;
    settings.auto_balance_affinity_escalation_enabled =
        values.auto_balance_affinity_escalation_enabled;
    settings.boost_foreground_app = values.boost_foreground_app;
    if values.boost_foreground_app {
        settings.foreground_boost = ForegroundBoostPriority::Auto;
    }
    settings.lower_background_auto_cpu_percent = true;
    settings.auto_balance_cpu_percent = values.manual_cpu_percent;
    settings.auto_balance_total_threshold_percent = values.total_threshold;
    settings.auto_balance_threshold_percent = values.process_threshold;
    settings.auto_balance_restore_threshold_percent = values.restore_threshold;
    settings.auto_balance_sustain_seconds = values.sustain_seconds;
    settings.auto_balance_minimum_restraint_seconds = values.minimum_restraint_seconds;
    settings.auto_balance_cooldown_seconds = values.cooldown_seconds;
}

fn auto_balance_matches_preset(
    settings: &ForegroundResponsivenessSettings,
    preset: AutoBalancePreset,
) -> bool {
    let values = auto_balance_preset_values(preset);
    settings.lower_background_apps == values.lower_background_apps
        && settings.lower_background_io_priority_enabled
            == values.lower_background_io_priority_enabled
        && settings.lower_background_io_priority == values.lower_background_io_priority
        && settings.auto_balance_memory_priority_enabled
            == values.auto_balance_memory_priority_enabled
        && settings.auto_balance_memory_priority == values.auto_balance_memory_priority
        && settings.auto_balance_affinity_escalation_enabled
            == values.auto_balance_affinity_escalation_enabled
        && settings.boost_foreground_app == values.boost_foreground_app
        && (!values.boost_foreground_app
            || settings.foreground_boost == ForegroundBoostPriority::Auto)
        && settings.lower_background_auto_cpu_percent
        && settings.auto_balance_total_threshold_percent == values.total_threshold
        && settings.auto_balance_threshold_percent == values.process_threshold
        && settings.auto_balance_restore_threshold_percent == values.restore_threshold
        && settings.auto_balance_sustain_seconds == values.sustain_seconds
        && settings.auto_balance_minimum_restraint_seconds == values.minimum_restraint_seconds
        && settings.auto_balance_cooldown_seconds == values.cooldown_seconds
}

#[derive(Clone, Copy)]
struct AutoBalancePresetValues {
    lower_background_apps: bool,
    lower_background_io_priority_enabled: bool,
    lower_background_io_priority: ProcessIoPriority,
    auto_balance_memory_priority_enabled: bool,
    auto_balance_memory_priority: ProcessMemoryPriority,
    auto_balance_affinity_escalation_enabled: bool,
    boost_foreground_app: bool,
    manual_cpu_percent: u8,
    total_threshold: u8,
    process_threshold: u8,
    restore_threshold: u8,
    sustain_seconds: u64,
    minimum_restraint_seconds: u64,
    cooldown_seconds: u64,
}

fn auto_balance_preset_values(preset: AutoBalancePreset) -> AutoBalancePresetValues {
    match preset {
        AutoBalancePreset::Gentle => AutoBalancePresetValues {
            lower_background_apps: false,
            lower_background_io_priority_enabled: false,
            lower_background_io_priority: ProcessIoPriority::Low,
            auto_balance_memory_priority_enabled: false,
            auto_balance_memory_priority: ProcessMemoryPriority::Low,
            auto_balance_affinity_escalation_enabled: false,
            boost_foreground_app: false,
            manual_cpu_percent: 90,
            total_threshold: 80,
            process_threshold: 35,
            restore_threshold: 15,
            sustain_seconds: 4,
            minimum_restraint_seconds: 2,
            cooldown_seconds: 4,
        },
        AutoBalancePreset::Balanced => AutoBalancePresetValues {
            lower_background_apps: true,
            lower_background_io_priority_enabled: true,
            lower_background_io_priority: ProcessIoPriority::Low,
            auto_balance_memory_priority_enabled: true,
            auto_balance_memory_priority: ProcessMemoryPriority::Low,
            auto_balance_affinity_escalation_enabled: false,
            boost_foreground_app: true,
            manual_cpu_percent: 75,
            total_threshold: 70,
            process_threshold: 22,
            restore_threshold: 8,
            sustain_seconds: 2,
            minimum_restraint_seconds: 3,
            cooldown_seconds: 5,
        },
        AutoBalancePreset::Responsive => AutoBalancePresetValues {
            lower_background_apps: true,
            lower_background_io_priority_enabled: true,
            lower_background_io_priority: ProcessIoPriority::Low,
            auto_balance_memory_priority_enabled: true,
            auto_balance_memory_priority: ProcessMemoryPriority::VeryLow,
            auto_balance_affinity_escalation_enabled: true,
            boost_foreground_app: true,
            manual_cpu_percent: 60,
            total_threshold: 55,
            process_threshold: 10,
            restore_threshold: 5,
            sustain_seconds: 1,
            minimum_restraint_seconds: 3,
            cooldown_seconds: 6,
        },
    }
}

fn foreground_boost_priority_label(priority: ForegroundBoostPriority) -> String {
    match priority {
        ForegroundBoostPriority::Auto => t!("responsiveness.priority_auto").to_string(),
        ForegroundBoostPriority::Normal => t!("responsiveness.priority_normal").to_string(),
        ForegroundBoostPriority::AboveNormal => {
            t!("responsiveness.priority_above_normal").to_string()
        }
    }
}

fn efficiency_aggressiveness_label(aggressiveness: EcoQosAggressiveness) -> String {
    match aggressiveness {
        EcoQosAggressiveness::Safe => t!("efficiency.aggressiveness_safe").to_string(),
        EcoQosAggressiveness::Balanced => t!("efficiency.aggressiveness_balanced").to_string(),
        EcoQosAggressiveness::Aggressive => t!("efficiency.aggressiveness_aggressive").to_string(),
    }
}

struct SuspensionIndicator {
    label: String,
    bg: u32,
    fg: u32,
}

struct AffinityIndicator {
    label: String,
    bg: u32,
    fg: u32,
    hover: String,
}

#[derive(Clone, Copy)]
enum CoreTileGridAction {
    EcoQosCpuRestriction { available_mask: u64 },
    BackgroundCpuRestriction { available_mask: u64 },
    CpuAffinityRule { index: usize },
}

fn suspension_indicator(status: &AppSuspensionSnapshot, process: &str) -> SuspensionIndicator {
    let accent = accent_color();
    let accent_bg = settings_card_hover_color();
    if suspension::is_builtin_excluded(process) {
        SuspensionIndicator {
            label: t!("suspension.indicator.protected").to_string(),
            bg: accent_bg,
            fg: accent,
        }
    } else if suspension::contains_process(&status.network_wake_apps, process) {
        SuspensionIndicator {
            label: t!("suspension.indicator.network").to_string(),
            bg: accent_bg,
            fg: accent,
        }
    } else if suspension::contains_process(&status.audio_wake_apps, process) {
        SuspensionIndicator {
            label: t!("suspension.indicator.audio").to_string(),
            bg: accent_bg,
            fg: accent,
        }
    } else if suspension::contains_process(&status.suspended_apps, process) {
        SuspensionIndicator {
            label: t!("suspension.indicator.frozen").to_string(),
            bg: success_bg_color(),
            fg: success_text_color(),
        }
    } else if suspension::contains_process(&status.temporary_thawed_apps, process) {
        SuspensionIndicator {
            label: t!("suspension.indicator.thawed").to_string(),
            bg: accent_bg,
            fg: accent,
        }
    } else if suspension::contains_process(&status.tracked_apps, process) {
        SuspensionIndicator {
            label: t!("suspension.indicator.waiting").to_string(),
            bg: warning_bg_color(),
            fg: warning_text_color(),
        }
    } else if status.enabled {
        SuspensionIndicator {
            label: t!("suspension.indicator.not_running").to_string(),
            bg: panel_active_color(),
            fg: muted_text_color(),
        }
    } else {
        SuspensionIndicator {
            label: t!("suspension.indicator.off").to_string(),
            bg: panel_active_color(),
            fg: dim_text_color(),
        }
    }
}

fn affinity_indicator(status: &CpuAffinitySnapshot, process: &str) -> AffinityIndicator {
    let accent = accent_color();
    let accent_bg = settings_card_hover_color();
    if affinity::is_builtin_excluded(process) {
        AffinityIndicator {
            label: t!("affinity.indicator.protected").to_string(),
            bg: accent_bg,
            fg: accent,
            hover: t!("affinity.indicator.protected_help").to_string(),
        }
    } else if affinity::contains_process(&status.adjusted_apps, process) {
        AffinityIndicator {
            label: t!("affinity.indicator.pinned").to_string(),
            bg: success_bg_color(),
            fg: success_text_color(),
            hover: t!("affinity.indicator.pinned_help").to_string(),
        }
    } else if status.enabled {
        AffinityIndicator {
            label: t!("affinity.indicator.ready").to_string(),
            bg: panel_active_color(),
            fg: muted_text_color(),
            hover: t!("affinity.indicator.ready_help").to_string(),
        }
    } else {
        AffinityIndicator {
            label: t!("affinity.indicator.off").to_string(),
            bg: panel_active_color(),
            fg: dim_text_color(),
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

fn action_log_mode_label(mode: ActionLogMode) -> String {
    match mode {
        ActionLogMode::Full => t!("settings.action_log_mode_full").to_string(),
        ActionLogMode::Warning => t!("settings.action_log_mode_warning").to_string(),
        ActionLogMode::Error => t!("settings.action_log_mode_error").to_string(),
        ActionLogMode::Off => t!("settings.action_log_mode_off").to_string(),
    }
}

fn action_log_mode_help(mode: ActionLogMode) -> String {
    match mode {
        ActionLogMode::Full => t!("settings.action_log_mode_full_help").to_string(),
        ActionLogMode::Warning => t!("settings.action_log_mode_warning_help").to_string(),
        ActionLogMode::Error => t!("settings.action_log_mode_error_help").to_string(),
        ActionLogMode::Off => t!("settings.action_log_mode_off_help").to_string(),
    }
}

fn efficiency_cpu_restriction_mode_label(mode: EcoQosCpuRestrictionMode) -> String {
    match mode {
        EcoQosCpuRestrictionMode::SoftCpuSets => t!("efficiency.cpu_restriction_soft").to_string(),
        EcoQosCpuRestrictionMode::HardAffinity => t!("efficiency.cpu_restriction_hard").to_string(),
    }
}

fn efficiency_cpu_restriction_strategy_label(strategy: EcoQosCpuRestrictionStrategy) -> String {
    match strategy {
        EcoQosCpuRestrictionStrategy::Off => t!("efficiency.cpu_set_off").to_string(),
        EcoQosCpuRestrictionStrategy::Auto => t!("efficiency.cpu_set_auto").to_string(),
        EcoQosCpuRestrictionStrategy::PreferEfficiencyCores => {
            t!("efficiency.cpu_set_prefer_e_cores").to_string()
        }
        EcoQosCpuRestrictionStrategy::LimitLogicalCpus => {
            t!("efficiency.cpu_set_limit_logical").to_string()
        }
    }
}

fn efficiency_cpu_restriction_control_style_label(
    style: EcoQosCpuRestrictionControlStyle,
) -> String {
    match style {
        EcoQosCpuRestrictionControlStyle::Percentage => {
            t!("efficiency.control_style_percentage").to_string()
        }
        EcoQosCpuRestrictionControlStyle::CoreToggle => {
            t!("efficiency.control_style_core_toggle").to_string()
        }
    }
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

fn toggle_affinity_core_with_available_mask(mask: &mut u64, core: usize, available_mask: u64) {
    *mask &= available_mask;
    let Some(bit) = affinity_processor_bit(core) else {
        return;
    };
    if (available_mask & bit) == 0 {
        return;
    }

    if (*mask & bit) == 0 {
        *mask |= bit;
    } else if mask.count_ones() > 1 {
        *mask &= !bit;
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

fn affinity_processors_no_smt_mask(processors: &[LogicalProcessorInfo]) -> u64 {
    let mut seen_cores = Vec::new();
    let mut mask = 0;

    for processor in processors {
        if seen_cores.contains(&processor.core_index) {
            continue;
        }
        seen_cores.push(processor.core_index);
        if let Some(bit) = affinity_processor_bit(processor.index) {
            mask |= bit;
        }
    }

    mask
}

fn eco_qos_strategy_core_mask(
    processors: &[LogicalProcessorInfo],
    strategy: EcoQosCpuRestrictionStrategy,
) -> u64 {
    match strategy {
        EcoQosCpuRestrictionStrategy::Off => 0,
        EcoQosCpuRestrictionStrategy::Auto => {
            let efficiency_mask =
                affinity_processors_kind_mask(processors, LogicalProcessorKind::Efficiency);
            if efficiency_mask != 0 {
                efficiency_mask
            } else {
                affinity_processors_mask(processors)
            }
        }
        EcoQosCpuRestrictionStrategy::PreferEfficiencyCores => {
            affinity_processors_kind_mask(processors, LogicalProcessorKind::Efficiency)
        }
        EcoQosCpuRestrictionStrategy::LimitLogicalCpus => affinity_processors_mask(processors),
    }
}

fn affinity_processor_bit(index: usize) -> Option<u64> {
    (index < 64).then_some(1_u64 << index)
}

fn core_tile_kind_label(processor: &LogicalProcessorInfo) -> String {
    match processor.kind {
        LogicalProcessorKind::Performance => "P-Core".to_owned(),
        LogicalProcessorKind::Efficiency => "E-Core".to_owned(),
        LogicalProcessorKind::Standard => "Core".to_owned(),
    }
}

fn processor_power_preset_label(preset: ProcessorPowerPreset) -> String {
    match preset {
        ProcessorPowerPreset::Performance => t!("processor_power.performance").to_string(),
        ProcessorPowerPreset::Balanced => t!("processor_power.balanced").to_string(),
        ProcessorPowerPreset::Saver => t!("processor_power.saver").to_string(),
    }
}

fn processor_boost_mode_label(boost_mode: ProcessorBoostMode) -> String {
    match boost_mode {
        ProcessorBoostMode::Disabled => t!("processor_power.boost_disabled").to_string(),
        ProcessorBoostMode::Enabled => t!("processor_power.boost_enabled").to_string(),
        ProcessorBoostMode::Aggressive => t!("processor_power.boost_aggressive").to_string(),
        ProcessorBoostMode::EfficientEnabled => {
            t!("processor_power.boost_efficient_enabled").to_string()
        }
        ProcessorBoostMode::EfficientAggressive => {
            t!("processor_power.boost_efficient_aggressive").to_string()
        }
        ProcessorBoostMode::AggressiveAtGuaranteed => {
            t!("processor_power.boost_aggressive_at_guaranteed").to_string()
        }
        ProcessorBoostMode::EfficientAggressiveAtGuaranteed => {
            t!("processor_power.boost_efficient_aggressive_at_guaranteed").to_string()
        }
    }
}

const fn processor_boost_mode_picker_id(source: ProcessorPowerSource) -> &'static str {
    match source {
        ProcessorPowerSource::Ac => "processor-power-ac-boost-mode-picker",
        ProcessorPowerSource::Dc => "processor-power-dc-boost-mode-picker",
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

fn network_threshold_edit_value(threshold_bytes: u64, unit: NetworkThresholdUnit) -> String {
    let value = unit.threshold_value_from_bytes(threshold_bytes);
    network_threshold_value_label(value)
}

fn network_threshold_value_label(value: f64) -> String {
    format!("{value:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
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

fn choose_action_log_export_file(hwnd: Option<HWND>) -> Result<Option<PathBuf>, String> {
    const FILE_BUFFER_LEN: usize = 4096;

    let mut file_buffer =
        path_to_wide_buffer(&default_action_log_export_csv_path(), FILE_BUFFER_LEN);
    let filter = wide_nulls("CSV files (*.csv)\0*.csv\0All files (*.*)\0*.*\0");
    let default_extension = wide_null("csv");
    let title = wide_null("Export log");

    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd.unwrap_or_default(),
        lpstrFilter: filter.as_ptr(),
        lpstrFile: file_buffer.as_mut_ptr(),
        nMaxFile: file_buffer.len() as u32,
        lpstrTitle: title.as_ptr(),
        lpstrDefExt: default_extension.as_ptr(),
        Flags: OFN_HIDEREADONLY | OFN_NOCHANGEDIR | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
        ..Default::default()
    };

    let selected = unsafe { GetSaveFileNameW(&mut dialog) };
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

fn default_action_log_export_csv_path() -> PathBuf {
    config::storage::config_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(format!(
            "powerleaf_action_log_{}_{}.csv",
            env!("CARGO_PKG_VERSION"),
            Local::now().format("%Y-%m-%d")
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_smt_mask_selects_one_logical_cpu_per_physical_core() {
        let processors = vec![
            LogicalProcessorInfo {
                index: 0,
                core_index: 0,
                kind: LogicalProcessorKind::Standard,
                efficiency_class: 0,
            },
            LogicalProcessorInfo {
                index: 1,
                core_index: 0,
                kind: LogicalProcessorKind::Standard,
                efficiency_class: 0,
            },
            LogicalProcessorInfo {
                index: 2,
                core_index: 1,
                kind: LogicalProcessorKind::Standard,
                efficiency_class: 0,
            },
            LogicalProcessorInfo {
                index: 3,
                core_index: 1,
                kind: LogicalProcessorKind::Standard,
                efficiency_class: 0,
            },
        ];

        assert_eq!(affinity_processors_no_smt_mask(&processors), 0b0101);
    }

    #[test]
    fn topology_aware_core_toggle_keeps_one_available_cpu_selected() {
        let mut mask = (1_u64 << 63) | 0b0001;
        toggle_affinity_core_with_available_mask(&mut mask, 0, 0b0011);

        assert_eq!(mask, 0b0001);

        toggle_affinity_core_with_available_mask(&mut mask, 1, 0b0011);
        assert_eq!(mask, 0b0011);

        toggle_affinity_core_with_available_mask(&mut mask, 0, 0b0011);
        assert_eq!(mask, 0b0010);
    }

    #[test]
    fn new_core_steering_rules_default_to_soft_cpu_sets() {
        let rule = new_affinity_rule("game.exe");

        assert_eq!(rule.mode, CpuAffinityMode::Soft);
    }

    #[test]
    fn csv_escape_quotes_fields_with_special_characters() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("two,parts"), "\"two,parts\"");
        assert_eq!(csv_escape("quoted \"value\""), "\"quoted \"\"value\"\"\"");
        assert_eq!(csv_escape("line\r\nbreak"), "\"line\r\nbreak\"");
    }

    #[test]
    fn action_log_entries_export_as_csv() {
        let entries = vec![ActionLogEntry {
            sequence: 7,
            timestamp_epoch_ms: 1_700_000_000_000,
            feature: ActionLogFeature::Watchdog,
            process_id: Some(42),
            process_name: "worker.exe".to_owned(),
            action: ActionLogAction::Fail,
            result: ActionLogResult::Failed,
            reason: "Restart failed, access denied".to_owned(),
        }];

        let csv = action_log_entries_to_csv(&entries);

        assert!(csv.starts_with(
            "sequence,timestamp,feature,process_id,process_name,action,result,reason\r\n"
        ));
        assert!(csv.contains(
            ",Watchdog Rules,42,worker.exe,Fail,Failed,\"Restart failed, access denied\"\r\n"
        ));
    }

    #[test]
    fn processor_power_slider_pairs_ac_and_battery_controls() {
        assert_eq!(
            ProcessorPowerSlider::AcCoreParkingMin.paired_power_source(),
            ProcessorPowerSlider::DcCoreParkingMin
        );
        assert_eq!(
            ProcessorPowerSlider::AcPerformanceMin.paired_power_source(),
            ProcessorPowerSlider::DcPerformanceMin
        );
        assert_eq!(
            ProcessorPowerSlider::AcPerformanceMax.paired_power_source(),
            ProcessorPowerSlider::DcPerformanceMax
        );
        assert_eq!(
            ProcessorPowerSlider::DcCoreParkingMin.paired_power_source(),
            ProcessorPowerSlider::AcCoreParkingMin
        );
    }

    #[test]
    fn processor_power_action_preserves_ac_and_dc_values() {
        let action = processor_power_action(
            "plan-guid",
            ProcessorPowerAcDcValues {
                ac: ProcessorPowerValues::new_with_boost_mode(
                    10,
                    20,
                    90,
                    ProcessorBoostMode::Aggressive,
                ),
                dc: ProcessorPowerValues::new_with_boost_mode(
                    5,
                    15,
                    70,
                    ProcessorBoostMode::EfficientEnabled,
                ),
            },
        );

        assert!(matches!(
            action,
            Action::SetProcessorPowerValues {
                plan_guid,
                ac_core_parking_min_percent: 10,
                ac_performance_min_percent: 20,
                ac_performance_max_percent: 90,
                ac_boost_mode: 2,
                dc_core_parking_min_percent: 5,
                dc_performance_min_percent: 15,
                dc_performance_max_percent: 70,
                dc_boost_mode: 3,
            } if plan_guid == "plan-guid"
        ));
    }

    #[test]
    fn input_hook_is_needed_for_activity_input_or_app_suspension() {
        let mut settings = Settings::default();

        assert!(!input_hook_required(&settings));

        settings.power_plans.performance_guid = Some("active-guid".to_owned());
        assert!(input_hook_required(&settings));

        settings.activity_mode.enabled = false;
        assert!(!input_hook_required(&settings));

        settings.activity_mode.enabled = true;
        settings.general.enabled = false;
        assert!(!input_hook_required(&settings));

        settings.general.enabled = true;
        settings.activity_mode.switch_to_performance_on_resume = false;
        assert!(!input_hook_required(&settings));

        settings.app_suspension.enabled = true;
        assert!(input_hook_required(&settings));

        settings.general.enabled = false;
        assert!(!input_hook_required(&settings));
    }

    #[test]
    fn input_hook_config_tracks_enabled_input_devices() {
        let mut settings = Settings::default();

        settings.activity_mode.input_detection.keyboard = true;
        settings.activity_mode.input_detection.mouse = false;
        assert_eq!(
            input_hook_config(&settings),
            InputHookConfig {
                keyboard: true,
                mouse: false,
            }
        );

        settings.activity_mode.input_detection.keyboard = false;
        settings.activity_mode.input_detection.mouse = true;
        assert_eq!(
            input_hook_config(&settings),
            InputHookConfig {
                keyboard: false,
                mouse: true,
            }
        );

        settings.app_suspension.enabled = true;
        assert_eq!(
            input_hook_config(&settings),
            InputHookConfig {
                keyboard: true,
                mouse: true,
            }
        );
    }

    #[test]
    fn split_watchdog_args_supports_quoted_and_spaced_values() {
        assert_eq!(
            split_watchdog_args(
                r#"--timeout 5 "C:\Program Files\Test\app.exe" "--label=hello world""#
            ),
            vec![
                "--timeout",
                "5",
                r#"C:\Program Files\Test\app.exe"#,
                "--label=hello world",
            ]
        );
    }

    #[test]
    fn split_watchdog_args_preserves_quoted_empty_argument() {
        assert_eq!(
            split_watchdog_args(r#""--flag" ""#),
            vec!["--flag".to_owned(), String::new(),]
        );
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
