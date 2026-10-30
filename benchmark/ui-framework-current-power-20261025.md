# Current-engine Wind UI and Iced background measurements

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

**Wind used less private memory; enabled-scheduler CPU results were mixed, and
there is no reliable package-power winner.** Six original captures passed the
strict replay checks. Two continuation captures completed, but Iced failed the
sampler timing limit, so the interrupted comparison is not a completed formal
eight-trial cohort. Their separately audited process measurements are diagnostic.

The [portable evidence](../benchmark-results/archive-20261028/benchmark/ui-framework-current-power-results-20261025/README.md)
retains raw readings, both protocols, build/source snapshots, failed attempts,
cleanup proof, strict rejection and a separate file-only diagnostic replay.

This comparison uses **Wind UI 0.28.0** and **Iced 0.14.0**, both with software
rendering and the same current shared engine. It supersedes the older
[Wind UI 0.24 power comparison](ui-framework-power-measured-20261025.md) for this
specific build comparison; those older observations are retained separately.

## Captures

Each formal capture follows 180 seconds settling and measures for 180 seconds at
a nominal 1 Hz. Cycles and memory below include Winderust and its recovery helper. The
external disposable workers, sampler, launcher and observer are excluded from
those process metrics. Package power includes all software on the CPU package.

| Condition / pair | Framework | Cycles/s | Private commit, median MiB | Private resident RAM, median MiB | CPU-package W |
| --- | --- | ---: | ---: | ---: | ---: |
| Hidden idle / 1, Wind first | Wind UI | 47,770 | 18.301 | 6.273 | 9.976896 |
| Hidden idle / 1 | Iced | 100,155 | 28.363 | 12.707 | 9.985384 |
| Scheduler enabled / 1, Wind first | Wind UI | 36,877,102 | 18.301 | 6.301 | 10.013718 |
| Scheduler enabled / 1 | Iced | 31,728,762 | 28.980 | 11.203 | 10.011167 |
| Scheduler enabled / 2, Iced first | Iced | 28,944,407 | 28.520 | 15.957 | 9.976352 |
| Scheduler enabled / 2 | Wind UI | 23,763,515 | 18.320 | 6.117 | 9.971807 |

Private commit measures private committed address space; private resident RAM
measures its resident physical pages. They answer different memory questions.
The evidence also retains main/helper/combined CPU time, cycles, working sets,
memory quantiles and lifetime peaks. Lifetime peaks are not sampling-window
peaks. Combined working sets can count shared pages more than once.

The first idle Wind capture recorded zero CPU-time increments at that counter's
resolution while recording nonzero cycles. It is not evidence of zero execution
or zero power. Cycles come from `QueryProcessCycleTime`, which includes user and
kernel execution for the process's threads. See
[Microsoft's API contract](https://learn.microsoft.com/en-us/windows/win32/api/realtimeapiset/nf-realtimeapiset-queryprocesscycletime).
The replay recomputes the collector's interval denominators from raw records;
asynchronous process reads make these slightly different from package-meter
elapsed time.

## Workloads and interpretation

- **Hidden idle (`disabled`):** automation disabled, with the UI absent or hidden.
- **Scheduler enabled (`enabled-no-match`):** Process Priority enabled with a
  rule for a nonexistent executable. The production engine observes processes;
  no target priority, affinity, efficiency or power-plan change is requested.
- Four identical owned workers run in idle mode for each application capture.
  Their heartbeats still execute periodically. Native identity and state checks
  accompany every accepted trial, followed by cooperative exit checks.

The enabled condition measures observation and rule evaluation. It does not
measure all schedulers, a busy CPU Limiter target, foreground interaction or
active process-control savings. Previous broad scheduler measurements remain in
the [scheduler report](ui-scheduler-resource-comparison-20261020.md); their older
builds must not be pooled with this comparison.

In the completed scheduler pairs, Wind's cycles/s were **16.23% higher**, then
**17.90% lower**, than Iced. A consistent scheduler CPU winner is unsupported.
Wind's private commit and private resident RAM were lower in each of the three
completed original pairs. The validated idle pair recorded **52.30% fewer
cycles/s** for Wind. The continuation's process counters showed the same
direction, but its whole-capture timing rejection prevents claiming a completed
formal two-pair idle comparison.

| Separate idle continuation, Iced first | Cycles/s | Private commit, median MiB | Private resident RAM, median MiB | Package W |
| --- | ---: | ---: | ---: | ---: |
| Iced, sampler interval rejected | 79,618 | 28.324 | 15.555 | 9.985852 |
| Wind UI, individual capture passes | 40,833 | 18.063 | 6.047 | 9.980181 |

The process counters, native/settings checks and package counter statuses pass
their separate diagnostic checks for both continuation captures. All 181 sample
points per audited scope exist and timestamps are monotonic. Iced's sampler clock advances from
92.2503259 to 95.1766124 seconds in one interval: **2.9262865 seconds exceeds the
unchanged 2.5-second gate**. Its maximum package/context/main/helper intervals
remain below 2.5 seconds. Both applications' CPU-time increments round to zero;
their cycle counts remain nonzero. The descriptive idle cycle difference is
-48.71%, but the strict combined replay returns `accepted: false` and exits 1.
No gate is relaxed and these rows are not pooled into the accepted cohort.

The shared engine's hidden Process Priority observation cadence is ten seconds
when event-driven invalidation does not request an earlier observation. A due
appearance scan uses the process-control inventory, including session, creation
identity, critical/protected-process and access checks. Priority evaluation
shares that inventory within a pass. Disabled automation can stop its worker.
This source evidence explains why an unmatched enabled rule is a different
workload from disabled idle; it does not quantify the share of measured cycles
attributable to those scans or either renderer. The
[frozen-source audit](../benchmark-results/archive-20261028/benchmark/ui-framework-current-power-results-20261025/build-scope-audit.md)
records implementation paths and adapter differences.

For further optimization, profile these shared process inventory and event
invalidation paths before changing frameworks. A future experiment could examine
redundant observations or identity-safe metadata reuse while retaining all
process safety checks. No saving is established by this comparison, and no
production engine optimization was made during this measurement.

## Power scope and host conditions

The meter is Windows' `RAPL_Package0_PKG` CPU-package channel. Replay calculates
joules from cumulative Energy differences in picowatt-hours and divides by
cumulative Time differences in milliseconds:

```text
joules = sum(valid Energy differences) * 3.6e-9
average_package_watts = joules / (sum(valid Time differences) / 1000)
```

This follows the cumulative energy/time approach described by
[Microsoft's Energy Meter Interface documentation](https://learn.microsoft.com/en-us/windows-hardware/drivers/powermeter/energy-meter-interface).
Successful statuses, monotonic counters, interval coverage and nonflat readings
are checked from the saved raw data. These establish internal consistency, not
independent calibration.

The original three paired Wind-minus-Iced package differences were
**-0.008488 W**, **+0.002551 W** and **-0.004545 W**. They do not isolate Winderust's
own power. Other applications, the sampler, observers and workers contribute to
the same package reading. No baseline subtraction or cycle-to-watt conversion is
used. There is no wall-power, whole-device, discrete-GPU wattage or battery-life
measurement in this comparison.

The continuation's descriptive package difference is -0.005671 W. Its two
strictly valid no-app baselines are 9.964476 W before and 9.973568 W after
(+0.009092 W). They provide context for that segment only. They are not an error
bound, a substitute for the missing original closing baseline or application
power to subtract.

The shared host remained under its current load; no quiet-host gate or outlier
deletion was used. In the six original captures, host CPU averaged approximately
34.0–35.7% of whole-machine capacity. The sampler alone consumed approximately
3.0–3.5% of one logical CPU, more cycles than the idle application. Its CPU is
excluded from application accounting but remains in package watts. The small
package differences cannot establish a reliable framework energy winner.
Continuation host CPU averages approximately 40.15–40.17% during the two trials;
its baseline host averages change from 36.76% to 21.60%. These varying host
conditions further limit interpretation of the small package-power differences.

The host identifies an Intel Core 5 210H, eight cores / twelve logical processors,
approximately 15.61 GiB physical RAM, Intel graphics and an RTX 4050 Laptop GPU.
AC status is sampled throughout; the original Balanced plan is verified at
recorded native prepare, ready and finished checks.
One-second observations are correlated samples within a trial, not 180
independent repetitions. The two accepted enabled-scheduler pairs and the idle
observations are descriptive, not a general framework ranking or a significance
test.

## Build parity and lifecycle

The frozen source inventories identify **112 byte-identical shared-engine Rust
files**, identical normalized settings and scheduler/runtime fixtures, the same
compiler and direct shared-engine dependency pins, locked builds and the same
release profile. The older Iced UI is adapted to the current engine; native UI,
tray and window adapters differ. Its hidden transition publishes to the current
shared visibility state. The renderer dependency trees also differ, including
tiny-skia 0.12 for Wind and 0.11 for Iced. Shared source does not mean identical
machine code or full dependencies. These are application builds, not empty
framework demos.

Both builds use Rust 1.94.1, thin LTO, one codegen unit, stripped symbols and the
opt-in `render-smoke,runtime-benchmark` features. Settings remain in memory,
startup registration is skipped, cross-session control is off and target control
is restricted to disposable owned workers.

| Build | Executable bytes | SHA-256 |
| --- | ---: | --- |
| Wind UI 0.28 | 12,776,448 | `c6e4bcfcc3846e6c2a715f27f67007a287abc043c68040cb2e76a9fe3736f2ea` |
| Iced 0.14 | 19,415,552 | `f52f100924eb991f081f0b6166c3e1d70a86534c468f742ffbc406420722b9bc` |

Executable sizes include those benchmark features. Fixture `source_commit`
fields have different historical meanings between adapters; engine parity is
established by source inventories and preparation records, not those fields.

A separate seven-minute Wind UI lifecycle probe passed with **421 sampled absent
main-window states**, clean shutdown and no runtime-worker error. It ran while the
Iced build was compiling and is excluded from performance comparisons. Accepted
formal captures also require the expected main-window state at every sampled
check, using the configured Wind UI window class or Iced main HWND filter. This
does not continuously inventory every process window or rule out unsampled brief
transitions. The prior archived Wind UI
0.24 window failure was not reproduced. Its exact historical cause remains
unproven because the old failure lacked the gated HWND/title diagnostics.

## Interrupted original cohort and continuation

The original fixed sequence planned eight trials: Wind/Iced for idle and enabled
scheduling, then Iced/Wind for enabled scheduling and idle. Six completed. The
seventh failed in native preparation while reading worker four's heartbeat with
`PermissionError` / errno 13. The collector had not reached Winderust's `Popen`;
there is no application readiness record or resource CSV for that attempt.

All four leftover workers were checked against their PID, creation FILETIME,
executable path and SHA-256 before cooperative stop requests. Their retained
handles confirmed zero exit codes; the power plan stayed unchanged. No
application actuator started in the failed attempt, so a restoration comparison
against a nonexistent native baseline is not claimed.

Only the harness heartbeat reader changed: transient access, sharing and lock
failures now use a 250 ms retry budget, capped by any enclosing deadline. OS
scheduling and filesystem-call latency are not interruptible I/O timeouts. Malformed
data and unrelated failures fail immediately; persistent denial remains an
error. Nineteen unit tests passed. A real exclusive Windows file lock reproduced
errno 13, recovered after a 120 ms hold and failed visibly after the 250 ms budget
under a persistent hold. No application binary or sampler changed.

The original protocol, sources, failure and strict eight-trial validator remain
unchanged. A separate declared continuation contains only the missing idle pair,
Iced then Wind, with fresh 120-second no-app baselines before and after. Its local
repeat 1 maps explicitly to comparison pair 2. The original closing baseline is
missing; the new baselines do not turn the interrupted sequence into an
uninterrupted cohort. Short readiness pilots are retained and excluded.

Both continuation collectors completed and all test workers exited. The frozen
strict replay then rejected the Iced sampler interval described above. That
rejection is preserved in `analysis.json`; `inspect_continuation.py` independently
audits the usable process/package records and retains all timing violations.
The portable README documents integrity verification, the expected strict
rejection and diagnostic replay. A final read-only process inventory found no
remaining application or worker processes at the three exact benchmark paths.

## Validation and practical choice

The harness passed **104 Python tests** covering sampling, scheduler orchestration
and native verification. Replay tests exercise acceptance and tampered/missing
evidence, while the bundle manifest checks every payload hash and inventory.
Repository validation also passed formatting, Clippy with warnings denied,
885 Rust tests (18 ignored), the legacy-name scan and whitespace checks.

These measurements give no CPU or power reason to switch the current Winderust
port to Iced. Wind's memory use is consistently lower in the observed pairs;
Wind used fewer idle cycles/s in the accepted pair and the diagnostic pair agreed. Enabled
engine costs and energy savings need their own profiling and controlled
experiments. Retaining Wind while investigating shared engine work is supported
by these results; claiming a universal framework or power advantage is not.
