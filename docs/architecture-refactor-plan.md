# Winderust Full Architecture Refactor Plan

- Status: Approved implementation contract
- Date: 2026-08-10
- Scope: Full internal architecture; preserve current product behavior, safety barriers, settings schema, and UI
- Implementation status: Phases 0, 1, and 2 completed on 2026-08-10; Phases 3 through 6 completed on 2026-08-11

## 1. Requirements summary

Winderust needs an architecture that can add process policies and Windows controls without adding another refresh loop, duplicating process discovery, expanding WinderustApp, or giving another feature independent ownership of the same Windows property.

The target is a mechanism-centered modular monolith:

- One external runtime owner for temporary automatic Windows state and reversible Process List actions.
- Several typed internal controllers, each owning a coherent Windows mechanism.
- Existing feature modules retain policy, hysteresis, cooldown, failure suppression, and user-facing status.
- One event-and-deadline worker remains dormant when no runtime work exists.
- Process and window observations are reused within a reconciliation pass.
- Persisted settings, temporary managed state, process-lifetime requests, persistent system configuration, and irreversible commands have different owners and lifecycle rules.
- Migration occurs through complete mechanism cutovers. A Windows property never has a legacy owner and a new owner in the same shipped state.

This is a behavior-preserving architecture refactor. Process List priority/CPU actions remain one-shot operations that later automation may supersede; a persistent manual-override mode would require a visible release affordance and is outside this refactor. Where today's automatic result is execution-order-dependent (notably Memory Priority versus Workload Engine and Core Limiter versus Workload Engine), Phase 0 records the deterministic target rule as an explicit approval item. Those clarifications are product decisions, not incidental refactor behavior.

The refactor must not:

- redesign pages, navigation, motion, wording, or localization;
- change feature defaults or retune Adaptive Engine presets;
- add settings aliases, migrations, or another storage location;
- weaken process identity, access, protected-process, cross-session, restoration, or failure-handling barriers;
- add an async runtime, actor framework, dependency-injection container, plugin system, or permanent worker thread without measured need;
- move code merely to make the directory tree look cleaner.

## 2. Repository evidence and current pressure points

The current system has strong safety behavior but concentrated and overlapping ownership:

- WinderustApp combines settings drafts, saved settings, dashboard telemetry, ordinary visible-mode power decisions, background status mirroring, Process List queries and quick-action restoration, tray state, updates, and animation state (src/ui/app.rs:405).
- Ordinary power-plan decisions and application run through the GPUI tick when visible (src/ui/app/runtime.rs:88, src/ui/app/runtime.rs:337), while HiddenAutomationRunner performs equivalent checks when hidden (src/backend/automation/runner.rs:672).
- run_background_automation owns many independent deadlines, event invalidation lists, feature requirement checks, execution order, publication, and sleep selection in one procedural loop (src/backend/automation.rs:418).
- HiddenAutomationRunner composes every stateful manager plus power-plan, telemetry, failure, Action Log, and restoration state (src/backend/automation/runner.rs:59).
- Process discovery is called independently by Background Efficiency, App Suspension, CPU controls, Priority controls, By Running App, Memory Trim, and Workload Engine. Visible-window and foreground observations are also repeated (representative calls: src/features/winderust_features/background_efficiency.rs:208, src/features/priority_control/process_priority.rs:128, src/features/winderust_features/workload_engine.rs:381).
- Process Priority, Background Efficiency, Workload Engine, and Process List quick actions can all affect process priority class (src/features/priority_control/process_priority.rs:47, src/features/winderust_features/background_efficiency.rs:621, src/features/winderust_features/workload_engine.rs:148, src/ui/app/pages/process_list_page.rs:1379).
- Background Efficiency and Workload Engine both affect power throttling, and Efficiency Mode is a compound priority-plus-power-throttling operation with compensating rollback (src/features/winderust_features/background_efficiency.rs:938).
- CPU Sets (Soft), Processor Affinity (Hard), Core Limiter, and Workload Engine share CPU allocation mechanisms and currently coordinate through execution order, exclusions, and adjusted-process sets (src/backend/automation/runner.rs:302, src/backend/automation/runner.rs:338, src/features/winderust_features/workload_engine.rs:817).
- Current precedence is mechanism-specific rather than one uniform feature order: Background Efficiency excludes its targets from regular Process Priority; Adaptive settings replace static Thread/I/O/GPU/Dynamic Priority Boost settings; explicit CPU allocation excludes Workload Engine; and regular Memory Priority currently runs after the Workload Engine path without an explicit exclusion.
- Core Limiter does not currently exclude Workload Engine hard-affinity targets, and Process List reversible actions mutate directly without a runtime claim. Both are ownership gaps that Phase 0 must characterize before choosing target semantics.
- Reversible setters call the crash-recovery journal from many feature modules, while Process List keeps a separate UI-owned stack of restoration closures (src/backend/crash_recovery.rs:373, src/ui/app.rs:1054).
- Runtime-generated auto-exclusions return to WinderustApp, mutate its settings value, and call the normal settings save path (src/ui/app/runtime.rs:555, src/ui/app/runtime.rs:668).
- The handwritten runtime-settings comparison omits Process Priority, Thread Priority, and Dynamic Priority Boost, so revisioned domain invalidation must replace it rather than wrap it.
- The Process List already has a distinct asynchronous read path for enumeration, icon loading, and resource sampling (src/ui/app/process_refresh.rs:140).
- Clean shutdown currently spans a UI restoration stack, manager-local restoration state, a hard-coded runner shutdown order, and finally recovery-helper pipe closure. The target typed controllers must replace these overlapping temporary-state authorities without changing the helper's final crash-recovery role.

These are scaling risks, not reasons to discard working feature managers or pure policies such as rules::decision_engine and workload_engine::policy.

## 3. Architectural decision

Adopt one RuntimeHandle and one RuntimeCore lifecycle, backed by typed mechanism controllers.

“One runtime” means one external authority and one coordinated worker lifecycle. It does not mean one struct owns settings persistence, every feature policy, every Windows adapter, every status field, and every UI query.

The runtime is a composition root:

~~~text
GPUI / WinderustApp
├── UI models and SettingsDraft
├── ProcessListQuery ───────────────────────> read-only Windows inventory
└── application lifecycle
    ├── SettingsCoordinator
    ├── typed persistent-system commands
    └── RuntimeHandle
          │
          ▼
      RuntimeCore — one event-and-deadline worker
      ├── RefreshScheduler
      ├── CycleObservations
      ├── feature policies and stateful managers
      ├── typed mechanism controllers
      │   ├── typed process-property controllers
      │   ├── EfficiencyMode transaction coordinator
      │   ├── CpuAllocationCoordinator
      │   ├── PowerPlanController
      │   ├── SuspensionController
      │   ├── TimerResolutionController
      │   └── SelfPowerController
      ├── RecoveryClient ───────────────────> Winderust crash helper
      ├── ActionLog
      └── segmented published read models
~~~

No second automation process is introduced. The recovery helper remains the existing second process of the same Winderust executable.

## 4. Architecture principles

1. **One external runtime authority.** UI code sends intent and renders published state. It does not own automatic power switching or temporary process restoration.
2. **Typed mechanism ownership.** Shared owner metadata is allowed, but desired values and reconciliation APIs remain typed by mechanism. There is no public ResourceKey plus untyped ControlValue pair.
3. **Complete resource cutovers.** All producers of one Windows property move together, including Adaptive Engine and Process List actions.
4. **Policy is separate from control.** Features decide targets and values; controllers own baselines, applied values, claims, recovery, and release.
5. **Observations inform but never authorize.** Cached process data may select a target, but every mutation reopens and revalidates exact identity and access.
6. **Hybrid scheduling remains.** Windows and input events mark dirty domains; deadlines handle polling, hysteresis, retries, and unsupported event cases.
7. **Different state classes have different lifecycles.** Temporary managed state is not treated like persistent configuration or irreversible commands.
8. **No new framework layer without evidence.** Concrete types and direct calls are preferred; common traits or generics are introduced only after repeated implementations prove the contract.
9. **Explicit shutdown is primary.** Drop remains a fallback, while the application lifecycle explicitly stops intake, joins work, releases state, and then closes the recovery pipe.
10. **Physical moves follow behavioral boundaries.** Establish ownership and tests before moving entire module families.

## 5. Target module boundaries

The destination is intentionally coarse. Subdirectories are created only when a phase needs them; do not scaffold empty layers.

~~~text
src/
├── application/
│   ├── lifecycle.rs              startup, explicit shutdown, component order
│   ├── settings.rs               SettingsCoordinator and SettingsDraft revision contract
│   └── system_commands.rs        typed persistent configuration commands
├── runtime/
│   ├── handle.rs                 UI-facing typed facade
│   ├── worker.rs                 RuntimeCore composition and reconciliation loop
│   ├── scheduler.rs              dirty domains, deadlines, dormancy, next wake
│   ├── observations.rs           per-pass observation cache and observation sources
│   └── published.rs              segmented generation-based read models
├── control/
│   ├── owner.rs                  shared owner identity and audit reason only
│   ├── process.rs                typed process-property controllers and shared private lifecycle utilities
│   ├── thread_priority.rs
│   ├── cpu_allocation.rs         CPU Sets/Affinity mutual-exclusion coordinator
│   ├── power_plan.rs
│   ├── suspension.rs
│   ├── timer_resolution.rs
│   └── self_power.rs
├── features/                     current UI-named feature policy and state
├── queries/
│   └── process_list.rs           read-only Process List query DTOs
├── platform/windows/             mechanism-focused Win32/NT/D3DKMT adapters
├── recovery/                     helper protocol, client, and recovery executor
└── ui/                           GPUI shell, pages, models, and visual state
~~~

Allowed dependency direction:

~~~text
ui -> application / RuntimeHandle / published read models / queries
application -> config / runtime / typed system-command adapters
runtime -> features / control / observations / ActionLog
features -> config / rules / observation types / typed claim APIs
control -> platform/windows / recovery
queries -> platform/windows read adapters
recovery -> platform/windows recovery adapters
platform/windows -> windows and windows-sys
~~~

Feature policy modules must not import GPUI or raw Windows mutation APIs after their mechanism is converted. UI modules may still use Windows types needed for the desktop shell, tray, dialogs, and window handles; the prohibition is specifically on raw managed process and automatic power mutation APIs.

## 6. State ownership contracts

### 6.1 Application lifecycle

The application lifecycle path owns startup and shutdown ordering. This may remain explicit orchestration in main/application::lifecycle; a new coordinator object is not required:

1. Detect recovery-helper mode.
2. Acquire the portable-copy-scoped single-instance guard.
3. Load settings from the executable directory.
4. Initialize RecoveryClient.
5. Start RuntimeHandle only when current settings or an explicit command require work.
6. Start GPUI and UI services.
7. On quit, stop command intake and event sources.
8. Ask RuntimeCore to release temporary managed state and join its worker.
9. Release process-lifetime requests and Winderust self-power state.
10. Close the recovery pipe and wait. Any journal entries left after failed clean restoration are recovered by the helper on pipe closure.

Normal shutdown must not depend solely on WinderustApp::drop or field drop order.

### 6.2 SettingsCoordinator

SettingsCoordinator is the only owner of the persisted Settings value and atomic write sequence. It preserves the existing TOML schema and executable-adjacent path in src/config/storage.rs. It is serialized by the application event path; RuntimeCore emits typed patch requests and never writes settings from its worker.

It publishes:

- Arc<Settings> plus a monotonic in-memory revision;
- typed save results;
- typed runtime-generated settings patches.

The UI owns SettingsDraft { base_revision, value }. A user save replaces the persisted value only through SettingsCoordinator. Auto-exclusions are idempotent patches such as “enable this exclusion” or “disable this exact rule,” not replacement Settings values.

If an auto-exclusion arrives while a draft exists:

- apply and atomically save the patch against the current persisted revision;
- apply the same narrow patch to the draft;
- preserve unrelated unsaved fields;
- let the safety patch win only when the user draft edits that same feature/path;
- advance the draft base revision after the persisted value and draft have received the same patch;
- publish the new revision and save result.

User saves and runtime patches are handled in one deterministic order. A save carries its draft base revision. A stale or gapped draft returns a typed conflict, retains the user's edited value, and consumes the missing narrow patch events before retry; it never replaces a newer committed value.

This prevents runtime safety feedback from silently saving unrelated UI draft changes.

### 6.3 RuntimeHandle

Evolve BackgroundAutomation into the migration facade instead of introducing a parallel facade with the same responsibility. Rename it only after the old role is gone.

RuntimeHandle exposes typed methods. A private internal command enum is allowed, but UI and features do not match on a global command enum.

Representative operations:

- replace_settings(revision, Arc<Settings>);
- apply_process_action(request);
- suspend_process and resume_process;
- terminate_process and terminate_process_tree;
- trim_memory_now;
- clear_action_log;
- request_refresh;
- shutdown.

Coalescible state uses shared slots plus generation counters and dirty flags. User commands requiring results use a bounded queue and one-shot result channel. No command blocks the GPUI thread on Win32 work.

### 6.4 RuntimeCore and RefreshScheduler

RuntimeCore owns one reconciliation worker and composes feature managers and controllers. It does not implement feature policy itself.

RefreshScheduler owns:

- typed feature/domain deadlines;
- event-to-dirty-domain mapping;
- settings invalidation;
- process-appearance invalidation;
- Workload Engine fast windows;
- retry and cooldown deadlines;
- worker dormancy and next-wake calculation.

Callbacks only coalesce dirty flags and wake the worker. They do not run policy or Windows mutations. The current conditional worker and event-watcher lifecycle must remain: no enabled work means no polling worker.

### 6.5 CycleObservations

CycleObservations is a per-reconciliation-pass cache, not a permanent global snapshot and not the Process List table model.

It lazily or demand-builds typed domains:

- process catalog and stable identity metadata;
- focused process and executable group;
- visible-window process and executable groups;
- active audio processes;
- CPU and per-processor usage;
- I/O, memory, and network observations;
- user activity and input state;
- power source, active plan, and effective power mode;
- session and processor topology.

Each requested domain is collected at most once in one runtime pass. A domain result distinguishes Available, Unavailable(error), and NotRequested. Feature-specific fail-closed behavior remains explicit.

Stateful delta samplers remain in ObservationSources or their owning feature manager. The observation cache does not erase cadence, sample history, hysteresis, or errors.

The Process List read query remains independent because it has different fields, cadence, asynchronous icon loading, and UI history. Sharing a lower-level process catalog across UI and runtime is optional and must be justified by release-build measurements.

### 6.6 Feature policies and managers

Keep concrete feature managers. They may own:

- timers, cooldowns, hysteresis, and candidate scoring;
- feature-specific target grouping and exclusions;
- Workload Engine pressure and topology state;
- App Suspension wake/user-intent policy;
- failure suppression and auto-exclusion decisions;
- feature status and Action Log semantics.

After conversion they do not own:

- the pre-Winderust baseline for a controlled Windows property;
- the currently applied Windows value as an independent restoration authority;
- crash-recovery Begin/Commit/Cancel calls;
- raw Win32 setters for converted mechanisms.

They submit typed claims or validated commands and consume typed outcomes. No common FeatureController trait is required until at least three converted families have the same proven lifecycle.

### 6.7 Typed control layer

The control layer is a set of cohesive controllers, not a required facade, universal arbiter, or central resource registry. RuntimeCore's worker is the sole mutator of controller state; UI commands and settings changes enter through RuntimeHandle, so the controller layer needs no internal actor system or cross-controller lock graph.

Each property has a concrete typed controller keyed by ProcessIdentity: priority class, power throttling, Dynamic Priority Boost, Thread Priority, I/O Priority, GPU Priority, and Memory Priority. ProcessIdentity is never a PID alone: it carries the captured creation time and verified executable identity needed to reject PID reuse, while executable names and paths remain policy/grouping inputs rather than authorization. CpuAllocationCoordinator owns both CPU Sets and affinity because their effective results constrain each other. PowerPlanController and SuspensionController remain separate because their identities and recovery contracts differ.

Public methods remain property-specific. A private generic ClaimSet<T> helper is acceptable only for repeated owner-selection bookkeeping inside typed controllers; query, validation, application, verification, recovery encoding, status, and errors remain mechanism-specific. There is no public heterogeneous process-property map.

Each managed property has:

- a captured original baseline;
- the last verified Winderust-applied value;
- active owner claims;
- the effective owner and value;
- a mechanism-specific precedence function;
- a mutation/application sequence used for reverse clean restoration.

There is no single global precedence number. Only owners valid for the same property compete. Phase 0 freezes current precedence and replacement semantics before conversion. The target arbitration tables start from these known rules:

| Property family | Required semantics |
| --- | --- |
| Process priority | Preserve Background Efficiency and Workload Engine exclusion behavior relative to regular Process Priority; define the coupled Efficiency Mode result with power throttling. |
| Thread, I/O, GPU, and Dynamic Priority Boost | Adaptive effective settings replace the corresponding static policy while Adaptive ownership is active; they are not merely higher numeric claims. |
| Memory priority | Replace execution-order wins with an explicit rule covering Memory Priority, Workload Engine, and Process List. |
| CPU allocation | Explicit CPU allocation outranks Workload Engine, while the coordinator prevents incompatible CPU Sets and hard affinity. |
| Process List priority/CPU actions | Apply immediately through the typed controller and remain in its baseline/expected chain for restoration, but do not become persistent highest-priority claims. The next applicable automatic reconciliation may supersede them, matching today's one-shot UX without inventing a hidden override. |

Any departure from a deterministic current rule is recorded as a product decision, not smuggled into controller plumbing.

CpuAllocationCoordinator prevents an incompatible hard-affinity and soft-CPU-Set result from being applied to the same process and preserves explicit CPU allocation precedence over Workload Engine.

PowerPlanController, SuspensionController, TimerResolutionController, and SelfPowerController have separate typed state because their lifecycle and recovery contracts differ materially from ordinary process properties.

### 6.8 Compound managed effects

Efficiency Mode is a compound claim spanning power throttling and priority class. CPU allocation also has cross-property constraints.

Compound effects use a compensating transaction:

1. Resolve the complete claim bundle and ensure it wins every required property.
2. Reopen and validate the target identity and required access.
3. Capture every changing baseline before the first mutation.
4. Journal each externally persistent property before its corresponding mutation.
5. Apply in a documented deterministic order.
6. If a later application or verification fails, compensate already-applied properties in reverse order.
7. Retain recovery entries until compensation is verified.
8. Publish success only when the complete compound invariant is observed.

This is not claimed to be OS-atomic. Crash recovery guarantees that a crash between steps unwinds every acknowledged journal entry.

### 6.9 RecoveryClient and live controller state

Typed controllers' live state and the watchdog journal have deliberately different authority:

- Each typed controller is authoritative for its active claims, captured baselines, expected values, clean reconciliation, and clean release.
- The watchdog journal is the external crash mirror of applied transitions.
- Feature managers do not keep a third independent restoration chain after cutover.

Clean shutdown follows an explicit dependency order (stop intake, stop producers, thaw suspension, release process controls, restore automatic power, release process-lifetime requests, then close the helper pipe). Within a typed controller, resources are released in reverse successful-application order and only while the observed state matches Winderust's expected chain. Cross-property compensation is owned by the specific compound operation, not a universal value registry.

For externally persistent reversible state:

1. Reopen and revalidate exact process/thread identity.
2. Check built-in, critical, protected, session, and operation-specific access policy.
3. Query the current value.
4. Refuse mutation if the original state cannot be preserved.
5. Send Begin and wait for helper acknowledgement.
6. Apply through a mechanism-specific Windows adapter.
7. Verify the expected value.
8. Commit on success or compensate/cancel on failure.
9. Restore only while the observed value matches Winderust's expected chain.

App Suspension retains its special rule: the helper must hold the Job Object handle before freeze acknowledgement.

Timer Resolution and Winderust self-power requests are process-lifetime state. They use explicit clean release, while Windows process termination is the crash boundary; no unnecessary external journal entry is added.

### 6.10 Published read models and queries

Do not replace AutomationStatusSnapshot with a larger monolithic snapshot containing every UI concern.

Start from the current AutomationStatusSnapshot, ProcessPolicySummary, ProcessListRenderData, and Action Log boundaries. Publish a small outer generation plus independently shared Arc segments only where ownership or update cadence is already distinct. Do not create a segment type merely to mirror a page or field group; settings patch/save results remain events rather than a permanent runtime snapshot.

Generation changes only when semantic content changes. The UI may retain local dashboard history, navigation state, drafts, animations, update state, Process List sorting/grouping, icons, and resource sample history.

ProcessListQuery returns read-only DTOs. Process List mutations use RuntimeHandle and report typed per-target outcomes, including partial batch failure.

### 6.11 Persistent configuration and irreversible commands

Not every Windows write is temporary managed state.

Classify operations before routing:

| Class | Examples | Owner | Restoration |
| --- | --- | --- | --- |
| Temporary externally persistent | process priority, EcoQoS, CPU Sets, affinity, power plans, suspended jobs | typed mechanism controller | clean restoration plus watchdog |
| Process-lifetime | Timer Resolution, Winderust self-power state | lifecycle controller | clean release plus Windows process cleanup |
| Intentional persistent configuration | startup registration, Advanced Power Plan Tuning, Win32 Priority Separation | typed application services | user-requested persistence; no watchdog rollback |
| Irreversible command | stop process/tree, Memory Trim | validated command service | impossible; report accurately |

Persistent configuration services still use typed errors, explicit user intent, and narrow Windows adapters. They must not be forced through the temporary-state claim ledger.

## 7. Resource ownership matrix

This matrix defines migration boundaries. “Current producers” includes policy sources even when the current code combines them by rewriting settings before one manager call.

| Controlled mechanism | Current producers | Target owner | Lifecycle class | Cutover group |
| --- | --- | --- | --- | --- |
| Automatic active power plan | ordinary decision engine in visible/hidden modes; Adaptive Engine managed plan | PowerPlanController | journaled reversible | Power |
| Process priority class | Process Priority; Background Efficiency; Workload Engine; Process List | PriorityClassController | journaled reversible | Priority + Efficiency |
| Process power throttling | Background Efficiency; Workload Engine; Process List Efficiency Mode | PowerThrottlingController | journaled reversible | Priority + Efficiency |
| Dynamic priority boost | Dynamic Priority Boost static/Adaptive policies; Process List | typed boost controller | journaled reversible | Pilot |
| Thread priority | Thread Priority static/Adaptive policies; Process List | ThreadPriorityController | journaled reversible | Priority family |
| I/O priority | I/O Priority static/Adaptive policies; Process List | typed I/O controller | journaled reversible | Priority family |
| GPU priority | GPU Priority static/Adaptive policies; Process List | typed GPU controller | journaled reversible | Priority family |
| Memory priority | Memory Priority; Workload Engine; Process List | typed memory controller | journaled reversible | Priority family |
| Default CPU Sets | CPU Sets (Soft); Workload Engine soft restriction | CpuAllocationCoordinator | journaled reversible | CPU allocation |
| Process affinity | Processor Affinity (Hard); Core Limiter; Workload Engine hard restriction | CpuAllocationCoordinator | journaled reversible | CPU allocation |
| Suspended Job Object | App Suspension rules; Process List manual action | SuspensionController | helper-held reversible | Suspension |
| Timer Resolution | Timer Resolution foreground rules | TimerResolutionController | process-lifetime | Advanced controls |
| Winderust priority/EcoQoS | hidden-to-tray and Adaptive Engine self-power policy | SelfPowerController | process-lifetime | Lifecycle |
| Process termination | Process List stop/tree | validated process command service | irreversible | Commands |
| Working-set trim | Memory Trim automatic/manual | validated memory command service | irreversible | Commands |
| Startup/registry/power tuning | settings save; Win32 Priority Separation; Advanced Power Plan Tuning | typed application services | persistent user configuration | Application/UI |

The matrix must be regenerated from source and locked by tests in Phase 0. If a producer is missing, its mechanism cannot begin conversion.

## 8. Primary flows after refactor

### 8.1 User settings save

~~~text
SettingsDraft(base revision)
  -> SettingsCoordinator validates current revision
  -> atomic settings.toml write beside executable
  -> publish Arc<Settings> + new revision
  -> RuntimeHandle replaces saved runtime settings
  -> scheduler dirties affected domains
  -> reconcile
  -> publish changed read-model segments
~~~

### 8.2 Runtime auto-exclusion

~~~text
feature failure suppression
  -> typed AutoExclusionPatch
  -> SettingsCoordinator applies patch to persisted settings
  -> same narrow patch merges into UI draft
  -> atomic save + new revision
  -> runtime reconciles against updated settings
  -> UI shows the exclusion and save result
~~~

### 8.3 Event or deadline reconciliation

~~~text
WinEvent / input / power / session callback OR scheduler deadline
  -> coalesce dirty domains and wake worker
  -> scheduler selects due feature policies
  -> CycleObservations collects each requested domain at most once
  -> feature managers update state and submit typed claims/commands
  -> typed controllers resolve per-mechanism owners and constraints
  -> recovery-aware adapters reconcile Windows state
  -> Action Log and changed read-model segments publish
  -> scheduler parks or exits the worker when no work remains
~~~

### 8.4 Process List query and action

~~~text
Process List visible
  -> ProcessListQuery runs asynchronously
  -> UI owns icons, sorting, grouping and resource history

User action
  -> capture ProcessActionTarget
  -> RuntimeHandle sends typed single/batch command
  -> runtime reopens and revalidates exact identity
  -> reversible one-shot action reconciles through the typed controller
     and later automation may supersede it
     OR irreversible action uses validated command service
  -> typed result reports every attempted target
  -> query refresh shows observed Windows state
~~~

### 8.5 Power automation

~~~text
RuntimeCore in visible or hidden UI state
  -> collect activity/foreground/running-app/CPU/time inputs
  -> ordinary decision engine selects its candidate
  -> Adaptive Engine selects managed-plan ownership when enabled
  -> PowerPlanController chooses the only valid owner
  -> journal and switch
  -> publish decision, owner and reason
~~~

Visibility changes observation cadence and UI work only; it never transfers power-plan ownership.

### 8.6 Clean shutdown and crash

~~~text
Clean:
stop intake -> stop event sources -> finish active pass
-> release claims in reverse applied order
-> restore original automatic power plan
-> delete only the identified Winderust Adaptive plan
-> release process-lifetime requests
-> join worker -> close recovery pipe
-> helper recovers any entries left by failed clean release

Crash/forced termination:
pipe closes -> helper folds pending intents into journal
-> reverse-unwind each matching identity/property chain
-> thaw helper-held Job Objects
-> leave externally changed state untouched
~~~

## 9. Migration strategy

Every phase is a separate PR or small PR series. Do not combine product changes, preset tuning, or visual redesign with architecture work.

The unit of reversible-control migration is one complete mechanism, not one page or feature. A mechanism PR includes every automatic producer, Adaptive Engine producer, Process List action, status path, recovery call, and raw setter for that property.

Legacy and target implementations may coexist in source only while the target controller is private and mutation-disabled. Before a property route switch, its cutover manifest must prove every producer and restoration path is accounted for, direct controller tests must pass, and the old path remains the sole runtime writer. The switch routes every producer at once; the old writer, UI restore closure, manager baseline, and direct recovery calls for that property then become unreachable and are deleted in the same PR. There is no live state transfer across application versions: startup recovery resolves any previous crashed instance before the new runtime captures fresh baselines.

Every mechanism route-switch PR must provide five concrete proofs:

1. Before switch: instrumented old behavior and precedence fixtures pass.
2. Before switch: the new controller passes direct identity, access, apply, verify, compensation, release, and crash-fault tests while unreachable from production producers. Its property-specific recovery matrix must cover `Begin`, apply, verify, `Commit`, replacement, and clean release.
3. At switch: every manifest producer changes route in the same PR.
4. After switch: source searches find one mutation adapter and zero legacy baselines, UI restore closures, or direct journal calls for that property.
5. Rollback: reverting the whole property PR restores the characterized old route; reverting an individual producer is forbidden.

### Phase 0 — Characterization, ownership inventory, and baselines

Primary files: existing tests under src/, src/backend/automation/tests.rs, src/backend/crash_recovery.rs, benchmark/.

Work:

- Generate the verified resource ownership matrix from every raw setter, current-value query, recovery call, quick action, and restore path.
- Produce a per-property cutover manifest covering producers, read paths, raw setters, UI actions, manager baselines, restore closures, journal variants, status fields, Action Log outcomes, and source-search patterns that must reach zero.
- Characterize exact current precedence for every shared property, including Background Efficiency versus Process Priority and CPU Sets versus affinity/Core Limiter/Workload Engine.
- Characterize visible/hidden power decisions, event invalidation, feature execution order, worker dormancy, process appearance, failure suppression, auto-exclusion persistence, and reverse restoration.
- Add fault-injection seams for the shared recovery protocol, exercise every recovery class with live or isolated Windows integration, and record the property-specific `Begin`/apply/verify/`Commit`/replacement/clean-release matrix required before each new controller's route switch. Do not add disposable injection plumbing to legacy setters that the complete mechanism cutover deletes.
- Record the current Settings TOML semantic round trip.
- Record release-build idle CPU, working set, thread count, wake frequency, process/window scan counts, and event-to-action latency.
- Lock Process List one-shot/supersession behavior and auto-exclusion/draft merge behavior in tests before their conversion phase.
- Record a pre-mortem for each critical cutover: likely mixed-owner failure, detection signal, rollback switch, and the exact condition that forbids route activation.

Exit gate:

- Every current resource producer is present in the matrix.
- Every temporary property has a reviewed cutover manifest and zero-writer search criteria.
- Every shared-property precedence rule has a deterministic test.
- Live or isolated Windows integration tests cover each recovery class; pure journal compaction tests alone are insufficient.
- Baseline artifact exists under benchmark/results/architecture-baseline-<date>.json.
- No production behavior changes.

Rollback: tests and instrumentation only.

### Phase 1 — Explicit lifecycle, settings authority, and migration facade

Completion evidence: [phase-1-application-runtime-foundation.md](architecture/phase-1-application-runtime-foundation.md)

Primary files: src/main.rs, src/config/storage.rs, src/ui/app/settings_io.rs, src/ui/app/runtime.rs, src/backend/automation.rs, src/backend/crash_recovery.rs; new src/application/ as justified.

Work:

- Make startup/shutdown ordering explicit in the application lifecycle and return an explicit runtime shutdown result; do not add a coordinator type unless ownership requires it.
- Wrap the existing recovery protocol with RecoveryClient without changing its wire contract.
- Introduce SettingsCoordinator, in-memory revision, SettingsDraft base revision, and typed auto-exclusion patches.
- Replace the handwritten runtime-settings equality predicate with revisioned section/domain invalidation; cover Process Priority, Thread Priority, and Dynamic Priority Boost explicitly.
- Audit every settings page and Process List rule editor: UI code may mutate SettingsDraft, but every save/import/runtime patch reaches persisted Settings only through SettingsCoordinator.
- Evolve BackgroundAutomation's existing shared-state/Condvar boundary into the RuntimeHandle migration facade; do not add a second equivalent facade.
- Publish current automation status through segmented Arc adapters while preserving the old UI fields temporarily.
- Keep the current worker, feature managers, and scheduling behavior.

Exit gate:

- Startup and shutdown order is deterministic and tested.
- Failed clean restoration leaves journal entries for helper recovery.
- Auto-exclusion patches preserve unrelated unsaved draft edits and current schema serialization.
- Source search finds one persisted-settings write boundary; pages and Process List editors mutate only SettingsDraft or submit typed patches.
- Existing runtime methods and new facade adapters produce equivalent results.
- Default settings still create no polling worker.

Rollback: route UI calls through the existing BackgroundAutomation methods and remove the new coordinator adapters.

### Phase 2 — Extract scheduler and per-pass observations

Primary files: src/backend/automation.rs, requirements.rs, wake.rs, runner.rs; new src/runtime/scheduler.rs and observations.rs.

Work:

- Replace local next_* variables and repeated invalidation arrays with typed scheduler entries.
- Preserve every existing interval, Workload Engine fast path, retry, suspended-process release cadence, and no-work exit.
- Add CycleObservations and convert process appearance, foreground, and visible-window collection first.
- Convert other observation domains only when two due consumers currently duplicate work.
- Keep ProcessListQuery separate.

Exit gate:

- Scheduler tests cover settings, foreground, window, power, session, input, process appearance, manual requests, retries, hidden/visible cadence, and dormancy.
- Instrumentation proves each requested process/window domain is collected at most once per runtime pass.
- Target sets and unavailable/fail-closed behavior match Phase 0 fixtures.
- Idle wake rate is no higher than baseline +5%.

Rollback: scheduler delegates to the old deadline logic; observation accessors delegate to existing collectors.

### Phase 3 — Centralize power and event-source ownership

Completion evidence: [phase-3-power-events.md](architecture/phase-3-power-events.md).

Primary files: src/ui/app/runtime.rs, src/ui/app/tray_state.rs, src/backend/automation/runner.rs, src/activity/input_hook.rs, src/rules/decision_engine.rs, src/power/, src/backend/self_power.rs.

Work:

- Introduce PowerPlanController and move visible ordinary decisions into RuntimeCore.
- Model ordinary automation and Adaptive Engine managed plans as mutually exclusive typed owners.
- Keep the existing decision engine and exact page/rule-owned plan selections.
- Move input/event-source lifecycle coordination out of WinderustApp while preserving any Win32 thread requirements.
- Introduce SelfPowerController for hidden/Adaptive Winderust process-lifetime state.
- Remove UI apply_decision only after parity.

Exit gate:

- Identical inputs produce identical power decisions while visible and hidden.
- Hiding/restoring the window does not transfer ownership, reset hysteresis, or create a switch.
- Original automatic power plan and managed Adaptive plan recovery pass the crash matrix.
- Only the current Winderust Adaptive name and description qualify for cleanup.
- No automatic power-plan mutation remains in src/ui/.

Rollback: one switch returns ordinary plan application to the characterized UI adapter; never leave both paths active.

### Phase 4 — Typed process-control foundation and Dynamic Priority Boost pilot

Completion evidence: [phase-4-dynamic-priority-boost.md](architecture/phase-4-dynamic-priority-boost.md).

Primary files: src/features/priority_control/dynamic_priority_boost.rs, src/ui/app/pages/process_list_page.rs, src/backend/crash_recovery.rs; new src/control/process.rs and mechanism Windows adapter.

Work:

- Introduce ControlOwner metadata and the first typed property claim set.
- Store baseline, expected value, claims, effective owner, and apply sequence in the typed boost controller.
- Route static policy, Adaptive Engine effective policy, and Process List Dynamic Priority Boost actions through the same typed controller.
- Route or delete the dormant Workload Engine Dynamic Priority Boost setter; no unused mutation capability remains behind the controller.
- Centralize identity/access validation and RecoveryClient sequencing for this property.
- Remove every legacy Dynamic Priority Boost setter and feature-owned restore record in the same cutover.
- Keep the new controller mutation-disabled outside direct tests until the complete route switch; no release build contains two active boost writers.

Exit gate:

- Source search finds exactly one mutation adapter for Dynamic Priority Boost.
- Manual override, rule, Adaptive policy, external state break, disable, process exit, clean shutdown, and crash tests pass.
- PID reuse and inaccessible/protected/cross-session targets fail closed.
- No duplicate restoration authority remains for the pilot property.

Rollback: revert the complete Dynamic Priority Boost slice; do not retain mixed legacy/new ownership.

### Phase 5 — Convert priority-related mechanisms by complete property

Completion evidence:

- Mechanism 1: [phase-5-thread-priority.md](architecture/phase-5-thread-priority.md).
- Mechanism 2: [phase-5-io-priority.md](architecture/phase-5-io-priority.md).
- Mechanism 3: [phase-5-gpu-priority.md](architecture/phase-5-gpu-priority.md).
- Mechanism 4: [phase-5-memory-priority.md](architecture/phase-5-memory-priority.md).
- Mechanism 5: [phase-5-priority-efficiency.md](architecture/phase-5-priority-efficiency.md).

Primary files: src/features/priority_control/, src/features/winderust_features/background_efficiency.rs, workload_engine.rs, workload_engine/process_control.rs, Process List quick actions.

Order, one independently gated mechanism PR each:

1. Thread Priority, including static/Adaptive policy and Process List. **Complete.**
2. I/O Priority, including static/Adaptive policy and Process List. **Complete.**
3. GPU Priority, including static/Adaptive policy and Process List. **Complete.**
4. Memory Priority, including Memory Priority, Workload Engine, and Process List. **Complete.**
5. Coupled process priority plus power throttling, including Process Priority, Background Efficiency, Workload Engine, Efficiency Mode, Process List, and compound compensation. **Complete.**

Work for every property:

- Convert feature managers from mutation ownership to typed claims and outcomes.
- Preserve exact target grouping, Focus App > Visible Window > Background behavior, custom rules, exclusions, failure suppression, and Action Log semantics.
- Move query/apply/verify code into the mechanism adapter.
- Cut over every producer and quick action before deleting the legacy path.
- Remove direct recovery calls and property baselines from converted feature managers.

Exit gate per property:

- The ownership inventory proves every producer uses the typed controller.
- Apply, unchanged, replacement, mismatch, external break, disable, process exit, access denied, suppression, auto-exclusion, clean shutdown, and crash tests pass.
- Process List batches attempt every captured target and report partial failure accurately.
- Efficiency Mode publishes enabled only when its complete priority-plus-power-throttling invariant is observed.
- No converted policy imports a raw mutation API.

Rollback: revert one complete property slice, never one producer inside it.

### Phase 6 — Convert CPU allocation as one constrained family

Completion evidence: [phase-6-cpu-allocation.md](architecture/phase-6-cpu-allocation.md)

Primary files: src/features/cpu_control/cpu_allocation.rs, core_limiter.rs, src/features/winderust_features/workload_engine.rs, src/cpu.rs; new control/cpu_allocation.rs.

Work:

- Introduce CpuAllocationCoordinator with typed CPU Sets and affinity claim sets plus their mutual-exclusion constraint.
- Convert CPU Sets (Soft), Processor Affinity (Hard), Core Limiter, and Workload Engine soft/hard allocation together.
- Preserve explicit CPU allocation precedence over Workload Engine.
- Preserve topology discovery, load-aware selection, saturation relaxation, hysteresis, and first-processor-group disclosure.
- Keep CPU Sets (Soft) and Processor Affinity (Hard) as separate product/settings/page owners.

Exit gate:

- No process receives incompatible effective CPU Sets and affinity claims.
- Every CPU allocation producer routes through CpuAllocationCoordinator.
- Hybrid and all-P topology suites pass.
- PID reuse, process exit, external change, clean release, and crash restoration pass for both Windows properties.
- Corrected CPU/I/O/MessageLoop benchmark integrity gates remain valid with no preset retuning.

Rollback: revert the complete CPU allocation family to its characterized managers.

### Phase 7 — Convert suspension, process-lifetime controls, and commands

Primary files: src/features/advanced_controls/app_suspension.rs,
src/control/suspension.rs, timer_resolution.rs, src/foreground/process_list.rs,
src/features/winderust_features/memory_trim.rs, Process List actions.

Work:

- Route automatic and manual App Suspension through SuspensionController.
- Preserve helper Job Object handle transfer before freeze acknowledgement.
- Move Timer Resolution behind its process-lifetime controller without external journal entries. Reuse the SelfPowerController completed in Phase 3; do not introduce a second self-power state owner.
- Route termination and Memory Trim through typed validated commands, not claim or recovery types.
- Preserve App Suspension network/audio/user-intent wake policy and unavailable-target UX.

Progress (2026-08-11):

- Memory Trim now uses a typed result-bearing command and `MemoryTrimController`; the old
  coalesced request flag and feature-owned setter are removed.
- Stop Process and Stop Process Tree now use `ProcessTerminationController`; complete-batch
  preflight is mutation-free, execution continues after individual failures, and GPUI waits on a
  background executor. Stop Tree retains exact selected roots across confirmation, rejects root
  instance changes, and follows only timestamp-valid parent/child edges before command submission.
- Timer Resolution now uses `TimerResolutionController` as its sole WinMM request owner. The
  foreground-rule manager is policy/reporting-only, request switching releases the exact prior
  period first, and explicit shutdown retains Drop only as a backstop. No recovery journal was
  introduced for process-lifetime state.
- App Suspension now uses `SuspensionController` as the sole normal Job Object state and mutation
  owner. Automatic rules, App-page Freeze, and Process List Suspend/Resume share typed RuntimeCore
  routes; exact-instance acquisition, explicit transaction states, pass-driven bounded cleanup
  retry, exact policy-record pruning, and aggregate shutdown replace the feature-owned freezer map
  and fire-and-forget queues.
- Crash recovery thaws the helper-held exact named job even after its recorded root exits, preserving
  descendant recovery. The controller and crash mirror are the only remaining
  `SetInformationJobObject` locations.

Exit gate:

- Session 0, service-account, curated host, critical, protected, inaccessible, and cross-session safety matrices pass.
- Forced main-process termination thaws every helper-held job.
- Timer Resolution and self-power state release cleanly and Windows cleanup is documented/tested for forced exit.
- Irreversible commands never appear in recovery entries and never claim restoration.
- Stop/tree behavior and grouped-target partial results remain unchanged.

Rollback: revert one lifecycle class at a time; App Suspension remains an indivisible automatic/manual slice.

### Phase 8 — Complete application/query boundaries and decompose WinderustApp

Status: implementation complete on 2026-08-12. See
[`architecture/phase-8-application-query-boundaries.md`](architecture/phase-8-application-query-boundaries.md).

Primary files: src/ui/app.rs and operational modules, src/ui/app/process_refresh.rs, src/ui/app/pages/, src/backend/startup.rs, src/backend/win_registry.rs, Advanced Power Plan Tuning helpers.

Work:

- Keep one GPUI WinderustApp entity as presentation composition root.
- Extract plain ShellModel, SettingsEditor, DashboardModel, ProcessListModel, UpdateModel, and AppearanceModel only where behavior moves with state.
- Replace mirrored individual runtime fields with segmented Arc read models.
- Keep Process List query, icons, grouping, sorting, and history on the read/UI side.
- Route startup registration, Advanced Power Plan Tuning, and Win32 Priority Separation through typed persistent application services.
- Keep tray, update checking, dialogs, and visual motion outside RuntimeCore.

Completed boundary:

- `SettingsEditor` owns draft/revision/persistence and typed startup reconciliation. Typed
  application services own Win32 Priority Separation and Advanced Power Plan Tuning persistence.
- `WinderustApp` composes `ShellModel`, `DashboardModel`, `ProcessCatalogModel`,
  `ProcessListModel`, and `UpdateModel`; runtime feature status is one semantically stable Arc
  segment rather than fifteen mirrored fields.
- Sampling, queries, icons, GPUI timers, tray, dialogs, focus, and visual motion remain UI-side.
  `AppearanceModel` was not added because no independent behavior would move with it.

Exit gate:

- WinderustApp owns GPUI composition, render dispatch, subscriptions, drafts, and visual state—not automation, recovery chains, or managed Windows state.
- Process List query and interaction tests pass without placing row/icon state in RuntimeCore.
- Settings save/revert/import/export and auto-patch merge tests pass.
- Navigation, localization, keyboard/focus, animation, and screenshots remain unchanged.

Rollback: extract one plain model or persistent command family per PR.

### Phase 9 — Consolidate Windows and recovery boundaries

Primary files: current Win32 code in src/backend/, src/foreground/, src/power/, and converted feature modules; new platform/windows/ and recovery/ paths as justified.

Status: complete (2026-08-12). Evidence:
[`architecture/phase-9-windows-recovery-boundaries.md`](architecture/phase-9-windows-recovery-boundaries.md).

Work:

- Move mechanism implementations after their callers already use typed boundaries.
- Keep safe wrappers narrow, unsafe blocks explicit, GetLastError immediate, handles RAII-owned, and SAFETY comments local.
- Reuse adapters between main runtime and recovery helper where their access/identity contract is actually identical.
- Update .agents/memory/30-reference-library.md for moved or changed compatibility-sensitive boundaries.
- Add dependency checks preventing policy/UI code from importing managed mutation APIs.

Progress (2026-08-12):

- Timer Resolution retains lifecycle ownership in `TimerResolutionController`, while its existing
  platform contract and all raw WinMM declarations/calls now live in the mechanism-focused
  `src/platform/windows/timer_resolution.rs` adapter. Feature policy remains independent of both
  raw WinMM and request ownership.
- The ownership gate requires exactly one Timer Resolution platform contract and begin/end call,
  rejects raw WinMM from controller and feature policy, and preserves its process-lifetime,
  no-journal classification.
- Memory Trim and process termination keep exact-target validation, sampling/batch orchestration,
  and typed outcomes in their controllers. Their raw process APIs now live in separate
  mechanism-focused Windows adapters, sharing only typed Win32 failure classification.
- Ownership gates require exactly one working-set trim and termination call, reject unsafe/raw
  Win32 access from the command controllers, and retain the irreversible no-recovery contract.
- Shared process-control acquisition now separates typed target/identity/safety validation in
  `src/control/process.rs` from operation-specific minimal access masks and raw `OpenProcess` in
  `src/platform/windows/process.rs`. App Suspension retains its synchronize-first access fallback.
- Dynamic Priority Boost keeps its single controller-owned recovery transaction and baseline chain;
  only the live `GetProcessPriorityBoost` / `SetProcessPriorityBoost` calls moved to a narrow
  Windows adapter. The crash executor remains the independent replay mirror.
- Memory Priority likewise retains multi-owner arbitration, unknown raw baselines, and its recovery
  transaction in `MemoryPriorityController`; raw class constants and the live query/set pair moved
  to `src/platform/windows/memory_priority.rs`.
- GPU Priority retains its exact-process baseline, preservation, recovery transaction, and
  pending-context semantics in `GpuPriorityController`; D3DKMT calls and NTSTATUS classification
  moved to `src/platform/windows/gpu_priority.rs`.
- I/O Priority retains unknown raw baselines, preservation, recovery, and clean release in
  `IoPriorityController`; its NT declarations, numeric information class, calls, and status
  classification moved to `src/platform/windows/io_priority.rs`.
- Process Priority and process Power Throttling retain one compound arbitration, compensation,
  recovery, and restoration boundary in `PriorityEfficiencyController`; their raw class constants,
  state conversion, query calls, and sole live writes moved to
  `src/platform/windows/priority_efficiency.rs`.
- Thread Priority retains exact process/thread identity, baselines, recovery sequencing, and clean
  release in `ThreadPriorityController`; Toolhelp enumeration, thread acquisition/identity reads,
  priority constants, and live query/set calls moved to
  `src/platform/windows/thread_priority.rs`.
- CPU allocation retains precedence, mutual exclusion, handoff/retry state, exact baselines,
  recovery sequencing, and restoration in `CpuAllocationCoordinator`; affinity/CPU Set query and
  write calls plus packed system CPU Set topology conversion moved to
  `src/platform/windows/cpu_allocation.rs`.
- App Suspension retains exact acquisition policy, named-job lifecycle, transaction/retry state,
  recovery handoff, and clean release in `SuspensionController`; raw Job Object creation,
  membership, assignment, freeze/thaw, and the shared undocumented layout moved to
  `src/platform/windows/suspension.rs`.
- Winderust self-power retains strict baseline capture, hidden/Adaptive composition, verified
  compound transition, compensation, retry, and shutdown in `SelfPowerController`; current-process
  priority and Power Throttling calls moved to `src/platform/windows/self_power.rs` without adding
  crash-recovery ownership.
- Power-plan control retains automatic baselines/recovery in `PowerPlanController`, managed-plan
  recognition and staged processor-value semantics in `src/power/powercfg.rs`, and persistent
  command ownership in its application service. GUID conversion, native power-scheme calls, raw
  processor-value reads/writes, and effective-mode callback registration moved to
  `src/platform/windows/power_plan.rs`.

Exit gate:

- Raw managed mutation APIs are reachable only through typed platform adapters.
- Recovery protocol/executor is independent from UI and feature policy.
- No unsafe block grows merely because code moved.
- Windows integration and unsafe-op Clippy checks pass.

Rollback: move one already-converted mechanism family at a time.

### Phase 10 — Delete migration paths and freeze the architecture

Status and evidence:
[`architecture/phase-10-architecture-freeze.md`](architecture/phase-10-architecture-freeze.md).
Complete as of 2026-08-12. Source freeze, automated validation, the standard isolated
footprint/latency benchmark, and the optimized Windows UI smoke/idle check all passed.

Work:

- Remove legacy deadlines, direct feature setters, UI restoration closures, duplicate status representations, and unused adapters.
- Rename BackgroundAutomation/HiddenAutomationRunner only when their roles match RuntimeHandle/RuntimeCore.
- Do not add aliases for the removed internal names.
- Update source map, DESIGN.md, contributor rules, Graphify, and architecture diagrams.

Exit gate:

- No migration adapter remains without a named owner, test, and deletion issue.
- A source audit finds one live controller for every temporary managed property.
- Full validation, release build, crash matrix, UI smoke test, and performance budgets pass.
- The final dependency graph has no UI-to-managed-mutation or policy-to-Win32 mutation path.

Rollback: do not enter until every adapter has been unused for one complete prior phase and verified by search/tests.

## 10. Verification strategy

### 10.1 Unit tests

- Scheduler invalidation, deadline ordering, coalescing, fast windows, retry, and dormancy.
- CycleObservations one-read caching and Available/Unavailable/NotRequested behavior.
- Per-property claim precedence, replacement, release, and external-state break.
- Compound Efficiency Mode winner selection, compensation, and verification.
- CPU Sets/affinity mutual exclusion.
- Settings revision, user save, auto-patch merge, collision, and atomic-save failure.
- Decision-engine and Workload Engine policy parity.
- Feature failure suppression and Action Log semantics.
- Plain UI model transitions.

### 10.2 Windows integration tests

- Live priority, power throttling, CPU Sets, affinity, dynamic boost, thread, I/O, GPU, memory, power plan, Timer Resolution, and Job Object round trips.
- Process identity replacement/PID reuse rejection.
- Built-in, critical, protected, inaccessible, same-session, cross-session, Session 0, and service-account matrix.
- Focused and visible-window executable-group targeting.
- Process List single/group action result reporting.
- Persistent configuration commands write only after explicit user action.

### 10.3 Recovery and shutdown matrix

For every externally persistent reversible property, terminate the main process:

1. before Begin acknowledgement;
2. after Begin acknowledgement but before mutation;
3. after mutation but before verification;
4. after verification but before Commit;
5. after Commit;
6. while replacing one owner with another;
7. during compound-operation compensation;
8. during clean shutdown release.

Verify exact identity, original/expected chains, reverse ordering, external-state breaks, helper exit result, and no mutation when acknowledgement is unavailable.

For process-lifetime controls, verify explicit clean release and forced-process-exit cleanup separately.

### 10.4 UI and end-to-end tests

- Startup, minimize, tray hide/restore, and quit.
- Settings edit/save/revert/import/export with concurrent auto-exclusion.
- Visible/hidden power parity.
- Process List grouped/child actions and observed state refresh.
- Advanced-control disabled/unavailable states.
- Action Log filter/export.
- Screenshot comparison for Home, Process List, Adaptive Engine, and representative settings pages.

### 10.5 Performance budgets

Compare release builds on the same machine, settings, and power scheme:

- Idle CPU median: no more than 5% relative or 0.1 percentage points above baseline, whichever is larger.
- Working set after ten idle minutes: no more than 10% or 10 MiB above baseline, whichever is larger.
- Permanent thread count: no unexplained increase; each added thread requires an ownership and stack-cost justification.
- Idle wake frequency: no more than 5% above baseline.
- Foreground/window event-to-action P95: no more than 50 ms slower than baseline.
- A requested process or visible-window domain is collected at most once per runtime reconciliation pass.
- Workload Engine cadence and corrected benchmark integrity gates remain unchanged.

### 10.6 Required checks per source phase

~~~powershell
git diff --check
cargo fmt -- --check
cargo clippy --locked --all-targets -- -D warnings -D unsafe-op-in-unsafe-fn
cargo test --locked
~~~

Also run the legacy naming scan from .agents/memory/10-development-guide.md, build the release binary at mechanism-family boundaries, complete the relevant Windows smoke tests, and run graphify update . after source changes.

Documentation-only revisions require git diff --check and link/file-reference validation; they do not require a release build.

## 11. Risks and mitigations

| Risk | Impact | Mitigation |
| --- | --- | --- |
| A single RuntimeCore becomes a new god object | High | RuntimeCore only composes scheduler, observations, policies, controllers, and publishers; logic stays with typed owners. |
| A universal resource/value enum becomes runtime-typed plumbing | High | Shared owner metadata only; public claim and apply APIs remain mechanism-typed. |
| Two mutation authorities coexist during migration | Critical | Cut over all producers by mechanism with one route switch; assertion and source scan block mixed ownership. |
| Resource-centric PRs become too large | High | Use internal preparatory commits/adapters, but ship/merge only after the complete property uses one controller. |
| Compound Efficiency Mode leaves partial state | Critical | Bundle winner rule, deterministic application, verification, reverse compensation, and crash injection. |
| Live ledger and watchdog journal diverge | Critical | RecoveryClient is the only journal writer; controller transitions have paired ledger/journal tests. |
| Settings auto-patch overwrites user draft | High | Revisioned persisted owner plus narrow idempotent patches merged into draft. |
| Event loss delays safety or responsiveness | High | Coalesced dirty flags plus generations; no reliance on queue multiplicity; latency tests. |
| Cached process data becomes stale | High | Cache selects candidates only; every mutation reopens and revalidates identity/access. |
| Deadlock between UI, worker, settings, and watchdog | Critical | One-way commands; no UI callback or settings write while controller locks are held; document lock order; bounded result channels. |
| Shared observations worsen footprint | Medium | Per-pass lazy cache, not permanent inventory; enforce release CPU/memory/wake budgets. |
| Process List becomes coupled to automation | Medium | Keep independent query/read model; share only measured lower-level adapters. |
| Platform directory becomes a dumping ground | Medium | Organize by Windows mechanism and move only converted code with stable callers. |
| Physical moves damage unsafe invariants | Critical | Move after behavioral conversion, one mechanism at a time, with unchanged small unsafe blocks and Windows tests. |
| Feature work conflicts with the migration | Medium | New work uses the target boundary for converted mechanisms and does not add another legacy owner. |

## 12. Acceptance criteria

The refactor is complete only when all are true:

- [x] WinderustApp owns presentation, drafts, and UI queries, with no automatic power mutation or temporary process restoration stack.
- [x] RuntimeHandle is the only UI entry point for temporary managed controls and validated process commands.
- [x] RuntimeCore is the one visible/hidden automation lifecycle owner and remains a composition root rather than a policy/controller implementation.
- [x] Every temporary controlled property has exactly one typed controller and one mechanism-specific Windows mutation adapter.
- [x] Every producer of a property—including Adaptive Engine and Process List—is cut over together.
- [x] No public untyped ResourceKey/ControlValue mutation API exists.
- [x] Feature managers retain policy state and failure behavior but no longer own baselines, recovery calls, or raw setters for converted properties.
- [x] Efficiency Mode is successful only when its complete priority-plus-power-throttling state is verified, with tested compensation.
- [x] CPU Sets and affinity constraints prevent incompatible simultaneous effective ownership.
- [x] Externally persistent reversible state receives synchronous helper acknowledgement before mutation and restores across clean shutdown and forced termination.
- [x] Process-lifetime controls use explicit clean release and documented Windows cleanup instead of invented journal entries.
- [x] Persistent configuration and irreversible commands are classified separately and never claim crash restoration.
- [x] Every mutation revalidates target identity, access, protection, session policy, and current expected state.
- [x] One runtime reconciliation pass performs at most one requested process/window observation per domain.
- [x] Process List table/icon/history state remains outside RuntimeCore.
- [x] Published runtime state is segmented and generation-based; semantic no-change does not notify the UI.
- [x] SettingsEditor is the only persisted-settings boundary and auto-exclusion patches preserve unrelated drafts.
- [x] Current settings.toml files deserialize and serialize with the current schema, path, and no aliases/migrations.
- [x] Visible and hidden power behavior is identical for identical observations.
- [x] Worker dormancy, CPU, memory, thread count, wake rate, and latency satisfy the budgets.
- [x] UI navigation, text, localization, accessibility, motion, and representative screenshots have no unintended change.
- [x] No raw managed mutation API is imported by UI or converted feature policy modules.
- [x] Formatting, Clippy, tests, naming scan, release build, Windows smoke tests, crash matrix, and Graphify all pass.

## 13. Architectural decision record

### Decision

Use a staged, mechanism-centered modular-monolith refactor with one external RuntimeHandle/RuntimeCore lifecycle, per-pass shared observations, feature-owned policy state, typed mechanism controllers, segmented read models, revisioned settings ownership, and the existing external recovery-helper model.

### Drivers

1. A Windows property must have one restoration authority even when several Winderust features influence it.
2. Crash recovery, identity validation, and conservative access policy must remain provable throughout migration.
3. Runtime footprint and responsiveness must not regress while duplicate observation work is removed.

### Alternatives considered

#### Big-bang rewrite

Advantages: reaches a clean tree quickly. Rejected because behavior and crash parity cannot be proved incrementally, and rollback would be all-or-nothing.

#### Original feature-centered RuntimeService with universal resource arbitration

Advantages: uniform commands, resources, and snapshots; simple conceptual diagram. Rejected because current properties cross feature boundaries, feature-by-feature conversion permits dual owners, and universal resource/value/snapshot types would concentrate coupling in new god objects.

#### Limited WinderustApp and scheduler extraction

Advantages: smaller change and immediate readability improvement. Rejected as the final architecture because direct UI mutations, scattered recovery calls, duplicated observation, and implicit cross-feature property ownership remain.

#### Mechanism-centered modular monolith

Advantages: one owner per Windows property, typed compile-time boundaries, complete per-property rollback, no framework dependency, and preservation of stateful feature policy. Selected because it addresses the actual ownership graph while keeping one efficient process and one coordinated automation worker.

### Consequences

- Migration slices follow Windows mechanisms even though product modules continue to use visible Winderust feature names.
- Some mechanism cutovers touch several feature modules and Process List in one PR.
- Temporary adapters exist only before the route switch; mixed ownership is never a valid released state.
- Each typed controller becomes the clean-release authority for its mechanism, while the helper journal remains the crash authority.
- Settings changes generated by runtime safety logic become explicit revisioned patches.
- Process List reads remain independently optimized instead of becoming runtime status.

### Follow-ups

- Approve this target before Phase 0 production work.
- Create one tracking issue per phase and one checklist per mechanism cutover.
- Freeze unrelated architecture moves while a mechanism is between preparation and cutover.
- Revisit common traits only after at least three typed controllers prove identical contracts.
- Update .agents/memory/30-reference-library.md whenever a compatibility-sensitive Windows boundary changes.

## 14. Execution order and stop rules

Start with Phase 0 only. Phase 1 is blocked until the ownership matrix, current precedence tests, live recovery seams, settings fixtures, and performance baseline exist.

Stop and restore the last complete mechanism route when a phase:

- cannot enumerate every producer of the property being converted;
- leaves a legacy setter and new controller active for the same property;
- cannot reproduce current target selection or precedence;
- weakens identity, access, protected-process, cross-session, watchdog, or restoration behavior;
- mutates before preserving the original state or receiving required helper acknowledgement;
- restores across an external-state break;
- changes the persisted schema or portable storage contract;
- causes an unexplained permanent thread, wake-rate, memory, CPU, or latency regression;
- requires a product/UI redesign or preset retune to proceed;
- or cannot roll back as a complete property slice.

The current implementation remains the behavioral reference until the corresponding typed mechanism clears its exit gate.
