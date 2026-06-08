# Core

PowerLeaf is a Windows-only Rust desktop app focused on power/efficiency controls and process behavior automation.

Core architecture references:
- `src/app.rs` owns app root state, page navigation, and settings persistence wiring.
- `src/config/settings.rs` defines settings model and backward-compatible fields.
- `src/config/storage.rs` owns TOML/INI load/save and migration paths.
- `src/automation.rs` coordinates background automation loop and safety checks.

Feature ownership boundaries:
- Power plan control and tuning: `src/power/`, `src/ui/power_plan_page.rs`.
- Process-affinity steering: `src/affinity/`.
- EcoQoS: `src/ecoqos/`.
- App suspension: `src/suspension/`.
- Foreground-aware behavior: `src/foreground/` and `src/ui/rules_page.rs`.

Decision ordering and runtime behavior are documented under `mem:core` references.

Further references:
- `mem:tech_stack` for build/runtime stack.
- `mem:conventions` for durable code and behavior invariants.
- `mem:suggested_commands` for commands actually used.
- `mem:task_completion` for done criteria.