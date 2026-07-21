# Adaptive Engine Implementation

Adaptive Engine combines processor power policy, Background Efficiency, and
workload-aware CPU scheduling. It extends existing managers instead of creating
a second automation engine.

## Components

- `src/backend/self_power.rs` manages Winderust's own EcoQoS and timer behavior.
- `src/backend/audio_activity.rs` provides the shared active-audio safety guard.
- `src/features/winderust_features/background_efficiency.rs` applies and restores
  background EcoQoS and process priority.
- `src/features/winderust_features/workload_engine.rs` implements the internal
  CPU-pressure scheduler, priority assists, and optional CPU Set escalation.
- `src/power/powercfg.rs` creates, applies, and restores the temporary
  `Winderust Adaptive` power plan.
- `src/backend/automation.rs` owns scheduling, status fan-out, and cleanup.

## Operating Profiles

The profile definitions live in `src/ui/app.rs` and combine processor values
with internal scheduling presets:

| Profile | Processor policy | Scheduling preset |
| --- | --- | --- |
| Power Save | Dynamic, maximum 45%, boost disabled | Low Impact with Background Efficiency |
| Balanced | Dynamic, maximum 95%, efficient boost | Low Impact |
| Performance | Fixed high-performance targets | Foreground First |
| Speed | Fixed maximum-performance targets | Max Foreground |

Changing advanced values makes the selected profile `Custom`.

## Runtime Behavior

- Power Save and Balanced create and activate a temporary managed power plan.
  Performance and Speed temporarily apply fixed processor targets to the active
  plan. Both paths restore the prior state when Adaptive Engine releases it.
- Winderust applies EcoQoS and
  `PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION` to itself while the low-power
  Adaptive Engine path is active.
- Background Efficiency and the internal Workload Engine preserve foreground,
  protected, excluded, and current-session safety filters.
- Ignore-timer-resolution is skipped when active-audio detection fails or the
  target owns an active audio session. Other eligible restraint may continue.
- Existing throttling, priority, CPU Set, affinity, and power-plan state is
  restored when a target stops matching, automation is disabled, Winderust
  exits, or the relevant manager is dropped.
- UI maintenance and eligible appearance/background polling use a slower
  60-second cadence. Active processor-demand sampling remains fast.

## Ownership and Recovery

Only a temporary plan whose name and description identify the current
`Winderust Adaptive` plan may be recovered. Other power plans remain owned by
Windows or the user.

Power-plan automation remains separate: By Activity owns its Idle and Active
selections, while By Foreground, By Running App, By CPU Load, and By Time rules
own their selected plan GUIDs. There is no global plan fallback.

## Validation

Run the standard Rust checks and `graphify update .` after implementation
changes. Use [`adaptive-engine-benchmark.md`](adaptive-engine-benchmark.md) for
synthetic scheduler methodology and `scripts/power_drain_benchmark.ps1` for
local package-power measurements. Benchmark results are directional and must
not be presented as universal battery-life claims.

## Deferred

- Thread-level power throttling remains out of scope without a safe owned-thread
  classifier.
- Job Object CPU caps remain out of scope because of compatibility and nested-job
  risks.
- Broader media classification should be added only if active-audio protection
  proves insufficient in real usage.
