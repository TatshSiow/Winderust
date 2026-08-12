# Phase 3 power and event-source ownership

- Status: **complete; Phase 4 may begin**
- Date: 2026-08-11
- Refactor contract: [architecture-refactor-plan.md](../architecture-refactor-plan.md)
- Phase 2 evidence: [phase-2-scheduler-observations.md](phase-2-scheduler-observations.md)

Phase 3 gives automatic power-plan mutation, Winderust self-power state, and automation event-source
lifecycle one runtime owner. It preserves the existing power-plan decision precedence, feature
settings, Win32 thread requirements, polling fallbacks, process safety barriers, and user-visible UI.

## Runtime flow and ownership

```text
Win32 input/window/power/session events
        |
        v
RuntimeHandle event sources -> dirty flags / generation -> RefreshScheduler
                                                        |
                                                        v
                                               RuntimeCore::run_check
                                                        |
                                                        v
                                           rules::decision_engine::decide
                                                        |
                                                        v
                                              PowerPlanController
                                                        |
                                   Begin / apply / verify / Commit
                                                        |
                                                        v
                                    RecoveryClient -> powercfg Win32 adapter

tray visibility + Adaptive settings -> SelfPowerController

RuntimeCore/controller status -> segmented RuntimeStatusSnapshot -> WinderustApp presentation
```

`RuntimeCore` is now the only automatic power-policy execution route. Visible and hidden operation
share the same activity detector, controller detector, CPU-load scheduler, decision state, retry
state, and `PowerPlanController`; visibility changes cadence only. The previous UI decision and
application route was removed, as was the hidden By Running App skip that could publish a decision
without applying it. UI power-plan access is read-only enumeration and presentation.

`RuntimeHandle` owns `InputHook`, `WindowsEventWatcher`, `TrayVisibilityWatcher`, and
`SelfPowerController`. The low-level input hook and WinEvent watcher retain separate dedicated
message-loop threads because their Win32 registrations and cleanup are thread-affine. Their
callbacks only coalesce typed wake state. Tray HWND subclassing remains at the Windows/GPUI
boundary, while every hide/show/quit transition emits a visibility notification and invokes
callbacks outside the tray lock.

Startup now restores only narrowly identified stale `Winderust Adaptive` plans before
`RuntimeHandle` starts. This prevents startup cleanup from racing a newly created managed plan.
Shutdown stops the input, window-event, and tray-visibility sources before the worker releases its
automatic power-plan state and joins. It then restores Winderust self-power. Every release is
attempted and errors are aggregated.

## Power-plan controller contract

`PowerPlanController` owns the actual/current GUID, the expected Winderust-applied GUID, the clean
ordinary baseline, retry suppression, and the temporary Adaptive plan lifecycle. Ordinary
automation and the Adaptive managed plan are mutually exclusive typed owners.

Every switch follows the existing crash barrier:

```text
read/capture baseline -> Recovery Begin -> apply -> verify -> Recovery Commit
                                      \-> failure: compensate -> Cancel/retain recovery intent
```

If application, verification, or journal commit fails, the controller attempts compensation. A
recovery intent remains available when compensation cannot prove restoration. Repeated identical
failures are suppressed for a bounded interval, while a changed request can retry immediately.

Power-plan policy and managed `Winderust Adaptive` recognition remain in `src/power/powercfg.rs`.
The GUID parser, native enumeration/read/write calls, effective-power-mode callback registration,
and all unsafe code live in `src/platform/windows/power_plan.rs`. Runtime switching, crash replay,
startup cleanup, and explicit persistent tuning share that raw adapter without sharing their
distinct lifecycle or recovery semantics.

An external power-plan change breaks Winderust's expected chain and rebases the clean ordinary
baseline instead of overwriting the user's choice. Adaptive setup reads the actual active plan
immediately before taking ownership. Adaptive release also refreshes actual state before deciding
whether to restore, and cleanup deletes only a temporary plan with the exact current Winderust name
and description contract.

## Winderust self-power contract

`SelfPowerController` captures Winderust's process priority and power-throttling state before its
first mutation. Capture is strict: if the reversible baseline cannot be read, no write occurs.
Hidden-to-tray and Adaptive requests are composed into one desired state, so either request can
enable EcoQoS without one owner accidentally undoing the other. Hidden mode requests Idle priority;
Adaptive mode requests timer-resolution throttling behavior. Disable and shutdown restore the exact
captured baseline.

The controller owns only composition, lifecycle, verification, compensation, and retry state.
`src/platform/windows/self_power.rs` owns the current-process pseudo-handle plus the raw priority and
Power Throttling queries/writes; it shares only the typed Power Throttling value shape with the
external-process adapter. No policy or recovery ownership crosses into that Windows boundary.

The composed transition is applied once, verified, and compensated on a partial write or
verification failure. Identical failures use a five-second retry suppression window; changed
requests bypass it. This state is process-lifetime state and does not use the external recovery
journal because terminating Winderust also terminates the controlled process.

## Characterization and failure matrix

Deterministic tests cover:

- visible/hidden routing through the same decision owner, including By Running App;
- ordinary baseline capture, repeated switches, external changes, retry suppression, and shutdown;
- Adaptive create/apply/update/release/delete sequencing and exact cleanup recognition;
- recovery Begin, apply, verify, and Commit failures with compensation;
- strict self-power baseline capture, hidden/Adaptive composition, partial failure, verification
  failure, retry behavior, and idempotent shutdown;
- input, window-event, tray-visibility, settings replacement, status publication, and source-stop
  ordering.

The default and `architecture-diagnostics` suites each pass **457 tests**, with the two live Windows
recovery tests ignored by the ordinary matrix. Both ignored tests were also run explicitly on
2026-08-11: disposable power-plan recovery restored the original plan and deleted the temporary
plan, and disposable named-job recovery thawed its suspended process.

## Same-session release A/B

The Phase 3 release benchmark is compared with a control built from the staged Phase 2 checkpoint
in the same session and target directory. Both runs used the same host, isolated portable settings,
30-second footprint windows, five process-priority action trials, and the
`architecture-diagnostics` feature.

| Metric | Phase 2 control | Phase 3 |
| --- | ---: | ---: |
| Idle CPU mean / P95 (% total capacity) | 0.1252 / 0.4953 | 0.0918 / 0.2530 |
| Idle working set / private memory median | 81.7969 / 125.1953 MiB | 82.4180 / 125.4688 MiB |
| Idle threads / handles median | 39 / 751 | 38 / 751 |
| Idle worker reconciliation passes | 0 | 0 |
| Active CPU mean / P95 (% total capacity) | 0.1085 / 0.2546 | 0.1668 / 0.5040 |
| Active working set / private memory median | 85.2031 / 127.0664 MiB | 84.1250 / 126.1055 MiB |
| Active threads / handles median | 40 / 748 | 40 / 750 |
| Reconciliation passes / wake frequency | 27 / 0.6557 Hz | 42 / 1.0285 Hz |
| Process / foreground / visible-window scans | 26 / 18 / 17 | 41 / 21 / 20 |
| Window-created events accepted | 13 | 25 |
| Process-appearance-to-action median / P95 | 65.3524 / 69.3386 ms | 69.6520 / 86.4457 ms |
| Applied actions / failures | 6 / 0 | 6 / 0 |
| Clean priority restoration | Pass | Pass |

Idle remained dormant and its CPU, thread, and handle footprint did not regress. Active Phase 3
accepted 25 window-created events versus 13 in the control, so its additional passes and scans are
not a controlled scheduling regression claim. The latency delta was +4.2996 ms median and
+17.1071 ms P95, within the refactor's 50 ms P95 budget. Both runs applied all six actions with no
failure and restored the retained target cleanly.

Artifacts:

- `benchmark/results/architecture-phase-3-control-20260811.json`
- `benchmark/results/architecture-phase-3-20260811.json`

## Exit-gate evidence

| Phase 3 gate | State | Evidence |
| --- | --- | --- |
| Visible and hidden inputs use one power decision path | Pass | One `decide` call in `RuntimeCore`; source ownership gate and parity characterization |
| Visibility does not transfer controller or policy state | Pass | One persistent runtime/controller instance; tray notifications only wake/synchronize cadence |
| Ordinary and Adaptive crash/release matrix passes | Pass | Deterministic failure matrix plus both explicit live recovery tests |
| Cleanup recognizes only current Winderust Adaptive plans | Pass | Exact name/description contract retained and tested |
| No automatic power-plan mutation remains in `src/ui/` | Pass | Architecture ownership script and source scan |
| Footprint and action latency remain within budget | Pass | Same-session release A/B above |

Phase 3 is independently revertible as one power/event slice: restore the characterized visible UI
adapter and hidden runner ownership together, remove both controllers and runtime event-source
ownership, and retain the Phase 0-2 settings, scheduler, observation, and recovery foundations. A
rollback must never activate the legacy and controller mutation paths at the same time.
