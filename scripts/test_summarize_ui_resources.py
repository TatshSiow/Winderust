"""Synthetic evidence checks; no benchmark processes or Windows state changes."""
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import summarize_ui_resources as analysis


def trial(target="A", index=1, cpu=1, gpu=None):
    main = {"samples": 10, "cpu_percent_one_core_weighted": cpu,
            "cpu_percent_one_core": {"p50": 99}, "cycles_weighted_per_s": 1000 * cpu,
            "cycles": {"p50": 1e20}, "private_bytes": {"p50": 10 * 2**20},
            "working_set_bytes": {"p50": 20 * 2**20, "max": 22 * 2**20},
            "private_bytes_slope_per_minute": -2**20, "private_bytes_end_minus_start": -2**19,
            "handles": {"p50": 50}, "threads": {"p50": 4},
            "io_read_bytes_weighted_per_s": 2048, "io_read_bytes": {"p50": 1e15}}
    if gpu is not None:
        main["gpu_busiest_engine_percent"] = {"p50": gpu, "p95": gpu + 1, "valid_samples": 8}
    return {"target": target, "scenario": "home", "trial": index, "exit_code": 0, "errors": [],
            "width": 1120, "height": 760, "sampling_elapsed_s": 15, "duration_s_requested": 15,
            "warmup_s": 5, "first_hwnd_ms": 100, "ready_observed_ms": 200,
            "ready": {"automation_enabled": False, "state": "visible"},
            "finished": {"automation_enabled": False, "cleanup_errors": [], "runtime_worker_error": None,
                         "loop_result": {"Ok": "ok"}, "actions_completed": 0, "cleanup_ms": 20,
                         "ready_to_cleanup_ms": 23020, "requested_duration_ms": 23000},
            "window_geometry": {"dpi": 96, "client_pixel_width": 1120, "client_pixel_height": 760},
            "window_state_counts": {"visible": 10},
            "native_interaction": {"kind": None, "messages": 0},
            "scope_summaries": {"main": main, "combined": copy.deepcopy(main)}}


def dataset(rows):
    return {"protocol": {"warmup_s": 5, "duration_s": 15, "interval_s": .5,
                         "gpu_enabled": True, "system_energy_enabled": False},
            "hardware": {"platform": "Windows-test", "logical_processors": 8,
                         "active_power_scheme": "unchanged"},
            "config": {"targets": [{"name": name, "sha256": name * 64}
                                   for name in sorted({row["target"] for row in rows})],
                       "scenarios": [{"name": "home", "window": "visible"}]},
            "results": rows, "baselines": []}


class SummaryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="resource-evidence-")
        self.directory = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)

    def save(self, data, name="measurements.json"):
        path = self.directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(data), encoding="utf-8")
        return path

    def test_independent_trials_and_weighted_rates_ignore_raw_levels(self):
        rows = [trial(index=index, cpu=cpu) for index, cpu in enumerate((1, 2, 9), 1)]
        rows[1]["scope_summaries"]["main"]["samples"] = 10000
        group = analysis.summarize([self.save(dataset(rows))])["groups"][0]
        self.assertEqual(group["n"], 3)
        self.assertEqual(group["metrics"]["tree_cpu_ms_per_s"],
                         {"valid_trials": 3, "missing_trials": 0, "median": 20, "min": 10, "max": 90})
        self.assertEqual(group["metrics"]["main_cycles_per_s"]["median"], 2000)
        self.assertEqual(group["metrics"]["main_io_read_kib_per_s"]["median"], 2)
        self.assertEqual(group["metrics"]["tree_observed_peak_working_set_mib"]["median"], 22)
        self.assertNotIn("cycles", group["metrics"])

    def test_missing_gpu_and_real_zero_remain_distinct(self):
        rows = [trial(index=1), trial(index=2, gpu=0), trial(index=3, gpu=4)]
        group = analysis.summarize([self.save(dataset(rows))])["groups"][0]
        self.assertEqual(group["metrics"]["main_gpu_busiest_engine_percent"],
                         {"valid_trials": 2, "missing_trials": 1, "median": 2, "min": 0, "max": 4})
        self.assertIsNone(group["metrics"]["main_gpu_dedicated_mib"]["median"])
        self.assertEqual(group["trials"][1]["gpu_coverage"]["main_busy_valid_samples"], 8)
        self.assertIsNone(analysis.aggregate([None, float("nan"), float("inf"), False])["median"])

    def test_private_resident_ram_and_shared_commit_preserve_missing_and_scope_values(self):
        first, second = trial(index=1), trial(index=2)
        for scope, resident, shared in (("main", 8, 3), ("helpers", 2, None), ("combined", 10, None)):
            summary = second["scope_summaries"].setdefault(scope, {})
            summary["private_working_set_bytes"] = {"p50": resident * 2**20}
            summary["shared_commit_bytes"] = {"p50": shared * 2**20} if shared is not None else None
        group = analysis.summarize([self.save(dataset([first, second]))])["groups"][0]
        for prefix, resident, shared in (("main", 8, 3), ("helpers", 2, None), ("tree", 10, None)):
            self.assertEqual(group["metrics"][prefix + "_private_working_set_mib"],
                {"valid_trials": 1, "missing_trials": 1, "median": resident, "min": resident, "max": resident})
            self.assertEqual(group["metrics"][prefix + "_shared_commit_mib"]["median"], shared)
        self.assertIsNone(group["trials"][0]["metrics"]["tree_private_working_set_mib"])
        self.assertEqual(group["metrics"]["tree_private_mib"]["median"], 10)
        text = analysis.markdown(analysis.summarize([self.directory / "measurements.json"]))
        self.assertIn("tree_private_working_set_mib", text)
        self.assertIn("private commit, not resident RAM", text)

    def test_main_lifetime_peaks_and_temporal_pool_usage_are_distinct_from_observed_tree_peaks(self):
        rows = [trial(index=1), trial(index=2)]
        for index, row in enumerate(rows):
            main = row["scope_summaries"]["main"]
            main["peak_working_set_bytes"] = {"p50": 70 * 2**20, "max": (80 + index * 20) * 2**20}
            main["peak_commit_bytes"] = {"max": (60 + index * 20) * 2**20}
            main["peak_gdi_objects"] = {"max": 30 + index * 10}
            main["peak_user_objects"] = {"max": 40 + index * 10}
            for scope, value in (("main", 1), ("helpers", 2), ("combined", 3)):
                summary = row["scope_summaries"].setdefault(scope, {})
                summary["paged_pool_bytes"] = {"p50": value * 2**20, "max": 999 * 2**20}
                summary["nonpaged_pool_bytes"] = {"p50": value * 2**19}
            row["scope_summaries"]["combined"]["sum_process_lifetime_peak_working_set_bytes"] = {"max": 999 * 2**20}
        group = analysis.summarize([self.save(dataset(rows))])["groups"][0]
        metrics = group["metrics"]
        self.assertEqual(metrics["main_lifetime_peak_working_set_mib"],
                         {"valid_trials": 2, "missing_trials": 0, "median": 90, "min": 80, "max": 100})
        self.assertEqual(metrics["main_lifetime_peak_commit_mib"]["median"], 70)
        self.assertEqual(metrics["main_lifetime_peak_gdi_objects"]["median"], 35)
        self.assertEqual(metrics["main_lifetime_peak_user_objects"]["median"], 45)
        self.assertEqual(metrics["tree_observed_peak_working_set_mib"]["median"], 22)
        self.assertEqual(metrics["main_observed_peak_working_set_mib"]["median"], 22)
        for prefix, value in (("main", 1), ("helpers", 2), ("tree", 3)):
            self.assertEqual(metrics[prefix + "_paged_pool_mib"]["median"], value)
            self.assertEqual(metrics[prefix + "_nonpaged_pool_mib"]["median"], value / 2)
        self.assertNotIn("tree_lifetime_peak_working_set_mib", metrics)
        self.assertIsNone(analysis.extract_metrics(trial())["main_lifetime_peak_commit_mib"])

    def test_unclean_unsafe_and_duplicate_trials_are_excluded(self):
        rows = [trial(index=index) for index in range(1, 7)]
        rows[1]["finished"]["cleanup_errors"] = ["restoration unconfirmed"]
        rows[2]["ready"]["automation_enabled"] = True
        rows[3]["finished"].pop("loop_result")
        rows[4]["exit_code"] = 1
        rows[5]["errors"] = ["helper inaccessible"]
        rows.append(copy.deepcopy(rows[0]))
        summary = analysis.summarize([self.save(dataset(rows))])
        self.assertEqual(summary["groups"][0]["n"], 1)
        self.assertEqual(len(summary["excluded"]), 6)

    def test_pilot_and_explicit_sources_never_enter_credible_results(self):
        pilot = self.save(dataset([trial()]), "pilot-run/measurements.json")
        ordinary = self.save(dataset([trial()]))
        summary = analysis.summarize([pilot, ordinary], [ordinary])
        self.assertEqual(summary["groups"], [])
        self.assertEqual(len(summary["excluded"]), 2)

    def test_different_protocols_and_binary_versions_are_not_pooled(self):
        first = dataset([trial()])
        slower = copy.deepcopy(first)
        slower["protocol"]["interval_s"] = 1
        newer = copy.deepcopy(first)
        newer["config"]["targets"][0]["sha256"] = "new-version"
        summary = analysis.summarize([self.save(first, "first.json"), self.save(slower, "slower.json"),
                                      self.save(newer, "newer.json")])
        self.assertEqual(len(summary["groups"]), 3)
        self.assertEqual(len(summary["cohorts"]), 2)
        self.assertTrue(all(group["n"] == 1 for group in summary["groups"]))
        self.assertEqual(len(summary["separate_cohorts"][0]["configurations"]), 3)

    def test_different_collector_hashes_are_separate_cohorts(self):
        first, second = dataset([trial()]), dataset([trial()])
        first["sampler_sha256"], second["sampler_sha256"] = "collectorA", "collectorB"
        summary = analysis.summarize([self.save(first, "first.json"), self.save(second, "second.json")])
        self.assertEqual(len(summary["cohorts"]), 2)
        self.assertEqual(len(summary["groups"]), 2)
        self.assertTrue(all(group["n"] == 1 for group in summary["groups"]))
        self.assertEqual(summary["paired_differences"], [])

    def test_resumed_row_collector_hash_overrides_original_source_hash(self):
        first, second = trial(index=1), trial(index=2)
        second["sampler_sha256"] = "collectorB"
        data = dataset([first, second])
        data["sampler_sha256"] = "collectorA"
        summary = analysis.summarize([self.save(data)])
        self.assertEqual({cohort["sampler_sha256"] for cohort in summary["cohorts"].values()},
                         {"collectorA", "collectorB"})
        self.assertEqual(len(summary["groups"]), 2)

    def test_copied_raw_source_cannot_double_independent_trial_count(self):
        data = dataset([trial()])
        summary = analysis.summarize([self.save(data, "original.json"), self.save(data, "copy.json")])
        self.assertEqual(summary["groups"][0]["n"], 1)
        self.assertEqual(summary["excluded"][0]["reason"], "duplicate source contents")

    def test_pairing_uses_same_source_and_trial_not_unrelated_trial_numbers(self):
        paired = dataset([trial("A", 1, cpu=4), trial("B", 1, cpu=2),
                          trial("A", 2, cpu=10), trial("B", 2, cpu=8)])
        unmatched = dataset([trial("B", 3, cpu=100)])
        summary = analysis.summarize([self.save(paired, "paired.json"), self.save(unmatched, "other.json")])
        pair = summary["paired_differences"][0]
        self.assertEqual(pair["matched_pairs"], 2)
        self.assertEqual(pair["metrics"]["tree_cpu_ms_per_s"]["difference"]["median"], -20)
        self.assertEqual(pair["metrics"]["tree_cpu_ms_per_s"]["percent_change_from_left"]["median"], -35)
        self.assertIsNone(pair["metrics"]["native_sampling_messages"]["percent_change_from_left"]["median"])
        self.assertIsNone(pair["metrics"]["tree_private_mib_growth_per_min"]["percent_change_from_left"]["median"])

    def test_protocol_geometry_state_and_message_differences_are_visible(self):
        left, right = trial("A"), trial("B")
        right["window_geometry"]["client_pixel_width"] = 1000
        right["ready"]["state"] = "hidden"
        right["native_interaction"] = {"kind": "scroll", "messages": 60}
        right["window_state_counts"] = {"hidden": 10}
        summary = analysis.summarize([self.save(dataset([left, right]))])
        self.assertEqual(set(summary["protocol_differences"][0]["differences"]),
                         {"geometry", "ready_state", "native_interaction"})
        self.assertTrue(any("client width" in note for note in summary["groups"][1]["notes"]))
        self.assertEqual(summary["groups"][1]["metrics"]["native_sampling_messages_per_s"]["median"], 4)

    def test_churn_needs_observed_transitions_and_csv_is_read_only(self):
        row = trial()
        row.pop("window_state_counts")
        row["csv"] = "trial.csv"
        data = dataset([row])
        data["config"]["scenarios"][0]["allow_window_changes"] = True
        source = self.save(data)
        csv_path = source.parent / row["csv"]
        csv_path.write_text("role,window_state\nmain,visible\nmain,visible\nhelper,hidden\n", encoding="utf-8")
        group = analysis.summarize([source])["groups"][0]
        self.assertEqual(group["trials"][0]["window_state_counts"], {"visible": 2})
        self.assertTrue(any("churn lacks" in note for note in group["notes"]))
        self.assertEqual(csv_path.read_text(encoding="utf-8").splitlines()[1], "main,visible")

    def test_cli_writes_summary_artifacts_and_preserves_raw_evidence(self):
        source = self.save(dataset([trial()]))
        before = hashlib.sha256(source.read_bytes()).hexdigest()
        output = self.directory / "analysis"
        subprocess.run([sys.executable, str(Path(analysis.__file__)), str(source), "--out", str(output)],
                       check=True, capture_output=True, text=True)
        summary = json.loads((output / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["groups"][0]["n"], 1)
        self.assertIn("unavailable (n=0)", (output / "tables.md").read_text(encoding="utf-8"))
        self.assertEqual(hashlib.sha256(source.read_bytes()).hexdigest(), before)


if __name__ == "__main__":
    unittest.main()
