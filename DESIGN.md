# Design

## Source of truth
- Status: Active
- Last refreshed: 2026-10-30
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
- Color: Cool pale background, white panels, and a muted blue-gray navigation surface in light mode; corresponding slate surfaces in dark mode. Configured accent marks selection and primary actions; status colors remain semantic.
- Surface hierarchy: Inputs, dropdowns, and their menus use the neutral control surface, distinct from cards. Controls are subtly lighter in Dark and AMOLED and darker in Light. Keep AMOLED workspace and dock surfaces pure black, and internal settings dividers subtler than card outlines.
- Typography: Segoe UI, regular 14px body text, 13px navigation labels, semibold headings, and prominent 28px dashboard values.
- Spacing/layout rhythm: More breathing room between panels with 16px shared card padding and 12px gaps; keep operational tables dense and stable.
- Shape/radius/elevation: 10px panel corners, 6px control corners, and subtle 1px panel outlines; avoid decorative shadows and blur.
- Motion: Small control transitions respect Animation Mode. Page, panel, and content-layout changes remain immediate; `.agents/memory/15-design-spec.md` is the authority for motion and scrolling.
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
- Layout adaptations: Center the editor within the remaining work area. An open side panel reserves its own width; dense rule tables scroll horizontally when space is limited, and slider labels sit above their controls.
- Touch/hover differences: Desktop-first; essential meaning must not rely on hover alone.

## Interaction states
- Loading: Use existing status/refresh patterns.
- Empty: Explain that no applications are configured and provide the process picker.
- Error: Show actionable status and Action Log entries without repeated spam.
- Success: Reflect applied process counts and per-rule indicators.
- Process actions: Stacked parent rows show only `Stop process tree`, covering every process in the stack and their descendants; expanded sub-items show both stop actions. Non-stacked rows show `Stop process`, plus `Stop process tree` only when the process currently has children.
- Navigation: Language and Appearance can show enabled-feature count pills in the secondary header for Winderust Features, Power Plan Control, Priority Control, and CPU Control; the primary rail stays unbadged.
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
- Use Iced's native widgets and interaction states with Winderust's pale light/slate palette. Keep shared outlined panel surfaces, tinted selected navigation, and hover/press feedback in widgets.rs.
- The Windows/custom accent preference seeds the primary palette; shared neutral surface, border, and muted-text roles preserve hierarchy across light and dark themes.
- Reserve filled primary buttons for primary commands. Use text buttons for secondary commands, gentle accent interaction shading, and subtle panel outlines.
- Keep application-specific layout: grouped navigation, readable page headings, compact dashboard charts, constrained form rows, status rails, and scrollbar spacing.
- Retain bundled icons, charts, and bounded motion where standard widgets do not cover existing functionality.
- Add custom styling only for a concrete requirement unsupported by Iced's built-ins.

## Open questions
- None blocking implementation. Density preferences can be adjusted after reviewing native screenshots.

## Current dashboard reference (2026-10-30)
- Reference: the user's supplied light Windows dashboard image. The user requested the direction across the entire interface, including navigation.
- Apply pale blue-gray chrome, white panels with fine cool borders, restrained rounded corners, generous panel spacing, clear metric hierarchy, and tinted active navigation across existing pages.
- The latest sidebar reference replaces the nested tree with a permanent icon-and-label rail and a separate secondary navigation panel. Both panels dock flush to the window edges with square corners and thin vertical dividers, without outer gaps or floating card outlines; preserve every feature route and pinned utility links.
- Keep System/Light/Dark/AMOLED, Windows/custom accent, native window framing, localization, and existing motion preferences.
- Center content within a maximum 1440px workspace. Forms, tables, and page headings keep their current ownership and controls.
- Home follows the latest dark dashboard reference: a two-by-two CPU/RAM/Disk I/O/Network I/O chart grid above equal-width Enabled Features and Recent Logs panels. Size the cards from available content height as well as width. Use four metric columns in wide, short workspaces (at least 1280px wide and below 680px high); otherwise use two columns from 720px wide, or one column below that. Omit redundant dashboard subtitles at every size. Keep chart cards at least 200px high and summary panels at least 160px high; use page scrolling only when these readable minimums cannot fit. Charts show a 60-second history, scale labels and legends. Hover uses a floating, viewport-clamped tooltip with the sample timestamp, named series values, a dashed crosshair, and point markers; CPU headers show Usage on the left and Frequency on the right. Render only recorded chart samples; missing history stays blank while sampled zero remains visible. RAM headers show Usage with a Used label and used/total capacity on the left and Available on the right. Disk and Network I/O headers show their two named, curve-colored metrics on the left and Total on the right; network uses decimal Mb/s, and CPU frequency is explicitly relative on its shared percentage scale. Enabled Features and Recent Logs retain fixed card geometry and headers. Enabled Features has no View All footer; Recent Logs retains its Details footer. Center empty messages within the content area; populated lists use inset scrolling inside the cards. Recent Logs shows actual Action Log outcomes and links to Log. Main section shortcuts live in navigation and global search rather than the dashboard. Dashboard sampling runs only while Home is shown and the window is not hidden to tray; omit manual Pause/Resume metrics controls.
- Only display available real telemetry. The reference's GPU, watts, battery estimates, and generic optimization claims do not authorize new backend features or simulated values.
- Shared setting cards, landing links, dialogs, and status surfaces use the same palette and geometry; retain dense borderless process and log tables, virtualization, and runtime safety.
- Render checks use `--features render-smoke -- --render-smoke` for all pages in English/light and Traditional Chinese/dark, plus compact navigation, expanded groups, large desktop layouts, and AMOLED dashboard, table, settings, flyout, and search views.

## Interaction and readability refinement
- Use Windows Segoe UI typography for small-text readability in Iced, with 14px body text and 13px sidebar labels. Keep Iced's existing advanced shaping, system font fallback, and DPI-aware software text rendering; do not add a second renderer or claim ClearType support.
- Combine Winderust's compact icon rows and inline feature status with the current reference's light panel hierarchy and selective accent. Hover changes the surface; pressing strengthens feedback without changing layout geometry.
- The primary navigation rail is 88px wide with vertically stacked icons and 11px centered, wrapping labels. Every primary item is 64px high; center the icon and actual one- or two-line label together vertically, with equal top and bottom insets. Icons and labels center on the full tile width; group chevrons overlay the right edge of the icon row and align with the icon centerline. The thin primary scrollbar floats within the outer padding so it does not shift the main entries relative to the footer. Combined navigation scales from 248px to 320px; the secondary list wraps long labels to preserve the narrow-window work area. The right status dock retains its separate compact width.
- Center the shared header and content canvas within the work area beside the primary rail and any open side panel, with shared padding and a 1440px width cap. Reserve the same 48px breadcrumb area and 32px toolbar on every page, including pages without power-source controls. Page content starts at the same position across routes.
- The right side panel is a 320px dock beside the editor. Opening it reserves width rather than covering content; both areas remain interactive. It starts closed and retains its open state throughout navigation, search, focus changes, and settings edits. Close only through the panel controls. Pages without panel content use the full canvas, preserving the session's open preference for the next supported page. Keep existing tabs, scrolling, and actions, with the unsaved-settings notice inside the editing area and dialogs above both areas. There is no collapsed strip or persisted mode setting.
- Page introductions live in the right sidebar. Use shared Status / Info tabs, adding Presets between them where available. Advanced Power Plan Tuning uses Presets / Info. Reset to the first tab on navigation; pages with only reference information show it directly without a lone tab. Give descriptions the full scrollable sidebar height and remove the page-header info action and inline disclosure; show concise setting descriptions, essential consequences, validation, and live runtime warnings beside their controls.
- Native render checks also cover English/Chinese Info tabs, long English navigation at minimum width, and the compact sidebar.

## Setting-card structure
The source-mapped hierarchy corrections are implemented. See [the design integrity review](docs/iced-design-integrity.md) for the original findings, corrections, and verification limits. Card surfaces alone are not evidence of parity.
- Keep standalone setting rows and expandable groups on shared panel surfaces, applying the current dashboard reference to their colors, borders, and spacing.
- Group related settings in one shared card beneath a concise, localized category heading outside its outline, with subtle internal dividers between complete rows. Apply the same structure to preset and rule dialogs. Independent collapsible groups use their own headers. Reserve standalone cards for independent settings and collapsible feature groups. Keep labels and their controls together, with consistent padding and vertical alignment.
- Show concise setting descriptions directly beneath their labels in readable 13px secondary text, with controls aligned on the right. Rows and group headers grow when labels or descriptions wrap; keep units in labels and avoid fixed heights that clip help. Use the shared setting-title layout for live pages and preset dialogs. Keep essential consequences inline, longer page-level explanations in the sidebar's Feature info section, and icon-action and truncated-data tooltips available. Preserve dense process and log table geometry.
- Chevrons are passive indicators inside shared clickable controls. Primary section entries navigate to their landing page and open the secondary panel; selecting the current landing section toggles that panel. Secondary lists show the existing section pages without a redundant landing-page row; primary category buttons open their landing pages.
- Use shared theme-aware dividers between complete settings in a shared card. Keep each label, inline description, and associated controls together; omit leading/trailing dividers and extra rules around single-setting cards.
- Each expandable group is one enclosing card containing its full-width clickable header, state chevron, and body. Preserve the existing collapse state and motion preferences.
- Use widgets::settings_card for standalone rows and widgets::setting_group for independent header actions and flat expandable bodies. Compose standard Iced containers/buttons; use the shared subtly outlined surfaces and existing hover styles. Do not restore GPUI dependencies or vendored code.
- Custom rule and exclusion rows have card boundaries. Process List remains a dense table, matching the old GPUI design. Headers, explanatory text, and command bars do not need independent cards.

- Card outlines remain stable across hover and pressed states; background shading supplies interaction feedback. Retain native input/control boundaries.

## Operational layout rules
- Keep headings, forms, dashboard, and tables aligned within the shared workspace.
- Use one 18px breadcrumb heading on every page: Home > section > current page, omitting unused levels. Ancestors are clickable; the current page is semibold text. Home shows only Home. Wrap the path in narrow windows; do not repeat the current page as a separate title. Retain Segoe UI and native Windows framing.
- All page, panel, and dialog tabs use the same compact segmented design: 32px height, widths that fit their centered 14px semibold labels, one neutral outline, no outer inset or gap, and an accent-tinted, outlined selected segment. Weekday selectors share this design and retain multiple selection. Plugged In / On Battery retains its live-source dot independently of the selected editing profile. Follow the current theme and accent without shadows or underlines.
- Buttons share 6px corners, 14px labels, and stable 1px outlines. Standard text actions and utility icons are 32px high; text actions use 12px horizontal padding. Primary actions use a solid accent fill; secondary actions use a neutral outline with accent text; Cancel and other tertiary actions use a neutral outline and text. Quiet utilities show a hover fill, destructive actions use danger colors, and persistent choices retain accent outlines and tint. Preserve explicit dense-row and navigation geometry, theme/accent support, native input, and disabled states. Use the shared action and icon helpers; keep action feedback distinct from persistent selection.
- Home section shortcuts follow the charts and feature status; narrow windows use two shortcut columns.
- Pin Log, Settings, and About below the scrollable primary navigation. Search opens an independent palette above the page; the category header X hides only the secondary navigation panel.
- Landing cards are compact, single-line navigation rows. Descriptions remain available in each feature page's right-sidebar Feature info section.
- Process List keeps its virtualization and configurable columns, with 52-pixel rows, aligned names/icons, and internal row dividers. Action Log uses aligned table columns and hover text for truncated reasons.

- Adaptive Engine uses numeric steppers with units and contextual help, right-aligned switches, compact segmented tuning tabs, and section breadcrumbs. Its side panel docks beside the editor and reserves its own width.
- Adaptive Engine Priority Control keeps each control's advanced options in its own expandable row, aligned under Focus, Visible Window, and Background. Process and Memory Priority include detection and priority-preservation controls. Disabling detection falls through to the next applicable tier without changing CPU restraint's focus protection. Process Priority always protects existing High/Realtime classes; optional same-or-higher/lower preservation defaults off. Memory preservation defaults on. Both detection layers default on, retaining existing behavior. Custom presets capture these options; built-in presets restore their defaults.

- Shared setting rows use a right-aligned On/Off switch or bounded numeric stepper. Status and tuning presets use the shared docked side panel at every width. Preserve all additional policy options below the main Adaptive Engine table. About groups identity and update controls into separate surfaces; Appearance groups Language, Theme, and Animation under Interface, keeps Accent independently expandable, and groups navigation and feature-card display settings. Use square color swatches. Unsaved settings remain in a lower-right notice.

- Custom-rule app selection uses the shared src/ui/app_picker.rs search dropdown: a compact input beside Add, two-line app name/path entries with cached icons and aligned placeholders, and Browse local executable inside the menu. The popup overlays the page, stays within the viewport, and supports keyboard selection and outside-click dismissal. Each feature retains its own eligibility and duplicate checks; unavailable App Suspension targets remain visible but disabled.

- Removing a rule, exclusion, or custom preset updates the settings draft immediately. Do not add a second removal confirmation; Save commits the draft and Discard restores the saved settings.

Dropdowns use the shared Select control: a native Iced pick-list field with a native-widget overlay menu, persistent accent selection marker, gentle accent hover, and white/base option text. The menu stays within the viewport, scrolls to the selected option on open, and supports keyboard selection and outside-click dismissal.

The shared palette derives workspace, navigation, card/field, button, border, and muted-text colors from the selected accent using restrained tints over light/dark grayscale tones. Keep those shared roles responsive to both custom and Windows accents; avoid fixed blue-gray surface colors. Chart series and informational Process List actions/statuses use accent roles, while success, warning, and danger retain their semantic colors. Accent fills, checkbox marks, and switch thumbs follow the original Winderust rule: white in light mode; in dark mode, white when weighted RGB brightness (0.299R + 0.587G + 0.114B) is below 140, otherwise #111111. Keep semantic status colors distinct. AMOLED is an explicit fourth theme: keep the workspace and docked navigation pure black, with near-black accent-tinted cards and controls, readable neutral borders, and the same semantic colors and dark-mode foreground rules. Windows appearance comes from UISettings.GetColorValue; explicit theme and custom accent preferences override system values.

Inactive navigation icons and resting control borders use neutral tones. Active navigation retains the accent; enabled switch thumbs use the shared theme-and-accent foreground rule. Expanded setting groups separate the header from the body with a subtle 1px neutral divider; complete settings inside multi-row cards use the same inset divider with even spacing, and the active custom color swatch has a contrasting outline. Appearance dropdowns use the shared select width.

## Two-panel sidebar reference (2026-10-30)
- Reference: the user supplied a narrow labeled icon rail alongside a separately outlined Power menu with a header close button. Global search uses a focused search palette above the page, with results, Enter to open, and Escape/outside-click dismissal; it never opens or changes the secondary navigation. Apply this structure to all existing Winderust categories in both themes.
- Active section styling remains visible in the primary rail while visiting any child page. Enabled-feature counts belong in the secondary header; enabled page markers remain in its list.
- The remembered navigation-collapsed preference controls the secondary panel. Clicking a primary category can reopen it; Search leaves its state unchanged. Direct leaf pages use only the primary rail.
- Global search follows the Fumadocs search-dialog reference: a compact 640px upper-center panel with a borderless search field, Esc keycap, and a compact category-filter row sharing the panel background. Esc uses the standard shared button and the filter uses the standard shared Select, including their project theme and interaction styles; compact results highlight the selected row. Center each result icon, one- or two-line label, and chevron vertically within its fixed-height row. New queries and filters select the first result; Up/Down move selection within the list and scroll it into view, and Enter opens the selected page. An empty All search omits the result list; typing or choosing a category reveals bounded inset results. Use a dimmed backdrop and outlined, shadow-free panel with the existing software renderer; blurred quad shadows accumulate outside partial repaint damage. Global search finds pages across all categories and respects Advanced Controls visibility. Choosing a result closes and clears the palette and navigates to that page and its category.
- Use existing Iced widgets, scrolling, bundled icons, and shared styling; navigation panel layout changes are immediate. Do not change runtime feature ownership or settings.

- Search defaults to Ctrl+K or / within the Winderust window. Respect keystrokes captured by text fields and menus. Settings > Winderust Behaviour provides primary and alternate shortcut recording, clearing, and default restoration through the normal Save workflow. The rail Search entry matches navigation item height, icon/label spacing, and vertical centering. Navigation entries, including Search, show no hover tooltips. Show only the Search icon and label in the rail and use a subtle neutral-text tint while the palette is open so the current page remains the primary selected entry.

- Surface hierarchy: left navigation and the right panel share the same docked chrome background, square outer corners, and thin separators against the workspace. Keep gaps inside panel padding rather than between shell surfaces. Cards, rules tables, dense table frames, settings groups, and settings dialogs use the shared surface outline (1px, neutral border at 75% opacity) and 10px radius. Dense table frames stay outside horizontal scrolling; their inner tiled background remains borderless for partial repaint efficiency.

- Process List refreshes automatically about once per second while its page is shown and the window is not hidden to tray. Keep only search and process count in its command row; omit manual refresh and population pause controls.
