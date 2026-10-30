use super::design;
use crate::config::Settings;
use crate::ui::{self, Page};
use iced::{Element, Theme};
use rust_i18n::t;
use std::collections::HashSet;
use std::time::{Duration, Instant};

const OPEN_DELAY: Duration = Duration::from_millis(250);
const CLOSE_DELAY: Duration = Duration::from_millis(300);

#[derive(Debug, Clone)]
pub(super) enum Event {
    Hover(Page),
    Leave(Page),
    OpenKeyboard(Page),
    Enter,
    Exit,
    Dismiss,
    Blur,
    Tick(Instant),
    Select(Page),
    Search,
    FocusMove(Option<Page>, bool),
    Tab(Option<Page>, bool),
    Key(iced::keyboard::Key, bool),
}

#[derive(Default)]
pub(super) struct Flyout {
    pub section: Option<Page>,
    pub selected: Option<usize>,
    hovered: Option<Page>,
    inside: bool,
    pending_open: Option<(Page, Instant)>,
    close_at: Option<Instant>,
}

impl Flyout {
    pub fn timer(&self) -> Option<(Instant, Duration)> {
        self.pending_open
            .map(|(_, due)| (due, OPEN_DELAY))
            .or_else(|| self.close_at.map(|due| (due, CLOSE_DELAY)))
    }

    pub fn dismiss(&mut self) {
        *self = Self::default();
    }

    pub fn update(&mut self, event: &Event, now: Instant) {
        match *event {
            Event::OpenKeyboard(page) => {
                self.section = Some(page);
                self.pending_open = None;
                self.close_at = None;
                self.selected = Some(0);
            }
            Event::Hover(page) => {
                self.hovered = Some(page);
                self.close_at = None;
                self.pending_open =
                    (self.section != Some(page)).then_some((page, now + OPEN_DELAY));
            }
            Event::Leave(page) if self.hovered == Some(page) => {
                self.hovered = None;
                self.pending_open = None;
                if !self.inside && self.section.is_some() {
                    self.close_at = Some(now + CLOSE_DELAY);
                }
            }
            Event::Enter => {
                self.inside = true;
                self.pending_open = None;
                self.close_at = None;
            }
            Event::Exit => {
                self.inside = false;
                if self.hovered.is_none() {
                    self.close_at = Some(now + CLOSE_DELAY);
                }
            }
            Event::Tick(_) => {
                if let Some((page, due)) = self.pending_open {
                    if now >= due {
                        self.section = Some(page);
                        self.selected = None;
                        self.pending_open = None;
                    }
                }
                if self.close_at.is_some_and(|due| now >= due) {
                    self.dismiss();
                }
            }
            Event::Dismiss => self.dismiss(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod flyout_tests {
    use super::*;

    #[test]
    fn hover_delays_cancel_stale_opens_and_allow_crossing_into_the_panel() {
        let start = Instant::now();
        let mut state = Flyout::default();
        let mut send = |event, millis| {
            let now = start + Duration::from_millis(millis);
            state.update(&event, now);
            state.section
        };
        assert_eq!(send(Event::Hover(Page::SettingsHome), 0), None);
        assert_eq!(send(Event::Tick(start), 249), None);
        assert_eq!(send(Event::Hover(Page::PowerPlanControl), 249), None);
        assert_eq!(send(Event::Leave(Page::SettingsHome), 250), None);
        assert_eq!(send(Event::Tick(start), 498), None);
        assert_eq!(send(Event::Tick(start), 499), Some(Page::PowerPlanControl));
        send(Event::Leave(Page::PowerPlanControl), 500);
        send(Event::Enter, 650);
        assert_eq!(send(Event::Tick(start), 900), Some(Page::PowerPlanControl));
        send(Event::Exit, 1000);
        send(Event::Hover(Page::PowerPlanControl), 1299);
        assert_eq!(send(Event::Tick(start), 1400), Some(Page::PowerPlanControl));
        send(Event::Leave(Page::PowerPlanControl), 1500);
        assert_eq!(send(Event::Tick(start), 1799), Some(Page::PowerPlanControl));
        assert_eq!(send(Event::Tick(start), 1800), None);
        assert!(state.timer().is_none());
    }

    #[test]
    fn keyboard_open_and_dismissal_cancel_pending_timers() {
        let now = Instant::now();
        let mut state = Flyout::default();
        state.update(&Event::Hover(Page::SettingsHome), now);
        state.update(&Event::Leave(Page::SettingsHome), now);
        assert!(state.timer().is_none());
        state.update(&Event::Hover(Page::SettingsHome), now);
        state.update(&Event::OpenKeyboard(Page::PowerPlanControl), now);
        assert_eq!(state.section, Some(Page::PowerPlanControl));
        assert!(state.timer().is_none());
        state.update(&Event::Tick(now), now + Duration::from_secs(1));
        assert_eq!(state.section, Some(Page::PowerPlanControl));
        state.update(&Event::Hover(Page::SettingsHome), now);
        state.update(&Event::Dismiss, now);
        state.update(&Event::Tick(now), now + Duration::from_secs(1));
        assert_eq!(state.section, None);
        state.update(&Event::OpenKeyboard(Page::SettingsHome), now);
        assert_eq!(state.selected, Some(0));
        state.dismiss();
        assert_eq!(state.section, None);
        assert!(state.timer().is_none());
    }
}

pub(super) fn rail_id(page: Option<Page>) -> iced::advanced::widget::Id {
    format!("navigation-{page:?}").into()
}

pub(super) fn item_style(
    theme: &Theme,
    status: iced::widget::button::Status,
    active: bool,
    focused: bool,
) -> iced::widget::button::Style {
    let mut style = if active {
        super::widgets::selected(theme, status)
    } else {
        super::widgets::quiet(theme, status)
    };
    if focused {
        style.border.color = theme.palette().primary;
        style.border.width = 2.0;
    }
    style
}

pub(super) fn children(section: Page) -> Vec<Page> {
    Page::sections()
        .iter()
        .find(|group| group.landing_page == section)
        .map(|group| {
            group
                .pages
                .iter()
                .copied()
                .filter(|p| *p != section)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn is_utility(page: Page) -> bool {
    matches!(page, Page::ActionLog | Page::SettingsHome | Page::About)
}

pub(super) fn rail_pages(show_advanced: bool) -> Vec<Page> {
    let (mut primary, utilities): (Vec<_>, Vec<_>) = Page::sections()
        .iter()
        .map(|section| section.landing_page)
        .filter(|page| *page != Page::AdvancedControls || show_advanced)
        .partition(|page| !is_utility(*page));
    primary.extend(utilities);
    primary
}

pub(super) struct History {
    pages: Vec<Page>,
    cursor: usize,
}

impl Default for History {
    fn default() -> Self {
        Self {
            pages: vec![Page::Home],
            cursor: 0,
        }
    }
}

impl History {
    pub(super) fn visit(&mut self, page: Page) {
        if self.pages[self.cursor] != page {
            self.pages.truncate(self.cursor + 1);
            self.pages.push(page);
            self.cursor += 1;
        }
    }

    pub(super) fn travel(&mut self, forward: bool) -> Option<Page> {
        let cursor = if forward {
            self.cursor.checked_add(1)?
        } else {
            self.cursor.checked_sub(1)?
        };
        let page = *self.pages.get(cursor)?;
        self.cursor = cursor;
        Some(page)
    }
}

pub(super) fn breadcrumb_path(page: Page) -> Vec<Page> {
    let mut path = vec![Page::Home];
    let parent = page.section_landing_page();
    if parent != Page::Home && parent != page {
        path.push(parent);
    }
    if page != Page::Home {
        path.push(page);
    }
    path
}

pub(super) fn has_children(page: Page) -> bool {
    Page::sections().iter().any(|section| {
        section.landing_page == page && section.pages.iter().any(|child| *child != page)
    })
}

pub(super) fn rail_item<'a, Message: Clone + 'a>(
    page: Page,
    selected: bool,
    message: Message,
) -> Element<'a, Message> {
    use iced::widget::{column, container, stack, text};
    let mut icon_row = stack![container(icon(page, selected))
        .center_x(iced::Fill)
        .height(design::ICON_SIZE)];
    if has_children(page) {
        icon_row = icon_row.push(
            container(
                iced::widget::svg(
                    crate::ui::assets::iced_icon("icons/chevron-right.svg")
                        .expect("Navigation chevron is bundled"),
                )
                .width(12)
                .height(12)
                .style(|theme: &Theme, _| iced::widget::svg::Style {
                    color: Some(super::widgets::muted_color(theme)),
                }),
            )
            .align_right(iced::Fill)
            .center_y(design::ICON_SIZE),
        );
    }
    let content = column![
        icon_row,
        container(
            text(page.label())
                .size(design::typography::BADGE)
                .width(iced::Fill)
                .align_x(iced::Center),
        )
        .width(iced::Fill)
        .max_height(30)
        .clip(true),
    ]
    .spacing(design::space::TIGHT)
    .align_x(iced::Center);
    super::widgets::button(container(content).center_x(iced::Fill).center_y(iced::Fill))
        .width(iced::Fill)
        .height(design::NAVIGATION_ITEM_HEIGHT)
        .padding([design::space::CONTROL as u16, design::space::TIGHT as u16])
        .selected(selected, super::widgets::selected)
        .on_press(message)
        .into()
}

pub(super) fn page_feature_info(page: Page) -> String {
    let (prefix, paragraphs) = match page {
        Page::AdaptiveEngine => ("adaptive_engine", 3),
        Page::BackgroundEfficiency => ("background_efficiency", 3),
        Page::MemoryTrim => ("memory_trim", 3),
        Page::AppSuspension => ("app_suspension", 3),
        Page::CpuLimiter => ("cpu_limiter", 5),
        Page::ByActivity => ("by_activity", 2),
        Page::ByCpuLoad => ("by_cpu_load", 2),
        Page::ByForeground => ("by_foreground", 2),
        Page::ByRunningApp => ("by_running_app", 3),
        Page::ByTime => ("by_time", 2),
        Page::ProcessPriority => ("process_priority", 2),
        Page::ThreadPriority => ("thread_priority", 2),
        Page::DynamicPriorityBoost => ("dynamic_priority_boost", 2),
        Page::IoPriority => ("io_priority", 2),
        Page::GpuPriority => ("gpu_priority", 3),
        Page::MemoryPriority => ("memory_priority", 2),
        Page::CpuSetsSoft => ("cpu_sets_soft", 2),
        Page::ProcessorAffinityHard => ("processor_affinity_hard", 2),
        Page::TimerResolution => ("timer_resolution", 2),
        Page::WinderustBehaviour => ("settings", 2),
        Page::AdvancedPowerPlanTuning | Page::Win32PrioritySeparation => ("", 0),
        _ => return String::new(),
    };
    let mut help = (1..=paragraphs)
        .map(|index| {
            let key = format!("{prefix}.intro_{index}");
            t!(&key).to_string()
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    if matches!(
        page,
        Page::ByActivity | Page::ByCpuLoad | Page::ByForeground | Page::ByRunningApp | Page::ByTime
    ) {
        for key in [
            "common.power_plan_priority",
            "common.power_plan_pause_priority",
        ] {
            help.push_str("\n\n");
            help.push_str(&t!(key));
        }
    }
    let extra: &[&str] = match page {
        Page::CpuSetsSoft => &["cpu_allocation.rules_help", "cpu_sets_soft.warning"],
        Page::ProcessorAffinityHard => &[
            "cpu_allocation.rules_help",
            "processor_affinity_hard.warning",
        ],
        Page::AppSuspension => &["app_suspension.suspendable_help"],
        Page::ProcessPriority => &["process_priority.exclusions_help"],
        Page::ThreadPriority => &["thread_priority.exclusions_help"],
        Page::DynamicPriorityBoost => &["dynamic_priority_boost.exclusions_help"],
        Page::IoPriority => &["io_priority.exclusions_help"],
        Page::GpuPriority => &["gpu_priority.exclusions_help"],
        Page::MemoryPriority => &["memory_priority.exclusions_help"],
        Page::TimerResolution => &["timer_resolution.warning"],
        Page::Win32PrioritySeparation => &["settings.win32_priority_separation_warning"],
        Page::AdvancedPowerPlanTuning => &["processor_power.help"],
        _ => &[],
    };
    for key in extra {
        if !help.is_empty() {
            help.push_str("\n\n");
        }
        help.push_str(&t!(*key));
    }
    help
}

pub(super) fn icon<'a, Message: 'a>(page: Page, selected: bool) -> Element<'a, Message> {
    iced::widget::svg(
        crate::ui::assets::iced_icon(icon_path(page)).expect("Every UI icon is bundled"),
    )
    .width(design::ICON_SIZE)
    .height(design::ICON_SIZE)
    .style(move |theme: &Theme, _| iced::widget::svg::Style {
        color: Some(if selected {
            theme.palette().primary
        } else {
            super::widgets::muted_color(theme)
        }),
    })
    .into()
}

pub(super) fn glyph<'a, Message: 'a>(path: &'static str) -> Element<'a, Message> {
    iced::widget::svg(crate::ui::assets::iced_icon(path).expect("Every UI icon is bundled"))
        .width(design::ICON_SIZE)
        .height(design::ICON_SIZE)
        .style(|theme: &Theme, _| iced::widget::svg::Style {
            color: Some(super::widgets::muted_color(theme)),
        })
        .into()
}
pub(super) fn icon_path(page: Page) -> &'static str {
    match page {
        Page::Home => "icons/house.svg",
        Page::PowerPlanControl => "icons/zap.svg",
        Page::WinderustFeatures => "icons/computer.svg",
        Page::CpuControl => "icons/cpu.svg",
        Page::PriorityControl => "icons/circle-fading-arrow-up.svg",
        Page::SettingsHome => "icons/settings.svg",
        Page::AdvancedControls => "icons/cog.svg",
        Page::ByActivity => "icons/square-activity.svg",
        Page::ByCpuLoad => "icons/chart-column.svg",
        Page::AdvancedPowerPlanTuning => "icons/drill.svg",
        Page::ProcessPriority => "icons/panels-top-left.svg",
        Page::ThreadPriority => "icons/spline.svg",
        Page::DynamicPriorityBoost => "icons/trending-up-down.svg",
        Page::CpuLimiter => "icons/octagon-minus.svg",
        Page::CpuSetsSoft => "icons/life-buoy.svg",
        Page::ProcessList => "icons/list.svg",
        Page::AdaptiveEngine => "icons/brain-circuit.svg",
        Page::BackgroundEfficiency => "icons/leaf.svg",
        Page::AppSuspension => "icons/monitor-pause.svg",
        Page::ByRunningApp => "icons/footprints.svg",
        Page::IoPriority => "icons/rotate-3d.svg",
        Page::GpuPriority => "icons/gpu.svg",
        Page::MemoryPriority => "icons/memory-stick.svg",
        Page::MemoryTrim => "icons/scissors.svg",
        Page::ProcessorAffinityHard => "icons/monitor-x.svg",
        Page::ByForeground => "icons/bring-to-front.svg",
        Page::ByTime => "icons/calendar-days.svg",
        Page::ActionLog => "icons/square-menu.svg",
        Page::WinderustBehaviour => "icons/settings.svg",
        Page::LanguageAndAppearance => "icons/palette.svg",
        Page::ExperimentalFeatures => "icons/flask-conical.svg",
        Page::TimerResolution => "icons/hourglass.svg",
        Page::Win32PrioritySeparation => "icons/wrench.svg",
        Page::About => "icons/info.svg",
    }
}
pub(super) fn feature_page_enabled(settings: &Settings, page: Page) -> Option<bool> {
    Some(match page {
        Page::AdaptiveEngine => settings.adaptive_engine.enabled,
        Page::BackgroundEfficiency => settings.background_efficiency.enabled,
        Page::MemoryTrim => settings.memory_trim.enabled,
        Page::ByForeground => settings.by_foreground.enabled,
        Page::ByRunningApp => settings.by_running_app.enabled,
        Page::ByCpuLoad => settings.by_cpu_load.enabled,
        Page::ByActivity => settings.by_activity.enabled,
        Page::ByTime => settings.by_time.enabled,
        Page::ProcessPriority => settings.process_priority.enabled,
        Page::ThreadPriority => settings.thread_priority.enabled,
        Page::DynamicPriorityBoost => settings.dynamic_priority_boost.enabled,
        Page::IoPriority => settings.io_priority.enabled,
        Page::GpuPriority => settings.gpu_priority.enabled,
        Page::MemoryPriority => settings.memory_priority.enabled,
        Page::CpuLimiter => settings.cpu_limiter.enabled,
        Page::CpuSetsSoft => settings.cpu_sets_soft.enabled,
        Page::ProcessorAffinityHard => settings.processor_affinity_hard.enabled,
        Page::AppSuspension => settings.app_suspension.enabled,
        Page::TimerResolution => settings.timer_resolution.enabled,
        _ => return None,
    })
}

pub(super) fn feature_toggle(page: Page) -> Option<fn(&mut Settings) -> &mut bool> {
    Some(match page {
        Page::AdaptiveEngine => |settings| &mut settings.adaptive_engine.enabled,
        Page::BackgroundEfficiency => |settings| &mut settings.background_efficiency.enabled,
        Page::MemoryTrim => |settings| &mut settings.memory_trim.enabled,
        Page::ByForeground => |settings| &mut settings.by_foreground.enabled,
        Page::ByRunningApp => |settings| &mut settings.by_running_app.enabled,
        Page::ByCpuLoad => |settings| &mut settings.by_cpu_load.enabled,
        Page::ByActivity => |settings| &mut settings.by_activity.enabled,
        Page::ByTime => |settings| &mut settings.by_time.enabled,
        Page::ProcessPriority => |settings| &mut settings.process_priority.enabled,
        Page::ThreadPriority => |settings| &mut settings.thread_priority.enabled,
        Page::DynamicPriorityBoost => |settings| &mut settings.dynamic_priority_boost.enabled,
        Page::IoPriority => |settings| &mut settings.io_priority.enabled,
        Page::GpuPriority => |settings| &mut settings.gpu_priority.enabled,
        Page::MemoryPriority => |settings| &mut settings.memory_priority.enabled,
        Page::CpuLimiter => |settings| &mut settings.cpu_limiter.enabled,
        Page::CpuSetsSoft => |settings| &mut settings.cpu_sets_soft.enabled,
        Page::ProcessorAffinityHard => |settings| &mut settings.processor_affinity_hard.enabled,
        Page::AppSuspension => |settings| &mut settings.app_suspension.enabled,
        Page::TimerResolution => |settings| &mut settings.timer_resolution.enabled,
        _ => return None,
    })
}

pub(super) fn section_enabled_feature_count(settings: &Settings, page: Page) -> Option<usize> {
    if !matches!(
        page,
        Page::WinderustFeatures | Page::PowerPlanControl | Page::PriorityControl | Page::CpuControl
    ) {
        return None;
    }
    page.child_pages().map(|pages| {
        pages
            .iter()
            .filter(|page| feature_page_enabled(settings, **page) == Some(true))
            .count()
    })
}

pub(super) fn dashboard_sections_in_nav_order(
    show_advanced_controls: bool,
) -> Vec<&'static ui::PageSection> {
    Page::sections()
        .iter()
        .filter(|section| {
            section.landing_page != Page::Home && !nav_section_in_footer(section.landing_page)
        })
        .filter(|section| show_advanced_controls || section.landing_page != Page::AdvancedControls)
        .chain(
            Page::sections()
                .iter()
                .filter(|section| nav_section_in_footer(section.landing_page)),
        )
        .collect()
}

pub(super) fn dashboard_search_pages(query: &str, show_advanced_controls: bool) -> Vec<Page> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }

    let mut pages = Vec::new();
    let mut seen = HashSet::new();

    for section in dashboard_sections_in_nav_order(show_advanced_controls) {
        let section_matches = dashboard_page_matches_query(section.landing_page, &query);

        for page in section.pages.iter().copied() {
            if page == Page::Home || !seen.insert(page) {
                continue;
            }

            if section_matches || dashboard_page_matches_query(page, &query) {
                pages.push(page);
            }
        }
    }

    pages
}

fn dashboard_page_matches_query(page: Page, query: &str) -> bool {
    let text = dashboard_page_search_text(page).to_lowercase();
    query.split_whitespace().all(|term| text.contains(term))
}

pub(super) fn dashboard_page_search_text(page: Page) -> String {
    let mut text = format!("{} {}", page.label(), page.section_label());

    let extra = match page {
        Page::Home => vec![
            t!("home.intro_1").to_string(),
            t!("home.intro_2").to_string(),
            "overview summary current automation decision power plan cpu enabled rules".to_string(),
        ],
        Page::PowerPlanControl => vec![
            "power plan automation foreground focused app running app performance mode cpu load activity idle schedule time battery plugged ac dc".to_string(),
        ],
        Page::WinderustFeatures => vec![
            "winderust features background efficiency background_efficiency Adaptive Engine foreground interactivity memory trim working set memory ram background restraint".to_string(),
        ],
        Page::CpuControl => vec![
            "processor cpu controls core parking limiter background restriction affinity steering power boost ac dc battery e cores p cores".to_string(),
        ],
        Page::PriorityControl => vec![
            "priority control process thread dynamic boost io gpu memory launch registry ifeo scheduler base priority".to_string(),
        ],
        Page::ActionLog => vec![
            t!("action_log.intro_1").to_string(),
            t!("action_log.intro_2").to_string(),
            "log action history details csv export skipped failed applied restored reason".to_string(),
        ],
        Page::SettingsHome => vec![
            "settings winderust behaviour startup tray toggles action log detail fail suppression appearance language theme accent color palette".to_string(),
        ],
        Page::AdvancedControls => vec![
            "advanced app suspension windows scheduler win32 priority separation quantum foreground boost registry".to_string(),
        ],
        Page::ByActivity => vec![
            t!("by_activity.intro_1").to_string(),
            t!("by_activity.intro_2").to_string(),
            t!("by_activity.enable").to_string(),
            "idle active input keyboard mouse controller gamepad activity power plan battery plugged".to_string(),
        ],
        Page::ByForeground => vec![
            t!("by_foreground.intro_1").to_string(),
            t!("by_foreground.intro_2").to_string(),
            t!("by_foreground.enable").to_string(),
            "foreground focused app process window power plan priority rule".to_string(),
        ],
        Page::ByTime => vec![
            t!("by_time.intro_1").to_string(),
            t!("by_time.intro_2").to_string(),
            t!("by_time.enable").to_string(),
            "time schedule clock date weekday overnight power plan".to_string(),
        ],
        Page::ByCpuLoad => vec![
            t!("by_cpu_load.intro_1").to_string(),
            t!("by_cpu_load.intro_2").to_string(),
            t!("by_cpu_load.enable").to_string(),
            "cpu load usage threshold sustained power plan percent samples".to_string(),
        ],
        Page::AdvancedPowerPlanTuning => vec![
            t!("processor_power.help").to_string(),
            t!("processor_power.performance_help").to_string(),
            t!("processor_power.balanced_help").to_string(),
            t!("processor_power.saver_help").to_string(),
            "core parking processor power boost min max ac dc battery plugged performance saver balanced".to_string(),
        ],
        Page::CpuLimiter => vec![
            t!("cpu_limiter.intro_1").to_string(),
            t!("cpu_limiter.intro_2").to_string(),
            t!("cpu_limiter.intro_3").to_string(),
            t!("cpu_limiter.intro_4").to_string(),
            t!("cpu_limiter.intro_5").to_string(),
            t!("cpu_limiter.rules_help").to_string(),
            "cpu limiter allowed time duty cycle freeze resume background process audio video network stutter child job group".to_string(),
        ],
        Page::ProcessPriority => vec![
            t!("process_priority.intro_1").to_string(),
            t!("process_priority.intro_2").to_string(),
            t!("process_priority.exclusions_help").to_string(),
            "process priority base priority normal below normal idle above normal high background foreground exclusion".to_string(),
        ],
        Page::ThreadPriority => vec![
            t!("thread_priority.intro_1").to_string(),
            t!("thread_priority.intro_2").to_string(),
            t!("thread_priority.exclusions_help").to_string(),
            "thread priority time critical highest above normal normal below normal lowest idle background foreground exclusion".to_string(),
        ],
        Page::DynamicPriorityBoost => vec![
            t!("dynamic_priority_boost.intro_1").to_string(),
            t!("dynamic_priority_boost.intro_2").to_string(),
            t!("dynamic_priority_boost.exclusions_help").to_string(),
            "dynamic priority boost process scheduler enabled disabled background foreground exclusion".to_string(),
        ],
        Page::CpuSetsSoft => vec![
            t!("cpu_sets_soft.intro_1").to_string(),
            t!("cpu_sets_soft.intro_2").to_string(),
            t!("cpu_allocation.rules_help").to_string(),
            "cpu sets soft preferred processors per app foreground".to_string(),
        ],
        Page::ProcessList => vec![
            t!("process_list.title").to_string(),
            "running processes pid process rules priority gpu cpu affinity efficiency policy overview".to_string(),
        ],
        Page::AdaptiveEngine => vec![
            t!("adaptive_engine.intro_1").to_string(),
            t!("adaptive_engine.intro_2").to_string(),
            t!("adaptive_engine.intro_3").to_string(),
            "adaptive engine power saving background_efficiency Adaptive Engine cpu scheduling uperf powersave balanced performance speed foreground boost background priority cpu spike stutter battery background".to_string(),
        ],
        Page::BackgroundEfficiency => vec![
            t!("background_efficiency.intro_1").to_string(),
            t!("background_efficiency.intro_2").to_string(),
            t!("background_efficiency.intro_3").to_string(),
            t!("background_efficiency.foreground_detection_help").to_string(),
            t!("common.visible_window_detection_help").to_string(),
            t!("background_efficiency.custom_rules_help").to_string(),
            "efficiency mode background_efficiency qos throttle background priority exclusion custom_rules".to_string(),
        ],
        Page::AppSuspension => vec![
            t!("app_suspension.intro_1").to_string(),
            t!("app_suspension.intro_2").to_string(),
            t!("app_suspension.intro_3").to_string(),
            t!("app_suspension.suspendable_help").to_string(),
            "suspend freeze thaw resume background app process job object delay network audio".to_string(),
        ],
        Page::ByRunningApp => vec![
            t!("by_running_app.intro_1").to_string(),
            t!("by_running_app.intro_2").to_string(),
            t!("by_running_app.intro_3").to_string(),
            t!("by_running_app.rules_help").to_string(),
            "running app performance mode power plan process game gaming active restore".to_string(),
        ],
        Page::IoPriority => vec![
            t!("io_priority.intro_1").to_string(),
            t!("io_priority.intro_2").to_string(),
            t!("io_priority.enable").to_string(),
            t!("io_priority.foreground_detection").to_string(),
            t!("io_priority.exclusions_help").to_string(),
            "io i/o disk storage priority low very low background foreground detection default exclusion".to_string(),
        ],
        Page::GpuPriority => vec![
            t!("gpu_priority.intro_1").to_string(),
            t!("gpu_priority.intro_2").to_string(),
            t!("gpu_priority.enable").to_string(),
            t!("gpu_priority.foreground_detection").to_string(),
            t!("gpu_priority.exclusions_help").to_string(),
            "gpu graphics scheduling priority d3dkmt idle below normal above normal foreground detection background default exclusion".to_string(),
        ],
        Page::MemoryPriority => vec![
            t!("memory_priority.intro_1").to_string(),
            t!("memory_priority.intro_2").to_string(),
            t!("memory_priority.enable").to_string(),
            t!("memory_priority.foreground_detection").to_string(),
            t!("memory_priority.exclusions_help").to_string(),
            "memory priority page priority ram paging working set very low low medium background foreground detection default exclusion".to_string(),
        ],
        Page::MemoryTrim => vec![
            t!("memory_trim.intro_1").to_string(),
            t!("memory_trim.intro_2").to_string(),
            t!("memory_trim.intro_3").to_string(),
            "memory ram trim working set idle background exclusion".to_string(),
        ],
        Page::ProcessorAffinityHard => vec![
            t!("processor_affinity_hard.intro_1").to_string(),
            t!("processor_affinity_hard.intro_2").to_string(),
            t!("processor_affinity_hard.warning").to_string(),
            t!("cpu_allocation.rules_help").to_string(),
            "processor affinity hard allowed processors per app foreground".to_string(),
        ],
        Page::WinderustBehaviour => vec![
            t!("settings.intro_1").to_string(),
            t!("settings.intro_2").to_string(),
            t!("settings.action_log_mode_full_help").to_string(),
            t!("settings.failure_suppression_threshold_help").to_string(),
            "winderust behaviour startup tray automation toggle action log detail fail failure suppression export import".to_string(),
        ],
        Page::LanguageAndAppearance => vec![
            "language appearance theme dark light system accent color palette localization display ui sidebar enabled feature counts status cards pills".to_string(),
        ],
        Page::ExperimentalFeatures => vec![
            t!("settings.expose_all_priority_values_help").to_string(),
            "experimental features process priority realtime advanced priority values".to_string(),
        ],
        Page::TimerResolution => vec![
            t!("timer_resolution.intro_1").to_string(),
            t!("timer_resolution.intro_2").to_string(),
            t!("timer_resolution.warning").to_string(),
            "timer resolution ntsettimerresolution scheduler latency wakeups battery high resolution timer foreground process rule".to_string(),
        ],
        Page::Win32PrioritySeparation => vec![
            t!("settings.win32_priority_separation_quantum_duration_help").to_string(),
            t!("settings.win32_priority_separation_quantum_behaviour_help").to_string(),
            t!("settings.win32_priority_separation_foreground_boost_help").to_string(),
            "win32 priority separation windows scheduler quantum foreground boost games gaming registry".to_string(),
        ],
        Page::About => vec![
            t!("about.intro_1").to_string(),
            t!("about.intro_2").to_string(),
            "about version project winderust update automatic check startup stable pre-release channel"
                .to_string(),
        ],
    };

    for value in extra {
        text.push(' ');
        text.push_str(&value);
    }

    text
}

pub(super) fn nav_section_in_footer(page: Page) -> bool {
    matches!(page, Page::ActionLog | Page::SettingsHome | Page::About)
}

#[cfg(test)]
mod tests {
    #[test]
    fn feature_toggle_fields_match_navigation_indicators() {
        for page in Page::sections().iter().flat_map(|section| section.pages) {
            let mut settings = Settings::default();
            match feature_toggle(*page) {
                Some(field) => {
                    for enabled in [true, false] {
                        *field(&mut settings) = enabled;
                        assert_eq!(feature_page_enabled(&settings, *page), Some(enabled));
                    }
                }
                None => assert_eq!(feature_page_enabled(&settings, *page), None),
            }
        }
    }

    #[test]
    fn page_history_supports_back_forward_and_new_branches() {
        let mut history = History::default();
        assert_eq!(history.travel(false), None);
        history.visit(Page::ProcessList);
        history.visit(Page::ActionLog);
        assert_eq!(history.travel(false), Some(Page::ProcessList));
        history.visit(Page::ProcessList);
        assert_eq!(history.travel(true), Some(Page::ActionLog));
        assert_eq!(history.travel(true), None);
        assert_eq!(history.travel(false), Some(Page::ProcessList));
        history.visit(Page::About);
        assert_eq!(history.travel(true), None);
        assert_eq!(history.travel(false), Some(Page::ProcessList));
        assert_eq!(history.travel(false), Some(Page::Home));
    }

    #[test]
    fn breadcrumb_path_keeps_ancestors_in_order() {
        assert_eq!(breadcrumb_path(Page::Home), vec![Page::Home]);
        assert_eq!(
            breadcrumb_path(Page::AdaptiveEngine),
            vec![Page::Home, Page::WinderustFeatures, Page::AdaptiveEngine]
        );
    }

    #[test]
    fn cpu_limiter_feature_info_includes_the_warning() {
        let help = super::page_feature_info(crate::ui::Page::CpuLimiter);
        assert!(help.contains(&rust_i18n::t!("cpu_limiter.intro_1").to_string()));
        assert!(help.contains(&rust_i18n::t!("cpu_limiter.intro_4").to_string()));
        assert!(help.contains(&rust_i18n::t!("cpu_limiter.intro_5").to_string()));
    }

    use super::*;
    #[test]
    fn rail_items_fit_their_width_and_keep_geometry_when_selected() {
        fn leaves(
            node: &iced::advanced::layout::Node,
            offset: iced::Point,
            bounds: &mut Vec<iced::Rectangle>,
        ) {
            let local = node.bounds();
            let origin = iced::Point::new(offset.x + local.x, offset.y + local.y);
            if node.children().is_empty() {
                bounds.push(iced::Rectangle::new(origin, local.size()));
            } else {
                for child in node.children() {
                    leaves(child, origin, bounds);
                }
            }
        }
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        for section in Page::sections() {
            let mut sizes = Vec::new();
            for selected in [false, true] {
                let mut view = rail_item(section.landing_page, selected, ());
                let mut tree = iced::advanced::widget::Tree::new(&view);
                let layout = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &iced::advanced::layout::Limits::new(
                        iced::Size::ZERO,
                        iced::Size::new(design::NAVIGATION_RAIL_WIDTH - 12.0, 900.0),
                    ),
                );
                assert!(layout.size().width <= design::NAVIGATION_RAIL_WIDTH - 12.0);
                assert_eq!(layout.size().height, design::NAVIGATION_ITEM_HEIGHT as f32);
                let mut bounds = Vec::new();
                leaves(&layout, iced::Point::ORIGIN, &mut bounds);
                let icon = bounds
                    .iter()
                    .find(|bounds| bounds.size() == iced::Size::new(18.0, 18.0))
                    .expect("Rail item contains its icon");
                assert_eq!(icon.x + icon.width / 2.0, layout.size().width / 2.0);
                let label = bounds
                    .iter()
                    .find(|bounds| bounds.width > 18.0 && bounds.height > 12.0)
                    .expect("Rail item contains its label");
                let top = icon.y;
                let bottom = layout.size().height - (label.y + label.height);
                assert!(
                    (top - bottom).abs() < 0.01,
                    "Unequal rail insets: {top} / {bottom}"
                );
                assert!(
                    (label.y - (icon.y + icon.height) - design::space::TIGHT as f32).abs() < 0.01
                );
                if has_children(section.landing_page) {
                    let chevron = bounds
                        .iter()
                        .find(|bounds| bounds.size() == iced::Size::new(12.0, 12.0))
                        .expect("Group contains a chevron");
                    assert_eq!(chevron.y + chevron.height / 2.0, icon.y + icon.height / 2.0);
                    assert!(chevron.x > icon.x + icon.width);
                }
                sizes.push(layout.size());
            }
            assert_eq!(sizes[0], sizes[1]);
        }
        assert!(!has_children(Page::Home));
        assert!(!has_children(Page::ProcessList));
        assert!(has_children(Page::PowerPlanControl));
        assert!(has_children(Page::SettingsHome));
    }
    #[test]
    fn every_page_has_a_bundled_icon_and_sidebar_counts_follow_feature_settings() {
        for section in Page::sections() {
            assert!(crate::ui::assets::iced_icon(icon_path(section.landing_page)).is_some());
            for page in section.pages {
                assert!(crate::ui::assets::iced_icon(icon_path(*page)).is_some());
            }
        }
        let mut settings = Settings::default();
        for page in [
            Page::ProcessPriority,
            Page::ThreadPriority,
            Page::DynamicPriorityBoost,
            Page::IoPriority,
            Page::GpuPriority,
            Page::MemoryPriority,
        ] {
            assert_eq!(feature_page_enabled(&settings, page), Some(false));
        }
        settings.process_priority.enabled = true;
        assert_eq!(
            section_enabled_feature_count(&settings, Page::PriorityControl),
            Some(1)
        );
        assert!(!dashboard_search_pages("suspension", false).contains(&Page::AppSuspension));
        assert!(dashboard_search_pages("suspension", true).contains(&Page::AppSuspension));
    }
}
