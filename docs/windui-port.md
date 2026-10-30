# Wind UI implementation and validation

Status: the complete 34-page presentation uses Wind UI on branch
`port/wind-ui`, 2026-10-19. Wind UI is the sole production UI framework;
Iced and its presentation dependencies have been removed. This guide records
implementation boundaries, native framework reuse, reproducible checks, and
remaining Windows release checks. Earlier framework probes do not establish
a performance advantage for this full application.

The target is **Winderust - Windows Performance & Power Manager** with a
resident application and optional UI windows. Keep the existing application,
runtime, controller, and Windows adapter boundaries. Port presentation and its
lifecycle adapters without rewriting working automation mechanisms.

## Baseline and sources of truth

The current visual refinement follows the user's latest WindInput reference:
restrained blue-gray surfaces, regular Segoe UI setting labels, compact outlined
secondary controls and a single 22-pixel page title. Process List uses quiet
native clickable names, compact commands and readable side-panel settings.
The original Iced source at `3d329a9` remains a behavior/composition reference.
Preserve configured accent, optional status/settings/preset panels, native
Windows frame, full-width Save/Discard footer and corrected shared help scroll.
[DESIGN.md](../DESIGN.md) records the current contract. Earlier dated styling
records below describe their own builds. Routes, settings, window behavior
and runtime safety remain the behavior reference.

The implementation pins crate `windui` 0.28.0 to commit
[`d5c105067d7a31f0f14e388f1fe6394f1a043988`](https://github.com/huanfeng/wind-ui-rust/tree/d5c105067d7a31f0f14e388f1fe6394f1a043988)
in `Cargo.toml` and `Cargo.lock`. It disables upstream default features and
uses the software renderer. Accelerated rendering remains a later measured
choice, rather than an implied advantage of this foundation.

The dependency was upgraded to 0.28.0 on 2026-10-23. The resident-host,
tray, quit, and channel contracts were rechecked against that revision.
Integration changes adapt vertical scroll-range and tooltip text access and
pass the resolved animation preference through `App::resident(...).motion(...)`
so startup preserves On, Off, and Follow system.
The measurements and dated validation records below retain their original build
scope, including the former 0.24.0 revision
[`36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2`](https://github.com/huanfeng/wind-ui-rust/tree/36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2).
Those results do not establish the upgrade's performance effect.

The [0.28 upgrade validation report](../benchmark/ui-windui-upgrade-20261023.md)
records the successful builds and Rust checks, render rechecks, and remaining
native lifecycle limitations. Both attempted background cohorts stopped before
measurement; they establish no new CPU-cycle or memory advantage.
Upstream released 0.29 and 0.30 during validation. The report records those
releases separately; 0.28 remains this tested pin and is not the latest release.

Earlier investigation used Wind UI 0.23.0 at
[`279058d7b616cb9f49b84f071a9c901876d9b520`](https://github.com/huanfeng/wind-ui-rust/tree/279058d7b616cb9f49b84f071a9c901876d9b520).
That historical evidence is identified separately below. It does not establish
0.28.0 behavior or performance of the Winderust implementation. Pin and review
any later upgrade; `main` and design documents can change.

Use these Winderust contracts during the port:

- [Architecture](architecture.md): application, runtime, policy, and controller ownership.
- [Development guide](../.agents/memory/10-development-guide.md): Rust practices, checks, and contribution workflow.
- [Design specification](../.agents/memory/15-design-spec.md): current layout, interaction, and terminology.
- [Runtime contracts](../.agents/memory/25-runtime-contracts.md): safety, restoration, settings, and recovery.
- [Historical Iced implementation](iced-port.md): the previous feature coverage and recorded test entry points.

Preserve behavior from the current implementation and these maintained
contracts. Historical screenshots and port audits provide context; they must
not override later product decisions. Build the page inventory from
[`Page` and `Page::sections()`](../src/ui.rs) and the render dispatch in
[`WinderustApp`](../src/ui/app.rs), including dialogs, profile variants, and
disabled/error states.

## Resident application implementation

Run the application from the repository root:

```powershell
cargo run --locked
```

This uses the existing portable settings, runtime, recovery helper, elevation,
and single-instance startup path. A normal run can apply existing configured
automation. Use the isolated opt-in smoke drivers when testing presentation
without applying real configuration.

- [`src/ui/app.rs`](../src/ui/app.rs) owns one `SettingsEditor`,
  runtime handle, recovery client, restore event, application signal scope, and
  shutdown state for the lifetime of the resident application. Closing and
  recreating its keyed main window preserves drafts.
- [`src/backend/window_visibility.rs`](../src/backend/window_visibility.rs)
  supplies the framework-independent visibility contract. The application
  observes native window state during application events and owned native visibility notifications: hidden or absent means
  background mode; minimized remains visible for existing runtime/self-power
  policy. A blocked native restore listener delivers second-launch requests
  through the application channel and opens/restores the current keyed main
  window without an idle polling loop.
- Production startup opens an accessible main window first. An off-thread
  tray-registration probe must succeed before close-to-zero-window is allowed
  or Start in system tray closes that initial window. Failed or overdue probes
  leave or reopen the window. Only one probe is outstanding; the UI does not
  block waiting for Explorer. This is an accessibility fallback, not evidence
  that every tray or Explorer-restart scenario has passed.
- Exit confirmation and cleanup are application-owned. A worker shuts down
  the runtime and then finalizes recovery. Only its typed completion permits
  application-wide native quit; repeated exit and editing intents cannot
  restart or bypass restoration.
- [`src/platform/windows/resident_shell.rs`](../src/platform/windows/resident_shell.rs)
  contains the native host guard, current UI-window lookup, visibility query,
  tray probe, and native popup hook. This narrow
  adapter depends on pinned private upstream class names and tray identity;
  upgrading Wind UI requires source review and native regression checks.

The opt-in lifecycle driver uses in-memory settings with automation disabled:

```powershell
cargo run --locked --features render-smoke -- --windui-lifecycle-smoke
```

The driver exercises zero-window channel delivery, three close/reopen cycles,
draft retention, exit cancellation, coalesced wakes, rejection of edits during
cleanup, and restoration followed by loop termination. It deliberately bypasses
the production tray-availability gate to isolate lifecycle behavior from
Explorer. A successful run covers those assertions only; it does not test
native tray clicks, Explorer restart, all window states, process mutations,
full UI parity, or comparative performance.

The final implementation's evidence is recorded under Validation results.
Historical foundation runs included the previous Iced presentation and are
excluded from that record.

The 2026-10-22 background optimization removes the unconditional application
interval, uses native visibility and health notifications, retains bounded live
tray checks, and caches unchanged tray-menu data. The fresh
[event-driven background comparison](../benchmark/ui-idle-event-driven-20261022.md)
measured a median paired cycle reduction of 60.44% against the previous Wind
integration. Iced still used fewer background cycles in all three blocks;
Wind used about half its private resident RAM. Enabled scheduler and energy
costs were not retested in that cohort.

The subsequent native idle cleanup uses the pinned upstream's blocking event
loop and channel wakeups without a framework patch. Tray verification runs at
startup, after `TaskbarCreated`, and on settings restore/destruction. Healthy
completion cancels the native deadline; unavailable state retries every five
seconds. This matches Iced's event-driven registration policy: silent tray loss
between events is no longer discovered by a periodic healthy check. Home's
sampler starts lazily, and window destruction releases disposable process,
picker, and log presentation caches while retaining drafts, history, filters,
and paused Process List snapshots. Generation-tagged query completions cannot
refill resources belonging to a destroyed window.

Its fresh nine-capture [native idle comparison](../benchmark/ui-native-idle-cleanup-20261022.md)
measured another 74.19% median paired cycle reduction against the immediate
Wind baseline. Combined application-plus-helper medians were 61,852 cycles/s
and 6.18 MiB private resident RAM, versus Iced's 30,067 cycles/s and 11.97 MiB.
Iced used fewer cycles in every block; Wind's RAM change from its own baseline
was small and mixed. This cold Home, automation-disabled cohort does not measure
enabled schedulers, populated-cache savings, or energy.

## Lifecycle integration and upgrade gates

Use the resident application model demonstrated in upstream
[`examples/resident.rs`](https://github.com/huanfeng/wind-ui-rust/blob/d5c105067d7a31f0f14e388f1fe6394f1a043988/examples/resident.rs):
`App::resident(...).tray(...).run_resident()`. Use `start_window` when launch
policy calls for an initial visible window. Keep one keyed main window through
`Window::single("main")`.

Historical native reproducers against 0.23.0 confirmed channel delivery with
zero, hidden, and minimized windows in resident mode, and clean exit through
the native tray quit callback. They did not validate a complete Winderust port
or the selected 0.28.0 implementation.
The earlier harness used normal window mode and a window-close request from an
application channel; those results do not demonstrate a resident-mode defect.

The following are mandatory integration gates:

| Gate | Verified upstream contract | Required Winderust solution |
| --- | --- | --- |
| Background delivery | Normal window mode defers channels when no window can render; resident mode drains them. | Own the runtime and completion channel at application lifetime and test every window state. |
| Final application exit | Channel callbacks discard window-close effects. `EventCtx::force_close()` does not quit the application there. Only the synchronous `TrayCtx::quit()` path was found to expose application quit publicly. | The native adapter guards resident-host close and posts authorized `WM_CLOSE` after asynchronous restoration finishes. Recheck this private contract on upgrades. |
| Close interception | Resident `App::on_close_request` is ignored. A `Window::on_close_request` handler guards that window only; tray quit and native close of the resident host bypass it. | Route Winderust's explicit exit actions through its shutdown coordinator. Retain recovery for forced termination and session interruption. |
| Hidden-mode policy | A minimized window is still `WindowState.visible`; a zero-window channel callback receives `WindowState::UNKNOWN`, whose `visible` field is true. | Maintain authoritative application visibility state for self-power policy and sampling cadence. |
| Reopen | An allowed close destroys a resident window; reopening reconstructs it. | Preserve durable UI/application state outside window content. Replace cached-window restore listeners. |

Version 0.28.0 still has no public application-level quit request usable after
an asynchronous channel completion. The selected adapter finds exactly one
`WindUiAppHostClass` host on the UI thread, validates its process/thread and
installed subclass, and blocks premature host `WM_CLOSE` by enqueueing an exit
intent. After cleanup, authorized host close reaches upstream `quit_app`, which
destroys registered UI windows and ends the resident loop. Posting `WM_QUIT`
alone would skip that teardown and is not the selected mechanism.

The same pinned private contract identifies UI windows by
`WindUiWindowClass` and the tray icon by the host HWND plus native ID `1`.
The off-thread probe validates host identity around
[`Shell_NotifyIconGetRect`](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyicongetrect).
Upstream initial tray installation failure does not preserve a tray state to
retry, so its restart recovery cannot replace Winderust's visible fallback.
The framework's public API does not report registration failure.

Wind UI owns the icon, left-click delivery, and its Explorer-restart handling.
Its public `TrayMenuItem` supports flat actions and separators, without nested
menus, dynamic visibility, or replacing a registered menu. Winderust retains
the existing native Win32 popup for grouped feature toggles, the active A/C or
Battery profile, advanced-control visibility, localization, Show, and Exit.
The adapter intercepts the pinned host's icon `WM_RBUTTONUP` notification
(`WM_APP + 1`, icon ID `1`) before upstream dispatch, shows the popup from a
snapshot, and queues typed intents. Show requests main-window restoration;
it never shows the hidden resident host. This is one tray icon with a
narrow native menu adapter. It must be retested with actual clicks and Explorer
restart before release.

The pinned public mouse event exposes Left, Right, and Middle only. The native
adapter supplies mouse Back/Forward through a thread-scoped
[`WH_GETMESSAGE` listener](https://learn.microsoft.com/en-us/windows/win32/winmsg/getmsgproc).
It validates each target's live process, UI thread, and `WindUiWindowClass`, so
closing and recreating a window does not leave a stale HWND. Only removed
XBUTTON1/2 messages are consumed; a press queues a typed history intent and its
release is consumed to avoid Windows generating a second
[`WM_APPCOMMAND`](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-appcommand).
Unrelated native windows and other mouse buttons retain their normal behavior.
The resident shell owns this listener and removes it at teardown.

[`resources/winderust.manifest`](../resources/winderust.manifest), embedded by
[`build.rs`](../build.rs) as manifest resource `1`, enables Common Controls 6.
Microsoft documents this activation requirement for
[`GetWindowSubclass`](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-getwindowsubclass).
Removing the manifest can prevent the executable from loading before Rust
startup. The class names, host-close teardown, tray ID/callback message, and subclass lifetime
must all be rechecked before changing the dependency revision. Prefer a future
supported upstream application-quit API when it provides the same guarded
lifecycle. Do not synthesize tray clicks as production IPC.

The visibility adapter must distinguish visible, minimized, hidden, absent,
and exiting states, then deliberately map them to Winderust's existing hidden
policy. Window existence and foreground activation are insufficient. Keep raw
Win32 lifecycle handling in `src/platform/windows/`, with a typed interface to
the application and runtime. Update the
[Windows reference library](../.agents/memory/30-reference-library.md) when that
boundary is implemented or changed.

Source contracts are in upstream
[`src/app/mod.rs`](https://github.com/huanfeng/wind-ui-rust/blob/d5c105067d7a31f0f14e388f1fe6394f1a043988/src/app/mod.rs)
(`App::channel`, `UiHost::drain_channels`, resident callbacks),
[`src/event.rs`](https://github.com/huanfeng/wind-ui-rust/blob/d5c105067d7a31f0f14e388f1fe6394f1a043988/src/event.rs)
(`WindowState`),
[`src/platform/tray.rs`](https://github.com/huanfeng/wind-ui-rust/blob/d5c105067d7a31f0f14e388f1fe6394f1a043988/src/platform/tray.rs)
(`TrayCtx`, `TrayHandle`), and
[`src/platform/win32/mod.rs`](https://github.com/huanfeng/wind-ui-rust/blob/d5c105067d7a31f0f14e388f1fe6394f1a043988/src/platform/win32/mod.rs)
(`WM_APP_WAKE`, resident-host `WM_CLOSE`, `quit_app`). Tray installation and
restart behavior are in
[`src/platform/win32/tray.rs`](https://github.com/huanfeng/wind-ui-rust/blob/d5c105067d7a31f0f14e388f1fe6394f1a043988/src/platform/win32/tray.rs).
Cross-check documentation examples against these implementation contracts.

## Ownership and module boundaries

Retain the modular monolith. One application owner coordinates settings, the
runtime facade, recovery, lifecycle, and UI state. Views express intent and
render read models; controllers continue to own Windows mutations.

| Responsibility | Owner | Implementation boundary |
| --- | --- | --- |
| Entry and instance ownership | [`src/main.rs`](../src/main.rs) | One resident UI retains watchdog dispatch, elevation handoff, and single-instance ownership. A native wait forwards restore events through the typed channel without retaining a window HWND. |
| Application composition | [`src/ui/app.rs`](../src/ui/app.rs) | One application-lifetime owner coordinates durable page editors, native window reconstruction, application signals, typed completions, and asynchronous exit. |
| Drafts and persistence | [`src/application/settings.rs`](../src/application/settings.rs) | Retain `SettingsEditor`, revisions, save/cancel/import/export, runtime projections, and startup-setting reconciliation. |
| Runtime facade | [`src/backend/automation.rs`](../src/backend/automation.rs) | Retain `RuntimeHandle`, typed commands, status snapshots, worker scheduling, and auto-exclusion wake behavior. Replace the UI wake/visibility connection only. |
| Self-power | [`src/backend/self_power.rs`](../src/backend/self_power.rs) | Retain composed hidden/Adaptive requests, baseline ownership, verification, compensation, retries, and restoration. |
| Tray and visibility | [`src/backend/tray.rs`](../src/backend/tray.rs), [`src/backend/window_visibility.rs`](../src/backend/window_visibility.rs) | Wind UI owns the icon; the native popup adapter preserves grouped menu behavior. Visibility reaches runtime policy through the framework-independent module. |
| Read-side work | [`src/ui/tasks.rs`](../src/ui/tasks.rs) and page samplers | Typed jobs run off the event thread and return through the application channel; existing read models and query ownership remain intact. |
| Pages and shared controls | [`src/ui.rs`](../src/ui.rs) and `src/ui/` | Canonical page/editor names and business logic are retained; views construct native Wind UI elements with small message/signal bindings. |

Keep framework types under `src/ui/`. Neither `windui::Signal`, `Element`, nor
event contexts belong in settings, runtime, policy, controllers, or process
query models. A native shell adapter may live under `src/platform/windows/`,
but exposes application concepts rather than widget types.

Split lifecycle and worker-to-UI delivery out of `ui/app.rs` only where they
own distinct behavior; `ui/lifecycle.rs` and `ui/events.rs` are possible future
modules, not existing requirements. Keep stateless helpers as functions. Do
not introduce a workspace split, generic frontend trait, dependency-injection
container, or general event bus just to replace the renderer.

### State and signal lifetimes

| Lifetime | Owns | Rules |
| --- | --- | --- |
| Application | `SettingsEditor`, `RuntimeHandle`, recovery client, instance guard, exit state, pending operation IDs, durable navigation and drafts | Must survive closing and recreating every UI window. Keep one authoritative owner of each value. |
| Published application state | Current immutable status/process snapshots and the small set of signals that project them | Update on the UI thread; reuse existing revisions and snapshot sharing. Signals do not become a second settings store. |
| Window | Focus, hover, popovers, transient control bindings, animation and layout state | Create within the window content scope. Dispose with the window; restore only state the product deliberately retains. |
| Worker operation | Owned inputs, cancellation/operation identity, typed result | No UI contexts, borrowed controls, or window-scoped signals cross threads. |

Wind UI scopes signals created by `Window::content` to that window. Reads
through disposed signal handles can panic; stale writes are discarded.
Application callbacks must therefore update durable models or application
signals rather than retaining window-local bindings. Use `try_get`/`try_with`
only when absence is an expected lifecycle outcome, not to hide a broken
ownership invariant. Signals created outside a scope are not automatically
reclaimed: allocate a finite application set or retain an explicit
application `SignalScope`; never allocate unscoped signals on every update.
See upstream
[`src/signal.rs`](https://github.com/huanfeng/wind-ui-rust/blob/d5c105067d7a31f0f14e388f1fe6394f1a043988/src/signal.rs).

Use the framework's retained tree and reactive bindings where they simplify
presentation. Preserve explicit application commands for settings and process
actions. Do not attach persistence or Windows mutations to a signal effect
whose reruns could repeat the operation.

### Worker delivery

Use application-owned typed messages delivered through `App::channel`.
The callback runs on the UI thread; senders may be used by workers. Keep
process enumeration, blocking dialogs, file I/O, network work, and restoration
outside UI callbacks. Preserve the existing execution mechanism where it
already fits; the port does not require adding another async runtime.

- Carry operation IDs or existing settings/snapshot revisions where late
  results could overwrite newer state. Ignore obsolete query results without
  dropping outstanding mutation or restoration outcomes.
- Coalesce replaceable process/status snapshots before sending. The pinned
  channel is unbounded and drains queued messages; a fast producer must not
  build an unlimited UI backlog. Reuse the latest-snapshot/wake pattern where
  available instead of cloning an entire process list for each tick.
- Deliver command results, errors, settings patches, and shutdown completion
  with explicit handling. Handle send failure according to application
  lifecycle; do not silently lose a safety-critical completion.
- Keep existing runtime cadence and retry ownership. Do not create per-page
  timers, poll all features at frame rate, or suspend restoration retries when
  a page or window closes.

The transport implementation is upstream
[`src/sync.rs`](https://github.com/huanfeng/wind-ui-rust/blob/d5c105067d7a31f0f14e388f1fe6394f1a043988/src/sync.rs).

## Startup, background operation, and exit

Preserve the startup ordering and failure handling in `main.rs`: watchdog
argument dispatch before GUI initialization, privilege handling, instance
ownership, portable settings, recovery helper, stale managed-plan recovery,
runtime, and shell. A failed startup must unwind already acquired resources
without leaving managed process state behind.

The session-wide `Local\Winderust.*` instance contract remains shared across
executable copies. A second launch restores or opens the existing main window;
it must not start a second runtime. Replace
the previous cached-HWND restore listener with the application-lifetime
`RestoreListener` guard. Its worker waits on the restore event and a cancellation
event, queues a Show intent, and joins before its handles close. Resolve the
current keyed window on the UI thread, or create it when absent. Preserve the
wait-for-previous-instance behavior during elevation.

For Start in system tray, the resident application opens its main
window until an off-thread probe confirms tray registration, then closes the
window through its normal close guard. This may briefly show the window.
The probe runs on startup, Explorer recovery, and settings restore/destruction.
Successful verification leaves no recurring health timer. Unavailable results
retry after five seconds; if a query is still pending after two seconds, the
shell treats the tray as unavailable and opens
the window. It leaves the single outstanding worker alone rather than blocking
the UI or spawning more stalled queries. Failed registration must never strand
an invisible running utility. Verify actual Explorer restart recovery instead
of assuming the framework tray reproduces all of Winderust's behavior. Do not
install both tray implementations or stack competing window subclasses.

Closing the main window follows Winderust Behaviour and unsaved-change rules.
Preserve drafts and selected profile through close/reopen when the operation
continues in the background. `Window::single("main")` restores a live keyed
window and ignores a later request's content/configuration; use shared state
to update an existing window rather than treating another open request as a
rerender.

Window visibility must reach both the runtime's hidden-mode sampling policy
and `SelfPowerController`. The application publishes through
`window_visibility::set_hidden`, and the runtime consumes
`window_visibility::is_hidden` and `VisibilityWatcher`. The native host wakes
the channel immediately on power-source changes and Explorer's `TaskbarCreated`
notification. A UI-thread lifecycle hook publishes visibility changes. A native
query deadline keeps one reusable tray-query worker and forwards only initial
availability, changes, or errors to the UI. Healthy completion cancels the timer;
only unavailable state schedules another check. An event received during a query
requires a fresh verification after that query finishes. A separate five-second retry timer
exists only while an auto-exclusion persistence retry is pending. Second-launch
restoration uses the native event listener. Hidden or
absent UI enables background policy; minimization alone does not. Hidden mode
must continue automation, pending
cleanup, auto-exclusion patches, and required recovery communication. Only
presentation work should stop because the UI is absent.

Implement exit as an explicit application state transition, for example:

```text
Running -> ExitRequested -> Restoring -> ReadyToExit -> Exited
               |
               +-> Running (user cancels before restoration starts)
```

This diagram describes application intent, not Wind UI API names. The
implementation uses guarded confirmation, restoration, failure/retry, and
ready-to-quit fields. A failed worker start or native quit request keeps the
application alive with a visible retry path.

1. Resolve unsaved-change and exit confirmation before entering restoration.
2. Accept an exit request once. Reject new mutation intents; continue handling
   the completion/recovery messages needed to finish existing work. Port the
   intent of `Message::allowed_during_exit` and its tests.
3. Run `RuntimeHandle::shutdown()` and then recovery-client finalization off
   the UI thread. Keep the resident event loop responsive. Retain current
   timeout, uncertainty, diagnostic, and journal-retention behavior; do not
   mark restoration successful merely because a wait ended.
4. On completion, request application-wide quit through the integration
   contract established above. Keep channels and lifecycle resources alive
   until completion has been consumed. Neither hiding nor destroying the main
   window completes this step.
5. Release the tray, wake registrations, and instance resources in a documented
   order after their work is finished. Return normally from `run_resident()`.

Do not wait for cleanup inside a synchronous tray callback, call
`TrayCtx::quit()` before cleanup, or use `std::process::exit` for normal exit.
RAII should release resources, but fallible restoration must have an explicit
observable path rather than relying only on `Drop`. Preserve recovery-helper
protection and journal semantics for crashes, forced termination, and Windows
session shutdown, where UI-level confirmation or asynchronous cleanup may not
complete.

## Preserve product behavior

Treat the design specification as the visual and interaction contract. Keep
the native Windows title bar and resize behavior, current initial/minimum
sizes, compact navigation, stable table widths, shared control families,
status rails, and modal ownership. Wind UI's design document is architectural
background, not a replacement Winderust design system.

The page inventory includes:

- Home; all landing pages; searchable/compact navigation; About and update flow.
- Power Plan Control: By Foreground, By Running App, By CPU Load, By Activity,
  By Time, and Advanced Power Plan Tuning.
- Priority Control: Process Priority, Thread Priority, Dynamic Priority Boost,
  I/O Priority, GPU Priority, and Memory Priority.
- CPU Control: CPU Limiter, CPU Sets (Soft), and Processor Affinity (Hard).
- Winderust Features: Adaptive Engine, Background Efficiency, and Memory Trim.
- Advanced Controls: App Suspension, Timer Resolution, and Win32 Priority Separation.
- Process List, process details/pickers, inline policy editing, grouping,
  confirmations, column sizing controls, and protected/unavailable states.
- Action Log and all settings pages, with A/C and Battery editing, validation,
  numeric drafts, save/cancel, and import/export behavior.

Use Wind UI's built-in controls, layouts, forms, cards, tables, scrolling,
dialogs, menus, themes, and window behavior wherever its API covers the
requirement. `widgets::Ui` binds native control signals to typed editor
messages; it is not a custom widget set. Native text input edits update the
model without rebuilding the host, preserving input focus and invalid raw
drafts. Structural changes request a refresh. Command handlers validate edits
even when a displayed control has not yet refreshed its derived enabled state.
Dependent Save/Apply controls read the native raw-text signals directly, and
actions flush pending input bindings before dispatch. Invalid or incomplete
numeric text stays visible while the last valid settings value remains intact.
The numeric binding composes Wind UI's native slider and native text input.
The pinned native Stepper exposes only its parsed number signal; its private
text draft cannot support the required invalid-draft and final-keystroke
validation contract through the public API.
Native dialog Escape/backdrop/close changes must also clear the editor's modal
state. Preserve master-switch/configuration independence, read-only presets,
live-page versus preset-editor state, and current grouping.
Editor dialogs use the native card, scroll, and layout elements: only the form
body scrolls, while Save/Cancel and other actions remain in a fixed footer.
The body keeps its natural height up to 420 logical pixels, and the themed
dialog panel has a maximum width of 860 with a 20-pixel viewport margin.

Retain `rust-i18n`, existing locale keys, English and Traditional Chinese,
theme/accent selection, system changes, and the Language and Appearance motion
setting. Idle controls must not schedule continuous redraws. Keep Process List
and Action Log virtualized. Check SVG/icon and font needs before selecting
Wind UI features or deleting existing assets/dependencies.

The port must preserve these application contracts:

- `SettingsEditor` remains the exclusive editing/persistence boundary. Keep
  portable settings beside the executable; no schema migration, legacy alias,
  or AppData fallback is needed for a UI replacement.
- Action Log remains in memory and exports only to a user-selected CSV path.
- Power-plan ownership remains local to each feature/rule; the global A/C
  scheduler pause stays on Power Plan Control. Recover only temporary plans
  identifying the current `Winderust Adaptive` plan.
- Process read models do not authorize mutations. Preserve exact target
  identity checks, protected-process filtering, access checks, conservative
  defaults, and restoration. Cross-session control remains governed by
  Winderust Behaviour and defaults on.

## Native framework reuse and remaining adapters

"Native" in this port means Wind UI's built-in retained widgets. Wind UI
renders those widgets itself; this does not imply that every control is a
Windows common control.

| Application surface | Built-in implementation |
| --- | --- |
| Text, actions, feature flags | TextInput, Button, Switch, Checkbox, native tooltips and disabled states |
| Choices and navigation | Dropdown/MenuItem, Segmented, Collapsible, native layout containers and SVG icons |
| Numeric fields | Slider plus TextInput with typed range validation and preserved raw drafts |
| Layout and transient surfaces | Card, Surface roles, Grid/Row/Column/Stack, Dialog, native Scroll |
| Dense data | VirtualTable with native header sorting/selection and menu actions; persistent row signals |
| Appearance | Native ThemeHandle, roles, ColorPicker, system animation preference and explicit native locale catalog |
| Resident shell | App::resident, native Tray icon, keyed Window::single, application channels and timers |
| Operating-system dialogs | Existing native Windows file and saved-color dialogs |

The remaining custom code covers concrete API gaps:

- Home's time-series chart paints the application data because this revision
  has no built-in chart. Nonvisual binding, scroll-memory and viewport widgets
  preserve typed edit ordering, page offsets and current-frame rail visibility;
  they do not paint replacement controls.
- The resident Win32 adapter supplies guarded asynchronous quit, grouped tray
  menu behavior, reliable visibility, restore-event delivery, and mouse
  Back/Forward where public Wind UI contracts are insufficient. Its private
  upstream assumptions are pinned and documented above.
- Process List column widths are native table weights. The existing sizing
  controls adjust those proportions. Wind UI's current table/scroll API does
  not provide the previous pixel-width horizontal overflow or header drag
  resizing; long values use tooltips and details. This presentation limitation
  is retained explicitly instead of adding another custom table.

## Shared native presentation

The earlier user-supplied WindInput screenshots established native control reuse. Its
[settings application](https://github.com/huanfeng/wind-setting/tree/559c352dd9276282e483d0fc6ff875230ea23a2d)
also composes Wind UI's built-in elements. The current contract is recorded in
[DESIGN.md](../DESIGN.md). Winderust keeps its product identity, native Windows
frame, existing routes, settings ownership and operational status panels.

The current shared theme uses neutral dark and light surfaces. Each navigation
section appears once, with inline children, quiet native
clickable rows, SVG icons, selected tint and optional enabled counts. Log,
Settings and About remain below the scrollable feature sections. Breadcrumb
ancestors use clickable native labels; the current destination is plain text.
Pinned Wind UI links take their colors from LinkTheme and ignore `.fg_role()`,
so they cannot implement per-item breadcrumb emphasis with that modifier.

Neutral OutlineSoft buttons cover secondary actions; explicit global and modal
Save actions use the primary variant. On-accent text chooses the better
relative-luminance contrast between white and the existing near-black in both
themes. Empty card titles no longer leave a header/divider. Appearance settings
form two coherent native cards. About groups identity/version/checking controls,
existing link destinations and update preferences. The full-width bottom bar
keeps pending state and Save/Discard/Exit outside the scrolling body.

Supported pages show their right panel by default, matching the original Iced
shell. A bottom Hide side panel / Show side panel toggle leaves an icon-only
56-pixel rail when collapsed; labelled header actions remain available. One
in-memory visibility choice survives navigation, resizing and window recreation
within the resident session. Wide windows reserve 320 pixels for the expanded
panel. Narrow windows reserve the collapsed rail's width and overlay the full
panel, retaining the same visibility choice and editor geometry. A zero-size
nonvisual viewport binding reads Wind UI's current logical `Tree.layout_size`
before layout and updates a signal only when crossing the shared geometry
threshold. Native `visible_when` constraints switch rail geometry without
rebuilding the editor or changing saved settings. Native-tree regressions verify
current-frame geometry and retained input identity, text and focus during resize.

## Rust implementation rules

Apply the repository's Rust engineering practices throughout the port:

- Prefer concrete types, enums for meaningful states/intents, and exhaustive
  matches. Keep modules private by default and expose the smallest useful
  `pub(crate)` boundary. Reuse existing error and identity types.
- Return typed `Result` errors when callers must distinguish failures. Add
  context at the handling boundary; translate user-facing text through the
  locale system. Avoid `unwrap`/`expect` on runtime process, configuration,
  file, network, and Win32 paths; do not silently ignore impossible wiring.
- Keep UI mutation on the UI thread. Use owned `Send` messages across threads;
  use `Arc` only for actual shared ownership and locks only for shared mutable
  data. Do not wrap the whole application in `Arc<Mutex<_>>`. Keep lock and
  `RefCell` borrows short and release them before callbacks or reentrant Win32
  calls. Avoid strong-reference cycles in retained callbacks.
- Own native handles with existing RAII wrappers. Keep each unsafe operation
  in a minimal explicit block with an immediately preceding `SAFETY:` rationale,
  including inside `unsafe fn`. Do not add unsafe `Send`/`Sync` implementations
  to make UI objects cross threads.
- Keep feature changes separate from mechanical porting. Delete superseded
  helpers after tracing callers rather than maintaining two settings stores,
  tray owners, or implementations of the same policy.

Use a reproducible dependency: verify the selected release or exact Git
revision, enable only justified features, and commit `Cargo.lock`. Do not use
a personal absolute path or an ignored benchmark checkout as a dependency.
Review licenses, Windows crate-version interactions, renderer features, and
transitive dependencies; do not blindly copy upstream defaults or change
Winderust's release profile. Choose software/D2D/GPU features with measurements.
Avoid unrelated edition, toolchain, or dependency upgrades in the port.

The Rust Book's
[`Result` guidance](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html)
and Cargo's
[manifest/lockfile guidance](https://doc.rust-lang.org/cargo/guide/cargo-toml-vs-cargo-lock.html)
support these choices. The specific ownership and safety requirements above
are Winderust project rules.

## Implementation coverage and acceptance gates

The implementation on `port/wind-ui` replaces the production presentation with
one resident application. The temporary frontend selector is not part of the
final architecture. Preserve the previous baseline in Git and retain its
settings/interaction fixtures for comparison. The table separates implemented
source areas from the evidence still needed for acceptance.

| Area | Source coverage | Required acceptance evidence |
| --- | --- | --- |
| Resident lifecycle | One runtime/settings owner, tray, visibility bridge, second-instance restore, asynchronous exit | Zero/hidden/minimized-window delivery; repeated destroy/reopen; settings patches while hidden; correct self-power transitions; restoration then final quit; failures leave a visible/reportable outcome. |
| Shared presentation | Native theme/locales, shell/navigation, controls, scrolling, dialogs, status rails | Representative settings pages in both locales/themes; actual input, focus, IME, scrolling, modal and disabled-state checks; no idle redraw loop. |
| Settings and feature pages | Existing editors and validation retained; native views cover the inventory | Save/cancel/import/export and profile parity; revision-safe completions; existing policy tests remain green; no direct UI process mutation. |
| Dense/live pages | Home, Process List/details/pickers, Action Log | Virtualization and column behavior; stale-process handling; action confirmations; no unbounded snapshot backlog; CSV export and error behavior. |
| Production composition | Wind UI is the sole production framework; obsolete presentation dependencies are removed | Full parity/lifecycle matrix, matched performance measurements, required checks, release build and Windows smoke test pass. |

The automated evidence below covers the integrated implementation. The
Windows release checks listed separately still require their own evidence;
rendered screenshots do not establish complete interaction or accessibility
coverage.

## Validation and performance

Keep backend/controller tests unchanged unless their application contract
actually changes. Add focused tests for new lifecycle transitions, stale
completions, draft preservation, and message/visibility adapters. Replace
Iced-specific rendering tests with meaningful equivalents; do not copy tests
that only assert the new implementation's structure.

Record automated and manual coverage separately:

| Area | Required scenarios |
| --- | --- |
| Resident lifecycle | Zero-window resident host; visible, minimized, hidden, and destroyed main window; safe startup-to-tray after positive registration; rapid close/reopen; queued completion after window disposal; second launch in each state. |
| Tray and Windows | Actual shell menu/show/quit interaction, Explorer restart, registration failure, elevation handoff, display/DPI change, suspend/resume, session end, crash/recovery. |
| Shutdown | Quit while hidden or without a window; repeated quit; in-flight command/dialog; worker failure; recovery timeout/unconfirmed result; no new mutation after shutdown begins; restoration before normal exit. |
| Settings | Both profiles; invalid numeric text; canceled dialogs; pending edits through reopen; auto-exclusion updates while hidden; stale import/query completion; unwritable portable directory and export errors. |
| Process safety | Process exits/PID reuse during an action, denied access, protected/critical targets, grouped actions, cross-session setting, existing restoration and recovery tests. Use controlled disposable processes for native mutation tests. |
| UI | Every inventory entry and important state; English/Traditional Chinese; light/dark/system accent; minimum window size and high DPI; long labels; input/focus/IME; table resize/sort/filter/scroll; reduced motion. |

Screenshot coverage establishes visual coverage only. Native interaction,
resident lifecycle, accessibility, and restoration require separate evidence.
Retain any existing deferred accessibility/keyboard scope explicitly; do not
claim full accessibility or silently add a product expansion to this port.
Preserve keyboard/input behavior the current application already supports.

For performance, compare equivalent release builds on the same machine with
the same runtime settings, process data, page content, window lifetime,
sampling cadence, renderer configuration, DPI, and capture method. Measure
startup, window opening/reopening, visible idle, hidden/zero-window idle,
active automation, page changes, large-list scrolling, memory growth over
repeated reopen, and shutdown completion. Report CPU time, private memory,
working set, latency, and redraw/wake activity separately, with repeated runs
and recorded conditions. Set acceptable regression budgets against that
baseline before cutover. Account for software versus accelerated rendering;
zero sampled CPU does not establish zero energy use.

Earlier minimal-host measurements are exploratory and do not establish the
memory or speed of a full port. Store reproducible final procedures and
sanitized evidence in the tracked benchmark/documentation structure; do not
make the release depend on ignored local investigation files.

## Cutover checklist

- [x] Implement and document the selected revision's lifecycle adapters, typed cleanup, and application-lifetime state.
- [x] Convert the complete page inventory, profile variants, feature editors, shared controls, and dialogs to Wind UI.
- [x] Remove Iced types and dependencies from active code and the final manifest; retain used image/icon/native-handle dependencies.
- [x] Retain isolated native render/lifecycle fixtures and the separate opt-in runtime benchmark host.
- [x] Retain controller/recovery ownership gates and add Wind UI boundary checks.
- [x] Update Dependabot, CI, README, architecture, contributing guide, release checklist, and current-stack agent references. Label original Iced reports historical.
- [ ] Before a release, complete actual tray/Explorer, mixed-monitor DPI, IME, suspend/resume, elevation, and session-end checks from the release checklist.
- [ ] Before a release, review the packaged executable/ZIP for GPL-3.0-only notices, Copyright (C) 2026 Tatsh Siow, and absence of personal tooling, settings, diagnostics, graph output, and benchmark checkouts.

For each code/dependency implementation stage, run the repository checks:

```powershell
git diff --check
cargo fmt -- --check
cargo clippy --locked --all-targets -- -D warnings -D unsafe-op-in-unsafe-fn
cargo test --locked
.\scripts\check_architecture_ownership.ps1
.\scripts\check_legacy_names.ps1
graphify update .
```

Use the feature-specific build/test commands applicable to any changed
optional harness as well. Before release, run `scripts/build_release.cmd` and
the [release checklist](release-checklist.md), including the Windows smoke
test and package-content review. A green compiler/test run alone does not
close the native lifecycle acceptance gates.

For the optional native harnesses, also run:

```powershell
cargo clippy --locked --all-targets --features 'render-smoke,runtime-benchmark' -- -D warnings -D unsafe-op-in-unsafe-fn
cargo test --locked --features 'render-smoke,runtime-benchmark'
cargo run --locked --features render-smoke -- --windui-lifecycle-smoke
cargo run --locked --features render-smoke -- --render-smoke
```

The render matrix writes captures and a manifest under
`target/windui-smoke/`. Its fixtures disable automation and skip portable
settings IO and startup registration. Review the resulting frames as well as
the driver's exit status. The `runtime-benchmark` feature retains the separate
automation-engine benchmark host; compiling/testing it does not run a benchmark
or measure UI-framework performance. Use the
[Adaptive Engine benchmark procedure](adaptive-engine-benchmark.md) when
collecting controlled runtime measurements.

For edits confined to this guide, follow the development guide's documentation
validation: check relative links/paths and `git diff --check`.

## Upstream architectural references

- [Wind UI design](https://github.com/huanfeng/wind-ui-rust/blob/d5c105067d7a31f0f14e388f1fe6394f1a043988/docs/DESIGN.md): design intent, not a compatibility guarantee or benchmark result.
- [API guide](https://github.com/huanfeng/wind-ui-rust/blob/d5c105067d7a31f0f14e388f1fe6394f1a043988/docs/API_GUIDE.md): cross-check examples against the pinned implementation.
- [Resident example](https://github.com/huanfeng/wind-ui-rust/blob/d5c105067d7a31f0f14e388f1fe6394f1a043988/examples/resident.rs) and [tray example](https://github.com/huanfeng/wind-ui-rust/blob/d5c105067d7a31f0f14e388f1fe6394f1a043988/examples/tray.rs): application/window lifetime and native tray intentions.
- [Background task example](https://github.com/huanfeng/wind-ui-rust/blob/d5c105067d7a31f0f14e388f1fe6394f1a043988/examples/background_task.rs): worker-to-UI delivery.

## Reproduce background measurements

The tracked [measurement script](../scripts/measure_windui_background.py)
uses Python's standard library and Windows process counters:

```powershell
cargo build --release --locked --features 'render-smoke,runtime-benchmark'
python scripts/measure_windui_background.py --exe target/release/winderust.exe --out target/windui-smoke/background --warmup 6 --duration 15 --trials 2
```

It runs the complete resident application with in-memory settings, automation
and updates disabled, and no portable settings or startup-registration IO.
The production event-driven tray verification and unavailable-state retries remain enabled. Each
trial asserts the actual native UI state throughout sampling: absent, visible,
minimized, or hidden. Separate absent and closed trials distinguish starting
without a UI from destroying a previously displayed UI through its normal
close-to-tray guard. The fixture's timed exit uses normal asynchronous cleanup;
the script waits for that exit and never force-kills the process.
Visible fixtures wait for the native initial show and settle for two seconds
before applying a measured state, followed by the requested warmup. Observing
an HWND alone is insufficient: upstream can still show/restore that window
while completing startup. State assertions continue throughout sampling.

The JSON includes the executable SHA-256, Windows version, samples, CPU time,
median private bytes, and working set for the main application process.
CPU percentage uses one logical core as 100%. Window-creation timing does not
measure completed painting. These short idle trials exclude active automation,
recovery-helper resource usage, power consumption, long-duration leaks, and a
matched Iced comparison. They cannot establish a framework performance winner.

## Validation results

### Navigation and Exit press/refresh race, 2026-10-29

An asynchronous Home sample could rebuild the entire shell between a native
mouse Down and Up. Wind UI 0.28.0 retained the deleted pressed node as its
private capture target, so subsequent navigation and Exit clicks never reached
live controls. About/update and power-plan completions exposed the same race.
A source check of [v0.31.0](https://github.com/huanfeng/wind-ui-rust/blob/v0.31.0/src/core.rs)
and [upstream main at 166d0e7](https://github.com/huanfeng/wind-ui-rust/blob/166d0e7973cd8e915c5c1b643c48409811f263fe/src/core.rs)
found the same dispatch behavior; upgrading alone would not repair it.

The application now coalesces structural fragment revisions while its owned
native UI window has mouse capture. The existing visible-window tick drains
them after release. Model updates, worker completions and the automation engine
continue immediately. Passive page completions refresh their own page without
replacing unrelated navigation, header and footer controls. Process List and
Action Log defer row publication together with callback metadata. A nonvisual
adapter also defers the pinned native virtual body's row realization during
capture, covering wheel/height changes; native scrolling remains active and rows
catch up on release. Viewport breakpoint changes wait until release. No framework
source, dependency pin, persisted setting, runtime policy or timer changes are
involved.

Regression evidence is kept under ignored
`benchmark-results/navigation-capture-20261029/`. A persistent-capture test
holds navigation and Exit across multiple coalesced revisions, releases, then
checks later clicks. Bypassing the guard makes it fail because the pressed
node is deleted. The isolated native lifecycle fixture also posts real Win32
Down/Up messages, verifies actual own-window capture, forces a refresh during
each held press, then checks navigation, Exit confirmation and queued refresh
delivery after release. These checks exercise the native message loop without
requiring physical mouse input or changing saved settings.

Validation passed: 911 standard tests and 933 with both optional harness features
(19 existing ignored in each run), strict Clippy in both configurations,
formatting, whitespace, architecture, legacy-name checks and an AST graph update.
Three native lifecycle trials passed, including six held navigation/Exit presses
and nine destroy/reopen cycles. The 128-case native render matrix passed at
`target/windui-smoke/run-32612-1793249513866/manifest.json`; representative Process
List and Action Log captures were checked. Build hashes and logs are recorded in
the evidence directory's `verification.json`.

An older running instance with an already orphaned capture must be restarted
to use the repair. This guard prevents the race; it does not reset upstream's
private capture state in an existing process. These interaction checks make no
new CPU, power or accessibility claim.

### Right-panel visibility parity, 2026-10-29

The original Iced shell opened right panels by default and placed its collapse
toggle beneath the panel content. The Wind UI redesign had changed that to a
closed default, a header action and a top close button. The current shell restores
the default-open panel, bottom native clickable hide/show row and collapsed icon
rail. This applies to every existing status, settings and preset panel. The same
visibility flag now covers wide and compact layouts; hiding a panel no longer
reveals a different preference after resizing or collapsing left navigation.
No persisted setting, runtime-policy or upstream framework change is involved.
Isolated validation evidence is kept under ignored
`benchmark-results/right-panel-parity-20261029/`.

Validation passed: 908 tests (19 existing ignored), strict Clippy with and
without `render-smoke`, formatting, whitespace, architecture and legacy-name
checks. The 128-case render matrix completed without failures in
`target/windui-smoke/run-31308-1793246604324/manifest.json`. The added cases cover
collapsed settings/status rails at wide and minimum widths, plus an expanded
panel beside collapsed left navigation at 900 pixels. Representative native
captures were visually checked. Native pointer checks exercise both bottom
toggle directions and the retained header action; three window destroy/reopen
cycles preserve the collapsed preference. Layout tests retain input identity,
focus and draft text across resizing in all left/right visibility combinations;
the native toggle also passes pointer and keyboard activation tests. Build
hashes and the final production build result are recorded in the evidence
directory's `verification.json`.

### Hover help restoration, 2026-10-29

The port lost the original setting-title helper's locale-key-to-help binding.
Adaptive Engine also attached help to the setting row, where Wind UI's
automatic full-title tooltip on the child label took precedence. Suppressing
that duplicate title did not continue searching the parent, so hovering the
visible title or info marker displayed no explanation.

Shared native setting/title helpers now bind help directly to the visible
label, preserving the theme's font, row geometry, control width and label
truncation. Existing help is restored for Settings, Background Efficiency,
Priority Control detection, CPU Limiter and CPU allocation rule headings,
Win32 Priority Separation, and Memory Trim category headers. The native
collapsible header can inherit its category help without the form-label
precedence problem. No upstream, renderer, dependency, locale, settings or
runtime-policy changes are required.

Native hover capture and regression evidence is kept under ignored
`benchmark-results/tooltip-repair-20261029/`. Fixtures disable automation and
portable settings IO.

Validation passed: 907 tests (19 existing ignored), strict Clippy with and
without `render-smoke`, formatting, whitespace, architecture and legacy-name
checks, and the production release build. The 123-case render matrix completed
without failures in `target/windui-smoke/run-20852-1793245226588/manifest.json`.
Native hover screenshots visibly verify Adaptive Engine preset help,
cross-session process control, Memory Trim thresholds and Process Priority
foreground detection. Regression tests also cover English/Chinese help,
truncated labels and expanded/collapsed category headers. The resident lifecycle
driver passed three destroy/reopen cycles and its cancellation/shutdown checks.
Desktop automation was unavailable; these hover screenshots use Wind UI's own
pointer-move capture path. They do not establish screen-reader support or
comparative performance. `verification.json` in the evidence directory records
the build hashes and check results.

### WindInput and Process List refinement, 2026-10-29

The user's side-by-side screenshot showed that native widget usage, rather
than framework capability, caused several visual problems. Process names used
default primary buttons, so every row became a filled accent bar; command
grids stretched secondary actions; side-panel selectors crowded their labels.

Process names now use native clickable containers with plain text, icons and
native keyboard/focus behavior. Commands use compact native flows, neutral
outlined secondary styles and an outlined destructive command. Selection
actions follow the exact visible process identities through the existing UI
metadata channel; no page rebuild or automation-policy change is required.
Column labels sit above width/auto-size controls. Shared theme roles use
blue-gray layers, fine group borders, regular setting labels and one 22-pixel
page heading. Full route context stays in the heading tooltip. The configured
system/custom accent, native frame, drafts, confirmations and lifecycle remain.

Wide windows reserve space for the optional side panel; compact windows open
the same panel over the right edge without rebuilding or squeezing the editor.
A native bounds/focus regression covers resizing and closing this panel.
The Process List retains its bounded native virtual table. No custom scroll
widget, renderer patch, dependency or background timer was added.

Generated evidence stays under ignored
`benchmark-results/windinput-refinement-20261029/` and `target/windui-smoke/`.
Render fixtures disable automation and settings IO. These checks do not
establish a new CPU-cycle, RAM or power result.

Validation passed: 903 tests, zero failures and 19 existing ignored Windows
integration tests; strict normal/render-smoke Clippy, formatting, whitespace,
legacy-name and architecture gates. The 123-case native render matrix is
`target/windui-smoke/run-25900-1793242959967/manifest.json`. Representative
Process List light/dark, Traditional Chinese minimum-size panels, Adaptive
Engine, Home and Language and Appearance frames were visually reviewed.
The resident lifecycle harness passed pointer navigation, modal blocking,
three destroy/reopen cycles, retained drafts and coordinated cleanup.
`verification.json` in the ignored evidence directory records executable and
source hashes and the production build result. The three maintained
screenshots were refreshed from this matrix. PNG capture success alone does
not establish full accessibility, hardware integration or power consumption.

### Original Iced visual baseline, 2026-10-29

The user accepted restoring the original Iced presentation after rejecting the
flattened redesign. Shared native theme roles restore cool layered surfaces,
14-pixel body text, semibold setting labels, 28-pixel headings, 12-pixel group
padding and compact rows. Forms center within 1040 pixels. Home restores its
three-plus-two dashboard and Main sections shortcuts; About uses two grouped
surfaces with native secondary buttons. Navigation uses accent selection and
starts with search. Header actions align beside profiles at wide widths and
stack beneath them in narrow editors, which show only the current page title.

Adaptive Engine keeps native controls and contextual tuning help, with safety
warnings and validation inline. Numeric and selection controls share narrow
row space with their labels; native horizontal scrolling keeps all tuning tabs
reachable when the optional Status panel is open at minimum window width.
Feature info remains in the main scroll, aligned with the form. Status/Presets
retain their fixed tabs and separate panel scroll. Drafts, typed intents,
portable settings, native window lifecycle and automation policy are preserved.

Locked tests passed with 901 passed, 0 failed and 19 existing integration tests
ignored. Strict normal and render-smoke Clippy, formatting, whitespace,
legacy-name and architecture ownership gates passed; the graph was updated.
Native lifecycle checks passed, including pointer navigation, modal blocking,
help/status controls, draft retention and three destroy/reopen cycles. All 121
render captures passed in
`target/windui-smoke/run-42100-1793239806343/manifest.json`. Representative wide,
minimum-width, light/dark and translated frames were visually reviewed; native
resize regressions cover the narrow toolbar and labelled, aligned controls.
The production build passed `cargo build --release --locked` without fixture
features. Final executable, source and screenshot hashes are recorded in
`benchmark-results/iced-visual-baseline-20261029/verification.json`.

Evidence belongs under ignored
`benchmark-results/iced-visual-baseline-20261029/` and `target/windui-smoke/`.
Fixtures use deterministic sample data, disable automation/settings IO, and
render window content without Windows frame decorations. These presentation
checks do not establish a new performance or power result.

### Adaptive Engine layout correction, 2026-10-29

The user's expanded Adaptive Engine screenshot rejected the first quiet-layout
pass. It exposed detached descriptions, weak grouping, repeated policy labels
and nested scrolling that the earlier top-of-page render review missed.

Adaptive Engine now uses native described setting rows and grouped sections
across its tuning tabs. CPU policies retain independent switches and advanced
controls without repeating the policy title as a disclosure. Status/Presets
tabs sit above one panel scroller; the previous nested page removed 72 pixels
from a 272-pixel panel. Empty status reasons no longer reserve blank space, and
textual bottleneck states use a full-width line. Body text is 15 logical pixels,
descriptions 13, group headings 16 and page headings 26; navigation is 280 pixels
wide to accommodate its labels and optional counts.

Feature info joins the main page scroll. Wind UI's native unbounded scroll
measurement expands the existing editor container to its content height, so
the outer page is the sole active vertical scroll area. A native range/wheel
regression verifies this behavior; another verifies fixed Status/Presets tabs
with exactly one panel scroller. Opening help resets the saved page offset so
its text is visible. Native lifecycle checks cover help open/close and retained
drafts as well as the existing Status, modal and destroy/reopen checks.

The render matrix now includes three dedicated states matching the user's
report: help/status open at 1920x1056, the same at 900x620 in Traditional
Chinese, and a scrolled 1920x1056 form showing the independent CPU policies.
All 121 captures passed in
`target/windui-smoke/run-40864-1793237137055/manifest.json`. Normal locked tests
passed with 899 passed, 0 failed and 19 existing integration tests ignored.
Strict Clippy, formatting, whitespace, legacy-name and architecture ownership
checks passed; the graph was updated. Native lifecycle checks passed, including
the new Feature info open/close and retained-draft assertions. At 900x620,
long Traditional Chinese descriptions wrap and require more vertical scrolling.
The broader review caught clipped six-digit PIDs after the typography/sidebar
change. Native Process List auto-fit now reserves more relative column space
for identity values; full fixture IDs were rechecked at 1120x760 and 900x620.
The final production build passed `cargo build --release --locked` without
fixture features. Binary, source and screenshot hashes are recorded in
`benchmark-results/adaptive-layout-20261029/verification.json`.
Generated evidence stays under ignored
`benchmark-results/adaptive-layout-20261029/`. These presentation changes do
not establish a new power, memory or CPU-cycle result.

### Quiet Windows utility redesign, 2026-10-29

The user selected a compact, restrained Windows utility direction. This pass
replaces the persistent secondary navigation column with inline children,
initially started detail panels closed behind labelled header actions (superseded
by the right-panel parity repair above), and puts related
policy and detection controls together. Native descriptions clarify that a
detection switch and its policy value are independent. A single Advanced
disclosure holds secondary policy controls. Empty rules omit empty table chrome;
Home no longer repeats the navigation grid, and About uses inline links.
No dependencies or upstream framework code changed in this pass.

Normal locked tests passed: 897 passed, 0 failed, 19 ignored. Formatting,
strict Clippy, diff whitespace, legacy-name and architecture ownership checks
passed. A native layout regression verifies that profile and header actions
share a row at wide widths and wrap after resizing. The knowledge graph was
updated after the code changes.

The production build passed `cargo build --release --locked`, without fixture
features. The executable is `target/release/winderust.exe`; final binary,
source and screenshot hashes are recorded in the ignored
`benchmark-results/quiet-ui-20261029/verification.json`.

All 118 native render cases passed in
`target/windui-smoke/run-9428-1793235247888/manifest.json`: all 34 pages in
English/light and Traditional Chinese/dark, plus editor, dialog, profile and
scale variants. Representative frames were visually reviewed, including
compact navigation, populated Process List, CPU allocation, policy settings,
Home, About, Appearance and dialogs. A separate 125% Background Efficiency
capture matches the dimensions of the user's reference. This is visual
coverage, not an automated assertion that every pixel is correct.

The isolated native lifecycle fixture passed pointer navigation, modal
blocking/cancellation, opening and closing the Status panel, zero-window
delivery, three destroy/reopen cycles, draft retention and coordinated exit.
Fixtures disable automation and settings IO. Generated screenshots and logs
remain under ignored `benchmark-results/quiet-ui-20261029/` and
`target/windui-smoke/`; repository screenshots are refreshed separately.

These checks do not establish a new CPU-cycle, memory or power result.
The 19 ignored integration tests and the remaining manual Windows release
checks retain their existing scope; screenshots do not prove accessibility.

### WindInput settings reference refinement, 2026-10-28

The user's latest screenshot supersedes the earlier instrument-panel direction.
The reference was checked against `huanfeng/wind-setting` commit
[`8b9740c73528c50caa91b16b0442322794623f42`](https://github.com/huanfeng/wind-setting/tree/8b9740c73528c50caa91b16b0442322794623f42),
particularly its theme/form metrics and native setting helpers. Winderust now
uses Segoe UI regular body text, blue-gray surfaces, 12 px grouped cards with
thin borders, icon section headings and aligned native controls. Configured
system/custom accent and semantic status colors remain authoritative.

Primary navigation allows canonical English/Traditional Chinese names and
optional counts. At 1280 logical px and above, expanded multi-page categories
use a separate child column; smaller windows retain inline children. Native
visibility changes preserve editor identity, focus and drafts on resize.
Priority Control, Background Efficiency and CPU Limiter keep primary per-tier
settings visible in grouped cards. Advanced details, all rule editors and
validation remain available. About uses native clickable link cards and keeps
its update controls and destinations.

Validation records and representative 125%/200% scale captures live under
ignored `benchmark-results/windinput-reference-20261028/`. The native lifecycle
fixture must use a normal visible window; forcing Hidden at process startup
can prevent its initial native layout. This presentation pass does not establish
a CPU, memory or power advantage over the measured earlier builds.

The final revision passed strict all-target Clippy, formatting/whitespace,
legacy-name and architecture checks, 897 tests (19 ignored), all 118 render
cases and the visible native lifecycle fixture. `graphify update .` completed.
The default release build excludes the render harness. Its binary metadata,
source hashes and render manifest are recorded in the local verification JSON;
Home, About and Language and Appearance previews were refreshed from that
verified render binary.

### Winderust redesign and artifact cleanup, 2026-10-28

The latest design replaces the Iced-style target with Winderust's own
instrument-panel composition: Bahnschrift, graphite/mineral surfaces, regular
body text, a compact sidebar identity, small ancestor links above the current
page title, unified Home charts, flat shortcuts and divider-separated status
readings. The shared native theme applies throughout the presentation.
Configured accent, native controls, routes, draft ordering and window lifecycle
remain in place. This refinement changes no runtime automation or dependency.

Visual review corrected palette-swatch text contrast and reserved sidebar
scrollbar space at 200% scaling. Accent swatches use native filled buttons so
their explicit foreground is honored; normal links use readable theme text
instead of assuming every user-selected accent contrasts with the background.

Generated evidence was moved intact to the ignored
`benchmark-results/archive-20261028/` archive. All 1,661 moved files
(187.19 MiB) were verified by size and SHA-256; its `archive-manifest.json`
records original and current locations. This removed 1,532 generated files
from the pending Git list. Maintained reports, reusable scripts, scenario
inputs, application source and tracked historical evidence remain in source
control. Evidence links now identify their local archive requirement, and
narrow ignore rules prevent new result folders and bundles from returning.

This redesign's validation logs and binary metadata live under ignored
`benchmark-results/ui-redesign-20261028/`; the verification record identifies
the final render matrix. The production metrics policy is unchanged, and this
visual work adds no new CPU, memory or power measurements.

Final formatting, strict all-target Clippy with unsafe-operation checks,
architecture ownership, legacy-name and whitespace checks passed. Locked Rust
tests passed: 895 passed, 0 failed, 19 ignored. All 118 native render cases and
the resident lifecycle check passed after the contrast and scrollbar corrections.
The final matrix is `target/windui-smoke/run-23912-1793177356204/manifest.json`;
representative Home, settings, rule, translated 2x and modal frames were visually
reviewed. Home, About and Language and Appearance previews were refreshed in
[`screenshots/`](../screenshots/). These checks establish native layout and the
covered interactions; the ignored integration tests and a full accessibility
audit remain outside this visual refinement.
The normal locked optimized release build also passed; its 12,505,600-byte
executable excludes the optional render harness. Binary hashes are recorded in
the local verification record.

### Original Iced visual style, 2026-10-28

The earlier refinement followed the original Iced reference at `3d329a9`:
neutral surfaces, compact typography and spacing, navigation markers and
counts, the three-chart wide Home layout, quieter status panels, an inline
application picker and aligned process-rule columns. These use built-in Wind UI
controls, including native tables with interactive cells. No dependency or
upstream framework source was changed for this refinement.

The native Windows frame, Wind UI control rendering and disclosure headers,
full-width Save/Discard footer and grouped appearance preferences remain
intentional differences. This is style and layout parity, not pixel-identical
control painting. Current product routes and runtime safety contracts remain
the behavior reference.

Validation passed after the final table-heading sizing correction:

- Locked tests: 895 passed, 0 failed, 19 ignored.
- Formatting, strict all-target Clippy with unsafe-operation checks, whitespace,
  architecture ownership and legacy-name checks passed.
- The normal optimized release build passed without the render harness;
  `target/release/winderust.exe` is 12,470,784 bytes. The AST knowledge graph
  was updated after the final Rust change.
- The 118-case native render matrix passed, covering all 34 pages in
  English/light and Traditional Chinese/dark, with selected narrow, large,
  1.5x/2x and dialog cases. Representative final frames were visually reviewed.
- Native lifecycle checks passed production-window pointer navigation, modal
  blocking/cancellation, three destroy/reopen cycles, retained drafts,
  zero-window channel delivery, wake coalescing and coordinated shutdown.
- New regressions exercise native rule-table pointer targets and scrolling,
  single-row visibility and Home breakpoint changes that retain focus during
  ordinary resizing.

The final render manifest is
`target/windui-smoke/run-28096-1793171788122/manifest.json`; command logs and
binary metadata are under `target/ui-style-parity-20261028/`. Refreshed Home,
About and Language and Appearance previews are in [`screenshots/`](../screenshots/).
The fixtures disable automation and portable settings IO. The ignored Windows
integration tests, manual release checks and a new CPU/power comparison were
not run for this visual change.

### Navigation pointer correction, 2026-10-19

The initial visual captures did not exercise navigation clicks. A user-reported
failure exposed an always-present, full-window `host_signal` above the shell:
its native `DynList` host remained hit-opaque even when the empty child was
hidden. Pointer events therefore never reached the navigation, page or footer.

`WinderustApp::overlay` now returns `Option<Element>`. The overlay host binds
its own native visibility to whether that factory actually produced a dialog.
The visibility signal lives outside the dynamic child scope, and hidden hosts
continue processing revisions so later dialogs still open normally. No custom
navigation widgets or changes to the upstream dependency were needed.

Two native pointer regressions cover icon, label, badge and row-padding clicks,
keyboard activation, page/footer actions, modal blocking and three close/reopen
cycles. Both tests fail when the host visibility gate is removed and pass with
it restored. The lifecycle fixture additionally dispatches pointer Down/Up in
the actual production window: About navigation, footer Exit, blocked Home
navigation under confirmation, Cancel, then successful Home navigation. Page
and dialog state are verified after the normal application channel delivers
each intent. Draft retention and the existing resident lifecycle checks pass.

Normal tests: 861 passed, 0 failed, 18 ignored. Optional harness tests: 867
passed, 0 failed, 18 ignored. Formatting, strict Clippy in both configurations,
architecture ownership, legacy-name checks and the AST knowledge-graph update
passed. Native pointer/lifecycle evidence is in
`target/windui-smoke/navigation-fix/verification.json` and `lifecycle.log`;
the dev-profile harness SHA-256 is
`242ccce7c942032366e7835a31a40237d9f65a353a9c07fd21b4966352027917`.

The normal release build passed, with optional harness markers absent from the
12,174,848-byte `target/release/winderust.exe`. Its SHA-256 is
`b5714e0e695cb0ce52be7fc9472ff7534cb1732597f83fa12882cbd8164c9602`.

The previous 118-frame matrix remains visual evidence. This interaction fix
does not claim another full render matrix or performance measurement. The
18 ignored Windows integration tests and manual release checks remain open.

### WindInput refinement, 2026-10-19

| Check | Result |
| --- | --- |
| Formatting, whitespace, architecture ownership, legacy-name scan, local documentation links | Passed |
| Strict Clippy, normal and optional harness configurations | Passed with warnings denied, including unsafe-operation checks |
| Normal production release | `scripts/build_release.cmd` passed; 12,173,824-byte executable, optional fixture code excluded |
| Normal locked tests | 859 passed, 0 failed, 18 ignored |
| Feature-enabled locked tests | 865 passed, 0 failed, 18 ignored |
| Native render matrix | 118 captures passed; all contact sheets visually reviewed |
| Final targeted renders | Six passed: About, Appearance, Adaptive Engine, Process List, Home and Adaptive preset modal |
| Native lifecycle | Passed zero-window delivery, three destroy/reopen cycles, retained drafts, cancellation, coalesced wakes and coordinated shutdown |
| Knowledge graph | Updated after the final Rust correction |

The final matrix is `target/windui-smoke/run-19796-1792396660955/manifest.json`.
The 34-page English/light and Traditional Chinese/dark coverage includes narrow,
large and selected 1.5x/2x cases. Review found a clipped Process List sort-direction
button: its dropdown needed a main-axis weight. The complete matrix and lifecycle
were rerun after that correction, and the final narrow/2x Process List frames show
the whole button. Native resize regressions cover input identity, text and focus.

Final captures use the dev profile with optimized dependencies; they establish
native layout and fixture behavior, not optimized performance. The preserved
harness is `target/windui-smoke/winderust-reference-validation.exe`, SHA-256
`cc1a2e865ce61e934eff43201de2e829b8713be495b705d8fa1846210a3a35a2`. The six targeted frames, command logs and result metadata are
under `target/windui-smoke/reference-final/`. Refreshed previews are tracked in
[`screenshots/`](../screenshots/); fixtures disable automation and settings IO.
The earlier optimized matrix and baseline measurements remain separate evidence.
The refinement executable before the navigation correction had SHA-256
`815c8aed2fe88891ed203e232a40e4389a8e26f767b329b0a3bb3c43241a9fec`.

The 18 ignored Windows integration tests and the manual release checks described
below remain outstanding. No background performance rerun or matched Iced
comparison is claimed for this visual refinement.

### Port baseline before the WindInput refinement

The following records the complete port before the visual refinement. Its
binary hashes identify that baseline; background measurements below continue
to refer to that preserved harness, not to the refined presentation build.

The local implementation checks on 2026-10-19 passed against that baseline:

| Check | Result |
| --- | --- |
| Formatting, diff whitespace, architecture ownership, legacy-name scan | Passed |
| Strict Clippy, normal and `render-smoke,runtime-benchmark` configurations | Passed with warnings denied, including unsafe-operation checks |
| Normal locked tests | 857 passed, 0 failed, 18 ignored |
| Feature-enabled locked tests | 863 passed, 0 failed, 18 ignored |
| Normal production release build | `scripts/build_release.cmd` passed; 12,091,392-byte executable |
| Knowledge graph | Updated successfully after the final Rust changes |
| Native render matrix | 118 release captures passed; all contact sheets visually reviewed |
| Final targeted renders | Process List and process details passed in English and Traditional Chinese; details actions remain visible at 760- and 620-pixel window heights |
| Native lifecycle | Passed zero-window delivery, three destroy/reopen cycles, retained drafts, cancellation, coalesced wakes, and coordinated cleanup/exit |

The render matrix includes all 34 pages in English/light and Traditional
Chinese/dark, plus profile variants, overlays, feature editors, and selected
1.5x/2x scale cases. The final four targeted frames reran Process List and
details after the metadata and footer corrections. Focused native regressions
also verify that asynchronously loaded process/plan choices and process-table
metadata preserve input text and focus. Typed worker-start failure and shutdown
failure tests cover pending-state recovery and restoration ordering.

Local evidence is under `target/windui-smoke/`: the 118-frame matrix is
`run-29648-1792390133117`, and final targeted images plus lifecycle status are
recorded in `final-targeted/verification.json`. The preserved optimized harness
is `winderust-native-validation.exe`, SHA-256
`4fe7e93de09337c2e67d2782be1860b0ceb5c23746721bc37dec6b01e0c20a81`.
These generated files are local validation artifacts, excluded from releases.
The baseline production executable was `target/release/winderust.exe`,
SHA-256 `0e8d1ce1bea46a89fd111f084cb1a316915ffd08e6cf382f2531f31bd2b00766`;
it excludes the optional fixture features.

The 18 existing ignored tests require explicit Windows integration conditions:
live process mutations or freeze/restore, a disposable machine-wide power plan,
an explicit GPU target, hardware counters, or isolated observation profiling.
They were not silently counted as passing. Actual tray interaction/Explorer
restart, mixed-monitor DPI, IME, suspend/resume, elevation/session-end behavior,
and final package review remain Windows release checks. Screenshot and
unit-test coverage do not establish those results.

### Measured background footprint

The subsequent [native idle cleanup](../benchmark/ui-native-idle-cleanup-20261022.md)
and [thread diagnostic](../benchmark/ui-idle-thread-diagnosis-20261022.md) record
the later application changes, matched comparisons and remaining attribution
limits. The original short series below is historical and excludes the helper.

Ten corrected release-fixture trials passed on 2026-10-19: two per state,
six-second warmup, fifteen-second sample, 0.5-second memory/state sampling,
300 samples total. Every sample matched the requested native state and every
fixture exited with status zero through coordinated cleanup. Builds and
captures were stopped during measurement. Automation and update checks were
disabled; the production tray probe and five-second health timer remained on.

The host reported Windows build 26200 (`Windows-10-10.0.26200-SP0` from
Python), `Intel64 Family 6 Model 186 Stepping 2, GenuineIntel`, and 12 logical
processors. The measured executable is the preserved harness with the SHA-256
recorded above. Raw counters, samples, conditions, and per-trial logs are in
`target/windui-smoke/final-background-20261019/measurements.json`.

| Native window state | CPU, % of one core | Private memory, MiB | Working set, MiB |
| --- | ---: | ---: | ---: |
| Started without a UI window | 0.00 | 7.15-7.18 | 21.77-21.80 |
| Home visible | 2.97-3.59 | 18.69-18.75 | 38.00-38.01 |
| Minimized | 0.20-0.51 | 18.77-18.93 | 37.88-37.91 |
| Hidden, HWND retained | 0.20-0.21 | 18.99-19.00 | 37.79-38.03 |
| Closed into the background, HWND destroyed | 0.00-0.10 | 11.94-12.37 | 31.09-31.20 |

CPU columns show the range of per-trial CPU-time rates; memory columns show
the range of per-trial medians. Zero means no CPU time was recorded by the
process counter during that short sample, not zero activity or energy use.
These values exclude the recovery helper. The fresh no-window fixture is a
diagnostic state: production startup first displays an accessible window, so
the closed-window result better represents its normal resident footprint
after opening the UI. Retained UI caches account for a different footprint
from starting without a window; these short trials do not test long-term growth.

Run native measurements in a Windows session with normal Explorer access.
A sandboxed trial correctly fell back to a visible window when the tray probe
could not confirm availability; its state assertion failed and its numbers
were excluded. An earlier runner revision could minimize an HWND before
Wind UI's final startup restore; that invalid run is also excluded. The
recorded ten-trial series uses the corrected settling procedure outside the
sandbox, without bypassing the tray guard. This original short series provides no matched full-application Iced,
active-automation or power-consumption result; subsequent comparisons are linked
above.
