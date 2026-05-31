# PowerLeaf

Make your Windows more eco-friendly.

PowerLeaf is a Windows-only desktop app for automatically switching Windows power plans.

It maps two logical modes, `Idle` and `Active`, to Windows power plans and switches between them using activity, schedule, and foreground-app rules.

## Features

- Detects and switches Windows power plans through Windows power APIs.
- Maps global `Idle plan` and `Active plan` in Settings.
- Action Based Scheduler switches by keyboard/mouse activity and idle timeout.
- Hybrid input detection uses Windows input hooks for faster active-resume checks, while polling still handles idle timeout.
- Time Based Scheduler switches by day and time ranges.
- CPU Usage Scheduler switches by custom CPU threshold rules.
- Efficiency Mode applies Windows EcoQoS to background user processes.
- App Suspension can suspend selected background apps after a delay.
- Foreground Rules can force Active or Idle plan for selected focused apps.
- Foreground rule inputs can search running apps with dropdown, arrow-key navigation, and Enter selection.
- Optional hide-to-system-tray behavior.
- Import and export settings as `.ini` files through native Windows file dialogs.
- Unsaved settings show a bottom popup with Save and Cancel actions.

## Build

Install Rust, then build from this folder:

```powershell
cargo build --release
```

The compiled executable is:

```text
target\release\powerleaf.exe
```

Run it from PowerShell or File Explorer like any other Windows application. The release build does not open a console window.

If the release executable is already running and locked, stop the app first or build to another target directory:

```powershell
cargo build --release --target-dir target-next
```

## Basic Setup

1. Open `Power Plan Mapping`.
2. Click `Refresh plans`.
3. Select an `Idle plan`.
4. Select an `Active plan`.
5. Open `Settings` and keep `Enable automation` turned on.
6. Open `Action Based Scheduler` and choose the idle timeout you want.
7. Click `Save` in the Unsaved changes popup.

Most changes take effect in the running app immediately. Efficiency Mode and App Suspension activation and target changes apply after `Save`. Use `Save` to keep changes for the next launch, or `Cancel` to restore the last saved settings.

## Recommended Use

For most users:

- Use `Action Based Scheduler` for normal automatic switching.
- Use `Time Based Scheduler` when you want fixed work or quiet hours.
- Use `CPU Usage Scheduler` when CPU load should force Active or Idle mode.
- Use `Efficiency Mode` when background apps should run with Windows EcoQoS.
- Use `App Suspension` only for apps you explicitly trust to pause while in the background.
- Use `Foreground Rules` for apps that should always force Active or Idle mode while focused.
- Keep plan selection in `Power Plan Mapping`; all schedulers use the same global Idle and Active plans.

## Pages

### Dashboard

Shows the current app state, current power plan, foreground app, activity state, Efficiency Mode state, App Suspension state, next schedule, and current decision reason.
It also shows current total CPU usage after two CPU samples are collected.

### Power Plan Mapping

Controls the global Windows power plans used by PowerLeaf.

- `Refresh plans`: reloads available Windows power plans.
- `Idle plan`: the Windows power plan used when PowerLeaf decides the PC is idle.
- `Active plan`: the Windows power plan used when PowerLeaf decides the PC is active.

Action Based Scheduler, Time Based Scheduler, CPU Usage Scheduler, and Foreground Rules all use these same global plan mappings.

### Action Based Scheduler

Controls activity-based switching.

- `Enable action-based switching`: turns activity-based decisions on or off.
- `Keyboard input`: keyboard activity can trigger Active mode.
- `Mouse input`: mouse activity can trigger Active mode.
- `Idle timeout`: how long user input must be idle before switching to Idle.
- `Check interval`: fallback polling interval for activity, foreground rules, and schedule checks.

At least one input type remains enabled.

### Time Based Scheduler

Controls schedule-based switching.

Each rule has:

- Name
- Days
- Start time
- End time

Schedules use the global Idle/Active plans selected in Power Plan Mapping.

### CPU Usage Scheduler

Controls CPU usage-based switching.

Each rule has:

- Name
- CPU comparison
- Threshold percentage
- Duration
- Target plan role

Rules use the global Idle/Active plans selected in Power Plan Mapping. Rules are checked in list order, and the first rule whose CPU condition has held for its configured duration wins.

### Efficiency Mode

Controls Windows EcoQoS for background user processes.

- `Enable Windows EcoQoS`: applies EcoQoS to background processes in the current user session.
- `Exclude foreground app`: keeps the currently focused app, and matching same-name app processes, out of Efficiency Mode.
- PowerLeaf, protected/elevated processes, system-session processes, and built-in Windows shell/input processes are not throttled.
- `Efficiency Whitelist`: apps that should never be throttled by PowerLeaf.
- EcoQoS activation and target changes are applied after `Save`; disabling EcoQoS stops it immediately.

PowerLeaf applies the same two-part behavior used by Windows Task Manager Efficiency Mode: EcoQoS plus idle process priority. It preserves each process's previous power-throttling state and priority when possible, then restores them when the process is no longer a target, when Efficiency Mode is disabled, or when PowerLeaf exits.

### App Suspension

Controls optional suspension for selected background desktop apps.

- `Enable app suspension`: turns suspension on or off.
- `Background delay`: how long an app must stay in the background before PowerLeaf suspends it.
- `Suspendable Apps`: apps that are allowed to be suspended by PowerLeaf.
- Foreground apps, same-name foreground app processes, PowerLeaf, system-session processes, and built-in Windows shell/input processes are not suspended.
- App Suspension activation and target changes are applied after `Save`; disabling App Suspension resumes suspended apps immediately.

Suspended apps are resumed when they become foreground, leave the suspendable app list, App Suspension is disabled, automation is disabled, or PowerLeaf exits. This is more aggressive than Efficiency Mode, so keep the list narrow.

### Foreground Rules

Controls focused-app overrides.

- `Enable foreground rules`: turns foreground rules on or off.
- `Force Active Plan`: apps that should force the Active plan when focused.
- `Force Idle Plan`: apps that should force the Idle plan when focused.

The app dropdowns list currently running processes. Apps already added to either list are hidden from both dropdowns to avoid duplicates.

If an app somehow exists in both lists, Force Idle wins.

### Settings

Controls global app settings.

- `Enable automation`: master switch for automatic power-plan changes.
- `Hide to system tray on close`: closing the window keeps PowerLeaf running in the tray.
- `Settings Files`: export or import all settings as `.ini`.

### About

Shows the PowerLeaf brand name, description, and version.

## Settings Files

PowerLeaf stores saved app settings in the user config directory as TOML.
If an older PowerSwitcher settings file exists and no PowerLeaf settings file exists yet, PowerLeaf loads the older settings and saves future changes under the PowerLeaf config folder.

The Settings tab can also:

- Export settings to a chosen `.ini` file.
- Import settings from a chosen `.ini` file.

Imported settings are applied immediately and saved to the normal app config.

## System Tray

When `Hide to system tray on close` is enabled:

- Closing the window hides it instead of exiting.
- The tray menu can show PowerLeaf again.
- The tray menu can quit the app.

## Notes

- This app is Windows-only.
- EcoQoS works best on Windows 11 and supported CPU platforms.
- The app runs without a console window.
- Some security tools may treat global input hooks cautiously. PowerLeaf uses them only to wake the app for activity checks; idle timeout still uses Windows idle-time polling.
