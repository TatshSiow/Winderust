# Hidden idle event-registration comparison — 2026-10-22

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

The narrower Windows event listener uses fewer resources, but this experiment
does **not** establish a repeatable CPU-cycle improvement or parity with Iced.
The candidate used fewer cycles than either comparator in only one of three
matched blocks. Wind's private memory remained lower than Iced's in every block.
This is an application-integration comparison with automation disabled, rather
than a universal framework ranking or a measurement of active schedulers.

## Change and rationale

The preceding [ETW investigation](ui-idle-etw-20261022.md) found RPC/threadpool
lifecycle activity and tracing overhead; it did not establish a Wind UI renderer
defect. Source inspection separately found that observing Windows appearance
registered desktop-wide foreground/window-creation hooks and power/session
subscriptions whose automation callbacks were discarded when automation was off.
The frozen Iced integration has the same broad registrations, so this candidate
is not an explanation of a framework-specific difference.

`src/backend/windows_events.rs` now has an AppearanceOnly mode retaining the
top-level broadcast window and appearance dispatch. The existing watcher
activation predicate is preserved. Whenever either AC/battery profile needs an
automation worker, a requested watcher retains its complete original automation
registrations, including the A/C transition wake needed by an inactive profile.
Mode changes publish the source inactive before teardown and publish readiness
after successful startup. Full registration validation, partial-failure cleanup
and polling fallback remain intact. No new preference or dependency was added.

Wind UI is the unmodified pinned 0.24.0 source at
`36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2`; no framework source was changed.

## Protocol and acceptance

Nine fresh launches ran from 06:31:28 to 06:43:24 UTC. The predeclared order was:

| Block | First | Second | Third |
| --- | --- | --- | --- |
| 1 | Wind before | Wind candidate | Iced |
| 2 | Wind candidate | Iced | Wind before |
| 3 | Iced | Wind before | Wind candidate |

Each launch used cold Home to background, a 15-second warmup, 60-second sampling
at 1 Hz, and a 78-second readiness-relative fixture lifetime. Automation, update
checks, startup registration and settings I/O were disabled. Wind destroyed its
settings HWND after the production tray permission; Iced retained a hidden
HWND. Both used software rendering and one real recovery helper. Main plus
helper is the primary scope; observer cost is separate and never subtracted.

There was no ETW, GPU/energy counter collection or per-thread polling. Builds,
tests and agent machine tools were paused during capture. Three rotated blocks
balance ordinal position, not all carryover effects. The collector's generic
order description is superseded by the exact frozen target list above.

All nine captures passed executable/config/collector hashes, exact helper
generation and process lifetime checks, native window readiness checks, clean
exit, fixture cleanup and raw-counter reconciliation. Collector exit status was
0. The Balanced power-plan GUID stayed
`381b4222-f694-41f0-9685-ff5bb260df2e`. No observations were removed or pooled with
earlier cohorts. HWND state was checked at readiness and at each 1 Hz sample;
there was no guard between samples. The frozen configuration's limitation
claiming no per-sample HWND check was inaccurate: the unchanged collector does
enumerate and validate windows inside its sampling loop. That historical
configuration is preserved, with this correction stated explicitly.

## Results

Cycles are each trial's sum of native deltas divided by its sum of actual
counter intervals. Combined intervals use the larger member interval. Memory,
threads and handles below are the median of three within-trial sample medians.
RAM values are MiB; private resident memory and private commit are distinct.

| Main + recovery helper | Wind before | Wind candidate | Iced |
| --- | ---: | ---: | ---: |
| Median cycles/second | 89,754 | 84,542 | 74,234 |
| Minimum–maximum cycles/second | 52,857–499,301 | 52,807–164,744 | 49,114–98,023 |
| Private resident memory | 6.13 | 6.05 | 12.38 |
| Private commit | 14.20 | 14.33 | 20.76 |
| Total working set | 42.22 | 40.85 | 49.77 |
| Threads | 22 | 21 | 36 |
| Handles | 527 | 508 | 573 |

The matched blocks do not support claiming the candidate's slightly lower
overall cycle median as a repeatable saving:

| Block | Before cycles/s | Candidate cycles/s | Iced cycles/s | Candidate vs before | Candidate vs Iced |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 499,301 | 52,807 | 98,023 | 89.4% fewer | 46.1% fewer |
| 2 | 52,857 | 84,542 | 49,114 | 59.9% more | 72.1% more |
| 3 | 89,754 | 164,744 | 74,234 | 83.6% more | 121.9% more |

The median paired difference is **31,685 more cycles/s than before** and
**35,429 more than Iced**. These descriptive differences do not prove regression
either, given this small, highly variable shared-host cohort. Pooled rates are
213,966 / 100,694 / 73,793 cycles/s respectively, but the unusually high first
before trial dominates that apparent before/candidate reduction.

The candidate had 15–19 fewer handles and 1–2 fewer median threads than before
in all three blocks. Private commit changed by −0.17, −0.11 and +0.75 MiB, so
there is no consistent new commit saving. Against Iced, candidate private commit
was 5.86–10.35 MiB lower and private resident memory 6.33–9.81 MiB lower in the
matched observations. These are application observations, not isolated renderer
allocations.

Main-process-only cycle medians were 54,653 / 64,617 / 23,483; helper medians were
35,104 / 19,928 / 25,633. Component medians do not necessarily add to the combined
median, and the helper fluctuation cannot be attributed to main-process hook
gating. Median UI readiness was 433 / 579 / 1,054 ms. Startup main-process cycle
medians were 334.91 / 348.78 / 906.74 million; they are cumulative at readiness,
exclude helper startup and are separate from the background measurement window.

## Host conditions and limits

The machine was not quiet: per-trial median host CPU load ranged from 7.78% to
47.27%, and median physical memory load was 93–97%. The first before trial's
private resident median fell to 0.50 MiB while private commit remained 14.50 MiB.
That is evidence of substantial residency variation; it must not be presented
as a successful memory optimization or a proven explanation of all cycle costs.

The observer consumed 3.20–3.65% of one core, around 92–100 million cycles/s.
It is much more active than these idle fixtures and can perturb scheduling and
residency. Main/helper CPU-time counters mostly quantized to zero while process
cycles stayed positive. Neither means zero activity, and cycles are not retired
instructions, watts or battery savings.

The 15+60-second horizon covers some early activity near 30 seconds but ends
before the previously observed thread expiration near 134 seconds. It does not
establish the long-settled idle floor. Enabled schedulers, visible interaction,
populated caches, restoration under active rules and hardware energy were not
remeasured by this cohort. Three blocks do not establish statistical significance.

The useful result is a smaller set of unused registrations and lower observed
resource counts. A claim that this Wind build consistently matches or beats
Iced on hidden CPU cycles remains unproven. A future parity claim needs a quieter
host and a separate longer-settled protocol; repeated trials should not be added
selectively to this completed cohort until a preferred result appears.

## Provenance and validation

| Executable | SHA-256 |
| --- | --- |
| Wind before | `a054975b20fc52bead814b0b42c84125b127a0bd3bcf1dd826ab34ca6f11c2b2` |
| Wind candidate | `35721b77ad06a86b61a6137c68747e593b9950fb455998ce88eba19d0c07f8e1` |
| Frozen Iced | `2d040026dae4b842a3853f5503817a15a00abd097f24e4a681749aa6976d2b82` |

Builds used Rust 1.94.1. Wind before already contains the earlier Adaptive scan
gating, which is unreachable with automation disabled. Candidate fixture build:
`cargo build --locked --release --features render-smoke,runtime-benchmark`.
Collector SHA-256:
`a6a48ce6569f633cf5ee6a175d8c29aaa8087ba2daac7b3351f5fe5d818fae45`.

The analyzer initially rejected the collector's slash-normalized executable
paths. It was corrected to compare resolved paths while requiring every other
target field to match exactly; a different-path regression remains rejected.
No raw measurement, frozen configuration or capture result was changed.
Seven analyzer selfchecks passed; independent recomputation agreed for all 36
trial/scope rows, including native clocks, delta sums and resource medians.

Production checks passed: 885 tests, 18 existing ignored tests, default all-target
Clippy with unsafe-operation warnings denied, formatting, diff whitespace,
legacy-name and architecture scans, and Graphify AST update. Native tests cover
AppearanceOnly → Automation → AppearanceOnly appearance delivery/window cleanup
and watcher availability transitions. Pure predicate regressions cover inactive
AC/battery worker requirements.
A resident-shell test's message-pump race was also corrected; runtime tray
behavior was unchanged. The normal release executable was rebuilt after capture.

The [reproducible result bundle](../benchmark-results/archive-20261028/benchmark/ui-idle-watcher-gating-results-20261022/README.md)
contains all nine observations, analysis, frozen metadata, source provenance,
public counter CSVs, source tools and hashes. Local originals and fixture logs
remain under `target/idle-watcher-gating-20261022/`.
