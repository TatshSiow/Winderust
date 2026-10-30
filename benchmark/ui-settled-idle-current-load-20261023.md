# Wind UI and Iced: long-settled background comparison under current load

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

Completed on **2026-10-23, 09:47:54–10:32:40 Asia/Taipei**. All eight fresh
application trials and four matched pairs completed successfully. Wind used less
private resident memory and private commit in every pair. CPU-cycle results were
mixed: Wind was lower in two pairs and higher in two. This run does not establish
a consistent CPU winner.

The user explicitly selected a separate comparison under the existing host load
after two quiet-host attempts could not launch any trials. This is that separate
cohort; no earlier measurements or failed-gate records are pooled into its results.
The [complete evidence bundle](../benchmark-results/archive-20261028/benchmark/ui-settled-idle-current-load-results-20261023/README.md)
includes raw counters, original logs, frozen policy, analysis and an independent
portable verifier.

## Results

Primary scope is **application plus its actual recovery helper**, excluding the
observer. Each value below is the median of four trial rates, or the median of
four per-trial memory/thread/handle p50s. Brackets show the four-trial range.
MiB means 1,048,576 bytes.

| Metric | Current Wind integration | Frozen Iced integration |
| --- | ---: | ---: |
| CPU cycles/second | **43,436** [32,265–68,343] | **35,683** [34,345–63,877] |
| Private resident memory, MiB | **5.963** [5.875–6.164] | **11.873** [8.512–13.215] |
| Private commit, MiB | **14.188** [13.820–14.637] | **20.332** [20.285–24.859] |
| Summed total working set, MiB | 41.805 | 48.070 |
| Threads | 15 | 27 |
| Handles | 510 | 540 |

Wind's median private resident memory was about 50% lower; its median private
commit was about 30% lower. Total working set sums can count shared pages in both
processes, so private resident memory is the clearer measure of this pair's
resident footprint. Residency also changes with host memory pressure. Private
commit is committed private virtual memory, not all physically resident RAM.

Every paired cycle result is retained below. Positive differences mean Wind used
more cycles; percentages use that pair's Iced rate as the denominator.

| Pair and launch order | Wind cycles/s | Iced cycles/s | Wind minus Iced | Difference, % |
| --- | ---: | ---: | ---: | ---: |
| 1: Wind, Iced | 68,343 | 63,877 | +4,467 | +6.99% |
| 2: Iced, Wind | 53,501 | 34,939 | +18,562 | +53.13% |
| 3: Wind, Iced | 32,265 | 36,427 | −4,162 | −11.43% |
| 4: Iced, Wind | 33,372 | 34,345 | −974 | −2.84% |

The separately pooled combined rates are 46,870 Wind and 42,397 Iced cycles/s.
These divide total cycles by total native interval time, rather than taking the
median of the four trial rates. Neither summary removes the mixed pair outcomes.
See the exact [paired values](../benchmark-results/archive-20261028/benchmark/ui-settled-idle-current-load-results-20261023/analysis/pairs.csv)
and [scope summaries](../benchmark-results/archive-20261028/benchmark/ui-settled-idle-current-load-results-20261023/analysis/trials.csv).

## Sparse activity dominates the cycle averages

The raw intervals give a more useful optimization lead than the averages alone:

- In **each Wind trial**, combined counters increased in only **one of 120
  intervals**. Both main and helper counters stayed unchanged in the other 119.
  All 22,500,172 combined cycles occurred in those four brief bursts.
- Wind's bursts occurred about 119–120 seconds into measurement, alongside
  sampled main thread increases from 9 to 11 or 12 and helper increases from 6
  to 8. Iced's helper had one corresponding burst per trial; its main process
  had 6, 4, 2 and 2 nonzero intervals.
- About **89.96% of Iced's combined cycles** occurred in the corresponding
  thread-rise intervals. Iced 1's thread-rise burst was two samples before its
  final sample. Approximate process age, including launch-to-readiness, was near five
  minutes for both integrations.

**All bursts remain in the primary results.** A 43,436 cycles/s average does not
mean Wind continuously executes that amount of background work every second.
Unchanged sampled counters mean no measured cycle increase for those process
generations over that interval, rather than a universal zero-work guarantee.

Source inspection found no completion command before these counter snapshots.
The collector checks final duration and requests any cooperative stop after its
reads; this cohort used no stop file or interaction messages. The fixtures' ready
relative cleanup deadline was 303 seconds, approximately three seconds after the
last measured sample. Recovery helper code has no 300-second idle timeout. The
Iced 1 thread-rise burst preceding the final sample also argues against a final-sample
only trigger.

The cause remains **unassigned**. A shared lifecycle event, observer perturbation
or other system activity has not been isolated. No stacks were collected here,
so the burst cannot be attributed to Wind UI, RPC, security software or a specific
application function. Earlier ETW evidence at a different process age is not
proof of the cause of this episode. A targeted trace spanning this five-minute
boundary is the next useful investigation before changing production code.

## Other measured aspects

| Scope / metric, median of trial summaries | Wind | Iced |
| --- | ---: | ---: |
| Main process cycles/s | 31,791 | 24,743 |
| Recovery helper cycles/s | 11,613 | 11,908 |
| Main private resident memory, MiB | 3.684 | 9.623 |
| Recovery helper private resident memory, MiB | 2.256 | 1.955 |
| Background-fixture readiness, ms | 425.0 | 690.3 |
| Startup main-process cycles, millions | 324.18 | 776.94 |
| Fixture cleanup duration, ms | 16.18 | 25.66 |

Separate scope medians need not sum to the combined median. Startup readiness
includes fixture setup and observer polling; it is not visible first-pixel
latency or a cold operating-system-cache test. Startup cycle counts exclude
helper startup. Cleanup durations are fixture timestamps, not complete native
process exit latency.

CPU time was too coarse to rank this workload: there were only two nonzero
15.625 ms process-time deltas across 1,920 process intervals. Each integration
recorded 15.625 ms total, with a per-trial CPU-percent median of zero. That
quantization does not contradict the measured cycles or establish equivalence.

No target process recorded read/write operation or byte counter increments.
Other-I/O byte counters increased by 116–174 bytes per combined trial, totaling
580 bytes for each condition. Page-fault deltas totaled 685 Wind and 494 Iced;
these include all faults, not specifically hard faults or disk reads. GPU,
package energy, wakeup events and instructions were not measured in this cohort.

## Protocol and host conditions

- Four fresh matched pairs in Wind/Iced, Iced/Wind, Wind/Iced, Iced/Wind order.
- 180 seconds settling after observed readiness; 120 seconds measurement at
  1 Hz, with 121 snapshots and 120 intervals per process.
- Fresh Home page sent to background, software rendering, automation off,
  in-memory settings, updates off and startup registration off. Wind had no UI
  HWND; Iced retained a hidden UI HWND. These states were checked at 1 Hz.
- The actual recovery helper is part of each result. No synthetic helper,
  workload worker, ETW recording, GPU sampling or per-thread polling was used.
- Before each pair, at least 60 actual seconds of valid CPU readings, available
  physical RAM of at least max(1 GiB, 10% of total), and true system commit no
  more than 85% of its limit were required. Waiting was bounded to five minutes.
  **CPU load was recorded only**, with no quiet-CPU eligibility threshold.
- Combined cumulative counters sum main and helper deltas. Combined elapsed
  time uses the maximum member interval so overlapping processes do not double
  wall time. Rates use native interval timestamps; no observer subtraction.

The four pre-pair gates passed, each with 61 readings spanning 60.41–60.51
seconds. Their machine CPU means were 19.54–22.84%, CPU p95s 27.97–40.47%,
minimum available physical RAM 3.256–3.657 GiB and maximum system commit
39.16–39.50%. Passing that gate does not establish that the machine was quiet
or enforce those memory limits throughout the subsequent pair.

| Trial, in capture order | Machine CPU p50 / p95, % | Physical load p50, % | Available RAM p50, GiB | Observer CPU, % of one core |
| --- | ---: | ---: | ---: | ---: |
| Wind 1 | 19.05 / 28.31 | 76 | 3.740 | 1.979 |
| Iced 1 | 19.05 / 34.83 | 76 | 3.726 | 2.096 |
| Iced 2 | 18.85 / 33.55 | 77 | 3.558 | 2.005 |
| Wind 2 | 20.71 / 43.21 | 76 | 3.594 | 2.031 |
| Wind 3 | 17.95 / 27.55 | 77 | 3.483 | 1.875 |
| Iced 3 | 17.88 / 27.27 | 77 | 3.512 | 1.849 |
| Iced 4 | 18.07 / 26.09 | 77 | 3.465 | 1.823 |
| Wind 4 | 20.41 / 33.35 | 77 | 3.454 | 1.797 |

The observer itself used 53.76–55.11 million cycles/s. Its one-core percentages
and whole-machine CPU percentages use different denominators. Observer cost
remains separate from target costs, and its interference is not eliminated by
excluding its counters from the primary scope. Exact host observations are in
the [host-context CSV](../benchmark-results/archive-20261028/benchmark/ui-settled-idle-current-load-results-20261023/analysis/host-context.csv).

## Identity, validation and limits

This run reused frozen binaries; it did not change product Rust code, dependency
versions or the normal release executable. Wind UI remains the unmodified pinned
0.24.0 revision `36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2`.

| Binary | SHA-256 |
| --- | --- |
| Wind benchmark integration | `35721b77ad06a86b61a6137c68747e593b9950fb455998ce88eba19d0c07f8e1` |
| Frozen Iced integration | `2d040026dae4b842a3853f5503817a15a00abd097f24e4a681749aa6976d2b82` |
| Unchanged normal release | `378ec8e403da97a6b3f458c191bbb55df38b15acb3d931ae445113282eb23ee6` |

The [protocol](../benchmark-results/archive-20261028/benchmark/ui-settled-idle-current-load-results-20261023/protocol.json)
fingerprint is `1f228ce30395fb46867cb88ad2973a67b99ffe53b45786f842f95a2ca6c54ad4`.
The public bundle's manifest fingerprint is
`39663db2a6c97913f2a521e4d7736f248d6ca2efae9b9d2e5a4b82f03ed583d2`.
Binaries are not distributed in that bundle; offline verification checks their
declared identities for consistency, without independently rebuilding them.

All four collectors exited successfully. Native identities, counters, clocks,
window states, helper lifecycles and fixture cleanup passed independent review.
The active Balanced power-plan GUID was unchanged and no fixture remained running.
The portable verifier recomputes all raw counter arithmetic and summaries,
checks the original byte copies and validates every gate and pair. The runner
and analyzer's 21 pure checks passed. Repository validation passed formatting,
strict Clippy, the legacy-name and architecture checks, and Rust tests:
885 passed, 18 ignored, none failed. Diff checks and the required AST-only
knowledge-graph update also passed.

These are **application integration measurements**, not an isolated framework
benchmark. The older frozen Iced integration lacks Wind's subsequent watcher
registration changes. Disabled automation and fresh Home do not cover enabled
schedulers, populated dashboard caches, visible interactions or active process
restoration. Four shared-host pairs do not establish CPU equivalence, a universal
idle floor, energy savings or the benefit of the longer settling period versus
earlier cohorts with different host conditions.

For Winderust's background-first goal, this cohort supports keeping the current
Wind integration for its lower memory footprint. It provides no CPU evidence
for switching frameworks, and no basis to promise that further Wind changes
will beat Iced. The unidentified five-minute burst is the specific next profiling
target; no further optimization was implemented from this run.
