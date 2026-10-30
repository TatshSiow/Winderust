"""Measure isolated scheduler fixtures against four disposable sibling workers.

This runner accepts only the dedicated scheduler-benchmark builds and profiles.
Workers remain outside the measured Winderust process tree. They stop cooperatively
after the collector verifies restoration; failed restoration leaves their recorded
identities and stop paths available for diagnosis. No process is forcibly killed.
"""
import argparse
import ctypes as C
import errno
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

import measure_ui_resources as collector


ROOT = Path(__file__).resolve().parent.parent
BENCHMARK = ROOT / "target" / "scheduler-benchmark"
PROFILE_FEATURES = {
    "disabled": [],
    "enabled-no-match": ["Process Priority"],
    "process-priority": ["Process Priority"],
    "thread-priority": ["Thread Priority"],
    "dynamic-priority-boost": ["Dynamic Priority Boost"],
    "io-priority": ["IO Priority"],
    "memory-priority": ["Memory Priority"],
    "gpu-priority": ["GPU Priority"],
    "background-efficiency": ["Background Efficiency"],
    "cpu-sets": ["CPU Sets (Soft)"],
    "processor-affinity": ["Processor Affinity (Hard)"],
    "cpu-limiter": ["CPU Limiter"],
    "app-suspension": ["App Suspension"],
    "by-activity": ["By Activity"],
    "by-foreground": ["By Foreground"],
    "by-running-app": ["By Running App"],
    "by-cpu-load": ["By CPU Load"],
    "by-time": ["By Time"],
    "timer-resolution": ["Timer Resolution"],
    "adaptive-detection": ["Adaptive Engine"],
    "adaptive-pressure-monitor": ["Adaptive Engine"],
    "combined": ["Background Efficiency", "IO Priority", "Memory Priority", "CPU Sets (Soft)"],
}
FRAMEWORKS = {
    "wind-ui": {"window_class": "WindUiWindowClass", "state": "absent",
                "renderer": "Wind UI 0.24.0 / tiny-skia software"},
    "iced": {"window_class": "Window Class", "state": "hidden",
             "renderer": "Iced 0.14.0 / tiny-skia software"},
}


def utc_now():
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2), encoding="utf-8")
    deadline = time.monotonic() + 0.25
    while True:
        try:
            temporary.replace(path)
            return
        except OSError as error:
            # Windows readers can briefly deny deletion of the replaced file.
            winerror = getattr(error, "winerror", None)
            transient = (winerror in (5, 32, 33) or
                         (winerror is None and error.errno in (errno.EACCES, errno.EPERM)))
            remaining = deadline - time.monotonic()
            if transient and remaining > 0:
                time.sleep(min(0.02, remaining))
                if time.monotonic() < deadline:
                    continue
            raise


def fingerprint(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return {"path": str(path), "bytes": path.stat().st_size, "sha256": digest.hexdigest()}


def dedicated_executable(path, worker=False):
    path = path.resolve(strict=True)
    if not path.is_relative_to(BENCHMARK.resolve()):
        raise ValueError("Executables must be isolated under target/scheduler-benchmark")
    if worker:
        valid_name = re.fullmatch(r"WinderustSchedulerBench-[A-Za-z0-9-]+\.exe", path.name)
    else:
        valid_name = path.name.lower() == "winderust.exe"
    if not valid_name or not path.is_file():
        raise ValueError("Unexpected scheduler fixture executable name")
    return path


def trial_order(frameworks, profiles, trials):
    for repeat in range(1, trials + 1):
        ordered = frameworks if repeat % 2 else list(reversed(frameworks))
        rotated = profiles[(repeat - 1) % len(profiles):] + profiles[:(repeat - 1) % len(profiles)]
        for profile in rotated:
            for framework in ordered:
                yield repeat, framework, profile


def build_config(framework, executable, profile, target, original_guid, manifest, verifier, renderer=None):
    spec = FRAMEWORKS[framework]
    enabled = profile != "disabled"
    common = {"schema_version": 1, "profile": profile, "scope": "fixture_owned_processes",
              "settings_io": "in_memory", "cross_session_control": False,
              "target_executable": str(target), "enabled_features": PROFILE_FEATURES[profile],
              "original_power_plan_guid": original_guid}
    contract = {"version": 1, "profile": profile,
                "ready": {"automation_enabled": enabled,
                          "scheduler_benchmark": dict(common, worker_running=enabled)},
                "finished": {"automation_enabled": enabled,
                             "worker_running_at_measurement_end": enabled,
                             "scheduler_benchmark": dict(common, worker_running=False,
                                                         restoration_succeeded=True)},
                "native_verifier": [sys.executable, "-B", str(verifier), "--phase", "{phase}",
                                    "--profile", "{profile}", "--trial-dir", "{trial_dir}",
                                    "--targets", str(manifest)]}
    collector.validate_automation_contract({"automation_contract": contract}, True)
    return {"targets": [{"name": framework, "exe": str(executable),
                         "renderer": spec["renderer"] if renderer is None else renderer,
                         "window_class": spec["window_class"],
                         "window_states": {"background": spec["state"]},
                         "args": ["--framework-benchmark-ms", "{lifetime_ms}",
                                  "--framework-benchmark-scenario", "{scenario}",
                                  "--framework-benchmark-output", "{trial_dir}",
                                  "--framework-benchmark-scheduler", profile,
                                  "--framework-benchmark-target", str(target),
                                  "--framework-benchmark-current-plan", original_guid]}],
            "scenarios": [{"name": "background", "window": spec["state"],
                           "automation_contract": contract}]}


def collector_command(config_path, output, options):
    command = [sys.executable, "-B", str(ROOT / "scripts" / "measure_ui_resources.py"),
            "--config", str(config_path), "--out", str(output), "--trials", "1",
            "--warmup", str(options.warmup), "--duration", str(options.duration),
            "--interval", str(options.interval), "--startup-timeout", str(options.startup_timeout),
            "--shutdown-timeout", str(options.shutdown_timeout), "--no-gpu", "--memory-ex2",
            "--allow-enabled-automation"]
    if options.system_energy:
        command.append("--system-energy")
    return command


def load_summary(path, protocol, resume):
    if not path.exists():
        if resume:
            raise ValueError("--resume requires an existing scheduler-measurements.json")
        return {"recorded_utc": utc_now(), "protocol": protocol, "results": [], "attempts": []}
    if not resume:
        raise ValueError("Output exists; use --resume to preserve completed trials")
    result = json.loads(path.read_text(encoding="utf-8-sig"))
    if result.get("protocol") != protocol:
        raise ValueError("Resume requires identical builds, scripts, profiles, worker load, and sampling protocol")
    keys = [(r["repeat"], r["framework"], r["profile"]) for r in result["results"]]
    if len(keys) != len(set(keys)):
        raise ValueError("Duplicate accepted scheduler trials")
    for row in result["results"]:
        collector_path = path.parent / row["collector_json"]
        if not collector_path.is_file():
            raise ValueError("Accepted scheduler trial is missing its collector JSON")
        if row.get("restoration_verified") is not True or row.get("workers_stopped") is not True:
            raise ValueError("Accepted scheduler trial lacks restoration or worker shutdown proof")
        accepted_collector(collector_path, row["framework"], row["profile"])
    result.setdefault("resume_history", []).append({"recorded_utc": utc_now(), "accepted_trials_preserved": len(keys)})
    return result


def wait_for_worker(process, ready_path, mode, timeout):
    deadline = time.perf_counter() + timeout
    while time.perf_counter() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"Worker PID {process.pid} exited before readiness")
        try:
            ready = collector.read_json(ready_path)
        except PermissionError:
            # Atomic replacement or an external file observer can briefly deny reads.
            ready = None
        if ready:
            if ready.get("pid") != process.pid or ready.get("mode") != mode:
                raise RuntimeError("Worker readiness identity or mode mismatch")
            return ready
        time.sleep(.05)
    raise RuntimeError(f"Worker PID {process.pid} readiness timeout")


def stop_workers(workers, timeout):
    for process, worker in workers:
        Path(worker["stop"]).write_text("stop\n", encoding="ascii")
    deadline = time.perf_counter() + timeout
    exits, errors = {}, []
    for process, _ in workers:
        try:
            exits[str(process.pid)] = process.wait(timeout=max(.01, deadline - time.perf_counter()))
            if process.returncode != 0:
                errors.append(f"Worker PID {process.pid} exit {process.returncode}")
        except subprocess.TimeoutExpired:
            errors.append(f"Worker PID {process.pid} did not stop cooperatively; it was not terminated")
    return exits, errors


def assert_previous_workers_exited(summary, native, capture_identity):
    for attempt in summary.get("attempts", []):
        if attempt.get("workers_stopped"):
            continue
        for worker in attempt.get("workers", []):
            handle = native.kernel.OpenProcess(0x100000, False, worker["pid"])
            if not handle:
                if C.get_last_error() == 87:  # ERROR_INVALID_PARAMETER: no such PID.
                    continue
                raise RuntimeError(f"Cannot establish whether prior worker PID {worker['pid']} exited")
            try:
                state = native.kernel.WaitForSingleObject(handle, 0)
                if state == 0:
                    continue
                if state != 258:
                    raise RuntimeError(f"Cannot query prior worker PID {worker['pid']} lifetime")
                current = capture_identity(native, worker["pid"])
            finally:
                native.kernel.CloseHandle(handle)
            if "creation_filetime" not in worker or current["creation_filetime"] == worker["creation_filetime"]:
                raise RuntimeError(f"Prior worker PID {worker['pid']} remains alive; resolve its recorded restoration before resuming")


def assert_no_other_workers(native):
    kernel = native.kernel
    for name in ("Process32FirstW", "Process32NextW"):
        function = getattr(kernel, name)
        function.argtypes, function.restype = [C.c_void_p, C.POINTER(collector.ProcessEntry)], C.c_int
    handle = kernel.CreateToolhelp32Snapshot(2, 0)
    if handle == C.c_void_p(-1).value:
        raise RuntimeError("Cannot inventory existing scheduler benchmark workers")
    matches = []
    try:
        entry = collector.ProcessEntry()
        entry.dwSize = C.sizeof(entry)
        present = kernel.Process32FirstW(handle, C.byref(entry))
        while present:
            if re.fullmatch(r"WinderustSchedulerBench-[A-Za-z0-9-]+\.exe", entry.szExeFile, re.IGNORECASE):
                matches.append(entry.th32ProcessID)
            present = kernel.Process32NextW(handle, C.byref(entry))
        if C.get_last_error() != 18:
            raise RuntimeError("Scheduler worker inventory did not complete")
    finally:
        kernel.CloseHandle(handle)
    if matches:
        raise RuntimeError(f"Existing scheduler benchmark workers must finish before this cohort: PIDs {matches}")


def accepted_collector(path, framework, profile):
    value = collector.read_json(path)
    if not value or value.get("failure") or len(value.get("results", [])) != 1:
        raise RuntimeError("Collector did not accept exactly one complete trial")
    row = value["results"][0]
    if row.get("target") != framework or row.get("scenario") != "background":
        raise RuntimeError("Collector accepted an unexpected target/scenario")
    if row.get("ready", {}).get("scheduler_benchmark", {}).get("profile") != profile:
        raise RuntimeError("Collector accepted an unexpected scheduler profile")
    finished = row.get("scheduler_native_verification", {}).get("finished", {})
    if finished.get("restoration_verified") is not True:
        raise RuntimeError("Collector lacks independent native restoration proof")
    if not (path.parent / row["csv"]).is_file():
        raise RuntimeError("Collector raw CSV is missing")
    settings = row.get("ready", {}).get("scheduler_benchmark", {}).get("settings_toml")
    if not isinstance(settings, str) or row.get("finished", {}).get("scheduler_benchmark", {}).get("settings_toml") != settings:
        raise RuntimeError("Fixture settings were not recorded consistently across the trial")
    row["scheduler_settings_sha256"] = hashlib.sha256(settings.encode("utf-8")).hexdigest()
    return row


def capture_workload_counters(workers, api):
    result = {}
    for _, worker in workers:
        tracked = collector.Process(api, worker["pid"], "external_workload")
        try:
            if tracked.created != worker["creation_filetime"] or not tracked.alive():
                raise RuntimeError("External worker identity changed during resource observation")
            values = tracked.lifetime()
            if values["exited_filetime"]:
                raise RuntimeError("External worker exited during resource observation")
            result[str(worker["pid"])] = {key: values[key] for key in ("created_filetime", "cycles", "user_s", "kernel_s")}
            result[str(worker["pid"])]["elapsed_s"] = time.perf_counter()
        finally:
            tracked.close()
    return result


def workload_observation(before, after):
    if before.keys() != after.keys():
        raise RuntimeError("External worker observation has inconsistent PID coverage")
    rows = {}
    for pid, previous in before.items():
        current = after[pid].copy()
        if current["created_filetime"] != previous["created_filetime"]:
            raise RuntimeError("External worker identity changed between observations")
        collector.add_rates(current, previous, os.cpu_count() or 1)
        current["cpu_ms_delta"] = 1000 * (current["user_s_delta"] + current["kernel_s_delta"])
        rows[pid] = current
    return {"scope": "external_workers_excluded_from_winderust_process_tree",
            "window": "whole_collector_trial_including_warmup_native_verification_and_cleanup; not_resource_sampling_window",
            "per_worker": rows, "before": before, "after": after,
            "total_cpu_ms": sum(row["cpu_ms_delta"] for row in rows.values()),
            "total_cycles": sum(row["cycles_delta"] for row in rows.values()),
            "combined_cpu_percent_one_core": sum(row["cpu_percent_one_core"] for row in rows.values()),
            "combined_cycles_per_s": sum(row["cycles_per_s"] for row in rows.values())}


def run_trial(attempt, framework_exe, worker_exe, verifier, native, capture_identity, options, original_guid, persist, resource_api):
    directory = options.out / attempt["directory"]
    directory.mkdir()
    worker_directory = directory / "workers"
    worker_directory.mkdir()
    manifest_path = directory / "targets.json"
    manifest = {"workers": [], "target_executable": str(worker_exe), "original_power_guid": original_guid}
    workers, app_started, restored = [], False, False
    measurement = None
    try:
        for index in range(4):
            prefix = worker_directory / f"worker-{index + 1}"
            worker = {"heartbeat": str(prefix.with_suffix(".heartbeat")),
                      "stop": str(prefix.with_suffix(".stop")),
                      "ready": str(prefix.with_suffix(".ready.json")), "mode": options.worker_mode}
            process = subprocess.Popen([str(worker_exe), "--heartbeat", worker["heartbeat"],
                "--stop", worker["stop"], "--mode", worker["mode"], "--ready", worker["ready"]],
                cwd=worker_exe.parent, creationflags=subprocess.CREATE_NO_WINDOW)
            worker["pid"] = process.pid
            workers.append((process, worker))
            attempt["workers"] = [entry for _, entry in workers]
            persist()
            ready = wait_for_worker(process, Path(worker["ready"]), worker["mode"], options.worker_timeout)
            identity = capture_identity(native, process.pid)
            if identity["exe"].casefold() != str(worker_exe).casefold() or identity["sha256"] != options.worker_hash:
                raise RuntimeError("Worker executable identity does not match the dedicated build")
            if ready.get("creation_filetime") != identity["creation_filetime"]:
                raise RuntimeError("Worker creation identity changed during launch")
            worker.update(identity)
            manifest["workers"].append(worker)
            write_json(manifest_path, manifest)
            persist()
        if native.active_power_guid().lower() != original_guid.lower():
            raise RuntimeError("Active power plan changed before the trial")
        config = build_config(attempt["framework"], framework_exe, attempt["profile"], worker_exe,
                              original_guid, manifest_path, verifier,
                              renderer=options.framework_renderers[attempt["framework"]])
        config_path = directory / "collector-config.json"
        write_json(config_path, config)
        command = collector_command(config_path, directory / "collector", options)
        attempt["collector_command"] = command
        workload_before = capture_workload_counters(workers, resource_api)
        with (directory / "collector.log").open("w", encoding="utf-8") as log:
            child = subprocess.Popen(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT,
                                     creationflags=subprocess.CREATE_NO_WINDOW)
            app_started = True
            attempt["collector_pid"] = child.pid
            try:
                persist()
            finally:
                code = child.wait()
        if code != 0:
            raise RuntimeError(f"Resource collector exit {code}; see {directory / 'collector.log'}")
        collector_path = directory / "collector" / "measurements.json"
        measurement = accepted_collector(collector_path, attempt["framework"], attempt["profile"])
        restored = True
        attempt["restoration_verified"] = True
        attempt["workload_observation"] = workload_observation(workload_before, capture_workload_counters(workers, resource_api))
        attempt["collector_json"] = str(collector_path.relative_to(options.out))
        attempt["measurement"] = measurement
    finally:
        if not app_started or restored:
            exits, errors = stop_workers(workers, options.worker_timeout)
            attempt["worker_exit_codes"] = exits
            attempt["workers_stopped"] = len(exits) == len(workers) and not errors
            attempt["worker_cleanup_errors"] = errors
            persist()
            if errors and sys.exc_info()[0] is None:
                raise RuntimeError("; ".join(errors))
        else:
            attempt["workers_stopped"] = False
            attempt["workers_left_running"] = [worker for process, worker in workers if process.poll() is None]
            persist()
    return measurement


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--profiles", nargs="+", choices=list(PROFILE_FEATURES), required=True)
    parser.add_argument("--frameworks", nargs="+", choices=list(FRAMEWORKS), default=list(FRAMEWORKS))
    parser.add_argument("--wind-exe", type=Path, default=BENCHMARK / "wind-build/release/winderust.exe")
    parser.add_argument("--iced-exe", type=Path, default=BENCHMARK / "iced-build/release/winderust.exe")
    parser.add_argument("--wind-renderer", default=FRAMEWORKS["wind-ui"]["renderer"],
                        help="renderer/version provenance label for the selected Wind UI build")
    parser.add_argument("--iced-renderer", default=FRAMEWORKS["iced"]["renderer"],
                        help="renderer/version provenance label for the selected Iced build")
    parser.add_argument("--worker-exe", type=Path, default=BENCHMARK / "WinderustSchedulerBench-20261020.exe")
    parser.add_argument("--worker-mode", choices=("idle", "busy"), default="idle")
    parser.add_argument("--trials", type=int, default=3)
    parser.add_argument("--warmup", type=float, default=15)
    parser.add_argument("--duration", type=float, default=60)
    parser.add_argument("--interval", type=float, default=1)
    parser.add_argument("--startup-timeout", type=float, default=45)
    parser.add_argument("--shutdown-timeout", type=float, default=30)
    parser.add_argument("--worker-timeout", type=float, default=15)
    parser.add_argument("--system-energy", action="store_true",
                        help="sample optional whole-CPU-package RAPL meter, never per-app watts")
    parser.add_argument("--resume", action="store_true")
    options = parser.parse_args()
    if options.trials < 1 or options.warmup < 0 or min(options.duration, options.interval,
            options.startup_timeout, options.shutdown_timeout, options.worker_timeout) <= 0:
        parser.error("trials and durations must be positive; warmup must be nonnegative")
    if len(set(options.profiles)) != len(options.profiles) or len(set(options.frameworks)) != len(options.frameworks):
        parser.error("profiles and frameworks must be unique")
    renderers = {"wind-ui": options.wind_renderer, "iced": options.iced_renderer}
    if any(not label.strip() for label in renderers.values()):
        parser.error("renderer provenance labels must be nonempty")
    options.framework_renderers = {name: renderers[name] for name in options.frameworks}
    options.out = Path(os.path.abspath(options.out))
    if not options.out.is_relative_to(BENCHMARK.absolute()):
        parser.error("output must be under target/scheduler-benchmark")
    options.out.mkdir(parents=True, exist_ok=True)
    options.out = options.out.resolve(strict=True)
    if not options.out.is_relative_to(BENCHMARK.resolve(strict=True)):
        parser.error("output resolves outside target/scheduler-benchmark")
    try:
        worker = dedicated_executable(options.worker_exe, worker=True)
        paths = {"wind-ui": options.wind_exe, "iced": options.iced_exe}
        executables = {framework: dedicated_executable(paths[framework]) for framework in options.frameworks}
    except (ValueError, OSError) as error:
        parser.error(str(error))
    verifier = ROOT / "scripts" / "verify_scheduler_target.py"
    protocol = {"profiles": options.profiles, "frameworks": options.frameworks,
        "trials": options.trials, "warmup_s": options.warmup, "duration_s": options.duration,
        "interval_s": options.interval, "startup_timeout_s": options.startup_timeout,
        "shutdown_timeout_s": options.shutdown_timeout, "worker_timeout_s": options.worker_timeout,
        "worker_count": 4, "worker_mode": options.worker_mode, "worker_attribution": "external_siblings_excluded_from_main_helpers_combined",
        "order": "AB/BA by repeat; rotating profiles", "gpu_enabled": False, "memory_ex2_enabled": True,
        "system_energy_enabled": options.system_energy,
        "framework_renderers": options.framework_renderers,
        "framework_builds": {name: fingerprint(path) for name, path in executables.items()},
        "worker_build": fingerprint(worker), "collector": fingerprint(ROOT / "scripts/measure_ui_resources.py"),
        "verifier": fingerprint(verifier), "orchestrator": fingerprint(Path(__file__).resolve())}
    options.worker_hash = protocol["worker_build"]["sha256"]
    output = options.out / "scheduler-measurements.json"
    try:
        summary = load_summary(output, protocol, options.resume)
    except (ValueError, RuntimeError) as error:
        parser.error(str(error))
    from verify_scheduler_target import Native, capture_identity
    native = Native()
    resource_api = collector.Windows()
    original_guid = native.active_power_guid()
    if summary.get("original_power_guid", original_guid).lower() != original_guid.lower():
        raise RuntimeError("Active power plan differs from the original scheduler cohort")
    summary["original_power_guid"] = original_guid
    assert_previous_workers_exited(summary, native, capture_identity)
    persist = lambda: write_json(output, summary)
    persist()
    completed = {(r["repeat"], r["framework"], r["profile"]) for r in summary["results"]}
    for repeat, framework, profile in trial_order(options.frameworks, options.profiles, options.trials):
        if (repeat, framework, profile) in completed:
            continue
        assert_no_other_workers(native)
        stem = collector.fresh_trial_stem(options.out, f"{repeat:02d}-{profile}-{framework}")
        attempt = {"repeat": repeat, "framework": framework, "profile": profile,
                   "directory": stem, "started_utc": utc_now(), "state": "running", "workers": []}
        summary["attempts"].append(attempt)
        persist()
        print(json.dumps({"event": "starting", "repeat": repeat, "framework": framework, "profile": profile}), flush=True)
        try:
            run_trial(attempt, executables[framework], worker, verifier, native, capture_identity,
                      options, original_guid, persist, resource_api)
            attempt["state"] = "complete"
            attempt["finished_utc"] = utc_now()
            summary["results"].append({key: value for key, value in attempt.items() if key != "state"})
            persist()
            scopes = attempt["measurement"]["scope_summaries"]
            print(json.dumps({"event": "accepted", "repeat": repeat, "framework": framework, "profile": profile,
                "combined_cycles_per_s": scopes["combined"].get("cycles_weighted_per_s"),
                "combined_cpu_percent_one_core": scopes["combined"].get("cpu_percent_one_core_weighted")}), flush=True)
        except BaseException as error:
            attempt["state"] = "failed"
            attempt["error"] = str(error)
            attempt["finished_utc"] = utc_now()
            persist()
            raise


if __name__ == "__main__":
    main()
