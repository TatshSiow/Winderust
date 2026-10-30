# Wind UI and Iced background package-energy measurements

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

**One matched pair was measured successfully. It does not establish a power
winner.** Wind recorded approximately 10.015 W and Iced 10.001 W at the whole CPU package,
a Wind-minus-Iced difference of 0.014689 W (+0.1469%). The planned two-pair
comparison did not complete. Both failed attempts and their original data are
retained; failed and partial captures are not counted as completed trials.

The [portable evidence](../benchmark-results/archive-20261028/benchmark/ui-framework-power-measured-results-20261025/README.md)
contains raw readings, frozen source/protocol snapshots, checksums, application
and native-state checks, failure records, cleanup evidence and an offline replay.
This supersedes the earlier [meter/readiness-only attempt](ui-framework-power-20261024.md)
for the narrow question of whether package energy while the applications ran
was measured.

## Accepted measurements

| Capture | Meter duration | Package energy | Average package power | Host CPU, machine capacity | Sampler CPU, one-core capacity |
| --- | ---: | ---: | ---: | ---: | ---: |
| Attempt 1 opening baseline | 119.994 s | 1198.843736 J | 9.990864 W | 18.329% | 1.745% |
| Attempt 1 Wind UI | 180.058 s | 1803.359653 J | **10.015438 W** | 37.460% | 3.409% |
| Attempt 1 Iced | 180.012 s | 1800.254692 J | **10.000748 W** | 39.129% | 3.201% |

These are Windows-reported **CPU-package** readings including other applications,
the sampler and the disposable workers. They are not watts attributable to
Winderust alone, whole-device power, discrete-GPU power or wall power. Baselines
are context; they are not subtracted to manufacture per-application watts.

Both accepted application trials had 181 populated samples and 180 positive
meter-time intervals. Every saved sample has successful collection/get status
and valid counter status for Energy, Time and Power. Both passed native
prepare/readiness/completion checks, process restoration and cooperative worker
cleanup. Settings digests match exactly. At every sampled window check, Wind
had no native UI window and Iced's window was hidden; checks were at 1 Hz.

The same accepted sampling windows provide these application-plus-recovery-helper
measurements. External workers and the sampler are excluded from this scope.

| Metric | Wind UI | Iced |
| --- | ---: | ---: |
| Combined CPU cycles | 3,981,695,192 | 4,345,245,011 |
| Combined cycles per second | 22,070,708 | 24,095,287 |
| CPU, one-core capacity | 0.8921% | 0.9964% |
| CPU, whole-machine capacity | 0.07434% | 0.08303% |
| Private commit, median / p95 | 18.36 / 19.08 MiB | 28.85 / 28.97 MiB |
| Private resident RAM, median / p95 | 6.23 / 6.90 MiB | 15.97 / 16.05 MiB |

A [separate offline recomputation](../benchmark-results/archive-20261028/benchmark/ui-framework-power-measured-results-20261025/independent-metrics.json)
matches the raw main/helper/combined accounting; the portable replay verifies
that secondary artifact's hash and independently recomputes package energy. The
collector's combined rate divides summed counter increments by the sum of the
maximum member interval at each sample (180.406320 s for Wind, 180.335891 s for
Iced), rather than by the package meter's time. This convention and asynchronous
process reads explain small differences from endpoint-span rates. Wind used less
memory and fewer cycles in this one pair; these process observations do not
establish a repeatable framework ranking or isolate per-process energy.

## Workload and builds

The condition was `enabled-no-match`: production Process Priority scheduling
enabled, a rule targeting a nonexistent executable, and four identical external
workers in idle mode. Their periodic heartbeats still perform work. No target
priority, affinity, efficiency state or power plan change was requested. This
measures background observation/scanning, not every scheduler or active target
actuation. Fixtures keep settings in memory and cross-session control off.

The matched V3 builds share the audited control algorithms and settings from the
[scheduler comparison](ui-scheduler-resource-comparison-20261020.md). They use
**Wind UI 0.24.0 / tiny-skia software** and **Iced 0.14 / tiny-skia software**.
Their executable SHA-256 values are respectively
`8f44a67a6050ed09c73917abe50cb4f5ad916a853c1a5855544712ad02577fe7`
and `2d040026dae4b842a3853f5503817a15a00abd097f24e4a681749aa6976d2b82`.
Neither application executable was rebuilt or modified for this measurement.
Current production Wind UI 0.28 also includes later engine changes; these data
do not rank that version against the older frozen Iced build.

Each frozen protocol requested 180 seconds settling, 180 seconds measurement at
1 Hz, two pairs in Wind/Iced then Iced/Wind order, and 120-second no-app baselines
before and after. Engine fixtures ran with an actual administrator token.
Shared host load was observed and retained; no quiet-host gate, outlier removal,
or result-dependent extra trial was used within either attempt. Short pilots
are readiness/collector checks and are excluded from the comparison.

The host was an Intel Core 5 210H, 8 cores / 12 logical processors, approximately
15.61 GiB physical RAM, with Intel graphics and an RTX 4050 Laptop GPU. AC was
online throughout the accepted windows. The original Balanced plan remained
selected. The hardware identity and per-sample load/power context are retained.

## Meter interpretation

The saved Windows counter metadata identifies `RAPL_Package0_PKG`: Energy is in
picowatt-hours, Time in milliseconds, and Power in milliwatts. Replay calculates:

```text
joules = sum(valid consecutive Energy differences) * 3.6e-9
meter_seconds = sum(valid consecutive Time differences) / 1000
average_package_watts = joules / meter_seconds
```

No CPU-cycle-to-watt conversion is used. The first opening baseline and accepted
pair contain 483 package samples: cumulative energy/time advance throughout,
all reported power values differ, and observed energy follows a 61 microjoule
grid with millisecond meter time. Power ranges are 8.756 to 11.532 W for baseline,
9.079 to 11.488 W for Wind, and 8.770 to 11.300 W for Iced. These checks rule out an
obviously flat or stale saved series. Agreement between reported power and
energy/time differences is internal consistency, not independent calibration.
No external power meter or battery-discharge experiment was performed.

One pair under different shared-host load cannot establish a stable energy
ordering. The 1 Hz readings are correlated observations, not 180 independent
trials. Neither attempt obtained its closing baseline or a completed reversed
pair, so there is no completed AB/BA result, defensible significance claim,
baseline-drift estimate, or battery-life prediction. The second attempt's
opening baseline was 1201.352788 J over 119.989 s, or 10.012191 W. Its difference
from the earlier baseline is context about separate host conditions, not an
uncertainty bound or a substitute for either missing closing baseline.

## Failed attempts and corrections

1. `power-20261024` completed the opening baseline and first Wind/Iced pair.
   Its second Iced run failed before warmup/sampling while opening `ready.json`
   with `PermissionError` / errno 13. A separate disposable worker exited after
   a heartbeat-file replacement sharing error. The failed Iced run contains no
   saved energy window. Iced subsequently reported clean coordinated shutdown.
2. The collector's readiness-only retry was extended to recognize `EACCES` and
   `EPERM` when Python supplies no Windows error number, within its existing
   deadline. The disposable worker received at most ten 20 ms publication
   retries for exact access/sharing/lock errors. Persistent failures still
   produce an error sidecar and nonzero exit. Four native held-file tests
   verified recovery or visible failure and cooperative cleanup outside the
   sandbox. The new worker and collector were frozen for a separate attempt.
3. `power-20261025` passed both short pilots and its opening baseline, then
   rejected the first long Wind run when a `WindUiWindowClass` window for the
   measured process became visible. The partial CSV retains 123 complete
   process samples reporting window absence and a following package sample at
   123.287486 seconds. It is not a valid full background capture. No later
   formal application trials or closing baseline ran.

The failing window check did not save the HWND/title or a separate exception
timestamp. Archived source has tray-availability recovery and explicit show
paths that can reopen the UI; the files do not prove which path ran. The fixture
reported cleanup near its requested 363-second deadline. Available sample
timing places the failure earlier, but cannot exclude a later unrecorded stall.
Increasing fixture lifetime or accepting visible windows would not repair a
demonstrated cause. This observation concerns the frozen 0.24 fixture and is not
evidence of a current 0.28 defect.

All accepted trials restored and stopped their workers normally. For attempt
1's failed Iced run, the remaining three workers were verified against their
original native state and stopped cooperatively; the already-exited fourth
worker's final native state is explicitly unverified. For attempt 2, all four
workers matched their original state and exited cooperatively. All original
workers are gone, the power plan is unchanged, and no force-kill was used.

## Practical conclusion

This measurement provides actual package-energy numbers, while retaining the
limits that prevent a framework power ranking. It supplies no evidence that
switching between these UI frameworks materially reduces Winderust's background
power. The failed lifecycle case also needs better event/window diagnostics
before a long background comparison can be repeated reliably.

## Repository validation

The readiness fix passed 60 non-native collector tests; the energy forwarding
change passed 17 scheduler-orchestrator tests. Four native worker publication
cases passed outside the sandbox, including persistent failures, with all test
workers exited. Windows lock-error 33 was not separately induced. The current
repository passed formatting, locked all-target Clippy with warnings and unsafe
operation lints denied, 885 Rust tests (18 ignored), the legacy-name scan and
`git diff --check`. These repository checks do not turn rejected captures into
accepted trials or test current Wind 0.28 power consumption.
