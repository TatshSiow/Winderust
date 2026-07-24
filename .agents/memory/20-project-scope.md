# Winderust Project Scope

This is the product-scope and future-goals doc. Development mechanics live in `10-development-guide.md`.

## Goal

Winderust is a Windows power and process-control app focused on power efficiency, foreground responsiveness, and conservative automation.

Process Lasso is a useful comparison point, not the target to clone. Prefer small, explainable controls over a full process-management suite.

## Current Scope

- Power Plan Control through By Foreground, By Running App, By CPU Load, By
  Activity, and By Time.
- Advanced Power Plan Tuning for core parking and processor performance values.
- Adaptive Engine CPU scheduling for foreground responsiveness and background restraint.
- Background Efficiency, implemented with Windows EcoQoS at the operating-system
  boundary.
- CPU Control through Core Limiter, Background CPU Restriction, and Core
  Steering.
- App Suspension for explicit opt-in apps.
- Priority Control through Process Priority, Thread Priority, Dynamic Priority
  Boost, IO Priority, GPU Priority, and Memory Priority.
- Memory Trim.
- Timer Resolution rules tied to foreground apps.
- Action Log and CSV export.
- Process List as the main per-process policy surface.

## Process Lasso Parity

Covered enough for day-to-day responsiveness:

- ProBalance-like restraint.
- Foreground exclusions.
- Per-process policy rules.
- Core Limiter.
- Priority and Core Steering controls.
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

- Process-list context actions if repeated workflows are awkward without them.
- Gaming/work/battery presets after the current settings model stabilizes.
- Better telemetry/export if Action Log is not enough.
- Startup/service hardening if always-on background operation becomes a requirement.
- Category-based exclusions only when users can name categories that beat simple process rules.

## Product Rules

- Runtime changes must restore cleanly when possible.
- Dangerous controls need explicit user intent and conservative defaults.
- If a feature can be a rule on an existing page, do that before adding a new top-level page.
- If the value is only for developers, put it in `10-development-guide.md`, not here.

