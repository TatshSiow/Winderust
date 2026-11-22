# Winderust Project Scope

This is the product-scope and future-goals doc. Development mechanics live in [Development guide](development.md).

## Goal

Winderust is a Windows power and process-control app focused on power efficiency, foreground responsiveness, and conservative automation.

Process Lasso is a useful comparison point, not the target to clone. Prefer small, explainable controls over a full process-management suite.

## Current Scope

- Power Plan Control through By Focus, By Running App, By CPU Load, By
  Activity, and By Time.
- Advanced Power Plan Tuning for core parking, processor performance and boost,
  and long- and short-running thread scheduling policies.
- Adaptive Engine CPU scheduling for foreground responsiveness and background restraint.
- Background Efficiency, implemented with Windows EcoQoS at the operating-system
  boundary.
- CPU Control through CPU Limiter, CPU Sets (Soft), and Processor Affinity (Hard).
- App Suspension for explicit opt-in apps.
- Priority Control through Process Priority, Thread Priority, Dynamic Priority
  Boost, I/O Priority, GPU Priority, and Memory Priority.
- Memory Trim.
- Watchdog with sustained CPU and memory thresholds and Windows notifications by
  path or process instance. Combined by path rules with an exact executable path
  can opt in to Terminate or Restart.
- Timer Resolution rules tied to foreground apps.
- Sleep Prevention rules for running apps, with separate display requests.
- Keep Running rules for opt-in automatic relaunch after a manually started app exits.
- Instance Limits for sampled per-path process counts and excess notifications in
  the current user's Windows session; it does not prevent or stop app instances.
- Instance Balancing for separate whole-core CPU Sets (Soft) preferences across
  eligible instances of the same executable in the current Windows user session.
- Action Log and CSV export.
- Win32 Priority Separation for Windows scheduler time slices and foreground
  boost, with backup and restore.
- Interrupt Timing for read-only DPC and ISR capture with JSON report export.
- Process List as the main per-process policy surface.

## Process Lasso Parity

Covered enough for day-to-day responsiveness:

- ProBalance-like restraint.
- Foreground exclusions.
- Per-process policy rules.
- Wall-clock CPU Limiter duty cycling.
- Priority and per-app CPU allocation controls.
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

Start with a named user workflow and the smallest change that fits an existing
page or controller. Define who owns the effect, how the UI reports unavailable
evidence, and how to verify the result and restoration. Verify read-only
diagnostics before considering persistent machine-wide controls.

Add only when a real workflow needs them:

- Better telemetry/export if Action Log is not enough.
- Startup/service hardening if always-on background operation becomes a requirement.
- Category-based exclusions only when users can name categories that beat simple process rules.

## Product Rules

- A reversible runtime change must capture and restore its original state or not be applied.
- Dangerous controls need explicit user intent and conservative defaults.
- If a feature can be a rule on an existing page, do that before adding a new top-level page.
- If the value is only for developers, put it in [Development guide](development.md), not here.
