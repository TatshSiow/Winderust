# Documentation

Winderust is a Windows-only Rust application using Iced 0.14, unmodified upstream
tiny-skia, TOML settings, and English/Traditional Chinese locales.
[AGENTS.md](../AGENTS.md) contains the product contract and required safety checks.

Work directly on `dev`. Create or switch branches only when the user explicitly
requests it; promote tested changes to `main` for an explicitly requested release.

## Guides

| Task | Read |
| --- | --- |
| Development, validation, dependency updates, and review | [Development guide](development.md) |
| Module ownership, source paths, and navigation | [Architecture](architecture.md) |
| UI, controls, layout, themes, and interaction | [Design](design.md) |
| Product boundaries and feature proposals | [Project scope](project-scope.md) |
| Feature policy, process safety, ownership, and restoration | [Runtime contracts](runtime-contracts.md) |
| Win32, NT, and WDK implementation evidence | [Windows API reference](windows-api-reference.md) |
| Release preparation, packaging, tagging, and publication | [Release checklist](release-checklist.md) |
| Adaptive Engine policy and presets | [Adaptive Engine implementation](adaptive-engine-implementation.md) |
| Performance measurement and limitations | [Adaptive Engine benchmarks](adaptive-engine-benchmark.md) |

Read the guides relevant to the task. Before changing runtime policy or Windows
mutations, read the runtime contracts and the corresponding API reference.
[Contributing](../CONTRIBUTING.md) covers external contributions;
[benchmark instructions](../benchmark/README.md) describe the reusable runners.
Machine-specific reports, raw results, and captures stay local in ignored paths.

## Maintaining documentation

- Update the guide that owns the decision instead of duplicating it across files.
- Keep current contracts, implementation paths, official references, and durable
  design decisions. Preserve undocumented API assumptions and runtime safeguards.
- Keep temporary task notes, command logs, test counts, completed migration reports,
  and superseded audits out of the active guides; Git history retains prior versions.
- Use descriptive filenames and relative links. Keep this index current when a
  guide is added, combined, or removed.
