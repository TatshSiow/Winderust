# Phase 6 typed CPU allocation coordinator

- Status: **complete**
- Date: 2026-08-11
- Refactor contract: [architecture-refactor-plan.md](../architecture-refactor-plan.md)
- Previous mechanism: [phase-5-priority-efficiency.md](phase-5-priority-efficiency.md)

This slice converts CPU Sets (Soft), Processor Affinity (Hard), Core Limiter, and Workload Engine
CPU allocation as one constrained Windows-mechanism family. The four producers now share one
`CpuAllocationCoordinator` in `RuntimeCore`. Feature managers retain discovery, protection,
sampling, hysteresis, topology policy, failure suppression, status, and Action Log attribution.
The coordinator alone owns claims, arbitration, property baselines, verified expected values,
mutual exclusion, recovery intents, and clean restoration. The crash helper remains the independent
forced-termination recovery mirror. `src/platform/windows/cpu_allocation.rs` owns the narrow raw
affinity, CPU Set, and system CPU Set topology adapter.

## Ownership and precedence

```text
CPU Sets (Soft) rules ---------------- owner: CpuSetsSoft --------------\
Processor Affinity (Hard) rules ------ owner: ProcessorAffinityHard ----+
Core Limiter sustained limits -------- owner: CoreLimiter --------------+--> effective claim
Workload Engine allocation ----------- owner: AdaptiveEngine -----------/
                                                                          |
                                                                          v
                                                      CpuAllocationCoordinator
                                                   PID + creation + exact path
                                                                          |
                            restore outgoing property -> apply incoming property
                                      Begin -> write -> readback -> Commit
                                                                          |
                          platform/windows/cpu_allocation.rs raw adapter
                                                                          |
                                                                          v
                                                        crash-recovery mirror
```

The deterministic order is:

1. CPU Sets (Soft)
2. Processor Affinity (Hard)
3. Core Limiter
4. Adaptive Engine / Workload Engine

An explicit per-app rule therefore wins over automatic policy, and Core Limiter temporarily
shadows Workload Engine allocation while its sustained limit is active. A shadowed lower claim is
kept rather than deleted. Removing an effective claim queues that exact process key instead of
executing a possibly stale lower claim inside the releasing feature. After every CPU producer has
processed the same worker pass, `RuntimeCore` flushes those keys once through the coordinator. The
flush re-resolves the then-effective claim and revalidates the current cross-session policy before
transitioning directly, without bouncing through the original baseline. This pass-end barrier
also covers lower claims whose producer is in cooldown or suppressing repeated access failures.
If no claim remains, the final release restores the original baseline. CPU Sets and affinity
cannot remain simultaneously owned by Winderust for one process instance.

Process List does not have a one-shot CPU allocation mutation. Its Process Rule Details surface
edits the persistent CPU Sets (Soft) and Processor Affinity (Hard) rule sets, so Phase 6 adds no
manual runtime command.

## Preserved policy behavior

CPU Sets (Soft) and Processor Affinity (Hard) retain their separate settings, pages, foreground
and visible-window protections, exact-path rules, built-in exclusions, cross-session setting,
failure suppression, auto-exclusion publication, status, and Action Log labels. Suppressed targets
remain active for release accounting without repeating unavailable writes.

Core Limiter retains per-process CPU sampling, threshold sustain, cooldown, exact-path rules,
foreground and visible-window protection, and auto-exclusion behavior. Its `tracked` and `limited`
maps are policy hysteresis only; they contain no Windows baseline or setter. An active limit now
submits a claim even when a stronger explicit claim shadows it, leaving all overlap arbitration in
the coordinator.

Workload Engine retains its pressure latch, restore band, process sampling history, repeat-offender
acceleration, candidate scoring, maximum-target selection, minimum-restraint and cooldown timers,
topology floors, 85% saturation relaxation, load-aware processor choice, and three-second/15-point
rebalance hold. Automatic percentage mode still uses CPU Sets; manual mode still honors the chosen
soft or hard mechanism. No preset or scheduling threshold changed in this slice.

CPU topology discovery remains policy-owned. The logical mask and CPU Set mapping intentionally
cover processor group 0, and the existing multi-group disclosure remains unchanged. Core Limiter
continues selecting the lowest available logical processors from the captured baseline; Workload
Engine continues using its efficiency- and load-aware mask policy.

## Identity, transitions, and restoration

Every claim contains the observed PID, creation time, and absolute executable path. Immediately
before a mutation, the runtime worker reopens the process through the shared typed boundary and
revalidates the exact instance, name, path, session policy, critical/protected status, and required
Windows access. A reused PID cannot inherit an old claim.

Affinity and CPU Sets have separate lazy baselines under one coordinator. A mode switch resolves
the incoming target first, restores and verifies the outgoing Winderust-owned property, and then
applies the incoming property. If the incoming transition fails with a known state, the coordinator
reapplies the prior effective property. If compensation is uncertain, its managed record remains
available for conservative later release instead of silently losing restoration state.

Each write records a crash-recovery Begin intent, applies through the sole production adapter,
reads the property back, and commits only after exact verification. Apply, verification, and Commit
failures compensate to the prior value where the live state is known. If clean release reaches the
baseline but recovery Commit fails, the baseline is kept and only the property journal is
relinquished or retried.

Clean release restores a baseline only while the live value still equals Winderust's expected
value. An external affinity or CPU Sets change breaks ownership and is left untouched after the
property journal is relinquished. Process exit and PID reuse similarly discard stale recovery
ownership without targeting a replacement. Shutdown clears claims and restores both property
ledgers in reverse successful-application order.

Every producer apply and pass-end handoff reopens the target with the current cross-session
setting. The flush runs only after all CPU producers have removed claims disabled by that settings
revision, preventing a stale lower policy from being applied during a multi-feature disable. A
failed handoff releases or relinquishes the outgoing Winderust-owned state; if that cleanup also
fails, a dedicated reconciliation domain retries release only rather than reapplying the rejected
claim. Release-only retries back off exponentially from one to 60 seconds and repeat failures are
not appended to the Action Log; eventual successful restoration is still reported. A newly queued
handoff always bypasses an older release-retry deadline, so one inaccessible process cannot delay
another process's pass-end transition. Shutdown bypasses the pass-end flush, clears all claims, and
performs direct reverse restoration without creating a new lower-owner transition.

Release and reconciliation summaries carry the actual managed or effective owner. Action Log
entries are therefore attributed to the feature whose value was restored or applied, while a
cross-owner failure does not contaminate the releasing feature's status or auto-exclusion tracker.
Restoration failures remain visible and retryable.

The Windows adapter's `GetProcessDefaultCpuSets` size discovery distinguishes a genuine empty list
from a failed probe: only `ERROR_INSUFFICIENT_BUFFER` is accepted as the documented request for a
larger buffer. The same rule is used by the live adapter and crash-recovery mirror.

## Validation evidence

Deterministic controller tests cover owner precedence, pass-end shadowed-owner handoff without a
producer resubmission, removal of a shadowed claim as a no-op, CPU Sets/affinity mutual exclusion,
both mode-switch directions, known and uncertain compensation failures, external breaks for both
properties, cross-session apply denial, multi-owner disable without a stale lower write,
failed-handoff release-only retry, actual-owner restoration attribution, PID reuse, apply Commit
failure, release Commit failure, reverse shutdown, mask intersection, Core Limiter mask semantics,
and CPU Set record parsing. Runtime tests lock producer order, the post-producer flush, immediate
handoff deadline bypass, cross-process retry isolation, failed-claim fingerprint promotion, bounded
retry backoff, coordinator ownership, precedence, retry scheduling, and shutdown placement. Existing
topology, Core Limiter, Workload Engine, failure-suppression, and preset tests remain unchanged and
pass.

The ownership gate permits `SetProcessAffinityMask` and `SetProcessDefaultCpuSets` only in
`src/platform/windows/cpu_allocation.rs` and the crash-recovery mirror. It asserts exactly one
production call for each adapter, rejects raw affinity, CPU Set, topology-buffer, and unsafe calls
from the coordinator, rejects policy imports from the adapter, and proves the two explicit
features, Core Limiter, and Workload Engine retain no legacy baseline, raw setter, recovery call,
or affinity-owning `Drop` path.

Live disposable-process tests verify production affinity and CPU Sets apply/readback/clean release.
Separate crash-recovery tests verify that both values return to their captured originals. These
tests use owned `System32\\PING.EXE` instances and restore before cleanup.

The default and `architecture-diagnostics` suites each pass **570 tests**, with 11 explicit
integration tests ignored by the ordinary matrix. The production CPU-allocation controller test
was run explicitly and verified affinity plus CPU Sets apply, exact readback, mutual exclusion, and
clean restoration. Strict Clippy passes for all targets.

## Same-host release comparison

The Phase 6 release benchmark uses the same Intel Core 5 210H host, isolated portable settings,
30-second footprint windows, five Process Priority action trials, and
`architecture-diagnostics` instrumentation as Phase 5.5. The workload deliberately remains the
same so this is an architecture overhead/regression gate, not a CPU-allocation policy benchmark.

| Metric | Phase 5.5 | Phase 6 |
| --- | ---: | ---: |
| Idle CPU median / mean / P95 (% total capacity) | 0.2478 / 0.1799 / 0.5079 | 0.2468 / 0.2081 / 0.5065 |
| Idle working set / private memory median | 79.4453 / 123.1680 MiB | 80.1875 / 123.0156 MiB |
| Idle threads / handles median | 38 / 735 | 38 / 733 |
| Active CPU median / mean / P95 (% total capacity) | 0 / 0.1762 / 0.5101 | 0.2469 / 0.2165 / 0.7476 |
| Active working set / private memory median | 83.0273 / 126.1406 MiB | 83.6875 / 127.5781 MiB |
| Active threads / handles median | 39 / 732 | 39 / 732 |
| Reconciliation passes / wake frequency | 31 / 0.7573 Hz | 28 / 0.6782 Hz |
| Process / path-enrichment / foreground / visible scans | 30 / 0 / 18 / 17 | 27 / 0 / 18 / 17 |
| Process-appearance-to-action median / P95 | 68.3335 / 81.2760 ms | 66.8005 / 68.2336 ms |
| Applied actions / failures | 6 / 0 | 6 / 0 |
| Clean priority restoration | Pass | Pass |

Idle remained architecturally dormant with zero worker reconciliations, wakes, or inventory scans.
Active reconciliation passes and process scans both fell by three, wake frequency fell by 0.0791
Hz, and median/P95 action latency improved by 1.5330/13.0424 ms. CPU means rose by only 0.0282 and
0.0403 percentage points; the active P95 contains one 0.7476% scheduler-quantized sample. Median
working set rose by 0.7422 MiB idle and 0.6602 MiB active, while handles did not increase and no new
path-enrichment scan appeared. These small same-host differences are treated as run-to-run variance:
there is no polling, wake, scan, failure, or restoration regression attributable to the coordinator.

Artifacts:

- `benchmark/results/architecture-phase-5-priority-efficiency-20260811.json`
- `benchmark/results/architecture-phase-6-cpu-allocation-20260811.json`

## Exit gate

| CPU allocation gate | State | Evidence |
| --- | --- | --- |
| All four producers share one coordinator | Pass | Owner-tagged claims in one `RuntimeCore` coordinator |
| CPU Sets and affinity cannot remain incompatibly owned | Pass | Cross-property switch and mutual-exclusion tests |
| Explicit, Core Limiter, and Workload precedence is deterministic | Pass | Precedence table and pass-end reconciliation tests |
| Exact process identity rejects PID reuse | Pass | Creation-bound targets and reuse test |
| Cross-session policy is revalidated by the producer that applies a claim | Pass | Typed open boundary and captured denied-apply test |
| Multi-owner disable and shutdown cannot apply stale lower claims | Pass | Central pass-end barrier, shadowed-removal, and shutdown tests |
| External changes are not overwritten during release | Pass | Affinity and CPU Sets relinquishment tests |
| No duplicate writer or restoration authority remains | Pass | Ownership script and source-zero assertions |
| Topology and Adaptive tuning remain unchanged | Pass | Existing topology/policy suites and zero preset edits |
| Clean and crash restoration work for both properties | Pass | Fake-platform matrix and live disposable-process tests |
| Full, diagnostics, live, and benchmark gates | Pass | 570 tests in each matrix, explicit live controller test, and valid release artifact |

Rollback must revert the complete family: coordinator, all four producer routes, recovery-forget
operations, legacy-manager deletion, ownership assertions, and characterization changes. Restoring
only one legacy producer would recreate competing baselines and incompatible CPU Sets/affinity
ownership.
