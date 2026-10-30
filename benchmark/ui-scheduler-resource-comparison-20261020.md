# Winderust: enabled scheduler resource comparison

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

Wind used less memory, while Iced used fewer cycles with automation Off. With
enabled schedulers, shared backend work often outweighed that idle UI difference.
The clearest improvement targets are CPU Limiter overhead and enabled waiting/
observation work. This experiment does not establish an energy winner.

Completed: **63 collector-accepted, native-restored trials across 20 profiles**.
Two By Time observations are retained but excluded because of a desktop-state
transition, leaving 61 eligible trials: 30 matched pairs and one unpaired Wind
observation. Five additional failed setup/readiness attempts are preserved
separately.

This extends the [UI resource comparison](ui-framework-resource-comparison-20261019.md)
to real enabled schedulers in the complete Wind UI and historical Iced
applications. It measures the cost of each configured background workload,
including the production automation runtime and recovery helper. A native
readback verifies the requested setting on disposable processes before an active
action is claimed.

The two ports share the same scheduler algorithms. Framework choice and backend
efficiency remain separate questions: an enabled scheduler can dominate a small
idle UI cost. This experiment does not establish an application energy winner.

## Machine and builds

Measurements ran on 20 October 2026, 01:50:24–04:24:30 UTC
(09:50:24–12:24:30 Asia/Taipei), on the same Windows machine as the preceding UI
comparison. These bounds cover accepted trials; setup failures and gaps remain
in their source records.

| Item | Configuration |
| --- | --- |
| CPU | Intel Core 5 210H; 8 cores, 12 logical processors |
| Memory | 16,763,805,696 usable bytes reported by Windows |
| Windows | Build 26200, x64 |
| Wind UI | 0.24.0, pinned revision `36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2` |
| Iced | 0.14, historical application at `3d329a9c57845bfbb2252e26f361e4d87bbc8b95` |
| Rendering | Software rendering: Wind tiny-skia 0.12; Iced tiny-skia 0.11.4 |
| Window lifecycle | Production close-to-tray: Wind destroys its UI HWND; Iced retains a hidden HWND |
| Power plan | Original Balanced GUID `381b4222-f694-41f0-9685-ff5bb260df2e`; independently checked unchanged |

These are isolated release builds with opt-in benchmark support. The application
model, real schedulers, protected-process filtering, exact process identity
checks, restoration, and recovery helper remain enabled. User settings I/O,
updates, startup registration, inherited user rules, and cross-session process
control are disabled in the fixture. Both ports receive the same typed settings
builder. Each accepted framework pair must have identical serialized settings.

There are three measured fixture versions. Version 1
allowed a power-rule fixture to declare readiness when the current plan was
known. Version 2 changes only the
opt-in fixture readiness gate: it waits for the selected typed decision, rule,
target GUID, and reason. By Activity additionally waits for its five-second idle
decision in v2. Version 3 corrects that unnecessarily restrictive By Activity
gate to accept either a coherent Active or Idle decision, with the original
current/target GUID, no rule index, and the corresponding reason. User input can
legitimately keep the scheduler Active. It also requires successful nonzero
matched-control counts before readiness; no-match may remain zero, and GPU
Priority may report applied or explicitly pending. App Suspension must have
actually suspended targets before its positive-action gate passes. The
production scheduler algorithms and configured rules are unchanged.
Build cohorts remain separate in the exports.

| Build | Wind UI executable SHA-256 | Iced executable SHA-256 |
| --- | --- | --- |
| v1 | `66360e918d0191a2d91495fd1d80117b3d92b9831079fa5eea3ae321578a2525` | `27d30a6d7be303f1b9ecdf920c8d02cd64d0f8388d20b31dcedddd060ac563ef` |
| v2 | `50482578fd216f5408dd9faff2885c37c54c4d896048e8b028768ae0e282598e` | `a857d73865150d5bcb935a5074b23900a2755cae1aec78ab39c20f53a1596e39` |
| v3 | `8f44a67a6050ed09c73917abe50cb4f5ad916a853c1a5855544712ad02577fe7` | `2d040026dae4b842a3853f5503817a15a00abd097f24e4a681749aa6976d2b82` |

The v1 shared scheduler fixture module is
`959b9cd47f94384f1e45023078c9ef8825a6e701b69d3ab6a705d76ab3258a4d`.
The v2 module is
`2d90ca9972b49804caf4fe93245b9b4fa902c7abca49fa9605b129a509e454ae`.
The final v3 module is
`70b27e2f784ba0e4d2ac0673b447b2e8a047b7418ef2dabb3f5152602a975fde`.
The intermediate module revision
`90159269a4ed30df075e47d2b20c55a18ead85ae1ad2373cb449dcf3a3226895`
was never built.
Fixture module, executable, worker, collector, verifier, and orchestrator hashes
accompany the final source/export records and evidence bundle. An orchestrator fix for a worker-startup
permission race creates a separate source cohort; it does not change counters,
settings, binaries, or scheduler algorithms.

The readiness field named `source_commit` has different meanings in the two
fixtures: Wind reports the Wind UI framework revision, while Iced reports the
historical Winderust application HEAD. These fields are not equivalent app-source
identifiers. Executable hashes, preserved source snapshots, and the shared fixture
overlay establish the benchmark build provenance. The older Iced
`framework-benchmark-manifest.json` describes a previous UI-only fixture and is
historical evidence, not this scheduler build's manifest.

This is a shared desktop, with other programs left running. No unrelated
program was closed or reconfigured for the benchmark. Earlier captures also
overlapped ordinary agent activity; later captures prohibit new external
process launches during the measurement window. Neither phase is a dedicated
quiet-machine benchmark. Host activity and memory
pressure affect interpretation. For example, the v1 Wind enabled-no-match trial
reported 94% system memory load at both median and p95, about 902.5 MB available
physical memory at the median, and machine CPU median/p95 of 14.22%/26.04%.
These observations describe one trial's context, not a constant load across
every case. The phase table below retains ranges of per-trial host medians.

Private working sets were therefore observed under memory pressure and can
shrink through working-set trimming. Private commit is less dependent on
residency. High process page-fault rates under these conditions do not identify
disk faults or prove that one framework caused the surrounding pressure.

## Collection phases and host context

| Phase | Source cohort | Build | Eligible trials / pairs |
| --- | --- | --- | ---: |
| A | [scheduler-matrix-20261020](../target/scheduler-benchmark/scheduler-matrix-20261020/scheduler-measurements.json) | v1 | 9 / 4 + 1 unpaired |
| B | [scheduler-matrix-continuation-20261020](../target/scheduler-benchmark/scheduler-matrix-continuation-20261020/scheduler-measurements.json) | v1 | 8 / 4 |
| C | [power-evaluators-foreground-unavailable-20261020](../target/scheduler-benchmark/power-evaluators-foreground-unavailable-20261020/scheduler-measurements.json) | v1 | 2 / 1 |
| D | [independent-controls-v2-20261020](../target/scheduler-benchmark/independent-controls-v2-20261020/scheduler-measurements.json) | v2 | 8 / 4 |
| E | [foreground-controls-v2-20261020](../target/scheduler-benchmark/foreground-controls-v2-20261020/scheduler-measurements.json) | v2 | 2 / 1 |
| F | [remaining-controls-v3-20261020](../target/scheduler-benchmark/remaining-controls-v3-20261020/scheduler-measurements.json) | v3 | 8 / 4 |
| G | [suspension-confirmation-v3-20261020](../target/scheduler-benchmark/suspension-confirmation-v3-20261020/scheduler-measurements.json) | v3 | 4 / 2 |
| H | [adaptive-observation-v3-20261020](../target/scheduler-benchmark/adaptive-observation-v3-20261020/scheduler-measurements.json) | v3 | 4 / 2 |
| I | [busy-control-v3-20261020](../target/scheduler-benchmark/busy-control-v3-20261020/scheduler-measurements.json) | v3 | 4 / 2 |
| J | [core-confirmation-v3-20261020](../target/scheduler-benchmark/core-confirmation-v3-20261020/scheduler-measurements.json) | v3 | 12 / 6 |

Phase D contains ten raw accepted trials, of which the two By Time rows are
explicitly excluded. Phase A's extra Wind Dynamic Priority Boost observation is
not paired with phase B's Iced run. Twenty-five compatible profile/phase groups
remain separate; no cross-phase pooled framework score is calculated.

Each phase has an explicit environment tag in the JSON/CSV. These labels preserve
collection phases and known foreground availability; they do not prove that host
conditions were identical within a phase. The following ranges use all 63 raw
accepted trials, including the two curated exclusions. They are ranges of
per-trial medians, not an average of host medians or peak-load bounds.

| Phase | Range of trial-median machine CPU % | Range of trial-median memory load % |
| --- | ---: | ---: |
| A | 9.6–27.6 | 82–94 |
| B | 9.6–17.7 | 82–97 |
| C | 11.6–12.7 | 94–94 |
| D | 13.1–27.1 | 74–93 |
| E | 16.0–24.0 | 87–87 |
| F | 14.1–27.4 | 86–96 |
| G | 14.9–21.3 | 86–95 |
| H | 13.7–19.2 | 86–95 |
| I | 36.3–50.1 | 87–96 |
| J | 9.7–27.9 | 86–88 |

## Target workload and measurement protocol

Each trial starts four independent sibling processes from one uniquely named
`WinderustSchedulerBench-*.exe`. Process-control rules target only that exact
executable path. The native verifier checks PID, creation
FILETIME, executable path, and SHA-256 to prevent PID reuse or another executable
from being mistaken for a target. The workers are outside the measured Winderust
process tree.

An idle worker writes an elapsed-time heartbeat once per second and otherwise
sleeps. A busy worker spins on one thread and maintains the same heartbeat. These
are reproducible disposable targets; they do not represent all real workloads.
Workers stop cooperatively after the application has exited and independent
native restoration verification passes. Failed restoration leaves exact target
identities available for diagnosis rather than silently terminating a suspended
process.

The standard profile uses a 15-second warmup, 60-second sampling window, and
one-second intervals. Each trial launches a fresh application and four fresh
workers. The intended order alternates frameworks between repeat blocks and
rotates profiles. Actual collection order includes interruptions and retries;
source timestamps are retained. Final release builds and Rust checks finished before v3 capture began. Earlier
phases included ordinary agent process activity, as noted above. Later captures
prohibit new external processes and use only persistent Node filesystem access
for small evidence reads or report edits. This reduces one source of WinEvent
interference without making the shared desktop quiet. Adaptive observations use
a 65-second warmup and 180-second sampling window. Busy controls and confirmation
repeats retain the 15/60-second protocol; their target modes and collection phases
remain separate.

The primary scope is **Winderust plus its recovery helper**. Main-process,
helper-only, and collector scopes are exported separately. The worker CPU
observations span the whole collector trial, including startup, warmup, native
verification, and cleanup; they are not the steady sampling window and are not
added to Winderust CPU. The collector's own CPU and collection duration describe
observer overhead.
Observer CPU is reported separately and is not subtracted from Winderust CPU.
The collector can still perturb scheduling, cache residency, and memory pressure;
separate process attribution does not remove that influence. Its final weighted
CPU range was 0.467–1.770% of one core across the accepted trials.

An accepted trial requires normal application exit, successful runtime/recovery
cleanup, no consumed runtime-worker error at readiness or measurement end, the
expected worker lifetime, unchanged power plan, native property restoration,
and clean cooperative target exit. App Suspension additionally needs stopped
heartbeats while suspended and resumed heartbeats after cleanup.

## Metric definitions

| Metric | Interpretation |
| --- | --- |
| CPU, percent of one core | Kernel plus user CPU-time delta divided by measured elapsed time; 100% means one logical core |
| Process cycles/s | `QueryProcessCycleTime` delta divided by actual elapsed time |
| User/kernel CPU | Weighted time rates, exported in ms/s |
| Private commit | Committed private virtual memory; not physical resident RAM |
| Private resident RAM | EX2 private working set; excludes pages shared with other processes |
| Total working set | Resident pages including shared pages; summing processes can double-count shared pages |
| RAM, handles, threads, GDI/USER objects, paged/nonpaged pools | Per-trial median, p95, and observed maximum where available |
| Growth | Observed endpoint difference and linear slope within the sampling window; not proof of a leak |
| Page faults | Includes soft faults; not a disk-read or hard-fault count |
| Process I/O | Read, write, and other operations/bytes; cached and non-disk operations are included |
| Setup/cleanup | Fixture/control-ready latency and coordinated cleanup timings; readiness includes mature scheduler gates and is not pure UI startup |

Windows process cycles include user and kernel execution across process threads.
They are not instructions, elapsed time, or a frequency-independent unit of work.
Use CPU time alongside cycles when comparing workloads. A zero CPU-time delta
at low activity can reflect counter quantization; the cycle counter can still
advance. [Microsoft: QueryProcessCycleTime](https://learn.microsoft.com/en-us/windows/win32/api/realtimeapiset/nf-realtimeapiset-queryprocesscycletime).

Private commit and private working set are different native fields. The latter
is available here through `PROCESS_MEMORY_COUNTERS_EX2.PrivateWorkingSetSize`;
missing measurements remain unavailable rather than being replaced with zero.
[Microsoft: PROCESS_MEMORY_COUNTERS_EX2](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters_ex2).

Per-process GPU utilization, application GPU energy, package-energy attribution, CPU hardware
performance events such as instructions/cache misses, and validated wakeup or
context-switch counts are outside this scheduler matrix. Native GPU scheduling
priority verification does not establish any GPU utilization or GPU performance
benefit on these CPU workers.

## Cases and verification coverage

The following table records what the accepted evidence verifies. Native active
action was independently verified in 27 accepted trials (including the unpaired
Wind observation); the remaining 36 were control, decision, observation, or
status-only workloads. All 63 passed native restoration verification. The two
curated exclusions are status-only, so the comparison retains 27 active-action
and 34 status-only trials. An enabled feature, a matched decision, and an actual
native mutation are distinct kinds of evidence.

| Profile | Workload or desired state | Current coverage |
| --- | --- | --- |
| Off | Production runtime without enabled automation | Matched v1 control |
| Enabled, no match | Process Priority enabled for a nonexistent dedicated path | Matched v1 waiting/scanning control; targets remain unchanged |
| Process Priority | Below Normal on all four workers | Matched v1 native readback and restoration |
| Thread Priority | Below Normal on readable target threads | Matched v1 exact thread identity/readback and surviving-thread restoration |
| Dynamic Priority Boost | Disabled for all four workers | Matched v1 readback and restoration; an earlier unpaired Wind run remains exploratory |
| IO Priority | Low on all four workers | Matched v1 native readback and restoration |
| Memory Priority | Low on all four workers | Matched v1 native readback and restoration |
| GPU Priority | Below Normal on all four workers | Matched v1 native class readback and restoration; no GPU-load performance claim |
| By Running App | Dedicated executable matches a rule selecting the original plan | Matched v1 decision verification; foreground-unavailable source cohort |
| Background Efficiency | Idle priority plus execution-speed EcoQoS | v2 available-desktop native readback/restoration; earlier interrupted attempt excluded |
| CPU Sets (Soft) | Lowest available logical processor, mapped to native CPU Set ID | v2 matched native readback and restoration |
| Processor Affinity (Hard) | Same selected logical processor | v2 matched native readback and restoration |
| CPU Limiter | 50% allowed CPU time on disposable workers | v2 idle overhead; v3 busy positive control shows reduced whole-trial worker CPU time; native duty proof remains status-only |
| App Suspension | Suspend disposable background workers after one second | v3 independent stopped/resumed heartbeats, all four suspended in ready/end status, native restoration; initial pair plus two confirmation pairs |
| By Activity | Active/Idle decision selects the original plan | v3 coherent typed decision and unchanged plan; v2 Idle-only readiness rejection excluded |
| By CPU Load | At-or-above-zero rule after one second, selecting the original plan | v2 matched decision/restoration; v1 readiness rejection excluded |
| By Time | All-day rule selecting the original plan | v3 available-desktop decision/restoration; v2 transition pair explicitly excluded |
| Combined | Background Efficiency, IO Priority, Memory Priority, and CPU Sets | v3 native readbacks and restoration for these four features |
| Adaptive Engine detection | Classifier observation with process actuators and processor power policy disabled | v3 65/180-second pair; complete CPU/GPU inputs and stabilized Headroom at endpoint |
| Adaptive Engine pressure monitor | Active process-domain observation with actuators and processor power policy disabled | v3 65/180-second pair; active scans at ready/end, zero adjustments/failures, no error |
| By Foreground | Headless target cannot own the foreground window | Active matching not covered; any headless run is unmatched evaluation only |
| Timer Resolution | Rule requires a matching foreground target | Active request/release not covered by headless workers |
| Memory Trim | No dedicated exact-target-only action profile | Actual trimming not covered |
| Process churn | New disposable target identities during sampling | Not yet covered |

Power-rule profiles deliberately select the already active plan. Verifying a
matched rule and selected GUID exercises detection and decision work, but there
is no actual power-plan transition, processor power-policy change, or measured
power saving. Full Adaptive Engine mutation is also outside the detection-only
profile.

CPU Limiter's 50% setting means 50 ms runnable followed by 50 ms frozen in each
100 ms cycle per managed application group. It is independent of the machine's
logical processor count. Four busy single-thread workers should each approach
half of one core when otherwise unconstrained in a successful positive control.
This requires measured worker
CPU-time deltas against an otherwise matched Off workload; an idle worker's
heartbeat and reported limited count cannot establish a 50% CPU-time reduction.
See [CPU Limiter duty schedule](../src/control/cpu_limiter.rs) and
[native timer](../src/platform/windows/cpu_limiter.rs).

## Interruptions and excluded evidence

Version 1 sources include `scheduler-matrix-20261020`,
`scheduler-matrix-continuation-20261020`, and
`power-evaluators-foreground-unavailable-20261020`. Matched pairs stay within a
source cohort and repeat. The continuation supplies the complete Dynamic
Priority Boost pair; the first source's extra Wind-only observation is preserved
without being paired to a different cohort's Iced run.

Windows temporarily returned no foreground HWND. The production process-control
safety policy can pause relevant controls when foreground protection is unknown.
An interrupted Background Efficiency attempt therefore failed the native
readback gate and is excluded. Its target restoration and subsequent worker
shutdown were checked independently and recorded in a manual-cleanup sidecar.
This outcome is not evidence of a framework performance defect.

The v1 By CPU Load attempt declared readiness before its delayed rule matched.
Its later finished snapshot showed a matching decision, and cleanup was
verified, but it remains rejected because the required ready-state proof was
absent. The v2 fixture readiness fix prevents that premature declaration. A
rejected fixture run is not converted into an accepted benchmark after the fact.

The v2 By Activity attempt reached a legitimate Active decision while user input
continued. Its fixture gate required Idle and timed out. Version 3 accepts either
coherent configured state. The rejected v2 attempt remains excluded; independent
native baselines and the original plan matched after cleanup, the runtime worker
was stopped, and all disposable workers then exited. Its conservative fixture
success flag remained false because it included the readiness failure. This is
distinct from a failed production restoration and is preserved in the manual
cleanup evidence.

The v2 App Suspension attempt declared readiness during its initial grace
period, before the suspended count became positive. Later evidence showed four
suspended targets and stopped heartbeats, but the ready-state requirement was
not met. Independent cleanup verified native baselines and heartbeat recovery
before the workers stopped. This rejected harness attempt remains excluded;
the v3 gate waits for actual suspension. These readiness corrections do not
indicate a framework defect.

The foreground-unavailable power cohort remains explicitly environment-tagged
and separate from desktop-available observations. The exports preserve
all accepted and rejected counts, actual dates, interruption gaps, and cleanup
evidence. Initial display state was not controlled sufficiently to make claims
about screen-state-independent system power.

The desktop became available during the v2
`independent-controls-v2-20261020` cohort. Its environment tag is
`foreground_unavailable_then_unlocked`. Both originally accepted By Time rows
are preserved as explicitly excluded observations: the Iced interval may overlap
the unlock, and the actual transition time is uncertain. They do not contribute
to paired comparisons. A fresh unlocked v3 By Time pair supplies the comparison. Being
independent of foreground matching does not make different desktop states
equivalent.

## Results and improvement priorities

The repeated core control is the strongest comparison here: three matched
pairs per profile in phase J, with alternating framework order. All figures
include Winderust and its recovery helper. CPU/cycle values are weighted trial
rates; memory values are each trial's median. Brackets show the range across
independent trials, not a confidence interval.

| Profile, metric | Wind median [trial range] | Iced median [trial range] |
| --- | ---: | ---: |
| Off, Mcycles/s | 1.287 [1.209–1.334] | 0.557 [0.505–0.646] |
| Off, CPU % of one core | 0.052 [0.000–0.052] | 0.026 [0.000–0.052] |
| Off, Private resident MiB | 6.32 [6.22–6.34] | 12.17 [12.11–12.42] |
| Off, Private commit MiB | 18.36 [18.25–18.88] | 24.71 [24.68–24.71] |
| Enabled, no match, Mcycles/s | 95.660 [91.052–102.470] | 96.198 [90.911–98.639] |
| Enabled, no match, CPU % of one core | 3.697 [3.463–3.801] | 3.567 [3.255–3.775] |
| Enabled, no match, Private resident MiB | 6.58 [6.42–6.60] | 12.46 [12.33–12.50] |
| Enabled, no match, Private commit MiB | 18.79 [18.51–19.39] | 24.87 [24.86–24.89] |

With automation Off, Iced used fewer cycles in all three pairs, while Wind used
about 6 MiB less private resident memory and private commit. The median paired
Wind-minus-Iced cycle difference was +0.688 Mcycles/s [0.652–0.782]. CPU-time
deltas at this level are quantized and changed direction in one pair; they do
not support a precise low-load CPU ratio.

Enabling a Process Priority rule that matches no process increased both ports
to about 96 Mcycles/s and 3.6% of one core. Their cycle ranges overlap. The
median of the three within-pair cycle differences was +0.141 Mcycles/s
[−2.979–6.272]; the paired CPU-time difference was +0.130 percentage points
[0.026–0.208]. A median paired difference need not equal the difference between
two independent medians. This is evidence that common enabled backend work
outweighs the measured idle UI cycle difference, without identifying its hot path.

The main process accounted for almost all of that no-match execution. Across
these core trials, the helper used about 2.39–2.44 MiB private resident memory
and 8.25–8.70 MiB private commit; its cycle rate stayed below 0.042 Mcycles/s.
Main/helper figures remain separate in the exports.

| Metric, median of trials | Off: Wind / Iced | No match: Wind / Iced |
| --- | ---: | ---: |
| Handles | 512 / 555 | 519 / 562 |
| Threads | 22 / 35 | 23 / 36 |
| GDI objects | 15 / 16 | 15 / 16 |
| USER objects | 25 / 28 | 26 / 29 |
| Page faults/s | 1.08 / 0.22 | 1040.87 / 1050.18 |
| User CPU ms/s | 0.00 / 0.26 | 7.29 / 6.77 |
| Kernel CPU ms/s | 0.26 / 0.00 | 29.17 / 28.90 |
| Other I/O operations/s | 0.00 / 0.00 | 108.07 / 108.08 |
| Private commit endpoint change, MiB | 0.129 / 0.059 | 0.145 / 0.063 |

User, kernel, and total CPU entries are independent medians across trials;
the component medians need not sum to the total CPU median.

The large fault and kernel-CPU changes are useful profiling leads. Page faults
include soft faults and are not disk reads. Small positive memory endpoint
changes over one minute, such as those above, do not demonstrate a leak.

### Standard idle profiles: exploratory single pairs

Each following row is one matched pair from its stated collection phase.
Different profiles were measured sequentially under changing host load and,
across versions, different fixture builds. These rows describe observed costs;
they are not a randomized causal ranking of frameworks or features. Phase J
confirmation above remains separate from the earlier Off/no-match pair.

| Profile (phase) | Wind Mcycles/s | Iced Mcycles/s | Wind CPU % | Iced CPU % |
| --- | ---: | ---: | ---: | ---: |
| Off (A) | 1.276 | 0.155 | 0.026 | 0.000 |
| Enabled, no match (A) | 87.857 | 79.555 | 3.203 | 2.994 |
| Process Priority (A) | 79.505 | 81.540 | 2.864 | 2.916 |
| Thread Priority (A) | 153.800 | 164.810 | 5.754 | 6.431 |
| Dynamic Priority Boost (B) | 114.364 | 119.794 | 4.166 | 4.609 |
| IO Priority (B) | 116.231 | 125.339 | 4.192 | 4.712 |
| Memory Priority (B) | 90.385 | 86.972 | 3.333 | 3.333 |
| GPU Priority (B) | 109.423 | 128.556 | 3.932 | 4.765 |
| By Running App (C) | 77.854 | 79.070 | 2.838 | 2.943 |
| CPU Sets (Soft) (D) | 81.813 | 82.031 | 3.046 | 3.125 |
| Processor Affinity (Hard) (D) | 77.202 | 76.662 | 2.994 | 2.864 |
| CPU Limiter, idle targets (D) | 488.817 | 483.820 | 17.131 | 18.121 |
| By CPU Load (D) | 1.565 | 0.943 | 0.130 | 0.026 |
| Background Efficiency (E) | 142.757 | 163.105 | 5.233 | 6.067 |
| App Suspension, initial (F) | 38.800 | 150.864 | 1.302 | 4.869 |
| Combined (F) | 146.949 | 123.973 | 5.441 | 4.452 |
| By Time (F) | 1.201 | 0.761 | 0.052 | 0.026 |
| By Activity (F) | 21.445 | 18.428 | 0.443 | 0.859 |

| Profile (phase) | Wind private resident / commit, MiB | Iced private resident / commit, MiB |
| --- | ---: | ---: |
| Off (A) | 6.18 / 18.29 | 12.23 / 24.69 |
| Enabled, no match (A) | 6.57 / 18.89 | 12.21 / 24.86 |
| Process Priority (A) | 6.48 / 18.89 | 12.55 / 24.95 |
| Thread Priority (A) | 6.84 / 19.27 | 12.85 / 25.25 |
| Dynamic Priority Boost (B) | 6.67 / 19.40 | 16.71 / 29.63 |
| IO Priority (B) | 6.80 / 19.62 | 12.64 / 25.11 |
| Memory Priority (B) | 6.48 / 19.35 | 12.59 / 25.01 |
| GPU Priority (B) | 6.55 / 19.34 | 8.78 / 25.14 |
| By Running App (C) | 6.44 / 19.24 | 12.25 / 24.87 |
| CPU Sets (Soft) (D) | 6.57 / 19.39 | 12.70 / 25.16 |
| Processor Affinity (Hard) (D) | 6.43 / 18.64 | 12.30 / 24.88 |
| CPU Limiter, idle targets (D) | 6.69 / 18.88 | 12.89 / 25.53 |
| By CPU Load (D) | 6.32 / 18.52 | 12.50 / 24.89 |
| Background Efficiency (E) | 6.77 / 19.86 | 12.80 / 25.35 |
| App Suspension, initial (F) | 6.61 / 18.77 | 16.58 / 29.33 |
| Combined (F) | 7.03 / 20.04 | 12.86 / 25.39 |
| By Time (F) | 6.37 / 18.64 | 12.56 / 24.94 |
| By Activity (F) | 6.42 / 18.58 | 12.61 / 25.02 |

Wind's memory footprint was lower in every listed pair. CPU/cycle ordering
varied: CPU Sets and hard affinity were close, while Combined favored Iced in
this pair and several individual controls favored Wind. The evidence does not
establish an overall enabled-scheduler CPU winner.

### App Suspension: reversal and repeat variability

The initial phase F pair measured 38.800 versus 150.864 Mcycles/s, favoring Wind.
Two additional phase G pairs reversed that ordering: Wind-minus-Iced cycle
rates were +136.438 and +12.263 Mcycles/s. The separate confirmation results are:

| Metric | Wind median [trial range] | Iced median [trial range] |
| --- | ---: | ---: |
| Mcycles/s | 214.687 [120.388–308.987] | 140.337 [108.124–172.549] |
| CPU % of one core | 8.344 [4.817–11.872] | 5.454 [4.139–6.769] |
| Private resident MiB | 6.86 [6.83–6.89] | 12.83 [12.80–12.86] |
| Private commit MiB | 19.64 [19.48–19.80] | 25.26 [25.23–25.29] |

All three pairs had all four targets suspended in the ready and measurement-end
status, independent stopped-heartbeat proof, resumed heartbeats after cleanup,
and verified restoration. Thus the control actually operated. The wide variation
and initial reversal prevent attribution to a framework. The two confirmation
pairs merit a controlled follow-up trace rather than pooling all three pairs
into a single favorable number.

### CPU Limiter: actual CPU-time reduction, substantial manager cost

Phase I used four busy single-thread workers. The manager columns are the
60-second resource sample. Worker columns span the whole collector trial,
including warmup, verification, and cleanup, and must not be added to those
manager rates as if their windows were identical.

| Profile, four busy targets | Wind manager Mcycles/s / CPU % | Iced manager Mcycles/s / CPU % | Worker CPU % of one core, Wind / Iced |
| --- | ---: | ---: | ---: |
| Off | 1.055 / 0.052 | 0.716 / 0.000 | 389.80 / 387.91 |
| CPU Limiter 50% | 419.500 / 15.855 | 413.697 / 15.023 | 199.42 / 200.44 |

Worker CPU time fell by about 49% in both ports, consistent with the configured
50% duty cycle. This is an observed CPU-time reduction, not a useful-work
throughput score or exact steady-window duty proof. The independent native
limiter verification remains status-only. Manager overhead was substantial:
about 15–16% of one core for busy targets and 17–18% for the earlier idle targets.
It is therefore a high-priority cost to investigate, particularly for a product
intended to remain mostly idle in the background. No energy saving is inferred.

### Adaptive Engine: detection and pressure observation

Phase H used 65 seconds of warmup and 180 seconds of sampling for each row.
Process actuators and processor power-policy changes were disabled; enabling
Adaptive Engine still includes its production self-EcoQoS behavior.

| Observation | Wind Mcycles/s / CPU % | Iced Mcycles/s / CPU % | Wind private resident / commit, MiB | Iced private resident / commit, MiB |
| --- | ---: | ---: | ---: | ---: |
| Adaptive Engine detection | 1.423 / 0.052 | 0.889 / 0.017 | 9.93 / 22.02 | 16.41 / 29.11 |
| Adaptive Engine pressure monitor | 111.572 / 4.210 | 112.026 / 4.262 | 10.69 / 23.07 | 16.75 / 29.70 |

Both detection endpoints had complete CPU/per-CPU/GPU inputs, stabilized
Headroom classification, and no classifier error. This proves endpoint
availability, not uninterrupted valid input throughout the sample. The pressure
monitor additionally reported active process scans at readiness and measurement
end: 272 Wind / 278 Iced processes at the endpoint, zero adjustments, zero
failures, and no error. These are counts in a snapshot, not cumulative scan counts.

The pressure-monitor pair consumed almost identical CPU/cycles in both ports,
far above detection-only observation. That is a strong shared-backend
optimization lead. It does not exercise actual pressure-triggered mutations,
GPU workload performance, or a power-plan transition.

### Priorities for reducing background cost

1. **Profile CPU Limiter's manager overhead first.** Its cost persisted with idle
   targets and busy targets. Attribute time to duty scheduling, native freeze/thaw,
   waits, and target maintenance before changing them. Preserve exact process
   identity, complete thaw/restoration, failure handling, and recovery coverage.
2. **Reduce unnecessary enabled waiting/scanning work.** The no-match control
   repeatedly cost about 96 Mcycles/s even though no target matched. Investigate
   process/window inventories, safety-metadata queries, and refresh causes.
   Keep protection, access checks, target identity, and restoration intact.
3. **Separate cheap pressure observation from expensive control eligibility.**
   Static inspection shows Adaptive Engine process update requests the process
   snapshot before its pressure decision. Consider a no-actuator/no-managed-state
   fast path or a later eligibility snapshot, while preserving status semantics,
   hysteresis, safety checks, and recovery. Validate with matched profiles.
4. **Explain App Suspension variability with bounded attribution.** Preserve the
   independent suspension/restoration proof; repeat under controlled host load
   and use a suitable trace or fixture counters to locate work.
5. **Retain Wind's memory advantage while checking its resident idle work.**
   Iced's lower Off cycle rate is useful evidence for a CPU-first baseline.
   A closed Wind window is destroyed; its retained-hidden 250 ms window timer
   from the earlier UI tests is not the normal closed-window scheduler workload.

These are follow-up candidates, not confirmed causal hot paths or implemented
optimizations. For a background-heavy product, neither fewer cycles nor lower
private memory establishes lower total energy. Measure energy with a controlled
host after reducing verified backend cost.

Static scheduling inspection gives an important limit on interpreting the
enabled-no-match case. Its Process Priority and Process Appearance domains have
nominal ten-second background maintenance intervals. The general one-second
check setting does not imply a one-second backend timer when Power Plan Checks
are unused. Window creation, foreground changes, power notifications, and session
changes can request additional work; equal foreground/background settings do not
automatically eliminate those refresh requests. Resource samples with elevated
cycles or faults are not scan counts, because one long operation can cross
several one-second bins. Event tracing or bounded fixture counters would be
needed to attribute the measured execution to individual refresh causes.

The relevant refresh requirements and intervals are in
[requirements.rs](../src/backend/automation/requirements.rs) and
[automation.rs](../src/backend/automation.rs). The pressure-monitor lead is more
specific: [AdaptiveEngineProcessManager::update](../src/features/winderust_features/adaptive_engine_process.rs#L343)
requests the process snapshot before its pressure decision. The enabled process
domain has a one-second reaction interval; Process Appearance remains at a
60-second adaptive interval. [CycleObservations](../src/runtime/observations.rs#L111)
and [list_processes](../src/foreground/process_list.rs#L645) collect full-table
creation/session/critical/protected/access-check metadata, including native
process-handle queries. This does not mean universal path/token enrichment.
The ordering is a static optimization lead, not attribution of the measured
cycles.

## Reproducibility and evidence

The implementation is in [measure_scheduler_resources.py](../scripts/measure_scheduler_resources.py),
[measure_ui_resources.py](../scripts/measure_ui_resources.py),
[verify_scheduler_target.py](../scripts/verify_scheduler_target.py),
[scheduler_worker.cs](scheduler_worker.cs), and
[the shared scheduler settings builder](../src/ui/scheduler_benchmark.rs).

[summarize_scheduler_resources.py](../scripts/summarize_scheduler_resources.py)
cross-checks accepted payloads against their original collector JSON, verifies
binary/collector hashes and sampling protocols, fingerprints the raw CSVs, and
exports trial metrics, within-pair Wind-minus-Iced differences, descriptive
groups, and separately labeled whole-trial target observations. Groups require
matching settings hashes, original power GUID, build/counter/worker protocol,
and explicit phase/environment tags. These annotations preserve collection
phases; matching tags do not prove identical host conditions. Independent pairs receive equal weight; samples
within one trial do not become extra repeats, and per-cohort medians are not
averaged together.
Explicit `--exclude-trial COHORT:REPEAT:PROFILE:FRAMEWORK=REASON` selections preserve
raw collector acceptance and native coverage in separate exclusion outputs while
omitting the affected observations from paired comparisons.

The [validated summary](../benchmark-results/archive-20261028/benchmark/ui-scheduler-resource-results-20261020/summary.json)
records all ten source fingerprints, protocols, phase tags, native coverage,
curated exclusions, and failed attempts. Exports contain 116 metric columns per
scope plus identity, timing, and verification fields. Unavailable fields remain
missing; exporting a column does not mean it was measured.

- [Trial metrics](../benchmark-results/archive-20261028/benchmark/ui-scheduler-resource-results-20261020/trials.csv): 61 eligible trials × seven scopes.
- [Within-pair Wind-minus-Iced differences](../benchmark-results/archive-20261028/benchmark/ui-scheduler-resource-results-20261020/pairs-wind-minus-iced.csv): 30 pairs × seven scopes.
- [Descriptive group statistics](../benchmark-results/archive-20261028/benchmark/ui-scheduler-resource-results-20261020/groups.csv): 25 separate profile/phase groups.
- [Whole-trial external-worker observations](../benchmark-results/archive-20261028/benchmark/ui-scheduler-resource-results-20261020/external-workers-whole-trial.csv): target CPU time, separate from Winderust's steady sample.
- [Excluded accepted observations](../benchmark-results/archive-20261028/benchmark/ui-scheduler-resource-results-20261020/excluded-trials.csv): two By Time rows × seven scopes, with explicit reasons.
- [Scheduler evidence ZIP](../target/scheduler-benchmark/ui-scheduler-evidence-20261020.zip), [SHA-256 sidecar](../target/scheduler-benchmark/ui-scheduler-evidence-20261020.zip.sha256), and [external entry inspection](../target/scheduler-benchmark/ui-scheduler-evidence-20261020.inspect.json).

The bundle includes six framework executables (v1/v2/v3), matching source
snapshots and fixture overlays, worker/instrumentation sources, per-trial raw
CSVs and native checks, the five rejected attempts, manual-cleanup evidence,
the desktop-transition annotation, and the final curation exports. Its internal
MANIFEST.json inventories payload files. The external inspection hashes every
ZIP entry, including that manifest. Raw artifacts retain workspace/process
paths and PIDs; review those before sharing them publicly. The report links the
archive checksum rather than embedding it, avoiding a self-referential hash.

[Final safety state](../target/scheduler-benchmark/final-safety-state.json)
records no remaining benchmark app/helper/worker processes, the original active
power plan, and unchanged production executable and earlier framework evidence
archive. All accepted
workers exited cooperatively after verified restoration. The four rejected
control/readiness attempts have separate manual cleanup evidence; the fifth
failure was a worker-ready-file startup race.

Validation: 21 focused summarizer tests pass, including exact-pair/provenance
checks and exclusion preservation. Final shared checks passed 96 Python tests,
880 optional-feature Rust tests, and 861 normal Rust tests; formatting, Clippy,
legacy-name, ownership, and graph checks also passed. The export validates
source payloads, binary/collector hashes, sample protocols, clean runtime exit,
and native restoration before including a trial.
