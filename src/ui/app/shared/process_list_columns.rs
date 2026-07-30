#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::ui::app) enum ProcessListColumn {
    Pid,
    Status,
    CpuUsage,
    MemoryUsage,
    PowerPlanForeground,
    PowerPlanRunning,
    AdaptiveEngine,
    BackgroundEfficiency,
    ProcessPriority,
    ThreadPriority,
    DynamicPriorityBoost,
    IoPriority,
    GpuPriority,
    MemoryPriority,
}

pub(in crate::ui::app) const PROCESS_LIST_OVERVIEW_COLUMNS: [ProcessListColumn; 4] = [
    ProcessListColumn::Pid,
    ProcessListColumn::Status,
    ProcessListColumn::CpuUsage,
    ProcessListColumn::MemoryUsage,
];
