# Intel Core 5 210H — Current Preset Benchmark and System Diagnostics

Measured on 2026-08-06 against the current development presets. This report
contains absolute results, paired comparisons, repeat counts, test conditions,
and explicit limitations. It replaces the earlier scheduler-model-only report.

## Result at a glance

- Every preset passed the local responsiveness gate in all 3/3 passes for
  Focus Process, Visible Window, and Background tiers.
- Speed produced the lowest Focus Process latency: 103.90 ms median and
  106.59 ms P95, with 9.57 million foreground iterations/s.
- Balanced was within 5.5 ms of Speed in every tier while using the less
  aggressive Low Impact workload preset.
- Performance was slower than Balanced and Speed in the foreground test, but
  retained 126.6–156.9% of paired Stock background throughput. It is the preset
  that most clearly favors concurrent work instead of maximum foreground
  isolation.
- Power Save was the only preset below its paired Stock package power in the
  single power pass: 15.489 W versus 16.844 W, an 8.0% saving.
- Component percentages are comparisons with each preset's adjacent Stock
  sample, not direct comparisons between presets. Power Save is below Balanced
  in 26/27 absolute component scores; the one exception is a highly variable
  Background AES-CBC result.
- Balanced beats Performance in 21 of 27 absolute component scores and in all
  three foreground-latency summaries, but Performance preserves far more
  background throughput. The presets are trade-off profiles, not a linear
  slow-to-fast ladder.
- The package-power comparison has one pass and is exploratory. It is not
  sufficient to claim universal power efficiency or rank all presets.

## Test system

| Item | Value |
| --- | --- |
| PC | ASUS Vivobook 16 V3607VU |
| OS | Windows 11 Home, build 26200 |
| CPU | Intel Core 5 210H |
| CPU topology | 8 physical cores / 12 logical processors |
| Memory | 15.61 GB SK Hynix DDR5-5600 |
| GPU | NVIDIA GeForce RTX 4050 Laptop GPU 6 GB and Intel Graphics |
| Storage | WD PC SN5000S 512 GB NVMe |
| Active stock plan | Ultimate Performance |
| Running processes | 239–240 |
| Running threads | 5,959 |
| System uptime | 76.6 hours |

This was a busy live desktop, not a clean laboratory image. Edge WebView,
VS Code, Termius, Logitech services, and other background software remained
active. Paired Stock runs reduce drift but do not remove it.

## Method

The benchmark used
[`cpu_scheduler_benchmark.ps1`](../scripts/cpu_scheduler_benchmark.ps1)
with 12 generated CPU workers. Each tier matrix used three independently
ordered passes, five foreground rounds per case, and a paired Stock case beside
each preset. The workload applied the current preset processor policy,
process priority, EcoQoS target count, hard-affinity behavior, and tier-specific
priority assists.

The local gate requires both median and P95 foreground latency to improve by at
least 3% in at least two of three passes. All reported wins below were 3/3.

Raw data:

- [Focus Process matrix](results/intel-core-5-210h-current-focus-20260806.json)
- [Visible Window matrix](results/intel-core-5-210h-current-visible-window-20260806.json)
- [Background matrix](results/intel-core-5-210h-current-background-20260806.json)
- [Focus Process package-power pass](results/intel-core-5-210h-current-focus-power-20260806.json)

## Stock and preset values

Processor values are `minimum unparked cores / minimum performance / maximum
performance / boost policy`.

| Mode | Processor values | Boost mode | Workload preset |
| --- | --- | --- | --- |
| Stock Ultimate Performance | 100% / 100% / 100% / 100% | Aggressive | Controls off |
| Power Save | 0% / 5% / 45% / 0% | Disabled | Low Impact |
| Balanced | 25% / 5% / 95% / 60% | Efficient Enabled | Low Impact |
| Performance | 100% / 25% / 100% / 85% | Efficient Aggressive | Foreground First |
| Speed | 100% / 25% / 100% / 100% | Aggressive | Maximum Foreground |

| Applied control | Power Save | Balanced | Performance | Speed |
| --- | ---: | ---: | ---: | ---: |
| Restrained worker processes | 6 | 6 | 8 | 12 |
| EcoQoS worker processes | 6 | 6 | 8 | 12 |
| Hard-affinity worker processes | 0 | 0 | 0 on this hybrid CPU | 12 |
| Speed hard-affinity mask | — | — | — | 2 logical processors |

Foreground First normally uses automatic CPU Sets (Soft). The runner does not
attempt to reproduce Winderust's topology-aware CPU-set selection on this
hybrid CPU, so its affinity count is zero. Maximum Foreground uses the selected
hard-affinity behavior and was measured with the current 10% setting, rounded
up to two logical processors.

## How to compare presets correctly

`Stock` in this report means the active Ultimate Performance plan with
Winderust's preset controls off. The raw JSON calls this condition `off`.

| Question | Correct value | Why |
| --- | --- | --- |
| Is a preset faster than Stock? | Paired `% vs Stock` in the same row | Each preset is compared with its adjacent Stock run to reduce drift. |
| Is one preset faster than another? | Absolute latency, iterations/s, or component score within the same tier | Paired percentages use different Stock denominators and are not directly comparable across rows. |
| Which preset is best overall? | Foreground result together with background retained and package power | The presets optimize different trade-offs; no single metric defines the ordering. |

Power Save can therefore show a larger paired percentage than Balanced for an
individual short test even when its absolute score is lower. Across the 27
absolute component results below, Balanced is higher than Power Save in 26/27.
Balanced is also higher than Performance in 21/27, while Performance retains
126.6–156.9% of Stock background throughput versus Balanced's 40.0–50.4%.

The sole Power Save absolute win is Background AES-CBC. Its three pass values
were 682.97, 165.67, and 828.60 MB/s, compared with Balanced at 277.32, 303.12,
and 298.63 MB/s. Power Save won 2/3 passes but had a 662.93 MB/s range; Balanced
had a 25.80 MB/s range. The result is preserved, but its instability makes it
unsuitable as evidence that Power Save is generally faster than Balanced.

## Foreground responsiveness under background load

Lower latency is better; higher iterations/s is better. `Off` is renamed
`Stock` in the reader-facing tables below. Improvement and
background-retained percentages are means of the paired per-pass ratios, so
they do not necessarily equal a ratio calculated from the displayed aggregate
absolute values.

### Focus Process tier

| Mode | Median | P95 | Foreground iterations/s | Median vs Stock | P95 vs Stock | Background retained | Wins |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Stock | 332.74 ms | 353.03 ms | 2,959,943 | baseline | baseline | 100.0% | baseline |
| Power Save | 251.39 ms | 257.19 ms | 3,955,876 | +26.1% | +26.1% | 40.1% | 3/3 |
| Balanced | 109.64 ms | 110.66 ms | 9,174,814 | +66.4% | +68.8% | 40.0% | 3/3 |
| Performance | 137.63 ms | 147.77 ms | 7,259,978 | +59.0% | +58.8% | 126.6% | 3/3 |
| Speed | 103.90 ms | 106.59 ms | 9,574,809 | +68.0% | +69.1% | 42.6% | 3/3 |

### Visible Window tier

| Mode | Median | P95 | Foreground iterations/s | Median vs Stock | P95 vs Stock | Background retained | Wins |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Stock | 323.79 ms | 346.80 ms | 3,057,454 | baseline | baseline | 100.0% | baseline |
| Power Save | 243.34 ms | 247.66 ms | 4,074,373 | +21.8% | +26.7% | 43.3% | 3/3 |
| Balanced | 109.40 ms | 111.14 ms | 9,007,366 | +67.0% | +67.3% | 49.6% | 3/3 |
| Performance | 145.15 ms | 154.15 ms | 6,898,139 | +56.2% | +56.1% | 156.9% | 3/3 |
| Speed | 105.17 ms | 111.39 ms | 9,244,088 | +66.6% | +68.7% | 41.9% | 3/3 |

### Background tier

| Mode | Median | P95 | Foreground iterations/s | Median vs Stock | P95 vs Stock | Background retained | Wins |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Stock | 327.32 ms | 343.70 ms | 3,054,314 | baseline | baseline | 100.0% | baseline |
| Power Save | 245.73 ms | 251.29 ms | 4,021,842 | +21.9% | +23.7% | 45.8% | 3/3 |
| Balanced | 108.49 ms | 109.95 ms | 9,200,092 | +68.5% | +68.6% | 50.4% | 3/3 |
| Performance | 147.80 ms | 162.28 ms | 6,599,781 | +55.4% | +55.2% | 132.3% | 3/3 |
| Speed | 105.59 ms | 107.81 ms | 9,346,340 | +66.0% | +67.3% | 50.3% | 3/3 |

## Package power under the Focus Process workload

The power run used five samples per case and skipped the short component-score
suite so the generated workers remained active throughout each measurement.
Only one paired pass was collected; run order is shown because temperature and
boost history can affect laptop results.

### Power Save compared directly with Stock

| Metric | Paired Stock | Power Save | Power Save change |
| --- | ---: | ---: | ---: |
| Package power median | 16.844 W | 15.489 W | −1.355 W / −8.0% |
| Package power P95 | 16.975 W | 15.676 W | −1.299 W / −7.7% |
| Foreground latency median | 444.24 ms | 284.97 ms | −159.27 ms / 35.9% faster |
| Foreground throughput | 2,400,676 iter/s | 3,094,246 iter/s | +693,570 iter/s / +28.9% |
| Background throughput retained | 100.0% | 66.4% | −33.6 percentage points |

In this paired pass, Power Save used 8.0% less median package power than Stock
while completing the foreground workload 35.9% faster. That improvement came
with 33.6% less background throughput, so it is a foreground-isolation result,
not free performance.

### All presets compared with their paired Stock run

| Mode | Pair order | Stock power | Preset power | Power change | Stock latency | Preset latency | Latency improvement | Background retained |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Power Save | Power Save → Stock | 16.844 W | 15.489 W | −8.0% | 444.24 ms | 284.97 ms | +35.9% | 66.4% |
| Balanced | Stock → Balanced | 16.611 W | 58.603 W | +252.8% | 491.51 ms | 168.61 ms | +65.7% | 106.8% |
| Performance | Performance → Stock | 16.729 W | 54.184 W | +223.9% | 436.78 ms | 164.06 ms | +62.4% | 87.0% |
| Speed | Stock → Speed | 17.246 W | 26.411 W | +53.1% | 697.04 ms | 102.83 ms | +85.2% | 23.0% |

Each row has its own adjacent Stock measurement; do not compare the Stock
latency values across rows as though they were one stable baseline. The
single-pass run is useful for showing the Power Save direction, but the large
Stock latency spread also demonstrates why a reliable all-preset power ranking
needs more alternating passes.

Package power came from
`\Energy Meter(RAPL_Package0_PKG)\Power`. The large Balanced and Performance
values show that their processor policies allowed sustained high package power
in this pass. They must not be read as stable long-run averages without more
alternating passes, thermal normalization, and battery/AC isolation.

## Arithmetic, compression, crypto, cache, and memory scores

Each cell is the absolute mean followed by the mean paired score relative to
its adjacent Stock run in parentheses. Stock is the mean of all 12 paired Off
cases in that tier. Units are Mops/s for integer, FP64, and float batch; MB/s
for the remaining columns. Higher is better.

### Focus Process component scores

| Mode | Integer | FP64 | Float batch | GZip | Deflate | SHA-256 | AES-CBC | L2 scan | Memory copy |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Stock | 1,439.53 (100%) | 416.76 (100%) | 2,038.94 (100%) | 26.80 (100%) | 26.29 (100%) | 819.96 (100%) | 544.09 (100%) | 820.66 (100%) | 8,703.97 (100%) |
| Power Save | 1,547.79 (137.2%) | 465.71 (108.6%) | 2,305.92 (170.0%) | 36.41 (215.4%) | 35.66 (130.2%) | 904.87 (103.2%) | 788.14 (350.8%) | 988.19 (113.2%) | 9,965.97 (105.7%) |
| Balanced | 3,539.96 (230.8%) | 1,079.01 (255.7%) | 5,188.21 (230.0%) | 85.30 (293.5%) | 79.70 (338.7%) | 1,996.85 (240.8%) | 1,211.89 (228.9%) | 2,198.98 (264.5%) | 14,689.96 (165.5%) |
| Performance | 3,412.23 (264.1%) | 1,023.48 (287.0%) | 4,924.01 (257.0%) | 71.26 (241.6%) | 61.64 (254.4%) | 1,864.25 (345.3%) | 1,281.59 (328.7%) | 2,133.74 (342.9%) | 13,244.57 (175.0%) |
| Speed | 3,509.30 (223.7%) | 1,081.95 (243.3%) | 5,257.86 (233.1%) | 85.96 (289.4%) | 86.09 (306.4%) | 2,027.75 (227.4%) | 1,654.85 (244.0%) | 2,075.60 (234.3%) | 18,577.06 (212.7%) |

### Visible Window component scores

| Mode | Integer | FP64 | Float batch | GZip | Deflate | SHA-256 | AES-CBC | L2 scan | Memory copy |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Stock | 1,551.03 (100%) | 409.41 (100%) | 2,154.75 (100%) | 29.04 (100%) | 28.27 (100%) | 786.74 (100%) | 707.65 (100%) | 796.91 (100%) | 7,421.63 (100%) |
| Power Save | 1,591.38 (104.8%) | 461.80 (121.3%) | 2,295.11 (111.5%) | 37.17 (136.3%) | 34.24 (122.5%) | 884.52 (98.5%) | 830.11 (240.8%) | 968.79 (107.1%) | 10,280.36 (221.1%) |
| Balanced | 3,483.64 (223.8%) | 1,054.12 (251.1%) | 5,013.89 (228.7%) | 77.65 (264.5%) | 76.53 (278.5%) | 2,015.93 (272.7%) | 1,179.06 (270.2%) | 2,200.03 (282.0%) | 14,313.84 (179.0%) |
| Performance | 3,495.50 (223.7%) | 1,020.66 (232.3%) | 4,975.73 (237.0%) | 66.98 (218.9%) | 64.25 (222.4%) | 1,912.22 (286.9%) | 1,483.97 (179.5%) | 2,085.19 (246.3%) | 12,496.68 (145.4%) |
| Speed | 3,615.83 (231.0%) | 1,075.54 (279.9%) | 4,926.68 (219.2%) | 85.18 (298.0%) | 73.31 (259.0%) | 2,017.75 (255.2%) | 1,631.03 (196.5%) | 2,124.36 (414.8%) | 17,599.62 (471.1%) |

### Background component scores

| Mode | Integer | FP64 | Float batch | GZip | Deflate | SHA-256 | AES-CBC | L2 scan | Memory copy |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Stock | 1,509.72 (100%) | 388.33 (100%) | 2,048.30 (100%) | 26.85 (100%) | 25.93 (100%) | 822.75 (100%) | 275.35 (100%) | 782.92 (100%) | 7,728.13 (100%) |
| Power Save | 1,554.30 (104.1%) | 462.72 (117.5%) | 2,235.64 (108.7%) | 35.70 (124.2%) | 36.66 (126.1%) | 867.34 (97.2%) | 559.08 (220.4%) | 962.96 (107.3%) | 9,997.48 (183.7%) |
| Balanced | 3,533.08 (230.5%) | 1,019.56 (338.3%) | 4,803.05 (219.7%) | 77.04 (323.3%) | 76.27 (301.9%) | 2,005.44 (295.5%) | 293.02 (290.2%) | 2,186.71 (282.6%) | 14,849.28 (165.1%) |
| Performance | 3,348.91 (222.9%) | 1,021.35 (242.3%) | 4,882.50 (298.3%) | 70.94 (262.4%) | 69.75 (375.3%) | 1,907.61 (232.6%) | 523.00 (138.8%) | 2,101.55 (283.5%) | 12,819.95 (174.8%) |
| Speed | 3,496.99 (232.4%) | 1,092.55 (277.1%) | 5,082.43 (245.3%) | 89.27 (374.8%) | 86.53 (328.6%) | 2,053.72 (236.8%) | 799.75 (228.4%) | 2,312.47 (402.7%) | 18,678.78 (233.8%) |

The large paired percentages in some short component tests reflect contention
and run-to-run variance as well as policy effects. The absolute scores and raw
paired cases are included so these are auditable rather than presented as
general performance-gain claims.

## Independent CPU and memory diagnostics

WinSAT provides an independent native reference outside the preset runner.

| Native WinSAT workload | Result |
| --- | ---: |
| Lempel-Ziv compression | 1,002.97 MB/s |
| Microsoft compression | 2,522.70 MB/s |
| AES-256 encryption | 15,162.06 MB/s |
| SHA-1 hashing | 7,274.09 MB/s |
| Memory bandwidth | 29,031.26 MB/s / 29.03 GB/s |

The separate custom diagnostic used nine runs:

| Workload | Median | P95 | P95 spread from median |
| --- | ---: | ---: | ---: |
| Integer dependency chain | 668.49 M iterations/s | 675.65 M/s | +1.07% |
| Scalar FP64 arithmetic | 3.10 GFLOP/s | 3.13 GFLOP/s | +0.97% |
| Eight-lane FP64 batch | 9.25 GFLOP/s | 9.30 GFLOP/s | +0.54% |
| Twelve-worker FP64 batch | 22.05 GFLOP/s | 27.81 GFLOP/s | +26.12% |
| 128 MB memory copy | 7.81 GB/s | 8.08 GB/s | +3.46% |
| 32 MB random-access latency | 98.14 ns | 111.57 ns | +13.68% worse |

The twelve-worker and random-access spreads confirm that this live desktop had
meaningful background variance. That supports using paired results and prevents
treating a single score as a hardware maximum.

References:

- [WinSAT CPU assessment](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-server-2012-r2-and-2012/cc742175%28v%3Dws.11%29)
- [WinSAT memory assessment](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-server-2012-r2-and-2012/cc742141%28v%3Dws.11%29)
- [Using WinSAT](https://learn.microsoft.com/en-us/windows/win32/winsat/using-winsat)

## Stock desktop and Winderust footprint

The stock baseline used 20 one-second samples with Winderust stopped.

| Metric | Median | P95 |
| --- | ---: | ---: |
| CPU utilization | 26.41% | 45.59% |
| CPU frequency | 4.51 GHz | 4.74 GHz |
| Available memory | 3.40 GB / 21.7% | 3.52 GB / 22.5% |
| Committed memory | 57.35% | 57.81% |
| Page faults | 101/s | 2,101/s |
| Disk throughput | 704 KB/s | 14.62 MB/s |
| Network throughput | 23.9 KB/s | 45.7 KB/s |
| GPU engine utilization | 6.47% | 27.92% |
| CPU package power | 15.79 W | 21.50 W |

Winderust was then measured with the current 500 ms CPU Scheduler cadence
and the user's existing configuration.

| Metric | Current result | Earlier 250 ms cadence | Change |
| --- | ---: | ---: | ---: |
| CPU usage of one core | 11.49% | 15.26% | −24.7% |
| Whole-machine CPU capacity | approximately 0.96% | approximately 1.27% | −0.31 percentage points |
| Working set | 114.6 MB | 117.2 MB | −2.2% |
| Private memory | 118.4 MB | — | — |
| Threads | 56 | — | — |
| Handles | 1,085 | — | — |
| Winderust processes | 2 | — | — |

A separate 30-second stability sample showed no increasing working-set or
handle trend. The cadence change reduces polling cost; it does not alter the
preset thresholds or the focus, visible-window, and background priority values.

## Applied-control audit

- Low Impact applied EcoQoS to exactly 6 workers in every Power Save and
  Balanced pass, with no failed actions.
- Foreground First applied EcoQoS to 8 workers and its tier-specific process,
  memory, I/O, thread, and boost controls where enabled.
- Maximum Foreground applied EcoQoS and hard affinity to all 12 workers. The
  corrected Background run applied native Idle thread priority in 3/3 passes
  with zero failed actions.
- Focus Process Speed could not apply the synthetic runner's High I/O priority
  in any of its three passes. Its foreground thread and GPU controls did apply.
  This limitation is preserved in the raw result instead of being hidden.
- Worker GPU priority was often unavailable because CPU-loop workers did not
  own an active GPU scheduling context. That is expected for this workload and
  is recorded separately from failed actions.

## Interpretation

Balanced is the strongest foreground-throughput compromise on this machine,
not simply a slower version of Performance. It beat Performance's absolute
foreground median by 20.3–26.6% across the three tiers and won 21/27 component
scores because its Low Impact policy removed more background contention.
Performance retained 126.6–156.9% of Stock background throughput, so it is the
better measured choice when concurrent background work matters. Speed remains
the lowest-latency choice; Power Save produced the only observed package-power
reduction.

These claims are local to this laptop, Windows state, workload, and preset
implementation. They justify the current tuning direction; they do not prove
the same ranking on AMD, desktop, low-core-count, or thermally constrained
systems.

## Limitations and next evidence needed

- The benchmark process was assigned a selected tier directly. It did not
  launch Winderust's automation loop or measure window-detection latency.
- The workload is CPU-heavy. I/O and GPU priority controls are audited for API
  application but are not meaningfully stressed by the foreground CPU loop.
- Three passes support the latency direction, not a universal hardware claim.
- Package power has only one paired pass. A publishable power ranking needs at
  least three alternating passes per preset after thermal stabilization.
- The live desktop was not isolated. A clean-boot run should be added before
  treating component scores as hardware-reference values.
- AMD and standard all-performance-core systems still need equivalent current
  preset matrices because CPU Sets and affinity behavior differ by topology.
