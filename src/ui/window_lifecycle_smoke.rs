//! Opt-in window teardown/reopen check with an isolated, disabled automation fixture.
use super::*;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Gdi::{GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONULL},
    System::{
        ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX2},
        Threading::GetCurrentProcess,
    },
    UI::WindowsAndMessaging::{GetWindowRect, IsWindow, IsZoomed},
};

static REQUESTED: AtomicBool = AtomicBool::new(false);
static COMPLETED: AtomicBool = AtomicBool::new(false);
const SETTLE: Duration = Duration::from_secs(3);
const OFFSCREEN: iced::Point = iced::Point::new(1_000_000.0, 1_000_000.0);

pub(super) struct Run {
    started: Instant,
    next: Instant,
    stage: u8,
    cycle: u8,
    retain_window: bool,
    start_minimized: bool,
    runtime: Option<usize>,
    tray: Option<usize>,
    previous_window: Option<iced::window::Id>,
    previous_hwnd: Option<usize>,
    normal_size: Option<iced::Size>,
    normal_position: Option<iced::Point>,
    normal_rect: Option<(i32, i32, i32, i32)>,
    draft_theme: Option<crate::config::AppThemeMode>,
    rows: Vec<String>,
}

impl Run {
    pub(super) fn requested() -> Option<Self> {
        REQUESTED.load(Ordering::Relaxed).then(|| {
            let started = Instant::now();
            Self {
                started,
                next: started + SETTLE,
                stage: 0,
                cycle: 0,
                retain_window: std::env::args().any(|arg| arg == "--retain-window"),
                start_minimized: std::env::args().any(|arg| arg == "--start-minimized"),
                runtime: None,
                tray: None,
                previous_window: None,
                previous_hwnd: None,
                normal_size: None,
                normal_position: None,
                normal_rect: None,
                draft_theme: None,
                rows: Vec::new(),
            }
        })
    }

    fn check_identity(&self, app: &WinderustApp) {
        assert_eq!(self.runtime, Some(Arc::as_ptr(&app.runtime) as usize));
        assert_eq!(
            self.tray,
            app.tray
                .as_ref()
                .map(|icon| std::ptr::from_ref(icon) as usize)
        );
        assert!(app.tray.as_ref().is_some_and(|icon| icon.is_registered()));
        if let Some(theme) = self.draft_theme {
            assert_eq!(app.page, Page::SettingsHome);
            assert_eq!(app.settings.global().general.theme_mode, theme);
            assert!(app.settings.has_unsaved_changes());
        }
    }

    fn record(&mut self, phase: &str) {
        let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32;
        let mut counters = PROCESS_MEMORY_COUNTERS_EX2 {
            cb: size,
            ..Default::default()
        };
        // SAFETY: the pseudo-handle denotes this live process; counters is writable for size bytes.
        let ok = unsafe {
            K32GetProcessMemoryInfo(
                GetCurrentProcess(),
                std::ptr::from_mut(&mut counters).cast(),
                size,
            )
        };
        assert_ne!(
            ok,
            0,
            "memory sample failed: {}",
            std::io::Error::last_os_error()
        );
        self.rows.push(format!(
            "{},{phase},{},{},{},{}",
            self.started.elapsed().as_millis(),
            self.cycle,
            counters.PrivateWorkingSetSize,
            counters.PrivateUsage,
            counters.WorkingSetSize
        ));
        let mode = if self.retain_window {
            "retained"
        } else {
            "destroyed"
        };
        let startup = if self.start_minimized {
            "minimized"
        } else {
            "visible"
        };
        let path = format!("target/window-lifecycle-smoke/{mode}-{startup}.csv");
        std::fs::write(
            path,
            format!(
                "elapsed_ms,phase,cycle,private_working_set_bytes,private_commit_bytes,working_set_bytes\n{}\n",
                self.rows.join("\n")
            ),
        )
        .expect("write lifecycle smoke measurements");
    }

    fn check_normal_settings(&self, app: &WinderustApp) {
        assert_eq!(Some(app.window_settings.size), self.normal_size);
        let position = match app.window_settings.position {
            iced::window::Position::Specific(position) => Some(position),
            _ => None,
        };
        assert_eq!(position, self.normal_position);
    }

    fn hide(&mut self, app: &mut WinderustApp) -> Task<Message> {
        self.previous_window = app.window;
        self.previous_hwnd = app.hwnd;
        self.cycle += 1;
        self.stage = 3;
        self.next = Instant::now() + SETTLE;
        if self.retain_window {
            tray::set_hidden_to_tray(true);
            app.hidden = true;
            self.previous_window
                .map(|window| iced::window::set_mode(window, iced::window::Mode::Hidden))
                .unwrap_or_else(Task::none)
        } else {
            // Exercise teardown independently of foreground focus during an automated run.
            app.close_window()
        }
    }

    fn advance(&mut self, app: &mut WinderustApp) -> Option<Task<Message>> {
        assert!(
            self.started.elapsed() < Duration::from_secs(60),
            "lifecycle smoke timed out"
        );
        if Instant::now() < self.next {
            return None;
        }
        self.next = Instant::now() + SETTLE;
        match self.stage {
            0 => {
                self.runtime = Some(Arc::as_ptr(&app.runtime) as usize);
                self.tray = app
                    .tray
                    .as_ref()
                    .map(|icon| std::ptr::from_ref(icon) as usize);
                self.check_identity(app);
                self.stage = 1;
                if self.start_minimized {
                    assert!(app.hidden);
                    assert!(app.window.is_none());
                    assert!(app.hwnd.is_none());
                    self.record("startup_hidden");
                    tray::request_restore();
                    return None;
                }
                self.next = Instant::now();
                self.advance(app)
            }
            1 => {
                assert!(!app.hidden);
                assert!(app.window.is_some());
                assert!(app.hwnd.is_some_and(is_window));
                self.check_identity(app);
                let theme = if app.settings.global().general.theme_mode
                    == crate::config::AppThemeMode::Dark
                {
                    crate::config::AppThemeMode::Light
                } else {
                    crate::config::AppThemeMode::Dark
                };
                app.settings
                    .edit_global(|settings| settings.general.theme_mode = theme);
                app.appearance = settings_pages::theme(&app.settings.global().general);
                self.draft_theme = Some(theme);
                self.stage = 2;
                let window = current_window(app);
                Some(Task::batch([
                    app.update(Message::Page(Page::SettingsHome)),
                    iced::window::resize(window, iced::Size::new(1000.0, 680.0)),
                    iced::window::move_to(window, iced::Point::new(80.0, 80.0)),
                ]))
            }
            2 => {
                self.check_identity(app);
                assert_eq!(app.window_settings.size, iced::Size::new(1000.0, 680.0));
                self.normal_size = Some(app.window_settings.size);
                self.normal_position = match app.window_settings.position {
                    iced::window::Position::Specific(position) => Some(position),
                    _ => panic!("normal window position was not observed"),
                };
                assert_eq!(self.normal_position, Some(iced::Point::new(80.0, 80.0)));
                self.normal_rect = app.hwnd.map(window_rect);
                self.record("initial_visible");
                Some(self.hide(app))
            }
            3 => {
                self.check_identity(app);
                assert!(app.hidden);
                if self.retain_window {
                    assert_eq!(app.window, self.previous_window);
                    assert!(app.hwnd.is_some_and(is_window));
                } else {
                    assert!(app.window.is_none());
                    assert!(app.hwnd.is_none());
                    assert!(!self.previous_hwnd.is_some_and(is_window));
                }
                self.record("hidden");
                self.stage = 4;
                tray::request_restore();
                tray::request_restore();
                let first = app.show_window();
                let pending = app.window;
                let second = app.show_window();
                assert_eq!(app.window, pending, "duplicate restore allocated a window");
                Some(Task::batch([first, second]))
            }
            4 => {
                self.check_identity(app);
                assert!(!app.hidden);
                assert!(app.hwnd.is_some_and(is_window));
                if self.retain_window {
                    assert_eq!(app.window, self.previous_window);
                } else {
                    assert!(app.window.is_some());
                    assert_ne!(app.window, self.previous_window);
                    if let Some(stale) = self.previous_window {
                        let current = (app.window, app.hwnd);
                        let _ = app.update(Message::Window(stale));
                        let _ = app.update(Message::NativeWindow(stale, None));
                        let _ = app.update(Message::WindowMoved(stale, iced::Point::ORIGIN));
                        let _ =
                            app.update(Message::WindowResized(stale, iced::Size::new(1.0, 1.0)));
                        assert_eq!((app.window, app.hwnd), current);
                    }
                }
                self.check_normal_settings(app);
                self.record("reopened");
                let window = current_window(app);
                let next = match self.cycle {
                    1 => {
                        self.stage = 8;
                        iced::window::maximize(window, true)
                    }
                    2 => {
                        assert!(app.maximized);
                        assert!(app.hwnd.is_some_and(is_maximized));
                        self.stage = 9;
                        iced::window::maximize(window, false)
                    }
                    3 if !self.retain_window => {
                        self.previous_window = app.window;
                        self.previous_hwnd = app.hwnd;
                        self.cycle = 4;
                        self.stage = 10;
                        let close = app.close_window();
                        app.window_settings.position = iced::window::Position::Specific(OFFSCREEN);
                        app.maximized = true;
                        close
                    }
                    _ => {
                        self.stage = 5;
                        app.update(Message::Close)
                    }
                };
                Some(single_window(window).chain(next))
            }
            5 => {
                self.check_identity(app);
                assert!(app.closing);
                assert!(!app.exiting);
                assert!(app.hwnd.is_some_and(is_window));
                self.stage = 6;
                Some(app.update(Message::Stay))
            }
            6 => {
                self.check_identity(app);
                assert!(!app.closing);
                assert!(!app.hidden);
                self.record("exit_cancelled");
                self.stage = 7;
                COMPLETED.store(true, Ordering::Relaxed);
                Some(app.update(Message::DiscardAndClose))
            }
            8 => {
                self.check_identity(app);
                self.check_normal_settings(app);
                assert!(app.maximized);
                assert!(app.hwnd.is_some_and(is_maximized));
                self.record("maximized_visible");
                Some(self.hide(app))
            }
            9 => {
                self.check_identity(app);
                self.check_normal_settings(app);
                assert!(!app.maximized);
                assert!(!app.hwnd.is_some_and(is_maximized));
                assert_eq!(app.hwnd.map(window_rect), self.normal_rect);
                self.record("normal_placement_restored");
                Some(self.hide(app))
            }
            10 => {
                self.check_identity(app);
                assert!(app.hidden);
                assert!(app.window.is_none());
                assert!(app.hwnd.is_none());
                assert!(!self.previous_hwnd.is_some_and(is_window));
                self.record("offscreen_hidden");
                self.stage = 11;
                tray::request_restore();
                None
            }
            11 => {
                self.check_identity(app);
                assert!(!app.hidden);
                assert!(app.hwnd.is_some_and(is_window));
                assert_ne!(app.window, self.previous_window);
                assert!(app.maximized);
                assert!(app.hwnd.is_some_and(is_maximized));
                assert!(app.hwnd.is_some_and(intersects_work_area));
                self.record("offscreen_maximized_recovered");
                self.stage = 12;
                Some(
                    single_window(current_window(app))
                        .chain(iced::window::maximize(current_window(app), false)),
                )
            }
            12 => {
                self.check_identity(app);
                assert!(!app.hidden);
                assert!(app.hwnd.is_some_and(is_window));
                assert!(!app.maximized);
                assert!(!app.hwnd.is_some_and(is_maximized));
                assert_eq!(Some(app.window_settings.size), self.normal_size);
                match app.window_settings.position {
                    iced::window::Position::Specific(position) => {
                        assert_ne!(position, OFFSCREEN, "recovered position was not remembered");
                    }
                    _ => panic!("recovered window position was not observed"),
                }
                assert!(app.hwnd.is_some_and(intersects_work_area));
                self.record("offscreen_recovered");
                self.stage = 5;
                Some(single_window(current_window(app)).chain(app.update(Message::Close)))
            }
            _ => None,
        }
    }
}

fn is_window(hwnd: usize) -> bool {
    // SAFETY: IsWindow accepts an arbitrary handle value without dereferencing it in this process.
    unsafe { IsWindow(hwnd as HWND) != 0 }
}

fn is_maximized(hwnd: usize) -> bool {
    // SAFETY: IsZoomed accepts the currently observed native window handle and retains nothing.
    unsafe { IsZoomed(hwnd as HWND) != 0 }
}

fn window_rect(hwnd: usize) -> (i32, i32, i32, i32) {
    let mut rect = RECT::default();
    // SAFETY: the fixture supplies its live window and rect is writable for the call.
    let ok = unsafe { GetWindowRect(hwnd as HWND, &mut rect) };
    assert_ne!(ok, 0, "failed to read lifecycle window geometry");
    (rect.left, rect.top, rect.right, rect.bottom)
}

fn intersects_work_area(hwnd: usize) -> bool {
    let (left, top, right, bottom) = window_rect(hwnd);
    let rect = RECT {
        left,
        top,
        right,
        bottom,
    };
    // SAFETY: rect is readable and MonitorFromRect only queries connected display geometry.
    let monitor = unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONULL) };
    if monitor.is_null() {
        return false;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: monitor is a live monitor handle and info is writable for its declared size.
    let ok = unsafe { GetMonitorInfoW(monitor, &mut info) };
    assert_ne!(ok, 0, "failed to read lifecycle monitor work area");
    left < info.rcWork.right
        && right > info.rcWork.left
        && top < info.rcWork.bottom
        && bottom > info.rcWork.top
}

fn current_window(app: &WinderustApp) -> iced::window::Id {
    match app.window {
        Some(window) => window,
        None => panic!("lifecycle fixture has no current window"),
    }
}

fn single_window(expected: iced::window::Id) -> Task<Message> {
    Task::batch([
        iced::window::oldest().then(move |window| {
            assert_eq!(window, Some(expected), "unexpected extra Iced window");
            Task::none()
        }),
        iced::window::latest().then(move |window| {
            assert_eq!(window, Some(expected), "unexpected extra Iced window");
            Task::none()
        }),
    ])
}

pub(super) fn advance(app: &mut WinderustApp) -> Option<Task<Message>> {
    let mut check = app.lifecycle_smoke.take()?;
    let task = check.advance(app);
    app.lifecycle_smoke = Some(check);
    task
}

pub(crate) fn run_check() {
    std::fs::create_dir_all("target/window-lifecycle-smoke")
        .expect("create lifecycle smoke output");
    let mut settings = crate::config::Settings::default();
    settings.general.enabled = false;
    settings.general.check_for_updates = false;
    settings.general.hide_to_tray = true;
    settings.general.start_minimized = std::env::args().any(|arg| arg == "--start-minimized");
    settings.general.startup_with_windows = false;
    let mut editor = SettingsEditor::with_settings(settings);
    let recovery = Arc::new(Mutex::new(crate::crash_recovery::RecoveryClient::start()));
    let runtime = RuntimeHandle::start(&editor.runtime_settings_snapshot());
    COMPLETED.store(false, Ordering::Relaxed);
    REQUESTED.store(true, Ordering::Relaxed);
    let result = run(editor, None, runtime, None, recovery);
    REQUESTED.store(false, Ordering::Relaxed);
    result.expect("run lifecycle smoke fixture");
    assert!(
        COMPLETED.load(Ordering::Relaxed),
        "lifecycle smoke exited before all checks completed"
    );
}
