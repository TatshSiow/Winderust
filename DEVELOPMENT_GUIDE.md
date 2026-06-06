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
- `Action Detection`
- `CPU Load Detection`
- `Efficiency Mode`
- `App Suspension`
- `Time Scheduler`
- `Foreground Detection`
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
- `settings.general.pause_power_plan_switching_while_plugged_in` only pauses power-plan switching when Windows reports AC power. It does not pause Efficiency Mode or App Suspension.
- Exception: EcoQoS and App Suspension activation and target changes are save-gated. `PowerLeafApp::background_settings()` sends the last saved EcoQoS/App Suspension settings to `BackgroundAutomation` while the current edited settings remain enabled but unsaved. Disabling either feature is allowed to take effect immediately.

INI import/export:

- Implemented in `src/config/storage.rs`.
- Export uses `export_ini_to(path, settings)`.
- Import uses `import_ini_from(path)`.
- Settings tab invokes native Windows open/save file dialogs through Win32 common dialog APIs in `src/app.rs`.
- Export defaults to `powerleaf_{version}_{date}.ini` and writes comment metadata for version and export date before `[general]`.
- INI round-trip coverage exists in `config::storage::tests::ini_round_trip_preserves_settings`.

When adding settings fields:

- Update `Settings` structs and defaults.
- Update TOML compatibility using `#[serde(default)]` where older config files need to keep loading.
- Update INI export/import in `storage.rs`.
- Update the INI round-trip test.

## Power Plan Selection

Idle/Active plan mapping controls belong inside Action, Time, and CPU power-plan control pages, not in a global page. Foreground Detection is different: each foreground rule can target any available Windows power plan directly.

Current behavior:

- `ui::power_plan_page::show_section(...)` renders the shared embedded plan selector.
- Action Detection owns `settings.activity_mode.power_plans`.
- Time Scheduler owns `settings.schedule_mode.power_plans`.
- CPU Load Detection owns `settings.cpu_usage_mode.power_plans`.
- Foreground Detection stores each custom target in `settings.foreground_rules.rules[*].power_plan_guid`.
- Settings must not show power plan mapping controls.
- There is no standalone global Power Plan Mapping page.
- `settings.power_plans` remains for legacy config compatibility and fallback only.

Do not reintroduce `Test Idle` / `Test Active`; those buttons were intentionally removed.

## Action Detection

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

## Time Scheduler

`src/ui/schedule_page.rs` owns schedule editing.

Current scheduler UI includes:

- Enable toggle.
- Rule add/remove.
- Name.
- Days.
- Start/end time.

It exposes one shared Idle/Active plan selector for the Time Scheduler page. Do not add per-rule plan selectors unless explicitly requested.

## CPU Load Detection

`src/ui/cpu_usage_page.rs` owns CPU usage rule editing.

Current behavior:

- CPU usage is sampled with `GetSystemTimes` in `src/cpu/mod.rs`.
- Dashboard shows total CPU usage after the second sample.
- Rules live in `settings.cpu_usage_mode.rules`.
- Each rule has name, comparison, threshold percentage, duration, and target role.
- Rule target roles map to `settings.cpu_usage_mode.power_plans`.
- CPU usage rules are checked in list order. The first rule whose condition has held for its duration wins.

## Efficiency Mode

`src/ecoqos/mod.rs` owns Windows EcoQoS application and restore behavior.
`src/ui/efficiency_page.rs` owns the settings page.

Current behavior:

- Settings live in `settings.eco_qos`.
- The feature is disabled by default.
- EcoQoS activation and target changes apply only after Save; disabling EcoQoS takes effect immediately.
- `BackgroundAutomation` runs the `EcoQosManager` every second while the app is visible or hidden to tray.
- The manager applies Task Manager-style Efficiency Mode: `PROCESS_POWER_THROTTLING_EXECUTION_SPEED` plus `IDLE_PRIORITY_CLASS`.
- The manager preserves each process's previous `PROCESS_POWER_THROTTLING_STATE` and priority class when possible, then restores those values when the process is no longer targeted.
- `exclude_foreground_app` defaults to true. When enabled, foreground detection failure pauses and clears throttling; same-name processes as the foreground app are skipped too. When disabled, foreground detection is not required.
- It pauses and clears throttling if automation is disabled, Efficiency Mode is disabled, or the current Windows session cannot be identified.
- It only targets processes in the current user session.
- It never targets the PowerLeaf process. It only skips the current foreground process when `exclude_foreground_app` is enabled.
- Built-in exclusions cover Windows shell/input processes such as `explorer.exe`, `dwm.exe`, and `textinputhost.exe`.
- Access-denied process opens are counted as skipped, not failed. This is expected for protected/elevated Windows processes.
- The Efficiency Whitelist is edited in the Efficiency Mode page with the same searchable running-app dropdown pattern as Foreground Detection and is persisted to TOML/INI.

Avoid copying EnergyStarX code into this project. If EcoQoS behavior needs to change, implement against Microsoft Win32 documentation directly.

## Processor Power Plan Tuning

`src/power/powercfg.rs` owns Windows power plan enumeration, switching, and processor-power tuning writes.

Current behavior:

- The UI lives on the Core Parking page under Power Plan Controls, not on the general Settings page.
- Processor power tuning is applied directly to the selected Windows power plan.
- These settings are persisted by Windows in the power plan, not stored in `settings.toml`.
- Custom AC values are written with `PowerWriteACValueIndex`; custom battery values are written with `PowerWriteDCValueIndex`.
- Active-plan AC and battery values are read back with `PowerReadACValueIndex` and `PowerReadDCValueIndex`.
- The UI exposes AC and battery core parking minimum cores, minimum processor performance, and maximum processor performance as free 0-100% values.
- Automatic active-plan refresh will not overwrite unsaved processor tuning edits for the same plan; use Refresh values to discard local edits and reload from Windows.
- Presets are quick-fill values only and fill both AC and battery: Performance uses 100/100/100, Balanced uses 50/5/100, and Saver uses 0/5/80.
- Maximum processor performance is normalized to be at least the configured minimum processor performance before writing.
- Do not add a "restore defaults" action unless it uses an explicit Windows default-restore path; OEM and plan defaults vary.

## Core Steering

`src/affinity/mod.rs` owns optional process affinity controls.

Current behavior:

- Settings live in `settings.cpu_affinity`.
- The feature is disabled by default.
- Core Steering activation and target changes apply only after Save; disabling Core Steering takes effect immediately.
- `BackgroundAutomation` runs the `CpuAffinityManager` every second while the app is visible or hidden to tray.
- Each rule can use hard affinity through `SetProcessAffinityMask`, soft CPU Sets through `SetProcessDefaultCpuSets`, or Efficiency Mode OFF through `SetProcessInformation(ProcessPowerThrottling)`.
- Hard mode preserves each process's previous affinity mask when possible, then restores that mask when the process is no longer targeted.
- Soft mode preserves each process's previous default CPU Set IDs when possible, then restores those IDs when the process is no longer targeted.
- Efficiency Mode OFF preserves each process's previous `PROCESS_POWER_THROTTLING_STATE` when possible, then restores that state when the process is no longer targeted.
- It detects multi-processor-group systems with `GetActiveProcessorGroupCount` and warns that hard affinity uses the process primary processor group.
- Built-in exclusions cover Windows shell/input/UWP lifecycle processes such as `explorer.exe`, `dwm.exe`, `searchapp.exe`, `searchhost.exe`, `systemsettings.exe`, and `textinputhost.exe`.

## App Suspension

`src/suspension/mod.rs` owns optional process suspension.
`src/ui/suspension_page.rs` owns the settings page.

Current behavior:

- Settings live in `settings.app_suspension`.
- The feature is disabled by default.
- App Suspension activation and target changes apply only after Save; disabling App Suspension takes effect immediately.
- `BackgroundAutomation` runs the `AppSuspensionManager` every second while the app is visible or hidden to tray.
- Only apps in `suspendable_apps` are candidates.
- A candidate must stay in the background for `background_delay_seconds` before suspension.
- The manager assigns each target process to a private Windows Job Object and freezes or thaws that job with `SetInformationJobObject`.
- It resumes the focused or clicked app's matching executable processes together, so multi-process apps such as browsers can recover without thawing unrelated apps.
- It pauses new suspension work if the foreground app or current Windows session cannot be identified, while preserving already suspended processes.
- It only targets processes in the current user session.
- It never targets the PowerLeaf process, current foreground process, or matching executable processes for the current foreground app.
- Taskbar and tray shell clicks temporarily thaw suspended top-level window owner processes only, which lets minimized and tray-hidden apps restore without thawing unrelated non-window worker processes. These shell-intent checks are rate-limited and do not extend an already active user-intent thaw.
- Built-in exclusions cover Windows shell/input/UWP lifecycle processes such as `explorer.exe`, `dwm.exe`, `searchapp.exe`, `searchhost.exe`, `systemsettings.exe`, and `textinputhost.exe`.
- Job-assignment failures include extra context when `IsProcessInJob` reports that the target process is already in a job object.
- Access-denied process opens are counted as skipped, not failed. This is expected for protected/elevated Windows processes.
- Suspendable Apps are edited with the same searchable running-app dropdown pattern as Foreground Detection and are persisted to TOML/INI.

Keep this feature opt-in and narrow. Do not add broad "suspend all background apps" behavior without explicit user direction and additional safeguards.

Security-hardening references:

- UWP lifecycle and Job Object abuse: https://www.orangecyberdefense.com/global/blog/threat/attack-technique-abuse-of-the-uwp-lifecycle-and-windows-job-objects
- Remote thread hijacking: https://www.ired.team/offensive-security/code-injection-process-injection/injecting-to-remote-process-via-thread-hijacking

## Foreground Detection

`src/ui/rules_page.rs` owns this page.

Current behavior:

- Has an `Enable foreground rules` toggle.
- When disabled, rule list controls are grayed out.
- Decision engine ignores foreground rules when disabled.
- Custom rules live in `settings.foreground_rules.rules`.
- Each foreground rule has a name, one process name, and one target power plan GUID.
- Rules are checked in list order. The first focused-app match wins.
- Legacy `whitelist` and `force_power_save` entries are migrated to custom rules by `Settings::fill_missing_power_plan_mappings()`.

Terminology:

- `whitelist` is still the internal config field for compatibility.
- `force_power_save` is still the internal legacy field for compatibility.
- UI text must describe custom foreground rules, not Force Active / Force Idle lists.

Dropdown behavior:

- Foreground rule app inputs are searchable running-app dropdowns.
- Running-app candidates refresh lazily while dropdowns are open, throttled by `PROCESS_REFRESH_INTERVAL` in `src/app.rs`.
- Click/focus opens the dropdown.
- Typing filters.
- Up/Down navigates.
- Enter or click fills the rule's focused app input with the highlighted app.
- Mouse hover only changes highlight when the pointer actually moves.
- Hover must not scroll or make the dropdown slide around.

Layout expectations:

- Foreground Detection rules should be rendered as compact rule cards, similar in spirit to Time Scheduler and CPU Load Detection.
- The target plan selector should allow any available Windows power plan, not only Idle or Active.

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
3. Plugged-in power-plan pause, only when enabled and Windows reports AC power.
4. Foreground rules, only when `foreground_rules.enabled`.
5. Time scheduler.
6. CPU Load Detection.
7. Action/activity mode.
8. Default Active plan.

The manual override sidebar UI was removed. The enum remains for backward compatibility with existing settings and INI/TOML imports.

## UI Guidance For Future Edits

- Keep operational controls in their relevant tabs.
- Keep plan mapping inside each Power Plan Controls page.
- Avoid reintroducing the removed Running Applications section.
- Avoid reintroducing `Add current app`.
- Avoid reintroducing manual pause controls in the sidebar.
- Use existing `egui` patterns and helpers rather than adding new UI frameworks.
- Keep controls compact and utilitarian.
