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

1. Open `Settings`.
2. In `Power Plan Mapping`, click `Refresh plans`.
3. Select an `Idle plan`.
4. Select an `Active plan`.
5. Keep `Enable automation` turned on.
6. Open `Action Based Scheduler` and choose the idle timeout you want.
7. Click `Save` in the Unsaved changes popup.

Changes take effect in the running app immediately. Use `Save` to keep those changes for the next launch, or `Cancel` to restore the last saved settings.

## Recommended Use

For most users:

- Use `Action Based Scheduler` for normal automatic switching.
- Use `Time Based Scheduler` when you want fixed work or quiet hours.
- Use `CPU Usage Scheduler` when CPU load should force Active or Idle mode.
- Use `Foreground Rules` for apps that should always force Active or Idle mode while focused.
- Keep plan selection in `Settings`; all schedulers use the same global Idle and Active plans.

## Pages

### Dashboard

Shows the current app state, current power plan, foreground app, activity state, next schedule, and current decision reason.
It also shows current total CPU usage after two CPU samples are collected.

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

Schedules use the global Idle/Active plans selected in Settings.

### CPU Usage Scheduler

Controls CPU usage-based switching.

Each rule has:

- Name
- CPU comparison
- Threshold percentage
- Duration
- Target plan role

Rules use the global Idle/Active plans selected in Settings. Rules are checked in list order, and the first rule whose CPU condition has held for its configured duration wins.

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
- `Power Plan Mapping`: select the global Idle and Active Windows power plans.
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
- The app runs without a console window.
- Some security tools may treat global input hooks cautiously. PowerLeaf uses them only to wake the app for activity checks; idle timeout still uses Windows idle-time polling.
