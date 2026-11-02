# Winderust Development Guide

This is the working guide for code changes. Product boundaries live in [Project scope](project-scope.md).

## Project Basics

- Windows-only Rust desktop app.
- UI stack: Iced 0.14 with tiny-skia only and rust-i18n locales.
- Keep the software renderer; do not restore GPUI, WGPU, or vendored renderer code.
- Use the unmodified crates.io `iced_tiny_skia` renderer; do not add local renderer patches. Rendering regressions live in the application test suite (`src/ui/scrolling.rs`, `motion.rs`, `select.rs`, and page tests).
- Debug builds optimize dependencies and disable runtime checks in upstream `tiny-skia` pixel loops; application debug checks remain enabled.
- Settings format: TOML through `serde` and `toml`.
- Localization: `rust-i18n` with files in `locales/`.
- Windows integration: direct Win32 APIs through `windows` and `windows-sys`.
- Typed error boilerplate uses `thiserror`; keep conditional Windows status
  formatting explicit when a derive would require extra helpers.

After code or dependency changes, use these checks before handoff:

```powershell
git diff --check
cargo fmt -- --check
cargo clippy --locked --all-targets -- -D warnings -D unsafe-op-in-unsafe-fn
cargo test --locked
.\scripts\check_architecture_ownership.ps1
.\scripts\check_legacy_names.ps1
graphify update .
```

When reviewing or committing an existing index, also run
`git diff --cached --check`.

For release builds:

```powershell
.\scripts\build_release.cmd
```

If `target\release\winderust.exe` is locked because the app is running:

```powershell
.\scripts\build_release.cmd -TargetDir target-next
```

## Rendering and performance

- `src/ui/scrolling.rs` owns scroll construction, virtual-list buffering, table surfaces, and card repaint grouping. Route application scroll areas through it.
- `src/ui/select.rs` retains native input while avoiding option measurement for explicitly sized fields.
- `src/ui/motion.rs` and `animated_controls.rs` own small transitions. Do not add per-page animation timers or animate large lists.
- Renderer dev-dependencies support pixel comparisons and damage-region tests; they do not replace the production renderer.
- `[profile.dev.package."*"] opt-level = 2` optimizes dependencies for debug use. Release settings are separate; benchmark before changing them.
- Compare the reported interaction before and after a performance change. Scrolling benchmarks do not establish tab-switch or expansion performance; raster timings exclude layout and event handling.
- Documentation-only changes need link/path, whitespace, and legacy-name checks. Run the Rust checks above after code or dependency changes.

The optional render harness uses an in-memory configuration with automation disabled
and read-only queries:

```powershell
cargo run --locked --features render-smoke -- --render-smoke
```

Captures are written under `target/iced-smoke/`. Review relevant pages and states in
both locales/themes and at minimum window size. Captures establish rendering coverage;
they do not establish physical input, IME, DPI-transition, tray, accessibility, or
pixel-identical visual behavior. Exercise those interactions on Windows when relevant.
Keep production memory and performance claims tied to measured release builds.

All scrolling uses `scrolling::scrollable`, including dialogs and dropdowns. It keeps
native Iced input while clipping repaint sections; virtual lists use
`scrolling::buffered` and tables use `scrolling::table_surface`. Keep native scrollable
construction inside that module. Idle controls must not schedule continuous redraws.
Keep overlay scrollbars: `Scrollable::spacing` and `Scrollbar::spacing` embed the
scrollbar and change content width when overflow appears. Put spacing on content
rows and columns instead.

## Research before custom implementation

Prefer `fff` search and `rtk` when available; otherwise use the available shell tools.
Follow the Graphify rules in [AGENTS.md](../AGENTS.md).

- Before implementing a new capability, search existing repository code, the standard library, native Windows APIs, and installed dependencies.
- If those do not cover it, always check crates.io and the candidate crates' official documentation (docs.rs or upstream) before writing a custom implementation. Use current web references; do not rely only on recalled crate names or APIs.
- Prefer an existing solution that reasonably meets the requirement. Check maintenance, license compatibility, Windows/Iced version support, dependency weight, and performance before adding a crate.
- Implement custom code only when existing options do not reasonably fit; briefly record the concrete gap or tradeoff. Finding a crate does not automatically justify adding it.
- Reuse findings while they remain applicable. Routine edits to an established implementation do not require repeating the same crate search.

The native tray integration retains Watchdog balloon notifications and
click-to-restore. The evaluated [`tray-icon` API](https://docs.rs/tray-icon/0.26.1/tray_icon/struct.TrayIcon.html)
does not expose those operations, so adopting it would still require custom
Win32 integration. Keep the current tray owner until a replacement covers these
behaviors and registration failure handling before hiding the window.

## Routine Chores

- Dependabot version updates target `dev`; Iced and its renderer are grouped for review. GitHub reads Dependabot configuration from the default branch, so configuration changes must also reach `main` to take effect. Security updates retain GitHub's default-branch behavior.
- CI and releases run the shared architecture and legacy-name scripts before Rust checks. CI supports manual runs and cancels superseded runs for the same PR or branch.

For dependency PRs, review each update independently. Check the changed files,
release notes, and whether the version is already required by the UI dependencies
pinned in `Cargo.toml`; do not merge a major-version bump merely because
Dependabot opened it. Run the default checks above, confirm CI passes, then merge
or close the PR. Keep unrelated dependency updates in separate commits so a bad
bump is easy to identify and revert.

For other chores, keep the diff limited to the requested maintenance, reuse the
existing scripts and workflows, and run the same default checks before pushing.
Use `chore(deps): ...` for dependency-only commits and a direct imperative
subject for other maintenance.

Work directly on `dev`. Do not create feature branches or switch to another
branch unless the user explicitly requests it. Keep `main` for releasable code;
promote tested `dev` changes to `main` for an explicitly requested release.
CI runs on pushes to `main` and `dev`, and on every pull request. Release automation remains tag-only.

## Rust Engineering Practices

- Trace every caller before changing a shared helper, enum, setting, or error
  contract. Fix a defect once at the shared boundary when all affected callers
  route through it.
- Keep behavior-preserving refactors mechanical. Move one coherent page or
  helper family at a time, avoid mixing feature work into the split, and verify
  the resulting tree before claiming behavior is unchanged.
- Prefer existing project helpers, the standard library, and native Windows
  facilities before adding abstractions or dependencies.
- Use `Result` and typed errors when callers need to distinguish or report a
  failure. Avoid `unwrap` and `expect` on live process, filesystem, network,
  configuration, and Win32 paths.
- Treat unsupported internal enum variants or impossible UI wiring as invariant
  violations with a clear message; do not silently convert them into a no-op.

### Maintainability Review Workflow

Use this order for simplification and over-engineering reviews:

1. Query Graphify, then read the complete module and trace every caller and
   lifecycle path before proposing a change.
2. Ask whether each type, helper, field, scan, or abstraction needs to exist.
   Reuse an existing project helper, the standard library, or Win32 directly
   before adding code.
3. Separate genuine state ownership from namespacing. Keep stateful managers
   that own timers, resources, restoration, or shutdown; replace stateless
   structs with functions when that makes callers simpler.
4. Remove parallel representations, duplicate scans, unused status fields, and
   one-use wrappers when one source of truth is sufficient.
5. Prefer the smallest coherent deletion-first change. Do not perform renames,
   file moves, or abstraction swaps that merely reshuffle working code.
6. Review the exact diff for changed behavior and damaged tests, especially
   after mechanical edits. Preserve runtime safety, failure handling, cleanup,
   and process-state restoration.
7. Compile all targets, run the required checks and tests, then update Graphify.
   If no change clearly reduces maintenance cost, leave the code alone and say
   why.

## Releases

Follow the [release checklist](release-checklist.md) for versioning, Windows smoke tests,
CI, promotion to `main`, tagging, and publication.

## Architecture and navigation

Use the [architecture guide](architecture.md) for module ownership, the source map,
and navigation. Keep UI drafts outside `RuntimeCore`; mutations and restoration belong
to typed controllers and their narrow Windows adapters.

## Settings

- Runtime settings live in `Settings`.
- Default only intentionally optional current settings; do not add pre-release migration aliases.
- If a setting is edited through the UI, update the relevant page module and
  editor reset and message handling in `src/ui/app.rs`.
- TOML import/export uses native Windows file dialogs from `src/backend/file_dialog.rs`, invoked by the UI.

## Naming

- Start from the English UI label, then keep page variants, settings types/fields, feature modules, backend snapshots, tests, locale keys, scripts, and docs as close to that label as Rust naming permits.
- Current canonical examples: `AdaptiveEngine`, `BackgroundEfficiency`, `ByRunningApp`, `CpuLimiter`, `CpuSetsSoft`, `ProcessorAffinityHard`, and `DynamicPriorityBoost`.
- CPU Behaviour policies belong inside Adaptive Engine; keep their settings and implementation aligned with the visible policy names.
- Do not use retired product or feature identifiers. The naming scan below owns the rejected-name list. `Performance Mode` is valid only for the active state held by By Running App, not as a standalone feature or settings page.
- Native Windows vocabulary is allowed when it describes the implementation rather than the product surface, for example EcoQoS flags, affinity masks, CPU Sets, and `SetProcessPriorityBoost`.

Run this quick compatibility/naming check before handoff:

```powershell
.\scripts\check_legacy_names.ps1
```

## Runtime safety

Read the [runtime contracts](runtime-contracts.md) before changing process policy,
Windows mutations, failure handling, ownership, or restoration. These are product
safety requirements, including for manual Process List commands.

## UI Rules

- Keep controls compact and operational.
- Use existing Iced widgets and local `widgets.rs` helpers before adding new UI primitives.
- Keep plan mapping inside the relevant power-plan pages, not in a global settings page.
- Follow [Design](design.md) for current controls, panels, and motion.
- Keep `src/ui/app.rs` for composition, messages, native integration, and teardown.
  Put complete page editors and views in sibling modules and reuse `widgets.rs`
  for shared behavior. Do not introduce another UI framework.
- Multiple focused `impl WinderustApp` blocks are acceptable for the private UI
  module. Keep glob imports contained there; use explicit imports when a module
  gains independent ownership or its dependencies become unclear, not as
  mechanical churn.

## Windows APIs

Prefer native API calls already used in the repo. Do not add command spawning around `powercfg` unless the Win32 path cannot support the needed behavior.

When adding, removing, or changing a feature-defining or
compatibility-sensitive Win32, NT, or WDK boundary, update
[Windows API reference](windows-api-reference.md) in the same change. Link directly to official
Microsoft documentation when it exists and explicitly mark numeric information
classes or manually declared interfaces that are not stable public SDK
contracts.

- Keep the crate-level `clippy::undocumented_unsafe_blocks` and
  `unsafe_op_in_unsafe_fn` warnings enabled.
- Prefer private safe wrappers around Win32 FFI. Required `unsafe extern`
  callbacks must still wrap unsafe operations in explicit blocks.
- Put an immediately preceding `// SAFETY:` comment on each unsafe block that
  states the handle, pointer, buffer, lifetime, ownership, and ABI invariants
  relevant to that call.
- Capture `GetLastError` immediately after the failing Win32 call, before any
  other call can overwrite the thread-local error.
- Put owned Windows handles in the existing RAII wrappers and release each
  resource exactly once.
