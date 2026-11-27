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
when the window returns. Adaptive Engine policies and power-plan rules keep their own sampling schedules.

## Presets

Setup contains the enable switch, CPU preset, and expandable controls for CPU
triggers, app recovery, and the individual policies. Enabling a power source with
factory tuning stages Balanced. Choose a preset to replace the selected source's
tuning, then Save to apply. Exclusions and Interrupt Timing have their own tabs.

The preset definitions live in `src/ui/adaptive_presets.rs`.

Power Save, Balanced, Performance, and Speed select scheduling values. Custom
presets capture tuning, including the three policy switches, both allocation
configurations, and Exclude service hosts. Applying a preset leaves Adaptive
Processor Power, the master switch, and per-app exclusions under their own
controls. Preset matching also uses those tuning values. Older nested Priority
Control fields retain their settings-file representation and have no effect on
preset selection or application. Existing custom preset records retain their
stored power values when tuning is edited.

| Preset | Pressure source and threshold | Sample interval | Minimum hot duration | CPU policy |
| --- | --- | ---: | ---: | --- |
| Power Save | System only, 85% | 2.5 s | 2.5 s | Temporary Below Normal and energy-efficient scheduling, up to 4 apps |
| Balanced | System only, 75% | 1.5 s | 1.5 s | Temporary Below Normal with unrestricted placement, up to 4 apps |
| Performance | System or foreground, 60% | 750 ms | First hot sample | 75% foreground zone and an independent 90% background placement fallback, up to 8 candidates |
| Speed | System only, 35% | 500 ms | 500 ms | Temporary Below Normal and an 85% background placement limit during verified restraint, up to 12 apps |

Power Save uses the longest sampling interval and lets Windows favor efficient
frequencies and cores for restrained background apps. Slower background completion
is an intended tradeoff. Balanced uses temporary priority restraint to preserve
responsiveness and keep the full processor pool available. Performance favors a
busy foreground app under contention. Speed reacts sooner and keeps the full
processor pool available to the foreground app, with energy-efficient scheduling
off. Both placement presets use CPU Sets (Soft). The background placement
percentages refer to the pool available to each eligible app.

Performance turns CPU Pressure Restraint off and uses the first valid hot CPU
interval for independent placement.
Power Save and Balanced qualify background apps at 10% of one logical processor.
Performance and Speed use 8%. App recovery thresholds are 5%, 5%, 4%, and 7%
respectively. Power Save and Balanced require three seconds of cool samples
after the minimum hold. Speed requires four. Performance uses the recovery
threshold to release independent placement.
Idle priority, Very Low I/O priority, and pause on idle are available in custom
tuning. Continuous priority and Efficiency Mode rules have separate controls.

Custom edits remain in the modal draft until saved. Built-ins are read-only.
Disabled policies remain editable.

Exclusions can match an exact executable path, an exact executable name, or
an executable name pattern. Patterns use `*` for any number of characters and
`?` for one. Both name selectors match case-insensitively across directories.
Process Details shows name coverage and directs users to Exclusions to edit it.
Exclude service hosts is also on this tab and stays available when editing a preset.

Setup places the shared CPU triggers and recovery card above three independent policies.

- **CPU Pressure Restraint** acts on apps with sustained CPU demand during aggregate
  CPU pressure. Its action selects Below Normal priority, Idle priority, or
  Processor placement only. Placement only uses the method and selection in Limit
  Background Processors for each verified restraint episode.
- **Limit Background Processors** restricts background candidates only, using its own selector,
  percentage/custom mask, and soft/hard method. Its Reduce affinity (Hard) method removes
  a configurable number of logical processors from each process's original mask. It never
  narrows foreground access.
  Only during CPU Pressure Restraint restricts the limiter to apps with verified
  restraint. Each app's limit ends when its restraint ends or loses verification.
  The option requires CPU Pressure Restraint and defaults off.
  Processor placement only selects this scope automatically and preserves the stored
  standalone limiter preferences for switching back.
  Restore child process affinity is off by default. With a hard affinity method,
  Adaptive Engine gives eligible direct children the parent's captured original mask
  when a later process observation confirms that they inherited the narrowed mask.
  The parent keeps its own placement. Explicit placement and external CPU Sets take
  precedence. A child that independently sets the same mask is indistinguishable from
  one that inherited it, so enable this only for workloads where that behavior fits.
  Restore affinity before restart optionally releases the exact parent’s Adaptive Engine
  hard-affinity assignment before Resource Thresholds creates a replacement. The replacement
  can inherit the parent’s restored affinity. This applies to Winderust-managed restarts.
- **Dynamic Resource Zones (Experimental)** uses separate settings and complementary CPU Sets (Soft).
  The Performance preset enables zoning. Speed, Balanced, and Power Save leave it off.

CPU triggers can use total system CPU, foreground-group CPU, either reading, or
both readings. Either and Both can use a separate foreground CPU threshold. Each selected signal
has a release boundary five percentage points below its own threshold. The foreground-group
reading is a share of total system CPU capacity. Background-app CPU demand is measured against
one logical processor.
With System only selected, a known load below the trigger can skip the foreground
process scan while pressure is inactive. Zoning, allocation recovery, and the
first pass after pressure falls keep their normal observation paths. Adaptive
Processor Power still collects foreground demand when its own settings need it.
Optional Exclude service hosts protects detected active service processes, Session 0,
and processes whose session could not be determined. It applies to all three policies.
App recovery applies to CPU Pressure Restraint and independent background
processor limits. Minimum restraint time is entered in milliseconds, with zero
allowing recovery without a minimum hold. The hold applies during continued
aggregate pressure and while the process is eligible. The lower of the recovery
threshold and the app CPU trigger defines the cutoff. Usage at or above the app
trigger still counts as hot. CPU recovery time measures how long usage must
remain low before CPU Pressure Restraint ends. Independent placement releases
when recovery begins, and Adaptive Engine continues tracking the cooling app
for the configured duration. CPU Pressure Restraint also uses Busy time before
restraint, Maximum restraint time, and the optional idle pause.

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
- Adaptive Engine checks CPU pressure at the configured CPU sample interval whenever
  any CPU policy is enabled. It checks foreground identity and samples CPU use before scanning
  for background candidates under pressure. Quiet passes still reconcile existing claims.
  Foreground and process lifecycle events may schedule an immediate safety pass to release
  obsolete foreground roles.
  Restraint requires at least two valid hot observations for the same process
  instance and the configured Busy time before restraint. CPU sample interval
  controls sampling. The default for both is one second. Restraint submits a temporary
  priority claim for a process at Normal priority by default. Below Normal is the
  default action and Idle is optional. Turning off Protect non-Normal priority
  allows lowering a known priority above the selected action and restores the
  captured priority at release. Realtime and unknown priorities remain protected.
  Processor placement only starts a hold once the shared CPU coordinator confirms the
  selected placement for that exact process. Explicit allocation and Dynamic Resource Zones
  retain precedence. Placement alone leaves CPU and I/O priority, scheduling hints and
  the restraint parking floor untouched.
  Windows can pass Idle priority to children launched while the parent is restrained.
  Protect focused app and Protect focused app’s children are on by default and
  can be changed separately for CPU Pressure Restraint. Independent background
  placement and zoning keep their existing focus eligibility. Other visible app
  groups, active audio, and exclusions remain protected. Saved Process Priority
  and Efficiency Mode take precedence.
- Pause when idle is off by default. When enabled, its Idle timeout uses the
  last input in Winderust's Windows session. After the timeout or when the idle
  observation is unavailable, release restraint claims and start a fresh
  qualification interval on resumption. The placement policies keep their own
  eligibility.
- Each policy ranks its own eligible candidates within the configured limit.
  Priority restoration follows the app's minimum hold and recovery settings
  during continued aggregate pressure. A higher-ranked candidate can take an
  occupied slot. Loss of aggregate pressure, a protected role, or unavailable
  required observations withdraw the priority claim. The background processor
  limit yields when its candidate enters recovery.
- Maximum restraint time limits a continuous restraint even if its process remains hot.
  It is entered in milliseconds and checked at the next evaluation. Zero leaves the adjustment
  uncapped. A positive maximum must be at least the minimum. After expiration, the process must
  build fresh qualification before being restrained again. Pending restoration retains its existing
  retry path.
- Restraint I/O priority can apply Low or Very Low only while an exact process has a verified CPU
  priority restraint. The default is Unchanged. The shared I/O Priority controller preserves a
  lower baseline and yields to saved I/O policies and manual changes. It releases the Adaptive
  adjustment when restraint ends or the option changes to Unchanged.
- Energy-efficient scheduling is an independent, default-off option during verified CPU Pressure
  Restraint. Windows can use the hint when choosing cores and processor frequencies. The shared
  priority and efficiency controller captures and restores the original power throttling state.
  Saved Efficiency Mode, manual settings, and explicit Windows execution-speed settings take
  precedence.
- Adaptive Processor Power can raise the minimum unparked percentage while CPU Pressure
  Restraint has a verified process. The temporary Winderust Adaptive plan has separate
  floors for Windows processor efficiency classes 0 and 1 on each power source. The
  class 1 floor requires matching processors in the Windows topology. A value of 0% uses
  the normal policy for that class. Both floors are withdrawn when the last verified
  restraint ends or the current power source becomes unknown.
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
  at its own 500 ms cadence, including when all Adaptive Engine CPU policies are off. It reuses the
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

The runtime benchmark generates canonical Balanced settings with CPU Pressure
Restraint enabled. See the
[benchmark guide](adaptive-engine-benchmark.md#benchmark-command) for the opt-in
host build, configuration export, activation checks, and runtime comparisons.
Its excluded-host I/O and message-loop scenarios measure latency under load;
they do not validate real foreground detection.
Unit/fake-backend tests establish safety and configuration behavior, not performance gains;
foreground-only and mixed-workload A/B measurements remain necessary on the target hardware.
