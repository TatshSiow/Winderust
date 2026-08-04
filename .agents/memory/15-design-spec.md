# Winderust Design Spec

This file describes the current app design direction. Use it when changing GPUI UI code, adding pages, or cleaning UI helpers.

## Product Feel

Winderust is an operational Windows power and process-control utility. The UI should feel dense, calm, scannable, and repeat-use friendly. Do not turn it into a marketing page, hero layout, or decorative dashboard.

Prefer visible state, compact controls, and predictable rows over large illustrations or card-heavy presentation.

## App Shell

- Keep the app shell as custom title bar, left navigation, and right work area.
- The title bar is compact: app icon/name, short description, native-feeling window controls.
- The sidebar is grouped by product area through `Page::sections()`. It is
  searchable when expanded and keeps a search action in its remembered compact
  icon rail; the quiet navigation-styled toggle stays below a divider in normal
  sidebar flow rather than floating over content. Keep icon and row geometry
  stable across the animated expanded/compact transition.
- The main page area scrolls vertically and keeps content constrained with stable width behavior.
- Navigation labels and page sections live in `src/ui.rs`; page rendering
  dispatch stays in `WinderustApp::render_page` in `src/ui/app/pages/app_shell.rs`.

## Layout Rules

- Use `h_flex()` and `v_flex()` consistently with `min_w(px(0.0))` / `min_h(px(0.0))` on flexible children.
- Rows should have stable heights. Existing defaults are `CARD_ROW_HEIGHT`, `PROCESS_LIST_ROW_HEIGHT`, and `PROCESS_LIST_HEADER_HEIGHT`.
- Use fixed or computed widths for tables and policy columns. Do not let dynamic labels resize the process list.
- Use `truncate()` for long process names, labels, and status values inside constrained rows.
- Avoid nested cards. Cards are for repeated rows, setting groups, status blocks, popovers, and tool surfaces.
- Keep cards at the existing `BRAND_RADIUS_SURFACE` / `BRAND_RADIUS_CONTROL` scale. Do not introduce large rounded marketing panels.

## Components

- Reuse local helpers before adding new wrappers:
  - `setting_group`, `setting_group_with_help`, and `setting_group_action_row` for settings.
  - `control_button`, `primary_control_button`, and `remove_control_button` for actions.
  - `dropdown_select_control` and existing dropdown helpers for option sets.
  - `switch_toggle_action`, `checkbox`, inputs, sliders, and steppers for their natural control types.
- Use switches or checkboxes for binary state.
- Use sliders, steppers, or numeric inputs for numeric settings.
- Use dropdowns for bounded option sets.
- Use icon buttons for compact repeated actions; text buttons are fine for clear primary commands such as save/import/export.

## Visual Language

- Base surfaces are neutral and restrained. Accent color marks primary action, active navigation, selection, and important status.
- Respect `AppThemeMode`, `AccentColorSource`, and system accent behavior through `cx.theme()` and existing color helpers.
- Do not add purple/blue gradients, decorative blobs, glow backgrounds, or one-note palettes.
- Status colors should stay semantic: success for active/applied, warning for caution, danger for destructive or failed actions.
- Text hierarchy is compact: small labels, body rows, muted helper text. Avoid hero-scale text inside panels.

## Icons

- Use `Icon::new(NavIcon::...)` for Lucide icons already registered through `src/ui/assets.rs`.
- If adding a Lucide icon, update both `NavIcon` and `src/ui/assets.rs`.
- Do not remove `icondata_core`, `icondata_lu`, or Lucide asset generation unless every `NavIcon` and generated SVG use is traced first.
- Keep action icons at existing sizes, usually 12-18 px depending on row density.

## Motion

- Preserve motion unless the user explicitly asks to remove it.
- Respect `AnimationMode`: system/on/off flows through `ui_animations_enabled()`.
- Use existing motion helpers such as `with_optional_motion`, `begin_expandable_motion`, `begin_control_motion`, hover layers, and collapsible chevrons.
- Motion should clarify state changes: selected navigation, hover, dropdowns, popovers, switches, collapsible groups, and process groups.
- Keep animation IDs stable and bounded. Do not create unbounded global motion state keyed by volatile data.

## Process List

- The process list is a dense table, not a card grid.
- Keep process icon, grouped process row, PID/count, and policy columns visible and stable.
- Keep the User column at the right edge. Show the exact process token account when available. When Windows denies token access, show `Unavailable · S#` with an explanatory tooltip; identify kernel pseudo-processes as `Windows kernel`.
- Keep Windows-critical processes visible as read-only `Protected system process` rows in Process List, omit them from every Add process picker, and fail closed when criticality cannot be verified.
- For other inaccessible rows, show `Administrator required` before elevation and `Access denied` when Winderust is already elevated.
- When elevated, enable Task Manager-level service access before process discovery and automation while retaining every critical, built-in, identity, and protected-process barrier.
- With Advanced Controls enabled, keep Suspend/Resume present in the context menu and use the existing disabled style when the selected process cannot be safely suspended. Grouped rows apply the action to every captured process in the group. Keep unavailable App Suspension picker candidates visible, disabled, and explicitly labeled `Unavailable`. App Suspension safety must not disable unrelated process controls.
- Stacked Process List parent rows expose only `Stop process tree`, covering every process in the stack and the union of their descendants; their expanded sub-items expose both stop actions. Non-stacked rows always expose `Stop process` and expose `Stop process tree` only while the selected process has a child in the current process snapshot.
- Report `Efficiency mode` only when the process has both EcoQoS and `IDLE_PRIORITY_CLASS`, matching Task Manager rather than treating EcoQoS alone as full Efficiency mode.
- Column visibility belongs in the existing dropdown/checkbox pattern.
- Language and Appearance owns the enabled-feature counts toggle. When enabled, expanded navigation shows numeric pills beside Winderust Features, Power Plan Control, Priority Control, and CPU Control based on each section's enabled modules; collapsed navigation keeps the pills hidden to preserve icon alignment.
- Language and Appearance separately owns the feature-card status toggle. When enabled, shared landing and search-result cards show Enabled or Disabled pills only for pages backed by a top-level feature switch; pages without a single enabled state remain unbadged.
- Editable policy cells should use inline dropdown controls. Avoid modal flows for simple policy edits.
- Preserve fixed column layout calculations and tests when adding columns.

## Settings Pages

- Adaptive Engine is the parent of Workload Engine. Workload Engine may retain its child configuration while the parent is off, but it must not run or appear as independently enabled.
- Prefer one setting per row when possible.
- Use collapsible setting groups for advanced or multi-row settings.
- Put explanatory text in muted helper labels or info popovers, not large instruction blocks.
- Settings that affect Windows behavior should show conservative defaults and explicit enable controls.

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
