# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com),
and this project adheres to [Semantic Versioning](https://semver.org).

## 0.6.1-alpha - 2026-08-20

### Added

- Add a Freeze/Thaw toggle to App Suspension.

### Changed

- Remove obsolete architecture diagnostic instrumentation and baseline tooling.
- Use the version tag alone as the generated draft release title.

### Fixed

- Restore running-process detection in App Suspension.
- Prevent Status panel cards from shifting when action messages change.
- Remove the redundant E-core no-SMT option.

## 0.6.0-alpha - 2026-08-19

### Added

- Add local executable browsing to custom process-rule pickers. (#17)
- Add custom Adaptive Engine presets alongside read-only built-in presets.
- Add separate A/C and battery presets to Advanced Power Plan Tuning.
- Add editable CPU allocation presets, exact logical-processor selection, and topology presets for all cores, P-cores, E-cores, and no-SMT variants.
- Add least-used processor selection across all processors, P-cores, or E-cores to Adaptive Engine CPU allocation.
- Add standardized feature status side panels with process activity, successful-action, and failed-action details.

### Changed

- Replace Workload Engine with the redesigned CPU Scheduler inside Adaptive Engine.
- Make CPU Pressure Restraint and Limit Background Processors independent.
- Redesign Adaptive Engine settings and presets into focused CPU Behaviour, Processor Power, Priority Control, and Custom Rules tabs.
- Standardize process rules around Focus Process, Visible Window, and Background tiers across priority, efficiency, and CPU controls.
- Redesign CPU Sets (Soft) and Processor Affinity (Hard) rules around shared presets while keeping per-tier processor selection independent.
- Redesign Advanced Power Plan Tuning around expandable A/C and battery cards and the shared save-confirmation flow.
- Unify collapsible navigation and feature side panels with matching slide animations, compact controls, and stable search layout.
- Improve sliders with hover and drag feedback, keyboard controls, and consistent value editing.
- Centralize Windows runtime control and restoration behind typed controllers, and persistent settings behind dedicated application services.

### Fixed

- Preserve Process List rule edits when the rule-details modal loses focus or closes.
- Close Process List overlays consistently when clicking outside them.
- Prevent the local executable browser from crashing the application.
- Prevent an unsuccessful administrator relaunch from silently closing Winderust.
- Revalidate exact process identities for process actions to avoid acting on reused process IDs or stale process-tree links.
- Improve clean-shutdown restoration across process controls, CPU allocation, App Suspension, Timer Resolution, and automatic power-plan switching, while hardening crash recovery for recoverable external state.
- Reduce repeated retries and Action Log spam from inaccessible targets and failed restoration attempts.

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
