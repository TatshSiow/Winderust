pub(crate) mod dynamic_priority_boost;
pub(crate) mod gpu_priority;
pub(crate) mod io_priority;
pub(crate) mod memory_priority;
pub(crate) mod process_priority;
pub(crate) mod thread_priority;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PriorityProcessTier {
    Foreground,
    VisibleWindow,
    Background,
}

impl PriorityProcessTier {
    pub(crate) fn from_flags(foreground: bool, visible_window: bool) -> Self {
        if foreground {
            Self::Foreground
        } else if visible_window {
            Self::VisibleWindow
        } else {
            Self::Background
        }
    }

    pub(crate) fn select<T: Copy>(self, foreground: T, visible_window: T, background: T) -> T {
        match self {
            Self::Foreground => foreground,
            Self::VisibleWindow => visible_window,
            Self::Background => background,
        }
    }
}

pub(crate) fn foreground_for_owner(
    owner: crate::control::process::ControlOwner,
    process: &crate::foreground::ProcessInfo,
    path: &std::path::Path,
    foreground_id: Option<u32>,
    foreground_path: Option<&std::path::Path>,
    workload: Option<&std::collections::BTreeMap<u32, u64>>,
) -> bool {
    if owner == crate::control::process::ControlOwner::AdaptiveEngine {
        workload
            .and_then(|group| group.get(&process.id))
            .copied()
            .zip(process.creation_time)
            .is_some_and(|(expected, current)| expected == current)
    } else {
        crate::foreground::is_foreground_process(process.id, path, foreground_id, foreground_path)
    }
}

#[cfg(test)]
mod tests {
    use super::PriorityProcessTier;

    #[test]
    fn priority_tier_prefers_foreground_then_visible_window_then_background() {
        assert_eq!(
            PriorityProcessTier::from_flags(true, true).select(3, 2, 1),
            3
        );
        assert_eq!(
            PriorityProcessTier::from_flags(false, true).select(3, 2, 1),
            2
        );
        assert_eq!(
            PriorityProcessTier::from_flags(false, false).select(3, 2, 1),
            1
        );
    }
}
