# Process Priority Control Plan

## Goal

Add configurable process priority control to PowerLeaf so background apps can be restrained for better foreground responsiveness, with EcoQoS remaining the primary power-efficiency mechanism.

Priority control should be presented honestly:

- It improves responsiveness under contention.
- It can support power saving when paired with EcoQoS.
- It does not guarantee lower power by itself, because an idle-priority process can still consume available CPU.

## Current State

PowerLeaf already has the main plumbing needed:

- `src/ecoqos/mod.rs` applies Windows EcoQoS and lowers matched processes to `IDLE_PRIORITY_CLASS`.
- `src/automation.rs` runs process automation in the background thread.
- `src/foreground` can detect the foreground process.
- `src/config/settings.rs` stores EcoQoS and App Suspension settings.
- `src/app.rs` already has process pickers and pages for process-control features.

The missing piece is a user-facing priority rule system independent of Efficiency Mode.

## Scope

Implement a conservative first version:

- Add a new Process Priority feature/page.
- Match rules by process name.
- Apply only safe downward/neutral priorities:
  - `Normal`
  - `Below Normal`
  - `Idle`
- Exclude the foreground app by default.
- Reuse or mirror the existing built-in exclusions from EcoQoS.
- Store the previous priority per PID and restore it when the process no longer matches.
- Avoid `Above Normal`, `High`, and `Realtime` in v1.

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
pub struct PriorityControlSettings {
    pub enabled: bool,
    pub exclude_foreground_app: bool,
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
```

Default:

```text
enabled = false
exclude_foreground_app = true
rules = []
```

TOML shape:

```toml
[priority_control]
enabled = false
exclude_foreground_app = true

[[priority_control.rules]]
enabled = true
process_name = "backup.exe"
priority = "idle"

[[priority_control.rules]]
enabled = true
process_name = "browser.exe"
priority = "below_normal"
```

## Runtime Design

Create a new module:

```text
src/priority/
  mod.rs
```

Responsibilities:

- Enumerate processes with existing `foreground::list_processes()`.
- Filter to the current user session.
- Skip PowerLeaf itself.
- Skip foreground process when configured.
- Skip protected/built-in excluded processes.
- Apply the configured priority class using `SetPriorityClass`.
- Remember previous priority per PID.
- Restore previous priority when:
  - automation is disabled,
  - Priority Control is disabled,
  - a process no longer matches a rule,
  - a process becomes foreground and foreground exclusion is enabled,
  - the manager is dropped.

Use existing Windows APIs already used in `src/ecoqos/mod.rs`:

- `OpenProcess`
- `GetPriorityClass`
- `SetPriorityClass`
- `CloseHandle`

Prefer sharing duplicated process-handle code only if the duplication becomes noisy. For v1, a small local helper is acceptable to keep the change low-risk.

## Priority Mapping

Map settings to Windows priority classes:

```text
Normal       -> NORMAL_PRIORITY_CLASS
BelowNormal  -> BELOW_NORMAL_PRIORITY_CLASS
Idle         -> IDLE_PRIORITY_CLASS
```

Do not expose:

```text
ABOVE_NORMAL_PRIORITY_CLASS
HIGH_PRIORITY_CLASS
REALTIME_PRIORITY_CLASS
```

Those can reduce stability or starve other work when misused.

## Automation Integration

Update `BackgroundAutomation`:

- Add `PriorityControlManager` to `HiddenAutomationRunner`.
- Add `PriorityControlSnapshot` to shared worker state.
- Refresh priority control on the same cadence as EcoQoS at first.
- Later, optimize cadence if needed.

Potential constant:

```rust
const PRIORITY_CONTROL_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
```

This is acceptable initially because EcoQoS already scans on a 1-second cadence. Avoid adding another independent full process scan if possible; a later optimization can share process snapshots across EcoQoS, Priority Control, and App Suspension.

## UI Plan

Add `Page::PriorityControl` under the process-control group.

Page content:

- Enable Priority Control toggle.
- Exclude foreground app toggle.
- Status metrics:
  - scanned processes,
  - adjusted processes,
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
Priority Control lowers background process priority to preserve foreground responsiveness.
For power saving, pair this with Efficiency Mode.
```

## Interaction With Existing Features

Priority Control and EcoQoS may both target the same process.

Initial behavior:

- EcoQoS can continue to set `Idle`.
- Priority Control should not fight EcoQoS.
- If EcoQoS is active for a process, Priority Control may skip it or treat `Idle` as already acceptable.

Preferred v1 rule:

```text
If EcoQoS has already throttled a PID, Priority Control does not overwrite it.
```

This avoids restore-order bugs where one manager restores a priority another manager intentionally changed.

Implementation options:

- Expose throttled PID set from `EcoQosManager`.
- Or keep v1 simpler by documenting that Priority Control and EcoQoS should not be configured for the same process.

Better implementation:

- Introduce one shared `ProcessPolicyManager` later to coordinate EcoQoS, priority, timer policy, and suspension state.
- Do not build that abstraction for v1 unless restore conflicts become hard to avoid.

## Safety Rules

- Never modify system processes.
- Never modify PowerLeaf itself.
- Never use Realtime or High priority in v1.
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

Manual verification:

- Add a rule for a harmless test process.
- Confirm priority changes in Task Manager or Process Explorer.
- Bring the process foreground and confirm it restores/skips when foreground exclusion is enabled.
- Disable Priority Control and confirm priority restores.
- Enable EcoQoS and Priority Control with different targets and confirm they do not interfere.

## Suggested Implementation Order

1. Add settings structs, defaults, and TOML storage tests.
2. Add `priority` module with manager, snapshot, priority mapping, and unit tests.
3. Integrate the manager into `BackgroundAutomation`.
4. Add status plumbing to `PowerLeafApp`.
5. Add `Priority Control` page and navigation entry.
6. Run `cargo fmt`, `cargo test`, and `cargo check`.
7. Manually verify priority apply/restore on Windows.

## Future Extensions

- Per-rule condition: only apply while background.
- Per-rule delay after process launch.
- Combined rules: EcoQoS + priority + timer-resolution policy.
- Process Watchdog rules based on CPU usage duration.
- Action log for applied/restored priority changes.
- Profile-specific rules for battery, plugged-in, gaming, and work.
