# Adaptive Engine Implementation

Adaptive Engine combines processor power policy, Background Efficiency, and
workload-aware CPU scheduling. It extends existing managers instead of creating
a second automation engine.

## Components

- `src/ui/app/shared/presets.rs` defines the visible Adaptive Engine presets and CPU Scheduler
  preset values.
- `src/backend/automation/runner.rs` collects demand signals, selects the active processor-power
  profile, and coordinates feature policies and controllers.
- `src/features/winderust_features/cpu_scheduler.rs` implements the internal
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

## Presets

The preset definitions live in `src/ui/app/shared/presets.rs` and combine processor values
with internal scheduling presets:

| Preset | Processor policy | Scheduling preset |
| --- | --- | --- |
| Power Save - Dynamic | Maximum 45%, boost disabled | Low Impact |
| Balanced - Dynamic | Maximum 95%, efficient boost | Low Impact |
| Performance - Fixed | Minimum 25%, maximum 100%, efficient aggressive boost | Foreground First |
| Speed - Fixed maximum | Minimum 25%, maximum 100%, aggressive boost | Max Foreground |

Changing tuning values makes the selected preset `Custom`. Custom presets capture Adaptive
Engine and CPU Scheduler tuning only; enable switches, custom rules, exclusions, and the separate
Background Efficiency feature remain independently owned.
Adaptive Engine and preset details render the same CPU Behaviour, Processor Power, and Priority
Control tuning tabs, so every preset-owned control has one UI implementation. Processor Power uses
separate setting cards; disabling its policy dims and blocks the value cards while preserving the
policy switch. The live page adds a Custom Rules tab for CPU Scheduler exclusions. CPU Scheduler
has no separate master switch: CPU Pressure Restraint and Limit Background Processors are the two
independent CPU Behaviour controls. Priority Control uses one Focus / Visible Window /
Background table with the safe automatic subset of the main Process Priority choices. Adaptive
Background Efficiency is a row in the same table; `Default` makes Focus or Visible Window inherit
the Background value. Memory Priority supports `Default` in all three tiers, which submits no
Adaptive Memory Priority claim for that tier. While CPU Pressure Restraint is enabled and pressure
is active, the configured priority, efficiency, and memory values apply across eligible Visible
Window and Background processes. Limit Background Processors can run without CPU Pressure
Restraint and applies only to selected Background candidates after they cross the configurable
per-app CPU threshold. Both use the shared reaction and recovery timing. Processor selection can
use the least-used logical processors across the All, P-core, or E-core pool, topology-derived
P/E/no-SMT masks, or an exact custom mask; Visible Window candidates keep the softer priority and
efficiency controls. Preset details remain editable without changing operational switches, and
built-in presets remain read-only.

Disabled CPU Pressure Restraint and Limit Background Processors groups keep their switches active
while dimming and blocking their own setting rows. Priority Control follows the same pattern per
row: its switch remains active and its Focus Process, Visible Window, and Background dropdowns are
disabled. Preset-only CPU Pressure tuning remains editable because presets do not own that runtime
switch.

## Runtime Behavior

- Every preset with processor policy enabled uses a temporary managed `Winderust Adaptive` plan.
  The selected preset supplies the processor baseline, while current CPU, foreground, launch, and
  I/O demand selects Idle, Responsive, Background Pressure, or Focus and Launch values. Demand can raise the preset
  immediately; de-escalation uses a short hysteresis delay. Background-dominant CPU pressure uses
  Background Pressure, while app launches and genuinely heavy Focus Process demand use Focus and Launch. The A/C and Battery
  boost policy/mode values for both contexts are editable and stored with the Adaptive preset.
- Winderust applies EcoQoS to itself while the low-power Adaptive Engine path is active.
- Background Efficiency and CPU Scheduler policy submit typed claims through RuntimeCore. Each
  controller revalidates exact identity, access, protection, and cross-session policy before a
  mutation.
- CPU Scheduler checks CPU pressure at the configured reaction interval whenever either CPU
  Behaviour control is enabled. Foreground and process
  lifecycle events may schedule an immediate safety pass. Focus processes are released immediately;
  eligible Visible Window and Background processes receive their configured soft pressure policy
  only when CPU Pressure Restraint is enabled. Hot Background candidates are independently ranked
  by current CPU demand with a small selection-stability bias for processor allocation.
- When a selected process falls below its recovery threshold, Winderust removes processor
  allocation first, keeps the softer restraint during the recovery period, and then restores the
  original state. Renewed CPU demand cancels recovery.
- Least-used selection samples its configured All, P-core, or E-core pool and changes the selected mask only after
  the existing rebalance interval or a meaningful load improvement. Fixed topology and custom
  selections do not rebalance.
- Existing throttling, priority, CPU Set, affinity, and power-plan state is restored when its
  claim is released or Winderust shuts down. The watchdog independently restores committed
  externally persistent state after an abnormal exit.
- Processor-power demand keeps its independent 500 ms sampling deadline; changing CPU Scheduler
  reaction time does not change Processor Power behavior.

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
