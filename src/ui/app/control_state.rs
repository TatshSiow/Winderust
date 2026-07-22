use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) enum SuggestionTarget {
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
pub(super) enum RuleTitleTarget {
    ByTime(usize),
    ByCpuLoad(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum RuleCardTarget {
    ByCpuLoad(usize),
    AppSuspension(String),
    CoreLimiter(String),
    CoreSteering(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum SettingGroupTarget {
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
    MemoryTrimBehaviour,
    MemoryTrimMonitoring,
    MemoryTrimSafety,
    MemoryTrimThresholds,
    MemoryTrimWhen,
    SuspensionThaw,
    SuspensionAudio,
    SuspensionNetwork,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkloadEnginePreset {
    LowImpact,
    ForegroundFirst,
    MaxForeground,
}

impl WorkloadEnginePreset {
    pub(super) const ALL: [Self; 3] = [Self::LowImpact, Self::ForegroundFirst, Self::MaxForeground];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PowerModePreset {
    PowerSave,
    Balanced,
    Performance,
    Speed,
}

impl PowerModePreset {
    pub(super) const ALL: [Self; 4] = [
        Self::PowerSave,
        Self::Balanced,
        Self::Performance,
        Self::Speed,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ThresholdField {
    Download(usize),
    Upload(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum NumericField {
    ActivityIdleTimeout,
    GeneralCheckInterval,
    ExecutionFailureSuppressionThreshold,
    BackgroundCpuRestrictionPercent,
    MemoryTrimCheckIntervalMinutes,
    MemoryTrimMemoryLoadThreshold,
    MemoryTrimWorkingSetThreshold,
    MemoryTrimCpuIdleThreshold,
    MemoryTrimIdleSeconds,
    MemoryTrimCooldownSeconds,
    MemoryTrimPurgeFreeRamThreshold,
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
pub(super) enum AdaptiveEngineProcessorPolicyField {
    CoreParkingMin,
    PerformanceMin,
    PerformanceMax,
    BoostPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ProcessorPowerSlider {
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
    pub(super) const fn paired_power_source(self) -> Self {
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
pub(super) enum ProcessorPowerSource {
    Ac,
    Dc,
}

impl ProcessorPowerSource {
    pub(super) const fn paired(self) -> Self {
        match self {
            Self::Ac => Self::Dc,
            Self::Dc => Self::Ac,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum CpuThresholdSlider {
    Lower(usize),
    Upper(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ActivitySlider {
    IdleTimeout,
    CheckInterval,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct StepChange<T> {
    pub(super) delta: T,
    pub(super) increase: bool,
}

pub(super) type StepChangeHandler<T> = Rc<dyn Fn(&StepChange<T>, &mut Window, &mut App)>;
pub(super) type BoolChangeHandler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

#[derive(Debug, Clone, Copy)]
pub(super) struct SliderRange {
    pub(super) min: u64,
    pub(super) max: u64,
    pub(super) step: u64,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct StableSliderSpec {
    pub(super) range: SliderRange,
    pub(super) enabled: bool,
    pub(super) track_color: u32,
    pub(super) thumb_color: u32,
}

pub(super) struct SliderRowSpec<'a, T> {
    pub(super) id: SharedString,
    pub(super) label: SharedString,
    pub(super) value_element: AnyElement,
    pub(super) state: &'a Entity<SliderState>,
    pub(super) enabled: bool,
    pub(super) delta: T,
}

pub(super) struct ActivitySliderCardSpec<'a> {
    pub(super) id: SharedString,
    pub(super) label: SharedString,
    pub(super) value_element: AnyElement,
    pub(super) state: &'a Entity<SliderState>,
    pub(super) enabled: bool,
    pub(super) range: SliderRange,
}

pub(super) struct SettingGroupBody {
    pub(super) collapsed: bool,
    pub(super) rows: Vec<AnyElement>,
    pub(super) animation_height: Option<f32>,
}

pub(super) fn make_input(
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

pub(super) fn make_percent_slider(
    cx: &mut Context<WinderustApp>,
    value: u64,
) -> Entity<SliderState> {
    make_range_slider(cx, value, 0, 100, 1)
}

pub(super) fn make_range_slider(
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

pub(super) fn make_processor_power_slider(
    cx: &mut Context<WinderustApp>,
    value: u64,
) -> Entity<SliderState> {
    make_percent_slider(cx, value)
}

pub(super) fn processor_power_slider_input(
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

pub(super) fn cpu_threshold_slider_input(
    inputs: &UiInputs,
    slider: CpuThresholdSlider,
) -> Option<Entity<SliderState>> {
    match slider {
        CpuThresholdSlider::Lower(index) => inputs.cpu_rule_thresholds.get(index),
        CpuThresholdSlider::Upper(index) => inputs.cpu_rule_upper_thresholds.get(index),
    }
    .cloned()
}

pub(super) fn sync_input_vec(
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

pub(super) fn sync_slider_vec(
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

pub(super) fn clear_input(
    input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
) {
    clear_input_to(input, "", window, cx);
}

pub(super) fn set_input_placeholder(
    input: &Entity<InputState>,
    placeholder: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
) {
    input.update(cx, |input, cx| {
        input.set_placeholder(placeholder, window, cx)
    });
}

pub(super) fn clear_input_to(
    input: &Entity<InputState>,
    value: &str,
    window: &mut Window,
    cx: &mut Context<WinderustApp>,
) {
    let value = SharedString::from(value.to_owned());
    input.update(cx, |input, cx| input.set_value(value, window, cx));
}
