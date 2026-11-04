# Release Checklist

1. Start from a clean, current `dev`. Choose a SemVer-compatible prerelease
   version such as `0.2.0-alpha`; do not create its tag yet. Confirm `gh` is
   installed and authenticated before starting the GitHub publishing steps.
2. Update the package version in `Cargo.toml`, refresh the root package entry in
   `Cargo.lock`, and add the dated release section to `CHANGELOG.md`. Verify that
   the lockfile contains no unrelated dependency changes. Build the changelog
   from the net changes between the previous release on `main` and the release
   candidate on `dev` (`git diff <previous-main-sha> dev`), rather than dev commit
   summaries alone. Include only changes from that released version and do not
   rewrite a published section.
3. Run the [development checks](development.md#project-basics) and
   `.\scripts\build_release.cmd`. Smoke-test startup, tray behavior, settings
   persistence beside the executable, process-state restoration, and power-plan
   restoration on Windows. Confirm personal tooling, settings, logs, credentials,
   and signing material are absent from tracked files and release artifacts. If UI
   automation is unavailable, obtain the user's explicit smoke-test confirmation
   before tagging.
4. Commit and push the release preparation to `dev`, wait for CI, then open and
   merge a `dev` to `main` pull request without deleting `dev`. Wait for CI on
   the final `main` commit.
5. Create and push an annotated tag on that final `main` commit. Never create
   the release tag on `dev`:

   ```powershell
   git tag -a v0.2.0-alpha -m "Winderust v0.2.0-alpha"
   git push origin v0.2.0-alpha
   ```

6. The `Draft Release` workflow validates the tag against Cargo metadata,
   repeats verification, builds the executable, and creates a draft prerelease
   with a ZIP and SHA-256 file. Release workflows are uncached and may take
   significantly longer than regular CI. Confirm the workflow succeeded and
   verify the checksum, ZIP contents, and embedded executable version, including
   any signing step and its resulting checksum. When a trusted signing process is
   available, sign the draft executable, rebuild the ZIP and checksum, and verify
   the final assets before publishing. Publish the draft only when the user
   explicitly requests publication or has already approved the complete release flow.
7. After publication, verify the release is a prerelease rather than a draft,
   confirm both assets are available, sync local `main` with `origin/main`, and
   keep `dev` as the integration branch for subsequent work.

Never move or recreate a published tag. If a draft workflow fails, fix the
cause on `main`; only replace an unpublished tag when no release was published
and the corrected commit must be the tagged source.
