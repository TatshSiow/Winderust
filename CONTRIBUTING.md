# Contributing to Winderust

Thanks for helping improve Winderust.

## Development Setup

Winderust is a Windows Rust application. Install:

- Stable Rust with the MSVC toolchain
- Visual Studio Build Tools with Desktop development with C++
- A Windows SDK that includes fxc.exe

Build and verify changes with:

    cargo fmt -- --check
    cargo clippy --locked --all-targets -- -D warnings
    cargo test --locked
    .\scripts\build_release.cmd

## Change Guidelines

- Open a focused issue or pull request and explain the user-visible behavior.
- Keep code names aligned with the corresponding UI labels.
- Preserve protected-process filtering, conservative defaults, process-state
  restoration, and failure handling.
- Add or update tests for behavior changes.
- Update English and Traditional Chinese locale entries together when UI copy
  changes.
- Do not commit personal development artifacts from .codex/, .agents/skills/,
  or graphify-out/.
- Never commit credentials, signing keys, private email addresses, logs, or
  exported user settings.

Run the naming scan documented in AGENTS.md before submitting broad renames.

By contributing, you agree that your contribution is licensed under
GPL-3.0-only, the same license as this project.
