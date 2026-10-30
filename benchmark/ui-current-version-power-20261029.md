# Current-source background package power — 29 October 2026

The completed fixed Wind / Iced / Iced / Wind comparison measured **18.80 W
with Wind UI and 24.04 W with Iced**, weighted by meter duration. Wind was
lower in both pairs. These are whole CPU-package observations under different
host loads, so the observed 21.80% gap does **not** establish a framework power
saving or Winderust-only consumption. No causal power winner is demonstrated.

The user selected self-closing **instrumented release benchmark executables**.
The default-feature production executables were not measured. This fresh cohort
is separate from the [earlier October 29 CPU/RAM study](ui-current-version-background-20261029.md)
and historical power runs; none of those observations enter these totals.

## Measured energy

Each formal trial settled for 300 seconds, then sampled for 180 seconds at 1 Hz.
Joules and elapsed meter time below determine average watts; instantaneous
Power-counter averages are not substituted. The two short pilots are excluded.

| Capture, in order | Package energy (J) | Meter duration (s) | Package power (W) | Host CPU, machine % | Host memory load % |
|---|---:|---:|---:|---:|---:|
| Opening no-app baseline | 2,958.078368 | 119.994 | 24.651886 | 23.750 | 89–93 |
| 1 — Wind UI | 3,317.077093 | 180.011 | 18.427080 | 13.857 | 91–93 |
| 2 — Iced | 4,271.955233 | 179.983 | 23.735326 | 18.931 | 86–94 |
| 3 — Iced | 4,383.461159 | 180.011 | 24.351074 | 18.020 | 86–87 |
| 4 — Wind UI | 3,452.204049 | 180.009 | 19.177952 | 14.448 | 85–87 |
| Closing no-app baseline | 3,245.943346 | 120.008 | 27.047725 | 27.299 | 83–89 |

Formal weighted totals use `sum(joules) / sum(meter seconds)`:

| Framework | Total energy (J) | Meter time (s) | Package power (W) | Mean host CPU, machine % |
|---|---:|---:|---:|---:|
| Wind UI | 6,769.281142 | 360.020 | **18.802514** | 14.152 |
| Iced | 8,655.416392 | 359.994 | **24.043224** | 18.475 |

Wind minus Iced was −5.308246 W (−22.36%) in pair 1 and −5.173121 W
(−21.24%) in pair 2, with percentages relative to Iced. The aggregate gap was
−5.240710 W (−21.80%). Meanwhile, the closing baseline rose **2.395839 W
(9.72%)** from the opening baseline. Baselines are contextual observations;
neither is subtracted to produce an application-only number.

Both formal Iced exposures coincided with higher overall host CPU load than
both Wind exposures. Fixed reverse order helps expose simple order effects,
but two pairs cannot separate framework effects from changing host activity,
thermal state, frequency, memory pressure or sampler overhead. The sampler
itself averaged 2.52% of one core during Wind trials and 2.82% during Iced
trials. The 1 Hz samples are correlated observations, not 724 independent trials.

## Concurrent application counters

These counters cover each application's main process **plus one recovery
helper**; the sampler and other host processes are excluded from this table.
Memory entries are ranges of the two trials' medians, not pooled quantiles.

| Metric | Wind UI | Iced |
|---|---:|---:|
| Aggregate process cycles | 3,934,138,839 | 3,707,209,516 |
| Cycles per second | 10.927 million | 10.298 million |
| Total user + kernel CPU time | 1.406250 s | 1.390625 s |
| CPU, % of one core | 0.391 | 0.386 |
| Private commit, median range | 19.02–19.10 MiB | 24.73–25.00 MiB |
| Private resident RAM, median range | 6.71–6.74 MiB | 11.84–12.05 MiB |
| Threads, median range | 21–22 | 34 |
| Handles, median range | 515 | 652–654 |
| Processes | 2 | 2 |

Wind's aggregate cycle rate was 6.11% higher, but the paired direction reversed:
Wind was 54.10% higher in pair 1 and 28.65% lower in pair 2. This cohort therefore
does not establish a consistent cycle advantage. Wind retained lower private
memory, thread and handle counts. The application CPU-time gap was only
0.015625 seconds over about 360 seconds per framework; CPU time alone cannot
attribute the whole-package difference. Process cycle counts are not joules.
Full I/O and page-fault increments remain in the independent raw review;
I/O includes cached/device operations and page faults include soft faults.

## Scope and acceptance

Both fixtures use Tiny-skia software rendering, Rust 1.94.1, thin LTO, one
codegen unit, stripped symbols and `render-smoke,runtime-benchmark` features.
Wind uses current application sources with pinned Wind UI 0.28.0; Iced uses
the preserved Iced 0.14.0 application with **112 byte-identical current shared
engine files** and eight matched tray-source comparisons. No upstream patch
or dependency upgrade was made.

| Fixture | Bytes | SHA-256 |
|---|---:|---|
| Wind UI | 13,069,312 | `27afc18910dd07b55092b32571e73bc2bebfae52b2230b1e6f2b901e5654d050` |
| Iced | 19,608,064 | `6172bc42ea05560d782c94038972f79cbf91aa9ba7d03f811af6d513d92013cf` |

Process Priority was enabled with a missing no-match executable declaration:
configured polling 1 second, existing hidden-engine observation floor 10 seconds.
There were no active rule targets or external workload workers. This tests an
enabled scheduler's no-match background behavior, not active process adjustment,
incremental engine cost or every scheduler running together.

Both fixtures checked a visible, unminimized Home window with usable native
geometry, then destroyed it before measurement. This proves geometry and
lifecycle, not first paint. Neither performed prewarm screenshot/RGBA/PNG
capture. Both retained a real tray, an absent settings HWND throughout sampling,
a live runtime worker at the boundaries, identical in-memory settings, ordinary
exit code 0 and successful cleanup/restoration. No injected wake, exit assistance
or forced termination was used. Updates, startup registration and cross-session
control were disabled. Saved settings were not modified.

All eight fixed phases passed: two excluded 10-second pilots, two 120-second
baselines and four formal trials. All **988 Energy/Power/Time observations** had
valid API and counter statuses; all 980 intervals were retained. AC remained
online throughout. Exact counts, meter and wall timing, source identities,
process identities, chronological nonoverlap and ordinary main/helper exits
passed frozen replay and two independent reviews. Sampled known-name process
checks found only the expected main/helper pair, and none in baselines. They
cannot exclude renamed executables or transients between samples.

The meter was `RAPL_Package0_PKG` through Windows PDH: cumulative Energy in
pWh and Time in ms. Conversion was `ΔEnergy × 3.6e-9` joules and
`ΔTime / 1000` seconds. Each query had a fixed priming sample and one-second
wait outside measurement. See Microsoft's [energy-meter interface](https://learn.microsoft.com/en-us/windows-hardware/drivers/powermeter/energy-meter-interface),
[performance-data collection](https://learn.microsoft.com/en-us/windows/win32/perfctrs/collecting-performance-data)
and [formatted PDH counter API](https://learn.microsoft.com/en-us/windows/win32/api/pdh/nf-pdh-pdhgetformattedcountervalue).
Units were also checked against this host's current native counter metadata.

This measures the **whole CPU package**, including other software and collector
overhead. It measures neither application-only watts, discrete-GPU power,
wall power nor battery life. GPU and per-thread counters were disabled.
The current host load was explicitly accepted; this was not a quiet-host test.

## Evidence and checks

Raw evidence is local-only and ignored under
`benchmark-results/production-power-20261029/`; its preparation name does not
mean normal production executables were measured. Replay requires those files
and the reused fixtures under `current-version-background-20261029/`.

- [Frozen protocol and replay command](../benchmark-results/production-power-20261029/protocol/README.md)
- [Accepted full replay](../benchmark-results/production-power-20261029/replay.json)
- [Independent raw arithmetic, counters and host context](../benchmark-results/production-power-20261029/independent-measurements/cohort-summary.json)
- [Independent final lifecycle, source and chronology review](../benchmark-results/production-power-20261029/power-review/final-formal04-closing-baseline-cohort-gates.json)
- [Source and executable preservation](../benchmark-results/production-power-20261029/post-run-source-preservation.json)
- [Final validation and cleanup](../benchmark-results/production-power-20261029/verification.json)

The protocol's 34 tests and four raw-energy tests passed, including an
independent rerun; nine additional malformed-input probes were rejected. Setup
retains a failed no-app reporting attempt (wrong dictionary key), an unprimed
meter probe with one unavailable Power sample, and a separate accepted 30-second
preflight. No application launched in those probes. They are separate from the
fixed cohort; no formal sample or trial was deleted, replaced or retried.

Formatting, strict all-target Clippy, tests (911 passed, 19 ignored), architecture
and legacy-name checks passed. Final inventory found no remaining Winderust or
dedicated worker process names; Balanced remained selected and startup
registration remained absent. All 168 current Wind source inputs, both fixture
executables and the normal release executable were unchanged. No production
code or dependencies changed; the user's staged work was preserved. No new
benchmark fixture builds were needed; existing frozen release executables were
reused. Generated evidence and earlier results
remain outside source control and release artifacts.
