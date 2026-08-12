# Phase 4 typed process control and Dynamic Priority Boost

- Status: **complete**
- Date: 2026-08-11
- Refactor contract: [architecture-refactor-plan.md](../architecture-refactor-plan.md)
- Phase 3 evidence: [phase-3-power-events.md](phase-3-power-events.md)

Phase 4 introduces the process identity/access foundation and migrates Dynamic Priority Boost as
one complete mechanism. Static Priority Control, the Adaptive Engine replacement policy, and
Process List one-shot actions now share one controller, one Windows mutation adapter, one captured
baseline chain, and one clean-release authority. The crash helper remains the independent crash
recovery mirror.

## Ownership and flow

```text
static Dynamic Priority Boost policy ----\
                                         +-> policy resolver -> ControlOwner claim --\
Adaptive Engine replacement policy -----/                                      |
                                                                                v
Process List -> bounded result-bearing runtime command -----------------> RuntimeCore
                                                                                |
                                                                                v
                                                        DynamicPriorityBoostController
                                              exact identity / baseline / expected / owner
                                                                                |
                                              Begin -> apply -> verify -> Commit
                                                                                |
                                                                                v
                                  one platform/windows SetProcessPriorityBoost adapter
                                                                                |
                                                                                v
                                                          crash-recovery journal mirror
```

`RuntimeCore` owns the controller for the worker lifetime. The feature manager retains policy
selection, Focus App > Visible Window > Background tiering, rules, exclusions, failure suppression,
Action Log reporting, and status. It emits typed claims instead of owning process handles,
baselines, or setters. Adaptive settings still replace static settings while Adaptive priority
assist is active; the effective owner changes in the same reconciliation without bouncing through
the captured baseline.

The previous dormant Workload Engine Dynamic Priority Boost request flag, stored fields, setter,
and restore branch were deleted. No unused mutation capability remains outside the controller.

The raw live `GetProcessPriorityBoost` / `SetProcessPriorityBoost` calls now reside in
`src/platform/windows/dynamic_priority_boost.rs`. The controller still owns opening, Begin,
apply/verify, Commit, compensation, recovery relinquishment, baseline/expected state, and release;
the physical adapter split does not create a second state or restoration owner.

## Identity, access, and restoration contract

Every mutation reopens and validates a `ProcessControlTarget` immediately on the runtime worker.
The resulting exact identity includes PID, creation time, executable path, executable name, and
session. A PID alone never authorizes a write. The boundary rejects Winderust itself, PID 0,
relative or changed paths, changed process names, critical or unverifiable processes, Windows
protected processes, denied `PROCESS_SET_INFORMATION` access, and disallowed cross-session
targets. Process List capture is advisory; the current Winderust Behaviour cross-session setting
is checked again immediately before the write.

The controller records the first baseline only when Winderust actually changes a process. A target
that already has the requested state is not adopted. Later static, Adaptive, or Process List
transitions preserve that first baseline and update the expected state and effective owner. A
Process List action is immediate but is not a persistent highest-priority claim; the next automatic
reconciliation may supersede it.

Clean release runs in reverse successful-application order. It restores only when the live process
still has Winderust's expected value. A changed external value breaks the ownership chain and is
left untouched after an acknowledged exact-property journal relinquishment. If relinquishment
fails, the controller retains the chain and reports the failure rather than silently losing its
recovery owner. Exited processes and reused PIDs are dropped without targeting the replacement
instance. General disable releases automatic claims; Process List state remains tracked until it
is superseded or Winderust shuts down, so every Winderust-applied change still has a clean exit
path. The controller also retries release from `Drop` if the worker unwinds unexpectedly.

## Result-bearing Process List command

Dynamic Priority Boost no longer performs a synchronous Win32 mutation inside a GPUI entity
update and no longer registers a UI restore closure. The menu submits one whole target batch to a
bounded FIFO on the existing automation worker. Every captured target is attempted in order, and
the typed reply preserves per-target applied, unchanged, or failed results. The UI waits for the
reply on GPUI's background executor and then publishes the existing `N of M` partial-failure
summary on the foreground executor.

The queue is bounded at 32 batches. Full and stopped states are typed errors. Shutdown drains
commands that have not reached the worker with `RuntimeStopped`, so no waiter can hang. A manual
change made while automatic features are disabled keeps the existing worker parked on its
condition variable until settings, another command, or shutdown wakes it; it does not create a
second worker or polling loop. A worker-scope exit guard also marks the worker unavailable and
replies `WorkerExited` to abandoned commands on an unexpected return or panic. The orderly idle
handoff leaves newer commands queued for the single replacement worker.

## Recovery transaction and failure behavior

Every forward transition and clean restoration uses the same mechanism-specific transaction:

```text
open and revalidate -> query actual -> recovery Begin -> SetProcessPriorityBoost
                    -> query/verify -> recovery Commit
```

Begin failure prevents the write. A forward apply, verification, or Commit failure attempts
immediate compensation to the pre-transition value. A successfully compensated transition drops
or cancels its pending intent. If compensation cannot prove restoration, the controller keeps
uncertain state for a later clean-release attempt and commits an available recovery intent so the
helper can still repair the externally persistent process state after a crash.

Clean release has a stricter inverse-transaction rule. Once the baseline has been applied and
verified, a failed Commit does not put Winderust's managed value back into the process. The
controller keeps the baseline, relinquishes the now-ambiguous property journal, and completes the
release. If that relinquishment fails, it retains a baseline-state record and retries only the
journal relinquishment on the next release. Recovery replay retains its own exact identity and
expected-state guard.

## Validation evidence

Deterministic coverage includes unmanaged matching state, Process List-to-policy supersession,
static-to-Adaptive replacement, external-state rebase and guarded release, process exit and PID
reuse, Begin/apply/verify/Commit failures, failed compensation, reverse clean shutdown, bounded
queue behavior, stopped and unexpectedly exited worker replies, partial batch failures, and
idle-worker retention. A live recovery test changes Dynamic Priority Boost on a disposable process
and replays its exact-identity crash journal. A separate ignored Windows integration test changes
and restores the property through the production controller adapter.

The default and `architecture-diagnostics` suites each pass **483 tests**, with three explicit live
tests ignored by the ordinary matrix. All three pass when run explicitly with the required Windows
permissions: the Dynamic Priority Boost production-adapter test, App Suspension named-job
recovery, and disposable power-plan recovery.

`cargo fmt -- --check`, both strict Clippy configurations, `git diff --check`, the compatibility
scan, and every architecture ownership assertion pass. The ownership gate finds exactly one
production `SetProcessPriorityBoost` call in the platform adapter, permits the independent
recovery-only writer, and finds no raw Win32 in the controller or legacy feature/Workload Engine
restore owner.

## Same-host release comparison

The Phase 4 release benchmark uses the same host, isolated portable settings, 30-second footprint
windows, five process-priority action trials, and `architecture-diagnostics` instrumentation as the
Phase 3 artifact.

| Metric | Phase 3 | Phase 4 |
| --- | ---: | ---: |
| Idle CPU median / mean / P95 (% total capacity) | 0 / 0.0918 / 0.2530 | 0 / 0.1289 / 0.4984 |
| Idle working set / private memory median | 82.4180 / 125.4688 MiB | 81.8008 / 124.0703 MiB |
| Idle threads / handles median | 38 / 751 | 38 / 749 |
| Active CPU median / mean / P95 (% total capacity) | 0.2410 / 0.1668 / 0.5040 | 0 / 0.1543 / 0.5004 |
| Active working set / private memory median | 84.1250 / 126.1055 MiB | 82.4922 / 125.3867 MiB |
| Active threads / handles median | 40 / 750 | 40 / 750 |
| Reconciliation passes / wake frequency | 42 / 1.0285 Hz | 36 / 0.8809 Hz |
| Process / foreground / visible-window scans | 41 / 21 / 20 | 34 / 18 / 17 |
| Process-appearance-to-action median / P95 | 69.6520 / 86.4457 ms | 65.1059 / 78.0559 ms |
| Applied actions / failures | 6 / 0 | 6 / 0 |
| Clean priority restoration | Pass | Pass |

Idle remained dormant. Its median CPU stayed at zero; the P95 and mean rose by 0.2454 and 0.0371
percentage points respectively, while working set, private memory, and handles declined. The
active sample used 13 accepted window-created events versus 25 in Phase 3, so its six fewer passes,
lower scan counts, and latency improvements are not attributed solely to this refactor. Both runs
applied every action without failure and restored the retained target cleanly.

Artifacts:

- `benchmark/results/architecture-phase-3-20260811.json`
- `benchmark/results/architecture-phase-4-20260811.json`

## Exit-gate evidence

| Phase 4 gate | State | Evidence |
| --- | --- | --- |
| Exactly one production Dynamic Priority Boost adapter | Pass | Architecture ownership assertion plus recovery-only allowlist |
| Manual, static, Adaptive, external-break, disable, exit, shutdown, and crash behavior | Pass | Controller failure matrix, runtime command tests, disposable-process recovery, and live adapter test |
| PID reuse and protected/inaccessible/cross-session targets fail closed | Pass | Creation-bound target keys, worker-side identity/access validation, and process-action safety tests |
| No duplicate restoration authority remains | Pass | Feature restore map and Process List closure removed; dormant Workload Engine capability deleted |
| Result-bearing commands cannot strand GPUI waiters | Pass | Bounded queue, shutdown drain, panic-safe worker guard, and orderly handoff tests |

Phase 4 is independently revertible only as the complete property slice. A rollback must restore
the old feature manager, Process List quick action, and Workload Engine capability together; it
must not leave the legacy and typed writers active in the same build.
