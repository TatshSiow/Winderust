# Architecture

Winderust is a mechanism-centered modular monolith: one Iced daemon process, one optional
automation worker, and one typed owner for each Windows mechanism. UI and feature code express
intent; only controllers and their narrow Windows adapters own live mutations.

```text
Windows events / UI intent / persisted settings
                    |
                    v
              WinderustApp
          /         |          \
 SettingsEditor  read models  typed commands
                                |
                                v
                          RuntimeHandle
                                |
                     wake state + scheduler
                                |
                                v
                           RuntimeCore
                  /             |             \
         observations      feature policy    replies
                                |
                                v
                    typed mechanism controllers
                       /                    \
              Windows adapters          RecoveryClient
                                              |
                                              v
                                   independent watchdog
```

## Responsibilities

- `WinderustApp` owns Iced composition, dialogs, navigation, process-list presentation, and local
  read models.
- `SettingsEditor` is the only settings draft, revision, persistence, import, and export boundary.
- `RuntimeHandle` owns worker lifecycle, event sources, typed commands, and published status.
- `RuntimeCore` owns reconciliation order, shared observations, controller composition, and
  shutdown.
- Feature managers own rules, target selection, timers, hysteresis, suppression, status, and
  Action Log policy.
- Typed controllers own exact identities, claims, baselines, precedence, verification,
  compensation, and restoration.
- `src/platform/windows/` owns raw Win32, NT, and WDK calls, handles, buffers, and native state
  conversion.
- `RecoveryClient` and the watchdog independently replay externally persistent temporary state
  after an abnormal exit.

The Iced daemon outlives its window. Closing to the tray destroys the GUI window and releases
its renderer and compositor while retaining navigation, settings drafts, and background owners.
Reopening creates a window with the previous geometry, relocating a wholly off-screen window
to a connected monitor before restoring maximization; widget-local focus and scroll caches
are recreated. Start minimized can run without creating a GUI window. The tray owns a separate
hidden native top-level window and message thread, so notifications, menu actions, Explorer
restart recovery, and duplicate-launch restore requests work without a GUI HWND. Only explicit
application exit shuts down automation and performs managed-state restoration.

Process enumeration and presentation remain a read-side query. A query result can suggest a
target, but every mutation reopens and revalidates the exact process or thread identity.

## State classes

- Temporary process and thread state belongs to a typed controller with baseline capture, a
  watchdog journal, verification, and reverse clean release.
- Automatic power-plan state belongs to `PowerPlanController`, including the first baseline,
  verified switching, managed-plan cleanup, and watchdog recovery.
- Process-lifetime state belongs to Timer Resolution or the Winderust self-power controller. It
  has explicit clean release, with process teardown as the forced-exit boundary.
- Persistent configuration belongs to `SettingsEditor` or a typed application service. It uses
  explicit user intent and readback without temporary claims or recovery entries.
- Irreversible commands use typed command controllers with complete safety preflight and
  per-target results, without a fictional baseline.

## Safety invariants

- A reversible mutation requires a captured pre-Winderust value bound to the validated identity.
- Process identity uses PID, creation time, and absolute executable path; thread identity also
  includes thread ID and creation time.
- Cross-session settings govern new acquisition, never release of state Winderust already owns.
- Critical, protected, inaccessible, or unverifiable targets fail closed.
- Temporary externally persistent writes follow Begin, apply, verify, then Commit. Clean release
  runs in reverse application order; the watchdog independently replays committed state after a
  crash or forced termination.
- External changes break Winderust ownership instead of being overwritten during restoration.
- Process List reversible actions use the same RuntimeCore controllers as automatic policy and
  may be superseded by a later automatic reconciliation.
- CPU allocation has one coordinator with this precedence: CPU Sets (Soft), Processor Affinity
  (Hard), then Adaptive Engine.
- App Suspension and CPU Limiter share one Job Object suspension controller. Their independent
  claims combine into one effective frozen state, so releasing either feature cannot thaw the
  other feature's claim.
- CPU Limiter keeps that Job Object path primary. Only an incompatible existing-job result selects
  its private exact-thread fallback; App Suspension remains Job-only. The fallback owns one
  suspend-count increment per exact thread and uses the same limiter worker and schedule.
- While a valid CPU Limiter rule is active, target refresh and process appearance discovery remain
  at one second even when Winderust is hidden or Adaptive Engine saver cadence is active.
- Process Priority and Power Throttling share one compound controller so Efficiency Mode cannot
  leave a half-applied state.

## Structural enforcement

`scripts/check_architecture_ownership.ps1` is the executable ownership boundary. It rejects raw
managed mutations outside approved Windows adapters and recovery mirrors, feature- or UI-owned
restoration state, duplicate runtime owners, and persistent or irreversible operations routed
through temporary recovery.

Architecture changes must keep the ownership scan, formatting, strict Clippy, locked tests,
legacy naming scan, and Graphify index green. Migration history and completed phase evidence
remain available in Git history rather than in the active documentation set.

## Source map

- `src/main.rs`: app entry, single-instance guard, and Iced startup.
- `src/ui/app.rs`: `WinderustApp` composition, message dispatch, subscriptions,
  native window/tray lifecycle, settings publication, and teardown. Runtime work remains
  in typed commands and services; UI drafts do not belong in `RuntimeCore`.
- `src/ui/`: page editors and views; `widgets.rs` supplies shared controls, `status_rail.rs` presents runtime status, and `navigation.rs` maps pages
  to bundled icons. `process_list.rs` owns table sorting, grouping, and pending actions.
- `src/ui/process_rules.rs`: shared process eligibility and rule-construction helpers.
- `src/ui.rs`: page enum, section grouping, labels, and small UI-independent helpers.
- `src/config/settings.rs`: persisted settings structs and defaults;
  `src/config/settings/validation.rs` owns shared persistence preflight and UI limits.
- `src/config/storage.rs`: config path, TOML load/save/import/export.
- `src/application/settings.rs`: `SettingsEditor`, the sole settings draft/revision/persistence and
  startup-intent application boundary. `src/application/win32_priority_separation.rs` and
  `src/application/advanced_power_plan_tuning.rs` own their explicit persistent Windows
  operations and typed staged errors; neither is temporary managed or crash-recovery state.
- `src/backend/automation.rs`: `RuntimeHandle`, event-source lifecycle, and worker scheduling loop. Runtime feature execution and power-plan policy state live in `src/backend/automation/runner.rs`; wake/event decisions live in `wake.rs`; status fan-out lives in `status.rs`; activation predicates and refresh timing live in `requirements.rs`.
- `src/backend/self_power.rs`: process-lifetime self-power controller and retry
  watcher, independent of the automation worker and GUI lifetime.
- `src/control/power_plan.rs`: sole automatic power-plan mutation owner, including ordinary and temporary Adaptive plan baselines, verification, recovery sequencing, compensation, and release.
- `src/control/process.rs`: exact process-instance identity and shared
  process-control safety boundary. `src/control/dynamic_priority_boost.rs`,
  `src/control/thread_priority.rs`, `src/control/io_priority.rs`, and
  `src/control/gpu_priority.rs`, and `src/control/memory_priority.rs` are the
  sole live state, baseline, owner, compensation, and clean-release controllers
  for their properties. Dynamic Priority Boost's query/set calls are isolated in
  `src/platform/windows/dynamic_priority_boost.rs`; its controller still owns the
  complete recovery transaction.
  Memory Priority's raw class conversion and query/set calls are isolated in
  `src/platform/windows/memory_priority.rs`; its controller preserves unknown raw baselines and
  owns the recovery transaction.
  GPU Priority's D3DKMT calls and NTSTATUS classification are isolated in
  `src/platform/windows/gpu_priority.rs`; its controller owns policy-independent state and recovery.
  I/O Priority's NT declarations, information class, calls, and status classification are isolated
  in `src/platform/windows/io_priority.rs`; its controller preserves unknown raw baselines.
  Thread Priority's enumeration, raw thread handle operations, identity reads, and priority
  query/set calls are isolated in `src/platform/windows/thread_priority.rs`; its controller owns
  exact process/thread identities and the recovery transaction.
  `src/control/priority_efficiency.rs` jointly owns Process Priority and process
  Power Throttling so compound Efficiency Mode transitions share one rollback
  boundary. `src/platform/windows/priority_efficiency.rs` owns their raw priority-class and
  Power Throttling constants, conversion, queries, and writes. Policy remains in the matching
  feature modules. Thread Priority
  keys ownership by exact process identity plus thread ID and thread creation
  time.
- `src/control/cpu_allocation.rs`: sole live CPU Sets and process-affinity baseline, claim
  arbitration, compensation, and clean-release boundary for CPU Sets (Soft), Processor Affinity
  (Hard), and Adaptive Engine CPU allocation. Its precedence is CPU Sets (Soft) > Processor
  Affinity (Hard) > Adaptive Engine.
  `src/platform/windows/cpu_allocation.rs` owns the raw affinity/CPU Set query and write calls plus
  packed system CPU Set topology conversion.
- `src/cpu_topology.rs`: validated, canonical read-only processor-group, core,
  efficiency, NUMA, and cache facts with checked group-0 selection projection.
  Saved selections retain group-qualified members and an exact validated topology signature.
  `src/platform/windows/cpu_allocation/topology.rs` owns native topology capture
  and packed-record validation; topology discovery grants no mutation authority.
- `src/control/memory_trim.rs` and `src/control/process_termination.rs`: sole
  irreversible working-set trim and process-termination adapters. They use the
  bounded runtime command queue, exact process-instance validation, current
  cross-session policy, and typed completion results; they intentionally have
  no recovery or restoration ownership. Stop Tree keeps its captured exact
  roots through confirmation, aborts if a root instance changes, and accepts a
  numeric parent edge only when both creation times are known and the child is
  not older than the parent.
- `src/control/timer_resolution.rs`: sole WinMM capability/query and process-lifetime request
  boundary. It owns the active period, exact begin/end pairing, switching, explicit shutdown, and
  Drop backstop; foreground matching and Action Log policy stay in the Timer Resolution feature.
- `src/control/suspension.rs`: shared App Suspension and CPU Limiter process/job handle,
  freeze/thaw transaction, retry, owner-phase arbitration, and clean-release boundary. Automatic rules,
  App-page Suspend, and Process List Suspend/Resume share its typed RuntimeCore
  routes. Pending compensation and journal cleanup reconcile on later feature
  passes; feature records retain exact creation time and are pruned when the
  controller no longer owns a frozen exact instance. The feature manager owns
  rule/grace/wake/reporting policy only. CPU Limiter policy is immediate rule matching, while
  `src/control/cpu_limiter.rs` owns its single duty-cycle worker. `src/platform/windows/job.rs`
  owns shared Job Object creation, assignment, and membership; `src/platform/windows/suspension.rs`
  owns freeze/thaw and the compatibility-sensitive layout; `src/platform/windows/cpu_limiter.rs`
  owns the high-resolution waitable timer and command event.
- `src/backend/self_power.rs`: instance-owned composition, verification, compensation, retry, and
  restoration of Winderust's hidden-to-tray process priority and Power Throttling state.
  `src/platform/windows/self_power.rs` is the sole raw current-process query/set adapter; this
  process-lifetime state intentionally has no crash-recovery journal.
- `src/backend/file_dialog.rs`: native settings and Action Log file dialogs.
- `src/backend/update_checker.rs`: GitHub release checks and Stable/Pre-release filtering.
- `src/rules/decision_engine.rs`: power-plan decision priority.
- Feature backends use the UI names. CPU Sets (Soft) and Processor Affinity
  (Hard) retain separate settings, pages, rules, status, and Action Log labels;
  `src/features/cpu_control/cpu_allocation.rs` owns their discovery, topology,
  tier selection, and reporting policy, while the typed controller owns the shared
  Windows mechanism and restoration state. CPU Limiter remains separate from CPU allocation and
  uses shared Job Object freeze/thaw duty cycling. Adaptive Engine retains pressure, selection, and tuning only.
  Adaptive Engine process sampling and identity helpers live in
  `adaptive_engine_process/process_control.rs`; its Process Priority, Power Throttling,
  and Memory Priority mutations route through typed controllers. Pure workload
  decisions and core-selection calculations live in `adaptive_engine_process/policy.rs`;
  stateful policy lifecycle remains in `adaptive_engine_process.rs`.

- `src/foreground/`: foreground tracking and read-only process enumeration.
- `src/activity/`: idle detection and input hooks.
- `src/backend/tray.rs`: native tray behavior.
- `src/ui/win32_priority_separation.rs` and `src/backend/win_registry.rs`: priority-separation
  page/value helpers and registry access, coordinated by the typed application service.

## Navigation

Pages are grouped in `src/ui.rs`:

- Home: dashboard.
- Process List: process table and per-process policy surface.
- Winderust Features: Adaptive Engine, Background Efficiency, Memory Trim, Watchdog.
- Power Plan Control: By Focus, By Running App, By CPU Load, By Activity, By Time, Advanced Power Plan Tuning.
- Priority Control: Process Priority, Thread Priority, Dynamic Priority Boost,
  IO Priority, GPU Priority, Memory Priority.
- CPU Control: CPU Limiter, CPU Sets (Soft), Processor Affinity (Hard).
- Action Log.
- Settings: Winderust Behaviour, Language and Appearance, Experimental
  Features.
- About.
- Advanced: App Suspension, Timer Resolution, Win32 Priority Separation.

Keep navigation changes in `Page`, `PAGE_SECTIONS`, labels, locale files, and
`WinderustApp::page_view` in `src/ui/app.rs` together.
