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
   - Status: backend ring buffer, read-only UI page, and CSV export implemented for CPU Cap Rules, EcoQoS, Core Steering, Foreground Responsiveness, App Suspension, Running App Power Plans, and Watchdog events.
   - Record process-control actions with timestamp, feature, process name, PID, action, result,
     and reason.
   - Keep an in-memory ring buffer first.
   - Export retained entries as CSV from the Action Log page.

3. Running App Power Plans
   - Status: backend and UI implemented with configurable rules, action-log entries, plan restore, and Foreground Responsiveness exclusion for active Running App Power Plan processes.
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
   - Status: navigation grouping refined into Power Automation, Processor Controls, and Process Policies.
   - Status: processor tuning can link AC and Battery edits for combined slider changes.
   - Status: shared Process Rules page added for mapping one process to multiple process-control modules.
   - Status: repeated process-control status/stat cards removed from detailed rule pages.
   - Status: Running App Power Plans folded into the main power-plan decision engine so it overlaps cleanly with Foreground Power Plans.
   - Status: Process Rules now owns foreground/running-app plan mapping in the main nav, reducing duplicate top-level pages.
   - Status: Process Rules groups per-process toggles by Plan, CPU, and Policy, with CPU Cap Rules shown as CPU Cap and Core Steering shown as CPU Placement.
   - Status: CPU Load power-plan automation now sits with CPU controls in navigation and is labeled separately from per-process CPU Cap rules.
   - Status: Foreground power-plan rules are visible in Power Automation and share foreground power-plan wording with Process Rules.
   - Status: Running-app power-plan rules are visible in Power Automation and share running-app power-plan wording with Process Rules.
   - Add dedicated pages or integrate controls into existing process-control pages.
   - Add gaming/work/battery profile presets after the runtime systems are stable.

## Safety Constraints

- Do not touch system processes.
- Do not use High or Realtime priority.
- Restore all runtime changes on disable/drop when possible.
- Treat access denied as skipped unless it indicates an implementation bug.
- Prefer isolated first versions over a premature shared process-policy manager.
