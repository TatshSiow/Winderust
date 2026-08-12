# Phase 7: Lifecycle Controls and Irreversible Commands

Status: Complete (2026-08-12)

## Completed command boundaries

Memory Trim and Stop Process / Stop Process Tree now share the bounded runtime command FIFO but
retain separate typed results and mechanism adapters:

- `src/control/memory_trim.rs` owns the typed sample/trim command; the sole raw
  `SetProcessWorkingSetSize` call lives in `src/platform/windows/memory_trim.rs`. Automatic policy
  remains in the Memory Trim manager. `Trim now` starts a worker even when automation is otherwise
  idle and always replies; it cannot become a delayed irreversible action.
- `src/control/process_termination.rs` owns complete-batch preflight and result semantics; the sole
  raw `TerminateProcess` call lives in `src/platform/windows/process_termination.rs`. Process List
  keeps confirmation, grouping, and read-side tree capture. The worker re-reads the current
  cross-session setting, retains every verified handle through complete preflight, and then
  attempts every target in children-first order. The confirmation retains the originally captured
  exact roots; after the user responds, a rebuilt tree must still match each root's PID, creation
  time, name, and path.
  Numeric parent links are followed only when both timestamps are known and the child is not older
  than the parent, preventing stale parent PIDs from pulling unrelated processes into the tree.

Both boundaries use PID + creation time + normalized absolute executable path, fail closed for
critical or protected/unverifiable processes, validate operation-specific rights on the mutation
handle, and remain deliberately absent from recovery entries, baselines, managed claims, and Drop
restoration. Their exact handle acquisition routes through `src/platform/windows/process.rs`,
while `src/control/process.rs` retains target, identity, session, and safety semantics.

Focused controller, worker reply, tree-order, source ownership, formatting, compile, and strict
clippy gates pass for these two slices.

## Completed process-lifetime boundary

`src/control/timer_resolution.rs` owns the active WinMM request lifecycle. Replacing a request
releases the exact successful prior period before beginning the next one; disable and explicit
RuntimeCore shutdown release it idempotently, with Drop retained only as a last-resort backstop.
`src/platform/windows/timer_resolution.rs` is the sole raw `timeGetDevCaps` / `timeBeginPeriod` /
`timeEndPeriod` adapter. Foreground rule matching, labels, snapshots, and Action Log translation
remain feature policy. This state is scoped to Winderust's process and deliberately has no
external crash-journal entry.

Winderust self-power already has strict captured-baseline composition and likewise needs no
external crash-journal entry.

## Completed App Suspension boundary

`src/control/suspension.rs` is now the sole normal Job Object assignment, freeze, thaw, transaction,
retry, and clean-release owner. `AppSuspensionManager` retains rule matching, background grace,
network/audio/user-intent wake policy, failure suppression, snapshots, and Action Log translation.
Automatic rules, App-page Freeze, and Process List Suspend/Resume all route through RuntimeCore and
the same controller; both UI commands wait off the GPUI thread for typed completion.

The controller binds acquisition to PID + creation time + normalized absolute executable path and
revalidates critical/PPL/access/session/service safety on the assignment handle. Cross-session policy
applies when acquiring a new job. Resume and cleanup operate on the already-owned exact job and do
not reapply that acquisition policy, so changing the setting cannot strand state Winderust owns.

The freeze transaction remains Begin -> helper handle acknowledgement -> apply -> Commit. Explicit
states distinguish frozen, failed-compensation, and thawed-but-cleanup-pending jobs. Failed thaw or
helper finalization retains the handle and uses bounded retry on later App Suspension passes without
requiring another user action. Failed freeze attempts do not leave an idle thawed claim, and policy
records are pruned by exact creation time when pending compensation completes. Explicit RuntimeCore
shutdown attempts all releases and reports an aggregate error, with controller Drop and the watchdog
as backstops.
Because Windows retains a Job Object while an associated process remains alive, a later Suspend can
encounter the exact released name. The adapter reuses it only when `IsProcessInJob` proves that exact
target is already a member; unrelated name collisions fail closed.

The raw normal Job Object boundary is now `src/platform/windows/suspension.rs`. It owns creation,
membership, assignment, freeze/thaw, error classification, and the one shared undocumented layout;
the controller retains every policy, transaction, retry, and ownership decision.

The watchdog now thaws its retained exact named job without requiring the recorded root process to
remain alive. Windows Job Objects normally include subsequently created descendants, so a root may
exit while a frozen child remains. An ignored Windows integration test creates that shape and proves
recovery succeeds after the root identity is no longer addressable.

The old feature-owned `process_freezer.rs` writer and both fire-and-forget App Suspension queues are
deleted. The architecture gate permits `SetInformationJobObject` only in the Windows adapter and
the independent crash-recovery mirror, asserts exactly one normal production call and layout, and
rejects raw Job Object APIs from the controller.

## Validation

- Controller transaction/error/layout suite: 15 passed, with one Windows integration test ignored
  by default.
- App Suspension policy/controller suite: 58 passed.
- Runtime command suite: 84 passed, including typed FIFO replies, shutdown rejection, and
  worker-lifetime coverage.
- Stop-tree identity/lineage suite: 3 passed.
- Full repository suite: 597 passed, 12 ignored, 0 failed.
- Ownership scan: no feature/UI Job Object writer or legacy suspension request queue remains.
- Both ignored Windows suspension tests passed when run explicitly: released named-job reuse and
  root-exit/descendant recovery. They remain ignored by default because they exercise the
  undocumented freeze information class and intentionally create disposable Windows processes.
- Locked optimized release build completed successfully.
