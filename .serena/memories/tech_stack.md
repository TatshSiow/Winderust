# Tech stack

- Language: Rust (crate `winderust`, edition `2021`).
- Primary runtime: native Windows desktop app (Win32 + app host, no browser runtime).
- UI/tooling: `gpui` + `gpui-component`.
- Serialization/config: `serde` (+ derive), `toml`.
- Scheduler/time and windows bindings: `chrono`, `dirs`, `windows`, `windows-sys`, `raw-window-handle`.
- Internationalization support: `rust-i18n`.
- Dependency policy: keep behavior aligned to existing Rust/Win32 modules; avoid introducing broad framework migrations unless requested.
