# Current-source background CPU and RAM comparison - 29 October 2026

Fresh release fixtures built from the current Wind UI application and the
shared-engine-matched Iced application show a tradeoff: **Wind UI used less
memory and fewer threads; Iced used fewer process cycles in both pairs**.
Iced's aggregate cycle rate was 7.40% lower. CPU-time direction reversed
between pairs, and no power or energy was measured. This is two pairs under
changing host load, not a universal framework ranking.

The [raw evidence directory](../benchmark-results/current-version-background-20261029/)
is local, ignored by Git and excluded from release artifacts. Replay requires
that separate evidence. The [frozen protocol and captures](../benchmark-results/current-version-background-20261029/protocol/),
[independent calculations](../benchmark-results/current-version-background-20261029/independent-review/formal-cohort-summary.json)
and [verification record](../benchmark-results/current-version-background-20261029/verification.json)
retain inputs, failures, hashes and validation results.

Both variants ran the same 112 byte-identical shared-engine files, software
rendering, a persistent native tray, and Home-window history. The settings
window was destroyed before sampling. Only Process Priority was enabled, with
one deliberately nonexistent exact-path rule and no external target workers.
Settings remained in memory; cross-session control, updates and startup
registration were off. A configured one-second interval is subject to the
shared engine's **ten-second hidden periodic floor**; events can trigger extra
passes. This did not measure one scheduler pass per second, active-target
control, every scheduler, or incremental cost over an engine-off baseline.

Two excluded short pilots preceded the fixed **Wind / Iced / Iced / Wind**
formal order. Each formal trial settled for 300 seconds, sampled for 180
seconds at 1 Hz, and reserved 48 seconds for ordinary shutdown. Every trial
retained 181 observations and 180 valid intervals: 724 observations and 720
intervals total. No trial was extended, discarded or added based on its result.
The coordinator exited normally at 06:28:20 UTC (14:28:20 Taipei).

Application figures include the main process and its identified recovery
helper: **two processes for each framework**. CPU percentage uses one logical
processor as 100%. Memory values below are medians of the combined sampled
gauges, in MiB. Private commit and physically resident private memory are
different measures; summed total working sets may double-count shared pages.

| Trial | Framework | Million cycles/s | CPU % of one core | Private commit MiB | Private resident MiB | Total working set MiB | Threads / handles p50 |
|---|---|---:|---:|---:|---:|---:|---:|
| 1 | Wind UI | 10.368540 | 0.381948 | 19.089844 | 6.746094 | 44.480469 | 22 / 515 |
| 2 | Iced | 9.805263 | 0.295123 | 24.738281 | 11.929688 | 51.812500 | 35 / 650 |
| 3 | Iced | 7.132241 | 0.277773 | 24.527344 | 11.792969 | 51.699219 | 34 / 654 |
| 4 | Wind UI | 7.922829 | 0.269077 | 19.585938 | 6.785156 | 44.921875 | 21 / 515 |

Iced used 5.43% fewer cycles in pair 1 and 9.98% fewer in pair 2, relative
to Wind. Equivalently, Wind's rate was 5.74% and 11.08% higher than Iced's.
The duration-weighted totals within this fresh cohort are:

| Framework | Total cycles | Observed exposure s | Million cycles/s | CPU time s | CPU % of one core |
|---|---:|---:|---:|---:|---:|
| Wind UI | 3,292,537,078 | 360.0119481 | 9.145633 | 1.171875 | 0.325510 |
| Iced | 3,048,871,281 | 360.0131606 | 8.468777 | 1.031250 | 0.286448 |

Rates divide summed integer counter increments by summed observed intervals;
the per-sample denominator is the maximum interval among that sample's
process members. They do not assume exactly 180 seconds or average rounded
trial rates. Wind used 0.156250 more CPU seconds in pair 1 and 0.015625 fewer
in pair 2. Its aggregate CPU-time excess was 0.140625 seconds over roughly
360 seconds of exposure, about 0.0391 percentage points of one core. Process
cycles and CPU time are distinct OS counters; neither measures joules.

Wind's private resident memory was lower by 5.183594 and 5.007813 MiB
(43.45% and 42.46%) in the two pairs. Private commit was lower by 5.648438
and 4.941406 MiB. These are modest absolute memory savings in the exercised
fixtures. They do not establish the cause of every allocation or a universal
framework memory cost.

The old Iced-only Home screenshot, RGBA capture and PNG encoding were removed
from this isolated Iced benchmark adapter before building. Neither new fixture
captures or encodes a prewarm image. Native external observations require each
app's own HWND/PID/TID/class, visible and unminimized Home window, usable client
and window geometry, and actual destruction before readiness. Iced additionally
records valid endpoint observations across a two-second asynchronous hold.
These checks establish native layout/visibility observations, not first paint
or uninterrupted visibility. Historical captures are not pooled, and their
memory values are not used to estimate an improvement caused by this change.

The additional observed counters were:

| Trial | Read / write bytes | Other I/O bytes / operations | Page faults | Host CPU % | Host memory load % | Sampler CPU s |
|---|---:|---:|---:|---:|---:|---:|
| Wind 1 | 0 / 0 | 5,282 / 114 | 37,688 | 14.8676 | 77-83 | 4.140625 |
| Iced 1 | 0 / 0 | 5,916 / 120 | 33,804 | 18.6887 | 90-91 | 5.765625 |
| Iced 2 | 0 / 0 | 5,398 / 114 | 22,832 | 19.6772 | 90-92 | 6.468750 |
| Wind 2 | 0 / 0 | 5,004 / 108 | 26,035 | 19.2325 | 88-89 | 6.250000 |

Other I/O does not identify physical disk traffic; page faults include soft
faults. The sampler's CPU time and cycles are excluded from app totals and
are not subtracted from them. Its collection median/max times were
23.0144/36.5649, 33.5139/44.4672, 35.0374/45.4028 and 34.5411/46.5855 ms.
App main/helper cycle totals were 3,288,005,914/4,531,164 for Wind and
3,046,953,125/1,918,156 for Iced. Per-member identities, counters, interval
bounds and sampler cycles remain in the independent raw review; p95/max
gauges also remain in the raw capture and complete records.

The host was Windows build 26200 on an Intel Core 5 210H, 12 logical
processors and about 16 GiB RAM, with AC online and Balanced selected. This
used the previously authorized current-load policy. Machine CPU and memory
pressure varied substantially, particularly between Wind 1 and the later
trials. Host work, instrumentation and residency effects remain confounders;
two pairs do not establish statistical significance or equivalence.

All four trials passed full file-only replay and independent lifecycle review.
The independent raw calculation matched all 112 complete-record crosschecks
and each member delta and combined sample sum. Each exact hidden tray
HWND/PID/TID remained alive throughout its capture, with 184 owner checks and
successful `Shell_NotifyIconGetRect` before and after measurement. Settings
windows and rule targets were absent throughout sampling. The runtime worker
was running at readiness and measurement end, with identical settings hashes
and zero no-match adjustments/failures. Ordinary exit then reported restoration,
no cleanup errors, exit 0, and absent tray host/icon. All observed recovery
helpers exited normally. There was no external wake, injected exit or forced
termination. No remaining Winderust or dedicated-worker process names were
found after the cohort, and Balanced remained selected. CIM enumeration was
denied; the final inventory used read-only `Get-Process` enumeration instead.
This is not a fresh active-target restoration, click-navigation, accessibility
or Explorer-restart test.

Both fresh fixtures use Rust 1.94.1, locked dependencies, thin LTO, one codegen
unit, stripped symbols and `render-smoke,runtime-benchmark` features. Wind
snapshots the current application, including the navigation-capture fix;
Iced is the preserved 0.14.0 application with the same current shared engine.
The two-file Iced adapter change and native prewarm/exit wiring were separately
reviewed. There is no new production Iced port or upstream framework patch.
These are instrumented release benchmark executables, not measurements of the
two default-feature production executables. Benchmark-only initialization,
deadline and validation paths may add overhead; the measurements cannot be
assumed to equal the normal shipping executables exactly.

| Fixture | Framework / renderer | Executable bytes | SHA-256 |
|---|---|---:|---|
| Current Wind | Wind UI 0.28.0, Tiny-skia software | 13,069,312 | `27afc18910dd07b55092b32571e73bc2bebfae52b2230b1e6f2b901e5654d050` |
| Matched Iced | Iced 0.14.0, Tiny-skia software | 19,608,064 | `6172bc42ea05560d782c94038972f79cbf91aa9ba7d03f811af6d513d92013cf` |

The Wind fixture executable is about 33.35% smaller. This compares matching
feature-enabled benchmark binaries, not a normal release against a fixture.
Wind UI remains pinned to revision
`d5c105067d7a31f0f14e388f1fe6394f1a043988`; dependencies were not upgraded.
All 168 current Wind source inputs match their built snapshot. The user's
staged diff remained unchanged; only this maintained report and the benchmark
index were added as unstaged documentation. The existing normal release
executable remained 12,605,952 bytes, SHA-256
`0e057080a4a90e687409ae9d1cc47e2f55371c589ee7bc03d843251b38e4ef25`.

Production formatting, strict all-target Clippy, architecture/legacy checks
and tests passed (911 passed, 19 ignored). The Wind feature build also passed
strict Clippy. Iced's isolated baseline passed with only its inherited
`dead_code` allowance for `TrayVisibilityWatcher` and `start`; its four
benchmark tests and native owner/menu/exit contracts passed. The frozen
protocol's 20 pure tests passed. A failed prelaunch binary-marker check and an initial
UAC launch initialization failure occurred before any fixture application
launched; neither is an excluded performance trial. Their setup records are
retained separately from the two planned pilots and four accepted trials.

Temporary build caches created only for this study were removed after replay
and identity verification, freeing 2,251,557,810 bytes (about 2.10 GiB).
Source snapshots, fixture executables, build logs, frozen harness, raw captures
and validation remain under the ignored evidence directory. Historical evidence
was left untouched.

GPU, per-thread context switches and system energy collection were disabled.
There are no fresh watts, battery-life or application power-attribution results.
For this current-source hidden no-match workload, Wind's measured advantage is
memory, threads and handles, while Iced's is process cycles. Neither result alone
establishes which framework saves more power.

To replay locally with the retained evidence:

```powershell
python -B benchmark-results/current-version-background-20261029/protocol/verify_tray_engine.py --cohort benchmark-results/current-version-background-20261029/protocol --output benchmark-results/current-version-background-20261029/protocol/validation.json
```
