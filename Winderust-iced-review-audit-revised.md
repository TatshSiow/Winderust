# Winderust `port/iced-review` Code Audit

**Repository:** `TatshSiow/Winderust`\
**Branch:** `port/iced-review`\
**Reviewed head:** `8974433c4955213f11256682617e323a4f675e70`\
**Audit date:** 2026-09-16\
**Comparison base:** `main`
(`cab3186a33ec51665812874149fe7e74c2f2913f`)\
**Branch delta at review:** 73 commits ahead, 0 behind

**Document revision:** 2 — corrected after source verification  
**Revision date:** 2026-09-16

> This revision supersedes the original audit's classifications, recommended
> fixes, regression expectations, and merge assessment. Finding IDs are retained
> for traceability. “Withdrawn” means the previous allegation is not supported,
> not that a code fix was applied. Repository references are pinned to the
> reviewed commit; this document makes no claim about later commits.

## 1. Scope

This audit focuses on correctness and regression risk in the
`port/iced-review` branch, particularly the areas changed or affected by
the Iced port:

-   Iced application lifecycle and update loop
-   Native Windows window/tray integration
-   Shutdown and restoration behavior
-   Settings coordination and persistence
-   Startup and elevation behavior
-   Runtime/process-control lifecycle
-   Async/background task integration
-   Windows API resource handling

The audit is intended to identify concrete bugs, unsafe lifecycle
behavior, and questionable implementations. Purely stylistic issues are
excluded unless they create a meaningful correctness or maintenance
risk.

### Verification limits

The revised findings reflect the source-level re-verification of the original
seven findings, including callers, shutdown ownership, watchdog recovery,
settings projection, existing tests, and documented product intent. This is
not a fresh exhaustive audit of every project file.

No Windows build, test suite, Explorer restart, injected restoration failure,
or interactive Iced integration test was executed for this revision. Existing
tests cited below were inspected as evidence of intended behavior, not rerun.
“Confirmed in source” does not mean a failure was reproduced on Windows.
The test matrix describes proposed acceptance checks, not successful results.

## 2. Executive Summary

**The original audit overstated its conclusions. The seven IDs do not represent
seven confirmed bugs, and the two original P1 merge blockers are not retained
as written.**

The corrected disposition is:

| Classification | Finding IDs | Disposition |
| --- | --- | --- |
| Source-supported tray recovery gaps | AUDIT-003, AUDIT-004 | Keep as P2 findings; verify fixes on Windows. |
| Findings with narrower supported scope | AUDIT-001, AUDIT-006 | Rewrite as a P2 shutdown-result/lifecycle issue and a P3 persistence/editor consistency issue. |
| Optional usability concern | AUDIT-005 | Downgrade to P3/UX; not a demonstrated correctness defect. |
| Withdrawn defect allegations | AUDIT-002, AUDIT-007 | Remove from the active defect list; preserve intentional close semantics and portable settings. |

The most important correction is the shutdown call chain: the UI invokes
`RuntimeHandle::shutdown()`, which consumes and joins its worker. A later UI
shutdown request does not call the same live `RuntimeCore` again. The external
watchdog also performs recovery retries after its input pipe closes. Changing
only `RuntimeCore::shutdown_started` therefore does not implement a valid
UI-level retry fix. See [runtime lifecycle][runtime] and [watchdog recovery][recovery].

The close-handling allegation did not establish duplicate termination or lost
unsaved changes. Background/minimized close behavior is explicitly tested, and
quit requests converge on application confirmation. Import also already
normalizes its runtime projection, and portable settings are documented.
See [tray behavior and tests][tray], [application close handling][app],
[settings projection][settings], and [Iced port scope][port].

This revision does not establish a mandatory P1 blocker or certify the branch
as production-ready. It identifies the remaining source-supported work without
turning architectural preferences into required fixes.

---

## 3. Findings

AUDIT-002 and AUDIT-007 remain below as withdrawn records only. They are not
active defects or implementation tasks.

### AUDIT-001 — Worker shutdown failure is not preserved across subsequent shutdown requests

**Severity:** Medium  
**Priority:** P2  
**Status:** Open — rewritten; original causal explanation and P1 classification withdrawn  
**Evidence confidence:** High for the call chain; failure injection not performed  
**Area:** Runtime shutdown, error reporting, and recovery handoff  
**Files:** `src/backend/automation.rs`, `src/backend/automation/runner.rs`,
`src/ui/app.rs`, `src/backend/crash_recovery.rs`, `src/main.rs`

#### Verified behavior

The application calls `RuntimeHandle::shutdown()`, not `RuntimeCore::shutdown()`
directly. The handle sets `stop_requested`, stops its watchers, removes the
worker `JoinHandle` with `.take()`, and joins it. A worker shutdown error is
included in that call's returned error. The worker owns a local `RuntimeCore`
and returns its cleanup result as it exits. [Sources: runtime wrapper and worker][runtime],
[RuntimeCore cleanup][runner], [UI shutdown][app].

On a subsequent `RuntimeHandle::shutdown()` call, the worker handle is already
absent. That call cannot rerun the exited worker's cleanup. If remaining
self-power cleanup succeeds, it can return `Ok(())` without carrying forward
the original worker failure. The failure can be reported initially through
shared status, but it is not retained as an authoritative terminal shutdown
result. [Source: `RuntimeHandle::shutdown()`][runtime].

The relevant sequence is:

```text
First UI shutdown request
    -> RuntimeHandle marks the runtime stopped
    -> takes and joins the worker handle
    -> receives a worker cleanup error
    -> UI stays open and reports the failure

Subsequent UI shutdown request
    -> no worker handle remains
    -> no new attempt through that worker's restoration path
    -> can return success if remaining self-power cleanup succeeds
    -> UI exits

After the Iced application returns
    -> main calls RecoveryClient::finish()
    -> watchdog input closes
    -> helper attempts recovery of remaining journaled state
```

This sequence is supported by [the runtime wrapper][runtime], [the application][app],
[main][main], and [the recovery helper][recovery]. It is not a Windows
failure-injection result.

#### Existing safeguards and limits

`RecoveryClient::finish()` closes the helper's input and waits for it. On input
closure, the helper processes remaining journal entries through
`recover_with_retry()`, with up to three recovery attempts. The original audit's
claim that a second Quit necessarily exits without any further restoration
attempts was therefore incorrect. Recovery attempts are a safeguard, not proof
that every possible cleanup failure will be resolved. [Source: watchdog lifecycle][recovery].

The runtime remains stopped after the first shutdown request. Settings
replacement and worker synchronization honor `stop_requested`, so keeping the
window open does not mean automation resumed. [Source: runtime stopped-state checks][runtime].

#### Corrected impact

The supported defect is that a later shutdown result can stop reflecting an
unresolved worker cleanup failure, while the UI remains open around an already
stopped runtime. The distinction between successful cleanup, failed cleanup,
and recovery deferred to the watchdog is not preserved in the shutdown result.
Permanent unrecovered system modification was not demonstrated.

#### Recommended correction

Preserve the terminal worker outcome at the `RuntimeHandle` boundary. Repeated
shutdown requests must not silently reinterpret a previous worker cleanup
failure as successful cleanup merely because the handle was consumed.

Define an explicit recovery path: either retain or return sufficient state for
a safe retry, or deliberately hand remaining journaled work to the existing
watchdog and communicate that outcome. Account for the watchdog's existing
pipe-closure trigger rather than assuming it can be awaited while its input
remains open. Represent the stopped/degraded runtime honestly while the window
remains visible.

**Do not treat a boolean-to-enum change inside `RuntimeCore` as a complete fix.**
Do not reset the runtime to “Running” when its worker has already exited.

#### Acceptance checks

Inject a worker cleanup failure and verify the first and subsequent shutdown
results, the stopped UI state, and eventual watchdog handoff. Verify both
successful and exhausted watchdog recovery. Successful shutdown must remain
idempotent, and any claimed retry must actually execute a recovery path rather
than return success because no worker handle remains.

---

### AUDIT-002 — Native/Iced close-handling conflict allegation withdrawn

**Severity:** Not assigned — no demonstrated defect  
**Priority:** None  
**Status:** Withdrawn — remove from active defects and merge blockers  
**Area:** Iced / Win32 window lifecycle  
**Files:** `src/backend/tray.rs`, `src/ui/app.rs`

#### Why the finding was withdrawn

The original audit inferred a correctness conflict from the existence of both
native `WM_CLOSE` interception and Iced close handling. It did not demonstrate
a double-processed close, a lost quit request, unintended termination, or
unsaved settings being discarded.

The native handler either hides the window or queues `QUIT_REQUESTED`; it does
not independently terminate the application. The application consumes that
flag and enters `Message::Close`, which displays quit confirmation. Tray Quit
uses the same application-level request. [Sources: tray handler][tray] and
[application update/confirmation][app].

The existing test
`background_and_minimized_closes_prompt_instead_of_hiding` explicitly checks
the focus/minimized-state distinction. This is evidence of implementation
intent, not proof that every native integration scenario is correct.
[Source: tray tests][tray].

| Existing path | Intended behavior to preserve |
| --- | --- |
| Foreground close with hide-to-tray enabled and an installed tray | Hide the window; retain the running application and draft state. |
| Background/minimized `WM_CLOSE` intercepted by the tray subclass | Queue the application quit-confirmation path rather than silently hiding. |
| Close with hide-to-tray disabled | Route to application quit confirmation. |
| Tray Quit | Route to the same application quit-confirmation path. |

These paths are described by [the native handler][tray] and [the application][app].
Hiding a window is not equivalent to quitting; an unsaved-change prompt is not
required merely to retain the same draft in a hidden application. Polling at
250 ms does not, by itself, establish a race condition.

#### Disposition

No mandatory close-handler refactor is justified by this finding. Preserve the
current semantics and retain focused Windows regression coverage. A future
refactor requires a separate demonstrated defect or an explicit product
behavior change; do not homogenize every close path solely to satisfy the
withdrawn audit.

---

### AUDIT-003 — Tray icon is not re-added after taskbar recreation

**Severity:** Medium  
**Priority:** P2  
**Status:** Open — source-supported recovery gap  
**Evidence confidence:** High for the missing handling; Explorer restart not reproduced  
**Area:** Windows tray lifecycle  
**Files:** `src/backend/tray.rs`, `src/ui/app.rs`

#### Verified behavior

The tray implementation installs an icon through `Shell_NotifyIconW` with
`NIM_ADD`. No `TaskbarCreated` registration or handler was identified in the
reviewed tray code. The application's normal synchronization does not reinstall
an icon while `self.tray` remains `Some(...)`. [Sources: tray implementation][tray]
and [tray synchronization][app].

Microsoft documents that, on receiving `TaskbarCreated`, an application should
assume its previously added taskbar icons were removed and add them again.
[Source: Microsoft, Taskbar Creation Notification][taskbar].

```text
Tray icon installed; application may be hidden
    -> Explorer/taskbar restarts
    -> shell-side icon is removed
    -> Rust-side TrayIcon object remains present
    -> ordinary sync path does not re-add the icon
```

This is a source-derived failure scenario, not a reproduced integration test.
The impact is loss of normal tray access. A separate duplicate-launch restore
mechanism exists, so the audit does not claim guaranteed permanent lockout.
[Sources: tray state][tray], [UI synchronization][app], and [single-instance restoration][main].

#### Recommended correction

Register and handle the shell's `TaskbarCreated` message. When tray functionality
is requested, re-add the icon with its identifier, callback, tooltip, and icon
data; report or retain a recoverable failure if re-registration fails.

Separate shell-icon re-registration from initial window subclassing. Do not
blindly invoke full initial installation on a window that is already subclassed:
`subclass_window()` rejects that state. Preserve the existing callback ownership
and close semantics. [Source: installation and subclass lifecycle][tray].

#### Acceptance checks

On Windows, restart Explorer with Winderust both visible and hidden. Verify the
icon returns, Show and Quit work, the window procedure is not subclassed twice,
and failed re-registration remains recoverable. Verify this under the
application's normal elevated execution mode as well.

---

### AUDIT-004 — Failed tray installation is not retried for unchanged intent

**Severity:** Medium  
**Priority:** P2  
**Status:** Open — source-supported recovery gap  
**Evidence confidence:** High for the latch; transient failure not injected  
**Area:** Tray installation / recovery  
**File:** `src/ui/app.rs`

#### Verified behavior

Tray synchronization constructs the following intent tuple:

```rust
let intent = (
    self.settings.general.hide_to_tray,
    self.settings.persisted().general.start_minimized,
);
```

It attempts installation only when the tray is absent and
`self.tray_attempt != Some(intent)`. The attempted intent is stored before the
installation result is known. A failure therefore leaves the tray absent but
marks the same intent as already attempted. [Source: `sync_tray()`][app].

```text
Initial installation fails
    -> tray remains None
    -> tray_attempt remains Some(intent)
    -> same intent on later ticks
    -> no new installation attempt
```

This can turn a transient failure into a persistent loss of tray functionality
for that configuration state. It is not irreversible: changing the relevant
settings or restarting can permit another attempt. [Source: intent/reset logic][app].

#### Recommended correction

Provide bounded retry, a relevant lifecycle-triggered retry, or an explicit
Retry action. Retain protection against repeated installation failures and
repeated error dialogs.

**Do not simply clear the marker and retry on every 250 ms tick.** If the failure
marker is reset, pair that change with an explicit retry deadline or user action.
Keep shell-icon installation state distinct from window subclass ownership.

This finding differs from AUDIT-003: here the initial installation never
succeeded; AUDIT-003 concerns loss of a previously installed shell icon.

#### Acceptance checks

Inject a first-attempt failure, then make the next attempt succeed without
changing the user's tray settings. Verify the supported recovery path, bounded
attempt frequency, no duplicate icon/subclass, and no repeated modal-error loop.

---

### AUDIT-005 — Startup minimization can make a tray error less noticeable

**Severity:** Low / usability  
**Priority:** P3 — optional UX improvement  
**Status:** Downgraded — not a demonstrated correctness defect or merge blocker  
**Evidence confidence:** High for control flow; user impact not tested  
**Area:** Startup error discoverability  
**File:** `src/ui/app.rs`

#### Verified behavior

When `start_minimized` is enabled, a successfully installed tray allows the
window to hide to the tray. When no tray object exists, startup falls back to
ordinary window minimization. A tray installation failure can already be
stored in `error_message` at that point. [Source: `Message::NativeWindow` and
`sync_tray()`][app].

The error is retained for the application's error interface; the fallback is
not hiding the application into a nonexistent tray. No reviewed requirement
establishes that tray failure must override startup minimization.
[Source: startup and error presentation][app].

#### Disposition

Treat this as an optional error-discoverability improvement, not a required
correctness fix. Keeping the window visible after tray initialization failure
is one possible UX decision; retaining a recoverable minimized fallback is
also compatible with the evidence. A new startup setting is not required by
this finding.

If adjusted, verify that the error remains discoverable when the window is
restored. Do not change the successful start-minimized path unintentionally.

---

### AUDIT-006 — Imported shared fields can differ between persisted/editor state and runtime projection

**Severity:** Low  
**Priority:** P3  
**Status:** Open — narrowed to persistence/editor consistency under divergent input  
**Evidence confidence:** High for the normalization asymmetry; import scenario not executed  
**Area:** Settings import, shared AC/battery fields, and runtime projection  
**Files:** `src/application/settings.rs`, `src/config/settings.rs`, `src/config/storage.rs`

#### Verified behavior

Normal Save calls `sync_shared_settings_to_battery()` before persistence. Import
loads and saves the imported settings directly, replaces persisted/draft state,
and refreshes the runtime snapshot. [Source: coordinator Save/import paths][settings].

The runtime projection already invokes the same synchronization function.
Consequently, the claim that runtime normalization must wait until a later Save
is withdrawn. [Source: `runtime_settings_for()`][settings].

For an accepted import with intentionally divergent shared fields, the
source-derived distinction is:

| Representation | Behavior immediately after import |
| --- | --- |
| Persisted settings | Retain imported root/battery differences. |
| Editor draft | Retains those differences; battery-selected access can read the battery draft directly. |
| Runtime snapshot | Receives normalized shared fields through `runtime_settings_for()`. |
| Subsequent normal Save | Synchronizes shared fields before persistence. |

The relevant sources are [coordinator/import/projection/editor access][settings]
and [shared-field definitions][schema]. This scenario was traced in source, not
executed as an import test.

`sync_shared_settings_to_battery()` copies root general settings, advanced
settings, and the shared Adaptive Engine, CPU allocation, and Advanced Power
Plan Tuning preset collections into the battery profile. It also clears a
nested battery profile. It does not make every feature policy identical across
AC and battery. [Source: shared-field synchronization][schema].

#### Corrected impact

Accepted noncanonical input can leave editor/persisted shared values different
from the runtime's normalized values. A later Save can change that stored
representation. This is not evidence that runtime automation immediately uses
unnormalized shared settings or that ordinary canonical exports are broken.

#### Recommended correction

Apply the intended shared-field invariant consistently at the settings
boundary, preferably normalizing accepted imported settings before the first
persistence operation. Alternatively, explicitly reject conflicting shared
values with a useful error. Preserve genuine per-profile feature settings;
do not flatten the AC/battery distinction or reset unrelated configuration.

#### Acceptance checks

Import a schema-valid fixture with deliberately divergent shared fields and
distinct legitimate per-profile feature policies. Inspect persisted state,
both editor profiles, the runtime snapshot, export, and a subsequent Save.
Confirm shared-field consistency without changing legitimate per-profile
values. Retain tests for failed imports and unchanged canonical round trips.

---

### AUDIT-007 — Executable-local settings are intentional portable design

**Severity:** Informational only  
**Priority:** None  
**Status:** Withdrawn from the defect list — documented design intent confirmed  
**Area:** Settings storage  
**Files:** `src/config/storage.rs`, `docs/iced-port.md`

The settings directory is derived from `current_exe()`, and settings are stored
beside the executable. The Iced port documentation explicitly lists portable
settings among the preserved application behaviors. The design-intent question
raised by the original audit is therefore already answered.
[Sources: configuration storage][storage] and [Iced port scope][port].

Executable-local configuration has deployment implications: protected install
locations, permissions after elevated writes, shared installations, update
behavior, and independent portable copies. These are architectural tradeoffs,
not demonstrated defects in the documented portable application.

**Do not relocate settings to a per-user application-data directory as a fix for
this audit.** Such a migration would require a separate product decision and
compatibility plan. Retain this record only as architectural context.

---

## 4. Positive Findings

### 4.1 Settings revision coordination

The new settings coordinator introduces explicit draft/persisted
revisions and stale-patch detection.

This is a substantial correctness improvement over uncoordinated
mutation because runtime-generated patches cannot silently overwrite a
newer persisted settings revision.

Notable concepts include:

-   `SettingsRevision`;
-   `StaleDraft`;
-   `StalePatch`;
-   separate persisted and runtime revisions;
-   runtime projection from draft and persisted settings.

Evidence: [Settings coordinator and runtime projection][settings].

### 4.2 Atomic settings persistence

Settings and exported data use atomic file replacement through
`AtomicWriteFile`.

This substantially reduces the risk of a partially written TOML file
after interruption or write failure.

Evidence: [Atomic settings storage][storage].

### 4.3 Single-instance and elevation handoff

The branch uses a named mutex scoped by executable path and explicitly
supports the elevated replacement waiting for the previous instance to
release ownership.

This avoids a common race where:

1.  unelevated instance starts;
2.  elevated instance launches;
3.  elevated instance sees the original instance and exits as a
    duplicate.

The executable-path hash also permits independent portable copies to
operate separately.

Evidence: [Application startup and single-instance handoff][main].

### 4.4 Thread-priority liveness validation

The reviewed head adds `THREAD_SYNCHRONIZE` access and zero-timeout
`WaitForSingleObject` checks to retained thread handles.

This allows exited threads to be distinguished from live threads before
identity validation/restoration.

That is preferable to treating an exited thread as a generic restoration
failure and reduces the risk associated with thread-ID lifecycle
behavior.

Evidence: [Thread liveness adapter][thread-platform] and [thread controller validation][thread-control].

### 4.5 Explicit restoration ordering

`RuntimeCore::shutdown()` intentionally restores features in reverse
interaction order because multiple Winderust features can modify
overlapping process state.

The restoration-order strategy is separate from AUDIT-001. The corrected
finding concerns preservation of the worker shutdown result, stopped-runtime
UI behavior, and explicit recovery handoff; it does not establish that
restoration ordering is wrong or that watchdog retries are absent.

Evidence: [RuntimeCore restoration order][runner] and [watchdog recovery][recovery].


---

## 5. Recommended Fix Order

### P1 — No substantiated P1 blocker in this corrected set

The original P1 classifications for AUDIT-001 and AUDIT-002 are superseded.
This is not a statement that the entire repository has no high-severity bugs.

### P2 — Address lifecycle and tray recovery

1. **AUDIT-001:** Preserve terminal worker shutdown failures, distinguish the
   stopped runtime from a running one, and make retry/watchdog handoff explicit.
   Fix the `RuntimeHandle` lifecycle, not just the core-level guard.
2. **AUDIT-003:** Re-add the shell icon after taskbar recreation without
   subclassing the same window twice.
3. **AUDIT-004:** Make failed installation recoverable without retry or error spam.

### P3 — Consistency and optional usability

4. **AUDIT-006:** Align imported shared fields across persisted/editor/runtime
   representations while preserving true per-profile feature policies.
5. **AUDIT-005:** Optionally improve startup-error discoverability without
   treating ordinary taskbar minimization as inherently defective.

### Not implementation tasks

**AUDIT-002:** Preserve the intentional close/hide/quit distinction.
**AUDIT-007:** Preserve the documented portable settings model.

These priorities refer to the evidence and qualifications in Section 3, not
seven mandatory fixes or a general rewrite of application lifecycle code.

---

## 6. Suggested Regression Test Matrix

These are proposed checks, not tests run during this revision. Existing
behavior-preservation checks must not be interpreted as a request to change
the product semantics. Remediation checks state the intended acceptance
criteria for the corrected active findings.

| Area | Scenario | Expected result / verification target |
| --- | --- | --- |
| Shutdown | All restoration succeeds | Normal successful shutdown and exit. |
| Shutdown | Worker cleanup fails | Failure is reported; the runtime is represented as stopped/degraded, not silently running. |
| Shutdown | Second request after worker cleanup failure | Original unresolved failure is not silently replaced by success because the worker handle was consumed. |
| Shutdown | Explicit retry or watchdog handoff | The stated recovery path actually executes and its outcome is distinguishable from already-successful cleanup. |
| Shutdown | Remaining watchdog journal on pipe closure | Existing recovery attempts execute; successful and exhausted recovery outcomes are accounted for. |
| Shutdown | Repeated call after complete successful cleanup | Idempotent success without reapplying mutations. |
| Window | Foreground close, hide-to-tray disabled | Existing application quit-confirmation path. |
| Window | Foreground close, hide-to-tray enabled and installed | Window hides; the application and unsaved draft remain alive. No exit prompt is required merely for hiding. |
| Window | Background/minimized close intercepted by tray subclass | Existing prompt-instead-of-hide behavior remains intact. |
| Window | Actual quit with unsaved settings | Cancel, Save and quit, and Quit without saving remain available. |
| Window | Cancel actual quit | Draft state remains; do not imply an already-stopped worker has restarted after an earlier shutdown failure. |
| Tray | Initial installation failure | Error is retained and normal window access remains possible. |
| Tray | Transient failure resolves with unchanged settings | Supported retry path can install the icon without changing user intent. |
| Tray | Persistent installation failure | Attempts and errors are bounded; no 250 ms retry/modal loop. |
| Tray | Explorer restart while visible | Icon is re-added; Show and Quit remain functional. |
| Tray | Explorer restart while hidden | Normal tray access is restored; window subclass is not duplicated. |
| Tray | Re-registration failure | Failure stays recoverable rather than being treated as a valid installed icon. |
| Startup | Start minimized with successful tray installation | Existing successful startup behavior is preserved. |
| Startup | Start minimized with tray installation failure | Recoverable taskbar minimization or an explicitly chosen visible fallback; retained error is available on restore. |
| Settings | Import canonical current TOML | Expected semantic round trip without unrelated changes. |
| Settings | Import supported older-schema TOML | Only supported defaults/compatibility rules apply; unsupported input produces an error rather than assumed migration. |
| Settings | Import divergent shared AC/battery fields | Shared-field consistency is enforced or conflicts are explicitly rejected. |
| Settings | Import distinct legitimate AC/battery feature policies | Per-profile differences remain intact. |
| Settings | Inspect runtime immediately after import | Existing shared-field runtime normalization remains effective. |
| Settings | Export and Save after accepted import | No unexpected later change to already-canonical shared fields. |
| Settings | Import parse/save failure | Failure is reported; prior valid state is not silently replaced. |
| Elevation | Unelevated launch | Elevated replacement acquires the instance cleanly after handoff. |
| Elevation | Duplicate normal launch | Existing-window restoration remains functional in the supported environment. |
| Thread control | Thread exits before restoration | Existing typed exited-thread handling is preserved. |
| Thread control | Thread remains live | Exact identity is validated before restoration. |

---

## 7. Merge Assessment

The original instruction to block merging until AUDIT-001 and AUDIT-002 were
fixed as P1 defects is withdrawn. AUDIT-002 did not demonstrate a conflict, and
the original AUDIT-001 explanation omitted both the runtime wrapper lifecycle
and watchdog recovery.

The remaining source-supported work concerns shutdown-result/lifecycle
consistency, two tray-recovery gaps, and shared-field import consistency.
Startup error discoverability is optional UX work. Portable settings and the
intentional close behavior are not defects to redesign.

Treat the active P2 items as targeted pre-release work and verify their failure
paths on Windows. This static reassessment neither certifies release readiness
nor demonstrates a catastrophic architectural failure. Merge/release decisions
should use the corrected findings and actual test evidence, not the superseded
seven-item mandatory-fix list.

## 8. Audit Status

| Finding | Corrected classification | Priority | Disposition |
| --- | --- | --- | --- |
| AUDIT-001 Worker shutdown result/lifecycle handling | Source-supported narrower issue; watchdog retries already exist | P2 | Open — rewrite supersedes the original causal explanation and P1 classification. |
| AUDIT-002 Native/Iced close conflict | Not demonstrated; intentional behavior is explicitly tested | None | Withdrawn — no mandatory refactor or merge blocker. |
| AUDIT-003 Taskbar recreation loses tray registration | Source-supported recovery gap | P2 | Open — keep; Windows integration validation required. |
| AUDIT-004 Failed installation is not retried for unchanged intent | Source-supported recovery gap | P2 | Open — keep with bounded recovery, not retry spam. |
| AUDIT-005 Startup error discoverability | Optional usability concern | P3 / UX | Downgraded — not a confirmed correctness defect. |
| AUDIT-006 Shared imported fields differ by representation | Source-supported persistence/editor consistency issue | P3 | Open — runtime is already normalized; divergent-input test required. |
| AUDIT-007 Executable-local settings | Documented portable design | None | Withdrawn from defects — retain architectural context only. |

### Revision summary

All seven IDs, the original scope, and the positive observations are retained.
The original AUDIT-001 and AUDIT-002 P1 blockers are removed; AUDIT-003 and
AUDIT-004 remain P2; AUDIT-005 is optional UX; AUDIT-006 is narrowed; and
AUDIT-007 is closed as a defect allegation. The fix order, regression matrix,
and merge assessment are updated consistently. No repository changes were
made, and “open” does not imply a runtime failure was reproduced.

---

## 9. Evidence References

Repository links below are pinned to
`8974433c4955213f11256682617e323a4f675e70`, the snapshot used in the audit and
subsequent verification. The primary evidence locations are:

| Reference | Relevant symbols / sections |
| --- | --- |
| [Runtime lifecycle][runtime] | `RuntimeHandle::shutdown`, `replace_settings`, `sync_worker`, `run_background_automation`. |
| [RuntimeCore cleanup][runner] | `RuntimeCore::shutdown`, restoration order, and the core-level shutdown guard. |
| [Iced application][app] | `Message::NativeWindow`, `Tick`, `WindowClose`, `Close`, `sync_tray`, `shutdown`, and quit/error presentation. |
| [Tray implementation][tray] | `TrayIcon::install`, `subclass_window`, `tray_wnd_proc`, tray Quit, and `background_and_minimized_closes_prompt_instead_of_hiding`. |
| [Watchdog recovery][recovery] | `RecoveryClient::finish`, `finish_recovery_runtime`, `run_watchdog_if_requested`, and `recover_with_retry`. |
| [Application entry point][main] | Runtime/UI/helper ordering and single-instance restore event. |
| [Settings coordinator][settings] | Save/import, `runtime_settings_for`, and `SettingsEditor` dereference by selected profile. |
| [Settings schema][schema] | `battery_profile`, `battery_profile_mut`, and `sync_shared_settings_to_battery`. |
| [Settings storage][storage] | Executable-local `config_dir`, import parsing, and atomic persistence. |
| [Iced port scope][port] | Preserved portable settings and lifecycle boundaries; documented integration-test limitations. |
| [Runtime contracts][contracts] | Restoration safety boundary and external watchdog responsibilities. |
| [Thread platform adapter][thread-platform] / [controller][thread-control] | Retained thread liveness and exact identity validation. |
| [Microsoft taskbar documentation][taskbar] | Taskbar Creation Notification and re-adding notification icons. |

[runtime]: https://github.com/TatshSiow/Winderust/blob/8974433c4955213f11256682617e323a4f675e70/src/backend/automation.rs
[runner]: https://github.com/TatshSiow/Winderust/blob/8974433c4955213f11256682617e323a4f675e70/src/backend/automation/runner.rs
[app]: https://github.com/TatshSiow/Winderust/blob/8974433c4955213f11256682617e323a4f675e70/src/ui/app.rs
[tray]: https://github.com/TatshSiow/Winderust/blob/8974433c4955213f11256682617e323a4f675e70/src/backend/tray.rs
[recovery]: https://github.com/TatshSiow/Winderust/blob/8974433c4955213f11256682617e323a4f675e70/src/backend/crash_recovery.rs
[main]: https://github.com/TatshSiow/Winderust/blob/8974433c4955213f11256682617e323a4f675e70/src/main.rs
[settings]: https://github.com/TatshSiow/Winderust/blob/8974433c4955213f11256682617e323a4f675e70/src/application/settings.rs
[schema]: https://github.com/TatshSiow/Winderust/blob/8974433c4955213f11256682617e323a4f675e70/src/config/settings.rs
[storage]: https://github.com/TatshSiow/Winderust/blob/8974433c4955213f11256682617e323a4f675e70/src/config/storage.rs
[port]: https://github.com/TatshSiow/Winderust/blob/8974433c4955213f11256682617e323a4f675e70/docs/iced-port.md
[contracts]: https://github.com/TatshSiow/Winderust/blob/8974433c4955213f11256682617e323a4f675e70/.agents/memory/25-runtime-contracts.md
[thread-platform]: https://github.com/TatshSiow/Winderust/blob/8974433c4955213f11256682617e323a4f675e70/src/platform/windows/thread_priority.rs
[thread-control]: https://github.com/TatshSiow/Winderust/blob/8974433c4955213f11256682617e323a4f675e70/src/control/thread_priority.rs
[taskbar]: https://learn.microsoft.com/en-us/windows/win32/shell/taskbar#taskbar-creation-notification

---

*This audit remains a static review of the referenced branch snapshot. Win32/Iced
message ordering, Explorer restart, injected cleanup failures, watchdog handoff,
and divergent settings imports require appropriate Windows or isolated unit
integration tests. No such tests are reported as executed in this revision.*
