# Design

## Source of truth
- Status: Active
- Last refreshed: 2026-09-09
- Primary product surfaces: Windows desktop app shell, process controls, automation settings, status dashboard, and Action Log.
- Evidence reviewed: `.agents/memory/15-design-spec.md`, `.agents/memory/20-project-scope.md`,
  `src/application/`, `src/runtime/`, `src/control/`, `src/platform/windows/`,
  `src/backend/automation.rs`, `src/backend/automation/runner.rs`,
  `src/backend/crash_recovery.rs`, `src/ui/app.rs`, `src/ui/`, `locales/`, and the current
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
- CPU allocation: CPU Sets (Soft) and Processor Affinity (Hard) are separate per-app pages. Adaptive Engine may temporarily limit hot background apps through its lower-precedence CPU Scheduler policy.

## Design principles
- One owner per mechanism: A page and its settings own one Windows mechanism.
- CPU allocation uses one runtime coordinator for CPU Sets and affinity. Its order is CPU Sets
  (Soft) > Processor Affinity (Hard) > Adaptive Engine / CPU Scheduler.
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
- Imagery/iconography: Bundled Lucide SVG navigation/action icons.

## Components
- Existing components to reuse: Page shell, feature toggle, process picker, rule cards, core grid, dropdowns, status rows, indicators, and removal confirmation.
- New/changed components: Mechanism-specific CPU Sets (Soft) and Processor Affinity (Hard) rule pages using existing components.
- Variants and states: Enabled, disabled, ready, applied, protected, inaccessible, empty, and failed.
- Token/component ownership: Iced built-in widget styles and palette generation; settings_pages.rs only adapts the existing system/custom accent preference.

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
- Framework/styling system: Rust and Iced 0.14 with the Tiny Skia software renderer.
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

## Iced presentation
- Use the native Windows title bar, resize borders, and window buttons through Iced decorations. Keep close requests routed through existing unsaved-change and tray handling.
- Status/error text and chart series use Iced semantic styles and theme palette colors.
- Use Iced's native widgets and interaction states with Winderust's neutral light/charcoal palette. Keep shared borderless card surfaces, selected navigation, and hover/press feedback in widgets.rs.
- The Windows/custom accent preference seeds the primary palette; shared neutral surface, border, and muted-text roles preserve hierarchy across light and dark themes.
- Reserve filled primary buttons for primary commands. Use text buttons for secondary commands, gentle accent interaction shading, and borderless surfaces.
- Keep application-specific layout: grouped navigation, readable page headings, compact dashboard charts, constrained form rows, status rails, and scrollbar spacing.
- Retain bundled icons, charts, and bounded motion where standard widgets do not cover existing functionality.
- Add custom styling only for a concrete requirement unsupported by Iced's built-ins.

## Open questions
- None blocking implementation. Density preferences can be adjusted after reviewing native screenshots.

## Kraken Desktop reference
- User reference: https://www.kraken.com/desktop (reviewed 2026-09-08).
- Observed in the app screenshots: compact module headers, aligned data, fine dividers, dark neutral surfaces, and selective accent color.
- Adapt the desktop application's hierarchy, not the surrounding promotional artwork or trading workflows.
- Use neutral charcoal surfaces in dark mode and neutral pale surfaces in light mode; keep gentle accent tinting in interaction states. Preserve System/Light/Dark and the existing accent preference.
- Page title and power-source controls share a compact toolbar; navigation uses tighter rows; dashboard charts and shortcuts form aligned modules; status groups use dividers instead of stacked nested cards.
- Keep the native Windows frame, all feature routes, existing safety behavior, and motion preferences. No new styling framework or dependencies.
- Reference PNGs are local review artifacts under target/design-reference/. Render checks use --features render-smoke -- --render-smoke (English/light 1120x760, Traditional Chinese/dark 900x620).

## Minimal surface refinement
- The supplied reference images emphasize color and hierarchy rather than outlined cards. Remove redundant outlines from headers, navigation, setting groups, and logs.
- Distinguish major dashboard/navigation surfaces with a slight theme-derived tone shift. Use spacing for setting groups and fine rules only where useful for data alignment.
- Selected tabs use the shared gentle accent-filled selection surface; inactive tabs are quiet. Navigation and menu selections retain accent text. Hover, selected, and pressed surfaces use the current accent at 10%, 15%, and 20% opacity. Disabled cards and buttons retain their surface with muted text. Reserve solid accent fills for primary actions such as Save.
- Keep standard Iced inputs, dropdowns, hover/disabled states, Windows framing, and existing functionality. Shared style functions address the requested refinement without a custom widget system.

- Landing-page navigation cards and Home shortcuts use the shared borderless surface and respond across the full card on hover. Search functions through the sidebar; do not duplicate its search field on Home. The entire card is clickable; setting rows and expandable groups also use visible card surfaces.

## Interaction and readability refinement
- Use Windows Segoe UI typography for small-text readability in Iced, with 14px body text and 13px sidebar labels. Keep Iced's existing advanced shaping, system font fallback, and DPI-aware software text rendering; do not add a second renderer or claim ClearType support.
- Combine Winderust's compact icon rows and inline feature status with Kraken's quiet neutral surfaces and selective accent. Hover changes the surface; pressing strengthens feedback without changing layout geometry.
- Expanded navigation has a fixed 264px width. Long labels stay on one line inside clipped slots, with full labels available in native Iced tooltips. Chevron controls have a fixed width.
- Page introductions start collapsed behind the localized How it works header action and reset when navigating. Keep warnings and validation beside their controls; landing cards retain a readable summary.
- Native render checks also cover expanded English/Chinese help, long English navigation at minimum width, and the compact sidebar.

## GPUI setting-card structure target
The source-mapped hierarchy corrections are implemented. See [the design integrity review](docs/iced-design-integrity.md) for the original findings, corrections, and verification limits. Card surfaces alone are not evidence of parity.
- The latest user direction supersedes the earlier flat setting-group treatment. Follow the former GPUI setting_action_card and setting_group layout from commit cab3186.
- Each standalone setting row has a full-width, small-radius card surface. Keep labels and their controls together, with consistent padding and vertical alignment.
- Chevrons are passive indicators inside a shared clickable row/header, never separate icon buttons. Sidebar section rows navigate to their landing page and expand; clicking the current section toggles its children.
- Each expandable group is one enclosing card containing its full-width clickable header, state chevron, and animated body. Preserve the existing collapse state and motion preferences.
- Use widgets::settings_card for standalone rows and widgets::setting_group for independent header actions and flat expandable bodies. Compose standard Iced containers/buttons; use Iced rounded_box styling without outlines and the existing hover styles. Do not restore GPUI dependencies or vendored code.
- Custom rule and exclusion rows have card boundaries. Process List remains a dense table, matching the old GPUI design. Headers, explanatory text, and command bars do not need independent cards.

- All card surfaces are borderless, including hover and pressed states. Use background shading and spacing to separate cards; retain native input/control boundaries.

## Original layout reference — 2026-09-09
- Reference: user-provided GPUI screenshots and `cab3186:src/ui/app/shared/page_navigation.rs`; `CONTENT_MAX_WIDTH` was 1040 pixels.
- Center a maximum 1040-pixel workspace in the area beside the sidebar. Keep headings, forms, dashboard, and tables aligned to that frame.
- Use 28-pixel semibold page headings with Home breadcrumbs. Retain Segoe UI and native Windows framing.
- Home: CPU/RAM/I/O across the first row; Network and Enabled Features across the second; section shortcuts below in three columns. Narrow windows use two columns.
- Pin Log, Settings, About, and the sidebar toggle below the scrollable main navigation.
- Landing cards are compact, single-line navigation rows. Descriptions remain available on feature pages through How it works.
- Process List keeps its virtualization and configurable columns, with 52-pixel rows, aligned names/icons, and internal row dividers. Action Log uses aligned table columns and hover text for truncated reasons.
- Cards use charcoal #191b1e over #0f1011 in dark mode, white over neutral gray in light mode; keep card outlines absent.

- Adaptive Engine uses numeric steppers with units and contextual help, right-aligned switches, equal-width tuning tabs, and section breadcrumbs. Its 320px desktop side panel sits outside the 1040px content frame; below 1400px it follows the editor to preserve form width.
- Adaptive Engine Priority Control keeps each control's advanced options in its own expandable row, aligned under Focus, Visible Window, and Background. Process and Memory Priority include detection and priority-preservation controls. Disabling detection falls through to the next applicable tier without changing CPU restraint's focus protection. Process Priority always protects existing High/Realtime classes; optional same-or-higher/lower preservation defaults off. Memory preservation defaults on. Both detection layers default on, retaining existing behavior. Custom presets capture these options; built-in presets restore their defaults.

- Shared setting rows use a right-aligned On/Off switch or bounded numeric stepper. Status and tuning presets use the shell dock on desktop and a compact disclosure at narrow widths. Preserve all additional policy options below the main Adaptive Engine table. About groups identity and update controls into separate surfaces; Appearance orders Language, Accent, Theme, Animation and uses square color swatches. Unsaved settings remain in a lower-right notice.

- Custom-rule app selection uses the shared src/ui/app_picker.rs search dropdown: a compact input beside Add, two-line app name/path entries with cached icons and aligned placeholders, and Browse local executable inside the menu. The popup overlays the page, stays within the viewport, and supports keyboard selection and outside-click dismissal. Each feature retains its own eligibility and duplicate checks; unavailable App Suspension targets remain visible but disabled.

- Removing a rule, exclusion, or custom preset updates the settings draft immediately. Do not add a second removal confirmation; Save commits the draft and Discard restores the saved settings.

Dropdowns use the shared Select control: a native Iced pick-list field with a native-widget overlay menu, persistent accent selection marker, gentle accent hover, and white/base option text. The menu stays within the viewport, scrolls to the selected option on open, and supports keyboard selection and outside-click dismissal.

The shared extended palette defines neutral navigation, card/field, button, border, and muted-text roles. Accent fills, checkbox marks, and switch thumbs follow the original Winderust rule: white in light mode; in dark mode, white when weighted RGB brightness (0.299R + 0.587G + 0.114B) is below 140, otherwise #111111. Keep semantic status colors distinct. Windows appearance comes from UISettings.GetColorValue; explicit theme and custom accent preferences override system values.

Inactive navigation icons and resting control borders use neutral tones. Active navigation retains the accent; enabled switch thumbs use the shared theme-and-accent foreground rule. Expanded setting groups have no header divider and use evenly spaced rows, and the active custom color swatch has a contrasting outline. Appearance dropdowns use the shared select width.
