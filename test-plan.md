# PowerLeaf / Windows Power Control App — Implementation Plan

## 0. Current-Code Migration Adjustment

This document is an architecture and migration plan, not a literal test plan yet. The
current codebase already has feature-specific managers coordinated by
`src/automation.rs`, while this plan describes the target shared rule/action model.

Migration should happen one slice at a time:

```text
1. Add shared rule/action data model without changing runtime behavior.
2. Add conflict groups and priority tests around the shared model.
3. Adapt power-plan decisions into generic rules/actions.
4. Adapt per-app resource controls into generic actions.
5. Move restore/recovery into an app-wide applied-action store.
6. Retire direct manager-to-Windows changes only after equivalent resolver coverage exists.
```

The current features that must be represented in the target model include:

```text
- Power plan switching
- Activity / idle rules
- Foreground app rules
- Running app performance mode
- Schedule rules
- CPU usage rules
- EcoQoS / Efficiency Mode
- App suspension
- CPU affinity / core steering
- Background CPU restriction
- CPU limiter
- Foreground responsiveness
- Watchdog rules
```

Foreground responsiveness belongs in App Resource Control because it changes process
priority and background CPU pressure. Watchdog belongs in its own action category, but
it still needs shared priority, cooldown, protected-process checks, and action logging.

Updated target priority order:

```text
1000 Manual Override
900  Safety / Critical Protection
850  Watchdog Rules
825  Foreground Responsiveness
800  Focused App Rules
700  Running App Rules
600  App Background Rules
500  CPU Load Rules
400  Activity / Idle Rules
300  Schedule Rules
100  Default Fallback
```

Migration slices implemented in code:

```text
src/rules/model.rs
src/rules/resolver.rs
src/rules/power_plan_adapter.rs
```

These modules define the shared `Rule`, `Trigger`, `Action`, `ConflictGroup`, and
`PriorityResolver` types. The adapter mirrors current power-plan decisions into generic
`Action::SwitchPowerPlan` rules so resolver behavior can be compared before changing
runtime execution. `src/automation.rs` now performs a debug-only shadow equivalence
check between the existing `DecisionEngine` target and the generic resolver target.
Power-plan application now reads the target GUID from the generic adapter/resolver path
while preserving the existing `DecisionOutcome` source and retry behavior. The apply path
now consumes the generic `Action::SwitchPowerPlan` and extracts the GUID only at the
final power-plan manager call boundary.
Existing power-plan settings and `DecisionInput` can also be mirrored into active
generic rules with `active_power_plan_rules_for_context`, allowing debug comparison
between the current `DecisionEngine` and the generic resolver before replacing the
engine.
Power-plan application now uses the generic context adapter as the production source
for `Action::SwitchPowerPlan`; `DecisionEngine` remains in `run_check` only as a
debug equivalence comparator and UI/reason compatibility bridge.
`ActionExecutor` now provides the generic execution boundary for power-plan actions;
`automation.rs` uses it with a cached power-plan backend before calling the Windows
power manager.
Foreground Responsiveness now has a pure app-resource adapter that mirrors configured
priority rules, foreground boost, and auto-balance policy into generic actions, and
runtime priority changes now route through the shared executor boundary.
`ActionExecutor` also supports generic app-priority actions through
`AppPriorityActionBackend`, covering `SetAppPriority` and `BoostForegroundPriority`
with idempotency and backend-error tests.
Watchdog settings now have a pure adapter into generic lifecycle rules/actions:
`TerminateApp` for launch blocking and `RestartApp` for missing-process restart rules.
CPU affinity, background CPU restriction, and app suspension settings now also have
pure adapters into generic app-resource actions, and their runtime apply paths now
route through `ActionExecutor` backends.
Core Steering generic rules now preserve hard affinity masks, soft CPU-set masks, and
Efficiency Mode Off as distinct `AffinityPolicy` variants instead of collapsing every
mode into a hard custom mask.
Core Steering runtime application now routes each selected process through generic
`Action::SetAppAffinity` and `ActionExecutor` using a Core Steering app-resource
backend, while preserving the existing PID filtering, restore store, skip handling,
and action logging.
Core Limiter runtime application now routes sustained per-process limits through
generic `Action::SetAppCpuLimit` and `ActionExecutor` using a Core Limiter
app-resource backend, while preserving the existing tracking, cooldown, restore, and
Core Steering exclusion behavior.
App Suspension runtime freeze application now routes background freezes through
generic `Action::SuspendApp` and `ActionExecutor` using an App Suspension
app-resource backend, while preserving the existing foreground, manual freeze,
temporary thaw, network wake, audio wake, restore, and release paths.
Foreground Responsiveness background priority, auto-balance priority, and foreground
boost application now route through generic `SetAppPriority` / `BoostForegroundPriority`
actions and `ActionExecutor` using Foreground Responsiveness priority backends, while
preserving existing process selection, EcoQoS exclusions, stability delay,
auto-balance tracking, restore state, and action logging.
`active_app_resource_rules_for_settings` aggregates migrated app-resource adapters so
`PriorityResolver` can evaluate cross-feature app conflicts in one place.
Shared runtime-state types now exist for detector output and restore tracking:
`RuntimeState`, `DetectorEvent`, `AppliedAction`, and `PreviousValue`.
`AppliedActionStore` tracks applied actions by conflict group and can identify actions
that are obsolete relative to a desired resolved-action set.
`RuleEngine` now ties active rules, `PriorityResolver`, and `AppliedActionStore`
together for desired-action and restore planning.
The app-wide `AppliedActionStore` now tracks resolved app-resource and lifecycle
actions from the shared app-resource rule evaluation, preserving power-plan records
while replacing stale app-resource conflict groups with the current resolver output.
`automation.rs` now performs a shadow app-resource evaluation from live settings/context
through the combined app-resource adapter and `RuleEngine`; feature managers still
own selection, safety, and concrete backend state, but their mutating apply paths now
cross the shared executor boundary.
That shadow evaluation now runs when any app-resource/process feature refresh is due,
not only during foreground responsiveness updates.
EcoQoS/Efficiency Mode is represented as a generic background efficiency policy action,
including enabled exclusions, efficiency-core preference, and optional CPU restriction
percentage.
EcoQoS runtime application now routes each selected process through generic
`Action::SetAppEfficiencyMode` and `ActionExecutor` using an EcoQoS app-resource
backend, while preserving foreground/session/exclusion filtering, failure suppression,
CPU-set sync, restore paths, and action logging.
CPU limiter settings now map per-process sustained-load rules to generic
`SetAppCpuLimit` actions with cooldown metadata.
`ActionExecutor` now supports lifecycle actions through `AppLifecycleActionBackend`,
covering generic `TerminateApp` and `RestartApp` execution paths.
Watchdog lifecycle execution now creates generic `TerminateApp` / `RestartApp` actions
and routes them through `ActionExecutor`; terminate-on-launch still keeps the existing
PID-specific safety selection before executor dispatch.
`ActionExecutor` now also supports generic app-resource actions through
`AppResourceActionBackend`: affinity, per-app CPU limits, suspend/resume, and
background efficiency policy.
`ActionExecutor::apply_action` now dispatches modeled actions through a unified
`GenericActionBackend`, giving future runtime code one executor entry point.
All current `Action` variants now have an executor route, including core parking,
system CPU limit, per-app efficiency mode, lower-background priority policy, and
auto-balance policy.
Power-plan runtime now records applied generic power-plan actions in
`AppliedActionStore`, including previous active-plan GUID when available.
Legacy `DecisionEngine` comparison is now debug-only; release power-plan selection runs
from the generic context resolver.
Power-plan runtime now evaluates active power-plan rules through `RuleEngine` with
`AppliedActionStore` before applying the selected generic `Action::SwitchPowerPlan`.
The visible-window power-plan apply path also routes `Action::SwitchPowerPlan`
through `ActionExecutor`, so foreground UI and hidden automation use the same
power-plan execution boundary.
Manual Core Parking / processor-power tuning now routes through generic
`Action::SetProcessorPowerValues` and `ActionExecutor` using a UI power-plan backend,
preserving separate AC and battery processor values instead of calling the Windows
power backend directly from the UI.
`HiddenAutomationRunner` now maintains shadow `RuntimeState` updates for foreground
app, CPU load, user idle time, and active schedule IDs from existing detector outputs.
The app-resource shadow evaluation now feeds resolved generic rule IDs into
`RuntimeState.active_rules`, making the central runtime state reflect generic resolver
output before those app-resource managers are fully executor-driven.
Generic versioned config destination types now exist: `GenericAppConfig`,
`AppResourcePolicy`, `AppStatePolicy`, and `PowerPlanProfile`.

## 1. Core Design Direction

The app should be implemented around this architecture:

```text
Detector Layer
    ↓
State Store
    ↓
Rule Engine
    ↓
Priority Resolver
    ↓
Action Executor
    ↓
Windows Backend
```

Do **not** let each feature directly change the system.

Instead:

```text
Feature detects condition → emits state → rule engine evaluates → executor applies changes
```

This prevents conflicts between:

```text
Schedule says Power Saver
Foreground app says Performance
CPU load rule says Balanced
App background rule says Efficiency Mode
System limiter says restrict CPU
```

---

## 2. Main App Modules

Recommended Rust module layout:

```text
src/
├─ app/
│  ├─ main_window.rs
│  ├─ dashboard.rs
│  ├─ rules_page.rs
│  ├─ power_plans_page.rs
│  ├─ cpu_control_page.rs
│  ├─ app_control_page.rs
│  └─ settings_page.rs
│
├─ core/
│  ├─ engine.rs
│  ├─ state.rs
│  ├─ rule.rs
│  ├─ trigger.rs
│  ├─ action.rs
│  ├─ priority.rs
│  ├─ profile.rs
│  └─ restore.rs
│
├─ detectors/
│  ├─ foreground.rs
│  ├─ process.rs
│  ├─ cpu_load.rs
│  ├─ input_idle.rs
│  └─ schedule.rs
│
├─ executor/
│  ├─ action_executor.rs
│  ├─ power_plan_executor.rs
│  ├─ app_resource_executor.rs
│  ├─ system_cpu_executor.rs
│  └─ restore_executor.rs
│
├─ windows/
│  ├─ power_plan.rs
│  ├─ process.rs
│  ├─ affinity.rs
│  ├─ efficiency_mode.rs
│  ├─ suspension.rs
│  ├─ cpu_info.rs
│  └─ idle.rs
│
├─ storage/
│  ├─ config.rs
│  ├─ rules_store.rs
│  └─ app_profiles_store.rs
│
└─ telemetry/
   ├─ event_log.rs
   └─ diagnostics.rs
```

GPUI should mostly handle presentation and user interaction.
The automation engine should be UI-independent.

---

## 3. Main Concepts

### 3.1 Trigger

A trigger describes **when** something should happen.

```rust
pub enum Trigger {
    AppFocused {
        app: AppMatcher,
    },
    AppRunning {
        app: AppMatcher,
    },
    AppBackground {
        app: AppMatcher,
        duration_secs: u64,
    },
    AppBackgroundIdle {
        app: AppMatcher,
        duration_secs: u64,
    },
    CpuLoadAbove {
        percent: f32,
        duration_secs: u64,
    },
    CpuLoadBelow {
        percent: f32,
        duration_secs: u64,
    },
    UserIdle {
        duration_secs: u64,
    },
    UserActive,
    Schedule {
        schedule_id: String,
    },
}
```

---

### 3.2 Action

An action describes **what** the app should change.

```rust
pub enum Action {
    SwitchPowerPlan {
        plan_guid: String,
    },
    SetCoreParking {
        plan_guid: String,
        min_cores_percent: u8,
        max_cores_percent: u8,
    },
    SetSystemCpuLimit {
        logical_processor_percent: u8,
    },
    SetAppEfficiencyMode {
        app: AppMatcher,
        enabled: bool,
    },
    SetAppPriority {
        app: AppMatcher,
        priority: ProcessPriority,
    },
    SetAppAffinity {
        app: AppMatcher,
        affinity: AffinityPolicy,
    },
    SetAppCpuLimit {
        app: AppMatcher,
        logical_processor_percent: u8,
    },
    SuspendApp {
        app: AppMatcher,
    },
    ResumeApp {
        app: AppMatcher,
    },
}
```

---

### 3.3 Rule

A rule connects triggers to actions.

```rust
pub struct Rule {
    pub id: RuleId,
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub trigger: Trigger,
    pub actions: Vec<Action>,
    pub restore_actions: Vec<Action>,
    pub cooldown_secs: u64,
}
```

Example:

```text
Rule:
When Chrome is background idle for 10 minutes

Actions:
- Enable Efficiency Mode
- Set priority to Low
- Limit to 50% logical processors

Restore:
- Disable Efficiency Mode
- Restore normal priority
- Restore full affinity
```

---

## 4. Rule Categories

Expose these categories in the UI:

```text
Rules
├─ App Rules
├─ CPU Load Rules
├─ Activity / Idle Rules
└─ Schedule Rules
```

Internally, they can all be the same `Rule` type.

---

## 5. Action Categories

Expose actions as grouped controls:

```text
Power Plan Control
├─ Switch Power Plan
└─ Core Parking

System CPU Control
└─ System CPU Limiter

App Resource Control
├─ Efficiency Mode
├─ Priority Control
├─ Background CPU Restriction
├─ Core Steering
└─ App Suspension
```

---

## 6. Priority Resolver

The resolver should decide which rule wins when multiple rules are active.

Recommended default priority order:

```text
1000 Manual Override
900  Safety / Critical Protection
800  Focused App Rules
700  Running App Rules
600  App Background Rules
500  CPU Load Rules
400  Activity / Idle Rules
300  Schedule Rules
100  Default Fallback
```

Example:

```text
Schedule Rule:
22:00–08:00 → Power Saver
Priority: 300

Focused App Rule:
Game.exe focused → High Performance
Priority: 800

Final result:
High Performance
```

For app resource actions, priority should be per-app.

Example:

```text
Discord background rule says:
Limit to 25% cores

Discord focused rule says:
Restore full cores

Focused rule wins.
```

---

## 7. State Store

Create a central runtime state.

```rust
pub struct RuntimeState {
    pub foreground_app: Option<ProcessInfo>,
    pub running_processes: Vec<ProcessInfo>,
    pub cpu_load_percent: f32,
    pub user_idle_secs: u64,
    pub active_schedule_ids: Vec<String>,
    pub active_rules: Vec<RuleId>,
    pub applied_actions: Vec<AppliedAction>,
}
```

The detectors only update this state.

They should not directly call Windows APIs that modify power plans, process priority, affinity, suspension, etc.

---

## 8. Detector Layer

### 8.1 Foreground App Detector

Purpose:

```text
Detect current focused window.
Map window → PID → process path/exe name.
```

Windows APIs likely needed:

```text
GetForegroundWindow
GetWindowThreadProcessId
OpenProcess
QueryFullProcessImageNameW
```

Output:

```rust
DetectorEvent::ForegroundChanged(ProcessInfo)
```

---

### 8.2 Running App Detector

Purpose:

```text
Track currently running processes.
```

Implementation options:

```text
Simple version:
- Poll process list every 2–5 seconds.

Advanced version:
- Use WMI or ETW later.
```

For first implementation, polling is fine.

Output:

```rust
DetectorEvent::ProcessListChanged(Vec<ProcessInfo>)
```

---

### 8.3 CPU Load Detector

Purpose:

```text
Track total CPU load.
Optionally track per-process CPU load later.
```

Use smoothing.

Do not react to instant CPU spikes.

Recommended behavior:

```text
Sample interval: 1s
Rolling window: 10s
Activation threshold: CPU > X for Y seconds
Restore threshold: CPU < Z for N seconds
```

Example:

```text
Activate limiter:
CPU > 85% for 30s

Restore:
CPU < 45% for 60s
```

---

### 8.4 Input / Idle Detector

Purpose:

```text
Detect user idle/active state.
```

Windows API:

```text
GetLastInputInfo
```

Output:

```rust
DetectorEvent::UserIdleChanged {
    idle_secs: u64,
}
```

---

### 8.5 Schedule Detector

Purpose:

```text
Detect whether current time matches schedule rules.
```

This can be evaluated inside the rule engine instead of being a separate thread.

Recommended:

```text
Evaluate schedules every 30–60 seconds.
```

---

## 9. Action Executor

The executor should be:

```text
Idempotent
Reversible when possible
Logged
Guarded by safety checks
```

Do not repeatedly apply the same action every tick.

Before applying:

```text
Check if current desired state already matches.
If yes, do nothing.
```

---

## 10. Power Plan Control

### 10.1 Switch Power Plan

Use a Windows backend function like:

```rust
pub trait PowerPlanBackend {
    fn list_power_plans(&self) -> Result<Vec<PowerPlan>>;
    fn get_active_power_plan(&self) -> Result<PowerPlanId>;
    fn set_active_power_plan(&self, id: &PowerPlanId) -> Result<()>;
}
```

Implementation options:

```text
Preferred:
- Windows Power Management API

Fallback:
- powercfg command wrapper
```

For production, prefer Windows API.
For quick implementation, `powercfg` is acceptable but less clean.

---

### 10.2 Core Parking

Core Parking should be implemented as **Power Plan Tuning**, not as a live detector.

Flow:

```text
User selects power plan
User adjusts core parking min/max
App writes setting to that plan
Optionally re-activates plan
```

Important:

```text
Do not silently mutate all power plans.
Only mutate the selected plan or explicitly chosen plans.
```

---

## 11. System CPU Limiter

Be careful with this feature.

Avoid doing this:

```text
Disable logical processors globally
Use bcdedit /numproc
Touch boot configuration
```

Recommended implementation:

```text
System CPU Limiter = policy layer that limits selected processes or applies power-plan processor constraints.
```

Possible approaches:

```text
Option A:
Apply affinity limits to user-selected apps.

Option B:
Apply affinity limits to all non-critical user processes.

Option C:
Use power plan processor settings to reduce CPU performance.

Option D:
Hybrid approach:
- Soft limit through power plan
- Hard limit through process affinity for selected apps
```

Recommended first version:

```text
Do not implement true global hard CPU limiting.
Implement App CPU Limits first.
Then add System CPU Limiter as an advanced feature.
```

---

## 12. App Resource Control

This should cover:

```text
Efficiency Mode
Priority
Affinity / Core Steering
Background CPU Restriction
App Suspension
```

Use one shared app policy model.

```rust
pub struct AppResourcePolicy {
    pub app: AppMatcher,

    pub foreground: AppStatePolicy,
    pub background: AppStatePolicy,
    pub background_idle: Option<AppStatePolicy>,
}

pub struct AppStatePolicy {
    pub efficiency_mode: Option<bool>,
    pub priority: Option<ProcessPriority>,
    pub affinity: Option<AffinityPolicy>,
    pub logical_processor_limit_percent: Option<u8>,
    pub suspend_after_secs: Option<u64>,
}
```

Example:

```text
Chrome policy:

Foreground:
- Efficiency Mode: Off
- Priority: Normal
- CPU Limit: All cores

Background:
- Efficiency Mode: On
- Priority: Low
- CPU Limit: 50%

Background idle for 10 minutes:
- Suspend: Enabled
```

---

## 13. Efficiency Mode

Efficiency Mode should be the default soft throttle.

Implementation behavior:

```text
When app enters background:
- Enable EcoQoS
- Optionally set priority to BelowNormal or Low

When app returns foreground:
- Disable EcoQoS
- Restore previous priority
```

Use a Windows backend abstraction:

```rust
pub trait EfficiencyModeBackend {
    fn set_efficiency_mode(&self, pid: u32, enabled: bool) -> Result<()>;
    fn set_priority(&self, pid: u32, priority: ProcessPriority) -> Result<()>;
}
```

Possible Windows APIs:

```text
OpenProcess
SetProcessInformation with PROCESS_POWER_THROTTLING_STATE
SetPriorityClass
```

Recommended priority options:

```rust
pub enum ProcessPriority {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
}
```

Default:

```text
Background → EcoQoS + BelowNormal
Aggressive → EcoQoS + Idle
```

Avoid defaulting to `Idle` priority for normal users.

---

## 14. Background CPU Restriction

Implement as app affinity / CPU-set policy.

Simple version:

```text
Limit process to N logical processors using SetProcessAffinityMask.
```

Better version later:

```text
Use CPU Sets / selected CPU Sets if you need better hybrid CPU behavior.
```

Recommended default presets:

```text
All logical processors
75%
50%
25%
Custom
```

Important:

```text
Store original affinity before changing it.
Restore original affinity when the app returns foreground or rule deactivates.
```

---

## 15. Core Steering

Core Steering should be part of App Resource Control.

Recommended UI:

```text
Core Steering
├─ Auto
├─ Prefer performance cores
├─ Prefer efficiency cores
├─ Use first N logical processors
└─ Custom mask
```

Implementation should depend on CPU type.

Detection:

```text
If CPU is hybrid:
- Show P-core / E-core options.

If CPU is non-hybrid:
- Hide P-core / E-core wording.
- Show logical processor limit only.
```

Fallback behavior:

```text
Prefer E-cores on non-hybrid CPU → map to lower logical processor count, not fake E-cores.
```

Do not expose impossible options.

---

## 16. App Suspension

App Suspension is aggressive. Treat it as advanced.

Recommended default:

```text
Disabled by default.
User must explicitly enable per-app.
```

Required safety checks before suspension:

```text
App is not foreground
App has been background for X seconds
App CPU usage is below threshold
No active audio
App is not on protected list
App is not a system process
App is not already suspended
```

Default values:

```text
Suspend after: 10 minutes background idle
CPU threshold: below 2%
```

Avoid:

```text
Suspend after 10 seconds
Suspend browsers by default
Suspend chat apps by default
Suspend anything with active audio
Suspend system processes
```

Recommended protected list:

```text
explorer.exe
dwm.exe
csrss.exe
winlogon.exe
services.exe
lsass.exe
svchost.exe
RuntimeBroker.exe
SearchHost.exe
ShellExperienceHost.exe
StartMenuExperienceHost.exe
audiodg.exe
```

Suspension backend:

```rust
pub trait SuspensionBackend {
    fn suspend_process(&self, pid: u32) -> Result<()>;
    fn resume_process(&self, pid: u32) -> Result<()>;
}
```

Keep suspension implementation isolated because some process suspension APIs are undocumented or risky.

---

## 17. Restore System

You need a restore system.

Any time the app changes something, record:

```text
PID
Process path
Changed setting
Previous value if available
Applied value
Rule ID
Timestamp
```

Example:

```rust
pub struct AppliedAction {
    pub rule_id: RuleId,
    pub target: ActionTarget,
    pub action: Action,
    pub previous: Option<PreviousValue>,
    pub applied_at: Instant,
}
```

On rule deactivation:

```text
Run restore action
or restore previous known value
```

On app exit:

```text
Restore all reversible actions.
```

Critical:

```text
If your app crashes, some changes may persist.
```

So add:

```text
Startup recovery:
- Load previous applied actions from disk.
- Check whether they are still relevant.
- Offer restore.
```

---

## 18. Conflict Handling

You need conflict groups.

Example:

```rust
pub enum ConflictGroup {
    PowerPlan,
    CoreParking,
    SystemCpuLimit,
    AppEfficiencyMode(ProcessIdentity),
    AppPriority(ProcessIdentity),
    AppAffinity(ProcessIdentity),
    AppSuspension(ProcessIdentity),
}
```

Only one action per conflict group should win.

Example:

```text
Rule A:
Chrome background → Low priority

Rule B:
Chrome focused → Normal priority

Conflict group:
AppPriority(Chrome)

Winner:
Higher priority active rule
```

---

## 19. Engine Loop

Recommended engine flow:

```text
Every 1 second:
1. Receive detector updates
2. Update RuntimeState
3. Evaluate rules
4. Resolve conflicts
5. Compare desired state vs applied state
6. Apply only required changes
7. Log changes
8. Notify GPUI state model
```

Pseudo-flow:

```rust
loop {
    let events = detector_bus.drain();

    runtime_state.apply_events(events);

    let matched_rules = rule_engine.evaluate(&runtime_state, &rules);

    let desired_actions = priority_resolver.resolve(matched_rules);

    let diff = desired_state.diff(desired_actions);

    action_executor.apply(diff)?;

    ui_event_bus.publish(AppEvent::RuntimeStateChanged);
}
```

---

## 20. GPUI Integration

GPUI should display state and allow configuration.

Recommended pages:

```text
Dashboard
Rules
Power Plans
CPU Control
App Control
Logs
Settings
```

### Dashboard

Show:

```text
Current Power Plan
Active Rules
Restricted Apps
Suspended Apps
CPU Load
User Idle Time
Last Applied Action
```

### Rules Page

Show:

```text
Rule list
Enable / disable
Priority
Trigger
Actions
Cooldown
```

### App Control Page

Show per-app policies:

```text
App
Foreground policy
Background policy
Background idle policy
Current status
```

### Logs Page

Show:

```text
Timestamp
Rule
Action
Target
Result
Error if any
```

This is important for trust.
Users need to know why the app changed something.

---

## 21. Storage

Use a versioned config file.

Recommended format:

```text
TOML or JSON
```

Example:

```rust
pub struct AppConfig {
    pub version: u32,
    pub rules: Vec<Rule>,
    pub app_profiles: Vec<AppResourcePolicy>,
    pub power_plan_profiles: Vec<PowerPlanProfile>,
    pub settings: AppSettings,
}
```

Add migration support early:

```rust
pub trait ConfigMigration {
    fn migrate(config: Value, from_version: u32, to_version: u32) -> Result<AppConfig>;
}
```

---

## 22. Safety Rules

Implement these hard protections:

```text
Never suspend protected system processes.
Never apply affinity to critical Windows processes.
Never set system-wide CPU limiter without explicit confirmation.
Never mutate all power plans silently.
Never repeatedly toggle power plan faster than cooldown.
Never suspend app with active audio.
Never suspend foreground app.
Never leave app suspended without visible restore button.
```

Add a global emergency button:

```text
Restore Everything
```

It should:

```text
Resume suspended apps
Restore process priorities
Restore process affinity
Disable app-applied efficiency modes
Clear active restrictions
Optionally restore previous power plan
```

---

## 23. Suggested Implementation Phases

## Phase 1 — Core Engine Skeleton

Goal:

```text
Create the rule engine without touching real Windows settings yet.
```

Tasks:

```text
- Define Trigger enum
- Define Action enum
- Define Rule model
- Define RuntimeState
- Define RuleEngine
- Define PriorityResolver
- Define fake ActionExecutor
- Add logging
```

Deliverable:

```text
Rules can be evaluated and logged, but no system changes are applied.
```

---

## Phase 2 — Basic Power Plan Switching

Goal:

```text
Support actual Windows power plan switching.
```

Tasks:

```text
- Implement list power plans
- Implement get active power plan
- Implement set active power plan
- Add Power Plans UI page
- Add focused app → power plan rule
- Add schedule → power plan rule
```

Deliverable:

```text
App can switch power plans based on foreground app and schedule.
```

---

## Phase 3 — Detection Layer

Goal:

```text
Build stable detection.
```

Tasks:

```text
- Foreground app detector
- Running process detector
- CPU load detector
- User idle detector
- Schedule evaluator
- Runtime state viewer in Dashboard
```

Deliverable:

```text
Dashboard accurately shows current foreground app, CPU load, idle time, and active schedules.
```

---

## Phase 4 — Rule Conflict Resolver

Goal:

```text
Prevent modules from fighting each other.
```

Tasks:

```text
- Add rule priorities
- Add conflict groups
- Add desired state calculation
- Add cooldown
- Add hysteresis for CPU load
- Add active rule display
```

Deliverable:

```text
Multiple rules can be active, but only the correct final action is applied.
```

---

## Phase 5 — App Resource Control: Soft Controls

Goal:

```text
Add low-risk per-app controls.
```

Tasks:

```text
- Implement priority control
- Implement Efficiency Mode / EcoQoS
- Store previous priority where possible
- Restore when rule deactivates
- Add App Control UI
```

Deliverable:

```text
Background apps can be moved to Efficiency Mode and lower priority.
```

---

## Phase 6 — App Affinity and Core Steering

Goal:

```text
Add medium-risk CPU controls.
```

Tasks:

```text
- Detect logical processors
- Detect hybrid CPU topology if possible
- Implement affinity policy
- Add logical processor limit presets
- Add P-core / E-core options only on hybrid CPUs
- Restore original affinity
```

Deliverable:

```text
Per-app background CPU restriction and core steering work safely.
```

---

## Phase 7 — Core Parking

Goal:

```text
Allow power plan processor tuning.
```

Tasks:

```text
- Read current core parking settings
- Write core parking settings to selected plan
- Add Power Plan Tuning UI
- Add reset-to-default option
```

Deliverable:

```text
User can tune core parking per power plan.
```

---

## Phase 8 — App Suspension

Goal:

```text
Add aggressive app lifecycle control.
```

Tasks:

```text
- Implement suspension backend
- Implement resume backend
- Add protected process denylist
- Add active audio check if possible
- Add visible Suspended Apps list
- Add one-click resume
- Add restore on app exit
```

Deliverable:

```text
Selected apps can be suspended only when safe conditions are met.
```

---

## Phase 9 — System CPU Limiter

Goal:

```text
Add global CPU limiting carefully.
```

Recommended first implementation:

```text
System CPU Limiter applies to selected app groups, not the whole OS.
```

Tasks:

```text
- Add global policy profile
- Apply CPU limit to non-critical user processes only
- Exclude protected/system processes
- Add emergency restore
```

Deliverable:

```text
System CPU Limiter reduces CPU pressure without touching boot config or critical system processes.
```

---

## Phase 10 — Reliability and Recovery

Goal:

```text
Make the app safe for daily use.
```

Tasks:

```text
- Persist applied actions
- Startup recovery
- Restore Everything button
- Crash-safe restore prompt
- Detailed logs
- Dry-run mode
- Export diagnostics
```

Deliverable:

```text
Users can trust the app and recover from bad rules.
```

---

## 24. Recommended MVP

Do not implement everything at once.

Best MVP order:

```text
1. Foreground detection
2. Power plan switching
3. Schedule rules
4. User idle detection
5. CPU load rules
6. Efficiency Mode
7. Priority control
8. App CPU limit / affinity
9. Core parking
10. App suspension
11. System CPU limiter
```

The safest first release should include:

```text
- Power plan switching
- Focused app rules
- Schedule rules
- Activity / idle rules
- Basic CPU load rules
- Dashboard
- Logs
```

Avoid putting App Suspension and System CPU Limiter in the first release.

---

## 25. Final Architecture Rule

The main rule for the whole app:

```text
Detectors observe.
Rules decide.
Resolver selects.
Executor applies.
Restore system protects.
GPUI displays and configures.
```

This gives you a scalable structure where new features can be added without creating conflicts between power plan switching, CPU limiting, app throttling, and suspension.
