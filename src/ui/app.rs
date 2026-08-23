use std::{
    cell::RefCell,
    cmp::Ordering as CmpOrdering,
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
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
    canvas, deferred, div, img, percentage, prelude::*, px, relative, rgb, rgba, size, Animation,
    AnimationExt, AnyElement, App, Bounds, Context, DragMoveEvent, Empty, Entity, EntityId,
    FocusHandle, Focusable, Hsla, Image, IntoElement, MouseButton, NavigationDirection, Pixels,
    Point, PromptButton, PromptLevel, Render, ScrollAnchor, ScrollHandle, SharedString,
    Subscription, Task, Timer, Window, WindowControlArea,
};
use gpui_component::{
    animation::cubic_bezier,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    chart::AreaChart,
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    h_flex,
    input::{Escape as InputEscape, Input, InputEvent, InputState},
    label::Label,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    scroll::{Scrollable, ScrollableElement, Scrollbar},
    slider::{SliderEvent, SliderState, SliderValue},
    theme::Colorize,
    tooltip::Tooltip,
    v_flex, v_virtual_list, ActiveTheme, Disableable, Icon, IconName, IconNamed, Sizable,
    VirtualListScrollHandle,
};

use crate::{
    action_log::{ActionLogEntry, ActionLogFeature, ActionLogResult, ActionLogSummaries},
    activity::{
        merge_activity_snapshot, ActivitySnapshot, ActivityState, ControllerActivityDetector,
        IdleDetector,
    },
    app_suspension::{self, AppSuspensionSnapshot},
    application::{
        AdvancedPowerPlanTuningService, NavigationCollapsedPatch, SettingsEditor,
        Win32PrioritySeparationError, Win32PrioritySeparationService,
        Win32PrioritySeparationSnapshot,
    },
    automation::{
        ProcessControlActionReceiver, RuntimeCommandError, RuntimeFeatureStatus, RuntimeHandle,
    },
    background_efficiency,
    config::{
        self, AccentColorSource, AccentSettings, ActionLogMode, AdaptiveEnginePreset,
        AdvancedPowerPlanTuningPreset, AnimationMode, AppLanguage, AppSuspensionRule,
        AppSuspensionSettings, AppThemeMode, BackgroundEfficiencyAggressiveness,
        BackgroundEfficiencyRule, BackgroundEfficiencySettings, BackgroundProcessorSelection,
        ByCpuLoadRule, ByForegroundRule, ByForegroundSettings, ByRunningAppRule,
        ByRunningAppSettings, ByTimeRule, CoreLimiterRule, CoreLimiterSettings,
        CpuAllocationMethod, CpuAllocationRule, CpuSchedulerSettings, CpuUsageComparison,
        DynamicPriorityBoostSettings, GpuPrioritySettings, IoPrioritySettings,
        MemoryPrioritySettings, MemoryTrimSettings, NetworkThresholdUnit, PowerSourceProfile,
        ProcessDynamicPriorityBoostSetting, ProcessExclusionRule, ProcessGpuPriority,
        ProcessGpuPrioritySetting, ProcessIoPriority, ProcessIoPrioritySetting,
        ProcessMemoryPriority, ProcessMemoryPrioritySetting, ProcessPrioritySetting,
        ProcessPrioritySettings, ProcessThreadPrioritySetting, Settings, ThreadPrioritySettings,
        TimerResolutionRule, TimerResolutionSettings, UpdateChannel, WeekdaySetting,
        CHECK_INTERVAL_MAX_MS, CHECK_INTERVAL_MIN_MS, CPU_SCHEDULER_REACTION_INTERVAL_MAX_MS,
        CPU_SCHEDULER_REACTION_INTERVAL_MIN_MS,
    },
    control::{
        dynamic_priority_boost::{current_dynamic_priority_boost_state, DynamicPriorityBoostState},
        gpu_priority::current_process_gpu_priority,
        io_priority::current_process_io_priority,
        memory_priority::current_process_memory_priority,
        power_plan::PowerPlanStatus,
        priority_efficiency::{current_efficiency_mode, current_process_priority},
        thread_priority::current_process_thread_priority,
    },
    core_limiter::{self, CoreLimiterSnapshot},
    cpu::{process_cpu_usage_percent, CpuUsageMonitor, CpuUsageSnapshot},
    cpu_allocation::{self, LogicalProcessorInfo, LogicalProcessorKind},
    cpu_scheduler, crash_recovery,
    dashboard_metrics::{
        sample_memory_usage, IoUsageMonitor, IoUsageSnapshot, MemoryUsageSnapshot,
        NetworkUsageMonitor, NetworkUsageSnapshot,
    },
    dynamic_priority_boost,
    features::power_plan_control::next_by_time_switch_label,
    file_dialog::{
        choose_action_log_export_file, choose_executable_file, choose_settings_file, FileDialogMode,
    },
    foreground::{
        capture_process_action_target, capture_process_action_target_for_owned_release,
        contains_process_name, ensure_process_action_target_access, executable_path_key,
        list_process_candidates, list_processes_with_paths, open_process_location,
        process_candidates_from_processes, process_tree_action_targets, same_executable_path,
        sample_process_resources, ProcessActionAccess, ProcessActionTarget,
        ProcessActionTargetError, ProcessCandidateInfo, ProcessInfo, ProcessResourceSample,
        CORE_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    gpu_priority, io_priority, memory_priority, memory_trim,
    power::{
        active_plan, list_plans, AdaptivePowerBoostValues, EffectivePowerMode,
        EffectivePowerModeMonitor, PowerPlan, PowerPlanPersonality, ProcessorBoostMode,
        ProcessorPowerPreset, ProcessorPowerSourceValues, ProcessorPowerValues,
    },
    privilege,
    process_icon::load_process_icon,
    process_priority,
    rules::{
        MAX_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD, MIN_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD,
    },
    thread_priority, timer_resolution,
    tray::{self, TrayIcon},
    ui::{self, Page},
    update_checker::{self, AvailableUpdate},
    win_registry::{read_registry_binary_root, read_registry_dword_root},
};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Registry::HKEY_CURRENT_USER;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
};

mod dashboard_model;
mod list_removal;
mod navigation_state;
mod pages;
mod process_models;
mod process_refresh;
pub(in crate::ui::app) use process_refresh::process_load_state_message;
mod runtime;
mod settings_io;
mod shared;
mod shell_model;
mod tray_state;
mod update_check;
mod update_model;

use dashboard_model::DashboardModel;
use pages::*;
use process_models::{ProcessCatalogModel, ProcessListModel};
use shared::*;
use shell_model::ShellModel;
use update_model::{UpdateModalDismissal, UpdateModel};

const ACTIVE_PLAN_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const APP_TICK_INTERVAL: Duration = Duration::from_secs(1);
const CPU_USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const DASHBOARD_IO_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const TIMER_RESOLUTION_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const DASHBOARD_HISTORY_LEN: usize = 30;
const DASHBOARD_SUMMARY_CARD_HEIGHT: f32 = 196.0;
const DASHBOARD_LINE_CHART_HEIGHT: f32 = 112.0;
const DASHBOARD_LINE_CHART_TICK_MARGIN: usize = DASHBOARD_HISTORY_LEN + 1;
const DASHBOARD_PERCENT_CHART_MAX: f64 = 100.0;
const DASHBOARD_SPLIT_ITEM_WIDTH: f32 = 140.0;
const DASHBOARD_SPLIT_VALUE_WIDTH: f32 = 90.0;
const CARD_ROW_HEIGHT: f32 = 58.0;
const CORE_TILE_GRID_COLUMNS: usize = 8;
const CORE_TILE_HEIGHT: f32 = 54.0;
const EXPANDED_CHILD_MAX_ANIMATION_HEIGHT: f32 = 1800.0;
const EXPANDED_CHILD_SLIDE_PX: f32 = 8.0;
const MOTION_CONTROL_SECONDS: f64 = 0.18;
const MOTION_CONTROL_MIN_SECONDS: f64 = 0.08;
const MOTION_CONTROL_FRAME_INTERVAL: Duration = Duration::from_millis(16);
const MOTION_FAST_SECONDS: f64 = 0.15;
const MOTION_STANDARD_SECONDS: f64 = 0.22;
const MOTION_EXPAND_SECONDS: f64 = 0.24;
const MOTION_EXPAND_MIN_SECONDS: f64 = 0.1;
const PROCESS_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const PROCESS_LIST_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const TITLE_BAR_HEIGHT: f32 = 40.0;
const TITLE_BAR_CONTROL_WIDTH: f32 = 46.0;
const TITLE_BAR_CONTROL_ICON_SIZE: f32 = 12.0;
const TITLE_BAR_CONTROL_ICON_LINE_HEIGHT: f32 = 12.0;
const PAGE_HEADER_HEIGHT: f32 = 48.0;
const PAGE_CONTENT_VERTICAL_PADDING: f32 = 24.0;
const CONTENT_MAX_WIDTH: f32 = 1040.0;
const NAV_PANE_WIDTH: f32 = 276.0;
const NAV_PANE_COMPACT_WIDTH: f32 = 64.0;

const fn navigation_pane_width(collapsed: bool) -> f32 {
    if collapsed {
        NAV_PANE_COMPACT_WIDTH
    } else {
        NAV_PANE_WIDTH
    }
}

fn navigation_pane_width_at_progress(progress: f32) -> f32 {
    NAV_PANE_COMPACT_WIDTH + (NAV_PANE_WIDTH - NAV_PANE_COMPACT_WIDTH) * progress
}
const BRAND_RADIUS_CONTROL: f32 = 5.0;
const BRAND_RADIUS_SURFACE: f32 = 7.0;
const BRAND_RADIUS_OVERLAY: f32 = 8.0;
const FONT_UI: &str = "Bahnschrift";
const FONT_BRAND: &str = "Bahnschrift";
const FONT_WINDOW_CONTROLS: &str = "Segoe Fluent Icons";
const PROCESS_PICKER_LAYER_PRIORITY: usize = 2;
const DROPDOWN_OPTION_ROW_HEIGHT: f32 = 40.0;
const DROPDOWN_CONTROL_HEIGHT: f32 = 32.0;
const DROPDOWN_SELECT_COMPACT_WIDTH: f32 = 136.0;
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
const MAX_NETWORK_THRESHOLD_BYTES: u64 = 1_000_000_000;
const ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS: u64 = 1;
const ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS: u64 = 60 * 60;
const ACTIVITY_CHECK_INTERVAL_STEP_MS: u64 = 250;
const TIMER_RESOLUTION_INPUT_MIN_MS: f64 = 0.1;
const TIMER_RESOLUTION_INPUT_MAX_MS: f64 = 1000.0;
const CPU_SCHEDULER_THRESHOLD_MIN_PERCENT: u64 = 1;
const CPU_SCHEDULER_THRESHOLD_MAX_PERCENT: u64 = 100;
const CPU_SCHEDULER_SECONDS_MIN: u64 = 1;
const CPU_SCHEDULER_SECONDS_MAX: u64 = 3_600;
const CPU_SCHEDULER_TARGET_LIMIT_MIN: u64 = 1;
const CPU_SCHEDULER_TARGET_LIMIT_MAX: u64 = 64;
const WIN32_PRIORITY_SEPARATION_WINDOWS_DEFAULT: u32 = 0x26;
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

struct ProcessCandidate {
    name: String,
    image_path: PathBuf,
    has_suspendable_instance: bool,
    icon: Option<Arc<Image>>,
}

#[derive(Debug, Clone, PartialEq)]
struct ProcessPolicySummary {
    status: String,
    cpu_percent: Option<f32>,
    memory_bytes: Option<u64>,
    power_plan_foreground: String,
    power_plan_running: String,
    adaptive_engine: String,
    background_efficiency: String,
    process_priority: String,
    thread_priority: String,
    dynamic_priority_boost: String,
    io_priority: String,
    gpu_priority: String,
    memory_priority: String,
    custom_columns: HashSet<ProcessListColumn>,
    active_columns: HashSet<ProcessListColumn>,
}

impl ProcessPolicySummary {
    fn mark_custom(&mut self, column: ProcessListColumn) {
        self.custom_columns.insert(column);
    }

    fn uses_custom_rule(&self, column: ProcessListColumn) -> bool {
        self.custom_columns.contains(&column)
    }

    fn set_active(&mut self, column: ProcessListColumn, active: bool) {
        if active {
            self.active_columns.insert(column);
        } else {
            self.active_columns.remove(&column);
        }
    }

    fn value_is_active(&self, column: ProcessListColumn) -> bool {
        self.active_columns.contains(&column)
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum ProcessLoadState {
    #[default]
    Loading,
    Loaded,
    Failed(String),
    Paused,
}

#[derive(Clone)]
struct ProcessDetailsDraft {
    display_name: String,
    executable_path: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PresetSidePanelTab {
    Status,
    #[default]
    Presets,
}

#[derive(Clone, Copy, Debug, Default)]
struct ProcessResourceUsage {
    cpu_percent: Option<f32>,
    working_set_bytes: Option<u64>,
    efficiency_mode: Option<bool>,
}

struct SettingsIoToast {
    title: String,
    message: String,
    success: bool,
    shown_at: Instant,
    closing: bool,
}

struct TabContentTransition {
    target: String,
    generation: u64,
    started_at: Instant,
    from_x: f32,
}

pub struct WinderustApp {
    settings: SettingsEditor,
    shell: ShellModel,
    editing_power_source_profile: PowerSourceProfile,
    plans: Vec<PowerPlan>,
    current_plan: Option<PowerPlan>,
    activity: ActivitySnapshot,
    dashboard: DashboardModel,
    feature_status: Arc<RuntimeFeatureStatus>,
    power_plan_status: Arc<PowerPlanStatus>,
    action_log_entries: Arc<Vec<ActionLogEntry>>,
    action_log_summaries: Arc<ActionLogSummaries>,
    last_appearance_change_generation: u64,
    last_runtime_status_generation: u64,
    last_auto_exclusion_patch_generation: u64,
    action_log_result_filter: ActionLogResultFilter,
    action_log_feature_filter: ActionLogFeatureFilter,
    action_log_page: usize,
    next_schedule: String,
    next_check: Instant,
    next_active_plan_refresh: Instant,
    next_cpu_usage_refresh: Instant,
    next_dashboard_io_refresh: Instant,
    next_timer_resolution_status_refresh: Instant,
    next_process_refresh: Instant,
    effective_power_mode_monitor: Option<EffectivePowerModeMonitor>,
    effective_power_mode: EffectivePowerMode,
    runtime_handle: RuntimeHandle,
    cpu_monitor: CpuUsageMonitor,
    io_monitor: IoUsageMonitor,
    network_monitor: NetworkUsageMonitor,
    idle_detector: IdleDetector,
    controller_activity_detector: ControllerActivityDetector,
    tray_hide_on_close: bool,
    hwnd: Option<HWND>,
    tray_icon: Option<TrayIcon>,
    tray_install_failed_for: Option<(bool, bool)>,
    status_message: String,
    process_catalog: ProcessCatalogModel,
    process_list: ProcessListModel,
    app_icon: Option<Arc<Image>>,
    active_power_plan_picker: Option<String>,
    advanced_power_plan_tuning_service: AdvancedPowerPlanTuningService,
    processor_power_ac_core_parking_min: u64,
    processor_power_ac_performance_min: u64,
    processor_power_ac_performance_max: u64,
    processor_power_ac_boost_policy: u64,
    processor_power_ac_boost_mode: ProcessorBoostMode,
    processor_power_battery_core_parking_min: u64,
    processor_power_battery_performance_min: u64,
    processor_power_battery_performance_max: u64,
    processor_power_battery_boost_policy: u64,
    processor_power_battery_boost_mode: ProcessorBoostMode,
    processor_power_target_plan_guid: Option<String>,
    processor_power_loaded_plan_guid: Option<String>,
    processor_power_target_plan_personality: Option<PowerPlanPersonality>,
    processor_power_dirty: bool,
    win32_priority_separation_service: Win32PrioritySeparationService,
    win32_priority_separation_value: Option<u32>,
    win32_priority_separation_edit_value: u32,
    win32_priority_separation_backup: Option<u32>,
    win32_priority_separation_status: String,
    start_minimized_applied: bool,
    editing_rule_title: Option<RuleTitleTarget>,
    adaptive_engine_side_panel_tab: PresetSidePanelTab,
    adaptive_engine_tuning_tab: AdaptiveEngineTuningTab,
    cpu_allocation_side_panel_tab: PresetSidePanelTab,
    side_panel_collapsed: bool,
    side_panel_visible: bool,
    retained_side_panel_page: Option<Page>,
    editing_numeric: Option<NumericField>,
    adaptive_engine_preset_editor: Option<AdaptiveEnginePresetEditor>,
    cpu_allocation_preset_editor: Option<CpuAllocationPresetEditor>,
    advanced_power_plan_tuning_preset_editor: Option<AdvancedPowerPlanTuningPresetEditor>,
    expanded_rule_cards: HashSet<RuleCardTarget>,
    expanded_setting_groups: HashSet<SettingGroupTarget>,
    update: UpdateModel,
    about_updates_focus_handle: FocusHandle,
    about_page_scroll_handle: ScrollHandle,
    about_updates_scroll_anchor: ScrollAnchor,
    unsaved_popup_was_visible: bool,
    unsaved_popup_vanish_started: Option<Instant>,
    settings_io_toast: Option<SettingsIoToast>,
    tab_content_transition: Option<TabContentTransition>,
    tab_content_transition_generation: u64,
    pending_list_item_removals: HashMap<ListItemRemovalTarget, Instant>,
    dropdown_anchor_bounds: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    accent_color_picker: Entity<ColorPickerState>,
    _rule_title_input_subscriptions: Vec<Subscription>,
    _process_picker_input_subscriptions: Vec<Subscription>,
    _numeric_input_subscription: Option<Subscription>,
    _dashboard_search_subscription: Option<Subscription>,
    _process_list_search_subscription: Option<Subscription>,
    _adaptive_engine_preset_name_subscription: Option<Subscription>,
    _cpu_allocation_preset_name_subscription: Option<Subscription>,
    _advanced_power_plan_tuning_preset_name_subscription: Option<Subscription>,
    _processor_power_slider_subscriptions: Vec<Subscription>,
    _cpu_threshold_slider_subscriptions: Vec<Subscription>,
    _activity_slider_subscriptions: Vec<Subscription>,
    _accent_color_picker_subscription: Subscription,
    _window_activation_subscription: Subscription,
    _shutdown_subscription: Option<Subscription>,
    shutdown_started: bool,
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
    BackgroundEfficiencyRule,
    AppSuspensionRule,
    CpuSetsSoftRule,
    ProcessorAffinityHardRule,
    AdaptiveEnginePreset,
    CpuAllocationPreset,
    AdvancedPowerPlanTuningPreset,
    CoreLimiterRule,
    ByRunningAppRule,
    CpuSchedulerCustomRule,
    ProcessPriorityExclusion,
    ThreadPriorityExclusion,
    DynamicPriorityBoostExclusion,
    IoPriorityExclusion,
    GpuPriorityExclusion,
    MemoryPriorityExclusion,
    TimerResolutionRule,
    MemoryTrimExclusion,
}

#[derive(Clone, Copy)]
struct CpuAllocationPresetEditor {
    target: CpuAllocationPresetEditorTarget,
    core_mask: u64,
}

#[derive(Clone)]
struct AdaptiveEnginePresetEditor {
    target: AdaptiveEnginePresetEditorTarget,
    preset: AdaptiveEnginePreset,
    tuning_tab: AdaptiveEngineTuningTab,
}

#[derive(Clone, Copy)]
enum AdaptiveEnginePresetEditorTarget {
    BuiltIn(BuiltInAdaptiveEnginePreset),
    Custom(Option<usize>),
}

#[derive(Clone, Copy)]
enum CpuAllocationPresetEditorTarget {
    Core(usize),
    Custom(Option<usize>),
}

struct AdvancedPowerPlanTuningPresetEditor {
    target: AdvancedPowerPlanTuningPresetEditorTarget,
    values: ProcessorPowerValues,
    sliders: [Entity<SliderState>; 4],
    _slider_subscriptions: Vec<Subscription>,
}

#[derive(Clone, Copy)]
enum AdvancedPowerPlanTuningPresetEditorTarget {
    BuiltIn(ProcessorPowerPreset),
    Custom(Option<usize>),
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
    process_list_search: Entity<InputState>,
    by_cpu_load_rule_names: Vec<Entity<InputState>>,
    cpu_rule_thresholds: Vec<Entity<SliderState>>,
    cpu_rule_upper_thresholds: Vec<Entity<SliderState>>,
    by_time_rule_names: Vec<Entity<InputState>>,
    schedule_start_times: Vec<Entity<InputState>>,
    schedule_end_times: Vec<Entity<InputState>>,
    foreground_process: Entity<InputState>,
    background_efficiency_process: Entity<InputState>,
    memory_trim_exclusion: Entity<InputState>,
    app_suspension_process: Entity<InputState>,
    core_limiter_process: Entity<InputState>,
    performance_process: Entity<InputState>,
    cpu_sets_soft_process: Entity<InputState>,
    processor_affinity_hard_process: Entity<InputState>,
    adaptive_engine_preset_name: Entity<InputState>,
    cpu_allocation_preset_name: Entity<InputState>,
    advanced_power_plan_tuning_preset_name: Entity<InputState>,
    cpu_scheduler_process: Entity<InputState>,
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
    processor_power_battery_core_parking_min: Entity<SliderState>,
    processor_power_battery_performance_min: Entity<SliderState>,
    processor_power_battery_performance_max: Entity<SliderState>,
    processor_power_battery_boost_policy: Entity<SliderState>,
}

struct InitialProcessorPowerState {
    plans: Vec<PowerPlan>,
    current_plan: Option<PowerPlan>,
    values: ProcessorPowerSourceValues,
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

fn default_processor_power_values() -> ProcessorPowerSourceValues {
    ProcessorPowerSourceValues::same(ProcessorPowerValues::for_preset(
        ProcessorPowerPreset::Balanced,
    ))
    .normalized()
}

fn load_initial_processor_power_state(
    service: &AdvancedPowerPlanTuningService,
) -> InitialProcessorPowerState {
    let fallback_values = default_processor_power_values();

    match list_plans() {
        Ok(plans) => {
            let current_plan = plans.iter().find(|plan| plan.active).cloned();
            let target_plan = current_plan.as_ref().or_else(|| plans.first()).cloned();
            let status_loaded = t!("status.loaded_power_plans", count = plans.len()).to_string();
            let target_plan_personality = target_plan
                .as_ref()
                .and_then(|plan| service.read_personality(&plan.guid).ok());

            let (values, loaded_plan_guid, status_message) = match target_plan.as_ref() {
                Some(plan) => match service.read_values(&plan.guid) {
                    Ok(values) => (values.normalized(), Some(plan.guid.clone()), status_loaded),
                    Err(error) => (fallback_values, None, error.to_string()),
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
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        settings: SettingsEditor,
        settings_load_error: Option<String>,
        runtime_handle: RuntimeHandle,
    ) -> Self {
        let hwnd = tray::hwnd_from_window(window);
        let window_activation_subscription =
            cx.observe_window_activation(window, |app, window, cx| {
                if window.is_window_active() && tray::take_restore_requested() {
                    app.refresh_after_tray_restore(window, cx);
                }
            });
        apply_language(settings.general.language);
        apply_appearance_settings(&settings.general, window, cx);
        let effective_power_mode_monitor = EffectivePowerModeMonitor::new().ok();
        let effective_power_mode = effective_power_mode_monitor
            .as_ref()
            .map(EffectivePowerModeMonitor::snapshot)
            .unwrap_or(EffectivePowerMode::Unknown);
        let advanced_power_plan_tuning_service = AdvancedPowerPlanTuningService::default();
        let mut initial_processor_power =
            load_initial_processor_power_state(&advanced_power_plan_tuning_service);
        if let Some(error) = settings_load_error {
            initial_processor_power.status_message = error;
        }
        if let Some(error) = crash_recovery::startup_error() {
            initial_processor_power.status_message = error;
        }
        let inputs = UiInputs::new(window, cx, &settings, initial_processor_power.values);
        let initial_process_load_state = if settings.advanced.pause_process_population {
            ProcessLoadState::Paused
        } else {
            ProcessLoadState::Loading
        };
        let win32_priority_separation_service = Win32PrioritySeparationService::default();
        let (
            win32_priority_separation_value,
            win32_priority_separation_backup,
            win32_priority_separation_status,
        ) = win32_priority_separation_snapshot_state(win32_priority_separation_service.snapshot());
        let win32_priority_separation_edit_value = win32_priority_separation_value
            .map(normalize_win32_priority_separation_value)
            .unwrap_or(WIN32_PRIORITY_SEPARATION_WINDOWS_DEFAULT);
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
        let about_page_scroll_handle = ScrollHandle::new();
        let about_updates_scroll_anchor =
            ScrollAnchor::for_handle(about_page_scroll_handle.clone());
        let mut app = Self {
            settings,
            shell: ShellModel::new(Page::Home),
            editing_power_source_profile: if crate::backend::power_source::is_plugged_in()
                == Some(false)
            {
                PowerSourceProfile::OnBattery
            } else {
                PowerSourceProfile::PluggedIn
            },
            plans: initial_processor_power.plans,
            current_plan: initial_processor_power.current_plan,
            activity: ActivitySnapshot {
                state: ActivityState::Unknown,
                idle_for: None,
            },
            dashboard: DashboardModel::new(),
            feature_status: Arc::new(RuntimeFeatureStatus {
                timer_resolution: initial_timer_resolution_status,
                ..Default::default()
            }),
            power_plan_status: Arc::new(PowerPlanStatus::default()),
            action_log_entries: Arc::new(Vec::new()),
            action_log_summaries: Arc::new(ActionLogSummaries::new()),
            last_appearance_change_generation: 0,
            last_runtime_status_generation: 0,
            last_auto_exclusion_patch_generation: 0,
            action_log_result_filter: ActionLogResultFilter::All,
            action_log_feature_filter: ActionLogFeatureFilter::All,
            action_log_page: 0,
            next_schedule: t!("status.no_active_time_rules").to_string(),
            next_check: Instant::now(),
            next_active_plan_refresh: Instant::now(),
            next_cpu_usage_refresh: Instant::now(),
            next_dashboard_io_refresh: Instant::now(),
            next_timer_resolution_status_refresh: Instant::now(),
            next_process_refresh: Instant::now(),
            effective_power_mode_monitor,
            effective_power_mode,
            runtime_handle,
            cpu_monitor: CpuUsageMonitor::default(),
            io_monitor: IoUsageMonitor::default(),
            network_monitor: NetworkUsageMonitor::default(),
            idle_detector: IdleDetector,
            controller_activity_detector: ControllerActivityDetector::default(),
            tray_hide_on_close: false,
            hwnd,
            tray_icon: None,
            tray_install_failed_for: None,
            status_message: initial_processor_power.status_message,
            process_catalog: ProcessCatalogModel::new(initial_process_load_state.clone()),
            process_list: ProcessListModel::new(initial_process_load_state),
            app_icon,
            active_power_plan_picker: None,
            advanced_power_plan_tuning_service,
            processor_power_ac_core_parking_min: initial_processor_power.values.ac.core_parking_min
                as u64,
            processor_power_ac_performance_min: initial_processor_power.values.ac.performance_min
                as u64,
            processor_power_ac_performance_max: initial_processor_power.values.ac.performance_max
                as u64,
            processor_power_ac_boost_policy: initial_processor_power.values.ac.boost_policy as u64,
            processor_power_ac_boost_mode: initial_processor_power.values.ac.boost_mode,
            processor_power_battery_core_parking_min: initial_processor_power
                .values
                .battery
                .core_parking_min as u64,
            processor_power_battery_performance_min: initial_processor_power
                .values
                .battery
                .performance_min as u64,
            processor_power_battery_performance_max: initial_processor_power
                .values
                .battery
                .performance_max as u64,
            processor_power_battery_boost_policy: initial_processor_power
                .values
                .battery
                .boost_policy as u64,
            processor_power_battery_boost_mode: initial_processor_power.values.battery.boost_mode,
            processor_power_target_plan_guid: initial_processor_power.target_plan_guid,
            processor_power_loaded_plan_guid: initial_processor_power.loaded_plan_guid,
            processor_power_target_plan_personality: initial_processor_power
                .target_plan_personality,
            processor_power_dirty: false,
            win32_priority_separation_service,
            win32_priority_separation_value,
            win32_priority_separation_edit_value,
            win32_priority_separation_backup,
            win32_priority_separation_status,
            start_minimized_applied: false,
            editing_rule_title: None,
            adaptive_engine_side_panel_tab: PresetSidePanelTab::default(),
            adaptive_engine_tuning_tab: AdaptiveEngineTuningTab::default(),
            cpu_allocation_side_panel_tab: PresetSidePanelTab::default(),
            side_panel_collapsed: false,
            side_panel_visible: false,
            retained_side_panel_page: None,
            editing_numeric: None,
            adaptive_engine_preset_editor: None,
            cpu_allocation_preset_editor: None,
            advanced_power_plan_tuning_preset_editor: None,
            expanded_rule_cards: HashSet::new(),
            expanded_setting_groups: HashSet::new(),
            update: UpdateModel::new(),
            about_updates_focus_handle: cx.focus_handle(),
            about_page_scroll_handle,
            about_updates_scroll_anchor,
            unsaved_popup_was_visible: false,
            unsaved_popup_vanish_started: None,
            settings_io_toast: None,
            tab_content_transition: None,
            tab_content_transition_generation: 0,
            pending_list_item_removals: HashMap::new(),
            dropdown_anchor_bounds: Rc::new(RefCell::new(HashMap::new())),
            accent_color_picker,
            _rule_title_input_subscriptions: Vec::new(),
            _process_picker_input_subscriptions: Vec::new(),
            _numeric_input_subscription: None,
            _dashboard_search_subscription: None,
            _process_list_search_subscription: None,
            _adaptive_engine_preset_name_subscription: None,
            _cpu_allocation_preset_name_subscription: None,
            _advanced_power_plan_tuning_preset_name_subscription: None,
            _processor_power_slider_subscriptions: Vec::new(),
            _cpu_threshold_slider_subscriptions: Vec::new(),
            _activity_slider_subscriptions: Vec::new(),
            _accent_color_picker_subscription: accent_color_picker_subscription,
            _window_activation_subscription: window_activation_subscription,
            _shutdown_subscription: None,
            shutdown_started: false,
            inputs,
            _tick_task: Task::ready(()),
        };

        app._shutdown_subscription = Some(cx.on_app_quit(|app, _| {
            if let Err(error) = app.shutdown() {
                app.status_message = error;
            }
            async {}
        }));
        app.rebuild_rule_title_input_subscriptions(window, cx);
        app.rebuild_process_picker_input_subscriptions(window, cx);
        app.subscribe_to_numeric_input(window, cx);
        app.subscribe_to_dashboard_search_input(window, cx);
        app.subscribe_to_process_list_search_input(window, cx);
        app.subscribe_to_adaptive_engine_preset_name_input(window, cx);
        app.subscribe_to_cpu_allocation_preset_name_input(window, cx);
        app.subscribe_to_advanced_power_plan_tuning_preset_name_input(window, cx);
        app.subscribe_to_processor_power_sliders(window, cx);
        app.rebuild_cpu_threshold_slider_subscriptions(window, cx);
        app.subscribe_to_activity_sliders(window, cx);
        window.on_window_should_close(cx, |_, _| !tray::is_hidden_to_tray());
        app.sync_tray_icon();
        app.run_check(Instant::now());
        app.sync_processor_power_slider_states(window, cx);
        if app.settings.persisted().general.check_for_updates {
            app.check_for_updates(false, cx);
        }
        app.schedule_tick(window, cx);
        app
    }
}
impl Drop for WinderustApp {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

impl WinderustApp {
    fn shutdown(&mut self) -> Result<(), String> {
        if self.shutdown_started {
            return Ok(());
        }
        self.shutdown_started = true;

        self.runtime_handle.shutdown()
    }
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
        self.clear_finished_tab_content_motion();

        let search_query = self.dashboard_search_query(cx);
        let search_active = !search_query.is_empty();
        let side_panel = self.render_animated_side_panel(search_active, cx);
        let page_body = if search_active {
            self.render_search_results_page(&search_query, cx)
        } else {
            self.render_page(window, cx)
        };
        let page_body = if !search_active && self.shell.page.supports_power_source_profiles() {
            let profile = self.editing_power_source_profile;
            let target = format!("power-source-{:?}-{profile:?}", self.shell.page);
            self.animated_tab_content(page_body, &target)
        } else {
            page_body
        };
        let page_header = if search_active {
            search_results_page_header(cx).into_any_element()
        } else {
            self.page_header(self.shell.page, cx).into_any_element()
        };
        let page_uses_inner_scroll = !search_active && self.shell.page == Page::ProcessList;
        let unsaved = self.has_pending_changes();
        let unsaved_popup_vanish_progress = self.unsaved_popup_vanish_progress(unsaved, window);
        let show_unsaved_popup = unsaved || unsaved_popup_vanish_progress.is_some();
        let page_content = animated_page_content_frame(
            page_content_frame(page_header, page_body, page_uses_inner_scroll),
            self.active_breadcrumb_transition(self.shell.page),
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
        } else if self.shell.page == Page::About {
            v_flex()
                .id("about-page-scroll")
                .flex_1()
                .h_full()
                .min_w(px(0.0))
                .min_h(px(0.0))
                .overflow_y_scroll()
                .track_scroll(&self.about_page_scroll_handle)
                .vertical_scrollbar(&self.about_page_scroll_handle)
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
                if app.update.startup_modal_visible {
                    app.dismiss_startup_update_modal(cx);
                } else if app.advanced_power_plan_tuning_preset_editor.is_some() {
                    app.close_advanced_power_plan_tuning_preset_editor(cx);
                } else if app.adaptive_engine_preset_editor.is_some() {
                    app.close_adaptive_engine_preset_editor(cx);
                } else if app.cpu_allocation_preset_editor.is_some() {
                    app.close_cpu_allocation_preset_editor(cx);
                } else if app.process_list.details.is_some() {
                    app.close_process_details(cx);
                } else {
                    clear_input(&app.inputs.dashboard_search, window, cx);
                }
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
                    )
                    .children(side_panel),
            )
            .child(if show_unsaved_popup {
                self.render_unsaved_popup(unsaved_popup_vanish_progress, cx)
                    .into_any_element()
            } else {
                div().into_any_element()
            })
            .child(self.render_settings_io_toast(cx))
            .child(if self.process_list.details.is_some() {
                self.render_process_details_modal(window, cx)
            } else {
                div().into_any_element()
            })
            .child(if self.cpu_allocation_preset_editor.is_some() {
                self.render_cpu_allocation_preset_modal(window, cx)
            } else {
                div().into_any_element()
            })
            .child(if self.adaptive_engine_preset_editor.is_some() {
                self.render_adaptive_engine_preset_modal(window, cx)
            } else {
                div().into_any_element()
            })
            .child(if self.advanced_power_plan_tuning_preset_editor.is_some() {
                self.render_advanced_power_plan_tuning_preset_modal(window, cx)
            } else {
                div().into_any_element()
            })
            .child(if self.update.startup_modal_visible {
                self.render_update_available_modal(cx)
            } else {
                div().into_any_element()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_width_tracks_collapsed_state() {
        assert_eq!(navigation_pane_width(false), NAV_PANE_WIDTH);
        assert_eq!(navigation_pane_width(true), NAV_PANE_COMPACT_WIDTH);
        assert_eq!(
            navigation_pane_width_at_progress(0.0),
            NAV_PANE_COMPACT_WIDTH
        );
        assert_eq!(navigation_pane_width_at_progress(1.0), NAV_PANE_WIDTH);
    }

    #[test]
    fn side_panel_width_tracks_visibility_progress() {
        assert_eq!(page_side_panel_width_at_progress(0.0, 1.0), 0.0);
        assert_eq!(
            page_side_panel_width_at_progress(1.0, 0.0),
            PAGE_SIDE_PANEL_COMPACT_WIDTH
        );
        assert_eq!(
            page_side_panel_width_at_progress(1.0, 1.0),
            PAGE_SIDE_PANEL_WIDTH
        );
        assert_eq!(
            page_side_panel_width_at_progress(2.0, 2.0),
            PAGE_SIDE_PANEL_WIDTH
        );
    }

    #[test]
    fn runtime_status_localizes_known_messages_and_preserves_errors() {
        assert_eq!(
            localized_runtime_status("Automation disabled."),
            t!("runtime_status.automation_disabled").to_string()
        );
        assert_eq!(
            localized_runtime_status("Memory Trim waiting for system memory load >= 80%."),
            t!("runtime_status.memory_trim_waiting", threshold = "80").to_string()
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

        let indicator = app_suspension_indicator(&status, "vivaldi.exe", false);

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
    fn app_suspension_indicator_reports_unavailable_before_runtime_state() {
        let status = AppSuspensionSnapshot {
            enabled: true,
            running_apps: vec!["service.exe".to_owned()],
            ..Default::default()
        };

        let indicator = app_suspension_indicator(&status, "service.exe", true);

        assert_eq!(
            indicator.label,
            t!("app_suspension.indicator.unavailable").to_string()
        );
        assert_eq!(
            indicator.hover,
            t!("app_suspension.indicator.unavailable_help").to_string()
        );
    }

    #[test]
    fn app_suspension_indicator_reports_running_before_not_running() {
        let status = AppSuspensionSnapshot {
            enabled: true,
            running_apps: vec!["vivaldi.exe".to_owned()],
            ..Default::default()
        };

        let indicator = app_suspension_indicator(&status, "vivaldi.exe", false);

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

        let indicator = app_suspension_indicator(&status, "vivaldi.exe", false);

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
    fn built_in_adaptive_engine_presets_set_cpu_scheduler_values() {
        let power_save = cpu_scheduler_preset_values(BuiltInAdaptiveEnginePreset::PowerSave);
        let performance = cpu_scheduler_preset_values(BuiltInAdaptiveEnginePreset::Performance);
        let speed = cpu_scheduler_preset_values(BuiltInAdaptiveEnginePreset::Speed);

        assert_eq!(
            power_save.background_priority,
            ProcessPrioritySetting::BelowNormal
        );
        assert_eq!(
            power_save.visible_window_priority,
            ProcessPrioritySetting::Normal
        );
        assert_eq!(
            performance.background_priority,
            ProcessPrioritySetting::BelowNormal
        );
        assert_eq!(
            performance.visible_window_priority,
            ProcessPrioritySetting::Normal
        );
        assert_eq!(speed.background_priority, ProcessPrioritySetting::Idle);
        assert_eq!(
            speed.visible_window_priority,
            ProcessPrioritySetting::BelowNormal
        );
        assert!(power_save.background_efficiency_enabled);
        assert!(performance.background_efficiency_enabled);
        assert!(speed.background_efficiency_enabled);
        assert!(!power_save.io_priority_enabled);
        assert!(performance.io_priority_enabled);
        assert!(speed.io_priority_enabled);
        assert!(!power_save.memory_priority_enabled);
        assert!(performance.memory_priority_enabled);
        assert!(speed.memory_priority_enabled);
        assert_eq!(power_save.background_io_priority, ProcessIoPriority::Low);
        assert_eq!(
            performance.background_io_priority,
            ProcessIoPriority::VeryLow
        );
        assert_eq!(speed.background_io_priority, ProcessIoPriority::VeryLow);
        assert_eq!(
            performance.focus_process_memory_priority,
            ProcessMemoryPrioritySetting::Normal
        );
        assert_eq!(
            performance.visible_window_memory_priority,
            ProcessMemoryPrioritySetting::BelowNormal
        );
        assert_eq!(
            speed.visible_window_memory_priority,
            ProcessMemoryPrioritySetting::Medium
        );
        assert_eq!(power_save.maximum_restrained_apps, 4);
        assert_eq!(performance.maximum_restrained_apps, 8);
        assert_eq!(speed.maximum_restrained_apps, 12);
        assert!(power_save.limit_background_processors_enabled);
        assert!(performance.limit_background_processors_enabled);
        assert!(speed.limit_background_processors_enabled);
        assert!(!power_save.dynamic_resource_zones_enabled);
        assert!(performance.dynamic_resource_zones_enabled);
        assert!(speed.dynamic_resource_zones_enabled);
        assert!(power_save.process_priority_enabled);
        assert!(performance.process_priority_enabled);
        assert!(speed.process_priority_enabled);
        assert_eq!(
            power_save.background_processor_selection,
            BackgroundProcessorSelection::LeastUsed
        );
        assert_eq!(
            performance.background_processor_selection,
            BackgroundProcessorSelection::LeastUsed
        );
        assert_eq!(
            speed.background_processor_selection,
            BackgroundProcessorSelection::LeastUsed
        );
        assert_eq!(
            power_save.focus_process_priority,
            ProcessPrioritySetting::AboveNormal
        );
        assert_eq!(
            performance.focus_process_priority,
            ProcessPrioritySetting::AboveNormal
        );
        assert_eq!(
            speed.focus_process_priority,
            ProcessPrioritySetting::AboveNormal
        );
        assert!(
            power_save.foreground_or_system_cpu_threshold_percent
                > performance.foreground_or_system_cpu_threshold_percent
        );
        assert!(
            performance.foreground_or_system_cpu_threshold_percent
                > speed.foreground_or_system_cpu_threshold_percent
        );
        assert!(
            power_save.background_app_cpu_threshold_percent
                > performance.background_app_cpu_threshold_percent
        );
        assert!(
            performance.background_app_cpu_threshold_percent
                > speed.background_app_cpu_threshold_percent
        );
        for (values, expected) in [
            (&power_save, (75, 10, 5, 1_500, 2, 4, 4)),
            (&performance, (60, 8, 4, 750, 3, 5, 8)),
            (&speed, (35, 4, 2, 500, 5, 8, 12)),
        ] {
            assert_eq!(
                (
                    values.foreground_or_system_cpu_threshold_percent,
                    values.background_app_cpu_threshold_percent,
                    values.cpu_recovery_threshold_percent,
                    values.reaction_time_ms,
                    values.cpu_restraint_time_seconds,
                    values.cpu_recovery_time_seconds,
                    values.maximum_restrained_apps,
                ),
                expected
            );
        }
        assert_eq!(power_save.processor_limit_percent, 60);
        assert_eq!(performance.processor_limit_percent, 75);
        assert_eq!(speed.processor_limit_percent, 75);
        assert!(!thread_priority_preset_values(BuiltInAdaptiveEnginePreset::PowerSave).enabled);
        assert!(
            !dynamic_priority_boost_preset_values(BuiltInAdaptiveEnginePreset::PowerSave).enabled
        );
        assert!(!gpu_priority_preset_values(BuiltInAdaptiveEnginePreset::PowerSave).enabled);
        assert_eq!(
            gpu_priority_preset_values(BuiltInAdaptiveEnginePreset::PowerSave).background_priority,
            ProcessGpuPrioritySetting::BelowNormal
        );
        assert_eq!(
            speed.focus_process_io_priority,
            ProcessIoPrioritySetting::High
        );
        assert_eq!(
            io_priority_preset_values(speed).visible_window_priority,
            ProcessIoPrioritySetting::Normal
        );
        assert_eq!(
            io_priority_preset_values(speed).background_priority,
            ProcessIoPrioritySetting::VeryLow
        );
        assert_eq!(
            thread_priority_preset_values(BuiltInAdaptiveEnginePreset::Speed).foreground_priority,
            ProcessThreadPrioritySetting::Highest
        );
        assert_eq!(
            thread_priority_preset_values(BuiltInAdaptiveEnginePreset::Speed)
                .visible_window_priority,
            ProcessThreadPrioritySetting::Normal
        );
        assert_eq!(
            thread_priority_preset_values(BuiltInAdaptiveEnginePreset::Speed).background_priority,
            ProcessThreadPrioritySetting::Idle
        );
        let max_dynamic = dynamic_priority_boost_preset_values(BuiltInAdaptiveEnginePreset::Speed);
        assert_eq!(
            (
                max_dynamic.foreground_boost,
                max_dynamic.visible_window_boost,
                max_dynamic.background_boost,
            ),
            (
                ProcessDynamicPriorityBoostSetting::Enabled,
                ProcessDynamicPriorityBoostSetting::Default,
                ProcessDynamicPriorityBoostSetting::Disabled,
            )
        );
        assert_eq!(
            gpu_priority_preset_values(BuiltInAdaptiveEnginePreset::Speed).foreground_priority,
            ProcessGpuPrioritySetting::High
        );
        assert_eq!(
            gpu_priority_preset_values(BuiltInAdaptiveEnginePreset::Speed).visible_window_priority,
            ProcessGpuPrioritySetting::Normal
        );
        assert_eq!(
            gpu_priority_preset_values(BuiltInAdaptiveEnginePreset::Speed).background_priority,
            ProcessGpuPrioritySetting::Idle
        );
    }

    #[test]
    fn adaptive_engine_default_keeps_cpu_scheduler_opt_in() {
        let mut settings = Settings::default();

        apply_adaptive_engine(&mut settings, true);

        assert!(settings.adaptive_engine.enabled);
        assert!(settings.adaptive_engine.processor_power_policy_enabled);
        assert!(!settings.background_efficiency.enabled);
        assert!(!settings.cpu_scheduler.cpu_pressure_restraint_enabled);
    }

    #[test]
    fn adaptive_engine_presets_tune_without_changing_feature_ownership() {
        let mut settings = Settings::default();
        settings.background_efficiency.enabled = true;
        settings.cpu_scheduler.cpu_pressure_restraint_enabled = true;

        apply_built_in_adaptive_engine_preset(
            &mut settings,
            BuiltInAdaptiveEnginePreset::PowerSave,
        );
        assert!(matches_built_in_adaptive_engine_preset(
            &settings,
            BuiltInAdaptiveEnginePreset::PowerSave
        ));
        assert!(!settings.adaptive_engine.enabled);
        assert!(settings.adaptive_engine.processor_power_policy_enabled);
        assert!(settings.background_efficiency.enabled);
        assert!(settings.cpu_scheduler.cpu_pressure_restraint_enabled);

        apply_built_in_adaptive_engine_preset(&mut settings, BuiltInAdaptiveEnginePreset::Balanced);
        assert!(matches_built_in_adaptive_engine_preset(
            &settings,
            BuiltInAdaptiveEnginePreset::Balanced
        ));
        assert!(!settings.adaptive_engine.enabled);
        assert!(settings.background_efficiency.enabled);
        assert!(cpu_scheduler_matches_preset(
            &settings.cpu_scheduler,
            BuiltInAdaptiveEnginePreset::Balanced
        ));

        apply_built_in_adaptive_engine_preset(
            &mut settings,
            BuiltInAdaptiveEnginePreset::Performance,
        );
        assert!(matches_built_in_adaptive_engine_preset(
            &settings,
            BuiltInAdaptiveEnginePreset::Performance
        ));
        assert!(!settings.adaptive_engine.enabled);
        assert!(settings.background_efficiency.enabled);
        assert!(settings.cpu_scheduler.cpu_pressure_restraint_enabled);
        assert!(settings.adaptive_engine.processor_power_policy_enabled);
        assert!(cpu_scheduler_matches_preset(
            &settings.cpu_scheduler,
            BuiltInAdaptiveEnginePreset::Performance
        ));

        apply_built_in_adaptive_engine_preset(&mut settings, BuiltInAdaptiveEnginePreset::Speed);
        assert!(matches_built_in_adaptive_engine_preset(
            &settings,
            BuiltInAdaptiveEnginePreset::Speed
        ));
        assert!(!settings.adaptive_engine.enabled);
        assert!(settings.background_efficiency.enabled);
        assert!(settings.cpu_scheduler.cpu_pressure_restraint_enabled);
        assert!(settings.adaptive_engine.processor_power_policy_enabled);
        assert!(cpu_scheduler_matches_preset(
            &settings.cpu_scheduler,
            BuiltInAdaptiveEnginePreset::Speed
        ));
    }

    #[test]
    fn adaptive_engine_custom_targets_make_preset_custom() {
        let mut settings = Settings::default();

        apply_built_in_adaptive_engine_preset(&mut settings, BuiltInAdaptiveEnginePreset::Balanced);
        settings
            .adaptive_engine
            .base_processor_policy
            .performance_max = 55;

        assert!(!matches_built_in_adaptive_engine_preset(
            &settings,
            BuiltInAdaptiveEnginePreset::Balanced
        ));
        assert_eq!(
            settings
                .adaptive_engine
                .base_processor_policy
                .performance_max,
            55
        );

        apply_built_in_adaptive_engine_preset(&mut settings, BuiltInAdaptiveEnginePreset::Balanced);
        apply_cpu_scheduler_preset(
            &mut settings.cpu_scheduler,
            BuiltInAdaptiveEnginePreset::Performance,
        );

        assert!(!matches_built_in_adaptive_engine_preset(
            &settings,
            BuiltInAdaptiveEnginePreset::Balanced
        ));
        assert!(cpu_scheduler_matches_preset(
            &settings.cpu_scheduler,
            BuiltInAdaptiveEnginePreset::Performance
        ));
    }

    #[test]
    fn adaptive_engine_toggle_does_not_change_preset_match() {
        let mut settings = Settings::default();

        for preset in BuiltInAdaptiveEnginePreset::ALL {
            apply_built_in_adaptive_engine_preset(&mut settings, preset);
            let enabled = !settings.adaptive_engine.enabled;
            apply_adaptive_engine(&mut settings, enabled);

            assert!(matches_built_in_adaptive_engine_preset(&settings, preset));
        }
    }

    #[test]
    fn cpu_scheduler_preset_match_ignores_hidden_preserve_flags() {
        let mut settings = CpuSchedulerSettings::default();
        apply_cpu_scheduler_preset(&mut settings, BuiltInAdaptiveEnginePreset::Performance);
        settings.io_priority.preserve_foreground_priority = false;
        settings.thread_priority.preserve_background_priority = false;
        settings.gpu_priority.foreground_detection_enabled = false;

        assert!(cpu_scheduler_matches_preset(
            &settings,
            BuiltInAdaptiveEnginePreset::Performance
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
            (0..(DASHBOARD_HISTORY_LEN + 2)).map(|index| (index as f32, (index * 2) as f32)),
            |value| format!("{:?}", value),
            |value| format!("{:?}", value),
        );

        assert_eq!(points.len(), DASHBOARD_HISTORY_LEN);
        assert_eq!(points[0].first_value, 2.0);
        assert_eq!(points[0].second_value, 4.0);
        assert_eq!(
            points[DASHBOARD_HISTORY_LEN - 1].first_value,
            (DASHBOARD_HISTORY_LEN + 1) as f64
        );

        let padded = dashboard_dual_line_points(
            [(7.0, 9.0)].into_iter(),
            |value| format!("{:?}", value),
            |value| format!("{:?}", value),
        );
        assert_eq!(padded.len(), DASHBOARD_HISTORY_LEN);
        assert_eq!(padded[DASHBOARD_HISTORY_LEN - 2].first_value, 0.0);
        assert_eq!(padded[DASHBOARD_HISTORY_LEN - 1].first_value, 7.0);
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
}
