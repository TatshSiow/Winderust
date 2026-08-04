# Design

## Source of truth
- Status: Active
- Last refreshed: 2026-08-04
- Primary product surfaces: Windows desktop app shell, process controls, automation settings, status dashboard, and Action Log.
- Evidence reviewed: `.agents/memory/15-design-spec.md`, `src/ui.rs`, `src/ui/app/pages/`, `src/ui/app/shared/`, `locales/`, and the current CPU-control backends.

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
- Explicit CPU allocation rules take precedence over Workload Engine CPU allocation for the same process.
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

## Open questions
- None for the approved CPU Sets (Soft) and Processor Affinity (Hard) split.
