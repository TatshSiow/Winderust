# Winderust Design Spec

This file describes the current app design direction. Use it when changing Iced UI code, adding pages, or cleaning UI helpers.

## Product Feel

Winderust is an operational Windows power and process-control utility. The UI should feel dense, calm, scannable, and repeat-use friendly. Do not turn it into a marketing page, hero layout, or decorative dashboard.

Prefer visible state, compact controls, and predictable rows over large illustrations or card-heavy presentation.

## App Shell

- Use the native Windows title bar and resize borders through Iced window decorations, with left navigation and a right work area.
- Windows owns the title bar and window buttons. Keep native close requests routed through the existing tray, unsaved-change, and shutdown handling.
- The sidebar is grouped by product area through `Page::sections()`. It is
  searchable when expanded and keeps a search action in its remembered compact
  icon rail; the quiet navigation-styled toggle stays below a divider in normal
  sidebar flow rather than floating over content. Keep icon and row geometry
  stable across the animated expanded/compact transition; selected and hover
  surfaces retain rounded compact-row geometry instead of being edge-clipped.
  Search remains one persistent field across both states so its icon, text
  metrics, focus, and rounded surface never swap or reflow during the motion.
- The main page area scrolls vertically and keeps content constrained with stable width behavior.
- Navigation labels and page sections live in `src/ui.rs`; page rendering
  dispatch stays in `WinderustApp::page_view` in `src/ui/iced/app.rs`.

## Layout Rules

- Use Iced rows and columns with explicit `Fill`, `Shrink`, and constrained widths for flexible children.
- Rows and table headers should have stable heights across state changes.
- Use fixed or computed widths for tables and policy columns. Do not let dynamic labels resize the process list.
- Constrain long process names, labels, and status values so they do not resize adjacent columns.
- Avoid nested cards. Cards are for repeated rows, setting groups, status blocks, popovers, and tool surfaces.
- Keep surface and control corner radii small and consistent. Do not introduce large rounded marketing panels.

## Components

- Reuse `src/ui/iced/widgets.rs` for shared numeric and power-plan controls, and
  `motion.rs` for collapsible groups and removal transitions. Use Iced buttons,
  checkboxes, pick lists, text inputs, and sliders for their natural control types.
- Use switches or checkboxes for binary state.
- Use sliders, steppers, or numeric inputs for numeric settings.
- Use dropdowns for bounded option sets.
- Use icon buttons for compact repeated actions; text buttons are fine for clear primary commands such as save/import/export.

## Visual Language

- Prefer Iced built-in themes and widget styles. Do not maintain custom surface/button style helpers; add custom styling only for a concrete unsupported requirement. Preserve the existing accent preference through the primary palette color.

- Base surfaces are neutral and restrained. Accent color marks primary action, active navigation, selection, and important status.
- Respect `AppThemeMode`, `AccentColorSource`, and system accent behavior through the Iced theme and `settings_pages.rs` color helpers.
- Do not add purple/blue gradients, decorative blobs, glow backgrounds, or one-note palettes.
- Status colors should stay semantic: success for active/applied, warning for caution, danger for destructive or failed actions.
- Text hierarchy is compact: small labels, body rows, muted helper text. Avoid hero-scale text inside panels.

## Icons

- Reuse bundled Lucide SVGs through `src/ui/assets.rs`; page icons are mapped in
  `src/ui/iced/navigation.rs`.
- Trace all asset consumers before changing Lucide generation or dependencies.
- Keep action icons at existing sizes, usually 12-18 px depending on row density.

## Motion

- Preserve motion unless the user explicitly asks to remove it.
- Respect `AnimationMode`: system/on/off flows through `settings_pages::animations()`.
- Use `motion::reveal` and `motion::removal` with stable keyed rows and the shared animation preference.
  Confirmed deletion must update settings immediately; animation may retain only a transient visual copy.
- Motion should clarify state changes: selected navigation, hover, dropdowns, popovers, switches, collapsible groups, and process groups.
- Right-side status and preset rails slide at the window edge using the shared control-motion timing, collapse to the same 64 px action-row pattern as navigation, and retain their content only until an exit transition completes.
- Keep animation IDs stable and bounded. Do not create unbounded global motion state keyed by volatile data.

## Process List

- The process list is a dense table, not a card grid.
- Keep process icon, grouped process row, PID/count, and policy columns visible and stable.
- Keep the User column at the right edge. Show the exact process token account when available. When Windows denies token access, show `Unavailable · S#` with an explanatory tooltip; identify kernel pseudo-processes as `Windows kernel`.
- Keep Windows-critical processes visible as read-only `Protected system process` rows in Process List, omit them from every Add process picker, and fail closed when criticality cannot be verified.
- For other inaccessible rows, show `Administrator required` before elevation and `Access denied` when Winderust is already elevated.
- When elevated, use normal Windows process permissions for discovery and automation while retaining every critical, built-in, identity, and protected-process barrier.
- With Advanced Controls enabled, keep Suspend/Resume present in the context menu and use the existing disabled style when the selected process cannot be safely suspended. Grouped rows apply the action to every captured process in the group. Keep unavailable App Suspension picker candidates visible, disabled, and explicitly labeled `Unavailable`. App Suspension safety must not disable unrelated process controls.
- Stacked Process List parent rows expose only `Stop process tree`, covering every process in the stack and the union of their descendants; their expanded sub-items expose both stop actions. Non-stacked rows always expose `Stop process` and expose `Stop process tree` only while the selected process has a child in the current process snapshot.
- Report `Efficiency mode` only when the process has both EcoQoS and `IDLE_PRIORITY_CLASS`, matching Task Manager rather than treating EcoQoS alone as full Efficiency mode.
- Column visibility belongs in the existing dropdown/checkbox pattern.
- Language and Appearance owns the enabled-feature counts toggle. When enabled, expanded navigation shows numeric pills beside Winderust Features, Power Plan Control, Priority Control, and CPU Control based on each section's enabled modules; collapsed navigation keeps the pills hidden to preserve icon alignment.
- Language and Appearance separately owns the feature-card status toggle. When enabled, shared landing and search-result cards show Enabled or Disabled pills only for pages backed by a top-level feature switch; pages without a single enabled state remain unbadged.
- Editable policy cells should use inline dropdown controls. Avoid modal flows for simple policy edits.
- Preserve fixed column layout calculations and tests when adding columns.

## Settings Pages

- Adaptive Engine is the parent of CPU Scheduler. CPU Scheduler has no separate master switch;
  CPU Pressure Restraint and Limit Background Processors own independent switches and retain their
  configuration while Adaptive Engine is off.
- Adaptive Engine uses the right-rail Status / Presets tabs. Built-in presets are read-only; custom presets can be added, renamed, refreshed from the current tuning, and deleted. Presets exclude master enable switches, custom rules, exclusions, and the separate Background Efficiency feature. The preset editor spans the available modal width, and its control state remains independent from the live page rendered behind it.
- Adaptive Engine and preset details share CPU Behaviour, Processor Power, and Priority Control tuning tabs; the live page also exposes Custom Rules. The right-rail Presets panel is the single preset entry point. Processor Power uses separate setting cards rather than a collapsible group; turning its policy off dims and disables the value cards while leaving the policy switch available. CPU Pressure Restraint and Limit Background Processors likewise dim and disable only their own setting rows when off; preset-only CPU Pressure tuning remains editable because presets do not own its operational switch. Adaptive Engine does not manage timer-resolution behavior. Priority Control is a Focus / Visible Window / Background table with full-width table dropdowns. Each row switch dims and disables that row's three dropdowns while remaining interactive. Its Process Priority row reuses the safe automatic subset of the main Process Priority choices; High and Realtime remain manual-only. Adaptive Background Efficiency is another row in that table; `Default` makes Focus or Visible Window inherit the Background value. Memory Priority supports `Default` in every tier, meaning Adaptive Engine submits no Memory Priority claim for that tier. While CPU Pressure Restraint is enabled and pressure is active, eligible Visible Window and Background processes receive their configured priority, efficiency, and memory policy. Limit Background Processors independently limits hot Background processes and does not activate those softer controls. It owns the per-app CPU threshold, CPU Sets (Soft) or Processor Affinity (Hard) method, and processor selection: least-used across All, P-core, or E-core pools; fixed P/E/no-SMT topology; or an exact custom logical-processor mask. Only least-used selections expose a processor-limit percentage and rebalance. Recovery removes background processor limits before restoring the softer pressure-wide controls. Focus processes remain protected. Preset details keep tuning values editable without changing operational switches, and built-in presets remain read-only.
- Processor Power exposes separate editable A/C and Battery boost policy/mode values for Background Pressure and Focus and Launch. Background-dominant pressure selects Background Pressure; app launches and genuinely heavy Focus App demand select Focus and Launch. These values are part of Adaptive Engine presets rather than hidden runtime constants.
- Prefer one setting per row when possible.
- Use collapsible setting groups for advanced or multi-row settings.
- Put explanatory text in muted helper labels or info popovers, not large instruction blocks.
- Settings that affect Windows behavior should show conservative defaults and explicit enable controls.

### CPU Allocation

- Runtime-backed feature pages use a fixed status rail at the right edge instead of interrupting
  the settings flow. Every rail uses the same Running / Not running / Unknown state, process
  counts, and latest success/failure Action Log summary; unavailable snapshot metrics show an
  em dash rather than a fabricated zero. Action counts and latest outcomes are runtime telemetry,
  independent of Action Log visibility mode and its bounded visible history; clearing the Action
  Log resets both.
- By Foreground, By Running App, By CPU Load, By Activity, and By Time also use the right rail;
  every power-plan rail uses Status, Current power plan, Successful actions, and Failed actions.
  Power-plan actions are recorded only after the controller verifies the Windows plan transition.
- CPU Sets (Soft) and Processor Affinity (Hard) share that rail with compact Status and Presets
  tabs, matching the navigation panel structure.
- Core Presets use compact read-only rows; custom presets use the same row geometry with edit and
  delete actions. Resting rows have no fill or divider, and hover supplies the background. Add,
  edit, and view use the existing full-window modal style.
- Rule tables use Active, App Name, Executable Path, Focus, Visible Window, Background, and Actions
  columns. Each policy column selects a Core or custom preset; do not restore expandable per-rule
  CPU grids.

### Background Efficiency

- The master group owns the Background Efficiency Mode default. Foreground Detection and Visible
  Window Detection use the same collapsible toggle-plus-value pattern as Priority Control.
- Custom rules use the same compact Active, App Name, Executable Path, Focus, Visible Window,
  Background, and Actions table pattern as CPU allocation. Each policy column selects Default,
  Enabled, or Disabled; Default inherits the matching page-wide Efficiency Mode value.

### Priority Control and CPU Limiter

- Priority Control custom rules use the same three policy columns and independently select the
  priority for Focus, Visible Window, and Background.
- CPU Limiter uses the same three policy columns with Follow Default, Custom, and Unlimited. Focus,
  Visible Window, and Background have editable 1% to 100% page defaults. A rule's expandable
  details show a 1% to 100% Allowed CPU Time slider only for Custom tiers, and an inactive rule
  retains its configured values while those sliders are disabled. A 100% target is Unlimited.

## Localization

- All translatable user-facing strings should use `t!()` and locale files.
- Proper names, language-neutral symbols, and intentional internal table
  abbreviations such as `FG` / `BG` may remain literals. Do not hardcode other
  visible English strings in UI code.

## Safety UX

- Destructive, risky, or system-wide actions need explicit user intent.
- Do not auto-enable broad controls when adding a feature.
- Keep status messages and the action log useful for understanding what changed.
- Auto-exclusion fallback should be visible through existing rule/exclusion UI patterns instead of hidden background behavior only.

## What Not To Add

- No landing pages, hero sections, marketing copy, decorative illustrations, or oversized cards.
- No new design framework.
- No new icon system while Lucide assets are active.
- No custom table abstraction unless the current process-list helpers become impossible to maintain.
- No UI-only refactor that moves many unrelated pages at once.
