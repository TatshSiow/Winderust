# Phase 10: Architecture Freeze

Status: Complete (2026-08-12)

## Final modular-monolith flow

```text
Windows events / GPUI intent / persisted settings
                    |
                    v
             WinderustApp composition
          /          |             \
 SettingsEditor   UI read models   typed commands
       |                              |
       v                              v
 portable storage                RuntimeHandle
                                      |
                        wake state + RefreshScheduler
                                      |
                                      v
                                  RuntimeCore
                     /                |                 \
            CycleObservations   feature policy     result replies
                                      |
                                      v
                         typed mechanism controllers
                           /                      \
              platform/windows adapters      RecoveryClient
                                                     |
                                                     v
                                      independent watchdog replay
```

The UI owns GPUI composition, interaction, dialogs, timers, process-list presentation, dashboard
history, update state, and navigation. `SettingsEditor` owns the only settings draft/revision/save
boundary. `RuntimeHandle` owns worker and event-source lifecycle; `RuntimeCore` is the single
automation composition root. A requested observation is collected at most once in one worker pass.

Feature managers own policy: rule matching, target selection, timers, hysteresis, cooldowns,
preservation choices, failure suppression, status, and Action Log translation. Typed controllers
own live claims, exact identities, first baselines, precedence, transaction/verification,
compensation, clean restoration, and recovery intents. Narrow Windows adapters own raw managed
mechanism structures, mutation handles, status conversion, unsafe calls, and native string/buffer
mechanics. Feature-specific query-only sampling remains read-side policy input and cannot authorize
a mutation; the controller always reopens and revalidates the selected exact target.

## Ownership classes

| Class | Route | Recovery contract |
| --- | --- | --- |
| Temporary external process/thread state | Feature policy -> RuntimeCore controller -> Windows adapter | Begin/apply/verify/Commit plus independent helper replay and reverse clean release |
| Automatic active power plan / temporary Adaptive plan | Decision policy -> `PowerPlanController` -> power-plan adapter | GUID recovery journal, verification, compensation, external-state rebase, managed-plan cleanup |
| Process-lifetime state | Timer Resolution or Winderust self-power controller -> adapter | Explicit clean shutdown; Windows process teardown is the forced-exit boundary |
| Persistent explicit configuration | `SettingsEditor` or typed application service -> adapter | No automatic claim or helper journal; typed staged result/readback |
| Irreversible command | UI -> bounded RuntimeHandle command -> typed controller -> adapter | Exact safety preflight and typed result; no fictional baseline or restoration |

CPU allocation has one coordinator with CPU Sets (Soft) > Processor Affinity (Hard) > Core Limiter
> Adaptive Engine / Workload Engine precedence. Process Priority and process Power Throttling share
one compound controller. App Suspension uses one normal named-Job controller and an independently
retained helper handle. All other temporary process properties have one controller per mechanism.

## Deleted migration shapes

- No `BackgroundAutomation` or `HiddenAutomationRunner` source type remains; the final names are
  `RuntimeHandle` and `RuntimeCore`.
- Process List owns no reversible-property setter or restoration closure.
- Converted feature modules own no raw mutation API, baseline record, recovery intent, or restoring
  `Drop` path.
- `WinderustApp` keeps one `Arc<RuntimeFeatureStatus>` segment rather than fifteen mirrored feature
  snapshots, plus one plain model each for Process Catalog, Process List, Dashboard, Update, and
  Shell state.
- The self-power DTO has one concrete type rather than a compatibility alias.
- No parallel runtime, scheduler, global process snapshot, settings writer, or recovery helper was
  introduced.

## Structural gates

`scripts/check_architecture_ownership.ps1` now freezes:

- exactly one `RuntimeHandle` and one `RuntimeCore` definition and zero legacy owner names;
- one live Windows adapter call per managed mutation mechanism, with only explicit recovery mirrors;
- no raw managed mutation in UI, feature policy, or typed transaction controllers;
- no legacy Process List restoration closures;
- no recovery protocol state in feature or UI layers;
- one SettingsEditor composition boundary, one segmented runtime feature-status model, and one of
  each plain UI read model;
- persistent and irreversible operations outside temporary baseline/recovery ownership.

## Automated evidence

- `cargo fmt -- --check`: passed.
- Strict all-target Clippy passed for both the normal and `architecture-diagnostics` feature
  graphs, including `-D unsafe-op-in-unsafe-fn`.
- The normal and `architecture-diagnostics` locked suites each passed 625 tests, with 12 explicit
  Windows integration tests ignored by default.
- `scripts/check_architecture_ownership.ps1`: passed.
- `scripts/build_release.cmd -TargetDir target-next`: passed; optimized binary produced.
- Legacy product/settings naming scan: passed with no matches.
- `git diff --check` and `git diff --cached --check`: passed.
- `graphify update .`: passed; 5,939 nodes and 17,825 edges rebuilt.

## Performance comparison

The standard isolated release benchmark is
[`architecture-phase-10-20260812.json`](../../benchmark/results/architecture-phase-10-20260812.json).
It used the same Intel Core 5 210H machine, 30-second cases, 500 ms sampling, and five action trials
as the Phase 0 baseline.

| Metric | Phase 0 | Phase 10 | Result |
| --- | ---: | ---: | --- |
| Idle CPU median | 0.0000% | 0.0000% | Pass; no increase |
| Idle working-set median | 84.1094 MiB | 79.5430 MiB | Pass; 4.5664 MiB / 5.43% lower |
| Idle thread-count median | 38 | 38 | Pass; no unexplained thread |
| Idle worker wake frequency | 0 Hz | 0 Hz | Pass; worker remains dormant |
| Active worker wake frequency | 0.8539 Hz | 0.7787 Hz | 8.81% lower |
| Process appearance-to-action P95 | 115.5333 ms | 70.1411 ms | Pass; 45.3922 ms faster |
| Clean Process Priority release | Restored | Restored | Pass; six expected changes, zero failures |

The startup-inclusive 30-second idle CPU mean was 0.2041% versus 0.1041% at Phase 0, a 0.1000
percentage-point increase; the contracted median remained zero and the dormant worker recorded no
wakes. The user confirmed the optimized binary's interactive navigation, resize/drag behavior, and
requested idle check on 2026-08-12. No unintended UI behavior was observed, and no UI behavior was
redesigned in Phases 9–10.
