# Wind UI and Iced: tray-resident enabled-engine comparison

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

Measured on 2026-10-27. The accepted comparison ran from 10:19:53 to
11:03:45 UTC; separate actuation checks finished at 11:04:48 UTC.

**There is a tradeoff, not an overall framework winner.** In this cohort,
Iced used 2.92% and 4.00% fewer process cycles in the two matched pairs.
The Wind fixture used less private memory, but Iced's additional prewarm PNG
capture can affect allocator retention, so this is not a clean measurement of
the frameworks' minimum memory requirements. Whole-package energy does not
establish either application's power consumption.

Both builds demonstrated the background architecture: render Home, destroy the
settings window, retain a real tray icon and an enabled scheduler, then clean up
and exit. Separate checks demonstrated a real Process Priority change and
restoration with both builds. These capabilities are therefore available with
Iced as well as Wind UI; Wind's more integrated Windows facilities are a
convenience and maintenance consideration, not an exclusive capability.

## What was held equal

| Item | Wind UI | Iced |
| --- | --- | --- |
| Framework version | 0.28.0, revision `d5c105067d7a31f0f14e388f1fe6394f1a043988` | 0.14.0 |
| Renderer | tiny-skia software | tiny-skia software |
| Engine sources | 112 byte-identical shared files | Same 112 files |
| Release build | Rust 1.94.1, thin LTO, one codegen unit, stripped symbols | Same compiler/profile |
| Persistent native tray | Registered icon, hidden host, same grouped menu actions | Registered icon, hidden host, same grouped menu actions |
| Settings history | Home opened before background readiness; settings window destroyed | Home opened, rendered, PNG captured, settings window destroyed |
| Enabled feature | Process Priority | Process Priority |
| Check interval | 1,000 ms | 1,000 ms |
| Rule | One exact, nonexistent fixture executable; Below Normal when matched | Same serialized settings |
| External worker processes during performance captures | Zero | Zero |
| Measured process scope | Main plus all observed recovery helpers | Same scope |
| Exit assistance | None | None |

The rule target was deliberately absent. This measures the absolute cost of an
enabled, idle scheduler checking for work. It does not measure the incremental
cost of enabling that scheduler, successful-actuation overhead, or every
Winderust scheduler. The later actuation checks are a separate capability test.

Settings stayed in memory; startup registration and update checks were disabled.
The fixture retained normal process identity, protection, access, restoration,
and recovery checks. No user application was targeted.

The host was an Intel Core 5 210H with 12 logical processors and approximately
16 GB RAM, Windows build 26200, on AC with the Balanced plan. The user had
authorized comparison under the current host load. This was not a quiet-host,
reboot-controlled or battery test. These are the pinned application builds,
not a measurement of Wind UI 0.30 or every possible renderer configuration.

## Fixed protocol

Two excluded pilots had to pass before collection. Formal order was
Wind → Iced → Iced → Wind. Each formal trial had a 300-second warm-up,
180-second capture, 1-second sampling interval and 48-second fixture cleanup
margin. Three-minute no-app package-energy baselines bracketed the four trials.

All four formal captures passed, each with 181 samples and 180 valid energy
intervals. All three required energy counters succeeded throughout, and AC
remained online. Frozen binaries and inputs were checked before each child
launch. No force termination, injected wake, retry, outlier removal or
post-hoc extension was used in the accepted cohort.

The sampler independently checked the tray host's PID, thread, class, hidden
state and registered icon rectangle. Startup evidence had to include a real
visible settings window; a disappearing startup handle could not satisfy this
proof. The settings window had to remain absent during measurement. Runtime
worker state, unchanged settings, error counts, helper completion and native
tray removal were acceptance gates.

## CPU and memory results

Values include the main process and observed recovery helper. Memory figures
are the median of the combined per-sample values. MiB means 1,048,576 bytes.
The sampler and the separate actuation workers are excluded.

| Metric | Wind 1 | Iced 1 | Iced 2 | Wind 2 |
| --- | ---: | ---: | ---: | ---: |
| Cycles per second, millions | 17.748 | 17.230 | 17.339 | 18.062 |
| CPU time, % of one logical CPU | 0.6771% | 0.6684% | 0.6423% | 0.6770% |
| Private commit, MiB | 18.418 | 20.301 | 20.391 | 18.680 |
| Private resident memory, MiB | 6.359 | 7.270 | 7.563 | 6.359 |
| Total working set, MiB | 42.508 | 45.199 | 45.219 | 42.516 |
| Handles, median | 498 | 540 | 540 | 498 |
| Threads, median | 21 | 33 | 33 | 22 |

Iced used 2.92% fewer cycles than Wind in the first pair and 4.00% fewer in
the reverse-order pair; the duration-weighted aggregate was 3.46% fewer. That is approximately 0.52–0.72 million fewer cycles
per second in this enabled-no-match workload. These are OS process cycle
counters, not instructions retired or an energy measurement. Four trials are
descriptive evidence; they do not establish a universal framework advantage.

Wind's measured private commit was 1.883 MiB and 1.711 MiB lower in the pairs.
Its private resident memory was 0.910 MiB and 1.203 MiB lower. Private commit
and resident memory describe different things; total working-set sums can also
double-count shared pages.

Host memory load was 86–87% during the formal captures, with later trials
spending more time at 87%. Resident-memory values therefore also reflect
substantial, changing memory pressure.

**Memory caveat:** only the Iced prewarm path encoded a PNG to prove completed
rendering. Its RGBA buffer was released before readiness and the five-minute
warm-up, but allocator/cache retention may persist. This instrumentation can
affect a difference of this size. Attribute these values to the instrumented
fixtures; a control without asymmetric PNG work is needed before crediting the
whole memory difference to the framework.

## Package energy: context, not application watts

| Capture | Average CPU-package watts |
| --- | ---: |
| No-app baseline before | 8.626 |
| Wind 1 | 8.836 |
| Iced 1 | 8.577 |
| Iced 2 | 8.675 |
| Wind 2 | 8.722 |
| No-app baseline after | 8.829 |

The package differences were 0.259 W and 0.046 W in Iced's direction.
The no-app baseline itself changed by 0.203 W, exceeding the aggregate
0.153 W package gap. Host CPU load averaged
7.77–8.35% of the machine during the application captures, and sampler CPU
cost was approximately 1.61–1.72% of one logical CPU. Other software and
measurement overhead are included in package energy.

The sampler itself used about 43.80–45.14 million cycles per second, separately
from the application's counters. Its cost is larger than the measured app's
CPU cost, further limiting attribution from the package meter.

There is no justified per-app power winner from these readings. No baseline
subtraction, per-app watts, battery-life prediction, discrete-GPU energy or
wall-power estimate is reported.

## Other observed counters

There were no read/write byte or operation increments in the formal process
I/O counters. Other I/O averaged approximately 159.6–169.2 operations and
7.99–8.47 kB per second. These counters do not identify physical-disk or network
traffic. They do not include startup PNG capture or post-capture result writes.

Process page-fault increments were 27,825 and 21,316 for Wind, and 26,612 and
20,436 for Iced. These include soft faults; they are not measurements of disk
faults. Their variation is a reason to inspect shared-engine allocation and
process-enumeration work in a separate trace, not evidence of a framework cause.

## Real actuation and restoration

After the accepted performance cohort, each build ran a separate 30-second
fixture against one fresh, identity-checked disposable process. Both checks
passed. The checker required a native Below Normal process priority while the
feature reported a successful adjustment, then verified the original native
properties on the same process after application cleanup. Both native priority
sequences were Normal (`32`) → Below Normal (`16384`) → Normal (`32`), with
affinity and throttling fields unchanged. Only after successful
restoration did it request the worker's cooperative stop and require its normal
exit. Recovery helper completion was also checked.

All eight application runs in the accepted comparison and actuation checks
exited with code zero. Both actuation workers and their explicitly tracked
recovery helpers also exited with code zero. No manual restoration was used.

These checks validate the shared engine's ability to act and restore under
both adapters. They do not supply CPU, memory or energy measurements for active
actuation, nor do they validate every feature or setting.

The final native inventory at 11:06:39 UTC found no Winderust or benchmark worker
processes. The active plan remained Balanced, matching the pre-run inventory.

## Iced exit finding and fix

The first tray-capable Iced pilot failed the unassisted-exit gate. Its runtime
cleanup began about 61.020 seconds after readiness and took 13.849 ms, but the
process's total runtime was 361.106 seconds. Thus the scheduler stopped and
restored on time; the delay was afterward, before the daemon loop returned.
It eventually exited without an injected wake. The failed attempt remains
excluded from all comparisons.

The new isolated Iced adapter dispatches daemon exit through:

```rust
Task::done(()).then(|()| iced::exit())
```

The pinned-task contract test verified Pending → one Exit action → end, whereas
the immediate exit task was ready on its first poll. The candidate passed its
fresh pilot, both formal trials and separate actuation check without external
wakes. Its new pilot completed in 61.833 seconds, and its two formal runs in
528.647 and 528.607 seconds for 528-second fixture durations. These totals
include startup and teardown; they are not direct exit-wait latency measurements.
It adds no native wake message, recurring timer, polling loop, dependency
patch or engine change. Only `src/ui/app.rs` differs from the previous frozen
tray-capable Iced source.

The source-supported explanation is an exit/wait ordering hazard: Iced can
process immediate actions during AboutToWait, while pinned Windows winit can
enter its wait after that callback before checking the exit flag again.
Deferring the action routes it through Iced's event proxy. This is a supported
explanation and a validated adapter remedy, not a runtime stack trace proving
the exact blocked instruction. See the pinned
[Iced daemon contract](https://github.com/iced-rs/iced/blob/0.14.0/src/daemon.rs#L19),
[task yield](https://github.com/iced-rs/iced/blob/0.14.0/runtime/src/task.rs#L537),
[proxy forwarding](https://github.com/iced-rs/iced/blob/0.14.0/winit/src/proxy.rs#L40)
and [winit Windows wait ordering](https://github.com/rust-windowing/winit/blob/v0.30.13/src/platform_impl/windows/event_loop.rs#L370).

No Wind UI dependency source or production Winderust code was changed for this
experiment. The new adapter and harness sources are isolated benchmark inputs.

## Preserved failed attempts

Three earlier cohorts are preserved separately and contribute zero formal
measurements:

1. Original harness: Wind completed its excluded pilot, but a reader wrongly
   required the missing rule path to use a double-quoted TOML string. The actual
   literal string was valid. The reader fix retained exact path/single-rule
   validation and added regressions.
2. Literal-string fix: Wind's transient settings HWND disappeared between
   enumeration and an ownership query. The sampler stopped. A startup-only fix
   skips an unresolved entry only when IsWindow proves it gone; discarded
   entries never count as positive startup proof. The old failure did not
   retrospectively record enough information to prove destruction as its cause.
3. Window-race fix: Wind passed; Iced completed runtime cleanup on time but
   returned from its daemon loop late. No formal trial ran. The public exit-task
   candidate was built separately before the accepted cohort.

No old capture was reclassified as accepted or pooled with the new cohort.

## Validation and practical conclusions

The final candidate passed seven focused fixture tests, including the actual
public-task ordering test, the release build, formatting and whitespace checks.
Its strict Clippy run retained two inherited unused-code warnings; the explicitly
labeled allowance for those warnings passed the remaining strict checks.
The final sampler passed 13 tests; the actuation checker passed nine.
The unchanged production tree had also passed strict locked Clippy, formatting,
885 tests with 18 ignored, legacy/architecture scans and graphify update during
this work. Those production checks are distinct from the Iced fixture checks.

For Winderust's stated priorities:

- **CPU cycles:** this matched enabled-engine cohort favors Iced modestly. It
  does not support a claim that Wind UI inherently uses fewer background cycles.
- **Memory:** the Wind fixture measured lower, with fewer handles and threads;
  asymmetric PNG work limits attribution of the memory difference.
- **Power:** unresolved by these whole-package readings.
- **Background functionality:** both demonstrated a persistent tray with no
  settings window, a running scheduler, real actuation, restoration and cleanup.
  Iced required adapter integration and the public exit-dispatch fix.

Optimizing the shared engine should be evaluated separately from choosing a UI
framework. This cohort does not isolate incremental scheduler overhead or prove
that one framework is best for all background features.

Tray menu clicks, Explorer restart recovery, Narrator/keyboard/IME/DPI behavior,
all schedulers, active-actuation performance, GPU usage and wall/battery energy
were not tested in this cohort. Do not infer those results from source/menu
parity or successful Process Priority checks.

## Evidence and reproduction

[Download the raw evidence and replay scripts](../benchmark-results/archive-20261028/benchmark/ui-tray-engine-parity-results-20261027.zip)
and [archive checksum](../benchmark-results/archive-20261028/benchmark/ui-tray-engine-parity-results-20261027.zip.sha256).
The archive contains 592 payload files plus its manifest, including the accepted
raw captures, native lifecycle and actuation evidence, independent calculations,
frozen input snapshots, selected adapter sources, validation records and all
three excluded attempts. It excludes executables, libraries and build caches;
it is a replay/audit bundle rather than a standalone application rebuild.

A fresh extracted replay accepted all four trials and reproduced the analysis
apart from declared CSV locations. Its 13 harness tests and three integrity tests
passed under the audit guard, and the manifest remained unchanged. The root
also independently checked the final ZIP's hash, CRC and executable exclusions.

| Identity | Value |
| --- | --- |
| Archive bytes | 2,985,400 |
| Archive SHA256 | `cd75378ab1ed4f9c15b69f148021436a0305638796c7a354707f937ec7ca53e4` |
| Payload manifest SHA256 | `e76fb81097e5f3c778ee331e323d02efdf9bac841c0620a4bec76c3cafdd79d1` |
| Wind executable SHA256 | `c6e4bcfcc3846e6c2a715f27f67007a287abc043c68040cb2e76a9fe3736f2ea` |
| Iced executable SHA256 | `293efcbbe2d1e461ce778831c6935a81cc3b9f0f47046ba68b157509c13c8dd9` |
