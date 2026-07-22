use std::{
    cell::RefCell,
    cmp::Ordering as CmpOrdering,
    collections::{HashMap, HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, LazyLock, Mutex,
    },
    time::{Duration, Instant},
};

use rust_i18n::t;

use chrono::{Local, TimeZone};
use gpui::{
    canvas, deferred, div, img, percentage, prelude::*, px, relative, rgb, size, Animation,
    AnimationExt, AnyElement, App, Bounds, Context, DragMoveEvent, Empty, Entity, EntityId,
    Focusable, Hsla, Image, IntoElement, MouseButton, NavigationDirection, Pixels, Point, Render,
    ScrollHandle, SharedString, Subscription, Task, Timer, Window, WindowControlArea,
};
use gpui_component::{
    animation::cubic_bezier,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    chart::AreaChart,
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    h_flex,
    input::{Escape as InputEscape, Input, InputEvent, InputState},
    label::Label,
    menu::{ContextMenuExt, PopupMenuItem},
    scroll::{Scrollable, ScrollableElement, Scrollbar},
    slider::{SliderEvent, SliderState, SliderValue},
    theme::Colorize,
    tooltip::Tooltip,
    v_flex, v_virtual_list, ActiveTheme, Disableable, Icon, IconName, IconNamed, Sizable,
    VirtualListScrollHandle,
};

use crate::{
    action_log::{ActionLogAction, ActionLogEntry, ActionLogFeature, ActionLogResult},
    activity::{
        merge_activity_snapshot, ActivitySnapshot, ActivityState, ControllerActivityDetector,
        IdleDetector, InputHook, InputHookConfig,
    },
    app_suspension::{self, AppSuspensionSnapshot},
    automation::BackgroundAutomation,
    background_efficiency::{self, BackgroundEfficiencySnapshot},
    by_running_app::{self, ByRunningAppSnapshot},
    config::{
        self, AccentColorSource, AccentSettings, ActionLogMode, AnimationMode, AppLanguage,
        AppSuspensionRule, AppSuspensionSettings, AppThemeMode, BackgroundCpuRestrictionSettings,
        BackgroundEfficiencyAggressiveness, BackgroundEfficiencyRule, BackgroundEfficiencySettings,
        ByCpuLoadRule, ByForegroundRule, ByForegroundSettings, ByRunningAppRule,
        ByRunningAppSettings, ByTimeRule, CoreLimiterRule, CoreLimiterSettings, CoreSteeringMode,
        CoreSteeringRule, CoreSteeringSettings, CpuRestrictionControlStyle, CpuRestrictionMode,
        CpuRestrictionStrategy, CpuUsageComparison, DynamicPriorityBoostSettings,
        ForegroundBoostPriority, GpuPrioritySettings, IoPrioritySettings, MemoryPrioritySettings,
        MemoryTrimSettings, NetworkThresholdUnit, ProcessDynamicPriorityBoostSetting,
        ProcessExclusionRule, ProcessGpuPrioritySetting, ProcessIoPriority,
        ProcessIoPrioritySetting, ProcessMemoryPriority, ProcessMemoryPrioritySetting,
        ProcessPriority, ProcessPrioritySetting, ProcessPrioritySettings,
        ProcessThreadPrioritySetting, Settings, ThreadPrioritySettings, TimerResolutionRule,
        TimerResolutionSettings, UpdateChannel, WeekdaySetting, WorkloadEngineSettings,
    },
    core_limiter::{self, CoreLimiterSnapshot},
    core_steering::{self, CoreSteeringSnapshot, LogicalProcessorInfo, LogicalProcessorKind},
    cpu::{CpuUsageMonitor, CpuUsageSnapshot},
    dashboard_metrics::{
        IoUsageMonitor, IoUsageSnapshot, MemoryUsageMonitor, MemoryUsageSnapshot,
        NetworkUsageMonitor, NetworkUsageSnapshot,
    },
    dynamic_priority_boost::{self, DynamicPriorityBoostSnapshot},
    file_dialog::{choose_action_log_export_file, choose_settings_file, FileDialogMode},
    foreground::{
        capture_process_action_target, list_process_candidates, list_processes, same_process_name,
        ForegroundDetector, ProcessActionTarget, ProcessActionTargetError, ProcessCandidateInfo,
        ProcessInfo,
    },
    gpu_priority::{self, GpuPrioritySnapshot},
    io_priority::{self, IoPrioritySnapshot},
    memory_priority::{self, MemoryPrioritySnapshot},
    memory_trim::{self, MemoryTrimSnapshot},
    power::{
        EffectivePowerMode, EffectivePowerModeMonitor, PowerPlan, PowerPlanManager,
        PowerPlanPersonality, ProcessorBoostMode, ProcessorPowerAcDcValues, ProcessorPowerPreset,
        ProcessorPowerValues,
    },
    power_source, privilege,
    process_icon::load_process_icon,
    process_priority::{self, ProcessPrioritySnapshot},
    rules::{
        ByRunningAppDecision, DecisionEngine, DecisionInput, DecisionOutcome, DecisionState,
        MAX_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD, MIN_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD,
    },
    scheduler::{ByCpuLoadScheduler, ByTimeScheduler},
    self_power, startup,
    thread_priority::{self, ThreadPrioritySnapshot},
    timer_resolution::{self, TimerResolutionSnapshot},
    tray::{self, TrayIcon},
    ui::{self, Page},
    update_checker::{self, AvailableUpdate},
    win_registry::{
        read_registry_binary_root, read_registry_dword_root, write_registry_dword_create_root,
        write_registry_dword_root,
    },
    workload_engine::{self, WorkloadEngineSnapshot},
};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
};

mod about_page;
mod action_log_page;
mod adaptive_engine_page;
mod advanced_controls_pages;
mod advanced_power_plan_tuning_page;
mod app_suspension_page;
mod appearance;
mod background_efficiency_page;
mod chrome;
mod common_render;
mod control_components;
mod control_state;
mod cpu_control_pages;
mod dropdowns;
mod formatting;
mod indicators;
mod memory_trim_page;
mod motion;
mod navigation_components;
mod overview_page;
mod power_plan_control_pages;
mod presets;
mod priority_control_pages;
mod process_list_page;
mod process_policies;
mod settings_pages;
mod widgets;

use action_log_page::*;
use appearance::*;
use control_components::*;
use control_state::*;
use dropdowns::*;
use formatting::*;
use indicators::*;
use motion::*;
use navigation_components::*;
use presets::*;
use process_list_page::*;
use process_policies::*;
use widgets::*;

const ACTIVE_PLAN_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const APP_TICK_INTERVAL: Duration = Duration::from_secs(1);
const ADAPTIVE_ENGINE_APP_TICK_INTERVAL: Duration = Duration::from_secs(60);
const CPU_USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const DASHBOARD_IO_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const TIMER_RESOLUTION_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const CPU_USAGE_HISTORY_LEN: usize = 30;
const DASHBOARD_SUMMARY_CARD_HEIGHT: f32 = 196.0;
const DASHBOARD_LINE_CHART_HEIGHT: f32 = 112.0;
const DASHBOARD_LINE_CHART_TICK_MARGIN: usize = CPU_USAGE_HISTORY_LEN + 1;
const DASHBOARD_PERCENT_CHART_MAX: f64 = 100.0;
const DASHBOARD_SPLIT_ITEM_WIDTH: f32 = 140.0;
const DASHBOARD_SPLIT_VALUE_WIDTH: f32 = 90.0;
const CARD_ROW_HEIGHT: f32 = 58.0;
const CORE_TILE_GRID_COLUMNS: usize = 8;
const CORE_TILE_HEIGHT: f32 = 54.0;
const CORE_TILE_GRID_GAP: f32 = 4.0;
const EXPANDED_CHILD_MAX_ANIMATION_HEIGHT: f32 = 1800.0;
const EXPANDED_CHILD_SLIDE_PX: f32 = 8.0;
const MOTION_CONTROL_SECONDS: f64 = 0.18;
const MOTION_CONTROL_MIN_SECONDS: f64 = 0.08;
const MOTION_CONTROL_FRAME_INTERVAL: Duration = Duration::from_millis(16);
const MOTION_FAST_SECONDS: f64 = 0.15;
const MOTION_STANDARD_SECONDS: f64 = 0.22;
const MOTION_EXPAND_SECONDS: f64 = 0.24;
const MOTION_EXPAND_MIN_SECONDS: f64 = 0.1;
const UNSAVED_POPUP_VANISH_SECONDS: f64 = 0.18;
const PROCESS_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const TITLE_BAR_HEIGHT: f32 = 40.0;
const TITLE_BAR_CONTROL_WIDTH: f32 = 46.0;
const TITLE_BAR_CONTROL_ICON_SIZE: f32 = 12.0;
const TITLE_BAR_CONTROL_ICON_LINE_HEIGHT: f32 = 12.0;
const PAGE_HEADER_HEIGHT: f32 = 48.0;
const PAGE_CONTENT_VERTICAL_PADDING: f32 = 24.0;
const CONTENT_MAX_WIDTH: f32 = 1040.0;
const NAV_PANE_WIDTH: f32 = 276.0;
const BRAND_RADIUS_CONTROL: f32 = 5.0;
const BRAND_RADIUS_SURFACE: f32 = 7.0;
const BRAND_RADIUS_OVERLAY: f32 = 8.0;
const FONT_UI: &str = "Bahnschrift";
const FONT_BRAND: &str = "Bahnschrift";
const FONT_WINDOW_CONTROLS: &str = "Segoe Fluent Icons";
const PROCESS_PICKER_LAYER_PRIORITY: usize = 2;
const DROPDOWN_OPTION_ROW_HEIGHT: f32 = 40.0;
const DROPDOWN_CONTROL_HEIGHT: f32 = 32.0;
const DROPDOWN_SELECT_COMPACT_WIDTH: f32 = 96.0;
const DROPDOWN_SELECT_TABLE_WIDTH: f32 = 168.0;
const DROPDOWN_SELECT_STANDARD_WIDTH: f32 = 240.0;
const DROPDOWN_SELECT_WIDE_WIDTH: f32 = 280.0;
const NETWORK_UNIT_PICKER_WIDTH: f32 = 76.0;
const SUSPENSION_ACTIVE_COLUMN_WIDTH: f32 = 56.0;
const SUSPENSION_STATUS_COLUMN_WIDTH: f32 = 96.0;
const SUSPENSION_DETECT_COLUMN_WIDTH: f32 = 72.0;
const SUSPENSION_ACTION_COLUMN_WIDTH: f32 = 76.0;
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
const TIMER_RESOLUTION_INPUT_MIN_MS: f64 = 0.1;
const TIMER_RESOLUTION_INPUT_MAX_MS: f64 = 1000.0;
const WORKLOAD_ENGINE_THRESHOLD_MIN_PERCENT: u64 = 1;
const WORKLOAD_ENGINE_THRESHOLD_MAX_PERCENT: u64 = 100;
const WORKLOAD_ENGINE_SECONDS_MIN: u64 = 1;
const WORKLOAD_ENGINE_SECONDS_MAX: u64 = 3_600;
const WORKLOAD_ENGINE_TARGET_LIMIT_MIN: u64 = 1;
const WORKLOAD_ENGINE_TARGET_LIMIT_MAX: u64 = 64;
const WIN32_PRIORITY_SEPARATION_MIN: u64 = 0;
const WIN32_PRIORITY_SEPARATION_MAX: u64 = 63;
const WIN32_PRIORITY_SEPARATION_WINDOWS_DEFAULT: u32 = 0x26;
const WIN32_PRIORITY_CONTROL_SUB_KEY: &str = "SYSTEM\\CurrentControlSet\\Control\\PriorityControl";
const WIN32_PRIORITY_SEPARATION_VALUE: &str = "Win32PrioritySeparation";
const WINDERUST_REGISTRY_SUB_KEY: &str = "Software\\Winderust";
const WIN32_PRIORITY_SEPARATION_BACKUP_VALUE: &str = "Win32PrioritySeparationBackup";
const DWM_REGISTRY_SUB_KEY: &str = "Software\\Microsoft\\Windows\\DWM";
const DWM_ACCENT_COLOR_VALUE: &str = "AccentColor";
const EXPLORER_ACCENT_REGISTRY_SUB_KEY: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent";
const EXPLORER_ACCENT_PALETTE_VALUE: &str = "AccentPalette";
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

const COLOR_APP_BG: u32 = 0x101112;
const COLOR_TITLE_BAR: u32 = 0x0c0d0f;
const COLOR_SETTINGS_CARD: u32 = 0x191b1f;
const COLOR_SETTINGS_CARD_HOVER: u32 = 0x23262b;
const COLOR_SIDEBAR_SELECTED: u32 = 0x272b31;
const COLOR_SIDEBAR_HOVER: u32 = 0x202329;
const COLOR_PANEL_ACTIVE: u32 = 0x2d3239;
const COLOR_BORDER: u32 = 0x363b43;
const COLOR_TEXT: u32 = 0xf4f4f5;
const COLOR_MUTED: u32 = 0xc7ccd1;
const COLOR_DIM: u32 = 0x8b929a;
const COLOR_ACCENT: u32 = 0xa7e957;
const COLOR_SUCCESS: u32 = 0x9ee069;
const COLOR_SUCCESS_BG: u32 = 0x1f3418;
const COLOR_WARNING: u32 = 0xffc857;
const COLOR_WARNING_BG: u32 = 0x3d2e14;
const COLOR_LIGHT_APP_BG: u32 = 0xf4f4f5;
const COLOR_LIGHT_TITLE_BAR: u32 = 0xebedef;
const COLOR_LIGHT_SETTINGS_CARD: u32 = 0xffffff;
const COLOR_LIGHT_SETTINGS_CARD_HOVER: u32 = 0xf0f2f4;
const COLOR_LIGHT_SIDEBAR_SELECTED: u32 = 0xe1e5e9;
const COLOR_LIGHT_SIDEBAR_HOVER: u32 = 0xe9ecef;
const COLOR_LIGHT_PANEL_ACTIVE: u32 = 0xe3e7eb;
const COLOR_LIGHT_BORDER: u32 = 0xc7ccd2;
const COLOR_LIGHT_TEXT: u32 = 0x171a1d;
const COLOR_LIGHT_MUTED: u32 = 0x565d64;
const COLOR_LIGHT_DIM: u32 = 0x747c84;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActionLogFeatureFilter {
    All,
    Feature(ActionLogFeature),
}

impl ActionLogFeatureFilter {
    const ALL: [Self; 16] = [
        Self::All,
        Self::Feature(ActionLogFeature::AppSuspension),
        Self::Feature(ActionLogFeature::BackgroundCpuRestriction),
        Self::Feature(ActionLogFeature::CoreSteering),
        Self::Feature(ActionLogFeature::BackgroundEfficiency),
        Self::Feature(ActionLogFeature::CoreLimiter),
        Self::Feature(ActionLogFeature::ByRunningApp),
        Self::Feature(ActionLogFeature::WorkloadEngine),
        Self::Feature(ActionLogFeature::ProcessPriority),
        Self::Feature(ActionLogFeature::ThreadPriority),
        Self::Feature(ActionLogFeature::DynamicPriorityBoost),
        Self::Feature(ActionLogFeature::IoPriority),
        Self::Feature(ActionLogFeature::GpuPriority),
        Self::Feature(ActionLogFeature::MemoryPriority),
        Self::Feature(ActionLogFeature::MemoryTrim),
        Self::Feature(ActionLogFeature::TimerResolution),
    ];

    fn matches(self, feature: ActionLogFeature) -> bool {
        match self {
            Self::All => true,
            Self::Feature(filter_feature) => filter_feature == feature,
        }
    }
}

const ACTION_LOG_PAGE_SIZE: usize = 15;

const ACCENT_PALETTE: [u32; 48] = [
    0xa7e957, 0xc7f36d, 0x8fd14f, 0x65b741, 0x3f8f34, 0x2f6f34, 0xd8c75b, 0xffc857, 0xe0a93a,
    0xb9802f, 0x8d6128, 0xff8f5a, 0xe46845, 0xbb4c38, 0x8d382f, 0x6a2f2a, 0x4fc3a5, 0x2aa889,
    0x167c68, 0x0f5f54, 0x76d0b2, 0xa8d6a1, 0xd1e3a4, 0xf2e5a0, 0xe8d7b2, 0xc7b58f, 0xa8946d,
    0x786a50, 0x9bbf74, 0x7fa15d, 0x5d8048, 0x3f6038, 0xd9a441, 0xbf8033, 0xa45f31, 0x7d452e,
    0xd96f6a, 0xb85b58, 0x8d4645, 0x633839, 0x8aa49a, 0x6f877d, 0x53665f, 0x3d4d47, 0xc1b897,
    0xa8a07d, 0x837c61, 0x625d48,
];
const ACCENT_SWATCHES_PER_ROW: usize = 8;
const ACCENT_SWATCH_SIZE: f32 = 42.0;
const ACCENT_COLOR_PICKER_INNER_SIZE: f32 = ACCENT_SWATCH_SIZE;
const ACCENT_COLOR_PICKER_WRAPPER_SIZE: f32 = ACCENT_SWATCH_SIZE;

static UI_ACCENT_COLOR: AtomicU32 = AtomicU32::new(COLOR_ACCENT);
static UI_ACCENT_TINT_SURFACES: AtomicBool = AtomicBool::new(false);
static UI_DARK_MODE: AtomicBool = AtomicBool::new(true);
static UI_ANIMATIONS_ENABLED: AtomicBool = AtomicBool::new(true);

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessPolicySummary {
    power_plan_foreground: String,
    power_plan_running: String,
    background_efficiency: String,
    core_limiter: String,
    background_cpu_restriction: String,
    core_steering: String,
    process_priority: String,
    io_priority: String,
    gpu_priority: String,
    memory_priority: String,
    memory_trim: String,
    app_suspension: String,
    timer_resolution: String,
    custom_columns: HashSet<ProcessListColumn>,
}

impl ProcessPolicySummary {
    fn mark_custom(&mut self, column: ProcessListColumn) {
        self.custom_columns.insert(column);
    }

    fn uses_custom_rule(&self, column: ProcessListColumn) -> bool {
        self.custom_columns.contains(&column)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct CpuUsageHistorySample {
    percent: f32,
    frequency_mhz: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default)]
struct MemoryUsageHistorySample {
    usage_percent: f32,
    cache_percent: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct IoUsageHistorySample {
    read_bytes_per_second: f32,
    write_bytes_per_second: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct NetworkUsageHistorySample {
    download_bytes_per_second: f32,
    upload_bytes_per_second: f32,
}

struct DashboardDualLinePoint {
    tick: String,
    first_value: f64,
    second_value: f64,
    first_label: String,
    second_label: String,
}

#[derive(Clone, Copy, Debug)]
struct MemoryCapacityParts {
    value: f64,
    unit: &'static str,
}

pub struct WinderustApp {
    settings: Settings,
    saved_settings: Settings,
    last_background_settings: Arc<Settings>,
    page: Page,
    back_stack: Vec<Page>,
    forward_stack: Vec<Page>,
    plans: Vec<PowerPlan>,
    current_plan: Option<PowerPlan>,
    activity: ActivitySnapshot,
    cpu_usage: CpuUsageSnapshot,
    cpu_usage_history: VecDeque<CpuUsageHistorySample>,
    memory_usage: MemoryUsageSnapshot,
    memory_usage_history: VecDeque<MemoryUsageHistorySample>,
    io_usage: IoUsageSnapshot,
    io_usage_history: VecDeque<IoUsageHistorySample>,
    network_usage: NetworkUsageSnapshot,
    network_usage_history: VecDeque<NetworkUsageHistorySample>,
    background_efficiency_status: BackgroundEfficiencySnapshot,
    app_suspension_status: AppSuspensionSnapshot,
    core_limiter_status: CoreLimiterSnapshot,
    core_steering_status: CoreSteeringSnapshot,
    background_cpu_restriction_status: CoreSteeringSnapshot,
    by_running_app_status: ByRunningAppSnapshot,
    workload_engine_status: WorkloadEngineSnapshot,
    process_priority_status: ProcessPrioritySnapshot,
    thread_priority_status: ThreadPrioritySnapshot,
    dynamic_priority_boost_status: DynamicPriorityBoostSnapshot,
    io_priority_status: IoPrioritySnapshot,
    gpu_priority_status: GpuPrioritySnapshot,
    memory_priority_status: MemoryPrioritySnapshot,
    memory_trim_status: MemoryTrimSnapshot,
    timer_resolution_status: TimerResolutionSnapshot,
    action_log_entries: Arc<Vec<ActionLogEntry>>,
    last_appearance_change_generation: u64,
    last_background_status_generation: u64,
    last_pending_auto_exclusions_generation: u64,
    action_log_result_filter: ActionLogResultFilter,
    action_log_feature_filter: ActionLogFeatureFilter,
    action_log_page: usize,
    foreground_app: Option<String>,
    decision: DecisionOutcome,
    next_schedule: String,
    next_check: Instant,
    next_active_plan_refresh: Instant,
    next_cpu_usage_refresh: Instant,
    next_dashboard_io_refresh: Instant,
    next_timer_resolution_status_refresh: Instant,
    next_process_refresh: Instant,
    last_switch_attempt: Option<(String, Instant)>,
    power: PowerPlanManager,
    effective_power_mode_monitor: Option<EffectivePowerModeMonitor>,
    effective_power_mode: EffectivePowerMode,
    background_automation: BackgroundAutomation,
    cpu_monitor: CpuUsageMonitor,
    memory_monitor: MemoryUsageMonitor,
    io_monitor: IoUsageMonitor,
    network_monitor: NetworkUsageMonitor,
    idle_detector: IdleDetector,
    controller_activity_detector: ControllerActivityDetector,
    input_hook: Option<InputHook>,
    tray_hide_on_close: bool,
    foreground_detector: ForegroundDetector,
    by_time_scheduler: ByTimeScheduler,
    by_cpu_load_scheduler: ByCpuLoadScheduler,
    decision_engine: DecisionEngine,
    hwnd: Option<HWND>,
    tray_icon: Option<TrayIcon>,
    status_message: String,
    process_candidates: Vec<ProcessCandidate>,
    running_processes: Vec<ProcessInfo>,
    app_icon: Option<Arc<Image>>,
    process_icon_cache: HashMap<PathBuf, Option<Arc<Image>>>,
    active_power_plan_picker: Option<String>,
    processor_power_ac_core_parking_min: u64,
    processor_power_ac_performance_min: u64,
    processor_power_ac_performance_max: u64,
    processor_power_ac_boost_policy: u64,
    processor_power_ac_boost_mode: ProcessorBoostMode,
    processor_power_dc_core_parking_min: u64,
    processor_power_dc_performance_min: u64,
    processor_power_dc_performance_max: u64,
    processor_power_dc_boost_policy: u64,
    processor_power_dc_boost_mode: ProcessorBoostMode,
    processor_power_target_plan_guid: Option<String>,
    processor_power_loaded_plan_guid: Option<String>,
    processor_power_target_plan_personality: Option<PowerPlanPersonality>,
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
    expanded_process_list_groups: HashSet<String>,
    hidden_process_list_columns: HashSet<ProcessListColumn>,
    process_list_sort: ProcessListSort,
    selected_process_id: Option<u32>,
    breadcrumb_transition: Option<BreadcrumbTransition>,
    page_transition_generation: u64,
    available_update: Option<AvailableUpdate>,
    latest_version: Option<String>,
    update_check_in_progress: bool,
    update_check_message: Option<String>,
    admin_rights_prompt_visible: bool,
    unsaved_popup_was_visible: bool,
    unsaved_popup_vanish_started: Option<Instant>,
    pending_list_item_removals: HashMap<ListItemRemovalTarget, Instant>,
    dropdown_anchor_bounds: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    accent_color_picker: Entity<ColorPickerState>,
    _rule_title_input_subscriptions: Vec<Subscription>,
    _numeric_input_subscription: Option<Subscription>,
    _dashboard_search_subscription: Option<Subscription>,
    _processor_power_slider_subscriptions: Vec<Subscription>,
    _cpu_threshold_slider_subscriptions: Vec<Subscription>,
    _activity_slider_subscriptions: Vec<Subscription>,
    _accent_color_picker_subscription: Subscription,
    _window_activation_subscription: Subscription,
    inputs: UiInputs,
    _tick_task: Task<()>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BreadcrumbSegment {
    page: Page,
    label: String,
}

struct BreadcrumbTransition {
    previous: Vec<BreadcrumbSegment>,
    current: Vec<BreadcrumbSegment>,
    started: Instant,
    generation: u64,
}

#[derive(Default)]
struct CardHoverState {
    hovered: HashSet<String>,
    changes: HashMap<String, CardHoverChange>,
    generation: u64,
}

#[derive(Clone, Copy)]
struct CardHoverChange {
    hovered: bool,
    generation: u64,
    changed_at: Instant,
}

static CARD_HOVER_STATE: LazyLock<Mutex<CardHoverState>> =
    LazyLock::new(|| Mutex::new(CardHoverState::default()));

#[derive(Clone, Copy)]
struct ExpandableTransition {
    from_progress: f32,
    to_progress: f32,
    started: Instant,
    duration: Duration,
}

#[derive(Default)]
struct ExpandableMotionState {
    transitions: HashMap<String, ExpandableTransition>,
}

static EXPANDABLE_MOTION_STATE: LazyLock<Mutex<ExpandableMotionState>> =
    LazyLock::new(|| Mutex::new(ExpandableMotionState::default()));

#[derive(Clone, Copy)]
struct ControlTransition {
    from_progress: f32,
    to_progress: f32,
    started: Instant,
    duration: Duration,
    generation: u64,
}

#[derive(Default)]
struct ControlMotionState {
    values: HashMap<String, String>,
    transitions: HashMap<String, ControlTransition>,
    generation: u64,
}

static CONTROL_MOTION_STATE: LazyLock<Mutex<ControlMotionState>> =
    LazyLock::new(|| Mutex::new(ControlMotionState::default()));

#[derive(Clone, Copy)]
struct DropdownCloseTransition {
    started: Instant,
    generation: u64,
}

#[derive(Default)]
struct DropdownMotionState {
    open: HashMap<String, u64>,
    closing: HashMap<String, DropdownCloseTransition>,
    generation: u64,
}

static DROPDOWN_MOTION_STATE: LazyLock<Mutex<DropdownMotionState>> =
    LazyLock::new(|| Mutex::new(DropdownMotionState::default()));
static DISABLED_FEATURE_STATES: LazyLock<Mutex<HashMap<String, bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Copy)]
enum DropdownPopupPhase {
    Hidden,
    Open(u64),
    Closing(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ListItemRemovalTarget {
    kind: ListItemRemovalKind,
    index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum ListItemRemovalKind {
    ByForegroundRule,
    ByTimeRule,
    ByCpuLoadRule,
    BackgroundEfficiencyExclusion,
    AppSuspensionRule,
    BackgroundCpuExclusion,
    CoreLimiterRule,
    ByRunningAppRule,
    WorkloadEngineExclusion,
    ProcessPriorityExclusion,
    ThreadPriorityExclusion,
    DynamicPriorityBoostExclusion,
    IoPriorityExclusion,
    GpuPriorityExclusion,
    MemoryPriorityExclusion,
    TimerResolutionRule,
    MemoryTrimExclusion,
    CoreSteeringRule,
}

impl ListItemRemovalTarget {
    const fn new(kind: ListItemRemovalKind, index: usize) -> Self {
        Self { kind, index }
    }

    const fn index(self) -> usize {
        self.index
    }

    const fn with_index(self, index: usize) -> Self {
        Self { index, ..self }
    }

    fn same_list(self, other: Self) -> bool {
        self.kind == other.kind
    }
}

struct UiInputs {
    dashboard_search: Entity<InputState>,
    by_cpu_load_rule_names: Vec<Entity<InputState>>,
    cpu_rule_thresholds: Vec<Entity<SliderState>>,
    cpu_rule_upper_thresholds: Vec<Entity<SliderState>>,
    by_time_rule_names: Vec<Entity<InputState>>,
    schedule_start_times: Vec<Entity<InputState>>,
    schedule_end_times: Vec<Entity<InputState>>,
    foreground_rule_names: Vec<Entity<InputState>>,
    foreground_rule_processes: Vec<Entity<InputState>>,
    foreground_process: Entity<InputState>,
    background_efficiency_process: Entity<InputState>,
    background_cpu_exclusion: Entity<InputState>,
    memory_trim_exclusion: Entity<InputState>,
    app_suspension_process: Entity<InputState>,
    core_limiter_process: Entity<InputState>,
    performance_process: Entity<InputState>,
    core_steering_process: Entity<InputState>,
    workload_engine_process: Entity<InputState>,
    process_priority_process: Entity<InputState>,
    thread_priority_process: Entity<InputState>,
    dynamic_priority_boost_process: Entity<InputState>,
    io_priority_process: Entity<InputState>,
    gpu_priority_process: Entity<InputState>,
    memory_priority_process: Entity<InputState>,
    timer_resolution_process: Entity<InputState>,
    numeric_value: Entity<InputState>,
    activity_idle_timeout: Entity<SliderState>,
    activity_check_interval: Entity<SliderState>,
    processor_power_ac_core_parking_min: Entity<SliderState>,
    processor_power_ac_performance_min: Entity<SliderState>,
    processor_power_ac_performance_max: Entity<SliderState>,
    processor_power_ac_boost_policy: Entity<SliderState>,
    processor_power_dc_core_parking_min: Entity<SliderState>,
    processor_power_dc_performance_min: Entity<SliderState>,
    processor_power_dc_performance_max: Entity<SliderState>,
    processor_power_dc_boost_policy: Entity<SliderState>,
}

struct InitialProcessorPowerState {
    plans: Vec<PowerPlan>,
    current_plan: Option<PowerPlan>,
    values: ProcessorPowerAcDcValues,
    target_plan_guid: Option<String>,
    loaded_plan_guid: Option<String>,
    target_plan_personality: Option<PowerPlanPersonality>,
    status_message: String,
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
        cx: &mut Context<WinderustApp>,
        settings: &Settings,
        processor_power_values: ProcessorPowerAcDcValues,
    ) -> Self {
        let processor_power_values = processor_power_values.normalized();
        Self {
            dashboard_search: make_input(window, cx, "", &t!("home.search_placeholder")),
            by_cpu_load_rule_names: settings
                .by_cpu_load
                .rules
                .iter()
                .map(|rule| make_input(window, cx, &rule.name, &t!("common.rule_name")))
                .collect(),
            cpu_rule_thresholds: settings
                .by_cpu_load
                .rules
                .iter()
                .map(|rule| make_percent_slider(cx, rule.threshold_percent as u64))
                .collect(),
            cpu_rule_upper_thresholds: settings
                .by_cpu_load
                .rules
                .iter()
                .map(|rule| {
                    make_percent_slider(cx, rule.upper_threshold_percent.unwrap_or(100) as u64)
                })
                .collect(),
            by_time_rule_names: settings
                .by_time
                .rules
                .iter()
                .map(|rule| make_input(window, cx, &rule.name, &t!("common.rule_name")))
                .collect(),
            schedule_start_times: settings
                .by_time
                .rules
                .iter()
                .map(|rule| make_input(window, cx, &rule.start_time, "HH:MM"))
                .collect(),
            schedule_end_times: settings
                .by_time
                .rules
                .iter()
                .map(|rule| make_input(window, cx, &rule.end_time, "HH:MM"))
                .collect(),
            foreground_rule_names: settings
                .by_foreground
                .rules
                .iter()
                .map(|rule| make_input(window, cx, &rule.name, &t!("common.rule_name")))
                .collect(),
            foreground_rule_processes: settings
                .by_foreground
                .rules
                .iter()
                .map(|rule| make_input(window, cx, &rule.process_name, "process.exe"))
                .collect(),
            foreground_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            background_efficiency_process: make_input(
                window,
                cx,
                "",
                &t!("common.search_running_apps"),
            ),
            background_cpu_exclusion: make_input(window, cx, "", &t!("common.search_running_apps")),
            memory_trim_exclusion: make_input(window, cx, "", &t!("common.search_running_apps")),
            app_suspension_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            core_limiter_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            performance_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            core_steering_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            workload_engine_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            process_priority_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            thread_priority_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            dynamic_priority_boost_process: make_input(
                window,
                cx,
                "",
                &t!("common.search_running_apps"),
            ),
            io_priority_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            gpu_priority_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            memory_priority_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            timer_resolution_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            numeric_value: make_input(window, cx, "", "Value"),
            activity_idle_timeout: make_range_slider(
                cx,
                settings.by_activity.idle_timeout_seconds,
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
            processor_power_ac_boost_policy: make_processor_power_slider(
                cx,
                processor_power_values.ac.boost_policy as u64,
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
            processor_power_dc_boost_policy: make_processor_power_slider(
                cx,
                processor_power_values.dc.boost_policy as u64,
            ),
        }
    }

    fn ensure_for_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<WinderustApp>,
        settings: &Settings,
    ) {
        sync_input_vec(
            &mut self.by_cpu_load_rule_names,
            settings.by_cpu_load.rules.len(),
            window,
            cx,
            |index| settings.by_cpu_load.rules[index].name.clone(),
            &t!("common.rule_name"),
        );
        sync_slider_vec(
            &mut self.cpu_rule_thresholds,
            settings.by_cpu_load.rules.len(),
            cx,
            |index| settings.by_cpu_load.rules[index].threshold_percent as u64,
        );
        sync_slider_vec(
            &mut self.cpu_rule_upper_thresholds,
            settings.by_cpu_load.rules.len(),
            cx,
            |index| {
                settings.by_cpu_load.rules[index]
                    .upper_threshold_percent
                    .unwrap_or(100) as u64
            },
        );
        sync_input_vec(
            &mut self.by_time_rule_names,
            settings.by_time.rules.len(),
            window,
            cx,
            |index| settings.by_time.rules[index].name.clone(),
            &t!("common.rule_name"),
        );
        sync_input_vec(
            &mut self.schedule_start_times,
            settings.by_time.rules.len(),
            window,
            cx,
            |index| settings.by_time.rules[index].start_time.clone(),
            "HH:MM",
        );
        sync_input_vec(
            &mut self.schedule_end_times,
            settings.by_time.rules.len(),
            window,
            cx,
            |index| settings.by_time.rules[index].end_time.clone(),
            "HH:MM",
        );
        sync_input_vec(
            &mut self.foreground_rule_names,
            settings.by_foreground.rules.len(),
            window,
            cx,
            |index| settings.by_foreground.rules[index].name.clone(),
            &t!("common.rule_name"),
        );
        sync_input_vec(
            &mut self.foreground_rule_processes,
            settings.by_foreground.rules.len(),
            window,
            cx,
            |index| settings.by_foreground.rules[index].process_name.clone(),
            "process.exe",
        );
    }

    fn refresh_localized_placeholders(&self, window: &mut Window, cx: &mut Context<WinderustApp>) {
        set_input_placeholder(
            &self.dashboard_search,
            t!("home.search_placeholder"),
            window,
            cx,
        );
        for input in [
            &self.foreground_process,
            &self.background_efficiency_process,
            &self.background_cpu_exclusion,
            &self.memory_trim_exclusion,
            &self.app_suspension_process,
            &self.core_limiter_process,
            &self.performance_process,
            &self.core_steering_process,
            &self.workload_engine_process,
            &self.process_priority_process,
            &self.thread_priority_process,
            &self.dynamic_priority_boost_process,
            &self.io_priority_process,
            &self.gpu_priority_process,
            &self.memory_priority_process,
            &self.timer_resolution_process,
        ] {
            set_input_placeholder(input, t!("common.search_running_apps"), window, cx);
        }
        for input in self
            .by_cpu_load_rule_names
            .iter()
            .chain(&self.by_time_rule_names)
            .chain(&self.foreground_rule_names)
        {
            set_input_placeholder(input, t!("common.rule_name"), window, cx);
        }
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
            let target_plan_personality = target_plan
                .as_ref()
                .and_then(|plan| power.read_plan_personality(&plan.guid).ok());

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
                target_plan_personality,
                status_message,
            }
        }
        Err(err) => InitialProcessorPowerState {
            plans: Vec::new(),
            current_plan: None,
            values: fallback_values,
            target_plan_guid: None,
            loaded_plan_guid: None,
            target_plan_personality: None,
            status_message: err,
        },
    }
}

impl WinderustApp {
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
        let power = PowerPlanManager;
        let _ = power.restore_stale_adaptive_plans();
        let background_automation = BackgroundAutomation::start(&settings);
        apply_language(settings.general.language);
        apply_appearance_settings(&settings.general, window, cx);
        let effective_power_mode_monitor = EffectivePowerModeMonitor::new().ok();
        let effective_power_mode = effective_power_mode_monitor
            .as_ref()
            .map(EffectivePowerModeMonitor::snapshot)
            .unwrap_or(EffectivePowerMode::Unknown);
        let initial_processor_power = load_initial_processor_power_state(&power);
        let inputs = UiInputs::new(window, cx, &settings, initial_processor_power.values);
        let (win32_priority_separation_value, win32_priority_separation_status) =
            read_win32_priority_separation_with_status();
        let win32_priority_separation_edit_value = win32_priority_separation_value
            .map(normalize_win32_priority_separation_value)
            .unwrap_or(WIN32_PRIORITY_SEPARATION_WINDOWS_DEFAULT);
        let win32_priority_separation_backup = read_win32_priority_separation_backup();
        let initial_timer_resolution_status =
            timer_resolution::query_snapshot(settings.timer_resolution.enabled);
        let app_icon = std::env::current_exe()
            .ok()
            .and_then(|path| load_process_icon(&path));
        let accent_color_picker = cx.new(|cx| {
            ColorPickerState::new(window, cx)
                .default_value(rgb(settings.general.accent.custom_color))
        });
        let accent_color_picker_subscription = cx.subscribe_in(
            &accent_color_picker,
            window,
            |app, _, event: &ColorPickerEvent, window, cx| {
                let ColorPickerEvent::Change(Some(color)) = event else {
                    return;
                };
                let Some(color) = hsla_to_rgb_u32(*color) else {
                    return;
                };
                app.settings.general.accent.source = AccentColorSource::Custom;
                app.settings.general.accent.custom_color = color;
                add_custom_accent_color(&mut app.settings.general.accent, color);
                app.set_setting_group_expanded(SettingGroupTarget::AccentColor, true);
                app.active_power_plan_picker = None;
                apply_appearance_settings(&app.settings.general, window, cx);
                cx.notify();
            },
        );

        let mut app = Self {
            saved_settings: settings.clone(),
            last_background_settings: Arc::new(settings.clone()),
            settings,
            page: Page::Home,
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
            network_usage: NetworkUsageSnapshot::default(),
            network_usage_history: VecDeque::with_capacity(CPU_USAGE_HISTORY_LEN),
            background_efficiency_status: BackgroundEfficiencySnapshot::default(),
            app_suspension_status: AppSuspensionSnapshot::default(),
            core_limiter_status: CoreLimiterSnapshot::default(),
            core_steering_status: CoreSteeringSnapshot::default(),
            background_cpu_restriction_status: CoreSteeringSnapshot::default(),
            by_running_app_status: ByRunningAppSnapshot::default(),
            workload_engine_status: WorkloadEngineSnapshot::default(),
            process_priority_status: ProcessPrioritySnapshot::default(),
            thread_priority_status: ThreadPrioritySnapshot::default(),
            dynamic_priority_boost_status: DynamicPriorityBoostSnapshot::default(),
            io_priority_status: IoPrioritySnapshot::default(),
            gpu_priority_status: GpuPrioritySnapshot::default(),
            memory_priority_status: MemoryPrioritySnapshot::default(),
            memory_trim_status: MemoryTrimSnapshot::default(),
            timer_resolution_status: initial_timer_resolution_status,
            action_log_entries: Arc::new(Vec::new()),
            last_appearance_change_generation: 0,
            last_background_status_generation: 0,
            last_pending_auto_exclusions_generation: 0,
            action_log_result_filter: ActionLogResultFilter::All,
            action_log_feature_filter: ActionLogFeatureFilter::All,
            action_log_page: 0,
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
            next_dashboard_io_refresh: Instant::now(),
            next_timer_resolution_status_refresh: Instant::now(),
            next_process_refresh: Instant::now(),
            last_switch_attempt: None,
            power,
            effective_power_mode_monitor,
            effective_power_mode,
            background_automation,
            cpu_monitor: CpuUsageMonitor::default(),
            memory_monitor: MemoryUsageMonitor,
            io_monitor: IoUsageMonitor::default(),
            network_monitor: NetworkUsageMonitor::default(),
            idle_detector: IdleDetector,
            controller_activity_detector: ControllerActivityDetector::default(),
            input_hook: None,
            tray_hide_on_close: false,
            foreground_detector: ForegroundDetector,
            by_time_scheduler: ByTimeScheduler,
            by_cpu_load_scheduler: ByCpuLoadScheduler::default(),
            decision_engine: DecisionEngine,
            hwnd,
            tray_icon: None,
            status_message: initial_processor_power.status_message,
            process_candidates: Vec::new(),
            running_processes: Vec::new(),
            app_icon,
            process_icon_cache: HashMap::new(),
            active_power_plan_picker: None,
            processor_power_ac_core_parking_min: initial_processor_power.values.ac.core_parking_min
                as u64,
            processor_power_ac_performance_min: initial_processor_power.values.ac.performance_min
                as u64,
            processor_power_ac_performance_max: initial_processor_power.values.ac.performance_max
                as u64,
            processor_power_ac_boost_policy: initial_processor_power.values.ac.boost_policy as u64,
            processor_power_ac_boost_mode: initial_processor_power.values.ac.boost_mode,
            processor_power_dc_core_parking_min: initial_processor_power.values.dc.core_parking_min
                as u64,
            processor_power_dc_performance_min: initial_processor_power.values.dc.performance_min
                as u64,
            processor_power_dc_performance_max: initial_processor_power.values.dc.performance_max
                as u64,
            processor_power_dc_boost_policy: initial_processor_power.values.dc.boost_policy as u64,
            processor_power_dc_boost_mode: initial_processor_power.values.dc.boost_mode,
            processor_power_target_plan_guid: initial_processor_power.target_plan_guid,
            processor_power_loaded_plan_guid: initial_processor_power.loaded_plan_guid,
            processor_power_target_plan_personality: initial_processor_power
                .target_plan_personality,
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
            expanded_process_list_groups: HashSet::new(),
            hidden_process_list_columns: HashSet::new(),
            process_list_sort: ProcessListSort::default(),
            selected_process_id: None,
            breadcrumb_transition: None,
            page_transition_generation: 0,
            available_update: None,
            latest_version: None,
            update_check_in_progress: false,
            update_check_message: None,
            admin_rights_prompt_visible: !privilege::is_running_as_admin(),
            unsaved_popup_was_visible: false,
            unsaved_popup_vanish_started: None,
            pending_list_item_removals: HashMap::new(),
            dropdown_anchor_bounds: Rc::new(RefCell::new(HashMap::new())),
            accent_color_picker,
            _rule_title_input_subscriptions: Vec::new(),
            _numeric_input_subscription: None,
            _dashboard_search_subscription: None,
            _processor_power_slider_subscriptions: Vec::new(),
            _cpu_threshold_slider_subscriptions: Vec::new(),
            _activity_slider_subscriptions: Vec::new(),
            _accent_color_picker_subscription: accent_color_picker_subscription,
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
        let startup_settings = app.saved_settings.clone();
        app.sync_adaptive_engine(&startup_settings);
        app.run_check(false, Instant::now());
        app.sync_processor_power_slider_states(window, cx);
        app.sync_input_hook();
        if app.saved_settings.general.check_for_updates {
            app.check_for_updates(false, cx);
        }
        app.schedule_tick(window, cx);
        app
    }

    fn check_for_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.update_check_in_progress {
            return;
        }
        self.update_check_in_progress = true;
        self.update_check_message = None;
        if manual {
            cx.notify();
        }
        let channel = self.settings.general.update_channel;
        let check = cx
            .background_executor()
            .spawn(async move { update_checker::check(channel) });
        cx.spawn(async move |this, cx| {
            let result = check.await;
            let _ = this.update(cx, |app, cx| {
                app.update_check_in_progress = false;
                match result {
                    Ok(check) => {
                        app.latest_version = Some(check.latest_version);
                        app.available_update = check.available_update;
                    }
                    Err(()) if manual => {
                        app.update_check_message =
                            Some(t!("about.update_check_failed").to_string());
                    }
                    Err(()) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn navigate_to(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page == page {
            return;
        }

        clear_page_hovered();
        Self::push_navigation_page(&mut self.back_stack, self.page);
        self.begin_breadcrumb_transition(self.page, page);
        self.page = page;
        self.forward_stack.clear();
        cx.notify();
    }

    fn navigate_back(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.back_stack.pop() else {
            return;
        };

        clear_page_hovered();
        Self::push_navigation_page(&mut self.forward_stack, self.page);
        self.begin_breadcrumb_transition(self.page, page);
        self.page = page;
        cx.notify();
    }

    fn navigate_forward(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.forward_stack.pop() else {
            return;
        };

        clear_page_hovered();
        Self::push_navigation_page(&mut self.back_stack, self.page);
        self.begin_breadcrumb_transition(self.page, page);
        self.page = page;
        cx.notify();
    }

    fn begin_breadcrumb_transition(&mut self, previous: Page, current: Page) {
        if previous == current || !ui_animations_enabled() {
            self.breadcrumb_transition = None;
            return;
        }

        let previous = breadcrumb_trail(previous);
        let current = breadcrumb_trail(current);
        if previous == current {
            self.breadcrumb_transition = None;
            return;
        }

        self.page_transition_generation = self.page_transition_generation.wrapping_add(1);
        self.breadcrumb_transition = Some(BreadcrumbTransition {
            previous,
            current,
            started: Instant::now(),
            generation: self.page_transition_generation,
        });
    }

    fn clear_finished_breadcrumb_transition(&mut self) {
        if !ui_animations_enabled()
            || self
                .breadcrumb_transition
                .as_ref()
                .is_some_and(|transition| {
                    transition.started.elapsed() >= Duration::from_secs_f64(MOTION_FAST_SECONDS)
                })
        {
            self.breadcrumb_transition = None;
        }
    }

    fn active_breadcrumb_transition(&self, page: Page) -> Option<&BreadcrumbTransition> {
        self.breadcrumb_transition
            .as_ref()
            .filter(|transition| transition.current == breadcrumb_trail(page))
    }

    fn page_header(&self, page: Page, cx: &mut Context<Self>) -> gpui::Div {
        page_header_with_help(
            page,
            self.page_header_help(page),
            self.active_breadcrumb_transition(page),
            cx,
        )
    }

    fn page_header_help(&self, page: Page) -> Option<SharedString> {
        match page {
            Page::ActionLog => Some(action_log_page_help()),
            _ => None,
        }
    }

    fn page_shell(&self, _page: Page, _cx: &mut Context<Self>) -> gpui::Div {
        page_body_shell()
    }

    fn animated_list_item(
        &self,
        target: ListItemRemovalTarget,
        id: impl Into<SharedString>,
        child: AnyElement,
    ) -> AnyElement {
        animated_list_item_child(
            id,
            child,
            self.pending_list_item_removals.contains_key(&target),
        )
    }

    fn request_list_item_removal(&mut self, target: ListItemRemovalTarget, cx: &mut Context<Self>) {
        if !ui_animations_enabled() {
            self.commit_list_item_removal(target);
            cx.notify();
            return;
        }

        if self.pending_list_item_removals.contains_key(&target) {
            cx.notify();
            return;
        }

        self.pending_list_item_removals
            .insert(target, Instant::now());

        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_secs_f64(MOTION_FAST_SECONDS)).await;
            let _ = this.update(cx, |app, cx| {
                app.finish_due_list_item_removals();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_due_list_item_removals(&mut self) {
        let now = Instant::now();
        let mut due = self
            .pending_list_item_removals
            .iter()
            .filter_map(|(target, started)| {
                (now.duration_since(*started) >= Duration::from_secs_f64(MOTION_FAST_SECONDS))
                    .then_some(*target)
            })
            .collect::<Vec<_>>();

        due.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| b.index().cmp(&a.index())));

        for target in due {
            if self.pending_list_item_removals.remove(&target).is_some() {
                self.commit_list_item_removal(target);
                self.shift_pending_list_item_removals_after(target);
            }
        }
    }

    fn shift_pending_list_item_removals_after(&mut self, removed: ListItemRemovalTarget) {
        let mut shifted = HashMap::new();
        for (target, started) in self.pending_list_item_removals.drain() {
            let target = if target.same_list(removed) && target.index() > removed.index() {
                target.with_index(target.index() - 1)
            } else {
                target
            };
            shifted.insert(target, started);
        }
        self.pending_list_item_removals = shifted;
    }

    fn commit_list_item_removal(&mut self, target: ListItemRemovalTarget) {
        let index = target.index();

        match target.kind {
            ListItemRemovalKind::ByForegroundRule => {
                if index < self.settings.by_foreground.rules.len() {
                    self.settings.by_foreground.rules.remove(index);
                }
                self.editing_rule_title = None;
                self.expanded_rule_cards.clear();
            }
            ListItemRemovalKind::ByTimeRule => {
                if index < self.settings.by_time.rules.len() {
                    self.settings.by_time.rules.remove(index);
                }
                self.editing_rule_title = None;
                self.expanded_rule_cards.clear();
            }
            ListItemRemovalKind::ByCpuLoadRule => {
                if index < self.settings.by_cpu_load.rules.len() {
                    self.settings.by_cpu_load.rules.remove(index);
                }
                self.editing_rule_title = None;
                self.expanded_rule_cards.clear();
            }
            ListItemRemovalKind::BackgroundEfficiencyExclusion => {
                if index < self.settings.background_efficiency.custom_rules.len() {
                    self.settings
                        .background_efficiency
                        .custom_rules
                        .remove(index);
                }
            }
            ListItemRemovalKind::AppSuspensionRule => {
                if let Some(rule) = self.settings.app_suspension.suspendable_apps.get(index) {
                    self.expanded_rule_cards
                        .remove(&RuleCardTarget::AppSuspension(rule.process_name.clone()));
                }
                if index < self.settings.app_suspension.suspendable_apps.len() {
                    self.settings.app_suspension.suspendable_apps.remove(index);
                }
            }
            ListItemRemovalKind::BackgroundCpuExclusion => {
                if index < self.settings.background_cpu_restriction.exclusions.len() {
                    self.settings
                        .background_cpu_restriction
                        .exclusions
                        .remove(index);
                }
            }
            ListItemRemovalKind::CoreLimiterRule => {
                if let Some(rule) = self.settings.core_limiter.rules.get(index) {
                    self.expanded_rule_cards
                        .remove(&RuleCardTarget::CoreLimiter(rule.process_name.clone()));
                }
                if index < self.settings.core_limiter.rules.len() {
                    self.settings.core_limiter.rules.remove(index);
                }
            }
            ListItemRemovalKind::ByRunningAppRule => {
                if index < self.settings.by_running_app.rules.len() {
                    self.settings.by_running_app.rules.remove(index);
                }
                self.editing_rule_title = None;
                self.expanded_rule_cards.clear();
            }
            ListItemRemovalKind::WorkloadEngineExclusion => {
                if index
                    < self
                        .settings
                        .workload_engine
                        .workload_engine_exclusions
                        .len()
                {
                    self.settings
                        .workload_engine
                        .workload_engine_exclusions
                        .remove(index);
                }
            }
            ListItemRemovalKind::ProcessPriorityExclusion => {
                if index < self.settings.process_priority.exclusions.len() {
                    self.settings.process_priority.exclusions.remove(index);
                }
            }
            ListItemRemovalKind::ThreadPriorityExclusion => {
                if index < self.settings.thread_priority.exclusions.len() {
                    self.settings.thread_priority.exclusions.remove(index);
                }
            }
            ListItemRemovalKind::DynamicPriorityBoostExclusion => {
                if index < self.settings.dynamic_priority_boost.exclusions.len() {
                    self.settings
                        .dynamic_priority_boost
                        .exclusions
                        .remove(index);
                }
            }
            ListItemRemovalKind::IoPriorityExclusion => {
                if index < self.settings.io_priority.exclusions.len() {
                    self.settings.io_priority.exclusions.remove(index);
                }
            }
            ListItemRemovalKind::GpuPriorityExclusion => {
                if index < self.settings.gpu_priority.exclusions.len() {
                    self.settings.gpu_priority.exclusions.remove(index);
                }
            }
            ListItemRemovalKind::MemoryPriorityExclusion => {
                if index < self.settings.memory_priority.exclusions.len() {
                    self.settings.memory_priority.exclusions.remove(index);
                }
            }
            ListItemRemovalKind::TimerResolutionRule => {
                if index < self.settings.timer_resolution.rules.len() {
                    self.settings.timer_resolution.rules.remove(index);
                }
            }
            ListItemRemovalKind::MemoryTrimExclusion => {
                if index < self.settings.memory_trim.exclusions.len() {
                    self.settings.memory_trim.exclusions.remove(index);
                }
            }
            ListItemRemovalKind::CoreSteeringRule => {
                if let Some(rule) = self.settings.core_steering.rules.get(index) {
                    self.expanded_rule_cards
                        .remove(&RuleCardTarget::CoreSteering(rule.process_name.clone()));
                }
                if index < self.settings.core_steering.rules.len() {
                    self.settings.core_steering.rules.remove(index);
                }
            }
        }
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
        let tick_interval = app_tick_interval(&self.saved_settings, self.start_minimized_applied);
        self._tick_task = cx.spawn_in(window, async move |this, cx| {
            Timer::after(tick_interval).await;
            let _ = cx.update(move |window, app_cx| {
                if let Some(this) = this.upgrade() {
                    this.update(app_cx, |app, cx| match app.tick(window, cx) {
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

    fn refresh_effective_power_mode(&mut self) -> bool {
        let Some(monitor) = &self.effective_power_mode_monitor else {
            return false;
        };
        let mode = monitor.snapshot();
        if self.effective_power_mode == mode {
            return false;
        }

        self.effective_power_mode = mode;
        true
    }

    fn sync_adaptive_engine(&self, settings: &Settings) {
        if settings.adaptive_engine.enabled {
            let _ = self_power::enable_adaptive_engine();
        } else {
            let _ = self_power::disable_adaptive_engine();
        }
    }

    fn refresh_processor_power_target_plan_personality(&mut self) -> bool {
        let personality = self
            .processor_power_target_plan()
            .and_then(|plan| self.power.read_plan_personality(&plan.guid).ok());
        if self.processor_power_target_plan_personality == personality {
            return false;
        }

        self.processor_power_target_plan_personality = personality;
        true
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
        self.refresh_processor_power_target_plan_personality();
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
                self.processor_power_ac_boost_policy as u32,
                self.processor_power_ac_boost_mode,
            ),
            ProcessorPowerValues::new_with_boost_mode(
                self.processor_power_dc_core_parking_min as u32,
                self.processor_power_dc_performance_min as u32,
                self.processor_power_dc_performance_max as u32,
                self.processor_power_dc_boost_policy as u32,
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
        self.processor_power_ac_boost_policy = values.ac.boost_policy as u64;
        self.processor_power_ac_boost_mode = values.ac.boost_mode;
        self.processor_power_dc_core_parking_min = values.dc.core_parking_min as u64;
        self.processor_power_dc_performance_min = values.dc.performance_min as u64;
        self.processor_power_dc_performance_max = values.dc.performance_max as u64;
        self.processor_power_dc_boost_policy = values.dc.boost_policy as u64;
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
            ProcessorPowerSlider::AcBoostPolicy => {
                self.processor_power_ac_boost_policy = value;
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
            ProcessorPowerSlider::DcBoostPolicy => {
                self.processor_power_dc_boost_policy = value;
            }
        }
    }

    fn adaptive_engine_processor_policy_percent(
        &self,
        field: AdaptiveEngineProcessorPolicyField,
    ) -> u32 {
        let values = self
            .settings
            .adaptive_engine
            .processor_policy_values
            .normalized();
        match field {
            AdaptiveEngineProcessorPolicyField::CoreParkingMin => values.core_parking_min,
            AdaptiveEngineProcessorPolicyField::PerformanceMin => values.performance_min,
            AdaptiveEngineProcessorPolicyField::PerformanceMax => values.performance_max,
            AdaptiveEngineProcessorPolicyField::BoostPolicy => values.boost_policy,
        }
    }

    fn set_adaptive_engine_processor_policy_percent(
        &mut self,
        field: AdaptiveEngineProcessorPolicyField,
        value: u64,
    ) {
        let mut values = self
            .settings
            .adaptive_engine
            .processor_policy_values
            .normalized();
        let value = value.min(100) as u32;
        match field {
            AdaptiveEngineProcessorPolicyField::CoreParkingMin => values.core_parking_min = value,
            AdaptiveEngineProcessorPolicyField::PerformanceMin => values.performance_min = value,
            AdaptiveEngineProcessorPolicyField::PerformanceMax => values.performance_max = value,
            AdaptiveEngineProcessorPolicyField::BoostPolicy => values.boost_policy = value,
        }
        self.settings.adaptive_engine.processor_policy_values = values.normalized();
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
                ProcessorPowerSlider::AcBoostPolicy,
                self.processor_power_ac_boost_policy,
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
            (
                ProcessorPowerSlider::DcBoostPolicy,
                self.processor_power_dc_boost_policy,
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
                if let Some(rule) = self.settings.by_cpu_load.rules.get_mut(index) {
                    rule.threshold_percent = value;
                }
            }
            CpuThresholdSlider::Upper(index) => {
                if let Some(rule) = self.settings.by_cpu_load.rules.get_mut(index) {
                    rule.upper_threshold_percent = Some(value);
                }
            }
        }
    }

    fn sync_cpu_threshold_slider_states(&self, window: &mut Window, cx: &mut Context<Self>) {
        for (index, rule) in self.settings.by_cpu_load.rules.iter().enumerate() {
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
                self.settings.by_activity.idle_timeout_seconds = value.clamp(
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
                self.settings.by_activity.idle_timeout_seconds.clamp(
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

        match self
            .power
            .apply_processor_power_values(&plan.guid, values.normalized())
        {
            Ok(()) => {
                self.processor_power_loaded_plan_guid = Some(plan.guid.clone());
                self.processor_power_dirty = false;
                self.status_message =
                    t!("processor_power.applied_custom", plan = plan.display_name()).to_string();
                self.refresh_active_plan();
            }
            Err(err) => self.status_message = err,
        }
    }

    fn run_check(&mut self, sample_dashboard: bool, now: Instant) {
        if now >= self.next_active_plan_refresh {
            self.refresh_active_plan();
        }

        let decision_settings = self.cached_runtime_settings();
        let decision_settings = decision_settings.as_ref();
        self.activity = self.activity_snapshot(decision_settings, now);
        if sample_dashboard && self.page == Page::Home {
            self.refresh_dashboard_resource_samples();
        } else if decision_settings.by_cpu_load.enabled {
            self.refresh_cpu_usage_sample(now);
        }
        self.foreground_app = foreground_lookup_required(decision_settings)
            .then(|| self.foreground_detector.process_name())
            .flatten();
        let by_time = self
            .by_time_scheduler
            .current_decision(&decision_settings.by_time);
        let by_cpu_load = self
            .by_cpu_load_scheduler
            .current_decision(&decision_settings.by_cpu_load, self.cpu_usage.percent);
        self.next_schedule = self
            .by_time_scheduler
            .next_switch_label(&decision_settings.by_time);

        self.decision = self.decision_engine.decide(
            decision_settings,
            DecisionInput {
                activity_state: self.activity.state,
                foreground_app: self.foreground_app.clone(),
                plugged_in: power_source::is_plugged_in(),
                by_running_app: by_running_app_decision(&self.by_running_app_status),
                by_time,
                by_cpu_load,
            },
        );

        if !(decision_settings.general.enabled
            && decision_settings.adaptive_engine.enabled
            && decision_settings.adaptive_engine.processor_policy_enabled)
        {
            self.apply_decision();
        }
    }

    fn run_check_changed(&mut self, now: Instant) -> bool {
        let activity_state = self.activity.state;
        let activity_idle_for = self.activity.idle_for;
        let cpu_usage = self.cpu_usage;
        let memory_usage = self.memory_usage;
        let io_usage = self.io_usage;
        let network_usage = self.network_usage;
        let decision_target_guid = self.decision.target_guid.take();
        let decision_state = self.decision.state;
        let decision_reason = std::mem::take(&mut self.decision.reason);
        let next_schedule = std::mem::take(&mut self.next_schedule);
        let plan_count = self.plans.len();
        let previous_active_plan_guid = active_plan_guid(&self.plans).map(str::to_owned);
        let current_plan_guid = self.current_plan.as_ref().map(|plan| plan.guid.clone());
        let processor_power_target_plan_personality = self.processor_power_target_plan_personality;
        let status_message = self.status_message.clone();

        self.run_check(false, now);

        let resource_samples_changed = self.cpu_usage != cpu_usage
            || self.memory_usage != memory_usage
            || self.io_usage != io_usage
            || self.network_usage != network_usage;
        let resource_samples_visible = self.page == Page::Home;

        self.activity.state != activity_state
            || self.activity.idle_for != activity_idle_for
            || (resource_samples_visible && resource_samples_changed)
            || self.decision.target_guid != decision_target_guid
            || self.decision.state != decision_state
            || self.decision.reason != decision_reason
            || self.next_schedule != next_schedule
            || self.plans.len() != plan_count
            || active_plan_guid(&self.plans) != previous_active_plan_guid.as_deref()
            || self.current_plan.as_ref().map(|plan| plan.guid.as_str())
                != current_plan_guid.as_deref()
            || self.processor_power_target_plan_personality
                != processor_power_target_plan_personality
            || self.status_message != status_message
    }

    fn activity_snapshot(&mut self, settings: &Settings, now: Instant) -> ActivitySnapshot {
        let idle_timeout = Duration::from_secs(settings.by_activity.idle_timeout_seconds);
        let snapshot = self.idle_detector.snapshot(idle_timeout);
        let controller_idle_for = if settings.by_activity.input_detection.controller {
            self.controller_activity_detector.poll(now);
            self.controller_activity_detector.idle_for(now)
        } else {
            self.controller_activity_detector.clear();
            None
        };

        merge_activity_snapshot(snapshot, controller_idle_for, idle_timeout)
    }

    fn refresh_cpu_usage_sample(&mut self, now: Instant) -> bool {
        if !refresh_due(
            now,
            &mut self.next_cpu_usage_refresh,
            CPU_USAGE_REFRESH_INTERVAL,
        ) {
            return false;
        }

        let previous_cpu_usage = self.cpu_usage;
        self.cpu_usage = self.cpu_monitor.sample_usage();
        self.cpu_usage != previous_cpu_usage
    }

    fn refresh_dashboard_resource_samples(&mut self) -> bool {
        let now = Instant::now();
        if !refresh_due(
            now,
            &mut self.next_cpu_usage_refresh,
            CPU_USAGE_REFRESH_INTERVAL,
        ) {
            return false;
        }

        let previous_cpu_usage = self.cpu_usage;
        let previous_memory_usage = self.memory_usage;
        let sample_io = refresh_due(
            now,
            &mut self.next_dashboard_io_refresh,
            DASHBOARD_IO_REFRESH_INTERVAL,
        );

        self.cpu_usage = self.cpu_monitor.sample();
        self.memory_usage = self.memory_monitor.sample();

        let mut changed =
            self.cpu_usage != previous_cpu_usage || self.memory_usage != previous_memory_usage;

        if let Some(percent) = self.cpu_usage.percent {
            if self.cpu_usage_history.len() == CPU_USAGE_HISTORY_LEN {
                self.cpu_usage_history.pop_front();
            }
            self.cpu_usage_history.push_back(CpuUsageHistorySample {
                percent: percent.clamp(0.0, 100.0),
                frequency_mhz: self.cpu_usage.frequency_mhz,
            });
            changed = true;
        }
        if let Some(percent) = self.memory_usage.percent {
            if self.memory_usage_history.len() == CPU_USAGE_HISTORY_LEN {
                self.memory_usage_history.pop_front();
            }
            self.memory_usage_history
                .push_back(MemoryUsageHistorySample {
                    usage_percent: percent.clamp(0.0, 100.0),
                    cache_percent: memory_cache_percent(self.memory_usage).unwrap_or(0.0),
                });
            changed = true;
        }
        if sample_io {
            let previous_io_usage = self.io_usage;
            let previous_network_usage = self.network_usage;
            self.io_usage = self.io_monitor.sample();
            self.network_usage = self.network_monitor.sample();
            changed |=
                self.io_usage != previous_io_usage || self.network_usage != previous_network_usage;

            if self.io_usage.bytes_per_second.is_some() {
                if self.io_usage_history.len() == CPU_USAGE_HISTORY_LEN {
                    self.io_usage_history.pop_front();
                }
                self.io_usage_history.push_back(IoUsageHistorySample {
                    read_bytes_per_second: self
                        .io_usage
                        .read_bytes_per_second
                        .unwrap_or(0.0)
                        .clamp(0.0, f32::MAX as f64)
                        as f32,
                    write_bytes_per_second: self
                        .io_usage
                        .write_bytes_per_second
                        .unwrap_or(0.0)
                        .clamp(0.0, f32::MAX as f64)
                        as f32,
                });
                changed = true;
            }
            if self.network_usage.bytes_per_second.is_some() {
                if self.network_usage_history.len() == CPU_USAGE_HISTORY_LEN {
                    self.network_usage_history.pop_front();
                }
                self.network_usage_history
                    .push_back(NetworkUsageHistorySample {
                        download_bytes_per_second: self
                            .network_usage
                            .download_bytes_per_second
                            .unwrap_or(0.0)
                            .clamp(0.0, f32::MAX as f64)
                            as f32,
                        upload_bytes_per_second: self
                            .network_usage
                            .upload_bytes_per_second
                            .unwrap_or(0.0)
                            .clamp(0.0, f32::MAX as f64)
                            as f32,
                    });
                changed = true;
            }
        }

        changed
    }

    fn install_input_hook(&mut self, config: InputHookConfig) {
        match InputHook::install(config, self.background_automation.input_event_callback()) {
            Ok(input_hook) => {
                self.input_hook = Some(input_hook);
            }
            Err(err) => {
                self.status_message = err;
            }
        }
    }

    fn sync_input_hook(&mut self) {
        if input_hook_required(&self.saved_settings) {
            let config = input_hook_config(&self.saved_settings);
            if self
                .input_hook
                .as_ref()
                .is_none_or(|input_hook| input_hook.config() != config)
            {
                self.input_hook = None;
                self.install_input_hook(config);
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

        match self.power.set_active(target_guid) {
            Ok(()) => {
                self.status_message =
                    t!("status.switched_power_plan", reason = self.decision.reason).to_string();
                self.refresh_power_plans();
            }
            Err(err) => self.status_message = err,
        }
    }

    fn save_settings(&mut self) -> bool {
        match config::storage::save(&self.settings) {
            Ok(()) => {
                self.saved_settings = self.settings.clone();
                self.sync_input_hook();
                self.sync_background_settings();
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
                true
            }
            Err(err) => {
                self.status_message = err;
                false
            }
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
                match fs::write(
                    &path,
                    action_log_entries_to_csv(self.action_log_entries.as_slice()),
                ) {
                    Ok(()) => {
                        self.status_message =
                            t!("status.exported_action_log", path = path.display()).to_string();
                    }
                    Err(err) => {
                        self.status_message = t!(
                            "status.action_log_export_failed",
                            path = path.display(),
                            error = err
                        )
                        .to_string();
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
                            self.sync_background_settings();
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

    fn retain_current_process_icons(
        cache: &mut HashMap<PathBuf, Option<Arc<Image>>>,
        candidates: &[ProcessCandidate],
    ) {
        if cache.is_empty() {
            return;
        }

        let current_paths = candidates
            .iter()
            .filter_map(|candidate| candidate.image_path.as_deref())
            .collect::<HashSet<_>>();
        let old_len = cache.len();
        cache.retain(|path, _| current_paths.contains(path.as_path()));
        if cache.len() != old_len {
            cache.shrink_to_fit();
        }
    }

    fn refresh_process_candidates(&mut self, report_status: bool) -> bool {
        self.next_process_refresh = Instant::now() + PROCESS_REFRESH_INTERVAL;
        match list_process_candidates() {
            Ok(processes) => {
                let processes = self.process_candidates_from_info(processes);
                let changed = self.process_candidates != processes;
                self.process_candidates = processes;
                Self::retain_current_process_icons(
                    &mut self.process_icon_cache,
                    &self.process_candidates,
                );
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

    fn refresh_running_processes(&mut self, report_status: bool) -> bool {
        self.next_process_refresh = Instant::now() + PROCESS_REFRESH_INTERVAL;
        match list_processes() {
            Ok(mut processes) => {
                processes.sort_by(|left, right| {
                    left.name
                        .cmp(&right.name)
                        .then_with(|| left.id.cmp(&right.id))
                });
                let changed = self.running_processes != processes;
                self.running_processes = processes;
                let expanded_group_count = self.expanded_process_list_groups.len();
                if expanded_group_count != 0 {
                    let active_group_keys = self
                        .running_processes
                        .iter()
                        .map(|process| process_list_group_key(&process.name))
                        .collect::<HashSet<_>>();
                    self.expanded_process_list_groups
                        .retain(|key| active_group_keys.contains(key));
                }
                let groups_changed =
                    self.expanded_process_list_groups.len() != expanded_group_count;
                if report_status {
                    let message = t!(
                        "status.loaded_running_processes",
                        count = self.running_processes.len()
                    )
                    .to_string();
                    let status_changed = self.status_message != message;
                    self.status_message = message;
                    changed || groups_changed || status_changed
                } else {
                    changed || groups_changed
                }
            }
            Err(err) => {
                let changed = self.status_message != err;
                self.status_message = err;
                changed
            }
        }
    }

    fn sync_tray_icon(&mut self) -> bool {
        let tray_required =
            self.settings.general.hide_to_tray || self.saved_settings.general.start_minimized;
        let tray_present = self.tray_icon.is_some();
        let mut changed = false;

        if tray_required {
            if self.tray_icon.is_none() {
                let Some(hwnd) = self.hwnd else {
                    self.set_tray_hide_on_close(false);
                    let message = t!("status.system_tray_unavailable").to_string();
                    if self.status_message != message {
                        self.status_message = message;
                        changed = true;
                    }
                    return changed;
                };

                match TrayIcon::install(hwnd) {
                    Ok(icon) => {
                        self.tray_icon = Some(icon);
                        changed = true;
                        let message = t!("status.system_tray_enabled").to_string();
                        if self.status_message != message {
                            self.status_message = message;
                            changed = true;
                        }
                    }
                    Err(err) => {
                        if self.status_message != err {
                            self.status_message = err;
                            changed = true;
                        }
                    }
                }
            }
            self.set_tray_hide_on_close(
                self.settings.general.hide_to_tray && self.tray_icon.is_some(),
            );
        } else if self.tray_icon.take().is_some() {
            self.set_tray_hide_on_close(false);
            changed = true;
            let message = t!("status.system_tray_disabled").to_string();
            if self.status_message != message {
                self.status_message = message;
                changed = true;
            }
        } else {
            self.set_tray_hide_on_close(false);
        }

        changed || tray_present != self.tray_icon.is_some()
    }

    fn set_tray_hide_on_close(&mut self, enabled: bool) {
        if self.tray_hide_on_close == enabled {
            return;
        }

        self.tray_hide_on_close = enabled;
        tray::set_hide_on_close(enabled);
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
        match self.tick(window, cx) {
            TickOutcome::Continue { changed } => {
                self.schedule_tick(window, cx);
                if changed {
                    cx.notify();
                }
            }
            TickOutcome::Stop => {}
        }
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) -> TickOutcome {
        if tray::take_quit_requested() {
            self.set_tray_hide_on_close(false);
            self.tray_icon = None;
            window.remove_window();
            return TickOutcome::Stop;
        }

        let mut changed = self.apply_start_minimized(window);
        changed |= self.apply_pending_auto_exclusions();
        if tray::is_hidden_to_tray() {
            self.sync_input_hook();
            self.sync_background_settings();
            return TickOutcome::Stop;
        }

        if let Some(background_status) = self
            .background_automation
            .status_snapshot_since(self.last_background_status_generation)
        {
            self.last_background_status_generation = background_status.generation;

            if self.background_efficiency_status != background_status.background_efficiency {
                self.background_efficiency_status = background_status.background_efficiency;
                changed = true;
            }

            if self.app_suspension_status != background_status.app_suspension {
                self.app_suspension_status = background_status.app_suspension;
                changed = true;
            }

            if self.core_limiter_status != background_status.core_limiter {
                self.core_limiter_status = background_status.core_limiter;
                changed = true;
            }

            if self.core_steering_status != background_status.core_steering {
                self.core_steering_status = background_status.core_steering;
                changed = true;
            }

            if self.background_cpu_restriction_status
                != background_status.background_cpu_restriction
            {
                self.background_cpu_restriction_status =
                    background_status.background_cpu_restriction;
                changed = true;
            }

            if self.by_running_app_status != background_status.by_running_app {
                self.by_running_app_status = background_status.by_running_app;
                changed = true;
            }

            if self.workload_engine_status != background_status.workload_engine {
                self.workload_engine_status = background_status.workload_engine;
                changed = true;
            }

            if self.process_priority_status != background_status.process_priority {
                self.process_priority_status = background_status.process_priority;
                changed = true;
            }

            if self.thread_priority_status != background_status.thread_priority {
                self.thread_priority_status = background_status.thread_priority;
                changed = true;
            }

            if self.dynamic_priority_boost_status != background_status.dynamic_priority_boost {
                self.dynamic_priority_boost_status = background_status.dynamic_priority_boost;
                changed = true;
            }

            if self.io_priority_status != background_status.io_priority {
                self.io_priority_status = background_status.io_priority;
                changed = true;
            }

            if self.gpu_priority_status != background_status.gpu_priority {
                self.gpu_priority_status = background_status.gpu_priority;
                changed = true;
            }

            if self.memory_priority_status != background_status.memory_priority {
                self.memory_priority_status = background_status.memory_priority;
                changed = true;
            }

            if self.memory_trim_status != background_status.memory_trim {
                self.memory_trim_status = background_status.memory_trim;
                changed = true;
            }

            if self.timer_resolution_status != background_status.timer_resolution {
                self.timer_resolution_status = background_status.timer_resolution;
                changed = true;
            }

            if !Arc::ptr_eq(
                &self.action_log_entries,
                &background_status.action_log_entries,
            ) {
                self.action_log_entries = background_status.action_log_entries;
                changed = true;
            }

            if self.last_appearance_change_generation
                != background_status.appearance_change_generation
            {
                self.last_appearance_change_generation =
                    background_status.appearance_change_generation;
                apply_appearance_settings(&self.settings.general, window, cx);
                changed = true;
            }
        }

        changed |= self.refresh_effective_power_mode();

        let now = Instant::now();

        if self.page == Page::TimerResolution
            && !self.settings.timer_resolution.enabled
            && refresh_due(
                now,
                &mut self.next_timer_resolution_status_refresh,
                TIMER_RESOLUTION_STATUS_REFRESH_INTERVAL,
            )
        {
            let timer_resolution_status =
                timer_resolution::query_snapshot(self.settings.timer_resolution.enabled);
            if self.timer_resolution_status != timer_resolution_status {
                self.timer_resolution_status = timer_resolution_status;
                changed = true;
            }
        }

        if now >= self.next_process_refresh {
            if self.page == Page::ProcessList {
                changed |= self.refresh_running_processes(false);
            } else if self.page_uses_process_candidates() {
                changed |= self.refresh_process_candidates(false);
            }
        }

        if self.page == Page::Home {
            changed |= self.refresh_dashboard_resource_samples();
        }

        let should_check_now = now >= self.next_check;

        if should_check_now {
            changed |= self.run_check_changed(now);
            self.next_check = now
                + Duration::from_millis(
                    self.settings
                        .general
                        .check_interval_ms
                        .max(ACTIVITY_CHECK_INTERVAL_MIN_MS),
                );
        }

        changed |= self.sync_tray_icon();

        if !should_check_now {
            self.sync_background_settings();
        }
        TickOutcome::Continue { changed }
    }

    fn apply_pending_auto_exclusions(&mut self) -> bool {
        let Some(pending) = self
            .background_automation
            .take_pending_auto_exclusions_since(&mut self.last_pending_auto_exclusions_generation)
        else {
            return false;
        };
        let mut changed = false;

        for process in pending.background_efficiency {
            if can_add_background_efficiency_process(&self.settings.background_efficiency, &process)
            {
                self.settings
                    .background_efficiency
                    .custom_rules
                    .push(new_background_efficiency_rule(&process));
                changed = true;
            }
        }

        for process in pending.app_suspension {
            if can_add_app_suspension_process(&self.settings.app_suspension, &process) {
                let mut rule = new_app_suspension_rule(&process);
                rule.enabled = false;
                self.settings.app_suspension.suspendable_apps.push(rule);
                changed = true;
            }
        }

        for process in pending.core_steering {
            if can_add_core_steering_process(&self.settings.core_steering, &process) {
                let mut rule = new_core_steering_rule(&process);
                rule.enabled = false;
                self.settings.core_steering.rules.push(rule);
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

        for process in pending.core_limiter {
            if can_add_core_limiter_process(&self.settings.core_limiter, &process) {
                let mut rule = new_core_limiter_rule(&process);
                rule.enabled = false;
                self.settings.core_limiter.rules.push(rule);
                changed = true;
            }
        }

        for process in pending.workload_engine {
            if can_add_workload_engine_exclusion(&self.settings.workload_engine, &process) {
                self.settings
                    .workload_engine
                    .workload_engine_exclusions
                    .push(new_process_exclusion_rule(&process));
                changed = true;
            }
        }

        for process in pending.io_priority {
            if can_add_io_priority_exclusion(&self.settings.io_priority, &process) {
                self.settings
                    .io_priority
                    .exclusions
                    .push(new_process_exclusion_rule(&process));
                changed = true;
            }
        }

        for process in pending.process_priority {
            if can_add_process_priority_exclusion(&self.settings.process_priority, &process) {
                self.settings
                    .process_priority
                    .exclusions
                    .push(new_process_exclusion_rule(&process));
                changed = true;
            }
        }

        for process in pending.thread_priority {
            if can_add_thread_priority_exclusion(&self.settings.thread_priority, &process) {
                self.settings
                    .thread_priority
                    .exclusions
                    .push(new_process_exclusion_rule(&process));
                changed = true;
            }
        }

        for process in pending.dynamic_priority_boost {
            if can_add_dynamic_priority_boost_exclusion(
                &self.settings.dynamic_priority_boost,
                &process,
            ) {
                self.settings
                    .dynamic_priority_boost
                    .exclusions
                    .push(new_process_exclusion_rule(&process));
                changed = true;
            }
        }

        for process in pending.gpu_priority {
            if can_add_gpu_priority_exclusion(&self.settings.gpu_priority, &process) {
                self.settings
                    .gpu_priority
                    .exclusions
                    .push(new_process_exclusion_rule(&process));
                changed = true;
            }
        }

        for process in pending.memory_priority {
            if can_add_memory_priority_exclusion(&self.settings.memory_priority, &process) {
                self.settings
                    .memory_priority
                    .exclusions
                    .push(new_process_exclusion_rule(&process));
                changed = true;
            }
        }

        for process in pending.memory_trim {
            if can_add_memory_trim_exclusion(&self.settings.memory_trim, &process) {
                self.settings
                    .memory_trim
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
            Page::ByForeground
                | Page::BackgroundEfficiency
                | Page::AppSuspension
                | Page::ProcessPriority
                | Page::DynamicPriorityBoost
                | Page::CoreLimiter
                | Page::BackgroundCpuRestriction
                | Page::IoPriority
                | Page::GpuPriority
                | Page::MemoryPriority
                | Page::TimerResolution
                | Page::ByRunningApp
                | Page::CoreSteering
        )
    }

    fn cancel_settings_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let had_unsaved_changes = self.settings != self.saved_settings;
        self.settings = self.saved_settings.clone();
        apply_language(self.settings.general.language);
        apply_appearance_settings(&self.settings.general, window, cx);
        self.status_message = t!("status.unsaved_canceled").to_string();
        self.editing_rule_title = None;
        self.expanded_rule_cards.clear();
        self.rebuild_inputs(window, cx);
        if had_unsaved_changes {
            self.start_unsaved_popup_vanish();
        }
    }

    fn start_unsaved_popup_vanish(&mut self) {
        self.unsaved_popup_was_visible = false;
        self.unsaved_popup_vanish_started = ui_animations_enabled().then_some(Instant::now());
    }

    fn unsaved_popup_vanish_progress(&mut self, unsaved: bool, window: &mut Window) -> Option<f32> {
        if unsaved {
            self.unsaved_popup_was_visible = true;
            self.unsaved_popup_vanish_started = None;
            return None;
        }

        if !ui_animations_enabled() {
            self.unsaved_popup_was_visible = false;
            self.unsaved_popup_vanish_started = None;
            return None;
        }

        if self.unsaved_popup_vanish_started.is_none() {
            if self.unsaved_popup_was_visible {
                self.start_unsaved_popup_vanish();
            } else {
                return None;
            }
        } else {
            self.unsaved_popup_was_visible = false;
        }

        let started = self.unsaved_popup_vanish_started?;
        let duration = Duration::from_secs_f64(UNSAVED_POPUP_VANISH_SECONDS);
        let elapsed = started.elapsed();
        if elapsed >= duration {
            self.unsaved_popup_was_visible = false;
            self.unsaved_popup_vanish_started = None;
            None
        } else {
            window.request_animation_frame();
            Some(expandable_motion_ease(
                (elapsed.as_secs_f32() / duration.as_secs_f32().max(f32::EPSILON)).clamp(0.0, 1.0),
                false,
            ))
        }
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
        self.inputs.by_time_rule_names.len() + self.inputs.by_cpu_load_rule_names.len()
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
                .by_time_rule_names
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, input)| (input, RuleTitleTarget::ByTime(index))),
        );
        inputs.extend(
            self.inputs
                .by_cpu_load_rule_names
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, input)| (input, RuleTitleTarget::ByCpuLoad(index))),
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
            ProcessorPowerSlider::AcBoostPolicy,
            ProcessorPowerSlider::DcCoreParkingMin,
            ProcessorPowerSlider::DcPerformanceMin,
            ProcessorPowerSlider::DcPerformanceMax,
            ProcessorPowerSlider::DcBoostPolicy,
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
            RuleTitleTarget::ByTime(index) => self.inputs.by_time_rule_names.get(index),
            RuleTitleTarget::ByCpuLoad(index) => self.inputs.by_cpu_load_rule_names.get(index),
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
            NumericField::BackgroundCpuRestrictionPercent => {
                if let Some(value) = parse_u64_input(&value, 1, 100) {
                    self.settings.background_cpu_restriction.percent = value as u8;
                }
            }
            NumericField::MemoryTrimCheckIntervalMinutes => {
                if let Some(value) = parse_u64_input(&value, 1, 1440) {
                    self.settings.memory_trim.check_interval_minutes = value;
                }
            }
            NumericField::MemoryTrimMemoryLoadThreshold => {
                if let Some(value) = parse_u64_input(&value, 1, 100) {
                    self.settings
                        .memory_trim
                        .system_memory_load_threshold_percent = value as u8;
                }
            }
            NumericField::MemoryTrimWorkingSetThreshold => {
                if let Some(value) = parse_u64_input(&value, 1, 1_048_576) {
                    self.settings.memory_trim.process_working_set_threshold_mb = value;
                }
            }
            NumericField::MemoryTrimCpuIdleThreshold => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.settings.memory_trim.process_cpu_idle_threshold_percent = value as u8;
                }
            }
            NumericField::MemoryTrimIdleSeconds => {
                if let Some(value) = parse_u64_input(&value, 1, 86_400) {
                    self.settings.memory_trim.process_idle_seconds = value;
                }
            }
            NumericField::MemoryTrimCooldownSeconds => {
                if let Some(value) = parse_u64_input(&value, 1, 86_400) {
                    self.settings.memory_trim.trim_cooldown_seconds = value;
                }
            }
            NumericField::MemoryTrimPurgeFreeRamThreshold => {
                if let Some(value) = parse_u64_input(&value, 0, 1_048_576) {
                    self.settings.memory_trim.purge_free_ram_threshold_mb = value;
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
            NumericField::WorkloadEngineThreshold => {
                if let Some(value) = parse_u64_input(
                    &value,
                    WORKLOAD_ENGINE_THRESHOLD_MIN_PERCENT,
                    WORKLOAD_ENGINE_THRESHOLD_MAX_PERCENT,
                ) {
                    self.settings
                        .workload_engine
                        .workload_engine_threshold_percent = value as u8;
                }
            }
            NumericField::WorkloadEngineRestoreThreshold => {
                if let Some(value) = parse_u64_input(
                    &value,
                    WORKLOAD_ENGINE_THRESHOLD_MIN_PERCENT,
                    WORKLOAD_ENGINE_THRESHOLD_MAX_PERCENT,
                ) {
                    self.settings
                        .workload_engine
                        .workload_engine_restore_threshold_percent = value as u8;
                }
            }
            NumericField::WorkloadEngineTotalThreshold => {
                if let Some(value) = parse_u64_input(
                    &value,
                    WORKLOAD_ENGINE_THRESHOLD_MIN_PERCENT,
                    WORKLOAD_ENGINE_THRESHOLD_MAX_PERCENT,
                ) {
                    self.settings
                        .workload_engine
                        .workload_engine_total_threshold_percent = value as u8;
                }
            }
            NumericField::WorkloadEngineCpuPercent => {
                if let Some(value) = parse_u64_input(
                    &value,
                    WORKLOAD_ENGINE_THRESHOLD_MIN_PERCENT,
                    WORKLOAD_ENGINE_THRESHOLD_MAX_PERCENT,
                ) {
                    self.settings.workload_engine.workload_engine_cpu_percent = value as u8;
                }
            }
            NumericField::WorkloadEngineSustain => {
                if let Some(value) = parse_u64_input(
                    &value,
                    WORKLOAD_ENGINE_SECONDS_MIN,
                    WORKLOAD_ENGINE_SECONDS_MAX,
                ) {
                    self.settings
                        .workload_engine
                        .workload_engine_sustain_seconds = value;
                }
            }
            NumericField::WorkloadEngineMinimumRestraint => {
                if let Some(value) = parse_u64_input(
                    &value,
                    WORKLOAD_ENGINE_SECONDS_MIN,
                    WORKLOAD_ENGINE_SECONDS_MAX,
                ) {
                    self.settings
                        .workload_engine
                        .workload_engine_minimum_restraint_seconds = value;
                }
            }
            NumericField::WorkloadEngineCooldown => {
                if let Some(value) = parse_u64_input(
                    &value,
                    WORKLOAD_ENGINE_SECONDS_MIN,
                    WORKLOAD_ENGINE_SECONDS_MAX,
                ) {
                    self.settings
                        .workload_engine
                        .workload_engine_cooldown_seconds = value;
                }
            }
            NumericField::WorkloadEngineMaxTargetedProcesses => {
                if let Some(value) = parse_u64_input(
                    &value,
                    WORKLOAD_ENGINE_TARGET_LIMIT_MIN,
                    WORKLOAD_ENGINE_TARGET_LIMIT_MAX,
                ) {
                    self.settings
                        .workload_engine
                        .workload_engine_max_targeted_processes = value as u8;
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
            NumericField::ProcessorAcBoostPolicy => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.set_processor_power_slider_value(
                        ProcessorPowerSlider::AcBoostPolicy,
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
            NumericField::ProcessorDcBoostPolicy => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.set_processor_power_slider_value(
                        ProcessorPowerSlider::DcBoostPolicy,
                        value,
                    );
                }
            }
            NumericField::AdaptiveEngineProcessorPolicy(field) => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.set_adaptive_engine_processor_policy_percent(field, value);
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
                    self.settings.by_cpu_load.rules.get_mut(index),
                    parse_u64_input(&value, 0, 86_400),
                ) {
                    rule.duration_seconds = value;
                }
            }
            NumericField::CoreLimiterThreshold(index) => {
                if let (Some(rule), Some(value)) = (
                    self.settings.core_limiter.rules.get_mut(index),
                    parse_u64_input(&value, 1, 100),
                ) {
                    rule.threshold_percent = value as u8;
                }
            }
            NumericField::CoreLimiterSustain(index) => {
                if let (Some(rule), Some(value)) = (
                    self.settings.core_limiter.rules.get_mut(index),
                    parse_u64_input(&value, 1, 86_400),
                ) {
                    rule.sustain_seconds = value;
                }
            }
            NumericField::CoreLimiterCooldown(index) => {
                if let (Some(rule), Some(value)) = (
                    self.settings.core_limiter.rules.get_mut(index),
                    parse_u64_input(&value, 1, 86_400),
                ) {
                    rule.cooldown_seconds = value;
                }
            }
            NumericField::CoreLimiterMaxProcessors(index) => {
                if let (Some(rule), Some(value)) = (
                    self.settings.core_limiter.rules.get_mut(index),
                    parse_u64_input(&value, 1, max_logical_processor_count() as u64),
                ) {
                    rule.max_logical_processors = value as u8;
                }
            }
            NumericField::TimerResolutionRule(index) => {
                let minimum_100ns = self
                    .timer_resolution_status
                    .minimum_100ns
                    .unwrap_or((TIMER_RESOLUTION_INPUT_MIN_MS * 10_000.0).round() as u32);
                let maximum_100ns = self
                    .timer_resolution_status
                    .maximum_100ns
                    .unwrap_or((TIMER_RESOLUTION_INPUT_MAX_MS * 10_000.0).round() as u32);
                if let (Some(rule), Some(value)) = (
                    self.settings.timer_resolution.rules.get_mut(index),
                    parse_timer_resolution_input_100ns(&value, minimum_100ns, maximum_100ns),
                ) {
                    rule.desired_100ns = value;
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
        let motion_id = rule_card_body_motion_id(&target);
        let expanded = if self.expanded_rule_cards.remove(&target) {
            false
        } else {
            self.expanded_rule_cards.insert(target);
            true
        };
        begin_expandable_motion(motion_id, expanded);
        cx.notify();
    }

    fn is_setting_group_collapsed(&self, target: SettingGroupTarget) -> bool {
        !self.expanded_setting_groups.contains(&target)
    }

    fn toggle_setting_group(&mut self, target: SettingGroupTarget, cx: &mut Context<Self>) {
        self.set_setting_group_expanded(target, self.is_setting_group_collapsed(target));
        cx.notify();
    }

    fn set_setting_group_expanded(&mut self, target: SettingGroupTarget, expanded: bool) {
        let changed = if expanded {
            self.expanded_setting_groups.insert(target)
        } else {
            self.expanded_setting_groups.remove(&target)
        };

        if changed {
            begin_expandable_motion(format!("setting-group-{target:?}"), expanded);
        }
    }

    fn is_process_list_group_collapsed(&self, process_name: &str) -> bool {
        !self
            .expanded_process_list_groups
            .contains(&process_list_group_key(process_name))
    }

    fn toggle_process_list_group(&mut self, process_name: String, cx: &mut Context<Self>) {
        let key = process_list_group_key(&process_name);
        let expanded = if self.expanded_process_list_groups.remove(&key) {
            false
        } else {
            self.expanded_process_list_groups.insert(key.clone());
            true
        };
        begin_expandable_motion(format!("process-list-group-{key}"), expanded);
        cx.notify();
    }

    fn set_process_list_column_visible(
        &mut self,
        column: ProcessListColumn,
        visible: bool,
        cx: &mut Context<Self>,
    ) {
        let changed = if visible {
            self.hidden_process_list_columns.remove(&column)
        } else {
            self.hidden_process_list_columns.insert(column)
        };

        let sort_changed =
            !visible && self.process_list_sort.column == ProcessListSortColumn::Column(column);
        if sort_changed {
            self.process_list_sort = ProcessListSort::default();
        }

        if changed || sort_changed {
            cx.notify();
        }
    }

    fn toggle_process_list_sort(&mut self, column: ProcessListSortColumn, cx: &mut Context<Self>) {
        self.process_list_sort = self.process_list_sort.toggled_for(column);
        cx.notify();
    }

    fn finish_process_list_edit(&mut self, cx: &mut Context<Self>) {
        self.active_power_plan_picker = None;
        cx.notify();
    }

    fn set_process_list_foreground_power_plan(
        &mut self,
        process_name: String,
        power_plan_guid: Option<String>,
        cx: &mut Context<Self>,
    ) {
        set_foreground_power_plan_override(
            &mut self.settings.by_foreground,
            &process_name,
            power_plan_guid,
        );
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_running_power_plan(
        &mut self,
        process_name: String,
        power_plan_guid: Option<String>,
        cx: &mut Context<Self>,
    ) {
        set_by_running_app_power_plan_override(
            &mut self.settings.by_running_app,
            &process_name,
            power_plan_guid,
        );
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_background_efficiency(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_background_efficiency_custom_rule(
            &mut self.settings.background_efficiency,
            &process_name,
            !included,
        );
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_background_cpu_restriction(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_process_exclusion(
            &mut self.settings.background_cpu_restriction.exclusions,
            &process_name,
            !included,
        );
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_core_limiter(
        &mut self,
        process_name: String,
        max_logical_processors: Option<u8>,
        cx: &mut Context<Self>,
    ) {
        set_core_limiter_override(
            &mut self.settings.core_limiter,
            &process_name,
            max_logical_processors,
        );
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_gpu_priority_included(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_process_exclusion(
            &mut self.settings.gpu_priority.exclusions,
            &process_name,
            !included,
        );
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_memory_trim(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_process_exclusion(
            &mut self.settings.memory_trim.exclusions,
            &process_name,
            !included,
        );
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_app_suspension(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_app_suspension_override(&mut self.settings.app_suspension, &process_name, included);
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_timer_resolution(
        &mut self,
        process_name: String,
        desired_100ns: Option<u32>,
        cx: &mut Context<Self>,
    ) {
        set_timer_resolution_override(
            &mut self.settings.timer_resolution,
            &process_name,
            desired_100ns,
        );
        self.finish_process_list_edit(cx);
    }

    fn apply_process_priority_once(
        &mut self,
        target: Result<ProcessActionTarget, ProcessActionTargetError>,
        process_name: &str,
        priority: ProcessPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        self.status_message = match target
            .map_err(|error| error.to_string())
            .and_then(|target| process_priority::apply_once(&target, priority))
        {
            Ok(priority) => t!(
                "process_list.applied_once",
                name = process_name,
                priority = priority
            )
            .to_string(),
            Err(error) => t!(
                "process_list.apply_once_failed",
                name = process_name,
                error = error
            )
            .to_string(),
        };
        cx.notify();
    }

    fn save_process_priority_rule(
        &mut self,
        process_name: &str,
        priority: ProcessPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        set_process_priority_rule(&mut self.settings.process_priority, process_name, priority);
        if self.save_settings() {
            let key = if self.settings.process_priority.enabled {
                "process_list.saved_priority_rule"
            } else {
                "process_list.saved_priority_rule_disabled"
            };
            self.status_message = t!(
                key,
                name = process_name,
                priority = process_priority_setting_label(priority)
            )
            .to_string();
        }
        cx.notify();
    }

    fn apply_memory_priority_once(
        &mut self,
        target: Result<ProcessActionTarget, ProcessActionTargetError>,
        process_name: &str,
        priority: ProcessMemoryPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        self.status_message = match target
            .map_err(|error| error.to_string())
            .and_then(|target| memory_priority::apply_once(&target, priority))
        {
            Ok(priority) => t!(
                "process_list.applied_memory_once",
                name = process_name,
                priority = priority
            )
            .to_string(),
            Err(error) => t!(
                "process_list.apply_memory_once_failed",
                name = process_name,
                error = error
            )
            .to_string(),
        };
        cx.notify();
    }

    fn save_memory_priority_rule(
        &mut self,
        process_name: &str,
        priority: ProcessMemoryPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        set_memory_priority_rule(&mut self.settings.memory_priority, process_name, priority);
        if self.save_settings() {
            let key = if self.settings.memory_priority.enabled {
                "process_list.saved_memory_rule"
            } else {
                "process_list.saved_memory_rule_disabled"
            };
            self.status_message = t!(
                key,
                name = process_name,
                priority = process_memory_priority_setting_label(priority)
            )
            .to_string();
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

    #[expect(
        clippy::too_many_arguments,
        reason = "dropdown metadata plus the options builder is clearer at each UI call site"
    )]
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
        let phase = dropdown_popup_phase(id.as_str(), is_open, cx);
        let control_id = SharedString::from(format!("{id}-control"));
        let toggle_id = id.clone();

        dropdown_select_container(width)
            .child(
                dropdown_select_control(control_id, selected_label, enabled, is_open, phase, cx)
                    .when(enabled, |control| {
                        control.on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                            app.active_power_plan_picker =
                                (app.active_power_plan_picker.as_deref()
                                    != Some(toggle_id.as_str()))
                                .then_some(toggle_id.clone());
                            cx.stop_propagation();
                            cx.notify();
                        }))
                    }),
            )
            .child(dropdown_anchor_sensor(
                id.clone(),
                Rc::clone(&self.dropdown_anchor_bounds),
            ))
            .child(dropdown_popup_or_empty(
                SharedString::from(id),
                phase,
                placement,
                options,
                cx,
            ))
            .into_any_element()
    }

    fn sync_input_values(&mut self, cx: &mut Context<Self>) {
        for (rule, input) in self
            .settings
            .by_cpu_load
            .rules
            .iter_mut()
            .zip(&self.inputs.by_cpu_load_rule_names)
        {
            rule.name = input.read(cx).value().to_string();
        }
        for (index, rule) in self.settings.by_time.rules.iter_mut().enumerate() {
            if let Some(input) = self.inputs.by_time_rule_names.get(index) {
                rule.name = input.read(cx).value().to_string();
            }
            if let Some(input) = self.inputs.schedule_start_times.get(index) {
                rule.start_time = input.read(cx).value().to_string();
            }
            if let Some(input) = self.inputs.schedule_end_times.get(index) {
                rule.end_time = input.read(cx).value().to_string();
            }
        }
        for (index, rule) in self.settings.by_foreground.rules.iter_mut().enumerate() {
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
        runtime_settings_from(&self.settings, &self.saved_settings)
    }

    fn cached_runtime_settings(&mut self) -> Arc<Settings> {
        self.sync_background_settings();
        Arc::clone(&self.last_background_settings)
    }

    fn sync_background_settings(&mut self) {
        if runtime_settings_matches(
            self.last_background_settings.as_ref(),
            &self.settings,
            &self.saved_settings,
        ) {
            return;
        }

        let settings = Arc::new(self.background_settings());
        self.sync_adaptive_engine(settings.as_ref());
        self.background_automation
            .update_settings(settings.as_ref());
        self.last_background_settings = settings;
    }
}

impl Drop for WinderustApp {
    fn drop(&mut self) {
        let _ = self_power::disable_adaptive_engine();
    }
}

fn runtime_settings_from(current: &Settings, saved: &Settings) -> Settings {
    let mut settings = saved.clone();
    settings.general = current.general.clone();
    settings.general.enabled = saved.general.enabled;
    settings.advanced = current.advanced.clone();
    settings
}

fn runtime_settings_matches(settings: &Settings, current: &Settings, saved: &Settings) -> bool {
    settings.general.enabled == saved.general.enabled
        && settings.general.startup_with_windows == current.general.startup_with_windows
        && settings.general.start_minimized == current.general.start_minimized
        && settings.general.hide_to_tray == current.general.hide_to_tray
        && settings.general.check_for_updates == current.general.check_for_updates
        && settings.general.update_channel == current.general.update_channel
        && settings.general.theme_mode == current.general.theme_mode
        && settings.general.accent == current.general.accent
        && settings.general.language == current.general.language
        && settings.general.animation_mode == current.general.animation_mode
        && settings.general.pause_power_plan_switching_while_plugged_in
            == current.general.pause_power_plan_switching_while_plugged_in
        && settings.general.check_interval_ms == current.general.check_interval_ms
        && settings.advanced == current.advanced
        && settings.adaptive_engine == saved.adaptive_engine
        && settings.by_activity == saved.by_activity
        && settings.by_foreground == saved.by_foreground
        && settings.by_time == saved.by_time
        && settings.by_cpu_load == saved.by_cpu_load
        && settings.background_efficiency == saved.background_efficiency
        && settings.app_suspension == saved.app_suspension
        && settings.core_steering == saved.core_steering
        && settings.background_cpu_restriction == saved.background_cpu_restriction
        && settings.core_limiter == saved.core_limiter
        && settings.by_running_app == saved.by_running_app
        && settings.workload_engine == saved.workload_engine
        && settings.io_priority == saved.io_priority
        && settings.gpu_priority == saved.gpu_priority
        && settings.memory_priority == saved.memory_priority
        && settings.memory_trim == saved.memory_trim
        && settings.timer_resolution == saved.timer_resolution
}

impl Render for WinderustApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.inputs.ensure_for_settings(window, cx, &self.settings);
        self.ensure_rule_title_input_subscriptions(window, cx);
        self.ensure_cpu_threshold_slider_subscriptions(window, cx);
        self.sync_input_values(cx);
        UI_ANIMATIONS_ENABLED.store(
            resolve_animation_enabled(self.settings.general.animation_mode),
            Ordering::Relaxed,
        );
        self.clear_finished_breadcrumb_transition();

        let search_query = self.dashboard_search_query(cx);
        let search_active = !search_query.is_empty();
        let page_body = if search_active {
            self.render_search_results_page(&search_query, cx)
        } else {
            self.render_page(window, cx)
        };
        let page_header = if search_active {
            search_results_page_header(cx).into_any_element()
        } else {
            self.page_header(self.page, cx).into_any_element()
        };
        let page_uses_inner_scroll = !search_active && self.page == Page::ProcessList;
        let unsaved = self.settings != self.saved_settings;
        let unsaved_popup_vanish_progress = self.unsaved_popup_vanish_progress(unsaved, window);
        let show_unsaved_popup = unsaved || unsaved_popup_vanish_progress.is_some();
        let show_admin_rights_prompt = self.admin_rights_prompt_visible;
        let admin_rights_prompt_bottom = if show_unsaved_popup { 190.0 } else { 54.0 };
        let page_content = animated_page_content_frame(
            page_content_frame(
                page_header,
                page_body,
                page_uses_inner_scroll,
                !search_active && self.page == Page::ProcessList,
            ),
            self.active_breadcrumb_transition(self.page),
        );
        let page_scroll_area = if page_uses_inner_scroll {
            v_flex()
                .flex_1()
                .h_full()
                .min_w(px(0.0))
                .min_h(px(0.0))
                .overflow_hidden()
                .child(page_content)
                .into_any_element()
        } else {
            v_flex()
                .flex_1()
                .h_full()
                .min_w(px(0.0))
                .min_h(px(0.0))
                .overflow_y_scrollbar()
                .child(page_content)
                .into_any_element()
        };

        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .font_family(FONT_UI)
            .capture_any_mouse_down(cx.listener(|app, event: &gpui::MouseDownEvent, _, cx| {
                handle_navigation_mouse_button(app, event.button, cx);
            }))
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
                    .child(self.render_navigation(window, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .h_full()
                            .min_w(px(0.0))
                            .min_h(px(0.0))
                            .overflow_hidden()
                            .child(page_scroll_area),
                    ),
            )
            .child(if show_unsaved_popup {
                self.render_unsaved_popup(unsaved_popup_vanish_progress, cx)
                    .into_any_element()
            } else {
                div().into_any_element()
            })
            .child(if show_admin_rights_prompt {
                self.render_admin_rights_prompt(admin_rights_prompt_bottom, cx)
            } else {
                div().into_any_element()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_status_localizes_known_messages_and_preserves_errors() {
        assert_eq!(
            localized_runtime_status("Automation disabled."),
            t!("runtime_status.automation_disabled").to_string()
        );
        assert_eq!(localized_runtime_status("Win32 error 5"), "Win32 error 5");
    }

    #[test]
    fn windows_accent_abgr_converts_to_rgb() {
        assert_eq!(windows_abgr_to_rgb(0xffb16300), 0x0063b1);
    }

    #[test]
    fn windows_accent_palette_uses_second_tint() {
        let palette = [
            0xc6, 0xe7, 0xeb, 0x00, 0xa5, 0xc7, 0xd1, 0x00, 0x66, 0x8f, 0xa7, 0x00,
        ];
        assert_eq!(windows_accent_palette_tint(&palette), Some(0xa5c7d1));
        assert_eq!(windows_accent_palette_tint(&palette[..4]), None);
    }

    #[test]
    fn app_suspension_indicator_reports_network_intent_before_suspended_state() {
        let status = AppSuspensionSnapshot {
            enabled: true,
            network_wake_apps: vec!["vivaldi.exe".to_owned()],
            suspended_apps: vec!["vivaldi.exe".to_owned()],
            ..Default::default()
        };

        let indicator = app_suspension_indicator(&status, "vivaldi.exe");

        assert_eq!(
            indicator.label,
            t!("app_suspension.indicator.network").to_string()
        );
        assert_eq!(
            indicator.hover,
            t!("app_suspension.indicator.network_help").to_string()
        );
    }

    #[test]
    fn app_suspension_indicator_reports_running_before_not_running() {
        let status = AppSuspensionSnapshot {
            enabled: true,
            running_apps: vec!["vivaldi.exe".to_owned()],
            ..Default::default()
        };

        let indicator = app_suspension_indicator(&status, "vivaldi.exe");

        assert_eq!(
            indicator.label,
            t!("app_suspension.indicator.running").to_string()
        );
        assert_eq!(
            indicator.hover,
            t!("app_suspension.indicator.running_help").to_string()
        );
    }

    #[test]
    fn app_suspension_indicator_reports_unknown_before_stale_running_state() {
        let status = AppSuspensionSnapshot {
            enabled: true,
            running_apps: vec!["vivaldi.exe".to_owned()],
            status_unknown: true,
            ..Default::default()
        };

        let indicator = app_suspension_indicator(&status, "vivaldi.exe");

        assert_eq!(
            indicator.label,
            t!("app_suspension.indicator.unknown").to_string()
        );
        assert_eq!(
            indicator.hover,
            t!("app_suspension.indicator.unknown_help").to_string()
        );
    }

    #[test]
    fn system_accent_keeps_neutral_surfaces() {
        assert_eq!(
            accent_surface_color_with_tint(COLOR_APP_BG, 0.5, 0xffffff, false),
            COLOR_APP_BG
        );
        assert_ne!(
            accent_surface_color_with_tint(COLOR_APP_BG, 0.5, 0xffffff, true),
            COLOR_APP_BG
        );
    }

    #[test]
    fn workload_engine_presets_set_background_process_priority() {
        let low_impact = workload_engine_preset_values(WorkloadEnginePreset::LowImpact);
        let foreground_first = workload_engine_preset_values(WorkloadEnginePreset::ForegroundFirst);
        let max_foreground = workload_engine_preset_values(WorkloadEnginePreset::MaxForeground);

        assert_eq!(low_impact.background_priority, ProcessPriority::Idle);
        assert_eq!(foreground_first.background_priority, ProcessPriority::Idle);
        assert_eq!(max_foreground.background_priority, ProcessPriority::Idle);
        assert!(low_impact.workload_engine_background_efficiency_enabled);
        assert!(foreground_first.workload_engine_background_efficiency_enabled);
        assert!(max_foreground.workload_engine_background_efficiency_enabled);
        assert!(low_impact.lower_background_io_priority_enabled);
        assert!(foreground_first.lower_background_io_priority_enabled);
        assert!(max_foreground.lower_background_io_priority_enabled);
        assert!(low_impact.workload_engine_memory_priority_enabled);
        assert!(foreground_first.workload_engine_memory_priority_enabled);
        assert!(max_foreground.workload_engine_memory_priority_enabled);
        assert_eq!(
            low_impact.lower_background_io_priority,
            ProcessIoPriority::Low
        );
        assert_eq!(
            foreground_first.lower_background_io_priority,
            ProcessIoPriority::VeryLow
        );
        assert_eq!(
            max_foreground.lower_background_io_priority,
            ProcessIoPriority::VeryLow
        );
        assert_eq!(
            foreground_first.workload_engine_foreground_memory_priority,
            ProcessMemoryPrioritySetting::Normal
        );
        assert_eq!(low_impact.max_targeted_processes, 12);
        assert_eq!(foreground_first.max_targeted_processes, 12);
        assert_eq!(max_foreground.max_targeted_processes, 12);
        assert!(low_impact.workload_engine_affinity_escalation_enabled);
        assert!(foreground_first.workload_engine_affinity_escalation_enabled);
        assert!(max_foreground.workload_engine_affinity_escalation_enabled);
        assert!(low_impact.lower_background_apps);
        assert!(foreground_first.lower_background_apps);
        assert!(max_foreground.lower_background_apps);
        assert!(low_impact.boost_foreground_app);
        assert!(foreground_first.boost_foreground_app);
        assert!(max_foreground.boost_foreground_app);
        assert!(low_impact.lower_background_auto_cpu_percent);
        assert!(foreground_first.lower_background_auto_cpu_percent);
        assert!(!max_foreground.lower_background_auto_cpu_percent);
        assert_eq!(low_impact.foreground_boost, ForegroundBoostPriority::Auto);
        assert_eq!(
            foreground_first.foreground_boost,
            ForegroundBoostPriority::Auto
        );
        assert_eq!(
            max_foreground.foreground_boost,
            ForegroundBoostPriority::AboveNormal
        );
        assert!(low_impact.total_threshold > foreground_first.total_threshold);
        assert!(foreground_first.total_threshold > max_foreground.total_threshold);
        assert!(low_impact.process_threshold > foreground_first.process_threshold);
        assert!(foreground_first.process_threshold > max_foreground.process_threshold);
        assert_eq!(low_impact.manual_cpu_percent, 60);
        assert_eq!(foreground_first.manual_cpu_percent, 16);
        assert_eq!(max_foreground.manual_cpu_percent, 6);
        assert!(
            workload_engine_thread_priority_preset_values(WorkloadEnginePreset::LowImpact).enabled
        );
        assert!(
            workload_engine_dynamic_priority_boost_preset_values(WorkloadEnginePreset::LowImpact)
                .enabled
        );
        assert!(
            workload_engine_gpu_priority_preset_values(WorkloadEnginePreset::LowImpact).enabled
        );
        assert_eq!(
            workload_engine_gpu_priority_preset_values(WorkloadEnginePreset::LowImpact)
                .background_priority,
            ProcessGpuPrioritySetting::BelowNormal
        );
        assert_eq!(
            max_foreground.foreground_io_priority,
            ProcessIoPrioritySetting::High
        );
        assert_eq!(
            workload_engine_thread_priority_preset_values(WorkloadEnginePreset::MaxForeground)
                .foreground_priority,
            ProcessThreadPrioritySetting::Highest
        );
        assert_eq!(
            workload_engine_thread_priority_preset_values(WorkloadEnginePreset::MaxForeground)
                .background_priority,
            ProcessThreadPrioritySetting::Idle
        );
        assert_eq!(
            workload_engine_gpu_priority_preset_values(WorkloadEnginePreset::MaxForeground)
                .foreground_priority,
            ProcessGpuPrioritySetting::High
        );
        assert_eq!(
            workload_engine_gpu_priority_preset_values(WorkloadEnginePreset::MaxForeground)
                .background_priority,
            ProcessGpuPrioritySetting::Idle
        );
    }

    #[test]
    fn adaptive_engine_default_keeps_workload_engine_opt_in() {
        let mut settings = Settings::default();

        apply_adaptive_engine(&mut settings, true);

        assert!(settings.adaptive_engine.enabled);
        assert!(settings.adaptive_engine.processor_policy_enabled);
        assert!(!settings.background_efficiency.enabled);
        assert!(!settings.workload_engine.enabled);
        assert!(!settings.workload_engine.workload_engine_enabled);
    }

    #[test]
    fn power_mode_presets_combine_adaptive_engine_and_workload_engine() {
        let mut settings = Settings::default();

        apply_power_mode_preset(&mut settings, PowerModePreset::PowerSave);
        assert!(power_mode_matches_preset(
            &settings,
            PowerModePreset::PowerSave
        ));
        assert!(settings.adaptive_engine.enabled);
        assert!(settings.adaptive_engine.processor_policy_enabled);
        assert!(settings.background_efficiency.enabled);
        assert!(settings.workload_engine.enabled);

        apply_power_mode_preset(&mut settings, PowerModePreset::Balanced);
        assert!(power_mode_matches_preset(
            &settings,
            PowerModePreset::Balanced
        ));
        assert!(settings.adaptive_engine.enabled);
        assert!(!settings.background_efficiency.enabled);
        assert!(settings.workload_engine.enabled);
        assert!(workload_engine_matches_preset(
            &settings.workload_engine,
            WorkloadEnginePreset::LowImpact
        ));

        apply_power_mode_preset(&mut settings, PowerModePreset::Performance);
        assert!(power_mode_matches_preset(
            &settings,
            PowerModePreset::Performance
        ));
        assert!(!settings.adaptive_engine.enabled);
        assert!(!settings.background_efficiency.enabled);
        assert!(settings.workload_engine.enabled);
        assert!(settings.workload_engine.workload_engine_enabled);
        assert!(settings.adaptive_engine.processor_policy_enabled);
        assert!(workload_engine_matches_preset(
            &settings.workload_engine,
            WorkloadEnginePreset::ForegroundFirst
        ));

        apply_power_mode_preset(&mut settings, PowerModePreset::Speed);
        assert!(power_mode_matches_preset(&settings, PowerModePreset::Speed));
        assert!(!settings.adaptive_engine.enabled);
        assert!(settings.workload_engine.enabled);
        assert!(settings.workload_engine.workload_engine_enabled);
        assert!(settings.adaptive_engine.processor_policy_enabled);
        assert!(workload_engine_matches_preset(
            &settings.workload_engine,
            WorkloadEnginePreset::MaxForeground
        ));
    }

    #[test]
    fn adaptive_engine_custom_targets_make_preset_custom() {
        let mut settings = Settings::default();

        apply_power_mode_preset(&mut settings, PowerModePreset::Balanced);
        settings
            .adaptive_engine
            .processor_policy_values
            .performance_max = 55;

        assert!(!power_mode_matches_preset(
            &settings,
            PowerModePreset::Balanced
        ));
        assert_eq!(
            settings
                .adaptive_engine
                .processor_policy_values
                .performance_max,
            55
        );

        apply_power_mode_preset(&mut settings, PowerModePreset::Balanced);
        apply_workload_engine_preset(
            &mut settings.workload_engine,
            WorkloadEnginePreset::ForegroundFirst,
        );

        assert!(!power_mode_matches_preset(
            &settings,
            PowerModePreset::Balanced
        ));
        assert!(workload_engine_matches_preset(
            &settings.workload_engine,
            WorkloadEnginePreset::ForegroundFirst
        ));
    }

    #[test]
    fn adaptive_engine_uses_low_power_app_tick() {
        let mut settings = Settings::default();

        assert_eq!(app_tick_interval(&settings, true), APP_TICK_INTERVAL);
        settings.adaptive_engine.enabled = true;
        assert_eq!(app_tick_interval(&settings, false), APP_TICK_INTERVAL);
        assert_eq!(
            app_tick_interval(&settings, true),
            ADAPTIVE_ENGINE_APP_TICK_INTERVAL
        );
    }

    #[test]
    fn workload_engine_preset_match_ignores_hidden_preserve_flags() {
        let mut settings = WorkloadEngineSettings::default();
        apply_workload_engine_preset(&mut settings, WorkloadEnginePreset::ForegroundFirst);
        settings
            .workload_engine_io_priority
            .preserve_foreground_priority = false;
        settings
            .workload_engine_thread_priority
            .preserve_background_priority = false;
        settings
            .workload_engine_gpu_priority
            .foreground_detection_enabled = false;

        assert!(workload_engine_matches_preset(
            &settings,
            WorkloadEnginePreset::ForegroundFirst
        ));
    }

    #[test]
    fn cpu_frequency_graph_uses_base_clock_as_floor() {
        assert_eq!(
            normalize_cpu_frequency_percent(Some(3_000), 3_000, Some(5_000)),
            0.0
        );
        assert_eq!(
            normalize_cpu_frequency_percent(Some(4_000), 3_000, Some(5_000)),
            50.0
        );
        assert_eq!(
            normalize_cpu_frequency_percent(Some(5_500), 3_000, Some(5_000)),
            100.0
        );
        assert_eq!(
            normalize_cpu_frequency_percent(None, 3_000, Some(5_000)),
            0.0
        );
        assert_eq!(
            normalize_cpu_frequency_percent(Some(4_000), 3_000, None),
            0.0
        );
    }

    #[test]
    fn dashboard_dual_line_points_pad_and_keep_latest_samples() {
        let points = dashboard_dual_line_points(
            (0..(CPU_USAGE_HISTORY_LEN + 2)).map(|index| (index as f32, (index * 2) as f32)),
            |value| format!("{:?}", value),
            |value| format!("{:?}", value),
        );

        assert_eq!(points.len(), CPU_USAGE_HISTORY_LEN);
        assert_eq!(points[0].first_value, 2.0);
        assert_eq!(points[0].second_value, 4.0);
        assert_eq!(
            points[CPU_USAGE_HISTORY_LEN - 1].first_value,
            (CPU_USAGE_HISTORY_LEN + 1) as f64
        );

        let padded = dashboard_dual_line_points(
            [(7.0, 9.0)].into_iter(),
            |value| format!("{:?}", value),
            |value| format!("{:?}", value),
        );
        assert_eq!(padded.len(), CPU_USAGE_HISTORY_LEN);
        assert_eq!(padded[CPU_USAGE_HISTORY_LEN - 2].first_value, 0.0);
        assert_eq!(padded[CPU_USAGE_HISTORY_LEN - 1].first_value, 7.0);
    }

    #[test]
    fn memory_cache_percent_uses_total_memory_scale() {
        assert_eq!(memory_bytes_percent(Some(4), Some(16)), Some(25.0));
        assert_eq!(memory_bytes_percent(Some(32), Some(16)), Some(100.0));
        assert_eq!(memory_bytes_percent(Some(4), Some(0)), None);
        assert_eq!(memory_bytes_percent(None, Some(16)), None);
    }

    #[test]
    fn refresh_due_advances_only_after_deadline() {
        let now = Instant::now();
        let mut next_refresh = now + Duration::from_secs(1);

        assert!(!refresh_due(now, &mut next_refresh, Duration::from_secs(3)));
        assert_eq!(next_refresh, now + Duration::from_secs(1));

        assert!(refresh_due(
            now + Duration::from_secs(1),
            &mut next_refresh,
            Duration::from_secs(3)
        ));
        assert_eq!(next_refresh, now + Duration::from_secs(4));
    }

    #[test]
    fn active_plan_guid_returns_active_plan_only() {
        let plans = vec![
            PowerPlan {
                guid: "balanced".to_owned(),
                name: "Balanced".to_owned(),
                active: false,
            },
            PowerPlan {
                guid: "saver".to_owned(),
                name: "Saver".to_owned(),
                active: true,
            },
        ];

        assert_eq!(active_plan_guid(&plans), Some("saver"));
        assert_eq!(active_plan_guid(&[]), None);
    }

    #[test]
    fn runtime_settings_gates_feature_sections_until_save() {
        let mut current = Settings::default();
        let mut saved = Settings::default();

        current.general.enabled = false;
        saved.general.enabled = true;
        current.general.check_interval_ms = 1_234;
        saved.general.check_interval_ms = 5_678;
        current.advanced.action_log_mode = ActionLogMode::Off;
        saved.advanced.action_log_mode = ActionLogMode::Full;
        current.by_activity.power_plans.performance_guid = Some("current".to_owned());
        saved.by_activity.power_plans.performance_guid = Some("saved".to_owned());
        current.background_cpu_restriction.enabled = true;
        saved.background_cpu_restriction.enabled = false;
        current.io_priority.enabled = true;
        saved.io_priority.enabled = false;
        current.timer_resolution.enabled = true;
        saved.timer_resolution.enabled = false;
        current.memory_trim.enabled = false;
        saved.memory_trim.enabled = true;

        let settings = runtime_settings_from(&current, &saved);

        assert!(settings.general.enabled);
        assert_eq!(settings.general.check_interval_ms, 1_234);
        assert_eq!(settings.advanced.action_log_mode, ActionLogMode::Off);
        assert_eq!(
            settings.by_activity.power_plans.performance_guid.as_deref(),
            Some("saved")
        );
        assert!(!settings.background_cpu_restriction.enabled);
        assert!(!settings.io_priority.enabled);
        assert!(!settings.timer_resolution.enabled);
        assert!(settings.memory_trim.enabled);
        assert!(runtime_settings_matches(&settings, &current, &saved));

        let mut stale_saved_section = settings.clone();
        stale_saved_section.memory_trim.enabled = false;
        assert!(!runtime_settings_matches(
            &stale_saved_section,
            &current,
            &saved
        ));

        let mut stale_saved_section = settings;
        stale_saved_section.timer_resolution.enabled = true;
        assert!(!runtime_settings_matches(
            &stale_saved_section,
            &current,
            &saved
        ));
    }

    #[test]
    fn process_list_column_layout_fits_headers_and_values() {
        let settings = Settings::default();
        let processes = vec![
            ProcessInfo {
                id: 1234,
                parent_id: None,
                name: "editor.exe".to_owned(),
            },
            ProcessInfo {
                id: 12345,
                parent_id: None,
                name: "worker.exe".to_owned(),
            },
        ];
        let groups = process_list_groups(&processes);
        let summaries = groups
            .iter()
            .map(|_| default_process_policy_summary())
            .collect::<Vec<_>>();

        let layout = process_list_column_layout(&settings, &groups, &summaries);

        assert!(layout.column_width(ProcessListColumn::Pid) < PROCESS_LIST_PID_MAX_WIDTH);
        assert!(layout.column_width(ProcessListColumn::MemoryTrim) < 140.0);
        assert!(
            layout.column_width(ProcessListColumn::PowerPlanForeground)
                >= process_list_estimated_cell_width(
                    &process_list_column_label(ProcessListColumn::PowerPlanForeground, &settings),
                    PROCESS_LIST_TEXT_CELL_HORIZONTAL_PADDING,
                )
        );
    }

    #[test]
    fn process_icon_cache_drops_stale_paths() {
        let kept_path = PathBuf::from("C:\\Apps\\kept.exe");
        let stale_path = PathBuf::from("C:\\Apps\\stale.exe");
        let mut cache = HashMap::from([(kept_path.clone(), None), (stale_path.clone(), None)]);
        let candidates = vec![ProcessCandidate {
            name: "kept.exe".to_owned(),
            image_path: Some(kept_path.clone()),
            icon: None,
        }];

        WinderustApp::retain_current_process_icons(&mut cache, &candidates);

        assert!(cache.contains_key(&kept_path));
        assert!(!cache.contains_key(&stale_path));
    }

    #[test]
    fn process_list_sort_orders_groups_by_name_direction() {
        let processes = vec![
            ProcessInfo {
                id: 1,
                parent_id: None,
                name: "editor.exe".to_owned(),
            },
            ProcessInfo {
                id: 2,
                parent_id: None,
                name: "worker.exe".to_owned(),
            },
        ];
        let groups = process_list_groups(&processes);
        let summaries = groups
            .iter()
            .map(|_| default_process_policy_summary())
            .collect::<Vec<_>>();
        let rows = process_list_sorted_rows(
            groups,
            summaries,
            ProcessListSort {
                column: ProcessListSortColumn::ProcessName,
                direction: ProcessListSortDirection::Descending,
            },
        );

        assert_eq!(rows[0].0.display_name, "worker.exe");
        assert_eq!(rows[1].0.display_name, "editor.exe");
    }

    #[test]
    fn process_list_text_sort_cmp_matches_ascii_lowercase_sorting() {
        for (left, right) in [
            ("Alpha.exe", "alpha.exe"),
            ("worker.exe", "Editor.exe"),
            ("z.exe", "é.exe"),
        ] {
            let expected = left
                .to_ascii_lowercase()
                .cmp(&right.to_ascii_lowercase())
                .then_with(|| left.cmp(right));
            assert_eq!(process_list_text_sort_cmp(left, right), expected);
        }
    }

    #[test]
    fn process_list_sort_orders_groups_and_children_by_pid() {
        let processes = vec![
            ProcessInfo {
                id: 30,
                parent_id: None,
                name: "editor.exe".to_owned(),
            },
            ProcessInfo {
                id: 10,
                parent_id: None,
                name: "worker.exe".to_owned(),
            },
            ProcessInfo {
                id: 20,
                parent_id: None,
                name: "editor.exe".to_owned(),
            },
        ];
        let sort = ProcessListSort {
            column: ProcessListSortColumn::Column(ProcessListColumn::Pid),
            direction: ProcessListSortDirection::Ascending,
        };
        let mut groups = process_list_groups(&processes);
        for group in &mut groups {
            process_list_sort_group_processes(group, sort);
        }
        let summaries = groups
            .iter()
            .map(|_| default_process_policy_summary())
            .collect::<Vec<_>>();
        let rows = process_list_sorted_rows(groups, summaries, sort);

        assert_eq!(rows[0].0.display_name, "worker.exe");
        assert_eq!(rows[1].0.display_name, "editor.exe");
        assert_eq!(rows[1].0.processes[0].id, 20);
        assert_eq!(rows[1].0.processes[1].id, 30);

        let sort = ProcessListSort {
            column: ProcessListSortColumn::Column(ProcessListColumn::Pid),
            direction: ProcessListSortDirection::Descending,
        };
        let mut groups = process_list_groups(&processes);
        for group in &mut groups {
            process_list_sort_group_processes(group, sort);
        }
        let summaries = groups
            .iter()
            .map(|_| default_process_policy_summary())
            .collect::<Vec<_>>();
        let rows = process_list_sorted_rows(groups, summaries, sort);

        assert_eq!(rows[0].0.display_name, "editor.exe");
        assert_eq!(rows[0].0.processes[0].id, 30);
        assert_eq!(rows[0].0.processes[1].id, 20);
        assert_eq!(rows[1].0.display_name, "worker.exe");
    }

    #[test]
    fn process_list_sort_orders_groups_by_policy_column_value() {
        let processes = vec![
            ProcessInfo {
                id: 1,
                parent_id: None,
                name: "editor.exe".to_owned(),
            },
            ProcessInfo {
                id: 2,
                parent_id: None,
                name: "worker.exe".to_owned(),
            },
        ];
        let groups = process_list_groups(&processes);
        let mut low = default_process_policy_summary();
        low.process_priority = "Idle".to_owned();
        let mut high = default_process_policy_summary();
        high.process_priority = "Normal".to_owned();
        let rows = process_list_sorted_rows(
            groups,
            vec![high, low],
            ProcessListSort {
                column: ProcessListSortColumn::Column(ProcessListColumn::ProcessPriority),
                direction: ProcessListSortDirection::Ascending,
            },
        );

        assert_eq!(rows[0].0.display_name, "worker.exe");
        assert_eq!(rows[1].0.display_name, "editor.exe");
    }

    #[test]
    fn process_list_policy_value_active_tracks_state_and_custom_values() {
        assert!(process_list_policy_value_active("Include", false));
        assert!(process_list_policy_value_active("Include (50%)", false));
        assert!(!process_list_policy_value_active("Exclude", true));
        assert!(!process_list_policy_value_active(
            process_list_default_label().as_str(),
            true
        ));
        assert!(!process_list_policy_value_active("Balanced", false));
        assert!(process_list_policy_value_active("Balanced", true));
    }

    #[test]
    fn process_list_split_policy_value_parses_foreground_background_pairs() {
        assert_eq!(
            process_list_split_policy_value("Normal / Very low"),
            Some(("Normal", "Very low"))
        );
        assert_eq!(
            process_list_split_policy_value("  Above normal / Idle  "),
            Some(("Above normal", "Idle"))
        );
        assert_eq!(process_list_split_policy_value("Default"), None);
    }

    #[test]
    fn process_list_policy_cell_editing_respects_row_editability() {
        assert!(!process_list_policy_cell_editable(
            true,
            ProcessListColumn::ProcessPriority
        ));
        assert!(!process_list_policy_cell_editable(
            false,
            ProcessListColumn::ProcessPriority
        ));
        assert!(!process_list_policy_cell_editable(
            true,
            ProcessListColumn::CoreSteering
        ));
    }

    #[test]
    fn process_policy_summary_matches_exact_process_rule() {
        let mut settings = Settings::default();
        settings.core_steering.enabled = true;
        settings.core_steering.rules.push(CoreSteeringRule {
            enabled: true,
            mode: CoreSteeringMode::Soft,
            process_name: "Editor.EXE".to_owned(),
            core_mask: 0b1011,
        });

        let matching = process_policy_summary(&settings, &[], "editor.exe");
        assert_eq!(matching.core_steering, "0-1, 3");
        assert!(matching.uses_custom_rule(ProcessListColumn::CoreSteering));

        let non_matching = process_policy_summary(&settings, &[], "browser.exe");
        assert_eq!(
            non_matching.power_plan_foreground,
            process_list_default_label()
        );
        assert_eq!(
            non_matching.power_plan_running,
            process_list_default_label()
        );
        assert_eq!(non_matching.core_steering, default_core_steering_label());
        assert_eq!(
            non_matching.process_priority,
            process_priority_setting_label(ProcessPrioritySetting::Default)
        );
        assert!(!non_matching.uses_custom_rule(ProcessListColumn::CoreSteering));
    }

    #[test]
    fn process_policy_summary_reports_priority_policy_values() {
        let mut settings = Settings::default();
        settings.io_priority.enabled = true;
        settings.gpu_priority.enabled = true;
        settings.memory_priority.enabled = true;

        let summary = process_policy_summary(&settings, &[], "editor.exe");

        assert_eq!(
            summary.io_priority,
            io_priority_policy_label(&settings.io_priority)
        );
        assert_eq!(
            summary.gpu_priority,
            gpu_priority_policy_label(&settings.gpu_priority)
        );
        assert_eq!(
            summary.memory_priority,
            memory_priority_policy_label(&settings.memory_priority)
        );
    }

    #[test]
    fn process_policy_summary_reports_process_rule_columns() {
        let mut settings = Settings::default();
        settings.by_foreground.enabled = true;
        settings.by_foreground.rules.push(ByForegroundRule {
            enabled: true,
            name: "Editor".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: Some("balanced-guid".to_owned()),
        });
        settings.by_running_app.enabled = true;
        settings.by_running_app.rules.push(ByRunningAppRule {
            enabled: true,
            name: "Editor".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: Some("performance-guid".to_owned()),
        });
        settings.core_limiter.enabled = true;
        settings.core_limiter.rules.push(CoreLimiterRule {
            enabled: true,
            process_name: "editor.exe".to_owned(),
            threshold_percent: 80,
            sustain_seconds: 5,
            cooldown_seconds: 30,
            max_logical_processors: 50,
        });
        settings.app_suspension.enabled = true;
        settings
            .app_suspension
            .suspendable_apps
            .push(AppSuspensionRule {
                enabled: true,
                process_name: "editor.exe".to_owned(),
                network_wake_enabled: true,
                audio_wake_enabled: true,
                network_download_threshold_bytes: 1,
                network_download_threshold_unit: NetworkThresholdUnit::Bytes,
                network_upload_threshold_bytes: 0,
                network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
            });
        settings.timer_resolution.enabled = true;
        settings.timer_resolution.rules.push(TimerResolutionRule {
            enabled: true,
            process_name: "editor.exe".to_owned(),
            desired_100ns: 10_000,
        });
        let plans = vec![
            PowerPlan {
                guid: "balanced-guid".to_owned(),
                name: "Balanced".to_owned(),
                active: false,
            },
            PowerPlan {
                guid: "performance-guid".to_owned(),
                name: "Performance".to_owned(),
                active: false,
            },
        ];

        let summary = process_policy_summary(&settings, &plans, "editor.exe");

        assert_eq!(summary.power_plan_foreground, "Balanced");
        assert_eq!(summary.power_plan_running, "Performance");
        assert_eq!(
            summary.core_limiter,
            process_list_include_value_label("50%")
        );
        assert_eq!(summary.app_suspension, process_list_include_label());
        assert_eq!(summary.timer_resolution, "1.00 ms");
        assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanForeground));
        assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanRunning));
        assert!(summary.uses_custom_rule(ProcessListColumn::CoreLimiter));
        assert!(summary.uses_custom_rule(ProcessListColumn::AppSuspension));
        assert!(summary.uses_custom_rule(ProcessListColumn::TimerResolution));
    }

    #[test]
    fn process_policy_summary_reports_include_exclude_columns() {
        let mut settings = Settings::default();
        settings
            .background_efficiency
            .custom_rules
            .push(new_background_efficiency_rule("editor.exe"));
        settings
            .memory_trim
            .exclusions
            .push(new_process_exclusion_rule("editor.exe"));
        settings
            .app_suspension
            .suspendable_apps
            .push(AppSuspensionRule {
                enabled: true,
                process_name: "editor.exe".to_owned(),
                network_wake_enabled: true,
                audio_wake_enabled: true,
                network_download_threshold_bytes: 1,
                network_download_threshold_unit: NetworkThresholdUnit::Bytes,
                network_upload_threshold_bytes: 0,
                network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
            });

        let summary = process_policy_summary(&settings, &[], "editor.exe");

        assert_eq!(summary.background_efficiency, process_list_exclude_label());
        assert_eq!(summary.core_limiter, process_list_exclude_label());
        assert_eq!(summary.memory_trim, process_list_exclude_label());
        assert_eq!(summary.app_suspension, process_list_include_label());
        assert_eq!(summary.timer_resolution, process_list_default_label());
        assert!(summary.uses_custom_rule(ProcessListColumn::BackgroundEfficiency));
        assert!(!summary.uses_custom_rule(ProcessListColumn::CoreLimiter));
        assert!(summary.uses_custom_rule(ProcessListColumn::MemoryTrim));
        assert!(summary.uses_custom_rule(ProcessListColumn::AppSuspension));
        assert!(!summary.uses_custom_rule(ProcessListColumn::TimerResolution));
    }

    #[test]
    fn process_policy_summary_reports_priority_exclusions_as_exclude() {
        let mut settings = Settings::default();
        settings
            .io_priority
            .exclusions
            .push(new_process_exclusion_rule("editor.exe"));
        settings
            .gpu_priority
            .exclusions
            .push(new_process_exclusion_rule("editor.exe"));
        settings
            .memory_priority
            .exclusions
            .push(new_process_exclusion_rule("editor.exe"));

        let summary = process_policy_summary(&settings, &[], "editor.exe");

        assert_eq!(summary.io_priority, process_list_exclude_label());
        assert_eq!(summary.gpu_priority, process_list_exclude_label());
        assert_eq!(summary.memory_priority, process_list_exclude_label());
        assert!(summary.uses_custom_rule(ProcessListColumn::IoPriority));
        assert!(summary.uses_custom_rule(ProcessListColumn::GpuPriority));
        assert!(summary.uses_custom_rule(ProcessListColumn::MemoryPriority));
    }

    #[test]
    fn process_list_rule_edit_helpers_update_process_overrides() {
        let mut settings = Settings::default();

        set_foreground_power_plan_override(
            &mut settings.by_foreground,
            "Editor.EXE",
            Some("balanced-guid".to_owned()),
        );
        let summary = process_policy_summary(&settings, &[], "editor.exe");
        assert_eq!(summary.power_plan_foreground, "balanced-guid");
        assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanForeground));

        set_foreground_power_plan_override(&mut settings.by_foreground, "editor.exe", None);
        let summary = process_policy_summary(&settings, &[], "editor.exe");
        assert_eq!(summary.power_plan_foreground, process_list_default_label());
        assert!(!summary.uses_custom_rule(ProcessListColumn::PowerPlanForeground));

        set_core_limiter_override(&mut settings.core_limiter, "editor.exe", Some(50));
        let summary = process_policy_summary(&settings, &[], "editor.exe");
        assert_eq!(
            summary.core_limiter,
            process_list_include_value_label("50%")
        );
        assert!(summary.uses_custom_rule(ProcessListColumn::CoreLimiter));

        set_core_limiter_override(&mut settings.core_limiter, "editor.exe", None);
        let summary = process_policy_summary(&settings, &[], "editor.exe");
        assert_eq!(summary.core_limiter, process_list_exclude_label());
        assert!(!summary.uses_custom_rule(ProcessListColumn::CoreLimiter));
    }

    #[test]
    fn process_list_rule_edit_helpers_update_timer_overrides() {
        let mut settings = Settings::default();

        set_timer_resolution_override(&mut settings.timer_resolution, "editor.exe", Some(20_000));
        let summary = process_policy_summary(&settings, &[], "editor.exe");
        assert_eq!(
            summary.timer_resolution,
            timer_resolution::format_resolution_ms(20_000)
        );
        assert!(summary.uses_custom_rule(ProcessListColumn::TimerResolution));

        set_timer_resolution_override(&mut settings.timer_resolution, "editor.exe", None);
        let summary = process_policy_summary(&settings, &[], "editor.exe");
        assert_eq!(summary.timer_resolution, process_list_default_label());
        assert!(!summary.uses_custom_rule(ProcessListColumn::TimerResolution));
    }

    #[test]
    fn process_policy_summary_reports_default_power_plan_when_unset() {
        let mut settings = Settings::default();
        settings.by_foreground.enabled = true;
        settings.by_foreground.rules.push(ByForegroundRule {
            enabled: true,
            name: "Editor".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: None,
        });
        settings.by_running_app.enabled = true;
        settings.by_running_app.rules.push(ByRunningAppRule {
            enabled: true,
            name: "Editor".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: None,
        });

        let summary = process_policy_summary(&settings, &[], "editor.exe");

        assert_eq!(summary.power_plan_foreground, process_list_default_label());
        assert_eq!(summary.power_plan_running, process_list_default_label());
        assert_eq!(summary.timer_resolution, process_list_default_label());
        assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanForeground));
        assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanRunning));
        assert!(!summary.uses_custom_rule(ProcessListColumn::TimerResolution));
    }

    #[test]
    fn process_policy_summary_reports_configured_rules_when_feature_disabled() {
        let mut settings = Settings::default();
        settings.by_foreground.enabled = false;
        settings.by_foreground.rules.push(ByForegroundRule {
            enabled: true,
            name: "Editor".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: Some("balanced-guid".to_owned()),
        });
        settings.by_running_app.enabled = false;
        settings.by_running_app.rules.push(ByRunningAppRule {
            enabled: true,
            name: "Editor".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: Some("performance-guid".to_owned()),
        });
        settings.core_limiter.enabled = false;
        settings.core_limiter.rules.push(CoreLimiterRule {
            enabled: true,
            process_name: "editor.exe".to_owned(),
            threshold_percent: 80,
            sustain_seconds: 5,
            cooldown_seconds: 30,
            max_logical_processors: 25,
        });
        settings.timer_resolution.enabled = false;
        settings.timer_resolution.rules.push(TimerResolutionRule {
            enabled: true,
            process_name: "editor.exe".to_owned(),
            desired_100ns: 10_000,
        });
        let plans = vec![
            PowerPlan {
                guid: "balanced-guid".to_owned(),
                name: "Balanced".to_owned(),
                active: false,
            },
            PowerPlan {
                guid: "performance-guid".to_owned(),
                name: "Performance".to_owned(),
                active: false,
            },
        ];

        let summary = process_policy_summary(&settings, &plans, "editor.exe");

        assert_eq!(summary.power_plan_foreground, "Balanced");
        assert_eq!(summary.power_plan_running, "Performance");
        assert_eq!(
            summary.core_limiter,
            process_list_include_value_label("25%")
        );
        assert_eq!(summary.timer_resolution, "1.00 ms");
        assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanForeground));
        assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanRunning));
        assert!(summary.uses_custom_rule(ProcessListColumn::CoreLimiter));
        assert!(summary.uses_custom_rule(ProcessListColumn::TimerResolution));
    }

    #[test]
    fn cpu_mask_formatter_uses_ranges() {
        assert_eq!(format_cpu_mask(0), t!("common.none").to_string());
        assert_eq!(format_cpu_mask(0b1111), "0-3");
        assert_eq!(format_cpu_mask(0b101101), "0, 2-3, 5");
    }

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

        assert_eq!(core_steering_processors_no_smt_mask(&processors), 0b0101);
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
        let rule = new_core_steering_rule("game.exe");

        assert_eq!(rule.mode, CoreSteeringMode::Soft);
    }

    #[test]
    fn csv_escape_quotes_fields_with_special_characters() {
        let mut escaped = String::new();
        push_csv_field(&mut escaped, "two,parts");
        assert_eq!(escaped.len(), 11);
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("two,parts"), "\"two,parts\"");
        assert_eq!(csv_escape("quoted \"value\""), "\"quoted \"\"value\"\"\"");
        assert_eq!(csv_escape("line\r\nbreak"), "\"line\r\nbreak\"");
    }

    #[test]
    fn timer_resolution_input_accepts_decimal_milliseconds() {
        assert_eq!(
            parse_timer_resolution_input_100ns("1", 10_000, 160_000),
            Some(10_000)
        );
        assert_eq!(
            parse_timer_resolution_input_100ns("0.5 ms", 10_000, 160_000),
            Some(10_000)
        );
        assert_eq!(
            parse_timer_resolution_input_100ns("15.625 MS", 10_000, 160_000),
            Some(160_000)
        );
    }

    #[test]
    fn timer_resolution_input_clamps_to_supported_range() {
        assert_eq!(
            parse_timer_resolution_input_100ns("0.1", 10_000, 160_000),
            Some(10_000)
        );
        assert_eq!(
            parse_timer_resolution_input_100ns("1000", 10_000, 160_000),
            Some(160_000)
        );
    }

    #[test]
    fn action_log_entries_export_as_csv() {
        let entries = vec![ActionLogEntry {
            sequence: 7,
            timestamp_epoch_ms: 1_700_000_000_000,
            feature: ActionLogFeature::CoreLimiter,
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
            ",Core Limiter,42,worker.exe,Fail,Failed,\"Restart failed, access denied\"\r\n"
        ));
    }

    #[test]
    fn action_log_filtering_matches_result_and_feature() {
        let entries = vec![
            ActionLogEntry {
                sequence: 1,
                timestamp_epoch_ms: 1_700_000_000_000,
                feature: ActionLogFeature::CoreLimiter,
                process_id: Some(42),
                process_name: "worker.exe".to_owned(),
                action: ActionLogAction::Fail,
                result: ActionLogResult::Failed,
                reason: "restart failed".to_owned(),
            },
            ActionLogEntry {
                sequence: 2,
                timestamp_epoch_ms: 1_700_000_000_100,
                feature: ActionLogFeature::GpuPriority,
                process_id: Some(43),
                process_name: "game.exe".to_owned(),
                action: ActionLogAction::Apply,
                result: ActionLogResult::Applied,
                reason: "priority applied".to_owned(),
            },
        ];

        let filtered_entries = action_log_filtered_entries(
            &entries,
            ActionLogResultFilter::Failed,
            ActionLogFeatureFilter::Feature(ActionLogFeature::CoreLimiter),
        );

        assert_eq!(filtered_entries.len(), 1);
        assert_eq!(filtered_entries[0].sequence, 1);
    }

    #[test]
    fn action_log_page_count_rounds_up() {
        assert_eq!(action_log_page_count(0), 0);
        assert_eq!(action_log_page_count(1), 1);
        assert_eq!(action_log_page_count(ACTION_LOG_PAGE_SIZE), 1);
        assert_eq!(action_log_page_count(ACTION_LOG_PAGE_SIZE + 1), 2);
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
            ProcessorPowerSlider::AcBoostPolicy.paired_power_source(),
            ProcessorPowerSlider::DcBoostPolicy
        );
        assert_eq!(
            ProcessorPowerSlider::DcCoreParkingMin.paired_power_source(),
            ProcessorPowerSlider::AcCoreParkingMin
        );
        assert_eq!(
            ProcessorPowerSlider::DcBoostPolicy.paired_power_source(),
            ProcessorPowerSlider::AcBoostPolicy
        );
    }

    #[test]
    fn input_hook_is_needed_for_activity_input_or_app_suspension() {
        let mut settings = Settings::default();

        assert!(!input_hook_required(&settings));

        settings.by_activity.power_plans.performance_guid = Some("active-guid".to_owned());
        assert!(input_hook_required(&settings));

        settings.by_activity.enabled = false;
        assert!(!input_hook_required(&settings));

        settings.by_activity.enabled = true;
        settings.general.enabled = false;
        assert!(!input_hook_required(&settings));

        settings.general.enabled = true;
        settings.by_activity.switch_to_performance_on_resume = false;
        assert!(!input_hook_required(&settings));

        settings.by_activity.switch_to_performance_on_resume = true;
        settings.by_activity.input_detection.keyboard = false;
        settings.by_activity.input_detection.mouse = false;
        settings.by_activity.input_detection.controller = true;
        assert!(!input_hook_required(&settings));

        settings.app_suspension.enabled = true;
        assert!(input_hook_required(&settings));

        settings.adaptive_engine.enabled = true;
        assert!(!input_hook_required(&settings));

        settings.adaptive_engine.enabled = false;
        settings.general.enabled = false;
        assert!(!input_hook_required(&settings));
    }

    #[test]
    fn input_hook_config_tracks_enabled_input_devices() {
        let mut settings = Settings::default();

        settings.by_activity.input_detection.keyboard = true;
        settings.by_activity.input_detection.mouse = false;
        assert_eq!(
            input_hook_config(&settings),
            InputHookConfig {
                keyboard: true,
                mouse: false,
            }
        );

        settings.by_activity.input_detection.keyboard = false;
        settings.by_activity.input_detection.mouse = true;
        assert_eq!(
            input_hook_config(&settings),
            InputHookConfig {
                keyboard: false,
                mouse: true,
            }
        );

        settings.by_activity.input_detection.keyboard = false;
        settings.by_activity.input_detection.mouse = false;
        settings.by_activity.input_detection.controller = true;
        assert_eq!(
            input_hook_config(&settings),
            InputHookConfig {
                keyboard: false,
                mouse: false,
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

        settings.adaptive_engine.enabled = true;
        assert_eq!(
            input_hook_config(&settings),
            InputHookConfig {
                keyboard: false,
                mouse: false,
            }
        );
    }

    #[test]
    fn foreground_lookup_runs_only_for_configured_by_foreground() {
        let mut settings = Settings::default();

        assert!(!foreground_lookup_required(&settings));

        settings.by_foreground.enabled = true;
        assert!(!foreground_lookup_required(&settings));

        settings.by_foreground.rules.push(ByForegroundRule {
            enabled: true,
            name: "backup.exe".to_owned(),
            process_name: "backup.exe".to_owned(),
            power_plan_guid: Some("idle-guid".to_owned()),
        });
        assert!(foreground_lookup_required(&settings));
    }
}
