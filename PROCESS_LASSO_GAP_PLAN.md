# Process Lasso Gap Implementation Plan

## Goal

Close the most useful Process Lasso feature gaps while keeping PowerLeaf's design centered on
power efficiency, foreground responsiveness, and conservative process control.

## Implementation Sequence

1. CPU Cap Rules
   - Status: backend and UI implemented with configurable process rules, thresholds, cooldowns, and logical processor limits.
   - Add TOML-backed per-process rules.
   - Detect sustained per-process CPU usage over a threshold.
   - Temporarily constrain the process to a smaller hard affinity mask.
   - Restore the prior affinity after cooldown, disable, process exit, or app shutdown.
   - Skip protected processes, the foreground app by default, PowerLeaf itself, and processes
     already managed by Core Steering.

2. Action Log and Rule History
   - Status: backend ring buffer, read-only UI page, and CSV export implemented for CPU Cap Rules, EcoQoS, Core Steering, Foreground Responsiveness, App Suspension, Running App Detection, and Watchdog events.
   - Record process-control actions with timestamp, feature, process name, PID, action, result,
     and reason.
   - Keep an in-memory ring buffer first.
   - Export retained entries as CSV from the Action Log page.

3. Running App Detection
   - Status: backend and UI implemented with configurable rules, action-log entries, plan restore, and Foreground Responsiveness exclusion for active Running App Detection processes.
   - Add per-process rules that switch to a selected performance power plan while matched apps run.
   - Exclude matched foreground apps from background lowering while the mode is active.
   - Restore the previous plan when the triggering process exits.

4. Watchdog Rules
   - Status: backend and UI implemented for terminate-on-launch and restart-if-exited rules.
   - Add process start/exit rules for selected apps.
   - Start with terminate-on-launch and restart-if-exited.
   - Add instance limits after the action log exists.

5. Persistent Launch-Time Rules
   - Status: runtime process-appearance detection implemented; launch-sensitive managers are refreshed immediately when new processes appear.
   - Reapply priority, affinity, CPU limiter, and Efficiency Mode rules immediately when a process
     appears.
   - Investigate registry-backed persistence only where Windows supports it cleanly and safely.

6. UI Polish and Profiles
   - Status: navigation grouping refined into Power Plan Automation, Processor Controls, and Process Policies.
   - Status: processor tuning can link AC and Battery edits for combined slider changes.
   - Status: shared Process Rules page added for mapping one process to multiple process-control modules.
   - Status: repeated process-control status/stat cards removed from detailed rule pages.
   - Status: Running App Detection folded into the main power-plan decision engine so it overlaps cleanly with Foreground Detection.
   - Status: Process Rules now owns foreground/running-app plan mapping in the main nav, reducing duplicate top-level pages.
   - Status: Process Rules groups per-process toggles by Plan, CPU, and Policy, with CPU Cap Rules shown as CPU Cap and Core Steering shown as CPU Placement.
   - Status: CPU Load Detection now sits under Running App Detection in Power Plan Automation and is labeled separately from per-process CPU Cap rules.
   - Status: Foreground Detection rules are visible in Power Plan Automation and share Foreground Detection wording with Process Rules.
   - Status: Running App Detection rules are visible in Power Plan Automation and share Running App Detection wording with Process Rules.
   - Add dedicated pages or integrate controls into existing process-control pages.
   - Add gaming/work/battery profile presets after the runtime systems are stable.

7. GPU Priority Control
   - Status: backend, automation worker, Action Log, Process Control UI page, navigation, search, and locale entries implemented.
   - Add per-process GPU scheduling priority rules after the existing CPU, I/O, and memory priority paths are stable.
   - Use a dedicated backend module such as `src/gpu_priority.rs` instead of folding this into CPU priority, CPU limiter, or EcoQoS.
   - Use the WDDM D3DKMT process scheduling priority APIs:
     - `D3DKMTGetProcessSchedulingPriorityClass`
     - `D3DKMTSetProcessSchedulingPriorityClass`
     - `D3DKMT_SCHEDULINGPRIORITYCLASS`
   - Reference implementation/API sources:
     - System Informer exposes wrappers in `phlib/include/phutil.h` and D3DKMT declarations in `plugins/ExtendedTools/d3dkmt/d3dkmthk.h`: https://github.com/winsiderss/systeminformer
     - NtDoc WDDM API docs: https://ntdoc.m417z.com/d3dkmtsetprocessschedulingpriorityclass, https://ntdoc.m417z.com/d3dkmtgetprocessschedulingpriorityclass, https://ntdoc.m417z.com/d3dkmt_schedulingpriorityclass
   - Start with manual per-process rules: Idle, Below Normal, Normal, and Above Normal.
   - Do not expose High or Realtime by default. If they are ever added, gate them behind an explicit advanced warning and never apply them automatically.
   - Store the previous GPU scheduling priority per PID and restore it on rule removal, disable, process exit, or app shutdown.
   - Reuse existing process safety gates: skip PowerLeaf itself, system/protected processes, built-in exclusions, cross-session processes, and foreground apps when the setting says to exclude foreground apps.
   - Add Action Log events for apply, restore, skip, and failure.
   - UI placement: Process Policies as "GPU Priority", and expose a compact per-process toggle/selector in Process Rules after backend behavior is proven.

8. Timer Resolution Control
   - Status: backend, foreground-rule automation, Action Log, Advanced UI page, navigation, search, and locale entries implemented.
   - Add a system-wide timer resolution page under Advanced, not under Process Policies. This is a global scheduler/power request, but PowerLeaf applies it only while a matching foreground rule is active.
   - Use a dedicated backend module such as `src/timer_resolution.rs`.
   - Use native timer resolution APIs:
     - `NtQueryTimerResolution` to read maximum, minimum, and current timer resolution.
     - `NtSetTimerResolution` to request or release a resolution from the PowerLeaf process.
   - Reference API source: NtDoc timer docs at https://ntdoc.m417z.com/ntquerytimerresolution and https://ntdoc.m417z.com/ntsettimerresolution
   - Display editable foreground-rule values in milliseconds while keeping internal values in 100-nanosecond units.
   - Treat PowerLeaf's request as process-held state: release it on disable, app shutdown, failed setting apply, and when the foreground app no longer matches a rule.
   - Show both the requested resolution and the actual current resolution because Windows may clamp the request.
   - Default to off and require at least one foreground app rule before requesting a resolution.
   - Add a warning in the UI that lower timer values can increase wakeups and battery drain.
   - Add Action Log events for request, release, query failure, apply failure, and actual-resolution changes.

## Safety Constraints

- Do not touch system processes.
- Do not use High or Realtime priority.
- Restore all runtime changes on disable/drop when possible.
- Treat access denied as skipped unless it indicates an implementation bug.
- Prefer isolated first versions over a premature shared process-policy manager.
- For GPU priority, apply the same priority safety posture as CPU priority unless a future explicit advanced mode says otherwise.
- For timer resolution, always release PowerLeaf's active request before shutdown and whenever automation is disabled.
