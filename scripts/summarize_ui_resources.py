"""Aggregate independent Windows UI resource trials without changing raw evidence.

Usage: python scripts/summarize_ui_resources.py measurements.json [...] --out DIR
Pilot/validation sources and unsafe or duplicate trials are explicitly excluded.
Different sampling protocols and executable hashes remain separate cohorts.
Only weighted counter rates are aggregated; cumulative counter levels are never
averaged. A trial's RAM/GPU median remains a trial median, not a global median.
"""
import argparse
from collections import Counter, defaultdict
import csv
import hashlib
import itertools
import json
import math
from pathlib import Path
import re
import statistics


MIB = 2**20
METRICS = {}


def metric(name, label, unit, path, factor=1):
    METRICS[name] = {"label": label, "unit": unit, "path": path, "factor": factor}


for prefix, scope in (("tree", "combined"), ("main", "main"),
                      ("helpers", "helpers"), ("sampler", "sampler")):
    base = ("scope_summaries", scope)
    metric(prefix + "_cpu_ms_per_s", prefix + " weighted CPU time", "ms/s",
           base + ("cpu_percent_one_core_weighted",), 10)
    metric(prefix + "_cycles_per_s", prefix + " weighted process cycles", "cycles/s",
           base + ("cycles_weighted_per_s",))
    for kind in ("user", "kernel"):
        metric(prefix + "_" + kind + "_cpu_ms_per_s", prefix + " " + kind + " CPU time", "ms/s",
               base + (kind + "_s_weighted_per_s",), 1000)
    for name, unit, factor in (("private_bytes", "MiB", 1 / MIB),
                               ("working_set_bytes", "MiB", 1 / MIB),
                               ("private_working_set_bytes", "MiB", 1 / MIB),
                               ("shared_commit_bytes", "MiB", 1 / MIB),
                               ("commit_bytes", "MiB", 1 / MIB),
                               ("handles", "count", 1), ("threads", "count", 1),
                               ("gdi_objects", "count", 1), ("user_objects", "count", 1)):
        short = name.replace("_bytes", "_mib")
        label = {"private_bytes": "private commit", "working_set_bytes": "total working set including shared pages",
                 "private_working_set_bytes": "private resident working set",
                 "shared_commit_bytes": "reported shared commit", "commit_bytes": "reported commit"}.get(name, name)
        metric(prefix + "_" + short, prefix + " median " + label, unit,
               base + (name, "p50"), factor)
        if name != "threads":
            metric(prefix + "_" + short + "_growth_per_min", prefix + " observed " + label + " growth",
                   unit + "/min", base + (name + "_slope_per_minute",), factor)
            metric(prefix + "_" + short + "_end_minus_start", prefix + " end minus start " + label,
                   unit, base + (name + "_end_minus_start",), factor)
    metric(prefix + "_observed_peak_working_set_mib", prefix + " peak simultaneously observed working set",
           "MiB", base + ("working_set_bytes", "max"), 1 / MIB)
    for kind in ("paged", "nonpaged"):
        metric(prefix + "_" + kind + "_pool_mib", prefix + " median " + kind + " kernel pool quota usage",
               "MiB", base + (kind + "_pool_bytes", "p50"), 1 / MIB)
    metric(prefix + "_page_faults_per_s", prefix + " page faults including soft faults", "faults/s",
           base + ("page_faults_weighted_per_s",))
    for kind in ("read", "write", "other"):
        metric(prefix + "_io_" + kind + "_kib_per_s", prefix + " process I/O " + kind,
               "KiB/s", base + ("io_" + kind + "_bytes_weighted_per_s",), 1 / 1024)
        metric(prefix + "_io_" + kind + "_ops_per_s", prefix + " process I/O " + kind + " operations",
               "ops/s", base + ("io_" + kind + "_ops_weighted_per_s",))
    metric(prefix + "_thread_context_switches_per_s", prefix + " median thread context switches",
           "switches/s", base + ("thread_context_switches_per_s", "p50"))

for name, unit, factor in (("working_set_bytes", "MiB", 1 / MIB),
                           ("commit_bytes", "MiB", 1 / MIB),
                           ("gdi_objects", "count", 1), ("user_objects", "count", 1)):
    metric("main_lifetime_peak_" + name.replace("_bytes", "_mib"),
           "main maximum observed OS lifetime peak " + name, unit,
           ("scope_summaries", "main", "peak_" + name, "max"), factor)

for prefix, scope in (("main", "main"), ("dwm_context", "compositor")):
    base = ("scope_summaries", scope)
    metric(prefix + "_gpu_busiest_engine_percent", prefix + " median busiest GPU engine",
           "%", base + ("gpu_busiest_engine_percent", "p50"))
    metric(prefix + "_gpu_busiest_engine_p95_percent", prefix + " p95 busiest GPU engine",
           "%", base + ("gpu_busiest_engine_percent", "p95"))
    for kind in ("dedicated", "shared"):
        metric(prefix + "_gpu_" + kind + "_mib", prefix + " median reported GPU " + kind + " allocation",
               "MiB", base + ("gpu_" + kind + "_bytes", "p50"), 1 / MIB)

for name, label in (("first_hwnd_ms", "first observed native HWND"),
                    ("first_visible_ms", "first observed visible window"),
                    ("ready_observed_ms", "observed fixture setup ready")):
    metric(name, label + " after launch; not first paint", "ms", (name,))
metric("cleanup_callback_ms", "coordinated cleanup request to completion callback; excludes remaining loop teardown",
       "ms", ("finished", "cleanup_ms"))
metric("cooperative_stop_to_exit_ms", "external cooperative stop to process exit, when explicitly measured",
       "ms", ("cooperative_stop_to_exit_ms",))
metric("fixture_total_scripted_actions", "whole fixture scripted actions, including warmup and final buffer",
       "count", ("finished", "actions_completed"))
metric("native_sampling_messages", "native interactions during sampling only", "count", ("native_interaction", "messages"))
metric("sampling_elapsed_s", "actual sampled duration", "s", ("sampling_elapsed_s",))
metric("sampler_collection_p95_ms", "collector p95 sample collection time", "ms",
       ("scope_summaries", "sampler", "sample_collection_ms", "p95"))
metric("system_cpu_context_percent", "whole-machine CPU median context", "%",
       ("scope_summaries", "system_context", "system_cpu_percent_machine", "p50"))
metric("system_memory_load_context_percent", "system memory load median context", "%",
       ("scope_summaries", "system_context", "system_memory_load_percent", "p50"))
metric("package_power_context_w", "whole-package meter weighted power context; not per-application", "W",
       ("scope_summaries", "system_package", "system_package_power_w_weighted"))
metric("native_sampling_messages_per_s", "native interaction rate during sampling only", "messages/s", None)
metric("deadline_overshoot_ms", "fixture timer overshoot; not shutdown duration", "ms", None)


def number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def dig(value, path):
    for part in path:
        if not isinstance(value, dict):
            return None
        value = value.get(part)
    return value


def extract_metrics(row):
    values = {}
    for name, spec in METRICS.items():
        value = dig(row, spec["path"]) if spec["path"] else None
        values[name] = value * spec["factor"] if number(value) else None
    duration = values["sampling_elapsed_s"]
    messages = values["native_sampling_messages"]
    if number(duration) and duration > 0 and number(messages):
        values["native_sampling_messages_per_s"] = messages / duration
    actual = dig(row, ("finished", "ready_to_cleanup_ms"))
    requested = dig(row, ("finished", "requested_duration_ms"))
    if number(actual) and number(requested):
        values["deadline_overshoot_ms"] = actual - requested
    return values


def aggregate(values):
    valid = [value for value in values if number(value)]
    return {"valid_trials": len(valid), "missing_trials": len(values) - len(valid),
            "median": statistics.median(valid) if valid else None,
            "min": min(valid) if valid else None, "max": max(valid) if valid else None}


def excluded_source(path, data, explicit):
    if str(path) in explicit:
        return "explicitly excluded by --exclude-source"
    tokens = {token for part in path.parts for token in re.split(r"[-_.]", part.lower())}
    if tokens & {"pilot", "validation", "smoke"}:
        return "pilot/validation/smoke source; excluded from credible results"
    if data.get("excluded") is True or data.get("credible_results") is False:
        return "source explicitly marked excluded or non-credible"
    if str(data.get("phase", "")).lower() in ("pilot", "validation", "smoke"):
        return "source phase is pilot/validation/smoke"
    return None


def invalid_trial(row):
    finished, ready = row.get("finished"), row.get("ready")
    if row.get("exit_code") != 0:
        return "fixture process did not exit successfully"
    if row.get("errors"):
        return "collector reported errors: " + str(row["errors"])
    if not isinstance(ready, dict) or ready.get("automation_enabled") is not False:
        return "readiness did not confirm disabled automation"
    if not isinstance(finished, dict) or finished.get("automation_enabled") is not False:
        return "completion did not confirm disabled automation"
    if finished.get("error") or finished.get("runtime_worker_error"):
        return "fixture reported an error"
    if finished.get("cleanup_errors") != []:
        return "missing or unsuccessful explicit cleanup result"
    if finished.get("loop_result") != {"Ok": "ok"}:
        return "missing or unsuccessful native loop result"
    if not row.get("target") or not row.get("scenario") or row.get("trial") is None:
        return "missing target/scenario/trial identity"
    if not isinstance(dig(row, ("scope_summaries", "combined")), dict):
        return "missing combined application scope"
    if not number(row.get("sampling_elapsed_s")) or row["sampling_elapsed_s"] <= 0:
        return "missing positive sampled duration"
    return None


def protocol_for(data, row):
    protocol = data.get("protocol", {})
    result = {key: protocol.get(key) for key in ("interval_s", "gpu_enabled", "system_energy_enabled",
              "thread_context_switches_enabled", "sampler_per_monitor_v2_awareness_set")}
    result.update(warmup_s=row.get("warmup_s", protocol.get("warmup_s")),
                  duration_s=row.get("duration_s_requested", protocol.get("duration_s")),
                  logical_width=row.get("width"), logical_height=row.get("height"))
    result["sampler_sha256"] = row.get("sampler_sha256", data.get("sampler_sha256"))
    hardware = data.get("hardware", {})
    # Different machines/power schemes must never be silently pooled.
    result["hardware"] = {key: hardware.get(key) for key in (
        "platform", "logical_processors", "cpu", "gpu", "system", "active_power_scheme")}
    fingerprint = hashlib.sha256(json.dumps(result, sort_keys=True).encode()).hexdigest()[:12]
    return fingerprint, result


def states_for(path, row):
    counts = row.get("window_state_counts")
    if isinstance(counts, dict):
        return counts
    csv_name = row.get("csv")
    if not csv_name:
        return {}
    csv_path = path.parent / csv_name
    if not csv_path.is_file():
        return {}
    with csv_path.open(encoding="utf-8-sig", newline="") as stream:
        return dict(Counter(record["window_state"] for record in csv.DictReader(stream)
                            if record.get("role") == "main" and record.get("window_state")))


def expected_state(data, row):
    target = next((item for item in data.get("config", {}).get("targets", [])
                   if item.get("name") == row["target"]), {})
    scenario = next((item for item in data.get("config", {}).get("scenarios", [])
                     if item.get("name") == row["scenario"]), {})
    state = target.get("window_states", {}).get(row["scenario"], scenario.get("window", "visible"))
    return ("absent" if state == "closed" else state), scenario.get("allow_window_changes", False)


def trial_notes(data, row, states):
    notes = []
    ready = row.get("ready", {})
    expected, changes = expected_state(data, row)
    if ready.get("state") != expected:
        notes.append("declared readiness state differs from configured state")
    if states and not changes and set(states) != {expected}:
        notes.append("sampled native states differ from configured state")
    if changes and (len(states) < 2 or "visible" not in states):
        notes.append("churn lacks sampled evidence of both visible and background states")
    if not states:
        notes.append("sampled window-state evidence unavailable")
    geometry = row.get("window_geometry") or {}
    dpi = geometry.get("dpi")
    if expected != "minimized" and number(dpi) and dpi > 0:
        for dimension in ("width", "height"):
            actual = geometry.get("client_pixel_" + dimension)
            logical = row.get(dimension)
            if number(actual) and number(logical) and abs(actual - round(logical * dpi / 96)) > 1:
                notes.append("client " + dimension + " differs from requested logical size and DPI")
    if row.get("native_interaction", {}).get("kind") and row["native_interaction"].get("messages", 0) <= 0:
        notes.append("scripted native interaction delivered no messages")
    return notes


def summarize(inputs, excluded=()):
    explicit = {str(Path(path).resolve()) for path in excluded}
    output = {"schema_version": 1, "sources": [], "excluded": [], "cohorts": {},
              "groups": [], "paired_differences": [], "protocol_differences": [],
              "separate_cohorts": [], "baselines": [],
              "metric_definitions": {name: {key: spec[key] for key in ("label", "unit")}
                                     for name, spec in METRICS.items()},
              "interpretation": [
                  "Each valid trial contributes once. Reported median/min/max describe independent trial summaries.",
                  "CPU ms/s = weighted one-logical-core CPU percent times 10; 1000 ms/s means one full core.",
                  "RAM median is the median of temporal trial medians; observed peak is the maximum sampled simultaneous scope working set.",
                  "Main lifetime peaks use the maximum sampled OS lifetime high-water counters; they may include startup before sampling and are never summed across PIDs.",
                  "Private bytes measure private commit, not resident RAM. Total working-set sums include shared pages and may double-count them.",
                  "Optional private working set measures private resident RAM; shared commit is reported separately and does not imply residency.",
                  "Shared commit is meaningful per PID; helper/tree shared-commit totals are unavailable because allocations can overlap.",
                  "Missing counters remain null; an unavailable GPU instance is not zero utilization.",
                  "GPU values are main PID only; DWM and package energy are shared system context, not application attribution.",
                  "Cumulative counter levels are excluded. Rates use collector interval-weighted deltas.",
                  "Short-window growth is observed growth, not evidence of a memory leak.",
                  "Setup/HWND timestamps are polling observations, not first paint or input latency.",
                  "Cleanup callback timing excludes remaining native loop teardown; timer overshoot is separate.",
                  "Fixture scripted action counts include warmup and the final lifetime buffer; native message counts cover sampling only.",
                  "Independent protocols, machine/power configurations, and executable versions are not pooled.",
                  "Paired differences match target trials within the same source and trial block; values are right minus left."]}
    records, seen_paths, seen_sources, seen_trials = [], set(), set(), set()
    for input_path in inputs:
        path = Path(input_path).resolve()
        if str(path) in seen_paths:
            output["excluded"].append({"source": str(path), "reason": "duplicate source path"})
            continue
        seen_paths.add(str(path))
        data = json.loads(path.read_text(encoding="utf-8-sig"))
        reason = excluded_source(path, data, explicit)
        if reason:
            output["excluded"].append({"source": str(path), "reason": reason,
                                       "trials": len(data.get("results", []))})
            continue
        source_hash = hashlib.sha256(path.read_bytes()).hexdigest()
        if source_hash in seen_sources:
            output["excluded"].append({"source": str(path), "reason": "duplicate source contents",
                                       "trials": len(data.get("results", []))})
            continue
        seen_sources.add(source_hash)
        output["sources"].append({"path": str(path), "sha256": source_hash,
                                   "recorded_utc": data.get("recorded_utc"), "protocol": data.get("protocol"),
                                   "sampler_sha256": data.get("sampler_sha256"),
                                   "targets": data.get("config", {}).get("targets", []),
                                   "failure": data.get("failure")})
        for baseline in data.get("baselines", []):
            output["baselines"].append({"source": str(path), "label": baseline.get("label"),
                "duration_s": baseline.get("duration_s"), "gpu_sample_status_counts": baseline.get("gpu_sample_status_counts"),
                "metrics": extract_metrics(baseline)})
        for row in data.get("results", []):
            identity = (str(path), row.get("target"), row.get("scenario"), row.get("trial"))
            reason = "duplicate trial identity within a source" if identity in seen_trials else invalid_trial(row)
            seen_trials.add(identity)
            if reason:
                output["excluded"].append({"source": str(path), "target": row.get("target"),
                    "scenario": row.get("scenario"), "trial": row.get("trial"), "reason": reason})
                continue
            cohort, protocol = protocol_for(data, row)
            output["cohorts"][cohort] = protocol
            target = next((target for target in data.get("config", {}).get("targets", [])
                           if target.get("name") == row["target"]), {})
            states = states_for(path, row)
            notes = trial_notes(data, row, states)
            if data.get("failure"):
                notes.append("source collection ended with a failure; only explicitly completed clean trials included")
            if not target.get("sha256"):
                notes.append("executable hash unavailable; version provenance is incomplete")
            records.append({"source": str(path), "target": row["target"], "scenario": row["scenario"],
                "trial": row["trial"], "cohort": cohort, "binary_sha256": target.get("sha256"),
                "metrics": extract_metrics(row), "notes": notes,
                "geometry": row.get("window_geometry"), "ready_state": row["ready"].get("state"),
                "expected_state": expected_state(data, row)[0], "window_state_counts": states,
                "native_interaction": row.get("native_interaction"),
                "gpu_sample_status_counts": row.get("gpu_sample_status_counts"),
                "gpu_counter_errors": row.get("gpu_counter_errors"),
                "gpu_coverage": {"main_samples": dig(row, ("scope_summaries", "main", "samples")),
                    "main_busy_valid_samples": dig(row, ("scope_summaries", "main", "gpu_busiest_engine_percent", "valid_samples"))},
                "thread_sample_status_counts": row.get("thread_sample_status_counts"),
                "ready_semantics": row["ready"].get("readiness", "native setup command completion; not first paint")})
    grouped = defaultdict(list)
    for record in records:
        # Unknown executable versions cannot safely be pooled across sources.
        version = record["binary_sha256"] or record["source"]
        grouped[(record["target"], record["scenario"], record["cohort"], version)].append(record)
    for (target, scenario, cohort, _), trials in sorted(grouped.items(), key=lambda item: str(item[0])):
        notes = sorted({note for trial in trials for note in trial["notes"]})
        if len(trials) < 3:
            notes.append("fewer than three independent trials")
        output["groups"].append({"target": target, "scenario": scenario, "cohort": cohort,
            "binary_sha256": trials[0]["binary_sha256"], "n": len(trials), "notes": notes,
            "metrics": {name: aggregate([trial["metrics"][name] for trial in trials]) for name in METRICS},
            "trials": trials})
    configurations = defaultdict(list)
    for group in output["groups"]:
        configurations[(group["target"], group["scenario"])].append(
            {"cohort": group["cohort"], "binary_sha256": group["binary_sha256"], "n": group["n"]})
    output["separate_cohorts"] = [{"target": target, "scenario": scenario, "configurations": items}
        for (target, scenario), items in sorted(configurations.items()) if len(items) > 1]
    add_comparisons(output)
    return output


def add_comparisons(output):
    comparable = defaultdict(list)
    for group in output["groups"]:
        comparable[(group["scenario"], group["cohort"])].append(group)
    for (scenario, cohort), groups in comparable.items():
        for left, right in itertools.combinations(groups, 2):
            if left["target"] == right["target"]:
                continue
            differences = {}
            for field in ("geometry", "ready_state", "ready_semantics", "native_interaction"):
                values = lambda group: sorted({json.dumps(trial[field], sort_keys=True) for trial in group["trials"]})
                left_values, right_values = values(left), values(right)
                if left_values != right_values:
                    differences[field] = {left["target"]: [json.loads(value) for value in left_values],
                                          right["target"]: [json.loads(value) for value in right_values]}
            if differences:
                output["protocol_differences"].append({"scenario": scenario, "cohort": cohort,
                    "targets": [left["target"], right["target"]], "differences": differences})
            left_trials = {(trial["source"], trial["trial"]): trial for trial in left["trials"]}
            pairs = [(left_trials[(trial["source"], trial["trial"])], trial) for trial in right["trials"]
                     if (trial["source"], trial["trial"]) in left_trials]
            if not pairs:
                continue
            metrics = {}
            for name in METRICS:
                values, percentages = [], []
                for a, b in pairs:
                    x, y = a["metrics"][name], b["metrics"][name]
                    values.append(y - x if number(x) and number(y) else None)
                    percentages.append(100 * (y - x) / x if number(x) and x > 0 and number(y) else None)
                metrics[name] = {"difference": aggregate(values), "percent_change_from_left": aggregate(percentages)}
            output["paired_differences"].append({"scenario": scenario, "cohort": cohort,
                "left": left["target"], "right": right["target"], "matched_pairs": len(pairs),
                "left_binary_sha256": left["binary_sha256"], "right_binary_sha256": right["binary_sha256"],
                "metrics": metrics})


def cell(value):
    if value["median"] is None:
        return "unavailable (n=0)"
    return f"{value['median']:.3g} [{value['min']:.3g}, {value['max']:.3g}] (n={value['valid_trials']})"


def markdown(summary):
    lines = ["# UI resource trial summaries", "", "Cells show independent-trial median [min, max] and valid n. Missing counters remain unavailable.", ""]
    tables = [
        ("CPU and application RAM", ["tree_cpu_ms_per_s", "tree_cycles_per_s", "tree_private_mib", "main_private_mib", "tree_working_set_mib", "tree_observed_peak_working_set_mib"]),
        ("Private resident RAM and main PID shared commit, when collected", ["main_private_working_set_mib", "helpers_private_working_set_mib", "tree_private_working_set_mib", "main_shared_commit_mib"]),
        ("Kernel pool quota usage", ["main_paged_pool_mib", "helpers_paged_pool_mib", "tree_paged_pool_mib", "main_nonpaged_pool_mib", "helpers_nonpaged_pool_mib", "tree_nonpaged_pool_mib"]),
        ("Main PID lifetime high-water counters, distinct from sampled peaks", ["main_lifetime_peak_working_set_mib", "main_lifetime_peak_commit_mib", "main_lifetime_peak_gdi_objects", "main_lifetime_peak_user_objects"]),
        ("Process resources", ["tree_handles", "tree_threads", "tree_gdi_objects", "tree_user_objects", "tree_page_faults_per_s", "tree_thread_context_switches_per_s"]),
        ("Process I/O", ["tree_io_read_kib_per_s", "tree_io_write_kib_per_s", "tree_io_other_kib_per_s", "tree_io_read_ops_per_s", "tree_io_write_ops_per_s"]),
        ("Main PID GPU", ["main_gpu_busiest_engine_percent", "main_gpu_busiest_engine_p95_percent", "main_gpu_dedicated_mib", "main_gpu_shared_mib"]),
        ("Observed timing and interaction", ["first_hwnd_ms", "ready_observed_ms", "cleanup_callback_ms", "deadline_overshoot_ms", "native_sampling_messages_per_s", "fixture_total_scripted_actions"]),
        ("Observed growth, not leak conclusions", ["tree_private_mib_growth_per_min", "tree_private_mib_end_minus_start", "tree_handles_growth_per_min", "tree_gdi_objects_end_minus_start", "tree_user_objects_end_minus_start"]),
        ("Collector and shared system context", ["sampler_cpu_ms_per_s", "sampler_collection_p95_ms", "system_cpu_context_percent", "package_power_context_w", "dwm_context_gpu_busiest_engine_percent"]),
    ]
    for title, names in tables:
        lines += ["## " + title, "", "| Scenario / target / cohort | Trials | " + " | ".join(name + " (" + METRICS[name]["unit"] + ")" for name in names) + " |",
                  "| --- | ---: | " + " | ".join("---:" for _ in names) + " |"]
        for group in summary["groups"]:
            label = f"{group['scenario']} / {group['target']} / {group['cohort']}"
            lines.append(f"| {label} | {group['n']} | " + " | ".join(cell(group["metrics"][name]) for name in names) + " |")
        lines.append("")
    lines += ["## Evidence and limits", ""] + ["- " + note for note in summary["interpretation"]]
    lines += ["", "## Exclusions", ""]
    lines += ["- " + str(item) for item in summary["excluded"]] or ["- None."]
    lines += ["", "## Protocol and state differences", ""]
    for item in summary["separate_cohorts"]:
        lines.append("- " + item["scenario"] + " / " + item["target"] +
                     ": separate protocols or executable versions retained: " +
                     json.dumps(item["configurations"], sort_keys=True))
    for item in summary["protocol_differences"]:
        lines.append("- " + item["scenario"] + " / " + item["cohort"] + ": " + json.dumps(item["differences"], sort_keys=True))
    for group in summary["groups"]:
        if group["notes"]:
            lines.append("- " + group["scenario"] + " / " + group["target"] + ": " + "; ".join(group["notes"]))
    lines += ["", "Full per-trial values, GPU status counts, cohorts, paired differences, and no-app baselines are in summary.json.", ""]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inputs", type=Path, nargs="+")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--exclude-source", type=Path, action="append", default=[])
    args = parser.parse_args()
    summary = summarize(args.inputs, args.exclude_source)
    args.out.mkdir(parents=True, exist_ok=True)
    paths = [args.out / "summary.json", args.out / "tables.md"]
    if any(path.resolve() in {source.resolve() for source in args.inputs} for path in paths):
        parser.error("summary output must not replace an input file")
    paths[0].write_text(json.dumps(summary, indent=2, allow_nan=False) + "\n", encoding="utf-8")
    paths[1].write_text(markdown(summary), encoding="utf-8")
    print(json.dumps({"groups": len(summary["groups"]), "accepted_trials": sum(group["n"] for group in summary["groups"]),
                      "exclusions": len(summary["excluded"]), "summary": str(paths[0]), "tables": str(paths[1])}))


if __name__ == "__main__":
    main()
