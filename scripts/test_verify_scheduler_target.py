"""Trust-boundary and native-readback interpretation checks; no process mutations."""
import copy
import errno
import json
from pathlib import Path
from types import SimpleNamespace
import struct
import tempfile
import unittest
from unittest.mock import patch

import verify_scheduler_target as verify


def properties():
    return {"priority_class": 0x20, "affinity_mask": 0xfff,
            "system_affinity_mask": 0xfff,
            "power_throttling": {"version": 1, "control_mask": 0, "state_mask": 0}}


class VerificationTests(unittest.TestCase):
    def test_manifest_rejects_unrelated_executable_and_pid_reuse(self):
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory) / "WinderustSchedulerBench-test.exe"
            executable.write_bytes(b"fixture")
            worker = {"pid": 123, "creation_filetime": 456, "exe": str(executable),
                      "sha256": verify.file_hash(executable),
                      "heartbeat": str(Path(directory) / "pulse"), "mode": "idle"}
            manifest = {"target_executable": str(executable), "workers": [worker],
                        "original_power_guid": "381b4222-f694-41f0-9685-ff5bb260df2e"}
            verify.validate_manifest(manifest)
            actual = {key: worker[key] for key in ("pid", "creation_filetime", "exe", "sha256")}
            verify.verify_identity(worker, actual)
            actual["creation_filetime"] += 1
            with self.assertRaisesRegex(verify.VerificationError, "identity changed"):
                verify.verify_identity(worker, actual)
            manifest["workers"] *= 2
            with self.assertRaisesRegex(verify.VerificationError, "Duplicate"):
                verify.validate_manifest(manifest)
            manifest["target_executable"] = str(Path(directory) / "explorer.exe")
            with self.assertRaisesRegex(verify.VerificationError, "dedicated"):
                verify.validate_manifest(manifest)

    def test_hash_change_at_same_path_is_not_the_same_target(self):
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory) / "WinderustSchedulerBench-test.exe"
            executable.write_bytes(b"original")
            expected = {"pid": 1, "creation_filetime": 2, "exe": str(executable),
                        "sha256": verify.file_hash(executable)}
            executable.write_bytes(b"replacement")
            actual = dict(expected, sha256=verify.file_hash(executable))
            with self.assertRaisesRegex(verify.VerificationError, "hash changed"):
                verify.verify_identity(expected, actual)

    def test_manifest_rejects_boolean_identity_and_relative_heartbeat(self):
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory) / "WinderustSchedulerBench-test.exe"
            executable.touch()
            worker = {"pid": True, "creation_filetime": 2, "exe": str(executable),
                      "heartbeat": "relative", "mode": "idle"}
            manifest = {"target_executable": str(executable), "workers": [worker],
                        "original_power_guid": "381b4222-f694-41f0-9685-ff5bb260df2e"}
            with self.assertRaisesRegex(verify.VerificationError, "integer pid"):
                verify.validate_manifest(manifest)
            worker["pid"] = 1
            with self.assertRaisesRegex(verify.VerificationError, "absolute heartbeat"):
                verify.validate_manifest(manifest)

    def test_inventory_uses_id_not_logical_index_and_skips_unknown_records(self):
        record = struct.pack("<IIIHBB", 16, 0, 44, 0, 2, 0)
        other_group = struct.pack("<IIIHBB", 16, 0, 55, 1, 2, 0)
        unknown = struct.pack("<II", 8, 9)
        inventory = verify.cpu_set_inventory(unknown + record + other_group)
        self.assertEqual(verify.cpu_set_ids_for_mask(inventory, 4), [44])
        with self.assertRaisesRegex(verify.VerificationError, "map uniquely"):
            verify.cpu_set_ids_for_mask(inventory, 5)

    def test_malformed_cpu_inventory_cannot_run_past_buffer(self):
        for data in (b"\x00", struct.pack("<II", 0, 0), struct.pack("<II", 100, 0),
                     struct.pack("<II", 8, 0)):
            with self.subTest(data=data), self.assertRaises(verify.VerificationError):
                verify.cpu_set_inventory(data)

    def test_combined_requires_all_four_actions_and_detects_unrelated_changes(self):
        before = dict(properties(), io_priority=2, memory_priority=5, cpu_set_ids=[])
        after = copy.deepcopy(before)
        after.update(priority_class=0x40, io_priority=1, memory_priority=2, cpu_set_ids=[44])
        after["power_throttling"].update(control_mask=5, state_mask=1)
        self.assertEqual(verify.ready_differences("combined", before, after, 4, [44]), [])
        after["io_priority"] = 2
        self.assertTrue(any("io_priority" in error for error in
                            verify.ready_differences("combined", before, after, 4, [44])))
        after["io_priority"] = 1
        after["affinity_mask"] = 4
        self.assertTrue(any("affinity_mask" in error for error in
                            verify.ready_differences("combined", before, after, 4, [44])))

    def test_memory_low_is_two_and_io_low_is_one(self):
        for profile, field, baseline, selected in (("memory-priority", "memory_priority", 5, 2),
                                                   ("io-priority", "io_priority", 2, 1)):
            before = dict(properties(), **{field: baseline})
            after = dict(before, **{field: selected})
            self.assertEqual(verify.ready_differences(profile, before, after), [])
            self.assertTrue(verify.ready_differences(profile, before, before))

    def test_ecoqos_requires_both_control_and_state_execution_speed_bit(self):
        before = properties()
        after = copy.deepcopy(before)
        after["priority_class"] = 0x40
        after["power_throttling"].update(control_mask=1, state_mask=0)
        self.assertTrue(verify.ready_differences("background-efficiency", before, after))
        after["power_throttling"]["state_mask"] = 1
        self.assertEqual(verify.ready_differences("background-efficiency", before, after), [])
        self.assertTrue(verify.restoration_differences(before, after))

    def test_thread_restoration_compares_creation_identity_and_valid_minus_one(self):
        before = dict(properties(), threads=[{"tid": 1, "creation_filetime": 10, "priority": 0},
                                             {"tid": 2, "creation_filetime": 20, "priority": 0}])
        after = dict(properties(), threads=[{"tid": 1, "creation_filetime": 10, "priority": -1},
                                            {"tid": 2, "creation_filetime": 21, "priority": -1}])
        self.assertEqual(verify.ready_differences("thread-priority", before, after), [])
        errors = verify.restoration_differences(before, after)
        self.assertEqual(len(errors), 1)
        self.assertIn("thread 1", errors[0])
        after["threads"][0]["priority"] = 0
        self.assertEqual(verify.restoration_differences(before, after), [])

    def test_gpu_without_context_is_not_a_fake_zero_or_wrong_priority(self):
        before = dict(properties(), gpu_priority={"supported": False, "reason": "no_gpu_context"})
        self.assertEqual(verify.ready_differences("gpu-priority", before, before), [])
        after = dict(before, gpu_priority={"supported": True, "value": 0})
        self.assertTrue(verify.ready_differences("gpu-priority", before, after))

    def test_pending_gpu_targets_must_remain_at_their_actual_baseline(self):
        before = dict(properties(), gpu_priority={"supported": True, "value": 2})
        self.assertEqual(verify.ready_differences("gpu-priority", before, before,
                                                gpu_pending=True), [])
        self.assertTrue(verify.ready_differences("gpu-priority", before, before))
        after = dict(before, gpu_priority={"supported": True, "value": 1})
        self.assertTrue(verify.ready_differences("gpu-priority", before, after,
                                               gpu_pending=True))

    def test_current_power_guid_alone_does_not_prove_a_matched_decision(self):
        guid = "381b4222-f694-41f0-9685-ff5bb260df2e"
        runtime = {"power_plan": {"current_guid": guid, "target_guid": None,
                                  "reason": "Using the configured default Active plan.",
                                  "apply_failed": False}}
        self.assertTrue(verify.power_decision_differences("by-cpu-load", runtime, guid, "target.exe"))
        runtime["power_plan"].update(target_guid=guid,
            reason="CPU load is 2.0% and matched rule 'Scheduler benchmark current plan'.")
        self.assertEqual(verify.power_decision_differences("by-cpu-load", runtime, guid,
                                                         "target.exe"), [])
        runtime["power_plan"]["apply_failed"] = True
        self.assertTrue(verify.power_decision_differences("by-cpu-load", runtime, guid, "target.exe"))

    def test_running_app_decision_identifies_the_disposable_executable(self):
        guid = "381b4222-f694-41f0-9685-ff5bb260df2e"
        name = "WinderustSchedulerBench-test.exe"
        runtime = {"power_plan": {"current_guid": guid, "target_guid": guid,
                    "reason": name + " is running and matched By Running App rule 'Scheduler benchmark target'.",
                    "apply_failed": False}, "by_running_app": {
                    "active_rule": "Scheduler benchmark target", "active_process": name,
                    "target_guid": guid}}
        self.assertEqual(verify.power_decision_differences("by-running-app", runtime, guid,
                                                         name), [])
        runtime["by_running_app"]["active_process"] = "explorer.exe"
        self.assertTrue(verify.power_decision_differences("by-running-app", runtime, guid, name))

    def test_control_profile_proves_no_native_mutation(self):
        before = properties()
        after = dict(before, priority_class=0x4000)
        self.assertEqual(verify.ready_differences("enabled-no-match", before, before), [])
        self.assertTrue(verify.ready_differences("enabled-no-match", before, after))

    def test_atomic_evidence_and_heartbeat_parse(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "evidence.json"
            verify.write_json(target, {"status": "verified"})
            self.assertEqual(json.loads(target.read_text()), {"status": "verified"})
            target.write_text("1234")
            self.assertEqual(verify.heartbeat(target)["elapsed_ms"], 1234)
            target.write_text("-1")
            with self.assertRaisesRegex(verify.VerificationError, "negative"):
                verify.heartbeat(target)

    def test_transient_heartbeat_access_sharing_and_lock_failures_retry_fresh_read(self):
        for winerror, error_number in ((5, errno.EACCES), (32, errno.EACCES), (33, errno.EACCES),
                                       (None, errno.EACCES), (None, errno.EPERM)):
            error = PermissionError(error_number, "busy")
            if winerror is not None:
                error.winerror = winerror
            now, sleeps = [0.0], []

            def sleep(duration):
                sleeps.append(duration)
                now[0] += duration

            with self.subTest(winerror=winerror, errno=error_number), \
                    patch.object(verify.time, "monotonic", side_effect=lambda: now[0]), \
                    patch.object(verify.time, "sleep", side_effect=sleep), \
                    patch.object(Path, "read_text", side_effect=[error, "1234"]) as read, \
                    patch.object(Path, "stat", return_value=SimpleNamespace(st_mtime_ns=99)):
                self.assertEqual(verify.heartbeat("pulse"), {"elapsed_ms": 1234, "mtime_ns": 99})
                self.assertEqual(read.call_count, 2)
                self.assertEqual(sleeps, [0.02])

    def test_persistent_heartbeat_denial_obeys_local_and_existing_deadlines(self):
        for deadline, expected in ((None, 0.25), (0.035, 0.035), (10, 0.25)):
            now, sleeps = [0.0], []

            def sleep(duration):
                sleeps.append(duration)
                now[0] += duration

            with self.subTest(deadline=deadline), \
                    patch.object(verify.time, "monotonic", side_effect=lambda: now[0]), \
                    patch.object(verify.time, "sleep", side_effect=sleep), \
                    patch.object(Path, "read_text", side_effect=PermissionError(errno.EACCES, "denied")) as read:
                with self.assertRaisesRegex(verify.VerificationError, "denied"):
                    verify.heartbeat("pulse", deadline=deadline)
                self.assertAlmostEqual(now[0], expected)
                self.assertTrue(sleeps and all(0 < duration <= 0.02 for duration in sleeps))
                self.assertLessEqual(read.call_count, 13)

    def test_unrelated_and_malformed_heartbeat_failures_do_not_retry(self):
        unsupported = PermissionError(errno.EACCES, "explicit unrelated Win32 error")
        unsupported.winerror = 87
        for error in (FileNotFoundError(errno.ENOENT, "missing"), OSError(errno.EIO, "I/O failure"),
                      unsupported, ValueError("invalid elapsed time")):
            with self.subTest(error=error), patch.object(verify.time, "monotonic", return_value=0), \
                    patch.object(verify.time, "sleep") as sleep, \
                    patch.object(Path, "read_text", side_effect=error) as read:
                with self.assertRaises(verify.VerificationError):
                    verify.heartbeat("pulse")
                read.assert_called_once()
                sleep.assert_not_called()

    def test_heartbeat_stat_contention_retries_content_and_expired_deadline_never_reads(self):
        now = [0.0]
        with patch.object(verify.time, "monotonic", side_effect=lambda: now[0]), \
                patch.object(verify.time, "sleep", side_effect=lambda duration: now.__setitem__(0, now[0] + duration)), \
                patch.object(Path, "read_text", side_effect=["1234", "1235"]) as read, \
                patch.object(Path, "stat", side_effect=[PermissionError(errno.EACCES, "busy"), SimpleNamespace(st_mtime_ns=100)]):
            self.assertEqual(verify.heartbeat("pulse"), {"elapsed_ms": 1235, "mtime_ns": 100})
            self.assertEqual(read.call_count, 2)
        with patch.object(verify.time, "monotonic", return_value=3), patch.object(Path, "read_text") as read:
            with self.assertRaisesRegex(verify.VerificationError, "deadline expired"):
                verify.heartbeat("pulse", deadline=3)
            read.assert_not_called()


if __name__ == "__main__":
    unittest.main()
