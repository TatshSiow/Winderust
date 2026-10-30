# Event-driven background UI comparison — 2026-10-22

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

Wind UI can use event-driven application maintenance. This implementation
reduced Winderust's background cycles in every paired block: **48.47–64.49%**,
with a median paired reduction of **60.44%**. It did **not** beat Iced on cycles.
Iced used fewer cycles in all three blocks; Wind UI used about half its private
resident RAM. These observations compare the current application integrations,
including their different tray-health policies, rather than framework minima.

| Condition | Cycles/s, median | Trial range | Private resident RAM | Private commit |
| --- | ---: | ---: | ---: | ---: |
| Wind UI, previous normal maintenance | 691,432 | 665,807–753,168 | 6.285 MiB | 12.375 MiB |
| Wind UI, event-driven maintenance | 297,978 | 236,436–356,322 | 6.180 MiB | 12.492 MiB |
| Iced, ordinary healthy hidden state | 31,833 | 24,521–58,785 | 12.379 MiB | 18.758 MiB |

Totals include the application and its recovery helper. Memory values are
medians of the three per-capture medians. Private resident RAM means the sampled
private working set; private commit measures allocated private memory and is
reported separately. The small memory differences between the Wind builds do
not establish a meaningful memory improvement. Sampler costs are excluded from
application totals and reported separately, without subtraction.

| Block | Previous Wind cycles/s | Event-driven Wind cycles/s | Iced cycles/s | Wind reduction vs previous |
| --- | ---: | ---: | ---: | ---: |
| 1 | 753,168 | 297,978 | 31,833 | 60.44% |
| 2 | 691,432 | 356,322 | 24,521 | 48.47% |
| 3 | 665,807 | 236,436 | 58,785 | 64.49% |

Percentages use the baseline within each block. Their median is not the
percentage calculated from the two group medians. Event-driven Wind used
4.02–14.53 times Iced's cycles in these blocks, with a median ratio of 9.36.
The absolute paired excess was 177,651–331,800 cycles/s. Windows CPU-time
accounting had a median of zero for all three conditions despite positive
cycle counts; displayed CPU percentages are too coarse to rank these cases.
Cycles are not retired instructions or an energy measurement.

The changes are in [application maintenance](../src/ui/app.rs), the
[native resident adapter](../src/platform/windows/resident_shell.rs), and its
[tray monitor](../src/platform/windows/resident_shell/tray_monitor.rs):

- Removed the unconditional five-second application interval and its temporary
  framework callback context. Native visibility, power, tray, and worker events
  wake the application when work is available.
- Retained live tray-health validation using one reusable read-only worker,
  a five-second native fallback, and a two-second query deadline. Initial
  results, changes, and errors reach the UI; unchanged healthy results do not.
- Kept at most one query in flight, including after timeout. A blocked Explorer
  query cannot block teardown or cause repeated thread creation.
- Kept a separate native retry timer only while a settings retry is pending.
- Cached tray-menu data by persisted settings revision, active power profile,
  and language, avoiding reconstruction when those inputs are unchanged.

The pinned Wind UI dependency is unmodified. Scheduler, process-control,
settings, self-power, and recovery backend sources match the frozen Wind
baseline. Fixture changes remove periodic setup work after readiness and use
one completion wake outside the measurement window.

The behavior is still different from Iced's tray integration. Iced uses its
cached successful registration state and Explorer restart notifications.
Wind retains a live registration fallback. Removing that check would change
the health contract and is not part of this optimization. Source inspection
and the sampled cadence make retained native health work a plausible remaining
cost, but no thread stacks or per-function cycle attribution were collected.
The measurements do not prove that one function accounts for the residual gap.
Wind's main process had nonzero cycle deltas in 12–13 of 60 intervals, versus
three intervals for Iced. The earlier sustained 60/60 Wind episode did not recur.

Other combined-process medians from the same captures are retained in
[additional metrics](../benchmark-results/archive-20261028/benchmark/ui-idle-event-driven-results-20261022/additional-metrics.json):

| Metric | Previous Wind | Event-driven Wind | Iced |
| --- | ---: | ---: | ---: |
| Threads | 23 | 24 | 36 |
| Handles | 525 | 527 | 569 |
| GDI objects | 15 | 15 | 16 |
| USER objects | 25 | 27 | 29 |
| Page faults/s, including soft faults | 1.450 | 0.500 | 0.217 |
| Read/write I/O bytes/s | 0 / 0 | 0 / 0 | 0 / 0 |
| Other I/O bytes/s | 35.260 | 1.933 | 0.967 |

The extra persistent Wind worker costs one thread. Object counts and short
memory trends do not establish leak freedom. Process I/O includes cached and
non-disk operations; these counters are not disk traffic measurements.

Collection used nine captures in three prespecified balanced blocks:
previous Wind → event-driven Wind → Iced; event-driven Wind → Iced → previous
Wind; Iced → previous Wind → event-driven Wind. Each condition occupies each
ordinal position once. There were no exclusions, replacement captures, or
pooled pilot observations. Each capture used 15 seconds of warmup and 60
seconds at 1 Hz, with a 78-second ready-relative completion deadline. All nine
confirmed the expected native state, in-memory settings, disabled automation,
updates and startup registration, successful EX2 memory sampling, clean runtime
and helper shutdown, and exit code zero.

Wind destroys its settings HWND; Iced retains a hidden HWND. Both use tiny-skia
software rendering and the same relevant backend behavior. Hidden self-power
parity is checked in source; raw operating-system self-power state was not
independently read back. The Wind fixture waits on one completion worker; Iced
uses an asynchronous deadline subscription. The baseline is the frozen
diagnostic build in its ordinary-maintenance mode, without a skip flag. The new
fixture has no diagnostic suppression path. No scheduler profile or disposable
worker workload was selected.

No builds or extra command launches occurred during timed collection. This was
a busy desktop: per-capture median machine CPU ranged from 9.42% to 22.33%,
memory-load medians from 81% to 96%, and memory load reached 98%. The final
baseline/new captures had about 730/535 MB of available physical memory.
The sampler used 2.03–2.45% of one logical core, separately attributed. Host
load, memory pressure, CPU frequency and sampling can affect the observations.
Three blocks are descriptive evidence, not a universal ranking or a quiet
idle floor. GPU, thread context-switch tracing, package energy and enabled
scheduler costs were not measured in this cohort. No power-saving percentage
is inferred from the cycle reduction.

Validation passed: default Clippy and 866 tests; feature-enabled Clippy and
888 tests, with 18 intentional ignores in each suite; formatting, diff,
legacy-name and architecture ownership checks; native lifecycle smoke and
timed background completion. Twelve focused native tests cover health changes,
timeouts, late completion, nonblocking teardown, visibility and stale timers.
Five repeated native suites passed after fixing early timer delivery that
could previously defer a two-second deadline to four seconds. Actual Explorer
restart and native tray clicks remain separate integration checks. Graphify
was updated after code changes.

For a background application prioritizing cycles, Iced remains ahead in this
cohort. Wind's lower RAM and improved integration are useful, but do not justify
claiming a background cycle or energy advantage. Further optimization should
attribute the native query worker and timer paths before changing their policy.
Enabled scheduler costs remain covered by the separate
[scheduler comparison](ui-scheduler-resource-comparison-20261020.md).

The [summary](../benchmark-results/archive-20261028/benchmark/ui-idle-event-driven-results-20261022/summary.json),
[capture results](../benchmark-results/archive-20261028/benchmark/ui-idle-event-driven-results-20261022/trials.csv),
[paired differences](../benchmark-results/archive-20261028/benchmark/ui-idle-event-driven-results-20261022/pairs.csv), and
[host context](../benchmark-results/archive-20261028/benchmark/ui-idle-event-driven-results-20261022/host-context.csv) accompany
this report. Raw captures, pilots, source snapshot, comparison patch, hashes,
analysis and native logs are under `target/idle-event-driven-20261022/`.

```powershell
python -B scripts/measure_ui_resources.py --config target/idle-event-driven-20261022/config.json --out target/idle-event-driven-20261022/capture-reproduction --warmup 15 --duration 60 --interval 1 --trials 1 --startup-timeout 30 --no-gpu --memory-ex2
```

Binary SHA-256 values: previous Wind
`6676a29becbd678d8ed7e7729e79ce13577ae5b12a08a8a226d0751c8f0d4733`;
event-driven Wind
`41dc6057c8bc77ef171d04d2555b934b905e29dd24e455b649a5cdca16e6e011`;
Iced `2d040026dae4b842a3853f5503817a15a00abd097f24e4a681749aa6976d2b82`.
The unchanged collector is
`98e473b2858c28e5a20f03dc71980a733f766916ee5ce7ad582e7182f9905946`;
prespecified config
`a048622143435c7d694f2ec42298a75486a015939f78027bf03ce771777c398c`.
