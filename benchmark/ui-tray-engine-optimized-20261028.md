# Optimized background scan: Wind UI and Iced CPU validation

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

Measured on 2026-10-28. The accepted CPU-only comparison ran from
01:49:42 to 02:27:18 UTC. Separate live actuation checks finished at
02:28:21 UTC. All four fresh captures and both actuation checks passed.

**This run does not establish a consistent CPU-cycle winner.** Iced used fewer
cycles in the first pair; Wind UI used fewer in the reverse-order pair. Wind's
two measurements varied substantially. Both builds contain the same engine
optimization, so these results also do not isolate its before/after benefit.

Wind used less private commit in both pairs. Resident memory varied with host
pressure, and only Iced performed screenshot/PNG prewarming; the memory results
do not establish either framework's minimum requirements. No power-consumption
winner is claimed.

## Implemented change

The change is in Winderust's shared engine. The pinned upstream frameworks and
renderers were not modified.

- Process appearance detection reads the existing read-only process identities
  instead of forcing collection of control eligibility for every process.
- When every reachable Process Priority default is `Default`, unrelated
  executable basenames can be rejected before path/control queries. Enabled
  exact-path rules still determine selection; a basename never authorizes a
  mutation. Cached paths and uncertain filenames retain conservative handling.
- Only concrete rule targets receive the existing generation-checked eligibility
  enrichment. Critical/protected-process checks, session restrictions, controller
  revalidation, exclusions, first-rule precedence, and obsolete-claim restoration
  remain in the path.
- Concrete global defaults retain the full-table path. Other features may still
  require a read-only-to-full observation upgrade within the same pass.

Implementation: `src/backend/automation/runner.rs`,
`src/features/priority_control/process_priority.rs`, and regression coverage in
`src/runtime/observations.rs`. The evidence package includes the engine-only
patch against the accepted October 27 source. This is a sparse-rule optimization,
not a claim that every automation profile now uses fewer cycles.

## Matched conditions

| Item | Wind UI | Iced |
| --- | --- | --- |
| Framework | 0.28.0, revision `d5c105067d7a31f0f14e388f1fe6394f1a043988` | 0.14.0 |
| Renderer | tiny-skia software | tiny-skia software |
| Shared engine | 112 byte-identical files; same three-file overlay | Same |
| Build | Rust 1.94.1; thin LTO; one codegen unit; stripped symbols | Same |
| Background lifecycle | Home opened, settings HWND destroyed, real tray retained | Same, with screenshot/PNG render proof |
| Enabled feature | Process Priority only | Same |
| Rule | One exact, absent executable; Below Normal if it appears | Same serialized settings |
| Default priorities | All reachable tiers `Default` | Same |
| Process scope | Main plus all observed recovery helpers | Same |
| External workers during performance capture | Zero | Zero |
| Power/GPU collection | Disabled | Disabled |
| Exit assistance | None | None |

Settings stayed in memory; startup registration and update checks were disabled.
No user application was targeted. Native protection, process identity, access,
restoration, and recovery checks remained enabled.

The host was an Intel Core 5 210H with 12 logical processors and about 16 GB RAM,
Windows build 26200, on AC with the Balanced plan. The user authorized comparison
under the current host load. This was not a quiet-host or reboot-controlled test.

## Fixed fresh protocol

Two excluded pilots used 3 seconds of warm-up and 10 seconds of sampling each.
Formal order was Wind, Iced, Iced, Wind. Each formal trial used a 300-second
warm-up, 180-second capture at 1 Hz, and a 48-second fixture cleanup margin.
Every formal capture contains 181 samples. All trials were retained; there was
no replacement trial, outlier deletion, post-hoc extension, forced termination,
or injected exit wake in this cohort.

Acceptance required complete monotonic CPU counters, exact process identities,
consistent combined totals, unchanged settings, constant AC state, a live hidden
tray host and registered icon, absent settings windows, an enabled runtime worker,
zero no-match adjustments/errors, ordinary exit, and verified cleanup. Source and
binary fingerprints were checked before child launches. All 120 coordinator
inputs were independently revalidated before launch.

The sampler is outside application CPU/resource totals. Its own costs are
retained in the evidence. Independent recomputation matched all 60 numerical
cross-checks and confirmed actual source/binary parity.

## CPU and memory

Values include main and observed recovery helper. MiB means 1,048,576 bytes.
Memory values are the median combined per-sample gauges.

| Metric | Wind 1 | Iced 1 | Iced 2 | Wind 2 |
| --- | ---: | ---: | ---: | ---: |
| Cycles/s, millions | 17.767 | 14.471 | 14.369 | 7.298 |
| CPU time, seconds in capture | 1.171875 | 0.921875 | 0.968750 | 0.484375 |
| CPU usage, % of one logical CPU | 0.6510% | 0.5121% | 0.5382% | 0.2691% |
| Private commit, MiB | 18.535 | 20.508 | 21.313 | 18.516 |
| Private resident RAM, MiB | 4.926 | 7.781 | 6.348 | 6.430 |
| Total working set, MiB | 42.641 | 46.992 | 46.805 | 44.168 |
| Handles, median | 515 | 557 | 557 | 515 |
| Threads, median | 20 | 32 | 32 | 21 |

Iced used **18.55% fewer cycles relative to Wind** in the first pair. Wind used
**49.21% fewer relative to Iced** in the reverse-order pair. Duration-weighted
totals give 12.533 million cycles/s for Wind and 14.420 million for Iced, or
13.09% fewer for Wind in aggregate. The reversed pair directions and Wind's
large variation prevent treating that aggregate as a reliable framework ranking.

Mean trial-median private commit was 18.525 MiB for Wind and 20.910 MiB for Iced.
Private commit differs from resident RAM; summed total working sets can also
double-count shared pages. Iced's additional prewarm PNG allocation was released
before readiness, but allocator/cache retention was not quantified or subtracted.

## Host variation and other counters

| Metric | Wind 1 | Iced 1 | Iced 2 | Wind 2 |
| --- | ---: | ---: | ---: | ---: |
| Host CPU usage, % of machine | 17.97% | 12.83% | 11.27% | 8.51% |
| Host memory load range | 87-91% | 82-87% | 83-88% | 90-92% |
| Process page-fault increments | 48,364 | 37,960 | 34,709 | 18,720 |
| Process read/write byte increments | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| Sampler CPU time, seconds | 2.453125 | 2.609375 | 2.703125 | 2.515625 |

The host was not stationary. Wind's change is visible in CPU time as well as
cycles, so a cycle-frequency difference alone does not describe the result.
High-cycle one-second intervals also had large page-fault increments. This is
diagnostic correlation, not proof of a particular cause or stack location.
Page-fault counts include soft faults; process I/O counters are not physical
disk-traffic measurements.

The structured Process Priority status exports no scan count. Its debug snapshots
do contain boundary counts: Wind 1 was 370 at ready/end; Iced 1 was 366/365;
Iced 2 was 364/364; Wind 2 was 365/365. These are two snapshots per trial, not
per-pass telemetry or a basis for normalizing work during capture. Every typed
ready/end Process Priority status reported zero adjusted/failed processes and
no error. All four runtime workers were running at readiness and measurement end.
The packaged independent review's `context-final.md` is the authoritative
clarification of scan-count availability.

## Live functionality and cleanup

After performance collection, each build separately controlled one disposable
worker. Native priority changed Normal (`32`) to Below Normal (`16384`), then
returned to its exact Normal baseline on ordinary cleanup. PID, creation time,
path and executable hash were validated; affinity remained `4095` and Power
Throttling masks remained zero. Both workers and main applications exited with
code zero. Restoration and coordinated cleanup passed for both frameworks.

The final host inventory at 02:29:48 UTC found no Winderust or benchmark worker
process. The Balanced plan remained active. These checks validate the tested
Process Priority path, not every scheduler, tray menu click, or Explorer recovery.

## Rejected power study

An earlier, separate study on the same date required valid package-power counters.
It stopped at Iced's first formal capture with
`package sample 157: failed status system_package_power_mw`. The whole planned
cohort was rejected, actuation was not launched, and cleanup inventory found no
remaining application/worker. Its raw evidence is preserved separately.

The fresh CPU-only study was declared before inspecting rejected CPU results.
It disabled energy collection and omitted energy baselines while retaining CPU
and lifecycle gates. No rejected samples were reused or pooled. The accepted
CPU study therefore supplies no watts, battery-life, or power-winner conclusion.

## Validation and evidence

Production checks passed: `git diff --check`, `cargo fmt -- --check`, strict
locked all-targets Clippy including `unsafe-op-in-unsafe-fn`, 887 passing Rust
tests with 18 intentionally ignored, the legacy-name and architecture scans,
and `graphify update .`.

The isolated fixtures passed all 11 focused tests. Wind's strict fixture Clippy
passed; Iced retained exactly two inherited unused-code errors, with its labeled
dead-code-only allowance passing the remaining strict checks. The CPU harness
passed 17 synthetic tests and the actuation harness passed 9 safety tests.

The [evidence ZIP](../benchmark-results/archive-20261028/benchmark/ui-tray-engine-optimized-results-20261028.zip) and
[SHA-256 sidecar](../benchmark-results/archive-20261028/benchmark/ui-tray-engine-optimized-results-20261028.zip.sha256) retain
the accepted raw captures, protocols, source/binary fingerprints, engine patch,
both copies of the 112 shared source files, independent calculations, validation
logs, live restoration evidence, and an explicitly excluded power-attempt subtree.
Executables, libraries, build caches, and personal tooling are excluded. This
supports offline audit/replay, not a standalone binary rebuild.

These results cover an enabled no-match Process Priority scheduler with a
destroyed settings window. They do not establish incremental scheduler cost,
causal before/after cycle savings, all-scheduler behavior, instructions retired,
hardware wakeups, GPU/context-switch costs, or application power consumption.
Historical October 27 measurements are separate observations, not a simultaneous
unoptimized control.
