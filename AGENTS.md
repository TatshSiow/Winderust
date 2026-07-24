# Agent Start

Read `.agents/memory/README.md` first.

## Current Product Contract

- Product: **Winderust - Windows Performance & Power Manager**.
- Treat visible UI names as the canonical names for feature modules, settings, page variants, helpers, tests, locale keys, scripts, and documentation.
- Keep Windows terms such as EcoQoS, affinity masks, CPU Sets, and Win32 API names only where code directly describes the operating-system mechanism.
- Winderust is public pre-release software licensed GPL-3.0-only, Copyright (C) 2026 Tatsh Siow. Do not add legacy config aliases, schema migrations, old product-name paths, cross-brand recovery, or hidden compatibility fallbacks unless the user explicitly requests them.
- Personal tooling is local-only. Keep .codex/, .agents/skills/, and graphify-out/ ignored and out of release artifacts.
- Runtime data is portable: keep settings beside the executable; keep Action
  Log entries in memory and export them only to a user-selected CSV path. Do not
  restore an AppData fallback or migration unless explicitly requested.
- Power-plan ownership is local: By Activity owns its Idle/Active selections; By Foreground, By Running App, By CPU Load, and By Time rules each own their selected plan. There is no global plan fallback.
- The global A/C scheduler pause belongs on the Power Plan Control landing page.
- Only recover temporary managed plans whose name and description identify the current `Winderust Adaptive` plan.

## Change Checklist

- Trace all callers before renaming or removing a setting.
- Keep `Page`, navigation sections, render dispatch, locale keys, settings fields, backend status, scripts, and docs aligned with the same UI term.
- Preserve runtime safety behavior: process-state restoration, protected-process filtering, conservative defaults, and failure handling are not legacy compatibility.
- Prefer typed errors when callers must distinguish failures; do not use `unwrap` or `expect` on runtime Win32/process paths, and do not silently ignore impossible internal wiring.
- Keep every unsafe operation in the smallest practical explicit `unsafe` block, including inside `unsafe fn`, with an immediately preceding `SAFETY:` rationale.
- When a feature-defining or compatibility-sensitive Win32, NT, or WDK
  boundary changes, update `.agents/memory/30-reference-library.md` with the
  implementation path, direct official reference when available, and any
  undocumented contract.
- Before handoff, run `git diff --check`, `cargo fmt -- --check`, `cargo clippy --locked --all-targets -- -D warnings -D unsafe-op-in-unsafe-fn`, `cargo test --locked`, the legacy scan from the development guide, and `graphify update .` after code changes.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

When the user types `/graphify`, use the installed graphify skill or instructions before doing anything else.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- Dirty graphify-out/ files are expected after hooks or incremental updates; dirty graph files are not a reason to skip graphify. Only skip graphify if the task is about stale or incorrect graph output, or the user explicitly says not to use it.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
