# Intel Core 5 210H CPU Limiter mechanism comparison

2026-11-09, local release-build experiment. These measurements compare fixed
mechanisms, not automatic restraint rules or competing products.

On this run, a half-pool Processor Affinity (Hard) selection preserved nearly all
single-thread throughput and avoided the duty-cycle pause in target replies.
With 12 compute threads, it completed less work than 50% Allowed CPU Time.
The controls are not interchangeable: placement limits concurrency; Allowed CPU
Time pauses execution even when an app has only one busy thread. These results
support keeping the existing duty-cycle semantics. They do not select a new
automatic mode or default.

## Setup and evidence

- Intel Core 5 210H, one processor group, 12 logical processors: four SMT
  performance cores and four efficiency cores, as reported by native topology.
- Windows `10.0.26200.0`, AC power reported Online before and after the run.
- Balanced was active before and after:
  `381b4222-f694-41f0-9685-ff5bb260df2e`.
- Winderust `0.10.0-alpha`, release build with `runtime-benchmark`, Rust 1.94.1.
  Source base `e902b038c2fb190d3e71681640985929e5160e81` plus uncommitted
  development changes. The metadata retains hashes of changed source files.
- Binary SHA-256:
  `4C7DA169E0961F67224EB7EDAFB99C838652C034250CE436577111B1821994E6`.
- [Raw measurement](results/intel-core-5-210h-cpu-limiter-mechanisms-20261109.json)
  and [build/system metadata](results/intel-core-5-210h-cpu-limiter-mechanisms-20261109-metadata.json).
- [Reproduction instructions](../docs/development.md#cpu-limiter-mechanism-comparison).

Off inherited affinity `0xfff` with no CPU Sets. The half-pool used `0x30f`:
logical processors 0–3 and 8–9, comprising two whole SMT performance cores and
two efficiency cores. Selection was deterministic within each efficiency class,
kept eligible SMT siblings together, and never widened inherited placement.
This is a particular mixed-core selection, not a cache-domain comparison.

The 50% cases used the production CPU Limiter's Job Object freeze backend.
Hard placement used the existing CPU allocation coordinator. No user-facing
mode, setting, power-plan mutation, elevation request, or SeDebugPrivilege was
added or used by this comparison.

## Method

Each fresh disposable child ran one compute thread (ST) or 12 compute threads
(MT), plus its request handler. MT retained 12 compute threads in every condition,
so the half-pool condition oversubscribed its six logical processors.
Each thread repeatedly completed the checked 65,536-step integer xorshift kernel
used by the existing throughput script. A deterministic expected result checks
the kernel before measurement. Completed blocks, not allocated CPU time, define
useful throughput.

Each case warmed for two seconds and sampled for at least four seconds. Four
passes alternated condition and ST/MT order: odd passes used Off, Duty50,
Half-pool; even passes reversed that order. Duty50 remained the middle condition.
All 24 samples lasted 4.0005–4.0192 seconds.
MT Off and Duty50 throughput varied with pass order while the half-pool was
steadier. Alternating order reduces one-way drift but is not a fully randomized
design, so small differences should not be overinterpreted.

The parent sent PING, waited for the reply, then paused 13–23 ms with deterministic
dither. Round-trip time includes IPC, serialization, the reader thread, and
parent/target scheduling. These closed-loop probes send fewer requests during
stalls; their percentiles describe observed requests, not fixed-rate or
time-weighted event delays. Each sample retained 121–215 replies.

Counter reads were bracketed by the worker's monotonic clock. Both throughput
endpoints required a capture span of at most 2 ms; rejected counter endpoints
would still contribute their observed response latency. No endpoint was rejected
on this run; the largest recorded span was 88.8 microseconds. Native process CPU
time comes from the existing GetProcessTimes adapter.

## Throughput and recorded CPU time

Each cell is the median [minimum–maximum] of four sample values. Changes versus
Off are calculated within each matching pass and workload before aggregation.
CPU percentages use one logical processor as 100%, not the whole machine.

| Workload | Mechanism | Blocks/s | Change vs Off | Recorded CPU % |
| --- | --- | ---: | ---: | ---: |
| ST | Off | 5,853 [5,791–5,906] | 0% | 97.6 [97.1–98.8] |
| ST | Duty50 | 2,802 [2,774–2,821] | -52.1% [-53.0–-51.3] | 48.6 [48.4–49.4] |
| ST | Half-pool | 5,796 [5,704–5,840] | -1.6% [-2.2–+0.8] | 97.0 [95.4–98.3] |
| MT | Off | 41,183 [38,125–42,223] | 0% | 553.1 [527.5–574.0] |
| MT | Duty50 | 24,918 [23,915–26,278] | -38.3% [-40.2–-37.3] | 327.8 [309.4–353.5] |
| MT | Half-pool | 21,559 [21,396–21,708] | -47.7% [-49.2–-43.1] | 288.2 [281.5–293.3] |

Off's MT CPU time was substantially below twelve fully scheduled logical
processors despite all 12 compute counters advancing. The native wrapper's
kernel-plus-user arithmetic and report calculations were checked, but the cause
was not established. External scheduling/resource constraints and accounting
behavior were not isolated. Do not interpret this as evidence of a particular
CPU cap, full-machine saturation, or 50% limiter accuracy under unrestricted MT.

## Observed target response latency

Each cell is the median [minimum–maximum] across the four per-sample statistics.
P95 uses nearest rank within each sample; requests are not pooled across passes.

| Workload | Mechanism | Median RTT, ms | P95 RTT, ms |
| --- | --- | ---: | ---: |
| ST | Off | 0.243 [0.204–0.253] | 0.344 [0.309–0.373] |
| ST | Duty50 | 0.272 [0.269–0.276] | 47.863 [47.619–47.959] |
| ST | Half-pool | 0.245 [0.239–0.253] | 0.352 [0.342–0.469] |
| MT | Off | 0.257 [0.229–0.278] | 0.327 [0.322–0.339] |
| MT | Duty50 | 0.293 [0.275–0.297] | 43.491 [41.980–44.470] |
| MT | Half-pool | 0.246 [0.230–0.262] | 0.341 [0.308–0.370] |

The half-pool avoided freeze-related reply tails on this workload; it did not
improve reply tails over Off. This measures the restrained target's own response,
not protection of a separate foreground application.

## Individual throughput samples

| Pass | Workload | Off blocks/s | Duty50 blocks/s | Half-pool blocks/s |
| --- | --- | ---: | ---: | ---: |
| 1 | ST | 5,874.4 | 2,804.3 | 5,781.2 |
| 1 | MT | 40,172.1 | 24,023.3 | 21,396.4 |
| 2 | ST | 5,790.9 | 2,820.9 | 5,839.6 |
| 2 | MT | 42,222.7 | 25,813.2 | 21,462.5 |
| 3 | ST | 5,831.6 | 2,798.9 | 5,703.9 |
| 3 | MT | 38,125.4 | 23,915.1 | 21,707.7 |
| 4 | ST | 5,906.3 | 2,773.8 | 5,810.3 |
| 4 | MT | 42,193.1 | 26,277.6 | 21,655.0 |

## Restoration and limits

The run reported `complete: true` and `finished.txt` contained `ok`. All 24 unique
pass/workload/condition cells verified exact baseline affinity and CPU Sets
restoration, released controller state, and received STOP acknowledgement before
the child exited successfully. The recovery helper shut down successfully.
Portable debug settings had the same SHA-256 before and after; the headless
entry point never loads or saves settings.

Four short repeats on one live machine do not establish statistical significance
or a universal winner. The experiment did not isolate competing activity,
frequency, thermal state, or external resource controls. Parent CPU time is in
the raw data but excludes the recovery helper. Energy and independent foreground
latency were not measured. This register-heavy kernel does not characterize
cache-sensitive, memory-heavy, interactive, or real application workloads.
Automatic trigger/release behavior, descendants, conflicting rules, and abnormal
recovery are outside this fixed-mechanism comparison. Existing duty-cycle targets
and product behavior remain unchanged.
