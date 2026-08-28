# Architecture

Winderust is a mechanism-centered modular monolith: one GPUI application process, one optional
automation worker, and one typed owner for each Windows mechanism. UI and feature code express
intent; only controllers and their narrow Windows adapters own live mutations.

```text
Windows events / UI intent / persisted settings
                    |
                    v
              WinderustApp
          /         |          \
 SettingsEditor  read models  typed commands
                                |
                                v
                          RuntimeHandle
                                |
                     wake state + scheduler
                                |
                                v
                           RuntimeCore
                  /             |             \
         observations      feature policy    replies
                                |
                                v
                    typed mechanism controllers
                       /                    \
              Windows adapters          RecoveryClient
                                              |
                                              v
                                   independent watchdog
```

## Responsibilities

- `WinderustApp` owns GPUI composition, dialogs, navigation, process-list presentation, and local
  read models.
- `SettingsEditor` is the only settings draft, revision, persistence, import, and export boundary.
- `RuntimeHandle` owns worker lifecycle, event sources, typed commands, and published status.
- `RuntimeCore` owns reconciliation order, shared observations, controller composition, and
  shutdown.
- Feature managers own rules, target selection, timers, hysteresis, suppression, status, and
  Action Log policy.
- Typed controllers own exact identities, claims, baselines, precedence, verification,
  compensation, and restoration.
- `src/platform/windows/` owns raw Win32, NT, and WDK calls, handles, buffers, and native state
  conversion.
- `RecoveryClient` and the watchdog independently replay externally persistent temporary state
  after an abnormal exit.

Process enumeration and presentation remain a read-side query. A query result can suggest a
target, but every mutation reopens and revalidates the exact process or thread identity.

## State classes

- Temporary process and thread state belongs to a typed controller with baseline capture, a
  watchdog journal, verification, and reverse clean release.
- Automatic power-plan state belongs to `PowerPlanController`, including the first baseline,
  verified switching, managed-plan cleanup, and watchdog recovery.
- Process-lifetime state belongs to Timer Resolution or the Winderust self-power controller. It
  has explicit clean release, with process teardown as the forced-exit boundary.
- Persistent configuration belongs to `SettingsEditor` or a typed application service. It uses
  explicit user intent and readback without temporary claims or recovery entries.
- Irreversible commands use typed command controllers with complete safety preflight and
  per-target results, without a fictional baseline.

## Safety invariants

- A reversible mutation requires a captured pre-Winderust value bound to the validated identity.
- Process identity uses PID, creation time, and absolute executable path; thread identity also
  includes thread ID and creation time.
- Cross-session settings govern new acquisition, never release of state Winderust already owns.
- Critical, protected, inaccessible, or unverifiable targets fail closed.
- Temporary externally persistent writes follow Begin, apply, verify, then Commit. Clean release
  runs in reverse application order; the watchdog independently replays committed state after a
  crash or forced termination.
- External changes break Winderust ownership instead of being overwritten during restoration.
- Process List reversible actions use the same RuntimeCore controllers as automatic policy and
  may be superseded by a later automatic reconciliation.
- CPU allocation has one coordinator with this precedence: CPU Sets (Soft), Processor Affinity
  (Hard), then Adaptive Engine / CPU Scheduler.
- App Suspension and CPU Limiter share one Job Object suspension controller. Their independent
  claims combine into one effective frozen state, so releasing either feature cannot thaw the
  other feature's claim.
- CPU Limiter keeps that Job Object path primary. Only an incompatible existing-job result selects
  its private exact-thread fallback; App Suspension remains Job-only. The fallback owns one
  suspend-count increment per exact thread and uses the same limiter worker and schedule.
- While a valid CPU Limiter rule is active, target refresh and process appearance discovery remain
  at one second even when Winderust is hidden or Adaptive Engine saver cadence is active.
- Process Priority and Power Throttling share one compound controller so Efficiency Mode cannot
  leave a half-applied state.

## Structural enforcement

`scripts/check_architecture_ownership.ps1` is the executable ownership boundary. It rejects raw
managed mutations outside approved Windows adapters and recovery mirrors, feature- or UI-owned
restoration state, duplicate runtime owners, and persistent or irreversible operations routed
through temporary recovery.

Architecture changes must keep the ownership scan, formatting, strict Clippy, locked tests,
legacy naming scan, and Graphify index green. Migration history and completed phase evidence
remain available in Git history rather than in the active documentation set.
