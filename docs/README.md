# Documentation

## Current guides

- [Architecture](architecture.md): application, runtime and controller responsibilities.
- [Adaptive Engine implementation](adaptive-engine-implementation.md): feature design and implementation.
- [Adaptive Engine benchmarks](adaptive-engine-benchmark.md): measurement procedure, controls and limitations.
- [Iced implementation](iced-port.md): UI stack and port implementation notes.
- [Release checklist](release-checklist.md): release validation and packaging.
- [Contributing](../CONTRIBUTING.md): development and review requirements.

## Agent references

- [Agent memory index](../.agents/memory/README.md): task-specific instructions.
- [Runtime contracts](../.agents/memory/25-runtime-contracts.md): ownership, observations and restoration guarantees.
- [Windows API references](../.agents/memory/30-reference-library.md): implementation paths, official sources and undocumented assumptions.

## Historical evidence

- [Benchmark reports](../benchmark/README.md): measurements tied to hardware and test conditions.
- [Iced design integrity review](iced-design-integrity.md): original hierarchy findings and recorded corrections.
- [Iced visual parity audit](iced-visual-parity.md): findings from the reviewed UI state.

Historical findings describe their recorded baseline, not necessarily current behavior.
Keep original reports intact; record later fixes and their validation separately.
Current contracts belong in the maintained guides, not duplicated audit summaries.
