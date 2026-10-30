# Long-settled hidden idle comparison: host gate stopped the attempt

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

The 2026-10-22 attempt produced **zero Wind UI observations and zero Iced
observations**. The host never met the predeclared eligibility limits during
the five-minute wait, so the runner stopped before launching either application.
This supplies no new application CPU-cycle, memory, or framework ranking result.

The unresolved question is whether Wind's residual hidden-idle cost decreases
after its observed RPC/threadpool cleanup, and whether it then consistently
matches or beats the frozen Iced application. The
[earlier short comparison](ui-idle-watcher-gating-20261022.md) and
[ETW investigation](ui-idle-etw-20261022.md) remain separate evidence.
Neither the earlier results nor this failed host gate establishes an idle floor.

## Frozen comparison protocol

The planned cohort contains four matched pairs, eight launches total, ordered
Wind/Iced, Iced/Wind, Wind/Iced, Iced/Wind. Each application would settle for
180 seconds after readiness verification, then be measured for 120 seconds at
1 Hz, with a 303-second readiness-relative fixture lifetime. These longer
observations would be kept separate from earlier 15-second warmup cohorts.

Both fixtures have automation disabled and use isolated settings. Wind's native
UI window is absent; Iced's is hidden. The primary process scope combines the
application and its actual recovery helper; collector overhead stays separate.
Private working set and private commit are distinct memory measures. This is a
comparison of two application builds, including their different integrations,
rather than isolated framework overhead. No ETW, GPU, energy, or thread-polling
instrumentation was enabled.

Before each pair, the runner requires a rolling window of 61 host readings
covering at least 60 actual seconds. The limits are CPU mean at most 5% of the
machine, CPU p95 at most 10%, at least the greater of 1 GiB or 10% of physical RAM
available throughout, and system commit at most 85% of its limit throughout.
Each gate waits at most 300 seconds. A failure stops before that pair; the runner
does not relax thresholds, resume, overwrite, or select quieter results afterward.
Eligibility before a pair would not guarantee a quiet host throughout the pair.

CPU utilization comes from native `GetSystemTimes` deltas. Physical availability
comes from `GlobalMemoryStatusEx`. System commit uses
`GetPerformanceInfo`'s `CommitTotal / CommitLimit`, rather than treating
process-limited `MEMORYSTATUSEX` page-file fields as system commit. See Microsoft's
[performance-information structure](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-performance_information),
[system CPU times](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-getsystemtimes), and
[memory-status structure](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/ns-sysinfoapi-memorystatusex).
The thresholds are experimental controls, not Windows health recommendations.

## Observed host gate

The attempt ran from **09:42:59 to 09:48:00 UTC**; the host gate itself waited
300.0048204 seconds. It retained all 300 host readings, comprising 299 intervals
and 240 rolling 61-reading windows. Of those, 239 span at least 60 seconds.
Every complete window failed each of the CPU mean, CPU p95, and system commit
limits. Physical RAM availability passed throughout.

The terminal diagnostic window spans **59.677 seconds**: its final sampling
interval was shortened at the overall deadline. The gate correctly rejects
windows shorter than 60 seconds. The preceding complete window spans
60.361 seconds and also fails the three resource limits. The terminal summary
therefore does not change the stop decision.

The last complete window provides the following diagnostic values:

| Last complete window metric | Observed | Required |
| --- | ---: | ---: |
| CPU mean, whole machine | 6.871% | At most 5% |
| CPU p95, whole machine | 18.139% | At most 10% |
| Maximum system commit | 87.786% | At most 85% |
| Minimum available physical RAM | 2,889,854,976 bytes | At least 1,676,380,569.6 bytes |
| Actual window duration | 60.361 s | At least 60 s |

The frozen runner contains an inaccurate comment that 61 readings necessarily
span 60 seconds; the deadline-shortened last interval is an exception. Its
actual elapsed-time eligibility check is correct. A future runner should report
the last complete window separately from a terminal partial window. The frozen
source and raw evidence for this attempt are preserved unchanged.

## Evidence and validation

The [evidence bundle](../benchmark-results/archive-20261028/benchmark/ui-settled-idle-results-20261022/README.md) preserves the
protocol, configuration, runner, analyzer, collector source, run metadata, gate
summary, and every raw host reading, with a checksum manifest and verifier.
There are no application resource CSVs or synthesized zero-valued results.
The runner's nonzero exit reports `host_gate_ineligible`; it is the planned
stop condition rather than an application crash or failed collector launch.

The runner and analyzer passed 19 pure checks before capture. Independent review
recomputed every rolling window, native CPU ratio, system commit conversion,
and evidence hash. No window was eligible, no pair collector ran, and the
accepted observation count is zero. The active Balanced power plan remained
`381b4222-f694-41f0-9685-ff5bb260df2e` before and after the attempt.

No Rust product code, Wind UI dependency source, executable, saved settings,
or power settings changed for this attempt. The selected fixture hashes are:

| Build | SHA-256 |
| --- | --- |
| Current Wind | `35721b77ad06a86b61a6137c68747e593b9950fb455998ce88eba19d0c07f8e1` |
| Frozen Iced | `2d040026dae4b842a3853f5503817a15a00abd097f24e4a681749aa6976d2b82` |

A fresh, separately identified attempt needs a quieter host. This attempt cannot
support a claim that Wind cannot get lower, that it has reached Iced's cost, or
that either application uses more energy. No unnecessary user processes or
services were stopped to force eligibility.
