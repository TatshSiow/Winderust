//! Isolated native render evidence and real resident lifecycle checks.
//!
//! Fixtures never load or save portable settings. Automation is disabled except
//! for explicitly selected scheduler workloads on dedicated test executables.
//! PNG checks establish rendered frames; lifecycle assertions exercise behavior.
use super::*;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[path = "smoke/lifecycle.rs"]
mod lifecycle;
pub(super) use lifecycle::{check_pointer_input, LifecycleDriver};
#[path = "smoke/resources.rs"]
mod resources;
pub(super) use resources::{resource_window_tick, ResourceDriver};

pub(super) fn resource_requested() -> bool {
    resources::requested()
}

const RENDER_CASE: &str = "--windui-render-case";
const LIFECYCLE: &str = "--windui-lifecycle-smoke";
const BACKGROUND_DURATION: &str = "--windui-background-fixture-ms";
const EXTRA_CASES: usize = 60;

#[derive(Debug, Clone)]
struct RenderCase {
    index: usize,
    page: Page,
    extra: Option<usize>,
    chinese: bool,
    dark: bool,
    width: u32,
    height: u32,
    scale: f32,
}

fn pages() -> Vec<Page> {
    let mut result = Vec::new();
    for section in Page::sections() {
        for page in std::iter::once(section.landing_page).chain(section.pages.iter().copied()) {
            if !result.contains(&page) {
                result.push(page);
            }
        }
    }
    result
}

fn cases() -> Vec<RenderCase> {
    let pages = pages();
    let base = pages.len() * 2;
    (0..base + EXTRA_CASES)
        .map(|index| {
            let extra = index.checked_sub(base);
            let chinese = extra.map_or(index >= pages.len(), |extra| {
                matches!(extra, 1 | 5 | 7 | 9 | 11 | 27 | 29 | 31 | 33 | 35)
                    || (extra >= 36 && extra % 2 == 1)
            });
            let page = match extra {
                Some(0 | 18 | 57..=59) => Page::BackgroundEfficiency,
                Some(1) => Page::CpuLimiter,
                Some(2) => Page::SettingsHome,
                Some(3 | 13 | 26..=31) => Page::Home,
                Some(4..=10 | 17 | 24 | 50..=52) => Page::AdaptiveEngine,
                Some(11) => Page::MemoryTrim,
                Some(12) => Page::ByForeground,
                Some(14 | 32..=33 | 36..=39 | 53..=56) => Page::ProcessList,
                Some(40 | 41) => Page::CpuSetsSoft,
                Some(42 | 43) => Page::CpuLimiter,
                Some(44 | 45) => Page::ByTime,
                Some(46 | 47) => Page::ByCpuLoad,
                Some(48 | 49) => Page::AdvancedPowerPlanTuning,
                Some(15) => Page::PowerPlanControl,
                Some(16) => Page::ActionLog,
                Some(19) => Page::ByActivity,
                Some(20) => Page::AdvancedPowerPlanTuning,
                Some(21) => Page::AppSuspension,
                Some(22 | 34..=35) => Page::LanguageAndAppearance,
                Some(23) => Page::About,
                Some(25) => Page::ProcessPriority,
                _ => pages[index % pages.len()],
            };
            let large = extra.is_some_and(|extra| {
                (13..=25).contains(&extra) || matches!(extra, 50 | 52 | 54 | 56 | 58)
            });
            RenderCase {
                index,
                page,
                extra,
                chinese,
                dark: chinese || extra.is_some_and(|extra| extra >= 13),
                width: if large {
                    1920
                } else if chinese || extra.is_some() {
                    900
                } else {
                    1120
                },
                height: if matches!(extra, Some(50 | 52)) {
                    1056
                } else if large {
                    1080
                } else if chinese {
                    620
                } else {
                    760
                },
                scale: match extra {
                    Some(32 | 34) => 1.5,
                    Some(33 | 35) => 2.0,
                    _ => 1.0,
                },
            }
        })
        .collect()
}

fn argument(flag: &str) -> Option<String> {
    let mut args = std::env::args();
    args.find(|argument| argument == flag)
        .and_then(|_| args.next())
}

fn requested_case() -> Option<RenderCase> {
    argument(RENDER_CASE).map(|index| {
        let index: usize = index.parse().expect("render case must be a number");
        cases()
            .into_iter()
            .find(|case| case.index == index)
            .expect("unknown render case")
    })
}

pub(super) fn lifecycle_requested() -> bool {
    std::env::args().any(|argument| argument == LIFECYCLE)
}
pub(super) fn background_fixture_requested() -> bool {
    argument(BACKGROUND_DURATION).is_some()
}

fn background_fixture_visible() -> bool {
    std::env::args().any(|argument| argument == "--windui-background-fixture-visible")
}

fn background_duration() -> Option<Duration> {
    argument(BACKGROUND_DURATION).map(|value| {
        let milliseconds: u64 = value
            .parse()
            .expect("background fixture duration must be milliseconds");
        assert!(
            milliseconds > 0,
            "background fixture duration must be positive"
        );
        Duration::from_millis(milliseconds)
    })
}

pub(super) fn requested() -> bool {
    lifecycle_requested()
        || background_fixture_requested()
        || resource_requested()
        || argument(RENDER_CASE).is_some()
}

/// Runs before privilege checks, portable-settings IO, and startup registration.
pub(crate) fn run_if_requested() -> bool {
    if std::env::args().any(|argument| argument == "--render-smoke") {
        render_all_pages();
        return true;
    }
    if requested() {
        run_fixture();
        return true;
    }
    false
}

fn fixture_settings() -> crate::config::Settings {
    if resource_requested() {
        return resources::fixture_settings();
    }
    let mut settings = crate::config::Settings::default();
    settings.general.enabled = false;
    settings.general.check_for_updates = false;
    settings.general.hide_to_tray = lifecycle_requested() || background_fixture_requested();
    settings.general.start_minimized =
        lifecycle_requested() || (background_fixture_requested() && !background_fixture_visible());
    settings.general.startup_with_windows = false;
    settings.advanced.show_advanced_controls = true;
    if let Some(case) = requested_case() {
        settings.general.language = if case.chinese {
            crate::config::AppLanguage::ZhTw
        } else {
            crate::config::AppLanguage::English
        };
        settings.general.theme_mode = if case.dark {
            crate::config::AppThemeMode::Dark
        } else {
            crate::config::AppThemeMode::Light
        };
    }
    settings
}

fn run_fixture() {
    let started = Instant::now();
    let scheduler = crate::ui::scheduler_benchmark::Options::parse()
        .unwrap_or_else(|error| panic!("Scheduler benchmark: {error}"));
    assert!(
        scheduler.is_none() || resource_requested(),
        "Scheduler profiles require a resource fixture"
    );
    let enabled = scheduler.as_ref().is_some_and(|options| {
        options.profile != crate::ui::scheduler_benchmark::Profile::Disabled
    });
    if enabled {
        assert!(
            crate::privilege::is_running_as_admin(),
            "Scheduler benchmark must run elevated"
        );
    }
    // Hold the normal ownership guard until both runtime and recovery finish.
    let _guard = enabled.then(|| {
        crate::SingleInstanceGuard::acquire(false).unwrap_or_else(|| {
            panic!("Close Winderust before benchmarking; another instance owns the runtime")
        })
    });
    let mut settings = fixture_settings();
    if let Some(options) = &scheduler {
        options
            .apply(&mut settings)
            .unwrap_or_else(|error| panic!("Scheduler benchmark: {error}"));
    }
    let mut editor = if resource_requested() {
        SettingsEditor::benchmark_in_memory(settings)
    } else {
        SettingsEditor::with_settings(settings)
    };
    // Recovery must exist before any runtime controller starts, including smoke runs.
    let recovery = Arc::new(Mutex::new(crate::crash_recovery::RecoveryClient::start()));
    if enabled {
        assert!(
            crate::crash_recovery::startup_error().is_none(),
            "Scheduler benchmark recovery watchdog failed to start"
        );
    }
    let runtime = RuntimeHandle::start(&editor.runtime_settings_snapshot());
    let result = super::run(editor, None, runtime, None, Arc::clone(&recovery));
    let helper_result = recovery
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .finish();
    resources::finish_run(&result, &helper_result, started.elapsed());
    assert!(
        result.is_ok(),
        "Native smoke application failed: {result:?}"
    );
    assert!(
        helper_result.is_ok(),
        "Native smoke recovery completion failed: {helper_result:?}"
    );
    if lifecycle_requested() {
        println!(
            "Wind UI lifecycle smoke passed: zero-window worker delivery, held native navigation/Exit presses across refresh, modal blocking/cancellation, three destroy/reopen cycles, draft retention, exit cancellation, late-edit rejection, wake coalescing, and coordinated shutdown."
        );
    }
}

pub(super) fn configure(state: &Rc<RefCell<WinderustApp>>, app: App) -> App {
    if resource_requested() {
        return resources::configure(state, app);
    }
    if let Some(duration) = background_duration() {
        let sender = state.borrow().sender();
        let deadline = Instant::now() + duration;
        std::thread::Builder::new()
            .name("winderust-background-deadline".into())
            .spawn(move || {
                std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
                send(&sender, Message::Wake, false);
            })
            .unwrap_or_else(|error| panic!("Background fixture deadline worker: {error}"));
        // Keep production tray safety and visibility behavior; completion does not
        // depend on an application polling interval after the UI is destroyed.
        return if background_fixture_visible() {
            app.start_window(super::main_window(state))
        } else {
            app
        };
    }
    if lifecycle_requested() {
        // Exercise the healthy-tray close policy deterministically. Explorer
        // registration probing is a separate integration boundary.
        state.borrow().view.can_hide.set(true);
        let weak = Rc::downgrade(state);
        return app.on_interval(Duration::from_millis(100), move |ctx| {
            if let Some(state) = weak.upgrade() {
                state.borrow_mut().handle(
                    ctx,
                    Intent {
                        message: Message::Tick,
                        refresh: false,
                    },
                );
            }
        });
    }
    let Some(case) = requested_case() else {
        return app;
    };
    let mut model = state.borrow_mut();
    apply_fixture(&mut model, &case);
    let request = super::window_request(Rc::downgrade(state), model.view, model.sender());
    let path = argument("--screenshot").expect("render case requires --screenshot output");
    app.screenshot_window(request)
        .screenshot(path)
        .screenshot_from_args()
}

/// Drop fixture commands deliberately: rendering must not launch IO, file pickers,
/// update checks, or process-control jobs. Transitions still use production update.
fn apply_fixture(app: &mut WinderustApp, case: &RenderCase) {
    assert!(!app.settings.global().general.enabled);
    let _ = app.update(Message::Page(case.page));
    if case.page == Page::Home {
        for _ in 0..30 {
            app.home.record(home::Sample::smoke_sample());
        }
    }
    assert_eq!(app.page, case.page);
    assert!(!app.feature_info_expanded);
    app.error_message.clear();
    app.power_plans_loading = false;
    app.power_plans_loaded = true;
    app.power_plans = vec![
        crate::power::PowerPlan {
            guid: "381b4222-f694-41f0-9685-ff5bb260df2e".into(),
            name: "Balanced (fixture)".into(),
            active: true,
        },
        crate::power::PowerPlan {
            guid: "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c".into(),
            name: "High performance (fixture)".into(),
            active: false,
        },
    ];
    app.view.plan_catalog.set(app.power_plans.clone());
    app.catalog_loading = false;
    app.candidates = super::super::app_picker::smoke_candidates();
    app.view.candidate_catalog.set(app.candidates.clone());
    app.processes.smoke_population();
    app.status.action_log_entries = Arc::new(log_fixture());
    let extra = case.extra;
    if matches!(extra, Some(53..=56)) {
        app.status_collapsed = matches!(extra, Some(55 | 56));
        let process_list::Message::Select(index, revision) = app.processes.smoke_selection() else {
            unreachable!("Process fixture selection must identify a row")
        };
        let _ = app.update(Message::Processes(process_list::Message::Focus(
            index, revision,
        )));
    }
    if matches!(extra, Some(0 | 1 | 50..=52)) {
        let _ = app.update(Message::ToggleFeatureInfo);
        assert!(app.feature_info_expanded);
    }
    if extra == Some(0) {
        let section = Page::SettingsHome;
        let _ = app.update(Message::ToggleSection(section));
        assert_eq!(app.page, section);
        assert_eq!(app.expanded_section, Some(section));
        let _ = app.update(Message::ToggleSection(section));
        assert_eq!(app.expanded_section, None);
        let _ = app.update(Message::ToggleSection(section));
        assert_eq!(app.expanded_section, Some(section));
        let _ = app.update(Message::ToggleSection(Page::PowerPlanControl));
        assert_eq!(app.expanded_section, Some(Page::PowerPlanControl));
        let _ = app.update(Message::Page(case.page));
        assert_eq!(app.expanded_section, Some(case.page.section_landing_page()));
        let _ = app.update(Message::Efficiency(
            background_efficiency::Message::Collapse(true),
        ));
    }
    if extra == Some(3) {
        app.settings.general.navigation_collapsed = true;
    }
    if let Some(index @ 4..=12) = extra {
        app.settings.general.navigation_collapsed = false;
        if index <= 10 {
            use adaptive_engine::{Message as AdaptiveMessage, TuningTab};
            if index == 4 {
                let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(
                    TuningTab::CpuBehaviour,
                )));
                let _ = app.update(Message::Adaptive(AdaptiveMessage::Collapse(0)));
            } else if index == 5 || index == 8 {
                if index == 8 {
                    let _ = app.update(Message::Adaptive(AdaptiveMessage::New));
                }
                let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(
                    TuningTab::ProcessorPower,
                )));
            } else if index == 6 || index == 9 {
                let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(
                    TuningTab::PriorityControl,
                )));
            } else if index == 7 {
                app.settings.adaptive_engine_process.custom_rules.push(
                    crate::config::ProcessExclusionRule {
                        executable_path: r"C:\Apps\Example\example.exe".into(),
                        enabled: false,
                        ..Default::default()
                    },
                );
                let _ = app.update(Message::Adaptive(AdaptiveMessage::TuningTab(
                    TuningTab::CustomRules,
                )));
            } else {
                let _ = app.update(Message::Adaptive(AdaptiveMessage::Cancel));
                let _ = app.update(Message::Adaptive(AdaptiveMessage::ViewBuiltIn(
                    adaptive_engine::BuiltInAdaptiveEnginePreset::Balanced,
                )));
            }
        } else if index == 11 {
            app.settings
                .memory_trim
                .exclusions
                .push(crate::config::ProcessExclusionRule {
                    executable_path: r"C:\Apps\Example\example.exe".into(),
                    enabled: false,
                    ..Default::default()
                });
            let _ = app.update(Message::Trim(memory_trim::Message::Collapse(0)));
        } else {
            app.settings
                .by_foreground
                .rules
                .push(crate::ui::process_rules::new_foreground_rule(
                    r"C:\Apps\Example\example.exe",
                    None,
                ));
        }
    }
    if extra == Some(9) {
        let before = app.status_collapsed;
        let _ = app.update(Message::ToggleStatus);
        assert_eq!(app.status_collapsed, !before);
    }
    if matches!(extra, Some(50..=52)) {
        app.status_collapsed = false;
        assert!(app.feature_info_expanded);
    }
    if matches!(extra, Some(57 | 58)) {
        app.status_collapsed = true;
    }
    if extra == Some(59) {
        app.settings.general.navigation_collapsed = true;
        app.status_collapsed = false;
    }
    if extra == Some(52) {
        app.page_offsets
            .get(&Page::AdaptiveEngine)
            .expect("Adaptive Engine fixture scroll state")
            .set(700);
    }
    if extra == Some(18) {
        app.settings.background_efficiency.enabled = true;
    }
    if extra == Some(19) {
        app.settings.by_activity.enabled = true;
    }
    if extra == Some(21) {
        app.settings.app_suspension.enabled = true;
    }
    if extra == Some(24) {
        let _ = app.update(Message::Adaptive(adaptive_engine::Message::TuningTab(
            adaptive_engine::TuningTab::PriorityControl,
        )));
    }
    if extra == Some(17) {
        app.adaptive = adaptive_engine::Editor::default();
    }
    match extra {
        Some(26 | 27) => {
            app.error_message = if case.chinese { "測試錯誤：無法套用選取的設定。既有程序狀態已保留。" } else { "Smoke error: the selected setting could not be applied. Existing process state was retained." }.into();
        }
        Some(28 | 29) => {
            let original = app.settings.general.navigation_collapsed;
            app.settings.general.navigation_collapsed = !original;
            let _ = app.update(Message::Close);
            assert!(app.closing && app.settings.has_unsaved_changes());
        }
        Some(30 | 31) => {
            // Render busy state without starting restoration from a render fixture.
            app.closing = true;
            app.exiting = true;
            app.view.editable.set(false);
        }
        Some(34 | 35) => {
            let _ = app.update(Message::Preferences(settings_pages::Message::ToggleAccent));
        }
        Some(36..=39) => {
            let selected = app.processes.smoke_selection();
            let _ = app.update(Message::Processes(selected));
            let _ = app.update(Message::Processes(if matches!(extra, Some(38 | 39)) {
                process_list::Message::Stop(false)
            } else {
                process_list::Message::OpenDetails
            }));
        }
        Some(40 | 41) => {
            let _ = app.update(Message::Allocation(
                cpu_allocation::Kind::Soft,
                cpu_allocation::Message::NewPreset,
            ));
        }
        Some(42 | 43) => {
            let mut rule = super::super::process_rules::new_cpu_limiter_rule(r"C:\Apps\smoke.exe");
            rule.focus_mode = crate::config::ProcessRuleMode::Enabled;
            rule.background_mode = crate::config::ProcessRuleMode::Enabled;
            app.settings.cpu_limiter.rules.push(rule);
            let _ = app.update(Message::CpuLimiter(cpu_limiter::Message::Edit(0)));
        }
        Some(44 | 45) => {
            let _ = app.update(Message::PowerRules(
                power_rules::Kind::Time,
                power_rules::Message::Add,
            ));
        }
        Some(46 | 47) => {
            let index = app.settings.by_cpu_load.rules.len();
            let _ = app.update(Message::PowerRules(
                power_rules::Kind::CpuLoad,
                power_rules::Message::Add,
            ));
            let _ = app.update(Message::PowerRules(
                power_rules::Kind::CpuLoad,
                power_rules::Message::Comparison(index, crate::config::CpuUsageComparison::Between),
            ));
            let _ = app.update(Message::PowerRules(
                power_rules::Kind::CpuLoad,
                power_rules::Message::Else(index, true),
            ));
        }
        Some(48 | 49) => {
            let _ = app.update(Message::PowerTuning(
                advanced_power_plan_tuning::Message::OpenPreset(
                    advanced_power_plan_tuning::PresetTarget::Custom(None),
                ),
            ));
        }
        _ => {}
    }
    app.view.confirming.set(app.closing);
    assert!(
        !app.settings.global().general.enabled,
        "No render fixture may enable automation"
    );
    assert!(!app.settings.persisted().general.startup_with_windows);
}

fn log_fixture() -> Vec<crate::action_log::ActionLogEntry> {
    use crate::action_log::{ActionLogEntry, ActionLogFeature, ActionLogResult};
    [
        ActionLogResult::Applied,
        ActionLogResult::Restored,
        ActionLogResult::Skipped,
        ActionLogResult::Failed,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, result)| ActionLogEntry {
        sequence: index as u64 + 1,
        batch_id: index as u64 + 1,
        timestamp_epoch_ms: 1_792_368_000_000 + index as u128 * 1000,
        feature: ActionLogFeature::BackgroundEfficiency,
        process_id: Some(900_000 + index as u32),
        process_name: format!("Example {:02}.exe", index + 1),
        result,
        reason: if result == ActionLogResult::Failed {
            "Access denied while applying the requested setting; no target state was changed."
        } else {
            "Synthetic render evidence; no process operation was performed."
        }
        .into(),
    })
    .collect()
}

pub(super) fn tick(app: &mut WinderustApp, ctx: &mut EventCtx) {
    if let Some(mut driver) = app.smoke.take() {
        driver.tick(app, ctx);
        app.smoke = Some(driver);
    }
    if let Some(mut driver) = app.resource_benchmark.take() {
        driver.tick(app, ctx);
        app.resource_benchmark = Some(driver);
    }
}

pub(super) fn finish(app: &WinderustApp) {
    if let Some(driver) = &app.smoke {
        driver.finish(app);
    }
    if let Some(driver) = &app.resource_benchmark {
        driver.complete(app);
    }
    if requested() && !resource_requested() {
        assert!(
            !app.settings.global().general.enabled,
            "Smoke run enabled automation"
        );
    }
}

fn render_all_pages() {
    let executable = std::env::current_exe().expect("locate render smoke executable");
    let run_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_millis();
    // A fresh directory prevents stale frames from satisfying a failed run.
    let directory = PathBuf::from(format!(
        "target/windui-smoke/run-{}-{run_id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create smoke evidence directory");
    let mut evidence = Vec::new();
    let mut failures = Vec::new();
    for case in cases() {
        let path = directory.join(format!(
            "{:03}-{:?}-{}-{}.png",
            case.index,
            case.page,
            if case.chinese { "zh-TW" } else { "en" },
            if case.dark { "dark" } else { "light" }
        ));
        let outcome = render_case(&executable, &path, &case);
        let error = outcome.as_ref().err().cloned();
        evidence.push(serde_json::json!({
            "index":case.index,"page":format!("{:?}",case.page),"extra":case.extra,
            "locale":if case.chinese {"zh-TW"} else {"en"},"theme":if case.dark {"dark"} else {"light"},
            "logical_width":case.width,"logical_height":case.height,"scale":case.scale,
            "path":path,"rendered":outcome.is_ok(),"error":error,
        }));
        if let Err(error) = outcome {
            failures.push(format!("case {} {:?}: {error}", case.index, case.page));
        }
        println!("Rendered case {} {:?}", case.index, case.page);
    }
    let report = serde_json::json!({"cases":evidence,"failures":failures,"scope":"Native production window screenshots and fixture controller assertions. PNG validation checks dimensions and nonblank pixels, not visual correctness or OS action success."});
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(&report).expect("serialize smoke manifest"),
    )
    .expect("write smoke manifest");
    assert!(
        failures.is_empty(),
        "Render smoke failed:\n{}\nEvidence: {}",
        failures.join("\n"),
        directory.display()
    );
    println!(
        "Wind UI render evidence: {} cases in {}",
        cases().len(),
        directory.display()
    );
}

fn render_case(executable: &Path, path: &Path, case: &RenderCase) -> Result<(), String> {
    let log =
        std::fs::File::create(path.with_extension("log")).map_err(|error| error.to_string())?;
    let mut child = Command::new(executable)
        .arg(RENDER_CASE)
        .arg(case.index.to_string())
        .arg("--screenshot")
        .arg(path)
        .arg("--size")
        .arg(case.width.to_string())
        .arg(case.height.to_string())
        .arg("--scale")
        .arg(case.scale.to_string())
        .arg("--renderer")
        .arg("software")
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|error| format!("start subprocess: {error}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) if status.success() => break,
            Some(status) => return Err(format!("subprocess exited {status}")),
            None if started.elapsed() > Duration::from_secs(30) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("subprocess exceeded 30 seconds".into());
            }
            None => std::thread::sleep(Duration::from_millis(30)),
        }
    }
    validate_frame(path, case)
}

fn validate_frame(path: &Path, case: &RenderCase) -> Result<(), String> {
    let image = image::open(path)
        .map_err(|error| format!("decode PNG: {error}"))?
        .into_rgba8();
    let expected = (
        (case.width as f32 * case.scale).round() as u32,
        (case.height as f32 * case.scale).round() as u32,
    );
    if image.dimensions() != expected {
        return Err(format!(
            "frame dimensions {:?}, expected {expected:?}",
            image.dimensions()
        ));
    }
    let colors = image
        .pixels()
        .map(|pixel| pixel.0)
        .take(400_000)
        .collect::<HashSet<_>>();
    if colors.len() < 16 {
        return Err(format!(
            "frame has only {} colors; content appears blank",
            colors.len()
        ));
    }
    if case.page == Page::ProcessList && !matches!(case.extra, Some(36..=39)) {
        for expected in [[30, 160, 220], [80, 210, 160]] {
            if !image.pixels().any(|pixel| pixel.0[..3] == expected) {
                return Err("native process table did not render its fixture bitmap icons".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windui::{
        core::{NodeId, Tree},
        event::{Key, KeyEvent},
        geometry::Size,
        text::NullTextEngine,
    };
    #[test]
    fn render_matrix_covers_every_page_and_previous_fixture() {
        let cases = cases();
        assert_eq!(pages().len(), 34);
        assert_eq!(cases.len(), 128);
        for page in pages() {
            assert!(cases
                .iter()
                .any(|case| case.page == page && !case.chinese && !case.dark));
            assert!(cases
                .iter()
                .any(|case| case.page == page && case.chinese && case.dark));
        }
        for extra in 0..EXTRA_CASES {
            assert!(cases.iter().any(|case| case.extra == Some(extra)));
        }
        assert!(cases.iter().any(|case| case.scale == 1.5));
        assert!(cases.iter().any(|case| case.scale == 2.0));
        for (extra, size) in [(50, (1920, 1056)), (51, (900, 620)), (52, (1920, 1056))] {
            let case = cases.iter().find(|case| case.extra == Some(extra)).unwrap();
            assert_eq!(case.page, Page::AdaptiveEngine);
            assert_eq!((case.width, case.height), size);
            assert_eq!(case.scale, 1.0);
        }
        for (extra, page, size) in [
            (55, Page::ProcessList, (900, 620)),
            (56, Page::ProcessList, (1920, 1080)),
            (57, Page::BackgroundEfficiency, (900, 620)),
            (58, Page::BackgroundEfficiency, (1920, 1080)),
            (59, Page::BackgroundEfficiency, (900, 620)),
        ] {
            let case = cases.iter().find(|case| case.extra == Some(extra)).unwrap();
            assert_eq!(case.page, page);
            assert_eq!((case.width, case.height), size);
            assert_eq!(case.scale, 1.0);
        }
    }

    fn key(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        }
    }

    fn type_into_first_input(tree: &mut Tree, text: &str) -> NodeId {
        let input = tree
            .focusable_order()
            .into_iter()
            .find(|id| tree.get(*id).is_some_and(|node| node.widget.accepts_text()))
            .expect("preset modal exposes its native name input");
        tree.set_focused(Some(input), None);
        tree.dispatch_key(
            KeyEvent {
                ctrl: true,
                // Win32 sends shortcut virtual keys through Key::Other.
                ..key(Key::Other(0x41))
            },
            Some(input),
        );
        let mut characters = text.chars().peekable();
        while let Some(character) = characters.next() {
            tree.dispatch_key(key(Key::Char(character)), Some(input));
            // The last key intentionally has no intervening frame. Native Save
            // must flush that pending binding before applying the editor action.
            if characters.peek().is_some() {
                tree.layout_root(Size::new(1400, 20_000), &mut NullTextEngine);
            }
        }
        assert_eq!(
            tree.get(input)
                .and_then(|node| node.widget.ime_text())
                .as_deref(),
            Some(text)
        );
        assert!(
            tree.focusable_order().contains(&input),
            "typing replaced the native input node"
        );
        input
    }

    fn save_before_cancel(tree: &mut Tree) {
        let save = tree
            .focusable_order()
            .into_iter()
            .rev()
            .nth(1)
            .expect("Save and Cancel actions");
        assert!(
            tree.node_enabled(save),
            "native Save must remain usable after a draft edit"
        );
        let result = tree.dispatch_key(key(Key::Enter), Some(save));
        assert!(result.consumed, "native Save did not consume Enter");
    }

    fn save_after_cancel(tree: &mut Tree) {
        let save = *tree.focusable_order().last().expect("native Save action");
        assert!(
            tree.node_enabled(save),
            "native Save must remain usable after a draft edit"
        );
        let result = tree.dispatch_key(key(Key::Enter), Some(save));
        assert!(result.consumed, "native Save did not consume Enter");
    }

    #[test]
    fn native_cpu_preset_name_input_keeps_its_node_and_save_creates_draft_preset() {
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let mut settings = crate::config::Settings::default();
            settings.general.enabled = false;
            let mut editor = cpu_allocation::Editor::default();
            editor.update(
                &mut settings,
                cpu_allocation::Kind::Soft,
                cpu_allocation::Message::NewPreset,
            );
            let state = Rc::new(RefCell::new((editor, settings)));
            let received = state.clone();
            let ui = Ui::new(move |message, _refresh| {
                let mut state = received.borrow_mut();
                let (editor, settings) = &mut *state;
                editor.update(settings, cpu_allocation::Kind::Soft, message);
            });
            let content = {
                let state = state.borrow();
                state.0.modal(&state.1, ui).expect("new CPU preset modal")
            };
            let mut tree = Tree::new();
            tree.root = Some(content.width(1400).height(20_000).build(&mut tree));
            tree.layout_root(Size::new(1400, 20_000), &mut NullTextEngine);
            type_into_first_input(&mut tree, "Native CPU preset");
            save_before_cancel(&mut tree);
            let state = state.borrow();
            assert!(state
                .1
                .cpu_allocation_presets
                .iter()
                .any(|preset| preset.name == "Native CPU preset"));
            assert!(!state.1.general.enabled);
        });
    }

    #[test]
    fn native_adaptive_preset_name_input_keeps_its_node_and_save_creates_draft_preset() {
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let mut settings = crate::config::Settings::default();
            settings.general.enabled = false;
            let mut editor = adaptive_engine::Editor::default();
            editor.update(&mut settings, adaptive_engine::Message::New);
            let state = Rc::new(RefCell::new((editor, settings)));
            let received = state.clone();
            let ui = Ui::new(move |message, _refresh| {
                let mut state = received.borrow_mut();
                let (editor, settings) = &mut *state;
                editor.update(settings, message);
            });
            let content = {
                let state = state.borrow();
                state.0.preset_modal(&state.1, &[], ui)
            };
            let mut tree = Tree::new();
            tree.root = Some(content.width(1400).height(20_000).build(&mut tree));
            tree.layout_root(Size::new(1400, 20_000), &mut NullTextEngine);
            type_into_first_input(&mut tree, "Native Adaptive preset");
            save_before_cancel(&mut tree);
            let state = state.borrow();
            assert!(state
                .1
                .adaptive_engine_presets
                .iter()
                .any(|preset| preset.name == "Native Adaptive preset"));
            assert!(!state.1.general.enabled);
        });
    }

    #[test]
    fn native_time_rule_name_input_and_save_create_a_draft_rule() {
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let mut settings = crate::config::Settings::default();
            settings.general.enabled = false;
            let mut editor = power_rules::Editor::default();
            editor.update(
                power_rules::Kind::Time,
                &mut settings,
                &[],
                power_rules::Message::Add,
            );
            let state = Rc::new(RefCell::new((editor, settings)));
            let received = state.clone();
            let ui = Ui::new(move |message, _refresh| {
                let mut state = received.borrow_mut();
                let (editor, settings) = &mut *state;
                editor.update(power_rules::Kind::Time, settings, &[], message);
            });
            let content = state
                .borrow()
                .0
                .modal(power_rules::Kind::Time, &[], ui)
                .expect("new Time rule modal");
            let mut tree = Tree::new();
            tree.root = Some(content.width(1400).height(20_000).build(&mut tree));
            tree.layout_root(Size::new(1400, 20_000), &mut NullTextEngine);
            type_into_first_input(&mut tree, "Native Time rule");
            save_after_cancel(&mut tree);
            let state = state.borrow();
            assert!(state
                .1
                .by_time
                .rules
                .iter()
                .any(|rule| rule.name == "Native Time rule"));
            assert!(!state.1.general.enabled);
        });
    }

    #[test]
    fn native_limiter_invalid_then_valid_input_updates_save_and_flushes_last_digit() {
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let mut settings = crate::config::CpuLimiterSettings::default();
            let mut rule =
                crate::ui::process_rules::new_cpu_limiter_rule(r"C:\Apps\Example\example.exe");
            rule.focus_mode = crate::config::ProcessRuleMode::Enabled;
            settings.rules.push(rule);
            let mut editor = cpu_limiter::CpuLimiter::default();
            editor.update(&mut settings, cpu_limiter::Message::Edit(0));
            let state = Rc::new(RefCell::new((editor, settings)));
            let received = state.clone();
            let ui = Ui::new(move |message, _refresh| {
                let mut state = received.borrow_mut();
                let (editor, settings) = &mut *state;
                editor.update(settings, message);
            });
            let content = state
                .borrow()
                .0
                .modal(&[], &Default::default(), ui)
                .expect("limiter rule modal");
            let mut tree = Tree::new();
            tree.root = Some(content.width(1400).height(20_000).build(&mut tree));
            tree.layout_root(Size::new(1400, 20_000), &mut NullTextEngine);
            let save = *tree.focusable_order().last().expect("limiter Save action");
            type_into_first_input(&mut tree, "0");
            tree.layout_root(Size::new(1400, 20_000), &mut NullTextEngine);
            assert!(
                !tree.node_enabled(save),
                "invalid 0% limit must disable Save"
            );
            tree.dispatch_key(key(Key::Enter), Some(save));
            assert!(state.borrow().0.has_pending_editor());
            assert_eq!(state.borrow().1.rules[0].focus_allowed_cpu_time_percent, 50);
            // There is no earlier valid frame to refresh aggregate editor state.
            type_into_first_input(&mut tree, "3");
            assert!(
                tree.node_enabled(save),
                "correcting the limit must enable Save"
            );
            tree.dispatch_key(key(Key::Enter), Some(save));
            let state = state.borrow();
            assert_eq!(state.1.rules[0].focus_allowed_cpu_time_percent, 3);
            assert!(!state.0.has_pending_editor());
            assert!(!state.1.enabled);
        });
    }
}
