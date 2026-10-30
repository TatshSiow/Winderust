# Best-Practice Research: further hidden-idle CPU-cycle reductions

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

Research date: **2026-10-23**. Scope is Winderust resident in the background,
with no settings UI HWND, no automation work and no pending restoration. This
research does not substitute enabled-scheduler optimization for idle optimization.
No production code, dependencies, security settings or benchmark records changed.

## Direct Recommendation

The strongest remaining experiment is **first-use recovery-helper startup**:
create the helper before the first recoverable mutation, rather than at every
application startup. Retain it through shutdown once started. This could remove
an entire process and its lifecycle work from sessions that never make a
recoverable change. Its CPU benefit has not been measured.

A second candidate is to suspend **appearance-only watching** while the UI is
absent, then register and refresh appearance before reopening. This can remove
one waiting thread/window and unnecessary responses to system appearance changes.
It is a weaker CPU candidate because the watcher already blocks when idle.

The focused source audit found no additional healthy-state, application-owned
periodic timer to remove. Neither candidate establishes a universal lower idle
floor or a Wind UI-versus-Iced result.

## Evidence Used

- [Microsoft GetMessageW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getmessagew):
  a message pump can wait rather than poll, while still dispatching sent messages
  and processing system events. A blocked message pump is compatible with sparse
  process activity.
- [Microsoft WaitForSingleObject](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject):
  indefinite event waits avoid a recurring timeout. Threads owning windows still
  need an appropriate message pump.
- [Microsoft SetThreadpoolTimer](https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-setthreadpooltimer):
  an owner can disarm its timer; previously queued callbacks can remain. Timer
  coalescing permits batching for power efficiency, without establishing fewer
  cycles spent executing the callbacks.
- [Microsoft Thread Pools](https://learn.microsoft.com/en-us/windows/win32/procthread/thread-pools):
  components share the default pool, and workers execute different callbacks.
  Microsoft advises against terminating pool workers. The inference for this
  investigation is to change an identified owner's initialization or callback
  lifecycle, rather than blanket pool settings or foreign DLL work.
- [Microsoft Quality of Service](https://learn.microsoft.com/en-us/windows/win32/procthread/quality-of-service):
  QoS influences processor placement and power management. It does not establish
  cancellation of work or a guaranteed reduction in measured CPU cycles.

The [successful settled-idle comparison](ui-settled-idle-current-load-20261023.md)
supplies counters. The separate [five-minute ETW investigation](ui-idle-five-minute-etw-20261023.md)
supplies bounded stack evidence, with its failed collector and provenance
deviation preserved. They are not pooled or treated as the same experiment.

## Version / Date Context

The application uses pinned Wind UI **0.24.0**, revision
`36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2`. The loop audit used the local pinned
checkout; an online fetch of that GitHub tree was unavailable in this session.
Windows API guidance was reviewed from official Microsoft pages on the research
date. No framework upgrade or fork is proposed by this research.

## Repo-Local Context

### What already sleeps

| Inspected path | Healthy hidden-idle behavior |
| --- | --- |
| Wind UI resident loop | Releases its timer-resolution request and blocks in `GetMessageW` when no animation is active; pinned `src/platform/win32/mod.rs:1923` |
| Settings window | Its 250 ms interval belongs to the window; closing removes the window and releases presentation resources; [app.rs](../src/ui/app.rs), lines 1689 and 2719 |
| Tray health | Worker waits on a channel; success disarms its query timer. Five-second retries require an unavailable/error state; [tray_monitor.rs](../src/platform/windows/resident_shell/tray_monitor.rs), lines 76 and 194 |
| Upstream tray restoration | Retries only after taskbar recreation and failed registration, with bounded 1/2/5-second backoff |
| Settings retry | Five-second timer requires an actual failed auto-exclusion patch; [app.rs](../src/ui/app.rs), lines 1091 and 1611 |
| Second-instance restore | Indefinite event wait; [resident_shell.rs](../src/platform/windows/resident_shell.rs), line 168 |
| Diagnostic writer | Blocking receiver; its 200 ms flush timeout is an exit/panic path; [diagnostics.rs](../src/backend/diagnostics.rs), lines 67 and 112 |
| Recovery helper / transport | Blocking stdin reader and blocking request channel; [crash_recovery.rs](../src/backend/crash_recovery.rs), lines 316 and 1637 |
| Automation worker | Lazy creation, event waits, and exit when no automation/restoration work remains; [automation.rs](../src/backend/automation.rs), lines 858 and 1602 |
| Async task driver | Parks pending futures until a waker signals; [tasks.rs](../src/ui/tasks.rs), line 145 |

These findings cover the inspected application paths. They do not exclude OS,
library, security-module or externally triggered work. Failure recovery must
remain available; removing its retry timers would change behavior.

### Candidate 1: first-use recovery helper

[main.rs](../src/main.rs), line 76, eagerly creates `RecoveryClient`. Its start
method immediately initializes the helper and transport
([crash_recovery.rs](../src/backend/crash_recovery.rs), lines 487 and 545).
The centralized `record_entry`, line 825, already waits for a successful `Begin`
acknowledgment before returning a recovery intent to the mutation caller.

The previous four accepted Wind trials give a concrete component-size reference:

| Wind trial | Helper cycles | Main + helper cycles | Helper share |
| --- | ---: | ---: | ---: |
| 1 | 1,693,976 | 8,202,162 | 20.65% |
| 2 | 1,702,042 | 6,420,829 | 26.51% |
| 3 | 1,083,802 | 3,872,063 | 27.99% |
| 4 | 1,093,479 | 4,005,118 | 27.30% |
| Total | **5,573,299** | **22,500,172** | **24.77%** |

Calculation: `5,573,299 / 22,500,172 * 100 = 24.7700284%`.
Pooled helper rate was 11,610.11 cycles/s over 480.038528 helper-seconds. Each
helper had 119 intervals with no cycle increase and one positive interval.
This average describes sparse work, not continuous execution. Sources are the
[raw capture files](../benchmark-results/archive-20261028/benchmark/ui-settled-idle-current-load-results-20261023/capture/)
and [scope analysis](../benchmark-results/archive-20261028/benchmark/ui-settled-idle-current-load-results-20261023/analysis/trials.csv).

**24.77% is an observed component share, not a predicted saving.** A change can
alter main-process activity and host interference. The measurements exclude
helper startup and shutdown; their existing startup counter covers only the
main process. The later Sophos/Windows stacks do not assign the earlier cycles
to a cause.

This candidate applies only before any recoverable change has occurred. Once
started, the helper and transport must remain alive through shutdown, including
uncertain acknowledgments. It therefore offers no demonstrated reduction for
idle periods after normal managed changes have already required the helper.

Implementation constraints are substantial enough to require explicit tests:

- Preserve the owning session before the helper starts. The current `active`
  boolean makes `finish()` a no-op when false; simply skipping `start()` would
  break ownership if a later call created a runtime.
- Serialize concurrent first use and shutdown. `initialize_inner()` currently
  takes the same runtime mutex as `record_entry()`; calling it while holding
  that mutex would deadlock.
- Start the helper, publish its protected PID, and receive the journal `Begin`
  acknowledgment before a reversible OS mutation. Failure must block mutation.
- Preserve baseline capture, exact target identity and safety checks across the
  added first-use latency. Do not treat an earlier target observation as new
  authorization after startup completes.
- Preserve the current acknowledgment uncertainty, pending/committed journal
  semantics, restoration and unconfirmed-shutdown behavior.
- Keep startup recovery of stale, locally owned `Winderust Adaptive` plans
  independent of whether the new helper has started; do not delay that recovery.
- Surface initialization failures when first use occurs. Current startup error
  presentation is collected while constructing the app, at [app.rs](../src/ui/app.rs),
  line 167; a late failure must reach the appropriate command/status error path.

### Candidate 2: defer appearance-only watching

The watcher predicate follows System theme or Windows accent even without a UI
window ([requirements.rs](../src/backend/automation/requirements.rs), line 462).
With no automation it creates an appearance-only thread/window that blocks in
`GetMessageW` ([windows_events.rs](../src/backend/windows_events.rs), line 127).
Its appearance generation is consumed by the UI theme refresh at
[app.rs](../src/ui/app.rs), line 1065.

The native tray menu refreshes its own Windows appearance at every popup
([tray.rs](../src/backend/tray.rs), line 135;
[menu_theme.rs](../src/platform/windows/menu_theme.rs), line 79). Its icon is
static. This supports investigating a shorter lifetime for appearance-only
watching. Current window reopening does not perform the necessary fresh theme
read, so stopping the watcher alone would be incorrect.

A candidate must refresh theme/accent/motion before window creation and cover
changes between reading appearance and registering the watcher. Automation's
independent event registrations, including inactive-profile A/C wake requirements,
must retain their existing lifetime. Gains in quiet idle may be negligible;
fewer appearance callbacks and resident resources are more plausible than a
large continuous-cycle reduction. This is not a cause assigned to the five-minute
event cluster.

## Boundaries / Non-goals

No new CPU measurements, energy measurements or production changes were made.
The research does not recommend disabling security protection, terminating
thread-pool workers, changing private RPC globals or weakening recovery. It
does not claim that EcoQoS, thread consolidation or timer coalescing will reduce
this measured idle-cycle workload. Active scheduler cadence is outside scope.

## Handoff

Evaluate first-use helper startup first, with its recovery barrier intact.
Tests must cover no-use shutdown, concurrent first use, startup/transport failure,
protected helper identity, acknowledgment-before-mutation, uncertain replies,
restoration, and shutdown racing first use. Once started, keep the helper alive.

Before measurement, repair and verify the observer's readiness handling: the
last ETW attempt failed on its first readiness read. Any retry must be bounded
and preserve a persistent access failure. A never-used helper must be explicitly
expected absent; a missing helper must never be accepted for a mutation workload.

Use an actual first-use implementation for the candidate, rather than merely
omitting protection in a benchmark fixture. Freeze baseline/candidate source and
binary provenance, then run two AB/BA pairs under a declared host policy. One
useful design is 180 seconds settling plus 240 seconds measurement, placing the
five-minute process-age boundary inside measurement instead of at its endpoint.
Keep cleanup outside the measured window. Compare complete main-plus-helper
scope, retain all bursts, and verify native process generations and cleanup.
Only expand the experiment if its first matched results justify doing so.

Measure appearance-watcher deferral separately. Test fresh theme/accent/motion
on reopening and native tray popup appearance before claiming parity.
