# PowerLeaf Development Guide

This guide is for future agent sessions. It captures the current architecture and the UI/behavior decisions made during recent development.

## Project Basics

- Windows-only Rust desktop app using `eframe`/`egui`.
- Release binary is built with:

```powershell
cargo build --release
```

- If `target\release\powerleaf.exe` is locked because the app is running, use:

```powershell
cargo build --release --target-dir target-next
```

- Standard verification before handoff:

```powershell
cargo fmt
cargo test
cargo build --release --target-dir target-next
```

## Current App Shape

Navigation pages are defined in `src/ui/mod.rs`:

- `Dashboard`
- `Action Based Scheduler`
- `CPU Usage Scheduler`
- `Efficiency Mode`
- `Time Based Scheduler`
- `Foreground Rules`
- `Settings`
- `About`

`src/app.rs` owns the main application state and wires pages together.

## Settings And Persistence

Settings model lives in `src/config/settings.rs`.

Primary settings storage:

- TOML file in the user config directory.
- Path helper: `config::storage::config_path()`.
- Load/save helpers: `config::storage::load()` and `config::storage::save()`.
- `load()` falls back to the legacy `PowerSwitcher` config folder when no `PowerLeaf` settings file exists.
- Runtime settings take effect immediately. Unsaved settings show a global bottom popup with Save and Cancel actions; disk persistence happens through Save or INI import.

INI import/export:

- Implemented in `src/config/storage.rs`.
- Export uses `export_ini_to(path, settings)`.
- Import uses `import_ini_from(path)`.
- Settings tab invokes native Windows open/save file dialogs through Win32 common dialog APIs in `src/app.rs`.
- INI round-trip coverage exists in `config::storage::tests::ini_round_trip_preserves_settings`.

When adding settings fields:

- Update `Settings` structs and defaults.
- Update TOML compatibility using `#[serde(default)]` where older config files need to keep loading.
- Update INI export/import in `storage.rs`.
- Update the INI round-trip test.

## Power Plan Mapping

All Idle/Active plan mapping controls belong in the Settings tab.

Current behavior:

- `Settings` page renders `ui::power_plan_page::show(...)`.
- Action Based Scheduler must not show plan mapping.
- Time Based Scheduler must not show per-rule plan mapping.
- CPU Usage Scheduler must not show per-rule plan mapping.
- Schedule decisions use the global Idle/Active plan mappings from Settings.

Do not reintroduce `Test Idle` / `Test Active`; those buttons were intentionally removed.

## Action Based Scheduler

Action-based switching uses a hybrid model:

- Polling remains the authoritative mechanism for idle timeout and fallback checks.
- `GetLastInputInfo` is still used by `IdleDetector`.
- `src/activity/input_hook.rs` installs low-level keyboard/mouse hooks.
- Hooks wake the egui loop and trigger an immediate check when the enabled input type occurs.
- Hook events are coalesced: repeated keyboard/mouse events only request one repaint until the app drains pending input events.

Important constraints:

- Hooks only accelerate active-resume checks.
- Polling is still required to detect "became idle after N seconds."
- The UI label is `Check interval`, not `Automation check interval`.
- Input type checkboxes are individual:
  - `Keyboard input`
  - `Mouse input`
- At least one input type must stay enabled.

## Time Based Scheduler

`src/ui/schedule_page.rs` owns schedule editing.

Current scheduler UI includes:

- Enable toggle.
- Rule add/remove.
- Name.
- Days.
- Start/end time.

It intentionally does not expose Idle/Active plan selectors. Scheduler output maps to global plans in Settings.

## CPU Usage Scheduler

`src/ui/cpu_usage_page.rs` owns CPU usage rule editing.

Current behavior:

- CPU usage is sampled with `GetSystemTimes` in `src/cpu/mod.rs`.
- Dashboard shows total CPU usage after the second sample.
- Rules live in `settings.cpu_usage_mode.rules`.
- Each rule has name, comparison, threshold percentage, duration, and target role.
- Rule target roles map to the global Idle/Active plans from Settings.
- CPU usage rules are checked in list order. The first rule whose condition has held for its duration wins.

## Efficiency Mode

`src/ecoqos/mod.rs` owns Windows EcoQoS application and restore behavior.
`src/ui/efficiency_page.rs` owns the settings page.

Current behavior:

- Settings live in `settings.eco_qos`.
- The feature is disabled by default.
- `BackgroundAutomation` runs the `EcoQosManager` every five seconds while the app is visible or hidden to tray.
- The manager applies Task Manager-style Efficiency Mode: `PROCESS_POWER_THROTTLING_EXECUTION_SPEED` plus `IDLE_PRIORITY_CLASS`.
- The manager preserves each process's previous `PROCESS_POWER_THROTTLING_STATE` and priority class when possible, then restores those values when the process is no longer targeted.
- `exclude_foreground_app` defaults to true. When enabled, foreground detection failure pauses and clears throttling; same-name processes as the foreground app are skipped too. When disabled, foreground detection is not required.
- It pauses and clears throttling if automation is disabled, Efficiency Mode is disabled, or the current Windows session cannot be identified.
- It only targets processes in the current user session.
- It never targets the PowerLeaf process. It only skips the current foreground process when `exclude_foreground_app` is enabled.
- Built-in exclusions cover Windows shell/input processes such as `explorer.exe`, `dwm.exe`, and `textinputhost.exe`.
- Access-denied process opens are counted as skipped, not failed. This is expected for protected/elevated Windows processes.
- The Efficiency Whitelist is edited in the Efficiency Mode page with the same searchable running-app dropdown pattern as Foreground Rules and is persisted to TOML/INI.

Avoid copying EnergyStarX code into this project. If EcoQoS behavior needs to change, implement against Microsoft Win32 documentation directly.

## Foreground Rules

`src/ui/rules_page.rs` owns this page.

Current behavior:

- Has an `Enable foreground rules` toggle.
- When disabled, rule list controls are grayed out.
- Decision engine ignores foreground rules when disabled.
- Force Idle wins if an app appears in both lists, though the UI is designed to avoid adding duplicates across lists.

Terminology:

- `whitelist` is still the internal config field for compatibility.
- UI text says `Force Active Plan`.
- `force_power_save` is the internal field for `Force Idle Plan`.

Dropdown behavior:

- Force Active and Force Idle inputs are searchable running-app dropdowns.
- Running-app candidates refresh lazily while dropdowns are open, throttled by `PROCESS_REFRESH_INTERVAL` in `src/app.rs`.
- Click/focus opens the dropdown.
- Typing filters.
- Up/Down navigates.
- Enter or click fills the input with the highlighted app; Add commits it to the list.
- Mouse hover only changes highlight when the pointer actually moves.
- Hover must not scroll or make the dropdown slide around.
- Running apps already added to either Force Active or Force Idle are hidden from both dropdowns.
- Manual typed entries still go through duplicate protection.

Layout expectations:

- Remove buttons in both rule lists should align right.
- List labels should truncate instead of pushing buttons.

## System Tray

Tray support is in `src/tray.rs`.

Behavior:

- Controlled by `settings.general.hide_to_tray`.
- Closing the window hides to tray when enabled.
- Tray menu can show or quit the app.

Keep tray logic Windows-only and avoid console popups.

## Windows Process And Power APIs

Power plan operations:

- `src/power/powercfg.rs`
- Uses Win32 power APIs for plan enumeration, active-plan reads, and switching.
- The module name is historical; avoid reintroducing `powercfg` command spawning unless the Win32 API path fails.

Foreground app detection:

- Active foreground process: `src/foreground/active_window.rs`.
- Running process list for dropdowns: `src/foreground/process_list.rs`.

EcoQoS:

- `src/ecoqos/mod.rs`.
- Uses `GetProcessInformation` / `SetProcessInformation` with `ProcessPowerThrottling`.
- Uses `SetPriorityClass(IDLE_PRIORITY_CLASS)` while a process is throttled and restores the previous priority or `NORMAL_PRIORITY_CLASS`.
- Uses `ProcessIdToSessionId` to avoid targeting system-session processes.

Input detection:

- Polling idle time: `src/activity/input_tracker.rs`.
- Hybrid input hooks: `src/activity/input_hook.rs`.

## Decision Priority

Decision engine lives in `src/rules/decision_engine.rs`.

Current priority:

1. Automation disabled.
2. Manual override state if present in config/imported settings.
3. Foreground rules, only when `foreground_rules.enabled`.
4. Time scheduler.
5. CPU usage scheduler.
6. Action/activity mode.
7. Default Active plan.

The manual override sidebar UI was removed. The enum remains for backward compatibility with existing settings and INI/TOML imports.

## UI Guidance For Future Edits

- Keep operational controls in their relevant tabs.
- Keep global plan mapping in Settings only.
- Avoid reintroducing the removed Running Applications section.
- Avoid reintroducing `Add current app`.
- Avoid reintroducing manual pause controls in the sidebar.
- Use existing `egui` patterns and helpers rather than adding new UI frameworks.
- Keep controls compact and utilitarian.
