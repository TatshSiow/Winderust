# Winderust

**Windows Performance & Power Manager**

Winderust balances Windows performance, heat, and power use by managing
background processes and automating power plans.

## Features

- **Workload Engine** protects the active app by reducing hot background work.
- **Background Efficiency** applies Windows Efficiency Mode / EcoQoS using
  protection lists and per-app rules.
- **Memory Trim** trims idle, high-memory background processes during memory
  pressure while protecting foreground and excluded apps.
- **Adaptive Engine** adjusts a temporary power plan to current CPU,
  foreground, and I/O demand, then restores the previous plan when stopped.
- **Power Automation** switches plans by foreground app, running app, CPU load,
  user activity, or schedule.
- **Advanced controls** cover priorities, CPU Sets and affinity, app suspension,
  timer resolution, and processor power settings.
- **Desktop integration** includes tray and startup support, settings
  import/export, themes, English and Traditional Chinese, and CSV action logs.

## Recommended Starting Points

- Use `Workload Engine` first when foreground interactivity is the goal.
- Use `Background Efficiency` for low-risk battery and heat reduction.
- Use `Memory Trim` for memory-pressure cleanup without trimming the foreground app.
- Use `Power Automation` when workloads need different power plans.
- Use advanced controls only when per-app automation is not enough.

## Workload Engine Benchmark

Machine-specific results and the run command are kept in
[`benchmark/`](benchmark/README.md).

## Build

Install Rust, then build from this folder:

```powershell
cargo build --release
```

The executable is written to:

```text
target\release\winderust.exe
```

## License

Copyright (C) 2026 Tatsh

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Affero General Public License as published
by the Free Software Foundation, version 3.

SPDX-License-Identifier: AGPL-3.0-only

Licensed under the [GNU General Public License v3.0](LICENSE).

## Notes
- EcoQoS works best on Windows 11 and supported CPU platforms.
- Development guidance: [AGENTS.md](AGENTS.md) and
  [agent memory](.agents/memory/README.md).
