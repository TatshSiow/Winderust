# Process Lasso Feature Parity Plan

## 1) Current project vs Process Lasso

This plan is for your current ProBalance-style mode scheduler concept.

## 2) Feature parity matrix

### A. Core already covered / in your current plan
- [x] ProBalance-like out-of-control process restraint
- [x] Temporary priority demotion with restore
- [x] Foreground/process exclusion model
- [x] Named process exclusions (wildcard-capable list)
- [x] Update interval + threshold-driven algorithm
- [x] TweakScheduler-style Win32PrioritySeparation handling
- [x] Mode concept (Adaptive/Balanced/Gaming/Background)

### B. Missing core Process Lasso features (high-value to add)
- [ ] Per-process default priorities and affinities (persistent rules)
- [ ] Process context actions (right-click-like command surface)
- [ ] UI process table with live CPU, priority, affinity, icons, log
- [ ] Persistent exclude classes by category (games/multimedia/anti-sleep/performance processes)
- [ ] Gaming mode special handling with power-plan induction
- [ ] CPU Limiter (actual throttling, not only priority changes)
- [ ] Process Watchdog (CPU/memory threshold actions: restart/terminate/change affinity)
- [ ] Instance count limits
- [ ] Keep-running processes (auto-restart behavior)
- [ ] Auto-terminate/disallowed process list
- [ ] Per-user vs global configuration handling
- [ ] Foreground/Thread boosting options
- [ ] Registry + service integration details for startup lifecycle
- [ ] Logging/visualization of responsiveness and restraint events

### C. Nice-to-have parity items (advanced/process ecosystem)
- [ ] SmartTrim / memory cleanup workflows
- [ ] Deep telemetry and exportable reports
- [ ] Forced mode enforcement of sticky priorities/affinities
- [ ] Multi-session orchestration and admin override workflow
- [ ] Enterprise-style policy import/export/versioning

## 3) Recommended implementation sequence

### Phase 1 (MVP parity)
1. Process rule model (per-process default priority/affinity)
2. Enhanced exclusion system (categories: game/multimedia/high-performance/anti-sleep)
3. Lightweight process UI surface for immediate actions and status
4. Durable logging/event history with restore reason and duration

### Phase 2 (value-add parity)
1. CPU Limiter
2. Watchdog actions (CPU/memory triggers)
3. Instance count limits
4. Keep-running + disallowed lists

### Phase 3 (advanced parity)
1. Foreground/thread boosting toggles
2. Power profile automation hooks
3. Forced mode and stricter rule enforcement
4. Startup/service reliability and rollback hardening

## 4) Practical “minimum realistic” target

To close the most important gap with the least complexity, deliver:
- Core restraints (already planned) +
- Per-process default rules +
- Exception categories (game/multimedia/anti-sleep/perf) +
- Basic logging/UI surface +
- Watchdog + CPU Limiter as optional plugin mode.

This gives you a product that is materially close on behavior for day-to-day responsiveness use, even if it is still smaller than full Process Lasso.

## 5) If you want tighter parity scoring
Use this maturity scale:
- **0–30%** = your current plan
- **30–50%** = Phase 1 complete
- **50–70%** = Phase 2 complete
- **70–85%** = Phase 3 partial
- **85%+** = near-platform-level Process Lasso parity
