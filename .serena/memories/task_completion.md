# Task completion criteria

When a coding task is considered complete in this repo, run at least:
- `cargo fmt`
- `cargo test`
- `cargo build --release --target-dir target-next`

If the target binary is not locked, `cargo build --release` is also a normal verification path.

Keep command order tied to the changed scope and avoid skipping verification steps without explicit task-level reason.