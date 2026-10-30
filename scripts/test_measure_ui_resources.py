"""Checks metric semantics without benchmarking or touching a user's app."""
import os
import errno
import subprocess
import sys
import time
import unittest
from types import SimpleNamespace
from unittest.mock import Mock, patch
import copy
import ctypes as C
import csv
import json
from pathlib import Path
import tempfile

import measure_ui_resources as metrics


def scheduler_contract():
    common = {"schema_version": 1, "profile": "priority-only",
              "scope": "fixture_owned_processes", "settings_io": "in_memory",
              "cross_session_control": False,
              "target_executable": str(Path.cwd() / "fixture-workload.exe"),
              "enabled_features": ["CPU Scheduler"]}
    return {"version": 1, "profile": "priority-only",
            "ready": {"automation_enabled": True,
                      "scheduler_benchmark": dict(common, worker_running=True)},
            "finished": {"automation_enabled": True,
                         "worker_running_at_measurement_end": True,
                         "scheduler_benchmark": dict(common, worker_running=False,
                                                     restoration_succeeded=True)}}


class FakeThreadApi:
    """Only the documented thread calls, with explicit ownership and allocations."""
    def __init__(self):
        self.threads = {7: {"owner": 101, "created": 110, "cycles": 100, "description": "worker"}}
        self.entries, self.snapshot_error, self.error = None, None, 0
        self.opened, self.closed, self.handles, self.allocations, self.freed = [], [], {}, {}, []
        self.owners, self.now, self.clock = None, 1, 0
        self.kernel = SimpleNamespace(
            OpenThread=self.open_thread, GetProcessIdOfThread=self.owner,
            GetThreadTimes=self.times, GetExitCodeThread=self.exit_code,
            QueryThreadCycleTime=self.cycles,
            GetThreadDescription=self.describe, LocalFree=self.free)

    def thread_snapshot(self, owners):
        self.owners = owners
        if self.snapshot_error:
            raise self.snapshot_error
        return self.entries if self.entries is not None else [
            (tid, value["owner"]) for tid, value in self.threads.items() if value["owner"] in owners]

    def open_thread(self, rights, inherit, tid):
        self.opened.append((rights, inherit, tid))
        value = self.threads.get(tid)
        self.error = value.get("open_error", 0) if value else 87
        if self.error:
            return None
        self.clock += 1
        handle = 1000 + self.clock
        self.handles[handle] = value.copy()
        return handle

    def owner(self, handle):
        value = self.handles[handle]
        self.error = value.get("owner_error", 0)
        return 0 if self.error else value["owner"]

    def times(self, handle, created, exited, kernel, user):
        value = self.handles[handle]
        self.error = value.get("times_error", 0)
        if self.error:
            return False
        for pointer, number in ((created, value["created"]), (exited, value.get("exited", 0))):
            pointer._obj.dwLowDateTime = number & 0xffffffff
            pointer._obj.dwHighDateTime = number >> 32
        return True

    def cycles(self, handle, cycles):
        value = self.handles[handle]
        if value.get("raise_cycles"):
            raise RuntimeError("fake unexpected API failure")
        self.error = value.get("cycles_error", 0)
        if self.error:
            return False
        cycles._obj.value = value["cycles"]
        return True

    def exit_code(self, handle, code):
        value = self.handles[handle]
        self.error = value.get("exit_error", 0)
        if self.error:
            return False
        code._obj.value = value.get("exit_code", 259)
        return True

    def describe(self, handle, pointer):
        value = self.handles[handle]
        buffer = C.create_unicode_buffer(value["description"])
        address = C.addressof(buffer)
        self.allocations[address] = buffer
        C.cast(pointer, C.POINTER(C.c_void_p))[0] = address
        return value.get("description_hresult", 0)

    def free(self, pointer):
        address = pointer.value
        self.allocations.pop(address)
        self.freed.append(address)
        return None

    def close(self, handle):
        self.closed.append(handle)
        self.handles.pop(handle)


class FakeWindowApi:
    def __init__(self):
        self.thread, self.error, self.foreground = 11, 0, 0
        self.creation_ok = self.enumeration_ok = self.destruction_ok = True
        self.created, self.destroyed, self.enum_calls = [], [], 0
        self.entries = None
        self.records = {43: {'pid': 202, 'thread': 22, 'class': 'Target', 'visible': True, 'enabled': True},
                        44: {'pid': 303, 'thread': 33, 'class': 'Other', 'visible': False, 'enabled': True}}
        self.api = metrics.Windows.__new__(metrics.Windows)
        self.api.kernel = SimpleNamespace(GetCurrentThreadId=lambda: self.thread, GetModuleHandleW=lambda _: 1)
        self.api.user = SimpleNamespace(CreateWindowExW=self.create, DestroyWindow=self.destroy,
            EnumWindows=self.enumerate, GetWindowThreadProcessId=self.owner, GetClassNameW=self.class_name,
            GetWindowTextLengthW=lambda hwnd: len(self.records[hwnd].get('title', '')),
            GetWindowTextW=self.title,
            IsWindowVisible=lambda hwnd: self.records[hwnd]['visible'],
            IsWindowEnabled=lambda hwnd: self.records[hwnd]['enabled'], IsIconic=lambda _: False,
            GetForegroundWindow=lambda: self.foreground)
        self.api.callback = lambda function: function
        self.api.window_anchor = metrics.WindowEnumerationAnchor(self.api)

    def create(self, *arguments):
        self.created.append(arguments)
        if not self.creation_ok:
            return 0
        self.records[42] = {'pid': os.getpid(), 'thread': self.thread, 'class': 'Static', 'visible': False, 'enabled': False}
        return 42

    def destroy(self, hwnd):
        self.destroyed.append(hwnd)
        if self.destruction_ok:
            self.records.pop(hwnd)
        return self.destruction_ok

    def owner(self, hwnd, pointer):
        value = self.records.get(hwnd, {'pid': 0, 'thread': 0})
        pointer._obj.value = value['pid']
        return value['thread']

    def class_name(self, hwnd, buffer, _):
        buffer.value = self.records.get(hwnd, {}).get('class', '')
        return len(buffer.value)

    def title(self, hwnd, buffer, _):
        buffer.value = self.records.get(hwnd, {}).get('title', '')
        return len(buffer.value)

    def enumerate(self, callback, parameter):
        self.enum_calls += 1
        for hwnd in (self.entries if self.entries is not None else sorted(self.records)):
            if not callback(hwnd, parameter):
                return False
        return self.enumeration_ok


class WindowAnchorTests(unittest.TestCase):
    def setUp(self):
        self.fake = FakeWindowApi()
        self.api = self.fake.api
        for name, function in (('get_last_error', lambda: self.fake.error),
                               ('set_last_error', lambda value: setattr(self.fake, 'error', value))):
            mocked = patch.object(metrics.C, name, function, create=True)
            mocked.start()
            self.addCleanup(mocked.stop)

    def test_anchor_is_lazy_hidden_disabled_and_filtered_even_for_sampler_pid(self):
        self.assertIsNone(self.api.window_anchor.hwnd)
        self.assertEqual(self.fake.created, [])
        self.assertEqual(self.api.windows(202, 'Target'), [(43, True, False)])
        self.assertEqual(self.api.windows(os.getpid()), [])
        self.assertEqual(len(self.fake.created), 1)
        self.assertEqual(self.fake.created[0][:8], (0, 'STATIC', '', 0x88000000, 0, 0, 0, 0))
        self.assertEqual(self.fake.created[0][8:10], (None, None))

    def test_creation_failure_never_enumerates_or_claims_empty(self):
        self.fake.creation_ok = False
        with self.assertRaisesRegex(RuntimeError, 'Cannot create window enumeration anchor'):
            self.api.windows(202)
        self.assertEqual(self.fake.enum_calls, 0)
        self.assertIsNone(self.api.window_anchor.hwnd)

    def test_detailed_failure_inventory_keeps_titles_and_excludes_other_pids_and_anchor(self):
        self.fake.records[43]['title'] = 'Winderust settings \u98ce'
        self.fake.records[45] = {'pid': 202, 'thread': 22, 'class': 'OtherTarget',
                                 'title': '', 'visible': False, 'enabled': True}
        self.assertEqual(self.api.windows(202, details=True), [
            {'hwnd': 43, 'pid': 202, 'class': 'Target', 'title': 'Winderust settings \u98ce',
             'visible': True, 'iconic': False, 'title_length_win32_error': 0, 'title_read_win32_error': 0},
            {'hwnd': 45, 'pid': 202, 'class': 'OtherTarget', 'title': '',
             'visible': False, 'iconic': False, 'title_length_win32_error': 0, 'title_read_win32_error': 0}])

    def test_false_error_zero_missing_and_duplicate_anchor_are_all_fatal(self):
        for entries, result, message in (([42, 43], False, 'EnumWindows failed'),
                                         ([43], True, 'exact anchor once'), ([42, 42], True, 'exact anchor once')):
            with self.subTest(entries=entries, result=result):
                self.fake.entries, self.fake.enumeration_ok = entries, result
                with self.assertRaisesRegex(RuntimeError, message):
                    self.api.windows(202)
                self.assertEqual(self.api.window_anchor.successful_enumerations, 0)

    def test_each_enumeration_revalidates_owner_class_and_hidden_disabled_state(self):
        self.api.windows(202)
        for key, value in (('pid', 999), ('thread', 999), ('class', 'Different'), ('visible', True), ('enabled', True)):
            with self.subTest(field=key):
                previous = self.fake.records[42][key]
                self.fake.records[42][key] = value
                with self.assertRaises(RuntimeError):
                    self.api.windows(202)
                self.fake.records[42][key] = previous
        self.fake.foreground = 42
        with self.assertRaisesRegex(RuntimeError, 'foreground'):
            self.api.windows(202)
        self.assertEqual(self.fake.enum_calls, 1)

    def test_cleanup_is_same_thread_checked_idempotent_and_preserves_failed_handle(self):
        self.api.windows(202)
        self.fake.thread = 99
        with self.assertRaisesRegex(RuntimeError, 'creating thread'):
            self.api.close_window_anchor()
        self.assertEqual(self.fake.destroyed, [])
        self.fake.thread, self.fake.destruction_ok = 11, False
        with self.assertRaisesRegex(RuntimeError, 'Cannot destroy'):
            self.api.close_window_anchor()
        self.assertEqual(self.api.window_anchor.hwnd, 42)
        self.fake.destruction_ok = True
        self.api.close_window_anchor()
        self.api.close_window_anchor()
        self.assertEqual(self.fake.destroyed, [42, 42])
        self.assertIsNone(self.api.window_anchor.hwnd)

    def test_metadata_distinguishes_trial_and_collector_coverage(self):
        with self.assertRaisesRegex(RuntimeError, 'no successful trial'):
            self.api.window_anchor.metadata(0)
        self.api.windows(202)
        self.api.windows(202)
        value = self.api.window_anchor.metadata(1)
        self.assertEqual((value['pid'], value['thread_id'], value['hwnd'], value['class']), (os.getpid(), 11, 42, 'STATIC'))
        self.assertEqual((value['enumerations_before_trial'], value['successful_enumerations'],
                          value['successful_enumerations_during_trial'], value['exact_anchor_callbacks']), (1, 2, 1, 2))
        self.assertFalse(value['is_visible'] or value['is_enabled'] or value['is_foreground'])

    def test_main_finally_destroys_anchor_after_measurement_exception(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            executable = base / 'fixture.exe'
            executable.write_bytes(b'not executed')
            config = base / 'config.json'
            config.write_text(json.dumps({'targets': [{'name': 'test', 'exe': str(executable)}],
                                          'scenarios': [{'name': 'background'}]}), encoding='utf-8')

            def fail(*_):
                self.api.windows(202)
                raise RuntimeError('injected measurement failure')

            with patch.object(metrics.sys, 'argv', ['sampler', '--config', str(config), '--out', str(base / 'out')]), \
                    patch.object(metrics, 'Windows', return_value=self.api), patch.object(metrics, 'measure', side_effect=fail):
                with self.assertRaisesRegex(RuntimeError, 'injected measurement failure'):
                    metrics.main()
        self.assertEqual(self.fake.destroyed, [42])
        self.assertIsNone(self.api.window_anchor.hwnd)

    @unittest.skipUnless(os.name == 'nt', 'Windows native hidden-window APIs')
    def test_native_owned_hidden_anchor_enumerates_and_cleans_up(self):
        api = metrics.Windows()
        self.assertIsNone(api.window_anchor.hwnd)
        hwnd = None
        try:
            self.assertEqual(api.windows(os.getpid(), 'WinderustCollectorMissingWindowClass'), [])
            value = api.window_anchor.metadata(0)
            hwnd = value['hwnd']
            self.assertGreater(hwnd, 0)
            self.assertEqual(value['pid'], os.getpid())
            self.assertEqual(value['thread_id'], api.kernel.GetCurrentThreadId())
            self.assertFalse(api.user.IsWindowVisible(hwnd))
            self.assertFalse(api.user.IsWindowEnabled(hwnd))
            self.assertNotEqual(api.user.GetForegroundWindow(), hwnd)
            self.assertNotIn(hwnd, [row[0] for row in api.windows(os.getpid())])
            self.assertEqual(api.window_anchor.successful_enumerations, 2)
        finally:
            api.close_window_anchor()
        api.user.IsWindow.argtypes, api.user.IsWindow.restype = [metrics.W.HWND], metrics.W.BOOL
        self.assertFalse(api.user.IsWindow(hwnd))


class WindowFailureEvidenceTests(unittest.TestCase):
    def test_valid_observation_adds_no_inventory_queries_or_artifacts(self):
        api = SimpleNamespace(windows=lambda *_args, **_kwargs: self.fail('unexpected inventory query'))
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(metrics.validate_window_observation(api, SimpleNamespace(pid=202), 'Target',
                [], 'absent', False, 0, 0, Path(directory)), 'absent')
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_failed_gate_is_persisted_before_teardown_and_copied_to_failed_measurement(self):
        calls = []
        inventory = [{'hwnd': 43, 'pid': 202, 'class': 'Target', 'title': 'Winderust',
                      'visible': True, 'iconic': False}]

        def windows(*args, **kwargs):
            calls.append((args, kwargs))
            return inventory

        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            trial = base / '01-wind-background'
            trial.mkdir()
            with patch.object(metrics.time, 'perf_counter_ns', side_effect=[123_000_000_000, 123_100_000_000]), \
                    self.assertRaisesRegex(metrics.WindowStateValidationError, "got visible") as caught:
                metrics.validate_window_observation(SimpleNamespace(windows=windows),
                    SimpleNamespace(pid=202, created=1234), 'Target', [(43, True, False)],
                    'absent', False, 100, 120, trial)
            evidence = caught.exception.window_validation_failure
            artifact = trial / 'window-validation-failure.json'
            self.assertEqual(json.loads(artifact.read_text(encoding='utf-8')), evidence)
            self.assertEqual(calls, [((202,), {'details': True})])
            self.assertEqual(evidence['gate_windows'], [{'hwnd': 43, 'visible': True, 'iconic': False}])
            self.assertEqual(evidence['inventory'], inventory)
            self.assertEqual((evidence['fixture_elapsed_s'], evidence['sampling_elapsed_s']), (23, 3))
            self.assertEqual((evidence['pid'], evidence['created_filetime'], evidence['window_filter']), (202, 1234, 'Target'))
            self.assertEqual(evidence['observed_perf_counter_ns'], 123_000_000_000)
            self.assertTrue(evidence['observed_utc'].endswith('Z'))
            options = SimpleNamespace(warmup=0, duration=1, interval=1, trials=1, no_gpu=True,
                system_energy=False, thread_context_switches=False, memory_ex2=False,
                allow_enabled_automation=False, thread_cycles=False, resume=False,
                baseline_duration=0, baseline_only=False, out=base)
            output = base / 'measurements.json'
            with patch.object(metrics, 'metadata', return_value={}), \
                    patch.object(metrics, 'run', side_effect=caught.exception), \
                    self.assertRaises(metrics.WindowStateValidationError):
                metrics.measure(SimpleNamespace(dpi_awareness_set=True), options,
                    {'targets': [{'name': 'wind'}], 'scenarios': [{'name': 'background'}]}, output)
            failed = json.loads(output.read_text(encoding='utf-8'))
            self.assertEqual(failed['results'], [])
            self.assertEqual(failed['failure']['window_validation_failure'], evidence)

    def test_diagnostic_errors_preserve_failed_gate_without_accepting_trial(self):
        def windows(*_args, **_kwargs):
            raise RuntimeError('inventory failed')

        with tempfile.TemporaryDirectory() as directory, \
                patch.object(Path, 'write_text', side_effect=PermissionError('artifact denied')), \
                self.assertRaisesRegex(metrics.WindowStateValidationError, 'got visible') as caught:
            metrics.validate_window_observation(SimpleNamespace(windows=windows),
                SimpleNamespace(pid=202, created=1234), 43, [(43, True, False)],
                'absent', False, 0, 0, Path(directory))
        evidence = caught.exception.window_validation_failure
        self.assertEqual(evidence['inventory_error'], 'inventory failed')
        self.assertEqual(evidence['artifact_write_error'], 'artifact denied')
        self.assertEqual(evidence['gate_windows'], [{'hwnd': 43, 'visible': True, 'iconic': False}])


class ReadinessFileTests(unittest.TestCase):
    def test_transient_readiness_access_denial_retries_before_accepting_data(self):
        ready = {"process_id": 101, "automation_enabled": False}
        for code in (5, 32, 33):
            with self.subTest(winerror=code):
                denied = PermissionError(13, "Readiness file temporarily unavailable")
                denied.winerror = code
                with patch.object(metrics, "read_json", side_effect=[denied, ready]) as read, \
                        patch.object(metrics.time, "perf_counter", return_value=1), \
                        patch.object(metrics.time, "sleep") as sleep:
                    self.assertIs(metrics.read_readiness_json(Path("ready.json"), 2), ready)
                self.assertEqual(read.call_count, 2)
                sleep.assert_called_once_with(.02)

    def test_transient_readiness_denial_without_winerror_retries_before_accepting_data(self):
        ready = {"process_id": 101, "automation_enabled": False}
        for code in (errno.EPERM, errno.EACCES):
            with self.subTest(errno=code):
                denied = PermissionError(code, "Readiness file temporarily unavailable")
                self.assertIsNone(getattr(denied, "winerror", None))
                with patch.object(metrics, "read_json", side_effect=[denied, ready]) as read, \
                        patch.object(metrics.time, "perf_counter", return_value=1), \
                        patch.object(metrics.time, "sleep") as sleep:
                    self.assertIs(metrics.read_readiness_json(Path("ready.json"), 2), ready)
                self.assertEqual(read.call_count, 2)
                sleep.assert_called_once_with(.02)

    def test_persistent_readiness_denial_is_bounded_by_existing_startup_deadline(self):
        denied = PermissionError(13, "Readiness file remains unavailable")
        denied.winerror = 32
        clock = [0.0]

        def advance(seconds):
            clock[0] += seconds

        with patch.object(metrics, "read_json", side_effect=denied) as read, \
                patch.object(metrics.time, "perf_counter", side_effect=lambda: clock[0]), \
                patch.object(metrics.time, "sleep", side_effect=advance) as sleep:
            with self.assertRaisesRegex(RuntimeError, "readiness file access timeout: ready.json") as raised:
                metrics.read_readiness_json(Path("ready.json"), .05)
        self.assertIs(raised.exception.__cause__, denied)
        self.assertAlmostEqual(clock[0], .05)
        self.assertEqual(read.call_count, 4)
        self.assertEqual(sleep.call_count, 3)

    def test_persistent_readiness_denial_without_winerror_obeys_existing_deadline(self):
        for code in (errno.EPERM, errno.EACCES):
            with self.subTest(errno=code):
                denied = PermissionError(code, "Readiness file remains unavailable")
                clock = [0.0]

                def advance(seconds):
                    clock[0] += seconds

                with patch.object(metrics, "read_json", side_effect=denied) as read, \
                        patch.object(metrics.time, "perf_counter", side_effect=lambda: clock[0]), \
                        patch.object(metrics.time, "sleep", side_effect=advance) as sleep:
                    with self.assertRaisesRegex(RuntimeError, "readiness file access timeout: ready.json") as raised:
                        metrics.read_readiness_json(Path("ready.json"), .05)
                self.assertIs(raised.exception.__cause__, denied)
                self.assertAlmostEqual(clock[0], .05)
                self.assertEqual(read.call_count, 4)
                self.assertEqual(sleep.call_count, 3)

    def test_other_access_errors_and_non_readiness_reads_are_not_hidden(self):
        for code in (0, 87, 1314):
            with self.subTest(winerror=code):
                denied = PermissionError(errno.EACCES, "Unexpected access failure")
                denied.winerror = code
                with patch.object(metrics, "read_json", side_effect=denied) as read, \
                        patch.object(metrics.time, "sleep") as sleep:
                    with self.assertRaises(PermissionError) as raised:
                        metrics.read_readiness_json(Path("ready.json"), 2)
                self.assertIs(raised.exception, denied)
                read.assert_called_once_with(Path("ready.json"))
                sleep.assert_not_called()
        with patch.object(Path, "read_text", side_effect=PermissionError(13, "Denied")):
            with self.assertRaises(PermissionError):
                metrics.read_json(Path("finished.json"))

    def test_readiness_denial_without_winerror_rejects_other_errno(self):
        denied = PermissionError(999, "Unexpected access failure")
        with patch.object(metrics, "read_json", side_effect=denied) as read, \
                patch.object(metrics.time, "sleep") as sleep:
            with self.assertRaises(PermissionError) as raised:
                metrics.read_readiness_json(Path("ready.json"), 2)
        self.assertIs(raised.exception, denied)
        read.assert_called_once_with(Path("ready.json"))
        sleep.assert_not_called()


class ThreadCycleTests(unittest.TestCase):
    def setUp(self):
        self.api = FakeThreadApi()
        self.sampler = metrics.ThreadCycles(self.api)
        self.process = SimpleNamespace(pid=101, created=100, role="main", alive=lambda: True)
        self.targets = {101: self.process}
        self.error_patch = patch.object(metrics.C, "get_last_error", lambda: self.api.error, create=True)
        self.time_patch = patch.object(metrics.time, "perf_counter", lambda: self.api.now)
        self.error_patch.start()
        self.time_patch.start()
        self.addCleanup(self.error_patch.stop)
        self.addCleanup(self.time_patch.stop)

    def sample(self):
        return self.sampler.sample(self.targets, 0)

    def test_baseline_delta_identity_metadata_and_minimal_rights(self):
        first, = self.sample()
        self.assertEqual(first["thread_cycle_status"], "baseline")
        self.assertEqual((first["pid"], first["process_created_filetime"], first["tid"],
                          first["thread_created_filetime"]), (101, 100, 7, 110))
        self.assertIsNone(first["cycles_delta"])
        self.assertIsNone(first["cycles_per_s"])
        self.assertEqual(first["description"], "worker")
        self.assertEqual(first["identity_status"], "verified")
        self.api.now = 3
        self.api.threads[7].update(cycles=300, description="renamed worker")
        second, = self.sample()
        self.assertEqual(second["thread_cycle_status"], "measured")
        self.assertEqual((second["cycles_delta"], second["delta_elapsed_s"], second["cycles_per_s"]), (200, 2, 100))
        self.assertEqual(second["description"], "renamed worker")
        self.assertEqual([first["sample_index"], second["sample_index"]], [0, 1])
        self.assertEqual(self.api.opened, [(0x0800, False, 7)] * 2)
        self.assertEqual(len(self.api.closed), 2)
        self.assertEqual(len(self.api.freed), 2)
        self.assertFalse(self.api.handles)
        self.assertFalse(self.api.allocations)

    def test_only_admitted_main_and_helper_processes_are_queried(self):
        self.targets[202] = SimpleNamespace(pid=202, created=200, role="helper", alive=lambda: True)
        self.targets[303] = SimpleNamespace(pid=303, created=300, role="sampler", alive=lambda: True)
        self.targets[404] = SimpleNamespace(pid=404, created=400, role="compositor", alive=lambda: True)
        self.api.threads[8] = dict(owner=202, created=210, cycles=10, description="helper")
        self.api.threads[9] = dict(owner=303, created=310, cycles=10, description="collector")
        self.api.entries = [(7, 101), (8, 202), (9, 303), (10, 999)]
        rows = self.sample()
        self.assertEqual(self.api.owners, {101, 202})
        self.assertEqual({(row["pid"], row["role"]) for row in rows}, {(101, "main"), (202, "helper")})
        self.assertEqual([tid for _, _, tid in self.api.opened], [7, 8])

    def test_reused_tid_and_reused_pid_cannot_bridge_lifetimes(self):
        self.sample()
        self.api.now = 2
        self.api.threads[7].update(created=120, cycles=1000)
        reused, = self.sample()
        self.assertEqual(reused["thread_cycle_status"], "unavailable_tid_reused")
        self.assertEqual(reused["thread_created_filetime"], 120)
        self.assertIsNone(reused["cycles_delta"])
        self.api.now = 3
        self.assertEqual(self.sample()[0]["thread_cycle_status"], "baseline")
        self.api.now = 4
        self.api.threads[7]["cycles"] = 1100
        self.assertEqual(self.sample()[0]["cycles_delta"], 100)
        self.process.created = 200
        self.api.now = 5
        self.api.threads[7].update(created=210, cycles=9000)
        replacement, = self.sample()
        self.assertEqual(replacement["thread_cycle_status"], "baseline")
        self.assertIsNone(replacement["cycles_delta"])
        self.assertEqual(replacement["process_created_filetime"], 200)

    def test_snapshot_to_open_owner_reuse_is_rejected_and_handle_closed(self):
        self.api.entries = [(7, 101)]
        self.api.threads[7]["owner"] = 999
        row, = self.sample()
        self.assertEqual(row["thread_cycle_status"], "unavailable_owner_changed")
        self.assertEqual(row["observed_owner_pid"], 999)
        self.assertIsNone(row["thread_created_filetime"])
        self.assertIsNone(row["cycles"])
        self.assertEqual(len(self.api.closed), 1)
        self.assertFalse(self.api.freed)

    def test_missing_exited_and_access_denied_break_delta_continuity(self):
        for failure in ("missing", "exited", "access_denied"):
            with self.subTest(failure=failure):
                self.sampler = metrics.ThreadCycles(self.api)
                self.api.threads[7].pop("exit_code", None)
                self.api.threads[7].pop("open_error", None)
                self.api.entries = None
                self.sample()
                self.api.now += 1
                if failure == "missing":
                    self.api.entries = []
                elif failure == "exited":
                    self.api.threads[7]["exit_code"] = 0
                else:
                    self.api.threads[7]["open_error"] = 5
                row, = self.sample()
                self.assertEqual(row["thread_cycle_status"], "unavailable_" + failure)
                self.assertIsNone(row["cycles"])
                self.assertIsNone(row["cycles_delta"])
                if failure == "missing":
                    self.assertEqual(row["identity_status"], "previous_observation_only")
                self.api.now += 1
                self.api.entries = None
                self.api.threads[7].pop("exit_code", None)
                self.api.threads[7].pop("open_error", None)
                recovered, = self.sample()
                self.assertEqual(recovered["thread_cycle_status"], "baseline")
                self.assertIsNone(recovered["cycles_delta"])
        self.assertFalse(self.api.handles)

    def test_active_thread_ignores_undefined_nonzero_exit_filetime(self):
        self.api.threads[7]["exited"] = 2**48 + 123
        row, = self.sample()
        self.assertEqual(row["thread_cycle_status"], "baseline")
        self.assertEqual(row["cycles"], 100)
        self.assertFalse(self.api.handles)

    def test_counter_regression_is_unavailable_then_rebaselines(self):
        self.sample()
        self.api.now = 2
        self.api.threads[7]["cycles"] = 99
        row, = self.sample()
        self.assertEqual(row["thread_cycle_status"], "unavailable_counter_regression")
        self.assertEqual(row["cycles"], 99)
        self.assertIsNone(row["cycles_delta"])
        self.api.now = 3
        self.assertEqual(self.sample()[0]["thread_cycle_status"], "baseline")
        self.api.now = 4
        self.api.threads[7]["cycles"] = 99
        row, = self.sample()
        self.assertEqual(row["thread_cycle_status"], "measured")
        self.assertEqual(row["cycles_delta"], 0)  # A verified idle interval is a real zero.

    def test_query_failures_and_unexpected_exception_always_close_handles(self):
        for field, operation in (("owner_error", "owner_query"), ("times_error", "thread_times"),
                                 ("exit_error", "thread_exit_code"),
                                 ("cycles_error", "thread_cycles")):
            with self.subTest(field=field):
                self.api.threads[7][field] = 87
                row, = self.sample()
                self.assertEqual(row["thread_cycle_status"], f"unavailable_{operation}_error")
                self.assertEqual(row["win32_error"], 87)
                self.assertIsNone(row["cycles_delta"])
                self.assertFalse(self.api.handles)
                self.api.threads[7].pop(field)
        self.api.threads[7]["raise_cycles"] = True
        with self.assertRaisesRegex(RuntimeError, "fake unexpected"):
            self.sample()
        self.assertFalse(self.api.handles)
        self.assertEqual(len(self.api.closed), 5)

    def test_missing_open_target_is_unavailable_and_dead_process_is_never_opened(self):
        self.api.entries = [(7, 101)]
        self.api.threads.clear()
        row, = self.sample()
        self.assertEqual(row["thread_cycle_status"], "unavailable_open_thread_error")
        self.assertEqual(row["win32_error"], 87)
        self.assertFalse(self.api.closed)
        self.process.alive = lambda: False
        self.api.opened.clear()
        row, = self.sample()
        self.assertEqual(row["thread_cycle_status"], "unavailable_process_exited")
        self.assertFalse(self.api.opened)

    def test_description_unavailability_does_not_invent_or_suppress_cycles(self):
        self.api.threads[7]["description_hresult"] = -2147467259
        row, = self.sample()
        self.assertEqual(row["description_status"], "unavailable_hresult_0x80004005")
        self.assertEqual(row["thread_cycle_status"], "baseline")
        self.assertIsNone(row["description"])
        self.assertFalse(self.api.allocations)
        self.assertEqual(len(self.api.freed), 1)
        del self.api.kernel.GetThreadDescription
        self.api.now = 2
        self.api.threads[7]["cycles"] = 200
        row, = self.sample()
        self.assertEqual(row["description_status"], "unavailable_api")
        self.assertEqual(row["cycles_delta"], 100)
        self.assertFalse(self.api.handles)

    def test_snapshot_failure_is_explicit_and_breaks_baseline(self):
        self.sample()
        self.api.now = 2
        self.api.snapshot_error = OSError("fake snapshot failure")
        row, = self.sample()
        self.assertEqual(row["thread_cycle_status"], "unavailable_snapshot_error")
        self.assertEqual(row["thread_created_filetime"], 110)
        self.assertIsNone(row["cycles"])
        self.api.snapshot_error = None
        self.api.now = 3
        self.assertEqual(self.sample()[0]["thread_cycle_status"], "baseline")

    def test_toolhelp_filters_owners_and_closes_snapshot_on_enumeration_error(self):
        entries = iter([(7, 101), (8, 999)])
        closed, flags, sizes = [], [], []

        def advance(_, pointer):
            sizes.append(pointer._obj.dwSize)
            value = next(entries, None)
            self.api.error = 18 if value is None else 0
            if value:
                pointer._obj.th32ThreadID, pointer._obj.th32OwnerProcessID = value
            return value is not None

        def snapshot(flags_value, pid):
            flags.append((flags_value, pid))
            return 42

        api = SimpleNamespace(kernel=SimpleNamespace(CreateToolhelp32Snapshot=snapshot,
                              Thread32First=advance, Thread32Next=advance), close=closed.append)
        self.assertEqual(metrics.Windows.thread_snapshot(api, {101}), [(7, 101)])
        self.assertEqual(flags, [(4, 0)])
        self.assertEqual(closed, [42])
        self.assertEqual(sizes, [C.sizeof(metrics.ThreadEntry)] * 3)
        if os.name == "nt":
            self.assertEqual(C.sizeof(metrics.ThreadEntry), 28)

        def fail(*_):
            self.api.error = 5
            return False

        api.kernel.Thread32First = fail
        with patch.object(metrics.C, "WinError", lambda code: OSError(code, "fake"), create=True):
            with self.assertRaises(OSError):
                metrics.Windows.thread_snapshot(api, {101})
        self.assertEqual(closed, [42, 42])


class EnergyCounterTests(unittest.TestCase):
    def reader(self, collect_status=0, outputs=None):
        reader = metrics.Gpu.__new__(metrics.Gpu)
        reader.system_energy_enabled, reader.collect_status = True, None
        reader.query, reader.thread_cache, reader.errors = 1, None, {}
        reader.energy_counters = dict(zip(reader.energy_paths, (1, 2, 3)))
        reader.get_calls = []
        outputs = outputs or {1: (0, 0, 1e12), 2: (0, 1, 10000), 3: (0, 0, 1000)}

        def read(handle, flags, kind, pointer):
            reader.get_calls.append(handle)
            status, cstatus, value = outputs[handle]
            if cstatus is not None:
                pointer._obj.status = cstatus
            if value is not None:
                pointer._obj.value = value
            return status

        reader.pdh = SimpleNamespace(PdhCollectQueryData=lambda _: collect_status,
                                     PdhGetFormattedCounterValue=read)
        return reader

    def test_measured_codes_and_numeric_units_survive_csv(self):
        reader = self.reader()
        row = reader.energy(reader.collect())
        self.assertEqual(row["system_package_power_w"], 10)
        self.assertEqual(row["system_package_energy_pwh"], 1e12)
        evidence = row["system_package_pdh_status"]
        self.assertEqual(evidence["collect_status"], 0)
        self.assertIsNone(evidence["collect_error"])
        self.assertEqual({v["status"] for v in evidence["counters"].values()}, {"measured"})
        self.assertEqual(evidence["counters"]["system_package_power_mw"]["cstatus"], 1)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "energy.csv"
            metrics.write_csv(path, [dict(row, role="system_package")])
            with path.open(newline="", encoding="utf-8-sig") as stream:
                stored = next(csv.DictReader(stream))
            self.assertEqual(json.loads(stored["system_package_pdh_status"]), evidence)

    def test_missing_query_and_handles_are_explicit_without_fake_zero(self):
        reader = self.reader()
        reader.query, reader.energy_counters = None, {}
        reader.errors["query"] = "PDH setup failed"
        row = reader.energy(reader.collect())
        evidence = row["system_package_pdh_status"]
        self.assertIsNone(evidence["collect_status"])
        self.assertEqual(evidence["collect_error"], "unavailable")
        self.assertEqual(reader.get_calls, [])
        for name, counter in evidence["counters"].items():
            self.assertIsNone(row[name])
            self.assertFalse(counter["counter_present"])
            self.assertEqual(counter["status"], "missing_counter")
            self.assertEqual(counter["setup_error"], "PDH setup failed")
            self.assertIsNone(counter["get_status"])
            self.assertIsNone(counter["cstatus"])

    def test_failed_collect_retains_raw_results_without_accepting_energy(self):
        reader = self.reader(0x800007d5)
        row = reader.energy(reader.collect())
        evidence = row["system_package_pdh_status"]
        self.assertEqual(evidence["collect_status"], 0x800007d5)
        self.assertEqual(evidence["collect_error"], "PDH 0x800007d5")
        for name, counter in evidence["counters"].items():
            self.assertIsNone(row[name])
            self.assertEqual(counter["get_status"], 0)
            self.assertEqual(counter["status"], "collect_failed_or_unavailable")
            self.assertIsNotNone(counter["raw_value"])
        recovered = self.reader()
        current = recovered.energy(recovered.collect())
        metrics.add_energy_rates(current, row)
        self.assertNotIn("system_package_energy_j_delta", current)

    def test_failed_get_does_not_invent_unwritten_cstatus_or_value(self):
        reader = self.reader(outputs={1: (0xc0000bba, None, None),
                                     2: (0, 1, 10000), 3: (0, 0, 1000)})
        row = reader.energy(reader.collect())
        evidence = row["system_package_pdh_status"]["counters"]["system_package_energy_pwh"]
        self.assertIsNone(row["system_package_energy_pwh"])
        self.assertEqual(evidence["get_status"], 0xc0000bba)
        self.assertIsNone(evidence["cstatus"])
        self.assertEqual(evidence["raw_value"], "nan")
        self.assertEqual(evidence["status"], "get_failed")
        self.assertEqual(row["system_package_power_w"], 10)

    def test_invalid_cstatus_and_nonfinite_values_are_raw_evidence_only(self):
        for cstatus, value, expected in ((0xc0000bc6, 123, "invalid_cstatus"),
                                         (0, float("nan"), "nonfinite_value"),
                                         (1, float("inf"), "nonfinite_value")):
            with self.subTest(cstatus=cstatus, value=value):
                reader = self.reader(outputs={1: (0, cstatus, value),
                                             2: (0, 1, 10000), 3: (0, 0, 1000)})
                row = reader.energy(reader.collect())
                evidence = row["system_package_pdh_status"]["counters"]["system_package_energy_pwh"]
                self.assertIsNone(row["system_package_energy_pwh"])
                self.assertEqual(evidence["cstatus"], cstatus)
                self.assertEqual(evidence["status"], expected)
                self.assertEqual(evidence["raw_value"], value if expected == "invalid_cstatus" else str(value))

    def test_energy_disabled_adds_no_energy_evidence(self):
        reader = metrics.Gpu(enabled=False)
        self.assertEqual(reader.collect(), "unavailable")
        self.assertEqual(reader.energy(), {})


class MetricTests(unittest.TestCase):
    def test_thread_rates_report_matching_counters_without_claiming_verified_coverage(self):
        counters = metrics.Gpu.__new__(metrics.Gpu)
        counters.thread_cache = None
        counters.thread_counters = {"fixture": {"pid": 1, "switches": 2}}
        arrays = {1: {"fixture/0": 101, "fixture/1": 101, "other/0": 102},
                  2: {"fixture/0": 3, "fixture/1": 4, "other/0": 50}}
        counters.array = lambda handle: (arrays[handle], "measured")
        self.assertEqual(counters.context_switches(101), {
            "thread_context_switches_per_s": 7,
            "thread_context_switches_status": "reported_matching_thread_counters"})
        self.assertIsNone(counters.context_switches(999)["thread_context_switches_per_s"])

    def test_ex2_private_resident_and_zero_shared_commit_are_valid(self):
        def read(_, pointer, size):
            value = pointer._obj
            value.WorkingSetSize = 4096
            value.PrivateWorkingSetSize = 2048
            value.SharedCommitUsage = 0
            return True

        api = SimpleNamespace(memory_ex2_enabled=True,
                              psapi=SimpleNamespace(GetProcessMemoryInfo=read))
        _, extras = metrics.read_memory(api, 1)
        self.assertEqual(extras, {"private_working_set_bytes": 2048,
                                 "shared_commit_bytes": 0, "memory_api_status": "measured_ex2"})
        if os.name == "nt" and C.sizeof(C.c_void_p) == 8:
            self.assertEqual(C.sizeof(metrics.MemoryEx2), 96)
            self.assertEqual(metrics.MemoryEx2.PrivateWorkingSetSize.offset, 80)
            self.assertEqual(metrics.MemoryEx2.SharedCommitUsage.offset, 88)

    def test_ex2_unwritten_extension_uses_ex_without_fake_resident_zero(self):
        sizes = []

        def read(_, pointer, size):
            sizes.append(size)
            pointer._obj.WorkingSetSize = 4096
            return True

        api = SimpleNamespace(memory_ex2_enabled=True,
                              psapi=SimpleNamespace(GetProcessMemoryInfo=read))
        value, extras = metrics.read_memory(api, 1)
        self.assertEqual(sizes, [C.sizeof(metrics.MemoryEx2), C.sizeof(metrics.Memory)])
        self.assertEqual(value.WorkingSetSize, 4096)
        self.assertIsNone(extras["private_working_set_bytes"])
        self.assertIsNone(extras["shared_commit_bytes"])
        self.assertEqual(extras["memory_api_status"], "ex_fallback_ex2_not_populated")

    def test_core_protocol_uses_only_original_ex_call(self):
        sizes = []

        def read(_, pointer, size):
            sizes.append(size)
            return True

        api = SimpleNamespace(memory_ex2_enabled=False,
                              psapi=SimpleNamespace(GetProcessMemoryInfo=read))
        _, extras = metrics.read_memory(api, 1)
        self.assertEqual(sizes, [C.sizeof(metrics.Memory)])
        self.assertIsNone(extras["private_working_set_bytes"])

    def test_resume_preserves_accepted_trials_and_excludes_invalid_attempt(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "measurements.json"
            row = {"trial": 1, "target": "A", "scenario": "home", "csv": "accepted.csv"}
            previous = {"config": {"targets": ["A"]}, "protocol": {},
                        "results": [row], "sampler_sha256": "original",
                        "failure": {"trial": 2, "target": "A", "scenario": "home",
                                    "error": "Fixture lost foreground"}}
            path.write_text(json.dumps(previous), encoding="utf-8")
            original_bytes = path.read_bytes()
            (path.parent / "accepted.csv").write_text("raw", encoding="utf-8")
            requested = {"config": previous["config"], "protocol": {
                             "memory_ex2_enabled": False, "enabled_automation_allowed": False},
                         "sampler_sha256": "new", "recorded_utc": "now", "hardware": {}}
            restored, completed = metrics.resume_result(path, requested)
            self.assertEqual(restored["results"], [row])
            self.assertEqual(completed, {(1, "A", "home")})
            self.assertEqual(restored["sampler_sha256"], "original")
            self.assertNotIn("failure", restored)
            self.assertEqual(len(restored["invalid_attempts"]), 1)
            self.assertEqual((path.parent / restored["resume_history"][0]["previous_json"]).read_bytes(), original_bytes)
            mismatch = copy.deepcopy(requested)
            mismatch["config"]["targets"] = ["changed_binary"]
            with self.assertRaises(ValueError):
                metrics.resume_result(path, mismatch)

    def test_retry_keeps_failed_directory_log_and_partial_samples(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "02-A-home").mkdir()
            (root / "02-A-home.partial.csv").write_text("invalid", encoding="utf-8")
            self.assertEqual(metrics.fresh_trial_stem(root, "02-A-home"), "02-A-home-attempt02")
            self.assertTrue((root / "02-A-home.partial.csv").exists())
        with self.assertRaises(ValueError):
            metrics.accepted_keys([{"trial": 1, "target": "A", "scenario": "home"}] * 2)

    def test_rates_use_actual_elapsed_and_one_core_normalization(self):
        previous = {"elapsed_s": 1, "user_s": 2, "kernel_s": .25, "cycles": 100,
                    "page_faults": 2**32 - 2}
        current = {"elapsed_s": 3, "user_s": 2.5, "kernel_s": .75, "cycles": 300,
                   "page_faults": 3}
        row = metrics.add_rates(current, previous, 8)
        self.assertEqual(row["cpu_percent_one_core"], 50)
        self.assertEqual(row["cpu_percent_machine"], 6.25)
        self.assertEqual(row["cycles_per_s"], 100)
        self.assertEqual(row["page_faults_delta"], 5)

    def test_counter_regression_is_not_a_fake_zero(self):
        with self.assertRaises(ValueError):
            metrics.add_rates({"elapsed_s": 2, "cycles": 50},
                              {"elapsed_s": 1, "cycles": 100}, 8)

    def test_missing_gpu_is_unavailable_and_engines_are_not_summed(self):
        rows = [{"pid": 1, "role": "main", "elapsed_s": 1,
                 "private_bytes": 10, "working_set_bytes": 20,
                 "gpu_engines": {"luid_A_eng_0": 20, "luid_A_eng_1": 40},
                 "gpu_busiest_engine_percent": 40, "gpu_dedicated_bytes": 10},
                {"pid": 2, "role": "helper", "elapsed_s": 1,
                 "private_bytes": 5, "working_set_bytes": 12,
                 "gpu_engines": {"luid_A_eng_0": 30},
                 "gpu_busiest_engine_percent": 30, "gpu_dedicated_bytes": None}]
        combined = metrics.combine(rows, "combined")
        self.assertEqual(combined["gpu_busiest_engine_percent"], 50)
        self.assertEqual(combined["private_bytes"], 15)
        self.assertIsNone(combined["gpu_dedicated_bytes"])
        combined = metrics.combine([{"elapsed_s": 0, "gpu_engines": {}}], "combined")
        self.assertIsNone(combined["gpu_busiest_engine_percent"])

    def test_rapl_units_and_weighted_energy_use_meter_clock(self):
        previous = {"system_package_energy_pwh": 1e12, "system_package_meter_time_ms": 1000}
        row = {"system_package_energy_pwh": 1e12 + 2e9,
               "system_package_meter_time_ms": 2000, "elapsed_s": 1}
        metrics.add_energy_rates(row, previous)
        self.assertAlmostEqual(row["system_package_energy_j_delta"], 7.2)
        self.assertAlmostEqual(row["system_package_power_w_from_energy"], 7.2)
        summary = metrics.summarize([row])
        self.assertAlmostEqual(summary["system_package_energy_sampled_j"], 7.2)
        self.assertAlmostEqual(summary["system_package_power_w_weighted"], 7.2)

    def test_fixture_cleanup_must_be_explicitly_safe(self):
        metrics.validate_finished({"automation_enabled": False, "loop_result": {"Ok": "ok"}})
        for value in (None, {}, {"automation_enabled": True},
                      {"automation_enabled": False, "cleanup_errors": ["failed"]},
                      {"automation_enabled": False, "loop_result": {"Err": "failed"}}):
            with self.subTest(value=value), self.assertRaises(RuntimeError):
                metrics.validate_finished(value)

    def test_enabled_scheduler_contract_requires_explicit_opt_in(self):
        scenario = {"automation_contract": scheduler_contract()}
        with self.assertRaisesRegex(ValueError, "allow-enabled-automation"):
            metrics.validate_automation_contract(scenario)
        contract = metrics.validate_automation_contract(scenario, True)
        metrics.validate_ready(contract["ready"], contract)
        metrics.validate_finished(contract["finished"], contract)
        self.assertIsNone(metrics.validate_automation_contract({}, True))
        with self.assertRaises(RuntimeError):
            metrics.validate_finished(contract["finished"])

    def test_default_readiness_rejects_enabled_automation_even_without_contract(self):
        metrics.validate_ready({"automation_enabled": False})
        metrics.validate_ready({"process_id": 123})
        for value in (True, 1, "false", None):
            with self.subTest(value=value), self.assertRaises(RuntimeError):
                metrics.validate_ready({"automation_enabled": value})

    def test_off_profile_can_use_full_contract_without_starting_automation(self):
        contract = scheduler_contract()
        for phase in ("ready", "finished"):
            contract[phase]["automation_enabled"] = False
            contract[phase]["scheduler_benchmark"]["worker_running"] = False
            contract[phase]["scheduler_benchmark"]["enabled_features"] = []
        contract["finished"]["worker_running_at_measurement_end"] = False
        metrics.validate_automation_contract({"automation_contract": contract}, True)
        metrics.validate_ready(contract["ready"], contract)
        metrics.validate_finished(contract["finished"], contract)
        contract["finished"]["automation_enabled"] = True
        with self.assertRaises(RuntimeError):
            metrics.validate_automation_contract({"automation_contract": contract}, True)

    def test_enabled_contract_cannot_omit_or_weaken_safety_assertions(self):
        for phase, field, value in (("ready", "scope", "all_processes"),
                                    ("ready", "settings_io", "user_file"),
                                    ("ready", "cross_session_control", True),
                                    ("ready", "worker_running", False),
                                    ("finished", "worker_running", True),
                                    ("finished", "restoration_succeeded", False),
                                    ("finished", "profile", "other"),
                                    ("ready", "schema_version", True)):
            contract = scheduler_contract()
            contract[phase]["scheduler_benchmark"][field] = value
            with self.subTest(phase=phase, field=field), self.assertRaises(RuntimeError):
                metrics.validate_automation_contract({"automation_contract": contract}, True)
        contract = scheduler_contract()
        del contract["finished"]["scheduler_benchmark"]["restoration_succeeded"]
        with self.assertRaises(RuntimeError):
            metrics.validate_automation_contract({"automation_contract": contract}, True)

    def test_enabled_contract_requires_consistent_target_and_feature_names(self):
        for field, value in (("target_executable", "relative.exe"),
                             ("enabled_features", True), ("enabled_features", [1])):
            contract = scheduler_contract()
            contract["ready"]["scheduler_benchmark"][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                metrics.validate_automation_contract({"automation_contract": contract}, True)
        contract = scheduler_contract()
        contract["version"] = True
        with self.assertRaises(ValueError):
            metrics.validate_automation_contract({"automation_contract": contract}, True)

    def test_contract_matching_is_recursive_subset_with_exact_lists_and_json_types(self):
        metrics.match_fixture_fields({"nested": {"ok": True, "extra": "retained"}, "other": 1},
                                     {"nested": {"ok": True}})
        for actual, expected in (({"ok": 1}, {"ok": True}), ({}, {"ok": None}),
                                 (["B", "A"], ["A", "B"]), ([1, 2], [1]),
                                 ([1], [True]), ({"count": 1.0}, {"count": 1})):
            with self.subTest(actual=actual, expected=expected), self.assertRaises(RuntimeError):
                metrics.match_fixture_fields(actual, expected)

    def test_opt_in_still_rejects_failed_cleanup_and_unexpected_finished_fields(self):
        contract = scheduler_contract()
        for update in ({"cleanup_errors": ["restore failed"]},
                       {"runtime_worker_error": "failed"}, {"loop_result": {"Err": "failed"}},
                       {"automation_enabled": False}):
            finished = dict(contract["finished"], **update)
            with self.subTest(update=update), self.assertRaises(RuntimeError):
                metrics.validate_finished(finished, contract)
        finished = copy.deepcopy(contract["finished"])
        finished["scheduler_benchmark"]["restoration_succeeded"] = False
        with self.assertRaises(RuntimeError):
            metrics.validate_finished(finished, contract)

    def test_scheduler_errors_and_measurement_worker_loss_invalidate_trial(self):
        contract = scheduler_contract()
        ready = dict(contract["ready"], runtime_status={"worker_error": "consumed failure"})
        with self.assertRaises(RuntimeError):
            metrics.validate_ready(ready, contract)
        for update in ({"measurement_runtime_status": {"worker_error": "earlier failure"}},
                       {"worker_running_at_measurement_end": False},
                       {"worker_running_at_measurement_end": 1}):
            with self.subTest(update=update), self.assertRaises(RuntimeError):
                metrics.validate_finished(dict(contract["finished"], **update), contract)

    def test_native_verifier_argv_and_three_phase_json_outputs_are_preserved(self):
        contract = scheduler_contract()
        contract["native_verifier"] = ["python", "verify.py", "--phase", "{phase}",
                                       "--profile", "{profile}", "--trial-dir", "{trial_dir}"]
        metrics.validate_automation_contract({"automation_contract": contract}, True)
        with tempfile.TemporaryDirectory() as directory, patch.object(metrics.subprocess, "run") as run:
            run.return_value = SimpleNamespace(returncode=0, stdout='{"verified": true}', stderr="diagnostic")
            for phase in ("prepare", "ready", "finished"):
                result = metrics.run_native_verifier(contract, phase, Path(directory), 2.5)
                self.assertEqual(result, {"verified": True})
                command = run.call_args.args[0]
                self.assertEqual(command, ["python", "verify.py", "--phase", phase,
                                          "--profile", "priority-only", "--trial-dir", directory])
                self.assertIs(run.call_args.kwargs["shell"], False)
                self.assertEqual(run.call_args.kwargs["timeout"], 2.5)
                artifact = json.loads((Path(directory) / f"native-verification-{phase}.json").read_text())
                self.assertEqual(artifact["stderr"], "diagnostic")
                self.assertEqual(artifact["returncode"], 0)

    def test_native_verifier_requires_valid_command_and_known_placeholders(self):
        for command in ([], "python verify.py", [""], ["python", "{unknown}"]):
            contract = scheduler_contract()
            contract["native_verifier"] = command
            with self.subTest(command=command), self.assertRaises(ValueError):
                metrics.validate_automation_contract({"automation_contract": contract}, True)

    def test_native_verifier_rejects_failed_exit_and_invalid_json_without_acceptance(self):
        contract = scheduler_contract()
        contract["native_verifier"] = ["python", "verify.py"]
        with tempfile.TemporaryDirectory() as directory, patch.object(metrics.subprocess, "run") as run:
            for code, output in ((1, '{"verified": false}'), (0, "not JSON"), (0, "[]")):
                run.return_value = SimpleNamespace(returncode=code, stdout=output, stderr="")
                with self.subTest(code=code, output=output), self.assertRaises(RuntimeError):
                    metrics.run_native_verifier(contract, "ready", Path(directory), 2)
                self.assertTrue((Path(directory) / "native-verification-ready.json").exists())
            run.side_effect = subprocess.TimeoutExpired(["python"], 2)
            with self.assertRaisesRegex(RuntimeError, "timed out"):
                metrics.run_native_verifier(contract, "ready", Path(directory), 2)

    def test_native_verifier_is_never_invoked_for_default_or_hookless_contract(self):
        with patch.object(metrics.subprocess, "run") as run:
            self.assertIsNone(metrics.run_native_verifier(None, "ready", Path("unused"), 1))
            self.assertIsNone(metrics.run_native_verifier(scheduler_contract(), "ready", Path("unused"), 1))
            run.assert_not_called()

    def test_churn_allows_only_expected_resident_window_states(self):
        for state in ("visible", "hidden", "absent"):
            metrics.validate_window_state(state, "visible", True)
        for state in ("hidden", "absent", "minimized"):
            with self.subTest(state=state), self.assertRaises(RuntimeError):
                metrics.validate_window_state(state, "visible")
        with self.assertRaises(RuntimeError):
            metrics.validate_window_state("minimized", "visible", True)

    def test_system_cpu_kernel_already_includes_idle(self):
        previous = {"system_idle_s": 10, "system_kernel_s": 20, "system_user_s": 20}
        current = {"system_idle_s": 16, "system_kernel_s": 28, "system_user_s": 24}
        metrics.add_system_rates(current, previous)
        self.assertEqual(current["system_cpu_percent_machine"], 50)

    def test_wheel_primes_framework_cursor_and_uses_screen_wheel_coordinates(self):
        messages = []

        def rectangle(_, value):
            value._obj.right, value._obj.bottom = 1000, 700
            return 1

        def to_screen(_, value):
            value._obj.x += 100
            value._obj.y += 50
            return 1

        api = SimpleNamespace(user=SimpleNamespace(GetWindowRect=rectangle,
            GetClientRect=rectangle, ClientToScreen=to_screen,
            PostMessageW=lambda *args: messages.append(args) or 1))
        action = metrics.Interaction(api, 123, {"interaction": "scroll"})
        action.update(0)
        self.assertEqual([value[1] for value in messages], [0x200, 0x20A])
        self.assertEqual(messages[0][3], 550 | (455 << 16))
        self.assertEqual(messages[1][3], 650 | (505 << 16))
        self.assertEqual(action.actions, 1)

    def test_weighted_totals_and_growth_do_not_average_unequal_intervals(self):
        rows = [{"elapsed_s": 0, "private_bytes": 100},
                {"elapsed_s": 1, "interval_s": 1, "cycles_delta": 200,
                 "cycles_per_s": 200, "private_bytes": 110},
                {"elapsed_s": 4, "interval_s": 3, "cycles_delta": 300,
                 "cycles_per_s": 100, "private_bytes": 140}]
        summary = metrics.summarize(rows)
        self.assertEqual(summary["cycles_weighted_per_s"], 125)
        self.assertAlmostEqual(summary["private_bytes_slope_per_minute"], 600)
        self.assertEqual(summary["private_bytes_end_minus_start"], 40)
        self.assertEqual(summary["cycles_per_s"]["p95"], 195)

    def test_combined_rates_keep_intervals_for_weighted_summary(self):
        previous = {"elapsed_s": 0, "cycles": 100}
        first = metrics.add_rates({"pid": 1, "elapsed_s": 1, "cycles": 120}, previous, 8)
        second = metrics.add_rates({"pid": 2, "elapsed_s": 1, "cycles": 130}, previous, 8)
        combined = metrics.combine([first, second], "combined")
        summary = metrics.summarize([combined])
        self.assertEqual(summary["cycles_weighted_per_s"], 50)

    def test_counterbalanced_target_and_scenario_order(self):
        targets = [{"name": "A"}, {"name": "B"}]
        scenarios = [{"name": "X"}, {"name": "Y"}]
        order = [(trial, target["name"], scenario["name"])
                 for trial, target, scenario in metrics.trial_order(targets, scenarios, 2)]
        self.assertEqual(order, [(1, "A", "X"), (1, "B", "X"),
                                 (1, "A", "Y"), (1, "B", "Y"),
                                 (2, "B", "Y"), (2, "A", "Y"),
                                 (2, "B", "X"), (2, "A", "X")])

    @unittest.skipUnless(os.name == "nt", "Windows process APIs")
    def test_native_counters_on_owned_sleeping_child(self):
        api = metrics.Windows()
        with tempfile.TemporaryDirectory() as directory:
            ready = Path(directory) / "ready"
            # Hold the owned child alive until its counters and snapshot are read,
            # rather than racing a fixed sleep under a loaded test host.
            code = "import pathlib,sys; pathlib.Path(sys.argv[1]).write_text('ready'); sys.stdin.buffer.read(1)"
            child = subprocess.Popen([sys.executable, "-c", code, str(ready)], stdin=subprocess.PIPE,
                                     creationflags=subprocess.CREATE_NO_WINDOW)
            tracked = metrics.Process(api, child.pid, "main")
            try:
                deadline = time.monotonic() + 5
                while not ready.exists() and child.poll() is None and time.monotonic() < deadline:
                    time.sleep(.01)
                self.assertTrue(ready.exists(), "owned counter-test child did not become ready")
                first = tracked.read()
                self.assertGreater(first["private_bytes"], 0)
                self.assertGreater(first["working_set_bytes"], 0)
                self.assertGreater(first["handles"], 0)
                self.assertGreaterEqual(first["cycles"], 0)
                snapshot = api.snapshot()
                self.assertIn(child.pid, snapshot)
                self.assertGreater(snapshot[child.pid][1], 0)
                self.assertEqual(snapshot[child.pid][0], os.getpid())
                child.communicate(input=b"x", timeout=5)
                self.assertFalse(tracked.alive())
                lifetime = tracked.lifetime()
                self.assertGreaterEqual(lifetime["cycles"], first["cycles"])
                self.assertGreater(lifetime["exited_filetime"], lifetime["created_filetime"])
            finally:
                try:
                    if child.poll() is None:
                        child.communicate(input=b"x", timeout=5)
                finally:
                    if child.poll() is None:
                        child.kill()
                        child.wait(timeout=5)
                    tracked.close()


class WindowPolicyTests(unittest.TestCase):
    def test_absent_window_never_queries_or_restores_cursor(self):
        policy = metrics.WindowPolicy(SimpleNamespace(user=SimpleNamespace()), {"window": "absent"})
        policy.verify([])
        policy.restore_pointer()
        self.assertIsNone(policy.original_pointer)

    def test_visible_activation_still_fails_before_side_effects_without_cursor_access(self):
        user = SimpleNamespace(GetAsyncKeyState=lambda key: 0, GetCursorPos=Mock(return_value=0),
                               SetForegroundWindow=Mock())
        policy = metrics.WindowPolicy(SimpleNamespace(user=user), {"window": "visible"})
        with patch.object(metrics, "checked", side_effect=OSError("desktop access denied")):
            with self.assertRaisesRegex(OSError, "desktop access denied"):
                policy.activate(99)
            policy.restore_pointer()
        user.GetCursorPos.assert_called_once()
        user.SetForegroundWindow.assert_not_called()
        self.assertIsNone(policy.original_pointer)

    def test_activations_keep_first_cursor_and_preserve_user_movement(self):
        cursor, foreground, moves = [12, 34], [None], []

        def get_cursor(pointer):
            pointer._obj.x, pointer._obj.y = cursor
            return True

        def set_cursor(x, y):
            cursor[:] = [x, y]
            moves.append((x, y))
            return True

        def client_rect(hwnd, pointer):
            pointer._obj.right, pointer._obj.bottom = 100, 100
            return True

        user = SimpleNamespace(GetAsyncKeyState=lambda key: 0,
            GetCursorPos=Mock(side_effect=get_cursor), SetCursorPos=set_cursor,
            SetForegroundWindow=lambda hwnd: foreground.__setitem__(0, hwnd),
            GetForegroundWindow=lambda: foreground[0], GetClientRect=client_rect,
            ClientToScreen=lambda hwnd, pointer: True)
        policy = metrics.WindowPolicy(SimpleNamespace(user=user), {"window": "visible"})
        policy.activate(99)
        policy.activate(100)
        user.GetCursorPos.assert_called_once()
        policy.restore_pointer()
        self.assertEqual(cursor, [12, 34])
        policy.activate(100)
        cursor[:] = [91, 92]
        policy.restore_pointer()
        self.assertEqual(cursor, [91, 92])
        self.assertEqual(moves.count((12, 34)), 1)


if __name__ == "__main__":
    unittest.main()
