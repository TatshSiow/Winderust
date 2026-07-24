# Contributing to Winderust

Thank you for helping improve Winderust. Bug reports, documentation fixes,
translations, testing, and focused code changes are all welcome.

Please follow the [Code of Conduct](CODE_OF_CONDUCT.md) in all project spaces.

## Before You Start

- Search existing issues before opening a new one.
- Open an issue before starting a large feature or architectural change so its
  scope and fit can be discussed.
- Keep each issue and pull request focused on one change.
- Use GitHub issues for actionable bugs and feature proposals, not private
  security reports or general Windows support.

Winderust is public pre-release software. Changes should improve the current
product rather than add compatibility layers for unpublished formats or retired
feature names.

## Report a Bug

Do not open a public issue for a suspected security vulnerability. Follow the
private reporting instructions in [SECURITY.md](SECURITY.md).

For other bugs, open a GitHub issue and include:

- The Winderust version or commit tested
- Your Windows version and processor architecture
- Steps to reproduce the problem
- What you expected and what happened instead
- Relevant logs, screenshots, or settings with sensitive information removed

## Suggest a Feature

Open a GitHub issue describing the user problem, the desired behavior, and why
it belongs in Winderust. For changes involving Windows internals, include the
relevant official Microsoft documentation when available.

Winderust favors focused features, conservative defaults, and native Windows
facilities over new dependencies or broad abstractions.

## Development Setup

Winderust is a Windows-only Rust application. Install:

- Stable Rust with the MSVC toolchain
- Visual Studio Build Tools with **Desktop development with C++**
- A Windows SDK that includes `fxc.exe`

Fork the repository, create a branch from `dev`, and make your change there.
Releasable changes are integrated into `dev` before promotion to `main`.

## Change Guidelines

- Keep code names, settings, tests, locale keys, scripts, and documentation
  aligned with the visible UI names.
- Preserve protected-process filtering, conservative defaults, process-state
  restoration, and clear failure handling.
- Prefer existing project helpers, the Rust standard library, and native
  Windows facilities before adding dependencies or abstractions.
- Avoid `unwrap` and `expect` on live process, filesystem, network,
  configuration, and Win32 paths.
- Document every unsafe block with an immediately preceding `// SAFETY:` comment
  that explains the relevant invariants.
- Add or update tests for behavior changes.
- Update English and Traditional Chinese locale entries together when visible
  UI text changes.
- Update the Windows API reference in `.agents/memory/30-reference-library.md`
  when changing a compatibility-sensitive Win32, NT, or WDK boundary.
- Do not commit credentials, signing keys, private email addresses, logs,
  exported settings, `.codex/`, `.agents/skills/`, or `graphify-out/`.

By contributing, you agree that your contribution is licensed under
GPL-3.0-only, the same license as this project.

## Verify Your Change

Run these checks before opening a pull request:

```powershell
git diff --check
cargo fmt -- --check
cargo clippy --locked --all-targets -- -D warnings -D unsafe-op-in-unsafe-fn
cargo test --locked
rg -n -i --glob '!target/**' --glob '!graphify-out/**' --glob '!.git/**' --glob '!.agents/**' 'PowerLeaf|Smart Saver|Smart Trim|serde.*alias|fill_missing_power_plan_mappings|Settings::power_plans' .
```

For release-related changes, also run:

```powershell
.\scripts\build_release.cmd
```

Test user-visible or Windows API behavior on a real Windows system and describe
the result in the pull request.

## Pull Requests

Target `dev` and include:

- A concise explanation of the problem and solution
- Any linked issue
- The verification commands run and their results
- Screenshots for visible UI changes
- Compatibility or restoration risks for Windows behavior changes

Maintainers may request changes to keep the implementation safe, focused, and
consistent with the current product. A pull request may be closed when its
scope does not fit the project, required checks fail, or requested changes are
left unresolved.
