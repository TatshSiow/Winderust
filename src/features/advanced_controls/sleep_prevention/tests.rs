use super::*;
use crate::config::SleepPreventionRule;

fn process(id: u32, path: &str) -> ProcessInfo {
    ProcessInfo {
        id,
        creation_time: Some(u64::from(id)),
        parent_id: None,
        session_id: Some(1),
        user_name: None,
        is_service_account: Some(false),
        is_critical: Some(false),
        can_set_information: true,
        name: Path::new(path)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .into(),
        image_path: Some(path.into()),
    }
}

fn settings() -> SleepPreventionSettings {
    SleepPreventionSettings {
        enabled: true,
        rules: vec![
            SleepPreventionRule {
                enabled: true,
                executable_path: r"C:\Apps\worker.exe".into(),
                keep_display_on: false,
            },
            SleepPreventionRule {
                enabled: true,
                executable_path: r"C:\Apps\video.exe".into(),
                keep_display_on: true,
            },
        ],
    }
}

fn observe(
    manager: &mut SleepPreventionManager,
    settings: &SleepPreventionSettings,
    apps: &[ProcessInfo],
) {
    manager.observe(settings, apps, u32::MAX, Some(1), false);
}

#[test]
fn overlapping_rules_combine_requests_until_the_last_matching_app_exits() {
    let settings = settings();
    let mut manager = SleepPreventionManager::default();
    let worker = process(7, r"C:\Apps\worker.exe");
    let video = process(8, r"c:\apps\VIDEO.exe");
    observe(&mut manager, &settings, &[worker.clone(), video.clone()]);
    assert_eq!(manager.matches.rules, 2);
    assert_eq!(manager.matches.apps.len(), 2);
    assert_eq!(
        manager.desired_mode(),
        SleepPreventionMode::SystemAndDisplay
    );
    observe(&mut manager, &settings, &[worker]);
    assert_eq!(manager.desired_mode(), SleepPreventionMode::System);
    observe(&mut manager, &settings, &[video]);
    assert_eq!(
        manager.desired_mode(),
        SleepPreventionMode::SystemAndDisplay
    );
    observe(&mut manager, &settings, &[]);
    assert_eq!(manager.desired_mode(), SleepPreventionMode::Off);
}

#[test]
fn matching_accepts_a_unicode_basename_candidate_with_the_exact_image_path() {
    let mut settings = settings();
    let path = r"C:\Apps\Éditeur.exe";
    settings.rules[0].executable_path = path.into();
    let mut worker = process(7, path);
    worker.name = "éditeur.exe".into();

    let matches = match_rules(&settings, &[worker], u32::MAX, Some(1), false);
    assert!(matches.known);
    assert!(matches.apps.contains_key(&(7, 7)));
}

#[test]
fn literal_filename_whitespace_keeps_sleep_requests_and_rearms_separate() {
    let worker = r"C:\Apps\worker.exe";
    let other = "C:\\Apps\\worker.exe\u{3000}";
    let mut configured = Settings {
        sleep_prevention: settings(),
        ..Settings::default()
    };
    configured.sleep_prevention.rules[1].executable_path = format!(" {other}");
    let apps = [process(7, worker), process(8, other)];
    let mut manager = SleepPreventionManager::default();
    manager.note_configuration(&configured, Some(true));
    observe(&mut manager, &configured.sleep_prevention, &apps[..1]);
    assert_eq!(manager.desired_mode(), SleepPreventionMode::System);
    observe(&mut manager, &configured.sleep_prevention, &apps);
    assert_eq!(manager.matches.apps.len(), 2);
    manager.suppress_current();

    configured.sleep_prevention.rules[0].enabled = false;
    manager.note_configuration(&configured, Some(true));
    configured.sleep_prevention.rules[0].enabled = true;
    manager.note_configuration(&configured, Some(true));
    observe(&mut manager, &configured.sleep_prevention, &apps);
    assert_eq!(manager.desired_mode(), SleepPreventionMode::System);
    assert!(manager.suppressed.contains_key(&(8, 8)));
}

#[test]
fn matching_rejects_wrong_paths_protected_apps_and_disallowed_sessions() {
    let settings = settings();
    let mut worker = process(7, r"C:\Apps\worker.exe");
    let baseline = worker.clone();
    assert!(match_rules(&settings, &[], u32::MAX, None, false).known);
    for modified in 0..7 {
        worker = baseline.clone();
        match modified {
            0 => worker.image_path = Some(r"C:\Other\worker.exe".into()),
            1 => worker.is_critical = Some(true),
            2 => worker.is_service_account = Some(true),
            3 => worker.session_id = Some(0),
            4 => worker.session_id = Some(2),
            5 => worker.can_set_information = false,
            _ => worker.id = u32::MAX,
        }
        assert!(match_rules(&settings, &[worker], u32::MAX, Some(1), false)
            .apps
            .is_empty());
    }
    let mut other_session = baseline.clone();
    other_session.session_id = Some(2);
    assert_eq!(
        match_rules(&settings, &[other_session], u32::MAX, Some(1), true)
            .apps
            .len(),
        1
    );
    for modified in 0..4 {
        worker = baseline.clone();
        match modified {
            0 => worker.image_path = None,
            1 => worker.creation_time = None,
            2 => worker.is_service_account = None,
            _ => worker.session_id = None,
        }
        let matches = match_rules(&settings, &[worker], u32::MAX, Some(1), false);
        assert!(matches.apps.is_empty());
        assert!(!matches.known);
    }
}

#[test]
fn requesting_instances_share_their_path_and_take_precedence_over_suppression() {
    let settings = settings();
    let path = settings.rules[1].executable_path.as_str();
    let mut manager = SleepPreventionManager::default();
    let original = process(8, path);
    observe(&mut manager, &settings, std::slice::from_ref(&original));
    manager.suppress_current();
    observe(&mut manager, &settings, std::slice::from_ref(&original));

    let replacement = process(9, path);
    let duplicate = process(10, path);
    observe(&mut manager, &settings, &[original, replacement, duplicate]);
    let snapshot = manager.matching_snapshot();
    assert_eq!(manager.suppressed.len(), 1);
    assert_eq!(snapshot.matched_processes, 3);
    assert!(snapshot.suppressed_apps.is_empty());
    assert_eq!(
        snapshot.requesting_apps,
        [RequestingApp {
            path: path.into(),
            keep_display_on: true,
        }]
    );
}

#[test]
fn path_observation_keeps_prior_apps_and_suppression_when_creation_is_unknown() {
    let settings = settings();
    let mut manager = SleepPreventionManager::default();
    let mut worker = process(7, r"C:\Apps\worker.exe");
    observe(&mut manager, &settings, std::slice::from_ref(&worker));

    worker.name = "unrelated.exe".into();
    worker.image_path = None;
    assert!(manager.needs_path_observation(&worker, &settings));
    worker.creation_time = None;
    assert!(manager.needs_path_observation(&worker, &settings));

    manager.suppress_current();
    manager.matches = Matches::default();
    assert!(manager.needs_path_observation(&worker, &settings));
    worker.creation_time = Some(99);
    assert!(!manager.needs_path_observation(&worker, &settings));
}

#[test]
fn sleep_suppression_survives_unknown_observations_and_pid_reuse_cannot_inherit_it() {
    let settings = settings();
    let mut manager = SleepPreventionManager::default();
    let mut worker = process(7, r"C:\Apps\worker.exe");
    observe(&mut manager, &settings, std::slice::from_ref(&worker));
    manager.suppress_current();
    observe(&mut manager, &settings, std::slice::from_ref(&worker));
    assert_eq!(manager.desired_mode(), SleepPreventionMode::Off);
    worker.creation_time = None;
    worker.image_path = None;
    observe(&mut manager, &settings, &[worker]);
    assert_eq!(manager.suppressed.len(), 1);
    worker = process(7, r"C:\Apps\worker.exe");
    observe(&mut manager, &settings, std::slice::from_ref(&worker));
    assert_eq!(manager.desired_mode(), SleepPreventionMode::Off);
    worker.creation_time = Some(99);
    observe(&mut manager, &settings, &[worker]);
    assert!(manager.suppressed.is_empty());
    assert_eq!(manager.desired_mode(), SleepPreventionMode::System);
}

#[test]
fn unknown_observations_retain_counts_without_adding_requests_and_rule_withdrawal_still_applies() {
    let mut settings = settings();
    let mut manager = SleepPreventionManager::default();
    let mut video = process(8, r"C:\Apps\video.exe");
    observe(&mut manager, &settings, std::slice::from_ref(&video));
    let both = SleepPreventionRequests {
        system: true,
        display: true,
    };
    manager.observe(
        &settings,
        std::slice::from_ref(&video),
        u32::MAX,
        None,
        false,
    );
    assert!(!manager.matches.known);
    assert_eq!(
        manager.permitted_mode(both),
        SleepPreventionMode::SystemAndDisplay
    );

    video.image_path = None;
    video.creation_time = None;
    observe(&mut manager, &settings, &[video]);
    assert!(!manager.matches.known);
    let system = SleepPreventionRequests {
        system: true,
        display: false,
    };
    assert_eq!(
        manager.permitted_mode(both),
        SleepPreventionMode::SystemAndDisplay
    );
    assert_eq!(manager.permitted_mode(system), SleepPreventionMode::System);
    assert_eq!(
        manager.permitted_mode(SleepPreventionRequests::default()),
        SleepPreventionMode::Off
    );

    settings.rules[1].keep_display_on = false;
    manager.retain_enabled_claims(&settings, false);
    assert_eq!(manager.permitted_mode(both), SleepPreventionMode::System);
    settings.rules[1].enabled = false;
    manager.retain_enabled_claims(&settings, false);
    assert_eq!(manager.permitted_mode(both), SleepPreventionMode::Off);
    assert!(manager.matches.apps.is_empty());
}

#[test]
fn cross_session_opt_out_releases_old_claims_even_when_observations_are_unavailable() {
    let settings = settings();
    let mut manager = SleepPreventionManager::default();
    let mut worker = process(7, r"C:\Apps\worker.exe");
    worker.session_id = Some(2);
    manager.observe(&settings, &[worker], u32::MAX, Some(1), true);
    assert_eq!(manager.desired_mode(), SleepPreventionMode::System);
    manager.matches.known = false;
    manager.retain_enabled_claims(&settings, false);
    assert_eq!(
        manager.permitted_mode(SleepPreventionRequests {
            system: true,
            display: false
        }),
        SleepPreventionMode::Off
    );
}

#[test]
fn inactive_profiles_and_missing_rules_preserve_sleep_suppression_until_explicit_rearm() {
    let settings = settings();
    let inactive = SleepPreventionSettings::default();
    let mut manager = SleepPreventionManager::default();
    let mut configured = Settings {
        sleep_prevention: settings.clone(),
        ..Default::default()
    };
    configured.battery_profile_mut().sleep_prevention = inactive.clone();
    manager.note_configuration(&configured, Some(true));
    manager.change_power_source(Some(true));
    let worker = process(7, r"C:\Apps\worker.exe");
    observe(&mut manager, &settings, std::slice::from_ref(&worker));
    manager.suppress_current();
    observe(&mut manager, &settings, std::slice::from_ref(&worker));
    let mut controller = SleepPreventionController::default();
    let mut log = ActionLog::new(4);
    manager.update_profile(
        &mut controller,
        configured.battery_profile(),
        true,
        &mut CycleObservations::default(),
        &mut log,
    );
    assert_eq!(manager.suppressed.len(), 1);
    observe(&mut manager, &inactive, std::slice::from_ref(&worker));
    assert_eq!(manager.suppressed.len(), 1);
    observe(&mut manager, &settings, std::slice::from_ref(&worker));
    assert_eq!(manager.desired_mode(), SleepPreventionMode::Off);
    configured.battery_profile_mut().sleep_prevention = settings.clone();
    manager.note_configuration(&configured, None);
    assert_eq!(manager.desired_mode(), SleepPreventionMode::Off);
    configured.sleep_prevention = inactive;
    manager.note_configuration(&configured, None);
    configured.sleep_prevention = settings.clone();
    manager.note_configuration(&configured, None);
    observe(&mut manager, &settings, &[worker]);
    assert_eq!(manager.desired_mode(), SleepPreventionMode::System);
}

#[test]
fn missing_sleep_events_before_any_accepted_request_does_not_suppress_apps() {
    let settings = settings();
    let mut manager = SleepPreventionManager::default();
    let worker = process(7, r"C:\Apps\worker.exe");
    observe(&mut manager, &settings, std::slice::from_ref(&worker));
    let mut controller = SleepPreventionController::default();
    let mut log = ActionLog::new(4);
    let snapshot = manager.unavailable(&mut controller, &mut log, "watcher unavailable");
    assert!(!snapshot.observation_known);
    assert!(!snapshot.system_requested);
    assert!(manager.suppressed.is_empty());
    observe(&mut manager, &settings, &[worker]);
    assert_eq!(manager.desired_mode(), SleepPreventionMode::System);
}

#[test]
fn suspend_before_first_observation_suppresses_running_apps_and_disabling_rearms() {
    let mut settings = settings();
    let mut manager = SleepPreventionManager::default();
    let worker = process(7, r"C:\Apps\worker.exe");
    manager.suppress_current();
    observe(&mut manager, &settings, std::slice::from_ref(&worker));
    assert_eq!(manager.desired_mode(), SleepPreventionMode::Off);
    let mut controller = SleepPreventionController::default();
    let mut log = ActionLog::new(4);
    manager.reset(&mut controller, &mut log);
    observe(&mut manager, &settings, std::slice::from_ref(&worker));
    assert_eq!(manager.desired_mode(), SleepPreventionMode::Off);
    let previous = settings.clone();
    settings.enabled = false;
    manager.rearm_withdrawn(&(true, previous), &(true, settings.clone()), None);
    let snapshot = manager.update_profile(
        &mut controller,
        &Settings {
            sleep_prevention: settings.clone(),
            ..Default::default()
        },
        true,
        &mut CycleObservations::default(),
        &mut log,
    );
    assert!(!snapshot.enabled);
    assert!(manager.suppressed.is_empty());
    settings.enabled = true;
    observe(&mut manager, &settings, &[worker]);
    assert_eq!(manager.desired_mode(), SleepPreventionMode::System);
}

#[test]
fn unavailable_power_samples_do_not_start_new_profile_transitions() {
    let mut manager = SleepPreventionManager::default();
    assert!(!manager.change_power_source(None));
    assert_eq!(manager.last_known_power_source(), None);
    assert!(!manager.change_power_source(Some(true)));
    assert!(!manager.change_power_source(None));
    assert_eq!(manager.last_known_power_source(), Some(true));
    assert!(!manager.change_power_source(Some(true)));
    assert!(manager.change_power_source(Some(false)));
    assert!(!manager.change_power_source(None));
    assert!(!manager.change_power_source(Some(false)));
    assert_eq!(manager.last_known_power_source(), Some(false));
}

#[test]
fn shared_power_source_policy_requires_the_same_native_rule_paths_and_request_types() {
    let mut ac = Settings {
        sleep_prevention: settings(),
        ..Default::default()
    };
    ac.sleep_prevention.rules[0].executable_path = r"\\?\C:\Apps\worker.exe".into();
    let mut battery = ac.clone();
    battery.sleep_prevention.rules.push(SleepPreventionRule {
        enabled: false,
        executable_path: r"C:\Apps\ignored.exe".into(),
        keep_display_on: true,
    });
    assert!(same_sleep_prevention_policy(&ac, &battery));

    battery.sleep_prevention.rules[0].executable_path = r"\\?\C:\Apps/worker.exe".into();
    assert!(!same_sleep_prevention_policy(&ac, &battery));
    battery.sleep_prevention.rules[0] = ac.sleep_prevention.rules[0].clone();
    battery.sleep_prevention.rules[0].keep_display_on = true;
    assert!(!same_sleep_prevention_policy(&ac, &battery));
}

#[test]
fn simultaneous_profile_switch_and_explicit_disable_rearms_only_withdrawn_paths() {
    let rules = settings().rules;
    let shared = SleepPreventionRule {
        executable_path: r"C:\Apps\shared.exe".into(),
        ..rules[0].clone()
    };
    let mut configured = Settings {
        sleep_prevention: SleepPreventionSettings {
            enabled: true,
            rules: vec![rules[0].clone(), shared.clone()],
        },
        ..Settings::default()
    };
    configured.battery_profile_mut().sleep_prevention = SleepPreventionSettings {
        enabled: true,
        rules: vec![rules[1].clone(), shared],
    };
    let mut manager = SleepPreventionManager::default();
    manager.note_configuration(&configured, Some(true));
    manager.change_power_source(Some(true));
    let apps = [
        process(7, r"C:\Apps\worker.exe"),
        process(8, r"C:\Apps\video.exe"),
        process(9, r"C:\Apps\shared.exe"),
    ];
    observe(&mut manager, &configured.sleep_prevention, &apps);
    manager.suppress_current();
    observe(
        &mut manager,
        &configured.battery_profile().sleep_prevention,
        &apps,
    );
    manager.suppress_current();
    assert_eq!(manager.suppressed.len(), 3);

    configured.sleep_prevention.enabled = false;
    let mut controller = SleepPreventionController::default();
    let mut log = ActionLog::new(4);
    manager.update(
        &mut controller,
        &configured,
        Some(false),
        false,
        &mut CycleObservations::default(),
        &mut log,
    );
    assert_eq!(manager.last_known_power_source(), Some(false));
    assert!(!manager.suppressed.contains_key(&(7, 7)));
    assert!(manager.suppressed.contains_key(&(8, 8)));
    assert!(manager.suppressed.contains_key(&(9, 9)));

    configured.sleep_prevention.enabled = true;
    manager.update(
        &mut controller,
        &configured,
        Some(true),
        false,
        &mut CycleObservations::default(),
        &mut log,
    );
    observe(&mut manager, &configured.sleep_prevention, &apps);
    assert_eq!(manager.desired_mode(), SleepPreventionMode::System);
    assert!(manager.suppressed.contains_key(&(8, 8)));
    assert!(!manager.suppressed.contains_key(&(9, 9)));
}

#[test]
fn fresh_rule_opt_ins_do_not_inherit_pending_sleep_suppression() {
    let rules = settings();
    for previous in [
        SleepPreventionSettings::default(),
        SleepPreventionSettings {
            enabled: true,
            ..Default::default()
        },
        SleepPreventionSettings {
            enabled: true,
            rules: rules.rules[..1].to_vec(),
        },
    ] {
        let previously_configured = previous.rules.len();
        let mut configured = Settings {
            sleep_prevention: previous,
            ..Settings::default()
        };
        let mut manager = SleepPreventionManager::default();
        manager.note_configuration(&configured, Some(true));
        manager.suppress_current();
        configured.sleep_prevention = rules.clone();
        manager.note_configuration(&configured, Some(true));
        observe(
            &mut manager,
            &configured.sleep_prevention,
            &[
                process(7, r"C:\Apps\worker.exe"),
                process(8, r"C:\Apps\video.exe"),
            ],
        );
        assert_eq!(manager.suppressed.len(), previously_configured);
        assert_eq!(
            manager.desired_mode(),
            SleepPreventionMode::SystemAndDisplay
        );
    }
}

#[test]
fn pending_sleep_takes_precedence_over_a_saved_opt_in() {
    let mut configured = Settings {
        sleep_prevention: settings(),
        ..Settings::default()
    };
    let mut manager = SleepPreventionManager::default();
    configured.sleep_prevention.rules[0].enabled = false;
    manager.change_power_source(Some(true));
    manager.note_configuration(&configured, Some(true));

    configured.sleep_prevention.rules[0].enabled = true;
    manager.suppress_and_release(
        &mut SleepPreventionController::default(),
        &configured,
        &mut ActionLog::new(4),
    );
    manager.note_configuration(&configured, Some(true));
    observe(
        &mut manager,
        &configured.sleep_prevention,
        &[process(7, r"C:\Apps\worker.exe")],
    );

    assert_eq!(manager.desired_mode(), SleepPreventionMode::Off);
}

#[test]
fn first_unknown_power_source_blocks_enabled_requests_without_suppressing_apps() {
    let configured = Settings {
        sleep_prevention: settings(),
        ..Settings::default()
    };
    let mut manager = SleepPreventionManager::default();
    let mut controller = SleepPreventionController::default();
    let mut log = ActionLog::new(4);
    let snapshot = manager.update(
        &mut controller,
        &configured,
        None,
        true,
        &mut CycleObservations::default(),
        &mut log,
    );
    assert!(snapshot.enabled);
    assert!(!snapshot.observation_known);
    assert!(!snapshot.system_requested);
    assert!(manager.suppressed.is_empty());
    assert!(snapshot.last_error.is_some());
    assert_eq!(manager.last_known_power_source(), None);
    let mut disabled = Settings::default();
    let snapshot = manager.update(
        &mut controller,
        &disabled,
        None,
        true,
        &mut CycleObservations::default(),
        &mut log,
    );
    assert!(!snapshot.enabled);
    assert!(!snapshot.system_requested);
    assert!(snapshot.last_error.is_none());
    disabled.sleep_prevention.enabled = true;
    let snapshot = manager.update(
        &mut controller,
        &disabled,
        None,
        true,
        &mut CycleObservations::default(),
        &mut log,
    );
    assert!(snapshot.enabled);
    assert!(snapshot.observation_known);
    assert_eq!(snapshot.matched_rules, 0);
    assert!(snapshot.last_error.is_none());
}

#[test]
#[ignore = "Creates one hidden bounded-lifetime worker and temporary Windows power requests."]
fn native_running_app_requests_and_releases_system_sleep_prevention() {
    use std::{
        os::windows::process::CommandExt,
        process::{Child, Command, Stdio},
    };

    struct Ping(Child);
    impl Drop for Ping {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let executable = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32\\ping.exe");
    let worker = Ping(
        Command::new(&executable)
            .args(["127.0.0.1", "-n", "30", "-w", "1000"])
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut settings = Settings {
        sleep_prevention: SleepPreventionSettings {
            enabled: true,
            rules: vec![SleepPreventionRule {
                enabled: true,
                executable_path: executable.to_string_lossy().into_owned(),
                keep_display_on: false,
            }],
        },
        ..Settings::default()
    };
    let mut manager = SleepPreventionManager::default();
    let mut controller = SleepPreventionController::default();
    let mut log = ActionLog::new(4);
    let snapshot = manager.update(
        &mut controller,
        &settings,
        Some(true),
        true,
        &mut CycleObservations::default(),
        &mut log,
    );
    assert!(manager
        .matches
        .apps
        .keys()
        .any(|identity| identity.0 == worker.0.id()));
    assert!(snapshot.system_requested, "{snapshot:?}");
    assert!(!snapshot.display_requested);
    assert!(snapshot.last_error.is_none(), "{snapshot:?}");

    for watcher_active in [false, true] {
        let snapshot = manager.update(
            &mut controller,
            &settings,
            Some(false),
            watcher_active,
            &mut CycleObservations::default(),
            &mut log,
        );
        assert!(!snapshot.system_requested, "{snapshot:?}");
        assert!(manager
            .suppressed
            .keys()
            .any(|identity| identity.0 == worker.0.id()));
    }

    settings.sleep_prevention.enabled = false;
    manager.update(
        &mut controller,
        &settings,
        Some(false),
        true,
        &mut CycleObservations::default(),
        &mut log,
    );
    settings.sleep_prevention.enabled = true;
    let snapshot = manager.update(
        &mut controller,
        &settings,
        Some(false),
        true,
        &mut CycleObservations::default(),
        &mut log,
    );
    assert!(snapshot.system_requested, "{snapshot:?}");

    settings.sleep_prevention.enabled = false;
    let snapshot = manager.update(
        &mut controller,
        &settings,
        Some(true),
        true,
        &mut CycleObservations::default(),
        &mut log,
    );
    assert!(!snapshot.system_requested);
    assert!(!snapshot.display_requested);
    assert!(snapshot.last_error.is_none(), "{snapshot:?}");
}
