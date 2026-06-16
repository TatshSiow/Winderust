# Plan: ProBalance-Style Mode Scheduler

## 1) Objective
Build a Windows scheduler utility inspired by Bitsum-style ProBalance behavior:
- Keep the machine responsive under high CPU load.
- Temporarily down-prioritize background processes.
- Preserve user control, reversibility, and safe defaults.
- Include a TweakScheduler-style foreground/background time-slice mode.

## 2) Scope
### In scope
- CPU contention detection and temporary priority restraint engine.
- Configurable thresholds and exclusion rules.
- Mode presets (Adaptive / Balanced / Gaming / Background).
- Win32PrioritySeparation tuning with rollback.
- Per-process logging and restore safety.

### Out of scope (initial version)
- Full UI redesign or advanced telemetry dashboards.
- Cross-platform support.
- Non-Windows scheduling internals.

## 3) Requirements
### Functional
1. Sample CPU and process activity periodically.
2. Detect global contention and identify out-of-control processes.
3. Apply temporary priority changes only when all constraints are met.
4. Restore priorities when load normalizes.
5. Respect exclusions by default and policy flags.
6. Persist settings and support import/export.
7. Toggle modes safely and deterministically.
8. Optionally write/read foreground scheduling registry policy.

### Non-functional
1. Safe defaults and conservative behavior.
2. Reversible operations with explicit audit logs.
3. Minimal overhead.
4. Clear diagnostics and reason codes for every action.
5. Graceful fallback if permission/priority operations fail.

## 4) Data model
### Config schema (INI-like)
- `[OutOfControlProcessRestraint]`
  - `OocOn`
  - `TotalProcessorUsageBeforeRestraint`
  - `PerProcessUsageBeforeRestraint`
  - `TimeOverQuotaBeforeRestraint`
  - `PerProcessUsageForRestore`
  - `MinimumTimeOfRestraint`
  - `TameOnlyNormal`
  - `LowerToIdleInsteadOfBelowNormal`
  - `ExcludeServices`
  - `ExcludeForegroundProcesses`
  - `OocExclusions` (wildcards)
- `[Performance]`
  - `UpdateSpeed`
  - `ForcedMode`
  - `ManageOnlyCurrentUser`
- `[ProcessDefaults]`
  - default priority/affinity baselines (optional)
- `[GUI]` (if UI mode present)
  - visibility preferences

### Runtime state
- Process snapshot:
  - PID, executable, CPU%, thread count, priority class, foreground flag, service flag
- Per-process restraint state:
  - original priority
  - restrained-at timestamp
  - last-evaluated timestamp
  - reason code
  - pending restore flag

## 5) Core algorithm
### Sampling loop
- Run every `UpdateSpeed` ms.
- Gather:
  - total CPU usage
  - foreground PID
  - per-process CPU usage and priority

### Trigger logic
- If `OocOn == false`, skip.
- If total CPU < `TotalProcessorUsageBeforeRestraint`, skip.
- For each candidate process:
  - skip if excluded by rules.
  - require sustained high usage for `TimeOverQuotaBeforeRestraint`.
  - ensure per-process usage >= `PerProcessUsageBeforeRestraint`.
  - select strongest candidate set (bounded by top consumers and safety caps).

### Action logic
- Restrain: set process priority to:
  - below normal (default), or idle (if strict mode).
- Record original priority and timestamp.
- Log action with reason and context.

### Restore logic
- Restore when process usage <= `PerProcessUsageForRestore`
  and held long enough for minimum restraint dwell.
- Restore only if no overlapping hard-constraint violation remains.

## 6) Exclusion rules
- Exclude foreground process (default ON).
- Exclude non-normal priorities (default ON).
- Exclude services if configured.
- Exclude explicit wildcards list (`OocExclusions`).
- Optional planned exclusions:
  - child-process policy,
  - game process whitelist rules,
  - temporary “do-not-touch” UI override per PID.

## 7) Mode presets
- **Adaptive (default):**
  - conservative thresholds
  - foreground exclusion ON
  - non-normal exclusion ON
  - no idle demotion
- **Balanced:**
  - slightly tighter thresholds
  - shorter restore/restraint windows
- **Gaming:**
  - restraint disabled by default
  - can pair with aggressive foreground scheduling preset
- **Background / Productivity:**
  - higher restraint aggressiveness
  - faster reaction to sustained hogging

## 8) TweakScheduler component
### Registry target
- `HKLM\SYSTEM\CurrentControlSet\Control\PriorityControl\Win32PrioritySeparation`

### Presets
- Let OS choose
- Foreground-favor (Applications-like behavior)
- Equal slice (Background-services-like behavior)

### Safety
- Read current value at startup.
- Save previous value for rollback.
- Restore on service stop or emergency disable.

## 9) Logging and observability
- Log entries:
  - timestamp, PID, exe, original priority, new priority, mode, reason, action (restraint/restore).
- Optional verbose mode:
  - thresholds applied
  - excluded matches
  - scheduler mode transitions

## 10) Implementation plan
### Milestone 1: Foundations (Week 1)
- Project skeleton, process sampler, permission model.
- INI parser and schema validation.
- CLI/API for read/write config.

### Milestone 2: Engine (Weeks 2–3)
- Implement core sampling and restraint state machine.
- Implement restore logic, cooldown, and exclusion engine.
- Add deterministic unit-style policy tests (logic-level).

### Milestone 3: Foreground scheduling (Week 3)
- Add Win32PrioritySeparation writer/reader.
- Add presets + rollback + permission checks.

### Milestone 4: Interface + operational controls (Week 4)
- Tray/CLI mode switcher.
- Exclusion list editor.
- Manual restore/all clear controls.

### Milestone 5: Hardening + release prep (Week 5)
- Stability tests under load.
- Race-condition and permission failure handling.
- Installer/startup behavior and rollback strategy.
- Documentation for tuning and safety.

## 11) Risks and mitigations
- **Permission errors** when setting priorities/registry.
  - Mitigation: elevation flow + clear error states + retry + fallback.
- **Priority oscillation** (thrashing).
  - Mitigation: hysteresis + minimum dwell + cooldown window.
- **False positives** on CPU spikes.
  - Mitigation: conservative defaults and strict restore thresholds.
- **User confusion** from hidden side effects.
  - Mitigation: explicit logs, visible mode status, one-click disable.

## 12) Definition of done
- Default mode does not destabilize system under CPU stress.
- Temporary actions always restore correctly.
- All mode changes are immediate and reversible.
- Exclusion and presets behave as configured.
- Installer/start/stop lifecycle restores modified scheduler state.

## 13) Next steps
1. Finalize exact default numbers per threshold and windows behavior from validation.
2. Confirm whether to start with CLI-only or CLI + lightweight UI.
3. Start Milestone 1 implementation.

## 14) Current Process Lasso parity gaps

PowerLeaf now covers several Process Lasso-like capabilities beyond the original
ProBalance-style plan:
- CPU limiter / CPU cap rules.
- Action log with CSV export.
- Running-app power plan switching.
- Watchdog terminate-on-launch and restart-if-exited rules.
- Per-process CPU affinity / CPU placement.
- Per-process I/O priority.
- EcoQoS / Efficiency Mode.
- Foreground responsiveness boosting and background lowering.
- App suspension.
- Win32PrioritySeparation tuning.
- TOML settings import/export.

The remaining gaps versus Bitsum Process Lasso are mostly depth, process-manager
coverage, and runtime architecture.

### A. Full process-manager UI
- Missing live all-processes and active-processes tables.
- Missing tree view, selectable columns, sorting, filtering, and process detail panels.
- Missing multi-select process actions.
- Missing right-click-style process context commands.
- Missing visible per-process rule/status columns comparable to Process Lasso.
- Current PowerLeaf UI has process pickers and feature-specific pages, but not a
  Task Manager-like control surface.

### B. Split GUI/governor architecture
- Process Lasso separates its GUI from an always-on Process Governor.
- PowerLeaf currently appears to run automation inside the app/background automation
  lifecycle rather than a separate service or headless governor.
- Gap: service mode, GUI-independent operation, restart recovery, and explicit
  governor status/control.

### C. Broader priority controls
- CPU priority coverage is narrower than Process Lasso.
- I/O priority supports Normal, Low, and VeryLow, but not the full Process Lasso
  surface.
- Missing GPU priority rules.
- Missing memory priority rules.
- Missing richer dynamic thread priority boost controls.
- Missing persistent default priority model across the full Process Lasso range.

### D. GPU telemetry and GPU policy
- Missing GPU utilization display.
- Missing per-process GPU metrics.
- Missing GPU priority rule support.

### E. Advanced Watchdog rules
- Current watchdog supports terminate-on-launch and restart-if-exited.
- Missing CPU threshold watchdog actions.
- Missing memory threshold watchdog actions.
- Missing elapsed-time, responsiveness, or other resource-condition triggers.
- Missing actions such as changing affinity/priority from watchdog conditions.

### F. Instance controls
- Missing per-process instance count limits.
- Missing Instance Balancer-like behavior.
- Missing "keep only N instances" and "stop duplicate launch" workflows.

### G. SmartTrim and memory workflows
- Missing SmartTrim-style working set trimming.
- Missing standby-list cleanup.
- Missing memory pressure telemetry and policy controls.

Process Lasso SmartTrim behavior to match:
- Periodically trims working sets of high-memory processes when thresholds are
  reached.
- Can clear the system standby list / cache.
- Supports per-process SmartTrim exclusions, shown as `t` in Process Lasso's
  Rules column.
- Has an immediate command path (`/TrimNow`) that asks SmartTrim to act now using
  its configured policy.

Current PowerLeaf status:
- No SmartTrim manager/module exists.
- No direct use of `EmptyWorkingSet`, `SetProcessWorkingSetSize`, or standby-list
  clearing APIs exists.
- `PROCESS_SET_QUOTA` is already used by App Suspension, but PowerLeaf does not
  currently use it for memory working-set trimming.

System Informer / NTDoc implementation notes:
- Per-process working-set trim can use the documented `EmptyWorkingSet` API, or
  `SetProcessWorkingSetSize(process, SIZE_MAX, SIZE_MAX)`.
- System Informer's lower-level equivalent sets `ProcessQuotaLimits` with
  `QUOTA_LIMITS_EX { MinimumWorkingSetSize = SIZE_MAX, MaximumWorkingSetSize =
  SIZE_MAX }` through `NtSetInformationProcess`.
- Opening a process for trimming normally needs `PROCESS_SET_QUOTA`; the
  documented `EmptyWorkingSet` path also requires query access.
- System memory-list inspection uses
  `NtQuerySystemInformation(SystemMemoryListInformation, SYSTEM_MEMORY_LIST_INFORMATION)`.
- The `SYSTEM_MEMORY_LIST_INFORMATION` data includes zero, free, modified,
  modified-no-write, bad, standby-by-priority, repurposed-by-priority, and
  modified-pagefile page counts.
- System memory-list commands use
  `NtSetSystemInformation(SystemMemoryListInformation, SYSTEM_MEMORY_LIST_COMMAND)`.
- Relevant commands are `MemoryEmptyWorkingSets`, `MemoryFlushModifiedList`,
  `MemoryPurgeStandbyList`, and `MemoryPurgeLowPriorityStandbyList`.
- System Informer's headers mark `SystemMemoryListInformation` set operations as
  requiring `SeProfileSingleProcessPrivilege`.
- System file-cache flushing can use `SetSystemFileCacheSize(SIZE_MAX, SIZE_MAX,
  0)` / the native `SystemFileCacheInformationEx` path, and requires
  `SE_INCREASE_QUOTA_NAME`.
- For PowerLeaf, prefer documented APIs first. Keep native memory-list and
  file-cache operations behind an Advanced/off-by-default setting because they
  can degrade performance and may change across Windows versions.

Recommended PowerLeaf implementation:
1. Add `SmartTrimSettings`:
   - `enabled`
   - `check_interval_seconds`
   - `system_memory_load_threshold_percent`
   - `process_working_set_threshold_mb`
   - `process_idle_seconds`
   - `exclude_foreground_app`
   - `clear_standby_list_enabled`
   - `clear_low_priority_standby_only`
   - `clear_file_cache_enabled`
   - `trim_now_requested`
   - `exclusions: Vec<ProcessExclusionRule>`
2. Add a `smart_trim` manager:
   - Sample system memory load.
   - Sample per-process working set/private bytes.
   - Skip protected/system processes, foreground app, PowerLeaf, and configured
     exclusions.
   - Trim only processes above threshold and idle long enough.
   - Record every trim, skip, and failure in Action Log.
3. Start with per-process working-set trim only:
   - Use `EmptyWorkingSet` or `SetProcessWorkingSetSize(..., SIZE_MAX, SIZE_MAX)`
     conservatively.
   - Treat access denied as skipped.
   - Add a cooldown so the same process is not repeatedly trimmed.
4. Add native memory-list cleanup later:
   - Keep it off by default.
   - Prefer `MemoryPurgeLowPriorityStandbyList` before full
     `MemoryPurgeStandbyList`.
   - Gate standby-list, modified-list, and file-cache cleanup behind an advanced
     warning because clearing cache can reduce performance by causing avoidable
     disk reads/page faults.
5. Add UI after the backend is stable:
   - SmartTrim page under Process Policies or Advanced.
   - Exclusion list.
   - `Trim Now` button.
   - Last trim count, freed working-set estimate, skipped count, and last error.

### H. Forced mode / sticky enforcement
- Missing a global mode that continuously reapplies process priority and affinity
  rules when another tool or the user changes them.
- Existing managers refresh rules, but this is not equivalent to explicit Process
  Lasso Forced Mode.

### I. More expressive process matching
- Matching is still mostly process-name based in many modules.
- Some wildcard behavior exists, but coverage is inconsistent.
- Missing first-class matching by full path, command line, user, parent process,
  service identity, and regex.

### J. Profiles and policy packaging
- Missing Process Lasso-style configuration profiles.
- Missing fast profile switching from UI/tray/CLI.
- Missing versioned policy import/export with compatibility checks.
- Missing separate per-user versus global policy handling.

### K. Enterprise and deployment features
- Missing silent install/update workflows.
- Missing service startup modes and service recovery configuration.
- Missing command-line switches for config/log locations and profile selection.
- Missing server/RDS/multi-session policy controls.
- Missing centralized policy deployment guidance.

### L. Logging and telemetry depth
- Action Log exists, but entries are in-memory and reset on app exit.
- Missing durable historical database/log store.
- Missing long-range charts for responsiveness, restraints, CPU pressure, and
  process actions.
- Missing exportable diagnostic bundles.

### M. Recommended next parity milestones
1. Build a live process table with context actions, rule/status columns, and basic
   process details.
2. Split automation into a small headless governor or Windows service while keeping
   the GPUI app as the controller.
3. Expand watchdog to CPU and memory threshold rules with priority/affinity actions.
4. Add instance count limits and keep-running/disallowed-process policy depth.
5. Add Forced Mode for sticky priority/affinity enforcement.
6. Add richer process matchers: path, command line, user, wildcard, and regex.
7. Add durable action history and longer-term telemetry charts.
8. Add GPU and memory priority/telemetry only after the core process manager and
   governor architecture are stable.
