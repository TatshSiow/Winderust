use super::*;

pub(super) fn apply_language(language: AppLanguage) {
    rust_i18n::set_locale(language.locale());
}

pub(super) fn breadcrumb_button(
    id: SharedString,
    target: Page,
    label: String,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    let hover_bg: Hsla = rgb(settings_card_hover_color()).into();

    breadcrumb_label_base(label, Some(360.0))
        .id(id)
        .flex_shrink_0()
        .opacity(0.68)
        .hover(move |style| style.bg(hover_bg))
        .cursor_pointer()
        .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
            app.navigate_to(target, cx);
        }))
}

pub(super) fn breadcrumb_label_base(label: String, max_width: Option<f32>) -> gpui::Div {
    let label_container = h_flex()
        .min_w(px(0.0))
        .items_center()
        .overflow_hidden()
        .px_1()
        .py(px(2.0))
        .rounded(px(BRAND_RADIUS_CONTROL))
        .text_size(px(TEXT_PAGE_TITLE_SIZE))
        .line_height(px(TEXT_PAGE_TITLE_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::SEMIBOLD);

    let label_container = if let Some(max_width) = max_width {
        label_container.max_w(px(max_width))
    } else {
        label_container
    };

    label_container.child(div().flex_1().min_w(px(0.0)).truncate().child(label))
}

pub(super) fn breadcrumb_separator() -> gpui::Div {
    div()
        .flex_shrink_0()
        .text_size(px(TEXT_PAGE_CRUMB_SIZE))
        .line_height(px(TEXT_PAGE_CRUMB_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(dim_text_color()))
        .opacity(0.48)
        .child(Icon::new(NavIcon::ChevronRight).with_size(px(16.0)))
}

pub(super) fn breadcrumb_trail(page: Page) -> Vec<BreadcrumbSegment> {
    if page == Page::Home {
        return vec![BreadcrumbSegment {
            page,
            label: page.label(),
        }];
    }

    let section_page = page.section_landing_page();
    let mut trail = vec![BreadcrumbSegment {
        page: Page::Home,
        label: Page::Home.label(),
    }];

    if page != section_page {
        trail.push(BreadcrumbSegment {
            page: section_page,
            label: page.section_label(),
        });
    }

    trail.push(BreadcrumbSegment {
        page,
        label: page.label(),
    });

    trail
}

pub(super) fn common_breadcrumb_prefix_len(
    previous: &[BreadcrumbSegment],
    current: &[BreadcrumbSegment],
) -> usize {
    previous
        .iter()
        .zip(current.iter())
        .take_while(|(previous, current)| previous.page == current.page)
        .count()
}

pub(super) fn breadcrumb_starts_with(
    trail: &[BreadcrumbSegment],
    prefix: &[BreadcrumbSegment],
) -> bool {
    trail.len() >= prefix.len()
        && trail
            .iter()
            .zip(prefix.iter())
            .take(prefix.len())
            .all(|(trail, prefix)| trail.page == prefix.page)
}

pub(super) fn breadcrumb_plain_label(label: String, current: bool, flexible: bool) -> gpui::Div {
    breadcrumb_label_base(label, if flexible { None } else { Some(360.0) })
        .when(flexible, |label| label.flex_1().overflow_hidden())
        .when(!flexible, |label| label.flex_shrink_0())
        .when(!current, |label| label.opacity(0.68))
}

pub(super) fn breadcrumb_segment_element(
    segment: &BreadcrumbSegment,
    current: bool,
    interactive: bool,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    if interactive && !current {
        breadcrumb_button(
            SharedString::from(format!("breadcrumb-link-{:?}", segment.page)),
            segment.page,
            segment.label.clone(),
            cx,
        )
        .into_any_element()
    } else {
        breadcrumb_plain_label(segment.label.clone(), current, current && interactive)
            .into_any_element()
    }
}

pub(super) fn breadcrumb_segment_group(
    segment: &BreadcrumbSegment,
    current: bool,
    interactive: bool,
    cx: &mut Context<WinderustApp>,
) -> gpui::Div {
    let flexible = current && interactive;

    h_flex()
        .min_w(px(0.0))
        .items_center()
        .gap_2()
        .when(flexible, |group| group.flex_1().overflow_hidden())
        .when(!flexible, |group| group.flex_shrink_0())
        .child(breadcrumb_separator())
        .child(breadcrumb_segment_element(
            segment,
            current,
            interactive,
            cx,
        ))
}

pub(super) fn breadcrumb_transition_group(
    id: SharedString,
    entering: bool,
    group: gpui::Div,
) -> AnyElement {
    if entering {
        with_optional_motion(
            group,
            SharedString::from(format!("breadcrumb-enter-{id}")),
            MotionSpeed::Fast,
            |group| group,
            |group, delta| group.opacity(delta),
        )
    } else {
        with_optional_motion(
            group,
            SharedString::from(format!("breadcrumb-exit-{id}")),
            MotionSpeed::Fast,
            |group| group.opacity(0.0),
            |group, delta| group.opacity(1.0 - delta),
        )
    }
}

pub(super) fn breadcrumb_exit_overlay(
    transition: &BreadcrumbTransition,
    current_trail_len: usize,
    cx: &mut Context<WinderustApp>,
) -> gpui::Div {
    let mut overlay = h_flex()
        .absolute()
        .inset_0()
        .min_w(px(0.0))
        .items_center()
        .gap_2()
        .overflow_hidden();

    if let Some(first) = transition.previous.first() {
        overlay = overlay.child(
            breadcrumb_plain_label(first.label.clone(), transition.previous.len() == 1, false)
                .opacity(0.0),
        );
    }

    for (index, segment) in transition.previous.iter().enumerate().skip(1) {
        let current = index + 1 == transition.previous.len();
        let exiting = index >= current_trail_len;
        let group = breadcrumb_segment_group(segment, current, exiting && current, cx);

        if exiting {
            overlay = overlay.child(breadcrumb_transition_group(
                SharedString::from(format!("breadcrumb-{:?}-{index}", segment.page)),
                false,
                group,
            ));
        } else {
            overlay = overlay.child(group.opacity(0.0));
        }
    }

    overlay
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

pub(super) fn dashboard_page_matches_query(page: Page, query: &str) -> bool {
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
            "winderust features background efficiency background_efficiency workload engine foreground interactivity memory trim working set memory ram background restraint".to_string(),
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
            t!("processor_power.link_ac_dc_help").to_string(),
            t!("processor_power.performance_help").to_string(),
            t!("processor_power.balanced_help").to_string(),
            t!("processor_power.saver_help").to_string(),
            "core parking processor power boost min max ac dc battery plugged performance saver balanced".to_string(),
        ],
        Page::CoreLimiter => vec![
            t!("core_limiter.intro_1").to_string(),
            t!("core_limiter.intro_2").to_string(),
            t!("core_limiter.intro_3").to_string(),
            t!("core_limiter.focus_detection_help").to_string(),
            t!("core_limiter.rules_help").to_string(),
            "cpu cap limit core affinity threshold sustain cooldown background process".to_string(),
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
        Page::BackgroundCpuRestriction => vec![
            t!("background_cpu.intro_1").to_string(),
            t!("background_cpu.intro_2").to_string(),
            t!("background_cpu.focus_detection_help").to_string(),
            t!("background_cpu.exclusions_help").to_string(),
            "background cpu restriction cpu set affinity limit e cores foreground exclusion".to_string(),
        ],
        Page::ProcessList => vec![
            t!("process_list.title").to_string(),
            "running processes pid process rules priority gpu cpu affinity efficiency policy overview".to_string(),
        ],
        Page::AdaptiveEngine => vec![
            t!("adaptive_engine.intro_1").to_string(),
            t!("adaptive_engine.intro_2").to_string(),
            t!("adaptive_engine.intro_3").to_string(),
            t!("adaptive_engine.timer_requests_help").to_string(),
            "adaptive engine power saving background_efficiency timer resolution audio guard workload engine cpu scheduling uperf powersave balanced performance speed foreground boost background priority cpu spike stutter battery background".to_string(),
        ],
        Page::BackgroundEfficiency => vec![
            t!("background_efficiency.intro_1").to_string(),
            t!("background_efficiency.intro_2").to_string(),
            t!("background_efficiency.intro_3").to_string(),
            t!("background_efficiency.focus_detection_help").to_string(),
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
            t!("io_priority.background_default").to_string(),
            t!("io_priority.foreground_default").to_string(),
            t!("io_priority.exclusions_help").to_string(),
            "io i/o disk storage priority low very low background foreground detection default exclusion".to_string(),
        ],
        Page::GpuPriority => vec![
            t!("gpu_priority.intro_1").to_string(),
            t!("gpu_priority.intro_2").to_string(),
            t!("gpu_priority.enable").to_string(),
            t!("gpu_priority.foreground_detection").to_string(),
            t!("gpu_priority.background_default").to_string(),
            t!("gpu_priority.foreground_default").to_string(),
            t!("gpu_priority.exclusions_help").to_string(),
            "gpu graphics scheduling priority d3dkmt idle below normal above normal foreground detection background default exclusion".to_string(),
        ],
        Page::MemoryPriority => vec![
            t!("memory_priority.intro_1").to_string(),
            t!("memory_priority.intro_2").to_string(),
            t!("memory_priority.enable").to_string(),
            t!("memory_priority.foreground_detection").to_string(),
            t!("memory_priority.background_default").to_string(),
            t!("memory_priority.foreground_default").to_string(),
            t!("memory_priority.exclusions_help").to_string(),
            "memory priority page priority ram paging working set very low low medium background foreground detection default exclusion".to_string(),
        ],
        Page::MemoryTrim => vec![
            t!("memory_trim.intro_1").to_string(),
            t!("memory_trim.intro_2").to_string(),
            t!("memory_trim.intro_3").to_string(),
            t!("memory_trim.trim_working_sets_help").to_string(),
            t!("memory_trim.purge_standby_list_help").to_string(),
            t!("memory_trim.purge_system_file_cache_help").to_string(),
            "memory ram trim working set standby list file cache purge background exclusion".to_string(),
        ],
        Page::CoreSteering => vec![
            t!("core_steering.intro_1").to_string(),
            t!("core_steering.intro_2").to_string(),
            t!("core_steering.intro_3").to_string(),
            t!("core_steering.rules_help").to_string(),
            t!("core_steering.p_cores_help").to_string(),
            t!("core_steering.e_cores_help").to_string(),
            t!("core_steering.no_smt_help").to_string(),
            "core steering affinity cpu sets p cores e cores smt logical processor background process".to_string(),
        ],
        Page::WinderustBehaviour => vec![
            t!("settings.intro_1").to_string(),
            t!("settings.intro_2").to_string(),
            t!("settings.action_log_mode_full_help").to_string(),
            t!("settings.failure_suppression_threshold_help").to_string(),
            "winderust behaviour startup tray automation toggle action log detail fail failure suppression export import".to_string(),
        ],
        Page::LanguageAndAppearance => vec![
            "language appearance theme dark light system accent color palette localization display ui".to_string(),
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
            "about version project winderust update automatic check stable pre-release channel"
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

pub(super) fn page_body_shell() -> gpui::Div {
    v_flex().w_full().min_w(px(0.0)).gap_2()
}

pub(super) fn search_results_page_header(_cx: &mut Context<WinderustApp>) -> gpui::Div {
    h_flex()
        .w_full()
        .min_h(px(PAGE_HEADER_HEIGHT))
        .flex_shrink_0()
        .items_center()
        .overflow_hidden()
        .child(
            div()
                .min_w(px(0.0))
                .text_size(px(TEXT_PAGE_TITLE_SIZE))
                .line_height(px(TEXT_PAGE_TITLE_LINE_HEIGHT))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .truncate()
                .child(t!("home.search_results").to_string()),
        )
}

pub(super) fn page_content_frame(
    header: AnyElement,
    body: AnyElement,
    fill_height: bool,
    full_width: bool,
) -> gpui::Div {
    let body_frame = v_flex()
        .w_full()
        .min_w(px(0.0))
        .child(body)
        .when(fill_height, |body| {
            body.flex_1().h_full().min_h(px(0.0)).overflow_hidden()
        });
    let content = v_flex()
        .w_full()
        .min_w(px(0.0))
        .gap_2()
        .when(!full_width, |content| content.max_w(px(CONTENT_MAX_WIDTH)))
        .when(fill_height, |content| {
            content.flex_1().h_full().min_h(px(0.0)).overflow_hidden()
        })
        .child(header)
        .child(body_frame);

    h_flex()
        .w_full()
        .min_w(px(0.0))
        .justify_center()
        .px(px(24.0))
        .py(px(24.0))
        .when(fill_height, |frame| {
            frame
                .flex_1()
                .h_full()
                .min_h(px(0.0))
                .items_start()
                .overflow_hidden()
        })
        .child(content)
}
