# Reference Library

This file collects the Windows API references behind Winderust features.
It covers feature-defining and compatibility-sensitive boundaries; routine
window rendering and infrastructure calls are not duplicated here.

## Maintenance Contract

- Keep each product surface mapped to its current implementation entry point.
- Link directly to Microsoft Learn API pages when they exist; use topic pages
  only for behavior shared by several calls.
- Mark NT, WDK, numeric information-class, or manually declared interfaces
  explicitly. Do not imply that an undocumented structure or value is a stable
  public SDK contract.
- Update this file in the same change that adds, removes, or changes a Windows
  API boundary.

## Feature API Index

| Product surface | Implementation | Windows boundary |
| --- | --- | --- |
| Power Plan Control and Advanced Power Plan Tuning | `src/power/powercfg.rs` | Power scheme enumeration, activation, duplication, deletion, and processor setting values |
| Adaptive Engine | `src/features/winderust_features/workload_engine.rs` | Process QoS, process priority, Dynamic Priority Boost, affinity masks, CPU Sets, and Memory Priority |
| Background Efficiency | `src/features/winderust_features/background_efficiency.rs` | Process power throttling and optional process-priority management |
| Memory Trim | `src/features/winderust_features/memory_trim.rs` | Working-set trimming, memory status, and compatibility-sensitive NT system information |
| CPU Control | `src/features/cpu_control/` | Process affinity masks and CPU Sets |
| Priority Control | `src/features/priority_control/` | Process/thread priorities, Dynamic Priority Boost, Memory Priority, NT I/O priority, and WDK GPU priority |
| App Suspension | `src/features/advanced_controls/app_suspension.rs`, `app_suspension/process_freezer.rs`, and `app_suspension/wake_activity.rs` | Job Objects, a compatibility-sensitive freeze information class, and audio/network wake detection |
| Timer Resolution | `src/features/advanced_controls/timer_resolution.rs` | WinMM timer capability, request, and release calls |
| Win32 Priority Separation | `src/ui/app/pages/win32_priority_separation_page.rs`, `src/ui/app/shared/appearance.rs`, and `src/backend/win_registry.rs` | Windows registry access and the `Win32PrioritySeparation` value |

## Power Plan Switching

Winderust switches Windows power plans through the native Win32 power management APIs. It does not call the `powercfg` command-line tool.

Implementation paths:

- `src/features/advanced_controls/app_suspension.rs`: policy coordinator.
- `src/features/advanced_controls/app_suspension/process_freezer.rs`: Job Object and process-handle boundary.
- `src/features/advanced_controls/app_suspension/wake_activity.rs`: audio and IP Helper wake detection.

User-facing behavior:

- Winderust enumerates available Windows power schemes.
- It reads each scheme's friendly display name.
- It reads the currently active scheme GUID.
- By Activity maps its visible `Idle plan` and `Active plan` settings to Windows power scheme GUIDs.
- By Foreground, By Running App, By CPU Load, and By Time store a selected GUID on each rule; missing selections do not fall back to a global plan.
- When automation decides to switch mode, Winderust calls the Windows API to set the selected scheme as active.
- Adaptive Engine creates a temporary plan named `Winderust Adaptive`; startup recovery only recognizes that exact current Winderust name/description pair.

### Power Scheme APIs

| API | Used for | Reference |
| --- | --- | --- |
| `PowerEnumerate` | Enumerates available power schemes using `ACCESS_SCHEME`. | https://learn.microsoft.com/en-us/windows/win32/api/powrprof/nf-powrprof-powerenumerate |
| `PowerReadFriendlyName` | Reads the display name for a power scheme GUID. | https://learn.microsoft.com/en-us/windows/win32/api/powrprof/nf-powrprof-powerreadfriendlyname |
| `PowerGetActiveScheme` | Reads the currently active power scheme GUID. | https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powergetactivescheme |
| `PowerSetActiveScheme` | Sets the selected Windows power scheme as active. | https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powersetactivescheme |
| PowrProf API header | Lists power management functions exposed by `powrprof.h` / `PowrProf.dll`. | https://learn.microsoft.com/en-us/windows/win32/api/powrprof/ |

### Power Plan Support APIs

| API | Used for | Reference |
| --- | --- | --- |
| `GUID` | Identifies each Windows power scheme. Winderust stores GUIDs as lowercase strings in settings. | https://learn.microsoft.com/en-us/windows/win32/api/guiddef/ns-guiddef-guid |
| `LocalFree` | Frees the GUID pointer returned by `PowerGetActiveScheme`. | https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-localfree |
| System error codes | Power APIs return Win32 error codes such as `ERROR_SUCCESS`, `ERROR_MORE_DATA`, and `ERROR_NO_MORE_ITEMS`. | https://learn.microsoft.com/en-us/windows/win32/debug/system-error-codes |

## Background Efficiency / EcoQoS

Winderust Background Efficiency applies Windows EcoQoS to selected background
user-session processes. It also manages idle process priority when Process
Priority is disabled; when Process Priority is enabled, that feature remains
the sole owner of process-priority changes.

Implementation paths:

- `src/features/advanced_controls/app_suspension.rs`: policy coordinator.
- `src/features/advanced_controls/app_suspension/process_freezer.rs`: Job Object and process-handle boundary.
- `src/features/advanced_controls/app_suspension/wake_activity.rs`: audio and IP Helper wake detection.

User-facing behavior:

- Winderust finds background processes in the current Windows session.
- It skips Winderust itself, built-in Windows shell/input/system processes, protected/elevated processes it cannot open, and apps in Background Efficiency custom rules.
- If `Exclude foreground app` is enabled, it also skips the focused app and same-name foreground app processes.
- It reads the process's existing power throttling state and, when it owns
  priority management, the existing priority class.
- It enables EcoQoS by setting `PROCESS_POWER_THROTTLING_EXECUTION_SPEED` through `SetProcessInformation`.
- When Process Priority is disabled, it sets the process priority class to
  `IDLE_PRIORITY_CLASS`. Otherwise it leaves priority unchanged.
- It restores the state it owns when the process stops being a target,
  Background Efficiency is disabled, automation is disabled, or Winderust
  exits.

### EcoQoS APIs

| API | Used for | Reference |
| --- | --- | --- |
| `SetProcessInformation` | Applies `ProcessPowerThrottling` with `PROCESS_POWER_THROTTLING_STATE` to enable or clear EcoQoS. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessinformation |
| `GetProcessInformation` | Reads the current `PROCESS_POWER_THROTTLING_STATE` before Winderust changes it, so the state can be restored later. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessinformation |
| `PROCESS_INFORMATION_CLASS` | Defines `ProcessPowerThrottling`, the information class used for process power throttling. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/ne-processthreadsapi-process_information_class |
| `PROCESS_POWER_THROTTLING_STATE` | Holds the throttling version, control mask, and state mask used for EcoQoS. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/ns-processthreadsapi-process_power_throttling_state |
| Quality of Service | Explains Windows QoS levels and that `SetProcessInformation` can explicitly tag a process as EcoQoS by toggling `PROCESS_POWER_THROTTLING_EXECUTION_SPEED`. | https://learn.microsoft.com/en-us/windows/win32/procthread/quality-of-service |

Important behavior from Microsoft: enabling `PROCESS_POWER_THROTTLING_EXECUTION_SPEED` classifies the process as EcoQoS. Windows then tries to improve power efficiency through strategies such as lower CPU frequency or more efficient CPU cores. EcoQoS should be used for work that is not part of the foreground user experience.

### Priority APIs

| API | Used for | Reference |
| --- | --- | --- |
| `GetPriorityClass` | Reads the existing priority class before Winderust changes it. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getpriorityclass |
| `SetPriorityClass` | Sets `IDLE_PRIORITY_CLASS` only while Background Efficiency owns process priority, then restores the previous class. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setpriorityclass |
| Scheduling Priorities | Documents process priority classes such as idle and normal. | https://learn.microsoft.com/en-us/windows/win32/procthread/scheduling-priorities |

### Process Access APIs

| API | Used for | Reference |
| --- | --- | --- |
| `OpenProcess` | Opens target processes with query and set-information access rights. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocess |
| Process Security and Access Rights | Documents access flags such as `PROCESS_QUERY_LIMITED_INFORMATION`, `PROCESS_SET_INFORMATION`, and `PROCESS_SET_LIMITED_INFORMATION`. | https://learn.microsoft.com/en-us/windows/win32/procthread/process-security-and-access-rights |

## Advanced Power Plan Tuning

Winderust can apply separate AC and battery processor-power percentages and processor boost modes to a selected Windows power plan, with presets available as quick-fill values. This is system-wide power-plan tuning, not per-process Core Steering.

Implementation paths:

- `src/features/advanced_controls/app_suspension.rs`: policy coordinator.
- `src/features/advanced_controls/app_suspension/process_freezer.rs`: Job Object and process-handle boundary.
- `src/features/advanced_controls/app_suspension/wake_activity.rs`: audio and IP Helper wake detection.

| API / Setting | Used for | Reference |
| --- | --- | --- |
| `PowerWriteACValueIndex` / `PowerWriteDCValueIndex` | Writes separate AC and battery processor setting percentages in the selected power plan. | [AC](https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powerwriteacvalueindex) / [DC](https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powerwritedcvalueindex) |
| `PowerReadACValueIndex` / `PowerReadDCValueIndex` | Reads the selected power plan's AC and battery processor setting percentages so the UI can reflect current Windows values. | [AC](https://learn.microsoft.com/en-us/windows/win32/api/powrprof/nf-powrprof-powerreadacvalueindex) / [DC](https://learn.microsoft.com/en-us/windows/win32/api/powrprof/nf-powrprof-powerreaddcvalueindex) |
| Processor power management options | Defines the processor settings and profiles used by Windows. | https://learn.microsoft.com/en-us/windows-hardware/customize/power-settings/configure-processor-power-management-options |
| Core parking minimum cores | Sets the percentage of logical processors that must remain unparked. | https://learn.microsoft.com/en-us/windows-hardware/customize/power-settings/options-for-core-parking-cpmincores |
| Processor performance min/max | Sets minimum and maximum processor performance percentages. | [Minimum](https://learn.microsoft.com/en-us/windows-hardware/customize/power-settings/options-for-perf-state-engine-minperformance) / [Maximum](https://learn.microsoft.com/en-us/windows-hardware/customize/power-settings/options-for-perf-state-engine-maxperformance) |
| Processor performance boost mode | Controls Windows processor boost policy values such as disabled, enabled, aggressive, and efficient modes. | https://learn.microsoft.com/en-us/windows-hardware/customize/power-settings/options-for-perf-state-engine-perfboostmode |

## Priority Control

Implementation entry points:

- `src/features/priority_control/process_priority.rs`
- `src/features/priority_control/thread_priority.rs`
- `src/features/priority_control/dynamic_priority_boost.rs`
- `src/features/priority_control/io_priority.rs`
- `src/features/priority_control/gpu_priority.rs`
- `src/features/priority_control/memory_priority.rs`

| Product feature / API | Used for | Reference |
| --- | --- | --- |
| Process Priority: `GetPriorityClass` / `SetPriorityClass` | Reads, applies, and restores process priority classes. | [Get](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getpriorityclass) / [Set](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setpriorityclass) |
| Thread Priority: `CreateToolhelp32Snapshot`, `Thread32First`, and `Thread32Next` | Enumerates threads belonging to target processes. | [Snapshot](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-createtoolhelp32snapshot) / [First](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-thread32first) / [Next](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-thread32next) |
| Thread Priority: `GetThreadPriority` / `SetThreadPriority` | Reads, applies, and restores thread priority values. | [Get](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreadpriority) / [Set](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setthreadpriority) |
| Thread Priority: `GetThreadTimes` | Records thread creation time so restoration does not target a recycled thread ID. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreadtimes |
| Dynamic Priority Boost: `GetProcessPriorityBoost` / `SetProcessPriorityBoost` | Reads, applies, and restores the process dynamic-priority-boost setting. | [Get](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesspriorityboost) / [Set](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocesspriorityboost) |
| Memory Priority: `GetProcessInformation` / `SetProcessInformation` | Reads, applies, and restores `ProcessMemoryPriority`. | [Get](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessinformation) / [Set](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessinformation) |
| `MEMORY_PRIORITY_INFORMATION` | Defines the memory-priority value passed to the process information APIs. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/ns-processthreadsapi-memory_priority_information |
| I/O Priority: `NtQueryInformationProcess` / `NtSetInformationProcess` | Reads, applies, and restores numeric process information class 33. | https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntqueryinformationprocess |
| GPU Priority: `D3DKMTGetProcessSchedulingPriorityClass` / `D3DKMTSetProcessSchedulingPriorityClass` | Reads, applies, and restores the WDK process GPU scheduling class. | [Get](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/d3dkmthk/nf-d3dkmthk-d3dkmtgetprocessschedulingpriorityclass) / [Set](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/d3dkmthk/nf-d3dkmthk-d3dkmtsetprocessschedulingpriorityclass) |
| `D3DKMT_SCHEDULINGPRIORITYCLASS` | Defines the GPU scheduling priority values. | https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/d3dkmthk/ne-d3dkmthk-_d3dkmt_schedulingpriorityclass |

Compatibility note: Microsoft documents `NtQueryInformationProcess` as an
internal interface that can change and recommends public alternatives where
available. The numeric I/O priority class and `NtSetInformationProcess` use in
Winderust are not documented as stable public SDK contracts. Keep their
declarations and class constant beside the I/O Priority implementation, preserve
failure handling, and revalidate them against supported Windows versions when
that boundary changes.

## Memory Trim

Implementation entry points:

- `src/features/winderust_features/memory_trim.rs`
- `src/backend/privilege.rs`

| API | Used for | Reference |
| --- | --- | --- |
| `GlobalMemoryStatusEx` | Reads overall physical-memory load and availability. | https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-globalmemorystatusex |
| `K32GetProcessMemoryInfo` | Reads process working-set counters before deciding whether a process is eligible for trimming. | https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getprocessmemoryinfo |
| `SetProcessWorkingSetSize` | Passes `SIZE_T(-1)` for both bounds to remove as many pages as possible from a target process working set. | https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-setprocessworkingsetsize |
| `NtQuerySystemInformation` | Queries the manually declared system memory-list structures used for free-memory calculations. | https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntquerysysteminformation |
| `NtSetSystemInformation` | Requests standby-list or system-file-cache purging through numeric information classes. | https://learn.microsoft.com/en-us/windows/win32/sysinfo/ntsetsysteminformation |

Compatibility note: Microsoft states that `NtSetSystemInformation` is not
declared by the Windows SDK and supports only a subset of system information
classes. Winderust's numeric classes 80 and 81 and their buffer layouts are
compatibility-sensitive. Keep the declarations, constants, layouts, privilege
requirements, and tests together in `memory_trim.rs`; do not reuse them as a
general system-information abstraction.

## Core Steering

Winderust Core Steering can apply hard process affinity masks, soft Windows CPU Sets, or Efficiency Mode OFF to selected current-session processes. On systems with more than one processor group, the status message warns that hard affinity uses the process primary processor group.

Implementation paths:

- `src/features/advanced_controls/app_suspension.rs`: policy coordinator.
- `src/features/advanced_controls/app_suspension/process_freezer.rs`: Job Object and process-handle boundary.
- `src/features/advanced_controls/app_suspension/wake_activity.rs`: audio and IP Helper wake detection.

### Core Steering APIs

| API | Used for | Reference |
| --- | --- | --- |
| `GetProcessAffinityMask` | Reads the current process and system affinity masks before Winderust changes them. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessaffinitymask |
| `SetProcessAffinityMask` | Applies the configured hard affinity mask to a target process. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessaffinitymask |
| `GetSystemCpuSetInformation` | Maps selected logical CPUs to Windows CPU Set IDs for soft affinity mode. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getsystemcpusetinformation |
| `GetProcessDefaultCpuSets` | Reads existing process default CPU Set IDs so soft mode can restore them later. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessdefaultcpusets |
| `SetProcessDefaultCpuSets` | Applies or clears process default CPU Set IDs for soft affinity mode. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessdefaultcpusets |
| `GetProcessInformation` / `SetProcessInformation` | Reads and clears `PROCESS_POWER_THROTTLING_EXECUTION_SPEED` for Efficiency Mode OFF rules. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessinformation |
| `GetActiveProcessorGroupCount` | Detects multi-group systems where single-mask affinity APIs are group-relative. | https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getactiveprocessorgroupcount |
| Processor Groups | Explains why hard affinity masks are group-relative and why multi-group systems need special handling. | https://learn.microsoft.com/en-us/windows/win32/procthread/processor-groups |
| CPU Sets | Explains soft processor preference while remaining more compatible with OS power management. | https://learn.microsoft.com/en-us/windows/win32/procthread/cpu-sets |

## App Suspension

Winderust App Suspension is manual Win32 Job Object freezing. It is not the same as Windows-managed UWP app suspension shown by Task Manager for some Store apps.

Implementation paths:

- `src/features/advanced_controls/app_suspension.rs`: policy coordinator.
- `src/features/advanced_controls/app_suspension/process_freezer.rs`: Job Object and process-handle boundary.
- `src/features/advanced_controls/app_suspension/wake_activity.rs`: audio and IP Helper wake detection.

User-facing behavior:

- Winderust finds selected background apps from `Suspendable Apps`.
- After the configured background delay, Winderust opens the target process and assigns it to a private Windows Job Object.
- It freezes that private job with `SetInformationJobObject` and thaws the same job when the focused or clicked app needs to recover, when the process is removed from the list, App Suspension is disabled, automation is disabled, or Winderust exits.
- Taskbar and tray shell clicks temporarily thaw suspended top-level window owner processes only, so minimized and tray-hidden apps can restore without thawing unrelated non-window worker processes. Repeated shell clicks do not keep extending the thaw window.

### Job Object Freeze APIs

| API | Used for | Reference |
| --- | --- | --- |
| `OpenProcess` | Opens a process handle with the rights needed for job assignment and process liveness checks. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocess |
| `CreateJobObjectW` | Creates the private job object used to isolate one target process. | https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-createjobobjectw |
| `AssignProcessToJobObject` | Assigns the target process to Winderust's private job object. | https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject |
| `IsProcessInJob` | Adds context to assignment failures when the target process is already in a job object. | https://learn.microsoft.com/en-us/windows/win32/api/jobapi/nf-jobapi-isprocessinjob |
| `SetInformationJobObject` | Freezes or thaws the private job object using the Windows Job Object freeze information class. | https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-setinformationjobobject |
| Job Objects | Explains Windows job objects and process grouping behavior. | https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects |

Compatibility note: `SetInformationJobObject` is public, but Microsoft does
not document information class 18 or Winderust's
`JobObjectFreezeInformation` layout as a public SDK contract. Keep the class
constant, structure, call site, thaw-on-drop behavior, and layout test together
in `app_suspension/process_freezer.rs`; revalidate this boundary against supported Windows
versions when it changes.

Winderust keeps App Suspension opt-in and limited to explicitly selected apps because freezing a process is disruptive by design. Built-in exclusions also block Windows shell/input/UWP lifecycle processes such as `SearchApp.exe`, `SearchHost.exe`, and `SystemSettings.exe`, even if they are added to Suspendable Apps.

### Safety And Filtering APIs

| API | Used for | Reference |
| --- | --- | --- |
| `GetCurrentProcessId` | Gets Winderust's own process ID so Winderust never suspends itself. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getcurrentprocessid |
| `ProcessIdToSessionId` | Checks the Windows session for a process so Winderust only targets the current user session. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-processidtosessionid |
| `WaitForSingleObject` | Checks whether a managed process has exited before reusing a cached freezer. | https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject |
| `CloseHandle` | Closes process and job handles after use. | https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-closehandle |
| `GetLastError` | Reads extended Win32 error codes after failed API calls. | https://learn.microsoft.com/en-us/windows/win32/api/errhandlingapi/nf-errhandlingapi-getlasterror |

### Related Windows Behavior

| Topic | Why it matters | Reference |
| --- | --- | --- |
| UWP app lifecycle | Explains Windows-managed UWP app suspension. This is the yellow pause/suspended state Task Manager can show for Store/UWP apps. Winderust App Suspension is different. | https://learn.microsoft.com/en-us/windows/uwp/launch-resume/app-lifecycle |

### Supplemental Security Context

These third-party articles explain risk context only; they are not API
contracts or substitutes for Microsoft documentation.

| Topic | Why it matters | Reference |
| --- | --- | --- |
| UWP lifecycle and Job Object abuse | Supports keeping App Suspension opt-in, excluding sensitive shell/UWP lifecycle processes, and avoiding broad background freezing. | https://www.orangecyberdefense.com/global/blog/threat/attack-technique-abuse-of-the-uwp-lifecycle-and-windows-job-objects |
| Remote thread hijacking | Supports avoiding memory writing, thread-context mutation, or injection-adjacent suspension behavior. | https://www.ired.team/offensive-security/code-injection-process-injection/injecting-to-remote-process-via-thread-hijacking |

## Timer Resolution

Implementation paths:

- `src/features/advanced_controls/app_suspension.rs`: policy coordinator.
- `src/features/advanced_controls/app_suspension/process_freezer.rs`: Job Object and process-handle boundary.
- `src/features/advanced_controls/app_suspension/wake_activity.rs`: audio and IP Helper wake detection.

| API | Used for | Reference |
| --- | --- | --- |
| `timeGetDevCaps` | Reads the timer service's supported minimum and maximum periods. | https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timegetdevcaps |
| `timeBeginPeriod` | Requests the millisecond period selected by the active foreground rule. | https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timebeginperiod |
| `timeEndPeriod` | Releases the matching request with the same period. | https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timeendperiod |
| Timer Resolution | Documents WinMM timer-resolution behavior and lifecycle requirements. | https://learn.microsoft.com/en-us/windows/win32/multimedia/timer-resolution |

Every successful `timeBeginPeriod` request must have one matching
`timeEndPeriod` call with the same period. Starting with Windows 10 version
2004, requests are primarily per-process; on Windows 11, Windows may not honor a
higher resolution for an occluded, minimized, invisible, and inaudible
window-owning process. Microsoft classifies multimedia timers as legacy and
recommends Multimedia Class Scheduler Service where it fits; Winderust retains
WinMM here because this feature is explicit timer-resolution control.

## Win32 Priority Separation

Implementation entry points:

- `src/ui/app/pages/win32_priority_separation_page.rs`
- `src/ui/app/shared/appearance.rs`
- `src/backend/win_registry.rs`

Winderust reads and writes the machine-wide `Win32PrioritySeparation` DWORD
under the Windows PriorityControl key and stores the original value as a
Winderust-owned per-user backup before the first change.

| API / Contract | Used for | Reference |
| --- | --- | --- |
| Windows Registry functions | Defines registry key/value access, access rights, and Win32 error handling. The Rust `winreg` wrapper is isolated in `src/backend/win_registry.rs`. | https://learn.microsoft.com/en-us/windows/win32/sysinfo/registry-functions |
| `Win32PrioritySeparation` value and bit layout | Decodes quantum duration, quantum behavior, and foreground boost for the Advanced page. | No stable public Microsoft API reference; project contract is in `src/ui/app/shared/appearance.rs` and its tests. |

Treat the value layout as compatibility-sensitive. Keep reading, backup,
writing, bit decoding, and tests aligned, and fail visibly if the machine value
cannot be read or written.
