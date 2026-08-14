# Agent Memory

## Tool Preferences

- Use the fff MCP tools for file search when available.
- Use the rtk tool for shell commands when available.
- Use Microsoft coreutils for Unix-style command-line utilities where applicable.

## Repo Shape

- Rust/GPUI Windows desktop app for power/process automation.
- GPUI composition and construction: `src/ui/app.rs`; operational `WinderustApp` method groups:
  `src/ui/app/*.rs`. Plain UI state transitions live in `shell_model.rs`, `dashboard_model.rs`,
  `process_models.rs`, and `update_model.rs`; they do not belong in `RuntimeCore`.
- Page and shell renderers: `src/ui/app/pages/`; reusable UI helpers: `src/ui/app/shared/`.
- Background worker and status fan-out: `src/backend/automation.rs`.
- Process add/check helpers: `src/ui/app/shared/process_policies.rs` (`can_add_*`,
  `new_*_rule`, and `new_process_exclusion_rule`).
- Prefer existing helpers over new abstractions.

## Current Decisions

- UI wording is the naming source of truth. Current feature names include Adaptive Engine, Background Efficiency, Memory Trim, By Foreground, By Running App, By CPU Load, By Activity, By Time, Core Limiter, CPU Sets (Soft), Processor Affinity (Hard), and Dynamic Priority Boost.
- Use native mechanism names only at Windows boundaries: EcoQoS, affinity masks, CPU Sets, and exact Win32 function names remain technical terms.
- Winderust is public pre-release software under GPL-3.0-only, Copyright (C) 2026 Tatsh Siow. Settings use only the current schema; do not add serde aliases, migration code, old brand paths, or compatibility-only fallbacks.
- Keep personal tooling local-only: .codex/, .agents/skills/, and graphify-out/ must remain ignored and excluded from release artifacts.
- Settings live beside the executable. Action Log entries stay in memory and
  export to a user-selected CSV path. Do not add an AppData fallback or
  migration unless the user explicitly requests it.
- Update checks support Stable and Pre-release channels. Automatic checks are optional; manual checks remain available on About.
- `SettingsEditor` is the application settings boundary. It privately owns draft/revision state,
  save/cancel/import/export/auto-patch merge, runtime publication, and persisted startup intent.
  Win32 Priority Separation and Advanced Power Plan Tuning use typed application services; they
  are explicit persistent operations, not temporary managed state or crash-recovery owners.
- Runtime feature status is one semantically stable `Arc<RuntimeFeatureStatus>` segment. Process
  catalog/list, dashboard history, update state, and shell navigation remain plain UI read models.
  Process queries, icons, monitors, GPUI deadlines, tray, dialogs, focus, and motion remain at the
  `WinderustApp` composition boundary.
- Power-plan selections belong to the page or rule that exposes them. By Activity owns Idle/Active plans; other automation rules own `power_plan_guid`. There is no global `Settings::power_plans` fallback.
- The global pause for power-plan switching on A/C belongs on the Power Plan Control landing page, not Winderust Behaviour.
- Managed adaptive-plan recovery recognizes only the current `Winderust Adaptive` name and description.
- `PowerPlanController` owns all automatic active-plan and temporary Adaptive-plan lifecycle state.
  `src/power/powercfg.rs` owns typed plan/domain semantics, while raw GUID, power-scheme,
  processor-setting, and effective-power-mode APIs live only in
  `src/platform/windows/power_plan.rs`. Persistent Advanced Power Plan Tuning remains a separate
  explicit application service and never enters the automatic recovery journal.
- Cross-session process control is owned by Winderust Behaviour and defaults on. Disabling it restores same-session-only acquisition/targeting; it never blocks restoration of exact state Winderust already owns. Windows access checks and existing protected-process safeguards always remain active for new mutations.
- Repeated process failure suppression uses `ExecutionFailureTracker` in `src/rules/execution_failure.rs`; the threshold comes from `settings.advanced.execution_failure_suppression_threshold`.
- Auto-exclusion fallback is shared through `PendingAutoExclusions` in `src/backend/automation.rs`.
- On newly suppressed process failures, features emit `auto_excluded_processes`; `WinderustApp::apply_pending_auto_exclusions` persists them into each feature's existing exclusion/rule list.
- Rule-only fallbacks use disabled rules: CPU Sets (Soft), Processor Affinity (Hard), Core Limiter, App Suspension.
- App Suspension rejects Session 0, LocalSystem, LocalService, and NetworkService processes plus
  curated Windows shell/shared-host processes. Process List and the App Suspension picker keep
  unavailable targets visible, labeled, and disabled; grouped Process List actions cover every
  captured process in the group. Other process controls are unaffected.
- App Suspension is a complete typed cutover through `src/control/suspension.rs`. Automatic rules,
  App-page Freeze, and Process List Suspend/Resume share the RuntimeCore controller; feature code
  owns grace/wake/reporting policy only. The controller retains failed thaw/finalization state for
  bounded retry and explicit shutdown. The crash helper opens the exact named job before freeze and
  thaws that retained job even if the recorded root exits while inherited children remain. Raw Job
  Object creation, assignment, membership, freeze/thaw, and the shared undocumented layout live in
  `src/platform/windows/suspension.rs`.
- CPU allocation has one runtime coordinator and deterministic precedence: CPU Sets (Soft) >
  Processor Affinity (Hard) > Core Limiter > Adaptive Engine / Workload Engine. Feature modules own
  policy only; the coordinator alone owns affinity/CPU Set baselines, mutation, compensation,
  arbitration, and restoration. A higher-owner release queues the exact process key; `RuntimeCore`
  reconciles it once after every CPU producer has processed that worker pass. Shutdown bypasses
  this handoff and directly restores all coordinator-owned state in reverse application order.
- Background Efficiency uses the same explicit Foreground Detection and Visible Window Detection
  layers as Priority Control. Foreground Detection defaults on, Visible Window Detection defaults
  off, and each layer owns an Enabled/Disabled Efficiency Mode default.
- Background Efficiency and Core Limiter custom rules use Focus, Visible Window, and Background
  columns with Default/Enabled/Disabled values and Focus > Visible Window > Background precedence.
  Default inherits the page-wide foreground/visible protection behavior.
- Every Priority Control page uses three ordered default tiers: Focus App, then apps with visible windows, then background. Visible Window Detection defaults off and has its own selectable value; custom process rules independently override all three tiers. Auto remains loadable for existing pre-release settings but is not offered in custom-rule selectors.
- Adaptive Engine uses the same Focus App, Visible Window, then Background ordering across Process, Thread, I/O, GPU, and Memory Priority plus Dynamic Priority Boost. Its Background Efficiency controls own separate foreground and visible-window detection and Efficiency Mode values instead of borrowing the Background Efficiency page's settings.
- Exclusion-list features append `ProcessExclusionRule`.
- Timer Resolution does not use process failure suppression.
- `src/control/timer_resolution.rs` is the sole Timer Resolution lifecycle owner;
  `src/platform/windows/timer_resolution.rs` is the sole raw WinMM adapter. The feature manager
  owns foreground-rule policy and reporting only; every successful begin is paired with the exact
  end period on replacement, disable, or shutdown. This process-lifetime state has no crash
  journal.
- `src/backend/self_power.rs` owns Winderust's composed hidden/Adaptive priority and Power
  Throttling lifecycle, including strict baseline capture, verification, compensation, retry, and
  clean shutdown. Raw current-process query/set calls live only in
  `src/platform/windows/self_power.rs`. This process-lifetime state does not use crash recovery.
- Runtime restoration is a product safety barrier: every reversible runtime
  change owned by Winderust must capture its pre-Winderust value and restore it
  in reverse application order. If the original state cannot be captured,
  Winderust must not make that reversible change.
- The barrier covers automation managers, Process List quick actions, and
  automatic power-plan switches. Clean shutdown restores through feature
  ownership; crash or forced-termination recovery is handed to Winderust's
  external watchdog before each mutation. Process termination, memory trimming,
  watchdog termination, Windows shutdown, and power loss remain outside this
  guarantee.
- Process Priority, Power Throttling/Efficiency Mode, Dynamic Priority Boost,
  Thread Priority, I/O Priority, GPU Priority, and Memory Priority are complete
  typed process-control cutovers. Static Priority Control, Background
  Efficiency, Adaptive Engine/Workload Engine policies, and Process List
  one-shot actions share their `RuntimeCore` controllers; feature code owns
  policy only, and the crash helper remains the independent recovery mirror.
  Dynamic Priority Boost's raw live query/set pair is isolated in
  `src/platform/windows/dynamic_priority_boost.rs`; its controller still owns the full recovery
  transaction and restoration chain.
  Process Priority and process Power Throttling raw class constants, state conversion, and live
  query/set calls are isolated in `src/platform/windows/priority_efficiency.rs`; their compound
  controller still owns arbitration, recovery transactions, compensation, and restoration.
  Memory Priority's raw class constants and live query/set pair are isolated in
  `src/platform/windows/memory_priority.rs`; unknown raw baselines remain controller-owned and
  exactly restorable.
  GPU Priority's raw D3DKMT query/set pair and NTSTATUS classification are isolated in
  `src/platform/windows/gpu_priority.rs`; temporary missing-context handling remains unchanged.
  I/O Priority's undocumented NT declarations, numeric information class, query/set pair, and
  NTSTATUS classification are isolated in `src/platform/windows/io_priority.rs`.
  Thread Priority's Toolhelp enumeration, thread open/identity reads, priority constants, and live
  query/set calls are isolated in `src/platform/windows/thread_priority.rs`.
  Thread Priority identity includes the exact process instance, thread ID, and
  thread creation time. Do not restore feature-owned setters, Process List
  restore closures, or duplicate Workload Engine setters for these properties.
  GPU Priority treats a temporarily unavailable GPU scheduling context as
  pending and retries without auto-excluding the process. Workload Engine keeps
  Process Priority independent when Power Throttling is unavailable and
  remembers that unavailable control for the exact process instance so it does
  not retry-spam.
- Memory Priority has two simultaneous automatic owners rather than an Adaptive replacement policy: static Memory Priority explicitly outranks an overlapping Workload Engine claim, while non-overlapping Workload claims remain effective. Both owners retain one shared exact-process baseline and restoration chain.
- CPU Sets, Processor Affinity, Core Limiter, and Workload Engine CPU allocation are a complete
  typed family cutover through `src/control/cpu_allocation.rs`. Do not restore feature-owned raw
  setters, property baselines, recovery calls, or affinity-owning `Drop` paths. Exact identity,
  mutual exclusion, actual-owner Action Log attribution, and clean/crash restoration are part of
  the boundary. Raw affinity, CPU Set, and packed topology-buffer calls live only in
  `src/platform/windows/cpu_allocation.rs`.
- CPU Sets (Soft) and Processor Affinity (Hard) share one CPU-selection preset catalog. The
  topology-derived Core Presets are read-only: All, P-cores, E-cores, All cores no SMT, P-cores no
  SMT, and E-cores no SMT. Custom presets remain editable. Every rule independently selects Focus,
  Visible Window, and Background masks with Focus > Visible Window > Background precedence.
  Selecting a preset copies its current mask into that tier; later preset edits or deletion do not
  silently rewrite configured rules.
- Memory Trim and Stop Process / Stop Process Tree are typed, result-bearing runtime commands.
  `src/control/memory_trim.rs` and `src/control/process_termination.rs` own command semantics;
  their raw calls live only in the matching `src/platform/windows/` adapters. Both revalidate exact
  process identity, protection, access, and the current cross-session setting on the runtime
  worker. They are irreversible and must never acquire a baseline, recovery entry, managed claim,
  or shutdown restore path. Stop Tree retains exact root identities across its confirmation prompt
  and rejects reused roots or known-stale numeric parent links before submitting the command.
- `src/control/process.rs` owns typed process targets, stable identity keys, and safety validation;
  `src/platform/windows/process.rs` alone translates operation-specific minimal access into
  `OpenProcess` and preserves App Suspension's synchronize-first fallback. Platform acquisition
  does not import feature, foreground, rule, UI, or controller policy. This is the mutation/command
  acquisition boundary: feature-specific CPU-time and age observations may use query-only handles,
  but they never authorize a mutation; controllers always reopen and revalidate selected targets.

## User Constraints

- Do not cut animation/motion unless explicitly asked.
- Lucide/icondata note: using specific lucide icons should not be removed just because `icondata_core`/`icondata_lu` look broad; verify icon references before trimming.
- Do not restore removed legacy identifiers or files unless the user explicitly asks for compatibility work.
