# Design

## Source of truth
- Status: Active
- Last refreshed: 2026-10-29
- Primary product surfaces: Windows desktop app shell, process controls, automation settings, status dashboard, and Action Log.
- Evidence reviewed: `.agents/memory/15-design-spec.md`, `.agents/memory/20-project-scope.md`,
  `src/application/`, `src/runtime/`, `src/control/`, `src/platform/windows/`,
  `src/backend/automation.rs`, `src/backend/automation/runner.rs`,
  `src/backend/crash_recovery.rs`, `src/ui/app.rs`, `src/ui/`, `locales/`, the current
  feature-policy modules, the creator's `.impeccable.md` context, the original Iced UI at commit `3d329a9`, its tracked
  `screenshots/Home.png`, and the latest user-supplied WindInput settings screenshot, cross-checked against
  `huanfeng/wind-setting` at `8b9740c73528c50caa91b16b0442322794623f42`.

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
- CPU allocation: CPU Sets (Soft) and Processor Affinity (Hard) are separate per-app pages. Adaptive Engine may temporarily limit hot background apps through its lower-precedence Adaptive Engine policy.

## Design principles
- One owner per mechanism: A page and its settings own one Windows mechanism.
- CPU allocation uses one runtime coordinator for CPU Sets and affinity. Its order is CPU Sets
  (Soft) > Processor Affinity (Hard) > Adaptive Engine.
- Feature modules own discovery and policy state; only the coordinator owns Windows baselines,
  mutations, compensation, arbitration, and restoration. Releasing one producer queues the exact
  process key; the runtime re-resolves it once after every CPU producer has processed that pass.
- Scope before detail: Show which applications are targeted before processor selection.
- Safe by default: Present CPU Sets (Soft) as recommended; clearly warn that Processor Affinity (Hard) is strict.
- Tradeoffs: Separate pages add one navigation item but remove mode ambiguity and conflicting ownership.

## Visual language
- Direction: Refine the presentation toward the user's latest WindInput reference: restrained color, smaller headings, coherent blue-gray surfaces and compact native controls. The original Iced source remains a behavior/composition reference. The user's side-by-side Process List screenshot identifies concrete issues to fix: filled process-name buttons, stretched commands and broken side-panel labels.
- Color: Cool layered surfaces. Light uses `#f4f6fa` work area, `#eef1f6` navigation and white groups; dark uses `#131a26`, `#10161f` and `#1b2534`. Preserve the configured system/custom accent and semantic status colors. Use accent for selected navigation, readings and primary actions.
- Typography: Segoe UI, 14 logical px body, 12-13 px supporting text, 16 px semibold group headings, 18 px subheadings and 22 px page titles. Body and setting labels use regular weight. Avoid decorative icon chips and repeated headings.
- Spacing/layout rhythm: Keep headers on a 24 px inset; center forms within a maximum 1040 logical px body. Separate sections by 20 px. Setting rows use 34 px minimum height with 8 px described-row vertical padding and 16 px group padding. Controls normally cap at 240 px. Contextual tuning help stays on the native row; warnings and validation remain visible.
- Shape/radius/elevation: Native Wind UI groups with 8 px corners, 4 px controls, fine borders and muted dividers and no shadows or custom control painting.
- Motion: Respect the existing Animation Mode; page/panel/layout transitions remain immediate. No new animation timers.
- Imagery/iconography: Bundled Lucide SVGs where they identify navigation or actions. Ordinary section headings are text.

## Components
- Existing components to reuse: Page shell, native feature toggles, process picker, rule tables, core grid, dropdowns, status rows, indicators, existing data plots and removal confirmation.
- New/changed components: One navigation pane, compact per-context policy rows, collapsible status/settings panels, centered forms, the original Home dashboard composition and grouped About details with native secondary buttons. All use the existing native composition helpers.
- Variants and states: Enabled, disabled, ready, applied, protected, inaccessible, empty, and failed.
- Token/component ownership: Wind UI built-in widget styles and theme roles in `src/ui/design.rs`; `settings_pages.rs` adapts the existing system/custom accent preference.

## Accessibility
- Target standard: Preserve existing keyboard, focus, contrast, and reduced-motion behavior.
- Keyboard/focus behavior: Every rule action and selector remains keyboard reachable.
- Contrast/readability: Use semantic theme colors and existing text hierarchy.
- Screen-reader semantics: Preserve component labels and tooltips for icon-only actions.
- Reduced motion and sensory considerations: Respect system or explicit Animation Mode.

## Responsive behavior
- Supported breakpoints/devices: Windows desktop layout at the existing minimum window size and above.
- Layout adaptations: One 264 logical px primary pane, with selected-category children inline at every width and an optional compact icon rail. No second navigation column. Headers stay aligned with the workspace; form bodies center within their width limit. Native flow wraps page actions at narrow sizes. Supported pages show their status/settings/preset panel by default. A bottom Hide side panel / Show side panel toggle leaves an icon-only right rail when collapsed; the labelled header action remains available. Preserve editor input identity on resize. Home changes its metric arrangement at 760 logical px of content width.
- Touch/hover differences: Desktop-first; essential meaning must not rely on hover alone.

## Interaction states
- Loading: Use existing status/refresh patterns.
- Empty: Explain that no applications are configured and provide the process picker.
- Error: Show actionable status and Action Log entries without repeated spam.
- Success: Reflect applied process counts and per-rule indicators.
- Process List presentation: Plain native clickable process names, compact toolbar/selection actions and vertically grouped column-width controls. Avoid accent-filled name bars or full-width command grids. Keep native focus, virtual table behavior, context menus and exact-target validation.
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
- Framework/styling system: Rust and pinned upstream Wind UI 0.28.0 with its software renderer; no additional control or styling framework.
- Design-token constraints: Reuse current theme, spacing, radius, and motion tokens.
- Performance constraints: Avoid duplicate process scans and overlapping managers.
- Compatibility constraints: Public pre-release; do not add legacy settings aliases or migrations. Preserve process identity validation and restoration. CPU selection currently covers the first processor group and discloses that limit on multi-group systems.
- Test/screenshot expectations: Keep settings round-trip, navigation, rule construction, manager lifecycle, and mask-selection tests aligned.

## Architecture direction
- Canonical architecture: `docs/architecture.md`.
- Keep UI flow one-way: send typed settings or commands and consume published read models. Process
  List enumeration and presentation remain a separate read-side query.
- Keep `SettingsEditor` as the sole settings draft and persistence boundary. Persistent Windows
  configuration uses typed application services rather than runtime claims.
- Feature managers own policy and reporting; typed controllers own live state, restoration, and
  recovery intent; `src/platform/windows/` owns raw Windows calls.
- Classify each Windows write as temporary, process-lifetime, persistent, or irreversible before
  routing it. Preserve exact-identity validation, access barriers, verification, compensation,
  and conservative restoration.

## WindInput reference refinement with native Wind UI controls
- The latest user comparison prefers WindInput's elegance. Use its restrained hierarchy, blue-gray surface layers, fine borders and compact controls while retaining Winderust's feature ownership and native Wind UI implementation. Preserve safety behavior and optional panel visibility. Audience and operational depth from `.impeccable.md` remain current; this document owns the latest visual direction.
- Research references: [Microsoft settings guidance](https://learn.microsoft.com/en-us/windows/apps/design/app-settings/guidelines-for-app-settings), [PowerToys General](https://learn.microsoft.com/en-us/windows/powertoys/general) and its [published settings screenshot](https://learn.microsoft.com/en-us/windows/powertoys/images/general/general.png), [EarTrumpet settings source](https://github.com/File-New-Project/EarTrumpet/blob/master/EarTrumpet/UI/Views/SettingsWindow.xaml), and [Microsoft navigation guidance](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/navigationview). Borrow organization and control hierarchy; preserve Winderust's own settings ownership and Save/Discard semantics. These references do not justify changing the native UI framework.
- One sidebar provides primary navigation and inline child pages. Keep canonical names, optional counts, search, history, pinned Log/Settings/About and compact mode. Selection uses restrained native surfaces, plain icons and an accent marker; avoid filled icon tiles.
- One current-page heading carries hierarchy; history controls remain quiet and the full route stays in the title tooltip. Narrow editors place actions below profiles. Profile, feature help and labelled Status/Settings/Presets actions stay in the header. Keep important Create and Trim now actions visible when their detail panel is closed.
- Status, filters and presets are visible by default on supported pages. A bottom toggle hides the panel or reopens it from its icon-only right rail, matching the left navigation pattern. Keep the labelled header panel action available. Remember the same visibility choice through navigation, resizing and window recreation within the resident session.
- Wide windows reserve space beside the editor for the side panel. Compact windows show the same panel as a right-hand overlay, preserving editor width, focus and native table virtualization. The bottom toggle retains the same visibility choice at every width.
- Feature enablement is distinct from background policy. Priority Control and Background Efficiency show one master row followed by Background, Foreground Detection and Visible Window Detection rows. Place each detection switch beside its selector, with an explanatory tooltip. Preserve independent settings and editability when disabled. One Advanced disclosure holds related secondary controls.
- Use simple text headings and grouped settings. Remove generic Default headings and repeated icon chips. Empty rules show their existing explanatory text beneath the process picker; reserve table headers and horizontal scrolling for actual rows.
- Home restores the original wide dashboard: CPU/RAM/disk on the first row, network/enabled features on the second, then compact Main sections shortcuts. Narrow layouts use two metric columns. Keep actual sampling, pause/resume and history behavior.
- About groups identity and native secondary project/community/documentation/license buttons in one surface and update controls in a second surface. Preserve all destinations, copyright, GPL-3.0-only, update states and preferences.
- Adaptive Engine groups shared detection/timing and processor-power settings with native dividers. Each independent CPU policy gets one labelled switch and its advanced details, without repeating the policy name as a disclosure heading. The Status/Presets tabs stay above one panel scroller, with one horizontal inset. Feature info scrolls away with page content instead of occupying a fixed, independently scrolling header.
- Preserve the full-width pending-state/Exit/Save/Discard footer, native Windows frame, draft ordering, input validation, focus, native controls and existing motion preference.

## Visual verification
- Reuse isolated native render cases; fixtures disable automation and portable settings IO. Capture representative Home, About, Language and Appearance, Adaptive Engine, Process List, and a modal, in English/light and Traditional Chinese/dark.
- Review minimum 900x620, standard 1120x760, and a larger desktop viewport. Check navigation, label/control spacing, primary/secondary hierarchy, long translations, and fixed footer/modal actions.
- Keep draft ordering, focus, native scrolling/virtualization, lifecycle, controller/recovery ownership, and the existing required checks. Do not add tests that merely duplicate style constants.
- Exclude the entire reactive modal host from native hit testing when no dialog is present. Pointer regressions must exercise the composed window, including navigation beneath an empty host and blocked background actions beneath an open dialog.
- Screenshots establish visual coverage only; do not claim full accessibility or Windows integration coverage from them.
- Exercise the expanded states users actually see: Adaptive Engine CPU Behaviour with Feature info and Status open at 1920x1056 and 900x620, including a scrolled form. Require one active main-content scroll area, an independently scrollable status panel with fixed tabs, contextual help, visible safety warnings, readable text and complete controls. A successful PNG capture is not a design acceptance verdict.

## Open questions
- None blocking this refinement. The latest WindInput reference guides visual decisions; all native controls, routes and settings remain available.
