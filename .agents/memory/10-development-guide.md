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

## Source Map

- `src/main.rs`: app entry, single-instance guard, GPUI startup.
- `src/ui/app.rs`: main GPUI state, rendering, navigation, dialogs, and page wiring. It is large; shrink by moving one complete page/helper cluster at a time.
- `src/ui.rs`: page enum, section grouping, labels, and small UI-independent helpers.
- `src/config/settings.rs`: persisted settings structs and defaults.
- `src/config/storage.rs`: config path, TOML load/save/import/export.
- `src/backend/automation.rs`: background worker loop that applies runtime policies.
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
- TOML import/export uses native Windows file dialogs from `src/ui/app.rs`.

### Power Plan Ownership

- `ByActivitySettings::power_plans` owns the visible Idle and Active plan selections.
- By Foreground, By Running App, By CPU Load, and By Time store the chosen GUID on each rule.
- A rule without a selected plan does not inherit a hidden global plan.
- Do not reintroduce `Settings::power_plans`, per-feature unused mapping fields, or load-time mapping fill/migration helpers.

## Naming

- Start from the English UI label, then keep page variants, settings types/fields, feature modules, backend snapshots, tests, locale keys, scripts, and docs as close to that label as Rust naming permits.
- Current canonical examples: `AdaptiveEngine`, `BackgroundEfficiency`, `ByRunningApp`, `CoreLimiter`, `CoreSteering`, and `DynamicPriorityBoost`.
- Do not use retired product identifiers such as Smart Saver, EcoQos settings/managers, CPU Affinity feature names, CPU Limiter feature names, or Performance Mode settings names.
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
