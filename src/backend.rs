pub(crate) mod audio_activity;
pub(crate) mod automation;
pub(crate) mod crash_recovery;
pub(crate) mod dashboard_metrics;
pub(crate) mod diagnostics;
pub(crate) mod file_dialog;
pub(crate) mod interrupt_timing;
pub(crate) mod power_source;
pub(crate) mod privilege;
pub(crate) mod process_icon;
pub(crate) mod self_power;
pub(crate) mod startup;
pub(crate) mod tray;
pub(crate) mod update_checker;
pub(crate) mod win_registry;
pub(crate) mod win_util;
pub(crate) mod windows_events;

pub(crate) fn is_helper_process(id: u32) -> bool {
    crash_recovery::is_watchdog_process(id) || interrupt_timing::is_worker_process(id)
}
