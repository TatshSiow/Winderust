# Phase 1 application and runtime foundation

- Status: **complete; Phase 2 may begin**
- Date: 2026-08-10
- Refactor contract: [architecture-refactor-plan.md](../architecture-refactor-plan.md)
- Phase 0 evidence: [phase-0-characterization.md](phase-0-characterization.md)

> Historical note: this document records the Phase 1 boundary as it existed on 2026-08-10.
> Phase 8 later made `SettingsCoordinator` and `SettingsDraft` private implementation details of
> the public application boundary, `SettingsEditor`. Phase 10 completed the
> `BackgroundAutomation` to `RuntimeHandle` migration and removed the former name from source.

Phase 1 establishes explicit application ownership without moving any Windows mechanism to a new
controller. The existing worker, feature managers, scheduling intervals, precedence, target
selection, and settings schema remain in place.

## Application lifecycle

`main` now owns the top-level startup sequence:

```text
recovery-helper dispatch
→ single-instance guard
→ SettingsCoordinator load
→ RecoveryClient start
→ RuntimeHandle start
→ GPUI application
→ RecoveryClient finish
```

The GPUI quit hook and `WinderustApp` drop fallback share one idempotent shutdown path. Reversible
Process List actions release first, the runtime worker and feature managers release second, and
Winderust self-power state releases last. Process List restoration must precede runtime restoration
because a quick action can have captured an automation-managed value; the runtime then restores the
pre-Winderust baseline. Every restore is attempted and errors are aggregated.

`RuntimeHandle::shutdown` stops new commands, removes the Windows event watcher, joins the worker,
and returns worker restoration failures or panics. `Drop` remains only a best-effort fallback.

## Settings authority

`SettingsCoordinator` is the sole caller of persisted Settings load, save, import, and export APIs.
It owns:

- the persisted `Settings` value and monotonic persisted revision;
- a `SettingsDraft` with a base revision and edit revision;
- a cached, revisioned runtime projection;
- narrow `NavigationCollapsedPatch` and `AutoExclusionPatch` merge paths.

The runtime projection preserves the existing interaction contract: unsaved General and Advanced
edits are live, the global Winderust enabled state and feature sections remain persisted until Save.
Full equality replaces the previous handwritten section predicate, so Process Priority, Thread
Priority, and Dynamic Priority Boost changes now invalidate the runtime correctly.

Runtime patches are transactional at the application boundary. A patch is applied to cloned
persisted and draft values, persisted atomically when necessary, and published only after the save
succeeds. It preserves unrelated draft edits without writing them. Stale drafts, stale patches,
load, save, import, and export failures remain distinguishable typed errors.

An auto-exclusion delivery is acknowledged only by a successful coordinator merge. Failed
deliveries are merged back into the runtime queue, rebased to the current persisted revision, and
retried after a five-second backoff so a transient file error neither loses the safety patch nor
causes a hot retry loop.

## Runtime migration facade

The former `BackgroundAutomation` boundary has evolved in place into `RuntimeHandle`; no duplicate
facade or second worker was added. It accepts `RuntimeSettingsSnapshot` values containing separate
runtime and persisted revisions:

- a runtime revision change replaces settings and wakes/reconciles the worker;
- a persisted-only revision change rebases queued auto-exclusion patches without a worker wake;
- unchanged revisions reuse the same `Arc<Settings>` allocation.

The outer `RuntimeStatusSnapshot` carries a generation, one shared feature-status segment, a
separate shared Action Log segment, appearance generation, and one-shot worker error. Semantic
no-op status updates retain the existing `Arc`, avoiding full status cloning while the legacy UI
fields remain temporarily available.

Default settings still create neither a polling worker nor event watcher. Shutdown checks at the
worker and watcher creation boundaries prevent a concurrent late start after shutdown begins.
Runtime lifecycle operations share one serialization lock, while status and event callbacks retain
their existing shared-state lock. Winderust self-power changes are also semantic: a successful
Adaptive Engine state is not reapplied every tick, while failures remain retryable after a bounded
backoff.

## Recovery boundary

`RecoveryClient` wraps the existing helper process lifecycle without changing the JSON-line wire
protocol or any `record_*` mutation sequence. Closing the helper pipe remains the clean-finish
signal: the helper recovers every committed journal entry still present, so a manager or Process
List restore failure does not discard the original baseline.

The live disposable-process test now covers that exact failure path: it commits a priority change,
begins and cancels a failed clean release, proves the original committed entry remains, runs helper
recovery, and verifies the process returns to its captured priority.

## Exit-gate evidence

| Phase 1 gate | State | Evidence |
| --- | --- | --- |
| Startup and shutdown order is deterministic | Pass | Source-scoped lifecycle/order tests plus idempotent runtime and application shutdown tests |
| Failed clean restoration leaves recovery evidence | Pass | Live disposable-process failed-release/journal/recovery test; watchdog Begin/Cancel/Commit and replacement tests |
| Auto-exclusion patches preserve unrelated draft edits and serialization | Pass | Narrow patch, navigation patch, save-failure, path identity, and existing TOML round-trip tests |
| One persisted Settings writer remains | Pass | `scripts/check_architecture_ownership.ps1`; all direct Settings storage calls are in `src/application/settings.rs` |
| Runtime facade preserves behavior and invalidates every settings section | Pass | Runtime projection/revision tests, scheduling characterization, and normal plus diagnostics suites |
| Default settings create no polling worker | Pass | `runtime_handle_default_settings_start_no_worker` and Phase 0 dormancy characterization |
| Status publication avoids semantic no-op copies | Pass | status generation and `Arc` identity tests |

## Validation notes

The normal and `architecture-diagnostics` strict Clippy and test matrices pass. The live ignored
Job Object recovery test also passes. The live power-plan recovery test could not be rerun in the
current non-elevated process because `PowerDuplicateScheme` returned Windows error 5 before making
a change; its successful elevated Phase 0 result remains the recovery baseline.

Phase 1 is independently revertible: restore the former `BackgroundAutomation` name and direct UI
settings adapters, remove `src/application`, and keep every Phase 0 characterization test and
recovery barrier intact. No mechanism route has switched, so Phase 2 can extract scheduling and
per-pass observations without also changing Windows-property ownership.
