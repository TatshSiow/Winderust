# PowerLeaf Project Scope

This is the product-scope and future-goals doc. Development mechanics live in `DEVELOPMENT_GUIDE.md`.

## Goal

PowerLeaf is a Windows power and process-control app focused on power efficiency, foreground responsiveness, and conservative automation.

Process Lasso is a useful comparison point, not the target to clone. Prefer small, explainable controls over a full process-management suite.

## Current Scope

- Power-plan automation by foreground app, running app, CPU load, idle/activity, and schedule.
- Processor power-plan tuning for core parking and processor performance values.
- Foreground responsiveness / ProBalance-style background restraint.
- CPU limiter, background CPU restriction, and CPU affinity/core steering.
- Efficiency Mode / EcoQoS.
- App Suspension for explicit opt-in apps.
- IO, GPU, memory, and launch priority policies.
- Smart Trim and memory-priority controls.
- Watchdog rules for terminate/restart behavior.
- Timer resolution rules tied to foreground apps.
- Action Log and CSV export.
- Process List as the main per-process policy surface.

## Process Lasso Parity

Covered enough for day-to-day responsiveness:

- ProBalance-like restraint.
- Foreground exclusions.
- Per-process policy rules.
- CPU limiter.
- Watchdog basics.
- Priority and affinity controls.
- Action history.
- Process table/policy surface.

Not a goal by default:

- Full Process Lasso feature parity.
- Service/admin orchestration.
- Enterprise policy management.
- Forced global modes that fight user intent.
- Automatic High/Realtime priority.
- Broad "suspend all background apps" behavior.

## Future Goals

Add only when a real workflow needs them:

- Instance limits, keep-running, and disallowed-process rules if Watchdog users need more actions.
- Process-list context actions if repeated workflows are awkward without them.
- Gaming/work/battery presets after the current settings model stabilizes.
- Better telemetry/export if Action Log is not enough.
- Startup/service hardening if always-on background operation becomes a requirement.
- Category-based exclusions only when users can name categories that beat simple process rules.

## Product Rules

- Runtime changes must restore cleanly when possible.
- Dangerous controls need explicit user intent and conservative defaults.
- If a feature can be a rule on an existing page, do that before adding a new top-level page.
- If the value is only for developers, put it in `DEVELOPMENT_GUIDE.md`, not here.

