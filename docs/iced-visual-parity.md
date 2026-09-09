# Iced visual parity audit ? 2026-09-09

Verdict: **not visually equivalent to the original GPUI UI**. The previous hierarchy fixes recovered some grouping, but did not establish page or interaction parity. Passing Rust tests and saving 86 screenshots proves neither visual correctness nor matching interaction behavior.

## Evidence and limits

Compared the user's 12 original screenshots in this turn, earlier Home/Process List/Power Plan Control/Log references, the current Iced working tree, and existing native captures under `target/iced-smoke/`. Checked original shared row composition at `cab3186:src/ui/app/shared/settings_components.rs` (fixed card-row height, centered children, right-aligned action area). This is a presentation audit, not a backend completeness certification.

Current captures include desktop Home, Process List and Adaptive Engine; other inspected captures are smaller light-theme windows. Their control types and grouping are comparable, but their absolute spacing and colors are not an exact screenshot comparison. Live metrics, preset values, enabled states, process counts, advanced-control visibility, scroll offsets and expanded navigation differ; these are not automatically defects. Hover, popup placement and click propagation were not re-tested in this audit.

## Shared gaps ? fix before more page-specific styling

1. **High: inconsistent shell and side panels.** Only Adaptive Engine has a separate desktop rail. Other status pages still consume the content frame's width; Advanced Power Plan Tuning owns another internal rail. The original uses a separate full-height dock, grouped status surfaces, and bottom actions/collapse control. Current shared status content is mostly text and separators. See `app.rs` view composition, `status_rail.rs`, and `advanced_power_plan_tuning.rs`.
2. **High: no consistent setting-row contract.** `widgets.rs` has a padded card and expandable header, but most child rows still choose their own height, action width, typography and control arrangement. Original rows share centered 58px geometry and a right-aligned action area. Standalone switches, child switches, selectors and numeric inputs remain inconsistent across pages.
3. **High: rule tables and policy tables.** Many original column-based editors became loose controls or independent cards. Adaptive Priority Control visibly overlaps labels/controls in the existing narrow capture. This is an actual layout defect, not merely different styling.
4. **Medium: contextual help and unsaved state.** Help remains expanded prose on several pages, while the original uses per-setting info affordances. Save/Cancel are inline at the content bottom instead of the original persistent lower-right unsaved-settings notice. Preserve current save/discard semantics while restoring presentation.
5. **Medium: navigation and heading treatment.** Current navigation has different row density, icon colors, count presentation, selected surface and no original cyan selection marker. Expanded children are a current behavior difference; screenshot expansion state alone does not prove a defect. Breadcrumbs exist now, but wrapping, muted ancestors and action alignment still differ.

## Page-by-page findings

| Reference | Current parity | Remaining difference and evidence |
|---|---|---|
| Home | Partial | Main chart/shortcut structure recovered. Chart legends, line/grid styling, metric weight and enabled-feature badge treatment differ. Vertical placement differs. Advanced Controls changes shortcut count when enabled; do not remove it merely to match a screenshot with that setting off. `home.rs`; `81-Home.png`. |
| Process List | Partial | 52px rows and internal separators recovered. Toolbar/search proportions, permanently exposed column toggles, table-header contrast, group/PID formatting, status icons/colors and user labels differ. The existing desktop capture contains only one visible group and cannot certify a full populated table. Re-capture with controlled search/scroll state. `process_list.rs`; `82-ProcessList.png`. |
| Winderust Features | Close structure, partial presentation | Single 58px navigation cards are present. Original Enabled pill / Disabled text became plain On/Off; icon-label spacing and sidebar selection differ. `app.rs::child_pages`. |
| Adaptive Engine ? CPU Behaviour | Partial | Groups, steppers, help and a separate rail recovered. Units sit outside the input; stepper color/geometry, header help, switch-label order, tab centering and preset panel remain different. Original preset panel has quiet rows, info actions, built-in/custom headings, bottom Add preset and bottom collapse. `adaptive_engine.rs`; `85-AdaptiveEngine.png`. |
| Adaptive Engine ? Priority Control | Poor | Original is one aligned seven-row policy table with Active, Control, Focus, Visible and Background columns. Current row cards contain stacked detection switches and other controls; narrow capture has overlaps. Preserve existing settings but give additional controls an appropriate secondary location rather than deleting them to imitate an older screenshot. `adaptive_engine.rs`; `74-AdaptiveEngine.png`. |
| By Activity | Poor | Keyboard/mouse/controller use left checkboxes instead of right On/Off switches; numeric controls lack minus/plus; plan controls and row sizes differ; extra explanatory prose remains inline; side panel is inside the frame. `by_activity.rs:90` onward. |
| Advanced Power Plan Tuning | Partial hierarchy, poor presentation | A/C and Battery header selectors recovered. Sliders lack steppers, child rows are compact, action alignment differs, preset rail is internal with prominent buttons instead of quiet rows and info actions, and bottom controls differ. `advanced_power_plan_tuning.rs:288` onward; `12-AdvancedPowerPlanTuning.png`. |
| Background Efficiency | Partial hierarchy, poor presentation | Master/Foreground/Visible groups recovered. Child Efficiency Mode is a checkbox instead of the original select; aggressiveness help is expanded; header help/On-Off labels and original rule-table empty state are absent. Current disabled selects can become plain text, also changing geometry. `background_efficiency.rs:114` onward; `04-BackgroundEfficiency.png`. |
| Core Limiter screenshot | Baseline unresolved | No Core Limiter page or matching protect-focus/protect-visible symbols were found in the current tree or the listed `cab3186` page paths. Current CPU Limiter is a CPU-time control and must not be assumed equivalent. Treat this as historical-version mapping work, not evidence that a feature should be renamed or recreated. |
| App Suspension | Partial hierarchy, poor presentation | Thaw/audio/network ownership is recovered. Master switch is left-aligned; numeric rows lack steppers; help and On/Off labels are missing; suspendable-app editor uses cards instead of the reference table; rail is internal. `app_suspension.rs:174`, `:403`; `31-AppSuspension.png`. |
| Language and Appearance | Poor | Order differs (Theme/Animation before Accent), swatches are tiny dots instead of square tiles, custom-color presentation differs, and feature-count/card-status settings use checkboxes instead of right switches. `settings_pages.rs:184` onward; `27-LanguageAndAppearance.png`. |
| About | Poor | Original has one identity card with horizontal logo/title and one Updates card. Current identity content is an unwrapped vertical stack; update controls are scattered, with different button emphasis and version alignment. `settings_pages.rs`, `Page::About`; `29-About.png`. |
| Power Plan Control and Log (earlier references) | Partial | Landing cards/table composition recovered previously; shared toggle, badge, toolbar, header and spacing gaps remain. Do not treat those prior fixes as a finished visual pass. `app.rs`, `action_log.rs`. |

The six separate Priority Control pages share `priority_control.rs`: group ownership was repaired, but default selectors are not consistently composed as labeled right-aligned rows; detection headers lack the original help/On-Off presentation. This propagates across Process Priority, Thread Priority, Dynamic Priority Boost, I/O Priority, GPU Priority and Memory Priority.

## Intentional differences to retain

- Native Windows title bar and Segoe UI were explicitly chosen. Do not restore the GPUI custom title bar or former font as part of parity work.
- Cards remain borderless per the user's instruction, even where a reference has an outer border. Internal table separators can remain.
- Keep current product names, safety behavior, current settings and runtime semantics. Historical labels and runtime values in screenshots are not a migration specification.
- Use Iced widgets and existing dependencies. The gap is primarily composition and consistent styling, not lack of a GUI framework API.

## Completion criteria for the correction pass

Restore shared row/action/help/switch/stepper patterns and one dock arrangement first; then repair rule tables, page order, presets and About grouping. Check desktop references with the same theme, language, scale, navigation state and representative populated/empty data. Check 900px windows separately for clipping and overlap. Exercise collapsed/expanded groups, selectors, steppers, presets, save/discard and panel controls. Review each screenshot rather than treating capture count as success. Pages without a matched reference or validated render remain explicitly unverified.

## Correction pass implemented

The audit above records the pre-correction state. The current pass adds shared setting rows, On/Off switches, help labels and bounded steppers; normalizes existing switches across feature pages; separates shared status and tuning preset rails from the content frame; restores status surfaces and the lower-right unsaved notice; repairs Appearance order/swatches and About grouping; restores horizontal rule editors in Background Efficiency and App Suspension; moves extra Adaptive policy options into an Advanced disclosure; and reduces Process List toolbar clutter. Desktop and narrow render cases are retained in `smoke.rs`, with representative feature pages added to the desktop pass.

Remaining verification limits: the historical Core Limiter screenshot is still not mapped to a current feature; custom title-bar/font differences remain intentional. These changes restore common patterns, not an assertion of pixel-identical rendering or complete live mouse-interaction parity. Native capture checks do not replace the latter.
