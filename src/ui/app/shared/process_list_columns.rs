#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::ui::app) enum ProcessListColumn {
    Pid,
    PowerPlanForeground,
    PowerPlanRunning,
    BackgroundEfficiency,
    CoreLimiter,
    BackgroundCpuRestriction,
    CoreSteering,
    ProcessPriority,
    IoPriority,
    GpuPriority,
    MemoryPriority,
    MemoryTrim,
    AppSuspension,
    TimerResolution,
}

pub(in crate::ui::app) const PROCESS_LIST_OPTIONAL_COLUMNS: [ProcessListColumn; 14] = [
    ProcessListColumn::Pid,
    ProcessListColumn::PowerPlanForeground,
    ProcessListColumn::PowerPlanRunning,
    ProcessListColumn::BackgroundEfficiency,
    ProcessListColumn::CoreLimiter,
    ProcessListColumn::BackgroundCpuRestriction,
    ProcessListColumn::CoreSteering,
    ProcessListColumn::ProcessPriority,
    ProcessListColumn::IoPriority,
    ProcessListColumn::GpuPriority,
    ProcessListColumn::MemoryPriority,
    ProcessListColumn::MemoryTrim,
    ProcessListColumn::AppSuspension,
    ProcessListColumn::TimerResolution,
];
