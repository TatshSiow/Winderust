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

`Off` is the comparison baseline under generated background load; the script
also emits a no-background `baseline_no_background_load` case for reference.
Adaptive Engine preset cases combine processor power policy with Workload
Engine scheduling plus process, IO, memory, GPU/thread, priority-boost, and
background-efficiency controls. Performance uses Foreground First scheduling,
while Speed uses Max Foreground scheduling.

Metrics:

- Foreground latency change: foreground work time delta vs `Off`; negative
  milliseconds and positive percent mean faster.
- P95 foreground latency improvement: near-worst latency vs `Off`; higher is better.
- Foreground throughput vs Off: foreground iterations per second vs `Off`;
  higher is better.
- Background latency vs Off: estimated fixed-background-work slowdown from
  measured retained background CPU throughput; lower is better.
- Package power vs Off: RAPL package-power delta vs `Off`; lower is better.

The README table compares each preset directly to the displayed `Off` row. The
benchmark script still records background retention and adjacent paired-Off comparisons in
`docs/workload-engine-benchmark.md` for deeper validation.

Latest Adaptive Engine preset CPU-loop validation on AMD Ryzen 7 7735HS, 16
logical processors, with RAPL package-power sampling:

| Case | Median latency vs Off | P95 latency vs Off | Foreground throughput vs Off | Background latency vs Off | Package power vs Off | Repeat passes won |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Off | 228.67 ms (baseline) | 239.84 ms (baseline) | 4,540,008 iter/s (baseline) | 1.00x (baseline) | 63.90 W (baseline) | baseline |
| Powersave | 377.30 ms (-70.6%) | 378.55 ms (-65.1%) | 2,643,422 iter/s (-41.8%) | 1.21x (+20.9%) | 10.97 W (-82.8%) | 0/3 |
| Balanced | 181.46 ms (+24.2%) | 187.33 ms (+24.7%) | 5,423,882 iter/s (+19.5%) | 1.20x (+20.3%) | 21.16 W (-66.9%) | 3/3 |
| Performance | 123.99 ms (+47.1%) | 125.15 ms (+50.4%) | 8,033,565 iter/s (+76.9%) | 1.50x (+50.2%) | 57.91 W (-9.4%) | 3/3 |
| Speed | 119.07 ms (+44.7%) | 119.35 ms (+47.6%) | 8,410,122 iter/s (+85.2%) | 12.05x (+1,105.4%) | 22.65 W (-64.5%) | 3/3 |

Score component ratios from the same run, all vs paired `Off`:

| Case | Int arithmetic | Double arithmetic | Float batch | GZip | Deflate | SHA-256 | AES-CBC | L2 scan | Memory copy |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Off | 3,026.5 Mops (100.0%) | 863.5 Mops (100.0%) | 5,515.4 Mops (100.0%) | 59.2 MB/s (100.0%) | 48.9 MB/s (100.0%) | 1,749.3 MB/s (100.0%) | 836.8 MB/s (100.0%) | 2,245.0 MB/s (100.0%) | 8,517.6 MB/s (100.0%) |
| Powersave | 1,097.8 Mops (36.4%) | 287.8 Mops (33.2%) | 1,889.7 Mops (34.3%) | 25.2 MB/s (41.5%) | 22.5 MB/s (45.7%) | 637.4 MB/s (38.8%) | 532.8 MB/s (130.4%) | 1,337.8 MB/s (49.3%) | 766.4 MB/s (50.6%) |
| Balanced | 2,333.6 Mops (84.0%) | 606.5 Mops (70.8%) | 3,982.0 Mops (72.1%) | 50.9 MB/s (94.1%) | 44.3 MB/s (94.3%) | 1,335.2 MB/s (75.5%) | 751.8 MB/s (102.2%) | 2,646.3 MB/s (144.5%) | 12,455.8 MB/s (96.0%) |
| Performance | 2,956.3 Mops (94.4%) | 867.6 Mops (100.0%) | 5,363.7 Mops (97.5%) | 77.7 MB/s (132.9%) | 60.9 MB/s (119.2%) | 1,759.0 MB/s (98.5%) | 877.9 MB/s (115.9%) | 4,172.6 MB/s (198.6%) | 15,197.6 MB/s (117.0%) |
| Speed | 3,505.4 Mops (111.7%) | 962.7 Mops (111.6%) | 5,975.3 Mops (108.1%) | 81.4 MB/s (143.0%) | 66.0 MB/s (142.0%) | 2,015.6 MB/s (113.9%) | 1,106.4 MB/s (138.3%) | 4,346.6 MB/s (220.5%) | 16,687.0 MB/s (122.8%) |

Run the benchmark from the repository root:

```powershell
.\scripts\workload_engine_benchmark.ps1 -Passes 3 -Rounds 5 -Iterations 1000000
```

The script spawns temporary CPU workers and changes their priority, affinity,
thread, memory, I/O, priority-boost, and GPU scheduling controls where possible.
Treat results as local direction only. I/O-loop and Winderust-launch scenarios
and the message-loop scenario are documented in `docs/workload-engine-benchmark.md`.

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
