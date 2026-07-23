# Agent Memory

## Tool Preferences

- Use the fff MCP tools for file search when available.
- Use the rtk tool for shell commands when available.
- Use Microsoft coreutils for Unix-style command-line utilities where applicable.

## Repo Shape

- Rust/GPUI Windows desktop app for power/process automation.
- Shared UI state and construction: `src/ui/app.rs`; operational `WinderustApp` method groups: `src/ui/app/*.rs`.
- Page and shell renderers: `src/ui/app/pages/`; reusable UI helpers: `src/ui/app/shared/`.
- Background worker and status fan-out: `src/backend/automation.rs`.
- Process add/check helpers: `src/ui/app/shared/process_policies.rs` (`can_add_*`,
  `new_*_rule`, and `new_process_exclusion_rule`).
- Prefer existing helpers over new abstractions.

## Current Decisions

- UI wording is the naming source of truth. Current feature names include Adaptive Engine, Background Efficiency, Memory Trim, By Foreground, By Running App, By CPU Load, By Activity, By Time, Core Limiter, Core Steering, and Dynamic Priority Boost.
- Use native mechanism names only at Windows boundaries: EcoQoS, affinity masks, CPU Sets, and exact Win32 function names remain technical terms.
- Winderust is public pre-release software under GPL-3.0-only, Copyright (C) 2026 Tatsh Siow. Settings use only the current schema; do not add serde aliases, migration code, old brand paths, or compatibility-only fallbacks.
- Keep personal tooling local-only: .codex/, .agents/skills/, and graphify-out/ must remain ignored and excluded from release artifacts.
- Settings live beside the executable. Action Log entries stay in memory and
  export to a user-selected CSV path. Do not add an AppData fallback or
  migration unless the user explicitly requests it.
- Update checks support Stable and Pre-release channels. Automatic checks are optional; manual checks remain available on About.
- Power-plan selections belong to the page or rule that exposes them. By Activity owns Idle/Active plans; other automation rules own `power_plan_guid`. There is no global `Settings::power_plans` fallback.
- The global pause for power-plan switching on A/C belongs on the Power Plan Control landing page, not Winderust Behaviour.
- Managed adaptive-plan recovery recognizes only the current `Winderust Adaptive` name and description.
- Repeated process failure suppression uses `ExecutionFailureTracker` in `src/rules/execution_failure.rs`; the threshold comes from `settings.advanced.execution_failure_suppression_threshold`.
- Auto-exclusion fallback is shared through `PendingAutoExclusions` in `src/backend/automation.rs`.
- On newly suppressed process failures, features emit `auto_excluded_processes`; `WinderustApp::apply_pending_auto_exclusions` persists them into each feature's existing exclusion/rule list.
- Rule-only fallbacks use disabled rules: Core Steering, Core Limiter, App Suspension.
- Exclusion-list features append `ProcessExclusionRule`.
- Timer Resolution does not use process failure suppression.

## User Constraints

- Do not cut animation/motion unless explicitly asked.
- Lucide/icondata note: using specific lucide icons should not be removed just because `icondata_core`/`icondata_lu` look broad; verify icon references before trimming.
- Do not restore removed legacy identifiers or files unless the user explicitly asks for compatibility work.
