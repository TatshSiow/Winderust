# PowerLeaf

PowerLeaf is a rust based Windows app designed to make your device more eco-friendly.

It aims to reduce power usage and carbon emissions.

## Features

### Power Plan scheduler adjust power plan based on
- Time rules
- Input event triggers
- CPU load

### Efficiency Mode
- Applies Windows EcoQoS to background user processes.
- Automatically detects the foreground app and treats it differently.

### App Suspension
- Freezes selected background processes to 0% CPU usage while keeping them resumable when focused.

## Recommended Usage Scenario

### Power plan controls
- `Action Detection`
    - For normal automatic switching.

- `Time Scheduler`
    - For working and sleep hours.

- `CPU Load Detection`
    - For detecting heavy or light workloads.
 
### Process Controls

- `Efficiency Mode`
    - You want to save a little battery but don't want to hurt daily use case.
    - **ONLY** For Windows EcoQoS supported system.
    
- `App Suspension`
    - Freeze background app to squeeze the performance and battery.

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
- Foreground rules have the highest scheduler priority and override other power plan rules.
