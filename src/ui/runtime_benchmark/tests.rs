use super::*;

#[test]
fn benchmark_cases_change_only_the_named_tuning() {
    let speed = settings_for("Speed").unwrap();
    let mut expected = Settings::default();
    apply_built_in_adaptive_engine_preset(&mut expected, BuiltInAdaptiveEnginePreset::Speed);
    expected.adaptive_engine.enabled = true;
    assert_eq!(speed, expected);
    expected
        .adaptive_engine_process
        .thread_priority
        .foreground_priority = ProcessThreadPrioritySetting::Default;
    assert_eq!(settings_for("SpeedThreadDefault").unwrap(), expected);
    expected.adaptive_engine.processor_power_policy_enabled = false;
    assert_eq!(
        settings_for("SpeedThreadDefaultPowerOff").unwrap(),
        expected
    );
    assert!(!settings_for("Default").unwrap().general.enabled);
    assert_eq!(
        settings_for("DefaultPCores").unwrap(),
        settings_for("Default").unwrap()
    );
    assert!(
        settings_for("SpeedRestraint")
            .unwrap()
            .adaptive_engine_process
            .cpu_pressure_restraint_enabled
    );
    assert!(settings_for("unknown").is_err());
}

#[test]
fn balanced_export_matches_the_canonical_preset_and_roundtrips() {
    let path = std::env::temp_dir().join(format!(
        "winderust-benchmark-settings-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    let mut expected = Settings::default();
    apply_built_in_adaptive_engine_preset(&mut expected, BuiltInAdaptiveEnginePreset::Balanced);
    expected.adaptive_engine.enabled = true;
    assert_eq!(export_settings(&path, "Balanced").unwrap(), expected);
    assert!(
        !expected
            .adaptive_engine_process
            .cpu_pressure_restraint_enabled
    );

    let host = std::env::current_exe().unwrap();
    fs::write(
        path.join("request.json"),
        serde_json::to_vec(&serde_json::json!({ "benchmark_host_path": host })).unwrap(),
    )
    .unwrap();
    expected
        .adaptive_engine_process
        .custom_rules
        .push(ProcessExclusionRule {
            executable_path: host.to_str().unwrap().to_owned(),
            ..Default::default()
        });
    assert_eq!(export_settings(&path, "Balanced").unwrap(), expected);
    assert_eq!(
        toml::from_str::<Settings>(&fs::read_to_string(path.join("settings.toml")).unwrap())
            .unwrap(),
        expected
    );
    assert_eq!(
        serde_json::from_slice::<Settings>(&fs::read(path.join("settings.json")).unwrap()).unwrap(),
        expected
    );
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn balanced_request_changes_only_explicit_overrides() {
    let mut settings = settings_for("Balanced").unwrap();
    let mut expected = settings.clone();
    let request: BenchmarkRequest = serde_json::from_value(serde_json::json!({
        "dynamic_resource_zones_enabled": true,
        "limit_background_processors_enabled": false,
        "cpu_pressure_restraint_enabled": true,
        "background_app_cpu_threshold_percent": 100,
        "maximum_restrained_apps": 32,
        "foreground_or_system_cpu_threshold_percent": 1,
        "background_pressure_ac_boost_policy": 0,
        "background_pressure_ac_boost_mode": "disabled"
    }))
    .unwrap();
    let process = &mut expected.adaptive_engine_process;
    process.dynamic_resource_zones_enabled = true;
    process.limit_background_processors_enabled = false;
    process.cpu_pressure_restraint_enabled = true;
    process.background_app_cpu_threshold_percent = 100;
    process.maximum_restrained_apps = 32;
    process.foreground_or_system_cpu_threshold_percent = 1;
    expected
        .adaptive_engine
        .background_pressure_profile
        .ac_policy = 0;
    expected.adaptive_engine.background_pressure_profile.ac_mode =
        power::ProcessorBoostMode::Disabled;
    request.apply(&mut settings).unwrap();
    assert_eq!(settings, expected);
    serde_json::from_str::<BenchmarkRequest>(r#"{"cpu_pressure_restraint_enabled":false}"#)
        .unwrap()
        .apply(&mut settings)
        .unwrap();
    expected
        .adaptive_engine_process
        .cpu_pressure_restraint_enabled = false;
    assert_eq!(settings, expected);
}

#[test]
fn benchmark_request_rejects_unknown_malformed_and_out_of_range_values() {
    for raw in [
        r#"{"processor_limit_percent":75}"#,
        r#"{"cpu_pressure_restraint_enabled":"true"}"#,
        r#"{"background_pressure_ac_boost_mode":"unknown"}"#,
        r#"{"background_app_cpu_threshold_percent":256}"#,
        r#"{"background_app_cpu_threshold_percent":0}"#,
        r#"{"maximum_restrained_apps":33}"#,
        r#"{"foreground_or_system_cpu_threshold_percent":101}"#,
        r#"{"background_pressure_ac_boost_policy":101}"#,
        r#"{"benchmark_host_path":"relative.exe"}"#,
        r#"{"benchmark_host_path":""}"#,
    ] {
        let result = serde_json::from_str::<BenchmarkRequest>(raw)
            .map_err(|error| error.to_string())
            .and_then(|request| request.apply(&mut settings_for("Balanced").unwrap()));
        assert!(result.is_err(), "accepted invalid request: {raw}");
    }
}
