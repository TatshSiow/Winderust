# Agent Memory

## Tool Preferences

- Use the fff MCP tools for file search when available.
- Use the rtk tool for shell commands when available.
- Use Microsoft coreutils for Unix-style command-line utilities where applicable.

## Repo Shape

- Rust/GPUI Windows desktop app for power/process automation.
- Main UI: `src/ui/app.rs`.
- Background worker and status fan-out: `src/backend/automation.rs`.
- Process add/check helpers are near the bottom of `src/ui/app.rs`: `can_add_*`, `new_*_rule`, `new_process_exclusion_rule`.
- Prefer existing helpers over new abstractions.

## Current Decisions

- Repeated process failure suppression uses `ExecutionFailureTracker` in `src/rules/execution_failure.rs`; the threshold comes from `settings.advanced.execution_failure_suppression_threshold`.
- Auto-exclusion fallback is shared through `PendingAutoExclusions` in `src/backend/automation.rs`.
- On newly suppressed process failures, features emit `auto_excluded_processes`; `WinderustApp::apply_pending_auto_exclusions` persists them into each feature's existing exclusion/rule list.
- Rule-only fallbacks use disabled rules: CPU Affinity, CPU Limiter, App Suspension.
- Exclusion-list features append `ProcessExclusionRule`.
- Timer Resolution and Performance Mode do not use process failure suppression.

## User Constraints

- Do not cut animation/motion unless explicitly asked.
- Lucide/icondata note: using specific lucide icons should not be removed just because `icondata_core`/`icondata_lu` look broad; verify icon references before trimming.
- Legacy cleanup already removed old launch-priority related leftovers; avoid restoring deleted legacy files unless asked.

## Audit Notes

- Memory audit found no obvious unreleased handle loop.
- Handles use `WinHandle`/Drop.
- Action log is bounded, dashboard history is capped, process icon cache prunes stale paths.
- Mild watch item: static UI motion maps in `src/ui/app.rs` keyed by string IDs; fine if IDs stay bounded/fixed.

## Verification

- Default check: `cargo test`.
- Release build may need a longer timeout; `cargo build --release` took about 2m13s.
