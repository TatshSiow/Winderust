# Winderust Development Guide

This is the working guide for code changes. Product scope and future goals live in `20-project-scope.md`.

## Project Basics

- Windows-only Rust desktop app.
- UI stack: GPUI plus `gpui-component`.
- Settings format: TOML through `serde` and `toml`.
- Localization: `rust-i18n` with files in `locales/`.
- Windows integration: direct Win32 APIs through `windows` and `windows-sys`.

Use these checks before handoff:

```powershell
git diff --check
cargo fmt -- --check
cargo clippy --locked --all-targets -- -D warnings -D unsafe-op-in-unsafe-fn
cargo test --locked
```

When reviewing or committing an existing index, also run
`git diff --cached --check`.

For release builds:

```powershell
.\scripts\build_release.cmd
```

If `target\release\winderust.exe` is locked because the app is running:

```powershell
.\scripts\build_release.cmd -TargetDir target-next
```

## Routine Chores

For dependency PRs, review each update independently. Check the changed files,
release notes, and whether the version is already required by the GPUI revisions
pinned in `Cargo.toml`; do not merge a major-version bump merely because
Dependabot opened it. Run the default checks above, confirm CI passes, then merge
or close the PR. Keep unrelated dependency updates in separate commits so a bad
bump is easy to identify and revert.

For other chores, keep the diff limited to the requested maintenance, reuse the
existing scripts and workflows, and run the same default checks before pushing.
Use `Chore(deps): ...` for dependency-only commits and a direct imperative
subject for other maintenance.

Use `main` for releasable code and `dev` as the integration branch. Create
feature branches from `dev` and merge them back through pull requests; promote
tested `dev` changes to `main` for release. CI runs on pushes to `main` and
`dev`, and on every pull request. Release automation remains tag-only.

## Rust Engineering Practices

- Trace every caller before changing a shared helper, enum, setting, or error
  contract. Fix a defect once at the shared boundary when all affected callers
  route through it.
- Keep behavior-preserving refactors mechanical. Move one coherent page or
  helper family at a time, avoid mixing feature work into the split, and verify
  the resulting tree before claiming behavior is unchanged.
- Prefer existing project helpers, the standard library, and native Windows
  facilities before adding abstractions or dependencies.
- Use `Result` and typed errors when callers need to distinguish or report a
  failure. Avoid `unwrap` and `expect` on live process, filesystem, network,
  configuration, and Win32 paths.
- Treat unsupported internal enum variants or impossible UI wiring as invariant
  violations with a clear message; do not silently convert them into a no-op.

### Maintainability Review Workflow

Use this order for simplification and over-engineering reviews:

1. Query Graphify, then read the complete module and trace every caller and
   lifecycle path before proposing a change.
2. Ask whether each type, helper, field, scan, or abstraction needs to exist.
   Reuse an existing project helper, the standard library, or Win32 directly
   before adding code.
3. Separate genuine state ownership from namespacing. Keep stateful managers
   that own timers, resources, restoration, or shutdown; replace stateless
   structs with functions when that makes callers simpler.
4. Remove parallel representations, duplicate scans, unused status fields, and
   one-use wrappers when one source of truth is sufficient.
5. Prefer the smallest coherent deletion-first change. Do not perform renames,
   file moves, or abstraction swaps that merely reshuffle working code.
6. Review the exact diff for changed behavior and damaged tests, especially
   after mechanical edits. Preserve runtime safety, failure handling, cleanup,
   and process-state restoration.
7. Compile all targets, run the required checks and tests, then update Graphify.
   If no change clearly reduces maintenance cost, leave the code alone and say
   why.

## Release Runbook

1. Start from a clean, current `dev`. Choose a SemVer-compatible prerelease
   version such as `0.2.0-alpha`; do not create its tag yet. Confirm `gh` is
   installed and authenticated before starting the GitHub publishing steps.
2. Update the package version in `Cargo.toml`, refresh the root package entry in
   `Cargo.lock`, and add the dated release section to `CHANGELOG.md`. Verify that
   the lockfile contains no unrelated dependency changes. Build the changelog
   from commits after the previous tag and do not rewrite a published section.
3. Run the default checks, the naming scan below, and
   `.\scripts\build_release.cmd`. Complete a Windows smoke test of the resulting
   executable. If UI automation is unavailable, obtain the user's explicit
   smoke-test confirmation before tagging.
4. Commit and push the release preparation to `dev`, wait for CI, then open and
   merge a `dev` to `main` pull request without deleting `dev`. Wait for CI on
   the final `main` commit.
5. Create and push an annotated tag on that final `main` commit. Never create
   the release tag on `dev`:

   ```powershell
   git tag -a v0.2.0-alpha -m "Winderust v0.2.0-alpha"
   git push origin v0.2.0-alpha
   ```

6. The `Draft Release` workflow validates the tag against Cargo metadata,
   repeats verification, builds the executable, and creates a draft prerelease
   with a ZIP and SHA-256 file. Release workflows are uncached and may take
   significantly longer than regular CI. Confirm the workflow succeeded and
   verify the checksum, ZIP contents, and embedded executable version. Publish
   the draft only when the user explicitly requests publication or has already
   approved the complete release flow.
7. After publication, verify the release is a prerelease rather than a draft,
   confirm both assets are available, sync local `main` with `origin/main`, and
   keep `dev` as the integration branch for subsequent work.

Never move or recreate a published tag. If a draft workflow fails, fix the
cause on `main`; only replace an unpublished tag when no release was published
and the corrected commit must be the tagged source.

## Source Map

- `src/main.rs`: app entry, single-instance guard, GPUI startup.
- `src/ui/app.rs`: the single GPUI `WinderustApp` composition root, construction, rendering,
  teardown, and app-level tests. Operational method groups live in `src/ui/app/*.rs`. Plain
  transition/read models are `shell_model.rs`, `dashboard_model.rs`, `process_models.rs`, and
  `update_model.rs`; sampling, queries, icons, GPUI timers, tray, dialogs, focus, and motion remain
  at the composition root rather than moving into `RuntimeCore`.
- `src/ui/app/pages/`: page and shell renderers. `src/ui/app/shared/`: reusable UI components, state helpers, formatting, policies, and shared feature logic. Page dispatch is
  in `app_shell.rs`; process add/check and rule-construction helpers are in
  `process_policies.rs`. Process List table grouping, sorting, and layout live in
  `pages/process_list_page/table_model.rs`; edit actions live in `editing.rs`.
- `src/ui.rs`: page enum, section grouping, labels, and small UI-independent helpers.
- `src/config/settings.rs`: persisted settings structs and defaults.
- `src/config/storage.rs`: config path, TOML load/save/import/export.
- `src/application/settings.rs`: `SettingsEditor`, the sole settings draft/revision/persistence and
  startup-intent application boundary. `src/application/win32_priority_separation.rs` and
  `src/application/advanced_power_plan_tuning.rs` own their explicit persistent Windows
  operations and typed staged errors; neither is temporary managed or crash-recovery state.
- `src/backend/automation.rs`: `RuntimeHandle`, event-source lifecycle, and worker scheduling loop. Runtime feature execution and power-plan policy state live in `src/backend/automation/runner.rs`; wake/event decisions live in `wake.rs`; status fan-out lives in `status.rs`; activation predicates and refresh timing live in `requirements.rs`.
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
  (Hard), Core Limiter, and Workload Engine CPU allocation. Its precedence is CPU Sets (Soft) >
  Processor Affinity (Hard) > Core Limiter > Adaptive Engine.
  `src/platform/windows/cpu_allocation.rs` owns the raw affinity/CPU Set query and write calls plus
  packed system CPU Set topology conversion.
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
- `src/control/suspension.rs`: sole normal App Suspension process/job handle,
  freeze/thaw transaction, retry, and clean-release boundary. Automatic rules,
  App-page Freeze, and Process List Suspend/Resume share its typed RuntimeCore
  routes. Pending compensation and journal cleanup reconcile on later feature
  passes; feature records retain exact creation time and are pruned when the
  controller no longer owns a frozen exact instance. The feature manager owns
  rule/grace/wake/reporting policy only. `src/platform/windows/suspension.rs` owns raw Job Object
  creation, assignment, membership, freeze/thaw, and the shared compatibility-sensitive layout.
- `src/backend/self_power.rs`: instance-owned composition, verification, compensation, retry, and
  restoration of Winderust's hidden/Adaptive process priority and Power Throttling state.
  `src/platform/windows/self_power.rs` is the sole raw current-process query/set adapter; this
  process-lifetime state intentionally has no crash-recovery journal.
- `src/backend/file_dialog.rs`: native settings and Action Log file dialogs.
- `src/backend/update_checker.rs`: GitHub release checks and Stable/Pre-release filtering.
- `src/rules/decision_engine.rs`: power-plan decision priority.
- Feature backends use the UI names. CPU Sets (Soft) and Processor Affinity
  (Hard) retain separate settings, pages, rules, status, and Action Log labels;
  `src/features/cpu_control/cpu_allocation.rs` owns their discovery, topology,
  tier selection, and reporting policy, while the typed controller owns the shared
  Windows mechanism and restoration state. Core Limiter retains sampling and
  hysteresis only. Workload Engine retains pressure, selection, and tuning only.
  Workload Engine process sampling and identity helpers live in
  `workload_engine/process_control.rs`; its Process Priority, Power Throttling,
  and Memory Priority mutations route through typed controllers. Pure workload
  decisions and core-selection calculations live in `workload_engine/policy.rs`;
  stateful policy lifecycle remains in `workload_engine.rs`.

## Navigation

Pages are grouped in `src/ui.rs`:

- Home: dashboard.
- Process List: process table and per-process policy surface.
- Winderust Features: Adaptive Engine, Background Efficiency, Memory Trim.
- Power Plan Control: By Foreground, By Running App, By CPU Load, By Activity, By Time, Advanced Power Plan Tuning.
- Priority Control: Process Priority, Thread Priority, Dynamic Priority Boost,
  IO Priority, GPU Priority, Memory Priority.
- CPU Control: Core Limiter, CPU Sets (Soft), Processor Affinity (Hard).
- Action Log.
- Settings: Winderust Behaviour, Language and Appearance, Experimental
  Features.
- About.
- Advanced: App Suspension, Timer Resolution, Win32 Priority Separation.

Keep navigation changes in `Page`, `PAGE_SECTIONS`, labels, locale files, and
`WinderustApp::render_page` in `src/ui/app/pages/app_shell.rs` together.

## Settings

- Runtime settings live in `Settings`.
- Use `#[serde(default)]` only when a current setting is intentionally optional; do not add pre-release migration aliases.
- If a setting is edited through the UI, update the relevant page module and
  input synchronization in `src/ui/app.rs`.
- TOML import/export uses native Windows file dialogs from `src/backend/file_dialog.rs`, invoked by the UI.

### Power Plan Ownership

- `ByActivitySettings::power_plans` owns the visible Idle and Active plan selections.
- By Foreground, By Running App, By CPU Load, and By Time store the chosen GUID on each rule.
- A rule without a selected plan does not inherit a hidden global plan.
- Do not reintroduce `Settings::power_plans`, per-feature unused mapping fields, or load-time mapping fill/migration helpers.

## Naming

- Start from the English UI label, then keep page variants, settings types/fields, feature modules, backend snapshots, tests, locale keys, scripts, and docs as close to that label as Rust naming permits.
- Current canonical examples: `AdaptiveEngine`, `BackgroundEfficiency`, `ByRunningApp`, `CoreLimiter`, `CpuSetsSoft`, `ProcessorAffinityHard`, and `DynamicPriorityBoost`.
- Workload Engine is the CPU-scheduling subsystem exposed inside Adaptive Engine; keep that name for its settings and implementation, not as a separate top-level product feature.
- Do not use retired product identifiers such as Smart Saver, EcoQos settings/managers, Background CPU Restriction, Core Steering, Soft CPU Sets, Hard CPU Affinity, or CPU Limiter feature names. `Performance Mode` is valid only for the active state held by By Running App, not as a standalone feature or settings page.
- Native Windows vocabulary is allowed when it describes the implementation rather than the product surface, for example EcoQoS flags, affinity masks, CPU Sets, and `SetProcessPriorityBoost`.

Run this quick compatibility/naming check before handoff:

```powershell
rg -n -i --glob '!target/**' --glob '!graphify-out/**' --glob '!.git/**' --glob '!.agents/**' --glob '!CONTRIBUTING.md' 'PowerLeaf|Smart Saver|Smart Trim|Background CPU Restriction|Core Steering|Soft CPU Sets|Hard CPU Affinity|background_cpu_restriction|core_steering|soft_cpu_sets|hard_cpu_affinity|serde.*alias|fill_missing_power_plan_mappings|Settings::power_plans' .
```

## Runtime Safety

Process-control features must keep these defaults:

- Do not target Winderust itself.
- Do not target protected/system processes. Cross-session targeting follows `general.allow_cross_session_process_control`; even when enabled, preserve Windows access checks, built-in exclusions, and identity revalidation.
- Treat access denied as skipped unless it indicates a real implementation bug.
- Restore previous process state on disable, process exit, app shutdown, or rule mismatch when the backend can observe it.
- Treat restoration as a barrier, not best-effort bookkeeping. Capture the
  original value before the first mutation, bind it to the validated process
  identity, and refuse the mutation when its original reversible state cannot
  be preserved.
- On clean shutdown, stop applying new work and restore overlapping changes in
  reverse application order so one feature cannot restore another feature's
  intermediate value. `RuntimeCore::shutdown` and `PowerPlanController` own the
  automatic restoration order; `RuntimeHandle` restores Winderust self-power;
  reversible Process List actions are owned and restored by `RuntimeCore`.
- Before every reversible process, thread, App Suspension, or automatic
  power-plan mutation, synchronously send the captured original and expected
  state to the external crash-recovery watchdog and wait for its acknowledgement.
  Block the mutation if the watchdog is unavailable. For App Suspension, the
  watchdog must retain its own exact named Job Object handle before acknowledging
  the freeze. Recovery thaws that helper-held job even if the recorded root exits,
  because inherited children may remain frozen. Acquire new suspension jobs under
  the current cross-session/session/service/protection/access policy; always allow
  cleanup of an already-owned exact job.
  Recovery must revalidate process/thread identity and
  only unwind a journal segment while its expected state still matches.
- Restore the power plan that preceded Winderust's first automatic switch.
- Irreversible operations such as process termination and memory trimming,
  watchdog termination, Windows shutdown, and power loss cannot be covered by
  the runtime-restoration barrier.
- Keep High/Realtime priority out of automatic paths.
- Keep broad app suspension opt-in and narrow.

## UI Rules

- Keep controls compact and operational.
- Use existing GPUI/gpui-component helpers before adding new UI primitives.
- Keep plan mapping inside the relevant power-plan pages, not in a global settings page.
- Do not reintroduce removed sidebar/manual-pause/test buttons without a current product reason.
- Keep `src/ui/app.rs` for shared app state, construction, rendering, and teardown; keep operational method groups in `src/ui/app/*.rs`. Put a
  complete page in `src/ui/app/pages/` and repeated helper families in `src/ui/app/shared/`;
  do not start a framework rewrite.
- Multiple focused `impl WinderustApp` blocks are acceptable for the private UI
  module. Keep glob imports contained there; use explicit imports when a module
  gains independent ownership or its dependencies become unclear, not as
  mechanical churn.

## Windows APIs

- Power plan and processor tuning: lifecycle/application semantics in `src/control/power_plan.rs`,
  `src/application/advanced_power_plan_tuning.rs`, and `src/power/powercfg.rs`; raw GUID,
  power-scheme, processor-setting, and effective-mode calls in
  `src/platform/windows/power_plan.rs`.
- Foreground and process enumeration: `src/foreground/`.
- Idle and input hooks: `src/activity/`.
- Tray behavior: `src/backend/tray.rs`.
- Timer Resolution: policy in `src/features/advanced_controls/timer_resolution.rs`, lifecycle
  ownership in `src/control/timer_resolution.rs`, and raw WinMM calls in
  `src/platform/windows/timer_resolution.rs`.
- Irreversible process commands: typed orchestration in `src/control/memory_trim.rs` and
  `src/control/process_termination.rs`; raw calls in the matching `src/platform/windows/` modules.
- Shared process-control acquisition: typed identity/safety validation in `src/control/process.rs`;
  minimal mutation/command access masks and raw `OpenProcess` in
  `src/platform/windows/process.rs`. Read-only Process List, Core Limiter, and Workload Engine
  sampling remains observation input and cannot authorize a write; every mutation reopens the
  exact target through the shared control boundary.
- Process Priority and Power Throttling: shared lifecycle/arbitration in
  `src/control/priority_efficiency.rs`; raw priority-class and `ProcessPowerThrottling` calls in
  `src/platform/windows/priority_efficiency.rs`.
- Thread Priority: exact process/thread identity and lifecycle in
  `src/control/thread_priority.rs`; Toolhelp enumeration and raw thread operations in
  `src/platform/windows/thread_priority.rs`.
- CPU Sets and affinity: shared arbitration/restoration in `src/control/cpu_allocation.rs`; raw
  affinity, CPU Set, and system CPU Set topology calls in
  `src/platform/windows/cpu_allocation.rs`.
- App Suspension: exact lifecycle/recovery transaction in `src/control/suspension.rs`; raw named
  Job Object operations and freeze layout in `src/platform/windows/suspension.rs`.
- Win32 Priority Separation: page logic in
  `src/ui/app/pages/win32_priority_separation_page.rs`, bit/value helpers in
  `src/ui/app/shared/appearance.rs`, and registry access in
  `src/backend/win_registry.rs`.

Prefer native API calls already used in the repo. Do not add command spawning around `powercfg` unless the Win32 path cannot support the needed behavior.

When adding, removing, or changing a feature-defining or
compatibility-sensitive Win32, NT, or WDK boundary, update
`30-reference-library.md` in the same change. Link directly to official
Microsoft documentation when it exists and explicitly mark numeric information
classes or manually declared interfaces that are not stable public SDK
contracts.

- Keep the crate-level `clippy::undocumented_unsafe_blocks` and
  `unsafe_op_in_unsafe_fn` warnings enabled.
- Prefer private safe wrappers around Win32 FFI. Required `unsafe extern`
  callbacks must still wrap unsafe operations in explicit blocks.
- Put an immediately preceding `// SAFETY:` comment on each unsafe block that
  states the handle, pointer, buffer, lifetime, ownership, and ABI invariants
  relevant to that call.
- Capture `GetLastError` immediately after the failing Win32 call, before any
  other call can overwrite the thread-local error.
- Put owned Windows handles in the existing RAII wrappers and release each
  resource exactly once.
