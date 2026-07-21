# Release Checklist

1. Start from a clean, current `dev` branch. Update `Cargo.toml`, the root
   package entry in `Cargo.lock`, and the dated section in `CHANGELOG.md`.
2. Run formatting, strict Clippy, locked tests, the naming scan, and the release
   build.
3. Smoke-test startup, tray behavior, settings persistence beside the
   executable, process-state restoration, and power-plan restoration on Windows.
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
