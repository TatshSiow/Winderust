# Wind UI / Iced background power attempt — 2026-10-24

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

**No framework power winner was measured.** The package-energy meter is operational,
but Wind did not reach its required background state. This attempt adds one
no-app meter check and four short readiness diagnostics, **zero accepted
application energy trials**, and no new CPU/memory ranking.

The portable [evidence bundle](../benchmark-results/archive-20261028/benchmark/ui-framework-power-results-20261024/README.md)
contains original CSV/JSON, diagnostic source, checksums and an offline verifier.
No production source, dependency, settings, power plan, or application safeguard
was changed for this attempt.

## Meter check

Before launching either fixture, the existing collector ran for 30 seconds:

```powershell
python -B scripts/measure_ui_resources.py --out target/framework-power-20261024/preflight --baseline-only --baseline-duration 30 --interval 1 --system-energy --no-gpu --memory-ex2
```

| Measurement | Result |
| --- | ---: |
| Populated Energy / Time / Power samples | 31 each |
| Positive energy/time intervals | 30 |
| CPU-package energy | 299.665672 J |
| Meter elapsed time | 29.902 s |
| Duration-weighted package power | **10.021593 W** |
| Difference between meter and CSV sample-clock spans | 0.106% |
| Host CPU, median / p95, whole machine | 38.13% / 47.75% |
| Sampler CPU, duration-weighted, one logical core | 4.392% |
| AC / battery readings | Online / 96%, all 31 samples |

The recorded Windows counter metadata identifies Energy as picowatt-hours, Time
as milliseconds and Power as milliwatts. Therefore:

```text
energy_J = (last_energy_pWh - first_energy_pWh) × 3.6e-9
meter_seconds = (last_time_ms - first_time_ms) / 1000
average_package_W = energy_J / meter_seconds
```

These are the PDH counter's units; direct EMI IOCTL timestamps have a different
unit. Calculating energy from differences and average power from elapsed time
follows Microsoft's [Energy Meter Interface guidance](https://learn.microsoft.com/en-us/windows-hardware/drivers/powermeter/energy-meter-interface).

**10.02 W is whole CPU-package consumption with neither fixture running.** It
includes other applications and the sampler. It is neither Winderust's power nor
whole-device, discrete-GPU or wall power. Host load was substantial. This is an
operational meter/units/arithmetic check, not an external accuracy calibration or
a quiet-host baseline.

The collector accepts finite values only after successful PDH calls and counter
status 0/1. All three counters were populated throughout this capture. However,
raw function return codes and `CStatus` were not saved, so their individual values
cannot be independently audited. Microsoft requires checking both
[function status and counter status](https://learn.microsoft.com/en-us/windows/win32/perfctrs/checking-pdh-interface-return-values).
Hardware CIM metadata collection also failed; that original error remains in
`calibration/measurements.json`. The collector still recorded 12 logical processors,
CPU family/model, Windows version and the active Balanced power-plan GUID.

## Matched builds and readiness

The preserved V3 pair was selected because its shared engine algorithms and
settings match. Current Wind UI 0.28 includes subsequent engine changes and
cannot fairly be paired with the frozen older Iced executable. This attempt
therefore provides **no Wind UI 0.28 power data**. The earlier
[enabled-scheduler report](ui-scheduler-resource-comparison-20261020.md) preserves
the V3 source/build provenance and matching-settings evidence.

| Fixture | SHA-256 |
| --- | --- |
| Wind UI 0.24, `wind-v3/winderust.exe` | `8f44a67a6050ed09c73917abe50cb4f5ad916a853c1a5855544712ad02577fe7` |
| Iced 0.14, `iced-v3/winderust.exe` | `2d040026dae4b842a3853f5503817a15a00abd097f24e4a681749aa6976d2b82` |

Readiness diagnostics used in-memory settings, disabled automation and a requested
1,000 ms lifetime **after readiness**. They ran sequentially and exited normally;
no scheduler workers or enabled engine energy trials were started.

| Diagnostic | Observed result |
| --- | --- |
| Initial Wind background | No `ready.json`; native-state timeout after approximately 20 s |
| Initial Iced background | Hidden native window verified; ready after 22.872 s; clean completion |
| Wind Home, with read-only native observations | Visible laid-out window; ready after 0.675 s; clean completion |
| Wind background, with read-only native observations | No readiness; window remained visible; same timeout |

Wind's production background behavior destroys its native settings window while
retaining the resident shell. The fixture requires positive layout, verified tray
availability and actual UI-window destruction. Home readiness clears a general
layout/callback failure for that diagnostic run. Both background attempts failed
the required state rather than producing unusually high background power.

The native diagnostic observed only windows belonging to its own fixture PID.
It used the collector's hidden enumeration anchor and encountered no enumeration
errors. Every sampled owned `WindUiAppHostClass` tray query returned
`0x80004005` (`E_FAIL`) for icon ID 1 with `GUID_NULL`; meanwhile
`WindUiWindowClass` remained visible with a 1120×760 client area. That identity
matches the pinned Wind UI 0.24 native tray registration.

This strongly points to the tray-availability safety gate. It does **not** establish
whether registration failed, a shell query was unavailable, or Windows was locked.
The [Shell_NotifyIconGetRect contract](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyicongetrect)
specifies `S_OK` on success and an HRESULT on failure; `E_FAIL` alone does not
identify the underlying cause. The external query also does not expose whether an
internal availability callback was delivered. Production cleanup clears the
transient UI error, so the final timeout is not a complete internal event trace.

The tray safeguard was retained. Forcing Wind's window away would change the
behavior being measured and could leave the settings window inaccessible.
Iced's hidden window and Wind's visible window are different workloads, so they
were not compared for power. A longer startup timeout would not resolve a
continually failing tray query.

## What remains necessary for a power comparison

First require an unchanged short Wind background preflight to publish readiness
and verify actual native UI destruction. Then freeze a new cohort, with neither
builds nor tracing running during collection:

- Identical V3 engine settings, four disposable idle target workers, and the
  `enabled-no-match` profile. This exercises a running scheduler without modifying
  unrelated processes or power plans.
- Two fixed paired repeats in Wind/Iced then Iced/Wind order, with 180 s settling,
  180 s measurement and 1 Hz sampling per launch. Retain later process-age bursts.
- 120 s no-app meter checks before and after the cohort, plus sampled AC state,
  power-plan verification, normal helper shutdown and restoration evidence.
- Complete finite counter coverage, monotonic energy/time, recorded PDH statuses,
  and independent energy/time arithmetic. The scheduler runner currently needs
  explicit energy-option forwarding before this cohort can run.

Report package joules, duration-weighted watts, paired differences, baseline drift,
host load, observer cost, process cycles and private resident/committed memory.
Keep all valid fixed trials; do not subtract the no-app baseline and call the
result per-app watts. Mixed pair directions or host drift larger than the observed
difference leave the comparison unresolved.

For Winderust's framework decision, the existing engine-enabled CPU measurements
remain close and workload-dependent; Wind's previously measured memory advantage
does not prove an energy advantage. This attempt provides no evidence that Iced
or Wind UI consumes less power with the background engine enabled.

## Verification

The portable verifier independently validated all 23 bundle files and the meter
arithmetic, 18 unchanged original-file copies, two failed Wind background
attempts, Home readiness, Iced's hidden-state declaration, 20 failed tray probes
and 18 retained-visible background samples. Five corruption tests passed.

Repository validation passed: `git diff --check`, `cargo fmt -- --check`, strict
all-target Clippy, the legacy-name scan, and `cargo test --locked` (885 passed,
18 ignored). These software checks do not replace the missing application energy
trials.

The required Graphify AST refresh also completed (8,292 nodes, 22,656 edges,
354 communities) after a retry outside the sandbox resolved its denied local
Python worker-pipe operation.
