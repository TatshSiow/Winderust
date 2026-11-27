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

- Keep the workflow predictable. Dashboard shows current conditions and running
  processes, feature pages edit rules, the Side panel shows live status and
  secondary controls, and Action Log records outcomes.
- Make settings easy to discover through navigation and search. Put common actions
  first and reveal advanced options where users need them.
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

- Use a compact themed title bar with the app icon/name, a centered Search input, a Side panel toggle immediately left of Minimize, and the Maximize/Restore and Close controls. Follow the active Light, Dark, AMOLED, and accent palette. The workspace row and work area follow the custom title bar.
- Iced owns native dragging, resizing, minimizing, and maximizing. The caption supports drag and double-click maximize/restore; restored windows expose edge/corner resize targets. Both custom and native close requests use the existing tray, unsaved-change, and shutdown handling. Keep window controls available above dialogs.
- Put text-only workspace tabs in one row under the title bar. Selecting a workspace opens its landing page. Highlight the active workspace on its landing page and on each of its child pages.
- Workspaces with child pages show a left page list beside the editor, starting with the landing page. Highlight the current page in the list. Standalone workspaces use the available width beside any open right Side panel.
- When Show enabled feature counts is on in Language and Appearance, display nonzero counts on the Watchdog, Power Plan Control, Priority Control, and CPU Control buttons in the workspace row. Mark enabled features with a dot beside their names in the left page list.
- Tab moves through the workspace row and page list. Enter or Space opens the focused destination. Use a visible focus outline distinct from the selection tint.
- Search uses one editable field in the title bar. Clicking the field, typing into it, or invoking a Search shortcut opens a panel aligned with the field above a dimmed workspace. Use the shared Select for the category filter and the shared button for Esc. Keep the panel shadow-free for partial repaints. An empty search in All shows just the filter row. Typing or choosing a category shows results with feature names and parent sections. Vertically center icons, labels, and chevrons in each result row and tint the Enter target.
- Enter opens the selected result. Up/Down move the selection and scroll it into view. New queries and filters select the first result. Escape, the Esc button, or clicking the dimmed workspace dismisses Search. Search defaults to Ctrl+K or / within the app window, respecting keys captured by text inputs and menus. Settings > Winderust Behaviour provides primary and alternate shortcut recording, clearing, and default restoration through normal Save.
- Keyboard Shortcuts in Winderust Behaviour includes the optional system-wide
  Show Winderust shortcut. Use the same keyboard recording and Clear controls as
  Search. Help explains the valid combinations and that enabling the shortcut
  keeps Winderust in the notification area. Show conflicts and registration
  failures in text. Save commits the selection and Esc cancels recording.
- Center the shared header and content canvas between the optional page list and
  Side panel, with shared padding and a 1440px width cap. Use one 48px header
  row for the page title on the left and any feature tabs and power-source
  selector on the right. Use a Select for feature tabs below 720px of editor
  width, and for the power-source selector when both groups share that space.
  Start editor content directly below the header on every page.
- The right Side panel docks beside the editor, reserving its own width. Use a 320px dock on wide windows and on standalone pages. When the window is narrower than 1280 logical pixels and the page list is visible, reduce the dock to 280px to leave more room for the editor. Both areas remain interactive. Start closed and retain its open state throughout navigation, search, focus changes, and settings edits. Close through the title-bar toggle or the panel's Close button. Pages with no panel content use the available editor width and preserve the session's open preference for the next supported page. Dialogs cover the work area.
- The title-bar toggle shows the side panel's current state and is inactive while a modal is open or the page has no panel. The in-panel Close button returns keyboard focus to the title-bar toggle.
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
- Group related settings in one card with shared internal dividers between complete rows. The main toggle card uses its own title. Use Settings for basic behavior and task-specific headings for distinct controls. Keep a heading visually attached to its own group. A tab may serve as the heading for its rule editor. Independent collapsible groups use their own headers. Keep each label, description, and control together.
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
  Settings rows and collapsed group headers use a 40px minimum content height.
  With 16px insets, standard one-row cards and destination cards are 72px tall.
  Rows grow when localized text wraps or controls need to stack.
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
- Use an accent tint for the active workspace and page list entries. Segmented page, panel, and dialog tabs and the power-source selector use a 32px height, widths that fit their centered 14px semibold labels, one neutral outline, and an accent-tinted, outlined selected segment. Tab labels retain normal theme text colors, including the Side panel's Status / Presets tabs. Weekday selectors share this design and retain multiple selection. Plugged In / On Battery retains its live-source dot independently of the selected editing profile, including in its Select options when compact. Respect the current theme and accent without shadows or underlines.
- Choose toggle knobs and checkmarks for contrast against the accent color.

- The workspace row, page list, and docked Side panel use edge-aligned chrome with square outer corners and thin dividers. Cards, rules tables, dense table frames, settings groups, and settings dialogs share the surface outline (1px neutral at 75% opacity) and 10px radius. Keep dense table frames outside horizontal scrolling and preserve their borderless tiled interior for partial repaint efficiency.
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

- Dashboard's Process List uses a dense table. Refresh automatically about once
  per second while Dashboard is visible. Its command row contains search and
  process count.
- Show Process List action results in a bottom-center toast over the editor,
  above any floating Save or update card. Dismiss successes after five seconds
  and leave errors visible until dismissed or replaced. Keep feedback from
  actions in process details inside the dialog.
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
- Offer Trim memory in Process List's context menu. A stacked app row targets
  the eligible processes in that stack. An expanded member targets one process.
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
- Editable policy cells should use inline dropdown controls. Avoid modal flows for simple policy edits.
- Columns are resizable and initially fit their content. Double-click a divider to auto-fit; the last visible column fills remaining width. Preserve the layout tests.

## Settings Pages

- Settings and its Winderust Behaviour, Language and Appearance, and Experimental
  Features pages keep help inline in the editor beside the Settings page list.
- The Settings Files card explains each file action before its picker opens. Export includes
  unsaved settings edits. A valid import replaces the active settings immediately and requires
  a clean settings draft.
- Open feature editors on Setup when they have distinct tasks. Use Custom Rules for per-app
  policies in Priority Control, Efficiency Mode, CPU Limiter, Resource Thresholds, Sleep
  Prevention, Instance Limits, and App Suspension. Memory Trim uses Exclusions. Keep manual
  trim and temporary sleep prevention controls in Setup.
  Keep a feature's last selected tab for the session. Opening a feature page or switching
  tabs starts at the top of the selected content and preserves the settings draft, power
  profile, and side panel.
- Power Plan Control puts the current plan and shared switching settings in Setup, ordered
  switching rules in Automation, and the tuning editors in Advanced. Advanced Power Plan
  Tuning uses Plugged In and On Battery tabs for the selected plan. Its target, Apply actions,
  and validation of both drafts span the source tabs.
- Adaptive Engine opens on Setup. Group the enable switch, CPU preset, and the
  selected preset's practical effect in one card. First enable stages Balanced
  when the selected source has factory tuning. Show a message if the engine is
  enabled with no CPU policy configured.
- Setup puts collapsible CPU settings below the preset card. The Exclusions tab
  contains Exclude service hosts and per-app exclusions. Interrupt Timing has
  its own tab. In short workspaces the enable switch and preset picker scroll
  with the tuning cards. In taller workspaces they stay visible above the cards.
- The Status and Presets rail tabs share the preset library across power sources.
  Built-in previews show the same brief explanation as Setup. Custom preset
  editors retain a separate draft and show the tuning directly. Keep Exclude
  service hosts visible in the preset editor because presets capture it. Preset
  selection and enablement stage changes for the selected source through the
  shared Save and Discard flow.
- Priority Control Setup shows the Background, Focus, and Visible Window settings
  in open cards. Use the same switch rows for detection, priority preservation,
  and child protection. Turning a row off does not lock its configuration. Runtime
  precedence and protection rules live in [Runtime contracts](runtime-contracts.md).
- Place a Protect child processes switch inside each Priority Control feature's
  Foreground Detection and Visible Window Detection cards. Both switches
  default on and use the selected power profile and the standard Save flow.
- Adaptive Processor Power lives under Power Plan Control. Its Plugged In and On Battery editors each have an enable switch, off by default, with a Preset dropdown below it. Both editors share the preset library. The dropdown stages tuning for the selected source. The Presets tab opens a read-only preview of the base policy and demand values for that source. Use preset stages the preview, and closing it preserves existing edits. Add preset names a copy of the selected source's tuning. List edits and tuning use the shared Save and Discard flow. Configuration remains editable while the feature is off.
- The Base Processor Policy tab groups Idle settings into Core parking, Processor performance, and Processor boost. These values are the baseline inherited by higher-demand profiles. The Demand Profiles tab explains automatic profile selection and shows one boost policy and one boost mode each for Background Pressure and Focus and Launch.
- The Adaptive Processor Power Status rail reports live ownership, errors, and restoration. The source profile selected for editing can differ from the live power source.
- The minimum unparked controls for CPU restraint follow the normal core parking control
  in Base Processor Policy. Use separate rows for Windows processor efficiency classes 0
  and 1. Preset previews name the normal policy at 0% and show the configured percentage
  otherwise.
- Prefer one setting per row when possible.
- Give basic behavior its own section when it forms a distinct group. Keep a
  feature switch with its immediate control when they belong together, as with
  Adaptive Engine's preset. Use collapsible groups for longer and
  advanced settings where they help keep frequent controls in view.
- Multi-setting cards use subtle 1px neutral, theme-aware dividers between complete settings, with even inset spacing. Keep labels, descriptions, and associated controls together. Expanded groups separate their header and body with the same divider; single-setting cards have no extra leading/trailing rules. Use shared widgets helpers and preserve dense table styling.
- Put concise feature explanations and essential consequences in the relevant card. Show setting descriptions directly beneath their labels in readable 13px secondary text, with controls aligned on the right. Let rows and group headers grow when labels or descriptions wrap; never clip help with a fixed height. Use the shared setting-title layout for live pages and preset dialogs. Preserve icon-action and truncated-data tooltips, localization, units, and dense process/log table geometry.
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
  restoration controls. By CPU Load retains Target plan and optional Else plan.
  By Activity places its enable switch above both selectors under Power plans. The
  Input detection card holds the input switches and the profile-specific Idle
  timeout. Plan selections remain visible independently of the shared restoration switch.
- Explain rule priority under the Automation heading above the five switching
  modes, which appear in priority order. Show the current switching status in
  the current-plan card, including the reason when switching is paused.
  The Power Plan Control side panel shows the active Windows plan, the current
  automation decision, and lower-priority modes that also selected a plan in
  the same pass. Ordinary rule Status rails identify a confirmed pause due to
  Adaptive Processor Power, A/C, or missing observations.
- By Focus and By Running App default to matching an executable path. Executable
  name rules match the same filename across paths. By Focus evaluates the app with
  input focus. Process List edits individual path rules. The rule editors accept
  full drive or network paths and show guidance for unsupported path formats.
- In By Focus, Preview matches checks a full executable path against the draft
  rules as the user enters it. It shows the first eligible rule and its selected
  plan, followed by any other eligible matches in rule order.
- In By Running App, Preview matches lists the processes each rule covers,
  same-name copies outside path rules, and processes with unavailable details.
  The list loads on open and refreshes every three seconds while visible. The
  winner shows its selected plan using the same rule number and name as its card.
  Lower-priority matching cards show their selected plans. Power Plan Control
  decides the active Windows plan.
- CPU Sets (Soft) and Processor Affinity (Hard) use Status and Presets tabs in the
  shared docked panel.
- Instance Balancing lives under CPU Control and shares one configuration across
  power sources. Use executable-path rules with activation, edit and delete actions;
  keep configuration editable while disabled. The rule table shows each rule's latest
  live result for an exact match to a saved rule, with details on hover. Its Status
  panel shows the overall scheduling preference assignment or waiting reason.
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
- Keep the Background default visible in the master card. Show each detection
  switch, tier default, and child protection together in its card.
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
  that state. Setup shows the Enable CPU Limiter card followed by the selected
  profile's three default limit cards under CPU time. Custom tiers show a 1% to 100%
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

- Offer saved Windows plans in the target selector and identify a different
  active plan when the target is inactive.
  If the temporary Winderust Adaptive plan is active, show it as the active plan
  while the selected saved plan remains editable.
- Show Plugged In and On Battery settings in consecutive sections. Each has a
  preset selector, processor settings, and long- and short-running thread
  policies. Offer all six scheduling choices for recognized policy values. For
  unavailable controls, distinguish missing,
  unreadable, and unknown values, and show the raw index for unknown values.
  Keep the other controls editable. Percentage fields use the shared slider row.
  Give processor minimum and maximum separate rows and report invalid ranges
  beneath them. Require a valid range before opening the apply review.
- Built-in presets keep current scheduling policies; custom presets can capture
  choices or keep current. Re-read after every Apply attempt, including partial
  failure. Adaptive Engine does not own these persistent writes.
- Save opens the review for pending tuning edits. Show the target plan, any
  Windows power mode override warning, and only changed values, grouped by
  Plugged In and On Battery. Identify a known active plan when it differs from
  the target. Cancel keeps the tuning
  draft. If a catalog refresh finds the target unavailable, keep the review open
  and show the reason in the target card. Disable confirmation until a refresh
  confirms the target is available.
- Offer Refresh values and Restore previous tuning values in the Presets Side
  panel. Restore opens the review for the selected plan's previous values. It
  is available for the current Winderust session after a
  verified change and requires applying or discarding any pending edits first.

### Watchdog

- Place Watchdog after Dashboard in the workspace row. Show Resource
  Thresholds, Memory Trim, Keep Running, Sleep Prevention, Instance Limits,
  and App Suspension in that order. Reuse the existing landing-page cards and
  link each one to its own editor. Keep feature actions and help in the central
  editor beside the Watchdog page list.
- Each Watchdog feature uses shared settings across power sources and omits the
  Plugged In and On Battery selector.
- Resource Thresholds and Instance Limits keep shared options under Settings
  below their enable switches. Settings remain editable while the feature is off.
- App Suspension Setup puts Background Suspension Delay and Auto resume when focused
  under Settings. Follow with TCP detection, Audio detection, and Temporary resume
  sections. Group Detect TCP activity and Detect audio playback with their own
  delays. Put Resume periodically, Resume every, and Resume duration together in
  Temporary resume. Keep the timers editable while their switches are off and allow
  the last timer to scroll above the floating Save card. Per-app policies live in
  Custom Rules.
- Memory Trim starts with a Manual trim card above Enable Memory Trim. Trim now
  sits on the right, uses saved settings, and shows the running state while a request
  is pending. Show the last completed trim in the same card, with the observed
  working-set reduction and measurement coverage.
- Put each Memory Trim exclusion switch and threshold in its own card under
  Settings. Exclude focused apps and Exclude apps with visible windows apply
  to automatic trimming and Trim now.
- Place Notify beside Enable in the Resource Thresholds, Instance Limits, and
  Keep Running rule tables. Enable controls whether the rule runs. Notify
  controls Windows alert delivery. Resource Thresholds and Instance Limits
  also retain their feature-wide notification switches. For Keep Running,
  Notify opts the rule into a single alert when retries run out and Resume is needed.
- Put the Keep Running enable switch and Custom Rules on one page. Show the
  app picker above the table. The rule editor shows the selected application and
  one optional arguments field.
- Put Temporary keep awake and Temporary keep display on under one Temporary timers
  heading with separate action rows. Show an editable duration and Start when idle, then the end time
  and Stop when active.
- Put the Instance Limits enable and notification switches in Setup and the app
  picker and rule table in Custom Rules. Keep the table focused on the configured maximum and rule
  controls. Report detected incidents and notification outcomes in the Action Log.
- Use shared inline help for the rule picker when its tab already names the task.
- Keep Instance Balancing under CPU Control. Advanced Controls contains Timer
  Resolution and Win32 Priority Separation.

### Resource Thresholds

- Keep Resource Thresholds global across power sources, with the shared app
  picker and rule table. CPU and Memory columns show warning and critical
  thresholds in their configured units. The main page owns Sustained time and Windows
  notification. The rule dialog shows the app identity and CPU, Memory, and
  Preview matches tabs. OK stages the draft. Cancel, Close, and Escape discard it, including
  when a numeric field has focus. If a picker is open, Escape closes it first.
- Give Executable path and Executable name the same input width, clear control,
  and Add rule placement. The path input offers the running-app picker and
  Browse. The name input accepts an exact filename or a wildcard pattern such
  as `abc*def.exe`. Match the whole filename without regard to case. `*` matches
  zero or more characters and `?` matches one.
  Combine usage by executable path. Name rules can send notifications.
  Terminate, Restart, and Trim memory require an executable path. Offer Trim
  memory only for Working Set warning and critical actions.
- Preview matches reads a fresh process snapshot on tab entry and every three
  seconds while visible. Show each eligible match with its app icon, PID, name,
  and executable path. Keep the current rows visible while the next snapshot loads.
- Open each enabled resource monitor expanded and each disabled monitor
  collapsed. Enabling a monitor reveals its thresholds and actions.
  CPU thresholds use percentages. Memory thresholds use Working Set in
  percent of physical RAM or MB. Switching units requires an explicit pair of
  thresholds for the new unit and preserves each unit's inputs while the rule
  dialog is open. The action choices are No action, Alert, Restart, and Terminate.
  Name rules offer No action and Alert.
- Notifications lead with the action result, including cooldown skips, then trigger
  details. Keep the registered tray icon for enabled notifications independently of
  hide-on-close and respect Windows notification suppression. Runtime sequencing
  belongs in the [Resource Thresholds contract](runtime-contracts.md#resource-thresholds).

### Appearance, About, and Adaptive priority details

- Appearance groups Language, Theme, and Animation under Interface, keeps Accent
  independently expandable, and groups navigation and feature-card display settings.
  Use square color swatches and shared select widths. About groups identity and
  updates into separate surfaces.
- When feature-card status is on in Language and Appearance, landing cards backed
  by a feature switch show Enabled or Disabled in the theme's normal text color.
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
- Priority Control keeps options visible in their Focus, Visible Window, and
  Background section cards. Process and Memory
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
- Put concise explanations in the relevant feature cards and keep extended
  reference in the documentation. When both Status and Presets are available,
  show them in that order and open Status on navigation. Show a single panel
  directly.
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
- Keep the page title anchored at the top left. Power-source selectors stay beneath the title.
- Quit confirmation is centered over a dimmed window. With unsaved changes show Cancel, Save and quit, and Quit without saving; otherwise show Cancel and Quit. Shutdown errors stay visible.

- CPU triggers and recovery groups the monitored CPU readings and thresholds with the
  Busy app threshold, sample interval, and candidate limit. Its App recovery section groups the
  recovery threshold, minimum hold, and recovery duration shared by restraint and
  independent background processor limits. CPU Pressure Restraint groups its
  action and scheduling effects first, then restraint-specific timing and idle
  behavior, then protections. Limit Background Processors and Dynamic Resource
  Zones have separate toggle headers with visible descriptions. Keep disclosure
  state separate for live and preset editors.
  Zoning keeps its 1–99 foreground share and visible throughput warning. The
  limiter percentage refers to background capacity, and its allocation method
  remains editable when zones are enabled. Show verified zoning status and counts.
- In Limit Background Processors, show Only during CPU Pressure Restraint and its
  prerequisite before the allocation method in live and preset editors. Reduce affinity (Hard)
  shows a removal count in place of all fixed processor-selection controls. The two hard methods
  show Preserve custom affinity and Restore affinity before restart inside the same expandable
  card. The restart control describes its scope under Resource Thresholds.
- Restraint action, Restraint I/O priority, and Energy-efficient scheduling sit together in
  CPU Pressure Restraint. Processor placement only shows its allocation method and
  selection through Limit Background Processors and hides the priority adjuncts.
  The same CPU Pressure Restraint card groups the independent protections for the
  focused app, its child processes, and non-Normal priorities for every action.
- Separate foreground CPU threshold is available in the shared detection card when the source
  combines system and foreground CPU. Enabling it reveals separate System CPU threshold and
  Foreground CPU threshold sliders. The foreground help states that its unit is whole-system
  capacity.
  The I/O selector defaults to Unchanged and uses the same Low and Very Low labels as I/O Priority.

## Current reference layout

- The user-approved 2026-10-30 dashboard direction guides the palette, spacing, and metric hierarchy. The App Shell section owns current navigation and window behavior.
- Use 16px standard card padding, 12px card gaps, regular Segoe UI body text at 14px, 13px Side panel labels, and semibold headings. Describe text rendering in terms of advanced shaping, system font fallback, and DPI-aware software rendering.
- Show the current page name as an 18px semibold title.
- Dense virtual process and log tables retain borderless tiled backgrounds and existing row geometry.
- Keep System/Light/Dark/AMOLED and custom/system accent behavior. Do not infer new telemetry or optimization guarantees from the reference image.

## Dashboard

- Open Dashboard at startup. Put a compact row of CPU, RAM, Disk I/O, and
  Network I/O charts above Process List. Give the process table the remaining
  height and keep its own scrolling, search, and row actions.
- Give every usage card the same height, with the metric label and live reading
  at consistent positions. Set the unit in smaller text beside the value. On
  wide cards, put a taller sparkline beside the reading and align its scale
  label with the chart's right edge. On narrower cards, put the plot below the
  reading and hide the scale label. Draw only the upper and lower guides and
  tint the area under the primary series.
- Compact charts show 60 seconds of recorded history without axes. Hover shows
  the sample timestamp and named series values in a floating tooltip, with a
  dashed crosshair and point markers on the plot. Keep tooltips within the viewport.
  Show gaps when samples are unavailable or paused, and retain recorded zeros.
  Use decimal byte rates for disk and network. Round their scales up to cover
  the peak combined rate in the displayed history. Plot CPU frequency relative to
  its reference frequency on the shared percentage scale and show the current
  GHz reading separately.
- Tab reaches charts in reading order. Left or Right starts at the newest recorded reading, then moves to older or newer readings and skips gaps. The focused chart displays the selected timestamp and named values in its header without changing card height. The selection stays on the same observation as new samples arrive. Escape restores the current readings, and the header returns to current readings when the selected observation ages out.
- RAM Usage charts physical memory. Show physical memory used and total in the
  live reading. Include physical memory used and available, and system commit
  against the Windows commit limit, in the chart tooltip.
- Position chart samples by elapsed time, retaining fractional seconds. Allow up to 1.5 seconds between consecutive valid observations for one-second sampling with dispatch and worker jitter; larger intervals and explicit unavailable readings break the curves. Hover selects the nearest recorded observation before checking that channel's availability.
- Windows contrast themes use a solid first series and a dashed second series,
  with round and square markers in the plots and readouts. Leave the plot
  background clear so both curves stay readable with the same Windows color.
- While a sample is overdue, show Collecting and continue aging the history
  while Dashboard is visible.
- In the Side panel, show the power source, current power plan, Enabled Features,
  and Recent Logs under Status. List up to five recent entries and link to Action
  Log. Use Settings for process filters, grouping, columns, and units.
- Recent Logs shows compact INFO (Applied/Restored), WARN (Skipped), and ERROR
  (Failed) levels from recorded Action Log entries. The Log table keeps Level
  and Result separate. Its level filters, capture thresholds, and CSV level
  field share the same mapping and short labels in both languages.
- In the focused Log table, Left and Right reveal offscreen columns. Page Up,
  Page Down, Home, and End move through the rows.
- In Log, search by process name or exact PID. Search combines with level
  and feature filters before entries are grouped and returns the list to the
  first row. Group each filter with its own label. Capture Mode affects new
  entries after Save. Level Filter changes the table, and Export Log uses
  the full recorded snapshot.
- Keep category shortcuts in navigation and global search; omit the old dashboard shortcut grid.
- Dashboard sampling and process refresh run only while Dashboard is selected
  and the window is visible. Background automation retains its independent
  monitoring.
- After a sampling gap exceeds 1.5 seconds, CPU Usage, Disk I/O, and Network I/O
  start a fresh rate baseline. The first reading shows Collecting until a
  fresh interval supplies a rate.
- Enabled Features uses the sampled Windows power source to select the displayed
  profile. Show the feature count when the power source is known. The feature
  editors retain their selected profile.

## Verification

Use the [render harness](development.md#rendering-and-performance) for both locales,
light/dark themes, AMOLED, minimum-size and desktop windows, empty/populated rules,
collapsed/expanded groups, live/editable/read-only presets, workspace navigation,
Side panels, and search. Check header controls independently from expansion; each
must retain its own hit target. Preserve live/preset navigation and collapse-state
isolation.

Follow the [PStack verification workflow](development.md#pstack-verification) for
changed workflows. Review the captures and check the relevant Windows interactions
at the supported window sizes and display scaling. Check keyboard access, readable
focus and contrast, and accessible names where a change affects shared controls.
Do not recreate unmapped historical features or infer telemetry/performance promises
from a reference image. Completed port audits and their old findings remain in Git
history rather than defining current behavior.
