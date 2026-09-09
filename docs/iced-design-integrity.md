# Iced design integrity review

The hierarchy corrections below are implemented in the Iced working tree. The original audit is retained as the comparison record; its “Current Iced difference” column describes the pre-correction state.

Baseline: `cab3186`, the GPUI application before the Iced port. Comparison target: the current working tree. This is a source-level presentation review, not a new assessment of backend feature parity. Existing uncommitted changes were preserved.

## Original audit findings

| Area | Original GPUI structure | Current Iced difference | Required correction |
|---|---|---|---|
| Adaptive Engine | Main preset selector; CPU Behaviour, Processor Power, Priority Control, and live-only Custom Rules tabs. CPU Behaviour contains CPU Pressure Restraint and Limit Background Processors groups. Priority Control uses a policy table. | A long sequence of setting cards mixes these categories. Status/Presets rail tabs exist, but they are not the original tuning tabs. | Restore the tuning tabs, their membership and ordering, the two CPU Behaviour groups, and the priority table. Preserve separate live/preset editing and live-only rules. |
| Six Priority Control pages | Expandable master card contains the background default and applicable preserve-background option. Foreground Detection and Visible Window Detection are separate expandable cards, each with its switch in the header and its default/preserve controls inside. | Master switch is a standalone card. Focus, Visible Window, and Background are three peer groups. Detection switches are inside the revealed body. | Restore master/background ownership, original group titles, header switches, and original ordering across all six pages. |
| Background Efficiency | Expandable master card contains background Efficiency Mode and aggressiveness. Foreground Detection and Visible Window Detection have header switches and an Efficiency Mode child row. | Master switch and aggressiveness are separate cards; Background is a separate third tier group. Detection switches are hidden inside the other groups. | Move background/default/aggressiveness into the master card; put detection switches back in their group headers. |
| CPU Limiter | Master header combines enable switch and expansion control. All three default CPU-time sliders belong inside that master card. Rules follow outside it. | Master switch and expandable Default card are separate siblings. | Combine them into the original master group; keep warnings visible and rules outside. |
| Memory Trim | Master switch followed by Thresholds, When to Trim, and Safety expandable groups. Thresholds owns two numeric fields; When to Trim owns idle time; Safety owns exclusions and their picker. | Three standalone numeric cards and a separate exclusion area; no group-collapse state or messages. | Restore those three groups and their exact field membership. |
| App Suspension | Temporary Thaw, Audio Detection, and Network Detection groups contain plain setting rows on a shared surface. Their enable controls remain in the headers. | Parent group membership mostly survives, but `delay_row` adds another complete bordered card inside each group. | Keep the three parent groups; render their child rows without a second card surface. Retain the standalone background-delay card. |
| Advanced Power Plan Tuning | A/C and Battery cards have preset selectors in their headers; their numeric and boost controls are the expandable bodies. | The preset selector occupies a separate row between the disclosure header and its body. | Restore title, selector, and chevron to one header while retaining both source groups. |
| By Foreground / By Running App | Enabled card followed by a Rules section, picker, and rule rows. | A new expandable Custom group wraps the entire rules section. This grouping was not in the GPUI pages. | Remove the invented parent disclosure; retain actual rule controls and their card surfaces. |

The six Priority Control pages are Process Priority, Thread Priority, Dynamic Priority Boost, I/O Priority, GPU Priority, and Memory Priority. Their shared Iced editor propagates the same hierarchy mismatch to all six.

## Shared component mismatch

GPUI distinguished `setting_action_card` from `setting_group_action_row_element`. A standalone row owned its surface; a row inside a group did not add another surface. The group header also accepted an independent action, usually a switch or preset selector.

Iced currently has a generic `settings_card(content)` and `disclosure(label, expanded, message)`. These cover appearance but do not encode that header/action/body relationship. Applying the standalone wrapper indiscriminately creates extra nesting; adding a disclosure around existing content does not recover its original parentage.

The correction needs a small shared group composition with a header action and a separate plain child-row composition. It does not require custom widget internals, GPUI dependencies, or vendor changes.

## Evidence map

Old paths below are relative to `src/ui/app/` at `cab3186`; current paths are relative to `src/ui/iced/`.

| Area | GPUI source | Iced source |
|---|---|---|
| Shared structure | `shared/settings_components.rs:120` (group), `:353` (child row), `:659` (standalone card) | `widgets.rs:5`, `widgets.rs:14` |
| Adaptive Engine | `pages/adaptive_engine_page.rs:796`, `:1446`, `:1837` | `adaptive_engine.rs:268` onward |
| Priority Control | `pages/process_priority_page.rs:19` onward; corresponding thread, dynamic-boost, I/O, GPU, and memory pages | `priority_control.rs:548` onward |
| Background Efficiency | `pages/background_efficiency_page.rs:42`, `:219` | `background_efficiency.rs:114` onward |
| CPU Limiter | `pages/cpu_limiter_page.rs:51` | `cpu_limiter.rs:214` onward |
| Memory Trim | `pages/memory_trim_page.rs:19`, `:56`, `:79` | `memory_trim.rs:93` onward |
| App Suspension | `pages/app_suspension_page.rs:49`, `:122`, `:164` | `app_suspension.rs:174`, `:410` |
| Advanced Power Plan Tuning | `pages/advanced_power_plan_tuning_page.rs:729` onward | `advanced_power_plan_tuning.rs:365` onward |
| Process power plans | `pages/by_foreground_page.rs`, `pages/by_running_app_page.rs` | `process_power_plans.rs:124` onward |

## Integrity requirements for the correction

1. Map each original group to its header action, child settings, and position before changing layouts.
2. Preserve those parent-child relationships. Do not invent replacement tier groups or move settings between groups for visual convenience.
3. Keep header switches/selectors visible when collapsed. Expanding must reveal only that group's children. Operating a header control must not accidentally toggle expansion.
4. Keep child rows flat inside their parent surface. Standalone cards and removable rule cards retain their own boundaries.
5. Check the live and preset variants separately, including read-only presets and live-only custom rules.
6. Verify expanded and collapsed states, English and Traditional Chinese, both themes, and minimum window size. Exercise controls in populated rule lists as well as empty states.
7. Preserve current Windows framing, font rendering, safe runtime behavior, hover feedback, and animation preferences.

The prior passing tests and page renders establish compilation and rendering coverage. They do not establish hierarchy parity: the render harness does not compare the control tree with GPUI, and its ordinary page sweep cannot expose missing tabs or groups that were never constructed.

## Corrections implemented

- `widgets::setting_group` composes one card with one clickable header and a passive chevron, a persistent header action, and an animated child body. Native Iced delivers events to header controls first and stops propagation when they capture the event, so changing a switch or preset does not collapse the group.
- Priority Control and Background Efficiency now render master/background, Foreground Detection, and Visible Window Detection in that order. Their defaults and preserve options belong to the corresponding parent; aggressiveness belongs to Background Efficiency’s master card.
- CPU Limiter’s enable switch and default limits share one expandable master card. Warnings and rules remain outside.
- Memory Trim restores Thresholds (load and working set), When to Trim (idle time), and Safety (exclusion picker and rules). Its three collapse states are independent.
- App Suspension uses plain delay rows inside its three groups, retaining a card for the standalone background delay. Advanced Power Plan Tuning keeps each source’s preset picker in the header.
- By Foreground and By Running App have a Rules section without the invented Custom disclosure; obsolete collapse state/messages were removed.
- Adaptive Engine restores the main preset selector and four live tuning tabs, with independent three-tab preset navigation and collapse state. CPU Behaviour owns the two original groups; Processor Power uses section headings and standalone setting cards; Priority Control uses control/foreground/visible/background columns. The policy table uses the full content width, with status/preset tools below it so all three policy columns remain visible at minimum window size. Custom Rules remains live-only, and read-only presets retain the existing mutation guard.

## Verification

Two state tests cover live/preset tab isolation, rejection of preset Custom Rules navigation, independent collapse state, and unchanged settings during navigation. Existing editing and read-only tests remain in place.

The native render harness now covers 81 page/state combinations, including both locales/themes, minimum-size windows, Adaptive Engine tuning tabs, editable and read-only preset views, collapsed groups, and populated rule lists. These captures verify rendering; they do not constitute native mouse interaction testing or pixel-for-pixel GPUI equivalence. Native computer-use automation was unavailable in this session.

