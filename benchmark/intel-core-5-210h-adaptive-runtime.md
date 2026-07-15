# Intel Core 5 210H — Adaptive Runtime

Run on 2026-07-15 with 12 logical processors and 12 workers. The CPU-loop suite
used 3 passes, 5 rounds, and 1,000,000 foreground iterations per round. Package
power came from the RAPL `Package0` counter.

## Five-pass repeatability gate

Raw results are preserved in `benchmark/results/`.

| Adaptive Burst policy | Median latency vs Stock | P95 latency vs Stock | Foreground throughput vs Stock | Package power vs Stock | Repeat passes won | Decision |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| Aggressive, no explicit cooldown | -1.6% | -1.5% | -2.2% | -0.6% | 1/5 | Superseded by controlled run |
| Efficient Aggressive, 50% parking, 15% minimum | -7.0% | -10.1% | -6.5% | -3.8% | 1/5 | Rejected and reverted |
| Aggressive, 10-second cooldown | +1.9% | +2.1% | +3.7% | +2.1% | 2/5 | Current controlled baseline |

The cooldown-controlled result shows a small performance-for-power trade rather than a
clear Pareto improvement. Softening boost improved power but caused a clear latency
regression, so that experiment was reverted. Sustained full saturation is therefore not
the next tuning target; mixed burst, interactive, and idle residency workloads are needed
to evaluate the adaptive transitions that Windows Balanced does not provide.


## Topology-aware rerun

This rerun includes separate P-core and E-core demand classification and the reduced
one-second aggregate I/O sampling cadence.

| Case | Median latency vs Stock | P95 latency vs Stock | Foreground throughput vs Stock | Background suppression vs Stock | Package power vs Stock | Repeat passes won |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Stock Balanced | 278.45 ms (baseline) | 307.80 ms (baseline) | 3,813,568 iter/s (baseline) | 0.0% | 58.29 W (baseline) | baseline |
| Adaptive runtime | 254.18 ms (+4.9%) | 256.78 ms (+11.1%) | 3,984,968 iter/s (+4.5%) | 0.07% | 56.15 W (-3.6%) | 1/3 |

The aggregate result remains favorable: latency, throughput, and package power all
improved versus Stock Balanced. Repeatability remains below the acceptance gate because
only one of three paired passes improved both median and p95 latency by at least 3%.
The topology change is therefore promising, but not yet proven optimal.

## Previous calibration run


| Case | Median latency vs Stock | P95 latency vs Stock | Foreground throughput vs Stock | Background suppression vs Stock | Package power vs Stock | Repeat passes won |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Stock Balanced | 280.04 ms (baseline) | 292.36 ms (baseline) | 3,601,668 iter/s (baseline) | 0.0% | 57.88 W (baseline) | baseline |
| Adaptive runtime | 263.12 ms (+5.2%) | 271.49 ms (+5.6%) | 3,796,346 iter/s (+5.4%) | 0.2% | 56.37 W (-2.6%) | 1/3 |

Score component ratios from the same run, all vs paired Stock Balanced samples:

| Case | Int arithmetic | Double arithmetic | Float batch | GZip | Deflate | SHA-256 | AES-CBC | L2 scan | Memory copy |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Stock Balanced | 3,232.0 Mops (100.0%) | 853.3 Mops (100.0%) | 4,438.9 Mops (100.0%) | 41.0 MB/s (100.0%) | 30.3 MB/s (100.0%) | 1,202.6 MB/s (100.0%) | 196.0 MB/s (100.0%) | 1,351.2 MB/s (100.0%) | 5,009.5 MB/s (100.0%) |
| Adaptive runtime | 2,236.3 Mops (69.8%) | 776.2 Mops (89.5%) | 4,403.4 Mops (99.1%) | 42.8 MB/s (107.3%) | 34.5 MB/s (122.6%) | 1,725.2 MB/s (167.5%) | 383.3 MB/s (162.8%) | 1,300.7 MB/s (97.0%) | 6,231.4 MB/s (124.4%) |

This is a real release-binary A/B. Stock used the Windows Balanced plan with no
Winderust process. Adaptive launched Winderust with an isolated configuration,
cloned Windows Balanced into `PowerLeaf Adaptive`, and ran the automation loop.
Every adaptive pass reached the hybrid-aware Burst AC policy: 50% minimum
unparked cores, 20% minimum processor state, 100% maximum processor state,
100% boost policy, and Aggressive boost mode.

Adaptive telemetry used a WALT-inspired dual cadence: per-core and I/O peaks
were sampled every 250 ms for burst response, while total CPU utilization kept
its one-second averaging window for sustained demand.

The topology and telemetry tuning reversed the original aggregate regressions:
median changed from -4.8% to +5.2%, p95 from -3.1% to +5.6%, and throughput
from -2.6% to +5.4%, while measured package power remained 2.6% below stock.
It still misses the
strict acceptance gate of at least 3% better median and p95 in two of three
passes, with one repeat pass won. Workload Engine scheduling controls remained
disabled so this test measures the adaptive power-plan controller itself.

Command:

```powershell
.\scripts\adaptive_runtime_benchmark.ps1 -Passes 3 -Rounds 5 -Iterations 1000000 -WorkerSeconds 45
```

The runner alternates case order between passes, removes the managed plan after
each adaptive case, and restores the plan that was active before the benchmark.
Results are local direction, not a cross-machine ranking. Thermal state, firmware,
Windows power policy, and other running software can change them.
