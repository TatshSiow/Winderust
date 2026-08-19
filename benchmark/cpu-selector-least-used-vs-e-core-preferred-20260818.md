# CPU Scheduler processor selector A/B

Tested on an Intel Core 5 210H with 12 logical processors and 11 background
workers. Both release binaries used CPU Sets (Soft), a 10% system-pressure
trigger, a 1% per-app trigger, and at most four restrained apps. The old build
used its automatic share logic with a 75% configured floor; the new build used
a fixed 75% processor limit. Each result is four counterbalanced Stock/Adaptive
passes with five foreground samples per case.

The old selector ranked only E-cores when a hybrid topology was present. The
new Least-used selector ranks every logical processor, while retaining the same
three-second rebalance hysteresis.

| Metric | Old E-core-preferred | New Least-used | Change |
| --- | ---: | ---: | ---: |
| Foreground median improvement vs Stock | 25.25% | 21.17% | -4.08 pp |
| Foreground P95 improvement vs Stock | 29.10% | 22.65% | -6.45 pp |
| Foreground median | 83.81 ms | 87.59 ms | +4.51% |
| Foreground P95 | 85.06 ms | 91.06 ms | +7.05% |
| Background throughput retained vs Stock | 82.33% | 97.78% | +15.45 pp |
| Background throughput suppressed | 17.68% | 2.40% | -15.28 pp |
| Package power saving vs Stock | 44.75% | 38.20% | -6.55 pp |
| Package power | 28.67 W | 32.96 W | +14.96% |
| Foreground passes won vs Stock | 4/4 | 4/4 | Same |
| Existing validation gate | Failed throughput gate | Passed | Improved balance |

## Verdict

Least-used is the better default for a general-purpose preset on this machine.
It preserved almost all background capacity and still delivered a 21% median
and 23% P95 foreground improvement. The old E-core-preferred behavior is more
aggressive: it produced about four to six percentage points more foreground
gain and greater power saving, but pushed background throughput below the 85%
acceptance floor.

Keep the explicit **E-cores** selection for users who prefer the old,
power-saving/foreground-first trade-off. Do not claim this result is universal
until the same release-runtime A/B is repeated on AMD/all-P-core and low-core
hardware.

This compares the complete old and new selector behaviors, including removal
of the old automatic-share adjustment; it does not attribute every delta to the
ranking pool alone.

Raw results:

- [Old E-core-preferred selector](results/cpu-selector-old-e-core-preferred-20260818.json)
- [New Least-used selector](results/cpu-selector-new-least-used-20260818.json)

During this run, the benchmark harness also had a stale `rules` field removed
from its strict CPU Scheduler settings table. Before that fix Winderust rejected
the isolated settings and the runtime-control validity gate correctly failed.
