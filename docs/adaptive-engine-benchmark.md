# Adaptive Engine Benchmark Guide

This guide documents the real-runtime and synthetic benchmarks used to tune
Adaptive Engine presets and CPU Scheduler scheduling presets.
Synthetic results isolate mechanisms; release-binary runtime A/B results are
the primary acceptance evidence.

## What This Measures

The default benchmark measures foreground CPU work completion time while
temporary background CPU workers compete for scheduler time. Lower milliseconds
are better.

Benchmark workers use a temporary hidden `cscript.exe` workload distinct from
the visible benchmark shell. They are created through the local WMI process
provider so they are not descendants of that foreground shell; otherwise
Winderust's foreground process-group protection would correctly protect the
generated load. The runner also clears inherited Power Throttling before each
measurement. Runtime results are rejected unless Windows reports an actual
priority change on one of those workers.

The optional `IoLoop` foreground scenario measures foreground temp-file
read/write completion time and reports foreground IOPS under the same generated
background CPU load. It is useful for checking I/O priority direction, not for
rating storage hardware.

The optional `MessageLoop` foreground scenario measures hidden WinForms timer
delay under the same generated background load. It is useful for checking
foreground UI/message-pump latency, not raw CPU throughput.

The optional `WinderustLaunch` foreground scenario measures launching this app
under the same generated background load. It refuses to run while Winderust is
already open, then closes only the instance it started for each sample. This is
the normal-user launch proxy when checking app startup behavior.

The power-drain companion benchmark measures CPU package power directly from
Windows performance counters. Lower median watts are better. It prefers the
RAPL package counter, for example:

```text
\Energy Meter(RAPL_Package0_PKG)\Power
```

On common Windows Energy Meter providers the raw counter is milliwatts, so the
script scales it to watts before summarizing.

The default benchmark also runs a compact foreground score suite while the same
generated background workers are active. The score suite covers managed integer
and double arithmetic, an unrolled float-batch instruction proxy, GZip and
Deflate round trips, SHA-256 hashing, AES-CBC encrypt/decrypt, a 256 KiB
L2-cache-size scan proxy, and a larger memory-copy probe.

ZIP archive structure, BZIP2, 7z, and native AVX/SSE/SSE2/SSE4/AVX2/FMA feature
microbenchmarks are not included because Windows PowerShell/.NET Framework does
not expose those as dependency-free benchmark APIs. Use an external native
runner if those exact formats or instruction sets need certification.

The synthetic runner is not a full Winderust automation benchmark. It does not launch the app or
exercise the real automation loop. It models the preset scheduler effects with:

- process priority,
- foreground process priority,
- foreground and background dynamic priority boost,
- foreground thread priority,
- foreground I/O priority,
- foreground GPU priority attempts,
- background thread priority,
- background memory priority,
- background I/O priority,
- background GPU priority attempts,
- target count,
- adaptive CPU share with CPU Sets (Soft) in the app,
- and hard processor affinity for manual benchmark approximations.

PowerShell hard affinity is stricter than Winderust CPU Sets (Soft). The benchmark
therefore omits adaptive affinity for likely hybrid CPUs, where Windows can
already push Idle background workers toward E-cores, but applies a hard-affinity
approximation for standard/all-P CPUs so shared-core machines are tested against
the same foreground-lane intent as the runtime preset.

The default foreground latency loop is CPU-bound. The separate score suite adds
compression, crypto, arithmetic, and cache/memory probes, but I/O and GPU
controls are still coverage checks unless you run the optional I/O, message
loop, launch, or external GPU benchmarks.

## Hardware Scope

Do not treat one local benchmark as universal. Record the CPU model, logical
processor count, Windows power mode, and whether the machine has Intel-style
P-cores plus E-cores or an all-P-core layout such as most AMD desktop CPUs.

Adaptive Engine's internal CPU Scheduler masking is topology-aware:

- Least-used selection ranks the configured All, P-core, or E-core logical-processor pool by
  sampled load and assigns the configured processor-limit share with rebalance hysteresis.
- Fixed selections can target P-cores, E-cores, all cores or P-cores without SMT, or an exact custom
  processor mask.
- The per-app CPU threshold controls when a background app becomes eligible; the foreground or
  system threshold still controls when pressure restraint is active.

Benchmark matrix for preset changes:

| Hardware class | Required check |
| --- | --- |
| Intel hybrid P-core + E-core | Verify foreground median, p95, jitter, and that Foreground First does not steal P-core time from the focused app. |
| AMD or other all-P-core CPU | Verify foreground median and p95 improve without collapsing background retained capacity. |
| Low-core CPU, 4 to 8 logical processors | Verify Foreground First still leaves at least one background lane and does not produce unstable tails. |

If only one hardware class is available, document that limitation and avoid
changing global preset constants unless the result is clearly supported by code
reasoning and topology-specific unit tests.

## Scheduling principles used for tuning

Winderust cannot reproduce Linux scheduling on Windows, but the upstream Linux
design provides useful policy constraints:

- [EEVDF](https://docs.kernel.org/scheduler/sched-eevdf.html) uses eligibility,
  virtual deadlines, and decaying lag to balance fairness with latency. Winderust
  therefore must not let an unbounded lifetime history permanently dominate
  current CPU demand.
- [Utilization clamping](https://docs.kernel.org/scheduler/sched-util-clamp.html)
  treats minimum and maximum performance as hints in a feedback loop and warns
  that static values are not portable. Winderust should adapt CPU Sets to
  measured pressure rather than treat a preset percentage as a universal cap.
- [Energy Aware Scheduling](https://docs.kernel.org/scheduler/sched-energy.html)
  uses energy-aware placement below its overutilization point, then falls back
  to normal load balancing when capacity is saturated. Winderust similarly
  releases CPU-set placement at foreground saturation. While pressure remains
  active, it keeps the configured priority and EcoQoS hints across eligible
  Visible Window and Background processes; only selected hot Background
  candidates receive processor-allocation escalation.
- [cgroup v2 CPU control](https://docs.kernel.org/admin-guide/cgroup-v2.html)
  distinguishes work-conserving weights from hard bandwidth limits. Winderust
  keeps visible-window restraint soft; selected background apps can receive the
  configured processor restriction in the same pressure pass.
- Keep total pressure as whole-machine utilization, but measure a hot process
  against one logical processor's capacity. Otherwise the same single-threaded
  offender appears colder merely because the PC has more logical processors.

The Windows mapping follows Microsoft's own guidance: [Quality of Service](https://learn.microsoft.com/en-us/windows/win32/procthread/quality-of-service)
already distinguishes focused, visible, and background work; [CPU Sets](https://learn.microsoft.com/en-us/windows/win32/procthread/cpu-sets)
provide soft affinity; and Microsoft advises that [hard affinity should generally
be avoided](https://learn.microsoft.com/en-us/windows/win32/procthread/multiple-processors)
because it can interfere with scheduler placement.

## Validation hierarchy

| Stage | Purpose | Acceptance rule |
| --- | --- | --- |
| Policy unit tests | Prove pressure, saturation, restoration, bounded history, and topology decisions. | All deterministic state transitions pass. |
| Synthetic mechanism matrix | Isolate priority, QoS, CPU Sets, affinity, and processor-policy effects. | Median and P95 improve in at least 2/3 paired passes; absolute values are used for direct preset ranking. |
| Release-runtime A/B | Exercise the real automation loop and current serialized preset. | Required before accepting a global preset change. Run CPU, I/O, and message-loop scenarios; the runner fails unless it directly observes a generated worker priority change. |
| Hardware matrix | Check topology portability. | At least Intel hybrid plus AMD/all-P evidence before claiming a universal default. |

Do not rank presets as a linear slow-to-fast ladder. Compare foreground latency,
background retained throughput, package power, and action stability as a Pareto
trade-off. A higher paired percentage does not mean one preset beats another
when each percentage has a different adjacent Stock denominator.

## Current Adaptive Engine Preset Model

Keep this in sync with the Adaptive Engine preset values in
`src/ui/app/shared/presets.rs`.

| Preset | Benchmark model |
| --- | --- |
| Off | 12 background workers at `Normal`; foreground benchmark process at `Normal`. |
| Powersave | Strict processor Saver policy (`max 45`, boost disabled) plus Low Impact scheduling, EcoQoS, Below Normal background process priority, and targeted restraint of at most 4 workers. Priority assists are disabled. |
| Balanced | Moderate processor policy (`max 95`, efficient boost) plus the same Low Impact scheduling with a higher processor ceiling than Powersave. |
| Performance | High processor policy (`min 25`, `max 100`, efficient aggressive boost) plus Foreground First scheduling, EcoQoS, Below Normal background process priority, Very Low background I/O and memory priority, Below Normal background thread/GPU priority, and at most 8 restrained workers. |
| Speed | Aggressive processor policy (`parking 100`, `min 25`, `max 100`, aggressive boost) plus Maximum Foreground scheduling, a 10% background CPU target, foreground Above Normal/High assists, Very Low/Idle background assists, and at most 12 restrained workers. |

For launch foreground scenarios, preset cases intentionally use launch grace:
foreground launch priority is raised to `AboveNormal`, while background
restraints are deferred until after the app-start window.

## Before Running

Run from the repository root:

```powershell
cargo check --locked
cargo test --locked cpu_scheduler
```

For cleaner benchmark results:

- Plug in AC power.
- Close browsers, game launchers, update tools, and other background work.
- Avoid moving windows or using the machine during the run.
- Close Winderust before using `-ForegroundScenario WinderustLaunch`.
- Run the benchmark at least twice if results are surprising.

The agent must request escalation for the benchmark command because it spawns
temporary CPU-load child processes, changes process priority, applies affinity,
and kills the workers during cleanup.

## Benchmark Command

Primary real-runtime Balanced/Low Impact A/B matrix:

```powershell
.\scripts\adaptive_runtime_benchmark.ps1 -ForegroundScenario CpuLoop -Passes 4 -Rounds 5
.\scripts\adaptive_runtime_benchmark.ps1 -ForegroundScenario IoLoop -Passes 4 -Rounds 5
.\scripts\adaptive_runtime_benchmark.ps1 -ForegroundScenario MessageLoop -Passes 4 -Rounds 5
```

Use `-DisableBackgroundProcessorLimit`,
`-ProcessRestraintThresholdPercent <1-100>`, or
`-MaximumRestrainedApps <1-32>` only for controlled tuning variants. Use
`-ForegroundOrSystemCpuThresholdPercent <1-100>` to force a known activation
threshold during controlled CPU Scheduler comparisons; the
default command keeps the serialized Low Impact values.

Use `-BackgroundPressureAcBoostPolicy <0-100>` and
`-BackgroundPressureAcBoostMode <mode>` to screen an A/C Background Pressure
profile without changing the app defaults. The selected values are written into
the JSON report. Focus and Launch remains at its default profile values.

Use `-MinimumPowerSavingPercent 20` only when testing an explicit power-saving
objective. The default remains `-2`, which rejects a package-power regression
beyond 2% without pretending that every responsiveness preset must save 20% in
a foreground-contended Focus and Launch workload.

The runtime benchmark uses an isolated portable configuration with the current
500 ms Processor Power cadence and 1.5 second Low Impact CPU Scheduler reaction
interval. It explicitly enables the current Balanced processor policy and Low
Impact CPU Scheduler preset. Use an even pass count of at least four so
Stock-first and Adaptive-first orders are equally represented. Stock and
Adaptive cases receive the same 100-second background-load warmup and 30-second
cooldown before measurement. The JSON validation gate requires observed
CPU Scheduler priority control, at least 3% aggregate median and P95
improvement, at least 85% retained background throughput, and no package-power
regression beyond 2%. A run that only creates the adaptive power plan without
changing a generated worker priority is invalid for scheduler tuning.

The JSON also records `worker_efficiency_enabled_counts` and
`worker_efficiency_coverage_percent` from direct Windows Power Throttling
queries. Use them to distinguish a missing EcoQoS application from a valid
control whose measured package-power effect simply misses the selected gate.

The runner adds the exact PowerShell benchmark-host path to CPU Scheduler
exclusions. Every case is rejected if that host leaves Normal priority or if any
generated worker exits before measurement completes. These are benchmark
integrity requirements: without them, Winderust can restrain the workload being
treated as foreground or a dead worker can create a false latency win.

Synthetic mechanism-isolation command:

```powershell
.\scripts\cpu_scheduler_benchmark.ps1 -Passes 3 -Rounds 5 -Iterations 1000000
```

Use `-ProcessTier Focus`, `-ProcessTier VisibleWindow`, or
`-ProcessTier Background` to benchmark the corresponding current preset tier.
The default is `Focus`.

For pressure-transition validation, include moderate load, foreground
saturation at 85% or more of whole-machine CPU, and recovery below the restore
band. At saturation, priority and EcoQoS must remain active while automatic CPU
Sets relax to 100%; recovery must not oscillate before the recovery period expires.

The score suite runs by default. Use `-ScoreIterations`, `-ScoreDataKb`, and
`-ScoreRounds` to scale it, or `-SkipScoreBenchmark` when validating only the
older foreground-latency path.

Foreground file-I/O scenario:

```powershell
.\scripts\cpu_scheduler_benchmark.ps1 -ForegroundScenario IoLoop -Passes 3 -Rounds 5 -IoOperations 2000
```

Foreground message-loop scenario:

```powershell
.\scripts\cpu_scheduler_benchmark.ps1 -ForegroundScenario MessageLoop -Passes 3 -Rounds 5 -MessageLoopTicks 200
```

Winderust launch scenario:

```powershell
.\scripts\cpu_scheduler_benchmark.ps1 -ForegroundScenario WinderustLaunch -Passes 3 -Rounds 3 -WorkerSeconds 20
```

Power-drain benchmark:

```powershell
.\scripts\power_drain_benchmark.ps1 -Phases Baseline,AdaptiveEngine -MinPasses 3 -MaxPasses 8 -SampleSeconds 30 -StableCvPercent 5
```

Quick sensor check:

```powershell
.\scripts\power_drain_benchmark.ps1 -Phases Current -MinPasses 3 -MaxPasses 8 -SampleSeconds 10 -StableCvPercent 20 -NoPrompt
```

If the release binary is not built, either run `cargo build --release` first or
pass `-WinderustExePath <path-to-winderust.exe>`.

Trust a local tuning direction only when median and P95 both improve by at least
3% in at least two of three passes. If median improves but P95 or jitter gets
worse, the change is not validated; it probably only moved the average.

The script rotates case order between passes to reduce order and thermal bias.
Each preset is compared with its own adjacent Off run, and the pair order flips
between passes. The summary `Off` row is the average of all Off runs, so it will
not always match the paired Off value shown beside a specific preset. The JSON
includes `assist_controls`, `assist_status`, and `assist_coverage` so a report
can show which OS controls were applied and where the synthetic workload is only
directional. It still remains a local synthetic benchmark, not proof of
universal defaults.

## Interpreting Results

Use `avg_ms`, `median_ms`, and `p95_ms`. Lower is better.

- `Average foreground time (avg_ms)`: average milliseconds for the foreground CPU
  benchmark loop across all rounds.
- `Median foreground time (median_ms)`: middle round time after sorting the
  samples; useful when one round is an outlier.
- `P95 foreground time (p95_ms)`: near-worst round time. With 7 rounds this is
  effectively the second-slowest round, because the benchmark keeps the single
  worst round visible separately as `max_ms`.
- `avg_vs_off`, `median_vs_off`, and `p95_vs_off`: case milliseconds, paired
  `Off` baseline milliseconds, absolute delta, and percent change, for example
  `224.50 ms vs 345.00 ms paired Off (-120.50 ms, +35.0%)`.
- `avg_delta_vs_off`, `median_delta_vs_off`, and `p95_delta_vs_off`: shorter
  absolute change plus percent change. Negative milliseconds means faster than
  Off.
- `Average vs Off`: percent change compared with the `Off` case. Positive means
  faster than Off; negative means slower than Off.
- `Foreground jitter (foreground_stddev_ms)`: standard deviation of foreground
  round times. Lower means the foreground work is more consistent.
- `Foreground range (foreground_range_ms)`: slowest round minus fastest round.
  Lower means fewer spikes.
- `P95 minus median (p95_minus_median_ms)`: tail-latency gap. Lower means the
  near-worst round is closer to normal behavior.
- `Foreground iterations/sec`: foreground work throughput derived from average
  time. Higher is better.
- `Score benchmark`: compact foreground real-work throughput suite. The JSON
  stores component values under `score_benchmark` and component ratios under
  `score_benchmark_vs_off`.
- `Foreground IOPS`: foreground file operations per second in the optional
  `IoLoop` scenario. It is a synthetic temp-file read/write loop, not a storage
  certification benchmark.
- `Message-loop delay`: average and P95 timer delay in the optional
  `MessageLoop` scenario. Lower is better; this is closer to UI pump
  interactive latency than CPU-loop throughput.
- `System average interactivity percent`: foreground average latency compared
  with the no-background baseline from the same pass. `100%` means equal to the
  no-background baseline; lower means the foreground loop slowed down under
  generated load. Values above `100%` can happen when priority changes make the
  synthetic foreground loop faster than the no-background sample.
- `Background throughput percent`: approximate share of total logical CPU capacity
  consumed by the generated background workers during the foreground measurement.
  Lower usually means the preset is sacrificing more background throughput.
- `Background throughput retained vs Off`: background throughput percent divided
  by the paired Off case. `100%` means the background workers kept the same CPU
  share as Off; lower means more foreground protection by reducing background
  work. Values above `100%` mean the background workers got more CPU time than
  in the paired Off case.
- `Background suppression vs Off`: the primary background-cost metric, calculated
  as `max(0, 100% - retained throughput)`. `0%` means no measured suppression;
  `87%` means only 13% of paired-Off background throughput remained.
- `Background latency slowdown vs Off`: fixed-background-work latency estimate
  derived from the inverse of retained background throughput. For example,
  `+50%` means the same background CPU work would take about 1.5x as long as
  paired `Off`; `+500%` means about 6x as long. This is the foreground/background
  cost that helps explain why keeping more background work active can
  raise CPU package watts even when foreground latency looks good.
- `Repeat passes won`: passes where both median and P95 beat paired Off by at
  least 3%.
- `median_w_avg`: average of repeated pass medians from
  `power_drain_benchmark.ps1`. Lower is better.
- `median_w_cv_percent`: repeat stability for package watts. Treat a result as
  usable only when `stable` is true.
- `saving_percent_vs_baseline`: package-watt reduction against the first phase.
  Positive means lower package power than baseline.

Prefer changes that improve median and p95 together. Ignore one-off wins where
average improves only because of a single outlier. If a preset is slower by less
than about 3%, treat it as neutral unless repeated runs show the same direction.

## Saved Results

Machine-specific reports and their raw JSON live in [`benchmark/`](../benchmark/README.md). Keep
this guide focused on methodology; add new measurements to a dated hardware report instead of
embedding a moving "latest result" here.

## Known Limitations

- The foreground latency loop is CPU-bound. The score suite covers managed CPU,
  compression, crypto, and cache/memory probes, but it is not a native
  AVX/SSE/FMA feature benchmark and does not run BZIP2, 7z, or real ZIP archive
  workloads.
- Foreground/background I/O and GPU priority controls are coverage checks unless
  the optional I/O/message/launch scenarios or an external GPU benchmark are
  run.
- GPU priority is attempted against the benchmark process and generated workers;
  CPU-only work may not have a GPU context, so `gpu_priority_unavailable` is
  expected on many systems.
- It does not test real foreground-app detection, Winderust exclusions, restore,
  cooldown, the Focus and Launch profile,
  or failure handling.
- Hard affinity may make CPU-share behavior look harsher than Winderust CPU Sets (Soft).
- Thermal throttling and Windows background services can move results by several
  percent.
- The power-drain benchmark needs a Windows `Energy Meter` or `Power Meter`
  counter. CPU package watts are not whole-system battery drain, and idle results
  can stay unstable until background activity settles.

## After Changes

Run:

```powershell
cargo check --locked
cargo test --locked cpu_scheduler
cargo test --locked
git diff --check
```

After source changes, run:

```powershell
graphify update .
```
