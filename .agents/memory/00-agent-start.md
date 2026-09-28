# Start Here

`AGENTS.md` owns the product contract, safety requirements, and required checks.
Read only the task-specific memory files linked from [README.md](README.md).

## Current stack

- Windows-only Rust application using Iced 0.14 and unmodified upstream tiny-skia.
- Do not restore GPUI, WGPU, or a vendored renderer. Prefer low memory use and measured performance.
- Small control transitions use `iced_anim`. Animation supports On, Off, and Follow system.
- Reuse the shared controls, select, motion, and scrolling modules; see [15-design-spec.md](15-design-spec.md).
- Verify icon consumers before trimming `icondata_core` or `icondata_lu`.

## Boundaries

- `src/ui/app.rs` owns UI composition, messages, subscriptions, and native-window lifecycle.
- Page editors and shared controls live in `src/ui/`; UI drafts stay outside `RuntimeCore`.
- `src/backend/automation.rs` owns the runtime entry point; controllers own mutations and restoration.
- Read [25-runtime-contracts.md](25-runtime-contracts.md) before changing runtime policy or ownership.
- Preserve exact-process identity checks, protection rules, failure handling, and restoration.

## Tools

- Before custom implementation, check existing code and dependencies, then crates.io and official crate documentation; follow [the research rule](10-development-guide.md#research-before-custom-implementation).

- Prefer fff search and rtk when available; otherwise use the available shell tools.
- Follow the Graphify rules in `AGENTS.md`.
