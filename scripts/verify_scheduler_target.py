"""Read-only native verification for exact disposable scheduler benchmark targets.

No process settings are changed here. A successful status_only result explicitly
does not prove an active action (for example GPU Priority without a GPU context).
Win32 CPU Set IDs are mapped through the native inventory, not logical indexes:
https://learn.microsoft.com/windows/win32/api/winnt/ns-winnt-system_cpu_set_information
"""
import argparse
from contextlib import contextmanager
import ctypes as C
import errno
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import sys
import time
import uuid


PROFILES = {
    "disabled", "enabled-no-match", "process-priority", "thread-priority",
    "dynamic-priority-boost", "io-priority", "memory-priority", "gpu-priority",
    "background-efficiency", "cpu-sets", "processor-affinity", "cpu-limiter",
    "app-suspension", "by-activity", "by-foreground", "by-running-app",
    "by-cpu-load", "by-time", "timer-resolution", "adaptive-detection",
    "adaptive-pressure-monitor", "combined",
}
TARGET_NAME = re.compile(r"WinderustSchedulerBench-[A-Za-z0-9-]+\.exe", re.IGNORECASE)
U32 = C.c_uint32
I32 = C.c_int32
HANDLE = C.c_void_p


class VerificationError(RuntimeError):
    pass


class FileTime(C.Structure):
    _fields_ = [("low", U32), ("high", U32)]

    def integer(self):
        return self.low | (self.high << 32)


class PowerThrottling(C.Structure):
    _fields_ = [("version", U32), ("control_mask", U32), ("state_mask", U32)]


class ThreadEntry(C.Structure):
    _fields_ = [("size", U32), ("usage", U32), ("tid", U32), ("pid", U32),
                ("base_priority", I32), ("delta_priority", I32), ("flags", U32)]


def normalized_path(value):
    return os.path.normcase(str(Path(value).resolve(strict=True)))


def file_hash(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def validate_manifest(manifest):
    target = Path(manifest.get("target_executable", ""))
    if not target.is_absolute() or not TARGET_NAME.fullmatch(target.name):
        raise VerificationError("Target must be an absolute dedicated benchmark executable")
    canonical = normalized_path(target)
    try:
        uuid.UUID(manifest["original_power_guid"])
    except (KeyError, ValueError, TypeError, AttributeError) as error:
        raise VerificationError("Manifest requires original_power_guid") from error
    workers = manifest.get("workers")
    if not isinstance(workers, list) or not workers:
        raise VerificationError("Manifest must identify at least one target worker")
    seen, heartbeats = set(), set()
    for worker in workers:
        for key in ("pid", "creation_filetime"):
            if type(worker.get(key)) is not int or worker[key] <= 0:
                raise VerificationError(f"Worker requires positive integer {key}")
        if worker["pid"] in seen:
            raise VerificationError("Duplicate target PID")
        seen.add(worker["pid"])
        if normalized_path(worker.get("exe", "")) != canonical:
            raise VerificationError("Every worker must use the exact dedicated executable")
        if not Path(worker.get("heartbeat", "")).is_absolute():
            raise VerificationError("Worker requires an absolute heartbeat path")
        pulse = os.path.normcase(str(Path(worker["heartbeat"]).resolve()))
        if pulse in heartbeats:
            raise VerificationError("Workers must have distinct heartbeat files")
        heartbeats.add(pulse)
        if worker.get("mode") not in ("idle", "busy"):
            raise VerificationError("Worker mode must be idle or busy")
        if "sha256" in worker and not re.fullmatch(r"[0-9a-fA-F]{64}", worker["sha256"]):
            raise VerificationError("Invalid executable SHA-256")
    return manifest


def verify_identity(expected, actual):
    for key in ("pid", "creation_filetime"):
        if expected[key] != actual[key]:
            raise VerificationError(f"Worker {expected['pid']} identity changed: {key}")
    if normalized_path(expected["exe"]) != normalized_path(actual["exe"]):
        raise VerificationError(f"Worker {expected['pid']} executable path changed")
    if expected.get("sha256", actual["sha256"]).lower() != actual["sha256"].lower():
        raise VerificationError(f"Worker {expected['pid']} executable hash changed")


def cpu_set_inventory(data):
    """Parse only the documented common fields of variable-sized CPU Set records."""
    result = []
    offset = 0
    while offset < len(data):
        if len(data) - offset < 8:
            raise VerificationError("Truncated CPU Set inventory header")
        size, kind = struct.unpack_from("<II", data, offset)
        if size < 8 or size > len(data) - offset:
            raise VerificationError("Invalid CPU Set inventory record size")
        if kind == 0:
            if size < 16:
                raise VerificationError("Truncated CPU Set inventory record")
            identifier, group, logical = struct.unpack_from("<IHB", data, offset + 8)
            result.append({"id": identifier, "group": group, "logical_processor": logical})
        offset += size
    return result


def cpu_set_ids_for_mask(inventory, mask):
    if type(mask) is not int or mask <= 0:
        raise VerificationError("Fixture requires a positive expected_core_mask")
    selected = [item["id"] for item in inventory
                if item["group"] == 0 and mask & (1 << item["logical_processor"])]
    covered = sum(1 << item["logical_processor"] for item in inventory
                  if item["group"] == 0 and mask & (1 << item["logical_processor"]))
    if covered != mask or len(set(selected)) != len(selected):
        raise VerificationError("Expected processor mask does not map uniquely to CPU Set IDs")
    return sorted(selected)


class Native:
    """Query-only native API. Construct after importing; non-Windows tests can import safely."""

    def __init__(self):
        self.kernel = C.WinDLL("kernel32", use_last_error=True)
        self.powr = C.WinDLL("powrprof", use_last_error=True)
        self.nt = C.WinDLL("ntdll", use_last_error=True)
        self.gdi = C.WinDLL("gdi32", use_last_error=True)
        signatures = {
            "OpenProcess": ([U32, I32, U32], HANDLE),
            "OpenThread": ([U32, I32, U32], HANDLE),
            "CloseHandle": ([HANDLE], I32),
            "WaitForSingleObject": ([HANDLE, U32], U32),
            "QueryFullProcessImageNameW": ([HANDLE, U32, C.c_wchar_p, C.POINTER(U32)], I32),
            "GetProcessTimes": ([HANDLE] + [C.POINTER(FileTime)] * 4, I32),
            "GetThreadTimes": ([HANDLE] + [C.POINTER(FileTime)] * 4, I32),
            "GetPriorityClass": ([HANDLE], U32),
            "GetProcessInformation": ([HANDLE, I32, HANDLE, U32], I32),
            "GetProcessAffinityMask": ([HANDLE, C.POINTER(C.c_size_t), C.POINTER(C.c_size_t)], I32),
            "GetProcessPriorityBoost": ([HANDLE, C.POINTER(I32)], I32),
            "GetProcessDefaultCpuSets": ([HANDLE, C.POINTER(U32), U32, C.POINTER(U32)], I32),
            "GetSystemCpuSetInformation": ([HANDLE, U32, C.POINTER(U32), HANDLE, U32], I32),
            "CreateToolhelp32Snapshot": ([U32, U32], HANDLE),
            "Thread32First": ([HANDLE, C.POINTER(ThreadEntry)], I32),
            "Thread32Next": ([HANDLE, C.POINTER(ThreadEntry)], I32),
            "GetProcessIdOfThread": ([HANDLE], U32),
            "GetThreadPriority": ([HANDLE], I32),
            "LocalFree": ([HANDLE], HANDLE),
        }
        for name, (arguments, result) in signatures.items():
            function = getattr(self.kernel, name)
            function.argtypes, function.restype = arguments, result
        self.powr.PowerGetActiveScheme.argtypes = [HANDLE, C.POINTER(HANDLE)]
        self.powr.PowerGetActiveScheme.restype = U32
        self.nt.NtQueryInformationProcess.argtypes = [HANDLE, U32, HANDLE, U32, C.POINTER(U32)]
        self.nt.NtQueryInformationProcess.restype = I32
        self.gdi.D3DKMTGetProcessSchedulingPriorityClass.argtypes = [HANDLE, C.POINTER(I32)]
        self.gdi.D3DKMTGetProcessSchedulingPriorityClass.restype = I32

    @staticmethod
    def checked(result, operation):
        if not result:
            raise VerificationError(f"{operation}: {C.WinError(C.get_last_error())}")
        return result

    @contextmanager
    def process(self, pid, access=0x100400):
        handle = self.checked(self.kernel.OpenProcess(access, False, pid), "OpenProcess")
        try:
            if self.kernel.WaitForSingleObject(handle, 0) != 258:
                raise VerificationError(f"Target {pid} is not running")
            yield handle
            if self.kernel.WaitForSingleObject(handle, 0) != 258:
                raise VerificationError(f"Target {pid} exited during verification")
        finally:
            self.kernel.CloseHandle(handle)

    def identity(self, handle, pid):
        path = C.create_unicode_buffer(32768)
        length = U32(len(path))
        self.checked(self.kernel.QueryFullProcessImageNameW(handle, 0, path, C.byref(length)),
                     "QueryFullProcessImageNameW")
        return {"pid": pid, "creation_filetime": self.creation(handle),
                "exe": path.value, "sha256": file_hash(path.value)}

    def creation(self, handle, thread=False):
        stamps = [FileTime() for _ in range(4)]
        function = self.kernel.GetThreadTimes if thread else self.kernel.GetProcessTimes
        self.checked(function(handle, *(C.byref(item) for item in stamps)), function.__name__)
        return stamps[0].integer()

    def active_power_guid(self):
        pointer = HANDLE()
        status = self.powr.PowerGetActiveScheme(None, C.byref(pointer))
        if status != 0 or not pointer:
            raise VerificationError(f"PowerGetActiveScheme failed: {status}")
        try:
            return str(uuid.UUID(bytes_le=C.string_at(pointer, 16)))
        finally:
            self.kernel.LocalFree(pointer)

    def cpu_sets(self, handle):
        required = U32()
        for _ in range(3):
            result = self.kernel.GetProcessDefaultCpuSets(handle, None, 0, C.byref(required))
            if not result and C.get_last_error() != 122:
                self.checked(result, "GetProcessDefaultCpuSets(size)")
            if not required.value:
                return []
            buffer = (U32 * required.value)()
            result = self.kernel.GetProcessDefaultCpuSets(handle, buffer, len(buffer), C.byref(required))
            if result:
                return sorted(buffer[:required.value])
            if C.get_last_error() != 122:
                self.checked(result, "GetProcessDefaultCpuSets")
        raise VerificationError("CPU Set selection changed repeatedly during readback")

    def cpu_set_catalog(self):
        required = U32()
        result = self.kernel.GetSystemCpuSetInformation(None, 0, C.byref(required), None, 0)
        if not result and C.get_last_error() != 122:
            self.checked(result, "GetSystemCpuSetInformation(size)")
        for _ in range(3):
            if not 0 < required.value <= 16 * 1024 * 1024:
                raise VerificationError("Invalid CPU Set inventory size")
            buffer = C.create_string_buffer(required.value)
            result = self.kernel.GetSystemCpuSetInformation(buffer, len(buffer), C.byref(required), None, 0)
            if result:
                return cpu_set_inventory(buffer.raw[:required.value])
            if C.get_last_error() != 122:
                self.checked(result, "GetSystemCpuSetInformation")
        raise VerificationError("CPU Set inventory changed repeatedly during readback")

    def threads(self, pid):
        snapshot = self.kernel.CreateToolhelp32Snapshot(4, 0)
        if snapshot == C.c_void_p(-1).value:
            self.checked(False, "CreateToolhelp32Snapshot")
        result = []
        try:
            entry = ThreadEntry()
            entry.size = C.sizeof(entry)
            success = self.kernel.Thread32First(snapshot, C.byref(entry))
            while success:
                if entry.pid == pid:
                    handle = self.kernel.OpenThread(0x100040, False, entry.tid)
                    if not handle:
                        if C.get_last_error() not in (6, 87):
                            self.checked(False, "OpenThread")
                    else:
                        try:
                            if (self.kernel.WaitForSingleObject(handle, 0) == 258 and
                                    self.kernel.GetProcessIdOfThread(handle) == pid):
                                priority = self.kernel.GetThreadPriority(handle)
                                if priority == 0x7fffffff:
                                    self.checked(False, "GetThreadPriority")
                                result.append({"tid": entry.tid,
                                               "creation_filetime": self.creation(handle, thread=True),
                                               "priority": priority})
                        finally:
                            self.kernel.CloseHandle(handle)
                success = self.kernel.Thread32Next(snapshot, C.byref(entry))
            if C.get_last_error() != 18:
                self.checked(False, "Thread32First/Thread32Next")
        finally:
            self.kernel.CloseHandle(snapshot)
        if not result:
            raise VerificationError(f"Target {pid} has no readable live threads")
        return sorted(result, key=lambda item: item["tid"])

    def properties(self, handle, pid, profile):
        priority = self.checked(self.kernel.GetPriorityClass(handle), "GetPriorityClass")
        power = PowerThrottling(1, 0, 0)
        self.checked(self.kernel.GetProcessInformation(handle, 4, C.byref(power), C.sizeof(power)),
                     "GetProcessInformation(ProcessPowerThrottling)")
        affinity, system_affinity = C.c_size_t(), C.c_size_t()
        self.checked(self.kernel.GetProcessAffinityMask(handle, C.byref(affinity), C.byref(system_affinity)),
                     "GetProcessAffinityMask")
        values = {"priority_class": priority,
                  "power_throttling": {name: getattr(power, name) for name, _ in power._fields_},
                  "affinity_mask": affinity.value, "system_affinity_mask": system_affinity.value}
        if profile == "dynamic-priority-boost":
            disabled = I32()
            self.checked(self.kernel.GetProcessPriorityBoost(handle, C.byref(disabled)),
                         "GetProcessPriorityBoost")
            values["priority_boost_disabled"] = bool(disabled.value)
        if profile in ("memory-priority", "combined"):
            memory = U32()
            self.checked(self.kernel.GetProcessInformation(handle, 0, C.byref(memory), C.sizeof(memory)),
                         "GetProcessInformation(ProcessMemoryPriority)")
            values["memory_priority"] = memory.value
        if profile in ("io-priority", "combined"):
            io = U32()
            status = self.nt.NtQueryInformationProcess(handle, 33, C.byref(io), C.sizeof(io), None)
            if status < 0:
                raise VerificationError(f"NtQueryInformationProcess(ProcessIoPriority): 0x{status & 0xffffffff:08x}")
            values["io_priority"] = io.value
        if profile in ("cpu-sets", "combined"):
            values["cpu_set_ids"] = self.cpu_sets(handle)
        if profile == "thread-priority":
            values["threads"] = self.threads(pid)
        if profile == "gpu-priority":
            gpu = I32()
            status = self.gdi.D3DKMTGetProcessSchedulingPriorityClass(handle, C.byref(gpu))
            if status == 0:
                values["gpu_priority"] = {"supported": True, "value": gpu.value}
            elif status & 0xffffffff in (0xc000000d, 0xc0000022):
                values["gpu_priority"] = {
                    "supported": False,
                    "reason": "no_gpu_context" if status & 0xffffffff == 0xc000000d else "native_query_access_denied",
                    "ntstatus": f"0x{status & 0xffffffff:08x}"}
            else:
                raise VerificationError(f"D3DKMTGetProcessSchedulingPriorityClass: 0x{status & 0xffffffff:08x}")
        return values


def capture_identity(native, pid):
    with native.process(pid) as handle:
        return native.identity(handle, pid)


def heartbeat(path, deadline=None):
    # Atomic replacement can briefly contend with antivirus/filesystem observers.
    started = time.monotonic()
    deadline = min(started + 0.25, deadline) if deadline is not None else started + 0.25
    if started >= deadline:
        raise VerificationError(f"Unreadable worker heartbeat {path}: read deadline expired")
    while True:
        try:
            value = int(Path(path).read_text(encoding="utf-8").strip())
            if value < 0:
                raise ValueError("negative elapsed time")
            return {"elapsed_ms": value, "mtime_ns": Path(path).stat().st_mtime_ns}
        except (OSError, ValueError) as error:
            winerror = getattr(error, "winerror", None)
            transient = (isinstance(error, OSError) and
                         (winerror in (5, 32, 33) or
                          (winerror is None and error.errno in (errno.EACCES, errno.EPERM))))
            remaining = deadline - time.monotonic()
            if transient and remaining > 0:
                time.sleep(min(0.02, remaining))
                if time.monotonic() < deadline:
                    continue
            raise VerificationError(f"Unreadable worker heartbeat {path}: {error}") from error


def capture_worker(native, worker, profile):
    # This Windows build requires SET_INFORMATION access even for the D3DKMT
    # getter (confirmed by the disposable-worker self-check); no setters run.
    access = 0x100600 if profile == "gpu-priority" else 0x100400
    with native.process(worker["pid"], access=access) as handle:
        identity = native.identity(handle, worker["pid"])
        verify_identity(worker, identity)
        values = native.properties(handle, worker["pid"], profile)
    return dict(identity, native=values, heartbeat=heartbeat(worker["heartbeat"]))


def restoration_differences(before, after):
    differences = []
    for field, expected in before.items():
        actual = after.get(field)
        if field == "threads":
            original = {(item["tid"], item["creation_filetime"]): item["priority"] for item in expected}
            surviving = {(item["tid"], item["creation_filetime"]): item["priority"] for item in actual}
            for identity in original.keys() & surviving.keys():
                if original[identity] != surviving[identity]:
                    differences.append(f"thread {identity[0]} priority: {surviving[identity]} != {original[identity]}")
        elif expected != actual:
            differences.append(f"{field}: {actual!r} != {expected!r}")
    return differences


def ready_differences(profile, before, after, expected_mask=None, expected_cpu_sets=None,
                      gpu_pending=False):
    expected = dict(before)
    if profile == "process-priority":
        expected["priority_class"] = 0x4000
    if profile in ("background-efficiency", "combined"):
        expected["priority_class"] = 0x40
        # Only execution-speed EcoQoS is deterministic; timer-resolution policy
        # depends on the independently observed audio workload.
        expected.pop("power_throttling")
    if profile == "dynamic-priority-boost":
        expected["priority_boost_disabled"] = True
    if profile in ("io-priority", "combined"):
        expected["io_priority"] = 1
    if profile in ("memory-priority", "combined"):
        expected["memory_priority"] = 2
    if profile == "processor-affinity":
        if type(expected_mask) is not int or expected_mask <= 0:
            raise VerificationError("Missing expected_core_mask for Processor Affinity")
        expected["affinity_mask"] = expected_mask
    if profile in ("cpu-sets", "combined"):
        if not expected_cpu_sets:
            raise VerificationError("Missing mapped expected CPU Set IDs")
        expected["cpu_set_ids"] = expected_cpu_sets
    if profile == "thread-priority":
        expected.pop("threads")
    if profile == "gpu-priority":
        expected.pop("gpu_priority")
    differences = restoration_differences(expected, after)
    if profile in ("background-efficiency", "combined"):
        power = after["power_throttling"]
        if not (power["control_mask"] & 1 and power["state_mask"] & 1):
            differences.append("Execution-speed EcoQoS is not controlled and enabled")
    if profile == "thread-priority" and (not after["threads"] or
                                         any(item["priority"] != -1 for item in after["threads"])):
        differences.append("Not every readable target thread has Below Normal priority")
    if profile == "gpu-priority":
        gpu = after["gpu_priority"]
        if gpu_pending and gpu != before["gpu_priority"]:
            differences.append("Pending GPU target unexpectedly changed native GPU priority")
        elif not gpu_pending and gpu["supported"] and gpu["value"] != 1:
            differences.append(f"GPU priority: {gpu['value']} != Below Normal (1)")
    return differences


def power_decision_differences(profile, runtime, original_guid, target):
    if profile not in ("by-activity", "by-running-app", "by-cpu-load", "by-time"):
        return []
    power = runtime.get("power_plan", {})
    differences = []
    for key in ("current_guid", "target_guid"):
        if str(power.get(key, "")).casefold() != original_guid.casefold():
            differences.append(f"Power decision {key} does not select the original active plan")
    if power.get("apply_failed") is not False:
        differences.append("Power decision reported an apply failure or lacks failure status")
    reason = power.get("reason") or ""
    if profile == "by-activity":
        matched = reason in ("User input has been idle past the configured timeout.",
                             "Recent user input detected; using the Active plan.")
    elif profile == "by-running-app":
        status = runtime.get("by_running_app", {})
        matched = (status.get("active_rule") == "Scheduler benchmark target" and
                   str(status.get("active_process", "")).casefold() == Path(target).name.casefold() and
                   str(status.get("target_guid", "")).casefold() == original_guid.casefold() and
                   "matched By Running App rule 'Scheduler benchmark target'." in reason)
    elif profile == "by-cpu-load":
        matched = reason.startswith("CPU load is ") and "matched rule 'Scheduler benchmark current plan'." in reason
    else:
        matched = reason == "By Time rule 'Scheduler benchmark current plan' is active."
    if not matched:
        differences.append(f"No matching {profile} decision was recorded")
    return differences


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    os.replace(temporary, path)


def verify_phase(native, phase, profile, trial_dir, manifest):
    validate_manifest(manifest)
    trial_dir = Path(trial_dir)
    original_guid = str(uuid.UUID(manifest["original_power_guid"]))
    active_guid = native.active_power_guid()
    if active_guid != original_guid:
        raise VerificationError(f"Active power plan changed: {active_guid} != {original_guid}")
    workers = manifest["workers"]
    current = [capture_worker(native, worker, profile) for worker in workers]
    result = {"schema_version": 1, "phase": phase, "profile": profile,
              "status": "verified", "active_action_verified": False,
              "active_power_guid": active_guid, "workers": current,
              "errors": [], "coverage_notes": []}
    if phase == "prepare":
        result["manifest"] = manifest
        return result
    baseline = json.loads((trial_dir / "scheduler-target-baseline.json").read_text(encoding="utf-8"))
    if baseline["profile"] != profile or baseline["manifest"] != manifest:
        raise VerificationError("Target manifest/profile differs from the prepared baseline")
    original = {item["pid"]: item for item in baseline["workers"]}
    for item in current:
        verify_identity(original[item["pid"]], item)
    if phase == "finished":
        fixture = json.loads((trial_dir / "finished.json").read_text(encoding="utf-8"))
        if fixture.get("scheduler_benchmark", {}).get("restoration_succeeded") is not True:
            result["errors"].append("Fixture did not report successful runtime/recovery restoration")
        for item in current:
            for difference in restoration_differences(original[item["pid"]]["native"], item["native"]):
                result["errors"].append(f"PID {item['pid']}: {difference}")
        if profile == "thread-priority":
            result["coverage_notes"].append("Restoration compared surviving exact thread identities; new/exited threads have no shared baseline")
        if profile == "app-suspension":
            start = {item["pid"]: item["heartbeat"]["elapsed_ms"] for item in current}
            pending = {worker["pid"]: worker for worker in workers}
            deadline = time.monotonic() + 3
            while pending and time.monotonic() < deadline:
                for pid, worker in list(pending.items()):
                    if heartbeat(worker["heartbeat"], deadline=deadline)["elapsed_ms"] > start[pid]:
                        del pending[pid]
                if pending:
                    time.sleep(0.1)
            result["heartbeat_recovery_verified"] = not pending
            if pending:
                result["errors"].append(f"Workers did not resume heartbeat after cleanup: {sorted(pending)}")
    else:
        fixture = json.loads((trial_dir / "ready.json").read_text(encoding="utf-8"))
        contract = fixture.get("scheduler_benchmark", {})
        if (contract.get("profile") != profile or
                contract.get("worker_running") is not (profile != "disabled")):
            result["errors"].append("Fixture scheduler profile/worker is not ready")
        runtime = fixture.get("runtime_status", {})
        result["reported_runtime_status"] = runtime
        if runtime.get("worker_error") is not None:
            result["errors"].append(f"Runtime worker error: {runtime['worker_error']}")
        power_errors = power_decision_differences(profile, runtime, original_guid, manifest["target_executable"])
        result["errors"].extend(power_errors)
        if profile in ("by-activity", "by-running-app", "by-cpu-load", "by-time"):
            result["power_decision_verified"] = not power_errors
            result["coverage_notes"].append("Selected rule/decision verified against the existing plan; no power-plan transition claimed")
        gpu_status = runtime.get("gpu_priority", {})
        gpu_pending = (profile == "gpu-priority" and gpu_status.get("pending", 0) >= len(workers)
                       and gpu_status.get("adjusted", 0) == 0)
        mask = contract.get("expected_core_mask")
        selected_ids = None
        if profile in ("cpu-sets", "combined"):
            catalog = native.cpu_set_catalog()
            selected_ids = cpu_set_ids_for_mask(catalog, mask)
            result["cpu_set_inventory"] = catalog
            result["expected_cpu_set_ids"] = selected_ids
        for item in current:
            for difference in ready_differences(profile, original[item["pid"]]["native"],
                                                 item["native"], mask, selected_ids, gpu_pending):
                result["errors"].append(f"PID {item['pid']}: {difference}")
        action_profiles = {"process-priority", "thread-priority", "dynamic-priority-boost",
                           "io-priority", "memory-priority", "background-efficiency",
                           "cpu-sets", "processor-affinity", "combined"}
        result["active_action_verified"] = profile in action_profiles
        if result["active_action_verified"]:
            changed = [item["pid"] for item in current
                       if item["native"] != original[item["pid"]]["native"]]
            result["native_change_observed_pids"] = changed
            if not changed:
                result["active_action_verified"] = False
                result["coverage_notes"].append("Desired native state was already present at baseline; no mutation observed")
        if profile == "gpu-priority":
            result["active_action_verified"] = (not gpu_pending and
                all(item["native"]["gpu_priority"]["supported"] for item in current))
            if gpu_pending:
                result["coverage_notes"].append("All targets reported pending; unchanged native GPU priority verified, no active GPU action claimed")
            if not result["active_action_verified"]:
                result["coverage_notes"].append("Native GPU scheduling action is unverified; see pending status and each readback/NTSTATUS")
        if profile == "app-suspension":
            suspended = set(runtime.get("app_suspension", {}).get("suspended_process_ids", []))
            pids = {worker["pid"] for worker in workers}
            time.sleep(1.25)
            stopped = all(heartbeat(worker["heartbeat"])["elapsed_ms"] == item["heartbeat"]["elapsed_ms"]
                          for worker, item in zip(workers, current))
            result["heartbeat_suspension_verified"] = stopped and pids <= suspended
            result["active_action_verified"] = result["heartbeat_suspension_verified"]
            if not result["active_action_verified"]:
                result["errors"].append("App Suspension did not report all workers suspended with stopped heartbeats")
        if profile == "cpu-limiter":
            result["coverage_notes"].append("CPU Limiter native duty cycle is not independently proven by heartbeat or reported status")
            result["reported_limiter"] = runtime.get("cpu_limiter")
        if profile not in action_profiles | {"gpu-priority", "app-suspension"}:
            result["coverage_notes"].append("Control/decision-only workload; no active native process mutation claimed")
        if not result["active_action_verified"]:
            result["status"] = "status_only"
    if result["errors"]:
        result["status"] = "failed"
        result["active_action_verified"] = False
    if phase == "finished":
        result["restoration_verified"] = not result["errors"]
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--phase", choices=("prepare", "ready", "finished"), required=True)
    parser.add_argument("--profile", choices=sorted(PROFILES), required=True)
    parser.add_argument("--trial-dir", type=Path, required=True)
    parser.add_argument("--targets", type=Path, required=True)
    args = parser.parse_args()
    filename = "baseline" if args.phase == "prepare" else args.phase
    output = args.trial_dir / f"scheduler-target-{filename}.json"
    try:
        manifest = json.loads(args.targets.read_text(encoding="utf-8"))
        result = verify_phase(Native(), args.phase, args.profile, args.trial_dir, manifest)
    except (OSError, ValueError, KeyError, TypeError, VerificationError) as error:
        result = {"schema_version": 1, "phase": args.phase, "profile": args.profile,
                  "status": "failed", "active_action_verified": False, "errors": [str(error)]}
    write_json(output, result)
    print(json.dumps(result, separators=(",", ":")))
    if result["status"] == "failed":
        print("; ".join(result["errors"]), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
