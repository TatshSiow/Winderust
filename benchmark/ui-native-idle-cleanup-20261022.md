# Native background idle cleanup — 2026-10-22

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

This follows the [first event-driven optimization](ui-idle-event-driven-20261022.md).
Removing healthy tray polling and retained presentation work reduced combined
application-plus-helper cycles in all three balanced blocks: **74.19% median
paired reduction**, with individual reductions of 72.07–86.25%. Iced still used
fewer cycles in every block. Wind's private resident RAM remained about half
Iced's; this cleanup did not demonstrate a material further RAM reduction.
The existing pinned Wind UI dependency is unchanged.

## Upstream findings

The pinned Wind UI 0.24.0 event loop already releases high-resolution timing and
blocks in `GetMessageW` when idle. Its public channels wake the resident host,
including when no UI window exists. Window-owned timers disappear with their
window. These facilities are sufficient for event-driven background operation:
[idle loop](https://github.com/huanfeng/wind-ui-rust/blob/36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2/src/platform/win32/mod.rs#L1923),
[channel API](https://github.com/huanfeng/wind-ui-rust/blob/36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2/src/app/mod.rs#L2426),
[resident example](https://github.com/huanfeng/wind-ui-rust/blob/36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2/examples/resident.rs).

Native tray integration already handles `TaskbarCreated` and bounded re-add
retries. Public tray APIs expose no availability or failure callback, so the
narrow Windows adapter remains necessary for startup verification and keeping
the application accessible when verification fails. Current upstream
[v0.26.0](https://github.com/huanfeng/wind-ui-rust/commit/50d47e2a989e329650eb46787ccde18bfa923944)
has the same public tray declaration. No newer health API was overlooked:
[tray declaration](https://github.com/huanfeng/wind-ui-rust/blob/36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2/src/platform/tray.rs),
[native installation and recovery](https://github.com/huanfeng/wind-ui-rust/blob/36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2/src/platform/win32/mod.rs#L710).

## Implementation and behavior

- Verify tray availability at startup, Explorer's `TaskbarCreated`, explicit
  settings restore, and observed settings-window destruction. Healthy completion
  cancels the native timer. Unavailable results retry after five seconds.
- Preserve one outstanding query, its two-second deadline, exact host identity
  validation, nonblocking teardown, and an accessible window on failure. Native
  events during a query require one fresh verification after it completes.
- Start Home's sampling worker lazily and disconnect it on window destruction.
  An in-flight sample retains its old worker until completion without blocking
  the UI thread. Keep dashboard history and pause preferences.
- Release unpaused Process List population, resource samples, decoded icons,
  native row strings, and both app-picker catalogs when the window is destroyed.
  Preserve paused snapshots, selections, editor drafts, filters, sorting and
  column preferences.
- Skip Action Log grouping/formatting without a live table and release its
  derived presentation on destruction. Authoritative log entries remain in the
  backend for later viewing and user-selected CSV export.
- Reject old-generation Home, catalog, and Process List completions even after
  reopening, while clearing their pending flags.

**Tray policy changes:** a silently lost icon between events is no longer
discovered by periodic healthy checks. This adopts the former Iced integration's
event-driven registration policy. Explorer restart recovery and known failures
still trigger verification/retries. It is a behavior change, not a free removal
of equivalent work.

The recovery helper and enabled-scheduler backend are unchanged. Their blocked
workers are not a demonstrated source of recurring idle CPU work. Lazy recovery
startup and scheduler-event coalescing are outside this UI cleanup.

## Validation

Default strict Clippy passed, with warnings and unsafe operations in unsafe
functions denied. Default locked tests passed **871 tests, with 18 ignored**.
The `render-smoke,runtime-benchmark` configuration also passed strict Clippy and
**893 tests, with 18 ignored**. Formatting, diff whitespace, legacy-name scan,
architecture ownership checks, and `graphify update .` passed. Native tests
cover healthy timer cancellation, unavailable retries, restart coalescing during
a blocked query, timeouts, late completion, stale timers and teardown. The
resident lifecycle smoke separately checks three warmed destroy/reopen cycles,
six stale-completion batches, retained paused snapshots/preferences/drafts, real
pointer navigation, modal cancellation and coordinated shutdown. It passed twice
using the collector's `CREATE_NO_WINDOW` launcher. An initial forced-hidden
`Start-Process -WindowStyle Hidden` launch failed a pointer-bounds assertion;
it is retained in the evidence and was not a timed benchmark capture. No code
change or trial replacement was used to turn that launch into a pass.

Actual Explorer restart and tray clicks remain separate integration checks.
Synthetic native messages and lifecycle assertions do not establish those results.

## Measurement protocol

The new cohort compares the immediately preceding event-driven Wind build,
this cleanup, and the same frozen Iced fixture. Three balanced blocks put each
condition in each ordinal position once: baseline/new/Iced, new/Iced/baseline,
Iced/baseline/new. Each capture uses 15 seconds warmup and 60 seconds at 1 Hz,
with a 78-second ready-relative fixture lifetime. Application plus recovery
helper totals exclude the separately measured sampler.

No builds, extra command launches or parallel agent tool work occurred during
the timed cohort. Wind's completion fixture uses one sleeping worker; Iced uses
an asynchronous deadline subscription. The ready-relative deadline guard allows
for warmup and sampling, but readiness polling/setup overhead is not timestamped
exactly.

These are cold Home-to-background measurements with automation, updates and
startup registration disabled. They measure the idle floor, not enabled
scheduler cost or populated-cache savings. Wind destroys its UI HWND; Iced
retains a hidden HWND. Both use software rendering. No GPU, energy, retired
instructions, per-thread stacks, or physical CPU-cycle attribution is inferred.

All nine captures passed required-counter, fixture, native-window-state,
one-helper, clean-exit and cleanup validation. An independent recomputation
matched all 36 scope rows and three paired comparisons to the raw JSON/CSV.
All nine captures remain included: no outlier removal, replacement captures,
pooling with pilots, or pooling with previous cohorts. A restricted-desktop pilot
failed `EnumWindows`; a separate desktop-access pilot then verified the launch
and tray/window state before the prespecified cohort began.

## Results

These are medians of three captures. Each memory value first takes the median
within its capture. Private resident RAM is private working set; private commit
is a separate allocation metric. Totals include the recovery helper.

| Condition | Cycles/s | Cycles/s range | Private resident, MiB | Private commit, MiB | Threads |
| --- | ---: | ---: | ---: | ---: | ---: |
| Wind: immediate baseline | 276,937 | 221,457–277,515 | 6.26 | 14.48 | 24 |
| Wind: this cleanup | 61,852 | 38,089–71,629 | 6.18 | 14.45 | 22 |
| Iced: frozen fixture | 30,067 | 25,741–60,544 | 11.97 | 20.76 | 35 |

| Block | Baseline Wind cycles/s | New Wind cycles/s | Iced cycles/s | Wind reduction from baseline | New Wind minus Iced cycles/s |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 276,937 | 38,089 | 25,741 | 86.25% | +12,347 |
| 2 | 277,515 | 71,629 | 30,067 | 74.19% | +41,562 |
| 3 | 221,457 | 61,852 | 60,544 | 72.07% | +1,309 |

The **74.19%** is the median of the three within-block percentage reductions,
not a percentage calculated from the two condition medians. New Wind exceeded
Iced by 47.97%, 138.23%, and 2.16% in their respective blocks. These observations
do not establish a framework-wide CPU or energy ranking.

The main Wind process recorded nonzero cycle deltas in **1, 3, and 1** of the
60 measurement intervals, versus **13, 12, and 12** before this cleanup. Iced
recorded **3, 3, and 4**. The recurring five-second pattern is gone; aggregate
counters cannot assign every remaining cycle to a function or thread. Main
process cycle medians were 250,426 before, 34,654 after, and 9,972 for Iced.
Helper medians were 20,808, 27,200, and 20,096 respectively. Component medians
need not sum to the combined median.

The cleanup removed two threads in every block. Its paired resident-memory
changes were −0.125, −0.094, and +0.027 MiB; private-commit changes were +0.461,
−0.129, and −0.027 MiB. Those small, mixed changes are not evidence of a material
RAM improvement. Populated Process List/icon/picker cleanup was checked in the
native lifecycle harness, but its memory savings were not benchmarked here.

## Interpretation and limits

This was a shared live desktop: per-capture median machine CPU ranged
**11.26–25.05%**, and median physical-memory load **84–92%**. The largest
per-capture p95 values were 54.54% CPU and 96% memory load. The sampler itself
used 2.00–2.81% of one core; it is reported separately, not subtracted. It can
still perturb scheduling and residency. Three repetitions provide descriptive
comparisons, not statistical significance or a quiet-machine idle floor.

CPU-time rates mostly quantized to zero despite positive cycle counts. A zero
CPU-time delta does not mean zero work. Windows process-cycle accounting is not
retired instructions, elapsed CPU time, or energy. No GPU, watts, package-state,
hardware-event, enabled-scheduler, suspend/resume, or long-term-growth result is
claimed by this cohort.

Wind uses native blocking/event delivery successfully. This fix addresses
application-owned polling and presentation lifetimes; it does not justify a
framework fork or promise that Wind will surpass Iced. The remaining small idle
cycle difference needs thread/stack attribution before further changes to
blocked workers or the recovery helper would be justified.

## Reproducible evidence

Public exports are [summary JSON](../benchmark-results/archive-20261028/benchmark/ui-native-idle-cleanup-results-20261022/summary.json),
[scope rows](../benchmark-results/archive-20261028/benchmark/ui-native-idle-cleanup-results-20261022/trials.csv),
[paired comparisons](../benchmark-results/archive-20261028/benchmark/ui-native-idle-cleanup-results-20261022/pairs.csv), and
[host context](../benchmark-results/archive-20261028/benchmark/ui-native-idle-cleanup-results-20261022/host-context.csv).
They are byte-identical to the independently verified analysis outputs.

Raw captures, configuration, analyzer, frozen binaries, logs and 164-file source
provenance are kept in `target/native-idle-cleanup-20261022/`. The default
production executable is a separate build without fixture features; measured
results refer to the feature-enabled fixture identified below.

The normal release build passed `cargo build --release --locked --offline`.
The 12,201,472-byte executable is `target/release/winderust.exe`, also preserved
under `production/` in the evidence directory. No Winderust processes remained
after validation, no settings file was created beside either production copy,
and the machine retained its original Balanced power plan.

Reproduce collection and analysis from this checkout:

```powershell
python -B scripts/measure_ui_resources.py --config target/native-idle-cleanup-20261022/config.json --out target/native-idle-cleanup-20261022/capture-reproduction --warmup 15 --duration 60 --interval 1 --trials 1 --startup-timeout 30 --no-gpu --memory-ex2
python -B target/native-idle-cleanup-20261022/analyze.py
```

The analyzer validates the original `capture/` directory; use a separate evidence
root/config when analyzing a reproduction rather than overwriting original data.

| Input | SHA-256 |
| --- | --- |
| Baseline Wind fixture | `41dc6057c8bc77ef171d04d2555b934b905e29dd24e455b649a5cdca16e6e011` |
| Cleanup Wind fixture | `e5e825397dd4417e292f39a2b300c1079dae458a4f10fea1c74cf26093cb51d6` |
| Normal production executable | `5c7dcfd5042d3b05188052b51a6ada74ca0c1b7fc4222839ebf529f1600b391c` |
| Frozen Iced fixture | `2d040026dae4b842a3853f5503817a15a00abd097f24e4a681749aa6976d2b82` |
| Unchanged collector | `98e473b2858c28e5a20f03dc71980a733f766916ee5ce7ad582e7182f9905946` |
| Prespecified configuration | `f7cbd2105ef5ba799ba18f39b0346534400d0425738ae1acac1b890af2361e36` |
| Analyzer | `ab60c01ab24e2b429e2418219df67d92ab8496aa91c3e8b9cc7584ec8ec04094` |
| Source provenance manifest | `783674d443265de3f48eb2d226bfcefd399293aaa87e08f0db98f62316d84a1c` |
