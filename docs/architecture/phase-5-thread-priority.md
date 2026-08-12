# Phase 5.1 typed Thread Priority control

- Status: **complete**
- Date: 2026-08-11
- Refactor contract: [architecture-refactor-plan.md](../architecture-refactor-plan.md)
- Previous mechanism: [phase-4-dynamic-priority-boost.md](phase-4-dynamic-priority-boost.md)

This slice converts Thread Priority as one complete Windows-property boundary. Static Priority
Control, the Adaptive Engine replacement policy, and Process List one-shot actions now share one
`ThreadPriorityController` in `RuntimeCore`. The feature manager owns policy and reporting only;
the controller owns exact identities, baselines, expected values, effective owners, transitions,
and clean release. `src/platform/windows/thread_priority.rs` owns the narrow enumeration,
thread-handle, identity-read, and priority query/set adapter. The crash helper remains the
independent recovery mirror.

## Ownership and flow

```text
static Thread Priority policy ---------\
                                        +-> policy resolver -> owner-tagged process claim --\
Adaptive Engine replacement policy ---/                                           |
                                                                                    v
Process List -> bounded result-bearing command -----------------------------> RuntimeCore
                                                                                    |
                                                                                    v
                                                               ThreadPriorityController
                                                      enumerate current process threads
                                                                                    |
                                           process identity + thread ID + creation time
                                                                                    |
                                           Begin -> apply -> verify -> Commit
                                                                                    |
                                                                                    v
                                     platform/windows/thread_priority.rs raw adapter
                                                                                    |
                                                                                    v
                                                        crash-recovery journal mirror
```

The policy manager preserves Focus App > Visible Window > Background tiering, path rules,
exclusions, failure suppression, auto-exclusion publication, status, and Action Log behavior.
Adaptive settings replace rather than merge with static settings while priority assist is active.
The owner changes from `ThreadPriority` to `AdaptiveEngine` in the same reconciliation, without
restoring through an intermediate baseline.

## Identity and access contract

An automatic claim is bound to the observed process creation time and executable path before it
reaches the controller. The runtime worker then reopens and validates the process through the
shared process-control boundary. It rejects Winderust itself, PID 0, changed or relative paths,
changed names, critical or unverifiable processes, Windows protected processes, disallowed
cross-session targets, and inaccessible handles.

Each enumerated thread is identified by the verified `ProcessIdentity`, thread ID, and thread
creation time. The adapter rechecks the thread owner and creation time immediately before reads,
journal creation, and writes. PID or thread-ID reuse therefore relinquishes only the obsolete
exact journal entry and never authorizes a write to the replacement instance. Thread access is
probed with thread-specific query/set rights; the controller does not require unrelated process
set-information rights.

## Baselines, preservation, and restoration

The controller captures a separate first baseline for each thread only when Winderust actually
changes it. This fixes mixed-priority processes: Process List can change every enumerated thread
and later restore each one to its own original value. Threads already at the desired value are not
adopted. New threads are discovered on later automatic passes; exited threads and reused IDs are
relinquished by exact identity.

Priority preservation uses the original per-thread baseline, matching the existing product
contract. Focus App and Visible Window can preserve a baseline at or above the requested level;
Background can preserve a baseline at or below it. If a later policy says to preserve a thread
that Winderust already manages, the controller safely releases it back to that baseline.

A Process List action is immediate and non-persistent. It shares the same baseline chain, but the
next applicable static or Adaptive reconciliation may supersede it. General disable releases only
automatic ownership; Process List state remains tracked until superseded or Winderust shutdown.
All release runs in reverse successful-application order.

Clean release writes the baseline only while the live value still equals Winderust's expected
value. An external change breaks ownership and is left untouched after an acknowledged exact
thread-property journal relinquishment. Failed relinquishment keeps the managed record for retry.
Begin, apply, verification, and Commit failures use compensation; ambiguous state stays tracked
and recoverable. Once a clean-release baseline has been applied and verified, a failed Commit does
not reapply Winderust's old value: the controller keeps the baseline and retries journal
relinquishment only.

## Shared Process List command queue

The Phase 4 Dynamic Priority Boost queue is now a bounded cross-property FIFO. Thread Priority and
Dynamic Priority Boost batches use the same existing worker and typed result envelope. Every
captured process target is attempted, partial failures retain the first concrete error, and GPUI
waits on its background executor before publishing the result on the foreground executor. No
Thread Priority setter or restore closure runs in a GPUI entity update.

Shutdown and unexpected worker exit reply to every abandoned result-bearing command. Manual-only
managed state keeps the single worker parked without polling until another command, settings
change, or shutdown. Cross-property FIFO order is characterized so later mechanism conversions
can reuse the boundary without adding an actor or worker.

## Validation evidence

Deterministic tests cover mixed baselines, unmanaged matching state, Process List-to-policy
supersession, static-to-Adaptive replacement, Focus/Visible/Background preservation direction,
new and exited threads, PID and thread-ID reuse, external-state breaks, failed relinquishment,
Begin/apply/verify/Commit faults, clean-release Commit ambiguity, retry without reapplication,
reverse shutdown, cross-property FIFO order, bounded/stopped/panicked queues, and exact crash
journal forget behavior. The ordinary recovery suite also mutates and restores a disposable
process thread through the crash helper. An ignored Windows integration test exercises live apply
and clean release through the production controller adapter.

The architecture ownership gate permits `SetThreadPriority` only in the Windows production adapter
and recovery replay. It asserts exactly one production call, rejects raw Toolhelp/thread APIs from
the controller, rejects policy imports from the adapter, and proves the feature manager and Process
List contain no legacy setter, query helper, baseline record, or restore closure.

The default and `architecture-diagnostics` suites each pass **507 tests**, with four explicit live
tests ignored by the ordinary matrix. All four pass when run with the required Windows
permissions: Thread Priority and Dynamic Priority Boost production adapters, App Suspension
named-job recovery, and disposable power-plan recovery. Both strict Clippy configurations,
formatting, compatibility scan, ownership assertions, and diff checks pass.

## Same-host release comparison

The Phase 5.1 release benchmark uses the same Intel Core 5 210H host, isolated portable settings,
30-second footprint windows, five process-priority action trials, and
`architecture-diagnostics` instrumentation as Phase 4. Thread Priority is disabled in this
general regression benchmark, so its mechanism correctness comes from the live adapter test; the
benchmark verifies that adding its controller and shared command variant does not create idle
work or regress the existing reconciliation path.

| Metric | Phase 4 | Phase 5.1 |
| --- | ---: | ---: |
| Idle CPU median / mean / P95 (% total capacity) | 0 / 0.1289 / 0.4984 | 0 / 0.0841 / 0.2544 |
| Idle working set / private memory median | 81.8008 / 124.0703 MiB | 81.5703 / 124.4844 MiB |
| Idle threads / handles median | 38 / 749 | 39 / 751 |
| Active CPU median / mean / P95 (% total capacity) | 0 / 0.1543 / 0.5004 | 0 / 0.1304 / 0.4994 |
| Active working set / private memory median | 82.4922 / 125.3867 MiB | 82.2656 / 124.1680 MiB |
| Active threads / handles median | 40 / 750 | 40 / 750 |
| Reconciliation passes / wake frequency | 36 / 0.8809 Hz | 28 / 0.6855 Hz |
| Process / foreground / visible-window scans | 34 / 18 / 17 | 26 / 19 / 18 |
| Process-appearance-to-action median / P95 | 65.1059 / 78.0559 ms | 57.3135 / 59.8749 ms |
| Applied actions / failures | 6 / 0 | 6 / 0 |
| Clean priority restoration | Pass | Pass |

Idle remained dormant with zero median CPU. Mean and P95 CPU declined in both cases, while idle
private memory and thread/handle counts moved slightly upward and active memory declined. Phase
5.1 observed 12 window-created events versus 13 in Phase 4, so its lower pass/scan counts and
latency are not attributed solely to this refactor. Every action was observed without failure and
the retained process priority restored cleanly.

Artifacts:

- `benchmark/results/architecture-phase-4-20260811.json`
- `benchmark/results/architecture-phase-5-thread-priority-20260811.json`

## Exit gate

| Thread Priority gate | State | Evidence |
| --- | --- | --- |
| Static, Adaptive, and Process List producers share one controller | Pass | Owner resolver, runtime command route, and replacement tests |
| Process and thread reuse fail closed | Pass | Creation-bound process claims and exact `ThreadIdentity` tests |
| External state is never overwritten during release | Pass | Expected-state guard and journal-relinquishment tests |
| Mixed thread baselines restore independently | Pass | Multi-thread Process List apply/release test |
| No duplicate live writer or restoration authority remains | Pass | Ownership script and source-zero assertions |
| GPUI cannot be blocked by Thread Priority mutation | Pass | Background result wait and shared queue lifecycle tests |

Rollback must revert this entire property slice: controller, runtime command variant, policy route,
Process List route, recovery forget command, and ownership assertions. Restoring only one legacy
producer would recreate duplicate mutation and restoration authority.
