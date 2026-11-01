# Winderust Design Spec

This file describes the current app design direction. Use it when changing Iced UI code, adding pages, or cleaning UI helpers.

## Product Feel

Winderust is an operational Windows power and process-control utility. The UI should feel calm, scannable, and repeat-use friendly, with the light dashboard reference supplied on 2026-10-30 guiding all pages and navigation. Do not turn it into a marketing page, hero layout, or decorative dashboard.

Prefer visible state, compact controls, and predictable rows. Use pale blue-gray chrome, white finely outlined panels, and more panel spacing; preserve dense operational tables.

## App Shell

- Use the native Windows title bar and resize borders through Iced window decorations, with left navigation and a right work area.
- Windows owns the title bar and window buttons. Keep native close requests routed through the existing tray, unsaved-change, and shutdown handling.
- Use a permanent 88px icon-and-label rail. Main entries are 64px high; center each icon and its actual one- or two-line label together vertically with equal top and bottom insets.
  Group chevrons overlay the icon row's right edge. Keep the thin primary scrollbar within the outer padding. Log, Settings, and About stay below the scrollable primary list.
- Secondary navigation always floats in a 252px overlay adjacent to its parent, using the existing surface, border, and radius. Clamp it inside the window and scroll long lists. Opening or dismissing it never changes main-content geometry.
  Hover a parent for 250ms to open or switch its child menu. Clicking a rail item navigates directly to its page and dismisses pending or open flyouts. Keep it open over either area, allow 300ms to cross between them, and cancel outdated timers. Wake only for a pending delay; do not poll while idle.
  Choosing a page closes the flyout. Outside clicks, Escape, and window focus loss dismiss it without changing the current page or active rail highlight.
- Flyouts list only child pages. Use a plain section heading for context, with existing enabled-feature count chips and individual status dots. Do not include a parent-page navigation entry or pin action. There is no persistent secondary sidebar or saved navigation mode. Category landing pages stay accessible directly from the rail and expose their existing child-page cards.
- Tab focuses rail entries; Up/Down move rail focus, Enter/Space opens the focused page, and Right Arrow opens a category's child flyout with its first child focused. Up/Down/Tab move through child pages; Enter selects; Escape/Left dismisses and returns focus to the parent. Show focus outlines, reveal offscreen parents, and keep focused items visible. Touch users can navigate through category landing-page cards without hover.
- Global search opens a focused palette above the page. Dismiss floating navigation
  before opening search. Follow the compact Fumadocs reference: a 640px upper-center panel, borderless search field, Esc keycap, and compact category-filter row over a dimmed backdrop. Match the filter row to the search panel background. Esc uses the standard shared button; the dropdown uses the standard shared Select with project colors and interaction styles. Tint the selected compact result to identify the Enter target. Vertically center icons, one- or two-line labels, and chevrons within each fixed-height result row. Keep the search panel shadow-free: upstream tiny-skia quad shadows draw outside partial repaint damage and accumulate black patches. Empty All searches omit the result list; typing or choosing a category reveals inset results. Results show function names and parent sections. Enter
  opens the selected result; Up/Down move selection and scroll it into view. New queries and filters select the first result. Escape, Close, or clicking outside dismisses the palette.
- Search defaults to Ctrl+K or / within the app window, respecting keys captured by text inputs and menus. Settings > Winderust Behaviour provides primary and alternate shortcut recording, clearing, and default restoration through normal Save. The rail Search entry matches other navigation item heights, icon/label spacing, and vertical centering. Navigation entries, including Search, have no hover tooltips. Keep Search to its icon and label; give Search only a subtle tint with neutral text while the palette is open, retaining the current page as the primary selection.
- The rail stays 88px wide plus its divider. Keep content geometry independent of flyout state and the right status rail independent.
- The main page area scrolls vertically and keeps content constrained with stable width behavior.
- Navigation labels and page sections live in `src/ui.rs`; page rendering
  dispatch stays in `WinderustApp::page_view` in `src/ui/app.rs`.

## Layout Rules

- Use Iced rows and columns with explicit `Fill`, `Shrink`, and constrained widths for flexible children.
- Rows and table headers should have stable heights across state changes.
- Use fixed or computed widths for tables and policy columns. Do not let dynamic labels resize the process list.
- Constrain long process names, labels, and status values so they do not resize adjacent columns.
- Avoid nested cards. Cards are for repeated rows, setting groups, status blocks, popovers, and tool surfaces.
- Use shared 10px surface and 6px control corner radii consistently. Do not introduce large rounded marketing panels.

## Components

- Reuse `src/ui/widgets.rs` for shared numeric and power-plan controls, and
  the shared `setting_group` for collapsible groups. Use Iced buttons,
  checkboxes, pick lists, text inputs, and sliders for their natural control types.
- Main feature switches use toggle cards. Checkboxes are for rule activation and multi-selection.
- Collapse expandable cards by default. Keep configuration editable when its main switch is off; read-only presets and invalid operations remain disabled.
- Sliders always reflect their input values, including when disabled. Put units in labels and use plus/minus icons.
- Use sliders, steppers, or numeric inputs for numeric settings.
- Use dropdowns for bounded option sets.
- Use icon buttons for compact repeated actions; text buttons are fine for clear primary commands such as save/import/export.

## Visual Language

- Reuse `design.rs` tokens and `widgets.rs` styles. Buttons share 6px corners, 14px labels, and stable 1px outlines. Standard text actions and utility icons are 32px high; text actions use 12px horizontal padding through the shared action helper. Primary actions use a solid accent fill; secondary actions use a neutral outline with accent text; tertiary actions use a neutral outline and text. Quiet utilities show hover feedback; destructive actions use danger colors; persistent choices use accent outlines and tint. Preserve explicit dense-row and navigation geometry, native input, disabled states, and theme/accent support. Keep action feedback distinct from persistent selection.
- Navigation selection uses a rounded vertical pill. All page, panel, and dialog tabs use the same compact segmented design: 32px height, widths that fit their centered 14px semibold labels, one neutral outline, no outer inset or gap, and an accent-tinted, outlined selected segment. Weekday selectors share this design and retain multiple selection. Plugged In / On Battery retains its live-source dot independently of the selected editing profile. Respect the current theme and accent without shadows or underlines.
- Choose toggle knobs and checkmarks for contrast against the accent color.

- The navigation rail and right panel use docked chrome with square outer corners, workspace-facing thin dividers, and no gaps between shell surfaces. Floating navigation uses the shared outlined surface. Cards, rules tables, dense table frames, settings groups, and settings dialogs share the surface outline (1px neutral at 75% opacity) and 10px radius. Keep dense table frames outside horizontal scrolling and preserve their borderless tiled interior for partial repaint efficiency.
- Base surfaces use cool pale neutrals in light mode and slate tones in dark mode; panels have subtle 1px outlines. Accent color marks primary action, active navigation, selection, and important status.
- Respect `AppThemeMode`, `AccentColorSource`, and system accent behavior through the Iced theme and `settings_pages.rs` color helpers.
- Do not add purple/blue gradients, decorative blobs, glow backgrounds, or one-note palettes.
- Status colors should stay semantic: success for active/applied, warning for caution, danger for destructive or failed actions.
- Text hierarchy is compact: small labels, body rows, muted helper text. Avoid hero-scale text inside panels.

## Icons

- Reuse bundled Lucide SVGs through `src/ui/assets.rs`; page icons are mapped in
  `src/ui/navigation.rs`.
- Trace all asset consumers before changing Lucide generation or dependencies.
- Keep action icons at existing sizes, usually 12-18 px depending on row density.

## Motion and scrolling

- On / Off / Follow system is owned by Language and Appearance. Use shared `iced_anim` transitions for small controls, chevrons, hover, and selection.
- Keep page, panel, and content-layout changes immediate for the software renderer; do not add large-surface motion without measuring it.
- Route scrolling through `scrolling::scrollable`. Keep Process List and Log virtualized and independent of animation subscriptions.
- Shared expandable cards group their repaint regions. Preserve this when changing card composition.
- Idle controls must not schedule continuous redraws. Hidden-to-tray mode retains the existing runtime sampling and power-management policy.

## Process List

- The process list is a dense table, not a card grid. Refresh automatically about once per second only while its page is shown and the window is not hidden to tray. Its command row contains search and process count; omit manual refresh and population pause controls.
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
- Language and Appearance owns the enabled-feature counts toggle. When enabled, the secondary navigation header shows a numeric pill for Winderust Features, Power Plan Control, Priority Control, and CPU Control based on the section's enabled modules; the primary rail stays unbadged.
- Language and Appearance separately owns the feature-card status toggle. When enabled, shared landing and search-result cards show Enabled or Disabled pills only for pages backed by a top-level feature switch; pages without a single enabled state remain unbadged.
- Editable policy cells should use inline dropdown controls. Avoid modal flows for simple policy edits.
- Columns are resizable and initially fit their content. Double-click a divider to auto-fit; the last visible column fills remaining width. Preserve the layout tests.

## Settings Pages

- Adaptive Engine is the parent of Adaptive Engine. Adaptive Engine has no separate master switch;
  CPU Pressure Restraint, Limit Background Processors, and Dynamic Resource Zones own independent switches and retain their
  configuration while Adaptive Engine is off.
- Adaptive Engine uses the right-rail Status / Presets tabs. Built-in presets are read-only; custom presets can be added, renamed, refreshed from the current tuning, and deleted. Presets exclude master enable switches, custom rules, exclusions, and the separate Background Efficiency feature. The preset editor spans the available modal width, and its control state remains independent from the live page rendered behind it.
- Adaptive Engine and preset details share CPU Behaviour, Processor Power, and Priority Control tabs; the live page also exposes Custom Rules. Preset modal state is independent of the live page.
- Priority Control uses Focus / Visible Window / Background dropdown columns and expandable detail cards. Turning a row off does not lock its configuration. Runtime precedence and protection rules live in `25-runtime-contracts.md`.
- Processor Power uses separate setting cards. CPU Pressure Restraint and Limit Background Processors have independent switches; configuration remains editable while off.
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
- Reuse the existing shared rule-table helpers instead of introducing another table design.
- No UI-only refactor that moves many unrelated pages at once.

## Panels, rules, and dialogs

- Side panels follow tabs (when present), main title, subtitle, then content. Subtitles stay outside cards. Put primary panel actions in the bottom footer.
- Use shared rule tables and icon edit/delete actions. Reuse existing modal layouts: name first, related settings together, actions at the bottom.
- Keep breadcrumbs anchored at the top left. Power-source selectors and How it works align right on the same row.
- Quit confirmation is centered over a dimmed window. With unsaved changes show Cancel, Save and quit, and Quit without saving; otherwise show Cancel and Quit. Shutdown errors stay visible.
- Avoid duplicated help text between inline descriptions, info popovers, and How it works.

- Adaptive Engine CPU Behaviour presents shared detection/timing once, followed by three sibling policy groups. Zoning has its own live/preset expansion state and 1-99 foreground share. Limiter percentage always means background capacity; its allocation method stays visible when zones are enabled. Show zoning's throughput warning and verified status/counts, not configured enablement as proof of application.

## Current reference layout

- The user approved the 2026-10-30 light dashboard direction across all UI and the sidebar; `DESIGN.md` records the reference mapping.
- The later two-panel sidebar reference uses an 88px icon-and-label rail, a separate closable category menu, and tinted selected entries. Keep all existing categories and global search.
- Use 16px card padding, 12px card gaps, regular Segoe UI body text, semibold headings, and prominent dashboard values.
- Use one 18px breadcrumb heading on every page: Home > section > current page, omitting unused levels. Ancestors are clickable; the current page is semibold text. Home shows only Home. Wrap the path in narrow windows; do not repeat the current page as a separate title.
- Home uses a two-by-two chart grid above Enabled Features and Recent Logs. Shared content is capped at 1440px.
- Dense virtual process and log tables retain borderless tiled backgrounds and existing row geometry.
- Keep System/Light/Dark and custom/system accent behavior. Do not infer new telemetry or optimization guarantees from the reference image.

## Home dashboard

- Follow the latest dashboard reference: a 2x2 CPU/RAM/Disk I/O/Network I/O chart grid, then equal-width Enabled Features and Recent Logs panels. Adapt card heights to available content height. Use four metric columns at widths of at least 1280px and heights below 680px, two columns from 720px wide, otherwise one column. Omit redundant dashboard subtitles at every size. Preserve readable minimum card sizes and retain page scrolling only when they cannot fit; the feature and log lists scroll independently.
- Charts show 60 seconds of real history with axes and legends. Hover shows a floating tooltip with the actual sample timestamp and named series values, plus a dashed crosshair and point markers. Clamp the tooltip to the viewport, including compact charts. CPU headers show Usage on the left and Frequency on the right. Render only recorded chart samples: missing history stays blank, actual zero samples remain visible. RAM headers show Usage with a Used label and used/total capacity on the left and Available on the right. Disk and Network I/O headers show their two named, curve-colored metrics on the left and Total on the right. Network uses decimal Mb/s. CPU frequency is explicitly relative on the shared percentage scale, with the current GHz value shown separately.
- Enabled Features and Recent Logs preserve card height and fixed headers in both empty and populated states. Enabled Features has no View All footer; Recent Logs retains its Details footer. Center empty messages in the remaining content area; populated lists use inset scrolling with space for the scrollbar and links to their full pages. Recent Logs uses actual Action Log outcomes, never fabricated INFO/WARN events. Keep unavailable telemetry and empty logs honest.
- Keep category shortcuts in navigation and global search; omit the old dashboard shortcut grid.
- Dashboard sampling runs only while Home is selected and the window is not hidden to tray. Do not add manual Pause/Resume metrics controls; background automation retains its independent monitoring.
