# Benchmarks

Keep machine-specific reports, raw results, and captures local under ignored
`benchmark/results/`, `benchmark-results/`, or `target/`. Markdown reports under
`benchmark/` are also ignored except for this README. Commit reusable runners and
methodology; retain hardware, build, settings, test conditions, and limitations
alongside each local run.

For CPU Limiter duty cycling versus fixed half-pool Processor Affinity (Hard),
use the [disposable-worker comparison](../docs/development.md#cpu-limiter-mechanism-comparison).
It measures synthetic work, CPU time, and response latency inside the target,
with exact placement restoration after each case. It does not require a focused
window or change saved settings. Concurrency and CPU-time limits have different
semantics; these results cannot establish an automatic policy or performance in
an independent foreground application.

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

Current custom-runner results use decimal byte units: 1 KB is 1,000 bytes and
1 MB is 1,000,000 bytes. Their score objects record `byte_unit_base = 1000` and
`byte_rate_unit = "MB/s"`. Before unit standardization, custom-runner byte rates
used 1,048,576 bytes per displayed unit, despite the `_mbps` field names and
MB/s table labels; size parameters used 1,024 bytes per KB. Preserve original
units in local raw JSON and reports. Multiply an older custom-runner byte-rate
value by 1.048576 to express it in decimal MB/s. Percentage ratios need no
conversion. This does not apply
to independently recorded third-party diagnostic results. Matching numeric
buffer-size parameters across the two conventions also changes the actual
workload, so cross-revision timing comparisons require matching byte sizes.

For a read-only observation-cost probe, run
`cargo test --locked profile_live_observation_costs -- --ignored --nocapture`
alone on an interactive Windows desktop. It checks that the visible-process
prefilter preserves membership and prints observation and path-grouping timings.
These component timings are not ST/MT throughput results.

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
