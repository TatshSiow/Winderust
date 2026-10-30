# Winderust Design Spec

This file describes the current app design direction. Use it when changing Wind UI code, adding pages, or cleaning UI helpers.

## Product Feel

Winderust is an operational Windows power and process-control utility. The UI should feel dense, calm, scannable, and repeat-use friendly. Do not turn it into a marketing page, hero layout, or decorative dashboard.

Prefer visible state, compact controls, and predictable rows over large illustrations or card-heavy presentation.

## App Shell

- Use the native Windows title bar and resize borders through Wind UI window configuration, with left navigation and a right work area.
- Windows owns the title bar and window buttons. Keep native close requests routed through the existing tray, unsaved-change, and shutdown handling.
- The sidebar is grouped by product area through `Page::sections()`. It is
  searchable when expanded and keeps a search action in its remembered compact
  icon rail; the quiet navigation-styled toggle stays below a divider in normal
  sidebar flow rather than floating over content. Keep icon and row geometry
  stable between expanded and compact states; selected and hover
  surfaces retain rounded compact-row geometry instead of being edge-clipped.
  Show a search field when expanded and a search action when compact.
- Each section has one native clickable navigation row with indented children at every width. Use one navigation pane. Keep Log, Settings and About below the scrollable feature sections; do not duplicate the section as a button and collapsible header.
- The main page area scrolls vertically and keeps content constrained with stable width behavior. The full-width bottom bar owns pending state, Exit, Discard and primary Save.
- Supported pages show their status/settings/preset panel by default. Put Hide side panel / Show side panel at the bottom, matching the left navigation toggle; collapsing leaves an icon-only right rail with the reopen action. Keep the labelled header panel action and primary Create and Trim now actions available. Remember one visibility choice through navigation, resizing and window recreation within the resident session. When the sidebar and full panel would leave less than 480 logical pixels for the editor, show the panel as a right-hand overlay instead of squeezing the editor. Use native layout/visibility without rebuilding the editor on resize; preserve input identity, text and focus.
- Navigation labels and page sections live in `src/ui.rs`; page rendering
  dispatch stays in `WinderustApp::page_view` in `src/ui/app.rs`.

## Layout Rules

- Use native Wind UI rows, columns, grids, cards, and forms with explicit weights on flexible main-axis children and constrained widths.
- Rows and table headers should have stable heights across state changes.
- Use fixed or computed widths for tables and policy columns. Do not let dynamic labels resize the process list.
- Constrain long process names, labels, and status values so they do not resize adjacent columns.
- Avoid nested cards. Cards are for repeated rows, setting groups, status blocks, popovers, and tool surfaces.
- Use the user's WindInput reference for visual refinement with native Wind UI controls: 8 px group and 4 px control radii, 16 px group padding, 8 px related-row gaps, 20 px between sections and 34 px minimum setting rows. Keep page headings on a 24 px inset and center forms within a maximum 1040 px body. Use blue-gray layered surfaces, fine group borders and contextual help for tuning details; keep safety warnings and validation visible.

## Components

- Reuse `src/ui/widgets.rs` for shared numeric and power-plan controls, and
  native collapsibles for expandable groups. Use built-in buttons,
  switches, checkboxes, dropdowns, segmented controls, color pickers, text inputs, and sliders for their natural control types.
- Main feature switches use toggle cards. Checkboxes are for rule activation and multi-selection.
- Keep primary settings visible. Collapse advanced details by default. Keep configuration editable when its main switch is off; read-only presets and invalid operations remain disabled.
- Sliders always reflect their input values, including when disabled. Put units in labels and use plus/minus icons.
- Use sliders, steppers, or numeric inputs for numeric settings.
- Use dropdowns for bounded option sets.
- Use icon buttons for compact repeated actions; text buttons are fine for clear primary commands such as save/import/export.

## Visual Language

- Reuse `design.rs` tokens and `widgets.rs` styles. Filled accent is reserved for explicit primary actions, including global and modal Save. Secondary buttons use native neutral OutlineSoft; destructive actions retain danger styling. Choose on-accent text by relative-luminance contrast in both themes, preserving the user's accent.
- The user's latest WindInput/Process List comparison guides the current refinement. See `DESIGN.md`. Use Segoe UI, 14 logical px regular body and setting labels, 12-13 px supporting text, 16 px semibold group headings and 22 px page titles. Dark workspace/navigation/groups use #131a26/#10161f/#1b2534. Preserve configured accent, native Windows frame, full-width Save/Discard footer and native controls. Use logical pixels rather than physical screenshot dimensions.
- Search anchors one 264 px navigation pane. Selected icon/text and a small marker use the accent over a restrained selected surface. Long labels may wrap rather than clip. Tabs and power-source selection remain native segmented controls. One current-page title carries hierarchy; full route context remains in its tooltip. Wind UI owns hover, focus and control painting. Optional status panels use 320 px, beside the editor in wide windows and over its right edge in compact windows.
- Choose toggle knobs and checkmarks for contrast against the accent color.

- Base surfaces are neutral and restrained. Accent color marks primary action, active navigation, selection, and important status.
- Respect `AppThemeMode`, `AccentColorSource`, and system accent behavior through the Wind UI theme and `settings_pages.rs` color helpers.
- Do not add purple/blue gradients, decorative blobs, glow backgrounds, or one-note palettes.
- Status colors should stay semantic: success for active/applied, warning for caution, danger for destructive or failed actions.
- Text hierarchy is compact: small labels, body rows, muted helper text. Avoid hero-scale text inside panels.

## Icons

- Reuse bundled Lucide SVGs through `src/ui/assets.rs`; page icons are mapped in
  `src/ui/navigation.rs`.
- Trace all asset consumers before changing Lucide generation or dependencies.
- Keep action icons at existing sizes, usually 12-18 px depending on row density.

## Motion and scrolling

- On / Off / Follow system is owned by Language and Appearance. Use built-in Wind UI animation for controls, chevrons, hover, and selection.
- Keep page, panel, and content-layout changes immediate for the software renderer; do not add large-surface motion without measuring it.
- Route scrolling through native `scrolling::scrollable`; retain per-page offsets through `scrolling::remember`. Keep Process List and Log virtualized and independent of animation subscriptions.
- Wind UI owns native card painting and damage tracking; do not restore a parallel repaint-group implementation.
- Idle controls must not schedule continuous redraws. Hidden-to-tray mode retains the existing runtime sampling and power-management policy.
- Preserve pressed native controls until mouse capture ends. Queue/coalesce structural host revisions, virtual-table row publication and responsive breakpoint changes during own-window capture; drain on the existing UI Tick after release. Keep model/runtime updates immediate and virtual rows coherent with their callback metadata. Do not substitute physical button polling or force capture release.

## Process List

- The process list is a dense table, not a card grid.
- Process names use native clickable containers with plain left-aligned text and existing icons, rather than filled primary buttons. Toolbar and selection actions stay compact; reserve filled accent for primary commands. Column-width settings place their label above the controls so words remain intact in the side panel.
- Keep process icon, grouped process row, PID/count, and policy columns visible and stable.
- Keep the User column at the right edge. Show the exact process token account when available. When Windows denies token access, show `Unavailable · S#` with an explanatory tooltip; identify kernel pseudo-processes as `Windows kernel`.
- Keep Windows-critical processes visible as read-only `Protected system process` rows in Process List, omit them from every Add process picker, and fail closed when criticality cannot be verified.
- For other inaccessible rows, show `Administrator required` before elevation and `Access denied` when Winderust is already elevated.
- When elevated, use normal Windows process permissions for discovery and automation while retaining every critical, built-in, identity, and protected-process barrier.
- With Advanced Controls enabled, keep Suspend/Resume present in the context menu and use the existing disabled style when the selected process cannot be safely suspended. Grouped rows apply the action to every captured process in the group. Keep unavailable App Suspension picker candidates visible, disabled, and explicitly labeled `Unavailable`. App Suspension safety must not disable unrelated process controls.
- Stacked Process List parent rows expose only `Stop process tree`, covering every process in the stack and the union of their descendants; their expanded sub-items expose both stop actions. Non-stacked rows always expose `Stop process` and expose `Stop process tree` only while the selected process has a child in the current process snapshot.
- Delete opens confirmation for the selected process or stack members only, without descendants. Tree termination remains an explicit context-menu action; Cancel submits no termination.
- Report `Efficiency mode` only when the process has both EcoQoS and `IDLE_PRIORITY_CLASS`, matching Task Manager rather than treating EcoQoS alone as full Efficiency mode.
- Filters, column visibility, and memory value/percentage controls belong in the Settings side panel.
- Language and Appearance owns the enabled-feature counts toggle. When enabled, expanded navigation shows numeric pills beside Winderust Features, Power Plan Control, Priority Control, and CPU Control based on each section's enabled modules; collapsed navigation keeps the pills hidden to preserve icon alignment.
- Language and Appearance separately owns the feature-card status toggle. When enabled, shared landing and search-result cards show Enabled or Disabled pills only for pages backed by a top-level feature switch; pages without a single enabled state remain unbadged.
- Editable policy cells should use inline dropdown controls. Avoid modal flows for simple policy edits.
- Use the native virtual table. Column width and auto-fit controls retain user adjustment through native controls; avoid recreating a custom table solely for divider dragging. Preserve identity, visibility, sorting, width, and keyboard tests.

## Settings Pages

- Adaptive Engine has a feature master switch. CPU Pressure Restraint, Limit Background Processors, and Dynamic Resource Zones own independent switches and retain their configuration while the feature is off.
- Adaptive Engine uses native compact grouped settings and dividers for shared detection/timing and processor-power controls. Tuning help uses native label tooltips and info markers; safety warnings and validation remain inline. Avoid repeated policy names in switch and disclosure headings. Status/Presets tabs remain above one panel scroller, without nesting a page inside the status scroller. Feature info participates in the main page scroll instead of reserving a separately scrolling header.
- Bind explanatory help to the actual visible label through the shared key-aware setting helpers. Wind UI's automatic full-title tooltip on form labels takes precedence over a parent-row tooltip, even when the duplicate title is suppressed. Keep label truncation and native tooltip rendering; verify hover through hit testing, not just tooltip metadata on the row.
- Adaptive Engine uses the right-rail Status / Presets tabs. Built-in presets are read-only; custom presets can be added, renamed, refreshed from the current tuning, and deleted. Presets exclude master enable switches, custom rules, exclusions, and the separate Background Efficiency feature. The preset editor uses a bounded native panel with a scrolling body and fixed footer; its state remains independent from the live page rendered behind it.
- Adaptive Engine and preset details share CPU Behaviour, Processor Power, and Priority Control tabs; the live page also exposes Custom Rules. Preset modal state is independent of the live page.
- Priority Control uses Focus / Visible Window / Background dropdown columns for rules. Defaults have one master row plus three context rows, placing each detection switch beside its selector. One Advanced disclosure holds tier-specific Preserve settings. Turning a row off does not lock its configuration. Runtime precedence and protection rules live in `25-runtime-contracts.md`.
- Processor Power uses separate setting cards. CPU Pressure Restraint and Limit Background Processors have independent switches; configuration remains editable while off.
- Processor Power exposes separate editable A/C and Battery boost policy/mode values for Background Pressure and Focus and Launch. Background-dominant pressure selects Background Pressure; app launches and genuinely heavy Focus App demand select Focus and Launch. These values are part of Adaptive Engine presets rather than hidden runtime constants.
- Prefer one setting per row when possible.
- Use collapsible setting groups for advanced or multi-row settings.
- Put explanatory text in muted helper labels or info popovers, not large instruction blocks.
- Settings that affect Windows behavior should show conservative defaults and explicit enable controls.

### CPU Allocation

- Runtime-backed feature pages show a collapsible status panel beside the settings instead of interrupting
  the settings flow. Every panel uses the same Running / Not running / Unknown state, process
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
  edit, and view use the bounded native editor panel with a scrolling body and fixed action footer.
- Rule tables use Active, App Name, Executable Path, Focus, Visible Window, Background, and Actions
  columns. Each policy column selects a Core or custom preset; do not restore expandable per-rule
  CPU grids.

### Background Efficiency

- The master switch remains separate from the Background Efficiency Mode default. Three context rows keep mode selectors visible, with Foreground Detection and Visible Window Detection switches beside their matching selectors. Advanced aggressiveness remains collapsible.
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
- Reuse the existing shared rule-table helpers instead of introducing another table design.
- No UI-only refactor that moves many unrelated pages at once.

## Panels, rules, and dialogs

- Side panels follow tabs (when present), main title, subtitle, then content. Subtitles stay outside cards. Put primary panel actions in the bottom footer.
- Use shared rule tables and icon edit/delete actions. Reuse existing modal layouts: name first, related settings together, actions at the bottom.
- Empty rules show their existing explanation beneath the process picker. Table headers and horizontal scrolling appear when actual rules exist. Home restores the original 3+2 dashboard and compact Main sections shortcuts; About groups identity/link buttons and update controls in two surfaces.
- Keep quiet history and breadcrumbs anchored at the top left. Put power-source selectors, feature help and panel actions in one native flow below the title; wrap only when the available width requires it.
- Quit confirmation is centered over a dimmed window. With unsaved changes show Cancel, Save and quit, and Quit without saving; otherwise show Cancel and Quit. Shutdown errors stay visible.
- Avoid duplicated help text between inline descriptions, info popovers, and How it works.

- Adaptive Engine CPU Behaviour presents shared detection/timing once, followed by three sibling policy groups. Zoning has its own live/preset expansion state and 1-99 foreground share. Limiter percentage always means background capacity; its allocation method stays visible when zones are enabled. Show zoning's throughput warning and verified status/counts, not configured enablement as proof of application.
