# Design

## Source of truth
- Status: Active
- Last refreshed: 2026-08-12
- Primary product surfaces: Windows desktop app shell, process controls, automation settings, status dashboard, and Action Log.
- Evidence reviewed: `.agents/memory/15-design-spec.md`, `.agents/memory/20-project-scope.md`,
  `src/application/`, `src/runtime/`, `src/control/`, `src/platform/windows/`,
  `src/backend/automation.rs`, `src/backend/automation/runner.rs`,
  `src/backend/crash_recovery.rs`, `src/ui/app.rs`, `src/ui/app/`, `locales/`, and the current
  feature-policy modules.

## Brand
- Personality: Calm, elegant, sleek, operational, and recognizably Winderust.
- Trust signals: Accurate Windows terminology, visible state, conservative defaults, restoration, and actionable errors.
- Avoid: Marketing layouts, decorative effects, ambiguous controls, and multiple features owning the same process property.

## Product goals
- Goals: Make Windows performance controls understandable, reversible, and efficient for repeated use.
- Non-goals: Hide Windows mechanisms behind invented terminology or provide unsafe system-wide shortcuts.
- Success signals: Users can identify what a control changes, its scope, and whether it is active without consulting documentation.

## Personas and jobs
- Primary personas: Windows power users and gamers managing application performance and efficiency.
- User jobs: Configure process behavior, understand applied state, diagnose failures, and restore original state safely.
- Key contexts of use: Desktop and laptop systems, administrator and standard-user sessions, mixed P/E-core CPUs.

## Information architecture
- Primary navigation: Home, Process List, Winderust Features, Power Plan Control, Priority Control, CPU Control, Action Log, Settings, About, and Advanced.
- Core routes/screens: Dense operational pages grouped by feature ownership.
- Content hierarchy: Feature enablement, concise explanation, controls/rules, current status, then exceptions or advanced details.
- CPU allocation: CPU Sets (Soft) and Processor Affinity (Hard) are separate per-app pages. There is no blanket background restriction, mixed-mode rule, or Efficiency Mode Off allocation rule.

## Design principles
- One owner per mechanism: A page and its settings own one Windows mechanism.
- CPU allocation uses one runtime coordinator for CPU Sets and affinity. Its order is CPU Sets
  (Soft) > Processor Affinity (Hard) > Core Limiter > Adaptive Engine / Workload Engine.
- Feature modules own discovery and policy state; only the coordinator owns Windows baselines,
  mutations, compensation, arbitration, and restoration. Releasing one producer queues the exact
  process key; the runtime re-resolves it once after every CPU producer has processed that pass.
- Scope before detail: Show which applications are targeted before processor selection.
- Safe by default: Present CPU Sets (Soft) as recommended; clearly warn that Processor Affinity (Hard) is strict.
- Tradeoffs: Separate pages add one navigation item but remove mode ambiguity and conflicting ownership.

## Visual language
- Color: Neutral surfaces with the configured accent for active state and semantic warning/danger colors.
- Typography: Compact hierarchy with readable labels and muted supporting text.
- Spacing/layout rhythm: Dense, stable rows using existing constants and setting groups.
- Shape/radius/elevation: Existing Winderust surface and control radii; no new visual layer.
- Motion: Existing bounded hover, expand/collapse, modal, and navigation motion respecting Animation Mode.
- Imagery/iconography: Existing Lucide navigation/action icons through `NavIcon`.

## Components
- Existing components to reuse: Page shell, feature toggle, process picker, rule cards, core grid, dropdowns, status rows, indicators, and removal confirmation.
- New/changed components: Mechanism-specific CPU Sets (Soft) and Processor Affinity (Hard) rule pages using existing components.
- Variants and states: Enabled, disabled, ready, applied, protected, inaccessible, empty, and failed.
- Token/component ownership: Existing GPUI/gpui-component helpers and Winderust theme tokens.

## Accessibility
- Target standard: Preserve existing keyboard, focus, contrast, and reduced-motion behavior.
- Keyboard/focus behavior: Every rule action and selector remains keyboard reachable.
- Contrast/readability: Use semantic theme colors and existing text hierarchy.
- Screen-reader semantics: Preserve component labels and tooltips for icon-only actions.
- Reduced motion and sensory considerations: Respect system or explicit Animation Mode.

## Responsive behavior
- Supported breakpoints/devices: Windows desktop layout at the existing minimum window size and above.
- Layout adaptations: Existing constrained work area, scrolling, truncation, and collapsible navigation.
- Touch/hover differences: Desktop-first; essential meaning must not rely on hover alone.

## Interaction states
- Loading: Use existing status/refresh patterns.
- Empty: Explain that no applications are configured and provide the process picker.
- Error: Show actionable status and Action Log entries without repeated spam.
- Success: Reflect applied process counts and per-rule indicators.
- Process actions: Stacked parent rows show only `Stop process tree`, covering every process in the stack and their descendants; expanded sub-items show both stop actions. Non-stacked rows show `Stop process`, plus `Stop process tree` only when the process currently has children.
- Navigation: Language and Appearance can show enabled-feature count pills beside Winderust Features, Power Plan Control, Priority Control, and CPU Control while the sidebar is expanded; collapsed navigation keeps icons unbadged and aligned.
- Cards: Language and Appearance can show Enabled or Disabled pills on cards for pages with a top-level feature switch. Cards without a single enabled state remain unbadged.
- Disabled: Preserve configured rules while restoring managed process state. Process List keeps
  Advanced Suspend/Resume visible but disables Suspend for inaccessible, protected, Session 0,
  service-account, and curated Windows host processes. The App Suspension picker uses the same
  visible-disabled behavior with an `Unavailable` label without disabling unrelated controls.
  Grouped Process List rows apply Suspend/Resume to every captured process in the group.
- Offline/slow network, if applicable: Not applicable to CPU allocation.

## Content voice
- Tone: Direct, technical when necessary, and calm.
- Terminology: Use CPU Sets (Soft), Processor Affinity (Hard), logical processors, P-cores, E-cores, and SMT consistently.
- Microcopy rules: State scope and consequence; label Processor Affinity (Hard) as strict and CPU Sets (Soft) as recommended.

## Implementation constraints
- Framework/styling system: Rust, GPUI, and gpui-component with existing helpers.
- Design-token constraints: Reuse current theme, spacing, radius, and motion tokens.
- Performance constraints: Avoid duplicate process scans and overlapping managers.
- Compatibility constraints: Public pre-release; do not add legacy settings aliases or migrations. Preserve process identity validation and restoration. CPU selection currently covers the first processor group and discloses that limit on multi-group systems.
- Test/screenshot expectations: Keep settings round-trip, navigation, rule construction, manager lifecycle, and mask-selection tests aligned.

## Architecture direction
- Canonical refactor plan: `docs/architecture-refactor-plan.md`.
- Use a mechanism-centered modular monolith: one external `RuntimeHandle`, one thin `RuntimeCore` lifecycle/composition root, and typed controllers that each own one coherent Windows mechanism.
- Keep UI flow one-way: UI sends typed settings, overrides, or commands and consumes segmented, generation-based read models. Keep Process List enumeration, icons, grouping, sorting, and sample history in its separate read-side query.
- Keep `SettingsEditor` as the sole settings draft/revision/persistence boundary. Apply runtime
  auto-exclusions as narrow, idempotent patches without saving unrelated draft edits; startup
  registration, Win32 Priority Separation, and Advanced Power Plan Tuning use typed application
  services rather than runtime claims.
- Collect process, foreground, visible-window, power, session, input, and topology observations at most once per requested domain in a reconciliation pass; do not create a permanent global process snapshot.
- Preserve feature managers as policy owners for timers, hysteresis, cooldowns, scoring, target selection, failure suppression, and status. Typed mechanism controllers own baselines, active claims, raw setters, clean restoration, and recovery replication.
- Process Priority/Power Throttling, Thread Priority, Dynamic Priority Boost, I/O Priority, GPU
  Priority, Memory Priority, CPU allocation, and App Suspension are complete mechanism boundaries.
  Static policy, Adaptive/Workload policy, and Process List actions share their RuntimeCore-owned
  controllers; feature managers retain target selection, timers, preservation, suppression,
  status, and Action Log policy only.
- Thread Priority uses per-thread baselines keyed by exact process identity, thread ID, and thread
  creation time. Static, Adaptive, and Process List producers share `ThreadPriorityController` and
  the bounded result-bearing process-control FIFO.
- I/O Priority is process-instance bound and preserves the exact raw Windows baseline, including values outside Winderust's selectable range. Static, Adaptive, and Process List producers share `IoPriorityController`; feature code owns only tiering, rules, preservation policy, suppression, and reporting.
- GPU Priority is process-instance bound and preserves the exact raw WDK scheduling-class baseline. Static, Adaptive, and Process List producers share `GpuPriorityController`; a missing GPU scheduling context is a typed pending condition rather than a permanent process failure.
- Memory Priority is process-instance bound and preserves the exact raw Windows baseline. Static Memory Priority, Workload Engine Memory Priority, and Process List share `MemoryPriorityController`; static policy explicitly outranks an overlapping Workload Engine claim without disturbing non-overlapping Workload claims or the first pre-Winderust baseline.
- Process Priority and process Power Throttling share
  `PriorityEfficiencyController` because Efficiency Mode changes both as one
  compensated transaction. Static Process Priority, Background Efficiency,
  Workload Engine priority/Efficiency policy, foreground boost, and Process
  List actions use deterministic owner precedence over property-specific exact
  process baselines; feature managers retain policy and reporting only.
- CPU Sets (Soft), Processor Affinity (Hard), Core Limiter, and Adaptive/Workload CPU allocation
  submit claims to `CpuAllocationCoordinator`; it alone owns mutually exclusive live properties,
  pass-end handoff/retry, exact baselines, recovery, and reverse restoration.
- App Suspension policy submits exact targets to `SuspensionController`. The controller alone owns
  named Job Objects, freeze/thaw transactions, cleanup retry, manual/automatic lifecycle, and
  aggregate shutdown; the crash helper independently retains the job before freeze.
- Automatic power-plan choices use `PowerPlanController`; explicit Advanced Power Plan Tuning uses
  its application service. Both route native GUID/scheme/processor-setting calls through the same
  narrow Windows adapter without sharing lifecycle or recovery ownership.
- Timer Resolution and Winderust self-power are process-lifetime controllers with explicit clean
  shutdown and no external journal. Irreversible Memory Trim and process termination use typed,
  result-bearing commands and deliberately own no baseline or restore path.
- Raw managed Windows calls live in mechanism-specific `src/platform/windows/` adapters. Typed
  controllers own identity, arbitration, transactions, compensation, restoration, and recovery
  intent; the crash helper remains the independent replay authority where its access contract
  differs.
- Define precedence per Windows property. Migrate every producer of a property together, including Adaptive Engine paths and Process List actions; mixed legacy/new ownership is never a valid shipped state.
- Classify Windows writes before routing them: temporary externally persistent state uses typed control plus the recovery helper; process-lifetime requests use clean release; intentional persistent configuration uses application services; irreversible commands use validated command services.
- Memory Trim and Stop Process / Stop Process Tree use the bounded runtime command FIFO and sole
  typed adapters. The worker revalidates exact identity and current safety policy; tree termination
  preflights the complete children-first batch and reports partial execution failures. Neither
  command participates in recovery or restoration.
- Preserve identity validation, access and protected-process policy, original-state capture, synchronous watchdog acknowledgement, expected-state verification, reverse compensation, and conservative restoration.
- Migrate through independently revertible, mechanism-complete phases. Do not mix product redesign, preset tuning, or speculative frameworks into the architecture refactor.

## Open questions
- None for the approved CPU Sets (Soft) and Processor Affinity (Hard) split.
