# Benchmarks

For isolated single-thread/multithread throughput comparisons, run in **64-bit
Windows PowerShell 5.1**:

```powershell
.\scripts\cpu_throughput_benchmark.ps1 -Label 'Default' -Rounds 5
.\scripts\cpu_throughput_benchmark.ps1 -Label 'Speed-thread-default-power-off' -Rounds 5
```

Apply and save each configuration manually before its run. The script does not
modify Winderust settings, power plans, affinity, or priorities. Its compiled
integer workload reports blocks/second, not CPU-Z scores; it does not reproduce
CPU-Z's instruction mix. A dedicated foreground window avoids classifying a
terminal-hosted benchmark as background work. Keep it focused; focus loss is
sampled during measurement and flagged in CSV, and flagged rows are excluded
from the printed average. Short focus changes between samples may be missed.

Each phase warms the same worker threads before a synchronized measurement.
MT uses the .NET-visible logical processor count by default (`-Threads` overrides
it); machines above 64 logical processors are unsupported. ST/MT order alternates.
CSV includes raw scores, elapsed time, CPU/runtime identity, script hash, and final
process priority. The label is your description, not verification of applied
settings. Save settings and Winderust build information alongside the results.

For comparisons, prefer alternating one-round invocations (`-Rounds 1`) in
A/B/B/A order. Keep AC power, cooling, Winderust visibility, other apps, and all
parameters constant. Fixed work and timing cannot fix thermal state or background
interference. Default measurements take roughly four minutes per configuration.
Use `-SelfTest -Label test` for a short kernel and ST/MT timing smoke test.

To automate configuration changes, build the opt-in runtime host, close Winderust,
and run the matrix from an elevated Windows PowerShell:

```powershell
cargo build --locked --release --features runtime-benchmark
.\scripts\adaptive_throughput_benchmark.ps1
```

This uses the production Speed preset function and automation/recovery controllers
without the UI. It never reads or writes your saved settings. The default matrix
rotates Default, Speed, SpeedThreadDefault, and SpeedThreadDefaultPowerOff over four
passes. Default means automation disabled on the current Windows power plan, not
an automatic switch to Balanced. Each case must restore that original plan and
complete runtime/watchdog shutdown before the next case. Failures stop the matrix;
the harness never force-kills the runtime or deletes a plan to hide a failed restore.

CPU Pressure Restraint stays at its fresh-settings value (off) in those cases.
Its Thread Priority helper is pressure-gated, so the thread-only comparison is also
a negative control. Optional `SpeedRestraint` and `SpeedRestraintThreadDefault` cases
explicitly enable restraint to exercise that helper. The saved settings, sampled
pressure state, final runtime status, binary hash, raw CSV and transcript accompany
every run under `target/throughput-*`. This measures the headless engine, not UI
rendering overhead. The host has a ten-minute timeout and retains the normal
single-instance and recovery barriers. The feature is excluded from normal builds.

For placement/power diagnostics, include `DefaultPCores` and use `-Telemetry`.
`DefaultPCores` uses the same disabled-engine settings as Default, but restricts
the benchmark process to the discovered performance-core mask during ST only.
The original process affinity is restored before MT and on failure. Missing
performance-core classification rejects the case rather than guessing indices.
Worker CPU masks are sampled, not complete migration traces. Each score also
records CPU milliseconds consumed by the runtime plus recovery helper during
measurement (the percentage column is relative to one logical CPU).

Telemetry samples Windows performance counters once per second into per-phase
TSV files: per-CPU and total Actual Frequency, total Processor Performance and
Performance Limit, package Power, and thermal-zone Temperature/Throttle Reasons.
Unavailable counters are recorded in `TelemetryNotes`; raw units are retained.
On the tested provider Power is milliwatts and Temperature is kelvin. ACPI thermal
zones are not necessarily CPU core temperatures, and frequency counters are not
a complete hardware throttling diagnosis. Use the same telemetry option in every
compared case; it adds measurement overhead. No sensor driver is installed.

- [Windows performance counters](https://learn.microsoft.com/en-us/windows/win32/perfctrs/about-performance-counters)
- [Process affinity](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.processoraffinity)
- [Sampled processor number](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getcurrentprocessornumber)

Adaptive Engine benchmark results are stored separately from the project README
because they are specific to the machine and its current thermal and power state.

For a read-only observation-cost probe, run
`cargo test --locked profile_live_observation_costs -- --ignored --nocapture`
alone on an interactive Windows desktop. It checks that the visible-process
prefilter preserves membership and prints observation and path-grouping timings.
These component timings are not ST/MT throughput results.

- [Intel Core 5 210H](intel-core-5-210h.md) - 2026-07-15 preset benchmark
- [Intel Core 5 210H adaptive runtime](intel-core-5-210h-adaptive-runtime.md) - 2026-07-15 real runtime A/B
- [Intel Core 5 210H system and preset diagnostics](intel-core-5-210h-system-and-preset-diagnostics.md) - 2026-08-06 live-system, scheduler, compute, memory, and package-power measurements
- [Intel Core 5 210H counterbalanced Adaptive runtime matrix](intel-core-5-210h-adaptive-runtime-counterbalanced-20260807.md) - 2026-08-07 and 2026-08-10 validated CPU, I/O, and MessageLoop runtime results with foreground-host integrity, package power, and Windows priority activation
- [Intel Core 5 210H CPU Scheduler redesign and context-aware power A/B](intel-core-5-210h-cpu-scheduler-redesign-20260818.md) - 2026-08-18 targeted-only, broad-policy, context-aware, and Limit Background Processors comparison with editable profile tuning, direct EcoQoS observation, and power-gate results
- [Intel Core 5 210H Least-used vs E-core-preferred selector A/B](cpu-selector-least-used-vs-e-core-preferred-20260818.md) - 2026-08-18 release-runtime comparison of foreground latency, background capacity, and package power
- [AMD Ryzen 7 7735HS](amd-ryzen-7-7735hs.md) - preset benchmark

Run from the repository root:

```powershell
.\scripts\adaptive_engine_process_benchmark.ps1 -Passes 3 -Rounds 5 -Iterations 1000000
```

For a real Adaptive Engine comparison against stock Windows Balanced:

```powershell
.\scripts\adaptive_runtime_benchmark.ps1 -Passes 4 -Rounds 5 -Iterations 1000000 -WorkerSeconds 180
```

See [the benchmark guide](../docs/adaptive-engine-benchmark.md) for methodology,
metrics, and additional scenarios.
