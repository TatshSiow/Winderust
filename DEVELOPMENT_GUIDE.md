# Winderust Development Guide

This is the working guide for code changes. Product scope and future goals live in `PROJECT_SCOPE.md`.

## Project Basics

- Windows-only Rust desktop app.
- UI stack: GPUI plus `gpui-component`.
- Settings format: TOML through `serde` and `toml`.
- Localization: `rust-i18n` with files in `locales/`.
- Windows integration: direct Win32 APIs through `windows` and `windows-sys`.

Use these checks before handoff:

```powershell
cargo fmt
cargo check
```

For release builds:

```powershell
cargo build --release
```

If `target\release\winderust.exe` is locked because the app is running:

```powershell
cargo build --release --target-dir target-next
```

## Source Map

- `src/main.rs`: app entry, single-instance guard, GPUI startup.
- `src/app.rs`: main GPUI state, rendering, navigation, dialogs, and page wiring. It is large; shrink by moving one complete page/helper cluster at a time.
- `src/ui/mod.rs`: page enum, section grouping, labels, and small UI-independent helpers.
- `src/config/settings.rs`: persisted settings structs, defaults, compatibility deserializers.
- `src/config/storage.rs`: config path, TOML load/save/import/export.
- `src/automation.rs`: background worker loop that applies runtime policies.
- `src/rules/decision_engine.rs`: power-plan decision priority.
- Feature backends live in their own modules, for example `ecoqos`, `suspension`, `affinity`, `cpu_limiter`, `responsiveness`, `smart_trim`, `watchdog`, and priority modules.

## Navigation

Pages are grouped in `src/ui/mod.rs`:

- Overview: dashboard.
- Process List: process table and per-process policy surface.
- Foreground Responsiveness.
- Power Plan Automation: foreground rules, running app/performance mode, CPU usage, activity, schedule, processor power tuning.
- Process Policies: Efficiency Mode, IO/GPU/launch priority, Watchdog.
- Processor Controls: CPU limiter, background CPU restriction, CPU affinity.
- Memory Control: Memory Priority, Smart Trim.
- Action Log.
- Settings.
- Advanced: App Suspension, Timer Resolution, Win32 priority separation.

Keep navigation changes in `Page`, `PAGE_SECTIONS`, labels, locale files, and `WinderustApp::render_page` together.

## Settings

- Runtime settings live in `Settings`.
- Add new fields with `#[serde(default)]` when older config files must keep loading.
- If a setting is edited through the UI, update the relevant input sync code in `src/app.rs`.
- TOML import/export uses native Windows file dialogs from `src/app.rs`.
- Legacy fields should deserialize, migrate, and then stay out of the UI unless the user asks for them back.

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
- For `src/app.rs` cleanup, move one complete page or repeated helper family at a time; do not start a framework rewrite.

## Windows APIs

- Power plan and processor tuning: `src/power/powercfg.rs`.
- Foreground and process enumeration: `src/foreground/`.
- Idle and input hooks: `src/activity/`.
- Tray behavior: `src/tray.rs`.
- Timer resolution: `src/timer_resolution.rs`.
- Win32 priority separation: registry code in `src/app.rs`.

Prefer native API calls already used in the repo. Do not add command spawning around `powercfg` unless the Win32 path cannot support the needed behavior.
