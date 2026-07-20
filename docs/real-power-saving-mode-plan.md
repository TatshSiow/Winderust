# Adaptive Engine Implementation Record

## Current Decision

Do not build a second adaptive engine.

Implement `Adaptive Engine` by extending the systems that already exist:

- `src/features/winderust_features/background_efficiency.rs`: background EcoQoS and priority restore.
- `src/features/winderust_features/workload_engine.rs`: adaptive background restraint under CPU pressure.
- `src/features/cpu_control/core_steering.rs`: soft CPU Sets and hard affinity fallback.
- `src/features/advanced_controls/app_suspension.rs`: active-audio detection that can be shared.
- `src/backend/automation.rs`: existing feature scheduling and status fan-out.

The only new low-risk saver behavior for the first implementation is:

```text
ProcessPowerThrottling = EXECUTION_SPEED | IGNORE_TIMER_RESOLUTION
```

That means Winderust keeps using EcoQoS, rollback, foreground protection, exclusions,
priority lowering, and CPU Sets that already exist, then adds the timer-resolution
power hint only when it is safe.

## Goals

- Reduce real battery and idle/background power use without switching Windows power plans.
- Temporarily apply and restore processor Saver values while Adaptive Engine is enabled.
- Preserve foreground responsiveness.
- Restore process state when a process becomes foreground, exits, is excluded, or automation is disabled.
- Avoid media, calls, audio, input, system, security, and foreground workloads.
- Add the smallest implementation that can be tested and rolled back.

## Non-Goals

- No new top-level engine.
- No permanent global processor power-plan changes.
- No global boost disable.
- No hard affinity by default.
- No Job Object CPU caps in MVP.
- No thread-level throttling in MVP.
- No heavyweight benchmark suite in MVP.
- No "battery-life improvement" claim until measured.

## Existing Coverage

| Need | Existing code | Use it how |
| --- | --- | --- |
| Foreground protection | `foreground::process_list`, `ForegroundDetector`, `WorkloadEngineManager::update` | Keep foreground PID/name exclusion as the primary safety gate. |
| Background EcoQoS | `BackgroundEfficiencyManager` | Add ignore-timer-resolution to the same saved/restored throttling state. |
| Adaptive CPU-pressure response | `WorkloadEngineManager` | Keep current pressure, sustain, cooldown, and candidate selection logic. |
| Priority lowering/restoration | `BackgroundEfficiencyManager`, `WorkloadEngineManager` | Reuse existing previous-priority tracking. |
| CPU Sets | `CoreSteeringManager` | Keep as Phase 2 escalation through existing Workload Engine settings. |
| Audio activity | `suspension::active_audio_process_ids` logic | Extract a small shared helper before applying the timer-resolution flag. |
| Exclusions | existing exclusion/rule lists | Reuse; do not add a second whitelist system. |
| Action history | `ActionLog` | Extend messages only where a new timer policy is applied/skipped. |

## MVP Behavior

### Winderust Self Power Handling

When Adaptive Engine is enabled:

- Save the active plan's processor-power values.
- Apply the existing Winderust `ProcessorPowerPreset::Saver` shape to the active plan when the Adaptive Engine processor power policy toggle is enabled by default.
- Apply EcoQoS to Winderust itself.
- Ignore Winderust's own high-resolution timer requests.
- Keep process-wide idle priority only for hidden-to-tray mode.
- Restore the saved processor-power values when Adaptive Engine is disabled or Winderust exits.

### Background Efficiency

Background Efficiency is an opt-in Adaptive Engine escalation. When it targets a process:

- Apply EcoQoS.
- Lower priority as it already does.
- Also set `PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION` if all guards pass.
- Restore the exact previous `PROCESS_POWER_THROTTLING_STATE` on release.

### Workload Engine

Workload Engine is an opt-in Adaptive Engine escalation, not part of the default
master toggle. When enabled and applying automatic background efficiency:

- Keep current CPU-pressure gating.
- Keep current foreground exclusion and launch-boost behavior.
- Add the timer-resolution flag only to the existing process throttling state.
- Keep CPU Sets as the existing escalation path; do not add a new one.

### Timer-Resolution Guard

Do not set `IGNORE_TIMER_RESOLUTION` when:

- The process is foreground or same-name foreground.
- The process has an active audio session.
- Audio detection fails for the tick.
- The process is built-in excluded.
- The process is user-excluded.
- The process already failed enough times to be auto-excluded.

EcoQoS and priority lowering may still run when audio detection fails; only the timer flag is skipped.

## Implementation Steps

### 1. Share Active-Audio Detection

Move the active-audio PID detection from `src/features/advanced_controls/app_suspension.rs` into a small shared module, for example:

```text
src/backend/audio_activity.rs
```

Expose only:

```rust
pub fn active_audio_process_ids() -> Result<BTreeSet<u32>, String>
```

Keep the suspension behavior unchanged.

### 2. Add Timer-Resolution Policy Bit

Update the process-throttling state builders in:

- `src/features/winderust_features/background_efficiency.rs`
- `src/features/winderust_features/workload_engine.rs`

Add `PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION` beside
`PROCESS_POWER_THROTTLING_EXECUTION_SPEED` only when the timer guard allows it.

The saved previous state remains the source of truth for rollback.

### 3. Track What Was Applied

Extend the tracked process state with a boolean if needed:

```rust
applied_ignore_timer_resolution: bool
```

Use it only for status/log clarity. Rollback should still restore the previous raw throttling state.

### 4. Add a Minimal Adaptive Engine Page

Add `Adaptive Engine` under Winderust Features as a thin preset surface over
the existing Background Efficiency and Workload Engine settings.

The page should:

- Enable/disable the saver path without creating a new engine.
- Make Background Efficiency, Workload Engine, and CPU-pressure restraint visible.
- Show timer-resolution saving as guarded by active-audio detection, not as a risky raw toggle.
- Keep detailed tuning on the existing feature pages.

### 5. Tests

Add the smallest tests that fail if the implementation regresses:

- Throttling state includes both EcoQoS and ignore-timer flags when requested.
- Throttling state includes only EcoQoS when timer ignore is blocked.
- Restore uses the original raw throttling state.
- Audio-detection failure skips only the timer flag.

Run:

```powershell
cargo fmt
cargo test
graphify update .
```

Run the package-watt benchmark when measuring power savings:

```powershell
.\scripts\power_drain_benchmark.ps1 -Phases Baseline,AdaptiveEngine -MinPasses 3 -MaxPasses 8 -SampleSeconds 30 -StableCvPercent 5
```

Latest integrated Adaptive Engine benchmark:

| Build | Median package W avg | Stable | Delta vs previous Adaptive Engine |
| --- | ---: | --- | ---: |
| Closed / no Winderust | 7.167 W | true | 0.6% higher |
| Closed / no Winderust, 75s samples | 6.764 W | true | 5.1% lower |
| Previous Adaptive Engine | 7.125 W | true | baseline |
| Tuned Adaptive Engine | 5.915 W | true | 17.0% lower |
| Visible tuned Adaptive Engine with slow app tick | 5.953 W | true | 16.4% lower |
| Hidden tuned Adaptive Engine with worker cadence fix, 75s samples | 5.792 W | true | 18.7% lower |
| Closed / no Winderust after hook suppression, 75s samples | 6.612 W | true | 7.2% lower |
| Hidden Adaptive Engine with hook suppression, 75s samples | 5.809 W | true | 18.5% lower |

Same-run closed-vs-hidden package result after hook suppression:
Hidden Adaptive Engine measured 5.809 W vs closed/no-app 6.612 W, a 12.1% lower package-power median.

## Implementation Status

- Added `src/backend/audio_activity.rs` and reused it from suspension, EcoQoS, and Workload Engine.
- Added Adaptive Engine self-power handling for Winderust's own EcoQoS and timer requests.
- Added temporary active-plan processor Saver values with restore-on-disable/drop and an Adaptive Engine child toggle to disable that processor-policy change.
- Tuned the Adaptive Engine processor shape to cap processor performance at 60% with boost disabled after package-watt benchmarking.
- Slowed Winderust's own UI maintenance tick from 1 second to 60 seconds while Adaptive Engine is enabled to reduce idle wakeups.
- Slowed hidden Adaptive Engine process-appearance and suspended-app release polling to the same 60-second cadence.
- Suppressed Adaptive Engine appearance-only Windows event watching plus app-suspension/process foreground, window, and input hooks so package residency is not kept worse by idle global hooks.
- Added guarded `PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION` handling to EcoQoS and Workload Engine.
- Added timer-resolution status counters to the EcoQoS and Workload Engine snapshots.
- Added the Adaptive Engine page under Winderust Features with English and Traditional Chinese labels.
- Kept Workload Engine restraint opt-in from Adaptive Engine after package-watt testing showed the heavier path can raise idle drain.
- Added `scripts/power_drain_benchmark.ps1` for repeated core/package watt sampling.
- Added focused bitmask and guard tests.

## Deferred Work

| Deferred item | Why skipped | Add when |
| --- | --- | --- |
| Thread power throttling | No safe thread classifier exists. | There is a known owned thread/work queue to target. |
| Job Object CPU caps | High compatibility risk and process job conflicts. | A user-selected Deep Saver workflow exists. |
| Hidden-window classification | Foreground/background already covers the safe MVP. | Timer policy needs finer targeting after real usage. |
| Full media/call detection | Audio PID detection is enough for MVP. | Users report video/call regressions not covered by audio. |

## Manual Measurement Before Claiming Savings

Use a small manual baseline before marketing or release notes claim power savings:

| Scenario | Compare | Pass condition |
| --- | --- | --- |
| Idle desktop on battery | disabled vs Adaptive Engine | Lower discharge trend, no app breakage. |
| Background CPU load | disabled vs Adaptive Engine | Foreground feels same or better, background CPU/power lower. |
| Browser with media tab | disabled vs Adaptive Engine | No audio stutter, active tab normal. |
| Game/fullscreen foreground | disabled vs Adaptive Engine | Background apps restrained, foreground unaffected. |

Use Windows battery report, Task Manager process CPU, Action Log, and observed foreground responsiveness first.
Use `scripts/power_drain_benchmark.ps1` for repeated package-watt readings; do not claim savings when it reports `stable = false`.

## Review

### Findings

1. High: Applying `IGNORE_TIMER_RESOLUTION` without an active-audio guard can regress calls, media playback, and low-latency tools.
   Mitigation: extract shared active-audio PID detection first; if detection fails, skip only the timer flag.

2. Medium: The old plan duplicated existing engine concepts and would create parallel policy ownership.
   Mitigation: keep Adaptive Engine inside Background Efficiency and Workload Engine.

3. Medium: Power savings are not proven by code changes alone.
   Mitigation: require stable baseline measurements before release claims; use package wattage as the release metric for repeatable local checks. Core, L3, and APU/STAPM rails can improve while package power still regresses if Winderust keeps package-level wake sources active.

4. Low: `ProcessPowerThrottling` helper logic is still local to EcoQoS and Workload Engine.
   Mitigation: keep the first diff local and test the bitmask builders; refactor only if another module needs the same policy.

### Review Result

The revised MVP fits the current architecture and has been implemented without adding a new engine.
The Windows binding for `PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION` is available and covered by compile/test verification.

## Final MVP

```text
Adaptive Engine =
  temporary active-plan processor Saver values
  + Winderust self EcoQoS
  + Winderust self ignore-timer-resolution
  + optional existing EcoQoS
  + optional existing safe priority lowering
  + optional existing foreground rollback
  + optional existing exclusions
```

Optional: existing Background Efficiency and Workload Engine CPU-pressure escalation.

Skipped: new engine, Job caps, thread throttling. Add when the measured MVP is not enough.
