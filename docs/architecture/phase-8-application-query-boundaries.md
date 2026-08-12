# Phase 8: Application and Query Boundaries

Status: Complete (2026-08-12)

## Application composition boundary

`WinderustApp` remains the single GPUI entity and composition root. It owns GPUI entities,
subscriptions, focus handles, render dispatch, dialogs, tray integration, visual motion, sampling
monitors, and refresh deadlines. It no longer owns automation policy, managed Windows state,
persistent-setting writes, or parallel copies of cohesive page models.

The application boundary is now split as follows:

```text
GPUI WinderustApp
  |-- SettingsEditor ---------------- settings draft/revision/save/import/export
  |     `-- startup registration ---- typed persistent application operation
  |-- ShellModel -------------------- page, bounded back/forward history, breadcrumb transition
  |-- DashboardModel ---------------- metric snapshots and bounded graph history
  |-- ProcessCatalogModel ----------- candidate discovery, selected paths, icon cache
  |-- ProcessListModel -------------- rows, resource deltas, sorting/grouping/details
  |-- UpdateModel ------------------- check/result/modal state transitions
  |-- Arc<RuntimeFeatureStatus> ------ independently published runtime read-model segment
  |-- persistent application services
  |     |-- Win32PrioritySeparationService
  |     `-- AdvancedPowerPlanTuningService
  |-- UI-only operational owners ---- GPUI timers, monitors, queries, tray, dialogs, motion
  `-- RuntimeHandle ----------------- typed runtime commands and published automation status
```

`AppearanceModel` was deliberately not introduced. Appearance behavior is currently coupled to
GPUI theme entities, process-wide render tokens, and animation rendering; wrapping those fields in
a passive struct would only add namespacing. Tray state, dialogs, focus handles, and update-check
network execution likewise stay outside `RuntimeCore`.

## Settings and persistent operations

`SettingsEditor` owns the settings coordinator and draft as private implementation details. It is
the only application surface for save, cancel, import, export, automatic patch merge, runtime
snapshot publication, and persisted startup intent. The existing periodic `replace_settings`
publication remains in place because it also drives Winderust self-power reconciliation when the
settings revision is unchanged.

Startup registration now returns typed errors. A settings save remains committed when registry
registration fails, reports the registration failure, and retries the persisted startup intent on
the next successful settings load. A settings-load fallback does not modify registration because
the user's persisted intent is unknown in that path.

The two persistent Windows feature families are application services rather than runtime managed
state:

- `Win32PrioritySeparationService` owns current and backup registry reads, first-backup creation,
  machine writes, and restore. A backup-write failure blocks the machine write; an existing backup
  is never overwritten; machine-write failure retains the backup and reports its stage.
- `AdvancedPowerPlanTuningService` owns personality/value reads and the ten ordered A/C and battery
  writes plus active-plan reactivation. It always rereads the actual plan after an explicit apply,
  including partial failure, so the UI reflects Windows rather than assuming the draft succeeded.

Both services remain outside temporary-state recovery. Their persistent effects occur only after
an explicit settings action.

## Published and local read models

The fifteen runtime feature snapshots are published as one
`Arc<RuntimeFeatureStatus>` segment. The backend preserves the Arc allocation when semantic content
does not change; the UI swaps one pointer instead of cloning and comparing fifteen Vec-bearing
fields. The three legitimate UI-side status adjustments use copy-on-write.

Process query state remains on the UI/read side:

- `ProcessCatalogModel` owns process candidates, load state, selected executable paths, and icons.
- `ProcessListModel` owns the full process rows, resource samples/deltas, accessibility filter,
  sorting, expanded groups, selection, and details draft.
- Candidate and Process List scans have independent in-flight flags. Navigating from a picker page
  to Process List no longer makes the second query wait for an unrelated scan. A pause preserves an
  already-running scan marker until its callback returns, preventing duplicate work after a quick
  pause/resume.
- `list_processes_with_paths` remains a single independent UI query. No row, icon, grouping,
  sorting, or resource-history state moved into `RuntimeCore`.

`DashboardModel` owns only the latest CPU, memory, I/O, and network snapshots and their bounded
30-sample histories. GPUI's composition root still owns the monitors and deadlines and supplies
samples to the model, so the extraction does not create a worker, timer, or additional poll.

`UpdateModel` owns check deduplication, results, result clearing, automatic-startup-modal policy,
and the explicit visible/closing/hidden transition. WinHTTP execution and GPUI animation timers
remain operational UI concerns.

`ShellModel` owns the current page, bounded navigation history, and breadcrumb transition state.
The `WinderustApp` wrappers still perform GPUI notifications, clear page-specific drafts, and
schedule appropriate read-side refreshes.

## Structural gates

`scripts/check_architecture_ownership.ps1` now verifies:

- one `SettingsEditor`, `ShellModel`, `DashboardModel`, `ProcessCatalogModel`, `ProcessListModel`,
  `UpdateModel`, and `Arc<RuntimeFeatureStatus>` composition field;
- zero mirrored legacy fields on `WinderustApp`;
- zero UI ownership of startup registration, registry writes, or Advanced Power Plan Tuning writes;
- zero runtime/control ownership of UI process, dashboard, update, or shell models;
- one independent Process List query and no dashboard sampling inside `DashboardModel`;
- persistent application services never acquire crash-recovery ownership.

## Validation

- SettingsEditor/startup tests: 19 passed.
- Win32 Priority Separation service tests: 5 passed.
- Advanced Power Plan Tuning and processor-power tests: 8 passed.
- Process List interaction/query tests: 52 passed.
- Process, dashboard, update, and shell model tests: 10 passed.
- Full locked repository suite: 619 passed, 12 ignored, 0 failed.
- Formatting, diff checks, strict all-target Clippy, the complete architecture ownership gate, and
  the legacy-name scan pass.
- The optimized release build completed successfully and Graphify rebuilt all 227 indexed code
  files in the current graph.
