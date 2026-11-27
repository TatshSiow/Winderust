# Runtime Contracts

Read the relevant section before changing feature policy, ownership, restoration, or backend boundaries. API evidence lives in [Windows API reference](windows-api-reference.md); UI presentation lives in [Design](design.md).

## Application boundaries

- Automation worker lifetime uses the complete AC/Battery configuration; policy application still uses only the active profile. An idle profile waits for events when the other profile requires automation.
- Scoped path matching for Resource Thresholds, its Preview matches tab, and Sleep
  Prevention selects candidates from one Toolhelp process snapshot using the
  rule selector or prior process ID before reading process metadata. Each row's
  first metadata attempt stays with that catalog. A later full catalog request
  fills the remaining rows from the same snapshot. If the full request needs
  paths, newly observed rows share one query handle for metadata, image path,
  and service-account classification. Previously observed rows retain their
  captured generation. Scoped callers reuse a complete path-enriched catalog
  when available. Exact path, process identity,
  and protection checks still apply. The next pass takes a fresh snapshot.
- Action Log entries and summaries remain in memory across automation worker
  restarts. The worker publishes its cleanup entries as it returns the log.
  Clear resets both without reusing sequence numbers.
- Settings contain Plugged In policies and at most one On Battery profile. Nested
  On Battery profiles fail load, import, save, and export before normalization or
  writing can discard their settings.
- Update checks support Stable and Pre-release channels. Automatic checks are optional; manual checks remain available on About.
- `SettingsEditor` is the application settings boundary. It privately owns draft/revision state,
  save/cancel/import/export/auto-patch merge, runtime publication, and persisted startup intent.
  Win32 Priority Separation and Advanced Power Plan Tuning use typed application services; they
  are explicit persistent operations, not temporary managed state or crash-recovery owners.
- Runtime feature status is one semantically stable `Arc<RuntimeFeatureStatus>` segment. Process
  catalog/list, dashboard history, update state, and shell navigation remain plain UI read models.
  Process queries, icons, monitors, Iced subscriptions, tray, dialogs, and focus remain at the
  `WinderustApp` composition boundary.
- Closing to the tray destroys only the Iced window and renderer. The daemon, settings drafts,
  Action Log, tray host, event subscriptions, and runtime owners remain alive. Tray restoration
  and duplicate launches recreate the window; explicit Exit still completes restoration before
  stopping the daemon. A failed tray registration keeps or reopens the GUI for recovery.
- Show Winderust is an opt-in, system-wide keyboard shortcut shared by both
  power profiles. It uses the existing tray host and restore path independently
  of the master automation switch. Save and Import reserve a changed shortcut
  before persisting settings and release the reservation on failure. A
  conflicting change leaves the previous settings and binding intact. A saved
  shortcut uses normal tray recovery when its host is unavailable. A transient
  failure to reach a live host can retry when Explorer recreates the taskbar.
  A Windows chord conflict is reported without polling. The tray host releases
  its registrations on exit.

## Keep Running

- Keep Running is a global, opt-in Watchdog feature. Each rule names one
  executable and optional literal arguments. Keep Running launches from the
  resolved executable's folder. Arguments are stored as plain text in the
  portable settings file. Enabling a rule with no
  existing eligible app never launches it. First observe an ordinary app with the
  current shell's verified user/logon/session token facts, then retain its exact
  handles. Fresh launches bind to the verified shell desktop; matching token facts
  do not prove the original app's desktop or restore it.
  Cross-session process-control permission does not widen launch eligibility.
- Missing or incomplete enumeration is not exit evidence. Only a signaled retained
  process handle retires a member. After the last confirmed exit, retry after 1,
  5, then 30 seconds, with at most three attempts until 60 seconds of verified
  overlapping-cohort runtime or explicit Resume. Unknown intervals earn no stable
  runtime credit. Disjoint replacement cohorts do not inherit it.
  A failed launch attempt is logged once and remains visible during its retry
  wait. An observation error takes precedence until a complete scan recovers.
  Keep Running records a successful action only after Windows confirms the relaunch.
  A failed post-launch verification remains in Action Log after a healthy member scan.
  Rule rows show Unknown until the current launch configuration has a matching
  worker status. Editing a rule resets its retry state and preserves the state
  of unchanged rules. Rapid edits and reverts still reset affected rules.
  Notify edits preserve retry state and status. An opted-in rule may request one
  Windows notification when retries are exhausted. The pending alert is consumed
  once and checked against the saved rule, pending resets, and current Windows
  context before delivery. Manual Resume starts a new retry episode. Alert
  outcomes go to Action Log.
- This is a fresh launch, not crash diagnosis, saved-session recovery, or the
  Restart in Resource Thresholds. Winderust never reads another app's arguments or
  copies its environment. Unsupported/elevated/service/restricted app contexts
  pause the rule. File-identity aliases and overlapping live membership pause
  conflicting rules instead of selecting arguments by rule order.
  A failed file lookup preserves the last verified alias until a successful
  lookup or a rule change supersedes it.
- One bounded worker owns all preparation, resume/acceptance, and disposal calls.
  The five-second preparation deadline bounds acceptance of results, not Windows
  call duration. A timed-out call or unconfirmed cleanup occupies the global slot;
  disabling, editing, removing, or re-adding a rule cannot discard its ownership.
  A disabled rule keeps its current diagnostic while it still owns pending
  native work. The Action Log retains recorded failures after the diagnostic clears.
  Shutdown cancels provisional work without joining a blocked Windows call.
  Membership and filesystem queries remain synchronous on RuntimeCore; the
  preparation deadline is not a bound on the entire runtime cycle. When full
  Windows event coverage is unavailable, discovery pauses while pending launch
  and cleanup results continue to drain.
- Immediately before launch commitment, re-enumerate membership outside the
  shared intake mutex, then reserve once under that mutex. Pending Stop commands,
  settings edits, suspend/session changes, shutdown, or watcher loss block the
  reservation. Watcher loss pauses pending retries even if coverage returns
  before the worker processes the loss. Native calls run after unlocking. Later
  intents pause subsequent retries; they cannot roll back a committed launch.
  Manual Stop/Stop Process Tree and eligible Resource Thresholds Terminate or
  Restart attempts pause affected rules before acting, even when the attempt
  fails. Confirmation of Stop Process Tree submits the captured roots for
  pausing before the fresh tree scan. Verified descendants are paused before
  termination is attempted.
- Closing to tray retains this owner. Suspend/session changes require Resume;
  power-source handoff does not reset global rules. Disable and Exit never stop
  an accepted replacement. Fresh launches are not crash-helper recovery records.
- Atomic Job List creation protects suspended/provisional roots while the private
  job's kill-on-close flag remains armed. Clearing that flag is irreversible native
  acceptance, before result publication. Explicit breakaway descendants may survive
  provisional cleanup; this feature does not promise descendant rollback. Nested
  jobs, console apps, custom token changes, and shell replacement remain compatibility
  constraints, not reasons to bypass access checks or enable privileges.

## Instance Limits

- Instance Limits is disabled by default and shares one configuration across power
  sources. Rules count process objects at the selected full Win32 image path for
  the current user SID and Windows session. Queryable elevated or restricted apps
  are included; logon authentication IDs do not partition the count. The feature
  neither prevents launches nor stops, suspends, relaunches, or places processes.
- Paths compare by Windows ordinal-ignore-case after slash normalization. Different
  hardlink names and directories remain separate targets. Normal drive-rooted and
  UNC syntax is accepted; relative/device paths, dot components, ambiguous trailing
  dots/spaces, invalid characters and duplicate rules are rejected. Each pass verifies
  the selected file's final path spelling; missing, inaccessible or differently
  resolved short/reparse aliases produce Unknown. Case-distinct paths intentionally
  compare equivalent even on a case-sensitive filesystem; those directories and
  native UNC behavior have no controlled compatibility evidence yet.
- One fresh Toolhelp catalog serves all rules. Only a complete usable executable
  basename may exclude a different name; positive counts require a retained live
  process handle, creation time, full image path and SID/session checks. Query-only
  access may exclude a known different path/scope, but cannot prove liveness.
  The collector returns exact PID/creation/path members with retained observation
  handles, rechecking liveness before returning them. Instance Limits counts those
  observations and releases the handles after collecting the sample.
  Retained observation is not mutation authority: placement still needs fresh scope,
  safety, identity and settings preflight. A discovered catalog/image-name
  contradiction invalidates the pass. Basename agreement was measured on ordinary
  and elevated fixtures, not established for every rename or filesystem behavior.
- Counts are sampled and non-atomic: short-lived processes may be missed. Incomplete
  counts provide a verified lower bound to the incident policy. A missing event
  watcher pauses sampling until coverage returns. Native queries run outside
  the shared intake mutex; one-second visible/tray cadence is not a Windows-call deadline.
- Excess and recovery each need two fresh qualifying samples at least one second
  apart. A lower bound above the maximum proves excess; only complete within-limit
  counts prove recovery. Uncertainty and gaps longer than three seconds restart
  qualification while preserving an unresolved incident. Each incident allows one
  notification attempt, including failures. Acceptance confirms delivery to Windows.
- Rule edits reset only that rule, while feature/master disable clears all state.
  Notify-only edits and power-source changes preserve incidents. Delivery requires
  both the feature-wide switch and the rule's Notify setting. Enabling Notify
  requires fresh excess evidence before an unattempted notification, including
  when the setting is turned off and on between worker snapshots. Notification
  outcomes are recorded in the Action Log. Pending reset intents survive rapid
  edits and toggles between worker snapshots. Suspend/session
  events discard continuity and stale results while preserving attempted incidents.
  Results are accepted under the intake mutex before notification delivery outside
  it; a later intent cannot retract an already reserved notification attempt.
- Action Log records excess as monitoring-only Skipped, complete recovery, changed
  observation failures and notification results. It contains no raw SID or logon keys.

## Restoration and process safety

- Never target Winderust itself or protected/system processes. Cross-session targeting
  still requires Windows access, built-in exclusions, and exact identity validation.
  Executable-path comparisons preserve whitespace in native filenames. Treat
  access denied as skipped unless it exposes an implementation bug.
- App Suspension, CPU Limiter, and Memory Trim compare ordinary slash separators
  as interchangeable across saved power profiles. Verbatim Windows paths retain
  their exact saved spelling for this comparison.
- Keep High/Realtime priority out of automatic paths; broad App Suspension stays
  narrow and opt-in. Restore owned state on disable, rule mismatch, observed exit,
  or shutdown without bypassing identity and external-state checks.
- Before reversible process/thread, suspension, or automatic power-plan mutations,
  synchronously journal the original and expected state to the crash helper and wait
  for acknowledgment. If the helper is unavailable, block the mutation.
- Clean shutdown stops new work before reversing applied changes. `RuntimeCore`
  and `PowerPlanController` own automatic restoration order; `RuntimeHandle`
  restores Winderust self-power. Manual reversible Process List actions remain
  owned by `RuntimeCore`.

- Thread Priority, I/O Priority, GPU Priority, Memory Priority, and Dynamic
  Priority Boost retain exact identities for failed automatic releases and retry
  once per second independently of feature enablement. Repeated release requests
  preserve the earliest pending deadline. Cancel cleanup only when that exact
  target accepts replacement ownership or completes restoration/relinquishment;
  failed or uncertain replacements retain unresolved cleanup. Memory Priority
  retries reconcile surviving claims before restoring a baseline.
- Failed process identity reads preserve restoration ownership unless a query on
  the same handle positively confirms exit; verified identity mismatch also
  relinquishes it. An unavailable termination query retains the original error.
- Retained-handle operation errors preserve their native failure code. In
  particular, `ERROR_INVALID_PARAMETER` from a setter is not process-exit
  evidence and must not discard restoration state.
- The live recovery helper PID is protected at the shared process-control acquisition boundary, including manual actions, priority changes, CPU allocation, suspension, termination, and trimming. The PID is published when the helper starts and read without the recovery-journal mutex; acquiring protection must not deadlock a caller holding a recovery intent. Keep this protection through shutdown, including unconfirmed helper completion. Restoration and helper-owned recovery remain independent of new-mutation eligibility.
- At watchdog EOF, unresolved intents remain separate from committed transitions. Recovery unwinds an expected value, or skips an unapplied pending edge only when the current value equals its original value. Committed mismatches and unrelated external values stop the chain; never compact pending intents into committed baselines.
- A recovery retry reopens only journal keys whose restore failed. Completed
  restores and keys relinquished to external changes remain resolved while
  other keys or Adaptive plan cleanup retry.
- The watchdog owns the session-local `Local\Winderust.RecoveryHandoff` mutex before its first acknowledgment and through journal replay and Adaptive plan cleanup. A new instance waits up to five seconds before startup cleanup, then releases the mutex before spawning its own watchdog. If the wait fails, Winderust reports the error and exits before starting automation.

- Watchdog pipe I/O runs on one dedicated transport thread. Recovery callers wait at most two seconds for a reply; uncertain delivery/acknowledgment disables further commands on that connection, while an ordinary explicit rejection preserves protocol synchronization. Generic Commit failures also block the connection: they must not permit a later mutation to overtake an unresolved transition. Power-plan Commit retries retain the exact intent ID and require acknowledgment before reuse. Keep the connection and helper journal alive until runtime shutdown so a late reply cannot trigger premature recovery or acknowledge another command. Shutdown waits at most five seconds and reports unconfirmed recovery without killing the helper. A stalled I/O thread is not joined; parent process exit closes its remaining pipe handles. This bounds caller/shutdown waits, not recovery completion by a hung helper.

- Process-property controllers retain a verified target and its restoration
  baseline when Commit acknowledgment fails. Rolling back to an intermediate
  value could hide the original baseline behind an already compacted journal
  entry. Uncertain compensation keeps its acknowledged Begin pending and blocks
  further journal commands until helper recovery; it must not become a committed
  edge while its actual outcome is unknown.

- Quiet Memory Priority retries record verified baseline restorations in the
  initiating feature's Action Log when exactly one owner requested reconciliation
  for that process. Shared release attempts, active policy handoffs, departed
  processes and external overrides do not create restoration entries.
- A delayed failure that belongs only to static Memory Priority updates that
  feature's current status, preserving other diagnostics. Shared and Adaptive
  Engine release errors retain their existing worker error reporting.
- Failed Process Priority and Efficiency policy releases remain queued independently of active claims. The runtime retries reconciliation once per second while pending, including with feature/master toggles disabled. Each retry resolves current claims and uses existing identity and restoration checks; it must not blindly restore over a new owner.

- Runtime restoration is a product safety barrier: every reversible runtime
  change owned by Winderust must capture its pre-Winderust value and restore it
  in reverse application order. If the original state cannot be captured,
  Winderust must not make that reversible change.
- The barrier covers automation managers, Process List quick actions, and
  automatic power-plan switches. Clean shutdown restores through feature
  ownership; crash or forced-termination recovery is handed to Winderust's
  external watchdog before each mutation. Manual and automatic process
  termination, memory trimming, Windows shutdown, and power loss remain outside
  this guarantee.
- Cross-session process control is owned by Winderust Behaviour and defaults on. Disabling it restores same-session-only acquisition/targeting; it never blocks restoration of exact state Winderust already owns. Windows access checks and existing protected-process safeguards always remain active for new mutations.
- `src/control/process.rs` owns typed process targets, stable identity keys, and safety validation;
  `src/platform/windows/process.rs` alone translates operation-specific minimal access into
  `OpenProcess` and preserves App Suspension's synchronize-first fallback. Platform acquisition
  does not import feature, foreground, rule, UI, or controller policy. This is the mutation/command
  acquisition boundary: feature-specific CPU-time and age observations may use query-only handles,
  but they never authorize a mutation; controllers always reopen and revalidate selected targets.
- Memory Trim and Stop Process / Stop Process Tree are typed, result-bearing runtime commands.
  `src/control/memory_trim.rs` and `src/control/process_termination.rs` own command semantics;
  their raw calls live only in the matching `src/platform/windows/` adapters. Both revalidate exact
  process identity, protection, access, and the current cross-session setting on the runtime
  worker. They are irreversible and must never acquire a baseline, recovery entry, managed claim,
  or shutdown restore path. Stop Tree retains exact root identities across its confirmation prompt
  and rejects reused roots or known-stale numeric parent links before submitting the command.

## Resource Thresholds

- Resource sampling in `src/foreground/process_list.rs` reads CPU and verifies
  creation time for Resource Thresholds. Memory rules use the working set from
  the existing memory query. Process List uses both working set and private
  commit from that query. Group totals require a known value for every displayed
  process. Process List requests Efficiency Mode and queries priority class
  only when EcoQoS is enabled.

- `src/features/watchdog/resource_thresholds.rs` owns CPU and memory
  sampling, sustained thresholds, escalation, and rearming. Rules are global across
  power sources. Sustained time (1–3,600 seconds) applies to every rule,
  monitor, and severity. Notification settings
  apply to both monitors. Delivery requires both the feature-wide switch and the
  rule's Notify setting. Sampling runs once a second in visible and tray modes.
  Portable settings use the existing `watchdog` TOML key for these rules.
  The parser rejects rules with removed comparison, scope, duration override,
  or Private Commit options before a policy can run. Working Set units use
  `memory_unit = "percent"` or `memory_unit = "megabytes"`.
- Each rule selects an absolute `executable_path`, an exact `executable_name`,
  or an `executable_name_pattern`. Name and pattern matching use the full
  executable basename, case-insensitively. Patterns share the Adaptive Engine
  `*` and `?` matcher. Name rules use known absolute image paths from current
  observations. The scoped catalog includes enabled selector candidates and
  prior incident PIDs for exact identity verification. Each executable path
  has its own combined usage total and incident. A failed sample or replaced
  process cannot carry qualification into a later instance. Enabled rules evaluate
  independently if their selectors overlap. Terminate, Restart, and Trim memory
  require an Executable path rule. Trim memory is available for memory thresholds.
- CPU deltas are calculated per exact instance and summed against total processor
  capacity (capped at 100%). Working Set uses a percentage of physical RAM or
  decimal MB. Summed working sets may count shared pages more than once. Each response
  uses the eligible processes at the matching executable path.
- Each monitor qualifies at or above its threshold, requires a positive warning
  below critical, and rearms below 95% of the threshold. Percentage thresholds
  accept up to 100 and MB thresholds accept up to 1,000,000.
- Warning action and Critical action default to Alert in new rules. Alert
  records the threshold event in Action Log and can send a Windows notification.
  No action suppresses the response. The feature defaults off. Editing either
  notification switch preserves sustained samples, incident state, and actions.
  Windows delivery checks the saved notification settings. When both response levels are
  enabled, a critical response subsumes a simultaneous warning for the same
  resource. Each response fires once per incident. Missing samples break only
  the affected resource's sustained interval. Collection gaps, complete
  app-instance replacement, shared-setting or rule changes, and disable clear
  stale sustained timers. Membership changes keep the app incident while any
  exact instance survives. Within a matched group, unavailable protection or
  resource data breaks the affected sustained intervals. An unresolved path on
  an exact prior member makes its group incomplete. Already-fired responses stay latched
  through observation gaps when the next sample confirms an exact member
  survives. New or unsampled members require a fresh CPU baseline. Power-source
  changes leave Resource Thresholds rules and incident state unchanged.
- Before each automatic process action, the runtime checks the latest
  Resource Thresholds settings, automation enablement, and cross-session policy.
  A changed policy or pending shutdown, suspend/resume, or session change cancels
  that incident's response before Keep Running is paused or a restart cooldown starts.
- Trim memory works through the same working-set controller as Memory Trim.
  The action skips the focused executable, already-stopped instances, and
  the shared saved Memory Trim exclusions. The runtime spaces threshold trim attempts
  for the same executable path at least 60 seconds apart. Action Log and
  notifications report the result. Resource Thresholds owns qualification,
  independently of the Memory Trim toggle and its pressure and idle thresholds.
- Before Restart reserves a cooldown or pauses Keep Running, it checks each exact
  target against jobs App Suspension has frozen, including jobs awaiting a thaw
  retry. This covers descendants that inherited membership. A match records a
  skipped Restart. An unavailable identity or membership check records a
  failure. Action Log and notifications report the reason when their settings
  allow it. An independently configured Terminate response can still run.
- `RuntimeCore` executes automatic termination through `ProcessTerminationController`,
  preserving exact identity, protected-process and helper exclusions, current
  cross-session policy, and Windows access checks. The controller preflights
  captured identities and records partial failures. Accepted requests are
  deduplicated by PID and creation time across simultaneous CPU and memory
  events. Action Log and notifications count the requests accepted for that
  action. Failures are recorded in Action Log and runtime status. Termination has no
  baseline, recovery entry, or restore path.
- Restart validates every captured exact instance, requires a non-service app
  in Winderust's Windows session, rejects mixed Windows logons, and prepares one
  suspended replacement with the oldest captured instance as its explicit
  parent. Windows inherits that parent's token, affinity, priority, and job.
  Before requesting termination, require a distinct replacement primary token
  and copy the original token object's DACL and mandatory label to it. An
  elevated caller otherwise passes its own security to the new token object.
  Use the token user's environment and the executable's parent directory.
  Previous arguments are omitted.
- `src/control/process_restart.rs` and `src/platform/windows/process_restart.rs`
  share a two-second wait budget across exact original handles and resume the
  prepared primary thread once after confirming every exit. Failed preflight
  leaves the originals running. A stop or wait failure cancels the suspended
  replacement and reports the number of confirmed exits. Simultaneous CPU and
  memory restarts deduplicate by captured PID and creation-time set, including
  failures. Restart events run before simultaneous termination events and share
  accepted termination requests and confirmed exits with them for deduplication.
  Restart is irreversible and has no recovery or restore path.
- Restore affinity before restart runs after restart preflight validates all exact
  targets and before the replacement is prepared. It restores only a confirmed
  hard-affinity assignment owned by Adaptive Engine on the selected parent. A
  restoration failure stops the restart before any original is terminated.
- Resource Thresholds starts a 60-second cooldown per exact executable path when
  a Restart attempt begins, including attempts that fail. The cooldown survives PID changes
  within the same automation worker. A Restart suppressed by the cooldown counts
  as that incident's response. The Skipped result follows the Action Log recording
  level, and enabled notifications include the reason. Restart becomes eligible on
  a new incident after the cooldown expires. Independently configured Terminate
  actions can still run during the cooldown.
- Windows notifications use the existing registered tray icon. Saved enabled rules
  that can send alerts keep the icon available independently of hide-on-close.
  Resource Thresholds notification text leads with the actual action outcome.
  Windows accepts the request and applies its own display and quiet-time settings.
  Settings stay portable and Action Log entries stay in memory.

## Power plans

- Failed Adaptive plan setup retains the created GUID until verified cleanup succeeds. Pending cleanup uses the existing bounded retry interval and does not allocate replacement plans. Ownership starts immediately after duplication, before metadata or idle-state initialization. A never-activated clone whose source remains active preserves the ordinary restoration chain; genuine external plan changes still invalidate it.
- Power-plan selections belong to the page or rule that exposes them. By Activity owns Idle/Active plans; other automation rules own `power_plan_guid`. There is no global power-plan fallback or load-time mapping migration. The `power_plans` field on `ByActivitySettings` owns the visible Idle/Active selections.
- Ordinary rules wait for a known power source when A/C pause is enabled or
  their settings differ between power profiles. While waiting, the worker polls
  at the configured check interval and can read the current plan and detect
  external changes. It retains the temporary restore point. Explicit automation
  disable, removal of all ordinary plan rules, and Exit can still restore the plan.
- Visible Power Plan Control pages and Process List Rulesets refresh the plan
  catalog on window focus and when a changed active GUID is absent from the
  catalog. Refocus or an active-plan change during a query queues a fresh read
  and discards the older result.
  Advanced Power Plan Tuning retains unsaved edits during catalog refresh.
  If the selected plan disappears while edits are pending, the page reports it
  as unavailable and disables Apply.
- Power Plan Control Status shows up to one qualified alternative per
  lower-priority mode from the same ordinary decision pass. It does not make
  additional Windows queries or evaluate ordinary alternatives while paused
  or while Adaptive Processor Power owns the plan. By CPU Load selections
  identify Else plans and show when a release threshold or minimum hold retained a match.
- Restore previous plan when no rule selects a plan is one global setting on the
  Power Plan Control landing page. It defaults off, applies to both power
  profiles, and is stored as `general.restore_previous_power_plan`. Target,
  Else, Idle, and Active plans remain local selections and take precedence
  whenever their rules are eligible. There are no per-mode restoration overrides.
- While restoration is enabled, all ordinary power-plan winners share one
  temporary episode baseline, captured from the actual plan before the first
  verified automatic switch and separate from the ordinary controller's original
  automation baseline. The episode survives app, feature, CPU Else, and Activity
  Active plan handoffs. Only after every rule yields does the controller restore
  through its existing verified recovery transaction. Turning the policy off
  cancels temporary returns without changing the active plan, even if the
  policy is enabled again before the worker processes the saved changes. Any pending
  journal transaction still requires acknowledgement before another mutation;
  it cannot recreate the canceled baseline. Ordinary shutdown and recovery
  safety ownership remain independent of this optional runtime return.
  External plan changes invalidate the baseline. Unavailable focus, plausible
  process matches, CPU samples, or activity observations do not prove a condition
  ended; they defer affected fallthrough while a known higher-priority rule can
  still take over. During this deferral, Power Plan Control shows that it is
  waiting for observations. Deferred candidates cannot start a By CPU Load hold,
  even when their target matches the active plan. Pending restoration keeps the
  worker alive and retries
  independently of feature enablement. Failed or no-op returns never log a
  successful switch.
- When the last automation feature releases the worker, Power Plan Control
  receives the last verified plan and any cleanup error before the worker exits.
- If a plan read fails after automation is disabled or paused on A/C, Power Plan
  Control shows the effective stop state beside the last verified plan. A pending
  restoration keeps its own status.
- A failed active-plan observation appears on Power Plan Control and its rule
  Status views. A fresh successful read clears the error even if the plan GUID
  stays the same.
- By Focus and By Running App rules choose one executable path or exact executable
  filename. A name rule compares the whole basename of the observed absolute image
  path using Windows ordinal case-insensitive matching. The first enabled rule
  with a matching target and selected plan wins within each mode. By Focus checks
  the protected state of the focused process. By Running App retains its current
  session and protected-process checks.
- By Focus Preview matches uses the same rule order and selector matching as
  automation. It shows the first eligible match followed by other eligible
  matches in the same order. It checks each valid path as it is entered against
  the draft, even when By Focus is disabled. The check is read-only.
- If an active-plan read fails, By Focus Status clears the earlier selection
  when an absolute image path already observed in the same pass rules it out.
  If the path is unavailable, the worker keeps the earlier selection while the
  saved profile still supports it.
- By Running App Preview matches uses the same eligibility checks to report matches
  and the winning rule for the current draft. It reads immediately on open and
  refreshes while the preview is visible.
- If an active-plan read fails, a confirmed absence of matches or a verified
  earlier By Running App rule clears the previous selection. An unavailable
  observation or a later match can leave the earlier rule unresolved.
- By Activity uses the same activity sample to select a plan and schedule the idle
  deadline. Time spent verifying a plan switch counts toward that deadline.
  If an active-plan read fails, the earlier By Activity selection remains visible
  while it still matches the saved profile and no known activity contradicts it.
- By Time rule clocks use parsed minute-precision local times and serialize as `HH:MM`.
  Rule selection and the next deadline use the same local time reading. Time
  spent verifying a plan switch counts toward the wait for that deadline.
  Invalid clock values fail settings load or import; raw incomplete edits stay in the UI editor.
  Repeated local clock boundaries retain both occurrences. A maximum 60-second
  scheduled recheck also covers local offset changes between rule endpoints.
  If an active-plan read fails, Status retains the earlier By Time selection
  only while the sampled time and saved profile still select that rule and plan.
- By CPU Load thresholds use bounded percentages from 0% to 100%. Between ranges
  include both bounds and require the threshold to be no greater than the upper
  threshold. Invalid values fail load, import, and save; incomplete or reversed
  range edits stay in the rule editor until corrected.
- By CPU Load requires finite observations from 0% to 100%. Missing or invalid
  observations, settings invalidation, and power-profile changes restart sustained
  duration timers; time in an inactive profile cannot satisfy a rule's duration.
  Deferring ordinary switching while Adaptive owns the active plan also resets
  qualification; unobserved time cannot complete an ordinary load condition.
  Duration starts at the first matching evaluation and advances with sampled CPU
  usage. Focus changes and other checks can reuse a sample without advancing it.
- By CPU Load durations range from zero through `i64::MAX` seconds, matching TOML
  integer storage. Larger edits remain invalid drafts and fail persistence preflight.
- By CPU Load Switching stability is optional and leaves existing rules unchanged
  when disabled. Entry still requires the configured sustained duration. Once the
  primary plan is confirmed active, its inclusive release threshold or wider
  release range retains the rule; a minimum hold can delay release beyond that
  range. The hold starts at confirmation, including when the plan is already active,
  and repeated checks do not extend it. Earlier qualified CPU rules and higher-priority
  features can preempt it. Else selections never acquire a primary rule's hold.
- Missing observations, settings/profile changes, pause, Adaptive deferral, and
  observed external plan changes clear held By CPU Load state. A failed or
  unconfirmed application cannot start a hold. Active-plan observation precedes
  retained-rule evaluation, using the controller's existing observation cadence.
  If a later verification detects a different active plan, a hold or release
  threshold cannot switch it back. The next check reevaluates ordinary rules.
  Release yields to other selected rules or a configured Else plan. The global
  restoration policy runs only when no plan is selected. An Else plan can begin
  or continue the shared temporary episode like any other automatic selection.
- Ordinary power-setting notifications preserve CPU qualification and holds;
  Winderust's own plan switch must not restart qualification and select Else.
  Actual power-source changes and suspend/resume restart qualification and holds,
  including when the two power profiles contain identical settings. They also
  discard cached CPU usage and prime a fresh sampling interval before any CPU rule,
  including a zero-duration rule, can select a plan.
- The global pause for power-plan switching on A/C and shared restoration policy
  belong on the Power Plan Control landing page. The A/C pause applies to ordinary
  plan rules. By CPU Load skips CPU sampling and By Focus skips foreground
  observation while paused. With full Windows event coverage and no pending
  power-plan work, a paused By CPU Load check waits for the active-plan read
  deadline or an earlier deadline required by another rule. Windows events
  still wake it, and failed reads and restoration retain their retry cadence.
  Independent consumers can still sample CPU usage or observe the foreground.
  Adaptive Processor Power uses its own saved enablement for each source.
- Adaptive Processor Power starts off. Saving an enablement or tuning change for one
  source preserves the other source's existing values. The former Adaptive Engine
  processor-power switch has no runtime effect. Previously saved tuning stays readable.
- When Windows cannot report a power source and Adaptive Processor Power policy
  differs between the saved profiles, the worker waits for a known source. It
  keeps a plan still owned by Adaptive Processor Power, checks the actual active
  plan, and defers applying or releasing based on a profile. A lower demand tier
  requires fresh observations after the wait. Turning off automation or both
  profiles still releases owned state. Equivalent profiles can run during an
  unknown reading.
- Minimum unparked during CPU restraint is a per-source floor for the temporary
  managed plan. It raises only the active source's normal class-0 minimum when
  the source and at least one exact CPU priority restraint are verified. When
  restraint ends or the source becomes unknown, the controller restores the
  cached normal values, including while ordinary demand updates are waiting
  for a source. A floor-only change leaves demand tier hysteresis intact.
- Selecting an installed plan on the landing page submits a manual runtime
  command immediately. Save and Discard handle settings drafts independently.
  The worker uses saved policy, rechecks the target, and verifies the active plan
  after switching. Manual switching is unavailable while Adaptive Processor Power
  owns the current plan. An unresolved source-dependent policy still protects the
  managed plan. A successful manual change ends the current restoration episode
  and becomes the baseline for later automatic changes. If Windows reports the
  selected plan as active and the recovery update remains unconfirmed, Winderust
  reports the error and reconciles recovery before allowing another switch.
  Exit waits while the manual command is pending. Enabled rules can switch plans
  again. The temporary Winderust Adaptive plan is reserved for Adaptive Processor
  Power.
- Managed adaptive-plan recovery recognizes only the current `Winderust Adaptive` name and description.
  Startup cleanup rechecks the live active GUID after matching metadata and
  verifies restoration before deleting an active managed plan. It preserves
  externally selected plans that differ from the managed GUID.
- `PowerPlanController` owns active-plan switching and the temporary Adaptive-plan lifecycle.
  `src/power/powercfg.rs` owns typed plan/domain semantics, while raw GUID, power-scheme,
  processor-setting, and effective-power-mode APIs live only in
  `src/platform/windows/power_plan.rs`. Persistent Advanced Power Plan Tuning remains a separate
  explicit application service and never enters the automatic recovery journal. Its optional
  energy preference and long/short-running thread scheduling policies stay outside
  Adaptive Processor Power.
  Tuning presets and Adaptive Processor Power profiles store concrete boost modes.
- Advanced Power Plan Tuning captures the target plan's GUID, live values, and
  normalized draft in the review. The same review gates page Apply and any save
  that includes tuning edits. A fresh read must match the draft's original
  readback before review and match the reviewed values before the staged write.
  Save and Save and Exit also verify the review before saving portable settings.
  If the recheck after that save fails, feedback reports the settings already
  saved and the remaining tuning error.
  These checks include optional setting availability and raw unknown
  indices for each power source. Selection, confirmation, and the
  Windows write preflight exclude the temporary Winderust Adaptive plan. Apply
  and Restore write only changed fields and skip missing, unreadable, and
  unknown optional settings.
  A stale review or failed preflight read preserves the draft. After a plan
  value changes externally, Refresh values reloads the plan for a new edit and
  review. A failed read or a missing plan during refresh keeps pending edits
  tied to their original plan.
  Readback verifies the outcome. Each mismatch identifies the power
  source, setting, requested value, and actual value. When a write stage fails,
  the report includes the stage error and any differences from a successful readback.
  On a failed Apply with readback, the draft retains the original changes that
  are still writable and differ from the plan. When readback fails, the editor
  keeps the reviewed edits pending. Refresh values reads the same plan before
  recovering changes that are still writable. If the write succeeded, a later
  exact read of that plan verifies the result and updates the restore point
  without writing again. Discard drops the retained edits. A retry requires a
  new review.
- Restore previous tuning values keeps one in-memory restore point per plan GUID
  after a verified Apply that changed values. It captures the pre-apply values
  only when the writer can reproduce the editable values exactly and preserve
  unavailable optional settings. Restore requires a
  clean editor, an unmanaged plan, and live values that match the verified Apply
  readback. It uses the same explicit confirmation and consumes the point after
  exact readback of the previous values, including unavailable settings and raw
  unknown indices. Exit clears the points.
- `src/backend/self_power.rs` owns Winderust's hidden-to-tray priority and Power
  Throttling lifecycle, including strict baseline capture, verification, compensation, retry, and
  clean shutdown. While hidden, it requests EcoQoS and keeps the captured process
  priority when automation is enabled and either saved power profile enables
  Adaptive Engine process policies.
  This lets automation respond to sustained CPU load after a power-source change.
  Otherwise it requests Idle priority. Visibility and saved settings changes
  apply synchronously. A process-lifetime
  worker retries failures every five seconds even when automation is disabled,
  and sleeps without polling while settled. Shutdown waits at most two seconds
  for self-power restoration and reports unconfirmed completion without waiting
  indefinitely on a stalled native call. Showing the window restores the
  execution-speed and priority baseline. An external timer-resolution change
  preserves that baseline, and a direct external change to execution speed or
  priority becomes the baseline for that setting. Raw current-process query/set calls live only in
  `src/platform/windows/self_power.rs`. This process-lifetime state does not use crash recovery.

## Failure handling

- CPU allocation application errors distinguish shared discovery failure from target failure in their return type. A cached discovery error cannot reclassify a later acquisition, affinity, mutation or recovery failure.
- Repeated process failure suppression uses `ExecutionFailureTracker` in `src/rules/execution_failure.rs`; the threshold comes from `settings.advanced.execution_failure_suppression_threshold`.
- Auto-exclusion fallback is shared through `PendingAutoExclusions` in `src/backend/automation.rs`.
- On newly suppressed process failures, features emit `auto_excluded_processes`; `WinderustApp::apply_pending_auto_exclusions` persists them into each feature's existing exclusion/rule list.
- Rule-only fallbacks use disabled rules: CPU Sets (Soft), Processor Affinity (Hard), CPU Limiter, App Suspension.
- Exclusion-list features append enabled exact path rules. Adaptive Engine keeps
  user-created executable-name rules separate from these automatic exclusions.

## Memory Trim

- Automatic trimming and Trim now honor Exclude focused apps and Exclude apps
  with visible windows on either power source. Focus exclusion is on by
  default and requires an observed foreground executable path for the catalog
  process generation. Visible-window exclusion is opt-in and uses the same
  top-level window detection as Process Priority. Both exclusions skip window
  owners and other instances of their executables. An unavailable required
  observation pauses trimming and breaks idle qualification without rearming
  already-trimmed processes.
- Automatic trimming requires current pressure, working-set size, and sustained
  observed CPU idleness. Missing/invalid samples, low pressure, and a working set
  below the size threshold break idle qualification without rearming a process
  already trimmed during that idle episode. Observed activity or a new exact
  process identity may rearm it; explicitly disabling automation clears tracking.
- The controller checks the working-set threshold on the verified process handle
  immediately before either automatic or manual trimming. A working set below
  the minimum returns to waiting and clears idle qualification.
- Automatic trim attempts for the same process are spaced at least 60 seconds
  apart based on CPU sample time, including after an error or drop in memory
  pressure. Trim now remains available during the cooldown.
- An automatic pass attempts up to four trims, counting failures toward the
  limit. If eligible processes remain, the worker schedules another pass in
  three seconds. Each pass checks current pressure and eligibility, and starts
  after the last attempted PID before wrapping to lower PIDs. Trim now can
  attempt all eligible processes in one pass.
- The manual reply keeps the trim outcome. The UI reports skipped commands and
  failed attempts. The last completed result stays in memory and includes the
  observed working-set reduction and how many trims could be measured.
- Process List's Trim memory targets the selected exact process or the eligible
  processes in the selected stack. The controller revalidates identity and
  protection on each working-set handle and logs each result in Action Log.
  The command is independent of the automation features' settings.
- Failure suppression is tracked by executable path. Native paths that cannot
  be represented in settings keep runtime suppression and an Action Log entry.
  Saved exclusions and auto-exclusions require a representable path. A successful
  trim clears prior failures after the pass if no attempt for the same path
  failed, was deferred, or was eligible but cooling down. Waiting for pressure,
  observing idleness, or temporarily excluding a focused or visible-window app
  keeps its failure count.
- Before/after working-set reduction is not measured system RAM reclaimed.
  Unavailable readback remains unknown; trimming cannot restore discarded pages.

## App Suspension and CPU Limiter

- App Suspension delay and temporary-thaw interval settings accept 1–86,400
  seconds; thaw, network-wake, and audio-wake durations accept 1–3,600 seconds.
  Load, import, Save, and export validate the shared policy using the same limits
  as the UI.

- App Suspension rejects Session 0, LocalSystem, LocalService, and NetworkService processes plus
  curated Windows shell/shared-host processes. Process List and the App Suspension picker keep
  unavailable targets visible, labeled, and disabled; grouped Process List actions cover every
  captured process in the group. Process List matches suspension status by PID and
  creation time. Other process controls are unaffected.
- Process List groups executable paths by normalized text or, for paths with
  unpaired UTF-16 units, by their original native spelling. Live actions require
  an exact observed identity, and Stop requires one for every selected root.
  Rulesets require an executable path representable in settings. Properties and
  Open process location use the original native path. Creation time is optional
  for these views and file commands.
- Automatic rules and Suspend on the App Suspension page require an absolute
  foreground image for the catalog process generation. An unavailable image pauses
  new freezes and restarts background grace when observations return.
- When a rule candidate is missing its catalog image, App Suspension verifies its
  process generation before accepting an absolute image from the fallback lookup.
  An unverified candidate keeps the rule's observation unresolved and cannot
  qualify for a new freeze or wake observation.
- If an enabled audio or network wake detector reports a failure, App Suspension
  waits for a successful observation before automatically suspending apps that
  use that detector. Affected rules show **Wake detection unavailable** when
  ready to suspend. Manual Suspend and existing resume signals remain available.
- Network wake compares payload byte counts across two samples of the same
  IPv4 or IPv6 TCP connection.
- App Suspension shares one policy across power sources. Power-source changes
  preserve background grace, rule-owned freezes, and active thaw holds.
- Auto resume when focused defaults on. Turning it off stops foreground and
  app-switch resume, including taskbar and tray restore pre-wakes. A focused app
  remains exempt from new rule freezes. Manual resume, configured temporary
  resumes, and cleanup continue to work.
- App Suspension releases a rule-owned freeze when its stored executable path
  no longer matches an enabled rule, even if focus, session, or process
  observations are unavailable. Process List freezes have separate ownership.
  User-initiated recovery preserves the saved enablement mode and reports thaw
  failures in the returned status. Process List freezes release when accounting
  confirms that their exact Job Object has no active processes. An absent root
  or unavailable accounting cannot establish that inherited processes have exited.
- App Suspension shows Not running only for an enabled saved rule covered by a
  successful scan with no unresolved candidate for that path. Suspend requires
  a matching enabled rule in saved settings. Temporarily resume remains
  available for a matching known freeze.
- Automatic rules, Suspend on the App Suspension page, and Process List
  Suspend/Resume share the controller in `src/control/suspension.rs`. Feature
  code owns grace, wake, and reporting policy. Automatic rules require executable
  paths representable in settings. Foreground release and saved-rule Resume
  compare native paths held by the controller. Process List freezes keep their
  exact process status. Only paths representable in settings contribute to
  saved-rule indicators and grace intervals. The controller retains failed
  thaw/finalization state for bounded retry and explicit shutdown. The crash
  helper opens the exact named job before freeze and
  thaws that retained job even if the recorded root exits while inherited children remain. Raw Job
  Object creation, assignment, and membership live in `src/platform/windows/job.rs`; freeze/thaw
  and the shared undocumented layout live in `src/platform/windows/suspension.rs`.
- Efficiency Mode and CPU Limiter's CPU time rules use Focus, Visible Window, and Background
  columns with Focus > Visible Window > Background precedence. The selected power profile defines
  the 1% to 100% default targets. Each CPU time rule tier selects Profile Limit for Winderust's
  matching target, Custom for its own 1% to 100% target, or No Time Cap to turn off
  the cap for that tier. A 100% target also leaves the tier uncapped.
- CPU Limiter's CPU time method uses a 100 ms freeze/thaw duty cycle. It shares
  `SuspensionController` with App Suspension; independent owner phases combine
  into one effective frozen state, and one feature cannot thaw the other's claim. One native
  high-resolution waitable-timer worker owns all limiter schedules. Job Object control remains the
  primary backend; only `NotSupported` job assignment selects CPU Limiter's exact-thread fallback.
  That fallback owns one suspend-count increment per exact thread and never applies to App
  Suspension. Active limiter target and process-appearance refresh remain at one second even while
  hidden or under Adaptive Engine saver cadence. When CPU Limiter is disabled or the saved rules
  have no finite limits, automation keeps a refresh scheduled until the timing worker releases its
  remaining targets and the cleanup status and Action Log can be published.
  On Exit, the shared suspension controller's final Job cleanup determines the Job result.
  CPU Limiter reports its own thread fallback and timing worker failures separately.
  The applied rule's editor shows the worker's confirmed duty-cycle target and backend,
  including a parent app group's target for inherited members. The percentage is the
  configured awake share of each 100 ms cycle.
  Inherited Job coverage retains a child while the matching Job is owned and the child
  may still be live. A complete process census drops absent or replaced child generations.
  CPU Limiter reads the foreground only when an eligible rule's Focus cap differs from another
  tier. Required observations that are unavailable pause CPU Limiter and its eligible rule status.
  When A/C and battery limits differ, CPU Limiter pauses and releases its limits until Windows
  reports the power source. App Suspension retains its own freeze claims.
  Process inventory and retained cleanup errors remain visible in the status rail.
- Load-triggered affinity is a separate method for an exact executable path. Consecutive CPU
  samples are normalized to host logical processor capacity and tracked per process generation.
  A qualified rule requests a group-0 selection through the CPU allocation coordinator. It can
  only narrow the verified affinity mask. A verified limit stays active for at least 10 seconds
  under continuous observations. Once CPU use remains for three seconds below 70% of the lesser
  of the configured trigger and the verified restricted capacity, the limiter releases it and
  waits 20 seconds before qualifying again. Missing or invalid observations, disable, and rule
  removal withdraw the claim. The coordinator owns recovery, restoration, and handoff.

## Runtime observations

- Per-processor placement samples are complete-or-unavailable: invalid intervals never become zero load or shift processor indices. Counter regression, overflow, inconsistent idle deltas and zero-length intervals are unavailable; failed/empty captures clear the baseline and domain-size changes re-prime it.

- Adaptive process policies and Adaptive Processor Power share a generation-checked foreground
  descendant workload and CPU counter sample per observation cycle. Processor Power keeps its own
  CPU baseline and sampling schedule, including with every process policy off. It re-primes after
  power-source and session changes, suspend/resume, or disable. Standalone priority rules retain
  executable-path matching.
- Foreground CPU deltas require complete samples of the same exact process generations; missing members and counter regression discard the baseline.
- CPU Set discovery is lazy and operation-local, including failure caching. Existing explicit and Adaptive claims preserve assignments on discovery failure without suppressing applications; pending handoffs retry on the existing cleanup interval. Guarded Instance Balancing claims withdraw instead.
- Process enrichment cannot replace the identity generation captured by the cycle. Newly available or changed generations are deferred (non-actionable) until a fresh cycle, preserving consistency with cached Adaptive membership and earlier consumers.
  Catalog path matching requires that captured creation time and an absolute image.
  When the catalog lacks an image, Winderust checks the creation time on the same
  handle used to read the path.

## CPU allocation and Adaptive Engine

- CPU allocation has one runtime coordinator and deterministic precedence: CPU Sets (Soft) >
  Processor Affinity (Hard) > Instance Balancing > CPU Limiter > Adaptive Engine. Feature modules own
  policy only; the coordinator alone owns affinity/CPU Set baselines, mutation, compensation,
  arbitration, and restoration. A higher-owner release queues the exact process key; `RuntimeCore`
  reconciles it once after every CPU producer has processed that worker pass. Shutdown bypasses
  this handoff and directly restores all coordinator-owned state in reverse application order.
- CPU Sets, Processor Affinity, Instance Balancing, load-triggered affinity, and Adaptive Engine
  use the typed boundary in `src/control/cpu_allocation.rs`. Do not restore feature-owned raw
  setters, property baselines, recovery calls, or affinity-owning `Drop` paths. Exact identity,
  mutual exclusion, actual-owner Action Log attribution, and clean/crash restoration are part of
  the boundary. Raw affinity, CPU Set, and packed topology-buffer calls live only in
  `src/platform/windows/cpu_allocation.rs`.
- Adaptive Engine's master switch gates its policies. CPU Pressure Restraint, Limit Background
  Processors, and Dynamic Resource Zones each have their own switch and eligibility by default.
  Limit Background Processors exposes one explicit processor selection:
  least-used logical processors across All, P-core, or E-core pools with a configurable percentage,
  fixed P/E/no-SMT topology masks, or an exact custom mask. The per-app CPU threshold decides when a background app becomes eligible;
  the shared CPU allocation coordinator remains the only mutation and restoration owner.
- Only during CPU Pressure Restraint limits the placement policy to exact processes whose
  priority restraint is selected and verified in the current pass. If CPU Pressure Restraint
  is off, the scoped limiter is dormant. It releases the placement claim when the episode
  loses verification or ends. Dynamic Resource Zones, explicit CPU allocation, and manual
  ownership keep their existing precedence.
- The Processor placement only action qualifies the same pressure episodes and uses the
  existing background processor selection. A hold starts only with an exact verified
  placement from the scoped limiter. A higher owner invalidates that claim and a fresh
  evaluation is required before the limiter can apply again. Existing assignments and
  zoning do not count towards verification for this action.
- Limit Background Processors can instead reduce each target's original group-0 affinity by
  a configured count, retaining at least one logical processor. Selection stays stable for the
  exact process while its captured baseline holds. Reduce affinity preserves external default
  CPU Sets. The preservation switch on each hard method also preserves narrowed external affinity.
  Guarded owner handoffs restore or relinquish the previous managed assignment if the target
  cannot be changed.
- Restore child process affinity acts only on observed direct children of a verified
  Adaptive Engine hard-affinity assignment. The coordinator requires a newer exact
  child identity, a known matching session and single group 0, and a live mask equal
  to the parent's narrowed mask. Existing placement owners, explicit rules, default
  CPU Sets, and different external masks take precedence. The child's recovery journal
  uses the parent's original mask so compensation or crash recovery cannot preserve the
  inherited restriction as the desired baseline.
- Adaptive per-process demand requires consecutive valid CPU observations for
  the same exact identity. Missing samples, invalid deltas, or a non-actionable
  target discard its old qualification/recovery state without resetting unrelated
  targets or changing global pressure policy.
- CPU Sets (Soft) and Processor Affinity (Hard) share one CPU-selection preset catalog. The
  topology-derived Core Presets are read-only: All, P-cores, E-cores, All cores no SMT, P-cores no
  SMT, and E-cores no SMT. CPU Sets (Soft) also shows one read-only preset per group on a
  multi-group system. Custom presets remain editable. Every rule independently selects Focus,
  Visible Window, and Background masks with Focus > Visible Window > Background precedence.
  Selecting a preset copies its complete selection and topology signature into that tier;
  later preset edits or deletion do not silently rewrite configured rules.
  Both features report Paused while an observation required for rule selection is
  unavailable. When Windows cannot confirm the power source, differing saved Soft and Hard
  policies pause explicit placement and restore their assignments. The comparison includes
  configured Soft exclusions, so even an inactive row or a different path spelling can
  require a pause. Equivalent settings continue to work. Adaptive Engine placement and
  Instance Balancing reserve configured placement paths from both profiles during the pause.
  Restoration failures remain current while the policy's released assignments
  remain owned. Once reconciliation settles them, Status keeps any independent observation
  error, and the original restoration failure remains in the Action Log.
- CPU-selection rules, custom presets, and Adaptive Engine custom selections store
  explicit `{ group, number }` members and an optional exact canonical topology
  snapshot. This signature describes scheduling layout, not permanent hardware
  identity. Its active-group masks use fixed-width hexadecimal strings to preserve
  logical processor 63. Malformed members or snapshots fail load/import without
  rewriting files. Old scalar masks and index arrays are not migrated or aliased.
  Missing signatures, changed topology, absent processors, and groups unsupported
  by the selected control remain preserved for review; saving or importing them
  never binds them implicitly.
- CPU Allocation topology discovery retains group-qualified logical processors,
  physical-core membership, efficiency classes, NUMA nodes, and cache domains.
  `src/cpu_topology.rs` validates these facts; the Windows adapter alone parses
  native variable-length records. CPU Sets (Soft) resolves complete group-qualified
  selections to current CPU Set IDs across all discovered groups. Duplicate
  processor mappings, duplicate IDs, or any missing selected member block the
  entire request before an ownership handoff. The shared inventory retains each
  Set's allocated/unallocated flag; it does not prove availability or reserve a
  processor. Existing explicit and Adaptive selections still resolve either
  state. Native readback verifies the
  process-default ID assignment, not actual thread residency; thread-selected
  CPU Sets, hard affinity, reservations, and Windows scheduling still apply.
  Processor Affinity (Hard) and Adaptive per-processor sampling remain explicitly
  limited to group 0. Cache presets require the entire verified L3 data/unified
  domain to fit the selected control's supported processors; no partial domain or
  inferred CCD is offered. Persisted selections are checked against a
  verified current snapshot on activation, including cached claims and pending
  ownership handoffs. Unresolved allocations restore owned state without gating
  unrelated Adaptive Engine controls or restoration itself. Discovery has no
  synthetic topology fallback on this path.
- Unavailable processor bits are not silently removed from selections. Editors
  preserve unsupported saved masks and require an explicit valid replacement;
  explicit review rebinds the entire saved membership only when every processor
  remains available and representable. Topology changes during preset editing
  block Save until the current selection is explicitly reviewed or replaced. Soft
  allocation rejects incomplete or ambiguous CPU Set mappings before mutation.
  Hard allocation requires the entire requested mask to fit the target's system
  affinity mask; it never applies a partial intersection.
  New hard-affinity claims require a verified single group-0 target; group
  validation does not gate compensation, release, or crash restoration.
- Instance Balancing has a guarded backend claim, below explicit CPU Sets (Soft)
  and Processor Affinity (Hard) and above Adaptive Engine. Its global configuration
  defaults off and is shared across power sources. Its CPU Control page edits
  executable-path rules and displays runtime status. Literal paths use Instance
  Limits' scope and ordinal comparison,
  with duplicate paths rejected even in disabled rules. Claims require a rule generation and a
  topology-bound Soft CPU Set request, one group-0 topology, complete current CPU
  Set mappings, and whole physical cores with no allocated siblings. Every selected
  CPU must fit the original allowed hard mask and the current system mask; no partial share
  or All fallback is substituted.
- One-second visible/tray observations qualify two identical complete actionable
  cohorts at least one second apart. Previously retained live members missing from
  a new catalog make the rule incomplete; absence alone never proves exit. Fresh
  SID/session and liveness queries use the retained handles, while mutation still
  reopens the exact target through shared safety and cross-session checks.
  Loss of full Windows event coverage withdraws balancing claims and pauses
  observations until coverage returns. The CPU allocation coordinator keeps
  retrying unresolved restoration.
- Each rule partitions the common eligible whole-core pool, balancing core counts
  within each numeric efficiency class. Survivor admission order and valid prior
  core assignments are stability hints only. New accepted plans are at least five
  seconds apart. Fewer than two eligible members or insufficient whole cores yields
  the entire rule; no overlapping or empty share substitutes for insufficient capacity.
- Explicit CPU selections, known denied control, unmanaged defaults and drift
  suppression exclude members visibly in Action Log. Incomplete observations or
  unavailable eligibility withdraw that rule's claims. A fresh catalog, topology
  and eligibility pass must match before group application. Settings, session and
  watcher changes cancel subsequent work; an admitted native operation may finish.
  Status discards new observations superseded by saved settings or a change in
  the observation context.
  Native writes are not atomic across members. Any failed or shadowed share stops
  the plan and withdraws every balancing claim for the rule, retaining failed cleanup.
  A failed target operation records Failed in Action Log. Context and eligibility
  changes that prevent application remain Skipped.
  The live Status panel translates waiting reasons, assignments, and explicit CPU
  selection exclusions in the current UI language from the existing observation.
  The main table reads each exact saved rule's result from the same published
  snapshot. An edited executable path waits for its own observation after Save.
  Relevant settings, session, watcher and placement-context changes clear row
  results before the next observation. Worker exit also clears these results.
  The live observation is shared across both editor power profiles.
  Scanned and skipped totals require complete coverage of enabled rules. Verified
  adjustments remain visible when another rule's observation is unavailable.
  Explicit CPU selection exclusions remain visible across rules during assignment.
- Observed cohort, pool or control-context changes withdraw current preferences
  before qualifying again. Topology changes remain paused until explicit rearm.
  Rule/feature/master edits preserve reset intents even when reversed between worker
  passes; power-source changes alone do not rearm drift suppression or discard hints.
  Different executable rules may prefer the same cores. Readback confirms accepted
  scheduling preferences, not core exclusivity, thread residency or faster execution.
- For managed properties, balancing uses the original baseline only while
  coordinator ownership is confirmed and the live value matches its expected
  value. Unowned properties use their current native values. Unowned nonempty
  CPU Sets, unavailable facts, uncertain recovery state, and external drift block
  placement. Rebalances do not shrink from a prior managed share. A final native
  read checks the expected affinity/default Sets before applying; the read/write
  sequence is optimistic, not atomic against another controller.
- A balancing claim's generation identifies an explicit rearm for an exact
  process. Observed drift suppresses that generation across release and retries;
  an older generation cannot rearm it. Other CPU features retain their own
  reapplication contracts. Guard failure withdraws balancing ownership through
  the existing reconciliation/restoration paths without losing pending cleanup.
- CPU Pressure Restraint qualifies sustained CPU use per exact process while
  aggregate pressure is high. Protect focused app and Protect focused app’s
  children are independent and default on. Turning either off admits that
  part of the observed focus group to CPU Pressure Restraint only. Other visible
  window groups, active audio, and exclusions stay protected. With Protect
  non-Normal priority on, a temporary Below Normal or Idle claim requires a
  Normal baseline. Turning it off allows lowering known non-Realtime priorities
  and restores the captured value at release. Placement-only restraint uses the
  same protection settings. A priority change observed from outside Winderust
  ends priority ownership for the current qualified episode. Efficiency Mode
  and saved Process Priority take precedence on the same process.
  Status lists the exact process instances whose priority or placement restraint is
  verified, whose priority restoration is pending, or whose priority ownership is unconfirmed.
  The list follows existing controller transitions and retries without querying
  Windows again. Pending and unconfirmed entries remain visible while paused or
  disabled.
  Thread, GPU, and Memory Priority and Dynamic Priority Boost use their dedicated settings.
  An optional Low or Very Low I/O Priority claim is gated on an exact verified restraint
  episode. The dedicated I/O Priority policy reserves applicable targets even when its
  requested value requires no write. Process List manual choices also make Adaptive yield.
  The shared I/O controller keeps release and retry ownership separate for the two producers.
  Optional Energy-efficient scheduling requires a verified CPU restraint for the same process.
  It adds an execution-speed hint through the shared priority and efficiency controller,
  preserves timer-resolution settings, and yields to saved Efficiency Mode, manual choices,
  or external execution-speed control. Restoration uses the original raw power state.
- CPU Pressure Restraint qualifies a process after at least two valid hot samples
  and the configured Busy time before restraint. CPU sample interval sets
  sampling cadence independently. CPU usage to watch determines which aggregate CPU readings
  qualify or maintain pressure. Combined sources can use separate system and foreground
  thresholds, with a five-point release band per signal. Zero CPU usage cannot
  sustain pressure. An override is dormant in single-signal modes. Foreground identity
  is still required for safe targeting.
  The editor accepts Minimum and Maximum restraint times in milliseconds up to one hour.
  The settings file stores these times in seconds and accepts fractional seconds
  at millisecond precision. A nonzero maximum ends the current adjustment at
  evaluation and requires fresh qualification. Optional Pause when idle uses the
  current Windows session's last input and withdraws CPU restraint and its optional effects. Missing idle
  evidence pauses restraint while this option is enabled. Resumption requires
  fresh qualification, and restoration entries stay visible during the pause.
- Adaptive Engine pauses its process policies and releases their claims when the
  focused process ID or creation time is unavailable. Saved settings, manual
  ownership, and Adaptive Processor Power retain their own state.
  Home and the Status panel report paused process policies separately from a
  verified Adaptive power plan. Process counts remain unavailable until observed.
- When Exclude service hosts is enabled, Adaptive Engine excludes observable active
  service hosts, known service accounts, and Session 0 or unknown-session processes. Unavailable
  service inventory pauses the process policies and restores managed state.
- Adaptive Engine Exclusions match exact executable names, exact paths, or
  executable name patterns. Both name selectors use the observed basename
  across directories. Exact path rules and automatic
  failure exclusions retain their path scope. The process identity required for
  any scheduling change is verified independently of the exclusion selector.
- Adaptive Engine uses the shared right-rail Status / Presets pattern. Applying a
  preset stages tuning for the selected source and preserves the master switch,
  per-app exclusions, and Adaptive Processor Power settings. Enabling from
  factory tuning selects Balanced for that source. Setup stages the change for
  Save, and the tray switch applies it through the existing immediate settings patch.

## Interrupt Timing

- Run diagnostic in Adaptive Engine's Interrupt Timing tab starts a five-second,
  in-memory trace even when Adaptive Engine is off. It changes no settings or
  scheduling rules. The result shows up to five rows by routine module and
  interrupt type, ranked by inclusive time. Event loss, buffer loss, and invalid
  timings are shown above the results with an incomplete capture warning. An
  affected capture without reportable rows shows that no usable samples were
  retained. Attribution and incomplete-interval counts remain visible separately.
  The module contains the routine address recorded by ETW, which can point to a
  driver framework. The report includes intervals fully contained in the five
  seconds after ETW startup.
  Intervals crossing either boundary count as incomplete. Capture timeline
  shows inclusive DPC, ISR, and Threaded DPC time across processors for each
  elapsed second. It splits intervals crossing a second boundary between those
  seconds. Per-CPU shows inclusive DPC and ISR times in milliseconds on verified
  single-group systems. Both sections start collapsed for each new result. When
  a report has module rows and the CPU breakdown cannot be verified, Per-CPU
  shows Unavailable.
- A completed report can be exported as JSON to a user-selected path. The
  export includes module basenames, aggregate durations, loss and attribution
  counts, and per-CPU totals when available. Its `timeline` array has five rows
  in elapsed-second order, starting at 0–1 seconds. It does not include raw trace
  events, addresses, or driver paths. When available, `capture_start` includes
  a UTC sample for approximate log correlation and the span in nanoseconds
  between the QPC sample that starts the first bucket and the QPC sample
  immediately after the UTC read. The wall clock can change during the
  capture. Use elapsed seconds for timing within the report.
  Cancelling or failing an export leaves the completed report available for
  another attempt. Once a destination is selected, the Exit prompt shows export
  progress and enables Exit after the write finishes. Closing the prompt lets
  the export continue. A failure
  during an open Exit prompt also appears in the prompt.
- Cancel and app exit close the capture helper's lifetime pipe. The helper
  records the capture ID in an administrator-only, volatile Windows registry
  value before starting ETW. The surviving app can clean up after a failed
  helper. If both processes terminate, Winderust tries cleanup at its next
  startup and before another capture. Cleanup skips a live helper and verifies
  the exact capture name and logger GUID before stopping a remaining trace.
  The app gives the helper 12 seconds overall and up to four seconds after
  Cancel or Exit. It stops only its own child and confirms exit before checking
  the lease. The UI can report a timeout while cleanup continues, and another
  capture stays blocked until the pending exit or cleanup check completes.
  When the user cancels before the page receives the report, any queued report
  is discarded and cleanup errors still reach the page. A failed or cancelled
  retry leaves the previous report available. A successful retry replaces it.

## Priority and efficiency

- Process List shows a current priority, Dynamic Priority Boost, or Efficiency
  Mode value for a group only when every selected process has a matching
  verified value. Readback follows Allow cross-session process control.
  Eligible selections with unknown Efficiency Mode can still choose Enabled or
  Disabled.
- The Process Priority Status panel counts processes whose automatic priority
  claims yielded to Efficiency Mode in the latest observed policy pass. The
  count describes claim precedence. Windows priority readback remains separate.
- Efficiency Mode uses the same explicit Foreground Detection and Visible Window Detection
  layers as Priority Control. Foreground Detection defaults on, Visible Window Detection defaults
  off, and each layer owns an Enabled/Disabled Efficiency Mode default. Automatic targeting
  always excludes the same 24 built-in process names. Custom rules match exact executable paths.
  Home and Status distinguish missing observations from a confirmed zero.
  Inventory and pending restoration failures surface as errors, with the
  restoration failure taking precedence until retry succeeds.
  When Windows cannot verify the power source, differing saved Efficiency Mode
  settings pause automatic claims if either profile can apply them. The comparison
  includes settings that may not affect the current processes. The worker releases
  only its Efficiency Mode claims and rechecks the source while paused, or
  when a dormant profile could become active without power notifications.
- Every Priority Control page uses three ordered default tiers: Focus App, then apps with visible windows, then background. Visible Window Detection defaults off and has its own selectable value; custom process rules independently override all three tiers. Retired Auto priority values are rejected rather than mapped to current defaults.
- Process Priority, Efficiency Mode, Thread Priority, Dynamic Priority Boost,
  I/O Priority, GPU Priority, and Memory Priority each configure child process
  protection independently for each power source. Both switches default on and
  follow the corresponding detection setting. A verified descendant in the
  Background tier skips the Background default or custom rule and releases
  the feature's existing automatic claim. Direct Focus and Visible Window
  policies and manual Process List changes keep their existing ownership.
- Resolve descendants from the focused PID and actual visible-window owners
  using the shared process observation. Each edge requires creation times and
  rejects a child older than its parent. Cache the resulting groups within the
  reconciliation pass. A feature with Focus child protection enabled pauses
  when a required Focus observation is unavailable.
- Each Priority Control pauses when Windows cannot verify the power source and
  its effective enablement or saved enabled settings differ. It releases its
  automatic claims through its existing restoration path. A dormant profile
  rechecks the source when full power notifications are unavailable.
- Foreground Detection requires an executable tied to the captured process generation. An
  unavailable or unverified image makes the Focus observation unavailable.
- I/O Priority and Dynamic Priority Boost restoration retries publish their current
  state and record verified baseline writes in Action Log even after the feature is
  disabled. Retry failures update current status. Successful retries clear the
  resolved restoration error, retain current inventory or application errors,
  and leave the original Action Log failure in history. When focus is unavailable, both
  features pause if an eligible process would receive different Focus App and
  fallback policies after accounting for custom rules and I/O preservation.
  Existing automatic claims yield through the usual restoration. Verified
  Process List changes keep their manual owner. Home and the status rail
  distinguish paused observations from a confirmed zero. Missing required
  inventory and failed restoration surface as errors.
- Process Priority, Power Throttling/Efficiency Mode, Dynamic Priority Boost,
  Thread Priority, I/O Priority, GPU Priority, and Memory Priority are complete
  typed process-control cutovers. Static Priority Control, Efficiency Mode,
  Adaptive Engine/Adaptive Engine policies, and Process List
  one-shot actions share their `RuntimeCore` controllers; feature code owns
  policy only, and the crash helper remains the independent recovery mirror.
  Dynamic Priority Boost's raw live query/set pair is isolated in
  `src/platform/windows/dynamic_priority_boost.rs`; its controller still owns the full recovery
  transaction and restoration chain.
  Process Priority and process Power Throttling raw class constants, state conversion, and live
  query/set calls are isolated in `src/platform/windows/priority_efficiency.rs`; their compound
  controller still owns arbitration, recovery transactions, compensation, and restoration.
  When a Priority or Efficiency policy yields, restoration also checks exact
  writes retained after an unsuccessful apply. A blocked recovery transport
  leaves the tracked baseline available for recovery and reports the failure.
  Efficiency Mode counts a process as restored only after its pending
  priority and power changes settle. A retry updates its current failure status
  and keeps the original failure in the Action Log.
  Memory Priority's raw class constants and live query/set pair are isolated in
  `src/platform/windows/memory_priority.rs`; unknown raw baselines remain controller-owned and
  exactly restorable.
  GPU Priority's raw D3DKMT query/set pair and NTSTATUS classification are isolated in
  `src/platform/windows/gpu_priority.rs`; temporary missing-context handling remains unchanged.
  I/O Priority's undocumented NT declarations, numeric information class, query/set pair, and
  NTSTATUS classification are isolated in `src/platform/windows/io_priority.rs`.
  Thread Priority's Toolhelp enumeration, thread open/identity reads, priority constants, and live
  query/set calls are isolated in `src/platform/windows/thread_priority.rs`.
  Thread Priority identity includes the exact process instance, thread ID, and
  thread creation time. Do not restore feature-owned setters, Process List
  restore closures, or duplicate Adaptive Engine setters for these properties.
  When focus is unavailable, Process Priority, Thread Priority, GPU Priority,
  and static Memory Priority pause before
  applying a Visible Window or Background policy that differs from what the
  same eligible process would receive as the Focus App. This comparison covers
  custom rules and preservation as well as priority values. Existing claims
  yield through the usual owner-aware restoration.
  Home and the status rail distinguish incomplete Process Priority and Thread
  Priority observations from a completed scan with no matches. Show a real
  restoration error ahead of an inventory error. Thread inventory failure still
  leaves otherwise eligible existing ownership in place and stops the pass.
  Quiet Process Priority and Thread Priority retries update current restoration
  status and record verified baseline writes in Action Log even after the feature
  is disabled. The original failure stays in history, and other current errors
  stay visible. Process Priority clears its release error after another policy
  resolves the queued change and only credits baseline writes made by its own
  retries.
  GPU Priority pauses the policy when required focus, session, window or process
  observations are unavailable. Its Home summary and status rail distinguish
  missing observations from a completed scan. A restoration error takes
  precedence over a process inventory error in runtime status. Quiet GPU
  restoration retries update that feature's status and record confirmed
  baseline writes in Action Log even when the feature is disabled. The current
  restoration error clears after cleanup while any unrelated application or
  observation error remains visible.
  GPU Priority treats a temporarily unavailable GPU scheduling context as
  pending and retries without auto-excluding the process. Memory Priority
  restoration counts and Action Log entries describe verified baseline writes.

## Sleep Prevention

- Sleep Prevention is an explicit running-app rule under Watchdog. It starts
  disabled and uses the same rules on A/C and battery. A matching eligible
  executable requests idle system sleep prevention. Keeping the display on is
  a separate opt-in that also retains the system request. Focus does not affect
  matching.
- The feature manager combines exact eligible process instances across rules.
  One controller owns the shared Windows request object and changes only the
  required request types. Removing one app cannot release another app's request.
  Disabling the rules or confirming the last app's exit releases the claims
  held for those apps. Exit releases all claims. Process teardown closes the
  request object without a crash journal.
- Temporary keep awake requests system sleep prevention. Temporary keep display
  on requests both display and system sleep prevention. Each action accepts
  1–1440 minutes and has a separate monotonic deadline. Both work with automated
  rules off and continue across power-source changes. Stop or expiry releases
  only that timer's claim. Suspend/resume, a session change, loss of the
  temporary action's Windows event watcher, or Exit cancels both timers.
  The dedicated watcher runs only while temporary requests are active.
  If Windows rejects clearing the display request when its last claim ends,
  Winderust closes the shared handle. Windows applies its ordinary power policy
  to accepted requests.
- A confirmed App Suspension freeze withdraws that exact instance's request.
  Other runnable instances still contribute their system and display requests.
  The next complete observation after thaw can restore an eligible request.
  CPU Limiter's duty cycles do not affect Sleep Prevention.
- Accepted Windows requests are reported separately from app observations and
  do not prove effective inhibition. Missing observations cannot establish new
  requests or renew existing counts; explicit rule removal still releases them.
  Normal reconciliation never renews an already accepted system request to defeat
  Windows policy or Modern Standby battery expiry.
- Suspend/resume releases requests and suppresses the observed exact instances
  until they exit or the user explicitly disables/re-enables their rules or the
  feature. Suspend/resume takes precedence over saved settings first observed
  in the same worker pass. Power-source changes preserve this suppression.
  Winderust can preserve a quick disable and re-enable between worker passes
  while Windows event coverage remains continuous. Winderust retains up to four
  intermediate saves per worker pass. Overflow or a session or watcher
  transition discards that history and the worker evaluates the latest saved
  configuration normally.
  User sleep, lid actions, critical-battery handling, and Windows policy remain
  authoritative.
  A missing or stopped Windows event watcher blocks new requests, releases owned
  requests, and suppresses the current cohort until explicitly rearmed. A stop
  and restart between worker passes also releases an accepted request and
  suppresses its apps. Settings observed in the same pass keep that suppression.
- Running-app reconciliation remains at one second in visible and tray modes.
  Windows suspend/resume and session notifications wake it independently of the UI.
  Path enrichment covers enabled rule candidates and processes that can still
  match an accepted or suppressed instance, including an unknown creation time.

## Timer Resolution

- Timer Resolution does not use process failure suppression.
- `src/control/timer_resolution.rs` is the sole Timer Resolution lifecycle owner;
  `src/platform/windows/timer_resolution.rs` is the sole raw WinMM adapter. The feature manager
  owns foreground-rule policy and reporting only; every successful begin is paired with the exact
  end period on replacement, disable, or shutdown. This process-lifetime state has no crash
  journal.
- When the saved power profiles differ in whether Timer Resolution has a usable
  rule, an idle worker uses power-source notifications. If event coverage is
  unavailable, it rechecks the source once a second. While Windows cannot verify
  the source and the saved foreground policies differ, Timer Resolution releases
  its request, pauses new requests, and rechecks the source once a second.
- Timer Resolution releases its owned request before refreshing capability bounds.
  A failed release keeps Timer Resolution scheduled while it owns the request.
  Status shows Paused when an enabled rule needs a foreground observation
  that is unavailable. A known nonmatch stays Waiting, and an active release or
  capability error takes precedence. The Action Log records a repeated
  identical release error once until the request is renewed or released.

## Dynamic Resource Zones

- CPU Pressure Restraint, Limit Background Processors, and Dynamic Resource Zones are independent Adaptive Engine producers. Zones-only work must keep AC/Battery automation alive without enabling priority/efficiency assists.
- The limiter owns its existing percentage, selector, custom mask, and soft/hard method. `dynamic_resource_zone_settings` owns the separate zone share, selector, and custom mask. Speed defaults zoning off; Performance retains zoning on.
- Zones require fresh eligible foreground and hot background competition outside startup grace. One combined Adaptive generation applies background first, requires verified effective placement, then applies foreground. Loss of competition withdraws foreground immediately on reconciliation; partial failure resolves to the independently eligible limiter or no allocation, retaining controller cleanup/backoff.
- When enabled Adaptive Engine tuning or its ordered By Running App exclusion selectors differ between
  A/C and battery, unknown power source pauses Adaptive Engine's process policies and releases
  only their CPU placement and priority claims. Restoration keeps the controllers' existing retry
  cadence. On reconfiguration or source recovery, Adaptive yields one pass if a pending By Running
  App observation can change its exclusion. By Running App still observes after manual stops.
  Confirmed source recovery starts fresh pressure qualification. System Bottleneck and Adaptive
  Processor Power keep their separate observation and ownership rules.
- The existing CPU allocation coordinator remains the only owner of baselines and recovery. CPU Sets (Soft), Processor Affinity (Hard), Instance Balancing, and CPU Limiter all take precedence over zoning. Unsupported/multiple processor groups or invalid/empty partitions make zoning unavailable, never a silent All fallback.
- Disabled zoning may deserialize a missing zone block into inactive defaults without rewriting input. Enabled zoning with a missing block fails at deserialization for root, Battery, and presets. Do not infer zone values from the limiter or add a migration.

## Thread Priority discovery

- Automatic Thread Priority reconciliation uses one lazy PID-indexed Toolhelp inventory per pass, including cached capture/enumeration failure. Discovery failure stops the pass without process failure suppression. Process List mutations and priority queries capture fresh operation-local inventories. A grouped priority query shares its inventory across the selected processes. No inventory survives on the controller.
- Discovery is not mutation authorization: exact process/thread checks, priority readback, preservation, baselines and recovery remain controller-owned. Missing entries require exact exit/replacement verification before journal removal; live or uncertain identities remain owned. Release and shutdown never require discovery.
