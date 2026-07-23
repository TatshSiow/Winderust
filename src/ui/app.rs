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
    action_log::{ActionLogEntry, ActionLogFeature, ActionLogResult},
    activity::{
        merge_activity_snapshot, ActivitySnapshot, ActivityState, ControllerActivityDetector,
        IdleDetector, InputHook, InputHookConfig,
    },
    app_suspension::{self, AppSuspensionSnapshot},
    automation::{foreground_lookup_required, BackgroundAutomation},
    background_efficiency::{self, BackgroundEfficiencySnapshot},
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
    features::power_plan_control::by_running_app::ByRunningAppSnapshot,
    features::power_plan_control::{
        current_by_time_decision, next_by_time_switch_label, ByCpuLoadScheduler,
    },
    file_dialog::{choose_action_log_export_file, choose_settings_file, FileDialogMode},
    foreground::{
        capture_process_action_target, foreground_process_name, list_process_candidates,
        list_processes, same_process_name, ProcessActionTarget, ProcessActionTargetError,
        ProcessCandidateInfo, ProcessInfo,
    },
    gpu_priority::{self, GpuPrioritySnapshot},
    io_priority::{self, IoPrioritySnapshot},
    memory_priority::{self, MemoryPrioritySnapshot},
    memory_trim::{self, MemoryTrimSnapshot},
    power::{
        active_plan, apply_processor_power_values, list_plans, read_plan_personality,
        read_processor_power_values, restore_stale_adaptive_plans, set_active, EffectivePowerMode,
        EffectivePowerModeMonitor, PowerPlan, PowerPlanPersonality, ProcessorBoostMode,
        ProcessorPowerAcDcValues, ProcessorPowerPreset, ProcessorPowerValues,
    },
    power_source, privilege,
    process_icon::load_process_icon,
    process_priority::{self, ProcessPrioritySnapshot},
    rules::{
        decide, ByRunningAppDecision, DecisionInput, DecisionOutcome, DecisionState,
        MAX_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD, MIN_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD,
    },
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

mod list_removal;
mod navigation_state;
mod pages;
mod process_refresh;
mod runtime;
mod settings_io;
mod shared;
mod tray_state;
mod update_check;

use pages::*;
use shared::*;

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
    by_cpu_load_scheduler: ByCpuLoadScheduler,
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

fn default_processor_power_values() -> ProcessorPowerAcDcValues {
    ProcessorPowerAcDcValues::same(ProcessorPowerValues::for_preset(
        ProcessorPowerPreset::Balanced,
    ))
    .normalized()
}

fn load_initial_processor_power_state() -> InitialProcessorPowerState {
    let fallback_values = default_processor_power_values();

    match list_plans() {
        Ok(plans) => {
            let current_plan = plans.iter().find(|plan| plan.active).cloned();
            let target_plan = current_plan.as_ref().or_else(|| plans.first()).cloned();
            let status_loaded = t!("status.loaded_power_plans", count = plans.len()).to_string();
            let target_plan_personality = target_plan
                .as_ref()
                .and_then(|plan| read_plan_personality(&plan.guid).ok());

            let (values, loaded_plan_guid, status_message) = match target_plan.as_ref() {
                Some(plan) => match read_processor_power_values(&plan.guid) {
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
        let adaptive_plan_recovery_error = restore_stale_adaptive_plans().err();
        let background_automation = BackgroundAutomation::start(&settings);
        apply_language(settings.general.language);
        apply_appearance_settings(&settings.general, window, cx);
        let effective_power_mode_monitor = EffectivePowerModeMonitor::new().ok();
        let effective_power_mode = effective_power_mode_monitor
            .as_ref()
            .map(EffectivePowerModeMonitor::snapshot)
            .unwrap_or(EffectivePowerMode::Unknown);
        let mut initial_processor_power = load_initial_processor_power_state();
        if let Some(error) = adaptive_plan_recovery_error {
            initial_processor_power.status_message =
                format!("Adaptive power-plan recovery failed: {error}");
        }
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
                power_plan_guid: None,
                state: DecisionState::NoPowerPlanSelected,
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
            by_cpu_load_scheduler: ByCpuLoadScheduler::default(),
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
}
