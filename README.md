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
- Manages I/O priority and GPU scheduler priority.
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

## Auto Balance Benchmark

Latest paired synthetic benchmark on AMD Ryzen 7 7735HS, 16 logical processors.
`Off` is the comparison baseline under generated background load; the script
also emits a no-background `baseline_no_background_load` case for reference.
Balance and Responsive now apply the extra priority assists the app preset uses
where Windows exposes them to the benchmark: priority boost, thread priority,
memory priority, I/O priority, and GPU priority.

Metrics:

- Foreground latency improvement: lower foreground work time vs `Off`; higher is better.
- P95 foreground latency improvement: near-worst latency vs `Off`; higher is better.
- Background retained: background CPU capacity kept vs `Off`; higher means less background sacrifice.
- Agreement: share of passes where median and P95 both beat `Off` by at least 3%.
- Signal: confidence label from repeated passes; `strong` is trustworthy, `noisy` is not.
- Tradeoff: background throughput cost; `high` means the preset buys latency by giving up background work.

| Case | Median foreground latency avg | Median foreground latency worst pass | P95 foreground latency avg | P95 foreground latency worst pass | Background CPU work kept avg | Background CPU work kept worst pass | Agreement | Signal | Tradeoff |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |
| Off baseline | 0.0% | 0.0% | 0.0% | 0.0% | 100.0% | 100.0% | 100.0% | baseline | baseline |
| Gentle | 48.3% | 45.0% | 49.2% | 46.9% | 83.7% | 83.2% | 100.0% | strong | moderate |
| Balance | 46.8% | 45.3% | 45.2% | 44.5% | 66.0% | 65.6% | 100.0% | strong | moderate |
| Responsive | 47.2% | 40.9% | 48.7% | 45.3% | 24.7% | 24.5% | 100.0% | strong | high |

Run the benchmark from the repository root:

```powershell
.\scripts\auto_balance_benchmark.ps1 -Passes 3 -Rounds 5 -Iterations 1000000
```

Optional Task Manager launch scenario:

```powershell
.\scripts\auto_balance_benchmark.ps1 -ForegroundScenario TaskManagerLaunch -Passes 3 -Rounds 3 -WorkerSeconds 20
```

Optional Winderust launch scenario:

```powershell
.\scripts\auto_balance_benchmark.ps1 -ForegroundScenario WinderustLaunch -Passes 3 -Rounds 3 -WorkerSeconds 20
```

Latest Winderust launch result on the same device after launch-grace tuning:

| Case | Median launch latency avg | Median launch latency worst pass | P95 launch latency avg | P95 launch latency worst pass | Background CPU work kept avg | Agreement | Signal | Tradeoff |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |
| Gentle | 3.8% | 0.0% | 3.8% | 0.0% | 99.8% | 33.3% | noisy | low |
| Balance | -4.1% | -11.3% | -4.1% | -11.3% | 99.7% | 0.0% | noisy | low |
| Responsive | -4.8% | -12.5% | -4.8% | -12.5% | 99.9% | 33.3% | noisy | low |

Launch-grace tuning keeps background restraints deferred while the app starts;
the launch result remains noisy and does not validate stronger launch behavior.

The script spawns temporary CPU workers and changes their priority, affinity,
thread, memory, I/O, priority-boost, and GPU scheduling controls where possible.
Treat results as local direction only; validate on more hardware before changing
global preset defaults. Full guide: `docs/auto-balance-benchmark.md`.

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
- Agent docs, development guide, scope, and references: `.agents/memory/README.md`.
