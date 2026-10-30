# Release Checklist

1. Start from a clean, current `dev` branch. Update `Cargo.toml`, the root
   package entry in `Cargo.lock`, and the dated section in `CHANGELOG.md`.
2. Run formatting, strict Clippy, locked tests, architecture ownership and naming
   scans, optional harness checks, and the release build. Follow the commands in
   the [Wind UI guide](windui-port.md#cutover-checklist).
3. Smoke-test startup, tray behavior, settings persistence beside the
   executable, process-state restoration, and power-plan restoration on Windows.
   Include close/reopen with unsaved drafts, zero-window background operation,
   second-instance restoration, actual tray commands, Explorer restart, and
   asynchronous exit. Check the native UI in both locales, themes, and high DPI;
   a screenshot run alone does not verify interaction or restoration.
4. Confirm personal tooling, settings, logs, credentials, and signing material
   are absent from tracked files and release artifacts.
5. Commit and push `dev`, wait for CI, merge a `dev` to `main` pull request, and
   wait for CI on the final `main` commit. Keep `dev`.
6. Create and push an annotated version tag on that final `main` commit. The tag
   must match the Cargo version, for example `v0.2.0-alpha`.
7. Review the generated draft prerelease. Verify the ZIP contents, SHA-256
   checksum, and embedded executable version.
8. Sign the executable when a trusted signing process is available.
9. Publish only after final review and explicit approval, then confirm the
   prerelease and both assets are publicly available.
