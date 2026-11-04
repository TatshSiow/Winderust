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
const HISTORY_WINDOW: std::time::Duration =
    std::time::Duration::from_secs((HISTORY_LEN - 1) as u64);
// One-second sampling is dispatched on 250ms UI ticks; allow dispatch and worker jitter.
const MAX_SAMPLE_GAP: std::time::Duration = std::time::Duration::from_millis(1500);
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
    history: VecDeque<Sample>,
    hovered: [Option<usize>; 4],
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Navigate(Page),
    HoverChart(ChartKind, Option<usize>),
}
impl Model {
    #[cfg(any(test, feature = "render-smoke"))]
    pub(super) fn populate_smoke_history(&mut self) {
        // Deterministic visual fixtures stay outside normal application builds.
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
        if let Some(now) = sample.captured_at {
            if self
                .latest
                .captured_at
                .is_some_and(|previous| now < previous)
            {
                self.history.clear();
            } else if self
                .history
                .back()
                .is_some_and(|previous| previous.captured_at == Some(now))
            {
                self.history.pop_back();
            }
            self.history.retain(|previous| {
                previous.captured_at.is_none_or(|captured_at| {
                    now.duration_since(captured_at)
                        .is_ok_and(|age| age <= HISTORY_WINDOW)
                })
            });
        }
        self.latest = sample;
        push_bounded(&mut self.history, sample);
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
            let layout = DashboardLayout::for_size(iced::Size::new(
                (size.width - super::scrolling::SCROLLBAR_GUTTER).max(0.0),
                size.height,
            ));
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
            let count = entries.len();
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
            for (page, detail) in entries {
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
                scrollable(features)
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
                super::scrolling::vertical_gutter(body)
                    .width(Fill)
                    .height(Fill)
                    .into()
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
            ChartKind::Io | ChartKind::Network => {
                [format!("{first} (B/s)"), format!("{second} (B/s)")]
            }
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
        // Chart tooltip content owns its inset background and cursor spacing.
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
        let mut timeline: [Option<&Sample>; HISTORY_LEN] = [None; HISTORY_LEN];
        for (slot, sample) in timeline.iter_mut().rev().zip(self.history.iter().rev()) {
            *slot = Some(sample);
        }
        let (base, peak) = if kind == ChartKind::Cpu {
            let frequencies = || {
                timeline
                    .iter()
                    .flatten()
                    .filter(|sample| sample.cpu.percent.is_some())
                    .filter_map(|sample| sample.cpu.frequency_mhz)
            };
            let base = self
                .latest
                .cpu
                .base_frequency_mhz
                .or_else(|| frequencies().min())
                .unwrap_or(0);
            (base, frequencies().max().filter(|peak| *peak > base))
        } else {
            (0, None)
        };
        let samples: [Option<ChartSample>; HISTORY_LEN] = std::array::from_fn(|index| {
            let sample = timeline[index]?;
            let seconds_ago = match (self.latest.captured_at, sample.captured_at) {
                (Some(now), Some(captured_at)) => {
                    now.duration_since(captured_at).ok()?.as_secs_f64()
                }
                _ => (HISTORY_LEN - index - 1) as f64,
            };
            let values_and_labels = match kind {
                ChartKind::Cpu => sample.cpu.percent.map(|percent| {
                    (
                        [
                            percent as f64,
                            normalize_cpu_frequency_percent(sample.cpu.frequency_mhz, base, peak)
                                as f64,
                        ],
                        [
                            cpu_usage_label(sample.cpu.percent),
                            cpu_frequency_label(sample.cpu.frequency_mhz),
                        ],
                    )
                }),
                ChartKind::Memory => sample.memory.percent.map(|percent| {
                    (
                        [percent as f64, 0.0],
                        [memory_usage_label(sample.memory.percent), String::new()],
                    )
                }),
                ChartKind::Io => sample.io.bytes_per_second.map(|_| {
                    (
                        [
                            sample.io.read_bytes_per_second.unwrap_or(0.0),
                            sample.io.write_bytes_per_second.unwrap_or(0.0),
                        ],
                        [
                            io_usage_label(sample.io.read_bytes_per_second),
                            io_usage_label(sample.io.write_bytes_per_second),
                        ],
                    )
                }),
                ChartKind::Network => sample.network.bytes_per_second.map(|_| {
                    (
                        [
                            sample.network.download_bytes_per_second.unwrap_or(0.0),
                            sample.network.upload_bytes_per_second.unwrap_or(0.0),
                        ],
                        [
                            network_usage_label(sample.network.download_bytes_per_second),
                            network_usage_label(sample.network.upload_bytes_per_second),
                        ],
                    )
                }),
            };
            Some(ChartSample {
                captured_at: sample.captured_at,
                seconds_ago,
                reading: values_and_labels.map(|(values, labels)| ChartReading { values, labels }),
            })
        });
        let maximum = match kind {
            ChartKind::Cpu | ChartKind::Memory => 100.0,
            _ => samples
                .iter()
                .flatten()
                .filter_map(|sample| sample.reading.as_ref())
                .flat_map(|reading| reading.values)
                .fold(1.0, f64::max),
        };
        Chart {
            samples,
            maximum,
            kind,
        }
    }
}
fn recent_logs(logs: &[crate::action_log::ActionLogEntry], height: f32) -> Element<'_, Message> {
    let mut rows = column![];
    for entry in logs.iter().rev().take(20) {
        rows = rows
            .push(responsive(move |size| recent_log_row(entry, size.width)).height(iced::Shrink));
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
        scrollable(rows).id("home-recent-logs").height(Fill).into()
    };
    container(super::widgets::action_card(
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
                    container(
                        row![
                            text(t!("home.details").to_string()),
                            super::navigation::glyph("icons/chevron-right.svg")
                        ]
                        .spacing(design::space::SMALL)
                        .align_y(iced::Center)
                    )
                    .center_y(Fill)
                )
                .height(design::BUTTON_HEIGHT)
                .on_press(Message::Navigate(Page::ActionLog)),
            ],
        ]
        .spacing(design::space::SMALL),
        Some(Message::Navigate(Page::ActionLog)),
    ))
    .width(Fill)
    .height(height)
    .into()
}
fn recent_log_row(entry: &crate::action_log::ActionLogEntry, width: f32) -> Element<'_, Message> {
    const TIME_WIDTH: f32 = 64.0;
    const LEVEL_WIDTH: f32 = 64.0;
    const PADDING: [u16; 2] = [6, 4];
    let level = entry.result.level();
    let description = format!(
        "{} · {}: {}",
        super::action_log::action_log_feature_label(entry.feature),
        super::action_log::action_log_result_label(entry.result),
        entry.reason
    );
    let row = button(
        row![
            text(super::action_log::action_log_time_label(
                entry.timestamp_epoch_ms
            ))
            .size(design::typography::CAPTION)
            .style(text::secondary)
            .width(TIME_WIDTH),
            text(super::action_log::action_log_level_label(level))
                .size(design::typography::CAPTION)
                .style(move |theme| super::action_log::action_log_level_style(theme, level))
                .width(LEVEL_WIDTH),
            container(
                text(description.clone())
                    .size(design::typography::CAPTION)
                    .wrapping(text::Wrapping::None)
                    .shaping(iced::advanced::text::Shaping::Advanced)
            )
            .width(Fill)
            .clip(true),
        ]
        .spacing(design::space::SMALL)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding(PADDING)
    .style(super::widgets::quiet)
    .on_press(Message::Navigate(Page::ActionLog));
    let available = (width
        - TIME_WIDTH
        - LEVEL_WIDTH
        - 2.0 * f32::from(PADDING[1])
        - 2.0 * design::space::SMALL as f32)
        .max(0.0);
    if super::widgets::text_width(&description, design::typography::CAPTION) > available {
        super::widgets::tooltip(row, text(description), iced::widget::tooltip::Position::Top).into()
    } else {
        row.into()
    }
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
    seconds_ago: f64,
    reading: Option<ChartReading>,
}
#[derive(Clone, PartialEq)]
struct ChartReading {
    values: [f64; 2],
    labels: [String; 2],
}
impl ChartSample {
    fn connects_to(&self, next: &Self) -> bool {
        self.reading.is_some()
            && next.reading.is_some()
            && match (self.captured_at, next.captured_at) {
                (Some(previous), Some(next)) => next
                    .duration_since(previous)
                    .is_ok_and(|gap| gap <= MAX_SAMPLE_GAP),
                _ => (0.0..=MAX_SAMPLE_GAP.as_secs_f64())
                    .contains(&(self.seconds_ago - next.seconds_ago)),
            }
    }

    fn x(&self, width: f32) -> f32 {
        (1.0 - self.seconds_ago / HISTORY_WINDOW.as_secs_f64()) as f32 * width
    }

    fn point(&self, series: usize, width: f32, height: f32, maximum: f64) -> Option<Point> {
        self.reading.as_ref().map(|reading| {
            Point::new(
                self.x(width),
                height * (1.0 - (reading.values[series] / maximum).clamp(0.0, 1.0) as f32),
            )
        })
    }
}
#[derive(Clone, PartialEq)]
struct Chart {
    samples: [Option<ChartSample>; HISTORY_LEN],
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
    [
        theme.palette().primary,
        iced::theme::palette::mix(theme.palette().primary, theme.palette().text, 0.5),
    ]
}

impl Chart {
    fn sample_at(&self, slot: usize) -> Option<&ChartSample> {
        self.samples
            .get(slot)
            .and_then(Option::as_ref)
            .filter(|sample| sample.reading.is_some())
    }

    fn sample_near(&self, seconds_ago: f64) -> Option<usize> {
        self.samples
            .iter()
            .enumerate()
            .filter_map(|(slot, sample)| {
                let sample = sample.as_ref()?;
                Some((slot, sample, (sample.seconds_ago - seconds_ago).abs()))
            })
            .min_by(|left, right| left.2.total_cmp(&right.2))
            .filter(|(_, sample, distance)| {
                sample.reading.is_some() && *distance <= MAX_SAMPLE_GAP.as_secs_f64() / 2.0
            })
            .map(|(slot, _, _)| slot)
    }

    fn tooltip_content(&self, slot: Option<usize>) -> Option<Element<'static, Message>> {
        let slot = slot?;
        let sample = self.sample_at(slot)?;
        let reading = sample.reading.as_ref()?;
        let time = sample
            .captured_at
            .map(|time| {
                chrono::DateTime::<chrono::Local>::from(time)
                    .format("%H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|| {
                t!(
                    "common.seconds_ago",
                    count = sample.seconds_ago.round() as u64
                )
                .to_string()
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
                        text(format!("{name}: {}", reading.labels[series]))
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
                    .style(super::widgets::tooltip_surface),
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
                .and_then(|point| {
                    self.sample_near(
                        (1.0 - (point.x / (bounds.width - CHART_AXIS_WIDTH).max(1.0)) as f64)
                            * HISTORY_WINDOW.as_secs_f64(),
                    )
                })
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
                    ChartKind::Io | ChartKind::Network => format_bytes_per_second(value),
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
            for run in self
                .samples
                .chunk_by(|previous, next| match (previous, next) {
                    (Some(previous), Some(next)) => previous.connects_to(next),
                    _ => false,
                })
            {
                let points = || {
                    run.iter()
                        .flatten()
                        .filter_map(|sample| sample.point(series, width, height, self.maximum))
                };
                let mut line = points();
                let Some(first) = line.next() else {
                    continue;
                };
                let Some(last) = line.next_back() else {
                    frame.fill(
                        &canvas::Path::circle(first, 2.0),
                        chart_colors(theme)[series],
                    );
                    continue;
                };
                let area = canvas::Path::new(|path| {
                    path.move_to(Point::new(first.x, height));
                    for point in points() {
                        path.line_to(point);
                    }
                    path.line_to(Point::new(last.x, height));
                    path.close();
                });
                frame.fill(&area, chart_colors(theme)[series].scale_alpha(0.08));
                let path = canvas::Path::new(|path| {
                    path.move_to(first);
                    for point in points().skip(1) {
                        path.line_to(point);
                    }
                });
                frame.stroke(
                    &path,
                    canvas::Stroke::default()
                        .with_width(1.8)
                        .with_color(chart_colors(theme)[series]),
                );
            }
        }
        if let Some(slot) = hovered {
            if let Some(sample) = self.sample_at(slot) {
                let x = sample.x(width);
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
                    let Some(point) = sample.point(series, width, height, self.maximum) else {
                        continue;
                    };
                    let marker = canvas::Path::circle(point, 4.0);
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
        .map(format_bytes_per_second)
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
    const KB: f64 = 1_000.0;
    const MB: f64 = KB * 1_000.0;
    const GB: f64 = MB * 1_000.0;
    const TB: f64 = GB * 1_000.0;

    let bytes = bytes as f64;
    if bytes >= TB {
        MemoryCapacityParts {
            value: bytes / TB,
            unit: "TB",
        }
    } else if bytes >= GB {
        MemoryCapacityParts {
            value: bytes / GB,
            unit: "GB",
        }
    } else if bytes >= MB {
        MemoryCapacityParts {
            value: bytes / MB,
            unit: "MB",
        }
    } else if bytes >= KB {
        MemoryCapacityParts {
            value: bytes / KB,
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
    const KB: f64 = 1_000.0;
    const MB: f64 = KB * 1_000.0;
    const GB: f64 = MB * 1_000.0;
    const TB: f64 = GB * 1_000.0;

    if bytes_per_second >= TB {
        format!("{:.1} TB/s", bytes_per_second / TB)
    } else if bytes_per_second >= GB {
        format!("{:.1} GB/s", bytes_per_second / GB)
    } else if bytes_per_second >= MB {
        format!("{:.1} MB/s", bytes_per_second / MB)
    } else if bytes_per_second >= KB {
        format!("{:.1} KB/s", bytes_per_second / KB)
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
) -> Vec<(Page, String)> {
    let mut entries = Vec::new();
    macro_rules! feature {
        ($page:ident, $field:ident, $detail:expr) => {
            if settings.$field.enabled {
                entries.push((Page::$page, $detail));
            }
        };
    }
    feature!(
        ByForeground,
        by_foreground,
        t!(
            "common.rule_count",
            count = settings.by_foreground.rules.len()
        )
        .to_string()
    );
    feature!(
        ByRunningApp,
        by_running_app,
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
            })
    );
    feature!(ByCpuLoad, by_cpu_load, cpu_usage_label(sample.cpu.percent));
    feature!(ByActivity, by_activity, activity_label(settings, sample));
    feature!(
        ByTime,
        by_time,
        crate::features::power_plan_control::by_time::next_switch_label(&settings.by_time)
    );
    feature!(
        CpuLimiter,
        cpu_limiter,
        t!(
            "home.limited_count",
            count = status.cpu_limiter.limited_processes
        )
        .to_string()
    );
    feature!(
        CpuSetsSoft,
        cpu_sets_soft,
        t!(
            "home.adjusted_count",
            count = status.cpu_sets_soft.adjusted_processes
        )
        .to_string()
    );
    feature!(
        ProcessorAffinityHard,
        processor_affinity_hard,
        t!(
            "home.adjusted_count",
            count = status.processor_affinity_hard.adjusted_processes
        )
        .to_string()
    );
    feature!(
        BackgroundEfficiency,
        background_efficiency,
        t!(
            "home.throttled_count",
            count = status.background_efficiency.throttled_processes
        )
        .to_string()
    );
    feature!(
        AppSuspension,
        app_suspension,
        t!(
            "home.suspended_count",
            count = status.app_suspension.suspended_processes
        )
        .to_string()
    );
    feature!(
        AdaptiveEngine,
        adaptive_engine,
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
        }
    );
    feature!(
        ProcessPriority,
        process_priority,
        t!(
            "home.adjusted_count",
            count = status.process_priority.adjusted_processes
        )
        .to_string()
    );
    feature!(
        ThreadPriority,
        thread_priority,
        t!(
            "home.adjusted_count",
            count = status.thread_priority.adjusted_processes
        )
        .to_string()
    );
    feature!(
        DynamicPriorityBoost,
        dynamic_priority_boost,
        t!(
            "home.adjusted_count",
            count = status.dynamic_priority_boost.adjusted_processes
        )
        .to_string()
    );
    feature!(
        IoPriority,
        io_priority,
        t!(
            "home.adjusted_count",
            count = status.io_priority.adjusted_processes
        )
        .to_string()
    );
    feature!(
        GpuPriority,
        gpu_priority,
        t!(
            "home.adjusted_count",
            count = status.gpu_priority.adjusted_processes
        )
        .to_string()
    );
    feature!(
        MemoryPriority,
        memory_priority,
        t!(
            "home.adjusted_count",
            count = status.memory_priority.adjusted_processes
        )
        .to_string()
    );
    feature!(
        Watchdog,
        watchdog,
        t!("common.rule_count", count = settings.watchdog.rules.len()).to_string()
    );
    feature!(
        MemoryTrim,
        memory_trim,
        t!(
            "home.trimmed_count",
            count = status.memory_trim.trimmed_processes
        )
        .to_string()
    );
    feature!(
        TimerResolution,
        timer_resolution,
        t!(
            "common.rule_count",
            count = settings.timer_resolution.rules.len()
        )
        .to_string()
    );
    entries
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
    fn enabled_features_keep_order_details_and_independent_toggles() {
        use crate::ui::navigation::feature_toggle;

        let pages = [
            Page::ByForeground,
            Page::ByRunningApp,
            Page::ByCpuLoad,
            Page::ByActivity,
            Page::ByTime,
            Page::CpuLimiter,
            Page::CpuSetsSoft,
            Page::ProcessorAffinityHard,
            Page::BackgroundEfficiency,
            Page::AppSuspension,
            Page::AdaptiveEngine,
            Page::ProcessPriority,
            Page::ThreadPriority,
            Page::DynamicPriorityBoost,
            Page::IoPriority,
            Page::GpuPriority,
            Page::MemoryPriority,
            Page::Watchdog,
            Page::MemoryTrim,
            Page::TimerResolution,
        ];
        let mut settings = Settings::default();
        settings.general.enabled = true;
        settings.advanced.show_advanced_controls = true;
        settings.by_time.rules.clear();
        for page in pages {
            *feature_toggle(page).unwrap()(&mut settings) = true;
        }
        let mut status = RuntimeFeatureStatus::default();
        status.by_running_app.active_process = Some("example.exe".into());
        status.cpu_sets_soft.adjusted_processes = 7;
        status.app_suspension.suspended_processes = 3;
        let sample = Sample {
            cpu: CpuUsageSnapshot {
                percent: Some(42.5),
                ..Default::default()
            },
            input_idle: Some(std::time::Duration::ZERO),
            ..Default::default()
        };
        let visible = |settings: &Settings| enabled_features(settings, &status, &sample);
        let expected = visible(&settings);
        assert_eq!(
            expected.iter().map(|(page, _)| *page).collect::<Vec<_>>(),
            pages
        );
        for entry in [
            (Page::ByRunningApp, "example.exe".into()),
            (Page::ByCpuLoad, "42.5%".into()),
            (
                Page::CpuSetsSoft,
                t!("home.adjusted_count", count = 7).to_string(),
            ),
            (
                Page::AppSuspension,
                t!("home.suspended_count", count = 3).to_string(),
            ),
        ] {
            assert!(expected.contains(&entry));
        }
        settings.general.enabled = false;
        settings.advanced.show_advanced_controls = false;
        assert_eq!(visible(&settings), expected);
        for page in pages {
            *feature_toggle(page).unwrap()(&mut settings) = false;
            assert_eq!(
                visible(&settings),
                expected
                    .iter()
                    .filter(|(listed, _)| *listed != page)
                    .cloned()
                    .collect::<Vec<_>>()
            );
            *feature_toggle(page).unwrap()(&mut settings) = true;
        }
        for page in pages {
            *feature_toggle(page).unwrap()(&mut settings) = false;
        }
        assert!(visible(&settings).is_empty());
    }

    #[test]
    fn recent_log_tooltip_tracks_truncation_and_preserves_navigation() {
        use iced::advanced::{layout, widget::Tree, Layout, Shell};
        let entry = crate::action_log::ActionLogEntry {
            sequence: 1,
            batch_id: 1,
            timestamp_epoch_ms: 1_700_000_000_000,
            feature: crate::action_log::ActionLogFeature::ProcessPriority,
            process_id: Some(1234),
            process_name: "app.exe".into(),
            result: crate::action_log::ActionLogResult::Applied,
            reason: "Applied configured process policy".into(),
        };
        let renderer = Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let mut view: Element<'_, Message> = responsive(|size| recent_log_row(&entry, size.width))
            .height(iced::Shrink)
            .into();
        let mut tree = Tree::new(&view);
        // Reuse the widget state while resizing in both directions.
        for (width, truncated) in [(900.0, false), (220.0, true), (900.0, false)] {
            let bounds = Rectangle::with_size(iced::Size::new(width, 100.0));
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, bounds.size()),
            );
            let position = Layout::new(&node).bounds().center();
            let cursor = mouse::Cursor::Available(position);
            let mut messages = Vec::new();
            for event in [
                mouse::Event::CursorMoved { position },
                mouse::Event::ButtonPressed(mouse::Button::Left),
                mouse::Event::ButtonReleased(mouse::Button::Left),
            ] {
                view.as_widget_mut().update(
                    &mut tree,
                    &iced::Event::Mouse(event),
                    Layout::new(&node),
                    cursor,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut Shell::new(&mut messages),
                    &bounds,
                );
            }
            assert!(matches!(
                messages.last(),
                Some(Message::Navigate(Page::ActionLog))
            ));
            assert_eq!(
                view.as_widget_mut()
                    .overlay(
                        &mut tree,
                        Layout::new(&node),
                        &renderer,
                        &bounds,
                        iced::Vector::ZERO,
                    )
                    .is_some(),
                truncated
            );
        }
    }

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
        for (kind, light, height, fraction) in [
            (ChartKind::Cpu, true, 244.0, 0.55),
            (ChartKind::Cpu, false, 244.0, 0.55),
            (ChartKind::Cpu, false, 200.0, 0.98),
            (ChartKind::Memory, false, 244.0, 0.55),
            (ChartKind::Io, false, 244.0, 0.55),
            (ChartKind::Network, false, 244.0, 0.55),
        ] {
            let theme = Theme::custom_with_fn(
                "preview",
                design::palette(light, None),
                design::extended_palette,
            );
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
    fn chart_timestamps_preserve_missing_intervals_and_expire_paused_history() {
        let mut model = Model::default();
        let start = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_770_000_000);
        let at = |seconds| Some(start + std::time::Duration::from_secs(seconds));
        let first = Sample {
            captured_at: at(0),
            cpu: CpuUsageSnapshot {
                percent: Some(32.5),
                frequency_mhz: Some(9000),
                base_frequency_mhz: Some(2000),
            },
            io: IoUsageSnapshot {
                bytes_per_second: Some(1_000_000.0),
                read_bytes_per_second: Some(900_000.0),
                write_bytes_per_second: Some(100_000.0),
            },
            network: NetworkUsageSnapshot {
                bytes_per_second: Some(1_000_000.0),
                download_bytes_per_second: Some(900_000.0),
                upload_bytes_per_second: Some(100_000.0),
            },
            ..Default::default()
        };
        model.record(first);
        model.record(Sample {
            captured_at: at(2),
            memory: MemoryUsageSnapshot {
                percent: Some(44.0),
                ..Default::default()
            },
            ..Default::default()
        });
        let chart = model.chart(ChartKind::Cpu);
        assert_eq!(
            chart
                .sample_at(HISTORY_LEN - 2)
                .map(|sample| sample.captured_at),
            Some(at(0)),
            "the CPU observation must stay two seconds old when that channel is unavailable"
        );
        assert_eq!(
            chart
                .sample_at(HISTORY_LEN - 2)
                .unwrap()
                .reading
                .as_ref()
                .unwrap()
                .labels[0],
            "32.5%"
        );
        assert_eq!(chart.sample_at(HISTORY_LEN - 2).unwrap().seconds_ago, 2.0);
        assert!(chart.sample_at(HISTORY_LEN - 1).is_none());
        assert!(chart.sample_at(HISTORY_LEN - 3).is_none());
        assert!(chart.tooltip_content(Some(HISTORY_LEN - 3)).is_none());
        assert_eq!(
            model
                .chart(ChartKind::Memory)
                .sample_at(HISTORY_LEN - 1)
                .unwrap()
                .captured_at,
            at(2)
        );
        for kind in [ChartKind::Io, ChartKind::Network] {
            assert_eq!(
                model
                    .chart(kind)
                    .sample_at(HISTORY_LEN - 2)
                    .unwrap()
                    .captured_at,
                at(0)
            );
            assert!(model.chart(kind).sample_at(HISTORY_LEN - 1).is_none());
        }

        model.record(Sample {
            captured_at: at(600),
            ..Default::default()
        });
        for kind in [
            ChartKind::Cpu,
            ChartKind::Memory,
            ChartKind::Io,
            ChartKind::Network,
        ] {
            assert!(
                (0..HISTORY_LEN).all(|slot| model.chart(kind).sample_at(slot).is_none()),
                "a long pause must expire every channel even without a fresh valid value"
            );
        }
        model.record(Sample {
            captured_at: at(601),
            cpu: CpuUsageSnapshot {
                frequency_mhz: Some(3000),
                ..first.cpu
            },
            io: IoUsageSnapshot {
                bytes_per_second: Some(30.0),
                read_bytes_per_second: Some(20.0),
                write_bytes_per_second: Some(10.0),
            },
            network: NetworkUsageSnapshot {
                bytes_per_second: Some(150.0),
                download_bytes_per_second: Some(100.0),
                upload_bytes_per_second: Some(50.0),
            },
            ..first
        });
        assert_eq!(
            model
                .chart(ChartKind::Cpu)
                .sample_at(HISTORY_LEN - 1)
                .unwrap()
                .reading
                .as_ref()
                .unwrap()
                .values[1],
            100.0
        );
        assert_eq!(model.chart(ChartKind::Io).maximum, 20.0);
        assert_eq!(model.chart(ChartKind::Network).maximum, 100.0);
        model.record(Sample {
            captured_at: at(661),
            ..Default::default()
        });
        assert_eq!(
            model
                .chart(ChartKind::Cpu)
                .sample_at(HISTORY_LEN - 2)
                .unwrap()
                .captured_at,
            at(601)
        );
        assert_eq!(
            model
                .chart(ChartKind::Cpu)
                .sample_at(HISTORY_LEN - 2)
                .unwrap()
                .x(60.0),
            0.0,
            "an observation exactly 60 seconds old remains at the left edge"
        );
        model.record(Sample {
            captured_at: Some(start + std::time::Duration::from_millis(661_001)),
            ..Default::default()
        });
        assert!(model
            .chart(ChartKind::Cpu)
            .samples
            .iter()
            .flatten()
            .all(|sample| sample.reading.is_none()));

        model.record(Sample {
            captured_at: at(663),
            cpu: CpuUsageSnapshot {
                frequency_mhz: Some(3000),
                ..first.cpu
            },
            ..first
        });
        model.record(Sample {
            captured_at: at(665),
            ..first
        });
        model.record(Sample {
            captured_at: Some(start + std::time::Duration::from_millis(665_200)),
            cpu: CpuUsageSnapshot {
                frequency_mhz: Some(60_000),
                ..first.cpu
            },
            ..Default::default()
        });
        let chart = model.chart(ChartKind::Cpu);
        assert_eq!(
            chart
                .samples
                .iter()
                .flatten()
                .filter(|sample| sample.reading.is_some())
                .count(),
            3
        );
        assert_eq!(
            chart.sample_at(HISTORY_LEN - 2).unwrap().captured_at,
            at(665),
            "distinct subsecond observations must not overwrite each other"
        );
        let count = model.history.len();
        model.record(Sample {
            captured_at: Some(start + std::time::Duration::from_millis(665_200)),
            cpu: CpuUsageSnapshot {
                base_frequency_mhz: Some(2000),
                ..Default::default()
            },
            ..Default::default()
        });
        assert_eq!(model.history.len(), count);
        assert!(
            model
                .chart(ChartKind::Cpu)
                .sample_at(HISTORY_LEN - 1)
                .is_none(),
            "a newer unavailable reading must replace an observation at the exact same timestamp"
        );
        assert_eq!(
            model
                .chart(ChartKind::Cpu)
                .sample_at(HISTORY_LEN - 2)
                .unwrap()
                .reading
                .as_ref()
                .unwrap()
                .values[1],
            100.0,
            "an overwritten exact-timestamp frequency must not set the visible chart scale"
        );
        model.record(Sample {
            captured_at: at(664),
            ..first
        });
        assert_eq!(
            model.history.len(),
            1,
            "backward clock changes reset the history"
        );
        assert_eq!(
            model
                .chart(ChartKind::Cpu)
                .sample_at(HISTORY_LEN - 1)
                .unwrap()
                .captured_at,
            at(664)
        );
    }
    #[test]
    fn chart_jitter_preserves_real_timestamps_without_false_gaps() {
        use canvas::Program;
        use iced::advanced::{graphics::geometry::Renderer as _, renderer::Renderer as _};

        let start = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_770_000_000);
        let valid = Sample {
            cpu: CpuUsageSnapshot {
                percent: Some(30.0),
                frequency_mhz: Some(3000),
                base_frequency_mhz: Some(2000),
            },
            memory: MemoryUsageSnapshot {
                percent: Some(75.0),
                ..Default::default()
            },
            io: IoUsageSnapshot {
                bytes_per_second: Some(30.0),
                read_bytes_per_second: Some(20.0),
                write_bytes_per_second: Some(10.0),
            },
            network: NetworkUsageSnapshot {
                bytes_per_second: Some(150.0),
                download_bytes_per_second: Some(100.0),
                upload_bytes_per_second: Some(50.0),
            },
            ..Default::default()
        };
        let at = |millis| Sample {
            captured_at: Some(start + std::time::Duration::from_millis(millis)),
            ..valid
        };
        let render = |chart: Chart| {
            let mut renderer = Renderer::new(design::typography::FONT, iced::Pixels(14.0));
            let bounds = Rectangle::with_size(iced::Size::new(664.0, 128.0));
            let viewport = iced::advanced::graphics::Viewport::with_physical_size(
                iced::Size::new(664, 128),
                1.0,
            );
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
            let mut pixels = tiny_skia::Pixmap::new(664, 128).unwrap();
            let mut mask = tiny_skia::Mask::new(664, 128).unwrap();
            renderer.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &viewport,
                &[bounds],
                iced::Color::BLACK,
            );
            pixels
        };
        let blank = render(Model::default().chart(ChartKind::Memory));
        for cadence in [&[1200][..], &[1250][..], &[1000, 1400][..]] {
            let mut model = Model::default();
            let mut millis = 0;
            let mut count = 0;
            while millis <= 60_000 {
                model.record(at(millis));
                millis += cadence[count % cadence.len()];
                count += 1;
            }
            for kind in [
                ChartKind::Cpu,
                ChartKind::Memory,
                ChartKind::Io,
                ChartKind::Network,
            ] {
                let chart = model.chart(kind);
                assert_eq!(chart.samples.iter().flatten().count(), count);
                for sample in chart.samples.iter().flatten() {
                    let index = chart.sample_near(sample.seconds_ago).unwrap();
                    assert_eq!(
                        chart.sample_at(index).unwrap().captured_at,
                        sample.captured_at
                    );
                }
            }
            let chart = model.chart(ChartKind::Memory);
            let left = chart
                .samples
                .iter()
                .flatten()
                .next()
                .unwrap()
                .x(600.0)
                .ceil() as u32;
            let pixels = render(chart);
            for x in left + 3..598 {
                if (3..148).contains(&(x % 150)) {
                    for y in [25, 75] {
                        assert_ne!(
                            pixels.pixel(x, y), blank.pixel(x, y),
                            "ordinary {cadence:?}ms sampling left a false stroke/fill gap at {x},{y}"
                        );
                    }
                }
            }
        }

        let mut model = Model::default();
        for millis in [0, 1250, 2500, 7500, 8750, 10_000] {
            model.record(at(millis));
        }
        let chart = model.chart(ChartKind::Memory);
        assert_eq!(chart.sample_at(HISTORY_LEN - 2).unwrap().x(600.0), 587.5);
        assert_eq!(chart.sample_near(1.25), Some(HISTORY_LEN - 2));
        assert_eq!(chart.sample_near(5.0), None);
        let pixels = render(chart);
        for y in [25, 75] {
            assert_eq!(
                pixels.pixel(550, y),
                blank.pixel(550, y),
                "a real five-second pause must stay blank"
            );
        }

        let mut model = Model::default();
        model.record(at(0));
        model.record(Sample {
            memory: MemoryUsageSnapshot::default(),
            ..at(600)
        });
        model.record(at(1200));
        let chart = model.chart(ChartKind::Memory);
        assert_eq!(
            chart.sample_near(0.6),
            None,
            "an unavailable reading must not borrow a nearby valid tooltip"
        );
        assert_eq!(chart.sample_near(0.0), Some(HISTORY_LEN - 1));
        let pixels = render(chart);
        for y in [25, 75] {
            assert_eq!(
                pixels.pixel(594, y),
                blank.pixel(594, y),
                "explicit unavailability must break stroke and fill"
            );
        }

        for (millis, connected) in [(1500, true), (1501, false)] {
            let mut model = Model::default();
            model.record(at(0));
            model.record(at(millis));
            model.record(at(2700));
            let pixels = render(model.chart(ChartKind::Memory));
            for y in [25, 75] {
                assert_eq!(
                    pixels.pixel(580, y) != blank.pixel(580, y),
                    connected,
                    "continuity boundary at {millis}ms"
                );
            }
        }
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
    fn dashboard_uses_available_memory_and_decimal_network_bytes() {
        assert_eq!(network_usage_label(Some(1_000_000.0)), "1.0 MB/s");
        assert_eq!(
            memory_available_value_label(MemoryUsageSnapshot {
                total_physical_bytes: Some(16 * 1024 * 1024 * 1024),
                used_physical_bytes: Some(7 * 1024 * 1024 * 1024),
                ..Default::default()
            }),
            "9.7 GB"
        );
        assert_eq!(network_usage_label(None), t!("home.collecting").to_string());
    }
    #[test]
    fn network_chart_and_headers_use_the_same_byte_rates() {
        let mut model = Model::default();
        model.record(Sample {
            network: NetworkUsageSnapshot {
                bytes_per_second: Some(1_500_000.0),
                download_bytes_per_second: Some(1_000_000.0),
                upload_bytes_per_second: Some(500_000.0),
            },
            ..Default::default()
        });
        let chart = model.chart(ChartKind::Network);
        let sample = chart
            .sample_at(HISTORY_LEN - 1)
            .unwrap()
            .reading
            .as_ref()
            .unwrap();
        assert_eq!(chart.maximum, 1_000_000.0);
        assert_eq!(sample.values, [1_000_000.0, 500_000.0]);
        assert_eq!(sample.labels, ["1.0 MB/s", "500.0 KB/s"]);
        assert_eq!(
            sample.labels,
            [
                network_usage_label(model.latest.network.download_bytes_per_second),
                network_usage_label(model.latest.network.upload_bytes_per_second),
            ]
        );
        assert_eq!(
            network_usage_label(model.latest.network.bytes_per_second),
            "1.5 MB/s"
        );
    }
    #[test]
    fn byte_capacities_and_rates_use_decimal_units() {
        for (bytes, value, unit, capacity, rate) in [
            (999, 999.0, "B", "999 B", "999 B/s"),
            (1_000, 1.0, "KB", "1.0 KB", "1.0 KB/s"),
            (1_000_000, 1.0, "MB", "1.0 MB", "1.0 MB/s"),
            (1_500_000, 1.5, "MB", "1.5 MB", "1.5 MB/s"),
            (1_000_000_000, 1.0, "GB", "1.0 GB", "1.0 GB/s"),
            (1_000_000_000_000, 1.0, "TB", "1.0 TB", "1.0 TB/s"),
        ] {
            let parts = memory_capacity_parts(bytes);
            assert_eq!(parts.unit, unit);
            assert_eq!(parts.value, value);
            assert_eq!(format_memory_capacity(bytes), capacity);
            assert_eq!(format_bytes_per_second(bytes as f64), rate);
        }
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
        let point = |slot, value| {
            Some(ChartSample {
                captured_at: None,
                seconds_ago: (HISTORY_LEN - slot - 1) as f64,
                reading: Some(ChartReading {
                    values: [value, 0.0],
                    labels: [String::new(), String::new()],
                }),
            })
        };
        let empty = std::array::from_fn(|_| None);
        let zeros = std::array::from_fn(|slot| {
            if slot >= HISTORY_LEN - 2 {
                point(slot, 0.0)
            } else {
                None
            }
        });
        let mut gaps = std::array::from_fn(|_| None);
        gaps[15] = point(15, 0.0);
        gaps[45] = point(45, 75.0);
        for samples in [empty, zeros, gaps] {
            let chart = Chart {
                kind: ChartKind::Memory,
                maximum: 100.0,
                samples,
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
        for (x, y) in [(128, 51), (128, 65)] {
            assert_eq!(
                images[0].pixel(x, y),
                images[2].pixel(x, y),
                "Missing intervals must not acquire a connecting stroke or area fill"
            );
        }
        for (x, y) in [(64, 81), (192, 20)] {
            assert_ne!(
                images[0].pixel(x, y),
                images[2].pixel(x, y),
                "Isolated real observations, including zero, must remain visible"
            );
        }
        assert_eq!(
            memory_usage_value_label(MemoryUsageSnapshot {
                used_physical_bytes: Some(7 * 1024 * 1024 * 1024),
                total_physical_bytes: Some(16 * 1024 * 1024 * 1024),
                ..Default::default()
            }),
            "7.5 / 17.2 GB"
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
        let accent_theme = |accent| {
            Theme::custom_with_fn(
                "accent",
                design::palette(true, Some(accent)),
                design::extended_palette,
            )
        };
        let blue = accent_theme(iced::Color::from_rgb8(40, 130, 225));
        let red = accent_theme(iced::Color::from_rgb8(215, 60, 80));
        let amoled = design::amoled_theme(Some(iced::Color::from_rgb8(215, 60, 80)));
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
            (74.0, None, blue.clone(), 200.0, Memory, small),
            (74.0, None, blue, 200.0, Memory, small),
            (74.0, None, red.clone(), 200.0, Memory, small),
            (74.0, None, red, 200.0, Memory, small),
            (74.0, None, amoled.clone(), 200.0, Memory, small),
            (74.0, None, amoled, 200.0, Memory, small),
        ]
        .into_iter()
        .enumerate()
        {
            state.hovered = hovered;
            let chart = Chart {
                samples: std::array::from_fn(|slot| {
                    Some(ChartSample {
                        captured_at: None,
                        seconds_ago: (HISTORY_LEN - slot - 1) as f64,
                        reading: Some(ChartReading {
                            values: [value, 18.0],
                            labels: ["RAM".into(), "Cache".into()],
                        }),
                    })
                }),
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
            if matches!(index, 1 | 3 | 10 | 12 | 14) {
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
    fn chart_hover_stays_on_occupied_samples_and_clears_over_gaps_or_outside_plot() {
        use canvas::Program;
        let chart = Chart {
            samples: std::array::from_fn(|slot| {
                (slot != 35).then(|| ChartSample {
                    captured_at: None,
                    seconds_ago: (HISTORY_LEN - slot - 1) as f64,
                    reading: Some(ChartReading {
                        values: [1.0, 2.0],
                        labels: [String::new(), String::new()],
                    }),
                })
            }),
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
        assert!(move_to(307.0, 21.0));
        assert!(!move_to(307.1, 21.0));
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
    fn history_stays_bounded_without_plotting_unavailable_values() {
        let mut model = Model::default();
        model.record(Sample::default());
        assert!(model
            .chart(ChartKind::Cpu)
            .samples
            .iter()
            .flatten()
            .all(|sample| sample.reading.is_none()));
        for n in 0..HISTORY_LEN + 3 {
            model.record(Sample {
                cpu: CpuUsageSnapshot {
                    percent: Some(n as f32),
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        assert_eq!(model.history.len(), HISTORY_LEN);
        assert_eq!(model.history.front().unwrap().cpu.percent, Some(3.0));
        assert!(model
            .chart(ChartKind::Memory)
            .samples
            .iter()
            .flatten()
            .all(|sample| sample.reading.is_none()));
        assert_eq!(
            normalize_cpu_frequency_percent(Some(4000), 2000, Some(4000)),
            100.0
        );
    }
}
