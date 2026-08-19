# Intel Core 5 210H Counterbalanced Adaptive Runtime Matrix

Dates: 2026-08-07 and 2026-08-10

## Verdict

The current Balanced plus Low Impact configuration passes the corrected CPU- and I/O-contention evaluators without further preset tuning. The MessageLoop result is valid but misses the aggregate median gate by 0.6 percentage points, so it is treated as neutral rather than a preset win.

The earlier runtime reports allowed Winderust to classify the PowerShell benchmark host as a background process. That could lower the measured foreground process itself and invalidate latency comparisons. The corrected harness excludes the exact benchmark-host path, verifies that its priority remains Normal, verifies that every worker survives measurement, and balances Stock-first and Adaptive-first ordering equally.

## Methodology

- CPU: Intel Core 5 210H, 12 logical processors.
- Background load: 11 generated `cscript.exe` CPU workers.
- Foreground load: 5 rounds of 1,000,000 iterations.
- Four passes: two Stock-first and two Adaptive-first.
- Warm-up: 100 seconds per case.
- Cooldown: 30 seconds before every case.
- Stock: Windows Balanced without Winderust.
- Adaptive: current release build using Balanced plus Low Impact.

## Validation gates

| Gate | Requirement | Result |
| --- | ---: | ---: |
| CPU Scheduler activation | 4/4 passes | 4/4 passes |
| Median improvement | at least 3% | 12.98% |
| P95 improvement | at least 3% | 13.98% |
| Background throughput retained | at least 85% | 98.05% |
| Package-power regression | no worse than 2% | 0.25% regression |
| Foreground host priority | Normal after every case | Normal |
| Workers alive after measurement | 11/11 every case | 11/11 |

Overall validation: **PASS**.

## Aggregate results

| Metric | Stock | Adaptive | Change |
| --- | ---: | ---: | ---: |
| Foreground median | 236.67 ms | 205.67 ms | 12.98% better paired average |
| Foreground P95 | 245.40 ms | 210.12 ms | 13.98% better paired average |
| Foreground throughput | 4,161,273.5 iter/s | 4,886,105 iter/s | 17.42% higher raw aggregate |
| Package-power median | 53.00 W | 53.13 W | 0.13 W higher raw aggregate |
| Background suppression | baseline | 2.53% | 98.05% retained paired average |
| Passes meeting latency gate | baseline | 4/4 | passed |

## Paired pass results

| Pass | Order | Median improvement | P95 improvement | Background retained | Power saving |
| ---: | --- | ---: | ---: | ---: | ---: |
| 1 | Stock then Adaptive | 10.6% | 10.3% | 97.4% | -0.5% |
| 2 | Adaptive then Stock | 15.2% | 15.9% | 97.8% | -3.2% |
| 3 | Stock then Adaptive | 18.2% | 23.8% | 94.7% | 0.7% |
| 4 | Adaptive then Stock | 7.9% | 5.9% | 102.3% | 2.0% |

All four passes improved both median and P95 latency. The package-power result is effectively neutral overall and varies by pass, so this result supports responsiveness rather than a power-saving claim.

## Additional scenario validation

The corrected four-pass I/O and MessageLoop scenarios were run on 2026-08-10 with the same release binary, isolation, warm-up, cooldown, ordering, host-priority, activation, and worker-survival checks.

| Scenario | Median improvement | P95 improvement | Background retained | Package-power saving | Passes meeting latency gate | Verdict |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| CPU loop | 12.98% | 13.98% | 98.05% | -0.25% | 4/4 | PASS |
| I/O loop | 29.42% | 29.28% | 98.67% | -0.35% | 4/4 | PASS |
| Message loop | 2.40% | 3.77% | 99.10% | -1.58% | 2/4 | FAIL: median below 3% |

Negative package-power saving means higher measured package power. All three regressions remain inside the 2% acceptance ceiling, so none supports a power-saving claim.

### I/O loop

| Metric | Stock | Adaptive | Change |
| --- | ---: | ---: | ---: |
| Foreground median | 60.36 ms | 42.55 ms | 29.42% better paired average |
| Foreground P95 | 62.95 ms | 44.40 ms | 29.28% better paired average |
| Package-power median | 50.93 W | 51.10 W | 0.17 W higher raw aggregate |
| Background throughput | baseline | 98.67% retained | 1.33% lower paired average |
| Passes meeting latency gate | baseline | 4/4 | passed |

Every I/O pass improved median and P95 by at least 25%. Background throughput stayed near Stock, so this is a repeatable responsiveness improvement rather than a trade for a heavily restricted background lane.

### Message loop

| Metric | Stock | Adaptive | Change |
| --- | ---: | ---: | ---: |
| Foreground median | 9.67 ms | 9.43 ms | 2.40% better paired average |
| Foreground P95 | 9.83 ms | 9.46 ms | 3.77% better paired average |
| Package-power median | 50.00 W | 50.76 W | 0.76 W higher raw aggregate |
| Background throughput | baseline | 99.10% retained | 0.90% lower paired average |
| Passes meeting latency gate | baseline | 2/4 | failed |

The absolute median change is only 0.24 ms. Two passes cleared both latency gates, while two were neutral at -0.7% and -0.4% median. This does not justify stronger global restraints or preset retuning on this hardware.

## Product finding

The benchmark exposed a CPU Scheduler defect: the broad background-priority target path did not check configured CPU Scheduler exclusions. The shared path now checks the exclusion before applying priority or Efficiency control. The corrected release build and shortened integrity run both verify the fix.

## Raw result

- [Counterbalanced CPU runtime](results/intel-core-5-210h-tuned-counterbalanced-cpu-20260807.json)
- [Counterbalanced I/O runtime](results/intel-core-5-210h-tuned-counterbalanced-io-20260810.json)
- [Counterbalanced MessageLoop runtime](results/intel-core-5-210h-tuned-counterbalanced-message-20260810.json)
