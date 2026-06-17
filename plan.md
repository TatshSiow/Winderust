# PowerLeaf Function Organization Plan

## 1. Purpose

PowerLeaf has grown enough that related functions are hard to find. The immediate goal is not to change behavior. The goal is to sectionize the app so every function has an obvious home, starting from the existing navigation groups in `src/ui/mod.rs`.

This plan should be used as the checklist for future refactors. Each migration should keep the app compiling after every step.

## 2. Current Evidence

- `src/main.rs:6` through `src/main.rs:33` declares many top-level domains, so the app already has useful feature modules.
- `src/ui/mod.rs:4` defines `Page`, and `src/ui/mod.rs:56` defines the six visible navigation sections.
- `src/app.rs:206` defines `PowerLeafApp`, but the same file also owns page rendering, settings mutation, shared widgets, labels, registry helpers, dialogs, and tests.
- `src/app.rs:2784` dispatches page rendering, while individual pages are implemented from `src/app.rs:2813` through `src/app.rs:9026`.
- `src/app.rs:9456` through `src/app.rs:15038` contains many shared helpers that are not specific to one page.
- `src/automation.rs:136` defines `BackgroundAutomation`; `src/automation.rs:326` starts the worker loop; `src/automation.rs:1216` defines `HiddenAutomationRunner`.
- `src/config/settings.rs:10` defines the full `Settings` tree, with feature-specific settings from `src/config/settings.rs:257` through `src/config/settings.rs:761`.
- Feature backends already follow a manager/snapshot pattern, for example `src/ecoqos/mod.rs:107`, `src/suspension/mod.rs:89`, `src/cpu_limiter/mod.rs:59`, `src/smart_trim.rs:63`, and `src/watchdog/mod.rs:61`.
- Generic rule infrastructure is already separated under `src/rules/mod.rs:1` through `src/rules/mod.rs:9`.

## 3. Target Sections

Use the existing navigation model as the product-level section map:

| Section | Pages | Current Source |
| --- | --- | --- |
| Overview | Dashboard landing page | `src/ui/mod.rs:32`, `src/app.rs:2813` |
| Power Plan Automation | Foreground Rules, Performance Mode, CPU Usage, Activity, Schedule | `src/ui/mod.rs:34`, `src/app.rs:3153`, `src/app.rs:3300`, `src/app.rs:3501`, `src/app.rs:3687`, `src/app.rs:5625` |
| Processor Controls | Core Parking, CPU Limiter, Background CPU Restriction, CPU Affinity | `src/ui/mod.rs:41`, `src/app.rs:4878`, `src/app.rs:5224`, `src/app.rs:7052`, `src/app.rs:8485` |
| Process Policies | Efficiency Mode, Foreground Responsiveness, I/O Priority, SmartTrim, App Suspension, Watchdog | `src/ui/mod.rs:47`, `src/app.rs:3975`, `src/app.rs:4481`, `src/app.rs:5412`, `src/app.rs:5757`, `src/app.rs:6546`, `src/app.rs:6740` |
| Log | Log | `src/app.rs:9026` |
| Settings | PowerLeaf Behaviour, Language and Appearance, About | `src/ui/mod.rs:54`, `src/app.rs:7952`, `src/app.rs:9010` |
| Advanced | Win32 Priority Separation | `src/ui/mod.rs:55`, `src/app.rs:8083` |

These sections should become source-code sections too. If a feature appears in the navigation, its UI, settings, rules adapter, runtime manager, and tests should be easy to locate from that section name.

### 3.1 Section Landing Page Navigation

The left navigation should show the major work areas, not every detailed page. Each section becomes a first-class page. Clicking a section opens a landing page with compact cards for the pages inside that section.

Example: clicking `Power Plan Automation` opens a section page with five cards:

- Foreground Rules
- Performance Mode
- CPU Usage
- Activity
- Schedule

Clicking one card navigates to the existing detailed page.

Navigation behavior:

- The left nav renders section pages only: Overview, Power Plan Automation, Processor Controls, Process Policies, Log, Settings, and Advanced.
- Overview acts as the dashboard landing page: CPU Usage on the left with a fixed 30-sample graph, Enabled Rules as a right-side list, and all dashboard cards using the same two-column width. Cards should stay max two columns when there is room, then expand to full available width when wrapping to one column. CPU Usage and Enabled Rules should share the same fixed outer card height, with titles and content contained inside that height so the Main sections heading cannot overlap either card. CPU Usage should use one card shell only; the graph itself should not be framed as a nested card.
- Child pages keep their current render functions and behavior.
- When a child page is active, the left nav highlights its parent section.
- Child pages should show a compact breadcrumb/header such as `Power Plan Automation / CPU Usage`.
- Child breadcrumbs should be clickable: clicking the parent section returns to that section landing page.
- App navigation should maintain local back/forward history for left-nav clicks, landing cards, and breadcrumb clicks.
- Mouse side buttons should support Back/Forward through app history.
- Section landing cards should be operational and dense: icon, title, current status or rule count, optional warning/error state, and a chevron.
- Overview main section cards should follow the same visual order as the left navbar: drawer sections first, then footer sections.
- Avoid marketing-style cards. These cards are navigation and status surfaces for a utility app.
- The Settings landing page should not contain a redundant `Settings` card. It should expose Settings categories directly: PowerLeaf Behaviour, Language and Appearance, and About.

Implementation direction:

- Add landing variants to `Page`, for example `PowerPlanAutomation`, `ProcessorControls`, `ProcessPolicies`, `AppHome`, and `AdvancedHome`.
- Update `PageSection` so each section has a `landing_page: Page` plus `pages: &'static [Page]`.
- Update navigation rendering so it iterates over `Page::sections()` and links to `landing_page`.
- Add a helper such as `Page::parent_section_page()` so child pages can highlight the correct section and render breadcrumbs.
- Add a reusable section-card component in `src/app/widgets.rs`.
- Render landing pages from the existing section page arrays instead of hard-coding duplicate lists.
- Render Overview main cards from `Page::sections()` so Power Plan Automation, Processor Controls, Process Policies, Log, Settings, and Advanced stay in sync with the left navigation.

## 4. Proposed Source Layout

Keep `src/app.rs` as the public entry point during the first pass, then move code into submodules under `src/app/`.

```text
src/
  app.rs
  app/
    state.rs
    render.rs
    navigation.rs
    dashboard.rs
    power_automation/
      mod.rs
      home.rs
      activity.rs
      foreground_rules.rs
      schedule.rs
      cpu_usage.rs
      performance_mode.rs
    processor_controls/
      mod.rs
      home.rs
      core_parking.rs
      cpu_limiter.rs
      background_cpu.rs
      affinity.rs
    process_policies/
      mod.rs
      home.rs
      efficiency.rs
      responsiveness.rs
      io_priority.rs
      smart_trim.rs
      suspension.rs
      watchdog.rs
    action_log.rs
    app_pages/
      mod.rs
      home.rs
      settings.rs
      about.rs
    advanced/
      mod.rs
      home.rs
      win32_priority_separation.rs
    widgets.rs
    inputs.rs
    labels.rs
    dialogs.rs
    theme.rs
```

Suggested boundaries:

- `src/app.rs`: `PowerLeafApp`, `UiInputs`, constructors, persistence entry points, and module declarations.
- `src/app/render.rs`: top-level `Render` implementation, title bar, page dispatch, shell frame, and status bar.
- `src/app/navigation.rs`: section-only navigation rows, parent-section highlighting, nav status, icons, and section labels.
- `src/app/widgets.rs`: reusable section cards, rule rows, setting groups, toggles, sliders, dropdown controls, and status pills.
- `src/app/inputs.rs`: input state creation, slider syncing, process picker input state, and rule title editing.
- `src/app/labels.rs`: pure formatting and label functions.
- `src/app/dialogs.rs`: settings import/export dialogs and action-log CSV export path selection.
- `src/app/theme.rs`: accent color, palette, Windows theme reads, and GPUI appearance application.
- Section page files: page-specific render functions and small page-local helpers only.

## 5. Extraction Map From `src/app.rs`

Move in this order to reduce privacy churn and keep diffs reviewable.

### 5.1 Top-Level Rendering

Move these into `src/app/render.rs`:

- `render` at `src/app.rs:2556`
- `render_title_bar` at `src/app.rs:2613`
- `render_navigation` at `src/app.rs:2657`
- `render_status_bar` at `src/app.rs:2714`
- `render_unsaved_popup` at `src/app.rs:2731`
- `render_page` at `src/app.rs:2784`
- `page_shell`, `page_content_frame`, and `page_shell_with_help` at `src/app.rs:10586`

Acceptance check: changing `Page` selection still routes to the same page output.

### 5.2 Overview Page

Move these into `src/app/dashboard.rs`:

- `render_dashboard` at `src/app.rs:2813`
- `render_cpu_usage_graph` at `src/app.rs:2865`
- dashboard summary helpers at `src/app.rs:2921`, `src/app.rs:2928`, `src/app.rs:2994`, `src/app.rs:3006`, `src/app.rs:3066`, and `src/app.rs:3098`

Acceptance check: Dashboard still compiles without importing page-specific modules from other sections.

### 5.3 Power Plan Automation Pages

Move these into `src/app/power_automation/`:

- Section landing page: `home.rs`, rendered when `Page::PowerPlanAutomation` is selected.
- Activity: `render_activity_page` at `src/app.rs:3153`
- Foreground rules: `render_foreground_rules_page` at `src/app.rs:3300`, `render_foreground_rule` at `src/app.rs:3374`
- Schedule: `render_schedule_page` at `src/app.rs:3501`, `render_schedule_rule` at `src/app.rs:3554`
- CPU usage: `render_cpu_usage_page` at `src/app.rs:3687`, `render_cpu_rule` at `src/app.rs:3747`
- Performance mode: `render_performance_mode_page` at `src/app.rs:5625`, `render_performance_mode_rules` at `src/app.rs:5699`

Acceptance check: each page file exposes one page render entry point and keeps page-local rule row helpers next to it.

### 5.4 Processor Controls Pages

Move these into `src/app/processor_controls/`:

- Section landing page: `home.rs`, rendered when `Page::ProcessorControls` is selected.
- Background CPU restriction: `render_background_cpu_restriction_page` at `src/app.rs:4878`, `render_background_cpu_exclusions` at `src/app.rs:5160`
- CPU limiter: `render_cpu_limiter_page` at `src/app.rs:5224`, `render_cpu_limiter_rules` at `src/app.rs:5302`
- CPU affinity: `render_affinity_page` at `src/app.rs:7052`, `render_affinity_rules` at `src/app.rs:7129`, core tile helpers at `src/app.rs:7247` and `src/app.rs:7339`
- Core parking and processor power: `render_core_parking_page` at `src/app.rs:8485`, `render_processor_power_card` at `src/app.rs:8491`

Acceptance check: affinity bitmask helpers can be unit-tested without rendering a full page.

### 5.5 Process Policies Pages

Move these into `src/app/process_policies/`:

- Section landing page: `home.rs`, rendered when `Page::ProcessPolicies` is selected.
- Efficiency Mode: `render_efficiency_page` at `src/app.rs:3975` and efficiency selectors through `src/app.rs:4428`
- App Suspension: `render_suspension_page` at `src/app.rs:4481`, `render_suspendable_apps` at `src/app.rs:4737`
- Watchdog: `render_watchdog_page` at `src/app.rs:5412`, `render_watchdog_rules` at `src/app.rs:5491`
- Foreground Responsiveness: `render_foreground_responsiveness_page` at `src/app.rs:5757` and Auto Balance helpers through `src/app.rs:6442`
- I/O Priority: `render_io_priority_page` at `src/app.rs:6546`, `render_io_priority_rules` at `src/app.rs:6621`
- SmartTrim: `render_smart_trim_page` at `src/app.rs:6740`, `render_smart_trim_exclusions` at `src/app.rs:7007`

Acceptance check: each process-policy page imports only its setting type, snapshot type, and shared widgets.

### 5.6 Log, Settings, and Advanced Pages

Move these into `src/app/app_pages/` and `src/app/advanced/`:

- Section landing pages: `src/app/app_pages/home.rs` and `src/app/advanced/home.rs`.
- Log: `src/app/action_log.rs`, with `render_action_log_page` at `src/app.rs:9026` plus action-log row/CSV helpers at `src/app.rs:11520`.
- Settings category pages: PowerLeaf Behaviour for general PowerLeaf toggles, Log detail, failure suppression, and settings import/export; Language and Appearance for theme/accent/palette/language.
- About: `render_about_page` at `src/app.rs:9010`
- Win32 Priority Separation: `render_win32_priority_separation_page` at `src/app.rs:8083` and registry helpers at `src/app.rs:10331`

Acceptance check: registry read/write helpers are no longer mixed with generic UI helpers.

## 6. Shared Helper Buckets

Move helpers only after page render functions are split. That keeps compiler errors localized.

| Bucket | Move From | Target |
| --- | --- | --- |
| Rule cards and setting cards | `src/app.rs:10665` through `src/app.rs:11488` | `src/app/widgets.rs` |
| Log labels and CSV | `src/app.rs:11520` through `src/app.rs:11745` | `src/app/action_log.rs` or `src/action_log.rs` if backend-neutral |
| Generic labels | `src/app.rs:11754` through `src/app.rs:11899` | `src/app/labels.rs` |
| Inputs, sliders, dropdowns | `src/app.rs:9456` through `src/app.rs:9894`, plus `src/app.rs:12866` through `src/app.rs:13573` | `src/app/inputs.rs` and `src/app/widgets.rs` |
| Theme and color | `src/app.rs:9904` through `src/app.rs:10209` | `src/app/theme.rs` |
| Process rule factories | `src/app.rs:13604` through `src/app.rs:14024` | section-local page files first; later consider `src/app/process_rule_forms.rs` |
| Affinity and processor labels | `src/app.rs:14452` through `src/app.rs:14585` | `src/app/processor_controls/affinity.rs` and `src/app/processor_controls/core_parking.rs` |
| File dialogs | `src/app.rs:14635` through `src/app.rs:14779` | `src/app/dialogs.rs` |

## 7. Runtime and Backend Organization

The backend is already partially organized. Use these rules before creating new features:

- Feature managers stay in feature modules, following the existing snapshot/manager pattern used by `src/ecoqos/mod.rs:107`, `src/cpu_limiter/mod.rs:59`, and `src/watchdog/mod.rs:61`.
- `src/automation.rs` should remain the scheduler/orchestrator, but feature-specific update details should stay in manager modules.
- Pure rule conversion belongs under `src/rules/`, following `src/rules/app_resource_adapter.rs:18`.
- Settings structs belong in `src/config/settings.rs` for now, but if the file continues to grow, split into `src/config/power.rs`, `src/config/process.rs`, `src/config/cpu.rs`, and `src/config/app.rs` while re-exporting from `src/config/mod.rs`.
- Windows API wrappers should live near the feature using them unless they are shared by three or more features. Shared wrappers should move to explicit modules such as `src/windows/process.rs`, `src/windows/registry.rs`, or `src/windows/power.rs`.

## 8. Naming Rules

- Page render entry points use `render_<page>_page`.
- Page-local helpers use the page prefix, for example `smart_trim_*`, `watchdog_*`, or `affinity_*`.
- Shared UI helpers use generic names only inside `widgets.rs`; outside that file, prefer explicit names.
- Backend manager methods should keep the existing pattern: `update`, `release_non_targets`, `clear_all`, `release_processes`, `apply_*`, and `restore_*`.
- Pure formatting helpers end with `_label`, `_text`, or `_message`.
- Constructors for config rules use `new_<feature>_rule`.

## 9. Migration Steps

1. Add empty `src/app/` submodules and wire them from `src/app.rs`.
2. Add section landing `Page` variants and extend `PageSection` with `landing_page`.
3. Update left navigation to render only section landing pages.
4. Add the reusable section-card component and one landing page, starting with Power Plan Automation.
5. Move top-level render shell functions into `src/app/render.rs`.
6. Move Dashboard into `src/app/dashboard.rs`.
7. Move one navigation section at a time: Power Plan Automation, Processor Controls, Process Policies, Log, Settings, Advanced.
8. Extract shared widgets after page modules compile.
9. Extract theme, dialogs, labels, and input helpers.
10. Move page-local tests next to the extracted page code.
11. Run formatting and the test/check suite after every section move.

## 10. Acceptance Criteria

- `src/app.rs` drops from about 562 KB to an entry point plus state/persistence glue.
- Every page listed in `src/ui/mod.rs:4` has a matching source file under `src/app/`.
- The left navigation shows section landing pages instead of every detailed page.
- Clicking a section landing page shows cards for its child pages.
- Overview shows CPU Usage, Enabled Rules, and two-column cards for every main section below the summary.
- Clicking a section card navigates to the existing detailed page.
- Clicking a child-page breadcrumb parent navigates back to the section landing page.
- Mouse side Back/Forward buttons move backward and forward through PowerLeaf page history.
- A child page highlights its parent section in the left navigation.
- A child page shows enough context to tell which section it belongs to, such as a breadcrumb or compact parent label.
- A developer can find a feature by first choosing one of the six sections from `src/ui/mod.rs:56`.
- No behavior changes are introduced during extraction.
- `cargo fmt` and `cargo check` pass after each migration step.
- Existing tests in `src/app.rs:14792` either remain passing or move next to their new helper modules.

## 11. Stop Rules

- Stop and split the migration if a single diff touches more than one navigation section plus shared widgets.
- Stop and add tests before moving a helper that changes rule creation, registry writes, process matching, process priority, affinity, suspension, or file dialog behavior.
- Stop and reassess if moving code requires making broad internal state public. Prefer `pub(super)` methods or small state accessors over exposing the whole app state.
