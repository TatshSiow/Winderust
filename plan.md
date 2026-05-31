# Windows Automatic Power Plan Switcher

> Legacy planning document.
>
> This file records the original product plan and may use older terms such as
> Power Save, Performance, whitelist, pause, and manual override. The current
> implemented app uses `Idle plan`, `Active plan`, `Action Based Scheduler`,
> `Time Based Scheduler`, and `Foreground Rules`.
>
> For the current implementation and future development guidance, use
> `DEVELOPMENT_GUIDE.md`. For user-facing instructions, use `README.md`.

## 1. Project Overview

This project is a Windows desktop application that automatically switches between user-selected power plans based on user activity, foreground applications, or scheduled time rules.

The application will be developed using:

* **Rust** for system-level reliability and performance
* **egui / eframe** for the desktop GUI
* **Windows APIs / `powercfg`** for power plan detection and switching

The terms **Power Save** and **Performance** are only logical labels inside the application. Users can map each label to any available Windows power plan.

---

## 2. Core Goals

* Automatically switch Windows power plans based on user behavior
* Support multiple switching modes
* Allow users to customize which Windows power plans are used
* Minimize background resource usage
* Provide a simple GUI for configuration
* Preserve manual user control when needed
* Avoid unnecessary power plan switching

---

## 3. Power Plan Mapping

The application should not hardcode specific Windows power plans.

Instead, it should detect available power plans from the system and allow users to assign them to logical roles.

Example:

```text
Logical Mode      Selected Windows Power Plan
-------------     ----------------------------
Power Save        Power saver
Performance       High performance
Balanced          Balanced
Custom 1          User-created power plan
```

Internally, the application should store the selected power plan GUIDs.

---

## 4. Switching Modes

## Mode 1: Activity-Based Switching

This mode switches power plans based on user activity.

### Behavior

* If no user input is detected for a configured duration, switch to the selected **Power Save** plan.
* If user input is detected again, switch to the selected **Performance** plan.

### Input Sources

The application should detect activity from:

* Mouse movement
* Mouse clicks
* Keyboard input
* Controller input
* Other supported Windows input events

### Configurable Options

```text
Idle timeout: 1 / 3 / 5 / 10 / custom minutes
Power Save plan: user-selected Windows power plan
Performance plan: user-selected Windows power plan
```

---

## 5. Foreground Application Rules

Mode 1 should support foreground application rules.

These rules override the normal idle/activity switching behavior.

---

### 5.1 Foreground Whitelist

Some applications should prevent automatic switching.

If a whitelisted application is currently in the foreground, the application should not change the power plan automatically.

### Example Use Cases

* Games
* Video editors
* 3D rendering software
* Benchmark tools
* Virtual machines
* Remote desktop sessions

### Example Behavior

```text
Current foreground app: blender.exe
Rule: Whitelisted
Result: Do not switch power plan automatically
```

---

### 5.2 Force Power Save Application List

Some applications should always trigger the selected **Power Save** plan when they are in the foreground.

### Example Use Cases

* Browser video playback
* E-book readers
* Music players
* Chat applications
* Note-taking apps

### Example Behavior

```text
Current foreground app: chrome.exe
Rule: Force Power Save
Result: Switch to Power Save plan
```

---

## 6. Mode 2: Schedule-Based Switching

This mode switches power plans based on a user-defined schedule.

### Behavior

* During configured schedule periods, switch to the selected **Power Save** plan.
* Outside configured schedule periods, switch to the selected **Performance** plan.

### Example Schedule

```text
Power Save period:
22:00 - 08:00

Outside this period:
Use Performance plan
```

### Configurable Options

```text
Start time
End time
Days of week
Power Save plan
Performance plan
```

---

## 7. Rule Priority

When multiple rules are active, the application should apply a clear priority order.

Suggested priority:

```text
1. Manual override
2. Foreground app force Power Save rule
3. Foreground app whitelist rule
4. Schedule-based rule
5. Activity-based idle rule
6. Default selected plan
```

---

## 8. Manual Override

Users should be able to temporarily disable automatic switching.

### Options

```text
Pause automation for 15 minutes
Pause automation for 30 minutes
Pause automation for 1 hour
Pause until next restart
Pause indefinitely
```

When automation is paused, the app should not change the Windows power plan.

---

## 9. Application Architecture

## 9.1 Suggested Modules

```text
src/
├── main.rs
├── app.rs
├── config/
│   ├── mod.rs
│   ├── settings.rs
│   └── storage.rs
├── power/
│   ├── mod.rs
│   ├── plan.rs
│   └── powercfg.rs
├── activity/
│   ├── mod.rs
│   ├── input_tracker.rs
│   └── idle_detector.rs
├── foreground/
│   ├── mod.rs
│   └── active_window.rs
├── scheduler/
│   ├── mod.rs
│   └── schedule_rule.rs
├── rules/
│   ├── mod.rs
│   └── decision_engine.rs
└── ui/
    ├── mod.rs
    ├── dashboard.rs
    ├── power_plan_page.rs
    ├── rules_page.rs
    └── schedule_page.rs
```

---

## 10. Main Components

## 10.1 Power Plan Manager

Responsibilities:

* List available Windows power plans
* Store power plan GUIDs
* Detect the currently active power plan
* Switch to a selected power plan

Possible implementation methods:

* Use `powercfg /list`
* Use `powercfg /getactivescheme`
* Use `powercfg /setactive <GUID>`

Later, this can be replaced or improved with direct Windows API usage.

---

## 10.2 Activity Tracker

Responsibilities:

* Detect last user input time
* Track whether the system is currently idle
* Detect input changes from mouse, keyboard, and controller
* Send activity state to the rule engine

Suggested state:

```text
Active
Idle
Unknown
```

---

## 10.3 Foreground Application Detector

Responsibilities:

* Detect the currently focused foreground window
* Extract process name
* Match process name against user-defined rules

Example detected process:

```text
chrome.exe
code.exe
blender.exe
eldenring.exe
```

---

## 10.4 Scheduler

Responsibilities:

* Store schedule rules
* Compare current time with configured schedule
* Return whether the current time is inside a Power Save period

Example rule:

```text
Days: Monday - Friday
Start: 22:00
End: 08:00
Target plan: Power Save
```

The scheduler should support overnight schedules.

Example:

```text
22:00 - 08:00
```

This means the schedule starts at night and ends the next morning.

---

## 10.5 Decision Engine

The decision engine decides which power plan should be active.

Input sources:

* Current user activity state
* Current foreground application
* Current schedule status
* Manual override status
* User configuration

Output:

```text
Target power plan GUID
Reason for decision
```

Example output:

```text
Target plan: Power Save
Reason: User has been idle for 5 minutes
```

---

## 11. Configuration Design

Suggested configuration format:

```toml
[general]
enabled = true
startup_with_windows = false
check_interval_ms = 1000

[power_plans]
power_save_guid = "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
performance_guid = "yyyyyyyy-yyyy-yyyy-yyyy-yyyyyyyyyyyy"

[activity_mode]
enabled = true
idle_timeout_seconds = 300

[foreground_rules]
whitelist = [
  "blender.exe",
  "davinciresolve.exe",
  "code.exe"
]

force_power_save = [
  "chrome.exe",
  "spotify.exe",
  "discord.exe"
]

[schedule_mode]
enabled = true

[[schedule_mode.rules]]
name = "Night Power Save"
days = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
start_time = "22:00"
end_time = "08:00"
power_save_guid = "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
performance_guid = "yyyyyyyy-yyyy-yyyy-yyyy-yyyyyyyyyyyy"
```

---

## 12. GUI Design

## 12.1 Main Dashboard

The dashboard should show:

```text
Current power plan
Current mode
Current detected foreground app
Current activity state
Last input time
Next scheduled switch
Automation status
```

Example:

```text
Current Plan: Power Saver
Mode: Activity-Based
State: Idle
Foreground App: chrome.exe
Reason: Idle for 5 minutes
```

---

## 12.2 Power Plan Settings Page

Features:

* Refresh available Windows power plans
* Select logical **Power Save** plan
* Select logical **Performance** plan
* Show currently active Windows power plan
* Test switch button

---

## 12.3 Activity Mode Page

Features:

* Enable or disable activity-based switching
* Set idle timeout
* Enable input detection options
* Configure behavior when activity resumes

---

## 12.4 Foreground Rules Page

Features:

* Add application to whitelist
* Add application to force Power Save list
* Remove application from rules
* Detect current foreground app and add it quickly
* Show rule conflict warnings

---

## 12.5 Schedule Mode Page

Features:

* Enable or disable schedule-based switching
* Add multiple schedule rules
* Select days of week
* Select start and end time
* Assign power plans for schedule periods
* Support overnight schedules

---

## 13. Background Service Behavior

The first version can run as a normal tray application.

Suggested behavior:

* Starts minimized to tray if configured
* Runs a lightweight loop every configured interval
* Checks foreground app, idle state, and schedule state
* Only switches power plans when the target plan changes
* Avoids repeatedly calling `powercfg` if the desired plan is already active

---

## 14. State Machine

Suggested states:

```text
Disabled
ManualOverride
ForegroundWhitelist
ForegroundForcePowerSave
ScheduledPowerSave
ScheduledPerformance
IdlePowerSave
ActivePerformance
```

Example transition:

```text
ActivePerformance
  -> no input for 5 minutes
IdlePowerSave

IdlePowerSave
  -> user moves mouse
ActivePerformance
```

---

## 15. MVP Scope

## Phase 1: Basic Power Plan Switching

* Detect available Windows power plans
* Allow user to select Power Save and Performance plans
* Switch plans manually from GUI
* Show current active power plan

## Phase 2: Activity-Based Switching

* Detect idle time
* Switch to Power Save after timeout
* Switch to Performance when activity resumes
* Add basic enable/disable toggle

## Phase 3: Foreground Application Rules

* Detect foreground process name
* Add whitelist
* Add force Power Save list
* Apply priority rules

## Phase 4: Schedule-Based Switching

* Add schedule rules
* Support day and time configuration
* Support overnight schedules
* Integrate schedule logic into decision engine

## Phase 5: Tray and Polish

* Add system tray support
* Add startup with Windows option
* Add manual override
* Add logging
* Add import/export configuration

---

## 16. Possible Rust Crates

Potential crates to evaluate:

```text
eframe / egui      GUI framework
serde              Serialization
toml               Config file format
chrono             Date and time handling
windows            Windows API bindings
sysinfo            Process information
tracing            Logging
tracing-subscriber Logging backend
directories        Config directory handling
```

Optional:

```text
gilrs              Controller input detection
tray-icon          System tray support
```

---

## 17. Logging

The application should keep lightweight logs for debugging.

Example log entries:

```text
[INFO] Current foreground app: chrome.exe
[INFO] User idle for 300 seconds
[INFO] Switching power plan to Power Save
[INFO] Skipped switch because target plan is already active
[WARN] Power plan GUID not found
```

---

## 18. Error Handling

The application should handle:

* Missing power plan GUID
* Deleted user-selected power plan
* Failed `powercfg` command
* Permission issues
* Unknown foreground process
* Invalid schedule configuration
* Conflicting foreground rules

The GUI should show clear error messages instead of silently failing.

---

## 19. Design Principles

* Avoid unnecessary background CPU usage
* Avoid aggressive polling where possible
* Avoid repeated power plan switching
* Keep user configuration explicit
* Make rule priority predictable
* Keep the MVP simple before adding advanced automation
* Allow users to fully customize which Windows power plans are used

---

## 20. Final Expected Behavior

The final application should allow users to configure automatic Windows power plan switching in two main ways:

1. **Activity Mode**

   * Active input means Performance plan
   * Idle state means Power Save plan
   * Foreground app rules can override this behavior

2. **Schedule Mode**

   * Specific time periods use Power Save plan
   * Outside those periods use Performance plan

Both modes should use user-selected Windows power plans instead of hardcoded Windows defaults.
