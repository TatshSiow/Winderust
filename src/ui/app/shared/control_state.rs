use crate::ui::app::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum SuggestionTarget {
    Foreground,
    BackgroundEfficiency,
    CpuSetsSoft,
    ProcessorAffinityHard,
    MemoryTrim,
    AppSuspension,
    CpuLimiter,
    ByRunningApp,
    CpuScheduler,
    ProcessPriority,
    ThreadPriority,
    DynamicPriorityBoost,
    IoPriority,
    GpuPriority,
    MemoryPriority,
    TimerResolution,
}

impl SuggestionTarget {
    pub(in crate::ui::app) const ALL: [Self; 16] = [
        Self::Foreground,
        Self::BackgroundEfficiency,
        Self::CpuSetsSoft,
        Self::ProcessorAffinityHard,
        Self::MemoryTrim,
        Self::AppSuspension,
        Self::CpuLimiter,
        Self::ByRunningApp,
        Self::CpuScheduler,
        Self::ProcessPriority,
        Self::ThreadPriority,
        Self::DynamicPriorityBoost,
        Self::IoPriority,
        Self::GpuPriority,
        Self::MemoryPriority,
        Self::TimerResolution,
    ];

    pub(in crate::ui::app) fn input(self, inputs: &UiInputs) -> &Entity<InputState> {
        match self {
            Self::Foreground => &inputs.foreground_process,
            Self::BackgroundEfficiency => &inputs.background_efficiency_process,
            Self::CpuSetsSoft => &inputs.cpu_sets_soft_process,
            Self::ProcessorAffinityHard => &inputs.processor_affinity_hard_process,
            Self::MemoryTrim => &inputs.memory_trim_exclusion,
            Self::AppSuspension => &inputs.app_suspension_process,
            Self::CpuLimiter => &inputs.cpu_limiter_process,
            Self::ByRunningApp => &inputs.performance_process,
            Self::CpuScheduler => &inputs.cpu_scheduler_process,
            Self::ProcessPriority => &inputs.process_priority_process,
            Self::ThreadPriority => &inputs.thread_priority_process,
            Self::DynamicPriorityBoost => &inputs.dynamic_priority_boost_process,
            Self::IoPriority => &inputs.io_priority_process,
            Self::GpuPriority => &inputs.gpu_priority_process,
            Self::MemoryPriority => &inputs.memory_priority_process,
            Self::TimerResolution => &inputs.timer_resolution_process,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui::app) enum RuleTitleTarget {
    ByTime(usize),
    ByCpuLoad(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum RuleCardTarget {
    ByCpuLoad(usize),
    AppSuspension(String),
    CpuLimiter(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum SettingGroupTarget {
    AccentColor,
    LimitBackgroundProcessors,
    CpuPressureRestraint,
    AdaptiveEnginePresetLimitBackgroundProcessors,
    AdaptiveEnginePresetCpuPressureRestraint,
    ProcessPriorityMaster,
    ProcessPriorityForegroundDetection,
    ProcessPriorityVisibleWindowDetection,
    ThreadPriorityMaster,
    ThreadPriorityForegroundDetection,
    ThreadPriorityVisibleWindowDetection,
    DynamicPriorityBoostMaster,
    DynamicPriorityBoostForegroundDetection,
    DynamicPriorityBoostVisibleWindowDetection,
    IoPriorityMaster,
    IoPriorityForegroundDetection,
    IoPriorityVisibleWindowDetection,
    EfficiencyEnable,
    BackgroundEfficiencyForegroundDetection,
    BackgroundEfficiencyVisibleWindowDetection,
    GpuPriorityMaster,
    GpuPriorityForegroundDetection,
    GpuPriorityVisibleWindowDetection,
    MemoryPriorityMaster,
    MemoryPriorityForegroundDetection,
    MemoryPriorityVisibleWindowDetection,
    MemoryTrimSafety,
    MemoryTrimThresholds,
    MemoryTrimWhen,
    ProcessorPowerAc,
    ProcessorPowerBattery,
    SuspensionThaw,
    SuspensionAudio,
    SuspensionNetwork,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum AdaptiveEngineTuningTab {
    #[default]
    CpuBehaviour,
    ProcessorPower,
    PriorityControl,
    CustomRules,
}

impl AdaptiveEngineTuningTab {
    pub(in crate::ui::app) const LIVE: [Self; 4] = [
        Self::CpuBehaviour,
        Self::ProcessorPower,
        Self::PriorityControl,
        Self::CustomRules,
    ];

    pub(in crate::ui::app) const PRESET: [Self; 3] = [
        Self::CpuBehaviour,
        Self::ProcessorPower,
        Self::PriorityControl,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum AdaptiveEngineTuningTarget {
    Live,
    Preset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum AdaptiveEngineTuningNumericField {
    ProcessorPowerPolicy(AdaptiveEngineProcessorPowerPolicyField),
    ProfileBoostPolicy(AdaptiveEngineProfile, ProcessorPowerSource),
    ProcessorLimit,
    ForegroundOrSystemCpuThreshold,
    BackgroundAppCpuThreshold,
    CpuRecoveryThreshold,
    MaximumRestrainedApps,
    ReactionTime,
    CpuRestraintTime,
    CpuRecoveryTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum AdaptiveEngineProfile {
    BackgroundPressure,
    FocusAndLaunch,
}

impl AdaptiveEngineProfile {
    pub(in crate::ui::app) const fn key(self) -> &'static str {
        match self {
            Self::BackgroundPressure => "background-pressure",
            Self::FocusAndLaunch => "focus-and-launch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum AdaptiveEngineBoostModeField {
    Base,
    Profile(AdaptiveEngineProfile, ProcessorPowerSource),
}

impl AdaptiveEngineBoostModeField {
    pub(in crate::ui::app) fn key(self) -> String {
        match self {
            Self::Base => "base".to_owned(),
            Self::Profile(profile, source) => format!(
                "{}-{}",
                profile.key(),
                match source {
                    ProcessorPowerSource::Ac => "ac",
                    ProcessorPowerSource::Battery => "battery",
                }
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum ProcessRuleTier {
    Focus,
    VisibleWindow,
    Background,
}

impl ProcessRuleTier {
    pub(in crate::ui::app) const ALL: [Self; 3] =
        [Self::Focus, Self::VisibleWindow, Self::Background];

    pub(in crate::ui::app) const fn key(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::VisibleWindow => "visible-window",
            Self::Background => "background",
        }
    }

    pub(in crate::ui::app) const fn flags(self) -> (bool, bool) {
        match self {
            Self::Focus => (true, false),
            Self::VisibleWindow => (false, true),
            Self::Background => (false, false),
        }
    }

    pub(in crate::ui::app) const fn cpu_limiter_allowed_time(self, rule: &CpuLimiterRule) -> u8 {
        match self {
            Self::Focus => rule.focus_allowed_cpu_time_percent,
            Self::VisibleWindow => rule.visible_window_allowed_cpu_time_percent,
            Self::Background => rule.background_allowed_cpu_time_percent,
        }
    }

    pub(in crate::ui::app) fn set_cpu_limiter_allowed_time(
        self,
        rule: &mut CpuLimiterRule,
        value: u8,
    ) {
        match self {
            Self::Focus => rule.focus_allowed_cpu_time_percent = value,
            Self::VisibleWindow => rule.visible_window_allowed_cpu_time_percent = value,
            Self::Background => rule.background_allowed_cpu_time_percent = value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui::app) enum BuiltInAdaptiveEnginePreset {
    PowerSave,
    Balanced,
    Performance,
    Speed,
}

impl BuiltInAdaptiveEnginePreset {
    pub(in crate::ui::app) const ALL: [Self; 4] = [
        Self::PowerSave,
        Self::Balanced,
        Self::Performance,
        Self::Speed,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum ThresholdField {
    Download(usize),
    Upload(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum NumericField {
    ActivityIdleTimeout,
    GeneralCheckInterval,
    ExecutionFailureSuppressionThreshold,
    MemoryTrimMemoryLoadThreshold,
    MemoryTrimWorkingSetThreshold,
    MemoryTrimIdleSeconds,
    SuspensionBackgroundDelay,
    SuspensionThawInterval,
    SuspensionThawDuration,
    SuspensionAudioRefreeze,
    SuspensionNetworkRefreeze,
    AdaptiveEngineTuning(AdaptiveEngineTuningTarget, AdaptiveEngineTuningNumericField),
    ProcessorAcCoreParkingMin,
    ProcessorAcPerformanceMin,
    ProcessorAcPerformanceMax,
    ProcessorAcBoostPolicy,
    ProcessorDcCoreParkingMin,
    ProcessorDcPerformanceMin,
    ProcessorDcPerformanceMax,
    ProcessorDcBoostPolicy,
    AdvancedPowerPlanTuningPreset(AdaptiveEngineProcessorPowerPolicyField),
    CpuThreshold(usize),
    CpuUpperThreshold(usize),
    CpuDuration(usize),
    CpuLimiterAllowedTime(usize, ProcessRuleTier),
    TimerResolutionRule(usize),
    NetworkThreshold(ThresholdField),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum AdaptiveEngineProcessorPowerPolicyField {
    CoreParkingMin,
    PerformanceMin,
    PerformanceMax,
    BoostPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum ProcessorPowerSlider {
    AcCoreParkingMin,
    AcPerformanceMin,
    AcPerformanceMax,
    AcBoostPolicy,
    BatteryCoreParkingMin,
    BatteryPerformanceMin,
    BatteryPerformanceMax,
    BatteryBoostPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum ProcessorPowerSource {
    Ac,
    Battery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum CpuThresholdSlider {
    Lower(usize),
    Upper(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum ActivitySlider {
    IdleTimeout,
    CheckInterval,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::ui::app) struct StepChange<T> {
    pub(in crate::ui::app) delta: T,
    pub(in crate::ui::app) increase: bool,
}

pub(in crate::ui::app) type StepChangeHandler<T> =
    Rc<dyn Fn(&StepChange<T>, &mut Window, &mut App)>;
pub(in crate::ui::app) type BoolChangeHandler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

#[derive(Debug, Clone, Copy)]
pub(in crate::ui::app) struct SliderRange {
    pub(in crate::ui::app) min: u64,
    pub(in crate::ui::app) max: u64,
    pub(in crate::ui::app) step: u64,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::ui::app) struct StableSliderSpec {
    pub(in crate::ui::app) range: SliderRange,
    pub(in crate::ui::app) enabled: bool,
    pub(in crate::ui::app) track_color: u32,
    pub(in crate::ui::app) thumb_color: u32,
}

pub(in crate::ui::app) struct SliderRowSpec<'a, T> {
    pub(in crate::ui::app) id: SharedString,
    pub(in crate::ui::app) label: SharedString,
    pub(in crate::ui::app) value_element: AnyElement,
    pub(in crate::ui::app) state: &'a Entity<SliderState>,
    pub(in crate::ui::app) enabled: bool,
    pub(in crate::ui::app) delta: T,
    pub(in crate::ui::app) range: SliderRange,
}

pub(in crate::ui::app) struct ActivitySliderCardSpec<'a> {
    pub(in crate::ui::app) id: SharedString,
    pub(in crate::ui::app) label: SharedString,
    pub(in crate::ui::app) value_element: AnyElement,
    pub(in crate::ui::app) state: &'a Entity<SliderState>,
    pub(in crate::ui::app) enabled: bool,
    pub(in crate::ui::app) range: SliderRange,
}

pub(in crate::ui::app) struct SettingGroupBody {
    pub(in crate::ui::app) collapsed: bool,
    pub(in crate::ui::app) rows: Vec<AnyElement>,
    pub(in crate::ui::app) animation_height: Option<f32>,
    pub(in crate::ui::app) controls_enabled: Option<bool>,
}

pub(in crate::ui::app) fn make_input(
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
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

pub(in crate::ui::app) fn make_percent_slider(
    cx: &mut Context<WinderustApp>,
    value: u64,
) -> Entity<SliderState> {
    make_range_slider(cx, value, 0, 100, 1)
}

pub(in crate::ui::app) fn make_range_slider(
    cx: &mut Context<WinderustApp>,
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

pub(in crate::ui::app) fn make_processor_power_slider(
    cx: &mut Context<WinderustApp>,
    value: u64,
) -> Entity<SliderState> {
    make_percent_slider(cx, value)
}

pub(in crate::ui::app) fn processor_power_slider_input(
    inputs: &UiInputs,
    slider: ProcessorPowerSlider,
) -> Entity<SliderState> {
    match slider {
        ProcessorPowerSlider::AcCoreParkingMin => {
            inputs.processor_power_ac_core_parking_min.clone()
        }
        ProcessorPowerSlider::AcPerformanceMin => inputs.processor_power_ac_performance_min.clone(),
        ProcessorPowerSlider::AcPerformanceMax => inputs.processor_power_ac_performance_max.clone(),
        ProcessorPowerSlider::AcBoostPolicy => inputs.processor_power_ac_boost_policy.clone(),
        ProcessorPowerSlider::BatteryCoreParkingMin => {
            inputs.processor_power_battery_core_parking_min.clone()
        }
        ProcessorPowerSlider::BatteryPerformanceMin => {
            inputs.processor_power_battery_performance_min.clone()
        }
        ProcessorPowerSlider::BatteryPerformanceMax => {
            inputs.processor_power_battery_performance_max.clone()
        }
        ProcessorPowerSlider::BatteryBoostPolicy => {
            inputs.processor_power_battery_boost_policy.clone()
        }
    }
}

pub(in crate::ui::app) fn cpu_threshold_slider_input(
    inputs: &UiInputs,
    slider: CpuThresholdSlider,
) -> Option<Entity<SliderState>> {
    match slider {
        CpuThresholdSlider::Lower(index) => inputs.cpu_rule_thresholds.get(index),
        CpuThresholdSlider::Upper(index) => inputs.cpu_rule_upper_thresholds.get(index),
    }
    .cloned()
}

pub(in crate::ui::app) fn cpu_limiter_slider_input(
    inputs: &UiInputs,
    index: usize,
    tier: ProcessRuleTier,
) -> Option<Entity<SliderState>> {
    match tier {
        ProcessRuleTier::Focus => inputs.cpu_limiter_focus_allowed_times.get(index),
        ProcessRuleTier::VisibleWindow => {
            inputs.cpu_limiter_visible_window_allowed_times.get(index)
        }
        ProcessRuleTier::Background => inputs.cpu_limiter_background_allowed_times.get(index),
    }
    .cloned()
}

pub(in crate::ui::app) fn sync_input_vec(
    inputs: &mut Vec<Entity<InputState>>,
    len: usize,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
    value_at: impl Fn(usize) -> String,
    placeholder: &str,
) {
    while inputs.len() < len {
        let index = inputs.len();
        inputs.push(make_input(window, cx, &value_at(index), placeholder));
    }
    inputs.truncate(len);
}

pub(in crate::ui::app) fn sync_slider_vec(
    inputs: &mut Vec<Entity<SliderState>>,
    len: usize,
    cx: &mut Context<WinderustApp>,
    range: SliderRange,
    value_at: impl Fn(usize) -> u64,
) {
    while inputs.len() < len {
        let index = inputs.len();
        inputs.push(make_range_slider(
            cx,
            value_at(index),
            range.min,
            range.max,
            range.step,
        ));
    }
    inputs.truncate(len);
}

pub(in crate::ui::app) fn clear_input(
    input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
) {
    clear_input_to(input, "", window, cx);
}

pub(in crate::ui::app) fn set_input_placeholder(
    input: &Entity<InputState>,
    placeholder: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
) {
    input.update(cx, |input, cx| {
        input.set_placeholder(placeholder, window, cx)
    });
}

pub(in crate::ui::app) fn clear_input_to(
    input: &Entity<InputState>,
    value: &str,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
) {
    let value = SharedString::from(value.to_owned());
    input.update(cx, |input, cx| input.set_value(value, window, cx));
}

impl UiInputs {
    pub(in crate::ui::app) fn new(
        window: &mut Window,
        cx: &mut Context<WinderustApp>,
        settings: &Settings,
        processor_power_values: ProcessorPowerSourceValues,
    ) -> Self {
        let processor_power_values = processor_power_values.normalized();
        Self {
            dashboard_search: make_input(window, cx, "", &t!("home.search_placeholder")),
            process_list_search: make_input(window, cx, "", &t!("process_list.search_placeholder")),
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
            cpu_limiter_focus_allowed_times: settings
                .cpu_limiter
                .rules
                .iter()
                .map(|rule| {
                    make_range_slider(cx, rule.focus_allowed_cpu_time_percent as u64, 1, 99, 1)
                })
                .collect(),
            cpu_limiter_visible_window_allowed_times: settings
                .cpu_limiter
                .rules
                .iter()
                .map(|rule| {
                    make_range_slider(
                        cx,
                        rule.visible_window_allowed_cpu_time_percent as u64,
                        1,
                        99,
                        1,
                    )
                })
                .collect(),
            cpu_limiter_background_allowed_times: settings
                .cpu_limiter
                .rules
                .iter()
                .map(|rule| {
                    make_range_slider(
                        cx,
                        rule.background_allowed_cpu_time_percent as u64,
                        1,
                        99,
                        1,
                    )
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
            foreground_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            background_efficiency_process: make_input(
                window,
                cx,
                "",
                &t!("common.search_running_apps"),
            ),
            memory_trim_exclusion: make_input(window, cx, "", &t!("common.search_running_apps")),
            app_suspension_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            cpu_limiter_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            performance_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            cpu_sets_soft_process: make_input(window, cx, "", &t!("common.search_running_apps")),
            processor_affinity_hard_process: make_input(
                window,
                cx,
                "",
                &t!("common.search_running_apps"),
            ),
            adaptive_engine_preset_name: make_input(
                window,
                cx,
                "",
                &t!("adaptive_engine.preset_name_placeholder"),
            ),
            cpu_allocation_preset_name: make_input(
                window,
                cx,
                "",
                &t!("cpu_allocation.preset_name_placeholder"),
            ),
            advanced_power_plan_tuning_preset_name: make_input(
                window,
                cx,
                "",
                &t!("processor_power.preset_name_placeholder"),
            ),
            cpu_scheduler_process: make_input(window, cx, "", &t!("common.search_running_apps")),
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
                CHECK_INTERVAL_MIN_MS,
                CHECK_INTERVAL_MAX_MS,
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
            processor_power_battery_core_parking_min: make_processor_power_slider(
                cx,
                processor_power_values.battery.core_parking_min as u64,
            ),
            processor_power_battery_performance_min: make_processor_power_slider(
                cx,
                processor_power_values.battery.performance_min as u64,
            ),
            processor_power_battery_performance_max: make_processor_power_slider(
                cx,
                processor_power_values.battery.performance_max as u64,
            ),
            processor_power_battery_boost_policy: make_processor_power_slider(
                cx,
                processor_power_values.battery.boost_policy as u64,
            ),
        }
    }

    pub(in crate::ui::app) fn ensure_for_settings(
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
            SliderRange {
                min: 0,
                max: 100,
                step: 1,
            },
            |index| settings.by_cpu_load.rules[index].threshold_percent as u64,
        );
        sync_slider_vec(
            &mut self.cpu_rule_upper_thresholds,
            settings.by_cpu_load.rules.len(),
            cx,
            SliderRange {
                min: 0,
                max: 100,
                step: 1,
            },
            |index| {
                settings.by_cpu_load.rules[index]
                    .upper_threshold_percent
                    .unwrap_or(100) as u64
            },
        );
        let cpu_limiter_range = SliderRange {
            min: 1,
            max: 99,
            step: 1,
        };
        sync_slider_vec(
            &mut self.cpu_limiter_focus_allowed_times,
            settings.cpu_limiter.rules.len(),
            cx,
            cpu_limiter_range,
            |index| u64::from(settings.cpu_limiter.rules[index].focus_allowed_cpu_time_percent),
        );
        sync_slider_vec(
            &mut self.cpu_limiter_visible_window_allowed_times,
            settings.cpu_limiter.rules.len(),
            cx,
            cpu_limiter_range,
            |index| {
                u64::from(settings.cpu_limiter.rules[index].visible_window_allowed_cpu_time_percent)
            },
        );
        sync_slider_vec(
            &mut self.cpu_limiter_background_allowed_times,
            settings.cpu_limiter.rules.len(),
            cx,
            cpu_limiter_range,
            |index| {
                u64::from(settings.cpu_limiter.rules[index].background_allowed_cpu_time_percent)
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
    }

    pub(in crate::ui::app) fn refresh_localized_placeholders(
        &self,
        window: &mut Window,
        cx: &mut Context<WinderustApp>,
    ) {
        set_input_placeholder(
            &self.dashboard_search,
            t!("home.search_placeholder"),
            window,
            cx,
        );
        set_input_placeholder(
            &self.process_list_search,
            t!("process_list.search_placeholder"),
            window,
            cx,
        );
        for target in SuggestionTarget::ALL {
            set_input_placeholder(
                target.input(self),
                t!("common.search_running_apps"),
                window,
                cx,
            );
        }
        for input in self
            .by_cpu_load_rule_names
            .iter()
            .chain(&self.by_time_rule_names)
        {
            set_input_placeholder(input, t!("common.rule_name"), window, cx);
        }
        set_input_placeholder(
            &self.adaptive_engine_preset_name,
            t!("adaptive_engine.preset_name_placeholder"),
            window,
            cx,
        );
        set_input_placeholder(
            &self.cpu_allocation_preset_name,
            t!("cpu_allocation.preset_name_placeholder"),
            window,
            cx,
        );
        set_input_placeholder(
            &self.advanced_power_plan_tuning_preset_name,
            t!("processor_power.preset_name_placeholder"),
            window,
            cx,
        );
    }
}

impl WinderustApp {
    pub(in crate::ui::app) fn rebuild_inputs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings = self.settings.clone();
        let processor_power_values = self.processor_power_values();
        self.editing_rule_title = None;
        self.editing_numeric = None;
        self.adaptive_engine_preset_editor = None;
        self.cpu_allocation_preset_editor = None;
        self.advanced_power_plan_tuning_preset_editor = None;
        self.expanded_rule_cards.clear();
        self.pending_list_item_removals.clear();
        self.process_catalog.selected_paths.clear();
        self.inputs = UiInputs::new(window, cx, &settings, processor_power_values);
        self.rebuild_rule_title_input_subscriptions(window, cx);
        self.rebuild_process_picker_input_subscriptions(window, cx);
        self.subscribe_to_numeric_input(window, cx);
        self.rebuild_notify_input_subscriptions(window, cx);
        self.subscribe_to_processor_power_sliders(window, cx);
        self.rebuild_cpu_threshold_slider_subscriptions(window, cx);
        self.rebuild_cpu_limiter_slider_subscriptions(window, cx);
        self.subscribe_to_activity_sliders(window, cx);
    }

    pub(in crate::ui::app) fn rebuild_process_picker_input_subscriptions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._process_picker_input_subscriptions = SuggestionTarget::ALL
            .into_iter()
            .map(|target| {
                let input = target.input(&self.inputs).clone();
                cx.subscribe_in(
                    &input,
                    window,
                    move |app, input, event: &InputEvent, _, cx| {
                        if matches!(event, InputEvent::Change) {
                            let display_name = input.read(cx).value();
                            let selection_still_matches = app
                                .process_catalog
                                .selected_paths
                                .get(&target)
                                .is_some_and(|path| {
                                    process_path_matches_display_name(path, display_name.as_ref())
                                });
                            if !selection_still_matches {
                                app.process_catalog.selected_paths.remove(&target);
                            }
                            cx.notify();
                        }
                    },
                )
            })
            .collect();
    }

    pub(in crate::ui::app) fn rule_title_input_count(&self) -> usize {
        self.inputs.by_time_rule_names.len() + self.inputs.by_cpu_load_rule_names.len()
    }

    pub(in crate::ui::app) fn ensure_rule_title_input_subscriptions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self._rule_title_input_subscriptions.len() != self.rule_title_input_count() {
            self.rebuild_rule_title_input_subscriptions(window, cx);
        }
    }

    pub(in crate::ui::app) fn rebuild_rule_title_input_subscriptions(
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

    pub(in crate::ui::app) fn subscribe_to_rule_title_input(
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

    pub(in crate::ui::app) fn handle_rule_title_input_event(
        &mut self,
        target: RuleTitleTarget,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            self.finish_rule_title_edit(target, cx);
        }
    }

    pub(in crate::ui::app) fn subscribe_to_numeric_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._numeric_input_subscription = Some(cx.subscribe_in(
            &self.inputs.numeric_value,
            window,
            move |app, _, event: &InputEvent, _, cx| {
                app.handle_numeric_input_event(event, cx);
            },
        ));
    }

    pub(in crate::ui::app) fn rebuild_notify_input_subscriptions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inputs = [
            self.inputs.dashboard_search.clone(),
            self.inputs.process_list_search.clone(),
            self.inputs.adaptive_engine_preset_name.clone(),
            self.inputs.cpu_allocation_preset_name.clone(),
            self.inputs.advanced_power_plan_tuning_preset_name.clone(),
        ];
        self._notify_input_subscriptions = inputs
            .into_iter()
            .map(|input| {
                cx.subscribe_in(&input, window, |_, _, _: &InputEvent, _, cx| {
                    cx.notify();
                })
            })
            .collect();
    }

    pub(in crate::ui::app) fn subscribe_to_processor_power_sliders(
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
            ProcessorPowerSlider::BatteryCoreParkingMin,
            ProcessorPowerSlider::BatteryPerformanceMin,
            ProcessorPowerSlider::BatteryPerformanceMax,
            ProcessorPowerSlider::BatteryBoostPolicy,
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

    pub(in crate::ui::app) fn handle_processor_power_slider_event(
        &mut self,
        slider: ProcessorPowerSlider,
        event: &SliderEvent,
        cx: &mut Context<Self>,
    ) {
        let SliderEvent::Change(value) = event;
        self.set_processor_power_slider_value(slider, value.end().round() as u64);
        cx.notify();
    }

    pub(in crate::ui::app) fn cpu_threshold_slider_input_count(&self) -> usize {
        self.inputs.cpu_rule_thresholds.len() + self.inputs.cpu_rule_upper_thresholds.len()
    }

    pub(in crate::ui::app) fn ensure_cpu_threshold_slider_subscriptions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self._cpu_threshold_slider_subscriptions.len() != self.cpu_threshold_slider_input_count()
        {
            self.rebuild_cpu_threshold_slider_subscriptions(window, cx);
        }
    }

    pub(in crate::ui::app) fn rebuild_cpu_threshold_slider_subscriptions(
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

    pub(in crate::ui::app) fn handle_cpu_threshold_slider_event(
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

    pub(in crate::ui::app) fn cpu_limiter_slider_input_count(&self) -> usize {
        self.inputs.cpu_limiter_focus_allowed_times.len()
            + self.inputs.cpu_limiter_visible_window_allowed_times.len()
            + self.inputs.cpu_limiter_background_allowed_times.len()
    }

    pub(in crate::ui::app) fn ensure_cpu_limiter_slider_subscriptions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self._cpu_limiter_slider_subscriptions.len() != self.cpu_limiter_slider_input_count() {
            self.rebuild_cpu_limiter_slider_subscriptions(window, cx);
        }
    }

    pub(in crate::ui::app) fn rebuild_cpu_limiter_slider_subscriptions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut inputs = Vec::new();
        for tier in ProcessRuleTier::ALL {
            let tier_inputs = match tier {
                ProcessRuleTier::Focus => &self.inputs.cpu_limiter_focus_allowed_times,
                ProcessRuleTier::VisibleWindow => {
                    &self.inputs.cpu_limiter_visible_window_allowed_times
                }
                ProcessRuleTier::Background => &self.inputs.cpu_limiter_background_allowed_times,
            };
            inputs.extend(
                tier_inputs
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(move |(index, input)| (input, index, tier)),
            );
        }

        self._cpu_limiter_slider_subscriptions.clear();
        for (input, index, tier) in inputs {
            self._cpu_limiter_slider_subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |app, _, event: &SliderEvent, _, cx| {
                    let SliderEvent::Change(value) = event;
                    app.set_cpu_limiter_slider_value(
                        index,
                        tier,
                        value.end().round().clamp(1.0, 99.0) as u8,
                    );
                    cx.notify();
                },
            ));
        }
    }

    pub(in crate::ui::app) fn subscribe_to_activity_sliders(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

    pub(in crate::ui::app) fn handle_activity_slider_event(
        &mut self,
        slider: ActivitySlider,
        event: &SliderEvent,
        cx: &mut Context<Self>,
    ) {
        let SliderEvent::Change(value) = event;
        self.set_activity_slider_value(slider, value.end().round() as u64);
        cx.notify();
    }

    pub(in crate::ui::app) fn handle_numeric_input_event(
        &mut self,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            self.finish_numeric_edit(cx);
        }
    }

    pub(in crate::ui::app) fn rule_title_input(
        &self,
        target: RuleTitleTarget,
    ) -> Option<Entity<InputState>> {
        match target {
            RuleTitleTarget::ByTime(index) => self.inputs.by_time_rule_names.get(index),
            RuleTitleTarget::ByCpuLoad(index) => self.inputs.by_cpu_load_rule_names.get(index),
        }
        .cloned()
    }

    pub(in crate::ui::app) fn begin_rule_title_edit(
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

    pub(in crate::ui::app) fn begin_numeric_edit(
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

    pub(in crate::ui::app) fn finish_numeric_edit(&mut self, cx: &mut Context<Self>) {
        let Some(field) = self.editing_numeric.take() else {
            return;
        };
        let value = self.inputs.numeric_value.read(cx).value().to_string();
        self.apply_numeric_input(field, &value);
        cx.notify();
    }

    pub(in crate::ui::app) fn apply_numeric_input(&mut self, field: NumericField, value: &str) {
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
                if let Some(value) =
                    parse_u64_input(&value, CHECK_INTERVAL_MIN_MS, CHECK_INTERVAL_MAX_MS)
                {
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
            NumericField::MemoryTrimIdleSeconds => {
                if let Some(value) = parse_u64_input(&value, 1, 86_400) {
                    self.settings.memory_trim.process_idle_seconds = value;
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
            NumericField::AdaptiveEngineTuning(target, field) => {
                self.apply_adaptive_engine_tuning_numeric_input(target, field, &value);
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
                        ProcessorPowerSlider::BatteryCoreParkingMin,
                        value,
                    );
                }
            }
            NumericField::ProcessorDcPerformanceMin => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.set_processor_power_slider_value(
                        ProcessorPowerSlider::BatteryPerformanceMin,
                        value,
                    );
                }
            }
            NumericField::ProcessorDcPerformanceMax => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.set_processor_power_slider_value(
                        ProcessorPowerSlider::BatteryPerformanceMax,
                        value,
                    );
                }
            }
            NumericField::ProcessorDcBoostPolicy => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.set_processor_power_slider_value(
                        ProcessorPowerSlider::BatteryBoostPolicy,
                        value,
                    );
                }
            }
            NumericField::AdvancedPowerPlanTuningPreset(field) => {
                if let Some(value) = parse_u64_input(&value, 0, 100) {
                    self.set_advanced_power_plan_tuning_preset_field_value(field, value);
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
            NumericField::CpuLimiterAllowedTime(index, tier) => {
                if let Some(value) = parse_cpu_limiter_allowed_time_percent(&value) {
                    self.set_cpu_limiter_slider_value(index, tier, value);
                }
            }
            NumericField::TimerResolutionRule(index) => {
                let minimum_100ns = self
                    .feature_status
                    .timer_resolution
                    .minimum_100ns
                    .unwrap_or((TIMER_RESOLUTION_INPUT_MIN_MS * 10_000.0).round() as u32);
                let maximum_100ns = self
                    .feature_status
                    .timer_resolution
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

    pub(in crate::ui::app) fn finish_rule_title_edit(
        &mut self,
        target: RuleTitleTarget,
        cx: &mut Context<Self>,
    ) {
        self.sync_input_values(cx);
        if self.editing_rule_title == Some(target) {
            self.editing_rule_title = None;
        }
        cx.notify();
    }

    pub(in crate::ui::app) fn sync_input_values(&mut self, cx: &mut Context<Self>) {
        for index in 0..self
            .settings
            .by_cpu_load
            .rules
            .len()
            .min(self.inputs.by_cpu_load_rule_names.len())
        {
            let value = self.inputs.by_cpu_load_rule_names[index]
                .read(cx)
                .value()
                .to_string();
            if self.settings.by_cpu_load.rules[index].name != value {
                self.settings.by_cpu_load.rules[index].name = value;
            }
        }
        for index in 0..self.settings.by_time.rules.len() {
            if let Some(input) = self.inputs.by_time_rule_names.get(index) {
                let value = input.read(cx).value().to_string();
                if self.settings.by_time.rules[index].name != value {
                    self.settings.by_time.rules[index].name = value;
                }
            }
            if let Some(input) = self.inputs.schedule_start_times.get(index) {
                let value = input.read(cx).value().to_string();
                if self.settings.by_time.rules[index].start_time != value {
                    self.settings.by_time.rules[index].start_time = value;
                }
            }
            if let Some(input) = self.inputs.schedule_end_times.get(index) {
                let value = input.read(cx).value().to_string();
                if self.settings.by_time.rules[index].end_time != value {
                    self.settings.by_time.rules[index].end_time = value;
                }
            }
        }
    }
}

fn parse_cpu_limiter_allowed_time_percent(value: &str) -> Option<u8> {
    value
        .parse::<u8>()
        .ok()
        .filter(|value| (1..=99).contains(value))
}

impl WinderustApp {
    pub(in crate::ui::app) fn load_power_source_input_values(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inputs.ensure_for_settings(window, cx, &self.settings);
        for (rule, input) in self
            .settings
            .by_cpu_load
            .rules
            .iter()
            .zip(&self.inputs.by_cpu_load_rule_names)
        {
            clear_input_to(input, &rule.name, window, cx);
        }
        for (index, rule) in self.settings.by_time.rules.iter().enumerate() {
            if let Some(input) = self.inputs.by_time_rule_names.get(index) {
                clear_input_to(input, &rule.name, window, cx);
            }
            if let Some(input) = self.inputs.schedule_start_times.get(index) {
                clear_input_to(input, &rule.start_time, window, cx);
            }
            if let Some(input) = self.inputs.schedule_end_times.get(index) {
                clear_input_to(input, &rule.end_time, window, cx);
            }
        }
    }

    pub(in crate::ui::app) fn set_cpu_threshold_slider_value(
        &mut self,
        slider: CpuThresholdSlider,
        value: u8,
    ) {
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

    pub(in crate::ui::app) fn set_cpu_limiter_slider_value(
        &mut self,
        index: usize,
        tier: ProcessRuleTier,
        value: u8,
    ) {
        if let Some(rule) = self.settings.cpu_limiter.rules.get_mut(index) {
            tier.set_cpu_limiter_allowed_time(rule, value.clamp(1, 99));
        }
    }

    pub(in crate::ui::app) fn sync_cpu_limiter_slider_states(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (index, rule) in self.settings.cpu_limiter.rules.iter().enumerate() {
            for tier in ProcessRuleTier::ALL {
                let Some(input) = cpu_limiter_slider_input(&self.inputs, index, tier) else {
                    continue;
                };
                let value = tier.cpu_limiter_allowed_time(rule).clamp(1, 99) as f32;
                input.update(cx, |state, cx| {
                    if (state.value().end() - value).abs() > f32::EPSILON {
                        state.set_value(value, window, cx);
                    }
                });
            }
        }
    }

    pub(in crate::ui::app) fn sync_cpu_threshold_slider_states(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

    pub(in crate::ui::app) fn sync_cpu_threshold_slider_state(
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

    pub(in crate::ui::app) fn set_activity_slider_value(
        &mut self,
        slider: ActivitySlider,
        value: u64,
    ) {
        match slider {
            ActivitySlider::IdleTimeout => {
                self.settings.by_activity.idle_timeout_seconds = value.clamp(
                    ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS,
                    ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS,
                );
            }
            ActivitySlider::CheckInterval => {
                self.settings.general.check_interval_ms =
                    snap_to_step(value, ACTIVITY_CHECK_INTERVAL_STEP_MS)
                        .clamp(CHECK_INTERVAL_MIN_MS, CHECK_INTERVAL_MAX_MS);
            }
        }
    }

    pub(in crate::ui::app) fn sync_activity_slider_states(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
                self.settings
                    .general
                    .check_interval_ms
                    .clamp(CHECK_INTERVAL_MIN_MS, CHECK_INTERVAL_MAX_MS),
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

    pub(in crate::ui::app) fn is_rule_card_collapsed(&self, target: &RuleCardTarget) -> bool {
        !self.expanded_rule_cards.contains(target)
    }

    pub(in crate::ui::app) fn toggle_rule_card(
        &mut self,
        target: RuleCardTarget,
        cx: &mut Context<Self>,
    ) {
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

    pub(in crate::ui::app) fn is_setting_group_collapsed(
        &self,
        target: SettingGroupTarget,
    ) -> bool {
        !self.expanded_setting_groups.contains(&target)
    }

    pub(in crate::ui::app) fn toggle_setting_group(
        &mut self,
        target: SettingGroupTarget,
        cx: &mut Context<Self>,
    ) {
        self.set_setting_group_expanded(target, self.is_setting_group_collapsed(target));
        cx.notify();
    }

    pub(in crate::ui::app) fn set_setting_group_expanded(
        &mut self,
        target: SettingGroupTarget,
        expanded: bool,
    ) {
        let changed = if expanded {
            self.expanded_setting_groups.insert(target)
        } else {
            self.expanded_setting_groups.remove(&target)
        };

        if changed {
            begin_expandable_motion(format!("setting-group-{target:?}"), expanded);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_cpu_limiter_allowed_time_percent;

    #[test]
    fn cpu_limiter_allowed_time_input_accepts_only_one_through_ninety_nine() {
        assert_eq!(parse_cpu_limiter_allowed_time_percent("1"), Some(1));
        assert_eq!(parse_cpu_limiter_allowed_time_percent("99"), Some(99));
        assert_eq!(parse_cpu_limiter_allowed_time_percent("0"), None);
        assert_eq!(parse_cpu_limiter_allowed_time_percent("100"), None);
    }
}
