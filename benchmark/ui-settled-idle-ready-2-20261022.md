# Fresh long-settled attempt after user readiness

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

The second quiet-host attempt launched **zero Wind applications and zero Iced
applications**. System commitment was lower after the readiness reply and passed
its gate, but CPU utilization remained above the frozen limits for the
entire five-minute wait. No new application cycle, CPU, or memory result exists.

This attempt followed the user's explicit readiness reply. It uses a new
directory and protocol identity; it does not resume, overwrite, or pool data with
the [first failed attempt](ui-settled-idle-comparison-20261022.md).

## Protocol and outcome

The method is unchanged: four matched Wind/Iced pairs ordered AB/BA/AB/BA, with
180 seconds settling after readiness and 120 seconds measurement at 1 Hz per
application, using fixed binaries and collector. Fixtures have automation off,
isolated in-memory settings, and the real recovery helper. Wind's native UI
window is absent; Iced's is hidden. The comparison would describe the two
application integrations, rather than isolate framework overhead.

Each pair requires a complete host window of at least 60 actual seconds, CPU
mean at most 5% of the machine, CPU p95 at most 10%, minimum physical availability
at least the greater of 1 GiB or 10% of total RAM, and maximum system commit at
most 85% of its limit. Each gate waits at most 300 seconds and does not relax its
limits or select a quieter subset afterward. No ETW, GPU, energy, or thread
polling was enabled. No unnecessary applications or services were stopped.

The attempt ran **2026-10-22 15:56:37–16:01:37 UTC**, crossing midnight into
October 23 in Taipei. All 300 host readings are preserved: 299 intervals and 240
rolling 61-reading windows, of which 239 span at least 60 seconds. Every complete
window fails both CPU limits. Every complete window passes the physical RAM and
system commit limits.

| Last complete window | Observed | Required |
| --- | ---: | ---: |
| Actual duration | 60.3444163 s | At least 60 s |
| CPU mean, whole machine | 12.994% | At most 5% |
| CPU p95, whole machine | 18.077% | At most 10% |
| Maximum system commit | 40.156% | At most 85% |
| Minimum available physical RAM | 2,697,605,120 bytes | At least 1,676,380,569.6 bytes |

Across complete windows, CPU means range from 10.466% to 13.930%, and CPU p95
from 16.765% to 25.307%. System commit window maxima range from 40.115% to 40.224%.
The last terminal window is shorter, 59.6314109 seconds, because the deadline
shortened its last interval. The runner's actual-duration check correctly
rejects that partial window; the preceding complete window also fails CPU.
The preserved historical source comment about all intervals spanning at least
one second remains imprecise at the deadline.

System commit is `GetPerformanceInfo`'s `CommitTotal / CommitLimit`, separate from
physical RAM residency. CPU uses `GetSystemTimes` deltas. Official definitions
are linked in the [protocol](../benchmark-results/archive-20261028/benchmark/ui-settled-idle-ready-2-results-20261022/protocol.json)
and the [first attempt's method](ui-settled-idle-comparison-20261022.md).

## Read-only host diagnostic

After the gate stopped, a separate 10.117-second accessible-process CPU sample
found several VS Code processes consuming about 4.75% of the 12-logical-processor
machine, and two Logitech processes about 1.88%. Those values are CPU-time
deltas normalized by elapsed time and logical processor count. Their working
sets are recorded separately and do not establish CPU use.

This sample occurred after the gate, so it cannot exactly attribute the gate's
earlier CPU load. Protected processes, short-lived processes, interrupts, and
other kernel work may be missing. The sample neither explains all machine load
nor identifies a Wind UI defect. No processes were stopped. A new comparison
under the current load would need its own explicitly identified protocol and
would retain the host-noise limitation.

## Evidence and validation

The [evidence bundle](../benchmark-results/archive-20261028/benchmark/ui-settled-idle-ready-2-results-20261022/README.md) retains
the frozen protocol, configuration, runner, analyzer, collector source, readiness
provenance, run metadata, gate summary, and every raw host reading. It includes
an offline verifier and checksum inventory; the later host diagnostic is labeled
separately from gate evidence. There are no application performance CSVs or
synthetic zero-valued application measurements.

All 19 pure runner/analyzer checks and independent preflight review passed.
Independent postflight review recomputed every rolling window and native
CPU/commit conversion, and verified frozen file, collector, binary, and evidence
hashes. The capture directory is empty, pair-collector count is zero, and
accepted application observation count is zero. The active Balanced power plan
remained `381b4222-f694-41f0-9685-ff5bb260df2e`.

The frozen protocol SHA-256 is
`4459c26d2926bc22c54329b35d0c49aeb09c127471c2d70609735dce4cf626a6`.
Runner, analyzer, configuration, collector, and both fixture binaries are
unchanged from the first attempt. No product Rust code or Wind UI dependency
source changed.

This attempt does not establish Wind's idle floor, parity with Iced, or an energy
advantage. The long-settled application comparison remains unmeasured.
