# Winderust Design Spec

This is the canonical app design guide for Iced UI changes, new pages, and shared
controls. Use [Architecture](architecture.md) for ownership and navigation,
[Runtime contracts](runtime-contracts.md) for behavior, and
[Project scope](project-scope.md) for product boundaries.

The audience is Windows power users and gamers configuring application performance
and efficiency on desktops and laptops, in standard or administrator sessions.
Use calm, direct language that states scope, consequence, and actual applied state.
Use decimal byte units consistently for memory, storage, thresholds, and byte rates:
B, KB, MB, GB, and TB, with each step equal to 1,000 of the preceding unit.
Use byte rates such as KB/s and MB/s for both disk and network throughput;
network threshold selectors also offer only byte units.
Present CPU Sets (Soft) as recommended and Processor Affinity (Hard) as strict.
Preserve keyboard reachability, focus, contrast, semantic labels, icon tooltips,
and reduced-motion behavior. Use the shared `widgets::tooltip` helper for opaque
tooltips matching dropdown menus: themed fill/text/border, 6px control corners,
8px padding, and 13px secondary text. Chart tooltips use the same surface while
retaining their inset layout and timestamp/series text hierarchy.

## Product Feel

Winderust is an operational Windows power and process-control utility. The UI should feel calm, scannable, and repeat-use friendly, with the light dashboard reference supplied on 2026-10-30 guiding all pages and navigation. Do not turn it into a marketing page, hero layout, or decorative dashboard.

Prefer visible state, compact controls, and predictable rows. Use restrained accent-derived chrome, light/dark finely outlined panels, and more panel spacing; preserve dense operational tables.

- Keep the workflow predictable. Home shows current conditions, feature pages edit
  rules, the side panel explains status and options, and Action Log records outcomes.
- Make settings easy to discover through navigation and search. Put common actions
  first and reveal advanced options where users need them. Keep essential consequences
  beside the control and longer explanations in Info.
- Distinguish a switch's configured state, an unsaved edit, and the last verified
  result from Windows. Direct commands apply when invoked. Configuration edits
  use the Save / Discard card floating at the bottom right of the work area.
  Save commits the draft. Discard clears pending edits.
- Make core tasks keyboard-operable with visible focus and predictable focus return.
  Give controls meaningful Windows UI Automation names and states. Use at least
  4.5:1 contrast for normal text across supported themes and accent choices. Use
  labels alongside color for status and selection. Follow the
  [Windows accessibility checklist](https://learn.microsoft.com/en-us/windows/apps/design/accessibility/accessibility-checklist).

## App Shell

- Use a compact themed title bar with the app icon/name and minimize, maximize/restore, and close controls. Follow the active Light, Dark, AMOLED, and accent palette; disable native title-bar decorations. Keep the left navigation and right work area beneath the title bar.
- Iced owns native dragging, resizing, minimizing, and maximizing. The caption supports drag and double-click maximize/restore; restored windows expose edge/corner resize targets. Both custom and native close requests use the existing tray, unsaved-change, and shutdown handling. Keep window controls available above dialogs.
- Use a permanent 88px icon-and-label rail. Main entries are 64px high; center each icon and its actual one- or two-line label together vertically with equal top and bottom insets.
  Group chevrons overlay the icon row's right edge. Search, primary items, and bottom shortcuts share the same 76px item width and centerline. Keep the thin primary scrollbar within the existing 6px outer padding; do not apply the wider page gutter to the navigation rail. Log, Settings, and About stay below the scrollable primary list.
- Secondary navigation always floats in a 252px overlay adjacent to its parent, using the existing surface, border, and radius. Clamp it inside the window and scroll long lists. Opening or dismissing it never changes main-content geometry.
  Hover a parent for 250ms to open or switch its child menu. Clicking a rail item navigates directly to its page and dismisses pending or open flyouts. Keep it open over either area, allow 300ms to cross between them, and cancel outdated timers. Wake only for a pending delay; do not poll while idle.
  Choosing a page closes the flyout. Outside clicks, Escape, and window focus loss dismiss it without changing the current page or active rail highlight.
- Flyouts list only child pages. Use a plain section heading for context, with existing enabled-feature count chips and individual status dots. Do not include a parent-page navigation entry or pin action. There is no persistent secondary sidebar or saved navigation mode. Category landing pages stay accessible directly from the rail and expose their existing child-page cards.
- Tab focuses rail entries; Up/Down move rail focus, Enter/Space opens the focused page, and Right Arrow opens a category's child flyout with its first child focused. Up/Down/Tab move through child pages; Enter selects; Escape/Left dismisses and returns focus to the parent. Show focus outlines and scroll only as far as needed to reveal focused or newly selected sections. Touch users can navigate through category landing-page cards without hover.
- When the window gets shorter, keep the focused primary rail item in view. If focus is elsewhere, reveal the active primary section. Resizing leaves keyboard focus where it is.
- Global search opens a focused palette above the page. Dismiss floating navigation
  before opening search. Follow the compact Fumadocs reference: a 640px upper-center panel, borderless search field, Esc keycap, and compact category-filter row over a dimmed backdrop. Match the filter row to the search panel background. Esc uses the standard shared button; the dropdown uses the standard shared Select with project colors and interaction styles. Tint the selected compact result to identify the Enter target. Vertically center icons, one- or two-line labels, and chevrons within each fixed-height result row. Keep the search panel shadow-free: upstream tiny-skia quad shadows draw outside partial repaint damage and accumulate black patches. Empty All searches omit the result list; typing or choosing a category reveals inset results. Results show function names and parent sections. Enter
  opens the selected result; Up/Down move selection and scroll it into view. New queries and filters select the first result. Escape, Close, or clicking outside dismisses the palette.
- Search defaults to Ctrl+K or / within the app window, respecting keys captured by text inputs and menus. Settings > Winderust Behaviour provides primary and alternate shortcut recording, clearing, and default restoration through normal Save. The rail Search entry matches other navigation item heights, icon/label spacing, and vertical centering. Navigation entries, including Search, have no hover tooltips. Keep Search to its icon and label; give Search only a subtle tint with neutral text while the palette is open, retaining the current page as the primary selection.
- Keyboard Shortcuts in Winderust Behaviour includes the optional system-wide
  Show Winderust shortcut. Use the same keyboard recording and Clear controls as
  Search. Help explains the valid combinations and that enabling the shortcut
  keeps Winderust in the notification area. Show conflicts and registration
  failures in text. Save commits the selection and Esc cancels recording.
- The rail stays 88px wide plus its divider. Keep content geometry independent of flyout state and the right status rail independent.
- Center the shared header and content canvas within the work area beside the primary rail and any open side panel, with shared padding and a 1440px width cap. Every page reserves the same 48px breadcrumb area and 32px toolbar, including pages without power-source selectors, so page content starts at a stable position.
- The right side panel is a 320px dock beside the editor, reserving its own width without covering content. Both areas remain interactive. Start closed and retain its open state throughout navigation, search, focus changes, and settings edits. Close only through panel controls. Pages without panel content use the full canvas and preserve the session's open preference for the next supported page. Keep existing tabs, scrolling, and actions. Dialogs cover both areas. There is no collapsed strip or persisted mode setting.
- The side panel Close button returns focus to the toolbar's Side panel button so keyboard navigation can continue in the editor.
- Navigation labels and page sections live in `src/ui.rs`; page rendering
  dispatch stays in `WinderustApp::page_view` in `src/ui/app.rs`.

## Layout Rules

- Use Iced rows and columns with explicit `Fill`, `Shrink`, and constrained widths for flexible children.
- Rows and table headers should have stable heights across state changes.
- Rules tables fill the same canvas width as settings cards. Use fixed or computed minimum widths for dense tables and policy columns, with horizontal scrolling when the dock leaves too little room. Do not let dynamic labels resize the process list.
- Keep the last rule reachable with the dock open at the minimum window size,
  including when both scrollbars are needed. Float the two-row Save card at
  the bottom right with standard-height actions. It can overlap the editor
  within its own bounds. Keep the same editor height and scroll range during
  edits. Process List hides the card while its context menu or dialog is open
  and keeps the draft.
- Constrain long process names, labels, and status values so they do not resize adjacent columns.
- Group related settings beneath each section heading in one card, with shared internal dividers between complete rows. Every grouped card needs a concise, localized category heading outside its outline. Apply this to pages, preset editors, rule dialogs, and process details; independent collapsible groups use their own headers. Keep each label, description, and control together; avoid a separate card for every setting. Preserve independent collapsible feature groups.
- Avoid nested cards. Cards are for repeated rows, setting groups, status blocks, popovers, and tool surfaces.
- Centered dialogs, Search, and picker popups are modal: shield background hover,
  pointer/touch actions, scrolling, typing, focus operations, and existing background
  overlays. Outside presses may dismiss a popup but must never activate the content
  behind it. Keep window chrome usable and preserve background scroll/input state.
- Escape discards By Time and By CPU Load rule drafts, including while a text
  field has focus. If a picker is open, Escape closes the picker first.
- Use shared 10px surface and 6px control corner radii consistently. Do not introduce large rounded marketing panels.
- Use the shared scroll helpers and scrollbar settings for both axes. Keep a permanent gutter for page, tab, and table scrolling, including when the thumb is hidden, so page/tab changes cannot shift content; never paint tracks over cards, controls, or table cells. Use a transparent track and a slim rounded neutral thumb, with stronger hover/drag contrast and no accent-colored strip.
- Floating menus and hover popups use balanced surface padding. Reserve scrollbar
  space only when popup content overflows, and group each floating surface's repaint
  region so hover changes redraw its background, outline, and contents together.

## Components

- Reuse `src/ui/widgets.rs` for shared numeric and power-plan controls, and
  the shared `setting_group` for collapsible groups. Use Iced buttons,
  checkboxes, pick lists, text inputs, and sliders for their natural control types.
- Main feature switches use toggle cards. Checkboxes are for rule activation and multi-selection.
- Keep primary settings visible and put related controls in one card. Use
  disclosure for optional detail and settings subordinate to a visible toggle.
  Destination cards share a height and use a right chevron to open a page.
  Expandable groups use a down chevron when closed and an up chevron when open.
  Settings cards grow to fit their rows and localized help.
- Place the rule picker and its validation feedback in a shared settings card
  below the rules heading. Keep dense rule lists in their own table surface.
- Action and toggle cards use their full surface, including padding and blank space, as the hit target. Expandable cards toggle only from the full header, including its padding; expanded content never toggles the group. Keep one 16px inset for headers and bodies. Nested controls retain their own actions, cursor, and input handling, and disabled controls stay inactive.
- Collapse expandable cards by default. Keep configuration editable when its main switch is off; read-only presets and invalid operations remain disabled.
- Sliders always reflect their input values, including when disabled. Put units in labels and use plus/minus icons.
- Use the shared numeric setting row. Keep the label and unit on the left and
  the controls on the right. Stack the label above the right-aligned controls
  when the card is narrow.
- Use a slider with minus, plus, and an editable value for ranges users may want
  to explore. Use a compact stepper for precise integers and a numeric input
  when the field accepts decimals or a blank override.
  Match the height, neutral fill, and outline of the step buttons and value field.
- Group the two endpoints of an editable range and show validation beside the pair.
- Use dropdowns for bounded option sets.
- Use icon buttons for compact repeated actions; text buttons are fine for clear primary commands such as save/import/export.

## Visual Language

- Reuse `design.rs` tokens and `widgets.rs` styles. Buttons share 6px corners, 14px labels, and stable 1px outlines. Standard text actions and utility icons are 32px high; text actions use 12px horizontal padding through the shared action helper. Primary actions use a solid accent fill; secondary actions use a neutral outline with accent text; tertiary actions use a neutral outline and text. Quiet utilities show hover feedback; destructive actions use danger colors; persistent choices use accent outlines and tint. Preserve explicit dense-row and navigation geometry, native input, disabled states, and theme/accent support. Keep action feedback distinct from persistent selection.
- Navigation selection uses a rounded vertical pill. All page, panel, and dialog tabs use the same compact segmented design: 32px height, widths that fit their centered 14px semibold labels, one neutral outline, no outer inset or gap, and an accent-tinted, outlined selected segment. Tab labels retain normal theme text colors, including the side panel's Info / Status / Presets tabs; selection changes the tint and outline rather than the text color. Weekday selectors share this design and retain multiple selection. Plugged In / On Battery retains its live-source dot independently of the selected editing profile. Respect the current theme and accent without shadows or underlines.
- Choose toggle knobs and checkmarks for contrast against the accent color.

- The navigation rail and docked right panel use edge-aligned chrome with square outer corners, workspace-facing thin dividers, and no gaps between shell surfaces. Floating navigation uses the shared outlined surface. Cards, rules tables, dense table frames, settings groups, and settings dialogs share the surface outline (1px neutral at 75% opacity) and 10px radius. Keep dense table frames outside horizontal scrolling and preserve their borderless tiled interior for partial repaint efficiency.
- Base surfaces use cool pale neutrals in light mode and slate tones in dark mode; panels have subtle 1px outlines. Inputs, dropdowns, and menus use the neutral control surface, distinct from cards: slightly lighter in Dark and AMOLED, darker in Light. Keep control hover fills opaque, and internal settings dividers subtler than outer card borders. AMOLED workspace and dock remain pure black. Accent color marks primary action, active navigation, selection, and important status.
- Respect `AppThemeMode`, `AccentColorSource`, and system accent behavior through the Iced theme and `settings_pages.rs` color helpers. Derive workspace, navigation, card/field, button, border, divider, and muted-text colors from the selected accent using restrained grayscale tints in the shared palette. Chart series and informational Process List colors also follow accent roles. Keep body/help text readable, semantic success/warning/danger distinct, and existing contrast rules for accent fills and marks. Accent changes must refresh the complete theme immediately, including cached chart/table surfaces.
- Language and Appearance offers System, Light, Dark, and AMOLED. AMOLED uses a pure-black workspace and docked navigation with near-black accent-tinted cards and controls. System follows Windows light, dark, and contrast themes. During a Windows contrast theme, System uses the Windows window and selection color pairs and holds the saved accent for when the contrast theme is turned off. Composite rows retain readable text and icons with an outlined selection. Menus use a paired fill and foreground, and keyboard focus uses a separate outline.
- Do not add purple/blue gradients, decorative blobs, glow backgrounds, or one-note palettes.
- Status colors should stay semantic: success for active/applied, warning for caution, danger for destructive or failed actions.
- Text hierarchy is compact: small labels, body rows, muted helper text. Avoid hero-scale text inside panels.

## Icons

- Reuse bundled Lucide SVGs through `src/ui/assets.rs`; page icons are mapped in
  `src/ui/navigation.rs`.
- Draw bundled icons as centered vector paths so their origins follow the same
  fractional layout coordinates as the surrounding controls.
- Bundle brand marks alongside the existing assets when a link needs its service's
  identity; About uses the official Discord symbol from its branding page.
- Trace all asset consumers before changing Lucide generation or dependencies.
- Verify consumers before trimming `icondata_core` or `icondata_lu`.
- Keep action icons at existing sizes, usually 12-18 px depending on row density.

Accent fills, checkbox marks, and switch thumbs use white in Light. In dark themes,
use white when weighted RGB brightness (0.299R + 0.587G + 0.114B) is below 140,
otherwise #111111. Windows appearance comes from `UISettings.GetColorValue`;
explicit theme and custom accent preferences override it. Inactive navigation icons
and resting control borders use neutral tones. Keep card outlines stable across
hover/press; background shading supplies feedback.

## Motion and scrolling

- On / Off / Follow system is owned by Language and Appearance. Use shared `iced_anim` transitions for small controls, chevrons, hover, and selection.
- Keep page, panel, and content-layout changes immediate for the software renderer; do not add large-surface motion without measuring it.
- Route scrolling through `scrolling::scrollable`. Keep Process List and Log virtualized and independent of animation subscriptions.
- Shared expandable cards group their repaint regions. Preserve this when changing card composition.
- Idle controls must not schedule continuous redraws. Hidden-to-tray mode retains the existing runtime sampling and power-management policy.

## Process List

- The process list is a dense table, not a card grid. Refresh automatically about once per second only while its page is shown and the window is not hidden to tray. Its command row contains search and process count; omit manual refresh and population pause controls.
- Page visits and periodic ticks share the same refresh deadline; quick returns reuse the current rows. Load the power-plan catalog when the selected process's Rulesets panel opens, including when returning to that open panel.
- Rulesets shows the power profile that receives its edits.
- Copy executable path is available in the context menu and details header when
  the selected path can be represented as text. Copy the path exactly as observed.
- Keep process icon, grouped process row, PID/count, and policy columns visible and stable.
- A grouped CPU or memory total is available only when every displayed member has that measurement.
- Keep the User column at the right edge. Show the exact process token account when available. When Windows denies token access, show `Unavailable · S#` with an explanatory tooltip; identify kernel pseudo-processes as `Windows kernel`.
- Keep Windows-critical processes visible as read-only `Protected system process` rows in Process List, omit them from every Add process picker, and fail closed when criticality cannot be verified.
- For other inaccessible rows, show `Administrator required` before elevation and `Access denied` when Winderust is already elevated.
- When elevated, use normal Windows process permissions for discovery and automation while retaining every critical, built-in, identity, and protected-process barrier.
- With Advanced Controls enabled, keep Suspend/Resume present in the context menu and use the existing disabled style when the selected process cannot be safely suspended. Grouped rows apply the action to every captured process in the group. Keep unavailable App Suspension picker candidates visible, disabled, and explicitly labeled `Unavailable`. App Suspension safety must not disable unrelated process controls.
- Stacked Process List parent rows expose only `Stop process tree`, covering every process in the stack and the union of their descendants; their expanded sub-items expose both stop actions. Non-stacked rows always expose `Stop process` and expose `Stop process tree` only while the selected process has a child in the current process snapshot.
- The rows share one Tab stop. Up and Down move through rows, Enter opens details, and Space expands or collapses the focused group.
- Shift+F10 or the Menu key opens the context menu at the focused row. Tab,
  Shift+Tab, and Up and Down move between its actions, Enter activates the focused
  action, and Escape closes the menu and returns focus to the table.
- With the rows focused, Delete opens confirmation for the selected process or stack members without descendants. Tree termination remains an explicit context-menu action. Escape or Cancel dismisses confirmation.
- If the focused row moves offscreen, opening its context menu, Enter, Space, or Delete first scrolls it
  into view. Press the key again to act on the visible row.
- Keep Stop process and Stop process tree consistent from their initiating action through the confirmation title and button. Tree confirmations name child processes, and both warnings retain the unsaved-work consequence.
- Report `Efficiency mode` only when the process has both EcoQoS and `IDLE_PRIORITY_CLASS`, matching Task Manager rather than treating EcoQoS alone as full Efficiency mode.
- Filters, column visibility, and memory value/percentage controls belong in the Settings side panel.
- The default Working Set column shows resident RAM in decimal MB or as a percentage of physical RAM. The optional Private Commit column is available under Columns and shows privately committed memory in decimal MB. Sort both columns by bytes and keep missing measurements distinct from zero.
- The optional CPU Time column shows cumulative user and kernel time as hours, minutes, and seconds. Groups total current members only when every member has a sample. Sort by the full measurement and make unmeasured values visible.
- GPU Usage is an optional column under Columns. It starts hidden and samples
  only while enabled on the visible Process List. Show the busiest GPU engine
  for each process as a percentage and sort by the unrounded value. A dash
  indicates an unavailable reading or a grouped parent row.
- Dedicated GPU Memory and Shared GPU Memory are optional columns under Columns.
  Both start hidden. Collect readings on the Process List refresh only while at
  least one is visible. Show decimal MB and sort by bytes. Show a dash for an
  unavailable reading and `0 MB` for a measured zero. Show readings on individual
  process rows. Leave grouped totals unavailable because Windows can attribute
  the same GPU allocation to multiple processes.
- Language and Appearance owns the enabled-feature counts toggle. When enabled, the secondary navigation header shows a numeric pill for Watchdog, Power Plan Control, Priority Control, and CPU Control based on the section's enabled modules. The primary rail stays unbadged.
- Language and Appearance separately owns the feature-card status toggle. When enabled, landing cards backed by a feature switch show Enabled or Disabled in the theme's normal text color.
- Editable policy cells should use inline dropdown controls. Avoid modal flows for simple policy edits.
- Columns are resizable and initially fit their content. Double-click a divider to auto-fit; the last visible column fills remaining width. Preserve the layout tests.

## Settings Pages

- Settings and its Winderust Behaviour, Language and Appearance, and Experimental
  Features pages have no side panel or Side panel button. Keep help inline.
- The Settings Files card explains each file action before its picker opens. Export includes
  unsaved settings edits. A valid import replaces the active settings immediately and requires
  a clean settings draft.
- Within Adaptive Engine, CPU Behaviour has no separate master switch;
  CPU Pressure Restraint, Limit Background Processors, and Dynamic Resource Zones own independent switches and retain their
  configuration while Adaptive Engine is off.
- In short Adaptive Engine workspaces, the feature toggle, preset picker, and
  editor tabs scroll with the selected tab. Keep the power-source toolbar and
  page header outside this scroll area.
- Adaptive Engine uses the right-rail Info / Status / Presets tabs. Built-in presets are read-only. Custom presets can be added, renamed, refreshed from the current CPU Behaviour tuning, and deleted. The live page also exposes Custom Rules. The preset editor spans the available modal width and keeps its own draft.
- Priority Control uses Focus / Visible Window / Background dropdown columns and expandable detail cards. Turning a row off does not lock its configuration. Runtime precedence and protection rules live in [Runtime contracts](runtime-contracts.md).
- Adaptive Processor Power lives under Power Plan Control. Its Plugged In and On Battery editors each have an enable switch, off by default, with a Preset dropdown below it. Both editors share the preset library. The dropdown stages tuning for the selected source. The Presets tab opens a read-only preview of the base policy and demand values for that source. Use preset stages the preview, and closing it preserves existing edits. Add preset names a copy of the selected source's tuning. List edits and tuning use the shared Save and Discard flow. Configuration remains editable while the feature is off.
- The Base Processor Policy tab shows the Idle settings and the baseline inherited by higher-demand profiles. The Demand Profiles tab explains automatic profile selection and shows one boost policy and one boost mode each for Background Pressure and Focus and Launch.
- The Adaptive Processor Power Status rail reports live ownership, errors, and restoration. The source profile selected for editing can differ from the live power source.
- Prefer one setting per row when possible.
- Use collapsible setting groups for advanced or multi-row settings.
- Multi-setting cards use subtle 1px neutral, theme-aware dividers between complete settings, with even inset spacing. Keep labels, descriptions, and associated controls together. Expanded groups separate their header and body with the same divider; single-setting cards have no extra leading/trailing rules. Use shared widgets helpers and preserve dense table styling.
- Show concise setting descriptions directly beneath their labels in readable 13px secondary text, with controls aligned on the right. Let rows and group headers grow when labels or descriptions wrap; never clip help with a fixed height. Use the shared setting-title layout for live pages and preset dialogs. Keep essential consequences inline and longer page-level reference in the right sidebar's Feature info section. Preserve icon-action and truncated-data tooltips, localization, units, and dense process/log table geometry.
- Settings that affect Windows behavior should show conservative defaults and explicit enable controls.
- Advanced Power Plan Tuning shows Energy performance preference separately
  for each power source. Plan readback controls availability, and applying
  edits goes through the existing review. Built-in presets preserve the plan
  value. Custom presets can leave the field blank to preserve it.

### CPU Allocation

- Runtime-backed feature pages use the shared docked status panel at the right edge instead of interrupting
  the settings flow. Every rail uses the same Running / Not running / Unknown state, process
  counts, and latest success/failure Action Log summary; unavailable snapshot metrics show an
  em dash rather than a fabricated zero. Action counts and latest outcomes are runtime telemetry,
  independent of Action Log visibility mode and its bounded visible history; clearing the Action
  Log resets both. The status area shows a current error when it differs from the latest failed
  action. The Failed actions section uses Action Log history.
- By Focus, By Running App, By CPU Load, By Activity, and By Time also use the right rail;
  every power-plan rail uses Status, Current power plan, Successful actions, and Failed actions.
  Power-plan actions are recorded only after the controller verifies the Windows plan transition.
- The Power Plan Control landing page shows the active Windows plan in a compact
  selector with an automation summary. Choosing a different plan requests a
  switch immediately. Keep showing the verified active plan and disable the
  selector until the switch finishes. Unsaved automation settings remain in the
  draft. If a read fails, show the read failure in the automation summary.
  When Adaptive Processor Power owns the active plan, show Winderust Adaptive
  in the disabled selector next to its settings shortcut. Enable manual selection
  after Windows confirms the plan release. Keep restoration status in the Status rail.
- Keep the Power Plan Control landing page in one scroll area. Align the active
  plan card, Switching settings group, and Automation and Advanced destination
  cards to the same width.
- Keep the shared A/C pause, Restore previous plan when no rule selects a plan,
  and Fallback polling interval under Switching settings. Windows events and
  scheduled changes can trigger checks sooner than the fallback interval.
  Restoration defaults off and applies to both power profiles. Help explains
  that every selected rule, including explicit Else and Active plans, takes
  precedence and external plan changes are kept. Overlapping rules share the
  plan captured before automatic switching began.
- By Focus, By Running App, and By Time keep their rule editors without separate
  restoration controls. By CPU Load retains Target plan and optional Else plan;
  By Activity retains Idle plan and Active plan plus its profile-specific Idle
  timeout. These plan selections remain visible independently of the shared
  restoration switch.
- The Power Plan Control side panel's Status tab shows the active Windows plan,
  the current automation decision, and lower-priority modes that also selected
  a plan in the same pass. The current-plan card stays compact. Ordinary rule Status rails
  identify a confirmed pause due to Adaptive Processor Power, A/C, or missing observations.
- By Focus and By Running App default to matching an executable path. Executable
  name rules match the same filename across paths. By Focus evaluates the app with
  input focus. Process List edits individual path rules. The rule editors accept
  full drive or network paths and show guidance for unsupported path formats.
- In By Focus, Preview matches checks a full executable path against the draft
  rules. It shows the first eligible rule and its selected plan, followed by
  any other eligible matches in rule order.
- In By Running App, Preview matches lists the processes each rule covers,
  same-name copies outside path rules, and processes with unavailable details.
  The winner shows its selected plan using the same rule number and name as
  its card. Lower-priority matching cards show their selected plans. Power Plan
  Control decides the active Windows plan.
- CPU Sets (Soft) and Processor Affinity (Hard) share that rail with compact Status and Presets
  tabs, alongside Info in the shared docked panel.
- Instance Balancing lives under CPU Control and shares one configuration across
  power sources. Use executable-path rules with activation, edit and delete actions;
  keep configuration editable while disabled. The rule table shows each rule's latest
  live result for an exact match to a saved rule, with details on hover. Its Info /
  Status panel shows the overall assignment or waiting reason without presenting
  preferences as reserved cores.
  When several cohorts are waiting, the rail prioritizes insufficient core capacity,
  then cohort stability, then missing instances. The Action Log records each rule's result.
- CPU Sets (Soft) and Processor Affinity (Hard) label their built-in section Core Presets. A preview
  shows the saved selection and its topology validation. Keep topology status, selection errors,
  and the review action above the processor grid. Keep duplicate-name feedback beside the name.
  Select a preset for a specific rule policy column to stage it.
- On systems with multiple processor groups, CPU Sets (Soft) offers a preset for
  all logical processors in each verified group. Group presets fit the existing
  policy columns and retain the full topology signature.
- Verified, fully representable L3 selections appear as Cache Domain presets
  with group and cache-size labels. Missing cache metadata creates no inferred
  preset. Unsupported saved selections remain visible; selection controls do
  not quietly shrink them to the processors currently available or replace
  their saved topology signature on load, opening an editor, or copying a preset.
  A separate explicit review action confirms the entire selection against the
  current topology. A topology change while editing blocks Save until explicit
  review or replacement; discovery failure disables confirmation.
- Rule tables use Active, App Name, Executable Path, Focus, Visible Window, Background, and Actions
  columns. Each policy column selects a Core or custom preset; do not restore expandable per-rule
  CPU grids.

### Efficiency Mode

- Place Efficiency Mode after Process Priority under Priority Control.
- The master group owns the Background tier default. Foreground Detection and Visible
  Window Detection use the same collapsible toggle-plus-value pattern as Priority Control.
- Custom rules match an exact executable path. The compact rule table has Active, App Name,
  Executable Path, Focus, Visible Window, Background, and Actions columns. Each policy column
  selects Default, Enabled, or Disabled. Default inherits the matching page-wide value.
- Save applies the tier defaults and custom rules. Discard restores the saved configuration.

### Priority Control and CPU Limiter

- Priority Control custom rules use the same three policy columns and independently select the
  priority for Focus, Visible Window, and Background.
- CPU Limiter offers CPU time and Load-triggered affinity as per-app methods.
  CPU time rules use Profile Limit, Custom, and No Time Cap in the Focus,
  Visible Window, and Background columns. Profile Limit uses Winderust's configured
  limit for the selected power profile and app state. No Time Cap turns off the cap for
  that state. The Enable CPU Limiter card starts collapsed and expands directly to
  the selected profile's three default limit controls. Custom tiers show a 1% to 100%
  Allowed CPU Time slider. At 100%, Winderust applies no CPU time cap. Inactive rules
  retain their selections.
  Explain both methods beside the selector. The Load-triggered affinity form
  shows the CPU trigger, sustained duration, and selected processor count.
  Choose processors opens the verified picker in the same dialog. Its processor
  buttons match the CPU Sets (Soft) preset editor and show group and core labels
  inside the picker alongside cache presets. A live preview describes
  the threshold, duration, and configured selection. An all-processor selection
  explains how to narrow the rule. Only the selected method's controls are actionable.
  CPU Limiter rule status follows the published observation for the selected live power source.
  With the other profile selected or the source unconfirmed, enabled rules show Unknown.
  Keep observed status compact in the editor. The applied rule shows its current duty-cycle
  target and whether it uses an app group, inherits another app group's target, or uses thread
  fallback. The editor indicates when the target or method varies by process.

### Advanced Power Plan Tuning

- Offer saved Windows plans in the target selector. Keep the observed Windows
  power mode and any inactive-plan or mode override message in the target card.
  If the temporary Winderust Adaptive plan is active, show it as the active plan
  while the selected saved plan remains editable.
- Show Performance Boost Mode and the long- and short-running thread policies
  separately for Plugged In and On Battery. Offer all six scheduling choices
  for recognized policy values. For unavailable controls, distinguish missing,
  unreadable, and unknown values, and show the raw index for unknown values.
  Keep the other controls editable. Percentage fields use the shared slider row.
  Show processor minimum and maximum together and put range feedback beneath
  them. Require a valid range before opening the apply review.
- Built-in presets keep current scheduling policies; custom presets can capture
  choices or keep current. Re-read after every Apply attempt, including partial
  failure. Adaptive Engine does not own these persistent writes.
- Apply and Save open the same review for pending tuning edits. Show the target
  plan and only changed values, grouped by Plugged In and On Battery. Identify
  a known active plan when it differs from the target. Cancel keeps the tuning
  draft. If a catalog refresh finds the target unavailable, keep the review open
  and show the reason in the target card. Disable confirmation until a refresh
  confirms the target is available.
- Restore previous tuning values opens the same review for the selected plan's
  previous values. It is available for the current Winderust session after a
  verified change and requires applying or discarding any pending edits first.

### Watchdog

- Place Watchdog after Process List in the primary navigation. Show Resource
  Thresholds, Memory Trim, Keep Running, Sleep Prevention, Instance Limits,
  and App Suspension in that order. Reuse the existing landing-page cards and
  link each one to its own editor. Keep enable controls, rules, and status per
  feature.
- Keep shared options for Resource Thresholds, Memory Trim, Instance Limits,
  and the App Suspension background delay in their feature's expandable toggle
  card. Let users configure these while the feature is off. Keep rule tables
  and independent App Suspension controls accessible outside that card.
- Place Notify beside Enable in the Resource Thresholds, Instance Limits, and
  Keep Running rule tables. Enable controls whether the rule runs. Notify
  controls Windows alert delivery. Resource Thresholds and Instance Limits
  also retain their feature-wide notification switches. For Keep Running,
  Notify opts the rule into a single alert when retries run out and Resume is needed.
- Keep Instance Balancing under CPU Control. Advanced Controls contains Timer
  Resolution and Win32 Priority Separation.

### Resource Thresholds

- Keep Resource Thresholds global across power sources, with the shared app
  picker, rule table, and Info / Status tabs. CPU and Memory columns show warning
  and critical thresholds, and the Memory column identifies the selected metric.
  Pencil dialogs show app identity and an optional sustained duration shared by
  the CPU / Memory tabs. A blank duration inherits the page default. Drafts apply
  only on OK. Cancel, Close, and Escape discard, including when a numeric field
  has focus. If a picker is open, Escape closes the picker first.
- Choose Executable path to use the app picker or Executable name to enter an
  exact filename such as `worker.exe`. Executable name rules monitor each
  resolved path separately and can send notifications. Choose Combined by path or Per
  instance in the rule dialog. Terminate and Restart require an At or above
  monitor in a Combined by path rule with an executable path.
- Match Preview reads a fresh process snapshot when opened or refreshed. It
  groups matching PIDs by executable path in Combined by path mode and lists
  each PID with its own path in Per instance mode. It identifies unavailable
  instances and explains that evaluation requires valid resource samples.
  Matching rows identify other enabled rules by their position in the current
  Resource Thresholds rule list.
  For an Executable path rule, it also lists same-named executables elsewhere.
- Each resource tab starts with a collapsed Enable CPU monitoring or Enable memory
  monitoring card. Its switch controls monitoring. The header reveals the
  comparison, memory metric where applicable, thresholds, and eligible actions
  independently of the switch. At or below clears the draft automatic actions
  for that resource. Where action controls are available, default to monitoring
  with optional notification and explain the restart rate limit and risk of
  losing unsaved work inline.
  The main page owns the default sustained duration and one notification switch.
- Explain that the chosen scope evaluates eligible instances at each matching path
  together or individually. Working Set measures resident memory and may double-count
  shared pages when combined. Private Commit (MB) measures committed private memory.
  Restart preserves the original user and privileges, supports non-service apps in
  the current session, and launches without previous arguments.
- Notifications lead with the action result, including cooldown skips, then trigger
  details. Keep the registered tray icon for enabled notifications independently of
  hide-on-close and respect Windows notification suppression. Runtime sequencing
  belongs in the [Resource Thresholds contract](runtime-contracts.md#resource-thresholds).

### Appearance, About, and Adaptive priority details

- Appearance groups Language, Theme, and Animation under Interface, keeps Accent
  independently expandable, and groups navigation and feature-card display settings.
  Use square color swatches and shared select widths. About groups identity and
  updates into separate surfaces.
- About shows the author name without inspiration or license prose. Its external-link
  buttons use GitHub and Discord marks, a book for Documentation, and scales for
  License, with icon and label centered within the button. The Updates card retains
  its controls and version rows, with update status immediately below Update channel.
  The result stays visible until another check or a change to the update channel.
- A startup update uses the same lower-right floating card as Save / Discard.
  The card shows the latest version and offers Ignore and Update. Ignore dismisses
  the notice for the current launch. Update opens the release page. A pending draft
  takes priority in the shared slot until it is saved or discarded.
- Accent palettes wrap to the available width. Time and color pickers use the
  shared application surface, border, accent colors, and labeled Cancel / OK
  actions. By Time keeps editable `HH:MM` fields alongside a 24-hour analog picker.
  Show separate Hour / Minute dials with numeric entry, advance to minutes after
  choosing an hour, and keep the rule dialog centered when the popup opens.
  Color selection offers a gradient square, hue / saturation / brightness sliders, and hex entry,
  with an opaque preview. Picker changes apply only on
  submit; Cancel, Escape, focus loss, and navigation discard them. Saved-color
  menus retain Escape/outside-click/focus-loss dismissal and reset with their item.
- Adaptive Engine Priority Control keeps each control's advanced options in its own
  expandable row under Focus, Visible Window, and Background. Process and Memory
  Priority include detection/preservation controls. Detection falls through to the
  next tier without changing CPU restraint's focus protection. Process Priority
  protects existing High/Realtime classes; same-or-higher/lower preservation defaults
  off, Memory preservation defaults on, and both detection layers default on.
  Custom presets capture these options; built-ins restore defaults.

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

- No marketing landing pages, hero sections, decorative illustrations, or oversized cards. Keep existing feature-section landing pages.
- No new design framework.
- No new icon system while Lucide assets are active.
- Reuse the existing shared rule-table helpers instead of introducing another table design.
- No UI-only refactor that moves many unrelated pages at once.

## Panels, rules, and dialogs

- Use `src/ui/app_picker.rs` for executable rules: compact input beside Add rule,
  two-line name/path entries with cached icons/placeholders, and Browse local
  executable inside the menu. Keep the overlay inside the viewport with keyboard
  selection and outside-click dismissal. Each feature owns eligibility and duplicate
  checks; unavailable App Suspension targets stay visible and disabled.
- Preserve the exact executable chosen through the running-app picker or Browse.
  Apply input formatting to typed text and finish eligibility checks before
  adding a rule to the settings draft.
- Dropdowns use the shared Select: native Iced field and overlay, persistent accent
  selection marker, gentle accent hover, and base option text. Scroll to the selected
  option on opening; retain keyboard and outside-click behavior.
- Removing a rule, exclusion, or custom preset immediately updates the settings draft.
  Save commits and Discard restores; do not add a second removal confirmation.
- Side panels follow tabs (when present), main title, subtitle, then content. Subtitles stay outside cards. Put primary panel actions in the bottom footer.
- Presets tabs share a scrollable list and pinned Add preset footer. Names wrap in neutral rows with hover feedback. Selecting a name opens a read-only preview; custom rows have separate edit and delete icons and the custom section has an empty state. Use the shared dialog frame, with each editor grouping its own fields. Page and group preset dropdowns stage settings for the current editor. Save or the feature's Apply review confirms the change.
- Feature info belongs in a full-height scrollable Info tab in the right sidebar. Order shared tabs left to right as Info / Status / Presets, omitting unavailable tabs. Adaptive Engine, Adaptive Processor Power, CPU Sets (Soft), and Processor Affinity (Hard) use all three. Advanced Power Plan Tuning uses Info / Presets. Reset to Info on navigation. Pages with only feature information show it directly without a lone tab. Remove the header info action and inline page disclosure; retain essential setting consequences and live warnings beside their controls.
- Keep rule creation above the rule table under Custom Rules. By Time and By CPU
  Load put Add rule on the main page. Other editors put it beside the target input.
- Use the shared rule table surface, row spacing, empty state, and action column.
  Keep Enable first, Status next when available, and Edit and Remove in fixed
  slots at the end. Between them, use the target and columns that describe the
  rule. Single-value edits use inline controls. Rule dialogs share a header and
  Cancel and OK footer, with target or name first and related controls grouped.
  OK stages a draft for the page's Save action.
- Executable rule tables show App name and Executable path. Long paths wrap
  within their column so the full target stays readable. Name-based rules
  show their exact filename and Any path with an Executable name label.
- Keep breadcrumbs anchored at the top left. Power-source selectors stay beneath the breadcrumb; page-level reference stays in the right sidebar.
- Quit confirmation is centered over a dimmed window. With unsaved changes show Cancel, Save and quit, and Quit without saving; otherwise show Cancel and Quit. Shutdown errors stay visible.
- Avoid duplicated help text between inline descriptions, info popovers, and sidebar Feature info.

- Adaptive Engine CPU Behaviour presents Shared Detection and Timing once as a collapsible group. CPU Pressure Restraint uses a toggle card with a visible description. Limit Background Processors and Dynamic Resource Zones keep their controls under independent toggle headers with visible descriptions. Keep disclosure state separate for the live and preset editors. Zoning keeps its 1-99 foreground share and visible throughput warning. The limiter percentage always refers to background capacity, and its allocation method remains editable when zones are enabled. Show verified zoning status and counts.

## Current reference layout

- The user-approved 2026-10-30 dashboard direction guides the palette, spacing, and metric hierarchy. The App Shell section owns current navigation and window behavior.
- Keep the 88px primary rail, floating child menus, tinted selected entries, all existing categories, and independent global search.
- Use 16px card padding, 12px card gaps, regular Segoe UI body text at 14px, 13px sidebar labels, semibold headings, and prominent 28px dashboard values. The primary rail uses 11px centered wrapping labels. Keep advanced shaping, system font fallback, and DPI-aware software text rendering; do not claim ClearType support.
- Use one 18px breadcrumb heading on every page: Home > section > current page, omitting unused levels. Ancestors are clickable; the current page is semibold text. Home shows only Home. Wrap the path in narrow windows; do not repeat the current page as a separate title.
- Home uses a two-by-two chart grid above Enabled Features and Recent Logs. Shared content is capped at 1440px.
- Dense virtual process and log tables retain borderless tiled backgrounds and existing row geometry.
- Keep System/Light/Dark/AMOLED and custom/system accent behavior. Do not infer new telemetry or optimization guarantees from the reference image.

## Home dashboard

- Follow the latest dashboard reference: a 2x2 CPU/RAM/Disk I/O/Network I/O chart grid, then equal-width Enabled Features and Recent Logs panels. Adapt card heights to available content height. Use four metric columns at widths of at least 1280px and heights below 680px, two columns from 720px wide, otherwise one column. Omit redundant dashboard subtitles at every size. Keep chart cards at least 200px high and summary panels at least 160px high; retain page scrolling only when these readable minimums cannot fit; the feature and log lists scroll independently.
- Charts show 60 seconds of real history with axes and legends. Hover shows a floating tooltip with the actual sample timestamp and named series values, plus a dashed crosshair and point markers. Clamp the tooltip to the viewport, including compact charts. CPU headers show Usage on the left and Frequency on the right. Render only recorded chart samples: missing history stays blank, actual zero samples remain visible. Sampling pauses and unavailable readings leave gaps instead of moving older values forward or connecting across the missing interval. RAM headers show Usage with a Used label and used/total capacity on the left and Available on the right. Disk and Network I/O headers show their two named, curve-colored metrics on the left and Total on the right. Network uses decimal byte rates, matching Disk I/O. CPU frequency is explicitly relative on the shared percentage scale, with the current GHz value shown separately.
- Tab reaches charts in reading order. Left or Right starts at the newest recorded reading, then moves to older or newer readings and skips gaps. The focused chart displays the selected timestamp and named values in its header without changing card height. The selection stays on the same observation as new samples arrive. Escape restores the current readings, and the header returns to current readings when the selected observation ages out.
- RAM Usage charts physical memory. Its live header also shows system committed memory against the current Windows commit limit, alongside the physical used and available values.
- Position chart samples by elapsed time, retaining fractional seconds. Allow up to 1.5 seconds between consecutive valid observations for one-second sampling with dispatch and worker jitter; larger intervals and explicit unavailable readings break the curves. Hover selects the nearest recorded observation before checking that channel's availability.
- Windows contrast themes use a solid first series and a dashed second series, with round and square markers in the plots, legends, and tooltips. Keep the grid subdued and leave the plot background clear so the curves stay readable when Windows supplies the same color for both series.
- While a sample is overdue, headers show Collecting and history continues to age while Home is visible.
- Enabled Features and Recent Logs preserve card height and fixed headers in both empty and populated states. Enabled Features has no View All footer; Recent Logs retains its Details footer. Center empty messages in the remaining content area; populated lists use inset scrolling with space for the scrollbar and links to their full pages. Recent Logs shows compact INFO (Applied/Restored), WARN (Skipped), and ERROR (Failed) levels derived from actual Action Log entries, preserving the outcome in the description. The Log table keeps Level and Result separate; its level filters, capture thresholds, and CSV level field share the same mapping and short labels in both languages. Never fabricate log events. Keep unavailable telemetry and empty logs honest.
- In the focused Log table, Left and Right reveal offscreen columns. Page Up,
  Page Down, Home, and End move through the rows.
- In Log, search by process name or exact PID. Search combines with level
  and feature filters before entries are grouped and returns the list to the
  first row. Group each filter with its own label. Capture Mode affects new
  entries after Save. Level Filter changes the table, and Export Log uses
  the full recorded snapshot.
- Keep category shortcuts in navigation and global search; omit the old dashboard shortcut grid.
- Dashboard sampling runs only while Home is selected and the window is not hidden to tray. Do not add manual Pause/Resume metrics controls; background automation retains its independent monitoring.
- After a sampling gap exceeds 1.5 seconds, CPU Usage, Disk I/O, and Network I/O
  start a fresh rate baseline. The first reading shows Collecting until a
  fresh interval supplies a rate.
- Enabled Features follows the sampled Windows power source. The feature editors retain their selected profile. Show the feature count only when the power source is known.
- Show the sampled power source, battery level, charging, and active Energy saver as text in the Enabled Features header. Omit battery details when Windows reports no battery, and label an unknown battery level as unavailable.


## Verification

Use the [render harness](development.md#rendering-and-performance) for both locales,
light/dark themes, AMOLED, minimum-size and desktop windows, empty/populated rules,
collapsed/expanded groups, live/editable/read-only presets, Info tabs, flyouts, and
search. Check header controls independently from expansion; each must retain its own
hit target. Preserve live/preset navigation and collapse-state isolation.

Follow the [PStack verification workflow](development.md#pstack-verification) for
changed workflows. Review the captures and check the relevant Windows interactions
at the supported window sizes and display scaling. Check keyboard access, readable
focus and contrast, and accessible names where a change affects shared controls.
Do not recreate unmapped historical features or infer telemetry/performance promises
from a reference image. Completed port audits and their old findings remain in Git
history rather than defining current behavior.
