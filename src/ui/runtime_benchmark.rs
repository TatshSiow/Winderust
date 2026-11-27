//! Opt-in local benchmarks using production runtime/controllers and recovery;
//! never load or save the user's configuration or start the UI.
#[path = "runtime_benchmark/cpu_limiter.rs"]
mod cpu_limiter;

use super::presets::{apply_built_in_adaptive_engine_preset, BuiltInAdaptiveEnginePreset};
use crate::{
    acquire_single_instance,
    application::settings::RuntimeSettingsSnapshot,
    backend::{
        automation::{RuntimeHandle, RuntimeStatusSnapshot},
        crash_recovery,
        interrupt_timing::{export_report, CaptureUpdate, InterruptTimingCapture},
        privilege,
    },
    config::{AdaptiveEngineExclusionRule, PowerSourceProfile, Settings},
    control::power_plan::PowerPlanOwner,
    power,
    runtime::pressure_trace::PressureTrace,
    ui::adaptive_processor_power::BuiltInPreset,
    win_util::NamedMutexGuard,
};
use std::{
    fs,
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct BenchmarkRequest {
    benchmark_host_path: Option<String>,
    adaptive_engine_enabled: Option<bool>,
    dynamic_resource_zones_enabled: Option<bool>,
    limit_background_processors_enabled: Option<bool>,
    cpu_pressure_restraint_enabled: Option<bool>,
    restraint_energy_efficient_scheduling: Option<bool>,
    background_app_cpu_threshold_percent: Option<u8>,
    maximum_restrained_apps: Option<u8>,
    foreground_or_system_cpu_threshold_percent: Option<u8>,
    background_pressure_ac_boost_policy: Option<u8>,
    background_pressure_ac_boost_mode: Option<power::ProcessorBoostMode>,
}

impl BenchmarkRequest {
    fn apply(self, settings: &mut Settings) -> Result<(), String> {
        self.apply_process_overrides(settings)?;
        if let Some(battery) = settings.on_battery.as_deref_mut() {
            self.apply_process_overrides(battery)?;
        }
        if let Some(value) = self.background_pressure_ac_boost_policy {
            if value > 100 {
                return Err("Background pressure A/C boost policy must be 0..=100".to_owned());
            }
            settings
                .adaptive_engine
                .background_pressure_profile
                .ac_policy = u32::from(value);
        }
        if let Some(value) = self.background_pressure_ac_boost_mode {
            settings.adaptive_engine.background_pressure_profile.ac_mode = value;
        }
        settings.validate()
    }

    fn apply_process_overrides(&self, settings: &mut Settings) -> Result<(), String> {
        if let Some(enabled) = self.adaptive_engine_enabled {
            settings.adaptive_engine.enabled = enabled;
        }
        let process = &mut settings.adaptive_engine_process;
        if let Some(path) = &self.benchmark_host_path {
            if !Path::new(path).is_absolute() {
                return Err("Benchmark host path must be absolute".to_owned());
            }
            process
                .custom_rules
                .push(AdaptiveEngineExclusionRule::exact_path(path.clone()));
        }
        if let Some(value) = self.dynamic_resource_zones_enabled {
            process.dynamic_resource_zones_enabled = value;
        }
        if let Some(value) = self.limit_background_processors_enabled {
            process.limit_background_processors_enabled = value;
        }
        if let Some(value) = self.cpu_pressure_restraint_enabled {
            process.cpu_pressure_restraint_enabled = value;
        }
        if let Some(value) = self.restraint_energy_efficient_scheduling {
            process.restraint_energy_efficient_scheduling = value;
        }
        if let Some(value) = self.background_app_cpu_threshold_percent {
            if !(1..=100).contains(&value) {
                return Err("Background app CPU threshold must be 1..=100".to_owned());
            }
            process.background_app_cpu_threshold_percent = value;
        }
        if let Some(value) = self.maximum_restrained_apps {
            if !(1..=32).contains(&value) {
                return Err("Restraint candidate limit must be 1..=32".to_owned());
            }
            process.maximum_restrained_apps = value;
        }
        if let Some(value) = self.foreground_or_system_cpu_threshold_percent {
            if !(1..=100).contains(&value) {
                return Err("Foreground or system CPU threshold must be 1..=100".to_owned());
            }
            process.foreground_or_system_cpu_threshold_percent = value;
        }
        Ok(())
    }
}

fn settings_for(case: &str) -> Result<Settings, String> {
    let mut settings = Settings::default();
    let process_only = match case {
        "PowerSavePowerOff" => Some(BuiltInAdaptiveEnginePreset::PowerSave),
        "BalancedPowerOff" => Some(BuiltInAdaptiveEnginePreset::Balanced),
        "PerformancePowerOff" => Some(BuiltInAdaptiveEnginePreset::Performance),
        _ => None,
    };
    if let Some(preset) = process_only {
        apply_built_in_adaptive_engine_preset(&mut settings, preset);
        settings.adaptive_engine.enabled = true;
        return Ok(settings);
    }
    match case {
        "Default" | "DefaultPCores" => settings.general.enabled = false,
        "Balanced" => {
            apply_built_in_adaptive_engine_preset(
                &mut settings,
                BuiltInAdaptiveEnginePreset::Balanced,
            );
            settings.adaptive_engine.enabled = true;
            settings.adaptive_engine.adaptive_processor_power_enabled = true;
            BuiltInPreset::Balanced
                .apply(&mut settings.adaptive_engine, PowerSourceProfile::PluggedIn);
            BuiltInPreset::Balanced.apply(
                &mut settings.battery_profile_mut().adaptive_engine,
                PowerSourceProfile::OnBattery,
            );
        }
        "Speed" | "SpeedPowerOff" | "SpeedRestraint" => {
            apply_built_in_adaptive_engine_preset(
                &mut settings,
                BuiltInAdaptiveEnginePreset::Speed,
            );
            settings.adaptive_engine.enabled = true;
            settings.adaptive_engine.adaptive_processor_power_enabled = case != "SpeedPowerOff";
            settings.adaptive_engine.base_processor_policy =
                power::ProcessorPowerValues::new_with_boost_mode(
                    100,
                    25,
                    100,
                    100,
                    power::ProcessorBoostMode::Aggressive,
                );
            if case == "SpeedRestraint" {
                settings
                    .adaptive_engine_process
                    .limit_background_processors_enabled = false;
            }
        }
        _ => return Err(format!("Unknown benchmark case: {case}")),
    }
    Ok(settings)
}

fn export_settings(path: &Path, case: &str) -> Result<Settings, String> {
    if !path.is_dir() {
        return Err("Benchmark directory does not exist".to_owned());
    }
    let mut settings = settings_for(case)?;
    let request = match fs::read(path.join("request.json")) {
        Ok(raw) => serde_json::from_slice::<BenchmarkRequest>(&raw).map_err(|e| e.to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BenchmarkRequest::default(),
        Err(error) => return Err(error.to_string()),
    };
    request.apply(&mut settings)?;
    write(
        path,
        "settings.toml",
        &toml::to_string_pretty(&settings).map_err(|e| e.to_string())?,
    )?;
    write(
        path,
        "settings.json",
        &serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?,
    )?;
    Ok(settings)
}

fn write(path: &Path, name: &str, content: &str) -> Result<(), String> {
    let temporary = path.join(format!("{name}.tmp"));
    fs::write(&temporary, content).map_err(|e| e.to_string())?;
    fs::rename(temporary, path.join(name)).map_err(|e| e.to_string())
}

pub(crate) fn run_if_requested() -> bool {
    if cpu_limiter::run_if_requested() {
        return true;
    }
    let args: Vec<_> = std::env::args_os().collect();
    let export_only = match args.get(1).and_then(|arg| arg.to_str()) {
        Some("--runtime-benchmark") => false,
        Some("--runtime-benchmark-settings") => true,
        _ => return false,
    };
    let Some(directory) = args.get(2) else {
        return true;
    };
    let path = Path::new(directory);
    let result = args
        .get(3)
        .and_then(|case| case.to_str())
        .ok_or_else(|| "Missing benchmark case".to_owned())
        .and_then(|case| {
            if export_only {
                export_settings(path, case).map(|_| ())
            } else {
                run(path, case)
            }
        });
    let text = match result {
        Ok(()) => "ok".to_owned(),
        Err(error) => format!("error: {error}"),
    };
    if let Err(error) = write(path, "finished.txt", &text) {
        eprintln!("{text}; {error}");
    }
    true
}

fn acquire_benchmark_session() -> Result<NamedMutexGuard, String> {
    let guard = acquire_single_instance(0)
        .map_err(|error| format!("Could not check for a running Winderust instance: {error}"))?
        .ok_or_else(|| {
            "Close Winderust before benchmarking; another instance owns the runtime".to_owned()
        })?;
    crash_recovery::acquire_watchdog_handoff()
        .map_err(|error| format!("Could not wait for previous Winderust recovery: {error}"))?;
    Ok(guard)
}

fn runtime_ready(settings: &Settings, status: &RuntimeStatusSnapshot) -> bool {
    if !settings.general.enabled {
        return true;
    }
    let power_ready = !settings.adaptive_engine.adaptive_processor_power_enabled
        || status.power_plan_status.owner == Some(PowerPlanOwner::AdaptiveProcessorPower);
    let process = &status.feature_status.adaptive_engine_process;
    let process_ready = !settings.adaptive_engine.enabled
        || !settings.adaptive_engine_process.has_enabled_policies()
        || (process.enabled && !process.observation_paused);
    power_ready && process_ready
}

fn finish_interrupt_capture(
    report_path: &Path,
    capture: Option<InterruptTimingCapture>,
    report_requested: bool,
) -> Result<(), String> {
    if let Some(capture) = capture {
        let report = capture
            .finish_with_report()
            .map_err(|error| format!("Interrupt Timing cleanup failed: {error}"))?;
        if let Some(report) = report.filter(|_| report_requested) {
            return export_report(report_path, &report);
        }
    }
    if report_requested {
        return Err("Benchmark ended before Interrupt Timing completed".to_owned());
    }
    Ok(())
}

fn run(path: &Path, case: &str) -> Result<(), String> {
    if !path.is_dir() {
        return Err("Benchmark directory does not exist".to_owned());
    }
    if !privilege::is_running_as_admin() {
        return Err("Benchmark host must run elevated".to_owned());
    }
    let _guard = acquire_benchmark_session()?;
    let settings = Arc::new(export_settings(path, case)?);
    let topology = crate::features::cpu_control::cpu_allocation::logical_processors();
    let mut topology_csv = "index,core,kind,efficiency_class\n".to_owned();
    for cpu in topology {
        topology_csv.push_str(&format!(
            "{},{},{:?},{}\n",
            cpu.index, cpu.core_index, cpu.kind, cpu.efficiency_class
        ));
    }
    write(path, "topology.csv", &topology_csv)?;
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
    let trace = path
        .join("trace-engine-pressure")
        .is_file()
        .then(PressureTrace::start)
        .transpose()?;
    let mut recovery = crash_recovery::RecoveryClient::start();
    if let Some(error) = crash_recovery::startup_error() {
        return Err(error);
    }
    let runtime = RuntimeHandle::start(&RuntimeSettingsSnapshot {
        runtime_revision: Default::default(),
        persisted_revision: Default::default(),
        value: Arc::clone(&settings),
    });
    let capture_request = path.join("capture-interrupt-timing");
    let capture_report = path.join("interrupt-timing.json");
    let mut capture: Option<InterruptTimingCapture> = None;
    let mut capture_started = false;
    let result = (|| {
        let start = Instant::now();
        let mut ready = false;
        let mut samples = String::new();
        while start.elapsed() < Duration::from_secs(600) {
            if let Some(update) = capture.as_mut().and_then(InterruptTimingCapture::poll) {
                let report = match update {
                    CaptureUpdate::CleanupPending(error) => return Err(error.to_string()),
                    CaptureUpdate::Finished(report) => {
                        capture = None;
                        report.map_err(|error| error.to_string())?
                    }
                };
                export_report(&capture_report, &report)?;
            }
            if path.join("stop").exists() {
                break;
            }
            if !capture_started && capture_request.is_file() {
                capture = Some(InterruptTimingCapture::start().map_err(|error| error.to_string())?);
                capture_started = true;
            }
            if let Some(status) = runtime.status_snapshot_since(0) {
                if let Some(error) = status
                    .power_plan_status
                    .adaptive_processor_power_error
                    .as_ref()
                    .map(ToString::to_string)
                    .or_else(|| status.worker_error.clone())
                {
                    return Err(error);
                }
                let adaptive = &status.feature_status.adaptive_engine_process;
                samples.push_str(&format!(
                    "elapsed={:.1} pressure={} foreground_cpu_tenths={:?} paused={} scanned={} adjusted={} failed={} error={:?}\n",
                    start.elapsed().as_secs_f64(), adaptive.cpu_pressure_restraint_active,
                    adaptive.foreground_cpu_usage_tenths, adaptive.observation_paused,
                    adaptive.scanned_processes,
                    adaptive.adjusted_processes, adaptive.failed_processes, adaptive.last_error,
                ));
                if !ready && runtime_ready(&settings, &status) {
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
    // Finish all cleanup even when measurement or restoration fails.
    if let Some(capture) = &mut capture {
        capture.cancel();
    }
    let shutdown = runtime.shutdown();
    let report_requested =
        result.is_ok() && (capture.is_some() || !capture_started && capture_request.is_file());
    let capture_status = finish_interrupt_capture(&capture_report, capture, report_requested);
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
    let trace_status = trace.map_or(Ok(()), |trace| {
        trace
            .finish()
            .and_then(|data| write(path, "engine-pressure.json", &data))
    });
    let errors: Vec<_> = [
        result,
        shutdown,
        capture_status,
        recovered,
        restored,
        trace_status,
    ]
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
#[path = "runtime_benchmark/tests.rs"]
mod tests;
