"""Synthetic source-integrity, pairing, and metric-semantics checks."""
import copy
import csv
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

import summarize_scheduler_resources as analysis


def trial(framework, repeat=1, profile="process-priority", cpu=2, settings="same settings"):
    proof = {phase: {"status": "verified", "errors": []} for phase in ("prepare", "ready", "finished")}
    proof["ready"]["active_action_verified"] = profile != "disabled"
    proof["finished"]["restoration_verified"] = True
    contract = {"profile": profile, "settings_toml": settings, "worker_running": profile != "disabled"}
    scope = {"samples": 61, "cpu_percent_one_core_weighted": cpu,
             "cpu_percent_one_core": {"p50": 99, "p95": 101},
             "cycles_weighted_per_s": cpu * 1000, "cycles": {"p50": 1e20},
             "user_s_weighted_per_s": .01, "kernel_s_weighted_per_s": .02,
             "private_bytes": {"p50": 10 * 2**20, "p95": 11 * 2**20},
             "private_working_set_bytes": {"p50": 4 * 2**20, "p95": 5 * 2**20},
             "private_bytes_slope_per_minute": -2**20,
             "handles": {"p50": 50, "p95": 52}}
    measurement = {"target": framework, "scenario": "background", "exit_code": 0, "errors": [],
                   "ready": {"scheduler_benchmark": contract},
                   "finished": {"scheduler_benchmark": dict(contract, worker_running=False,
                                                            restoration_succeeded=True),
                                "worker_running_at_measurement_end": profile != "disabled",
                                "cleanup_errors": [], "runtime_worker_error": None,
                                "loop_result": {"Ok": "ok"}, "cleanup_ms": 10},
                   "scheduler_native_verification": proof, "scope_summaries": {"combined": scope},
                   "sampling_elapsed_s": 60, "duration_s_requested": 60, "warmup_s": 15,
                   "startup_main_counters": {"cycles": 123, "user_s": .01, "kernel_s": .02}}
    return {"framework": framework, "repeat": repeat, "profile": profile,
            "restoration_verified": True, "workers_stopped": True,
            "worker_cleanup_errors": [], "worker_exit_codes": {"1": 0}, "measurement": measurement,
            "workload_observation": {"window": "whole collector trial; not sampling window",
                                     "combined_cpu_percent_one_core": 200}}


class SchedulerSummaryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)

    def save(self, name, trials, mode="idle", duration=60, orchestrator="one", failure=False,
             power_guid="381b4222-f694-41f0-9685-ff5bb260df2e"):
        folder = self.root / name
        folder.mkdir()
        protocol = {"warmup_s": 15, "duration_s": duration, "interval_s": 1,
                    "worker_count": 4, "worker_mode": mode,
                    "memory_ex2_enabled": True, "gpu_enabled": False,
                    "framework_builds": {key: {"sha256": key} for key in ("wind-ui", "iced")},
                    "collector": {"sha256": "collector"}, "verifier": {"sha256": "verifier"},
                    "worker_build": {"sha256": "worker"}, "orchestrator": {"sha256": orchestrator}}
        results = copy.deepcopy(trials)
        for index, row in enumerate(results):
            collector_dir = folder / str(index)
            collector_dir.mkdir()
            measurement = row["measurement"]
            measurement["csv"] = "raw.csv"
            (collector_dir / "raw.csv").write_text("pid,cycles\n1,100\n")
            (collector_dir / "measurements.json").write_text(json.dumps({"results": [measurement],
                "sampler_sha256": protocol["collector"]["sha256"],
                "config": {"targets": [{"name": row["framework"],
                    "sha256": protocol["framework_builds"][row["framework"]]["sha256"]}]},
                "protocol": {key: protocol[key] for key in ("warmup_s", "duration_s", "interval_s",
                                                            "memory_ex2_enabled", "gpu_enabled")}}))
            row["collector_json"] = f"{index}/measurements.json"
            measurement["scheduler_settings_sha256"] = hashlib.sha256(
                measurement["ready"]["scheduler_benchmark"]["settings_toml"].encode()).hexdigest()
        attempts = [{"state": "failed", "profile": "not-accepted", "error": "foreground unavailable"}] if failure else []
        path = folder / "scheduler-measurements.json"
        path.write_text(json.dumps({"protocol": protocol, "results": results, "attempts": attempts,
                                    "original_power_guid": power_guid}))
        return path

    def test_weighted_rates_resident_commit_and_p95_preserve_units(self):
        source = self.save("matrix", [trial("wind-ui", cpu=2), trial("iced", cpu=3)])
        summary = analysis.summarize([source])
        values = summary["trials"][0]["scopes"]["combined"]
        self.assertEqual(values["cycles_weighted_per_s"], 2000)
        self.assertEqual(values["cpu_percent_one_core_weighted"], 2)
        self.assertEqual(values["cpu_percent_one_core_p95"], 101)
        self.assertEqual(values["user_cpu_ms_per_s"], 10)
        self.assertEqual(values["kernel_cpu_ms_per_s"], 20)
        self.assertEqual(values["private_commit_mib_p50"], 10)
        self.assertEqual(values["private_resident_mib_p95"], 5)
        self.assertEqual(values["private_commit_mib_slope_per_minute"], -1)
        self.assertNotIn("cycles", values)
        self.assertIsNone(values["gpu_shared_mib_p50"])
        self.assertEqual(summary["pairs"][0]["wind_minus_iced"]["combined"]["cpu_percent_one_core_weighted"], -1)

    def test_incomplete_framework_observations_cannot_pair_across_sources(self):
        first = self.save("a", [trial("wind-ui")])
        second = self.save("b", [trial("iced")])
        summary = analysis.summarize([first, second])
        self.assertEqual(len(summary["trials"]), 2)
        self.assertEqual(len(summary["unpaired_trial_ids"]), 2)
        self.assertEqual(summary["pairs"], [])
        self.assertEqual(summary["groups"], [])

    def test_setting_hash_mismatch_cannot_be_compared(self):
        source = self.save("a", [trial("wind-ui", settings="first"), trial("iced", settings="second")])
        with self.assertRaisesRegex(ValueError, "Paired settings differ"):
            analysis.summarize([source])

    def test_different_settings_or_original_power_plan_cannot_pool_across_cohorts(self):
        a = self.save("a", [trial("wind-ui"), trial("iced")])
        b = self.save("b", [trial("wind-ui", settings="other settings"),
                             trial("iced", settings="other settings")])
        c = self.save("c", [trial("wind-ui"), trial("iced")],
                      power_guid="8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c")
        summary = analysis.summarize([a, b, c])
        self.assertEqual(len(summary["pairs"]), 3)
        self.assertEqual(len(summary["groups"]), 3)
        self.assertEqual({item["paired_repeats"] for item in summary["groups"]}, {1})
        self.assertEqual(len({item["settings_sha256"] for item in summary["groups"]}), 2)
        self.assertEqual(len({item["original_power_guid"] for item in summary["groups"]}), 2)

    def test_consumed_worker_errors_in_original_evidence_cannot_be_accepted(self):
        for index, (location, field) in enumerate((("ready", "runtime_status"),
                                                  ("finished", "measurement_runtime_status"))):
            row = trial("wind-ui")
            row["measurement"][location][field] = {"worker_error": "worker failed before cleanup consumed the error"}
            # Both source payloads contain the error, so equality alone passes.
            source = self.save(str(index), [row])
            with self.assertRaisesRegex(ValueError, "Runtime worker error"):
                analysis.summarize([source])

    def test_measurement_end_worker_state_is_profile_specific_and_required(self):
        for index, profile in enumerate(("disabled", "process-priority")):
            row = trial("wind-ui", profile=profile)
            row["measurement"]["finished"]["worker_running_at_measurement_end"] = profile == "disabled"
            with self.assertRaisesRegex(ValueError, "state changed during measurement"):
                analysis.summarize([self.save(str(index), [row])])
        missing = trial("wind-ui")
        del missing["measurement"]["finished"]["worker_running_at_measurement_end"]
        with self.assertRaisesRegex(ValueError, "state changed during measurement"):
            analysis.summarize([self.save("missing", [missing])])

    def test_paired_deltas_are_aggregated_before_medians(self):
        rows = []
        for repeat, a, b in ((1, 1, 2), (2, 5, 100), (3, 99, 98)):
            rows.extend([trial("wind-ui", repeat, cpu=a), trial("iced", repeat, cpu=b)])
        summary = analysis.summarize([self.save("a", rows)])
        group = summary["groups"][0]
        self.assertEqual(group["paired_repeats"], 3)
        self.assertEqual(group["wind_minus_iced"]["combined"]["cpu_percent_one_core_weighted"]["median"], -1)
        self.assertEqual(group["frameworks"]["wind-ui"]["combined"]["cpu_percent_one_core_weighted"]["median"], 5)

    def test_compatible_orchestrator_only_fix_keeps_cohorts_and_pairs(self):
        a = self.save("a", [trial("wind-ui"), trial("iced")], orchestrator="old")
        b = self.save("b", [trial("wind-ui"), trial("iced")], orchestrator="fixed")
        summary = analysis.summarize([a, b])
        self.assertEqual(len(summary["sources"]), 2)
        self.assertEqual(len(summary["groups"]), 1)
        self.assertEqual(summary["groups"][0]["cohorts"], ["a", "b"])
        self.assertEqual(summary["groups"][0]["paired_repeats"], 2)

    def test_load_or_duration_change_keeps_comparison_groups_separate(self):
        a = self.save("a", [trial("wind-ui"), trial("iced")])
        b = self.save("b", [trial("wind-ui"), trial("iced")], mode="busy")
        c = self.save("c", [trial("wind-ui"), trial("iced")], duration=180)
        summary = analysis.summarize([a, b, c])
        self.assertEqual(len(summary["groups"]), 3)

    def test_explicit_foreground_unavailable_environment_is_never_pooled(self):
        a = self.save("healthy", [trial("wind-ui"), trial("iced")])
        b = self.save("unavailable", [trial("wind-ui"), trial("iced")])
        summary = analysis.summarize([a, b], {"healthy": "foreground_present",
                                             "unavailable": "foreground_unavailable"})
        self.assertEqual(len(summary["groups"]), 2)
        self.assertEqual({item["environment_tag"] for item in summary["groups"]},
                         {"foreground_present", "foreground_unavailable"})
        with self.assertRaisesRegex(ValueError, "absent"):
            analysis.summarize([a], {"wrong-name": "foreground_unavailable"})

    def test_failed_attempts_are_visible_but_never_ranked(self):
        source = self.save("a", [trial("wind-ui"), trial("iced")], failure=True)
        summary = analysis.summarize([source])
        self.assertEqual(len(summary["sources"][0]["other_attempts"]), 1)
        self.assertEqual(len(summary["trials"]), 2)
        self.assertEqual(len(summary["pairs"]), 1)

    def test_explicit_environment_transition_exclusions_preserve_raw_acceptance(self):
        rows = [trial("wind-ui", profile="by-time"), trial("iced", profile="by-time")]
        source = self.save("transition", rows)
        exclusions = {f"transition:1:by-time:{framework}": "Desktop unlock timing overlaps this pair"
                      for framework in ("wind-ui", "iced")}
        summary = analysis.summarize([source], {"transition": "foreground_unavailable_then_unlocked"}, exclusions)
        self.assertEqual(summary["raw_accepted_trials_count"], 2)
        self.assertEqual(summary["comparison_eligible_trials_count"], 0)
        self.assertEqual(summary["trials"], [])
        self.assertEqual(summary["pairs"], [])
        self.assertEqual(summary["groups"], [])
        self.assertEqual(len(summary["excluded_trials"]), 2)
        self.assertTrue(all(row["coverage"]["native_restoration_verified"] for row in summary["excluded_trials"]))
        self.assertEqual(summary["sources"][0]["accepted_count"], 2)
        self.assertEqual(summary["sources"][0]["excluded_accepted_count"], 2)
        output = self.root / "export"
        analysis.export(summary, output)
        with (output / "excluded-trials.csv").open(encoding="utf-8-sig", newline="") as stream:
            exported = list(csv.DictReader(stream))
        self.assertEqual(len(exported), 2 * len(analysis.SCOPES))
        self.assertEqual(exported[0]["exclusion_reason"], "Desktop unlock timing overlaps this pair")
        self.assertFalse(json.loads(source.read_text()).get("excluded_trials"))

    def test_one_excluded_counterpart_cannot_be_replaced_from_another_cohort(self):
        a = self.save("a", [trial("wind-ui"), trial("iced")])
        b = self.save("b", [trial("iced")])
        summary = analysis.summarize([a, b], exclude_trials={"a:1:process-priority:iced": "Transition"})
        self.assertEqual(summary["pairs"], [])
        self.assertEqual(len(summary["unpaired_trial_ids"]), 2)
        self.assertEqual(len(summary["excluded_trials"]), 1)

    def test_exclusion_typo_cannot_silently_leave_a_trial_in_comparison(self):
        source = self.save("a", [trial("wind-ui"), trial("iced")])
        with self.assertRaisesRegex(ValueError, "exactly one accepted trial"):
            analysis.summarize([source], exclude_trials={"a:2:process-priority:iced": "Wrong repeat"})

    def test_broken_native_restoration_cannot_enter_summary(self):
        row = trial("wind-ui")
        row["measurement"]["scheduler_native_verification"]["finished"]["restoration_verified"] = False
        with self.assertRaisesRegex(ValueError, "native restoration"):
            analysis.summarize([self.save("a", [row])])

    def test_collector_payload_change_cannot_hide_behind_cached_summary(self):
        source = self.save("a", [trial("wind-ui")])
        data = json.loads(source.read_text())
        data["results"][0]["measurement"]["scope_summaries"]["combined"]["cycles_weighted_per_s"] = 999
        source.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, "original collector"):
            analysis.summarize([source])

    def test_original_collector_provenance_must_match_declared_binary_and_sampler(self):
        source = self.save("a", [trial("wind-ui")])
        data = json.loads(source.read_text())
        row = data["results"][0]
        collector_path = source.parent / row["collector_json"]
        collector = json.loads(collector_path.read_text())
        analysis.validate_collector_provenance(collector, row, data["protocol"])
        changed = copy.deepcopy(collector)
        changed["config"]["targets"][0]["sha256"] = "other binary"
        with self.assertRaisesRegex(ValueError, "binary hash"):
            analysis.validate_collector_provenance(changed, row, data["protocol"])
        changed = copy.deepcopy(collector)
        changed["config"]["targets"][0]["name"] = "iced"
        with self.assertRaisesRegex(ValueError, "target/binary"):
            analysis.validate_collector_provenance(changed, row, data["protocol"])
        changed = copy.deepcopy(collector)
        changed["results"][0]["sampler_sha256"] = "different row sampler"
        with self.assertRaisesRegex(ValueError, "sampler hash"):
            analysis.validate_collector_provenance(changed, row, data["protocol"])
        changed = copy.deepcopy(collector)
        del changed["sampler_sha256"]
        with self.assertRaisesRegex(ValueError, "sampler hash"):
            analysis.validate_collector_provenance(changed, row, data["protocol"])
        # A declared source hash cannot silently override the collector file.
        data["protocol"]["framework_builds"]["wind-ui"]["sha256"] = "replacement"
        source.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, "binary hash"):
            analysis.summarize([source])

    def test_original_sampling_protocol_requires_every_matching_counter_and_duration(self):
        source = self.save("a", [trial("wind-ui")])
        data = json.loads(source.read_text())
        row = data["results"][0]
        original = json.loads((source.parent / row["collector_json"]).read_text())
        for key in ("warmup_s", "duration_s", "interval_s", "memory_ex2_enabled", "gpu_enabled"):
            for missing in (False, True):
                changed = copy.deepcopy(original)
                if missing:
                    del changed["protocol"][key]
                else:
                    value = changed["protocol"][key]
                    changed["protocol"][key] = not value if type(value) is bool else value + 1
                with self.subTest(key=key, missing=missing), self.assertRaisesRegex(ValueError, key):
                    analysis.validate_collector_provenance(changed, row, data["protocol"])

    def test_status_only_and_whole_trial_target_cpu_keep_their_labels(self):
        row = trial("wind-ui")
        row["measurement"]["scheduler_native_verification"]["ready"].update(
            status="status_only", active_action_verified=False)
        summary = analysis.summarize([self.save("a", [row])])
        output = summary["trials"][0]
        self.assertFalse(output["coverage"]["active_action_verified"])
        self.assertEqual(output["coverage"]["ready_status"], "status_only")
        self.assertIn("not sampling", output["whole_trial_external_workers"]["window"])
        self.assertNotIn("combined_cpu_percent_one_core", output["scopes"]["combined"])

    def test_export_csv_matches_json_values_and_has_scope_rows(self):
        summary = analysis.summarize([self.save("a", [trial("wind-ui"), trial("iced")])])
        directory = self.root / "export"
        analysis.export(summary, directory)
        with (directory / "trials.csv").open(encoding="utf-8-sig", newline="") as source:
            rows = list(csv.DictReader(source))
        self.assertEqual(len(rows), 2 * len(analysis.SCOPES))
        first = rows[0]
        self.assertEqual(first["scope"], "combined")
        self.assertEqual(float(first["cycles_weighted_per_s"]), 2000)
        self.assertEqual(first["gpu_shared_mib_p50"], "")
        self.assertTrue((directory / "pairs-wind-minus-iced.csv").is_file())

    def test_duplicate_sources_and_nonfinite_values_are_not_silently_counted(self):
        source = self.save("a", [trial("wind-ui")])
        with self.assertRaisesRegex(ValueError, "Duplicate source"):
            analysis.summarize([source, source])
        self.assertEqual(analysis.aggregate([None, float("nan"), float("inf"), False, 0]),
                         {"n": 1, "missing": 4, "median": 0, "min": 0, "max": 0})


if __name__ == "__main__":
    unittest.main()
