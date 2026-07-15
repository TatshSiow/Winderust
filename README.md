# Winderust

Winderust is a Rust-based Windows tuning controller.

Naming: **Wanderlust** + **Windows Derust**

Definition: Wander and explore, polish your rusty Windows and shine.

## Features

### Winderust Features
- `Workload Engine`: protects the active app by temporarily lowering hot background work. It can tune process priority, efficiency mode, I/O priority, thread priority, dynamic boost, GPU priority, memory priority, and CPU affinity escalation.
- `Background Efficiency`: applies Windows Efficiency Mode / EcoQoS to eligible background apps with protection lists and per-app rules.
- `Memory Trim`: trims idle high-memory background processes during memory pressure while protecting foreground and excluded apps.

### Power Automation
- `Adaptive Engine` activates a temporary `PowerLeaf Adaptive` plan and scales core parking, processor minimum/maximum state, boost policy, and boost mode from burst, peak-core, total-load, foreground, and I/O demand. The previous plan is restored when Adaptive Engine stops.
- Switches power plans by foreground app, running app, CPU load, user activity, and schedule.
- Supports per-rule plans plus processor power tuning such as core parking, CPU limits, and boost mode.
- Applies rules in foreground > running app > CPU load > activity > schedule order.

### Priority Control
- Manages process priority, thread priority, dynamic priority boost, I/O priority, GPU scheduler priority, and memory priority.
- Keeps risky High/Realtime-style controls behind explicit advanced/experimental flows.
- Provides foreground/background defaults plus app exclusions where supported.

### Processor Controls
- Limits selected background apps after sustained CPU load.
- Restricts broad background CPU access when foreground/system pressure warrants it.
- Steers selected apps with soft CPU Sets or hard affinity.

### Process View, Advanced, And App
- Process List shows live processes and their active Winderust policies in one place.
- App Suspension freezes explicit opt-in background apps and resumes them when needed.
- Timer Resolution and Win32PrioritySeparation controls cover advanced scheduler tuning.
- GPUI desktop interface includes tray support, startup options, import/export settings, themes, accent colors, animation preference, English / Traditional Chinese localization, and CSV Action Log export.

## Recommended Starting Points

- Use `Workload Engine` first when foreground interactivity is the goal.
- Use `Background Efficiency` for low-risk battery and heat reduction.
- Use `Memory Trim` for memory-pressure cleanup without trimming the foreground app.
- Use `Power Automation` when a workload should choose or hold a specific power plan.
- Use `Priority Control` or `Processor Controls` only when you need per-subsystem or per-app tuning beyond Workload Engine.

## Workload Engine Benchmark

Machine-specific results and the run command are kept in
[`benchmark/`](benchmark/README.md).

## Build

Install Rust, then build from this folder:

```powershell
cargo build --release
```

The compiled executable is:

```text
target\release\winderust.exe
```

## License

Winderust is proprietary software. No open-source license is granted. See
`EULA.md`.

## Notes
- EcoQoS works best on Windows 11 and supported CPU platforms.
- Agent docs, development guide, scope, and references: `.agents/memory/README.md`.
