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
| Power Plan Control and Advanced Power Plan Tuning | `src/rules/decision_engine.rs`, `src/control/power_plan.rs`, `src/application/advanced_power_plan_tuning.rs`, `src/power/powercfg.rs`, and `src/platform/windows/power_plan.rs` | Power policy, automatic lifecycle/recovery, typed persistent tuning, domain façade, and the sole native power-scheme boundary |
| Automation event wake handling | `src/backend/automation.rs`, `src/activity/input_hook.rs`, and `src/backend/windows_events.rs` | Runtime-owned low-level input hooks, foreground/window WinEvent hooks, power, suspend/resume, and session notifications |
| Winderust self-power | `src/backend/self_power.rs` and `src/platform/windows/self_power.rs` | Strict baseline/composition lifecycle plus the sole raw current-process priority and Power Throttling adapter |
| System tray lifecycle | `src/backend/tray.rs`, `src/ui/app.rs` | Notification-area icon, window-procedure subclassing, popup menu, restore/quit messages, and bounded install failure |
| Administrator relaunch and single-instance handoff | `src/backend/privilege.rs` and `src/main.rs` | Synchronous UAC process creation plus an explicit mutex handoff from the closing standard instance to its elevated replacement |
| Crash recovery watchdog | `src/backend/crash_recovery.rs` | Private inherited stdin journal, process/thread identity validation, reversible state replay, retained App Suspension and CPU Limiter freeze jobs, and automatic power-plan recovery |
| Adaptive Engine | `src/features/winderust_features/cpu_scheduler.rs`, `cpu_scheduler/policy.rs`, `cpu_scheduler/process_control.rs`, and `src/control/priority_efficiency.rs` | CPU scheduling decisions and read-only process sampling plus typed Process Priority and Power Throttling claims; affinity masks, CPU Sets, Memory Priority, and Dynamic Priority Boost route through their feature or typed-controller owners |
| Background Efficiency | `src/features/winderust_features/background_efficiency.rs` and `src/control/priority_efficiency.rs` | Policy-only target selection plus shared compound Process Priority and process Power Throttling ownership |
| Memory Trim | `src/features/winderust_features/memory_trim.rs`, `src/control/memory_trim.rs`, and `src/platform/windows/memory_trim.rs` | Memory-pressure policy, typed exact-process command, and raw working-set adapter |
| Stop Process / Stop Process Tree | `src/foreground/process_list.rs`, `src/control/process_termination.rs`, and `src/platform/windows/process_termination.rs` | Read-side tree capture, typed batch command, and raw termination adapter |
| CPU Control | `src/features/cpu_control/`, `src/control/cpu_allocation.rs`, `src/control/cpu_limiter.rs`, `src/control/suspension.rs`, and the CPU allocation, CPU Limiter timing, and shared Job Object adapters under `src/platform/windows/` | Policy, shared affinity/CPU Set ownership, wall-clock duty-cycle scheduling, shared freeze/thaw ownership, and narrow raw Windows adapters |
| Priority Control | `src/features/priority_control/`, `src/control/process.rs`, `src/control/priority_efficiency.rs`, `src/control/thread_priority.rs`, `src/control/dynamic_priority_boost.rs`, `src/control/io_priority.rs`, `src/control/gpu_priority.rs`, `src/control/memory_priority.rs`, and mechanism adapters in `src/platform/windows/` | Policy-only feature routes, typed property ownership/transactions, and narrow raw Windows adapters, including the compound Process Priority / Power Throttling adapter |
| Shared process-control acquisition | `src/control/process.rs` and `src/platform/windows/process.rs` | Typed exact identity/safety validation plus the sole operation-specific `OpenProcess` adapter for control commands |
| App Suspension | `src/features/advanced_controls/app_suspension.rs`, `src/control/suspension.rs`, `src/platform/windows/job.rs`, `src/platform/windows/suspension.rs`, and `app_suspension/wake_activity.rs` | Policy, shared App Suspension / CPU Limiter lifecycle controller, named Job Object primitives, compatibility-sensitive freeze information class, and audio/network wake detection |
| Timer Resolution | `src/features/advanced_controls/timer_resolution.rs`, `src/control/timer_resolution.rs`, and `src/platform/windows/timer_resolution.rs` | Foreground-rule policy, process-lifetime ownership, and the sole raw WinMM adapter |
| Win32 Priority Separation | `src/application/win32_priority_separation.rs`, `src/backend/win_registry.rs`, and `src/ui/win32_priority_separation.rs` | Typed persistent backup/apply/restore service, narrow registry adapter, and UI presentation for the `Win32PrioritySeparation` value |

## Power Plan Switching

Winderust switches Windows power plans through the native Win32 power management APIs. It does not call the `powercfg` command-line tool.

Implementation paths:

- `src/rules/decision_engine.rs`: unchanged ordinary power-plan precedence and page/rule selections.
- `src/backend/automation/runner.rs`: the single visible/hidden automatic decision route.
- `src/control/power_plan.rs`: automatic baseline, expected state, recovery transaction, verification, compensation, retry suppression, external-state rebase, and Adaptive-plan lifecycle.
- `src/power/powercfg.rs`: typed plan enumeration/tuning semantics and exact managed-plan recognition.
- `src/platform/windows/power_plan.rs`: GUID conversion, native power-scheme enumeration/mutation,
  processor-setting reads/writes, and effective-power-mode callback registration.

User-facing behavior:

- Winderust enumerates available Windows power schemes.
- It reads each scheme's friendly display name.
- It reads the currently active scheme GUID.
- By Activity maps its visible `Idle plan` and `Active plan` settings to Windows power scheme GUIDs.
- By Foreground, By Running App, By CPU Load, and By Time store a selected GUID on each rule; missing selections do not fall back to a global plan.
- When automation decides to switch mode, Winderust calls the Windows API to set the selected scheme as active.
- Adaptive Engine creates a temporary plan named `Winderust Adaptive`; startup recovery only recognizes that exact current Winderust name/description pair.
- Ordinary automation and the temporary Adaptive plan are mutually exclusive typed controller owners. Window visibility changes cadence, not ownership.
- Before each automatic mutation, the controller records a recovery intent, applies and verifies the active GUID, then commits the intent. Failures attempt compensation; an unrecovered intent remains for the crash helper.
- If an external actor breaks Winderust's expected GUID chain, the controller rebases its clean baseline instead of overwriting that choice.

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
| `GUID` | Identifies each Windows power scheme. `src/platform/windows/power_plan.rs` converts them to lowercase settings strings and rejects malformed or non-ASCII input without slicing panics. | https://learn.microsoft.com/en-us/windows/win32/api/guiddef/ns-guiddef-guid |
| `LocalFree` | Frees the GUID pointer returned by `PowerGetActiveScheme`. | https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-localfree |
| System error codes | Power APIs return Win32 error codes such as `ERROR_SUCCESS`, `ERROR_MORE_DATA`, and `ERROR_NO_MORE_ITEMS`. | https://learn.microsoft.com/en-us/windows/win32/debug/system-error-codes |
| `PowerRegisterForEffectivePowerModeNotifications` | Registers the callback that keeps the displayed Windows effective power mode current. Only `S_OK` indicates successful registration. | https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powerregisterforeffectivepowermodenotifications |
| `PowerUnregisterFromEffectivePowerModeNotifications` | Unregisters the callback and waits for active callbacks to complete. If it returns failure, Winderust retains the callback context for process lifetime because Windows may still reference it. | https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powerunregisterfromeffectivepowermodenotifications |

## Automation Event Watcher

`RuntimeHandle` owns `src/backend/windows_events.rs` as an RAII event source. The watcher retains one dedicated hidden-window thread for event-driven automation wakes. Startup is all-or-nothing: the hidden window, foreground hook, window-creation hook, every configured power-setting notification, suspend/resume notification, and current-session notification must all register successfully. Partial registrations are released immediately and the watcher remains inactive so automation retains its polling fallback. Runtime shutdown posts the source's quit message and joins its thread before automatic state is released.

| API | Used for | Reference |
| --- | --- | --- |
| `SetWinEventHook` | Receives foreground changes and top-level window creation events outside Winderust's own process. | https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook |
| `RegisterPowerSettingNotification` | Delivers A/C source, battery percentage, and power-scheme personality changes to the hidden window. | https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerpowersettingnotification |
| `RegisterSuspendResumeNotification` | Delivers suspend and resume notifications to the hidden window. | https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registersuspendresumenotification |
| `WTSRegisterSessionNotification` | Delivers current-session lock, unlock, logon, logoff, and related session changes. | https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsregistersessionnotification |

## Automation Input Hook

`RuntimeHandle` owns `src/activity/input_hook.rs` as an RAII event source. The low-level keyboard and mouse hooks retain their dedicated Windows message-loop thread so hook installation, callback dispatch, unhooking, and thread exit remain paired. Callbacks ignore injected input, recognize activity and app-switch intent, coalesce one typed notification path, and never run feature policy or a Windows mutation. Dropping the source posts `WM_QUIT` and joins the thread.

| API | Used for | Reference |
| --- | --- | --- |
| `SetWindowsHookExW` | Installs low-level keyboard and mouse hooks on their owning message-loop thread. | https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowshookexw |
| `UnhookWindowsHookEx` | Removes each installed hook exactly once before the source thread exits. | https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-unhookwindowshookex |
| `GetMessageW` | Runs the hook thread's message loop required for callback delivery. | https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getmessagew |
| `PostThreadMessageW` | Posts `WM_QUIT` to stop the hook thread during runtime shutdown. | https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postthreadmessagew |

## System Tray Lifecycle

The standalone Iced validation prototype uses `benchmark/gui-prototypes/src/iced_tray.rs`.
It installs [SetWindowSubclass](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-setwindowsubclass)
on Iced's window thread, forwards unhandled messages with `DefSubclassProc`, and removes
the subclass and notification icon on `WM_NCDESTROY`. Tray notifications wake an Iced
subscription, which restores visibility through Iced's window API without polling.
This prototype does not implement Explorer-restart recovery or change the production tray.

`src/backend/tray.rs` adds and removes Winderust's notification-area icon and temporarily subclasses the live Iced window to receive tray callbacks. `TrayIcon` owns both resources: failed icon installation and normal `Drop` restore the exact window procedure returned by `SetWindowLongPtrW`, while unhandled messages continue through `CallWindowProcW`. `src/ui/app.rs` latches a failed install for the current Hide to tray / Start minimized configuration, preventing the visible UI tick from retrying `Shell_NotifyIconW` every second; changing that configuration permits one new attempt and the original failure remains visible when Start minimized falls back to ordinary minimization.


| API | Used for | Reference |
| --- | --- | --- |
| `Shell_NotifyIconW` | Adds and removes the Winderust notification-area icon. | https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyiconw |
| `ShowWindow` | Hides the window with `SW_HIDE` and shows it with `SW_SHOW`, preserving its current size and maximized state instead of resetting it with `SW_RESTORE`. | https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow |
| `SetWindowLongPtrW` | Installs and restores the temporary `GWLP_WNDPROC` tray callback. A zero return is a failure only when `GetLastError` is nonzero after first clearing it with `SetLastError(0)`. | https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowlongptrw |
| `CallWindowProcW` | Forwards unhandled messages to the exact original window procedure. | https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-callwindowprocw |

## External Web Links

`src/backend/win_util.rs::open_url` opens About and update links through `ShellExecuteW`.
It accepts HTTPS URLs only, rejects credentials and control/whitespace characters, passes
no shell command or parameters, and reports return codes at or below 32 as failures.
`src/ui/app.rs` handles the result as a UI message. This is separate from administrator relaunch.

| API | Used for | Reference |
| --- | --- | --- |
| `ShellExecuteW` | Opens a validated HTTPS link with the registered Windows handler. | https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shellexecutew |

## Administrator Relaunch And Single-Instance Handoff

`src/main.rs` acquires the path-scoped single-instance mutex before its administrator check. A fresh
normal launch therefore requests elevation immediately through `src/backend/privilege.rs`, then
exits while the elevated replacement waits for its mutex ownership to end. `ShellExecuteExW` uses
the private elevated-relaunch argument, `SEE_MASK_NOASYNC` completes process creation before the
standard process exits, and `SEE_MASK_NOCLOSEPROCESS` confirms that Windows returned a live
replacement-process handle. If an instance already owns the mutex, an ordinary duplicate instead
signals the path-scoped auto-reset event that restores the existing Iced window through
`src/backend/tray.rs`; it does not open another UAC prompt or elevated waiter. The primary waits on
that event from a blocked listener thread, so the handoff adds no polling wake source.

| API | Used for | Reference |
| --- | --- | --- |
| `ShellExecuteExW` / `SHELLEXECUTEINFOW` | Starts the current executable with the `runas` verb and confirms creation of the elevated replacement before the caller exits. | https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shellexecuteexw / https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-shellexecuteinfow |
| `SEE_MASK_NOASYNC` | Keeps shell activation synchronous because the standard instance exits immediately after a successful launch. | https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-shellexecuteinfow |
| `SEE_MASK_NOCLOSEPROCESS` | Requests the replacement process handle used to distinguish accepted shell execution from an actual process launch. | https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-shellexecuteinfow |
| Named mutex / `WaitForSingleObject` | Keeps normal launches single-instance while allowing the explicit elevated replacement to wait for the closing instance's ownership to end. | https://learn.microsoft.com/en-us/windows/win32/sync/mutex-objects / https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject |
| `CreateEventW` / `OpenEventW` / `SetEvent` | Carries a duplicate normal launch to the existing process as a coalescing restore request without polling. | https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createeventw / https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-openeventw / https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-setevent |

## Winderust Self-Power

`src/backend/self_power.rs` owns Winderust's own process-lifetime priority and Power Throttling state. Before its first write, `SelfPowerController` strictly reads both original values; failure to capture either reversible baseline blocks the transition. Hidden-to-tray and Adaptive requests are composed into one desired state, applied as one verified transaction, and compensated if a later write or verification fails. These requests control only EcoQoS and hidden Idle priority; they preserve timer-resolution throttling bits exactly as observed. Clean disable and shutdown restore the exact captured baseline. This state does not use the external journal because process termination also terminates the controlled Winderust process.

`src/platform/windows/self_power.rs` owns the current-process pseudo-handle and every raw query/set
for this lifecycle. It converts the Windows structure to the shared platform-layer typed Power
Throttling value and initializes `PROCESS_POWER_THROTTLING_STATE.Version` to
`PROCESS_POWER_THROTTLING_CURRENT_VERSION` before every read; a zero-initialized query structure is
rejected on supported Windows versions.

| API | Used for | Reference |
| --- | --- | --- |
| `GetProcessInformation(ProcessPowerThrottling)` | Captures and verifies Winderust's process power-throttling state. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessinformation |
| `SetProcessInformation(ProcessPowerThrottling)` | Applies and compensates Winderust's composed EcoQoS state without claiming timer-resolution behavior. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessinformation |
| `GetPriorityClass` | Captures and verifies Winderust's original process priority. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getpriorityclass |
| `SetPriorityClass` | Applies hidden Idle priority and restores the captured priority. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setpriorityclass |

## Crash Recovery Watchdog

`src/backend/crash_recovery.rs` starts `winderust.exe` in a hidden watchdog
mode before runtime mutations are allowed. The parent sends a pending recovery
intent through the child's inherited standard-input pipe before each reversible
mutation and waits for the child's acknowledgement, then commits or cancels it
after the Win32 result. Pipe closure triggers replay after a crash or forced
termination; a clean shutdown first restores normal feature ownership, then
closes the pipe. The watchdog also invokes the existing narrowly identified
`Winderust Adaptive` stale-plan cleanup. No recovery journal is persisted to
disk.

`PowerPlanController` is the sole producer of automatic power-plan journal
transactions. It verifies the active GUID before committing, compensates
failed transitions, and leaves an acknowledged intent available when clean
compensation cannot prove restoration.

Recovery reopens a process only when its PID, creation time, and executable path
still identify the recorded instance. It restores a property only while the
live value matches the value Winderust expected, so later external changes stop
that recovery chain. A live controller that detects an external ownership break
uses an acknowledged exact-instance/property forget command before abandoning
its local baseline, preventing the watchdog from later replaying a relinquished
entry. Thread recovery additionally validates the thread creation
time and owning process. App Suspension is different: after the helper has
opened and retained the exact named Job Object at Begin, recovery thaws that
job even when the recorded root process has exited. Job descendants join by
default and can remain frozen after their root exits, so root liveness is not a
valid precondition for restoring a helper-owned job. If the watchdog cannot
accept an intent, Winderust blocks the corresponding mutation.

For App Suspension and CPU Limiter freeze intents, the watchdog opens and retains its own Job Object
handle before acknowledging the pending freeze. This keeps the object alive if
the main process is force-terminated. The handle requests only
`JOB_OBJECT_SET_ATTRIBUTES`, which is sufficient for the thaw operation and
object retention. The name includes Winderust's executable hash plus the
captured PID and creation time; the retained handle prevents name rebinding
before recovery.

| API | Used for | Reference |
| --- | --- | --- |
| `GetProcessId`, `GetProcessTimes`, and `QueryFullProcessImageNameW` | Bind recovery entries to a process instance and prevent PID-reuse restoration. | [ID](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessid) / [Times](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes) / [Path](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-queryfullprocessimagenamew) |
| `GetThreadTimes` and `GetProcessIdOfThread` | Revalidate a recorded thread instance and owner before restoring thread priority. | [Times](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreadtimes) / [Owner](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessidofthread) |
| `OpenJobObjectW` | Reopens a named App Suspension or CPU Limiter freeze job so the watchdog can retain and thaw it. | https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-openjobobjectw |

## Background Efficiency / EcoQoS

Winderust Background Efficiency applies Windows EcoQoS and idle process
priority to selected background processes. Process Priority, Background
Efficiency, CPU Scheduler, and Process List actions share one
`PriorityEfficiencyController`, so overlapping requests use one baseline and a
deterministic effective owner instead of competing writers.

Implementation paths:

- `src/features/winderust_features/background_efficiency.rs`: Background
  Efficiency target policy, protections, exclusions, suppression, status, and
  Action Log attribution.
- `src/features/winderust_features/cpu_scheduler.rs`: Adaptive Engine and CPU
  Scheduler Process Priority and Power Throttling claims.
- `src/features/winderust_features/cpu_scheduler/process_control.rs`: read-only
  workload process sampling and identity helpers.
- `src/features/priority_control/process_priority.rs`: static Process Priority
  policy.
- `src/control/priority_efficiency.rs`: sole live process Priority Class and
  Power Throttling baseline, arbitration, mutation, verification, compensation,
  recovery-journal relinquishment, and clean-release authority.
- `src/platform/windows/priority_efficiency.rs`: sole live priority-class and process Power
  Throttling query/set adapter, Windows constant mapping, state conversion, and raw error
  classification.
- `src/backend/crash_recovery.rs`: independent crash-replay mirror.

User-facing behavior:

- Winderust finds eligible processes under the configured cross-session policy
  and preserves all protected-process and access checks. CPU Scheduler protects
  Focus processes and considers hot Visible Window and Background processes;
  Background Efficiency retains its own foreground/visible protection policy.
- It skips Winderust itself, built-in Windows shell/input/system processes,
  protected processes, inaccessible processes, exclusions, and any process
  protected by the active feature's tier policy.
- The runtime worker reopens and revalidates PID, creation time, and exact
  executable path immediately before a read or write.
- It reads the process's existing Power Throttling and Priority Class values
  before the first Winderust mutation and retains one baseline per property and
  exact process instance.
- It enables EcoQoS by setting `PROCESS_POWER_THROTTLING_EXECUTION_SPEED`
  through `SetProcessInformation` and sets `IDLE_PRIORITY_CLASS` as one compound
  Efficiency Mode transaction.
- Adaptive Engine Power Throttling claims do not set
  `PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION`; that behavior remains
  owned only by the separate Background Efficiency feature.
- The Process List context-menu action uses the same Task Manager-style
  invariant. Efficiency Mode reports enabled only when both EcoQoS and Idle
  process priority are observed.
- Priority precedence is Background Efficiency > CPU Scheduler Focus Process Priority >
  Adaptive Engine > static Process Priority. Power Throttling precedence is
  Background Efficiency > Adaptive Engine. Removing a higher claim reveals a
  lower claim without restoring through the original baseline.
- If `GetProcessInformation(ProcessPowerThrottling)` cannot capture a reversible
  baseline for a CPU Scheduler target, its independent Process Priority claim
  may still proceed. The unavailable Power Throttling control is remembered by
  exact process instance so each reconciliation does not retry and log the same
  failure.
- It restores only while the live value still equals Winderust's expected
  value. An external change relinquishes ownership and the matching crash
  journal entry without overwriting the external state.
- It restores managed state when claims disappear, automation is disabled, or
  Winderust exits, in reverse successful-application order.

### EcoQoS APIs

| API | Used for | Reference |
| --- | --- | --- |
| `SetProcessInformation` | `src/platform/windows/priority_efficiency.rs` applies `ProcessPowerThrottling` with `PROCESS_POWER_THROTTLING_STATE` to enable or clear EcoQoS. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessinformation |
| `GetProcessInformation` | The same adapter reads the current `PROCESS_POWER_THROTTLING_STATE` before Winderust changes it, so the controller can restore the exact state later. It initializes `Version = PROCESS_POWER_THROTTLING_CURRENT_VERSION` before every query; zero initialization is invalid for this information class. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessinformation |
| `PROCESS_INFORMATION_CLASS` | Defines `ProcessPowerThrottling`, the information class used for process power throttling. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/ne-processthreadsapi-process_information_class |
| `PROCESS_POWER_THROTTLING_STATE` | Holds the throttling version, control mask, and state mask used for EcoQoS. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/ns-processthreadsapi-process_power_throttling_state |
| Quality of Service | Explains Windows QoS levels and that `SetProcessInformation` can explicitly tag a process as EcoQoS by toggling `PROCESS_POWER_THROTTLING_EXECUTION_SPEED`. | https://learn.microsoft.com/en-us/windows/win32/procthread/quality-of-service |

Important behavior from Microsoft: enabling `PROCESS_POWER_THROTTLING_EXECUTION_SPEED` classifies the process as EcoQoS. Windows then tries to improve power efficiency through strategies such as lower CPU frequency or more efficient CPU cores. EcoQoS should be used for work that is not part of the foreground user experience.

### Priority APIs

| API | Used for | Reference |
| --- | --- | --- |
| `GetPriorityClass` | `src/platform/windows/priority_efficiency.rs` reads the existing priority class before the controller changes it. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getpriorityclass |
| `SetPriorityClass` | The same adapter applies the controller's effective Process Priority owner, including Idle priority as the priority half of Efficiency Mode, then restores the captured baseline. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setpriorityclass |
| Scheduling Priorities | Documents process priority classes such as idle and normal. | https://learn.microsoft.com/en-us/windows/win32/procthread/scheduling-priorities |

### Process Access APIs

| API | Used for | Reference |
| --- | --- | --- |
| `OpenProcess` | `src/platform/windows/process.rs` maps each typed control operation to its minimal access profile and owns raw acquisition for process-control commands. `src/control/process.rs` then validates PID, creation time, absolute executable path, session policy, process name, critical/protected state, and operation-specific capability on that same retained handle. Other read-only discovery and recovery paths keep their independently justified openers. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocess |
| `GetProcessTimes` | Captures creation time in the lightweight shared process snapshot using its existing query handle. Typed controls pair that value with the exact executable path, then revalidate both immediately before mutation so PID reuse fails closed without requiring a full path-enrichment pass. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes |
| `QueryFullProcessImageNameW` | Reads executable paths through `src/foreground/process_list.rs` for process discovery, foreground/cursor identification, and exact executable-path matching across every user-configured process rule. Process mutation paths revalidate the expected executable path on the same opened handle immediately before the change; built-in Windows safety exclusions remain filename-based. | https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-queryfullprocessimagenamew |
| Process handles and identifiers | Documents that a process ID is valid only until process termination and can be reused, while an open handle continues to identify that process object. Winderust pairs executable-path checks with creation-time checks wherever sampled state crosses into a later mutation. | https://learn.microsoft.com/en-us/windows/win32/procthread/process-handles-and-identifiers |
| Process Security and Access Rights | Documents the query-only `PROCESS_QUERY_LIMITED_INFORMATION` right and the `PROCESS_SET_INFORMATION` right required by `SetProcessInformation`. | https://learn.microsoft.com/en-us/windows/win32/procthread/process-security-and-access-rights |

Winderust does not enable debug privilege. Elevated process access continues to follow the target process's Windows access checks alongside Winderust's critical-process, built-in, identity, minimal-access, and protected-process barriers.

## Advanced Power Plan Tuning

Winderust can apply separate AC and battery processor-power percentages and processor boost modes to a selected Windows power plan, with presets available as quick-fill values. This is system-wide power-plan tuning, not per-process CPU allocation.

Implementation paths:

- `src/ui/advanced_power_plan_tuning.rs`: plan selection,
  presets, separate A/C and battery controls, and apply/reset UI.
- `src/application/advanced_power_plan_tuning.rs`: typed persistent read,
  staged apply, and mandatory post-attempt readback service.
- `src/power/powercfg.rs`: typed normalization, ordered stages, active-plan refresh decision, and
  managed/persistent domain semantics.
- `src/platform/windows/power_plan.rs`: raw processor-setting reads/writes, scheme activation, GUID
  conversion, and native power-policy boundary.

The explicit tuning service applies ten ordered A/C and battery settings and
retains the exact failing stage. It re-reads the selected plan after every
attempt, including a partial failure, so the UI reflects the values Windows now
holds rather than the pre-apply draft. This persistent user command does not use
the automatic power-plan recovery journal or claim ledger.

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
- `src/control/priority_efficiency.rs`: sole Process Priority and process Power
  Throttling baseline, owner arbitration, compound Efficiency Mode transition,
  mutation, verification, compensation, journal relinquishment, and
  clean-release authority.
- `src/platform/windows/priority_efficiency.rs`: sole live priority-class and process Power
  Throttling constant conversion, query/set, and Win32 failure-classification adapter; crash
  recovery and Winderust self-power retain their separate allowlisted contracts.
- `src/features/priority_control/thread_priority.rs`
- `src/features/priority_control/dynamic_priority_boost.rs`: Dynamic Priority Boost policy, tiering, rules, suppression, status, and Action Log.
- `src/control/process.rs`: shared exact process identity and mechanism-specific access safety boundary.
- `src/control/thread_priority.rs`: sole Thread Priority process/thread identity,
  enumeration, baseline, owner, mutation, verification, compensation, journal
  relinquishment, and clean-release authority.
- `src/platform/windows/thread_priority.rs`: sole live Toolhelp enumeration, thread open/owner/time
  query, priority constant mapping, and priority query/set adapter; crash recovery retains its
  separate replay-only thread path.
- `src/control/dynamic_priority_boost.rs`: sole Dynamic Priority Boost baseline, owner, transaction, verification, compensation, and clean-release authority.
- `src/platform/windows/dynamic_priority_boost.rs`: sole live Dynamic Priority Boost query/set adapter; crash recovery retains its independent replay-only mirror.
- `src/features/priority_control/io_priority.rs`: I/O Priority policy, tiering, rules, preservation, suppression, status, and Action Log.
- `src/control/io_priority.rs`: sole I/O Priority process identity, raw baseline, owner, transaction, verification, compensation, journal relinquishment, and clean-release authority.
- `src/platform/windows/io_priority.rs`: sole live NT declaration, numeric information class, query/set, and NTSTATUS-classification adapter; crash recovery retains its independent replay-only mirror.
- `src/features/priority_control/gpu_priority.rs`: GPU Priority policy, tiering, rules, preservation, suppression, pending-context handling, status, and Action Log.
- `src/control/gpu_priority.rs`: sole GPU Priority process identity, raw baseline, owner, transaction, verification, compensation, journal relinquishment, and clean-release authority.
- `src/platform/windows/gpu_priority.rs`: sole live D3DKMT query/set and NTSTATUS-classification adapter; crash recovery retains its independent replay-only mirror.
- `src/features/priority_control/memory_priority.rs`: static and CPU Scheduler Memory Priority target policy, tiering, rules, preservation, suppression, status, and Action Log attribution.
- `src/control/memory_priority.rs`: sole Memory Priority process identity, simultaneous-owner arbitration, raw baseline, transaction, verification, compensation, journal relinquishment, and clean-release authority.
- `src/platform/windows/memory_priority.rs`: sole live Memory Priority raw-class conversion and query/set adapter; crash recovery retains its independent replay-only mirror.

| Product feature / API | Used for | Reference |
| --- | --- | --- |
| Process Priority: `GetPriorityClass` / `SetPriorityClass` | `src/platform/windows/priority_efficiency.rs` is the sole live query/set adapter. The typed controller owns exact-process baselines, owner arbitration, Begin/apply/verify/Commit, compensation, relinquishment, and clean release. Static Process Priority, Background Efficiency, CPU Scheduler, Focus Process Priority, and Process List commands share its deterministic owner chain; crash recovery retains a separate replay-only setter. | [Get](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getpriorityclass) / [Set](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setpriorityclass) |
| Process Power Throttling: `GetProcessInformation` / `SetProcessInformation` | `src/platform/windows/priority_efficiency.rs` is the sole live `ProcessPowerThrottling` adapter and initializes the required current-version field before reads. The typed controller owns the full raw baseline, compound Efficiency Mode transaction, verification, compensation, and restoration chain; crash recovery retains a separate replay-only setter. | [Get](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessinformation) / [Set](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessinformation) |
| Thread Priority: `CreateToolhelp32Snapshot`, `Thread32First`, and `Thread32Next` | `src/platform/windows/thread_priority.rs` enumerates the current threads of an exact process claim on each applicable reconciliation. The controller discovers new threads and relinquishes missing exact identities without targeting replacements. | [Snapshot](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-createtoolhelp32snapshot) / [First](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-thread32first) / [Next](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-thread32next) |
| Thread Priority: `GetThreadPriority` / `SetThreadPriority` | The same adapter is the sole live query/set boundary. The typed controller reads, applies, verifies, compensates, and restores per-thread state; static policy, Adaptive replacement policy, and Process List commands share exact baseline/expected chains. Crash recovery retains a separate replay-only setter. | [Get](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreadpriority) / [Set](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setthreadpriority) |
| Thread Priority: `GetThreadTimes` | The adapter reads creation time; the controller binds ownership and recovery to it so a recycled thread ID cannot receive or restore another thread's state. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreadtimes |
| Thread Priority: `GetProcessIdOfThread` | The adapter reads the owning PID immediately before a controller query, journal transaction, or write; the controller rejects any mismatch with the verified process identity. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessidofthread |
| Dynamic Priority Boost: `GetProcessPriorityBoost` / `SetProcessPriorityBoost` | `src/platform/windows/dynamic_priority_boost.rs` is the sole live query/set adapter. The typed controller owns Begin, apply, verify, Commit, compensation, external-break relinquishment, and clean release. Static policy, Adaptive replacement policy, and Process List commands share its exact-identity baseline/expected chain; crash recovery retains a separate replay-only setter. | [Get](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesspriorityboost) / [Set](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocesspriorityboost) |
| Memory Priority: `GetProcessInformation` / `SetProcessInformation` | `src/platform/windows/memory_priority.rs` is the sole live raw-class query/set adapter. The typed controller preserves unknown raw values and owns Begin/apply/verify/Commit, compensation, arbitration, relinquishment, and clean release. Static Memory Priority, lower-precedence CPU Scheduler claims, and Process List commands share one exact-process raw baseline/expected chain; crash recovery retains a separate replay-only setter. | [Get](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessinformation) / [Set](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessinformation) |
| `MEMORY_PRIORITY_INFORMATION` | Defines the memory-priority value passed to the process information APIs. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/ns-processthreadsapi-memory_priority_information |
| I/O Priority: `NtQueryInformationProcess` / `NtSetInformationProcess` | `src/platform/windows/io_priority.rs` is the sole live declaration/query/set adapter for numeric process information class 33. The typed controller preserves unknown raw values and owns Begin/apply/verify/Commit, preservation, compensation, relinquishment, and clean release. Static policy, Adaptive replacement policy, and Process List commands share one exact-process baseline/expected chain; crash recovery retains a separate replay-only setter. | https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntqueryinformationprocess |
| GPU Priority: `D3DKMTGetProcessSchedulingPriorityClass` / `D3DKMTSetProcessSchedulingPriorityClass` | `src/platform/windows/gpu_priority.rs` is the sole live query/set adapter and keeps the observed `STATUS_INVALID_PARAMETER`-as-temporary-context interpretation local. The typed controller owns Begin/apply/verify/Commit, preservation, compensation, relinquishment, and clean release. Static policy, Adaptive replacement policy, and Process List commands share one exact-process baseline/expected chain; crash recovery retains a separate replay-only setter. | [Get](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/d3dkmthk/nf-d3dkmthk-d3dkmtgetprocessschedulingpriorityclass) / [Set](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/d3dkmthk/nf-d3dkmthk-d3dkmtsetprocessschedulingpriorityclass) |
| `D3DKMT_SCHEDULINGPRIORITYCLASS` | Defines the GPU scheduling priority values. | https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/d3dkmthk/ne-d3dkmthk-_d3dkmt_schedulingpriorityclass |

Compatibility note: Microsoft documents `NtQueryInformationProcess` as an
internal interface that can change and recommends public alternatives where
available. The numeric I/O priority class and `NtSetInformationProcess` use in
Winderust are not documented as stable public SDK contracts. Keep their
declarations and class constant beside the typed I/O Priority adapter, preserve
failure handling, and revalidate them against supported Windows versions when
that boundary changes.

Microsoft defines the six `D3DKMT_SCHEDULINGPRIORITYCLASS` values in increasing
order from Idle through Realtime. The Get and Set documentation does not define
`STATUS_INVALID_PARAMETER` as a formal "no GPU context" signal. Winderust's
typed GPU adapter treats that status as a temporary unavailable/pending
condition based on observed Windows behavior and the previous product contract;
keep this inference isolated in the adapter and revalidate it when the WDK
boundary or supported Windows versions change.

## Memory Trim

Implementation entry points:

- `src/features/winderust_features/memory_trim.rs`
- `src/control/memory_trim.rs`
- `src/platform/windows/memory_trim.rs`

| API | Used for | Reference |
| --- | --- | --- |
| `GlobalMemoryStatusEx` | Reads overall physical-memory load and availability. | https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-globalmemorystatusex |
| `K32GetProcessMemoryInfo` | The typed command boundary reads the working set before a trim and, when available, after it; a failed post-trim query leaves the freed-memory estimate unknown rather than treating the whole prior working set as freed. | https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getprocessmemoryinfo |
| `SetProcessWorkingSetSize` | `src/platform/windows/memory_trim.rs` alone passes `SIZE_T(-1)` for both bounds to remove as many pages as possible. `MemoryTrimController` supplies a handle with exact creation/path identity, critical/PPL, current-session policy, and `PROCESS_SET_QUOTA` validation immediately before the irreversible call. | https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-setprocessworkingsetsize |


## CPU Sets (Soft) and Processor Affinity (Hard)

Winderust exposes two separate per-app rule features. CPU Sets (Soft) applies preferred Windows CPU Sets and is the recommended default. Processor Affinity (Hard) applies a strict process affinity mask and warns that, on systems with more than one processor group, the mask covers only the process primary group. The current rule mask covers processor group 0 only, so CPU Sets (Soft) discloses that limit when multiple groups are present. All automatic CPU allocation shares one coordinator with this order: CPU Sets (Soft) > Processor Affinity (Hard) > Adaptive Engine / CPU Scheduler. CPU Sets and affinity cannot remain simultaneously Winderust-owned for one exact process instance. CPU Scheduler may select the least-used logical processors across the All, P-core, or E-core pool from per-processor samples, a fixed P/E/no-SMT topology mask, or an exact custom mask; these policy choices do not create another mutation owner.

Background Efficiency exposes Foreground Detection and Visible Window Detection. CPU Limiter,
CPU Sets (Soft), and Processor Affinity (Hard) classify each matched process as Focus, Visible
Window, or Background and select that tier's policy. Foreground
resolution starts from the active window; visible-window detection keeps top-level windows that are
visible, not minimized, and not DWM-cloaked. Both classifications include sibling processes with
the same executable path. A fully covered window still qualifies because `IsWindowVisible` reports
window style state rather than pixel occlusion. CPU allocation skips a foreground or visible-window
observation when the adjacent tier masks are identical and the observation cannot change the
selected mask.

Implementation paths:

- `src/control/cpu_allocation.rs`: sole live affinity and CPU Set ownership coordinator,
  exact-identity validation, claim arbitration, baseline/expected state, mutual exclusion,
  recovery sequencing, compensation, and clean release.
- `src/platform/windows/cpu_allocation.rs`: sole live affinity/CPU Set query and setter adapter,
  including packed `GetSystemCpuSetInformation` topology-buffer conversion.
- `src/features/cpu_control/cpu_allocation.rs`: explicit-rule discovery,
  Focus/Visible Window/Background tier selection, topology policy, failure suppression,
  status, and Action Log reporting.
- `src/features/winderust_features/cpu_scheduler.rs`: pressure, candidate,
  topology, saturation, and rebalance policy only.
- `src/backend/crash_recovery.rs`: independent crash-recovery mirror for
  affinity and CPU Set values.

Every claim is bound to PID, creation time, and normalized absolute executable
path, then reopened through the shared process-control safety boundary before a
write. A higher-owner release queues the exact process key instead of executing
another producer's stored claim. After all CPU producers have processed that
worker pass, `RuntimeCore` asks the coordinator to re-resolve the effective
claim and reopen it with the current cross-session setting. If no claim remains,
the coordinator restores the first baseline only while the live state still
matches Winderust's expected value. External changes break ownership and are
left untouched after the recovery entry is relinquished. Shutdown skips the
pass-end handoff and directly restores coordinator-owned state in reverse
application order. Failed release-only cleanup retries with bounded one-to-60
second backoff; repeated identical attempts do not create repeated Action Log
failures, and a newly queued handoff bypasses an older retry deadline.

`GetProcessDefaultCpuSets` buffer discovery accepts
`ERROR_INSUFFICIENT_BUFFER` as the only larger-buffer signal. A successful
zero-count probe represents a genuinely empty CPU Set list. Both the live
Windows adapter and crash-recovery mirror follow this contract.

### CPU Allocation APIs

| API | Used for | Reference |
| --- | --- | --- |
| `GetProcessAffinityMask` | `src/platform/windows/cpu_allocation.rs` reads the current process and system affinity masks before the coordinator changes them. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessaffinitymask |
| `SetProcessAffinityMask` | The same adapter is the sole live writer for the coordinator's configured hard affinity mask; crash recovery retains a separate replay-only writer. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessaffinitymask |
| `GetSystemCpuSetInformation` | The adapter converts the packed variable-length topology buffer into group-zero logical-processor-to-CPU-Set IDs for soft mode. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getsystemcpusetinformation |
| `GetProcessDefaultCpuSets` | The adapter reads existing process default CPU Set IDs so the coordinator can capture and restore the exact baseline. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessdefaultcpusets |
| `SetProcessDefaultCpuSets` | The adapter is the sole live writer that applies or clears process default CPU Set IDs; crash recovery retains a separate replay-only writer. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessdefaultcpusets |
| `GetActiveProcessorGroupCount` | Detects multi-group systems where single-mask affinity APIs are group-relative. | https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getactiveprocessorgroupcount |
| Processor Groups | Explains why hard affinity masks are group-relative and why multi-group systems need special handling. | https://learn.microsoft.com/en-us/windows/win32/procthread/processor-groups |
| CPU Sets | Explains soft processor preference while remaining more compatible with OS power management. | https://learn.microsoft.com/en-us/windows/win32/procthread/cpu-sets |
| `GetForegroundWindow` / `GetWindowThreadProcessId` | Resolves the current active top-level window for Focus-tier selection and foreground-protection features. | [GetForegroundWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getforegroundwindow) / [GetWindowThreadProcessId](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowthreadprocessid) |
| `EnumWindows` / `IsWindowVisible` / `IsIconic` | Enumerates top-level windows and filters hidden or minimized windows for Visible Window tiers and visible-window protection. | [EnumWindows](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumwindows) / [IsWindowVisible](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-iswindowvisible) / [IsIconic](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-isiconic) |
| `DwmGetWindowAttribute(DWMWA_CLOAKED)` | Excludes windows hidden by DWM, including windows not shown on the current virtual desktop. | [DwmGetWindowAttribute](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmgetwindowattribute) / [DWMWINDOWATTRIBUTE](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute) |

## CPU Limiter Duty Cycling

CPU Limiter page defaults target 1% to 100% Allowed CPU Time for Focus, Visible Window, and
Background app groups within a fixed 100 ms cycle. Each rule tier can follow that page default,
remain Unlimited, or apply its own 1% to 100% target. A 100% target is Unlimited and creates no
limiter schedule.
This is wall-clock duty cycling, not processor affinity and not Windows Job Object
CPU-rate control. During the awake phase the app may use any processors Windows schedules for it;
during the frozen phase its selected backend prevents execution. Future child processes normally
join the primary Job Object backend. At very low values, timer and freeze/thaw transition latency
can raise observed CPU use above the target, so those settings are approximate.

Before assigning a matched child its own limiter job, the shared suspension controller revalidates
the child and uses `IsProcessInJob` against each active Winderust ancestor job identified by the
verified process snapshot. A child already covered by that job keeps the ancestor's duty cycle and
does not receive a nested limiter job. A pre-existing or breakaway child that is not a member still
receives its own exact job.

Job Object acquisition remains primary. Only `SuspensionError::NotSupported`, which identifies an
incompatible existing Job Object, selects the private fallback in
`src/control/cpu_limiter/thread_fallback.rs`; access-denied, exited, protected, reused, and
unverifiable targets remain failures. App Suspension remains Job-only. The fallback captures exact
thread IDs, creation times, and baseline suspend counts through
`src/platform/windows/thread_suspension.rs`, owns exactly one suspend-count increment per thread,
and refuses conflicting counts. Each frozen worker batch freezes known threads, takes one Toolhelp
inventory for all due fallback processes, and Process-Snapshots only processes with unknown thread
IDs before adopting them.

The external watchdog records each exact thread before suspension and calls `ResumeThread` once
only when process identity, thread identity, and the current count all match the recorded owned
increment. Ordinary awake phases retain that recovery entry; final release thaws before forgetting
it. CPU Limiter target refresh and process appearance discovery stay at a one-second maximum while
a valid limiter rule is active, including hidden and Adaptive Engine modes.

`src/control/cpu_limiter.rs` owns one worker and every target schedule.
`src/platform/windows/cpu_limiter.rs` owns the high-resolution waitable timer, command event, and
multi-object wait. `src/control/suspension.rs` owns exact-process Job Object acquisition and combines
CPU Limiter and App Suspension owner phases into one effective frozen state. Recovery stays armed
across limiter duty cycles and is forgotten only after the last owner releases the job.

| API or behavior | Used for | Reference |
| --- | --- | --- |
| `CreateWaitableTimerExW` | Creates the worker's high-resolution waitable timer with `CREATE_WAITABLE_TIMER_HIGH_RESOLUTION`. | https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createwaitabletimerexw |
| `SetWaitableTimer` | Arms one relative deadline for the next target phase transition. | https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-setwaitabletimer |
| `WaitForMultipleObjects` | Waits for either the next timer deadline or a target/shutdown command. | https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitformultipleobjects |
| `SetInformationJobObject` | Applies the compatibility-sensitive Job Object freeze/thaw operation shared with App Suspension. | https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-setinformationjobobject |
| `IsProcessInJob` | Confirms whether a matched child is already covered by an active Winderust ancestor job before creating another limiter job. | https://learn.microsoft.com/en-us/windows/win32/api/jobapi/nf-jobapi-isprocessinjob |
| Job Objects | Documents default child membership and Job Object lifetime. | https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects |
| Nested Jobs | Documents that assigning a process already in a job can create a nested hierarchy, which CPU Limiter avoids for an already-covered child. | https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs |
| `PssCaptureSnapshot`, `PssWalkSnapshot`, and `PSS_THREAD_ENTRY` | Captures exact thread identity and baseline suspend counts when preparing or extending a fallback target. | [Capture](https://learn.microsoft.com/en-us/windows/win32/api/processsnapshot/nf-processsnapshot-psscapturesnapshot) / [Walk](https://learn.microsoft.com/en-us/windows/win32/api/processsnapshot/nf-processsnapshot-psswalksnapshot) / [Entry](https://learn.microsoft.com/en-us/windows/win32/api/processsnapshot/ns-processsnapshot-pss_thread_entry) |
| `SuspendThread` / `ResumeThread` | Adds and removes exactly one CPU Limiter-owned suspend-count increment after recovery is armed. | [Suspend](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-suspendthread) / [Resume](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-resumethread) |

## App Suspension

Winderust App Suspension is manual Win32 Job Object freezing. It is not the same as Windows-managed UWP app suspension shown by Task Manager for some Store apps.

Implementation paths:

- `src/features/advanced_controls/app_suspension.rs`: rule, grace, wake, suppression, snapshot, and Action Log policy.
- `src/control/suspension.rs`: shared exact-process assignment, owner-phase arbitration, Job Object freeze/thaw transaction, retry, and clean-release boundary.
- `src/platform/windows/job.rs`: shared named Job Object creation, assignment, and membership primitives.
- `src/platform/windows/suspension.rs`: sole normal freeze/thaw writer and freeze-information layout boundary.
- `src/features/advanced_controls/app_suspension/wake_activity.rs`: audio and IP Helper wake detection.
- `src/backend/crash_recovery.rs`: independent helper-held recovery mirror.

User-facing behavior:

- Winderust finds selected background apps from `Suspendable Apps`.
- After the configured background delay, Winderust opens and revalidates PID, creation time, executable path, session, service account, critical/PPL state, and job-assignment rights on the same target handle before assigning an instance-bound named Windows Job Object.
- It freezes that job with `SetInformationJobObject` and thaws the same job when the focused or clicked app needs to recover, when the process is removed from the list, App Suspension is disabled, automation is disabled, Winderust exits, or the recovery watchdog observes abnormal termination.
- Automatic rules, App-page Freeze, and Process List Suspend/Resume all use typed RuntimeCore commands and the same controller. New acquisition honors the current cross-session setting; restoration of an already-owned exact job does not reapply that acquisition gate.
- Windows keeps a Job Object alive until both its last handle is closed and all associated processes exit. A released live process therefore remains in its exact named, thawed job. A later Suspend may reuse that existing name only after `IsProcessInJob` proves the exact target is already a member; an unrelated name collision fails closed.
- A failed thaw retains the frozen job for bounded retry. If thaw succeeds but recovery-journal cleanup fails, status changes to thawed while a cleanup-only record remains; later App Suspension passes reconcile both pending states without another user action. The controller does not falsely report the process as frozen or reapply the OS mutation during cleanup retry, and the feature prunes its exact record once compensation removes controller ownership.
- Taskbar and tray shell clicks temporarily thaw suspended top-level window owner processes only, so minimized and tray-hidden apps can restore without thawing unrelated non-window worker processes. Repeated shell clicks do not keep extending the thaw window.

### Job Object Freeze APIs

| API | Used for | Reference |
| --- | --- | --- |
| `OpenProcess` | Opens the exact target with query, `PROCESS_SET_QUOTA`, and `PROCESS_TERMINATE`; Winderust first attempts to include `SYNCHRONIZE` but does not require it because the owned Job Object, not root-process liveness, governs restoration. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocess |
| `CreateJobObjectW` | `src/platform/windows/job.rs` creates the instance-bound named job, or reopens its still-live exact name after a prior thaw/release; each controller permits existing-name reuse only after exact membership validation. | https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-createjobobjectw |
| `AssignProcessToJobObject` | The adapter assigns the controller-validated exact target process to Winderust's private job object. | https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject |
| `IsProcessInJob` | The adapter checks exact membership and whether an assignment failure reflects a process already constrained by another job; controller policy fails closed. | https://learn.microsoft.com/en-us/windows/win32/api/jobapi/nf-jobapi-isprocessinjob |
| `SetInformationJobObject` | The adapter is the sole normal freeze/thaw writer for the controller; crash recovery retains an independent replay path using the same shared layout contract. | https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-setinformationjobobject |
| Job Objects | Documents that children normally join their parent's job and that a job persists until its last handle closes and all associated processes terminate. This is why recovery thaws the helper-held job even after the recorded root exits. | https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects |

Compatibility note: `SetInformationJobObject` is public, but Microsoft does
not document information class 18 or Winderust's
`JobObjectFreezeInformation` layout as a public SDK contract. Keep the class
constant, structure, freeze/thaw call sites, and layout test aligned between
`platform/windows/suspension.rs` and `backend/crash_recovery.rs`; revalidate
this boundary against supported Windows versions when it changes.

Winderust keeps App Suspension opt-in and limited to explicitly selected apps because freezing a process is disruptive by design. Built-in exclusions also block Windows shell/input/UWP lifecycle processes such as `SearchApp.exe`, `SearchHost.exe`, and `SystemSettings.exe`, even if they are added to Suspendable Apps.

### Safety And Filtering APIs

| API | Used for | Reference |
| --- | --- | --- |
| `GetCurrentProcessId` | Gets Winderust's own process ID so Winderust never suspends itself. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getcurrentprocessid |
| `ProcessIdToSessionId` | Captures the session suffix/fallback shown in the Process List User column, enforces same-session targeting when Allow cross-session process control is disabled, and keeps Session 0 outside App Suspension. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-processidtosessionid |
| `IsProcessCritical` | Marks Windows-critical processes as `Protected system process`, excludes them from rule candidates and automation targets, and makes unverifiable targets fail closed. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-isprocesscritical |
| `OpenProcess(PROCESS_SET_INFORMATION)` | Performs a non-mutating access-right probe during process discovery. Processes that expose query metadata but deny this mutation right, including protected security services, are classified as inaccessible and excluded from process-control rules. | https://learn.microsoft.com/en-us/windows/win32/procthread/process-security-and-access-rights |
| `GetProcessInformation(ProcessProtectionLevelInfo)` | Reads `PROCESS_PROTECTION_LEVEL_INFORMATION` during process discovery. Any level other than `PROTECTION_LEVEL_NONE` identifies a Protected Process Light target and makes it inaccessible to Winderust controls. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/ns-processthreadsapi-process_protection_level_information |
| `NtQueryObject(ObjectBasicInformation)` | Reads `PUBLIC_OBJECT_BASIC_INFORMATION.GrantedAccess` from Winderust's own opened process handle so kernel security callbacks that silently strip requested rights are detected. Discovery probes `PROCESS_SET_INFORMATION`; action validation probes the operation-specific mask (`PROCESS_TERMINATE`, `PROCESS_SET_INFORMATION`, or job-assignment rights) while retaining critical/PPL checks. This API is documented by Microsoft but explicitly compatibility-sensitive; failure is treated as inaccessible. | https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntqueryobject |
| `OpenProcessToken` | Opens each queryable process token once with `TOKEN_QUERY`; the same captured SID populates the Process List User column and App Suspension picker eligibility. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocesstoken |
| `GetTokenInformation(TokenUser)` | Reads the SID of the account associated with the process token. | https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-gettokeninformation |
| `IsWellKnownSid` | Makes App Suspension automation, Process List actions, and process-picker eligibility fail closed for LocalSystem, LocalService, and NetworkService process tokens while leaving unrelated process controls available. | https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-iswellknownsid |
| `LookupAccountSidW` | Resolves the process token SID to its local account name; inaccessible or unmapped tokens use the neutral `Unavailable · S#` UI fallback rather than guessing an account. | https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-lookupaccountsidw |
| `CloseHandle` | Closes process and job handles after use. | https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-closehandle |
| `SetLastError` / `GetLastError` | Clears the thread error slot immediately before `CreateJobObjectW`, then captures it immediately afterward so `ERROR_ALREADY_EXISTS` cannot be confused with a stale success-path value; other failures likewise capture their error before another Win32 call. | [Set](https://learn.microsoft.com/en-us/windows/win32/api/errhandlingapi/nf-errhandlingapi-setlasterror) / [Get](https://learn.microsoft.com/en-us/windows/win32/api/errhandlingapi/nf-errhandlingapi-getlasterror) |

Implementation paths: process account/session capture and one-time target validation are in
`src/foreground/process_list.rs`; rule-driven filtering is applied by the
individual process-control managers under `src/features/`. Winderust Behaviour
owns `general.allow_cross_session_process_control`, which defaults on. Enabling
it removes only the session-equality filter: self-process protection, built-in
exclusions, target identity revalidation, minimal access masks, and Windows
process DACL/protected-process enforcement remain active.

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

- `src/features/advanced_controls/timer_resolution.rs`: foreground rule, snapshot, and Action Log policy.
- `src/control/timer_resolution.rs`: sole request lifecycle, switch, and clean-release owner.
- `src/platform/windows/timer_resolution.rs`: sole WinMM capability query, begin, and end adapter.

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

- `src/application/win32_priority_separation.rs`
- `src/ui/win32_priority_separation.rs`
- `src/backend/win_registry.rs`

Winderust reads and writes the machine-wide `Win32PrioritySeparation` DWORD
under the Windows PriorityControl key and stores the original value as a
Winderust-owned per-user backup before the first change. The typed application
service owns current/backup reads, never overwrites an existing automatic
backup, blocks the machine write when backup creation fails, and reports the
exact read/backup/machine-write stage. The UI owns only edit values, localized
messages, and rendering. Missing registry values remain distinct from access or
type errors.

| API / Contract | Used for | Reference |
| --- | --- | --- |
| Windows Registry functions | Defines registry key/value access, access rights, and Win32 error handling. The Rust `winreg` wrapper is isolated in `src/backend/win_registry.rs`; persistent transaction ordering is in `src/application/win32_priority_separation.rs`. | https://learn.microsoft.com/en-us/windows/win32/sysinfo/registry-functions |
| `Win32PrioritySeparation` value and bit layout | Decodes quantum duration, quantum behavior, and foreground boost for the Advanced page. | No stable public Microsoft API reference; project contract is in `src/ui/win32_priority_separation.rs` and its tests. |

Treat the value layout as compatibility-sensitive. Keep reading, backup,
writing, bit decoding, and tests aligned, and fail visibly if the machine value
cannot be read or written.

## Process List Resource Usage

Implementation entry points: `src/foreground/process_list.rs` for read-side
tree capture and `src/control/process_termination.rs` for the sole mutation
adapter. Sampling runs on
the existing Process List background refresh and treats inaccessible or exited
processes as unavailable rather than failing the complete refresh.

| API | Used for | Reference |
| --- | --- | --- |
| `GetProcessTimes` | Samples per-process kernel and user time; consecutive samples are converted to total-system CPU percentage. Creation time prevents PID-reuse comparisons and binds path-enriched automatic process-control observations to the exact process instance. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes |
| `K32GetProcessMemoryInfo` | Reads the current working-set size displayed in the Process List. | https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getprocessmemoryinfo |
| `GetProcessInformation(ProcessPowerThrottling)` | Reads the execution-speed control and state masks used to report Efficiency mode where supported. The query is unavailable before Windows 11 22H2 even though `SetProcessInformation` can still apply the state. Query failure leaves status unavailable and blocks a reversible mutation because Winderust cannot capture the original state for clean-exit or crash recovery. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessinformation |
| `TerminateProcess` | `src/platform/windows/process_termination.rs` is the sole raw adapter for confirmed Stop Process and Stop Process Tree commands. `ProcessTerminationController` retains the selected exact roots through confirmation, requires PID, creation time, name, and path to match, and opens every exact, critical/PPL-safe `PROCESS_TERMINATE` handle before the first kill. Tree discovery rejects unknown timestamps and stale numeric parent links; a preflight failure causes zero mutation, while an execution failure does not prevent later targets and is returned as a typed partial result. | https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess |
| `GetSystemWindowsDirectoryW` | Resolves the shared Windows directory and launches its absolute `explorer.exe` path for Open Process Location, avoiding executable-name search through application-controlled directories. | [Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-getsystemwindowsdirectoryw) / [Rust `Command`](https://doc.rust-lang.org/stable/std/process/struct.Command.html#method.new) |
| Process security and access rights | Defines the minimal `PROCESS_TERMINATE` right required by `TerminateProcess`; inaccessible processes remain unavailable rather than triggering privilege escalation. | https://learn.microsoft.com/en-us/windows/win32/procthread/process-security-and-access-rights |

Windows exposes no documented process-wide suspension query. The Process List
therefore reports suspension only for process IDs currently suspended and
tracked by Winderust; it does not infer suspension from undocumented NT thread
state.

# GPU Engine utilization

- Implementation: `src/platform/windows/gpu_usage.rs` owns the English PDH wildcard query for
  `\\GPU Engine(*)\\Utilization Percentage`, formatted-array parsing, per-engine aggregation, and
  query cleanup. `src/bottleneck_classifier.rs` consumes only the resulting observation.
- References: [PdhAddEnglishCounterW](https://learn.microsoft.com/windows/win32/api/pdh/nf-pdh-pdhaddenglishcounterw),
  [PdhGetFormattedCounterArrayW](https://learn.microsoft.com/windows/win32/api/pdh/nf-pdh-pdhgetformattedcounterarrayw).
- Contract: GPU Engine instances are process-scoped. Strip only the `pid_<number>_` prefix, sum
  matching physical-engine instances, clamp each engine to 100%, and report the busiest engine.
  Missing or invalid samples are unavailable observations, not zero utilization.

# Appearance change notifications

- `src/backend/windows_events.rs` routes WM_SETTINGCHANGE, WM_THEMECHANGED, and
  WM_DWMCOLORIZATIONCOLORCHANGED through AppearanceChanged. The runtime generation
  makes `src/ui/app.rs` rebuild the shared Iced theme using the current preferences.
- References: [WM_THEMECHANGED](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-themechanged),
  [WM_DWMCOLORIZATIONCOLORCHANGED](https://learn.microsoft.com/en-us/windows/win32/dwm/wm-dwmcolorizationcolorchanged).
- `src/platform/windows/appearance.rs` reads UISettings Foreground and AccentLight2 via
  [GetColorValue](https://learn.microsoft.com/en-us/uwp/api/windows.ui.viewmanagement.uisettings.getcolorvalue).
  Dark system foreground indicates light mode. Each successful
  [RoInitialize](https://learn.microsoft.com/en-us/windows/win32/api/roapi/nf-roapi-roinitialize)
  is balanced after the UISettings object is released. Explicit theme/custom accent
  preferences override system values. API failure is reported and uses the app default palette;
  no undocumented appearance registry fallback is retained.
