# Foreground Responsiveness Mode Plan

## Goal

Add configurable responsiveness controls to PowerLeaf so foreground apps stay responsive under CPU contention, while EcoQoS remains the primary power-efficiency mechanism.

The feature should be presented honestly:

- It improves responsiveness under contention.
- It can support power saving when paired with EcoQoS.
- It does not guarantee lower power by itself, because lower process priority does not cap total CPU work.
- It should prefer lowering background work over boosting foreground work.

## Current State

PowerLeaf already has the main plumbing needed:

- `src/ecoqos/mod.rs` applies Windows EcoQoS and lowers matched processes to `IDLE_PRIORITY_CLASS`.
- `src/automation.rs` runs process automation in the background thread.
- `src/foreground` can detect the foreground process.
- `src/config/settings.rs` stores EcoQoS and App Suspension settings.
- `src/app.rs` already has process pickers and pages for process-control features.

The missing piece is a user-facing responsiveness rule system that coordinates with Efficiency Mode instead of independently fighting it.

## Scope

Implement a conservative first version:

- Add a new Foreground Responsiveness feature/page.
- Match rules by process name.
- Apply only safe downward/neutral background priorities:
  - `Normal`
  - `Below Normal`
  - `Idle`
- Exclude the foreground app by default for background lowering.
- Optionally allow a foreground boost to `Above Normal`, disabled by default.
- Reuse or mirror the existing built-in exclusions from EcoQoS.
- Store the previous priority per PID and restore it when the process no longer matches.
- Avoid `High` and `Realtime` entirely.
- Do not touch thread priority in v1.

## Non-Goals

- No CPU affinity or CPU Sets.
- No hard CPU throttling.
- No priority boosting for foreground apps.
- No registry-enforced persistent priority rules.
- No service process split.
- No global timer-resolution control in this feature.

## Configuration Model

Add settings similar to:

```rust
pub struct ForegroundResponsivenessSettings {
    pub enabled: bool,
    pub lower_background_apps: bool,
    pub boost_foreground_app: bool,
    pub foreground_boost: ForegroundBoostPriority,
    pub foreground_stability_delay_ms: u64,
    pub rules: Vec<PriorityRule>,
}

pub struct PriorityRule {
    pub enabled: bool,
    pub process_name: String,
    pub priority: ProcessPriority,
}

pub enum ProcessPriority {
    Normal,
    BelowNormal,
    Idle,
}

pub enum ForegroundBoostPriority {
    Normal,
    AboveNormal,
}
```

Default:

```text
enabled = false
lower_background_apps = true
boost_foreground_app = false
foreground_boost = above_normal
foreground_stability_delay_ms = 750
rules = []
```

TOML shape:

```toml
[foreground_responsiveness]
enabled = false
lower_background_apps = true
boost_foreground_app = false
foreground_boost = "above_normal"
foreground_stability_delay_ms = 750

[[foreground_responsiveness.rules]]
enabled = true
process_name = "backup.exe"
priority = "idle"

[[foreground_responsiveness.rules]]
enabled = true
process_name = "browser.exe"
priority = "below_normal"
```

## Runtime Design

Create a new module:

```text
src/responsiveness/
  mod.rs
```

Responsibilities:

- Enumerate processes with existing `foreground::list_processes()`.
- Filter to the current user session.
- Skip PowerLeaf itself.
- Always skip the foreground process for background lowering.
- Skip protected/built-in excluded processes.
- Apply the configured priority class using `SetPriorityClass`.
- Remember previous priority per PID.
- Restore previous priority when:
  - automation is disabled,
  - Foreground Responsiveness Mode is disabled,
  - a process no longer matches a rule,
  - a process becomes foreground,
  - the manager is dropped.
- Optionally boost the stable foreground process to `ABOVE_NORMAL_PRIORITY_CLASS`.
- Restore boosted foreground process priority when focus changes.

Use existing Windows APIs already used in `src/ecoqos/mod.rs`:

- `OpenProcess`
- `GetPriorityClass`
- `SetPriorityClass`
- `CloseHandle`

Prefer sharing duplicated process-handle code only if the duplication becomes noisy. For v1, a small local helper is acceptable to keep the change low-risk.

## Priority Mapping

Map settings to Windows priority classes:

```text
Normal background       -> NORMAL_PRIORITY_CLASS
BelowNormal background  -> BELOW_NORMAL_PRIORITY_CLASS
Idle background         -> IDLE_PRIORITY_CLASS
AboveNormal foreground  -> ABOVE_NORMAL_PRIORITY_CLASS
```

Do not expose:

```text
ABOVE_NORMAL_PRIORITY_CLASS
HIGH_PRIORITY_CLASS
REALTIME_PRIORITY_CLASS
```

Those can reduce stability or starve other work when misused. Microsoft also warns that `REALTIME_PRIORITY_CLASS` can interfere with system threads responsible for input and disk flushing.

## Automation Integration

Update `BackgroundAutomation`:

- Add `ForegroundResponsivenessManager` to `HiddenAutomationRunner`.
- Add `ForegroundResponsivenessSnapshot` to shared worker state.
- Refresh priority control on the same cadence as EcoQoS at first.
- Later, optimize cadence if needed.

Potential constant:

```rust
const FOREGROUND_RESPONSIVENESS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
```

This is acceptable initially because EcoQoS already scans on a 1-second cadence. Avoid adding another independent full process scan if possible; a later optimization can share process snapshots across EcoQoS, Foreground Responsiveness, CPU Affinity, and App Suspension.

## UI Plan

Add `Page::ForegroundResponsiveness` under the process-control group.

Page content:

- Enable Foreground Responsiveness toggle.
- Lower background apps toggle.
- Boost foreground app toggle.
- Foreground boost selector, limited to `Above Normal` or `Normal`.
- Foreground stability delay input.
- Status metrics:
  - scanned processes,
  - background adjusted processes,
  - foreground boosted process,
  - skipped processes,
  - failed processes.
- Add process rule row:
  - process picker,
  - priority selector,
  - add button.
- Rule list:
  - enabled toggle,
  - process name,
  - selected priority,
  - remove button.

Suggested copy:

```text
Foreground Responsiveness lowers selected background apps to preserve active-app responsiveness.
For power saving, pair this with Efficiency Mode.
```

## Interaction With Existing Features

Foreground Responsiveness and EcoQoS may both target the same process.

Initial behavior:

- EcoQoS continues to own `Idle` priority when it throttles a process.
- Foreground Responsiveness must not fight EcoQoS.
- If EcoQoS has already throttled a PID, Foreground Responsiveness must skip background priority changes for that PID.
- If a PID becomes foreground and is currently EcoQoS-throttled, EcoQoS should release it before any optional foreground boost is applied.

Preferred v1 rule:

```text
If EcoQoS has already throttled a PID, Foreground Responsiveness does not overwrite it.
```

This avoids restore-order bugs where one manager restores a priority another manager intentionally changed.

Implementation options:

- Expose throttled PID set from `EcoQosManager`.
- Or keep v1 simpler by documenting that Foreground Responsiveness and EcoQoS should not be configured for the same process.

Better implementation:

- Introduce one shared `ProcessPolicyManager` later to coordinate EcoQoS, priority, timer policy, affinity, and suspension state.
- Do not build that abstraction for v1 unless restore conflicts become hard to avoid.

`CpuAffinityMode::EfficiencyOff` also touches `ProcessPowerThrottling`, so it should not target the same process as EcoQoS unless there is explicit coordination.

## Safety Rules

- Never modify system processes.
- Never modify PowerLeaf itself.
- Never use Realtime or High priority in v1.
- Only use `Above Normal` as an optional foreground boost.
- Keep foreground boost disabled by default.
- Debounce foreground changes before boosting.
- Restore prior priority on disable/drop.
- Treat access denied as skipped, not failed.
- Keep foreground exclusion enabled by default.
- Preserve user changes in unrelated files.

## Tests

Unit tests:

- Priority enum serializes/deserializes expected TOML names.
- Built-in exclusions are respected.
- Foreground skip matches by PID and process name.
- Disabled settings clear previously adjusted processes.
- Rule matching is case-insensitive and trims whitespace.
- Priority mapping returns the expected Windows constants.
- Foreground boosting waits for the configured stability delay.
- EcoQoS-managed PIDs are skipped by foreground responsiveness background lowering.

Manual verification:

- Add a rule for a harmless test process.
- Confirm priority changes in Task Manager or Process Explorer.
- Bring the process foreground and confirm it restores/skips when foreground exclusion is enabled.
- Enable optional foreground boost and confirm only the stable foreground process becomes `Above Normal`.
- Disable Foreground Responsiveness and confirm priority restores.
- Enable EcoQoS and Foreground Responsiveness with different targets and confirm they do not interfere.

## Suggested Implementation Order

1. Add settings structs, defaults, and TOML storage tests.
2. Add `responsiveness` module with manager, snapshot, priority mapping, and unit tests.
3. Integrate the manager into `BackgroundAutomation`.
4. Add status plumbing to `PowerLeafApp`.
5. Add `Foreground Responsiveness` page and navigation entry.
6. Run `cargo fmt`, `cargo test`, and `cargo check`.
7. Manually verify priority apply/restore on Windows.

## Future Extensions

- Per-rule condition: only apply while background.
- Per-rule delay after process launch.
- Combined rules: EcoQoS + priority + timer-resolution policy.
- Process Watchdog rules based on CPU usage duration.
- Action log for applied/restored priority changes.
- Profile-specific rules for battery, plugged-in, gaming, and work.
