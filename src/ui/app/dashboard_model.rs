use crate::ui::app::*;

pub(in crate::ui::app) struct DashboardModel {
    pub(in crate::ui::app) cpu: CpuUsageSnapshot,
    pub(in crate::ui::app) cpu_history: VecDeque<CpuUsageHistorySample>,
    pub(in crate::ui::app) memory: MemoryUsageSnapshot,
    pub(in crate::ui::app) memory_history: VecDeque<MemoryUsageHistorySample>,
    pub(in crate::ui::app) io: IoUsageSnapshot,
    pub(in crate::ui::app) io_history: VecDeque<IoUsageHistorySample>,
    pub(in crate::ui::app) network: NetworkUsageSnapshot,
    pub(in crate::ui::app) network_history: VecDeque<NetworkUsageHistorySample>,
}

impl DashboardModel {
    pub(in crate::ui::app) fn new() -> Self {
        Self {
            cpu: CpuUsageSnapshot::default(),
            cpu_history: VecDeque::with_capacity(DASHBOARD_HISTORY_LEN),
            memory: MemoryUsageSnapshot::default(),
            memory_history: VecDeque::with_capacity(DASHBOARD_HISTORY_LEN),
            io: IoUsageSnapshot::default(),
            io_history: VecDeque::with_capacity(DASHBOARD_HISTORY_LEN),
            network: NetworkUsageSnapshot::default(),
            network_history: VecDeque::with_capacity(DASHBOARD_HISTORY_LEN),
        }
    }

    pub(in crate::ui::app) fn record_cpu_memory(
        &mut self,
        cpu: CpuUsageSnapshot,
        memory: MemoryUsageSnapshot,
    ) -> bool {
        let mut changed = self.cpu != cpu || self.memory != memory;
        self.cpu = cpu;
        self.memory = memory;

        if let Some(percent) = cpu.percent {
            push_bounded(
                &mut self.cpu_history,
                CpuUsageHistorySample {
                    percent: percent.clamp(0.0, 100.0),
                    frequency_mhz: cpu.frequency_mhz,
                },
            );
            changed = true;
        }
        if let Some(percent) = memory.percent {
            push_bounded(
                &mut self.memory_history,
                MemoryUsageHistorySample {
                    usage_percent: percent.clamp(0.0, 100.0),
                    cache_percent: memory_cache_percent(memory).unwrap_or(0.0),
                },
            );
            changed = true;
        }

        changed
    }

    pub(in crate::ui::app) fn record_io_network(
        &mut self,
        io: IoUsageSnapshot,
        network: NetworkUsageSnapshot,
    ) -> bool {
        let mut changed = self.io != io || self.network != network;
        self.io = io;
        self.network = network;

        if io.bytes_per_second.is_some() {
            push_bounded(
                &mut self.io_history,
                IoUsageHistorySample {
                    read_bytes_per_second: rate_as_f32(io.read_bytes_per_second),
                    write_bytes_per_second: rate_as_f32(io.write_bytes_per_second),
                },
            );
            changed = true;
        }
        if network.bytes_per_second.is_some() {
            push_bounded(
                &mut self.network_history,
                NetworkUsageHistorySample {
                    download_bytes_per_second: rate_as_f32(network.download_bytes_per_second),
                    upload_bytes_per_second: rate_as_f32(network.upload_bytes_per_second),
                },
            );
            changed = true;
        }

        changed
    }
}

fn push_bounded<T>(history: &mut VecDeque<T>, sample: T) {
    if history.len() == DASHBOARD_HISTORY_LEN {
        history.pop_front();
    }
    history.push_back(sample);
}

fn rate_as_f32(value: Option<f64>) -> f32 {
    value.unwrap_or(0.0).clamp(0.0, f32::MAX as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histories_keep_only_the_latest_dashboard_window() {
        let mut model = DashboardModel::new();

        for index in 0..DASHBOARD_HISTORY_LEN + 3 {
            let percent = index as f32;
            assert!(model.record_cpu_memory(
                CpuUsageSnapshot {
                    percent: Some(percent),
                    frequency_mhz: Some(index as u32),
                    base_frequency_mhz: Some(1_000),
                },
                MemoryUsageSnapshot {
                    percent: Some(percent),
                    cached_physical_bytes: Some(index as u64),
                    total_physical_bytes: Some(100),
                    ..Default::default()
                },
            ));
            assert!(model.record_io_network(
                IoUsageSnapshot {
                    bytes_per_second: Some(percent as f64),
                    read_bytes_per_second: Some(percent as f64),
                    write_bytes_per_second: Some(percent as f64),
                },
                NetworkUsageSnapshot {
                    bytes_per_second: Some(percent as f64),
                    download_bytes_per_second: Some(percent as f64),
                    upload_bytes_per_second: Some(percent as f64),
                },
            ));
        }

        assert_eq!(model.cpu_history.len(), DASHBOARD_HISTORY_LEN);
        assert_eq!(model.memory_history.len(), DASHBOARD_HISTORY_LEN);
        assert_eq!(model.io_history.len(), DASHBOARD_HISTORY_LEN);
        assert_eq!(model.network_history.len(), DASHBOARD_HISTORY_LEN);
        assert_eq!(
            model.cpu_history.front().map(|sample| sample.percent),
            Some(3.0)
        );
    }

    #[test]
    fn unavailable_samples_do_not_extend_histories() {
        let mut model = DashboardModel::new();

        assert!(
            !model.record_cpu_memory(CpuUsageSnapshot::default(), MemoryUsageSnapshot::default(),)
        );
        assert!(
            !model.record_io_network(IoUsageSnapshot::default(), NetworkUsageSnapshot::default(),)
        );
        assert!(model.cpu_history.is_empty());
        assert!(model.memory_history.is_empty());
        assert!(model.io_history.is_empty());
        assert!(model.network_history.is_empty());
    }
}
