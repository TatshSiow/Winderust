use super::design;
use crate::config::Settings;
use crate::ui::{self, Page};
use iced::{Element, Theme};
use rust_i18n::t;
use std::collections::HashSet;

pub(super) fn section<'a, Message: 'a>(
    header: impl Into<Element<'a, Message>>,
    children: Vec<Element<'a, Message>>,
    expanded: bool,
) -> Element<'a, Message> {
    let mut section = iced::widget::column![header.into()].spacing(design::space::TINY);
    // An empty column still participates in spacing; leaf pages must have no child slot.
    if expanded && !children.is_empty() {
        section = section.push(
            iced::widget::column(children)
                .spacing(design::space::TINY)
                .padding([0, design::space::COMPACT as u16]),
        );
    }
    section.into()
}

pub(super) fn label<'a, Message: 'a>(page: Page) -> Element<'a, Message> {
    use iced::widget::{container, text, tooltip};
    tooltip(
        container(
            text(page.label())
                .size(design::typography::SECONDARY)
                .wrapping(text::Wrapping::None),
        )
        .width(iced::Fill)
        .height(design::ICON_SIZE)
        .clip(true),
        text(page.label()),
        tooltip::Position::Right,
    )
    .style(container::bordered_box)
    .into()
}

pub(super) fn page_help(page: Page) -> String {
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
        _ => return String::new(),
    };
    (1..=paragraphs)
        .map(|index| {
            let key = format!("{prefix}.intro_{index}");
            t!(&key).to_string()
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(super) fn icon<'a, Message: 'a>(page: Page) -> Element<'a, Message> {
    iced::widget::svg(
        crate::ui::assets::iced_icon(icon_path(page)).expect("Every UI icon is bundled"),
    )
    .width(design::ICON_SIZE)
    .height(design::ICON_SIZE)
    .style(|theme: &Theme, _| iced::widget::svg::Style {
        color: Some(theme.palette().primary),
    })
    .into()
}

pub(super) fn glyph<'a, Message: 'a>(path: &'static str) -> Element<'a, Message> {
    iced::widget::svg(crate::ui::assets::iced_icon(path).expect("Every UI icon is bundled"))
        .width(design::ICON_SIZE)
        .height(design::ICON_SIZE)
        .style(|theme: &Theme, _| iced::widget::svg::Style {
            color: Some(theme.palette().text),
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
        Page::ActionLog => "icons/info.svg",
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
            "winderust features background efficiency background_efficiency CPU Scheduler foreground interactivity memory trim working set memory ram background restraint".to_string(),
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
            "adaptive engine power saving background_efficiency CPU Scheduler cpu scheduling uperf powersave balanced performance speed foreground boost background priority cpu spike stutter battery background".to_string(),
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
    use super::*;
    #[test]
    fn leaf_navigation_keeps_one_layout_child_after_first_visit() {
        let build = |expanded, children| {
            section::<()>(
                iced::widget::Space::new().height(design::NAVIGATION_ROW_HEIGHT),
                children,
                expanded,
            )
        };
        for expanded in [false, true] {
            assert_eq!(build(expanded, vec![]).as_widget().children().len(), 1);
        }
        let child = || {
            iced::widget::Space::new()
                .height(design::NAVIGATION_CHILD_ROW_HEIGHT)
                .into()
        };
        assert_eq!(build(false, vec![child()]).as_widget().children().len(), 1);
        assert_eq!(build(true, vec![child()]).as_widget().children().len(), 2);
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
