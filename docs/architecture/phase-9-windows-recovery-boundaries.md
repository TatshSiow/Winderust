# Phase 9: Windows and Recovery Boundaries

Status: Complete (2026-08-12)

## Migration rule

Phase 9 moves raw Windows mechanisms only after typed behavioral ownership is established. A move
must keep policy, lifecycle/state ownership, and operating-system calls distinct:

```text
feature policy -> typed controller/application service -> platform/windows adapter
                                                   `-> recovery protocol when restoration requires it
```

This is not a wholesale source-tree shuffle. Each mechanism moves as an independently testable
slice, keeps unsafe blocks local, retains its existing failure and restoration semantics, and adds
a source ownership gate before the next mechanism starts.

## Shared exact-process acquisition

`src/control/process.rs` retains domain and safety ownership: typed targets, stable identity keys,
PID/creation/path/name revalidation, current cross-session policy, and operation-specific critical
or protected-process checks. It no longer imports `windows_sys`, access-mask constants, or unsafe
acquisition calls.

`src/platform/windows/process.rs` maps five typed access purposes to their minimal Windows masks
and owns `GetCurrentProcessId` plus the raw `OpenProcess` call. App Suspension still tries
`PROCESS_SYNCHRONIZE` first and falls back to its existing non-synchronize mask. The adapter returns
typed access-denied, process-exited, or raw-code failure outcomes and imports no feature, foreground,
rule, UI, or control policy.

This boundary covers handles that authorize managed mutations and irreversible commands. Read-only
policy observations remain separate: Process List resource sampling, Core Limiter CPU-time samples,
and Workload Engine CPU-time/age samples may open query-only handles with
`PROCESS_QUERY_LIMITED_INFORMATION`. Those observations select candidates but never authorize a
write; every selected target is reopened and revalidated through the typed control boundary before
mutation. The ownership gate therefore prohibits feature-owned mutation APIs and recovery state,
not feature-specific read-only sampling.

## Timer Resolution slice

Timer Resolution is the first completed physical boundary:

- `src/features/advanced_controls/timer_resolution.rs` owns foreground matching, feature status,
  labels, and Action Log translation.
- `src/control/timer_resolution.rs` owns the active request, replacement ordering, explicit
  shutdown, and Drop fallback.
- `src/platform/windows/timer_resolution.rs` owns the platform contract, WinMM declarations,
  capability conversion, `timeGetDevCaps`, `timeBeginPeriod`, and `timeEndPeriod`.

Every successful begin remains paired with the exact normalized period used for its end. A failed
release retains lifecycle ownership; a successful release followed by a failed replacement leaves
no fictional active request. Timer Resolution remains process-lifetime state and never enters the
external recovery journal.

## Irreversible process-command slice

Memory Trim and process termination retain their Phase 7 command semantics while the raw calls move
behind Windows adapters:

- `MemoryTrimController` owns exact-target opening, protected-process and cross-session policy,
  sampling order, freed-byte calculation, and typed command outcomes.
- `src/platform/windows/memory_trim.rs` owns working-set and CPU-time reads plus the sole
  `SetProcessWorkingSetSize` call.
- `ProcessTerminationController` owns complete-batch preflight, retained handles, attempt-all
  execution, and ordered partial results.
- `src/platform/windows/process_termination.rs` owns the sole `TerminateProcess` call.

The two adapters share only a small typed classification of Windows process-operation failures.
They do not share command policy, create a generic process service, or introduce restoration for
irreversible operations. Error mapping still distinguishes access denial, process exit, and the
exact failed Windows operation.

## Dynamic Priority Boost slice

`DynamicPriorityBoostController` remains the only live owner of baseline, expected value, owner
precedence, Begin/apply/verify/Commit sequencing, compensation, external-break relinquishment, and
reverse clean release. `src/platform/windows/dynamic_priority_boost.rs` now contains the sole live
`GetProcessPriorityBoost` and `SetProcessPriorityBoost` calls. The external helper keeps its
independent replay-only writer; no recovery protocol or state ownership moved into the adapter.

## Memory Priority slice

`MemoryPriorityController` keeps static-over-Workload arbitration, unknown raw baselines, expected
state, preservation policy, recovery sequencing, compensation, relinquishment, and reverse clean
release. `src/platform/windows/memory_priority.rs` owns the Windows raw-class constants and the sole
live `GetProcessInformation(ProcessMemoryPriority)` / `SetProcessInformation` calls. The crash
executor remains a separate replay-only mirror.

## GPU Priority slice

`GpuPriorityController` keeps exact-process baseline/expected state, owner replacement,
preservation, recovery sequencing, compensation, relinquishment, and reverse clean release.
`src/platform/windows/gpu_priority.rs` owns the sole live D3DKMT query/set calls, raw-class
conversion, and NTSTATUS classification. The observed invalid-parameter-as-temporary-GPU-context
contract remains adapter-local, while crash recovery remains the separate replay-only writer.

## I/O Priority slice

`IoPriorityController` keeps exact-process raw baseline/expected state, preservation, recovery
sequencing, compensation, relinquishment, and reverse clean release.
`src/platform/windows/io_priority.rs` owns the undocumented NT declarations, numeric process
information class 33, sole live query/set calls, and NTSTATUS classification. Unknown raw values
remain exactly restorable, and crash recovery remains the separate replay-only writer.

## Process Priority and Power Throttling slice

`PriorityEfficiencyController` remains the single compound owner for Process Priority and process
Power Throttling. It keeps exact-process baselines, owner arbitration, Efficiency Mode's two-step
transaction, verification, compensation, recovery-journal relinquishment, and reverse clean
release. `src/platform/windows/priority_efficiency.rs` owns the priority-class constants, Power
Throttling state conversion, error classification, and the sole live `GetPriorityClass` /
`SetPriorityClass` and `GetProcessInformation(ProcessPowerThrottling)` /
`SetProcessInformation` calls. Winderust self-power and the crash helper retain their separate,
explicitly allowlisted ownership contracts.

## Thread Priority slice

`ThreadPriorityController` retains process-plus-thread identity, per-thread baselines and expected
values, owner replacement, preservation, recovery sequencing, compensation, relinquishment, and
reverse clean release. `src/platform/windows/thread_priority.rs` owns Toolhelp enumeration,
minimal-rights thread acquisition, owner PID and creation-time reads, priority constant mapping,
and the sole live `GetThreadPriority` / `SetThreadPriority` calls. The controller still decides
whether those raw observations match the verified process and exact thread identity. The crash
helper keeps its independent replay-only thread path.

## CPU allocation slice

`CpuAllocationCoordinator` retains exact-process claims, CPU Sets (Soft) > Processor Affinity
(Hard) > Core Limiter > Adaptive Engine precedence, mutually exclusive live properties, first
baselines, pass-end handoff/retry state, recovery sequencing, compensation, external-break
relinquishment, and reverse clean release. `src/platform/windows/cpu_allocation.rs` owns affinity
and default CPU Set query/set calls plus the packed `GetSystemCpuSetInformation` topology-buffer
conversion. The crash helper remains the independent replay-only writer for both properties.

## App Suspension slice

`SuspensionController` retains exact-process acquisition policy, session/service safeguards,
deterministic job naming, Begin/helper-acknowledgement/Commit sequencing, explicit frozen and
cleanup-pending states, bounded retry, and aggregate clean shutdown. The narrow
`src/platform/windows/suspension.rs` adapter owns named Job Object creation, membership checks,
assignment, freeze/thaw, Win32 error classification, and the shared undocumented information-class
layout. Crash recovery imports that layout but keeps its independent retained-handle replay path.

## Winderust self-power slice

`SelfPowerController` retains strict first-baseline capture, hidden/Adaptive request composition,
expected-state tracking, verified compound application, reverse compensation, external-state
rebasing, bounded retry, and explicit idempotent shutdown. The narrow
`src/platform/windows/self_power.rs` adapter owns the current-process pseudo-handle and the raw
priority-class and Power Throttling query/set calls. It reuses the platform-layer typed Power
Throttling value shape without importing feature, controller, rules, foreground, or UI policy.

This remains process-lifetime state: clean disable/shutdown restores the exact captured values,
while process termination ends the controlled Winderust instance. It deliberately has no external
recovery-journal entry or crash-helper writer.

## Power-plan and processor-setting slice

`PowerPlanController` retains ordinary/Adaptive owner arbitration, first baselines, expected GUIDs,
recovery Begin/apply/verify/Commit sequencing, compensation, external-state rebasing, retry, managed
plan lifecycle, and clean shutdown. `src/power/powercfg.rs` remains the domain façade for plan
enumeration, exact `Winderust Adaptive` name/description recognition, typed processor values, and
the ten-stage persistent apply contract.

`src/platform/windows/power_plan.rs` now owns GUID parsing/formatting, native string buffers,
enumeration, active-plan read/write, scheme duplicate/delete/metadata calls, AC/DC setting
read/write calls, and effective-power-mode callback registration. Automatic switching, recovery
replay, startup cleanup, and explicit Advanced Power Plan Tuning reuse this raw adapter while
retaining separate controller/application ownership. Persistent tuning remains outside the
automatic recovery journal.

## Recovery replay boundary

`src/backend/crash_recovery.rs` remains the independent protocol, helper-process, journal, and
replay authority. Process and thread replay deliberately keeps reduced, recovery-only raw writers
instead of calling normal live adapters whose handle, access, verification, or controller
lifecycle contracts differ. App Suspension shares only its compatibility-sensitive freeze layout;
the helper still owns its independently retained Job handle. Power-plan replay reuses the Windows
power-plan adapter because GUID-based activation has the same identity and access contract in both
processes.

No UI or feature policy owns recovery commands, entries, baselines, or replay. This is an explicit
boundary decision rather than a temporary duplicate mutation path.

## Structural gates

`scripts/check_architecture_ownership.ps1` verifies that:

- raw Timer Resolution writes occur only in the Windows adapter;
- exactly one platform contract and one begin/end adapter call exist;
- neither feature policy nor the lifecycle controller imports raw WinMM names;
- the controller remains outside crash-recovery types and managed-baseline ownership.
- working-set trim and termination each have exactly one raw adapter call;
- their command controllers contain no `windows_sys`, unsafe block, or raw mutation API;
- both command families remain outside recovery and Drop-based restoration.
- shared process-control validation contains no raw acquisition API or unsafe block;
- the Windows acquisition adapter contains exactly one access contract and `OpenProcess` call and
  imports no policy/controller layer.
- Dynamic Priority Boost has one live setter in its Windows adapter, no raw Win32 in its controller,
  and one separately allowlisted crash-recovery writer.
- Memory Priority has one live setter in its Windows adapter, preserves raw unknown values across
  the boundary, and contains no raw Win32 in its controller.
- GPU Priority has one live setter in its Windows adapter, contains no raw WDK in its controller,
  and keeps temporary context classification local to the platform boundary.
- I/O Priority has one live setter declaration/call pair in its Windows adapter and no raw NT
  declarations or unsafe calls in its controller.
- Process Priority and Power Throttling each have one live setter in their shared Windows adapter,
  while the compound controller contains no raw Win32 imports, structures, or unsafe blocks.
- Thread Priority has one live setter in its Windows adapter; the controller contains no raw
  Toolhelp, thread-handle, or unsafe calls, and the adapter imports no policy layer.
- CPU allocation has one live affinity setter and one live CPU Set setter in its Windows adapter;
  the coordinator contains no raw Windows calls or unsafe blocks, and the adapter imports no
  feature/arbitration policy.
- App Suspension has one normal freeze/thaw setter in its Windows adapter, one shared layout
  definition, no raw Job Object calls in its controller, and an independently allowlisted crash
  replay writer.
- Winderust self-power has one priority setter and one Power Throttling setter in its Windows
  adapter; its controller contains no raw Win32 types, calls, or unsafe blocks, and the adapter
  imports no policy layer.
- power-plan native writes occur only in one Windows adapter, with exactly one active-plan,
  duplicate, delete, A/C-setting, and battery-setting call; the power domain façade contains no
  raw Windows imports or unsafe blocks.

## Validation so far

- Focused locked Timer Resolution tests: 11 passed.
- Focused locked Memory Trim tests: 11 passed.
- Focused locked process-termination tests: 4 passed.
- Shared process identity and access-profile tests: 4 passed.
- Focused locked Dynamic Priority Boost tests: 19 passed, 1 ignored.
- Focused locked Memory Priority tests: 19 passed, 2 ignored.
- Focused locked GPU Priority tests: 25 passed, 2 ignored.
- Focused locked I/O Priority tests: 22 passed, 1 ignored.
- Focused locked Process Priority and Power Throttling tests: 13 passed, 1 ignored.
- Focused locked Thread Priority tests: 28 passed, 1 ignored.
- Focused locked CPU allocation tests: 38 passed, 1 ignored.
- Focused locked App Suspension tests: 75 passed, 1 ignored.
- Focused locked Winderust self-power tests: 6 passed.
- Focused locked power-plan tests: 51 passed, 1 ignored.
- Full locked suite: 625 passed, 12 ignored.
- Strict all-target Clippy with warnings and unsafe operations denied: passed.
- Release-profile build in `target-next`: passed.
- Architecture ownership gate: passed.
- Formatting completed for the platform and controller files.

The phase is closed. Managed mutation APIs are reachable only through typed lifecycle/application
owners and narrow Windows adapters, while crash recovery remains an explicit independent replay
boundary.
