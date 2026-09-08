//! Opt-in native render check; compiled only with the render-smoke feature.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

static REQUESTED: AtomicBool = AtomicBool::new(false);
pub(super) struct Run {
    pages: Vec<Page>,
    index: usize,
    ticks: u8,
}
impl Run {
    pub(super) fn requested() -> Option<Self> {
        REQUESTED.load(Ordering::Relaxed).then(|| {
            let mut pages = Vec::new();
            for section in Page::sections() {
                if !pages.contains(&section.landing_page) {
                    pages.push(section.landing_page);
                }
                for page in section.pages {
                    if !pages.contains(page) {
                        pages.push(*page);
                    }
                }
            }
            Self {
                pages,
                index: 0,
                ticks: 0,
            }
        })
    }
}
pub(super) fn advance(app: &mut WinderustApp) -> Option<Task<Message>> {
    let window = app.window?;
    let run = app.smoke.as_mut()?;
    if run.index == run.pages.len() * 2 {
        return Some(app.shutdown());
    }
    if run.ticks == 0 {
        let chinese = run.index >= run.pages.len();
        let page = run.pages[run.index % run.pages.len()];
        run.ticks = 1;
        app.settings.general.language = if chinese {
            crate::config::AppLanguage::ZhTw
        } else {
            crate::config::AppLanguage::English
        };
        app.settings.general.theme_mode = if chinese {
            crate::config::AppThemeMode::Dark
        } else {
            crate::config::AppThemeMode::Light
        };
        rust_i18n::set_locale(app.settings.general.language.locale());
        app.appearance = settings_pages::theme(&app.settings.general);
        return Some(Task::batch([
            iced::window::resize(
                window,
                iced::Size::new(
                    if chinese { 900.0 } else { 1120.0 },
                    if chinese { 620.0 } else { 760.0 },
                ),
            ),
            app.update(Message::Page(page)),
        ]));
    }
    run.ticks += 1;
    if run.ticks == 4 {
        Some(iced::window::screenshot(window).map(Message::SmokeScreenshot))
    } else {
        Some(Task::none())
    }
}
pub(super) fn captured(app: &mut WinderustApp, shot: iced::window::Screenshot) -> Task<Message> {
    let run = app
        .smoke
        .as_mut()
        .expect("smoke screenshot belongs to a smoke run");
    assert!(shot.size.width >= 900 && shot.size.height >= 620);
    let path = format!("target/iced-smoke/{:02}-{:?}.png", run.index, app.page);
    image::save_buffer(
        path,
        &shot.rgba,
        shot.size.width,
        shot.size.height,
        image::ColorType::Rgba8,
    )
    .expect("save rendered smoke evidence");
    run.index += 1;
    run.ticks = 0;
    Task::none()
}

pub(crate) fn render_all_pages() {
    std::fs::create_dir_all("target/iced-smoke").expect("create smoke output");
    let mut settings = crate::config::Settings::default();
    settings.general.enabled = false;
    settings.general.check_for_updates = false;
    settings.general.hide_to_tray = false;
    settings.general.start_minimized = false;
    settings.general.startup_with_windows = false;
    settings.advanced.show_advanced_controls = true;
    settings.general.animation_mode = crate::config::AnimationMode::Off;
    let mut editor = SettingsEditor::with_settings(settings);
    let runtime = RuntimeHandle::start(&editor.runtime_settings_snapshot());
    REQUESTED.store(true, Ordering::Relaxed);
    let result = run(editor, None, runtime, None);
    REQUESTED.store(false, Ordering::Relaxed);
    result.expect("all page windows rendered");
    assert_eq!(
        std::fs::read_dir("target/iced-smoke")
            .expect("read smoke output")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
            .count(),
        68
    );
}
