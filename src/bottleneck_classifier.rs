use crate::{cpu::PerProcessorUsageMonitor, platform::windows::gpu_usage::GpuUsageQuery};

const CPU_BUSY_PERCENT: f32 = 85.0;
const BUSIEST_CPU_BUSY_PERCENT: f32 = 90.0;
const GPU_BUSY_PERCENT: f32 = 75.0;
const STABLE_SAMPLE_COUNT: u8 = 3;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BottleneckState {
    #[default]
    Unknown,
    Headroom,
    CpuBound,
    GpuBound,
    Mixed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BottleneckSnapshot {
    pub enabled: bool,
    pub state: BottleneckState,
    pub total_cpu_tenths: Option<u16>,
    pub busiest_cpu_tenths: Option<u16>,
    pub busiest_gpu_tenths: Option<u16>,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub(crate) struct BottleneckClassifier {
    processors: PerProcessorUsageMonitor,
    gpu: Option<GpuUsageQuery>,
    candidate: BottleneckState,
    candidate_samples: u8,
    stable: BottleneckState,
}

impl BottleneckClassifier {
    pub(crate) fn reset(&mut self) -> BottleneckSnapshot {
        self.gpu = None;
        self.candidate = BottleneckState::Unknown;
        self.candidate_samples = 0;
        self.stable = BottleneckState::Unknown;
        BottleneckSnapshot::default()
    }

    pub(crate) fn sample(&mut self, total_cpu: Option<f32>) -> BottleneckSnapshot {
        let busiest_cpu = self
            .processors
            .sample()
            .and_then(|usage| usage.into_iter().reduce(f32::max));
        let gpu = self.gpu_sample();
        let (busiest_gpu, last_error) = match gpu {
            Ok(value) => (value, None),
            Err(error) => (None, Some(error)),
        };
        let raw = total_cpu
            .map(|total_cpu| classify(total_cpu, busiest_cpu, busiest_gpu))
            .unwrap_or(BottleneckState::Unknown);
        self.stabilize(raw);

        BottleneckSnapshot {
            enabled: true,
            state: self.stable,
            total_cpu_tenths: total_cpu.and_then(tenths),
            busiest_cpu_tenths: busiest_cpu.and_then(tenths),
            busiest_gpu_tenths: busiest_gpu.and_then(tenths),
            last_error,
        }
    }

    fn stabilize(&mut self, raw: BottleneckState) {
        if raw == self.candidate {
            self.candidate_samples = self.candidate_samples.saturating_add(1);
        } else {
            self.candidate = raw;
            self.candidate_samples = 1;
        }
        if self.candidate_samples >= STABLE_SAMPLE_COUNT {
            self.stable = raw;
        }
    }

    fn gpu_sample(&mut self) -> Result<Option<f32>, String> {
        if self.gpu.is_none() {
            self.gpu = Some(GpuUsageQuery::open().map_err(|error| error.to_string())?);
        }
        let Some(gpu) = self.gpu.as_ref() else {
            return Err("GPU query was not initialized.".to_owned());
        };
        let result = gpu.sample().map_err(|error| error.to_string());
        if result.is_err() {
            self.gpu = None;
        }
        result
    }
}

fn tenths(value: f32) -> Option<u16> {
    value
        .is_finite()
        .then(|| (value.clamp(0.0, 100.0) * 10.0).round() as u16)
}

fn classify(total_cpu: f32, busiest_cpu: Option<f32>, gpu: Option<f32>) -> BottleneckState {
    let Some(gpu) = gpu else {
        return BottleneckState::Unknown;
    };
    if total_cpu >= CPU_BUSY_PERCENT && gpu >= GPU_BUSY_PERCENT {
        BottleneckState::Mixed
    } else if gpu >= GPU_BUSY_PERCENT {
        BottleneckState::GpuBound
    } else if total_cpu >= CPU_BUSY_PERCENT
        || busiest_cpu.is_some_and(|usage| usage >= BUSIEST_CPU_BUSY_PERCENT)
    {
        BottleneckState::CpuBound
    } else {
        BottleneckState::Headroom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_covers_validated_boundaries() {
        assert_eq!(
            classify(20.0, Some(30.0), Some(10.0)),
            BottleneckState::Headroom
        );
        assert_eq!(
            classify(86.0, Some(90.0), Some(40.0)),
            BottleneckState::CpuBound
        );
        assert_eq!(
            classify(30.0, Some(40.0), Some(80.0)),
            BottleneckState::GpuBound
        );
        assert_eq!(
            classify(90.0, Some(95.0), Some(80.0)),
            BottleneckState::Mixed
        );
        assert_eq!(classify(90.0, Some(95.0), None), BottleneckState::Unknown);
    }

    #[test]
    fn state_changes_only_after_three_matching_samples() {
        let mut classifier = BottleneckClassifier::default();
        classifier.stabilize(BottleneckState::GpuBound);
        classifier.stabilize(BottleneckState::GpuBound);
        assert_eq!(classifier.stable, BottleneckState::Unknown);
        classifier.stabilize(BottleneckState::GpuBound);
        assert_eq!(classifier.stable, BottleneckState::GpuBound);
        classifier.stabilize(BottleneckState::CpuBound);
        assert_eq!(classifier.stable, BottleneckState::GpuBound);
    }
}
