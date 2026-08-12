# Phase 5.5 typed Process Priority and Efficiency Mode control

- Status: **complete**
- Date: 2026-08-11
- Refactor contract: [architecture-refactor-plan.md](../architecture-refactor-plan.md)
- Previous mechanism: [phase-5-memory-priority.md](phase-5-memory-priority.md)

This slice converts the coupled Process Priority and process Power Throttling boundary as one
unit. Static Process Priority, Background Efficiency, Workload Engine pressure policy, Workload
foreground boost, and Process List one-shot actions now share one
`PriorityEfficiencyController` in `RuntimeCore`. Feature managers own policy, suppression, status,
and Action Log attribution only. The controller owns exact process identity, owner claims,
arbitration, raw baselines, expected values, compound transitions, and clean release.
`src/platform/windows/priority_efficiency.rs` owns the narrow priority-class and Power Throttling
Win32 adapter. The crash helper remains the independent recovery mirror.

## Ownership and arbitration

```text
Background Efficiency ---------------- owner: BackgroundEfficiency --\
Workload foreground boost ------------- owner: WorkloadForegroundBoost +--> priority precedence
Adaptive/Workload pressure ------------ owner: AdaptiveEngine --------+
static Process Priority --------------- owner: ProcessPriority -------/

Background Efficiency ---------------- owner: BackgroundEfficiency --\
Adaptive/Workload Efficiency ---------- owner: AdaptiveEngine --------+--> power precedence

Process List -> bounded result-bearing command -----------------------+--> RuntimeCore worker
                                                                         |
                                                                         v
                                                     PriorityEfficiencyController
                                                  PID + creation time + exact path
                                                                         |
                                      Begin -> apply -> verify -> Commit |
                                                                         v
                    platform/windows/priority_efficiency.rs raw query/set adapter
                                                                         |
                                                                         v
                                                         crash-recovery mirror
```

Process Priority precedence is Background Efficiency > Workload foreground boost > Adaptive
Engine > static Process Priority. Power Throttling precedence is Background Efficiency > Adaptive
Engine. A higher owner shadows but does not delete a lower claim. Removing the higher owner reveals
the lower claim without bouncing through the pre-Winderust baseline. The first successful
Winderust mutation keeps one property-specific baseline across all owner changes.

Background Efficiency owns a compound Efficiency Mode claim: Idle process priority and the
`PROCESS_POWER_THROTTLING_EXECUTION_SPEED` EcoQoS bit must transition together. If either half
fails, the controller compensates the completed half and retains no hidden claim. Process Priority
and Power Throttling otherwise have independent ledgers, so Workload Engine can still apply a
priority claim when Windows cannot expose a reversible Power Throttling state.

## Policy behavior

Static Process Priority retains Focus App > Visible Window > Background tiering, exact-path rule
semantics, exclusions, cross-session policy, preservation settings, normalized-path failure
suppression, auto-exclusion publication, status, and Action Log summaries.

Background Efficiency retains foreground and visible-window protection, aggressiveness-specific
built-in exclusions, custom rules, audio-aware timer-resolution behavior, failure suppression,
status, and Action Log attribution. Workload Engine retains background/visible target selection,
pressure gating, foreground stability and launch boost, High/Realtime preservation, and its own
Action Log attribution.

An unavailable Workload Engine Power Throttling control is remembered by exact process instance.
That suppresses repeated retries and log spam while leaving the independent priority claim active.
The entry disappears when the target or process instance disappears, so a reused PID is evaluated
as new work.

## Identity, transition, and recovery safety

The lightweight pass-local process snapshot now captures creation time with the query handle it
already opens. Executable paths remain lazy. This supplies every converted policy with an exact
PID + creation time + executable path identity without an additional full path/account enrichment
pass. The runtime worker reopens and revalidates that identity immediately before every read or
write. PID 0, Winderust itself, changed or relative paths, changed process names, critical or
unverifiable processes, protected processes, disallowed cross-session targets, and inaccessible
handles fail closed.

Each property transition records a crash-recovery Begin intent, writes through the sole production
adapter, reads the exact value back, and commits only after verification. Apply and verification
failures compensate to the prior value. A compound transition compensates both properties in
reverse order. If a release reaches and verifies the baseline before Commit fails, the controller
keeps the verified baseline and retries only journal relinquishment.

Clean release writes a baseline only while the current value still equals Winderust's expected
value. An external change breaks ownership and is left untouched after the exact property journal
entry is relinquished. Process exit and PID reuse also relinquish stale entries without targeting a
replacement. Shutdown clears all automatic claims and restores both property ledgers in reverse
successful-application order, including Process List-only state.

Every `GetProcessInformation(ProcessPowerThrottling)` caller now initializes
`PROCESS_POWER_THROTTLING_STATE.Version` to
`PROCESS_POWER_THROTTLING_CURRENT_VERSION`. The production integration test showed that passing a
zero-initialized query structure makes supported Windows reject the read. The external-process
adapter, crash recovery mirror, Winderust self-power adapter, and Process List status query all
follow this contract.

## Process List and worker lifetime

Process Priority and Efficiency Mode join the shared bounded, result-bearing process-control FIFO.
Automatic policy executes before manual commands in a pass, so a click takes effect immediately
and only a later applicable automatic reconciliation may supersede it. Group actions attempt every
captured process and preserve the existing partial-failure summary.

GPUI performs no process mutation while updating an entity. It queues a typed request, waits on the
background executor, and publishes completion on the foreground executor. The old Process List
priority/Efficiency Mode setter closures and restoration stack are gone.

Manual commands can start the runtime worker while automation is otherwise dormant. Worker exit is
generation-gated so a request cannot be stranded between the final idle snapshot and thread exit.
An unwind guard marks an unexpectedly exiting worker unavailable and returns a typed terminal
result to queued waiters, preventing a UI wait from hanging after a worker panic.

## Validation evidence

Deterministic controller tests cover owner precedence and reveal, Process List supersession,
independent priority when Power Throttling is unavailable, compound apply and Commit failures,
external-state breaks, PID reuse, session metadata drift, High/Realtime preservation, release
Commit ambiguity, and reverse shutdown. Runtime tests cover both manual command replies,
cross-property FIFO order, managed-state lifetime, worker handoff, panic-safe waiter completion,
and shutdown order.

The ownership script permits `SetPriorityClass` only in the external-process Windows adapter,
Winderust self-power Windows adapter, and crash recovery, and permits process Power Throttling
`SetProcessInformation` only in those corresponding adapter/recovery boundaries. It asserts
exactly one production call per normal adapter, rejects raw Win32 APIs from both lifecycle
controllers, and proves Process Priority, Background Efficiency, Workload Engine, and Process List
retain no competing writer or restoration owner.

The default and `architecture-diagnostics` suites each pass **552 tests**, with ten explicit
Windows integration tests ignored by the ordinary matrix. The Power Throttling crash-recovery test
runs against a disposable process in both matrices. The ignored production-controller integration
test was run explicitly and verified compound apply, exact readback, and clean restoration. Strict
Clippy passes in both configurations.

## Same-host release comparison

The Phase 5.5 release benchmark uses the same Intel Core 5 210H host, isolated portable settings,
30-second footprint windows, five Process Priority action trials, and
`architecture-diagnostics` instrumentation as Phase 5.4.

| Metric | Phase 5.4 | Phase 5.5 |
| --- | ---: | ---: |
| Idle CPU median / mean / P95 (% total capacity) | 0 / 0.0787 / 0.2521 | 0.2478 / 0.1799 / 0.5079 |
| Idle working set / private memory median | 84.3594 / 127.4336 MiB | 79.4453 / 123.1680 MiB |
| Idle threads / handles median | 38 / 749 | 38 / 735 |
| Active CPU median / mean / P95 (% total capacity) | 0.2497 / 0.2508 / 0.7523 | 0 / 0.1762 / 0.5101 |
| Active working set / private memory median | 82.1133 / 123.1289 MiB | 83.0273 / 126.1406 MiB |
| Active threads / handles median | 40 / 750 | 39 / 732 |
| Reconciliation passes / wake frequency | 34 / 0.8298 Hz | 31 / 0.7573 Hz |
| Process / path-enrichment / foreground / visible scans | 33 / 0 / 18 / 17 | 30 / 0 / 18 / 17 |
| Process-appearance-to-action median / P95 | 65.0675 / 68.5434 ms | 68.3335 / 81.2760 ms |
| Applied actions / failures | 6 / 0 | 6 / 0 |
| Clean priority restoration | Pass | Pass |

Idle remained architecturally dormant with zero worker reconciliations, wakes, or inventory scans;
the nonzero UI CPU samples are therefore not automation work. Median working set fell by 4.9141
MiB, private memory fell by 4.2656 MiB, and handle count fell by 14. In the active case, mean and P95
CPU decreased, one fewer thread and 18 fewer handles were observed, and the same six changes
completed with no failures and clean restoration. Working set and private memory were higher by
0.9140 MiB and 3.0117 MiB respectively. Median action latency increased by 3.2660 ms; P95 contains
one 81.2760 ms trial. Reconciliation and scan counts did not increase, and the required identity
capture added no full path-enrichment pass, so these small same-host differences are treated as
run-to-run variance rather than a new polling or allocation cost.

Artifacts:

- `benchmark/results/architecture-phase-5-memory-priority-20260811.json`
- `benchmark/results/architecture-phase-5-priority-efficiency-20260811.json`

## Exit gate

| Process Priority / Efficiency gate | State | Evidence |
| --- | --- | --- |
| Every automatic and Process List producer shares one controller | Pass | Owner-tagged policy routes and typed runtime commands |
| Priority and Power Throttling precedence is deterministic | Pass | Explicit precedence tables and owner-reveal tests |
| Compound Efficiency Mode cannot leave a half-applied owner | Pass | Apply/Commit failure compensation tests |
| Exact process identity rejects PID reuse | Pass | Creation-bound snapshot, controller validation, and reuse test |
| External state is never overwritten during release | Pass | Expected-state guards and property journal relinquishment |
| Unavailable Power Throttling does not block or spam priority work | Pass | Independent-ledger test and exact-instance unavailable set |
| No duplicate live writer or restoration authority remains | Pass | Ownership script and source-zero assertions |
| GPUI and command waiters cannot be blocked by process mutation or worker exit | Pass | Background command route and panic/exit queue tests |
| Full, diagnostics, live, and benchmark gates | Pass | 552 tests in each matrix, explicit live controller test, and valid release artifact |

Rollback must revert this complete coupled slice: controller, every automatic producer, both
Process List command variants, crash-recovery forget operations, lightweight creation-time capture,
worker-lifetime handling, and ownership assertions. Restoring only one legacy producer would
recreate competing baselines and duplicate restoration authority.
