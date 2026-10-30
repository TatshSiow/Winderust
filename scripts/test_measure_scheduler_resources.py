"""Scheduler orchestration checks; no fixtures, native probes, or workers launch."""
import copy
import errno
import json
from pathlib import Path
from types import SimpleNamespace
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

import measure_scheduler_resources as runner


GUID = "381b4222-f694-41f0-9685-ff5bb260df2e"


def collected_row():
    evidence = {"profile": "disabled", "settings_toml": "[general]\nenabled = false\n"}
    return {"target": "wind-ui", "scenario": "background", "csv": "samples.csv",
            "ready": {"scheduler_benchmark": dict(evidence)},
            "finished": {"scheduler_benchmark": dict(evidence)},
            "scheduler_native_verification": {"finished": {"restoration_verified": True}},
            "scope_summaries": {"combined": {"cycles_weighted_per_s": 123}}}


class FakeProcess:
    def __init__(self, pid):
        self.pid, self.returncode = pid, None

    def poll(self):
        return self.returncode

    def wait(self, timeout=None):
        self.returncode = 0
        return 0


class SchedulerRunnerTests(unittest.TestCase):
    def test_atomic_write_retries_transient_replace_errors_without_reserializing(self):
        for winerror, error_number in ((5, errno.EACCES), (32, errno.EACCES), (33, errno.EACCES),
                                       (None, errno.EACCES), (None, errno.EPERM)):
            error = PermissionError(error_number, "busy")
            if winerror is not None:
                error.winerror = winerror
            now, sleeps = [0.0], []

            def sleep(duration):
                sleeps.append(duration)
                now[0] += duration

            with self.subTest(winerror=winerror, errno=error_number), tempfile.TemporaryDirectory() as directory:
                target = Path(directory) / "summary.json"
                target.write_text("previous", encoding="utf-8")
                original_replace, calls = Path.replace, []

                def replace(source, destination):
                    calls.append((source, destination))
                    if len(calls) == 1:
                        raise error
                    return original_replace(source, destination)

                with patch.object(Path, "replace", replace), \
                        patch.object(runner.time, "monotonic", side_effect=lambda: now[0]), \
                        patch.object(runner.time, "sleep", side_effect=sleep), \
                        patch.object(runner.json, "dumps", wraps=json.dumps) as serialize:
                    runner.write_json(target, {"status": "complete"})
                    serialize.assert_called_once()
                self.assertEqual(json.loads(target.read_text()), {"status": "complete"})
                self.assertFalse(target.with_suffix(".json.tmp").exists())
                self.assertEqual(len(calls), 2)
                self.assertEqual(sleeps, [0.02])

    def test_atomic_write_persistent_replace_denial_preserves_target_and_temp(self):
        error = PermissionError(errno.EACCES, "denied")
        error.winerror = 5
        now, sleeps = [0.0], []

        def sleep(duration):
            sleeps.append(duration)
            now[0] += duration

        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "summary.json"
            target.write_text("previous", encoding="utf-8")
            with patch.object(Path, "replace", side_effect=error) as replace, \
                    patch.object(runner.time, "monotonic", side_effect=lambda: now[0]), \
                    patch.object(runner.time, "sleep", side_effect=sleep):
                with self.assertRaises(PermissionError) as raised:
                    runner.write_json(target, {"status": "complete"})
            self.assertIs(raised.exception, error)
            self.assertEqual(target.read_text(), "previous")
            self.assertEqual(json.loads(target.with_suffix(".json.tmp").read_text()), {"status": "complete"})
            self.assertAlmostEqual(now[0], 0.25)
            self.assertTrue(sleeps and all(0 < duration <= 0.02 for duration in sleeps))
            self.assertLessEqual(replace.call_count, 13)

    def test_atomic_write_unrelated_replace_errors_are_not_retried(self):
        unsupported = PermissionError(errno.EACCES, "unrelated Win32 error")
        unsupported.winerror = 87
        for error in (unsupported, FileNotFoundError(errno.ENOENT, "missing"), OSError(errno.EIO, "I/O failure")):
            with self.subTest(error=error), tempfile.TemporaryDirectory() as directory:
                target = Path(directory) / "summary.json"
                with patch.object(Path, "replace", side_effect=error) as replace, \
                        patch.object(runner.time, "sleep") as sleep:
                    with self.assertRaises(OSError) as raised:
                        runner.write_json(target, {"status": "complete"})
                self.assertIs(raised.exception, error)
                replace.assert_called_once_with(target)
                sleep.assert_not_called()

    def test_trial_order_counterbalances_frameworks_and_rotates_profiles(self):
        order = list(runner.trial_order(["wind-ui", "iced"], ["disabled", "combined"], 2))
        self.assertEqual(order, [(1, "wind-ui", "disabled"), (1, "iced", "disabled"),
            (1, "wind-ui", "combined"), (1, "iced", "combined"),
            (2, "iced", "combined"), (2, "wind-ui", "combined"),
            (2, "iced", "disabled"), (2, "wind-ui", "disabled")])

    def test_bound_config_declares_exact_scope_and_independent_verifier(self):
        root = Path.cwd()
        for framework in runner.FRAMEWORKS:
            for profile in ("disabled", "combined", "enabled-no-match"):
                config = runner.build_config(framework, root / "winderust.exe", profile,
                    root / "WinderustSchedulerBench-test.exe", GUID, root / "targets.json", root / "verify.py")
                target, scenario = config["targets"][0], config["scenarios"][0]
                contract = scenario["automation_contract"]
                self.assertEqual(contract["ready"]["automation_enabled"], profile != "disabled")
                self.assertEqual(contract["finished"]["worker_running_at_measurement_end"], profile != "disabled")
                self.assertEqual(contract["ready"]["scheduler_benchmark"]["enabled_features"], runner.PROFILE_FEATURES[profile])
                self.assertEqual(target["window_states"]["background"], runner.FRAMEWORKS[framework]["state"])
                self.assertEqual(target["renderer"], runner.FRAMEWORKS[framework]["renderer"])
                self.assertIn("--framework-benchmark-current-plan", target["args"])
                self.assertIn("{phase}", contract["native_verifier"])

    def test_explicit_build_renderer_label_reaches_collector_without_changing_contract(self):
        root = Path.cwd()
        for framework in runner.FRAMEWORKS:
            arguments = (framework, root / "winderust.exe", "disabled",
                         root / "WinderustSchedulerBench-test.exe", GUID,
                         root / "targets.json", root / "verify.py")
            original = runner.build_config(*arguments)
            explicit = runner.build_config(*arguments, renderer="current build / tiny-skia software")
            self.assertEqual(explicit["targets"][0].pop("renderer"), "current build / tiny-skia software")
            original["targets"][0].pop("renderer")
            self.assertEqual(explicit, original)

    def test_collector_command_uses_low_overhead_explicit_safe_protocol(self):
        options = SimpleNamespace(warmup=15, duration=60, interval=1, startup_timeout=45,
                                  shutdown_timeout=30, system_energy=False)
        command = runner.collector_command(Path("config.json"), Path("out"), options)
        for flag in ("--no-gpu", "--memory-ex2", "--allow-enabled-automation"):
            self.assertIn(flag, command)
        self.assertNotIn("--thread-context-switches", command)
        self.assertNotIn("--system-energy", command)
        self.assertEqual(command[command.index("--trials") + 1], "1")
        self.assertEqual(command[command.index("--interval") + 1], "1")
        options.system_energy = True
        self.assertEqual(runner.collector_command(Path("config.json"), Path("out"), options),
                         command + ["--system-energy"])

    def test_system_energy_option_is_recorded_in_protocol_before_native_setup(self):
        class ProtocolCaptured(Exception):
            pass

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for flags, enabled in (([], False), (["--system-energy"], True)):
                arguments = ["scheduler", "--out", str(root / "capture"),
                             "--profiles", "enabled-no-match"] + flags
                with self.subTest(enabled=enabled), patch.object(runner, "BENCHMARK", root), \
                        patch.object(runner.sys, "argv", arguments), \
                        patch.object(runner, "dedicated_executable", side_effect=lambda path, **_: path), \
                        patch.object(runner, "fingerprint", return_value={"sha256": "fixed"}), \
                        patch.object(runner, "load_summary", side_effect=ProtocolCaptured) as load:
                    with self.assertRaises(ProtocolCaptured):
                        runner.main()
                    self.assertIs(load.call_args.args[1]["system_energy_enabled"], enabled)

    def test_renderer_flags_are_recorded_in_protocol_before_native_setup(self):
        class ProtocolCaptured(Exception):
            pass

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for flags, expected in (([], {name: spec["renderer"] for name, spec in runner.FRAMEWORKS.items()}),
                    (["--wind-renderer", "Wind UI 0.28.0 / tiny-skia software", "--iced-renderer", "Iced 0.14.0 / tiny-skia software"],
                     {"wind-ui": "Wind UI 0.28.0 / tiny-skia software", "iced": "Iced 0.14.0 / tiny-skia software"})):
                arguments = ["scheduler", "--out", str(root / "capture"), "--profiles", "disabled"] + flags
                with self.subTest(flags=flags), patch.object(runner, "BENCHMARK", root), \
                        patch.object(runner.sys, "argv", arguments), \
                        patch.object(runner, "dedicated_executable", side_effect=lambda path, **_: path), \
                        patch.object(runner, "fingerprint", return_value={"sha256": "fixed"}), \
                        patch.object(runner, "load_summary", side_effect=ProtocolCaptured) as load:
                    with self.assertRaises(ProtocolCaptured):
                        runner.main()
                    self.assertEqual(load.call_args.args[1]["framework_renderers"], expected)

    def test_executables_cannot_point_at_production_or_arbitrary_targets(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            isolated = root / "scheduler-benchmark"
            isolated.mkdir()
            worker = isolated / "WinderustSchedulerBench-test.exe"
            worker.write_bytes(b"worker")
            production = root / "winderust.exe"
            production.write_bytes(b"production")
            wrong = isolated / "unrelated.exe"
            wrong.write_bytes(b"wrong")
            with patch.object(runner, "BENCHMARK", isolated):
                self.assertEqual(runner.dedicated_executable(worker, True), worker.resolve())
                for path, is_worker in ((production, False), (wrong, True), (worker, False)):
                    with self.subTest(path=path), self.assertRaises(ValueError):
                        runner.dedicated_executable(path, is_worker)

    def test_accepted_collector_requires_restore_proof_raw_data_and_consistent_settings(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "measurements.json"
            row = collected_row()
            (root / "samples.csv").write_text("raw", encoding="utf-8")
            runner.write_json(path, {"results": [row]})
            accepted = runner.accepted_collector(path, "wind-ui", "disabled")
            self.assertEqual(len(accepted["scheduler_settings_sha256"]), 64)
            for change in ("restore", "settings", "failure", "profile"):
                value = {"results": [copy.deepcopy(row)]}
                if change == "restore":
                    value["results"][0]["scheduler_native_verification"]["finished"]["restoration_verified"] = False
                elif change == "settings":
                    value["results"][0]["finished"]["scheduler_benchmark"]["settings_toml"] = "changed"
                elif change == "profile":
                    value["results"][0]["ready"]["scheduler_benchmark"]["profile"] = "other"
                else:
                    value["failure"] = {"error": "incomplete"}
                runner.write_json(path, value)
                with self.subTest(change=change), self.assertRaises(RuntimeError):
                    runner.accepted_collector(path, "wind-ui", "disabled")

    def test_resume_preserves_success_and_rejects_protocol_drift_or_missing_raw(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "scheduler-measurements.json"
            protocol = {"worker_mode": "idle", "collector": "fixed", "system_energy_enabled": False,
                        "framework_renderers": {"wind-ui": "Wind UI 0.24.0 / tiny-skia software"}}
            original = runner.load_summary(path, protocol, False)
            (root / "samples.csv").write_text("raw", encoding="utf-8")
            runner.write_json(root / "collector.json", {"results": [collected_row()]})
            record = {"repeat": 1, "framework": "wind-ui", "profile": "disabled",
                      "collector_json": "collector.json", "restoration_verified": True, "workers_stopped": True}
            original["results"].append(record)
            original["attempts"].append({"state": "failed", "error": "preserve me"})
            runner.write_json(path, original)
            resumed = runner.load_summary(path, protocol, True)
            self.assertEqual(resumed["results"], [record])
            self.assertEqual(resumed["attempts"], original["attempts"])
            self.assertEqual(resumed["resume_history"][0]["accepted_trials_preserved"], 1)
            with self.assertRaises(ValueError):
                runner.load_summary(path, {"worker_mode": "busy"}, True)
            with self.assertRaises(ValueError):
                runner.load_summary(path, dict(protocol, system_energy_enabled=True), True)
            with self.assertRaises(ValueError):
                runner.load_summary(path, dict(protocol,
                    framework_renderers={"wind-ui": "Wind UI 0.28.0 / tiny-skia software"}), True)
            with self.assertRaises(ValueError):
                runner.load_summary(path, protocol, False)
            (root / "samples.csv").unlink()
            with self.assertRaises(RuntimeError):
                runner.load_summary(path, protocol, True)

    def test_prior_live_worker_blocks_resume_but_exited_or_reused_pid_does_not(self):
        worker = {"pid": 12, "creation_filetime": 100}
        summary = {"attempts": [{"workers_stopped": False, "workers": [worker]}]}
        kernel = SimpleNamespace(OpenProcess=Mock(return_value=1), WaitForSingleObject=Mock(return_value=258), CloseHandle=Mock())
        native = SimpleNamespace(kernel=kernel)
        capture = Mock(return_value={"creation_filetime": 100})
        with self.assertRaises(RuntimeError):
            runner.assert_previous_workers_exited(summary, native, capture)
        capture.return_value = {"creation_filetime": 200}
        runner.assert_previous_workers_exited(summary, native, capture)
        kernel.WaitForSingleObject.return_value = 0
        capture.reset_mock()
        runner.assert_previous_workers_exited(summary, native, capture)
        capture.assert_not_called()
        self.assertEqual(kernel.CloseHandle.call_count, 3)

    def test_targeted_inventory_blocks_only_dedicated_benchmark_worker_names(self):
        for names, rejected in ((["normal.exe", "WinderustSchedulerBench-other.exe"], True),
                                (["normal.exe", "WinderustSchedulerBench.exe"], False)):
            pending = iter(enumerate(names, 100))

            def next_entry(_, pointer):
                item = next(pending, None)
                if item is None:
                    return 0
                pointer._obj.th32ProcessID, pointer._obj.szExeFile = item
                return 1

            kernel = SimpleNamespace(CreateToolhelp32Snapshot=Mock(return_value=1),
                                     Process32FirstW=Mock(side_effect=next_entry),
                                     Process32NextW=Mock(side_effect=next_entry), CloseHandle=Mock())
            with self.subTest(names=names), patch.object(runner.C, "get_last_error", return_value=18):
                if rejected:
                    with self.assertRaisesRegex(RuntimeError, "PIDs"):
                        runner.assert_no_other_workers(SimpleNamespace(kernel=kernel))
                else:
                    runner.assert_no_other_workers(SimpleNamespace(kernel=kernel))
            kernel.CloseHandle.assert_called_once_with(1)

    def test_worker_readiness_validates_its_owned_pid_and_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            ready = Path(directory) / "ready.json"
            process = FakeProcess(123)
            runner.write_json(ready, {"pid": 123, "mode": "idle"})
            self.assertEqual(runner.wait_for_worker(process, ready, "idle", 1)["pid"], 123)
            with self.assertRaises(RuntimeError):
                runner.wait_for_worker(process, ready, "busy", 1)
            process.returncode = 1
            with self.assertRaises(RuntimeError):
                runner.wait_for_worker(process, ready, "idle", 1)

    def test_worker_readiness_retries_transient_permission_error(self):
        expected = {"pid": 123, "mode": "idle"}
        with patch.object(runner.collector, "read_json", side_effect=[PermissionError("busy"), expected]) as read, \
                patch.object(runner.time, "perf_counter", side_effect=[0, .1, .2]), \
                patch.object(runner.time, "sleep") as sleep:
            self.assertEqual(runner.wait_for_worker(FakeProcess(123), Path("ready.json"), "idle", 1), expected)
            self.assertEqual(read.call_count, 2)
            sleep.assert_called_once_with(.05)

    def test_worker_readiness_permanent_permission_error_still_times_out(self):
        with patch.object(runner.collector, "read_json", side_effect=PermissionError("denied")) as read, \
                patch.object(runner.time, "perf_counter", side_effect=[0, .1, .2, 1]), \
                patch.object(runner.time, "sleep") as sleep:
            with self.assertRaisesRegex(RuntimeError, "readiness timeout"):
                runner.wait_for_worker(FakeProcess(123), Path("ready.json"), "idle", 1)
            self.assertEqual(read.call_count, 2)
            self.assertEqual(sleep.call_count, 2)

    def run_mock_trial(self, root, accepted=True):
        worker = root / "WinderustSchedulerBench-test.exe"
        options = SimpleNamespace(out=root, warmup=15, duration=60, interval=1, startup_timeout=45,
                                  shutdown_timeout=30, worker_timeout=2, worker_mode="idle", worker_hash="a" * 64,
                                  system_energy=False, framework_renderers={"wind-ui": "Wind UI test / tiny-skia software"})
        native = SimpleNamespace(active_power_guid=lambda: GUID)
        attempt = {"directory": "trial", "framework": "wind-ui", "profile": "disabled", "workers": []}
        processes = [FakeProcess(pid) for pid in range(101, 106)]
        capture = lambda _, pid: {"pid": pid, "creation_filetime": 100, "exe": str(worker), "sha256": "a" * 64}
        with patch.object(runner.subprocess, "Popen", side_effect=processes) as launch, \
                patch.object(runner, "wait_for_worker", return_value={"creation_filetime": 100}), \
                patch.object(runner, "capture_workload_counters", return_value={}), \
                patch.object(runner, "accepted_collector", return_value=collected_row()) as accept:
            if not accepted:
                accept.side_effect = RuntimeError("restoration failed")
            if accepted:
                runner.run_trial(attempt, root / "winderust.exe", worker, root / "verify.py",
                                 native, capture, options, GUID, lambda: None, object())
            else:
                with self.assertRaisesRegex(RuntimeError, "restoration failed"):
                    runner.run_trial(attempt, root / "winderust.exe", worker, root / "verify.py",
                                     native, capture, options, GUID, lambda: None, object())
            calls = launch.call_args_list
        return attempt, calls

    def test_four_sibling_workers_stop_only_after_collector_restoration(self):
        with tempfile.TemporaryDirectory() as directory:
            attempt, calls = self.run_mock_trial(Path(directory))
            self.assertEqual(len(calls), 5)
            self.assertEqual(len(attempt["workers"]), 4)
            self.assertIs(attempt["restoration_verified"], True)
            self.assertIs(attempt["workers_stopped"], True)
            self.assertEqual(len(attempt["worker_exit_codes"]), 4)
            config = json.loads((Path(directory) / "trial/collector-config.json").read_text(encoding="utf-8"))
            self.assertEqual(config["targets"][0]["renderer"], "Wind UI test / tiny-skia software")
            for worker in attempt["workers"]:
                self.assertTrue(Path(worker["stop"]).exists())

    def test_failed_restore_retains_target_identities_without_stopping_or_killing(self):
        with tempfile.TemporaryDirectory() as directory:
            attempt, calls = self.run_mock_trial(Path(directory), accepted=False)
            self.assertEqual(len(calls), 5)
            self.assertIs(attempt["workers_stopped"], False)
            self.assertEqual(len(attempt["workers_left_running"]), 4)
            for worker in attempt["workers"]:
                self.assertFalse(Path(worker["stop"]).exists())

    def test_post_launch_persist_failure_waits_for_collector_and_retains_workers(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            worker = root / "WinderustSchedulerBench-test.exe"
            options = SimpleNamespace(out=root, warmup=15, duration=60, interval=1, startup_timeout=45,
                                      shutdown_timeout=30, worker_timeout=2, worker_mode="idle", worker_hash="a" * 64,
                                      system_energy=False, framework_renderers={"wind-ui": "test renderer"})
            attempt = {"directory": "trial", "framework": "wind-ui", "profile": "disabled", "workers": []}
            processes = [FakeProcess(pid) for pid in range(101, 106)]
            child = processes[-1]
            child.wait = Mock(wraps=child.wait)
            error = PermissionError(errno.EACCES, "summary denied after collector launch")
            failed = [False]

            def persist():
                if "collector_pid" in attempt:
                    if not failed[0]:
                        failed[0] = True
                        raise error
                    child.wait.assert_called_once_with()

            capture = lambda _, pid: {"pid": pid, "creation_filetime": 100, "exe": str(worker), "sha256": "a" * 64}
            with patch.object(runner.subprocess, "Popen", side_effect=processes), \
                    patch.object(runner, "wait_for_worker", return_value={"creation_filetime": 100}), \
                    patch.object(runner, "capture_workload_counters", return_value={}), \
                    patch.object(runner, "accepted_collector") as accept, \
                    patch.object(runner, "stop_workers") as stop:
                with self.assertRaises(PermissionError) as raised:
                    runner.run_trial(attempt, root / "winderust.exe", worker, root / "verify.py",
                                     SimpleNamespace(active_power_guid=lambda: GUID), capture,
                                     options, GUID, persist, object())
            self.assertIs(raised.exception, error)
            child.wait.assert_called_once_with()
            self.assertEqual(child.returncode, 0)
            accept.assert_not_called()
            stop.assert_not_called()
            self.assertIs(attempt["workers_stopped"], False)
            self.assertEqual(len(attempt["workers_left_running"]), 4)
            for entry in attempt["workers"]:
                self.assertFalse(Path(entry["stop"]).exists())

    def test_cooperative_shutdown_timeout_never_terminates_worker(self):
        with tempfile.TemporaryDirectory() as directory:
            process = Mock(pid=123)
            process.wait.side_effect = subprocess.TimeoutExpired("worker", 1)
            stop = Path(directory) / "stop"
            exits, errors = runner.stop_workers([(process, {"stop": str(stop)})], .01)
            self.assertEqual(exits, {})
            self.assertEqual(len(errors), 1)
            self.assertTrue(stop.exists())
            process.kill.assert_not_called()
            process.terminate.assert_not_called()

    def test_external_worker_cpu_cycles_and_time_use_whole_trial_intervals(self):
        before = {"10": {"created_filetime": 100, "elapsed_s": 1, "cycles": 100,
                          "user_s": 1, "kernel_s": .25}}
        after = {"10": {"created_filetime": 100, "elapsed_s": 5, "cycles": 900,
                         "user_s": 2, "kernel_s": 1.25}}
        observed = runner.workload_observation(before, after)
        self.assertEqual(observed["total_cycles"], 800)
        self.assertEqual(observed["combined_cycles_per_s"], 200)
        self.assertEqual(observed["total_cpu_ms"], 2000)
        self.assertEqual(observed["combined_cpu_percent_one_core"], 50)
        self.assertIn("not_resource_sampling_window", observed["window"])
        self.assertEqual(observed["per_worker"]["10"]["interval_s"], 4)
        after["10"]["created_filetime"] = 200
        with self.assertRaises(RuntimeError):
            runner.workload_observation(before, after)

    def test_external_counter_capture_validates_creation_and_closes_handle(self):
        worker = {"pid": 10, "creation_filetime": 100}
        tracked = SimpleNamespace(created=100, alive=lambda: True, close=Mock(),
            lifetime=lambda: {"created_filetime": 100, "exited_filetime": 0,
                              "cycles": 10, "user_s": 1, "kernel_s": .1})
        with patch.object(runner.collector, "Process", return_value=tracked):
            counters = runner.capture_workload_counters([(None, worker)], object())
            self.assertEqual(counters["10"]["cycles"], 10)
            tracked.close.assert_called_once()
            tracked.created = 200
            with self.assertRaises(RuntimeError):
                runner.capture_workload_counters([(None, worker)], object())
            self.assertEqual(tracked.close.call_count, 2)


if __name__ == "__main__":
    unittest.main()
