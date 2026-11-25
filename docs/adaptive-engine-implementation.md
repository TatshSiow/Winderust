# Adaptive Engine Implementation

Adaptive Engine lives under CPU Control and provides CPU Pressure Restraint and optional processor placement.
Adaptive Processor Power is configured under Power Plan Control. Both features
use the shared automation runtime and foreground demand observations.

## Components

- `src/ui/adaptive_presets.rs` defines the visible Adaptive Engine presets and Adaptive Engine
  preset values.
- `src/ui/adaptive_processor_power/presets.rs` defines the built-in Adaptive Processor Power presets.
- `src/backend/automation/runner.rs` collects demand signals, selects the active processor-power
  profile, and coordinates feature policies and controllers.
- `src/features/cpu_control/adaptive_engine_process.rs` implements pressure
  detection, temporary background priority restraint, and placement policy.
- `src/control/priority_efficiency.rs`, `src/control/cpu_allocation.rs`, and
  `src/control/power_plan.rs` own live mutations, baselines, compensation, and restoration.
- `src/power/powercfg.rs` provides typed power-plan operations; raw power APIs remain in
  `src/platform/windows/power_plan.rs`.
- `src/backend/self_power.rs` manages Winderust's own priority and Power Throttling state, while
  `src/backend/audio_activity.rs` provides the shared active-audio safety guard.
- `src/backend/automation.rs` owns worker scheduling, status publication, and shutdown.

The System Bottleneck rail is diagnostic. It uses total CPU, busiest logical processor, and
busiest GPU engine readings. The diagnostic pauses in the tray and starts a fresh observation
when the window returns. CPU Behaviour and power-plan rules keep their own sampling schedules.

## Presets

The preset definitions live in `src/ui/adaptive_presets.rs` and apply CPU Behaviour settings.

Power Save, Balanced, Performance, and Speed select scheduling values. Custom
presets capture CPU Behaviour tuning, including its three switches and both
allocation configurations. Applying a preset leaves Adaptive Processor Power,
the master switch, and custom rules under their own controls. Existing custom
preset records retain their stored power values when CPU Behaviour tuning is edited.

Custom edits remain in the modal draft until saved. Built-ins are read-only.
Disabled policies remain editable.

CPU Behaviour has shared detection/timing followed by three independent policies:

- **CPU Pressure Restraint** temporarily lowers the priority of background apps
  with sustained CPU demand during aggregate CPU pressure.
- **Limit Background Processors** restricts background candidates only, using its own selector,
  percentage/custom mask, and soft/hard method. Its Reduce affinity (Hard) method removes
  a configurable number of logical processors from each process's original mask. It never
  narrows foreground access.
- **Dynamic Resource Zones (Experimental)** uses separate settings and complementary CPU Sets (Soft).
  The Performance preset enables zoning; Speed, Balanced, and Power Save leave it off.

Shared Detection and Timing can use total system CPU, foreground-group CPU, either reading, or
both readings. The threshold and release band use the same selected source. Background-app CPU
demand is normalized against one logical processor; the thresholds are not directly interchangeable.
Optional Exclude service hosts protects detected active service processes, Session 0,
and processes whose session could not be determined. It applies to all three policies.
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
specific_processors = { processors = [] }
```

The share accepts 1?99. Existing files with zoning disabled may omit this block; defaults are
created in memory without rewriting the file. Enabled zoning requires the explicit block in
root, Battery, and saved presets. A load error preserves the original file and blocks automatic
saving. To correct an older enabled configuration manually, either disable zoning or add the
block with the intended previous foreground share and background selection. No automatic
migration or copying from the limiter is performed.

## Runtime Behavior

- Adaptive Processor Power uses a temporary managed `Winderust Adaptive` plan when enabled for
  the current source. Its saved Base Processor Policy supplies the processor baseline. Current
  CPU, foreground, launch, and I/O demand select Idle, Responsive, Background Pressure, or Focus
  and Launch. Transitions to lower-demand profiles use the existing delay.
- The Power Save, Balanced, and Performance presets replace the selected source's Base Processor
  Policy and reset its Background Pressure and Focus and Launch boost settings to the profile
  defaults. Balanced sets Processor max to 100% when plugged in and 95% on battery. Custom presets
  capture those same tuning values from the selected source and can be applied to either source.
  Applying a preset leaves the enable switches and the other source's settings under their own
  controls.
- Winderust applies EcoQoS to itself while the low-power Adaptive Engine path is active.
- Efficiency Mode and Adaptive Engine policy submit typed claims through RuntimeCore. Each
  controller revalidates exact identity, access, protection, and cross-session policy before a
  mutation.
- Adaptive Engine checks CPU pressure at the configured reaction interval whenever any CPU
  Behaviour policy is enabled. It checks foreground identity and samples CPU use before scanning
  for background candidates under pressure. Quiet passes still reconcile existing claims.
  Foreground and process lifecycle events may schedule an immediate safety pass to release
  obsolete foreground roles.
  Restraint requires at least two valid hot observations for the same process
  instance and a continuous Time above threshold. Reaction time controls
  sampling. The default for both is one second. Restraint submits a temporary
  priority claim for a process at Normal priority. Below Normal is the default and Idle is optional.
  Windows can pass Idle priority to children launched while the parent is restrained.
  Focused apps and their process groups,
  apps with visible windows, active audio, and exclusions are protected. Saved
  Process Priority and Efficiency Mode take precedence.
- Pause when idle is off by default. When enabled, its Idle timeout uses the
  last input in Winderust's Windows session. After the timeout or when the idle
  observation is unavailable, restore restrained priorities and start a fresh
  qualification interval on resumption. The placement policies keep their own
  eligibility.
- Each policy ranks its own eligible candidates within the configured limit.
  Priority restoration follows the app's minimum hold and recovery settings
  during continued aggregate pressure. A higher-ranked candidate can take an
  occupied slot. Loss of aggregate pressure, a protected role, or unavailable
  required observations withdraw the priority claim. The background processor
  limit yields when its candidate enters recovery.
- Maximum restraint time limits a continuous priority adjustment even if its process remains hot.
  Zero leaves the adjustment uncapped. After expiration, the process must build fresh qualification
  before being restrained again. Pending restoration retains its existing retry path.
- Restraint I/O priority can apply Low or Very Low only while an exact process has a verified CPU
  priority restraint. The default is Unchanged. The shared I/O Priority controller preserves a
  lower baseline and yields to saved I/O policies and manual changes. It releases the Adaptive
  adjustment when restraint ends or the option changes to Unchanged.
- Energy-efficient scheduling is an independent, default-off option during verified CPU Pressure
  Restraint. Windows can use the hint when choosing cores and processor frequencies. The shared
  priority and efficiency controller captures and restores the original power throttling state.
  Saved Efficiency Mode, manual settings, and explicit Windows execution-speed settings take
  precedence.
- Missing per-process samples or invalid CPU deltas discard that target's old
  baseline and restraint/recovery episode. Re-entry requires a fresh valid interval;
  skipped/excluded targets cannot retain stale candidate tracking.
- Adaptive Engine resets its pressure observations on a delivered Windows suspend/resume event.
- Least-used selection samples its configured All, P-core, or E-core pool and changes the selected mask only after
  the existing rebalance interval or a meaningful load improvement. Fixed topology and custom
  selections do not rebalance. Least-used selection requires a fresh load interval after sampling
  pauses.
- Reduce affinity (Hard) requires one active processor group and uses a stable per-process choice
  from its captured original mask. It leaves at least one logical processor and respects custom
  default CPU Sets. Both hard methods can also preserve an already narrowed affinity. The
  coordinator restores the original mask and does not overwrite an external change during release.
- Exclude service hosts reads the local service inventory only when a policy is about
  to act. If Windows cannot read that inventory, Adaptive Engine releases its claims and pauses.
  Windows can omit services whose status the caller lacks permission to read.
- Existing priority, CPU Set, affinity, and power-plan state is restored when its
  claim is released or Winderust shuts down. The watchdog independently restores committed
  externally persistent state after an abnormal exit.
- Adaptive Processor Power samples foreground workload, aggregate CPU use and per-processor load
  at its own 500 ms cadence, including when all CPU Behaviour policies are off. It reuses the
  foreground process generations and counters when both consumers run in the same cycle.
  A known foreground workload below 25% and system usage at or above 75% select Background
  Pressure outside Focus and Launch. CPU Pressure Restraint tuning controls priority claims.

## Ownership and Recovery

`PowerPlanController` owns the temporary plan lifecycle and clean restoration. Only a plan whose
name and description identify the current `Winderust Adaptive` plan may be recovered after an
abnormal exit. Other power plans remain owned by Windows or the user.

Power-plan automation remains separate: By Activity owns its Idle and Active
selections, while By Focus, By Running App, By CPU Load, and By Time rules
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

The runtime benchmark generates canonical Balanced settings, with zoning and
CPU Pressure Restraint off unless explicitly requested. See the
[benchmark guide](adaptive-engine-benchmark.md#benchmark-command) for the opt-in
host build, configuration export, activation checks, and runtime comparisons.
Its excluded-host I/O and message-loop scenarios measure latency under load;
they do not validate real foreground detection.
Unit/fake-backend tests establish safety and configuration behavior, not performance gains;
foreground-only and mixed-workload A/B measurements remain necessary on the target hardware.
