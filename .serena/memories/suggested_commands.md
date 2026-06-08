# Suggested commands

Windows PowerShell commands used in normal development:
- `cargo build --release` (standard build; outputs `target\release\powerleaf.exe`).
- `cargo build --release --target-dir target-next` (fallback when output exe is locked by a running instance).
- `cargo fmt`.
- `cargo test`.
- `cargo fmt && cargo test && cargo build --release --target-dir target-next` (local completion check sequence).

Avoid generic command notes; prefer the above Windows-native invocation paths.