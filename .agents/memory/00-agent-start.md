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

- UI wording is the naming source of truth. Current feature names include Adaptive Engine, Background Efficiency, Memory Trim, By Foreground, By Running App, By CPU Load, By Activity, By Time, Core Limiter, Core Steering, and Dynamic Priority Boost.
- Use native mechanism names only at Windows boundaries: EcoQoS, affinity masks, CPU Sets, and exact Win32 function names remain technical terms.
- Winderust is public pre-release software under GPL-3.0-only, Copyright (C) 2026 Tatsh Siow. Settings use only the current schema; do not add serde aliases, migration code, old brand paths, or compatibility-only fallbacks.
- Keep personal tooling local-only: .codex/, .agents/skills/, and graphify-out/ must remain ignored and excluded from release artifacts.
- Power-plan selections belong to the page or rule that exposes them. By Activity owns Idle/Active plans; other automation rules own `power_plan_guid`. There is no global `Settings::power_plans` fallback.
- Managed adaptive-plan recovery recognizes only the current `Winderust Adaptive` name and description.
- Repeated process failure suppression uses `ExecutionFailureTracker` in `src/rules/execution_failure.rs`; the threshold comes from `settings.advanced.execution_failure_suppression_threshold`.
- Auto-exclusion fallback is shared through `PendingAutoExclusions` in `src/backend/automation.rs`.
- On newly suppressed process failures, features emit `auto_excluded_processes`; `WinderustApp::apply_pending_auto_exclusions` persists them into each feature's existing exclusion/rule list.
- Rule-only fallbacks use disabled rules: Core Steering, Core Limiter, App Suspension.
- Exclusion-list features append `ProcessExclusionRule`.
- Timer Resolution and Performance Mode do not use process failure suppression.

## User Constraints

- Do not cut animation/motion unless explicitly asked.
- Lucide/icondata note: using specific lucide icons should not be removed just because `icondata_core`/`icondata_lu` look broad; verify icon references before trimming.
- Do not restore removed legacy identifiers or files unless the user explicitly asks for compatibility work.

## Audit Notes

- Memory audit found no obvious unreleased handle loop.
- Handles use `WinHandle`/Drop.
- Action log is bounded, dashboard history is capped, process icon cache prunes stale paths.
- Mild watch item: static UI motion maps in `src/ui/app.rs` keyed by string IDs; fine if IDs stay bounded/fixed.

## Verification

- Default checks: `cargo fmt -- --check`, `cargo clippy --locked --all-targets -- -D warnings`, and `cargo test --locked`.
- Release build: `.\scripts\build_release.cmd`. It discovers fxc.exe from the installed Windows SDK.
