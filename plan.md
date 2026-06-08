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
