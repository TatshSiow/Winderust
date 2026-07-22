# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com),
and this project adheres to [Semantic Versioning](https://semver.org).


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
