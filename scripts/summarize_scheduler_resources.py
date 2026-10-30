"""Export accepted scheduler trials, exact framework pairs, and descriptive ranges.

Usage: python scripts/summarize_scheduler_resources.py COHORT.json [...] --out DIR
Pairing never crosses source cohorts. Compatible matched pairs may be summarized
together, preserving every cohort and within-pair difference. Failed attempts and
single-framework observations are retained but excluded from comparison groups.
"""
import argparse
from collections import defaultdict
import csv
import hashlib
import json
import math
from pathlib import Path
import statistics
import time
import uuid


MIB = 2 ** 20
SCOPES = ("combined", "main", "helpers", "sampler", "system_package", "compositor", "system_context")
METRICS = {}


def metric(name, source, factor=1):
    METRICS[name] = (tuple(source.split(".")), factor)


for name in ("cpu_percent_one_core_weighted", "cycles_weighted_per_s", "page_faults_weighted_per_s"):
    metric(name, name)
for kind in ("user", "kernel"):
    metric(kind + "_cpu_ms_per_s", kind + "_s_weighted_per_s", 1000)
for source, label in (("private_bytes", "private_commit_mib"),
                      ("working_set_bytes", "total_working_set_mib"),
                      ("private_working_set_bytes", "private_resident_mib"),
                      ("shared_commit_bytes", "shared_commit_mib"),
                      ("commit_bytes", "reported_commit_mib"),
                      ("paged_pool_bytes", "paged_pool_mib"),
                      ("nonpaged_pool_bytes", "nonpaged_pool_mib"),
                      ("handles", "handles"), ("threads", "threads"),
                      ("gdi_objects", "gdi_objects"), ("user_objects", "user_objects")):
    factor = 1 / MIB if source.endswith("_bytes") else 1
    for statistic in ("p50", "p95", "max"):
        metric(label + "_" + statistic, source + "." + statistic, factor)
    for statistic in ("end_minus_start", "slope_per_minute"):
        metric(label + "_" + statistic, source + "_" + statistic, factor)
for source in ("cpu_percent_one_core", "cycles_per_s", "page_faults_per_s",
               "sample_collection_ms", "system_cpu_percent_machine", "system_memory_load_percent",
               "gpu_busiest_engine_percent", "thread_context_switches_per_s"):
    for statistic in ("p50", "p95", "max"):
        metric(source + "_" + statistic, source + "." + statistic)
for kind in ("read", "write", "other"):
    metric(f"io_{kind}_kib_per_s", f"io_{kind}_bytes_weighted_per_s", 1 / 1024)
    metric(f"io_{kind}_ops_per_s", f"io_{kind}_ops_weighted_per_s")
    for statistic in ("p50", "p95", "max"):
        metric(f"io_{kind}_kib_per_s_{statistic}", f"io_{kind}_bytes_per_s.{statistic}", 1 / 1024)
        metric(f"io_{kind}_ops_per_s_{statistic}", f"io_{kind}_ops_per_s.{statistic}")
for kind in ("dedicated", "shared"):
    for statistic in ("p50", "p95", "max"):
        metric(f"gpu_{kind}_mib_{statistic}", f"gpu_{kind}_bytes.{statistic}", 1 / MIB)
metric("system_package_power_w_weighted", "system_package_power_w_weighted")
metric("samples", "samples")


def number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def dig(value, path):
    for key in path:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def scope_metrics(scope):
    return {name: value * factor if number(value := dig(scope, source)) else None
            for name, (source, factor) in METRICS.items()}


def aggregate(values):
    values = list(values)
    present = [value for value in values if number(value)]
    return {"n": len(present), "missing": len(values) - len(present),
            "median": statistics.median(present) if present else None,
            "min": min(present) if present else None, "max": max(present) if present else None}


def fingerprint(path):
    path = Path(path).resolve(strict=True)
    return {"path": str(path), "bytes": path.stat().st_size,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def protocol_identity(protocol, environment="unspecified"):
    # Orchestrator safety/handshake fixes and profile ordering remain recorded in
    # each source. They do not pool incompatible loads, counters, or binaries.
    keys = ("warmup_s", "duration_s", "interval_s", "worker_count", "worker_mode",
            "worker_attribution", "gpu_enabled", "memory_ex2_enabled")
    identity = {key: protocol.get(key) for key in keys}
    identity["environment_tag"] = environment
    identity["framework_builds"] = {key: value.get("sha256")
                                    for key, value in protocol.get("framework_builds", {}).items()}
    for key in ("worker_build", "collector", "verifier"):
        identity[key] = protocol.get(key, {}).get("sha256")
    return hashlib.sha256(json.dumps(identity, sort_keys=True).encode()).hexdigest()[:16], identity


def validate_trial(row, measurement):
    if (row.get("restoration_verified") is not True or row.get("workers_stopped") is not True or
            row.get("worker_cleanup_errors") or any(value != 0 for value in row.get("worker_exit_codes", {}).values())):
        raise ValueError("Accepted trial lacks clean target restoration/shutdown")
    if measurement.get("exit_code") != 0 or measurement.get("errors"):
        raise ValueError("Accepted trial contains collector/app errors")
    if measurement.get("target") != row["framework"] or measurement.get("scenario") != "background":
        raise ValueError("Accepted collector target/scenario mismatch")
    ready, finished = measurement.get("ready", {}), measurement.get("finished", {})
    if ready.get("runtime_status", {}).get("worker_error") is not None:
        raise ValueError("Runtime worker error at readiness")
    if finished.get("measurement_runtime_status", {}).get("worker_error") is not None:
        raise ValueError("Runtime worker error during measurement")
    expected_running = row["profile"] != "disabled"
    if finished.get("worker_running_at_measurement_end") is not expected_running:
        raise ValueError("Runtime worker state changed during measurement")
    if (finished.get("cleanup_errors") or finished.get("runtime_worker_error") is not None or
            finished.get("loop_result") != {"Ok": "ok"}):
        raise ValueError("Accepted trial contains runtime cleanup failure")
    contract = ready.get("scheduler_benchmark", {})
    end_contract = finished.get("scheduler_benchmark", {})
    if contract.get("profile") != row["profile"] or end_contract.get("profile") != row["profile"]:
        raise ValueError("Accepted scheduler profile mismatch")
    if contract.get("worker_running") is not expected_running:
        raise ValueError("Unexpected runtime worker state at readiness")
    if end_contract.get("restoration_succeeded") is not True or end_contract.get("worker_running") is not False:
        raise ValueError("Fixture did not report successful scheduler shutdown")
    settings = contract.get("settings_toml")
    if not isinstance(settings, str) or settings != end_contract.get("settings_toml"):
        raise ValueError("Settings differ between ready and cleanup")
    digest = hashlib.sha256(settings.encode()).hexdigest()
    if measurement.get("scheduler_settings_sha256", digest) != digest:
        raise ValueError("Recorded scheduler settings hash is incorrect")
    proof = measurement.get("scheduler_native_verification", {})
    if proof.get("finished", {}).get("restoration_verified") is not True:
        raise ValueError("Accepted trial lacks independent native restoration proof")
    for phase in ("prepare", "ready", "finished"):
        evidence = proof.get(phase, {})
        if evidence.get("status") not in ("verified", "status_only", "unsupported") or evidence.get("errors"):
            raise ValueError(f"Accepted trial has invalid native {phase} proof")
    return digest


def validate_collector_provenance(collected, row, protocol):
    targets = collected.get("config", {}).get("targets", [])
    expected_binary = protocol.get("framework_builds", {}).get(row["framework"], {}).get("sha256")
    if (not expected_binary or len(targets) != 1 or targets[0].get("name") != row["framework"] or
            targets[0].get("sha256") != expected_binary):
        raise ValueError("Collector target/binary hash differs from the source protocol")
    measurement = collected["results"][0]
    expected_sampler = protocol.get("collector", {}).get("sha256")
    observed_sampler = measurement.get("sampler_sha256", collected.get("sampler_sha256"))
    if not expected_sampler or observed_sampler != expected_sampler:
        raise ValueError("Collector sampler hash differs from the source protocol")
    observed = collected.get("protocol", {})
    for key in ("warmup_s", "duration_s", "interval_s", "memory_ex2_enabled", "gpu_enabled"):
        if key.endswith("_enabled"):
            valid = type(protocol.get(key)) is bool and type(observed.get(key)) is bool
        else:
            valid = number(protocol.get(key)) and number(observed.get(key))
        if not valid or protocol[key] != observed[key]:
            raise ValueError(f"Collector sampling protocol differs or is missing: {key}")


def timings(measurement):
    result = {name: measurement.get(name) for name in (
        "sampling_elapsed_s", "warmup_s", "duration_s_requested", "process_lifetime_observed_s",
        "first_hwnd_ms", "first_visible_ms", "ready_observed_ms", "cooperative_stop_to_exit_ms")}
    result["cleanup_callback_ms"] = measurement.get("finished", {}).get("cleanup_ms")
    startup = measurement.get("startup_main_counters", {})
    result["startup_main_cycles"] = startup.get("cycles")
    user, kernel = startup.get("user_s"), startup.get("kernel_s")
    result["startup_main_cpu_ms"] = (user + kernel) * 1000 if number(user) and number(kernel) else None
    return result


def summarize(paths, environments=None, exclude_trials=None):
    environments = environments or {}
    exclude_trials = exclude_trials or {}
    output = {"schema_version": 1, "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "sources": [], "trials": [], "pairs": [], "groups": [], "unpaired_trial_ids": [],
              "excluded_trials": [],
              "limitations": [
                  "Comparison groups contain complete Wind/Iced pairs from the same source cohort/repeat only.",
                  "Medians/ranges describe independent trial metrics, not pooled time samples or confidence intervals.",
                  "CPU is percent of one logical core; process cycles are not instructions or a frequency-independent work unit.",
                  "Private commit is not resident RAM; private resident working set is EX2 PrivateWorkingSetSize.",
                  "External worker CPU observations span the whole collector trial, including warmup/verification/cleanup.",
                  "The Wind production close destroys its window; Iced retains a hidden window. This is an app lifecycle comparison.",
                  "System/compositor counters are context, not attributable application consumption or energy proof."]}
    inputs, keys, cohorts, matched_exclusions = set(), set(), set(), set()
    for supplied in paths:
        path = Path(supplied).resolve(strict=True)
        if path in inputs:
            raise ValueError("Duplicate source file")
        inputs.add(path)
        data = json.loads(path.read_text(encoding="utf-8-sig"))
        cohort = path.parent.name
        if cohort in cohorts:
            raise ValueError("Source cohort directory names must be unique")
        cohorts.add(cohort)
        protocol = data["protocol"]
        original_guid = str(uuid.UUID(data["original_power_guid"]))
        environment = environments.get(cohort, "unspecified")
        protocol_id, identity = protocol_identity(protocol, environment)
        source = dict(fingerprint(path), cohort=cohort, protocol=protocol, comparison_protocol_id=protocol_id,
                      environment_tag=environment,
                      comparison_protocol=identity, original_power_guid=original_guid,
                      accepted_count=len(data["results"]),
                      other_attempts=[{key: item.get(key) for key in (
                          "repeat", "framework", "profile", "state", "directory", "error", "started_utc", "finished_utc")}
                          for item in data.get("attempts", []) if item.get("state") != "complete"])
        output["sources"].append(source)
        for attempt in source["other_attempts"]:
            cleanup = (path.parent / str(attempt.get("directory", "")) / "manual-cleanup-status.json").resolve()
            if cleanup.is_relative_to(path.parent) and cleanup.is_file():
                attempt["manual_cleanup"] = {"source": fingerprint(cleanup),
                                             "evidence": json.loads(cleanup.read_text(encoding="utf-8-sig"))}
        for row in data["results"]:
            key = (cohort, row["repeat"], row["profile"], row["framework"])
            if key in keys:
                raise ValueError("Duplicate accepted trial key")
            keys.add(key)
            if row["framework"] not in ("wind-ui", "iced"):
                raise ValueError("Unexpected framework")
            collector_path = (path.parent / row["collector_json"]).resolve(strict=True)
            if not collector_path.is_relative_to(path.parent):
                raise ValueError("Collector evidence escapes its source cohort")
            collected = json.loads(collector_path.read_text(encoding="utf-8-sig"))
            if collected.get("failure") or len(collected.get("results", [])) != 1:
                raise ValueError("Accepted source does not contain exactly one successful collector result")
            validate_collector_provenance(collected, row, protocol)
            measurement = dict(collected["results"][0])
            recorded = dict(row["measurement"])
            recorded.pop("scheduler_settings_sha256", None)
            original = dict(measurement)
            original.pop("scheduler_settings_sha256", None)
            if recorded != original:
                raise ValueError("Summary measurement differs from its original collector evidence")
            settings_hash = validate_trial(row, row["measurement"])
            csv_path = (collector_path.parent / measurement["csv"]).resolve(strict=True)
            if not csv_path.is_relative_to(collector_path.parent):
                raise ValueError("Raw CSV evidence escapes its collector directory")
            proof = measurement["scheduler_native_verification"]
            trial = {"id": "/".join(map(str, key)), "cohort": cohort, "repeat": row["repeat"],
                     "profile": row["profile"], "framework": row["framework"], "environment_tag": environment,
                     "original_power_guid": original_guid,
                     "started_utc": row.get("started_utc"), "finished_utc": row.get("finished_utc"),
                     "comparison_protocol_id": protocol_id, "settings_sha256": settings_hash,
                     "paired": False, "sources": {"collector": fingerprint(collector_path), "csv": fingerprint(csv_path)},
                     "timings": timings(measurement),
                     "scopes": {scope: scope_metrics(measurement.get("scope_summaries", {}).get(scope, {})) for scope in SCOPES},
                     "coverage": {"ready_status": proof["ready"]["status"],
                         "active_action_verified": proof["ready"].get("active_action_verified", False),
                         "power_decision_verified": proof["ready"].get("power_decision_verified"),
                         "native_restoration_verified": proof["finished"]["restoration_verified"],
                         "notes": proof["ready"].get("coverage_notes", []),
                         "runtime_status": measurement["ready"].get("runtime_status"),
                         "gpu_sample_status_counts": measurement.get("gpu_sample_status_counts"),
                         "memory_sample_status_counts": measurement.get("memory_sample_status_counts")},
                     "whole_trial_external_workers": row.get("workload_observation")}
            exclusion_key = ":".join(map(str, key))
            if exclusion_key in exclude_trials:
                matched_exclusions.add(exclusion_key)
                trial.update(exclusion_key=exclusion_key, exclusion_reason=exclude_trials[exclusion_key],
                             raw_acceptance_preserved=True, excluded_from_comparison=True)
                output["excluded_trials"].append(trial)
            else:
                output["trials"].append(trial)
    if set(environments) - cohorts:
        raise ValueError("Environment tag names a cohort absent from the inputs")
    if set(exclude_trials) - matched_exclusions:
        raise ValueError("Exclusion must match exactly one accepted trial: " +
                         ", ".join(sorted(set(exclude_trials) - matched_exclusions)))
    output["raw_accepted_trials_count"] = len(output["trials"]) + len(output["excluded_trials"])
    output["comparison_eligible_trials_count"] = len(output["trials"])
    for source in output["sources"]:
        source["excluded_accepted_count"] = sum(trial["cohort"] == source["cohort"] for trial in output["excluded_trials"])
        source["comparison_eligible_count"] = source["accepted_count"] - source["excluded_accepted_count"]
    candidates = defaultdict(dict)
    for trial in output["trials"]:
        candidates[(trial["cohort"], trial["repeat"], trial["profile"])][trial["framework"]] = trial
    for key, frameworks in candidates.items():
        if set(frameworks) != {"wind-ui", "iced"}:
            output["unpaired_trial_ids"].extend(item["id"] for item in frameworks.values())
            continue
        wind, iced = frameworks["wind-ui"], frameworks["iced"]
        if wind["settings_sha256"] != iced["settings_sha256"]:
            raise ValueError(f"Paired settings differ: {key}")
        for trial in (wind, iced):
            trial["paired"] = True
        output["pairs"].append({"id": "/".join(map(str, key)), "cohort": key[0], "repeat": key[1], "profile": key[2],
            "comparison_protocol_id": wind["comparison_protocol_id"], "settings_sha256": wind["settings_sha256"],
            "original_power_guid": wind["original_power_guid"],
            "environment_tag": wind["environment_tag"],
            "wind_trial_id": wind["id"], "iced_trial_id": iced["id"],
            "wind_minus_iced": {scope: {name: a - b if number(a) and number(b) else None
                for name in METRICS for a, b in [(wind["scopes"][scope][name], iced["scopes"][scope][name])]}
                for scope in SCOPES}})
    group_pairs = defaultdict(list)
    for pair in output["pairs"]:
        group_pairs[(pair["comparison_protocol_id"], pair["profile"], pair["settings_sha256"],
                     pair["original_power_guid"])].append(pair)
    by_id = {trial["id"]: trial for trial in output["trials"]}
    for (protocol_id, profile, settings_hash, original_guid), pairs in sorted(group_pairs.items()):
        output["groups"].append({"comparison_protocol_id": protocol_id, "profile": profile,
            "settings_sha256": settings_hash, "original_power_guid": original_guid,
            "environment_tag": pairs[0]["environment_tag"],
            "paired_repeats": len(pairs), "cohorts": sorted({pair["cohort"] for pair in pairs}),
            "pair_ids": [pair["id"] for pair in pairs],
            "frameworks": {framework: {scope: {name: aggregate(
                by_id[pair[identifier]]["scopes"][scope][name] for pair in pairs) for name in METRICS} for scope in SCOPES}
                for framework, identifier in (("wind-ui", "wind_trial_id"), ("iced", "iced_trial_id"))},
            "wind_minus_iced": {scope: {name: aggregate(pair["wind_minus_iced"][scope][name] for pair in pairs)
                for name in METRICS} for scope in SCOPES}})
    return output


def write_csv(path, rows, fields):
    with path.open("w", encoding="utf-8-sig", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)


def export(summary, directory):
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    base = ["cohort", "repeat", "profile", "framework", "scope", "environment_tag", "comparison_protocol_id", "paired",
            "ready_native_status", "active_action_verified", "settings_sha256", "original_power_guid", "started_utc"]
    all_trials = summary["trials"] + summary["excluded_trials"]
    timing_fields = list(all_trials[0]["timings"]) if all_trials else []
    for filename, trials in (("trials.csv", summary["trials"]), ("excluded-trials.csv", summary["excluded_trials"])):
        rows = []
        excluded_fields = ["exclusion_key", "exclusion_reason"] if filename == "excluded-trials.csv" else []
        for trial in trials:
            for scope, values in trial["scopes"].items():
                row = {key: trial[key] for key in ("cohort", "repeat", "profile", "framework", "environment_tag", "comparison_protocol_id", "paired")}
                row.update(scope=scope, ready_native_status=trial["coverage"]["ready_status"],
                           active_action_verified=trial["coverage"]["active_action_verified"],
                           settings_sha256=trial["settings_sha256"], started_utc=trial["started_utc"],
                           original_power_guid=trial["original_power_guid"])
                row.update({key: trial[key] for key in excluded_fields})
                row.update(trial["timings"])
                row.update(values)
                rows.append(row)
        write_csv(directory / filename, rows, base + excluded_fields + timing_fields + list(METRICS))
    rows = [{**{key: pair[key] for key in ("cohort", "repeat", "profile", "environment_tag", "comparison_protocol_id",
                                         "settings_sha256", "original_power_guid")},
             "scope": scope, **values} for pair in summary["pairs"] for scope, values in pair["wind_minus_iced"].items()]
    write_csv(directory / "pairs-wind-minus-iced.csv", rows,
              ["cohort", "repeat", "profile", "scope", "environment_tag", "comparison_protocol_id",
               "settings_sha256", "original_power_guid"] + list(METRICS))
    rows = []
    for group in summary["groups"]:
        for framework, scopes in dict(group["frameworks"], **{"wind-minus-iced": group["wind_minus_iced"]}).items():
            for scope, metrics in scopes.items():
                for name, stats in metrics.items():
                    rows.append({"comparison_protocol_id": group["comparison_protocol_id"], "profile": group["profile"],
                        "environment_tag": group["environment_tag"], "framework": framework, "scope": scope,
                        "settings_sha256": group["settings_sha256"], "original_power_guid": group["original_power_guid"],
                        "metric": name, "paired_repeats": group["paired_repeats"], **stats})
    write_csv(directory / "groups.csv", rows, ["comparison_protocol_id", "profile", "framework", "scope", "metric",
                                             "environment_tag", "settings_sha256", "original_power_guid", "paired_repeats",
                                             "n", "missing", "median", "min", "max"])
    rows = []
    worker_metrics = ["interval_s", "user_s_delta", "kernel_s_delta", "cycles_delta", "cycles_per_s",
                      "cpu_percent_one_core", "cpu_percent_machine"]
    for trial in summary["trials"]:
        observation = trial.get("whole_trial_external_workers") or {}
        for pid, values in observation.get("per_worker", {}).items():
            rows.append({**{key: trial[key] for key in ("cohort", "repeat", "profile", "framework", "environment_tag", "paired")},
                "pid": pid, "window": observation.get("window"),
                **{key: values.get(key) for key in worker_metrics}})
    write_csv(directory / "external-workers-whole-trial.csv", rows,
              ["cohort", "repeat", "profile", "framework", "environment_tag", "paired", "pid", "window"] + worker_metrics)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cohorts", type=Path, nargs="+")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--environment", action="append", default=[], metavar="COHORT=TAG",
                        help="Explicit environment label; different tags cannot be pooled")
    parser.add_argument("--exclude-trial", action="append", default=[],
                        metavar="COHORT:REPEAT:PROFILE:FRAMEWORK[=REASON]",
                        help="Preserve a raw accepted trial separately and omit it from comparison")
    args = parser.parse_args()
    environments = {}
    for option in args.environment:
        name, separator, tag = option.partition("=")
        if not separator or not name or not tag or name in environments:
            parser.error("--environment requires a unique COHORT=TAG")
        environments[name] = tag
    exclusions = {}
    for option in args.exclude_trial:
        key, separator, reason = option.partition("=")
        parts = key.split(":")
        if (len(parts) != 4 or not all(parts) or not parts[1].isdigit() or int(parts[1]) <= 0 or
                parts[3] not in ("wind-ui", "iced") or key in exclusions or separator and not reason):
            parser.error("--exclude-trial requires a unique COHORT:REPEAT:PROFILE:FRAMEWORK[=REASON]")
        exclusions[key] = reason if separator else "Explicit analyst exclusion; raw collector acceptance retained"
    summary = summarize(args.cohorts, environments, exclusions)
    export(summary, args.out)
    print(json.dumps({"raw_accepted_trials": summary["raw_accepted_trials_count"],
                      "comparison_eligible_trials": len(summary["trials"]), "excluded_trials": len(summary["excluded_trials"]),
                      "matched_pairs": len(summary["pairs"]),
                      "unpaired_trials": len(summary["unpaired_trial_ids"]), "groups": len(summary["groups"])}))


if __name__ == "__main__":
    main()
