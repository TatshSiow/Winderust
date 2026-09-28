> [!IMPORTANT]
> Join the [Winderust Discord community](https://discord.com/invite/M7nctFZUxX).

> [!WARNING]  
> Alpha state, expects things to break,imperfect or not nicely handled.

# Winderust

Windows Performance & Power Manager. A system engine to improve your Windows experience.

The Windows desktop UI uses Iced with software rendering. Settings remain portable
beside the executable, with English and Traditional Chinese locales.

![Winderust Home dashboard](screenshots/Home.png)

## Download

[Download the latest release](https://github.com/TatshSiow/Winderust/releases).

## Features

- Adaptive Engine that automatically adjust background process resource
- Automatic power plan switching modules
- CPU/GPU/Memory/IO etc. Priority Control
- Smart memory trimming for background process
- iOS inspired background app suspension
- etc. 

## Documentation

- [Documentation index](docs/README.md)
- [Architecture](docs/architecture.md)
- [Adaptive Engine implementation](docs/adaptive-engine-implementation.md)
- [Release checklist](docs/release-checklist.md)

## Benchmark

Adaptive Engine benchmarks compare foreground responsiveness, retained background throughput,
and package power on specific hardware. See [`benchmark/`](benchmark/README.md) for reports and
methodology limits.

## Run/Build it yourself
- [Prerequisites](https://rustup.rs)

Debug build
```powershell
cargo run
```

Release build
```powershell
cargo build --release
```

The executable is written to `target\*`


## Diagnostic logs

When reporting a crash or shutdown failure, attach `winderust-diagnostics.log`
and `winderust-recovery.log` from beside the executable, plus any matching
`*.previous.log` files. Each log is limited to 1 MiB with one backup.
They record startup/version details, Rust panics with backtraces, and startup,
shutdown, and recovery failures. No upload occurs automatically.

Logs can contain process names and local paths; review them before sharing.
The executable directory must be writable. Forced termination, native crashes,
and power loss may leave no final entry. These diagnostics are separate from
the in-memory Action Log, which still requires manual CSV export.

## Contributions

Before contributing, run:

```powershell
cargo fmt -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

See 
- [CONTRIBUTING.md](CONTRIBUTING.md)
- [SECURITY.md](SECURITY.md).

## License

Copyright (C) 2026 Tatsh Siow.
Licensed under [GPL-3.0-only](LICENSE).
