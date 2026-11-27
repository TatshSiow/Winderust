use super::{assets, design, widgets};
use crate::config::Settings;
use crate::ui::{self, Page};
use iced::{advanced::widget::Id, Element, Fill};
use rust_i18n::t;
use std::collections::HashSet;

pub(super) const SEARCH_ID: &str = "navigation-search-trigger";
pub(super) const PAGE_LIST_ID: &str = "navigation-page-list";

#[derive(Debug, Clone)]
pub(super) enum Event {
    Select(Page),
    Search,
}

pub(super) fn workspace_id(page: Page) -> Id {
    format!("navigation-workspace-{page:?}").into()
}

pub(super) fn page_id(page: Page) -> Id {
    format!("navigation-page-{page:?}").into()
}

pub(super) fn children(section: Page) -> Vec<Page> {
    section
        .child_pages()
        .unwrap_or_default()
        .iter()
        .copied()
        .filter(|page| *page != section)
        .collect()
}

fn is_utility(page: Page) -> bool {
    matches!(page, Page::ActionLog | Page::SettingsHome | Page::About)
}

fn workspace_sections(show_advanced: bool) -> impl Iterator<Item = &'static ui::PageSection> {
    let sections = Page::sections().iter();
    sections
        .clone()
        .filter(move |section| {
            !is_utility(section.landing_page)
                && (show_advanced || section.landing_page != Page::AdvancedControls)
        })
        .chain(sections.filter(|section| is_utility(section.landing_page)))
}

pub(super) fn workspace_pages(show_advanced: bool) -> impl Iterator<Item = Page> {
    workspace_sections(show_advanced).map(|section| section.landing_page)
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
    page.child_pages()
        .is_some_and(|pages| pages.iter().any(|child| *child != page))
}

pub(super) fn top_bar(
    current: Page,
    show_advanced: bool,
    search_open: bool,
) -> Element<'static, Event> {
    use iced::widget::{container, row, text, Space};

    let active = current.section_landing_page();
    let inset = design::space::CONTROL as u16;
    let style_button = |content: Element<'static, Event>| {
        widgets::button(container(content).center_y(Fill))
            .height(design::BUTTON_HEIGHT)
            .padding([0, inset])
    };
    let mut workspaces = row![].spacing(design::space::TINY).align_y(iced::Center);
    for page in workspace_pages(show_advanced) {
        workspaces = workspaces.push(
            style_button(
                text(page.label())
                    .size(design::typography::SECONDARY)
                    .wrapping(text::Wrapping::None)
                    .into(),
            )
            .focus_id(workspace_id(page))
            .selected(active == page, widgets::selected)
            .on_press(Event::Select(page)),
        );
    }

    let search = style_button(
        row![
            glyph("icons/search.svg"),
            text(t!("home.search").into_owned())
                .size(design::typography::SECONDARY)
                .wrapping(text::Wrapping::None),
        ]
        .align_y(iced::Center)
        .spacing(design::space::TIGHT)
        .into(),
    )
    .focus_id(SEARCH_ID)
    .selected(search_open, |theme, status| {
        let mut style = widgets::quiet(theme, status);
        if status == iced::widget::button::Status::Active {
            style.background = Some(theme.palette().primary.scale_alpha(0.05).into());
        }
        style
    })
    .on_press(Event::Search);

    container(
        workspaces
            .push(Space::new().width(Fill))
            .push(search)
            .height(Fill),
    )
    .padding([0, design::space::SMALL as u16])
    .width(Fill)
    .height(design::NAVIGATION_ROW_HEIGHT)
    .into()
}

pub(super) fn page_list(current: Page, settings: &Settings) -> Option<Element<'static, Event>> {
    use iced::widget::{column, container, row, text, Space};

    let section = current.section_landing_page();
    if !has_children(section) {
        return None;
    }

    let count = settings
        .general
        .show_enabled_feature_counts_in_sidebar
        .then(|| section_enabled_feature_count(settings, section))
        .flatten();

    let mut entries = column![].spacing(design::space::TINY).width(Fill);
    for page in std::iter::once(section).chain(children(section)) {
        let active = current == page;
        let mut entry = row![
            icon(page, active),
            text(page.label())
                .size(design::typography::SECONDARY)
                .wrapping(text::Wrapping::WordOrGlyph)
                .width(Fill),
        ]
        .spacing(design::space::SMALL)
        .align_y(iced::Center);

        if let Some(count) = count.filter(|_| page == section) {
            entry = entry.push(
                container(text(count.to_string()).size(design::typography::BADGE))
                    .padding([2, 6])
                    .style(move |theme| widgets::indicator_chip(theme, count > 0)),
            );
        } else if let Some(enabled) = feature_page_enabled(settings, page) {
            entry = entry.push(container(Space::new()).width(6).height(6).style(
                move |theme: &iced::Theme| container::Style {
                    background: enabled.then(|| theme.palette().success.into()),
                    border: iced::border::rounded(3),
                    ..Default::default()
                },
            ));
        }

        entries = entries.push(
            widgets::button(entry)
                .focus_id(page_id(page))
                .width(Fill)
                .padding(design::space::SMALL as u16)
                .selected(active, widgets::selected)
                .on_press(Event::Select(page)),
        );
    }

    let scroll = super::scrolling::navigation_scrollable(
        entries.padding([design::space::CONTROL as u16, 0]),
    )
    .id(PAGE_LIST_ID)
    .width(Fill)
    .height(Fill);
    Some(super::motion::wrap(
        scroll,
        false,
        super::motion::Effect::Content(section as u64),
    ))
}

pub(super) fn icon<'a, Message: 'a>(page: Page, selected: bool) -> Element<'a, Message> {
    assets::icon(icon_path(page), design::ICON_SIZE, move |theme| {
        if selected {
            super::widgets::accent_text(
                theme,
                Some(theme.palette().primary.scale_alpha(0.22).into()),
            )
        } else {
            super::widgets::muted_color(theme)
        }
    })
}

pub(super) fn glyph<'a, Message: 'a>(path: &'static str) -> Element<'a, Message> {
    glyph_with_opacity(path, 1.0)
}

pub(super) fn glyph_with_opacity<'a, Message: 'a>(
    path: &'static str,
    opacity: f32,
) -> Element<'a, Message> {
    assets::icon(path, design::ICON_SIZE, move |theme| {
        super::widgets::muted_color(theme).scale_alpha(opacity)
    })
}
pub(super) fn icon_path(page: Page) -> &'static str {
    match page {
        Page::Home => "icons/house.svg",
        Page::PowerPlanControl => "icons/zap.svg",
        Page::Watchdog => "icons/shield.svg",
        Page::CpuControl => "icons/cpu.svg",
        Page::PriorityControl => "icons/circle-fading-arrow-up.svg",
        Page::SettingsHome => "icons/settings.svg",
        Page::AdvancedControls => "icons/cog.svg",
        Page::ByActivity => "icons/square-activity.svg",
        Page::ByCpuLoad => "icons/chart-column.svg",
        Page::AdaptiveProcessorPower => "icons/zap.svg",
        Page::AdvancedPowerPlanTuning => "icons/drill.svg",
        Page::ProcessPriority => "icons/panels-top-left.svg",
        Page::ThreadPriority => "icons/spline.svg",
        Page::DynamicPriorityBoost => "icons/trending-up-down.svg",
        Page::CpuLimiter => "icons/octagon-minus.svg",
        Page::CpuSetsSoft => "icons/life-buoy.svg",
        Page::ProcessList => "icons/list.svg",
        Page::AdaptiveEngine => "icons/brain-circuit.svg",
        Page::EfficiencyMode => "icons/leaf.svg",
        Page::AppSuspension => "icons/monitor-pause.svg",
        Page::SleepPrevention => "icons/coffee.svg",
        Page::KeepRunning => "icons/rotate-ccw.svg",
        Page::InstanceLimits => "icons/list.svg",
        Page::InstanceBalancing => "icons/scale.svg",
        Page::ByRunningApp => "icons/footprints.svg",
        Page::IoPriority => "icons/rotate-3d.svg",
        Page::GpuPriority => "icons/gpu.svg",
        Page::MemoryPriority => "icons/memory-stick.svg",
        Page::MemoryTrim => "icons/scissors.svg",
        Page::ResourceThresholds => "icons/square-activity.svg",
        Page::ProcessorAffinityHard => "icons/monitor-x.svg",
        Page::ByFocus => "icons/bring-to-front.svg",
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
        Page::AdaptiveProcessorPower => settings.adaptive_engine.adaptive_processor_power_enabled,
        Page::EfficiencyMode => settings.efficiency_mode.enabled,
        Page::MemoryTrim => settings.memory_trim.enabled,
        Page::ResourceThresholds => settings.resource_thresholds.enabled,
        Page::KeepRunning => settings.keep_running.enabled,
        Page::InstanceLimits => settings.instance_limits.enabled,
        Page::InstanceBalancing => settings.instance_balancing.enabled,
        Page::ByFocus => settings.by_focus.enabled,
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
        Page::SleepPrevention => settings.sleep_prevention.enabled,
        Page::TimerResolution => settings.timer_resolution.enabled,
        _ => return None,
    })
}

pub(super) fn feature_toggle(page: Page) -> Option<fn(&mut Settings) -> &mut bool> {
    Some(match page {
        Page::AdaptiveEngine => |settings| &mut settings.adaptive_engine.enabled,
        Page::AdaptiveProcessorPower => {
            |settings| &mut settings.adaptive_engine.adaptive_processor_power_enabled
        }
        Page::EfficiencyMode => |settings| &mut settings.efficiency_mode.enabled,
        Page::MemoryTrim => |settings| &mut settings.memory_trim.enabled,
        Page::ResourceThresholds => |settings| &mut settings.resource_thresholds.enabled,
        Page::KeepRunning => |settings| &mut settings.keep_running.enabled,
        Page::InstanceLimits => |settings| &mut settings.instance_limits.enabled,
        Page::InstanceBalancing => |settings| &mut settings.instance_balancing.enabled,
        Page::ByFocus => |settings| &mut settings.by_focus.enabled,
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
        Page::SleepPrevention => |settings| &mut settings.sleep_prevention.enabled,
        Page::TimerResolution => |settings| &mut settings.timer_resolution.enabled,
        _ => return None,
    })
}

pub(super) fn section_enabled_feature_count(settings: &Settings, page: Page) -> Option<usize> {
    if !matches!(
        page,
        Page::Watchdog | Page::PowerPlanControl | Page::PriorityControl | Page::CpuControl
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
) -> impl Iterator<Item = &'static ui::PageSection> {
    workspace_sections(show_advanced_controls).filter(|section| section.landing_page != Page::Home)
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
        if section_matches && seen.insert(section.landing_page) {
            pages.push(section.landing_page);
        }

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
    let mut text = dashboard_page_search_text(page);
    if text.is_ascii() {
        text.make_ascii_lowercase();
    } else {
        text = text.to_lowercase();
    }
    query.split_whitespace().all(|term| text.contains(term))
}

pub(super) fn dashboard_page_search_text(page: Page) -> String {
    let label = page.label();
    let section = page.section_label();
    let mut search = match page {
        Page::Home => format!(
            "{label} {section} {} {} overview summary current automation decision power plan cpu enabled rules",
            t!("home.intro_1"),
            t!("home.intro_2"),
        ),
        Page::PowerPlanControl => format!(
            "{label} {section} {} {} power plan automation foreground focused app running app performance mode cpu load activity idle schedule time battery plugged ac dc",
            t!("power_plan_control.restore_previous_plan"),
            t!("power_plan_control.restore_previous_plan_help"),
        ),
        Page::Watchdog => format!("{label} {section} app automation"),
        Page::CpuControl => format!(
            "{label} {section} processor cpu controls limiter background restriction affinity steering e cores p cores",
        ),
        Page::PriorityControl => format!(
            "{label} {section} priority control process thread dynamic boost io gpu memory launch registry ifeo scheduler base priority",
        ),
        Page::ActionLog => format!(
            "{label} {section} {} {} {} {} log action history details csv export skipped failed applied restored reason log level",
            t!("action_log.intro_1"),
            t!("action_log.intro_2"),
            t!("settings.action_log_mode"),
            t!("action_log.level_filter"),
        ),
        Page::SettingsHome => format!(
            "{label} {section} settings winderust behaviour startup tray toggles fail suppression appearance language theme accent color palette",
        ),
        Page::AdvancedControls => format!(
            "{label} {section} advanced windows scheduler win32 priority separation quantum foreground boost registry",
        ),
        Page::ByActivity => format!(
            "{label} {section} {} idle active input keyboard mouse controller gamepad activity power plan battery plugged",
            t!("by_activity.enable_help"),
        ),
        Page::ByFocus => format!(
            "{label} {section} {} focus focused app process window power plan priority rule",
            t!("by_focus.enable_help"),
        ),
        Page::ByTime => format!(
            "{label} {section} {} time schedule clock date weekday overnight power plan",
            t!("by_time.enable_help"),
        ),
        Page::ByCpuLoad => format!(
            "{label} {section} {} cpu load usage threshold sustained power plan percent samples",
            t!("by_cpu_load.enable_help"),
        ),
        Page::AdaptiveProcessorPower => format!(
            "{label} {section} {} adaptive processor power core parking boost processor min max ac dc battery temporary power plan background pressure focus launch",
            t!("adaptive_processor_power.enable_help"),
        ),
        Page::AdvancedPowerPlanTuning => format!(
            "{label} {section} {} {} {} {} core parking processor power boost min max ac dc battery plugged performance saver balanced scheduling hybrid long short thread policy schedpolicy shortschedpolicy",
            t!("processor_power.target_plan_help"),
            t!("processor_power.performance_help"),
            t!("processor_power.balanced_help"),
            t!("processor_power.saver_help"),
        ),
        Page::CpuLimiter => format!(
            "{label} {section} {} {} {} {} cpu limiter allowed time duty cycle freeze resume background process audio video network stutter child job group",
            t!("cpu_limiter.limit_method_help"),
            t!("cpu_limiter.cpu_time_defaults_help"),
            t!("cpu_limiter.cpu_time_scope_help"),
            t!("cpu_limiter.rules_help"),
        ),
        Page::ProcessPriority => format!(
            "{label} {section} {} {} process priority base priority normal below normal idle above normal high background foreground exclusion",
            t!("process_priority.enable_help"),
            t!("priority_control.custom_rules_help"),
        ),
        Page::ThreadPriority => format!(
            "{label} {section} {} {} thread priority time critical highest above normal normal below normal lowest idle background foreground exclusion",
            t!("thread_priority.enable_help"),
            t!("priority_control.custom_rules_help"),
        ),
        Page::DynamicPriorityBoost => format!(
            "{label} {section} {} {} dynamic priority boost process scheduler enabled disabled background foreground exclusion",
            t!("dynamic_priority_boost.enable_help"),
            t!("priority_control.custom_rules_help"),
        ),
        Page::CpuSetsSoft => format!(
            "{label} {section} {} {} cpu sets soft preferred processors per app foreground",
            t!("cpu_sets_soft.enable_help"),
            t!("cpu_allocation.rules_help"),
        ),
        Page::ProcessList => format!(
            "{label} {section} {} running processes pid process rules priority gpu cpu affinity efficiency policy overview",
            t!("process_list.title"),
        ),
        Page::AdaptiveEngine => format!(
            "{label} {section} {} adaptive engine cpu scheduling foreground background priority cpu spike stutter restraint processor placement",
            t!("adaptive_engine.enable_help"),
        ),
        Page::EfficiencyMode => format!(
            "{label} {section} {} {} {} {} efficiency mode efficiency_mode qos throttle background priority exclusion custom_rules",
            t!("efficiency_mode.enable_help"),
            t!("common.foreground_detection_help"),
            t!("common.visible_window_detection_help"),
            t!("efficiency_mode.custom_rules_help"),
        ),
        Page::AppSuspension => format!(
            "{label} {section} {} {} {} {} {} {} suspend freeze thaw resume background app process job object delay network audio",
            t!("app_suspension.enable_help"),
            t!("app_suspension.suspendable_help"),
            t!("app_suspension.auto_resume_when_focused"),
            t!("app_suspension.network_detection"),
            t!("app_suspension.audio_detection"),
            t!("app_suspension.temporary_resume"),
        ),
        Page::SleepPrevention => format!(
            "{label} {section} {} {} {} sleep prevention awake system idle display monitor running app power request",
            t!("sleep_prevention.enable_help"),
            t!("sleep_prevention.temporary_keep_awake"),
            t!("sleep_prevention.temporary_keep_display_on"),
        ),
        Page::ByRunningApp => format!(
            "{label} {section} {} {} running app performance mode power plan process game gaming active restore",
            t!("by_running_app.enable_help"),
            t!("by_running_app.rules_help"),
        ),
        Page::IoPriority => format!(
            "{label} {section} {} {} {} io i/o disk storage priority low very low background foreground detection default exclusion",
            t!("io_priority.enable_help"),
            t!("common.foreground_detection"),
            t!("priority_control.custom_rules_help"),
        ),
        Page::GpuPriority => format!(
            "{label} {section} {} {} {} gpu graphics scheduling priority d3dkmt idle below normal above normal foreground detection background default exclusion",
            t!("gpu_priority.enable_help"),
            t!("common.foreground_detection"),
            t!("priority_control.custom_rules_help"),
        ),
        Page::MemoryPriority => format!(
            "{label} {section} {} {} {} memory priority page priority ram paging working set very low low medium background foreground detection default exclusion",
            t!("memory_priority.enable_help"),
            t!("common.foreground_detection"),
            t!("priority_control.custom_rules_help"),
        ),
        Page::ResourceThresholds => format!(
            "{label} {section} {} monitor cpu memory ram threshold warning critical toast notification terminate kill process",
            t!("resource_thresholds.enable_help"),
        ),
        Page::InstanceLimits => format!(
            "{label} {section} {} instance limits count maximum processes monitor notifications full path",
            t!("instance_limits.enable_help"),
        ),
        Page::InstanceBalancing => format!(
            "{label} {section} {} instance balancing instances processes executable full path whole cores scheduling preferences CPU Sets",
            t!("instance_balancing.enable_help"),
        ),
        Page::KeepRunning => format!(
            "{label} {section} {} keep running relaunch restart app process exit arguments retry resume",
            t!("keep_running.enable_help"),
        ),
        Page::MemoryTrim => format!(
            "{label} {section} {} {} {} {} memory ram trim working set idle background exclusion",
            t!("memory_trim.enable_help"),
            t!("memory_trim.rules_help"),
            t!("memory_trim.exclude_foreground"),
            t!("memory_trim.exclude_visible_windows"),
        ),
        Page::ProcessorAffinityHard => format!(
            "{label} {section} {} {} processor affinity hard allowed processors per app foreground",
            t!("processor_affinity_hard.enable_help"),
            t!("cpu_allocation.rules_help"),
        ),
        Page::WinderustBehaviour => format!(
            "{label} {section} {} {} {} winderust behaviour startup tray automation toggle fail failure suppression export import",
            t!("settings.intro_1"),
            t!("settings.intro_2"),
            t!("settings.failure_suppression_threshold_help"),
        ),
        Page::LanguageAndAppearance => format!(
            "{label} {section} {} language appearance theme dark light amoled oled pure black system accent color palette localization display ui sidebar enabled feature counts status cards pills",
            t!("common.theme_help"),
        ),
        Page::ExperimentalFeatures => format!(
            "{label} {section} {} experimental features process priority realtime advanced priority values",
            t!("settings.expose_all_priority_values_help"),
        ),
        Page::TimerResolution => format!(
            "{label} {section} {} {} timer resolution ntsettimerresolution scheduler latency wakeups battery high resolution timer foreground process rule",
            t!("timer_resolution.enable_help"),
            t!("timer_resolution.rules_help"),
        ),
        Page::Win32PrioritySeparation => format!(
            "{label} {section} {} {} {} win32 priority separation windows scheduler quantum foreground boost games gaming registry",
            t!("settings.win32_priority_separation_quantum_duration_help"),
            t!("settings.win32_priority_separation_quantum_behaviour_help"),
            t!("settings.win32_priority_separation_foreground_boost_help"),
        ),
        Page::About => format!(
            "{label} {section} {} {} about version project winderust update automatic check startup stable pre-release channel",
            t!("about.intro_1"),
            t!("about.intro_2"),
        ),
    };
    if page != Page::PriorityControl && page.section_landing_page() == Page::PriorityControl {
        search.push(' ');
        search.push_str(&t!("priority_control.protect_child_processes"));
        search.push_str(" child processes descendants");
    }
    search
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchdog_pages_are_discoverable_with_advanced_controls_hidden() {
        assert_eq!(
            workspace_pages(false).take(3).collect::<Vec<_>>(),
            [Page::Home, Page::ProcessList, Page::Watchdog],
        );
        assert_eq!(
            dashboard_search_pages("watchdog", false),
            [
                Page::Watchdog,
                Page::ResourceThresholds,
                Page::MemoryTrim,
                Page::KeepRunning,
                Page::SleepPrevention,
                Page::InstanceLimits,
                Page::AppSuspension,
            ]
        );
        assert!(feature_toggle(Page::Watchdog).is_none());
        assert!(dashboard_search_pages("memory trim", false).contains(&Page::MemoryTrim));
    }

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
            vec![Page::Home, Page::CpuControl, Page::AdaptiveEngine]
        );
        assert_eq!(
            breadcrumb_path(Page::MemoryTrim),
            vec![Page::Home, Page::Watchdog, Page::MemoryTrim]
        );
        assert_eq!(
            breadcrumb_path(Page::EfficiencyMode),
            vec![Page::Home, Page::PriorityControl, Page::EfficiencyMode]
        );
    }

    #[test]
    fn page_list_is_only_present_for_workspaces_with_children() {
        let settings = Settings::default();
        for section in Page::sections() {
            for page in std::iter::once(section.landing_page).chain(section.pages.iter().copied()) {
                assert_eq!(
                    page_list(page, &settings).is_some(),
                    section.pages != [section.landing_page],
                    "{page:?}",
                );
            }
        }
    }

    #[test]
    fn advanced_workspace_is_optional_and_utilities_keep_their_order() {
        let standard: Vec<_> = workspace_pages(false).collect();
        let extended: Vec<_> = workspace_pages(true).collect();
        assert_eq!(
            &standard[standard.len() - 3..],
            &[Page::ActionLog, Page::SettingsHome, Page::About],
        );
        assert_eq!(extended[standard.len() - 3], Page::AdvancedControls);
        assert_eq!(
            extended
                .into_iter()
                .filter(|page| *page != Page::AdvancedControls)
                .collect::<Vec<_>>(),
            standard,
        );
    }

    #[test]
    fn every_page_has_a_bundled_icon_and_sidebar_counts_follow_feature_settings() {
        for section in Page::sections() {
            assert!(assets::has_icon(icon_path(section.landing_page)));
            for page in section.pages {
                assert!(assets::has_icon(icon_path(*page)));
            }
        }
        let mut settings = Settings::default();
        assert_eq!(
            &Page::PriorityControl.child_pages().unwrap()[..2],
            &[Page::ProcessPriority, Page::EfficiencyMode]
        );
        for page in [
            Page::ProcessPriority,
            Page::EfficiencyMode,
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
        settings.efficiency_mode.enabled = true;
        assert_eq!(
            section_enabled_feature_count(&settings, Page::PriorityControl),
            Some(2)
        );
        let watchdog_count = section_enabled_feature_count(&settings, Page::Watchdog).unwrap();
        settings.memory_trim.enabled = true;
        assert_eq!(
            section_enabled_feature_count(&settings, Page::Watchdog),
            Some(watchdog_count + 1)
        );
        settings.adaptive_engine.enabled = false;
        let cpu_count = section_enabled_feature_count(&settings, Page::CpuControl).unwrap();
        settings.adaptive_engine.enabled = true;
        assert_eq!(
            Page::CpuControl.child_pages().unwrap().first(),
            Some(&Page::AdaptiveEngine)
        );
        assert_eq!(
            section_enabled_feature_count(&settings, Page::CpuControl),
            Some(cpu_count + 1)
        );
        assert!(dashboard_search_pages("adaptive engine", false).contains(&Page::AdaptiveEngine));
        assert!(dashboard_search_pages("efficiency mode", false).contains(&Page::EfficiencyMode));
        assert!(dashboard_search_pages("suspension", false).contains(&Page::AppSuspension));
        assert!(!dashboard_search_pages("timer resolution", false).contains(&Page::TimerResolution));
        assert!(dashboard_search_pages("timer resolution", true).contains(&Page::TimerResolution));
    }
}
