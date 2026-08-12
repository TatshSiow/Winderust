# Phase 2 scheduler and per-pass observations

- Status: **complete; Phase 3 may begin**
- Date: 2026-08-10
- Refactor contract: [architecture-refactor-plan.md](../architecture-refactor-plan.md)
- Phase 1 evidence: [phase-1-application-runtime-foundation.md](phase-1-application-runtime-foundation.md)

Phase 2 extracts the outer automation deadlines into one typed scheduler and shares lazy Windows
observations within one reconciliation pass. It does not move a Windows mutation, baseline,
recovery entry, feature policy, retry, hysteresis, or stateful sampler to a new owner.

## Refresh scheduler

`RefreshScheduler` owns the 19 outer-worker deadlines and the Workload Engine fast-refresh window.
`RefreshDomain` names each independently due lane, while `SchedulerEvent` owns the exact deadline
fan-out for settings, foreground, window, power, session, input, app-switch, process-appearance,
manual App Suspension, Memory Trim, and controller-activity events.

Feature predicates and cadence policy remain in `automation/requirements.rs`. CPU usage, Adaptive
I/O, active-plan sampling, retry suppression, and feature-local cooldowns remain with their
stateful owners. The scheduler only answers whether a lane is due, moves its deadline, and selects
the minimum required wait. Default settings still leave the worker dormant, visible mode still
does not take hidden power-plan ownership, and hidden power checks retain their event-or-delay
behavior.

Deterministic scheduler tests assert every event mapping, manual request isolation, fast-window
expiry, deadline behavior, and dormancy. Existing requirement and characterization tests retain
coverage for visible/hidden cadence, retry behavior, suspended-process release, execution order,
and reverse shutdown order.

## Cycle observations

`CycleObservations` is created once at the start of each worker pass and discarded at its end. Each
domain starts as `NotRequested`, becomes `Available(value)` after one successful collection, or
`Unavailable(error)` after one failed collection. Successes and failures are both cached for that
pass so one failing collector cannot produce contradictory feature decisions or a retry storm.

The first converted domains are:

- the process catalog;
- optional exact-path and process-identity enrichment of that same catalog;
- focused process ID and focused process details;
- visible-window process IDs;
- top-level-window process IDs used by App Suspension release.

Process catalogs and window-ID sets use shared `Arc` values. Exact-path consumers enrich the same
catalog rather than taking a second snapshot, and `ProtectedProcesses` shares the visible-window
set instead of copying it for every feature. The cache selects policy candidates only. Every
mutation still reopens its target and revalidates identity, access, critical/protected status,
session policy, and expected state through the existing feature path.

Background Efficiency, App Suspension, CPU Sets (Soft), Processor Affinity (Hard), Core Limiter,
By Running App, Workload Engine, every Priority Control manager, Memory Trim, process-appearance
detection, and hidden power decisions now consume the pass-local observations. Stateful CPU, I/O,
audio, memory, and topology samplers were deliberately not folded into this cache.

The Process List remains independent in `ui/app/process_refresh.rs`. It keeps its asynchronous
query cadence, icon loading, grouping, sorting, and resource history, and continues to call
`list_processes_with_paths()` directly.

## Behavior and safety preservation

- Feature execution and reverse clean-restoration order are unchanged.
- A disabled feature returns before requesting a process or window observation, including during
  shutdown restoration.
- Unavailable process and visible-window observations retain each feature's existing fail-closed
  path and release already-managed state where it did before.
- Process appearance still ignores its initial inventory and invalidates the same 13 consumers
  when a new PID appears.
- Session changes still combine foreground, window-created, active-plan, and immediate power-plan
  invalidation behavior.
- No raw Win32/NT/D3DKMT setter, recovery protocol, settings schema, or Process List mutation route
  changed, so no reference-library boundary update was required.

`scripts/check_architecture_ownership.ps1` now blocks raw process/window collectors in converted
runtime feature consumers, local `next_*` deadlines in the outer loop, more than one worker-loop
`CycleObservations` construction, and accidental removal of the independent Process List query.

## Same-session release A/B

Artifacts:

- Control: [architecture-phase-1-control-20260810.json](../../benchmark/results/architecture-phase-1-control-20260810.json)
- Phase 2: [architecture-phase-2-20260810.json](../../benchmark/results/architecture-phase-2-20260810.json)
- Historical Phase 0 reference: [architecture-baseline-20260810.json](../../benchmark/results/architecture-baseline-20260810.json)

The control executable was built from the staged pre-Phase-2 Git index. Both control and Phase 2
used the same corrected runner, Intel Core 5 210H host, Windows 10.0.26200 session, release profile,
30-second footprint windows, five action trials, and isolated portable settings. The runner now
normalizes its own priority before creating children and verifies each owned hidden `PING.EXE`
target is `Normal` before timing. This removes dependence on an invoking shell that may itself be
in Efficiency Mode or `Idle` priority.

| Metric | Pre-Phase-2 control | Phase 2 |
| --- | ---: | ---: |
| Idle CPU mean / median / P95 (% total capacity) | 0.1406 / 0.0000 / 0.2526 | 0.1370 / 0.0000 / 0.4776 |
| Idle working set median | 84.2422 MiB | 82.3086 MiB |
| Idle private memory median | 128.0117 MiB | 125.7344 MiB |
| Idle threads / handles median | 39 / 735 | 38 / 732 |
| Idle worker wake frequency | 0.0000 Hz | 0.0000 Hz |
| Active CPU mean / P95 (% total capacity) | 0.2109 / 0.7526 | 0.2123 / 0.7383 |
| Active working set / private memory median | 84.1055 / 126.5586 MiB | 83.3945 / 126.5156 MiB |
| Active threads / handles median | 40 / 730 | 40 / 731 |
| Process snapshot scans | 40 | 33 |
| Foreground-process queries | 41 | 18 |
| Visible-window scans | 17 | 17 |
| Process-appearance-to-action median / P95 | 67.3579 / 68.7749 ms | 66.4260 / 74.4831 ms |
| Applied action count / failures | 6 / 0 | 6 / 0 |
| Clean priority restoration | Pass | Pass |

The active case performed the same 17 Process Priority cycles and accepted the same 12
window-created events. Process scans fell 17.5%, foreground queries fell 56.1%, active CPU mean
changed by 0.0014 percentage points, and action P95 increased by 5.7082 ms, within the 50 ms budget.
Idle median CPU and idle wake frequency did not increase; memory and permanent thread count also
remained within budget.

The Phase 2 sample did record 27 timeout wakes versus 21 in the control, producing 41 versus 34
worker passes. These were timeout-only reconciliation passes: feature cycles did not increase and
active CPU remained effectively flat. This is retained as a watch metric for later worker
centralization rather than presented as a measured efficiency win.

## Exit-gate evidence

| Phase 2 gate | State | Evidence |
| --- | --- | --- |
| Typed event/deadline semantics preserve current behavior | Pass | Ten deterministic scheduler tests plus existing cadence, retry, order, and dormancy characterization |
| Each requested process/window domain is collected at most once per pass | Pass | Observation source-call tests cache both success and failure; live active case recorded 33 process scans, 18 foreground queries, and 17 visible scans across 41 passes |
| Target sets and unavailable behavior remain fail-closed | Pass | Full feature test suite, unavailable-observation test, unchanged mutation revalidation, and six successful isolated actions with zero failures |
| Process List remains independent | Pass | Ownership source gate and direct `ProcessListQuery` collector path |
| Idle wake rate is no higher than baseline +5% | Pass | Same-session control and Phase 2 both recorded 0.0000 Hz and zero reconciliation passes |
| Latency and footprint remain within budget | Pass | Same-session release A/B; P95 delta +5.7082 ms, lower memory medians, unchanged active permanent thread count |
| Clean restoration still succeeds | Pass | Retained target returned to `Normal`; normal and diagnostics suites preserve reverse-order tests |

## Validation notes

Normal and `architecture-diagnostics` strict Clippy and test matrices pass with 443 tests passed and
two environment-sensitive recovery tests ignored. The ownership script and naming scan pass. The
live Job Object and power-plan recovery limitations remain those documented by Phase 1; this phase
did not alter either Windows boundary.

Phase 2 is independently revertible: restore the local deadline variables and direct collector
calls from the pre-Phase-2 index, remove `src/runtime/scheduler.rs` and
`src/runtime/observations.rs`, and retain every Phase 0/1 safety and lifecycle checkpoint. No
Windows-property ownership route changed, so Phase 3 can centralize power and event-source
ownership without coupling that work to observation extraction.
