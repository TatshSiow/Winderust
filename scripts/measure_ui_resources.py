"""Windows process-resource comparison for isolated, self-terminating UI fixtures.

The JSON configuration supplies targets (name, exe, args, renderer) and scenarios
(name, window, args, width, height). Argument templates may use {lifetime_ms},
{trial_dir}, {events}, {stop_file}, {width}, {height}, and {scenario}. Fixtures disable
automation by default. Enabled scheduler fixtures require --allow-enabled-automation
and a scenario automation_contract with exact ready/finished safety assertions.
All fixtures must avoid user-settings writes and exit cooperatively. This tool never
terminates the measured application. Optional JSONL events named ready/shutdown_started improve
timing; a configured stop_file is created after sampling to request clean exit.

CPU percentages use one logical core; cycles are QueryProcessCycleTime counts,
not instructions or a frequency-independent work measurement. Page faults include
soft faults. Process I/O includes cached/non-disk I/O. Combined working sets can
count shared pages more than once. GPU utilization is the busiest observed engine;
missing GPU instances are unavailable, never invented zeros. GPU Process Memory
can overreport on affected Windows drivers/builds (Microsoft KB 4490156).
"""
import argparse
import csv
import ctypes as C
from ctypes import wintypes as W
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import sys
import time


class Memory(C.Structure):
    _fields_ = [("cb", W.DWORD), ("PageFaultCount", W.DWORD)] + [
        (name, C.c_size_t) for name in (
            "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
            "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
            "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage",
            "PrivateUsage")]


class MemoryEx2(C.Structure):
    _fields_ = Memory._fields_ + [("PrivateWorkingSetSize", C.c_size_t),
                                 ("SharedCommitUsage", C.c_ulonglong)]


def read_memory(api, handle):
    if not getattr(api, "memory_ex2_enabled", False):
        value = Memory()
        value.cb = C.sizeof(value)
        checked(api.psapi.GetProcessMemoryInfo(handle, C.byref(value), value.cb))
        return value, {"private_working_set_bytes": None, "shared_commit_bytes": None,
                       "memory_api_status": "measured_ex_ex2_disabled"}
    extended = MemoryEx2()
    extended.cb = C.sizeof(extended)
    extended.PrivateWorkingSetSize = C.c_size_t(-1).value
    extended.SharedCommitUsage = C.c_ulonglong(-1).value
    status = "ex2_not_populated"
    if api.psapi.GetProcessMemoryInfo(handle, C.byref(extended), extended.cb):
        if (extended.cb >= C.sizeof(extended) and
                extended.PrivateWorkingSetSize != C.c_size_t(-1).value and
                extended.SharedCommitUsage != C.c_ulonglong(-1).value and
                extended.PrivateWorkingSetSize <= extended.WorkingSetSize):
            return extended, {"private_working_set_bytes": extended.PrivateWorkingSetSize,
                              "shared_commit_bytes": extended.SharedCommitUsage,
                              "memory_api_status": "measured_ex2"}
    else:
        status = f"ex2_error_{C.get_last_error()}"
    value = Memory()
    value.cb = C.sizeof(value)
    checked(api.psapi.GetProcessMemoryInfo(handle, C.byref(value), value.cb))
    return value, {"private_working_set_bytes": None, "shared_commit_bytes": None,
                   "memory_api_status": "ex_fallback_" + status}


class IoCounters(C.Structure):
    _fields_ = [(name, C.c_ulonglong) for name in (
        "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
        "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]


class ProcessEntry(C.Structure):
    _fields_ = [("dwSize", W.DWORD), ("cntUsage", W.DWORD),
                ("th32ProcessID", W.DWORD), ("th32DefaultHeapID", C.c_size_t),
                ("th32ModuleID", W.DWORD), ("cntThreads", W.DWORD),
                ("th32ParentProcessID", W.DWORD), ("pcPriClassBase", W.LONG),
                ("dwFlags", W.DWORD), ("szExeFile", W.WCHAR * 260)]


class ThreadEntry(C.Structure):
    _fields_ = [("dwSize", W.DWORD), ("cntUsage", W.DWORD),
                ("th32ThreadID", W.DWORD), ("th32OwnerProcessID", W.DWORD),
                ("tpBasePri", W.LONG), ("tpDeltaPri", W.LONG), ("dwFlags", W.DWORD)]


class CounterValue(C.Structure):
    _fields_ = [("status", W.DWORD), ("value", C.c_double)]


class CounterItem(C.Structure):
    _fields_ = [("name", W.LPWSTR), ("value", CounterValue)]


class MemoryStatus(C.Structure):
    _fields_ = [("length", W.DWORD), ("load_percent", W.DWORD)] + [
        (name, C.c_ulonglong) for name in ("total_phys", "available_phys", "total_commit",
            "available_commit", "total_virtual", "available_virtual", "available_extended_virtual")]


class PowerStatus(C.Structure):
    _fields_ = [("ac", W.BYTE), ("battery_flags", W.BYTE), ("battery_percent", W.BYTE),
                ("system_status", W.BYTE), ("battery_life_s", W.DWORD), ("battery_full_life_s", W.DWORD)]


class WindowPlacement(C.Structure):
    _fields_ = [("length", W.UINT), ("flags", W.UINT), ("show_command", W.UINT),
                ("minimum", W.POINT), ("maximum", W.POINT), ("normal", W.RECT)]


class SessionProcess(C.Structure):
    _fields_ = [("session_id", W.DWORD), ("pid", W.DWORD),
                ("name", W.LPWSTR), ("user_sid", C.c_void_p)]


def checked(value):
    if not value:
        raise C.WinError(C.get_last_error())
    return value


def filetime(value):
    return (value.dwHighDateTime << 32) | value.dwLowDateTime


WINDOW_ENUMERATION_ANCHOR = {
    "enabled": True, "owner": "sampler", "class": "STATIC", "style": 0x88000000, "ex_style": 0,
    "visibility": "never_shown", "activation_requested": False, "creation": "lazy_before_first_windows_call",
    "validation": "EnumWindows_TRUE_and_exact_anchor_callback_each_call", "overhead_scope": "sampler"}


class WindowEnumerationAnchor:
    def __init__(self, api):
        self.api, self.hwnd, self.thread_id = api, None, None
        self.successful_enumerations = 0

    def ensure(self):
        thread = self.api.kernel.GetCurrentThreadId()
        if not thread:
            raise RuntimeError("Cannot identify window enumeration anchor owner thread")
        if self.hwnd is None:
            instance = checked(self.api.kernel.GetModuleHandleW(None))
            hwnd = self.api.user.CreateWindowExW(0, "STATIC", "", 0x88000000, 0, 0, 0, 0,
                                                None, None, instance, None)
            if not hwnd:
                raise RuntimeError(f"Cannot create window enumeration anchor (Win32 error {C.get_last_error()})")
            self.hwnd, self.thread_id = hwnd, thread
        if thread != self.thread_id:
            raise RuntimeError("Window enumeration anchor used outside its creating thread")
        owner = W.DWORD()
        actual_thread = self.api.user.GetWindowThreadProcessId(self.hwnd, C.byref(owner))
        name = C.create_unicode_buffer(256)
        length = self.api.user.GetClassNameW(self.hwnd, name, len(name))
        if actual_thread != self.thread_id or owner.value != os.getpid() or not length or name.value.casefold() != "static":
            raise RuntimeError("Window enumeration anchor owner/class validation failed")
        if self.api.user.IsWindowVisible(self.hwnd) or self.api.user.IsWindowEnabled(self.hwnd):
            raise RuntimeError("Window enumeration anchor must remain hidden and disabled")
        if self.api.user.GetForegroundWindow() == self.hwnd:
            raise RuntimeError("Window enumeration anchor unexpectedly became foreground")
        return self.hwnd

    def metadata(self, before):
        if self.hwnd is None or self.successful_enumerations <= before:
            raise RuntimeError("Window enumeration anchor has no successful trial enumeration")
        self.ensure()
        return {**WINDOW_ENUMERATION_ANCHOR, "pid": os.getpid(), "thread_id": self.thread_id, "hwnd": self.hwnd,
                "is_visible": False, "is_enabled": False, "is_foreground": False,
                "enumerations_before_trial": before, "successful_enumerations": self.successful_enumerations,
                "successful_enumerations_during_trial": self.successful_enumerations - before,
                "exact_anchor_callbacks": self.successful_enumerations}

    def close(self):
        if self.hwnd is not None:
            if self.api.kernel.GetCurrentThreadId() != self.thread_id:
                raise RuntimeError("Window enumeration anchor must be destroyed by its creating thread")
            if not self.api.user.DestroyWindow(self.hwnd):
                raise RuntimeError(f"Cannot destroy window enumeration anchor (Win32 error {C.get_last_error()})")
            self.hwnd, self.thread_id = None, None


class Windows:
    def __init__(self, memory_ex2=False, thread_cycles=False):
        if os.name != "nt":
            raise RuntimeError("This sampler requires Windows")
        self.kernel = C.WinDLL("kernel32", use_last_error=True)
        self.memory_ex2_enabled = memory_ex2
        self.user = C.WinDLL("user32", use_last_error=True)
        self.psapi = C.WinDLL("psapi", use_last_error=True)
        self.wts = C.WinDLL("wtsapi32", use_last_error=True)
        k, u = self.kernel, self.user
        k.GetModuleHandleW.argtypes, k.GetModuleHandleW.restype = [W.LPCWSTR], W.HMODULE
        self.wts.WTSEnumerateProcessesW.argtypes = [W.HANDLE, W.DWORD, W.DWORD,
            C.POINTER(C.POINTER(SessionProcess)), C.POINTER(W.DWORD)]
        self.wts.WTSEnumerateProcessesW.restype = W.BOOL
        self.wts.WTSFreeMemory.argtypes, self.wts.WTSFreeMemory.restype = [C.c_void_p], None
        u.SetProcessDpiAwarenessContext.argtypes = [W.HANDLE]
        u.SetProcessDpiAwarenessContext.restype = W.BOOL
        self.dpi_awareness_set = bool(u.SetProcessDpiAwarenessContext(W.HANDLE(-4)))
        self.compositor_pid = None
        k.OpenProcess.argtypes, k.OpenProcess.restype = [W.DWORD, W.BOOL, W.DWORD], W.HANDLE
        k.CloseHandle.argtypes, k.CloseHandle.restype = [W.HANDLE], W.BOOL
        k.GetProcessTimes.argtypes = [W.HANDLE] + [C.POINTER(W.FILETIME)] * 4
        k.GetProcessTimes.restype = W.BOOL
        k.GetSystemTimes.argtypes = [C.POINTER(W.FILETIME)] * 3
        k.GetSystemTimes.restype = W.BOOL
        k.GlobalMemoryStatusEx.argtypes, k.GlobalMemoryStatusEx.restype = [C.POINTER(MemoryStatus)], W.BOOL
        k.GetSystemPowerStatus.argtypes, k.GetSystemPowerStatus.restype = [C.POINTER(PowerStatus)], W.BOOL
        k.QueryProcessCycleTime.argtypes = [W.HANDLE, C.POINTER(C.c_ulonglong)]
        k.QueryProcessCycleTime.restype = W.BOOL
        k.GetProcessIoCounters.argtypes = [W.HANDLE, C.POINTER(IoCounters)]
        k.GetProcessIoCounters.restype = W.BOOL
        k.GetProcessHandleCount.argtypes = [W.HANDLE, C.POINTER(W.DWORD)]
        k.GetProcessHandleCount.restype = W.BOOL
        k.WaitForSingleObject.argtypes, k.WaitForSingleObject.restype = [W.HANDLE, W.DWORD], W.DWORD
        k.ProcessIdToSessionId.argtypes, k.ProcessIdToSessionId.restype = [W.DWORD, C.POINTER(W.DWORD)], W.BOOL
        k.CreateToolhelp32Snapshot.argtypes = [W.DWORD, W.DWORD]
        k.CreateToolhelp32Snapshot.restype = W.HANDLE
        for name in ("Process32FirstW", "Process32NextW"):
            fn = getattr(k, name)
            fn.argtypes, fn.restype = [W.HANDLE, C.POINTER(ProcessEntry)], W.BOOL
        if thread_cycles:
            for name in ("Thread32First", "Thread32Next"):
                fn = getattr(k, name)
                fn.argtypes, fn.restype = [W.HANDLE, C.POINTER(ThreadEntry)], W.BOOL
            k.OpenThread.argtypes, k.OpenThread.restype = [W.DWORD, W.BOOL, W.DWORD], W.HANDLE
            k.GetProcessIdOfThread.argtypes, k.GetProcessIdOfThread.restype = [W.HANDLE], W.DWORD
            k.GetThreadTimes.argtypes = [W.HANDLE] + [C.POINTER(W.FILETIME)] * 4
            k.GetThreadTimes.restype = W.BOOL
            k.GetExitCodeThread.argtypes = [W.HANDLE, C.POINTER(W.DWORD)]
            k.GetExitCodeThread.restype = W.BOOL
            k.QueryThreadCycleTime.argtypes = [W.HANDLE, C.POINTER(C.c_ulonglong)]
            k.QueryThreadCycleTime.restype = W.BOOL
            if getattr(k, "GetThreadDescription", None) is not None:
                k.GetThreadDescription.argtypes = [W.HANDLE, C.POINTER(W.LPWSTR)]
                k.GetThreadDescription.restype = W.LONG  # HRESULT is signed.
            k.LocalFree.argtypes, k.LocalFree.restype = [C.c_void_p], C.c_void_p
        self.psapi.GetProcessMemoryInfo.argtypes = [W.HANDLE, C.c_void_p, W.DWORD]
        self.psapi.GetProcessMemoryInfo.restype = W.BOOL
        u.GetGuiResources.argtypes, u.GetGuiResources.restype = [W.HANDLE, W.DWORD], W.DWORD
        self.callback = C.WINFUNCTYPE(W.BOOL, W.HWND, W.LPARAM)
        u.EnumWindows.argtypes, u.EnumWindows.restype = [self.callback, W.LPARAM], W.BOOL
        u.GetWindowThreadProcessId.argtypes = [W.HWND, C.POINTER(W.DWORD)]
        u.GetWindowThreadProcessId.restype = W.DWORD
        u.GetClassNameW.argtypes = [W.HWND, W.LPWSTR, C.c_int]
        u.GetWindowTextLengthW.argtypes, u.GetWindowTextLengthW.restype = [W.HWND], C.c_int
        u.GetWindowTextW.argtypes, u.GetWindowTextW.restype = [W.HWND, W.LPWSTR, C.c_int], C.c_int
        u.IsWindowVisible.argtypes, u.IsWindowVisible.restype = [W.HWND], W.BOOL
        u.IsWindowEnabled.argtypes, u.IsWindowEnabled.restype = [W.HWND], W.BOOL
        u.CreateWindowExW.argtypes = [W.DWORD, W.LPCWSTR, W.LPCWSTR, W.DWORD, C.c_int, C.c_int, C.c_int, C.c_int,
                                     W.HWND, W.HMENU, W.HINSTANCE, C.c_void_p]
        u.CreateWindowExW.restype = W.HWND
        u.DestroyWindow.argtypes, u.DestroyWindow.restype = [W.HWND], W.BOOL
        u.IsIconic.argtypes, u.IsIconic.restype = [W.HWND], W.BOOL
        u.ShowWindowAsync.argtypes, u.ShowWindowAsync.restype = [W.HWND, C.c_int], W.BOOL
        u.PostMessageW.argtypes = [W.HWND, W.UINT, W.WPARAM, W.LPARAM]
        u.PostMessageW.restype = W.BOOL
        u.SetWindowPos.argtypes = [W.HWND, W.HWND, C.c_int, C.c_int, C.c_int, C.c_int, W.UINT]
        u.SetWindowPos.restype = W.BOOL
        u.GetWindowRect.argtypes, u.GetWindowRect.restype = [W.HWND, C.POINTER(W.RECT)], W.BOOL
        u.GetClientRect.argtypes, u.GetClientRect.restype = [W.HWND, C.POINTER(W.RECT)], W.BOOL
        u.ClientToScreen.argtypes, u.ClientToScreen.restype = [W.HWND, C.POINTER(W.POINT)], W.BOOL
        u.GetDpiForWindow.argtypes, u.GetDpiForWindow.restype = [W.HWND], W.UINT
        u.GetWindowPlacement.argtypes, u.GetWindowPlacement.restype = [W.HWND, C.POINTER(WindowPlacement)], W.BOOL
        u.SetForegroundWindow.argtypes, u.SetForegroundWindow.restype = [W.HWND], W.BOOL
        u.GetForegroundWindow.argtypes, u.GetForegroundWindow.restype = [], W.HWND
        u.GetCursorPos.argtypes, u.GetCursorPos.restype = [C.POINTER(W.POINT)], W.BOOL
        u.SetCursorPos.argtypes, u.SetCursorPos.restype = [C.c_int, C.c_int], W.BOOL
        u.GetAsyncKeyState.argtypes, u.GetAsyncKeyState.restype = [C.c_int], W.SHORT
        u.AttachThreadInput.argtypes, u.AttachThreadInput.restype = [W.DWORD, W.DWORD, W.BOOL], W.BOOL
        u.BringWindowToTop.argtypes, u.BringWindowToTop.restype = [W.HWND], W.BOOL
        k.GetCurrentThreadId.argtypes, k.GetCurrentThreadId.restype = [], W.DWORD
        self.window_anchor = WindowEnumerationAnchor(self)

    def close_window_anchor(self):
        self.window_anchor.close()

    def close(self, handle):
        checked(self.kernel.CloseHandle(handle))

    def system_context(self):
        values = [W.FILETIME() for _ in range(3)]
        checked(self.kernel.GetSystemTimes(*(C.byref(value) for value in values)))
        mem, power = MemoryStatus(), PowerStatus()
        mem.length = C.sizeof(mem)
        checked(self.kernel.GlobalMemoryStatusEx(C.byref(mem)))
        checked(self.kernel.GetSystemPowerStatus(C.byref(power)))
        return {"system_idle_s": filetime(values[0]) / 10_000_000,
                "system_kernel_s": filetime(values[1]) / 10_000_000,
                "system_user_s": filetime(values[2]) / 10_000_000,
                "system_available_physical_bytes": mem.available_phys,
                "system_available_commit_bytes": mem.available_commit,
                "system_memory_load_percent": mem.load_percent,
                "system_ac_line_status": {0: "offline", 1: "online", 255: "unknown"}.get(power.ac, "unknown"),
                "system_battery_percent": power.battery_percent if power.battery_percent <= 100 else None}

    def snapshot(self):
        handle = self.kernel.CreateToolhelp32Snapshot(2, 0)
        if handle == C.c_void_p(-1).value:
            raise C.WinError(C.get_last_error())
        result = {}
        current_session = W.DWORD()
        checked(self.kernel.ProcessIdToSessionId(os.getpid(), C.byref(current_session)))
        self.compositor_pid = None
        try:
            entry = ProcessEntry()
            entry.dwSize = C.sizeof(entry)
            ok = self.kernel.Process32FirstW(handle, C.byref(entry))
            while ok:
                # Discard executable names and all non-target details after use.
                result[entry.th32ProcessID] = (entry.th32ParentProcessID, entry.cntThreads)
                if entry.szExeFile.lower() == "dwm.exe":
                    session = W.DWORD()
                    if self.kernel.ProcessIdToSessionId(entry.th32ProcessID, C.byref(session)) and session.value == current_session.value:
                        self.compositor_pid = entry.th32ProcessID
                ok = self.kernel.Process32NextW(handle, C.byref(entry))
            if C.get_last_error() != 18:  # ERROR_NO_MORE_FILES
                raise C.WinError(C.get_last_error())
            if self.compositor_pid is None:
                self.compositor_pid = self.session_compositor(current_session.value)
            return result
        finally:
            self.close(handle)

    def thread_snapshot(self, owners):
        """Discard non-target thread entries before opening or querying any thread."""
        handle = self.kernel.CreateToolhelp32Snapshot(4, 0)  # TH32CS_SNAPTHREAD
        if handle == C.c_void_p(-1).value:
            raise C.WinError(C.get_last_error())
        try:
            result, entry = [], ThreadEntry()
            entry.dwSize = C.sizeof(entry)
            ok = self.kernel.Thread32First(handle, C.byref(entry))
            while ok:
                if entry.th32OwnerProcessID in owners:
                    result.append((entry.th32ThreadID, entry.th32OwnerProcessID))
                entry.dwSize = C.sizeof(entry)
                ok = self.kernel.Thread32Next(handle, C.byref(entry))
            if C.get_last_error() != 18:  # ERROR_NO_MORE_FILES
                raise C.WinError(C.get_last_error())
            return result
        finally:
            self.close(handle)

    def session_compositor(self, session_id):
        entries, count = C.POINTER(SessionProcess)(), W.DWORD()
        if not self.wts.WTSEnumerateProcessesW(None, 0, 1, C.byref(entries), C.byref(count)):
            return None
        try:
            return next((entries[i].pid for i in range(count.value)
                         if entries[i].session_id == session_id and
                         entries[i].name and entries[i].name.lower() == "dwm.exe"), None)
        finally:
            self.wts.WTSFreeMemory(entries)

    def windows(self, pid, window_class=None, details=False):
        anchor = self.window_anchor.ensure()
        found = []
        anchor_callbacks = 0

        @self.callback
        def visit(hwnd, _):
            nonlocal anchor_callbacks
            if hwnd == anchor:
                anchor_callbacks += 1
                return True
            owner = W.DWORD()
            self.user.GetWindowThreadProcessId(hwnd, C.byref(owner))
            if owner.value == pid:
                name = C.create_unicode_buffer(256)
                self.user.GetClassNameW(hwnd, name, len(name))
                if ((isinstance(window_class, int) and hwnd == window_class) or
                        (isinstance(window_class, str) and name.value == window_class) or
                        window_class is None):
                    visible, iconic = bool(self.user.IsWindowVisible(hwnd)), bool(self.user.IsIconic(hwnd))
                    if details:
                        C.set_last_error(0)
                        length = self.user.GetWindowTextLengthW(hwnd)
                        length_error = C.get_last_error() if not length else 0
                        title = C.create_unicode_buffer(length + 1)
                        C.set_last_error(0)
                        copied = self.user.GetWindowTextW(hwnd, title, len(title))
                        title_error = C.get_last_error() if not copied else 0
                        found.append({"hwnd": hwnd, "pid": owner.value, "class": name.value,
                                      "title": title.value, "visible": visible, "iconic": iconic,
                                      "title_length_win32_error": length_error,
                                      "title_read_win32_error": title_error})
                    else:
                        found.append((hwnd, visible, iconic))
            return True

        C.set_last_error(0)
        if not self.user.EnumWindows(visit, 0):
            raise RuntimeError(f"EnumWindows failed (Win32 error {C.get_last_error()}); successful anchor enumeration required")
        if anchor_callbacks != 1:
            raise RuntimeError(f"EnumWindows did not encounter the exact anchor once (callbacks={anchor_callbacks})")
        self.window_anchor.successful_enumerations += 1
        return found


class Process:
    def __init__(self, api, pid, role, parent=None):
        self.api, self.pid, self.role, self.parent = api, pid, role, parent
        self.handle = checked(api.kernel.OpenProcess(0x100000 | 0x400 | 0x10, False, pid))
        values = [W.FILETIME() for _ in range(4)]
        try:
            checked(api.kernel.GetProcessTimes(self.handle, *(C.byref(v) for v in values)))
            self.created = filetime(values[0])
        except BaseException:
            api.close(self.handle)
            raise

    def alive(self):
        status = self.api.kernel.WaitForSingleObject(self.handle, 0)
        if status not in (0, 258):
            raise C.WinError(C.get_last_error())
        return status == 258

    def read(self, threads=None):
        k, u = self.api.kernel, self.api.user
        values = [W.FILETIME() for _ in range(4)]
        checked(k.GetProcessTimes(self.handle, *(C.byref(v) for v in values)))
        cycles, handles = C.c_ulonglong(), W.DWORD()
        checked(k.QueryProcessCycleTime(self.handle, C.byref(cycles)))
        checked(k.GetProcessHandleCount(self.handle, C.byref(handles)))
        mem, extended_memory = read_memory(self.api, self.handle)
        io = IoCounters()
        checked(k.GetProcessIoCounters(self.handle, C.byref(io)))
        row = {"pid": self.pid, "role": self.role,
               "created_filetime": self.created, "threads": threads,
               "user_s": filetime(values[3]) / 10_000_000,
               "kernel_s": filetime(values[2]) / 10_000_000,
               "cycles": cycles.value, "handles": handles.value,
               "private_bytes": mem.PrivateUsage, "working_set_bytes": mem.WorkingSetSize,
               "peak_working_set_bytes": mem.PeakWorkingSetSize,
               "commit_bytes": mem.PagefileUsage, "peak_commit_bytes": mem.PeakPagefileUsage,
               "paged_pool_bytes": mem.QuotaPagedPoolUsage,
               "nonpaged_pool_bytes": mem.QuotaNonPagedPoolUsage,
               "page_faults": mem.PageFaultCount}
        row.update(extended_memory)
        for kind in ("Read", "Write", "Other"):
            row[f"io_{kind.lower()}_ops"] = getattr(io, kind + "OperationCount")
            row[f"io_{kind.lower()}_bytes"] = getattr(io, kind + "TransferCount")
        for name, flag in (("gdi_objects", 0), ("user_objects", 1),
                           ("peak_gdi_objects", 2), ("peak_user_objects", 4)):
            C.set_last_error(0)
            count = u.GetGuiResources(self.handle, flag)
            row[name] = count if count or C.get_last_error() == 0 else None
        return row

    def close(self):
        self.api.close(self.handle)

    def lifetime(self):
        values = [W.FILETIME() for _ in range(4)]
        checked(self.api.kernel.GetProcessTimes(self.handle, *(C.byref(v) for v in values)))
        cycles, io = C.c_ulonglong(), IoCounters()
        checked(self.api.kernel.QueryProcessCycleTime(self.handle, C.byref(cycles)))
        checked(self.api.kernel.GetProcessIoCounters(self.handle, C.byref(io)))
        return {"user_s": filetime(values[3]) / 10_000_000,
                "kernel_s": filetime(values[2]) / 10_000_000, "cycles": cycles.value,
                "created_filetime": filetime(values[0]), "exited_filetime": filetime(values[1]),
                "io_read_ops": io.ReadOperationCount, "io_write_ops": io.WriteOperationCount,
                "io_other_ops": io.OtherOperationCount, "io_read_bytes": io.ReadTransferCount,
                "io_write_bytes": io.WriteTransferCount, "io_other_bytes": io.OtherTransferCount}


class ThreadCycles:
    """Polling diagnostic for exact thread instances in already-validated processes.

    Every handle is closed within a sample. Missing observations break delta
    continuity; neither thread names nor matching numeric IDs establish identity.
    """
    def __init__(self, api):
        self.api, self.previous, self.known, self.active = api, {}, {}, set()
        self.sample_count = 0

    def row(self, process, tid, sampling_start):
        old = self.known.get((process.pid, process.created, tid), {})
        wall_ns = time.time_ns()
        return {"role": process.role, "pid": process.pid,
                "process_created_filetime": process.created, "tid": tid,
                "thread_created_filetime": old.get("thread_created_filetime"),
                "identity_status": "previous_observation_only" if old else "unavailable",
                "description": old.get("description"),
                "description_status": "previous_observation_only" if old else "unavailable",
                "sample_index": self.sample_count - 1,
                "sampled_unix_ns": wall_ns,
                "sampled_utc": time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(wall_ns // 10**9)) +
                               f".{wall_ns % 10**9:09d}Z",
                "elapsed_s": time.perf_counter() - sampling_start,
                "cycles": None, "cycles_delta": None, "cycles_per_s": None,
                "delta_elapsed_s": None, "thread_cycle_status": "unavailable"}

    @staticmethod
    def api_error(row, operation):
        error = C.get_last_error()
        row.update(thread_cycle_status="unavailable_access_denied" if error == 5 else
                   f"unavailable_{operation}_error", win32_error=error, error_operation=operation)

    def describe(self, handle, row):
        description = getattr(self.api.kernel, "GetThreadDescription", None)
        row.update(description=None, description_status="unavailable_api")
        if description is None:
            return
        text = W.LPWSTR()
        status = description(handle, C.byref(text))
        try:
            if status >= 0:
                row.update(description=text.value or "", description_status="measured")
            else:
                row["description_status"] = f"unavailable_hresult_0x{status & 0xffffffff:08x}"
        finally:
            if text and self.api.kernel.LocalFree(C.cast(text, C.c_void_p)):
                row["description_status"] = "unavailable_local_free_error"

    def read(self, process, tid, sampling_start):
        row = self.row(process, tid, sampling_start)
        if not process.alive():
            row["thread_cycle_status"] = "unavailable_process_exited"
            return row
        k = self.api.kernel
        handle = k.OpenThread(0x0800, False, tid)  # THREAD_QUERY_LIMITED_INFORMATION
        if not handle:
            self.api_error(row, "open_thread")
            return row
        try:
            owner = k.GetProcessIdOfThread(handle)
            if not owner:
                self.api_error(row, "owner_query")
                return row
            if owner != process.pid:
                row.update(thread_cycle_status="unavailable_owner_changed", observed_owner_pid=owner)
                return row
            values = [W.FILETIME() for _ in range(4)]
            if not k.GetThreadTimes(handle, *(C.byref(value) for value in values)):
                self.api_error(row, "thread_times")
                return row
            created = filetime(values[0])
            # The retained process handle, not just its PID, must still be alive.
            if not process.alive():
                row["thread_cycle_status"] = "unavailable_process_exited"
                return row
            row.update(thread_created_filetime=created, identity_status="verified",
                       description=None, description_status="unavailable")
            if created < process.created:
                row["thread_cycle_status"] = "unavailable_thread_predates_process"
                return row
            # GetThreadTimes' exit FILETIME is undefined for a live thread.
            exit_code = W.DWORD()
            if not k.GetExitCodeThread(handle, C.byref(exit_code)):
                self.api_error(row, "thread_exit_code")
                return row
            if exit_code.value != 259:  # STILL_ACTIVE; 259 itself is ambiguous if used as an exit code.
                row["thread_cycle_status"] = "unavailable_exited"
                return row
            cycles = C.c_ulonglong()
            if not k.QueryThreadCycleTime(handle, C.byref(cycles)):
                self.api_error(row, "thread_cycles")
                return row
            row.update(cycles=cycles.value, elapsed_s=time.perf_counter() - sampling_start,
                       thread_cycle_status="observed")
            self.describe(handle, row)
            return row
        finally:
            self.api.close(handle)

    def sample(self, processes, sampling_start):
        # Callers pass only Process handles admitted by the existing descendant
        # creation-time checks. Never include sampler/compositor/context PIDs.
        targets = {pid: process for pid, process in processes.items()
                   if process.role in ("main", "helper")}
        self.sample_count += 1
        rows, current, active = [], {}, set()
        try:
            snapshot = self.api.thread_snapshot(set(targets))
        except OSError as error:
            for process in targets.values():
                tids = [key[2] for key in self.active
                        if key[:2] == (process.pid, process.created)] or [None]
                for tid in tids:
                    row = self.row(process, tid, sampling_start)
                    row.update(thread_cycle_status="unavailable_snapshot_error",
                               win32_error=getattr(error, "winerror", None), error_operation="snapshot")
                    rows.append(row)
            self.previous = {}
            return rows
        for tid, pid in sorted(set(snapshot)):
            if pid not in targets:
                continue
            process = targets[pid]
            key = (pid, process.created, tid)
            active.add(key)
            row = self.read(process, tid, sampling_start)
            if row["thread_cycle_status"] == "observed":
                old, previous = self.known.get(key), self.previous.get(key)
                if old and old["thread_created_filetime"] != row["thread_created_filetime"]:
                    row["thread_cycle_status"] = "unavailable_tid_reused"
                elif previous is None:
                    row["thread_cycle_status"] = "baseline"
                    current[key] = row.copy()
                elif row["cycles"] < previous["cycles"]:
                    row["thread_cycle_status"] = "unavailable_counter_regression"
                elif row["elapsed_s"] <= previous["elapsed_s"]:
                    row["thread_cycle_status"] = "unavailable_nonpositive_interval"
                else:
                    elapsed = row["elapsed_s"] - previous["elapsed_s"]
                    delta = row["cycles"] - previous["cycles"]
                    row.update(thread_cycle_status="measured", cycles_delta=delta,
                               delta_elapsed_s=elapsed, cycles_per_s=delta / elapsed)
                    current[key] = row.copy()
            if row["identity_status"] == "verified":
                self.known[key] = row.copy()
            rows.append(row)
        for pid, created, tid in sorted(self.active - active):
            process = targets.get(pid)
            if process is None or process.created != created:
                continue
            row = self.row(process, tid, sampling_start)
            row["thread_cycle_status"] = ("unavailable_missing" if process.alive() else
                                          "unavailable_process_exited")
            rows.append(row)
        for process in targets.values():
            if not any(row["pid"] == process.pid for row in rows):
                row = self.row(process, None, sampling_start)
                row["thread_cycle_status"] = ("unavailable_no_threads" if process.alive() else
                                              "unavailable_process_exited")
                rows.append(row)
        self.previous, self.active = current, active
        return rows


class Gpu:
    """One PDH query restricted to launched process IDs, not all system processes."""
    paths = {"engines": r"\GPU Engine(pid_{pid}_*)\Utilization Percentage",
             "dedicated_bytes": r"\GPU Process Memory(pid_{pid}_*)\Dedicated Usage",
             "shared_bytes": r"\GPU Process Memory(pid_{pid}_*)\Shared Usage"}

    energy_paths = {"system_package_energy_pwh": r"\Energy Meter(RAPL_Package0_PKG)\Energy",
                    "system_package_power_mw": r"\Energy Meter(RAPL_Package0_PKG)\Power",
                    "system_package_meter_time_ms": r"\Energy Meter(RAPL_Package0_PKG)\Time"}

    def __init__(self, enabled=True, system_energy=False, thread_context_switches=False):
        self.query, self.counters, self.errors = W.HANDLE(), {}, {}
        self.energy_counters, self.gpu_enabled = {}, enabled
        self.system_energy_enabled, self.collect_status = system_energy, None
        self.thread_counters, self.thread_cache = {}, None
        self.pdh = None
        if not enabled and not system_energy and not thread_context_switches:
            self.errors["query"] = "disabled"
            return
        try:
            self.pdh = C.WinDLL("pdh", use_last_error=True)
            self.pdh.PdhOpenQueryW.argtypes = [W.LPCWSTR, C.c_size_t, C.POINTER(W.HANDLE)]
            self.pdh.PdhAddEnglishCounterW.argtypes = [W.HANDLE, W.LPCWSTR, C.c_size_t, C.POINTER(W.HANDLE)]
            self.pdh.PdhCollectQueryData.argtypes = [W.HANDLE]
            self.pdh.PdhGetFormattedCounterArrayW.argtypes = [W.HANDLE, W.DWORD,
                C.POINTER(W.DWORD), C.POINTER(W.DWORD), C.c_void_p]
            self.pdh.PdhGetFormattedCounterValue.argtypes = [W.HANDLE, W.DWORD,
                C.POINTER(W.DWORD), C.POINTER(CounterValue)]
            self.pdh.PdhCloseQuery.argtypes = [W.HANDLE]
            for name in ("PdhOpenQueryW", "PdhAddEnglishCounterW", "PdhCollectQueryData",
                         "PdhGetFormattedCounterArrayW", "PdhGetFormattedCounterValue", "PdhCloseQuery"):
                getattr(self.pdh, name).restype = W.DWORD
            status = self.pdh.PdhOpenQueryW(None, 0, C.byref(self.query))
            if status:
                self.errors["query"] = f"PDH 0x{status:08x}"
            elif system_energy:
                for name, path in self.energy_paths.items():
                    handle = W.HANDLE()
                    status = self.pdh.PdhAddEnglishCounterW(self.query, path, 0, C.byref(handle))
                    if status:
                        self.errors[name] = f"PDH 0x{status:08x}"
                    else:
                        self.energy_counters[name] = handle
        except OSError as error:
            self.errors["query"] = str(error)

    def add(self, pid):
        if not self.query or not self.gpu_enabled or pid in self.counters:
            return
        self.counters[pid] = {}
        for kind, path in self.paths.items():
            handle = W.HANDLE()
            status = self.pdh.PdhAddEnglishCounterW(
                self.query, path.format(pid=pid), 0, C.byref(handle))
            if status:
                self.errors[f"{pid}:{kind}"] = f"PDH 0x{status:08x}"
            else:
                self.counters[pid][kind] = handle

    def collect(self):
        self.thread_cache = None
        self.collect_status = None
        if not self.query:
            return "unavailable"
        status = self.pdh.PdhCollectQueryData(self.query)
        if self.system_energy_enabled:
            self.collect_status = int(status)
        return f"PDH 0x{status:08x}" if status else None

    def add_thread_image(self, image):
        if not self.query or image in self.thread_counters:
            return
        self.thread_counters[image] = {}
        for name, counter in (("pid", "ID Process"), ("switches", "Context Switches/sec")):
            handle = W.HANDLE()
            status = self.pdh.PdhAddEnglishCounterW(self.query,
                f"\\Thread({image}*/*)\\{counter}", 0, C.byref(handle))
            if status:
                self.errors[f"thread:{image}:{name}"] = f"PDH 0x{status:08x}"
            else:
                self.thread_counters[image][name] = handle

    def context_switches(self, pid):
        if self.thread_cache is None:
            self.thread_cache = {}
            for counters in self.thread_counters.values():
                if "pid" not in counters or "switches" not in counters:
                    continue
                pids, _ = self.array(counters["pid"])
                switches, _ = self.array(counters["switches"])
                for name, owner in pids.items():
                    self.thread_cache.setdefault(int(owner), []).append(switches.get(name))
        values = self.thread_cache.get(pid, [])
        complete = bool(values) and all(v is not None for v in values)
        return {"thread_context_switches_per_s": sum(values) if complete else None,
                "thread_context_switches_status": "reported_matching_thread_counters" if complete else "unavailable_or_incomplete_thread_instances"}

    def array(self, handle):
        size, count = W.DWORD(), W.DWORD()
        # PDH_FMT_DOUBLE | PDH_FMT_NOCAP100; preserve data before aggregation.
        status = self.pdh.PdhGetFormattedCounterArrayW(
            handle, 0x8200, C.byref(size), C.byref(count), None)
        if status != 0x800007D2:  # PDH_MORE_DATA
            return {}, f"PDH 0x{status:08x}" if status else "no_matching_instance"
        for _ in range(3):
            buffer = C.create_string_buffer(size.value)
            status = self.pdh.PdhGetFormattedCounterArrayW(
                handle, 0x8200, C.byref(size), C.byref(count), buffer)
            if status != 0x800007D2:
                break
        if status:
            return {}, f"PDH 0x{status:08x}"
        items = C.cast(buffer, C.POINTER(CounterItem))
        values = {items[i].name: items[i].value.value for i in range(count.value)
                  if items[i].value.status in (0, 1) and math.isfinite(items[i].value.value)}
        return values, "measured" if values else "no_valid_instance"

    def read(self, pid, collect_error=None):
        result, statuses = {}, {}
        for kind in self.paths:
            handle = self.counters.get(pid, {}).get(kind)
            if collect_error or not handle:
                values, status = {}, collect_error or self.errors.get(f"{pid}:{kind}", "unavailable")
            else:
                values, status = self.array(handle)
            statuses[kind] = status
            if kind == "engines":
                # Strip only the leading PID; preserve LUID/physical engine identity.
                result["gpu_engines"] = {name.split("_", 2)[2]: max(0.0, value)
                                         for name, value in values.items()
                                         if name.startswith(f"pid_{pid}_")}
                result["gpu_busiest_engine_percent"] = max(result["gpu_engines"].values(), default=None)
            else:
                result["gpu_" + kind] = sum(values.values()) if values else None
        result["gpu_status"] = json.dumps(statuses, sort_keys=True)
        return result

    def close(self):
        if self.query:
            self.pdh.PdhCloseQuery(self.query)
            self.query = W.HANDLE()

    def energy(self, collect_error=None):
        if not self.system_energy_enabled:
            return {}
        result, counters = {}, {}
        for name in self.energy_paths:
            handle = self.energy_counters.get(name)
            evidence = {"counter_present": bool(handle), "get_status": None, "cstatus": None,
                        "raw_value": None, "status": "missing_counter"}
            result[name] = None
            if handle:
                value = CounterValue()
                # Sentinels distinguish unpopulated API output from valid zero data/status.
                value.status, value.value = 0xffffffff, math.nan
                status = self.pdh.PdhGetFormattedCounterValue(handle, 0x8200, None, C.byref(value))
                evidence.update(get_status=int(status),
                                cstatus=int(value.status) if value.status != 0xffffffff else None,
                                raw_value=value.value if math.isfinite(value.value) else str(value.value))
                if collect_error or self.collect_status != 0:
                    evidence["status"] = "collect_failed_or_unavailable"
                elif status:
                    evidence["status"] = "get_failed"
                elif value.status not in (0, 1):
                    evidence["status"] = "invalid_cstatus"
                elif not math.isfinite(value.value):
                    evidence["status"] = "nonfinite_value"
                else:
                    evidence["status"] = "measured"
                    result[name] = value.value
            else:
                evidence["setup_error"] = self.errors.get(name, self.errors.get("query", "unavailable"))
            counters[name] = evidence
        result["system_package_pdh_status"] = {"collect_status": self.collect_status,
                                               "collect_error": collect_error, "counters": counters}
        if result.get("system_package_power_mw") is not None:
            result["system_package_power_w"] = result["system_package_power_mw"] / 1000
        return result


COUNTERS = ("user_s", "kernel_s", "cycles", "page_faults", "io_read_ops",
            "io_write_ops", "io_other_ops", "io_read_bytes", "io_write_bytes", "io_other_bytes")


def add_rates(row, previous, logical_processors):
    if previous is None:
        return row
    elapsed = row["elapsed_s"] - previous["elapsed_s"]
    if elapsed <= 0:
        raise ValueError("Non-increasing sample clock")
    row["interval_s"] = elapsed
    for name in COUNTERS:
        if row.get(name) is None or previous.get(name) is None:
            continue
        delta = row[name] - previous[name]
        if name == "page_faults" and delta < 0:
            delta += 2**32
        if delta < 0:
            raise ValueError(f"Counter regressed: {name}")
        row[name + "_delta"] = delta
        row[name + "_per_s"] = delta / elapsed
    if "user_s_delta" in row and "kernel_s_delta" in row:
        row["cpu_percent_one_core"] = 100 * (row["user_s_delta"] + row["kernel_s_delta"]) / elapsed
        row["cpu_percent_machine"] = row["cpu_percent_one_core"] / logical_processors
    return row


def combine(rows, role):
    if not rows:
        return None
    result = {"pid": "", "role": role, "elapsed_s": max(r["elapsed_s"] for r in rows),
              "members": len(rows)}
    if all("interval_s" in row for row in rows):
        result["interval_s"] = max(row["interval_s"] for row in rows)
    keys = set.intersection(*(set(r) for r in rows))
    for key in keys - {"pid", "role", "created_filetime", "elapsed_s", "interval_s"}:
        values = [r[key] for r in rows]
        if all(isinstance(v, (int, float)) for v in values):
            result[key] = sum(values)
    engines = {}
    for row in rows:
        for name, value in row.get("gpu_engines", {}).items():
            engines[name] = engines.get(name, 0) + value
    result["gpu_engines"] = engines
    # Never sum engine busy percentages into an apparent overall utilization.
    result["gpu_busiest_engine_percent"] = max(engines.values(), default=None)
    result["gpu_status"] = "all_members_measured" if all(
        r.get("gpu_busiest_engine_percent") is not None for r in rows) else "incomplete_or_unavailable"
    # Shared allocations can appear in several processes; no defensible tree sum.
    for key in ("gpu_dedicated_bytes", "gpu_shared_bytes"):
        result[key] = None
    result["gpu_memory_status"] = "unsupported_process_tree_sum_shared_allocations_may_overlap"
    result["private_working_set_bytes"] = sum(r["private_working_set_bytes"] for r in rows) if all(
        r.get("private_working_set_bytes") is not None for r in rows) else None
    result["shared_commit_bytes"] = None
    result["memory_api_status"] = "private_resident_sum_measured_shared_commit_sum_unsupported" if result["private_working_set_bytes"] is not None else "private_resident_unavailable_shared_commit_sum_unsupported"
    for key in ("peak_working_set_bytes", "peak_commit_bytes", "peak_gdi_objects", "peak_user_objects"):
        if key in result:
            result["sum_process_lifetime_" + key] = result.pop(key)
    result["thread_context_switches_per_s"] = sum(r["thread_context_switches_per_s"] for r in rows) if all(
        r.get("thread_context_switches_per_s") is not None for r in rows) else None
    return result


def add_energy_rates(row, previous):
    energy, clock = "system_package_energy_pwh", "system_package_meter_time_ms"
    if previous is None or any(value is None for value in (
            row.get(energy), previous.get(energy), row.get(clock), previous.get(clock))):
        return
    delta, interval = row[energy] - previous[energy], (row[clock] - previous[clock]) / 1000
    if delta < 0 or interval <= 0:
        row["energy_status"] = "meter_counter_regression_or_no_interval"
        return
    row["system_package_energy_j_delta"] = delta * 3.6e-9
    row["system_package_meter_interval_s"] = interval
    row["system_package_power_w_from_energy"] = row["system_package_energy_j_delta"] / interval
    row["energy_status"] = "measured_whole_package_not_per_application"


def add_system_rates(row, previous):
    if previous is None:
        return
    idle = row["system_idle_s"] - previous["system_idle_s"]
    total = sum(row[key] - previous[key] for key in ("system_kernel_s", "system_user_s"))
    row["system_cpu_percent_machine"] = 100 * (total - idle) / total if total > 0 and 0 <= idle <= total else None


def quantile(values, fraction):
    ordered = sorted(values)
    if not ordered:
        return None
    position = (len(ordered) - 1) * fraction
    lo, hi = math.floor(position), math.ceil(position)
    return ordered[lo] + (ordered[hi] - ordered[lo]) * (position - lo)


def summarize(rows):
    result = {"samples": len(rows)}
    if not rows:
        return result
    for key in sorted(set.union(*(set(r) for r in rows))):
        if key.endswith("_delta") or key in ("pid", "created_filetime", "elapsed_s"):
            continue
        values = [r[key] for r in rows if isinstance(r.get(key), (float, int))]
        if values:
            result[key] = {"valid_samples": len(values), "p50": statistics.median(values),
                           "p95": quantile(values, .95), "max": max(values)}
    span = rows[-1]["elapsed_s"] - rows[0]["elapsed_s"]
    for key in COUNTERS:
        deltas = [r[key + "_delta"] for r in rows if key + "_delta" in r]
        intervals = [r["interval_s"] for r in rows if key + "_delta" in r]
        if deltas and sum(intervals) > 0:
            result[key + "_sampled_delta"] = sum(deltas)
            result[key + "_weighted_per_s"] = sum(deltas) / sum(intervals)
    if "user_s_weighted_per_s" in result and "kernel_s_weighted_per_s" in result:
        result["cpu_percent_one_core_weighted"] = 100 * (
            result["user_s_weighted_per_s"] + result["kernel_s_weighted_per_s"])
    energy = [r for r in rows if r.get("system_package_energy_j_delta") is not None]
    if energy:
        result["system_package_energy_sampled_j"] = sum(r["system_package_energy_j_delta"] for r in energy)
        meter_duration = sum(r["system_package_meter_interval_s"] for r in energy)
        result["system_package_meter_duration_s"] = meter_duration
        result["system_package_power_w_weighted"] = result["system_package_energy_sampled_j"] / meter_duration
    for key in ("private_bytes", "working_set_bytes", "private_working_set_bytes", "commit_bytes", "handles", "gdi_objects", "user_objects"):
        points = [(r["elapsed_s"], r[key]) for r in rows if r.get(key) is not None]
        if len(points) > 1 and span > 0:
            center = statistics.mean(p[0] for p in points)
            denominator = sum((p[0] - center)**2 for p in points)
            result[key + "_end_minus_start"] = points[-1][1] - points[0][1]
            result[key + "_slope_per_minute"] = (60 * sum((x - center) * y for x, y in points)
                                                  / denominator if denominator else None)
    return result


def trial_order(targets, scenarios, trials):
    # Alternating AB/BA balances drift; rotate scenarios across repeat blocks.
    for trial in range(trials):
        order = targets if trial % 2 == 0 else list(reversed(targets))
        rotated = scenarios[trial % len(scenarios):] + scenarios[:trial % len(scenarios)]
        for scenario in rotated:
            for target in order:
                yield trial + 1, target, scenario


def write_csv(path, rows):
    columns = sorted(set.union(*(set(r) for r in rows))) if rows else []
    with path.open("w", encoding="utf-8", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=columns)
        writer.writeheader()
        for row in rows:
            writer.writerow({key: json.dumps(value, sort_keys=True) if isinstance(value, dict) else value
                             for key, value in row.items()})


def status_counts(rows, key):
    result = {}
    for row in rows:
        if key in row:
            scope = result.setdefault(row["role"], {})
            scope[row[key]] = scope.get(row[key], 0) + 1
    return result


def accepted_keys(results):
    keys = [(row["trial"], row["target"], row["scenario"]) for row in results]
    if len(keys) != len(set(keys)):
        raise ValueError("Existing measurements contain duplicate accepted trials")
    return set(keys)


def fresh_trial_stem(directory, stem):
    candidate, attempt = stem, 1
    while any((directory / (candidate + suffix)).exists() for suffix in
              ("", ".csv", ".log", ".partial.csv", ".threads.csv", ".threads.partial.csv")):
        attempt += 1
        candidate = f"{stem}-attempt{attempt:02d}"
    return candidate


def resume_result(output, requested):
    previous = read_json(output)
    if previous is None:
        raise ValueError("--resume requires an existing valid measurements.json")
    if previous.get("config") != requested["config"]:
        raise ValueError("Resume requires exactly the same targets, binary hashes, and scenarios")
    previous_protocol = dict(previous.get("protocol", {}))
    previous_protocol.setdefault("memory_ex2_enabled", False)
    previous_protocol.setdefault("enabled_automation_allowed", False)
    if previous_protocol != requested["protocol"]:
        raise ValueError("Resume requires the same sampling protocol and trial count")
    completed = accepted_keys(previous.get("results", []))
    for row in previous.get("results", []):
        if not (output.parent / row["csv"]).is_file():
            raise ValueError(f"Accepted trial is missing its raw CSV: {row['csv']}")
        thread_csv = row.get("thread_cycles", {}).get("csv")
        if thread_csv and not (output.parent / thread_csv).is_file():
            raise ValueError(f"Accepted trial is missing its thread CSV: {thread_csv}")
    backup = output.with_name(f"measurements-before-resume-{time.time_ns()}.json")
    shutil.copy2(output, backup)
    history = {"recorded_utc": requested["recorded_utc"],
               "sampler_sha256": requested["sampler_sha256"], "previous_json": backup.name,
               "accepted_trials_preserved": len(completed), "hardware": requested["hardware"]}
    previous.setdefault("resume_history", []).append(history)
    if "failure" in previous:
        previous.setdefault("invalid_attempts", []).append(previous.pop("failure"))
    return previous, completed


def window_state(windows):
    if any(visible and iconic for _, visible, iconic in windows):
        return "minimized"
    if any(visible for _, visible, _ in windows):
        return "visible"
    return "hidden" if windows else "absent"


def validate_window_state(observed, expected, allow_changes=False):
    permitted = {"visible", "hidden", "absent"} if allow_changes else {
        "absent" if expected == "closed" else expected}
    if observed not in permitted:
        raise RuntimeError(f"Expected window state in {sorted(permitted)}, got {observed}")


class WindowStateValidationError(RuntimeError):
    def __init__(self, message, evidence):
        super().__init__(message)
        self.window_validation_failure = evidence


def validate_window_observation(api, process, window_class, windows, expected, allow_changes,
                                started, sampling_start, directory):
    observed = window_state(windows)
    try:
        validate_window_state(observed, expected, allow_changes)
    except RuntimeError as error:
        observed_ns = time.perf_counter_ns()
        evidence = {"observed_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                    "observed_perf_counter_ns": observed_ns,
                    "clock": "time.perf_counter_ns; same monotonic clock as trial and sampling elapsed",
                    "fixture_elapsed_s": observed_ns / 1e9 - started,
                    "sampling_elapsed_s": observed_ns / 1e9 - sampling_start,
                    "pid": process.pid, "created_filetime": process.created,
                    "window_filter": window_class, "expected": expected,
                    "observed": observed, "allow_window_changes": allow_changes,
                    "gate_windows": [{"hwnd": hwnd, "visible": visible, "iconic": iconic}
                                     for hwnd, visible, iconic in windows],
                    "inventory_note": "All target-PID top-level windows, enumerated after the failed gate; window queries are non-atomic."}
        try:
            evidence["inventory"] = api.windows(process.pid, details=True)
        except Exception as inventory_error:
            evidence["inventory_error"] = str(inventory_error)
        evidence["inventory_finished_perf_counter_ns"] = time.perf_counter_ns()
        artifact = directory / "window-validation-failure.json"
        evidence["artifact"] = str(Path(directory.name) / artifact.name)
        try:
            artifact.write_text(json.dumps(evidence, indent=2), encoding="utf-8")
        except OSError as artifact_error:
            evidence["artifact_write_error"] = str(artifact_error)
        raise WindowStateValidationError(str(error), evidence) from error
    return observed


def read_events(path):
    if not path.exists():
        return []
    events = []
    for line in path.read_text(encoding="utf-8").splitlines():
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            # A writer may be between write and newline during a readiness poll.
            continue
    return events


def read_json(path):
    try:
        return json.loads(path.read_text(encoding="utf-8-sig"))
    except (FileNotFoundError, json.JSONDecodeError):
        return None


def read_readiness_json(path, deadline):
    import errno

    while True:
        try:
            return read_json(path)
        except PermissionError as error:
            # The fixture or a scanner can briefly deny access while ready.json
            # is published. Only readiness may retry, within its existing deadline.
            winerror = getattr(error, "winerror", None)
            if (winerror is not None and winerror not in (5, 32, 33)) or (
                    winerror is None and error.errno not in (errno.EACCES, errno.EPERM)):
                raise
            remaining = deadline - time.perf_counter()
            if remaining <= 0:
                raise RuntimeError(f"Fixture readiness file access timeout: {path}") from error
            time.sleep(min(.02, remaining))


def match_fixture_fields(value, expected, path="fixture"):
    if type(value) is not type(expected):
        raise RuntimeError(f"{path} has the wrong JSON type")
    if isinstance(expected, dict):
        for key, wanted in expected.items():
            if key not in value:
                raise RuntimeError(f"{path}.{key} is missing")
            match_fixture_fields(value[key], wanted, f"{path}.{key}")
    elif isinstance(expected, list):
        if len(value) != len(expected):
            raise RuntimeError(f"{path} has an unexpected list length")
        for index, wanted in enumerate(expected):
            match_fixture_fields(value[index], wanted, f"{path}[{index}]")
    elif value != expected:
        raise RuntimeError(f"{path} does not match the expected value")


def validate_automation_contract(scenario, allow_enabled=False):
    contract = scenario.get("automation_contract")
    if contract is None:
        return None
    if not allow_enabled:
        raise ValueError("automation_contract requires --allow-enabled-automation")
    if not isinstance(contract, dict) or type(contract.get("version")) is not int or contract["version"] != 1:
        raise ValueError("automation_contract must use version 1")
    profile = contract.get("profile")
    if not isinstance(profile, str) or not profile.strip():
        raise ValueError("automation_contract needs a nonempty profile")
    enabled = contract.get("ready", {}).get("automation_enabled") if isinstance(contract.get("ready"), dict) else None
    if type(enabled) is not bool:
        raise ValueError("automation_contract.ready needs an explicit automation_enabled boolean")
    for phase in ("ready", "finished"):
        required = {"automation_enabled": enabled, "scheduler_benchmark": {
            "schema_version": 1, "profile": profile, "scope": "fixture_owned_processes",
            "settings_io": "in_memory", "cross_session_control": False,
            "worker_running": enabled and phase == "ready"}}
        if phase == "finished":
            required["scheduler_benchmark"]["restoration_succeeded"] = True
        match_fixture_fields(contract.get(phase), required, f"automation_contract.{phase}")
    ready = contract["ready"]["scheduler_benchmark"]
    finished = contract["finished"]["scheduler_benchmark"]
    target = ready.get("target_executable")
    if not isinstance(target, str) or not Path(target).is_absolute() or finished.get("target_executable") != target:
        raise ValueError("automation_contract needs the same absolute target_executable in both phases")
    features = ready.get("enabled_features")
    if not isinstance(features, list) or any(not isinstance(name, str) or not name for name in features):
        raise ValueError("automation_contract.ready needs an enabled_features list of names")
    if "native_verifier" in contract:
        command = contract["native_verifier"]
        if not isinstance(command, list) or not command or any(not isinstance(arg, str) or not arg for arg in command):
            raise ValueError("automation_contract.native_verifier must be a nonempty command argv list")
        try:
            for arg in command:
                arg.format_map({"phase": "prepare", "trial_dir": "trial", "profile": profile})
        except (KeyError, ValueError) as error:
            raise ValueError("native_verifier supports only {phase}, {trial_dir}, and {profile} placeholders") from error
    return contract


def run_native_verifier(contract, phase, trial_directory, timeout):
    if not contract or "native_verifier" not in contract:
        return None
    command = [arg.format_map({"phase": phase, "trial_dir": str(trial_directory),
                               "profile": contract["profile"]}) for arg in contract["native_verifier"]]
    try:
        completed = subprocess.run(command, shell=False, capture_output=True,
                                   text=True, encoding="utf-8-sig", timeout=timeout)
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f"Native verifier {phase} timed out after {timeout}s") from error
    artifact = {"returncode": completed.returncode, "stdout": completed.stdout, "stderr": completed.stderr}
    (trial_directory / f"native-verification-{phase}.json").write_text(json.dumps(artifact, indent=2), encoding="utf-8")
    if completed.returncode != 0:
        raise RuntimeError(f"Native verifier {phase} failed with exit {completed.returncode}; see its trial artifact")
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"Native verifier {phase} did not return valid JSON") from error
    if not isinstance(result, dict):
        raise RuntimeError(f"Native verifier {phase} must return a JSON object")
    return result


def validate_ready(value, contract=None):
    if not isinstance(value, dict):
        raise RuntimeError("Fixture did not write ready.json")
    if contract:
        match_fixture_fields(value, contract["ready"], "ready")
        if value.get("runtime_status", {}).get("worker_error") is not None:
            raise RuntimeError("Fixture reported a runtime worker error at readiness")
    elif value.get("automation_enabled", False) is not False:
        raise RuntimeError("Fixture enabled automation without an opt-in contract")


def validate_finished(value, contract=None):
    if not isinstance(value, dict):
        raise RuntimeError("Fixture did not write finished.json")
    if value.get("error"):
        raise RuntimeError(f"Fixture reported an error: {value['error']}")
    if value.get("cleanup_errors") or value.get("runtime_worker_error"):
        raise RuntimeError("Fixture reported incomplete runtime cleanup")
    if contract:
        match_fixture_fields(value, contract["finished"], "finished")
        if value.get("measurement_runtime_status", {}).get("worker_error") is not None:
            raise RuntimeError("Fixture reported a runtime worker error during measurement")
        expected_running = contract["ready"]["scheduler_benchmark"]["worker_running"]
        if value.get("worker_running_at_measurement_end") is not expected_running:
            raise RuntimeError("Fixture worker state changed during measurement")
    elif value.get("automation_enabled") is not False:
        raise RuntimeError("Fixture did not confirm automation remained disabled")
    if "loop_result" in value and value["loop_result"] != {"Ok": "ok"}:
        raise RuntimeError(f"Fixture loop failed: {value['loop_result']}")


def set_client_size(api, hwnd, width, height):
    outer, client = W.RECT(), W.RECT()
    checked(api.user.GetWindowRect(hwnd, C.byref(outer)))
    checked(api.user.GetClientRect(hwnd, C.byref(client)))
    dpi = api.user.GetDpiForWindow(hwnd) or 96
    # API coordinates are physical pixels after Per-Monitor-V2 awareness setup.
    client_width, client_height = round(width * dpi / 96), round(height * dpi / 96)
    checked(api.user.SetWindowPos(hwnd, None, 0, 0,
        client_width + outer.right - outer.left - client.right,
        client_height + outer.bottom - outer.top - client.bottom, 0x16))
    result = read_window_geometry(api, hwnd)
    result.update(logical_width_requested=width, logical_height_requested=height)
    return result


def read_window_geometry(api, hwnd):
    outer, client, placement = W.RECT(), W.RECT(), WindowPlacement()
    placement.length = C.sizeof(placement)
    checked(api.user.GetWindowRect(hwnd, C.byref(outer)))
    checked(api.user.GetClientRect(hwnd, C.byref(client)))
    checked(api.user.GetWindowPlacement(hwnd, C.byref(placement)))
    return {"dpi": api.user.GetDpiForWindow(hwnd),
            "client_pixel_width": client.right, "client_pixel_height": client.bottom,
            "outer_pixel_width": outer.right - outer.left, "outer_pixel_height": outer.bottom - outer.top,
            "normal_outer_pixel_width": placement.normal.right - placement.normal.left,
            "normal_outer_pixel_height": placement.normal.bottom - placement.normal.top,
            "show_command": placement.show_command}


class Interaction:
    """Identical native messages target only this fixture's reported main HWND."""
    def __init__(self, api, hwnd, scenario):
        self.api, self.hwnd, self.kind = api, hwnd, scenario.get("interaction")
        self.next_action, self.actions = 0, 0
        self.rect = W.RECT()
        if self.kind:
            if not hwnd:
                raise RuntimeError("Interaction requires a fixture-owned HWND")
            checked(api.user.GetWindowRect(hwnd, C.byref(self.rect)))

    def update(self, elapsed):
        if not self.kind or elapsed < self.next_action:
            return
        if self.kind == "scroll":
            rect = W.RECT()
            checked(self.api.user.GetClientRect(self.hwnd, C.byref(rect)))
            point = W.POINT(int(rect.right * .55), int(rect.bottom * .65) + self.actions % 2)
            # Winit routes wheel deltas using its last CursorMoved position.
            client_coordinates = (point.x & 0xffff) | ((point.y & 0xffff) << 16)
            checked(self.api.user.PostMessageW(self.hwnd, 0x200, 0, client_coordinates))
            checked(self.api.user.ClientToScreen(self.hwnd, C.byref(point)))
            delta = -120 if self.actions % 30 < 15 else 120
            coordinates = (point.x & 0xffff) | ((point.y & 0xffff) << 16)
            checked(self.api.user.PostMessageW(self.hwnd, 0x20A, (delta & 0xffff) << 16, coordinates))
            self.next_action = elapsed + .1
        elif self.kind == "resize":
            width, height = ((1000, 700) if self.actions % 2 == 0 else (1120, 760))
            set_client_size(self.api, self.hwnd, width, height)
            self.next_action = elapsed + .5
        else:
            raise ValueError(f"Unsupported interaction: {self.kind}")
        self.actions += 1

    def restore(self):
        if self.kind == "resize":
            checked(self.api.user.SetWindowPos(self.hwnd, None, self.rect.left, self.rect.top,
                self.rect.right - self.rect.left, self.rect.bottom - self.rect.top, 0x14))


class WindowPolicy:
    def __init__(self, api, scenario):
        self.api, self.scenario, self.hwnd = api, scenario, None
        self.activations, self.last_pointer = 0, None
        self.original_pointer = None

    def activate(self, hwnd):
        if any(self.api.user.GetAsyncKeyState(key) & 0x8000 for key in
               (1, 2, 4, 5, 6, 16, 17, 18, 91, 92)):
            raise RuntimeError("Mouse button or modifier held during fixture activation; retry when input is idle")
        if self.original_pointer is None:
            original_pointer = W.POINT()
            checked(self.api.user.GetCursorPos(C.byref(original_pointer)))
            self.original_pointer = original_pointer
        self.api.user.SetForegroundWindow(hwnd)
        if self.api.user.GetForegroundWindow() != hwnd:
            foreground = self.api.user.GetForegroundWindow()
            foreground_thread = self.api.user.GetWindowThreadProcessId(foreground, None)
            current_thread = self.api.kernel.GetCurrentThreadId()
            attached = foreground_thread and foreground_thread != current_thread
            if attached:
                checked(self.api.user.AttachThreadInput(current_thread, foreground_thread, True))
            try:
                self.api.user.BringWindowToTop(hwnd)
                self.api.user.SetForegroundWindow(hwnd)
            finally:
                if attached:
                    checked(self.api.user.AttachThreadInput(current_thread, foreground_thread, False))
            if self.api.user.GetForegroundWindow() != hwnd:
                raise RuntimeError("Windows denied foreground activation of the isolated fixture")
        self.hwnd, self.activations = hwnd, self.activations + 1
        rect = W.RECT()
        checked(self.api.user.GetClientRect(hwnd, C.byref(rect)))
        x, y = ((.55, .65) if self.scenario.get("interaction") == "scroll" else (.5, .03))
        point = W.POINT(round(rect.right * x), round(rect.bottom * y))
        checked(self.api.user.ClientToScreen(hwnd, C.byref(point)))
        checked(self.api.user.SetCursorPos(point.x, point.y))
        self.last_pointer = (point.x, point.y)

    def verify(self, windows, allow_changes=False):
        visible = [hwnd for hwnd, shown, iconic in windows if shown and not iconic]
        if not visible:
            return
        if len(visible) != 1:
            raise RuntimeError("Fixture has more than one visible top-level window")
        if allow_changes and visible[0] != self.hwnd:
            self.activate(visible[0])
        if self.api.user.GetForegroundWindow() != visible[0]:
            raise RuntimeError("Fixture lost foreground during sampling; trial is not comparable")

    def restore_pointer(self):
        if self.last_pointer is None:
            return
        current = W.POINT()
        checked(self.api.user.GetCursorPos(C.byref(current)))
        # Preserve a user's intervening pointer movement rather than overwrite it.
        if self.last_pointer and (current.x, current.y) == self.last_pointer:
            checked(self.api.user.SetCursorPos(self.original_pointer.x, self.original_pointer.y))


def run(api, target, scenario, options, directory, trial):
    contract = validate_automation_contract(scenario, getattr(options, "allow_enabled_automation", False))
    stem = fresh_trial_stem(directory, f"{trial:02d}-{target['name']}-{scenario['name']}")
    trial_directory = directory / stem
    events_path, stop_path = trial_directory / "events.jsonl", trial_directory / "stop"
    trial_directory.mkdir()
    ready_path, finished_path = trial_directory / "ready.json", trial_directory / "finished.json"
    # The agreed full-application protocol starts its lifetime at fixture readiness.
    uses_ready_file = any("{trial_dir}" in str(arg) for arg in target.get("args", []) + scenario.get("args", []))
    if contract and not uses_ready_file:
        raise ValueError("Enabled automation requires the ready.json/finished.json fixture protocol")
    warmup, duration = scenario.get("warmup", options.warmup), scenario.get("duration", options.duration)
    lifetime = warmup + duration + 3 + (0 if uses_ready_file else options.startup_timeout)
    substitutions = {"lifetime_ms": int(lifetime * 1000), "events": str(events_path),
                     "stop_file": str(stop_path), "scenario": scenario["name"],
                     "trial_dir": str(trial_directory),
                     "width": scenario.get("width", 1120), "height": scenario.get("height", 760)}
    arguments = [str(value).format_map(substitutions) for value in
                 target.get("args", []) + scenario.get("args", [])]
    exe = Path(target["exe"]).resolve(strict=True)
    rows, processes, errors, verification = [], {}, [], {}
    thread_rows = []
    thread_cycles = ThreadCycles(api) if getattr(options, "thread_cycles", False) else None
    gpu = Gpu(not options.no_gpu, options.system_energy, options.thread_context_switches)
    first_hwnd_ms = first_visible_ms = ready_observed_ms = None
    anchor_before = api.window_anchor.successful_enumerations
    process = None
    log_path = directory / (stem + ".log")
    try:
        with log_path.open("w", encoding="utf-8") as log:
            environment = os.environ.copy()
            environment.update(target.get("environment", {}))
            verification["prepare"] = run_native_verifier(contract, "prepare", trial_directory, options.startup_timeout)
            started = time.perf_counter()
            process = subprocess.Popen([str(exe)] + arguments, cwd=exe.parent,
                stdout=log, stderr=log, env=environment, creationflags=subprocess.CREATE_NO_WINDOW)
            main = Process(api, process.pid, "main")
            processes[main.pid] = main
            sampler = Process(api, os.getpid(), "sampler")
            gpu.add(main.pid)
            if options.thread_context_switches:
                gpu.add_thread_image(exe.stem)
            deadline = started + options.startup_timeout
            expected = target.get("window_states", {}).get(scenario["name"], scenario.get("window", "visible"))
            window_class = target.get("window_class")
            uses_events = any("{events}" in str(arg) for arg in target.get("args", []) + scenario.get("args", []))
            while time.perf_counter() < deadline:
                if process.poll() is not None:
                    raise RuntimeError(f"Fixture exited before readiness; see {log_path}")
                observed_windows = api.windows(process.pid, window_class)
                current = window_state(observed_windows)
                elapsed_ms = (time.perf_counter() - started) * 1000
                if observed_windows and first_hwnd_ms is None:
                    first_hwnd_ms = elapsed_ms
                if current == "visible" and first_visible_ms is None:
                    first_visible_ms = elapsed_ms
                ready_file = read_readiness_json(ready_path, deadline) if uses_ready_file else None
                ready_event = any(e.get("event") == "ready" for e in read_events(events_path))
                ready = bool(ready_file) if uses_ready_file else (ready_event if uses_events else current == ("visible" if expected != "absent" else "absent"))
                if ready:
                    if ready_file:
                        validate_ready(ready_file, contract)
                        if ready_file.get("process_id", process.pid) != process.pid:
                            raise RuntimeError("Fixture readiness file reports a different PID")
                        if ready_file.get("hwnd") and not scenario.get("allow_window_changes", False):
                            window_class = int(ready_file["hwnd"])
                        observed_windows = api.windows(process.pid, window_class)
                        current = window_state(observed_windows)
                        if ready_file.get("state") and ready_file["state"] != expected:
                            raise RuntimeError(f"Fixture declares {ready_file['state']}, expected {expected}")
                    if current != expected and uses_ready_file:
                        time.sleep(.02)
                        continue
                    if expected != "absent" and current != "visible" and not uses_ready_file:
                        time.sleep(.02)
                        continue
                    if not uses_events and not uses_ready_file and expected == "absent" and elapsed_ms < 1500:
                        time.sleep(.02)
                        continue
                    ready_observed_ms = elapsed_ms
                    break
                time.sleep(.02)
            else:
                raise RuntimeError(f"Fixture readiness timeout; see {log_path}")
            startup_counters = main.lifetime()
            verification["ready"] = run_native_verifier(contract, "ready", trial_directory, options.startup_timeout)
            fixture_windows = api.windows(process.pid, window_class)
            geometry = None
            if fixture_windows and expected != "minimized":
                geometry = set_client_size(api, fixture_windows[0][0],
                    substitutions["width"], substitutions["height"])
            elif fixture_windows:
                geometry = read_window_geometry(api, fixture_windows[0][0])
                geometry["minimized_geometry_note"] = "normal placement is recorded; minimized client bounds are not render dimensions"
            window_policy = WindowPolicy(api, scenario)
            if expected == "visible" and fixture_windows:
                window_policy.activate(fixture_windows[0][0])
            if expected in ("hidden", "minimized", "closed") and not uses_ready_file:
                handles = api.windows(process.pid, window_class)
                visible = [hwnd for hwnd, shown, _ in handles if shown]
                if len(visible) != 1:
                    raise RuntimeError(f"Expected one fixture window, got {len(visible)}")
                if expected == "closed":
                    checked(api.user.PostMessageW(visible[0], 0x10, 0, 0))
                else:
                    # ShowWindowAsync returns prior visibility; zero is not reliably an error.
                    api.user.ShowWindowAsync(visible[0], 0 if expected == "hidden" else 6)
            warmup_until = time.perf_counter() + warmup
            while time.perf_counter() < warmup_until:
                if process.poll() is not None:
                    raise RuntimeError("Fixture exited during warmup")
                time.sleep(min(.05, max(0, warmup_until - time.perf_counter())))
            sampling_start = time.perf_counter()
            fixture_windows = api.windows(process.pid, window_class)
            interaction = Interaction(api, fixture_windows[0][0] if fixture_windows else None, scenario)
            previous = {}
            gpu.collect()
            next_sample = sampling_start
            while True:
                sample_begin = time.perf_counter()
                if process.poll() is not None:
                    raise RuntimeError("Fixture exited during sampling")
                snapshot = api.snapshot()
                # Only retain descendants whose creation time cannot predate their parent.
                changed = True
                while changed:
                    changed = False
                    for pid, (parent, _) in snapshot.items():
                        if pid in processes or parent not in processes or not processes[parent].alive():
                            continue
                        try:
                            child = Process(api, pid, "helper", parent)
                            if child.created >= processes[parent].created:
                                processes[pid] = child
                                gpu.add(pid)
                                changed = True
                            else:
                                child.close()
                        except OSError as error:
                            message = f"Descendant {pid} could not be sampled: {error}"
                            if message not in errors:
                                errors.append(message)
                collect_error = gpu.collect()
                system = api.system_context()
                system.update(role="system_context", pid="", elapsed_s=time.perf_counter() - sampling_start)
                add_system_rates(system, previous.get("system_context"))
                previous["system_context"] = system.copy()
                rows.append(system)
                if options.system_energy:
                    package = gpu.energy(collect_error)
                    package.update(role="system_package", pid="", elapsed_s=time.perf_counter() - sampling_start)
                    add_energy_rates(package, previous.get("system_package"))
                    previous["system_package"] = package.copy()
                    rows.append(package)
                if api.compositor_pid:
                    # This shared desktop compositor includes every visible app.
                    gpu.add(api.compositor_pid)
                    compositor = gpu.read(api.compositor_pid, collect_error)
                    compositor.update(role="compositor", pid=api.compositor_pid,
                                      elapsed_s=time.perf_counter() - sampling_start)
                    rows.append(compositor)
                current_rows = []
                for pid, tracked in processes.items():
                    if not tracked.alive():
                        continue
                    value = tracked.read(snapshot.get(pid, (None, None))[1])
                    value.update(gpu.read(pid, collect_error))
                    if options.thread_context_switches:
                        value.update(gpu.context_switches(pid))
                    value["elapsed_s"] = time.perf_counter() - sampling_start
                    add_rates(value, previous.get(pid), os.cpu_count() or 1)
                    previous[pid] = value.copy()
                    current_rows.append(value)
                if thread_cycles is not None:
                    thread_begin = time.perf_counter()
                    thread_rows.extend(thread_cycles.sample(processes, sampling_start))
                    thread_collection_ms = (time.perf_counter() - thread_begin) * 1000
                current_windows = api.windows(process.pid, window_class)
                observed = validate_window_observation(api, main, window_class, current_windows, expected,
                    scenario.get("allow_window_changes", False), started, sampling_start, trial_directory)
                if expected == "visible":
                    window_policy.verify(current_windows, scenario.get("allow_window_changes", False))
                for value in current_rows:
                    value["window_state"] = observed
                for role, members in (("combined", current_rows),
                                      ("helpers", [r for r in current_rows if r["role"] == "helper"])):
                    combined = combine(members, role)
                    if combined:
                        rows.append(combined)
                sampler_row = sampler.read(snapshot.get(os.getpid(), (None, None))[1])
                sampler_row["elapsed_s"] = time.perf_counter() - sampling_start
                add_rates(sampler_row, previous.get("sampler"), os.cpu_count() or 1)
                previous["sampler"] = sampler_row.copy()
                sampler_row["sample_collection_ms"] = (time.perf_counter() - sample_begin) * 1000
                if thread_cycles is not None:
                    sampler_row["thread_cycles_collection_ms"] = thread_collection_ms
                rows.extend(current_rows + [sampler_row])
                if time.perf_counter() - sampling_start >= duration:
                    break
                next_sample += options.interval
                while time.perf_counter() < next_sample:
                    interaction.update(time.perf_counter() - sampling_start)
                    time.sleep(min(.02, max(0, next_sample - time.perf_counter())))
            sampling_elapsed = time.perf_counter() - sampling_start
            interaction.restore()
            stop_requested = time.perf_counter()
            uses_stop = any("{stop_file}" in str(arg) for arg in target.get("args", []) + scenario.get("args", []))
            if uses_stop:
                stop_path.write_text("stop\n", encoding="ascii")
            process.wait(timeout=lifetime + options.shutdown_timeout)
            exit_observed = time.perf_counter()
            if process.returncode != 0:
                raise RuntimeError(f"Fixture exit {process.returncode}; see {log_path}")
            finished = read_json(finished_path)
            if uses_ready_file:
                validate_finished(finished, contract)
                expected_page = {"home": "Home", "background": "Home", "hidden": "Home",
                    "minimized": "Home", "settings": "LanguageAndAppearance",
                    "process-list": "ProcessList", "scroll": "ProcessList", "resize": "Home"}.get(scenario["name"])
                if expected_page and finished.get("last_page") != expected_page:
                    raise RuntimeError(f"Fixture page changed unexpectedly: expected {expected_page}, got {finished.get('last_page')}")
            # Retained handles still identify exited descendants; wait for cooperative cleanup.
            helper_deadline = time.perf_counter() + options.shutdown_timeout
            while any(p.alive() for pid, p in processes.items() if pid != main.pid):
                if time.perf_counter() > helper_deadline:
                    raise RuntimeError("Fixture descendant did not exit cooperatively")
                time.sleep(.05)
            verification["finished"] = run_native_verifier(contract, "finished", trial_directory, options.shutdown_timeout)
            result = {"target": target["name"], "scenario": scenario["name"], "trial": trial,
                "pid": main.pid, "renderer": target.get("renderer", "unspecified"),
                "width": substitutions["width"], "height": substitutions["height"],
                "first_hwnd_ms": first_hwnd_ms, "first_visible_ms": first_visible_ms,
                "ready_observed_ms": ready_observed_ms, "ready_poll_resolution_ms": 20,
                "sampling_elapsed_s": sampling_elapsed,
                "warmup_s": warmup, "duration_s_requested": duration,
                "process_lifetime_observed_s": exit_observed - started,
                "cooperative_stop_to_exit_ms": (exit_observed - stop_requested) * 1000 if uses_stop else None,
                "exit_code": process.returncode, "events": read_events(events_path),
                "ready": read_json(ready_path), "finished": finished,
                "window_geometry": geometry,
                "window_enumeration_anchor": api.window_anchor.metadata(anchor_before),
                "startup_main_counters": startup_counters,
                "lifetime_process_counters": {str(pid): p.lifetime() for pid, p in processes.items()},
                "native_interaction": {"kind": interaction.kind, "messages": interaction.actions},
                "foreground_policy": {"required_while_visible": expected == "visible",
                    "activations": window_policy.activations,
                    "pointer": "inside scroll body for scroll; quiet top area for other visible cases; restored after trial"},
                "gpu_counter_errors": gpu.errors, "errors": errors,
                "gpu_sample_status_counts": status_counts(rows, "gpu_status"),
                "memory_sample_status_counts": status_counts(rows, "memory_api_status"),
                "thread_sample_status_counts": status_counts(rows, "thread_context_switches_status"),
                "descendants_observed": [{"pid": p.pid, "parent": p.parent,
                    "created_filetime": p.created} for p in processes.values() if p.pid != main.pid],
                "scope_summaries": {role: summarize([r for r in rows if r["role"] == role])
                    for role in ("main", "helpers", "combined", "sampler", "system_package", "compositor", "system_context")},
                "process_summaries": {str(pid): summarize([r for r in rows if r["pid"] == pid])
                                      for pid in processes},
                "csv": stem + ".csv", "log": log_path.name}
            if contract and "native_verifier" in contract:
                result["scheduler_native_verification"] = verification
            if thread_cycles is not None:
                result["thread_cycles"] = {
                    "csv": stem + ".threads.csv", "row_count": len(thread_rows),
                    "sample_count": thread_cycles.sample_count,
                    "sample_status_counts": status_counts(thread_rows, "thread_cycle_status"),
                    "description_status_counts": status_counts(thread_rows, "description_status"),
                    "coverage": "polling_observed_threads_only"}
                write_csv(directory / result["thread_cycles"]["csv"], thread_rows)
            write_csv(directory / result["csv"], rows)
            return result
    finally:
        if process and process.poll() is None:
            # Only request a fixture-owned stop; never kill a normal production app.
            if any("{stop_file}" in str(arg) for arg in target.get("args", []) + scenario.get("args", [])):
                stop_path.write_text("stop\n", encoding="ascii")
            try:
                process.wait(timeout=lifetime + options.shutdown_timeout)
            except subprocess.TimeoutExpired:
                print(f"Fixture PID {process.pid} did not exit; no process was terminated.", file=sys.stderr)
        for tracked in processes.values():
            tracked.close()
        if "sampler" in locals():
            sampler.close()
        if "window_policy" in locals():
            window_policy.restore_pointer()
        gpu.close()
        if rows and not (directory / (stem + ".csv")).exists():
            write_csv(directory / (stem + ".partial.csv"), rows)
        if thread_rows and not (directory / (stem + ".csv")).exists():
            write_csv(directory / (stem + ".threads.partial.csv"), thread_rows)


def metadata():
    script = """
$ErrorActionPreference='Stop'
$cpu=Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors,MaxClockSpeed
$gpu=Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion,AdapterRAM,CurrentHorizontalResolution,CurrentVerticalResolution
$system=Get-CimInstance Win32_ComputerSystem | Select-Object TotalPhysicalMemory
[pscustomobject]@{cpu=@($cpu);gpu=@($gpu);system=$system} | ConvertTo-Json -Depth 5 -Compress
"""
    result = {"platform": platform.platform(), "python": platform.python_version(),
              "logical_processors": os.cpu_count(), "processor": platform.processor()}
    try:
        completed = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", script],
            capture_output=True, text=True, timeout=30, creationflags=subprocess.CREATE_NO_WINDOW, check=True)
        result.update(json.loads(completed.stdout))
    except (OSError, subprocess.SubprocessError, ValueError) as error:
        result["hardware_metadata_error"] = str(error)
    completed = subprocess.run(["powercfg", "/getactivescheme"], capture_output=True,
        text=True, timeout=10, creationflags=subprocess.CREATE_NO_WINDOW)
    result["active_power_scheme"] = completed.stdout.strip() if completed.returncode == 0 else "unavailable"
    return result


def baseline(api, options, directory, label):
    """Measure this collector and desktop/package context with no target app."""
    gpu = Gpu(not options.no_gpu, options.system_energy, options.thread_context_switches)
    sampler = Process(api, os.getpid(), "sampler")
    gpu.add(sampler.pid)
    if options.thread_context_switches:
        gpu.add_thread_image(Path(sys.executable).stem)
    rows, previous = [], {}
    started, next_sample = time.perf_counter(), time.perf_counter()
    try:
        gpu.collect()
        while True:
            begin = time.perf_counter()
            snapshot = api.snapshot()
            if api.compositor_pid:
                gpu.add(api.compositor_pid)
            error = gpu.collect()
            system = api.system_context()
            system.update(role="system_context", pid="", elapsed_s=time.perf_counter() - started)
            add_system_rates(system, previous.get("system_context"))
            previous["system_context"] = system.copy()
            rows.append(system)
            sampler_row = sampler.read(snapshot.get(sampler.pid, (None, None))[1])
            if options.thread_context_switches:
                sampler_row.update(gpu.context_switches(sampler.pid))
            sampler_row["elapsed_s"] = time.perf_counter() - started
            add_rates(sampler_row, previous.get("sampler"), os.cpu_count() or 1)
            previous["sampler"] = sampler_row.copy()
            sampler_row["sample_collection_ms"] = (time.perf_counter() - begin) * 1000
            rows.append(sampler_row)
            if options.system_energy:
                package = gpu.energy(error)
                package.update(role="system_package", pid="", elapsed_s=time.perf_counter() - started)
                add_energy_rates(package, previous.get("system_package"))
                previous["system_package"] = package.copy()
                rows.append(package)
            if api.compositor_pid:
                compositor = gpu.read(api.compositor_pid, error)
                compositor.update(role="compositor", pid=api.compositor_pid, elapsed_s=time.perf_counter() - started)
                rows.append(compositor)
            if time.perf_counter() - started >= options.baseline_duration:
                break
            next_sample += options.interval
            time.sleep(max(0, next_sample - time.perf_counter()))
        csv_name = f"baseline-{label}.csv"
        write_csv(directory / csv_name, rows)
        return {"label": label, "duration_s": time.perf_counter() - started,
                "target_application": "none", "csv": csv_name, "gpu_counter_errors": gpu.errors,
                "gpu_sample_status_counts": status_counts(rows, "gpu_status"),
                "scope_summaries": {role: summarize([r for r in rows if r["role"] == role])
                    for role in ("sampler", "system_package", "compositor", "system_context")}}
    finally:
        sampler.close()
        gpu.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--warmup", type=float, default=5)
    parser.add_argument("--duration", type=float, default=15)
    parser.add_argument("--interval", type=float, default=.5)
    parser.add_argument("--trials", type=int, default=3)
    parser.add_argument("--startup-timeout", type=float, default=15)
    parser.add_argument("--shutdown-timeout", type=float, default=20)
    parser.add_argument("--no-gpu", action="store_true")
    parser.add_argument("--system-energy", action="store_true",
                        help="sample optional whole-CPU-package RAPL meter, never per-app watts")
    parser.add_argument("--baseline-duration", type=float, default=0,
                        help="no-app collector/package/compositor baselines before and after trials")
    parser.add_argument("--baseline-only", action="store_true")
    parser.add_argument("--thread-context-switches", action="store_true",
                        help="map target-image PDH thread instances to PID and sum scheduled context switches")
    parser.add_argument("--thread-cycles", action="store_true",
                        help="diagnose observed target/validated-descendant threads in a separate identity-safe cycle CSV")
    parser.add_argument("--memory-ex2", action="store_true",
                        help="opt in to EX2 private resident/shared commit diagnostics; default preserves the EX core protocol")
    parser.add_argument("--allow-enabled-automation", action="store_true",
                        help="allow only scenarios declaring a validated isolated scheduler automation_contract")
    parser.add_argument("--resume", action="store_true",
                        help="continue unaccepted trials, preserving accepted results and failed attempt files")
    args = parser.parse_args()
    if args.warmup < 0 or min(args.duration, args.interval, args.startup_timeout, args.shutdown_timeout) <= 0 or args.trials < 1:
        parser.error("warmup must be nonnegative; durations, interval, and trials must be positive")
    if not args.config and not args.baseline_only:
        parser.error("--config is required unless --baseline-only is set")
    if args.baseline_duration < 0 or (args.baseline_only and args.baseline_duration <= 0):
        parser.error("--baseline-duration must be positive for --baseline-only")
    config = json.loads(args.config.read_text(encoding="utf-8-sig")) if args.config else {"targets": [], "scenarios": []}
    if not args.baseline_only and (not config.get("targets") or not config.get("scenarios")):
        parser.error("config needs nonempty targets and scenarios")
    for group in (config["targets"], config["scenarios"]):
        names = [entry["name"] for entry in group]
        if len(set(names)) != len(names) or any(not name or any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_" for c in name) for name in names):
            parser.error("target/scenario names must be unique ASCII letters, digits, hyphens, or underscores")
    for scenario in config["scenarios"]:
        try:
            validate_automation_contract(scenario, args.allow_enabled_automation)
        except (ValueError, RuntimeError) as error:
            parser.error(str(error))
    args.out.mkdir(parents=True, exist_ok=True)
    args.out = args.out.resolve()
    output = args.out / "measurements.json"
    if output.exists() and not args.resume:
        parser.error("output directory already contains measurements.json")
    if args.resume and (not output.exists() or args.baseline_only):
        parser.error("--resume requires existing measurements and cannot be combined with --baseline-only")
    for target in config["targets"]:
        exe = Path(target["exe"]).resolve(strict=True)
        target["exe"] = str(exe)
        target["binary_bytes"] = exe.stat().st_size
        digest = hashlib.sha256()
        with exe.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
        target["sha256"] = digest.hexdigest()
    api = Windows(args.memory_ex2, args.thread_cycles)
    try:
        measure(api, args, config, output)
    finally:
        api.close_window_anchor()


def measure(api, args, config, output):
    result = {"recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "sampler_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "hardware": metadata(), "config": config,
        "protocol": {"warmup_s": args.warmup, "duration_s": args.duration,
            "interval_s": args.interval, "trials": args.trials,
            "order": "AB/BA alternation; rotating scenarios", "gpu_enabled": not args.no_gpu,
            "system_energy_enabled": args.system_energy,
            "thread_context_switches_enabled": args.thread_context_switches,
            "memory_ex2_enabled": args.memory_ex2,
            "enabled_automation_allowed": args.allow_enabled_automation,
            "window_enumeration_anchor": WINDOW_ENUMERATION_ANCHOR,
            "sampler_per_monitor_v2_awareness_set": api.dpi_awareness_set},
        "limitations": ["Warm-cache process launches; no OS cache flush or reboot control.",
            "Cycles are OS-reported process cycles, not instructions retired or energy.",
            "Per-sample percentiles describe resource samples, not frame/input latency.",
            "PageFaultCount includes soft faults, not a hard-fault measurement.",
            "I/O counters include cached/device I/O, not physical-disk traffic.",
            "Combined working sets can double-count shared pages; private bytes measure private commit, not resident RAM.",
            "Child discovery is polling-based; very short-lived descendants may be missed.",
            "GPU missing instances are unavailable; compositor/DWM work is not charged to the target PID.",
            "DWM is shared by all desktop apps; its counters are context and cannot be attributed to the target.",
            "RAPL CPU-package energy includes all software and collector overhead; it is not per-app, discrete-GPU, or wall power.",
            "GPU process memory is reported per PID; process-tree sums are unsupported because allocations can overlap.",
            "Sums of individual process lifetime peaks are labeled as such; they are not simultaneous tree peaks.",
            "Optional PDH thread rates and coverage are unvalidated provider reports, not hardware wakeup counts; the expensive profile must not be pooled with core CPU/power results.",
            "GPU Process Memory may overreport on affected systems (Microsoft KB 4490156).",
            "Sampler resource usage is measured separately and not subtracted from app counters."],
        "results": [], "baselines": []}
    if args.thread_cycles:
        result["protocol"]["thread_cycles_enabled"] = True
        result["thread_cycle_profile"] = {
            "scope": "measured main process and validated descendants only; no-app baselines have no thread targets",
            "identity": "PID + process creation FILETIME + TID + thread creation FILETIME; handle owner revalidated",
            "rights": "THREAD_QUERY_LIMITED_INFORMATION",
            "apis": ["CreateToolhelp32Snapshot/Thread32First/Thread32Next", "OpenThread",
                     "GetProcessIdOfThread", "GetThreadTimes", "GetExitCodeThread", "QueryThreadCycleTime",
                     "GetThreadDescription (optional); LocalFree"],
            "cadence": "once per process collection at --interval; individual query timestamps use actual elapsed time",
            "coverage": "polling can miss short-lived threads and final work; first observations and gaps have no delta",
            "descriptions": "optional changeable/nonunique metadata; generic names do not identify code or libraries",
            "observer_effect": "included in sampler CPU and sample_collection_ms; thread_cycles_collection_ms isolates elapsed collection cost",
            "interpretation": "cycles are OS-reported cycles, not instructions, energy, wakeups, or a complete process-cycle decomposition"}
        result["limitations"].append(
            "Opt-in thread cycles use non-atomic polling with incomplete lifetime coverage; process/thread intervals differ. "
            "Do not subtract observed thread deltas as exact residual work or pool diagnostic CPU/power with core trials.")
        print("Warning: per-thread cycles are an opt-in polling diagnostic with observer overhead and incomplete lifetime coverage.",
              file=sys.stderr)
    if args.thread_context_switches:
        result["thread_profile"] = {
            "validation": "provider rates and thread coverage are unvalidated",
            "physical_hardware_wakeups": "not measured",
            "interpretation": "matching counter values are reported without independent full-thread coverage validation",
            "observer_effect": "expensive diagnostic profile; do not pool CPU or power with core measurements"}
        print("Warning: thread-counter rates and coverage are unvalidated; they do not measure hardware wakeups. "
              "This expensive diagnostic profile must not be pooled with core CPU/power results.", file=sys.stderr)
    completed = set()
    if args.resume:
        thread_profile = result.get("thread_profile")
        result, completed = resume_result(output, result)
        if thread_profile:
            result["thread_profile"] = thread_profile
        output.write_text(json.dumps(result, indent=2), encoding="utf-8")
    sampler_hash = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    baseline_suffix = f"-resume-{len(result.get('resume_history', []))}" if args.resume else ""
    if args.baseline_duration:
        result["baselines"].append(baseline(api, args, args.out, "before" + baseline_suffix))
        output.write_text(json.dumps(result, indent=2), encoding="utf-8")
    if args.baseline_only:
        print(json.dumps(result["baselines"][0]), flush=True)
        return
    for trial, target, scenario in trial_order(config["targets"], config["scenarios"], args.trials):
        if (trial, target["name"], scenario["name"]) in completed:
            continue
        try:
            measured = run(api, target, scenario, args, args.out.resolve(), trial)
            measured["sampler_sha256"] = sampler_hash
            result["results"].append(measured)
            output.write_text(json.dumps(result, indent=2), encoding="utf-8")
            main_summary = measured["scope_summaries"]["combined"]
            print(json.dumps({"trial": trial, "target": target["name"], "scenario": scenario["name"],
                "cpu_percent_one_core": main_summary.get("cpu_percent_one_core_weighted"),
                "private_mib_p50": main_summary.get("private_bytes", {}).get("p50", 0) / 2**20}), flush=True)
        except Exception as error:
            result["failure"] = {"trial": trial, "target": target["name"],
                                 "scenario": scenario["name"], "error": str(error),
                                 "sampler_sha256": sampler_hash}
            if isinstance(error, WindowStateValidationError):
                result["failure"]["window_validation_failure"] = error.window_validation_failure
            output.write_text(json.dumps(result, indent=2), encoding="utf-8")
            raise
    if args.baseline_duration:
        result["baselines"].append(baseline(api, args, args.out, "after" + baseline_suffix))
        output.write_text(json.dumps(result, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
