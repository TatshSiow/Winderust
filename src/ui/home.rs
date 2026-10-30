use super::design;
use super::widgets::button;
use crate::automation::RuntimeFeatureStatus;
use crate::backend::dashboard_metrics::{
    sample_memory_usage, DiskUsageMonitor, IoUsageSnapshot, MemoryUsageSnapshot,
    NetworkUsageMonitor, NetworkUsageSnapshot,
};
use crate::config::Settings;
use crate::cpu::{CpuUsageMonitor, CpuUsageSnapshot};
use crate::ui::scrolling::scrollable;
use crate::ui::Page;
use iced::widget::{canvas, column, container, responsive, row, text};
use iced::{mouse, Element, Fill, Point, Rectangle, Renderer, Theme};
use rust_i18n::t;
use std::collections::VecDeque;

const HISTORY_LEN: usize = 61;
const CHART_AXIS_WIDTH: f32 = 64.0;

struct DashboardLayout {
    columns: usize,
    chart_height: f32,
    summary_height: f32,
    fits: bool,
}

impl DashboardLayout {
    fn for_size(size: iced::Size) -> Self {
        let columns = if size.width >= 1280.0 && size.height < 680.0 {
            4
        } else if size.width >= 720.0 {
            2
        } else {
            1
        };
        let chart_rows = 4 / columns;
        let summary_rows = if columns == 1 { 2 } else { 1 };
        let gaps = (chart_rows + summary_rows - 1) as f32 * super::widgets::CARD_GAP as f32;
        let available = size.height - gaps - 4.0;
        let minimum_summary = if columns == 1 {
            220.0
        } else if size.width < 1000.0 {
            176.0
        } else {
            160.0
        };
        let chart_height = ((available - minimum_summary * summary_rows as f32)
            / chart_rows as f32)
            .floor()
            .clamp(200.0, 244.0);
        let summary_height = ((available - chart_height * chart_rows as f32) / summary_rows as f32)
            .floor()
            .clamp(minimum_summary, 288.0);
        let fits = gaps + chart_height * chart_rows as f32 + summary_height * summary_rows as f32
            <= size.height;
        Self {
            columns,
            chart_height,
            summary_height,
            fits,
        }
    }
}
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
                let mut io = DiskUsageMonitor::default();
                let mut network = NetworkUsageMonitor::default();
                let mut controller = crate::activity::ControllerActivityDetector::default();
                while requests.recv().is_ok() {
                    let now = std::time::Instant::now();
                    controller.poll(now);
                    let sample = Sample {
                        captured_at: Some(std::time::SystemTime::now()),
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
    captured_at: Option<std::time::SystemTime>,
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
    cpu: VecDeque<Timed<CpuUsageSnapshot>>,
    memory: VecDeque<Timed<MemoryUsageSnapshot>>,
    io: VecDeque<Timed<IoUsageSnapshot>>,
    network: VecDeque<Timed<NetworkUsageSnapshot>>,
    hovered: [Option<usize>; 4],
}
struct Timed<T> {
    value: T,
    captured_at: Option<std::time::SystemTime>,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Navigate(Page),
    HoverChart(ChartKind, Option<usize>),
}
impl Model {
    #[cfg(feature = "render-smoke")]
    pub(super) fn populate_smoke_history(&mut self) {
        // Deterministic visual fixtures are compiled only into the render-smoke build.
        for index in 0..HISTORY_LEN {
            let wave = (index as f64 * 0.55).sin();
            self.record(Sample {
                captured_at: Some(
                    std::time::UNIX_EPOCH
                        + std::time::Duration::from_secs(1_770_000_000 + index as u64),
                ),
                cpu: CpuUsageSnapshot {
                    percent: Some((24.0 + 12.0 * wave) as f32),
                    frequency_mhz: Some((3600.0 + 600.0 * wave) as u32),
                    base_frequency_mhz: Some(2000),
                },
                memory: MemoryUsageSnapshot {
                    percent: Some(44.0 + wave as f32),
                    used_physical_bytes: Some(7 * 1024 * 1024 * 1024),
                    total_physical_bytes: Some(16 * 1024 * 1024 * 1024),
                    ..Default::default()
                },
                io: IoUsageSnapshot {
                    bytes_per_second: Some(30_000_000.0 + wave * 10_000_000.0),
                    read_bytes_per_second: Some(28_000_000.0 + wave * 9_000_000.0),
                    write_bytes_per_second: Some(2_000_000.0 + wave * 1_000_000.0),
                },
                network: NetworkUsageSnapshot {
                    bytes_per_second: Some(1_550_000.0 + wave * 300_000.0),
                    download_bytes_per_second: Some(1_250_000.0 + wave * 250_000.0),
                    upload_bytes_per_second: Some(300_000.0 + wave * 50_000.0),
                },
                ..Default::default()
            });
        }
    }
    pub(super) fn record(&mut self, sample: Sample) {
        self.latest = sample;
        if sample.cpu.percent.is_some() {
            push_bounded(
                &mut self.cpu,
                Timed {
                    value: sample.cpu,
                    captured_at: sample.captured_at,
                },
            );
        }
        if sample.memory.percent.is_some() {
            push_bounded(
                &mut self.memory,
                Timed {
                    value: sample.memory,
                    captured_at: sample.captured_at,
                },
            );
        }
        if sample.io.bytes_per_second.is_some() {
            push_bounded(
                &mut self.io,
                Timed {
                    value: sample.io,
                    captured_at: sample.captured_at,
                },
            );
        }
        if sample.network.bytes_per_second.is_some() {
            push_bounded(
                &mut self.network,
                Timed {
                    value: sample.network,
                    captured_at: sample.captured_at,
                },
            );
        }
    }
    pub(super) fn hover_chart(&mut self, kind: ChartKind, slot: Option<usize>) {
        self.hovered[kind as usize] = slot;
    }
    pub(super) fn view<'a>(
        &'a self,
        settings: &'a Settings,
        status: &'a RuntimeFeatureStatus,
        logs: &'a [crate::action_log::ActionLogEntry],
    ) -> Element<'a, Message> {
        responsive(move |size| {
            let layout = DashboardLayout::for_size(size);
            let mut body = column![].spacing(super::widgets::CARD_GAP);
            for kinds in [
                ChartKind::Cpu,
                ChartKind::Memory,
                ChartKind::Io,
                ChartKind::Network,
            ]
            .chunks(layout.columns)
            {
                let mut charts = row![].spacing(super::widgets::CARD_GAP);
                for kind in kinds {
                    charts =
                        charts.push(self.chart_card(*kind, self.chart(*kind), layout.chart_height));
                }
                body = body.push(charts);
            }
            let entries = enabled_features(settings, status, &self.latest);
            let count = entries.iter().filter(|(_, active, _)| *active).count();
            let title = row![
                super::widgets::heading(
                    t!("home.enabled_features").to_string(),
                    design::typography::SECTION
                ),
                container(text(count.to_string()).size(design::typography::CAPTION))
                    .padding([2, 8])
                    .style(move |theme| super::widgets::indicator_chip(theme, count > 0)),
            ]
            .spacing(design::space::SMALL)
            .align_y(iced::Center);
            let master = button(
                text(
                    if settings.general.enabled {
                        t!("home.master_switch_enabled")
                    } else {
                        t!("home.master_switch_disabled")
                    }
                    .to_string(),
                )
                .size(design::typography::CAPTION),
            )
            .padding(0)
            .style(super::widgets::quiet)
            .on_press(Message::Navigate(Page::WinderustBehaviour));
            let header: Element<'_, Message> = if size.width >= 1000.0 {
                row![title, iced::widget::Space::new().width(Fill), master]
                    .align_y(iced::Center)
                    .into()
            } else {
                column![title, master].spacing(design::space::TIGHT).into()
            };
            let mut features = column![];
            for (page, active, detail) in entries {
                if active {
                    features = features.push(
                        button(
                            row![
                                container(iced::widget::Space::new())
                                    .width(8)
                                    .height(8)
                                    .style(|theme: &Theme| container::Style {
                                        background: Some(theme.palette().success.into()),
                                        border: iced::border::rounded(4),
                                        ..Default::default()
                                    }),
                                text(page.label())
                                    .size(design::typography::SECONDARY)
                                    .width(Fill),
                                text(detail)
                                    .size(design::typography::CAPTION)
                                    .style(text::secondary),
                            ]
                            .spacing(design::space::SMALL)
                            .align_y(iced::Center),
                        )
                        .width(Fill)
                        .padding([6, 4])
                        .style(super::widgets::quiet)
                        .on_press(Message::Navigate(page)),
                    );
                    features = features.push(iced::widget::rule::horizontal(1));
                }
            }
            let feature_content: Element<'_, Message> = if count == 0 {
                container(
                    text(t!("home.no_enabled_features").to_string())
                        .align_x(iced::Center)
                        .style(text::secondary),
                )
                .padding(design::space::SMALL as u16)
                .center_x(Fill)
                .center_y(Fill)
                .into()
            } else {
                scrollable(features.padding(iced::Padding {
                    right: 12.0,
                    ..Default::default()
                }))
                .id("home-enabled-features")
                .height(Fill)
                .into()
            };
            let enabled = container(
                column![header, iced::widget::rule::horizontal(1), feature_content,]
                    .spacing(design::space::SMALL),
            )
            .padding(design::space::LARGE as u16)
            .height(layout.summary_height)
            .width(Fill)
            .style(super::widgets::surface);
            let recent = recent_logs(logs, layout.summary_height);
            if layout.columns > 1 {
                body = body.push(row![enabled, recent].spacing(super::widgets::CARD_GAP));
            } else {
                body = body.push(enabled).push(recent);
            }
            if layout.fits {
                container(body).width(Fill).height(Fill).into()
            } else {
                scrollable(body).height(Fill).into()
            }
        })
        .into()
    }
    fn chart_card(&self, kind: ChartKind, chart: Chart, height: f32) -> Element<'static, Message> {
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
                t!("home.memory_available"),
                memory_usage_value_label(sample.memory),
                memory_available_value_label(sample.memory),
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
                network_usage_label(sample.network.download_bytes_per_second),
                network_usage_label(sample.network.upload_bytes_per_second),
            ),
        };
        let total = match kind {
            ChartKind::Cpu => cpu_usage_label(sample.cpu.percent),
            ChartKind::Memory => memory_usage_label(sample.memory.percent),
            ChartKind::Io => io_usage_label(sample.io.bytes_per_second),
            ChartKind::Network => network_usage_label(sample.network.bytes_per_second),
        };
        let labels = match kind {
            ChartKind::Cpu => [
                format!("{first} (%)"),
                t!("home.cpu_frequency_relative").to_string(),
            ],
            ChartKind::Memory => [format!("{title} (%)"), String::new()],
            ChartKind::Io => [format!("{first} (B/s)"), format!("{second} (B/s)")],
            ChartKind::Network => [format!("{first} (Mb/s)"), format!("{second} (Mb/s)")],
        };
        let mut legend = row![].spacing(design::space::LARGE);
        for (series, label) in labels.into_iter().enumerate() {
            if !label.is_empty() {
                legend = legend.push(
                    row![
                        container(iced::widget::Space::new())
                            .width(8)
                            .height(8)
                            .style(move |theme: &Theme| container::Style {
                                background: Some(chart_colors(theme)[series].into()),
                                border: iced::border::rounded(4),
                                ..Default::default()
                            }),
                        text(label).size(design::typography::BADGE),
                    ]
                    .spacing(design::space::CONTROL)
                    .align_y(iced::Center),
                );
            }
        }
        let tip = chart
            .tooltip_content(self.hovered[kind as usize])
            .unwrap_or_else(|| iced::widget::Space::new().into());
        let plot = iced::widget::tooltip(
            canvas(chart).width(Fill).height((height - 136.0).max(64.0)),
            tip,
            iced::widget::tooltip::Position::FollowCursor,
        )
        .padding(0)
        .delay(std::time::Duration::ZERO)
        .style(|_| container::Style::default());
        let is_io = matches!(kind, ChartKind::Io | ChartKind::Network);
        let right_label = if is_io {
            t!("home.io_total")
        } else {
            second.clone()
        };
        let right_value = if is_io {
            total.clone()
        } else {
            second_value.clone()
        };
        let primary: Element<'static, Message> = if is_io {
            column![
                text(format!("{first}: {first_value}"))
                    .size(design::typography::SECTION)
                    .style(|theme| text::Style {
                        color: Some(chart_colors(theme)[0])
                    }),
                text(format!("{second}: {second_value}"))
                    .size(design::typography::SECTION)
                    .style(|theme| text::Style {
                        color: Some(chart_colors(theme)[1])
                    }),
            ]
            .spacing(design::space::TIGHT)
            .into()
        } else {
            column![
                super::widgets::heading(total, design::typography::TITLE),
                text(if kind == ChartKind::Memory {
                    format!("{first}: {first_value}")
                } else {
                    String::new()
                })
                .size(design::typography::CAPTION)
                .style(text::secondary),
            ]
            .spacing(design::space::TIGHT)
            .into()
        };
        container(
            column![
                row![
                    super::widgets::heading(title.to_string(), design::typography::SECTION)
                        .width(Fill),
                    text(right_label.to_string())
                        .size(design::typography::CAPTION)
                        .style(text::secondary),
                ]
                .align_y(iced::Center)
                .spacing(design::space::SMALL),
                row![
                    container(primary).width(Fill),
                    super::widgets::heading(right_value, design::typography::DIALOG_TITLE).style(
                        move |theme: &Theme| text::Style {
                            color: Some(if kind == ChartKind::Cpu {
                                chart_colors(theme)[1]
                            } else {
                                theme.palette().text
                            }),
                        }
                    ),
                ]
                .height(55)
                .align_y(iced::Top)
                .spacing(design::space::SMALL),
                plot,
                legend,
            ]
            .spacing(design::space::TIGHT),
        )
        .padding(design::space::LARGE as u16)
        .height(height)
        .width(Fill)
        .style(super::widgets::surface)
        .into()
    }
    fn chart(&self, kind: ChartKind) -> Chart {
        let samples: Vec<ChartSample> = match kind {
            ChartKind::Cpu => {
                let base = self
                    .latest
                    .cpu
                    .base_frequency_mhz
                    .or_else(|| self.cpu.iter().filter_map(|s| s.value.frequency_mhz).min())
                    .unwrap_or(0);
                let peak = self
                    .cpu
                    .iter()
                    .filter_map(|s| s.value.frequency_mhz)
                    .max()
                    .filter(|peak| *peak > base);
                self.cpu
                    .iter()
                    .map(|sample| {
                        let s = sample.value;
                        ChartSample {
                            captured_at: sample.captured_at,
                            values: [
                                s.percent.unwrap_or(0.0) as f64,
                                normalize_cpu_frequency_percent(s.frequency_mhz, base, peak) as f64,
                            ],
                            labels: [
                                cpu_usage_label(s.percent),
                                cpu_frequency_label(s.frequency_mhz),
                            ],
                        }
                    })
                    .collect()
            }
            ChartKind::Memory => self
                .memory
                .iter()
                .map(|sample| {
                    let s = sample.value;
                    ChartSample {
                        captured_at: sample.captured_at,
                        values: [s.percent.unwrap_or(0.0) as f64, 0.0],
                        labels: [memory_usage_label(s.percent), String::new()],
                    }
                })
                .collect(),
            ChartKind::Io => self
                .io
                .iter()
                .map(|sample| {
                    let s = sample.value;
                    ChartSample {
                        captured_at: sample.captured_at,
                        values: [
                            s.read_bytes_per_second.unwrap_or(0.0),
                            s.write_bytes_per_second.unwrap_or(0.0),
                        ],
                        labels: [
                            io_usage_label(s.read_bytes_per_second),
                            io_usage_label(s.write_bytes_per_second),
                        ],
                    }
                })
                .collect(),
            ChartKind::Network => self
                .network
                .iter()
                .map(|sample| {
                    let s = sample.value;
                    ChartSample {
                        captured_at: sample.captured_at,
                        values: [
                            s.download_bytes_per_second.unwrap_or(0.0) * 8.0 / 1_000_000.0,
                            s.upload_bytes_per_second.unwrap_or(0.0) * 8.0 / 1_000_000.0,
                        ],
                        labels: [
                            network_usage_label(s.download_bytes_per_second),
                            network_usage_label(s.upload_bytes_per_second),
                        ],
                    }
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
            kind,
        }
    }
}
fn recent_logs(logs: &[crate::action_log::ActionLogEntry], height: f32) -> Element<'_, Message> {
    use crate::action_log::ActionLogResult;
    let mut rows = column![];
    for entry in logs.iter().rev().take(20) {
        let style = match entry.result {
            ActionLogResult::Applied | ActionLogResult::Restored => text::success,
            ActionLogResult::Skipped => text::warning,
            ActionLogResult::Failed => text::danger,
        };
        let description = format!(
            "{} · {}",
            super::action_log::action_log_feature_label(entry.feature),
            entry.reason
        );
        rows = rows.push(iced::widget::tooltip(
            button(
                row![
                    text(super::action_log::action_log_time_label(
                        entry.timestamp_epoch_ms
                    ))
                    .size(design::typography::CAPTION)
                    .style(text::secondary)
                    .width(64),
                    text(super::action_log::action_log_result_label(entry.result))
                        .size(design::typography::CAPTION)
                        .style(style)
                        .width(64),
                    container(
                        text(description.clone())
                            .size(design::typography::CAPTION)
                            .wrapping(text::Wrapping::None)
                    )
                    .width(Fill)
                    .clip(true),
                ]
                .spacing(design::space::SMALL)
                .align_y(iced::Center),
            )
            .width(Fill)
            .padding([6, 4])
            .style(super::widgets::quiet)
            .on_press(Message::Navigate(Page::ActionLog)),
            text(description),
            iced::widget::tooltip::Position::Top,
        ));
        rows = rows.push(iced::widget::rule::horizontal(1));
    }
    let content: Element<'_, Message> = if logs.is_empty() {
        container(
            text(t!("action_log.empty").to_string())
                .align_x(iced::Center)
                .style(text::secondary),
        )
        .padding(design::space::SMALL as u16)
        .center_x(Fill)
        .center_y(Fill)
        .into()
    } else {
        scrollable(rows.padding(iced::Padding {
            right: 12.0,
            ..Default::default()
        }))
        .id("home-recent-logs")
        .height(Fill)
        .into()
    };
    container(
        column![
            super::widgets::heading(
                t!("home.recent_logs").to_string(),
                design::typography::SECTION
            ),
            iced::widget::rule::horizontal(1),
            content,
            row![
                iced::widget::Space::new().width(Fill),
                button(
                    row![
                        text(t!("home.details").to_string()),
                        super::navigation::glyph("icons/chevron-right.svg")
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center)
                )
                .on_press(Message::Navigate(Page::ActionLog)),
            ],
        ]
        .spacing(design::space::SMALL),
    )
    .padding(design::space::LARGE as u16)
    .width(Fill)
    .height(height)
    .style(super::widgets::surface)
    .into()
}
fn push_bounded<T>(history: &mut VecDeque<T>, sample: T) {
    if history.len() == HISTORY_LEN {
        history.pop_front();
    }
    history.push_back(sample);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChartKind {
    Cpu,
    Memory,
    Io,
    Network,
}
#[derive(Clone, PartialEq)]
struct ChartSample {
    captured_at: Option<std::time::SystemTime>,
    values: [f64; 2],
    labels: [String; 2],
}
#[derive(Clone, PartialEq)]
struct Chart {
    samples: Vec<ChartSample>,
    maximum: f64,
    kind: ChartKind,
}
#[derive(Default)]
struct ChartState {
    hovered: Option<usize>,
    cache: canvas::Cache,
    drawn: std::cell::RefCell<Option<(Chart, Theme, Option<usize>)>>,
}
fn chart_colors(theme: &Theme) -> [iced::Color; 2] {
    [theme.palette().primary, theme.palette().success]
}

impl Chart {
    fn sample_at(&self, slot: usize) -> Option<&ChartSample> {
        self.samples
            .get(slot.checked_sub(HISTORY_LEN - self.samples.len())?)
    }

    fn tooltip_content(&self, slot: Option<usize>) -> Option<Element<'static, Message>> {
        let slot = slot?;
        let sample = self.sample_at(slot)?;
        let time = sample
            .captured_at
            .map(|time| {
                chrono::DateTime::<chrono::Local>::from(time)
                    .format("%H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|| {
                t!("common.seconds_ago", count = HISTORY_LEN - slot - 1).to_string()
            });
        let names = match self.kind {
            ChartKind::Cpu => [t!("home.by_cpu_load"), t!("home.cpu_frequency")],
            ChartKind::Memory => [t!("home.memory_usage"), "".into()],
            ChartKind::Io => [t!("home.io_read"), t!("home.io_write")],
            ChartKind::Network => [t!("home.network_download"), t!("home.network_upload")],
        };
        let mut content = column![text(time)
            .size(design::typography::CAPTION)
            .style(text::secondary)]
        .spacing(design::space::TIGHT);
        for (series, name) in names.into_iter().enumerate() {
            if !name.is_empty() {
                content = content.push(
                    row![
                        container(iced::widget::Space::new())
                            .width(6)
                            .height(6)
                            .style(move |theme: &Theme| container::Style {
                                background: Some(chart_colors(theme)[series].into()),
                                border: iced::border::rounded(3),
                                ..Default::default()
                            }),
                        text(format!("{name}: {}", sample.labels[series]))
                            .size(design::typography::SECONDARY),
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center),
                );
            }
        }
        Some(
            row![
                iced::widget::Space::new().width(12),
                container(content)
                    .padding(design::space::MEDIUM as u16)
                    .max_width(280)
                    .style(|theme: &Theme| container::Style {
                        background: Some(theme.extended_palette().background.neutral.color.into()),
                        ..super::widgets::surface(theme)
                    }),
            ]
            .into(),
        )
    }
}

impl canvas::Program<Message> for Chart {
    type State = ChartState;
    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<Message>> {
        let hovered = &mut state.hovered;
        let next = if matches!(event, canvas::Event::Mouse(mouse::Event::CursorLeft)) {
            None
        } else {
            cursor
                .position_in(bounds)
                .filter(|point| {
                    point.y <= (bounds.height - 28.0).max(0.0)
                        && point.x <= (bounds.width - CHART_AXIS_WIDTH).max(1.0)
                })
                .map(|point| {
                    ((point.x / (bounds.width - CHART_AXIS_WIDTH).max(1.0)).clamp(0.0, 1.0)
                        * (HISTORY_LEN - 1) as f32)
                        .round() as usize
                })
                .filter(|slot| *slot >= HISTORY_LEN - self.samples.len())
        };
        if *hovered == next {
            return None;
        }
        *hovered = next;
        Some(iced::widget::Action::publish(Message::HoverChart(
            self.kind, next,
        )))
    }
    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let changed =
            state
                .drawn
                .borrow()
                .as_ref()
                .is_none_or(|(chart, previous_theme, hovered)| {
                    chart != self || previous_theme != theme || *hovered != state.hovered
                });
        if changed {
            state.cache.clear();
            *state.drawn.borrow_mut() = Some((self.clone(), theme.clone(), state.hovered));
        }
        vec![state.cache.draw(renderer, bounds.size(), |frame| {
            self.draw_chart(frame, theme, bounds, state.hovered);
        })]
    }
}
impl Chart {
    fn draw_chart(
        &self,
        frame: &mut canvas::Frame,
        theme: &Theme,
        bounds: Rectangle,
        hovered: Option<usize>,
    ) {
        // Repaint the canvas: stroke-only damage bounds omit flat lines and stroke edges.
        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            theme.extended_palette().background.weak.color,
        );
        let height = (bounds.height - 28.0).max(1.0);
        let width = (bounds.width - CHART_AXIS_WIDTH).max(1.0);
        let grid = theme.extended_palette().background.strong.color;
        for fraction in [0.0, 0.5, 1.0] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(0.0, height * fraction),
                    Point::new(width, height * fraction),
                ),
                canvas::Stroke::default().with_color(grid),
            );
            let value = self.maximum * (1.0 - fraction as f64);
            frame.fill_text(canvas::Text {
                content: match self.kind {
                    ChartKind::Cpu | ChartKind::Memory => format!("{value:.0}%"),
                    ChartKind::Io => format_bytes_per_second(value),
                    ChartKind::Network => format!("{value:.1}"),
                },
                position: Point::new(
                    width + 6.0,
                    (height * fraction - 6.0).clamp(0.0, height - 12.0),
                ),
                color: super::widgets::muted_color(theme),
                size: 10.0.into(),
                ..Default::default()
            });
        }
        for (index, age) in [60, 45, 30, 15, 0].into_iter().enumerate() {
            let x = width * index as f32 / 4.0;
            frame.stroke(
                &canvas::Path::line(Point::new(x, 0.0), Point::new(x, height)),
                canvas::Stroke::default().with_color(grid),
            );
            frame.fill_text(canvas::Text {
                content: format!("{age}s"),
                position: Point::new(x, height + 5.0),
                align_x: if index == 4 {
                    iced::alignment::Horizontal::Right.into()
                } else {
                    iced::alignment::Horizontal::Left.into()
                },
                color: super::widgets::muted_color(theme),
                size: 10.0.into(),
                ..Default::default()
            });
        }
        let series_count = if self.kind == ChartKind::Memory { 1 } else { 2 };
        for series in 0..series_count {
            let area = canvas::Path::new(|path| {
                let start = (HISTORY_LEN - self.samples.len()).min(HISTORY_LEN - 1);
                path.move_to(Point::new(
                    start as f32 / (HISTORY_LEN - 1) as f32 * width,
                    height,
                ));
                for (index, sample) in self.samples.iter().enumerate() {
                    let slot = HISTORY_LEN - self.samples.len() + index;
                    let x = slot as f32 / (HISTORY_LEN - 1) as f32 * width;
                    let y = height
                        * (1.0 - (sample.values[series] / self.maximum).clamp(0.0, 1.0) as f32);
                    path.line_to(Point::new(x, y));
                }
                path.line_to(Point::new(width, height));
                path.close();
            });
            frame.fill(&area, chart_colors(theme)[series].scale_alpha(0.08));
            let path = canvas::Path::new(|path| {
                for (index, sample) in self.samples.iter().enumerate() {
                    let slot = HISTORY_LEN - self.samples.len() + index;
                    let x = slot as f32 / (HISTORY_LEN - 1) as f32 * width;
                    let value = sample.values[series];
                    let y = height * (1.0 - (value / self.maximum).clamp(0.0, 1.0) as f32);
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
                    .with_color(chart_colors(theme)[series]),
            );
        }
        if let Some(slot) = hovered {
            if let Some(sample) = self.sample_at(slot) {
                let x = slot as f32 / (HISTORY_LEN - 1) as f32 * width;
                frame.stroke(
                    &canvas::Path::line(Point::new(x, 0.0), Point::new(x, height)),
                    canvas::Stroke {
                        line_dash: canvas::LineDash {
                            segments: &[4.0, 4.0],
                            offset: 0,
                        },
                        ..canvas::Stroke::default().with_color(super::widgets::muted_color(theme))
                    },
                );
                for series in 0..series_count {
                    let y = height
                        * (1.0 - (sample.values[series] / self.maximum).clamp(0.0, 1.0) as f32);
                    let marker = canvas::Path::circle(Point::new(x, y), 4.0);
                    frame.fill(&marker, chart_colors(theme)[series]);
                    frame.stroke(
                        &marker,
                        canvas::Stroke::default()
                            .with_width(1.5)
                            .with_color(theme.extended_palette().background.weak.color),
                    );
                }
            }
        }
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
        (Some(used), Some(total)) => {
            let used_parts = memory_capacity_parts(used);
            let total_parts = memory_capacity_parts(total);
            if used_parts.unit == total_parts.unit && total_parts.unit != "B" {
                format!(
                    "{} / {} {}",
                    format_capacity_number(used_parts.value),
                    format_capacity_number(total_parts.value),
                    total_parts.unit
                )
            } else {
                format!(
                    "{} / {}",
                    format_memory_capacity(used),
                    format_memory_capacity(total)
                )
            }
        }
        _ => t!("home.collecting").to_string(),
    }
}

fn memory_available_value_label(snapshot: MemoryUsageSnapshot) -> String {
    match (snapshot.total_physical_bytes, snapshot.used_physical_bytes) {
        (Some(total), Some(used)) => format_memory_capacity(total.saturating_sub(used)),
        _ => t!("home.collecting").to_string(),
    }
}

fn network_usage_label(bytes_per_second: Option<f64>) -> String {
    bytes_per_second
        .map(|bytes| format!("{:.1} Mb/s", bytes * 8.0 / 1_000_000.0))
        .unwrap_or_else(|| t!("home.collecting").to_string())
}
fn io_usage_label(bytes_per_second: Option<f64>) -> String {
    bytes_per_second
        .map(format_bytes_per_second)
        .unwrap_or_else(|| t!("home.collecting").to_string())
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
    fn chart_hover_overlay_fits_edges_and_short_charts() {
        use iced::advanced::{layout, renderer::Renderer as _, widget::Tree, Layout, Shell};
        let mut model = Model::default();
        for index in 0..HISTORY_LEN {
            model.record(Sample {
                captured_at: Some(
                    std::time::UNIX_EPOCH
                        + std::time::Duration::from_secs(1_770_000_000 + index as u64),
                ),
                cpu: CpuUsageSnapshot {
                    percent: Some(30.0 + (index as f32 * 0.5).sin() * 10.0),
                    frequency_mhz: Some(3600 + index as u32 * 10),
                    base_frequency_mhz: Some(2000),
                },
                memory: MemoryUsageSnapshot {
                    percent: Some(84.0),
                    used_physical_bytes: Some(13_200_000_000),
                    total_physical_bytes: Some(15_600_000_000),
                    ..Default::default()
                },
                io: IoUsageSnapshot {
                    bytes_per_second: Some(45_700_000.0),
                    read_bytes_per_second: Some(5_600_000.0),
                    write_bytes_per_second: Some(40_100_000.0),
                },
                network: NetworkUsageSnapshot {
                    bytes_per_second: Some(99_875_000.0),
                    download_bytes_per_second: Some(99_275_000.0),
                    upload_bytes_per_second: Some(600_000.0),
                },
                ..Default::default()
            });
        }
        fn plot_bounds(node: &layout::Node, offset: Point, height: f32) -> Option<Rectangle> {
            let bounds = node.bounds();
            let origin = Point::new(offset.x + bounds.x, offset.y + bounds.y);
            if node.children().is_empty()
                && (bounds.height - height).abs() < 0.01
                && bounds.width > 200.0
            {
                Some(Rectangle::new(origin, bounds.size()))
            } else {
                node.children()
                    .iter()
                    .find_map(|child| plot_bounds(child, origin, height))
            }
        }
        std::fs::create_dir_all("target/ui-preview").unwrap();
        for (kind, light, height, fraction, name) in [
            (ChartKind::Cpu, true, 244.0, 0.55, "light"),
            (ChartKind::Cpu, false, 244.0, 0.55, "dark"),
            (ChartKind::Cpu, false, 200.0, 0.98, "edge"),
            (ChartKind::Memory, false, 244.0, 0.55, "memory"),
            (ChartKind::Io, false, 244.0, 0.55, "disk"),
            (ChartKind::Network, false, 244.0, 0.55, "network"),
        ] {
            let theme =
                Theme::custom_with_fn("preview", design::palette(light), design::extended_palette);
            let slot = (fraction * (HISTORY_LEN - 1) as f32).round() as usize;
            model.hover_chart(kind, Some(slot));
            let mut view = model.chart_card(kind, model.chart(kind), height);
            let mut renderer = Renderer::new(design::typography::FONT, iced::Pixels(14.0));
            let viewport = Rectangle::with_size(iced::Size::new(640.0, 280.0));
            let mut tree = Tree::new(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, viewport.size()),
            );
            let plot = plot_bounds(&node, Point::ORIGIN, (height - 136.0).max(64.0)).unwrap();
            let position = Point::new(
                plot.x + (plot.width - CHART_AXIS_WIDTH) * fraction,
                plot.y + 12.0,
            );
            let cursor = mouse::Cursor::Available(position);
            let mut messages = Vec::new();
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(mouse::Event::CursorMoved { position }),
                Layout::new(&node),
                cursor,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &viewport,
            );
            assert!(matches!(
                messages.last(),
                Some(Message::HoverChart(actual, Some(_))) if *actual == kind
            ));
            let style = iced::advanced::renderer::Style {
                text_color: theme.palette().text,
            };
            renderer.reset(viewport);
            view.as_widget().draw(
                &tree,
                &mut renderer,
                &theme,
                &style,
                Layout::new(&node),
                cursor,
                &viewport,
            );
            {
                let mut overlay = view
                    .as_widget_mut()
                    .overlay(
                        &mut tree,
                        Layout::new(&node),
                        &renderer,
                        &viewport,
                        iced::Vector::ZERO,
                    )
                    .unwrap();
                let overlay = overlay.as_overlay_mut();
                let tip = overlay.layout(&renderer, viewport.size());
                let bounds = tip.bounds();
                assert!(bounds.x >= 0.0 && bounds.y >= 0.0);
                assert!(
                    bounds.x + bounds.width <= viewport.width
                        && bounds.y + bounds.height <= viewport.height
                );
                assert!(
                    bounds.y < plot.y + plot.height - 28.0,
                    "Tooltip floats above the time axis"
                );
                renderer.with_layer(viewport, |renderer| {
                    overlay.draw(renderer, &theme, &style, Layout::new(&tip), cursor);
                });
            }
            let mut pixels = tiny_skia::Pixmap::new(640, 280).unwrap();
            let mut mask = tiny_skia::Mask::new(640, 280).unwrap();
            renderer.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &iced::advanced::graphics::Viewport::with_physical_size(
                    iced::Size::new(640, 280),
                    1.0,
                ),
                &[viewport],
                theme.palette().background,
            );
            // The software renderer writes BGRA pixels for the window surface.
            for pixel in pixels.data_mut().chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            image::save_buffer(
                format!("target/ui-preview/chart-tooltip-{name}.png"),
                pixels.data(),
                640,
                280,
                image::ColorType::Rgba8,
            )
            .unwrap();
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(mouse::Event::CursorLeft),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &viewport,
            );
            assert!(matches!(
                messages.last(),
                Some(Message::HoverChart(actual, None)) if *actual == kind
            ));
            assert!(view
                .as_widget_mut()
                .overlay(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &viewport,
                    iced::Vector::ZERO
                )
                .is_none());
        }
    }

    #[test]
    fn tooltip_timestamps_follow_available_channel_samples() {
        let mut model = Model::default();
        let captured_at =
            Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_770_000_000));
        model.record(Sample {
            captured_at,
            cpu: CpuUsageSnapshot {
                percent: Some(32.5),
                ..Default::default()
            },
            ..Default::default()
        });
        model.record(Sample {
            memory: MemoryUsageSnapshot {
                percent: Some(44.0),
                ..Default::default()
            },
            ..Default::default()
        });
        let chart = model.chart(ChartKind::Cpu);
        assert_eq!(
            chart.sample_at(HISTORY_LEN - 1).unwrap().captured_at,
            captured_at
        );
        assert_eq!(chart.sample_at(HISTORY_LEN - 1).unwrap().labels[0], "32.5%");
        assert!(chart.sample_at(HISTORY_LEN - 2).is_none());
        assert!(chart.tooltip_content(Some(HISTORY_LEN - 2)).is_none());
    }
    #[test]
    fn dashboard_fits_short_and_tall_workspaces_without_shrinking_text() {
        for (width, height, columns) in [
            (1100.0, 600.0, 2),
            (1400.0, 640.0, 4),
            (1400.0, 820.0, 2),
            (760.0, 880.0, 2),
            (650.0, 1500.0, 1),
        ] {
            let layout = DashboardLayout::for_size(iced::Size::new(width, height));
            assert_eq!(layout.columns, columns);
            assert!(layout.fits, "Dashboard should fit {width}x{height}");
            assert!(layout.chart_height >= 200.0);
            assert!(layout.summary_height >= 160.0);
        }
        for size in [iced::Size::new(900.0, 400.0), iced::Size::new(650.0, 800.0)] {
            assert!(
                !DashboardLayout::for_size(size).fits,
                "Small workspaces retain page scrolling"
            );
        }
    }
    #[test]
    fn dashboard_uses_available_memory_and_decimal_network_bits() {
        assert_eq!(network_usage_label(Some(1_000_000.0)), "8.0 Mb/s");
        assert_eq!(
            memory_available_value_label(MemoryUsageSnapshot {
                total_physical_bytes: Some(16 * 1024 * 1024 * 1024),
                used_physical_bytes: Some(7 * 1024 * 1024 * 1024),
                ..Default::default()
            }),
            "9.0 GB"
        );
        assert_eq!(network_usage_label(None), t!("home.collecting").to_string());
    }
    #[test]
    fn missing_history_has_no_zero_line_but_sampled_zero_does() {
        use canvas::Program;
        use iced::advanced::{graphics::geometry::Renderer as _, renderer::Renderer as _};
        let mut renderer = Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let bounds = Rectangle::with_size(iced::Size::new(320.0, 110.0));
        let viewport =
            iced::advanced::graphics::Viewport::with_physical_size(iced::Size::new(320, 110), 1.0);
        let mut mask = tiny_skia::Mask::new(320, 110).unwrap();
        let mut images = Vec::new();
        for count in [0, 2] {
            let chart = Chart {
                kind: ChartKind::Memory,
                maximum: 100.0,
                samples: (0..count)
                    .map(|_| ChartSample {
                        captured_at: None,
                        values: [0.0; 2],
                        labels: [String::new(), String::new()],
                    })
                    .collect(),
            };
            renderer.reset(bounds);
            for geometry in chart.draw(
                &ChartState::default(),
                &renderer,
                &Theme::Dark,
                bounds,
                mouse::Cursor::Unavailable,
            ) {
                renderer.draw_geometry(geometry);
            }
            let mut pixels = tiny_skia::Pixmap::new(320, 110).unwrap();
            renderer.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &viewport,
                &[bounds],
                iced::Color::BLACK,
            );
            images.push(pixels);
        }
        assert_eq!(
            images[0].pixel(30, 81),
            images[1].pixel(30, 81),
            "Missing history must stay blank"
        );
        assert_ne!(
            images[0].pixel(254, 81),
            images[1].pixel(254, 81),
            "Actual zero samples must draw a line"
        );
        assert_eq!(
            memory_usage_value_label(MemoryUsageSnapshot {
                used_physical_bytes: Some(7 * 1024 * 1024 * 1024),
                total_physical_bytes: Some(16 * 1024 * 1024 * 1024),
                ..Default::default()
            }),
            "7.0 / 16.0 GB"
        );
    }

    #[test]
    fn chart_repaints_flat_lines_and_hover_without_artifacts() {
        use canvas::Program;
        use iced::advanced::{graphics::geometry::Renderer as _, renderer::Renderer as _};
        let mut renderer = Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let bounds = Rectangle::with_size(iced::Size::new(320.0, 110.0));
        let viewport =
            iced::advanced::graphics::Viewport::with_physical_size(iced::Size::new(320, 110), 1.0);
        let mut pixels = tiny_skia::Pixmap::new(320, 110).unwrap();
        let mut mask = tiny_skia::Mask::new(320, 110).unwrap();
        let mut previous = Vec::new();
        let mut state = ChartState::default();
        use ChartKind::{Cpu, Memory};
        let normal = bounds.size();
        let small = iced::Size::new(300.0, 100.0);
        // Repeat unchanged frames, then change each cached drawing input.
        for (index, (value, hovered, theme, maximum, kind, size)) in [
            (75.0, None, Theme::Dark, 100.0, Cpu, normal),
            (75.0, None, Theme::Dark, 100.0, Cpu, normal),
            (74.0, Some(12), Theme::Dark, 100.0, Cpu, normal),
            (74.0, Some(12), Theme::Dark, 100.0, Cpu, normal),
            (74.0, None, Theme::Dark, 100.0, Cpu, normal),
            (74.0, None, Theme::Light, 100.0, Cpu, normal),
            (74.0, None, Theme::Light, 200.0, Cpu, normal),
            (74.0, None, Theme::Light, 200.0, Memory, normal),
            (74.0, None, Theme::Light, 200.0, Memory, small),
        ]
        .into_iter()
        .enumerate()
        {
            state.hovered = hovered;
            let chart = Chart {
                samples: (0..HISTORY_LEN)
                    .map(|_| ChartSample {
                        captured_at: None,
                        values: [value, 18.0],
                        labels: ["RAM".into(), "Cache".into()],
                    })
                    .collect(),
                maximum,
                kind,
            };
            let chart_bounds = Rectangle::with_size(size);
            renderer.reset(bounds);
            for geometry in chart.draw(
                &state,
                &renderer,
                &theme,
                chart_bounds,
                mouse::Cursor::Unavailable,
            ) {
                renderer.draw_geometry(geometry);
            }
            let damage = iced::advanced::graphics::damage::group(
                iced::advanced::graphics::damage::diff(
                    &previous,
                    renderer.layers(),
                    |layer| vec![layer.bounds],
                    iced_tiny_skia::Layer::damage,
                ),
                bounds,
            );
            if matches!(index, 1 | 3) {
                assert!(damage.is_empty(), "unchanged chart damaged {damage:?}");
            } else {
                assert!(!damage.is_empty(), "changed chart did not repaint");
            }
            renderer.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &viewport,
                &damage,
                iced::Color::BLACK,
            );
            previous = renderer.layers().to_vec();
            // Compare cached partial repaints with a fresh, uncached drawing.
            renderer.reset(bounds);
            let mut frame = canvas::Frame::new(&renderer, size);
            chart.draw_chart(&mut frame, &theme, chart_bounds, hovered);
            renderer.draw_geometry(frame.into_geometry());
            let mut full = tiny_skia::Pixmap::new(320, 110).unwrap();
            renderer.draw(
                &mut full.as_mut(),
                &mut mask,
                &viewport,
                &[bounds],
                iced::Color::BLACK,
            );
            assert_eq!(
                pixels.data(),
                full.data(),
                "partial repaint left chart artifacts"
            );
        }
    }

    #[test]
    fn chart_hover_stays_on_the_same_sample_and_clears_outside_the_plot() {
        use canvas::Program;
        let chart = Chart {
            samples: (0..HISTORY_LEN)
                .map(|_| ChartSample {
                    captured_at: None,
                    values: [1.0, 2.0],
                    labels: [String::new(), String::new()],
                })
                .collect(),
            maximum: 100.0,
            kind: ChartKind::Cpu,
        };
        let bounds = Rectangle::with_size(iced::Size::new(590.0, 110.0));
        let mut state = ChartState::default();
        let mut move_to = |x, y| {
            let position = Point::new(x, y);
            chart
                .update(
                    &mut state,
                    &canvas::Event::Mouse(mouse::Event::CursorMoved { position }),
                    bounds,
                    mouse::Cursor::Available(position),
                )
                .is_some()
        };
        assert!(move_to(300.0, 20.0));
        assert!(!move_to(300.1, 21.0));
        assert!(move_to(320.0, 20.0));
        assert!(move_to(320.0, 100.0));
        assert!(!move_to(320.0, 100.0));
        assert_eq!(state.hovered, None);
        let colors = chart_colors(&Theme::Dark);
        assert_ne!(colors[0], colors[1]);
        assert_eq!(colors[0].a, 1.0);
        assert_eq!(colors[1].a, 1.0);
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
        assert_eq!(model.cpu.front().unwrap().value.percent, Some(3.0));
        assert!(model.memory.is_empty());
        assert_eq!(
            normalize_cpu_frequency_percent(Some(4000), 2000, Some(4000)),
            100.0
        );
    }
}
