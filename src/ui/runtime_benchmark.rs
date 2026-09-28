//! Opt-in local benchmark host. Uses production presets, runtime, and recovery;
//! never loads or saves the user's configuration and never starts the UI.
use super::presets::{apply_built_in_adaptive_engine_preset, BuiltInAdaptiveEnginePreset};
use crate::{
    application::settings::RuntimeSettingsSnapshot,
    backend::{automation::RuntimeHandle, crash_recovery, privilege},
    config::{ProcessThreadPrioritySetting, Settings},
    power, SingleInstanceGuard,
};
use std::{
    fs,
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

fn settings_for(case: &str) -> Result<Settings, String> {
    let mut settings = Settings::default();
    match case {
        "Default" | "DefaultPCores" => settings.general.enabled = false,
        "Speed"
        | "SpeedThreadDefault"
        | "SpeedThreadDefaultPowerOff"
        | "SpeedRestraint"
        | "SpeedRestraintThreadDefault" => {
            apply_built_in_adaptive_engine_preset(
                &mut settings,
                BuiltInAdaptiveEnginePreset::Speed,
            );
            settings.adaptive_engine.enabled = true;
            if case.contains("ThreadDefault") {
                settings
                    .adaptive_engine_process
                    .thread_priority
                    .foreground_priority = ProcessThreadPrioritySetting::Default;
            }
            if case.ends_with("PowerOff") {
                settings.adaptive_engine.processor_power_policy_enabled = false;
            }
            settings
                .adaptive_engine_process
                .cpu_pressure_restraint_enabled = case.contains("Restraint");
        }
        _ => return Err(format!("Unknown benchmark case: {case}")),
    }
    Ok(settings)
}

fn write(path: &Path, name: &str, content: &str) -> Result<(), String> {
    let temporary = path.join(format!("{name}.tmp"));
    fs::write(&temporary, content).map_err(|e| e.to_string())?;
    fs::rename(temporary, path.join(name)).map_err(|e| e.to_string())
}

pub(crate) fn run_if_requested() -> bool {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_none_or(|arg| arg != "--runtime-benchmark") {
        return false;
    }
    let Some(directory) = args.get(2) else {
        return true;
    };
    let path = Path::new(directory);
    let result = args
        .get(3)
        .and_then(|case| case.to_str())
        .ok_or_else(|| "Missing benchmark case".to_owned())
        .and_then(|case| run(path, case));
    let text = match result {
        Ok(()) => "ok".to_owned(),
        Err(error) => format!("error: {error}"),
    };
    if let Err(error) = write(path, "finished.txt", &text) {
        eprintln!("{text}; {error}");
    }
    true
}

fn run(path: &Path, case: &str) -> Result<(), String> {
    if !path.is_dir() {
        return Err("Benchmark directory does not exist".to_owned());
    }
    if !privilege::is_running_as_admin() {
        return Err("Benchmark host must run elevated".to_owned());
    }
    let _guard = SingleInstanceGuard::acquire(false).ok_or_else(|| {
        "Close Winderust before benchmarking; another instance owns the runtime".to_owned()
    })?;
    let settings = settings_for(case)?;
    let topology = crate::features::cpu_control::cpu_allocation::logical_processors();
    let mut topology_csv = "index,core,kind,efficiency_class\n".to_owned();
    for cpu in topology {
        topology_csv.push_str(&format!(
            "{},{},{:?},{}\n",
            cpu.index, cpu.core_index, cpu.kind, cpu.efficiency_class
        ));
    }
    write(path, "topology.csv", &topology_csv)?;
    write(
        path,
        "settings.toml",
        &toml::to_string_pretty(&settings).map_err(|e| e.to_string())?,
    )?;
    let original = power::active_plan()?.guid;
    // Do not clean stale plans here: an unexpected managed baseline invalidates this test.
    if power::list_plans()?
        .iter()
        .any(|p| p.name == "Winderust Adaptive")
    {
        return Err(
            "An existing Winderust Adaptive plan must be recovered before benchmarking".to_owned(),
        );
    }
    let mut recovery = crash_recovery::RecoveryClient::start();
    if let Some(error) = crash_recovery::startup_error() {
        return Err(error);
    }
    let enabled = settings.adaptive_engine.enabled;
    let runtime = RuntimeHandle::start(&RuntimeSettingsSnapshot {
        runtime_revision: Default::default(),
        persisted_revision: Default::default(),
        value: Arc::new(settings),
    });
    let result = (|| {
        let start = Instant::now();
        let mut ready = false;
        let mut samples = String::new();
        while start.elapsed() < Duration::from_secs(600) {
            if path.join("stop").exists() {
                break;
            }
            if let Some(status) = runtime.status_snapshot_since(0) {
                if let Some(error) = &status.worker_error {
                    return Err(error.clone());
                }
                let adaptive = &status.feature_status.adaptive_engine_process;
                samples.push_str(&format!(
                    "elapsed={:.1} pressure={} foreground_cpu_tenths={:?} scanned={} adjusted={} failed={} error={:?}\n",
                    start.elapsed().as_secs_f64(), adaptive.cpu_pressure_restraint_active,
                    adaptive.foreground_cpu_usage_tenths, adaptive.scanned_processes,
                    adaptive.adjusted_processes, adaptive.failed_processes, adaptive.last_error,
                ));
                if !ready
                    && (!enabled
                        || status
                            .feature_status
                            .adaptive_engine_process
                            .scanned_processes
                            > 0)
                {
                    write(path, "ready.txt", &original)?;
                    ready = true;
                }
            }
            if !ready && start.elapsed() > Duration::from_secs(20) {
                return Err("Runtime did not become ready within 20 seconds".to_owned());
            }
            thread::sleep(Duration::from_secs(1));
        }
        write(path, "runtime-status.txt", &samples)?;
        write(
            path,
            "final-status.txt",
            &format!("{:#?}", runtime.status_snapshot_since(0)),
        )?;
        if !path.join("stop").exists() {
            return Err("Benchmark timed out".to_owned());
        }
        Ok(())
    })();
    // Both cleanup stages must run, even when measurement or restoration fails.
    let shutdown = runtime.shutdown();
    let recovered = recovery.finish();
    let restored = power::active_plan().and_then(|plan| {
        if plan.guid.eq_ignore_ascii_case(&original) {
            Ok(())
        } else {
            Err(format!(
                "Active plan was not restored: {} (expected {original})",
                plan.guid
            ))
        }
    });
    let errors: Vec<_> = [result, shutdown, recovered, restored]
        .into_iter()
        .filter_map(Result::err)
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
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
}
