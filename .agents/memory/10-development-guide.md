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
cargo fmt -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

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
- `src/ui/app.rs`: main GPUI state, rendering, navigation, dialogs, and page wiring. It is large; shrink by moving one complete page/helper cluster at a time.
- `src/ui.rs`: page enum, section grouping, labels, and small UI-independent helpers.
- `src/config/settings.rs`: persisted settings structs and defaults.
- `src/config/storage.rs`: config path, TOML load/save/import/export.
- `src/backend/automation.rs`: background worker loop that applies runtime policies.
- `src/backend/file_dialog.rs`: native settings and Action Log file dialogs.
- `src/backend/update_checker.rs`: GitHub release checks and Stable/Pre-release filtering.
- `src/rules/decision_engine.rs`: power-plan decision priority.
- Feature backends use the UI names: `background_efficiency`, `workload_engine`, `memory_trim`, `app_suspension`, `core_limiter`, `core_steering`, `by_running_app`, and the priority-control modules.

## Navigation

Pages are grouped in `src/ui.rs`:

- Overview: dashboard.
- Process List: process table and per-process policy surface.
- Winderust Features: Adaptive Engine, Background Efficiency, Memory Trim.
- Power Plan Control: By Foreground, By Running App, By CPU Load, By Activity, By Time, Advanced Power Plan Tuning.
- Priority Control: CPU priority, thread priority, dynamic priority boost, IO priority, GPU priority, memory priority.
- CPU Control: Core Limiter, Background CPU Restriction, Core Steering.
- Action Log.
- Settings.
- Advanced: App Suspension, Timer Resolution, Win32 priority separation.

Keep navigation changes in `Page`, `PAGE_SECTIONS`, labels, locale files, and `WinderustApp::render_page` together.

## Settings

- Runtime settings live in `Settings`.
- Use `#[serde(default)]` only when a current setting is intentionally optional; do not add pre-release migration aliases.
- If a setting is edited through the UI, update the relevant input sync code in `src/ui/app.rs`.
- TOML import/export uses native Windows file dialogs from `src/backend/file_dialog.rs`, invoked by the UI.

### Power Plan Ownership

- `ByActivitySettings::power_plans` owns the visible Idle and Active plan selections.
- By Foreground, By Running App, By CPU Load, and By Time store the chosen GUID on each rule.
- A rule without a selected plan does not inherit a hidden global plan.
- Do not reintroduce `Settings::power_plans`, per-feature unused mapping fields, or load-time mapping fill/migration helpers.

## Naming

- Start from the English UI label, then keep page variants, settings types/fields, feature modules, backend snapshots, tests, locale keys, scripts, and docs as close to that label as Rust naming permits.
- Current canonical examples: `AdaptiveEngine`, `BackgroundEfficiency`, `ByRunningApp`, `CoreLimiter`, `CoreSteering`, and `DynamicPriorityBoost`.
- Workload Engine is the CPU-scheduling subsystem exposed inside Adaptive Engine; keep that name for its settings and implementation, not as a separate top-level product feature.
- Do not use retired product identifiers such as Smart Saver, EcoQos settings/managers, CPU Affinity feature names, or CPU Limiter feature names. `Performance Mode` is valid only for the active state held by By Running App, not as a standalone feature or settings page.
- Native Windows vocabulary is allowed when it describes the implementation rather than the product surface, for example EcoQoS flags, affinity masks, CPU Sets, and `SetProcessPriorityBoost`.

Run this quick compatibility/naming check before handoff:

```powershell
rg -n -i --glob '!target/**' --glob '!graphify-out/**' --glob '!.git/**' --glob '!.agents/**' 'PowerLeaf|Smart Saver|Smart Trim|serde.*alias|fill_missing_power_plan_mappings|Settings::power_plans' .
```

## Runtime Safety

Process-control features must keep these defaults:

- Do not target Winderust itself.
- Do not target protected/system/session-mismatched processes.
- Treat access denied as skipped unless it indicates a real implementation bug.
- Restore previous process state on disable, process exit, app shutdown, or rule mismatch when the backend can observe it.
- Keep High/Realtime priority out of automatic paths.
- Keep broad app suspension opt-in and narrow.

## UI Rules

- Keep controls compact and operational.
- Use existing GPUI/gpui-component helpers before adding new UI primitives.
- Keep plan mapping inside the relevant power-plan pages, not in a global settings page.
- Do not reintroduce removed sidebar/manual-pause/test buttons without a current product reason.
- For `src/ui/app.rs` cleanup, move one complete page or repeated helper family at a time; do not start a framework rewrite.

## Windows APIs

- Power plan and processor tuning: `src/power/powercfg.rs`.
- Foreground and process enumeration: `src/foreground/`.
- Idle and input hooks: `src/activity/`.
- Tray behavior: `src/backend/tray.rs`.
- Timer resolution: `src/features/advanced_controls/timer_resolution.rs`.
- Win32 priority separation: registry code in `src/ui/app.rs`.

Prefer native API calls already used in the repo. Do not add command spawning around `powercfg` unless the Win32 path cannot support the needed behavior.
