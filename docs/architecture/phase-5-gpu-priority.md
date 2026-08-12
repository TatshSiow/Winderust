# Phase 5.3 typed GPU Priority control

- Status: **complete**
- Date: 2026-08-11
- Refactor contract: [architecture-refactor-plan.md](../architecture-refactor-plan.md)
- Previous mechanism: [phase-5-io-priority.md](phase-5-io-priority.md)

This slice converts GPU Priority as one complete Windows-property boundary. Static Priority
Control, the Adaptive Engine replacement policy, and Process List one-shot actions now share one
`GpuPriorityController` in `RuntimeCore`. The feature manager owns policy, temporary GPU-context
handling, suppression, and reporting only. The controller owns exact identities, raw baselines,
expected values, effective owners, transitions, and clean release. The crash helper remains the
independent recovery mirror.

## Ownership and flow

```text
static GPU Priority policy ------------\
                                        +-> policy resolver -> owner-tagged process claim --\
Adaptive Engine replacement policy ---/                                           |
                                                                                    v
Process List -> bounded result-bearing command -----------------------------> RuntimeCore
                                                                                    |
                                                                                    v
                                                                 GpuPriorityController
                                                       PID + creation time + exact path
                                                                                    |
                                           Begin -> apply -> verify -> Commit
                                                                                    |
                                                                                    v
             one platform/windows D3DKMTSetProcessSchedulingPriorityClass adapter
                                                                                    |
                                                                                    v
                                                        crash-recovery journal mirror
```

The policy manager preserves Focus App > Visible Window > Background tiering, exact-path rules,
exclusions, priority-preservation settings, failure suppression, auto-exclusion publication,
status, and rate-limited Action Log summaries. Adaptive settings replace rather than merge with
static settings while Workload Engine GPU Priority assistance is active. The effective owner moves
from `GpuPriority` to `AdaptiveEngine` in the same reconciliation without restoring through an
intermediate baseline.

## Identity, access, and the WDK boundary

Every automatic claim carries the creation time and executable path from the shared cycle
observation. The runtime worker reopens and revalidates that exact process immediately before a
read, recovery Begin, or write. The shared safety boundary rejects Winderust itself, PID 0,
relative or changed executable paths, changed process names, critical or unverifiable processes,
protected processes, disallowed cross-session targets, and inaccessible handles.

Windows exposes the property through `D3DKMTGetProcessSchedulingPriorityClass` and
`D3DKMTSetProcessSchedulingPriorityClass`. The documented scheduling-class order is Idle (0),
Below Normal (1), Normal (2), Above Normal (3), High (4), and Realtime (5). Winderust keeps the
raw `u32` baseline so an unrecognized future value can still be restored exactly; Process List
fails closed instead of displaying an unsupported value as a known class.

Microsoft's Get and Set documentation does not formally define `STATUS_INVALID_PARAMETER` as
"no GPU context." Winderust preserves its observed product behavior by classifying that status as
a typed temporary `Unavailable` result. Automatic policy keeps retrying without recording a
permanent failure or auto-excluding the process. Other failures retain the normal suppression and
reporting behavior. This inference remains isolated in the Windows adapter and is documented in
the reference library for future Windows-version validation.
That adapter now resides in `src/platform/windows/gpu_priority.rs`; controller-owned transaction,
recovery, preservation, pending-context policy outcome, and restoration state did not move.

## Baselines, preservation, and restoration

The first successful Winderust mutation captures one baseline for the exact process instance.
Process List actions, static policy, and Adaptive replacement continue the same baseline and
expected-value chain. A process already at the requested value is not adopted. A Process List
action is immediate and non-persistent; a later applicable automatic reconciliation may supersede
it.

Preservation is evaluated against the original baseline. Focus App and Visible Window can preserve
a baseline at or above the requested priority, while Background can preserve a baseline at or
below it. If a later claim preserves a process already managed by Winderust, the controller first
releases it to the original baseline.

Clean release writes the baseline only while the live value still equals Winderust's expected
value. An external change breaks ownership and remains untouched after an acknowledged
exact-process/property journal relinquishment. Failed relinquishment keeps the managed record for
retry. Begin, apply, verification, and Commit failures compensate where possible; uncertain state
stays tracked and recoverable. If a release has already applied and verified the baseline when
Commit fails, the controller keeps that baseline and retries only journal relinquishment. Release
and shutdown use reverse successful-application order.

## Shared Process List command path

GPU Priority joins Dynamic Priority Boost, Thread Priority, and I/O Priority in the bounded,
result-bearing process-control FIFO owned by `RuntimeHandle`. Automatic GPU policy runs before
manual commands in a pass, so a click is immediate and only a later reconciliation may supersede
it. Every captured target in a stacked row is attempted, and partial failures retain the first
concrete error.

The grouped-row menu now reads the first target with an available GPU scheduling context rather
than assuming the representative PID has one. GPUI performs no GPU mutation inside an entity
update: it queues a typed command, waits on the background executor, and publishes the result on
the foreground executor. Manual-only managed state keeps the existing single runtime worker parked
without polling until another command, settings change, or shutdown.

## Validation evidence

Deterministic tests cover the six documented raw values and unknown values, NTSTATUS
classification, temporarily unavailable GPU contexts, unmanaged matching state,
Process-List-to-policy supersession, static-to-Adaptive replacement,
Focus/Visible/Background preservation, identity metadata drift, external-state breaks, failed
journal relinquishment, process exit and PID reuse, unknown raw-baseline restoration,
Begin/apply/verify/Commit faults, failed compensation, release Commit ambiguity, relinquishment
retry without reapplication, reverse shutdown, Action Log rate limiting, the cross-property FIFO,
worker replies, and shutdown ordering.

The default and `architecture-diagnostics` suites each pass **537 tests**, with seven explicit
Windows integration tests ignored by the ordinary matrix. Both new GPU tests were then run against
an isolated hidden release instance with a real GPUI/D3D scheduling context: production-controller
apply/verify/clean-release passed, and crash-recovery replay restored the captured GPU priority.
Strict Clippy passes in both configurations. Formatting, compatibility scanning, architecture
ownership assertions, diff checks, and Graphify refresh are part of the final phase gate.

The ownership script permits `D3DKMTSetProcessSchedulingPriorityClass` only in the Windows platform
adapter and crash-recovery replay. It asserts exactly one production setter call, rejects raw WDK
access from the controller, and proves the feature manager, Process List, and Workload Engine
contain no GPU setter or legacy restoration owner.

## Same-host release comparison

The Phase 5.3 release benchmark uses the same Intel Core 5 210H host, isolated portable settings,
30-second footprint windows, five Process Priority action trials, and
`architecture-diagnostics` instrumentation as Phase 5.2. GPU Priority is disabled in this general
regression benchmark, so mechanism correctness comes from the two explicit GPU integration tests;
the benchmark verifies that adding the controller and command route does not create idle work or
break the established reconciliation and restoration path.

| Metric | Phase 5.2 | Phase 5.3 |
| --- | ---: | ---: |
| Idle CPU median / mean / P95 (% total capacity) | 0 / 0.1207 / 0.2521 | 0 / 0.1109 / 0.2497 |
| Idle working set / private memory median | 85.1719 / 129.2656 MiB | 85.6133 / 127.4180 MiB |
| Idle threads / handles median | 39 / 751 | 38 / 751 |
| Active CPU median / mean / P95 (% total capacity) | 0.2467 / 0.1832 / 0.5000 | 0 / 0.1438 / 0.4931 |
| Active working set / private memory median | 82.5312 / 125.6289 MiB | 85.6523 / 127.7539 MiB |
| Active threads / handles median | 40 / 748 | 40 / 750 |
| Reconciliation passes / wake frequency | 31 / 0.7533 Hz | 37 / 0.8947 Hz |
| Process / foreground / visible-window scans | 30 / 18 / 17 | 33 / 18 / 17 |
| Process-appearance-to-action median / P95 | 69.0976 / 72.0123 ms | 63.2045 / 74.2309 ms |
| Applied actions / failures | 6 / 0 | 6 / 0 |
| Clean priority restoration | Pass | Pass |

Idle remained fully dormant with zero reconciliation passes, wakes, or inventory scans. Its CPU
mean and P95 decreased slightly, median working set rose by 0.4414 MiB, median private memory fell
by 1.8476 MiB, one fewer thread was observed, and handle count was unchanged. The active case had
the same 17 Process Priority cycles and the same foreground/visible scan counts, but six more outer
passes and three more process scans. Phase 5.3 accepted 12 window-created events versus 15 in Phase
5.2, so these small timing and footprint differences are treated as run-to-run variance rather
than a GPU-controller effect while the property is disabled. All actions completed without failure
and the retained target restored cleanly.

Artifacts:

- `benchmark/results/architecture-phase-5-io-priority-20260811.json`
- `benchmark/results/architecture-phase-5-gpu-priority-20260811.json`

## Exit gate

| GPU Priority gate | State | Evidence |
| --- | --- | --- |
| Static, Adaptive, and Process List producers share one controller | Pass | Owner resolver, runtime command route, and replacement tests |
| Exact process identity rejects PID reuse and metadata drift | Pass | Creation-bound target and stable identity tests |
| Missing GPU context remains pending without auto-exclusion | Pass | Typed status and manager deduplication tests |
| Unknown Windows values restore exactly | Pass | Raw-baseline round-trip test |
| External state is never overwritten during release | Pass | Expected-state guard and journal-relinquishment tests |
| Clean and crash restoration cross the production WDK boundary | Pass | Two isolated-release GPU integration tests |
| No duplicate live writer or restoration authority remains | Pass | Ownership script and source-zero assertions |
| GPUI cannot be blocked by GPU Priority mutation | Pass | Background result wait and shared queue lifecycle tests |

Rollback must revert this entire property slice: controller, runtime command variant, policy route,
Process List route, recovery forget helper, and ownership assertions. Restoring only one legacy
producer would recreate duplicate mutation and restoration authority.
