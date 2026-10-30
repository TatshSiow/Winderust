# Winderust: Wind UI and Iced resource measurements

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

Measured on 19–20 October 2026, Asia/Taipei (UTC+08:00). This report compares the
current Wind UI port with the complete historical Iced application, using
isolated release builds and the same Windows machine. The results describe these
two application implementations, including their current window lifecycle and
page composition.

Wind UI saves memory when normal close-to-tray disposes of its window. Iced uses
less idle CPU and used less CPU in every matched trial of six active workloads;
Wind used less CPU in every repeated-navigation trial. There is no universal
performance winner. The closed-window CPU difference is small in absolute terms,
while the memory saving is repeatable in the five-trial private-commit comparison.

## Builds and environment

| Item | Configuration |
| --- | --- |
| CPU | Intel Core 5 210H; 8 cores, 12 logical processors |
| Installed usable memory | 16,763,805,696 bytes reported by Windows |
| Windows | Build 26200, x64 |
| Graphics | Intel Graphics 32.0.101.7076; NVIDIA RTX 4050 Laptop 32.0.15.8142 |
| Windows power plan | Balanced; unchanged by the fixtures |
| Rust | 1.94.1, x86_64-pc-windows-msvc |
| Release profile | Thin LTO, one codegen unit, stripped symbols |
| Wind UI | 0.24.0, revision `36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2` |
| Iced | Historical application at `3d329a9c57845bfbb2252e26f361e4d87bbc8b95`; Iced 0.14 |
| Renderer | Both use software rendering through tiny-skia |
| UI | English, dark theme, expanded navigation, advanced controls enabled |
| Window | 1120 × 760 logical client units, normalized at the actual monitor DPI |

The Wind UI fixture is 12,478,976 bytes, SHA-256
`2f8e4c37af72be57a25a9db3291b217c3973a5f5c6c92e21306016b2e50fd7e4`.
The Iced fixture is 19,305,472 bytes, SHA-256
`2c6755e6772067e33fa3b58ac48b51d430ebea4c3ca12f80a58d720593ea5052`.
These sizes include the opt-in benchmark harness in both builds.

Automation, update checks, startup registration, and user settings writes are
disabled. The real application model, read-only live process queries, resident
shell, runtime, and recovery helper remain present. This measures the UI and
resident application overhead; it does not measure enabled process-control rules.

## Protocol

The planned order alternates frameworks and rotates scenarios between repeat
blocks. Resumption preserves completed clean trials and retries rejected attempts,
so actual collection order includes interruption gaps. Each trial launches a
fresh process, waits for verified native readiness, warms up, samples every
500 ms, and requires cooperative exit plus successful
runtime/recovery cleanup. CPU and cycle rates use actual elapsed sample intervals.
All builds and test jobs finish before measurement starts.

Background states completed five paired trials with a five-second warmup and
20-second sampling window on 19 October. All seven active workloads completed
three paired trials using a three-second warmup and 12–20-second sampling window.
The first 16 active runs were collected on 19 October; the remaining 26 completed
on 20 October after the user unlocked Windows. Each active workload has one pair
from 19 October and two from 20 October, except Language and Appearance with two
and one respectively. Separate diagnostics check growth and instrumentation
overhead. A trial is the independent repeat; 500 ms samples within a trial are
not independent experiments.

| Scenario | Workload |
| --- | --- |
| Background after closing UI | Production close-to-tray behavior: Wind UI destroys the main HWND; Iced retains a hidden HWND |
| Hidden, retained | Both keep their main HWND hidden |
| Minimized | Both retain a minimized main window |
| Home | Visible live dashboard |
| Language and Appearance | Visible settings page |
| Process List | Visible live process inventory and sampling |
| Navigation | Home → Process List → Language and Appearance → About, scheduled every 250 ms |
| Scroll | Process List wheel input every 100 ms, reversing direction every 15 messages |
| Resize | Home alternates between 1000 × 700 and 1120 × 760 client units every 500 ms |
| Window reopening | Production close/reopen, alternating every three seconds |

Visible workloads require foreground ownership. The collector parks the pointer
inside the fixture and restores it afterward. Both frameworks were verified to
scroll actual table rows before the measured runs. Hidden and minimized states
are checked against native Windows state. Window reopening re-enumerates the
owned HWND because Wind UI creates a new one.

The 20 October sessions temporarily requested
`ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED` for the wrapper's own
thread and restored the prior request in `finally` after each attempt. No global
power setting changed. Original display power state was not recorded on
19 October. Session dates and collector versions remain separate in the results;
the two days are not treated as one identical desktop condition.

The background scenario deliberately includes each application's current window
lifecycle. The retained-hidden scenario helps separate that decision from the
cost of keeping an allocated UI. These are application-port comparisons; page
composition and rendering invalidation also affect the result.

## Metric interpretation

- **CPU:** user and kernel time, milliseconds per second, and percentage of one
  logical CPU. A value of 100% means one fully occupied logical CPU. Very short
  idle intervals can quantize to zero in Windows CPU-time accounting.
- **Cycles:** `QueryProcessCycleTime`, including user and kernel execution across
  the process's threads. Raw cycles are reported alongside CPU time. They are
  not retired instructions or a fixed-frequency work measure.
  [Microsoft API reference](https://learn.microsoft.com/en-us/windows/win32/api/realtimeapiset/nf-realtimeapiset-queryprocesscycletime).
- **Memory:** private commit, total working set, observed and lifetime peaks,
  pagefile commit, paged/nonpaged pool, faults, and growth. Private commit is not
  a physical-RAM-only measure. A working set includes shared resident pages;
  adding main/helper working sets can double-count pages. Page faults include
  soft faults. Separate EX2 diagnostics add private working set: private resident
  RAM that can be summed across these processes without counting shared pages
  twice. Shared resident DLL/code pages still have a physical cost outside that
  private number. Unsupported EX2 fields remain null.
  [Windows memory counters](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters_ex),
  [EX2 private working-set counter](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters_ex2).
- **GPU:** per-process engine activity and dedicated/shared allocation counters,
  with availability recorded explicitly. Utilization uses the busiest engine.
  DWM is recorded separately because it composes the entire desktop. Process GPU
  memory is not summed across processes, since allocations can overlap.
  [Microsoft's GPU accounting explanation](https://devblogs.microsoft.com/directx/gpus-in-the-task-manager/),
  [documented GPU memory counter limitation](https://learn.microsoft.com/en-us/troubleshoot/windows-client/performance/gpu-process-memory-counters-report-wrong-value).
- **I/O:** read, write, and other operation/byte counts. These include cached and
  device I/O; they do not directly measure physical disk traffic.
  [GetProcessIoCounters](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getprocessiocounters).
- **Resources:** handles, threads, GDI/USER objects, descendants, and changes over
  time. Main process and recovery helper are retained separately and combined
  where summation is meaningful.
- **Energy and system context:** RAPL package energy, package watts, total system
  CPU load, available memory/commit, A/C state, battery percentage, and DWM activity.
  The package meter includes the rest of the system and collector. It cannot
  establish per-app watts or battery-life improvement in this desktop session.
- **Startup and shutdown:** first observed HWND, visibility, fixture readiness,
  lifetime execution cost, and cleanup. Startup uses warm OS caches. Readiness
  differs between frameworks and does not establish first meaningful paint.
- **Observer cost:** the collector's own CPU, cycles, memory, and collection time
  are recorded. Detailed thread counters are collected separately because their
  PDH enumeration adds substantial observer cost.

Resource percentiles describe resource samples, not input latency or frame time.
Short-run growth is a diagnostic signal; bounded stability cannot prove the
absence of leaks. Native input message counts are reported because slow collection
can affect achieved input cadence. Navigation action counts span the entire
fixture lifetime, including warmup and its shutdown grace period.

Process List uses the live system inventory. The collector did not retain the
system process count, so per-row cost cannot be normalized. Neither these runs
nor successful native input validate full application feature parity.

Not measured: retired instructions, IPC, cache/branch misses, hardware wakeups,
per-core residency, physical-disk traffic, hard faults, first meaningful paint,
frame times/FPS, input-to-display latency, individual heap allocation stacks,
per-app GPU composition attribution, or per-app energy. Those need different
instrumentation and controlled experiments; resource percentiles cannot stand
in for them.

## Background results

These are medians of five independent trials per framework and state. Memory is
MiB. CPU time is milliseconds per second across the main process and its recovery
helper; 10 ms/s equals 1% of one logical CPU.

| State | Wind CPU ms/s | Iced CPU ms/s | Wind cycles/s | Iced cycles/s | Wind private commit | Iced private commit |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Closed to tray | 0* | 0* | 760,000 | 3,440 | 14.1 | 20.7 |
| Hidden window retained | 0.78* | 0* | 5,130,000 | 3,950 | 21.4 | 20.7 |
| Minimized | 5.47 | 5.47 | 12,600,000 | 15,200,000 | 24.7 | 25.1 |

\* Coarse Windows CPU-time accounting can report zero despite nonzero cycle
counts. The hidden Wind CPU-time trials ranged from 0 to 4.69 ms/s. Prefer the
absolute cycle rates over ratios against Iced's near-zero denominator. These are
OS execution-cycle counters, not retired instructions.

Closing Wind's actual window saves about 6.6 MiB of private commit relative to
Iced's retained window. Keeping both windows allocated removes that advantage.
Main-process total working-set medians were 26.68 versus 33.84 MiB after closing,
and 33.47 versus 34.21 MiB while hidden. These include shared resident pages.
The recovery helper costs approximately 6.2 MiB of private commit in either build
and is included in the process-tree numbers.

| Closed-to-tray resource | Wind UI | Iced |
| --- | ---: | ---: |
| Process-tree handles | 520 | 565 |
| Process-tree threads | 17 | 33 |
| GDI objects | 15 | 16 |
| USER objects | 24 | 28 |
| Page faults/s, including soft faults | 2.15 | 0 |
| Steady-state read/write bytes/s | 0 / 0 | 0 / 0 |

Lower thread counts describe footprint; idle threads need not consume CPU.
Wind's steady-state saving also does not mean lower startup peaks:

| Main-process peak or resource counter, closed trials | Wind UI | Iced |
| --- | ---: | ---: |
| Lifetime peak working set | 34.99 MiB | 33.84 MiB |
| Lifetime peak commit | 16.92 MiB | 14.43 MiB |
| Paged pool quota usage | 324.06 KiB | 294.62 KiB |
| Nonpaged pool quota usage | 18.75 KiB | 20.34 KiB |
| Lifetime peak GDI / USER objects | 25 / 29 | 18 / 28 |

These are medians of per-trial main-process counters. Lifetime peaks include
initial UI construction before closing. They exclude the helper and are not
simultaneous process-tree peaks. Pool quota counters do not measure total system
kernel memory.

Warm-cache first-HWND observation medians were 142 ms for Wind and 115 ms for
Iced, with 20 ms polling resolution and overlapping ranges. Cleanup callback
medians were 21.4/20.5 ms. Neither measures first meaningful paint or complete
native-loop teardown. Wind's closed-fixture readiness is about five seconds
because it waits for its resident timer to verify disposal; that is a benchmark
setup wait, not a five-second application startup time.

## Active workloads by session

All seven workloads have three matched pairs in total. The table preserves the
19 October collector A and 20 October collector C cohorts. Cells show the median
[minimum, maximum] of independent trial summaries within that session; a single
trial appears without a range. CPU is percentage of one logical core, cycles are
millions per second, and memory is process-tree private commit in MiB. CPU/cycle
rates use elapsed-time weighting within each trial. Memory summarizes each
trial's temporal median. No daily medians are averaged together.

| Workload | Session / collector | Pairs | Wind CPU % | Iced CPU % | Wind Mcycles/s | Iced Mcycles/s |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Home | 19 Oct / A | 1 | 5.08 | 2.03 | 126.25 | 60.61 |
| Home | 20 Oct / C | 2 | 4.80 [4.69, 4.92] | 2.34 [2.11, 2.58] | 115.95 [110.10, 121.81] | 61.53 [59.66, 63.40] |
| Language and Appearance | 19 Oct / A | 2 | 3.98 [3.75, 4.22] | 1.25 [1.02, 1.48] | 96.81 [92.98, 100.65] | 33.27 [17.99, 48.55] |
| Language and Appearance | 20 Oct / C | 1 | 2.97 | 1.33 | 77.45 | 21.41 |
| Process List | 19 Oct / A | 1 | 8.43 | 3.98 | 215.98 | 125.85 |
| Process List | 20 Oct / C | 2 | 11.28 [11.09, 11.48] | 4.18 [3.59, 4.76] | 281.73 [275.84, 287.62] | 126.91 [126.52, 127.29] |
| Repeated navigation | 19 Oct / A | 1 | 13.28 | 58.31 | 345.62 | 1538.51 |
| Repeated navigation | 20 Oct / C | 2 | 12.63 [12.38, 12.89] | 46.47 [44.65, 48.30] | 337.96 [331.57, 344.34] | 1264.68 [1199.92, 1329.44] |
| Scroll Process List | 19 Oct / A | 1 | 22.26 | 13.80 | 599.15 | 413.53 |
| Scroll Process List | 20 Oct / C | 2 | 22.13 [21.87, 22.39] | 16.07 [15.49, 16.66] | 620.63 [613.14, 628.12] | 403.27 [402.79, 403.74] |
| Resize Home | 19 Oct / A | 1 | 7.55 | 4.42 | 186.64 | 87.27 |
| Resize Home | 20 Oct / C | 2 | 7.55 [7.29, 7.81] | 3.19 [2.74, 3.65] | 193.62 [190.95, 196.28] | 83.12 [82.25, 83.99] |
| Close/reopen | 19 Oct / A | 1 | 1.95 | 0.78 | 49.20 | 28.50 |
| Close/reopen | 20 Oct / C | 2 | 2.03 [1.95, 2.11] | 1.09 [1.02, 1.17] | 52.05 [49.42, 54.68] | 38.32 [37.88, 38.76] |

Private commit for the same pairs, in MiB:

| Workload | Session / collector | Pairs | Wind commit | Iced commit |
| --- | --- | ---: | ---: | ---: |
| Home | 19 Oct / A | 1 | 25.22 | 25.13 |
| Home | 20 Oct / C | 2 | 25.42 [25.09, 25.76] | 25.29 [25.16, 25.41] |
| Language and Appearance | 19 Oct / A | 2 | 22.42 [22.39, 22.45] | 20.74 [20.74, 20.74] |
| Language and Appearance | 20 Oct / C | 1 | 22.40 | 20.67 |
| Process List | 19 Oct / A | 1 | 24.32 | 23.76 |
| Process List | 20 Oct / C | 2 | 23.58 [23.57, 23.58] | 25.70 [23.25, 28.14] |
| Repeated navigation | 19 Oct / A | 1 | 28.25 | 33.21 |
| Repeated navigation | 20 Oct / C | 2 | 28.19 [28.17, 28.21] | 32.84 [32.81, 32.87] |
| Scroll Process List | 19 Oct / A | 1 | 23.98 | 24.23 |
| Scroll Process List | 20 Oct / C | 2 | 23.68 [23.66, 23.70] | 24.49 [24.21, 24.77] |
| Resize Home | 19 Oct / A | 1 | 25.34 | 24.94 |
| Resize Home | 20 Oct / C | 2 | 24.94 [24.91, 24.97] | 25.55 [25.38, 25.73] |
| Close/reopen | 19 Oct / A | 1 | 19.09 | 24.98 |
| Close/reopen | 20 Oct / C | 2 | 18.60 [18.41, 18.79] | 25.22 [25.14, 25.31] |

Home, Language and Appearance, Process List, and close/reopen use the 20-second
cohorts `10a9fcb641e5` (A) and `e66bdd553bd1` (C). Navigation, scroll, and resize
use the 12-second cohorts `2ff02fb9985f` (A) and `cf01317df3be` (C). Full collector
hashes appear below. The per-day ranges remain descriptive: each session has
only one or two independent pairs per workload.

The following differences are computed for each matched source, scenario, trial,
session date, and collector cohort before summarizing. Values are Wind minus
Iced in percentage points of one logical CPU, with median [minimum, maximum].
Positive values mean more CPU time for Wind. These are not differences between
separately pooled framework medians.

| Workload | 19 Oct paired CPU difference | 20 Oct paired CPU difference |
| --- | ---: | ---: |
| Home | 3.05 | 2.46 [2.34, 2.58] |
| Language and Appearance | 2.73 [2.73, 2.73] | 1.64 |
| Process List | 4.45 | 7.11 [6.33, 7.89] |
| Repeated navigation | -45.03 | -33.84 [-35.92, -31.76] |
| Scroll Process List | 8.46 | 6.06 [5.21, 6.90] |
| Resize Home | 3.12 | 4.36 [4.17, 4.55] |
| Close/reopen | 1.17 | 0.94 [0.78, 1.09] |

The CPU ordering agrees across all three pairs of every workload, although the
size of the difference varies between sessions. This describes these ports and
workloads; it does not establish a general framework ranking or isolate the
cause of the observed work.

Each scroll trial delivered 107/110 wheel messages for Wind/Iced during sampling;
each resize trial delivered 24 actions. Navigation completed 71/72 actions over
each entire fixture lifetime, including warmup and its final buffer. Close/reopen
completed eight actions per fixture lifetime. These counts do not measure
latency, frame rate, or exact per-action cost. Reopening memory combines visible
and closed states; it is not a leak measurement.

An inactive display followed by Windows' LockApp surface interrupted collection
after the first 16 accepted active runs. Collection resumed after the user
unlocked Windows on 20 October. Four foreground-invalid attempts are excluded:
two during the lock-screen interruption and two after browser focus interrupted
the resumed collection. Clean accepted counterparts were retained; a matched
pair can therefore include a retry gap within the same session. All 42 accepted
active runs passed the collector's native-state/foreground checks and cooperative
cleanup requirements. Invalid attempts and resumption history remain in the raw
evidence. The unrecorded original display power state and different session policy
continue to limit cross-day energy/compositor comparisons.

## Separate private RAM and stability diagnostics

Both fixtures could still run background scenarios after the interruption. These
EX2 measurements form a separate lock-screen diagnostic cohort: one 180-second
pair after closing, and one 20-second pair each for hidden/minimized windows.
They are not pooled with the core five-repeat results.

| State | Wind private resident RAM | Iced private resident RAM | Wind private commit | Iced private commit |
| --- | ---: | ---: | ---: | ---: |
| Closed to tray, 180 seconds | 6.04 MiB | 12.16 MiB | 14.12 MiB | 20.50 MiB |
| Hidden window retained | 12.65 MiB | 12.21 MiB | 20.62 MiB | 20.53 MiB |
| Minimized | 16.61 MiB | 16.09 MiB | 24.85 MiB | 25.06 MiB |

The closed Wind run used 0.035% of one logical core and approximately 708,000
cycles/s; Iced's CPU time quantized to zero, with approximately 60,600 cycles/s.
Private commit ended 0.090/0.188 MiB lower and handles ended 6/8 lower for
Wind/Iced. The short-trial Wind increase did not accumulate upward during these
three minutes. Neither run establishes freedom from leaks.

A Wind-only 60-second resident-start diagnostic, with no window ever opened,
recorded 13.36 MiB private commit. It used the older cooperative fixture without
the full ready/finished JSON contract and is excluded from matched results.
Raw data remain available locally; this run is not used to rank the frameworks.

Four separate thread-counter profiles were also collected. Their observer used
94–98% of one core, with roughly 0.5 seconds per collection. Main-process context
switch rates remained zero despite known timer activity, and raw per-thread
PID/TID coverage was not preserved for validation. Those rates are **unvalidated**;
the profiles' application CPU/power values are excluded from framework ranking.
The collector now warns explicitly for this optional mode. A useful follow-up
needs validated per-thread coverage or ETW at a less intrusive rate.

## GPU, energy, and observer limits

Neither software-rendered application's PID exposed a usable GPU engine or
GPU-memory counter instance during the accepted core trials. Those values
are **unavailable**, not measured zeros. DWM GPU counters were available, but
represent composition for the whole desktop.

Background trial package-power averages spanned approximately 10.8–23.3 W and
system CPU load varied substantially. The collector itself used approximately
33–34 CPU ms/s at two samples per second. Per-process CPU/cycle counters exclude
collector execution, but scheduling, clocks, and package energy still reflect
the shared machine. These data cannot establish an application power or battery
life winner.

## Improvement candidates

These are source-supported hypotheses for subsequent isolated before/after
experiments. The benchmark does not identify call-stack CPU attribution.

1. **Preserve closed-window disposal and inspect unnecessary resident work.**
   Normal close-to-tray destroys Wind's window and stops its 250 ms timer; its
   resident timer continues every five seconds. Its cost is small, so preserve
   tray probe/retry/fail-open behavior and runtime/recovery/task wakeups. Inspect
   unconditional Tick work, unchanged boolean Signal writes, and tray-menu
   reconstruction in [app.rs](../src/ui/app.rs). Pinned
   Wind UI's `Signal::set` writes and notifies even when the boolean value is
   unchanged. Guarding unchanged values and rebuilding tray state only when its
   inputs change are promising experiments, especially for static visible pages
   where the observed CPU difference is larger.
2. **Suspend display-only dashboard sampling while minimized in both ports.**
   The current [resident visibility contract](../src/platform/windows/resident_shell.rs)
   treats a minimized window as visible, so the Home query gate remains active.
   Introduce a distinct drawable-state condition for display queries; changing
   the meaning of hidden could also change process-control policy.
3. **Reduce retained-hidden work where that state is needed.** An allocated
   Wind window has a 250 ms timer and the diagnostic shows activity to investigate.
   This is not Wind's default closed-to-tray state. Validate show/hide,
   minimize/restore, task completion, settings/locale changes, and tray loss.
   Preserve the recovery helper and all restoration checks.
4. **Avoid duplicate native lookups and unchanged settings work.**
   `native_tick` obtains visibility and the HWND through separate native window
   enumerations. A single validated snapshot could serve both. Cache draft-change
   comparisons by edit revision and short-circuit empty task dispatch only after
   measuring their contribution.

Wind UI already skips painting hidden/minimized windows in the pinned backend.
The observed CPU activity does not establish that hidden rasterization or GPU
rendering is occurring.

## Reproduction and evidence

The scenario configuration is [ui-framework-resources.json](ui-framework-resources.json).
The collector is [measure_ui_resources.py](../scripts/measure_ui_resources.py), and
the trial-level analysis tool is
[summarize_ui_resources.py](../scripts/summarize_ui_resources.py).

Durable results are [trial metrics in CSV](../benchmark-results/archive-20261028/benchmark/ui-framework-resource-results-20261019/trials.csv)
and [group statistics, ranges, counter definitions, and cohorts in JSON](../benchmark-results/archive-20261028/benchmark/ui-framework-resource-results-20261019/summary.json).
CSV blank values and JSON nulls mean unavailable. The 82 summarized runs comprise
30 core background runs, 42 active runs, six separate locked-desktop
RAM/stability runs, and four intrusive thread profiles. The 78 background, active,
and RAM/stability runs contribute to application comparisons within their own
cohorts. The legacy resident-start run is excluded.
Thread-profile rows explicitly disable use for application CPU ranking; their
context-switch values remain raw, unvalidated provider reports.

For a **new full matrix with five repeats of every scenario**, run from the
repository root on an unlocked Windows desktop. This example uses five repeats
and a five-second warmup for every scenario; the reported active matrix used
three repeats and a three-second warmup:

```powershell
python scripts/measure_ui_resources.py --config benchmark/ui-framework-resources.json --out target/framework-benchmark/repeat --warmup 5 --trials 5 --system-energy --baseline-duration 20
python scripts/summarize_ui_resources.py target/framework-benchmark/repeat/measurements.json --out target/framework-benchmark/repeat-summary
```

The executable paths refer to isolated benchmark builds. Wind UI's harness uses
the `render-smoke,runtime-benchmark` features. The historical Iced fixture is
built from the archived commit with its isolated fixture patch; the normal
product dependency remains Wind UI. Native desktop access is required: a
desktop-isolated sandbox cannot validate Explorer registration or enumerate the
real fixture windows.

Local raw evidence lives under `target/framework-benchmark/`, including CSV
samples, ready/finished files, logs, hardware metadata, binary hashes, source
archives, build manifest, and the Iced reproduction patch. Functional pilot and
scroll-verification data are excluded from the performance results.

The local `resource-evidence-20261019.zip` packages the 19–20 October measured
cohorts, exact configuration files, collector versions, binary/build manifest,
current source snapshot, historical source archive, Iced fixture patch, and
resumption/display-request evidence. The dated artifact paths retain their
original name; their contents cover both measurement days. Collector versions:

- **A:** `103f0943be6fe6d1675d3117c1240722324ab573dcc1b763f273a174305453c9`
  measured 30 background and the first 16 active runs on 19 October.
- **B:** `2f6f26625e88b859951debbd7fbf5731e83efd83e08d4644f76fa26151fc7b8d`
  measured the six separate EX2 RAM/stability runs and four thread profiles on
  19 October. Its unmatched resident-start run remains excluded.
- **C:** `3f734f85c6c5e32c7117707f53a51ad6d858080a57743238d009bb2a021344b1`
  measured the remaining 26 active runs on 20 October. Its changes from B concern
  diagnostic warning/status wording; counter arithmetic is unchanged.

Core A/C measurements use the same 80-byte `PROCESS_MEMORY_COUNTERS_EX` query
with EX2 disabled and unchanged application binaries. Optional EX2 diagnostics
remain separate. Collector hashes and session dates are retained rather than
merging their CPU or package-power distributions. Pre-resume JSON snapshots are
archived for provenance and are not additional analysis inputs.

Rust validation on 19 October: normal and optional-feature formatting/strict
Clippy passed; 861 normal tests and 869 optional-feature tests passed
(18 ignored in each).
Collector and summarizer unit tests, architecture ownership, legacy-name scan,
diff checks, and graph update also passed. The normal release executable excludes
the opt-in fixture arguments. No production performance optimization was applied
as part of these measurements.

On 20 October, all 32 Python collector/summarizer unit tests and an independent
raw-data/export cross-check passed. All 42 active runs passed native-state,
foreground, and cleanup checks. Final cleanup confirmed no orphaned
Winderust/recovery processes, the Balanced power plan, and restoration of the
thread's previous display/system request after each of the three resumed attempts.
