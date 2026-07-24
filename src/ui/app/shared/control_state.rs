use crate::ui::app::*;

#[derive(Debug, Clone, Copy)]
pub(in crate::ui::app) enum SuggestionTarget {
    Foreground,
    BackgroundEfficiency,
    BackgroundCpu,
    MemoryTrim,
    AppSuspension,
    CoreLimiter,
    ByRunningApp,
    WorkloadEngine,
    ProcessPriority,
    ThreadPriority,
    DynamicPriorityBoost,
    IoPriority,
    GpuPriority,
    MemoryPriority,
    TimerResolution,
    CoreSteering,
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
    CoreLimiter(String),
    CoreSteering(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum SettingGroupTarget {
    AccentColor,
    AdaptiveEngineCpuScheduling,
    AdaptiveEngineProcessorPolicy,
    WorkloadEngineAffinity,
    WorkloadEngineBehaviourTuning,
    WorkloadEngineEfficiency,
    WorkloadEngineGpuPriority,
    WorkloadEngineIoPriority,
    WorkloadEngineMemoryPriority,
    WorkloadEngineDynamicPriorityBoost,
    WorkloadEngineProcessPriority,
    WorkloadEngineThreadPriority,
    ProcessPriorityMaster,
    ProcessPriorityForegroundDetection,
    ThreadPriorityMaster,
    ThreadPriorityForegroundDetection,
    DynamicPriorityBoostMaster,
    DynamicPriorityBoostForegroundDetection,
    IoPriorityMaster,
    IoPriorityForegroundDetection,
    EfficiencyEnable,
    BackgroundCpuRestriction,
    GpuPriorityMaster,
    GpuPriorityForegroundDetection,
    MemoryPriorityMaster,
    MemoryPriorityForegroundDetection,
    MemoryTrimMonitoring,
    MemoryTrimSafety,
    MemoryTrimThresholds,
    MemoryTrimWhen,
    SuspensionThaw,
    SuspensionAudio,
    SuspensionNetwork,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui::app) enum WorkloadEnginePreset {
    LowImpact,
    ForegroundFirst,
    MaxForeground,
}

impl WorkloadEnginePreset {
    pub(in crate::ui::app) const ALL: [Self; 3] =
        [Self::LowImpact, Self::ForegroundFirst, Self::MaxForeground];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui::app) enum PowerModePreset {
    PowerSave,
    Balanced,
    Performance,
    Speed,
}

impl PowerModePreset {
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
    BackgroundCpuRestrictionPercent,
    MemoryTrimMemoryLoadThreshold,
    MemoryTrimWorkingSetThreshold,
    MemoryTrimIdleSeconds,
    SuspensionBackgroundDelay,
    SuspensionThawInterval,
    SuspensionThawDuration,
    SuspensionAudioRefreeze,
    SuspensionNetworkRefreeze,
    WorkloadEngineTotalThreshold,
    WorkloadEngineThreshold,
    WorkloadEngineRestoreThreshold,
    WorkloadEngineCpuPercent,
    WorkloadEngineSustain,
    WorkloadEngineMinimumRestraint,
    WorkloadEngineCooldown,
    WorkloadEngineMaxTargetedProcesses,
    ProcessorAcCoreParkingMin,
    ProcessorAcPerformanceMin,
    ProcessorAcPerformanceMax,
    ProcessorAcBoostPolicy,
    ProcessorDcCoreParkingMin,
    ProcessorDcPerformanceMin,
    ProcessorDcPerformanceMax,
    ProcessorDcBoostPolicy,
    CpuThreshold(usize),
    CpuUpperThreshold(usize),
    CpuDuration(usize),
    CoreLimiterThreshold(usize),
    CoreLimiterSustain(usize),
    CoreLimiterCooldown(usize),
    CoreLimiterMaxProcessors(usize),
    TimerResolutionRule(usize),
    NetworkThreshold(ThresholdField),
    AdaptiveEngineProcessorPolicy(AdaptiveEngineProcessorPolicyField),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum AdaptiveEngineProcessorPolicyField {
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
    DcCoreParkingMin,
    DcPerformanceMin,
    DcPerformanceMax,
    DcBoostPolicy,
}

impl ProcessorPowerSlider {
    pub(in crate::ui::app) const fn paired_power_source(self) -> Self {
        match self {
            Self::AcCoreParkingMin => Self::DcCoreParkingMin,
            Self::AcPerformanceMin => Self::DcPerformanceMin,
            Self::AcPerformanceMax => Self::DcPerformanceMax,
            Self::AcBoostPolicy => Self::DcBoostPolicy,
            Self::DcCoreParkingMin => Self::AcCoreParkingMin,
            Self::DcPerformanceMin => Self::AcPerformanceMin,
            Self::DcPerformanceMax => Self::AcPerformanceMax,
            Self::DcBoostPolicy => Self::AcBoostPolicy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::ui::app) enum ProcessorPowerSource {
    Ac,
    Dc,
}

impl ProcessorPowerSource {
    pub(in crate::ui::app) const fn paired(self) -> Self {
        match self {
            Self::Ac => Self::Dc,
            Self::Dc => Self::Ac,
        }
    }
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
        ProcessorPowerSlider::DcCoreParkingMin => {
            inputs.processor_power_dc_core_parking_min.clone()
        }
        ProcessorPowerSlider::DcPerformanceMin => inputs.processor_power_dc_performance_min.clone(),
        ProcessorPowerSlider::DcPerformanceMax => inputs.processor_power_dc_performance_max.clone(),
        ProcessorPowerSlider::DcBoostPolicy => inputs.processor_power_dc_boost_policy.clone(),
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
    value_at: impl Fn(usize) -> u64,
) {
    while inputs.len() < len {
        let index = inputs.len();
        inputs.push(make_percent_slider(cx, value_at(index)));
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
        self.expanded_rule_cards.clear();
        self.pending_list_item_removals.clear();
        self.inputs = UiInputs::new(window, cx, &settings, processor_power_values);
        self.rebuild_rule_title_input_subscriptions(window, cx);
        self.subscribe_to_numeric_input(window, cx);
        self.subscribe_to_dashboard_search_input(window, cx);
        self.subscribe_to_processor_power_sliders(window, cx);
        self.rebuild_cpu_threshold_slider_subscriptions(window, cx);
        self.subscribe_to_activity_sliders(window, cx);
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

    pub(in crate::ui::app) fn subscribe_to_dashboard_search_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._dashboard_search_subscription = Some(cx.subscribe_in(
            &self.inputs.dashboard_search,
            window,
            move |_, _, _: &InputEvent, _, cx| {
                cx.notify();
            },
        ));
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
            NumericField::BackgroundCpuRestrictionPercent => {
                if let Some(value) = parse_u64_input(&value, 1, 100) {
                    self.settings.background_cpu_restriction.percent = value as u8;
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
}

impl WinderustApp {
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
    use super::*;

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
}
