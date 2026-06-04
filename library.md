# Reference Library

This file collects the Windows API references behind PowerLeaf features.

## Power Plan Switching

PowerLeaf switches Windows power plans through the native Win32 power management APIs. It does not call the `powercfg` command-line tool.

Implementation entry point:

- `src/power/powercfg.rs`

User-facing behavior:

- PowerLeaf enumerates available Windows power schemes.
- It reads each scheme's friendly display name.
- It reads the currently active scheme GUID.
- It maps PowerLeaf's logical `Idle plan` and `Active plan` settings to Windows power scheme GUIDs.
- When automation decides to switch mode, PowerLeaf calls the Windows API to set the selected scheme as active.

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
| `GUID` | Identifies each Windows power scheme. PowerLeaf stores GUIDs as lowercase strings in settings. | https://learn.microsoft.com/en-us/windows/win32/api/guiddef/ns-guiddef-guid |
| `LocalFree` | Frees the GUID pointer returned by `PowerGetActiveScheme`. | https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-localfree |
| System error codes | Power APIs return Win32 error codes such as `ERROR_SUCCESS`, `ERROR_MORE_DATA`, and `ERROR_NO_MORE_ITEMS`. | https://learn.microsoft.com/en-us/windows/win32/debug/system-error-codes |

## Efficiency Mode / EcoQoS

PowerLeaf Efficiency Mode applies Windows EcoQoS to selected background user-session processes. It also lowers the target process priority to idle priority, matching the practical behavior users expect from Task Manager-style Efficiency Mode.

Implementation entry point:

- `src/ecoqos/mod.rs`

User-facing behavior:

- PowerLeaf finds background processes in the current Windows session.
- It skips PowerLeaf itself, built-in Windows shell/input/system processes, protected/elevated processes it cannot open, and apps in `Efficiency Whitelist`.
- If `Exclude foreground app` is enabled, it also skips the focused app and same-name foreground app processes.
- It reads the process's existing power throttling state and priority class when possible.
- It enables EcoQoS by setting `PROCESS_POWER_THROTTLING_EXECUTION_SPEED` through `SetProcessInformation`.
- It sets the process priority class to `IDLE_PRIORITY_CLASS`.
- It restores the previous throttling state and priority class when the process stops being a target, Efficiency Mode is disabled, automation is disabled, or PowerLeaf exits.

### EcoQoS APIs

| API | Used for | Reference |
| --- | --- | --- |
| `SetProcessInformation` | Applies `ProcessPowerThrottling` with `PROCESS_POWER_THROTTLING_STATE` to enable or clear EcoQoS. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessinformation |
| `GetProcessInformation` | Reads the current `PROCESS_POWER_THROTTLING_STATE` before PowerLeaf changes it, so the state can be restored later. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessinformation |
| `PROCESS_INFORMATION_CLASS` | Defines `ProcessPowerThrottling`, the information class used for process power throttling. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/ne-processthreadsapi-process_information_class |
| `PROCESS_POWER_THROTTLING_STATE` | Holds the throttling version, control mask, and state mask used for EcoQoS. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/ns-processthreadsapi-process_power_throttling_state |
| Quality of Service | Explains Windows QoS levels and that `SetProcessInformation` can explicitly tag a process as EcoQoS by toggling `PROCESS_POWER_THROTTLING_EXECUTION_SPEED`. | https://learn.microsoft.com/en-us/windows/win32/procthread/quality-of-service |

Important behavior from Microsoft: enabling `PROCESS_POWER_THROTTLING_EXECUTION_SPEED` classifies the process as EcoQoS. Windows then tries to improve power efficiency through strategies such as lower CPU frequency or more efficient CPU cores. EcoQoS should be used for work that is not part of the foreground user experience.

### Priority APIs

| API | Used for | Reference |
| --- | --- | --- |
| `GetPriorityClass` | Reads the existing priority class before PowerLeaf changes it. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getpriorityclass |
| `SetPriorityClass` | Sets `IDLE_PRIORITY_CLASS` while Efficiency Mode is active, then restores the previous class or `NORMAL_PRIORITY_CLASS`. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setpriorityclass |
| Scheduling Priorities | Documents process priority classes such as idle and normal. | https://learn.microsoft.com/en-us/windows/win32/procthread/scheduling-priorities |

### Process Access APIs

| API | Used for | Reference |
| --- | --- | --- |
| `OpenProcess` | Opens target processes with query and set-information access rights. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocess |
| Process Security and Access Rights | Documents access flags such as `PROCESS_QUERY_LIMITED_INFORMATION`, `PROCESS_SET_INFORMATION`, and `PROCESS_SET_LIMITED_INFORMATION`. | https://learn.microsoft.com/en-us/windows/win32/procthread/process-security-and-access-rights |

## App Suspension

PowerLeaf App Suspension is manual Win32 thread suspension. It is not the same as Windows-managed UWP app suspension shown by Task Manager for some Store apps.

Implementation entry point:

- `src/suspension/mod.rs`

User-facing behavior:

- PowerLeaf finds selected background apps from `Suspendable Apps`.
- After the configured background delay, PowerLeaf opens the target process and assigns it to a private Windows Job Object.
- It freezes that private job with `SetInformationJobObject` and thaws the same job when the focused or clicked app needs to recover, when the process is removed from the list, App Suspension is disabled, automation is disabled, or PowerLeaf exits.
- Taskbar and tray shell clicks temporarily thaw suspended top-level window owner processes only, so minimized and tray-hidden apps can restore without thawing unrelated non-window worker processes. Repeated shell clicks do not keep extending the thaw window.

### Job Object Freeze APIs

| API | Used for | Reference |
| --- | --- | --- |
| `OpenProcess` | Opens a process handle with the rights needed for job assignment and process liveness checks. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocess |
| `CreateJobObjectW` | Creates the private job object used to isolate one target process. | https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-createjobobjectw |
| `AssignProcessToJobObject` | Assigns the target process to PowerLeaf's private job object. | https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject |
| `SetInformationJobObject` | Freezes or thaws the private job object using the Windows Job Object freeze information class. | https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-setinformationjobobject |
| Job Objects | Explains Windows job objects and process grouping behavior. | https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects |

PowerLeaf keeps App Suspension opt-in and limited to explicitly selected apps because freezing a process is disruptive by design. Built-in exclusions also block Windows shell/input/UWP lifecycle processes such as `SearchApp.exe`, `SearchHost.exe`, and `SystemSettings.exe`, even if they are added to Suspendable Apps.

### Safety And Filtering APIs

| API | Used for | Reference |
| --- | --- | --- |
| `GetCurrentProcessId` | Gets PowerLeaf's own process ID so PowerLeaf never suspends itself. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getcurrentprocessid |
| `ProcessIdToSessionId` | Checks the Windows session for a process so PowerLeaf only targets the current user session. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-processidtosessionid |
| `WaitForSingleObject` | Checks whether a managed process has exited before reusing a cached freezer. | https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject |
| `CloseHandle` | Closes process and job handles after use. | https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-closehandle |
| `GetLastError` | Reads extended Win32 error codes after failed API calls. | https://learn.microsoft.com/en-us/windows/win32/api/errhandlingapi/nf-errhandlingapi-getlasterror |

### Related Windows Behavior

| Topic | Why it matters | Reference |
| --- | --- | --- |
| UWP app lifecycle | Explains Windows-managed UWP app suspension. This is the yellow pause/suspended state Task Manager can show for Store/UWP apps. PowerLeaf App Suspension is different. | https://learn.microsoft.com/en-us/windows/uwp/launch-resume/app-lifecycle |
