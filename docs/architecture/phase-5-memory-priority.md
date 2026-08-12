# Phase 5.4 typed Memory Priority control

- Status: **complete**
- Date: 2026-08-11
- Refactor contract: [architecture-refactor-plan.md](../architecture-refactor-plan.md)
- Previous mechanism: [phase-5-gpu-priority.md](phase-5-gpu-priority.md)

This slice converts Memory Priority as one complete Windows-property boundary. Static Memory
Priority, Workload Engine Memory Priority, and Process List one-shot actions now share one
`MemoryPriorityController` in `RuntimeCore`. Feature managers own target policy, suppression,
status, and Action Log attribution only. The controller owns exact process identities, simultaneous
owner claims, the effective owner, raw baselines, expected values, transitions, and clean release.
The crash helper remains the independent recovery mirror.

## Ownership and arbitration

```text
static Memory Priority policy ---------------------- owner: MemoryPriority --\
                                                                        explicit precedence
Workload Engine pressure policy -------------------- owner: AdaptiveEngine ---+--> effective claim
                                                                                     |
Process List -> bounded result-bearing command -------------------------------+     |
                                                                               v     v
                                                                    RuntimeCore worker
                                                                               |
                                                                               v
                                                                MemoryPriorityController
                                                    PID + creation time + exact path
                                                                               |
                                      Begin -> apply -> verify -> Commit        |
                                                                               v
                     one platform/windows SetProcessInformation adapter -> recovery mirror
```

Memory Priority differs from the earlier Phase 5 mechanisms because both automatic producers can
be active at once. Static Memory Priority explicitly has higher precedence for an overlapping exact
process instance. A Workload Engine claim remains stored underneath it and becomes effective again
when the static claim disappears. Workload claims for other processes remain effective throughout;
there is no global static-versus-Adaptive mode switch.

This preserves the Phase 0 product result that static Memory Priority wins an overlap, but replaces
the old timing-dependent last-writer behavior with deterministic arbitration. Replacing an owner
does not restore through the pre-Winderust baseline. The first successful Winderust mutation keeps
one baseline across Workload-to-static-to-Workload transitions.

## Policy behavior

Static policy retains Focus App > Visible Window > Background tiering, exact-path rule semantics,
exclusions, cross-session policy, preservation settings, normalized-path failure suppression,
auto-exclusion publication, status, and Memory Priority Action Log summaries. Workload Engine
retains its foreground, visible-window, and selected pressure-target construction; its claims exist
only while the Adaptive Engine, Workload Engine, pressure restraint, and Memory Priority assistance
conditions are active. Workload outcomes continue to use the Workload Engine Action Log feature.

Focus App and Visible Window preservation keep an original baseline at or above the requested
priority. Background preservation keeps a baseline at or below the requested priority. Preservation
uses the first raw pre-Winderust baseline rather than a value written by another Winderust owner.
If a higher-precedence preserving claim covers a process already managed by a lower owner, the
controller returns the process to the original baseline while keeping the lower claim dormant.

Suppressed targets remain active for release accounting but do not submit new writes. A failed new
claim is not retained as hidden future work; a previously successful claim is retained if a
replacement value fails, so the last established policy remains the fallback.

## Identity, transition, and recovery safety

Every automatic target carries creation time and executable path from the pass-local process
observation. The runtime worker reopens and revalidates that exact process immediately before every
read or write. The shared process-control boundary rejects PID 0, Winderust itself, relative or
changed paths, changed process names, critical or unverifiable processes, protected processes,
disallowed cross-session targets, and inaccessible handles.

The Windows adapter keeps raw `u32` values from `MEMORY_PRIORITY_INFORMATION`. Known selectable
values map to Very Low, Low, Medium, Below Normal, and Normal. An unknown live value is not collapsed
to Normal: Process List fails closed, while a raw unknown baseline remains restorable.
The adapter is physically isolated in `src/platform/windows/memory_priority.rs`; controller-owned
arbitration, transaction, recovery, and restoration state did not move.

Each transition records crash recovery Begin, writes with
`SetProcessInformation(ProcessMemoryPriority)`, reads the value back, and commits only after exact
verification. Apply and verification failures compensate to the prior value. Ambiguous state stays
tracked. A release that reaches and verifies the baseline before Commit fails keeps the baseline and
retries only recovery-journal relinquishment.

Clean release writes the baseline only while the live value still equals Winderust's expected value.
An external change breaks ownership and is left untouched after the exact process/property journal
entry is relinquished. Process exit and PID reuse relinquish the stale journal without targeting the
replacement. Shutdown clears all policy claims and restores managed instances in reverse successful
application order, including Process List-only state.

## Process List command route

Memory Priority joins Dynamic Priority Boost, Thread Priority, I/O Priority, and GPU Priority in the
bounded result-bearing process-control FIFO. Automatic Memory policy executes before manual commands
in a pass, so the click is immediate and only a later applicable automatic reconciliation may
supersede it. A stacked row attempts every captured process target and keeps the existing partial
failure summary.

The menu reads the first target whose Memory Priority can be queried rather than assuming the group
representative is usable. GPUI performs no process mutation inside an entity update: it queues the
typed command, waits on the background executor, then publishes completion on the foreground
executor. The legacy `"memory-priority"` UI restore closure and direct feature setter are gone.

## Validation evidence

Deterministic tests cover known and unknown raw values, static-over-Workload arbitration,
non-overlapping and shadowed claims, lower-owner reveal without a baseline bounce, failed owner
replacement retry, Process List supersession, Focus/Visible/Background preservation, external-state
breaks, PID reuse, Begin/apply/Commit failures, release Commit ambiguity, and reverse shutdown. The
runtime suite covers the Memory Priority command reply, cross-property FIFO ordering, automation
ordering, managed-state lifetime, and shutdown ordering.

The ownership script permits a Memory Priority `SetProcessInformation` call only in the Windows
platform adapter and crash-recovery replay. It asserts exactly one production call, rejects raw
Win32 from the controller, and proves the feature manager, Workload Engine, and Process List contain
no Memory Priority writer or legacy restoration owner.

The default and `architecture-diagnostics` suites each pass **551 tests**, with nine explicit
Windows integration tests ignored by the ordinary matrix. The two Memory Priority integration
tests were then run explicitly against disposable `System32\\PING.EXE` process instances:
production-controller apply/verify/clean-release passed, and crash-recovery replay restored the
captured raw Memory Priority. Strict Clippy passes in both configurations.

## Same-host release comparison

The Phase 5.4 release benchmark uses the same Intel Core 5 210H host, isolated portable settings,
30-second footprint windows, five Process Priority action trials, and
`architecture-diagnostics` instrumentation as Phase 5.3. Memory Priority is disabled in this
general regression benchmark, so mechanism correctness comes from the two explicit integration
tests; this comparison checks that adding the controller and command route creates no idle work and
does not break the established reconciliation or restoration path.

| Metric | Phase 5.3 | Phase 5.4 |
| --- | ---: | ---: |
| Idle CPU median / mean / P95 (% total capacity) | 0 / 0.1109 / 0.2497 | 0 / 0.0787 / 0.2521 |
| Idle working set / private memory median | 85.6133 / 127.4180 MiB | 84.3594 / 127.4336 MiB |
| Idle threads / handles median | 38 / 751 | 38 / 749 |
| Active CPU median / mean / P95 (% total capacity) | 0 / 0.1438 / 0.4931 | 0.2497 / 0.2508 / 0.7523 |
| Active working set / private memory median | 85.6523 / 127.7539 MiB | 82.1133 / 123.1289 MiB |
| Active threads / handles median | 40 / 750 | 40 / 750 |
| Reconciliation passes / wake frequency | 37 / 0.8947 Hz | 34 / 0.8298 Hz |
| Process / foreground / visible-window scans | 33 / 18 / 17 | 33 / 18 / 17 |
| Process-appearance-to-action median / P95 | 63.2045 / 74.2309 ms | 65.0675 / 68.5434 ms |
| Applied actions / failures | 6 / 0 | 6 / 0 |
| Clean priority restoration | Pass | Pass |

Idle remained fully dormant with zero reconciliation passes, wakes, or inventory scans. Median CPU
remained zero, mean CPU decreased, median working set decreased by 1.2539 MiB, private memory was
effectively unchanged, and two fewer handles were observed. The active case retained the same
process, foreground, and visible-window scan counts, the same 12 accepted window-created events,
the same six successful changes, and clean restoration. Its CPU sample distribution was higher in
this run while reconciliation passes and wake frequency were lower; working set and private memory
were lower, and latency stayed within 1.8629 ms at the median while improving by 5.6875 ms at P95.
Because Memory Priority was disabled and the controlled activity counts did not increase, these
small same-host differences are treated as run-to-run system variance rather than a controller
cost or gain.

Artifacts:

- `benchmark/results/architecture-phase-5-gpu-priority-20260811.json`
- `benchmark/results/architecture-phase-5-memory-priority-20260811.json`

## Exit gate

| Memory Priority gate | State | Evidence |
| --- | --- | --- |
| Static, Workload Engine, and Process List producers share one controller | Pass | Owner-tagged policy routes and runtime command route |
| Static overlap precedence is explicit without suppressing other Workload claims | Pass | Multi-owner arbitration and replacement tests |
| Exact process identity rejects PID reuse | Pass | Creation-bound targets and identity test |
| Raw unknown values are not collapsed and remain restorable | Pass | Raw conversion and controller baseline design |
| External state is never overwritten during release | Pass | Expected-state guard and journal relinquishment test |
| No duplicate live writer or restoration authority remains | Pass | Ownership script and source-zero assertions |
| GPUI cannot be blocked by Memory Priority mutation | Pass | Shared background result-wait route |
| Full, diagnostics, live, and benchmark gates | Pass | 551 tests in each matrix, two explicit live tests, and valid same-host release artifact |

Rollback must revert this entire property slice: controller, both automatic owner routes, Process
List command variant, recovery forget helper, and ownership assertions. Restoring only one legacy
producer would recreate conflicting baselines and duplicate restoration authority.
