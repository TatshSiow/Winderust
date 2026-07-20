# Winderust

**Windows Performance & Power Manager**

Winderust balances Windows performance, heat, and power use by managing
background processes and automating power plans.

> **Public alpha:** Winderust is pre-release software. Settings and behavior may
> change before the first stable release.

## Screenshots

### Home

![Winderust Home dashboard](screenshots/Home.png)

### Adaptive Engine

![Winderust Adaptive Engine settings](<screenshots/Adaptive Engine.png>)

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

## Platform and Safety

- Windows 11 is the primary tested platform. Some features may work on Windows
  10, but it is not currently release-tested.
- Administrator access is required for some process and power controls.
- Winderust changes process state and Windows power settings. It restores
  temporary managed state when it can, but a crash or power loss can prevent
  cleanup.
- Settings and action logs remain local unless you explicitly export them.
- Release binaries are currently unsigned, so Windows may show a warning.

Start with Background Efficiency or Workload Engine, and enable advanced
controls one at a time.

## Build and Test

Install stable Rust with the MSVC toolchain, Visual Studio Build Tools with C++
support, and a Windows SDK that includes fxc.exe. Then run:

    .\scripts\build_release.cmd

The executable is written to:

    target\release\winderust.exe

Before contributing, run:

    cargo fmt -- --check
    cargo clippy --locked --all-targets -- -D warnings
    cargo test --locked

See [CONTRIBUTING.md](CONTRIBUTING.md) for development guidance and
[SECURITY.md](SECURITY.md) for vulnerability reporting.

## License

Copyright (C) 2026 Tatsh Siow

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by the Free
Software Foundation, version 3 of the License.

Licensed under the [GNU General Public License v3.0](LICENSE).

SPDX-License-Identifier: GPL-3.0-only
