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

## App Shell

- Use a compact themed title bar with the app icon/name and minimize, maximize/restore, and close controls. Follow the active Light, Dark, AMOLED, and accent palette; disable native title-bar decorations. Keep the left navigation and right work area beneath the title bar.
- Iced owns native dragging, resizing, minimizing, and maximizing. The caption supports drag and double-click maximize/restore; restored windows expose edge/corner resize targets. Both custom and native close requests use the existing tray, unsaved-change, and shutdown handling. Keep window controls available above dialogs.
- Use a permanent 88px icon-and-label rail. Main entries are 64px high; center each icon and its actual one- or two-line label together vertically with equal top and bottom insets.
  Group chevrons overlay the icon row's right edge. Search, primary items, and bottom shortcuts share the same 76px item width and centerline. Keep the thin primary scrollbar within the existing 6px outer padding; do not apply the wider page gutter to the navigation rail. Log, Settings, and About stay below the scrollable primary list.
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
- Center the shared header and content canvas within the work area beside the primary rail and any open side panel, with shared padding and a 1440px width cap. Every page reserves the same 48px breadcrumb area and 32px toolbar, including pages without power-source selectors, so page content starts at a stable position.
- The right side panel is a 320px dock beside the editor, reserving its own width without covering content. Both areas remain interactive. Start closed and retain its open state throughout navigation, search, focus changes, and settings edits. Close only through panel controls. Pages without panel content use the full canvas and preserve the session's open preference for the next supported page. Keep existing tabs, scrolling, and actions. The unsaved-settings notice stays within the editing area, and dialogs cover both areas. There is no collapsed strip or persisted mode setting.
- Navigation labels and page sections live in `src/ui.rs`; page rendering
  dispatch stays in `WinderustApp::page_view` in `src/ui/app.rs`.

## Layout Rules

- Use Iced rows and columns with explicit `Fill`, `Shrink`, and constrained widths for flexible children.
- Rows and table headers should have stable heights across state changes.
- Rules tables fill the same canvas width as settings cards. Use fixed or computed minimum widths for dense tables and policy columns, with horizontal scrolling when the dock leaves too little room; slider labels sit above their controls. Do not let dynamic labels resize the process list.
- Constrain long process names, labels, and status values so they do not resize adjacent columns.
- Group related settings beneath each section heading in one card, with shared internal dividers between complete rows. Every grouped card needs a concise, localized category heading outside its outline. Apply this to pages, preset editors, rule dialogs, and process details; independent collapsible groups use their own headers. Keep each label, description, and control together; avoid a separate card for every setting. Preserve independent collapsible feature groups.
- Avoid nested cards. Cards are for repeated rows, setting groups, status blocks, popovers, and tool surfaces.
- Centered dialogs, Search, and picker popups are modal: shield background hover,
  pointer/touch actions, scrolling, typing, focus operations, and existing background
  overlays. Outside presses may dismiss a popup but must never activate the content
  behind it. Keep window chrome usable and preserve background scroll/input state.
- Unhandled Escape cancels By Time, By CPU Load, and Watchdog rule dialogs without
  saving their drafts. Open pickers and focused controls handle Escape first.
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
- Action and toggle cards use their full surface, including padding and blank space, as the hit target. Expandable cards toggle only from the full header, including its padding; expanded content never toggles the group. Keep one 16px inset for headers and bodies. Nested controls retain their own actions, cursor, and input handling, and disabled controls stay inactive.
- Collapse expandable cards by default. Keep configuration editable when its main switch is off; read-only presets and invalid operations remain disabled.
- Sliders always reflect their input values, including when disabled. Put units in labels and use plus/minus icons.
- Use sliders, steppers, or numeric inputs for numeric settings.
- Use dropdowns for bounded option sets.
- Use icon buttons for compact repeated actions; text buttons are fine for clear primary commands such as save/import/export.

## Visual Language

- Reuse `design.rs` tokens and `widgets.rs` styles. Buttons share 6px corners, 14px labels, and stable 1px outlines. Standard text actions and utility icons are 32px high; text actions use 12px horizontal padding through the shared action helper. Primary actions use a solid accent fill; secondary actions use a neutral outline with accent text; tertiary actions use a neutral outline and text. Quiet utilities show hover feedback; destructive actions use danger colors; persistent choices use accent outlines and tint. Preserve explicit dense-row and navigation geometry, native input, disabled states, and theme/accent support. Keep action feedback distinct from persistent selection.
- Navigation selection uses a rounded vertical pill. All page, panel, and dialog tabs use the same compact segmented design: 32px height, widths that fit their centered 14px semibold labels, one neutral outline, no outer inset or gap, and an accent-tinted, outlined selected segment. Tab labels retain normal theme text colors, including the side panel's Info / Status / Presets tabs; selection changes the tint and outline rather than the text color. Weekday selectors share this design and retain multiple selection. Plugged In / On Battery retains its live-source dot independently of the selected editing profile. Respect the current theme and accent without shadows or underlines.
- Choose toggle knobs and checkmarks for contrast against the accent color.

- The navigation rail and docked right panel use edge-aligned chrome with square outer corners, workspace-facing thin dividers, and no gaps between shell surfaces. Floating navigation uses the shared outlined surface. Cards, rules tables, dense table frames, settings groups, and settings dialogs share the surface outline (1px neutral at 75% opacity) and 10px radius. Keep dense table frames outside horizontal scrolling and preserve their borderless tiled interior for partial repaint efficiency.
- Base surfaces use cool pale neutrals in light mode and slate tones in dark mode; panels have subtle 1px outlines. Inputs, dropdowns, and menus use the neutral control surface, distinct from cards: slightly lighter in Dark and AMOLED, darker in Light. Keep control hover fills opaque, and internal settings dividers subtler than outer card borders. AMOLED workspace and dock remain pure black. Accent color marks primary action, active navigation, selection, and important status.
- Respect `AppThemeMode`, `AccentColorSource`, and system accent behavior through the Iced theme and `settings_pages.rs` color helpers. Derive workspace, navigation, card/field, button, border, divider, and muted-text colors from the selected accent using restrained grayscale tints in the shared palette. Chart series and informational Process List colors also follow accent roles. Keep body/help text readable, semantic success/warning/danger distinct, and existing contrast rules for accent fills and marks. Accent changes must refresh the complete theme immediately, including cached chart/table surfaces.
- Language and Appearance offers System, Light, Dark, and AMOLED. AMOLED uses a pure-black workspace and docked navigation with near-black accent-tinted cards and controls; retain readable borders, semantic status colors, and dark-mode contrast rules. System continues to follow Windows light/dark selection.
- Do not add purple/blue gradients, decorative blobs, glow backgrounds, or one-note palettes.
- Status colors should stay semantic: success for active/applied, warning for caution, danger for destructive or failed actions.
- Text hierarchy is compact: small labels, body rows, muted helper text. Avoid hero-scale text inside panels.

## Icons

- Reuse bundled Lucide SVGs through `src/ui/assets.rs`; page icons are mapped in
  `src/ui/navigation.rs`.
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
- Keep process icon, grouped process row, PID/count, and policy columns visible and stable.
- Keep the User column at the right edge. Show the exact process token account when available. When Windows denies token access, show `Unavailable · S#` with an explanatory tooltip; identify kernel pseudo-processes as `Windows kernel`.
- Keep Windows-critical processes visible as read-only `Protected system process` rows in Process List, omit them from every Add process picker, and fail closed when criticality cannot be verified.
- For other inaccessible rows, show `Administrator required` before elevation and `Access denied` when Winderust is already elevated.
- When elevated, use normal Windows process permissions for discovery and automation while retaining every critical, built-in, identity, and protected-process barrier.
- With Advanced Controls enabled, keep Suspend/Resume present in the context menu and use the existing disabled style when the selected process cannot be safely suspended. Grouped rows apply the action to every captured process in the group. Keep unavailable App Suspension picker candidates visible, disabled, and explicitly labeled `Unavailable`. App Suspension safety must not disable unrelated process controls.
- Stacked Process List parent rows expose only `Stop process tree`, covering every process in the stack and the union of their descendants; their expanded sub-items expose both stop actions. Non-stacked rows always expose `Stop process` and expose `Stop process tree` only while the selected process has a child in the current process snapshot.
- Delete opens confirmation for the selected process or stack members only, without descendants. Tree termination remains an explicit context-menu action; Cancel submits no termination.
- Keep Stop process and Stop process tree consistent from their initiating action through the confirmation title and button. Tree confirmations name child processes, and both warnings retain the unsaved-work consequence.
- Report `Efficiency mode` only when the process has both EcoQoS and `IDLE_PRIORITY_CLASS`, matching Task Manager rather than treating EcoQoS alone as full Efficiency mode.
- Filters, column visibility, and memory value/percentage controls belong in the Settings side panel.
- Language and Appearance owns the enabled-feature counts toggle. When enabled, the secondary navigation header shows a numeric pill for Winderust Features, Power Plan Control, Priority Control, and CPU Control based on the section's enabled modules; the primary rail stays unbadged.
- Language and Appearance separately owns the feature-card status toggle. When enabled, shared landing and search-result cards show Enabled or Disabled pills only for pages backed by a top-level feature switch; pages without a single enabled state remain unbadged.
- Editable policy cells should use inline dropdown controls. Avoid modal flows for simple policy edits.
- Columns are resizable and initially fit their content. Double-click a divider to auto-fit; the last visible column fills remaining width. Preserve the layout tests.

## Settings Pages

- Settings and its Winderust Behaviour, Language and Appearance, and Experimental
  Features pages have no side panel or Side panel button. Keep help inline.
- Within Adaptive Engine, CPU Behaviour has no separate master switch;
  CPU Pressure Restraint, Limit Background Processors, and Dynamic Resource Zones own independent switches and retain their
  configuration while Adaptive Engine is off.
- Adaptive Engine uses the right-rail Info / Status / Presets tabs. Built-in presets are read-only; custom presets can be added, renamed, refreshed from the current tuning, and deleted. Presets exclude master enable switches, custom rules, exclusions, and the separate Background Efficiency feature. The preset editor spans the available modal width, and its control state remains independent from the live page rendered behind it.
- Adaptive Engine and preset details share CPU Behaviour, Processor Power, and Priority Control tabs; the live page also exposes Custom Rules. Preset modal state is independent of the live page.
- Priority Control uses Focus / Visible Window / Background dropdown columns and expandable detail cards. Turning a row off does not lock its configuration. Runtime precedence and protection rules live in [Runtime contracts](runtime-contracts.md).
- Processor Power groups Base Processor Policy, Background Pressure Profile, and Focus and Launch Profile into one card per section with shared internal dividers. CPU Pressure Restraint and Limit Background Processors have independent switches; configuration remains editable while off.
- Processor Power exposes separate editable A/C and Battery boost policy/mode values for Background Pressure and Focus and Launch. Background-dominant pressure selects Background Pressure; app launches and genuinely heavy Focus App demand select Focus and Launch. These values are part of Adaptive Engine presets rather than hidden runtime constants.
- Prefer one setting per row when possible.
- Use collapsible setting groups for advanced or multi-row settings.
- Multi-setting cards use subtle 1px neutral, theme-aware dividers between complete settings, with even inset spacing. Keep labels, descriptions, and associated controls together. Expanded groups separate their header and body with the same divider; single-setting cards have no extra leading/trailing rules. Use shared widgets helpers and preserve dense table styling.
- Show concise setting descriptions directly beneath their labels in readable 13px secondary text, with controls aligned on the right. Let rows and group headers grow when labels or descriptions wrap; never clip help with a fixed height. Use the shared setting-title layout for live pages and preset dialogs. Keep essential consequences inline and longer page-level reference in the right sidebar's Feature info section. Preserve icon-action and truncated-data tooltips, localization, units, and dense process/log table geometry.
- Settings that affect Windows behavior should show conservative defaults and explicit enable controls.

### CPU Allocation

- Runtime-backed feature pages use the shared docked status panel at the right edge instead of interrupting
  the settings flow. Every rail uses the same Running / Not running / Unknown state, process
  counts, and latest success/failure Action Log summary; unavailable snapshot metrics show an
  em dash rather than a fabricated zero. Action counts and latest outcomes are runtime telemetry,
  independent of Action Log visibility mode and its bounded visible history; clearing the Action
  Log resets both.
- By Focus, By Running App, By CPU Load, By Activity, and By Time also use the right rail;
  every power-plan rail uses Status, Current power plan, Successful actions, and Failed actions.
  Power-plan actions are recorded only after the controller verifies the Windows plan transition.
- Power Plan Control exposes one Restore previous plan when no rule selects a
  plan switch beside the global A/C pause. It defaults off and applies to both
  power profiles. Help explains that every selected rule, including explicit
  Else and Active plans, takes precedence and external plan changes are kept.
  Overlapping rules share the plan captured before automatic switching began.
- By Focus, By Running App, and By Time keep their rule editors without separate
  restoration controls. By CPU Load retains Target plan and optional Else plan;
  By Activity retains Idle plan and Active plan. These local choices remain
  visible independently of the shared restoration switch.
- CPU Sets (Soft) and Processor Affinity (Hard) share that rail with compact Status and Presets
  tabs, alongside Info in the shared docked panel.
- Core Presets use compact read-only rows; custom presets use the same row geometry with edit and
  delete actions. Resting rows have no fill or divider, and hover supplies the background. Add,
  edit, and view use the existing full-window modal style.
- Verified, fully representable L3 selections appear as Cache Domain presets
  with group and cache-size labels. Missing cache metadata creates no inferred
  preset. Unsupported saved selections remain visible; selection controls do
  not quietly shrink them to the processors currently available. Preset edits
  require review if discovery fails or topology changes while the dialog is open.
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

### Advanced Power Plan Tuning

- Expose separate Plugged In / On Battery long- and short-running thread scheduling
  policies for hybrid processors. Read the selected plan and offer all six Windows
  choices without an implicit Automatic override. Missing settings are unavailable;
  unknown indices and access errors remain visible failures.
- Built-in presets keep current scheduling policies; custom presets can capture
  choices or keep current. Re-read after every Apply attempt, including partial
  failure. Adaptive Engine does not own these persistent writes.

### Watchdog

- Keep Watchdog under Winderust Features, global across power sources, with the shared
  app picker, rule table, and Info / Status tabs. CPU and Memory columns show warning
  and critical thresholds. Pencil dialogs show app identity below the header divider,
  then CPU / Memory tabs. Drafts apply only on OK; Cancel, Close, and Escape discard.
- Each tab starts with its Enable CPU Watchdog or Enable Memory Watchdog toggle and
  reveals thresholds and optional termination/restart only when enabled. Default to
  notification only and show the data-loss consequence inline. The main page owns
  shared sustained duration and one notification switch.
- Explain that each rule aggregates eligible instances of its executable, excluding
  different child executables; summed working sets may double-count shared memory.
  Restart preserves the original user and privileges, supports non-service apps in
  the current session, and launches without previous arguments.
- Notifications lead with the actual action result, then trigger details. Keep the
  registered tray icon for enabled notifications independently of hide-on-close;
  respect Windows notification suppression. Runtime sequencing belongs in the
  [Watchdog contract](runtime-contracts.md#watchdog).

### Appearance, About, and Adaptive priority details

- Appearance groups Language, Theme, and Animation under Interface, keeps Accent
  independently expandable, and groups navigation and feature-card display settings.
  Use square color swatches and shared select widths. About groups identity and
  updates into separate surfaces; unsaved settings stay in the lower-right notice.
- About shows the author name without inspiration or license prose. Its external-link
  buttons use GitHub and Discord marks, a book for Documentation, and scales for
  License, with icon and label centered within the button. The Updates card retains
  its controls and version rows,
  with update status immediately below Update channel.
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

- Use `src/ui/app_picker.rs` for custom-rule selection: compact input beside Add,
  two-line name/path entries with cached icons/placeholders, and Browse local
  executable inside the menu. Keep the overlay inside the viewport with keyboard
  selection and outside-click dismissal. Each feature owns eligibility and duplicate
  checks; unavailable App Suspension targets stay visible and disabled.
- Dropdowns use the shared Select: native Iced field and overlay, persistent accent
  selection marker, gentle accent hover, and base option text. Scroll to the selected
  option on opening; retain keyboard and outside-click behavior.
- Removing a rule, exclusion, or custom preset immediately updates the settings draft.
  Save commits and Discard restores; do not add a second removal confirmation.
- Side panels follow tabs (when present), main title, subtitle, then content. Subtitles stay outside cards. Put primary panel actions in the bottom footer.
- Feature info belongs in a full-height scrollable Info tab in the right sidebar. Order shared tabs left to right as Info / Status / Presets, omitting unavailable tabs. Adaptive Engine, CPU Sets (Soft), and Processor Affinity (Hard) use all three; Advanced Power Plan Tuning uses Info / Presets. Reset to Info on navigation. Pages with only feature information show it directly without a lone tab. Remove the header info action and inline page disclosure; retain essential setting consequences and live warnings beside their controls.
- Use shared rule tables and icon edit/delete actions. Reuse existing modal layouts: name first, related settings together, actions at the bottom.
- Keep breadcrumbs anchored at the top left. Power-source selectors stay beneath the breadcrumb; page-level reference stays in the right sidebar.
- Quit confirmation is centered over a dimmed window. With unsaved changes show Cancel, Save and quit, and Quit without saving; otherwise show Cancel and Quit. Shutdown errors stay visible.
- Avoid duplicated help text between inline descriptions, info popovers, and sidebar Feature info.

- Adaptive Engine CPU Behaviour presents shared detection/timing once, followed by three sibling policy groups. Zoning has its own live/preset expansion state and 1-99 foreground share. Limiter percentage always means background capacity; its allocation method stays visible when zones are enabled. Show zoning's throughput warning and verified status/counts, not configured enablement as proof of application.

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
- Position chart samples by elapsed time, retaining fractional seconds. Allow up to 1.5 seconds between consecutive valid observations for one-second sampling with dispatch and worker jitter; larger intervals and explicit unavailable readings break the curves. Hover selects the nearest recorded observation before checking that channel's availability.
- Enabled Features and Recent Logs preserve card height and fixed headers in both empty and populated states. Enabled Features has no View All footer; Recent Logs retains its Details footer. Center empty messages in the remaining content area; populated lists use inset scrolling with space for the scrollbar and links to their full pages. Recent Logs shows compact INFO (Applied/Restored), WARN (Skipped), and ERROR (Failed) levels derived from actual Action Log entries, preserving the outcome in the description. The Log table keeps Level and Result separate; its level filters, capture thresholds, and CSV level field share the same mapping and short labels in both languages. Never fabricate log events. Keep unavailable telemetry and empty logs honest.
- Keep category shortcuts in navigation and global search; omit the old dashboard shortcut grid.
- Dashboard sampling runs only while Home is selected and the window is not hidden to tray. Do not add manual Pause/Resume metrics controls; background automation retains its independent monitoring.


## Verification

Use the [render harness](development.md#rendering-and-performance) for both locales,
light/dark themes, AMOLED, minimum-size and desktop windows, empty/populated rules,
collapsed/expanded groups, live/editable/read-only presets, Info tabs, flyouts, and
search. Check header controls independently from expansion; each must retain its own
hit target. Preserve live/preset navigation and collapse-state isolation.

Review the captures and exercise native interactions. Capture counts and passing Rust
tests establish coverage, not visual or behavioral equivalence with historical UI.
Do not recreate unmapped historical features or infer telemetry/performance promises
from a reference image. Completed port audits and their old findings remain in Git
history rather than defining current behavior.
