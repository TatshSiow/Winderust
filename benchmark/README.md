# Benchmarks

Adaptive Engine benchmark results are stored separately from the project README
because they are specific to the machine and its current thermal and power state.

- [Intel Core 5 210H](intel-core-5-210h.md) - 2026-07-15 preset benchmark
- [Intel Core 5 210H adaptive runtime](intel-core-5-210h-adaptive-runtime.md) - 2026-07-15 real runtime A/B
- [Intel Core 5 210H system and preset diagnostics](intel-core-5-210h-system-and-preset-diagnostics.md) - 2026-08-06 live-system, scheduler, compute, memory, and package-power measurements
- [Intel Core 5 210H counterbalanced Adaptive runtime matrix](intel-core-5-210h-adaptive-runtime-counterbalanced-20260807.md) - 2026-08-07 and 2026-08-10 validated CPU, I/O, and MessageLoop runtime results with foreground-host integrity, package power, and Windows priority activation
- [AMD Ryzen 7 7735HS](amd-ryzen-7-7735hs.md) - preset benchmark

Run from the repository root:

```powershell
.\scripts\workload_engine_benchmark.ps1 -Passes 3 -Rounds 5 -Iterations 1000000
```

For a real Adaptive Engine comparison against stock Windows Balanced:

```powershell
.\scripts\adaptive_runtime_benchmark.ps1 -Passes 4 -Rounds 5 -Iterations 1000000 -WorkerSeconds 180
```

See [the benchmark guide](../docs/adaptive-engine-benchmark.md) for methodology,
metrics, and additional scenarios.
