# Workload Engine Benchmark Guide

This guide is for the next agent run. It documents the synthetic benchmark used
while tuning Workload Engine presets.

## What This Measures

The default benchmark measures foreground CPU work completion time while
temporary background CPU workers compete for scheduler time. Lower milliseconds
are better.

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

It is not a full Winderust automation benchmark. It does not launch the app or
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
- adaptive CPU share with Soft CPU Sets in the app,
- and hard processor affinity for manual benchmark approximations.

PowerShell hard affinity is stricter than Winderust Soft CPU Sets. The benchmark
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

Workload Engine runtime masking is topology-aware:

- Hybrid CPUs: background affinity candidates prefer E-cores, then choose the
  least-loaded allowed E-cores when load data is available.
- All-P-core CPUs: background affinity candidates use all logical processors
  and choose the least-loaded allowed cores when load data is available.
- Automatic CPU-share floors are intentionally different: hybrid systems prefer
  E-cores, while all-P-core systems reserve a clearer foreground lane without
  going as far as Max Foreground.

Benchmark matrix for preset changes:

| Hardware class | Required check |
| --- | --- |
| Intel hybrid P-core + E-core | Verify foreground median, p95, jitter, and that Foreground First does not steal P-core time from the focused app. |
| AMD or other all-P-core CPU | Verify foreground median and p95 improve without collapsing background retained capacity. |
| Low-core CPU, 4 to 8 logical processors | Verify Foreground First still leaves at least one background lane and does not produce unstable tails. |

If only one hardware class is available, document that limitation and avoid
changing global preset constants unless the result is clearly supported by code
reasoning and topology-specific unit tests.

## Current Adaptive Engine Preset Model

Keep this in sync with the Adaptive Engine preset values in `src/app.rs`.

| Preset | Benchmark model |
| --- | --- |
| Off | 12 background workers at `Normal`; foreground benchmark process at `Normal`. |
| Powersave | Strict processor Saver policy (`max 45`, boost disabled) plus Low Impact scheduling, background efficiency, Idle process priority, Low IO/memory priority, and BelowNormal GPU/thread priority. |
| Balanced | Moderate processor policy (`max 95`, efficient boost) plus the same Low Impact scheduling/priority assists with a higher processor ceiling than Powersave. |
| Performance | High processor policy (`min 25`, `max 100`, efficient aggressive boost) plus active Foreground First CPU-pressure scheduling, background efficiency, Idle process priority, VeryLow background IO, Low memory priority, and BelowNormal GPU/thread priority. |
| Speed | Aggressive processor policy (`parking 100`, `min 25`, `max 100`, aggressive boost) plus active Max Foreground CPU-pressure scheduling, a stricter 6% background CPU target, foreground AboveNormal/High assists, and VeryLow/Idle background IO/memory/GPU/thread priority. |

For launch foreground scenarios, preset cases intentionally use launch grace:
foreground launch priority is raised to `AboveNormal`, while background
restraints are deferred until after the app-start window.

## Before Running

Run from the repository root:

```powershell
rtk cargo check
rtk cargo test workload_engine
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

Preferred repeat-loop command:

```powershell
.\scripts\workload_engine_benchmark.ps1 -Passes 3 -Rounds 5 -Iterations 1000000
```

The score suite runs by default. Use `-ScoreIterations`, `-ScoreDataKb`, and
`-ScoreRounds` to scale it, or `-SkipScoreBenchmark` when validating only the
older foreground-latency path.

Foreground file-I/O scenario:

```powershell
.\scripts\workload_engine_benchmark.ps1 -ForegroundScenario IoLoop -Passes 3 -Rounds 5 -IoOperations 2000
```

Foreground message-loop scenario:

```powershell
.\scripts\workload_engine_benchmark.ps1 -ForegroundScenario MessageLoop -Passes 3 -Rounds 5 -MessageLoopTicks 200
```

Winderust launch scenario:

```powershell
.\scripts\workload_engine_benchmark.ps1 -ForegroundScenario WinderustLaunch -Passes 3 -Rounds 3 -WorkerSeconds 20
```

Power-drain benchmark:

```powershell
.\scripts\power_drain_benchmark.ps1 -Phases Baseline,SmartSaver -MinPasses 3 -MaxPasses 8 -SampleSeconds 30 -StableCvPercent 5
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

Previous reference run after the one-parameter optimization pass:

| Case | Average foreground time | Median foreground time | P95 foreground time | Average vs Off |
| --- | ---: | ---: | ---: | ---: |
| Off | 289.93 ms | 288.64 ms | 297.23 ms | baseline |
| Low Impact | 261.99 ms | 263.03 ms | 268.07 ms | 9.6% faster |
| Foreground First | 148.42 ms | 145.27 ms | 151.42 ms | 48.8% faster |

Richer reference run with stability and background-throughput metrics:

| Case | Average foreground time | P95 foreground time | Foreground jitter | P95 minus median | Foreground iterations/sec | Background throughput retained vs Off |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Off | 844.28 ms | 892.48 ms | 59.31 ms | 71.09 ms | 1,480,548 | baseline |
| Low Impact | 826.93 ms | 853.80 ms | 19.35 ms | 36.50 ms | 1,511,608 | 96.1% |
| Foreground First | 251.25 ms | 256.34 ms | 6.07 ms | 2.87 ms | 4,975,123 | 84.2% |

That richer run shows why a single compact score is risky: Foreground First
improved both speed and stability, while Low Impact mostly improved stability.

Previous paired methodology validation on Intel Core 5 210H, 12 logical processors:

| Case | Median improvement avg | P95 improvement avg | Repeat passes won |
| --- | ---: | ---: | ---: |
| Low Impact | -0.7% | -2.2% | 0/3 |
| Foreground First | 60.1% | 54.9% | 3/3 |

Use this result to avoid over-tuning Low Impact from this synthetic loop. It
validates the method for large scheduling changes, but priority-only changes
need longer runs, more hardware, or real app traces before changing global
defaults.

Previous paired validation after adding background-throughput measurement on the same CPU:

| Case | Median improvement avg | P95 improvement avg | Background throughput retained avg |
| --- | ---: | ---: | ---: |
| Low Impact | 10.8% | 41.5% | 100.0% |
| Foreground First | 72.9% | 84.7% | 29.5% |

This shows the foreground/background cost directly: Foreground First is the
only large foreground win, but it deliberately gives up background throughput.
Low Impact may be useful for a light touch.

Latest CPU-loop validation after standard/all-P topology tuning on AMD Ryzen 7 7735HS, 16 logical processors:

| Case | Median improvement avg | P95 improvement avg | Background throughput retained avg | Repeat passes won |
| --- | ---: | ---: | ---: | ---: |
| Low Impact | 34.3% | 44.1% | 91.0% | 3/3 |
| Foreground First | 49.9% | 52.9% | 66.2% | 3/3 |
| Max Foreground | 50.5% | 55.5% | 16.6% | 3/3 |

This run used the standard/all-P benchmark approximation: Low Impact limited
background workers to 11 logical processors, Foreground First to 8, and Max
Foreground to 1.

Latest Adaptive Engine preset CPU-loop check on AMD Ryzen 7 7735HS, 16 logical
processors, 3 passes, 5 rounds, 1,000,000 foreground iterations per round, with
the score suite and RAPL package-power sampling:

| Case | Median latency vs Off | P95 latency vs Off | Foreground throughput vs Off | Background retained vs Off | Background latency vs Off | Package power vs Off | Repeat passes won |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Off | 228.67 ms | 239.84 ms | 4,540,008 iter/s | 100.0% | 1.00x | 63.90 W | baseline |
| Powersave | 377.30 ms (-70.6%) | 378.55 ms (-65.1%) | 2,643,422 iter/s (-41.8%) | 82.7% | 1.21x (+20.9%) | 10.97 W (-82.8%) | 0/3 |
| Balanced | 181.46 ms (+24.2%) | 187.33 ms (+24.7%) | 5,423,882 iter/s (+19.5%) | 83.2% | 1.20x (+20.3%) | 21.16 W (-66.9%) | 3/3 |
| Performance | 123.99 ms (+47.1%) | 125.15 ms (+50.4%) | 8,033,565 iter/s (+76.9%) | 66.6% | 1.50x (+50.2%) | 57.91 W (-9.4%) | 3/3 |
| Speed | 119.07 ms (+44.7%) | 119.35 ms (+47.6%) | 8,410,122 iter/s (+85.2%) | 8.3% | 12.05x (+1,105.4%) | 22.65 W (-64.5%) | 3/3 |

This run validates the current split with broader coverage: Balanced clears the
repeat-pass gate and saves package power but gives up score throughput,
Performance keeps a larger background lane while improving several score components,
and Speed now wins every score component in this run while making the
background lane much slower by design.

Native WinSAT D3D was checked on the same machine, but this Windows build no
longer runs the D3D assessment. The XML reports `NoD3DTestRun` and hardcoded
sentinel values, so it is not a valid gaming/GPU benchmark.

Latest foreground I/O-loop validation on Intel Core 5 210H, 12 logical processors:

| Case | Avg latency vs Off | Foreground IOPS vs Off | Median latency vs Off | P95 latency vs Off | Interactivity vs no-load | Background throughput vs Off | Repeat passes won |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Off | 74.61 ms | 55,604 | 75.45 ms | 80.11 ms | 36.7% | 100.0% | baseline |
| Low Impact | 52.38 ms (+29.8%) | 76,377 (+37.4%) | 54.79 ms (+27.4%) | 55.68 ms (+30.5%) | 52.3% | 75.2% | 3/3 |
| Foreground First | 27.43 ms (+63.2%) | 145,830 (+162.3%) | 27.43 ms (+63.6%) | 27.85 ms (+65.2%) | 99.8% | 19.8% | 3/3 |
| Max Foreground | 27.15 ms (+63.6%) | 147,368 (+165.0%) | 27.09 ms (+64.1%) | 27.56 ms (+65.6%) | 100.8% | 19.6% | 3/3 |

Latest Winderust launch scenario on AMD Ryzen 7 7735HS after launch-grace tuning:

| Case | Median improvement avg | Median improvement min | P95 improvement avg | P95 improvement min | Background throughput retained avg | Repeat passes won |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Low Impact | 3.8% | 0.0% | 3.8% | 0.0% | 99.8% | 1/3 |
| Foreground First | -4.8% | -12.5% | -4.8% | -12.5% | 99.9% | 1/3 |

Launch grace keeps background throughput intact while the foreground app starts,
but this app-launch scenario still does not validate stronger Foreground First
launch behavior. Treat the CPU-loop wins as scheduler headroom, not guaranteed
app-startup improvement.

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
  cooldown, launch boost,
  or failure handling.
- Hard affinity may make CPU-share behavior look harsher than Winderust Soft CPU
  Sets.
- Thermal throttling and Windows background services can move results by several
  percent.
- The power-drain benchmark needs a Windows `Energy Meter` or `Power Meter`
  counter. CPU package watts are not whole-system battery drain, and idle results
  can stay unstable until background activity settles.

## After Changes

Run:

```powershell
rtk cargo check
rtk cargo test workload_engine
rtk cargo test
rtk git diff --check
```

If `graphify-out/graph.json` exists, run:

```powershell
graphify update .
```

In the current workspace, `graphify-out` has been absent and `graphify update .`
has failed with:

```text
error: uv trampoline failed to canonicalize script path
```
