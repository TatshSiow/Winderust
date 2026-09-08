use super::navigation::{dashboard_search_pages, dashboard_sections_in_nav_order};
use crate::automation::RuntimeFeatureStatus;
use crate::backend::dashboard_metrics::{
    sample_memory_usage, IoUsageMonitor, IoUsageSnapshot, MemoryUsageSnapshot, NetworkUsageMonitor,
    NetworkUsageSnapshot,
};
use crate::config::Settings;
use crate::cpu::{CpuUsageMonitor, CpuUsageSnapshot};
use crate::ui::Page;
use iced::widget::{button, canvas, column, container, row, scrollable, text, text_input};
use iced::{mouse, Element, Fill, Point, Rectangle, Renderer, Theme};
use rust_i18n::t;
use std::collections::VecDeque;

const HISTORY_LEN: usize = 30;
#[derive(Debug)]
pub(super) struct Sampler {
    channels: Result<
        (
            std::sync::mpsc::Sender<()>,
            std::sync::mpsc::Receiver<Sample>,
        ),
        String,
    >,
}
impl Default for Sampler {
    fn default() -> Self {
        let (request, requests) = std::sync::mpsc::channel();
        let (samples, response) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("winderust-dashboard".into())
            .spawn(move || {
                // PDH handles stay owned, sampled, and dropped on this worker thread.
                let mut cpu = CpuUsageMonitor::default();
                let mut io = IoUsageMonitor::default();
                let mut network = NetworkUsageMonitor::default();
                let mut controller = crate::activity::ControllerActivityDetector::default();
                while requests.recv().is_ok() {
                    let now = std::time::Instant::now();
                    controller.poll(now);
                    let sample = Sample {
                        cpu: cpu.sample(),
                        memory: sample_memory_usage(),
                        io: io.sample(),
                        network: network.sample(),
                        input_idle: crate::activity::input_tracker::last_input_elapsed(),
                        controller_idle: controller.idle_for(now),
                    };
                    if samples.send(sample).is_err() {
                        break;
                    }
                }
            });
        Self {
            channels: worker
                .map(|_| (request, response))
                .map_err(|error| error.to_string()),
        }
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Sample {
    cpu: CpuUsageSnapshot,
    memory: MemoryUsageSnapshot,
    io: IoUsageSnapshot,
    network: NetworkUsageSnapshot,
    input_idle: Option<std::time::Duration>,
    controller_idle: Option<std::time::Duration>,
}
impl Sampler {
    // Call from an application task once per second while Home sampling is enabled.
    // Dropping Sampler disconnects requests and lets its worker drop all native monitors.
    pub(super) fn sample(&mut self) -> Result<Sample, String> {
        let (request, response) = self.channels.as_ref().map_err(Clone::clone)?;
        request.send(()).map_err(|error| error.to_string())?;
        response.recv().map_err(|error| error.to_string())
    }
}
#[derive(Default)]
pub(super) struct Model {
    latest: Sample,
    cpu: VecDeque<CpuUsageSnapshot>,
    memory: VecDeque<MemoryUsageSnapshot>,
    io: VecDeque<IoUsageSnapshot>,
    network: VecDeque<NetworkUsageSnapshot>,
    search: String,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Search(String),
    Navigate(Page),
}
impl Model {
    pub(super) fn update(&mut self, message: Message) {
        match message {
            Message::Search(search) => self.search = search,
            Message::Navigate(_) => self.search.clear(),
        }
    }
    pub(super) fn record(&mut self, sample: Sample) {
        self.latest = sample;
        if sample.cpu.percent.is_some() {
            push_bounded(&mut self.cpu, sample.cpu);
        }
        if sample.memory.percent.is_some() {
            push_bounded(&mut self.memory, sample.memory);
        }
        if sample.io.bytes_per_second.is_some() {
            push_bounded(&mut self.io, sample.io);
        }
        if sample.network.bytes_per_second.is_some() {
            push_bounded(&mut self.network, sample.network);
        }
    }
    pub(super) fn view<'a>(
        &'a self,
        settings: &'a Settings,
        status: &'a RuntimeFeatureStatus,
    ) -> Element<'a, Message> {
        let mut body = column![
            text_input(&t!("home.search_placeholder"), &self.search).on_input(Message::Search)
        ]
        .spacing(12);
        if !self.search.trim().is_empty() {
            let pages =
                dashboard_search_pages(&self.search, settings.advanced.show_advanced_controls);
            if pages.is_empty() {
                body = body.push(text(t!("home.no_matching_functions").to_string()));
            }
            for page in pages {
                body = body.push(
                    button(text(page.label()))
                        .on_press(Message::Navigate(page))
                        .width(Fill),
                );
            }
            return scrollable(body).spacing(10).height(Fill).into();
        }
        let cpu = self.chart(ChartKind::Cpu);
        let memory = self.chart(ChartKind::Memory);
        let io = self.chart(ChartKind::Io);
        let network = self.chart(ChartKind::Network);
        body = body
            .push(
                row![
                    self.chart_card(ChartKind::Cpu, cpu),
                    self.chart_card(ChartKind::Memory, memory)
                ]
                .spacing(12),
            )
            .push(
                row![
                    self.chart_card(ChartKind::Io, io),
                    self.chart_card(ChartKind::Network, network)
                ]
                .spacing(12),
            );
        let mut enabled = column![
            text(t!("home.enabled_features").to_string()).size(18),
            text(
                if settings.general.enabled {
                    t!("home.master_switch_enabled")
                } else {
                    t!("home.master_switch_disabled")
                }
                .to_string()
            )
        ]
        .spacing(8);
        let mut count = 0;
        for (page, active, detail) in enabled_features(settings, status, &self.latest) {
            if active {
                count += 1;
                enabled = enabled.push(
                    button(row![text(page.label()).width(Fill), text(detail)].spacing(8))
                        .on_press(Message::Navigate(page))
                        .width(Fill),
                );
            }
        }
        if count == 0 {
            enabled = enabled.push(text(t!("home.no_enabled_features").to_string()));
        }
        body = body
            .push(
                container(enabled)
                    .padding(16)
                    .width(Fill)
                    .style(iced::widget::container::bordered_box),
            )
            .push(text(t!("home.main_sections").to_string()).size(18));
        for section in dashboard_sections_in_nav_order(settings.advanced.show_advanced_controls) {
            body = body.push(
                button(text(section.landing_page.label()))
                    .style(iced::widget::button::secondary)
                    .padding([10, 14])
                    .on_press(Message::Navigate(section.landing_page))
                    .width(Fill),
            );
        }
        scrollable(body).spacing(10).height(Fill).into()
    }
    fn chart_card(&self, kind: ChartKind, chart: Chart) -> Element<'static, Message> {
        let sample = self.latest;
        let (title, first, second, first_value, second_value) = match kind {
            ChartKind::Cpu => (
                t!("home.by_cpu_load"),
                t!("home.cpu_load"),
                t!("home.cpu_frequency"),
                cpu_usage_label(sample.cpu.percent),
                cpu_frequency_label(sample.cpu.frequency_mhz),
            ),
            ChartKind::Memory => (
                t!("home.memory_usage"),
                t!("home.memory_used"),
                t!("home.memory_cache"),
                memory_usage_value_label(sample.memory),
                memory_cache_value_label(sample.memory),
            ),
            ChartKind::Io => (
                t!("home.io_usage"),
                t!("home.io_read"),
                t!("home.io_write"),
                io_usage_label(sample.io.read_bytes_per_second),
                io_usage_label(sample.io.write_bytes_per_second),
            ),
            ChartKind::Network => (
                t!("home.network_usage"),
                t!("home.network_download"),
                t!("home.network_upload"),
                io_usage_label(sample.network.download_bytes_per_second),
                io_usage_label(sample.network.upload_bytes_per_second),
            ),
        };
        let total = match kind {
            ChartKind::Cpu => cpu_usage_label(sample.cpu.percent),
            ChartKind::Memory => memory_usage_label(sample.memory.percent),
            ChartKind::Io => io_usage_label(sample.io.bytes_per_second),
            ChartKind::Network => io_usage_label(sample.network.bytes_per_second),
        };
        container(
            column![
                row![
                    text(title.to_string()).size(18).width(Fill),
                    text(total).size(20)
                ]
                .spacing(8),
                row![
                    text(format!("{first}: {first_value}"))
                        .style(text::primary)
                        .width(Fill),
                    text(format!("{second}: {second_value}"))
                        .style(text::success)
                        .width(Fill)
                ]
                .spacing(8),
                canvas(chart).width(Fill).height(82)
            ]
            .spacing(8),
        )
        .padding(16)
        .width(Fill)
        .style(iced::widget::container::bordered_box)
        .into()
    }
    fn chart(&self, kind: ChartKind) -> Chart {
        let samples: Vec<ChartSample> = match kind {
            ChartKind::Cpu => {
                let base = self
                    .latest
                    .cpu
                    .base_frequency_mhz
                    .or_else(|| self.cpu.iter().filter_map(|s| s.frequency_mhz).min())
                    .unwrap_or(0);
                let peak = self
                    .cpu
                    .iter()
                    .filter_map(|s| s.frequency_mhz)
                    .max()
                    .filter(|peak| *peak > base);
                self.cpu
                    .iter()
                    .map(|s| ChartSample {
                        values: [
                            s.percent.unwrap_or(0.0) as f64,
                            normalize_cpu_frequency_percent(s.frequency_mhz, base, peak) as f64,
                        ],
                        labels: [
                            cpu_usage_label(s.percent),
                            cpu_frequency_label(s.frequency_mhz),
                        ],
                    })
                    .collect()
            }
            ChartKind::Memory => self
                .memory
                .iter()
                .map(|s| ChartSample {
                    values: [
                        s.percent.unwrap_or(0.0) as f64,
                        memory_cache_percent(*s).unwrap_or(0.0) as f64,
                    ],
                    labels: [
                        memory_usage_label(s.percent),
                        memory_usage_label(memory_cache_percent(*s)),
                    ],
                })
                .collect(),
            ChartKind::Io => self
                .io
                .iter()
                .map(|s| ChartSample {
                    values: [
                        s.read_bytes_per_second.unwrap_or(0.0),
                        s.write_bytes_per_second.unwrap_or(0.0),
                    ],
                    labels: [
                        io_usage_label(s.read_bytes_per_second),
                        io_usage_label(s.write_bytes_per_second),
                    ],
                })
                .collect(),
            ChartKind::Network => self
                .network
                .iter()
                .map(|s| ChartSample {
                    values: [
                        s.download_bytes_per_second.unwrap_or(0.0),
                        s.upload_bytes_per_second.unwrap_or(0.0),
                    ],
                    labels: [
                        io_usage_label(s.download_bytes_per_second),
                        io_usage_label(s.upload_bytes_per_second),
                    ],
                })
                .collect(),
        };
        let maximum = match kind {
            ChartKind::Cpu | ChartKind::Memory => 100.0,
            _ => samples.iter().flat_map(|s| s.values).fold(1.0, f64::max),
        };
        Chart { samples, maximum }
    }
}
fn push_bounded<T>(history: &mut VecDeque<T>, sample: T) {
    if history.len() == HISTORY_LEN {
        history.pop_front();
    }
    history.push_back(sample);
}
#[derive(Clone, Copy)]
enum ChartKind {
    Cpu,
    Memory,
    Io,
    Network,
}
struct ChartSample {
    values: [f64; 2],
    labels: [String; 2],
}
struct Chart {
    samples: Vec<ChartSample>,
    maximum: f64,
}
impl canvas::Program<Message> for Chart {
    type State = ();
    fn update(
        &self,
        _: &mut (),
        event: &canvas::Event,
        _: Rectangle,
        _: mouse::Cursor,
    ) -> Option<iced::widget::Action<Message>> {
        matches!(
            event,
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft)
        )
        .then(iced::widget::Action::request_redraw)
    }
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let height = (bounds.height - 28.0).max(1.0);
        let width = bounds.width.max(1.0);
        let grid = theme.extended_palette().background.strong.color;
        for fraction in [0.0, 0.5, 1.0] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(0.0, height * fraction),
                    Point::new(width, height * fraction),
                ),
                canvas::Stroke::default().with_color(grid),
            );
        }
        for series in 0..2 {
            let path = canvas::Path::new(|path| {
                for (index, sample) in self.samples.iter().enumerate() {
                    let x = (HISTORY_LEN - self.samples.len() + index) as f32
                        / (HISTORY_LEN - 1) as f32
                        * width;
                    let y = height
                        * (1.0 - (sample.values[series] / self.maximum).clamp(0.0, 1.0) as f32);
                    if index == 0 {
                        path.move_to(Point::new(x, y));
                    } else {
                        path.line_to(Point::new(x, y));
                    }
                }
            });
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_width(1.8)
                    .with_color(if series == 0 {
                        theme.palette().primary
                    } else {
                        theme.palette().success
                    }),
            );
        }
        if let Some(cursor) = cursor.position_in(bounds) {
            let slot =
                ((cursor.x / width).clamp(0.0, 1.0) * (HISTORY_LEN - 1) as f32).round() as usize;
            if let Some(index) = slot.checked_sub(HISTORY_LEN - self.samples.len()) {
                if let Some(sample) = self.samples.get(index) {
                    let x = slot as f32 / (HISTORY_LEN - 1) as f32 * width;
                    frame.stroke(
                        &canvas::Path::line(Point::new(x, 0.0), Point::new(x, height)),
                        canvas::Stroke::default().with_color(theme.palette().text),
                    );
                    let age = HISTORY_LEN - slot - 1;
                    let age = if age == 0 {
                        t!("common.latest_sample").to_string()
                    } else {
                        t!("common.seconds_ago", count = age).to_string()
                    };
                    frame.fill_text(canvas::Text {
                        content: format!("{age}: {} / {}", sample.labels[0], sample.labels[1]),
                        position: Point::new(2.0, height + 5.0),
                        color: theme.palette().text,
                        size: 11.0.into(),
                        ..Default::default()
                    });
                }
            }
        }
        vec![frame.into_geometry()]
    }
}
fn cpu_usage_label(percent: Option<f32>) -> String {
    percent
        .map(|percent| format!("{percent:.1}%"))
        .unwrap_or_else(|| t!("home.collecting").to_string())
}

fn cpu_frequency_label(frequency_mhz: Option<u32>) -> String {
    frequency_mhz
        .map(|frequency_mhz| {
            if frequency_mhz >= 1_000 {
                format!("{:.2} GHz", frequency_mhz as f64 / 1_000.0)
            } else {
                format!("{frequency_mhz} MHz")
            }
        })
        .unwrap_or_else(|| t!("home.collecting").to_string())
}

fn memory_usage_label(percent: Option<f32>) -> String {
    percent
        .map(|percent| format!("{percent:.1}%"))
        .unwrap_or_else(|| t!("home.collecting").to_string())
}

fn memory_usage_value_label(snapshot: MemoryUsageSnapshot) -> String {
    match (snapshot.used_physical_bytes, snapshot.total_physical_bytes) {
        (Some(used), Some(total)) => format_memory_used_total(used, total),
        _ => t!("home.collecting").to_string(),
    }
}

fn memory_cache_value_label(snapshot: MemoryUsageSnapshot) -> String {
    snapshot
        .cached_physical_bytes
        .map(format_memory_capacity)
        .unwrap_or_else(|| t!("home.collecting").to_string())
}

fn memory_cache_percent(snapshot: MemoryUsageSnapshot) -> Option<f32> {
    memory_bytes_percent(
        snapshot.cached_physical_bytes,
        snapshot.total_physical_bytes,
    )
}

fn memory_bytes_percent(bytes: Option<u64>, total_bytes: Option<u64>) -> Option<f32> {
    let bytes = bytes?;
    let total_bytes = total_bytes?;
    if total_bytes == 0 {
        return None;
    }

    Some(((bytes as f64 / total_bytes as f64) * 100.0).clamp(0.0, 100.0) as f32)
}

fn io_usage_label(bytes_per_second: Option<f64>) -> String {
    bytes_per_second
        .map(format_bytes_per_second)
        .unwrap_or_else(|| t!("home.collecting").to_string())
}

fn format_memory_used_total(used_bytes: u64, total_bytes: u64) -> String {
    let used = memory_capacity_parts(used_bytes);
    let total = memory_capacity_parts(total_bytes);

    if used.unit == total.unit && used.unit != "B" {
        format!(
            "{} / {} {}",
            format_capacity_number(used.value),
            format_capacity_number(total.value),
            used.unit
        )
    } else {
        format!(
            "{} / {}",
            format_memory_capacity(used_bytes),
            format_memory_capacity(total_bytes)
        )
    }
}

fn format_memory_capacity(bytes: u64) -> String {
    let capacity = memory_capacity_parts(bytes);
    if capacity.unit == "B" {
        format!("{} B", bytes)
    } else {
        format!(
            "{} {}",
            format_capacity_number(capacity.value),
            capacity.unit
        )
    }
}

fn format_capacity_number(value: f64) -> String {
    format!("{value:.1}")
}

fn memory_capacity_parts(bytes: u64) -> MemoryCapacityParts {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    const TIB: f64 = GIB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= TIB {
        MemoryCapacityParts {
            value: bytes / TIB,
            unit: "TB",
        }
    } else if bytes >= GIB {
        MemoryCapacityParts {
            value: bytes / GIB,
            unit: "GB",
        }
    } else if bytes >= MIB {
        MemoryCapacityParts {
            value: bytes / MIB,
            unit: "MB",
        }
    } else if bytes >= KIB {
        MemoryCapacityParts {
            value: bytes / KIB,
            unit: "KB",
        }
    } else {
        MemoryCapacityParts {
            value: bytes,
            unit: "B",
        }
    }
}

fn format_bytes_per_second(bytes_per_second: f64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    if bytes_per_second >= GIB {
        format!("{:.1} GB/s", bytes_per_second / GIB)
    } else if bytes_per_second >= MIB {
        format!("{:.1} MB/s", bytes_per_second / MIB)
    } else if bytes_per_second >= KIB {
        format!("{:.1} KB/s", bytes_per_second / KIB)
    } else {
        format!("{bytes_per_second:.0} B/s")
    }
}
struct MemoryCapacityParts {
    value: f64,
    unit: &'static str,
}
fn normalize_cpu_frequency_percent(
    frequency_mhz: Option<u32>,
    base_frequency_mhz: u32,
    peak_frequency_mhz: Option<u32>,
) -> f32 {
    let Some(frequency_mhz) = frequency_mhz else {
        return 0.0;
    };
    let Some(peak_frequency_mhz) = peak_frequency_mhz else {
        return 0.0;
    };
    let range = peak_frequency_mhz.saturating_sub(base_frequency_mhz);
    if range == 0 || frequency_mhz <= base_frequency_mhz {
        return 0.0;
    }

    ((frequency_mhz.saturating_sub(base_frequency_mhz) as f32 / range as f32) * 100.0)
        .clamp(0.0, 100.0)
}

fn enabled_features(
    settings: &Settings,
    status: &RuntimeFeatureStatus,
    sample: &Sample,
) -> Vec<(Page, bool, String)> {
    vec![
        (
            Page::ByForeground,
            settings.by_foreground.enabled,
            t!(
                "common.rule_count",
                count = settings.by_foreground.rules.len()
            )
            .to_string(),
        ),
        (
            Page::ByRunningApp,
            settings.by_running_app.enabled,
            status
                .by_running_app
                .active_process
                .clone()
                .unwrap_or_else(|| {
                    t!(
                        "common.rule_count",
                        count = settings.by_running_app.rules.len()
                    )
                    .to_string()
                }),
        ),
        (
            Page::ByCpuLoad,
            settings.by_cpu_load.enabled,
            cpu_usage_label(sample.cpu.percent),
        ),
        (
            Page::ByActivity,
            settings.by_activity.enabled,
            activity_label(settings, sample),
        ),
        (
            Page::ByTime,
            settings.by_time.enabled,
            crate::features::power_plan_control::by_time::next_switch_label(&settings.by_time),
        ),
        (
            Page::CpuLimiter,
            settings.cpu_limiter.enabled,
            t!(
                "home.limited_count",
                count = status.cpu_limiter.limited_processes
            )
            .to_string(),
        ),
        (
            Page::CpuSetsSoft,
            settings.cpu_sets_soft.enabled,
            t!(
                "home.adjusted_count",
                count = status.cpu_sets_soft.adjusted_processes
            )
            .to_string(),
        ),
        (
            Page::ProcessorAffinityHard,
            settings.processor_affinity_hard.enabled,
            t!(
                "home.adjusted_count",
                count = status.processor_affinity_hard.adjusted_processes
            )
            .to_string(),
        ),
        (
            Page::BackgroundEfficiency,
            settings.background_efficiency.enabled,
            t!(
                "home.throttled_count",
                count = status.background_efficiency.throttled_processes
            )
            .to_string(),
        ),
        (
            Page::AppSuspension,
            settings.app_suspension.enabled,
            t!(
                "home.suspended_count",
                count = status.app_suspension.suspended_processes
            )
            .to_string(),
        ),
        (
            Page::AdaptiveEngine,
            settings.adaptive_engine.enabled,
            if status.cpu_scheduler.focus_and_launch_profile_active {
                t!("home.focus_and_launch_profile").to_string()
            } else if !settings.cpu_scheduler.cpu_pressure_restraint_enabled
                && !settings.cpu_scheduler.limit_background_processors_enabled
            {
                t!("common.enabled").to_string()
            } else {
                t!(
                    "home.adjusted_count",
                    count = status.cpu_scheduler.adjusted_processes
                )
                .to_string()
            },
        ),
        (
            Page::ProcessPriority,
            settings.process_priority.enabled,
            t!(
                "home.adjusted_count",
                count = status.process_priority.adjusted_processes
            )
            .to_string(),
        ),
        (
            Page::ThreadPriority,
            settings.thread_priority.enabled,
            t!(
                "home.adjusted_count",
                count = status.thread_priority.adjusted_processes
            )
            .to_string(),
        ),
        (
            Page::DynamicPriorityBoost,
            settings.dynamic_priority_boost.enabled,
            t!(
                "home.adjusted_count",
                count = status.dynamic_priority_boost.adjusted_processes
            )
            .to_string(),
        ),
        (
            Page::IoPriority,
            settings.io_priority.enabled,
            t!(
                "home.adjusted_count",
                count = status.io_priority.adjusted_processes
            )
            .to_string(),
        ),
        (
            Page::GpuPriority,
            settings.gpu_priority.enabled,
            t!(
                "home.adjusted_count",
                count = status.gpu_priority.adjusted_processes
            )
            .to_string(),
        ),
        (
            Page::MemoryPriority,
            settings.memory_priority.enabled,
            t!(
                "home.adjusted_count",
                count = status.memory_priority.adjusted_processes
            )
            .to_string(),
        ),
        (
            Page::MemoryTrim,
            settings.memory_trim.enabled,
            t!(
                "home.trimmed_count",
                count = status.memory_trim.trimmed_processes
            )
            .to_string(),
        ),
        (
            Page::TimerResolution,
            settings.timer_resolution.enabled,
            t!(
                "common.rule_count",
                count = settings.timer_resolution.rules.len()
            )
            .to_string(),
        ),
    ]
}
fn activity_label(settings: &Settings, sample: &Sample) -> String {
    let idle = match (
        sample.input_idle,
        sample
            .controller_idle
            .filter(|_| settings.by_activity.input_detection.controller),
    ) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    match idle {
        Some(idle)
            if idle
                >= std::time::Duration::from_secs(settings.by_activity.idle_timeout_seconds) =>
        {
            t!("home.activity_idle")
        }
        Some(_) => t!("home.activity_active"),
        None => t!("home.activity_unknown"),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn histories_bound_real_samples_and_do_not_insert_unavailable_data() {
        let mut model = Model::default();
        model.record(Sample::default());
        assert!(model.cpu.is_empty());
        for n in 0..HISTORY_LEN + 3 {
            model.record(Sample {
                cpu: CpuUsageSnapshot {
                    percent: Some(n as f32),
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        assert_eq!(model.cpu.len(), HISTORY_LEN);
        assert_eq!(model.cpu.front().unwrap().percent, Some(3.0));
        assert!(model.memory.is_empty());
        assert_eq!(memory_bytes_percent(Some(10), Some(0)), None);
        assert_eq!(
            normalize_cpu_frequency_percent(Some(4000), 2000, Some(4000)),
            100.0
        );
    }
}
