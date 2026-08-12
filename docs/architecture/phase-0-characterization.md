# Phase 0 characterization and readiness

- Status: **complete; Phase 1 may begin**
- Date: 2026-08-10
- Refactor contract: [architecture-refactor-plan.md](../architecture-refactor-plan.md)
- Resource inventory: [phase-0-resource-ownership.md](phase-0-resource-ownership.md)

Phase 0 records the current implementation before any production route moves. The added Rust code is limited to tests and the opt-in `architecture-diagnostics` build feature; a normal release does not compile the diagnostic counters or output path.

## Locked behavior

### Shared-property precedence

The current rules are mechanism-specific:

1. Background Efficiency runs before Workload Engine. Workload Engine excludes Background Efficiency's throttled PIDs, so Background Efficiency owns their overlap.
2. Regular Process Priority excludes both Workload Engine-managed and Background Efficiency-throttled PIDs.
3. Adaptive/Workload settings replace the static Thread Priority, Dynamic Priority Boost, I/O Priority, and GPU Priority settings while Workload Engine priority assistance is active.
4. Static Memory Priority runs after Workload Engine Memory Priority and can replace it for an overlap. This execution-order result is characterized, not endorsed as the eventual arbitration rule.
5. The Phase 0 baseline had CPU Sets (Soft) remove a same-path Processor Affinity (Hard) rule, while both explicit pages excluded their active usable paths from Workload Engine allocation. Phase 6 preserves the product result and adds controller-level arbitration.
6. The Phase 0 Core Limiter/Workload Engine overlap was execution-order-dependent. Phase 6 resolves it as CPU Sets (Soft) > Processor Affinity (Hard) > Core Limiter > Adaptive Engine in one coordinator.
7. Process List priority actions are one-shot operations that later automation can supersede. Process List CPU controls edit persistent rules; there is no one-shot CPU allocation mutation.

The tests in `src/backend/automation/tests.rs`, `src/config/settings.rs`, and `src/ui/app.rs` lock the current exclusion composition, CPU Sets-over-affinity rule, effective Adaptive settings, source execution order, exact-path custom-rule precedence, and Process List restore-key behavior.

### Automation order and invalidation

The normal worker order is:

```text
Background Efficiency
→ Workload Engine
→ I/O Priority
→ Process Priority
→ Thread Priority
→ Dynamic Priority Boost
→ GPU Priority
→ Memory Priority
→ App Suspension
→ CPU Sets (Soft)
→ Processor Affinity (Hard)
→ Core Limiter
→ By Running App
→ Memory Trim
→ Timer Resolution
```

Clean shutdown invokes the reversible subset in reverse order, then restores the Adaptive plan and the original automatic power plan. Memory Trim is deliberately absent because it is irreversible.

The source-scoped characterization tests lock these current deadline effects without introducing a second scheduler model:

| Trigger | Deadlines made immediately due |
| --- | --- |
| Settings changed | Every power, feature, appearance, suspension-release, and controller-poll deadline |
| Foreground or session changed | Power decision; Background Efficiency; CPU controls; Workload Engine; all priority controls; Memory Trim; Timer Resolution; App Suspension foreground release |
| Window created or session changed | Process-appearance scan and App Suspension |
| Power or session changed | Active-plan refresh and power decision |
| Input activity | Power decision |
| New process detected | Background Efficiency; CPU controls; By Running App; Workload Engine; priority controls; Memory Trim |
| App switch or shell click | App Suspension user-intent release and Timer Resolution refresh |

Existing tests also prove that the first process snapshot is only a baseline, new PIDs wake process work, exits alone do not, and a default configuration creates no polling worker.

### Visible and hidden power routes

Both modes use the same `decide` policy and journal a power-plan change before calling the Windows setter, but their boundaries are not equivalent today:

- The visible UI route passes the discovered foreground path directly. The hidden route rejects a foreground process unless Windows reports it as non-critical.
- Both routes have a short same-target retry interval. Only the hidden route adds repeated-failure suppression.
- The hidden runner retains and explicitly restores the first pre-automation plan. The visible route has no UI-owned original-plan field and relies on the recovery helper to unwind its remaining committed plan chain when the pipe closes.

These differences are now source-characterized. Phase 3 must choose and test one safety/ownership rule before the visible and hidden routes are unified.

### Settings, auto-exclusions, and failure suppression

- Settings TOML round-trips all custom foreground/background values for Process, Thread, Dynamic Priority Boost, I/O, GPU, and Memory Priority and preserves their post-deserialization override semantics.
- Disabled duplicate custom rules are skipped, the first enabled exact-path match wins, and an enabled rule whose value is `Default` remains an explicit no-override/exclusion result.
- Process Priority, Thread Priority, and Dynamic Priority Boost now have manager-level suppression characterization matching the existing mechanism families: repeated normalized-path failures emit one auto-exclusion and clearing the failure permits another attempt.
- Runtime auto-exclusion currently mutates the live UI draft and calls the ordinary full-settings save. Therefore an unrelated unsaved draft edit is persisted with the safety exclusion. The test deliberately locks this current behavior; Phase 1's revisioned typed patch is responsible for replacing it.

## Recovery evidence

The recovery protocol tests now cover:

- every `ProcessValue` variant's compaction and return-to-baseline behavior;
- thread and power-plan chain compaction;
- `Begin`, rejected `Begin`, `Cancel`, `Commit`, replacement, return-to-baseline, Job Object handle retention, `ForgetJob`, and protocol acknowledgements;
- a real disposable process priority mutation followed by `recover_journal` and verified restoration;
- a real disposable child-thread priority mutation followed by verified restoration;
- a real named Job Object containing a disposable child, freeze, recovery thaw, and cleanup;
- a real temporary Winderust Adaptive power plan, active-plan recovery, and cleanup.

Both opt-in integration tests passed on 2026-08-10. The power test left the original plan active with no temporary plan present, and the Job Object test left no disposable child. The shared protocol and reducer tests are paired with these live Windows tests rather than being treated as equivalent evidence on their own.

Property-local fault injection is intentionally attached to each new typed controller before that controller's complete route switch. Building six-point injection adapters around every legacy setter in Phase 0 would create production-shaped test plumbing for code that the cutover deletes. The route-switch proof therefore requires every replacement controller to cover `Begin`, apply, verify, `Commit`, replacement, and clean release while it is still unreachable from production producers.

## Release baseline

Artifact: [architecture-baseline-20260810.json](../../benchmark/results/architecture-baseline-20260810.json)

Runner: `scripts/architecture_baseline.ps1`

Method: feature-gated release build, isolated portable settings, 30-second cases sampled every 500 ms, summed main/helper footprint, five fresh hidden `System32\PING.EXE` appearances, one retained target for clean-release verification, and visible-window observation enabled in the active case. The runner refuses a pre-existing `ping.exe`, requires the exact action count, rejects any Process Priority failure counter, and cleans its temporary directory.

| Metric | UI idle | Process Priority reconciliation |
| --- | ---: | ---: |
| CPU median / P95 (% total capacity) | 0.0000 / 0.2553 | 0.0000 / 0.5003 |
| Working set median | 84.1094 MiB | 82.6914 MiB |
| Private memory median | 125.8242 MiB | 124.9453 MiB |
| Thread count median | 38 | 39 |
| Handle count median | 753 | 752 |
| Worker wake frequency | 0.0000 Hz | 0.8539 Hz |
| Reconciliation passes | 0 | 35 |
| Process snapshot scans | 0 | 40 |
| Visible-window scans | 0 | 17 |

Process-appearance-to-priority latency was 79.8404 ms median and 115.5333 ms P95. All five trials and the retained target were observed, all six expected changes were applied, no Process Priority failure was recorded, and graceful Winderust shutdown restored the retained target to Normal priority.

This artifact is the comparison baseline for the budgets in the refactor plan. It is machine-specific, not a general product-performance claim.

## Exit-gate ledger

| Phase 0 gate | State | Evidence / blocker |
| --- | --- | --- |
| Every current producer is in the ownership matrix | Pass | Reviewed resource manifest plus executable raw-writer gate |
| Every temporary property has a cutover manifest and zero-writer criteria | Pass | Resource manifest and `scripts/check_architecture_ownership.ps1` |
| Shared-property precedence is deterministic and locked | Pass | Direct setting/helper tests plus source-scoped execution/exclusion tests; Memory Priority and CPU allocation overlap rules are now explicit in their typed controllers |
| Live or isolated Windows integration covers every recovery class | Pass | Process, thread, suspended Job Object, and power-plan recovery tests passed |
| Shared recovery protocol and every recovery class are exercised | Pass | Protocol fault/replacement/clean-release tests plus live process, thread, suspended Job Object, and power-plan restoration |
| Per-property `Begin`/apply/verify/`Commit`/replacement/clean-release matrix is assigned | Pass as a cutover gate | Each new typed controller must pass this matrix while unreachable, immediately before its complete mechanism route switch; legacy-only injection plumbing is prohibited |
| Release baseline artifact exists and validates | Pass | `benchmark/results/architecture-baseline-20260810.json`, `validation.passed = true` |
| Production behavior is unchanged | Pass | Normal builds omit diagnostics; other changes are tests, scripts, and documentation |

All Phase 0 exit gates pass. Phase 1 may start; no mechanism route may switch until its property-specific fault matrix and the other route-switch proofs in the refactor contract pass.
