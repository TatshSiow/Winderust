# Phase 5.2 typed I/O Priority control

- Status: **complete**
- Date: 2026-08-11
- Refactor contract: [architecture-refactor-plan.md](../architecture-refactor-plan.md)
- Previous mechanism: [phase-5-thread-priority.md](phase-5-thread-priority.md)

This slice converts I/O Priority as one complete Windows-property boundary. Static Priority
Control, the Adaptive Engine replacement policy, and Process List one-shot actions now share one
`IoPriorityController` in `RuntimeCore`. The feature manager owns policy and reporting only; the
controller owns exact identities, raw baselines, expected values, effective owners, transitions,
and clean release. The crash helper remains the independent recovery mirror.

## Ownership and flow

```text
static I/O Priority policy ------------\
                                        +-> policy resolver -> owner-tagged process claim --\
Adaptive Engine replacement policy ---/                                           |
                                                                                    v
Process List -> bounded result-bearing command -----------------------------> RuntimeCore
                                                                                    |
                                                                                    v
                                                                  IoPriorityController
                                                       PID + creation time + exact path
                                                                                    |
                                           Begin -> apply -> verify -> Commit
                                                                                    |
                                                                                    v
                                   one platform/windows NtSetInformationProcess adapter
                                                                                    |
                                                                                    v
                                                        crash-recovery journal mirror
```

The policy manager preserves Focus App > Visible Window > Background tiering, path rules,
exclusions, failure suppression, auto-exclusion publication, status, and Action Log behavior.
Adaptive settings replace rather than merge with static settings while Workload Engine I/O
assistance is active. The owner changes from `IoPriority` to `AdaptiveEngine` in the same
reconciliation without restoring through an intermediate baseline.

## Identity, access, and the NT boundary

Every automatic claim is bound to the creation time and executable path captured by the shared
cycle observation. The runtime worker reopens and revalidates that exact process immediately
before reading, journaling, or writing. The shared safety boundary rejects Winderust itself, PID
0, changed or relative paths, changed names, critical or unverifiable processes, protected
processes, disallowed cross-session targets, and inaccessible handles.

Windows exposes this control through internal `NtQueryInformationProcess` and
`NtSetInformationProcess` calls with numeric information class 33. Winderust keeps both declarations
and the class constant beside the single production adapter. This remains a compatibility-sensitive
contract rather than a stable public SDK surface.
That adapter now resides in `src/platform/windows/io_priority.rs`; controller-owned transaction,
raw baseline, recovery, preservation, and restoration state did not move.

The controller stores the raw `u32` returned by Windows as its baseline instead of coercing unknown
values to `Normal`. Selectable values 0 through 4 map to Very Low, Low, Normal, High, and Critical;
an unknown value disables the Process List selection display but can still be captured and restored
exactly by automatic policy. This preserves state Winderust does not understand instead of silently
rewriting it.

## Baselines, preservation, and restoration

The first successful Winderust mutation captures one baseline for the exact process instance.
Process List actions, static policy, and Adaptive replacement all continue the same baseline and
expected-value chain. A Process List action is immediate and non-persistent; the next applicable
automatic reconciliation may supersede it. A process already at the requested value is not adopted.

Priority preservation is evaluated against the original baseline. Focus App and Visible Window
can preserve a baseline at or above the requested priority, while Background can preserve a
baseline at or below it. If a later claim preserves a process already managed by Winderust, the
controller releases it back to the original baseline before dropping ownership.

Clean release writes the baseline only while the live value still equals Winderust's expected
value. An external change breaks ownership and remains untouched after an acknowledged
exact-process/property journal relinquishment. Failed relinquishment keeps the managed record for
retry. Begin, apply, verification, and Commit failures compensate where possible; ambiguous state
stays tracked and recoverable. Once a release has applied and verified the baseline, a failed
Commit keeps that baseline and retries only journal relinquishment rather than reapplying the old
Winderust value. Release and shutdown run in reverse successful-application order.

## Shared Process List command path

I/O Priority joins Dynamic Priority Boost and Thread Priority in the bounded, result-bearing
process-control FIFO owned by `RuntimeHandle`. The runtime worker processes automatic I/O policy
before manual commands in a pass, so a click is immediate and a later reconciliation can supersede
it. Every captured target in a stacked row is attempted, and partial failures preserve the first
concrete error.

The Process List now uses one generic typed runtime-priority submenu for all converted priority
properties. GPUI performs no I/O Priority mutation inside an entity update: it queues the command,
waits for the synchronous receiver on the background executor, and publishes the result on the
foreground executor. Shutdown and unexpected worker exit reply to every abandoned command, while
manual-only managed state keeps the single worker parked without polling.

## Validation evidence

Deterministic tests cover unmanaged matching state, Process List-to-policy supersession,
static-to-Adaptive replacement, Focus/Visible/Background preservation, stable identity despite
metadata drift, external-state break and failed relinquishment, process exit and PID reuse,
unknown raw baseline restoration, Begin/apply/verify/Commit faults, failed compensation, release
Commit ambiguity, relinquishment retry without reapplication, reverse shutdown, the shared command
FIFO, and worker shutdown/panic replies. The crash-recovery suite mutates and restores I/O priority
on a disposable process. An ignored Windows integration test exercises live apply and clean release
through the production controller adapter.

The ownership gate permits `NtSetInformationProcess` only in the Windows platform adapter and
recovery replay. It asserts one adapter declaration/call pair, rejects raw NT calls from the
controller, and proves the feature manager, Process List, and Workload Engine contain no I/O
Priority setter, baseline record, or restore owner.

The default and `architecture-diagnostics` suites each pass **520 tests**, with five explicit live
tests ignored by the ordinary matrix. The I/O Priority production adapter and crash-recovery tests
both pass against disposable processes, as do the existing Thread Priority, Dynamic Priority
Boost, and App Suspension live tests. The combined ignored-test run is blocked only by the
independent disposable power-plan test when this non-elevated shell receives Windows error 5.
Both strict Clippy configurations, formatting, compatibility scan, ownership assertions, and diff
checks pass.

## Same-host release comparison

The Phase 5.2 release benchmark uses the same Intel Core 5 210H host, isolated portable settings,
30-second footprint windows, five process-priority action trials, and
`architecture-diagnostics` instrumentation as Phase 5.1. I/O Priority is disabled in this general
regression benchmark, so mechanism correctness comes from its controller and recovery live tests;
the benchmark verifies that adding the controller and shared command variant does not create idle
work or break the established reconciliation and restoration path.

| Metric | Phase 5.1 | Phase 5.2 |
| --- | ---: | ---: |
| Idle CPU median / mean / P95 (% total capacity) | 0 / 0.0841 / 0.2544 | 0 / 0.1207 / 0.2521 |
| Idle working set / private memory median | 81.5703 / 124.4844 MiB | 85.1719 / 129.2656 MiB |
| Idle threads / handles median | 39 / 751 | 39 / 751 |
| Active CPU median / mean / P95 (% total capacity) | 0 / 0.1304 / 0.4994 | 0.2467 / 0.1832 / 0.5000 |
| Active working set / private memory median | 82.2656 / 124.1680 MiB | 82.5312 / 125.6289 MiB |
| Active threads / handles median | 40 / 750 | 40 / 748 |
| Reconciliation passes / wake frequency | 28 / 0.6855 Hz | 31 / 0.7533 Hz |
| Process / foreground / visible-window scans | 26 / 19 / 18 | 30 / 18 / 17 |
| Process-appearance-to-action median / P95 | 57.3135 / 59.8749 ms | 69.0976 / 72.0123 ms |
| Applied actions / failures | 6 / 0 | 6 / 0 |
| Clean priority restoration | Pass | Pass |

Idle remained fully dormant with zero worker passes, wakes, or inventory scans and unchanged
thread/handle counts. Its working-set and private-memory medians rose by 3.6016 MiB and 4.7812 MiB,
while active medians rose by 0.2656 MiB and 1.4609 MiB. Phase 5.2 accepted 15 window-created events
versus 12 in Phase 5.1, so the higher active pass/scan counts, CPU median, and action latency are not
attributed solely to this disabled mechanism. P95 CPU remained effectively unchanged, every action
was observed without failure, and the retained process priority restored cleanly.

Artifacts:

- `benchmark/results/architecture-phase-5-thread-priority-20260811.json`
- `benchmark/results/architecture-phase-5-io-priority-20260811.json`

## Exit gate

| I/O Priority gate | State | Evidence |
| --- | --- | --- |
| Static, Adaptive, and Process List producers share one controller | Pass | Owner resolver, runtime command route, and replacement tests |
| PID reuse and metadata drift fail closed without losing the live baseline | Pass | Creation-bound target and stable identity tests |
| Unknown Windows values restore exactly | Pass | Raw-baseline round-trip test |
| External state is never overwritten during release | Pass | Expected-state guard and journal-relinquishment tests |
| No duplicate live writer or restoration authority remains | Pass | Ownership script and source-zero assertions |
| GPUI cannot be blocked by I/O Priority mutation | Pass | Background result wait and shared queue lifecycle tests |

Rollback must revert this entire property slice: controller, runtime command variant, policy route,
Process List route, recovery forget helper, and ownership assertions. Restoring only one legacy
producer would recreate duplicate mutation and restoration authority.
