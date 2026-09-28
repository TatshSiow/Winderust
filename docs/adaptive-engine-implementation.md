# Adaptive Engine Implementation

Adaptive Engine combines processor power policy, Background Efficiency, and
workload-aware CPU scheduling. It extends existing managers instead of creating
a second automation engine.

## Components

- `src/ui/adaptive_presets.rs` defines the visible Adaptive Engine presets and Adaptive Engine
  preset values.
- `src/backend/automation/runner.rs` collects demand signals, selects the active processor-power
  profile, and coordinates feature policies and controllers.
- `src/features/winderust_features/adaptive_engine_process.rs` implements the internal
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

The preset definitions live in `src/ui/adaptive_presets.rs` and combine processor values
with internal scheduling presets:

| Preset | Processor policy | Scheduling preset |
| --- | --- | --- |
| Power Save - Dynamic | Maximum 45%, boost disabled | Low Impact |
| Balanced - Dynamic | Maximum 95%, efficient boost | Low Impact |
| Performance - Fixed | Minimum 25%, maximum 100%, efficient aggressive boost | Foreground First |
| Speed - Fixed maximum | Minimum 25%, maximum 100%, aggressive boost | Max Foreground |

Custom presets capture Adaptive Engine tuning, including the three CPU Behaviour switches and
both independent allocation configurations. Application/Adaptive master switches, exclusions,
and the separate Background Efficiency feature stay independently owned. Live and preset views
share CPU Behaviour, Processor Power, and Priority Control controls; custom edits remain in the
modal draft until saved. Built-ins are read-only. Disabled policies remain editable.

CPU Behaviour has shared detection/timing followed by three independent policies:

- **CPU Pressure Restraint** adjusts eligible priority, efficiency, and memory policy under pressure.
- **Limit Background Processors** restricts background candidates only, using its own selector,
  percentage/custom mask, and soft/hard method. It never narrows foreground access.
- **Dynamic Resource Zones (Experimental)** uses separate settings and complementary CPU Sets (Soft).
  Speed leaves zoning off; Performance retains explicit opt-in. Balanced and Power Save leave it off.

Shared foreground/system pressure uses the existing aggregate metric. Background-app CPU demand
is normalized against one logical processor; the thresholds are not directly interchangeable.
Reaction time, candidate cap, restraint hold, and recovery values retain their existing units.

Zoning requires fresh foreground observations outside startup grace and current hot eligible
background competition. Excluded, protected, unavailable, overridden, and suppressed targets do
not establish competition. Background placement must succeed before foreground narrowing.
When competition disappears, foreground zoning is withdrawn on the next reconciliation even
if foreground pressure remains high. Background-only limiting may independently remain eligible.

One Adaptive allocation generation owns the combined target set. Zones take precedence when
eligible; otherwise the limiter's saved method/mask is the fallback. Partial foreground failure
withdraws the zone plan, retaining coordinator cleanup retries and bounded allocation backoff.
Explicit CPU Sets (Soft) and Processor Affinity (Hard) still outrank Adaptive Engine. Handoffs use
the same captured baselines and never clear thread-selected sets or force wider external affinity.

Zoning requires a known single-group topology and two nonempty masks. For Least Used (All), the
background count is `ceil(N * (100 - foreground_share_percent) / 100)`; the foreground gets its
complement. P/E strategies apply the share within their candidate pool, so realized totals can
differ. Missing pools and invalid custom selections do not fall back to All. CPU Sets control
placement, not exclusive physical-core reservation, and can reduce foreground multi-thread throughput.
The status rail reports eligibility, effective foreground/background counts, and incomplete recovery.

### Independent zone settings

`processor_limit_percent`, `background_processor_selection`, `specific_processors`, and
`cpu_allocation_method` belong only to the background limiter. Zoning uses:

```toml
[adaptive_engine_process.dynamic_resource_zone_settings]
foreground_share_percent = 75
background_processor_selection = "least_used"
specific_processors = []
```

The share accepts 1?99. Existing files with zoning disabled may omit this block; defaults are
created in memory without rewriting the file. Enabled zoning requires the explicit block in
root, Battery, and saved presets. A load error preserves the original file and blocks automatic
saving. To correct an older enabled configuration manually, either disable zoning or add the
block with the intended previous foreground share and background selection. No automatic
migration or copying from the limiter is performed.

## Runtime Behavior

- Every preset with processor policy enabled uses a temporary managed `Winderust Adaptive` plan.
  The selected preset supplies the processor baseline, while current CPU, foreground, launch, and
  I/O demand selects Idle, Responsive, Background Pressure, or Focus and Launch values. Demand can raise the preset
  immediately; de-escalation uses a short hysteresis delay. Background-dominant CPU pressure uses
  Background Pressure, while app launches and genuinely heavy Focus Process demand use Focus and Launch. The A/C and Battery
  boost policy/mode values for both contexts are editable and stored with the Adaptive preset.
- Winderust applies EcoQoS to itself while the low-power Adaptive Engine path is active.
- Background Efficiency and Adaptive Engine policy submit typed claims through RuntimeCore. Each
  controller revalidates exact identity, access, protection, and cross-session policy before a
  mutation.
- Adaptive Engine checks CPU pressure at the configured reaction interval whenever any CPU
  Behaviour policy is enabled. Foreground and process
  lifecycle events may schedule an immediate safety pass. Obsolete foreground roles are released;
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
- Processor-power demand keeps its independent 500 ms sampling deadline; changing Adaptive Engine
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

The fixed-policy runtime benchmark keeps zoning off by default. Use
`scripts/adaptive_runtime_benchmark.ps1 -EnableDynamicResourceZones` for the separate opt-in
scenario (`-SettingsOnly` validates/generates its configuration without starting workloads).
Unit/fake-backend tests establish safety and configuration behavior, not performance gains;
foreground-only and mixed-workload A/B measurements remain necessary on the target hardware.
