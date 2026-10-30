use super::design;
use super::navigation::dashboard_sections_in_nav_order;
use super::widgets::{self, Ui};
use crate::automation::RuntimeFeatureStatus;
use crate::backend::dashboard_metrics::{
    sample_memory_usage, DiskUsageMonitor, IoUsageSnapshot, MemoryUsageSnapshot,
    NetworkUsageMonitor, NetworkUsageSnapshot,
};
use crate::config::Settings;
use crate::cpu::{CpuUsageMonitor, CpuUsageSnapshot};
use crate::ui::Page;
use rust_i18n::t;
use std::collections::VecDeque;
use windui::{
    core::{EventCtx, Widget},
    event::{Event, PointerKind},
    prelude::{Align, Color, Element, Point, Rect, Role, Theme},
    render::{Canvas, Paint},
    style::Style,
    text::TextStyle,
};

const HISTORY_LEN: usize = 30;
type SamplerChannels = (
    std::sync::mpsc::Sender<()>,
    std::sync::mpsc::Receiver<Sample>,
);

#[derive(Debug, Default)]
pub(super) struct Sampler {
    channels: Option<Result<SamplerChannels, String>>,
}
impl Sampler {
    fn start() -> Result<SamplerChannels, String> {
        let (request, requests) = std::sync::mpsc::channel();
        let (samples, response) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("winderust-dashboard".into())
            .spawn(move || {
                // PDH handles stay owned, sampled, and dropped on this worker thread.
                let mut cpu = CpuUsageMonitor::default();
                let mut io = DiskUsageMonitor::default();
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
        worker
            .map(|_| (request, response))
            .map_err(|error| error.to_string())
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct Sample {
    cpu: CpuUsageSnapshot,
    memory: MemoryUsageSnapshot,
    io: IoUsageSnapshot,
    network: NetworkUsageSnapshot,
    input_idle: Option<std::time::Duration>,
    controller_idle: Option<std::time::Duration>,
}
#[cfg(feature = "render-smoke")]
impl Sample {
    pub(super) fn smoke_sample() -> Self {
        const GIB: u64 = 1024 * 1024 * 1024;
        Self {
            cpu: CpuUsageSnapshot {
                percent: Some(10.3),
                frequency_mhz: Some(2_970),
                base_frequency_mhz: Some(3_000),
            },
            memory: MemoryUsageSnapshot {
                percent: Some(81.25),
                used_physical_bytes: Some(13 * GIB),
                total_physical_bytes: Some(16 * GIB),
                cached_physical_bytes: Some(2 * GIB),
            },
            io: IoUsageSnapshot {
                bytes_per_second: Some(403.5 * 1024.0),
                read_bytes_per_second: Some(157.1 * 1024.0),
                write_bytes_per_second: Some(246.4 * 1024.0),
            },
            network: NetworkUsageSnapshot {
                bytes_per_second: Some(841.3 * 1024.0),
                download_bytes_per_second: Some(369.4 * 1024.0),
                upload_bytes_per_second: Some(471.9 * 1024.0),
            },
            ..Default::default()
        }
    }
}
impl Sampler {
    // Call from an application task once per second while Home sampling is enabled.
    // Construction is lazy. Dropping Sampler disconnects requests and lets its worker
    // drop all native monitors; an in-flight task may retain it until its sample finishes.
    pub(super) fn sample(&mut self) -> Result<Sample, String> {
        let (request, response) = self
            .channels
            .get_or_insert_with(Self::start)
            .as_ref()
            .map_err(Clone::clone)?;
        request.send(()).map_err(|error| error.to_string())?;
        response.recv().map_err(|error| error.to_string())
    }

    /// Release an idle sampler without touching dashboard history or preferences.
    pub(super) fn release(&mut self) {
        self.channels = None;
    }
}
#[derive(Default)]
pub(super) struct Model {
    pub(super) metrics_paused: bool,
    latest: Sample,
    cpu: VecDeque<CpuUsageSnapshot>,
    memory: VecDeque<MemoryUsageSnapshot>,
    io: VecDeque<IoUsageSnapshot>,
    network: VecDeque<NetworkUsageSnapshot>,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Navigate(Page),
    PauseMetrics(bool),
}
impl Model {
    #[cfg(feature = "render-smoke")]
    pub(super) fn smoke_history(&self) -> (Sample, [usize; 4]) {
        (
            self.latest,
            [
                self.cpu.len(),
                self.memory.len(),
                self.io.len(),
                self.network.len(),
            ],
        )
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
    pub(super) fn view(
        &self,
        settings: &Settings,
        status: &RuntimeFeatureStatus,
        ui: Ui<Message>,
        wide: bool,
    ) -> Element {
        let entries = enabled_features(settings, status, &self.latest);
        let count = entries.iter().filter(|(_, active, _)| *active).count();
        let features = entries
            .into_iter()
            .filter(|(_, active, _)| *active)
            .map(|(page, _, detail)| {
                let ui = ui.clone();
                widgets::row()
                    .height(36)
                    .padding_xy(8, 0)
                    .corner(design::CONTROL_RADIUS)
                    .child(super::navigation::glyph(
                        super::navigation::icon_path(page),
                        Role::TextMuted,
                    ))
                    .child(widgets::label(page.label()).max_lines(1).weight(1.0))
                    .child(widgets::label(detail).max_lines(1))
                    .clickable()
                    .on_click(move |ctx| {
                        widgets::flush_bindings(ctx);
                        ui.emit(Message::Navigate(page));
                    })
            })
            .collect::<Vec<_>>();
        let master = ui.clone();
        let enabled = widgets::col()
            .spacing(design::space::MEDIUM as i32)
            .bg_role(Role::Surface)
            .corner(design::CARD_RADIUS)
            .padding(14)
            .height(196)
            .children([
                widgets::row()
                    .child(
                        widgets::row()
                            .child(
                                widgets::label(t!("home.enabled_features").to_string())
                                    .font_weight(600),
                            )
                            .child(Element::badge_intent(
                                count.to_string(),
                                if count > 0 {
                                    windui::theme::Intent::Success
                                } else {
                                    windui::theme::Intent::Neutral
                                },
                            ))
                            .weight(1.0),
                    )
                    .child(
                        Element::link(
                            t!(if settings.general.enabled {
                                "home.master_switch_enabled"
                            } else {
                                "home.master_switch_disabled"
                            })
                            .to_string(),
                        )
                        .font_size(design::typography::CAPTION as f32)
                        .fg_role(if settings.general.enabled {
                            Role::Accent
                        } else {
                            Role::TextMuted
                        })
                        .on_click(move |ctx| {
                            widgets::flush_bindings(ctx);
                            master.emit(Message::Navigate(Page::WinderustBehaviour));
                        }),
                    ),
                if count == 0 {
                    widgets::label(t!("home.no_enabled_features").to_string())
                        .fg_role(Role::TextMuted)
                } else {
                    Element::scroll()
                        .width_match()
                        .weight(1.0)
                        .child(Element::col().width_match().children(features))
                },
            ]);
        let mut body = vec![widgets::row().children([
            widgets::label(t!("home.home").to_string())
                .font_weight(600)
                .weight(1.0),
            ui.button(
                t!(if self.metrics_paused {
                    "home.resume_metrics"
                } else {
                    "home.pause_metrics"
                })
                .to_string(),
                Message::PauseMetrics(!self.metrics_paused),
            )
            .small(),
        ])];
        let chart = |kind| self.chart_panel(kind, self.chart(kind));
        if wide {
            body.extend([
                widgets::row().children([
                    chart(ChartKind::Cpu).weight(1.0),
                    chart(ChartKind::Memory).weight(1.0),
                    chart(ChartKind::Io).weight(1.0),
                ]),
                widgets::row()
                    .children([chart(ChartKind::Network).weight(1.0), enabled.weight(1.0)]),
            ]);
        } else {
            body.extend([
                widgets::row().children([
                    chart(ChartKind::Cpu).weight(1.0),
                    chart(ChartKind::Memory).weight(1.0),
                ]),
                widgets::row().children([
                    chart(ChartKind::Io).weight(1.0),
                    chart(ChartKind::Network).weight(1.0),
                ]),
                enabled,
            ]);
        }
        body.push(widgets::label(t!("home.main_sections").to_string()).font_weight(600));
        let sections = dashboard_sections_in_nav_order(settings.advanced.show_advanced_controls);
        for group in sections.chunks(if wide { 3 } else { 2 }) {
            body.push(widgets::row().children(group.iter().map(|section| {
                let page = section.landing_page;
                let ui = ui.clone();
                widgets::card(
                    "",
                    [widgets::row().children([
                        super::navigation::glyph(
                            super::navigation::icon_path(page),
                            Role::TextMuted,
                        ),
                        widgets::label(page.label()).font_weight(600).weight(1.0),
                        super::navigation::glyph("icons/chevron-right.svg", Role::TextMuted),
                    ])],
                )
                .height(58)
                .weight(1.0)
                .clickable()
                .on_click(move |ctx| {
                    widgets::flush_bindings(ctx);
                    ui.emit(Message::Navigate(page));
                })
            })));
        }
        widgets::page([widgets::col()
            .spacing(design::CARD_GAP as i32)
            .children(body)])
    }

    fn chart_panel(&self, kind: ChartKind, chart: Chart) -> Element {
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
        let colors = chart_colors(&windui::theme::current());
        widgets::col()
            .spacing(8)
            .bg_role(Role::Surface)
            .corner(design::CARD_RADIUS)
            .padding(14)
            .children([
                widgets::row().children([
                    widgets::label(title.to_string())
                        .font_weight(600)
                        .font_size(design::typography::BODY as f32)
                        .weight(1.0),
                    widgets::label(total).font_weight(600),
                ]),
                widgets::row().children([
                    widgets::label(format!("{first}: {first_value}"))
                        .font_size(design::typography::SECONDARY as f32)
                        .fg(colors[0])
                        .tooltip(format!("{first}: {first_value}"))
                        .max_lines(1)
                        .weight(1.0),
                    widgets::label(format!("{second}: {second_value}"))
                        .font_size(design::typography::SECONDARY as f32)
                        .fg(colors[1])
                        .tooltip(format!("{second}: {second_value}"))
                        .max_lines(1)
                        .weight(1.0),
                ]),
                // Wind UI supplies the surface, text and layout; only the data plot is drawn.
                Element::leaf().width_match().height(110).widget(chart),
            ])
            .height(196)
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
        Chart {
            samples,
            maximum,
            hovered: None,
        }
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
    hovered: Option<usize>,
}
fn chart_colors(theme: &Theme) -> [Color; 2] {
    let bright = theme.palette.accent;
    [bright, bright.darken(0.35)]
}

impl Chart {
    fn hover_slot(&self, position: Option<Point>, bounds: Rect) -> Option<usize> {
        let point = position?;
        if !bounds.contains(point) || point.y - bounds.y > (bounds.h - 28).max(0) {
            return None;
        }
        let fraction = (point.x - bounds.x) as f32 / bounds.w.max(1) as f32;
        let slot = (fraction.clamp(0.0, 1.0) * (HISTORY_LEN - 1) as f32).round() as usize;
        (slot >= HISTORY_LEN - self.samples.len()).then_some(slot)
    }

    fn set_hover(&mut self, position: Option<Point>, bounds: Rect) -> bool {
        let next = self.hover_slot(position, bounds);
        if self.hovered == next {
            return false;
        }
        self.hovered = next;
        true
    }

    fn paint_plot(&self, bounds: Rect, canvas: &mut dyn Canvas, theme: &Theme) {
        let x = bounds.x as f32;
        let y = bounds.y as f32;
        let width = bounds.w.max(1) as f32;
        let height = (bounds.h - 28).max(1) as f32;
        // Erase the complete plot, including flat-line edges and the previous
        // hover marker, before repainting a changed sample.
        canvas.fill_rect(
            x,
            y,
            width,
            bounds.h.max(1) as f32,
            &Paint::fill(theme.palette.surface),
        );
        for fraction in [0.0, 0.5, 1.0] {
            canvas.draw_line(
                x,
                y + height * fraction,
                x + width,
                y + height * fraction,
                1.0,
                &Paint::fill(theme.palette.border),
            );
        }
        for (series, color) in chart_colors(theme).into_iter().enumerate() {
            // Empty history is a visual baseline, never recorded sample data.
            let values = std::iter::repeat_n(0.0, HISTORY_LEN - self.samples.len())
                .chain(self.samples.iter().map(|sample| sample.values[series]));
            let points = values
                .enumerate()
                .map(|(index, value)| {
                    (
                        x + index as f32 / (HISTORY_LEN - 1) as f32 * width,
                        y + height * (1.0 - (value / self.maximum).clamp(0.0, 1.0) as f32),
                    )
                })
                .collect::<Vec<_>>();
            canvas.draw_polyline(&points, 1.8, &Paint::fill(color));
        }
        if let Some(slot) = self.hovered {
            if let Some(index) = slot.checked_sub(HISTORY_LEN - self.samples.len()) {
                if let Some(sample) = self.samples.get(index) {
                    let marker_x = x + slot as f32 / (HISTORY_LEN - 1) as f32 * width;
                    canvas.draw_line(
                        marker_x,
                        y,
                        marker_x,
                        y + height,
                        1.0,
                        &Paint::fill(theme.palette.text),
                    );
                    let age = HISTORY_LEN - slot - 1;
                    let age = if age == 0 {
                        t!("common.latest_sample").to_string()
                    } else {
                        t!("common.seconds_ago", count = age).to_string()
                    };
                    canvas.draw_text(
                        &format!("{age}: {} / {}", sample.labels[0], sample.labels[1]),
                        Rect::new(bounds.x + 2, bounds.y + height as i32 + 5, bounds.w - 4, 20),
                        theme.palette.text,
                        Align::Start,
                        &TextStyle::new(11.0),
                    );
                }
            }
        }
    }
}

impl Widget for Chart {
    fn paint(
        &self,
        _bounds: Rect,
        content: Rect,
        _focused: bool,
        _enabled: bool,
        canvas: &mut dyn Canvas,
        _style: &Style,
    ) {
        self.paint_plot(content, canvas, &windui::theme::current());
    }

    fn on_event(&mut self, ctx: &mut EventCtx, event: &Event) -> bool {
        if let Event::Pointer(pointer) = event {
            let position = if pointer.kind == PointerKind::Leave {
                None
            } else {
                Some(pointer.pos)
            };
            if self.set_hover(position, ctx.bounds()) {
                ctx.mark_dirty();
            }
        }
        // The plot only observes hover. Wheel events continue to the native scroll.
        false
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
            if status
                .adaptive_engine_process
                .focus_and_launch_profile_active
            {
                t!("home.focus_and_launch_profile").to_string()
            } else if !settings
                .adaptive_engine_process
                .cpu_pressure_restraint_enabled
                && !settings
                    .adaptive_engine_process
                    .limit_background_processors_enabled
                && !settings
                    .adaptive_engine_process
                    .dynamic_resource_zones_enabled
            {
                t!("common.enabled").to_string()
            } else {
                t!(
                    "home.adjusted_count",
                    count = status.adaptive_engine_process.adjusted_processes
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
    fn sampler_stays_unstarted_until_needed_and_release_disconnects_its_worker() {
        let mut sampler = Sampler::default();
        assert!(sampler.channels.is_none());
        let (request, requests) = std::sync::mpsc::channel();
        let (samples, response) = std::sync::mpsc::channel();
        sampler.channels = Some(Ok((request, response)));
        sampler.release();
        assert!(sampler.channels.is_none());
        assert!(matches!(
            requests.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ));
        assert!(samples.send(Sample::default()).is_err());
    }
    #[test]
    fn chart_repaints_flat_lines_and_hover_without_artifacts() {
        let bounds = Rect::new(0, 0, 320, 110);
        let theme = Theme::dark();
        let mut pixels = tiny_skia::Pixmap::new(320, 110).unwrap();
        for (value, hovered) in [(75.0, None), (74.0, Some(12)), (74.0, None)] {
            let chart = Chart {
                samples: (0..HISTORY_LEN)
                    .map(|_| ChartSample {
                        values: [value, 18.0],
                        labels: ["RAM".into(), "Cache".into()],
                    })
                    .collect(),
                maximum: 100.0,
                hovered,
            };
            chart.paint_plot(
                bounds,
                &mut windui::render::skia::SkiaCanvas::new(&mut pixels),
                &theme,
            );
            let mut full = tiny_skia::Pixmap::new(320, 110).unwrap();
            chart.paint_plot(
                bounds,
                &mut windui::render::skia::SkiaCanvas::new(&mut full),
                &theme,
            );
            assert_eq!(
                pixels.data(),
                full.data(),
                "chart retained a previous line or hover marker"
            );
        }
    }

    #[test]
    fn chart_hover_stays_on_the_same_sample_and_clears_outside_the_plot() {
        let mut chart = Chart {
            samples: (0..HISTORY_LEN)
                .map(|_| ChartSample {
                    values: [1.0, 2.0],
                    labels: [String::new(), String::new()],
                })
                .collect(),
            maximum: 100.0,
            hovered: None,
        };
        let bounds = Rect::new(0, 0, 590, 110);
        assert!(chart.set_hover(Some(Point::new(300, 20)), bounds));
        assert!(!chart.set_hover(Some(Point::new(300, 21)), bounds));
        assert!(chart.set_hover(Some(Point::new(320, 20)), bounds));
        assert!(chart.set_hover(Some(Point::new(320, 100)), bounds));
        assert!(!chart.set_hover(Some(Point::new(320, 100)), bounds));
        assert_eq!(chart.hovered, None);
        let colors = chart_colors(&Theme::dark());
        assert_ne!(colors[0], colors[1]);
        assert_eq!(colors[0].a, 255);
        assert_eq!(colors[1].a, 255);
    }

    #[test]
    fn empty_history_has_no_hover_samples() {
        let chart = Chart {
            samples: Vec::new(),
            maximum: 100.0,
            hovered: None,
        };
        assert_eq!(
            chart.hover_slot(Some(Point::new(300, 20)), Rect::new(0, 0, 590, 110)),
            None
        );
    }

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
