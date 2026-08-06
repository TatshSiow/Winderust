# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com),
and this project adheres to [Semantic Versioning](https://semver.org).

## 0.5.0-alpha - 2026-08-06

### Added

- Add crash recovery protection.
- Add collapsible sidebar.
- Add enabled-feature counters in the sidebar.
- Add status indicators to navigation cards.
- Add visible-window detection.

### Changed

- Replace the previous mixed CPU control designs with separate CPU Sets (Soft) and Processor Affinity (Hard).
- Allow each CPU allocation rule to select presets or individual logical processors.
- Improve Process List context menus, grouped-process actions, suspend controls, and process-tree handling.
- Prevent the same application from using CPU Sets (Soft) and Processor Affinity (Hard) simultaneously.
- Update navigation icons for Winderust Features and Adaptive Engine.

### Fixed

- Remove `SeDebugPrivilege` and rely on standard Windows process access checks, prevent false positive Windows Defender blocking.
- Restore all Winderust-managed process states during normal shutdown. (Crash Recovery Protection)
- Protect critical Windows, shell, security, and infrastructure processes from unsafe suspend and stop actions.
- Apply grouped Process List actions to all eligible subprocesses.
- Reduce unnecessary retries against inaccessible or unsupported processes.

### Dependencies

- Update `serde` from 1.0.228 to 1.0.229.
- Update `serde_json` from 1.0.150 to 1.0.151.

## 0.4.0-alpha - 2026-07-31

### Added

- Add process search, grouped-process context menus, and user-session details to Process List
- Add startup update notifications
- Identify process rules by executable path so same-named applications remain distinct

### Changed

- Redesign Process List for better UI/UX
- Improve the process rule to be cross session/user instead of only current user session (changeable in settings)


### Fixed
- Run Workload Engine only when Adaptive Engine is enabled
- Preserve the window size and maximized state when restoring Winderust from the tray (Fixes Issue #11 )
- Stop retry spam for inaccessible or unsupported process-rule targets

## 0.3.0-alpha - 2026-07-24

### Added

- Add clickable Enabled Features entries on Home with feature-section icons.
- Add settings to pause dashboard metrics and process-list population.

### Changed

- Disable all automation rules by default on first run.
- Rename and clarify the Home enabled-features summary.
- Reorder Language and Appearance controls for a clearer setup flow.
- Restructure UI, automation, and feature modules for easier maintenance.

### Fixed

- Preserve the selected Adaptive Engine profile when toggling the feature.
- Harden settings loading, importing, cancellation, and validation behavior.
- Improve power-plan rule timing, selection, cleanup, and process-state restoration.
- Improve startup, input-hook, Windows-event, tray, update-check, and Action Log reliability.
- Complete localization for system dialogs, tray actions, and runtime statuses.


## 0.2.0-alpha - 2026-07-22

### Added

- Add automatic update checks, also able to switch between stable/pre-release channel.
- Add project, documentation, license, GitHub, and Discord links to About Page.

### Changed

- Settings file and log export now stays at the same folder with Winderust executable.
- Home automation rules now with a master switch indicator to improve fool-proof mechanism.
- Power-plan scheduler A/C toggle moved to Power Plan Control page.
- Shortened the README and speed up regular CI runs.

### Fixed

- Adaptive Engine operating-profile text truncation.

## 0.1.1-alpha - 2026-07-21

### Added

- Home and Adaptive Engine screenshots in the README.

### Changed

- Completed Traditional Chinese coverage for current UI locale keys, dynamic
  dashboard values, common runtime statuses, rule controls, and search fields.
- Search and rule-name placeholders now refresh immediately when the language
  changes.

### Fixed

- Home dashboard labels that displayed untranslated locale key names.

## 0.1.0-alpha - 2026-07-20

### Added

- Public contribution, security, and conduct policies.
- Windows continuous integration and draft release automation.
- Portable release build script with Windows SDK shader compiler discovery.

### Changed

- Project license clarified as GPL-3.0-only.
- Personal Graphify and agent tooling excluded from the public repository and
  release artifacts.
