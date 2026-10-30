# Adaptive Engine observation-only scan gating — 2026-10-22

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

Settled, monitor-only Adaptive Engine operation used **36.88% fewer CPU cycles**
by median paired reduction after skipping unnecessary process-control eligibility
and visible-window checks. Both matched pairs improved. CPU time fell by **38.83%**
by the same paired calculation. Private committed memory was essentially unchanged;
these captures do not establish a RAM or energy saving.

This changes Winderust's shared scheduler backend. Wind UI source, dependency
revision, UI lifecycle and the benchmark fixture are unchanged. This experiment
does not compare the optimized build with Iced.

## Implementation and safety

- Keep the existing full reconciliation on startup and settings changes.
- Use query-only process identities on subsequent cycles when Process Priority,
  Background Efficiency, Memory Priority, Limit Background Processors and Dynamic
  Resource Zones are disabled, with no tracked targets, policy claims, restoration,
  pending CPU reconciliation, focus candidate, unavailable target or failure state.
- Preserve foreground descendant identities, foreground CPU sampling, unavailable
  samples, pressure thresholds, recovery-band hysteresis and helper invalidation.
- Share one cycle-local inventory. If a later consumer needs control eligibility,
  enrich the captured table once and revalidate its known creation identities.
  Unknown or changed generations remain non-actionable until a fresh cycle.
  Cached observations never authorize a mutation; controllers retain their
  acquisition, protection, access, exact-identity and restoration checks.
- Reuse the established workload in the automation runner so reporting does not
  immediately force the expensive scan again. Claims are checked separately from
  managed state because a request equal to the original value can still own policy.

Tracked state from an earlier full pass can conservatively delay the shortcut
until normal reconciliation clears it. Active controls and pending cleanup retain
the full path. This is a bounded optimization of observation-only monitoring.

Implementation: [manager](../src/features/winderust_features/adaptive_engine_process.rs),
[cycle observations](../src/runtime/observations.rs),
[process collection](../src/foreground/process_list.rs), and
[workload reporting](../src/backend/automation/runner.rs).

## Matched release measurements

Intel Core 5 210H, 8 cores / 12 logical processors, Windows build 26200,
Rust 1.94.1. Both conditions use Wind UI 0.24.0 at
`36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2` and identical serialized settings
(`394d88fd007d0a72b3da889be2ae21f698168062918bc22dbcd5d5c868ddbfce`).

The prespecified order was baseline/candidate, then candidate/baseline: **two
matched pairs**, four accepted captures. Each uses four idle disposable sibling
workers, 65 seconds warmup, 90 seconds measurement, 1 Hz counters and EX2 private
working-set collection. Pressure monitoring and automation are enabled; all
process actuators and processor power-policy changes are disabled. The production
close-to-tray path destroys the settings HWND. Settings remain in memory.

The primary scope is the application plus its one real recovery helper. Workers
and the sampler are excluded from that scope. Rates use recorded native counter
deltas and their measured intervals, rather than the requested duration.

| Pair / order | Baseline cycles/s | Candidate cycles/s | Cycle reduction | Baseline CPU* | Candidate CPU* |
|---|---:|---:|---:|---:|---:|
| 1 / baseline → candidate | 130.762M | 78.710M | 39.81% | 4.808% | 2.812% |
| 2 / candidate → baseline | 128.501M | 84.879M | 33.95% | 4.896% | 3.126% |

*CPU is a percentage of one logical core, including user and kernel time.
Median paired cycle reduction is **36.876%**; median paired CPU-time reduction
is **38.828%**. Descriptive condition medians are **129.631M → 81.795M cycles/s**
and **4.852% → 2.969% of one core** (48.52 → 29.69 CPU milliseconds/second).

| Scope, median of two runs | Baseline cycles/s | Candidate cycles/s |
|---|---:|---:|
| Main application | 129.610M | 81.766M |
| Recovery helper | 26.1K | 39.1K |
| Combined | 129.631M | 81.795M |

The improvement is in the main application. Helper cycles did not decrease;
its sampled CPU-time deltas rounded to zero in both conditions. Combined and
individual rates have independently recorded interval denominators, so their
rounded rates need not sum exactly.

## Memory, observer and host context

| Combined scope, median of each run's median | Baseline | Candidate |
|---|---:|---:|
| Private commit | 23.197 MiB | 23.186 MiB |
| Private resident working set | 10.596 MiB | 6.959 MiB |
| Threads | 21 | 21 |
| Handles | 512 | 515 |

**The resident-memory median is not evidence of a RAM improvement.** Pair 1
changed 10.480 → 10.551 MiB; pair 2 changed 10.711 → 3.367 MiB. The second
candidate's sampler and helper working sets also fell. Private commit stayed
similar: 23.023 → 22.824 MiB in pair 1 and 23.371 → 23.547 MiB in pair 2.
This is consistent with shared-host working-set variability; these counters do
not prove its cause. Candidate handle medians ranged from 512 to 518; this short
experiment does not establish a handle trend or leak.

The sampler consumed 1.528–1.685% of one core. Per-capture collection-time medians
were 13.39–14.62 ms and p95 values 17.43–43.37 ms. Observer cost is reported
separately and is not subtracted. Host CPU medians ranged from 12.91–19.53%,
p95 values from 25.42–49.65%, and the maximum was 57.03%. Host RAM-use medians
were 83–86%, with p95 values of 85–88%. The desktop was shared rather than an
isolated laboratory machine. I/O, page faults, pool use, working-set trends,
compositor and system-process observations remain in the exports.

## Validation and provenance

Every accepted capture contains 91 snapshots and 90 delta intervals, an absent
settings HWND, one helper, four idle worker siblings, positive endpoint inventory
counts (364–366), zero adjustments/failures and no runtime or collector errors.
All four fixtures exited normally, reported successful restoration, passed
independent native identity/state checks, preserved the original Balanced power
plan, and stopped all workers cooperatively with exit code zero. Native
`active_action_verified=false` correctly describes this no-actuator workload.
It is not a live test of restoration after active restraint.

The initial non-admin launch panicked before readiness/runtime initialization.
It produced no accepted measurements. Its four exact workers were independently
verified unchanged with advancing heartbeats, then stopped cooperatively. That
failed attempt and its cleanup proof are retained alongside the four valid runs.

Raw CSV summaries were recomputed and independently reviewed. The first version
of the new post-capture analyzer incorrectly treated combined timestamps as one
process clock. The collector queries processes separately and uses the maximum
individual interval for its aggregate. The corrected analyzer reconstructs that
contract exactly, validates the individual clocks and cumulative deltas, and
preserves the original analyzer and revision note. No capture, denominator or
tolerance was changed to make the analysis pass.

Locked default tests passed **880**, with **18 ignored**; benchmark-feature tests
passed **902**, with **18 ignored**. Strict Clippy passed in both configurations,
including `-D unsafe-op-in-unsafe-fn`. Formatting, diff whitespace, legacy scan,
architecture ownership gates and `graphify update .` passed. New tests cover
unchanged policy claims, cheap/full cache ordering, failed observations,
generation-safe enrichment, startup/settings/actuator guards and repeated
high → unavailable → recovery-band → low pressure without losing hysteresis.
The existing restoration and retry tests remain in the passing suite.

- Baseline fixture SHA-256: `e5e825397dd4417e292f39a2b300c1079dae458a4f10fea1c74cf26093cb51d6`.
- Candidate fixture SHA-256: `a054975b20fc52bead814b0b42c84125b127a0bd3bcf1dd826ab34ca6f11c2b2`.
- Normal release SHA-256: `410c7bdb27e9b7a32846852604e4470064ee9cbb4160b00dcc90ef23215ad69f`.

The normal release at `target/release/winderust.exe` was rebuilt with default
features. Benchmark binaries remain isolated under
`target/adaptive-scan-gating-20261022/`. Frozen 164-file source manifests differ
only in the nine intended implementation/test files. Wind UI has no tracked
source changes, and Cargo.lock is unchanged.

See the [evidence bundle](../benchmark-results/archive-20261028/benchmark/adaptive-scan-gating-results-20261022/README.md),
[analysis JSON](../benchmark-results/archive-20261028/benchmark/adaptive-scan-gating-results-20261022/analysis/analysis.json),
[trial CSV](../benchmark-results/archive-20261028/benchmark/adaptive-scan-gating-results-20261022/analysis/trials.csv), and
[paired CSV](../benchmark-results/archive-20261028/benchmark/adaptive-scan-gating-results-20261022/analysis/paired.csv).

## Limits

These are descriptive results from two pairs, without statistical significance
or pooling with older cohorts. GPU and package energy were not measured. CPU
cycles are not instructions, elapsed time or joules. No hot-function profile,
active-restraint performance result, all-profile improvement, RAM saving or new
Wind UI-versus-Iced conclusion follows from this matrix. Mixed-feature workloads
need their own measurements. If Adaptive monitoring collects first and a later
priority, allocation or pressure-dependent helper needs control metadata in the
same pass, enrichment reopens query handles and rereads creation times for known
captured identities. There is still one Toolhelp inventory, and full-first passes
avoid that upgrade, but the extra identity queries can offset part of the saving.
Foreground and pressure transitions with mixed features were not benchmarked;
no net regression or gain is established for those workloads. Keep identity
validation intact when investigating a future schedule-aware observation hint.
