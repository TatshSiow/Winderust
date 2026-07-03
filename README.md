# Winderust

Winderust is a Rust-based Windows tuning controller.

Naming: **Wanderlust** + **Windows Derust**

Definition: Wander and explore, polish your rusty Windows and shine.

## Features

### Winderust Features
- `Auto Balance`: protects the active app by temporarily lowering hot background work. It can tune process priority, efficiency mode, I/O priority, thread priority, dynamic boost, GPU priority, memory priority, and CPU affinity escalation.
- `Background Efficiency`: applies Windows Efficiency Mode / EcoQoS to eligible background apps with protection lists and per-app rules.
- `Smart Trim`: trims idle high-memory background processes during memory pressure while protecting foreground and excluded apps.

### Power Automation
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

- Use `Auto Balance` first when foreground responsiveness is the goal.
- Use `Background Efficiency` for low-risk battery and heat reduction.
- Use `Smart Trim` for memory-pressure cleanup without trimming the foreground app.
- Use `Power Automation` when a workload should choose or hold a specific power plan.
- Use `Priority Control` or `Processor Controls` only when you need per-subsystem or per-app tuning beyond Auto Balance.

## Auto Balance Benchmark

`Off` is the comparison baseline under generated background load; the script
also emits a no-background `baseline_no_background_load` case for reference.
Foreground First and Max Foreground apply the full priority-assist set where
Windows exposes it to the benchmark: priority boost, thread priority, memory
priority, I/O priority, and GPU priority. Low Impact keeps the extra I/O,
memory, and GPU assists off.

Metrics:

- Foreground latency change: foreground work time delta vs `Off`; negative
  milliseconds and positive percent mean faster.
- P95 foreground latency improvement: near-worst latency vs `Off`; higher is better.
- Background throughput vs Off: generated background-worker CPU time vs `Off`.
  Lower means the preset protected foreground work by taking CPU away from
  background workers; above `100%` means background workers got more CPU time.

The README tables compare each preset directly to the displayed `Off` row. The
benchmark script still records adjacent paired-Off comparisons in
`docs/auto-balance-benchmark.md` for deeper validation.

Latest CPU-loop validation on Intel Core 5 210H, 12 logical processors:

| Case | Avg latency vs Off | Median latency vs Off | P95 latency vs Off | Background throughput vs Off | Repeat passes won |
| --- | ---: | ---: | ---: | ---: | ---: |
| Off | 679.10 ms | 616.47 ms | 682.97 ms | 100.0% | baseline |
| Low Impact | 348.20 ms (+48.7%) | 326.29 ms (+47.1%) | 364.75 ms (+46.6%) | 124.4% | 2/3 |
| Foreground First | 202.27 ms (+70.2%) | 200.82 ms (+67.4%) | 201.14 ms (+70.5%) | 33.6% | 3/3 |
| Max Foreground | 209.11 ms (+69.2%) | 207.01 ms (+66.4%) | 210.01 ms (+69.3%) | 16.5% | 3/3 |

Run the benchmark from the repository root:

```powershell
.\scripts\auto_balance_benchmark.ps1 -Passes 3 -Rounds 5 -Iterations 1000000
```

The script spawns temporary CPU workers and changes their priority, affinity,
thread, memory, I/O, priority-boost, and GPU scheduling controls where possible.
Treat results as local direction only. I/O-loop and Winderust-launch scenarios
and the message-loop scenario are documented in `docs/auto-balance-benchmark.md`.

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
- Agent docs, development guide, scope, and references: `.agents/memory/README.md`.
