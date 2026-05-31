# PowerLeaf

PowerLeaf is a Windows app designed to make your device more eco-friendly.
It aims to reduce power usage and carbon emissions.

## Features

- Power plan scheduling based on:
    - Scheduled time
    - Input event triggers
    - CPU load

- Efficiency Mode:
    - Applies Windows EcoQoS to background user processes.
    - Automatically detects the foreground app and treats it differently.

- App Suspension:
    - Freezes selected background processes to 0% CPU usage while keeping them resumable when focused.

## Recommended Use

For most users:

- Use `Action Based Scheduler` for normal automatic switching.
- Use `Time Based Scheduler` when you want fixed work hours or quiet hours.
- Use `CPU usage-based Scheduler` when CPU load should force the Active or Idle plan.
- Use `Efficiency Mode` when background apps should run with Windows EcoQoS.
- Use `App Suspension` only for apps you explicitly trust to pause while in the background.
- Use `Foreground Rules` for apps that should always switch to a specific power plan while focused.
- Set power plans inside each Power Plan Controls tab you enable. Foreground Rules can target any available plan per rule.

## Build

Install Rust, then build from this folder:

```powershell
cargo build --release
```

The compiled executable is:

```text
target\release\powerleaf.exe
```

## Notes

- EcoQoS works best on Windows 11 and supported CPU platforms.
- Foreground rules have the highest scheduler priority and override other power plan schedulers.
