# Adaptive Engine Implementation

Adaptive Engine combines processor power policy, Background Efficiency, and
workload-aware CPU scheduling. It extends existing managers instead of creating
a second automation engine.

## Components

- `src/ui/app/shared/presets.rs` defines the visible Adaptive Engine profiles and Workload Engine
  preset values.
- `src/backend/automation/runner.rs` collects demand signals, selects the active processor-power
  profile, and coordinates feature policies and controllers.
- `src/features/winderust_features/workload_engine.rs` implements the internal
  CPU-pressure, target-selection, hysteresis, and priority-assist policy.
- `src/features/winderust_features/background_efficiency.rs` selects eligible targets and submits
  Background Efficiency claims.
- `src/control/priority_efficiency.rs`, `src/control/cpu_allocation.rs`, and
  `src/control/power_plan.rs` own live mutations, baselines, compensation, and restoration.
- `src/power/powercfg.rs` provides typed power-plan operations; raw power APIs remain in
  `src/platform/windows/power_plan.rs`.
- `src/backend/self_power.rs` manages Winderust's own priority and Power Throttling state, while
  `src/backend/audio_activity.rs` provides the shared active-audio safety guard.
- `src/backend/automation.rs` owns worker scheduling, status publication, and shutdown.

## Operating Profiles

The profile definitions live in `src/ui/app/shared/presets.rs` and combine processor values
with internal scheduling presets:

| Profile | Processor policy | Scheduling preset |
| --- | --- | --- |
| Power Save - Dynamic | Maximum 45%, boost disabled | Low Impact with Background Efficiency |
| Balanced - Dynamic | Maximum 95%, efficient boost | Low Impact |
| Performance - Fixed | Minimum 25%, maximum 100%, efficient aggressive boost | Foreground First |
| Speed - Fixed maximum | Minimum 25%, maximum 100%, aggressive boost | Max Foreground |

Changing advanced values makes the selected profile `Custom`.

## Runtime Behavior

- Every profile with processor policy enabled uses a temporary managed `Winderust Adaptive` plan.
  The selected preset supplies the processor baseline, while current CPU, foreground, launch, and
  I/O demand selects Idle, Responsive, Sustained, or Burst values. Demand can raise the profile
  immediately; de-escalation uses a short hysteresis delay.
- Winderust applies EcoQoS and
  `PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION` to itself while the low-power
  Adaptive Engine path is active.
- Background Efficiency and Workload Engine policy submit typed claims through RuntimeCore. Each
  controller revalidates exact identity, access, protection, and cross-session policy before a
  mutation.
- Ignore-timer-resolution is skipped when active-audio detection fails or the
  target owns an active audio session. Other eligible restraint may continue.
- Existing throttling, priority, CPU Set, affinity, and power-plan state is restored when its
  claim is released or Winderust shuts down. The watchdog independently restores committed
  externally persistent state after an abnormal exit.
- Idle maintenance and appearance polling use slower scheduler deadlines than active
  processor-demand sampling.

## Ownership and Recovery

`PowerPlanController` owns the temporary plan lifecycle and clean restoration. Only a plan whose
name and description identify the current `Winderust Adaptive` plan may be recovered after an
abnormal exit. Other power plans remain owned by Windows or the user.

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
