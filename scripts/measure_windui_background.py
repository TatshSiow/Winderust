"""Measure Windows idle CPU/memory with Winderust's opt-in isolated fixture.

Build with --features render-smoke. Automation is disabled; portable settings
and startup registration are skipped. Samples assert actual native window state.
The fixture exits through coordinated restoration. No process is force-killed.
CPU is a percentage of one logical core. Window creation time is not render
latency. Results do not establish energy consumption or an advantage over Iced.
"""
import argparse
import ctypes as C
from ctypes import wintypes as W
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import time
import platform
import os

user = C.WinDLL("user32", use_last_error=True)
kernel = C.WinDLL("kernel32", use_last_error=True)
psapi = C.WinDLL("psapi", use_last_error=True)
CALLBACK = C.WINFUNCTYPE(W.BOOL, W.HWND, W.LPARAM)


class Memory(C.Structure):
    _fields_ = [("cb", W.DWORD), ("PageFaultCount", W.DWORD)] + [
        (name, C.c_size_t) for name in (
            "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
            "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage",
            "PagefileUsage", "PeakPagefileUsage", "PrivateUsage")]


user.EnumWindows.argtypes = [CALLBACK, W.LPARAM]
user.EnumWindows.restype = W.BOOL
user.GetWindowThreadProcessId.argtypes = [W.HWND, C.POINTER(W.DWORD)]
user.IsWindowVisible.argtypes = [W.HWND]
user.IsWindowVisible.restype = W.BOOL
kernel.OpenProcess.argtypes = [W.DWORD, W.BOOL, W.DWORD]
kernel.OpenProcess.restype = W.HANDLE
kernel.CloseHandle.argtypes = [W.HANDLE]
kernel.CloseHandle.restype = W.BOOL
kernel.GetProcessTimes.argtypes = [W.HANDLE] + [C.POINTER(W.FILETIME)] * 4
kernel.GetProcessTimes.restype = W.BOOL
psapi.GetProcessMemoryInfo.argtypes = [W.HANDLE, C.POINTER(Memory), W.DWORD]
psapi.GetProcessMemoryInfo.restype = W.BOOL


def checked(value):
    if not value:
        raise C.WinError(C.get_last_error())
    return value


def cpu_seconds(handle):
    values = [W.FILETIME() for _ in range(4)]
    checked(kernel.GetProcessTimes(handle, *(C.byref(value) for value in values)))
    return sum((value.dwHighDateTime << 32) + value.dwLowDateTime
               for value in values[2:]) / 10_000_000


def memory(handle):
    value = Memory()
    value.cb = C.sizeof(value)
    checked(psapi.GetProcessMemoryInfo(handle, C.byref(value), value.cb))
    return value.PrivateUsage, value.WorkingSetSize

user.GetClassNameW.argtypes = [W.HWND, W.LPWSTR, C.c_int]
user.IsIconic.argtypes = [W.HWND]
user.IsIconic.restype = W.BOOL
user.ShowWindowAsync.argtypes = [W.HWND, C.c_int]
user.ShowWindowAsync.restype = W.BOOL
user.PostMessageW.argtypes = [W.HWND, W.UINT, W.WPARAM, W.LPARAM]
user.PostMessageW.restype = W.BOOL


def windows(pid):
    result = []
    @CALLBACK
    def visit(hwnd, _):
        owner = W.DWORD()
        user.GetWindowThreadProcessId(hwnd, C.byref(owner))
        name = C.create_unicode_buffer(128)
        user.GetClassNameW(hwnd, name, len(name))
        if owner.value == pid and name.value == 'WindUiWindowClass':
            result.append((hwnd, bool(user.IsWindowVisible(hwnd)), bool(user.IsIconic(hwnd))))
        return True
    checked(user.EnumWindows(visit, 0))
    return result


def state(pid):
    ws = windows(pid)
    if not ws:
        return 'absent'
    if any(visible and minimized for _, visible, minimized in ws):
        return 'minimized'
    if any(visible for _, visible, _ in ws):
        return 'visible'
    return 'hidden'


def run(exe, mode, warmup, duration, trial, directory):
    lifetime_ms = int((warmup + duration + 12) * 1000)
    args = [str(exe), '--windui-background-fixture-ms', str(lifetime_ms)]
    if mode != 'absent':
        args.append('--windui-background-fixture-visible')
    logpath = directory / f'{mode}-{trial}.log'
    with logpath.open('w', encoding='utf-8') as log:
        started = time.perf_counter()
        process = subprocess.Popen(args, cwd=exe.parent, stdout=log, stderr=log,
                                   creationflags=subprocess.CREATE_NO_WINDOW)
        handle = None
        try:
            handle = checked(kernel.OpenProcess(0x0400 | 0x0010, False, process.pid))
            while time.perf_counter() - started < 10:
                if process.poll() is not None:
                    raise RuntimeError(f'Fixture exited early: {logpath}')
                if mode == 'absent':
                    if time.perf_counter() - started > 2:
                        break
                elif windows(process.pid):
                    break
                time.sleep(.02)
            first_ui_ms = (time.perf_counter() - started) * 1000 if mode != 'absent' else None
            if mode != 'absent':
                # HWND creation precedes the framework's initial ShowWindow.
                # Wait for that show before applying a measurement state.
                show_deadline = time.perf_counter() + 5
                while state(process.pid) != 'visible':
                    if process.poll() is not None or time.perf_counter() > show_deadline:
                        raise RuntimeError('Fixture did not show its initial native window')
                    time.sleep(.02)
                # Also allow the production tray-registration probe to settle.
                time.sleep(2)
            if mode in ('hidden', 'minimized'):
                ws = windows(process.pid)
                assert len(ws) == 1, ws
                checked(user.ShowWindowAsync(ws[0][0], 0 if mode == 'hidden' else 6))
            if mode == 'closed':
                # The sample asserts normal close-to-background actually destroyed
                # the window; no tray-availability guard is bypassed here.
                ws = windows(process.pid)
                assert len(ws) == 1, ws
                checked(user.PostMessageW(ws[0][0], 0x0010, 0, 0))
            time.sleep(warmup)
            samples = []
            cpu_start = cpu_seconds(handle)
            sample_start = time.perf_counter()
            while time.perf_counter() - sample_start < duration:
                if process.poll() is not None:
                    raise RuntimeError('Fixture ended during sampling')
                private, working = memory(handle)
                observed = state(process.pid)
                expected = 'absent' if mode == 'closed' else mode
                assert observed == expected, f'Expected {expected} ({mode}); actual {observed}'
                samples.append({'elapsed_s': time.perf_counter()-sample_start,
                                'private_bytes': private, 'working_set_bytes': working,
                                'state': observed})
                time.sleep(.5)
            elapsed = time.perf_counter() - sample_start
            cpu = cpu_seconds(handle) - cpu_start
            exit_code = process.wait(timeout=20)
            assert exit_code == 0, f'{logpath}: exit {exit_code}'
            return {'mode': mode, 'trial': trial, 'first_ui_ms': first_ui_ms,
                    'elapsed_s': elapsed, 'cpu_s': cpu, 'cpu_percent_one_core': 100*cpu/elapsed,
                    'private_mib_median': statistics.median(s['private_bytes'] for s in samples)/2**20,
                    'working_mib_median': statistics.median(s['working_set_bytes'] for s in samples)/2**20,
                    'samples': samples, 'log': str(logpath)}
        finally:
            if handle:
                kernel.CloseHandle(handle)
            if process.poll() is None:
                # The isolated fixture disables all automation and owns no user config.
                # Let its timed coordinated shutdown finish before reporting an error.
                process.wait(timeout=lifetime_ms/1000+20)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--warmup', type=float, default=6)
    parser.add_argument('--duration', type=float, default=15)
    parser.add_argument('--trials', type=int, default=3)
    parser.add_argument('--modes', nargs='+', choices=['absent', 'visible', 'minimized', 'hidden', 'closed'],
                        default=['absent', 'visible', 'minimized', 'hidden', 'closed'])
    args = parser.parse_args()
    if args.warmup < 0 or args.duration <= 0 or args.trials <= 0:
        parser.error('warmup must be nonnegative; duration and trials must be positive')
    exe = args.exe.resolve(strict=True)
    args.out.mkdir(parents=True, exist_ok=True)
    results = {'platform': platform.platform(), 'processor': platform.processor(),
               'logical_processors': os.cpu_count(), 'recorded_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
               'binary': str(exe), 'sha256': hashlib.sha256(exe.read_bytes()).hexdigest(),
               'automation': 'disabled', 'build': 'release with opt-in render-smoke fixture',
               'comparison': 'No matched Iced full-application run; no relative-performance claim.',
               'warmup_s': args.warmup, 'duration_s': args.duration, 'results': []}
    output = args.out / 'measurements.json'
    for trial in range(1, args.trials+1):
        for mode in args.modes:
            measured = run(exe, mode, args.warmup, args.duration, trial, args.out)
            results['results'].append(measured)
            output.write_text(json.dumps(results, indent=2), encoding='utf-8')
            print(json.dumps({k:v for k,v in measured.items() if k != 'samples'}), flush=True)

if __name__ == '__main__':
    main()
