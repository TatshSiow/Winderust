# Benchmarks

Workload Engine benchmark results are stored separately from the project README
because they are specific to the machine and its current thermal and power state.

- [Intel Core 5 210H](intel-core-5-210h.md) - 2026-07-15 preset benchmark
- [Intel Core 5 210H adaptive runtime](intel-core-5-210h-adaptive-runtime.md) - 2026-07-15 real runtime A/B
- [AMD Ryzen 7 7735HS](amd-ryzen-7-7735hs.md) - previous README result

Run from the repository root:

```powershell
.\scripts\workload_engine_benchmark.ps1 -Passes 3 -Rounds 5 -Iterations 1000000
```

For a real Adaptive Engine comparison against stock Windows Balanced:

```powershell
.\scripts\adaptive_runtime_benchmark.ps1 -Passes 3 -Rounds 5 -Iterations 1000000 -WorkerSeconds 45
```

See [the benchmark guide](../docs/workload-engine-benchmark.md) for methodology,
metrics, and additional scenarios.
