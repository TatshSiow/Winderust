# Winderust

Winderust is a Rust-based Windows tuning controller.

Naming: **Wanderlust** + **Windows Derust**

Definition: Wander and explore, polish your rusty Windows and shine.

## Features

### Power Plan Control
- Switches Windows power plans by foreground app, running app, CPU load, activity, and time.
- Supports per-rule target plans and processor power tuning for core parking, CPU limits, and boost mode.
- Prioritizes foreground rules, then running-app rules, CPU load, activity, and time.

### Auto Balance
- Protects the foreground app by restraining hot background processes.
- Can lower background process priority, I/O priority, memory priority, and CPU access.
- Supports foreground boost, launch boost, cooldowns, and app exclusions.

### Process List
- Shows running processes and their configured Winderust policies in one place.
- Surfaces per-process power-plan, efficiency, limiter, priority, suspension, timer, and steering state.

### Process Control
- Applies Background Efficiency / Windows EcoQoS to eligible background apps.
- Manages I/O priority, GPU scheduler priority, launch priority, and Watchdog terminate/restart rules.
- Keeps dangerous automatic High/Realtime priority controls out of normal workflows.

### CPU Control
- Limits selected background apps when sustained CPU load crosses a threshold.
- Restricts background CPU access globally or by rule.
- Steers selected apps to preferred logical CPUs with soft CPU Sets or hard affinity.

### RAM Control
- Applies process memory-priority defaults for foreground and background apps.
- Uses Smart Trim to trim idle high-memory background processes under memory pressure.

### Advanced Controls
- Suspends explicit opt-in background apps and resumes them when needed.
- Requests timer resolution only while matching foreground apps are active.
- Exposes Win32PrioritySeparation tuning with backup and restore controls.

### App Experience
- GPUI desktop interface with tray support, startup options, import/export settings, themes, accent colors, animation preference, and English / Traditional Chinese localization.
- Action Log records recent automation and process-control decisions with CSV export.

## Recommended Usage Scenario

### Power plan controls
- `By Foreground`
    - For apps that should immediately choose a specific power plan.

- `By Running App`
    - For workloads that should hold a performance plan while the app is open.

- `By Time`
    - For working and sleep hours.

- `By CPU Load`
    - For detecting heavy or light workloads.

- `By Activity`
    - For keyboard, mouse, and controller idle/active switching.
 
### Process Controls

- `Auto Balance`
    - You want foreground apps to stay responsive while background work is restrained.

- `Background Efficiency`
    - You want to save a little battery but don't want to hurt daily use case.

- `Core Limiter`
    - You want selected background apps capped only after sustained high CPU use.

- `Core Steering`
    - You want selected background apps kept on preferred logical CPUs.

- `Smart Trim`
    - You want memory pressure cleanup without trimming the foreground app.
    
- `App Suspension`
    - Freeze explicit opt-in background apps to squeeze performance and battery.

## Build

Install Rust, then build from this folder:

```powershell
cargo build --release
```

The compiled executable is:

```text
target\release\winderust.exe
```

## Notes

- EcoQoS works best on Windows 11 and supported CPU platforms.
- Foreground rules have the highest scheduler priority and override other power plan rules.
- Development guide: `DEVELOPMENT_GUIDE.md`.
- Product scope and future goals: `PROJECT_SCOPE.md`.
