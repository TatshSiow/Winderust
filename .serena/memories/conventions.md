# Conventions

- Keep feature ownership inside existing page modules; do not reintroduce removed global pages/controls.
- Power plan mapping controls belong to each Power Plan Controls page (Action, Time, CPU); no standalone global mapping page.
- Automation decisions follow existing `src/rules/decision_engine.rs` order and compatibility behavior.
- EcoQoS/Core Steering/App Suspension: enablement and target changes generally apply only after Save; disablement is immediate.
- Foreground/app candidate lists should use established dropdown/picklist patterns; avoid broad or eager targeting in background automation.
- File changes should prioritize existing symbols and modules over introducing abstractions.
- Prefer durable invariants over task-local notes; avoid one-off implementation details and avoid speculative design changes.