# Background idle thread diagnostic — 2026-10-22

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

The [native idle cleanup comparison](ui-native-idle-cleanup-20261022.md) remains
the evidence for the measured optimization: its three balanced blocks showed a
74.19% median paired reduction in application-plus-helper cycles. This follow-up
investigates the remaining activity. It does not replace that comparison or
establish another performance ranking. The Wind UI dependency and both measured
fixture binaries are unchanged; no framework fork was used.

## Diagnostic protocol

One frozen Wind capture followed by one frozen Iced capture used a 15-second
warmup and 180 seconds of collection at 1 Hz. Each fixture requested a
198-second lifetime after readiness. Automation, update checks and startup
registration were disabled, with settings held in memory. Wind's UI HWND was
destroyed after production tray permission; Iced retained its HWND hidden.
Both fixtures exited cleanly through coordinated cleanup.

The observer added read-only per-thread cycle counters, creation identities and
thread descriptions to the existing process measurements. Polling can miss
short-lived threads, work before a thread's first observation, and work after
its last observation. Process and thread counters are also read at different
times. Thread descriptions identify metadata, not executing stacks or library
functions. Missing counters remain unavailable rather than being replaced with
zero.

The first 15 seconds after readiness are warmup and are not measured. Time
windows therefore describe the observed portion of the early period; counter
intervals crossing nominal readiness ages of 30, 60 or 120 seconds are kept
separately without prorating cycles. Readiness observation and individual
counter reads introduce small timing offsets.

## Observations

These are individual diagnostic captures, including the recovery helper and
excluding the separately attributed observer.

| Metric | Wind | Iced |
| --- | ---: | ---: |
| Main process cycles/s | 166,246 | 62,856 |
| Recovery helper cycles/s | 18,155 | 24,353 |
| Combined cycles/s | 184,397 | 87,198 |
| Main process cycle-active intervals | 29 / 180 | 12 / 180 |
| Combined private resident RAM, MiB | 6.20 | 12.30 |
| Combined private commit, MiB | 14.43 | 20.66 |
| Observer CPU, percent of one core | 15.59% | 18.30% |
| Thread collection latency, median | 132 ms | 157 ms |
| Full collection latency, median | 159 ms | 187 ms |

Private resident RAM is sampled private working set. Private commit is a
separate allocation measure. Counter horizons differ slightly by process role,
so displayed rates are independently computed rather than added after rounding.

Every named Wind thread recorded zero observed cycle deltas across 180 measured
intervals, including `main`, `winderust-resource-deadline`,
`winderust-tray-health`, `recovery-transport`, and the helper's `main` and
`winderust-diagnostics` threads. The named Iced runtime, dashboard, presentation
and recovery threads also recorded zero observed interval deltas. This supports
leaving these blocked workers intact; it does not prove they can never execute
outside this capture.

All 22,275,994 positive observed Wind-main thread cycles belonged to one unnamed
thread, active in 28 intervals. Positive observed Iced deltas likewise belonged
to unnamed threads. Their descriptions cannot establish whether Windows, a
runtime, an application callback or a framework function executed.

## Thread lifecycle accounting

Both applications again showed a burst near measurement second 15, approximately
30 seconds after readiness/startup. In Wind, the main process recorded
2,685,043 cycles and the helper recorded 1,608,768 cycles. Each total exactly
equals the cumulative counters of two newly observed unnamed threads. Iced's
helper burst of 1,989,655 cycles likewise equals its two new unnamed threads'
first cumulative counters. These counters had already advanced before the first
thread observation, so the frozen analyzer correctly excludes them from
interval-delta totals. The separate reconciliation explains their contribution
without changing those totals.

After separately accounting for those first cumulative counters, Wind's
remaining residual is 4,965,545 main cycles and 1,659,302 helper cycles. Those
totals exactly match process intervals
near measurement second 119, when four observed main-process threads and three
helper threads disappear. Iced's remaining main residual of 4,096,516 cycles
matches its disappearance/query-gap intervals near seconds 108 and 119; its
helper residual of 1,828,109 cycles matches the second-119 interval. This is
aggregate lifecycle-boundary reconciliation, not terminal counter readback or
proof that a particular exiting thread owned every cycle.

The startup-age burst appeared once in each run. Later activity was irregular:
86.66% of Wind's combined cycles occurred in the nominal-ready 120–195-second
window, and most positive observed Wind thread activity was between measurement
seconds 107 and 169. The former recurring five-second pattern did not return.
The fixture deadline worker itself recorded zero measured deltas, and its
completion deadline was outside the collection window.

## Interpretation and limits

This was a fixed Wind-then-Iced diagnostic with one capture per application and
substantial observer overhead. It is not a balanced comparison and must not be
pooled with the shorter, uninstrumented cohorts. The observer's CPU cost is
reported separately, without subtraction; observing threads can still affect
scheduling and residency.

The desktop was shared and active. Median machine CPU was 23.16% during Wind and
35.19% during Iced; the respective p95 values were 46.55% and 58.09%. Median
memory load was 86% and 94%, with p95 values of 90% and 96%. Different host
conditions and the diagnostic observer prevent a causal framework comparison.

Process CPU time is coarse enough to quantize despite positive cycle counts.
Process cycles do not measure retired instructions, watts or battery savings.
Page faults do not establish disk reads. No GPU, energy, enabled-scheduler or
long-term-growth conclusion follows from this diagnostic.

The findings narrow the remaining investigation to unnamed-thread activity and
thread lifecycle boundaries. They do not establish an original Wind UI defect
or justify removing recovery, tray or completion workers. Executing stacks are needed before choosing another optimization; the separate
entry-module diagnostic below narrows identity without providing those stacks.

## Inspectable evidence

Public exports are [summary JSON](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/summary.json),
[process scopes](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/scopes.csv),
[time windows](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/windows.csv),
[cycle-active intervals](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/active-intervals.csv),
[thread summaries](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/threads.csv),
[thread activity](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/thread-active-intervals.csv),
[thread windows](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/thread-windows.csv), and
[lifecycle reconciliation](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/lifecycle-reconciliation.json).

Raw captures, the prespecified configuration and the frozen analyzer are under
`target/idle-thread-diagnosis-20261022/`. The completed pilot is schema/launch
validation only and is excluded from every diagnostic total above.

## Retained-thread follow-up

A separate frozen Wind-only capture used the same 15-second warmup, 180-second
measurement horizon and 198-second ready-relative fixture lifetime. It retained
limited-query thread handles until exit, sampled counters at 1 Hz, and discovered
threads every five seconds. A one-time, separately opened query-information
handle supplied optional thread entrypoints after limited-query access was
denied; owner and creation identity were revalidated and each extra handle was
closed immediately. No scheduling, settings, framework or application source
changed. Retained handles and inventories still add observer effects.

All measured process-cycle increases reconcile exactly:

| Contribution | Main cycles | Helper cycles | Combined cycles |
| --- | ---: | ---: | ---: |
| First cumulative counters of new threads near readiness age 30 s | 1,943,961 | 3,704,871 | 5,648,832 |
| Final counter increases of exiting threads near age 134 s | 8,055,133 | 3,691,699 | 11,746,832 |
| Measured process increases | 9,999,094 | 7,396,570 | 17,395,664 |

The strict measurement subset contains 177 intervals covering 178.701 seconds;
its combined diagnostic rate is 97,345 cycles/s. The observer itself used
2.47% of one core over this subset, with collection latency median 2.62 ms and
p95 128.17 ms. Across the whole 198.899-second probe it consumed 5.016 CPU
seconds (2.52% of one core). These are observer-influenced attribution values,
not another framework comparison or a regression against the lightweight
balanced trials.

First cumulative counters are separate lifecycle accounting, not invented
interval deltas. Eight threads with positive observed measurement deltas all
mapped to the same entrypoint, `ntdll.dll+0x24990`, in both processes; each
positive delta was a final read. Named Winderust workers, unnamed
application-entry threads, `inputhost.dll` entry threads and `SophosED.dll`
entry threads recorded zero strict measurement-period deltas in this run.

The locally inspected Sophos DLL is signed by Sophos Limited and identifies
itself as Sophos Endpoint Defense In-Process Protection. Its presence is an
additional host condition, not evidence that it caused this run's measured
cycles. Neither these module mappings nor this separate capture identify the
unnamed active thread from the earlier capture. Entry modules identify thread
initialization; callbacks and other libraries may execute later on that thread.
There are no sampled executing stacks or thread-creator traces.

All 23 discovered thread instances have valid final counters. Over the entire
process lifetime, including startup and shutdown, the helper's observed thread
sum equals its 108,024,263 process cycles. The main-process thread sum is 380,836,368
against 383,126,005 process cycles, leaving 2,289,637 lifetime cycles
unattributed. Threads born and exited between inventories can still be missed.
Whole-lifetime totals must not be presented as the background measurement.
Boundary-crossing intervals remain separate, and rates use actual covered
interval durations rather than nominal wall time.

The capture completed with zero diagnostic errors, disabled automation and
updates, in-memory settings, one validated recovery helper, and no settings
HWND observed in the 1 Hz post-readiness native checks. The fixture completed its full requested lifetime and
cooperative cleanup. This additional attribution capture is not an Iced
comparison and is not pooled with any earlier cohort.

## Source and validation

The [dependency proof](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/dependency-proof.json)
records the unchanged Wind UI 0.24.0 commit
`36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2`, pristine tracked dependency source,
and identical manifest pin. Cargo's empty untracked `.cargo-ok` cache marker is
not a source edit. Final hashes also confirm unchanged Winderust application
sources, Cargo manifest/lock, both frozen fixtures and the production binary.
Only diagnostic tooling and documentation changed during this investigation.

The opt-in collector's 42 focused tests and retained probe's four pure tests
passed. Default locked Rust tests passed 871 tests with 18 ignored; strict
Clippy, formatting, whitespace, legacy-name and architecture ownership checks,
and the AST graph update passed. The ignored Windows integration tests were
not run. No application process or portable TOML settings file remained after
the fixtures; the original Balanced power plan remained selected.

The final production binary remains SHA-256
`5c7dcfd5042d3b05188052b51a6ada74ca0c1b7fc4222839ebf529f1600b391c`.
No production rebuild or framework patch was made for these measurements.

Thread counters use documented
[QueryThreadCycleTime](https://learn.microsoft.com/en-us/windows/win32/api/realtimeapiset/nf-realtimeapiset-querythreadcycletime).
The optional entrypoint query uses dynamically loaded
[NtQueryInformationThread](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntqueryinformationthread),
which Microsoft marks internal and subject to change. Its numeric class 9 and
observed access requirement remain diagnostic compatibility limits; this is
not a new runtime dependency. Module ranges use
[Module32FirstW](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-module32firstw)
and are timestamped discovery-batch snapshots.

Retained follow-up exports include the [validated summary](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/retained/summary.json),
[phase accounting](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/retained/process-phases.csv),
[entry modules](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/retained/entrypoint-modules.csv),
[thread lifetimes](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/retained/threads.csv),
[positive process intervals](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/retained/positive-process-intervals.csv),
[positive thread intervals](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/retained/positive-thread-intervals.csv),
and [reconciliation](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/retained/reconciliation.csv).
The [module identity proof](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/module-proof.json)
and [final source hashes](../benchmark-results/archive-20261028/benchmark/ui-idle-thread-diagnosis-results-20261022/after-provenance.json)
provide independent provenance. The retained analyzer's three pure selfchecks
also passed. Executing CPU stacks, thread creators, energy use and enabled
scheduler behavior were not measured by this follow-up.
