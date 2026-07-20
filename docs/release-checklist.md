# Release Checklist

1. Update Cargo.toml and CHANGELOG.md.
2. Run formatting, strict Clippy, locked tests, and the release build.
3. Smoke-test startup, tray behavior, settings persistence, process-state
   restoration, and power-plan restoration on Windows.
4. Confirm personal tooling, settings, logs, credentials, and signing material
   are absent from tracked files and release artifacts.
5. Create and push an annotated version tag matching Cargo.toml, such as v0.1.0.
6. Review the generated draft prerelease, its ZIP contents, and SHA-256 file.
7. Sign the executable when a trusted signing process is available.
8. Publish the GitHub release manually after final review.
