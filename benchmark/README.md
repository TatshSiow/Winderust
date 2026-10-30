# Benchmarks

Historical linked raw evidence is archived locally under
`benchmark-results/archive-20261028/`; the October 29 reports use their own
dated directories under `benchmark-results/`. All generated evidence is
excluded from Git and release artifacts; replay requires those separate files.

Keep reusable benchmark scripts, scenario input configuration and maintained
reports in source control. Generated captures, frozen source copies, result
CSV/JSON, trace files and evidence ZIPs belong in the ignored
`benchmark-results/` directory. The archive's `archive-manifest.json` records
the original paths, new paths, sizes and verified SHA-256 hashes.

For the fresh actual package-energy comparison, see the
[October 29 power report](ui-current-version-power-20261029.md). All four
fixed-order application trials and both baselines pass replay and independent
review. Whole CPU-package power averaged 18.80 W with Wind and 24.04 W with
Iced; overall host CPU load also differed and baselines drifted. These
self-closing release fixtures establish neither application-only watts nor a
causal framework power winner. Normal production executables were not measured.

For the fresh current-source hidden scheduler comparison, see the
[October 29 CPU and RAM report](ui-current-version-background-20261029.md).
All four fixed-order captures pass replay and independent review. Wind used
about 5 MiB less private resident RAM; Iced used 7.40% fewer aggregate cycles
and fewer cycles in both pairs. CPU-time directions reversed between pairs.
The isolated Iced adapter no longer captures a prewarm screenshot. Host load
varied, and this CPU-only study establishes no power winner.

For deferred process identity reads and the earlier matched tray comparison, see
the [October 28 staged-inventory report](ui-tray-engine-staged-20261028.md) and
[raw evidence](../benchmark-results/archive-20261028/benchmark/ui-tray-engine-staged-results-20261028.zip). The scan diagnostic
used 14.4% fewer cycles; the framework trial pairs reversed direction, with a
0.24% aggregate gap. Both live restoration checks passed. This CPU-only study
does not establish a consistent framework or power winner.

For the implemented sparse Process Priority scan optimization and fresh matched
CPU validation, see the [October 28 background report](ui-tray-engine-optimized-20261028.md).
All four captures and both live restoration checks passed. Cycle results reverse
between pairs, so the aggregate does not establish a reliable framework winner.
An earlier power-counter failure is retained as an excluded study; the accepted
CPU-only study makes no power claim.

For the completed idle-only daemon comparison, see the
[idle checkup](ui-idle-daemon-checkup-20261027.md). Four fixed-order captures passed
validation. Iced without a persistent tray used slightly less private memory and
fewer cycles in both pairs; most cycles came from one brief housekeeping event.
Host pressure and the tray mismatch limit the comparison, and package power
establishes no application power winner. This is separate from the earlier
failed daemon scheduler cohorts below.

For the actual accessibility/dependency checks and attempted Iced zero-window
comparison, see the [framework follow-up](ui-framework-followup-20261027.md).
Both settings builds exposed no semantic UI Automation controls. The dependency
audit found shared and configuration-specific maintenance issues. All four
daemon cohorts stopped during pilots and add zero accepted formal comparisons;
their failures and cleanup limits remain in a separate portable evidence ZIP.

For the current shared-engine Wind UI 0.28 / Iced 0.14 comparison, see the
[current background power and resource report](ui-framework-current-power-20261025.md).
Six original captures pass strict replay. Wind uses less private memory; enabled
scheduler CPU results reverse between pairs and package power is about 10 W
for both. A separate reverse idle continuation is retained as diagnostic data
because Iced exceeded the sampler timing limit. No reliable power winner or
completed formal eight-trial cohort is claimed.

For actual Wind/Iced background package-energy readings, see the
[measured power report](ui-framework-power-measured-20261025.md). One matched
180-second pair measured 10.015 W with Wind and 10.001 W with Iced. Both planned
two-pair cohorts stopped at validation failures; raw data and failures are
retained, and no reliable power winner is established.

For the earlier attempted Wind/Iced background energy comparison, see the
[power report](ui-framework-power-20261024.md). The package meter returned usable
readings, but Wind's tray availability gate prevented background readiness.
The portable evidence adds one no-app meter check, zero application energy
trials, and no power winner.

For the Wind UI 0.28 upgrade, validation and the failed background comparison,
see the [upgrade report](ui-windui-upgrade-20261023.md). Both attempts stopped
before readiness and add zero accepted CPU or memory trials; a portable verifier
preserves their evidence and the separate old/new readiness diagnostics.

For remaining idle-only candidates and their limits, see the
[hidden-idle research](ui-hidden-idle-research-20261023.md). First-use recovery
startup is the strongest conditional experiment; the helper's observed 24.77%
cycle share is a component measurement, not a promised saving. No new benchmark
or production change was made by that research.

For the stack investigation around five minutes of background process age, see
the [five-minute ETW report](ui-idle-five-minute-etw-20261023.md). The CPU collector
failed, but a separate bounded trace analysis found Windows threadpool and Sophos
module execution. It identifies no demonstrated application optimization or new
framework ranking; the original failures and provenance limits are preserved.

For the completed eight-trial comparison after selecting the current host load,
see the [long-settled current-load report](ui-settled-idle-current-load-20261023.md).
Wind used less private memory in all four pairs; cycle results split two pairs
each way and were dominated by brief activity near five minutes of process age.
It preserves a separate cohort and a portable offline verifier.

For the fresh attempt after the desktop was made available, see the
[second long-settled host-gate report](ui-settled-idle-ready-2-20261022.md).
System commit passed, but CPU load prevented all application launches. It adds
host diagnostics and no Wind/Iced performance observations.

For the attempted longer-settled Wind/Iced comparison, see the
[long-settled hidden idle report](ui-settled-idle-comparison-20261022.md).
Its five-minute host gate failed before any application launch, so it adds no
application performance observations or new framework ranking.

For the subsequent event-registration cleanup, fresh nine-trial comparison and
its limits under host memory pressure, see the
[hidden idle event-registration report](ui-idle-watcher-gating-20261022.md).
It does not establish a repeatable cycle improvement or parity with Iced.

For the resolved RPC/threadpool lifecycle stacks and tracing limits, see the
[background ETW investigation](ui-idle-etw-20261022.md).

For the implemented Adaptive Engine observation-only scan optimization, see the
[matched before/after report](adaptive-scan-gating-20261022.md). Two AB/BA pairs
measure its CPU/cycle benefit, preserve cleanup checks, and distinguish unstable
resident RAM from essentially unchanged private commit.

For the longer per-thread investigation of residual background cycles, see the
[background thread diagnostic](ui-idle-thread-diagnosis-20261022.md). The Wind UI
dependency is unchanged; diagnostic rates are separate from balanced comparisons.

For the subsequent upstream audit, native idle cleanup and its validation, see
the [native background idle cleanup](ui-native-idle-cleanup-20261022.md).

For the implemented event-driven Wind UI background optimization and a fresh
nine-capture comparison with the previous build and Iced, see the
[event-driven background comparison](ui-idle-event-driven-20261022.md).

For Wind UI versus Iced CPU, cycle, memory, GPU-counter, I/O, and lifecycle
measurements, see the [UI framework resource comparison](ui-framework-resource-comparison-20261019.md).

For enabled production schedulers, disposable-target native verification, and
background cost improvement priorities, see the [enabled scheduler resource
comparison](ui-scheduler-resource-comparison-20261020.md). It covers 20 profiles,
including busy CPU Limiter controls and repeated Off/no-match observations.

For the measured cause of Wind UI's extra disabled-background cycles, see the
[background maintenance diagnosis](ui-idle-maintenance-diagnosis-20261020.md).
Three controlled pairs isolate application maintenance while preserving native
timer dispatch; they are separate from the framework and scheduler comparisons.

For fresh Iced versus normal and low-maintenance Wind UI measurements, see the
[matched background comparison](ui-idle-framework-comparison-20261020.md).
It uses three balanced blocks with matching disabled-automation fixture settings.

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
