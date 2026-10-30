# Matched background comparison — 2026-10-20

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

This matched test does **not** show Wind UI using fewer background CPU cycles
than Iced. Iced had the lower median and used fewer cycles in two of the three
blocks, including against Wind UI's maintenance-suppressed diagnostic mode.
Wind UI used less private resident RAM and private commit. One block favored
low-maintenance Wind UI, and the ranges overlapped; three blocks on a live
desktop do not establish a universal framework ranking or a power saving.

| Condition | Cycles/second, median | Trial range | Private resident RAM, median | Private commit, median |
| --- | ---: | ---: | ---: | ---: |
| Wind UI, normal maintenance | 763,518 | 583,835–853,022 | 6.168 MiB | 15.098 MiB |
| Wind UI, maintenance skipped after readiness | 144,972 | 124,356–631,173 | 6.160 MiB | 14.496 MiB |
| Iced, ordinary healthy hidden state | 49,879 | 20,979–144,394 | 12.336 MiB | 20.691 MiB |

All values include the application and its recovery helper. Each memory value
is the median of three per-capture medians, rather than a pooled sample median.
The separately measured sampler is excluded from application totals. Median
thread counts were 22 for both Wind UI conditions and 35 for Iced. Windows CPU
time deltas were quantized to zero in both low-maintenance Wind UI and Iced;
their process-cycle counters remained positive.

| Block | Low-maintenance Wind UI cycles/s | Iced cycles/s | Wind UI minus Iced cycles/s |
| --- | ---: | ---: | ---: |
| 1 | 144,972 | 20,979 | +123,993 |
| 2 | 124,356 | 144,394 | −20,038 |
| 3 | 631,173 | 49,879 | +581,294 |

Positive differences favor Iced. The median within-block difference was
**+123,993 cycles/second**, with a range of **−20,038 to +581,294**. No sample,
outlier, or block was removed. These results provide no measured support for a
Wind UI cycle advantage in this configuration. They also show why a hypothetical
optimization percentage cannot be applied to an earlier Iced result to predict
a winner.

Collection used three balanced blocks, with each condition occupying each
ordinal position once:

1. Wind normal → Wind low-maintenance → Iced.
2. Wind low-maintenance → Iced → Wind normal.
3. Iced → Wind normal → Wind low-maintenance.

The unchanged collector encoded these as nine unique named targets in one
pass. `comparison_condition`, `comparison_block`, and `comparison_position` in
the configuration provide the grouping; each condition therefore has three
captures even though the collector's `trials` parameter is one. The config
prespecified this order before collection.

Each capture used 15 seconds of warmup followed by 60 seconds at 1 Hz. Automation
was disabled, settings were in memory, and updates and startup registration were
off. No scheduler profile or dedicated worker was selected. Both applications
were compiled with tiny-skia software rendering. GPU, energy, and optional
expensive thread counters were disabled. No builds or extra command launches
occurred during timed collection. The short two-mode pilot is retained separately
and excluded from these nine captures.

The host was busy: per-capture median whole-machine CPU use ranged from about
10.08% to 34.24%, with memory load reaching 96%. The sampler itself used about
1.98–3.57% of one core, recorded separately without subtraction. High host load
and memory pressure can affect scheduling, page residency and event activity.
These are background-application observations, not a quiet-machine idle floor.
The [per-capture host context](../benchmark-results/archive-20261028/benchmark/ui-idle-framework-results-20261020/host-context.csv)
is retained with the results.

Source preflight confirmed matching fixture settings and relevant shared
backend code: settings defaults, self-power implementation, automation
requirements/runner/wake/status, and runtime scheduler/observations. Both publish
actual hidden state and request the same hidden-process self-power policy. Raw
Win32 self-power state was not separately sampled, so that parity is a source
check rather than an independent operating-system readback.

Lifecycle and harness differences remain explicit. Wind UI destroys its native
settings window; Iced retains a hidden HWND, following each application's real
background behavior. Wind UI's diagnostic flag skips the five-second application
maintenance body after readiness, while leaving the native interval and framework
dispatch registered. Iced ordinarily disables its hidden UI tick when no retry
is pending. Wind UI uses one sleeping completion thread; Iced uses an asynchronous
deadline subscription. Both completion deadlines occur 78 seconds after readiness,
outside the 15 + 60 second collection interval. These are application-plus-fixture
measurements, not an isolated measure of either framework's minimum possible cost.

All nine accepted captures confirmed the expected native state (Wind absent,
Iced hidden), automation disabled, correct diagnostic flags, successful EX2
memory sampling, exit code zero, completed message loop, and no runtime or
cleanup errors. Iced additionally reported successful runtime/helper cleanup
and an inactive automation worker at measurement end. Production code and the
production executable were not changed.

The third low-maintenance Wind UI run was elevated throughout the measurement.
Its main process accounted for about 618,096 cycles/s and helpers for about
13,084 cycles/s. All 60 main-process intervals had nonzero deltas, unlike the
first two low-maintenance runs, which had 12 nonzero main intervals aligned with
the five-second cadence. The largest third-run interval contributed about 10.6%
of total cycles; the top five contributed about 21.8%. This is sustained activity,
not a single removable spike. Its cause was not isolated, and no desktop-event
trace was recorded. It remains included in every aggregate.

The earlier [maintenance diagnosis](ui-idle-maintenance-diagnosis-20261020.md)
established substantial avoidable maintenance-body work in separate Wind-only
runs. This fresh comparison shows that eliminating that body does not by itself
establish a cycle advantage over Iced. The remaining native interval/framework
dispatch and other event-driven activity require separate attribution. The
diagnostic suppresses periodic tray health fallback and is not a safe production
optimization by itself. Enabled scheduler costs and energy savings were not
retested here. Do not pool this cohort with the earlier scheduler fixture, which
used dedicated workers and additional scheduler bookkeeping.

The [summary](../benchmark-results/archive-20261028/benchmark/ui-idle-framework-results-20261020/summary.json),
[per-capture results](../benchmark-results/archive-20261028/benchmark/ui-idle-framework-results-20261020/trials.csv), and
[within-block differences](../benchmark-results/archive-20261028/benchmark/ui-idle-framework-results-20261020/pairs.csv) accompany
this report. Raw JSON/CSV, logs, pilots, configuration and analysis are under
`target/idle-framework-comparison-20261020/`. Run the same sequence with:

```powershell
python -B scripts/measure_ui_resources.py --config target/idle-framework-comparison-20261020/config.json --out target/idle-framework-comparison-20261020/capture-reproduction --warmup 15 --duration 60 --interval 1 --trials 1 --startup-timeout 30 --no-gpu --memory-ex2
```

Both Wind UI conditions use diagnostic binary SHA-256
`6676a29becbd678d8ed7e7729e79ce13577ae5b12a08a8a226d0751c8f0d4733`.
The Iced V3 binary SHA-256 is
`2d040026dae4b842a3853f5503817a15a00abd097f24e4a681749aa6976d2b82`.
The unchanged collector SHA-256 is
`98e473b2858c28e5a20f03dc71980a733f766916ee5ce7ad582e7182f9905946`.
